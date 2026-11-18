//! Buff_Charm's extension as the record writes it.

use mvs_sim::chars::bugs::charm::CharmExt;
use mvs_trace::Value;

use crate::conv::{Plain, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(CharmExt { "by" = by, "dur" = dur, "air" = air });

/// Buff_Charm_C's extension as the oracle writes it: `{ by, dur, air }`.
pub fn charm_put(e: &CharmExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`charm_put`]'s inverse.
pub fn charm_load(e: &mut CharmExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
