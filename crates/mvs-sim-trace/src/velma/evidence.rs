//! Velma's evidence as the record writes it: its classes' `put` and `load`. The record's
//! x, y, vx and vy are the clue's own fields.

use mvs_sim::chars::velma::evidence::{CountExt, Evidence};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

macro_rules! evidence_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "angle" = angle, "blastAt" = blast_at, "blastBounds" = blast_bounds,
            "canPickUp" = can_pick_up, "collectable" = collectable, "grabRadius" = grab_radius, "half" = half,
            "over" = over, "radius" = radius, "simulating" = simulating, "spin" = spin, "wait" = wait, "wc" = wc,
        )
    };
}

/// A VelmaEvidence_C / VelmaEvidenceNoGrav_C as the record writes it.
pub fn evidence_put(s: &Evidence, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(f64::from(s.vx));
    r.vy = Some(f64::from(s.vy));
    evidence_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`evidence_put`]'s inverse; the keys it knows go into `known`.
pub fn evidence_load(s: &mut Evidence, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    opt_load(&mut s.vx, r.vx, "vx", l);
    opt_load(&mut s.vy, r.vy, "vy", l);
    l.at("rest", |l| {
        evidence_keys!(plain_keys load s, ob, l;);
    });
    known.extend(evidence_keys!(plain_keys names));
}

plain_struct!(CountExt { "allow" = allow, "count" = count, "from" = from, "wait" = wait });

/// Buff_EvidenceCount_C's extension as the oracle writes it (`{ count, allow, wait, from }`).
pub fn count_put(e: &CountExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`count_put`]'s inverse.
pub fn count_load(e: &mut CountExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
