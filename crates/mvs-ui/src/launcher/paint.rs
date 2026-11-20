//! The launcher's 2D painter: shapes in the order they are added, drawn by one shader of the port's own (compiled
//! with d3dcompiler_47) on any render target, in one call for each picture the shapes ask for in turn. A shape is a
//! rounded rectangle or an ellipse (fill, border, soft edge for glows and shadows, a linear or radial gradient), a
//! ring with a gap (the spinner), a stroked segment (the icons), a glyph of the text atlas or a picture; each with
//! its clip and its 2x2 transform about an origin (CSS skew and rotate). Colours are the page's sRGB values, blended
//! as a browser does: on the encoded values, premultiplied.

use std::ffi::c_void;

use mvs_render::gpu::{
    BlendKey, BlendTarget, BufDesc, BufKind, Buffer, DepthKey, Gpu, RasterKey, SamplerKey, StateId, StateKey,
    Subresource, TexDesc, TexKind, TexViews, Texture,
};
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCOMPILE_OPTIMIZATION_LEVEL3, D3DCompile};
use windows::Win32::Graphics::Direct3D::{D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, ID3DBlob};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM};
use windows::core::PCSTR;

use crate::{Error, Result};

/// An sRGB colour with straight alpha, 0..1.
pub type Rgba = [f32; 4];

/// `#rrggbb` with alpha `a`.
pub const fn hex(c: u32, a: f32) -> Rgba {
    [((c >> 16) & 0xff) as f32 / 255.0, ((c >> 8) & 0xff) as f32 / 255.0, (c & 0xff) as f32 / 255.0, a]
}

/// `rgba(r, g, b, a)`.
pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Rgba {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a]
}

pub const CLEAR: Rgba = [0.0; 4];

/// The gradient atlas: a row per gradient, its stops resolved over 256 texels.
const GRAD_W: u32 = 256;
const GRAD_ROWS: u32 = 128;
/// The text atlas's side.
pub const ATLAS: u32 = 2048;

/// What a shape draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A rounded rectangle (radius up to half the short side: an ellipse).
    Rect = 0,
    /// A coverage glyph of the text atlas (`uv` its texels).
    Glyph = 1,
    /// The picture of slot `picture` (`uv` 0..1), its corners rounded by `radius`, faded on its left and bottom
    /// by `fade`; `p`: whether its texels are sRGB, and the taps a side of its box filter (a picture drawn smaller
    /// than its texels; 0 or 1: one sample).
    Image = 2,
    /// A stroke from `p[0..2]` to `p[2..4]`, `border` wide, round caps.
    Segment = 3,
}

/// How the fill's colour is taken.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fill {
    Solid(Rgba),
    /// A gradient (`Painter::gradient`) along `from` to `to`; `repeat`: the gradient's length is `to - from` and it
    /// repeats, moved by `shift` of its length (CSS background-size and an animated background-position).
    Linear {
        grad: u32,
        from: [f32; 2],
        to: [f32; 2],
        repeat: bool,
        shift: f32,
    },
    /// A radial gradient: 0 at `centre`, 1 at the ellipse of `radii`.
    Radial {
        grad: u32,
        centre: [f32; 2],
        radii: [f32; 2],
    },
}

/// One shape, in target pixels.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub kind: Kind,
    /// x, y, width, height before the transform.
    pub rect: [f32; 4],
    pub fill: Fill,
    pub border: f32,
    pub border_color: Rgba,
    pub radius: f32,
    /// The edge's width: 1 a crisp antialiased edge, more a blurred one (CSS blur and box-shadow's blur).
    pub soft: f32,
    pub opacity: f32,
    pub uv: [f32; 4],
    /// x0, y0, x1, y1 in target pixels (after the transform).
    pub clip: [f32; 4],
    /// The 2x2 transform's rows and its origin.
    pub xform: [f32; 4],
    pub origin: [f32; 2],
    /// A segment's ends; a ring's gap (start angle, length, radians; clockwise from +x).
    pub p: [f32; 4],
    /// The picture's fade: its left side from u p0 (clear) to p1 (whole), its bottom from v p2 (whole) to p3 (clear).
    pub fade: [f32; 4],
    /// An Image's picture: its slot (`Painter::set_picture`).
    pub picture: u32,
}

