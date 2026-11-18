//! Iron Giant's aimers and the cannon's burst as the record writes them.
//!
//! The aimers' notify (and their arrow's) is the whole property object, no `{static}` wrapper; the arrow's owner is the
//! actor's (the record's `owner`).

use std::sync::Arc;

use indexmap::IndexSet;
use mvs_data::value::Value as DataValue;
use mvs_sim::chars::iron_giant::cannon_aim::{Arrow, CannonAim, CannonBurst, DiscAim, PlasmaAim};
use mvs_sim::fighter::FighterId;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, as_obj, check_keys, obj, plain_keys};
use crate::cx::{Lx, Wx};
use crate::parts::{data_load, data_put};

fn notify_put(n: &Arc<mvs_data::value::Props>) -> Value {
    data_put(&DataValue::Obj((**n).clone()))
}

fn notify_load(n: &mut Arc<mvs_data::value::Props>, v: &Value, l: &mut Lx) {
    match data_load(v) {
        DataValue::Obj(p) => *n = Arc::new(p),
        _ => l.miss("a notify that is no object"),
    }
}

/// The arrow as the oracle writes it (`owner`: the aimer's).
fn arrow_put(a: &Arrow, owner: FighterId, x: &Wx) -> Value {
    obj(vec![
        ("ay", a.ay.to_value(x)),
        ("az", a.az.to_value(x)),
        ("facing", a.facing.to_value(x)),
        ("notify", notify_put(&a.notify)),
        ("owner", owner.to_value(x)),
        ("sy", a.sy.to_value(x)),
        ("sz", a.sz.to_value(x)),
    ])
}

/// [`arrow_put`]'s inverse (the owner is the actor's).
fn arrow_load(a: &mut Arrow, v: &Value, l: &mut Lx) {
    let Some(o) = as_obj(v, l) else { return };
    plain_keys!(load a, o, l;
        "ay" = ay, "az" = az, "facing" = facing, "sy" = sy, "sz" = sz,
    );
    match o.get("notify") {
        Some(v) => l.at("notify", |l| notify_load(&mut a.notify, v, l)),
        None => l.miss("key notify absent"),
    }
    check_keys(o, &["ay", "az", "facing", "notify", "owner", "sy", "sz"], l);
}

/// A key set as the oracle writes it: `{set: ["p12", ...]}`, the kind letter and the id.
fn key_set_put(s: &IndexSet<(u8, i32)>) -> Value {
    obj(vec![("set", Value::List(s.iter().map(|&(k, id)| Value::Str(format!("{}{id}", char::from(k)))).collect()))])
}

/// [`key_set_put`]'s inverse.
fn key_set_load(s: &mut IndexSet<(u8, i32)>, v: &Value, l: &mut Lx) {
    let Some(o) = as_obj(v, l) else { return };
    let Some(Value::List(a)) = o.get("set").filter(|_| o.len() == 1) else {
        l.miss("a key set that is no {set: [...]}");
        return;
    };
    s.clear();
    for e in a {
        let key = match e {
            Value::Str(t) => t.chars().next().filter(char::is_ascii).and_then(|c| {
                let id = t[1..].parse::<i32>().ok()?;
                Some((u8::try_from(c).ok()?, id))
            }),
            _ => None,
        };
        match key {
            Some(k) => {
                s.insert(k);
            }
            None => l.miss(format_args!("an actor key {e:?} that is no kind letter and id")),
        }
    }
}

// ---------------------------------------------------------------- C017_CannonShotAim_C

/// the cannon aim's own keys and place.
pub fn cannon_aim_put(s: &CannonAim, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    plain_keys!(put s, x, o;
        "age" = age, "facing" = facing, "facingLeft" = facing_left, "offset" = offset,
    );
    o.push(("p".into(), static_object()));
}

/// [`cannon_aim_put`]'s inverse.
pub fn cannon_aim_load(s: &mut CannonAim, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        plain_keys!(load s, ob, l;
            "age" = age, "facing" = facing, "facingLeft" = facing_left, "offset" = offset,
        );
    });
    known.extend(plain_keys!(names
        "age" = age, "facing" = facing, "facingLeft" = facing_left, "offset" = offset,
    ));
}

// ---------------------------------------------------------------- C017_EnergyDiscAimer_C

