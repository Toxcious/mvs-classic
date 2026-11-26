//! A material drawn: its cooked shaders for a vertex factory and pass (FMeshPassProcessor's choice), its uniform
//! buffer filled by the preshaders over the instance's parameters, its textures and samplers bound by the shaders'
//! resource tables, and its draw state (the contexts and the policy).

use std::collections::HashMap;
use std::sync::Arc;

use mvs_d3d::Direct3D11::*;
use mvs_material::material::{Address, Filter, MaterialTexture, Sampler, TextureBind};
use mvs_material::{Frequency, ShaderCode};

use crate::assets::Assets;
use crate::gpu::{
    BlendKey, BlendTarget, BufDesc, BufKind, Buffer, DepthKey, Gpu, RasterKey, SamplerKey, ShaderId, StateId, StateKey,
    StencilFace, StencilKey, Texture,
};
use crate::targets::TargetId;
use crate::view::view_resource;
use crate::{Error, Result};

/// The vertex factory a mesh is drawn with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VertexFactory {
    /// FLocalVertexFactory.
    Local,
    /// FInstancedStaticMeshVertexFactory.
    InstancedStatic,
    /// FLandscapeVertexFactory.
    Landscape,
    /// TGPUSkinVertexFactory; `extra_influences`: eight influences a vertex; `morph`: TGPUSkinMorphVertexFactory (the
    /// morph vertex buffer's deltas added before the skinning).
    GpuSkin { extra_influences: bool, morph: bool },
    /// FParticleSpriteVertexFactory.
    Sprite,
    /// FMeshParticleVertexFactory.
    MeshParticle,
    /// FParticleBeamTrailVertexFactory.
    BeamTrail,
}

impl VertexFactory {
    /// The vertex factory type's name, as the shader maps hash it.
    pub fn type_name(self) -> &'static str {
        match self {
            VertexFactory::Local => "FLocalVertexFactory",
            VertexFactory::InstancedStatic => "FInstancedStaticMeshVertexFactory",
            VertexFactory::Landscape => "FLandscapeVertexFactory",
            // TGPUSkinVertexFactory<GPUSkinBoneInfluenceType::DefaultBoneInfluence>: eight influences by its
            // NumBoneInfluences parameter.
            VertexFactory::GpuSkin { morph: false, .. } => "TGPUSkinVertexFactoryDefault",
            VertexFactory::GpuSkin { morph: true, .. } => "TGPUSkinMorphVertexFactoryDefault",
            VertexFactory::Sprite => "FParticleSpriteVertexFactory",
            VertexFactory::MeshParticle => "FMeshParticleVertexFactory",
            VertexFactory::BeamTrail => "FParticleBeamTrailVertexFactory",
        }
    }
}

/// A mesh pass a material draws in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PassKind {
    Base,
    DepthOnly,
    CustomDepth,
    Translucent,
    Distortion,
    Decal,
    /// A spot light's perspective shadow depth (lighting/shadow.rs).
    ShadowDepth,
}

/// The shadow depth shaders of a spot light's (perspective) shadow: the PerspectiveCorrect pair, standalone rows
/// (no pipeline). The trailing `false` is the tessellation flag.
const SHADOW_DEPTH_VS: &str = "TShadowDepthVSVertexShadowDepth_PerspectiveCorrectfalse";
const SHADOW_DEPTH_PS: &str = "TShadowDepthPSPixelShadowDepth_PerspectiveCorrectfalse";

/// FBasePassMeshProcessor::AddMeshBatch's light-map policy (1419234e0): the base pass pixel shader type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LightMapPolicy {
    NoLightMap,
    PrecomputedIrradianceVolumeIndirectLighting,
    CachedVolumeIndirectLighting,
    CachedPointIndirectLighting,
    SelfShadowedTranslucency,
    SelfShadowedCachedPointIndirectLighting,
    SelfShadowedVolumetricLightmap,
    LqLightmap,
    HqLightmap,
    DistanceFieldShadowsAndHqLightmap,
}

impl LightMapPolicy {
    /// The policy's part of the base pass shader type names (TBasePassVS<policy>, TBasePassPS<policy>; 4.26's
    /// names, which the shader maps' hashed type names match).
    pub fn type_suffix(self) -> &'static str {
        match self {
            LightMapPolicy::NoLightMap => "FNoLightMapPolicy",
            LightMapPolicy::PrecomputedIrradianceVolumeIndirectLighting => {
                "FPrecomputedVolumetricLightmapLightingPolicy"
            }
            LightMapPolicy::CachedVolumeIndirectLighting => "FCachedVolumeIndirectLightingPolicy",
            LightMapPolicy::CachedPointIndirectLighting => "FCachedPointIndirectLightingPolicy",
            LightMapPolicy::SelfShadowedTranslucency => "FSelfShadowedTranslucencyPolicy",
            LightMapPolicy::SelfShadowedCachedPointIndirectLighting => "FSelfShadowedCachedPointIndirectLightingPolicy",
            LightMapPolicy::SelfShadowedVolumetricLightmap => "FSelfShadowedVolumetricLightmapPolicy",
            LightMapPolicy::LqLightmap => "TLightMapPolicyLQ",
            LightMapPolicy::HqLightmap => "TLightMapPolicyHQ",
            LightMapPolicy::DistanceFieldShadowsAndHqLightmap => "TDistanceFieldShadowsAndLightMapPolicyHQ",
        }
    }
}

/// EComponentMobility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mobility {
    Static,
    Stationary,
    Movable,
}

/// The indirect lighting cache's policy a proxy's allocation takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndirectCache {
    Volume,
    Point,
}

/// What the policy choice reads of a surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightMapSurface {
    pub lit: bool,
    /// The blend mode is above Masked.
    pub translucent: bool,
    pub light_cache: bool,
    pub cast_volumetric_translucent_shadow: bool,
    pub texture_light_map: bool,
    pub hq_interaction: bool,
    pub texture_shadow_map: bool,
    pub mobility: Mobility,
    pub preview_or_forced_volumetric: bool,
    pub indirect_cache: Option<IndirectCache>,
}

/// What the policy choice reads of the scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightMapScene {
    pub static_lighting: bool,
    pub hq_lightmaps: bool,
    pub low_quality_lightmaps: bool,
    pub volumetric_lightmap: bool,
    pub precomputed_light_volumes: bool,
}

impl LightMapScene {
    /// The scene the build's stages give the choice: static lighting, HQ and LQ
    /// lightmaps allowed, a volumetric lightmap with data, no precomputed light volumes.
    pub const STAGE: LightMapScene = LightMapScene {
        static_lighting: true,
        hq_lightmaps: true,
        low_quality_lightmaps: true,
        volumetric_lightmap: true,
        precomputed_light_volumes: false,
    };
}

/// FBasePassMeshProcessor::AddMeshBatch's choice (1419234e0). The light-map
/// interaction is read only for a lit material whose batch has a light-cache interface, when static lighting is
/// allowed; lit translucency that casts volumetric translucent shadow takes a self-shadow policy; then a texture
/// light map gives the HQ lightmap (the feature level and the interaction both allowing it; else the LQ one), with
/// distance-field shadows when the shadow map is a texture; without one, a Stationary or Movable (or preview-lit,
/// or forced volumetric) proxy in a scene whose volumetric lightmap holds data gives the volumetric policy, else the
/// indirect lighting cache's, else none.
pub fn light_map_policy(s: &LightMapSurface, scene: &LightMapScene) -> LightMapPolicy {
    use LightMapPolicy as P;
    if !s.lit {
        return P::NoLightMap;
    }
    if s.translucent && s.cast_volumetric_translucent_shadow {
        // (1419086b0, 1419080b0, 1419083b0. Unverified: the three addresses matched to 4.26's three self-shadow
        // policies by the branch order alone)
        if scene.static_lighting && scene.volumetric_lightmap {
            return P::SelfShadowedVolumetricLightmap;
        }
        if scene.precomputed_light_volumes {
            return P::SelfShadowedCachedPointIndirectLighting;
        }
        return P::SelfShadowedTranslucency;
    }
    if s.light_cache && scene.static_lighting && s.texture_light_map {
        if scene.hq_lightmaps && s.hq_interaction {
            return if s.texture_shadow_map { P::DistanceFieldShadowsAndHqLightmap } else { P::HqLightmap };
        }
        return if scene.low_quality_lightmaps { P::LqLightmap } else { P::NoLightMap };
    }
    if scene.static_lighting
        && scene.volumetric_lightmap
        && (s.mobility != Mobility::Static || s.preview_or_forced_volumetric)
    {
        return P::PrecomputedIrradianceVolumeIndirectLighting;
    }
    if scene.precomputed_light_volumes
        && let Some(c) = s.indirect_cache
    {
        return match c {
            IndirectCache::Volume => P::CachedVolumeIndirectLighting,
            IndirectCache::Point => P::CachedPointIndirectLighting,
        };
    }
    P::NoLightMap
}

