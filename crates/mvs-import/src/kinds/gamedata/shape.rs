//! What every bind of the kind shapes the build's objects with: an object's members as the bag the binds read of
//! the object kind's JSON (`mvs_data::Value`), the tables' names and paths, UserDefinedEnum
//! values as their bytes, the bind helpers and the
//! fighter parts' deepUpdate and key order.

use std::sync::Arc;

use indexmap::IndexMap;
use mvs_data::{Props, Value};
use mvs_uasset::PackageCache;
use mvs_uasset::value::{Props as UProps, Value as UValue};

use crate::json::{self, Json};
use crate::kind::{Cx, KindError, Memo};
use crate::objview::ObjView;

pub use crate::objview::short_name;

/// A failure naming where it happened.
pub fn fail<T>(msg: impl Into<String>) -> Result<T, KindError> {
    Err(KindError::Failed(msg.into()))
}

// ---- the bag: an object's members as the object kind's JSON gives them

/// A JSON value as the bag holds it (a JSON number is a double; no Float32Array comes out of JSON).
pub fn from_json(j: Json) -> Value {
    match j {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(b),
        Json::Num(n) => Value::Num(n),
        Json::Str(s) => Value::Str(s),
        Json::Arr(a) => Value::Arr(a.into_iter().map(from_json).collect()),
        Json::Obj(m) => Value::Obj(m.into_iter().map(|(k, v)| (k, from_json(v))).collect()),
    }
}

/// Whether a value holds a binary section (which the object kind's JSON lays out in its body, not as a value).
fn has_bytes(v: &UValue) -> bool {
    match v {
        UValue::Bytes(_) => true,
        UValue::Array(a) => a.iter().any(has_bytes),
        UValue::Struct(p) => p.values().any(has_bytes),
        UValue::Map(m) => m.iter().any(|(k, v)| has_bytes(k) || has_bytes(v)),
        _ => false,
    }
}

/// One value of the build as a bind reads it of the object kind's JSON (`kinds/object.rs`'s writer, read back):
/// a reference as its object path, a map as `[key, value]` pairs, a text as its `{ source, .. }` object, a float as
/// the .NET-rounded double, a non-finite number as the string the object kind holds. A binary section is refused.
pub fn bag(cache: &PackageCache, v: &UValue, what: &str) -> Result<Value, KindError> {
    if has_bytes(v) {
        return fail(format!("{what}: binary data, not a value"));
    }
    Ok(from_json(json::value_json(cache, v)))
}

/// Tagged properties as the bag (`o.properties` / `o.defaults` / `o.native`).
pub fn bag_props(cache: &PackageCache, p: &UProps, what: &str) -> Result<Props, KindError> {
    if let Some((k, _)) = p.iter().find(|(_, v)| has_bytes(v)) {
        return fail(format!("{what}: {k} is binary data, not a value"));
    }
    Ok(json::props_json(cache, p).into_iter().map(|(k, v)| (k, from_json(v))).collect())
}

/// An object's tagged properties as the bag (`o.properties`).
pub fn properties(o: &ObjView<'_>) -> Result<Props, KindError> {
    bag_props(o.cache(), o.props(), o.path())
}

/// A class default object's properties over its Blueprint parents' (`o.defaults`), `None` for another object.
pub fn defaults(o: &ObjView<'_>) -> Result<Option<Props>, KindError> {
    o.defaults()?.map(|d| bag_props(o.cache(), &d, o.path())).transpose()
}

/// `o.defaults ?? o.properties`.
pub fn defaults_or_properties(o: &ObjView<'_>) -> Result<Props, KindError> {
    match defaults(o)? {
        Some(d) => Ok(d),
        None => properties(o),
    }
}

/// What the class's native serializer read (`o.native`), `None` when nothing.
pub fn native(o: &ObjView<'_>) -> Result<Option<Props>, KindError> {
    o.native()?.map(|n| bag_props(o.cache(), &n, o.path())).transpose()
}

// ---- paths and names

