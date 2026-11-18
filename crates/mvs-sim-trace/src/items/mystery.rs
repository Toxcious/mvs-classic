//! Item_Mystery_C as the record writes it.

use mvs_sim::items::mystery::MysteryItem;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// A MysteryItem as the record writes it.
pub fn mystery_item_put(s: &MysteryItem, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "done" = done, "teamsBeating" = teams_beating, "chosen" = chosen, "equipAt" = equip_at);
}

/// [`mystery_item_put`]'s inverse; the keys it knows go into `known`.
pub fn mystery_item_load(s: &mut MysteryItem, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "done" = done, "teamsBeating" = teams_beating, "chosen" = chosen, "equipAt" = equip_at);
    known.extend(
        plain_keys!(names "done" = done, "teamsBeating" = teams_beating, "chosen" = chosen, "equipAt" = equip_at),
    );
}
