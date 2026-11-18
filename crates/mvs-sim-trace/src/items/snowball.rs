//! Item_Snowball_C and Item_SuperSnowball_C as the record writes it: the oracle `Snowball` class
//! adds no field, so there is nothing to write.

use mvs_sim::items::snowball::Snowball;
use mvs_trace::{Obj, Value};

use crate::cx::{Lx, Wx};

/// A Snowball as the record writes it: no key.
pub fn snowball_put(_s: &Snowball, _x: &Wx, _o: &mut Vec<(String, Value)>) {}

/// [`snowball_put`]'s inverse: no key.
pub fn snowball_load(_s: &mut Snowball, _ob: &Obj, _l: &mut Lx, _known: &mut Vec<&'static str>) {}
