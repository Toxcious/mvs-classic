//! Gizmo's arrow and bow slow as the record writes them.

use mvs_sim::chars::gizmo::bow::{Arrow, BowSlowExt};
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(BowSlowExt { "adjusted" = adjusted });

/// A C023A_Arrow_Projectile_C as the record writes it: the keys its class adds to the projectile.
pub fn arrow_put(s: &Arrow, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "fire" = fire, "useChargeGate" = use_charge_gate, "closed" = closed,);
}

/// [`arrow_put`]'s inverse; the keys it knows go into `known`.
pub fn arrow_load(s: &mut Arrow, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "fire" = fire, "useChargeGate" = use_charge_gate, "closed" = closed,);
    known.extend(plain_keys!(names "fire" = fire, "useChargeGate" = use_charge_gate, "closed" = closed,));
}

/// C023A_BowSlow_C's extension as the oracle writes it.
pub fn bow_slow_put(e: &BowSlowExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`bow_slow_put`]'s inverse.
pub fn bow_slow_load(e: &mut BowSlowExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
