//! Batcave's Batmobiles (BatcaveWallDestructible_C) drawn from their sim state (mvs-sim stages/batcave/car.rs). The
//! data: the block `batcave`'s `cars`; the graphs: BatcaveWallDestructible, BatMobile_AnimBlueprint (offsets: each
//! class's ubergraph's).
//! - A car's pose: its AnimBP (BatMobile_AnimBlueprint_C) under the DefaultSlot's montage. IsActive = !Destroyed
//!   picks the state machine Idle or BatMobile_Retracted_Idle (blend time 0; the latter's time kept across switches,
//!   bResetChildOnActivation false). NoWieght -> Wieght when HasWieght (the sim's `weight`), back when not; each
//!   transition's 0.5 s shows its custom blend graph's pose, the sequence player alone (BatMobile_WieghtAdd /
//!   BatMobile_WieghtLift from 0). A player's time: the ticks since it started plus that tick's own, x 1/60 (the
//!   montages' "+1" rule, tree_fort/pillars).
//! - The graph is updated only in a tick whose montage leaves the slot's source a weight (FAnimNode_Slot::Update
//!   FUN_1424945f0: SourceWeight > 1e-5; the car's slot has bAlwaysUpdateSourcePose false): a full-weight Hit /
//!   Retract / Spawn holds it, players and transitions included. The machine updated after a missed update (the
//!   first, after a hold, or after IsActive false) starts again at NoWieght and takes that update's transitions with
//!   no crossfade (bReinitializeOnBecomingRelevant; Update 142867fa0: bSkipFirstUpdateTransition only drops the
//!   active transitions).
//! - The montage (the sim's `montage` from `montage_at`, till its `montage_done`): its slot track's segments at
//!   (tick - montage_at + 1) / 60, over the AnimBP's pose by its blend in / out weight (Hit's 0.05 s blend out: the
//!   montage's own end). A new montage replaces the last (blend in 0 on all three: a cut).
//! - The montages' queued notifies, fired when previous < at <= position (a play seen late or played again by a
//!   rollback fires none of the passed ones; scoobys-haunted-mansion's rule): AnimNotify_PlayParticleEffect
//!   (attached at its socket, auto destroyed), AnimNotifyState_TimedParticleEffect (destroyed at its end with
//!   bDestroyAtEnd, else deactivated), AnimNotify_PlayVFX (a non-fighter owner's SpawnVFX: attached with its Location /
//!   Rotation / VFX Scale, deactivated at its end with KillOnNotifyEnd), Ak_OneShot (its event on the car's root
//!   AkComponent: the root's place). A state still open when its montage ends or is replaced ends then (NotifyEnd).
//! - SetTopLights (@6788: the four PS_BatmobileHit_PlatformLights at VFX_Light01..04) at each `top_at`,
//!   SetFrontLights (@6157: the two PS_BatmobileHit_Lights at VFX_CarLightL / R) at each `front_at`: the old ones go
//!   on (dropped from their list, auto destroyed). Nothing destroys them: the Front / Top Lights arrays are filled with
//!   temps no statement writes (Array_Add of Temp_object_Variable_1 / _0 @5891 / @6056, _5 .. _2 @6208 .. @6619, not
//!   the spawns' CallFunc_SpawnEmitterAttached_ReturnValue_N), so DestroyDestructible's loop over Front Lights
//!   (@1261-2020) meets nulls (the sim's `front_off` is not drawn).
//! - The splash (PS_BatcaveWaterfallForeGround_Splashing, auto-activated at BeginPlay, on VFX_Root) deactivated from
//!   `splash_off_at` (the Delay(1.5) @535 -> @590); nothing activates it again (the respawn's @2210 does not).
//! - The tint: DestroyDestructible's and the Retract end's SetVectorParameterValueOnMaterials(Color, ...) (@1051,
//!   @2543) name a parameter neither car's materials have (BCAVE_Batmobile_B_MI, M001_BatmobileComic_01_M:
//!   SelectionColor and "Emissive Color" only): nothing to draw, as the build.
//! - A spawn made by a timer or an event (the lights) is first ticked the next frame; a tick stamp seen 6 ticks late
//!   or more (a rollback that played it again) spawns nothing.
// Unverified: a transition taken while another runs replaces it (the custom graphs read no source pose; the 4.26
// stack of active transitions is not read); a state's player starts with the transition into it (its phase: the two
// idles are 2 and 10 frames long).

