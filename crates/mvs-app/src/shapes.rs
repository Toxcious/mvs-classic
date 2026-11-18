//! (the port's own) The sim's shapes as it tests them, drawn over the match above the HUD:
//! the Lab Tools' Display page (lab_tools.rs). Nothing here runs unless a switch is on; the sim is read (the box
//! sampler and the hit shapes fill buffers of this file's own or new lists).
//!
//! The shapes are those of the latest tick. A fighter's are moved by the distance between its drawn place of this
//! frame (the HUD's point: between the last two ticks) and the tick's, so they sit on the drawn mesh; the pose is the
//! tick's.
//! Not drawn: the clone shapes of a Disguise's second mesh, the points a pawn with its normal hitboxes off leaves (its
//! shapes show at their full size).
// Deviation: the shapes are elements of the HUD's list: lines of the UI renderer, fills as one-pixel rows blended
// in its linear colour, the labels in the HUD fonts' default typeface (not a 2D canvas of their own with fills
// blended in sRGB and a monospace font). A World actor's radius, hit box and hit spheres are read by class
// (`radius`, the AirFlap's, the Dogpile's and the Banana Guard's), not from any object's fields and methods of those
// names.

use mvs_sim::World;
use mvs_sim::combat::boxes::{CH_ALL, WorldSphere};
use mvs_sim::combat::packet::HitPacket;
use mvs_sim::fighter::FighterId;
use mvs_ui::element::{BoxEl, DrawEffects, Element, ElementList, LineEl, PaintGeometry, TextEl};
use mvs_ui::menu::screens::lab_tools::linear;
use mvs_ui::text::{FontInfo, Fonts};
use mvs_view::{Camera, HudPoint};

/// A colour (a CSS hex colour's bytes).
type Hex = [u8; 3];

const HIT: Hex = [0xff, 0x3b, 0x30];
const HURT: Hex = [0xff, 0xd6, 0x0a];
const HURT_OFF: Hex = [0x8e, 0x8e, 0x93];
const INVULN: Hex = [0x5a, 0xc8, 0xfa];
const CAPSULE: Hex = [0xff, 0xff, 0xff];
const ACTOR_HIT: Hex = [0xff, 0x9f, 0x0a];
const ACTOR_HURT: Hex = [0x30, 0xd1, 0x58];
const PICKUP: Hex = [0x64, 0xd2, 0xff];
const SOLID: Hex = [0xbf, 0x5a, 0xf2];
const GROUND: Hex = [0x30, 0xd1, 0x58];
const PLATFORM: Hex = [0x34, 0xc7, 0xb9];
const BLAST: Hex = [0xff, 0x45, 0x3a];
/// The outer blast box: BLAST at 0x80 alpha ('#ff453a80').
const BLAST_OUTER_ALPHA: f64 = 128.0 / 255.0;
const SPAWN: Hex = [0x0a, 0x84, 0xff];
const WHITE: Hex = [0xff, 0xff, 0xff];

/// The Display screen's shape switches (ShowKey), held by the Lab's tools.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LabShow {
    pub hit: bool,
    pub hurt: bool,
    pub capsule: bool,
    pub actors: bool,
    pub stage: bool,
    pub labels: bool,
}

/// The switches of the Display page, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowKey {
    Hit,
    Hurt,
    Capsule,
    Actors,
    Stage,
    Labels,
}

impl ShowKey {
    pub const ALL: [ShowKey; 6] =
        [ShowKey::Hit, ShowKey::Hurt, ShowKey::Capsule, ShowKey::Actors, ShowKey::Stage, ShowKey::Labels];

    /// The colour the switch's shapes are drawn in (the Display page shows it beside the switch): the stage's ground
    /// for its collision, the labels' white.
    pub fn color(self) -> Hex {
        match self {
            ShowKey::Hit => HIT,
            ShowKey::Hurt => HURT,
            ShowKey::Capsule => CAPSULE,
            ShowKey::Actors => ACTOR_HIT,
            ShowKey::Stage => GROUND,
            ShowKey::Labels => WHITE,
        }
    }

