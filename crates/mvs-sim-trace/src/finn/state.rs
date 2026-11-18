//! Finn's fighter state as the record writes it (`FinnState`): a record
//! per file (`gem`, `store`, `chop` and `perks` hold nothing), the slot mix and its timed lists, the live aimer, shop
//! and BMO and the dash's and the last hit's actors by reference.

use mvs_sim::chars::finn::FinnState;
use mvs_sim::chars::finn::state::{FinnCoinsState, FinnDashState, FinnMomentum};
use mvs_trace::{FinnCoinsRecord, FinnDashRecord, FinnMomentumRecord, FinnPawnRecord, FinnRecord};

use crate::conv::{rec_rows, rec_struct};
use crate::cx::{Lx, Wx};

rec_struct!(FinnCoinsState => FinnCoinsRecord {
    spawn_x: spawn_x, spawn_y: spawn_y, knock_back: knock_back, dir_x: dir_x, dir_y: dir_y,
    last_stack_count: last_stack_count,
});
rec_struct!(FinnDashState => FinnDashRecord { target: target, trail_at: trail_at, trail_pitch: trail_pitch });
rec_struct!(FinnMomentum => FinnMomentumRecord {
    stored: stored, max: max, amount: amount, dmg_mult: dmg_mult, kb_mult: kb_mult, calculating: calculating,
    high_five: high_five, high_five_pct: high_five_pct, high_five_max: high_five_max, meter: meter,
});

/// `FinnState` as the record writes it.
pub fn finn_put(s: &FinnState, x: &Wx) -> FinnRecord {
    let m = &s.pawn.slot_mix;
    let mut r = FinnRecord {
        pawn: FinnPawnRecord {
            slot_mix: crate::fighter::slot_mix_put(&m.slot_mix, x),
            slot_blend: crate::fighter::slot_blend_put(m.slot_blend.as_ref(), x),
        },
        ..FinnRecord::default()
    };
    rec_rows!(put s, x, r;
        coins: coins, dash: dash, momentum: momentum, last_hit_actor: last_hit_actor, last_hit_time: last_hit_time,
        drop_coin_override: drop_coin_override, drop_coin_amount: drop_coin_amount, aimer: aimer, shop: shop,
        bmo: bmo, run_ui: run_ui,
    );
    r
}

/// [`finn_put`]'s inverse.
pub fn finn_load(s: &mut FinnState, r: &FinnRecord, l: &mut Lx) {
    let m = &mut s.pawn.slot_mix;
    m.slot_mix = crate::fighter::slot_mix_load(&r.pawn.slot_mix, l);
    let b = m.slot_blend.get_or_insert_with(Default::default);
    l.at("slot_blend", |l| crate::fighter::slot_blend_load(b, &r.pawn.slot_blend, l));
    rec_rows!(load s, r, l;
        coins: coins, dash: dash, momentum: momentum, last_hit_actor: last_hit_actor, last_hit_time: last_hit_time,
        drop_coin_override: drop_coin_override, drop_coin_amount: drop_coin_amount, aimer: aimer, shop: shop,
        bmo: bmo, run_ui: run_ui,
    );
}
