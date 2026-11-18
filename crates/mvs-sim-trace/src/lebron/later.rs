//! `DashToBasketball` (DashToBasketball_C, a world actor): its own keys; it has no place of
//! its own (the record's x and y stay absent).

use mvs_sim::chars::lebron::later::DashToBasketball;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::static_object;
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! dash_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "ball" = ball, "blastAt" = blast_at, "blastBounds" = blast_bounds, "ignited" = ignited,
            "move" = mv, "rot" = rot, "trail" = trail,
        )
    };
}

/// The dash's own keys.
pub fn put(s: &DashToBasketball, x: &Wx, _r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    dash_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`put`]'s inverse.
pub fn load(s: &mut DashToBasketball, _r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    l.at("rest", |l| {
        dash_keys!(plain_keys load s, ob, l;);
    });
    known.extend(dash_keys!(plain_keys names));
}
