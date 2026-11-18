//! Wonder Woman's shields (the WW_ShieldActor_C and WW_ShieldActor_U_Special_C entries,
//! the shared `shield` kind of `actors::shield_actor`) and the condition WWShieldBlockedProjectile (the first live
//! WW_ShieldActor of her spawner's ProjectileBlocked).

use crate::actors::shield_actor::{self, shield_of};
use crate::actors::{ActorRef, WorldActorClass};
use crate::fighter::FighterId;
use crate::registry::{Condition, Registry, RegistryError};
use crate::world::World;

/// WWShieldBlockedProjectile Check @51-520: the first WW_ShieldActor (a cast: the up special's child class and the
/// evade shield too) among the owner's Comp_Spawner.SpawnedActors gives its ProjectileBlocked @474; without one false
/// @520.
fn blocked(w: &mut World, f: FighterId) -> bool {
    w.fighter(f)
        .items
        .spawned_actors
        .iter()
        .find_map(|a| match *a {
            ActorRef::Actor(id) => w.actors.actors.get(id).filter(|a| !a.dead).and_then(shield_of),
            _ => None,
        })
        .is_some_and(|s| s.projectile_blocked)
}

/// This file's registry entries (the two shield classes, WWShieldBlockedProjectile).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors
        .enter("WW_ShieldActor_C", shield_actor::entry(WorldActorClass::WWShieldActor, &["SpawnedPlatformParent_C"]))?;
    r.actors.enter(
        "WW_ShieldActor_U_Special_C",
        shield_actor::entry(WorldActorClass::WWShieldActorUSpecial, &["WW_ShieldActor_C", "SpawnedPlatformParent_C"]),
    )?;
    r.conditions.enter("WWShieldBlockedProjectile_C", Condition::Fn(blocked))
}
