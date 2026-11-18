//! The store's drawn kinds on the GPU: meshes and textures uploaded once and
//! shared, materials and shader codes decoded once. BC textures go to the GPU as stored.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak};

use serde::Deserialize;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::gpu::{BufDesc, BufKind, Buffer, Gpu, Subresource, TexDesc, TexKind, TexViews, Texture};
use crate::{Error, Result};

/// A cache's lock (a panic on another thread leaves the maps whole: each change is one insert).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The store, its open packs, and every asset loaded so far by (kind, path). Shared by the frame thread, the
/// loader's (`Renderer::warm_job`) and the picture loaders (`Assets::texture_soon`): a load runs outside the caches'
/// locks, the first of two racing loads is kept.
pub struct Assets {
    shared: Arc<Shared>,
}

/// What [`Assets`] and its picture loaders share.
struct Shared {
    store: mvs_store::Store,
    gpu: Gpu,
    meshes_pack: std::sync::Arc<mvs_store::Pack>,
    textures_pack: std::sync::Arc<mvs_store::Pack>,
    materials_pack: std::sync::Arc<mvs_store::Pack>,
    shaders_pack: std::sync::Arc<mvs_store::Pack>,
    meshes: Mutex<HashMap<String, Kept<Mesh>>>,
    textures: Mutex<HashMap<String, Kept<Texture>>>,
    /// The mark a look-up stamps its mesh or texture with (`Assets::trim` moves it on).
    mark: AtomicU64,
    /// What `Assets::trim` keeps of the meshes and textures no draw holds, bytes.
    budget: AtomicUsize,
    materials: Mutex<HashMap<String, Arc<mvs_material::Material>>>,
    codes: Mutex<HashMap<mvs_material::Sha1, Arc<mvs_material::ShaderCode>>>,
    global: OnceLock<mvs_material::GlobalShaders>,
    /// The white textures by TextureType: 2D, cube, array, volume.
    white: Mutex<[Option<Arc<Texture>>; 4]>,
    far: OnceLock<Arc<Texture>>,
    /// The material parameter collections set at run time (`Renderer::set_collection`): object path -> buffer rows.
    /// An instance made after takes these rows in place of its entry's.
    collections: Mutex<HashMap<String, Vec<[u32; 4]>>>,
    /// The engine brush files (the image kind) made, by path: few and small, kept.
    images: Mutex<HashMap<String, Arc<Texture>>>,
    loads: Loads,
}

/// A picture loader's job.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Job {
    /// A texture kind entry read, decoded and made.
    Texture(String),
    /// An image kind entry (an engine Slate brush .png) decoded and made.
    Image(String),
    /// A material's entry read, its textures made and its Slate pass's shaders made (`Assets::material_soon`).
    Material(String),
}

/// The picture loaders' queue and what they finished.
#[derive(Default)]
struct Loads {
    state: Mutex<LoadState>,
    /// A job queued, or the loaders told to stop.
    work: Condvar,
    /// A job finished.
    done: Condvar,
}

#[derive(Default)]
struct LoadState {
    queue: VecDeque<Job>,
    /// The jobs a loader runs now.
    running: HashSet<Job>,
    /// The material jobs finished.
    warmed: HashSet<String>,
    /// The jobs that failed, with why, each until its error is given to an ask (the ask after it tries again).
    failed: HashMap<Job, String>,
    workers: usize,
    stop: bool,
}

/// The picture loaders started at most: the store's reads and zstd and the device's creates of a screen's pictures
/// run on these, off the frame thread.
fn loaders_wanted() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get() / 4).clamp(2, 4)
}

/// A mesh or texture made, its bytes (its entry's) and the trim's mark of its last look-up.
#[derive(Debug)]
struct Kept<T> {
    value: Arc<T>,
    bytes: usize,
    used: u64,
}

/// What [`Assets::trim`] let go and kept, bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Trimmed {
    pub freed: usize,
    pub kept: usize,
}

/// The anisotropy cap of an anisotropic sampler at the texture quality level in force (r.MaxAnisotropy): for the
/// samplers made outside the renderer (the UI's element batches).
pub fn max_anisotropy() -> u8 {
    crate::texture_quality::max_anisotropy()
}

/// The meshes and textures `Assets::trim` keeps past what is drawn, for this adapter: a quarter of its own memory,
/// between 256 MB and 1 GB (a match draws 300 to 600 MB of them; a shared-memory adapter takes the least).
pub fn default_budget(video_memory: u64) -> usize {
    usize::try_from(video_memory / 4).unwrap_or(usize::MAX).clamp(256 << 20, 1 << 30)
}

impl std::fmt::Debug for Assets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = &self.shared;
        f.debug_struct("Assets")
            .field("store", &s.store.dir())
            .field("meshes", &lock(&s.meshes).len())
            .field("textures", &lock(&s.textures).len())
            .field("materials", &lock(&s.materials).len())
            .field("codes", &lock(&s.codes).len())
            .finish()
    }
}

/// The assets loaded so far (Assets::loaded).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    pub meshes: Vec<String>,
    pub textures: Vec<String>,
    pub materials: Vec<String>,
    pub codes: usize,
}

/// A render section of a LOD.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshSection {
    /// The material slot it draws with.
    pub material: u16,
    pub first_index: u32,
    pub triangles: u32,
    /// The section's bCastShadow (the mesh kind's castShadow).
    pub cast_shadow: bool,
}

/// A LOD's buffers. A static mesh's are FLocalVertexFactory's: the position stream and the manual-fetch SRVs
/// (LocalVertexFactory.ush's VertexFetch_* buffers). A skeletal mesh's are TGPUSkinVertexFactory's vertex streams.
#[derive(Clone, Debug)]
pub struct LodBuffers {
    /// Positions, float3 (ATTRIBUTE0 of both factories).
    pub position: Buffer,
    pub index: Buffer,
    /// Static: VertexFetch_TexCoordBuffer, the UVs interleaved per vertex (vertex x NumTexCoords + k).
    pub tex_coords: Option<Buffer>,
    pub num_tex_coords: u32,
    /// Static: VertexFetch_PackedTangentsBuffer, TangentX and TangentZ interleaved (vertex x 2, + 1).
    pub tangents: Option<Buffer>,
    /// Static: VertexFetch_ColorComponentsBuffer (absent: the white one, ColorIndexMask 0).
    pub color: Option<Buffer>,
    /// Static: the LocalVF uniform buffer: VertexFetch_Parameters (ColorIndexMask, NumTexCoords,
    /// LightMapCoordinateIndex, the base vertex) and (PreSkinBaseVertexIndex, LODLightmapDataIndex).
    pub local_vf: Option<Buffer>,
    /// Skeletal: the vertex streams by GPUSkin attribute: TangentX (1), TangentZ (2), BlendIndices (3),
    /// BlendWeights (4), TexCoord0 / 1 (5, 6), BlendIndicesExtra (14), BlendWeightsExtra (15), with their formats
    /// and strides.
    pub skin: Vec<(u32, Buffer, DXGI_FORMAT, u32)>,
}

