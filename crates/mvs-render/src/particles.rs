//! The particle emitters' draws from mvs-vfx with the build's particle vertex factories (FParticleSpriteVertexFactory,
//! FMeshParticleVertexFactory, FParticleBeamTrailVertexFactory) and their materials' cooked translucent base pass
//! shaders, in the player's sort, and the distortion pass over SceneColor.
//!
//! A frame's draws are uploaded into one dynamic vertex buffer in `prepare`; `draw` runs them in the player's order,
//! the BLEND_Modulate materials first (a stable partition), each material's blend state and the vertex factory's
//! own uniform buffer (SpriteVF, MeshParticleVF, BeamTrailVF) from the emitter's DrawParams.

use std::collections::HashMap;
use std::sync::Arc;

use mvs_material::{Frequency, ShaderCode};
use mvs_vfx::draw::{DrawParams, EmitterDraw, EmitterKind};
use mvs_vfx::fill::{MESH_STRIDE, RIBBON_STRIDE, SPRITE_STRIDE};
use windows::Win32::Graphics::Direct3D::{
    D3D_PRIMITIVE_TOPOLOGY, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::assets::{Assets, Mesh};
use crate::draw::{PassInput, StageTables, Streams};
use crate::gpu::{
    BlendKey, BlendTarget, BufDesc, BufKind, Buffer, DepthKey, Gpu, RasterKey, SamplerKey, ShaderId, StateId, StateKey,
    TexDesc, TexKind, TexViews, Texture, dx, rows_bytes,
};
use crate::material::{
    LightMapPolicy, LightMapScene, LightMapSurface, MaterialInstance, MaterialResource, Mobility, TextureSource,
    VertexFactory, base_depth, light_map_policy, view_sampler,
};
use crate::renderer::{Frame, matrix_of};
use crate::view::{PRIMITIVE_ROWS, PrimitiveParams, PrimitiveRows, SceneBuffers, VIEW_RESOURCES, identity};
use crate::{Error, Result};

/// The constant buffer the loose $Globals of the sprite vertex factory reads (zeros: NumCutoutVerticesPerFrame 0, the
/// corners come from the texcoord stream).
const ZERO_ROWS: usize = 256;

/// DistortionPass's (FDistortionPassUniformParameters, metadata 144f9ed40): SceneTextures' SceneDepthTexture and its
/// sampler; DistortionParams is row 7.
const DP_SCENE_DEPTH: u16 = 1;
const DP_SCENE_DEPTH_SAMPLER: u16 = 12;
const DP_ROWS: usize = 8;

/// Where a register of a particle draw's shader is filled from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Src {
    View(u16),
    Primitive,
    Material(u16),
    Collection(u16),
    /// The vertex factory's own buffer: SpriteVF, MeshParticleVF, BeamTrailVF.
    Factory,
    TranslucentBasePass(u16),
    DistortionPass(u16),
    /// The loose ($Globals) buffer.
    Globals,
    None,
}

fn buffer_src(name: &str, register: u32) -> Src {
    match name {
        "View" => Src::View(u16::MAX),
        "Primitive" => Src::Primitive,
        "Material" => Src::Material(u16::MAX),
        "SpriteVF" | "MeshParticleVF" | "BeamTrailVF" => Src::Factory,
        "TranslucentBasePass" => Src::TranslucentBasePass(u16::MAX),
        "DistortionPass" => Src::DistortionPass(u16::MAX),
        "" if register == 0 => Src::Globals,
        n => match n.strip_prefix("MaterialCollection").and_then(|k| k.parse().ok()) {
            Some(k) => Src::Collection(k),
            None => Src::None,
        },
    }
}

/// One stage's (register, source) for constant buffers, SRVs and samplers.
#[derive(Clone, Debug, Default)]
struct Binds {
    cbs: Vec<(u32, Src)>,
    srvs: Vec<(u32, Src)>,
    samplers: Vec<(u32, Src)>,
}

fn binds_of(code: &ShaderCode) -> Result<Binds> {
    let mut b = Binds::default();
    for (reg, name) in code.uniform_buffer_names.iter().enumerate() {
        b.cbs.push((reg as u32, buffer_src(name, reg as u32)));
    }
    if code.uniform_buffer_names.is_empty() && code.global_uniform_buffer_used {
        b.cbs.push((0, Src::Globals));
    }
    let resource = |slot: mvs_material::shader::TableSlot| -> Src {
        let name = code.uniform_buffer_names.get(slot.buffer as usize).map_or("", String::as_str);
        match buffer_src(name, slot.buffer as u32) {
            Src::View(_) => Src::View(slot.resource),
            Src::Material(_) => Src::Material(slot.resource),
            Src::TranslucentBasePass(_) => Src::TranslucentBasePass(slot.resource),
            Src::DistortionPass(_) => Src::DistortionPass(slot.resource),
            _ => Src::None,
        }
    };
    let rt = &code.resource_table;
    for map in [&rt.texture_map, &rt.srv_map] {
        for s in rt.slots(map)? {
            b.srvs.push((s.register as u32, resource(s)));
        }
    }
    for s in rt.slots(&rt.sampler_map)? {
        b.samplers.push((s.register as u32, resource(s)));
    }
    Ok(b)
}

/// A material's shaders and state for one vertex factory and pass.
#[derive(Debug)]
struct Pass {
    vs: ShaderId,
    ps: Option<ShaderId>,
    vs_binds: Binds,
    ps_binds: Binds,
    blend: StateId,
    depth: StateId,
}

/// How a material blends (EBlendMode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Blend {
    Opaque,
    Masked,
    Translucent,
    Additive,
    Modulate,
    AlphaComposite,
}

impl Blend {
    fn of(name: &str) -> Option<Blend> {
        Some(match name {
            "BLEND_Opaque" => Blend::Opaque,
            "BLEND_Masked" => Blend::Masked,
            "BLEND_Translucent" => Blend::Translucent,
            "BLEND_Additive" => Blend::Additive,
            "BLEND_Modulate" => Blend::Modulate,
            "BLEND_AlphaComposite" => Blend::AlphaComposite,
            _ => return None,
        })
    }

    fn opaque(self) -> bool {
        matches!(self, Blend::Opaque | Blend::Masked)
    }
}

fn target(color: (D3D11_BLEND, D3D11_BLEND), alpha: (D3D11_BLEND, D3D11_BLEND), mask: u8) -> BlendTarget {
    BlendTarget {
        color_op: D3D11_BLEND_OP_ADD.0 as u8,
        color_src: color.0.0 as u8,
        color_dst: color.1.0 as u8,
        alpha_op: D3D11_BLEND_OP_ADD.0 as u8,
        alpha_src: alpha.0.0 as u8,
        alpha_dst: alpha.1.0 as u8,
        write_mask: mask,
    }
}

/// The translucent pass's blend state of a blend mode (SetTranslucentRenderState): Translucent
/// SrcAlpha / InvSrcAlpha, Additive One / One, AlphaComposite One / InvSrcAlpha, each with alpha Zero /
/// InvSrcAlpha (the target's alpha the transmittance the separate translucency composite reads); Modulate DestColor /
/// Zero on RGB only.
// Unverified: AlphaComposite's state is 4.26's (no material of the port's reach has it).
fn blend_key(b: Blend) -> BlendKey {
    use D3D11_BLEND_DEST_COLOR as DC;
    use D3D11_BLEND_INV_SRC_ALPHA as ISA;
    use D3D11_BLEND_ONE as ONE;
    use D3D11_BLEND_SRC_ALPHA as SA;
    use D3D11_BLEND_ZERO as ZERO;
    let all = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    let t = match b {
        Blend::Opaque | Blend::Masked => return BlendKey::OPAQUE,
        Blend::Translucent => target((SA, ISA), (ZERO, ISA), all),
        Blend::Additive => target((ONE, ONE), (ZERO, ISA), all),
        Blend::AlphaComposite => target((ONE, ISA), (ZERO, ISA), all),
        Blend::Modulate => {
            let rgb = (D3D11_COLOR_WRITE_ENABLE_RED.0
                | D3D11_COLOR_WRITE_ENABLE_GREEN.0
                | D3D11_COLOR_WRITE_ENABLE_BLUE.0) as u8;
            target((DC, ZERO), (ZERO, ONE), rgb)
        }
    };
    let mut k = BlendKey::OPAQUE;
    k.targets[0] = t;
    k
}

/// The distortion accumulation's blend: BO_Add, BF_One, BF_One on colour and alpha (the distortion mesh processor's
/// state).
fn distortion_blend() -> BlendKey {
    let mut k = BlendKey::OPAQUE;
    k.targets[0] = target((D3D11_BLEND_ONE, D3D11_BLEND_ONE), (D3D11_BLEND_ONE, D3D11_BLEND_ONE), 0x0f);
    k
}

