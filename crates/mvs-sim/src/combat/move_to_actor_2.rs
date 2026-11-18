//! Buff_MoveToActor2 (Panda_Main/Blueprints/Abilities/Shield): a force
//! move to an actor that may move, its source replaced whenever the actor has left the planned point by more than
//! BufferDistance, its arrival broadcast (ReachedActor) and a LaunchCharacter at its end whatever ended it. The build's
//! one spawner is Gizmo's Comp_C023A_AllyAttach AtemptToReachAlly (@466 SpawnBuffActor, @1386 MulticastBeginMove(Actor,
//! false, false, distance / DashToSpeedPerSecond, (0, 0, half height), direction x 2500), @1460 binds ReachedActor to
//! its ActorReachedLogic). Offsets are the class's ubergraph's.
//! - ReceiveBeginPlay @3839: with authority and DestroyOnKnockedBack (class default true) the owner's
//!   Comp_Pawn_DamageManager.OnKnockedBack is bound (@4093) -> OnOwnerKnockedBack @4135 K2_DestroyActor.
//! - MulticastBeginMove(ActorDestination, bEaseOut, bEaseIn, OverTime, LocationOffset, LaunchOnEnd) @3641: Launch on
//!   End = LaunchOnEnd; with the owner's local role not SimulatedProxy (every fighter of the port) BeginMove @3779.
//! - BeginMove @2754 -> @2435: a DoOnce (Has_Been_Initd / IsClosed @2395-2440, @906-921: a second call does nothing).
//!   @955 ActorDestination.OnEndPlay += TargetActorDestroyed (-> @4375 -> @15 K2_DestroyActor); Location Offset; @1231
//!   the owner's Comp_HitPause.OnHitPauseTimeChanged += OnHitPause; the owner's FighterMovementComponent (no cast, no
//!   move: @1518 pop); Over Time = FMax(0.1, OverTime) @1528; DestinationActor; CurrentDestination = location +
//!   Location Offset @1712; RootMotion_ForceMoveToPoint @1961 to it for Over Time, finish mode LaunchAtEnd ? 1
//!   SetVelocity : 2 ClampVelocity (LaunchAtEnd is never written: the class default false, so 2), EndingLaunchVector
//!   (never written: 0) and clamp 0.0, no path curve, the time curve EaseIn && EaseOut ? EaseInOutFloatCurve : EaseOut
//!   ? EaseOutFloatCurve : EaseIn ? EaseInFloatCurve : none -> RootMotionID @2252; Timeline.SetPlayRate(1 / Over Time)
//!   @2321, PlayFromStart @2362.
//! - The Timeline (length 1.0, one float track PercentRemaining, linear (0, 1) -> (1, 0)); its update
//!   (Timeline__UpdateFunc -> @4150 -> @63): DestinationActor not valid -> @891 K2_DestroyActor; else when
//!   Vector_Distance(location + Location Offset, CurrentDestination) > BufferDistance (class default 100) @248:
//!   RemoveRootMotionSourceByID(RootMotionID) @296, CurrentDestination = the actor's location @387 (the offset is not
//!   added here: the build's graph, kept), RootMotion_ForceMoveToPoint @623 to it for Over Time x PercentRemaining,
//!   finish mode 0 (MaintainLastRootMotionVelocity), the same curve pick -> RootMotionID @863. Its Finished
//!   (Timeline__FinishedFunc -> @4342): broadcast ReachedActor(DestinationActor), then @891 K2_DestroyActor.
//! - OnHitPause(HitPauseTime, AmountChanged) @4214: not being destroyed -> Timeline.Stop @2445, then
//!   Delay(HitPauseTime) @4283 -> resume @30 (latent_links) Timeline.Play @30.
//! - ReceiveEndPlay @2759: Timeline.Stop; RemoveRootMotionSourceByID(RootMotionID) @2938; the owner as a Character:
//!   LaunchCharacter(MakeVector(0, VSize(Launch on End) > 50 ? Y : 0, VSize > 50 ? Z : 100), true, true) @3344 (so a
//!   buff without a launch of its own still launches (0, 0, 100)); OnHitPauseTimeChanged unbound @3595; with authority
//!   OnKnockedBack unbound @2712.
//!
//! The native side (RootMotion_ForceMoveToPoint, its removal and its finish) is `fighter::moves` force_move_to_point /
//! remove_force_move, as [`crate::combat::dynamic_move`] uses it.
//!
//! Timing:
//! - The Timeline is a component of the buff actor (UTimelineComponent: TG_PrePhysics; FTimeline::TickTimeline:
//!   position + PlayRate x dt in float32, finished only past the length: held at it, the update, then Finished in the
//!   same tick). The buff actor's delta is the world's (its own dilation; the owner's hit pause stops the timeline
//!   through OnHitPause instead). PlayFromStart fires the update at 0 at once (SetPlaybackPosition(0) with the
//!   update), which finds the actor where it was planned.
//! - The Delay(HitPauseTime) counts the frame of its call (as `dynamic_move`'s Delay): it ends in the owner's tick pass
//!   of the call's tick + latent_frames(HitPauseTime) - 1 (at least the next); a second call while it is pending is not
//!   started again (execDelay 142f9baa0). Comp_HitPause broadcasts OnHitPauseTimeChanged with the new time at every
//!   SetHitPauseTime that is taken and with 0 at the pause's end (BuffSignal HitPauseTimeChanged).
//!
//! Deviation: the timeline advances in the buff's tick pass (as Buff_TunnelTeleport's, `chars::bugs::tunnel`), not at
//! its own place in TG_PrePhysics against the owner's CMC (the order inside the group is not fixed by code:
//! UTimelineComponent ctor 142e34c70), and from the first tick pass after BeginMove (a BeginMove made ahead of the
//! owner's buff pass in a frame has its first advance in that frame).
//! Deviation: TargetActorDestroyed (the destination's OnEndPlay) is taken at the buff's next tick pass, by the
//! destination's dead flag where it is a spawned actor or a projectile (a fighter is never destroyed in the port); the
//! build runs it inside the destination's EndPlay.
//! Deviation: EaseInFloatCurve and EaseOutFloatCurve alone are not ported (as `dynamic_move`): a move with one of them
//! warns and runs linear. The one caller passes neither.
//! Deviation: the EndPlay launch of a buff ended by the owner's knock-back (OnOwnerKnockedBack) is the port's impulse,
//! set ahead of the hit's drop of impulses (`combat::hit` receive_hit), which drops it for a hit with hitstun. In the
//! build the buff's LaunchCharacter (PendingLaunchVelocity, the later call wins) overwrites CancelJump's, is taken at
//! the next movement tick under the pause and is then replaced by the knock-back's launch at the pause's end; the port
//! keeps CancelJump's. Left: the small drift while the pause slows the fighter (under about 0.2 u a frame) and a
//! possible lift off the ground.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::actors::world_actor::actor_dead;
use crate::combat::buff_class::BuffClass;
use crate::combat::buffs::{BuffExt, BuffRef, buff, buff_mut, inst_view};
use crate::combat::dynamic_move::dest_place;
use crate::consts::DELTA_SECONDS;
use crate::fighter::FighterId;
use crate::fighter::common::{latent_frames, warn_parts};
use crate::fighter::moves::{Impulse, force_move_to_point, remove_force_move};
use crate::math::{max, to_int32, ue};
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::Value;

