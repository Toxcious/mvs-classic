//! The element list Slate paints (FSlateWindowElementList) and its draw (FSlateRHIRenderingPolicy::DrawElements):
//! boxes, nine-slice borders, text runs, UI materials and lines in layer order with their clip rects and draw
//! effects, batched and drawn on Direct3D 11 with the global FSlateElementVS / TSlateElementPS and the UI materials'
//! TSlateMaterialShader passes.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use indexmap::IndexSet;
use mvs_material::material::Filter;
use mvs_render::Assets;
use mvs_render::gpu::{
    BufDesc, BufKind, Buffer, DepthKey, Gpu, InputElement, RasterKey, SamplerKey, ShaderId, StateId, StateKey, TexDesc,
    TexKind, TexViews, Texture,
};
use mvs_render::material::{
    MaterialId, MaterialInstance, MaterialResource, Materials, SlateMaterialShader, Source, StageBinds, TextureSource,
    slate_blend, slate_material_blend, view_sampler,
};
use mvs_render::view::{VIEW_RESOURCES, VIEW_ROWS};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::bundle::{Bundle, Rgba};
use crate::menu::blur::BlurPass;
use crate::slate::{Margin, Mid};
use crate::text::{Fonts, GlyphPart, ShapedRun};
use crate::{Error, Result};

/// ESlateDrawEffect's bits (the 4.26 enum; NoPixelSnapping is 0x10 in the exe's FSlateElementBatcher, 141675570).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DrawEffects(pub u8);

impl DrawEffects {
    pub const NONE: DrawEffects = DrawEffects(0);
    pub const NO_BLENDING: DrawEffects = DrawEffects(1 << 0);
    pub const PRE_MULTIPLIED_ALPHA: DrawEffects = DrawEffects(1 << 1);
    pub const NO_GAMMA: DrawEffects = DrawEffects(1 << 2);
    pub const INVERT_ALPHA: DrawEffects = DrawEffects(1 << 3);
    pub const NO_PIXEL_SNAPPING: DrawEffects = DrawEffects(1 << 4);
    pub const DISABLED_EFFECT: DrawEffects = DrawEffects(1 << 5);
    pub const IGNORE_TEXTURE_ALPHA: DrawEffects = DrawEffects(1 << 6);
    pub const REVERSE_GAMMA: DrawEffects = DrawEffects(1 << 7);

    pub fn has(self, e: DrawEffects) -> bool {
        self.0 & e.0 != 0
    }
}

/// The paint geometry of an element: its local size and its layout-to-window transform (a 2D affine, row
/// vectors: x' = x a + y c + e, y' = x b + y d + f), pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaintGeometry {
    pub size: [f64; 2],
    pub transform: [f64; 6],
}

/// An image or nine-slice brush.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoxEl {
    pub geometry: PaintGeometry,
    /// The brush's texture (an object path, a store texture or a Slate brush PNG); None: white.
    pub texture: Option<String>,
    /// The nine-slice margin (a border or box brush), fractions of the texture.
    pub margin: Option<Margin>,
    pub tint: Rgba,
    /// The brush's UV region (u0, v0, u1, v1) and tiling.
    pub uv: [f64; 4],
    pub tile: [bool; 2],
}

/// A shaped text run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextEl {
    pub geometry: PaintGeometry,
    pub run: ShapedRun,
    pub color: Rgba,
    pub outline_color: Rgba,
    /// The font's FontMaterial (its dynamic instance when the HUD made one): the glyphs are drawn by the material's
    /// TSlateMaterialShaderPS GrayscaleFont pass, the atlas its texture.
    pub material: Option<Mid>,
}

/// A box drawn by a UI material.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaterialEl {
    pub geometry: PaintGeometry,
    pub material: Mid,
    pub tint: Rgba,
    /// The nine-slice margin, fractions of `image`.
    pub margin: Option<Margin>,
    /// The brush's ImageSize, Slate units (a box margin is a fraction of it).
    pub image: [f64; 2],
}

/// An SRetainerWidget's draw: its content rendered to a texture at `content_scale`, then drawn by the effect
/// material over the widget's geometry with the texture as its Texture parameter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RetainedEl {
    pub geometry: PaintGeometry,
    pub content: MaterialEl,
    pub content_scale: [f64; 2],
    pub effect: Mid,
    pub tint: Rgba,
}

/// A BackgroundBlur's pass (menu/blur.rs): what was drawn before it, blurred inside its geometry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlurEl {
    pub geometry: PaintGeometry,
    /// The strength after the alpha rule (more than 0).
    pub strength: f64,
}

/// A line strip (FSlateDrawElement::MakeLines).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineEl {
    pub geometry: PaintGeometry,
    pub points: Vec<[f64; 2]>,
    pub color: Rgba,
    pub thickness: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Element {
    Box(BoxEl),
    Text(TextEl),
    Material(MaterialEl),
    Line(LineEl),
    Retained(Box<RetainedEl>),
    Blur(BlurEl),
}

/// A convex clip zone: its corners in pixels, in order around it (either winding).
pub type Zone = Arc<[[f64; 2]]>;

/// An element with its layer, clip rect (pixels: x0, y0, x1, y1; None: the whole target), the zone a rotated or
/// sheared clip cuts it to (None: the rect alone) and draw effects.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub layer: i32,
    pub clip: Option<[f64; 4]>,
    pub zone: Option<Zone>,
    pub effects: DrawEffects,
    pub element: Element,
}

/// A frame's elements in paint order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ElementList {
    pub elements: Vec<Placed>,
}

impl ElementList {
    pub fn clear(&mut self) {
        self.elements.clear();
    }

    pub fn push(&mut self, layer: i32, clip: Option<[f64; 4]>, effects: DrawEffects, element: Element) {
        self.elements.push(Placed { layer, clip, zone: None, effects, element });
    }

    /// An element cut to the convex `zone` as well as its clip rect.
    pub fn push_zoned(
        &mut self,
        layer: i32,
        clip: Option<[f64; 4]>,
        zone: Option<Zone>,
        effects: DrawEffects,
        element: Element,
    ) {
        self.elements.push(Placed { layer, clip, zone, effects, element });
    }
}

/// FSlateVertex: TexCoords, MaterialTexCoords, Position, Color (an FColor: B, G, R, A in memory), PixelSize.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Vertex {
    tex: [f32; 4],
    mat: [f32; 2],
    pos: [f32; 2],
    color: [u8; 4],
    pixel: [u16; 2],
}

const VERTEX_BYTES: u32 = std::mem::size_of::<Vertex>() as u32;
/// Each attribute's byte offset in the vertex (one input slot each, the same buffer).
const ATTRIBUTE_OFFSETS: [u32; 5] = [0, 16, 24, 32, 36];
const VERTEX_LAYOUT: [InputElement; 5] = [
    InputElement {
        semantic: c"ATTRIBUTE",
        index: 0,
        format: DXGI_FORMAT_R32G32B32A32_FLOAT,
        slot: 0,
        per_instance: false,
    },
    InputElement { semantic: c"ATTRIBUTE", index: 1, format: DXGI_FORMAT_R32G32_FLOAT, slot: 1, per_instance: false },
    InputElement { semantic: c"ATTRIBUTE", index: 2, format: DXGI_FORMAT_R32G32_FLOAT, slot: 2, per_instance: false },
    InputElement { semantic: c"ATTRIBUTE", index: 3, format: DXGI_FORMAT_B8G8R8A8_UNORM, slot: 3, per_instance: false },
    InputElement { semantic: c"ATTRIBUTE", index: 4, format: DXGI_FORMAT_R16G16_UINT, slot: 4, per_instance: false },
];
/// The input layout's key for `Gpu::set_input_layout` (one layout per vertex shader).
const LAYOUT_KEY: u32 = 0x5c47_e700;

/// GEngine DisplayGamma (UEngine +0x904).
const DISPLAY_GAMMA: f32 = 2.2;
/// Slate.Contrast (144c35a44).
const CONTRAST: f32 = 1.0;
/// FSlateElementBatcher's anti-aliased line filter: FilterScale 1 x 2 sqrt 2, pixels.
// Unverified: 4.26's AddLineElement constants, not read from the exe.
const LINE_FILTER: f64 = 2.0 * std::f64::consts::SQRT_2;

/// TSlateElementPS's shader type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ElementPs {
    Default,
    GrayscaleFont,
    LineSegment,
}

/// A batch's texture.
#[derive(Clone, Debug, PartialEq)]
enum Tex {
    /// GWhiteTexture.
    White,
    /// A texture of `Ui::textures` by its path.
    Path(String),
    /// A glyph atlas page.
    Atlas(u16),
}

#[derive(Clone, Debug, PartialEq)]
enum Shading {
    /// The global FSlateElementVS / TSlateElementPS; `params`: the pixel shader's ShaderParams.
    /// `wrap`: the batch's DrawFlags TileU (0x20) and TileV (0x40), the brush's tiling per axis.
    Element { ps: ElementPs, tex: Tex, wrap: [bool; 2], params: [f32; 4] },
    /// A UI material's pass; `atlas`: the font atlas page a font pass reads.
    Material { mid: Mid, ps: SlateMaterialShader, atlas: Option<u16> },
}

