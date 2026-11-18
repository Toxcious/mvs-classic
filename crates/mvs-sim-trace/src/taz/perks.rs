//! The perk EatProjectiles's item as the record writes it: C015Anvil_C.

use mvs_sim::chars::taz::perks::Anvil;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// The keys the anvil adds to its item's `rest`.
pub fn anvil_put(s: &Anvil, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "platformsOn" = platforms_on);
}

/// [`anvil_put`]'s inverse; the keys it knows go into `known`.
pub fn anvil_load(s: &mut Anvil, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "platformsOn" = platforms_on);
    known.extend(plain_keys!(names "platformsOn" = platforms_on));
}
