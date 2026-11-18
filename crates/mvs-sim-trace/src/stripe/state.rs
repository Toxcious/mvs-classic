//! Stripe's fighter state as the record writes it (`StripeState`): a
//! record per file (`saw` holds nothing), the slot mix with its timed lists and players, the tracker's bound fighters
//! and actors and the live saws by their ids, the target and the dash listener by reference.

use mvs_sim::chars::stripe::StripeState;
use mvs_sim::chars::stripe::state::{StripeBombState, StripeGunState, StripePawnState, StripePerkState};
use mvs_trace::{StripeBombRecord, StripeGunRecord, StripePawnRecord, StripePerkRecord, StripeRecord};

use crate::conv::{rec_rows, rec_struct};
use crate::cx::{Lx, Wx};

rec_struct!(StripeGunState => StripeGunRecord {
    timer: timer, rate: rate, elapsed: elapsed, exit_bound: exit_bound, end_bound: end_bound,
});
rec_struct!(StripeBombState => StripeBombRecord {
    equipped: equipped, active: active, timer: timer, timer_valid: timer_valid, timer_set: timer_set,
    timer_rate: timer_rate, time_remaining_on_drop: time_remaining_on_drop, fuse_offset: fuse_offset,
    fuse_at: fuse_at, knocked_bound: knocked_bound, hit_sound_bound: hit_sound_bound, stun_end_bound: stun_end_bound,
    bounce_bound: bounce_bound, kb_override: kb_override, body_dir_x: body_dir_x, body_dir_y: body_dir_y,
});
rec_struct!(StripePerkState => StripePerkRecord { launch_next_leap: launch_next_leap });

/// `StripePawnState` as the record writes it.
fn pawn_put(s: &StripePawnState, x: &Wx) -> StripePawnRecord {
    let m = &s.slot_mix;
    let mut r = StripePawnRecord {
        slot_mix: crate::fighter::slot_mix_put(&m.slot_mix, x),
        slot_blend: crate::fighter::slot_blend_put(m.slot_blend.as_ref(), x),
        slot_players: crate::fighter::slot_players_put(m.slot_players.as_ref(), x),
        ..StripePawnRecord::default()
    };
    rec_rows!(put s, x, r;
        ev_kb_override: ev_kb_override, com_angle: com_angle, com_alpha: com_alpha, com_blend_start: com_blend_start,
        com_blend_time: com_blend_time, fall_on: fall_on, com_fall: com_fall, chain_saw_on: chain_saw_on,
        chain_saw_rot: chain_saw_rot, ev_com_alpha: ev_com_alpha, ev_com_pitch: ev_com_pitch, ev_fall_on: ev_fall_on,
        ev_fall: ev_fall, ev_saw_on: ev_saw_on, ev_saw_rot: ev_saw_rot,
    );
    r
}

/// [`pawn_put`]'s inverse.
fn pawn_load(s: &mut StripePawnState, r: &StripePawnRecord, l: &mut Lx) {
    let m = &mut s.slot_mix;
    m.slot_mix = crate::fighter::slot_mix_load(&r.slot_mix, l);
    let b = m.slot_blend.get_or_insert_with(Default::default);
    l.at("slot_blend", |l| crate::fighter::slot_blend_load(b, &r.slot_blend, l));
    let p = m.slot_players.get_or_insert_with(Default::default);
    l.at("slot_players", |l| crate::fighter::slot_players_load(p, &r.slot_players, l));
    rec_rows!(load s, r, l;
        ev_kb_override: ev_kb_override, com_angle: com_angle, com_alpha: com_alpha, com_blend_start: com_blend_start,
        com_blend_time: com_blend_time, fall_on: fall_on, com_fall: com_fall, chain_saw_on: chain_saw_on,
        chain_saw_rot: chain_saw_rot, ev_com_alpha: ev_com_alpha, ev_com_pitch: ev_com_pitch, ev_fall_on: ev_fall_on,
        ev_fall: ev_fall, ev_saw_on: ev_saw_on, ev_saw_rot: ev_saw_rot,
    );
}

/// `StripeState` as the record writes it.
pub fn stripe_put(s: &StripeState, x: &Wx) -> StripeRecord {
    let mut r = StripeRecord { pawn: pawn_put(&s.pawn, x), ..StripeRecord::default() };
    rec_rows!(put s, x, r;
        gun: gun, bomb: bomb, perks: perks, last_decay: last_decay, target: target, aim_back: aim_back,
        aim_direction: aim_direction, chainsaw_visible: chainsaw_visible, dash_listener: dash_listener, saws: saws,
    );
    r
}

/// [`stripe_put`]'s inverse.
pub fn stripe_load(s: &mut StripeState, r: &StripeRecord, l: &mut Lx) {
    l.at("pawn", |l| pawn_load(&mut s.pawn, &r.pawn, l));
    rec_rows!(load s, r, l;
        gun: gun, bomb: bomb, perks: perks, last_decay: last_decay, target: target, aim_back: aim_back,
        aim_direction: aim_direction, chainsaw_visible: chainsaw_visible, dash_listener: dash_listener, saws: saws,
    );
}
