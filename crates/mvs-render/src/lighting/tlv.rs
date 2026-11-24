//! The translucency lighting volume (TranslucentLighting.cpp in 4.26): two cascades of 3D textures around the camera,
//! cleared each frame, every light that affects translucency injected into them with the build's own inject shaders
//! (WorldGridMaterial's TTranslucentLightingInjectPS through FWriteToSliceVS / GS), then filtered
//! (FFilterTranslucentVolumePS). The translucent base pass reads the filtered pair (TranslucentBasePass 56..63).

use mvs_d3d::Direct3D::D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP;
use mvs_d3d::Direct3D11::*;
use mvs_d3d::Dxgi::Common::*;
use mvs_data::stage::TlvShadow;
use mvs_material::jsonbin::{BodyRef, JsonBin};
use mvs_material::{Frequency, ShaderCode};
use mvs_view::Camera;
use serde::Deserialize;

use crate::assets::Assets;
use crate::gpu::{
    BlendKey, BlendTarget, BufDesc, BufKind, Buffer, DepthKey, Gpu, InputElement, RasterKey, SamplerKey, ShaderId,
    StateId, StateKey, Subresource, TexDesc, TexKind, TexViews, Texture,
};
use crate::renderer::Frame;
use crate::scene::{Light, LightKind};
use crate::view::ViewMembers;
use crate::{Error, Result};

/// r.TranslucencyLightingVolumeDim.
// Dim 64 and r.TranslucencyVolumeBlur 1 are the build's (BaseScalability [EffectsQuality@3], not overridden).
// Unverified: InnerDistance 1500, OuterDistance 5000, MinFOV 45 and FOVSnapFactor 10 are 4.26's compiled defaults;
// the exe's cvar registration was not read.
const DIM: u32 = 64;
const INNER_DISTANCE: f32 = 1500.0;
const OUTER_DISTANCE: f32 = 5000.0;
const MIN_FOV_DEGREES: f32 = 45.0;
const FOV_SNAP_DEGREES: f32 = 10.0;

/// The inject pixel shaders' types (TTranslucentLightingInjectPS<type, bDynamicallyShadowed, bApplyLightFunction,
/// bInverseSquared>; a spot light takes the point type, its cone from SpotAngles): no dynamic shadow (no light has
/// a shadow map of its own) and no light function.
const INJECT_DIRECTIONAL: &str = "TTranslucentLightingInjectPSLightType_Directionalfalsefalsefalse";
const INJECT_LOCAL: &str = "TTranslucentLightingInjectPSLightType_Pointfalsefalsefalse";
const INJECT_LOCAL_INVERSE_SQUARED: &str = "TTranslucentLightingInjectPSLightType_Pointfalsefalsetrue";
/// The material whose shader map holds the inject shaders (the light-function material of a light without one).
const INJECT_MATERIAL: &str = "/Engine/EngineMaterials/WorldGridMaterial.WorldGridMaterial";

/// The inject shaders' $Globals (byte offsets, read from 000b5744 / 0011641b and their parameter maps).
mod globals {
    /// ShadowInjectParams.
    pub const SHADOW_INJECT_PARAMS: u32 = 528;
    /// bStaticallyShadowed.
    pub const STATICALLY_SHADOWED: u32 = 560;
    /// WorldToStaticShadowMatrix.
    pub const WORLD_TO_STATIC_SHADOW: u32 = 576;
    /// StaticShadowBufferSize: (x, y, 1 / x, 1 / y).
    pub const STATIC_SHADOW_BUFFER_SIZE: u32 = 640;
    /// VolumeCascadeIndex.
    pub const VOLUME_CASCADE_INDEX: u32 = 656;
    /// The volumetric cloud shadow's WorldToLightClipShadowMatrix.
    pub const CLOUD_SHADOW_MATRIX: u32 = 704;
    /// The cloud shadow's FarDepthKm, Enabled, Strength, then AtmospherePerPixelTransmittanceEnabled.
    pub const CLOUD_SHADOW_FAR_DEPTH: u32 = 768;
    pub const CLOUD_SHADOW_ENABLED: u32 = 772;
    pub const CLOUD_SHADOW_STRENGTH: u32 = 776;
    pub const ATMOSPHERE_TRANSMITTANCE: u32 = 780;
    /// SpotlightMask.
    pub const SPOTLIGHT_MASK: u32 = 816;
    /// ClippingPlanes[2].
    pub const CLIPPING_PLANES: u32 = 864;
    /// Every member the two shaders read.
    pub const ALL: &[u32] = &[
        SHADOW_INJECT_PARAMS,
        STATICALLY_SHADOWED,
        WORLD_TO_STATIC_SHADOW,
        STATIC_SHADOW_BUFFER_SIZE,
        VOLUME_CASCADE_INDEX,
        CLOUD_SHADOW_MATRIX,
        CLOUD_SHADOW_FAR_DEPTH,
        CLOUD_SHADOW_ENABLED,
        CLOUD_SHADOW_STRENGTH,
        ATMOSPHERE_TRANSMITTANCE,
        SPOTLIGHT_MASK,
        CLIPPING_PLANES,
    ];
    /// Rows of the buffer.
    pub const ROWS: usize = 56;
}