#[derive(Debug)]
pub struct MeshLod {
    pub sections: Vec<MeshSection>,
    pub vertices: u32,
    pub buffers: LodBuffers,
}

/// A static or skeletal mesh on the GPU.
#[derive(Debug)]
pub struct Mesh {
    pub path: String,
    pub lods: Vec<MeshLod>,
    /// Per material slot: the mesh's own material's object path.
    pub materials: Vec<Arc<str>>,
    /// The reference skeleton's bones; 0 for a static mesh.
    pub bones: u32,
    /// Eight influences a vertex (the second joints / weights stream), not four.
    pub extra_influences: bool,
    /// Local bounds: origin and box extent, cm.
    pub bounds: [[f32; 3]; 2],
    /// A static mesh's LightMapCoordinateIndex.
    pub lightmap_uv: u32,
    /// A skeletal mesh's morph targets (UMorphTarget), in the mesh's MorphTargets order.
    pub morphs: Vec<MorphTarget>,
}

/// A morph target: per LOD, its sparse deltas (FMorphTargetDelta) on the CPU, which a pose's weights sum into the
/// morph vertex buffer.
#[derive(Debug)]
pub struct MorphTarget {
    pub name: String,
    pub lods: Vec<MorphLod>,
}

/// One LOD's deltas: the vertex each moves, its position delta (cm) and its TangentZ delta.
#[derive(Debug, Default)]
pub struct MorphLod {
    pub vertex: Vec<u32>,
    pub position: Vec<[f32; 3]>,
    pub tangent_z: Vec<[f32; 3]>,
}

#[derive(Deserialize)]
struct MorphRaw {
    name: String,
    lods: Vec<MorphLodRaw>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MorphLodRaw {
    count: usize,
    vertex: BodyRef,
    position: BodyRef,
    tangent_z: BodyRef,
}

#[derive(Deserialize)]
struct BodyRef {
    offset: usize,
    length: usize,
}

#[derive(Deserialize)]
struct BodyStream {
    offset: usize,
    length: usize,
    format: String,
}

#[derive(Deserialize)]
struct IndexRef {
    offset: usize,
    length: usize,
    format: String,
    count: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SectionRaw {
    slot: u16,
    first: u32,
    count: u32,
    disabled: bool,
    cast_shadow: bool,
}

#[derive(Deserialize)]
struct LodRaw {
    vertices: u32,
    streams: HashMap<String, BodyStream>,
    index: IndexRef,
    sections: Vec<SectionRaw>,
}

#[derive(Deserialize)]
struct BoundsRaw {
    origin: [f32; 3],
    extent: [f32; 3],
}

#[derive(Deserialize)]
struct SlotRaw {
    material: Option<String>,
}

#[derive(Deserialize)]
struct SkeletonRaw {
    bones: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MeshRaw {
    bounds: BoundsRaw,
    slots: Vec<SlotRaw>,
    #[serde(rename = "lightmapUV")]
    lightmap_uv: Option<u32>,
    skeleton: Option<SkeletonRaw>,
    lods: Vec<LodRaw>,
    #[serde(default)]
    morphs: Vec<MorphRaw>,
}

fn bad(what: &str, why: String) -> Error {
    Error::Format { what: what.to_string(), why }
}

/// A stream format's element bytes and its DXGI format.
fn stream_format(f: &str) -> Option<(usize, DXGI_FORMAT)> {
    Some(match f {
        "f32x3" => (12, DXGI_FORMAT_R32G32B32_FLOAT),
        "f32x2" => (8, DXGI_FORMAT_R32G32_FLOAT),
        "f16x2" => (4, DXGI_FORMAT_R16G16_FLOAT),
        "snorm8x4" => (4, DXGI_FORMAT_R8G8B8A8_SNORM),
        "snorm16x4" => (8, DXGI_FORMAT_R16G16B16A16_SNORM),
        "unorm8x4" => (4, DXGI_FORMAT_R8G8B8A8_UNORM),
        "u8x4" => (4, DXGI_FORMAT_R8G8B8A8_UINT),
        "u16x4" => (8, DXGI_FORMAT_R16G16B16A16_UINT),
        _ => return None,
    })
}

/// A texture format's (the MVTX header's code: DXT1, DXT3, DXT5, BC4, BC5, BC7, G8, RGBA8, FloatRGBA; BC6H, the
/// texture kind's own) DXGI format (sRGB where the texture is), its block size in texels and bytes a block.
fn texture_format(code: u8, srgb: bool) -> Option<(DXGI_FORMAT, u32, u32)> {
    let pick = |lin: DXGI_FORMAT, s: DXGI_FORMAT| if srgb { s } else { lin };
    Some(match code {
        0 => (pick(DXGI_FORMAT_BC1_UNORM, DXGI_FORMAT_BC1_UNORM_SRGB), 4, 8),
        1 => (pick(DXGI_FORMAT_BC2_UNORM, DXGI_FORMAT_BC2_UNORM_SRGB), 4, 16),
        2 => (pick(DXGI_FORMAT_BC3_UNORM, DXGI_FORMAT_BC3_UNORM_SRGB), 4, 16),
        3 => (DXGI_FORMAT_BC4_UNORM, 4, 8),
        4 => (DXGI_FORMAT_BC5_UNORM, 4, 16),
        5 => (pick(DXGI_FORMAT_BC7_UNORM, DXGI_FORMAT_BC7_UNORM_SRGB), 4, 16),
        // PF_G8: R8 on D3D11, no sRGB form.
        6 => (DXGI_FORMAT_R8_UNORM, 1, 1),
        // B8G8R8A8 stored swapped to R8G8B8A8.
        7 => (pick(DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB), 1, 4),
        8 => (DXGI_FORMAT_R16G16B16A16_FLOAT, 1, 8),
        // PF_BC6H kept as its blocks: D3D11 samples them. No sRGB form (an HDR format, linear). Unsigned half floats:
        // the build's D3D11 RHI maps PF_BC6H to DXGI_FORMAT_BC6H_UF16 (FD3D11DynamicRHI ctor, 14185ae2a).
        9 => (DXGI_FORMAT_BC6H_UF16, 4, 16),
        _ => return None,
    })
}

/// Decodes an MVTX entry onto the GPU.
pub fn upload_texture(gpu: &Gpu, b: &[u8], what: &str) -> Result<Texture> {
    let u16at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]) as u32;
    let u32at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
    if b.len() < 18 || &b[..4] != b"MVTX" {
        return Err(bad(what, "not an MVTX entry".into()));
    }
    if u16at(4) != 1 {
        return Err(bad(what, format!("texture entry version {}", u16at(4))));
    }
    let (code, flags) = (b[6], b[7]);
    let srgb = flags & 1 != 0;
    let cube = flags & 2 != 0;
    let (format, block, block_bytes) =
        texture_format(code, srgb).ok_or_else(|| bad(what, format!("texture format code {code}")))?;
    let (w, h, depth) = (u16at(8), u16at(10), u16at(12).max(1));
    let n = b[14] as usize;
    if b.len() < 18 + 12 * n || n == 0 {
        return Err(bad(what, format!("{n} mips in {} bytes", b.len())));
    }
    let kind = if cube {
        TexKind::Cube
    } else if depth > 1 {
        TexKind::D3
    } else {
        TexKind::D2
    };
    let faces = if cube { 6 } else { 1 };
    let mut mips = Vec::with_capacity(n);
    for i in 0..n {
        let o = 18 + 12 * i;
        let (mw, mh, off, len) = (u16at(o), u16at(o + 2), u32at(o + 4), u32at(o + 8));
        let data = b.get(off..off + len).ok_or_else(|| bad(what, format!("mip {i} at {off}+{len}")))?;
        let row = mw.div_ceil(block) * block_bytes;
        let rows = mh.div_ceil(block);
        mips.push((data, row, row * rows));
    }
    let mut subs = Vec::with_capacity(n * faces);
    for f in 0..faces {
        for (i, &(data, row, slice)) in mips.iter().enumerate() {
            let s = if cube {
                let face = data.len() / 6;
                &data[f * face..(f + 1) * face]
            } else {
                data
            };
            let need = if kind == TexKind::D3 { slice as usize * (depth >> i).max(1) as usize } else { slice as usize };
            if s.len() < need {
                return Err(bad(what, format!("mip {i}: {} bytes, {need} wanted", s.len())));
            }
            subs.push(Subresource { data: s, row_pitch: row, slice_pitch: slice });
        }
    }
    let desc = TexDesc {
        kind,
        size: [w, h, if cube { 6 } else { depth }],
        mips: n as u32,
        format,
        views: TexViews { srv: true, ..Default::default() },
    };
    gpu.texture(&desc, &subs)
}

/// An `image` kind entry (an engine Slate brush .png) as an sRGB RGBA8 texture.
// Unverified: Slate's resource manager makes a brush file's texture sRGB (FSlateRHIResourceManager, 4.26).
fn image_texture(gpu: &Gpu, bytes: &[u8], path: &str) -> Result<Texture> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().map_err(|e| bad(path, e.to_string()))?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or_else(|| bad(path, "an image too large".into()))?];
    let info = reader.next_frame(&mut buf).map_err(|e| bad(path, e.to_string()))?;
    let px = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => px.chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(bad(path, "an indexed image after the expansion".into())),
    };
    let desc = TexDesc {
        kind: TexKind::D2,
        size: [info.width, info.height, 1],
        mips: 1,
        format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        views: TexViews { srv: true, ..Default::default() },
    };
    gpu.texture(&desc, &[Subresource { data: &rgba, row_pitch: info.width * 4, slice_pitch: 0 }])
}

