//! A captured frame's lighting inputs from a capture's files: the stage's lights and processed sky as the capture held
//! them (`<dir>/*-lights.json`, `<dir>/sky/`), and its base pass targets (`<dir>/gbuffer/`: SceneColor, GBufferA to
//! E, the device depth and the stencil's channel bits). render-frame's check of the lighting pass on the capture's own
//! G-buffer, and its light set while the stage does not give one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use windows::Win32::Graphics::Direct3D::D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

use super::compile;
use crate::gpu::{
    BlendKey, BufDesc, BufKind, DepthKey, Gpu, RasterKey, StateKey, StencilFace, StencilKey, Subresource, TexDesc,
    TexKind, TexViews, Texture, dx,
};
use crate::scene::{Light, LightKind, LightSet, SkyLight};
use crate::targets::SceneTargets;
use crate::{Error, Result};

fn bad(what: &Path, why: impl Into<String>) -> Error {
    Error::Format { what: what.display().to_string(), why: why.into() }
}

fn read(p: &Path) -> Result<Vec<u8>> {
    std::fs::read(p).map_err(|e| bad(p, e.to_string()))
}

fn json(p: &Path) -> Result<Value> {
    serde_json::from_slice(&read(p)?).map_err(|e| bad(p, e.to_string()))
}

fn nums<const N: usize>(v: &Value, what: &Path, field: &str) -> Result<[f64; N]> {
    let a: Vec<f64> = v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    a.try_into().map_err(|a: Vec<f64>| bad(what, format!("{field}: {} numbers, {N} wanted", a.len())))
}

fn channels(l: &Value) -> u8 {
    l["channels"]
        .as_array()
        .map(|c| c.iter().enumerate().fold(0, |m, (k, on)| m | ((on.as_bool() == Some(true)) as u8) << k))
        .unwrap_or(0)
}

/// An 8-bit linear value as an sRGB render target stores it (D3D's float -> sRGB conversion, rounded).
fn linear_to_srgb_byte(b: u8) -> u8 {
    let c = b as f64 / 255.0;
    let s = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// FLinearColor(FColor): the sRGB curve on each 0..1 channel.
fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// The `*-lights.json` of `dir`.
fn lights_file(dir: &Path) -> Result<PathBuf> {
    let rd = std::fs::read_dir(dir).map_err(|e| bad(dir, e.to_string()))?;
    rd.flatten()
        .map(|e| e.path())
        .find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with("-lights.json")))
        .ok_or_else(|| bad(dir, "no *-lights.json"))
}

/// The light set of a captured frame: the stage's dynamic directional lights (affectsWorld, not Static) and its
/// radial lights as the capture lit them, and
/// its processed sky.
pub fn light_set(gpu: &Gpu, dir: &Path) -> Result<LightSet> {
    let path = lights_file(dir)?;
    let st = json(&path)?;
    let mut set = LightSet::default();
    for l in st["lights"].as_array().ok_or_else(|| bad(&path, "no lights"))? {
        // Deviation: a Static light lights nothing dynamic in the build, but the capture's fighter rig
        // takes every channel-0 light that affects the world, whatever its mobility.
        let rig = channels(l) & 1 != 0;
        if l["affectsWorld"].as_bool() != Some(true) || (l["mobility"].as_str() == Some("Static") && !rig) {
            continue;
        }
        let name = l["name"].as_str().unwrap_or("?").to_string();
        let [p, y, _] = nums::<3>(&l["rot"], &path, "rot")?.map(f64::to_radians);
        let color = nums::<3>(&l["color"], &path, "color")?;
        let intensity = l["intensity"].as_f64().ok_or_else(|| bad(&path, "intensity"))?;
        // FDirectionalLightSceneProxy::GetLightShaderParameters (exe 1429680dd): radius = sin(0.5 x angle), in f32.
        let rad = |deg: f64| (((deg * 0.008726646f32 as f64) as f32) as f64).sin() as f32;
        set.directional.push(Light {
            name,
            kind: LightKind::Directional,
            position: [0.0; 3],
            // FRotator::Vector: the way the light travels.
            direction: [p.cos() * y.cos(), p.cos() * y.sin(), p.sin()].map(|v| v as f32),
            tangent: [0.0; 3],
            color: color.map(|c| (srgb_to_linear(c) * intensity) as f32),
            inv_radius: 0.0,
            falloff_exponent: 0.0,
            spot_angles: [0.0; 2],
            source_radius: rad(l["sourceAngle"].as_f64().unwrap_or(0.0)),
            soft_source_radius: rad(l["softSourceAngle"].as_f64().unwrap_or(0.0)),
            source_length: 0.0,
            specular_scale: l["specularScale"].as_f64().unwrap_or(1.0) as f32,
            channels: channels(l),
            shadow_channel: -1,
            cast_shadows: false,
            translucent: l["translucent"].as_bool().unwrap_or(false),
        });
    }
    for l in st["radialLights"].as_array().ok_or_else(|| bad(&path, "no radialLights"))? {
        let name = l["name"].as_str().unwrap_or("?").to_string();
        let towards = nums::<3>(&l["dir"], &path, "dir")?;
        let src = nums::<3>(&l["src"], &path, "src")?;
        let att = nums::<3>(&l["att"], &path, "att")?;
        let tangent = if l["tangent"].is_array() { nums::<3>(&l["tangent"], &path, "tangent")? } else { towards };
        set.radial.push(Light {
            name,
            kind: if att[1] == -2.0 { LightKind::Point } else { LightKind::Spot },
            position: nums::<3>(&l["pos"], &path, "pos")?,
            direction: towards.map(|v| -v as f32),
            tangent: tangent.map(|v| v as f32),
            color: nums::<3>(&l["color"], &path, "color")?.map(|v| v as f32),
            inv_radius: att[0] as f32,
            falloff_exponent: l["falloffExponent"].as_f64().unwrap_or(0.0) as f32,
            spot_angles: [att[1] as f32, att[2] as f32],
            source_radius: src[0] as f32,
            soft_source_radius: src[1] as f32,
            source_length: l["sourceLength"].as_f64().unwrap_or(0.0) as f32,
            specular_scale: src[2] as f32,
            channels: channels(l),
            shadow_channel: l["shadowChannel"].as_i64().unwrap_or(-1).clamp(-1, 3) as i8,
            cast_shadows: false,
            translucent: l["translucent"].as_bool().unwrap_or(false),
        });
    }
    set.sky = Some(sky(gpu, &dir.join("sky"))?);
    Ok(set)
}

