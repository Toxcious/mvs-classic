//! Finn's coins as the record writes them.

use mvs_sim::chars::finn::coins::FinnTreasure;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! treasure_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "isTargeted" = is_targeted, "fade" = fade, "fadeAt" = fade_at, "pickupOpen" = pickup_open,
            "closed" = closed, "lifeAt" = life_at, "lifePaused" = life_paused, "lifeLeft" = life_left,
            "gravAt" = grav_at, "pickAt" = pick_at, "dashAt" = dash_at,
        )
    };
}

/// A FinnTreasure_C as the record writes it: the keys it adds to its projectile's `rest`.
pub fn treasure_put(s: &FinnTreasure, x: &Wx, o: &mut Vec<(String, Value)>) {
    treasure_keys!(plain_keys put s, x, o;);
}

/// [`treasure_put`]'s inverse; the keys it knows go into `known`.
pub fn treasure_load(s: &mut FinnTreasure, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    treasure_keys!(plain_keys load s, ob, l;);
    known.extend(treasure_keys!(plain_keys names));
}
