//! The stage solids' queries: a solid's section by the play plane (`section`),
//! the flat-base floor on a polygon section (`poly_floor`), the least way out of one (`poly_out`), and capsuleSweep
//! over a list (`capsule_sweep_all`). The fighters' reactions to them (outOfPolys: the landing, the step up, the
//! slide) stay in the sim: they take these answers. The port's own geometry standing in for PhysX's queries on the
//! solids, in float32 in a fixed order of operations.

use crate::capsule::{len2, sweep};
use crate::{Ground, arg, max, min, out};
use mvs_math::f32::{fmath, vector};

const EPS: f32 = 1e-3;
/// WALKABLE_Z: the CMC's WalkableFloorZ (a float property: 0.71 as float32).
pub const WALKABLE_Z: f64 = 0.71_f32 as f64;
const WALKABLE: f32 = 0.71;

/// StageSolidPoly's section: its corners counter-clockwise at the data place (the StageSolid's dx / dy come with
/// each query).
#[derive(Clone, Debug, Default)]
pub struct Poly {
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
}

/// polyFloor: whether the flat base of half width r at x finds a walkable floor on p moved by (ox, oy), and the floor's
/// height (FLOOR_H[0]: -Infinity where the base misses the polygon).
// Unverified: the base's half width is r, as the box rule's (the measured 74.02 past a box's edge at R 74), not the
// shrunk sweep's r - 0.1501 (IsWithinEdgeTolerance's resweep); a corner the base's end lies on is walkable.
pub fn poly_floor(
    p: &Poly,
    ox: impl Into<f64>,
    oy: impl Into<f64>,
    x: impl Into<f64>,
    r: impl Into<f64>,
) -> (bool, f64) {
    let (ox, oy, x, r) = (arg(ox), arg(oy), arg(x), arg(r));
    let xs: Vec<f32> = p.xs.iter().map(|&v| arg(v)).collect();
    let ys: Vec<f32> = p.ys.iter().map(|&v| arg(v)).collect();
    let n = xs.len();
    let (a, b) = (x - r - ox, x + r - ox);
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for &v in xs.iter() {
        lo = min(lo, v);
        hi = max(hi, v);
    }
    if b < lo || a > hi {
        return (false, f64::NEG_INFINITY);
    }
    let (mut h, mut walk) = (f32::NEG_INFINITY, true);
    for i in 0..n {
        if xs[i] >= a && xs[i] <= b && ys[i] > h {
            h = ys[i];
        }
    }
    // the base's ends inside the polygon: its upper edge there
    for k in 0..2 {
        let e = if k == 1 { b } else { a };
        if e <= lo || e >= hi {
            continue;
        }
        let (mut ey, mut eny) = (f32::NEG_INFINITY, 0.0f32);
        for i in 0..n {
            let j = (i + 1) % n;
            let ex = xs[j] - xs[i];
            if ex >= 0.0 {
                continue; // the upper chain runs right to left (counter-clockwise)
            }
            if e > xs[i] || e < xs[j] {
                continue;
            }
            let y = ys[i] + ((ys[j] - ys[i]) * (e - xs[i])) / ex;
            let ny = -ex / len2(ex, ys[j] - ys[i]);
            if y > ey + EPS || ((y - ey).abs() <= EPS && ny > eny) {
                ey = max(ey, y);
                eny = ny;
            }
        }
        if ey > h + EPS {
            h = ey;
            walk = eny >= WALKABLE;
        }
    }
    (walk, out(h + oy))
}

/// polyOut's answer: the depth of the least move out (None: no overlap), its direction (OUT_N) and the contact's
/// height (OUT_C[0]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Out {
    pub depth: f64,
    pub nx: f64,
    pub ny: f64,
    pub cy: f64,
}