fn raster(cull: D3D11_CULL_MODE) -> RasterKey {
    RasterKey {
        fill: D3D11_FILL_SOLID.0 as u8,
        cull: cull.0 as u8,
        front_ccw: true,
        depth_bias: 0,
        slope_scaled_depth_bias: 0,
    }
}

/// A particle draw's cull mode (UE's rasterizer, FrontCounterClockwise true). A sprite culls its back only when its
/// material is one-sided, it has a fixed axis lock and is not velocity aligned (fill.rs DrawParams.axis_locked;
/// the camera-facing, velocity and rotate-lock bases always face the camera); a mesh culls by its material's sides,
/// the other side when mirrored; a beam or trail strip draws both faces.
// Unverified: the beam and trail strips drawn unculled for a one-sided material (DoubleSide), not read in
// the exe.
fn cull_of(kind: &Kind, two_sided: bool, p: &DrawParams) -> D3D11_CULL_MODE {
    match kind {
        Kind::Sprite if !two_sided && p.axis_locked && !p.velocity_aligned => D3D11_CULL_BACK,
        Kind::Mesh { .. } if !two_sided => {
            if p.mirrored {
                D3D11_CULL_FRONT
            } else {
                D3D11_CULL_BACK
            }
        }
        _ => D3D11_CULL_NONE,
    }
}

struct Inputs<'a> {
    m: &'a mvs_material::Material,
    scalars: &'a [f64],
    vectors: &'a [[f64; 4]],
}

impl mvs_material::PreshaderInputs for Inputs<'_> {
    fn scalar(&self, k: usize) -> f64 {
        self.scalars[k]
    }
    fn vector(&self, k: usize) -> [f64; 4] {
        self.vectors[k]
    }
    fn texture_size(&self, k: usize) -> Option<[f64; 4]> {
        self.m.uniforms.texture_sizes.as_ref()?.get(k).copied().flatten()
    }
}

/// A particle material: its instance (resources, the Material and collection buffers), its parameters, the scalars
/// its Material buffer was last written with, and its passes per vertex factory (None: no such shaders, a material
/// without distortion).
struct ParticleMaterial {
    inst: MaterialInstance,
    blend: Blend,
    scalars: Vec<f64>,
    vectors: Vec<[f64; 4]>,
    written: Option<Vec<(Arc<str>, f32)>>,
    passes: HashMap<(VertexFactory, bool), Option<Pass>>,
}

impl std::fmt::Debug for ParticleMaterial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParticleMaterial").field("path", &self.inst.material.path).field("blend", &self.blend).finish()
    }
}

impl ParticleMaterial {
    fn new(gpu: &Gpu, assets: &Assets, path: &str) -> Result<ParticleMaterial> {
        Ok(ParticleMaterial::try_new(gpu, assets, path).unwrap_or_else(|e| panic!("unported: {e}")))
    }

    /// `new`, what it has no port of as an error.
    fn try_new(gpu: &Gpu, assets: &Assets, path: &str) -> std::result::Result<ParticleMaterial, String> {
        if path.is_empty() {
            return Err("a particle emitter without a material".into());
        }
        let inst =
            MaterialInstance::new(gpu, assets, path).map_err(|e| format!("the particle material {path}: {e}"))?;
        let m = &inst.material;
        let blend = Blend::of(&m.state.blend)
            .ok_or_else(|| format!("the particle material {path}'s blend mode {}", m.state.blend))?;
        Ok(ParticleMaterial {
            blend,
            scalars: m.uniforms.scalars.iter().map(|s| s.value).collect(),
            vectors: m.uniforms.vectors.iter().map(|v| v.value).collect(),
            written: None,
            passes: HashMap::new(),
            inst,
        })
    }

    /// The Material buffer's rows for the component's dynamic instance scalars, written when they differ from the
    /// last write.
    fn refresh(&mut self, gpu: &Gpu, set: &[(Arc<str>, f32)]) -> Result<()> {
        if self.written.as_deref() == Some(set) {
            return Ok(());
        }
        if let Some(b) = &self.inst.buffer {
            let m = &self.inst.material;
            let mut scalars = self.scalars.clone();
            for (name, v) in set {
                if let Some(k) = m.uniforms.scalars.iter().position(|s| *s.name == **name) {
                    scalars[k] = *v as f64;
                }
            }
            let rows = m.uniform_rows(&Inputs { m, scalars: &scalars, vectors: &self.vectors })?;
            let mut out = vec![[0u32; 4]; (b.desc.size / 16) as usize];
            for (o, r) in out.iter_mut().zip(&rows) {
                *o = r.map(|x| (x as f32).to_bits());
            }
            gpu.write_rows(b, &out);
        }
        self.written = Some(set.to_vec());
        Ok(())
    }

    /// The pass's shaders and state, made on first use.
    fn pass(&mut self, gpu: &Gpu, assets: &Assets, vf: VertexFactory, distortion: bool) -> Result<Option<&Pass>> {
        let key = (vf, distortion);
        if !self.passes.contains_key(&key) {
            let p = self.make_pass(gpu, assets, vf, distortion)?;
            self.passes.insert(key, p);
        }
        Ok(self.passes[&key].as_ref())
    }

    /// The base pass's light-map policy of the material: none for
    /// unlit, a lit one's the volumetric lightmap's (a movable primitive in the stage's scene). The Skylight variant:
    /// a translucency's for TLM_Surface* and a subsurface sprite; an opaque or masked lit one's always, as the opaque
    /// meshes' base pass takes it (material.rs make_pass, the mesh
    /// context).
    // Unverified: LightMapScene::STAGE for every view (a menu's player has no stage).
    fn policy(&self, vf: VertexFactory) -> (LightMapPolicy, bool) {
        let st = &self.inst.material.state;
        let lit = st.shading_model != "MSM_Unlit";
        let s = LightMapSurface {
            lit,
            translucent: !self.blend.opaque(),
            light_cache: false,
            cast_volumetric_translucent_shadow: false,
            texture_light_map: false,
            hq_interaction: false,
            texture_shadow_map: false,
            mobility: Mobility::Movable,
            preview_or_forced_volumetric: false,
            indirect_cache: None,
        };
        let surface = st.translucency_lighting_mode.starts_with("TLM_Surface");
        let sss = st.shading_model == "MSM_Subsurface" && vf == VertexFactory::Sprite;
        let policy = light_map_policy(&s, &LightMapScene::STAGE);
        let sky = if self.blend.opaque() { lit } else { lit && (surface || sss) };
        (policy, sky)
    }

    fn make_pass(&self, gpu: &Gpu, assets: &Assets, vf: VertexFactory, distortion: bool) -> Result<Option<Pass>> {
        let m = &self.inst.material;
        let what = || format!("{} {} distortion {distortion}", m.path, vf.type_name());
        let (types, pipeline): (Vec<String>, Option<String>) = if distortion {
            (vec!["FDistortionMeshVS".into(), "FDistortionMeshPS".into()], None)
        } else {
            let (policy, sky) = self.policy(vf);
            let p = policy.type_suffix();
            let sky = if sky { "Skylight" } else { "" };
            // Unverified: the plain vertex shader, not its AtmosphericFog variant (the scene's atmospheric fog is not
            // read; the translucent pass's fog is the stand-in TranslucentBasePass's).
            (vec![format!("TBasePassVS{p}"), format!("TBasePassPS{p}{sky}")], Some(format!("BasePassPipeline{p}{sky}")))
        };
        let t: Vec<(&str, i32)> = types.iter().map(|t| (t.as_str(), 0)).collect();
        let Some((_, rows)) = m.shaders.pass(Some(vf.type_name()), pipeline.as_deref(), &t) else {
            if distortion {
                return Ok(None);
            }
            return Err(Error::Format { what: what(), why: format!("no {types:?} in the shader map") });
        };
        let codes: Vec<(mvs_material::Sha1, Arc<ShaderCode>)> = rows
            .iter()
            .map(|r| assets.code(m.shaders.code(r)).map(|c| (m.shaders.code(r), c)))
            .collect::<Result<_>>()?;
        if codes[0].1.frequency != Frequency::Vertex || codes[1].1.frequency != Frequency::Pixel {
            return Err(Error::Format { what: what(), why: "the pass is not a vertex and a pixel shader".into() });
        }
        let st = &m.state;
        let (blend, depth) = if distortion {
            // No depth test: FDistortionMeshPS fades its offset by the scene depth itself.
            (
                distortion_blend(),
                DepthKey { depth_write: false, depth_func: D3D11_COMPARISON_ALWAYS.0 as u8, stencil: None },
            )
        } else {
            let func = if st.disable_depth_test { D3D11_COMPARISON_ALWAYS } else { D3D11_COMPARISON_GREATER_EQUAL };
            // An opaque or masked one draws in the base pass with its state (the stencil's lighting channels: the
            // draw's reference, channel 0's).
            let depth = if self.blend.opaque() {
                base_depth()
            } else {
                DepthKey { depth_write: false, depth_func: func.0 as u8, stencil: None }
            };
            (blend_key(self.blend), depth)
        };
        Ok(Some(Pass {
            vs: gpu.shader_by(codes[0].0, &codes[0].1)?,
            ps: Some(gpu.shader_by(codes[1].0, &codes[1].1)?),
            vs_binds: binds_of(&codes[0].1)?,
            ps_binds: binds_of(&codes[1].1)?,
            blend: gpu.state(StateKey::Blend(blend)),
            depth: gpu.state(StateKey::Depth(depth)),
        }))
    }
}

