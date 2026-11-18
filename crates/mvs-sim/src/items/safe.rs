//! Item_Safe_C: an ItemActorParent child; CanUseItem false: ActivateObject is the
//! parent's drop.
//! ReceiveBeginPlay @1869: the parent's, then the Visual's Damage scalar 0 (the view's).
//! Thrown @1930: the parent's, then SetActorEnableCollision(false) @1949 and Capsule.SetVisibility(false, true) @1960;
//! with authority and the owner's Comp_Spawner valid, CompSpawnActor(BugsBunnySafe_C, not attached, owner, world
//! transform at the item's location, direction (0, 0, 0), speed 0, max 0) @2270 (`chars::bugs::safe`), then Delay(0)
//! @2404 -> @15: the owner's Get Spawned Actor(BugsBunnySafe_C, last)[0] valid and a BugsBunnySafe @86-357 ->
//! Capsule.SetSimulatePhysics(true) @367, a Struct_HitBoxDamage {Damage 0, BaseKnockBack VSize(ProjectileMovement
//! .Velocity) @426, KnockBackDirectionVector (Y, Z) of Normal(Velocity, 1e-4) @485-595, the rest neutral} and the
//! safe's OnHitByAlly(that packet, no hit, owner) @1772, then ReleaseToPool @1854. An invalid lookup pops the flow:
//! the item flies on with no collision until its own end.
//! Ported for a spawn with no owner: the class reads the owner from its Thrown on.

use crate::State;
use crate::actors::item::ItemExt;
use crate::actors::{ActorClass, ActorRef, ItemClass, ItemId};
use crate::combat::hits::hit_by;
use crate::fighter::common::latent_frames;
use crate::math::to_int32;
use crate::name;
use crate::registry::entries::ItemHooks;
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::spawning::{comp_spawn_projectile, last_spawned_projectile};

/// `SafeItem extends Item` (Item_Safe_C).
#[derive(State, Debug)]
pub struct SafeItem {
    /// Capsule.SetVisibility(false, true) @1960: the item and every child of its root hidden.
    pub capsule_hidden: bool,
    /// The `World::flow.tick` its Delay(0) @2404 fires at (-1: none).
    pub knock_at: i32,
}

impl Default for SafeItem {
    fn default() -> Self {
        SafeItem { capsule_hidden: false, knock_at: -1 }
    }
}

fn state(w: &mut World, it: ItemId) -> &mut SafeItem {
    match &mut w.actors.items.get_mut(it).expect("a live item").ext {
        ItemExt::SafeItem(s) => s,
        _ => unreachable!("Item_Safe_C holds a SafeItem"),
    }
}

/// `thrown`.
fn thrown(w: &mut World, it: ItemId) {
    let (x, y, owner) = {
        let i = w.actors.items.get_mut(it).expect("a live item");
        i.no_collision = Some(true);
        (i.x, i.y, i.owner.expect("a thrown Item_Safe_C has its owner"))
    };
    state(w, it).capsule_hidden = true;
    comp_spawn_projectile(w, owner, name!("BugsBunnySafe_C"), x, y, [Some(0.0); 3], 0.0, 0.0, None);
    // The Delay(0), created in the thrower's tick: its first update is the item's own tick of this frame.
    let at = w.flow.tick + to_int32(latent_frames(0.0)) - 1;
    state(w, it).knock_at = at;
}

/// `step`: the Delay(0)'s end (ReleaseToPool @1854); true destroys the item.
fn step(w: &mut World, it: ItemId) -> bool {
    let at = state(w, it).knock_at;
    if at < 0 || w.flow.tick < at {
        return false;
    }
    state(w, it).knock_at = -1;
    let (vx, vy, owner) = {
        let i = w.actors.items.get(it).expect("a live item");
        (i.vx, i.vy, i.owner.expect("a thrown Item_Safe_C has its owner"))
    };
    let Some(safe) = last_spawned_projectile(w, owner, name!("BugsBunnySafe_C")) else { return false };
    // VSize and Normal(Velocity, 1e-4) of the flight. The packet's X is the velocity's UE Y, which the safe's knock
    // puts back on UE Y: the knock goes along the flight, (dx, dy) in the world's sense.
    let vs = crate::math::ue::size2d([vx, vy]);
    let n = crate::math::ue::normal([0.0, vx, vy], crate::math::ue::KINDA_SMALL_NUMBER);
    // Deviation: SetSimulatePhysics(true) @367 is the knock's own (`chars::bugs::safe`); a knock its
    // BufferForHitboxActivate gate or the hitstun test refuses leaves the safe on its ProjectileMovement.
    hit_by(w, safe, true, n[1], n[2], vs, -1, Some(ActorRef::Fighter(owner)));
    true
}

fn sub() -> ItemExt {
    ItemExt::SafeItem(SafeItem::default())
}

/// This file's registry entries (Item_Safe_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ItemHooks { sub: Some(sub), thrown: Some(thrown), step: Some(step), ..ItemHooks::default() };
    r.actors.enter(
        ItemClass::ItemSafe.name(),
        ActorEntry {
            ownerless: true,
            ..ActorEntry::new(ActorClass::Item(ItemClass::ItemSafe), ActorHooks::Item(hooks))
        },
    )
}
