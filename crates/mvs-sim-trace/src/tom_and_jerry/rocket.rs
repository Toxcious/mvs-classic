//! Jerry's rocket as the record writes it: the keys the class adds to a projectile.

use mvs_sim::chars::tom_and_jerry::rocket::JerryRocket;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! rocket_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "spawnOwner" = spawn_owner, "shouldSpawnJerry" = should_spawn_jerry, "jerrySpawned" = jerry_spawned,
            "accelGate" = accel_gate, "hitSelfGate" = hit_self_gate, "overlapOn" = overlap_on,
            "ownerOver" = owner_over, "selfHitDone" = self_hit_done, "hasBeenReflected" = has_been_reflected,
            "hitActor" = hit_actor, "blockingActor" = blocking_actor, "endAge" = end_age,
            "endPlayed" = end_played, "boxDamage" = box_damage,
        )
    };
}

/// A JerryRocket_C's own keys.
pub fn rocket_put(s: &JerryRocket, x: &Wx, o: &mut Vec<(String, Value)>) {
    rocket_keys!(plain_keys put s, x, o;);
}

/// [`rocket_put`]'s inverse; the keys it knows go into `known`.
pub fn rocket_load(s: &mut JerryRocket, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    rocket_keys!(plain_keys load s, ob, l;);
    known.extend(rocket_keys!(plain_keys names));
}
