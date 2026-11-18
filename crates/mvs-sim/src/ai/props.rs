//! A bot tree node's instance properties (the node's tagged properties over its class defaults)
//! and the reads the node classes make of them: `p.X ?? d` takes `d` for a missing or null
//! property, `p.X === true` is a bool that is set, `if (p.X)` is JavaScript truthiness.

use crate::Real;
use mvs_data::value::{Props, Value};

/// `p[k]`: `None` for a missing or null property.
pub fn get<'a>(p: &'a Props, k: &str) -> Option<&'a Value> {
    p.get(k).filter(|v| !v.is_null())
}

/// `p[k]` as a number (`p.X ?? d` is `num(p, "X").unwrap_or(d)`).
pub fn num(p: &Props, k: &str) -> Option<Real> {
    get(p, k).and_then(Value::as_f64).map(crate::math::narrow)
}

/// `p[k] === true`.
pub fn is_true(p: &Props, k: &str) -> bool {
    matches!(get(p, k), Some(Value::Bool(true)))
}

/// `if (p[k])`: JavaScript truthiness (false, 0, NaN, '' and null are false; an object or an array is true).
pub fn truthy(p: &Props, k: &str) -> bool {
    match get(p, k) {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Value::Str(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `p[k]` as a string (an enum value is its full name: 'EBTNodeResult::Failed').
pub fn str<'a>(p: &'a Props, k: &str) -> Option<&'a str> {
    get(p, k).and_then(Value::as_str)
}

/// `p[k]?.key ?? 'None'`: the blackboard key name of a key selector property.
pub fn key<'a>(p: &'a Props, k: &str) -> &'a str {
    get(p, k).and_then(Value::as_obj).and_then(|o| o.get("key")).and_then(Value::as_str).unwrap_or("None")
}

/// `p[k]` as an array of numbers (an array's entries that are no number are left out).
pub fn nums<'a>(p: &'a Props, k: &str) -> impl Iterator<Item = Real> + 'a {
    get(p, k).and_then(Value::as_arr).unwrap_or(&[]).iter().filter_map(Value::as_f64).map(crate::math::narrow)
}

/// `p[k]?.find((e) => e.Key === key)?.Value`: a TMap property (an array of `{ Key, Value }`) at `key`.
pub fn map_find(p: &Props, k: &str, key: &str) -> Option<Real> {
    get(p, k)?
        .as_arr()?
        .iter()
        .filter_map(Value::as_obj)
        .find(|e| e.get("Key").and_then(Value::as_str) == Some(key))?
        .get("Value")
        .and_then(Value::as_f64)
        .map(crate::math::narrow)
}

/// `instanceProps(base, over)`: a node instance's properties. An instance is saved as a delta against its
/// archetype, so a struct holds only the members that differ: structs merge member by member at any depth, arrays
/// and scalars replace.
pub fn instance_props(base: Option<&Props>, over: Option<&Props>) -> Props {
    let mut out = base.cloned().unwrap_or_default();
    if let Some(over) = over {
        for (k, v) in over {
            let merged = match (v, out.get(k)) {
                (Value::Obj(o), Some(Value::Obj(b))) => Value::Obj(instance_props(Some(b), Some(o))),
                _ => v.clone(),
            };
            out.insert(k.clone(), merged);
        }
    }
    out
}
