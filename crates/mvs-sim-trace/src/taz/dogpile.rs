//! The dogpile as the record writes it: C015Dogpile_C.
//!
//! `p` is the class defaults (`{static: "object"}`) until a graph writes the instance's variables, then the whole
//! property object.

use std::sync::Arc;

use mvs_data::value::Value as DataValue;
use mvs_sim::chars::taz::dogpile::Dogpile;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};
use crate::parts::{data_load, data_put};

macro_rules! dogpile_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "alpha" = alpha, "attackId" = attack_id, "blastAt" = blast_at, "blastBounds" = blast_bounds,
            "final" = fin, "growing" = growing, "hasHitEnemy" = has_hit_enemy, "hit" = hit, "hitN" = hit_n,
            "hitOn" = hit_on, "hitSet" = hit_set, "hitSetAt" = hit_set_at, "is2D" = is_2d, "once" = once,
            "over" = over, "oz" = oz, "radius" = radius, "refreshN" = refresh_n, "refreshSet" = refresh_set,
            "resume" = resume, "sphereR" = sphere_r, "types" = types, "weak" = weak, "weakHits" = weak_hits,
        )
    };
}

/// A C015Dogpile_C's own keys and place.
pub fn dogpile_put(s: &Dogpile, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    dogpile_keys!(plain_keys put s, x, o;);
    let p = match &s.own_p {
        Some(p) => data_put(&DataValue::Obj((**p).clone())),
        None => static_object(),
    };
    o.push(("p".into(), p));
}

/// [`dogpile_put`]'s inverse; the keys it knows go into `known`.
pub fn dogpile_load(s: &mut Dogpile, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        dogpile_keys!(plain_keys load s, ob, l;);
        s.own_p = match ob.get("p") {
            Some(v @ Value::Obj(o)) if o.get("static").is_none() => match data_load(v) {
                DataValue::Obj(p) => Some(Arc::new(p)),
                _ => {
                    l.at("p", |l| l.miss("no object"));
                    None
                }
            },
            _ => None,
        };
    });
    known.extend(dogpile_keys!(plain_keys names));
    known.push("p");
}
