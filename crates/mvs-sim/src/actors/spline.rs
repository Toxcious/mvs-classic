//! USplineComponent's curve model (FSplineCurves: the Position curve and its reparam table),
//! shared by the spawned actors that keep a Spline component (Morty's C019Marker, Jake's JakeStretch). Sim space:
//! x = -UE Y, y = UE Z; the depth axis is left out. A point's arrive and leave tangents are one
//! (SetTangentAtSplinePoint sets both).
// `!(a < b)` is kept as written, true for a NaN: partial_cmp would hide that.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::Real;
use crate::State;
use crate::actors::lists::ReparamEntry;
use crate::actors::lists::SplinePt;
use crate::fighter::moves::V2;
use crate::math;
use mvs_data::value::Value;

/// USplineComponent ReparamStepsPerSegment (the Spline template does not set it).
pub const REPARAM_STEPS: i32 = 10;

/// FSplineCurves::GetSegmentLength (exe 1429bb890): its table of 5 Legendre-Gauss abscissae and weights (floats at
/// 14424b0d8, in this order).
const LG: [(Real, Real); 5] = [
    (0.0, 0.5688889),
    (-0.5384693, 0.47862867),
    (0.5384693, 0.47862867),
    (-0.90617985, 0.23692688),
    (0.90617985, 0.23692688),
];

/// FSplineCurves::UpdateSpline's reparam table (exe 1429cd400, the
/// loop at 1429cd5d0-1429cd6ad): per segment REPARAM_STEPS entries of (segment length to step / steps, 0 for step
/// 0, + the length so far; segment + step / steps), the full segment's length added after them, and a last entry
/// (full length, segment count). Writes `table` over and returns the spline's length.
pub fn build_reparam_table(pts: &[SplinePt], table: &mut Vec<ReparamEntry>) -> Real {
    table.clear();
    // `pts.length - 1`: -1 for no point (no segment, the last entry keyed -1).
    let segs = crate::real(i32::try_from(pts.len()).unwrap_or(i32::MAX)) - 1.0;
    let mut acc = 0.0;
    let mut i = 0usize;
    while crate::real(i32::try_from(i).unwrap_or(i32::MAX)) < segs {
        let fi = crate::real(i32::try_from(i).unwrap_or(i32::MAX));
        for s in 0..REPARAM_STEPS {
            let param = crate::real(s) / crate::real(REPARAM_STEPS);
            let d = if s == 0 { 0.0 } else { segment_length(pts, i, param) };
            table.push(ReparamEntry { d: d + acc, key: fi + param });
        }
        acc += segment_length(pts, i, 1.0);
        i += 1;
    }
    table.push(ReparamEntry { d: acc, key: segs });
    acc
}

/// FSplineCurves::GetSegmentLength (exe 1429bb890), Legendre-Gauss
/// quadrature of the cubic's derivative over [0, param]: c1 = ((P0 - P1) x 2 + T0 + T1) x 3,
/// c2 = (P1 - P0) x 6 - T0 x 4 - T1 x 2, the sum over the table of |((c1 x a + c2) x a + T0) x Scale3D| x weight at
/// a = param / 2 x (1 + abscissa), times param / 2. The scale is the component's (1 here).
pub fn segment_length(pts: &[SplinePt], i: usize, param: Real) -> Real {
    let a = &pts[i];
    let b = &pts[i + 1];
    let c1x = ((a.x - b.x) * 2.0 + a.tx + b.tx) * 3.0;
    let c1y = ((a.y - b.y) * 2.0 + a.ty + b.ty) * 3.0;
    let c2x = (b.x - a.x) * 6.0 - a.tx * 4.0 - b.tx * 2.0;
    let c2y = (b.y - a.y) * 6.0 - a.ty * 4.0 - b.ty * 2.0;
    let half = param * 0.5;
    let mut len = 0.0;
    for (ab, w) in LG {
        let al = half * (1.0 + ab);
        len += math::ue::size2d([(c1x * al + c2x) * al + a.tx, (c1y * al + c2y) * al + a.ty]) * w;
    }
    len * half
}