/// A Blueprint class's class default object: `/Game/A/B.B_C` -> `/Game/A/B.Default__B_C`.
pub fn cdo_path(cls: &str) -> String {
    match cls.rfind('.') {
        Some(dot) => format!("{}.Default__{}", &cls[..dot], &cls[dot + 1..]),
        None => format!(".Default__{cls}"),
    }
}

/// The enum name of a UserDefinedEnum value as the reader writes it (`^(Enum_\w+)::NewEnumerator\d+$`); its byte is the
/// enum asset's (`NewEnumerator<n>` is the order of creation, not the value).
pub fn enum_value_name(s: &str) -> Option<&str> {
    let (name, rest) = s.split_once("::")?;
    let word = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    let digits = rest.strip_prefix("NewEnumerator")?;
    let ok = name.starts_with("Enum_")
        && word(&name[5..])
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit());
    ok.then_some(name)
}

/// An enum's values' bytes, by value name (`Enum_X::NewEnumerator3` -> its byte).
pub type EnumNames = IndexMap<String, f64>;
/// By enum name, its values' bytes.
pub type EnumTable = IndexMap<String, Arc<EnumNames>>;

/// A native name-keyed table of an object (a DataTable's or CurveTable's `Rows`, an enum's `Names`): [name, value]
/// pairs in the build's serialized order.
pub fn native_table(o: &ObjView<'_>, field: &str) -> Result<Vec<(String, Value)>, KindError> {
    let n = native(o)?;
    let Some(Value::Arr(t)) = n.as_ref().and_then(|n| n.get(field)) else {
        return fail(format!("{}: no {field}", o.path()));
    };
    t.iter()
        .map(|e| match e.as_arr() {
            Some([Value::Str(k), v]) => Ok((k.clone(), v.clone())),
            _ => fail(format!("{}: {field}: not a [name, value] pair", o.path())),
        })
        .collect()
}

/// A data table's rows, [row name, row] in the build's order.
pub fn table_rows(o: &ObjView<'_>) -> Result<Vec<(String, Value)>, KindError> {
    native_table(o, "Rows")
}

/// An enum asset's Names as a map.
pub fn enum_names(o: &ObjView<'_>) -> Result<EnumNames, KindError> {
    native_table(o, "Names")?
        .into_iter()
        .map(|(k, v)| match v {
            Value::Num(n) => Ok((k, n)),
            _ => fail(format!("{}: Names: {k} is no number", o.path())),
        })
        .collect()
}

/// The names of the enums a value holds values of, added to `into` (first seen first).
pub fn enums_named(v: &Value, into: &mut Vec<String>) {
    match v {
        Value::Str(s) => {
            if let Some(n) = enum_value_name(s)
                && !into.iter().any(|x| x == n)
            {
                into.push(n.to_owned());
            }
        }
        Value::Arr(a) => a.iter().for_each(|x| enums_named(x, into)),
        Value::Obj(o) => o.values().for_each(|x| enums_named(x, into)),
        _ => {}
    }
}

/// The enum assets `names` by the paths `known` gives them (a name it lacks is an error naming `where_`, the table
/// that should list it).
pub fn load_enum_table(
    cx: &Cx<'_>,
    names: &[String],
    known: &[(&str, &str)],
    where_: &str,
) -> Result<EnumTable, KindError> {
    let unknown: Vec<&str> = names.iter().filter(|n| !known.iter().any(|(k, _)| k == n)).map(String::as_str).collect();
    if !unknown.is_empty() {
        return fail(format!("{}: enums the class defaults name, not in {where_}", unknown.join(", ")));
    }
    let mut out = EnumTable::new();
    for n in names {
        let p = known.iter().find(|(k, _)| k == n).map(|(_, p)| *p).unwrap_or_default();
        out.insert(n.clone(), Arc::new(enum_names(&cx.object(p)?)?));
    }
    Ok(out)
}

const TEXT_KEYS: [&str; 4] = ["source", "namespace", "key", "table"];

