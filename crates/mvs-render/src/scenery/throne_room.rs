//! Throne Room (Map_M008): the block its scenery entry has of its own Blueprint actors and level Blueprints
//! Block `throneRoom`.
//!
//! - The five movers (M008_Elevator_BP_C: `movers` in the sim's ROLE_NAMES order): each skeletal mesh's component (its
//!   level transform, the left ones' scale (1, -1, 1) in it) moved by its root's travel, which is its Box solid's
//!   (dx, dy) in the sim (the Box and the SkeletalMesh both sit under DefaultSceneRoot, which MoveComponentTo moves),
//!   UE (0, -dx, dy). Drawn between the tick before and the latest at the frame's alpha, as the platforms and the
//!   fighters are (platform_blend.rs: a move the tick made is drawn on the way, a reset or a rollback where it is
//!   now).
//! - The level script's animation calls (`anims`: per component its calls, keyed to the sim's presentation calls,
//!   M008Presentation; `cached`: the ten whose play rates BeginPlay caches). A single node instance:
//!   - PlayAnimation(asset, false): the asset from 0 at rate 1.0, playing, not looping;
//!   - Play(false): playing from where it is, at its rate, the asset it has (the intro five, the wall gears);
//!   - ResetAnims (each PlayTransformAnim): the ten at 0 (SetPosition, no notifies), their rate the cached one; the
//!     playing flag left as it is (it stops nothing);
//!   - PlayAnimsInReverse: the ten at minus the cached rate, playing from where they are;
//!   - each update: time += dt x rate x the asset's RateScale while playing, clamped to [0, length] (not looping), the
//!     playing flag off at the end of its direction: a play holds its end, a reverse holds 0.
//!
//!   The cached rate is the component's SavedPlayRate (1 for the ten: none saves one), the intro's rate the same (the
//!   throne's 0.8). The call's tick is the world tick of its Delay's link (the leftover pass, after the components'
//!   animation update of that frame): its effect first shows a tick later, with one update made. The pose is a pure
//!   function of the world tick and the presentation ticks: each component's calls with a tick before the current one
//!   are replayed in tick order from the BeginPlay state (the five intro calls happen once, the others once a cycle
//!   in a fixed order, so the latest of each are the component's latest calls: from the first PlayAnimation among
//!   them nothing before matters), so a Lab reset, a rollback or a page that joins late draws the same pose. With no
//!   world (outside a match) the BeginPlay state: the reference pose of the movers, the placed assets at their start.
//! - The stage (M008_Stage_01_BP_C: StaticMesh and Skelmesh, M008_Stage_01_Fold at 0) stands where it is; its Skelmesh
//!   plays as above.
//! - The four fire pillars (M008_Pillar_01_SkelMesh*_Blueprint_C) play their Raise once from the class's play data
//!   (the file's skeletal entry: not looping, `start`, rate) and at BeginPlay attach PS_M008_Fire to the bone
//!   Pillar_Buttom (SpawnEmitterAttached @10, KeepRelativeOffset, offset 0, scale 1, auto destroyed): spawned at
//!   begin, following the bone.
// Deviation: the pillars' fire takes the bone as the tick before the skeletal meshes' animation left it (the
// animations run after the blocks' ticks here, the follow is evaluated in the particle player's tick after
// them).

use std::sync::Arc;

use mvs_sim::stages::throne_room::{M008Elevator, M008Presentation, ThroneRoomOwn};
use mvs_sim::world::stage::StageOwn;
use mvs_sim::world::stage_actors::StageActorOwn;
use mvs_vfx::SystemId;
use mvs_view::M4;
use serde::Deserialize;
use serde_json::Value;

use super::host::SceneryHost;
use super::platform_blend::PlatformBlend;
use super::sky_arena::attachment_of;
use crate::Result;
use crate::stage::{bad, transform};

#[derive(Debug, Deserialize)]
struct MoverData {
    skeletal: String,
}

#[derive(Debug, Deserialize)]
struct FireData {
    system: String,
    bone: String,
}

#[derive(Debug, Deserialize)]
struct PillarData {
    actor: String,
    skeletal: String,
    fire: FireData,
}

#[derive(Debug, Deserialize)]
struct AnimData {
    skeletal: String,
    /// [the presentation field, the op, the PlayAnimation asset].
    calls: Vec<(String, String, Option<String>)>,
}

