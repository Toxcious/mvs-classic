//! The deferred lighting: the build's base pass writes the G-buffer unchanged, then one full-screen pass of the
//! lighting maths over every light, lighting channels by
//! stencil, and the sky light's specular.
//!
//! The pass is drawn once per lighting channel mask the frame's opaque draws wrote to the stencil (bits 4-6): the
//! stencil test keeps that mask's pixels, and the pass's light list holds the lights sharing a channel with it, so
//! no pixel loops over a light it does not take. Each draw adds the lights and the sky specular to SceneColor.
//!
//! The shader (lighting/deferred.hlsl) is the port's own HLSL, compiled once by d3dcompiler_47 (part of Windows)
//! when the renderer is made: its text is fixed, the light lists come in a buffer.

pub mod capture;
pub mod shadow;
pub mod tlv;

use std::ffi::c_void;

use mvs_view::{Camera, M4};
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCOMPILE_OPTIMIZATION_LEVEL3, D3DCompile};
use windows::Win32::Graphics::Direct3D::{D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, ID3DBlob};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::{Interface, PCSTR};

use crate::assets::Assets;
use crate::gpu::{
    BlendKey, BlendTarget, BufDesc, BufKind, Buffer, DepthKey, Gpu, RasterKey, SamplerKey, StateId, StateKey,
    StencilFace, StencilKey, Texture, dx,
};
use crate::renderer::Frame;
use crate::scene::{Light, LightKind, LightSet};
use crate::targets::SceneTargets;
use crate::view::{inverse, mul};
use crate::{Error, Result};
use shadow::SpotShadows;

const SOURCE: &str = include_str!("lighting/deferred.hlsl");

/// The stencil bits the base pass writes the lighting channels to (SetDepthStencilStateForBasePass 14193d4a0).
const CHANNEL_BITS: u8 = 0x70;

/// The stencil bits of a LightingChannelMask (bits 0-2 of `mask`) as the base pass writes them: GetLightingChannelStencil
/// (mask & 6 | !mask & 1, channel 0 flipped so the default mask is 0) shifted to bits 4-6.
pub fn channel_stencil(mask: u8) -> u8 {
    ((mask & 6) | (!mask & 1)) << 4
}

/// One light as deferred.hlsl's `Light` (six float4).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct GpuLight([[f32; 4]; 6]);

/// The pass constants (deferred.hlsl's `Constants`): 22 rows.
const PASS_ROWS: usize = 10 + 3 * MAX_CAPTURES;
/// The reflection captures the pass composites (deferred.hlsl MAX_CAPTURES).
const MAX_CAPTURES: usize = 4;

/// The screen tiles of the radial light lists, pixels square.
const TILE: u32 = 16;

/// The tiles across and down a screen of `size` pixels.
fn tile_count(size: [u32; 2]) -> [u32; 2] {
    size.map(|v| v.div_ceil(TILE))
}

/// The tiles (x0, y0, x1, y1, inclusive) a radial light's sphere covers on a screen of `size` pixels: the box of
/// its bounding cube's corners projected by `to_clip`, in pixels, divided by TILE as deferred.hlsl takes a pixel's
/// tile; every tile when a corner is not in front of the camera, None when the box is off the screen. `row` is the
/// light's GpuLight (translated position, 1 / radius).
fn tile_rect(row: &GpuLight, to_clip: &M4, size: [u32; 2]) -> Option<[u32; 4]> {
    let tiles = tile_count(size);
    let all = Some([0, 0, tiles[0] - 1, tiles[1] - 1]);
    let [x, y, z, inv_r] = row.0[0];
    if inv_r <= 0.0 {
        return all;
    }
    // A hair over the radius: the f32 position is rounded.
    let r = 1.001 / inv_r as f64;
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for k in 0..8 {
        let p = [
            x as f64 + if k & 1 == 0 { -r } else { r },
            y as f64 + if k & 2 == 0 { -r } else { r },
            z as f64 + if k & 4 == 0 { -r } else { r },
        ];
        let clip =
            [0, 1, 2, 3].map(|j| p[0] * to_clip[0][j] + p[1] * to_clip[1][j] + p[2] * to_clip[2][j] + to_clip[3][j]);
        if clip[3] <= 1e-3 {
            return all;
        }
        for a in 0..2 {
            lo[a] = lo[a].min(clip[a] / clip[3]);
            hi[a] = hi[a].max(clip[a] / clip[3]);
        }
    }
    if hi[0] < -1.0 || lo[0] > 1.0 || hi[1] < -1.0 || lo[1] > 1.0 {
        return None;
    }
    // Clip x, y to pixels (y down the screen), then to the tile holding that pixel.
    let tile = |px: f64, a: usize| ((px.floor().max(0.0) as u32) / TILE).min(tiles[a] - 1);
    let tx = |v: f64| tile((v * 0.5 + 0.5) * size[0] as f64, 0);
    let ty = |v: f64| tile((0.5 - v * 0.5) * size[1] as f64, 1);
    Some([tx(lo[0]), ty(hi[1]), tx(hi[0]), ty(lo[1])])
}

