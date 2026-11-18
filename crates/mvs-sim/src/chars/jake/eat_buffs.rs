//! The swallow's buffs: Buff_JakEat (a Buff_HeldByEnemy child: its hold kind
//! [`KIND`] on `combat::holds`' grapple hold, its own variables [`JakEatOwn`] on `Hold.own`, and a `buffs` entry) and
//! Buff_JakEatSlow (BPI_SetBlockTaunt on and off). Comp_JakeEat's side is eat.rs.
//! Not ported: EndOfMatch (the instigator's OnBFDefeated @4778: no match end mid-hold in the port's
//! flow;
//! Deviation), DestroyWithInstigator, the IsPickedUp bindings, LockLocation, the EndPlay's SetFacingDirection (@1254: the
//! view draws the yaw from the facing already).

use crate::Real;
use crate::State;
use crate::combat::buffs::{buff_of, set_buff_values};
use crate::combat::holds::{Hold, HoldKind, HoldOwn, buff_stun_end, consume_all_attached, consume_attached};
use crate::combat::{BuffClass, BuffRef};
use crate::fighter::anim::{add_invisibility, remove_invisibility};
use crate::fighter::control::{bpi_set_block_taunt, trigger_ability};
use crate::fighter::moves::Impulse;
use crate::fighter::{FighterId, hold_def, latent};
use crate::math::max;
use crate::name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::props::{num, obj};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

use super::eat::{JAK_EAT, JAK_EAT_SLOW, add_held_fighters, remove_held_fighter};

/// Buff_JakEat_C's class name (`hold_def`'s key).
const EAT: crate::names::Name = name!("Buff_JakEat_C");

/// Buff_JakEat's own variables on its hold (`Hold.own`, `combat::holds::HoldOwn::JakEat`).
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub struct JakEatOwn {
    /// The attach check's Temp_int_Variable (`tries`).
    pub tries: i32,
    /// Its retry Delay(0.033), in frames left (`wait`).
    pub wait: i32,
    /// The Delay(0.066) ahead of GateOpen, in frames left (`gateWait`).
    pub gate_wait: i32,
    /// GateOpen made (`gate`).
    pub gate: bool,
    /// SetCollision(owner, false) made (`collOff`): the buff entry's actor_collision_off.
    pub coll_off: bool,
    /// DeathSave (`deathSave`).
    pub death_save: bool,
}

/// The hold's own variables of the swallowed fighter, if it is a Buff_JakEat's.
fn own_mut(w: &mut World, vic: FighterId) -> Option<&mut JakEatOwn> {
    match &mut w.fighter_mut(vic).holds.hold.as_mut()?.own {
        HoldOwn::JakEat(o) => Some(o),
        _ => None,
    }
}

/// A count of ticks from a latent Delay: `Math.max(latent(s) - 1, 1)`.
fn delay_frames(w: &World, vic: FighterId, s: Real) -> i32 {
    #[allow(clippy::cast_possible_truncation, reason = "a count of ticks")]
    let n = max(latent(w, vic, s) - 1.0, 1.0) as i32;
    n
}

/// The attach check @90 (authority, @1423): the owner's attach parent the instigator (the parent's Delay(0.033) attach
/// done: `h.attached`, its InitiateGrapple) -> SetCollision(owner, false) @236 (`coll_off`: the entry's
/// actor_collision_off below), VisibilityReplication(true) (the owner's AddInvisibility), the instigator's Comp_JakeEat
/// AddHeldFighers(owner) @397, DestroyAttachedConsumableActors(owner) @537. Else the DoOnce sets Temp_int_Variable 0, and
/// while it is below 5: + 1 and Delay(0.033) @699 -> @27: 4 or more -> K2_DestroyActor, else the check again.
// Unverified: GetAttachParentActor is taken as the parent's attach.
fn attach_check(w: &mut World, vic: FighterId) {
    let Some(h) = &w.fighter(vic).holds.hold else { return };
    let (attached, by) = (h.attached, h.by);
    if attached {
        if let Some(o) = own_mut(w, vic) {
            o.coll_off = true;
        }
        add_invisibility(w, vic);
        add_held_fighters(w, by, vic);
        if let Some(h) = &mut w.fighter_mut(vic).holds.hold {
            consume_attached(h);
        }
        return;
    }
    let tries = own_mut(w, vic).map_or(5, |o| o.tries);
    if tries < 5 {
        // The retry Delay of the re-check is `latent(0.033)` whole (the first, in BeginPlay, is one less).
        #[allow(clippy::cast_possible_truncation, reason = "a count of ticks")]
        let wait = latent(w, vic, 0.033) as i32;
        if let Some(o) = own_mut(w, vic) {
            o.tries = tries + 1;
            o.wait = wait;
        }
    }
}

