//! The translucent passes and separate translucency's composite, with the build's shaders.
//!
//! RenderTranslucency (141d90760) with r.SeparateTranslucency on draws three passes through 141d90ec0: Standard into
//! SceneColor, AfterDOF into the separate translucency colour target (cleared (0, 0, 0, 1)) and AfterDOFModulate into
//! the modulate target (PF_FloatR11G11B10, cleared (1, 1, 1, 1)). AddMeshBatch (1419234e0) puts a material in
//! AfterDOF when it asks for separate translucency, in AfterDOFModulate instead when it is also BLEND_Modulate, else
//! in Standard. The meshes draw with their material's base pass shaders and the blend of their blend mode
//! (SetTranslucentRenderState), sorted far first. ComposeSeparateTranslucency (141ba9fc0) then lays the two targets
//! over SceneColor: rgb = scene.rgb x sep.a x mod.rgb + sep.rgb, alpha 0. Before its first pass it takes the scene
//! colour copy (SceneColorCopy 141d79f00) a material's SceneColor node reads.

use mvs_d3d::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use mvs_d3d::Direct3D11::*;
use mvs_d3d::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R11G11B10_FLOAT, DXGI_FORMAT_R32_UINT, DXGI_FORMAT_R32G32B32A32_FLOAT,
};
use mvs_view::M4;

use crate::Result;
use crate::assets::Assets;
use crate::draw::{DrawItem, PassInput, execute};
use crate::gpu::{
    BlendKey, BlendTarget, BufDesc, BufKind, Buffer, DepthKey, Gpu, RasterKey, SamplerKey, StateId, StateKey, TexDesc,
    TexKind, TexViews, Texture, dx,
};
use crate::lighting::compile;
use crate::material::PassKind;
use crate::renderer::Frame;
use crate::scene::{Light, LightKind, LightSet};
use crate::scenery::widget::WidgetQuads;
use crate::targets::SceneTargets;

/// The composite (Global/002a6a5e's sum, by the blend unit): a full-screen triangle; target 0 the separate colour,
/// target 1 the factor SceneColor is scaled by (dual-source blend: dst x src1 + src0).
// Deviation: the targets are read texel by texel (Load at the pixel) where the build samples them bilinearly at the
// pixel's centre clamped to SeparateTranslucencyBilinearUVMinMax: the same texels at full size (r.SeparateTranslucency
// ScreenPercentage 100), and the port makes no reduced target.
pub(crate) const COMPOSE: &str = r"
Texture2D<float4> Separate : register(t0);
Texture2D<float4> Modulate : register(t1);
void MainVS(uint id : SV_VertexID, out float4 pos : SV_Position) {
    float2 uv = float2((id << 1) & 2, id & 2);
    pos = float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
}
void MainPS(float4 pos : SV_Position, out float4 color : SV_Target0, out float4 scale : SV_Target1) {
    int3 p = int3(pos.xy, 0);
    float4 sep = Separate.Load(p);
    color = float4(sep.rgb, 0);
    scale = float4(sep.a * Modulate.Load(p).rgb, 0);
}
float4 CopyPS(float4 pos : SV_Position) : SV_Target0 {
    return float4(pow(saturate(Separate.Load(int3(pos.xy, 0)).rgb * 0.1), 0.25), 1);
}
";

/// The translucent passes' shaders, states, the modulate target and the frame's items by pass.
#[derive(Debug)]
pub struct Translucency {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    /// The scene colour copy's pixel shader (the composite's vertex shader draws it).
    copy_ps: ID3D11PixelShader,
    compose_blend: StateId,
    opaque_blend: StateId,
    raster: StateId,
    depth: StateId,
    /// SeparateTranslucencyModulateColor, made at the scene targets' size.
    modulate: Option<Texture>,
    /// The frame's translucent items in draw order, by pass: Standard, AfterDOF, AfterDOFModulate.
    passes: [Vec<DrawItem>; 3],
    /// The items of a TranslucencySortPriority above 0, by pass: drawn after the particles (`draw_late`).
    late: [Vec<DrawItem>; 3],
    /// SceneColorCopy's target (PF_B8G8R8A8 at the scene colour's size), made on first use and again when the size
    /// changes.
    scene_copy: Option<Texture>,
    /// TranslucentBasePass's constants, written each draw.
    pass_buffer: Buffer,
    /// TranslucentBasePass's resources by index: the views (set each draw) and the samplers.
    pass_srvs: Vec<Option<ID3D11ShaderResourceView>>,
    pass_samplers: Vec<Option<ID3D11SamplerState>>,
    /// Custom depth's stencil for CustomStencilTexture at the next fill (None: the slot left empty).
    custom_stencil: Option<ID3D11ShaderResourceView>,
    /// A read-only view of the scene depth and the texture it views: the passes test against the depth while their
    /// shaders read it (SceneDepthTexture).
    read_only_depth: Option<(ID3D11Resource, ID3D11DepthStencilView)>,
    /// The translucency lighting volume's filtered textures (`set_volume`): ambient inner, ambient outer,
    /// directional inner, directional outer.
    volume: [Option<ID3D11ShaderResourceView>; 4],
    /// The forward light data's buffers and the local light rows they were made from.
    forward: Option<(Vec<[u32; 4]>, ForwardBuffers)>,
    /// The world-space widgets' quads (scenery/widget.rs) of the AfterDOF pass: the scenery's first, a menu level's
    /// among its items.
    pub(crate) widgets: WidgetQuads,
}