/// One of the port's own icons (`port_icons`) as an sRGB RGBA8 texture with its mips.
fn port_icon_texture(gpu: &Gpu, mips: &[Vec<u8>]) -> Result<Texture> {
    let side = crate::port_icons::SIZE;
    let desc = TexDesc {
        kind: TexKind::D2,
        size: [side, side, 1],
        mips: mips.len() as u32,
        format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        views: TexViews { srv: true, ..Default::default() },
    };
    let subs: Vec<Subresource<'_>> = mips
        .iter()
        .enumerate()
        .map(|(i, m)| Subresource { data: m, row_pitch: (side >> i) * 4, slice_pitch: 0 })
        .collect();
    gpu.texture(&desc, &subs)
}

/// Uploads a mesh entry: its header and every LOD's buffers.
fn upload_mesh(gpu: &Gpu, path: &str, bytes: &[u8]) -> Result<Mesh> {
    let jb = mvs_material::jsonbin::JsonBin::split(b"MVM1", bytes, path)?;
    let h: MeshRaw = jb.parse(path)?;
    let skeletal = h.skeleton.is_some();
    let mut lods = Vec::with_capacity(h.lods.len());
    let mut extra = false;
    for (li, l) in h.lods.iter().enumerate() {
        let what = |s: &str| format!("{path} LOD {li} {s}");
        let n = l.vertices as usize;
        let stream = |name: &str| -> Result<Option<(&[u8], usize, DXGI_FORMAT)>> {
            let Some(s) = l.streams.get(name) else { return Ok(None) };
            let (size, f) =
                stream_format(&s.format).ok_or_else(|| bad(&what(name), format!("stream format {}", s.format)))?;
            let data =
                jb.body.get(s.offset..s.offset + s.length).ok_or_else(|| bad(&what(name), "past the body".into()))?;
            if data.len() != size * n {
                return Err(bad(&what(name), format!("{} bytes for {n} vertices of {size}", data.len())));
            }
            Ok(Some((data, size, f)))
        };
        let (pos, _, _) = stream("position")?.ok_or_else(|| bad(&what("position"), "no stream".into()))?;
        let position =
            gpu.buffer(&BufDesc { kind: BufKind::Vertex, size: pos.len() as u32, dynamic: false }, Some(pos))?;
        let index_bytes = jb
            .body
            .get(l.index.offset..l.index.offset + l.index.length)
            .ok_or_else(|| bad(&what("index"), "past the body".into()))?;
        let index_kind = match l.index.format.as_str() {
            "u16" => BufKind::Index16,
            "u32" => BufKind::Index32,
            f => return Err(bad(&what("index"), format!("format {f}"))),
        };
        let isize = if index_kind == BufKind::Index16 { 2 } else { 4 };
        if index_bytes.len() != l.index.count as usize * isize {
            return Err(bad(&what("index"), format!("{} bytes for {} indices", index_bytes.len(), l.index.count)));
        }
        let index = gpu
            .buffer(&BufDesc { kind: index_kind, size: index_bytes.len() as u32, dynamic: false }, Some(index_bytes))?;
        let mut uvs = Vec::new();
        while let Some(s) = stream(&format!("uv{}", uvs.len()))? {
            uvs.push(s);
        }
        let tx = stream("tangentX")?.ok_or_else(|| bad(&what("tangentX"), "no stream".into()))?;
        let tz = stream("tangentZ")?.ok_or_else(|| bad(&what("tangentZ"), "no stream".into()))?;
        let mut buffers = LodBuffers {
            position,
            index,
            tex_coords: None,
            num_tex_coords: uvs.len() as u32,
            tangents: None,
            color: None,
            local_vf: None,
            skin: Vec::new(),
        };
        if skeletal {
            let mut vb = |attr: u32, s: (&[u8], usize, DXGI_FORMAT)| -> Result<()> {
                let b =
                    gpu.buffer(&BufDesc { kind: BufKind::Vertex, size: s.0.len() as u32, dynamic: false }, Some(s.0))?;
                buffers.skin.push((attr, b, s.2, s.1 as u32));
                Ok(())
            };
            vb(1, tx)?;
            vb(2, tz)?;
            let j0 = stream("joints0")?.ok_or_else(|| bad(&what("joints0"), "no stream".into()))?;
            let w0 = stream("weights0")?.ok_or_else(|| bad(&what("weights0"), "no stream".into()))?;
            vb(3, j0)?;
            vb(4, w0)?;
            for (k, uv) in uvs.iter().take(2).enumerate() {
                vb(5 + k as u32, *uv)?;
            }
            if let (Some(j1), Some(w1)) = (stream("joints1")?, stream("weights1")?) {
                vb(14, j1)?;
                vb(15, w1)?;
                extra = true;
            }
            // TGPUSkinVertexFactory's ColorComponent (ATTRIBUTE13): the LOD's colour vertex buffer where it has one
            // (FSkeletalMeshObjectGPUSkin InitGPUSkinVertexFactoryComponents, 4.26 source), else the null colour
            // buffer (set_streams). The store's colours are RGBA (mvs-import mesh.rs color_rgba), read as such.
            if let Some(c) = stream("color")? {
                vb(13, c)?;
            }
        } else {
            // VertexFetch_TexCoordBuffer: the UVs interleaved, all in the first stream's format (f32 if they differ).
            if !uvs.is_empty() {
                let same = uvs.iter().all(|u| u.2 == uvs[0].2);
                let (size, f) = if same { (uvs[0].1, uvs[0].2) } else { (8, DXGI_FORMAT_R32G32_FLOAT) };
                let mut out = Vec::with_capacity(n * size * uvs.len());
                for v in 0..n {
                    for u in &uvs {
                        let e = &u.0[v * u.1..(v + 1) * u.1];
                        if same || u.1 == 8 {
                            out.extend_from_slice(e);
                        } else {
                            for c in 0..2 {
                                let half = u16::from_le_bytes([e[2 * c], e[2 * c + 1]]);
                                out.extend_from_slice(&crate::image::f16_to_f32(half).to_le_bytes());
                            }
                        }
                    }
                }
                buffers.tex_coords = Some(gpu.buffer(
                    &BufDesc { kind: BufKind::Typed { format: f }, size: out.len() as u32, dynamic: false },
                    Some(&out),
                )?);
            }
            // VertexFetch_PackedTangentsBuffer: TangentX, TangentZ per vertex.
            if tx.2 != tz.2 {
                return Err(bad(&what("tangents"), "TangentX and TangentZ in different formats".into()));
            }
            let mut t = Vec::with_capacity(n * tx.1 * 2);
            for v in 0..n {
                t.extend_from_slice(&tx.0[v * tx.1..(v + 1) * tx.1]);
                t.extend_from_slice(&tz.0[v * tz.1..(v + 1) * tz.1]);
            }
            buffers.tangents = Some(gpu.buffer(
                &BufDesc { kind: BufKind::Typed { format: tx.2 }, size: t.len() as u32, dynamic: false },
                Some(&t),
            )?);
            let color = stream("color")?;
            let mask: u32 = if color.is_some() { u32::MAX } else { 0 };
            let white = [255u8; 4];
            let c = color.map_or(&white[..], |c| c.0);
            buffers.color = Some(gpu.buffer(
                &BufDesc {
                    kind: BufKind::Typed { format: DXGI_FORMAT_R8G8B8A8_UNORM },
                    size: c.len() as u32,
                    dynamic: false,
                },
                Some(c),
            )?);
            // LocalVF: VertexFetch_Parameters (ColorIndexMask, NumTexCoords, LightMapCoordinateIndex, the base
            // vertex 0), then (PreSkinBaseVertexIndex 0, LODLightmapDataIndex 0).
            // Unverified: the members' places are read from the base pass vertex shaders' code (cb1[0].x-w,
            // cb1[1].y), not from the exe's LocalVF member table.
            let rows: [[u32; 4]; 2] = [[mask, uvs.len() as u32, h.lightmap_uv.unwrap_or(0), 0], [0; 4]];
            buffers.local_vf = Some(gpu.buffer(
                &BufDesc { kind: BufKind::Constant, size: 32, dynamic: false },
                Some(crate::gpu::rows_bytes(&rows)),
            )?);
        }
        let sections = l
            .sections
            .iter()
            .map(|s| MeshSection {
                material: s.slot,
                first_index: s.first,
                triangles: if s.disabled { 0 } else { s.count / 3 },
                cast_shadow: s.cast_shadow,
            })
            .collect();
        lods.push(MeshLod { sections, vertices: l.vertices, buffers });
    }
    // the morph targets' sparse deltas (mesh kind: per LOD a u32 vertex, f32x3 position and f32x3 TangentZ part)
    let mut morphs = Vec::with_capacity(h.morphs.len());
    for m in &h.morphs {
        let mut out = MorphTarget { name: m.name.clone(), lods: Vec::with_capacity(m.lods.len()) };
        for (li, l) in m.lods.iter().enumerate() {
            let what = format!("{path} morph {} LOD {li}", m.name);
            let part = |r: &BodyRef, size: usize| -> Result<&[u8]> {
                let b = jb.body.get(r.offset..r.offset + r.length).ok_or_else(|| bad(&what, "past the body".into()))?;
                if b.len() != l.count * size {
                    return Err(bad(&what, format!("{} bytes for {} deltas of {size}", b.len(), l.count)));
                }
                Ok(b)
            };
            let words = |b: &[u8]| b.as_chunks::<4>().0.iter().map(|w| u32::from_le_bytes(*w)).collect::<Vec<_>>();
            let vecs = |b: &[u8]| {
                b.as_chunks::<12>()
                    .0
                    .iter()
                    .map(|c| {
                        [0, 1, 2].map(|k| f32::from_le_bytes([c[4 * k], c[4 * k + 1], c[4 * k + 2], c[4 * k + 3]]))
                    })
                    .collect::<Vec<_>>()
            };
            let vertex = words(part(&l.vertex, 4)?);
            let n = h.lods.get(li).map_or(0, |x| x.vertices);
            if let Some(v) = vertex.iter().find(|&&v| v >= n) {
                return Err(bad(&what, format!("vertex {v} of {n}")));
            }
            out.lods.push(MorphLod {
                vertex,
                position: vecs(part(&l.position, 12)?),
                tangent_z: vecs(part(&l.tangent_z, 12)?),
            });
        }
        morphs.push(out);
    }
    Ok(Mesh {
        path: path.to_string(),
        lods,
        materials: h.slots.iter().map(|s| Arc::from(s.material.as_deref().unwrap_or(""))).collect(),
        bones: h.skeleton.as_ref().map_or(0, |s| s.bones.len() as u32),
        extra_influences: extra,
        bounds: [h.bounds.origin, h.bounds.extent],
        lightmap_uv: h.lightmap_uv.unwrap_or(0),
        morphs,
    })
}