/// ReceiveBeginPlay after the parent's (@3107): the sequence's pins in order. @1423 the attach check; the first retry
/// Delay is made in BeginPlay as the parent's attach Delay is, so both end in the same frame (Unverified, the parent's
/// first, `combat::holds::hold_tick`). @4274 Delay(0.066) -> @15 GateOpen. @3818 the instigator's team the owner's ->
/// Stun.CanStruggle false @3954, the owner's UpdatePlayerStatus(null) @3974, CurrentJumpNumber 0 @4065. @3137 the owner's
/// attached actors that implement BPI_SpawnedActor destroyed. @4329 the instigator's Comp_Pawn_AbilityManager
/// TriggerAbilityActual(Eat Ability Index, 1).
// Unverified: the stun pick of a buff not yet past its spawn's own status update; the port counts the live entry.
fn begin(w: &mut World, vic: FighterId) {
    let Some(h) = &w.fighter(vic).holds.hold else { return };
    let (attached, by) = (h.attached, h.by);
    let mut o = JakEatOwn::default();
    if attached {
        w.fighter_mut(vic).holds.hold.as_mut().expect("hold").own = HoldOwn::JakEat(o);
        attach_check(w, vic);
        o = *own_mut(w, vic).expect("own");
    } else {
        o.tries = 1;
        o.wait = delay_frames(w, vic, 0.033);
    }
    o.gate_wait = delay_frames(w, vic, 0.066);
    w.fighter_mut(vic).holds.hold.as_mut().expect("hold").own = HoldOwn::JakEat(o);
    if w.fighter(by).team.team == w.fighter(vic).team.team {
        if let Some(b) = buff_of(w, vic, EAT) {
            let mut stun = obj(hold_def(w, vic, EAT), "Stun").cloned().unwrap_or_default();
            stun.insert("CanStruggle".to_owned(), Value::Bool(false));
            set_buff_values(w, b, &Props::from_iter([("Stun".to_owned(), Value::Obj(stun))]));
        }
        crate::combat::holds::apply_stun_buffs(w, vic, None, false);
        w.fighter_mut(vic).jump.jumps = 0;
    }
    if let Some(h) = &mut w.fighter_mut(vic).holds.hold {
        consume_all_attached(h);
    }
    #[allow(clippy::cast_possible_truncation, reason = "a small slot index")]
    let slot = num(hold_def(w, vic, EAT), "Eat Ability Index").map_or(1, |v| v as i32);
    trigger_ability(w, by, slot);
}

/// The child's Delays after the parent's in the pass: the retry @699 -> @27, then the gate @4274 -> @15. True:
/// K2_DestroyActor.
fn latent_tick(w: &mut World, vic: FighterId) -> bool {
    let Some(o) = own_mut(w, vic) else { return false };
    if o.wait > 0 {
        o.wait -= 1;
        if o.wait == 0 {
            if o.tries >= 4 {
                return true;
            }
            attach_check(w, vic);
        }
    }
    let Some(o) = own_mut(w, vic) else { return false };
    if o.gate_wait > 0 {
        o.gate_wait -= 1;
        if o.gate_wait == 0 {
            o.gate = true;
        }
    }
    false
}

/// ReceiveEndPlay @829 after the parent's: SetCollision(owner, true) (the entry is gone), the instigator's
/// Comp_JakeEat RemoveHeldFighter(owner) @964 (whether it was added or not), VisibilityReplication(false) @1094 (the
/// owner's RemoveInvisibility).
fn end(w: &mut World, vic: FighterId, h: &Hold) {
    remove_held_fighter(w, h.by, vic);
    remove_invisibility(w, vic);
}

