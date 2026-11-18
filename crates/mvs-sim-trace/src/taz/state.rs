//! Taz's fighter state as the record writes it (`TazState`): the eaten
//! fighters and the spit's target by fighter id, the tornado buff by its place in his buff list (-1 none, -2 ended;
//! [`taz_refs_load`] once his buffs have their serials), the slot mix as the other characters'.
//!
//! `spitHit` is the montage notify's Hit the frame's BPI_SpitFighters carries: a value only between eat.rs's
//! AnimNotify_C015SpitFighter and its worldTick, so no frame's end holds one until the eat's port writes it.

use mvs_sim::Fighter;
use mvs_sim::chars::CharState;
use mvs_sim::chars::taz::TazState;
use mvs_sim::chars::taz::state::{EatenFighter, SpitOffset};
use mvs_trace::{CharRecord, EatenFighterRecord, SpitOffsetRecord, TazRecord};

use crate::conv::rec_rows;
use crate::cx::{Lx, Wx};
use mvs_data::fighter::Notify;
use mvs_sim::events::NotifyRef;
use mvs_trace::Value;

/// The Hit an AnimNotify_C015SpitFighter_C writes to `spitHit` (`n.p.Hit ?? {KnockBackDirectionVector: {X: 0, Y: 0}}`),
/// as the record writes it.
// Unverified: the record's own object is written as `static_put` writes a property set (no frame's end holds a spitHit,
// so no baseline trace shows its form).
pub fn spit_hit_value(n: &Notify) -> Value {
    match mvs_sim::registry::props::obj(&n.p, "Hit") {
        Some(h) => crate::parts::static_put(h),
        None => crate::conv::obj(vec![(
            "KnockBackDirectionVector",
            crate::conv::obj(vec![("X", Value::Num(0.0)), ("Y", Value::Num(0.0))]),
        )]),
    }
}

/// [`taz_put`]'s `spit_hit` inverse: the first notify among the loading character's montages whose Hit is `v`.
fn spit_hit_load(v: &Value, l: &mut Lx) -> Option<NotifyRef> {
    let book = l.book;
    let Some(own) = l.ch.as_ref().and_then(|c| book.char_owners.get(c)) else {
        l.miss("a spitHit outside a fighter of a character of the data");
        return None;
    };
    let found = book.spit_hits.iter().find(|((m, _), q)| own.contains(m) && crate::parts::same_value(q, v));
    match found {
        Some(((m, i), _)) => Some(NotifyRef { montage: l.name(m), index: *i }),
        None => {
            l.miss("no AnimNotify_C015SpitFighter_C of the data with this Hit");
            None
        }
    }
}

/// `TazState` as the record writes it (`tornado_buff` by its place in `f`'s buffs).
pub fn taz_put(s: &TazState, f: &Fighter, x: &Wx) -> TazRecord {
    let mut r = TazRecord {
        eaten_fighters: s
            .eaten_fighters
            .iter()
            .map(|e| EatenFighterRecord { f: f64::from(x.fid(e.f)), v: f64::from(e.v), slot: f64::from(e.slot) })
            .collect(),
        spit_hit: s.spit_hit.and_then(|n| {
            let v = x.book.spit_hits.get(&(x.str(n.montage).to_owned(), n.index)).cloned();
            if v.is_none() {
                x.unmapped("taz spitHit: a notify that is no AnimNotify_C015SpitFighter_C of the data");
            }
            v
        }),
        spit_offset: s.spit_offset.as_ref().map(|o| SpitOffsetRecord {
            vic: f64::from(x.fid(o.vic)),
            dx: f64::from(o.dx),
            dy: f64::from(o.dy),
        }),
        hit_allies: s.hit_allies.iter().map(|&a| f64::from(x.fid(a))).collect(),
        tornado_buff: crate::fighter::buff_ref_put(f, s.tornado_buff),
        slot_mix: crate::fighter::slot_mix_put(&s.slot_mix.slot_mix, x),
        ..TazRecord::default()
    };
    rec_rows!(put s, x, r;
        eat_level: eat_level, eat_free: eat_free, eat_slots: eat_slots,
        projectiles_being_eaten: projectiles_being_eaten, stored_projectile: stored_projectile,
        stored_starting_speed: stored_starting_speed, stored_max_speed: stored_max_speed,
        burping_projectile: burping_projectile, hitting_projectiles: hitting_projectiles,
        tornado_active: tornado_active, tornado_movement_on: tornado_movement_on,
        is_non_cooldown_tornado: is_non_cooldown_tornado, tornadoing_ally: tornadoing_ally, hit_ally: hit_ally,
        listening_for_ally_hits: listening_for_ally_hits, timer_valid: timer_valid, timer_at: timer_at,
        timer_set: timer_set, timer_rate: timer_rate, ally_hit_closed: ally_hit_closed,
        dog_pile_active: dog_pile_active, dog_pile_at: dog_pile_at, dog_pile_end_at: dog_pile_end_at,
        fight_cloud_hurt: fight_cloud_hurt, spawning_melee_buff_actor: spawning_melee_buff_actor,
        run_value: run_value, turning_blend: turning_blend, was_facing_left: was_facing_left,
        landing_blend: landing_blend, moving_in_air_blend: moving_in_air_blend,
        was_moving_on_ground: was_moving_on_ground, percent_charged: percent_charged, aim_direction: aim_direction,
    );
    r
}

