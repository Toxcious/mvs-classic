//! His projectiles as the record writes them: C015StewSpitAir_C.

use mvs_sim::chars::taz::projectiles::StewSpitAir;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// a C015StewSpitAir_C as the record writes it.
pub fn spit_air_put(s: &StewSpitAir, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "reverseAt" = reverse_at, "dealerHalf" = dealer_half, "dealerR" = dealer_r, "stewVx" = stew_vx, "stewVy" = stew_vy,);
}

/// [`spit_air_put`]'s inverse; the keys it knows go into `known`.
pub fn spit_air_load(s: &mut StewSpitAir, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "reverseAt" = reverse_at, "dealerHalf" = dealer_half, "dealerR" = dealer_r, "stewVx" = stew_vx, "stewVy" = stew_vy,);
    known.extend(plain_keys!(names "reverseAt" = reverse_at, "dealerHalf" = dealer_half, "dealerR" = dealer_r, "stewVx" = stew_vx, "stewVy" = stew_vy,));
}
