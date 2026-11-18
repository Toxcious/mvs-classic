//! Item_Safe_C as the record writes it.

use mvs_sim::items::safe::SafeItem;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// A SafeItem as the record writes it.
pub fn safe_item_put(s: &SafeItem, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "capsuleHidden" = capsule_hidden, "knockAt" = knock_at);
}

/// [`safe_item_put`]'s inverse; the keys it knows go into `known`.
pub fn safe_item_load(s: &mut SafeItem, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "capsuleHidden" = capsule_hidden, "knockAt" = knock_at);
    known.extend(plain_keys!(names "capsuleHidden" = capsule_hidden, "knockAt" = knock_at));
}