    pub fn get(self, s: &LabShow) -> bool {
        match self {
            ShowKey::Hit => s.hit,
            ShowKey::Hurt => s.hurt,
            ShowKey::Capsule => s.capsule,
            ShowKey::Actors => s.actors,
            ShowKey::Stage => s.stage,
            ShowKey::Labels => s.labels,
        }
    }

    pub fn set(self, s: &mut LabShow, v: bool) {
        *match self {
            ShowKey::Hit => &mut s.hit,
            ShowKey::Hurt => &mut s.hurt,
            ShowKey::Capsule => &mut s.capsule,
            ShowKey::Actors => &mut s.actors,
            ShowKey::Stage => &mut s.stage,
            ShowKey::Labels => &mut s.labels,
        } = v;
    }
}

/// The lines' width, px (the canvas's lineWidth), the fills' alpha, the dashes (on, off), px.
const LINE: f64 = 1.5;
const FILL_ALPHA: f64 = 0.22;
const DASH: [f64; 2] = [5.0, 4.0];
/// The labels: the font's px size ('11px'), the line step, their outline ('#000c', lineWidth 3).
const LABEL_PX: f64 = 11.0;
const LABEL_STEP: f64 = 13.0;
const LABEL_OUTLINE: f64 = 1.5;
const LABEL_OUTLINE_ALPHA: f64 = 204.0 / 255.0;

/// What a frame's overlay draws on.
pub struct Overlay<'a> {
    pub camera: &'a Camera,
    pub size: [u32; 2],
    pub list: &'a mut ElementList,
    pub fonts: &'a mut Fonts,
    pub layer: i32,
    /// Each player's HUD point of this frame (None: not shown).
    pub at: &'a [Option<HudPoint>],
    /// Where the frame is between the last two ticks.
    pub alpha: f64,
}

/// The pose buffer of the box sampler (kept from frame to frame).
#[derive(Debug, Default)]
pub struct ShapesState {
    pose: Vec<f32>,
    spheres: Vec<WorldSphere>,
    ids: Vec<mvs_sim::actors::ActorId>,
    /// The labels' places of this frame (pixels: x, top).
    label_at: Vec<[f64; 3]>,
}

