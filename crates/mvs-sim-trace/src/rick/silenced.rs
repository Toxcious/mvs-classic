//! C020_Buff_Silenced's extensions as the record writes them.

use mvs_sim::chars::rick::{SilencedActorExt, SilencedExt};
use mvs_trace::Value;

use crate::conv::{Plain, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(SilencedExt { "by" = by, "left" = left });
plain_struct!(SilencedActorExt { "inst" = inst, "left" = left });

/// C020_Buff_Silenced_C's extension as the oracle writes it.
pub fn silenced_put(e: &SilencedExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`silenced_put`]'s inverse.
pub fn silenced_load(e: &mut SilencedExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}

/// C020_Buff_Silenced_C's extension on a spawned actor as the oracle writes it (`ElemBuff.ext`,
/// `actors::elem_ext_put`).
pub fn silenced_actor_put(e: &SilencedActorExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`silenced_actor_put`]'s inverse (`actors::elem_ext_load`, by the buff's class).
pub fn silenced_actor_load(v: &Value, l: &mut Lx) -> SilencedActorExt {
    let mut e = SilencedActorExt::default();
    e.load_value(v, l);
    e
}
