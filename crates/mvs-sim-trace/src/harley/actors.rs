//! Harley Quinn's other actors as the record writes them: the jack-in-the-box (a world
//! actor) and the confetti buffs' extensions. The jack-in-the-box stands where it spawned: the record's x and y, vx and
//! vy readonly 0.

use mvs_sim::chars::harley::actors::{ConfettiCount, ConfettiExplosion, JackBox};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, check_keys, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! jack_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "active" = active, "age" = age, "allies" = allies, "attackId" = attack_id, "balloons" = balloons,
            "blastAt" = blast_at, "blastBounds" = blast_bounds, "blastR" = blast_r, "detectR" = detect_r,
            "detectY" = detect_y, "ended" = ended, "explodeAge" = explode_age, "exploded" = exploded,
            "facing" = facing, "grounded" = grounded, "hit" = hit, "hitSet" = hit_set, "inside" = inside,
            "once" = once, "over" = over, "resetAge" = reset_age, "resetFor" = reset_for, "scanAt" = scan_at,
        )
    };
}

/// The jack-in-the-box's own keys and place.
pub fn jack_put(s: &JackBox, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    jack_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`jack_put`]'s inverse.
pub fn jack_load(s: &mut JackBox, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        jack_keys!(plain_keys load s, ob, l;);
    });
    known.extend(jack_keys!(plain_keys names));
}

fn from_value(o: &Obj, l: &mut Lx) -> Option<mvs_sim::fighter::FighterId> {
    match o.get("from") {
        Some(Value::Null) | None => None,
        Some(v) => l.at("from", |l| Plain::from_value(v, l)),
    }
}

fn from_put(from: Option<mvs_sim::fighter::FighterId>, x: &Wx) -> Value {
    from.map_or(Value::Null, |f| f.to_value(x))
}

/// Buff_ConfettiCount's extension as the oracle writes it (`null` before it has one).
pub fn count_put(e: &ConfettiCount, x: &Wx) -> Value {
    if !e.begun {
        return Value::Null;
    }
    Value::Obj(Obj::new(vec![("from".to_owned(), from_put(e.from, x))]))
}

/// [`count_put`]'s inverse.
pub fn count_load(e: &mut ConfettiCount, v: &Value, l: &mut Lx) {
    match v {
        Value::Null => *e = ConfettiCount::default(),
        Value::Obj(o) => {
            e.begun = true;
            e.from = from_value(o, l);
            check_keys(o, &["from"], l);
        }
        _ => l.miss(format_args!("Buff_ConfettiCount's extension is neither null nor an object")),
    }
}

/// Buff_ConfettiExplosionCount's extension as the oracle writes it (`null` before it has one). The buff actor's dodge
/// entries live only inside the explosion's call.
pub fn explosion_put(e: &ConfettiExplosion, x: &Wx) -> Value {
    if !e.begun {
        return Value::Null;
    }
    Value::Obj(Obj::new(vec![("boom".to_owned(), Value::Bool(e.boom)), ("from".to_owned(), from_put(e.from, x))]))
}

/// [`explosion_put`]'s inverse.
pub fn explosion_load(e: &mut ConfettiExplosion, v: &Value, l: &mut Lx) {
    match v {
        Value::Null => *e = ConfettiExplosion::default(),
        Value::Obj(o) => {
            e.begun = true;
            e.from = from_value(o, l);
            e.boom = matches!(o.get("boom"), Some(Value::Bool(true)));
            check_keys(o, &["boom", "from"], l);
        }
        _ => l.miss(format_args!("Buff_ConfettiExplosionCount's extension is neither null nor an object")),
    }
}