/// DeferredLightUniforms' rows (FLightShaderParameters from row 3, 4.26 order; the shaders read 3, 4, 5 and 7).
const LIGHT_ROWS: usize = 9;

/// The stage's lights' static shadow depth maps (`static_shadows`), or a menu level's (`level_shadows`).
#[derive(Debug, Default)]
pub struct StaticShadows(Vec<StaticShadow>);

/// The static shadow maps a `Tlv::draw` applies: the stage's (`Tlv::set_shadows`), or the shown menu level's (none:
/// a level whose lights have none).
#[derive(Clone, Copy, Debug)]
pub enum TlvShadows<'a> {
    Stage,
    Menu(Option<&'a StaticShadows>),
}

/// A light's static shadow depth map (FStaticShadowDepthMap): its texture of FFloat16 depths and WorldToLight.
#[derive(Debug)]
struct StaticShadow {
    light: String,
    texture: Texture,
    world_to_light: [f32; 16],
    size: [u32; 2],
}

/// An inject pixel shader and the register its static shadow depth texture binds at.
#[derive(Debug)]
struct Inject {
    ps: ShaderId,
    shadow_register: u32,
}

/// The volume's two cascades' bounds: the box's minimum corner and its size (a cube).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cascade {
    pub min: [f32; 3],
    pub size: f32,
}

#[derive(Debug)]
pub struct Tlv {
    vs: ShaderId,
    gs: ShaderId,
    directional: Inject,
    local: Inject,
    local_inverse_squared: Inject,
    filter: ShaderId,
    /// The rasterize quad (FScreenVertex: positions, then UVs, in two buffers).
    positions: Buffer,
    uvs: Buffer,
    vs_globals: Buffer,
    ps_globals: Buffer,
    light: Buffer,
    filter_globals: Buffer,
    /// [cascade][ambient, directional] as injected, and filtered.
    raw: [[Texture; 2]; 2],
    filtered: [[Texture; 2]; 2],
    add: StateId,
    opaque: StateId,
    raster: StateId,
    depth: StateId,
    point_clamp: StateId,
    bilinear_clamp: StateId,
    /// GWhiteTexture's stand-in, bound where a light has no static shadow.
    white: Texture,
    shadows: StaticShadows,
    cascades: [Cascade; 2],
}

const ATTRIBUTE: &std::ffi::CStr = c"ATTRIBUTE";
/// The input layout's key: ATTRIBUTE0 float2 in slot 0, ATTRIBUTE1 float2 in slot 1.
const LAYOUT_KEY: u32 = 0x746c_7671;

fn bad(what: &str, why: impl Into<String>) -> Error {
    Error::Format { what: what.to_string(), why: why.into() }
}

/// The global shader `ty` (permutation 0) at `freq`, created.
fn global(gpu: &Gpu, assets: &Assets, ty: &str, freq: Frequency) -> Result<(ShaderId, std::sync::Arc<ShaderCode>)> {
    let g = assets.global_shaders();
    let row = g.find(ty, 0, freq).ok_or_else(|| bad("global shader cache", format!("no {ty} at {freq:?}")))?;
    let sha = g.shaders.code(row);
    let code = assets.code(sha)?;
    Ok((gpu.shader_by(sha, &code)?, code))
}

/// A volume texture of the cascades: PF_FloatRGBA, DIM cubed.
fn volume(gpu: &Gpu) -> Result<Texture> {
    gpu.texture(
        &TexDesc {
            kind: TexKind::D3,
            size: [DIM; 3],
            mips: 1,
            format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            views: TexViews { srv: true, rtv: true, ..Default::default() },
        },
        &[],
    )
}

