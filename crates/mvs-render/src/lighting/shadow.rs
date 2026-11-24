//! The dynamic shadows of movable spot lights with CastShadows (the menu fighters' rig lights): per light a
//! whole-scene perspective shadow (FProjectedShadowInfo flag 0x40), its depth map drawn with the casters' cooked
//! shadow depth shaders (material.rs PassKind::ShadowDepth), read by the deferred lighting with the build's
//! projection (deferred.hlsl SpotShadow).
//!
//! The maps share one R16 depth array (PF_ShadowDepth, R16_TYPELESS): each light's map a square tile of a slice,
//! a power of two with the build's border, the slices as large as the frame's largest tile. A light without
//! CastShadows, or out of the view, gets no map; the array is let go when a frame that is not a menu level's draws.
// Deviation: the build projects each shadow into LightAttenuation (B8G8R8A8 cleared white, its B and A taken by a
// Min blend) inside the stencil-marked shadow frustum, then draws that light; the port's one tiled lighting pass
// reads each light's map in place with the same arithmetic, the attenuation's 8-bit store included. Outside the
// frustum the build's attenuation is the white clear, and so is the port's.
// Deviation: the translucency lighting volume and translucent sections take no dynamic shadow (the menu fighters'
// translucent parts are lit unshadowed).

use mvs_d3d::Direct3D11::*;
use mvs_d3d::Dxgi::Common::*;
use mvs_view::{Camera, M4};

use super::{fill, in_frustum, shares};
use crate::Result;
use crate::draw::{PassInput, execute};
use crate::fx::warn_once;
use crate::gpu::{BufDesc, BufKind, Buffer, Gpu, dx};
use crate::material::PassKind;
use crate::renderer::Frame;
use crate::scene::{Light, LightKind, LightSet};

/// The shadow map's border, texels (ShadowInfo+0x550).
const BORDER: u32 = 4;
/// r.Shadow.MaxResolution at sg.ShadowQuality 3 (BaseScalability.ini @3).
const MAX_RESOLUTION: u32 = 2048;
/// r.Shadow.MinResolution and r.Shadow.FadeResolution (FUN_141a796e0), r.Shadow.FadeExponent (0x143a4f220).
const MIN_RESOLUTION: f32 = 32.0;
const FADE_RESOLUTION: f32 = 64.0;
const FADE_EXPONENT: f32 = 0.25;
/// r.Shadow.TexelsPerPixelSpotlight (8 / pi, 0x143ee070c).
const TEXELS_PER_PIXEL: f32 = 2.54648;
/// r.Shadow.SpotLightDepthBias (0x143a4f230), r.Shadow.SpotLightSlopeDepthBias, r.Shadow.ShadowMaxSlopeScaleDepthBias.
const SPOT_DEPTH_BIAS: f32 = 3.0;
const SPOT_SLOPE_BIAS: f32 = 3.0;
const MAX_SLOPE_BIAS: f32 = 1.0;
/// r.Shadow.SpotLightTransitionScale (0x143a4f258) and r.Shadow.SpotLightReceiverBias (0x143a4f224).
const TRANSITION_SCALE: f32 = 60.0;
const RECEIVER_BIAS: f32 = 0.5;
/// The light's ShadowBias, ShadowSlopeBias, ShadowSharpen and ShadowAmount (proxy +0xd4, +0xd8, +0xdc, +0x178): no
/// rig light template sets them (spot_shadows.md N7), so the class defaults.
// Unverified: the defaults (0.5, 0.5, 0, 1) are 4.26's ULightComponent ones, not read from the exe.
const SHADOW_BIAS: f32 = 0.5;
const SHADOW_SLOPE_BIAS: f32 = 0.5;
const SHADOW_SHARPEN: f32 = 0.0;
const SHADOW_AMOUNT: f32 = 1.0;
/// The ShadowDepthPass buffer's rows (the shaders read rows 19-28).
const PASS_ROWS: usize = 32;

