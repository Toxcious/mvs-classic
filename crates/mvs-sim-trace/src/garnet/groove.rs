//! Buff_Garnet_ElectricGroove's extension as the record writes it.

use mvs_sim::chars::garnet::groove::Groove;
use mvs_trace::Value;

use crate::conv::{Plain, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(Groove { "from" = from });

/// Buff_Garnet_ElectricGroove_C's extension as the oracle writes it (`{ from }`).
pub fn groove_put(e: &Groove, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`groove_put`]'s inverse.
pub fn groove_load(e: &mut Groove, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
