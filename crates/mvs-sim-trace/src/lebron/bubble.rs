//! `SassBubble` (C016_SassBubble_C: a projectile's `ext`). C016ProjectileBarrier_C is the
//! shared barrier's (`barrier.rs`).

use mvs_sim::chars::lebron::bubble::SassBubble;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// The keys a sass bubble adds to its projectile's `rest`.
pub fn put(p: &SassBubble, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put p, x, o;
        "boxFwd" = box_fwd, "boxGone" = box_gone, "boxX" = box_x, "boxY" = box_y, "boxZ" = box_z,
        "growthLen" = growth_len, "psScale" = ps_scale,
    );
}

/// [`put`]'s inverse.
pub fn load(p: &mut SassBubble, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load p, ob, l;
        "boxFwd" = box_fwd, "boxGone" = box_gone, "boxX" = box_x, "boxY" = box_y, "boxZ" = box_z,
        "growthLen" = growth_len, "psScale" = ps_scale,
    );
    known.extend(plain_keys!(names
        "boxFwd" = box_fwd, "boxGone" = box_gone, "boxX" = box_x, "boxY" = box_y, "boxZ" = box_z,
        "growthLen" = growth_len, "psScale" = ps_scale,
    ));
}