/// Each pass's tile words: per tile, `words` u32 whose bit j is the pass's radial light 32 w + j; the passes one
/// after the other. Returns the words and each pass's (first word, words per tile).
fn tile_words(rows: &[GpuLight], passes: &[PassRange], to_clip: &M4, size: [u32; 2]) -> (Vec<u32>, Vec<[u32; 2]>) {
    let tiles = tile_count(size);
    let n = (tiles[0] * tiles[1]) as usize;
    let mut out = Vec::new();
    let mut at = Vec::with_capacity(passes.len());
    for p in passes {
        let [_, _, first, count] = p.ranges;
        let words = count.div_ceil(32);
        let base = out.len();
        at.push([base as u32, words]);
        out.resize(base + n * words as usize, 0);
        for j in 0..count {
            let Some([x0, y0, x1, y1]) = tile_rect(&rows[(first + j) as usize], to_clip, size) else { continue };
            for ty in y0..=y1 {
                for tx in x0..=x1 {
                    out[base + (ty * tiles[0] + tx) as usize * words as usize + (j / 32) as usize] |= 1 << (j % 32);
                }
            }
        }
    }
    (out, at)
}

/// A dynamic structured buffer in `slot` of at least `bytes` (`stride` apart), grown by powers of two, written.
fn fill(gpu: &Gpu, slot: &mut Option<Buffer>, stride: u32, bytes: &[u8]) -> Result<Option<ID3D11ShaderResourceView>> {
    let count = (bytes.len() as u32).div_ceil(stride).max(1);
    if slot.as_ref().is_none_or(|b| b.desc.size < count * stride) {
        let size = count.next_power_of_two().max(16) * stride;
        *slot = Some(gpu.buffer(&BufDesc { kind: BufKind::Structured { stride }, size, dynamic: true }, None)?);
    }
    let b = slot.as_ref().expect("made above");
    if !bytes.is_empty() {
        gpu.write(b, bytes);
    }
    Ok(b.srv.clone())
}

/// DeferredLightUniforms' ShadowedBits (FUN_141acb690): 1 for a light that casts static shadows, 3 for one that casts
/// dynamic shadows (the view family's DynamicShadows flag is on), else 0.
// Unverified: a light with a precomputed shadow channel is taken as one that casts static shadows (proxy flag bit
// 0x20); the stage lights carry no CastShadows.
fn shadowed_bits(l: &Light) -> u32 {
    let mut bits = 0;
    if l.shadow_channel >= 0 {
        bits |= 1;
    }
    if l.cast_shadows {
        bits |= 3;
    }
    bits
}

/// The light rows of `l`: a directional light's direction and source terms, a radial one's position in translated
/// world (`origin` the camera's), attenuation, cone and tube, its shadow terms (`shadow` its dynamic shadow's row).
fn gpu_light(l: &Light, origin: [f64; 3], shadow: Option<u32>) -> GpuLight {
    let towards = l.direction.map(|v| -v);
    let mut r = [[0.0; 4]; 6];
    r[1] = [towards[0], towards[1], towards[2], 0.0];
    r[2] = [l.color[0], l.color[1], l.color[2], l.specular_scale];
    r[3] = [l.source_radius, l.soft_source_radius, 0.0, 0.0];
    if l.kind != LightKind::Directional {
        let p = [0, 1, 2].map(|i| (l.position[i] - origin[i]) as f32);
        r[0] = [p[0], p[1], p[2], l.inv_radius];
        r[1][3] = l.falloff_exponent;
        // A point light's SpotAngles are (-2, 1): its cone term is 1 everywhere (FPointLightSceneProxy).
        let cone = if l.kind == LightKind::Point { [-2.0, 1.0] } else { l.spot_angles };
        r[3][2] = cone[0];
        r[3][3] = cone[1];
        r[4] = [l.tangent[0], l.tangent[1], l.tangent[2], l.source_length];
        // The precomputed shadow factor's channel + 1, 0 for none; ShadowedBits; the dynamic shadow's row + 1, 0 for
        // none (deferred.hlsl `Shadow`).
        r[5] = [
            (l.shadow_channel.max(-1) + 1) as f32,
            shadowed_bits(l) as f32,
            shadow.map_or(0.0, |k| (k + 1) as f32),
            0.0,
        ];
    }
    GpuLight(r)
}