/// A spot light's shadow frustum (FProjectedShadowInfo::SetupWholeSceneProjection of the spot's initializer).
// Unverified: the initializer (PreShadowTranslation the light's place, the light's X axis forward, scales
// 1 / tan(outer cone), MinLightW 0.1, MaxDistanceToCastInLightW the radius, the subject bounds the cone's bounding
// sphere) and the Z range from those bounds are 4.26's source: the exe's ShadowInfo+0x160 / +0x164 / +0x168 were
// named, not their sources (spot_shadows.md N2, N4).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Frustum {
    pos: [f64; 3],
    /// Right, up, forward: the shadow's x, y and depth axes, as the camera's (right x up = forward).
    axes: [[f64; 3]; 3],
    inv_tan: f64,
    min_z: f64,
    max_z: f64,
    /// 1 / MaxSubjectDepth (ShadowInfo+0x160): the subject sphere's far point, MaxZ (q2991 FUN_141d5afe0 498-515;
    /// the sphere radius only on the one-pass point path).
    inv_max_depth: f64,
    radius: f64,
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(a: [f64; 3]) -> [f64; 3] {
    let l = dot(a, a).sqrt();
    a.map(|v| v / l)
}

fn frustum_of(l: &Light) -> Frustum {
    let f = normalize(l.direction.map(f64::from));
    let up = if f[2].abs() < 0.999 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let r = normalize(cross(up, f));
    let u = cross(f, r);
    let cos = (l.spot_angles[0] as f64).clamp(1e-4, 1.0 - 1e-6);
    let sin = (1.0 - cos * cos).sqrt();
    let radius = 1.0 / l.inv_radius as f64;
    // FMath::ComputeBoundingSphereForCone: the subject bounds along the axis.
    let (centre, bound) = if cos < std::f64::consts::FRAC_1_SQRT_2 {
        (radius * cos, radius * sin)
    } else {
        let b = radius / (2.0 * cos);
        (b, b)
    };
    let max_z = centre + bound;
    let min_z = (max_z - 2.0 * bound).max(0.1);
    Frustum { pos: l.position, axes: [r, u, f], inv_tan: cos / sin, min_z, max_z, inv_max_depth: 1.0 / max_z, radius }
}

/// Translated world (`origin` the camera's) to the shadow's clip space, row vectors: x, y over the cone's tangent,
/// z = MaxZ (depth - MinZ) / (MaxZ - MinZ) (FShadowProjectionMatrix with WAxis (0, 0, 1, 0)), w the depth.
fn light_matrix(s: &Frustum, origin: [f64; 3]) -> M4 {
    let [r, u, f] = s.axes;
    let k = s.max_z / (s.max_z - s.min_z);
    let mut m = [[0.0; 4]; 4];
    for i in 0..3 {
        m[i] = [r[i] * s.inv_tan, u[i] * s.inv_tan, f[i] * k, f[i]];
    }
    let d = [0, 1, 2].map(|i| origin[i] - s.pos[i]);
    m[3] = [0, 1, 2, 3].map(|j| d[0] * m[0][j] + d[1] * m[1][j] + d[2] * m[2][j]);
    m[3][2] -= s.min_z * k;
    m
}

/// The light's screen radius in pixels: the sphere inscribed in the cone at its end, seen from the camera, times
/// the view's screen scale (FViewMatrices::GetScreenScale).
// Unverified: the proxy's ScreenRadius (vtable +0x60) is not decoded; this is 4.26's
// FSpotLightSceneProxy::GetEffectiveScreenRadius.
fn screen_radius(s: &Frustum, camera: &Camera, size: [u32; 2]) -> f32 {
    let p = &camera.view_to_clip;
    let scale = (0.5 * size[0] as f64 * p[0][0]).max(0.5 * size[1] as f64 * p[1][1]);
    let at = [0, 1, 2].map(|i| s.pos[i] + s.axes[2][i] * s.radius - camera.origin[i]);
    (scale * (s.radius / s.inv_tan) / dot(at, at).sqrt().max(1.0)) as f32
}

/// FUN_141d480b0: the shadow's fade by its unclamped resolution (spot_shadows.md N6 step 2).
fn fade_alpha(size: f32) -> f32 {
    if size > FADE_RESOLUTION {
        return 1.0;
    }
    if size <= MIN_RESOLUTION {
        return 0.0;
    }
    let x = 1.0 / (FADE_RESOLUTION - MIN_RESOLUTION);
    let first = x.powf(FADE_EXPONENT);
    (((size - MIN_RESOLUTION) * x).powf(FADE_EXPONENT) - first) / (1.0 - first)
}

