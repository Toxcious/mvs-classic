//! Gizmo's perks: Buff_C023A_Backpack ([`BackpackExt`], C023A_Buff_Perk_ChargeSpeed
//! while attached), Buff_C023A_Bounce (HandleBounceOnAuthority, the car's and the popcorn's OnHitByAlly),
//! Buff_C023A_Dance (the music actors' On Hit Buffs row), C023A_DanceCount_Debuff ([`DanceCountExt`]) and
//! C023A_ForceDance_Debuff. No pawn hook.
//!
//! Hooks of other files (`carBounce.fn`, `popcornBounce.fn`, attach.rs's `onAfterAttached` /
//! `onAfterDetached` listener maps): here they are this file's functions the other files call ([`car_bounce`],
//! [`popcorn_bounce`], [`backpack_attached`], [`backpack_detached`]). C023A_Buff_Perk_ChargeSpeed_C has no entry (it
//! enters none: a data-only buff, `BuffClass::Data`).

use super::gizmo_of;
use super::music::{MUSIC_PICKUP, MUSIC_PROJECTILE, PIANO_NOTE};
use crate::Real;
use crate::State;
use crate::actors::projectile::{class_p, set_instance_vars};
use crate::actors::{ActorRef, ProjectileId};
use crate::combat::buffs::{buff, buff_mut, end_buff, spawn_buff};
use crate::combat::{BuffClass, BuffExt, BuffRef};
use crate::events::{Event, PopKind, PopParams, UiStateOp, UiStateParams};
use crate::fighter::common::{buff_def, latent_frames, warn_text};
use crate::fighter::moves::set_flying_mode;
use crate::fighter::{FighterId, actor_y};
use crate::math::to_int32;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::props::num;
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

/// `BACKPACK`.
pub const BACKPACK: &str = "Buff_C023A_Backpack_C";
/// `BOUNCE`.
pub const BOUNCE: &str = "Buff_C023A_Bounce_C";
/// `DANCE`.
pub const DANCE: &str = "Buff_C023A_Dance_C";
/// `DANCE_COUNT`.
pub const DANCE_COUNT: &str = "C023A_DanceCount_Debuff_C";
/// `FORCE_DANCE`.
pub const FORCE_DANCE: &str = "C023A_ForceDance_Debuff_C";
/// `CHARGE_SPEED`.
pub const CHARGE_SPEED: &str = "C023A_Buff_Perk_ChargeSpeed_C";
/// `FLIP_BOUNCE`.
const FLIP_BOUNCE: &str = "C023A_Attack_Air_FlipBounce_Montage";

/// Buff_C023A_Backpack's extension (`BackpackExt`): the binds @461-690 were made; SpawnedBuffs.
#[derive(State, Debug, Default)]
pub struct BackpackExt {
    pub bound: bool,
    pub spawned: Vec<BuffRef>,
}

/// C023A_DanceCount_Debuff's extension (`DanceCountExt`): the Instigator; the Delay(0.25) @15 in buff ticks left (0:
/// none pending).
#[derive(State, Debug, Default)]
pub struct DanceCountExt {
    pub from: Option<FighterId>,
    pub wait: i32,
}

// ---------------------------------------------------------------- the call surface

/// `live(f, b)`: the buff is on its fighter and running.
fn live(w: &World, b: BuffRef) -> bool {
    buff(w, b).is_some_and(|x| x.frames > 0.0)
}

// Buff_C023A_Bounce (Perk_C023A_Bounce): no event graph: its function HandleBounceOnAuthority(Attacker, HitBoxDamage)
// -> BounceSuccessful, called by the car's OnHitByAlly (C023A_Car_SpawnedActor @16513, `car_bounce`) and the popcorn's
// (C023A_PopCorn @8684, `popcorn_bounce`) on the attacker's Bounce buff (BPI_CheckIfHasBuff). Authority @20:
// HitBoxDamage.AttackAnimation == C023A_Attack_Air_FlipBounce_Montage @34 -> the Attacker (a BPI_BaseFighter) gets
// BPI_AddLandedAttack(AttackID) when its LandedAttackIDs lacks it @174-424, BounceSuccessful = true @479; another
// animation false @495. An Attacker that is no fighter finds an empty list and is true too (@511 -> 220 -> 410 -> 479).

