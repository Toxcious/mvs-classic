//! Jake's fighter state as the record writes it (`JakeState`): a record
//! per group of files (`stretch`, `house`, `horse`, `spit`, `ally_spit` and `weapons` hold nothing), the slot mix with
//! its timed lists, the held, bound and spat fighters by their ids, the slow buff by its place in his buff list (-1
//! none, -2 ended; [`jake_refs_load`] once his buffs have their serials), the live actors by reference.

use mvs_sim::Fighter;
use mvs_sim::Real;
use mvs_sim::chars::CharState;
use mvs_sim::chars::jake::JakeState;
use mvs_sim::chars::jake::state::{JakeAimState, JakeAnim, JakeEatState, JakePawnState};
use mvs_trace::{CharRecord, JakeAimRecord, JakeAnimRecord, JakeEatRecord, JakePawnRecord, JakeRecord};

use crate::conv::{rec_rows, rec_struct};
use crate::cx::{Lx, Wx};

rec_struct!(JakeAimState => JakeAimRecord { aim_punch_y: aim_punch_y, aim_punch_z: aim_punch_z });
rec_struct!(JakeAnim => JakeAnimRecord {
    charge_amount: charge_amount, stretch_on: stretch_on, calculate_stretch_angle: calculate_stretch_angle,
    stretch_direction: stretch_direction, aim_direction: aim_direction, aim_distance_alpha: aim_distance_alpha,
    eat_override: eat_override,
});

/// A fixed list of the state as the record's list.
fn arr_put<const N: usize>(a: &[Real; N]) -> Vec<f64> {
    crate::conv::wide(a)
}

/// [`arr_put`]'s inverse: a list of another length is a miss.
fn arr_load<const N: usize>(a: &mut [Real; N], v: &[f64], l: &mut Lx) {
    match <[f64; N]>::try_from(v) {
        Ok(x) => *a = x.map(mvs_sim::math::narrow),
        Err(_) => l.miss(format_args!("a list of {} numbers for one of {N}", v.len())),
    }
}

/// `JakePawnState` as the record writes it.
fn pawn_put(s: &JakePawnState, x: &Wx) -> JakePawnRecord {
    let m = &s.slot_mix;
    let mut r = JakePawnRecord {
        slot_mix: crate::fighter::slot_mix_put(&m.slot_mix, x),
        slot_blend: crate::fighter::slot_blend_put(m.slot_blend.as_ref(), x),
        eat_times: arr_put(&s.eat_times),
        ..JakePawnRecord::default()
    };
    rec_rows!(put s, x, r;
        section_override: section_override, charge_total_time: charge_total_time, bind_delay: bind_delay,
        charge_bound: charge_bound, eat: eat, walk_time: walk_time, walk_w: walk_w, walk_from: walk_from,
        walk_rem: walk_rem, walk_dur: walk_dur, walk_last: walk_last, eat_first: eat_first, fade_from: fade_from,
        fade_dur: fade_dur, fade_t: fade_t,
    );
    r
}

/// [`pawn_put`]'s inverse.
fn pawn_load(s: &mut JakePawnState, r: &JakePawnRecord, l: &mut Lx) {
    let m = &mut s.slot_mix;
    m.slot_mix = crate::fighter::slot_mix_load(&r.slot_mix, l);
    let b = m.slot_blend.get_or_insert_with(Default::default);
    l.at("slot_blend", |l| crate::fighter::slot_blend_load(b, &r.slot_blend, l));
    l.at("eat_times", |l| arr_load(&mut s.eat_times, &r.eat_times, l));
    rec_rows!(load s, r, l;
        section_override: section_override, charge_total_time: charge_total_time, bind_delay: bind_delay,
        charge_bound: charge_bound, eat: eat, walk_time: walk_time, walk_w: walk_w, walk_from: walk_from,
        walk_rem: walk_rem, walk_dur: walk_dur, walk_last: walk_last, eat_first: eat_first, fade_from: fade_from,
        fade_dur: fade_dur, fade_t: fade_t,
    );
}

/// `JakeEatState` as the record writes it (`slow_buff` by its place in `f`'s buffs).
fn eat_put(s: &JakeEatState, f: &Fighter, x: &Wx) -> JakeEatRecord {
    let mut r = JakeEatRecord { slow_buff: crate::fighter::buff_ref_put(f, s.slow_buff), ..JakeEatRecord::default() };
    rec_rows!(put s, x, r;
        timer: timer, rate: rate, elapsed: elapsed, timer_active: timer_active, struggle_count: struggle_count,
        struggle_bound: struggle_bound, can_attack: can_attack, can_drop_ally: can_drop_ally,
        spit_fighter: spit_fighter, closed: closed, spit_bound: spit_bound,
    );
    r
}

/// [`eat_put`]'s inverse but the slow buff (left `None` for [`jake_refs_load`]).
fn eat_load(s: &mut JakeEatState, r: &JakeEatRecord, l: &mut Lx) {
    s.slow_buff = None;
    rec_rows!(load s, r, l;
        timer: timer, rate: rate, elapsed: elapsed, timer_active: timer_active, struggle_count: struggle_count,
        struggle_bound: struggle_bound, can_attack: can_attack, can_drop_ally: can_drop_ally,
        spit_fighter: spit_fighter, closed: closed, spit_bound: spit_bound,
    );
}

/// `JakeState` as the record writes it.
pub fn jake_put(s: &JakeState, f: &Fighter, x: &Wx) -> JakeRecord {
    let mut r = JakeRecord {
        pawn: pawn_put(&s.pawn, x),
        eat: eat_put(&s.eat, f, x),
        head_scale: arr_put(&s.head_scale),
        ..JakeRecord::default()
    };
    rec_rows!(put s, x, r;
        aim: aim, anim: anim, reach_check: reach_check, holding_actor: holding_actor, held_fighters: held_fighters,
        weapon: weapon, weapon_equipped: weapon_equipped, weapons_hidden: weapons_hidden,
        stretch_actor: stretch_actor, house_actor: house_actor, horse_actor: horse_actor, aimer: aimer,
        spit_aimer: spit_aimer, ally_aim: ally_aim,
    );
    r
}

/// [`jake_put`]'s inverse but the slow buff ([`jake_refs_load`]).
pub fn jake_load(s: &mut JakeState, r: &JakeRecord, l: &mut Lx) {
    l.at("pawn", |l| pawn_load(&mut s.pawn, &r.pawn, l));
    l.at("eat", |l| eat_load(&mut s.eat, &r.eat, l));
    l.at("head_scale", |l| arr_load(&mut s.head_scale, &r.head_scale, l));
    rec_rows!(load s, r, l;
        aim: aim, anim: anim, reach_check: reach_check, holding_actor: holding_actor, held_fighters: held_fighters,
        weapon: weapon, weapon_equipped: weapon_equipped, weapons_hidden: weapons_hidden,
        stretch_actor: stretch_actor, house_actor: house_actor, horse_actor: horse_actor, aimer: aimer,
        spit_aimer: spit_aimer, ally_aim: ally_aim,
    );
}

/// JakeState's slow buff reference, loaded once the fighter's buffs have their serials.
pub fn jake_refs_load(f: &mut Fighter, r: &CharRecord, l: &mut Lx) {
    let CharRecord::Jake(r) = r else { return };
    let id = l.fighter(f64::from(f.id));
    let b = l.at("eat", |l| crate::fighter::buff_ref_load(f, id, r.eat.slow_buff, l));
    if let CharState::Jake(s) = &mut f.cs {
        s.eat.slow_buff = b;
    }
}
