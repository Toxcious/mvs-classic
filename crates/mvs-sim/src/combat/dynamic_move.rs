//! Buff_DynamicMoveToActor and its data-only child Buff_StunnedDynamicMoveToActor (Panda_Main/Blueprints/Abilities/
//! Shield): a force move toward an actor that may move, planned again every
//! 0.1 s. Marvin's swap (Buff_C018WarpFighterHit -> Buff_C018ProjectileWarpEnemy, a child of
//! Buff_C018ProjectileWarpMove, a child of the Stunned class) is the build's one caller: its classes enter through
//! [`dynamic_move_buffs`] with their overrides ([`DynamicMoveClass`]) and keep their own fields in
//! [`DynamicMove::child`]. Offsets are the class's ubergraph's unless named.
//! - BeginMoveToActor(Actor, Time, EaseIn, EaseOut, LaunchAtEnd, EndingLaunchVelocity) @281-440 (the multicast
//!   MulticastBeginMoveToActor @739 calls it): DestinationActor, Duration, EaseIn, EaseOut, LaunchAtEnd, Ending Launch
//!   Velocity, StartTime = GetGameTimeInSeconds; then @15 HandleMoveOnTimer() at once (the class's override, a
//!   child's when it has one), @52 K2_SetTimerDelegate(HandleMoveOnTimer, 0.1, looping) -> UpdateTimer, and with
//!   authority @162 Delay(Duration) -> @221 K2_DestroyActor.
//! - HandleMoveOnTimer (@808): HandleMove(DestinationActor location); HandleMove ignores its argument.
//! - HandleMove (function @0-1214), with a FighterMovementComponent on the owner: RootMotionID >= -1 (always true: the
//!   ids are 1..65535 and -1 is 0xFFFF, which matches id 65535 natively; the port's per-fighter ids reach 65535 only
//!   after 65535 sources, so -1 matches none here) -> RootMotion_RemoveRootMotionSourceByID(RootMotionID) @233; then
//!   RootMotion_ForceMoveToPoint @923 to MakeVector(0, Y, Z) of Offset + DestinationActor location, for
//!   FMax(Duration - (now - StartTime), 0), finish mode LaunchAtEnd ? 1 SetVelocity (Ending Launch Velocity) : 2 ClampVelocity
//!   (clamp 0.0), no path curve, the time curve EaseIn && EaseOut ? EaseInOutFloatCurve : EaseOut ?
//!   EaseOutFloatCurve : EaseIn ? EaseInFloatCurve : none, AccumulateMode false (Override) -> RootMotionID @1214 (0
//!   when the time is 0: no source).
//! - OwnerKnockedBack @734 -> @236: with authority K2_DestroyActor.
//! - ReceiveEndPlay @472: K2_ClearAndInvalidateTimerHandle(UpdateTimer) @491, then with a FighterMovementComponent on
//!   the owner RemoveRootMotionSourceByID(RootMotionID) @692.
//! - Class defaults: RootMotionID -1, Offset (0, 0, 0), Ending Launch Velocity (0, 0, 0); StateInfo group 19
//!   NewEnumerator13. The Stunned child adds Stun.StunDuration 10 and has no graph: its stun is the data's
//!   (`combat::holds` apply_stun_buffs), its behaviour the parent's.
//!
//! The native side: the removal is applied in the next PerformMovement's CleanUpInvalidRootMotion (the removed
//! source's FinishVelocity written), then PrepareRootMotion makes the new source active and it drives that same update
//! from its time 0 and the current location. The port's one-source model (`fighter::moves::command`
//! force_move_to_point / remove_force_move / the clean-up) does just that: the removal stores the finish, the next CMC
//! tick applies it and then runs the new source. So the mid-move finish is seen only when no new source follows (the
//! remaining time 0).
//!
//! Timing:
//! - The looping timer is set inside BeginMoveToActor (the tick groups, or a timer callback): it starts at that
//!   frame's timer pass (or the end of it) without counting it, and call n comes at the first later timer pass whose
//!   counted deltas exceed n x 0.1 (ExpireTime += Rate: UWorld::Tick's order, FTimerManager::Tick). Counted by the
//!   owner's tick at the set: the buff's `timer` hook, the World's class timers.
//! - The Delay counts the frame of its call (as Buff_JumpIncrease's, `combat::buffs`): the buff goes in the owner's
//!   buff tick of the call's tick + latent_frames(Duration) - 1 (at least the next).
//! - StartTime and the timer's GetGameTimeInSeconds are the owner's float32 clock (`Fighter.clock.time`).
//!
//! Deviation: EaseInFloatCurve and EaseOutFloatCurve alone are not ported (the one live caller passes both eases:
//! EaseInOutFloatCurve): a move with only one of them warns and runs linear.
//! Unverified: a second BeginMoveToActor on the same buff (no caller in the build) keeps the first Delay (a Delay node
//! with a pending action is not started again: stock FLatentActionManager, not read) and sets the timer again.
//! Unverified: a DestinationActor that is gone is read at its last place (the build's
//! K2_GetActorLocation on a pending-kill actor reads its last transform, an invalid one (0, 0, 0)). Here a spawned
//! actor gone from its arena reads (0, 0) (`actor_place`); the build's callers move to a fighter only.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::actors::world_actor::actor_place;
use crate::combat::buff_class::BuffClass;
use crate::combat::buffs::{BuffExt, BuffRef, buff, buff_mut};
use crate::fighter::FighterId;
use crate::fighter::common::{latent_frames, warn_parts};
use crate::fighter::moves::{force_move_to_point, remove_force_move};
use crate::math::{max, to_int32};
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// `DynamicMove`: the buff actor's fields (its [`BuffExt::DynamicMove`]), in sim axes
/// (x = -UE Y, y = UE Z).
#[derive(State, Debug)]
pub struct DynamicMove {
    /// DestinationActor.
    pub dest: Option<ActorRef>,
    /// Duration (float32).
    pub dur: Real,
    pub ease_in: bool,
    pub ease_out: bool,
    /// LaunchAtEnd.
    pub launch: bool,
    /// Ending Launch Velocity: -Y and Z.
    pub lvx: Real,
    pub lvy: Real,
    /// StartTime (the owner's float32 clock).
    pub start: Real,
    /// RootMotionID (-1: the class default).
    pub id: i32,
    /// Offset: -Y and Z (a child may set it; the class default is 0).
    pub ox: Real,
    pub oy: Real,
    /// The owner's tick at the UpdateTimer's set, -1 none (cleared).
    pub arm_at: Real,
    /// The looping timer's calls since that set.
    pub calls: Real,
    /// The owner's tick whose buff tick ends the Delay(Duration), -1 none.
    pub end_at: Real,
    /// A child class's own fields (a subclass of the extension).
    pub child: DynamicMoveChild,
}

