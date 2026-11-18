//! Stripe's gun shot as the record writes it: the shot adds no key of its own.

use mvs_sim::chars::stripe::gun::GunShot;
use mvs_trace::{Obj, Value};

use crate::cx::{Lx, Wx};

/// A C023B_SpawnedGunShot_C as the record writes it: nothing past the projectile's keys.
pub fn gun_shot_put(s: &GunShot, x: &Wx, o: &mut Vec<(String, Value)>) {
    let _ = (s, x, o);
}

/// [`gun_shot_put`]'s inverse; the keys it knows go into `known`.
pub fn gun_shot_load(s: &mut GunShot, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    let _ = (s, ob, l, known);
}