/// A light reaches a pixel when their lighting channels share a bit.
fn shares(l: &Light, mask: u8) -> bool {
    l.channels & mask != 0
}

/// One draw of the pass: the stencil value it lights and its ranges in the light buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PassRange {
    stencil: u8,
    /// First directional, directional count, first radial, radial count.
    ranges: [u32; 4],
}

/// Whether a radial light's sphere (its attenuation radius: its LightMask is 0 past it) reaches the view frustum
/// (`to_clip` TranslatedWorldToClip, row vectors; reversed Z, no far plane). A light outside lights no pixel.
fn in_frustum(l: &Light, origin: [f64; 3], to_clip: &M4) -> bool {
    if l.inv_radius <= 0.0 {
        return true;
    }
    let c = [0, 1, 2].map(|i| l.position[i] - origin[i]);
    let r = 1.0 / l.inv_radius as f64;
    let col = |j: usize| [to_clip[0][j], to_clip[1][j], to_clip[2][j], to_clip[3][j]];
    let (x, y, z, w) = (col(0), col(1), col(2), col(3));
    // Left, right, bottom, top and near (z <= w): each plane's (n, d) with n . p + d >= 0 inside.
    let planes = [
        [0, 1, 2, 3].map(|k| w[k] + x[k]),
        [0, 1, 2, 3].map(|k| w[k] - x[k]),
        [0, 1, 2, 3].map(|k| w[k] + y[k]),
        [0, 1, 2, 3].map(|k| w[k] - y[k]),
        [0, 1, 2, 3].map(|k| w[k] - z[k]),
    ];
    planes.iter().all(|p| {
        let len = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        p[0] * c[0] + p[1] * c[1] + p[2] * c[2] + p[3] >= -r * len
    })
}

/// The light buffer's rows and each draw's ranges, for the stencil values in `masks` (bit v: value v is drawn); the
/// radial lights outside the frustum of `to_clip` (TranslatedWorldToClip) are left out; `shadow_of` gives radial
/// light k's dynamic shadow row.
fn build_passes(
    lights: &LightSet,
    masks: u8,
    origin: [f64; 3],
    to_clip: &M4,
    shadow_of: &dyn Fn(usize) -> Option<u32>,
) -> (Vec<GpuLight>, Vec<PassRange>) {
    let mut rows = Vec::new();
    let mut passes = Vec::new();
    for v in 0..8u8 {
        if masks & (1 << v) == 0 {
            continue;
        }
        let first_dir = rows.len() as u32;
        rows.extend(lights.directional.iter().filter(|l| shares(l, v)).map(|l| gpu_light(l, origin, None)));
        let first_radial = rows.len() as u32;
        rows.extend(
            lights
                .radial
                .iter()
                .enumerate()
                .filter(|(_, l)| shares(l, v) && in_frustum(l, origin, to_clip))
                .map(|(k, l)| gpu_light(l, origin, shadow_of(k))),
        );
        let end = rows.len() as u32;
        if end == first_dir && lights.sky.is_none() {
            continue;
        }
        passes.push(PassRange {
            stencil: v,
            ranges: [first_dir, first_radial - first_dir, first_radial, end - first_radial],
        });
    }
    (rows, passes)
}

fn translated_world_to_clip(c: &Camera) -> M4 {
    mul(&c.translated_world_to_view, &c.view_to_clip)
}

