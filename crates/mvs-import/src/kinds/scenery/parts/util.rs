//! What the scenery parts share: a placed component's properties over its class's templates, the curves of a
//! Blueprint's curve assets as the scenery entry writes them, a component's world transform up its attach chain, on
//! the object kind's JSON (references are object paths).

use std::borrow::Cow;

use indexmap::IndexMap;
use mvs_math::hypot::{hypot3, hypot4};
use mvs_math::libm::{asin, atan2, cos, sin};

use crate::json::{Json, props_json};
use crate::kind::KindError;
use crate::kinds::scenery::part::{JsonProps, PropsOf, SceneryBindCtx, SceneryPlace};
use crate::objview::{ObjView, short_name};

pub type Vec3 = [f64; 3];
pub type Quat = [f64; 4];

fn fail(msg: String) -> KindError {
    KindError::Failed(format!("scenery: {msg}"))
}

/// A JSON value read as `v as number` does arithmetic on it: a number, a non-finite float's name (the
/// object kind writes those as strings), else NaN (`undefined`).
pub fn number(v: Option<&Json>) -> f64 {
    match v {
        Some(Json::Num(n)) => *n,
        Some(Json::Str(s)) if s == "NaN" => f64::NAN,
        Some(Json::Str(s)) if s == "Infinity" => f64::INFINITY,
        Some(Json::Str(s)) if s == "-Infinity" => f64::NEG_INFINITY,
        _ => f64::NAN,
    }
}

/// Whether the value is an object (not null, not an array).
pub fn is_struct(v: Option<&Json>) -> bool {
    matches!(v, Some(Json::Obj(_)))
}

/// A struct's X Y Z, or `d`.
pub fn vec(v: Option<&Json>, d: Vec3) -> Vec3 {
    match v {
        Some(s @ Json::Obj(_)) => [number(s.get("X")), number(s.get("Y")), number(s.get("Z"))],
        _ => d,
    }
}

/// A rotator's pitch, yaw, roll (degrees), or zeros.
pub fn rot_of(v: Option<&Json>) -> Vec3 {
    match v {
        Some(s @ Json::Obj(_)) => [number(s.get("Pitch")), number(s.get("Yaw")), number(s.get("Roll"))],
        _ => [0.0; 3],
    }
}

/// An object's name (the last of its chain).
pub fn name_of<'o>(o: &'o ObjView<'_>) -> &'o str {
    short_name(o.path())
}

/// An object's class short name (`M011_Arm_C`, `StaticMeshComponent`).
pub fn class_of<'o>(o: &'o ObjView<'_>) -> &'o str {
    short_name(o.class())
}

/// The name of the object an inner object is under (a component's actor).
pub fn outer_name(o: &ObjView<'_>) -> String {
    let path = o.path();
    let chain = &path[path.find(':').map_or(0, |i| i + 1)..];
    let Some(p) = chain.rfind('.') else { return String::new() };
    let start = chain[..p].rfind('.').map_or(0, |i| i + 1);
    chain[start..p].to_owned()
}

/// An actor's components (its direct inner objects) in export order.
pub fn comps_of<'v, 'a>(objects: &'v [ObjView<'a>], actor: &ObjView<'_>) -> Vec<&'v ObjView<'a>> {
    let prefix = format!("{}.", actor.path());
    objects.iter().filter(|o| o.path().strip_prefix(&prefix).is_some_and(|rest| !rest.contains('.'))).collect()
}

/// The tags of an actor (its properties' `Tags`).
pub fn tags_of(props: &JsonProps) -> Vec<String> {
    match props.get("Tags") {
        Some(Json::Arr(a)) => a.iter().map(|t| t.as_str().map_or_else(|| json_text(t), str::to_owned)).collect(),
        _ => Vec::new(),
    }
}

// (a tag the object kind wrote as no string: the value is kept; a name always writes as a string)
fn json_text(v: &Json) -> String {
    String::from_utf8(crate::json::to_vec(v).unwrap_or_default()).unwrap_or_default()
}

/// A string reference property (an object path), or fails.
pub fn reference(v: Option<&Json>, what: &str) -> Result<String, KindError> {
    match v {
        Some(Json::Str(s)) => Ok(s.clone()),
        _ => Err(fail(format!("{what} is not a reference"))),
    }
}

