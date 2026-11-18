//! Asset classes whose native part is small: UAnimationAsset, UDataTable, UCurveTable (CUE4Parse 1.2.2 Deserialize
//! and WriteJson of each). (UEnum and UUserDefinedStruct are in `class_struct`.) A name-keyed table (a data table's or
//! curve table's Rows) is a `Value::Map` in the build's serialized order.

use super::Native;
use crate::archive::Result;
use crate::fields::StructFields;
use crate::objects::{ExportArchive, UObj, super_deserialize};
use crate::props::read_struct_fallback;
use crate::value::{Props, Value};
use crate::versions;

/// By C# class name.
pub static NATIVES: &[(&str, Native)] = &[
    ("UAnimationAsset", Native { deserialize: Some(anim_deserialize), json: Some(anim_json), extra: None }),
    ("UDataTable", Native { deserialize: Some(data_table_deserialize), json: Some(data_table_json), extra: None }),
    ("UCurveTable", Native { deserialize: Some(curve_table_deserialize), json: Some(curve_table_json), extra: None }),
];

const CURVE_TABLE_MODES: &[&str] = &["Empty", "SimpleCurves", "RichCurves"];
/// FFortniteMainBranchObjectVersion::ShrinkCurveTableSize.
const SHRINK_CURVE_TABLE_SIZE: i32 = 23;
const NO_GUID: &str = "00000000-00000000-00000000-00000000";

/// Rows by name as a `Value::Map`: a name read twice keeps its first place and its last value (as an insertion-ordered
/// map's set does).
fn rows_value(rows: Props) -> Value {
    Value::Map(rows.into_iter().map(|(k, v)| (Value::Str(k), v)).collect())
}

fn anim_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    let g = ar.guid()?;
    obj.data.insert("SkeletonGuid".into(), Value::Str(g.to_string()));
    Ok(())
}

fn anim_json(obj: &UObj, o: &mut Props) {
    let g = obj.data.get("SkeletonGuid").cloned().unwrap_or_else(|| Value::Str(NO_GUID.into()));
    o.insert("SkeletonGuid".into(), g);
}

fn data_table_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    // GetOrDefault<FPackageIndex>("RowStruct"): only a missing tag stops it ("Can't find or load RowStruct"); a None
    // index still reads the rows.
    // Unverified: a RowStruct tag whose value failed to parse is null there and stops it; here it reads on
    let Some(rs) = obj.props.get("RowStruct") else { return Ok(()) };
    let fields = StructFields::of(rs.as_ref().cloned());
    let n = ar.i32()?;
    let mut rows = Props::new();
    for _ in 0..n {
        let k = ar.fname()?;
        let v = read_struct_fallback(ar, Some(&fields))?;
        rows.insert(k, Value::Struct(v));
    }
    obj.data.insert("Rows".into(), rows_value(rows));
    Ok(())
}

fn data_table_json(obj: &UObj, o: &mut Props) {
    o.insert("Rows".into(), obj.data.get("Rows").cloned().unwrap_or(Value::Null));
}

fn curve_table_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    let n = ar.i32()?;
    let mode = if ar.custom(versions::FORTNITE_MAIN) >= SHRINK_CURVE_TABLE_SIZE {
        ar.u8()?
    } else if n > 0 {
        2
    } else {
        0
    };
    obj.data.insert("CurveTableMode".into(), Value::Int(mode.into()));
    let mut rows = Props::new();
    for _ in 0..n {
        let k = ar.fname()?;
        let v = read_struct_fallback(ar, None)?;
        rows.insert(k, Value::Struct(v));
    }
    obj.data.insert("Rows".into(), rows_value(rows));
    Ok(())
}

fn curve_table_mode(m: i64) -> Value {
    usize::try_from(m).ok().and_then(|i| CURVE_TABLE_MODES.get(i)).map_or(Value::Int(m), |s| Value::Str((*s).into()))
}

fn curve_table_json(obj: &UObj, o: &mut Props) {
    let m = obj.data.get("CurveTableMode").and_then(Value::as_i64).unwrap_or(0);
    o.insert("CurveTableMode".into(), curve_table_mode(m));
    o.insert("Rows".into(), obj.data.get("Rows").cloned().unwrap_or(Value::Map(Vec::new())));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_table_mode_names() {
        assert_eq!(curve_table_mode(0).as_str(), Some("Empty"));
        assert_eq!(curve_table_mode(2).as_str(), Some("RichCurves"));
        assert!(matches!(curve_table_mode(7), Value::Int(7)));
    }
}