#[derive(Clone, Debug)]
struct Batch {
    shading: Shading,
    scissor: RECT,
    effects: DrawEffects,
    first: u32,
    count: u32,
    /// The retained target the batch draws into (None: the frame's target).
    target: Option<usize>,
    /// The retained target the batch's material reads as its Mask parameter.
    mask: Option<usize>,
}

/// The largest retained target's side, pixels.
const RETAINED_MAX: u32 = 4096;
/// The retained targets kept before the next one frees them all (64 targets or 32 MB).
const RETAINED_KEPT: usize = 64;
const RETAINED_BYTES: u64 = 32_000_000;
/// The glyph atlas sets kept (`Ui::atlases`): the menus', a match's HUD and a screen's own fonts.
const ATLAS_SETS: usize = 4;

/// The most bytes of brush pictures `Ui` holds once no draw of the frame uses them. A picture held is one the
/// assets' trim cannot let go, so this is what the menus keep through a match.
const PICTURES_KEPT: u64 = 64 << 20;
/// The instances of one UI material kept with their parameters: past it, the least recently drawn is set again.
const POOLED_KEPT: usize = 64;

/// A brush picture held, its bytes on the GPU and the frame it last drew in.
#[derive(Debug)]
struct Picture {
    tex: Arc<Texture>,
    bytes: u64,
    used: u64,
}

/// A UI material's instance, the dynamic instance and Mask it was last set to and the frame it last drew in.
#[derive(Debug)]
struct Pooled {
    id: MaterialId,
    mid: Mid,
    mask: Option<Arc<Texture>>,
    used: u64,
}

/// What [`warm_hud`] made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HudWarmed {
    pub textures: usize,
    pub materials: usize,
}

/// A match's HUD pictures made with its load (on the frame thread, in the match load), so its first frames draw them:
/// the hud bundle's textures but those only a soft reference names, the textures of `chars`' own HUD
/// widgets (their stocks and icons), and every UI material of the bundle (the portraits' and the pop-ups', with their
/// textures and Slate shaders). A texture the build does not hold (cooked out) is left out, as the draw leaves it.
// Deviation: a texture of more than 1 MB decoded is not deferred to its first paint (a saving of memory); every
// texture but the soft ones is made here.
pub fn warm_hud(assets: &Assets, b: &Bundle, chars: &[&str]) -> Result<HudWarmed> {
    let mut warmed = HudWarmed::default();
    let own: std::collections::HashSet<&str> =
        chars.iter().filter_map(|c| b.char_textures.get(*c)).flatten().map(String::as_str).collect();
    let texture_pack = assets.store().kind("texture").ok();
    let image_pack = assets.store().kind("image").ok();
    for (name, info) in &b.textures {
        if b.hud_soft_textures.contains(name) && !own.contains(name.as_str()) {
            continue;
        }
        match (&info.path, &info.image) {
            (Some(p), _) if texture_pack.as_ref().is_some_and(|k| k.has(p)) => drop(assets.texture(p)?),
            (None, Some(p)) if image_pack.as_ref().is_some_and(|k| k.has(p)) => drop(assets.image(p)?),
            _ => continue,
        }
        warmed.textures += 1;
    }
    // (the portraits' materials are the bundle's, by name)
    let materials: IndexSet<&str> = b.materials.values().map(String::as_str).filter(|p| !p.is_empty()).collect();
    for p in materials {
        // (a material that fails is left to its first draw, which reports it: warming is no reason to stop the load)
        match assets.warm_ui_material(p) {
            Ok(()) => warmed.materials += 1,
            Err(e) => eprintln!("warning: hud warm: {p}: {e}"),
        }
    }
    Ok(warmed)
}

/// An element batch's sampler (DrawElements 142566370): the texture's filter (its Filter, TF_Default its LOD group's;
/// bilinear without a texture: the white brush, a glyph atlas, an engine Slate brush file), the address by the batch's
/// TileU / TileV flags per axis, else clamped.
// Unverified: the filters' D3D11 terms and the anisotropy (r.MaxAnisotropy) are the material samplers'
// (mvs-render material.rs sampler_key, 4.26's TStaticSamplerState), not read from DrawElements.
fn element_sampler(filter: Filter, wrap: [bool; 2]) -> SamplerKey {
    let f = match filter {
        Filter::Point => D3D11_FILTER_MIN_MAG_MIP_POINT,
        Filter::Bilinear => D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT,
        Filter::Trilinear => D3D11_FILTER_MIN_MAG_MIP_LINEAR,
        Filter::AnisotropicPoint | Filter::AnisotropicLinear => D3D11_FILTER_ANISOTROPIC,
    };
    let a = |wrap: bool| if wrap { D3D11_TEXTURE_ADDRESS_WRAP } else { D3D11_TEXTURE_ADDRESS_CLAMP };
    let mut s = SamplerKey::of(f, a(wrap[0]));
    s.address[1] = a(wrap[1]).0 as u8;
    if f == D3D11_FILTER_ANISOTROPIC {
        s.max_anisotropy = mvs_render::assets::max_anisotropy();
    }
    s
}

/// A texture's bytes on the GPU (every mip and layer).
fn texture_bytes(t: &Texture) -> u64 {
    let [w, h, d] = t.desc.size;
    let (block, block_bytes) = mvs_render::gpu::format_layout(t.desc.format).unwrap_or((1, 4));
    (0..t.desc.mips.max(1))
        .map(|m| {
            let (mw, mh) = ((w >> m).max(1), (h >> m).max(1));
            u64::from(mw.div_ceil(block)) * u64::from(mh.div_ceil(block)) * u64::from(block_bytes)
        })
        .sum::<u64>()
        * u64::from(d.max(1))
}

/// One glyph atlas page on the GPU (DXGI A8: the font pixel shaders read the coverage from alpha).
#[derive(Debug)]
struct AtlasTexture {
    tex: ID3D11Texture2D,
    srv: ID3D11ShaderResourceView,
}

/// The element batches' draw: the Slate shaders and states, the vertex and index buffers, the glyph atlas texture,
/// the UI materials' instances.
#[derive(Debug)]
pub struct Ui {
    vs: ShaderId,
    ps: HashMap<(ElementPs, bool, bool), ShaderId>,
    vb: Option<Buffer>,
    ib: Option<Buffer>,
    /// FSlateElementVS's $Globals (ViewProjection), TSlateElementPS's (GammaAndAlphaValues, ShaderParams).
    vs_globals: Buffer,
    ps_globals: Buffer,
    /// TSlateMaterialShaderVS's $Globals (ViewProjection at row 1), TSlateMaterialShaderPS's (ShaderParams,
    /// GammaAndAlphaValues, ShaderParams2, DrawFlags).
    material_vs_globals: Buffer,
    material_ps_globals: Buffer,
    /// The Slate scene view's View buffer (its rect and time).
    view: Buffer,
    raster: ID3D11RasterizerState,
    /// A rasterizer state for `Gpu::set_states`; the scissor one (`raster`) replaces it before each draw.
    raster_id: StateId,
    depth: StateId,
    /// The font atlas's sampler in a material pass (bilinear, clamped).
    clamp_sampler: StateId,
    /// The filter of each texture the UI bundles name, by its texture kind entry (`Bundle::texture_filters`).
    filters: HashMap<String, Filter>,
    /// The glyph atlas pages on the GPU per `Fonts` drawn with (`GlyphAtlas::id`), the last drawn with first: the
    /// menus' and the HUD's fonts draw in one frame, and each packs its glyphs from the same corner.
    atlases: Vec<(u64, Vec<AtlasTexture>)>,
    /// The brush pictures drawn lately by path, held past the screen that drew them up to `PICTURES_KEPT` bytes
    /// (the store's caches let go what nobody holds when a match loads).
    pictures: HashMap<String, Picture>,
    pictures_bytes: u64,
    /// The UI materials' instances (MPC_CentralPalette set through `Materials::set_collection`).
    pub materials: Materials,
    /// The instances of each UI material by its path, each with the parameters it was last set to.
    material_ids: HashMap<String, Vec<Pooled>>,
    /// Draw without a picture or UI material not on the GPU yet (a loader makes it; the element shows once it is),
    /// else make it on this thread before the draw (a draw painted once and kept needs every picture).
    pub stream_pictures: bool,
    /// The elements the frame's draws left out for a picture or material still loading.
    waiting: usize,
    /// The frames drawn (a draw at a new `time`), the pictures' and instances' last use.
    frame: u64,
    /// The last draw's vertices and batches, kept for their memory.
    builder: Builder,
    /// The application time the Slate scene view gives the materials (View GameTime and RealTime), seconds; None:
    /// the time since `Ui::new`.
    pub time: Option<f64>,
    start: Instant,
    /// The BackgroundBlur passes' shaders and scratch textures, made at the first blur.
    blur: Option<BlurPass>,
    /// The time of the last draw: a draw at another is a new frame's (the frame's draws share `time`).
    drawn_at: Option<f64>,
    /// The retained draws' targets by pixel size (SRetainerWidget's render targets).
    retained: HashMap<[u32; 2], Arc<Texture>>,
    /// The bytes of `retained` (RGBA8).
    retained_bytes: u64,
}

