//! JakeHorse_Actor3 and JakeHorsePlatform as the record writes them (the platform a
//! PlatformActor: its part the actor's `plat`). The horse stands at its place (the record's x, y, vx, vy); `p` is the
//! class's static data.

use mvs_sim::actors::ActorRef;
use mvs_sim::actors::world_actor::WorldActor;
use mvs_sim::chars::jake::horse::{HORSE, JakeHorse, JakeHorsePlatform};
use mvs_sim::world::spawned_platform::SpawnedPlatform;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, plain_keys};
use crate::cx::{Lx, Wx};
use crate::shield_actor::{parts_put, plat_load, plat_put};

macro_rules! horse_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "armor" = armor, "attackId" = attack_id, "baseMaxAccel" = base_max_accel, "canHit" = can_hit,
            "canJump" = can_jump, "cruise" = cruise, "facingLeft" = facing_left, "falling" = falling, "hit" = hit,
            "hitIds" = hit_ids, "hitTimes" = hit_times, "inside" = inside, "jumpGate" = jump_gate,
            "jumpOnUi" = jump_on_ui, "jumpWait" = jump_wait, "oldFacing" = old_facing, "rider" = rider,
            "soundSide" = sound_side, "turnGate" = turn_gate, "turnWait" = turn_wait, "up" = up,
        )
    };
}

macro_rules! platform_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "dx" = dx, "dy" = dy,
        )
    };
}

/// A JakeHorse_Actor3_C as the record writes it.
pub fn horse_put(s: &JakeHorse, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(f64::from(s.vx));
    r.vy = Some(f64::from(s.vy));
    horse_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`horse_put`]'s inverse; the keys it knows go into `known`.
pub fn horse_load(s: &mut JakeHorse, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    opt_load(&mut s.vx, r.vx, "vx", l);
    opt_load(&mut s.vy, r.vy, "vy", l);
    l.at("rest", |l| {
        horse_keys!(plain_keys load s, ob, l;);
    });
    known.extend(horse_keys!(plain_keys names));
}

/// A JakeHorsePlatform_C as the record writes it (`a`: its world actor, for the platform).
pub fn platform_put(a: &WorldActor, s: &JakeHorsePlatform, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    platform_keys!(plain_keys put s, x, o;);
    o.push(("horse".into(), horse_ref_put(a, s, x)));
    o.push(("p".into(), static_object()));
    match &a.plat {
        Some(pl) => {
            o.push(("plat".into(), plat_put(pl, a.seq, x)));
            o.push(("solids".into(), parts_put(&pl.parts, a.seq, x)));
        }
        None => x.unmapped("a horse platform without its SpawnedPlatform (dead at its spawn)"),
    }
}

/// The platform's `horse`: a reference while the horse is in `World.actors`; one that left it is the object whole (the
/// oracle writes only a base actor as the 'spawned' reference), as it stood when it ended.
fn horse_ref_put(a: &WorldActor, s: &JakeHorsePlatform, x: &Wx) -> Value {
    let gone = match s.horse {
        Some(ActorRef::Actor(id)) => !x.w.actors.actors.contains(id),
        _ => false,
    };
    let (true, Some(h)) = (gone, &s.horse_gone) else { return s.horse.to_value(x) };
    let mut r = ActorRecord::default();
    let mut o = Vec::new();
    horse_put(h, x, &mut r, &mut o);
    o.push(("cls".into(), Value::Str(HORSE.into())));
    o.push(("dead".into(), Value::Bool(true)));
    o.push(("id".into(), Value::Num(f64::from(s.horse_seq))));
    o.push(("owner".into(), a.owner.to_value(x)));
    for (k, v) in [("x", h.x), ("y", h.y), ("vx", h.vx), ("vy", h.vy)] {
        o.push((k.into(), Value::Num(f64::from(v))));
    }
    Value::Obj(Obj::new(o))
}

/// The id a platform's dead horse is given: no slot of the arena is it.
const GONE_HORSE: u32 = u32::MAX - 100_000;

/// [`horse_ref_put`]'s inverse for a horse written whole: its state as it ended, its `id`.
fn horse_whole_load(s: &mut JakeHorsePlatform, h: &Obj, l: &mut Lx) {
    let num = |k: &str| match h.get(k) {
        Some(Value::Num(n)) => Some(*n),
        _ => None,
    };
    let r = ActorRecord { x: num("x"), y: num("y"), vx: num("vx"), vy: num("vy"), ..ActorRecord::default() };
    let mut g = JakeHorse::default();
    let mut known = Vec::new();
    horse_load(&mut g, &r, h, l, &mut known);
    known.extend(["cls", "dead", "id", "owner", "p", "x", "y", "vx", "vy"]);
    crate::conv::check_keys(h, &known, l);
    s.horse_gone = Some(g);
    match num("id") {
        Some(n) => s.horse_seq = crate::conv::int(n, l).unwrap_or(-1),
        None => l.miss("a dead horse with no id"),
    }
    s.horse = Some(ActorRef::Actor(mvs_sim::actors::ActorId::new(GONE_HORSE, 0)));
}

/// [`platform_put`]'s inverse; the keys it knows go into `known`.
pub fn platform_load(
    s: &mut JakeHorsePlatform,
    plat: &mut Option<SpawnedPlatform>,
    r: &ActorRecord,
    ob: &Obj,
    l: &mut Lx,
    known: &mut Vec<&'static str>,
) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        platform_keys!(plain_keys load s, ob, l;);
        match ob.get("horse") {
            // A horse written whole (it left World.actors): the platform's reference is an id of no live slot (the
            // writer finds it gone and writes `horse_gone` whole).
            Some(Value::Obj(h)) if h.get("ref").is_none() => l.at("horse", |l| horse_whole_load(s, h, l)),
            Some(v) => {
                s.horse_gone = None;
                l.at("horse", |l| s.horse.load_value(v, l));
            }
            None => l.miss("key horse absent"),
        }
        match ob.get("plat") {
            Some(v) => l.at("plat", |l| plat_load(plat, v, l)),
            None => l.miss("key plat absent"),
        }
    });
    known.extend(platform_keys!(plain_keys names));
    known.extend(["horse", "plat", "solids"]);
}
