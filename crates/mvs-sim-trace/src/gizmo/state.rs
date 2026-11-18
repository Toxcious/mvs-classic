//! Gizmo's fighter state as the record writes it (`GizmoState`): a
//! record per file (`cars`, `music`, `popcorn` and `perks` hold nothing), the slot mix with its timed lists and
//! players, the carried ally, the last dash target and the notify's holders by their fighter ids, the cars by
//! reference, the move, share and shrink buffs by their place in their holder's buff list (-1 none, -2 ended),
//! resolved by [`resolve_gizmo_buffs`] once every fighter has its buffs (the share buff is the carried ally's).

use mvs_sim::chars::CharState;
use mvs_sim::chars::gizmo::GizmoState;
use mvs_sim::chars::gizmo::state::{GizmoAttach, GizmoPawnState};
use mvs_sim::combat::BuffRef;
use mvs_sim::fighter::FighterId;
use mvs_sim::{Fighter, World};
use mvs_trace::{CharRecord, GizmoAttachRecord, GizmoPawnRecord, GizmoRecord, WorldRecord};

use crate::conv::rec_rows;
use crate::cx::{Lx, Wx};

/// `GizmoPawnState` as the record writes it.
fn pawn_put(s: &GizmoPawnState, x: &Wx) -> GizmoPawnRecord {
    let m = &s.slot_mix;
    let mut r = GizmoPawnRecord {
        slot_mix: crate::fighter::slot_mix_put(&m.slot_mix, x),
        slot_blend: crate::fighter::slot_blend_put(m.slot_blend.as_ref(), x),
        slot_players: crate::fighter::slot_players_put(m.slot_players.as_ref(), x),
        ..GizmoPawnRecord::default()
    };
    rec_rows!(put s, x, r;
        override_fall_lean: override_fall_lean, stop_overrides: stop_overrides, rolling: rolling, turning: turning,
        driving_turning: driving_turning, turn_delay: turn_delay, com_blend_start: com_blend_start,
        com_blend_time: com_blend_time, vis_delay: vis_delay, visibility_delay: visibility_delay,
        has_max_arrow: has_max_arrow, fire_facing: fire_facing,
    );
    r
}

/// [`pawn_put`]'s inverse.
fn pawn_load(s: &mut GizmoPawnState, r: &GizmoPawnRecord, l: &mut Lx) {
    let m = &mut s.slot_mix;
    m.slot_mix = crate::fighter::slot_mix_load(&r.slot_mix, l);
    let b = m.slot_blend.get_or_insert_with(Default::default);
    l.at("slot_blend", |l| crate::fighter::slot_blend_load(b, &r.slot_blend, l));
    let p = m.slot_players.get_or_insert_with(Default::default);
    l.at("slot_players", |l| crate::fighter::slot_players_load(p, &r.slot_players, l));
    rec_rows!(load s, r, l;
        override_fall_lean: override_fall_lean, stop_overrides: stop_overrides, rolling: rolling, turning: turning,
        driving_turning: driving_turning, turn_delay: turn_delay, com_blend_start: com_blend_start,
        com_blend_time: com_blend_time, vis_delay: vis_delay, visibility_delay: visibility_delay,
        has_max_arrow: has_max_arrow, fire_facing: fire_facing,
    );
}

/// A buff reference by its place in its holder's buff list (`buffRef`: -1 none, -2 ended).
fn buff_at(b: Option<BuffRef>, x: &Wx) -> f64 {
    b.map_or(-1.0, |b| {
        x.w.fighter(b.fighter).buffs.buffs.iter().position(|e| e.serial == b.serial).map_or(-2.0, |i| i as f64)
    })
}

/// The attach record's rows but the three buffs ([`attach_put`], [`resolve_gizmo_buffs`]).
macro_rules! attach_rows {
    ($mode:ident $s:ident, $x:ident, $r:ident) => {
        rec_rows!($mode $s, $x, $r;
            attached: attached, carrying: carrying, meter: meter, off_x: off_x, off_z: off_z, rel_x: rel_x,
            rel_z: rel_z, turn_x: turn_x, turn_z: turn_z, turn_on: turn_on, turn_pos: turn_pos,
            input_bound: input_bound, ally_bound: ally_bound, buff_queue: buff_queue, queue_due: queue_due,
            timer_set: timer_set, timer_next: timer_next, collision_off: collision_off,
            notify_holders: notify_holders, dash_car: dash_car,
        )
    };
}