/// An R16F texture of `w` x `h` FFloat16 texels.
fn depth_texture(gpu: &Gpu, data: &[u8], w: u32, h: u32) -> Result<Texture> {
    gpu.texture(
        &TexDesc {
            kind: TexKind::D2,
            size: [w, h, 1],
            mips: 1,
            format: DXGI_FORMAT_R16_FLOAT,
            views: TexViews { srv: true, ..Default::default() },
        },
        &[Subresource { data, row_pitch: w * 2, slice_pitch: w * h * 2 }],
    )
}

fn vertex_buffer(gpu: &Gpu, v: &[[f32; 2]; 4]) -> Result<Buffer> {
    let b: Vec<u8> = v.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
    gpu.buffer(&BufDesc { kind: BufKind::Vertex, size: b.len() as u32, dynamic: false }, Some(&b))
}

fn constants(gpu: &Gpu, rows: usize) -> Result<Buffer> {
    gpu.buffer(&BufDesc { kind: BufKind::Constant, size: rows as u32 * 16, dynamic: true }, None)
}

/// Floats from byte offset `byte` of `rows`.
fn put(rows: &mut [[u32; 4]], byte: u32, v: &[f32]) {
    for (k, x) in v.iter().enumerate() {
        let w = byte as usize / 4 + k;
        rows[w / 4][w % 4] = x.to_bits();
    }
}

