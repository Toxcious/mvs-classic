//! Jerry's champagne cork as the record writes it.

use mvs_sim::chars::tom_and_jerry::champagne::JerryCork;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! cork_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "aimDown" = aim_down, "bounceLeft" = bounce_left, "popTick" = pop_tick, "popped" = popped,
            "reflectFx" = reflect_fx, "reflected" = reflected, "tom" = tom, "trail" = trail,
        )
    };
}

/// A JerryCork_C's own fields as the record writes them.
pub fn cork_put(s: &JerryCork, x: &Wx, o: &mut Vec<(String, Value)>) {
    cork_keys!(plain_keys put s, x, o;);
}

/// [`cork_put`]'s inverse; the keys it knows go into `known`.
pub fn cork_load(s: &mut JerryCork, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    cork_keys!(plain_keys load s, ob, l;);
    known.extend(cork_keys!(plain_keys names));
}
