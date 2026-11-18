//! Tom and Jerry's fighter state as the record writes it (`TomAndJerryState`;
//!): a record per file (`jerry`, `tennis`, `slingshot`, `rocket`, `kicked` and `trap`
//! hold nothing), the slot mix with its timed lists, the AnimBP's arms blend, the Jerry actors the follow and the
//! champagne belong to by their ids, the live actors by reference.

use mvs_sim::chars::tom_and_jerry::TomAndJerryState;
use mvs_sim::chars::tom_and_jerry::state::{
    TomAndJerryAnim, TomAndJerryChampagneState, TomAndJerryFishingState, TomAndJerryFollowState, TomAndJerryPawnState,
};
use mvs_trace::{
    TomAndJerryAnimRecord, TomAndJerryChampagneRecord, TomAndJerryFishingRecord, TomAndJerryFollowRecord,
    TomAndJerryPawnRecord, TomAndJerryRecord,
};

use crate::conv::{rec_rows, rec_struct};
use crate::cx::{Lx, Wx};

rec_struct!(TomAndJerryFollowState => TomAndJerryFollowRecord {
    jerry_id: jerry_id, follow_y: follow_y, follow_z: follow_z, crouch_height: crouch_height,
    is_crouching: is_crouching, rel_y: rel_y, rel_z: rel_z, bound: bound, delay: delay,
});
rec_struct!(TomAndJerryChampagneState => TomAndJerryChampagneRecord {
    actor_id: actor_id, gate: gate, gate_left: gate_left, start_time: start_time, queued_facing: queued_facing,
    widget_visible: widget_visible, bottle_anim: bottle_anim, bottle_tick: bottle_tick, pop_tick: pop_tick,
});
rec_struct!(TomAndJerryFishingState => TomAndJerryFishingRecord { montage_ends: montage_ends });

/// `TomAndJerryPawnState` as the record writes it.
fn pawn_put(s: &TomAndJerryPawnState, x: &Wx) -> TomAndJerryPawnRecord {
    let m = &s.slot_mix;
    let mut r = TomAndJerryPawnRecord {
        slot_mix: crate::fighter::slot_mix_put(&m.slot_mix, x),
        slot_blend: crate::fighter::slot_blend_put(m.slot_blend.as_ref(), x),
        ..TomAndJerryPawnRecord::default()
    };
    rec_rows!(put s, x, r;
        jerry_offset_alpha: jerry_offset_alpha, pointer_visible: pointer_visible, hide_done: hide_done,
        gate_open: gate_open, spawned_jerry: spawned_jerry,
    );
    r
}

/// [`pawn_put`]'s inverse.
fn pawn_load(s: &mut TomAndJerryPawnState, r: &TomAndJerryPawnRecord, l: &mut Lx) {
    let m = &mut s.slot_mix;
    m.slot_mix = crate::fighter::slot_mix_load(&r.slot_mix, l);
    let b = m.slot_blend.get_or_insert_with(Default::default);
    l.at("slot_blend", |l| crate::fighter::slot_blend_load(b, &r.slot_blend, l));
    rec_rows!(load s, r, l;
        jerry_offset_alpha: jerry_offset_alpha, pointer_visible: pointer_visible, hide_done: hide_done,
        gate_open: gate_open, spawned_jerry: spawned_jerry,
    );
}

/// `TomAndJerryAnim` as the record writes it.
fn anim_put(s: &TomAndJerryAnim, x: &Wx) -> TomAndJerryAnimRecord {
    let mut r = TomAndJerryAnimRecord {
        arms: crate::fighter::slot_blend_put(Some(&s.arms), x),
        ..TomAndJerryAnimRecord::default()
    };
    rec_rows!(put s, x, r;
        aim_direction: aim_direction, wall_alpha: wall_alpha, turn_alpha: turn_alpha,
        ignore_wall_offset: ignore_wall_offset, rocket_control: rocket_control, override_mirror: override_mirror,
        mirror_override: mirror_override, jerry_wall_offset: jerry_wall_offset, with_jerry: with_jerry,
        old_facing_left: old_facing_left,
    );
    r
}

/// [`anim_put`]'s inverse.
fn anim_load(s: &mut TomAndJerryAnim, r: &TomAndJerryAnimRecord, l: &mut Lx) {
    l.at("arms", |l| crate::fighter::slot_blend_load(&mut s.arms, &r.arms, l));
    rec_rows!(load s, r, l;
        aim_direction: aim_direction, wall_alpha: wall_alpha, turn_alpha: turn_alpha,
        ignore_wall_offset: ignore_wall_offset, rocket_control: rocket_control, override_mirror: override_mirror,
        mirror_override: mirror_override, jerry_wall_offset: jerry_wall_offset, with_jerry: with_jerry,
        old_facing_left: old_facing_left,
    );
}

/// `TomAndJerryState` as the record writes it.
pub fn tj_put(s: &TomAndJerryState, x: &Wx) -> TomAndJerryRecord {
    let mut r =
        TomAndJerryRecord { pawn: pawn_put(&s.pawn, x), anim: anim_put(&s.anim, x), ..TomAndJerryRecord::default() };
    rec_rows!(put s, x, r;
        follow: follow, champagne: champagne, fishing: fishing, jerry_attached: jerry_attached,
        visible_cooldown: visible_cooldown, jerry_offset_override_trace: jerry_offset_override_trace,
        jerry_actor: jerry_actor, sling_aimer: sling_aimer, no_jerry_aimer: no_jerry_aimer,
        tennis_balls: tennis_balls, cork_ammo: cork_ammo,
    );
    r
}

/// [`tj_put`]'s inverse.
pub fn tj_load(s: &mut TomAndJerryState, r: &TomAndJerryRecord, l: &mut Lx) {
    l.at("pawn", |l| pawn_load(&mut s.pawn, &r.pawn, l));
    l.at("anim", |l| anim_load(&mut s.anim, &r.anim, l));
    rec_rows!(load s, r, l;
        follow: follow, champagne: champagne, fishing: fishing, jerry_attached: jerry_attached,
        visible_cooldown: visible_cooldown, jerry_offset_override_trace: jerry_offset_override_trace,
        jerry_actor: jerry_actor, sling_aimer: sling_aimer, no_jerry_aimer: no_jerry_aimer,
        tennis_balls: tennis_balls, cork_ammo: cork_ammo,
    );
}
