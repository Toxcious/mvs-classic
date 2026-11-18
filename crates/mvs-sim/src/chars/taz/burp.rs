//! The burp: the notifies and conditions of his eat and spit montages that read or
//! write the projectile store (`projectile_eat.rs`), and Pawn_C015 OnActorSpawned (@17254), the Tasty stacks on a
//! burped actor.

use super::pawn::{is_eating_projectile, stored_projectile_class};
use super::projectile_eat::{TRASH, reset_stored_projectile};
use super::{taz, taz_mut, taz_of};
use crate::actors::ActorRef;
use crate::combat::buffs::turn_off_hit_boxes;
use crate::combat::elem::{instigator_charge, spawn_actor_buff};
use crate::combat::perks::perk_part_actor;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::anim::{conds_ok, notify_data};
use crate::fighter::common::data_of;
use crate::fighter::match_side::{Foreign, SpawnRequest};
use crate::math::min;
use crate::name;
use crate::registry::entries::{CondFn, Condition};
use crate::registry::props::{num, num_or};
use crate::registry::spawner_notifies::cond_list;
use crate::registry::{CharHooks, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use mvs_data::value::Value;
use std::sync::Arc;

const ANVIL: &str = "C015Anvil_C";

/// The hooks of this file in Taz's CharHooks (afterSpawnedActor).
pub fn hooks(h: &mut CharHooks) {
    h.after_spawned_actor = Some(after_spawned_actor);
}

/// The parent's handler (AnimNotify_ConditionalSpawner_C), looked up at the call: an owner that is no Taz takes its
/// path.
fn parent(w: &World) -> NotifyHandler {
    let r: &'static Registry = w.registry;
    *r.notifies.get("AnimNotify_ConditionalSpawner_C").expect("AnimNotify_ConditionalSpawner_C is not entered")
}

/// AnimNotify_C015ResetConditionalSpawner Received_NotifyBegin (authority): an owner without BPI_C015 gets the
/// parent's (@1033). For a Taz: BPI_CheckConditions(Conditions) @366 (false: nothing); IsFacingLeft and the attack
/// count into locals the call does not read; TurnOffHitboxCheck @958 (true: nothing); DestroyAllActorsOnNotifyEnd ->
/// AddSpawnerNotify @1159 (no instance in the data has it); BPI_GetStoredProjectile @1275. CompSpawnActor @1782 with
/// the class the stored one when IsValidClass (always: the getter gives C015Trash_C for None), else ActorClass (the
/// Spit montages' C015StoredProjectile_C, a data-only placeholder that is therefore never spawned and has no entry);
/// MaxSpeed = FMin(stored MaxSpeed, MaxSpeedOfStoredProjectiles) when the stored one is not 0 and
/// UseStoredProjectileMoveValues, else the notify's; InitialSpeed by the same rule; the notify's own socket,
/// transforms, direction, charge and momentum values; AttackID C015LocalAttackID and AnimNotifySpawner
/// C015LocalAnimNotifySpawner, two variables nothing writes (0 and null: Comp_Spawner takes the owner's attack count,
/// and no destroy at the notify's end is registered), Instigator null, HoldChargeOverrideSpeedMod false.
/// The spawn is the world's spawn of a SpawnRequest as the parent's. A class of another kit (the actor he ate) is
/// spawned with him as its owner: the request is marked foreign, so the world spawns it only for a kind whose code was
/// read against another owner (`foreign_owner`) and refuses the rest by name (warned: "burp of an eaten actor
/// refused").
// Deviation: the refused classes (any reflectable projectile of a kit whose kind is not marked) are not burped.
fn reset_spawner_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if taz_of(w, f).is_none() {
        if let Some(b) = parent(w).begin {
            b(w, f, n, idx);
        }
        return;
    }
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    let conds = cond_list(w, p, "Conditions");
    if !conds_ok(w, f, conds.as_deref(), false) {
        return;
    }
    if !w.fighter(f).buffs.buffs.is_empty() && turn_off_hit_boxes(w, f) {
        return; // @958-1028
    }
    let cls = stored_projectile_class(w, f);
    let t = taz(w, f);
    let use_stored = p.get("UseStoredProjectileMoveValues").is_some_and(crate::registry::props::truthy);
    let cap = num_or(p, "MaxSpeedOfStoredProjectiles", 0.0);
    let ms =
        if t.stored_max_speed != 0.0 && use_stored { min(t.stored_max_speed, cap) } else { num_or(p, "MaxSpeed", 0.0) };
    let is = if t.stored_starting_speed != 0.0 && use_stored {
        min(t.stored_starting_speed, cap)
    } else {
        num_or(p, "InitialSpeed", 0.0)
    };
    let cls_s = w.names.str(cls);
    let mut props = p.clone();
    props.insert("ActorClass".into(), Value::Str(cls_s.to_owned()));
    props.insert("InitialSpeed".into(), Value::Num(f64::from(is)));
    props.insert("MaxSpeed".into(), Value::Num(f64::from(ms)));
    props.insert("HoldChargeOverrideSpeedMod".into(), Value::Bool(false));
    // (C015Anvil_C, the EatProjectiles perk's stored class, is his perk part's: perks.rs, not an eaten actor's)
    let own = data_of(w, f).fighter.actors.as_ref().is_some_and(|m| m.contains_key(cls_s));
    let foreign = if !own && perk_part_actor(w, f, None, cls_s).is_none() { Foreign::Burp } else { Foreign::None };
    let r = SpawnRequest {
        cls,
        n,
        attack_id: w.fighter(f).decay.attack_count,
        foreign,
        copy: None,
        charge: instigator_charge(w, f),
        aim: None,
        props: Some(Arc::new(props)),
        instigator: None,
        origin: None,
    };
    w.fighter_mut(f).requests.spawn_requests.push(r);
}

