//! Item_Basketball_C as the record writes it.

use mvs_sim::stages::the_court::Basketball;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// A Item_Basketball_C's own keys, pushed on `o`.
pub fn basketball_put(s: &Basketball, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "thrownX" = thrown_x, "canBounce" = can_bounce, "pvx" = pvx, "pvy" = pvy, "kb" = kb, "chained" = chained, "lastHolder" = last_holder);
}

/// [`basketball_put`]'s inverse; the keys it knows go into `known`.
pub fn basketball_load(s: &mut Basketball, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "thrownX" = thrown_x, "canBounce" = can_bounce, "pvx" = pvx, "pvy" = pvy, "kb" = kb, "chained" = chained, "lastHolder" = last_holder);
    known.extend(plain_keys!(names "thrownX" = thrown_x, "canBounce" = can_bounce, "pvx" = pvx, "pvy" = pvy, "kb" = kb, "chained" = chained, "lastHolder" = last_holder));
}
