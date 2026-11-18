//! PolymorphGrenade as the record writes it: the rigid body's place and velocity are the
//! record's x, y, vx and vy.

use mvs_sim::chars::rick::grenade::PolymorphGrenade;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! grenade_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "angle" = angle, "attackId" = attack_id, "blastAt" = blast_at,
            "blastBounds" = blast_bounds, "facing" = facing, "fuse" = fuse, "hit" = hit, "pvx" = pvx, "pvy" = pvy,
            "radius" = radius, "spin" = spin, "wa" = wa, "wb" = wb, "wc" = wc,
        )
    };
}

/// A PolymorphGrenade_C as the record writes it.
pub fn grenade_put(s: &PolymorphGrenade, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(f64::from(s.vx));
    r.vy = Some(f64::from(s.vy));
    grenade_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`grenade_put`]'s inverse; the keys it knows go into `known`.
pub fn grenade_load(s: &mut PolymorphGrenade, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    opt_load(&mut s.vx, r.vx, "vx", l);
    opt_load(&mut s.vy, r.vy, "vy", l);
    l.at("rest", |l| {
        grenade_keys!(plain_keys load s, ob, l;);
    });
    known.extend(grenade_keys!(plain_keys names));
}