impl Shape {
    pub fn new(kind: Kind, rect: [f32; 4], fill: Fill) -> Shape {
        Shape {
            kind,
            rect,
            fill,
            border: 0.0,
            border_color: CLEAR,
            radius: 0.0,
            soft: 1.0,
            opacity: 1.0,
            uv: [0.0; 4],
            clip: [-1e6, -1e6, 1e6, 1e6],
            xform: [1.0, 0.0, 0.0, 1.0],
            origin: [0.0; 2],
            p: [0.0; 4],
            fade: [0.0; 4],
            picture: 0,
        }
    }
}

/// The instance the shader reads: 12 rows of four floats (SOURCE's Shape).
#[repr(C)]
#[derive(Clone, Copy)]
struct Row([f32; 48]);

fn row_of(s: &Shape, atlas_scale: f32) -> Row {
    let (color, grad, mode, params, repeat, shift) = match s.fill {
        Fill::Solid(c) => (c, -1.0, 0.0, [0.0; 4], 0.0, 0.0),
        Fill::Linear { grad, from, to, repeat, shift } => {
            ([1.0; 4], grad as f32, 1.0, [from[0], from[1], to[0], to[1]], f32::from(u8::from(repeat)), shift)
        }
        Fill::Radial { grad, centre, radii } => {
            ([1.0; 4], grad as f32, 2.0, [centre[0], centre[1], radii[0], radii[1]], 0.0, 0.0)
        }
    };
    let uv = if s.kind == Kind::Glyph { s.uv.map(|x| x * atlas_scale) } else { s.uv };
    let mut r = [0.0f32; 48];
    let rows: [[f32; 4]; 12] = [
        s.rect,
        color,
        s.border_color,
        [s.radius, s.border, s.soft, s.kind as u32 as f32],
        [(grad + 0.5) / GRAD_ROWS as f32, mode, repeat, shift],
        params,
        uv,
        s.clip,
        s.xform,
        [s.origin[0], s.origin[1], s.opacity, 0.0],
        s.p,
        s.fade,
    ];
    for (k, x) in rows.iter().enumerate() {
        r[k * 4..k * 4 + 4].copy_from_slice(x);
    }
    Row(r)
}

const SOURCE: &str = r#"
struct Shape {
    float4 rect; float4 color; float4 border_color; float4 misc; float4 grad; float4 gparams;
    float4 uv; float4 clip; float4 xform; float4 origin; float4 p; float4 fade;
};
StructuredBuffer<Shape> shapes : register(t0);
Texture2D<float> atlas : register(t1);
Texture2D<float4> grads : register(t2);
Texture2D<float4> picture : register(t3);
SamplerState lin : register(s0);
// (xy the target's size; z the draw's first shape: SV_InstanceID counts from 0 in every draw)
cbuffer Frame : register(b0) { float4 target; };

struct V { float4 pos : SV_Position; float2 local : LOCAL; nointerpolation uint id : SHAPE; };

V MainVS(uint vid : SV_VertexID, uint iid : SV_InstanceID) {
    uint id = iid + (uint)target.z;
    Shape s = shapes[id];
    // (the quad grows by the soft edge and the border, and covers a segment's stroke)
    float pad = s.misc.z + 1.0;
    float4 r = s.rect;
    if (s.misc.w == 3) {
        float h = s.misc.y * 0.5 + pad;
        r = float4(min(s.p.x, s.p.z) - h, min(s.p.y, s.p.w) - h, abs(s.p.z - s.p.x) + 2 * h, abs(s.p.w - s.p.y) + 2 * h);
        pad = 0;
    }
    // (two triangles: 0 1 2 and 1 3 2 of the corners TL TR BL BR)
    const uint corner[6] = { 0, 1, 2, 1, 3, 2 };
    uint k = corner[vid];
    float2 c = float2(k & 1, k >> 1);
    float2 local = r.xy - pad + c * (r.zw + 2 * pad);
    float2 d = local - s.origin.xy;
    float2 at = s.origin.xy + float2(dot(s.xform.xy, d), dot(s.xform.zw, d));
    V o;
    o.pos = float4(at / target.xy * float2(2, -2) + float2(-1, 1), 0, 1);
    o.local = local;
    o.id = id;
    return o;
}

