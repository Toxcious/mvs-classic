//! An upright capsule against a box in the play plane: the port's own geometry
//! standing in for PhysX's capsule sweep, in float32 in a fixed order of operations.

use crate::{Box2, Ground, arg, max, max3, out};

/// capsuleGap: the distance from the capsule's axis (centre (x, y), radius r, half height hh) to the box, 0 with the
/// axis in it.
pub fn capsule_gap(x: impl Into<f64>, y: impl Into<f64>, r: impl Into<f64>, hh: impl Into<f64>, g: &Ground) -> f64 {
    let (x, y, r, hh) = (arg(x), arg(y), arg(r), arg(hh));
    let g = g.narrow();
    let s = max(hh - r, 0.0);
    out(len2(max3(g.left - x, 0.0, x - g.right), max3(g.bottom - s - y, 0.0, y - g.top - s)))
}

/// The length of (a, b): sqrtss of a a + b b (Math.hypot's value).
#[inline]
pub(crate) fn len2(a: f32, b: f32) -> f32 {
    (a * a + b * b).sqrt()
}

/// capsuleSweep: the time (0..1) at which the capsule whose centre moves a -> b first touches the box, None for none;
/// 0 when it starts in touch.
#[allow(clippy::too_many_arguments)]
pub fn capsule_sweep(
    ax: impl Into<f64>,
    ay: impl Into<f64>,
    bx: impl Into<f64>,
    by: impl Into<f64>,
    r: impl Into<f64>,
    hh: impl Into<f64>,
    g: &Ground,
) -> Option<f64> {
    sweep(arg(ax), arg(ay), arg(bx), arg(by), arg(r), arg(hh), &g.narrow()).map(out)
}

/// capsuleSweep in float32.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sweep(ax: f32, ay: f32, bx: f32, by: f32, r: f32, hh: f32, g: &Box2) -> Option<f32> {
    let s = max(hh - r, 0.0);
    let (l, rt, b, t) = (g.left, g.right, g.bottom - s, g.top + s);
    if len2(max3(l - ax, 0.0, ax - rt), max3(b - ay, 0.0, ay - t)) <= r {
        return Some(0.0);
    }
    let (dx, dy) = (bx - ax, by - ay);
    let mut slab = [0.0f32, 1.0];
    if !slab_axis(&mut slab, ax, dx, l - r, rt + r) || !slab_axis(&mut slab, ay, dy, b - r, t + r) {
        return None;
    }
    let t0 = slab[0];
    let (ex, ey) = (ax + dx * t0, ay + dy * t0);
    if (ex >= l && ex <= rt) || (ey >= b && ey <= t) {
        return Some(t0);
    }
    // a corner zone: the arc about the box's corner
    let cx = if ex < l { l } else { rt };
    let cy = if ey < b { b } else { t };
    let (ox, oy) = (ax - cx, ay - cy);
    let qa = dx * dx + dy * dy;
    let qb = 2.0 * (ox * dx + oy * dy);
    let qc = ox * ox + oy * oy - r * r;
    let disc = qb * qb - 4.0 * qa * qc;
    if qa == 0.0 || disc < 0.0 {
        return None;
    }
    let k = (-qb - disc.sqrt()) / (2.0 * qa);
    if (0.0..=1.0).contains(&k) { Some(k) } else { None }
}

// One axis of the slab test (slab): narrows the interval, false when the move misses the slab.
fn slab_axis(slab: &mut [f32; 2], p: f32, d: f32, lo: f32, hi: f32) -> bool {
    if d == 0.0 {
        return !(p < lo || p > hi);
    }
    let mut u = (lo - p) / d;
    let mut v = (hi - p) / d;
    if u > v {
        std::mem::swap(&mut u, &mut v);
    }
    if u > slab[0] {
        slab[0] = u;
    }
    if v < slab[1] {
        slab[1] = v;
    }
    slab[0] <= slab[1]
}