use std::collections::BTreeMap;

use mvs_sim::stages::batcave::BatcaveCar;
use mvs_vfx::SystemId;
use mvs_view::M4;
use mvs_view::anim::{Pose, blend_pose};
use serde::Deserialize;

use crate::Result;
use crate::scenery::host::SceneryHost;
use crate::scenery::sky_arena::attachment_of;
use crate::stage::{bad, transform};
use crate::view::mul;

const STEP: f64 = 1.0 / 60.0;
/// A tick stamp seen this many ticks late or more spawns nothing.
pub(super) const FX_LATE: i32 = 6;

#[derive(Debug, Deserialize)]
struct Socket {
    bone: String,
    loc: [f64; 3],
    rot: [f64; 3],
    scale: [f64; 3],
}

#[derive(Debug, Deserialize)]
struct Splash {
    system: String,
    socket: Socket,
    loc: [f64; 3],
}

#[derive(Debug, Deserialize)]
struct Segment {
    anim: String,
    pos: f64,
    start: f64,
    end: f64,
    rate: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum NotifyKind {
    Particle,
    Timed,
    Vfx,
    Sound,
}

#[derive(Debug, Deserialize)]
struct Notify {
    at: f64,
    end: Option<f64>,
    kind: NotifyKind,
    system: Option<String>,
    socket: Option<String>,
    loc: Option<[f64; 3]>,
    rot: Option<[f64; 3]>,
    scale: Option<[f64; 3]>,
    #[serde(default)]
    destroy: bool,
    #[serde(default)]
    kill: bool,
    sound: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Play {
    name: String,
    length: f64,
    blend_in: f64,
    blend_out: f64,
    segments: Vec<Segment>,
    notifies: Vec<Notify>,
}

/// A sequence player as the AnimBP plays it.
#[derive(Debug, Deserialize)]
struct Player {
    anim: String,
    length: f64,
}

#[derive(Debug, Deserialize)]
struct Transition {
    time: f64,
    pose: Player,
}

#[derive(Debug, Deserialize)]
struct CarAnim {
    retracted: Player,
    states: Vec<Player>,
    add: Transition,
    lift: Transition,
}

#[derive(Debug, Deserialize)]
struct Lights {
    system: String,
    sockets: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CarLights {
    top: Lights,
    front: Lights,
}

#[derive(Debug, Deserialize)]
struct Plays {
    destroy: Play,
    damage: Play,
    respawn: Play,
}

/// A car of the block's `cars`.
#[derive(Debug, Deserialize)]
pub(super) struct CarData {
    actor: String,
    skeletal: String,
    splash: Splash,
    anim: CarAnim,
    plays: Plays,
    lights: CarLights,
    sockets: BTreeMap<String, Socket>,
    /// The root's place (UE): the Ak_OneShot notifies' AkComponent.
    root: [f64; 3],
}

impl CarData {
    fn plays(&self) -> [&Play; 3] {
        [&self.plays.destroy, &self.plays.damage, &self.plays.respawn]
    }

