//! `ProjectileBarrier` (ProjectileBarrier_C and its children C016ProjectileBarrier_C, C023A_ProjectileBarrier_C: a
//! world actor): the keys the oracle writes for it and its place.

use mvs_sim::actors::barrier::ProjectileBarrier;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! barrier_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "blocked" = blocked, "fwd" = fwd,
            "inside" = inside, "insideItems" = inside_items, "radius" = radius, "up" = up,
        )
    };
}

/// The barrier's own keys and its place in the record.
pub fn put(s: &ProjectileBarrier, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    barrier_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`put`]'s inverse.
pub fn load(s: &mut ProjectileBarrier, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        barrier_keys!(plain_keys load s, ob, l;);
    });
    known.extend(barrier_keys!(plain_keys names));
}