impl Overlay<'_> {
    /// sim (x, y, depth) in UE cm -> pixels (run.rs `anchor`'s projection, unrounded). None behind the camera.
    fn to(&self, x: f64, y: f64, z: f64) -> Option<[f64; 2]> {
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
    fn px(&self, x: f64, y: f64, z: f64, r: f64) -> f64 {
        let Some(t) = self.to(x, y + r, z) else { return 0.0 };
        self.to(x, y, z).map_or(0.0, |p| (p[1] - t[1]).abs())
    }

    fn color(c: Hex, a: f64) -> [f64; 4] {
        let l = linear(c);
        [l[0], l[1], l[2], a]
    }

    fn line(&mut self, pts: Vec<[f64; 2]>, c: [f64; 4]) {
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
    fn stroke(&mut self, pts: &[[f64; 2]], col: Hex, alpha: f64, dash: bool) {
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
    fn fill(&mut self, pts: &[[f64; 2]], col: Hex) {
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

    fn rect_fill(&mut self, at: [f64; 2], size: [f64; 2], c: [f64; 4]) {
        let geometry = PaintGeometry { size, transform: [1.0, 0.0, 0.0, 1.0, at[0], at[1]] };
        let el = BoxEl { geometry, tint: c, uv: [0.0, 0.0, 1.0, 1.0], ..BoxEl::default() };
        self.list.push(self.layer, None, DrawEffects::NONE, Element::Box(el));
    }

    /// The points of an arc (the canvas's, clockwise on the screen from `a0` to `a1`).
    fn arc(out: &mut Vec<[f64; 2]>, c: [f64; 2], r: f64, a0: f64, a1: f64) {
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

    fn circle(&mut self, x: f64, y: f64, z: f64, r: f64, col: Hex, dash: bool) {
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
    #[allow(clippy::too_many_arguments, reason = "a draw call's parameters")]
    fn capsule(&mut self, a: [f64; 3], b: [f64; 3], r: f64, col: Hex, dash: bool) {
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
    fn box3(&mut self, c: [f64; 3], e: &[f64; 9], col: Hex) {
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

    #[allow(clippy::too_many_arguments, reason = "a draw call's parameters")]
    fn rect(&mut self, left: f64, right: f64, bottom: f64, top: f64, col: Hex, alpha: f64, dash: bool, fill: bool) {
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

    fn cross(&mut self, x: f64, y: f64, col: Hex) {
        let Some(p) = self.to(x, y, 0.0) else { return };
        let c = Self::color(col, 1.0);
        self.line(vec![[p[0] - 6.0, p[1]], [p[0] + 6.0, p[1]]], c);
        self.line(vec![[p[0], p[1] - 6.0], [p[0], p[1] + 6.0]], c);
    }

    /// The widest of a label's lines, in pixels.
    fn label_width(&mut self, lines: &[String]) -> f64 {
        let info = FontInfo { size: LABEL_PX * 72.0 / 96.0, outline: LABEL_OUTLINE, ..FontInfo::default() };
        lines.iter().map(|t| self.fonts.measure(&info, t, 1.0).width).fold(0.0, f64::max)
    }

    /// Lines of text centred at a place, their tops `dy` below it and one under the other.
    fn label(&mut self, x: f64, y: f64, lines: &[String], col: Hex, dy: f64) {
        let Some(p) = self.to(x, y, 0.0) else { return };
        // (a Slate size is points at 96 DPI)
        let info = FontInfo { size: LABEL_PX * 72.0 / 96.0, outline: LABEL_OUTLINE, ..FontInfo::default() };
        for (k, t) in lines.iter().enumerate() {
            let m = self.fonts.measure(&info, t, 1.0);
            let run = self.fonts.shape(&info, t, 1.0);
            let geometry = PaintGeometry {
                size: [m.width, m.height],
                transform: [1.0, 0.0, 0.0, 1.0, p[0] - m.width / 2.0, p[1] + dy + k as f64 * LABEL_STEP],
            };
            let el = TextEl {
                geometry,
                run,
                color: Self::color(col, 1.0),
                outline_color: [0.0, 0.0, 0.0, LABEL_OUTLINE_ALPHA],
                material: None,
            };
            self.list.push(self.layer, None, DrawEffects::NONE, Element::Text(el));
        }
    }
}

/// `n` as the labels show it: one decimal, JavaScript's shortest number text.
fn n1(x: f64) -> String {
    let r = (x * 10.0 + 0.5).floor() / 10.0 + 0.0;
    format!("{r}")
}

fn fighter_shapes(o: &mut Overlay<'_>, st: &mut ShapesState, w: &mut World, f: FighterId, i: usize, show: &LabShow) {
    use mvs_sim::fighter as fi;
    let Some(at) = o.at.get(i).copied().flatten() else { return };
    if w.fighter(f).life.dead != 0.0 {
        return;
    }
    // the drawn mesh's place against the tick's (the HUD's point is the mesh's: meshWX / meshWY)
    let (ox, oy) = (at.x - f64::from(fi::mesh_wx(w, f)), at.y - f64::from(fi::mesh_wy(w, f)));
    let x = w.fighter(f).body.x;
    if show.capsule {
        let r = fi::cap_radius(w, f);
        let s = (fi::capsule_half(w, f) - r).max(0.0);
        let ay = fi::actor_y(w, f);
        o.capsule(
            [f64::from(x) + ox, f64::from(ay) - f64::from(s) + oy, 0.0],
            [f64::from(x) + ox, f64::from(ay) + f64::from(s) + oy, 0.0],
            f64::from(r),
            CAPSULE,
            true,
        );
    }
    if !show.hit && !show.hurt {
        return;
    }
    let ch = w.fighter(f).ch;
    let key = w.names.str(ch).to_owned();
    let data = w.data.clone();
    let Some(d) = data.chars.get(&key) else { return };
    st.pose.clear();
    st.pose.resize(d.boxes.stride as usize, 0.0);
    if !fi::anim::sample_boxes(w, f, &mut st.pose) {
        return;
    }
    let (xx, yy) = (at.x, at.y);
    let facing = w.fighter(f).body.facing_dir;
    if show.hurt {
        let col = if fi::invulnerable(w, f) { INVULN } else { HURT };
        let Some(sampler) = w.fixed.samplers.get(&ch) else { return };
        let pose = &st.pose;
        let p = |k: usize| f64::from(pose[k]);
        for &k in &sampler.hurt {
            let sh = &d.boxes.shapes[k as usize];
            let o0 = sampler.offsets[k as usize] as usize;
            let off = w.fighter(f).hit.hurt_off.contains(&(k as i32));
            let c = if off { HURT_OFF } else { col };
            if sh.capsule {
                o.capsule(
                    [xx + p(o0) * f64::from(facing), yy + p(o0 + 1), p(o0 + 2)],
                    [xx + p(o0 + 3) * f64::from(facing), yy + p(o0 + 4), p(o0 + 5)],
                    sh.radius * p(o0 + 6),
                    c,
                    off,
                );
            } else {
                o.circle(xx + p(o0) * f64::from(facing), yy + p(o0 + 1), p(o0 + 2), sh.radius * p(o0 + 3), c, off);
            }
        }
        // (the floor bounce's hurt capsule on the root capsule)
        if let Some(e) = fi::extra_hurt(w, f) {
            o.capsule(
                [f64::from(e.x) + ox, f64::from(e.y0) + oy, 0.0],
                [f64::from(e.x) + ox, f64::from(e.y1) + oy, 0.0],
                f64::from(e.r),
                col,
                false,
            );
        }
    }
    if show.hit {
        let hits: Vec<_> = w.fighter(f).montages.active_hits.values().cloned().collect();
        let pose = std::mem::take(&mut st.pose);
        for h in &hits {
            let props = mvs_sim::combat::hit::hit_props(w, f, h);
            let packet = HitPacket::from_props(&w.names, &props, Some(h.n));
            // (the call writes only the World's warnings, `warned`: a tag with no shape;
            // its readers, the Disguise's clone shapes included, take the World unchanged)
            for s in mvs_sim::combat::phases::hit_spheres(
                w,
                f,
                &packet,
                &pose,
                CH_ALL,
                0.0,
                mvs_sim::math::narrow(f64::INFINITY),
                None,
            ) {
                let z = s.z.unwrap_or(0.0);
                if let Some(b) = &s.bx {
                    o.box3([f64::from(s.x) + ox, f64::from(s.y) + oy, f64::from(z)], &b.map(f64::from), HIT);
                } else if let Some(g) = s.seg {
                    o.capsule(
                        [f64::from(s.x) + ox, f64::from(s.y) + oy, f64::from(z)],
                        [f64::from(g[0]) + ox, f64::from(g[1]) + oy, f64::from(g[2])],
                        f64::from(s.r),
                        HIT,
                        false,
                    );
                } else {
                    o.circle(f64::from(s.x) + ox, f64::from(s.y) + oy, f64::from(z), f64::from(s.r), HIT, false);
                }
            }
        }
        st.pose = pose;
    }
}

fn actor_shapes(o: &mut Overlay<'_>, st: &mut ShapesState, w: &mut World) {
    use mvs_sim::actors::projectile as pj;
    let a = o.alpha;
    let lerp = |p: f64, x: f64| p + (x - p) * a;
    for (_, pr) in w.actors.projectiles.iter() {
        if pr.dead {
            continue;
        }
        let (x, y) = (lerp(f64::from(pr.prev_x), f64::from(pr.x)), lerp(f64::from(pr.prev_y), f64::from(pr.y)));
        // The shapes where the sim tests them (hurt_x / hurt_y, hit_shapes: off the root for the Bugs rockets).
        if pr.hurt_radius > 0.0 {
            o.circle(
                x + f64::from(pj::hurt_x(pr) - pr.x),
                y + f64::from(pj::hurt_y(pr) - pr.y),
                0.0,
                f64::from(pr.hurt_radius),
                ACTOR_HURT,
                false,
            );
        }
        if pr.hit_radius > 0.0 {
            st.spheres.clear();
            pj::hit_shapes(w, pr, &mut st.spheres);
            for s in std::mem::take(&mut st.spheres) {
                o.circle(x + f64::from(s.x - pr.x), y + f64::from(s.y - pr.y), 0.0, f64::from(s.r), ACTOR_HIT, false);
            }
        } else {
            o.cross(x, y, ACTOR_HIT);
        }
        // (its solid boxes with their collision on, where the frame's head placed them)
        for s in pr.solids.iter().flatten() {
            if s.on && !pr.no_collision {
                o.rect(
                    f64::from(s.left),
                    f64::from(s.right),
                    f64::from(s.bottom),
                    f64::from(s.top),
                    SOLID,
                    1.0,
                    false,
                    false,
                );
            }
        }
        o.label(x, y, &[format!("{} #{}", w.names.str(pr.cls), pr.seq)], ACTOR_HIT, 10.0);
    }
    for (_, it) in w.actors.items.iter() {
        if it.dead {
            continue;
        }
        let (x, y) = (lerp(f64::from(it.prev_x), f64::from(it.x)), lerp(f64::from(it.prev_y), f64::from(it.y)));
        let def = it.def(&w.data);
        o.circle(
            x + f64::from(it.pickup_x(def)) - f64::from(it.x),
            y + f64::from(it.pickup_y(def)) - f64::from(it.y),
            0.0,
            f64::from(it.pickup_radius(def)),
            PICKUP,
            true,
        );
        let hurt = it.hurt_radius();
        if hurt > 0.0 {
            o.circle(x, y, 0.0, f64::from(hurt), ACTOR_HURT, false);
        }
        let hit = it.hit_radius(def.p);
        if hit > 0.0 {
            o.circle(x, y, 0.0, f64::from(hit), ACTOR_HIT, false);
        }
        o.label(x, y, &[format!("{} #{}", w.names.str(it.cls), it.seq)], PICKUP, 10.0);
    }
    let mark = |o: &mut Overlay<'_>, cls: &str, seq: i32, x: f64, y: f64| {
        o.cross(x, y, ACTOR_HIT);
        o.label(x, y, &[format!("{cls} #{seq}")], ACTOR_HIT, 10.0);
    };
    for (_, m) in w.actors.markers.iter().filter(|(_, m)| !m.dead) {
        mark(o, w.names.str(m.cls), m.seq, f64::from(m.x), f64::from(m.y));
    }
    for (_, m) in w.actors.ships.iter().filter(|(_, m)| !m.dead) {
        mark(o, w.names.str(m.cls), m.seq, f64::from(m.x), f64::from(m.y));
    }
    for (_, m) in w.actors.mortars.iter().filter(|(_, m)| !m.dead) {
        mark(o, w.names.str(m.cls), m.seq, f64::from(m.x), f64::from(m.y));
    }
    world_actor_shapes(o, st, w);
    for (_, m) in w.actors.aim_punches.iter().filter(|(_, m)| !m.dead) {
        mark(
            o,
            w.names.str(m.cls),
            m.seq,
            lerp(f64::from(m.prev_x), f64::from(m.x)),
            lerp(f64::from(m.prev_y), f64::from(m.y)),
        );
    }
    for (_, m) in w.actors.accelerators.iter().filter(|(_, m)| !m.dead) {
        mark(o, w.names.str(m.cls), m.seq, f64::from(m.x), f64::from(m.y));
    }
}

/// The World's actors: a SpawnedPlatformParent child's solid boxes, then for one with a
/// place its root capsule as a pawn (`pawnBody`) or else the circle of its radius, its oriented hit box
/// (`hitBoxShape`: the Banana Guard's spear) and its hit spheres (`hitSpheres`: the AirFlap's, the Dogpile's), each
/// grey while its hit boxes are off, the cross and the label.
fn world_actor_shapes(o: &mut Overlay<'_>, st: &mut ShapesState, w: &mut World) {
    use mvs_sim::actors::WorldActorState as S;
    use mvs_sim::registry::entries::ActorHooks;
    let a = o.alpha;
    st.ids.clear();
    st.ids.extend(w.actors.actors.iter().filter(|(_, p)| !p.dead).map(|(id, _)| id));
    for i in 0..st.ids.len() {
        let id = st.ids[i];
        let Some(p) = w.actors.actors.get(id) else { continue };
        // (a SpawnedPlatformParent child's solid boxes, where they carry their sides)
        for s in p.plat.iter().flat_map(|pl| pl.solids()) {
            o.rect(
                f64::from(s.left),
                f64::from(s.right),
                f64::from(s.bottom),
                f64::from(s.top),
                SOLID,
                1.0,
                false,
                false,
            );
        }
        let Some((ox, oy, _)) = mvs_sim::actors::world_actor::world_actor_place_of(w, id) else { continue };
        let (x, y) = match &p.state {
            S::AimMarker(m) => (
                f64::from(m.prev_x) + f64::from(m.x - m.prev_x) * a,
                f64::from(m.prev_y) + f64::from(m.y - m.prev_y) * a,
            ),
            _ => (f64::from(ox), f64::from(oy)),
        };
        let (dx, dy) = (x - f64::from(ox), y - f64::from(oy));
        let hooks = match w.registry.actors.get(w.names.str(p.cls)).map(|e| e.hooks) {
            Some(ActorHooks::World(h)) => Some(h),
            _ => None,
        };
        if let Some(pb) = hooks.and_then(|h| h.pawn_body).and_then(|f| f(w, id)) {
            o.capsule(
                [(f64::from(pb.x) + dx), (f64::from(pb.y0) + dy), 0.0],
                [(f64::from(pb.x) + dx), (f64::from(pb.y1) + dy), 0.0],
                f64::from(pb.r),
                ACTOR_HURT,
                false,
            );
        } else if let Some(r) = radius(&p.state).filter(|&r| r > 0.0) {
            o.circle(x, y, 0.0, r, ACTOR_HIT, false);
        }
        let (cls, seq) = (p.cls, p.seq);
        let off = |on: bool| if on { ACTOR_HIT } else { HURT_OFF };
        st.spheres.clear();
        let mut sphere_col = ACTOR_HIT;
        let mut spear = None;
        match &p.state {
            S::AirFlap(s) => {
                sphere_col = off(s.hit_on);
                if let Some(f) = hooks.and_then(|h| h.hit_shapes) {
                    f(w, id, &mut st.spheres);
                }
            }
            S::Dogpile(s) => {
                sphere_col = off(s.hit_on);
                let r = s.radius * mvs_sim::chars::taz::dogpile::world_scale(w, id);
                st.spheres.push(WorldSphere { x: s.x, y: s.y, z: None, r, ..WorldSphere::default() });
            }
            S::BananaGuard(g) => spear = Some(off(g.hit_on)),
            _ => {}
        }
        // (the spear's pose fills the guard's clip cache on its first call: static data, not state)
        if let Some(col) = spear
            && let Some(hb) = mvs_sim::items::banana_guard::spear_pose(w, id)
            && let Some(b) = &hb.bx
        {
            o.box3(
                [(f64::from(hb.x) + dx), (f64::from(hb.y) + dy), f64::from(hb.z.unwrap_or(0.0))],
                &b.map(f64::from),
                col,
            );
        }
        for s in &st.spheres {
            o.circle(
                f64::from(s.x) + dx,
                f64::from(s.y) + dy,
                f64::from(s.z.unwrap_or(0.0)),
                f64::from(s.r),
                sphere_col,
                false,
            );
        }
        o.cross(x, y, ACTOR_HIT);
        o.label(x, y, &[format!("{} #{}", w.names.str(cls), seq)], ACTOR_HIT, 10.0);
    }
}

/// The first of `hitRadius`, `radius` and `r` a World actor's class holds.
// Unverified: the Rust classes' fields keep their names (`radius`, `r`; TurretBomb's `hitRadius` getter is its
// sphere's radius at its scale); a field renamed in a port draws no circle here.
fn radius(s: &mvs_sim::actors::WorldActorState) -> Option<f64> {
    use mvs_sim::actors::WorldActorState as S;
    Some(f64::from(match s {
        S::TurretBomb(b) => b.sphere_r * b.scale,
        S::ProjectileBarrier(s) => s.radius,
        S::SmokeGrenade(s) => s.radius,
        S::SmokeZone(s) => s.radius,
        S::SnackBox(s) => s.radius,
        S::ScrubShield(s) => s.radius,
        S::AntenaBolts(s) => s.radius,
        S::BracerShield(s) => s.radius,
        S::HarleyBomb(s) => s.radius,
        S::DisguiseShock(s) => s.radius,
        S::KillPie(s) => s.radius,
        S::ForceField(s) => s.radius,
        S::PortalAimer(s) => s.radius,
        S::PolymorphGrenade(s) => s.radius,
        S::Dogpile(s) => s.radius,
        S::HealZone(s) => s.radius,
        S::ElectricZone(s) => s.radius,
        S::JakeSpitAllyHitBox(s) => s.radius,
        S::SpitHitBox(s) => s.radius,
        S::Evidence(s) => s.radius,
        S::Ray(s) => s.radius,
        S::AllySaw(s) => s.radius,
        S::BananaGuard(s) => s.radius,
        S::Nepter(s) => s.radius,
        S::HighFiveShockwave(s) => s.r,
        S::GemBoostBlast(s) => s.r,
        S::RocketExplosion(s) => s.r,
        _ => return None,
    }))
}

fn stage_shapes(o: &mut Overlay<'_>, w: &World) {
    let s = &w.stage;
    let g = &s.ground;
    o.rect(g.left, g.right, g.bottom, g.top, GROUND, 1.0, false, true);
    for p in &s.stage_platforms {
        o.rect(
            f64::from(p.left),
            f64::from(p.right),
            f64::from(p.bottom),
            f64::from(p.top),
            PLATFORM,
            1.0,
            false,
            true,
        );
    }
    let (b, bo) = (&s.blast, &s.blast_outer);
    o.rect(f64::from(b.left), f64::from(b.right), f64::from(b.bottom), f64::from(b.top), BLAST, 1.0, false, false);
    o.rect(
        f64::from(bo.left),
        f64::from(bo.right),
        f64::from(bo.bottom),
        f64::from(bo.top),
        BLAST,
        BLAST_OUTER_ALPHA,
        true,
        false,
    );
    for (i, p) in s.spawns.iter().enumerate() {
        o.cross(f64::from(p.x), f64::from(p.y), SPAWN);
        o.label(f64::from(p.x), f64::from(p.y), &[format!("spawn {}", i + 1)], SPAWN, 8.0);
    }
    for (i, p) in s.respawns.iter().enumerate() {
        o.cross(f64::from(p.x), f64::from(p.y), SPAWN);
        o.label(f64::from(p.x), f64::from(p.y), &[format!("respawn {}", i + 1)], SPAWN, 8.0);
    }
}

/// A fighter's state label over its head, moved up past an earlier one's it would overlap.
fn state_label(o: &mut Overlay<'_>, st: &mut ShapesState, w: &World, f: FighterId, i: usize) {
    use mvs_sim::fighter as fi;
    let Some(at) = o.at.get(i).copied().flatten() else { return };
    let fm = w.fighter(f);
    if fm.life.dead != 0.0 {
        return;
    }
    let invuln = fi::invulnerable(w, f);
    let mo = fm.montages.montage.as_ref();
    let mut v = format!("v {}, {}", n1(f64::from(fm.body.vx)), n1(f64::from(fm.body.vy)));
    if fm.hit.hitstun > 0.0 {
        v += &format!(" · hitstun {}", fm.hit.hitstun);
    }
    if fm.hit.hitpause > 0.0 {
        v += &format!(" · hitpause {}", fm.hit.hitpause);
    }
    if invuln {
        v += " · invulnerable";
    }
    let lines = [
        format!(
            "P{} {}{} · {} dmg",
            i + 1,
            fi::anim::update::state_name(w, f),
            if fi::grounded(w, f) { "" } else { " (air)" },
            n1(f64::from(fm.stats.damage))
        ),
        match mo {
            Some(m) => format!("{} {:.3} s (f {})", w.names.str(m.montage.name), m.pos, (m.pos * 60.0 + 1e-4).floor()),
            None => w.names.str(fm.anim.loco_anim).to_owned(),
        },
        v,
    ];
    let x = at.x;
    let y = f64::from(fi::actor_y(w, f)) + f64::from(fi::capsule_half(w, f)) + (at.y - f64::from(fi::mesh_wy(w, f)));
    let Some(p) = o.to(x, y, 0.0) else { return };
    let n = lines.len() as f64;
    let half = o.label_width(&lines) / 2.0;
    let mut top = p[1] - 8.0 - n * LABEL_STEP;
    for l in &st.label_at {
        if (l[0] - p[0]).abs() < l[2] + half + 8.0 && (l[1] - top).abs() < n * LABEL_STEP + 2.0 {
            top = l[1] - n * LABEL_STEP - 4.0;
        }
    }
    st.label_at.push([p[0], top, half]);
    o.label(x, y, &lines, WHITE, top - p[1]);
}

/// One frame's overlay, after the HUD's draw.
pub fn draw(o: &mut Overlay<'_>, st: &mut ShapesState, w: &mut World, show: &LabShow) {
    if !ShowKey::ALL.iter().any(|k| k.get(show)) {
        return;
    }
    if show.stage {
        stage_shapes(o, w);
    }
    if show.actors {
        actor_shapes(o, st, w);
    }
    for i in 0..w.players.len() {
        let f = w.players[i];
        fighter_shapes(o, st, w, f, i, show);
    }
    st.label_at.clear();
    if show.labels {
        for i in 0..w.players.len() {
            let f = w.players[i];
            state_label(o, st, w, f, i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_show_as_the_ts_labels() {
        assert_eq!(n1(1.25), "1.3");
        assert_eq!(n1(2.0), "2");
        assert_eq!(n1(-0.04), "0");
        assert_eq!(n1(-1.25), "-1.2");
    }

    #[test]
    fn every_switch_reads_and_writes_its_field() {
        let mut s = LabShow::default();
        for k in ShowKey::ALL {
            k.set(&mut s, true);
            assert!(k.get(&s));
        }
        assert_eq!(s, LabShow { hit: true, hurt: true, capsule: true, actors: true, stage: true, labels: true });
    }
}
