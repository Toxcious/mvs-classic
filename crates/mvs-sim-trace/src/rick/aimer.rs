//! Rick's portal aimer as the record writes it: the record's x and y; no velocity.

use mvs_sim::chars::rick::aimer::PortalAimer;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! aimer_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "activated" = activated, "age" = age, "allies" = allies, "blastAt" = blast_at,
            "blastBounds" = blast_bounds, "conds" = conds, "gunLeft" = gun_left, "halfHeight" = half_height,
            "inputActive" = input_active, "inputLeft" = input_left, "locallyControlled" = locally_controlled,
            "maxX" = max_x, "maxY" = max_y, "minX" = min_x, "minY" = min_y, "partner" = partner, "radius" = radius,
            "releaseLeft" = release_left,
        )
    };
}

/// A C020PortalAimer_C as the record writes it (AllyIndex a number).
pub fn aimer_put(s: &PortalAimer, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    aimer_keys!(plain_keys put s, x, o;);
    o.push(("allyIndex".into(), Value::Num(s.ally_index as f64)));
    o.push(("p".into(), static_object()));
}

/// [`aimer_put`]'s inverse; the keys it knows go into `known`.
pub fn aimer_load(s: &mut PortalAimer, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        aimer_keys!(plain_keys load s, ob, l;);
        match ob.get("allyIndex") {
            Some(v) => l.at("allyIndex", |l| {
                let mut n = 0.0_f64;
                n.load_value(v, l);
                s.ally_index = n as usize;
            }),
            None => l.miss("key allyIndex absent"),
        }
    });
    known.extend(aimer_keys!(plain_keys names));
    known.push("allyIndex");
}