/// polyOut: the least move that takes the upright capsule (centre (x, y), radius r, axis reaching hs up and down) out of
/// the polygon moved by (ox, oy); None when they do not overlap (-1 in the sim).
#[allow(clippy::too_many_arguments)]
pub fn poly_out(
    p: &Poly,
    ox: impl Into<f64>,
    oy: impl Into<f64>,
    x: impl Into<f64>,
    y: impl Into<f64>,
    r: impl Into<f64>,
    hs: impl Into<f64>,
) -> Option<Out> {
    let (ox, oy, x, y, r, hs) = (arg(ox), arg(oy), arg(x), arg(y), arg(r), arg(hs));
    let xs: Vec<f32> = p.xs.iter().map(|&v| arg(v)).collect();
    let ys: Vec<f32> = p.ys.iter().map(|&v| arg(v)).collect();
    let n = xs.len();
    let (y0, y1) = (y - hs, y + hs);
    if !segment_meets(&xs, &ys, ox, oy, x, y0, y1) {
        // apart: along the line of the nearest points (a corner against the axis, an end of the axis against an edge)
        let (mut best, mut bx, mut by, mut cy0) = (f32::INFINITY, 0.0f32, 0.0f32, 0.0f32);
        for i in 0..n {
            let (ax, ay) = (xs[i] + ox, ys[i] + oy);
            let (cx, cy) = (xs[(i + 1) % n] + ox, ys[(i + 1) % n] + oy);
            let py = max(y0, min(y1, ay));
            let mut d2 = (x - ax) * (x - ax) + (py - ay) * (py - ay);
            if d2 < best {
                best = d2;
                bx = x - ax;
                by = py - ay;
                cy0 = ay;
            }
            let (ex, ey) = (cx - ax, cy - ay);
            let ee = ex * ex + ey * ey;
            for k in 0..2 {
                let sy = if k == 1 { y1 } else { y0 };
                let t = if ee > 0.0 { max(0.0, min(1.0, ((x - ax) * ex + (sy - ay) * ey) / ee)) } else { 0.0 };
                let (qx, qy) = (ax + ex * t, ay + ey * t);
                d2 = (x - qx) * (x - qx) + (sy - qy) * (sy - qy);
                if d2 < best {
                    best = d2;
                    bx = x - qx;
                    by = sy - qy;
                    cy0 = qy;
                }
            }
        }
        let d = len2(bx, by);
        if d >= r {
            return None;
        }
        return Some(Out {
            depth: out(r - d),
            nx: out(if d > 0.0 { bx / d } else { 0.0 }),
            ny: out(if d > 0.0 { by / d } else { 1.0 }),
            cy: out(cy0),
        });
    }
    // the separating axes: the edges' outward normals and the play plane's axes; the least depth
    let (mut pen, mut nx, mut ny) = (f32::INFINITY, 0.0f32, 0.0f32);
    let mut axis = |dx: f32, dy: f32| {
        let mut mx = f32::NEG_INFINITY;
        for i in 0..n {
            mx = max(mx, (xs[i] + ox) * dx + (ys[i] + oy) * dy);
        }
        let k = mx - (min(x * dx + y0 * dy, x * dx + y1 * dy) - r);
        if k < pen {
            pen = k;
            nx = dx;
            ny = dy;
        }
    };
    axis(1.0, 0.0);
    axis(-1.0, 0.0);
    axis(0.0, 1.0);
    axis(0.0, -1.0);
    for i in 0..n {
        let (ex, ey) = (xs[(i + 1) % n] - xs[i], ys[(i + 1) % n] - ys[i]);
        let l = len2(ex, ey);
        if l > 0.0 {
            axis(ey / l, -ex / l);
        }
    }
    let cy = if ny < 0.0 {
        y1
    } else if ny > 0.0 {
        y0
    } else {
        y
    };
    Some(Out { depth: out(pen), nx: out(nx), ny: out(ny), cy: out(cy) })
}

// segmentMeets: whether the vertical segment (x, y0..y1) meets the polygon.
#[allow(clippy::too_many_arguments)]
fn segment_meets(xs: &[f32], ys: &[f32], ox: f32, oy: f32, x: f32, y0: f32, y1: f32) -> bool {
    let n = xs.len();
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for i in 0..n {
        let (ax, ay) = (xs[i] + ox, ys[i] + oy);
        let (cx, cy) = (xs[(i + 1) % n] + ox, ys[(i + 1) % n] + oy);
        if ax == x {
            lo = min(lo, ay);
            hi = max(hi, ay);
        }
        if (ax - x) * (cx - x) >= 0.0 || ax == cx {
            continue;
        }
        let yy = ay + ((cy - ay) * (x - ax)) / (cx - ax);
        lo = min(lo, yy);
        hi = max(hi, yy);
    }
    lo <= y1 && hi >= y0
}

