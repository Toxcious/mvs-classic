//! The edge polygons' queries of Comp_EdgeTrace: Find Lowest Leftmost Point,
//! Sort Points By Polar Angle, Line Intersection, Get Closest Point on Polygon and Get Next Point, with the vector and
//! rotator math they use. The polygons come in as what the sim's edge set gives for them at the call (`EdgeSnap`): the
//! rider, its latent actions and the edge set's upkeep stay in the sim. Blueprint vector math: float32 through the
//! build's routines (GetSafeNormal, VSize, Distance2D, DegAcos, RotateAngleAxis, FQuat), f64 at the interface.

use crate::rotator::{Quat, narrow};
use mvs_math::f32::{crt, estimate, fmath, vector};

pub type Vec3 = [f64; 3];

type V = [f32; 3];

/// One live element as the query reads it: its id, its tag (interned), whether it has left the generator (gone),
/// whether its crossings follow its place now (moves), its place now (flat), its data place (flat0) and its Local
/// Points.
#[derive(Clone, Debug, Default)]
pub struct EdgePoly {
    pub id: i32,
    pub tag: u32,
    pub gone: bool,
    pub moves: bool,
    pub flat: Vec3,
    pub flat0: Vec3,
    pub lp: Vec<Vec3>,
}

/// The level's edge set at the call: None where the level has no generator; else its elements in order.
pub type EdgeSnap = Option<Vec<EdgePoly>>;

fn n3(a: &Vec3) -> V {
    a.map(narrow)
}
fn w3(a: V) -> Vec3 {
    a.map(f64::from)
}

fn addf(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn subf(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dotf(a: V, b: V) -> f32 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
// FVector::Equals
fn eqf(a: V, b: V, tol: f32) -> bool {
    (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol && (a[2] - b[2]).abs() <= tol
}

pub fn add(a: &Vec3, b: &Vec3) -> Vec3 {
    w3(addf(n3(a), n3(b)))
}
pub fn sub(a: &Vec3, b: &Vec3) -> Vec3 {
    w3(subf(n3(a), n3(b)))
}
pub fn dot(a: &Vec3, b: &Vec3) -> f64 {
    f64::from(dotf(n3(a), n3(b)))
}
/// VSize.
pub fn size(a: &Vec3) -> f64 {
    f64::from(vector::size(n3(a)))
}
/// Vector_Distance.
pub fn dist(a: &Vec3, b: &Vec3) -> f64 {
    f64::from(vector::dist(n3(b), n3(a)))
}
/// FClamp: one of its arguments, exact in either width.
pub fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}
fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}
/// FVector::Equals.
pub fn eq(a: &Vec3, b: &Vec3, tol: f64) -> bool {
    eqf(n3(a), n3(b), narrow(tol))
}
/// GetSafeNormal(Tolerance), the Normal node.
pub fn normal(v: &Vec3, tol: f64) -> Vec3 {
    w3(vector::safe_normal(n3(v), narrow(tol)))
}

/// Find Lowest Leftmost Point (lowLeft).
pub fn low_left(pc: &[Vec3]) -> Vec3 {
    let mut low = n3(&pc[0]);
    for p in pc {
        let p = n3(p);
        if p[2] < low[2] || ((p[2] - low[2]).abs() <= 1e-6 && p[1] > low[1]) {
            low = p;
        }
    }
    w3(low)
}

/// Sort Points By Polar Angle (polarSort).
pub fn polar_sort(pc_in: &[Vec3], anchor: &Vec3) -> Vec<Vec3> {
    let mut pc: Vec<V> = pc_in.iter().map(n3).collect();
    let anchor = n3(anchor);
    let f = pc
        .iter()
        .position(|p| p[0] == anchor[0] && p[1] == anchor[1] && p[2] == anchor[2])
        .expect("polarSort: the anchor is one of the points");
    pc.swap(f, 0);
    let (down, ax) = ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0]);
    let mut to_remove: Vec<V> = Vec::new();
    let n = pc.len();
    for i in 1..n {
        let mut j = 1;
        while j + i < n {
            let a1 = signed_angle_f(subf(pc[j], anchor), down, ax);
            let a2 = signed_angle_f(subf(pc[j + 1], anchor), down, ax);
            if (a1 - a2).abs() <= 0.001 {
                let near =
                    if vector::dist(pc[j], anchor) > vector::dist(pc[j + 1], anchor) { pc[j + 1] } else { pc[j] };
                if !to_remove.iter().any(|p| eqf(*p, near, 0.0)) {
                    to_remove.push(near);
                }
                pc.swap(j, j + 1);
            } else if a1 > a2 {
                pc.swap(j, j + 1);
            }
            j += 1;
        }
    }
    pc.into_iter().filter(|p| !to_remove.iter().any(|r| eqf(*r, *p, 0.0))).map(w3).collect()
}