#[derive(Debug, Deserialize)]
struct ThroneBlock {
    movers: Vec<MoverData>,
    pillars: Vec<PillarData>,
    cached: Vec<String>,
    anims: Vec<AnimData>,
}

/// The call of the sim's presentation a level script call is keyed to (M008Presentation's fields by name).
#[derive(Clone, Copy, Debug)]
enum Field {
    IntroGears,
    IntroCentre,
    IntroSides,
    IntroThrone,
    Transform,
    FloorFold,
    TowerFold,
    WallGears,
    ElevatorFold,
    StageFold,
    Reverse,
}

impl Field {
    fn named(name: &str) -> Option<Field> {
        Some(match name {
            "introGears" => Field::IntroGears,
            "introCentre" => Field::IntroCentre,
            "introSides" => Field::IntroSides,
            "introThrone" => Field::IntroThrone,
            "transform" => Field::Transform,
            "floorFold" => Field::FloorFold,
            "towerFold" => Field::TowerFold,
            "wallGears" => Field::WallGears,
            "elevatorFold" => Field::ElevatorFold,
            "stageFold" => Field::StageFold,
            "reverse" => Field::Reverse,
            _ => return None,
        })
    }

    /// The world tick of the call, -1 before the first.
    fn tick(self, p: &M008Presentation) -> i32 {
        match self {
            Field::IntroGears => p.intro_gears,
            Field::IntroCentre => p.intro_centre,
            Field::IntroSides => p.intro_sides,
            Field::IntroThrone => p.intro_throne,
            Field::Transform => p.transform,
            Field::FloorFold => p.floor_fold,
            Field::TowerFold => p.tower_fold,
            Field::WallGears => p.wall_gears,
            Field::ElevatorFold => p.elevator_fold,
            Field::StageFold => p.stage_fold,
            Field::Reverse => p.reverse,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Asset,
    Play,
    Reset,
    Reverse,
}

/// A PlayAnimation asset: the scenery's sequence by build object path, its length and RateScale.
#[derive(Clone, Debug)]
struct ClipRef {
    path: Arc<str>,
    length: f64,
    rate_scale: f64,
}

#[derive(Debug)]
struct Call {
    field: Field,
    op: Op,
    clip: Option<ClipRef>,
}

/// One animated component: its calls, its BeginPlay state and the asset and time its pose is at.
#[derive(Debug)]
struct Comp {
    name: String,
    calls: Vec<Call>,
    /// The calls' ticks this tick (-1: not yet).
    tick: Vec<i32>,
    /// The calls made, in tick order.
    order: Vec<usize>,
    /// The BeginPlay position (SavedPosition).
    start: f64,
    /// The BeginPlay rate (SavedPlayRate: GetPlayRate @8194..8842).
    cached_rate: f64,
    /// The file's own asset (the placed one).
    first: Option<ClipRef>,
    /// The asset and the time the pose is at.
    clip: Option<ClipRef>,
    time: f64,
}

impl Comp {
    /// The state after the calls with a tick before `now`, each replayed from its tick (the update count from there),
    /// into `clip` / `time`.
    fn replay(&mut self, now: i32, dt: f64) {
        self.order.clear();
        for i in 0..self.calls.len() {
            let t = self.tick[i];
            if t < 0 || t >= now {
                continue;
            }
            let mut j = self.order.len();
            self.order.push(i);
            while j > 0 && self.tick[self.order[j - 1]] > t {
                self.order[j] = self.order[j - 1];
                j -= 1;
            }
            self.order[j] = i;
        }
        let n = self.order.len();
        let mut clip = self.first.clone();
        let (mut time, mut rate, mut playing) = (self.start, self.cached_rate, false);
        for k in 0..n {
            let call = &self.calls[self.order[k]];
            match call.op {
                Op::Asset => {
                    clip = call.clip.clone();
                    time = 0.0;
                    rate = 1.0;
                    playing = true;
                }
                Op::Play => playing = true,
                Op::Reset => {
                    time = 0.0;
                    rate = self.cached_rate;
                }
                Op::Reverse => {
                    rate = -self.cached_rate;
                    playing = true;
                }
            }
            // the updates from the call's to the next call's or to now
            let until = if k + 1 < n { self.tick[self.order[k + 1]] } else { now };
            let Some(c) = clip.as_ref().filter(|_| playing) else { continue };
            let r = rate * c.rate_scale;
            if r == 0.0 {
                continue;
            }
            time += f64::from(until - self.tick[self.order[k]]) * dt * r;
            if r > 0.0 && time >= c.length {
                time = c.length;
                playing = false;
            } else if r < 0.0 && time <= 0.0 {
                time = 0.0;
                playing = false;
            }
        }
        self.time = clip.as_ref().map_or(0.0, |c| time.max(0.0).min(c.length));
        self.clip = clip;
    }
}

#[derive(Debug)]
struct Mover {
    skeletal: String,
    base: M4,
    dy: f64,
    dz: f64,
    blend: PlatformBlend,
}

#[derive(Debug)]
struct Fire {
    system: String,
    skeletal: String,
    bone: String,
    id: Option<SystemId>,
}

/// The `throneRoom` block.
#[derive(Debug)]
pub struct ThroneRoom {
    movers: Vec<Mover>,
    comps: Vec<Comp>,
    fires: Vec<Fire>,
    /// The scenery tick's time (the world step).
    dt: f64,
}

impl ThroneRoom {
    pub fn make(key: &str, data: &Value, host: &mut SceneryHost) -> Result<ThroneRoom> {
        let what = format!("scenery: throne room {key}");
        let d: ThroneBlock = serde_json::from_value(data.clone()).map_err(|e| bad(&what, e.to_string()))?;
        let mut movers = Vec::new();
        for m in &d.movers {
            let Some(k) = host.skeletal_index(&m.skeletal) else {
                return Err(bad(&what, "movers not in the file"));
            };
            let s = &host.skeletals()[k];
            movers.push(Mover {
                skeletal: m.skeletal.clone(),
                base: transform(s.loc, s.rot, s.scale),
                dy: 0.0,
                dz: 0.0,
                blend: PlatformBlend::default(),
            });
        }
        let mut fires = Vec::new();
        for p in &d.pillars {
            if host.skeletal_index(&p.skeletal).is_none() || host.bone_world(&p.skeletal, &p.fire.bone).is_none() {
                return Err(bad(&what, format!("pillar {} has no bone {}", p.actor, p.fire.bone)));
            }
            fires.push(Fire {
                system: p.fire.system.clone(),
                skeletal: p.skeletal.clone(),
                bone: p.fire.bone.clone(),
                id: None,
            });
        }
        let clip_of = |host: &SceneryHost, name: &str| -> Result<ClipRef> {
            let c = host.state.clips.get(name).ok_or_else(|| bad(&what, format!("{name} not loaded")))?;
            let a = &host.state.clips.clips[c].anim;
            Ok(ClipRef { path: Arc::from(name), length: a.length, rate_scale: a.rate_scale })
        };
        let mut comps = Vec::new();
        for a in &d.anims {
            let name = &a.skeletal;
            let Some(k) = host.skeletal_index(name) else {
                return Err(bad(&what, format!("{name} is not a skeletal mesh of the file")));
            };
            // (BeginPlay: not playing, so the file has it at rate 0; its rate the saved one)
            let (rate, looping, first) = {
                let an = host.skeletal(name).expect("a skeletal mesh of the file");
                (an.rate, an.looping, an.clip.clone())
            };
            let first = first.as_deref().map(|n| clip_of(host, n)).transpose()?;
            if rate != 0.0 || looping {
                return Err(bad(&what, format!("{name} is not a paused single-node animation")));
            }
            let fields = &host.skeletals()[k].fields;
            let start = fields.get("start").and_then(Value::as_f64).unwrap_or(0.0);
            let cached_rate = fields.get("savedRate").and_then(Value::as_f64).unwrap_or(1.0);
            let mut calls = Vec::new();
            for (field, op, asset) in &a.calls {
                let field = Field::named(field).ok_or_else(|| bad(&what, format!("{name}: the call {field}")))?;
                let op = match op.as_str() {
                    "asset" => Op::Asset,
                    "play" => Op::Play,
                    "reset" => Op::Reset,
                    "reverse" => Op::Reverse,
                    other => return Err(bad(&what, format!("{name}: the op {other}"))),
                };
                let clip = asset.as_deref().map(|n| clip_of(host, n)).transpose()?;
                calls.push(Call { field, op, clip });
            }
            let n = calls.len();
            comps.push(Comp {
                name: name.clone(),
                calls,
                tick: vec![-1; n],
                order: Vec::with_capacity(n),
                start,
                cached_rate,
                clip: first.clone(),
                first,
                time: start,
            });
        }
        for name in &d.cached {
            if !comps.iter().any(|c| c.name == *name) {
                return Err(bad(&what, format!("{name} cached and not played")));
            }
        }
        Ok(ThroneRoom { movers, comps, fires, dt: 1.0 / 60.0 })
    }

    /// A bone's UE world transform as an emitter's attachment.
    fn bone_at(host: &SceneryHost, f: &Fire) -> mvs_vfx::Attachment {
        let m = host
            .bone_world(&f.skeletal, &f.bone)
            .unwrap_or_else(|| panic!("scenery: throne room {} has no bone {}", f.skeletal, f.bone));
        attachment_of(&m)
    }

    pub fn begin(&mut self, host: &mut SceneryHost) {
        for f in &mut self.fires {
            if let (Some(id), Some(v)) = (f.id.take(), host.vfx()) {
                v.kill(id);
            }
        }
        for i in 0..self.fires.len() {
            let at = Self::bone_at(host, &self.fires[i]);
            self.fires[i].id = host.vfx().and_then(|v| v.spawn_level(&self.fires[i].system, &at, false, &[], 0, false));
        }
    }

    pub fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        self.dt = dt;
        let mut now = 0;
        {
            let w = host.world();
            // the mover offsets of the world's latest tick (none: at their level place)
            let mut seen = 0u32;
            if let Some(run) = w.and_then(|w| w.stage_run.as_ref()) {
                for a in &run.actors {
                    let StageActorOwn::M008Elevator(M008Elevator { role, floor, .. }) = &a.own else { continue };
                    let (Ok(role), Ok(floor)) = (usize::try_from(*role), usize::try_from(*floor)) else { continue };
                    let Some(s) = run.solids.get(floor) else { continue };
                    let Some(m) = self.movers.get_mut(role) else { continue };
                    // (sim x = -UE Y, sim y = UE Z)
                    m.blend.tick(f64::from(-s.m.dx), f64::from(s.m.dy), Some((f64::from(s.m.mx), f64::from(s.m.my))));
                    seen |= 1 << role;
                }
            }
            for (k, m) in self.movers.iter_mut().enumerate() {
                if seen & (1 << k) == 0 {
                    m.blend.tick(0.0, 0.0, None);
                }
            }
            // (the three script instances call at the same ticks: the first one's)
            let none = M008Presentation::default();
            let shown = w
                .and_then(|w| w.stage_run.as_ref())
                .and_then(|r| match &r.s {
                    StageOwn::ThroneRoom(ThroneRoomOwn { scripts, .. }) => scripts.first(),
                    _ => None,
                })
                .map_or(&none, |s| &s.shown);
            if let Some(w) = w {
                now = w.flow.tick;
            }
            for c in &mut self.comps {
                for (t, call) in c.tick.iter_mut().zip(&c.calls) {
                    *t = call.field.tick(shown);
                }
            }
        }
        for c in &mut self.comps {
            c.replay(now, dt);
            // the single node's pose: the asset at the replayed time (sampled again only when either changed)
            if let Some(a) = host.skeletal(&c.name) {
                a.clip = c.clip.as_ref().map(|c| c.path.clone());
                a.time = c.time;
                a.rate = 0.0;
                a.looping = false;
            }
        }
        // (the fires follow their bones)
        for i in 0..self.fires.len() {
            let Some(id) = self.fires[i].id else { continue };
            if !host.vfx().is_some_and(|v| v.alive(id)) {
                self.fires[i].id = None;
                continue;
            }
            let at = Self::bone_at(host, &self.fires[i]);
            if let Some(v) = host.vfx() {
                v.attach(id, &at);
            }
        }
    }

    pub fn frame(&mut self, host: &mut SceneryHost, alpha: f64) {
        for m in &mut self.movers {
            let (dy, dz) = m.blend.at(alpha);
            if dy == m.dy && dz == m.dz {
                continue;
            }
            m.dy = dy;
            m.dz = dz;
            // the holder's matrix as made, translated by (0, dy, dz) in the world
            let mut t = m.base;
            t[3][1] += dy;
            t[3][2] += dz;
            host.place_skeletal(&m.skeletal, Some(t));
        }
    }

    pub fn end(&mut self, host: &mut SceneryHost) {
        let _ = host;
    }
}
