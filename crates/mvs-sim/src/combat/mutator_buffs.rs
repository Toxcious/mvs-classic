//! The fighter mutators: PandaGameMode Apply Mutators To Fighter
//! (`world::mutators::apply_mutators_to_fighter`) and the Mutator_Parent children it spawns on a pawn.
//! Mutator_Parent (BuffParentActor child, no graph): BuffDuration 0, StateInfo group "Mutator" (no state breaks or
//! blocks it), DestroyBuffWhenOwnerDefeated and MaxStacks 1 from BuffParentActor: a mutator lives for one life, the
//! defeat ends it and the respawn spawns a fresh one.
//! The classes whose only graph is the parent's BeginPlay act through their class defaults alone, read by the state
//! manager's passes behind the spawn (`combat::buffs::spawn_buff`): Mutator_Giant (MultSizeMod 1.7, taken KnockBackMod
//! 0.85, GiveBuffToSpawnedActors), Mutator_Miniature (MultSizeMod 0.6, taken KnockBackMod 1.25),
//! Mutator_BigHeadAbilityBoost (HeadSizeMultiplier 2, AbilityCooldownMultiplier 0.25; its ubergraph is the parent's
//! BeginPlay alone), Mutator_Boost_CooldownReductionLarge (AbilityCooldownMultiplier 0.6), Mutator_Boost_UnlimitedJumps
//! (JumpCountMod 9999999), Mutator_Boost_AttacksBreakArmor (dealt ArmorBreak).

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::buffs::{BuffExt, BuffRef, buff, buff_mut, set_buff_values, spawn_buff};
use crate::combat::hit::apply_direct_damage;
use crate::combat::{BuffClass, defense};
use crate::fighter::FighterId;
use crate::fighter::common::{buff_def, latent_frames};
use crate::fighter::control::{air_special_max, data_name};
use crate::math::to_int32;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

/// `InstantKill`: Mutator_InstantKill's variables (`BuffExt::InstantKill`).
#[derive(State, Debug, Default)]
pub struct InstantKillExt {
    /// The Delay's frames to go; 0 once it has fired.
    pub left: i32,
}

/// A number of the buff class's defaults (`defNum`), 0 when absent.
fn def_num(w: &World, f: FighterId, b: BuffRef, k: &str) -> Real {
    let Some(cls) = buff(w, b).map(|x| x.cls) else { return 0.0 };
    crate::math::narrow(
        buff_def(w, ActorRef::Fighter(f), cls).and_then(|d| d.get(k)).and_then(Value::as_f64).unwrap_or(0.0),
    )
}

/// Mutator_LowGravity: ReceiveBeginPlay @10, then ChangeGravity @39: the owner's FighterMovementComp
/// .OverrideGravity(GravityValue 0.01) @138. Its JumpMods.MultJumpHeightMod 1.2 and MiscMovementMods
/// .MultFallingAcceleration 0.5 are the state manager's.
/// Offline the scale is not read: the override (`fighter::moves::override_gravity`) is GravityScale 1.0 after every
/// move, never cleared, so 1.0 all match but for the one move a NetGravity write gets.
fn low_gravity_begin(w: &mut World, f: FighterId, _b: BuffRef, _by: Option<FighterId>, _d: Option<Real>) {
    crate::fighter::moves::override_gravity(w, f);
}

/// Mutator_GreyHealth: ReceiveBeginPlay @10, then the owner's Comp_Armor.AddTempHealthPack(MutatorHealthPack) @170
/// (TempHealthType "Mutator", CanOverride, HealthAmount 25, Time 0).
fn grey_health_begin(w: &mut World, f: FighterId, b: BuffRef, _by: Option<FighterId>, _d: Option<Real>) {
    let Some(cls) = buff(w, b).map(|x| x.cls) else { return };
    let Some(p) =
        buff_def(w, ActorRef::Fighter(f), cls).and_then(|d| d.get("MutatorHealthPack")).and_then(Value::as_obj)
    else {
        return;
    };
    let num = |k: &str| p.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let kind = p.get("TempHealthType").and_then(Value::as_str).map_or(Name::NONE, |s| data_name(w, s));
    let can_override = p.get("CanOverride").is_some_and(|v| matches!(v, Value::Bool(true)));
    let (amount, time) = (num("HealthAmount"), num("Time"));
    defense::add_temp_health_pack(w, f, kind, crate::math::narrow(amount), can_override, crate::math::narrow(time));
}

/// Mutator_InstantKill: ReceiveBeginPlay @197, with authority (@162) Delay(0.2) @15 (resumes @70), then
/// ApplyDamage(owner, StartingDamage 200, null, self, null) @90 (`apply_direct_damage`: the causer is the buff actor,
/// so RecordDamage counts its Instigator, the pawn itself: Comp_Pawn_DamageManager RecordDamage @711).
/// Its DealtDamageMods.KnockBackMod 100 (Multiply) is the dealt pass's.
// Unverified: the Delay counts from the buff's first tick, as the other buffs' Delay does (`tick_buffs`).
fn instant_kill_begin(w: &mut World, _f: FighterId, b: BuffRef, _by: Option<FighterId>, _d: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::InstantKill(InstantKillExt { left: to_int32(latent_frames(0.2)) });
    }
}

