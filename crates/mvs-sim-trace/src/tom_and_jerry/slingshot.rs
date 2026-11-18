//! Tom and Jerry's slingshot aimer as the record writes it.

use mvs_sim::chars::tom_and_jerry::slingshot::SlingAim;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! sling_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "aimDirection" = aim_direction, "aimMax" = aim_max, "aimMin" = aim_min,
            "aimSpeed" = aim_speed, "aimType" = aim_type, "ay" = ay, "az" = az, "blastAt" = blast_at,
            "blastBounds" = blast_bounds, "checkLeft" = check_left, "sy" = sy, "sz" = sz,
            "traceAlpha" = trace_alpha,
        )
    };
}

/// A TomAndJerry_SlingShot_Aim_02_C as the record writes it (its place is the record's x and y).
pub fn sling_aim_put(s: &SlingAim, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    sling_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`sling_aim_put`]'s inverse; the keys it knows go into `known`.
pub fn sling_aim_load(s: &mut SlingAim, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        sling_keys!(plain_keys load s, ob, l;);
    });
    known.extend(sling_keys!(plain_keys names));
}
