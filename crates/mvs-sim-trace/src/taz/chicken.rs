//! The chicken's classes as the record writes them: C015_HealPickup_C and Buff_Chicken's
//! extension.

use mvs_sim::chars::taz::chicken::{ChickenExt, HealPickup};
use mvs_sim::fighter::FighterId;
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, as_obj, check_keys, obj, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! pickup_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)* "bvx" = bvx, "bvy" = bvy, "bspin" = bspin, "collideAt" = collide_at)
    };
}

/// A C015_HealPickup_C's own keys: the body's velocity and spin, and the age its collision turns on at.
pub fn heal_pickup_put(s: &HealPickup, x: &Wx, o: &mut Vec<(String, Value)>) {
    pickup_keys!(plain_keys put s, x, o;);
}

/// [`heal_pickup_put`]'s inverse; the keys it knows go into `known`.
pub fn heal_pickup_load(s: &mut HealPickup, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    pickup_keys!(plain_keys load s, ob, l;);
    known.extend(pickup_keys!(plain_keys names));
}

/// Buff_Chicken_C's extension as the oracle writes it: `{ left, block, inst }`, `inst` a fighter or null.
pub fn chicken_put(e: &ChickenExt, x: &Wx) -> Value {
    obj(vec![
        ("left", Value::Num(f64::from(e.left))),
        ("block", Value::Bool(e.block)),
        ("inst", e.inst.map_or(Value::Null, |f| f.to_value(x))),
    ])
}

/// [`chicken_put`]'s inverse.
pub fn chicken_load(e: &mut ChickenExt, v: &Value, l: &mut Lx) {
    if *v == Value::Null {
        *e = ChickenExt::default();
        return;
    }
    let Some(o) = as_obj(v, l) else { return };
    match o.get("left") {
        Some(v) => l.at("left", |l| e.left.load_value(v, l)),
        None => l.miss("key left absent"),
    }
    match o.get("block") {
        Some(v) => l.at("block", |l| e.block.load_value(v, l)),
        None => l.miss("key block absent"),
    }
    e.inst = match o.get("inst") {
        Some(Value::Null) | None => None,
        Some(v) => l.at("inst", |l| FighterId::from_value(v, l)),
    };
    check_keys(o, &["left", "block", "inst"], l);
}
