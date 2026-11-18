//! The tests of hit shapes against hurt shapes: the overlap math, without the
//! BoxSampler's sampling of the per-frame shapes. The port's own geometry standing in for PhysX's overlap queries, in
//! float32 in a fixed order of operations.

use crate::{arg, max, min};

/// WorldSphere: a hit shape in world units. z: depth (None: extruded along the depth); box_: a BoxComponent's three
/// half extent vectors; seg: a capsule hit shape's second end.
#[derive(Clone, Debug, Default)]
pub struct Sphere {
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
    pub r: f64,
    pub box_: Option<[f64; 9]>,
    pub seg: Option<[f64; 3]>,
}

/// ExtraHurt: a hurtbox capsule on the root capsule.
#[derive(Clone, Copy, Debug, Default)]
pub struct Extra {
    pub x: f64,
    pub y0: f64,
    pub y1: f64,
    pub r: f64,
}

/// What the tests read of a BoxSampler: per shape its capsule flag, radius and pose offset (index.shapes, offsets),
/// and the hurt shapes (hurt).
#[derive(Clone, Debug, Default)]
pub struct Sampler {
    pub capsule: Vec<bool>,
    pub radius: Vec<f64>,
    pub offsets: Vec<usize>,
    pub hurt: Vec<usize>,
}

/// The hurt shapes without overlap events (a Set<number>; membership alone is read).
pub type Off<'a> = Option<&'a [usize]>;

fn is_off(off: Off, i: usize) -> bool {
    off.is_some_and(|o| o.contains(&i))
}

/// A `Sphere` narrowed to float32.
#[derive(Clone, Copy, Debug)]
struct Sh {
    x: f32,
    y: f32,
    z: Option<f32>,
    r: f32,
    box_: Option<[f32; 9]>,
    seg: Option<[f32; 3]>,
}

impl Sh {
    fn of(s: &Sphere) -> Sh {
        Sh {
            x: arg(s.x),
            y: arg(s.y),
            z: s.z.map(arg),
            r: arg(s.r),
            box_: s.box_.map(|b| b.map(arg)),
            seg: s.seg.map(|g| g.map(arg)),
        }
    }
}

/// An `Extra` narrowed to float32.
#[derive(Clone, Copy, Debug)]
struct Ex {
    x: f32,
    y0: f32,
    y1: f32,
    r: f32,
}

impl Ex {
    fn of(e: &Extra) -> Ex {
        Ex { x: arg(e.x), y0: arg(e.y0), y1: arg(e.y1), r: arg(e.r) }
    }
}

/// A `Victim` with its place narrowed to float32.
struct Vi<'a> {
    vic: &'a [f32],
    vx: f32,
    vy: f32,
    vf: f32,
    extra: Option<Ex>,
    off: Off<'a>,
}

