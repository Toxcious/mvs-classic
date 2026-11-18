//! Shaggy's world actor the record writes beyond the shockwave: the Scooby Snack box.
//! The record's x, y, vx and vy are the box's own; the rest are its keys.

use mvs_sim::chars::shaggy::SnackBox;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! snack_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "angle" = angle, "blastAt" = blast_at, "blastBounds" = blast_bounds,
            "canPickUp" = can_pick_up, "half" = half, "over" = over, "radius" = radius, "spin" = spin,
            "wait" = wait, "wc" = wc,
        )
    };
}

/// The box's own keys and place.
pub fn snack_put(s: &SnackBox, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(f64::from(s.vx));
    r.vy = Some(f64::from(s.vy));
    snack_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`snack_put`]'s inverse.
pub fn snack_load(s: &mut SnackBox, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    opt_load(&mut s.vx, r.vx, "vx", l);
    opt_load(&mut s.vy, r.vy, "vy", l);
    l.at("rest", |l| {
        snack_keys!(plain_keys load s, ob, l;);
    });
    known.extend(snack_keys!(plain_keys names));
}
