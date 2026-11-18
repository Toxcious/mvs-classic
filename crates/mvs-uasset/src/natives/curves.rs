//! UCurveLinearColor / UCurveVector (CUE4Parse 1.2.2): FloatCurves built from the struct-valued properties, by the
//! property's place in the list (an index past the array fails, as in C#, after the earlier ones are set).

use super::Native;
use crate::archive::{ReadError, Result};
use crate::objects::{ExportArchive, UObj, super_deserialize};
use crate::value::{Props, Value};

/// By C# class name.
pub static NATIVES: &[(&str, Native)] = &[
    ("UCurveLinearColor", Native { deserialize: Some(deserialize::<4>), json: Some(json::<4>), extra: None }),
    ("UCurveVector", Native { deserialize: Some(deserialize::<3>), json: Some(json::<3>), extra: None }),
];

const EXTRAP: &[&str] =
    &["RCCE_Cycle", "RCCE_CycleWithOffset", "RCCE_Oscillate", "RCCE_Linear", "RCCE_Constant", "RCCE_None"];

/// An enum property's value ("EType::Name") as GetOrDefault converts it to the C# enum's name.
fn enum_value(v: Option<&Value>, names: &[&str], def: &str) -> String {
    let Some(v) = v.and_then(Value::as_str) else { return def.to_owned() };
    let n = v.split_once("::").map_or(v, |(_, n)| n);
    if names.contains(&n) { n.to_owned() } else { def.to_owned() }
}

/// FRichCurve(FStructFallback) as Newtonsoft writes it.
pub fn rich_curve(s: &Props) -> Value {
    let mut o = Props::new();
    let keys = match s.get("Keys") {
        Some(k @ Value::Array(_)) => k.clone(),
        _ => Value::Array(Vec::new()),
    };
    o.insert("Keys".into(), keys);
    let default = match s.get("DefaultValue") {
        Some(v @ (Value::Float(_) | Value::Double(_) | Value::Int(_) | Value::UInt(_))) => v.clone(),
        _ => Value::Float(f32::MAX),
    };
    o.insert("DefaultValue".into(), default);
    o.insert("PreInfinityExtrap".into(), Value::Str(enum_value(s.get("PreInfinityExtrap"), EXTRAP, "RCCE_Constant")));
    o.insert("PostInfinityExtrap".into(), Value::Str(enum_value(s.get("PostInfinityExtrap"), EXTRAP, "RCCE_Constant")));
    Value::Struct(o)
}

// Unverified: every struct-valued property counts as an FStructFallback here; a reader that counts only the structs
// read as tagged properties would not count (a native struct, an FVector, would not). The curve classes hold no struct
// property but their FloatCurves (FRichCurve, read as tagged properties).
fn deserialize<const N: usize>(
    obj: &mut UObj,
    ar: &mut ExportArchive<'_>,
    valid_pos: usize,
    level: usize,
) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    let mut out = vec![Value::Null; N];
    let mut err = None;
    for (i, v) in obj.props.values().enumerate() {
        let Value::Struct(s) = v else { continue };
        if i >= N {
            err = Some(ReadError::parse("Index was outside the bounds of the array."));
            break;
        }
        out[i] = rich_curve(s);
    }
    obj.data.insert("FloatCurves".into(), Value::Array(out));
    err.map_or(Ok(()), Err)
}

fn json<const N: usize>(obj: &UObj, o: &mut Props) {
    let c = obj.data.get("FloatCurves").cloned().unwrap_or_else(|| Value::Array(vec![Value::Null; N]));
    o.insert("FloatCurves".into(), c);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rich_curve_defaults() {
        let Value::Struct(o) = rich_curve(&Props::new()) else { panic!() };
        assert!(matches!(&o["Keys"], Value::Array(a) if a.is_empty()));
        assert!(matches!(o["DefaultValue"], Value::Float(f) if f == f32::MAX));
        assert_eq!(o["PreInfinityExtrap"].as_str(), Some("RCCE_Constant"));
        let mut s = Props::new();
        s.insert("PreInfinityExtrap".into(), Value::Str("ERichCurveExtrapolation::RCCE_Linear".into()));
        s.insert("PostInfinityExtrap".into(), Value::Str("ERichCurveExtrapolation::RCCE_Bogus".into()));
        s.insert("DefaultValue".into(), Value::Float(1.5));
        let Value::Struct(o) = rich_curve(&s) else { panic!() };
        assert_eq!(o["PreInfinityExtrap"].as_str(), Some("RCCE_Linear"));
        assert_eq!(o["PostInfinityExtrap"].as_str(), Some("RCCE_Constant"));
        assert!(matches!(o["DefaultValue"], Value::Float(f) if f == 1.5));
    }
}