/// A class default value as the tables hold it (the shape the sim's buff and actor code reads): a reference as its
/// last name component; a UserDefinedEnum value as its byte (`enums`, by enum name); a map as `{ Key, Value }`
/// entries; a culture-invariant text as its string.
pub fn shape_value(v: &Value, enums: &EnumTable) -> Result<Value, KindError> {
    Ok(match v {
        Value::Null | Value::Num(_) | Value::Bool(_) | Value::F32s(_) => v.clone(),
        Value::Str(s) => {
            if let Some(n) = enum_value_name(s) {
                match enums.get(n).and_then(|t| t.get(s.as_str())) {
                    Some(b) => Value::Num(*b),
                    None => return fail(format!("{s}: not a value of a known enum asset")),
                }
            } else if s.starts_with("/Game/") || s.starts_with("/Script/") || s.starts_with("/Engine/") {
                Value::Str(short_name(s).to_owned())
            } else {
                v.clone()
            }
        }
        Value::Arr(a) => {
            // (a map: [key, value] pairs; UE has no array of arrays)
            let pairs = !a.is_empty() && a.iter().all(|e| matches!(e, Value::Arr(p) if p.len() == 2));
            if pairs {
                let mut out = Vec::with_capacity(a.len());
                for e in a {
                    let p = e.as_arr().unwrap_or_default();
                    let mut o = Props::new();
                    o.insert("Key".into(), shape_value(&p[0], enums)?);
                    o.insert("Value".into(), shape_value(&p[1], enums)?);
                    out.push(Value::Obj(o));
                }
                Value::Arr(out)
            } else {
                Value::Arr(a.iter().map(|x| shape_value(x, enums)).collect::<Result<_, _>>()?)
            }
        }
        Value::Obj(o) => {
            // (an FColor's Hex: the reader's own derived string, not a member)
            let color = ["R", "G", "B", "A"].iter().all(|k| o.contains_key(*k));
            let keys: Vec<&String> = o.keys().filter(|k| *k != "Hex" || !color).collect();
            // (a culture-invariant text: its string; an empty one, which the build stores with no string, null)
            if let Some(src) = o.get("source") {
                if keys.len() == 1 && matches!(src, Value::Str(_) | Value::Null) {
                    return Ok(src.clone());
                }
                if keys.iter().all(|k| TEXT_KEYS.contains(&k.as_str())) {
                    return Ok(v.clone());
                }
            }
            let mut out = Props::new();
            for k in keys {
                out.insert(k.clone(), shape_value(&o[k], enums)?);
            }
            Value::Obj(out)
        }
    })
}

/// A class default object's merged defaults, shaped, without its ubergraph frame.
pub fn class_defaults(o: &ObjView<'_>, enums: &EnumTable) -> Result<Props, KindError> {
    let mut d = defaults_or_properties(o)?;
    d.shift_remove("UberGraphFrame");
    match shape_value(&Value::Obj(d), enums)? {
        Value::Obj(p) => Ok(p),
        _ => unreachable!("an object shapes to an object"),
    }
}

// ---- the UserDefinedEnums a part's values name

/// Where the UserDefinedEnum assets whose values the class defaults hold are found (by name under Panda_Main).
const ENUM_ROOT: &str = "/Game/Panda_Main";
const USER_DEFINED_ENUM: &str = "/Script/Engine.UserDefinedEnum";

/// The run's UserDefinedEnum packages under ENUM_ROOT by lower-case name, and each enum's table once read.
#[derive(Default)]
struct Enums {
    by_name: std::sync::OnceLock<IndexMap<String, Vec<String>>>,
    tables: Memo<Result<Arc<EnumNames>, String>>,
}

