//! `Knife` (MarkTeleportProjectile_C, a projectile's `ext`): the keys a knife adds to its
//! projectile's `rest`.

use mvs_sim::chars::arya::knife::Knife;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// The keys a knife adds to its projectile's `rest`.
pub fn put(p: &Knife, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put p, x, o;
        "conds" = conds, "stuck" = stuck, "toEnemy" = to_enemy, "target" = target, "ground" = ground,
        "facingLeftAtThrow" = facing_left_at_throw, "rootPitch" ^ root_pitch, "rootYaw" = root_yaw,
        "visuals" = visuals, "faceAt" = face_at, "loop" = loop_on, "triggered" = triggered,
        "refundClosed" = refund_closed, "refundPct" = refund_pct, "warning" = warning, "warnAt" = warn_at,
        "endAt" = end_at, "dieAt" = die_at, "teleporting" = teleporting, "teleportTime" = teleport_time,
        "travelLeft" = travel_left, "cast" = cast, "castLeft" = cast_left, "doneAt" = done_at,
        "allyIn" = ally_in, "capsuleR" = capsule_r,);
}

/// [`put`]'s inverse.
pub fn load(p: &mut Knife, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load p, ob, l;
        "conds" = conds, "stuck" = stuck, "toEnemy" = to_enemy, "target" = target, "ground" = ground,
        "facingLeftAtThrow" = facing_left_at_throw, "rootPitch" ^ root_pitch, "rootYaw" = root_yaw,
        "visuals" = visuals, "faceAt" = face_at, "loop" = loop_on, "triggered" = triggered,
        "refundClosed" = refund_closed, "refundPct" = refund_pct, "warning" = warning, "warnAt" = warn_at,
        "endAt" = end_at, "dieAt" = die_at, "teleporting" = teleporting, "teleportTime" = teleport_time,
        "travelLeft" = travel_left, "cast" = cast, "castLeft" = cast_left, "doneAt" = done_at,
        "allyIn" = ally_in, "capsuleR" = capsule_r,);
    known.extend(plain_keys!(names
        "conds" = conds, "stuck" = stuck, "toEnemy" = to_enemy, "target" = target, "ground" = ground,
        "facingLeftAtThrow" = facing_left_at_throw, "rootPitch" ^ root_pitch, "rootYaw" = root_yaw,
        "visuals" = visuals, "faceAt" = face_at, "loop" = loop_on, "triggered" = triggered,
        "refundClosed" = refund_closed, "refundPct" = refund_pct, "warning" = warning, "warnAt" = warn_at,
        "endAt" = end_at, "dieAt" = die_at, "teleporting" = teleporting, "teleportTime" = teleport_time,
        "travelLeft" = travel_left, "cast" = cast, "castLeft" = cast_left, "doneAt" = done_at,
        "allyIn" = ally_in, "capsuleR" = capsule_r,));
}
