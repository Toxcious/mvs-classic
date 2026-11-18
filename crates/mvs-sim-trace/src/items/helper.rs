//! Item_Helper as the record writes it.

use mvs_sim::items::helper::HelperItem;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// A Item_Helper_C as the record writes it.
pub fn helper_put(s: &HelperItem, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "spawnOnDestroyed" ~ spawn_on_destroyed);
}

/// [`helper_put`]'s inverse; the keys it knows go into `known`.
pub fn helper_load(s: &mut HelperItem, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "spawnOnDestroyed" ~ spawn_on_destroyed);
    known.extend(plain_keys!(names "spawnOnDestroyed" ~ spawn_on_destroyed));
}
