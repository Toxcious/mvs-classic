//! Buff_Jake_SafetyNet's and Buff_AirSuperiority's extensions as the record writes them.

use mvs_sim::chars::jake::perks::{AirExt, AllyStamp, SafetyNetExt};
use mvs_sim::fighter::FighterId;
use mvs_trace::Value;

use crate::conv::{Plain, as_obj, plain_struct};
use crate::cx::{Lx, Wx};

// An Allies entry (`{ id, t }`).
plain_struct!(AllyStamp { "id" = id, "t" = t } make |v, l| {
    let ob = as_obj(v, l)?;
    let id = FighterId::from_value(ob.get("id")?, l)?;
    let mut s = AllyStamp { id, t: 0.0 };
    s.load_value(v, l);
    Some(s)
});
plain_struct!(SafetyNetExt { "allies" = allies });
plain_struct!(AirExt { "at" = at });

/// Buff_Jake_SafetyNet_C's extension as the oracle writes it: `{ allies: { id, t }[] }`.
pub fn safety_net_put(e: &SafetyNetExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`safety_net_put`]'s inverse.
pub fn safety_net_load(e: &mut SafetyNetExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}

/// Buff_AirSuperiority_C's extension as the oracle writes it: `{ at }`.
pub fn air_put(e: &AirExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`air_put`]'s inverse.
pub fn air_load(e: &mut AirExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
