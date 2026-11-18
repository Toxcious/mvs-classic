//! Bugs Bunny's perks' projectiles as the record writes them.

use mvs_sim::chars::bugs::perks::{BugsShockwave, StillActor};
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// A LingeringLoveProjectile_C as the record writes it: no key beyond the projectile's.
pub fn still_put(_s: &StillActor, _x: &Wx, _o: &mut Vec<(String, Value)>) {}

/// [`still_put`]'s inverse: nothing to read.
pub fn still_load(_s: &mut StillActor, _ob: &Obj, _l: &mut Lx, _known: &mut Vec<&'static str>) {}

/// A TunnelShockwave_C as the record writes it: `left`.
pub fn shockwave_put(s: &BugsShockwave, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "left" = left,);
}

/// [`shockwave_put`]'s inverse; the keys it knows go into `known`.
pub fn shockwave_load(s: &mut BugsShockwave, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "left" = left,);
    known.extend(plain_keys!(names "left" = left,));
}
