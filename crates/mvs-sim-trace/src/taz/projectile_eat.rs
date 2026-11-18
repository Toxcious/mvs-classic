//! Buff_C015HitProjectile's extension on a spawned actor as the record writes it.

use mvs_sim::chars::taz::projectile_eat::HitProjExt;
use mvs_trace::Value;

use crate::conv::{Plain, plain_struct};
use crate::cx::{Lx, Wx};

// (each key written where the oracle set it: `{}` before the begin's, `hurt` / `life` the tunnel carry's)
plain_struct!(HitProjExt {
    "act" ~ act,
    "fl" ~ fl,
    "hurt" ~ carry.hurt,
    "inst" ~ inst,
    "life" ~ carry.life,
    "rate" ~ rate,
    "sx" ~ sx,
    "t" ~ t,
});

/// Buff_C015HitProjectile_C's extension as the oracle writes it (`ElemBuff.ext`, `actors::elem_ext_put`).
pub fn hit_projectile_put(e: &HitProjExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`hit_projectile_put`]'s inverse (`actors::elem_ext_load`, by the buff's class).
pub fn hit_projectile_load(v: &Value, l: &mut Lx) -> HitProjExt {
    let mut e = HitProjExt::default();
    e.load_value(v, l);
    e
}