/// A static mesh's LOD 0 on the CPU, for geometry laid from it (the effects' spline meshes).
#[derive(Clone, Debug)]
pub(crate) struct MeshSource {
    pub positions: Vec<[f32; 3]>,
    /// UV0 as floats (empty without a UV stream).
    pub uv0: Vec<[f32; 2]>,
    /// TangentX and TangentZ (the normal, its W the binormal sign) as floats.
    pub tangent_x: Vec<[f32; 4]>,
    pub tangent_z: Vec<[f32; 4]>,
    /// The colour stream (empty without one).
    pub colors: Vec<[u8; 4]>,
    /// The whole index, in the cooked order.
    pub indices: Vec<u32>,
    pub sections: usize,
    pub extent: [f32; 3],
}

/// Reads an MVM1 entry's LOD 0 streams the CPU needs.
pub(crate) fn mesh_source(path: &str, bytes: &[u8]) -> Result<MeshSource> {
    let jb = mvs_material::jsonbin::JsonBin::split(b"MVM1", bytes, path)?;
    let h: MeshRaw = jb.parse(path)?;
    let l = h.lods.first().ok_or_else(|| bad(path, "no LOD".into()))?;
    let n = l.vertices as usize;
    let body = |o: usize, len: usize, what: &str| {
        jb.body.get(o..o + len).ok_or_else(|| bad(&format!("{path} {what}"), "past the body".into()))
    };
    let p = l.streams.get("position").ok_or_else(|| bad(path, "no position stream".into()))?;
    if p.format != "f32x3" || p.length != 12 * n {
        return Err(bad(path, format!("position stream {} of {} bytes", p.format, p.length)));
    }
    let f32s = |b: &[u8]| -> Vec<f32> { b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect() };
    let positions = f32s(body(p.offset, p.length, "position")?).as_chunks::<3>().0.to_vec();
    let uv0 = match l.streams.get("uv0") {
        None => Vec::new(),
        Some(u) => {
            let b = body(u.offset, u.length, "uv0")?;
            match (u.format.as_str(), u.length) {
                ("f32x2", len) if len == 8 * n => f32s(b).as_chunks::<2>().0.to_vec(),
                ("f16x2", len) if len == 4 * n => b
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| {
                        let h = |k: usize| crate::image::f16_to_f32(u16::from_le_bytes([c[k], c[k + 1]]));
                        [h(0), h(2)]
                    })
                    .collect(),
                (f, len) => return Err(bad(path, format!("uv0 stream {f} of {len} bytes"))),
            }
        }
    };
    let snorm = |name: &str| -> Result<Vec<[f32; 4]>> {
        let t = l.streams.get(name).ok_or_else(|| bad(path, format!("no {name} stream")))?;
        let b = body(t.offset, t.length, name)?;
        let unit = |v: f32, max: f32| (v / max).max(-1.0);
        Ok(match (t.format.as_str(), t.length) {
            ("snorm8x4", len) if len == 4 * n => {
                b.as_chunks::<4>().0.iter().map(|c| c.map(|v| unit(f32::from(v.cast_signed()), 127.0))).collect()
            }
            ("snorm16x4", len) if len == 8 * n => b
                .as_chunks::<8>()
                .0
                .iter()
                .map(|c| {
                    std::array::from_fn(|k| unit(f32::from(i16::from_le_bytes([c[2 * k], c[2 * k + 1]])), 32767.0))
                })
                .collect(),
            (f, len) => return Err(bad(path, format!("{name} stream {f} of {len} bytes"))),
        })
    };
    let (tangent_x, tangent_z) = (snorm("tangentX")?, snorm("tangentZ")?);
    let colors = match l.streams.get("color") {
        None => Vec::new(),
        Some(c) if c.format == "unorm8x4" && c.length == 4 * n => {
            body(c.offset, c.length, "color")?.as_chunks::<4>().0.to_vec()
        }
        Some(c) => return Err(bad(path, format!("color stream {} of {} bytes", c.format, c.length))),
    };
    let ib = body(l.index.offset, l.index.length, "index")?;
    let indices = match l.index.format.as_str() {
        "u16" => ib.as_chunks::<2>().0.iter().map(|c| u32::from(u16::from_le_bytes(*c))).collect(),
        "u32" => ib.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect(),
        f => return Err(bad(path, format!("index format {f}"))),
    };
    Ok(MeshSource {
        positions,
        uv0,
        tangent_x,
        tangent_z,
        colors,
        indices,
        sections: l.sections.len(),
        extent: h.bounds.extent,
    })
}

