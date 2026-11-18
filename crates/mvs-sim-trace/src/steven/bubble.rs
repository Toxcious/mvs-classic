//! The bubble charge as the record writes it: Steven_BubbleCharge_C.

use mvs_sim::chars::steven::bubble::BubbleCharge;
use mvs_trace::Obj;
use mvs_trace::Value;

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! charge_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)* "growth" = growth, "onAge" = on_age, "unit" = unit,)
    };
}

/// a Steven_BubbleCharge_C as the record writes it: the keys it adds to its projectile's `rest`.
pub fn charge_put(s: &BubbleCharge, x: &Wx, o: &mut Vec<(String, Value)>) {
    charge_keys!(plain_keys put s, x, o;);
}

/// [`charge_put`]'s inverse; the keys it knows go into `known`.
pub fn charge_load(s: &mut BubbleCharge, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    charge_keys!(plain_keys load s, ob, l;);
    known.extend(charge_keys!(plain_keys names));
}
