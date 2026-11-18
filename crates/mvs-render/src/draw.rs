//! The frame's draw list and its executor: each item a mesh section (or a particle emitter's own geometry) with its
//! vertex factory input, material, primitive and lightmap rows, run pass by pass with the build's shaders.

use std::sync::Arc;

use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::assets::{Assets, Mesh};
use crate::gpu::{BufKind, Buffer, Gpu, InputElement, SamplerKey, ShaderId, StateId, StateKey, Texture};
use crate::material::{
    LightMapPolicy, MaterialId, MaterialInstance, MaterialResource, Materials, PassKind, Source, StageBinds,
    TextureSource, VertexFactory, view_sampler,
};
use crate::targets::SceneTargets;
use crate::view::{SceneBuffers, VIEW_RESOURCES};
use crate::{Error, Result};

/// The passes an item draws in, a bit per PassKind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PassMask(pub u8);

impl PassMask {
    pub fn of(passes: &[PassKind]) -> PassMask {
        PassMask(passes.iter().fold(0, |m, &p| m | 1 << p as u8))
    }

    pub fn has(self, p: PassKind) -> bool {
        self.0 & 1 << p as u8 != 0
    }

    pub fn with(self, p: PassKind) -> PassMask {
        PassMask(self.0 | 1 << p as u8)
    }
}

/// A render section of a mesh's LOD.
#[derive(Clone, Debug)]
pub struct MeshSlice {
    pub mesh: Arc<Mesh>,
    pub lod: u8,
    pub section: u16,
}

/// What the vertex factory reads besides the mesh's streams.
#[derive(Clone, Debug, Default)]
pub enum VfInput {
    #[default]
    None,
    /// GPUSkin's BoneMatrices, and the morph vertex buffer of a morphed pose (TGPUSkinMorphVertexFactory's
    /// DeltaPosition / DeltaTangentZ stream).
    Bones { bones: Arc<Buffer>, morph: Option<Arc<Buffer>> },
    /// Per-instance vertices (particles) and the vertex factory's own uniform buffer (SpriteVF, MeshParticleVF,
    /// BeamTrailVF), when it has one.
    Instances { vertices: Arc<Buffer>, count: u32, uniforms: Option<Arc<Buffer>> },
    /// An instanced static mesh component's InstanceVF buffer and its instance SRVs.
    StaticInstances(Arc<crate::ism::IsmInstances>),
    /// A landscape component's LandscapeParameters and the per-view LOD buffers it reads.
    Landscape(Arc<crate::landscape::ComponentBinds>),
}

/// A resource a vertex factory's module resolves for a register.
#[derive(Clone, Debug, Default)]
pub enum Bound {
    #[default]
    None,
    Buffer(ID3D11Buffer),
    Srv(ID3D11ShaderResourceView),
    Sampler(ID3D11SamplerState),
}

/// The textures of a lightmap resource cluster (FLightmapResourceClusterShaderParameters).
// Unverified: the member list is 4.26's; the exe's layout is not read.
#[derive(Clone, Debug, Default)]
pub struct LightmapTextures {
    pub lightmap: Option<Arc<Texture>>,
    pub sky_occlusion: Option<Arc<Texture>>,
    pub ao_material_mask: Option<Arc<Texture>>,
    pub static_shadow: Option<Arc<Texture>>,
}

/// A lightmapped item's rows and textures.
#[derive(Clone, Debug)]
pub struct LightmapRef {
    /// LightmapDataIndex.
    pub row: u32,
    pub textures: Arc<LightmapTextures>,
}

#[derive(Clone, Debug)]
pub struct DrawItem {
    /// None: the vertex factory's own geometry (sprite quads, beam and trail strips).
    pub mesh: Option<MeshSlice>,
    pub vf: crate::material::VertexFactory,
    pub vf_input: VfInput,
    pub material: MaterialId,
    pub policy: LightMapPolicy,
    /// PrimitiveId.
    pub primitive: u32,
    pub lightmap: Option<LightmapRef>,
    pub passes: PassMask,
    /// CustomDepthStencilValue.
    pub custom_stencil: u8,
    /// LightingChannelMask in bits 0-2 (written to the stencil's bits 4-6 by lighting::channel_stencil for the
    /// lighting pass); bit 7 (decal.rs RECEIVES_DECALS_BIT) set when the primitive receives decals, written to the
    /// stencil's bit 7 for the decal pass's test.
    pub lighting_channels: u8,
    /// The transform's determinant is negative: the front face flips.
    pub mirrored: bool,
    /// Translucent items: the sort's key (priority first, then the view depth, far first).
    pub sort_key: f32,
    /// Translucent items: drawn after the other items of its sort key (an effects' component mesh: cmesh.rs).
    pub after_meshes: bool,
    /// The draw's instance count: 1, an instanced static mesh's instances.
    pub instances: u32,
}