const IDENTITY: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// The lighting kind's entry header: the lights' static shadow depth maps by light guid.
#[derive(Deserialize)]
struct LightingHeader {
    lights: std::collections::BTreeMap<String, BuiltLight>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct BuiltLight {
    depth_map: DepthMap,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DepthMap {
    shadow_map_size_x: u32,
    shadow_map_size_y: u32,
    depth_samples: BodyRef,
    /// `M00` .. `M33`, row-major.
    #[serde(default)]
    world_to_light: Option<std::collections::HashMap<String, f64>>,
}

/// CalcTranslucencyLightingVolumeBounds (4.26): per cascade the bounding sphere of the view frustum's slice from
/// the cascade's start to its end distance, its centre snapped to a multiple of the voxel size x 2, the box the
/// sphere's centre +- its radius.
// Unverified: from 4.26 source; the exe's function was not read.
pub fn cascades(cam: &Camera) -> [Cascade; 2] {
    let p = &cam.view_to_clip;
    let snap = FOV_SNAP_DEGREES.to_radians();
    let mut fov = (1.0 / p[0][0] as f32).atan().max(MIN_FOV_DEGREES.to_radians());
    fov = fov + snap - fov % snap;
    let aspect = (p[1][1] / p[0][0]) as f32;
    let m = &cam.translated_world_to_view;
    let col = |c: usize| [m[0][c] as f32, m[1][c] as f32, m[2][c] as f32];
    let (right, up, forward) = (col(0), col(1), col(2));
    let origin = cam.origin.map(|x| x as f32);
    let slice = |start: f32, end: f32| -> Cascade {
        let mut v = Vec::with_capacity(8);
        for d in [start, end] {
            let h = d * fov.tan();
            let vl = h / aspect;
            for (sr, su) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
                v.push([0, 1, 2].map(|k| origin[k] + forward[k] * d + right[k] * h * sr + up[k] * vl * su));
            }
        }
        let mut c = [0.0f32; 3];
        for x in &v {
            for k in 0..3 {
                c[k] += x[k];
            }
        }
        c = c.map(|x| x / 8.0);
        let r2 = v.iter().map(|x| (0..3).map(|k| (c[k] - x[k]) * (c[k] - x[k])).sum::<f32>()).fold(0.0, f32::max);
        let w = r2.sqrt();
        let step = w * 2.0 / DIM as f32;
        let c = c.map(|x| x - x % step);
        Cascade { min: c.map(|x| x - w), size: 2.0 * w }
    };
    [slice(0.0, INNER_DISTANCE), slice(INNER_DISTANCE, OUTER_DISTANCE)]
}

/// The stage's static shadow depth maps: each light `shadow` names (a Stationary light's, from the build data),
/// its map from its registry's lighting entry (the light's DepthMap) and its WorldToLight (none for a point light,
/// which reads its map by direction).
pub fn static_shadows(gpu: &Gpu, store: &mvs_store::Store, shadow: Option<&TlvShadow>) -> Result<StaticShadows> {
    let mut out = Vec::new();
    let Some(t) = shadow else { return Ok(StaticShadows(out)) };
    let pack = store.kind("lighting")?;
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for (name, l) in t.lights.iter() {
        if !entries.iter().any(|e| e.0 == l.built_data) {
            entries.push((l.built_data.clone(), pack.bytes(&l.built_data)?));
        }
        let bytes = &entries.iter().find(|e| e.0 == l.built_data).expect("read above").1;
        let j = JsonBin::split(b"MVLT", bytes, &l.built_data)?;
        let h: LightingHeader = j.parse(&l.built_data)?;
        let d = &h
            .lights
            .get(&l.guid)
            .ok_or_else(|| bad(name, format!("no light {} in {}", l.guid, l.built_data)))?
            .depth_map;
        let (w, hh) = (d.shadow_map_size_x, d.shadow_map_size_y);
        let data = j.section(d.depth_samples, &l.built_data)?;
        if w == 0 || hh == 0 || data.len() != (w * hh * 2) as usize {
            return Err(bad(name, format!("a {w} x {hh} depth map of {} bytes", data.len())));
        }
        let world_to_light = match &l.world_to_light {
            Some(m) if m.len() == 16 => std::array::from_fn(|k| m[k] as f32),
            Some(m) => return Err(bad(name, format!("a WorldToLight of {} values", m.len()))),
            None => IDENTITY,
        };
        out.push(StaticShadow {
            light: name.to_string(),
            texture: depth_texture(gpu, data, w, hh)?,
            world_to_light,
            size: [w, hh],
        });
    }
    Ok(StaticShadows(out))
}

/// A menu level's static shadow depth maps: each named light's, by its LightGuid, in the lighting kind's entry
/// `built_data` (the level's MapBuildDataRegistry), with the entry's WorldToLight (directional lights only). A light
/// the build gave no map (0 x 0) has none.
// Deviation: every map the build holds is applied; the stage's import keeps only a map with a texel that can shadow
// (built_lighting.rs tlv_static_shadows), which gives the same light.
pub fn level_shadows<'a>(
    gpu: &Gpu,
    store: &mvs_store::Store,
    built_data: &str,
    lights: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<StaticShadows> {
    let bytes = store.kind("lighting")?.bytes(built_data)?;
    let j = JsonBin::split(b"MVLT", &bytes, built_data)?;
    let h: LightingHeader = j.parse(built_data)?;
    let mut out = Vec::new();
    for (name, guid) in lights {
        let d = &h.lights.get(guid).ok_or_else(|| bad(name, format!("no light {guid} in {built_data}")))?.depth_map;
        let (w, hh) = (d.shadow_map_size_x, d.shadow_map_size_y);
        if w == 0 || hh == 0 {
            continue;
        }
        let data = j.section(d.depth_samples, built_data)?;
        if data.len() != (w * hh * 2) as usize {
            return Err(bad(name, format!("a {w} x {hh} depth map of {} bytes", data.len())));
        }
        let m = d.world_to_light.as_ref().ok_or_else(|| bad(name, "no WorldToLight"))?;
        let mut world_to_light = IDENTITY;
        for (k, v) in world_to_light.iter_mut().enumerate() {
            let x = m.get(&format!("M{}{}", k / 4, k % 4)).copied().unwrap_or(f64::NAN);
            if !x.is_finite() {
                return Err(bad(name, format!("WorldToLight M{}{}: {x}", k / 4, k % 4)));
            }
            *v = x as f32;
        }
        out.push(StaticShadow {
            light: name.to_string(),
            texture: depth_texture(gpu, data, w, hh)?,
            world_to_light,
            size: [w, hh],
        });
    }
    Ok(StaticShadows(out))
}

impl Tlv {
    pub fn new(gpu: &Gpu, assets: &Assets) -> Result<Tlv> {
        let (vs, _) = global(gpu, assets, "FWriteToSliceVS", Frequency::Vertex)?;
        let (gs, _) = global(gpu, assets, "FWriteToSliceGS", Frequency::Geometry)?;
        let (filter, _) = global(gpu, assets, "FFilterTranslucentVolumePS", Frequency::Pixel)?;
        let m = assets.material(INJECT_MATERIAL)?;
        let inject = |ty: &str, shadow_register: u32| -> Result<Inject> {
            let (_, rows) =
                m.shaders.pass(None, None, &[(ty, 0)]).ok_or_else(|| bad(INJECT_MATERIAL, format!("no {ty}")))?;
            let row = rows[0];
            for b in &m.shaders.params(row).parameter_map.loose_parameter_buffers {
                for p in &b.parameters {
                    if !globals::ALL.contains(&p.base_index) {
                        return Err(bad(ty, format!("reads $Globals at {}, not filled", p.base_index)));
                    }
                }
            }
            let sha = m.shaders.code(row);
            let code = assets.code(sha)?;
            let names: Vec<&str> = code.uniform_buffer_names.iter().map(String::as_str).collect();
            if names[..3] != ["", "View", "DeferredLightUniforms"] {
                return Err(bad(ty, format!("uniform buffers {names:?}")));
            }
            Ok(Inject { ps: gpu.shader_by(sha, &code)?, shadow_register })
        };
        // (000b5744: t0 the atmosphere transmittance, t1 the static shadow, t2 the cloud shadow; 0011641b: t0
        // the static shadow)
        let directional = inject(INJECT_DIRECTIONAL, 1)?;
        let local = inject(INJECT_LOCAL, 0)?;
        let local_inverse_squared = inject(INJECT_LOCAL_INVERSE_SQUARED, 0)?;
        // GVolumeRasterizeVertexBuffer: a strip of (position, UV).
        let positions = vertex_buffer(gpu, &[[1.0, -1.0], [1.0, 1.0], [-1.0, -1.0], [-1.0, 1.0]])?;
        let uvs = vertex_buffer(gpu, &[[1.0, 1.0], [1.0, 0.0], [0.0, 1.0], [0.0, 0.0]])?;
        let one = [0u8, 0x3c];
        let mut add = BlendKey::OPAQUE;
        let one_one = BlendTarget {
            color_op: D3D11_BLEND_OP_ADD.0 as u8,
            color_src: D3D11_BLEND_ONE.0 as u8,
            color_dst: D3D11_BLEND_ONE.0 as u8,
            alpha_op: D3D11_BLEND_OP_ADD.0 as u8,
            alpha_src: D3D11_BLEND_ONE.0 as u8,
            alpha_dst: D3D11_BLEND_ONE.0 as u8,
            write_mask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        add.targets[0] = one_one;
        add.targets[1] = one_one;
        Ok(Tlv {
            vs,
            gs,
            directional,
            local,
            local_inverse_squared,
            filter,
            positions,
            uvs,
            vs_globals: constants(gpu, 2)?,
            ps_globals: constants(gpu, globals::ROWS)?,
            light: constants(gpu, LIGHT_ROWS)?,
            filter_globals: constants(gpu, 3)?,
            raw: [[volume(gpu)?, volume(gpu)?], [volume(gpu)?, volume(gpu)?]],
            filtered: [[volume(gpu)?, volume(gpu)?], [volume(gpu)?, volume(gpu)?]],
            add: gpu.state(StateKey::Blend(add)),
            opaque: gpu.state(StateKey::Blend(BlendKey::OPAQUE)),
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
            // Unverified: the static shadow's whole depth map read point clamp (4.26), not read in the exe.
            point_clamp: gpu
                .state(StateKey::Sampler(SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_POINT, D3D11_TEXTURE_ADDRESS_CLAMP))),
            bilinear_clamp: gpu.state(StateKey::Sampler(SamplerKey::of(
                D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT,
                D3D11_TEXTURE_ADDRESS_CLAMP,
            ))),
            white: depth_texture(gpu, &one, 1, 1)?,
            shadows: StaticShadows::default(),
            cascades: [Cascade::default(); 2],
        })
    }