/// GetLocationAtDistanceAlongSpline: the reparam table (linear
/// between its entries, clamped at the ends) gives the input key, then the cubic Hermite segment
/// (FMath::CubicInterp).
pub fn location_at_distance(pts: &[SplinePt], tb: &[ReparamEntry], d: Real) -> V2 {
    location_at_key(pts, distance_to_key(tb, d))
}

/// The reparam table's Eval (exe 140945850): linear between its entries, clamped at the ends; an empty table (no
/// UpdateSpline yet) gives the curve's default 0.
fn distance_to_key(tb: &[ReparamEntry], d: Real) -> Real {
    let (Some(first), Some(last)) = (tb.first(), tb.last()) else { return 0.0 };
    let mut key = first.key;
    if d >= last.d {
        key = last.key;
    } else if d >= first.d {
        let mut i = 0;
        while i + 1 < tb.len() && tb[i + 1].d <= d {
            i += 1;
        }
        let diff = tb[i + 1].d - tb[i].d;
        key = if diff > 0.0 { tb[i].key + (tb[i + 1].key - tb[i].key) * ((d - tb[i].d) / diff) } else { tb[i].key };
    }
    key
}

/// The Position curve's Eval at an input key (exe 140e69c70; the keys are the point indices): clamped to the first
/// and the last point, else the cubic Hermite segment (FMath::CubicInterp).
fn location_at_key(pts: &[SplinePt], key: Real) -> V2 {
    let n = pts.len();
    if n == 0 {
        return V2::default();
    }
    if key < 0.0 {
        return V2 { x: pts[0].x, y: pts[0].y };
    }
    let fl = key.floor();
    let last = crate::real(i32::try_from(n - 1).unwrap_or(i32::MAX));
    // `Math.min(Math.floor(key), n - 1)`. A NaN key has no point to read; here it gives
    // the last point.
    if !(fl < last) {
        return V2 { x: pts[n - 1].x, y: pts[n - 1].y };
    }
    // Exact: 0 <= fl < n - 1.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "an integer in 0..n - 1")]
    let i = fl as usize;
    hermite(&pts[i], &pts[i + 1], key - fl)
}

/// FMath::CubicInterp on one segment.
fn hermite(a: &SplinePt, b: &SplinePt, t: Real) -> V2 {
    let t2 = t * t;
    let t3 = t2 * t;
    let h0 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h1 = t3 - 2.0 * t2 + t;
    let h2 = t3 - t2;
    let h3 = -2.0 * t3 + 3.0 * t2;
    V2 { x: h0 * a.x + h1 * a.tx + h2 * b.tx + h3 * b.x, y: h0 * a.y + h1 * a.ty + h2 * b.ty + h3 * b.y }
}

// ---- the component's queries (JakeStretch, Jake's side special)

/// ESplineCoordinateSpace.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SplineSpace {
    Local,
    World,
}

/// `SplineCurve`: a spawned actor's Spline component: the points in the component's own space (x its local X, y its
/// local Z), the reparam table UpdateSpline last built and the component's world transform, which the owner keeps
/// up to date when it moves: world x = ox + sx x local X (sx = +1 / -1: the actor's yaw of -90 / 90, so local X is
/// the facing), world y = oy + local Z. Scale 1 (Scale3D and the depth axis are left out). The keys are the point
/// indices (a template's points are keyed 0, 1, ...; AddSplinePoint keys the new point last + 1).
#[derive(State, Debug, Default)]
pub struct SplineCurve {
    pub pts: Vec<SplinePt>,
    pub table: Vec<ReparamEntry>,
    pub ox: Real,
    pub oy: Real,
    pub sx: Real,
}

