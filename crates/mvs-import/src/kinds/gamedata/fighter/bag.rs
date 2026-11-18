//! Reading the bag as the fighter binds read the object kind's JSON: a member that may be absent, `??`, JS
//! truthiness and the enum tails the binds cut.

use mvs_data::{Props, Value};

/// `v.k`: a member of an object, `None` for an absent one or a value that is no object.
pub fn at<'v>(v: &'v Value, k: &str) -> Option<&'v Value> {
    v.as_obj()?.get(k)
}

/// `v.k` of an optional value (`v?.k`).
pub fn at_opt<'v>(v: Option<&'v Value>, k: &str) -> Option<&'v Value> {
    v.and_then(|v| at(v, k))
}

/// A value that `??` keeps (neither absent nor null).
pub fn some(v: Option<&Value>) -> Option<&Value> {
    v.filter(|x| !x.is_null())
}

/// JS truthiness of a value that may be absent: an empty array or object is true; 0, NaN, '' and null are false.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Value::Str(s)) => !s.is_empty(),
        Some(Value::Arr(_) | Value::Obj(_) | Value::F32s(_)) => true,
    }
}

/// `v ?? d` as a value.
pub fn or(v: Option<&Value>, d: Value) -> Value {
    some(v).cloned().unwrap_or(d)
}

/// `v ?? d` read as a number (a value that is no number compares as NaN, unequal to every number).
pub fn n_or(v: Option<&Value>, d: f64) -> f64 {
    some(v).map_or(d, |x| x.as_f64().unwrap_or(f64::NAN))
}

/// `!!(v ?? d)`.
pub fn b_or(v: Option<&Value>, d: bool) -> bool {
    if some(v).is_none() { d } else { truthy(v) }
}

/// `v ?? d` read as a string (a value that is no string reads as `d`).
pub fn s_or<'v>(v: Option<&'v Value>, d: &'v str) -> &'v str {
    some(v).and_then(Value::as_str).unwrap_or(d)
}

/// `(v ?? [])` as an array (a value that is no array reads as none).
pub fn arr_of(v: Option<&Value>) -> &[Value] {
    some(v).and_then(Value::as_arr).unwrap_or_default()
}

/// `v || []`: the value where it is truthy, else an empty array.
pub fn or_empty(v: Option<&Value>) -> Value {
    if truthy(v) { v.cloned().unwrap_or(Value::Null) } else { Value::Arr(Vec::new()) }
}

/// The text after the last `::` (`EAlphaBlendOption::Linear` -> `Linear`).
pub fn tail(s: &str) -> &str {
    s.rsplit("::").next().unwrap_or(s)
}

/// A string value, else `None`.
pub fn str_of(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str)
}

pub fn num(n: f64) -> Value {
    Value::Num(n)
}

pub fn s(t: impl Into<String>) -> Value {
    Value::Str(t.into())
}

/// An object from `(key, value)` pairs in order.
pub fn obj<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    Value::Obj(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// An object's members, `None` for a value that is no object.
pub fn props_of(v: &Value) -> Option<&Props> {
    v.as_obj()
}

/// Whether a JS object key is an array index (`0`, `12`; not `01`): JS lays such keys out first, ascending, whatever
/// order they were set in.
pub fn is_index_key(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 10
        && k.bytes().all(|b| b.is_ascii_digit())
        && (k == "0" || !k.starts_with('0'))
        && k.parse::<u64>().is_ok_and(|n| n < u32::MAX as u64)
}

/// An object's keys in JS order: the array-index keys first, ascending, then the rest in the order set.
pub fn js_order(p: Props) -> Props {
    if !p.keys().any(|k| is_index_key(k)) {
        return p;
    }
    let (mut ix, rest): (Vec<_>, Vec<_>) = p.into_iter().partition(|(k, _)| is_index_key(k));
    ix.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(0));
    ix.into_iter().chain(rest).collect()
}