/// A static mesh's streams as vertex buffers for FMeshParticleVertexFactory (the store's typed buffers copied into
/// vertex buffers on the GPU): TangentX and TangentZ interleaved, the UVs interleaved, the colour (stride 0 when the
/// mesh has none: white).
#[derive(Debug)]
struct MeshStreams {
    mesh: Arc<Mesh>,
    tangents: ID3D11Buffer,
    tangent_format: DXGI_FORMAT,
    tangent_bytes: u32,
    /// The UV buffer, its format, bytes a UV and UVs a vertex (None: the mesh has none).
    uvs: Option<(ID3D11Buffer, DXGI_FORMAT, u32, u32)>,
    color: ID3D11Buffer,
    color_stride: u32,
}

/// A buffer's bytes copied into a vertex buffer.
fn vertex_copy(gpu: &Gpu, src: &Buffer) -> Result<ID3D11Buffer> {
    let bd = D3D11_BUFFER_DESC {
        ByteWidth: src.desc.size,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut b = None;
    // SAFETY: a plain buffer description, no initial data.
    unsafe { gpu.dev.CreateBuffer(&bd, None, Some(&mut b)).map_err(dx("CreateBuffer"))? };
    let b = b.expect("CreateBuffer");
    // SAFETY: two buffers of this device of the same size.
    unsafe { gpu.ctx.CopyResource(&b, &src.buf) };
    Ok(b)
}

fn typed_format(b: &Buffer, what: &str) -> Result<DXGI_FORMAT> {
    match b.desc.kind {
        BufKind::Typed { format } => Ok(format),
        k => Err(Error::Format { what: what.to_string(), why: format!("a {k:?} buffer, not a typed one") }),
    }
}

fn format_bytes(f: DXGI_FORMAT, what: &str) -> Result<u32> {
    crate::gpu::format_layout(f)
        .map(|(_, bytes)| bytes)
        .ok_or_else(|| Error::Format { what: what.to_string(), why: format!("format {}", f.0) })
}

impl MeshStreams {
    fn new(gpu: &Gpu, mesh: Arc<Mesh>) -> Result<MeshStreams> {
        let what = &mesh.path;
        let lod = mesh.lods.first().ok_or_else(|| Error::Format { what: what.clone(), why: "no LOD".into() })?;
        let b = &lod.buffers;
        let none = |s: &str| Error::Format { what: what.clone(), why: format!("no {s} buffer (a skeletal mesh?)") };
        let t = b.tangents.as_ref().ok_or_else(|| none("tangent"))?;
        let tangent_format = typed_format(t, what)?;
        let tangent_bytes = format_bytes(tangent_format, what)?;
        let uvs = match &b.tex_coords {
            Some(u) if b.num_tex_coords > 0 => {
                let f = typed_format(u, what)?;
                Some((vertex_copy(gpu, u)?, f, format_bytes(f, what)?, b.num_tex_coords))
            }
            _ => None,
        };
        let c = b.color.as_ref().ok_or_else(|| none("colour"))?;
        // The white stand-in of a mesh without colours is one texel.
        let color_stride = if c.desc.size == 4 && lod.vertices != 1 { 0 } else { 4 };
        Ok(MeshStreams {
            tangents: vertex_copy(gpu, t)?,
            tangent_format,
            tangent_bytes,
            uvs,
            color: vertex_copy(gpu, c)?,
            color_stride,
            mesh,
        })
    }
}

/// What geometry a draw takes.
#[derive(Clone, Debug)]
enum Kind {
    /// One instanced quad a particle.
    Sprite,
    /// The mesh's sections (or the one), instanced a particle.
    Mesh { streams: Arc<MeshStreams>, section: Option<u32> },
    /// A beam or trail's triangle strip.
    Strip,
}

/// One emitter's prepared draw.
#[derive(Debug)]
struct Item {
    material: usize,
    vf: VertexFactory,
    kind: Kind,
    /// Its vertices' byte offset in the frame's vertex buffer.
    offset: u32,
    /// Its dynamic parameters' byte offset in the frame's dynamic parameter stream (after the vertices).
    dynamic: u32,
    count: u32,
    params: DrawParams,
    local_to_world: Option<[f32; 12]>,
    scalars: Vec<(Arc<str>, f32)>,
}

/// The distortion pass's targets: the accumulated offsets (PF_B8G8R8A8) and the scene colour copy the apply reads.
#[derive(Debug)]
struct DistortionTargets {
    size: [u32; 2],
    offsets: Texture,
    scene_copy: Texture,
}

/// The global shaders of the distortion apply (FScreenPassVS's DrawRectangle quad and FDistortionApplyScreenPS).
#[derive(Debug)]
struct Apply {
    vs: ShaderId,
    ps: ShaderId,
    /// FScreenPassVS's DrawRectangleParameters register, FDistortionApplyScreenPS's View register.
    rect_slot: u32,
    view_slot: u32,
    /// FScreenPassVS's DrawRectangleParameters: PosScaleBias, UVScaleBias, InvTargetSizeAndTextureSize.
    rect: Buffer,
    /// The unit quad: (position, UV) per corner, a strip.
    quad: Buffer,
}

/// The particle materials and mesh streams a loader thread's warm-up made, for `Particles::adopt`.
#[derive(Debug, Default)]
pub(crate) struct ParticlesMade {
    materials: Vec<(Arc<str>, ParticleMaterial)>,
    meshes: Vec<(Arc<str>, Arc<MeshStreams>)>,
}

/// The particle draws' vertex buffers, materials and the frame's items.
#[derive(Debug)]
pub struct Particles {
    materials: Vec<ParticleMaterial>,
    by_path: HashMap<Arc<str>, usize>,
    meshes: HashMap<Arc<str>, Arc<MeshStreams>>,
    params: Vec<DrawParams>,
    items: Vec<Item>,
    /// The scalar lists of the last frame's items, kept for the next.
    spare_scalars: Vec<Vec<(Arc<str>, f32)>>,
    /// The frame's vertices, then (from `dynamic_base`) their dynamic parameters, one buffer.
    bytes: Vec<u8>,
    dynamic_bytes: Vec<u8>,
    dynamic_base: u32,
    vertices: Option<Buffer>,
    /// The sprite quad's corners (GParticleTexCoordVertexBuffer) and its indices (GParticleIndexBuffer).
    corners: Buffer,
    quad_index: Buffer,
    sprite_vf: Buffer,
    mesh_vf: Buffer,
    beam_vf: Buffer,
    /// The Primitive buffer of a world-space emitter (LocalToWorld identity) and one rewritten for a local one.
    world_primitive: Buffer,
    local_primitive: Buffer,
    zeros: Buffer,
    distortion_pass: Buffer,
    read_only_depth: Option<(usize, ID3D11DepthStencilView)>,
    distortion_targets: Option<DistortionTargets>,
    apply: Option<Apply>,
}

/// GParticleTexCoordVertexBuffer's corners and GParticleIndexBuffer's two triangles of a quad.
// Unverified: 4.26's buffers (ParticleResources.cpp), not read from the exe; the quad (0,2,3), (0,1,2) is
// counter-clockwise on screen for the camera-facing basis (Right = ViewRight, Up = -ViewUp).
const QUAD_CORNERS: [[f32; 2]; 4] = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
const QUAD_INDICES: [u16; 6] = [0, 2, 3, 0, 1, 2];

fn f4(v: [f32; 4]) -> [u32; 4] {
    v.map(f32::to_bits)
}

/// SpriteVF's rows (FParticleSpriteUniformParameters, 0xa0 bytes, metadata 145095b10): AxisLockRight and AxisLockUp
/// (w: the lerp from the camera's), TangentSelector (camera facing / axis lock, velocity, rotate lock, facing the
/// camera position), NormalsSphereCenter, NormalsCylinderUnitDirection, SubImageSize, CameraFacingBlend (w
/// RemoveHMDRoll), MacroUVParameters, (RotationScale, RotationBias, NormalsType, InvDeltaSeconds), PivotOffset.
// Unverified: the zeros (normals type 0, no camera-facing blend, no macro UV) are the defaults of 4.26's
// FDynamicSpriteEmitterData; InvDeltaSeconds is read by the velocity pass only, not drawn.
fn sprite_rows(p: &DrawParams) -> [[u32; 4]; 10] {
    let lock = if p.axis_locked { 1.0 } else { 0.0 };
    let selector = if p.velocity_aligned {
        [0.0, 1.0, 0.0, 0.0]
    } else if p.rotate_lock {
        [0.0, 0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0, 0.0]
    };
    let [h, v] = p.sub_images;
    let mut r = [[0u32; 4]; 10];
    r[0] = f4([p.lock_right[0], p.lock_right[1], p.lock_right[2], lock]);
    r[1] = f4([p.lock_up[0], p.lock_up[1], p.lock_up[2], lock]);
    r[2] = f4(selector);
    r[5] = f4([h, v, 1.0 / h, 1.0 / v]);
    r[8] = f4([p.rotation_scale, p.rotation_bias, 0.0, 0.0]);
    r[9] = f4([p.pivot[0], p.pivot[1], 0.0, 0.0]);
    r
}

/// MeshParticleVF's rows (FMeshParticleUniformParameters, 0x20 bytes, metadata 145091410): SubImageSize
/// (1 / SubImages_Horizontal, 1 / SubImages_Vertical, 0, 0), then TexCoordWeightA / B, PrevTransformAvailable (uints)
/// and DeltaSeconds.
// Unverified: the second row's zeros (no texture coordinate weights, no previous transforms, DeltaSeconds 0) are
// 4.26's FDynamicMeshEmitterData defaults, not read in the exe.
fn mesh_rows(p: &DrawParams) -> [[u32; 4]; 2] {
    let [h, v] = p.sub_images;
    [f4([1.0 / h, 1.0 / v, 0.0, 0.0]), [0; 4]]
}

/// BeamTrailVF's rows (FParticleBeamTrailUniformParameters, 0x30 bytes, metadata 145091ea0): CameraRight, CameraUp,
/// ScreenAlignment. The strip's positions are final; the rows give only the tangent basis, which no unlit pixel shader
/// reads.
// Unverified: the values are 4.26's FDynamicTrailsEmitterData / FDynamicBeam2EmitterData ones, not read in the exe.
const BEAM_ROWS: [[u32; 4]; 3] = [[0, 0x3f80_0000, 0, 0], [0, 0, 0x3f80_0000, 0], [0; 4]];

fn constant(gpu: &Gpu, rows: usize) -> Result<Buffer> {
    gpu.buffer(&BufDesc { kind: BufKind::Constant, size: (rows * 16) as u32, dynamic: true }, None)
}

/// A primitive's rows: LocalToWorld `m`, its bounds at its origin.
fn primitive_rows(m: &mvs_view::M4) -> [[u32; 4]; PRIMITIVE_ROWS] {
    let mut rows = PrimitiveRows::default();
    rows.push(&PrimitiveParams {
        local_to_world: *m,
        previous_local_to_world: *m,
        actor_world_position: [m[3][0], m[3][1], m[3][2]],
        bounds_origin: [m[3][0], m[3][1], m[3][2]],
        bounds_extent: [0.0; 3],
        bounds_radius: 0.0,
        local_bounds_min: [0.0; 3],
        local_bounds_max: [0.0; 3],
        lighting_channels: 1,
        receives_decals: false,
        use_single_sample_shadow: false,
        use_volumetric_lightmap_shadow: false,
        lightmap_index: 0,
        lightmap_uv_index: 0,
        custom_data: [[0.0; 4]; 9],
    });
    rows.0[0]
}

/// The input layout's key of `s` (the formats, strides and step rates by attribute) and its streams bound.
fn bind_streams(s: &Streams, gpu: &Gpu, vs: ShaderId, base: u32) -> Result<()> {
    let mut k: u32 = 0x811c_9dc5 ^ base;
    for (e, st) in s.elems().iter().zip(s.strides()) {
        for x in [e.index, e.format.0 as u32, *st, e.per_instance as u32] {
            k = (k ^ x).wrapping_mul(0x0100_0193);
        }
    }
    s.bind(gpu, vs, k)
}

const F4: DXGI_FORMAT = DXGI_FORMAT_R32G32B32A32_FLOAT;

/// The dynamic parameter's stream (FParticleVertexDynamicParameter, DynamicValue[4]): its stride, at the end of each
/// of the player's vertices.
const DYNAMIC_STRIDE: u32 = 16;

/// Where a draw's two streams sit in the frame's buffer: the vertices' byte offset, the dynamic parameters'.
#[derive(Clone, Copy)]
struct At {
    vertices: u32,
    dynamic: u32,
}

/// The sprite declaration (FParticleSpriteVertexFactory InitRHI 142c75ea0, declaration 144c75850): stream 0 the quad
/// corner (GParticleTexCoordVertexBuffer, Float2, ATTRIBUTE4, stride 8); stream 1 FParticleSpriteVertex a particle
/// (stride 64): Position + RelativeTime 0, OldPosition + ParticleId 1, Size + Rotation + SubImageIndex 2, Color 3;
/// stream 2 the dynamic parameter a particle, ATTRIBUTE5.
fn sprite_streams(s: &mut Streams, vb: &ID3D11Buffer, at: At, corners: &ID3D11Buffer) {
    s.add(4, DXGI_FORMAT_R32G32_FLOAT, corners, 8, 0, false);
    let stride = SPRITE_STRIDE - DYNAMIC_STRIDE;
    for (attr, off) in [(0, 0), (1, 16), (2, 32), (3, 48)] {
        s.add(attr, F4, vb, stride, at.vertices + off, true);
    }
    s.add(5, F4, vb, DYNAMIC_STRIDE, at.dynamic, true);
}

/// The beam / trail declaration (FParticleBeamTrailVertexFactory, declaration 144c754f0): stream 0
/// FParticleBeamTrailVertex a strip vertex (stride 80): Position + RelativeTime ATTRIBUTE0, OldPosition (Float3) 1,
/// Size + Rotation + SubImageIndex 2, Color (offset 48) 4, Tex_U / Tex_V / Tex_U2 / Tex_V2 (offset 64) 3; stream 1
/// the dynamic parameter, ATTRIBUTE5.
fn strip_streams(s: &mut Streams, vb: &ID3D11Buffer, at: At) {
    let stride = RIBBON_STRIDE - DYNAMIC_STRIDE;
    for (attr, format, off) in [(0, F4, 0), (1, DXGI_FORMAT_R32G32B32_FLOAT, 16), (2, F4, 32), (4, F4, 48), (3, F4, 64)]
    {
        s.add(attr, format, vb, stride, at.vertices + off, false);
    }
    s.add(5, F4, vb, DYNAMIC_STRIDE, at.dynamic, false);
}

/// FMeshParticleVertexFactory: the mesh's Position 0, TangentX 1, TangentZ 2, Color 3, TexCoords 4..7 (the last one
/// repeated past the mesh's count) a vertex; FMeshParticleInstanceVertex a particle (the instance stream, stride
/// 96, the mesh emitter's SetupVertexFactory 142c58bf0): Color 14 (offset 0), Transform 8..10, Velocity 15 (64),
/// SubUVParams 11 (Short4, 80), SubUVLerp + RelativeTime 12 (88); the dynamic parameter 13 on its own stream.
// Unverified: the mesh's own colour element (the null colour buffer when the LOD has none) and the repeated last UV
// are 4.26's FMeshParticleVertexFactory; the mesh's TexCoords from ATTRIBUTE4 are the exe's.
fn mesh_streams(s: &mut Streams, m: &MeshStreams, zeros: &ID3D11Buffer, vb: &ID3D11Buffer, at: At) {
    let b = &m.mesh.lods[0].buffers;
    s.add(0, DXGI_FORMAT_R32G32B32_FLOAT, &b.position.buf, 12, 0, false);
    let (tf, tb) = (m.tangent_format, m.tangent_bytes);
    s.add(1, tf, &m.tangents, 2 * tb, 0, false);
    s.add(2, tf, &m.tangents, 2 * tb, tb, false);
    s.add(3, DXGI_FORMAT_R8G8B8A8_UNORM, &m.color, m.color_stride, 0, false);
    for k in 0..4 {
        match &m.uvs {
            Some((buf, f, bytes, n)) => s.add(4 + k, *f, buf, n * bytes, k.min(n - 1) * bytes, false),
            None => s.add(4 + k, DXGI_FORMAT_R32G32_FLOAT, zeros, 0, 0, false),
        }
    }
    for (attr, format, off) in [
        (8, F4, 16),
        (9, F4, 32),
        (10, F4, 48),
        (11, DXGI_FORMAT_R16G16B16A16_SINT, 80),
        (12, DXGI_FORMAT_R32G32_FLOAT, 88),
        (14, F4, 0),
        (15, F4, 64),
    ] {
        s.add(attr, format, vb, MESH_STRIDE - DYNAMIC_STRIDE, at.vertices + off, true);
    }
    s.add(13, F4, vb, DYNAMIC_STRIDE, at.dynamic, true);
}

/// What the stage registers of a particle draw read.
struct Resolver<'a> {
    gpu: &'a Gpu,
    scene: &'a SceneBuffers,
    mat: &'a ParticleMaterial,
    factory: Option<&'a Buffer>,
    primitive: &'a Buffer,
    zeros: &'a Buffer,
    /// TranslucentBasePass as the translucent passes fill it (translucency.rs); None in the distortion pass.
    pass: Option<PassInput<'a>>,
    distortion_pass: &'a Buffer,
    depth: Option<&'a ID3D11ShaderResourceView>,
}

