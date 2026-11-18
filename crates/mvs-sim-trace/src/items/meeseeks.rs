//! Item_Meeseek and its runner as the record writes them.

use mvs_sim::items::meeseeks::{MeeseekItem, MeeseeksRun};
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// A Item_Meeseek_C as the record writes it.
pub fn meeseek_put(s: &MeeseekItem, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "useCount" = use_count, "canBounce" = can_bounce, "bigBox" = big_box, "pvx" = pvx, "pvy" = pvy);
}

/// [`meeseek_put`]'s inverse; the keys it knows go into `known`.
pub fn meeseek_load(s: &mut MeeseekItem, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "useCount" = use_count, "canBounce" = can_bounce, "bigBox" = big_box, "pvx" = pvx, "pvy" = pvy);
    known.extend(plain_keys!(names "useCount" = use_count, "canBounce" = can_bounce, "bigBox" = big_box, "pvx" = pvx, "pvy" = pvy));
}

/// A SelfActivate_Meeseeks_Run_C as the record writes it.
pub fn run_put(s: &MeeseeksRun, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "bvx" = bvx, "bvy" = bvy, "bspin" = bspin, "facingLeft" = facing_left, "dir" = dir, "activated" = activated, "canReflect" = can_reflect, "running" = running, "anim" = anim, "kbBound" = kb_bound, "kbAt" = kb_at, "inst" = inst, "activateAt" = activate_at, "collisionAt" = collision_at, "turnClosed" = turn_closed, "turnAt" = turn_at, "endAt" = end_at, "offY" = off_y);
}

/// [`run_put`]'s inverse; the keys it knows go into `known`.
pub fn run_load(s: &mut MeeseeksRun, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "bvx" = bvx, "bvy" = bvy, "bspin" = bspin, "facingLeft" = facing_left, "dir" = dir, "activated" = activated, "canReflect" = can_reflect, "running" = running, "anim" = anim, "kbBound" = kb_bound, "kbAt" = kb_at, "inst" = inst, "activateAt" = activate_at, "collisionAt" = collision_at, "turnClosed" = turn_closed, "turnAt" = turn_at, "endAt" = end_at, "offY" = off_y);
    known.extend(plain_keys!(names "bvx" = bvx, "bvy" = bvy, "bspin" = bspin, "facingLeft" = facing_left, "dir" = dir, "activated" = activated, "canReflect" = can_reflect, "running" = running, "anim" = anim, "kbBound" = kb_bound, "kbAt" = kb_at, "inst" = inst, "activateAt" = activate_at, "collisionAt" = collision_at, "turnClosed" = turn_closed, "turnAt" = turn_at, "endAt" = end_at, "offY" = off_y));
}