/// `handleBounceOnAuthority(attacker, anim, attackId)`: Buff_C023A_Bounce's HandleBounceOnAuthority; BounceSuccessful.
pub fn handle_bounce_on_authority(w: &mut World, attacker: Option<FighterId>, anim: Name, attack_id: i32) -> bool {
    if w.names.str(anim) != FLIP_BOUNCE {
        return false;
    }
    if let Some(a) = attacker {
        w.fighter_mut(a).decay.landed_attack_ids.insert(attack_id);
    }
    true
}

/// `meleeHitOn(f, key)`: the HitBoxDamage of a melee hit on a spawned actor: AttackAnimation and AttackID of `f`'s
/// active hit box whose hit set holds `key` (the melee pass on projectiles adds 1000 + its id ahead of the HitByAlly
/// broadcast), the last such box in the map. `None` and 0 when no box holds it (a packet without a hit box).
pub fn melee_hit_on(w: &World, f: FighterId, key: i32) -> (Name, i32) {
    let (mut anim, mut id) = (name!("None"), 0);
    for h in w.fighter(f).montages.active_hits.values() {
        if !h.hit_set.contains(&key) {
            continue;
        }
        anim = h.attack_name;
        id = h.attack_id;
    }
    (anim, id)
}

/// `carBounce.fn`: the car's OnHitByAlly @16513 on its Instigator's Bounce buff (car.rs calls it).
pub fn car_bounce(w: &mut World, car: ProjectileId, by: FighterId) -> bool {
    let key = 1000 + w.actors.projectiles.get(car).map_or(0, |p| p.seq);
    let (anim, id) = melee_hit_on(w, by, key);
    handle_bounce_on_authority(w, Some(by), anim, id)
}

/// `popcornBounce.fn`: the popcorn's OnHitBy fighter branch @8256-8761: the same call on the melee box that hit it
/// (popcorn.rs calls it).
pub fn popcorn_bounce(w: &mut World, pr: ProjectileId, by: FighterId) -> bool {
    let key = 1000 + w.actors.projectiles.get(pr).map_or(0, |p| p.seq);
    let (anim, id) = melee_hit_on(w, by, key);
    handle_bounce_on_authority(w, Some(by), anim, id)
}

// Buff_C023A_Backpack (Perk_C023A_Backpack): ReceiveBeginPlay @311 (authority): the owner's Comp_C023A_AllyAttach valid
// @422 (Pawn_C023A's) -> OnAllyAttached bound to its AfterAttachedToAlly @461-555 and OnAllyDetached to its
// AfterDetachedFromAlly @667-690 (attach.rs broadcasts both: one listener each, below, that runs every bound Backpack
// of the attacher). OnAllyAttached @732: the owner's Comp_State_Manager valid -> SpawnBuffActor(C023A_Buff_Perk_
// ChargeSpeed, the owner, Instigator the owner, 0 stacks, 0.0 s, BreakIfBlocked) @913 and Array_Add(SpawnedBuffs) @997.
// OnAllyDetached @1066: SpawnedBuffs from its last index down to 0, each K2_DestroyActor @94 (the array is not cleared:
// a destroyed one is destroyed again, a no-op). ReceiveEndPlay @1369 (authority) -> the same @15. ChargeSpeed's
// ChargeMod 0.6 reaches the charges through Comp_State_Manager UpdatePlayerStatus -> ApplyChargeBuffs @483.

/// The Backpack buff `b`'s extension.
fn backpack_ext(w: &World, b: BuffRef) -> Option<&BackpackExt> {
    match &buff(w, b)?.ext {
        BuffExt::Backpack(e) => Some(e),
        _ => None,
    }
}

/// A running, bound Backpack buff of `f` at `i` of its buff list (the live list: a loop over `f.buffs`).
fn bound_backpack_at(w: &World, f: FighterId, i: usize) -> Option<BuffRef> {
    let x = w.fighter(f).buffs.buffs.get(i)?;
    if w.names.str(x.cls) != BACKPACK || x.frames <= 0.0 {
        return None;
    }
    let b = BuffRef { fighter: f, serial: x.serial };
    backpack_ext(w, b).is_some_and(|e| e.bound).then_some(b)
}

