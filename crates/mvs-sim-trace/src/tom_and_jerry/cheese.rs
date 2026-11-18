//! Buff_Cheese's extension as the oracle writes it (`{ delay, bound }`).

use mvs_sim::chars::tom_and_jerry::cheese::CheeseExt;
use mvs_trace::Value;

use crate::conv::{Plain, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(CheeseExt { "bound" = bound, "delay" = delay });

/// Buff_Cheese_C's extension as the oracle writes it.
pub fn cheese_put(e: &CheeseExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`cheese_put`]'s inverse.
pub fn cheese_load(e: &mut CheeseExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