    /// The animation `c` (build object path).
    fn path(&self, c: Clip) -> &str {
        let g = &self.anim;
        match c {
            Clip::Retracted => &g.retracted.anim,
            Clip::State(s) => &g.states[s].anim,
            Clip::Add => &g.add.pose.anim,
            Clip::Lift => &g.lift.pose.anim,
            Clip::Segment(p, s) => &self.plays()[p].segments[s].anim,
        }
    }
}

/// A place attached under a bone: the bone, and the component's own transform x the socket's (UE, row-vector).
#[derive(Debug)]
struct Follow {
    bone: String,
    local: M4,
}

/// An animation the car's pose shows, as the data names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Clip {
    Retracted,
    State(usize),
    Add,
    Lift,
    /// A play's segment.
    Segment(usize, usize),
}

/// The AnimBP's transition running: NoWieght -> Wieght (`add`) or back (`lift`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tr {
    Add,
    Lift,
}

/// A car as it is drawn.
#[derive(Debug)]
pub(super) struct Car {
    d: CarData,
    /// Every place a system follows: the splash's, the top and front lights', each play's notifies'.
    places: Vec<Follow>,
    splash_at: usize,
    top: Vec<usize>,
    front: Vec<usize>,
    /// Per play, per notify, its place (none for a sound).
    notify_at: [Vec<Option<usize>>; 3],
    /// The systems that follow their place.
    followers: Vec<(SystemId, usize)>,
    // the AnimBP: the tick it last ran, the tick the machine was last updated, its state and transition
    last: i32,
    sm_at: i32,
    state: usize,
    state_at: i32,
    tr: Option<Tr>,
    tr_at: i32,
    retracted: i32,
    // the montage (a play's index) and its notify states (by notify)
    start: i32,
    pos: f64,
    seen_start: i32,
    seen: f64,
    fx: Vec<Option<SystemId>>,
    fx_of: Option<usize>,
    // this tick's pose: the base player and time, the montage's clip, time and weight; dirty: to be written
    base: Option<Clip>,
    base_t: f64,
    mont: Option<Clip>,
    mont_t: f64,
    mont_w: f64,
    dirty: bool,
    rest: Pose,
    base_pose: Pose,
    tmp: Pose,
    // the splash, the lights
    splash: Option<SystemId>,
    splash_off: bool,
    top_fx: Vec<Option<SystemId>>,
    top_seen: i32,
    front_fx: Vec<Option<SystemId>>,
    front_seen: i32,
}

fn kill(host: &mut SceneryHost, id: Option<SystemId>) {
    if let (Some(id), Some(v)) = (id, host.vfx()) {
        v.kill(id);
    }
}

fn deactivate(host: &mut SceneryHost, id: Option<SystemId>) {
    if let (Some(id), Some(v)) = (id, host.vfx()) {
        v.deactivate(id);
    }
}

impl Car {
    pub(super) fn make(d: CarData, host: &mut SceneryHost) -> Result<Car> {
        let what = format!("scenery: batcave car {}", d.actor);
        if host.skeletal_index(&d.skeletal).is_none() {
            return Err(bad(&what, format!("{} is not a skeletal mesh of the file", d.skeletal)));
        }
        if d.anim.states.len() != 2 {
            return Err(bad(&what, "the AnimBP's two states"));
        }
        let mut places = Vec::new();
        let mut place = |socket: &Socket, local: M4| -> Result<usize> {
            if host.bone_world(&d.skeletal, &socket.bone).is_none() {
                return Err(bad(&what, format!("no bone {}", socket.bone)));
            }
            // (the component's own transform, then the socket's: row-vector)
            places.push(Follow {
                bone: socket.bone.clone(),
                local: mul(&local, &transform(socket.loc, socket.rot, socket.scale)),
            });
            Ok(places.len() - 1)
        };
        let socket_of =
            |name: &str| d.sockets.get(name).ok_or_else(|| bad(&what, format!("socket {name} not in the file")));
        let id = transform([0.0; 3], [0.0; 3], [1.0; 3]);
        let splash_at = place(&d.splash.socket, transform(d.splash.loc, [0.0; 3], [1.0; 3]))?;
        let top = d.lights.top.sockets.iter().map(|s| place(socket_of(s)?, id)).collect::<Result<Vec<_>>>()?;
        let front = d.lights.front.sockets.iter().map(|s| place(socket_of(s)?, id)).collect::<Result<Vec<_>>>()?;
        let mut notify_at: [Vec<Option<usize>>; 3] = Default::default();
        for (k, p) in d.plays().into_iter().enumerate() {
            if p.segments.is_empty() {
                return Err(bad(&what, format!("{}: no segment", p.name)));
            }
            for nt in &p.notifies {
                let at = match nt.kind {
                    NotifyKind::Sound => {
                        if nt.sound.is_none() {
                            return Err(bad(&what, format!("{}: a sound notify with no event", p.name)));
                        }
                        None
                    }
                    _ => {
                        if nt.system.is_none() {
                            return Err(bad(&what, format!("{}: a notify with no system", p.name)));
                        }
                        let s = socket_of(nt.socket.as_deref().unwrap_or(""))?;
                        let local = transform(
                            nt.loc.unwrap_or([0.0; 3]),
                            nt.rot.unwrap_or([0.0; 3]),
                            nt.scale.unwrap_or([1.0; 3]),
                        );
                        Some(place(s, local)?)
                    }
                };
                notify_at[k].push(at);
            }
        }
        let mut car = Car {
            places,
            splash_at,
            top_fx: vec![None; top.len()],
            front_fx: vec![None; front.len()],
            top,
            front,
            notify_at,
            followers: Vec::new(),
            last: -1,
            sm_at: -2,
            state: 0,
            state_at: 0,
            tr: None,
            tr_at: 0,
            retracted: 0,
            start: -1,
            pos: -1.0,
            seen_start: -1,
            seen: 0.0,
            fx: Vec::new(),
            fx_of: None,
            base: None,
            base_t: 0.0,
            mont: None,
            mont_t: 0.0,
            mont_w: 0.0,
            dirty: true,
            rest: Pose::default(),
            base_pose: Pose::default(),
            tmp: Pose::default(),
            splash: None,
            splash_off: false,
            top_seen: -1,
            front_seen: -1,
            d,
        };
        // (every animation the car plays is one of the scenery's: SceneryRun::new loads them with the block)
        let mut clips = vec![Clip::Retracted, Clip::State(0), Clip::State(1), Clip::Add, Clip::Lift];
        for (p, play) in car.d.plays().into_iter().enumerate() {
            clips.extend((0..play.segments.len()).map(|s| Clip::Segment(p, s)));
        }
        for c in clips {
            let path = car.d.path(c);
            if host.state.clips.get(path).is_none() {
                return Err(bad(&what, format!("the animation {path} is not loaded")));
            }
        }
        // the AnimBP's pose, the block's from now on
        host.drive(&car.d.skeletal);
        let rest = host.rest(&car.d.skeletal).cloned().unwrap_or_default();
        car.base_pose = Pose::new(rest.n);
        car.tmp = Pose::new(rest.n);
        car.rest = rest;
        Ok(car)
    }