/// UMaterial::GetDefaultMaterial(MD_Surface): BaseEngine.ini's DefaultMaterialName.
const DEFAULT_MATERIAL: &str = "/Engine/EngineMaterials/WorldGridMaterial.WorldGridMaterial";

const EFFECTS: &[PassKind] = &[PassKind::Base, PassKind::Translucent, PassKind::Distortion];
const LIT: &[PassKind] = &[PassKind::Base, PassKind::DepthOnly, PassKind::CustomDepth, PassKind::Translucent];
/// A component drawn for its custom depth pass alone (the `customdepth` context).
const CUSTOM_DEPTH: &[PassKind] = &[PassKind::CustomDepth];

/// The vertex factory and passes of a draw context of the material kind's draw refs: the
/// passes a draw of the context may take, its material's blend mode choosing among them. Panics on a context with
/// no vertex factory of the enum (`spline`'s FSplineMeshVertexFactory, `slate`'s material shaders, `decal`'s
/// FDeferredDecalPS, which has none, the build drawing decals without the DBuffer) or one the table does not hold.
pub fn context_of(ctx: &str) -> (VertexFactory, &'static [PassKind]) {
    match ctx {
        "sprite" | "bannersprite" => (VertexFactory::Sprite, EFFECTS),
        "mesh" => (VertexFactory::MeshParticle, EFFECTS),
        "trail" => (VertexFactory::BeamTrail, EFFECTS),
        "local" | "bannerlocal" => (VertexFactory::Local, EFFECTS),
        "customdepth" => (VertexFactory::Local, CUSTOM_DEPTH),
        "locallm" | "static" | "shadowmap" => (VertexFactory::Local, LIT),
        "skin" => (VertexFactory::GpuSkin { extra_influences: false, morph: false }, LIT),
        "landscape" => (VertexFactory::Landscape, LIT),
        _ => panic!("draw context {ctx}: no vertex factory of the renderer's"),
    }
}

/// A texture parameter's value: an asset, or a target of this frame (scene colour, depth).
#[derive(Clone, Debug)]
pub enum TextureSource {
    Asset(Arc<Texture>),
    Target(TargetId),
}

/// Where a shader's constant buffer, SRV or sampler register is filled from at the draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The View struct's resource k (its constants: the View buffer).
    View(u16),
    /// The Primitive buffer (the draw's GPUScene rows as a uniform buffer).
    Primitive,
    /// The material buffer's resource k (its constants: the material buffer).
    Material(u16),
    /// MaterialCollection<n>'s buffer.
    Collection(u16),
    /// The LocalVF buffer's resource k (its constants: the LOD's LocalVF buffer).
    LocalVf(u16),
    /// FLightmapResourceClusterShaderParameters' resource k.
    Lightmap(u16),
    /// The loose ($Globals) buffer: GPUSkin's NumBoneInfluences, a Slate pass's parameters. As an SRV or a sampler:
    /// the shader's loose texture parameter (a Slate pass's element texture, the font atlas).
    Globals,
    /// GPUSkin's BoneMatrices.
    Bones,
    /// TranslucentBasePass's resource k (its constants: the pass buffer the translucent passes fill).
    Pass(u16),
    /// The InstanceVF buffer's resource k (its constants: the instanced static mesh component's InstanceVF buffer).
    InstanceVf(u16),
    /// The LandscapeParameters buffer's resource k (its constants: the landscape component's buffer).
    LandscapeVf(u16),
    /// The landscape's per-view LOD buffer's resource k (the section LOD and LOD bias SRVs; its constants: the
    /// buffer).
    LandscapeLod(u16),
    /// PrecomputedLightingBuffer (FPrecomputedLightingUniformParameters, 0xf0: the 15 lightmap rows of the draw's
    /// component): the lightmap rows of a vertex factory with no GPUScene lightmap fetch (the landscape's, the
    /// instanced static mesh's).
    PrecomputedLighting,
    /// A buffer or resource the renderer does not fill (bound null).
    None,
}

/// One stage's bindings: (register, source) for constant buffers, SRVs and samplers.
#[derive(Clone, Debug, Default)]
pub struct StageBinds {
    pub cbs: Vec<(u32, Source)>,
    pub srvs: Vec<(u32, Source)>,
    pub samplers: Vec<(u32, Source)>,
}

/// A material bound for one (vertex factory, pass, policy): its shaders, the state, and its resources.
#[derive(Debug)]
pub struct BoundPass {
    pub vs: ShaderId,
    /// None for a depth-only pass with no pixel shader.
    pub ps: Option<ShaderId>,
    pub blend: StateId,
    pub raster: StateId,
    pub depth: StateId,
    /// The rasterizer state of a mirrored transform (the cull mode reversed).
    pub raster_mirrored: StateId,
    /// What each stage's registers read.
    pub vs_binds: StageBinds,
    pub ps_binds: StageBinds,
}

/// TSlateMaterialShaderPS's material shader type: the Slate element type a UI material draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SlateMaterialShader {
    Default,
    Border,
    GrayscaleFont,
    ColorFont,
    Custom,
}

/// A material resource's value.
#[derive(Clone, Debug)]
pub enum MaterialResource {
    Texture(TextureSource),
    Sampler(StateId),
}

/// A material and its instance's parameters (a dynamic material instance over the material).
#[derive(Debug)]
pub struct MaterialInstance {
    pub material: Arc<mvs_material::Material>,
    scalars: Vec<f64>,
    vectors: Vec<[f64; 4]>,
    /// The material buffer's resources, by index.
    pub resources: Vec<MaterialResource>,
    /// The material buffer (None: the layout has no constants).
    pub buffer: Option<Buffer>,
    /// MaterialCollection<n>'s buffers.
    pub collections: Vec<Buffer>,
    dirty: bool,
    passes: HashMap<(VertexFactory, PassKind, LightMapPolicy), BoundPass>,
    slate_passes: HashMap<(SlateMaterialShader, bool), BoundPass>,
    /// Collection rows set by `Materials::set_collection` before this instance joined, written at the next bind.
    pending_collections: Vec<(usize, Vec<[u32; 4]>)>,
}

/// A material texture's sampler (in terms of TStaticSamplerState).
// Unverified: the anisotropy (8, r.MaxAnisotropy's default) and the mip filter of the anisotropic filters.
fn sampler_key(s: &Sampler) -> SamplerKey {
    let filter = match s.filter {
        Filter::Point => D3D11_FILTER_MIN_MAG_MIP_POINT,
        Filter::Bilinear => D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT,
        Filter::Trilinear => D3D11_FILTER_MIN_MAG_MIP_LINEAR,
        Filter::AnisotropicPoint | Filter::AnisotropicLinear => D3D11_FILTER_ANISOTROPIC,
    };
    let address = |a: Address| match a {
        Address::Wrap => D3D11_TEXTURE_ADDRESS_WRAP,
        Address::Clamp => D3D11_TEXTURE_ADDRESS_CLAMP,
        Address::Mirror => D3D11_TEXTURE_ADDRESS_MIRROR,
    };
    let mut k = SamplerKey::of(filter, address(s.address_x));
    k.address[1] = address(s.address_y).0 as u8;
    if filter == D3D11_FILTER_ANISOTROPIC {
        k.max_anisotropy = crate::texture_quality::max_anisotropy();
    }
    k
}