const CLS: &str = "Buff_MoveToActor2_C";

/// `MoveToActor2`: the buff actor's fields (its [`BuffExt::MoveToActor2`]), in sim
/// axes (x = -UE Y, y = UE Z).
#[derive(State, Debug)]
pub struct MoveToActor2 {
    /// OnKnockedBack bound (ReceiveBeginPlay: authority and DestroyOnKnockedBack).
    pub kb: bool,
    /// BufferDistance.
    pub buffer: Real,
    /// BeginMove's DoOnce closed.
    pub began: bool,
    /// DestinationActor.
    pub dest: Option<ActorRef>,
    /// Location Offset: -Y and Z.
    pub ox: Real,
    pub oy: Real,
    /// Launch on End: -Y and Z.
    pub lx: Real,
    pub ly: Real,
    /// CurrentDestination.
    pub cx: Real,
    pub cy: Real,
    /// Over Time (float32).
    pub over_time: Real,
    /// BeginMove's bEaseIn / bEaseOut (the event's pins, read again by the replacement).
    pub ease_in: bool,
    pub ease_out: bool,
    /// RootMotionID (class default 0: no source of the port has it).
    pub id: i32,
    /// OnHitPauseTimeChanged bound.
    pub hp: bool,
    /// The Timeline's position (length 1).
    pub pos: Real,
    /// Its PlayRate.
    pub rate: Real,
    pub playing: bool,
    /// The owner's tick whose tick pass ends the Delay(HitPauseTime), -1 none.
    pub hp_at: Real,
    /// The ReachedActor binding ([`move_to_actor_2_reached`]'s name), none none.
    pub reached: Option<String>,
}