impl<'a> Vi<'a> {
    fn of(v: &Victim<'a>) -> Vi<'a> {
        Vi { vic: v.vic, vx: arg(v.vx), vy: arg(v.vy), vf: arg(v.vf), extra: v.extra.map(Ex::of), off: v.off }
    }
    fn p(&self, k: usize) -> f32 {
        crate::ftz(self.vic[k])
    }
}

fn seg_dist2(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let l2 = dx * dx + dy * dy;
    let mut t = if l2 > 0.0 { ((px - ax) * dx + (py - ay) * dy) / l2 } else { 0.0 };
    t = max(0.0, min(1.0, t));
    let qx = ax + dx * t - px;
    let qy = ay + dy * t - py;
    qx * qx + qy * qy
}

/// segBoxDist2: squared distance from the segment a-b to the oriented box s (0 where they meet; Infinity for a box with
/// a zero extent).
#[allow(clippy::too_many_arguments)]
pub fn seg_box_dist2(
    s: &Sphere,
    ax: impl Into<f64>,
    ay: impl Into<f64>,
    az: impl Into<f64>,
    bx: impl Into<f64>,
    by: impl Into<f64>,
    bz: impl Into<f64>,
) -> f64 {
    crate::out(seg_box(&Sh::of(s), arg(ax), arg(ay), arg(az), arg(bx), arg(by), arg(bz)))
}

#[allow(clippy::too_many_arguments)]
fn seg_box(s: &Sh, ax: f32, ay: f32, az: f32, bx: f32, by: f32, bz: f32) -> f32 {
    let v = s.box_.as_ref().expect("segBoxDist2 on a shape without a box");
    let (px, py, pz) = (ax - s.x, ay - s.y, az - s.z.unwrap_or(0.0));
    let (dx, dy, dz) = (bx - ax, by - ay, bz - az);
    let mut cuts = [0.0f32; 8];
    let (mut la, mut ld, mut lh) = ([0.0f32; 3], [0.0f32; 3], [0.0f32; 3]);
    let mut n = 0;
    cuts[n] = 0.0;
    n += 1;
    for k in 0..3 {
        let (ux, uy, uz) = (v[k * 3], v[k * 3 + 1], v[k * 3 + 2]);
        let h = (ux * ux + uy * uy + uz * uz).sqrt();
        if h == 0.0 {
            return f32::INFINITY;
        }
        la[k] = (px * ux + py * uy + pz * uz) / h;
        ld[k] = (dx * ux + dy * uy + dz * uz) / h;
        lh[k] = h;
        if ld[k] != 0.0 {
            for sg in [-1.0f32, 1.0] {
                let t = (sg * h - la[k]) / ld[k];
                if t > 0.0 && t < 1.0 {
                    cuts[n] = t;
                    n += 1;
                }
            }
        }
    }
    cuts[n] = 1.0;
    n += 1;
    for i in 1..n {
        let t = cuts[i];
        let mut j = i as isize - 1;
        while j >= 0 && cuts[j as usize] > t {
            cuts[j as usize + 1] = cuts[j as usize];
            j -= 1;
        }
        cuts[(j + 1) as usize] = t;
    }
    let mut best = f32::INFINITY;
    for i in 0..n - 1 {
        let (t0, t1) = (cuts[i], cuts[i + 1]);
        let tm = (t0 + t1) / 2.0;
        let (mut num, mut den) = (0.0f32, 0.0f32);
        for k in 0..3 {
            let m = la[k] + tm * ld[k];
            let e = if m > lh[k] {
                lh[k]
            } else if m < -lh[k] {
                -lh[k]
            } else {
                continue;
            };
            num -= ld[k] * (la[k] - e);
            den += ld[k] * ld[k];
        }
        let t = if den > 0.0 { min(max(num / den, t0), t1) } else { t0 };
        let mut d2 = 0.0f32;
        for k in 0..3 {
            let m = la[k] + t * ld[k];
            let o = max(m.abs() - lh[k], 0.0);
            d2 += o * o;
        }
        if d2 < best {
            best = d2;
        }
    }
    best
}

#[allow(clippy::too_many_arguments)]
fn separates(u: &[f32; 9], v: &[f32; 9], tx: f32, ty: f32, tz: f32, lx: f32, ly: f32, lz: f32) -> bool {
    let mut r = 0.0f32;
    for k in 0..3 {
        r += (u[k * 3] * lx + u[k * 3 + 1] * ly + u[k * 3 + 2] * lz).abs();
        r += (v[k * 3] * lx + v[k * 3 + 1] * ly + v[k * 3 + 2] * lz).abs();
    }
    (tx * lx + ty * ly + tz * lz).abs() > r
}

/// boxBoxOverlap: whether two oriented boxes overlap (the separating axis test).
pub fn box_box_overlap(a: &Sphere, b: &Sphere) -> bool {
    box_box(&Sh::of(a), &Sh::of(b))
}

fn box_box(a: &Sh, b: &Sh) -> bool {
    let (u, v) = (a.box_.as_ref().expect("a box"), b.box_.as_ref().expect("a box"));
    for k in 0..3 {
        if u[k * 3] == 0.0 && u[k * 3 + 1] == 0.0 && u[k * 3 + 2] == 0.0 {
            return false;
        }
        if v[k * 3] == 0.0 && v[k * 3 + 1] == 0.0 && v[k * 3 + 2] == 0.0 {
            return false;
        }
    }
    let (tx, ty, tz) = (b.x - a.x, b.y - a.y, b.z.unwrap_or(0.0) - a.z.unwrap_or(0.0));
    for k in 0..3 {
        if separates(u, v, tx, ty, tz, u[k * 3], u[k * 3 + 1], u[k * 3 + 2]) {
            return false;
        }
        if separates(u, v, tx, ty, tz, v[k * 3], v[k * 3 + 1], v[k * 3 + 2]) {
            return false;
        }
    }
    for i in 0..3 {
        for j in 0..3 {
            let (ax, ay, az, bx, by, bz) = (u[i * 3], u[i * 3 + 1], u[i * 3 + 2], v[j * 3], v[j * 3 + 1], v[j * 3 + 2]);
            let (lx, ly, lz) = (ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx);
            if lx * lx + ly * ly + lz * lz <= 1e-6 * (ax * ax + ay * ay + az * az) * (bx * bx + by * by + bz * bz) {
                continue;
            }
            if separates(u, v, tx, ty, tz, lx, ly, lz) {
                return false;
            }
        }
    }
    true
}

/// shapeOverBox: a hit shape against a spawned actor's damage shape b (a box, or a sphere without one).
pub fn shape_over_box(s: &Sphere, b: &Sphere) -> bool {
    let (s, b) = (Sh::of(s), Sh::of(b));
    if b.box_.is_none() {
        let bz = b.z.unwrap_or(0.0);
        if s.box_.is_some() {
            return seg_box(&s, b.x, b.y, bz, b.x, b.y, bz) <= b.r * b.r;
        }
        let (dx, dy) = (s.x - b.x, s.y - b.y);
        let dz = match s.z {
            None => 0.0,
            Some(z) => z - bz,
        };
        let rr = s.r + b.r;
        return dx * dx + dy * dy + dz * dz <= rr * rr;
    }
    if s.box_.is_some() {
        return box_box(&s, &b);
    }
    match s.z {
        None => seg_box(&b, s.x, s.y, -1e5, s.x, s.y, 1e5) <= s.r * s.r,
        Some(z) => seg_box(&b, s.x, s.y, z, s.x, s.y, z) <= s.r * s.r,
    }
}

// hurtEnd: the offset in a pose of hurt shape i's second end (0 for a sphere).
fn hurt_end(b: &Sampler, i: usize) -> usize {
    if b.capsule[i] { 3 } else { 0 }
}
// hurtR: hurt shape i's radius in the pose.
fn hurt_r(b: &Sampler, i: usize, v: &Vi, o: usize) -> f32 {
    arg(b.radius[i]) * v.p(o + if b.capsule[i] { 6 } else { 3 })
}

fn hurt_dist2(s: &Sh, v: &Vi, o: usize, e: usize) -> f32 {
    let (ax, ay, bx, by) = (v.vx + v.p(o) * v.vf, v.vy + v.p(o + 1), v.vx + v.p(o + e) * v.vf, v.vy + v.p(o + e + 1));
    if s.box_.is_some() {
        return seg_box(s, ax, ay, v.p(o + 2), bx, by, v.p(o + e + 2));
    }
    let Some(sz) = s.z else {
        return seg_dist2(s.x, s.y, ax, ay, bx, by);
    };
    let (az, bz) = (v.p(o + 2), v.p(o + e + 2));
    let (dx, dy, dz) = (bx - ax, by - ay, bz - az);
    let l2 = dx * dx + dy * dy + dz * dz;
    let mut t = if l2 > 0.0 { ((s.x - ax) * dx + (s.y - ay) * dy + (sz - az) * dz) / l2 } else { 0.0 };
    t = max(0.0, min(1.0, t));
    let (qx, qy, qz) = (ax + dx * t - s.x, ay + dy * t - s.y, az + dz * t - sz);
    qx * qx + qy * qy + qz * qz
}

fn extra_dist2(s: &Sh, e: &Ex) -> f32 {
    if s.box_.is_some() {
        return seg_box(s, e.x, e.y0, 0.0, e.x, e.y1, 0.0);
    }
    let sz = s.z.unwrap_or(0.0);
    if let Some(g) = &s.seg {
        return seg_seg_dist2(s.x, s.y, sz, g[0], g[1], g[2], e.x, e.y0, 0.0, e.x, e.y1, 0.0);
    }
    seg_dist2(s.x, s.y, e.x, e.y0, e.x, e.y1) + sz * sz
}

#[allow(clippy::too_many_arguments)]
fn seg_seg_dist2(
    ax: f32,
    ay: f32,
    az: f32,
    bx: f32,
    by: f32,
    bz: f32,
    cx: f32,
    cy: f32,
    cz: f32,
    dx: f32,
    dy: f32,
    dz: f32,
) -> f32 {
    let (ux, uy, uz, vx, vy, vz) = (bx - ax, by - ay, bz - az, dx - cx, dy - cy, dz - cz);
    let (wx, wy, wz) = (ax - cx, ay - cy, az - cz);
    let (a, b, c) = (ux * ux + uy * uy + uz * uz, ux * vx + uy * vy + uz * vz, vx * vx + vy * vy + vz * vz);
    let (d, e) = (ux * wx + uy * wy + uz * wz, vx * wx + vy * wy + vz * wz);
    let den = a * c - b * b;
    let mut s = if a > 0.0 && den > 1e-9 { max(0.0, min(1.0, (b * e - c * d) / den)) } else { 0.0 };
    let mut t = if c > 0.0 { (b * s + e) / c } else { 0.0 };
    if t < 0.0 || t > 1.0 {
        t = max(0.0, min(1.0, t));
        s = if a > 0.0 { max(0.0, min(1.0, (b * t - d) / a)) } else { 0.0 };
    }
    let (qx, qy, qz) = (wx + ux * s - vx * t, wy + uy * s - vy * t, wz + uz * s - vz * t);
    qx * qx + qy * qy + qz * qz
}

/// shapeOverCapsule: a hit shape against an upright capsule on the play plane.
pub fn shape_over_capsule(s: &Sphere, e: &Extra) -> bool {
    let (s, e) = (Sh::of(s), Ex::of(e));
    let rr = s.r + e.r;
    extra_dist2(&s, &e) <= rr * rr
}

#[allow(clippy::too_many_arguments)]
fn seg_rect_dist2(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32, hx: f32, hy: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let (mut t0, mut t1, mut cut) = (0.0f32, 1.0f32, true);
    let mut k = 0;
    while k < 4 && cut {
        let p = match k {
            0 => -dx,
            1 => dx,
            2 => -dy,
            _ => dy,
        };
        let q = match k {
            0 => ax - (cx - hx),
            1 => cx + hx - ax,
            2 => ay - (cy - hy),
            _ => cy + hy - ay,
        };
        if p == 0.0 {
            cut = q >= 0.0;
        } else if p < 0.0 {
            if q / p > t1 {
                cut = false;
            } else if q / p > t0 {
                t0 = q / p;
            }
        } else if q / p < t0 {
            cut = false;
        } else if q / p < t1 {
            t1 = q / p;
        }
        k += 1;
    }
    if cut {
        return 0.0;
    }
    let (ex, ey) = (max((ax - cx).abs() - hx, 0.0), max((ay - cy).abs() - hy, 0.0));
    let (fx, fy) = (max((bx - cx).abs() - hx, 0.0), max((by - cy).abs() - hy, 0.0));
    let mut d = min(ex * ex + ey * ey, fx * fx + fy * fy);
    for k in 0..4 {
        let px = cx + if k & 1 != 0 { hx } else { -hx };
        let py = cy + if k & 2 != 0 { hy } else { -hy };
        d = min(d, seg_dist2(px, py, ax, ay, bx, by));
    }
    d
}

/// The victim's pose and place, as the hurt tests read them: the pose numbers (a Float32Array), the actor's place and
/// facing, the root capsule's hurtbox and the shapes switched off.
#[derive(Clone, Copy)]
pub struct Victim<'a> {
    pub vic: &'a [f32],
    pub vx: f64,
    pub vy: f64,
    pub vf: f64,
    pub extra: Option<&'a Extra>,
    pub off: Off<'a>,
}

