//! Tree Fort (Map_TreeHouse, Map_TreeHouseSmall, Map_TreeHouse_1Plat_Large): the blocks its scenery entries have of
//! its level Blueprints: `treefort` (the blackout sphere,
//! Comp_Parallax, the cloud groups, the snails) and `treefortSky` (Lady Rainicorn, the cloud ring).
//! The stage actors' block, `treefortActors`, is [`pillars`].

pub mod pillars;

use std::collections::BTreeMap;

use mvs_view::M4;
use mvs_view::math::{compose, qmul, rotator_quat};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::host::{InstanceId, SceneryHost};
use super::parallax::{ParallaxActor, ParallaxCamera};
use super::{CurveKey, rich_curve};
use crate::Result;
use crate::stage::{bad, transform};

/// The stage's block of one key of its entry.
#[derive(Debug)]
pub enum TreeFort {
    /// `treefort`: the level Blueprints' part.
    Level(Box<Level>),
    /// `treefortSky`: Lady_Rainicorn_Blueprint_C and MoutainCloudSpin_C.
    Sky(Box<Sky>),
}

impl TreeFort {
    pub fn make(key: &str, data: &Value, host: &mut SceneryHost) -> Result<TreeFort> {
        Ok(match key {
            "treefort" => TreeFort::Level(Box::new(Level::make(parse(key, data)?, host)?)),
            "treefortSky" => TreeFort::Sky(Box::new(Sky::make(parse(key, data)?, host)?)),
            _ => return Err(bad("scenery: treefort", format!("the block {key} is not Tree Fort's"))),
        })
    }

    pub fn begin(&mut self, host: &mut SceneryHost) {
        match self {
            TreeFort::Level(b) => b.begin(host),
            TreeFort::Sky(b) => b.begin(host),
        }
    }

    pub fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        match self {
            TreeFort::Level(b) => b.tick(host, dt),
            TreeFort::Sky(b) => b.tick(host, dt),
        }
    }

    pub fn frame(&mut self, host: &mut SceneryHost, _alpha: f64) {
        match self {
            TreeFort::Level(b) => b.frame(host),
            TreeFort::Sky(b) => b.frame(host),
        }
    }

    pub fn end(&mut self, _host: &mut SceneryHost) {}
}

fn parse<T: DeserializeOwned>(key: &str, data: &Value) -> Result<T> {
    serde_json::from_value(data.clone()).map_err(|e| bad(&format!("scenery: {key}"), e.to_string()))
}

// ---- the block's data

fn one() -> [f64; 3] {
    [1.0; 3]
}

#[derive(Clone, Debug, Deserialize)]
struct Place {
    loc: [f64; 3],
    rot: [f64; 3],
    #[serde(default = "one")]
    scale: [f64; 3],
}

#[derive(Debug, Deserialize)]
struct LevelBlock {
    blackout: Option<Blackout>,
    snails: Option<SnailBlock>,
    clouds: Option<CloudBlock>,
}

#[derive(Debug, Deserialize)]
struct Blackout {
    actor: String,
    at: f64,
}

#[derive(Debug, Deserialize)]
struct SnailBlock {
    actors: Vec<String>,
    mounted: Mounted,
    delay: f64,
    rate: f64,
    curve: Vec<CurveKey>,
    rail: RailBlock,
}

#[derive(Debug, Deserialize)]
struct Mounted {
    #[serde(flatten)]
    place: Place,
    snail: usize,
}

/// A spline point: input key, position, arrive and leave tangents, interp mode.
type SplinePoint = (f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, String);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RailBlock {
    position: f64,
    lock_orientation: bool,
    root: Place,
    spline: SplineBlock,
    reparam: Vec<[f64; 2]>,
}

#[derive(Debug, Deserialize)]
struct SplineBlock {
    position: Vec<SplinePoint>,
}

#[derive(Debug, Deserialize)]
struct CloudBlock {
    distance: f64,
    reset: f64,
    groups: Vec<CloudGroup>,
}

#[derive(Debug, Deserialize)]
struct CloudGroup {
    actors: Vec<String>,
    speed: f64,
}

#[derive(Debug, Deserialize)]
struct SkyBlock {
    ladies: Vec<LadyBlock>,
    spins: Vec<SpinBlock>,
}

#[derive(Debug, Deserialize)]
struct LadyBlock {
    skeletal: String,
    speed: f64,
    distance: f64,
    height: [f64; 2],
    wait: [f64; 2],
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpinBlock {
    actor: String,
    yaw_rate: BTreeMap<String, f64>,
    parallax: Option<[f64; 2]>,
    comps: BTreeMap<String, Comp>,
}

#[derive(Debug, Deserialize)]
struct Comp {
    #[serde(flatten)]
    place: Place,
    parent: Option<String>,
}

// ---- FTransform (translation, rotation, Scale3D) in UE space; child x parent: scale componentwise, rotation
// parent x child, translation parent.rot(parent.scale x child.t) + parent.t, no shear ).

#[derive(Clone, Copy, Debug, PartialEq)]
struct Xf {
    t: [f64; 3],
    /// (x, y, z, w)
    q: [f64; 4],
    s: [f64; 3],
}

impl Default for Xf {
    fn default() -> Xf {
        Xf { t: [0.0; 3], q: [0.0, 0.0, 0.0, 1.0], s: [1.0; 3] }
    }
}

impl Xf {
    fn of(p: &Place) -> Xf {
        Xf { t: p.loc, q: rotator_quat(p.rot[0], p.rot[1], p.rot[2]), s: p.scale }
    }

