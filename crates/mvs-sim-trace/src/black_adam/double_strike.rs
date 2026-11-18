//! `DoubleStrike` (C021_DoubleStrikeProjectile_C, a projectile's `ext`): the keys
//! a double strike adds to its projectile's `rest`.

use mvs_sim::chars::black_adam::double_strike::DoubleStrike;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// The keys a double strike adds to its projectile's `rest`.
pub fn put(p: &DoubleStrike, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put p, x, o; "hitActor" = hit_actor, "exploded" = exploded,);
}

/// [`put`]'s inverse.
pub fn load(p: &mut DoubleStrike, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load p, ob, l; "hitActor" = hit_actor, "exploded" = exploded,);
    known.extend(plain_keys!(names "hitActor" = hit_actor, "exploded" = exploded,));
}
