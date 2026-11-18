//! Buff_Jake_WeaponMaster's extension as the record writes it.

use mvs_sim::chars::jake::weapons::MasterExt;
use mvs_trace::Value;

use crate::conv::{Plain, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(MasterExt { "bound" = bound, "at" = at, "active" = active });

/// Buff_Jake_WeaponMaster_C's extension as the oracle writes it: `{ bound, at, active }`.
pub fn master_put(e: &MasterExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`master_put`]'s inverse.
pub fn master_load(e: &mut MasterExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