/// A reference's last name (`/Game/A/B.B:Comp` -> `Comp`).
pub fn short_ref(v: Option<&Json>) -> Result<String, KindError> {
    Ok(short_name(&reference(v, "a reference")?).to_owned())
}

/// An instance's tagged properties over its archetype's: a struct holds only the members that differ, so structs
/// merge member by member at any depth; arrays and scalars replace.
pub fn over_props(a: &JsonProps, b: Option<&JsonProps>) -> JsonProps {
    let mut out = a.clone();
    for (k, v) in b.into_iter().flatten() {
        let merged = match (out.get(k), v) {
            (Some(Json::Obj(o)), Json::Obj(vo)) => Json::Obj(over_props(o, Some(vo))),
            _ => v.clone(),
        };
        out.insert(k.clone(), merged);
    }
    out
}

/// An object's own properties as the object kind's JSON holds them (`o.properties`).
pub fn props_of(o: &ObjView<'_>) -> JsonProps {
    props_json(o.cache(), o.props())
}

/// A class default object's properties over its Blueprint parents' (`o.defaults`), `None` for another object.
pub fn defaults_of(o: &ObjView<'_>) -> Result<Option<JsonProps>, KindError> {
    Ok(o.defaults()?.map(|d| props_json(o.cache(), &d)))
}

/// A placed Blueprint actor's component as the engine makes it: its templates' properties (the class chain's
/// `<name>_GEN_VARIABLE`, a construction-script node's template, or a native component's default subobject of the
/// CDO: the object's `template` chain up to a /Script/ object), base first, then the level's own.
pub fn component_props<'a>(ctx: &dyn SceneryBindCtx<'a>, c: &ObjView<'a>) -> Result<JsonProps, KindError> {
    let mut chain: Vec<ObjView<'a>> = Vec::new();
    let mut t = c.template()?;
    while let Some(tp) = t.filter(|t| !t.starts_with("/Script/")) {
        let o = ctx.load(&tp)?;
        t = o.template()?;
        chain.insert(0, o);
    }
    if chain.is_empty() {
        return Err(fail(format!("{}: no template in its class chain", c.path())));
    }
    let mut p = JsonProps::new();
    for o in &chain {
        p = over_props(&p, Some(&props_of(o)));
    }
    Ok(over_props(&p, Some(&props_of(c))))
}

/// Every component of an actor with its properties over its templates, read once (`propsOfActor`'s function):
/// what [`SceneryBindCtx::place_of`] and [`world_of`] take.
pub struct ActorProps {
    actor: String,
    map: IndexMap<String, JsonProps>,
}

impl ActorProps {
    /// The properties of component `c`, or fails for an object that is no component of the actor.
    pub fn get(&self, c: &ObjView<'_>) -> Result<&JsonProps, KindError> {
        self.map.get(c.path()).ok_or_else(|| fail(format!("{} is not a component of {}", c.path(), self.actor)))
    }
}

impl PropsOf for ActorProps {
    fn props_of<'s>(&'s self, c: &ObjView<'_>) -> Result<Cow<'s, JsonProps>, KindError> {
        self.get(c).map(Cow::Borrowed)
    }
}

/// [`ActorProps`] of `actor` (`propsOfActor`): a component with a template outside /Script/ over its templates, else
/// its own.
pub fn props_of_actor<'a>(ctx: &dyn SceneryBindCtx<'a>, actor: &ObjView<'a>) -> Result<ActorProps, KindError> {
    let mut map = IndexMap::new();
    for c in comps_of(&ctx.level().objects, actor) {
        let p = match c.template()? {
            Some(t) if !t.starts_with("/Script/") => component_props(ctx, c)?,
            _ => props_of(c),
        };
        map.insert(c.path().to_owned(), p);
    }
    Ok(ActorProps { actor: actor.path().to_owned(), map })
}

// curves

/// A curve key as the scenery entry writes one: [time, value, arrive tangent, leave tangent, mode (0 linear, 1
/// cubic, 2 constant)].
pub type CurveKey = [f64; 5];