/// boxHitsHurt: whether a box hitbox (upright in the play plane) touches any of the victim's hurt capsules.
pub fn box_hits_hurt(
    b: &Sampler,
    cx: impl Into<f64>,
    cy: impl Into<f64>,
    hx: impl Into<f64>,
    hy: impl Into<f64>,
    v: &Victim,
) -> bool {
    let (cx, cy, hx, hy) = (arg(cx), arg(cy), arg(hx), arg(hy));
    let v = Vi::of(v);
    if let Some(e) = v.extra
        && seg_rect_dist2(e.x, e.y0, e.x, e.y1, cx, cy, hx, hy) <= e.r * e.r
    {
        return true;
    }
    for &i in &b.hurt {
        if is_off(v.off, i) {
            continue;
        }
        let o = b.offsets[i];
        let (r, e) = (hurt_r(b, i, &v, o), hurt_end(b, i));
        if seg_rect_dist2(
            v.vx + v.p(o) * v.vf,
            v.vy + v.p(o + 1),
            v.vx + v.p(o + e) * v.vf,
            v.vy + v.p(o + e + 1),
            cx,
            cy,
            hx,
            hy,
        ) <= r * r
        {
            return true;
        }
    }
    false
}

/// firstHurt: the place of the first hurt capsule the sphere touches (the extra hurtbox last), -1 for none.
pub fn first_hurt(b: &Sampler, s: &Sphere, v: &Victim) -> i32 {
    let (s, v) = (Sh::of(s), Vi::of(v));
    for (k, &i) in b.hurt.iter().enumerate() {
        if is_off(v.off, i) {
            continue;
        }
        let o = b.offsets[i];
        let rr = s.r + hurt_r(b, i, &v, o);
        if hurt_dist2(&s, &v, o, hurt_end(b, i)) <= rr * rr {
            return k as i32;
        }
    }
    if let Some(e) = v.extra {
        let rr = s.r + e.r;
        if extra_dist2(&s, &e) <= rr * rr {
            return b.hurt.len() as i32;
        }
    }
    -1
}