    /// A montage's pose is on the car.
    #[cfg(test)]
    pub(super) fn montage_on(&self) -> bool {
        self.mont.is_some()
    }

    pub(super) fn actor(&self) -> &str {
        &self.d.actor
    }

    /// A system spawned at place `at`, following it.
    fn spawn(&mut self, host: &mut SceneryHost, system: &str, at: usize, next: bool) -> Option<SystemId> {
        let f = &self.places[at];
        let a = attachment_of(&mul(&f.local, &host.bone_world(&self.d.skeletal, &f.bone)?));
        let id = host.vfx()?.spawn_level(system, &a, next, &[], 0, false)?;
        self.followers.push((id, at));
        Some(id)
    }

    /// The followed systems at their places as the pose stands (VfxPlayer's follow, before its tick); the dead ones
    /// dropped.
    pub(super) fn follow(&mut self, host: &mut SceneryHost) {
        let mut k = 0;
        while k < self.followers.len() {
            let (id, at) = self.followers[k];
            if !host.vfx().is_some_and(|v| v.alive(id)) {
                self.followers.swap_remove(k);
                continue;
            }
            let f = &self.places[at];
            if let Some(bone) = host.bone_world(&self.d.skeletal, &f.bone) {
                let a = attachment_of(&mul(&f.local, &bone));
                if let Some(v) = host.vfx() {
                    v.attach(id, &a);
                }
            }
            k += 1;
        }
    }