fn mode_of(m: &str) -> Option<f64> {
    match m {
        "RCIM_Linear" => Some(0.0),
        "RCIM_Cubic" => Some(1.0),
        "RCIM_Constant" => Some(2.0),
        _ => None,
    }
}

/// FRichCurve keys; the curves here have no tangent weights and the default constant extrapolation.
pub fn curve_keys(c: Option<&Json>) -> Result<Vec<CurveKey>, KindError> {
    let Some(c @ Json::Obj(_)) = c else { return Ok(Vec::new()) };
    let constant = |k: &str| c.get(k).is_none_or(|v| matches!(v, Json::Null) || v.as_str() == Some("RCCE_Constant"));
    if !constant("PreInfinityExtrap") || !constant("PostInfinityExtrap") {
        return Err(fail("curve extrapolation".into()));
    }
    let keys = match c.get("Keys") {
        Some(Json::Arr(a)) => a.as_slice(),
        _ => &[],
    };
    keys.iter()
        .map(|key| {
            let mode = key.get("InterpMode").and_then(Json::as_str).and_then(mode_of);
            match mode {
                Some(m) if key.get("TangentWeightMode").and_then(Json::as_str) == Some("RCTWM_WeightedNone") => Ok([
                    number(key.get("Time")),
                    number(key.get("Value")),
                    number(key.get("ArriveTangent")),
                    number(key.get("LeaveTangent")),
                    m,
                ]),
                _ => Err(fail("curve key mode".into())),
            }
        })
        .collect()
}

/// A CurveVector's three curves.
pub fn vector_curve(p: &JsonProps) -> Result<Vec<Vec<CurveKey>>, KindError> {
    ["FloatCurves", "FloatCurves[1]", "FloatCurves[2]"].iter().map(|k| curve_keys(p.get(*k))).collect()
}

/// A CurveFloat's keys' times.
pub fn event_times(p: &JsonProps) -> Result<Vec<f64>, KindError> {
    Ok(curve_keys(p.get("FloatCurve"))?.iter().map(|k| k[0]).collect())
}

// FTransform (4.26: rotation quaternion, translation, Scale3D; a child's world transform is child * parent: scale
// componentwise, rotation parent.rot x child.rot, translation parent.rot(parent.scale x child.t) + parent.t, with no
// shear). Rotators as FRotator::Quaternion and FQuat::Rotator. Doubles, computed in this order.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xform {
    pub t: Vec3,
    pub q: Quat,
    pub s: Vec3,
}

const D2: f64 = std::f64::consts::PI / 360.0;
const R2D: f64 = 180.0 / std::f64::consts::PI;

/// FRotator::Quaternion of (pitch, yaw, roll) degrees.
pub fn rot_quat([p, y, r]: Vec3) -> Quat {
    let (sp, cp, sy, cy, sr, cr) = (sin(p * D2), cos(p * D2), sin(y * D2), cos(y * D2), sin(r * D2), cos(r * D2));
    [
        cr * sp * sy - sr * cp * cy,
        -cr * sp * cy - sr * cp * sy,
        cr * cp * sy - sr * sp * cy,
        cr * cp * cy + sr * sp * sy,
    ]
}

fn norm_axis(a: f64) -> f64 {
    // JS `%`: the sign of the dividend, as Rust's
    let mut a = a % 360.0;
    if a < 0.0 {
        a += 360.0;
    }
    if a > 180.0 { a - 360.0 } else { a }
}

/// FQuat::Rotator: (pitch, yaw, roll) degrees.
pub fn quat_rot([x, y, z, w]: Quat) -> Vec3 {
    let sing = z * x - w * y;
    let yaw_y = 2.0 * (w * z + x * y);
    let yaw_x = 1.0 - 2.0 * (y * y + z * z);
    const T: f64 = 0.4999995;
    let yaw = atan2(yaw_y, yaw_x) * R2D;
    if sing < -T {
        return [-90.0, yaw, norm_axis(-yaw - 2.0 * atan2(x, w) * R2D)];
    }
    if sing > T {
        return [90.0, yaw, norm_axis(yaw - 2.0 * atan2(x, w) * R2D)];
    }
    [asin(2.0 * sing) * R2D, yaw, atan2(-2.0 * (w * x + y * z), 1.0 - 2.0 * (x * x + y * y)) * R2D]
}

