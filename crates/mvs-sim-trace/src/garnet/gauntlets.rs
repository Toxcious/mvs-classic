//! The side special's projectiles as the record writes them.

use mvs_sim::chars::garnet::gauntlets::GarnetGauntlets;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// a GarnetGauntletsProjectile as the record writes it.
pub fn gauntlets_put(s: &GarnetGauntlets, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "endRan" = end_ran, "kbAt" = kb_at, "kbBound" = kb_bound,);
}

/// [`gauntlets_put`]'s inverse; the keys it knows go into `known`.
pub fn gauntlets_load(s: &mut GarnetGauntlets, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "endRan" = end_ran, "kbAt" = kb_at, "kbBound" = kb_bound,);
    known.extend(plain_keys!(names "endRan" = end_ran, "kbAt" = kb_at, "kbBound" = kb_bound,));
}