/// The map's resolution inside its border and its fade, from the unclamped resolution (FUN_141d4b2a0, one view,
/// ShadowResolutionScale 1).
// Unverified: the clamp order after the scale step and the power-of-two step (the tile, border included,
// the next power of two of the clamped size plus both borders) are the spec's medium reading.
// Deviation: the rigs' ShadowResolutionScale (0.75 on 9 of the 104 templates) is not imported: every light takes 1.
fn resolution(unclamped: f32) -> (u32, f32) {
    let limit = (MAX_RESOLUTION - 2 * BORDER) as f32;
    let desired = unclamped.min(limit).max(MIN_RESOLUTION.min(limit));
    let tile = ((desired + (2 * BORDER) as f32).ceil() as u32).next_power_of_two();
    (tile - 2 * BORDER, fade_alpha(unclamped) * SHADOW_AMOUNT)
}

/// The constant and slope depth biases of a spot's map (FUN_141d2b920), in the map's depth units.
fn biases(resolution: u32, s: &Frustum) -> [f32; 2] {
    let range = (s.max_z - s.min_z) as f32;
    let depth = (SPOT_DEPTH_BIAS * 512.0 / (resolution as f32 * range) * 2.0 * SHADOW_BIAS).clamp(0.0, 0.1);
    [depth, (SHADOW_SLOPE_BIAS * SPOT_SLOPE_BIAS * depth).max(0.0)]
}

/// Square power-of-two tiles (`sizes`, any order) packed into slices of `slice` texels: each tile's (slice, x, y)
/// and the slices used. Every size is at most `slice`.
fn pack(sizes: &[u32], slice: u32) -> (Vec<[u32; 3]>, u32) {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes[i]));
    let mut free: Vec<[u32; 4]> = Vec::new();
    let mut slices = 0;
    let mut out = vec![[0; 3]; sizes.len()];
    for i in order {
        let s = sizes[i];
        // The smallest free square that holds it, split down to its size.
        let pick = free.iter().enumerate().filter(|(_, f)| f[3] >= s).min_by_key(|(_, f)| (f[3], f[0], f[2], f[1]));
        let mut sq = match pick {
            Some((k, _)) => free.swap_remove(k),
            None => {
                slices += 1;
                [slices - 1, 0, 0, slice]
            }
        };
        while sq[3] > s {
            let h = sq[3] / 2;
            free.push([sq[0], sq[1] + h, sq[2], h]);
            free.push([sq[0], sq[1], sq[2] + h, h]);
            free.push([sq[0], sq[1] + h, sq[2] + h, h]);
            sq[3] = h;
        }
        out[i] = [sq[0], sq[1], sq[2]];
    }
    (out, slices)
}

/// One light's shadow in the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Plan {
    /// The light's index in `LightSet::radial`.
    radial: usize,
    frustum: Frustum,
    /// ResolutionX = ResolutionY, inside the border.
    resolution: u32,
    /// ShadowFadeFraction (FadeAlphas[view]).
    fade: f32,
    /// The tile's slice and corner (the border's).
    tile: [u32; 3],
}

impl Plan {
    /// The ShadowDepthPass rows the depth shaders read (FUN_141d1a630): 19-22 the light's matrix with row 22 its
    /// translation from translated world, 23-25 the face axes (column 2 the light's axis, the slope bias's), 27
    /// ShadowParams, 28 the near clamp (0 for a spot).
    fn pass_rows(&self, origin: [f64; 3]) -> [[f32; 4]; PASS_ROWS] {
        let mut rows = [[0.0; 4]; PASS_ROWS];
        let m = light_matrix(&self.frustum, origin);
        for (k, row) in m.iter().enumerate() {
            rows[19 + k] = row.map(|v| v as f32);
        }
        let [r, u, f] = self.frustum.axes;
        for i in 0..3 {
            rows[23 + i] = [r[i] as f32, u[i] as f32, f[i] as f32, 0.0];
        }
        rows[26] = [0.0, 0.0, 0.0, 1.0];
        let [depth, slope] = biases(self.resolution, &self.frustum);
        rows[27] = [depth, slope, MAX_SLOPE_BIAS, self.frustum.inv_max_depth as f32];
        rows
    }

