//! Finn's high five and kick actors as the record writes them.

use mvs_sim::chars::finn::high_five::{FinnListener, HighFiveShockwave};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// A listener of Finn as the record writes it: its `done` and the class's data `p`; it has no place.
pub fn listener_put(s: &FinnListener, x: &Wx, _r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "done" = done,);
    o.push(("p".into(), static_object()));
}

/// [`listener_put`]'s inverse; the keys it knows go into `known`.
pub fn listener_load(s: &mut FinnListener, _r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    l.at("rest", |l| {
        plain_keys!(load s, ob, l; "done" = done,);
    });
    known.extend(plain_keys!(names "done" = done,));
}

macro_rules! wave_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "attackId" = attack_id, "end" = end, "facing" = facing, "hit" = hit, "hitOn" = hit_on,
            "hitSet" = hit_set, "left" = left, "r" = r,
        )
    };
}

/// A Finn_HighFive_Shockwave_C as the record writes it (vx and vy readonly 0).
pub fn shockwave_put(s: &HighFiveShockwave, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    wave_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`shockwave_put`]'s inverse; the keys it knows go into `known`.
pub fn shockwave_load(s: &mut HighFiveShockwave, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        wave_keys!(plain_keys load s, ob, l;);
    });
    known.extend(wave_keys!(plain_keys names));
}