/// `backpackDetached(f, b)`: SpawnedBuffs from its last index down to 0, each destroyed while it runs.
fn backpack_detached_of(w: &mut World, f: FighterId, b: BuffRef) {
    let spawned = backpack_ext(w, b).map(|e| e.spawned.clone()).unwrap_or_default();
    for s in spawned.into_iter().rev() {
        if live(w, s) {
            end_buff(w, f, s);
        }
    }
}

/// `onAfterAttached('Buff_C023A_Backpack')`: each bound Backpack of the attacher `f` spawns ChargeSpeed (attach.rs
/// calls it at AfterAttachedToAlly).
pub fn backpack_attached(w: &mut World, f: FighterId) {
    let mut i = 0;
    while i < w.fighter(f).buffs.buffs.len() {
        if let Some(b) = bound_backpack_at(w, f, i) {
            // @913 (a refused spawn adds none @997)
            let nb = spawn_buff(w, f, name!("C023A_Buff_Perk_ChargeSpeed_C"), 0.0, Some(0), Some(f), true, Some(f));
            if let Some(BuffExt::Backpack(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
                e.spawned.extend(nb);
            }
        }
        i += 1;
    }
}

/// `onAfterDetached('Buff_C023A_Backpack')`: each bound Backpack of `f` destroys its SpawnedBuffs (attach.rs calls it
/// at AfterDetachedFromAlly).
pub fn backpack_detached(w: &mut World, f: FighterId) {
    let mut i = 0;
    while i < w.fighter(f).buffs.buffs.len() {
        if let Some(b) = bound_backpack_at(w, f, i) {
            backpack_detached_of(w, f, b);
        }
        i += 1;
    }
}

// ---------------------------------------------------------------- the buffs

/// Buff_C023A_Backpack_C begin.
fn backpack_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    let bound = gizmo_of(w, f).is_some();
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::Backpack(BackpackExt { bound, spawned: Vec::new() });
    }
}

/// Buff_C023A_Backpack_C end: ReceiveEndPlay @1369 -> @15.
fn backpack_end(w: &mut World, f: FighterId, b: BuffRef) {
    if backpack_ext(w, b).is_some_and(|e| e.bound) {
        backpack_detached_of(w, f, b);
    }
}

// Buff_C023A_Dance (Perk_C023A_Dance): ReceiveBeginPlay @298 -> @10 (authority): the owner's Comp_Spawner valid (every
// fighter's) -> OnSpawnedActor bound to its AfterSpawnedActor @158-252 (the owner's 'afterSpawnedActor' signal).
// OnSpawnedActor @313: the actor a SpawnedActorParent cast to C023A_Music_Projectile, C023A_PianoNote or
// C023A_MusicPickup @392-663 -> its On Hit Buffs.OnHitBuffs[0].AppliedBuffs = C023A_DanceCount_Debuff @677 (the class
// data's one row, C023A_Music_DeBuff to a hit enemy: the dance count takes the silence count's place). An instance
// variable of that actor (`set_instance_vars`: the hit reads the rows of `p`). The three are Projectile kinds
// (music.rs); the signal comes behind their BeginPlay and ahead of their first hit test, as the build's broadcast.

/// Buff_C023A_Dance_C on (afterSpawnedActor): `danceSpawned`.
fn dance_on(w: &mut World, _f: FighterId, _b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s == BuffSignal::AfterSpawnedActor {
        dance_spawned(w, a.actor);
    }
    false
}

