//! Stripe's saws as the record writes them: the saw's own keys, its rider (the edge rider the
//! oracle keeps under `rider`) and the C023B_AllySaw_C.

use mvs_sim::chars::stripe::saw::{AllySaw, Saw};
use mvs_sim::fighter::traces::edges::{EdgePend, EdgeRider};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, as_list, as_obj, obj, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(EdgePend { "left" = left, "made" = made, "node" = node, "own" = own });

macro_rules! rider_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "aDest" = a_dest, "active" = active, "ang" = ang, "ao1" = ao1, "attachOnStart" = attach_on_start,
            "attached" = attached, "bound" = bound, "ccw" = ccw, "clockwise" = clockwise, "cpLast" = cp_last,
            "cpNear" = cp_near, "cpNext" = cp_next, "cpPoly" = cp_poly, "ctd" = ctd, "d1" = d1, "evNext" = ev_next,
            "evStart" = ev_start, "final1" = final1, "final2" = final2, "gen" = generated, "i1" = i1,
            "init1" = init1, "init2" = init2, "io1" = io1, "iv" = iv, "lastAttached" = last_attached,
            "locd" = locd, "lop" = lop, "moveSpeed" = move_speed, "pend" = pend, "pivot" = pivot,
            "rotateAlong" = rotate_along, "rotateSpeed" = rotate_speed, "sp" = sp, "st" = st, "st2" = st2,
            "st3" = st3,
        )
    };
}

macro_rules! saw_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "attachGate" = attach_gate, "bounceGate" = bounce_gate, "bounceSoundDone" = bounce_sound_done,
            "canSetTimer" = can_set_timer, "canWallRoll" = can_wall_roll, "floorSpeed" = floor_speed,
            "groundBound" = ground_bound, "initialDirectionLeft" = initial_direction_left,
            "latentAt" = latent_at, "launchBound" = launch_bound, "lister" ~ lister,
            "maxFloorSpeed" = max_floor_speed, "onGround" = on_ground, "pausedClockwise" = paused_clockwise,
            "pausedEdgeAttach" = paused_edge_attach, "sawAttackId" = saw_attack_id, "sawHbd" = saw_hbd,
            "sawPend" = saw_pend, "seenStationary" = seen_stationary, "startAttached" = start_attached,
            "turnBound" = turn_bound,
        )
    };
}

macro_rules! ally_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "ally" = ally, "attackId" = attack_id, "dieAt" = die_at, "hit" = hit, "hitDone" = hit_done,
            "hitSet" = hit_set, "lastFacing" = last_facing, "ox" = ox, "oy" = oy, "radius" = radius,
        )
    };
}

fn rider_put(r: &EdgeRider, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = Vec::new();
    rider_keys!(plain_keys put r, x, o;);
    // every rider's Tag is None
    o.push(("tag".into(), Value::Str("None".into())));
    Value::Obj(Obj::new(o))
}

fn rider_load(r: &mut EdgeRider, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    rider_keys!(plain_keys load r, ob, l;);
    let mut names: Vec<&str> = rider_keys!(plain_keys names).to_vec();
    names.push("tag");
    crate::conv::check_keys(ob, &names, l);
}

/// A saw of Stripe as the record writes it.
pub fn saw_put(s: &Saw, x: &Wx, o: &mut Vec<(String, Value)>) {
    saw_keys!(plain_keys put s, x, o;);
    o.push(("rider".into(), rider_put(&s.rider, x)));
}

/// [`saw_put`]'s inverse; the keys it knows go into `known`.
pub fn saw_load(s: &mut Saw, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    saw_keys!(plain_keys load s, ob, l;);
    match ob.get("rider") {
        Some(v) => l.at("rider", |l| rider_load(&mut s.rider, v, l)),
        None => l.miss(format_args!("key rider absent")),
    }
    known.extend(saw_keys!(plain_keys names));
    known.push("rider");
}

/// A C023B_AllySaw_C as the record writes it.
pub fn ally_saw_put(s: &AllySaw, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    ally_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
    let sphere = obj(vec![
        ("r", Value::Num(f64::from(s.sphere_r))),
        ("x", Value::Num(f64::from(s.sphere_x))),
        ("y", Value::Num(f64::from(s.sphere_y))),
        ("z", Value::Num(0.0)),
    ]);
    o.push(("sphere".into(), Value::List(vec![sphere])));
}

/// [`ally_saw_put`]'s inverse; the keys it knows go into `known`.
pub fn ally_saw_load(s: &mut AllySaw, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        ally_keys!(plain_keys load s, ob, l;);
        match ob.get("sphere") {
            Some(v) => l.at("sphere", |l| {
                let Some([e]) = as_list(v, l) else { return };
                let Some(e) = as_obj(e, l) else { return };
                for (k, dst) in [("r", &mut s.sphere_r), ("x", &mut s.sphere_x), ("y", &mut s.sphere_y)] {
                    if let Some(n) = e.get(k) {
                        l.at(k, |l| dst.load_value(n, l));
                    }
                }
            }),
            None => l.miss(format_args!("key sphere absent")),
        }
    });
    known.extend(ally_keys!(plain_keys names));
    known.extend(["p", "sphere"]);
}