fn q_mul([ax, ay, az, aw]: Quat, [bx, by, bz, bw]: Quat) -> Quat {
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// `v` rotated by `q`.
pub fn q_rotate(q: Quat, v: Vec3) -> Vec3 {
    let [x, y, z, w] = q;
    let t = [2.0 * (y * v[2] - z * v[1]), 2.0 * (z * v[0] - x * v[2]), 2.0 * (x * v[1] - y * v[0])];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

/// A component's relative transform from its properties.
pub fn rel_of(p: &JsonProps) -> Xform {
    Xform {
        t: vec(p.get("RelativeLocation"), [0.0; 3]),
        q: rot_quat(rot_of(p.get("RelativeRotation"))),
        s: vec(p.get("RelativeScale3D"), [1.0; 3]),
    }
}

// A negative scale on either side takes the engine's matrix path
// (UpdateComponentToWorldWithParent 1429a28b0 -> 140c5f7c0 / 140885f60; FTransform::MultiplyUsingMatrixWithScale):
// the product of the two scaled matrices (row vectors: child then parent), each axis normalised and given the sign of
// the per-axis scale product, its rotation read off as a quaternion, the scale that product, the translation the
// matrix's origin. The Court's BP2 hoop (root scale (-1,1,1), yaw -180) is the one chain in the build's scenery with a
// negative scale below a root.
fn axes_of(x: &Xform) -> [Vec3; 3] {
    [q_rotate(x.q, [x.s[0], 0.0, 0.0]), q_rotate(x.q, [0.0, x.s[1], 0.0]), q_rotate(x.q, [0.0, 0.0, x.s[2]])]
}

// FQuat(const FMatrix&) of a rotation matrix given by its rows (the images of X, Y, Z)
fn mat_quat(m: &[Vec3; 3]) -> Quat {
    let tr = m[0][0] + m[1][1] + m[2][2];
    if tr > 0.0 {
        let s = 0.5 / (tr + 1.0).sqrt();
        return [(m[1][2] - m[2][1]) * s, (m[2][0] - m[0][2]) * s, (m[0][1] - m[1][0]) * s, 0.25 / s];
    }
    let mut i = 0;
    if m[1][1] > m[0][0] {
        i = 1;
    }
    if m[2][2] > m[i][i] {
        i = 2;
    }
    let j = (i + 1) % 3;
    let k = (j + 1) % 3;
    let s0 = (m[i][i] - m[j][j] - m[k][k] + 1.0).sqrt();
    let mut q = [0.0; 4];
    q[i] = 0.5 * s0;
    let s = 0.5 / s0;
    q[3] = (m[j][k] - m[k][j]) * s;
    q[j] = (m[i][j] + m[j][i]) * s;
    q[k] = (m[i][k] + m[k][i]) * s;
    q
}

fn xf_mul_matrix(c: &Xform, par: &Xform) -> Xform {
    let a = axes_of(c);
    let b = axes_of(par);
    let s = [c.s[0] * par.s[0], c.s[1] * par.s[1], c.s[2] * par.s[2]];
    let mut rows = [[0.0; 3]; 3];
    for (i, ai) in a.iter().enumerate() {
        let r: Vec3 = std::array::from_fn(|k| ai[0] * b[0][k] + ai[1] * b[1][k] + ai[2] * b[2][k]);
        let n = hypot3(r[0], r[1], r[2]);
        let g = (if s[i] < 0.0 { -1.0 } else { 1.0 }) / (if n * n > 1e-8 { n } else { 1.0 });
        rows[i] = r.map(|v| v * g);
    }
    let q = mat_quat(&rows);
    let n = hypot4(q[0], q[1], q[2], q[3]);
    let r = q_rotate(par.q, [par.s[0] * c.t[0], par.s[1] * c.t[1], par.s[2] * c.t[2]]);
    Xform { t: [r[0] + par.t[0], r[1] + par.t[1], r[2] + par.t[2]], q: q.map(|v| v / n), s }
}

/// Transform `c` (relative) under `par`.
pub fn xf_mul(c: &Xform, par: &Xform) -> Xform {
    if c.s.iter().any(|v| *v < 0.0) || par.s.iter().any(|v| *v < 0.0) {
        return xf_mul_matrix(c, par);
    }
    let r = q_rotate(par.q, [par.s[0] * c.t[0], par.s[1] * c.t[1], par.s[2] * c.t[2]]);
    Xform {
        t: [r[0] + par.t[0], r[1] + par.t[1], r[2] + par.t[2]],
        q: q_mul(par.q, c.q),
        s: [c.s[0] * par.s[0], c.s[1] * par.s[1], c.s[2] * par.s[2]],
    }
}

/// The transform as the scenery entry writes it (`xfOut`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XfOut {
    pub loc: Vec3,
    pub rot: Vec3,
    pub scale: Vec3,
}

pub fn xf_out(x: &Xform) -> XfOut {
    XfOut { loc: x.t, rot: quat_rot(x.q), scale: x.s }
}

fn struct3(names: [&str; 3], v: Vec3) -> Json {
    Json::object(names.into_iter().zip(v).map(|(k, n)| (k, Json::Num(n))))
}

/// The transform as a component's relative properties (a place).
pub fn as_props(x: &Xform) -> SceneryPlace {
    let o = xf_out(x);
    let mut p = SceneryPlace::new();
    p.insert("RelativeLocation".into(), struct3(["X", "Y", "Z"], o.loc));
    p.insert("RelativeRotation".into(), struct3(["Pitch", "Yaw", "Roll"], o.rot));
    p.insert("RelativeScale3D".into(), struct3(["X", "Y", "Z"], o.scale));
    p
}

/// A placed component's world transform: its relative one up through AttachParent to its actor's root, which is its
/// place in the level (the parents may be another actor's). `props` gives a component's properties. A parent through
/// a socket or bone fails: the caller attaches those itself.
pub fn world_of(objects: &[ObjView<'_>], comp: &ObjView<'_>, props: &dyn PropsOf) -> Result<Xform, KindError> {
    let mut x = rel_of(&*props.props_of(comp)?);
    let mut at = comp;
    loop {
        let p = props.props_of(at)?;
        let Some(parent_ref) = p.get("AttachParent").filter(|v| json_truthy(v)) else { return Ok(x) };
        if let Some(s) = p.get("AttachSocketName").filter(|v| json_truthy(v) && v.as_str() != Some("None")) {
            return Err(fail(format!(
                "{}: attached to socket {}",
                at.path(),
                s.as_str().map_or_else(|| json_text(s), str::to_owned)
            )));
        }
        let pp = reference(Some(parent_ref), &format!("{} AttachParent", at.path()))?;
        let parent =
            objects.iter().find(|o| o.path() == pp).ok_or_else(|| fail(format!("{}: no parent {pp}", at.path())))?;
        x = xf_mul(&x, &rel_of(&*props.props_of(parent)?));
        at = parent;
    }
}

/// A JSON value's truth as JavaScript takes it (null, false, 0, NaN and "" false).
pub fn json_truthy(v: &Json) -> bool {
    match v {
        Json::Null => false,
        Json::Bool(b) => *b,
        Json::Num(n) => *n != 0.0 && !n.is_nan(),
        Json::Str(s) => !s.is_empty(),
        Json::Arr(_) | Json::Obj(_) => true,
    }
}

/// The component of an actor by name.
pub fn comp_of<'v, 'a>(
    objects: &'v [ObjView<'a>],
    actor: &ObjView<'_>,
    name: &str,
) -> Result<&'v ObjView<'a>, KindError> {
    let path = format!("{}.{name}", actor.path());
    objects.iter().find(|o| o.path() == path).ok_or_else(|| fail(format!("{} has no component {name}", actor.path())))
}

/// The class default object of a class path (`/Game/A/B.B_C` -> `/Game/A/B.Default__B_C`).
fn cdo_path(cls: &str) -> String {
    let dot = cls.rfind('.').unwrap_or(0);
    format!("{}.Default__{}", &cls[..dot], &cls[dot + 1..])
}

/// A Blueprint class's CDO's properties over its parents' (the object kind's `defaults`), a shallow merge.
pub fn cdo_props<'a>(ctx: &dyn SceneryBindCtx<'a>, cls: &str) -> Result<JsonProps, KindError> {
    let cdo = ctx.load(&cdo_path(cls))?;
    let mut out = defaults_of(&cdo)?.unwrap_or_default();
    for (k, v) in props_of(&cdo) {
        out.insert(k, v);
    }
    Ok(out)
}