fn dance_spawned(w: &mut World, a: Option<ActorRef>) {
    let Some(ActorRef::Projectile(pr)) = a else { return };
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let cls = w.names.str(p.cls);
    if ![MUSIC_PROJECTILE, PIANO_NOTE, MUSIC_PICKUP].contains(&cls) {
        return;
    }
    let ohb = match class_p(w, p).get("On Hit Buffs") {
        Some(Value::Obj(o)) => Some(o),
        _ => None,
    };
    let rows: &[Value] = match ohb.and_then(|o| o.get("OnHitBuffs")) {
        Some(Value::Arr(r)) => r,
        _ => &[],
    };
    let Some(first) = rows.first() else {
        let msg = format!("{DANCE}: {cls} has no On Hit Buffs row 0 (EX_ArrayGetByRef out of range)");
        warn_text(w, &msg);
        return;
    };
    let mut row0 = match first {
        Value::Obj(r) => r.clone(),
        _ => Props::new(),
    };
    row0.insert("AppliedBuffs".to_owned(), Value::Str(DANCE_COUNT.to_owned()));
    let mut new_rows = vec![Value::Obj(row0)];
    new_rows.extend(rows[1..].iter().cloned());
    let mut o = ohb.cloned().unwrap_or_default();
    o.insert("OnHitBuffs".to_owned(), Value::Arr(new_rows));
    set_instance_vars(w, pr, vec![("On Hit Buffs".to_owned(), Value::Obj(o))]);
}

// C023A_DanceCount_Debuff. Class data: IsDebuff, CanStack, MaxStacks 12, BuffDuration 0, StackDecayRate 1,
// StackDecayBuffer 5, BlockerBuffs [C023A_ForceDance_Debuff]. BPI On Max Stacks @260: the owner's Comp_State_Manager
// valid -> Delay(0.25) @15 (latent: a second call while it runs is dropped) -> @70 the owner's SpawnBuffActor(
// C023A_ForceDance_Debuff, the owner, this Instigator, 0 stacks, 2.5 s, BreakIfBlocked) @161 (whose BreakBuffs destroy
// this one), K2_DestroyActor @245. BPI After Status Update Multicast @375 (not a dedicated server, a BP_FighterCharacter
// owner with a UI broker): BuffRemoved or a count <= 0 -> RemoveStateForPlayer DanceCountDebuff @772, else
// AddStateForPlayer @1245 (StateType 1, DisplayType 0, the count / MaxStacks, timer -1, MI_State_Music, Active, PopText
// '' @1164, AutoRemoveTime -1).

fn dance_count_ext(w: &mut World, b: BuffRef) -> Option<&mut DanceCountExt> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::DanceCount(e) => Some(e),
        _ => None,
    }
}

/// C023A_DanceCount_Debuff_C begin.
fn dance_count_begin(w: &mut World, _f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::DanceCount(DanceCountExt { from, wait: 0 });
    }
}

/// C023A_DanceCount_Debuff_C maxStacks: the Delay(0.25) @15.
fn dance_count_max_stacks(w: &mut World, _f: FighterId, b: BuffRef) -> bool {
    if let Some(e) = dance_count_ext(w, b)
        && e.wait == 0
    {
        e.wait = to_int32(latent_frames(0.25));
    }
    false
}

/// C023A_DanceCount_Debuff_C tick: @70 the ForceDance spawn, K2_DestroyActor @245.
fn dance_count_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(e) = dance_count_ext(w, b) else { return false };
    if e.wait == 0 {
        return false;
    }
    e.wait -= 1;
    if e.wait > 0 {
        return false;
    }
    let from = e.from;
    spawn_buff(w, f, name!("C023A_ForceDance_Debuff_C"), 2.5, Some(0), from, true, Some(f)); // @161
    live(w, b) // K2_DestroyActor @245 (the spawn's BreakBuffs ended it already)
}

/// C023A_DanceCount_Debuff_C status.
fn dance_count_status(w: &mut World, f: FighterId, _c: BuffClass, stacks: Real, _applied: bool, removed: bool) {
    if w.fighter(f).life.not_player_character {
        return;
    }
    if removed || stacks <= 0.0 {
        w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(name!("DanceCountDebuff")));
        return;
    }
    let max_value = buff_def(w, ActorRef::Fighter(f), name!("C023A_DanceCount_Debuff_C"))
        .and_then(|d| num(d, "MaxStacks"))
        .unwrap_or(1.0);
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
        id: name!("DanceCountDebuff"),
        state_type: 1,
        display_type: 0,
        current_value: stacks,
        max_value,
        timer: -1.0,
        icon_material: name!("MI_State_Music"),
        active: true,
        pop_text: Name::NONE,
        auto_remove_time: -1.0,
        subicon: None,
    }));
}

