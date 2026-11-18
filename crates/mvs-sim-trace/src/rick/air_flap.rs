//! MeeSeeks_AirFlap as the record writes it. The actor stands at its place (the record's x
//! and y, vx and vy readonly 0); `comps` and `p` are the class's static data.

use mvs_sim::chars::rick::air_flap::AirFlap;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! flap_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "attackId" = attack_id, "blastAt" = blast_at, "blastBounds" = blast_bounds,
            "facing" = facing, "hit" = hit, "hitOn" = hit_on, "hitSet" = hit_set, "offAt" = off_at,
        )
    };
}

/// A MeeSeeks_AirFlap as the record writes it.
pub fn air_flap_put(s: &AirFlap, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    flap_keys!(plain_keys put s, x, o;);
    o.push(("comps".into(), static_object()));
    o.push(("p".into(), static_object()));
}

/// [`air_flap_put`]'s inverse; the keys it knows go into `known`.
pub fn air_flap_load(s: &mut AirFlap, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        flap_keys!(plain_keys load s, ob, l;);
    });
    known.extend(flap_keys!(plain_keys names));
    known.push("comps");
}