/// The enum tables of the UserDefinedEnums whose values `vs` hold (shape_value's `enums`), each found by name under
/// Panda_Main (a name with no asset, or with two, is an error naming `key`, the bind's character key); read once per
/// run.
pub fn enums_for(cx: &Cx<'_>, key: &str, vs: &[&Value]) -> Result<EnumTable, KindError> {
    let mut want = Vec::new();
    for v in vs {
        enums_named(v, &mut want);
    }
    if want.is_empty() {
        return Ok(EnumTable::new());
    }
    let e = cx.shared.get_or_init(Enums::default);
    let by_name = e.by_name.get_or_init(|| {
        let mut m: IndexMap<String, Vec<String>> = IndexMap::new();
        for p in cx.catalog.list_packages(ENUM_ROOT, true, Some(USER_DEFINED_ENUM)) {
            let k = p[p.rfind('/').map_or(0, |i| i + 1)..].to_lowercase();
            m.entry(k).or_default().push(p);
        }
        m
    });
    let mut out = EnumTable::new();
    for n in want {
        let hits = by_name.get(&n.to_lowercase()).map_or(&[][..], Vec::as_slice);
        if hits.len() != 1 {
            return fail(format!(
                "fighter data {key}: enum {n}: {} UserDefinedEnum assets of that name under {ENUM_ROOT}",
                hits.len()
            ));
        }
        let t = e
            .tables
            .get(&n, || cx.object(&hits[0]).and_then(|o| enum_names(&o)).map(Arc::new).map_err(|e| e.to_string()));
        out.insert(n, t.map_err(KindError::Failed)?);
    }
    Ok(out)
}

/// The fighter data's keys in the order the parts bind them.
pub const FIGHTER_KEY_ORDER: [&str; 30] = [
    "pawn",
    "base",
    "components",
    "animSet",
    "animSets",
    "animSetBlend",
    "polymorphAnimSets",
    "animAssets",
    "fallSpace",
    "wallSpaces",
    "seqNotifies",
    "branchGroups",
    "montages",
    "buffs",
    "actors",
    "actorMontages",
    "slotHolds",
    "animNodes",
    "skeleton",
    "vo",
    "voTags",
    "skin",
    "extraSkins",
    "skins",
    "taunts",
    "tauntTable",
    "danceMontage",
    "missingMontages",
    "meshComposites",
    "paths",
];

/// `b`'s members over `a`'s, a struct (a plain object) into a struct member by member.
pub fn deep_update(a: &mut Props, b: &Props) {
    for (k, v) in b {
        match (a.get_mut(k), v) {
            (Some(Value::Obj(av)), Value::Obj(bv)) => deep_update(av, bv),
            (Some(av), _) => *av = v.clone(),
            (None, _) => {
                a.insert(k.clone(), v.clone());
            }
        }
    }
}

// ---- a property the bind needs is there, of the type it reads (a missing one is an error naming the
// object, never a default of the port's: an absent tagged property is the class's own default, which is code)

pub fn prop<'v>(o: &'v Props, name: &str, where_: &str) -> Result<&'v Value, KindError> {
    o.get(name).ok_or_else(|| KindError::Failed(format!("{where_}: no {name}")))
}

pub fn num(o: &Props, name: &str, where_: &str) -> Result<f64, KindError> {
    prop(o, name, where_)?.as_f64().ok_or_else(|| KindError::Failed(format!("{where_}: {name} is not a number")))
}

pub fn bool(o: &Props, name: &str, where_: &str) -> Result<bool, KindError> {
    prop(o, name, where_)?.as_bool().ok_or_else(|| KindError::Failed(format!("{where_}: {name} is not a boolean")))
}

pub fn arr<'v>(o: &'v Props, name: &str, where_: &str) -> Result<&'v [Value], KindError> {
    prop(o, name, where_)?.as_arr().ok_or_else(|| KindError::Failed(format!("{where_}: {name} is not an array")))
}

pub fn strukt<'v>(o: &'v Props, name: &str, where_: &str) -> Result<&'v Props, KindError> {
    prop(o, name, where_)?.as_obj().ok_or_else(|| KindError::Failed(format!("{where_}: {name} is not a struct")))
}

/// A vector struct as [X, Y, Z].
pub fn vec3(o: &Props, name: &str, where_: &str) -> Result<[f64; 3], KindError> {
    let v = strukt(o, name, where_)?;
    let w = format!("{where_} {name}");
    Ok([num(v, "X", &w)?, num(v, "Y", &w)?, num(v, "Z", &w)?])
}

