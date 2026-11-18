//! Iron Giant's actor classes as the record writes them: the world actors
//! AntenaBolts, CannonBall, TurretAim, StompWave and DownThruster, and the projectile subclasses Bomb, DiscShot and
//! Missile (a projectile's `ext`).
//!
//! The bolts, the stomp wave and the thruster stand still as the record has them (vx and vy readonly 0); the cannon
//! ball's place is its owner's (getters, not own keys: its x and y come from the owner's), the turret aim has none.
//! The bolts' `buff` is the whole buff entry of the owner's list the oracle keeps, written through the reference and
//! read back as a marker [`resolve_bolt_buffs`] turns into the owner's Buff_C017_Passive_C.

use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::chars::iron_giant::actor_state::{
    AntenaBolts, Bomb, CannonBall, DiscShot, DownThruster, Missile, StompWave, TurretAim, ball_place,
};
use mvs_sim::combat::BuffRef;
use mvs_sim::world::World;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, spheres_load, spheres_put, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// The serial a loaded `buff` waits with.
const PENDING_BOLT_BUFF: u32 = u32::MAX - 5;
/// The serial of a loaded `buff` gone from its fighter's list (ended, swept).
const GONE_BOLT_BUFF: u32 = u32::MAX - 6;
const BOLT_BUFF: &str = "Buff_C017_Passive_C";

macro_rules! antena_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "alpha" = alpha, "allies" = allies, "attackId" = attack_id, "bolts" = bolts,
            "bound" = bound, "dying" = dying, "facing" = facing, "hit" = hit, "kbSeq" = kb_seq, "radius" = radius,
            "relY" = rel_y, "relZ" = rel_z, "ring" = ring, "spin" = spin, "start" = start,
        )
    };
}

/// the bolts's own keys and place.
pub fn antena_put(s: &AntenaBolts, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    antena_keys!(plain_keys put s, x, o;);
    // (the socket is a box index, a number: u32's Plain is a montage instance's)
    if let Some(si) = s.socket {
        o.push(("socket".into(), Value::Num(f64::from(si))));
    }
    o.push(("buff".into(), crate::black_adam::chain::buff_entry_put(s.buff, x)));
    o.push(("p".into(), static_object()));
}

/// [`antena_put`]'s inverse.
pub fn antena_load(s: &mut AntenaBolts, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        antena_keys!(plain_keys load s, ob, l;);
        s.socket = match ob.get("socket") {
            None => None,
            Some(Value::Num(n)) => l.at("socket", |l| crate::conv::uint(*n, u32::MAX, l)),
            Some(_) => {
                l.at("socket", |l| l.miss("no box index"));
                None
            }
        };
    });
    known.extend(antena_keys!(plain_keys names));
    known.push("socket");
    known.push("buff");
    s.buff = match ob.get("buff") {
        None | Some(Value::Null) => None,
        Some(v @ Value::Obj(o)) => match l.fighter(r.owner) {
            Some(fighter) if matches!(o.get("swept"), Some(Value::Bool(true))) => {
                Some(crate::black_adam::chain::buff_entry_gone_load(v, fighter, GONE_BOLT_BUFF, l))
            }
            Some(fighter) => Some(BuffRef { fighter, serial: PENDING_BOLT_BUFF }),
            None => {
                l.miss("the bolts' buff with no owner");
                None
            }
        },
        Some(_) => {
            l.miss("the bolts' buff is no object");
            None
        }
    };
}

/// Turns the markers [`antena_load`] left into the entries of the owners' lists (the first of the class).
pub fn resolve_bolt_buffs(w: &mut World, l: &mut Lx) {
    let mut fix = Vec::new();
    let mut i = 0;
    while let Some(id) = w.actors.actors.nth(i) {
        i += 1;
        let a = w.actors.actors.get(id).expect("a live actor id");
        if let WorldActorState::AntenaBolts(s) = &a.state
            && let Some(r) = s.buff
            && r.serial == PENDING_BOLT_BUFF
        {
            match w.fighter(r.fighter).buffs.buffs.iter().find(|b| w.names.str(b.cls) == BOLT_BUFF) {
                Some(b) => fix.push((id, BuffRef { fighter: r.fighter, serial: b.serial })),
                None => l.miss(format_args!("the bolts' buff {BOLT_BUFF} is in no list of its fighter")),
            }
        }
    }
    for (id, r) in fix {
        if let Some(a) = w.actors.actors.get_mut(id)
            && let WorldActorState::AntenaBolts(s) = &mut a.state
        {
            s.buff = Some(r);
        }
    }
}