/// HLSL of the port's own compiled by d3dcompiler_47.
pub(crate) fn compile(src: &str, entry: &str, target: &str) -> Result<Vec<u8>> {
    let e = format!("{entry}\0");
    let t = format!("{target}\0");
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the source pointer and length are the &str's; the entry and target are NUL-terminated; the blobs are
    // read within their reported sizes.
    unsafe {
        let r = D3DCompile(
            src.as_ptr() as *const c_void,
            src.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(e.as_ptr()),
            PCSTR(t.as_ptr()),
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        );
        let blob = |b: &ID3DBlob| std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize());
        if let Err(source) = r {
            let msg = errors.as_ref().map(|b| String::from_utf8_lossy(blob(b)).into_owned()).unwrap_or_default();
            return Err(Error::Format { what: format!("D3DCompile {entry}"), why: format!("{source}: {msg}") });
        }
        Ok(blob(code.as_ref().expect("D3DCompile gave no code")).to_vec())
    }
}

/// The pass's shaders and buffers.
#[derive(Debug)]
pub struct Lighting {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    constants: Buffer,
    /// The light rows, grown to the largest frame's.
    lights: Option<Buffer>,
    /// The radial lights' tile words.
    tiles: Option<Buffer>,
    /// A read-only view of the scene depth-stencil (the pass tests the stencil while it reads the depth), by the
    /// resource it was made for.
    depth_view: Option<(usize, ID3D11DepthStencilView)>,
    blend: StateId,
    raster: StateId,
    depth: StateId,
    sampler: StateId,
    /// ShadowDepthTextureSampler: point, clamp (FUN_141d28320's registration; the projection reads the map by
    /// Gather4).
    point: StateId,
}

