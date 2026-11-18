//! The extensions of the shared force-move buffs (`combat::dynamic_move`, `combat::move_to_actor_2`) as the oracle
//! writes them: one object of the oracle's keys. A child class of the dynamic move chain writes its own keys into the
//! same object (the oracle's subclass of the extension): [`dynamic_move_put`] and [`dynamic_move_load`] add them by the
//! child's variant.

use mvs_sim::combat::dynamic_move::{DynamicMove, DynamicMoveChild};
use mvs_sim::combat::move_to_actor_2::MoveToActor2;
use mvs_trace::{Obj, Value};

use crate::conv::{as_obj, check_keys, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

/// Buff_C018ProjectileWarpMove's and WarpEnemy's own keys, all written.
macro_rules! warp_keys {
    (names) => {
        plain_keys!(names "sx" = sx, "sy" = sy, "maxDist" = max_dist, "minT" = min_t, "maxT" = max_t, "inst" = inst, "attackId" = attack_id)
    };
    ($mode:ident $($a:ident),*;) => {
        plain_keys!($mode $($a),*; "sx" = sx, "sy" = sy, "maxDist" = max_dist, "minT" = min_t, "maxT" = max_t, "inst" = inst, "attackId" = attack_id)
    };
}

macro_rules! dynamic_move_keys {
    (names) => {
        plain_keys!(names
            "dest" = dest,
            "dur" = dur,
            "easeIn" = ease_in,
            "easeOut" = ease_out,
            "launch" = launch,
            "lvx" = lvx,
            "lvy" = lvy,
            "start" = start,
            "id" = id,
            "ox" = ox,
            "oy" = oy,
            "armAt" = arm_at,
            "calls" = calls,
            "endAt" = end_at,
        )
    };
    ($mode:ident $($a:ident),*;) => {
        plain_keys!($mode $($a),*;
            "dest" = dest,
            "dur" = dur,
            "easeIn" = ease_in,
            "easeOut" = ease_out,
            "launch" = launch,
            "lvx" = lvx,
            "lvy" = lvy,
            "start" = start,
            "id" = id,
            "ox" = ox,
            "oy" = oy,
            "armAt" = arm_at,
            "calls" = calls,
            "endAt" = end_at,
        )
    };
}

/// Buff_DynamicMoveToActor's extension and its child's keys.
pub fn dynamic_move_put(e: &DynamicMove, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = Vec::new();
    dynamic_move_keys!(put e, x, o;);
    match &e.child {
        DynamicMoveChild::None => {}
        DynamicMoveChild::C018Warp(c) => {
            warp_keys!(put c, x, o;);
        }
    }
    Value::Obj(Obj::new(o))
}

/// [`dynamic_move_put`]'s inverse.
pub fn dynamic_move_load(e: &mut DynamicMove, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    // The child's fields are the record's: Marvin's warp buffs write theirs (`sx` among them), the others none.
    let warp = ob.get("sx").is_some();
    match (&e.child, warp) {
        (DynamicMoveChild::None, true) => e.child = DynamicMoveChild::C018Warp(Default::default()),
        (DynamicMoveChild::C018Warp(_), false) => e.child = DynamicMoveChild::None,
        _ => {}
    }
    dynamic_move_keys!(load e, ob, l;);
    let mut known: Vec<&'static str> = dynamic_move_keys!(names).to_vec();
    match &mut e.child {
        DynamicMoveChild::None => {}
        DynamicMoveChild::C018Warp(c) => {
            warp_keys!(load c, ob, l;);
            known.extend_from_slice(warp_keys!(names));
        }
    }
    check_keys(ob, &known, l);
}

plain_struct!(MoveToActor2 {
    "kb" = kb,
    "buffer" = buffer,
    "began" = began,
    "dest" = dest,
    "ox" = ox,
    "oy" = oy,
    "lx" = lx,
    "ly" = ly,
    "cx" = cx,
    "cy" = cy,
    "overTime" = over_time,
    "easeIn" = ease_in,
    "easeOut" = ease_out,
    "id" = id,
    "hp" = hp,
    "pos" = pos,
    "rate" = rate,
    "playing" = playing,
    "hpAt" = hp_at,
    "reached" = reached,
});
