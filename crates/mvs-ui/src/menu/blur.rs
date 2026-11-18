//! UBackgroundBlur's pass:
//! the kernel and the samples SBackgroundBlur::OnPaint / FSlatePostProcessor::BlurRect take from a strength, and the
//! pass itself on Direct3D 11, which the element batches run where a `BlurEl` stands in the order.
//!
//! The pass reads what was drawn before the widget (the 3D scene, the widgets painted earlier): the rectangle of
//! the target it covers, taken to 1 / downsample of its size with its edge pixels repeated around it, the table's
//! samples across and down, and the result laid over the rectangle in place of what was there, inside the widget's
//! clip.
//! Unverified: the build's downsample and upsample shaders are cooked and not read; the downsample here is one
//! bilinear sample of each block (a 2 x 2 average at half size) and the upsample bilinear. The blur's samples are the
//! table's (the build's 14255d7e0, 142571350), the weights not normalised.
//! Deviation: the kernel is not a canvas's Gaussian filter at the same sigma; this draws
//! the table's paired bilinear samples as the build does.
//!
//! What is held: two scratch textures of the largest picture a pass needed, made smaller
//! after SHRINK_FRAMES frames that needed under half, and the copy of the target; all are given back after a frame
//! with no blur.

use std::ffi::c_void;

use mvs_render::gpu::{
    BufDesc, BufKind, Buffer, Gpu, SamplerKey, StateId, StateKey, TexDesc, TexKind, TexViews, Texture,
};
use windows::Win32::Graphics::Direct3D::D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCOMPILE_OPTIMIZATION_LEVEL3, D3DCompile};
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;
use windows::core::PCSTR;

use crate::element::BlurEl;
use crate::{Error, Result};

/// The pass's numbers for a strength: the kernel (odd, a count for one side with the centre), the downsample (0: the
/// rectangle's own size) and the Gaussian's sigma in pixels of the pass's picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlurPlan {
    pub kernel: i32,
    pub downsample: i32,
    pub sigma: f64,
}

/// The kernel and the strength SBackgroundBlur::OnPaint passes on (1417f49e0): the kernel is round(3 x strength);
/// above 9 the pass runs on a target half the size (a quarter from 64), with the kernel (integer division) and the
/// strength divided by the same; the kernel is made odd, at least 3 and at most Slate.BackgroundBlurMaxKernelSize
/// (255); the strength (the Gaussian's sigma) is at least 0.5 before the division.
/// 7.5: 23 -> half size, 11, sigma 3.75. 7: 21 -> half, 11 (10 made odd), 3.5. 15: 45 -> half, 23 (22 made odd),
/// 7.5. 15.619048: 47 -> half, 23, 7.81. 35.238094: 106 -> quarter, 27 (26 made odd), 8.81.
pub fn blur_kernel(strength: f64) -> BlurPlan {
    let mut kernel = (strength * 3.0 + 0.5).floor() as i32;
    let mut down = 0;
    if kernel > 9 {
        down = if kernel >= 64 { 4 } else { 2 };
        kernel /= down;
    }
    if kernel % 2 == 0 {
        kernel += 1;
    }
    BlurPlan { kernel: kernel.clamp(3, 255), downsample: down, sigma: strength.max(0.5) / f64::from(down.max(1)) }
}

/// The farthest pixel a pass of that kernel reads on one side: the pairs at 1 and at X, X + 2 (X = 3, 7, ..
/// under the kernel) end at 2, 6, 10, ..
pub fn blur_reach(kernel: i32) -> i32 {
    2 + 4 * (((kernel - 3).max(0) + 3) / 4)
}

/// The samples of one pass (across, then down) of a kernel of `blur_kernel`: (offset in pixels of the pass's
/// target, weight) for the centre and one side, the other side mirrored. FSlatePostProcessor::BlurRect's table
/// (14255d7e0, 142571350): w(d) = exp(-d^2 / 2 sigma^2) / sqrt(2 pi sigma^2), not normalised; the centre w(0), then two
/// neighbours d, d + 1 as one bilinear sample at their weighted mean with their weights' sum, for d = 1 and for
/// d = X, X + 2 with X = 3, 7, 11, .. under the kernel. So the kernel counts one side with the centre.
// Unverified: the pixel shader is cooked and not read; that it mirrors the table and uses every row is the 4.26
// shader's (SampleCount = (kernel + 1) / 2 covers the rows made).
pub fn blur_taps(kernel: i32, sigma: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    blur_taps_into(kernel, sigma, &mut out);
    out
}