    /// The UE transform as a row-vector matrix (`stage::transform`'s form).
    fn m4(&self) -> M4 {
        let e = compose(self.t, self.q, self.s);
        std::array::from_fn(|i| std::array::from_fn(|j| e[i * 4 + j]))
    }
}

/// Vector3.applyQuaternion.
fn qrot(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let [qx, qy, qz, qw] = q;
    let [vx, vy, vz] = v;
    let tx = 2.0 * (qy * vz - qz * vy);
    let ty = 2.0 * (qz * vx - qx * vz);
    let tz = 2.0 * (qx * vy - qy * vx);
    [vx + qw * tx + qy * tz - qz * ty, vy + qw * ty + qz * tx - qx * tz, vz + qw * tz + qx * ty - qy * tx]
}

/// child x parent.
fn xf_mul(c: &Xf, p: &Xf) -> Xf {
    let scaled = [c.t[0] * p.s[0], c.t[1] * p.s[1], c.t[2] * p.s[2]];
    let r = qrot(p.q, scaled);
    Xf {
        t: [r[0] + p.t[0], r[1] + p.t[1], r[2] + p.t[2]],
        q: qmul(p.q, c.q),
        s: [c.s[0] * p.s[0], c.s[1] * p.s[1], c.s[2] * p.s[2]],
    }
}

/// Quaternion.setFromRotationMatrix of the basis (x, y, z) as columns.
fn quat_of_basis(x: [f64; 3], y: [f64; 3], z: [f64; 3]) -> [f64; 4] {
    let (m11, m12, m13) = (x[0], y[0], z[0]);
    let (m21, m22, m23) = (x[1], y[1], z[1]);
    let (m31, m32, m33) = (x[2], y[2], z[2]);
    let trace = m11 + m22 + m33;
    if trace > 0.0 {
        let s = 0.5 / (trace + 1.0).sqrt();
        [(m32 - m23) * s, (m13 - m31) * s, (m21 - m12) * s, 0.25 / s]
    } else if m11 > m22 && m11 > m33 {
        let s = 2.0 * (1.0 + m11 - m22 - m33).sqrt();
        [0.25 * s, (m12 + m21) / s, (m13 + m31) / s, (m32 - m23) / s]
    } else if m22 > m33 {
        let s = 2.0 * (1.0 + m22 - m11 - m33).sqrt();
        [(m12 + m21) / s, 0.25 * s, (m23 + m32) / s, (m13 - m31) / s]
    } else {
        let s = 2.0 * (1.0 + m33 - m11 - m22).sqrt();
        [(m13 + m31) / s, (m23 + m32) / s, 0.25 * s, (m21 - m12) / s]
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let l = if l == 0.0 { 1.0 } else { l };
    [v[0] / l, v[1] / l, v[2] / l]
}

/// A composed transform is the one the file placed (checked to 1e-3 x max(1, |element|) of the holder matrix in
/// metres: the translation row in UE units is 100 x that, the rotation block is not scaled).
fn same(a: &M4, b: &M4) -> bool {
    a.iter().zip(b).enumerate().all(|(r, (ra, rb))| {
        ra.iter().zip(rb).enumerate().all(|(c, (x, y))| {
            let unit = if r == 3 && c < 3 { 100.0 } else { 1.0 };
            (x - y).abs() <= 1e-3 * y.abs().max(unit)
        })
    })
}

// ---- CameraRig_Rail (native). Unverified: ACameraRig_Rail::UpdateRailComponents is not read; this takes its stock
// 4.26 form: each tick the RailCameraMount goes to the rail's spline at distance CurrentPositionOnRail x the spline's
// length (GetLocationAtDistanceAlongSpline, world), and with bLockOrientationToRail to its rotation there
// (GetQuaternionAtDistanceAlongSpline: MakeFromXZ(the position curve's derivative, the Rotation curve's up), the
// Rotation keys all the identity here, so world Z under the rig's root). Distance -> input key: the spline's cooked
// ReparamTable (linear); length: its last distance. FInterpCurveVector Eval / EvalDerivative: the segment's start
// point's mode, cubic Hermite with its LeaveTangent and the next point's ArriveTangent times the key interval; at or
// past the ends the end point's value and its own tangent (Leave at the start, Arrive at the end).

#[derive(Clone, Debug)]
struct RailPoint {
    key: f64,
    p: [f64; 3],
    arrive: [f64; 3],
    leave: [f64; 3],
    /// 0 linear, 1 cubic, 2 constant.
    mode: u8,
}

#[derive(Clone, Debug)]
struct Rail {
    pts: Vec<RailPoint>,
    reparam: Vec<[f64; 2]>,
    length: f64,
    root: Xf,
    lock: bool,
}

impl Rail {
    fn new(r: &RailBlock) -> Result<Rail> {
        let pts = r
            .spline
            .position
            .iter()
            .map(|q| {
                let mode = match q.10.as_str() {
                    "Linear" => 0,
                    "CurveAuto" | "CurveAutoClamped" | "CurveUser" | "CurveBreak" => 1,
                    "Constant" => 2,
                    m => return Err(bad("scenery: treefort rail", format!("the interp mode {m}"))),
                };
                Ok(RailPoint { key: q.0, p: [q.1, q.2, q.3], arrive: [q.4, q.5, q.6], leave: [q.7, q.8, q.9], mode })
            })
            .collect::<Result<Vec<_>>>()?;
        let length = r.reparam.last().ok_or_else(|| bad("scenery: treefort rail", "no reparam table"))?[0];
        if pts.is_empty() {
            return Err(bad("scenery: treefort rail", "no spline points"));
        }
        Ok(Rail { pts, reparam: r.reparam.clone(), length, root: Xf::of(&r.root), lock: r.lock_orientation })
    }

    /// FInterpCurveFloat (linear) Eval of the reparam table.
    fn key_at(&self, d: f64) -> f64 {
        let t = &self.reparam;
        let n = t.len();
        if d <= t[0][0] {
            return t[0][1];
        }
        if d >= t[n - 1][0] {
            return t[n - 1][1];
        }
        let mut i = 0;
        while t[i + 1][0] <= d {
            i += 1;
        }
        t[i][1] + ((t[i + 1][1] - t[i][1]) * (d - t[i][0])) / (t[i + 1][0] - t[i][0])
    }

    /// The spline's position and derivative at an input key, in the spline's space.
    fn eval_at(&self, key: f64) -> ([f64; 3], [f64; 3]) {
        let pts = &self.pts;
        let n = pts.len();
        if key <= pts[0].key {
            return (pts[0].p, pts[0].leave);
        }
        if key >= pts[n - 1].key {
            return (pts[n - 1].p, pts[n - 1].arrive);
        }
        let mut i = 0;
        while pts[i + 1].key <= key {
            i += 1;
        }
        let (p0, p1) = (&pts[i], &pts[i + 1]);
        let diff = p1.key - p0.key;
        let u = (key - p0.key) / diff;
        if p0.mode == 2 {
            return (p0.p, [0.0; 3]);
        }
        if p0.mode == 0 {
            let pos = std::array::from_fn(|c| p0.p[c] + (p1.p[c] - p0.p[c]) * u);
            let dir = std::array::from_fn(|c| (p1.p[c] - p0.p[c]) / diff);
            return (pos, dir);
        }
        // FMath::CubicInterp / CubicInterpDerivative with T0 = leave x diff, T1 = arrive x diff
        let (u2, u3) = (u * u, u * u * u);
        let (h0, h1, h2, h3) = (2.0 * u3 - 3.0 * u2 + 1.0, u3 - 2.0 * u2 + u, u3 - u2, -2.0 * u3 + 3.0 * u2);
        let a: [f64; 3] = std::array::from_fn(|c| p0.leave[c] * diff);
        let b: [f64; 3] = std::array::from_fn(|c| p1.arrive[c] * diff);
        let pos = std::array::from_fn(|c| p0.p[c] * h0 + a[c] * h1 + b[c] * h2 + p1.p[c] * h3);
        // the basis' derivatives (CubicInterpDerivative's a u^2 + b u + c), over diff
        let dir = std::array::from_fn(|c| {
            (p0.p[c] * (6.0 * u2 - 6.0 * u)
                + a[c] * (3.0 * u2 - 4.0 * u + 1.0)
                + b[c] * (3.0 * u2 - 2.0 * u)
                + p1.p[c] * (-6.0 * u2 + 6.0 * u))
                / diff
        });
        (pos, dir)
    }

    /// The mount's world transform at a rail position (scale 1).
    fn mount(&self, position: f64) -> Xf {
        let (t, x) = self.eval_at(self.key_at(position * self.length));
        let q = if self.lock {
            // FRotationMatrix::MakeFromXZ(X, Z): X normalised, Y = Z ^ X normalised, Z = X ^ Y
            let x = normalize(x);
            let mut z = [0.0, 0.0, 1.0];
            let dot = x[0] * z[0] + x[1] * z[1] + x[2] * z[2];
            if (dot.abs() - 1.0).abs() < 1e-8 {
                let flat = x[2].abs() < 1.0 - 1e-4;
                z = if flat { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
            }
            let y = normalize(cross(z, x));
            let z = cross(x, y);
            quat_of_basis(x, y, z)
        } else {
            [0.0, 0.0, 0.0, 1.0]
        };
        xf_mul(&Xf { t, q, s: [1.0; 3] }, &self.root)
    }
}

// ---- the level block (`treefort`)
// - BlackoutSphere (the instance with `destroyAt`): Map_TreeHouse_C / Map_TreeHouseSmall_C /
//   Map_TreeHouse_1Plat_Large_C's BeginPlay waits Delay(0.5) and destroys it: hidden 0.5 s after the level begins.
// - Comp_Parallax (parallax.rs) of each instance with `parallax` (its actor's, a StaticMeshActor whose mesh is its
//   root: the instance's location is the actor's); an instance `attachedTo` such an actor moves with it.
// - Map_TreeHouse_Geo_C ReceiveTick (@3835..3740, off a dedicated server): per group 1, 2, 3, per cloud,
//   Comp_Parallax.ActorStartLocation.Y += DeltaSeconds x BackCloudGroup<n>Speed, set to 9000 at CloudDistance or
//   below (`clouds`). Unverified: the level script's tick and the clouds' Comp_Parallax ticks are taken in that
//   order (the move shows in the same frame).
// - the snails (`snails`): hidden at the start. BeginPlay picks ServerSnail = RandomInteger(3) on the authority, a
//   replicated property (OnRep_ServerSnail, MarkPropertyDirtyFromRepIndex @222), so every peer shows the same snail.
//   Deviation: the build's draw is the server's RandomInteger, the port's a stream of its own on the match's seed
//   every peer has (SceneryHost::seeded). OnRep_ServerSnail shows that snail (SetActorHiddenInGame(false)); the
//   third one's case alone calls PlaySnail (@233..270), which plays Timeline_0 from the start at rate 1 / (2 x 60)
//   (in the Geo level after Delay(10), latent 3913 -> 16), its track (0 -> 1, flat cubic keys at 0 and 1) setting
//   CameraRig_Rail's CurrentPositionOnRail each update; that snail is attached to the rail's RailCameraMount
//   (`mounted`). Until the timeline starts the rail stays at its placed position (1, the spline's end: where the
//   file placed the snail), after it ends at 1. The timeline ticks from the tick after PlayFromStart, whose update
//   sets the start's value. The other two snails stay where they are.

#[derive(Debug)]
struct Snails {
    actors: Vec<String>,
    mounted: usize,
    delay: f64,
    rate: f64,
    curve: Vec<CurveKey>,
    start: f64,
    rail: Rail,
    rel: Xf,
    shown: usize,
    /// The Geo level's Delay(10) left; < 0: the timeline plays; infinite: it never starts.
    wait: f64,
    /// Timeline_0's position.
    timeline: f64,
    /// CurrentPositionOnRail.
    position: f64,
    placed_at: f64,
}

impl Snails {
    fn at(&self, position: f64) -> M4 {
        xf_mul(&self.rel, &self.rail.mount(position)).m4()
    }

    fn place(&mut self, host: &mut SceneryHost) {
        if self.position == self.placed_at {
            return;
        }
        self.placed_at = self.position;
        host.place_skeletal(&self.actors[self.mounted], Some(self.at(self.position)));
    }
}

#[derive(Debug)]
pub struct Level {
    blackout: Vec<InstanceId>,
    blackout_at: Option<f64>,
    time: f64,
    gone: bool,
    cam: ParallaxCamera,
    owners: Vec<ParallaxActor>,
    clouds: Option<CloudBlock>,
    /// The cloud groups' owners (indexes in `owners`).
    groups: Vec<Vec<usize>>,
    snails: Option<Snails>,
}

fn field_f64s(v: &Value) -> Option<Vec<f64>> {
    v.as_array()?.iter().map(Value::as_f64).collect()
}

impl Level {
    fn make(block: LevelBlock, host: &mut SceneryHost) -> Result<Level> {
        let what = "scenery: treefort";
        let mut blackout = Vec::new();
        let mut owners: Vec<ParallaxActor> = Vec::new();
        let mut groups: Vec<Vec<usize>> =
            block.clouds.as_ref().map_or(Vec::new(), |c| vec![Vec::new(); c.groups.len()]);
        let mut attached: Vec<(String, InstanceId, M4)> = Vec::new();
        for (k, inst) in host.instances().iter().enumerate() {
            let id = InstanceId(k as u32);
            let base = transform(inst.loc, inst.rot, inst.scale);
            if let Some(at) = inst.fields.get("destroyAt") {
                let b = block.blackout.as_ref();
                if b.is_none_or(|b| b.actor != inst.name || at.as_f64() != Some(b.at)) {
                    return Err(bad(what, format!("{} destroyAt", inst.name)));
                }
                blackout.push(id);
                continue;
            }
            let group = inst.fields.get("cloudGroup").and_then(Value::as_u64);
            if let Some(par) = inst.fields.get("parallax") {
                let par = field_f64s(par)
                    .filter(|p| p.len() == 2)
                    .ok_or_else(|| bad(what, format!("{} parallax", inst.name)))?;
                if owners.iter().any(|o| o.name == inst.name) {
                    return Err(bad(what, format!("parallax {} twice", inst.name)));
                }
                let mut p = ParallaxActor::new(&inst.name, inst.loc, [par[0], par[1]]);
                p.add(id, base);
                if let Some(g) = group {
                    let g = groups.get_mut(g as usize).ok_or_else(|| bad(what, format!("cloud group {g}")))?;
                    g.push(owners.len());
                }
                owners.push(p);
                continue;
            }
            if group.is_some() {
                return Err(bad(what, format!("cloud {} has no Comp_Parallax", inst.name)));
            }
            if let Some(to) = inst.fields.get("attachedTo").and_then(Value::as_str) {
                attached.push((to.to_owned(), id, base));
            }
        }
        // (an actor with no Comp_Parallax, Geo's Mountains2, keeps what is attached to it where it is)
        for (to, id, base) in attached {
            if let Some(p) = owners.iter_mut().find(|o| o.name == to) {
                p.add(id, base);
            }
        }
        if let Some(c) = &block.clouds {
            for (i, g) in groups.iter().enumerate() {
                if g.len() != c.groups[i].actors.len() {
                    return Err(bad(what, format!("cloud group {i} instances")));
                }
            }
        }
        let snails = block.snails.map(|sn| Level::snails(sn, host)).transpose()?;
        Ok(Level {
            blackout,
            blackout_at: block.blackout.map(|b| b.at),
            time: 0.0,
            gone: false,
            cam: ParallaxCamera::default(),
            owners,
            clouds: block.clouds,
            groups,
            snails,
        })
    }

    /// The snails' skeletal meshes (`snail`: their index in the block's actors), the mounted one where the rail at its
    /// placed position puts it.
    fn snails(sn: SnailBlock, host: &SceneryHost) -> Result<Snails> {
        let what = "scenery: treefort snails";
        let mut found = vec![None; sn.actors.len()];
        for k in host.skeletals() {
            let Some(i) = k.fields.get("snail").and_then(Value::as_u64) else { continue };
            let i = i as usize;
            if sn.actors.get(i) != Some(&k.name) {
                return Err(bad(what, format!("snail {}", k.name)));
            }
            found[i] = Some(transform(k.loc, k.rot, k.scale));
        }
        if found.iter().any(Option::is_none) || sn.mounted.snail >= found.len() {
            return Err(bad(what, "not in the file"));
        }
        let s = Snails {
            rail: Rail::new(&sn.rail)?,
            rel: Xf::of(&sn.mounted.place),
            mounted: sn.mounted.snail,
            actors: sn.actors,
            delay: sn.delay,
            rate: sn.rate,
            curve: sn.curve,
            start: sn.rail.position,
            shown: 0,
            wait: f64::INFINITY,
            timeline: 0.0,
            position: sn.rail.position,
            placed_at: f64::NAN,
        };
        let placed = found[s.mounted].as_ref().unwrap_or_else(|| unreachable!());
        if !same(&s.at(s.start), placed) {
            return Err(bad(what, format!("{} is not where the file placed it", s.actors[s.mounted])));
        }
        Ok(s)
    }

    fn begin(&mut self, host: &mut SceneryHost) {
        self.time = 0.0;
        self.gone = false;
        host.show(&self.blackout, true);
        self.cam.begin(host);
        for o in &mut self.owners {
            o.begin();
        }
        let Some(sn) = &mut self.snails else { return };
        sn.shown = (host.seeded([0.0, 3.0]).floor() as usize).min(2);
        for (i, a) in sn.actors.iter().enumerate() {
            host.show_skeletal(a, i == sn.shown);
        }
        sn.position = sn.start;
        sn.placed_at = f64::NAN;
        sn.timeline = 0.0;
        sn.wait = if sn.shown != sn.mounted {
            f64::INFINITY
        } else if sn.delay > 0.0 {
            sn.delay
        } else {
            -1.0
        };
        if sn.wait < 0.0 {
            sn.position = rich_curve(&sn.curve, 0.0);
        }
        sn.place(host);
    }

    fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        if let Some(c) = &self.clouds {
            for (g, group) in self.groups.iter().zip(&c.groups) {
                for &k in g {
                    let p = &mut self.owners[k];
                    p.start[1] += dt * group.speed;
                    if p.start[1] <= c.distance {
                        p.start[1] = c.reset;
                    }
                }
            }
        }
        self.tick_snails(host, dt);
        if self.gone {
            return;
        }
        let Some(at) = self.blackout_at else { return };
        self.time += dt;
        if self.time >= at {
            self.gone = true;
            host.show(&self.blackout, false);
        }
    }

    fn tick_snails(&mut self, host: &mut SceneryHost, dt: f64) {
        let Some(sn) = &mut self.snails else { return };
        if sn.wait == f64::INFINITY {
            return;
        }
        if sn.wait >= 0.0 {
            sn.wait -= dt;
            if sn.wait > 0.0 {
                return;
            }
            // (Delay(10) resumes: PlayFromStart's update)
            sn.wait = -1.0;
            sn.timeline = 0.0;
        } else {
            sn.timeline = (sn.timeline + dt * sn.rate).min(1.0);
        }
        sn.position = rich_curve(&sn.curve, sn.timeline);
        sn.place(host);
    }

    fn frame(&mut self, host: &mut SceneryHost) {
        self.cam.frame(host);
        for o in &mut self.owners {
            o.frame(&self.cam, host);
        }
    }
}

// ---- treefortSky
// Lady_Rainicorn_Blueprint_C (Lady_Rainicorn_Blueprint.txt): BeginPlay keeps its Y (StartingLocY). ReceiveTick
// (@129..812), while RainicornGate is false: K2_AddActorWorldOffset(0, -DeltaSeconds x RainicornSpeed, 0); at Y <=
// -RainicornDistance the gate closes, the actor goes to (X, StartingLocY, RandomFloatInRange(1000, 2500)) and
// Delay(RandomFloatInRange(0, 20)) opens the gate again (resume @15): it flies from the tick after.
// MoutainCloudSpin_C (MoutainCloudSpin.txt): each tick K2_AddLocalRotation(MakeRotator(0, 0, DeltaSeconds x rate))
// (the yaw) on CloudRing_Mesh / CloudRing_Mesh1 (5), CloudRing_Wisp_Mesh (10) and Sphere (15), the parent of the three
// planes: relative rotation x the yaw about the component's own Z. Its Comp_Parallax moves the whole actor (the
// root's location). Drawn at the frame, by the FTransform rule from the file's relative transforms.

#[derive(Debug)]
struct Lady {
    skeletal: String,
    speed: f64,
    distance: f64,
    height: [f64; 2],
    wait_range: [f64; 2],
    /// The component's UE transform as placed.
    base: M4,
    y0: f64,
    z0: f64,
    y: f64,
    z: f64,
    gate: bool,
    wait: f64,
}

#[derive(Debug)]
struct SpinPart {
    /// Its parent's index in `Spin::order`.
    parent: Option<usize>,
    rel: Xf,
    rate: f64,
    yaw: f64,
    world: Xf,
    instances: Vec<InstanceId>,
}

#[derive(Debug)]
struct Spin {
    /// The parts, parents first.
    order: Vec<SpinPart>,
    owner: Option<ParallaxActor>,
    dirty: bool,
    drawn: [f64; 3],
}

impl Spin {
    fn place(&mut self, host: &mut SceneryHost) {
        let offset = self.owner.as_ref().map(|o| o.offset);
        for k in 0..self.order.len() {
            let p = &self.order[k];
            // the yaw after the relative rotation; the root at its parallax place
            let spun = Xf { t: p.rel.t, q: qmul(p.rel.q, rotator_quat(0.0, p.yaw, 0.0)), s: p.rel.s };
            let world = match p.parent {
                Some(i) => xf_mul(&spun, &self.order[i].world),
                None => {
                    let mut w = spun;
                    if let Some(o) = offset {
                        for (t, o) in w.t.iter_mut().zip(o) {
                            *t += o;
                        }
                    }
                    w
                }
            };
            let p = &mut self.order[k];
            p.world = world;
            let m = world.m4();
            for &id in &p.instances {
                host.place(id, Some(m));
            }
        }
    }
}

#[derive(Debug)]
pub struct Sky {
    cam: ParallaxCamera,
    ladies: Vec<Lady>,
    spins: Vec<Spin>,
}

impl Sky {
    fn make(block: SkyBlock, host: &mut SceneryHost) -> Result<Sky> {
        let what = "scenery: treefortSky";
        let mut ladies = Vec::new();
        for l in block.ladies {
            let k =
                host.skeletals().iter().find(|k| k.name == l.skeletal).ok_or_else(|| bad(what, "Lady Rainicorn"))?;
            let (y0, z0) = (k.loc[1], k.loc[2]);
            ladies.push(Lady {
                base: transform(k.loc, k.rot, k.scale),
                skeletal: l.skeletal,
                speed: l.speed,
                distance: l.distance,
                height: l.height,
                wait_range: l.wait,
                y0,
                z0,
                y: y0,
                z: z0,
                gate: false,
                wait: 0.0,
            });
        }
        let mut spins = Vec::new();
        for s in &block.spins {
            spins.push(Sky::spin(s, host)?);
        }
        Ok(Sky { cam: ParallaxCamera::default(), ladies, spins })
    }

    /// A MoutainCloudSpin actor's parts, parents first, with the instances of each (`spin` the actor; the name
    /// `<actor>.<component>`), checked against the file's places at yaw 0.
    fn spin(s: &SpinBlock, host: &mut SceneryHost) -> Result<Spin> {
        let what = format!("scenery: treefortSky {}", s.actor);
        for name in s.yaw_rate.keys() {
            if !s.comps.contains_key(name) {
                return Err(bad(&what, format!("has no {name}")));
            }
        }
        // parents first
        let mut names: Vec<&str> = Vec::new();
        fn visit<'a>(n: &'a str, comps: &'a BTreeMap<String, Comp>, out: &mut Vec<&'a str>, what: &str) -> Result<()> {
            if out.contains(&n) {
                return Ok(());
            }
            let c = comps.get(n).ok_or_else(|| bad(what, format!("{n} parent")))?;
            if let Some(p) = &c.parent {
                if out.len() > comps.len() {
                    return Err(bad(what, "a parent cycle"));
                }
                visit(p, comps, out, what)?;
            }
            out.push(n);
            Ok(())
        }
        for n in s.comps.keys() {
            visit(n, &s.comps, &mut names, &what)?;
        }
        if names.iter().filter(|n| s.comps[**n].parent.is_none()).count() != 1 {
            return Err(bad(&what, "root"));
        }
        let mut order: Vec<SpinPart> = names
            .iter()
            .map(|n| {
                let c = &s.comps[*n];
                SpinPart {
                    parent: c.parent.as_ref().and_then(|p| names.iter().position(|x| x == p)),
                    rel: Xf::of(&c.place),
                    rate: s.yaw_rate.get(*n).copied().unwrap_or(0.0),
                    yaw: 0.0,
                    world: Xf::default(),
                    instances: Vec::new(),
                }
            })
            .collect();
        let mut placed = Vec::new();
        for (k, inst) in host.instances().iter().enumerate() {
            if inst.fields.get("spin").and_then(Value::as_str) != Some(&s.actor) {
                continue;
            }
            let part =
                inst.name.strip_prefix(&format!("{}.", s.actor)).and_then(|p| names.iter().position(|n| *n == p));
            let part = part.ok_or_else(|| bad(&what, inst.name.clone()))?;
            order[part].instances.push(InstanceId(k as u32));
            placed.push((part, transform(inst.loc, inst.rot, inst.scale), inst.name.clone()));
        }
        let par = s.parallax.unwrap_or([0.0, 0.0]);
        let owner =
            (par[0] != 0.0 || par[1] != 0.0).then(|| ParallaxActor::new(&s.actor, s.comps[names[0]].place.loc, par));
        let mut spin = Spin { order, owner, dirty: true, drawn: [f64::NAN; 3] };
        // (at yaw 0 the composed transforms are the file's places)
        spin.place(host);
        for (part, m, name) in placed {
            if !same(&spin.order[part].world.m4(), &m) {
                return Err(bad(&what, format!("{name} is not where the file placed it")));
            }
        }
        Ok(spin)
    }

    fn begin(&mut self, host: &mut SceneryHost) {
        self.cam.begin(host);
        for d in &mut self.ladies {
            d.y = d.y0;
            d.z = d.z0;
            d.gate = false;
            d.wait = 0.0;
            host.place_skeletal(&d.skeletal, Some(d.base));
        }
        for sp in &mut self.spins {
            for p in &mut sp.order {
                p.yaw = 0.0;
            }
            if let Some(o) = &mut sp.owner {
                o.begin();
            }
            sp.dirty = true;
        }
    }

    fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        for d in &mut self.ladies {
            if d.gate {
                d.wait -= dt;
                if d.wait <= 0.0 {
                    d.gate = false;
                }
                continue;
            }
            d.y -= dt * d.speed;
            if d.y <= -d.distance {
                d.gate = true;
                d.y = d.y0;
                d.z = host.rand(d.height);
                d.wait = host.rand(d.wait_range);
            }
            // (a translation in UE space)
            let mut m = d.base;
            m[3][1] += d.y - d.y0;
            m[3][2] += d.z - d.z0;
            host.place_skeletal(&d.skeletal, Some(m));
        }
        for sp in &mut self.spins {
            for p in &mut sp.order {
                if p.rate != 0.0 {
                    p.yaw = (p.yaw + dt * p.rate) % 360.0;
                }
            }
            sp.dirty = true;
        }
    }