impl Default for MoveToActor2 {
    /// The begin's values but for the data's (`buffs` begin).
    fn default() -> Self {
        MoveToActor2 {
            kb: true,
            buffer: 100.0,
            began: false,
            dest: None,
            ox: 0.0,
            oy: 0.0,
            lx: 0.0,
            ly: 0.0,
            cx: 0.0,
            cy: 0.0,
            over_time: 0.0,
            ease_in: false,
            ease_out: false,
            id: 0,
            hp: false,
            pos: 0.0,
            rate: 0.0,
            playing: false,
            hp_at: -1.0,
            reached: None,
        }
    }
}

/// `moveToActor2Of`: the buff's [`MoveToActor2`]; `None` for a buff of another
/// class (or one gone).
pub fn move_to_actor_2_of(w: &World, b: BuffRef) -> Option<&MoveToActor2> {
    match &buff(w, b)?.ext {
        BuffExt::MoveToActor2(e) => Some(e),
        _ => None,
    }
}

/// [`move_to_actor_2_of`], to write.
pub fn move_to_actor_2_mut(w: &mut World, b: BuffRef) -> Option<&mut MoveToActor2> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::MoveToActor2(e) => Some(e),
        _ => None,
    }
}

/// `ReachedActorFn`: a ReachedActor binder (a component's handler: Gizmo's
/// Comp_C023A_AllyAttach.ActorReachedLogic): the owner, the buff, the DestinationActor. Called at the timeline's
/// Finished, ahead of the buff's K2_DestroyActor (its EndPlay launch comes after the handler).
pub type ReachedActorFn = fn(&mut World, FighterId, BuffRef, Option<ActorRef>);

/// `moveToActor2Reached`: enters a ReachedActor binder by name (kept by name so the
/// buff's state holds a name, not a function), from its module's `register`.
pub fn move_to_actor_2_reached(r: &mut Registry, name: &'static str, f: ReachedActorFn) -> Result<(), RegistryError> {
    r.move_to_actor_2_reached.enter(name, f)
}

/// `bindReachedActor`: ReachedActor += `name` (Comp_C023A_AllyAttach @1483). One
/// binder: the build's one bind.
pub fn bind_reached_actor(w: &mut World, b: BuffRef, name: &str) {
    let r: &'static Registry = w.registry;
    if move_to_actor_2_of(w, b).is_none() {
        let c = buff(w, b).map_or_else(String::new, |x| w.names.str(x.cls).to_owned());
        return warn_parts(w, &["ReachedActor bound on ", &c, ", which is no Buff_MoveToActor2"]);
    }
    if !r.move_to_actor_2_reached.contains(name) {
        warn_parts(w, &["Buff_MoveToActor2 ReachedActor: no binder entered as ", name]);
    }
    if let Some(e) = move_to_actor_2_mut(w, b) {
        e.reached = Some(name.to_owned());
    }
}

/// A destination's actor location (`destX` / `destY`): (0, 0) for none.
fn dest_xy(w: &World, d: Option<ActorRef>) -> (Real, Real) {
    d.map_or((0.0, 0.0), |d| dest_place(w, d))
}