/// Forward's light grid of one cell: ForwardLocalLightBuffer, NumCulledLightsGrid, CulledLightDataGrid.
#[derive(Debug)]
struct ForwardBuffers {
    local_lights: Buffer,
    num_culled: Buffer,
    culled_data: Buffer,
}

/// FTranslucentBasePassUniformParameters' size (0x950) in rows.
const PASS_ROWS: usize = 0x950 / 16;
/// The entries of its resource table: FSharedBasePassUniformParameters' 35, SceneTextures' 13, its own 21.
const PASS_RESOURCES: usize = 69;
/// The resource indices the port fills: SkyLightCubemap, SkyLightBlendDestinationCubemap, the reflection struct's
/// PreIntegratedGF, SceneDepthTexture, the pass's PreIntegratedGFTexture.
const SKY_CUBEMAP: usize = 16;
const BLEND_DEST_CUBEMAP: usize = 18;
const REFLECTION_PRE_INTEGRATED_GF: usize = 22;
const SCENE_DEPTH: usize = 36;
/// SceneTextures' CustomStencilTexture.
const CUSTOM_STENCIL: usize = 46;
const PRE_INTEGRATED_GF: usize = 64;
/// SceneColorCopyTexture.
const SCENE_COLOR_COPY: usize = 67;
/// Forward's ForwardLocalLightBuffer, NumCulledLightsGrid and CulledLightDataGrid, and ForwardISR's (+ 8).
const FORWARD_LOCAL_LIGHTS: usize = 5;
const FORWARD_NUM_CULLED: usize = 6;
const FORWARD_CULLED_DATA: usize = 7;
const FORWARD_ISR: usize = 8;
/// TranslucencyLightingVolumeAmbientInner, AmbientOuter, DirectionalInner, DirectionalOuter.
const VOLUME: [usize; 4] = [56, 58, 60, 62];

/// The samplers of the resource table, by index (a texture's index + 1).
// Unverified: 4.26's static sampler states for each (SetupSharedBasePassParameters,
// SetupSceneTextureUniformParameters, CreateTranslucentBasePassUniformBuffer); the exe's are not read.
fn pass_sampler(k: usize) -> Option<SamplerKey> {
    use D3D11_TEXTURE_ADDRESS_CLAMP as C;
    use D3D11_TEXTURE_ADDRESS_WRAP as W;
    let point = SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_POINT, C);
    let bilinear = SamplerKey::of(D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT, C);
    let trilinear = |a| SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_LINEAR, a);
    Some(match k {
        // ShadowmapSampler, StaticShadowmapSampler (Forward, ForwardISR).
        1 | 9 => point,
        3 | 11 => bilinear,
        // SkyLightCubemapSampler, SkyLightBlendDestinationCubemapSampler, ReflectionCubemapSampler.
        17 | 19 | 21 => trilinear(W),
        // FogInscatteringColorSampler (Fog, FogISR).
        27 | 31 => trilinear(C),
        // PointClampSampler, HZBSampler.
        47 | 49 => point,
        // PreIntegratedGFSampler (the reflection struct's and the pass's), PlanarReflectionSampler,
        // IntegratedLightScatteringSampler (Fog, FogISR), PrevSceneColorSampler, the volumetric cloud's, the
        // lighting volume's, SceneColorCopySampler.
        23 | 25 | 29 | 33 | 51 | 53 | 55 | 57 | 59 | 61 | 63 | 65 | 68 => bilinear,
        _ => return None,
    })
}