    /// The stage's static shadow depth maps (`static_shadows`).
    pub fn set_shadows(&mut self, shadows: StaticShadows) {
        self.shadows = shadows;
    }

    /// The cascades for `cam`, and their View members (TranslucencyLightingVolumeMin: the box's minimum, 1 / Dim;
    /// TranslucencyLightingVolumeInvSize: 1 / its size, the voxel size).
    pub fn set_view(&mut self, cam: &Camera, members: &mut ViewMembers) {
        self.cascades = cascades(cam);
        const MIN: [&str; 2] = ["TranslucencyLightingVolumeMin[0]", "TranslucencyLightingVolumeMin[1]"];
        const INV: [&str; 2] = ["TranslucencyLightingVolumeInvSize[0]", "TranslucencyLightingVolumeInvSize[1]"];
        for (k, c) in self.cascades.iter().enumerate() {
            members.set_f(MIN[k], [c.min[0], c.min[1], c.min[2], 1.0 / DIM as f32]);
            let inv = 1.0 / c.size;
            members.set_f(INV[k], [inv, inv, inv, c.size / DIM as f32]);
        }
    }

    /// The filtered volume as the base pass binds it: ambient inner, ambient outer, directional inner, directional
    /// outer (TranslucentBasePass 56, 58, 60, 62).
    pub fn srvs(&self) -> [Option<ID3D11ShaderResourceView>; 4] {
        let f = &self.filtered;
        [f[0][0].srv.clone(), f[1][0].srv.clone(), f[0][1].srv.clone(), f[1][1].srv.clone()]
    }