/// hurtsTouched: every hurt capsule the sphere touches, into out; its length.
pub fn hurts_touched(b: &Sampler, s: &Sphere, v: &Victim, out: &mut Vec<u32>) -> usize {
    let (s, v) = (Sh::of(s), Vi::of(v));
    out.clear();
    for (k, &i) in b.hurt.iter().enumerate() {
        if is_off(v.off, i) {
            continue;
        }
        let o = b.offsets[i];
        let rr = s.r + hurt_r(b, i, &v, o);
        if hurt_dist2(&s, &v, o, hurt_end(b, i)) <= rr * rr {
            out.push(k as u32);
        }
    }
    if let Some(e) = v.extra {
        let rr = s.r + e.r;
        if extra_dist2(&s, &e) <= rr * rr {
            out.push(b.hurt.len() as u32);
        }
    }
    out.len()
}

/// spheresHitHurt: the first sphere (its index) that touches any of the victim's hurt capsules.
pub fn spheres_hit_hurt(b: &Sampler, spheres: &[Sphere], v: &Victim) -> Option<usize> {
    let v = Vi::of(v);
    for (n, s) in spheres.iter().enumerate() {
        let s = Sh::of(s);
        if let Some(e) = v.extra {
            let rr = s.r + e.r;
            if extra_dist2(&s, &e) <= rr * rr {
                return Some(n);
            }
        }
        for &i in &b.hurt {
            if is_off(v.off, i) {
                continue;
            }
            let o = b.offsets[i];
            let rr = s.r + hurt_r(b, i, &v, o);
            if hurt_dist2(&s, &v, o, hurt_end(b, i)) <= rr * rr {
                return Some(n);
            }
        }
    }
    None
}