/// TranslucentBasePass's constants from the stage's lights (the exe's member table, 144f80730): Shared's
/// ForwardLightData (rows 0..39, the instanced-stereo copy 40..79), ReflectionStruct (80..85), the planar
/// reflection (86..106, zero: none), FogStruct (107..116, the copy 117..126), then the pass's own members.
///
/// The forward light data as GatherLightsAndComputeLightGrid (141a71960) fills it: the local lights (`local_lights`)
/// and the directional light last in the sorted light set (each directional met overwrites the block; the sort puts
/// the lights that use lighting channels, a mask other than channel 0 alone, after those that do not). The shaders
/// gate both on the light's lighting channels and the primitive's.
// Unverified: the members' meaning is 4.26's (ForwardLightingCommon, ReflectionEnvironmentShared, HeightFogCommon);
// two directional lights of the same sort key keep the level's order.
// Deviation: the light grid is one cell holding every local light (CulledGridSize 1, the pixel shift and Z
// parameters putting every pixel in it): the build's cells hold the lights whose bounds reach them, and a light adds
// nothing past its radius or its cone, so the sum is the same while no cell of the build's passes its light limit.
// No directional shadow cascades or static shadow map (NumDirectionalLightCascades 0, UseStaticShadowing 0: the
// forward directional light is unshadowed), no reflection captures. FogStruct: `fog`'s rows (the height fog pass's:
// ExponentialFogParameters to ExponentialFogParameters3, the inscattering cubemap and volumetric fog off), or no fog
// (density 0, colour 0) without one; either way InscatteringLightDirection.w -1: no directional inscattering.
fn pass_rows(lights: &LightSet, fog: Option<&[[u32; 4]]>) -> Vec<[u32; 4]> {
    let f = |v: [f32; 4]| v.map(f32::to_bits);
    let mut rows = vec![[0u32; 4]; PASS_ROWS];
    let mut forward = [[0u32; 4]; 40];
    // NumLocalLights, NumReflectionCaptures, HasDirectionalLight, NumGridCells; CulledGridSize;
    // LightGridPixelSizeShift; LightGridZParams (log2(z x 0 + 1) x 0: slice 0).
    forward[0] = [lights.radial.len() as u32, 0, 0, 1];
    forward[1] = [1, 1, 1, 0];
    forward[2] = [31, 0, 0, 0];
    forward[3] = f([0.0, 1.0, 0.0, 0.0]);
    let uses_channels = |l: &&Light| l.channels != 1;
    let last = lights
        .directional
        .iter()
        .rfind(uses_channels)
        .or_else(|| lights.directional.iter().rfind(|l| !uses_channels(l)));
    if let Some(l) = last {
        forward[0][2] = 1;
        // DirectionalLightDirection: towards the light.
        let d = l.direction;
        forward[4] = f([-d[0], -d[1], -d[2], 0.0]);
        // DirectionalLightColor, .w DirectionalLightVolumetricScatteringIntensity.
        // Unverified: the light's VolumetricScatteringIntensity is not carried; 1 is the property's default.
        forward[5] = f([l.color[0], l.color[1], l.color[2], 1.0]);
        // DirectionalLightShadowMapChannelMask: the shadow map channels (none) and the lighting channels from bit 8.
        forward[6][0] = (l.channels as u32) << 8;
    }
    rows[..40].copy_from_slice(&forward);
    rows[40..80].copy_from_slice(&forward);
    // ReflectionStruct: SkyLightParameters (max mip, a sky light, not dynamic: a stationary sky light (FUN_141a8de80),
    // no blend), SkyLightCubemapBrightness.
    if let Some(s) = &lights.sky {
        rows[80] = f([(s.cube.desc.mips - 1) as f32, 1.0, 0.0, 0.0]);
        rows[81] = f([s.brightness, 0.0, 0.0, 0.0]);
    }
    // FogStruct and its copy.
    for at in [107, 117] {
        if let Some(fog) = fog {
            rows[at..at + 4].copy_from_slice(&fog[..4]);
        }
        rows[at + 4][3] = (-1f32).to_bits();
    }
    // PrevSceneColorPreExposureInv (no pre-exposure); HZBUvFactorAndInvFactor, PrevScreenPositionScaleBias and
    // ApplyVolumetricCloudOnTransparent stay 0 (no HZB, no screen space reflections, no volumetric cloud).
    rows[137][0] = 1f32.to_bits();
    rows
}

/// ForwardLocalLightBuffer's rows: six float4 per local light, in the light set's order.
/// - [0] position, InvRadius; [1] colour (x the fade factor: 1, no light has a MaxDrawDistance), FalloffExponent
///   (0: inverse squared).
/// - [2] the direction towards the light (a spot light's axis reversed), the packed word: bits 0..3 the static shadow
///   map channel, 4..7 the dynamic one, 8..15 the lighting channels, 16..17 the type (1 point, 2 spot).
/// - [3] SpotAngles, SourceRadius, f16 SourceLength (low) and f16 VolumetricScatteringIntensity (high).
/// - [4] Tangent, SoftSourceRadius; [5] the rect light's barn door (none).
// Unverified: the shadow map channel bits are 0 (the shader's shadow factor 1): which of the static and dynamic
// channels the build sets for a stationary light was not read, and with a static channel but no dynamic one the
// shader drops the light. The sorted order of the lights was not read. VolumetricScatteringIntensity 1 (the
// property's default, not carried).
fn local_lights(lights: &LightSet) -> Vec<[u32; 4]> {
    let f = |v: [f32; 4]| v.map(f32::to_bits);
    let mut rows = Vec::with_capacity(lights.radial.len() * 6);
    for l in &lights.radial {
        let p = l.position.map(|x| x as f32);
        let d = l.direction.map(|x| -x);
        let ty = if l.kind == LightKind::Spot { 2 } else { 1 };
        let word = ((l.channels as u32) << 8) | (ty << 16);
        rows.push(f([p[0], p[1], p[2], l.inv_radius]));
        rows.push(f([l.color[0], l.color[1], l.color[2], l.falloff_exponent]));
        rows.push([d[0].to_bits(), d[1].to_bits(), d[2].to_bits(), word]);
        let packed = half(l.source_length) as u32 | ((half(1.0) as u32) << 16);
        rows.push([l.spot_angles[0].to_bits(), l.spot_angles[1].to_bits(), l.source_radius.to_bits(), packed]);
        rows.push(f([l.tangent[0], l.tangent[1], l.tangent[2], l.soft_source_radius]));
        rows.push([0; 4]);
    }
    rows
}

