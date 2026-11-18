//! Ground down's actor as the record writes it: the GarnetShockwaveHitBox classes. The
//! actor stands at its spawn place (the record's x and y, vx and vy readonly 0).

use mvs_sim::chars::garnet::shockwave::ShockwaveHitBox;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! box_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "attackId" = attack_id, "blastAt" = blast_at, "blastBounds" = blast_bounds, "ended" = ended,
            "facing" = facing, "hit" = hit, "hitOn" = hit_on, "hitSet" = hit_set, "hx" = hx, "hy" = hy,
            "locations" = locations, "playing" = playing, "position" = position, "prevHx" = prev_hx,
            "prevHy" = prev_hy, "releaseAge" = release_age, "startTime" = start_time,
            "totalDistance" = total_distance,
        )
    };
}

/// a GarnetShockwaveHitBox as the record writes it.
pub fn hit_box_put(s: &ShockwaveHitBox, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    box_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`hit_box_put`]'s inverse; the keys it knows go into `known`.
pub fn hit_box_load(s: &mut ShockwaveHitBox, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        box_keys!(plain_keys load s, ob, l;);
    });
    known.extend(box_keys!(plain_keys names));
}
