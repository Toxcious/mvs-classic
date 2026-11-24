//! (the port's own) Lines, fills and labels at places of the play space, projected with a frame's camera onto an
//! element list: what the Lab's shapes overlay and the stage editor draw the sim's shapes with. The places are the
//! sim's (x across, y up, depth), in UE cm.
// Deviation: the shapes are elements of the UI's list: lines of the UI renderer, fills as one-pixel rows blended in
// its linear colour, the labels in the font's default typeface (not a 2D canvas of their own with fills blended in
// sRGB and a monospace font).

use mvs_view::Camera;

use crate::element::{BoxEl, DrawEffects, Element, ElementList, LineEl, PaintGeometry, TextEl};
use crate::menu::screens::lab_tools::linear;
use crate::text::{FontInfo, Fonts};

/// A colour (a CSS hex colour's bytes).
pub type Hex = [u8; 3];

/// The lines' width, px (the canvas's lineWidth), the fills' alpha, the dashes (on, off), px.
const LINE: f64 = 1.5;
pub const FILL_ALPHA: f64 = 0.22;
const DASH: [f64; 2] = [5.0, 4.0];
/// The labels: the font's px size ('11px'), the line step, their outline ('#000c', lineWidth 3).
const LABEL_PX: f64 = 11.0;
pub const LABEL_STEP: f64 = 13.0;
const LABEL_OUTLINE: f64 = 1.5;
const LABEL_OUTLINE_ALPHA: f64 = 204.0 / 255.0;

/// What a frame's shapes draw on.
pub struct Canvas<'a> {
    pub camera: &'a Camera,
    pub size: [u32; 2],
    pub list: &'a mut ElementList,
    pub fonts: &'a mut Fonts,
    pub layer: i32,
}