#[derive(Clone, Copy)]
enum Stage {
    Vertex,
    Pixel,
}

impl Resolver<'_> {
    fn buffer(&self, s: Src) -> Option<ID3D11Buffer> {
        match s {
            Src::View(_) => Some(self.scene.view.buf.clone()),
            Src::Primitive => Some(self.primitive.buf.clone()),
            Src::Material(_) => self.mat.inst.buffer.as_ref().map(|b| b.buf.clone()),
            Src::Collection(n) => self.mat.inst.collections.get(n as usize).map(|b| b.buf.clone()),
            Src::Factory => self.factory.map(|b| b.buf.clone()),
            Src::TranslucentBasePass(_) => self.pass.map(|p| p.buffer.clone()),
            Src::Globals => Some(self.zeros.buf.clone()),
            Src::DistortionPass(_) => Some(self.distortion_pass.buf.clone()),
            Src::None => None,
        }
    }

    fn srv(&self, s: Src) -> Option<ID3D11ShaderResourceView> {
        match s {
            Src::View(k) => match VIEW_RESOURCES.get(k as usize).copied() {
                Some("PrimitiveSceneData") => self.scene.primitives.srv.clone(),
                Some("LightmapSceneData") => self.scene.lightmaps.srv.clone(),
                Some(name) => {
                    self.scene.view_textures.iter().rev().find(|(n, _)| *n == name).and_then(|(_, t)| t.srv.clone())
                }
                None => None,
            },
            Src::Material(k) => match self.mat.inst.resources.get(k as usize) {
                Some(MaterialResource::Texture(TextureSource::Asset(t))) => t.srv.clone(),
                // (no particle material of the reach takes a frame target)
                _ => None,
            },
            Src::TranslucentBasePass(k) => self.pass?.srvs.get(k as usize).cloned().flatten(),
            Src::DistortionPass(DP_SCENE_DEPTH) => self.depth.cloned(),
            _ => None,
        }
    }

    fn sampler(&self, s: Src) -> Option<ID3D11SamplerState> {
        let key = match s {
            Src::View(k) => view_sampler(VIEW_RESOURCES.get(k as usize)?),
            Src::Material(k) => match self.mat.inst.resources.get(k as usize) {
                Some(MaterialResource::Sampler(id)) => return Some(self.gpu.sampler(*id)),
                _ => return None,
            },
            Src::TranslucentBasePass(k) => return self.pass?.samplers.get(k as usize).cloned().flatten(),
            // Unverified: point clamp for the scene depth (SceneTextures' samplers).
            Src::DistortionPass(DP_SCENE_DEPTH_SAMPLER) => {
                SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_POINT, D3D11_TEXTURE_ADDRESS_CLAMP)
            }
            _ => return None,
        };
        Some(self.gpu.sampler(self.gpu.state(StateKey::Sampler(key))))
    }

    fn bind(&self, stage: Stage, b: &Binds) {
        let t = StageTables::of(&b.cbs, &b.srvs, &b.samplers, |s| self.buffer(s), |s| self.srv(s), |s| self.sampler(s));
        t.set(self.gpu, matches!(stage, Stage::Pixel));
    }
}