float rect_sdf(float2 p, float4 r, float rad) {
    float2 h = r.zw * 0.5;
    rad = min(rad, min(h.x, h.y));
    float2 q = abs(p - r.xy - h) - (h - rad);
    return length(max(q, 0)) + min(max(q.x, q.y), 0) - rad;
}

float4 fill_color(Shape s, float2 p) {
    if (s.grad.y == 0) return s.color;
    float t;
    if (s.grad.y == 1) {
        float2 a = s.gparams.xy, b = s.gparams.zw - s.gparams.xy;
        t = dot(p - a, b) / max(dot(b, b), 1e-6);
        if (s.grad.z > 0) t = frac(t + s.grad.w);
    } else {
        t = length((p - s.gparams.xy) / max(s.gparams.zw, 1e-3));
    }
    return grads.SampleLevel(lin, float2((saturate(t) * 255 + 0.5) / 256, s.grad.x), 0);
}

float4 MainPS(V v) : SV_Target {
    Shape s = shapes[v.id];
    if (v.pos.x < s.clip.x || v.pos.y < s.clip.y || v.pos.x >= s.clip.z || v.pos.y >= s.clip.w) discard;
    float2 p = v.local;
    float soft = max(s.misc.z, 1e-3);
    float4 c;
    if (s.misc.w == 1) {
        // (the quad's pad lies outside the glyph: its texels clamped to the glyph's own, its coverage cut at its box)
        float2 uv = s.uv.xy + (p - s.rect.xy) / max(s.rect.zw, 1e-3) * (s.uv.zw - s.uv.xy);
        float2 half_texel = 0.5 / 2048.0; // (ATLAS)
        uv = clamp(uv, s.uv.xy + half_texel, s.uv.zw - half_texel);
        float2 inside = min(p - s.rect.xy, s.rect.xy + s.rect.zw - p);
        float a = atlas.SampleLevel(lin, uv, 0) * saturate(0.5 + min(inside.x, inside.y));
        c = fill_color(s, p);
        // Deviation: coverage blended on encoded values thins light text on dark; it is lifted by the text's
        // lightness (as browsers' text contrast does), so a white stem's edge reads as solid as a dark one's.
        float lum = dot(c.rgb, float3(0.2126, 0.7152, 0.0722));
        c.a *= pow(a, lerp(1.0, 0.72, lum));
    } else if (s.misc.w == 2) {
        float2 span = s.uv.zw - s.uv.xy;
        float2 uv = s.uv.xy + (p - s.rect.xy) / s.rect.zw * span;
        int taps = (int)s.p.y;
        if (taps > 1) {
            // (a picture with no mips drawn smaller than its texels: the mean over the pixel's footprint)
            float2 step = span / s.rect.zw / taps;
            c = 0;
            [loop] for (int j = 0; j < taps; j++) {
                [loop] for (int i = 0; i < taps; i++) {
                    c += picture.SampleLevel(lin, uv + (float2(i, j) + 0.5 - taps * 0.5) * step, 0);
                }
            }
            c /= taps * taps;
        } else {
            c = picture.SampleLevel(lin, uv, 0);
        }
        // (an sRGB picture samples linear: encoded again, the page blends encoded values)
        if (s.p.x > 0) c.rgb = c.rgb <= 0.0031308 ? c.rgb * 12.92 : 1.055 * pow(abs(c.rgb), 1 / 2.4) - 0.055;
        float k = smoothstep(s.fade.x, s.fade.y, uv.x) * (1 - smoothstep(s.fade.z, s.fade.w, uv.y));
        c.a *= k * saturate(1 - rect_sdf(p, s.rect, s.misc.x));
    } else if (s.misc.w == 3) {
        float2 a = s.p.xy, b = s.p.zw - s.p.xy;
        float t = saturate(dot(p - a, b) / max(dot(b, b), 1e-6));
        float d = length(p - a - b * t) - s.misc.y * 0.5;
        c = fill_color(s, p);
        c.a *= saturate(0.5 - d / soft);
    } else {
        float d = rect_sdf(p, s.rect, s.misc.x);
        float cover = saturate(0.5 - d / soft);
        c = fill_color(s, p);
        if (s.misc.y > 0) {
            float inner = saturate(0.5 - (d + s.misc.y));
            float4 f = float4(c.rgb * c.a, c.a);
            float4 b = float4(s.border_color.rgb * s.border_color.a, s.border_color.a);
            // (the background runs under the border, CSS's border-box: the border is over it)
            float4 m = lerp(b + f * (1 - b.a), f, inner);
            c = float4(m.rgb / max(m.a, 1e-4), m.a);
        }
        if (s.p.w > 0) {
            float2 h = s.rect.xy + s.rect.zw * 0.5;
            float ang = atan2(p.y - h.y, p.x - h.x) - s.p.z;
            ang = ang - 6.2831853 * floor(ang / 6.2831853);
            if (ang < s.p.w) cover = 0;
        }
        c.a *= cover;
    }
    c.a *= s.origin.z;
    float4 o = float4(c.rgb * c.a, c.a);
    // A gradient or the picture is dithered by a step of the 8-bit target (interleaved gradient noise): the large dark
    // shades over the sky otherwise show as vertical bands.
    if (s.grad.y != 0 || s.misc.w == 2) {
        float n = frac(52.9829189 * frac(dot(v.pos.xy, float2(0.06711056, 0.00583715))));
        o.rgb = max(o.rgb + (n - 0.5) / 255.0 * o.a, 0);
    }
    return o;
}
"#;