/// `gone`: a destination destroyed (a spawned actor's or a projectile's dead flag; a fighter never is).
fn gone(w: &World, d: Option<ActorRef>) -> bool {
    d.is_some_and(|d| actor_dead(w, d))
}

/// The time curve pick of @1739-1902 (and @414-558): only EaseInOutFloatCurve is ported.
fn eased(w: &World, ease_in: bool, ease_out: bool) -> bool {
    if ease_in != ease_out {
        let curve = if ease_out { "EaseOutFloatCurve" } else { "EaseInFloatCurve" };
        warn_parts(w, &[CLS, ": ", curve, " not ported (a linear move)"]);
    }
    ease_in && ease_out
}

/// `beginMoveToActor2`: MulticastBeginMove (@3641) -> BeginMove (@2754). `dest`:
/// ActorDestination; `offset`, `launch_on_end` in sim axes (`[-Y, Z]`). A missing destination ends the buff inside
/// BeginMove (@891).
#[allow(clippy::too_many_arguments, reason = "the graph's event's parameters")]
pub fn begin_move_to_actor_2(
    w: &mut World,
    f: FighterId,
    b: BuffRef,
    dest: Option<ActorRef>,
    ease_out: bool,
    ease_in: bool,
    over_time: Real,
    offset: [Real; 2],
    launch_on_end: [Real; 2],
) {
    let Some(e) = move_to_actor_2_mut(w, b) else {
        let c = buff(w, b).map_or_else(String::new, |x| w.names.str(x.cls).to_owned());
        return warn_parts(w, &["MulticastBeginMove on ", &c, ", which is no Buff_MoveToActor2"]);
    };
    e.lx = launch_on_end[0]; // @3641
    e.ly = launch_on_end[1];
    if e.began {
        return; // the DoOnce @906
    }
    let (dx, dy) = dest_xy(w, dest);
    let Some(e) = move_to_actor_2_mut(w, b) else { return };
    e.began = true;
    e.ox = offset[0]; // @996
    e.oy = offset[1];
    e.hp = true; // @1231
    e.over_time = max(0.1, over_time); // @1528
    e.dest = dest; // @1597
    e.cx = dx + e.ox; // @1712
    e.cy = dy + e.oy;
    e.ease_in = ease_in;
    e.ease_out = ease_out;
    let (cx, cy, t) = (e.cx, e.cy, e.over_time);
    let ease = eased(w, ease_in, ease_out);
    let id = force_move_to_point(w, f, cx, cy, t, 2.0, 0.0, 0.0, 0.0, ease); // @1961 -> @2252
    let Some(e) = move_to_actor_2_mut(w, b) else { return };
    e.id = to_int32(id);
    e.rate = 1.0 / e.over_time; // @2321
    e.pos = 0.0; // PlayFromStart @2362: the update at 0
    e.playing = true;
    if timeline_update(w, f, b) {
        crate::combat::buffs::end_buff(w, f, b);
    }
}

/// Timeline__UpdateFunc (@4150 -> @63). True: K2_DestroyActor (@891).
fn timeline_update(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(e) = move_to_actor_2_of(w, b) else { return false };
    let Some(d) = e.dest.filter(|&d| !actor_dead(w, d)) else { return true }; // IsValid(DestinationActor) @92
    let (px, py) = dest_place(w, d);
    let (dx, dy) = ((px + e.ox), (py + e.oy));
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated test is kept for its NaN result")]
    if !(ue::size2d([dx - e.cx, dy - e.cy]) > e.buffer) {
        return false; // @248
    }
    let (id, ease_in, ease_out) = (e.id, e.ease_in, e.ease_out);
    remove_force_move(w, f, id); // @296
    let ease = eased(w, ease_in, ease_out);
    let Some(e) = move_to_actor_2_mut(w, b) else { return false };
    e.cx = px; // @387: the location alone
    e.cy = py;
    let pct = 1.0 - e.pos; // PercentRemaining: the linear key pair (0, 1) -> (1, 0)
    let (cx, cy, t) = (e.cx, e.cy, (e.over_time * pct));
    let id = force_move_to_point(w, f, cx, cy, t, 0.0, 0.0, 0.0, 0.0, ease); // @623 -> @863
    if let Some(e) = move_to_actor_2_mut(w, b) {
        e.id = to_int32(id);
    }
    false
}