/// capsuleSweepAll: capsuleSweep over a list, the first contact's time.
#[allow(clippy::too_many_arguments)]
pub fn capsule_sweep_all(
    ax: impl Into<f64>,
    ay: impl Into<f64>,
    bx: impl Into<f64>,
    by: impl Into<f64>,
    r: impl Into<f64>,
    hh: impl Into<f64>,
    gs: &[Ground],
) -> Option<f64> {
    let (ax, ay, bx, by, r, hh) = (arg(ax), arg(ay), arg(bx), arg(by), arg(r), arg(hh));
    let mut best: Option<f32> = None;
    for g in gs {
        if let Some(t) = sweep(ax, ay, bx, by, r, hh, &g.narrow())
            && best.is_none_or(|b| t < b)
        {
            best = Some(t);
        }
    }
    best.map(out)
}

// ------------------------------------------------------------------ the polygon queries of the box readers

/// A polygon's corners in float32, moved by (ox, oy), read in place.
#[derive(Clone, Copy)]
pub(crate) struct Corners<'a> {
    p: &'a Poly,
    ox: f32,
    oy: f32,
}

impl Corners<'_> {
    pub(crate) fn n(self) -> usize {
        self.p.xs.len()
    }

    pub(crate) fn x(self, i: usize) -> f32 {
        arg(self.p.xs[i]) + self.ox
    }

    pub(crate) fn y(self, i: usize) -> f32 {
        arg(self.p.ys[i]) + self.oy
    }
}

pub(crate) fn corners(p: &Poly, ox: f32, oy: f32) -> Corners<'_> {
    Corners { p, ox, oy }
}

/// Whether (x, y) is inside the counter-clockwise corners (on an edge counts).
pub(crate) fn inside(c: Corners, x: f32, y: f32) -> bool {
    let n = c.n();
    (0..n).all(|i| {
        let j = (i + 1) % n;
        (c.x(j) - c.x(i)) * (y - c.y(i)) - (c.y(j) - c.y(i)) * (x - c.x(i)) >= 0.0
    })
}

/// The point of the segment a -> b nearest (x, y).
fn seg_nearest(ax: f32, ay: f32, bx: f32, by: f32, x: f32, y: f32) -> (f32, f32) {
    let (ex, ey) = (bx - ax, by - ay);
    let ee = ex * ex + ey * ey;
    let t = if ee > 0.0 { max(0.0, min(1.0, ((x - ax) * ex + (y - ay) * ey) / ee)) } else { 0.0 };
    (ax + ex * t, ay + ey * t)
}

/// The point of the corners' outline nearest (x, y) (inside or out).
fn edge_nearest(c: Corners, x: f32, y: f32) -> (f32, f32) {
    let (_, qx, qy) = edge_nearest_at(c, x, y);
    (qx, qy)
}

/// [`edge_nearest`] with the edge it lies on (from corner i to the next; the first of equal distances).
pub(crate) fn edge_nearest_at(c: Corners, x: f32, y: f32) -> (usize, f32, f32) {
    let n = c.n();
    let (mut best, mut q) = (f32::INFINITY, (0, x, y));
    for i in 0..n {
        let j = (i + 1) % n;
        let (qx, qy) = seg_nearest(c.x(i), c.y(i), c.x(j), c.y(j), x, y);
        let d2 = (x - qx) * (x - qx) + (y - qy) * (y - qy);
        if d2 < best {
            best = d2;
            q = (i, qx, qy);
        }
    }
    q
}

/// The outward unit normal of the corners' edge from corner i to the next ((0, 1) for an edge of no length).
pub(crate) fn edge_normal(c: Corners, i: usize) -> (f32, f32) {
    let j = (i + 1) % c.n();
    let (ex, ey) = (c.x(j) - c.x(i), c.y(j) - c.y(i));
    let l = len2(ex, ey);
    if l > 0.0 { (ey / l, -ex / l) } else { (0.0, 1.0) }
}