/// Clears the shader resources the particle draws bound (the depth, the targets read).
fn unbind_resources(gpu: &Gpu) {
    let none: [Option<ID3D11ShaderResourceView>; 16] = Default::default();
    // SAFETY: plain state calls.
    unsafe {
        gpu.ctx.VSSetShaderResources(0, Some(&none));
        gpu.ctx.PSSetShaderResources(0, Some(&none));
        gpu.ctx.OMSetRenderTargets(None, None);
    }
}

/// Where the translucent draws go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Into {
    /// A material's separate translucency setting picks SceneColor or the separate translucency target.
    ByMaterial,
    /// Everything into SceneColor.
    SceneColor,
}

impl Particles {
    pub fn new(gpu: &Gpu, assets: &Assets) -> Result<Particles> {
        let _ = assets;
        let corners: Vec<u8> = QUAD_CORNERS.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        let index: Vec<u8> = QUAD_INDICES.iter().flat_map(|x| x.to_le_bytes()).collect();
        let world: [[u32; 4]; PRIMITIVE_ROWS] = primitive_rows(&identity());
        let fixed = |kind: BufKind, bytes: &[u8]| {
            gpu.buffer(&BufDesc { kind, size: bytes.len() as u32, dynamic: false }, Some(bytes))
        };
        Ok(Particles {
            materials: Vec::new(),
            by_path: HashMap::new(),
            meshes: HashMap::new(),
            params: Vec::new(),
            items: Vec::new(),
            spare_scalars: Vec::new(),
            bytes: Vec::new(),
            dynamic_bytes: Vec::new(),
            dynamic_base: 0,
            vertices: None,
            corners: fixed(BufKind::Vertex, &corners)?,
            quad_index: fixed(BufKind::Index16, &index)?,
            sprite_vf: constant(gpu, 10)?,
            mesh_vf: constant(gpu, 2)?,
            beam_vf: fixed(BufKind::Constant, rows_bytes(&BEAM_ROWS))?,
            world_primitive: fixed(BufKind::Constant, rows_bytes(&world))?,
            local_primitive: constant(gpu, PRIMITIVE_ROWS)?,
            zeros: fixed(BufKind::Constant, &[0; ZERO_ROWS * 16])?,
            distortion_pass: constant(gpu, DP_ROWS)?,
            read_only_depth: None,
            distortion_targets: None,
            apply: None,
        })
    }

    /// The warm-up (crate::warm): each emitter's material made with the passes its draws take (the base or
    /// translucent one, a translucent material's distortion one), a mesh emitter's streams, the input layouts and
    /// the rasterizer states. What fails is left out and named in the lines returned (a system the match never
    /// spawns may hold it).
    pub(crate) fn warm(&mut self, gpu: &Gpu, assets: &Assets, emitters: &[(Arc<str>, EmitterKind)]) -> Vec<String> {
        let mut out = Vec::new();
        for cull in [D3D11_CULL_NONE, D3D11_CULL_BACK, D3D11_CULL_FRONT] {
            gpu.state(StateKey::Raster(raster(cull)));
        }
        // (the distortion apply, drawn once a distorting emitter draws)
        match self.make_apply(gpu, assets) {
            Ok(()) => {
                let a = self.apply.as_ref().expect("made above");
                let mut st = Streams::default();
                st.add(0, F4, &a.quad.buf, 24, 0, false);
                st.add(1, DXGI_FORMAT_R32G32_FLOAT, &a.quad.buf, 24, 16, false);
                if let Err(e) = bind_streams(&st, gpu, a.vs, 0x0d15) {
                    out.push(format!("the distortion apply: {e}"));
                }
            }
            Err(e) => out.push(format!("the distortion apply: {e}")),
        }
        for (material, kind) in emitters {
            let path = material.split_once('?').map_or(&**material, |(p, _)| p);
            let k = match self.by_path.get(path) {
                Some(&k) => k,
                None => match ParticleMaterial::try_new(gpu, assets, path) {
                    Ok(m) => {
                        self.materials.push(m);
                        self.by_path.insert(Arc::from(path), self.materials.len() - 1);
                        self.materials.len() - 1
                    }
                    Err(e) => {
                        out.push(e);
                        continue;
                    }
                },
            };
            let (vf, streams) = match kind {
                EmitterKind::Sprite => (VertexFactory::Sprite, None),
                EmitterKind::Mesh { mesh } => {
                    let s = match self.meshes.get(mesh) {
                        Some(s) => s.clone(),
                        None => match assets.mesh(mesh).and_then(|m| MeshStreams::new(gpu, m)) {
                            Ok(s) => {
                                let s = Arc::new(s);
                                self.meshes.insert(mesh.clone(), s.clone());
                                s
                            }
                            Err(e) => {
                                out.push(format!("{mesh}: {e}"));
                                continue;
                            }
                        },
                    };
                    (VertexFactory::MeshParticle, Some(s))
                }
                EmitterKind::Beam | EmitterKind::Ribbon | EmitterKind::AnimTrail => (VertexFactory::BeamTrail, None),
            };
            let mat = &mut self.materials[k];
            let distortion: &[bool] = if mat.blend.opaque() { &[false] } else { &[false, true] };
            for &d in distortion {
                let vs = match mat.pass(gpu, assets, vf, d) {
                    Ok(Some(p)) => p.vs,
                    Ok(None) => continue,
                    Err(e) => {
                        out.push(e.to_string());
                        continue;
                    }
                };
                // (the layout's key is its elements' formats, strides and step rates: any buffer stands in)
                let mut st = Streams::default();
                let (vb, at) = (&self.corners.buf, At { vertices: 0, dynamic: 0 });
                match &streams {
                    Some(m) => mesh_streams(&mut st, m, vb, vb, at),
                    None if vf == VertexFactory::Sprite => sprite_streams(&mut st, vb, at, vb),
                    None => strip_streams(&mut st, vb, at),
                }
                if let Err(e) = bind_streams(&st, gpu, vs, mvs_material::hashed_name(vf.type_name()) as u32) {
                    out.push(e.to_string());
                }
            }
        }
        out
    }

