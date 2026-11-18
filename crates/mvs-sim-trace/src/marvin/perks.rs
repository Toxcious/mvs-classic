//! Marvin's perk buffs as the record writes them.

use mvs_sim::chars::marvin::perks::{PerkUfoVars, SpeedExt};
use mvs_trace::Value;

use crate::conv::{Plain, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(PerkUfoVars {
    "aimActors" = aim_actors, "homingTarget" = homing_target, "bound" = bound, "graceAt" = grace_at, "aimAt" = aim_at,
    "chargeAt" = charge_at,
});

plain_struct!(SpeedExt { "active" = active, "bound" = bound });

/// Buff_C018_PerkProjectileSpeed_C's extension as the oracle writes it (`{ active, bound }`; `ElemBuff.ext`,
/// `actors::elem_ext_put`).
pub fn speed_put(e: &SpeedExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`speed_put`]'s inverse (`actors::elem_ext_load`, by the buff's class).
pub fn speed_load(v: &Value, l: &mut Lx) -> SpeedExt {
    let mut e = SpeedExt::default();
    e.load_value(v, l);
    e
}
