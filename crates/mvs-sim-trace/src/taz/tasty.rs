//! The Tasty buffs' extensions as the record writes them: on a fighter and on a spawned actor.
//! A fighter is written by reference (null: none); an actor buff's Instigator by its oracle id (-1: none).

use mvs_sim::Real;
use mvs_sim::chars::taz::tasty::{
    ApplyTastyActor, ApplyTastyExt, LickExt, TastyActor, TastyBlockExt, TastyExt, TastyOnHitActor, TastyOnHitExt,
};
use mvs_sim::fighter::FighterId;
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, as_obj, check_keys, int, obj};
use crate::cx::{Lx, Wx};

fn fighter_put(f: Option<FighterId>, x: &Wx) -> Value {
    f.map_or(Value::Null, |f| f.to_value(x))
}

fn fighter_load(o: &Obj, k: &str, l: &mut Lx) -> Option<FighterId> {
    match o.get(k) {
        Some(Value::Null) => None,
        Some(v) => l.at(k, |l| FighterId::from_value(v, l)),
        None => {
            l.miss(format_args!("key {k} absent"));
            None
        }
    }
}

/// A key that must be there.
fn need<T: Plain>(o: &Obj, k: &str, t: &mut T, l: &mut Lx) {
    match o.get(k) {
        Some(v) => l.at(k, |l| t.load_value(v, l)),
        None => l.miss(format_args!("key {k} absent")),
    }
}

/// Buff_Tasty_C's extension as the oracle writes it: `{ inst }`.
pub fn tasty_put(e: &TastyExt, x: &Wx) -> Value {
    obj(vec![("inst", fighter_put(e.inst, x))])
}

/// [`tasty_put`]'s inverse.
pub fn tasty_load(e: &mut TastyExt, v: &Value, l: &mut Lx) {
    if *v == Value::Null {
        *e = TastyExt::default();
        return;
    }
    let Some(o) = as_obj(v, l) else { return };
    e.inst = fighter_load(o, "inst", l);
    check_keys(o, &["inst"], l);
}

/// Buff_ApplyTasty_C's extension: none (the buff ends in its BeginPlay; undefined, null in the record).
pub fn apply_tasty_put(_e: &ApplyTastyExt, _x: &Wx) -> Value {
    Value::Null
}

/// [`apply_tasty_put`]'s inverse.
pub fn apply_tasty_load(e: &mut ApplyTastyExt, v: &Value, l: &mut Lx) {
    *e = ApplyTastyExt::default();
    if *v != Value::Null {
        l.miss(format_args!("Buff_ApplyTasty_C's extension is not null"));
    }
}

/// Buff_TastyOnHit_C's extension as the oracle writes it: `{ inst, open }`.
pub fn tasty_on_hit_put(e: &TastyOnHitExt, x: &Wx) -> Value {
    obj(vec![("inst", fighter_put(e.inst, x)), ("open", Value::Bool(e.open))])
}

/// [`tasty_on_hit_put`]'s inverse.
pub fn tasty_on_hit_load(e: &mut TastyOnHitExt, v: &Value, l: &mut Lx) {
    if *v == Value::Null {
        *e = TastyOnHitExt::default();
        return;
    }
    let Some(o) = as_obj(v, l) else { return };
    e.inst = fighter_load(o, "inst", l);
    need(o, "open", &mut e.open, l);
    check_keys(o, &["inst", "open"], l);
}

/// Buff_TastyBlock_C's extension as the oracle writes it: `{ due, duration }`.
pub fn tasty_block_put(e: &TastyBlockExt, _x: &Wx) -> Value {
    obj(vec![("due", Value::Num(f64::from(e.due))), ("duration", Value::Num(f64::from(e.duration)))])
}

/// [`tasty_block_put`]'s inverse.
pub fn tasty_block_load(e: &mut TastyBlockExt, v: &Value, l: &mut Lx) {
    if *v == Value::Null {
        *e = TastyBlockExt::default();
        return;
    }
    let Some(o) = as_obj(v, l) else { return };
    need(o, "due", &mut e.due, l);
    need(o, "duration", &mut e.duration, l);
    check_keys(o, &["due", "duration"], l);
}