fn instant_kill_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let fired = match buff_mut(w, b).map(|x| &mut x.ext) {
        Some(BuffExt::InstantKill(e)) if e.left > 0 => {
            e.left -= 1;
            e.left == 0
        }
        _ => false,
    };
    if fired {
        let amount = def_num(w, f, b, "StartingDamage");
        apply_direct_damage(w, f, amount, Some(f));
    }
    false
}

/// Mutator_Boost_AdditionalAirSpecial: ReceiveBeginPlay @10, then the owner's AirSpecialMax + 1 (@119-259). No
/// EndPlay of its own, and nothing else writes AirSpecialMax: the respawn keeps the pawn, so each life's fresh
/// mutator adds one more (3 in the first life, 4 in the second), as the build's graph does.
fn air_special_begin(w: &mut World, f: FighterId, _b: BuffRef, _by: Option<FighterId>, _d: Option<Real>) {
    let max = air_special_max(w, f) + 1.0;
    w.fighter_mut(f).air.air_special_max_set = Some(max);
}

/// Mutator_Boost_WeakenOnHit: BPI On Fighter Owner Hit Enemy Player (the owner's Comp_Pawn_HitBoxHandler
/// HitEnemyPlayer, the `HitEnemy` signal) @10: a valid hit player -> its Comp_State_Manager.SpawnBuffActor
/// (Buff_Weakened_C, the hit player, self, 1 stack, 0.0, true, false) @104. The buff actor as the Instigator gives
/// the new buff its owner, the pawn (SpawnBuffActor @440-842).
fn weaken_on(w: &mut World, f: FighterId, _b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s == BuffSignal::HitEnemy
        && let Some(v) = a.victim
    {
        spawn_buff(w, v, name!("Buff_Weakened_C"), 0.0, Some(1), Some(f), true, None);
    }
    false
}

/// Mutator_Boost_ChargeFreeze: BPI On Fighter Owner Hit Enemy Player @10: the event's Actor (HitEnemyPlayer's
/// second pin, the handler's owner: the pawn, Comp_Pawn_HitBoxHandler @22750-22770) has a Comp_Pawn_ChargeAttack
/// whose GetChargeValue(PercentCharged) >= ChargeThreshold 1 (@155-240), and the hit player is a BaseFighter: its
/// Comp_State_Manager.SpawnBuffActor(Buff_Ice_C, the hit player, the owner, 15 stacks, 0.0, true, false) @353.
fn charge_freeze_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some(v) = a.victim.filter(|_| s == BuffSignal::HitEnemy) else { return false };
    if crate::fighter::percent_charged(w, f) >= def_num(w, f, b, "ChargeThreshold") {
        spawn_buff(w, v, name!("Buff_Ice_C"), 0.0, Some(15), Some(f), true, None);
    }
    false
}

/// Mutator_Boost_IceGauntlets: ReceiveBeginPlay @10, then the owner's Comp_State_Manager.SpawnBuffActor
/// (Buff_SupermanAllyIceGauntlet_C, owner, self, 0 stacks, 0.0, true, false) @91 (no life timer: the gauntlet stays
/// for the life), and on the new actor ExpireOnHit = false @246 (`combat::gauntlet` reads it from the instance). The
/// gauntlet's own do-once still lets the first hit alone give Buff_Ice.
fn ice_gauntlets_begin(w: &mut World, f: FighterId, _b: BuffRef, _by: Option<FighterId>, _d: Option<Real>) {
    let cls = name!("Buff_SupermanAllyIceGauntlet_C");
    if let Some(g) = spawn_buff(w, f, cls, 0.0, Some(0), Some(f), true, None) {
        let mut p = Props::default();
        p.insert("ExpireOnHit".to_owned(), Value::Bool(false));
        set_buff_values(w, g, &p);
    }
}

/// The fighter mutators' entries (`buffs({ ... })`), and Mutator_TestPrint's `buffGraphsPorted` (ReceiveTick
/// PrintString("TEST MUTATOR ACTIVE ON: " + the owner's name) @149, a debug line and nothing else).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    use BuffClass as C;
    r.buff_graphs_ported.enter("Mutator_TestPrint_C", ())?;
    let none = BuffHooks::default();
    for (c, h) in [
        (C::MutatorLowGravity, BuffHooks { begin: Some(low_gravity_begin), ..none }),
        (C::MutatorGreyHealth, BuffHooks { begin: Some(grey_health_begin), ..none }),
        (C::MutatorInstantKill, BuffHooks { begin: Some(instant_kill_begin), tick: Some(instant_kill_tick), ..none }),
        (C::MutatorBoostAdditionalAirSpecial, BuffHooks { begin: Some(air_special_begin), ..none }),
        (C::MutatorBoostWeakenOnHit, BuffHooks { on: Some(weaken_on), ..none }),
        (C::MutatorBoostChargeFreeze, BuffHooks { on: Some(charge_freeze_on), ..none }),
        (C::MutatorBoostIceGauntlets, BuffHooks { begin: Some(ice_gauntlets_begin), ..none }),
    ] {
        r.buffs.enter(c.registered_name().expect("a registered class"), BuffEntry::new(c, h))?;
    }
    Ok(())
}