impl Default for DynamicMove {
    /// The begin's values (`dynamicMoveBuffs` begin).
    fn default() -> Self {
        DynamicMove {
            dest: None,
            dur: 0.0,
            ease_in: false,
            ease_out: false,
            launch: false,
            lvx: 0.0,
            lvy: 0.0,
            start: 0.0,
            id: -1,
            ox: 0.0,
            oy: 0.0,
            arm_at: -1.0,
            calls: 0.0,
            end_at: -1.0,
            child: DynamicMoveChild::None,
        }
    }
}

/// The fields a child class of the chain adds beside [`DynamicMove`]'s, a variant per class that has some (Marvin's
/// Buff_C018ProjectileWarpMove and WarpEnemy add theirs: `chars::marvin::swap`).
#[derive(State, Debug, Default)]
pub enum DynamicMoveChild {
    #[default]
    None,
    /// Buff_C018ProjectileWarpMove_C and Buff_C018ProjectileWarpEnemy_C.
    C018Warp(crate::chars::marvin::swap::WarpExt),
}

/// `DynamicMoveClass`: a class of the chain with its own overrides. The parent's parts
/// run first, then the class's own (a child's ReceiveEndPlay calls the parent's first: Buff_C018ProjectileWarpMove
/// @15).
#[allow(clippy::type_complexity, reason = "the buff hooks' signatures, as `BuffHooks` has them")]
#[derive(Clone, Copy, Debug, Default)]
pub struct DynamicMoveClass {
    /// Its HandleMoveOnTimer override (Buff_C018ProjectileWarpMove's @898: its own distance test, then
    /// [`handle_move`] or K2_DestroyActor), in place of the parent's HandleMove(DestinationActor location). True:
    /// K2_DestroyActor.
    pub handle_move_on_timer: Option<fn(&mut World, FighterId, BuffRef) -> bool>,
    /// Its ReceiveBeginPlay (the parent has none of its own), after the extension is made: the owner, the buff, the
    /// Instigator, the duration.
    pub begin: Option<fn(&mut World, FighterId, BuffRef, Option<FighterId>, Option<Real>)>,
    /// Its ReceiveEndPlay, after the parent's.
    pub end: Option<fn(&mut World, FighterId, BuffRef)>,
    /// Its own bound delegates (the parent's OwnerKnockedBack comes first).
    pub on: Option<fn(&mut World, FighterId, BuffRef, BuffSignal, &BuffSignalArg) -> bool>,
    /// Its own latent actions, after the parent's Delay.
    pub tick: Option<fn(&mut World, FighterId, BuffRef) -> bool>,
}