impl Assets {
    pub fn open(store: mvs_store::Store, gpu: &Gpu) -> Result<Assets> {
        Ok(Assets { shared: Arc::new(Shared::open(store, gpu)?) })
    }

    /// A collection's rows set at run time: the instances made from now on take them.
    pub(crate) fn set_collection(&self, path: &str, rows: &[[u32; 4]]) {
        self.shared.set_collection(path, rows);
    }

    /// The rows a collection was set to at run time, if it was.
    pub(crate) fn collection(&self, path: &str) -> Option<Vec<[u32; 4]>> {
        self.shared.collection(path)
    }

    pub fn store(&self) -> &mvs_store::Store {
        &self.shared.store
    }

    /// The paths of the meshes, textures and materials loaded so far, sorted, and the shader codes' count (what a
    /// warm-up's check watches).
    pub fn loaded(&self) -> Loaded {
        self.shared.loaded()
    }

    pub fn mesh(&self, path: &str) -> Result<Arc<Mesh>> {
        self.shared.mesh(path)
    }

    /// A texture kind entry on the GPU, made now if no loader has it (one a loader is making is waited for).
    pub fn texture(&self, path: &str) -> Result<Arc<Texture>> {
        if let Some(t) = self.shared.texture_kept(path) {
            return Ok(t);
        }
        self.take_or_wait(&Job::Texture(path.to_string()));
        self.shared.texture(path)
    }