/// `blur_taps` written into `out` (cleared first).
pub fn blur_taps_into(kernel: i32, sigma: f64, out: &mut Vec<[f64; 2]>) {
    let k = -1.0 / (2.0 * sigma * sigma);
    let norm = 1.0 / (2.0 * std::f64::consts::PI * sigma * sigma).sqrt();
    out.clear();
    out.push([0.0, norm]);
    let reach = blur_reach(kernel);
    let mut d = 1;
    while d < reach {
        let (d1, d2) = (f64::from(d), f64::from(d + 1));
        let w1 = (d1 * d1 * k).exp() * norm;
        let w2 = (d2 * d2 * k).exp() * norm;
        out.push([if w1 + w2 > 0.0 { (d1 * w1 + d2 * w2) / (w1 + w2) } else { 0.0 }, w1 + w2]);
        d += 2;
    }
}

/// The pass's taps the shader's constant buffer holds.
const MAX_TAPS: usize = 130;
/// The constant buffer's rows: five parameter rows, then the taps.
const ROWS: usize = 5 + MAX_TAPS;
/// The frames in a row that needed under half of the scratch textures before they are made that size.
const SHRINK_FRAMES: u32 = 30;

fn dx(what: &'static str) -> impl FnOnce(windows::core::Error) -> Error {
    move |source| Error::Render(mvs_render::Error::Device { what: what.into(), source })
}

