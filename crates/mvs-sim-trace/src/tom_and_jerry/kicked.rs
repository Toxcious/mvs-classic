//! Tom and Jerry's kicked and aimed rockets as the record writes them: the keys each
//! class adds to a projectile.

use mvs_sim::chars::tom_and_jerry::kicked::{AimRocket, KickedRocket};
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! kicked_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "spawnOwner" = spawn_owner, "shouldSpawnJerry" = should_spawn_jerry, "facingLeft" = facing_left,
            "reflected" = reflected, "blockingActor" = blocking_actor, "arcPos" = arc_pos, "arcOn" = arc_on,
            "arcResume" = arc_resume, "lifeSpanAt" = life_span_at, "exploded" = exploded,
        )
    };
}

macro_rules! aim_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "jerry" = jerry, "homeTarget" = home_target, "homeOn" = home_on, "homeMag" = home_mag,
            "aimDown" = aim_down, "reflected" = reflected, "bigHit" = big_hit, "varHit" = var_hit,
            "handlerHit" = handler_hit, "noJerry" = no_jerry, "reflectSound" = reflect_sound,
            "hitActor" = hit_actor, "blockingActor" = blocking_actor, "overJerry" = over_jerry,
            "lifeSpanAt" = life_span_at, "exploded" = exploded,
        )
    };
}

/// A JerryKickedRocket_C's own keys.
pub fn kicked_put(s: &KickedRocket, x: &Wx, o: &mut Vec<(String, Value)>) {
    kicked_keys!(plain_keys put s, x, o;);
}

/// [`kicked_put`]'s inverse; the keys it knows go into `known`.
pub fn kicked_load(s: &mut KickedRocket, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    kicked_keys!(plain_keys load s, ob, l;);
    known.extend(kicked_keys!(plain_keys names));
}

/// A TomAimRocket1_C's own keys.
pub fn aim_rocket_put(s: &AimRocket, x: &Wx, o: &mut Vec<(String, Value)>) {
    aim_keys!(plain_keys put s, x, o;);
}

/// [`aim_rocket_put`]'s inverse; the keys it knows go into `known`.
pub fn aim_rocket_load(s: &mut AimRocket, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    aim_keys!(plain_keys load s, ob, l;);
    known.extend(aim_keys!(plain_keys names));
}