    /// Lets go the meshes and textures nobody else holds and nothing has looked up since the trim before, least
    /// recently looked up first, until what is kept is at most the budget (a fighter or stage no longer drawn: what
    /// the match being installed warmed was looked up since and stays). Then moves the mark on.
    pub fn trim(&self) -> Trimmed {
        self.shared.trim()
    }

    /// What `trim` keeps past what is drawn, bytes ([`default_budget`] at the open).
    pub fn set_budget(&self, bytes: usize) {
        self.shared.budget.store(bytes, Ordering::Relaxed);
    }

    /// The bytes `trim` keeps past what is drawn (`set_budget`).
    pub fn budget(&self) -> usize {
        self.shared.budget.load(Ordering::Relaxed)
    }

    pub fn material(&self, path: &str) -> Result<Arc<mvs_material::Material>> {
        self.shared.material(path)
    }

    /// The global shader cache's rows.
    pub fn global_shaders(&self) -> &mvs_material::GlobalShaders {
        self.shared.global_shaders()
    }

    /// The shader kind's code `sha`.
    pub fn code(&self, sha: mvs_material::Sha1) -> Result<Arc<mvs_material::ShaderCode>> {
        self.shared.code(sha)
    }

    /// The engine's white texture of a parameter type (GWhiteTexture, GWhiteTextureCube, the white volume and
    /// array): one texel of 1.
    pub fn white(&self, ty: mvs_material::material::TextureType) -> Arc<Texture> {
        self.shared.white(ty)
    }

    /// The stand-in of a render target the game fills (a fighter material's LocalPlayerSceneDepth before the
    /// priority capture is drawn): one RGBA16F texel of depth 60000 cm, far (a render
    /// target).
    pub fn far_depth(&self) -> Arc<Texture> {
        self.shared.far_depth()
    }

    /// An image kind entry (an engine Slate brush .png) or one of the port's own icons (`port_icons`) as an sRGB
    /// RGBA8 texture, made now if no loader has it.
    pub fn image(&self, path: &str) -> Result<Arc<Texture>> {
        if let Some(t) = lock(&self.shared.images).get(path) {
            return Ok(t.clone());
        }
        self.take_or_wait(&Job::Image(path.to_string()));
        self.shared.image(path)
    }

    /// A texture kind entry if it is on the GPU; else None and a loader makes it (the frame thread draws on without
    /// it). A load that failed is its error.
    // Deviation: the build loads a widget's brush texture with its widget (the UI's textures are cooked into its
    // package); a picture here is read and made off the frame thread and may show a frame or two after its widget.
    pub fn texture_soon(&self, path: &str) -> Result<Option<Arc<Texture>>> {
        if let Some(t) = self.shared.texture_kept(path) {
            return Ok(Some(t));
        }
        self.ask(Job::Texture(path.to_string())).map(|()| None)
    }

    /// `texture_soon` of an image kind entry.
    pub fn image_soon(&self, path: &str) -> Result<Option<Arc<Texture>>> {
        if let Some(t) = lock(&self.shared.images).get(path) {
            return Ok(Some(t.clone()));
        }
        self.ask(Job::Image(path.to_string())).map(|()| None)
    }

    /// Whether the material at `path` is read, its textures made and its Slate pass's shaders made (a UI material's
    /// instance is then made on the frame thread without a store read); else false and a loader makes them.
    pub fn material_soon(&self, path: &str) -> Result<bool> {
        if lock(&self.shared.loads.state).warmed.contains(path) {
            return Ok(true);
        }
        self.ask(Job::Material(path.to_string())).map(|()| false)
    }

    /// What `material_soon` has a loader do, on this thread (a match's load warming its HUD's UI materials): the
    /// material read, its textures made and its Slate passes' shaders made; then `material_soon` of it is true.
    pub fn warm_ui_material(&self, path: &str) -> Result<()> {
        if lock(&self.shared.loads.state).warmed.contains(path) {
            return Ok(());
        }
        self.shared.warm_material(path)?;
        lock(&self.shared.loads.state).warmed.insert(path.to_string());
        Ok(())
    }

    /// `job` queued unless it is queued or running; a failed job's error, once: the next ask queues it again (a
    /// failure may pass, E_OUTOFMEMORY from a create while a match loads). Starts a loader while fewer run than
    /// wanted.
    fn ask(&self, job: Job) -> Result<()> {
        let loads = &self.shared.loads;
        let mut st = lock(&loads.state);
        if let Some(why) = st.failed.remove(&job) {
            return Err(bad(job.path(), why));
        }
        if st.running.contains(&job) || st.queue.contains(&job) {
            return Ok(());
        }
        st.queue.push_back(job);
        if st.workers < loaders_wanted() {
            st.workers += 1;
            let weak = Arc::downgrade(&self.shared);
            let spawned = std::thread::Builder::new().name("mvs-pictures".into()).spawn(move || loader(&weak));
            if spawned.is_err() {
                st.workers -= 1;
            }
        }
        loads.work.notify_one();
        Ok(())
    }

