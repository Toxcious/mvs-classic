//! Stripe's dropped bomb as the record writes it: BouncingItem's keys, then its own.

use mvs_sim::chars::stripe::bomb::DroppedBomb;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! bomb_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "detonated" = detonated, "detonateTime" = detonate_time, "bombDuration" = bomb_duration,
            "detonateAt" = detonate_at, "detonateSet" = detonate_set, "detonateRate" = detonate_rate, "boom" = boom,
            "explosionBox" = explosion_box,
        )
    };
}

/// A C023B_DroppedBomb_C as the record writes it.
pub fn bomb_put(s: &DroppedBomb, x: &Wx, o: &mut Vec<(String, Value)>) {
    crate::parts::bounce_put(&s.bounce, x, o);
    bomb_keys!(plain_keys put s, x, o;);
}

/// [`bomb_put`]'s inverse; the keys it knows go into `known`.
pub fn bomb_load(s: &mut DroppedBomb, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    crate::parts::bounce_load(&mut s.bounce, ob, l, known);
    bomb_keys!(plain_keys load s, ob, l;);
    known.extend(bomb_keys!(plain_keys names));
}
