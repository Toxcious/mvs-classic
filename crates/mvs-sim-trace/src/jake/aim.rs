//! JakeAimer and JakeAimPunchEatListener as the record writes them.

use mvs_sim::chars::jake::aim::{JakeAimPunchEatListener, JakeAimer};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! aimer_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "dx" = dx, "dy" = dy, "aimX" = aim_x, "aimY" = aim_y, "distance" = distance, "ay" = ay, "az" = az,
            "sy" = sy, "sz" = sz, "min" = min, "max" = max, "speed" = speed, "aimType" = aim_type,
            "percentCharged" = percent_charged, "zeroTick" = zero_tick, "wait" = wait,
        )
    };
}

macro_rules! listener_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)* "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "dx" = dx, "dy" = dy,)
    };
}

/// A JakeAimer_C as the record writes it: its place and own keys.
pub fn aimer_put(s: &JakeAimer, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    aimer_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`aimer_put`]'s inverse; the keys it knows go into `known`.
pub fn aimer_load(s: &mut JakeAimer, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        aimer_keys!(plain_keys load s, ob, l;);
    });
    known.extend(aimer_keys!(plain_keys names));
}

/// A JakeAimPunchEatListener_C as the record writes it: the blast-box state and its data's `p`.
pub fn listener_put(s: &JakeAimPunchEatListener, x: &Wx, _r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    listener_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`listener_put`]'s inverse.
pub fn listener_load(
    s: &mut JakeAimPunchEatListener,
    _r: &ActorRecord,
    ob: &Obj,
    l: &mut Lx,
    known: &mut Vec<&'static str>,
) {
    l.at("rest", |l| {
        listener_keys!(plain_keys load s, ob, l;);
    });
    known.extend(listener_keys!(plain_keys names));
}
