//! Buff_GarnetStackingSinging's extension as the record writes it.

use mvs_sim::chars::garnet::sing::SingExt;
use mvs_trace::Value;

use crate::conv::{Plain, plain_struct};
use crate::cx::{Lx, Wx};

// (`singExt`: every key from the first read)
plain_struct!(SingExt {
    "active" = active,
    "armorBreaker" = armor_breaker,
    "boosted" = boosted,
    "inc" = inc,
    "paused" = paused,
    "pending" = pending,
    "prev" = prev,
});

/// Buff_GarnetStackingSinging_C's extension as the oracle writes it.
pub fn singing_put(e: &SingExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`singing_put`]'s inverse.
pub fn singing_load(e: &mut SingExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
