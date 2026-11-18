//! BPI_ReverseMoveDirection on a spawned actor (a graph's interface call:
//! Buff_GarnetStopProjectile, C018's and C023B's own graphs): the implementation of the first class of the actor's
//! chain that has one.
//!
//! The build's implementers (every class with a BPI_ReverseMoveDirection function):
//! - ParentProjectilePhysics @0 -> ubergraph @1975 -> @820-948: ProjectileMovement.IsActive(), then
//!   ProjectileMovement.Velocity = Velocity x (1, -1, 1): the lateral velocity (sim x, UE Y) flipped.
//! - ItemActorParent @0 -> @635-759: the same on its ProjectileMovement.
//! - SpawnedActorParent @0 -> @4500: pop_flow, nothing.
//! - C019_CookGrenade (a SpawnedActorParent child) @0 -> @6501 -> @5737-5789 -> @4873-4945: IsActive, the same flip.
//! - Meeseeks_SpawnedActor (a ParentProjectilePhysics child) @0 -> @1113-1142: SetFacing(!FacingLeft), no parent
//!   call.
//! - C018CannonBlast (and C018PerkCannonBlast, its child): `chars::marvin::ufo`'s entry's `reverse_move`.
//! - C023B_SawBlade_EdgeTrace.
//!
//! An override does not call its parent's. A class of the chain below the implementer runs the implementer's: a
//! data child (C019_CookGrenade_NoGrav) the grenade's. The class's own code is its entry's `reverse_move`, under the
//! implementer's name; an implementer that is a character's class with no entry warns, never the parent's rule. A
//! class with no implementer in its chain takes the call as nothing (an interface call on an object that does not
//! implement it).

use crate::actors::ActorRef;
use crate::actors::world_actor::{actor_cls, actor_owner, warn};
use crate::fighter::common::actor_class_def;
use crate::registry::entries::ActorHooks;
use crate::world::World;

const OWN: [&str; 4] =
    ["C019_CookGrenade_C", "Meeseeks_SpawnedActor_C", "C018CannonBlast_C", "C023B_SawBlade_EdgeTrace_C"];
const PARENT_PHYSICS: &str = "ParentProjectilePhysics_C";
const ITEM_PARENT: &str = "ItemActorParent_C";
const SPAWNED_PARENT: &str = "SpawnedActorParent_C";

/// `flipActiveVelocity`: the parents' rule, ProjectileMovement.IsActive() and
/// Velocity.Y flipped. Only a projectile or an item has the movement.
pub fn flip_active_velocity(w: &mut World, a: ActorRef) {
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id)
                && crate::actors::projectile::movement_active(p)
            {
                p.vx = -p.vx;
            }
        }
        ActorRef::Item(id) => {
            // movementActive: a class's own override (Bugs's safe, Garnet's marker, Marvin's flag, Steven's
            // watermelon) is the item's.
            if w.actors.items.get(id).is_some()
                && crate::actors::item::item_movement_active(w, id)
                && let Some(p) = w.actors.items.get_mut(id)
            {
                p.vx = -p.vx;
            }
        }
        _ => {}
    }
}

/// `reverseMoveDirection`.
pub fn reverse_move_direction(w: &mut World, a: ActorRef) {
    let Some(cls) = actor_cls(w, a) else { return };
    // (the chain is read from the owner's data: an actor with no owner has none to read, no other fighter's
    // stands in)
    let chain: Option<Vec<String>> = actor_owner(w, a)
        .and_then(|own| actor_class_def(w, own, cls))
        .and_then(|d| d.supers.as_deref())
        .map(|s| s.split(' ').map(str::to_owned).collect());
    let Some(supers) = chain else {
        warn("BPI_ReverseMoveDirection: no class chain in the data: not reversed");
        return;
    };
    let me = w.names.str(cls).to_owned();
    for c in std::iter::once(me.as_str()).chain(supers.iter().map(String::as_str)) {
        if OWN.contains(&c) {
            let r: &'static crate::registry::Registry = w.registry;
            let hooks = r.actors.get(c).map(|e| e.hooks);
            match (hooks, a) {
                (Some(ActorHooks::World(h)), ActorRef::Actor(id)) if h.reverse_move.is_some() => {
                    h.reverse_move.expect("checked")(w, id);
                }
                (Some(ActorHooks::Projectile(h)), ActorRef::Projectile(id)) if h.reverse_move.is_some() => {
                    h.reverse_move.expect("checked")(w, id);
                }
                _ => warn("BPI_ReverseMoveDirection: the class's own implementation is not ported"),
            }
            return;
        }
        if c == PARENT_PHYSICS || c == ITEM_PARENT {
            if matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_)) {
                flip_active_velocity(w, a);
            } else {
                warn("BPI_ReverseMoveDirection: a child with no movement component in the port: not reversed");
            }
            return;
        }
        if c == SPAWNED_PARENT {
            return;
        }
    }
}
