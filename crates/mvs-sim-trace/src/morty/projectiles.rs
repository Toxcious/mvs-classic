//! Morty's cook grenade as the record writes it (a projectile's `ext`).

use mvs_sim::chars::morty::projectiles::CookGrenade;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// The keys the cook grenade adds to its projectile's `rest`: `ignorePlatformsCheck` always, `useCookDuration` and
/// `useMaxRadius` once the Cook perk set them.
pub fn grenade_put(p: &CookGrenade, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put p, x, o;
        "ignorePlatformsCheck" = ignore_platforms_check, "useCookDuration" ~ use_cook_duration,
        "useMaxRadius" ~ use_max_radius,);
}

/// [`grenade_put`]'s inverse.
pub fn grenade_load(p: &mut CookGrenade, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load p, ob, l;
        "ignorePlatformsCheck" = ignore_platforms_check, "useCookDuration" ~ use_cook_duration,
        "useMaxRadius" ~ use_max_radius,);
    known.extend(plain_keys!(names
        "ignorePlatformsCheck" = ignore_platforms_check, "useCookDuration" ~ use_cook_duration,
        "useMaxRadius" ~ use_max_radius,));
}