const LICK_KEYS: &[&str] = &["inst", "setup", "on", "canApply", "team", "inside", "allow"];

/// Buff_c015_ApplybuffOnOverlap_C's extension as the oracle writes it: `{ inst, setup, on, canApply, team, inside,
/// allow }`, `inside` the fighters' oracle ids.
pub fn lick_put(e: &LickExt, x: &Wx) -> Value {
    obj(vec![
        ("inst", fighter_put(e.inst, x)),
        ("setup", Value::Num(f64::from(e.setup))),
        ("on", Value::Bool(e.on)),
        ("canApply", Value::Bool(e.can_apply)),
        ("team", Value::Num(f64::from(e.team))),
        ("inside", Value::List(e.inside.iter().map(|&i| Value::Num(f64::from(i))).collect())),
        ("allow", Value::Num(f64::from(e.allow))),
    ])
}

/// [`lick_put`]'s inverse.
pub fn lick_load(e: &mut LickExt, v: &Value, l: &mut Lx) {
    if *v == Value::Null {
        *e = LickExt::default();
        return;
    }
    let Some(o) = as_obj(v, l) else { return };
    e.inst = fighter_load(o, "inst", l);
    need(o, "setup", &mut e.setup, l);
    need(o, "on", &mut e.on, l);
    need(o, "canApply", &mut e.can_apply, l);
    need(o, "team", &mut e.team, l);
    need(o, "inside", &mut e.inside, l);
    need(o, "allow", &mut e.allow, l);
    check_keys(o, LICK_KEYS, l);
}

/// Buff_ApplyTasty_C's extension on an actor: none (the buff ends in its BeginPlay; undefined, not written).
pub fn apply_tasty_actor_put(_e: &ApplyTastyActor, _x: &Wx) -> Value {
    Value::Null
}

/// [`apply_tasty_actor_put`]'s inverse.
pub fn apply_tasty_actor_load(v: &Value, l: &mut Lx) -> ApplyTastyActor {
    if *v != Value::Null {
        l.miss(format_args!("Buff_ApplyTasty_C's extension on an actor is not null"));
    }
    ApplyTastyActor::default()
}

/// An Instigator's oracle id (`-1`: none).
fn id_load(o: &Obj, k: &str, l: &mut Lx) -> i32 {
    let mut n: Real = -1.0;
    need(o, k, &mut n, l);
    int(f64::from(n), l).unwrap_or(-1)
}

/// Buff_Tasty_C's extension on an actor as the oracle writes it: `{ inst }`, the Instigator's id.
pub fn tasty_actor_put(e: &TastyActor, _x: &Wx) -> Value {
    obj(vec![("inst", Value::Num(f64::from(e.inst)))])
}

/// [`tasty_actor_put`]'s inverse.
pub fn tasty_actor_load(v: &Value, l: &mut Lx) -> TastyActor {
    let Some(o) = as_obj(v, l) else { return TastyActor { inst: -1 } };
    let inst = id_load(o, "inst", l);
    check_keys(o, &["inst"], l);
    TastyActor { inst }
}

/// Buff_TastyOnHit_C's extension on an actor as the oracle writes it: `{ inst, open }`, the Instigator's id.
pub fn tasty_on_hit_actor_put(e: &TastyOnHitActor, _x: &Wx) -> Value {
    obj(vec![("inst", Value::Num(f64::from(e.inst))), ("open", Value::Bool(e.open))])
}

/// [`tasty_on_hit_actor_put`]'s inverse.
pub fn tasty_on_hit_actor_load(v: &Value, l: &mut Lx) -> TastyOnHitActor {
    let Some(o) = as_obj(v, l) else { return TastyOnHitActor { inst: -1, open: false } };
    let inst = id_load(o, "inst", l);
    let mut open = false;
    need(o, "open", &mut open, l);
    check_keys(o, &["inst", "open"], l);
    TastyOnHitActor { inst, open }
}