fn dx(what: &'static str) -> impl FnOnce(windows::core::Error) -> Error {
    move |source| Error::Render(mvs_render::Error::Device { what: what.into(), source })
}

/// FLinearColor::ToFColor(bSRGB true) (exe 141371400), the batcher's vertex colour (FSlateElementBatcher's
/// constructor 14166b330 sets bSRGBVertexColor): each channel clamped, the colour sRGB-encoded, floored to 8 bits;
/// B, G, R, A as FColor holds them.
fn vertex_color(c: Rgba) -> [u8; 4] {
    let byte = |x: f32| (x * 255.999).floor() as u8;
    let srgb = |x: f64| {
        let x = (x as f32).clamp(0.0, 1.0);
        byte(if x <= 0.003_130_8 { x * 12.92 } else { x.powf(1.0 / 2.4) * 1.055 - 0.055 })
    };
    [srgb(c[2]), srgb(c[1]), srgb(c[0]), byte((c[3] as f32).clamp(0.0, 1.0))]
}

/// A point through the element's transform.
fn place(t: &[f64; 6], x: f64, y: f64) -> [f32; 2] {
    [(t[0] * x + t[2] * y + t[4]) as f32, (t[1] * x + t[3] * y + t[5]) as f32]
}

/// The geometry's draw scale on each axis (the transform's axis lengths).
fn draw_scale(t: &[f64; 6]) -> [f64; 2] {
    [t[0].hypot(t[1]), t[2].hypot(t[3])]
}

/// The four stops of a nine-slice span `size` long: margins `m0` / `m1` (local units), clamped to half the size each
/// when they overlap (FSlateElementBatcher::AddBoxElement).
fn stops(size: f64, m0: f64, m1: f64) -> [f64; 4] {
    let (m0, m1) = if m0 + m1 > size { (size / 2.0, size / 2.0) } else { (m0, m1) };
    [0.0, m0, size - m1, size]
}

/// The element vertices of `list` in batches, built before they are drawn.
#[derive(Debug, Default)]
struct Builder {
    verts: Vec<Vertex>,
    idx: Vec<u32>,
    batches: Vec<Batch>,
    /// The blur passes, each with the number of batches before it and its clip rect: they run where they stand in the
    /// order, over what the batches before them drew.
    blurs: Vec<(usize, BlurEl, Option<[f64; 4]>)>,
    /// The batches before the last blur pass: a batch after it does not merge into one of them.
    fence: usize,
    /// The retained targets' pixel sizes, by their index in the batches' `target` / `mask`.
    targets: Vec<[u32; 2]>,
    /// The target and mask of the quads added now.
    target: Option<usize>,
    mask: Option<usize>,
    /// The zone the quads added now are cut to.
    zone: Option<Zone>,
    /// The elements' indices in draw order.
    order: Vec<usize>,
}

impl Builder {
    /// Emptied for the next draw, its memory kept.
    fn clear(&mut self) {
        self.verts.clear();
        self.idx.clear();
        self.batches.clear();
        self.blurs.clear();
        self.fence = 0;
        self.targets.clear();
        self.target = None;
        self.mask = None;
        self.zone = None;
        self.order.clear();
    }

    /// A quad's 4 vertices (corners in the order top left, top right, bottom left, bottom right) and its 6 indices
    /// into the batch of `shading`, merged with the last batch when that draws the same way.
    /// Under a zone, each of the quad's two triangles is cut to it (the zone's pixels only, as Slate's stencil clip of
    /// a rotated or sheared clip rect draws; the cut corners' attributes interpolated as the rasterizer would).
    fn quad(&mut self, shading: &Shading, scissor: RECT, effects: DrawEffects, v: [Vertex; 4]) {
        let first = u32::try_from(self.idx.len()).expect("ui: indices past u32");
        match self.zone.clone() {
            None => {
                let base = u32::try_from(self.verts.len()).expect("ui: vertices past u32");
                self.verts.extend_from_slice(&v);
                self.idx.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
            }
            Some(zone) => {
                for tri in [[v[0], v[1], v[2]], [v[2], v[1], v[3]]] {
                    let poly = cut(&tri, &zone);
                    if poly.len() < 3 {
                        continue;
                    }
                    let base = u32::try_from(self.verts.len()).expect("ui: vertices past u32");
                    self.verts.extend_from_slice(&poly);
                    for k in 1..poly.len() as u32 - 1 {
                        self.idx.extend_from_slice(&[base, base + k, base + k + 1]);
                    }
                }
            }
        }
        let count = u32::try_from(self.idx.len()).expect("ui: indices past u32") - first;
        if count == 0 {
            return;
        }
        // (a material batch merges only with its own parameters: the same draw)
        let (target, mask) = (self.target, self.mask);
        let merge = self.batches.len() > self.fence
            && self.batches.last().is_some_and(|b| {
                b.shading == *shading
                    && b.scissor == scissor
                    && b.effects == effects
                    && b.first + b.count == first
                    && b.target == target
                    && b.mask == mask
            });
        if merge {
            self.batches.last_mut().expect("a batch").count += count;
        } else {
            self.batches.push(Batch { shading: shading.clone(), scissor, effects, first, count, target, mask });
        }
    }

    /// A box of `size` (local units) through `t`, nine-sliced by `margin` (local units: left, top, right, bottom)
    /// with UV `uv` and its margins `uv_margin` (fractions of the UV span); `tiling` the TexCoords' z / w.
    #[allow(clippy::too_many_arguments)]
    fn boxed(
        &mut self,
        shading: &Shading,
        scissor: RECT,
        effects: DrawEffects,
        t: &[f64; 6],
        size: [f64; 2],
        margin: Option<[f64; 4]>,
        uv_margin: Option<Margin>,
        uv: [f64; 4],
        tiling: [f64; 2],
        color: [u8; 4],
    ) {
        let [w, h] = size;
        let s = draw_scale(t);
        let pixel = [(w * s[0]).round().clamp(0.0, 65535.0) as u16, (h * s[1]).round().clamp(0.0, 65535.0) as u16];
        let vert = |x: f64, y: f64, u: f64, v: f64| Vertex {
            tex: [u as f32, v as f32, tiling[0] as f32, tiling[1] as f32],
            mat: [if w != 0.0 { (x / w) as f32 } else { 0.0 }, if h != 0.0 { (y / h) as f32 } else { 0.0 }],
            pos: place(t, x, y),
            color,
            pixel,
        };
        let (xs, ys, us, vs) = match (margin, uv_margin) {
            (Some(m), Some(f)) => {
                let (du, dv) = (uv[2] - uv[0], uv[3] - uv[1]);
                (
                    stops(w, m[0], m[2]),
                    stops(h, m[1], m[3]),
                    [uv[0], uv[0] + du * f.l, uv[2] - du * f.r, uv[2]],
                    [uv[1], uv[1] + dv * f.t, uv[3] - dv * f.b, uv[3]],
                )
            }
            _ => {
                self.quad(
                    shading,
                    scissor,
                    effects,
                    [
                        vert(0.0, 0.0, uv[0], uv[1]),
                        vert(w, 0.0, uv[2], uv[1]),
                        vert(0.0, h, uv[0], uv[3]),
                        vert(w, h, uv[2], uv[3]),
                    ],
                );
                return;
            }
        };
        for j in 0..3 {
            for i in 0..3 {
                if xs[i + 1] <= xs[i] || ys[j + 1] <= ys[j] {
                    continue;
                }
                self.quad(
                    shading,
                    scissor,
                    effects,
                    [
                        vert(xs[i], ys[j], us[i], vs[j]),
                        vert(xs[i + 1], ys[j], us[i + 1], vs[j]),
                        vert(xs[i], ys[j + 1], us[i], vs[j + 1]),
                        vert(xs[i + 1], ys[j + 1], us[i + 1], vs[j + 1]),
                    ],
                );
            }
        }
    }
}

/// The convex polygon `poly` cut to the convex `zone` (Sutherland-Hodgman), each new corner's attributes interpolated
/// along its edge.
fn cut(poly: &[Vertex], zone: &[[f64; 2]]) -> Vec<Vertex> {
    // (the zone's winding: inside is the side of each edge its area's sign gives)
    let n = zone.len();
    let area: f64 = (0..n).map(|i| zone[i][0] * zone[(i + 1) % n][1] - zone[(i + 1) % n][0] * zone[i][1]).sum();
    if area == 0.0 {
        return Vec::new();
    }
    let sign = area.signum();
    let mut out: Vec<Vertex> = poly.to_vec();
    for i in 0..n {
        if out.is_empty() {
            break;
        }
        let (a, b) = (zone[i], zone[(i + 1) % n]);
        let side = |v: &Vertex| {
            let (x, y) = (f64::from(v.pos[0]), f64::from(v.pos[1]));
            sign * ((b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]))
        };
        let input = std::mem::take(&mut out);
        for (k, p) in input.iter().enumerate() {
            let q = &input[(k + 1) % input.len()];
            let (dp, dq) = (side(p), side(q));
            if dp >= 0.0 {
                out.push(*p);
            }
            if (dp >= 0.0) != (dq >= 0.0) {
                out.push(lerp_vertex(p, q, dp / (dp - dq)));
            }
        }
    }
    out
}