/// The box round the corners (left, right, bottom, top).
pub(crate) fn bounds(c: Corners) -> (f32, f32, f32, f32) {
    let (mut l, mut r, mut b, mut t) = (f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY);
    for i in 0..c.n() {
        l = min(l, c.x(i));
        r = max(r, c.x(i));
        b = min(b, c.y(i));
        t = max(t, c.y(i));
    }
    (l, r, b, t)
}

/// polyInside: whether (x, y) is inside the polygon moved by (ox, oy) (on its outline counts): the polygon's stand-in
/// for a box reader's `left <= x <= right && bottom <= y <= top`.
pub fn poly_inside(p: &Poly, ox: impl Into<f64>, oy: impl Into<f64>, x: impl Into<f64>, y: impl Into<f64>) -> bool {
    inside(corners(p, arg(ox), arg(oy)), arg(x), arg(y))
}

/// polyBounds: the box round the polygon moved by (ox, oy).
pub fn poly_bounds(p: &Poly, ox: impl Into<f64>, oy: impl Into<f64>) -> Ground {
    let (l, r, b, t) = bounds(corners(p, arg(ox), arg(oy)));
    Ground { left: out(l), right: out(r), bottom: out(b), top: out(t) }
}

/// polyClosest: the point of the polygon moved by (ox, oy) nearest (x, y): (x, y) itself inside it. The polygon's
/// stand-in for a box reader's clamp of a point to the box (`max(left, min(x, right))`, the same for y).
pub fn poly_closest(
    p: &Poly,
    ox: impl Into<f64>,
    oy: impl Into<f64>,
    x: impl Into<f64>,
    y: impl Into<f64>,
) -> (f64, f64) {
    let (ox, oy, x, y) = (arg(ox), arg(oy), arg(x), arg(y));
    let c = corners(p, ox, oy);
    if inside(c, x, y) {
        return (out(x), out(y));
    }
    let (qx, qy) = edge_nearest(c, x, y);
    (out(qx), out(qy))
}

/// polyTop: the polygon's upper outline at x (moved by (ox, oy)), its top corner where x is outside its span: the
/// polygon's stand-in for a box's top in the box readers' "a point inside leaves through the top" rule.
pub fn poly_top(p: &Poly, ox: impl Into<f64>, oy: impl Into<f64>, x: impl Into<f64>) -> f64 {
    let (ox, oy, x) = (arg(ox), arg(oy), arg(x));
    let c = corners(p, ox, oy);
    let n = c.n();
    let (mut h, mut top) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for i in 0..n {
        let j = (i + 1) % n;
        top = max(top, c.y(i));
        let (lo, hi) = (min(c.x(i), c.x(j)), max(c.x(i), c.x(j)));
        if x < lo || x > hi {
            continue;
        }
        let y = if c.x(j) == c.x(i) {
            max(c.y(i), c.y(j))
        } else {
            c.y(i) + (c.y(j) - c.y(i)) * (x - c.x(i)) / (c.x(j) - c.x(i))
        };
        h = max(h, y);
    }
    out(if h == f32::NEG_INFINITY { top } else { h })
}

/// Whether the segment a -> b meets the corners' polygon.
fn seg_meets(c: Corners, ax: f32, ay: f32, bx: f32, by: f32) -> bool {
    if inside(c, ax, ay) || inside(c, bx, by) {
        return true;
    }
    let n = c.n();
    let side = |px: f32, py: f32, qx: f32, qy: f32, x: f32, y: f32| (qx - px) * (y - py) - (qy - py) * (x - px);
    (0..n).any(|i| {
        let j = (i + 1) % n;
        let (d1, d2) = (side(ax, ay, bx, by, c.x(i), c.y(i)), side(ax, ay, bx, by, c.x(j), c.y(j)));
        let (d3, d4) = (side(c.x(i), c.y(i), c.x(j), c.y(j), ax, ay), side(c.x(i), c.y(i), c.x(j), c.y(j), bx, by));
        d1 * d2 <= 0.0 && d3 * d4 <= 0.0 && !(d1 == 0.0 && d2 == 0.0)
    })
}

