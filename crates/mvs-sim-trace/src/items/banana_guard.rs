//! ItemHelper_BananaGuard_C, the Helper's Character-like helper as the record writes it.

use mvs_collision::Ground;
use mvs_sim::fighter::body::PlatformRef;
use mvs_sim::items::banana_guard::{BananaGuard, GuardMode, GuardSolid};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::conv::{Plain, plain_keys};
use crate::cx::{Lx, Wx};

crate::conv::named_enum!(GuardMode, "banana guard movement mode", [
    GuardMode::None => "none", GuardMode::Walk => "walk", GuardMode::Fall => "fall",
]);

/// The movement base: the stage block (`World.ground`, its four sides), a world actor's or a projectile's box written
/// whole (as `PlatformRef` writes it). A stage solid's box of `staticSolids` has no rule yet: the oracle writes the
/// SolidBox object of that list.
impl Plain for GuardSolid {
    fn to_value(&self, x: &Wx) -> Value {
        match *self {
            GuardSolid::Ground => x.w.stage.ground.to_value(x),
            GuardSolid::Part(r) => r.to_value(x),
            GuardSolid::Static(_) => {
                x.unmapped("a BananaGuard base on a stage solid's box (no rule to write it)");
                Value::Null
            }
        }
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(r) = Self::from_value(v, l) {
            *self = r;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        // The stage block: its four sides alone.
        if let Value::Obj(ob) = v
            && ob.len() == 4
            && ["bottom", "left", "right", "top"].iter().all(|k| ob.get(k).is_some())
        {
            let _ = Ground::from_value(v, l)?;
            return Some(GuardSolid::Ground);
        }
        PlatformRef::from_value(v, l).map(GuardSolid::Part)
    }
}

/// `{static: "object"}` for the SpearHitBox component the guard holds, `null` without the data's track.
fn spear_put(on: bool) -> Value {
    if on { crate::actors::static_object() } else { Value::Null }
}

/// A BananaGuard as the record writes it.
pub fn banana_guard_put(s: &BananaGuard, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    // (a DealerActor: vx / vy readonly 0)
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    r.hit_actors = crate::actors::hit_actors_put(&s.hit_actors, x);
    plain_keys!(put s, x, o;
        "age" = age, "airControl" = air_control, "anim" = anim, "animRel" = anim_rel, "animTime" = anim_time,
        "attackId" = attack_id, "base" = base, "baseX" = base_x, "baseY" = base_y, "brakingDecel" = braking_decel,
        "brakingFriction" = braking_friction, "facingLeft" = facing_left, "gravity" = gravity,
        "groundFriction" = ground_friction, "half" = half, "hit" = hit, "hitOn" = hit_on, "hitSet" = hit_set,
        "ignored" = ignored, "input" = input, "lifeAt" = life_at, "maxAccel" = max_accel, "maxWalk" = max_walk,
        "meshScale" = mesh_scale, "meshZ" = mesh_z, "mode" = mode, "moveAlpha" = move_alpha, "moveIn" = move_in,
        "perch" = perch, "radius" = radius, "runAt" = run_at, "runGate" = run_gate, "stepHeight" = step_height,
        "stopAt" = stop_at, "team" = team, "tickedAt" = ticked_at, "velX" = vel_x, "velY" = vel_y,
        "walkOffLedges" = walk_off_ledges,
    );
    // (`facing`: readonly 1, the DealerActor's side)
    o.push(("facing".into(), Value::Num(1.0)));
    o.push(("spear".into(), spear_put(s.spear)));
    // (`p`: the class's parameters, static data)
    o.push(("p".into(), crate::actors::static_object()));
}

/// [`banana_guard_put`]'s inverse; the keys it knows go into `known`.
pub fn banana_guard_load(s: &mut BananaGuard, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    if let Some(v) = r.x {
        s.x = mvs_sim::math::narrow(v);
    }
    if let Some(v) = r.y {
        s.y = mvs_sim::math::narrow(v);
    }
    l.at("hit_actors", |l| crate::actors::hit_actors_load(&mut s.hit_actors, &r.hit_actors, l));
    plain_keys!(load s, ob, l;
        "age" = age, "airControl" = air_control, "anim" = anim, "animRel" = anim_rel, "animTime" = anim_time,
        "attackId" = attack_id, "base" = base, "baseX" = base_x, "baseY" = base_y, "brakingDecel" = braking_decel,
        "brakingFriction" = braking_friction, "facingLeft" = facing_left, "gravity" = gravity,
        "groundFriction" = ground_friction, "half" = half, "hit" = hit, "hitOn" = hit_on, "hitSet" = hit_set,
        "ignored" = ignored, "input" = input, "lifeAt" = life_at, "maxAccel" = max_accel, "maxWalk" = max_walk,
        "meshScale" = mesh_scale, "meshZ" = mesh_z, "mode" = mode, "moveAlpha" = move_alpha, "moveIn" = move_in,
        "perch" = perch, "radius" = radius, "runAt" = run_at, "runGate" = run_gate, "stepHeight" = step_height,
        "stopAt" = stop_at, "team" = team, "tickedAt" = ticked_at, "velX" = vel_x, "velY" = vel_y,
        "walkOffLedges" = walk_off_ledges,
    );
    s.spear = !matches!(ob.get("spear"), None | Some(Value::Null));
    known.extend(["facing", "spear", "p"]);
    known.extend(plain_keys!(names
        "age" = age, "airControl" = air_control, "anim" = anim, "animRel" = anim_rel, "animTime" = anim_time,
        "attackId" = attack_id, "base" = base, "baseX" = base_x, "baseY" = base_y, "brakingDecel" = braking_decel,
        "brakingFriction" = braking_friction, "facingLeft" = facing_left, "gravity" = gravity,
        "groundFriction" = ground_friction, "half" = half, "hit" = hit, "hitOn" = hit_on, "hitSet" = hit_set,
        "ignored" = ignored, "input" = input, "lifeAt" = life_at, "maxAccel" = max_accel, "maxWalk" = max_walk,
        "meshScale" = mesh_scale, "meshZ" = mesh_z, "mode" = mode, "moveAlpha" = move_alpha, "moveIn" = move_in,
        "perch" = perch, "radius" = radius, "runAt" = run_at, "runGate" = run_gate, "stepHeight" = step_height,
        "stopAt" = stop_at, "team" = team, "tickedAt" = ticked_at, "velX" = vel_x, "velY" = vel_y,
        "walkOffLedges" = walk_off_ledges,
    ));
}
