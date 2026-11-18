//! The launcher's backgrounds and wordmark: the night
//! sky of the splash and setup with its drifting glows, Home's sky under the night Treehouse picture and the picture's
//! lit windows, smoke, twinkles and fireflies, the stars of a fixed seed, the wordmark drawn in type.

use super::draw::Canvas;
use super::fonts::Face;
use super::paint::{CLEAR, Fill, Kind, Rgba, Shape, hex, rgba};

/// A wandering glow: size, colour, centre (%), opacity, its keys, its period (s), alternating.
type Glow = (f32, Rgba, [f32; 2], f32, [[f32; 3]; 3], f32, bool);
/// A firefly: x, y (%), its path, its period (s), its blink's period, its delay, its blink's delay.
type Fly = (f32, f32, [[f32; 2]; 3], f32, f32, f32, f32);

/// A star (its place in % of the sky, its size, its twinkle's period and phase).
#[derive(Clone, Copy, Debug)]
pub struct Star {
    x: f32,
    y: f32,
    size: f32,
    dur: f32,
    delay: f32,
}

/// The stars of the splash's sky, setup's and Home's, in the order they are made (one seeded generator).
pub fn stars() -> [Vec<Star>; 3] {
    let mut seed: u32 = 7;
    let mut rnd = move || {
        // mulberry32
        seed = seed.wrapping_add(0x6d2b_79f5);
        let mut t = (seed ^ (seed >> 15)).wrapping_mul(1 | seed);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        f64::from(t ^ (t >> 14)) as f32 / 4_294_967_296.0
    };
    let mut set = |n: usize, height: f32| -> Vec<Star> {
        (0..n)
            .map(|_| {
                let size = 1.0 + rnd() * 1.6;
                let x = rnd() * 100.0;
                let y = rnd() * height;
                let dur = 2.0 + rnd() * 4.0;
                let delay = -rnd() * 6.0;
                Star { x, y, size, dur, delay }
            })
            .collect()
    };
    let splash = set(90, 75.0);
    let setup = set(90, 75.0);
    let home = set(60, 60.0);
    [splash, setup, home]
}

/// CSS ease-in-out (cubic-bezier(.42, 0, .58, 1)), near enough.
fn ease(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

/// An `infinite alternate` animation's progress at `t` (s): 0..1 forward, then back.
fn alternate(t: f64, dur: f32, delay: f32) -> f32 {
    let c = (t as f32 - delay) / dur;
    let f = c - c.floor();
    if (c.floor() as i64).rem_euclid(2) == 1 { 1.0 - f } else { f }
}

/// An `infinite` animation's progress at `t` (s), 0..1.
fn cycle(t: f64, dur: f32, delay: f32) -> f32 {
    let c = (t as f32 - delay) / dur;
    c - c.floor()
}

/// Three keyframes (0, 50, 100 %) at `p`, each half eased.
fn keys3(p: f32, k: [[f32; 3]; 3]) -> [f32; 3] {
    let (a, b, q) = if p < 0.5 { (k[0], k[1], p * 2.0) } else { (k[1], k[2], p * 2.0 - 1.0) };
    let q = ease(q);
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * q)
}

fn draw_stars(c: &mut Canvas, stars: &[Star], area: [f32; 2]) {
    for s in stars {
        let p = ease(alternate(c.t, s.dur, s.delay));
        let op = 0.15 + 0.85 * p;
        let r = s.size * (0.7 + 0.3 * p) * 0.5;
        let at = [s.x / 100.0 * area[0] + s.size * 0.5, s.y / 100.0 * area[1] + s.size * 0.5];
        // box-shadow: 0 0 6px rgba(190,210,255,.9)
        let mut glow =
            Shape::new(Kind::Rect, [at[0] - r, at[1] - r, 2.0 * r, 2.0 * r], Fill::Solid(rgba(190, 210, 255, 0.9)));
        glow.radius = r;
        glow.soft = 6.0;
        glow.opacity = op * 0.6;
        c.push(glow);
        let mut dot = Shape::new(Kind::Rect, [at[0] - r, at[1] - r, 2.0 * r, 2.0 * r], Fill::Solid(hex(0xdfe8ff, 1.0)));
        dot.radius = r;
        dot.opacity = op;
        c.push(dot);
    }
}