/// K2_SetTimerDelegate(HandleMoveOnTimer, 0.1, true) @52.
const RATE: Real = 0.1;

const PARENT: &str = "Buff_DynamicMoveToActor_C";
const STUNNED: &str = "Buff_StunnedDynamicMoveToActor_C";

/// `dynamicMoveOf`: the buff's [`DynamicMove`]; `None` for a buff of another class
/// (or one gone).
pub fn dynamic_move_of(w: &World, b: BuffRef) -> Option<&DynamicMove> {
    match &buff(w, b)?.ext {
        BuffExt::DynamicMove(e) => Some(e),
        _ => None,
    }
}

/// [`dynamic_move_of`], to write.
pub fn dynamic_move_mut(w: &mut World, b: BuffRef) -> Option<&mut DynamicMove> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::DynamicMove(e) => Some(e),
        _ => None,
    }
}

/// The class's overrides, by the buff's registered class.
fn class_of(w: &World, b: BuffRef) -> DynamicMoveClass {
    let r: &'static Registry = w.registry;
    buff(w, b).and_then(|x| x.class.registered_name()).and_then(|n| r.dynamic_moves.get(n)).copied().unwrap_or_default()
}

/// `dynamicMoveBuffs`: enters each class of the chain as a buff with the parent's hooks
/// and its overrides.
pub fn dynamic_move_buffs(
    r: &mut Registry,
    defs: &[(&'static str, BuffClass, DynamicMoveClass)],
) -> Result<(), RegistryError> {
    for &(cls, class, c) in defs {
        r.buffs.enter(cls, BuffEntry::new(class, HOOKS))?;
        r.dynamic_moves.enter(cls, c)?;
    }
    Ok(())
}

const HOOKS: BuffHooks = BuffHooks {
    begin: Some(begin),
    tick: Some(tick),
    timer: Some(timer),
    on: Some(on),
    end: Some(end),
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

fn begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, dur: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::DynamicMove(DynamicMove::default());
    }
    if let Some(h) = class_of(w, b).begin {
        h(w, f, b, from, dur);
    }
}

/// The Delay(Duration) @162 -> @221 K2_DestroyActor, then the class's own.
fn tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let now = crate::real(w.fighter(f).clock.tick);
    if dynamic_move_of(w, b).is_some_and(|e| e.end_at >= 0.0 && now >= e.end_at) {
        return true;
    }
    class_of(w, b).tick.is_some_and(|h| h(w, f, b))
}

/// The looping UpdateTimer -> HandleMoveOnTimer.
fn timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let now = crate::real(w.fighter(f).clock.tick);
    let Some(e) = dynamic_move_mut(w, b) else { return false };
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated test is kept for its NaN result")]
    if e.arm_at < 0.0 || !(crate::consts::clock(now - e.arm_at) > f64::from(e.calls + 1.0) * f64::from(RATE)) {
        return false;
    }
    e.calls += 1.0;
    handle_move_on_timer(w, f, b)
}

/// OwnerKnockedBack @734 -> @266 K2_DestroyActor (authority), else the class's own.
fn on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    s == BuffSignal::KnockedBack || class_of(w, b).on.is_some_and(|h| h(w, f, b, s, a))
}

/// ReceiveEndPlay @472: the timer cleared @491, the source removed by its id @692; then the class's own.
fn end(w: &mut World, f: FighterId, b: BuffRef) {
    if let Some(e) = dynamic_move_mut(w, b) {
        e.arm_at = -1.0;
        let id = e.id;
        remove_force_move(w, f, id);
    }
    if let Some(h) = class_of(w, b).end {
        h(w, f, b);
    }
}