// pointTouched: one hurt shape as the point "BPI Disable Normal Hitboxes" leaves of it, against a hit shape.
fn point_touched(b: &Sampler, i: usize, s: &Sh, v: &Vi) -> bool {
    let o = b.offsets[i];
    let cap = b.capsule[i];
    let e = if cap { 3 } else { 0 };
    let (cx, cy, cz) =
        ((v.p(o) + v.p(o + e)) / 2.0, (v.p(o + 1) + v.p(o + e + 1)) / 2.0, (v.p(o + 2) + v.p(o + e + 2)) / 2.0);
    let (px, py) = (v.vx + cx * v.vf, v.vy + cy);
    let rr = s.r + if cap { 1e-4 } else { 0.1 * v.p(o + 3) };
    let d2 = if s.box_.is_some() {
        seg_box(s, px, py, cz, px, py, cz)
    } else {
        match s.z {
            None => (s.x - px) * (s.x - px) + (s.y - py) * (s.y - py),
            Some(z) => (s.x - px) * (s.x - px) + (s.y - py) * (s.y - py) + (z - cz) * (z - cz),
        }
    };
    d2 <= rr * rr
}

// pointInRect: the same point against a box hitbox.
#[allow(clippy::too_many_arguments)]
fn point_in_rect(b: &Sampler, i: usize, cx: f32, cy: f32, hx: f32, hy: f32, v: &Vi) -> bool {
    let o = b.offsets[i];
    let cap = b.capsule[i];
    let e = if cap { 3 } else { 0 };
    let (px, py) = (v.vx + ((v.p(o) + v.p(o + e)) / 2.0) * v.vf, v.vy + (v.p(o + 1) + v.p(o + e + 1)) / 2.0);
    let r = if cap { 1e-4 } else { 0.1 * v.p(o + 3) };
    let (dx, dy) = (max((px - cx).abs() - hx, 0.0), max((py - cy).abs() - hy, 0.0));
    dx * dx + dy * dy <= r * r
}

