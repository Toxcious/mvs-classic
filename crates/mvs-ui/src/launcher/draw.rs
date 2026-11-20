//! The launcher's drawing kit over the painter: the page's 1100 x 660 CSS pixels scaled to the window (the page's
//! short side fits it, its long side grows: the parts anchored right or bottom stay at their edges), the page's
//! colours and gradients, text in a line box, paragraphs, buttons and the pointer's hover and click.

use mvs_render::gpu::Gpu;

use super::fonts::{Face, Fonts};
use super::paint::{CLEAR, Fill, Kind, Painter, Rgba, Shape, hex, rgba};

/// The page's size, CSS pixels.
pub const PAGE: [f32; 2] = [1100.0, 660.0];

/// The page's colours.
pub const INK: Rgba = hex(0x071030, 1.0);
pub const ORANGE: Rgba = hex(0xe9a84a, 1.0);
pub const ORANGE_2: Rgba = hex(0x7d4a17, 1.0);
pub const CYAN: Rgba = hex(0xa9c6ff, 1.0);
pub const GOLD: Rgba = hex(0xf4cf7a, 1.0);
pub const GOOD: Rgba = hex(0x63d6a0, 1.0);
pub const WARN: Rgba = hex(0xf2bd5c, 1.0);
pub const BAD: Rgba = hex(0xf06478, 1.0);
pub const WHITE: Rgba = [1.0; 4];
// Deviation: brighter than the page's 0.66 and 0.42, so small text reads on the night sky.
pub const DIM: Rgba = rgba(222, 232, 255, 0.82);
pub const FAINT: Rgba = rgba(200, 215, 250, 0.6);
pub const PANEL: Rgba = rgba(8, 22, 60, 0.55);
pub const PANEL_SOLID: Rgba = hex(0x0b1a48, 1.0);
pub const LINE: Rgba = rgba(150, 190, 240, 0.12);

/// The pointer this frame, page pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pointer {
    pub at: Option<[f32; 2]>,
    /// The left button is down.
    pub down: bool,
    /// The left button went up this frame where it went down (a click), at `at`.
    pub click: bool,
    /// Wheel notches this frame (up positive).
    pub wheel: f32,
}

/// A frame's drawing on the page.
pub struct Canvas<'a> {
    pub gpu: &'a Gpu,
    pub p: &'a mut Painter,
    pub fonts: &'a mut Fonts,
    /// Window pixels per page pixel.
    pub s: f32,
    /// The page's size at this window, page pixels (one side PAGE's, the other at least PAGE's).
    pub size: [f32; 2],
    pub pointer: Pointer,
    /// The page's clock, s.
    pub t: f64,
    /// A part over the page takes the pointer: the page under it sees none.
    pub blocked: bool,
    /// The transform the shapes that follow take (a 2x2 about an origin, page pixels).
    pub xf: Option<([f32; 4], [f32; 2])>,
}

/// The page's size and scale for a window of `w` x `h` pixels.
pub fn fit(window: [u32; 2]) -> ([f32; 2], f32) {
    let (w, h) = (window[0].max(1) as f32, window[1].max(1) as f32);
    let s = (w / PAGE[0]).min(h / PAGE[1]);
    ([w / s, h / s], s)
}

