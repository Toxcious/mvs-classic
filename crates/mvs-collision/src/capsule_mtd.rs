//! The least move that takes a capsule (or a sphere) out of a box it overlaps: a
//! projectile's start in penetration and the spawn spot's adjustment of a sphere. The port's own geometry standing in
//! for PhysX's MTD (computePenetration), in float32 in a fixed order of operations.

use crate::Ground;
use crate::{arg, max, min, out};

/// `Mtd`: the way out's direction and its depth.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mtd {
    pub nx: f64,
    pub ny: f64,
    pub depth: f64,
}

/// `capsuleMtd`: the least move that takes a capsule at (x, y) out of a box it overlaps. With the capsule's axis
/// outside the box: along the line of the two nearest points (an end of the axis against the box, or a corner
/// against the axis), by the radius less their distance. With the axis through the box: the least of the ways out
/// along the box's two axes and across the capsule's own, plus the radius. `r`: the radius, `h`: the half length of
/// its axis (0: a sphere), (`ux`, `uy`): the axis's direction.
#[allow(clippy::too_many_arguments, reason = "the signature: a box, a place and the capsule's shape")]
pub fn capsule_mtd(
    b: &Ground,
    x: impl Into<f64>,
    y: impl Into<f64>,
    r: impl Into<f64>,
    h: impl Into<f64>,
    ux: impl Into<f64>,
    uy: impl Into<f64>,
) -> Mtd {
    let (x, y, r, h, ux, uy) = (arg(x), arg(y), arg(r), arg(h), arg(ux), arg(uy));
    let b = b.narrow();
    let ax = x - ux * h;
    let ay = y - uy * h;
    let bx = x + ux * h;
    let by = y + uy * h;
    let ex = bx - ax;
    let ey = by - ay;
    let mut t0: f32 = 0.0;
    let mut t1: f32 = 1.0;
    let mut through = true;
    if ex == 0.0 {
        through = ax >= b.left && ax <= b.right;
    } else {
        let p = (b.left - ax) / ex;
        let q = (b.right - ax) / ex;
        t0 = max(t0, min(p, q));
        t1 = min(t1, max(p, q));
    }
    if ey == 0.0 {
        through = through && ay >= b.bottom && ay <= b.top;
    } else {
        let p = (b.bottom - ay) / ey;
        let q = (b.top - ay) / ey;
        t0 = max(t0, min(p, q));
        t1 = min(t1, max(p, q));
    }
    if through && t0 <= t1 {
        let mut pen = b.right - min(ax, bx);
        let mut nx = 1.0f32;
        let mut ny = 0.0f32;
        let mut q = max(ax, bx) - b.left;
        if q < pen {
            pen = q;
            nx = -1.0;
        }
        q = b.top - min(ay, by);
        if q < pen {
            pen = q;
            nx = 0.0;
            ny = 1.0;
        }
        q = max(ay, by) - b.bottom;
        if q < pen {
            pen = q;
            nx = 0.0;
            ny = -1.0;
        }
        if h > 1e-4 {
            let s = -uy * x + ux * y;
            let mid = -uy * (b.left + b.right) * 0.5 + ux * (b.bottom + b.top) * 0.5;
            let ext = uy.abs() * (b.right - b.left) * 0.5 + ux.abs() * (b.top - b.bottom) * 0.5;
            q = mid + ext - s;
            if q < pen {
                pen = q;
                nx = -uy;
                ny = ux;
            }
            q = s - (mid - ext);
            if q < pen {
                pen = q;
                nx = uy;
                ny = -ux;
            }
        }
        return Mtd { nx: out(nx), ny: out(ny), depth: out(pen + r) };
    }
    let mut d2 = f32::INFINITY;
    let mut vx = 0.0f32;
    let mut vy = 1.0f32;
    for (px, py) in [(ax, ay), (bx, by)] {
        let qx = px - max(b.left, min(px, b.right));
        let qy = py - max(b.bottom, min(py, b.top));
        let q = qx * qx + qy * qy;
        if q < d2 {
            d2 = q;
            vx = qx;
            vy = qy;
        }
    }
    let e2 = ex * ex + ey * ey;
    if e2 > 0.0 {
        for i in 0..4 {
            let cx = if i & 1 != 0 { b.right } else { b.left };
            let cy = if i & 2 != 0 { b.top } else { b.bottom };
            let t = min(max(((cx - ax) * ex + (cy - ay) * ey) / e2, 0.0), 1.0);
            let qx = ax + ex * t - cx;
            let qy = ay + ey * t - cy;
            let q = qx * qx + qy * qy;
            if q < d2 {
                d2 = q;
                vx = qx;
                vy = qy;
            }
        }
    }
    let d = d2.sqrt();
    Mtd {
        nx: out(if d > 0.0 { vx / d } else { 0.0 }),
        ny: out(if d > 0.0 { vy / d } else { 1.0 }),
        depth: out(max(r - d, 0.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sphere_inside_box_goes_out_of_the_nearer_face() {
        let b = Ground { left: -100.0, right: 100.0, bottom: -50.0, top: 0.0 };
        let m = capsule_mtd(&b, 10.0, -5.0, 20.0, 0.0, 0.0, 1.0);
        assert_eq!((m.nx, m.ny, m.depth), (0.0, 1.0, 25.0));
        let m = capsule_mtd(&b, 0.0, 10.0, 20.0, 0.0, 0.0, 1.0);
        assert_eq!((m.nx, m.ny, m.depth), (0.0, 1.0, 10.0));
    }
}