/// `beginMoveToActor`: BeginMoveToActor (@281-467, then @15-162). `dest`:
/// DestinationActor; `seconds`: Time; `vel`: Ending Launch Velocity in sim axes (`[-Y, Z]`). True: the class's
/// HandleMoveOnTimer destroyed the buff in the first call (the caller ends it).
#[allow(clippy::too_many_arguments, reason = "the graph's call's parameters")]
pub fn begin_move_to_actor(
    w: &mut World,
    f: FighterId,
    b: BuffRef,
    dest: Option<ActorRef>,
    seconds: Real,
    ease_in: bool,
    ease_out: bool,
    launch_at_end: bool,
    vel: [Real; 2],
) -> bool {
    let (now, time) = (crate::real(w.fighter(f).clock.tick), w.fighter(f).clock.time);
    let Some(e) = dynamic_move_mut(w, b) else {
        let c = buff(w, b).map_or_else(String::new, |x| w.names.str(x.cls).to_owned());
        warn_parts(w, &["BeginMoveToActor on ", &c, ", which is no Buff_DynamicMoveToActor"]);
        return false;
    };
    e.dest = dest;
    e.dur = seconds;
    e.ease_in = ease_in;
    e.ease_out = ease_out;
    e.launch = launch_at_end;
    e.lvx = vel[0];
    e.lvy = vel[1];
    e.start = time;
    if handle_move_on_timer(w, f, b) {
        return true; // @15
    }
    let Some(e) = dynamic_move_mut(w, b) else { return false };
    e.arm_at = now; // @52
    e.calls = 0.0;
    if e.end_at < 0.0 {
        e.end_at = now + max(latent_frames(e.dur) - 1.0, 1.0); // @162
    }
    false
}

/// HandleMoveOnTimer: the class's override, or the parent's HandleMove(DestinationActor location).
fn handle_move_on_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    if let Some(h) = class_of(w, b).handle_move_on_timer {
        return h(w, f, b);
    }
    handle_move(w, f, b);
    false
}

/// A DestinationActor's actor location: a fighter's (x, actor Y), a spawned actor's root, a stage actor's (x, y).
pub(crate) fn dest_place(w: &World, d: ActorRef) -> (Real, Real) {
    match d {
        ActorRef::Fighter(id) => (w.fighter(id).body.x, crate::fighter::actor_y(w, id)),
        ActorRef::Stage(i) => {
            let a = &w.stage_run.as_ref().expect("a stage actor with the stage's run").actors[i as usize];
            (a.x, a.y)
        }
        _ => match actor_place(w, d) {
            Some((x, y, _)) => (x, y),
            None => crate::unported!("a buff actor as a move destination", d),
        },
    }
}

/// `handleMove`: the parent's HandleMove (a child's override calls it as its Super:
/// Buff_C018ProjectileWarpMove HandleMove @0).
pub fn handle_move(w: &mut World, f: FighterId, b: BuffRef) {
    let Some(e) = dynamic_move_of(w, b) else { return };
    let id = e.id;
    remove_force_move(w, f, id); // @233
    let time = w.fighter(f).clock.time;
    let Some(e) = dynamic_move_of(w, b) else { return };
    let (dx, dy) = e.dest.map_or((0.0, 0.0), |d| dest_place(w, d));
    let elapsed = time - e.start;
    let left = max(e.dur - elapsed, 0.0);
    let (tx, ty) = ((e.ox + dx), (e.oy + dy));
    let both = e.ease_in && e.ease_out;
    let (launch, lvx, lvy) = (e.launch, e.lvx, e.lvy);
    if !both && (e.ease_in || e.ease_out) {
        let curve = if e.ease_out { "EaseOutFloatCurve" } else { "EaseInFloatCurve" };
        let c = buff(w, b).map_or_else(String::new, |x| w.names.str(x.cls).to_owned());
        warn_parts(w, &[&c, ": ", curve, " not ported (a linear move)"]);
    }
    // @923 -> @1214
    let id = if launch {
        force_move_to_point(w, f, tx, ty, left, 1.0, lvx, lvy, 0.0, both)
    } else {
        force_move_to_point(w, f, tx, ty, left, 2.0, 0.0, 0.0, 0.0, both)
    };
    if let Some(e) = dynamic_move_mut(w, b) {
        e.id = to_int32(id);
    }
}

/// The two classes of the chain this module holds.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    dynamic_move_buffs(
        r,
        &[
            (PARENT, BuffClass::BuffDynamicMoveToActor, DynamicMoveClass::default()),
            (STUNNED, BuffClass::BuffStunnedDynamicMoveToActor, DynamicMoveClass::default()),
        ],
    )
}
