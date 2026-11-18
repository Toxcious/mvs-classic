//! Item_Dynamite as the record writes it.

use mvs_sim::items::dynamite::Dynamite;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// A Item_Dynamite_C as the record writes it.
pub fn dynamite_put(s: &Dynamite, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "boom" = boom, "boomV" = boom_v, "detonated" = detonated);
}

/// [`dynamite_put`]'s inverse; the keys it knows go into `known`.
pub fn dynamite_load(s: &mut Dynamite, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "boom" = boom, "boomV" = boom_v, "detonated" = detonated);
    known.extend(plain_keys!(names "boom" = boom, "boomV" = boom_v, "detonated" = detonated));
}