fn turn90(v: V) -> V {
    [v[0], -v[2], v[1]]
}
fn line_plane(start: V, end: V, origin: V, n: V) -> Option<V> {
    let dir = subf(end, start);
    let dn = dotf(dir, n);
    if dn == 0.0 {
        return None;
    }
    let t = dotf(subf(origin, start), n) / dn;
    if t < 0.0 || t > 1.0 {
        return None;
    }
    Some([start[0] + dir[0] * t, start[1] + dir[1] * t, start[2] + dir[2] * t])
}

/// Line Intersection (lineIntersection).
pub fn line_intersection(a1: &Vec3, a2: &Vec3, b1: &Vec3, b2: &Vec3) -> Option<Vec3> {
    line_intersection_f(n3(a1), n3(a2), n3(b1), n3(b2)).map(w3)
}
fn line_intersection_f(a1: V, a2: V, b1: V, b2: V) -> Option<V> {
    let n1 = turn90(subf(a1, a2));
    let n2 = turn90(subf(b1, b2));
    line_plane(b1, b2, a1, n1)?;
    line_plane(a1, a2, b1, n2)
}

/// Get Closest Point on Polygon's outputs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Closest {
    pub poly: i32,
    pub near: Vec3,
    pub last: i32,
    pub next: i32,
}

/// Get Closest Point on Polygon (closestPoint).
pub fn closest_point(snap: &EdgeSnap, rel: &Vec3, tag: u32, exclude: i32) -> Closest {
    let mut res = Closest { poly: -1, near: [0.0; 3], last: 0, next: 0 };
    let Some(all) = snap else {
        return res;
    };
    let rel = n3(rel);
    let mut shortest = -1.0f32;
    for p in all {
        if p.id == exclude || p.tag != tag || p.gone {
            continue;
        }
        let loc = n3(&p.flat);
        let n = p.lp.len();
        for i in 0..n {
            let (a, b) = (addf(loc, n3(&p.lp[i])), addf(loc, n3(&p.lp[(i + 1) % n])));
            let d = subf(b, a);
            let dir = vector::safe_normal(d, 1e-4);
            let k = clampf(dotf(dir, subf(rel, a)), 0.0, vector::size(d));
            let pt = [a[0] + dir[0] * k, a[1] + dir[1] * k, a[2] + dir[2] * k];
            let dd = vector::dist2d([pt[1], pt[2]], [rel[1], rel[2]]);
            if dd < shortest || shortest == -1.0 {
                shortest = dd;
                res = Closest { poly: p.id, near: w3(pt), last: i as i32, next: ((i + 1) % n) as i32 };
            }
        }
    }
    res
}

/// Get Next Point's outputs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Next {
    pub dest: Vec3,
    pub poly: i32,
    pub index: i32,
    pub hit: bool,
}

// Rec: one of T's Intersecting Edges.
struct Rec {
    a: V,
    b: V,
    ip: V,
    poly: i32,
    last: i32,
    next: i32,
}