/// HLSL of the port's own compiled by d3dcompiler_47.
fn compile(entry: &str, target: &str) -> Result<Vec<u8>> {
    const SOURCE: &str = include_str!("blur.hlsl");
    let e = format!("{entry}\0");
    let t = format!("{target}\0");
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the source pointer and length are the &str's; the entry and target are NUL-terminated; the blobs are
    // read within their reported sizes.
    unsafe {
        let r = D3DCompile(
            SOURCE.as_ptr().cast::<c_void>(),
            SOURCE.len(),
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
        let blob = |b: &ID3DBlob| std::slice::from_raw_parts(b.GetBufferPointer().cast::<u8>(), b.GetBufferSize());
        if let Err(source) = r {
            let msg = errors.as_ref().map(|b| String::from_utf8_lossy(blob(b)).into_owned()).unwrap_or_default();
            return Err(Error::Format { what: format!("D3DCompile {entry}"), why: format!("{source}: {msg}") });
        }
        Ok(blob(code.as_ref().expect("D3DCompile gave no code")).to_vec())
    }
}

fn scratch(gpu: &Gpu, size: [u32; 2], format: DXGI_FORMAT, rtv: bool) -> Result<Texture> {
    Ok(gpu.texture(
        &TexDesc {
            kind: TexKind::D2,
            size: [size[0], size[1], 1],
            mips: 1,
            format,
            views: TexViews { srv: true, rtv, ..TexViews::default() },
        },
        &[],
    )?)
}

/// The pass's shaders, states and scratch textures.
#[derive(Debug)]
pub struct BlurPass {
    vs: ID3D11VertexShader,
    down: ID3D11PixelShader,
    blur: ID3D11PixelShader,
    up: ID3D11PixelShader,
    constants: Buffer,
    opaque: ID3D11BlendState,
    sampler: StateId,
    /// The target as it was when the pass began (a render target is not read while it is written).
    backdrop: Option<Texture>,
    /// The pass's picture, and its first and second sample pass's results: grown within a frame, made smaller by
    /// `end_frame`.
    a: Option<Texture>,
    b: Option<Texture>,
    /// This frame: the passes run and the largest picture they needed.
    count: u32,
    need: [u32; 2],
    /// The frames in a row that needed under half of `a`.
    small: u32,
    /// The taps of a pass and the constant rows of a draw (kept: no allocation per pass).
    taps: Vec<[f64; 2]>,
    rows: Box<[[u32; 4]; ROWS]>,
}

impl BlurPass {
    pub fn new(gpu: &Gpu) -> Result<BlurPass> {
        let vs_code = compile("MainVS", "vs_5_0")?;
        let (mut vs, mut down, mut blur, mut up) = (None, None, None, None);
        let ps = |entry| compile(entry, "ps_5_0");
        let (down_code, blur_code, up_code) = (ps("DownPS")?, ps("BlurPS")?, ps("UpPS")?);
        // SAFETY: DXBC just compiled for this device's shader model.
        unsafe {
            gpu.dev.CreateVertexShader(&vs_code, None, Some(&mut vs)).map_err(dx("CreateVertexShader blur"))?;
            gpu.dev.CreatePixelShader(&down_code, None, Some(&mut down)).map_err(dx("CreatePixelShader blur down"))?;
            gpu.dev.CreatePixelShader(&blur_code, None, Some(&mut blur)).map_err(dx("CreatePixelShader blur"))?;
            gpu.dev.CreatePixelShader(&up_code, None, Some(&mut up)).map_err(dx("CreatePixelShader blur up"))?;
        }
        // the pass writes the picture as it is: no blending
        let mut desc = D3D11_BLEND_DESC::default();
        desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
        let mut opaque = None;
        // SAFETY: a plain description on this device.
        unsafe { gpu.dev.CreateBlendState(&desc, Some(&mut opaque)).map_err(dx("CreateBlendState blur"))? };
        Ok(BlurPass {
            vs: vs.expect("CreateVertexShader"),
            down: down.expect("CreatePixelShader"),
            blur: blur.expect("CreatePixelShader"),
            up: up.expect("CreatePixelShader"),
            constants: gpu
                .buffer(&BufDesc { kind: BufKind::Constant, size: (ROWS * 16) as u32, dynamic: true }, None)?,
            opaque: opaque.expect("CreateBlendState"),
            sampler: gpu.state(StateKey::Sampler(SamplerKey::of(
                D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT,
                D3D11_TEXTURE_ADDRESS_CLAMP,
            ))),
            backdrop: None,
            a: None,
            b: None,
            count: 0,
            need: [0, 0],
            small: 0,
            taps: Vec::new(),
            rows: Box::new([[0; 4]; ROWS]),
        })
    }

    /// A frame of draws is over: with no pass the textures go back; after SHRINK_FRAMES
    /// frames in a row that needed under half of them they are made the size needed.
    pub fn end_frame(&mut self, gpu: &Gpu) -> Result<()> {
        let (count, need) = (std::mem::take(&mut self.count), std::mem::take(&mut self.need));
        if count == 0 {
            self.a = None;
            self.b = None;
            self.backdrop = None;
            self.small = 0;
            return Ok(());
        }
        let Some(a) = &self.a else { return Ok(()) };
        let area = |s: [u32; 2]| u64::from(s[0]) * u64::from(s[1]);
        if area(need) * 2 <= area([a.desc.size[0], a.desc.size[1]]) {
            self.small += 1;
            if self.small >= SHRINK_FRAMES {
                let format = a.desc.format;
                self.a = Some(scratch(gpu, need, format, true)?);
                self.b = Some(scratch(gpu, need, format, true)?);
                self.small = 0;
            }
        } else {
            self.small = 0;
        }
        Ok(())
    }

    /// The constant rows of a draw: the five parameter rows, then `taps` of the frame's taps (the rest zero).
    fn set_rows(&mut self, params: &[[f32; 4]; 5], taps: usize) {
        self.rows.fill([0; 4]);
        for (d, s) in self.rows.iter_mut().zip(params) {
            *d = s.map(f32::to_bits);
        }
        for (d, t) in self.rows[5..].iter_mut().zip(self.taps.iter().take(taps)) {
            *d = [t[0] as f32, t[1] as f32, 0.0, 0.0].map(f32::to_bits);
        }
    }

    /// One fullscreen triangle of `ps` into `rtv` over the viewport `vp` and scissor `rect`, reading `src`, with
    /// the constant rows `set_rows` wrote.
    #[allow(clippy::too_many_arguments)]
    fn draw(
        &self,
        gpu: &Gpu,
        raster: &ID3D11RasterizerState,
        ps: &ID3D11PixelShader,
        rtv: &ID3D11RenderTargetView,
        src: &ID3D11ShaderResourceView,
        vp: [u32; 2],
        scissor: [i32; 4],
    ) {
        gpu.write_rows(&self.constants, &self.rows[..]);
        let rect = windows::Win32::Foundation::RECT {
            left: scissor[0],
            top: scissor[1],
            right: scissor[2],
            bottom: scissor[3],
        };
        gpu.viewport([0, 0, vp[0], vp[1]]);
        // SAFETY: states, views and shaders made on this device, alive through the calls; the draw has no vertex
        // buffer (the vertex shader makes its triangle from the vertex id).
        unsafe {
            gpu.ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            gpu.ctx.IASetInputLayout(None);
            gpu.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            gpu.ctx.VSSetShader(&self.vs, None);
            gpu.ctx.PSSetShader(ps, None);
            gpu.ctx.RSSetState(raster);
            gpu.ctx.RSSetScissorRects(Some(&[rect]));
            gpu.ctx.OMSetBlendState(&self.opaque, None, 0xffff_ffff);
            gpu.ctx.VSSetConstantBuffers(0, Some(&[Some(self.constants.buf.clone())]));
            gpu.ctx.PSSetConstantBuffers(0, Some(&[Some(self.constants.buf.clone())]));
            gpu.ctx.PSSetShaderResources(0, Some(&[Some(src.clone())]));
            gpu.ctx.PSSetSamplers(0, Some(&[Some(gpu.sampler(self.sampler))]));
            gpu.ctx.Draw(3, 0);
            gpu.ctx.PSSetShaderResources(0, Some(&[None]));
        }
    }

    /// The blur of `e` over `target`, inside `clip` (pixels) when there is one; what the draw had set (the render
    /// target, the viewport) is put back.
    pub fn run(
        &mut self,
        gpu: &Gpu,
        raster: &ID3D11RasterizerState,
        target: &Texture,
        e: &BlurEl,
        clip: Option<[f64; 4]>,
    ) -> Result<()> {
        let [tw, th] = [target.desc.size[0], target.desc.size[1]];
        let m = &e.geometry.transform;
        let [w, h] = e.geometry.size;
        // the rectangle: the pixels whose centres the widget's geometry holds (its bounding rectangle when turned)
        let (ax, ay, bx, by) = (m[0] * w, m[1] * w, m[2] * h, m[3] * h);
        let lo = |a: f64, b: f64| 0f64.min(a).min(b).min(a + b);
        let hi = |a: f64, b: f64| 0f64.max(a).max(b).max(a + b);
        let px = |v: f64, max: u32| (v - 0.5).ceil().clamp(0.0, f64::from(max)) as i32;
        let (x0, x1) = (px(m[4] + lo(ax, bx), tw), px(m[4] + hi(ax, bx), tw));
        let (y0, y1) = (px(m[5] + lo(ay, by), th), px(m[5] + hi(ay, by), th));
        let (bw, bh) = (x1 - x0, y1 - y0);
        if bw <= 0 || bh <= 0 {
            return Ok(());
        }
        // inside the clip
        let (mut cx0, mut cy0, mut cx1, mut cy1) = (x0, y0, x1, y1);
        if let Some(c) = clip {
            cx0 = cx0.max(c[0].round() as i32);
            cy0 = cy0.max(c[1].round() as i32);
            cx1 = cx1.min(c[2].round() as i32);
            cy1 = cy1.min(c[3].round() as i32);
        }
        if cx1 <= cx0 || cy1 <= cy0 {
            return Ok(());
        }
        let plan = blur_kernel(e.strength);
        let f = plan.downsample.max(1);
        // (the second pixel of the outermost bilinear sample)
        let pad = blur_reach(plan.kernel);
        // the picture at the pass's size: whole pixels of it, each f x f of the rectangle
        let (fw, fh) = ((bw / f).max(1), (bh / f).max(1));
        let (sw, sh) = (bw.min(fw * f), bh.min(fh * f));
        let (pw, ph) = (fw + 2 * pad, fh + 2 * pad);
        let format = target.desc.format;
        let grown = |t: &Option<Texture>, [w, h]: [u32; 2]| {
            t.as_ref().is_none_or(|t| t.desc.size[0] < w || t.desc.size[1] < h || t.desc.format != format)
        };
        let need = [pw as u32, ph as u32];
        self.count += 1;
        self.need = [self.need[0].max(need[0]), self.need[1].max(need[1])];
        if grown(&self.a, need) {
            let s = self.a.as_ref().map_or(need, |t| [t.desc.size[0].max(need[0]), t.desc.size[1].max(need[1])]);
            self.a = Some(scratch(gpu, s, format, true)?);
            self.b = Some(scratch(gpu, s, format, true)?);
        }
        if self.backdrop.as_ref().is_none_or(|t| t.desc.size[..2] != target.desc.size[..2] || t.desc.format != format) {
            self.backdrop = Some(scratch(gpu, [tw, th], format, false)?);
        }
        let (a, b, back) = (
            self.a.as_ref().expect("made above"),
            self.b.as_ref().expect("made above"),
            self.backdrop.as_ref().expect("made above"),
        );
        let ts = [a.desc.size[0] as f32, a.desc.size[1] as f32];
        // what is on the target now: the rectangle of it
        let src_box =
            D3D11_BOX { left: x0 as u32, top: y0 as u32, front: 0, right: x1 as u32, bottom: y1 as u32, back: 1 };
        // SAFETY: both resources are textures of this device with the same format; the box lies inside the target.
        unsafe { gpu.ctx.CopySubresourceRegion(&back.res, 0, x0 as u32, y0 as u32, 0, &target.res, 0, Some(&src_box)) };
        let params = |taps: usize, dir: [f32; 2]| -> [[f32; 4]; 5] {
            [
                [x0 as f32, y0 as f32, sw as f32, sh as f32],
                [fw as f32, fh as f32, pad as f32, taps as f32],
                [tw as f32, th as f32, ts[0], ts[1]],
                [dir[0], dir[1], 0.0, 0.0],
                [pw as f32, ph as f32, bw as f32, bh as f32],
            ]
        };
        // (the views' references counted again: the rows below are written between the draws)
        let (a_rtv, b_rtv) = (a.rtv.clone().expect("a render target"), b.rtv.clone().expect("a render target"));
        let (a_srv, b_srv) = (a.srv.clone().expect("a view"), b.srv.clone().expect("a view"));
        let (back_srv, t_rtv) = (back.srv.clone().expect("a view"), target.rtv.as_ref().expect("a render target"));
        let whole = [0, 0, pw, ph];
        let pass = [pw as u32, ph as u32];
        // 1. what is behind the widget, then the edge pixels around it
        self.set_rows(&params(0, [0.0; 2]), 0);
        self.draw(gpu, raster, &self.down, &a_rtv, &back_srv, pass, whole);
        // 2. the table across, then down
        blur_taps_into(plan.kernel, plan.sigma, &mut self.taps);
        let n = self.taps.len().min(MAX_TAPS);
        self.set_rows(&params(n, [1.0 / ts[0], 0.0]), n);
        self.draw(gpu, raster, &self.blur, &b_rtv, &a_srv, pass, whole);
        self.set_rows(&params(n, [0.0, 1.0 / ts[1]]), n);
        self.draw(gpu, raster, &self.blur, &a_rtv, &b_srv, pass, whole);
        // 3. in place of the rectangle, inside the rectangle's bounding box and the paint's clip
        self.set_rows(&params(0, [0.0; 2]), 0);
        self.draw(gpu, raster, &self.up, t_rtv, &a_srv, [tw, th], [cx0, cy0, cx1, cy1]);
        Ok(())
    }
}
