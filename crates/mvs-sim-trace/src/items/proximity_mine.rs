//! Item_ProximityMine_C as the record writes it.

use mvs_sim::items::proximity_mine::ProximityMine;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// A ProximityMine as the record writes it.
pub fn proximity_mine_put(s: &ProximityMine, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "armed" = armed, "attached" = attached, "canActivate" = can_activate, "detonated" = detonated, "inStep" = in_step, "inside" = inside, "noPick" = no_pick, "roll" = roll, "wasHeld" = was_held);
}

/// [`proximity_mine_put`]'s inverse; the keys it knows go into `known`.
pub fn proximity_mine_load(s: &mut ProximityMine, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "armed" = armed, "attached" = attached, "canActivate" = can_activate, "detonated" = detonated, "inStep" = in_step, "inside" = inside, "noPick" = no_pick, "roll" = roll, "wasHeld" = was_held);
    known.extend(plain_keys!(names "armed" = armed, "attached" = attached, "canActivate" = can_activate, "detonated" = detonated, "inStep" = in_step, "inside" = inside, "noPick" = no_pick, "roll" = roll, "wasHeld" = was_held));
}