/// A Spline component from its template (SplineCurves.Position.Points
/// of the class's Spline_GEN_VARIABLE: OutVal, LeaveTangent, InterpMode) at a world transform, with its reparam
/// table built (UpdateSpline at registration).
// Unverified: that registration runs UpdateSpline (so a CurveAuto template point's tangents are recomputed from
// the points; JakeStretch's two template points carry exactly those, Next - This) is the stock 4.26 reading.
// Deviation: simplified interp modes. A point without InterpMode is taken as CurveAuto (the data leaves a default
// out); CurveAutoClamped is taken as CurveAuto (no clamp of the tangent); Linear and Constant points keep their
// stored tangents and are evaluated as cubic Hermite (no straight or step segment). Every JakeStretch template
// point is CurveAuto. The component's Scale3D (1 assumed) and the depth axis are left out.
pub fn new_spline_curve(def: Option<&[Value]>, ox: Real, oy: Real, sx: Real) -> SplineCurve {
    let num = |q: &Value, k: &str, c: &str| {
        q.as_obj().and_then(|o| o.get(k)).and_then(|v| v.as_obj()).and_then(|o| o.get(c)).and_then(|v| v.as_f64())
    };
    let pts = def
        .unwrap_or(&[])
        .iter()
        .map(|q| {
            let mode = q.as_obj().and_then(|o| o.get("InterpMode")).and_then(|v| v.as_str());
            let auto = matches!(mode, None | Some("CIM_CurveAuto") | Some("CIM_CurveAutoClamped"));
            SplinePt {
                x: crate::math::narrow(num(q, "OutVal", "X").unwrap_or(0.0)),
                y: crate::math::narrow(num(q, "OutVal", "Z").unwrap_or(0.0)),
                tx: crate::math::narrow(num(q, "LeaveTangent", "X").unwrap_or(0.0)),
                ty: crate::math::narrow(num(q, "LeaveTangent", "Z").unwrap_or(0.0)),
                auto: auto.then_some(true),
            }
        })
        .collect();
    let mut s = SplineCurve { pts, table: Vec::new(), ox, oy, sx };
    update_spline_curve(&mut s);
    s
}

/// USplineComponent::UpdateSpline -> FSplineCurves::UpdateSpline:
/// the Position curve's AutoSetTangents (Tension 0, bStationaryEndpoints false) on its CurveAuto points, then the
/// reparam table (exe 1429cd400).
// Unverified: AutoSetTangents is the stock 4.26 rule (FSplineCurves::UpdateSpline's body is not read): an inner
// point's tangent is (Next - Prev) / (NextKey - PrevKey), an end point's the chord to its one neighbour (Prev or
// Next = itself); JakeStretch's template points (exactly Next - This at both ends) agree with it.
pub fn update_spline_curve(s: &mut SplineCurve) {
    let n = s.pts.len();
    for i in 0..n {
        if s.pts[i].auto != Some(true) {
            continue;
        }
        let ia = if i == 0 { 0 } else { i - 1 };
        let ib = if i == n - 1 { n - 1 } else { i + 1 };
        // KINDA_SMALL_NUMBER; the index gap is 0, 1 or 2.
        let dt = math::max(1e-4, crate::real(u8::try_from(ib - ia).unwrap_or(u8::MAX)));
        let tx = (s.pts[ib].x - s.pts[ia].x) / dt;
        let ty = (s.pts[ib].y - s.pts[ia].y) / dt;
        s.pts[i].tx = tx;
        s.pts[i].ty = ty;
    }
    build_reparam_table(&s.pts, &mut s.table);
}

/// GetSplineLength, the reparam table's last distance (0 before any
/// UpdateSpline). A point moved or added without bUpdateSpline is not in it until the next update.
pub fn spline_length(s: &SplineCurve) -> Real {
    s.table.last().map_or(0.0, |e| e.d)
}

/// GetNumberOfSplinePoints.
pub fn spline_point_count(s: &SplineCurve) -> usize {
    s.pts.len()
}

fn to_space(s: &SplineCurve, mut out: V2, space: SplineSpace) -> V2 {
    if space == SplineSpace::World {
        out.x = s.ox + s.sx * out.x;
        out.y += s.oy;
    }
    out
}

/// `getLocationAtDistanceAlongSpline`: (exe 143030d20 -> 1429b8fd0).
pub fn get_location_at_distance_along_spline(s: &SplineCurve, d: Real, space: SplineSpace) -> V2 {
    to_space(s, location_at_key(&s.pts, distance_to_key(&s.table, d)), space)
}

/// The index clamped to 0..Num - 1 (`Math.min(Math.max(i, 0), n - 1)`).
fn clamp_index(i: i32, n: usize) -> usize {
    usize::try_from(i).unwrap_or(0).min(n - 1)
}