    /// The pose written when its players or times changed: a full-weight montage alone; no
    /// update of the graph yet, the reference pose; else the graph's player under the montage by its weight.
    pub(super) fn pose(&mut self, host: &mut SceneryHost) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let (d, sk) = (&self.d, &self.d.skeletal);
        let mont = self.mont.map(|m| d.path(m));
        if let Some(m) = mont
            && self.mont_w >= 1.0
        {
            host.sample(sk, m, self.mont_t, &mut self.tmp);
            if let Some(p) = host.pose(sk) {
                p.copy(&self.tmp);
            }
            return;
        }
        let Some(base) = self.base else {
            if let Some(p) = host.pose(sk) {
                p.copy(&self.rest);
            }
            return;
        };
        host.sample(sk, d.path(base), self.base_t, &mut self.base_pose);
        let blend = match mont {
            Some(m) if self.mont_w > 0.0 => {
                host.sample(sk, m, self.mont_t, &mut self.tmp);
                true
            }
            _ => false,
        };
        let Some(p) = host.pose(sk) else { return };
        if blend {
            blend_pose(&self.base_pose, &self.tmp, self.mont_w, p);
        } else {
            p.copy(&self.base_pose);
        }
    }

    /// The AnimBP's update this tick (BatMobile_AnimBlueprint: IsActive, the state machine Idle), when the slot
    /// updates its source.
    fn graph_tick(&mut self, a: &BatcaveCar, tick: i32) {
        let g = &self.d.anim;
        let (base, t);
        if !a.wall.destroyed {
            let want = usize::from(a.weight);
            if self.sm_at != tick - 1 {
                // a missed update: the entry state, and this update's transition with no crossfade
                // (MaxTransitionsPerFrame 3: the way back's rule is the negation, so one at most)
                self.tr = None;
                self.state = want;
                self.state_at = tick;
            } else if want != self.state {
                self.tr = Some(if want == 1 { Tr::Add } else { Tr::Lift });
                self.tr_at = tick;
                self.state = want;
                self.state_at = tick;
            }
            self.sm_at = tick;
            let tr_of = |tr: Tr| if tr == Tr::Add { &g.add } else { &g.lift };
            if let Some(tr) = self.tr
                && f64::from(tick - self.tr_at + 1) * STEP >= tr_of(tr).time
            {
                self.tr = None;
            }
            if let Some(tr) = self.tr {
                let x = tr_of(tr);
                base = if tr == Tr::Add { Clip::Add } else { Clip::Lift };
                t = (f64::from(tick - self.tr_at + 1) * STEP).min(x.pose.length);
            } else {
                let p = &g.states[self.state];
                base = Clip::State(self.state);
                t = (f64::from(tick - self.state_at + 1) * STEP) % p.length;
            }
        } else {
            self.retracted += 1;
            base = Clip::Retracted;
            t = (f64::from(self.retracted) * STEP).min(g.retracted.length);
        }
        if Some(base) != self.base || t != self.base_t {
            self.base = Some(base);
            self.base_t = t;
            self.dirty = true;
        }
    }

    fn graph_reset(&mut self) {
        self.last = -1;
        self.sm_at = -2;
        self.state = 0;
        self.state_at = 0;
        self.tr = None;
        self.retracted = 0;
    }