/// The vertex a fraction `t` of the way from `p` to `q`.
fn lerp_vertex(p: &Vertex, q: &Vertex, t: f64) -> Vertex {
    let f = |a: f32, b: f32| (f64::from(a) + (f64::from(b) - f64::from(a)) * t) as f32;
    let c = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round().clamp(0.0, 255.0) as u8;
    Vertex {
        tex: std::array::from_fn(|i| f(p.tex[i], q.tex[i])),
        mat: std::array::from_fn(|i| f(p.mat[i], q.mat[i])),
        pos: std::array::from_fn(|i| f(p.pos[i], q.pos[i])),
        color: std::array::from_fn(|i| c(p.color[i], q.color[i])),
        pixel: p.pixel,
    }
}

/// D3D11's scissor rect of a clip (pixels; None: the whole target), clamped to the target.
// Unverified: the rounding of FSlateRect to the scissor's whole pixels (4.26 rounds to the nearest).
fn scissor_of(clip: Option<[f64; 4]>, size: [u32; 2]) -> RECT {
    let [w, h] = size.map(|v| i32::try_from(v).unwrap_or(i32::MAX));
    match clip {
        None => RECT { left: 0, top: 0, right: w, bottom: h },
        Some(c) => {
            let r = |v: f64, hi: i32| (v.round() as i32).clamp(0, hi);
            let (left, top) = (r(c[0], w), r(c[1], h));
            RECT { left, top, right: r(c[2], w).max(left), bottom: r(c[3], h).max(top) }
        }
    }
}