    /// What this pass's warm-up made, taken out (a loader thread's own pass, for `Particles::adopt`).
    pub(crate) fn into_made(self) -> ParticlesMade {
        let mut slots: Vec<Option<ParticleMaterial>> = self.materials.into_iter().map(Some).collect();
        let mut materials: Vec<(Arc<str>, ParticleMaterial)> =
            self.by_path.into_iter().filter_map(|(p, k)| slots[k].take().map(|m| (p, m))).collect();
        materials.sort_by(|a, b| a.0.cmp(&b.0));
        ParticlesMade { materials, meshes: self.meshes.into_iter().collect() }
    }

    /// What a loader thread's warm-up made taken in: the materials and mesh streams this pass has not made yet.
    pub(crate) fn adopt(&mut self, made: ParticlesMade) {
        for (path, m) in made.materials {
            if !self.by_path.contains_key(&path) {
                self.materials.push(m);
                self.by_path.insert(path, self.materials.len() - 1);
            }
        }
        for (mesh, s) in made.meshes {
            self.meshes.entry(mesh).or_insert(s);
        }
    }

    /// The particle materials and the mesh emitters' streams made so far (a warm-up's check).
    pub(crate) fn counts(&self) -> [usize; 2] {
        [self.materials.len(), self.meshes.len()]
    }

    /// The emitters' DrawParams (Player::draw_params, aligned with the draws `prepare` takes), before `prepare`.
    pub fn set_params(&mut self, params: &[DrawParams]) {
        self.params.clear();
        self.params.extend_from_slice(params);
    }

    /// The frame's emitter draws uploaded and made into items, before the View is written. `set_params` gives each
    /// draw's DrawParams first.
    pub fn prepare(&mut self, gpu: &Gpu, assets: &Assets, emitters: &[EmitterDraw]) -> Result<()> {
        // (the last frame's items' scalar lists kept for this frame's)
        self.spare_scalars.extend(self.items.drain(..).map(|it| it.scalars));
        self.bytes.clear();
        self.dynamic_bytes.clear();
        if emitters.is_empty() {
            return Ok(());
        }
        if self.params.len() != emitters.len() {
            return Err(Error::Format {
                what: "particles".into(),
                why: format!("{} emitter draws with {} DrawParams", emitters.len(), self.params.len()),
            });
        }
        for (e, p) in emitters.iter().zip(&self.params) {
            if e.count == 0 {
                continue;
            }
            let (vf, stride, kind) = match &e.kind {
                EmitterKind::Sprite => (VertexFactory::Sprite, SPRITE_STRIDE, Kind::Sprite),
                EmitterKind::Mesh { mesh } => {
                    let streams = match self.meshes.get(mesh) {
                        Some(s) => s.clone(),
                        None => {
                            let s = Arc::new(MeshStreams::new(gpu, assets.mesh(mesh)?)?);
                            self.meshes.insert(mesh.clone(), s.clone());
                            s
                        }
                    };
                    (VertexFactory::MeshParticle, MESH_STRIDE, Kind::Mesh { streams, section: p.mesh_section })
                }
                EmitterKind::Beam | EmitterKind::Ribbon | EmitterKind::AnimTrail => {
                    (VertexFactory::BeamTrail, RIBBON_STRIDE, Kind::Strip)
                }
            };
            let what = || format!("{} {:?}", e.material, e.kind);
            if e.stride != stride || e.vertices.len() != (stride * e.count) as usize {
                return Err(Error::Format {
                    what: what(),
                    why: format!("{} bytes of stride {} for {} vertices", e.vertices.len(), e.stride, e.count),
                });
            }
            let path = e.material.split_once('?').map_or(&*e.material, |(p, _)| p);
            let material = match self.by_path.get(path) {
                Some(&k) => k,
                None => {
                    let m = ParticleMaterial::new(gpu, assets, path)?;
                    self.materials.push(m);
                    self.by_path.insert(Arc::from(path), self.materials.len() - 1);
                    self.materials.len() - 1
                }
            };
            // The player writes each vertex's dynamic parameter after it; the vertex factories stream it apart.
            let offset = self.bytes.len() as u32;
            let dynamic = self.dynamic_bytes.len() as u32;
            let at = (stride - DYNAMIC_STRIDE) as usize;
            for v in e.vertices.chunks_exact(stride as usize) {
                self.bytes.extend_from_slice(&v[..at]);
                self.dynamic_bytes.extend_from_slice(&v[at..]);
            }
            let mut scalars = self.spare_scalars.pop().unwrap_or_default();
            scalars.clone_from(&e.material_scalars);
            self.items.push(Item {
                material,
                vf,
                kind,
                offset,
                dynamic,
                count: e.count,
                params: p.clone(),
                local_to_world: e.local_to_world,
                scalars,
            });
        }
        if self.bytes.is_empty() {
            return Ok(());
        }
        self.dynamic_base = self.bytes.len().next_multiple_of(16) as u32;
        self.bytes.resize(self.dynamic_base as usize, 0);
        self.bytes.extend_from_slice(&self.dynamic_bytes);
        let need = self.bytes.len() as u32;
        if self.vertices.as_ref().is_none_or(|b| b.desc.size < need) {
            let size = need.next_power_of_two().max(64 << 10);
            self.vertices = Some(gpu.buffer(&BufDesc { kind: BufKind::Vertex, size, dynamic: true }, None)?);
        }
        gpu.write(self.vertices.as_ref().expect("made above"), &self.bytes);
        Ok(())
    }

    /// The prepared translucent items in the player's order (BLEND_Modulate first) into SceneColor or, for a material
    /// with separate translucency, the separate translucency target the translucent pass cleared (the opaque and
    /// masked ones: `draw_opaque`).
    // Deviation: a BLEND_Modulate material multiplies SceneColor here, where the build draws it into the separate
    // translucency's modulate target that the composite multiplies the scene by; the picture is the same but for the
    // distortion pass, which here bends the modulated scene.
    /// `pass`: TranslucentBasePass as the translucent passes filled it (Translucency::pass_input).
    pub fn draw(&mut self, fr: &mut Frame, pass: PassInput) -> Result<()> {
        self.draw_into(fr, Into::ByMaterial, pass)
    }

    /// `draw` with every item into SceneColor: for a frame drawn without the separate translucency composite (each
    /// blend state's result is the composite's, the order among the translucent meshes aside).
    pub fn draw_scene_color(&mut self, fr: &mut Frame, pass: PassInput) -> Result<()> {
        self.draw_into(fr, Into::SceneColor, pass)
    }

