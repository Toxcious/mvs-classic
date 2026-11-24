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
use crate::combat::buffs::{BuffExt, BuffRef, buff, buff_mut, inst_view, set_buff_values, spawn_buff};
use crate::combat::hit::apply_direct_damage;
use crate::combat::{BuffClass, defense};
use crate::fighter::FighterId;
use crate::fighter::common::{buff_def, latent_frames};
use crate::fighter::control::data_name;
use crate::math::to_int32;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

/// The port's Chaos Wheel reworks of the build's mutators (the user's call, 2026-11-22: a dull mutator is changed for
/// every use): Ability Frenzy's cooldown multiplier.
const FRENZY_COOLDOWN: Real = 0.1;
/// Heavy Hitters' dealt knockback multiplier.
const HEAVY_KNOCKBACK: Real = 1.4;
/// Double Damage's dealt damage multiplier.
const DOUBLE_DAMAGE: Real = 2.0;
/// Ice Charge's charge speed multiplier and the share of a full charge whose hit freezes.
const ICE_CHARGE_RATE: Real = 2.0;
const ICE_CHARGE_THRESHOLD: Real = 0.5;
/// Freeze Touch: every this many hits a fighter lands, the hit one freezes; the Ice stacks a freeze gives (Charge
/// Freeze's own 15).
const FREEZE_EVERY: i32 = 3;
const FREEZE_STACKS: i32 = 15;
/// Air Show's air specials a fighter may use before landing.
const AIR_SHOW_MAX: Real = 99.0;
/// Shields Up's grey health and the seconds without a hit after which it is given again whole.
const SHIELD: Real = 50.0;
const SHIELD_QUIET: Real = 4.0;

/// `Chaos`: the reworked mutators' own count (`BuffExt::Chaos`): Freeze Touch's hits, Shields Up's ticks without a
/// hit and the pack and damage it last saw.
#[derive(State, Debug, Default)]
pub struct ChaosExt {
    pub hits: i32,
    pub quiet: i32,
    pub shield: Real,
    pub damage: Real,
}

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

/// Mutator_GreyHealth, reworked as Shields Up: the build's pack (ReceiveBeginPlay @10, the owner's
/// Comp_Armor.AddTempHealthPack(MutatorHealthPack) @170: TempHealthType "Mutator", CanOverride, Time 0) with
/// `SHIELD` health in place of its HealthAmount 25, given again whole once the fighter has gone `SHIELD_QUIET`
/// seconds without a hit (`shields_up_tick`).
// Deviation: the build's pack is 25 grey health given once a life; the port's Chaos Wheel rework gives 50 and
// refills it.
fn grey_health_begin(w: &mut World, f: FighterId, b: BuffRef, _by: Option<FighterId>, _d: Option<Real>) {
    give_shield(w, f, b);
    let damage = w.fighter(f).stats.damage;
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::Chaos(ChaosExt { shield: SHIELD, damage, ..ChaosExt::default() });
    }
}

/// The pack's kind (`MutatorHealthPack.TempHealthType`) and whether it may override, from the class defaults.
fn shield_kind(w: &World, f: FighterId, b: BuffRef) -> Option<(Name, bool)> {
    let cls = buff(w, b)?.cls;
    let p = buff_def(w, ActorRef::Fighter(f), cls)?.get("MutatorHealthPack")?.as_obj()?;
    let kind = p.get("TempHealthType").and_then(Value::as_str).map_or(Name::NONE, |s| data_name(w, s));
    let can_override = p.get("CanOverride").is_some_and(|v| matches!(v, Value::Bool(true)));
    Some((kind, can_override))
}

fn give_shield(w: &mut World, f: FighterId, b: BuffRef) {
    let Some((kind, can_override)) = shield_kind(w, f, b) else { return };
    defense::add_temp_health_pack(w, f, kind, SHIELD, can_override, 0.0);
}

/// Shields Up's tick: a hit is a fall of the pack's health or a rise of the damage; `SHIELD_QUIET` seconds with
/// none and a pack below `SHIELD` give it again whole.
fn shields_up_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some((kind, _)) = shield_kind(w, f, b) else { return false };
    let fr = w.fighter(f);
    let shield = fr.stats.temp_health.iter().find(|p| p.kind == kind).map_or(0.0, |p| p.amount);
    let damage = fr.stats.damage;
    let quiet = to_int32(latent_frames(SHIELD_QUIET));
    let refill = match buff_mut(w, b).map(|x| &mut x.ext) {
        Some(BuffExt::Chaos(e)) => {
            if shield < e.shield || damage > e.damage {
                e.quiet = 0;
            } else {
                e.quiet += 1;
            }
            (e.shield, e.damage) = (shield, damage);
            e.quiet >= quiet && shield < SHIELD
        }
        _ => false,
    };
    if refill {
        give_shield(w, f, b);
        if let Some(BuffExt::Chaos(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
            (e.quiet, e.shield) = (0, SHIELD);
        }
    }
    false
}