impl Canvas<'_> {
    /// A shape in page pixels, scaled to the window and added.
    pub fn push(&mut self, mut sh: Shape) {
        let s = self.s;
        if let Some((m, o)) = self.xf
            && sh.xform == [1.0, 0.0, 0.0, 1.0]
        {
            sh.xform = m;
            sh.origin = o;
        }
        sh.rect = sh.rect.map(|v| v * s);
        sh.radius *= s;
        sh.border *= s;
        sh.soft = (sh.soft * s).max(1.0);
        sh.origin = sh.origin.map(|v| v * s);
        if sh.kind == Kind::Segment {
            sh.p = sh.p.map(|v| v * s);
        }
        if sh.clip[0] > -1e5 {
            sh.clip = sh.clip.map(|v| v * s);
        }
        sh.fill = match sh.fill {
            Fill::Linear { grad, from, to, repeat, shift } => {
                Fill::Linear { grad, from: from.map(|v| v * s), to: to.map(|v| v * s), repeat, shift }
            }
            Fill::Radial { grad, centre, radii } => {
                Fill::Radial { grad, centre: centre.map(|v| v * s), radii: radii.map(|v| v * s) }
            }
            f => f,
        };
        self.p.push(sh);
    }

    /// The clip of the shapes that follow (page pixels; None: none).
    pub fn clip(&mut self, r: Option<[f32; 4]>) {
        self.p.clip = match r {
            Some([x, y, w, h]) => [x * self.s, y * self.s, (x + w) * self.s, (y + h) * self.s],
            None => [-1e6, -1e6, 1e6, 1e6],
        };
    }

    /// A gradient along `from` to `to` (page pixels).
    pub fn linear(&mut self, stops: &[(f32, Rgba)], from: [f32; 2], to: [f32; 2]) -> Fill {
        Fill::Linear { grad: self.p.gradient(stops), from, to, repeat: false, shift: 0.0 }
    }

    /// A vertical gradient over `r` (CSS linear-gradient(180deg, ...)).
    pub fn down(&mut self, stops: &[(f32, Rgba)], r: [f32; 4]) -> Fill {
        self.linear(stops, [r[0], r[1]], [r[0], r[1] + r[3]])
    }

    /// A radial gradient (CSS radial-gradient(rx ry at cx cy, ...)).
    pub fn radial(&mut self, stops: &[(f32, Rgba)], centre: [f32; 2], radii: [f32; 2]) -> Fill {
        Fill::Radial { grad: self.p.gradient(stops), centre, radii }
    }

    /// A rounded rectangle.
    pub fn rect(&mut self, r: [f32; 4], fill: Fill, radius: f32) {
        let mut s = Shape::new(Kind::Rect, r, fill);
        s.radius = radius;
        self.push(s);
    }

    pub fn solid(&mut self, r: [f32; 4], c: Rgba, radius: f32) {
        self.rect(r, Fill::Solid(c), radius);
    }

    /// A rounded rectangle with its border (CSS border inside the box).
    pub fn framed(&mut self, r: [f32; 4], fill: Fill, border: f32, border_color: Rgba, radius: f32) {
        let mut s = Shape::new(Kind::Rect, r, fill);
        s.radius = radius;
        s.border = border;
        s.border_color = border_color;
        self.push(s);
    }

    /// The picture of `slot` over `r`, scaled to cover it about its middle (CSS object-fit: cover), `zoom` times
    /// closer, its corners rounded; `fade`: its bottom fades out between these two parts of `r`'s height. False
    /// when the slot holds no picture: nothing drawn.
    pub fn picture(
        &mut self,
        slot: u32,
        r: [f32; 4],
        radius: f32,
        zoom: f32,
        opacity: f32,
        fade: Option<[f32; 2]>,
    ) -> bool {
        let Some([tw, th]) = self.p.picture_size(slot) else { return false };
        let (tw, th) = (tw as f32, th as f32);
        // (a box wider than the picture shows its whole width and loses rows; a taller one loses columns)
        let k = (r[2] / r[3]) / (tw / th);
        let (uw, uh) = if k >= 1.0 { (1.0 / zoom, 1.0 / k / zoom) } else { (k / zoom, 1.0 / zoom) };
        let (u0, v0) = ((1.0 - uw) * 0.5, (1.0 - uh) * 0.5);
        let mut s = Shape::new(Kind::Image, r, Fill::Solid(CLEAR));
        s.picture = slot;
        s.radius = radius;
        s.opacity = opacity;
        s.uv = [u0, v0, u0 + uw, v0 + uh];
        // (no fade on the left; none at the bottom unless asked: the shader's ramps lie outside 0..1)
        let bottom = fade.map_or([2.0, 3.0], |[a, b]| [v0 + a * uh, v0 + b * uh]);
        s.fade = [-1.0, -0.5, bottom[0], bottom[1]];
        // The texels a window pixel covers: two to a tap of the box filter (the sampler blends neighbours).
        let texels = (uw * tw / (r[2] * self.s)).max(uh * th / (r[3] * self.s));
        let taps = (texels * 0.5).ceil().clamp(1.0, 4.0);
        s.p = [f32::from(u8::from(self.p.picture_srgb(slot))), taps, 0.0, 0.0];
        self.push(s);
        true
    }

    /// A blurred rectangle (CSS box-shadow with blur `blur` and spread 0, or filter: blur on a box).
    // Deviation: the blur is the rectangle's soft edge `blur` wide (a linear ramp), not a Gaussian.
    pub fn shadow(&mut self, r: [f32; 4], c: Rgba, radius: f32, blur: f32) {
        let mut s = Shape::new(Kind::Rect, r, Fill::Solid(c));
        s.radius = radius;
        s.soft = blur.max(1.0);
        self.push(s);
    }

    /// A one-pixel rule.
    pub fn hline(&mut self, x: f32, y: f32, w: f32, c: Rgba) {
        self.solid([x, y, w, 1.0], c, 0.0);
    }

    /// A stroke (an icon's line), round capped.
    pub fn seg(&mut self, a: [f32; 2], b: [f32; 2], width: f32, c: Rgba) {
        let mut s = Shape::new(Kind::Segment, [0.0; 4], Fill::Solid(c));
        s.p = [a[0], a[1], b[0], b[1]];
        s.border = width;
        self.push(s);
    }

    /// A polyline of strokes.
    pub fn path(&mut self, pts: &[[f32; 2]], width: f32, c: Rgba) {
        for w in pts.windows(2) {
            self.seg(w[0], w[1], width, c);
        }
    }

    /// A circle's ring (`gap`: start and length of a missing arc, radians).
    pub fn ring(&mut self, centre: [f32; 2], r: f32, width: f32, c: Rgba, gap: Option<[f32; 2]>) {
        let mut s = Shape::new(Kind::Rect, [centre[0] - r, centre[1] - r, 2.0 * r, 2.0 * r], Fill::Solid([0.0; 4]));
        s.radius = r;
        s.border = width;
        s.border_color = c;
        if let Some([a, l]) = gap {
            s.p = [0.0, 0.0, a, l];
        }
        self.push(s);
    }

    pub fn dot(&mut self, centre: [f32; 2], r: f32, c: Rgba) {
        self.solid([centre[0] - r, centre[1] - r, 2.0 * r, 2.0 * r], c, r);
    }

    /// A line of text with its line box's top at `y`, its pen at `x`; its width. `fill` its colour or gradient;
    /// `xform` a transform about `origin` (page pixels).
    #[allow(clippy::too_many_arguments)]
    pub fn text_with(
        &mut self,
        f: Face,
        px: f32,
        spacing: f32,
        x: f32,
        y: f32,
        line_h: f32,
        text: &str,
        fill: Fill,
        xform: Option<([f32; 4], [f32; 2])>,
        opacity: f32,
    ) -> f32 {
        let s = self.s;
        let base = (y + self.fonts.baseline(f, px, line_h)) * s;
        let glyphs = self.fonts.layout(self.gpu, self.p, f, px * s, spacing * s, text);
        let x0 = (x * s).round();
        let base = base.round();
        for g in glyphs {
            let mut sh = Shape::new(
                Kind::Glyph,
                [(x0 + g.rect[0]) / s, (base + g.rect[1]) / s, g.rect[2] / s, g.rect[3] / s],
                fill,
            );
            sh.uv = g.uv;
            sh.opacity = opacity;
            if let Some((m, o)) = xform {
                sh.xform = m;
                sh.origin = o;
            }
            self.push(sh);
        }
        self.width(f, px, spacing, text)
    }

    /// A line of text in `c`; its width.
    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, f: Face, px: f32, x: f32, y: f32, line_h: f32, text: &str, c: Rgba) -> f32 {
        self.text_with(f, px, 0.0, x, y, line_h, text, Fill::Solid(c), None, 1.0)
    }

    /// The width of a line, page pixels.
    pub fn width(&mut self, f: Face, px: f32, spacing: f32, text: &str) -> f32 {
        self.fonts.width(f, px * self.s, spacing * self.s, text) / self.s
    }

    /// A line cut with an ellipsis to `max`; its width.
    #[allow(clippy::too_many_arguments)]
    pub fn text_cut(&mut self, f: Face, px: f32, x: f32, y: f32, line_h: f32, text: &str, c: Rgba, max: f32) -> f32 {
        let cut = self.fonts.ellipsis(f, px * self.s, 0.0, text, max * self.s);
        self.text(f, px, x, y, line_h, &cut, c)
    }

    /// A paragraph wrapped to `w`; its height.
    #[allow(clippy::too_many_arguments)]
    pub fn para(&mut self, f: Face, px: f32, line_h: f32, x: f32, y: f32, w: f32, text: &str, c: Rgba) -> f32 {
        let lines = self.lines(f, px, w, text);
        for (k, l) in lines.iter().enumerate() {
            self.text(f, px, x, y + k as f32 * line_h, line_h, l, c);
        }
        lines.len() as f32 * line_h
    }

    /// A paragraph's lines at `w`.
    pub fn lines(&mut self, f: Face, px: f32, w: f32, text: &str) -> Vec<String> {
        self.fonts.wrap(f, px * self.s, text, w * self.s)
    }

    /// The pointer is over `r` (and no part over the page takes it).
    pub fn hover(&self, r: [f32; 4]) -> bool {
        !self.blocked
            && self.pointer.at.is_some_and(|[x, y]| x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3])
    }

    /// `r` was clicked this frame.
    pub fn clicked(&self, r: [f32; 4]) -> bool {
        self.pointer.click && self.hover(r)
    }
}