/// capsulePolyMtd: [`crate::capsule_mtd::capsule_mtd`]'s rule against the polygon moved by (ox, oy): with the
/// capsule's axis (centre (x, y), half length `h` along (ux, uy), radius `r`) outside the polygon, along the line of
/// the two nearest points by the radius less their distance; with the axis through it, the least of the ways out
/// along the polygon's edge normals and across the capsule's own axis, plus the radius.
#[allow(clippy::too_many_arguments, reason = "a polygon, its move, a place and the capsule's shape")]
pub fn capsule_poly_mtd(
    p: &Poly,
    ox: impl Into<f64>,
    oy: impl Into<f64>,
    x: impl Into<f64>,
    y: impl Into<f64>,
    r: impl Into<f64>,
    h: impl Into<f64>,
    ux: impl Into<f64>,
    uy: impl Into<f64>,
) -> crate::capsule_mtd::Mtd {
    let (ox, oy, x, y, r, h, ux, uy) = (arg(ox), arg(oy), arg(x), arg(y), arg(r), arg(h), arg(ux), arg(uy));
    let c = corners(p, ox, oy);
    let n = c.n();
    let (ax, ay, bx, by) = (x - ux * h, y - uy * h, x + ux * h, y + uy * h);
    if seg_meets(c, ax, ay, bx, by) {
        let (mut pen, mut nx, mut ny) = (f32::INFINITY, 0.0f32, 1.0f32);
        let mut axis = |dx: f32, dy: f32| {
            let mut mx = f32::NEG_INFINITY;
            for i in 0..n {
                mx = max(mx, c.x(i) * dx + c.y(i) * dy);
            }
            let k = mx - min(ax * dx + ay * dy, bx * dx + by * dy);
            if k < pen {
                pen = k;
                nx = dx;
                ny = dy;
            }
        };
        for i in 0..n {
            let j = (i + 1) % n;
            let (ex, ey) = (c.x(j) - c.x(i), c.y(j) - c.y(i));
            let l = len2(ex, ey);
            if l > 0.0 {
                axis(ey / l, -ex / l);
            }
        }
        if h > 1e-4 {
            axis(-uy, ux);
            axis(uy, -ux);
        }
        return crate::capsule_mtd::Mtd { nx: out(nx), ny: out(ny), depth: out(pen + r) };
    }
    let (mut d2, mut vx, mut vy) = (f32::INFINITY, 0.0f32, 1.0f32);
    for (px, py) in [(ax, ay), (bx, by)] {
        let (qx, qy) = edge_nearest(c, px, py);
        let q = (px - qx) * (px - qx) + (py - qy) * (py - qy);
        if q < d2 {
            d2 = q;
            vx = px - qx;
            vy = py - qy;
        }
    }
    for i in 0..n {
        let (sx, sy) = seg_nearest(ax, ay, bx, by, c.x(i), c.y(i));
        let q = (sx - c.x(i)) * (sx - c.x(i)) + (sy - c.y(i)) * (sy - c.y(i));
        if q < d2 {
            d2 = q;
            vx = sx - c.x(i);
            vy = sy - c.y(i);
        }
    }
    let d = d2.sqrt();
    crate::capsule_mtd::Mtd {
        nx: out(if d > 0.0 { vx / d } else { 0.0 }),
        ny: out(if d > 0.0 { vy / d } else { 1.0 }),
        depth: out(max(r - d, 0.0)),
    }
}

// ------------------------------------------------------------------ the section by the play plane

/// StageSolidData's shape, as section reads it.
#[derive(Clone, Debug)]
pub enum SolidShape {
    /// kind 'box': centre and half extent
    Box { c: [f64; 3], e: [f64; 3] },
    /// kind 'obox': and its rotation (pitch, yaw, roll)
    OBox { c: [f64; 3], e: [f64; 3], rot: [f64; 3] },
    /// any other kind: its points (x y z after one another; none for a capsule or a sphere)
    Points { c: [f64; 3], e: [f64; 3], points: Vec<f64> },
}