// crossingsOf: T's Intersecting Edges as the registrations leave them.
fn crossings_of(all: &[EdgePoly], t: i32) -> Vec<Rec> {
    let mut out = Vec::new();
    let Some(tp) = all.iter().find(|p| p.id == t) else {
        return out;
    };
    if tp.gone {
        return out;
    }
    for e in all {
        if e.id == t || e.tag != tp.tag || e.gone {
            continue;
        }
        let now = tp.moves || e.moves;
        let ft = n3(if now { &tp.flat } else { &tp.flat0 });
        let fe = n3(if now { &e.flat } else { &e.flat0 });
        let (l, lf, r, rf, late_is_t) = if tp.id > e.id { (tp, ft, e, fe, true) } else { (e, fe, tp, ft, false) };
        let (nl, nr) = (l.lp.len(), r.lp.len());
        for i in 0..nl {
            for j in 0..nr {
                let (li, li1) = (n3(&l.lp[i]), n3(&l.lp[(i + 1) % nl]));
                let (rj, rj1) = (n3(&r.lp[j]), n3(&r.lp[(j + 1) % nr]));
                let pt = line_intersection_f(addf(lf, li), addf(lf, li1), addf(rj, rf), addf(rj1, rf));
                let Some(pt) = pt else {
                    continue;
                };
                if late_is_t {
                    out.push(Rec {
                        a: li,
                        b: li1,
                        ip: subf(pt, lf),
                        poly: e.id,
                        last: j as i32,
                        next: ((j + 1) % nr) as i32,
                    });
                } else {
                    out.push(Rec {
                        a: rj,
                        b: rj1,
                        ip: subf(pt, rf),
                        poly: e.id,
                        last: i as i32,
                        next: ((i + 1) % nl) as i32,
                    });
                }
            }
        }
    }
    out
}

// Point Lies on Line (pointOnLine).
fn point_on_line(l1: V, l2: V, p: V) -> bool {
    ((p[1] - l1[1]) * (l2[2] - l1[2]) - (p[2] - l1[2]) * (l2[1] - l1[1])).abs() <= 1e-6
}
// Inverse Lerp (Vector) (invLerp).
fn inv_lerp(a: V, b: V, p: V) -> f32 {
    let d = subf(b, a);
    let den = dotf(d, d);
    if den == 0.0 { 0.0 } else { dotf(subf(p, a), d) / den }
}
// Percent_IntInt (pct): the remainder of integers truncates toward zero, as i32's % does.
fn pct(a: i32, b: i32) -> i32 {
    if b == 0 { 0 } else { a % b }
}
fn wrap(v: i32, lo: i32, hi: i32) -> i32 {
    let size = hi - lo;
    if size <= 0 {
        return lo;
    }
    let mut r = v;
    while r < lo {
        r += size;
    }
    while r > hi {
        r -= size;
    }
    r
}
fn at(lp: &[Vec3], i: i32) -> Option<V> {
    if i < 0 { None } else { lp.get(i as usize).map(n3) }
}

/// Get Next Point (getNextPoint) of polygon `self_`.
pub fn get_next_point(snap: &EdgeSnap, self_: i32, index_in: i32, initial: &Vec3, cw: bool, ignore: i32) -> Next {
    let none = Next { dest: [0.0; 3], poly: -1, index: 0, hit: false };
    let Some(all) = snap else {
        return none;
    };
    let Some(p) = all.iter().find(|p| p.id == self_) else {
        return none;
    };
    let n = p.lp.len() as i32;
    let is = n3(initial);
    let mut cur = at(&p.lp, index_in).unwrap_or([0.0; 3]);
    let ie = cur;
    let mut cur_poly = -1;
    let mut cur_index = if cw { pct(index_in + 1, n) } else { wrap(index_in - 1, 0, n) };
    let mut hit = false;
    let recs = crossings_of(all, self_);
    let f = n3(&p.flat);
    let (s, e) = (addf(f, is), addf(f, ie));
    for r in &recs {
        let q = all.iter().find(|q| q.id == r.poly).expect("a crossing's polygon");
        let m = q.lp.len() as i32;
        let fq = n3(&q.flat);
        let k = if cw { pct(r.next + 1, m) } else { wrap(r.last - 1, 0, m) };
        let test = addf(at(&q.lp, k).expect("a crossing's point"), fq);
        if point_on_line(s, e, test) {
            continue;
        }
        let (t_end, t) = (inv_lerp(is, ie, cur), inv_lerp(is, ie, r.ip));
        if !((0.0..=1.0).contains(&t) && t < t_end && eqf(ie, if cw { r.b } else { r.a }, 0.001)) {
            continue;
        }
        if r.poly == ignore {
            continue;
        }
        if !eqf(is, r.ip, 0.001) {
            cur = r.ip;
            cur_poly = r.poly;
            cur_index = if cw { r.next } else { r.last };
            hit = true;
            continue;
        }
        if p.lp.iter().any(|v| eqf(n3(v), is, 0.001)) {
            return Next { dest: w3(r.ip), poly: r.poly, index: if cw { r.next } else { r.last }, hit: true };
        }
    }
    Next { dest: w3(cur), poly: cur_poly, index: cur_index, hit }
}

