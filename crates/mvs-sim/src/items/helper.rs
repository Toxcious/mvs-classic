//! Item_Helper_C: one of Bugs Bunny's ACME box pool, the TV ([`HelperItem`]), whose
//! HelperList is ItemHelper_BananaGuard_C (banana_guard.rs) and ItemHelper_Nepter_C (nepter.rs). Durability 0, CanUseItem
//! and DestroyWhenSpent, DestroyedOnHit.
//! - ActivateObject and SpawnOnThrown both run @29 (authority): Array_Random(HelperList); a SpawnedActorParent class
//!   (Nepter) through the owner's Comp_Spawner.CompSpawnActor(not attached, world transforms, at (0, Y, Z) of its
//!   location, no direction or speed) @434; another (BananaGuard, a Character) by a second Array_Random of the list
//!   and BeginDeferredActorSpawnFromClass / FinishSpawningActor at the same place, the Instigator set (@595-1089).
//!   So Nepter comes three times in four. Then ItemActorParent's ActivateObject @560 on every branch (Durability 0
//!   and DestroyWhenSpent: the TV goes, `item_activate_parent`).
//! - Thrown @1184: the parent's, then OnDestroyed bound to SpawnOnThrown @1132: once thrown, whatever ends it spawns
//!   a helper. OnDestroyed comes after ReceiveEndPlay: the kind's `end`.
//!
//! Ported for a spawn with no owner (the FFA area's): the class reads its owner in ActivateObject and after a Thrown,
//! both from a fighter's hand. The helpers are their own classes (banana_guard.rs, nepter.rs): the spawn goes to
//! them by name and warns while one has no entry.

use crate::State;
use crate::actors::item::ItemExt;
use crate::actors::item::item_activate_parent;
use crate::actors::{ActorClass, ActorRef, ItemClass, ItemId};
use crate::combat::elem::instigator_charge;
use crate::fighter::FighterId;
use crate::fighter::common::{actor_data, warn_text};
use crate::names::Name;
use crate::registry::entries::ItemHooks;
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;
use crate::world::spawning::{add_actor, comp_spawn_actor};
use mvs_data::value::Value;

/// `HelperItem extends Item` (Item_Helper_C).
#[derive(State, Debug, Default)]
pub struct HelperItem {
    /// OnDestroyed bound to SpawnOnThrown (its Thrown); it stays unset until then.
    pub spawn_on_destroyed: bool,
}

/// The SpawnedActorParent children of the list (the ClassDynamicCast @122); the rest takes the deferred spawn.
fn spawned_actor_parent(cls: &str) -> bool {
    cls == "ItemHelper_Nepter_C"
}

/// The class `HelperList[Array_Random]`'s name, `None` for an empty list; the string stays `&str` (a data string,
/// interned at bind).
fn pick(w: &mut World, list: &[String]) -> Option<String> {
    if list.is_empty() {
        return None;
    }
    let n = w.random_int(crate::real(u32::try_from(list.len()).expect("a short list")));
    let i = usize::try_from(crate::math::to_int32(n)).expect("an index");
    Some(list[i].clone())
}

/// `spawnHelper`.
fn spawn_helper(w: &mut World, it: ItemId) {
    let data = std::sync::Arc::clone(&w.data);
    let i = w.actors.items.get(it).expect("a live item");
    let (x, y) = (i.x, i.y);
    let Some(f) = i.owner else { return };
    let list: Vec<String> = match i.def(&data).p.get("HelperList") {
        Some(Value::Arr(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
        _ => Vec::new(),
    };
    let Some(cls) = pick(w, &list) else { return };
    if spawned_actor_parent(&cls) {
        // A World.actors class (ActorKind.spawn): the owner's ability trigger, the class's spawn at the place (world
        // transforms: the TV's), then numbered and listed (CompSpawnActor).
        let Some(c) = w.names.get(&cls) else { return warn_text(w, &format!("spawned actor not implemented ({cls})")) };
        let own = ActorRef::Fighter(f);
        let Some(a) = actor_data(w, &data, own, c) else {
            return warn_text(w, &format!("spawned actor not implemented ({cls})"));
        };
        let Some(spawn) = world_deferred(w, c) else {
            return warn_text(w, &format!("spawned actor not implemented ({cls})"));
        };
        crate::fighter::control::actor_spawn_ability(w, f, &a.p);
        let id = spawn(w, f, c, x, y);
        let charge = instigator_charge(w, f);
        comp_spawn_actor(w, id, charge, f);
        return;
    }
    let Some(other) = pick(w, &list) else { return }; // the second Array_Random @595
    // BeginDeferredActorSpawnFromClass(other, at (0, Y, Z) of the TV, collision handling 2), the Instigator, then
    // FinishSpawningActor @595-1089: the class's own kind (ActorKind.deferred).
    let c = w.names.get(&other);
    let spawn = c.and_then(|c| world_deferred(w, c));
    let own = ActorRef::Fighter(f);
    let has_def = c.is_some_and(|c| actor_data(w, &data, own, c).is_some());
    let (Some(c), Some(spawn), true) = (c, spawn, has_def) else {
        return warn_text(w, &format!("Item_Helper_C: the deferred spawn of {other} has no entry"));
    };
    let id = spawn(w, f, c, x, y);
    add_actor(w, id, None, false);
}

/// A kind's `deferred`.
type Deferred = fn(&mut World, FighterId, Name, crate::Real, crate::Real) -> crate::actors::ActorId;

/// `ACTORS.get(cls)?.deferred`.
fn world_deferred(w: &World, cls: Name) -> Option<Deferred> {
    let reg: &'static Registry = w.registry;
    match reg.actors.get(w.names.str(cls))?.hooks {
        ActorHooks::World(h) => h.deferred,
        _ => None,
    }
}

fn state(w: &mut World, it: ItemId) -> &mut HelperItem {
    match &mut w.actors.items.get_mut(it).expect("a live item").ext {
        ItemExt::HelperItem(h) => h,
        _ => unreachable!("Item_Helper_C holds a HelperItem"),
    }
}

/// `thrown`: OnDestroyed bound to SpawnOnThrown.
fn thrown(w: &mut World, it: ItemId) {
    state(w, it).spawn_on_destroyed = true;
}

/// `activate`: the helper, then the parent's ActivateObject unless the TV is gone.
fn activate(w: &mut World, it: ItemId, _f: FighterId) {
    spawn_helper(w, it);
    if w.actors.items.get(it).is_some_and(|i| !i.dead) {
        item_activate_parent(w, it);
    }
}

/// `end`: OnDestroyed -> SpawnOnThrown, after ReceiveEndPlay.
fn end(w: &mut World, it: ItemId) {
    if state(w, it).spawn_on_destroyed && !reset_pass(w) {
        spawn_helper(w, it);
    }
}

fn sub() -> ItemExt {
    ItemExt::HelperItem(HelperItem::default())
}

/// This file's registry entries (Item_Helper_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ItemHooks {
        sub: Some(sub),
        thrown: Some(thrown),
        activate: Some(activate),
        end: Some(end),
        ..ItemHooks::default()
    };
    r.actors.enter(
        ItemClass::ItemHelper.name(),
        ActorEntry {
            ownerless: true,
            ..ActorEntry::new(ActorClass::Item(ItemClass::ItemHelper), ActorHooks::Item(hooks))
        },
    )
}