/// A section: a rectangle on the axes (box: left, right, bottom, top) or the convex polygon (xs, ys counter-clockwise).
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub box_: Option<[f64; 4]>,
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
}

/// section: the solid's section by X = 0 in sim space; None where it does not reach the plane.
pub fn section(d: &SolidShape) -> Option<Section> {
    let widen = |v: &[f32]| v.iter().map(|&x| out(x)).collect::<Vec<f64>>();
    let (corners, edges): (Vec<[f32; 3]>, Vec<(usize, usize)>) = match d {
        SolidShape::Box { c, e } => {
            let (c, e) = (c.map(arg), e.map(arg));
            if c[0].abs() > e[0] {
                return None;
            }
            let b = [-(c[1] + e[1]), -(c[1] - e[1]), c[2] - e[2], c[2] + e[2]];
            return Some(Section {
                box_: Some(b.map(out)),
                xs: widen(&[b[0], b[1], b[1], b[0]]),
                ys: widen(&[b[2], b[2], b[3], b[3]]),
            });
        }
        SolidShape::OBox { c, e, rot } => {
            let (c, e) = (c.map(arg), e.map(arg));
            // Unverified: the box's axes turned by FRotator::Quaternion and FQuat::RotateVector; PhysX builds the
            // shape from that quaternion and turns its corners with its own PxQuat::rotate (operation order unread).
            let q = fmath::rotator_quaternion(rot.map(arg));
            let ax: Vec<[f32; 3]> = (0..3)
                .map(|j| {
                    vector::quat_rotate_vector(
                        q,
                        [
                            if j == 0 { e[0] } else { 0.0 },
                            if j == 1 { e[1] } else { 0.0 },
                            if j == 2 { e[2] } else { 0.0 },
                        ],
                    )
                })
                .collect();
            let mut corners = Vec::with_capacity(8);
            for k in 0..8 {
                let s: [f32; 3] = [
                    if k & 1 != 0 { 1.0 } else { -1.0 },
                    if k & 2 != 0 { 1.0 } else { -1.0 },
                    if k & 4 != 0 { 1.0 } else { -1.0 },
                ];
                let mut v = [0.0f32; 3];
                for i in 0..3 {
                    v[i] = c[i] + s[0] * ax[0][i] + s[1] * ax[1][i] + s[2] * ax[2][i];
                }
                corners.push(v);
            }
            let mut edges = Vec::new();
            for k in 0..8usize {
                for b in [1usize, 2, 4] {
                    if k & b == 0 {
                        edges.push((k, k | b));
                    }
                }
            }
            (corners, edges)
        }
        SolidShape::Points { points: p, .. } => {
            // a convex hull: its section is the hull of the crossings of the segments between its points (all pairs)
            let mut corners = Vec::new();
            let mut i = 0;
            while i + 2 < p.len() {
                corners.push([arg(p[i]), arg(p[i + 1]), arg(p[i + 2])]);
                i += 3;
            }
            let mut edges = Vec::new();
            for i in 0..corners.len() {
                for j in i + 1..corners.len() {
                    edges.push((i, j));
                }
            }
            (corners, edges)
        }
    };
    let mut pts: Vec<[f32; 2]> = Vec::new(); // UE (Y, Z) of the section's corners
    for v in &corners {
        if v[0].abs() < EPS {
            pts.push([v[1], v[2]]);
        }
    }
    for &(i, j) in &edges {
        let (a, b) = (corners[i], corners[j]);
        if (a[0] < -EPS && b[0] > EPS) || (a[0] > EPS && b[0] < -EPS) {
            let t = a[0] / (a[0] - b[0]);
            pts.push([a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]);
        }
    }
    let pts = hull(pts.iter().map(|&[y, z]| [-y, z]).collect());
    if pts.len() < 3 {
        return None;
    }
    let xs: Vec<f32> = pts.iter().map(|v| v[0]).collect();
    let ys: Vec<f32> = pts.iter().map(|v| v[1]).collect();
    // a rectangle on the axes is the box it is
    if pts.len() == 4
        && pts.iter().enumerate().all(|(i, v)| {
            let w = pts[(i + 1) % 4];
            (v[0] - w[0]).abs() < EPS || (v[1] - w[1]).abs() < EPS
        })
    {
        let fold =
            |a: &[f32], lo: bool| {
                a.iter().fold(if lo { f32::INFINITY } else { f32::NEG_INFINITY }, |m, &v| {
                    if lo { min(m, v) } else { max(m, v) }
                })
            };
        let b = [fold(&xs, true), fold(&xs, false), fold(&ys, true), fold(&ys, false)];
        return Some(Section { box_: Some(b.map(out)), xs: widen(&xs), ys: widen(&ys) });
    }
    Some(Section { box_: None, xs: widen(&xs), ys: widen(&ys) })
}