/// `.bg.simple`: the night sky with its haze and drifting glows (splash, setup).
pub fn simple(c: &mut Canvas, stars: &[Star]) {
    let [w, h] = c.size;
    let full = [0.0, 0.0, w, h];
    let base = c.down(
        &[(0.0, hex(0x071240, 1.0)), (0.4, hex(0x081a55, 1.0)), (0.78, hex(0x12406f, 1.0)), (1.0, hex(0x1b5470, 1.0))],
        full,
    );
    c.rect(full, base, 0.0);
    let haze = |c: &mut Canvas, at: [f32; 2], radii: [f32; 2], col: Rgba, end: f32| {
        let mut clear = col;
        clear[3] = 0.0;
        let f = c.radial(&[(0.0, col), (end, clear), (1.0, clear)], [at[0] / 100.0 * w, at[1] / 100.0 * h], radii);
        c.rect(full, f, 0.0);
    };
    haze(c, [104.0, 70.0], [600.0, 380.0], rgba(22, 74, 127, 0.6), 0.65);
    haze(c, [-6.0, 104.0], [700.0, 420.0], rgba(110, 97, 165, 0.55), 0.65);
    haze(c, [50.0, 118.0], [1000.0, 520.0], rgba(38, 90, 121, 0.85), 0.7);
    // The glows: blurred discs that wander (wander1..3) over their periods.
    let w1 = [[0.0, 0.0, 1.0], [-110.0, 70.0, 1.12], [-30.0, 140.0, 0.94]];
    let w2 = [[0.0, 0.0, 1.0], [120.0, -50.0, 0.9], [200.0, 20.0, 1.08]];
    let w3 = [[0.0, 0.0, 1.0], [-80.0, -60.0, 1.15], [60.0, -20.0, 0.92]];
    let glows: [Glow; 4] = [
        (540.0, hex(0x1d4f9a, 1.0), [50.0, -14.0], 0.28, w1, 19.0, false),
        (440.0, hex(0x3d8fb0, 1.0), [4.0, 52.0], 0.16, w2, 23.0, false),
        (320.0, hex(0xffc45a, 1.0), [78.0, 66.0], 0.05, w3, 27.0, false),
        (380.0, hex(0x6e61a5, 1.0), [22.0, 60.0], 0.14, w3, 31.0, true),
    ];
    for (d, col, at, op, keys, dur, reverse) in glows {
        let mut p = alternate(c.t, dur, 0.0);
        if reverse {
            p = 1.0 - p;
        }
        let [dx, dy, k] = keys3(p, keys);
        let r = d * 0.5 * k;
        let centre = [at[0] / 100.0 * w + d * 0.5 + dx, at[1] / 100.0 * h + d * 0.5 + dy];
        let mut s = Shape::new(Kind::Rect, [centre[0] - r, centre[1] - r, 2.0 * r, 2.0 * r], Fill::Solid(col));
        s.radius = r;
        // Unverified: filter: blur(80-100px) as a linear edge of 200 px.
        s.soft = 200.0;
        s.opacity = op;
        c.push(s);
    }
    draw_stars(c, stars, [w, h]);
}

