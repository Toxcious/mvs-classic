//! `ForceField` (C021_ForceField_C) and `PerkForceField` (C021_ForceField_Perk_C), world actors
//!: their own keys and places. The field stands where it spawned (the perk's
//! rides its owner): the record's x and y, vx and vy readonly 0.

use mvs_sim::chars::black_adam::field::{ForceField, PerkForceField, Timeline};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(Timeline { "on" = on, "pos" = pos });

macro_rules! field_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "actorsThatHit" = actors_that_hit, "age" = age, "attackId" = attack_id, "facing" = facing,
            "health" = health, "hit" = hit, "hitSet" = hit_set, "inside" = inside, "lastHitTime" = last_hit_time,
            "lifeLeft" = life_left, "loopPosted" = loop_posted, "overlapsOn" = overlaps_on, "radius" = radius,
            "sphereScale" = sphere_scale, "t0" = t0, "t1" = t1, "t2" = t2,
        )
    };
}

macro_rules! perk_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "actorsThatHit" = base.actors_that_hit, "age" = base.age, "attackId" = base.attack_id,
            "facing" = base.facing, "health" = base.health, "hit" = base.hit, "hitSet" = base.hit_set,
            "inside" = base.inside, "lastHitTime" = base.last_hit_time, "lifeLeft" = base.life_left,
            "loopPosted" = base.loop_posted, "overlapsOn" = base.overlaps_on, "radius" = base.radius,
            "sphereScale" = base.sphere_scale, "t0" = base.t0, "t1" = base.t1, "t2" = base.t2,
            "blastAt" = blast_at, "blastBounds" = blast_bounds, "relX" = rel_x, "relY" = rel_y,
        )
    };
}

fn place_put(s: &ForceField, r: &mut ActorRecord) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
}

fn place_load(s: &mut ForceField, r: &ActorRecord, l: &mut Lx) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
}

/// The C021_ForceField's own keys and place.
pub fn field_put(s: &ForceField, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    place_put(s, r);
    field_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`field_put`]'s inverse.
pub fn field_load(s: &mut ForceField, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    place_load(s, r, l);
    l.at("rest", |l| {
        field_keys!(plain_keys load s, ob, l;);
    });
    known.extend(field_keys!(plain_keys names));
}

/// The C021_ForceField_Perk's own keys and place.
pub fn perk_put(s: &PerkForceField, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    place_put(&s.base, r);
    perk_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`perk_put`]'s inverse.
pub fn perk_load(s: &mut PerkForceField, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    place_load(&mut s.base, r, l);
    l.at("rest", |l| {
        perk_keys!(plain_keys load s, ob, l;);
    });
    known.extend(perk_keys!(plain_keys names));
}