// Andrew's monotone chain: the convex hull counter-clockwise, the points on its edges dropped.
fn hull(p: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    let mut s = p;
    // (a[0] - b[0] || a[1] - b[1]: a stable sort, as V8's)
    s.sort_by(|a, b| {
        let d = a[0] - b[0];
        let k = if d != 0.0 && !d.is_nan() { d } else { a[1] - b[1] };
        k.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    if s.len() < 3 {
        return s;
    }
    let cross =
        |o: &[f32; 2], a: &[f32; 2], b: &[f32; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let (mut lo, mut hi): (Vec<[f32; 2]>, Vec<[f32; 2]>) = (Vec::new(), Vec::new());
    for v in &s {
        while lo.len() >= 2 && cross(&lo[lo.len() - 2], &lo[lo.len() - 1], v) <= EPS {
            lo.pop();
        }
        lo.push(*v);
    }
    for v in s.iter().rev() {
        while hi.len() >= 2 && cross(&hi[hi.len() - 2], &hi[hi.len() - 1], v) <= EPS {
            hi.pop();
        }
        hi.push(*v);
    }
    lo.pop();
    hi.pop();
    lo.extend(hi);
    lo
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A diamond about (0, 0) of half size 100, moved by (1000, 0).
    fn diamond() -> Poly {
        Poly { xs: vec![0.0, 100.0, 0.0, -100.0], ys: vec![-100.0, 0.0, 100.0, 0.0] }
    }

    #[test]
    fn closest_point_is_the_point_inside_and_on_the_edge_outside() {
        let p = diamond();
        assert_eq!(poly_closest(&p, 1000.0, 0.0, 1010.0, 10.0), (1010.0, 10.0));
        let (qx, qy) = poly_closest(&p, 1000.0, 0.0, 1100.0, 100.0);
        assert!((qx - 1050.0).abs() < 1e-3 && (qy - 50.0).abs() < 1e-3, "({qx}, {qy})");
        assert_eq!(poly_closest(&p, 1000.0, 0.0, 1000.0, 300.0), (1000.0, 100.0));
    }

    #[test]
    fn top_is_the_upper_outline() {
        let p = diamond();
        assert_eq!(poly_top(&p, 1000.0, 0.0, 1000.0), 100.0);
        assert_eq!(poly_top(&p, 1000.0, 0.0, 1050.0), 50.0);
    }

    #[test]
    fn a_sphere_on_the_top_goes_out_upwards() {
        let p = diamond();
        // a sphere of radius 20 whose centre is 10 above the top corner: out by 10 along the line of the nearest points
        let m = capsule_poly_mtd(&p, 1000.0, 0.0, 1000.0, 110.0, 20.0, 0.0, 0.0, 1.0);
        assert!(m.nx.abs() < 1e-6 && (m.ny - 1.0).abs() < 1e-6 && (m.depth - 10.0).abs() < 1e-4, "{m:?}");
        // its centre inside, near the upper right edge: out across that edge, depth past it plus the radius
        let m = capsule_poly_mtd(&p, 1000.0, 0.0, 1040.0, 40.0, 20.0, 0.0, 0.0, 1.0);
        let s = std::f64::consts::FRAC_1_SQRT_2;
        assert!((m.nx - s).abs() < 1e-4 && (m.ny - s).abs() < 1e-4, "{m:?}");
        assert!((m.depth - (20.0 * s + 20.0)).abs() < 1e-3, "{m:?}");
    }
}