/// The world texture group's shared sampler (ESamplerSourceMode 1 Wrap, 2 Clamp): TEXTUREGROUP_World's filter.
// Unverified: TEXTUREGROUP_World's aniso filter and anisotropy 8 are 4.26's BaseDeviceProfiles defaults.
fn world_sampler(address: D3D11_TEXTURE_ADDRESS_MODE) -> SamplerKey {
    let mut k = SamplerKey::of(D3D11_FILTER_ANISOTROPIC, address);
    k.max_anisotropy = crate::texture_quality::max_anisotropy();
    k
}

/// A View sampler member's state, by its name.
// Unverified: the shared and the volumetric lightmap samplers' filters are 4.26's (TStaticSamplerState in
// FViewUniformShaderParameters' setup).
// MaterialTextureBilinearWrapedSampler / ...ClampedSampler (View resources 0 and 1, what a texture sample through
// the world group's shared Wrap or Clamp binds) are not bilinear: FViewInfo::SetupUniformBufferParameters (141cdb139)
// stores the Wrap_WorldGroupSettings / Clamp_WorldGroupSettings shared sampler states (InitializeSharedSamplerStates
// 142d53d70) when the view's material mip bias is the global one (a view sampler of the view's own filter and bias
// only under a temporal upsample, which the port has not), so the world group's filter (world_sampler).
// Deviation: these are sampled bilinear, and
// anisotropic only on the stage grid.
pub fn view_sampler(name: &str) -> SamplerKey {
    use D3D11_TEXTURE_ADDRESS_CLAMP as C;
    use D3D11_TEXTURE_ADDRESS_WRAP as W;
    match name {
        "MaterialTextureBilinearWrapedSampler" => world_sampler(W),
        "MaterialTextureBilinearClampedSampler" => world_sampler(C),
        // (PerlinNoiseGradientTextureSampler: FViewInfo::SetupUniformBufferParameters 141cdaff6 stores the static
        // sampler resource 144f8fc20 (its RHI at +0x10), whose InitRHI 1418fccb0 makes SF_Point, AM_Wrap x 3)
        "SharedPointWrappedSampler" | "PerlinNoiseGradientTextureSampler" => {
            SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_POINT, W)
        }
        "SharedPointClampedSampler" => SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_POINT, C),
        "SharedBilinearWrappedSampler" => SamplerKey::of(D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT, W),
        "SharedTrilinearWrappedSampler" | "PerlinNoise3DTextureSampler" => {
            SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_LINEAR, W)
        }
        "SharedTrilinearClampedSampler" => SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_LINEAR, C),
        _ => SamplerKey::of(D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT, C),
    }
}

/// The opaque base pass's draw state: depth written and tested GREATER_EQUAL (reversed Z), the stencil written as
/// SetDepthStencilStateForBasePass (14193d4a0) has it: write mask 0xf0, the receive-decals bit in bit 7 and the
/// lighting channels in bits 4-6 (lighting::channel_stencil); bit 0, the decal pass's sandbox bit, stays 0.
// Unverified: no depth prepass (the base pass writes depth; r.EarlyZPass is not read from the exe).
pub fn base_depth() -> DepthKey {
    let replace = StencilFace {
        func: D3D11_COMPARISON_ALWAYS.0 as u8,
        fail: D3D11_STENCIL_OP_KEEP.0 as u8,
        depth_fail: D3D11_STENCIL_OP_KEEP.0 as u8,
        pass: D3D11_STENCIL_OP_REPLACE.0 as u8,
    };
    DepthKey {
        depth_write: true,
        depth_func: D3D11_COMPARISON_GREATER_EQUAL.0 as u8,
        stencil: Some(StencilKey { front: replace, back: replace, read_mask: 0xff, write_mask: 0xf0 }),
    }
}

/// The custom depth pass's draw state (the exe's custom depth state 144f8ceb8 / 144f99d30): depth written and tested
/// CF_GreaterEqual, stencil Always / Replace by the draw's CustomDepthStencilValue, read and write masks 0xff.
pub fn custom_depth_state() -> DepthKey {
    let replace = StencilFace {
        func: D3D11_COMPARISON_ALWAYS.0 as u8,
        fail: D3D11_STENCIL_OP_KEEP.0 as u8,
        depth_fail: D3D11_STENCIL_OP_KEEP.0 as u8,
        pass: D3D11_STENCIL_OP_REPLACE.0 as u8,
    };
    DepthKey {
        depth_write: true,
        depth_func: D3D11_COMPARISON_GREATER_EQUAL.0 as u8,
        stencil: Some(StencilKey { front: replace, back: replace, read_mask: 0xff, write_mask: 0xff }),
    }
}

