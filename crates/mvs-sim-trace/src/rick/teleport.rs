//! Rick's teleport buffs' extensions as the record writes them.

use mvs_sim::chars::rick::teleport::{PortalBuffExt, TeleportExt};
use mvs_trace::Value;

use crate::conv::{Plain, plain_struct};
use crate::cx::{Lx, Wx};

// (`TeleportExt`: every key from the begin)
plain_struct!(TeleportExt {
    "alpha" = alpha,
    "by" = by,
    "clamp" = clamp,
    "dx" = dx,
    "dy" = dy,
    "end" = end,
    "ended" = ended,
    "kb" = kb,
    "left" = left,
    "moving" = moving,
    "rDelay" = r_delay,
    "rExit" = r_exit,
    "rMain" = r_main,
    "start" = start,
    "sx" = sx,
    "sy" = sy,
    "time" = time,
    "tl" = tl,
    "visible" = visible,
    "vx" = vx,
    "vy" = vy,
});

plain_struct!(PortalBuffExt { "left" = left });

/// Buff_Teleporting3_C's extension as the oracle writes it.
pub fn teleporting_put(e: &TeleportExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`teleporting_put`]'s inverse.
pub fn teleporting_load(e: &mut TeleportExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}

/// Buff_PortalBuff_C's extension as the oracle writes it.
pub fn portal_buff_put(e: &PortalBuffExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`portal_buff_put`]'s inverse.
pub fn portal_buff_load(e: &mut PortalBuffExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