impl Lighting {
    pub fn new(gpu: &Gpu, assets: &Assets) -> Result<Lighting> {
        let _ = assets;
        let vs_code = compile(SOURCE, "MainVS", "vs_5_0")?;
        let ps_code = compile(SOURCE, "MainPS", "ps_5_0")?;
        let (mut vs, mut ps) = (None, None);
        // SAFETY: DXBC just compiled for this device's shader model.
        unsafe {
            gpu.dev.CreateVertexShader(&vs_code, None, Some(&mut vs)).map_err(dx("CreateVertexShader lighting"))?;
            gpu.dev.CreatePixelShader(&ps_code, None, Some(&mut ps)).map_err(dx("CreatePixelShader lighting"))?;
        }
        let constants =
            gpu.buffer(&BufDesc { kind: BufKind::Constant, size: (PASS_ROWS * 16) as u32, dynamic: true }, None)?;
        // SceneColor += the pass's colour; its alpha is kept.
        let add = BlendTarget {
            color_op: D3D11_BLEND_OP_ADD.0 as u8,
            color_src: D3D11_BLEND_ONE.0 as u8,
            color_dst: D3D11_BLEND_ONE.0 as u8,
            alpha_op: D3D11_BLEND_OP_ADD.0 as u8,
            alpha_src: D3D11_BLEND_ZERO.0 as u8,
            alpha_dst: D3D11_BLEND_ONE.0 as u8,
            write_mask: (D3D11_COLOR_WRITE_ENABLE_RED.0
                | D3D11_COLOR_WRITE_ENABLE_GREEN.0
                | D3D11_COLOR_WRITE_ENABLE_BLUE.0) as u8,
        };
        let mut targets = [BlendTarget::OPAQUE; 8];
        targets[0] = add;
        let equal = StencilFace {
            func: D3D11_COMPARISON_EQUAL.0 as u8,
            fail: D3D11_STENCIL_OP_KEEP.0 as u8,
            depth_fail: D3D11_STENCIL_OP_KEEP.0 as u8,
            pass: D3D11_STENCIL_OP_KEEP.0 as u8,
        };
        Ok(Lighting {
            vs: vs.expect("CreateVertexShader"),
            ps: ps.expect("CreatePixelShader"),
            constants,
            lights: None,
            tiles: None,
            depth_view: None,
            blend: gpu.state(StateKey::Blend(BlendKey { targets, alpha_to_coverage: false })),
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
                stencil: Some(StencilKey { front: equal, back: equal, read_mask: CHANNEL_BITS, write_mask: 0 }),
            })),
            sampler: gpu
                .state(StateKey::Sampler(SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_TEXTURE_ADDRESS_CLAMP))),
            point: gpu
                .state(StateKey::Sampler(SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_POINT, D3D11_TEXTURE_ADDRESS_CLAMP))),
        })
    }

    /// The G-buffer and depth lit into SceneColor by `fr.lights`, with the frame's spot shadows.
    pub fn draw(&mut self, fr: &mut Frame, shadows: Option<&SpotShadows>) -> Result<()> {
        let masks = fr.list.opaque.iter().fold(0u8, |m, it| m | 1 << (it.lighting_channels & 7));
        self.light(fr.gpu, fr.targets, fr.lights, fr.input.camera, masks, shadows)
    }

    /// Lights `targets` (SceneColor holding the base pass's colour, the G-buffer, the depth and the channel bits in
    /// the stencil) by `lights` from `camera`, for the pixels whose stencil channel value v has bit v of `masks` set;
    /// `shadows` the frame's spot shadow maps (drawn for `lights`).
    pub fn light(
        &mut self,
        gpu: &Gpu,
        targets: &SceneTargets,
        lights: &LightSet,
        camera: &Camera,
        masks: u8,
        shadows: Option<&SpotShadows>,
    ) -> Result<()> {
        let to_clip = translated_world_to_clip(camera);
        let shadow_of = |k: usize| shadows.and_then(|s| s.row_of(k));
        let (rows, passes) = build_passes(lights, masks, camera.origin, &to_clip, &shadow_of);
        if passes.is_empty() {
            return Ok(());
        }
        let bytes: Vec<u8> = rows.iter().flat_map(|l| l.0.iter().flatten()).flat_map(|f| f.to_le_bytes()).collect();
        let light_srv = fill(gpu, &mut self.lights, std::mem::size_of::<GpuLight>() as u32, &bytes)?;
        let [w, h] = targets.size;
        let tiles = tile_count([w, h]);
        // Deviation: radial lights are listed per screen tile (and culled by the frustum in build_passes) where the
        // build draws each light's geometry; both culls are conservative and change no pixel.
        let (words, tile_at) = tile_words(&rows, &passes, &to_clip, [w, h]);
        let bytes: Vec<u8> = words.iter().flat_map(|v| v.to_le_bytes()).collect();
        let tile_srv = fill(gpu, &mut self.tiles, 4, &bytes)?;

        // Clip space (x, y, device z, 1) to translated world.
        let to_world = inverse(&to_clip);
        let mut capture_srvs: [Option<ID3D11ShaderResourceView>; MAX_CAPTURES] = Default::default();
        let mut capture_rows = [[0.0f32; 4]; 1 + 3 * MAX_CAPTURES];
        if let Some(s) = &lights.sky {
            if s.captures.len() > MAX_CAPTURES {
                return Err(Error::Format {
                    what: "reflection captures".into(),
                    why: format!("{} captures, the pass takes {MAX_CAPTURES}", s.captures.len()),
                });
            }
            // ReflectionCubemapMaxMip: the captures' mips below the top.
            // Unverified: the View member's value (FloorLog2 of the capture size) is 4.26 source.
            let max_mip = s.captures.first().map_or(0, |c| c.cube.desc.mips - 1);
            capture_rows[0] = [s.captures.len() as f32, max_mip as f32, 0.0, 0.0];
            for (k, c) in s.captures.iter().enumerate() {
                if c.cube.desc.mips - 1 != max_mip {
                    return Err(Error::Format {
                        what: "reflection captures".into(),
                        why: "captures of different sizes (the build's cube array has one)".into(),
                    });
                }
                let p = [0, 1, 2].map(|i| (c.position[i] - camera.origin[i]) as f32);
                capture_rows[1 + 3 * k] = [p[0], p[1], p[2], c.influence_radius];
                capture_rows[2 + 3 * k] = [c.brightness, k as f32, 0.0, 0.0];
                let o = c.capture_offset;
                capture_rows[3 + 3 * k] = [o[0], o[1], o[2], c.average_brightness];
                capture_srvs[k] = c.cube.srv.clone();
            }
        }
        let (sky_params, sky_color, sky_srvs) = match &lights.sky {
            Some(s) => {
                check_gf(&s.pre_integrated_gf)?;
                (
                    [(s.cube.desc.mips - 1) as f32, s.brightness, 1.0, 0.0],
                    [s.color[0], s.color[1], s.color[2], 0.0],
                    [s.cube.srv.clone(), s.pre_integrated_gf.srv.clone()],
                )
            }
            None => ([0.0; 4], [0.0; 4], [None, None]),
        };
        let mut pass = [[0u32; 4]; PASS_ROWS];
        for (r, row) in to_world.iter().enumerate() {
            pass[r] = row.map(|v| (v as f32).to_bits());
        }
        pass[4] = [2.0 / w as f32, 2.0 / h as f32, 0.0, 0.0].map(f32::to_bits);
        pass[6] = sky_params.map(f32::to_bits);
        pass[7] = sky_color.map(f32::to_bits);
        for (k, row) in capture_rows.iter().enumerate() {
            pass[9 + k] = row.map(f32::to_bits);
        }

        let dsv = self.depth_view(gpu, &targets.depth)?;
        let [shadow_rows, shadow_maps] = shadows.map_or([None, None], SpotShadows::srvs);
        let t = targets;
        let srvs = [
            light_srv,
            t.depth.srv.clone(),
            t.gbuffer[0].srv.clone(),
            t.gbuffer[1].srv.clone(),
            t.gbuffer[2].srv.clone(),
            t.gbuffer[4].srv.clone(),
            sky_srvs[0].clone(),
            sky_srvs[1].clone(),
            tile_srv,
            shadow_rows,
            shadow_maps,
            capture_srvs[0].clone(),
            capture_srvs[1].clone(),
            capture_srvs[2].clone(),
            capture_srvs[3].clone(),
        ];
        gpu.viewport([0, 0, w, h]);
        gpu.set_geometry_shader(None);
        let sampler = gpu.sampler(self.sampler);
        let point = gpu.sampler(self.point);
        // SAFETY: shaders, views and buffers of this device; the depth-stencil is bound read-only while its depth
        // is read; everything bound here is unbound before the return.
        unsafe {
            let ctx = &gpu.ctx;
            ctx.OMSetRenderTargets(Some(std::slice::from_ref(&t.scene_color.rtv)), &dsv);
            ctx.IASetInputLayout(None);
            ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.VSSetShader(&self.vs, None);
            ctx.PSSetShader(&self.ps, None);
            ctx.PSSetShaderResources(0, Some(&srvs));
            ctx.PSSetSamplers(0, Some(&[Some(sampler), Some(point)]));
            ctx.PSSetConstantBuffers(0, Some(&[Some(self.constants.buf.clone())]));
        }
        for (p, at) in passes.iter().zip(&tile_at) {
            pass[5] = p.ranges;
            pass[8] = [tiles[0], at[0], at[1], TILE];
            gpu.write_rows(&self.constants, &pass);
            gpu.set_states(self.blend, self.raster, self.depth, channel_stencil(p.stencil) as u32);
            // SAFETY: the pipeline set above.
            unsafe { gpu.ctx.Draw(3, 0) };
        }
        // SAFETY: unbinds what was bound above.
        unsafe {
            gpu.ctx.PSSetShaderResources(0, Some(&[const { None }; 15]));
            gpu.ctx.OMSetRenderTargets(None, None);
        }
        Ok(())
    }

    /// The read-only depth-stencil view of `depth`, made once per resource.
    fn depth_view(&mut self, gpu: &Gpu, depth: &Texture) -> Result<ID3D11DepthStencilView> {
        let key = depth.res.as_raw() as usize;
        if let Some((k, v)) = &self.depth_view
            && *k == key
        {
            return Ok(v.clone());
        }
        let format = match depth.desc.format {
            DXGI_FORMAT_R32G8X24_TYPELESS => DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
            DXGI_FORMAT_R24G8_TYPELESS => DXGI_FORMAT_D24_UNORM_S8_UINT,
            f => {
                return Err(Error::Format {
                    what: "lighting".into(),
                    why: format!("depth format {} has no stencil", f.0),
                });
            }
        };
        let d = D3D11_DEPTH_STENCIL_VIEW_DESC {
            Format: format,
            ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
            Flags: (D3D11_DSV_READ_ONLY_DEPTH.0 | D3D11_DSV_READ_ONLY_STENCIL.0) as u32,
            Anonymous: D3D11_DEPTH_STENCIL_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_DSV { MipSlice: 0 } },
        };
        let mut v = None;
        // SAFETY: a view description of this device's depth resource.
        unsafe { gpu.dev.CreateDepthStencilView(&depth.res, Some(&d), Some(&mut v)).map_err(dx("read-only DSV"))? };
        let v = v.expect("CreateDepthStencilView");
        self.depth_view = Some((key, v.clone()));
        Ok(v)
    }
}

