//! `Bolt` (C021_Bolt_C) and `SmashIn` (C021_EarthSmashIn_C), world actors, and `Pillar`, the extension of
//! Buff_C021_EarthSmashVictim: their own keys. The bolt stands at its place (x, y, vx and
//! vy readonly 0); the smash is dead at its spawn and never recorded.

use mvs_sim::chars::black_adam::bolt::{Bolt, Pillar, SmashIn};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, check_keys, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! bolt_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "attackId" = attack_id, "blastAt" = blast_at, "blastBounds" = blast_bounds,
            "facing" = facing, "ignore" = ignore, "traceFrom" = trace_from,
        )
    };
}

/// The C021_Bolt's own keys and place. The oracle's `BOLT_HIT` has 17 keys: the packet's unset members are left out.
pub fn bolt_put(s: &Bolt, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    bolt_keys!(plain_keys put s, x, o;);
    let hit = match mvs_sim::chars::black_adam::bolt::bolt_hit().to_value(x) {
        Value::Obj(h) => Value::Obj(Obj::new(
            h.entries()
                .iter()
                .filter(|(_, v)| !matches!(v, Value::Null))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )),
        v => v,
    };
    o.push(("hit".into(), hit));
    o.push(("p".into(), static_object()));
}

/// [`bolt_put`]'s inverse.
pub fn bolt_load(s: &mut Bolt, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        bolt_keys!(plain_keys load s, ob, l;);
    });
    known.extend(bolt_keys!(plain_keys names));
    known.push("hit");
}

/// The C021_EarthSmashIn's own keys and place: it is dead at its spawn, so no trace holds one.
pub fn smash_put(_s: &SmashIn, x: &Wx, _r: &mut ActorRecord, _o: &mut Vec<(String, Value)>) {
    x.unmapped("a C021_EarthSmashIn record (the actor is dead at its spawn)");
}

/// [`smash_put`]'s inverse.
pub fn smash_load(_s: &mut SmashIn, _r: &ActorRecord, _ob: &Obj, l: &mut Lx, _known: &mut Vec<&'static str>) {
    l.miss(format_args!("a C021_EarthSmashIn record (the actor is dead at its spawn)"));
}

/// The victim buff's extension (`buff.ext`): none while Pillar Source Fighter is unset.
pub fn pillar_put(e: &Pillar, x: &Wx) -> Value {
    if e.by.is_none() {
        return Value::Null;
    }
    let mut o: Vec<(String, Value)> = Vec::new();
    macro_rules! k {
        ($($k:literal = $f:ident),* $(,)?) => { $( o.push(($k.to_owned(), e.$f.to_value(x))); )* };
    }
    k!(
        "angle" = angle,
        "burst" = burst,
        "by" = by,
        "endAt" = end_at,
        "fireAt" = fire_at,
        "ny" = ny,
        "nz" = nz,
        "pct" = pct,
        "pending" = pending,
        "quake" = quake,
        "warned" = warned,
        "x" = x,
        "y" = y
    );
    if let Some(d) = &e.dodge {
        o.push(("dodge".to_owned(), d.to_value(x)));
    }
    Value::Obj(Obj::new(o))
}

fn key<T: Plain>(o: &Obj, k: &str, set: &mut T, l: &mut Lx) {
    match o.get(k) {
        Some(v) => l.at(k, |l| set.load_value(v, l)),
        None => l.miss(format_args!("key {k} absent")),
    }
}

/// [`pillar_put`]'s inverse.
pub fn pillar_load(e: &mut Pillar, v: &Value, l: &mut Lx) {
    match v {
        Value::Null => *e = Pillar::default(),
        Value::Obj(o) => {
            key(o, "angle", &mut e.angle, l);
            key(o, "burst", &mut e.burst, l);
            key(o, "by", &mut e.by, l);
            key(o, "endAt", &mut e.end_at, l);
            key(o, "fireAt", &mut e.fire_at, l);
            key(o, "ny", &mut e.ny, l);
            key(o, "nz", &mut e.nz, l);
            key(o, "pct", &mut e.pct, l);
            key(o, "pending", &mut e.pending, l);
            key(o, "quake", &mut e.quake, l);
            key(o, "warned", &mut e.warned, l);
            key(o, "x", &mut e.x, l);
            key(o, "y", &mut e.y, l);
            e.dodge = match o.get("dodge") {
                Some(v) => l.at("dodge", |l| Plain::from_value(v, l)),
                None => None,
            };
            check_keys(
                o,
                &[
                    "angle", "burst", "by", "dodge", "endAt", "fireAt", "ny", "nz", "pct", "pending", "quake",
                    "warned", "x", "y",
                ],
                l,
            );
        }
        _ => l.miss(format_args!("an extension that is neither null nor an object")),
    }
}