macro_rules! ball_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "facing" = facing, "offset" = offset, "relY" = rel_y, "relZ" = rel_z,
        )
    };
}

/// the cannon ball's own keys and place.
pub fn ball_put(
    s: &CannonBall,
    a: &mvs_sim::actors::WorldActor,
    x: &Wx,
    r: &mut ActorRecord,
    o: &mut Vec<(String, Value)>,
) {
    if let Some((px, py)) = ball_place(x.w, a, s) {
        r.x = Some(f64::from(px));
        r.y = Some(f64::from(py));
    }
    ball_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`ball_put`]'s inverse (its place is its owner's).
pub fn ball_load(s: &mut CannonBall, _r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    l.at("rest", |l| {
        ball_keys!(plain_keys load s, ob, l;);
    });
    known.extend(ball_keys!(plain_keys names));
}

macro_rules! aim_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "attackId" = attack_id, "gate" = gate, "offset" = offset,
        )
    };
}

/// the turret aim's own keys and place.
pub fn turret_aim_put(s: &TurretAim, x: &Wx, _r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    aim_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`turret_aim_put`]'s inverse.
pub fn turret_aim_load(s: &mut TurretAim, _r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    l.at("rest", |l| {
        aim_keys!(plain_keys load s, ob, l;);
    });
    known.extend(aim_keys!(plain_keys names));
}

macro_rules! stomp_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "attackId" = attack_id, "facing" = facing, "ground" = ground, "height" = height, "hit" = hit,
            "kbScaling" = kb_scaling, "rot" = rot, "shocks" = shocks, "stage" = stage, "then" = then,
            "wait" = wait, "placed" ~ placed,
        )
    };
}

/// the stomp wave's own keys and place.
pub fn stomp_put(s: &StompWave, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    stomp_keys!(plain_keys put s, x, o;);
    o.push(("spheres".into(), spheres_put(&s.spheres)));
    o.push(("p".into(), static_object()));
}

/// [`stomp_put`]'s inverse.
pub fn stomp_load(s: &mut StompWave, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        stomp_keys!(plain_keys load s, ob, l;);
        if let Some(v) = ob.get("spheres") {
            l.at("spheres", |l| spheres_load(&mut s.spheres, v, l));
        }
    });
    known.extend(stomp_keys!(plain_keys names));
    known.push("spheres");
}

macro_rules! thruster_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "attackId" = attack_id, "base" = base, "facing" = facing,
            "flame" = flame, "hit" = hit, "inFlame" = in_flame, "lifeTicks" = life_ticks, "nextTick" = next_tick,
            "relY" = rel_y, "relZ" = rel_z, "t0" = t0, "timerOn" = timer_on,
        )
    };
}

/// the down thruster's own keys and place. Its flame boxes are not written (the record leaves every actor's
/// `boxes` out: rebuilt by its step before a hit test reads them).
pub fn thruster_put(s: &DownThruster, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    thruster_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`thruster_put`]'s inverse.
pub fn thruster_load(s: &mut DownThruster, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        thruster_keys!(plain_keys load s, ob, l;);
    });
    known.extend(thruster_keys!(plain_keys names));
}

macro_rules! bomb_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "exploding" = exploding, "t0" = t0, "expandT" = expand_t, "nextTick" = next_tick, "sphereR" = sphere_r,
        )
    };
}

/// The keys the bomb adds to its projectile's `rest`.
pub fn bomb_put(p: &Bomb, x: &Wx, o: &mut Vec<(String, Value)>) {
    bomb_keys!(plain_keys put p, x, o;);
}

/// [`bomb_put`]'s inverse.
pub fn bomb_load(p: &mut Bomb, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    bomb_keys!(plain_keys load p, ob, l;);
    known.extend(bomb_keys!(plain_keys names));
}

/// The keys the disc shot adds to its projectile's `rest`.
pub fn disc_put(p: &DiscShot, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put p, x, o; "ownerFacing" = owner_facing);
}

/// [`disc_put`]'s inverse.
pub fn disc_load(p: &mut DiscShot, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load p, ob, l; "ownerFacing" = owner_facing);
    known.extend(plain_keys!(names "ownerFacing" = owner_facing));
}

/// The keys the missile adds to its projectile's `rest`.
pub fn missile_put(p: &Missile, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put p, x, o; "faceAtStart" = face_at_start);
}

/// [`missile_put`]'s inverse.
pub fn missile_load(p: &mut Missile, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load p, ob, l; "faceAtStart" = face_at_start);
    known.extend(plain_keys!(names "faceAtStart" = face_at_start));
}