/// `x` as a half float's bits (round to nearest, ties to even; overflow to infinity, underflow to subnormals).
// Unverified: the rounding of the exe's inlined conversion (FFloat16::Set in 4.26 source truncates).
fn half(x: f32) -> u16 {
    let b = x.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let e = ((b >> 23) & 0xff) as i32;
    let m = b & 0x7f_ffff;
    if e == 0xff {
        return sign | 0x7c00 | if m != 0 { 0x200 } else { 0 };
    }
    let e = e - 127 + 15;
    if e >= 31 {
        return sign | 0x7c00;
    }
    let (m, shift) = if e <= 0 { (m | 0x80_0000, (14 - e) as u32) } else { (m, 13u32) };
    if shift > 24 {
        return sign;
    }
    let mut h = m >> shift;
    let rest = m & ((1 << shift) - 1);
    let halfway = 1 << (shift - 1);
    if rest > halfway || (rest == halfway && h & 1 == 1) {
        h += 1;
    }
    // (a carry out of the mantissa moves into the exponent, up to infinity)
    if e > 0 { sign | ((((e as u32) << 10) + h).min(0x7c00) as u16) } else { sign | h as u16 }
}

/// The view Z of `origin` (world, cm): TranslatedWorldToView's third column at the translated position.
pub(crate) fn view_z(to_view: &M4, camera: [f64; 3], origin: [f64; 3]) -> f32 {
    let p = [origin[0] - camera[0], origin[1] - camera[1], origin[2] - camera[2]];
    (p[0] * to_view[0][2] + p[1] * to_view[1][2] + p[2] * to_view[2][2] + to_view[3][2]) as f32
}

/// AddMeshBatch's pass of a material: 0 Standard, 1 AfterDOF, 2 AfterDOFModulate.
// Unverified: the material's dual blending (vt+0x260, which also sends a material to AfterDOFModulate) is not read;
// no material of the store is taken to use it.
fn pass_of(separate: bool, blend: &str) -> usize {
    match (separate, blend == "BLEND_Modulate") {
        (false, _) => 0,
        (true, false) => 1,
        (true, true) => 2,
    }
}

/// The order under the build's r.TranslucentSortPolicy 1 (SortByProjectedZ; the keys of UpdateTranslucentMeshSortKeys
/// 141b2abf0, sorted ascending by 141ab0950): the priority first, then the view Z of the primitive's bounds origin,
/// far first, then the mesh's id in its primitive; ties keep the list's order. The effects' component meshes go after
/// the other items of their priority (cmesh.rs: render order 999).
// Unverified: the mesh's id in its primitive (MeshIdInPrimitive) is taken as its section index.
fn order(items: &mut [DrawItem], view_z: impl Fn(&DrawItem) -> f32) {
    items.sort_by(|a, b| {
        let mesh = |it: &DrawItem| it.mesh.as_ref().map_or(0, |m| m.section);
        a.sort_key
            .total_cmp(&b.sort_key)
            .then(a.after_meshes.cmp(&b.after_meshes))
            .then(view_z(b).total_cmp(&view_z(a)))
            .then(mesh(a).cmp(&mesh(b)))
    });
}