    /// A notify state's end (NotifyEnd): a timed effect destroyed (bDestroyAtEnd) or deactivated, a PlayVFX
    /// deactivated with KillOnNotifyEnd, else left as it is.
    fn end_state(&mut self, host: &mut SceneryHost, k: usize) {
        let Some(fx) = self.fx[k] else { return };
        if let Some(p) = self.fx_of {
            let nt = &self.d.plays()[p].notifies[k];
            match nt.kind {
                NotifyKind::Timed if nt.destroy => kill(host, Some(fx)),
                NotifyKind::Timed => deactivate(host, Some(fx)),
                NotifyKind::Vfx if nt.kill => deactivate(host, Some(fx)),
                _ => {}
            }
        }
        self.fx[k] = None;
    }

    fn end_states(&mut self, host: &mut SceneryHost) {
        for k in 0..self.fx.len() {
            self.end_state(host, k);
        }
    }

    /// The montage this tick (None: none), its pose and its notifies.
    fn montage_tick(&mut self, host: &mut SceneryHost, tick: i32, play: Option<usize>, start: i32) {
        self.start = if play.is_some() { start } else { -1 };
        self.pos = if play.is_some() { f64::from(tick - start + 1) * STEP } else { -1.0 };
        if self.start != self.seen_start || play != self.fx_of {
            // a new play ends the last one's states; one played again (a rollback) or seen late fires no passed
            // notify (Deviation: a different montage at the same start, a rollback, is a new play, where a key on the
            // start alone would grow the notify list)
            self.end_states(host);
            self.seen =
                if self.start > self.seen_start && tick - self.start < FX_LATE { f64::NEG_INFINITY } else { self.pos };
            self.seen_start = self.start;
            self.fx_of = play;
            self.fx.clear();
            if let Some(p) = play {
                self.fx.resize(self.d.plays()[p].notifies.len(), None);
            }
        }
        let Some(pi) = play else {
            if self.mont.is_some() {
                self.mont = None;
                self.mont_w = 0.0;
                self.dirty = true;
            }
            return;
        };
        let pl = self.d.plays()[pi];
        let pos = self.pos.min(pl.length);
        // the slot's pose: the segment at the position, the blend weight
        let mut s = 0;
        for (k, x) in pl.segments.iter().enumerate() {
            if pos >= x.pos {
                s = k;
            }
        }
        let seg = &pl.segments[s];
        let mont = Clip::Segment(pi, s);
        let mt = seg.end.min(seg.start + (pos - seg.pos) * seg.rate);
        let w_in = if pl.blend_in > 0.0 { (pos / pl.blend_in).min(1.0) } else { 1.0 };
        let w_out = if pl.blend_out > 0.0 { ((pl.length - pos) / pl.blend_out).clamp(0.0, 1.0) } else { 1.0 };
        let mw = w_in.min(w_out);
        if Some(mont) != self.mont || mt != self.mont_t || mw != self.mont_w {
            self.mont = Some(mont);
            self.mont_t = mt;
            self.mont_w = mw;
            self.dirty = true;
        }
        let done = self.pos >= pl.length;
        for k in 0..pl.notifies.len() {
            let nt = &self.d.plays()[pi].notifies[k];
            if nt.at > self.seen && nt.at <= pos {
                if nt.kind == NotifyKind::Sound {
                    let (sound, root) = (nt.sound.clone().unwrap_or_default(), self.d.root);
                    host.sound(&sound, root, None);
                } else {
                    self.end_state(host, k);
                    let nt = &self.d.plays()[pi].notifies[k];
                    let (system, kind) = (nt.system.clone().unwrap_or_default(), nt.kind);
                    let fx = self.notify_at[pi][k].and_then(|at| self.spawn(host, &system, at, false));
                    // (a PlayParticleEffect is auto destroyed and nobody holds it)
                    if kind != NotifyKind::Particle {
                        self.fx[k] = fx;
                    }
                }
            }
            let end = self.d.plays()[pi].notifies[k].end;
            if self.fx[k].is_some() && (done || end.is_some_and(|e| e <= pos)) {
                self.end_state(host, k);
            }
        }
        self.seen = pos;
    }

