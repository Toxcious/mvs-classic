//! Bugs Bunny's safe as the record writes it: the keys the classes add to a projectile.

use mvs_sim::chars::bugs::safe::{BugsBunnySafe, SafeDrop};
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! safe_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "origInst" = orig_inst, "hp" = hp, "maxHp" = max_hp, "boxOn" = box_on, "safeHits" = safe_hits,
            "inside" = inside, "lastHit" = last_hit, "landed" = landed, "landDist" = land_dist,
            "landAt" = land_at, "landedAge" = landed_age, "sim" = sim, "pmActive" = pm_active, "bvx" = bvx,
            "bvy" = bvy, "bspin" = bspin, "facingLeft" = facing_left, "dying" = dying, "deathAt" = death_at,
            "blockOpen" = block_open, "blockAt" = block_at, "reflectAt" = reflect_at,
            "reflectIdleAt" = reflect_idle_at, "shakeAge" = shake_age, "loot" = loot,
        )
    };
}

macro_rules! drop_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)* "strike" = strike, "dest" = dest, "endAt" = end_at, "struck" = struck)
    };
}

/// A BugsBunnySafe_C's own keys.
pub fn safe_put(s: &BugsBunnySafe, x: &Wx, o: &mut Vec<(String, Value)>) {
    safe_keys!(plain_keys put s, x, o;);
}

/// [`safe_put`]'s inverse; the keys it knows go into `known`.
pub fn safe_load(s: &mut BugsBunnySafe, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    safe_keys!(plain_keys load s, ob, l;);
    known.extend(safe_keys!(plain_keys names));
}

/// A BugsBunny_SafeDrop_C's own keys (`activated` is the projectile's).
pub fn drop_put(s: &SafeDrop, x: &Wx, o: &mut Vec<(String, Value)>) {
    drop_keys!(plain_keys put s, x, o;);
}

/// [`drop_put`]'s inverse; the keys it knows go into `known`.
pub fn drop_load(s: &mut SafeDrop, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    drop_keys!(plain_keys load s, ob, l;);
    known.extend(drop_keys!(plain_keys names));
}