/// Get Signed Angle (Between) (signedAngle): DegAcos(Na . Nb) x SignOfFloat((Nb x Na) . Axis), the normals at
/// tolerance 1e-4.
pub fn signed_angle(a: &Vec3, b: &Vec3, axis: &Vec3) -> f64 {
    f64::from(signed_angle_f(n3(a), n3(b), n3(axis)))
}
fn signed_angle_f(a: V, b: V, axis: V) -> f32 {
    let (na, nb) = (vector::safe_normal(a, 1e-4), vector::safe_normal(b, 1e-4));
    // DegAcos clamps its input to [-1, 1]
    let deg = vector::deg_acos(dotf(na, nb));
    let s = dotf(cross(nb, na), axis);
    deg * if s > 0.0 {
        1.0
    } else if s < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// RotateAngleAxis (the node 142f81490: the axis safe-normalised, then FVector::RotateAngleAxis).
pub fn rotate_angle_axis(v: &Vec3, deg: f64, axis_in: &Vec3) -> Vec3 {
    let ax = vector::safe_normal(n3(axis_in), vector::SMALL_NUMBER);
    w3(fmath::rotate_angle_axis(n3(v), narrow(deg), ax))
}

// FQuat operator* (qmul).
// Unverified: the scalar product in the order the sim fixes; the build's FQuat operator* is VectorQuaternionMultiply2
// (SIMD, its operation order unread).
fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// qrot: v turned by q (FQuat::RotateVector).
pub fn qrot(q: &Quat, v: &Vec3) -> Vec3 {
    w3(vector::quat_rotate_vector(q.map(narrow), n3(v)))
}

/// RotatorFromAxisAndAngle (axisAngle).
pub fn axis_angle(axis_in: &Vec3, deg: f64) -> Vec3 {
    w3(vector::rotator_from_axis_angle(n3(axis_in), narrow(deg)))
}

/// ComposeRotators(A, B) (compose): (Quat(B) * Quat(A)).Rotator(), A applied first.
pub fn compose(a: &Vec3, b: &Vec3) -> Vec3 {
    w3(fmath::quat_rotator(qmul(fmath::rotator_quaternion(n3(b)), fmath::rotator_quaternion(n3(a)))))
}

/// RLerp(A, B, Alpha, bShortestPath true) (rlerp): FQuat::Slerp (Slerp_NotNormalized, then GetNormalized).
// Unverified: Slerp_NotNormalized and GetNormalized as UE 4.26's source (FMath::Acos / Sin as the CRT acosf / sinf,
// InvSqrt for the normalisation); the native 142b2bbd0 is not read.
pub fn rlerp(a: &Vec3, b: &Vec3, t: f64) -> Vec3 {
    let t = narrow(t);
    let (qa, qb) = (fmath::rotator_quaternion(n3(a)), fmath::rotator_quaternion(n3(b)));
    let raw = qa[0] * qb[0] + qa[1] * qb[1] + qa[2] * qb[2] + qa[3] * qb[3];
    let cosom = raw.abs();
    let (s0, mut s1);
    if cosom < 0.9999 {
        let omega = crt::acosf(cosom);
        let inv = 1.0 / crt::sinf(omega);
        s0 = crt::sinf((1.0 - t) * omega) * inv;
        s1 = crt::sinf(t * omega) * inv;
    } else {
        s0 = 1.0 - t;
        s1 = t;
    }
    if raw < 0.0 {
        s1 = -s1;
    }
    let q = [s0 * qa[0] + s1 * qb[0], s0 * qa[1] + s1 * qb[1], s0 * qa[2] + s1 * qb[2], s0 * qa[3] + s1 * qb[3]];
    let sq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    let n = if sq >= vector::SMALL_NUMBER {
        let k = estimate::inv_sqrt(sq);
        [q[0] * k, q[1] * k, q[2] * k, q[3] * k]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
    w3(fmath::quat_rotator(n))
}