/// HLSL of the launcher's own compiled by d3dcompiler_47.
fn compile(entry: &str, target: &str) -> Result<Vec<u8>> {
    let e = format!("{entry}\0");
    let t = format!("{target}\0");
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the source pointer and length are the &str's; the entry and target are NUL-terminated; the blobs are
    // read within their reported sizes.
    unsafe {
        let r = D3DCompile(
            SOURCE.as_ptr() as *const c_void,
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
        let blob = |b: &ID3DBlob| std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize());
        if let Err(source) = r {
            let msg = errors.as_ref().map(|b| String::from_utf8_lossy(blob(b)).into_owned()).unwrap_or_default();
            return Err(Error::Format { what: format!("launcher shader {entry}"), why: format!("{source}: {msg}") });
        }
        Ok(blob(code.as_ref().expect("D3DCompile gave no code")).to_vec())
    }
}

fn dx(what: &'static str) -> impl FnOnce(windows::core::Error) -> Error {
    move |e| Error::Format { what: what.into(), why: e.to_string() }
}

/// The painter and its frame's shapes.
pub struct Painter {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    raster: StateId,
    depth: StateId,
    blend: StateId,
    sampler: StateId,
    frame: Buffer,
    shapes: Option<Buffer>,
    grads: Texture,
    grad_rows: Vec<Vec<(f32, Rgba)>>,
    grads_dirty: bool,
    atlas: Texture,
    /// The pictures the Image shapes draw, by slot.
    pictures: Vec<Option<std::sync::Arc<Texture>>>,
    list: Vec<Shape>,
    /// The clip every new shape takes.
    pub clip: [f32; 4],
}

impl std::fmt::Debug for Painter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Painter").field("shapes", &self.list.len()).finish()
    }
}