    /// The deferred pass's row (deferred.hlsl SpotShadow): translated world to (uv w, depth, w) of the array's
    /// slice (FUN_141d03f30's screen-to-shadow matrix from the G-buffer's position: the tile's place and
    /// 1 / MaxSubjectDepth on z), the slice, SoftTransitionScale, the fade and the sharpen, 1 - ReceiverBias and the
    /// buffer's size, the tile's inner rectangle in uv.
    fn gpu_row(&self, origin: [f64; 3], size: u32) -> [[f32; 4]; 7] {
        let m = light_matrix(&self.frustum, origin);
        let w = size as f64;
        let a = self.resolution as f64 / (2.0 * w);
        let x0 = (self.tile[1] + BORDER) as f64;
        let y0 = (self.tile[2] + BORDER) as f64;
        let bx = (x0 + self.resolution as f64 / 2.0) / w;
        let by = (y0 + self.resolution as f64 / 2.0) / w;
        let mut out = [[0.0; 4]; 7];
        for (k, r) in m.iter().enumerate() {
            out[k] = [r[0] * a + r[3] * bx, -r[1] * a + r[3] * by, r[2] * self.frustum.inv_max_depth, r[3]]
                .map(|v| v as f32);
        }
        let transition = 1.0 / (1.0 / TRANSITION_SCALE * SHADOW_BIAS * 2.0).max(1e-5);
        out[4] = [self.tile[0] as f32, transition, self.fade, SHADOW_SHARPEN * 7.0 + 1.0];
        out[5] = [1.0 - RECEIVER_BIAS, size as f32, (1.0 / w) as f32, 0.0];
        let res = self.resolution as f64;
        out[6] = [x0 / w, y0 / w, (x0 + res) / w, (y0 + res) / w].map(|v| v as f32);
        out
    }
}

/// 141ac3250's bounds test: a light's and a primitive's bounding spheres overlap unless (r_light + r_prim)^2 <
/// |c_light - c_prim|^2, in float32. `object` is the primitive's ObjectWorldPositionAndRadius row.
// Unverified: the light's bounding sphere is taken as its position and AttenuationRadius (the compact
// light's sphere and the proxy's AffectsBounds, vtable +0x8, are not read; a spot's cone sphere lies inside that
// one, so the test may take a primitive the build's leaves out). The primitive's sphere is the object row's
// ref-pose radius; the build's is the physics / posed bounds of the skeletal mesh, so a posed mesh that reaches
// beyond or short of its ref-pose extent differs.
// The build rejects on the less-than, so a NaN distance keeps the pair: not rewritten as a greater-or-equal.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn spheres_meet(l: &Light, object: &[u32; 4]) -> bool {
    let o = object.map(f32::from_bits);
    let d = [0, 1, 2].map(|k| l.position[k] as f32 - o[k]);
    let r = 1.0 / l.inv_radius + o[3];
    !(r * r < d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
}

/// The frame's shadows: the spot lights with CastShadows that light a drawn pixel, each sized by its screen radius.
fn plan(lights: &LightSet, masks: u8, camera: &Camera, size: [u32; 2]) -> Vec<Plan> {
    let to_clip = super::translated_world_to_clip(camera);
    let mut out = Vec::new();
    for (k, l) in lights.radial.iter().enumerate() {
        if !l.cast_shadows
            || !(0..8u8).any(|v| masks & (1 << v) != 0 && shares(l, v))
            || !in_frustum(l, camera.origin, &to_clip)
        {
            continue;
        }
        if l.kind != LightKind::Spot {
            // Deviation: a point light's shadow (the one-pass cube) is not ported; no rig point light casts.
            warn_once(format!("{}: a {:?} light's dynamic shadow is not drawn", l.name, l.kind));
            continue;
        }
        let frustum = frustum_of(l);
        let (resolution, fade) = resolution(screen_radius(&frustum, camera, size) * TEXELS_PER_PIXEL);
        // Unverified: a shadow whose fade is at most 1/256 is not made (4.26's test after the fade; the fade's
        // formula goes below 0 between MinResolution and MinResolution + 1).
        if fade <= 1.0 / 256.0 {
            continue;
        }
        out.push(Plan { radial: k, frustum, resolution, fade, tile: [0; 3] });
    }
    out
}

