//! Marvin's fighter state as the record writes it (`MarvinState`):
//! a record per group of files (`platform`, `bubble`, `blaster`, `flag`, `swap` and `perks` hold nothing), the slot
//! mix, the live UFO by reference.

use mvs_sim::chars::marvin::MarvinState;
use mvs_sim::chars::marvin::state::MarvinDisint;
use mvs_trace::{MarvinDisintRecord, MarvinPawnRecord, MarvinRecord};

use crate::conv::{rec_rows, rec_struct};
use crate::cx::{Lx, Wx};

rec_struct!(MarvinDisint => MarvinDisintRecord {
    taking: taking, has_hit: has_hit, inc_timer: inc_timer, inc_t: inc_t, inc_at: inc_at, end_timer: end_timer,
    end_t: end_t, end_at: end_at, end_rate: end_rate,
});

/// `MarvinState` as the record writes it.
pub fn marvin_put(s: &MarvinState, x: &Wx) -> MarvinRecord {
    let mut r = MarvinRecord {
        pawn: MarvinPawnRecord {
            slot_mix: crate::fighter::slot_mix_put(&s.pawn.slot_mix.slot_mix, x),
            wall_blend_space_value: f64::from(s.pawn.wall_blend_space_value),
        },
        ..MarvinRecord::default()
    };
    rec_rows!(put s, x, r;
        aim_deg: aim_deg, aim_value: aim_value, disint: disint, wall_cling_blend: wall_cling_blend, ufo: ufo,
    );
    r
}

/// [`marvin_put`]'s inverse.
pub fn marvin_load(s: &mut MarvinState, r: &MarvinRecord, l: &mut Lx) {
    s.pawn.slot_mix.slot_mix = crate::fighter::slot_mix_load(&r.pawn.slot_mix, l);
    s.pawn.wall_blend_space_value = mvs_sim::math::narrow(r.pawn.wall_blend_space_value);
    rec_rows!(load s, r, l;
        aim_deg: aim_deg, aim_value: aim_value, disint: disint, wall_cling_blend: wall_cling_blend, ufo: ufo,
    );
}