impl Painter {
    pub fn new(gpu: &Gpu) -> Result<Painter> {
        let (vs_code, ps_code) = (compile("MainVS", "vs_5_0")?, compile("MainPS", "ps_5_0")?);
        let (mut vs, mut ps) = (None, None);
        // SAFETY: DXBC just compiled for this device's shader model.
        unsafe {
            gpu.dev.CreateVertexShader(&vs_code, None, Some(&mut vs)).map_err(dx("CreateVertexShader launcher"))?;
            gpu.dev.CreatePixelShader(&ps_code, None, Some(&mut ps)).map_err(dx("CreatePixelShader launcher"))?;
        }
        // Premultiplied over.
        let over = BlendTarget {
            color_op: D3D11_BLEND_OP_ADD.0 as u8,
            color_src: D3D11_BLEND_ONE.0 as u8,
            color_dst: D3D11_BLEND_INV_SRC_ALPHA.0 as u8,
            alpha_op: D3D11_BLEND_OP_ADD.0 as u8,
            alpha_src: D3D11_BLEND_ONE.0 as u8,
            alpha_dst: D3D11_BLEND_INV_SRC_ALPHA.0 as u8,
            write_mask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        let mut blend = BlendKey::OPAQUE;
        blend.targets[0] = over;
        let tex = |size: [u32; 2], format| {
            gpu.texture(
                &TexDesc {
                    kind: TexKind::D2,
                    size: [size[0], size[1], 1],
                    mips: 1,
                    format,
                    views: TexViews { srv: true, ..Default::default() },
                },
                &[],
            )
        };
        Ok(Painter {
            vs: vs.expect("CreateVertexShader"),
            ps: ps.expect("CreatePixelShader"),
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
            blend: gpu.state(StateKey::Blend(blend)),
            sampler: gpu
                .state(StateKey::Sampler(SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_TEXTURE_ADDRESS_CLAMP))),
            frame: gpu.buffer(&BufDesc { kind: BufKind::Constant, size: 16, dynamic: true }, None)?,
            shapes: None,
            grads: tex([GRAD_W, GRAD_ROWS], DXGI_FORMAT_R8G8B8A8_UNORM)?,
            grad_rows: Vec::new(),
            grads_dirty: false,
            atlas: tex([ATLAS, ATLAS], DXGI_FORMAT_R8_UNORM)?,
            pictures: Vec::new(),
            list: Vec::new(),
            clip: [-1e6, -1e6, 1e6, 1e6],
        })
    }

    /// The gradient of `stops` (position 0..1, colour), made once: its row.
    pub fn gradient(&mut self, stops: &[(f32, Rgba)]) -> u32 {
        if let Some(k) = self.grad_rows.iter().position(|g| g.as_slice() == stops) {
            return k as u32;
        }
        assert!((self.grad_rows.len() as u32) < GRAD_ROWS, "launcher: more than {GRAD_ROWS} gradients");
        self.grad_rows.push(stops.to_vec());
        self.grads_dirty = true;
        (self.grad_rows.len() - 1) as u32
    }

    /// The picture the Image shapes of `slot` draw (None: none).
    pub fn set_picture(&mut self, slot: u32, t: Option<std::sync::Arc<Texture>>) {
        let slot = slot as usize;
        if self.pictures.len() <= slot {
            self.pictures.resize(slot + 1, None);
        }
        self.pictures[slot] = t;
    }

    fn picture(&self, slot: u32) -> Option<&Texture> {
        self.pictures.get(slot as usize).and_then(|t| t.as_deref())
    }

    pub fn has_picture(&self, slot: u32) -> bool {
        self.picture(slot).is_some()
    }

    /// The picture's texels are sRGB (sampled linear): the Image shapes encode them again.
    pub fn picture_srgb(&self, slot: u32) -> bool {
        use windows::Win32::Graphics::Dxgi::Common::*;
        self.picture(slot).is_some_and(|t| {
            matches!(
                t.desc.format,
                DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
                    | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
                    | DXGI_FORMAT_BC1_UNORM_SRGB
                    | DXGI_FORMAT_BC2_UNORM_SRGB
                    | DXGI_FORMAT_BC3_UNORM_SRGB
                    | DXGI_FORMAT_BC7_UNORM_SRGB
            )
        })
    }

    /// The picture's size, texels.
    pub fn picture_size(&self, slot: u32) -> Option<[u32; 2]> {
        self.picture(slot).map(|t| [t.desc.size[0], t.desc.size[1]])
    }

