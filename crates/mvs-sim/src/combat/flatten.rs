//! Buff_Flatten (Blueprints/Characters/Velma/Buff_Flatten, offsets of its ubergraph): shared
//! code. LeBron's C016_Special_Dunk_DownSlam_Montage and C016_GroundAimSlam_Montage hit boxes give it (OnHitBuffs,
//! BuffDuration 2) as Velma's VelmaCarryRun_Actor does (On Hit Buffs, FlattenBuff). The class data: BuffDuration 5,
//! Stun 5 s (CanStruggle, StunnedHitReaction 6), IsDebuff.
//!
//! - ReceiveBeginPlay @416 (the parent's first): authority, the owner a BaseFighter (BaseFighter = it @559):
//!   LineTraceSingle(the owner's location, that location - (0, 0, 1000), channel 3 ProjectileGroundTrace, no actor
//!   to ignore, bIgnoreSelf) @791, then the owner's K2_SetActorLocation(hit ? its Location : the Instigator's
//!   location, no sweep, no teleport) @1235. Then the owner's DamageManager.KnockedBack -> KnockedBack @1391 and its
//!   HitstunHandler.BuffStunEnded -> BreakOut @1526 are bound, and FlattenVisualMulticast(BaseFighter) -> @191: the
//!   owner's Comp_HitPause.SetHitPauseTime(5.0) @262 and BaseFighter.OnIsBouncing(3, 0, 0) @303.
//! - KnockedBack @158 and BreakOut @10: K2_DestroyActor.
//! - ReceiveEndPlay @369 (the parent's first) -> FlattenVisualStopMulticast -> @29: SetHitPauseTime(0.0) @29
//!   (Comp_HitPause makes it a pause of 0.01 s) and OnIsBouncing(0, 0, 0) @92.
//!
//! OnIsBouncing is BP_FighterCharacter's delegate: Base_AnimBP's OnIsBouncingChanged (its IsBouncing, the Bounce
//! state's pose: `HitState.anim_bounce`; BouncePitchAngleDegrees = the second value @27; KnockBackAngle =
//! CalculateDirection of the third, 0 for 0, @154-441) and the buffs bound to it (`BuffSignal::IsBouncing`).
//! BaseFighter's own IsBouncing (`HitState.bouncing`) is not written: the hit pause end (BFHitPauseEnd @143454)
//! broadcasts 0 again (`hit_pause_end`).
//! Unverified: the K2_SetActorLocation without a sweep at the trace's floor point leaves the capsule half in the
//! floor; the movement's next move pushes it out (stock ResolvePenetration), done at once by `teleport_move` against
//! the walking boxes it lists.

use crate::Real;
use crate::actors::ActorRef;
use crate::combat::buffs::buff_signal;
use crate::combat::hit::knockback::timer_count;
use crate::combat::hit::set_hit_pause;
use crate::combat::{BuffClass, BuffRef};
use crate::fighter::anim::update::update_pose;
use crate::fighter::moves::teleport_move;
use crate::fighter::traces::{TRACE_PROJECTILE_GROUND, sphere_trace_single};
use crate::fighter::{FighterId, actor_y};
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// Comp_HitPause SetHitPauseTime(t): a pause of t, or of 0.01 s for t <= 0.
fn hit_pause_time(w: &mut World, f: FighterId, sec: Real) {
    let s = if sec > 0.0 { sec } else { 0.01 };
    set_hit_pause(w, f, crate::real(timer_count(s)), s);
}

/// OnIsBouncing(bounce, 0, 0) @303 / @92, Base_AnimBP's binding: its IsBouncing, BouncePitchAngleDegrees and
/// KnockBackAngle (CalculateDirection of a zero vector: 0).
fn anim_is_bouncing(w: &mut World, f: FighterId, bounce: Real) {
    let x = w.fighter_mut(f);
    x.hit.anim_bounce = Some(bounce);
    x.react.bounce_pitch = 0.0;
    x.react.knock_back_angle = 0.0;
}

/// The same broadcast's buff binders.
fn buffs_is_bouncing(w: &mut World, f: FighterId, bounce: Real) {
    if !w.fighter(f).buffs.buffs.is_empty() {
        let arg = BuffSignalArg { bounce: Some(bounce), ..BuffSignalArg::default() };
        buff_signal(w, f, BuffSignal::IsBouncing, arg);
    }
}

fn flatten_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _: Option<Real>) {
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
    let h = sphere_trace_single(w, x, y, x, y - 1000.0, 0.0, TRACE_PROJECTILE_GROUND, &[], Some(ActorRef::Buff(b)));
    match (h, from) {
        (Some(h), _) => teleport_move(w, f, h.x, h.y),
        (None, Some(i)) => {
            let (ix, iy) = (w.fighter(i).body.x, actor_y(w, i));
            teleport_move(w, f, ix, iy);
        }
        // No Instigator: GetActorLocation(None) is (0, 0, 0).
        (None, None) => teleport_move(w, f, 0.0, 0.0),
    }
    hit_pause_time(w, f, 5.0); // @262
    // The mesh does not run the pose machine in a hit pause: the hit's own update_pose has run before this row, so
    // the Bounce state is entered here, as at a hit.
    // Unverified: the build's first dilated AnimBP update taking the transition is not read natively.
    anim_is_bouncing(w, f, 3.0); // @303
    update_pose(w, f);
    buffs_is_bouncing(w, f, 3.0);
}

fn flatten_on(_: &mut World, _: FighterId, _: BuffRef, s: BuffSignal, _: &BuffSignalArg) -> bool {
    s == BuffSignal::KnockedBack || s == BuffSignal::BuffStunEnded
}

fn flatten_end(w: &mut World, f: FighterId, _: BuffRef) {
    hit_pause_time(w, f, 0.0); // @29
    anim_is_bouncing(w, f, 0.0); // @92
    buffs_is_bouncing(w, f, 0.0);
}

pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        "Buff_Flatten_C",
        BuffEntry::new(
            BuffClass::BuffFlatten,
            BuffHooks {
                begin: Some(flatten_begin),
                on: Some(flatten_on),
                end: Some(flatten_end),
                ..BuffHooks::default()
            },
        ),
    )
}
