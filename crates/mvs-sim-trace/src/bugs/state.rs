//! Bugs Bunny's fighter state as the record writes it (`BugsState`): one
//! record, the Lingering Love kiss by its projectile's reference (a destroyed one stays referenced: the `spawned`
//! family).

use mvs_sim::chars::bugs::BugsState;
use mvs_trace::BugsRecord;

use crate::conv::{Rec, rec_struct};
use crate::cx::{Lx, Wx};

rec_struct!(BugsState => BugsRecord {
    gliding: gliding, anim_gliding: anim_gliding, glide_w: glide_w, glide_last: glide_last,
    glide_from_w: glide_from_w, glide_to_w: glide_to_w, glide_blend: glide_blend, glide_elapsed: glide_elapsed,
    glide_time: glide_time, glide_seq_from: glide_seq_from, glide_seq_delta: glide_seq_delta,
    glide_once_closed: glide_once_closed, original_gravity_scale: original_gravity_scale,
    skip_impulse_validation: skip_impulse_validation, lingering_kiss: lingering_kiss, item_equip: item_equip,
});

/// `BugsState` as the record writes it.
pub fn bugs_put(s: &BugsState, x: &Wx) -> BugsRecord {
    s.put(x)
}

/// [`bugs_put`]'s inverse.
pub fn bugs_load(s: &mut BugsState, r: &BugsRecord, l: &mut Lx) {
    s.load(r, l);
}