/// FMeshPassProcessor::ComputeMeshCullMode on UE's D3D11 rasterizer (FrontCounterClockwise true): CM_None for a
/// two-sided material, else CM_CW (D3D11 back) or, mirrored, CM_CCW (front).
fn raster_key(two_sided: bool, mirrored: bool) -> RasterKey {
    let cull = if two_sided {
        D3D11_CULL_NONE
    } else if mirrored {
        D3D11_CULL_FRONT
    } else {
        D3D11_CULL_BACK
    };
    RasterKey {
        fill: D3D11_FILL_SOLID.0 as u8,
        cull: cull.0 as u8,
        front_ccw: true,
        depth_bias: 0,
        slope_scaled_depth_bias: 0,
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

/// A material texture parameter's texture as the build binds it.
fn texture_of(assets: &Assets, t: &MaterialTexture) -> Result<Arc<Texture>> {
    Ok(match &t.texture {
        TextureBind::Texture { path, .. } => assets.texture(path)?,
        TextureBind::White { .. } => assets.white(t.texture_type),
        // A render target the game fills: the far-depth stand-in until one is set (the fighters'
        // LocalPlayerSceneDepth).
        TextureBind::RenderTarget { .. } => assets.far_depth(),
    })
}

/// SetTranslucentRenderState (14193f5a0): the blend of a translucent blend mode. BLEND_Modulate is the exe's static
/// state 144c3a100: rgb = src x dst, the destination's alpha kept (colour writes only).
// Unverified: the other modes' states (144c3a0d0 / 0e8 / 0b8 / 118) are not decoded; their factors are stock 4.26's,
// whose alpha (Zero, InvSrcAlpha) leaves separate translucency's alpha the coverage left of the scene.
pub fn translucent_blend(blend: &str) -> Option<BlendKey> {
    let b = |s: D3D11_BLEND| s.0 as u8;
    let all = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    let rgb =
        (D3D11_COLOR_WRITE_ENABLE_RED.0 | D3D11_COLOR_WRITE_ENABLE_GREEN.0 | D3D11_COLOR_WRITE_ENABLE_BLUE.0) as u8;
    let (src, dst, a_src, a_dst, write_mask) = match blend {
        "BLEND_Translucent" => {
            (D3D11_BLEND_SRC_ALPHA, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ZERO, D3D11_BLEND_INV_SRC_ALPHA, all)
        }
        "BLEND_Additive" => (D3D11_BLEND_ONE, D3D11_BLEND_ONE, D3D11_BLEND_ZERO, D3D11_BLEND_INV_SRC_ALPHA, all),
        "BLEND_Modulate" => (D3D11_BLEND_DEST_COLOR, D3D11_BLEND_ZERO, D3D11_BLEND_ONE, D3D11_BLEND_ZERO, rgb),
        "BLEND_AlphaComposite" => {
            (D3D11_BLEND_ONE, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ZERO, D3D11_BLEND_INV_SRC_ALPHA, all)
        }
        "BLEND_AlphaHoldout" => {
            (D3D11_BLEND_ZERO, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ZERO, D3D11_BLEND_INV_SRC_ALPHA, all)
        }
        _ => return None,
    };
    let t = BlendTarget {
        color_op: D3D11_BLEND_OP_ADD.0 as u8,
        color_src: b(src),
        color_dst: b(dst),
        alpha_op: D3D11_BLEND_OP_ADD.0 as u8,
        alpha_src: b(a_src),
        alpha_dst: b(a_dst),
        write_mask,
    };
    Some(BlendKey { targets: [t; 8], alpha_to_coverage: false })
}

/// A blend state of (colour source, colour destination, alpha source, alpha destination) factors, adds, `write`.
fn blend_of(f: [D3D11_BLEND; 4], write_mask: u8) -> BlendKey {
    let b = |s: D3D11_BLEND| s.0 as u8;
    let t = BlendTarget {
        color_op: D3D11_BLEND_OP_ADD.0 as u8,
        color_src: b(f[0]),
        color_dst: b(f[1]),
        alpha_op: D3D11_BLEND_OP_ADD.0 as u8,
        alpha_src: b(f[2]),
        alpha_dst: b(f[3]),
        write_mask,
    };
    BlendKey { targets: [t; 8], alpha_to_coverage: false }
}

/// FSlateRHIRenderingPolicy::DrawElements' (142566370) blend state of an element batch by its DrawFlags: 0x01
/// NoBlending opaque (144be5e60); else 0x02 PreMultipliedAlpha colour and alpha (One, InvSrcAlpha) (144c3b4c0);
/// else colour (SrcAlpha, InvSrcAlpha), alpha (One, InvSrcAlpha) (144c44bf8).
pub fn slate_blend(no_blending: bool, premultiplied: bool) -> BlendKey {
    use D3D11_BLEND_INV_SRC_ALPHA as INV_SRC_A;
    let all = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    if no_blending {
        BlendKey::OPAQUE
    } else if premultiplied {
        blend_of([D3D11_BLEND_ONE, INV_SRC_A, D3D11_BLEND_ONE, INV_SRC_A], all)
    } else {
        blend_of([D3D11_BLEND_SRC_ALPHA, INV_SRC_A, D3D11_BLEND_ONE, INV_SRC_A], all)
    }
}

/// DrawElements' blend state of a UI material's batch, by the material's blend mode alone: Translucent colour
/// (SrcAlpha, InvSrcAlpha), alpha (InvDestAlpha, One) (144c55ba0); Additive (One, One) on both (144c3a1e8); Modulate
/// (Zero, SrcColor) on the colour, RGB written (144c55bb8); AlphaComposite (One, InvSrcAlpha) on both (144c3b4c0);
/// AlphaHoldout (Zero, InvSrcAlpha) on both (144c3b4d8); Opaque and Masked unblended (144be5e60). A batch with a
/// second texture (a font material's atlas) is Translucent whatever the mode.
pub fn slate_material_blend(blend: &str, second_texture: bool) -> Option<BlendKey> {
    use D3D11_BLEND_INV_SRC_ALPHA as INV_SRC_A;
    let all = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    let rgb =
        (D3D11_COLOR_WRITE_ENABLE_RED.0 | D3D11_COLOR_WRITE_ENABLE_GREEN.0 | D3D11_COLOR_WRITE_ENABLE_BLUE.0) as u8;
    let translucent = blend_of([D3D11_BLEND_SRC_ALPHA, INV_SRC_A, D3D11_BLEND_INV_DEST_ALPHA, D3D11_BLEND_ONE], all);
    if second_texture {
        return Some(translucent);
    }
    Some(match blend {
        "BLEND_Opaque" | "BLEND_Masked" => BlendKey::OPAQUE,
        "BLEND_Translucent" => translucent,
        "BLEND_Additive" => blend_of([D3D11_BLEND_ONE, D3D11_BLEND_ONE, D3D11_BLEND_ONE, D3D11_BLEND_ONE], all),
        "BLEND_Modulate" => blend_of([D3D11_BLEND_ZERO, D3D11_BLEND_SRC_COLOR, D3D11_BLEND_ZERO, D3D11_BLEND_ONE], rgb),
        "BLEND_AlphaComposite" => blend_of([D3D11_BLEND_ONE, INV_SRC_A, D3D11_BLEND_ONE, INV_SRC_A], all),
        "BLEND_AlphaHoldout" => blend_of([D3D11_BLEND_ZERO, INV_SRC_A, D3D11_BLEND_ZERO, INV_SRC_A], all),
        _ => return None,
    })
}

/// A collection buffer's rows rewritten (a short list: the rest of the buffer zero).
fn write_collection(gpu: &Gpu, b: &Buffer, rows: &[[u32; 4]]) {
    let mut out = vec![[0u32; 4]; (b.desc.size / 16) as usize];
    for (o, r) in out.iter_mut().zip(rows) {
        *o = *r;
    }
    gpu.write_rows(b, &out);
}

/// Which source a uniform buffer name gives a register.
fn buffer_source(name: &str, register: u32) -> Source {
    match name {
        "View" => Source::View(u16::MAX),
        "Primitive" => Source::Primitive,
        "Material" => Source::Material(u16::MAX),
        "LocalVF" => Source::LocalVf(u16::MAX),
        "LightmapResourceCluster" => Source::Lightmap(u16::MAX),
        "TranslucentBasePass" => Source::Pass(u16::MAX),
        "InstanceVF" => Source::InstanceVf(u16::MAX),
        "PrecomputedLightingBuffer" => Source::PrecomputedLighting,
        "LandscapeParameters" => Source::LandscapeVf(u16::MAX),
        // The cooked FLandscapeVertexFactory vertex shaders' uniform buffer of the section LOD and LOD bias buffers.
        "LandscapeContinuousLODParameters" => Source::LandscapeLod(u16::MAX),
        // The shadow depth pass's buffer (FShadowDepthPassUniformParameters), the pass input of that pass.
        "ShadowDepthPass" => Source::Pass(u16::MAX),
        "" if register == 0 => Source::Globals,
        n => match n.strip_prefix("MaterialCollection").and_then(|k| k.parse().ok()) {
            Some(k) => Source::Collection(k),
            None => Source::None,
        },
    }
}

/// A stage's bindings from its code's 'u' item and resource table; `bones`: the register GPUSkin's BoneMatrices
/// reads (a loose SRV, outside the table).
fn stage_binds(code: &ShaderCode, bones: Option<u32>) -> Result<StageBinds> {
    let mut b = StageBinds::default();
    for (reg, name) in code.uniform_buffer_names.iter().enumerate() {
        let s = buffer_source(name, reg as u32);
        if s != Source::None || !name.is_empty() {
            b.cbs.push((reg as u32, s));
        }
    }
    if code.uniform_buffer_names.is_empty() && code.global_uniform_buffer_used {
        b.cbs.push((0, Source::Globals));
    }
    let resource = |slot: mvs_material::shader::TableSlot| -> Source {
        let name = code.uniform_buffer_names.get(slot.buffer as usize).map_or("", String::as_str);
        match buffer_source(name, slot.buffer as u32) {
            Source::View(_) => Source::View(slot.resource),
            Source::Material(_) => Source::Material(slot.resource),
            Source::LocalVf(_) => Source::LocalVf(slot.resource),
            Source::Lightmap(_) => Source::Lightmap(slot.resource),
            Source::Pass(_) => Source::Pass(slot.resource),
            Source::InstanceVf(_) => Source::InstanceVf(slot.resource),
            Source::LandscapeVf(_) => Source::LandscapeVf(slot.resource),
            Source::LandscapeLod(_) => Source::LandscapeLod(slot.resource),
            _ => Source::None,
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
    if let Some(r) = bones {
        b.srvs.push((r, Source::Bones));
    }
    Ok(b)
}

impl MaterialInstance {
    pub fn new(gpu: &Gpu, assets: &Assets, path: &str) -> Result<MaterialInstance> {
        let m = assets.material(path)?;
        let u = &m.uniforms;
        let mut resources = Vec::with_capacity(u.resources.len());
        for t in &u.textures {
            resources.push(MaterialResource::Texture(TextureSource::Asset(
                texture_of(assets, t)
                    .map_err(|e| Error::Format { what: format!("{path} {}", t.name), why: e.to_string() })?,
            )));
            let key = match (t.sampler_source, &t.texture) {
                (1, _) => world_sampler(D3D11_TEXTURE_ADDRESS_WRAP),
                (2, _) => world_sampler(D3D11_TEXTURE_ADDRESS_CLAMP),
                (_, TextureBind::Texture { sampler, .. }) => sampler_key(sampler),
                _ => SamplerKey::of(D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT, D3D11_TEXTURE_ADDRESS_WRAP),
            };
            resources.push(MaterialResource::Sampler(gpu.state(StateKey::Sampler(key))));
        }
        // The shared samplers after the pairs: Wrap_WorldGroupSettings, Clamp_WorldGroupSettings.
        let shared = [world_sampler(D3D11_TEXTURE_ADDRESS_WRAP), world_sampler(D3D11_TEXTURE_ADDRESS_CLAMP)];
        let mut k = 0;
        while resources.len() < u.resources.len() {
            let key = shared.get(k).copied().unwrap_or(shared[0]);
            resources.push(MaterialResource::Sampler(gpu.state(StateKey::Sampler(key))));
            k += 1;
        }
        let buffer = if u.size > 0 {
            Some(gpu.buffer(&BufDesc { kind: BufKind::Constant, size: u.size.div_ceil(16) * 16, dynamic: true }, None)?)
        } else {
            None
        };
        let mut collections = Vec::with_capacity(m.collections.len());
        for c in &m.collections {
            // (a collection the game set at run time: its rows, else the entry's)
            let mut rows: Vec<[u32; 4]> = assets.collection(&c.path).unwrap_or_else(|| {
                c.values
                    .chunks(4)
                    .map(|ch| std::array::from_fn(|i| (ch.get(i).copied().unwrap_or(0.0) as f32).to_bits()))
                    .collect()
            });
            if rows.is_empty() {
                rows.push([0; 4]);
            }
            let bytes = crate::gpu::rows_bytes(&rows);
            // Dynamic: a collection the game sets at run time (Materials::set_collection) is rewritten in place.
            collections.push(
                gpu.buffer(&BufDesc { kind: BufKind::Constant, size: bytes.len() as u32, dynamic: true }, Some(bytes))?,
            );
        }
        Ok(MaterialInstance {
            scalars: u.scalars.iter().map(|s| s.value).collect(),
            vectors: u.vectors.iter().map(|v| v.value).collect(),
            material: m,
            resources,
            buffer,
            collections,
            dirty: true,
            passes: HashMap::new(),
            slate_passes: HashMap::new(),
            pending_collections: Vec::new(),
        })
    }

    /// The material's own value of every parameter back (a dynamic instance cleared of its overrides), textures
    /// included.
    pub fn reset_parameters(&mut self, assets: &Assets) -> Result<()> {
        let u = &self.material.uniforms;
        for (k, s) in u.scalars.iter().enumerate() {
            if self.scalars[k] != s.value {
                self.scalars[k] = s.value;
                self.dirty = true;
            }
        }
        for (k, v) in u.vectors.iter().enumerate() {
            if self.vectors[k] != v.value {
                self.vectors[k] = v.value;
                self.dirty = true;
            }
        }
        for (k, t) in u.textures.iter().enumerate() {
            let own = texture_of(assets, t)?;
            if !matches!(&self.resources[2 * k], MaterialResource::Texture(TextureSource::Asset(a)) if Arc::ptr_eq(a, &own))
            {
                self.resources[2 * k] = MaterialResource::Texture(TextureSource::Asset(own));
            }
        }
        Ok(())
    }

    /// Every parameter set to the material's own value, or to the last of `scalars` / `vectors` / `textures` naming
    /// it (a dynamic instance cleared of its overrides and set again, as `reset_parameters` then `set_*`), marked to
    /// write only where a value changed.
    pub fn set_parameters(
        &mut self,
        assets: &Assets,
        scalars: &[(String, f32)],
        vectors: &[(String, [f32; 4])],
        textures: &[(&str, Arc<Texture>)],
    ) -> Result<()> {
        let u = &self.material.uniforms;
        // (an override sets the first parameter of its name only, as `set_scalar` / `set_vector` / `set_texture`)
        let mut want_scalars: Vec<f64> = u.scalars.iter().map(|s| s.value).collect();
        for (n, v) in scalars {
            if let Some(k) = u.scalars.iter().position(|s| s.name == *n) {
                want_scalars[k] = *v as f64;
            }
        }
        let mut want_vectors: Vec<[f64; 4]> = u.vectors.iter().map(|s| s.value).collect();
        for (n, v) in vectors {
            if let Some(k) = u.vectors.iter().position(|s| s.name == *n) {
                want_vectors[k] = v.map(|c| c as f64);
            }
        }
        if self.scalars != want_scalars {
            self.scalars = want_scalars;
            self.dirty = true;
        }
        if self.vectors != want_vectors {
            self.vectors = want_vectors;
            self.dirty = true;
        }
        let mut set: Vec<Option<&Arc<Texture>>> = vec![None; u.textures.len()];
        for (n, t) in textures {
            if let Some(k) = u.textures.iter().position(|s| s.name == *n) {
                set[k] = Some(t);
            }
        }
        for (k, t) in u.textures.iter().enumerate() {
            let own = match set[k] {
                Some(t) => t.clone(),
                None => texture_of(assets, t)?,
            };
            if !matches!(&self.resources[2 * k], MaterialResource::Texture(TextureSource::Asset(a)) if Arc::ptr_eq(a, &own))
            {
                self.resources[2 * k] = MaterialResource::Texture(TextureSource::Asset(own));
            }
        }
        Ok(())
    }

    /// The buffers of the collection at `path` rewritten with `rows` (a collection set at run time).
    pub fn set_collection(&mut self, gpu: &Gpu, path: &str, rows: &[[u32; 4]]) {
        let at: Vec<usize> =
            self.material.collections.iter().enumerate().filter(|c| c.1.path == path).map(|c| c.0).collect();
        for k in at {
            self.pending_collections.retain(|p| p.0 != k);
            write_collection(gpu, &self.collections[k], rows);
        }
    }

    /// The collection rows set before this instance joined its set, written into its buffers.
    fn write_pending(&mut self, gpu: &Gpu) {
        for (k, rows) in self.pending_collections.drain(..) {
            write_collection(gpu, &self.collections[k], &rows);
        }
    }

    /// Sets a scalar parameter by name; a name the material does not hold is ignored (as a dynamic instance's).
    pub fn set_scalar(&mut self, name: &str, v: f32) {
        if let Some(k) = self.material.uniforms.scalars.iter().position(|s| s.name == name)
            && self.scalars[k] != v as f64
        {
            self.scalars[k] = v as f64;
            self.dirty = true;
        }
    }

    pub fn set_vector(&mut self, name: &str, v: [f32; 4]) {
        let v = v.map(|c| c as f64);
        if let Some(k) = self.material.uniforms.vectors.iter().position(|s| s.name == name)
            && self.vectors[k] != v
        {
            self.vectors[k] = v;
            self.dirty = true;
        }
    }

    // Unverified: a texture set over the material's does not re-evaluate the TextureSize expressions (the
    // imported sizes stay).
    pub fn set_texture(&mut self, name: &str, t: TextureSource) {
        if let Some(k) = self.material.uniforms.textures.iter().position(|s| s.name == name) {
            self.resources[2 * k] = MaterialResource::Texture(t);
        }
    }

    /// The material buffer's rows (the preshaders over the instance's parameters) written when a parameter changed.
    fn refresh(&mut self, gpu: &Gpu) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(b) = &self.buffer {
            let m = &self.material;
            let rows = m.uniform_rows(&Inputs { m, scalars: &self.scalars, vectors: &self.vectors })?;
            let mut out = vec![[0u32; 4]; (b.desc.size / 16) as usize];
            for (o, r) in out.iter_mut().zip(&rows) {
                *o = r.map(|x| (x as f32).to_bits());
            }
            gpu.write_rows(b, &out);
        }
        self.dirty = false;
        Ok(())
    }

    /// The pass's shaders and state, created on first use; the Material uniform buffer rewritten when a parameter
    /// changed since the last bind.
    pub fn bind(
        &mut self,
        gpu: &Gpu,
        assets: &Assets,
        vf: VertexFactory,
        pass: PassKind,
        policy: LightMapPolicy,
    ) -> Result<&BoundPass> {
        self.refresh(gpu)?;
        self.write_pending(gpu);
        let key = (vf, pass, policy);
        if !self.passes.contains_key(&key) {
            let b = self.make_pass(gpu, assets, vf, pass, policy)?;
            self.passes.insert(key, b);
        }
        Ok(&self.passes[&key])
    }

    /// The Slate pass of a UI material (MD_UI; no vertex factory): TSlateMaterialShaderVS<instanced> and
    /// TSlateMaterialShaderPS<ps>, the state and resources; the Material uniform buffer rewritten when a parameter
    /// changed since the last bind.
    pub fn bind_slate(
        &mut self,
        gpu: &Gpu,
        assets: &Assets,
        ps: SlateMaterialShader,
        instanced: bool,
    ) -> Result<&BoundPass> {
        self.refresh(gpu)?;
        self.write_pending(gpu);
        let key = (ps, instanced);
        if !self.slate_passes.contains_key(&key) {
            let b = self.make_slate_pass(gpu, assets, ps, instanced)?;
            self.slate_passes.insert(key, b);
        }
        Ok(&self.slate_passes[&key])
    }

    fn make_slate_pass(
        &self,
        gpu: &Gpu,
        assets: &Assets,
        ps: SlateMaterialShader,
        instanced: bool,
    ) -> Result<BoundPass> {
        let m = &self.material;
        let what = || format!("{} Slate {ps:?} instanced {instanced}", m.path);
        if m.state.domain != "MD_UI" {
            return Err(Error::Format { what: what(), why: format!("{}: not a UI material", m.state.domain) });
        }
        // The cooked type names hash as the typedefs (TSlateMaterialShaderVSfalse), not as the templates.
        let vs_type = if instanced { "TSlateMaterialShaderVStrue" } else { "TSlateMaterialShaderVSfalse" };
        let ps_type = format!("TSlateMaterialShaderPS{ps:?}");
        let (_, rows) = m.shaders.pass(None, None, &[(vs_type, 0), (ps_type.as_str(), 0)]).ok_or_else(|| {
            Error::Format { what: what(), why: format!("no {vs_type} / {ps_type} in the shader map") }
        })?;
        let codes: Vec<(mvs_material::Sha1, Arc<ShaderCode>)> = rows
            .iter()
            .map(|r| assets.code(m.shaders.code(r)).map(|c| (m.shaders.code(r), c)))
            .collect::<Result<_>>()?;
        let vs = gpu.shader_by(codes[0].0, &codes[0].1)?;
        let ps_id = gpu.shader_by(codes[1].0, &codes[1].1)?;
        if codes[0].1.frequency != Frequency::Vertex || codes[1].1.frequency != Frequency::Pixel {
            return Err(Error::Format { what: what(), why: "the rows are not a vertex and a pixel shader".into() });
        }
        let vs_binds = stage_binds(&codes[0].1, None)?;
        let mut ps_binds = stage_binds(&codes[1].1, None)?;
        // The pixel shader's loose texture and sampler (outside the resource table): TSlateMaterialShaderPS's
        // element texture, the font atlas of a font pass.
        let params = m.shaders.params(rows[1]);
        let rt = &codes[1].1.resource_table;
        let mut table = Vec::new();
        for map in [&rt.texture_map, &rt.srv_map] {
            table.extend(rt.slots(map)?.iter().map(|s| s.register as u32));
        }
        let samplers: Vec<u32> = rt.slots(&rt.sampler_map)?.iter().map(|s| s.register as u32).collect();
        for p in &params.parameter_map.srvs {
            if !table.contains(&p.base_index) {
                ps_binds.srvs.push((p.base_index, Source::Globals));
            }
        }
        for p in &params.parameter_map.texture_samplers {
            if !samplers.contains(&p.base_index) {
                ps_binds.samplers.push((p.base_index, Source::Globals));
            }
        }
        let blend = slate_material_blend(&m.state.blend, false)
            .ok_or_else(|| Error::Format { what: what(), why: format!("{}: no Slate blend state", m.state.blend) })?;
        let raster = raster_key(true, false);
        Ok(BoundPass {
            vs,
            ps: Some(ps_id),
            blend: gpu.state(StateKey::Blend(blend)),
            raster: gpu.state(StateKey::Raster(raster)),
            depth: gpu.state(StateKey::Depth(DepthKey {
                depth_write: false,
                depth_func: D3D11_COMPARISON_ALWAYS.0 as u8,
                stencil: None,
            })),
            raster_mirrored: gpu.state(StateKey::Raster(raster)),
            vs_binds,
            ps_binds,
        })
    }

    /// The depth-only and custom depth passes' shaders of `vf` are in the material's shader map, or in the default
    /// material's for an opaque one (`make_pass` finds them; it panics on a depth pass without them).
    pub(crate) fn has_depth_pass(&self, assets: &Assets, vf: VertexFactory) -> bool {
        let m = &self.material;
        let masked = m.state.blend == "BLEND_Masked";
        if masked || m.state.blend != "BLEND_Opaque" {
            return pipeline_rows(
                &m.shaders,
                vf.type_name(),
                &[("TDepthOnlyVS<false>", 0), ("FDepthOnlyPS<false>", 0)],
            )
            .is_some();
        }
        let has = |set: &mvs_material::ShaderSet| {
            set.pass(Some(vf.type_name()), Some("DepthNoPixelPipeline"), &[("TDepthOnlyVS<false>", 0)]).is_some()
        };
        has(&m.shaders) || assets.material(DEFAULT_MATERIAL).is_ok_and(|d| has(&d.shaders))
    }

    /// The spot shadow depth pass's shaders of `vf`: a masked material's own, an opaque one's from the default
    /// material (`make_pass`); a translucent material casts none.
    pub(crate) fn has_shadow_depth_pass(&self, assets: &Assets, vf: VertexFactory) -> bool {
        let m = &self.material;
        let types = [(SHADOW_DEPTH_VS, 0), (SHADOW_DEPTH_PS, 0)];
        let has = |set: &mvs_material::ShaderSet| set.pass(Some(vf.type_name()), None, &types).is_some();
        match m.state.blend.as_str() {
            "BLEND_Masked" => has(&m.shaders),
            "BLEND_Opaque" => assets.material(DEFAULT_MATERIAL).is_ok_and(|d| has(&d.shaders)),
            _ => false,
        }
    }

    /// A pass bound before (by `bind`).
    pub fn bound(&self, vf: VertexFactory, pass: PassKind, policy: LightMapPolicy) -> Option<&BoundPass> {
        self.passes.get(&(vf, pass, policy))
    }

    fn make_pass(
        &self,
        gpu: &Gpu,
        assets: &Assets,
        vf: VertexFactory,
        pass: PassKind,
        policy: LightMapPolicy,
    ) -> Result<BoundPass> {
        let m = &self.material;
        let what = || format!("{} {} {pass:?} {policy:?}", m.path, vf.type_name());
        let st = &m.state;
        let masked = st.blend == "BLEND_Masked";
        let translucent = !masked && st.blend != "BLEND_Opaque";
        let (vs_type, ps_type, pipeline) = match pass {
            PassKind::Base => {
                if st.blend != "BLEND_Opaque" && !masked {
                    return Err(Error::Format { what: what(), why: format!("{} in the opaque base pass", st.blend) });
                }
                // The Skylight variant for a lit material, whatever its policy: a lit mesh in a level with no build
                // data (Map_Classic3Platform_Small / _Large) takes the NoLightMap policy and still gets the sky light
                // (indirect 0, the sky SH, the Key). An unlit material is the one
                // NoLightMap case without it. Unverified: 4.26's ShouldRenderSkylightInBasePass (a lit shading model
                // and a stationary sky light), not read from the exe.
                let sky = if policy == LightMapPolicy::NoLightMap && st.shading_model == "MSM_Unlit" {
                    ""
                } else {
                    "Skylight"
                };
                let p = policy.type_suffix();
                (format!("TBasePassVS{p}"), Some(format!("TBasePassPS{p}{sky}")), format!("BasePassPipeline{p}{sky}"))
            }
            // FDepthPassMeshProcessor: the position-only depth shaders, a pixel shader for a masked material (and a
            // translucent one that writes custom depth); an opaque material without its own takes the default
            // material's (below).
            // Unverified: the choice is 4.26's (no WPO / tessellation cases), not read from the exe.
            PassKind::DepthOnly | PassKind::CustomDepth => {
                let writes =
                    !translucent || (pass == PassKind::CustomDepth && st.allow_translucent_custom_depth_writes);
                if !writes {
                    panic!("unported: {} ({}) in the {pass:?} pass", m.path, st.blend);
                }
                if masked || translucent {
                    // The pixel shader's cooked type name is FDepthOnlyPS<false>; its pipeline's name is not known (hashed
                    // 0x45a01c1108d0a460), so its rows are found by the pipeline holding the pixel shader.
                    ("TDepthOnlyVS<false>".to_string(), Some("FDepthOnlyPS<false>".to_string()), String::new())
                } else {
                    ("TDepthOnlyVS<false>".to_string(), None, "DepthNoPixelPipeline".to_string())
                }
            }
            // FBasePassMeshProcessor for a translucent blend mode: the base pass shaders of the policy.
            // Unverified: the Skylight variant where the map has it, else the plain one (ShouldRenderSkylightInBasePass
            // for a translucent blend mode is not read from the exe).
            PassKind::Translucent => {
                if st.blend == "BLEND_Opaque" || masked {
                    return Err(Error::Format { what: what(), why: format!("{} in the translucent pass", st.blend) });
                }
                if st.shading_model == "MSM_ThinTranslucent" {
                    mvs_sim::unported!("SetTranslucentRenderState's ThinTranslucent states (144c3a070 / 088 / 0a0)");
                }
                let p = policy.type_suffix();
                let sky = if policy == LightMapPolicy::NoLightMap { "" } else { "Skylight" };
                let has = |t: &str| m.shaders.pass(Some(vf.type_name()), None, &[(t, 0)]).is_some();
                let sky = if has(&format!("TBasePassPS{p}{sky}")) { sky } else { "" };
                (format!("TBasePassVS{p}"), Some(format!("TBasePassPS{p}{sky}")), format!("BasePassPipeline{p}{sky}"))
            }
            // FShadowDepthPassMeshProcessor for a spot's whole-scene shadow: the PerspectiveCorrect pair, a masked
            // material's own (its pixel shader discards on the opacity mask), an opaque one's from the default
            // material (below).
            // Unverified: the processor was not found in the exe; the rule is the cooked maps' (every
            // masked map carries the rows, an opaque one none) read with 4.26's source.
            PassKind::ShadowDepth => {
                if translucent {
                    return Err(Error::Format { what: what(), why: format!("{} casts no shadow depth", st.blend) });
                }
                (SHADOW_DEPTH_VS.to_string(), Some(SHADOW_DEPTH_PS.to_string()), String::new())
            }
            PassKind::Distortion | PassKind::Decal => {
                return Err(Error::Format { what: what(), why: "the pass's shaders are not chosen yet".into() });
            }
        };
        let mut types = vec![(vs_type.as_str(), 0)];
        if let Some(p) = &ps_type {
            types.push((p.as_str(), 0));
        }
        fn find<'a>(
            set: &'a mvs_material::ShaderSet,
            vf: &str,
            pipeline: &str,
            types: &[(&str, i32)],
        ) -> Option<Vec<&'a mvs_material::material::ShaderRow>> {
            if pipeline.is_empty() {
                pipeline_rows(set, vf, types)
            } else {
                set.pass(Some(vf), Some(pipeline), types).map(|p| p.1)
            }
        }
        let depth_pass = matches!(pass, PassKind::DepthOnly | PassKind::CustomDepth | PassKind::ShadowDepth);
        // (an opaque material's shadow depth is the default material's even with rows of its own)
        // Unverified: the build's rule is Masked or world position offset casts with its own shaders (spot_shadows.md
        // N8); no menu material has WPO (2.4), so WPO is not tested here.
        let own = if pass == PassKind::ShadowDepth && !masked {
            None
        } else {
            find(&m.shaders, vf.type_name(), &pipeline, &types)
        };
        let default;
        let (set, rows) = match own {
            Some(rows) => (&m.shaders, rows),
            // FDepthPassMeshProcessor::AddMeshBatch: an opaque material that writes every pixel (no mask, no world
            // position offset) draws its depth with the default material's shaders, which the cook leaves it without.
            None if depth_pass && !masked && !translucent => {
                default = assets.material(DEFAULT_MATERIAL)?;
                let rows = find(&default.shaders, vf.type_name(), &pipeline, &types).unwrap_or_else(|| {
                    panic!("unported: no {types:?} for {} in {DEFAULT_MATERIAL} ({})", vf.type_name(), m.path)
                });
                (&default.shaders, rows)
            }
            // A masked material whose map holds no depth vertex shader for the factory at all
            // (Velma_PoliceCarWindow_MAT: its maps hold the NoLightMap base pass only) draws its depth with the
            // default material's opaque shaders, unmasked.
            // Unverified: taken as 4.26's TryAddMeshBatch walking the proxy's GetFallback to the default material
            // when TryGetShaders fails; not read from the exe.
            None if matches!(pass, PassKind::DepthOnly | PassKind::CustomDepth)
                && masked
                && !m.shaders.rows.iter().any(|r| {
                    r.map == mvs_material::hashed_name(vf.type_name())
                        && r.shader_type == mvs_material::hashed_name("TDepthOnlyVS<false>")
                }) =>
            {
                eprintln!(
                    "warning: {}: no depth shaders for {}: its depth drawn with {DEFAULT_MATERIAL}'s",
                    m.path,
                    vf.type_name()
                );
                default = assets.material(DEFAULT_MATERIAL)?;
                let types = [("TDepthOnlyVS<false>", 0)];
                let rows =
                    find(&default.shaders, vf.type_name(), "DepthNoPixelPipeline", &types).unwrap_or_else(|| {
                        panic!("unported: no {types:?} for {} in {DEFAULT_MATERIAL} ({})", vf.type_name(), m.path)
                    });
                (&default.shaders, rows)
            }
            None if depth_pass => {
                panic!("unported: {} ({}) has no {types:?} for {} in its shader map", m.path, st.blend, vf.type_name())
            }
            None => return Err(Error::Format { what: what(), why: format!("no {types:?} in the shader map") }),
        };
        let codes: Vec<(mvs_material::Sha1, Arc<ShaderCode>)> =
            rows.iter().map(|r| assets.code(set.code(r)).map(|c| (set.code(r), c))).collect::<Result<_>>()?;
        let vs = gpu.shader_by(codes[0].0, &codes[0].1)?;
        let ps = codes.get(1).map(|(sha, c)| gpu.shader_by(*sha, c)).transpose()?;
        // GPUSkin's BoneMatrices: the vertex shader's loose SRV (the parameter map's, outside the resource table).
        let bones = if matches!(vf, VertexFactory::GpuSkin { .. }) {
            let table: Vec<u32> = {
                let rt = &codes[0].1.resource_table;
                let mut t = Vec::new();
                for map in [&rt.texture_map, &rt.srv_map] {
                    t.extend(rt.slots(map)?.iter().map(|s| s.register as u32));
                }
                t
            };
            let params = set.params(rows[0]);
            let r = params.parameter_map.srvs.iter().map(|p| p.base_index).find(|r| !table.contains(r));
            Some(r.ok_or_else(|| Error::Format {
                what: what(),
                why: "the GPU skin vertex shader binds no BoneMatrices SRV".into(),
            })?)
        } else {
            None
        };
        if codes[0].1.frequency != Frequency::Vertex {
            return Err(Error::Format { what: what(), why: "the first shader is not a vertex shader".into() });
        }
        let vs_binds = stage_binds(&codes[0].1, bones)?;
        let ps_binds = codes.get(1).map(|c| stage_binds(&c.1, None)).transpose()?.unwrap_or_default();
        let reads_material = |b: &StageBinds| {
            b.cbs.iter().any(|c| matches!(c.1, Source::Material(_)))
                || b.srvs.iter().chain(&b.samplers).any(|c| matches!(c.1, Source::Material(_)))
        };
        if !std::ptr::eq(set, &m.shaders) && (reads_material(&vs_binds) || reads_material(&ps_binds)) {
            panic!("unported: {DEFAULT_MATERIAL}'s depth shaders read its Material buffer ({})", m.path);
        }
        let depth = match pass {
            PassKind::Base => base_depth(),
            // The translucent pass tests the scene depth and writes none (TStaticDepthStencilState<false, CF_DepthNearOrEqual>);
            // a material with bDisableDepthTest draws over the scene (the blast border's frame).
            // Unverified: the translucent mesh pass's state for bDisableDepthTest is not read from the exe.
            PassKind::Translucent => {
                let func = if st.disable_depth_test { D3D11_COMPARISON_ALWAYS } else { D3D11_COMPARISON_GREATER_EQUAL };
                DepthKey { depth_write: false, depth_func: func.0 as u8, stencil: None }
            }
            PassKind::CustomDepth => custom_depth_state(),
            // The shadow depth target holds the light's linear depth, near small, cleared to 1 (lighting/shadow.rs).
            // Unverified: CF_LessEqual and the far clear are what the projection's rule (lit where the stored depth
            // is at or past the receiver's) needs; the pass's state is not read from the exe.
            PassKind::ShadowDepth => {
                DepthKey { depth_write: true, depth_func: D3D11_COMPARISON_LESS_EQUAL.0 as u8, stencil: None }
            }
            _ => DepthKey { depth_write: true, depth_func: D3D11_COMPARISON_GREATER_EQUAL.0 as u8, stencil: None },
        };
        let blend = match pass {
            PassKind::Translucent => translucent_blend(&st.blend).ok_or_else(|| Error::Format {
                what: what(),
                why: format!("{}: no translucent blend state", st.blend),
            })?,
            _ => BlendKey::OPAQUE,
        };
        Ok(BoundPass {
            vs,
            ps,
            blend: gpu.state(StateKey::Blend(blend)),
            raster: gpu.state(StateKey::Raster(raster_key(st.two_sided, false))),
            depth: gpu.state(StateKey::Depth(depth)),
            raster_mirrored: gpu.state(StateKey::Raster(raster_key(st.two_sided, true))),
            vs_binds,
            ps_binds,
        })
    }
}

/// A material instance of the renderer's set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaterialId(pub u32);

/// The renderer's material instances.
#[derive(Debug, Default)]
pub struct Materials {
    pub instances: Vec<MaterialInstance>,
    /// The collections set at run time (`set_collection`): (object path, rows).
    collections: Vec<(String, Vec<[u32; 4]>)>,
}

impl Materials {
    /// `m` joins the set; a collection set before takes its rows at its next bind.
    pub fn add(&mut self, mut m: MaterialInstance) -> MaterialId {
        for (k, c) in m.material.collections.iter().enumerate() {
            if let Some((_, rows)) = self.collections.iter().find(|s| s.0 == c.path) {
                m.pending_collections.push((k, rows.clone()));
            }
        }
        let id = MaterialId(u32::try_from(self.instances.len()).expect("material instances past u32"));
        self.instances.push(m);
        id
    }

    pub fn get_mut(&mut self, id: MaterialId) -> &mut MaterialInstance {
        &mut self.instances[id.0 as usize]
    }

    /// A material parameter collection's vectors set at run time (MPC_CentralPalette from the UI preferences): every
    /// instance's buffer of the collection at `path` rewritten with `rows`, and the instances made after it take it.
    pub fn set_collection(&mut self, gpu: &Gpu, path: &str, rows: &[[f32; 4]]) {
        let bits: Vec<[u32; 4]> = rows.iter().map(|r| r.map(f32::to_bits)).collect();
        for m in &mut self.instances {
            m.set_collection(gpu, path, &bits);
        }
        match self.collections.iter_mut().find(|c| c.0 == path) {
            Some(c) => c.1 = bits,
            None => self.collections.push((path.to_string(), bits)),
        }
    }
}

/// A View resource's index, for a binding by name.
pub fn view_index(name: &str) -> u16 {
    view_resource(name).unwrap_or_else(|| panic!("View: no resource {name}")) as u16
}

/// `types`' rows of one pipeline in `vertex_factory`'s map: the pipeline holding the last type's row.
fn pipeline_rows<'a>(
    set: &'a mvs_material::ShaderSet,
    vertex_factory: &str,
    types: &[(&str, i32)],
) -> Option<Vec<&'a mvs_material::material::ShaderRow>> {
    let map = mvs_material::hashed_name(vertex_factory);
    let (last, perm) = *types.last()?;
    let last = mvs_material::hashed_name(last);
    let p = set.rows.iter().find(|r| r.map == map && r.shader_type == last && r.permutation == perm)?.pipeline;
    types
        .iter()
        .map(|&(t, perm)| {
            let t = mvs_material::hashed_name(t);
            set.rows.iter().find(|r| r.map == map && r.pipeline == p && r.shader_type == t && r.permutation == perm)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policies() {
        let skin = LightMapSurface {
            lit: true,
            translucent: false,
            light_cache: false,
            cast_volumetric_translucent_shadow: false,
            texture_light_map: false,
            hq_interaction: true,
            texture_shadow_map: false,
            mobility: Mobility::Movable,
            preview_or_forced_volumetric: false,
            indirect_cache: None,
        };
        let scene = LightMapScene::STAGE;
        assert_eq!(light_map_policy(&skin, &scene), LightMapPolicy::PrecomputedIrradianceVolumeIndirectLighting);
        let stat = LightMapSurface { light_cache: true, texture_light_map: true, mobility: Mobility::Static, ..skin };
        assert_eq!(light_map_policy(&stat, &scene), LightMapPolicy::HqLightmap);
        let shadow = LightMapSurface { texture_shadow_map: true, ..stat };
        assert_eq!(light_map_policy(&shadow, &scene), LightMapPolicy::DistanceFieldShadowsAndHqLightmap);
        let lq = LightMapScene { hq_lightmaps: false, ..scene };
        assert_eq!(light_map_policy(&stat, &lq), LightMapPolicy::LqLightmap);
        let unlit = LightMapSurface { lit: false, ..stat };
        assert_eq!(light_map_policy(&unlit, &scene), LightMapPolicy::NoLightMap);
        let still = LightMapSurface { texture_light_map: false, ..stat };
        assert_eq!(light_map_policy(&still, &scene), LightMapPolicy::NoLightMap);
        let tr = LightMapSurface { translucent: true, cast_volumetric_translucent_shadow: true, ..skin };
        assert_eq!(light_map_policy(&tr, &scene), LightMapPolicy::SelfShadowedVolumetricLightmap);
        let cache = LightMapScene { volumetric_lightmap: false, precomputed_light_volumes: true, ..scene };
        let pt = LightMapSurface { indirect_cache: Some(IndirectCache::Point), ..skin };
        assert_eq!(light_map_policy(&pt, &cache), LightMapPolicy::CachedPointIndirectLighting);
    }

    #[test]
    fn contexts() {
        assert_eq!(context_of("skin").0, VertexFactory::GpuSkin { extra_influences: false, morph: false });
        assert_eq!(context_of("static").0, VertexFactory::Local);
        assert!(context_of("sprite").1.contains(&PassKind::Distortion));
        assert_eq!(context_of("customdepth"), (VertexFactory::Local, &[PassKind::CustomDepth][..]));
    }
}