/// `getLocationAtSplinePoint`: (exe 143030f00 -> 1429b91d0), the index clamped to
/// 0..Num - 1.
pub fn get_location_at_spline_point(s: &SplineCurve, i: i32, space: SplineSpace) -> V2 {
    let n = s.pts.len();
    if n == 0 {
        return to_space(s, V2::default(), space);
    }
    let q = &s.pts[clamp_index(i, n)];
    to_space(s, V2 { x: q.x, y: q.y }, space)
}

/// `getTangentAtSplinePoint`: (exe 143030990 -> 1429b8890), the point's LeaveTangent
/// (one tangent per point here), the index clamped as GetLocationAtSplinePoint's; World space turned by the
/// component (x by sx), not moved.
pub fn get_tangent_at_spline_point(s: &SplineCurve, i: i32, space: SplineSpace) -> V2 {
    let n = s.pts.len();
    if n == 0 {
        return V2::default();
    }
    let q = &s.pts[clamp_index(i, n)];
    V2 { x: if space == SplineSpace::World { s.sx * q.tx } else { q.tx }, y: q.ty }
}

/// `setLocationAtSplinePoint`: (exe 143033a50 -> 1429c6b90), an index out of range does
/// nothing; the location only (tangents left, the auto ones recomputed by the update); with bUpdateSpline
/// UpdateSpline inside the call.
pub fn set_location_at_spline_point(s: &mut SplineCurve, i: i32, x: Real, y: Real, space: SplineSpace, update: bool) {
    let Some(i) = usize::try_from(i).ok().filter(|&i| i < s.pts.len()) else { return };
    let (ox, oy, sx) = (s.ox, s.oy, s.sx);
    let q = &mut s.pts[i];
    q.x = if space == SplineSpace::World { (x - ox) * sx } else { x };
    q.y = if space == SplineSpace::World { y - oy } else { y };
    if update {
        update_spline_curve(s);
    }
}

/// `setTangentAtSplinePoint`: (exe 143034350 -> 1429c9570), an index out of range does
/// nothing; else both tangents, InterpMode CurveUser, and with bUpdateSpline UpdateSpline.
pub fn set_tangent_at_spline_point(s: &mut SplineCurve, i: i32, tx: Real, ty: Real, space: SplineSpace, update: bool) {
    let Some(i) = usize::try_from(i).ok().filter(|&i| i < s.pts.len()) else { return };
    let sx = s.sx;
    let q = &mut s.pts[i];
    q.tx = if space == SplineSpace::World { tx * sx } else { tx };
    q.ty = ty;
    if q.auto == Some(true) {
        q.auto = Some(false);
    }
    if update {
        update_spline_curve(s);
    }
}

/// `addSplinePoint`: (exe 14302f3b0 -> 1429a9150), a CurveAuto point keyed last + 1 (0
/// on an empty spline; the keys stay the indices) with zero tangents at the end (its Rotation / Scale points
/// identity, left out here); with bUpdateSpline UpdateSpline inside the call.
pub fn add_spline_point(s: &mut SplineCurve, x: Real, y: Real, space: SplineSpace, update: bool) {
    let w = space == SplineSpace::World;
    s.pts.push(SplinePt {
        x: if w { (x - s.ox) * s.sx } else { x },
        y: if w { y - s.oy } else { y },
        tx: 0.0,
        ty: 0.0,
        auto: Some(true),
    });
    if update {
        update_spline_curve(s);
    }
}

/// `findLocationClosestToWorldLocation`: (exe 14302f900 -> 1429b3230)
/// FindInputKeyClosestToWorldLocation (exe 14302f860 -> 1429b30a0) on the location in the component's space, then
/// Position.Eval at that key, through the component transform for World, as is for Local. Per segment (140e83130)
/// three starts 0 / 0.5 / 1, each three Newton steps along the segment's derivative, a step clamped to 0.75 x the
/// last one's size (1 at first) and the parameter to [0, 1]; the segment's best start, and the strictly nearest
/// segment (the first of equal ones). One point or none: key 0.
// Unverified: the tie order between a segment's three starts (the first of equal ones) is the stock 4.26 reading.
pub fn find_location_closest_to_world_location(s: &SplineCurve, wx: Real, wy: Real, space: SplineSpace) -> V2 {
    to_space(s, location_at_key(&s.pts, find_input_key_closest(&s.pts, (wx - s.ox) * s.sx, wy - s.oy)), space)
}