impl Translucency {
    pub fn new(gpu: &Gpu, assets: &Assets) -> Result<Translucency> {
        let _ = assets;
        let vs_code = compile(COMPOSE, "MainVS", "vs_5_0")?;
        let ps_code = compile(COMPOSE, "MainPS", "ps_5_0")?;
        let copy_code = compile(COMPOSE, "CopyPS", "ps_5_0")?;
        let (mut vs, mut ps, mut copy_ps) = (None, None, None);
        // SAFETY: DXBC just compiled for this device's shader model.
        unsafe {
            gpu.dev.CreateVertexShader(&vs_code, None, Some(&mut vs)).map_err(dx("CreateVertexShader compose"))?;
            gpu.dev.CreatePixelShader(&ps_code, None, Some(&mut ps)).map_err(dx("CreatePixelShader compose"))?;
            gpu.dev.CreatePixelShader(&copy_code, None, Some(&mut copy_ps)).map_err(dx("CreatePixelShader copy"))?;
        }
        // SceneColor = SceneColor x src1 + src0, alpha 0.
        let compose = BlendTarget {
            color_op: D3D11_BLEND_OP_ADD.0 as u8,
            color_src: D3D11_BLEND_ONE.0 as u8,
            color_dst: D3D11_BLEND_SRC1_COLOR.0 as u8,
            alpha_op: D3D11_BLEND_OP_ADD.0 as u8,
            alpha_src: D3D11_BLEND_ZERO.0 as u8,
            alpha_dst: D3D11_BLEND_ZERO.0 as u8,
            write_mask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        let mut targets = [BlendTarget::OPAQUE; 8];
        targets[0] = compose;
        Ok(Translucency {
            vs: vs.expect("CreateVertexShader"),
            ps: ps.expect("CreatePixelShader"),
            copy_ps: copy_ps.expect("CreatePixelShader"),
            compose_blend: gpu.state(StateKey::Blend(BlendKey { targets, alpha_to_coverage: false })),
            opaque_blend: gpu
                .state(StateKey::Blend(BlendKey { targets: [BlendTarget::OPAQUE; 8], alpha_to_coverage: false })),
            raster: gpu.state(StateKey::Raster(RasterKey {
                fill: D3D11_FILL_SOLID.0 as u8,
                cull: D3D11_CULL_NONE.0 as u8,
                front_ccw: false,
                depth_bias: 0,
                slope_scaled_depth_bias: 0,
            })),
            depth: gpu.state(StateKey::Depth(DepthKey {
                depth_write: false,
                depth_func: D3D11_COMPARISON_ALWAYS.0 as u8,
                stencil: None,
            })),
            modulate: None,
            passes: Default::default(),
            late: Default::default(),
            scene_copy: None,
            pass_buffer: gpu
                .buffer(&BufDesc { kind: BufKind::Constant, size: (PASS_ROWS * 16) as u32, dynamic: true }, None)?,
            pass_srvs: vec![None; PASS_RESOURCES],
            custom_stencil: None,
            pass_samplers: (0..PASS_RESOURCES)
                .map(|k| pass_sampler(k).map(|key| gpu.sampler(gpu.state(StateKey::Sampler(key)))))
                .collect(),
            read_only_depth: None,
            volume: Default::default(),
            forward: None,
            widgets: WidgetQuads::new(gpu)?,
        })
    }

    /// The modulate target at the scene targets' size, made on first use and again when the size changes.
    fn modulate_target(&mut self, gpu: &Gpu, size: [u32; 2]) -> Result<&Texture> {
        if self.modulate.as_ref().is_none_or(|t| t.desc.size[..2] != size) {
            let desc = TexDesc {
                kind: TexKind::D2,
                size: [size[0], size[1], 1],
                mips: 1,
                format: DXGI_FORMAT_R11G11B10_FLOAT,
                views: TexViews { srv: true, rtv: true, ..Default::default() },
            };
            self.modulate = Some(gpu.texture(&desc, &[])?);
        }
        Ok(self.modulate.as_ref().expect("made above"))
    }

    /// TranslucentBasePass as the passes last filled it, for the particles' translucent draws.
    pub(crate) fn pass_input(&self) -> PassInput<'_> {
        PassInput { buffer: &self.pass_buffer.buf, srvs: &self.pass_srvs, samplers: &self.pass_samplers }
    }

    /// A read-only depth-stencil view of `depth`, made again when the texture changes.
    fn read_only_depth(&mut self, gpu: &Gpu, depth: &Texture) -> Result<ID3D11DepthStencilView> {
        if self.read_only_depth.as_ref().is_none_or(|(r, _)| *r != depth.res) {
            let dsv = depth.dsv.as_ref().expect("a depth view");
            let mut desc = D3D11_DEPTH_STENCIL_VIEW_DESC::default();
            let mut view = None;
            // SAFETY: the view and the resource are this device's; the description is the writable view's with the
            // read-only flags.
            unsafe {
                dsv.GetDesc(&mut desc);
                desc.Flags = (D3D11_DSV_READ_ONLY_DEPTH.0 | D3D11_DSV_READ_ONLY_STENCIL.0) as u32;
                gpu.dev
                    .CreateDepthStencilView(&depth.res, Some(&desc), Some(&mut view))
                    .map_err(dx("CreateDepthStencilView read-only"))?;
            }
            self.read_only_depth = Some((depth.res.clone(), view.expect("CreateDepthStencilView")));
        }
        Ok(self.read_only_depth.as_ref().expect("made above").1.clone())
    }