/// A struct of the class defaults (`DealtDamageMods`, `AbilityMods`...) with `fields` written over it, on the live
/// buff's own values: the passes behind the spawn read them (`combat::buffs::inst_view`).
fn write_mods(w: &mut World, f: FighterId, b: BuffRef, key: &str, fields: &[(&str, Value)]) {
    let Some(cls) = buff(w, b).map(|x| x.cls) else { return };
    let mut m = buff_def(w, ActorRef::Fighter(f), cls)
        .and_then(|d| d.get(key))
        .and_then(Value::as_obj)
        .cloned()
        .unwrap_or_default();
    for (k, v) in fields {
        m.insert((*k).to_owned(), v.clone());
    }
    set_buff_values(w, b, &Props::from_iter([(key.to_owned(), Value::Obj(m))]));
}

/// `BuffModType` Multiply, as `combat::buffs::mod_type` reads a number.
const MULTIPLY: Value = Value::Num(1.0);

/// Mutator_Boost_CooldownReductionLarge, reworked as Ability Frenzy: its AbilityMods.AbilityCooldownMultiplier 0.6
/// made `FRENZY_COOLDOWN`.
// Deviation: the build's cooldowns are x0.6 under it; the port's Chaos Wheel rework makes them x0.1.
fn frenzy_begin(w: &mut World, f: FighterId, b: BuffRef, _by: Option<FighterId>, _d: Option<Real>) {
    write_mods(w, f, b, "AbilityMods", &[("AbilityCooldownMultiplier", Value::Num(f64::from(FRENZY_COOLDOWN)))]);
}

/// Mutator_Boost_AttacksBreakArmor, reworked as Heavy Hitters: its DealtDamageMods (ArmorBreak) with a KnockBackMod
/// of `HEAVY_KNOCKBACK`, Multiply.
// Deviation: the build's mutator breaks armor alone; the port's Chaos Wheel rework also launches harder.
fn heavy_begin(w: &mut World, f: FighterId, b: BuffRef, _by: Option<FighterId>, _d: Option<Real>) {
    let kb = Value::Num(f64::from(HEAVY_KNOCKBACK));
    write_mods(w, f, b, "DealtDamageMods", &[("KnockBackMod", kb), ("KnockBackModType", MULTIPLY)]);
}

/// Mutator_Boost_WeakenOnHit, reworked as Double Damage: its BPI On Fighter Owner Hit Enemy Player graph (a hit
/// gave the hit player a stack of Buff_Weakened_C) is not run; its DealtDamageMods get a DamageMod of
/// `DOUBLE_DAMAGE`, Multiply.
// Deviation: the build's mutator weakens the one hit; the port's Chaos Wheel rework doubles the damage dealt.
fn double_damage_begin(w: &mut World, f: FighterId, b: BuffRef, _by: Option<FighterId>, _d: Option<Real>) {
    let dmg = Value::Num(f64::from(DOUBLE_DAMAGE));
    write_mods(w, f, b, "DealtDamageMods", &[("DamageMod", dmg), ("DamageModType", MULTIPLY)]);
}

/// Mutator_Boost_ChargeFreeze, reworked as Ice Charge: charges fill `ICE_CHARGE_RATE` times as fast (ChargeMod,
/// Multiply, read by `recalc_charge_mod`) and its ChargeThreshold 1 is `ICE_CHARGE_THRESHOLD`.
// Deviation: the build's mutator freezes on a full charge alone and leaves the charge's speed.
fn ice_charge_begin(w: &mut World, _f: FighterId, b: BuffRef, _by: Option<FighterId>, _d: Option<Real>) {
    let values = Props::from_iter([
        ("ChargeMod".to_owned(), Value::Num(f64::from(ICE_CHARGE_RATE))),
        ("ChargeModType".to_owned(), MULTIPLY),
        ("ChargeThreshold".to_owned(), Value::Num(f64::from(ICE_CHARGE_THRESHOLD))),
    ]);
    set_buff_values(w, b, &values);
}