/// JakeDeath @1458 (bound to the owner's and the instigator's OnExitedArena @2378 / @2442, and to the instigator's
/// OnExitArenaWarning @4736): RemoveGrapple, LockLocation false, SetMovementMode(Falling) (the hold's end), DeathSave,
/// the owner's HitstunBuffActive -> RemoveHitStunFromBuff @1842 (`buff_stun_end`: its BuffStunEnded may end the hold
/// already), then on authority LaunchCharacter((0, Y >= 0 ? -500 : 500, 2000), true, true) @2174 and K2_DestroyActor
/// @2221. Sim x = -UE Y.
fn jake_death(w: &mut World, vic: FighterId) -> bool {
    if let Some(o) = own_mut(w, vic) {
        o.death_save = true;
    }
    if w.fighter(vic).buffs.buff_stunned {
        buff_stun_end(w, vic, false);
    }
    let vx = if w.fighter(vic).body.x <= 0.0 { 500.0 } else { -500.0 };
    w.fighter_mut(vic).moves.impulse = Some(Impulse { vx, vy: 2000.0, xy: true, kind: 0, z: true, d: None });
    true
}

/// JakeDeath @1458 bound to the owner's and the instigator's OnExitedArena (@2378 / @2442).
fn exited_arena(w: &mut World, vic: FighterId, _from: FighterId) -> bool {
    jake_death(w, vic)
}

/// JakeDeath @1458 bound to the instigator's OnExitArenaWarning (@4736): only the instigator's broadcast.
fn warning(w: &mut World, vic: FighterId, from: FighterId) -> bool {
    let by = w.fighter(vic).holds.hold.as_ref().map(|h| h.by);
    by == Some(from) && jake_death(w, vic)
}

/// Buff_JakEat_C's kind (`combat::holds::hold_kind`).
pub static KIND: HoldKind = HoldKind {
    before_begin: None,
    begin: Some(begin),
    tick: None,
    end: Some(end),
    track: None,
    warning: Some(warning),
    exited_arena: Some(exited_arena),
    latent: Some(latent_tick),
};

/// Buff_JakEat's SetCollision(owner, false) (@236) to (true) (@849): its hold's `coll_off`.
fn eat_collision_off(w: &World, f: FighterId, _b: BuffRef) -> bool {
    w.fighter(f).holds.hold.as_ref().is_some_and(|h| {
        h.cls == BuffClass::BuffJakEat && matches!(h.own, HoldOwn::JakEat(JakEatOwn { coll_off: true, .. }))
    })
}

/// HitStunEnded @3063: GateOpen and no DeathSave -> the parent's HitStunEnded @9547 -> K2_DestroyActor. The owner's
/// KnockedBack is BuffParentActor's DestroyOnOwnerInterrupt (`buff_signal`) ahead of the child's own @4778.
fn eat_on(w: &mut World, f: FighterId, _b: BuffRef, s: BuffSignal, _arg: &BuffSignalArg) -> bool {
    if s != BuffSignal::HitStunEnded {
        return false;
    }
    w.fighter(f).holds.hold.as_ref().is_some_and(|h| {
        h.cls == BuffClass::BuffJakEat
            && matches!(h.own, HoldOwn::JakEat(JakEatOwn { gate: true, death_save: false, .. }))
    })
}

/// Buff_JakEatSlow ReceiveBeginPlay: BPI_SetBlockTaunt(true) on the owner.
fn slow_begin(w: &mut World, f: FighterId, _b: BuffRef, _by: Option<FighterId>, _d: Option<Real>) {
    bpi_set_block_taunt(w, f, true);
}

/// Buff_JakEatSlow ReceiveEndPlay: BPI_SetBlockTaunt(false).
fn slow_end(w: &mut World, f: FighterId, _b: BuffRef) {
    bpi_set_block_taunt(w, f, false);
}

/// This file's registry entries (Buff_JakEat_C's buff side, Buff_JakEatSlow_C; the hold side is [`KIND`]).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        JAK_EAT,
        BuffEntry::new(
            BuffClass::BuffJakEat,
            BuffHooks { actor_collision_off: Some(eat_collision_off), on: Some(eat_on), ..BuffHooks::default() },
        ),
    )?;
    r.buffs.enter(
        JAK_EAT_SLOW,
        BuffEntry::new(
            BuffClass::BuffJakEatSlow,
            BuffHooks { begin: Some(slow_begin), end: Some(slow_end), ..BuffHooks::default() },
        ),
    )
}