impl Canvas<'_> {
    /// sim (x, y, depth) in UE cm -> pixels (the HUD's anchor projection, unrounded). None behind the camera.
    pub fn to(&self, x: f64, y: f64, z: f64) -> Option<[f64; 2]> {
        let cam = self.camera;
        let ue = [z, -x, y];
        let rel = [ue[0] - cam.origin[0], ue[1] - cam.origin[1], ue[2] - cam.origin[2]];
        let row = |v: [f64; 4], m: &mvs_view::M4| {
            let mut o = [0.0; 4];
            for (j, oj) in o.iter_mut().enumerate() {
                *oj = v[0] * m[0][j] + v[1] * m[1][j] + v[2] * m[2][j] + v[3] * m[3][j];
            }
            o
        };
        let view = row([rel[0], rel[1], rel[2], 1.0], &cam.translated_world_to_view);
        let clip = row(view, &cam.view_to_clip);
        if clip[3] <= 0.0 {
            return None;
        }
        let (nx, ny) = (clip[0] / clip[3], clip[1] / clip[3]);
        let [w, h] = self.size.map(f64::from);
        Some([((nx + 1.0) / 2.0) * w, ((1.0 - ny) / 2.0) * h])
    }

    /// A length at a place, in pixels.
    pub fn px(&self, x: f64, y: f64, z: f64, r: f64) -> f64 {
        let Some(t) = self.to(x, y + r, z) else { return 0.0 };
        self.to(x, y, z).map_or(0.0, |p| (p[1] - t[1]).abs())
    }

    /// A hex colour at `a` alpha, linear.
    pub fn color(c: Hex, a: f64) -> [f64; 4] {
        let l = linear(c);
        [l[0], l[1], l[2], a]
    }

    /// A polyline of pixel points.
    pub fn line(&mut self, pts: Vec<[f64; 2]>, c: [f64; 4]) {
        if pts.len() < 2 {
            return;
        }
        let geometry = PaintGeometry { size: [0.0, 0.0], transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] };
        self.list.push(
            self.layer,
            None,
            DrawEffects::NONE,
            Element::Line(LineEl { geometry, points: pts, color: c, thickness: LINE }),
        );
    }

    /// A path stroked, solid or dashed (the dash pattern from the path's start).
    pub fn stroke(&mut self, pts: &[[f64; 2]], col: Hex, alpha: f64, dash: bool) {
        let c = Self::color(col, alpha);
        if !dash {
            self.line(pts.to_vec(), c);
            return;
        }
        let mut on = true;
        let mut left = DASH[0];
        let mut run: Vec<[f64; 2]> = vec![pts[0]];
        for seg in pts.windows(2) {
            let (mut a, b) = (seg[0], seg[1]);
            let mut len = (b[0] - a[0]).hypot(b[1] - a[1]);
            while len > left {
                let k = left / len;
                let p = [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k];
                if on {
                    run.push(p);
                    self.line(std::mem::take(&mut run), c);
                } else {
                    run = vec![p];
                }
                on = !on;
                len -= left;
                left = DASH[usize::from(!on)];
                a = p;
            }
            left -= len;
            if on {
                run.push(b);
            }
        }
        if on {
            self.line(run, c);
        }
    }

    /// A convex polygon filled (one-pixel rows).
    pub fn fill(&mut self, pts: &[[f64; 2]], col: Hex) {
        let c = Self::color(col, FILL_ALPHA);
        let (y0, y1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[1]), b.max(p[1])));
        if !y0.is_finite() || y1 - y0 > f64::from(self.size[1]) * 2.0 {
            return;
        }
        let mut y = y0.floor();
        while y < y1 {
            let yc = y + 0.5;
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for k in 0..pts.len() {
                let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
                if (a[1] <= yc) != (b[1] <= yc) {
                    let x = a[0] + (yc - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                    lo = lo.min(x);
                    hi = hi.max(x);
                }
            }
            if hi > lo {
                self.rect_fill([lo, y], [hi - lo, 1.0], c);
            }
            y += 1.0;
        }
    }

    /// A pixel rectangle filled.
    pub fn rect_fill(&mut self, at: [f64; 2], size: [f64; 2], c: [f64; 4]) {
        let geometry = PaintGeometry { size, transform: [1.0, 0.0, 0.0, 1.0, at[0], at[1]] };
        let el = BoxEl { geometry, tint: c, uv: [0.0, 0.0, 1.0, 1.0], ..BoxEl::default() };
        self.list.push(self.layer, None, DrawEffects::NONE, Element::Box(el));
    }

    /// The points of an arc (the canvas's, clockwise on the screen from `a0` to `a1`).
    pub fn arc(out: &mut Vec<[f64; 2]>, c: [f64; 2], r: f64, a0: f64, a1: f64) {
        let tau = std::f64::consts::TAU;
        let mut sweep = (a1 - a0) % tau;
        if sweep <= 0.0 {
            sweep += tau;
        }
        // (a point every 4 px of the arc, 8 at the least)
        let n = ((sweep * r / 4.0).ceil() as usize).clamp(8, 256);
        for k in 0..=n {
            let a = a0 + sweep * k as f64 / n as f64;
            out.push([c[0] + r * a.cos(), c[1] + r * a.sin()]);
        }
    }

    /// A circle at a place: filled and stroked, or dashed alone.
    pub fn circle(&mut self, x: f64, y: f64, z: f64, r: f64, col: Hex, dash: bool) {
        let rp = self.px(x, y, z, r);
        let Some(c) = self.to(x, y, z) else { return };
        let mut pts = Vec::new();
        Self::arc(&mut pts, c, rp.max(1.5), 0.0, std::f64::consts::TAU);
        if !dash {
            self.fill(&pts, col);
        }
        self.stroke(&pts, col, 1.0, dash);
    }

    /// A capsule between two points of the play plane (a stadium).
    pub fn capsule(&mut self, a: [f64; 3], b: [f64; 3], r: f64, col: Hex, dash: bool) {
        let rp = self.px(a[0], a[1], a[2], r).max(1.5);
        let Some(p0) = self.to(a[0], a[1], a[2]) else { return };
        let Some(p1) = self.to(b[0], b[1], b[2]) else { return };
        let ang = (p1[1] - p0[1]).atan2(p1[0] - p0[0]);
        let h = std::f64::consts::FRAC_PI_2;
        let mut pts = Vec::new();
        Self::arc(&mut pts, p0, rp, ang + h, ang - h);
        Self::arc(&mut pts, p1, rp, ang - h, ang + h);
        pts.push(pts[0]);
        if !dash {
            self.fill(&pts, col);
        }
        self.stroke(&pts, col, 1.0, dash);
    }

    /// An oriented box: its centre and three half extent vectors (x, y, depth each).
    pub fn box3(&mut self, c: [f64; 3], e: &[f64; 9], col: Hex) {
        const EDGES: [usize; 24] = [0, 1, 1, 3, 3, 2, 2, 0, 4, 5, 5, 7, 7, 6, 6, 4, 0, 4, 1, 5, 2, 6, 3, 7];
        let mut corners = [[0.0; 2]; 8];
        for (k, corner) in corners.iter_mut().enumerate() {
            let s = |bit: usize| if k & bit != 0 { 1.0 } else { -1.0 };
            let (a, b, d) = (s(1), s(2), s(4));
            let Some(p) = self.to(
                c[0] + a * e[0] + b * e[3] + d * e[6],
                c[1] + a * e[1] + b * e[4] + d * e[7],
                c[2] + a * e[2] + b * e[5] + d * e[8],
            ) else {
                return;
            };
            *corner = p;
        }
        let c = Self::color(col, 1.0);
        for k in (0..EDGES.len()).step_by(2) {
            self.line(vec![corners[EDGES[k]], corners[EDGES[k + 1]]], c);
        }
    }

    /// A rectangle of the play plane (depth 0), stroked (solid or dashed) and filled when asked.
    #[allow(clippy::too_many_arguments, reason = "a draw call's parameters")]
    pub fn rect(&mut self, left: f64, right: f64, bottom: f64, top: f64, col: Hex, alpha: f64, dash: bool, fill: bool) {
        let Some(p0) = self.to(left, top, 0.0) else { return };
        let Some(p1) = self.to(right, bottom, 0.0) else { return };
        if fill {
            let c = Self::color(col, FILL_ALPHA);
            let (x, y) = (p0[0].min(p1[0]), p0[1].min(p1[1]));
            self.rect_fill([x, y], [(p1[0] - p0[0]).abs(), (p1[1] - p0[1]).abs()], c);
        }
        let pts = [p0, [p1[0], p0[1]], p1, [p0[0], p1[1]], p0];
        self.stroke(&pts, col, alpha, dash);
    }

    /// A cross of 12 px at a place of the play plane.
    pub fn cross(&mut self, x: f64, y: f64, col: Hex) {
        let Some(p) = self.to(x, y, 0.0) else { return };
        let c = Self::color(col, 1.0);
        self.line(vec![[p[0] - 6.0, p[1]], [p[0] + 6.0, p[1]]], c);
        self.line(vec![[p[0], p[1] - 6.0], [p[0], p[1] + 6.0]], c);
    }

    fn label_info() -> FontInfo {
        // (a Slate size is points at 96 DPI)
        FontInfo { size: LABEL_PX * 72.0 / 96.0, outline: LABEL_OUTLINE, ..FontInfo::default() }
    }

    /// The widest of a label's lines, in pixels.
    pub fn label_width(&mut self, lines: &[String]) -> f64 {
        let info = Self::label_info();
        lines.iter().map(|t| self.fonts.measure(&info, t, 1.0).width).fold(0.0, f64::max)
    }

    /// Lines of text centred at a place of the play plane, their tops `dy` below it and one under the other.
    pub fn label(&mut self, x: f64, y: f64, lines: &[String], col: Hex, dy: f64) {
        let Some(p) = self.to(x, y, 0.0) else { return };
        for (k, t) in lines.iter().enumerate() {
            let w = self.fonts.measure(&Self::label_info(), t, 1.0).width;
            self.text([p[0] - w / 2.0, p[1] + dy + k as f64 * LABEL_STEP], t, Self::color(col, 1.0));
        }
    }

    /// A line of label text with its top left at a pixel point.
    pub fn text(&mut self, at: [f64; 2], t: &str, color: [f64; 4]) {
        let info = Self::label_info();
        let m = self.fonts.measure(&info, t, 1.0);
        let run = self.fonts.shape(&info, t, 1.0);
        let geometry = PaintGeometry { size: [m.width, m.height], transform: [1.0, 0.0, 0.0, 1.0, at[0], at[1]] };
        let el = TextEl { geometry, run, color, outline_color: [0.0, 0.0, 0.0, LABEL_OUTLINE_ALPHA], material: None };
        self.list.push(self.layer, None, DrawEffects::NONE, Element::Text(el));
    }
}