    /// TranslucentBasePass from the stage's lights, `fog` (FogStruct's rows of the stage's height fog, None without
    /// one) and the scene depth: done by `draw`, and by the frame when only the particles bind it.
    pub(crate) fn fill_pass(&mut self, fr: &Frame, fog: Option<&[[u32; 4]]>) -> Result<()> {
        fr.gpu.write_rows(&self.pass_buffer, &pass_rows(fr.lights, fog));
        self.pass_srvs.fill(None);
        if let Some(sky) = &fr.lights.sky {
            self.pass_srvs[SKY_CUBEMAP] = sky.cube.srv.clone();
            self.pass_srvs[BLEND_DEST_CUBEMAP] = sky.cube.srv.clone();
            self.pass_srvs[REFLECTION_PRE_INTEGRATED_GF] = sky.pre_integrated_gf.srv.clone();
            self.pass_srvs[PRE_INTEGRATED_GF] = sky.pre_integrated_gf.srv.clone();
        }
        self.pass_srvs[SCENE_DEPTH] = fr.targets.depth.srv.clone();
        self.pass_srvs[CUSTOM_STENCIL] = self.custom_stencil.clone();
        for (k, v) in VOLUME.iter().zip(&self.volume) {
            self.pass_srvs[*k] = v.clone();
        }
        let local = local_lights(fr.lights);
        if self.forward.as_ref().is_none_or(|(rows, _)| *rows != local) {
            let n = local.len() / 6;
            let typed = |format, bytes: &[u8]| {
                let desc = BufDesc { kind: BufKind::Typed { format }, size: bytes.len() as u32, dynamic: false };
                fr.gpu.buffer(&desc, Some(bytes))
            };
            let words = |v: Vec<u32>| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
            // (one zero row where there is no light: a view needs an element)
            let rows = if local.is_empty() { vec![[0u32; 4]] } else { local.clone() };
            let buffers = ForwardBuffers {
                local_lights: typed(DXGI_FORMAT_R32G32B32A32_FLOAT, crate::gpu::rows_bytes(&rows))?,
                // the cell's count and start, then the reflection captures' (none)
                num_culled: typed(DXGI_FORMAT_R32_UINT, &words(vec![n as u32, 0, 0, 0]))?,
                culled_data: typed(DXGI_FORMAT_R32_UINT, &words((0..n.max(1) as u32).collect()))?,
            };
            self.forward = Some((local, buffers));
        }
        let (_, b) = self.forward.as_ref().expect("made above");
        for at in [0, FORWARD_ISR] {
            self.pass_srvs[at + FORWARD_LOCAL_LIGHTS] = b.local_lights.srv.clone();
            self.pass_srvs[at + FORWARD_NUM_CULLED] = b.num_culled.srv.clone();
            self.pass_srvs[at + FORWARD_CULLED_DATA] = b.culled_data.srv.clone();
        }
        Ok(())
    }

    /// Custom depth's stencil (a uint view) as the passes' CustomStencilTexture from the next fill on, or none: the
    /// banners' pieces compare it with their StencilValue.
    // Deviation: bound only in a frame with meshes drawn into custom depth alone (the menus' banner masks), so a
    // match's translucency reads the slot empty as before; the build binds custom depth's stencil in every frame whose
    // custom depth pass ran (SetupSceneTextureUniformParameters, else StencilDummy).
    pub(crate) fn set_custom_stencil(&mut self, s: Option<ID3D11ShaderResourceView>) {
        self.custom_stencil = s;
    }

    /// The translucency lighting volume's filtered textures for the passes' next fill (ambient inner, ambient outer,
    /// directional inner, directional outer).
    pub(crate) fn set_volume(&mut self, volume: [Option<ID3D11ShaderResourceView>; 4]) {
        self.volume = volume;
    }

    /// SceneColor as a material's SceneColor node reads it (FTranslucentBasePassUniformParameters' SceneColorCopyTexture,
    /// 67): EncodeSceneColorForMaterialNode's (colour x 0.1)^(1/4), the range 0..10 in an 8-bit target; the cooked
    /// reader (DecodeSceneColorForMaterialNode) samples it at the pixel's buffer UV and takes rgb^4 x 10.
    // Unverified: the encode is 4.26's copy pass , the exe's shader not found. The build
    // takes the copy once per translucency render call (141d90760), the port only in the frames a reader draws.
    fn take_scene_copy(&mut self, gpu: &Gpu, t: &SceneTargets) -> Result<()> {
        if self.scene_copy.as_ref().is_none_or(|c| c.desc.size[..2] != t.size) {
            let desc = TexDesc {
                kind: TexKind::D2,
                size: [t.size[0], t.size[1], 1],
                mips: 1,
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                views: TexViews { srv: true, rtv: true, ..Default::default() },
            };
            self.scene_copy = Some(gpu.texture(&desc, &[])?);
        }
        let copy = self.scene_copy.as_ref().expect("made above");
        gpu.viewport([0, 0, t.size[0], t.size[1]]);
        gpu.set_geometry_shader(None);
        gpu.set_states(self.opaque_blend, self.raster, self.depth, 0);
        // SAFETY: shaders and views of this device; SceneColor is read while the copy is written; both are unbound
        // before the return.
        unsafe {
            let ctx = &gpu.ctx;
            ctx.OMSetRenderTargets(Some(std::slice::from_ref(&copy.rtv)), None);
            ctx.IASetInputLayout(None);
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.VSSetShader(&self.vs, None);
            ctx.PSSetShader(&self.copy_ps, None);
            ctx.PSSetShaderResources(0, Some(std::slice::from_ref(&t.scene_color.srv)));
            ctx.Draw(3, 0);
            ctx.PSSetShaderResources(0, Some(&[None]));
            ctx.OMSetRenderTargets(None, None);
        }
        Ok(())
    }