/// The frame's items by where they draw.
#[derive(Clone, Debug, Default)]
pub struct DrawList {
    pub opaque: Vec<DrawItem>,
    pub translucent: Vec<DrawItem>,
    pub custom_depth: Vec<DrawItem>,
    /// PFGCamera's priority captures' items: (the fighter's priority stack index, below 3; the item's index in
    /// `opaque`), the opaque and masked sections of the stack's fighters' meshes not left out of the captures.
    pub captures: Vec<(u8, u32)>,
}

impl DrawList {
    pub fn clear(&mut self) {
        self.opaque.clear();
        self.translucent.clear();
        self.custom_depth.clear();
        self.captures.clear();
    }
}

/// The pass uniform buffer of a pass that has one (TranslucentBasePass) and its resources by index: SRVs and
/// samplers share the index space of the buffer's resource table.
#[derive(Clone, Copy, Debug)]
pub struct PassInput<'a> {
    pub buffer: &'a ID3D11Buffer,
    pub srvs: &'a [Option<ID3D11ShaderResourceView>],
    pub samplers: &'a [Option<ID3D11SamplerState>],
}

/// Draws `items` that take part in `pass`, into the targets the caller has set, with the scene buffers bound;
/// `pass_input` fills the pass buffer's slot (None: bound null, as the base and custom depth passes have none).
/// An item with no mesh (a particle emitter's own geometry) is not this executor's: it panics.
#[allow(clippy::too_many_arguments)]
pub fn execute(
    gpu: &Gpu,
    assets: &Assets,
    scene: &SceneBuffers,
    targets: &SceneTargets,
    materials: &mut Materials,
    items: &[DrawItem],
    pass: PassKind,
    pass_input: Option<PassInput>,
) -> Result<()> {
    // SAFETY: a plain state call.
    unsafe { gpu.ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST) };
    let lightmap_sampler =
        gpu.state(StateKey::Sampler(SamplerKey::of(D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT, D3D11_TEXTURE_ADDRESS_WRAP)));
    let mut last_primitive = u32::MAX;
    for item in items.iter().filter(|i| i.passes.has(pass)) {
        let slice = item.mesh.as_ref().unwrap_or_else(|| panic!("a {:?} item with no mesh", item.vf));
        let lod = &slice.mesh.lods[slice.lod as usize];
        let section = lod.sections[slice.section as usize];
        if section.triangles == 0 {
            continue;
        }
        let inst = materials.get_mut(item.material);
        inst.bind(gpu, assets, item.vf, pass, item.policy)?;
        let inst = &*inst;
        let bp = inst.bound(item.vf, pass, item.policy).expect("bound above");
        gpu.set_shaders(bp.vs, bp.ps);
        let stencil = match pass {
            PassKind::CustomDepth => item.custom_stencil,
            _ => (item.lighting_channels & 0x80) | crate::lighting::channel_stencil(item.lighting_channels & 7),
        };
        gpu.set_states(bp.blend, if item.mirrored { bp.raster_mirrored } else { bp.raster }, bp.depth, stencil as u32);
        set_streams(gpu, scene, item, slice, bp.vs)?;
        let b = &lod.buffers;
        let index_format =
            if b.index.desc.kind == BufKind::Index16 { DXGI_FORMAT_R16_UINT } else { DXGI_FORMAT_R32_UINT };
        // SAFETY: a buffer of this device.
        unsafe { gpu.ctx.IASetIndexBuffer(&b.index.buf, index_format, 0) };
        let reads_primitive =
            [&bp.vs_binds, &bp.ps_binds].iter().any(|s| s.cbs.iter().any(|c| c.1 == Source::Primitive));
        if reads_primitive && last_primitive != item.primitive {
            let rows = scene.primitive_rows.0.get(item.primitive as usize).unwrap_or_else(|| {
                panic!("PrimitiveId {} of {} uploaded", item.primitive, scene.primitive_rows.0.len())
            });
            gpu.write_rows(&scene.primitive_buffer, rows);
            last_primitive = item.primitive;
        }
        let r = Resolver { gpu, scene, targets, inst, item, lod, lightmap_sampler, pass: pass_input };
        for (stage, binds) in [(Stage::Vertex, &bp.vs_binds), (Stage::Pixel, &bp.ps_binds)] {
            r.bind(stage, binds);
        }
        // SAFETY: the draw reads the buffers bound above; the instance is the PrimitiveId (ATTRIBUTE13's stream
        // holds 0.., so instance `primitive` reads its own id).
        unsafe {
            gpu.ctx.DrawIndexedInstanced(section.triangles * 3, item.instances, section.first_index, 0, item.primitive);
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Stage {
    Vertex,
    Pixel,
}

/// The element names the vertex factories' inputs take.
const ATTRIBUTE: &std::ffi::CStr = c"ATTRIBUTE";

fn element(index: u32, format: DXGI_FORMAT, slot: u32, per_instance: bool) -> InputElement {
    InputElement { semantic: ATTRIBUTE, index, format, slot, per_instance }
}

/// FLocalVertexFactory's input layout (manual vertex fetch): the position stream and the PrimitiveId instance
/// stream (ATTRIBUTE13); its key 1.
pub(crate) fn local_layout() -> ([InputElement; 2], u32) {
    ([element(0, DXGI_FORMAT_R32G32B32_FLOAT, 0, false), element(13, DXGI_FORMAT_R32_UINT, 1, true)], 1)
}

/// The vertex streams of a draw by slot (elements, buffers, strides and offsets), held on the stack: at most
/// D3D11's input slots, each stream one element.
pub(crate) struct Streams {
    elems: [InputElement; IA_SLOTS],
    vbs: [Option<ID3D11Buffer>; IA_SLOTS],
    strides: [u32; IA_SLOTS],
    offsets: [u32; IA_SLOTS],
    n: usize,
}

const IA_SLOTS: usize = D3D11_IA_VERTEX_INPUT_RESOURCE_SLOT_COUNT as usize;

impl Default for Streams {
    fn default() -> Streams {
        Streams {
            elems: [element(0, DXGI_FORMAT_UNKNOWN, 0, false); IA_SLOTS],
            vbs: [const { None }; IA_SLOTS],
            strides: [0; IA_SLOTS],
            offsets: [0; IA_SLOTS],
            n: 0,
        }
    }
}

impl Streams {
    /// A stream in the next slot: attribute `attr` read from `buf` at `offset`, per instance when `inst`.
    pub(crate) fn add(
        &mut self,
        attr: u32,
        format: DXGI_FORMAT,
        buf: &ID3D11Buffer,
        stride: u32,
        offset: u32,
        inst: bool,
    ) {
        let k = self.n;
        assert!(k < IA_SLOTS, "more than {IA_SLOTS} vertex streams");
        self.elems[k] = element(attr, format, k as u32, inst);
        self.vbs[k] = Some(buf.clone());
        self.strides[k] = stride;
        self.offsets[k] = offset;
        self.n += 1;
    }

    pub(crate) fn elems(&self) -> &[InputElement] {
        &self.elems[..self.n]
    }

    pub(crate) fn strides(&self) -> &[u32] {
        &self.strides[..self.n]
    }

    /// The input layout of `key` and the buffers, set from slot 0.
    pub(crate) fn bind(&self, gpu: &Gpu, vs: ShaderId, key: u32) -> Result<()> {
        gpu.set_input_layout(vs, key, self.elems())?;
        // SAFETY: the arrays hold `n` streams; the buffers are this device's.
        unsafe {
            gpu.ctx.IASetVertexBuffers(
                0,
                self.n as u32,
                Some(self.vbs.as_ptr()),
                Some(self.strides.as_ptr()),
                Some(self.offsets.as_ptr()),
            )
        };
        Ok(())
    }
}

/// The vertex factory's streams and input layout.
pub(crate) fn set_streams(
    gpu: &Gpu,
    scene: &SceneBuffers,
    item: &DrawItem,
    slice: &MeshSlice,
    vs: ShaderId,
) -> Result<()> {
    let b = &slice.mesh.lods[slice.lod as usize].buffers;
    let mut s = Streams::default();
    let key = match item.vf {
        VertexFactory::InstancedStatic => return crate::ism::set_streams(gpu, scene, item, slice, vs),
        VertexFactory::Landscape => return crate::landscape::set_streams(gpu, scene, item, slice, vs),
        // FLocalVertexFactory with manual vertex fetch: the position stream and the PrimitiveId instance stream
        // (ATTRIBUTE13); the rest is read from the LocalVF buffer's SRVs.
        VertexFactory::Local => {
            let (local, key) = local_layout();
            s.add(0, DXGI_FORMAT_R32G32B32_FLOAT, &b.position.buf, 12, 0, false);
            s.add(13, DXGI_FORMAT_R32_UINT, &scene.primitive_ids.buf, 4, 0, true);
            debug_assert_eq!(s.elems(), local);
            key
        }
        // TGPUSkinVertexFactory: TangentX 1, TangentZ 2, BlendIndices 3 (14), BlendWeights 4 (15), TexCoords 5..,
        // Color 13; a missing TexCoord or the pre- and post-skin offsets 11 and 12 read as zeros (stride 0), a mesh
        // without a colour stream reads GNullColorVertexBuffer, white (4.26 source, as the static meshes' rule).
        VertexFactory::GpuSkin { .. } => {
            s.add(0, DXGI_FORMAT_R32G32B32_FLOAT, &b.position.buf, 12, 0, false);
            let mut k: u32 = 0x811c_9dc5;
            // (the attributes the mesh has, a bit each)
            let mut have = 0u64;
            for (attr, buf, format, stride) in &b.skin {
                s.add(*attr, *format, &buf.buf, *stride, 0, false);
                have |= 1 << attr;
                k = (k ^ (attr << 8 | format.0 as u32)).wrapping_mul(0x0100_0193);
            }
            let zero = [
                (5, DXGI_FORMAT_R32G32_FLOAT),
                (6, DXGI_FORMAT_R32G32_FLOAT),
                (11, DXGI_FORMAT_R32G32B32_FLOAT),
                (12, DXGI_FORMAT_R32G32B32_FLOAT),
                (14, DXGI_FORMAT_R8G8B8A8_UINT),
                (15, DXGI_FORMAT_R8G8B8A8_UNORM),
            ];
            for (attr, format) in zero {
                if have & 1 << attr == 0 {
                    s.add(attr, format, &scene.zeros.buf, 0, 0, false);
                }
            }
            if have & 1 << 13 == 0 {
                s.add(13, DXGI_FORMAT_R8G8B8A8_UNORM, &scene.white.buf, 0, 0, false);
            }
            // TGPUSkinMorphVertexFactory: DeltaPosition 9 and DeltaTangentZ 10, the morph vertex buffer's two halves
            // (the base pass VS adds them to the position and TangentZ before the skinning).
            if let VertexFactory::GpuSkin { morph: true, .. } = item.vf {
                let VfInput::Bones { morph: Some(m), .. } = &item.vf_input else {
                    panic!("a morph factory item of {} with no morph vertex buffer", slice.mesh.path)
                };
                for (attr, at) in [(9, 0), (10, 12)] {
                    s.add(attr, DXGI_FORMAT_R32G32B32_FLOAT, &m.buf, 24, at, false);
                }
                k = (k ^ (9 << 8)).wrapping_mul(0x0100_0193);
            }
            k | 2
        }
        vf => {
            return Err(Error::Format {
                what: slice.mesh.path.clone(),
                why: format!("{} is not drawn by the mesh executor", vf.type_name()),
            });
        }
    };
    s.bind(gpu, vs, key)
}

/// A shader stage's register tables, held on the stack (D3D11's slot counts): each as long as its highest register
/// bound plus one (one at least), set from register 0.
pub(crate) struct StageTables {
    cbs: [Option<ID3D11Buffer>; CB_SLOTS],
    srvs: [Option<ID3D11ShaderResourceView>; SRV_SLOTS],
    samplers: [Option<ID3D11SamplerState>; SAMPLER_SLOTS],
    len: [usize; 3],
}

const CB_SLOTS: usize = D3D11_COMMONSHADER_CONSTANT_BUFFER_API_SLOT_COUNT as usize;
const SRV_SLOTS: usize = D3D11_COMMONSHADER_INPUT_RESOURCE_SLOT_COUNT as usize;
const SAMPLER_SLOTS: usize = D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT as usize;

impl StageTables {
    /// The tables of a stage's registers (`cbs`, `srvs`, `samplers`: (register, source)), each source resolved.
    pub(crate) fn of<S: Copy>(
        cbs: &[(u32, S)],
        srvs: &[(u32, S)],
        samplers: &[(u32, S)],
        buffer: impl Fn(S) -> Option<ID3D11Buffer>,
        srv: impl Fn(S) -> Option<ID3D11ShaderResourceView>,
        sampler: impl Fn(S) -> Option<ID3D11SamplerState>,
    ) -> StageTables {
        let len = |v: &[(u32, S)]| v.iter().map(|c| c.0 as usize + 1).max().unwrap_or(0).max(1);
        let mut t = StageTables {
            cbs: [const { None }; CB_SLOTS],
            srvs: [const { None }; SRV_SLOTS],
            samplers: [const { None }; SAMPLER_SLOTS],
            len: [len(cbs), len(srvs), len(samplers)],
        };
        for &(r, s) in cbs {
            t.cbs[r as usize] = buffer(s);
        }
        for &(r, s) in srvs {
            t.srvs[r as usize] = srv(s);
        }
        for &(r, s) in samplers {
            t.samplers[r as usize] = sampler(s);
        }
        t
    }

    /// The tables bound to the vertex shader's registers, or the pixel shader's when `pixel`.
    pub(crate) fn set(&self, gpu: &Gpu, pixel: bool) {
        let ctx = &gpu.ctx;
        let [c, r, s] = self.len;
        let (cbs, srvs, samplers) = (&self.cbs[..c], &self.srvs[..r], &self.samplers[..s]);
        // SAFETY: the resources are this device's; each slice starts at register 0.
        unsafe {
            if pixel {
                ctx.PSSetConstantBuffers(0, Some(cbs));
                ctx.PSSetShaderResources(0, Some(srvs));
                ctx.PSSetSamplers(0, Some(samplers));
            } else {
                ctx.VSSetConstantBuffers(0, Some(cbs));
                ctx.VSSetShaderResources(0, Some(srvs));
                ctx.VSSetSamplers(0, Some(samplers));
            }
        }
    }
}

impl Bound {
    fn buffer(self) -> Option<ID3D11Buffer> {
        if let Bound::Buffer(b) = self { Some(b) } else { None }
    }

    fn srv(self) -> Option<ID3D11ShaderResourceView> {
        if let Bound::Srv(v) = self { Some(v) } else { None }
    }

    fn sampler(self) -> Option<ID3D11SamplerState> {
        if let Bound::Sampler(v) = self { Some(v) } else { None }
    }
}

/// What a draw's register sources resolve to.
struct Resolver<'a> {
    gpu: &'a Gpu,
    scene: &'a SceneBuffers,
    targets: &'a SceneTargets,
    inst: &'a MaterialInstance,
    item: &'a DrawItem,
    lod: &'a crate::assets::MeshLod,
    lightmap_sampler: StateId,
    pass: Option<PassInput<'a>>,
}

impl Resolver<'_> {
    fn buffer(&self, s: Source) -> Option<ID3D11Buffer> {
        match s {
            Source::View(_) => Some(self.scene.view.buf.clone()),
            Source::Primitive => Some(self.scene.primitive_buffer.buf.clone()),
            Source::Material(_) => self.inst.buffer.as_ref().map(|b| b.buf.clone()),
            Source::Collection(n) => self.inst.collections.get(n as usize).map(|b| b.buf.clone()),
            Source::LocalVf(_) => self.lod.buffers.local_vf.as_ref().map(|b| b.buf.clone()),
            Source::Globals => match self.item.vf {
                VertexFactory::GpuSkin { extra_influences, .. } => {
                    Some(self.scene.skin_globals[extra_influences as usize].buf.clone())
                }
                VertexFactory::InstancedStatic => crate::ism::globals(self.item),
                _ => None,
            },
            Source::Pass(_) => self.pass.map(|p| p.buffer.clone()),
            Source::InstanceVf(k) => crate::ism::resource(self.item, k).buffer(),
            Source::LandscapeVf(_) | Source::LandscapeLod(_) => crate::landscape::resource(self.item, s).buffer(),
            Source::PrecomputedLighting => match self.item.vf {
                VertexFactory::Landscape => crate::landscape::resource(self.item, s).buffer(),
                VertexFactory::InstancedStatic => match &self.item.vf_input {
                    VfInput::StaticInstances(i) => i.lighting(),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    }

    fn srv(&self, s: Source) -> Option<ID3D11ShaderResourceView> {
        match s {
            Source::View(k) => match VIEW_RESOURCES.get(k as usize).copied() {
                Some("PrimitiveSceneData") => self.scene.primitives.srv.clone(),
                Some("LightmapSceneData") => self.scene.lightmaps.srv.clone(),
                Some(name) => {
                    self.scene.view_textures.iter().rev().find(|(n, _)| *n == name).and_then(|(_, t)| t.srv.clone())
                }
                None => None,
            },
            Source::Material(k) => match self.inst.resources.get(k as usize) {
                Some(MaterialResource::Texture(TextureSource::Asset(t))) => t.srv.clone(),
                Some(MaterialResource::Texture(TextureSource::Target(id))) => self.targets.get(*id).srv.clone(),
                _ => None,
            },
            // VertexFetch_TexCoordBuffer, PositionBuffer, PreSkinPositionBuffer, PackedTangentsBuffer,
            // ColorComponentsBuffer.
            // Unverified: the order is read from the base pass vertex shaders' fetches, not from the exe's LocalVF
            // member table; the position buffers are not made (no shader of the reach reads them).
            Source::LocalVf(k) => {
                let b = &self.lod.buffers;
                match k {
                    0 => b.tex_coords.as_ref(),
                    3 => b.tangents.as_ref(),
                    4 => b.color.as_ref(),
                    _ => None,
                }
                .and_then(|b| b.srv.clone())
            }
            // LightMapTexture, SkyOcclusionTexture, AOMaterialMaskTexture, StaticShadowTexture.
            Source::Lightmap(k) => {
                let t = &self.item.lightmap.as_ref()?.textures;
                match k {
                    0 => t.lightmap.as_ref(),
                    1 => t.sky_occlusion.as_ref(),
                    2 => t.ao_material_mask.as_ref(),
                    3 => t.static_shadow.as_ref(),
                    _ => None,
                }
                .and_then(|t| t.srv.clone())
            }
            Source::Bones => match &self.item.vf_input {
                VfInput::Bones { bones, .. } => bones.srv.clone(),
                _ => panic!("a GPUSkin item with no bones"),
            },
            Source::Pass(k) => self.pass?.srvs.get(k as usize).cloned().flatten(),
            Source::InstanceVf(k) => crate::ism::resource(self.item, k).srv(),
            Source::LandscapeVf(_) | Source::LandscapeLod(_) => crate::landscape::resource(self.item, s).srv(),
            _ => None,
        }
    }

    fn sampler(&self, s: Source) -> Option<ID3D11SamplerState> {
        let id = match s {
            Source::View(k) => {
                let name = VIEW_RESOURCES.get(k as usize)?;
                self.gpu.state(StateKey::Sampler(view_sampler(name)))
            }
            Source::Material(k) => match self.inst.resources.get(k as usize) {
                Some(MaterialResource::Sampler(id)) => *id,
                _ => return None,
            },
            // LightMapSampler, SkyOcclusionSampler, AOMaterialMaskSampler, StaticShadowTextureSampler.
            // Unverified: bilinear wrap for each.
            Source::Lightmap(9..=12) => self.lightmap_sampler,
            Source::Pass(k) => return self.pass?.samplers.get(k as usize).cloned().flatten(),
            Source::InstanceVf(k) => return crate::ism::resource(self.item, k).sampler(),
            Source::LandscapeVf(_) | Source::LandscapeLod(_) => {
                return crate::landscape::resource(self.item, s).sampler();
            }
            _ => return None,
        };
        Some(self.gpu.sampler(id))
    }

    fn bind(&self, stage: Stage, b: &StageBinds) {
        let t = StageTables::of(&b.cbs, &b.srvs, &b.samplers, |s| self.buffer(s), |s| self.srv(s), |s| self.sampler(s));
        t.set(self.gpu, matches!(stage, Stage::Pixel));
    }
}