// C023A_ForceDance_Debuff. Class data: IsDebuff, BuffDuration 2.5, BreakBuffs [C023A_DanceCount_Debuff], Stun: 2.5 s,
// StunnedHitReaction 6, SuspendPositionOnGround / InAir, PlayDanceMontage (the stun pick and its suspension).
// ReceiveBeginPlay @1854 -> @1517: the owner's CharacterMovement valid -> SetMovementMode(5 Flying, 0) @1702 and
// StopMovementImmediately @1813, as Buff_Fear's: the stun's own suspension when the stun is the pick (it is: both
// suspend flags are set). The stun's BuffStunVisuals with PlayDanceMontage plays the victim's DanceTaunt montage
// (`holds`); the stun's end stops it.
// BPI After Status Update Multicast @10 (not a dedicated server, a BP_FighterCharacter owner with a UI broker):
// BuffApplied -> UI_Functions PopText @849 ("DANCING!", lifetime 1.0, text_color (1, 0.00687, 0, 1), at the owner's
// location in world space, justification 1, scale 1, audio sfx_cha_C023a_silenced_cast: the GizmoForceDance pop text,
// params x / y the owner's place); then BuffRemoved -> RemoveStateForPlayer DanceDebuff @991, else AddStateForPlayer
// @1458 (StateType 1, DisplayType 3, -1 / -1, Timer BuffDuration, MI_State_Music, Active, PopText '' @1373,
// AutoRemoveTime BuffDuration).

/// C023A_ForceDance_Debuff_C begin.
/// SetMovementMode(Flying) @1702 on every path (as Buff_Fear's @1213), then the velocity stop.
fn force_dance_begin(w: &mut World, f: FighterId, _b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    set_flying_mode(w, f, true);
    let x = w.fighter_mut(f);
    x.body.vx = 0.0;
    x.body.vy = 0.0;
    w.fighter_mut(f).platforms.base = None;
}

/// C023A_ForceDance_Debuff_C status.
fn force_dance_status(w: &mut World, f: FighterId, _c: BuffClass, _stacks: Real, applied: bool, removed: bool) {
    if w.fighter(f).life.not_player_character {
        return;
    }
    if applied {
        let (id, x, y) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
        w.fighter_mut(f).events.push(Event::PopText {
            fighter: id,
            kind: PopKind::GizmoForceDance,
            stacks: 0.0,
            gain: None,
            params: Some(PopParams::At { x, y }),
        }); // @849
    }
    if removed {
        w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(name!("DanceDebuff")));
        return;
    }
    let t = buff_def(w, ActorRef::Fighter(f), name!("C023A_ForceDance_Debuff_C"))
        .and_then(|d| num(d, "BuffDuration"))
        .unwrap_or(0.0);
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
        id: name!("DanceDebuff"),
        state_type: 1,
        display_type: 3,
        current_value: -1.0,
        max_value: -1.0,
        timer: t,
        icon_material: name!("MI_State_Music"),
        active: true,
        pop_text: Name::NONE,
        auto_remove_time: t,
        subicon: None,
    }));
}

/// This file's registry entries: the five buffs.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        BACKPACK,
        BuffEntry::new(
            BuffClass::BuffC023ABackpack,
            BuffHooks { begin: Some(backpack_begin), end: Some(backpack_end), ..BuffHooks::default() },
        ),
    )?;
    r.buffs.enter(BOUNCE, BuffEntry::new(BuffClass::BuffC023ABounce, BuffHooks::default()))?;
    r.buffs.enter(
        DANCE,
        BuffEntry::new(BuffClass::BuffC023ADance, BuffHooks { on: Some(dance_on), ..BuffHooks::default() }),
    )?;
    r.buffs.enter(
        DANCE_COUNT,
        BuffEntry::new(
            BuffClass::C023ADanceCountDebuff,
            BuffHooks {
                begin: Some(dance_count_begin),
                max_stacks: Some(dance_count_max_stacks),
                tick: Some(dance_count_tick),
                status: Some(dance_count_status),
                ..BuffHooks::default()
            },
        ),
    )?;
    r.buffs.enter(
        FORCE_DANCE,
        BuffEntry::new(
            BuffClass::C023AForceDanceDebuff,
            BuffHooks { begin: Some(force_dance_begin), status: Some(force_dance_status), ..BuffHooks::default() },
        ),
    )
}