/// [`taz_put`]'s inverse but the tornado buff (left `None` for [`taz_refs_load`]).
pub fn taz_load(s: &mut TazState, r: &TazRecord, l: &mut Lx) {
    s.eaten_fighters = l.at("eaten_fighters", |l| {
        r.eaten_fighters
            .iter()
            .filter_map(|e| {
                Some(EatenFighter { f: l.fighter(e.f)?, v: mvs_sim::math::narrow(e.v), slot: e.slot as i32 })
            })
            .collect()
    });
    s.spit_hit = r.spit_hit.as_ref().and_then(|v| l.at("spit_hit", |l| spit_hit_load(v, l)));
    s.spit_offset = l.at("spit_offset", |l| {
        r.spit_offset.as_ref().and_then(|o| {
            Some(SpitOffset {
                vic: l.fighter(o.vic)?,
                dx: mvs_sim::math::narrow(o.dx),
                dy: mvs_sim::math::narrow(o.dy),
            })
        })
    });
    s.hit_allies = l.at("hit_allies", |l| r.hit_allies.iter().filter_map(|&a| l.fighter(a)).collect());
    s.tornado_buff = None;
    s.slot_mix.slot_mix = crate::fighter::slot_mix_load(&r.slot_mix, l);
    rec_rows!(load s, r, l;
        eat_level: eat_level, eat_free: eat_free, eat_slots: eat_slots,
        projectiles_being_eaten: projectiles_being_eaten, stored_projectile: stored_projectile,
        stored_starting_speed: stored_starting_speed, stored_max_speed: stored_max_speed,
        burping_projectile: burping_projectile, hitting_projectiles: hitting_projectiles,
        tornado_active: tornado_active, tornado_movement_on: tornado_movement_on,
        is_non_cooldown_tornado: is_non_cooldown_tornado, tornadoing_ally: tornadoing_ally, hit_ally: hit_ally,
        listening_for_ally_hits: listening_for_ally_hits, timer_valid: timer_valid, timer_at: timer_at,
        timer_set: timer_set, timer_rate: timer_rate, ally_hit_closed: ally_hit_closed,
        dog_pile_active: dog_pile_active, dog_pile_at: dog_pile_at, dog_pile_end_at: dog_pile_end_at,
        fight_cloud_hurt: fight_cloud_hurt, spawning_melee_buff_actor: spawning_melee_buff_actor,
        run_value: run_value, turning_blend: turning_blend, was_facing_left: was_facing_left,
        landing_blend: landing_blend, moving_in_air_blend: moving_in_air_blend,
        was_moving_on_ground: was_moving_on_ground, percent_charged: percent_charged, aim_direction: aim_direction,
    );
}

/// TazState's tornado buff reference, loaded once the fighter's buffs have their serials.
pub fn taz_refs_load(f: &mut Fighter, r: &CharRecord, l: &mut Lx) {
    let CharRecord::Taz(r) = r else { return };
    let id = l.fighter(f64::from(f.id));
    let b = crate::fighter::buff_ref_load(f, id, r.tornado_buff, l);
    if let CharState::Taz(s) = &mut f.cs {
        s.tornado_buff = b;
    }
}