/// The depth array: its slices' size and count, the SRV the lighting reads and a DSV per slice.
#[derive(Debug)]
struct Maps {
    size: u32,
    slices: u32,
    srv: ID3D11ShaderResourceView,
    dsvs: Vec<ID3D11DepthStencilView>,
}

impl Maps {
    fn new(gpu: &Gpu, size: u32, slices: u32) -> Result<Maps> {
        let td = D3D11_TEXTURE2D_DESC {
            Width: size,
            Height: size,
            MipLevels: 1,
            ArraySize: slices,
            Format: DXGI_FORMAT_R16_TYPELESS,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_DEPTH_STENCIL.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let sd = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R16_UNORM,
            ViewDimension: mvs_d3d::Direct3D::D3D_SRV_DIMENSION_TEXTURE2DARRAY,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2DArray: D3D11_TEX2D_ARRAY_SRV {
                    MostDetailedMip: 0,
                    MipLevels: 1,
                    FirstArraySlice: 0,
                    ArraySize: slices,
                },
            },
        };
        // SAFETY: descriptions of this device's resource, filled above; the views are of the texture just made.
        unsafe {
            let mut t = None;
            gpu.dev.CreateTexture2D(&td, None, Some(&mut t)).map_err(dx("CreateTexture2D spot shadows"))?;
            let t = t.expect("CreateTexture2D");
            let mut srv = None;
            gpu.dev.CreateShaderResourceView(&t, Some(&sd), Some(&mut srv)).map_err(dx("spot shadows SRV"))?;
            let mut dsvs = Vec::with_capacity(slices as usize);
            for k in 0..slices {
                let dd = D3D11_DEPTH_STENCIL_VIEW_DESC {
                    Format: DXGI_FORMAT_D16_UNORM,
                    ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2DARRAY,
                    Flags: 0,
                    Anonymous: D3D11_DEPTH_STENCIL_VIEW_DESC_0 {
                        Texture2DArray: D3D11_TEX2D_ARRAY_DSV { MipSlice: 0, FirstArraySlice: k, ArraySize: 1 },
                    },
                };
                let mut v = None;
                gpu.dev.CreateDepthStencilView(&t, Some(&dd), Some(&mut v)).map_err(dx("spot shadows DSV"))?;
                dsvs.push(v.expect("CreateDepthStencilView"));
            }
            Ok(Maps { size, slices, srv: srv.expect("CreateShaderResourceView"), dsvs })
        }
    }
}

/// The spot shadows of the last frame drawn and what draws them.
#[derive(Debug, Default)]
pub struct SpotShadows {
    plans: Vec<Plan>,
    /// Per radial light of the frame: its row in `rows`.
    index: Vec<Option<u32>>,
    maps: Option<Maps>,
    /// The deferred pass's rows (deferred.hlsl SpotShadow).
    rows: Option<Buffer>,
    /// The ShadowDepthPass buffer.
    pass: Option<Buffer>,
}