/// `.bg.art`: Home's sky, the picture when it is there and its life, the shade under the launcher's parts.
// Deviation: no film grain (.grain, an SVG noise at 7 % in overlay blend).
pub fn art(c: &mut Canvas, stars: &[Star], picture: bool, picture_on: f32) {
    let [w, h] = c.size;
    let full = [0.0, 0.0, w, h];
    let base = c.down(
        &[
            (0.0, hex(0x081447, 1.0)),
            (0.15, hex(0x061b55, 1.0)),
            (0.32, hex(0x0b2a6a, 1.0)),
            (0.42, hex(0x0f3274, 1.0)),
            (0.55, hex(0x17397f, 1.0)),
            (0.66, hex(0x1f4189, 1.0)),
            (0.78, hex(0x27468f, 1.0)),
            (0.90, hex(0x2f4490, 1.0)),
            (1.0, hex(0x3a4594, 1.0)),
        ],
        full,
    );
    c.rect(full, base, 0.0);
    let f = c.radial(
        &[(0.0, rgba(38, 90, 121, 0.55)), (0.7, rgba(38, 90, 121, 0.0)), (1.0, CLEAR)],
        [w, 1.1 * h],
        [700.0, 420.0],
    );
    c.rect(full, f, 0.0);
    let f = c.radial(
        &[(0.0, rgba(60, 140, 150, 0.16)), (0.7, rgba(60, 140, 150, 0.0)), (1.0, CLEAR)],
        [0.46 * w, 0.74 * h],
        [520.0, 340.0],
    );
    c.rect(full, f, 0.0);
    draw_stars(c, stars, [w, h]);
    if picture {
        // top -10px, right -70px, 122% of the sky's height, square
        let side = 1.22 * h;
        let r = [w + 70.0 - side, -10.0, side, side];
        let mut s = Shape::new(Kind::Image, r, Fill::Solid(CLEAR));
        s.uv = [0.0, 0.0, 1.0, 1.0];
        // Deviation: the left fade starts at u 0.2, not the page's 0.04: the picture's umbrella platform and branch,
        // cut off by its left edge, showed half faded beside the house.
        s.fade = [0.2, 0.52, 0.66, 0.98];
        s.p = [if c.p.picture_srgb() { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0];
        s.opacity = picture_on;
        c.push(s);
        alive(c, r, ((picture_on - 0.25) / 0.75).clamp(0.0, 1.0));
    }
    // ::after: the shade at the bottom, the left and the top
    let shade = rgba(4, 10, 35, 0.0);
    let f = c.linear(&[(0.0, rgba(4, 10, 35, 0.94)), (0.5, shade), (1.0, shade)], [0.0, h], [0.0, 0.0]);
    c.rect(full, f, 0.0);
    let f = c.linear(&[(0.0, rgba(4, 10, 35, 0.55)), (0.38, shade), (1.0, shade)], [0.0, 0.0], [w, 0.0]);
    c.rect(full, f, 0.0);
    let f = c.linear(&[(0.0, rgba(4, 10, 35, 0.55)), (0.14, shade), (1.0, shade)], [0.0, 0.0], [0.0, h]);
    c.rect(full, f, 0.0);
}

/// The picture's life (`.alive`): `r` its square, `op` the box's opacity.
fn alive(c: &mut Canvas, r: [f32; 4], op: f32) {
    if op <= 0.0 {
        return;
    }
    let cq = r[2] / 100.0;
    let at = |x: f32, y: f32| [r[0] + x / 100.0 * r[2], r[1] + y / 100.0 * r[3]];
    let blob = |c: &mut Canvas, centre: [f32; 2], size: [f32; 2], stops: &[(f32, Rgba)], o: f32| {
        let f = c.radial(stops, centre, [size[0] * 0.5, size[1] * 0.5]);
        let mut s = Shape::new(Kind::Rect, [centre[0] - size[0] * 0.5, centre[1] - size[1] * 0.5, size[0], size[1]], f);
        s.opacity = o * op;
        c.push(s);
    };
    // The lit windows: a halo and a core, flickering in steps (lampFlickerSoft / lampFlicker).
    let flicker = |p: f32, soft: bool| -> f32 {
        let k: &[(f32, f32, f32)] = &[
            (0.37, 0.78, 0.45),
            (0.385, 1.05, 1.15),
            (0.40, 0.86, 0.6),
            (0.42, 1.0, 1.0),
            (0.79, 0.9, 0.7),
            (0.805, 1.0, 1.0),
        ];
        let mut v = 1.0;
        for &(at, s, h) in k {
            if p >= at {
                v = if soft { s } else { h };
            }
        }
        v
    };
    let warm = |a: f32| rgba(255, 196, 96, a);
    let win = at(66.2, 42.8);
    let b = flicker(cycle(c.t, 5.3, 0.0), true);
    blob(
        c,
        win,
        [30.0 * cq, 34.0 * cq],
        &[(0.0, warm(0.42 * b)), (0.55, rgba(255, 170, 70, 0.18 * b)), (1.0, CLEAR)],
        1.0,
    );
    blob(
        c,
        win,
        [13.0 * cq, 17.0 * cq],
        &[(0.0, rgba(255, 226, 150, 0.5 * b)), (0.6, rgba(255, 200, 110, 0.16 * b)), (1.0, CLEAR)],
        1.0,
    );
    let door = at(72.6, 58.1);
    let b = flicker(cycle(c.t, 7.1, -2.2), false);
    blob(
        c,
        door,
        [9.0 * cq, 9.0 * cq],
        &[(0.0, warm(0.32 * b)), (0.55, rgba(255, 170, 70, 0.12 * b)), (1.0, CLEAR)],
        1.0,
    );
    blob(c, door, [3.6 * cq, 3.6 * cq], &[(0.0, rgba(255, 230, 160, 0.45 * b)), (1.0, CLEAR)], 1.0);
    // The chimney's smoke: three puffs a third of a period apart.
    for delay in [0.0, -2.5, -5.0] {
        let p = ease(cycle(c.t, 7.5, delay));
        let o = if p < 0.15 { p / 0.15 * 0.9 } else { 0.9 * (1.0 - (p - 0.15) / 0.85) };
        let scale = 0.4 + 1.5 * p;
        let centre = at(86.2, 9.3);
        let centre = [centre[0] + 2.5 * cq * p, centre[1] - 7.0 * cq * p];
        let d = 4.0 * cq * scale;
        blob(c, centre, [d, d], &[(0.0, rgba(205, 215, 255, 0.55)), (0.6, rgba(180, 195, 250, 0.2)), (1.0, CLEAR)], o);
    }
    // The painted stars that twinkle.
    for (x, y, size, dur, delay) in [
        (42.6, 15.4, 2.0, 3.2, 0.0),
        (71.7, 17.8, 1.4, 2.6, -1.1),
        (45.5, 17.4, 1.4, 4.1, -0.6),
        (25.8, 26.4, 1.4, 3.6, -2.3),
    ] {
        let p = ease(alternate(c.t, dur, delay));
        let d = size * cq * (0.5 + 0.65 * p);
        blob(
            c,
            at(x, y),
            [d, d],
            &[(0.0, rgba(255, 255, 255, 0.95)), (0.4, rgba(190, 210, 255, 0.35)), (1.0, CLEAR)],
            0.1 + 0.9 * p,
        );
    }
    // Fireflies: dots that wander on loops of their own and blink.
    let fly_a = [[0.0, 0.0], [1.6, -1.2], [-0.8, -2.4]];
    let fly_b = [[0.0, 0.0], [-1.8, -0.8], [-0.6, 1.4]];
    let fly_c = [[0.0, 0.0], [1.0, 1.2], [2.0, -1.0]];
    let flies: [Fly; 9] = [
        (48.0, 52.0, fly_a, 11.0, 4.2, 0.0, 0.0),
        (57.0, 61.0, fly_b, 13.0, 5.1, -3.0, -1.2),
        (63.0, 50.0, fly_c, 9.5, 3.7, -6.0, -2.5),
        (70.0, 64.0, fly_a, 12.0, 4.6, -8.0, -0.4),
        (78.0, 54.0, fly_b, 10.0, 3.9, -1.0, -3.1),
        (83.0, 62.0, fly_c, 14.0, 5.6, -5.0, -1.9),
        (53.0, 66.0, fly_a, 10.5, 4.9, -4.0, -3.6),
        (74.0, 47.0, fly_c, 12.5, 4.4, -7.0, -0.9),
        (60.0, 57.0, fly_b, 11.5, 6.0, -2.0, -4.4),
    ];
    for (x, y, path, dur, blink, delay, blink_delay) in flies {
        let p = alternate(c.t, dur, delay);
        let k = keys3(p, path.map(|v| [v[0], v[1], 0.0]));
        let b = cycle(c.t, blink, blink_delay);
        let o = match b {
            b if b < 0.35 => 0.0,
            b if b < 0.5 => (b - 0.35) / 0.15 * 0.95,
            b if b < 0.65 => 0.95 - (b - 0.5) / 0.15 * 0.25,
            b if b < 0.8 => 0.7 * (1.0 - (b - 0.65) / 0.15),
            _ => 0.0,
        };
        let centre = at(x, y);
        let centre = [centre[0] + k[0] * cq, centre[1] + k[1] * cq];
        // box-shadow: 0 0 .9cqw .35cqw rgba(255,214,90,.65)
        let g = 0.55 * cq + 0.7 * cq;
        blob(
            c,
            centre,
            [g * 2.2, g * 2.2],
            &[(0.0, rgba(255, 214, 90, 0.65)), (0.5, rgba(255, 214, 90, 0.3)), (1.0, CLEAR)],
            o,
        );
        let mut s = Shape::new(
            Kind::Rect,
            [centre[0] - 0.275 * cq, centre[1] - 0.275 * cq, 0.55 * cq, 0.55 * cq],
            Fill::Solid(hex(0xfff3b0, 1.0)),
        );
        s.radius = cq;
        s.opacity = o * op;
        c.push(s);
    }
}

/// The wordmark: MVS in Barlow 900 italic at `size` with its gradient and ledge, the CLASSIC tag over its lower right;
/// `x`, `y` its box's top left. Its box's width and height.
pub fn wordmark(c: &mut Canvas, x: f32, y: f32, size: f32) -> [f32; 2] {
    let spacing = -0.02 * size;
    let line = c.fonts.normal(Face::Wordmark, size * c.s) / c.s;
    let tw = c.width(Face::Wordmark, size, spacing, "MVS") + 10.0;
    // (the span's font shorthand sets its line-height to normal: the box is that line's)
    let bh = line;
    let ty = y;
    let (asc, desc) = c.fonts.ascent(Face::Wordmark, size * c.s);
    let base = ty + c.fonts.baseline(Face::Wordmark, size * c.s, line * c.s) / c.s;
    let (top, bottom) = (base - asc / c.s, base + desc / c.s);
    // drop-shadow(0 12px 26px rgba(3,10,40,.52)), drop-shadow(0 3px 0 #964716)
    c.text_with(
        Face::Wordmark,
        size,
        spacing,
        x,
        ty + 12.0,
        line,
        "MVS",
        Fill::Solid(rgba(3, 10, 40, 0.22)),
        None,
        1.0,
    );
    c.text_with(Face::Wordmark, size, spacing, x, ty + 3.0, line, "MVS", Fill::Solid(hex(0x964716, 1.0)), None, 1.0);
    let fill = c.linear(
        &[
            (0.06, hex(0xffeca8, 1.0)),
            (0.40, hex(0xf9c850, 1.0)),
            (0.70, hex(0xf0a030, 1.0)),
            (0.96, hex(0xe27f2c, 1.0)),
        ],
        [x, top],
        [x, bottom],
    );
    c.text_with(Face::Wordmark, size, spacing, x, ty, line, "MVS", fill, None, 1.0);
    // The tag: right -16, bottom -6, skew(-12deg) rotate(-4deg).
    let px = 28.0;
    let tag_spacing = 0.14 * px;
    let tline = c.fonts.normal(Face::Display, px * c.s) / c.s;
    let tw2 = c.width(Face::Display, px, tag_spacing, "CLASSIC") + 30.0;
    let th = tline + 1.0;
    let r = [x + tw + 16.0 - tw2, y + bh + 6.0 - th, tw2, th];
    let (sk, rot) = ((-12.0f32).to_radians().tan(), (-4.0f32).to_radians());
    let (cs, sn) = (rot.cos(), rot.sin());
    // skew * rotate (CSS applies the rightmost first)
    let m = [cs + sk * sn, -sn + sk * cs, sn, cs];
    c.xf = Some((m, [r[0] + r[2] * 0.5, r[1] + r[3] * 0.5]));
    c.shadow([r[0], r[1] + 10.0, r[2], r[3]], rgba(3, 10, 40, 0.45), 3.0, 20.0);
    c.solid([r[0], r[1] + 3.0, r[2], r[3]], hex(0x6c0f1c, 1.0), 3.0);
    let f = c.down(&[(0.0, hex(0xe64f5b, 1.0)), (0.55, hex(0xc92a3b, 1.0)), (1.0, hex(0xa91e31, 1.0))], r);
    c.rect(r, f, 3.0);
    c.solid([r[0] + 2.0, r[1], r[2] - 4.0, 1.0], rgba(255, 255, 255, 0.4), 0.0);
    c.text_with(
        Face::Display,
        px,
        tag_spacing,
        r[0] + 16.0,
        r[1] + 1.0,
        tline,
        "CLASSIC",
        Fill::Solid([1.0; 4]),
        None,
        1.0,
    );
    c.xf = None;
    [tw, bh]
}