    /// The read-only view of the scene depth (the depth tested while the pixel shaders read it).
    fn read_only_depth(&mut self, gpu: &Gpu, depth: &Texture) -> Result<ID3D11DepthStencilView> {
        use windows::core::Interface;
        let key = depth.res.as_raw() as usize;
        if let Some((k, v)) = &self.read_only_depth
            && *k == key
        {
            return Ok(v.clone());
        }
        let format = match depth.desc.format {
            DXGI_FORMAT_R32G8X24_TYPELESS => DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
            DXGI_FORMAT_R24G8_TYPELESS => DXGI_FORMAT_D24_UNORM_S8_UINT,
            f => return Err(Error::Format { what: "scene depth".into(), why: format!("format {}", f.0) }),
        };
        let dd = D3D11_DEPTH_STENCIL_VIEW_DESC {
            Format: format,
            ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
            Flags: (D3D11_DSV_READ_ONLY_DEPTH.0 | D3D11_DSV_READ_ONLY_STENCIL.0) as u32,
            Anonymous: D3D11_DEPTH_STENCIL_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_DSV { MipSlice: 0 } },
        };
        let mut v = None;
        // SAFETY: a view of the depth target's resource in its depth format.
        unsafe { gpu.dev.CreateDepthStencilView(&depth.res, Some(&dd), Some(&mut v)).map_err(dx("read-only DSV"))? };
        let v = v.expect("CreateDepthStencilView");
        self.read_only_depth = Some((key, v.clone()));
        Ok(v)
    }

    /// The prepared opaque and masked items into the targets the base pass has bound (SceneColor, the G-buffer, the
    /// depth), after the meshes' base pass and before the deferred lighting, so a lit material is lit as an opaque
    /// mesh is (FBasePassMeshProcessor takes a particle's mesh batch as any primitive's). No pass buffer, as the
    /// meshes' base pass binds none.
    // Unverified: the particles' primitives in lighting channel 0 and not receiving decals (the stencil's bit 7 left
    // 0); the UParticleSystemComponent defaults are not read from the exe. Their pixels are lit only when an opaque
    // mesh of the frame is in channel 0 too (lighting.rs takes the masks from the mesh list; the fighters' always are).
    pub fn draw_opaque(&mut self, fr: &mut Frame) -> Result<()> {
        for i in 0..self.items.len() {
            if self.materials[self.items[i].material].blend.opaque() {
                self.draw_item(fr, i, false, None, None)?;
            }
        }
        Ok(())
    }

    fn draw_into(&mut self, fr: &mut Frame, into: Into, pass: PassInput) -> Result<()> {
        if self.items.is_empty() {
            return Ok(());
        }
        let gpu = fr.gpu;
        let t = fr.targets;
        let read_only = self.read_only_depth(gpu, &t.depth)?;
        gpu.viewport([0, 0, t.size[0], t.size[1]]);
        let depth_srv = t.depth.srv.clone();
        let mut bound: Option<bool> = None;
        // (the translucent items, the modulated ones first, each group in item order)
        let n = self.items.len();
        for k in 0..2 * n {
            let i = k % n;
            let b = self.materials[self.items[i].material].blend;
            if b.opaque() || (b == Blend::Modulate) != (k < n) {
                continue;
            }
            let separate = into == Into::ByMaterial
                && self.materials[self.items[i].material].inst.material.state.separate_translucency
                && self.materials[self.items[i].material].blend != Blend::Modulate;
            if bound != Some(separate) {
                let rtv = if separate { t.separate_translucency.rtv.clone() } else { t.scene_color.rtv.clone() };
                // SAFETY: as above; the depth is read-only, so the pixel shaders may read it.
                unsafe { gpu.ctx.OMSetRenderTargets(Some(&[rtv]), &read_only) };
                bound = Some(separate);
            }
            self.draw_item(fr, i, false, depth_srv.as_ref(), Some(pass))?;
        }
        unbind_resources(gpu);
        Ok(())
    }

    /// One item: its material's pass (the distortion one when `distortion`), states, streams and buffers, the draw.
    fn draw_item(
        &mut self,
        fr: &Frame,
        i: usize,
        distortion: bool,
        depth: Option<&ID3D11ShaderResourceView>,
        tbp: Option<PassInput>,
    ) -> Result<bool> {
        let gpu = fr.gpu;
        let item = &self.items[i];
        let mat = &mut self.materials[item.material];
        mat.refresh(gpu, &item.scalars)?;
        let Some(pass) = mat.pass(gpu, fr.assets, item.vf, distortion)? else { return Ok(false) };
        let (vs, ps, blend, depth_state) = (pass.vs, pass.ps, pass.blend, pass.depth);
        let two_sided = mat.inst.material.state.two_sided;
        let rs = gpu.state(StateKey::Raster(raster(cull_of(&item.kind, two_sided, &item.params))));
        gpu.set_shaders(vs, ps);
        gpu.set_states(blend, rs, depth_state, 0);
        let vb = &self.vertices.as_ref().expect("prepared").buf;
        let mut s = Streams::default();
        let at = At { vertices: item.offset, dynamic: self.dynamic_base + item.dynamic };
        let factory = match &item.kind {
            Kind::Sprite => {
                sprite_streams(&mut s, vb, at, &self.corners.buf);
                gpu.write_rows(&self.sprite_vf, &sprite_rows(&item.params));
                Some(&self.sprite_vf)
            }
            Kind::Mesh { streams, .. } => {
                mesh_streams(&mut s, streams, &fr.scene.zeros.buf, vb, at);
                gpu.write_rows(&self.mesh_vf, &mesh_rows(&item.params));
                Some(&self.mesh_vf)
            }
            Kind::Strip => {
                strip_streams(&mut s, vb, at);
                Some(&self.beam_vf)
            }
        };
        bind_streams(&s, gpu, vs, mvs_material::hashed_name(item.vf.type_name()) as u32)?;
        let primitive = match &item.local_to_world {
            Some(m) => {
                gpu.write_rows(&self.local_primitive, &primitive_rows(&matrix_of(m)));
                &self.local_primitive
            }
            None => &self.world_primitive,
        };
        let pass = mat.passes[&(item.vf, distortion)].as_ref().expect("made above");
        let r = Resolver {
            gpu,
            scene: fr.scene,
            mat,
            factory,
            primitive,
            zeros: &self.zeros,
            pass: tbp,
            distortion_pass: &self.distortion_pass,
            depth,
        };
        r.bind(Stage::Vertex, &pass.vs_binds);
        r.bind(Stage::Pixel, &pass.ps_binds);
        let topology = |t: D3D_PRIMITIVE_TOPOLOGY| {
            // SAFETY: a plain state call.
            unsafe { gpu.ctx.IASetPrimitiveTopology(t) }
        };
        // SAFETY: the draws read the buffers bound above, within their sizes (checked at prepare).
        unsafe {
            match &item.kind {
                Kind::Sprite => {
                    topology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                    gpu.ctx.IASetIndexBuffer(&self.quad_index.buf, DXGI_FORMAT_R16_UINT, 0);
                    gpu.ctx.DrawIndexedInstanced(6, item.count, 0, 0, 0);
                }
                Kind::Mesh { streams, section } => {
                    topology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                    let lod = &streams.mesh.lods[0];
                    let b = &lod.buffers;
                    let format =
                        if b.index.desc.kind == BufKind::Index16 { DXGI_FORMAT_R16_UINT } else { DXGI_FORMAT_R32_UINT };
                    gpu.ctx.IASetIndexBuffer(&b.index.buf, format, 0);
                    for (k, sec) in lod.sections.iter().enumerate() {
                        if section.is_some_and(|s| s as usize != k) || sec.triangles == 0 {
                            continue;
                        }
                        gpu.ctx.DrawIndexedInstanced(sec.triangles * 3, item.count, sec.first_index, 0, 0);
                    }
                }
                Kind::Strip => {
                    topology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
                    gpu.ctx.Draw(item.count, 0);
                }
            }
        }
        Ok(true)
    }

    /// The distortion pass over SceneColor: the distortion materials' FDistortionMeshPS draws accumulate their
    /// offsets into a PF_B8G8R8A8 target (BO_Add, One / One, no depth test), then FDistortionApplyScreenPS reads a copy
    /// of SceneColor at the offset UV (BufferBilinearUVMinMax bounds it) into SceneColor.
    // Deviation: the apply writes every pixel and no merge pass copies back under the distortion stencil; a pixel
    // with no offset reads its own texel through the point sampler, so the picture is the same.
    pub fn distortion(&mut self, fr: &mut Frame) -> Result<()> {
        let gpu = fr.gpu;
        let mut any = false;
        for i in 0..self.items.len() {
            let item = &self.items[i];
            let mat = &mut self.materials[item.material];
            if !mat.blend.opaque() && mat.pass(gpu, fr.assets, item.vf, true)?.is_some() {
                any = true;
                break;
            }
        }
        if !any {
            return Ok(());
        }
        let t = fr.targets;
        let [w, h] = t.size;
        if self.distortion_targets.as_ref().is_none_or(|d| d.size != t.size) {
            let make = |format: DXGI_FORMAT, rtv: bool| {
                gpu.texture(
                    &TexDesc {
                        kind: TexKind::D2,
                        size: [w, h, 1],
                        mips: 1,
                        format,
                        views: TexViews { srv: true, rtv, ..Default::default() },
                    },
                    &[],
                )
            };
            self.distortion_targets = Some(DistortionTargets {
                size: t.size,
                offsets: make(DXGI_FORMAT_B8G8R8A8_UNORM, true)?,
                scene_copy: make(t.scene_color.desc.format, false)?,
            });
        }
        // DistortionParams (FUN_1419b16d0): the projection's M[0][0], the view's
        // aspect, width and height in pixels.
        let mut rows = [[0u32; 4]; DP_ROWS];
        let p00 = fr.input.camera.view_to_clip[0][0] as f32;
        rows[7] = f4([p00, w as f32 / h as f32, w as f32, h as f32]);
        gpu.write_rows(&self.distortion_pass, &rows);
        let dt = self.distortion_targets.as_ref().expect("made above");
        let rtv = dt.offsets.rtv.clone().expect("a target view");
        // SAFETY: views of this device's targets.
        unsafe {
            gpu.ctx.ClearRenderTargetView(&rtv, &[0.0; 4]);
            gpu.ctx.OMSetRenderTargets(Some(&[Some(rtv)]), None);
        }
        gpu.viewport([0, 0, w, h]);
        let depth = t.depth.srv.clone();
        for i in 0..self.items.len() {
            if !self.materials[self.items[i].material].blend.opaque() {
                self.draw_item(fr, i, true, depth.as_ref(), None)?;
            }
        }
        unbind_resources(gpu);
        self.apply(fr)
    }

    /// The distortion apply's shaders, quad and constant buffer (made once: its first draw or the warm-up).
    fn make_apply(&mut self, gpu: &Gpu, assets: &Assets) -> Result<()> {
        if self.apply.is_none() {
            let g = assets.global_shaders();
            let shader = |name: &str, f: Frequency, buffer: &str| -> Result<(ShaderId, u32)> {
                let row = g.find(name, 0, f).ok_or_else(|| Error::Format {
                    what: "global shaders".into(),
                    why: format!("no {name} permutation 0"),
                })?;
                let sha = g.shaders.code(row);
                let code = assets.code(sha)?;
                let slot = code
                    .buffer_register(buffer)
                    .ok_or_else(|| Error::Format { what: name.into(), why: format!("no {buffer} uniform buffer") })?;
                Ok((gpu.shader_by(sha, &code)?, slot))
            };
            // The DrawRectangle quad: (position x, y, 0, 1; UV) per corner, both the unit square, a strip.
            let quad: [[f32; 6]; 4] = [
                [0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                [1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
                [1.0, 1.0, 0.0, 1.0, 1.0, 1.0],
            ];
            let bytes: Vec<u8> = quad.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
            let (vs, rect_slot) = shader("FScreenPassVS", Frequency::Vertex, "DrawRectangleParameters")?;
            let (ps, view_slot) = shader("FDistortionApplyScreenPS", Frequency::Pixel, "View")?;
            self.apply = Some(Apply {
                vs,
                ps,
                rect_slot,
                view_slot,
                rect: constant(gpu, 3)?,
                quad: gpu.buffer(&BufDesc { kind: BufKind::Vertex, size: 96, dynamic: false }, Some(&bytes))?,
            });
        }
        Ok(())
    }

    /// FScreenPassVS and FDistortionApplyScreenPS over the whole view.
    fn apply(&mut self, fr: &Frame) -> Result<()> {
        let gpu = fr.gpu;
        self.make_apply(gpu, fr.assets)?;
        let a = self.apply.as_ref().expect("made above");
        let t = fr.targets;
        let dt = self.distortion_targets.as_ref().expect("made by distortion");
        let [w, h] = t.size.map(|x| x as f32);
        gpu.write_rows(
            &a.rect,
            &[f4([w, h, 0.0, 0.0]), f4([w, h, 0.0, 0.0]), f4([1.0 / w, 1.0 / h, 1.0 / w, 1.0 / h])],
        );
        gpu.set_shaders(a.vs, Some(a.ps));
        let depth = DepthKey { depth_write: false, depth_func: D3D11_COMPARISON_ALWAYS.0 as u8, stencil: None };
        gpu.set_states(
            gpu.state(StateKey::Blend(BlendKey::OPAQUE)),
            gpu.state(StateKey::Raster(raster(D3D11_CULL_NONE))),
            gpu.state(StateKey::Depth(depth)),
            0,
        );
        let mut s = Streams::default();
        s.add(0, F4, &a.quad.buf, 24, 0, false);
        s.add(1, DXGI_FORMAT_R32G32_FLOAT, &a.quad.buf, 24, 16, false);
        s.bind(gpu, a.vs, 0x0d15)?;
        // Unverified: both textures through a point clamp sampler (the static state the exe's distortion pass set-up
        // picks).
        let point = gpu.sampler(
            gpu.state(StateKey::Sampler(SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_POINT, D3D11_TEXTURE_ADDRESS_CLAMP))),
        );
        // SAFETY: this device's resources; SceneColor is copied before it is bound as the target.
        unsafe {
            gpu.ctx.CopyResource(&dt.scene_copy.res, &t.scene_color.res);
            gpu.ctx.OMSetRenderTargets(Some(std::slice::from_ref(&t.scene_color.rtv)), None);
            gpu.ctx.VSSetConstantBuffers(a.rect_slot, Some(&[Some(a.rect.buf.clone())]));
            gpu.ctx.PSSetConstantBuffers(a.view_slot, Some(&[Some(fr.scene.view.buf.clone())]));
            gpu.ctx.PSSetShaderResources(0, Some(&[dt.scene_copy.srv.clone(), dt.offsets.srv.clone()]));
            gpu.ctx.PSSetSamplers(0, Some(&[Some(point.clone()), Some(point)]));
            gpu.ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            gpu.ctx.Draw(4, 0);
        }
        unbind_resources(gpu);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> DrawParams {
        DrawParams {
            lock_up: [0.0; 3],
            lock_right: [0.0; 3],
            axis_locked: false,
            rotate_lock: false,
            velocity_aligned: false,
            pivot: [-0.5, -0.5],
            sub_images: [4.0, 2.0],
            rotation_scale: 1.0,
            rotation_bias: 0.0,
            mesh_section: None,
            mirrored: false,
            view_z: 0.0,
            mesh_id: 0,
        }
    }

    fn row(r: [u32; 4]) -> [f32; 4] {
        r.map(f32::from_bits)
    }

    #[test]
    fn sprite_uniforms() {
        let p = params();
        let r = sprite_rows(&p);
        assert_eq!(row(r[2]), [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(row(r[5]), [4.0, 2.0, 0.25, 0.5]);
        assert_eq!(row(r[9]), [-0.5, -0.5, 0.0, 0.0]);
        let locked = DrawParams { axis_locked: true, lock_up: [-1.0, 0.0, 0.0], lock_right: [0.0, 1.0, 0.0], ..p };
        let r = sprite_rows(&locked);
        assert_eq!(row(r[0]), [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(row(r[1]), [-1.0, 0.0, 0.0, 1.0]);
        let v = DrawParams { velocity_aligned: true, rotate_lock: true, ..params() };
        assert_eq!(row(sprite_rows(&v)[2]), [0.0, 1.0, 0.0, 0.0]);
        let rl = DrawParams { rotate_lock: true, ..params() };
        assert_eq!(row(sprite_rows(&rl)[2]), [0.0, 0.0, 1.0, 0.0]);
    }

    /// The quad's triangles wind counter-clockwise on screen (x right, y up) for the camera-facing basis: a corner
    /// (u, v) at Right (u - 0.5) + Up (v - 0.5) with Right = +x and Up = -ViewUp = -y.
    #[test]
    fn quad_faces_the_camera() {
        let at = |k: u16| {
            let [u, v] = QUAD_CORNERS[k as usize];
            [u - 0.5, -(v - 0.5)]
        };
        for t in QUAD_INDICES.chunks(3) {
            let [a, b, c] = [at(t[0]), at(t[1]), at(t[2])];
            let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            assert!(cross > 0.0, "{t:?}");
        }
    }

    #[test]
    fn culling() {
        let p = params();
        assert_eq!(cull_of(&Kind::Sprite, false, &p), D3D11_CULL_NONE);
        let locked = DrawParams { axis_locked: true, ..params() };
        assert_eq!(cull_of(&Kind::Sprite, false, &locked), D3D11_CULL_BACK);
        assert_eq!(cull_of(&Kind::Sprite, true, &locked), D3D11_CULL_NONE);
        let vel = DrawParams { velocity_aligned: true, ..locked.clone() };
        assert_eq!(cull_of(&Kind::Sprite, false, &vel), D3D11_CULL_NONE);
        assert_eq!(cull_of(&Kind::Strip, false, &locked), D3D11_CULL_NONE);
    }

    #[test]
    fn blends() {
        let t = blend_key(Blend::Translucent).targets[0];
        assert_eq!((t.color_src, t.color_dst), (D3D11_BLEND_SRC_ALPHA.0 as u8, D3D11_BLEND_INV_SRC_ALPHA.0 as u8));
        assert_eq!((t.alpha_src, t.alpha_dst), (D3D11_BLEND_ZERO.0 as u8, D3D11_BLEND_INV_SRC_ALPHA.0 as u8));
        let m = blend_key(Blend::Modulate).targets[0];
        assert_eq!(
            (m.color_src, m.color_dst, m.write_mask),
            (D3D11_BLEND_DEST_COLOR.0 as u8, D3D11_BLEND_ZERO.0 as u8, 7)
        );
        assert_eq!(blend_key(Blend::Masked), BlendKey::OPAQUE);
    }

    #[test]
    fn sources() {
        assert_eq!(buffer_src("SpriteVF", 4), Src::Factory);
        assert_eq!(buffer_src("", 0), Src::Globals);
        assert_eq!(buffer_src("MaterialCollection1", 3), Src::Collection(1));
        assert_eq!(buffer_src("TranslucentDepthPass", 2), Src::None);
    }
}