/// The processed sky: `sky.json`, the filtered cube (RGBA16F, every mip, mip by mip and face by face) and the
/// PreIntegratedGF table (RG16).
fn sky(gpu: &Gpu, dir: &Path) -> Result<SkyLight> {
    let p = dir.join("sky.json");
    let s = json(&p)?;
    let size = s["size"].as_u64().ok_or_else(|| bad(&p, "size"))? as u32;
    let mips = s["mips"].as_u64().ok_or_else(|| bad(&p, "mips"))? as u32;
    let cube_path = dir.join("cube.bin");
    let cube = read(&cube_path)?;
    let mut at = 0;
    let mut by_mip = Vec::new();
    for m in 0..mips {
        let n = (size >> m).max(1);
        let len = (n * n * 8) as usize;
        let mut faces = Vec::new();
        for _ in 0..6 {
            let data = cube.get(at..at + len).ok_or_else(|| bad(&cube_path, "short"))?;
            faces.push(Subresource { data, row_pitch: n * 8, slice_pitch: n * n * 8 });
            at += len;
        }
        by_mip.push(faces);
    }
    if at != cube.len() {
        return Err(bad(&cube_path, format!("{} bytes, {at} for {size}^2 x {mips} mips", cube.len())));
    }
    // D3D's order: the mips of face 0, then face 1, ...
    let subs: Vec<Subresource> = (0..6).flat_map(|f| by_mip.iter().map(move |m| m[f])).collect();
    let cube = gpu.texture(
        &TexDesc {
            kind: TexKind::Cube,
            size: [size, size, 6],
            mips,
            format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            views: TexViews { srv: true, ..Default::default() },
        },
        &subs,
    )?;
    let [gw, gh] = nums::<2>(&s["gfSize"], &p, "gfSize")?.map(|v| v as u32);
    let gf = read(&dir.join("gf.bin"))?;
    let gf = texture2d(gpu, [gw, gh], DXGI_FORMAT_R16G16_UNORM, 4, &gf)?;
    let mut irradiance = [[0.0; 4]; 7];
    for (k, row) in s["skyIrradiance"].as_array().ok_or_else(|| bad(&p, "skyIrradiance"))?.iter().enumerate() {
        *irradiance.get_mut(k).ok_or_else(|| bad(&p, "skyIrradiance rows"))? =
            nums::<4>(row, &p, "skyIrradiance")?.map(|v| v as f32);
    }
    // The capture's rows hold the light's colour and intensity; the build's View keeps them in SkyLightColor.
    let color = nums::<3>(&s["color"], &p, "color")?.map(|v| v as f32);
    Ok(SkyLight {
        color,
        cube: Arc::new(cube),
        brightness: s["averageBrightness"].as_f64().ok_or_else(|| bad(&p, "averageBrightness"))? as f32,
        irradiance: crate::stage::sky_rows_unscaled(irradiance, color),
        pre_integrated_gf: Arc::new(gf),
        captures: Vec::new(),
    })
}