    /// Clears the volume, injects every light that affects translucency (no lighting channel test; the light set
    /// holds no Static light), then filters each cascade, each light with its static shadow map of `shadows` (by name).
    pub fn draw(&mut self, fr: &Frame, shadows: TlvShadows<'_>) -> Result<()> {
        let maps = match shadows {
            TlvShadows::Stage => Some(&self.shadows),
            TlvShadows::Menu(m) => m,
        };
        let gpu = fr.gpu;
        // Deviation: ClearRenderTargetView, where the build runs FClearTranslucentLightingVolumeCS (it stores zeros: the
        // same result).
        // SAFETY: views of this device's volume textures; the colour is four floats.
        unsafe {
            for t in self.raw.iter().flatten() {
                gpu.ctx.ClearRenderTargetView(t.rtv.as_ref().expect("a render target view"), &[0.0; 4]);
            }
        }
        // FWriteToSliceVS: MinZ 0, UVScaleBias of the whole volume.
        let mut vs = [[0u32; 4]; 2];
        put(&mut vs, 16, &[1.0, 1.0, 0.0, 0.0]);
        gpu.write_rows(&self.vs_globals, &vs);
        gpu.viewport([0, 0, DIM, DIM]);
        gpu.set_states(self.add, self.raster, self.depth, 0);
        gpu.set_geometry_shader(Some(self.gs));
        let lights = &fr.lights;
        // Deviation: every translucent light is injected (no ShouldRenderLight frustum / screen-radius cull),
        // each over the whole volume (the build rasterizes a local light's bounds; the PS discards past the
        // radius). The inverse-squared shader is picked by a 0 falloff exponent, not bInverseSquared (no stage light
        // has one).
        for l in lights.directional.iter().chain(&lights.radial).filter(|l| l.translucent) {
            let inject = match (l.kind, l.falloff_exponent == 0.0) {
                (LightKind::Directional, _) => &self.directional,
                (_, false) => &self.local,
                (_, true) => &self.local_inverse_squared,
            };
            gpu.write_rows(&self.light, &light_rows(l));
            let shadow = maps.and_then(|m| m.0.iter().find(|s| s.light == l.name));
            gpu.set_shaders(self.vs, Some(inject.ps));
            let mut srvs: [Option<ID3D11ShaderResourceView>; 3] = [None, None, None];
            srvs[inject.shadow_register as usize] = shadow.map_or(&self.white, |s| &s.texture).srv.clone();
            let point = gpu.sampler(self.point_clamp);
            let samplers = [Some(point.clone()), Some(point.clone()), Some(point)];
            for c in 0..2 {
                gpu.write_rows(&self.ps_globals, &inject_globals(l, shadow, c as u32));
                let rtvs = [self.raw[c][0].rtv.clone(), self.raw[c][1].rtv.clone()];
                let cbs =
                    [Some(self.ps_globals.buf.clone()), Some(fr.scene.view.buf.clone()), Some(self.light.buf.clone())];
                // SAFETY: the pipeline set above; buffers, views and samplers of this device.
                unsafe {
                    gpu.ctx.OMSetRenderTargets(Some(&rtvs), None);
                    gpu.ctx.VSSetConstantBuffers(0, Some(&[Some(self.vs_globals.buf.clone())]));
                    gpu.ctx.PSSetConstantBuffers(0, Some(&cbs));
                    gpu.ctx.PSSetShaderResources(0, Some(&srvs));
                    gpu.ctx.PSSetSamplers(0, Some(&samplers));
                }
                self.rasterize(gpu)?;
            }
        }

        // FilterTranslucentVolumeLighting: TexelSize 1 / Dim.
        let mut fg = [[0u32; 4]; 3];
        put(&mut fg, 36, &[1.0 / DIM as f32]);
        gpu.write_rows(&self.filter_globals, &fg);
        gpu.set_states(self.opaque, self.raster, self.depth, 0);
        gpu.set_shaders(self.vs, Some(self.filter));
        let bilinear = gpu.sampler(self.bilinear_clamp);
        for c in 0..2 {
            let rtvs = [self.filtered[c][0].rtv.clone(), self.filtered[c][1].rtv.clone()];
            let srvs = [self.raw[c][0].srv.clone(), self.raw[c][1].srv.clone()];
            // SAFETY: as above; the raw volume is read while the filtered one is written.
            unsafe {
                gpu.ctx.OMSetRenderTargets(Some(&rtvs), None);
                gpu.ctx.VSSetConstantBuffers(0, Some(&[Some(self.vs_globals.buf.clone())]));
                gpu.ctx.PSSetConstantBuffers(0, Some(&[Some(self.filter_globals.buf.clone())]));
                gpu.ctx.PSSetShaderResources(0, Some(&srvs));
                gpu.ctx.PSSetSamplers(0, Some(&[Some(bilinear.clone()), Some(bilinear.clone())]));
            }
            self.rasterize(gpu)?;
            // SAFETY: unbinds the targets before the next cascade reads nothing of them.
            unsafe { gpu.ctx.OMSetRenderTargets(None, None) };
        }
        gpu.set_geometry_shader(None);
        // SAFETY: unbinds what was bound above.
        unsafe {
            gpu.ctx.OMSetRenderTargets(None, None);
            gpu.ctx.PSSetShaderResources(0, Some(&[None, None, None]));
        }
        Ok(())
    }