/// A button's look (styles.css .btn and its variants).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Plain,
    Ghost,
    Primary,
}

impl Canvas<'_> {
    /// The width of a button of `label` (`small`: .btn.small).
    pub fn btn_width(&mut self, label: &str, small: bool, look: Btn) -> f32 {
        let face = if look == Btn::Primary { Face::Bold } else { Face::Semi };
        let px = if small { 14.0 } else { 14.5 };
        self.width(face, px, 0.0, label) + if small { 24.0 } else { 32.0 }
    }

    /// A button at `x`, `y`; whether it was clicked (never when `disabled`), and its width.
    pub fn button(&mut self, x: f32, y: f32, label: &str, small: bool, look: Btn, disabled: bool) -> (bool, f32) {
        let w = self.btn_width(label, small, look);
        let h = if small { 30.0 } else { 38.0 };
        let r = [x, y, w, h];
        let hover = !disabled && self.hover(r);
        let op = if disabled { 0.45 } else { 1.0 };
        let (face, px) = (if look == Btn::Primary { Face::Bold } else { Face::Semi }, if small { 14.0 } else { 14.5 });
        let lift = if look == Btn::Primary && hover { -1.0 } else { 0.0 };
        let r = [x, y + lift, w, h];
        match look {
            Btn::Plain => {
                let bg = rgba(255, 255, 255, if hover { 0.13 } else { 0.07 });
                let mut s = Shape::new(Kind::Rect, r, Fill::Solid(bg));
                s.radius = 10.0;
                s.border = 1.0;
                s.border_color = rgba(255, 255, 255, 0.14);
                s.opacity = op;
                self.push(s);
            }
            Btn::Ghost => {
                if hover {
                    self.solid(r, rgba(255, 255, 255, 0.06), 10.0);
                }
            }
            Btn::Primary => {
                // box-shadow: 0 8px 22px rgba(233,168,74,.2), 0 4px 0 #7d4a17
                let mut glow = Shape::new(Kind::Rect, [x, y + 8.0 + lift, w, h], Fill::Solid(rgba(233, 168, 74, 0.2)));
                glow.radius = 10.0;
                glow.soft = 22.0;
                glow.opacity = op;
                self.push(glow);
                let mut ledge = Shape::new(Kind::Rect, [x, y + 4.0 + lift, w, h], Fill::Solid(ORANGE_2));
                ledge.radius = 10.0;
                ledge.opacity = op;
                self.push(ledge);
                let stops = if hover {
                    [(0.0, hex(0xf4b658, 1.0)), (1.0, hex(0xdf8d32, 1.0))]
                } else {
                    [(0.0, hex(0xf0ad4c, 1.0)), (1.0, hex(0xd9862c, 1.0))]
                };
                let fill = self.down(&stops, r);
                let mut s = Shape::new(Kind::Rect, r, fill);
                s.radius = 10.0;
                s.border = 1.0;
                s.border_color = rgba(255, 225, 180, 0.3);
                s.opacity = op;
                self.push(s);
            }
        }
        let color = match look {
            Btn::Ghost if !hover => DIM,
            _ => WHITE,
        };
        let tx = x + if small { 12.0 } else { 16.0 };
        if look == Btn::Primary {
            // text-shadow: 0 1px 2px rgba(100,52,12,.8)
            self.text_with(face, px, 0.0, tx, r[1] + 1.0, h, label, Fill::Solid(rgba(100, 52, 12, 0.8)), None, op);
        }
        self.text_with(face, px, 0.0, tx, r[1], h, label, Fill::Solid(color), None, op);
        (hover && self.pointer.click, w)
    }
}
