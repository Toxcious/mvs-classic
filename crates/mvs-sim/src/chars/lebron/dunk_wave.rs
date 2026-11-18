//! C016_DunkShockwave_C: Dunk1's AnimNotify_ConditionalSpawner at 50.265 behind
//! C016TeleportDunkActiveCondition (IsTeleportDunking, set by DashToBasketball's completion), socket Root, not
//! attached; it spawns whether the slam hit or not. A data child of ShaggyShockwaveActor_C (its supers
//! `ShaggyShockwaveActor_C SpawnedActorParent_C Actor`; C016_DunkShockwave holds the class, its CDO and two
//! component overrides, no function). The parent's graph is the whole class, so the shared shockwave kind serves it:
//! the Shockwave takes its class from the request and every value from this class's own actor data (HitDamage
//! BaseKnockBack 1400, scaling 14, direction (200, 900); Shockwave0 scale 4.5; ShockwaveDefault X -150 scale 7; the
//! rest the parent's). Its hits are the wave's own handler's: LeBron's IgnitedDunk does not ignite them.

use crate::actors::classes::WorldActorClass;
use crate::chars::shaggy::shockwave;
use crate::registry::entries::ActorEntry;
use crate::registry::{Registry, RegistryError};

/// This file's registry entry. `parents`: Comp_Spawner's FilterArray(ShaggyShockwaveActor_C) finds the child.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        "C016_DunkShockwave_C",
        ActorEntry { parents: &["ShaggyShockwaveActor_C"], ..shockwave::entry(WorldActorClass::C016DunkShockwave) },
    )
}
