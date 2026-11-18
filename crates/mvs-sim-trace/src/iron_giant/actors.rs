//! Iron Giant's actors as the record writes them: Buff_C017_AllyBurst's extension and
//! the thrown scrap (a projectile's `ext`).

use mvs_sim::chars::iron_giant::actors::{AllyBurst, ThrownScrap};
use mvs_sim::fighter::FighterId;
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, check_keys, plain_keys};
use crate::cx::{Lx, Wx};

fn by_value(o: &Obj, l: &mut Lx) -> Option<FighterId> {
    match o.get("by") {
        Some(Value::Null) | None => None,
        Some(v) => l.at("by", |l| Plain::from_value(v, l)),
    }
}

/// Buff_C017_AllyBurst's extension as the oracle writes it (`null` before it has one).
pub fn burst_put(e: &AllyBurst, x: &Wx) -> Value {
    if !e.begun {
        return Value::Null;
    }
    Value::Obj(Obj::new(vec![
        ("by".to_owned(), e.by.map_or(Value::Null, |f| f.to_value(x))),
        ("wait".to_owned(), Value::Num(f64::from(e.wait))),
        ("damage".to_owned(), Value::Num(f64::from(e.damage))),
        ("baseKb".to_owned(), Value::Num(f64::from(e.base_kb))),
    ]))
}

/// [`burst_put`]'s inverse.
pub fn burst_load(e: &mut AllyBurst, v: &Value, l: &mut Lx) {
    match v {
        Value::Null => *e = AllyBurst::default(),
        Value::Obj(o) => {
            e.begun = true;
            e.by = by_value(o, l);
            plain_keys!(load e, o, l; "wait" = wait, "damage" = damage, "baseKb" = base_kb);
            check_keys(o, &["by", "wait", "damage", "baseKb"], l);
        }
        _ => l.miss(format_args!("Buff_C017_AllyBurst's extension is neither null nor an object")),
    }
}

/// The keys the thrown scrap adds to its projectile's `rest`.
pub fn scrap_put(p: &ThrownScrap, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put p, x, o; "mesh" = mesh, "spent" = spent);
}

/// [`scrap_put`]'s inverse.
pub fn scrap_load(p: &mut ThrownScrap, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load p, ob, l; "mesh" = mesh, "spent" = spent);
    known.extend(plain_keys!(names "mesh" = mesh, "spent" = spent));
}