/// Mutator_Boost_ChargeFreeze: BPI On Fighter Owner Hit Enemy Player @10: the event's Actor (HitEnemyPlayer's
/// second pin, the handler's owner: the pawn, Comp_Pawn_HitBoxHandler @22750-22770) has a Comp_Pawn_ChargeAttack
/// whose GetChargeValue(PercentCharged) >= ChargeThreshold (@155-240; the rework's, `ice_charge_begin`), and the hit
/// player is a BaseFighter: its Comp_State_Manager.SpawnBuffActor(Buff_Ice_C, the hit player, the owner, 15 stacks,
/// 0.0, true, false) @353.
fn charge_freeze_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some(v) = a.victim.filter(|_| s == BuffSignal::HitEnemy) else { return false };
    let threshold = inst_view(w, b).num("ChargeThreshold").unwrap_or(1.0);
    if crate::fighter::percent_charged(w, f) >= threshold {
        spawn_buff(w, v, name!("Buff_Ice_C"), 0.0, Some(FREEZE_STACKS), Some(f), true, None);
    }
    false
}

/// Mutator_Boost_IceGauntlets, reworked as Freeze Touch: its ReceiveBeginPlay (the Superman ally gauntlet for the
/// life, whose first hit alone gives Buff_Ice) is not run; every `FREEZE_EVERY`th hit the owner lands gives the hit
/// player `FREEZE_STACKS` of Buff_Ice_C, as Ice Charge's hit does.
// Deviation: the build's mutator ices the first hit of a life (the ally gauntlet); the port's Chaos Wheel rework
// ices every third hit.
fn freeze_touch_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some(v) = a.victim.filter(|_| s == BuffSignal::HitEnemy) else { return false };
    let Some(x) = buff_mut(w, b) else { return false };
    if !matches!(x.ext, BuffExt::Chaos(_)) {
        x.ext = BuffExt::Chaos(ChaosExt::default());
    }
    let BuffExt::Chaos(e) = &mut x.ext else { return false };
    e.hits += 1;
    if e.hits >= FREEZE_EVERY {
        e.hits = 0;
        spawn_buff(w, v, name!("Buff_Ice_C"), 0.0, Some(FREEZE_STACKS), Some(f), true, None);
    }
    false
}

/// Mutator_Boost_AdditionalAirSpecial, reworked as Air Show: its ReceiveBeginPlay (@10, the owner's AirSpecialMax
/// + 1, @119-259) sets the max to `AIR_SHOW_MAX`, so air specials do not run out in the air.
// Deviation: the build's mutator gives one more air special (each life's adds another); the port's Chaos Wheel
// rework gives as many as a fighter can use.
fn air_special_begin(w: &mut World, f: FighterId, _b: BuffRef, _by: Option<FighterId>, _d: Option<Real>) {
    w.fighter_mut(f).air.air_special_max_set = Some(AIR_SHOW_MAX);
}

/// The fighter mutators' entries (`buffs({ ... })`), and Mutator_TestPrint's `buffGraphsPorted` (ReceiveTick
/// PrintString("TEST MUTATOR ACTIVE ON: " + the owner's name) @149, a debug line and nothing else).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    use BuffClass as C;
    r.buff_graphs_ported.enter("Mutator_TestPrint_C", ())?;
    let none = BuffHooks::default();
    for (c, h) in [
        (C::MutatorLowGravity, BuffHooks { begin: Some(low_gravity_begin), ..none }),
        (C::MutatorGreyHealth, BuffHooks { begin: Some(grey_health_begin), tick: Some(shields_up_tick), ..none }),
        (C::MutatorInstantKill, BuffHooks { begin: Some(instant_kill_begin), tick: Some(instant_kill_tick), ..none }),
        (C::MutatorBoostAdditionalAirSpecial, BuffHooks { begin: Some(air_special_begin), ..none }),
        (C::MutatorBoostWeakenOnHit, BuffHooks { begin: Some(double_damage_begin), ..none }),
        (C::MutatorBoostChargeFreeze, BuffHooks { begin: Some(ice_charge_begin), on: Some(charge_freeze_on), ..none }),
        (C::MutatorBoostIceGauntlets, BuffHooks { on: Some(freeze_touch_on), ..none }),
        (C::MutatorBoostCooldownReductionLarge, BuffHooks { begin: Some(frenzy_begin), ..none }),
        (C::MutatorBoostAttacksBreakArmor, BuffHooks { begin: Some(heavy_begin), ..none }),
    ] {
        r.buffs.enter(c.registered_name().expect("a registered class"), BuffEntry::new(c, h))?;
    }
    Ok(())
}
