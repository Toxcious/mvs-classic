//! Buff_HeldByPlasmaBall's extension as the record writes it.

use mvs_sim::chars::iron_giant::plasma::BallHold;
use mvs_trace::Value;

use crate::conv::{Plain, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(BallHold {
    "shot" = shot, "stage" = stage, "wait" = wait, "tries" = tries, "snap" = snap, "ended" = ended,
});

/// Buff_HeldByPlasmaBall's extension as the oracle writes it.
pub fn ball_put(e: &BallHold, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`ball_put`]'s inverse.
pub fn ball_load(e: &mut BallHold, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