const STARTS: [Real; 3] = [0.0, 0.5, 1.0];
const STEP_SCALE: Real = 0.75;

/// FMath::Clamp: X < Min ? Min : X < Max ? X : Max.
fn fclamp(x: Real, lo: Real, hi: Real) -> Real {
    if x < lo {
        lo
    } else if x < hi {
        x
    } else {
        hi
    }
}

fn find_input_key_closest(p: &[SplinePt], x: Real, y: Real) -> Real {
    let n = p.len();
    if n < 2 {
        return 0.0;
    }
    let (mut best, mut bd) = nearest_on_segment(p, 0, x, y);
    for i in 1..n - 1 {
        let (k, d) = nearest_on_segment(p, i, x, y);
        if d < bd {
            bd = d;
            best = k;
        }
    }
    best
}

/// A segment's nearest key and its squared distance.
fn nearest_on_segment(p: &[SplinePt], i: usize, x: Real, y: Real) -> (Real, Real) {
    let a = &p[i];
    let b = &p[i + 1];
    let mut bt = 0.0;
    let mut bd = Real::INFINITY;
    for t0 in STARTS {
        let mut t = t0;
        let mut last = 1.0;
        for _ in 0..3 {
            let t2 = t * t;
            // FMath::CubicInterpDerivative
            let dx = (6.0 * a.x + 3.0 * a.tx + 3.0 * b.tx - 6.0 * b.x) * t2
                + (-6.0 * a.x - 4.0 * a.tx - 2.0 * b.tx + 6.0 * b.x) * t
                + a.tx;
            let dy = (6.0 * a.y + 3.0 * a.ty + 3.0 * b.ty - 6.0 * b.y) * t2
                + (-6.0 * a.y - 4.0 * a.ty - 2.0 * b.ty + 6.0 * b.y) * t
                + a.ty;
            let q = hermite(a, b, t);
            // no guard on a zero tangent: 0 / 0 is NaN, which FMath::Clamp takes to the top bound
            let mv =
                fclamp(((x - q.x) * dx + (y - q.y) * dy) / (dx * dx + dy * dy), -last * STEP_SCALE, last * STEP_SCALE);
            t = fclamp(t + mv, 0.0, 1.0);
            last = mv.abs();
        }
        let q = hermite(a, b, t);
        let d = (q.x - x) * (q.x - x) + (q.y - y) * (q.y - y);
        if d < bd {
            bd = d;
            bt = t;
        }
    }
    (crate::real(i32::try_from(i).unwrap_or(i32::MAX)) + bt, bd)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: Real, y: Real, tx: Real, ty: Real) -> SplinePt {
        SplinePt { x, y, tx, ty, auto: None }
    }

    #[test]
    fn straight_segment_length_and_midpoint() {
        let pts = [pt(0.0, 0.0, 100.0, 0.0), pt(100.0, 0.0, 100.0, 0.0)];
        let mut tb = Vec::new();
        let len = build_reparam_table(&pts, &mut tb);
        assert_eq!(tb.len(), 11);
        assert_eq!(len, 100.0);
        let m = location_at_distance(&pts, &tb, 50.0);
        assert!(m.x == 50.0 && m.y == 0.0);
        let s = SplineCurve { pts: pts.to_vec(), table: tb, ox: 10.0, oy: 5.0, sx: -1.0 };
        let c = find_location_closest_to_world_location(&s, -40.0, 30.0, SplineSpace::Local);
        assert_eq!(c.x, 50.0);
    }

    #[test]
    fn empty_spline() {
        let mut tb = Vec::new();
        assert_eq!(build_reparam_table(&[], &mut tb), 0.0);
        assert_eq!(tb.len(), 1);
        assert_eq!(tb[0].key, -1.0);
    }
}