    /// `fr.list.translucent`, sorted, through the Standard, AfterDOF and AfterDOFModulate passes, with
    /// TranslucentBasePass filled (fill_pass, and the scene colour copy taken first) and the depth bound read-only.
    /// The items of a TranslucencySortPriority above 0 wait for `draw_late`.
    pub fn draw(&mut self, fr: &mut Frame, fog: Option<&[[u32; 4]]>) -> Result<()> {
        let cam = &fr.input.camera;
        let rows = &fr.scene.primitive_rows.0;
        let depth_of = |it: &DrawItem| {
            let o = rows.get(it.primitive as usize).map_or([0u32; 4], |r| r[5]);
            let o = [0, 1, 2].map(|k| f32::from_bits(o[k]) as f64);
            view_z(&cam.translated_world_to_view, cam.origin, o)
        };
        for p in self.passes.iter_mut().chain(&mut self.late) {
            p.clear();
        }
        for it in &fr.list.translucent {
            let st = &fr.materials.get_mut(it.material).material.state;
            let k = pass_of(st.separate_translucency, &st.blend);
            if it.sort_key > 0.0 { &mut self.late[k] } else { &mut self.passes[k] }.push(it.clone());
        }
        for p in self.passes.iter_mut().chain(&mut self.late) {
            order(p, depth_of);
        }
        self.widgets.place_among(&self.passes[1], |it| (it.sort_key, it.after_meshes, depth_of(it)));

        self.take_scene_copy(fr.gpu, fr.targets)?;
        self.fill_pass(fr, fog)?;
        self.pass_srvs[SCENE_COLOR_COPY] = self.scene_copy.as_ref().and_then(|c| c.srv.clone());
        self.run(fr, false)
    }

    /// The translucent items of a TranslucencySortPriority above 0, after the particles (the build sorts by the
    /// priority first), into the targets `draw` filled, with TranslucentBasePass as `draw` left it.
    // Unverified: the particles are taken to be below these items' priority (the blast
    // border's frame, priority 101, draws after every effect).
    pub fn draw_late(&mut self, fr: &mut Frame) -> Result<()> {
        if self.late.iter().all(Vec::is_empty) {
            return Ok(());
        }
        self.run(fr, true)
    }

    /// The Standard, AfterDOF and AfterDOFModulate passes of `passes`, or of `late` (the targets not cleared).
    fn run(&mut self, fr: &mut Frame, late: bool) -> Result<()> {
        let (gpu, t) = (fr.gpu, fr.targets);
        let modulate = self.modulate_target(gpu, t.size)?.rtv.clone();
        let dsv = self.read_only_depth(gpu, &t.depth)?;
        gpu.viewport([0, 0, t.size[0], t.size[1]]);
        gpu.set_geometry_shader(None);
        let sep = t.separate_translucency.rtv.clone();
        for (k, target) in [t.scene_color.rtv.clone(), sep, modulate].into_iter().enumerate() {
            let rtv = target.expect("a render target view");
            // SAFETY: views of this device's targets; the clear colours are four floats.
            unsafe {
                match k {
                    _ if late => {}
                    1 => gpu.ctx.ClearRenderTargetView(&rtv, &[0.0, 0.0, 0.0, 1.0]),
                    2 => gpu.ctx.ClearRenderTargetView(&rtv, &[1.0; 4]),
                    _ => {}
                }
                gpu.ctx.OMSetRenderTargets(Some(&[Some(rtv)]), &dsv);
            }
            let items = if late { &self.late[k] } else { &self.passes[k] };
            let input = self.pass_input();
            // the world-space widgets' quads among the AfterDOF items, where `place_among` put them
            let mut from = 0;
            if k == 1 && !late {
                for at in self.widgets.places() {
                    let run = &items[from..at.min(items.len())];
                    execute(gpu, fr.assets, fr.scene, t, fr.materials, run, PassKind::Translucent, Some(input))?;
                    self.widgets.draw(gpu, at);
                    from = at.min(items.len());
                }
            }
            execute(gpu, fr.assets, fr.scene, t, fr.materials, &items[from..], PassKind::Translucent, Some(input))?;
        }
        // SAFETY: unbinds the targets and the views the passes read (the depth among them).
        unsafe {
            gpu.ctx.OMSetRenderTargets(None, None);
            gpu.ctx.VSSetShaderResources(0, Some(&[const { None }; 32]));
            gpu.ctx.PSSetShaderResources(0, Some(&[const { None }; 32]));
        }
        Ok(())
    }