/// A placed Blueprint actor's own variables over its class chain's defaults.
pub fn vars_of<'a>(ctx: &dyn SceneryBindCtx<'a>, actor: &ObjView<'a>) -> Result<JsonProps, KindError> {
    let cdo = ctx.load(&cdo_path(actor.class()))?;
    let base = over_props(&defaults_of(&cdo)?.unwrap_or_default(), Some(&props_of(&cdo)));
    Ok(over_props(&base, Some(&props_of(actor))))
}

// Comp_Parallax_C: BeginPlay keeps the CameraCenter_C actor's location and its owner's; every tick it moves its owner
// to that start + (0, dY x ParalaxValueY, dZ x ParalaxValueZ), d the camera centre's start minus where it is now
// (ExecuteUbergraph @51..402). Written as `parallax: [Y, Z]` for the renderer's stage block (Tree Fort: the clouds,
// sky, mountains and cloud ring).
const COMP_PARALLAX: &str = "/Game/Panda_Main/Blueprints/Components/Comp_Parallax.Comp_Parallax_C";

/// A Comp_Parallax_C component's fields of its actor's instance, from its properties.
pub fn parallax_of<'a>(p: &JsonProps, ctx: &dyn SceneryBindCtx<'a>) -> Result<JsonProps, KindError> {
    let d = cdo_props(ctx, COMP_PARALLAX)?;
    // `a ?? b ?? 0`
    let set = |m: &JsonProps, k: &str| m.get(k).filter(|v| !matches!(v, Json::Null)).cloned();
    let pick = |k: &str| set(p, k).or_else(|| set(&d, k)).unwrap_or(Json::Num(0.0));
    let mut out = JsonProps::new();
    out.insert("parallax".into(), Json::Arr(vec![pick("ParalaxValueY"), pick("ParalaxValueZ")]));
    Ok(out)
}