/// boxHitsHurtPoints.
pub fn box_hits_hurt_points(
    b: &Sampler,
    cx: impl Into<f64>,
    cy: impl Into<f64>,
    hx: impl Into<f64>,
    hy: impl Into<f64>,
    v: &Victim,
) -> bool {
    let (cx, cy, hx, hy) = (arg(cx), arg(cy), arg(hx), arg(hy));
    let v = Vi::of(v);
    if let Some(e) = v.extra
        && seg_rect_dist2(e.x, e.y0, e.x, e.y1, cx, cy, hx, hy) <= e.r * e.r
    {
        return true;
    }
    b.hurt.iter().any(|&i| !is_off(v.off, i) && point_in_rect(b, i, cx, cy, hx, hy, &v))
}

/// hurtsTouchedPoints.
pub fn hurts_touched_points(b: &Sampler, s: &Sphere, v: &Victim, out: &mut Vec<u32>) -> usize {
    let (s, v) = (Sh::of(s), Vi::of(v));
    out.clear();
    for (k, &i) in b.hurt.iter().enumerate() {
        if is_off(v.off, i) {
            continue;
        }
        if point_touched(b, i, &s, &v) {
            out.push(k as u32);
        }
    }
    if let Some(e) = v.extra {
        let rr = s.r + e.r;
        if extra_dist2(&s, &e) <= rr * rr {
            out.push(b.hurt.len() as u32);
        }
    }
    out.len()
}

/// firstHurtPoints.
pub fn first_hurt_points(b: &Sampler, s: &Sphere, v: &Victim) -> i32 {
    let (s, v) = (Sh::of(s), Vi::of(v));
    for (k, &i) in b.hurt.iter().enumerate() {
        if is_off(v.off, i) {
            continue;
        }
        if point_touched(b, i, &s, &v) {
            return k as i32;
        }
    }
    if let Some(e) = v.extra {
        let rr = s.r + e.r;
        if extra_dist2(&s, &e) <= rr * rr {
            return b.hurt.len() as i32;
        }
    }
    -1
}

/// spheresHitHurtPoints.
pub fn spheres_hit_hurt_points(b: &Sampler, spheres: &[Sphere], v: &Victim) -> Option<usize> {
    let v = Vi::of(v);
    for (n, s) in spheres.iter().enumerate() {
        let s = Sh::of(s);
        if let Some(e) = v.extra {
            let rr = s.r + e.r;
            if extra_dist2(&s, &e) <= rr * rr {
                return Some(n);
            }
        }
        if b.hurt.iter().any(|&i| !is_off(v.off, i) && point_touched(b, i, &s, &v)) {
            return Some(n);
        }
    }
    None
}