impl SpotShadows {
    /// The frame's shadow maps drawn: each shadowed spot light's from the opaque items that cast
    /// (PassKind::ShadowDepth).
    pub fn draw(&mut self, fr: &mut Frame) -> Result<()> {
        self.plans.clear();
        self.index.clear();
        if !fr.list.opaque.iter().any(|it| it.passes.has(PassKind::ShadowDepth)) {
            // (no caster: every map would hold the clear, every attenuation 1)
            return Ok(());
        }
        let masks = fr.list.opaque.iter().fold(0u8, |m, it| m | 1 << (it.lighting_channels & 7));
        let camera = fr.input.camera;
        let mut plans = plan(fr.lights, masks, camera, fr.targets.size);
        if plans.is_empty() {
            return Ok(());
        }
        let gpu = fr.gpu;
        let tiles: Vec<u32> = plans.iter().map(|p| p.resolution + 2 * BORDER).collect();
        let largest = tiles.iter().copied().max().unwrap_or(1);
        let size = self.maps.as_ref().map_or(largest, |m| m.size.max(largest));
        let (at, slices) = pack(&tiles, size);
        if self.maps.as_ref().is_none_or(|m| m.size != size || m.slices < slices) {
            self.maps = None;
            self.maps = Some(Maps::new(gpu, size, slices)?);
        }
        for (p, t) in plans.iter_mut().zip(&at) {
            p.tile = *t;
        }
        let maps = self.maps.as_ref().expect("made above");

        let bytes: Vec<u8> =
            plans.iter().flat_map(|p| p.gpu_row(camera.origin, size)).flatten().flat_map(|v| v.to_le_bytes()).collect();
        fill(gpu, &mut self.rows, 7 * 16, &bytes)?;
        self.index = vec![None; fr.lights.radial.len()];
        for (k, p) in plans.iter().enumerate() {
            self.index[p.radial] = Some(k as u32);
        }

        if self.pass.is_none() {
            let d = BufDesc { kind: BufKind::Constant, size: (PASS_ROWS * 16) as u32, dynamic: true };
            self.pass = Some(gpu.buffer(&d, None)?);
        }
        let pass = self.pass.as_ref().expect("made above");
        // SAFETY: views of this device's array; each slice used is cleared to the far depth once.
        unsafe {
            for k in 0..slices {
                gpu.ctx.ClearDepthStencilView(&maps.dsvs[k as usize], D3D11_CLEAR_DEPTH.0, 1.0, 0);
            }
        }
        gpu.set_geometry_shader(None);
        let mut casters = Vec::new();
        for p in &plans {
            let rows = p.pass_rows(camera.origin).map(|r| r.map(f32::to_bits));
            gpu.write_rows(pass, &rows);
            // SAFETY: the slice's DSV, no colour target.
            unsafe { gpu.ctx.OMSetRenderTargets(None, &maps.dsvs[p.tile[0] as usize]) };
            // The map inside its border: the border keeps the clear.
            // Unverified: whether the build's viewport takes the border in with the casters drawn there is not read;
            // the projection reaches it only at the frustum's edge, where the cone term is 0.
            gpu.viewport([p.tile[1] + BORDER, p.tile[2] + BORDER, p.resolution, p.resolution]);
            let input = PassInput { buffer: &pass.buf, srvs: &[], samplers: &[] };
            // CreateWholeSceneProjectedShadow 141d4b2a0 walks the light's primitive interactions, which 141ac3250
            // makes only where the light's and the primitive's bounding spheres overlap and their lighting channel
            // masks share a bit.
            let light = &fr.lights.radial[p.radial];
            casters.clear();
            casters.extend(
                fr.list
                    .opaque
                    .iter()
                    .filter(|it| {
                        it.passes.has(PassKind::ShadowDepth)
                            && shares(light, it.lighting_channels & 7)
                            && spheres_meet(light, &fr.primitives.0[it.primitive as usize][5])
                    })
                    .cloned(),
            );
            execute(gpu, fr.assets, fr.scene, fr.targets, fr.materials, &casters, PassKind::ShadowDepth, Some(input))?;
        }
        // SAFETY: unbinds the depth target.
        unsafe { gpu.ctx.OMSetRenderTargets(None, None) };
        self.plans = plans;
        Ok(())
    }

    /// The depth array and the rows let go (a frame of no menu level).
    pub fn release(&mut self) {
        self.plans.clear();
        self.index.clear();
        self.maps = None;
        self.rows = None;
    }

    /// The row of radial light `k` of the last frame's lights; None when it has no map.
    pub(crate) fn row_of(&self, k: usize) -> Option<u32> {
        self.index.get(k).copied().flatten()
    }