    /// Spawned lights (`top` or the front ones): a new stamp spawns them (the old ones go on); a stamp put back (a
    /// rollback, a reset) ends them.
    fn lights_tick(&mut self, host: &mut SceneryHost, top: bool, at: i32, tick: i32) {
        let seen = if top { self.top_seen } else { self.front_seen };
        if at == seen {
            return;
        }
        let n = if top { self.top.len() } else { self.front.len() };
        if at < seen {
            for k in 0..n {
                let fx = if top { self.top_fx[k].take() } else { self.front_fx[k].take() };
                kill(host, fx);
            }
        }
        if at >= 0 && tick - at < FX_LATE {
            let system = if top { self.d.lights.top.system.clone() } else { self.d.lights.front.system.clone() };
            for k in 0..n {
                let place = if top { self.top[k] } else { self.front[k] };
                let fx = self.spawn(host, &system, place, true);
                if top {
                    self.top_fx[k] = fx;
                } else {
                    self.front_fx[k] = fx;
                }
            }
        }
        if top {
            self.top_seen = at;
        } else {
            self.front_seen = at;
        }
    }

    fn spawn_splash(&mut self, host: &mut SceneryHost) {
        let system = self.d.splash.system.clone();
        self.splash = self.spawn(host, &system, self.splash_at, false);
    }

    fn reset(&mut self, host: &mut SceneryHost) {
        self.graph_reset();
        self.end_states(host);
        self.fx_of = None;
        self.start = -1;
        self.seen_start = -1;
        self.pos = -1.0;
        self.fx.clear();
        self.base = None;
        self.mont = None;
        self.mont_w = 0.0;
        self.dirty = true;
        for k in 0..self.top_fx.len() {
            kill(host, self.top_fx[k].take());
        }
        for k in 0..self.front_fx.len() {
            kill(host, self.front_fx[k].take());
        }
        self.top_seen = -1;
        self.front_seen = -1;
        kill(host, self.splash.take());
        self.spawn_splash(host);
        self.splash_off = false;
    }

    /// The level begins: the systems of the level before are gone (VfxPlayer.endAll), the reference pose, the splash.
    pub(super) fn begin(&mut self, host: &mut SceneryHost) {
        self.splash = None;
        self.fx.clear();
        self.fx_of = None;
        self.top_fx.fill(None);
        self.front_fx.fill(None);
        self.followers.clear();
        self.reset(host);
        self.pose(host);
    }

    fn play_of(&self, a: &BatcaveCar) -> Option<usize> {
        if a.montage.is_empty() || a.montage_done {
            return None;
        }
        self.d.plays().iter().position(|p| p.name == a.montage)
    }

    /// One tick of the World's car `a`.
    pub(super) fn tick(&mut self, host: &mut SceneryHost, a: &BatcaveCar, tick: i32) {
        // a tick put back (a Lab reset): everything from the start
        if tick < self.last {
            self.reset(host);
        }
        self.last = tick;
        let play = self.play_of(a);
        self.montage_tick(host, tick, play, a.montage_at);
        // (the slot's source weight 1 - the montage's)
        if self.mont.is_none() || 1.0 - self.mont_w > 1e-5 {
            self.graph_tick(a, tick);
        }
        self.lights_tick(host, true, a.top_at, tick);
        self.lights_tick(host, false, a.front_at, tick);
        let off = a.splash_off_at >= 0;
        if off != self.splash_off {
            self.splash_off = off;
            if off {
                deactivate(host, self.splash);
            } else {
                kill(host, self.splash.take());
                self.spawn_splash(host);
            }
        }
    }
}
