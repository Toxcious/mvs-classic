#![allow(clippy::doc_lazy_continuation, reason = "the doc lists wrap by the notes' own layout")]
//! The signature perks of Rick: Portal and Meeseeks (Buff_C020_Portal,
//! Buff_C020_Meeseeks).
//!
//! Buff_C020_Portal, Buff_C020_Meeseeks, Buff_MeeseeksDamageWithHitstun; Comp_Pawn_Portal's broadcasts @1582 / @2567 and
//! PortalParent's cooldown select @4443 / @6543 are in portal.rs; C020_ChargedWithPerk is mod.rs's. The values are the
//! perk table's (the buffs' class defaults). All three perks are self-only (SpawnOnAllies false). Buff_C020_Whip is a
//! marker read by C020_ChargedWithPerk; Buff_FireGauntlets is `combat::gauntlet`'s.
//!
//! Buff_C020_Portal: ReceiveBeginPlay @1834 (authority @1864): the owner's Comp_Pawn_Portal valid -> its AfterActorTeleported
//! bound to OnActorTeleported @1984-2078 (the AfterActorTeleported signal, sent to the portal owner's buffs from
//! portal_comp's teleport_server). OnActorTeleported(TeleportedActor) @15: the actor's Comp_Actor_Destructible valid
//! @95, the owner's Comp_State_Manager @205 and Comp_Actor_Destructible @315 (a fighter has both), then three passes in
//! order, each on the two Destructibles' TeamIndex:
//! - @330-745: another team and BPI_BaseFighter -> the owner's SpawnBuffActor(Buff_Fire_C, the actor, the owner,
//!   EnemyIgniteStacks 3, 0.0, true, false).
//! - @834-1275: the same team and (its Destructible's IsProjectile or BPI_Projectiles) -> SpawnBuffActor(
//!   Buff_Elemental_Fire_C, the actor, the owner, AllyProjectileFireStacks 3, 0.0) (`combat::elem::elem_spawn`).
//! - @1364-1745: the same team and BPI_BaseFighter (Rick himself too) -> SpawnBuffActor(Buff_FireGauntlets_C, the
//!   actor, the owner, 0, AllyMeleeIgniteDuration, true, false). AllyMeleeIgniteDuration is not in the class's data:
//!   its default 0, so the gauntlet has no life timer (`combat::gauntlet`).
//! A fighter is BPI_BaseFighter and never IsProjectile. A spawned actor's team is its owner's (portal_comp's
//! check_if_ally); the Destructible valid (@95) and IsProjectile or BPI_Projectiles (@854-1136) are the port's test of
//! every spawned actor (`combat::elem::projectile_like`).
// Deviation: an actor of another list than the projectiles' and items' (a world actor, a marker) of the same team that
// passes the test (`actor_is_projectile`: IsProjectile or BPI_Projectiles in its data) is warned and left without the
// buff: its actors carry no buff list.
//
//! Buff_C020_Meeseeks: ReceiveBeginPlay @284 (authority @314): the owner's Comp_Spawner valid -> AfterSpawnedActor bound to
//! OnSpawnedActor @442-536 (the AfterSpawnedActor signal). OnSpawnedActor @582: IsSoftRef(SpawnedActor,
//! Meeseeks_SpawnedActor_C) (UI/AccessorLibrary IsSoftRef @0-104: the actor's exact class) @739; the owner's
//! Comp_State_Manager valid @110 -> SpawnBuffActor(Buff_MeeseeksDamageWithHitstun_C, the spawned actor, the owner, 0,
//! 0.0, true, false) @195. So only the summon of C020_Attack_SummonMeeseeks_Montage takes it; the attack Meeseeks
//! classes are other classes. Buff_MeeseeksDamageWithHitstun's actor-side condition is in meeseeks.rs.

use crate::actors::world_actor::{actor_cls, actor_owner};
use crate::actors::{ActorRef, item};
use crate::combat::BuffClass;
use crate::combat::BuffRef;
use crate::combat::buffs::{buff, spawn_buff};
use crate::combat::elem::{elem_spawn, projectile_like, spawn_actor_buff};
use crate::fighter::FighterId;
use crate::fighter::common::{actor_is_projectile, buff_def, warn_text};
use crate::math::to_int32;
use crate::name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::props::num;
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// `OnActorTeleported` (the AfterActorTeleported signal's handler).
fn portal_on(w: &mut World, owner: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::AfterActorTeleported {
        return false;
    }
    let Some(t) = a.teleported else { return false };
    let cls = buff(w, b).map(|x| x.cls);
    let d = cls.and_then(|c| buff_def(w, ActorRef::Fighter(owner), c)).cloned().unwrap_or_default();
    let stacks = |k: &str| to_int32(num(&d, k).unwrap_or(0.0));
    let team = w.fighter(owner).team.team;
    match t {
        ActorRef::Fighter(f) => {
            if w.fighter(f).team.team != team {
                spawn_buff(w, f, name!("Buff_Fire_C"), 0.0, Some(stacks("EnemyIgniteStacks")), Some(owner), true, None); // @745
            } else {
                let dur = num(&d, "AllyMeleeIgniteDuration").unwrap_or(0.0);
                spawn_buff(w, f, name!("Buff_FireGauntlets_C"), dur, Some(0), Some(owner), true, None); // @1745
            }
        }
        ActorRef::Projectile(_) | ActorRef::Item(_) => {
            let owned = match t {
                ActorRef::Item(id) => item::is_owned(w, id),
                _ => actor_owner(w, t).is_some(),
            };
            if !owned {
                return false;
            }
            if actor_owner(w, t).is_some_and(|o| w.fighter(o).team.team == team) && projectile_like(w, t) {
                elem_spawn(w, t, BuffClass::BuffElementalFire, stacks("AllyProjectileFireStacks")); // @1275
            }
        }
        _ => {
            if let (Some(o), Some(c)) = (actor_owner(w, t), actor_cls(w, t))
                && w.fighter(o).team.team == team
                && actor_is_projectile(w, ActorRef::Fighter(o), c)
            {
                let m = format!(
                    "Buff_C020_Portal_C: Buff_Elemental_Fire on {} (a World.actors kind) not ported",
                    w.names.str(c)
                );
                warn_text(w, &m);
            }
        }
    }
    false
}

/// `OnSpawnedActor` (the AfterSpawnedActor signal's handler).
fn meeseeks_on(w: &mut World, owner: FighterId, _b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::AfterSpawnedActor {
        return false;
    }
    let Some(act @ ActorRef::Projectile(_)) = a.actor else { return false };
    if actor_cls(w, act) != Some(super::meeseeks::MEESEEKS_SUMMON) {
        return false;
    }
    spawn_actor_buff(w, act, name!("Buff_MeeseeksDamageWithHitstun_C"), 0, 0.0, true, Some(owner));
    false
}

/// This file's registry entries (Buff_C020_Portal_C, Buff_C020_Meeseeks_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let portal = BuffHooks { on: Some(portal_on), ..BuffHooks::default() };
    r.buffs.enter(
        BuffClass::BuffC020Portal.registered_name().expect("a registered class"),
        BuffEntry::new(BuffClass::BuffC020Portal, portal),
    )?;
    let meeseeks = BuffHooks { on: Some(meeseeks_on), ..BuffHooks::default() };
    r.buffs.enter(
        BuffClass::BuffC020Meeseeks.registered_name().expect("a registered class"),
        BuffEntry::new(BuffClass::BuffC020Meeseeks, meeseeks),
    )
}
