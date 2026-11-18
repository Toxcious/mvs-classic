//! Marvin's disintegration rays as the record writes them.

use mvs_sim::chars::marvin::ray::Ray;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! ray_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "attackId" = attack_id, "blastAt" = blast_at, "blastBounds" = blast_bounds, "down" = down,
            "endDist" = end_dist, "endX" = end_x, "endY" = end_y, "facing" = facing, "half" = half, "hit" = hit,
            "hitAge" = hit_age, "hitOn" = hit_on, "hitSet" = hit_set, "hx" = hx, "hy" = hy, "lx" = lx, "ly" = ly,
            "playing" = playing, "position" = position, "radius" = radius, "rate" = rate, "socket" = socket,
            "startX" = start_x, "startY" = start_y, "vfxDx" = vfx_dx, "vfxDy" = vfx_dy,
        )
    };
}

/// a disintegration ray of Marvin as the record writes it (vx and vy readonly 0).
pub fn ray_put(s: &Ray, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    ray_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`ray_put`]'s inverse; the keys it knows go into `known`.
pub fn ray_load(s: &mut Ray, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        ray_keys!(plain_keys load s, ob, l;);
    });
    known.extend(ray_keys!(plain_keys names));
}