    /// The rows' and the depth array's views (None without a map this frame).
    pub(crate) fn srvs(&self) -> [Option<ID3D11ShaderResourceView>; 2] {
        if self.plans.is_empty() {
            return [None, None];
        }
        [self.rows.as_ref().and_then(|b| b.srv.clone()), self.maps.as_ref().map(|m| m.srv.clone())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot(dir: [f32; 3], cos_outer: f32, radius: f32) -> Light {
        Light {
            name: String::new(),
            kind: LightKind::Spot,
            position: [100.0, 200.0, 300.0],
            direction: dir,
            tangent: dir,
            color: [1.0; 3],
            inv_radius: 1.0 / radius,
            falloff_exponent: 0.0,
            spot_angles: [cos_outer, 1.0],
            source_radius: 0.0,
            soft_source_radius: 0.0,
            source_length: 0.0,
            specular_scale: 1.0,
            channels: 1,
            shadow_channel: -1,
            cast_shadows: true,
            translucent: true,
        }
    }

    fn apply(m: &M4, p: [f64; 3]) -> [f64; 4] {
        [0, 1, 2, 3].map(|j| p[0] * m[0][j] + p[1] * m[1][j] + p[2] * m[2][j] + m[3][j])
    }

    /// A point on the axis maps to the centre at its depth; the cone's edge to the frustum's; the axes keep the
    /// camera's handedness.
    #[test]
    fn the_frustum_maps_the_cone() {
        let l = spot([0.0, 0.0, -1.0], 0.5f32.sqrt(), 500.0);
        let s = frustum_of(&l);
        assert!((dot(cross(s.axes[0], s.axes[1]), s.axes[2]) - 1.0).abs() < 1e-9);
        let origin = [10.0, 20.0, 30.0];
        let m = light_matrix(&s, origin);
        let tw = |p: [f64; 3]| [0, 1, 2].map(|i| p[i] - origin[i]);
        let c = apply(&m, tw([100.0, 200.0, 0.0]));
        assert!(c[0].abs() < 1e-9 && c[1].abs() < 1e-9 && (c[3] - 300.0).abs() < 1e-9, "{c:?}");
        // 45 degrees off the axis: the frustum's edge (x / w = 1 along the right axis).
        let r = s.axes[0];
        let e = apply(&m, tw([100.0 + 100.0 * r[0], 200.0 + 100.0 * r[1], 200.0 + 100.0 * r[2]]));
        assert!((e[0] / e[3] - 1.0).abs() < 1e-6, "{e:?}");
        // Z: 0 at MinZ, w at MaxZ.
        let near = apply(&m, tw([100.0, 200.0, 300.0 - s.min_z]));
        let far = apply(&m, tw([100.0, 200.0, 300.0 - s.max_z]));
        assert!(near[2].abs() < 1e-9 && (far[2] - far[3]).abs() < 1e-6, "{near:?} {far:?}");
        // A 45 degree cone's bounding sphere: radius R / (2 cos), centred as far along.
        assert!((s.max_z - 500.0 / 0.5f64.sqrt()).abs() < 1e-3 && s.min_z == 0.1, "{s:?}");
    }

    /// The deferred row puts the axis at the tile's centre and the cone's edge at the inner tile's edge.
    #[test]
    fn the_row_lands_in_the_tile() {
        let l = spot([1.0, 0.0, 0.0], 0.5f32.sqrt(), 400.0);
        let frustum = frustum_of(&l);
        let p = Plan { radial: 0, frustum, resolution: 1016, fade: 1.0, tile: [1, 1024, 0] };
        let row = p.gpu_row([0.0; 3], 2048);
        let at = |w: [f64; 3]| {
            let v = [0, 1, 2, 3].map(|j| {
                w[0] * row[0][j] as f64 + w[1] * row[1][j] as f64 + w[2] * row[2][j] as f64 + row[3][j] as f64
            });
            [v[0] / v[3], v[1] / v[3], v[2]]
        };
        let c = at([300.0, 200.0, 300.0]);
        assert!((c[0] - (1024.0 + 4.0 + 508.0) / 2048.0).abs() < 1e-5 && (c[1] - 512.0 / 2048.0).abs() < 1e-5);
        // Up the light's view is up the map: v decreases.
        let up = at([300.0, 200.0, 350.0]);
        assert!(up[1] < c[1], "{up:?} {c:?}");
        // The depth: light depth over MaxZ, near MinZ scaled by MaxZ / (MaxZ - MinZ).
        let k = frustum.max_z / (frustum.max_z - frustum.min_z);
        assert!((c[2] - k * (200.0 - frustum.min_z) / frustum.max_z).abs() < 1e-5, "{c:?}");
        let edge = at([300.0, 400.0, 300.0]);
        let [x0, _, x1, _] = row[6];
        assert!((edge[0] - x1 as f64).abs() < 1e-5 || (edge[0] - x0 as f64).abs() < 1e-5, "{edge:?} {:?}", row[6]);
    }

    #[test]
    fn resolutions() {
        // Past the cap: 2048 with the borders.
        assert_eq!(resolution(5000.0), (2040, 1.0));
        // A 1500 texel wish takes the next power of two.
        assert_eq!(resolution(1500.0).0, 2040);
        assert_eq!(resolution(1000.0).0, 1016);
        assert_eq!(resolution(10.0), (56, 0.0));
        // The fade between 32 and 64: 0 at the bottom (with the first value rescaled), 1 at the top.
        assert!(fade_alpha(33.0).abs() < 1e-6 && fade_alpha(32.5) < 0.0);
        assert!(fade_alpha(40.0) > 0.0 && fade_alpha(40.0) < 1.0);
        assert!((fade_alpha(64.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn bias() {
        let s = frustum_of(&spot([1.0, 0.0, 0.0], 0.5, 1000.0));
        let [d, slope] = biases(2040, &s);
        let range = (s.max_z - s.min_z) as f32;
        assert!((d - 3.0 * 512.0 / (2040.0 * range)).abs() < 1e-9);
        assert!((slope - 1.5 * d).abs() < 1e-9);
        // A small map's bias stops at 0.1.
        assert_eq!(biases(1, &frustum_of(&spot([1.0, 0.0, 0.0], 0.5, 1.0)))[0], 0.1);
    }

    #[test]
    fn tiles_pack() {
        let (at, slices) = pack(&[2048, 1024, 2048, 512, 512, 1024], 2048);
        assert_eq!(slices, 3);
        assert_eq!(at[0], [0, 0, 0]);
        assert_eq!(at[2], [1, 0, 0]);
        // The smaller ones share the third slice without overlapping.
        let rects: Vec<[u32; 4]> = [1, 3, 4, 5]
            .iter()
            .map(|&i| [at[i][0], at[i][1], at[i][2], [2048, 1024, 2048, 512, 512, 1024][i]])
            .collect();
        for (a, ra) in rects.iter().enumerate() {
            assert_eq!(ra[0], 2);
            for rb in &rects[a + 1..] {
                let apart = ra[1] + ra[3] <= rb[1]
                    || rb[1] + rb[3] <= ra[1]
                    || ra[2] + ra[3] <= rb[2]
                    || rb[2] + rb[3] <= ra[2];
                assert!(apart, "{ra:?} {rb:?}");
            }
        }
    }

    /// Only a casting spot light in view and on a drawn channel gets a map.
    #[test]
    fn which_lights_cast() {
        let mut m = [[0.0; 4]; 4];
        m[0][0] = 1.0;
        m[1][1] = 1.0;
        m[2][3] = 1.0;
        m[3][2] = 10.0;
        let camera = Camera { origin: [0.0; 3], translated_world_to_view: crate::view::identity(), view_to_clip: m };
        let ahead = |cast: bool, channels: u8| Light {
            position: [0.0, 0.0, 1000.0],
            cast_shadows: cast,
            channels,
            ..spot([0.0, 0.0, 1.0], 0.8, 300.0)
        };
        let set = LightSet {
            radial: vec![
                ahead(true, 1),
                ahead(false, 1),
                ahead(true, 4),
                Light { position: [0.0, 0.0, -5000.0], ..ahead(true, 1) },
            ],
            ..Default::default()
        };
        let plans = plan(&set, 0b10, &camera, [1920, 1080]);
        assert_eq!(plans.iter().map(|p| p.radial).collect::<Vec<_>>(), vec![0]);
    }
}