/// The object name of a reference's path (`/Game/A/B.B` -> `B`, `/Game/A/B.B_C:Comp` -> `B_C:Comp`), '' for none.
pub fn ref_name(path: &Value) -> Result<String, KindError> {
    match path {
        Value::Null => Ok(String::new()),
        Value::Str(s) => Ok(s[s.rfind('.').map_or(0, |i| i + 1)..].to_owned()),
        _ => fail(format!("not an object path: {}", show(path))),
    }
}

/// The asset name of a bare package path (`/Game/A/B` -> `B`).
pub fn package_name(path: &str) -> &str {
    let last = &path[path.rfind('/').map_or(0, |i| i + 1)..];
    last.split('.').next().unwrap_or(last)
}

/// A value as JSON, for a message.
pub fn show(v: &Value) -> String {
    let b = mvs_data::value::encode(v).unwrap_or_default();
    let n = b.get(4..8).map_or(0, |l| u32::from_le_bytes([l[0], l[1], l[2], l[3]]) as usize);
    String::from_utf8_lossy(b.get(8..8 + n).unwrap_or_default()).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(m: &[(&str, Value)]) -> Value {
        Value::Obj(m.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect())
    }

    #[test]
    fn names_and_paths() {
        assert_eq!(cdo_path("/Game/A/B.B_C"), "/Game/A/B.Default__B_C");
        assert_eq!(enum_value_name("Enum_Kind::NewEnumerator3"), Some("Enum_Kind"));
        assert_eq!(enum_value_name("Enum_::NewEnumerator3"), None);
        assert_eq!(enum_value_name("Enum_Kind::NewEnumerator"), None);
        assert_eq!(enum_value_name("EKind::NewEnumerator3"), None);
        assert_eq!(package_name("/Game/A/B"), "B");
        assert_eq!(ref_name(&Value::Str("/Game/A/B.B_C:Comp".into())).unwrap(), "B_C:Comp");
        assert_eq!(ref_name(&Value::Null).unwrap(), "");
    }

    #[test]
    fn shaped_as_the_tables_hold_it() {
        let mut enums = EnumTable::new();
        enums.insert("Enum_K".into(), Arc::new([("Enum_K::NewEnumerator0".to_owned(), 2.0)].into_iter().collect()));
        let v = obj(&[
            ("Cls", Value::Str("/Game/A/B.B_C".into())),
            ("E", Value::Str("Enum_K::NewEnumerator0".into())),
            ("Text", obj(&[("source", Value::Str("hi".into()))])),
            ("Keyed", obj(&[("source", Value::Null), ("namespace", Value::Str("n".into()))])),
            (
                "Color",
                obj(&[
                    ("R", Value::Num(1.0)),
                    ("G", Value::Num(2.0)),
                    ("B", Value::Num(3.0)),
                    ("A", Value::Num(4.0)),
                    ("Hex", Value::Str("x".into())),
                ]),
            ),
            ("Map", Value::Arr(vec![Value::Arr(vec![Value::Str("k".into()), Value::Num(1.0)])])),
        ]);
        let s = shape_value(&v, &enums).unwrap();
        assert_eq!(
            show(&s),
            r#"{"Cls":"B_C","E":2,"Text":"hi","Keyed":{"source":null,"namespace":"n"},"Color":{"R":1,"G":2,"B":3,"A":4},"Map":[{"Key":"k","Value":1}]}"#
        );
        assert!(shape_value(&Value::Str("Enum_X::NewEnumerator1".into()), &enums).is_err());
        let mut into = Vec::new();
        enums_named(&v, &mut into);
        assert_eq!(into, ["Enum_K"]);
    }

    #[test]
    fn deep_update_merges_structs() {
        let Value::Obj(mut a) =
            obj(&[("S", obj(&[("X", Value::Num(1.0)), ("Y", Value::Num(2.0))])), ("N", Value::Num(1.0))])
        else {
            unreachable!()
        };
        let Value::Obj(b) = obj(&[("S", obj(&[("Y", Value::Num(3.0))])), ("M", Value::Bool(true))]) else {
            unreachable!()
        };
        deep_update(&mut a, &b);
        assert_eq!(show(&Value::Obj(a)), r#"{"S":{"X":1,"Y":3},"N":1,"M":true}"#);
    }
}
