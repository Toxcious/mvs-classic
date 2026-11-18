//! The shield actors as the record writes them, and the SpawnedPlatform of a
//! SpawnedPlatformParent child (the actor's `plat`, its parts by `actor` reference), which
//! every such child writes.

use std::sync::Arc;

use mvs_data::value::Value as DataValue;
use mvs_sim::actors::shield_actor::{Placed, ShieldActor, SocketPart};
use mvs_sim::actors::world_actor::WorldActor;
use mvs_sim::world::spawned_platform::{PlatformPart, SpawnedPlatform};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, as_obj, check_keys, obj, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};
use crate::parts::{data_load, data_put};

plain_struct!(Placed { "fwd" = fwd, "hx" = hx, "hy" = hy, "up" = up });

macro_rules! shield_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "activated" = activated, "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "box" = box_,
            "collision" = collision, "gateAge" = gate_age, "inside" = inside, "projectileBlocked" = projectile_blocked,
            "startTime" = start_time, "wall" = wall,
        )
    };
}

/// A shield actor's own keys and place (`a`: its world actor, for the platform).
pub fn shield_put(a: &WorldActor, s: &ShieldActor, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    shield_keys!(plain_keys put s, x, o;);
    if let Some(k) = &s.socket {
        // SocketShieldActor's own: `comps` the class's components, `t` the spawn's Transforms
        o.push(("aligned".into(), Value::Bool(k.aligned)));
        o.push(("comps".into(), static_object()));
        o.push(("socket".into(), Value::Num(f64::from(k.socket))));
        o.push(("t".into(), data_put(&DataValue::Obj((*k.t).clone()))));
    }
    o.push(("p".into(), static_object()));
    let listed = a.plat.as_ref().is_some_and(|pl| pl.listed);
    match &a.plat {
        Some(pl) => {
            o.push(("plat".into(), plat_put(pl, a.seq, x)));
            o.push(("solids".into(), if listed { parts_put(&pl.parts, a.seq, x) } else { Value::List(Vec::new()) }));
        }
        None => x.unmapped("a shield actor without its SpawnedPlatform (dead at its spawn)"),
    }
}

/// [`shield_put`]'s inverse (`plat`: its world actor's).
pub fn shield_load(
    s: &mut ShieldActor,
    plat: &mut Option<SpawnedPlatform>,
    r: &ActorRecord,
    ob: &Obj,
    l: &mut Lx,
    known: &mut Vec<&'static str>,
) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        shield_keys!(plain_keys load s, ob, l;);
        match (ob.get("socket"), ob.get("aligned"), ob.get("t")) {
            (Some(Value::Num(i)), Some(Value::Bool(al)), Some(t)) => {
                let t = match data_load(t) {
                    DataValue::Obj(p) => Arc::new(p),
                    _ => {
                        l.at("t", |l| l.miss("the socket shield's Transforms is not an object"));
                        Arc::default()
                    }
                };
                // (a whole socket index, the oracle's number)
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let socket = *i as u32;
                s.socket = Some(SocketPart { aligned: *al, socket, t });
            }
            (None, None, None) => s.socket = None,
            _ => l.miss("a socket shield's keys (socket, aligned, t) not all there"),
        }
        match ob.get("plat") {
            Some(v) => l.at("plat", |l| plat_load(plat, v, l)),
            None => l.miss("key plat absent"),
        }
        if let (Some(pl), Some(Value::List(sl))) = (plat.as_mut(), ob.get("solids")) {
            pl.listed = !sl.is_empty() || pl.parts.is_empty();
        }
    });
    known.extend(shield_keys!(plain_keys names));
    known.extend(["aligned", "comps", "socket", "t", "plat", "solids"]);
}

// ------------------------------------------------------------------ the SpawnedPlatform

macro_rules! part_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "bottom" = bottom, "dead" = dead, "dx" = dx, "dy" = dy, "hx" = hx, "hy" = hy, "ignored" = ignored,
            "ignoredMinions" ~ ignored_minions, "item" = item, "left" = left, "nav" = nav, "on" = on, "placed" = placed,
            "right" = right, "top" = top, "trace" = trace, "vx" = vx, "vy" = vy, "wall" = wall, "x" = x, "y" = y,
        )
    };
}

/// A PlatformPart as the oracle writes it: `actor` its actor by reference (the actor of oracle id `seq`), the component
/// and the ignore rule are the class's (not written).
pub(crate) fn part_put(p: &PlatformPart, seq: i32, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = Vec::new();
    o.push(("actor".into(), obj(vec![("id", Value::Num(f64::from(seq))), ("ref", Value::Str("actors".into()))])));
    part_keys!(plain_keys put p, x, o;);
    Value::Obj(Obj::new(o))
}

pub(crate) fn parts_put(ps: &[PlatformPart], seq: i32, x: &Wx) -> Value {
    Value::List(ps.iter().map(|p| part_put(p, seq, x)).collect())
}

