//! The scrub shield as the record writes it.

use mvs_sim::chars::morty::scrub::ScrubShield;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! scrub_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "cleaned" = cleaned, "inProj" = in_proj, "inActors" = in_actors, "radius" = radius,
            "up" = up, "fwd" = fwd,
        )
    };
}

/// The scrub shield's own keys and place.
pub fn scrub_put(s: &ScrubShield, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    scrub_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`scrub_put`]'s inverse.
pub fn scrub_load(s: &mut ScrubShield, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        scrub_keys!(plain_keys load s, ob, l;);
    });
    known.extend(scrub_keys!(plain_keys names));
}