    /// RasterizeToVolumeTexture: the quad once per slice (the geometry shader routes each to its slice).
    fn rasterize(&self, gpu: &Gpu) -> Result<()> {
        let elems = [
            InputElement {
                semantic: ATTRIBUTE,
                index: 0,
                format: DXGI_FORMAT_R32G32_FLOAT,
                slot: 0,
                per_instance: false,
            },
            InputElement {
                semantic: ATTRIBUTE,
                index: 1,
                format: DXGI_FORMAT_R32G32_FLOAT,
                slot: 1,
                per_instance: false,
            },
        ];
        gpu.set_input_layout(self.vs, LAYOUT_KEY, &elems)?;
        let vbs = [Some(self.positions.buf.clone()), Some(self.uvs.buf.clone())];
        let strides = [8u32, 8];
        let offsets = [0u32; 2];
        // SAFETY: two vertex buffers of this device with four vertices each.
        unsafe {
            gpu.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            gpu.ctx.IASetVertexBuffers(0, 2, Some(vbs.as_ptr()), Some(strides.as_ptr()), Some(offsets.as_ptr()));
            gpu.ctx.DrawInstanced(4, DIM, 0, 0);
        }
        Ok(())
    }
}

/// DeferredLightUniforms of `l` (FLightShaderParameters, 4.26 GetLightShaderParameters): Position and InvRadius,
/// Color and FalloffExponent, Direction (towards the light; a spot light's axis reversed) and SpecularScale, Tangent
/// and SourceRadius, SpotAngles, SoftSourceRadius and SourceLength. The rows before (the shadow map channel mask,
/// the distance fade, the contact shadow, the lighting channels) the inject shaders do not read.
fn light_rows(l: &Light) -> [[u32; 4]; LIGHT_ROWS] {
    let mut r = [[0u32; 4]; LIGHT_ROWS];
    let p = l.position.map(|x| x as f32);
    let d = l.direction.map(|x| -x);
    put(&mut r, 48, &[p[0], p[1], p[2], l.inv_radius]);
    put(&mut r, 64, &[l.color[0], l.color[1], l.color[2], l.falloff_exponent]);
    put(&mut r, 80, &[d[0], d[1], d[2], l.specular_scale]);
    put(&mut r, 96, &[l.tangent[0], l.tangent[1], l.tangent[2], l.source_radius]);
    put(&mut r, 112, &[l.spot_angles[0], l.spot_angles[1], l.soft_source_radius, l.source_length]);
    r
}

/// The inject shader's $Globals for `l` into cascade `cascade`. FVolumeShadowingParameters::Set with no shadow map
/// (InnerSplitIndex -1): ShadowInjectParams (1, 1, 0, 0) and ClippingPlanes (0, 0, 0, -1) twice, so the cascade
/// fade is 1. bStaticallyShadowed with the light's static shadow depth map, else the identity and size 0. The
/// volumetric cloud shadow and the atmosphere transmittance are off (no level has a cloud or a sky atmosphere):
/// FarDepthKm 1, the identity matrix.
fn inject_globals(l: &Light, shadow: Option<&StaticShadow>, cascade: u32) -> [[u32; 4]; globals::ROWS] {
    use globals::*;
    let mut r = [[0u32; 4]; ROWS];
    put(&mut r, SHADOW_INJECT_PARAMS, &[1.0, 1.0, 0.0, 0.0]);
    put(&mut r, CLIPPING_PLANES, &[0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0]);
    match shadow {
        Some(s) => {
            r[STATICALLY_SHADOWED as usize / 16][0] = 1;
            put(&mut r, WORLD_TO_STATIC_SHADOW, &s.world_to_light);
            let [w, h] = s.size.map(|x| x as f32);
            put(&mut r, STATIC_SHADOW_BUFFER_SIZE, &[w, h, 1.0 / w, 1.0 / h]);
        }
        None => put(&mut r, WORLD_TO_STATIC_SHADOW, &IDENTITY),
    }
    r[VOLUME_CASCADE_INDEX as usize / 16][0] = cascade;
    put(&mut r, CLOUD_SHADOW_MATRIX, &IDENTITY);
    put(&mut r, CLOUD_SHADOW_FAR_DEPTH, &[1.0]);
    r[CLOUD_SHADOW_ENABLED as usize / 16][1] = 0;
    put(&mut r, CLOUD_SHADOW_STRENGTH, &[0.0]);
    r[ATMOSPHERE_TRANSMITTANCE as usize / 16][3] = 0;
    put(&mut r, SPOTLIGHT_MASK, &[if l.kind == LightKind::Spot { 1.0 } else { 0.0 }]);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera(fov_half: f64) -> Camera {
        let t = 1.0 / fov_half.tan();
        let mut p = [[0.0; 4]; 4];
        p[0][0] = t;
        p[1][1] = t * 16.0 / 9.0;
        Camera {
            origin: [-1000.0, 10.0, 200.0],
            // UE view axes from world: X right = world Y, Y up = world Z, Z forward = world X.
            translated_world_to_view: [[0.0, 0.0, 1.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0; 4]],
            view_to_clip: p,
        }
    }

    #[test]
    fn the_cascades_hold_their_frustum_slices() {
        let c = cascades(&camera(0.3));
        // FOV clamped up to 45 degrees, then snapped up to 50.
        let h = 1500.0f32 * 50f32.to_radians().tan();
        for k in 0..3 {
            assert!(c[0].min[k] <= [-1000.0, 10.0 - h, 200.0][k]);
            assert!(c[0].min[k] + c[0].size >= [500.0, 10.0 + h, 200.0][k]);
        }
        assert!(c[1].size > c[0].size);
        // the centre snapped to twice the voxel size
        let step = c[0].size / DIM as f32;
        let centre = c[0].min[0] + c[0].size / 2.0;
        assert!((centre / step).fract().abs() < 1e-3 || (centre / step).fract().abs() > 1.0 - 1e-3);
    }

    #[test]
    fn a_spot_light_sets_the_spotlight_mask_and_its_cascade() {
        let l = Light {
            name: "s".into(),
            kind: LightKind::Spot,
            position: [0.0; 3],
            direction: [0.0, 0.0, -1.0],
            tangent: [0.0; 3],
            color: [1.0; 3],
            inv_radius: 0.001,
            falloff_exponent: 8.0,
            spot_angles: [0.5, 2.0],
            source_radius: 0.0,
            soft_source_radius: 0.0,
            source_length: 0.0,
            specular_scale: 1.0,
            channels: 1,
            shadow_channel: -1,
            cast_shadows: false,
            translucent: true,
        };
        let g = inject_globals(&l, None, 1);
        assert_eq!(f32::from_bits(g[51][0]), 1.0);
        assert_eq!(g[41][0], 1);
        assert_eq!(g[35][0], 0);
        assert_eq!(f32::from_bits(g[55][3]), -1.0);
        let r = light_rows(&l);
        assert_eq!(f32::from_bits(r[5][2]), 1.0);
        assert_eq!(f32::from_bits(r[7][0]), 0.5);
    }
}