    fn frame(&mut self, host: &mut SceneryHost) {
        self.cam.frame(host);
        for sp in &mut self.spins {
            if let Some(o) = &mut sp.owner {
                o.frame(&self.cam, host);
            }
            let off = sp.owner.as_ref().map(|o| o.offset);
            if !sp.dirty && off.is_none_or(|o| o == sp.drawn) {
                continue;
            }
            if let Some(o) = off {
                sp.drawn = o;
            }
            sp.dirty = false;
            sp.place(host);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rail's mount: linear and cubic segments, the reparam table and the clamped ends.
    #[test]
    fn rail_evaluates_the_spline() {
        let block: RailBlock = serde_json::from_value(serde_json::json!({
            "position": 1,
            "lockOrientation": false,
            "root": { "loc": [0, 0, 0], "rot": [0, 0, 0] },
            "spline": { "position": [
                [0, 0, 0, 0, 10, 0, 0, 10, 0, 0, "Linear"],
                [1, 10, 0, 0, 10, 0, 0, 10, 0, 0, "CurveAuto"],
                [2, 20, 0, 0, 10, 0, 0, 10, 0, 0, "Linear"]
            ] },
            "reparam": [[0, 0], [10, 1], [20, 2]]
        }))
        .unwrap();
        let r = Rail::new(&block).unwrap();
        assert_eq!(r.length, 20.0);
        assert_eq!(r.key_at(5.0), 0.5);
        assert_eq!(r.eval_at(0.5), ([5.0, 0.0, 0.0], [10.0, 0.0, 0.0]));
        // a cubic segment with tangents of the line's slope is the line
        let (p, d) = r.eval_at(1.25);
        assert!((p[0] - 12.5).abs() < 1e-12 && (d[0] - 10.0).abs() < 1e-12, "{p:?} {d:?}");
        assert_eq!(r.mount(1.0).t, [20.0, 0.0, 0.0]);
        assert_eq!(r.mount(-1.0).t, [0.0, 0.0, 0.0]);
    }

    /// MakeFromXZ along +Y: a yaw of 90 degrees.
    #[test]
    fn rail_locks_orientation() {
        let block: RailBlock = serde_json::from_value(serde_json::json!({
            "position": 0,
            "lockOrientation": true,
            "root": { "loc": [100, 0, 0], "rot": [0, 0, 0] },
            "spline": { "position": [
                [0, 0, 0, 0, 0, 1, 0, 0, 1, 0, "Linear"],
                [1, 0, 10, 0, 0, 1, 0, 0, 1, 0, "Linear"]
            ] },
            "reparam": [[0, 0], [10, 1]]
        }))
        .unwrap();
        let m = Rail::new(&block).unwrap().mount(0.5);
        let want = rotator_quat(0.0, 90.0, 0.0);
        assert_eq!(m.t, [100.0, 5.0, 0.0]);
        for c in 0..4 {
            assert!((m.q[c] - want[c]).abs() < 1e-12, "{:?} {want:?}", m.q);
        }
    }

    /// The FTransform rule against stage::transform for a parent with a uniform scale.
    #[test]
    fn xf_mul_composes_as_the_matrices() {
        let child = Place { loc: [10.0, -5.0, 3.0], rot: [10.0, 20.0, 30.0], scale: [2.0, 2.0, 2.0] };
        let parent = Place { loc: [-100.0, 50.0, 7.0], rot: [-11.0, -178.0, 170.0], scale: [6.5, 6.5, 6.5] };
        let got = xf_mul(&Xf::of(&child), &Xf::of(&parent)).m4();
        let want = crate::view::mul(
            &transform(child.loc, child.rot, child.scale),
            &transform(parent.loc, parent.rot, parent.scale),
        );
        assert!(same(&got, &want), "{got:?} {want:?}");
    }

    /// Every Tree Fort entry of the developer's store makes its blocks: the snail on the rail's end and the cloud
    /// ring's parts where the file placed them (make's checks); the clouds move and the blackout sphere goes.
    #[test]
    fn tree_fort_blocks_make() {
        let dir = mvs_store::dev_store_dir();
        let Ok(store) = mvs_store::Store::open(&dir) else {
            eprintln!("no store at {}: nothing to check", dir.display());
            return;
        };
        let Ok(pack) = store.kind("scenery") else { return };
        let mut seen = 0;
        for path in pack.paths().filter(|p| p.to_ascii_lowercase().contains("treehouse")) {
            let bytes = pack.bytes(path).unwrap();
            let bin = mvs_material::jsonbin::JsonBin::split(b"MVSC", &bytes, path).unwrap();
            let h: super::super::SceneryHeader = bin.parse(path).unwrap();
            let s = h.scenery;
            let meshes = s
                .skeletal
                .iter()
                .map(|k| std::sync::Arc::new(mvs_view::mesh::MeshInfo::load(&store, &k.mesh).unwrap()))
                .collect();
            let mut run =
                super::super::SceneryRun::new(&store, 0, Vec::new(), s.instances, s.skeletal, meshes, &s.rest).unwrap();
            let n = run.blocks.iter().filter(|b| matches!(b, super::super::SceneryBlock::TreeFort(_))).count();
            assert_eq!(n, 2, "{path}");
            let mut sound = super::super::NullSound;
            let mut host = SceneryHost { state: &mut run.state, vfx: None, sound: &mut sound, world: None };
            host.state.camera = Some([0.0, 0.0, 0.0]);
            for b in &mut run.blocks {
                b.begin(&mut host);
            }
            let blackout = host.instance("BlackoutSphere").unwrap();
            assert!(host.state.shown[blackout.0 as usize]);
            for _ in 0..60 {
                for b in &mut run.blocks {
                    b.tick(&mut host, 1.0 / 60.0);
                }
            }
            host.state.camera = Some([0.0, 100.0, 0.0]);
            for b in &mut run.blocks {
                b.frame(&mut host, 1.0);
            }
            assert!(!host.state.shown[blackout.0 as usize], "{path}");
            // an actor with Comp_Parallax and no cloud group: the camera's dY (-100) x ParalaxValueY on its Y
            let mut owners = 0;
            for (k, inst) in host.state.instances.iter().enumerate() {
                let Some(par) = inst.fields.get("parallax").and_then(field_f64s) else { continue };
                if inst.fields.contains_key("cloudGroup") {
                    continue;
                }
                let placed = host.state.placed[k].unwrap();
                assert!((placed[3][1] - inst.loc[1] - -100.0 * par[0]).abs() < 1e-9, "{path}: {}", inst.name);
                owners += 1;
            }
            assert!(owners > 0, "{path}");
            // the cloud ring's spinning parts moved
            let ring = host.instance("MoutainCloudSpin2.CloudRing_Mesh").unwrap();
            assert!(host.state.placed[ring.0 as usize].is_some());
            // one snail shown
            let shown = host.state.skeletal.iter().zip(&host.state.skeletal_shown);
            assert_eq!(shown.filter(|(k, on)| k.fields.contains_key("snail") && **on).count(), 1, "{path}");
            seen += 1;
        }
        eprintln!("{seen} Tree Fort entries");
    }
}