/// Received_NotifyEnd: the parent's (the Conditions again, then the Spawner's end), then for a BPI_C015 owner
/// BPI_ResetStoredProjectile @159, whatever the parent did.
fn reset_spawner_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if let Some(e) = parent(w).end {
        e(w, f, n, idx);
    }
    if taz_of(w, f).is_some() {
        reset_stored_projectile(w, f);
    }
}

/// Received_NotifyBegin / End: the owner's BPI_SetBurpingProjectile(true / false) (Pawn_C015 @8477, authority).
fn burping_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    if let Some(t) = taz_mut(w, f) {
        t.burping_projectile = true;
    }
}

fn burping_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    if let Some(t) = taz_mut(w, f) {
        t.burping_projectile = false;
    }
}

/// Received_NotifyBegin / End: the owner's BPI_SetHittingProjectiles(true / false) (Pawn_C015 @13065, authority): with
/// TazEatBuff valid (Buff_C015Eating, spawned in his BeginPlay @9988 and kept for the match), its SetActiveServer(Active)
/// (Buff_C015Eating @352: Active, and the owner's Comp_Pawn_DamageManager Light Projectile Armor = Active @556), then
/// HittingProjectiles = Active @13179. Buff_C015Eating's DealtDamageModCondition returns Active; its DealtDamageMods
/// are all of mod type 3 with value 0 in the data, so the port does not keep the buff itself.
fn hitting_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    set_hitting_projectiles(w, f, true);
}

fn hitting_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    set_hitting_projectiles(w, f, false);
}

fn set_hitting_projectiles(w: &mut World, f: FighterId, on: bool) {
    if taz_of(w, f).is_none() {
        return;
    }
    w.fighter_mut(f).invuln.light_projectile_armor = Some(on);
    taz_mut(w, f).expect("a Taz").hitting_projectiles = on;
}

/// C015BurpFullPower Check: BPI_GetStoredProjectile's class is not C015Trash_C (false without BPI_C015).
fn burp_full_power(w: &mut World, f: FighterId) -> bool {
    taz_of(w, f).is_some() && w.names.str(stored_projectile_class(w, f)) != TRASH
}

/// Condition_C015EatingProjectile Check: BPI_IsEatingProjectile (ProjectilesBeingEaten > 0; false without BPI_C015).
fn eating_projectile(w: &mut World, f: FighterId) -> bool {
    taz_of(w, f).is_some() && is_eating_projectile(w, f)
}

/// Pawn_C015 OnActorSpawned (@17254, bound to his Comp_Spawner.AfterSpawnedActor in BeginPlay): while
/// BurpingProjectile, an actor that is no C015Trash and no C015Anvil and is a SpawnedActorParent gets
/// TriggerAbilityOnSpawn = TriggerAbilityOnSpawnIndex = false (@17575 / @17608: read in its BeginPlay @13212, which
/// ran: nothing left to do) and, its Comp_State_Manager valid, SpawnBuffActor(Buff_TastyOnHit_C, it, him,
/// TastyStacksFromBurpedProjectile, 0.0, BreakIfBlocked true) @17782.
// Unverified: called behind the shared binding (`enlarge_spawned`); the order of the two bindings is not read.
fn after_spawned_actor(w: &mut World, f: FighterId, a: ActorRef) {
    if !taz(w, f).burping_projectile {
        return;
    }
    let Some(cls) = crate::actors::world_actor::actor_cls(w, a) else { return };
    let c = w.names.str(cls);
    if c == TRASH || c == ANVIL {
        return;
    }
    let stacks = num(&data_of(w, f).fighter.pawn, "TastyStacksFromBurpedProjectile").unwrap_or(0.0);
    let stacks = crate::math::to_int32(stacks);
    spawn_actor_buff(w, a, name!("Buff_TastyOnHit_C"), stacks, 0.0, true, Some(f));
}

/// This file's registry entries (the three notifies, the two conditions).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    type Fns = Option<fn(&mut World, FighterId, NotifyRef, i32)>;
    let be = |begin: Fns, end: Fns| NotifyHandler { begin, end, ..NotifyHandler::default() };
    let n = &mut r.notifies;
    n.enter("AnimNotify_C015ResetConditionalSpawner_C", be(Some(reset_spawner_begin), Some(reset_spawner_end)))?;
    n.enter("AnimNotify_C015BurpingProjectile_C", be(Some(burping_begin), Some(burping_end)))?;
    n.enter("AnimNotify_C015HitProjectiles_C", be(Some(hitting_begin), Some(hitting_end)))?;
    r.conditions.enter("C015BurpFullPower_C", Condition::Fn(burp_full_power as CondFn))?;
    r.conditions.enter("Condition_C015EatingProjectile_C", Condition::Fn(eating_projectile as CondFn))?;
    Ok(())
}