    /// A shape added (it takes the painter's clip, intersected with its own); an Image whose slot holds no picture
    /// is not.
    pub fn push(&mut self, mut s: Shape) {
        if s.kind == Kind::Image && !self.has_picture(s.picture) {
            return;
        }
        s.clip = [
            s.clip[0].max(self.clip[0]),
            s.clip[1].max(self.clip[1]),
            s.clip[2].min(self.clip[2]),
            s.clip[3].min(self.clip[3]),
        ];
        if s.clip[0] >= s.clip[2] || s.clip[1] >= s.clip[3] || s.opacity <= 0.0 {
            return;
        }
        self.list.push(s);
    }

    /// Glyph coverage written into the atlas at `at` (`w` x `h` bytes).
    pub fn atlas_write(&self, gpu: &Gpu, at: [u32; 2], w: u32, h: u32, pixels: &[u8]) {
        if w == 0 || h == 0 {
            return;
        }
        let b = D3D11_BOX { left: at[0], top: at[1], front: 0, right: at[0] + w, bottom: at[1] + h, back: 1 };
        // SAFETY: the box lies in the atlas (the packer's); `pixels` holds `h` rows of `w` bytes.
        unsafe { gpu.ctx.UpdateSubresource(&self.atlas.res, 0, Some(&b), pixels.as_ptr().cast(), w, 0) };
    }

