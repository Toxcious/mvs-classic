//! Rick's fighter state as the record writes it (`RickState`): the pawn's
//! and the AnimBP's variables, the slot mix as the other characters', Comp_Pawn_Portal's variables with the allied
//! portals by reference.

use mvs_sim::chars::rick::RickState;
use mvs_sim::chars::rick::state::PortalComp;
use mvs_trace::{PortalCompRecord, RickRecord};

use crate::conv::{rec_rows, rec_struct};
use crate::cx::{Lx, Wx};

rec_struct!(PortalComp => PortalCompRecord { allied: allied, exit_left: exit_left, vfx_x: vfx_x, vfx_y: vfx_y });

/// `RickState` as the record writes it.
pub fn rick_put(s: &RickState, x: &Wx) -> RickRecord {
    let mut r = RickRecord { slot_mix: crate::fighter::slot_mix_put(&s.slot_mix.slot_mix, x), ..RickRecord::default() };
    rec_rows!(put s, x, r;
        listening: listening, charge_amount: charge_amount, aim_direction: aim_direction, portal_comp: portal_comp,
    );
    r
}

/// [`rick_put`]'s inverse.
pub fn rick_load(s: &mut RickState, r: &RickRecord, l: &mut Lx) {
    s.slot_mix.slot_mix = crate::fighter::slot_mix_load(&r.slot_mix, l);
    rec_rows!(load s, r, l;
        listening: listening, charge_amount: charge_amount, aim_direction: aim_direction, portal_comp: portal_comp,
    );
}