/// the disc aimer's own keys and place (`owner`: its owner, the arrow's).
pub fn disc_aim_put(s: &DiscAim, owner: FighterId, x: &Wx, _r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o;
        "active" = active, "age" = age, "attackId" = attack_id, "shot" = shot, "wait" = wait,
    );
    o.push(("arrow".into(), arrow_put(&s.arrow, owner, x)));
    o.push(("notify".into(), notify_put(&s.notify)));
    o.push(("p".into(), static_object()));
}

/// [`disc_aim_put`]'s inverse.
pub fn disc_aim_load(s: &mut DiscAim, _r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    l.at("rest", |l| {
        plain_keys!(load s, ob, l;
            "active" = active, "age" = age, "attackId" = attack_id, "shot" = shot, "wait" = wait,
        );
        match ob.get("arrow") {
            Some(v) => l.at("arrow", |l| arrow_load(&mut s.arrow, v, l)),
            None => l.miss("key arrow absent"),
        }
        match ob.get("notify") {
            Some(v) => l.at("notify", |l| notify_load(&mut s.notify, v, l)),
            None => l.miss("key notify absent"),
        }
    });
    known.extend(plain_keys!(names
        "active" = active, "age" = age, "attackId" = attack_id, "shot" = shot, "wait" = wait,
    ));
    known.extend(["arrow", "notify"]);
}

// ---------------------------------------------------------------- C017_PlasmaAimer_C

/// the plasma aimer's own keys and place (`owner`: its owner, the arrow's).
pub fn plasma_aim_put(s: &PlasmaAim, owner: FighterId, x: &Wx, _r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "attackId" = attack_id,);
    o.push(("arrow".into(), arrow_put(&s.arrow, owner, x)));
    o.push(("notify".into(), notify_put(&s.notify)));
    o.push(("p".into(), static_object()));
}

/// [`plasma_aim_put`]'s inverse.
pub fn plasma_aim_load(s: &mut PlasmaAim, _r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    l.at("rest", |l| {
        plain_keys!(load s, ob, l; "attackId" = attack_id,);
        match ob.get("arrow") {
            Some(v) => l.at("arrow", |l| arrow_load(&mut s.arrow, v, l)),
            None => l.miss("key arrow absent"),
        }
        match ob.get("notify") {
            Some(v) => l.at("notify", |l| notify_load(&mut s.notify, v, l)),
            None => l.miss("key notify absent"),
        }
    });
    known.extend(plain_keys!(names "attackId" = attack_id,));
    known.extend(["arrow", "notify"]);
}

// ---------------------------------------------------------------- C017_CannonExplotion_C

/// the cannon burst's own keys and place (`vx` and `vy` 0, readonly).
pub fn burst_put(s: &CannonBurst, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    plain_keys!(put s, x, o;
        "age" = age, "allyOver" = ally_over, "attackId" = attack_id, "facing" = facing, "hit" = hit,
        "hitSet" = hit_set, "hx" = hx, "hy" = hy, "lifeTicks" = life_ticks, "pitch" ^ pitch, "upY" = up_y,
    );
    o.push(("actorOnce".into(), key_set_put(&s.actor_once)));
    o.push(("actorOver".into(), key_set_put(&s.actor_over)));
    o.push(("p".into(), static_object()));
}

/// [`burst_put`]'s inverse.
pub fn burst_load(s: &mut CannonBurst, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        plain_keys!(load s, ob, l;
            "age" = age, "allyOver" = ally_over, "attackId" = attack_id, "facing" = facing, "hit" = hit,
            "hitSet" = hit_set, "hx" = hx, "hy" = hy, "lifeTicks" = life_ticks, "pitch" ^ pitch, "upY" = up_y,
        );
        match ob.get("actorOnce") {
            Some(v) => l.at("actorOnce", |l| key_set_load(&mut s.actor_once, v, l)),
            None => l.miss("key actorOnce absent"),
        }
        match ob.get("actorOver") {
            Some(v) => l.at("actorOver", |l| key_set_load(&mut s.actor_over, v, l)),
            None => l.miss("key actorOver absent"),
        }
    });
    known.extend(plain_keys!(names
        "age" = age, "allyOver" = ally_over, "attackId" = attack_id, "facing" = facing, "hit" = hit,
        "hitSet" = hit_set, "hx" = hx, "hy" = hy, "lifeTicks" = life_ticks, "pitch" ^ pitch, "upY" = up_y,
    ));
    known.extend(["actorOnce", "actorOver"]);
}