const HOOKS: BuffHooks = BuffHooks {
    begin: Some(begin),
    tick: Some(tick),
    on: Some(on),
    end: Some(end),
    timer: None,
    take_any_damage: None,
    branch_group_blocked: None,
    branch_group_end_blocked: None,
    actor_collision_off: None,
    max_stacks: None,
    stacks: None,
    cleanse: None,
    status: None,
    dealt_cond: None,
    taken_cond: None,
    attach_parent: None,
};

fn begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    let d = inst_view(w, b);
    let kb = d.get("DestroyOnKnockedBack").and_then(Value::as_bool).unwrap_or(true); // @3879 (class default true)
    let buffer = d.num("BufferDistance").unwrap_or(100.0); // class default 100
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::MoveToActor2(MoveToActor2 { kb, buffer, ..MoveToActor2::default() });
    }
}

fn tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(e) = move_to_actor_2_of(w, b) else { return false };
    if gone(w, e.dest) {
        return true; // TargetActorDestroyed @4375 (the head comment)
    }
    // The Timeline's component tick.
    if e.playing {
        let np = e.pos + (e.rate * DELTA_SECONDS);
        let done = np > 1.0;
        if let Some(e) = move_to_actor_2_mut(w, b) {
            e.pos = if done { 1.0 } else { np };
        }
        if timeline_update(w, f, b) {
            return true;
        }
        if done {
            // Finished @4342: broadcast ReachedActor(DestinationActor), then @4370 -> @891 K2_DestroyActor.
            let Some(e) = move_to_actor_2_mut(w, b) else { return true };
            e.playing = false;
            let (dest, reached) = (e.dest, e.reached.clone());
            let r: &'static Registry = w.registry;
            if let Some(h) = reached.and_then(|n| r.move_to_actor_2_reached.get(&n).copied()) {
                h(w, f, b, dest);
            }
            return true;
        }
    }
    // The Delay(HitPauseTime) -> @30 Timeline.Play.
    let now = crate::real(w.fighter(f).clock.tick);
    if let Some(e) = move_to_actor_2_mut(w, b)
        && e.hp_at >= 0.0
        && now >= e.hp_at
    {
        e.hp_at = -1.0;
        e.playing = true;
    }
    false
}

fn on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let now = crate::real(w.fighter(f).clock.tick);
    let Some(e) = move_to_actor_2_mut(w, b) else { return false };
    if s == BuffSignal::KnockedBack {
        return e.kb; // OnOwnerKnockedBack @4135
    }
    if s == BuffSignal::HitPauseTimeChanged && e.hp {
        // OnHitPause @4214: Timeline.Stop, Delay(HitPauseTime).
        e.playing = false;
        if e.hp_at < 0.0 {
            e.hp_at = now + max(latent_frames(a.duration.unwrap_or(0.0)) - 1.0, 1.0);
        }
    }
    false
}

/// ReceiveEndPlay @2759.
fn end(w: &mut World, f: FighterId, b: BuffRef) {
    let Some(e) = move_to_actor_2_mut(w, b) else { return };
    e.playing = false; // Timeline.Stop
    e.hp_at = -1.0;
    let (id, lx, ly) = (e.id, e.lx, e.ly);
    remove_force_move(w, f, id); // @2938
    let big = ue::size2d([lx, ly]) > 50.0; // @3157
    w.fighter_mut(f).moves.impulse = Some(Impulse {
        vx: if big { lx } else { 0.0 },
        vy: if big { ly } else { 100.0 },
        xy: true,
        z: true,
        kind: 0,
        d: None,
    }); // LaunchCharacter @3344
    if let Some(e) = move_to_actor_2_mut(w, b) {
        e.hp = false; // @3595
        e.kb = false; // @2712
    }
}

pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(CLS, BuffEntry::new(BuffClass::BuffMoveToActor2, HOOKS))
}