    fn upload_grads(&mut self, gpu: &Gpu) {
        if !self.grads_dirty {
            return;
        }
        self.grads_dirty = false;
        let mut px = vec![0u8; (GRAD_W * GRAD_ROWS * 4) as usize];
        for (row, stops) in self.grad_rows.iter().enumerate() {
            for x in 0..GRAD_W {
                let t = x as f32 / (GRAD_W - 1) as f32;
                let c = sample(stops, t);
                // (stored straight: the shader premultiplies; CSS mixes premultiplied, near enough for these stops)
                let at = (row * GRAD_W as usize + x as usize) * 4;
                for k in 0..4 {
                    px[at + k] = (c[k].clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
        }
        let b = D3D11_BOX { left: 0, top: 0, front: 0, right: GRAD_W, bottom: GRAD_ROWS, back: 1 };
        // SAFETY: the whole texture's box, `px` its rows of GRAD_W texels.
        unsafe { gpu.ctx.UpdateSubresource(&self.grads.res, 0, Some(&b), px.as_ptr().cast(), GRAD_W * 4, 0) };
    }

    /// The frame's shapes drawn over `target` (its contents kept), then forgotten.
    pub fn draw(&mut self, gpu: &Gpu, target: &Texture) -> Result<()> {
        self.upload_grads(gpu);
        let list = std::mem::take(&mut self.list);
        if list.is_empty() {
            return Ok(());
        }
        let rows: Vec<Row> = list.iter().map(|s| row_of(s, 1.0 / ATLAS as f32)).collect();
        let stride = std::mem::size_of::<Row>() as u32;
        let bytes_len = rows.len() * stride as usize;
        if self.shapes.as_ref().is_none_or(|b| (b.desc.size as usize) < bytes_len) {
            // (a structured buffer's size is whole rows)
            let size = (rows.len().next_power_of_two().max(64) * stride as usize) as u32;
            self.shapes =
                Some(gpu.buffer(&BufDesc { kind: BufKind::Structured { stride }, size, dynamic: true }, None)?);
        }
        let shapes = self.shapes.as_ref().expect("the shapes' buffer");
        // SAFETY: Row is plain f32 data (repr(C)), read as bytes.
        let bytes = unsafe { std::slice::from_raw_parts(rows.as_ptr().cast::<u8>(), bytes_len) };
        gpu.write(shapes, bytes);
        let size = [target.desc.size[0], target.desc.size[1]];
        let rtv = target.rtv.as_ref().ok_or(Error::Format { what: "launcher target".into(), why: "no RTV".into() })?;
        // (the picture's slot is always bound: the gradients stand in until a draw brings its own)
        let srvs = [shapes.srv.clone(), self.atlas.srv.clone(), self.grads.srv.clone(), self.grads.srv.clone()];
        // The draws: the shapes in order, a new one where an Image asks for another picture than the one bound.
        let mut draws: Vec<(usize, usize, Option<u32>)> = Vec::new();
        let (mut first, mut bound) = (0, None);
        for (k, s) in list.iter().enumerate().filter(|(_, s)| s.kind == Kind::Image) {
            if bound.is_some_and(|b| b != s.picture) {
                draws.push((first, k - first, bound));
                first = k;
            }
            bound = Some(s.picture);
        }
        draws.push((first, list.len() - first, bound));
        gpu.set_states(self.blend, self.raster, self.depth, 0);
        // SAFETY: state and draw calls with objects made on this device; the arrays outlive the calls.
        unsafe {
            gpu.ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            gpu.ctx.IASetInputLayout(None);
            gpu.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            gpu.ctx.VSSetShader(&self.vs, None);
            gpu.ctx.PSSetShader(&self.ps, None);
            gpu.ctx.GSSetShader(None, None);
            gpu.ctx.VSSetShaderResources(0, Some(&srvs[..1]));
            gpu.ctx.VSSetConstantBuffers(0, Some(&[Some(self.frame.buf.clone())]));
            gpu.ctx.PSSetShaderResources(0, Some(&srvs));
            gpu.ctx.PSSetSamplers(0, Some(&[Some(gpu.sampler(self.sampler))]));
        }
        gpu.viewport([0, 0, size[0], size[1]]);
        for (first, count, slot) in draws {
            gpu.write(&self.frame, &[size[0] as f32, size[1] as f32, first as f32, 0.0].map(f32::to_le_bytes).concat());
            let picture = slot.and_then(|s| self.picture(s)).and_then(|t| t.srv.clone());
            // SAFETY: six vertices an instance, the instances the buffer holds from the draw's first on; the picture
            // is a view made on this device.
            unsafe {
                if picture.is_some() {
                    gpu.ctx.PSSetShaderResources(3, Some(&[picture]));
                }
                gpu.ctx.DrawInstanced(6, count as u32, 0, 0);
            }
        }
        // SAFETY: the views unbound.
        unsafe {
            gpu.ctx.PSSetShaderResources(0, Some(&[None, None, None, None]));
            gpu.ctx.VSSetShaderResources(0, Some(&[None]));
        }
        Ok(())
    }
}

/// A gradient's colour at `t` (CSS: stops interpolated in order, the ends held).
fn sample(stops: &[(f32, Rgba)], t: f32) -> Rgba {
    let first = stops.first().map_or(CLEAR, |s| s.1);
    if stops.len() < 2 || t <= stops[0].0 {
        return first;
    }
    for w in stops.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if t <= b {
            let k = if b > a { (t - a) / (b - a) } else { 1.0 };
            // (mixed premultiplied, as CSS gradients are: a stop toward transparent keeps its hue)
            let pa = [ca[0] * ca[3], ca[1] * ca[3], ca[2] * ca[3], ca[3]];
            let pb = [cb[0] * cb[3], cb[1] * cb[3], cb[2] * cb[3], cb[3]];
            let m: [f32; 4] = std::array::from_fn(|k2| pa[k2] + (pb[k2] - pa[k2]) * k);
            return if m[3] > 1e-5 {
                [m[0] / m[3], m[1] / m[3], m[2] / m[3], m[3]]
            } else {
                [ca[0], ca[1], ca[2], 0.0]
            };
        }
    }
    stops[stops.len() - 1].1
}

/// A picture's RGBA8 texels as a texture the painter can draw.
pub fn picture_texture(gpu: &Gpu, w: u32, h: u32, rgba: &[u8]) -> Result<Texture> {
    Ok(gpu.texture(
        &TexDesc {
            kind: TexKind::D2,
            size: [w, h, 1],
            mips: 1,
            format: DXGI_FORMAT_R8G8B8A8_UNORM,
            views: TexViews { srv: true, ..Default::default() },
        },
        &[Subresource { data: rgba, row_pitch: w * 4, slice_pitch: 0 }],
    )?)
}
