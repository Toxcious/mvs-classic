//! Reads over a bank's tree (`mvs_uasset::natives::wwise_bank`'s [`Value`]s): the lookups the compaction makes, and
//! the error a tree that is not what the port reads gives.

use std::fmt::Display;

use mvs_data::sound::{Curve, Id, Pt, Scaling};
use mvs_uasset::value::Value;

use crate::kind::KindError;
use crate::objview::{num, truthy};

pub(super) type R<T> = Result<T, KindError>;

/// What the compaction cannot read: an error naming the bank.
pub(super) fn fail<T>(what: &str, msg: impl Display) -> R<T> {
    Err(KindError::Failed(format!("soundbank: {what}: {msg}")))
}

/// Member `k` of a struct value.
pub(super) fn get<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
    match v {
        Value::Struct(p) => p.get(k),
        _ => None,
    }
}

/// The elements of an array value (none for anything else).
pub(super) fn items(v: Option<&Value>) -> &[Value] {
    match v {
        Some(Value::Array(a)) => a,
        _ => &[],
    }
}

/// Member `k` of `v` as an array's elements.
pub(super) fn list<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    items(get(v, k))
}

/// A number value as the double the JSON holds.
pub(super) fn f(v: Option<&Value>) -> Option<f64> {
    v.and_then(num)
}

pub(super) fn req_f(v: &Value, k: &str, what: &str) -> R<f64> {
    match f(get(v, k)) {
        Some(x) => Ok(x),
        None => fail(what, format!("{k} is not a number")),
    }
}

/// An integer value in `T`'s range.
pub(super) fn int<T: TryFrom<i64>>(v: Option<&Value>) -> Option<T> {
    v.and_then(Value::as_i64).and_then(|n| T::try_from(n).ok())
}

pub(super) fn req_int<T: TryFrom<i64>>(v: &Value, k: &str, what: &str) -> R<T> {
    match int(get(v, k)) {
        Some(x) => Ok(x),
        None => fail(what, format!("{k} is not an integer in range")),
    }
}

pub(super) fn id_list(v: Option<&Value>, what: &str) -> R<Vec<Id>> {
    items(v)
        .iter()
        .map(|x| match int(Some(x)) {
            Some(i) => Ok(i),
            None => fail(what, "an id out of range"),
        })
        .collect()
}

/// A string value (an enum's name), or an enum's number written as text (what `String(v)` gives).
pub(super) fn text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Str(s) => Some(s.clone()),
        Value::Int(i) => Some(i.to_string()),
        Value::UInt(u) => Some(u.to_string()),
        _ => None,
    }
}

pub(super) fn str_of(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str)
}

pub(super) fn is_true(v: Option<&Value>) -> bool {
    v.is_some_and(truthy)
}

pub(super) fn curve(v: Option<&Value>, what: &str) -> R<Curve> {
    Ok(match str_of(v) {
        Some("Log3") => Curve::Log3,
        Some("Sine") => Curve::Sine,
        Some("Log1") => Curve::Log1,
        Some("InvSCurve") => Curve::InvSCurve,
        Some("Linear") => Curve::Linear,
        Some("SCurve") => Curve::SCurve,
        Some("Exp1") => Curve::Exp1,
        Some("SineRecip") => Curve::SineRecip,
        Some("Exp3") => Curve::Exp3,
        Some("Constant") => Curve::Constant,
        _ => return fail(what, format!("interpolation {v:?} not handled")),
    })
}

pub(super) fn scaling(v: Option<&Value>, what: &str) -> R<Scaling> {
    Ok(match str_of(v) {
        Some("None") => Scaling::None,
        Some("dB") => Scaling::Db,
        Some("Log") => Scaling::Log,
        _ => return fail(what, format!("curve scaling {v:?} not handled")),
    })
}

/// A conversion table's or automation's graph points.
pub(super) fn pts(gp: &[Value], what: &str) -> R<Vec<Pt>> {
    gp.iter()
        .map(|g| {
            Ok(Pt {
                x: req_f(g, "From", what)?,
                y: req_f(g, "To", what)?,
                curve: curve(get(g, "Interpolation"), what)?,
            })
        })
        .collect()
}