/// A 2D texture of one mip with its shader view, made here: Gpu::texture has no layout for R16G16_UNORM and
/// R8_UINT.
fn texture2d(gpu: &Gpu, [w, h]: [u32; 2], format: DXGI_FORMAT, bpp: u32, bytes: &[u8]) -> Result<Texture> {
    if bytes.len() != (w * h * bpp) as usize {
        return Err(Error::Format {
            what: "texture".into(),
            why: format!("{} bytes for {w} x {h} x {bpp}", bytes.len()),
        });
    }
    let td = D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let init = D3D11_SUBRESOURCE_DATA { pSysMem: bytes.as_ptr() as _, SysMemPitch: w * bpp, SysMemSlicePitch: 0 };
    let (mut t, mut v) = (None, None);
    // SAFETY: `init` points at `bytes`, checked above to hold the w x h texels.
    unsafe {
        gpu.dev.CreateTexture2D(&td, Some(&init), Some(&mut t)).map_err(dx("CreateTexture2D"))?;
        let t = t.as_ref().expect("CreateTexture2D");
        gpu.dev.CreateShaderResourceView(t, None, Some(&mut v)).map_err(dx("CreateShaderResourceView"))?;
    }
    Ok(Texture {
        res: t.expect("CreateTexture2D").cast().map_err(dx("ID3D11Resource"))?,
        srv: v,
        rtv: None,
        dsv: None,
        uav: None,
        desc: TexDesc {
            kind: TexKind::D2,
            size: [w, h, 1],
            mips: 1,
            format,
            views: TexViews { srv: true, ..Default::default() },
        },
    })
}

/// The device depth and the stencil's channel bits of a dump written into the depth-stencil target: SV_Depth from
/// the depth, drawn once per stencil value (the capture's channel mask) over that value's pixels with the reference set
/// to its base-pass bits (channel_stencil).
const DEPTH_FILL: &str = r"
Texture2D<float> Depth : register(t0);
Texture2D<uint> Stencil : register(t1);
cbuffer C : register(b0) { uint Value; };
float4 MainVS(uint id : SV_VertexID) : SV_Position {
  float2 p = float2((id << 1) & 2, id & 2);
  return float4(p * float2(2, -2) + float2(-1, 1), 0, 1);
}
float MainPS(float4 p : SV_Position) : SV_Depth {
  int3 q = int3(p.xy, 0);
  if (Stencil.Load(q) != Value) discard;
  return Depth.Load(q);
}";

