//! Velma's mark as the record writes it: the keys VelmaMarkProjectile_C, VelmaAimProjectile_C
//! and VelmamSassFollowUpProjectile_C add to their projectile's `rest`.

use mvs_sim::chars::velma::mark::{AimProjectile, FollowUp, Mark};
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! mark_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "conds" = conds, "homeMag" = home_mag, "stuck" = stuck, "target" = target, "toAlly" = to_ally,
            "toEnemy" = to_enemy, "ground" = ground, "gateDone" = gate_done, "warning" = warning,
            "lifeAt" = life_at, "dieAt" = die_at,
        )
    };
}

macro_rules! aim_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "homeMag" = home_mag, "aimAt" = aim_at, "bigHit" = big_hit, "hitOnce" = hit_once,
            "destroyAt" = destroy_at, "reflectAt" = reflect_at, "reflected" = reflected, "lifeAt" = life_at,
            "inside" = inside,
        )
    };
}

macro_rules! follow_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "homeMag" = home_mag, "searchAt" = search_at, "dieAt" = die_at, "loop" = loop_on,
            "minDist" = min_dist,
        )
    };
}

/// A VelmaMarkProjectile_C's own fields as the record writes them.
pub fn mark_put(s: &Mark, x: &Wx, o: &mut Vec<(String, Value)>) {
    mark_keys!(plain_keys put s, x, o;);
}

/// [`mark_put`]'s inverse; the keys it knows go into `known`.
pub fn mark_load(s: &mut Mark, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    mark_keys!(plain_keys load s, ob, l;);
    known.extend(mark_keys!(plain_keys names));
}

/// A VelmaAimProjectile_C's own fields as the record writes them.
pub fn aim_put(s: &AimProjectile, x: &Wx, o: &mut Vec<(String, Value)>) {
    aim_keys!(plain_keys put s, x, o;);
}

/// [`aim_put`]'s inverse; the keys it knows go into `known`.
pub fn aim_load(s: &mut AimProjectile, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    aim_keys!(plain_keys load s, ob, l;);
    known.extend(aim_keys!(plain_keys names));
}

/// A VelmamSassFollowUpProjectile_C's own fields as the record writes them.
pub fn follow_put(s: &FollowUp, x: &Wx, o: &mut Vec<(String, Value)>) {
    follow_keys!(plain_keys put s, x, o;);
}

/// [`follow_put`]'s inverse; the keys it knows go into `known`.
pub fn follow_load(s: &mut FollowUp, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    follow_keys!(plain_keys load s, ob, l;);
    known.extend(follow_keys!(plain_keys names));
}