    /// A job taken off the queue (the caller makes it now), or waited for while a loader runs it.
    fn take_or_wait(&self, job: &Job) {
        let loads = &self.shared.loads;
        let mut st = lock(&loads.state);
        if let Some(k) = st.queue.iter().position(|j| j == job) {
            st.queue.remove(k);
            return;
        }
        while st.running.contains(job) {
            st = loads.done.wait(st).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
}

impl Drop for Assets {
    fn drop(&mut self) {
        lock(&self.shared.loads.state).stop = true;
        self.shared.loads.work.notify_all();
    }
}

impl Job {
    fn path(&self) -> &str {
        match self {
            Job::Texture(p) | Job::Image(p) | Job::Material(p) => p,
        }
    }
}

/// A picture loader: runs the queue's jobs until the assets are let go.
fn loader(weak: &Weak<Shared>) {
    loop {
        let Some(s) = weak.upgrade() else { return };
        let job = {
            let mut st = lock(&s.loads.state);
            loop {
                if st.stop {
                    return;
                }
                if let Some(j) = st.queue.pop_front() {
                    st.running.insert(j.clone());
                    break j;
                }
                st = s.loads.work.wait(st).unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        let made = match &job {
            Job::Texture(p) => s.texture(p).map(drop),
            Job::Image(p) => s.image(p).map(drop),
            Job::Material(p) => s.warm_material(p),
        };
        let mut st = lock(&s.loads.state);
        st.running.remove(&job);
        match made {
            Ok(()) => {
                if let Job::Material(p) = &job {
                    st.warmed.insert(p.clone());
                }
            }
            Err(e) => {
                st.failed.insert(job, e.to_string());
            }
        }
        drop(st);
        s.loads.done.notify_all();
    }
}

impl Shared {
    fn open(store: mvs_store::Store, gpu: &Gpu) -> Result<Shared> {
        Ok(Shared {
            meshes_pack: store.kind("mesh")?,
            textures_pack: store.kind("texture")?,
            materials_pack: store.kind("material")?,
            shaders_pack: store.kind("shader")?,
            store,
            gpu: gpu.share(),
            meshes: Mutex::default(),
            textures: Mutex::default(),
            mark: AtomicU64::new(0),
            budget: AtomicUsize::new(default_budget(gpu.video_memory)),
            materials: Mutex::default(),
            codes: Mutex::default(),
            global: OnceLock::new(),
            white: Mutex::default(),
            far: OnceLock::new(),
            collections: Mutex::default(),
            images: Mutex::default(),
            loads: Loads::default(),
        })
    }

    fn set_collection(&self, path: &str, rows: &[[u32; 4]]) {
        lock(&self.collections).insert(path.to_string(), rows.to_vec());
    }

    fn collection(&self, path: &str) -> Option<Vec<[u32; 4]>> {
        lock(&self.collections).get(path).cloned()
    }

    fn loaded(&self) -> Loaded {
        fn paths<T>(m: &HashMap<String, T>) -> Vec<String> {
            let mut v: Vec<String> = m.keys().cloned().collect();
            v.sort_unstable();
            v
        }
        Loaded {
            meshes: paths(&lock(&self.meshes)),
            textures: paths(&lock(&self.textures)),
            materials: paths(&lock(&self.materials)),
            codes: lock(&self.codes).len(),
        }
    }

    fn mesh(&self, path: &str) -> Result<Arc<Mesh>> {
        let used = self.mark.load(Ordering::Relaxed);
        if let Some(m) = lock(&self.meshes).get_mut(path) {
            m.used = used;
            return Ok(m.value.clone());
        }
        let b = self.meshes_pack.bytes(path)?;
        let value = Arc::new(upload_mesh(&self.gpu, path, &b)?);
        let made = Kept { value, bytes: b.len(), used };
        Ok(lock(&self.meshes).entry(path.to_string()).or_insert(made).value.clone())
    }

    /// A texture kept, stamped with the mark.
    fn texture_kept(&self, path: &str) -> Option<Arc<Texture>> {
        let used = self.mark.load(Ordering::Relaxed);
        let mut kept = lock(&self.textures);
        let t = kept.get_mut(path)?;
        t.used = used;
        Some(t.value.clone())
    }

    fn texture(&self, path: &str) -> Result<Arc<Texture>> {
        if let Some(t) = self.texture_kept(path) {
            return Ok(t);
        }
        let used = self.mark.load(Ordering::Relaxed);
        let b = self.textures_pack.bytes(path)?;
        let value = Arc::new(upload_texture(&self.gpu, &b, path)?);
        let made = Kept { value, bytes: b.len(), used };
        Ok(lock(&self.textures).entry(path.to_string()).or_insert(made).value.clone())
    }

    /// Lets go the meshes and textures nobody else holds and nothing has looked up since the trim before, least
    /// recently looked up first, until what is kept is at most the budget (a fighter or stage no longer drawn: what
    /// the match being installed warmed was looked up since and stays). Then moves the mark on.
    fn trim(&self) -> Trimmed {
        let mark = self.mark.fetch_add(1, Ordering::Relaxed);
        let budget = self.budget.load(Ordering::Relaxed);
        let (mut meshes, mut textures) = (lock(&self.meshes), lock(&self.textures));
        let mut kept: usize =
            meshes.values().map(|k| k.bytes).sum::<usize>() + textures.values().map(|k| k.bytes).sum::<usize>();
        // (oldest first: (mark, is a texture, path, bytes))
        let mut free: Vec<(u64, bool, String, usize)> = meshes
            .iter()
            .filter(|(_, k)| k.used < mark && Arc::strong_count(&k.value) == 1)
            .map(|(p, k)| (k.used, false, p.clone(), k.bytes))
            .chain(
                textures
                    .iter()
                    .filter(|(_, k)| k.used < mark && Arc::strong_count(&k.value) == 1)
                    .map(|(p, k)| (k.used, true, p.clone(), k.bytes)),
            )
            .collect();
        free.sort_unstable();
        let mut freed = 0;
        for (_, texture, path, bytes) in free {
            if kept <= budget {
                break;
            }
            if texture {
                textures.remove(&path);
            } else {
                meshes.remove(&path);
            }
            kept -= bytes;
            freed += bytes;
        }
        Trimmed { freed, kept }
    }

    fn image(&self, path: &str) -> Result<Arc<Texture>> {
        if let Some(t) = lock(&self.images).get(path) {
            return Ok(t.clone());
        }
        let t = match crate::port_icons::mips(path) {
            Some(mips) => Arc::new(port_icon_texture(&self.gpu, &mips)?),
            None => Arc::new(image_texture(&self.gpu, &self.store.kind("image")?.bytes(path)?, path)?),
        };
        Ok(lock(&self.images).entry(path.to_string()).or_insert(t).clone())
    }

    /// A UI material's entry read, its textures made and its Slate passes' shaders (Default and GrayscaleFont, the
    /// ones it has) made: what its instance's first draw would read from the store.
    fn warm_material(&self, path: &str) -> Result<()> {
        let m = self.material(path)?;
        for t in &m.uniforms.textures {
            if let mvs_material::material::TextureBind::Texture { path, .. } = &t.texture {
                self.texture(path)?;
            }
        }
        if m.state.domain == "MD_UI" {
            for ps in ["TSlateMaterialShaderPSDefault", "TSlateMaterialShaderPSGrayscaleFont"] {
                let Some((_, rows)) = m.shaders.pass(None, None, &[("TSlateMaterialShaderVSfalse", 0), (ps, 0)]) else {
                    continue;
                };
                // (a code that fails is left to the draw's own look-up and its error)
                for r in rows.iter() {
                    let sha = m.shaders.code(r);
                    if let Ok(c) = self.code(sha) {
                        let _ = self.gpu.shader_by(sha, &c);
                    }
                }
            }
        }
        Ok(())
    }

    fn material(&self, path: &str) -> Result<Arc<mvs_material::Material>> {
        if let Some(m) = lock(&self.materials).get(path) {
            return Ok(m.clone());
        }
        let m = match mvs_material::read_entry(&self.materials_pack, path)? {
            mvs_material::Entry::Material(m) => Arc::new(*m),
            mvs_material::Entry::Global(_) => return Err(bad(path, "the global shader cache, not a material".into())),
        };
        Ok(lock(&self.materials).entry(path.to_string()).or_insert(m).clone())
    }

    /// The global shader cache's rows.
    fn global_shaders(&self) -> &mvs_material::GlobalShaders {
        self.global.get_or_init(|| {
            match mvs_material::read_entry(&self.materials_pack, mvs_material::GLOBAL_SHADER_CACHE) {
                Ok(mvs_material::Entry::Global(g)) => g,
                Ok(_) => panic!("{}: a material, not the global rows", mvs_material::GLOBAL_SHADER_CACHE),
                Err(e) => panic!("{}: {e}", mvs_material::GLOBAL_SHADER_CACHE),
            }
        })
    }

    /// The shader kind's code `sha`.
    fn code(&self, sha: mvs_material::Sha1) -> Result<Arc<mvs_material::ShaderCode>> {
        if let Some(c) = lock(&self.codes).get(&sha) {
            return Ok(c.clone());
        }
        let c = Arc::new(mvs_material::read_code(&self.shaders_pack, sha)?);
        Ok(lock(&self.codes).entry(sha).or_insert(c).clone())
    }

    /// The engine's white texture of a parameter type (GWhiteTexture, GWhiteTextureCube, the white volume and
    /// array): one texel of 1.
    fn white(&self, ty: mvs_material::material::TextureType) -> Arc<Texture> {
        use mvs_material::material::TextureType as T;
        let k = match ty {
            T::Texture2d => 0,
            T::Cube => 1,
            T::Array => 2,
            T::Volume => 3,
        };
        if let Some(t) = &lock(&self.white)[k] {
            return t.clone();
        }
        let texel = [255u8; 4];
        let (kind, layers) = match ty {
            T::Texture2d => (TexKind::D2, 1),
            T::Cube => (TexKind::Cube, 6),
            T::Array => (TexKind::D2Array, 1),
            T::Volume => (TexKind::D3, 1),
        };
        let subs = vec![Subresource { data: &texel, row_pitch: 4, slice_pitch: 4 }; layers];
        let desc = TexDesc {
            kind,
            size: [1, 1, layers as u32],
            mips: 1,
            format: DXGI_FORMAT_R8G8B8A8_UNORM,
            views: TexViews { srv: true, ..Default::default() },
        };
        let t = Arc::new(self.gpu.texture(&desc, &subs).unwrap_or_else(|e| panic!("white texture: {e}")));
        lock(&self.white)[k] = Some(t.clone());
        t
    }

    /// The stand-in of a render target the game fills (a fighter material's LocalPlayerSceneDepth before the
    /// priority capture is drawn): one RGBA16F texel of depth 60000 cm, far (a render
    /// target).
    fn far_depth(&self) -> Arc<Texture> {
        self.far
            .get_or_init(|| {
                let far = 0x7b53u16.to_le_bytes(); // 60000 as a half float
                let texel = [far[0], far[1], far[0], far[1], far[0], far[1], far[0], far[1]];
                let desc = TexDesc {
                    kind: TexKind::D2,
                    size: [1, 1, 1],
                    mips: 1,
                    format: DXGI_FORMAT_R16G16B16A16_FLOAT,
                    views: TexViews { srv: true, ..Default::default() },
                };
                let sub = [Subresource { data: &texel, row_pitch: 8, slice_pitch: 8 }];
                Arc::new(self.gpu.texture(&desc, &sub).unwrap_or_else(|e| panic!("far depth texture: {e}")))
            })
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trim lets go a texture nobody holds once a trim has passed since its look-up, not one still held, and a
    /// look-up after makes it again.
    #[test]
    fn a_trim_lets_go_what_nobody_holds() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no developer store: skipped");
            return;
        };
        let Ok(gpu) = Gpu::new(None, false) else {
            eprintln!("no device: skipped");
            return;
        };
        let assets = Assets::open(store.clone(), &gpu).unwrap();
        assets.set_budget(0);
        let paths: Vec<String> = store.kind("texture").unwrap().paths().take(2).map(str::to_owned).collect();
        let held = assets.texture(&paths[0]).unwrap();
        drop(assets.texture(&paths[1]).unwrap());
        // (both looked up since the last trim: kept)
        assert_eq!(assets.trim().freed, 0);
        let t = assets.trim();
        assert!(t.freed > 0);
        assert_eq!(assets.loaded().textures, vec![paths[0].clone()]);
        assert!(t.kept > 0);
        drop(held);
        assert!(assets.texture(&paths[1]).is_ok());
        assert_eq!(assets.loaded().textures.len(), 2);
    }

    /// A texture asked of the picture loaders is not on the GPU at the ask; a blocking look-up after (waiting for the
    /// loader or taking the job off its queue) gives the one the loaders keep.
    #[test]
    fn a_texture_asked_of_the_loaders_is_the_one_looked_up() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no developer store: skipped");
            return;
        };
        let Ok(gpu) = Gpu::new(None, false) else {
            eprintln!("no device: skipped");
            return;
        };
        let assets = Assets::open(store.clone(), &gpu).unwrap();
        let paths: Vec<String> = store.kind("texture").unwrap().paths().take(16).map(str::to_owned).collect();
        for p in &paths {
            assert!(assets.texture_soon(p).unwrap().is_none());
        }
        for p in &paths {
            let t = assets.texture(p).unwrap();
            assert!(Arc::ptr_eq(&t, &assets.texture_soon(p).unwrap().unwrap()));
        }
        assert_eq!(assets.loaded().textures.len(), paths.len());
    }

    #[test]
    fn a_failed_load_is_its_error_once_then_asked_again() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no developer store: skipped");
            return;
        };
        let Ok(gpu) = Gpu::new(None, false) else {
            eprintln!("no device: skipped");
            return;
        };
        let assets = Assets::open(store, &gpu).unwrap();
        let path = "/Game/NoSuchTexture.NoSuchTexture";
        let job = Job::Texture(path.to_string());
        let wait_failed = || {
            let loads = &assets.shared.loads;
            let mut st = lock(&loads.state);
            while !st.failed.contains_key(&job) {
                st = loads.done.wait(st).unwrap();
            }
        };
        assert!(assets.texture_soon(path).unwrap().is_none());
        wait_failed();
        assert!(assets.texture_soon(path).is_err());
        // the error given, the next ask queues the job again
        assert!(assets.texture_soon(path).unwrap().is_none());
        wait_failed();
        assert!(assets.texture_soon(path).is_err());
    }
}