/// The PreIntegratedGF table is read as two unorm 16-bit channels (the sky kind's RG16).
fn check_gf(t: &Texture) -> Result<()> {
    if t.desc.format != DXGI_FORMAT_R16G16_UNORM {
        return Err(Error::Format {
            what: "lighting".into(),
            why: format!("PreIntegratedGF format {}, R16G16_UNORM wanted", t.desc.format.0),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(kind: LightKind, channels: u8) -> Light {
        Light {
            name: String::new(),
            kind,
            position: [100.0, 200.0, 300.0],
            direction: [0.0, 0.0, -1.0],
            tangent: [1.0, 0.0, 0.0],
            color: [1.0, 2.0, 3.0],
            inv_radius: 0.01,
            falloff_exponent: 8.0,
            spot_angles: [0.5, 2.0],
            source_radius: 4.0,
            soft_source_radius: 5.0,
            source_length: 0.0,
            specular_scale: 1.0,
            channels,
            shadow_channel: -1,
            cast_shadows: false,
            translucent: true,
        }
    }

    #[test]
    fn passes_take_the_lights_sharing_a_channel() {
        let set = LightSet {
            directional: vec![light(LightKind::Directional, 1), light(LightKind::Directional, 2)],
            radial: vec![light(LightKind::Point, 2), light(LightKind::Spot, 6)],
            ..Default::default()
        };
        // Values 0 (no lights, no sky: not drawn), 1, 2 and 4.
        // A clip transform with every point inside (w = 1, x = y = z = 0).
        let mut all = [[0.0; 4]; 4];
        all[3][3] = 1.0;
        let (rows, passes) = build_passes(&set, 0b1_0111, [0.0; 3], &all, &|_| None);
        assert_eq!(
            passes,
            vec![
                PassRange { stencil: 1, ranges: [0, 1, 1, 0] },
                PassRange { stencil: 2, ranges: [1, 1, 2, 2] },
                PassRange { stencil: 4, ranges: [4, 0, 4, 1] },
            ]
        );
        assert_eq!(rows.len(), 5);
    }

    #[test]
    fn radial_lights_outside_the_frustum_are_culled() {
        // Looking along +Z: clip (x, y, 10, z), the near plane at z = 10.
        let mut m = [[0.0; 4]; 4];
        m[0][0] = 1.0;
        m[1][1] = 1.0;
        m[3][2] = 10.0;
        m[2][3] = 1.0;
        let at = |p: [f64; 3]| Light { position: p, ..light(LightKind::Point, 1) };
        assert!(in_frustum(&at([0.0, 0.0, 1000.0]), [0.0; 3], &m));
        assert!(!in_frustum(&at([0.0, 0.0, -1000.0]), [0.0; 3], &m));
        assert!(!in_frustum(&at([2000.0, 0.0, 1000.0]), [0.0; 3], &m));
        // Its sphere (radius 100) reaches over the right plane.
        assert!(in_frustum(&at([1050.0, 0.0, 1000.0]), [0.0; 3], &m));
        // Behind the camera's origin moved by the translation.
        assert!(!in_frustum(&at([0.0, 0.0, 1000.0]), [0.0, 0.0, 2000.0], &m));
    }

    #[test]
    fn radial_lights_mark_the_tiles_their_sphere_reaches() {
        // Clip (x, y, 10, z): at z = 1000 the screen spans x, y in -1000..1000.
        let mut m = [[0.0; 4]; 4];
        m[0][0] = 1.0;
        m[1][1] = 1.0;
        m[3][2] = 10.0;
        m[2][3] = 1.0;
        let row = |p: [f64; 3]| gpu_light(&Light { position: p, ..light(LightKind::Point, 1) }, [0.0; 3], None);
        // Radius 100 about the screen's centre: ndc +-0.1 (a little more for the cube's near corners).
        assert_eq!(tile_rect(&row([0.0, 0.0, 1000.0]), &m, [160, 160]), Some([4, 4, 5, 5]));
        // The top left corner: y up the world is the top of the screen.
        assert_eq!(tile_rect(&row([-950.0, 950.0, 1000.0]), &m, [160, 160]), Some([0, 0, 1, 1]));
        assert_eq!(tile_rect(&row([3000.0, 0.0, 1000.0]), &m, [160, 160]), None);
        // A corner behind the camera: every tile.
        assert_eq!(tile_rect(&row([0.0, 0.0, 50.0]), &m, [160, 160]), Some([0, 0, 9, 9]));

        let rows = [row([-950.0, 950.0, 1000.0]), row([0.0, 0.0, 50.0])];
        let passes = [PassRange { stencil: 1, ranges: [0, 0, 0, 2] }, PassRange { stencil: 2, ranges: [0, 0, 1, 1] }];
        let (words, at) = tile_words(&rows, &passes, &m, [160, 160]);
        assert_eq!(at, [[0, 1], [100, 1]]);
        assert_eq!(words.len(), 200);
        assert_eq!(words[0], 0b11);
        assert_eq!(words[11], 0b11);
        assert_eq!(words[99], 0b10);
        assert_eq!(words[100], 1);
        assert_eq!(words[199], 1);
    }

    #[test]
    fn a_light_box_holds_every_tile_whose_pixels_see_its_sphere() {
        for size in [[1366u32, 768], [1920, 1080]] {
            let [w, h] = size.map(|v| v as f64);
            // Square pixels, the near plane at z = 10: pixel centre (u, v) in clip looks along (u w / h, v, 1).
            let mut m = [[0.0; 4]; 4];
            m[0][0] = h / w;
            m[1][1] = 1.0;
            m[3][2] = 10.0;
            m[2][3] = 1.0;
            let mut lights = Vec::new();
            for z in [300.0, 1000.0, 2500.0] {
                for k in 0..7 {
                    let x = (k as f64 - 3.0) * 0.6 * z;
                    let y = (k as f64 % 3.0 - 1.0) * 0.55 * z;
                    lights.push(([x, y, z], 37.0 + 11.0 * k as f64));
                }
            }
            for (c, r) in lights {
                let row = gpu_light(
                    &Light { position: c, inv_radius: (1.0 / r) as f32, ..light(LightKind::Point, 1) },
                    [0.0; 3],
                    None,
                );
                let rect = tile_rect(&row, &m, size);
                for j in 0..size[1] {
                    for i in 0..size[0] {
                        let u = ((i as f64 + 0.5) / w * 2.0 - 1.0) * w / h;
                        let v = 1.0 - (j as f64 + 0.5) / h * 2.0;
                        let d = [u, v, 1.0];
                        let dd = d[0] * d[0] + d[1] * d[1] + 1.0;
                        let t = (c[0] * d[0] + c[1] * d[1] + c[2]) / dd;
                        let e = [c[0] - t * d[0], c[1] - t * d[1], c[2] - t];
                        if t <= 0.0 || e[0] * e[0] + e[1] * e[1] + e[2] * e[2] >= r * r {
                            continue;
                        }
                        let [x0, y0, x1, y1] =
                            rect.unwrap_or_else(|| panic!("{size:?} {c:?}: pixel {i} {j} lit, no tiles"));
                        let (tx, ty) = (i / TILE, j / TILE);
                        assert!(
                            (x0..=x1).contains(&tx) && (y0..=y1).contains(&ty),
                            "{size:?} {c:?} r {r}: pixel {i} {j} (tile {tx} {ty}) outside {:?}",
                            [x0, y0, x1, y1]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn light_rows() {
        let d = gpu_light(&light(LightKind::Directional, 1), [0.0; 3], None);
        assert_eq!(d.0[1], [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(d.0[0], [0.0; 4]);
        let p = gpu_light(&light(LightKind::Point, 1), [100.0, 0.0, 0.0], None);
        assert_eq!(p.0[0], [0.0, 200.0, 300.0, 0.01]);
        assert_eq!(p.0[1], [0.0, 0.0, 1.0, 8.0]);
        assert_eq!(p.0[3], [4.0, 5.0, -2.0, 1.0]);
        let s = gpu_light(&light(LightKind::Spot, 1), [0.0; 3], None);
        assert_eq!(s.0[3], [4.0, 5.0, 0.5, 2.0]);
        assert_eq!(s.0[5], [0.0; 4]);
        // A casting spot with its shadow's row 2, a static one's with channel 1.
        let c = Light { cast_shadows: true, ..light(LightKind::Spot, 1) };
        assert_eq!(gpu_light(&c, [0.0; 3], Some(2)).0[5], [0.0, 3.0, 3.0, 0.0]);
        let st = Light { shadow_channel: 1, ..light(LightKind::Spot, 1) };
        assert_eq!(gpu_light(&st, [0.0; 3], None).0[5], [2.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn the_shader_compiles() {
        compile(SOURCE, "MainVS", "vs_5_0").unwrap();
        compile(SOURCE, "MainPS", "ps_5_0").unwrap();
    }
}