/// An inner object of a Blueprint class (`<class path>:<name>`).
pub fn class_inner<'a>(ctx: &dyn SceneryBindCtx<'a>, cls: &str, name: &str) -> Result<ObjView<'a>, KindError> {
    ctx.load(&format!("{cls}:{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms() {
        let q = rot_quat([10.0, -30.0, 45.0]);
        let r = quat_rot(q);
        assert!((r[0] - 10.0).abs() < 1e-9 && (r[1] + 30.0).abs() < 1e-9 && (r[2] - 45.0).abs() < 1e-9);
        let c = Xform { t: [1.0, 2.0, 3.0], q: rot_quat([0.0, 90.0, 0.0]), s: [1.0; 3] };
        let p = Xform { t: [10.0, 0.0, 0.0], q: rot_quat([0.0, 90.0, 0.0]), s: [2.0; 3] };
        let w = xf_mul(&c, &p);
        assert!((w.t[0] - 6.0).abs() < 1e-9 && (w.t[1] - 2.0).abs() < 1e-9 && (w.t[2] - 6.0).abs() < 1e-9);
        assert!((quat_rot(w.q)[1] - 180.0).abs() < 1e-9);
        // the matrix path agrees with the quaternion path on a mirror of a mirror
        let m = Xform { t: [0.0; 3], q: rot_quat([0.0, -180.0, 0.0]), s: [-1.0, 1.0, 1.0] };
        let back = xf_mul(&Xform { t: [1.0, 0.0, 0.0], q: rot_quat([0.0; 3]), s: [-1.0, 1.0, 1.0] }, &m);
        assert!(back.s == [1.0; 3] && (back.t[0] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn props_merge_and_curves() {
        let j = |s: &str| match Json::parse(s.as_bytes()).unwrap() {
            Json::Obj(m) => m,
            _ => unreachable!(),
        };
        let a = j(r#"{"A":{"X":1,"Y":2},"B":[1,2],"C":1}"#);
        let b = j(r#"{"A":{"Y":5},"B":[3]}"#);
        let o = over_props(&a, Some(&b));
        assert_eq!(Json::Obj(o), Json::Obj(j(r#"{"A":{"X":1,"Y":5},"B":[3],"C":1}"#)));
        let c = j(
            r#"{"FloatCurve":{"Keys":[{"InterpMode":"RCIM_Cubic","TangentWeightMode":"RCTWM_WeightedNone","Time":0.5,"Value":1,"ArriveTangent":0,"LeaveTangent":0}]}}"#,
        );
        assert_eq!(event_times(&c).unwrap(), [0.5]);
        assert!(curve_keys(Some(&Json::Obj(j(r#"{"PostInfinityExtrap":"RCCE_Cycle"}"#)))).is_err());
        assert_eq!(vec(None, [1.0; 3]), [1.0; 3]);
        assert!(number(Some(&Json::Str("Infinity".into()))).is_infinite());
    }
}

/// Small JS-semantics helpers on [`Json`] the scenery parts share (a missing member is `undefined`).
pub mod js {
    use crate::json::Json;
    use crate::kind::KindError;
    use crate::kinds::scenery::part::JsonProps;

    use super::{CurveKey, Vec3, json_truthy, number};

    pub fn fail<T>(part: &str, msg: String) -> Result<T, KindError> {
        Err(KindError::Failed(format!("scenery {part}: {msg}")))
    }

    /// `v as number` (NaN for none).
    pub fn number_of(v: Option<&Json>) -> f64 {
        number(v)
    }

    /// A JavaScript truth value.
    pub fn truthy(v: Option<&Json>) -> bool {
        v.is_some_and(json_truthy)
    }

    /// `v === s`.
    pub fn is_str(v: Option<&Json>, s: &str) -> bool {
        v.and_then(Json::as_str) == Some(s)
    }

    /// A member of an object value.
    pub fn at<'j>(v: Option<&'j Json>, k: &str) -> Option<&'j Json> {
        v.and_then(|v| v.get(k))
    }

    /// `a ?? b`.
    pub fn first<'j>(a: Option<&'j Json>, b: Option<&'j Json>) -> Option<&'j Json> {
        a.filter(|v| !matches!(v, Json::Null)).or(b)
    }

    /// `(v as number) ?? d`.
    pub fn num_or(v: Option<&Json>, d: f64) -> f64 {
        match v {
            None | Some(Json::Null) => d,
            v => number(v),
        }
    }

    /// `(v as number) || d`.
    pub fn or_num(v: Option<&Json>, d: f64) -> f64 {
        let n = number(v);
        if n == 0.0 || n.is_nan() { d } else { n }
    }

    /// `v ?? d` as a value of an entry.
    pub fn nn(v: Option<&Json>, d: f64) -> Json {
        first(v, None).cloned().unwrap_or(Json::Num(d))
    }

    /// An array value's items (none for another value).
    pub fn arr(v: Option<&Json>) -> &[Json] {
        match v {
            Some(Json::Arr(a)) => a,
            _ => &[],
        }
    }

    /// An array value (`as T[]`), or fails.
    pub fn req_arr<'j>(part: &str, v: Option<&'j Json>, what: &str) -> Result<&'j [Json], KindError> {
        match v {
            Some(Json::Arr(a)) => Ok(a),
            _ => fail(part, format!("{what} is not an array")),
        }
    }

    /// Sets `k` when the value is not `undefined` (a JS object member of `undefined` is not written).
    pub fn put_opt(o: &mut JsonProps, k: &str, v: Option<&Json>) {
        if let Some(v) = v {
            o.insert(k.to_owned(), v.clone());
        }
    }

    pub fn put(o: &mut JsonProps, k: &str, v: impl Into<Json>) {
        o.insert(k.to_owned(), v.into());
    }

    pub fn triple(v: Vec3) -> Json {
        Json::Arr(v.into_iter().map(Json::Num).collect())
    }

    pub fn key_json(k: &CurveKey) -> Json {
        Json::Arr(k.iter().copied().map(Json::Num).collect())
    }

    /// `+(x as number)`-style index of a number value.
    pub fn index(v: Option<&Json>) -> usize {
        let n = number(v);
        if n.is_finite() && n >= 0.0 { n as usize } else { usize::MAX }
    }
}