/// Writes a captured frame's base pass targets (`dir`: targets.json and the dumps) into `targets`, whose size must
/// be theirs. Returns a copy of the captured SceneColor, to put back before each lighting of the same frame.
pub fn load_targets(gpu: &Gpu, dir: &Path, targets: &SceneTargets) -> Result<Texture> {
    let tj = dir.join("targets.json");
    let size = nums::<2>(&json(&tj)?["size"], &tj, "size")?.map(|v| v as u32);
    if size != targets.size {
        return Err(bad(&tj, format!("size {size:?}, the renderer's {:?}", targets.size)));
    }
    let [w, h] = size;
    let load = |name: &str, bpp: u32| -> Result<Vec<u8>> {
        let p = dir.join(format!("{name}.bin"));
        let b = read(&p)?;
        if b.len() != (w * h * bpp) as usize {
            return Err(bad(&p, format!("{} bytes for {w} x {h} x {bpp}", b.len())));
        }
        Ok(b)
    };
    let upload = |t: &Texture, bytes: &[u8], bpp: u32| {
        // SAFETY: a default-usage 2D texture's subresource 0, `bytes` holding its rows at `bpp` bytes a texel.
        unsafe { gpu.ctx.UpdateSubresource(&t.res, 0, None, bytes.as_ptr() as _, w * bpp, w * h * bpp) };
    };
    let scene = load("scenecolor", 8)?;
    upload(&targets.scene_color, &scene, 8);
    for (k, t) in targets.gbuffer.iter().enumerate() {
        let mut bytes = load(&format!("gbuffer{}", (b'a' + k as u8) as char), 4)?;
        if t.desc.format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB {
            // The capture's GBufferC is linear UNORM: stored as the build's base pass would store that linear value.
            for px in bytes.as_chunks_mut::<4>().0 {
                for c in &mut px[..3] {
                    *c = linear_to_srgb_byte(*c);
                }
            }
        }
        upload(t, &bytes, 4);
    }
    let base = gpu.texture(
        &TexDesc {
            kind: TexKind::D2,
            size: [w, h, 1],
            mips: 1,
            format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            views: TexViews::default(),
        },
        &[Subresource { data: &scene, row_pitch: w * 8, slice_pitch: w * h * 8 }],
    )?;

    let plain = |format: DXGI_FORMAT, bytes: &[u8], bpp: u32| {
        gpu.texture(
            &TexDesc {
                kind: TexKind::D2,
                size: [w, h, 1],
                mips: 1,
                format,
                views: TexViews { srv: true, ..Default::default() },
            },
            &[Subresource { data: bytes, row_pitch: w * bpp, slice_pitch: w * h * bpp }],
        )
    };
    let depth = plain(DXGI_FORMAT_R32_FLOAT, &load("depth", 4)?, 4)?;
    let stencil = texture2d(gpu, size, DXGI_FORMAT_R8_UINT, 1, &load("stencil", 1)?)?;
    let vs_code = compile(DEPTH_FILL, "MainVS", "vs_5_0")?;
    let ps_code = compile(DEPTH_FILL, "MainPS", "ps_5_0")?;
    let (mut vs, mut ps) = (None, None);
    // SAFETY: DXBC just compiled.
    unsafe {
        gpu.dev.CreateVertexShader(&vs_code, None, Some(&mut vs)).map_err(dx("CreateVertexShader depth fill"))?;
        gpu.dev.CreatePixelShader(&ps_code, None, Some(&mut ps)).map_err(dx("CreatePixelShader depth fill"))?;
    }
    let value = gpu.buffer(&BufDesc { kind: BufKind::Constant, size: 16, dynamic: true }, None)?;
    let replace = StencilFace {
        func: D3D11_COMPARISON_ALWAYS.0 as u8,
        fail: D3D11_STENCIL_OP_KEEP.0 as u8,
        depth_fail: D3D11_STENCIL_OP_KEEP.0 as u8,
        pass: D3D11_STENCIL_OP_REPLACE.0 as u8,
    };
    let ds = gpu.state(StateKey::Depth(DepthKey {
        depth_write: true,
        depth_func: D3D11_COMPARISON_ALWAYS.0 as u8,
        stencil: Some(StencilKey { front: replace, back: replace, read_mask: 0xff, write_mask: 0xff }),
    }));
    let raster = gpu.state(StateKey::Raster(RasterKey {
        fill: D3D11_FILL_SOLID.0 as u8,
        cull: D3D11_CULL_NONE.0 as u8,
        front_ccw: false,
        depth_bias: 0,
        slope_scaled_depth_bias: 0,
    }));
    let blend = gpu.state(StateKey::Blend(BlendKey::OPAQUE));
    gpu.viewport([0, 0, w, h]);
    gpu.set_geometry_shader(None);
    // SAFETY: this device's views and shaders; all unbound at the end.
    unsafe {
        let ctx = &gpu.ctx;
        let dsv = targets.depth.dsv.as_ref().expect("a depth view");
        ctx.ClearDepthStencilView(dsv, D3D11_CLEAR_DEPTH.0 | D3D11_CLEAR_STENCIL.0, 0.0, 0);
        ctx.OMSetRenderTargets(None, dsv);
        ctx.IASetInputLayout(None);
        ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        ctx.VSSetShader(vs.as_ref(), None);
        ctx.PSSetShader(ps.as_ref(), None);
        ctx.PSSetShaderResources(0, Some(&[depth.srv.clone(), stencil.srv.clone()]));
        ctx.PSSetConstantBuffers(0, Some(&[Some(value.buf.clone())]));
    }
    for v in 0..8u32 {
        gpu.write_rows(&value, &[[v, 0, 0, 0]]);
        gpu.set_states(blend, raster, ds, super::channel_stencil(v as u8) as u32);
        // SAFETY: the pipeline set above.
        unsafe { gpu.ctx.Draw(3, 0) };
    }
    // SAFETY: unbinds the views set above.
    unsafe {
        gpu.ctx.PSSetShaderResources(0, Some(&[None, None]));
        gpu.ctx.OMSetRenderTargets(None, None);
    }
    Ok(base)
}

/// Puts the captured SceneColor back (`base`, from load_targets).
pub fn restore_scene_color(gpu: &Gpu, targets: &SceneTargets, base: &Texture) {
    // SAFETY: two textures of one size and format on this device.
    unsafe { gpu.ctx.CopyResource(&targets.scene_color.res, &base.res) };
}

/// Bit v set for each stencil value v of the dump's stencil (`dir`/stencil.bin).
pub fn stencil_values(dir: &Path) -> Result<u8> {
    Ok(read(&dir.join("stencil.bin"))?.iter().fold(0u8, |m, &v| m | 1u8 << (v & 7)))
}
