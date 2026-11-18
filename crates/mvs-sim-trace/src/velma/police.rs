//! Velma's police as the record writes it: its classes' `put` and `load`. The record's x and y
//! are the pin's own fields.

use mvs_sim::chars::velma::police::{ArrestedExt, JenkinsPin};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

macro_rules! pin_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "anim" = anim, "attachedToCar" = attached_to_car, "blastAt" = blast_at,
            "blastBounds" = blast_bounds, "entered" = entered, "relX" = rel_x, "relY" = rel_y, "target" = target,
            "visible" = visible,
        )
    };
}

/// A VelmaJenkinsPin_C as the record writes it.
pub fn pin_put(s: &JenkinsPin, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    pin_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`pin_put`]'s inverse; the keys it knows go into `known`.
pub fn pin_load(s: &mut JenkinsPin, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        pin_keys!(plain_keys load s, ob, l;);
    });
    known.extend(pin_keys!(plain_keys names));
}

plain_struct!(ArrestedExt { "car" = car });

/// Buff_Arrested_C's extension as the oracle writes it (`{ car }`).
pub fn arrested_put(e: &ArrestedExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`arrested_put`]'s inverse.
pub fn arrested_load(e: &mut ArrestedExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