    /// The separate translucency and modulate targets composited over SceneColor.
    // Deviation: the build composites inside post processing (after the distortion, before the outline and the
    // bloom); here it runs where the frame calls it.
    pub fn composite(&mut self, fr: &mut Frame) -> Result<()> {
        let (gpu, t) = (fr.gpu, fr.targets);
        let modulate = self.modulate_target(gpu, t.size)?.srv.clone();
        gpu.viewport([0, 0, t.size[0], t.size[1]]);
        gpu.set_geometry_shader(None);
        gpu.set_states(self.compose_blend, self.raster, self.depth, 0);
        // SAFETY: shaders and views of this device; the two targets are read while SceneColor is written; all are
        // unbound before the return.
        unsafe {
            let ctx = &gpu.ctx;
            ctx.OMSetRenderTargets(Some(std::slice::from_ref(&t.scene_color.rtv)), None);
            ctx.IASetInputLayout(None);
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.VSSetShader(&self.vs, None);
            ctx.PSSetShader(&self.ps, None);
            ctx.PSSetShaderResources(0, Some(&[t.separate_translucency.srv.clone(), modulate]));
            ctx.Draw(3, 0);
            ctx.PSSetShaderResources(0, Some(&[None, None]));
            ctx.OMSetRenderTargets(None, None);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_composite_compiles() {
        assert!(!compile(COMPOSE, "MainVS", "vs_5_0").expect("vs").is_empty());
        assert!(!compile(COMPOSE, "MainPS", "ps_5_0").expect("ps").is_empty());
        assert!(!compile(COMPOSE, "CopyPS", "ps_5_0").expect("copy ps").is_empty());
    }

    #[test]
    fn materials_go_to_their_pass() {
        assert_eq!(pass_of(false, "BLEND_Modulate"), 0);
        assert_eq!(pass_of(false, "BLEND_Translucent"), 0);
        assert_eq!(pass_of(true, "BLEND_Additive"), 1);
        assert_eq!(pass_of(true, "BLEND_Modulate"), 2);
    }

    #[test]
    fn a_stage_with_no_lights_fills_no_light_and_no_fog() {
        let rows = pass_rows(&LightSet::default(), None);
        assert_eq!(rows.len(), 149);
        assert_eq!(rows[0], [0, 0, 0, 1], "no local light, HasDirectionalLight 0, one grid cell");
        assert_eq!(rows[80], [0; 4], "no sky light");
        assert_eq!(f32::from_bits(rows[111][3]), -1.0, "InscatteringLightDirection.w");
        assert_eq!(f32::from_bits(rows[121][3]), -1.0, "the instanced-stereo copy's");
        assert_eq!(f32::from_bits(rows[137][0]), 1.0, "PrevSceneColorPreExposureInv");
    }

    fn light(kind: LightKind, channels: u8) -> Light {
        Light {
            name: "l".into(),
            kind,
            position: [1.0, 2.0, 3.0],
            direction: [0.0, 0.0, -1.0],
            tangent: [1.0, 0.0, 0.0],
            color: [2.0; 3],
            inv_radius: 0.01,
            falloff_exponent: 0.0,
            spot_angles: [0.5, 4.0],
            source_radius: 3.0,
            soft_source_radius: 0.0,
            source_length: 2765.29,
            specular_scale: 1.0,
            channels,
            shadow_channel: -1,
            cast_shadows: false,
            translucent: false,
        }
    }

    #[test]
    fn the_last_directional_in_sorted_order_is_the_forward_light() {
        let dirs =
            vec![light(LightKind::Directional, 1), light(LightKind::Directional, 2), light(LightKind::Directional, 1)];
        let mut set = LightSet { directional: dirs, ..Default::default() };
        let rows = pass_rows(&set, None);
        assert_eq!(rows[0][2], 1);
        assert_eq!(rows[6][0], 2 << 8, "the channel-1 light, sorted after the channel-0 ones");
        set.directional.truncate(1);
        assert_eq!(pass_rows(&set, None)[6][0], 1 << 8);
    }

    #[test]
    fn a_local_light_packs_its_type_channels_and_source_length() {
        let set = LightSet { radial: vec![light(LightKind::Spot, 2)], ..Default::default() };
        let r = local_lights(&set);
        assert_eq!(r.len(), 6);
        assert_eq!(r[2][3], (2 << 8) | (2 << 16));
        assert_eq!(f32::from_bits(r[2][2]), 1.0, "towards the light along the axis");
        assert_eq!(r[3][3] & 0xffff, 0x6967, "2765.29 to the nearest half float, 2766");
        assert_eq!(r[3][3] >> 16, 0x3c00);
        assert_eq!(half(1.0), 0x3c00);
        assert_eq!(half(65536.0), 0x7c00);
        assert_eq!(half(0.0), 0);
        assert_eq!(half(-2.0), 0xc000);
    }

    #[test]
    fn view_z_is_the_third_column() {
        // UE's view: X right, Y up, Z forward; a camera at (100, 0, 0) looking down world +X.
        let mut m = [[0.0; 4]; 4];
        (m[0][2], m[1][0], m[2][1], m[3][3]) = (1.0, 1.0, 1.0, 1.0);
        assert_eq!(view_z(&m, [100.0, 0.0, 0.0], [350.0, 20.0, -5.0]), 250.0);
    }
}