pub(crate) fn part_load(p: &mut PlatformPart, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    part_keys!(plain_keys load p, ob, l;);
    let mut names: Vec<&str> = vec!["actor"];
    names.extend_from_slice(part_keys!(plain_keys names));
    check_keys(ob, &names, l);
}

/// A part made by a load (the actor's state not there yet): the component by its wall flag (Collision or
/// WallbounceCollision, the parent's two; a child's own box loads as the one of its flag), the ignore rule on.
pub(crate) fn part_make(v: &Value, l: &mut Lx) -> PlatformPart {
    let wall = matches!(as_obj(v, l).and_then(|o| o.get("wall")), Some(Value::Bool(true)));
    let comp = l.name(if wall { "WallbounceCollision" } else { "Collision" });
    let mut p = PlatformPart::new(comp, wall, 0.0, 0.0, &mvs_data::value::Props::new());
    part_load(&mut p, v, l);
    p
}

/// A SpawnedPlatform as the oracle writes it (the actor's `plat`; `seq` the actor's oracle id): its `collision` and
/// `wallbounce` parts (null for none), `solids` every part, `edgeAt` absent while unset.
pub fn plat_put(pl: &SpawnedPlatform, seq: i32, x: &Wx) -> Value {
    plat_put_without(pl, seq, x, None)
}

/// [`plat_put`] with the part at `skip` out of `solids` (a child's own box the parent does not list: Steven's WallBox).
pub fn plat_put_without(pl: &SpawnedPlatform, seq: i32, x: &Wx, skip: Option<u8>) -> Value {
    let part =
        |i: Option<u8>| i.and_then(|i| pl.parts.get(usize::from(i))).map_or(Value::Null, |p| part_put(p, seq, x));
    let mut o: Vec<(&str, Value)> = vec![
        ("age", pl.age.to_value(x)),
        ("collision", part(pl.collision)),
        ("collisionAt", pl.collision_at.to_value(x)),
    ];
    if let Some((ex, ey)) = pl.edge_at {
        o.push(("edgeAt", Value::List(vec![Value::Num(f64::from(ex)), Value::Num(f64::from(ey))])));
    }
    let listed: Vec<Value> = (0..pl.parts.len())
        .filter(|&i| skip.is_none_or(|k| usize::from(k) != i))
        .map(|i| part_put(&pl.parts[i], seq, x))
        .collect();
    o.push(("solids", Value::List(listed)));
    o.push(("team", pl.team.to_value(x)));
    o.push(("wallbounce", part(pl.wallbounce)));
    obj(o)
}

/// [`plat_put`]'s inverse: made where the actor has none (`listed` true, the parent's two parts the first of
/// `solids` in the constructor's order); the actor's own `solids` key is its writer's.
pub fn plat_load(pl: &mut Option<SpawnedPlatform>, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    let Some(Value::List(sl)) = ob.get("solids") else {
        l.miss("key solids absent");
        return;
    };
    let has = |k: &str| matches!(ob.get(k), Some(Value::Obj(_)));
    let fresh = pl.is_none() || pl.as_ref().is_some_and(|p| p.parts.len() != sl.len());
    let p = pl.get_or_insert_with(|| SpawnedPlatform {
        age: 0,
        team: 0,
        collision: None,
        wallbounce: None,
        parts: Vec::new(),
        listed: true,
        collision_at: -1.0,
        edge_at: None,
    });
    if fresh {
        p.parts = sl.iter().map(|v| l.at("solids", |l| part_make(v, l))).collect();
        p.collision = has("collision").then_some(0);
        p.wallbounce = has("wallbounce").then_some(u8::from(p.collision.is_some()));
    } else {
        for (i, (q, v)) in p.parts.iter_mut().zip(sl).enumerate() {
            l.at(&format!("solids[{i}]"), |l| part_load(q, v, l));
        }
    }
    // (the collision / wallbounce objects are the same parts as solids[i]: loaded through the list)
    l.at("age", |l| match ob.get("age") {
        Some(v) => p.age.load_value(v, l),
        None => l.miss("key age absent"),
    });
    l.at("team", |l| match ob.get("team") {
        Some(v) => p.team.load_value(v, l),
        None => l.miss("key team absent"),
    });
    l.at("collisionAt", |l| match ob.get("collisionAt") {
        Some(v) => p.collision_at.load_value(v, l),
        None => l.miss("key collisionAt absent"),
    });
    p.edge_at = match ob.get("edgeAt") {
        Some(Value::List(e)) => match e.as_slice() {
            [Value::Num(ex), Value::Num(ey)] => Some((mvs_sim::math::narrow(*ex), mvs_sim::math::narrow(*ey))),
            _ => {
                l.at("edgeAt", |l| l.miss("edgeAt is not [x, y]"));
                None
            }
        },
        _ => None,
    };
    check_keys(ob, &["age", "collision", "collisionAt", "edgeAt", "solids", "team", "wallbounce"], l);
}