impl Ui {
    pub fn new(gpu: &Gpu, assets: &Assets) -> Result<Ui> {
        let g = assets.global_shaders();
        let code = |ty: &str| -> Result<_> {
            let row = g
                .find(ty, 0, mvs_material::Frequency::Vertex)
                .ok_or_else(|| Error::Format { what: ty.to_string(), why: "not in the global shader cache".into() })?;
            Ok((g.shaders.code(row), assets.code(g.shaders.code(row))?))
        };
        let (sha, vs_code) = code("FSlateElementVS")?;
        let vs = gpu.shader_by(sha, &vs_code)?;
        let constant =
            |rows: u32| gpu.buffer(&BufDesc { kind: BufKind::Constant, size: rows * 16, dynamic: true }, None);
        // No culling, the scissor test on: the batch's clip rect is its scissor rect.
        let rd = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            FrontCounterClockwise: true.into(),
            DepthBias: 0,
            DepthBiasClamp: 0.0,
            SlopeScaledDepthBias: 0.0,
            DepthClipEnable: true.into(),
            ScissorEnable: true.into(),
            MultisampleEnable: false.into(),
            AntialiasedLineEnable: false.into(),
        };
        let mut raster = None;
        // SAFETY: a plain description on this device.
        unsafe { gpu.dev.CreateRasterizerState(&rd, Some(&mut raster)).map_err(dx("ui rasterizer state"))? };
        Ok(Ui {
            vs,
            ps: HashMap::new(),
            vb: None,
            ib: None,
            vs_globals: constant(4)?,
            ps_globals: constant(2)?,
            material_vs_globals: constant(5)?,
            material_ps_globals: constant(4)?,
            view: constant(VIEW_ROWS as u32)?,
            raster: raster.expect("CreateRasterizerState"),
            raster_id: gpu.state(StateKey::Raster(RasterKey {
                fill: D3D11_FILL_SOLID.0 as u8,
                cull: D3D11_CULL_NONE.0 as u8,
                front_ccw: true,
                depth_bias: 0,
                slope_scaled_depth_bias: 0,
            })),
            depth: gpu.state(StateKey::Depth(DepthKey {
                depth_write: false,
                depth_func: D3D11_COMPARISON_ALWAYS.0 as u8,
                stencil: None,
            })),
            clamp_sampler: gpu.state(StateKey::Sampler(element_sampler(Filter::Bilinear, [false; 2]))),
            // (a store without the ui kind names no bundle texture: every brush bilinear)
            filters: crate::bundle::Bundle::texture_filters(assets.store()).unwrap_or_default(),
            atlases: Vec::new(),
            pictures: HashMap::new(),
            pictures_bytes: 0,
            materials: Materials::default(),
            material_ids: HashMap::new(),
            stream_pictures: false,
            waiting: 0,
            frame: 0,
            builder: Builder::default(),
            time: None,
            start: Instant::now(),
            blur: None,
            drawn_at: None,
            retained: HashMap::new(),
            retained_bytes: 0,
        })
    }

    /// TSlateElementPS<type, bDrawDisabledEffect, bUseTextureAlpha> from the global cache, made on first use.
    fn element_ps(
        &mut self,
        gpu: &Gpu,
        assets: &Assets,
        ty: ElementPs,
        disabled: bool,
        alpha: bool,
    ) -> Result<ShaderId> {
        // The font and line shaders are cooked with texture alpha only.
        let alpha = alpha || ty != ElementPs::Default;
        if let Some(&s) = self.ps.get(&(ty, disabled, alpha)) {
            return Ok(s);
        }
        let name = format!("TSlateElementPS{ty:?}{disabled}{alpha}A");
        let g = assets.global_shaders();
        let row = g
            .find(&name, 0, mvs_material::Frequency::Pixel)
            .ok_or_else(|| Error::Format { what: name.clone(), why: "not in the global shader cache".into() })?;
        let sha = g.shaders.code(row);
        let s = gpu.shader_by(sha, &*assets.code(sha)?)?;
        self.ps.insert((ty, disabled, alpha), s);
        Ok(s)
    }

    /// A brush picture by its path: the texture kind's, else an engine brush file of the image kind or one of the
    /// port's own icons (both made by `Assets::image`). None: a loader
    /// makes it (`stream_pictures`). A picture made is held (`pictures`), the least recently drawn let go past
    /// `PICTURES_KEPT` bytes.
    fn picture(&mut self, assets: &Assets, path: &str) -> Result<Option<Arc<Texture>>> {
        if let Some(p) = self.pictures.get_mut(path) {
            p.used = self.frame;
            return Ok(Some(p.tex.clone()));
        }
        let texture = assets.store().kind("texture").is_ok_and(|k| k.has(path));
        let t = match (texture, self.stream_pictures) {
            (true, true) => assets.texture_soon(path)?,
            (false, true) => assets.image_soon(path)?,
            (true, false) => Some(assets.texture(path)?),
            (false, false) => Some(assets.image(path)?),
        };
        let Some(t) = t else { return Ok(None) };
        let bytes = texture_bytes(&t);
        self.pictures.insert(path.to_string(), Picture { tex: t.clone(), bytes, used: self.frame });
        self.pictures_bytes += bytes;
        let kept = PICTURES_KEPT;
        while self.pictures_bytes > kept {
            let Some(old) =
                self.pictures.iter().filter(|p| p.1.used < self.frame).min_by_key(|p| p.1.used).map(|p| p.0.clone())
            else {
                break;
            };
            let p = self.pictures.remove(&old).expect("a picture held");
            self.pictures_bytes -= p.bytes;
        }
        Ok(Some(t))
    }

    /// Whether a UI material's instance can be made or found without a store read: its material warmed and the
    /// textures `mid` sets on the GPU (each asked of a loader if not). Always true unless `stream_pictures`.
    fn mid_ready(&self, assets: &Assets, mid: &Mid) -> Result<bool> {
        if !self.stream_pictures {
            return Ok(true);
        }
        let mut ready =
            self.material_ids.get(&mid.path).is_some_and(|v| !v.is_empty()) || assets.material_soon(&mid.path)?;
        for (_, p) in &mid.textures {
            ready &= assets.texture_soon(p)?.is_some();
        }
        Ok(ready)
    }

    /// An instance of `mid`'s material set to `mid`'s parameters and `mask` as its Mask: the one last set to them,
    /// else one not drawn this frame set again (or a new one while fewer than `POOLED_KEPT`), so a material drawn
    /// with the same parameters frame after frame writes its buffer once.
    fn material(&mut self, gpu: &Gpu, assets: &Assets, mid: &Mid, mask: Option<Arc<Texture>>) -> Result<MaterialId> {
        let frame = self.frame;
        let same = |p: &Pooled| {
            p.mid == *mid
                && match (&p.mask, &mask) {
                    (None, None) => true,
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    _ => false,
                }
        };
        if let Some(p) = self.material_ids.get_mut(&mid.path).and_then(|v| v.iter_mut().find(|p| same(p))) {
            p.used = frame;
            return Ok(p.id);
        }
        let pool = self.material_ids.get(&mid.path);
        let len = pool.map_or(0, Vec::len);
        let idle = pool.and_then(|v| (0..v.len()).filter(|&k| v[k].used != frame).min_by_key(|&k| v[k].used));
        let oldest = pool.and_then(|v| (0..v.len()).min_by_key(|&k| v[k].used));
        let k = match (idle, oldest) {
            (Some(k), _) => k,
            (None, Some(k)) if len >= POOLED_KEPT => k,
            _ => {
                let id = self.materials.add(MaterialInstance::new(gpu, assets, &mid.path)?);
                let v = self.material_ids.entry(mid.path.clone()).or_default();
                v.push(Pooled { id, mid: Mid::of(""), mask: None, used: frame });
                v.len() - 1
            }
        };
        let mut textures = Vec::with_capacity(mid.textures.len() + 1);
        for (n, p) in &mid.textures {
            textures.push((n.as_str(), assets.texture(p)?));
        }
        if let Some(t) = &mask {
            textures.push(("Mask", t.clone()));
        }
        let pooled = &mut self.material_ids.get_mut(&mid.path).expect("a pool")[k];
        let id = pooled.id;
        self.materials.get_mut(id).set_parameters(assets, &mid.scalars, &mid.vectors, &textures)?;
        pooled.mid = mid.clone();
        pooled.mask = mask;
        pooled.used = frame;
        Ok(id)
    }

    /// The glyph atlas pages of `fonts` written since its last draw, uploaded, its pages the first set. A `Fonts`
    /// whose set was let go, or never made, gets its pages whole.
    fn upload_atlas(&mut self, gpu: &Gpu, fonts: &mut Fonts) -> Result<()> {
        let atlas = fonts.atlas();
        let [w, h] = atlas.size;
        match self.atlases.iter().position(|(id, _)| *id == atlas.id) {
            Some(i) => self.atlases[..=i].rotate_right(1),
            None => {
                self.atlases.insert(0, (atlas.id, Vec::new()));
                self.atlases.truncate(ATLAS_SETS);
            }
        }
        let set = &mut self.atlases[0].1;
        for (k, page) in atlas.pages.iter().enumerate() {
            if k == set.len() {
                let td = D3D11_TEXTURE2D_DESC {
                    Width: w,
                    Height: h,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };
                let init = D3D11_SUBRESOURCE_DATA {
                    pSysMem: page.pixels.as_ptr().cast(),
                    SysMemPitch: w,
                    SysMemSlicePitch: 0,
                };
                let (mut tex, mut srv) = (None, None);
                // SAFETY: `init` points at the page's w x h bytes, alive through the call.
                unsafe {
                    gpu.dev.CreateTexture2D(&td, Some(&init), Some(&mut tex)).map_err(dx("glyph atlas page"))?;
                }
                let tex = tex.expect("CreateTexture2D");
                // SAFETY: a view of a texture made with the shader-resource bind flag.
                unsafe { gpu.dev.CreateShaderResourceView(&tex, None, Some(&mut srv)).map_err(dx("glyph atlas SRV"))? };
                set.push(AtlasTexture { tex, srv: srv.expect("CreateShaderResourceView") });
            } else if let Some([x0, y0, x1, y1]) = page.dirty {
                let b = D3D11_BOX { left: x0, top: y0, front: 0, right: x1, bottom: y1, back: 1 };
                let at = (y0 * w + x0) as usize;
                // SAFETY: the box lies in the page (the packer's dirty rect); the source starts at its first texel
                // and rows are w bytes apart, all inside `pixels`.
                unsafe {
                    gpu.ctx.UpdateSubresource(&set[k].tex, 0, Some(&b), page.pixels[at..].as_ptr().cast(), w, 0);
                }
            }
        }
        fonts.take_dirty();
        Ok(())
    }

    /// `list` drawn over `target` (the back buffer, after the renderer's picture), batched by layer, shader,
    /// texture, clip and effects.
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        assets: &Assets,
        fonts: &mut Fonts,
        list: &ElementList,
        target: &Texture,
    ) -> Result<()> {
        self.upload_atlas(gpu, fonts)?;
        let time = self.time.unwrap_or_else(|| self.start.elapsed().as_secs_f64());
        if self.drawn_at != Some(time) {
            // (the last frame's draws are over: the blur's scratch textures kept, made smaller or given back)
            self.drawn_at = Some(time);
            self.frame += 1;
            self.waiting = 0;
            if let Some(b) = &mut self.blur {
                b.end_frame(gpu)?;
            }
        }
        let mut b = std::mem::take(&mut self.builder);
        b.clear();
        let drawn = self.draw_built(gpu, assets, fonts, list, target, &mut b);
        self.builder = b;
        drawn
    }

    /// The elements left out of the frame's draws so far for a picture or UI material still loading.
    pub fn waiting(&self) -> usize {
        self.waiting
    }

    /// `draw` with its builder (cleared).
    fn draw_built(
        &mut self,
        gpu: &Gpu,
        assets: &Assets,
        fonts: &Fonts,
        list: &ElementList,
        target: &Texture,
        b: &mut Builder,
    ) -> Result<()> {
        let size = [target.desc.size[0], target.desc.size[1]];
        // FSlateElementBatcher: the elements by layer, in the order they were added within a layer.
        let mut order = std::mem::take(&mut b.order);
        order.extend(0..list.elements.len());
        order.sort_by_key(|&k| list.elements[k].layer);
        let built = order.iter().try_for_each(|&k| self.build(assets, fonts, &list.elements[k], size, b));
        b.order = order;
        built?;
        if b.batches.is_empty() && b.blurs.is_empty() {
            return Ok(());
        }
        let vb_bytes = std::mem::size_of_val(b.verts.as_slice());
        let ib_bytes = std::mem::size_of_val(b.idx.as_slice());
        // SAFETY: Vertex is plain data (repr(C), no padding: 40 bytes of f32, u8 and u16), read as bytes.
        let vb_data = unsafe { std::slice::from_raw_parts(b.verts.as_ptr().cast::<u8>(), vb_bytes) };
        // SAFETY: u32s read as bytes.
        let ib_data = unsafe { std::slice::from_raw_parts(b.idx.as_ptr().cast::<u8>(), ib_bytes) };
        let grow = |buf: &mut Option<Buffer>, kind: BufKind, bytes: &[u8]| -> Result<()> {
            if buf.as_ref().is_none_or(|x| (x.desc.size as usize) < bytes.len()) {
                let size = (bytes.len().next_power_of_two().max(4096)) as u32;
                *buf = Some(gpu.buffer(&BufDesc { kind, size, dynamic: true }, None)?);
            }
            gpu.write(buf.as_ref().expect("a buffer"), bytes);
            Ok(())
        };
        grow(&mut self.vb, BufKind::Vertex, vb_data)?;
        grow(&mut self.ib, BufKind::Index32, ib_data)?;

        self.write_projection(gpu, size);
        let (w, h) = (size[0] as f32, size[1] as f32);
        // The Slate scene view: ViewRectMin (row 129), ViewSizeAndInvSize (130), GameTime / RealTime (142 z / w).
        // (the frame's time: `draw` set it)
        let time = self.drawn_at.unwrap_or_default() as f32;
        let mut view = vec![[0u32; 4]; VIEW_ROWS];
        view[130] = [w, h, 1.0 / w, 1.0 / h].map(f32::to_bits);
        view[142] = [0.0, 0.0, time, time].map(f32::to_bits);
        gpu.write_rows(&self.view, &view);

        let rtv = target
            .rtv
            .as_ref()
            .ok_or_else(|| Error::Format { what: "ui target".into(), why: "no render target view".into() })?;
        let vb = &self.vb.as_ref().expect("a vertex buffer").buf;
        let vbs = [Some(vb.clone()), Some(vb.clone()), Some(vb.clone()), Some(vb.clone()), Some(vb.clone())];
        let strides = [VERTEX_BYTES; 5];
        // SAFETY: state calls with views and buffers made on this device; the arrays outlive the calls.
        unsafe {
            gpu.ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            gpu.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            gpu.ctx.IASetVertexBuffers(
                0,
                5,
                Some(vbs.as_ptr()),
                Some(strides.as_ptr()),
                Some(ATTRIBUTE_OFFSETS.as_ptr()),
            );
            gpu.ctx.IASetIndexBuffer(&self.ib.as_ref().expect("an index buffer").buf, DXGI_FORMAT_R32_UINT, 0);
        }
        gpu.viewport([0, 0, size[0], size[1]]);
        // The retained targets, one per pixel size (a target is drawn into and read before the next element's).
        let mut targets = Vec::with_capacity(b.targets.len());
        for &[tw, th] in &b.targets {
            let bytes = u64::from(tw) * u64::from(th) * 4;
            let target = match self.retained.get(&[tw, th]) {
                Some(t) => t.clone(),
                None => {
                    // (past the count or the bytes, all are freed: the ones this
                    // frame's earlier batches hold stay alive through their handles)
                    if self.retained.len() >= RETAINED_KEPT || self.retained_bytes + bytes > RETAINED_BYTES {
                        self.retained.clear();
                        self.retained_bytes = 0;
                    }
                    let desc = TexDesc {
                        kind: TexKind::D2,
                        size: [tw, th, 1],
                        mips: 1,
                        format: DXGI_FORMAT_R8G8B8A8_UNORM,
                        views: TexViews { srv: true, rtv: true, ..Default::default() },
                    };
                    let t = Arc::new(gpu.texture(&desc, &[])?);
                    self.retained_bytes += bytes;
                    self.retained.insert([tw, th], t.clone());
                    t
                }
            };
            targets.push(target);
        }
        let mut bound: Option<usize> = None;
        let mut blurs = b.blurs.iter().peekable();
        for (i, batch) in b.batches.iter().enumerate() {
            while let Some((_, e, clip)) = blurs.next_if(|(at, ..)| *at <= i) {
                self.run_blur(gpu, target, e, *clip)?;
            }
            if batch.target != bound {
                bound = batch.target;
                let (rtv, sz) = match bound {
                    Some(k) => (targets[k].rtv.clone().expect("a retained target's view"), b.targets[k]),
                    None => (rtv.clone(), size),
                };
                // SAFETY: a view made on this device; the array outlives the call.
                unsafe {
                    // (the target read by an earlier batch is unbound from the pixel shader first)
                    gpu.ctx.PSSetShaderResources(0, Some(&[None, None, None, None, None, None, None, None]));
                    gpu.ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
                    if bound.is_some() {
                        gpu.ctx.ClearRenderTargetView(&rtv, &[0.0; 4]);
                    }
                }
                gpu.viewport([0, 0, sz[0], sz[1]]);
                self.write_projection(gpu, sz);
            }
            let mask = batch.mask.map(|k| targets[k].clone());
            self.draw_batch(gpu, assets, batch, mask)?;
        }
        if bound.is_some() {
            // SAFETY: a view made on this device.
            unsafe { gpu.ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None) };
            gpu.viewport([0, 0, size[0], size[1]]);
            self.write_projection(gpu, size);
        }
        for (_, e, clip) in blurs {
            self.run_blur(gpu, target, e, *clip)?;
        }
        Ok(())
    }

    /// FSlateRHIRenderingPolicy's view projection of a target of `size` pixels (CreateProjectionMatrix: left 0, top
    /// 0, near -100, far 100), in both vertex shaders' $Globals.
    fn write_projection(&self, gpu: &Gpu, size: [u32; 2]) {
        let (w, h) = (size[0] as f32, size[1] as f32);
        let projection: [[f32; 4]; 4] =
            [[2.0 / w, 0.0, 0.0, 0.0], [0.0, -2.0 / h, 0.0, 0.0], [0.0, 0.0, -1.0 / 200.0, 0.0], [-1.0, 1.0, 0.5, 1.0]];
        let rows = |r: &[[f32; 4]]| -> Vec<[u32; 4]> { r.iter().map(|x| x.map(f32::to_bits)).collect() };
        gpu.write_rows(&self.vs_globals, &rows(&projection));
        let mut mvs = vec![[0.0f32; 4]];
        mvs.extend_from_slice(&projection);
        gpu.write_rows(&self.material_vs_globals, &rows(&mvs));
    }

    /// A BackgroundBlur's pass over what the batches before it drew.
    fn run_blur(&mut self, gpu: &Gpu, target: &Texture, e: &BlurEl, clip: Option<[f64; 4]>) -> Result<()> {
        if self.blur.is_none() {
            self.blur = Some(BlurPass::new(gpu)?);
        }
        self.blur.as_mut().expect("made above").run(gpu, &self.raster, target, e, clip)
    }

    /// GammaAndAlphaValues: (2.2 / G, 1 / G, InvertAlpha, Contrast), G the display gamma, 1 for NoGamma
    /// (DrawElements 142566370: the batch flag 0x04 gives gamma 1, the colour written raw).
    fn gamma(effects: DrawEffects) -> [f32; 4] {
        if effects.has(DrawEffects::REVERSE_GAMMA) {
            mvs_sim::unported!("ESlateDrawEffect::ReverseGamma (DrawElements' batch flag 0x80)");
        }
        let g = if effects.has(DrawEffects::NO_GAMMA) { 1.0 } else { DISPLAY_GAMMA };
        let invert = if effects.has(DrawEffects::INVERT_ALPHA) { 1.0 } else { 0.0 };
        [2.2 / g, 1.0 / g, invert, CONTRAST]
    }

    /// `mask`: the retained target the batch's material reads as its Mask.
    fn draw_batch(&mut self, gpu: &Gpu, assets: &Assets, b: &Batch, mask: Option<Arc<Texture>>) -> Result<()> {
        let fx = b.effects;
        let disabled = fx.has(DrawEffects::DISABLED_EFFECT);
        // SAFETY: a rect on the bound target.
        unsafe { gpu.ctx.RSSetScissorRects(Some(&[b.scissor])) };
        match &b.shading {
            Shading::Element { ps, tex, wrap, params } => {
                let alpha = !fx.has(DrawEffects::IGNORE_TEXTURE_ALPHA);
                let ps = self.element_ps(gpu, assets, *ps, disabled, alpha)?;
                let blend = gpu.state(StateKey::Blend(slate_blend(
                    fx.has(DrawEffects::NO_BLENDING),
                    fx.has(DrawEffects::PRE_MULTIPLIED_ALPHA),
                )));
                let rows = [Self::gamma(fx), *params];
                gpu.write_rows(&self.ps_globals, &rows.map(|r| r.map(f32::to_bits)));
                let srv = match tex {
                    Tex::White => assets.white(mvs_material::material::TextureType::Texture2d).srv.clone(),
                    Tex::Path(p) => self
                        .picture(assets, p)?
                        .ok_or_else(|| Error::Format { what: p.clone(), why: "drawn before it loaded".into() })?
                        .srv
                        .clone(),
                    Tex::Atlas(k) => Some(self.atlases[0].1[*k as usize].srv.clone()),
                };
                let filter = match tex {
                    Tex::Path(p) => self.filters.get(p).copied().unwrap_or(Filter::Bilinear),
                    Tex::White | Tex::Atlas(_) => Filter::Bilinear,
                };
                let sampler = gpu.sampler(gpu.state(StateKey::Sampler(element_sampler(filter, *wrap))));
                gpu.set_shaders(self.vs, Some(ps));
                gpu.set_input_layout(self.vs, LAYOUT_KEY, &VERTEX_LAYOUT)?;
                gpu.set_states(blend, self.raster_id, self.depth, 0);
                // SAFETY: buffers, views and samplers made on this device; the arrays outlive the calls.
                unsafe {
                    gpu.ctx.RSSetState(&self.raster);
                    gpu.ctx.VSSetConstantBuffers(0, Some(&[Some(self.vs_globals.buf.clone())]));
                    gpu.ctx.PSSetConstantBuffers(0, Some(&[Some(self.ps_globals.buf.clone())]));
                    gpu.ctx.PSSetShaderResources(0, Some(&[srv]));
                    gpu.ctx.PSSetSamplers(0, Some(&[Some(sampler)]));
                    gpu.ctx.DrawIndexed(b.count, b.first, 0);
                }
            }
            Shading::Material { mid, ps, atlas } => {
                let id = self.material(gpu, assets, mid, mask)?;
                let m = self.materials.get_mut(id);
                let (vs, ps_id, vs_binds, ps_binds) = {
                    let p = m.bind_slate(gpu, assets, *ps, false)?;
                    (p.vs, p.ps.expect("a Slate pixel shader"), p.vs_binds.clone(), p.ps_binds.clone())
                };
                let blend = slate_material_blend(&m.material.state.blend, atlas.is_some())
                    .expect("bind_slate took the blend mode");
                let blend = gpu.state(StateKey::Blend(if fx.has(DrawEffects::NO_BLENDING) {
                    slate_blend(true, false)
                } else {
                    blend
                }));
                let flags = [if disabled { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0];
                let rows = [[0.0; 4], Self::gamma(fx), [0.0; 4], flags];
                gpu.write_rows(&self.material_ps_globals, &rows.map(|r| r.map(f32::to_bits)));
                gpu.set_shaders(vs, Some(ps_id));
                gpu.set_input_layout(vs, LAYOUT_KEY, &VERTEX_LAYOUT)?;
                gpu.set_states(blend, self.raster_id, self.depth, 0);
                let m = &self.materials.instances[id.0 as usize];
                let loose_srv = match atlas {
                    Some(k) => Some(self.atlases[0].1[*k as usize].srv.clone()),
                    None => assets.white(mvs_material::material::TextureType::Texture2d).srv.clone(),
                };
                let cb = |s: Source, globals: &Buffer| -> Option<ID3D11Buffer> {
                    match s {
                        Source::Globals => Some(globals.buf.clone()),
                        Source::View(_) => Some(self.view.buf.clone()),
                        Source::Material(_) => m.buffer.as_ref().map(|b| b.buf.clone()),
                        Source::Collection(k) => m.collections.get(k as usize).map(|b| b.buf.clone()),
                        _ => None,
                    }
                };
                let srv = |s: Source| -> Option<ID3D11ShaderResourceView> {
                    match s {
                        Source::Material(k) => match &m.resources[k as usize] {
                            MaterialResource::Texture(TextureSource::Asset(t)) => t.srv.clone(),
                            MaterialResource::Texture(TextureSource::Target(_)) => {
                                mvs_sim::unported!("a scene target in a UI material")
                            }
                            MaterialResource::Sampler(_) => None,
                        },
                        Source::Globals => loose_srv.clone(),
                        Source::View(k) => {
                            mvs_sim::unported!("a View texture in a UI material", VIEW_RESOURCES[k as usize])
                        }
                        _ => None,
                    }
                };
                let sampler = |s: Source| -> Option<ID3D11SamplerState> {
                    match s {
                        Source::Material(k) => match &m.resources[k as usize] {
                            MaterialResource::Sampler(id) => Some(gpu.sampler(*id)),
                            MaterialResource::Texture(_) => None,
                        },
                        Source::View(k) => {
                            Some(gpu.sampler(gpu.state(StateKey::Sampler(view_sampler(VIEW_RESOURCES[k as usize])))))
                        }
                        // The font atlas's sampler (bilinear, clamped).
                        Source::Globals => Some(gpu.sampler(self.clamp_sampler)),
                        _ => None,
                    }
                };
                let set = |binds: &StageBinds, globals: &Buffer, vertex: bool| {
                    for &(r, s) in &binds.cbs {
                        let c = [cb(s, globals)];
                        // SAFETY: a buffer made on this device.
                        unsafe {
                            if vertex {
                                gpu.ctx.VSSetConstantBuffers(r, Some(&c));
                            } else {
                                gpu.ctx.PSSetConstantBuffers(r, Some(&c));
                            }
                        }
                    }
                    for &(r, s) in &binds.srvs {
                        let v = [srv(s)];
                        // SAFETY: a view made on this device.
                        unsafe {
                            if vertex {
                                gpu.ctx.VSSetShaderResources(r, Some(&v));
                            } else {
                                gpu.ctx.PSSetShaderResources(r, Some(&v));
                            }
                        }
                    }
                    for &(r, s) in &binds.samplers {
                        let v = [sampler(s)];
                        // SAFETY: a sampler made on this device.
                        unsafe {
                            if vertex {
                                gpu.ctx.VSSetSamplers(r, Some(&v));
                            } else {
                                gpu.ctx.PSSetSamplers(r, Some(&v));
                            }
                        }
                    }
                };
                set(&vs_binds, &self.material_vs_globals, true);
                set(&ps_binds, &self.material_ps_globals, false);
                // SAFETY: a state made on this device; the draw reads the buffers bound in `draw`.
                unsafe {
                    gpu.ctx.RSSetState(&self.raster);
                    gpu.ctx.DrawIndexed(b.count, b.first, 0);
                }
            }
        }
        Ok(())
    }

    /// One element's vertices and batches.
    fn build(&mut self, assets: &Assets, fonts: &Fonts, p: &Placed, size: [u32; 2], b: &mut Builder) -> Result<()> {
        let scissor = scissor_of(p.clip, size);
        if scissor.right <= scissor.left || scissor.bottom <= scissor.top {
            return Ok(());
        }
        let fx = p.effects;
        b.zone = p.zone.clone();
        match &p.element {
            Element::Box(e) => {
                let g = &e.geometry;
                let (tex, tex_size) = match &e.texture {
                    None => (Tex::White, [1.0, 1.0]),
                    // A brush naming a texture the build does not hold (UI_IconButton's default icon_128_15, cooked
                    // out) draws nothing, as for any texture it has no image of.
                    Some(path)
                        if !self.pictures.contains_key(path)
                            && !mvs_render::port_icons::is_port_icon(path)
                            && !["texture", "image"]
                                .iter()
                                .any(|k| assets.store().kind(k).is_ok_and(|p| p.has(path))) =>
                    {
                        return Ok(());
                    }
                    Some(path) => {
                        let Some(t) = self.picture(assets, path)? else {
                            self.waiting += 1;
                            return Ok(());
                        };
                        (Tex::Path(path.clone()), [t.desc.size[0] as f64, t.desc.size[1] as f64])
                    }
                };
                // FSlateElementBatcher::AddBoxElement: a tiled axis repeats the texture every texture size, the
                // margins are fractions of the texture's size.
                let tiling = [
                    if e.tile[0] { g.size[0] / tex_size[0] } else { 1.0 },
                    if e.tile[1] { g.size[1] / tex_size[1] } else { 1.0 },
                ];
                let margin =
                    e.margin.map(|m| [m.l * tex_size[0], m.t * tex_size[1], m.r * tex_size[0], m.b * tex_size[1]]);
                let shading = Shading::Element { ps: ElementPs::Default, tex, wrap: e.tile, params: [0.0; 4] };
                b.boxed(
                    &shading,
                    scissor,
                    fx,
                    &g.transform,
                    g.size,
                    margin,
                    e.margin,
                    e.uv,
                    tiling,
                    vertex_color(e.tint),
                );
            }
            Element::Material(e) => {
                if !self.mid_ready(assets, &e.material)? {
                    self.waiting += 1;
                    return Ok(());
                }
                let g = &e.geometry;
                // The margins are fractions of the brush's ImageSize.
                let margin = e.margin.map(|m| [m.l * e.image[0], m.t * e.image[1], m.r * e.image[0], m.b * e.image[1]]);
                let shading =
                    Shading::Material { mid: e.material.clone(), ps: SlateMaterialShader::Default, atlas: None };
                b.boxed(
                    &shading,
                    scissor,
                    fx,
                    &g.transform,
                    g.size,
                    margin,
                    e.margin,
                    [0.0, 0.0, 1.0, 1.0],
                    [1.0, 1.0],
                    vertex_color(e.tint),
                );
            }
            Element::Text(e) => {
                if let Some(mid) = &e.material
                    && !self.mid_ready(assets, mid)?
                {
                    self.waiting += 1;
                    return Ok(());
                }
                self.build_text(fonts, e, scissor, fx, b);
            }
            Element::Line(e) => {
                let t = &e.geometry.transform;
                let s = draw_scale(t);
                // FSlateElementBatcher::AddLineElement's anti-aliased quads: each segment widened to half the
                // thickness plus the filter on each side, TexCoords.x 0 -> 1 across it; the LineSegment shader
                // fades the edges by (thickness, filter).
                // Unverified: 4.26's builder (its joins between segments are not made: each segment is its own quad).
                let thick = (e.thickness * 0.5 * (s[0] + s[1])).max(1.0);
                let half = 0.5 * thick + LINE_FILTER;
                let shading = Shading::Element {
                    ps: ElementPs::LineSegment,
                    tex: Tex::White,
                    wrap: [false; 2],
                    params: [thick as f32, LINE_FILTER as f32, 0.0, 0.0],
                };
                let color = vertex_color(e.color);
                let pts: Vec<[f32; 2]> = e.points.iter().map(|q| place(t, q[0], q[1])).collect();
                for seg in pts.windows(2) {
                    let (p0, p1) = (seg[0], seg[1]);
                    let d = [(p1[0] - p0[0]) as f64, (p1[1] - p0[1]) as f64];
                    let len = d[0].hypot(d[1]);
                    if len == 0.0 {
                        continue;
                    }
                    let n = [(-d[1] / len * half) as f32, (d[0] / len * half) as f32];
                    let v = |p: [f32; 2], side: f32, u: f32| Vertex {
                        tex: [u, 0.0, 0.0, 0.0],
                        mat: [0.0; 2],
                        pos: [p[0] + n[0] * side, p[1] + n[1] * side],
                        color,
                        pixel: [0; 2],
                    };
                    b.quad(
                        &shading,
                        scissor,
                        fx,
                        [v(p0, -1.0, 0.0), v(p1, -1.0, 0.0), v(p0, 1.0, 1.0), v(p1, 1.0, 1.0)],
                    );
                }
            }
            Element::Retained(e) => {
                // SRetainerWidget with an effect material (OnPaint 1425e2190): the content drawn into a target of the
                // widget's pixel size under its render scale about the centre, then a box with the effect material
                // over the widget's geometry, its Mask the target, with the element's draw effects
                // (PreMultipliedAlpha | NoGamma) and the tint premultiplied (rgb x a, a x a).
                // Unverified: a material batch blends by the material's blend mode whatever PreMultipliedAlpha.
                // (both asked of the loaders at once)
                let (content, effect) =
                    (self.mid_ready(assets, &e.content.material)?, self.mid_ready(assets, &e.effect)?);
                if !(content && effect) {
                    self.waiting += 1;
                    return Ok(());
                }
                let g = &e.geometry;
                let s = draw_scale(&g.transform);
                let px = |v: f64| (v.ceil().max(1.0) as u32).min(RETAINED_MAX);
                let (pw, ph) = (px(g.size[0] * s[0]), px(g.size[1] * s[1]));
                let k = b.targets.len();
                b.targets.push([pw, ph]);
                let [sx, sy] = e.content_scale;
                let (fw, fh) = (f64::from(pw), f64::from(ph));
                let (cw, ch) = (g.size[0] * sx, g.size[1] * sy);
                let t = [
                    if cw != 0.0 { fw * sx / cw } else { 0.0 },
                    0.0,
                    0.0,
                    if ch != 0.0 { fh * sy / ch } else { 0.0 },
                    fw * (1.0 - sx) / 2.0,
                    fh * (1.0 - sy) / 2.0,
                ];
                let c = &e.content;
                let whole = scissor_of(None, [pw, ph]);
                b.target = Some(k);
                // (the content's target is the widget's own: the zone is in the frame's pixels)
                let zone = b.zone.take();
                let shading =
                    Shading::Material { mid: c.material.clone(), ps: SlateMaterialShader::Default, atlas: None };
                let unit = [0.0, 0.0, 1.0, 1.0];
                let color = vertex_color(c.tint);
                b.boxed(&shading, whole, DrawEffects::NONE, &t, [cw, ch], None, None, unit, [1.0, 1.0], color);
                b.target = None;
                b.zone = zone;
                b.mask = Some(k);
                let shading =
                    Shading::Material { mid: e.effect.clone(), ps: SlateMaterialShader::Default, atlas: None };
                let a = e.tint[3];
                let tint = vertex_color([e.tint[0] * a, e.tint[1] * a, e.tint[2] * a, a * a]);
                b.boxed(&shading, scissor, fx, &g.transform, g.size, None, None, unit, [1.0, 1.0], tint);
                b.mask = None;
            }
            Element::Blur(e) => {
                b.blurs.push((b.batches.len(), e.clone(), p.clip));
                b.fence = b.batches.len();
            }
        }
        Ok(())
    }

    /// A text run's glyph quads (FSlateElementBatcher::BuildShapedTextSequence): the outline glyphs by the
    /// GrayscaleFont element shader in the outline colour, the fill glyphs by it or by the font material's
    /// GrayscaleFont pass, a batch per atlas page.
    fn build_text(&self, fonts: &Fonts, e: &TextEl, scissor: RECT, fx: DrawEffects, b: &mut Builder) {
        let t = &e.geometry.transform;
        let [aw, ah] = fonts.atlas().size.map(|v| v as f64);
        // A font material's TexCoords z / w: U runs glyph index / glyph count -> (index + 1) / count across a glyph
        // of its line, V the quad's top / bottom from the text's top over the font's height.
        // Unverified: the rule is 4.26's BuildShapedTextSequence; the font's
        // height is taken as the element's height over its lines.
        let lines = e.run.glyphs.iter().map(|g| g.line + 1).max().unwrap_or(1) as usize;
        let mut count = vec![0u32; lines];
        // (an emoji's silhouette is a shadow's: `GlyphPart`)
        let glyphs = || e.run.glyphs.iter().filter(|g| g.part != GlyphPart::Silhouette);
        for g in glyphs().filter(|g| !g.outline) {
            count[g.line as usize] += 1;
        }
        let height = (e.geometry.size[1] / lines as f64).max(1.0);
        let mut index = vec![0u32; lines];
        // (the font material's shading, its atlas page set per glyph)
        let mut font_shading = e.material.as_ref().map(|mid| Shading::Material {
            mid: mid.clone(),
            ps: SlateMaterialShader::GrayscaleFont,
            atlas: None,
        });
        for g in glyphs() {
            let entry = fonts.glyph(g.atlas);
            let k = if g.outline {
                0
            } else {
                let k = index[g.line as usize];
                index[g.line as usize] += 1;
                k
            };
            let [rx, ry, rw, rh] = entry.rect.map(f64::from);
            if rw == 0.0 || rh == 0.0 {
                continue;
            }
            let x0 = g.x + f64::from(entry.offset[0]);
            let y0 = g.y - f64::from(entry.offset[1]);
            let (u0, v0, u1, v1) = (rx / aw, ry / ah, (rx + rw) / aw, (ry + rh) / ah);
            // a glyph of its own colour (an emoji's layer) is not the font material's
            let material = e.material.as_ref().filter(|_| !g.outline && g.part != GlyphPart::Layer);
            let (ut, utm, vt, vtm) = match material {
                Some(_) => {
                    let n = f64::from(count[g.line as usize].max(1));
                    (f64::from(k) / n, f64::from(k + 1) / n, y0 / height, (y0 + rh) / height)
                }
                None => (1.0, 1.0, 1.0, 1.0),
            };
            let color = match g.color.filter(|_| !g.outline && g.part == GlyphPart::Layer) {
                // sRGB already: B, G, R and the layer's alpha times the text's
                Some([r, gr, bl, al]) => {
                    let a = (f64::from(al) / 255.0 * e.color[3]) as f32;
                    [bl, gr, r, (a.clamp(0.0, 1.0) * 255.999).floor() as u8]
                }
                None => vertex_color(if g.outline { e.outline_color } else { e.color }),
            };
            let v = |x: f64, y: f64, u: f64, vv: f64, ux: f64, vy: f64, mx: f32, my: f32| Vertex {
                tex: [u as f32, vv as f32, ux as f32, vy as f32],
                mat: [mx, my],
                pos: place(t, x, y),
                color,
                pixel: [0; 2],
            };
            let quad = [
                v(x0, y0, u0, v0, ut, vt, 0.0, 0.0),
                v(x0 + rw, y0, u1, v0, utm, vt, 1.0, 0.0),
                v(x0, y0 + rh, u0, v1, ut, vtm, 0.0, 1.0),
                v(x0 + rw, y0 + rh, u1, v1, utm, vtm, 1.0, 1.0),
            ];
            match (material, &mut font_shading) {
                (Some(_), Some(shading)) => {
                    if let Shading::Material { atlas, .. } = shading {
                        *atlas = Some(entry.page);
                    }
                    b.quad(shading, scissor, fx, quad);
                }
                _ => {
                    let shading = Shading::Element {
                        ps: ElementPs::GrayscaleFont,
                        tex: Tex::Atlas(entry.page),
                        wrap: [false; 2],
                        params: [0.0; 4],
                    };
                    b.quad(&shading, scissor, fx, quad);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_color_is_srgb_bgra() {
        assert_eq!(vertex_color([1.0, 0.0, 0.0, 1.0]), [0, 0, 255, 255]);
        assert_eq!(vertex_color([0.5, 0.5, 0.5, 0.5]), [188, 188, 188, 127]);
        assert_eq!(vertex_color([2.0, -1.0, 0.0, 3.0]), [0, 0, 255, 255]);
    }

    #[test]
    fn box_margins_meet_in_the_middle_when_too_wide() {
        assert_eq!(stops(100.0, 10.0, 20.0), [0.0, 10.0, 80.0, 100.0]);
        assert_eq!(stops(20.0, 15.0, 15.0), [0.0, 10.0, 10.0, 20.0]);
    }

    #[test]
    fn scissor_rounds_and_clamps_to_the_target() {
        let r = scissor_of(Some([-5.0, 10.4, 2000.0, 10.6]), [1920, 1080]);
        assert_eq!((r.left, r.top, r.right, r.bottom), (0, 10, 1920, 11));
        let r = scissor_of(None, [640, 360]);
        assert_eq!((r.left, r.top, r.right, r.bottom), (0, 0, 640, 360));
    }

    #[test]
    fn a_quad_under_a_sheared_zone_keeps_only_the_zone() {
        // A 100 x 100 box under a parallelogram leaning right by 20 px over its height: the clipped quad covers the
        // parallelogram's area inside the box, its corners' UVs where the cut lands.
        let zone: Zone = vec![[20.0, 0.0], [120.0, 0.0], [100.0, 100.0], [0.0, 100.0]].into();
        let mut b = Builder { zone: Some(zone), ..Builder::default() };
        let shading = Shading::Element { ps: ElementPs::Default, tex: Tex::White, wrap: [false; 2], params: [0.0; 4] };
        let rect = RECT { left: 0, top: 0, right: 200, bottom: 200 };
        let t = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        b.boxed(
            &shading,
            rect,
            DrawEffects::NONE,
            &t,
            [100.0, 100.0],
            None,
            None,
            [0.0, 0.0, 1.0, 1.0],
            [1.0; 2],
            [255; 4],
        );
        let area: f64 = b
            .idx
            .chunks(3)
            .map(|k| {
                let p: [[f64; 2]; 3] = std::array::from_fn(|j| b.verts[k[j] as usize].pos.map(f64::from));
                ((p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1])).abs() / 2.0
            })
            .sum();
        // (the box less the zone's left corner triangle, 20 x 100 / 2)
        assert!((area - 9000.0).abs() < 1e-3, "{area}");
        assert_eq!(b.batches.len(), 1);
        assert_eq!(b.batches[0].count as usize, b.idx.len());
        for v in &b.verts {
            assert!((f64::from(v.tex[0]) - f64::from(v.pos[0]) / 100.0).abs() < 1e-6);
            assert!((f64::from(v.tex[1]) - f64::from(v.pos[1]) / 100.0).abs() < 1e-6);
        }
        // A zone apart from the quad draws nothing.
        let mut b =
            Builder { zone: Some(vec![[300.0, 0.0], [400.0, 0.0], [400.0, 10.0]].into()), ..Builder::default() };
        b.boxed(
            &shading,
            rect,
            DrawEffects::NONE,
            &t,
            [100.0, 100.0],
            None,
            None,
            [0.0, 0.0, 1.0, 1.0],
            [1.0; 2],
            [255; 4],
        );
        assert!(b.idx.is_empty() && b.batches.is_empty());
    }
}