/// `GizmoAttach` as the record writes it.
fn attach_put(s: &GizmoAttach, x: &Wx) -> GizmoAttachRecord {
    let mut r = GizmoAttachRecord {
        move_buff: buff_at(s.move_buff, x),
        share_buff: buff_at(s.share_buff, x),
        shrink_buff: buff_at(s.shrink_buff, x),
        ..GizmoAttachRecord::default()
    };
    attach_rows!(put s, x, r);
    r
}

/// [`attach_put`]'s inverse but the buffs (left `None` for [`resolve_gizmo_buffs`]).
fn attach_load(s: &mut GizmoAttach, r: &GizmoAttachRecord, l: &mut Lx) {
    attach_rows!(load s, r, l);
    s.move_buff = None;
    s.share_buff = None;
    s.shrink_buff = None;
}

/// `GizmoState` as the record writes it.
pub fn gizmo_put(s: &GizmoState, x: &Wx) -> GizmoRecord {
    let mut r = GizmoRecord { pawn: pawn_put(&s.pawn, x), attach: attach_put(&s.attach, x), ..GizmoRecord::default() };
    rec_rows!(put s, x, r;
        car: car, drives: drives, driving: driving, com_override: com_override,
        com_override_alpha: com_override_alpha, lantern: lantern, last_dash_target: last_dash_target,
    );
    r
}

/// [`gizmo_put`]'s inverse but the attach record's buffs ([`resolve_gizmo_buffs`]).
pub fn gizmo_load(s: &mut GizmoState, r: &GizmoRecord, l: &mut Lx) {
    l.at("pawn", |l| pawn_load(&mut s.pawn, &r.pawn, l));
    l.at("attach", |l| attach_load(&mut s.attach, &r.attach, l));
    rec_rows!(load s, r, l;
        car: car, drives: drives, driving: driving, com_override: com_override,
        com_override_alpha: com_override_alpha, lantern: lantern, last_dash_target: last_dash_target,
    );
}

/// The entry at `v`'s place in `on`'s buffs (-2: an ended one, a serial no entry has).
fn buff_load(w: &World, on: FighterId, v: f64, at: &str, l: &mut Lx) -> Option<BuffRef> {
    if v == -2.0 {
        // (never handed out: serials count up from 0)
        return Some(BuffRef { fighter: on, serial: u32::MAX - 1 });
    }
    let f: &Fighter = w.fighter(on);
    match f.buffs.buffs.get(v as usize) {
        Some(e) => Some(BuffRef { fighter: on, serial: e.serial }),
        None => {
            l.at(at, |l| l.miss(format_args!("buff reference {v} past its holder's buffs")));
            None
        }
    }
}

/// Each Gizmo's attach buffs from its record, once every fighter of the World has its buffs (their serials): the
/// move and shrink buffs in his own list, the share buff in the carried ally's.
pub fn resolve_gizmo_buffs(w: &mut World, r: &WorldRecord, l: &mut Lx) {
    let ids: Vec<_> = w.pawns.clone().unwrap_or_else(|| w.players.clone());
    for (i, (&id, fr)) in ids.iter().zip(&r.fighters).enumerate() {
        let CharRecord::Gizmo(g) = &fr.state else { continue };
        let a = &g.attach;
        let at = |k: &str| format!("fighters[{i}].state.attach.{k}");
        let mv = (a.move_buff != -1.0).then(|| buff_load(w, id, a.move_buff, &at("move_buff"), l)).flatten();
        let shrink = (a.shrink_buff != -1.0).then(|| buff_load(w, id, a.shrink_buff, &at("shrink_buff"), l)).flatten();
        let share = if a.share_buff == -1.0 {
            None
        } else {
            match l.at(&at("share_buff"), |l| l.fighter(a.carrying)) {
                Some(on) => buff_load(w, on, a.share_buff, &at("share_buff"), l),
                None => {
                    l.at(&at("share_buff"), |l| l.miss("a share buff with no carried ally"));
                    None
                }
            }
        };
        if let CharState::Gizmo(s) = &mut w.fighter_mut(id).cs {
            s.attach.move_buff = mv;
            s.attach.shrink_buff = shrink;
            s.attach.share_buff = share;
        }
    }
}
