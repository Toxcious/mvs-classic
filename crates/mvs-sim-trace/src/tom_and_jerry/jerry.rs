//! Jerry_SpawnedActor as the record writes it: the root Capsule's centre is the
//! record's x and y, its velocity the record's vx and vy; the elemental, charge and dodge parts are the actor row's.

use mvs_sim::actors::follower::FollowerMeshVis;
use mvs_sim::chars::tom_and_jerry::jerry::JerryActor;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! jerry_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "aimDown" = aim_down, "allyPickUpGate" = ally_pick_up_gate, "anim" = anim,
            "attackId" = attack_id, "blastAt" = blast_at, "blastBounds" = blast_bounds, "bounceCount" = bounce_count,
            "bounceGate" = bounce_gate, "bounciness" = bounciness, "champagne" = champagne, "chargeUp" = charge_up,
            "colorMap" = color_map, "colorWait" = color_wait, "conds" = conds, "damageDisabled" = damage_disabled,
            "dirWait" = dir_wait, "dmgWait" = dmg_wait, "ended" = ended, "facing" = facing, "falling" = falling,
            "fastFall" = fast_fall, "flying" = flying, "following" = following, "friction" = friction,
            "gravScale" = grav_scale, "hit" = hit, "hitOn" = hit_on, "hitR" = hit_r, "hitReact" = hit_react,
            "hitSet" = hit_set, "hitShakeT" = hit_shake_t, "hp" = hp, "ignorePlatforms" = ignore_platforms,
            "inside" = inside, "is2D" = is_2d, "isAttacking" = is_attacking, "jumpNumber" = jump_number,
            "jumping" = jumping, "kbOnce" = kb_once, "maxSpeed" = max_speed, "moving" = moving,
            "openBounce" = open_bounce, "parentVx" = parent_vx, "parentVy" = parent_vy, "pickUpGate" = pick_up_gate,
            "pickWait" = pick_wait, "prevHitTime" = prev_hit_time, "prevNx" = prev_nx, "prevNy" = prev_ny,
            "racket" = racket, "resetJerry" = reset_jerry, "rocket" = rocket, "rotPitch" = rot_pitch,
            "rotYaw" = rot_yaw, "sliding" = sliding, "tennisBall" = tennis_ball, "trail" = trail,
            "wallSliding" = wall_sliding, "wasRungOut" = was_rung_out,
        )
    };
}

/// `meshVis`: `FollowerMeshVis | 'hidden'`.
fn mesh_vis_put(v: Option<FollowerMeshVis>) -> Value {
    Value::Str(
        match v {
            Some(FollowerMeshVis::All) => "all",
            Some(FollowerMeshVis::Seers) => "seers",
            None => "hidden",
        }
        .to_owned(),
    )
}

/// A Jerry_SpawnedActor_C as the record writes it (its place is the record's x and y, its velocity vx and vy).
pub fn jerry_put(s: &JerryActor, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(f64::from(s.vx));
    r.vy = Some(f64::from(s.vy));
    jerry_keys!(plain_keys put s, x, o;);
    o.push(("meshVis".into(), mesh_vis_put(s.mesh_vis)));
    o.push(("onCamera".into(), Value::Bool(true)));
    o.push(("p".into(), static_object()));
}

/// [`jerry_put`]'s inverse; the keys it knows go into `known`.
pub fn jerry_load(s: &mut JerryActor, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    opt_load(&mut s.vx, r.vx, "vx", l);
    opt_load(&mut s.vy, r.vy, "vy", l);
    l.at("rest", |l| {
        jerry_keys!(plain_keys load s, ob, l;);
        match ob.get("meshVis") {
            Some(Value::Str(t)) if t == "all" => s.mesh_vis = Some(FollowerMeshVis::All),
            Some(Value::Str(t)) if t == "seers" => s.mesh_vis = Some(FollowerMeshVis::Seers),
            Some(Value::Str(t)) if t == "hidden" => s.mesh_vis = None,
            Some(v) => l.miss(format_args!("meshVis {v:?} is not a visibility")),
            None => l.miss(format_args!("key meshVis absent")),
        }
    });
    known.extend(jerry_keys!(plain_keys names));
    known.extend(["meshVis", "onCamera"]);
}
