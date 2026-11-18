//! A package export as a typed object of our own shape (the object kind): what
//! the game's bind code reads.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::natives;
use crate::objects::{IssueKind, RF_CLASS_DEFAULT_OBJECT, ReadIssue, UObj};
use crate::package::{Package, PackageCache, Resolved};
use crate::value::{Props, TextValue, Value};

/// One object of the object kind.
#[derive(Clone, Debug, Default)]
pub struct AssetObject {
    /// Its own object path: `/Game/A/B.B`.
    pub path: String,
    /// Its class as an object path: `/Script/Engine.DataTable`, `/Game/A/BP.BP_C`.
    pub class: String,
    /// Its index in its package's export table.
    pub index: usize,
    /// The C# class name of the most basic class on its chain whose native serializer the reader lacks.
    pub partial: Option<String>,
    /// A class or struct export's parent, as an object path.
    pub super_: Option<String>,
    /// Its archetype (the export entry's TemplateIndex), as an object path.
    pub template: Option<String>,
    /// The tagged properties it holds, by stored name.
    pub properties: Props,
    /// What the class's native serializer reads beyond the tagged properties, by CUE4Parse's names; `None` when
    /// there is none.
    pub native: Option<Props>,
    /// A class default object only: its properties over its Blueprint parents' class defaults.
    pub defaults: Option<Props>,
    /// A top-level export only: the exports inside it, by their name chain below it. Empty when there is none.
    pub inner: IndexMap<String, AssetObject>,
}

/// The body of an object kind entry: the binary sections its objects hold, each 16-byte aligned, replaced in the
/// JSON by `{ offset, length }`.
#[derive(Default)]
pub struct BodySink {
    pub sections: Vec<Arc<[u8]>>,
    /// `(offset, length)` of each section in the body.
    pub refs: Vec<(usize, usize)>,
    pub end: usize,
}

impl BodySink {
    /// Adds a section; its `(offset, length)`.
    pub fn add(&mut self, bytes: Arc<[u8]>) -> (usize, usize) {
        let r = (self.end, bytes.len());
        self.end = (self.end + bytes.len() + 15) & !15;
        self.sections.push(bytes);
        self.refs.push(r);
        r
    }
}

/// Why an export is not an object of the kind: its read reported an error (a native struct or a text history the
/// reader lacks, a failed read), or a parent class's defaults are missing. A native class the reader lacks is no
/// error: the object is `partial`.
#[derive(Clone, Debug, thiserror::Error)]
#[error("{0}")]
pub struct ObjectError(pub String);

/// A UserDefinedStruct member's name as the struct declares it: the stored `<Name>_<n>_<32 hex GUID>` without its
/// suffix (`/^(.+)_\d+_[0-9A-F]{32}$/`); any other name as it is.
pub fn member_name(stored: &str) -> &str {
    let b = stored.as_bytes();
    if b.len() < 33 + 3 {
        return stored;
    }
    if !stored.is_char_boundary(stored.len() - 32) {
        return stored; // a multi-byte character in the last 32 bytes: no GUID there
    }
    let (rest, guid) = stored.split_at(stored.len() - 32);
    if !guid.bytes().all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c)) {
        return stored;
    }
    let Some(rest) = rest.strip_suffix('_') else { return stored };
    let Some(at) = rest.rfind('_') else { return stored };
    let digits = &rest[at + 1..];
    if at == 0 || digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return stored;
    }
    &rest[..at]
}

fn issues_text(path: &str, name: &str, issues: &[&ReadIssue]) -> String {
    let list: Vec<String> = issues
        .iter()
        .map(|i| {
            let kind = match i.kind {
                IssueKind::Read => "read",
                IssueKind::Native => "native",
                IssueKind::NotPorted => "not-ported",
            };
            format!("{kind}: {}", i.message)
        })
        .collect();
    format!("{path} {name}: {}", list.join("; "))
}

/// Export `index` of `pkg` as an AssetObject (no `inner`: the object kind's entry adds them). Binary sections of its
/// native extras go to `body`.
pub fn to_asset_object(
    cache: &PackageCache,
    pkg: &Arc<Package>,
    index: usize,
    body: &mut BodySink,
) -> Result<AssetObject, ObjectError> {
    let obj = cache.object(pkg, index);
    let errors: Vec<&ReadIssue> =
        obj.issues.iter().filter(|e| !(e.kind == IssueKind::Native && obj.partial.is_some())).collect();
    if !errors.is_empty() {
        return Err(ObjectError(issues_text(&pkg.path, &pkg.exports[index].object_name, &errors)));
    }
    let e = &pkg.exports[index];
    let self_ = Resolved::Export { pkg: pkg.clone(), index };
    let mut out = AssetObject {
        path: self_.object_path(cache),
        class: obj.cls.as_ref().map_or_else(|| "/Script/CoreUObject.Object".to_owned(), |c| c.object_path(cache)),
        index,
        properties: obj.props.clone(),
        ..AssetObject::default()
    };
    check_members_of(&out.properties).map_err(ObjectError)?;
    if obj.flags & RF_CLASS_DEFAULT_OBJECT != 0
        && let Some(cls) = &obj.cls
    {
        out.defaults = Some(class_defaults(cache, cls, &out.properties)?);
    }
    let resolve = |i: i32| cache.try_resolve(pkg, i).map_err(|err| ObjectError(format!("{}: {err}", out.path)));
    if let Some(s) = resolve(e.super_index)? {
        out.super_ = Some(s.object_path(cache));
    }
    if let Some(t) = resolve(e.template_index)? {
        out.template = Some(t.object_path(cache));
    }
    let mut native = Props::new();
    // most-derived writer first in the chain: each calls its base first, so run from the base up
    for c in obj.chain.iter().rev() {
        if let Some(json) = natives::native(c).and_then(|n| n.json) {
            json(&obj, &mut native);
        }
    }
    let mut extra = Props::new();
    for c in obj.chain.iter().rev() {
        if let Some(f) = natives::native(c).and_then(|n| n.extra) {
            f(&obj, &mut extra);
        }
    }
    for (k, mut v) in extra {
        if native.contains_key(&k) {
            return Err(ObjectError(format!("typed: {}: native {k} written twice", out.path)));
        }
        lay_out(&mut v, body);
        native.insert(k, v);
    }
    check_members_of(&native).map_err(ObjectError)?;
    if !native.is_empty() {
        out.native = Some(native);
    }
    out.partial = obj.partial.map(str::to_owned);
    Ok(out)
}

/// Each binary section of a native extra laid out in the body, the value replaced by its `{ offset, length }`.
fn lay_out(v: &mut Value, body: &mut BodySink) {
    match v {
        Value::Bytes(b) => {
            let (offset, length) = body.add(b.clone());
            *v = Value::Struct(Props::from([
                ("offset".to_owned(), Value::Int(i64::try_from(offset).expect("body offset"))),
                ("length".to_owned(), Value::Int(i64::try_from(length).expect("body length"))),
            ]));
        }
        Value::Array(a) => a.iter_mut().for_each(|x| lay_out(x, body)),
        Value::Struct(p) => p.values_mut().for_each(|x| lay_out(x, body)),
        Value::Map(m) => m.iter_mut().for_each(|(_, x)| lay_out(x, body)),
        _ => {}
    }
}

/// Two members of one struct whose declared names are the same.
fn check_members_of(p: &Props) -> Result<(), String> {
    check_struct(p)
}

fn check_struct(p: &Props) -> Result<(), String> {
    let mut seen: Vec<&str> = Vec::with_capacity(p.len());
    for (k, v) in p {
        let n = member_name(k);
        if seen.contains(&n) {
            return Err(format!("typed: two members named {n} ({k})"));
        }
        seen.push(n);
        check_value(v)?;
    }
    Ok(())
}

fn check_value(v: &Value) -> Result<(), String> {
    match v {
        Value::Bytes(_) => Err("typed: binary data outside a native extra (no body to lay it out in)".into()),
        Value::Struct(p) => check_struct(p),
        Value::Array(a) => a.iter().try_for_each(check_value),
        Value::Map(m) => m.iter().try_for_each(|(k, x)| check_value(k).and_then(|()| check_value(x))),
        _ => Ok(()),
    }
}

/// A float32 as .NET's shortest round-trip text gives it, read back as a double: the fewest
/// significant digits p whose rounding reads back as the same float32, rounded to nearest, an exact decimal tie to
/// the even digit first (.NET), then the other (toPrecision's, away from zero).
pub fn f32_dotnet(v: f32) -> f64 {
    let d = f64::from(v);
    if !v.is_finite() || v == 0.0 {
        return d;
    }
    // the exact decimal of the float: `{:e}` with more digits than any float32 has is exact
    let text = format!("{:.200e}", d.abs());
    let (mant, exp) = text.split_once('e').expect("an exponent");
    let exp: i32 = exp.parse().expect("an exponent");
    let digits: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).collect();
    let end = digits.iter().rposition(|&c| c != b'0').map_or(1, |i| i + 1);
    let digits = &digits[..end];
    let sign = if v < 0.0 { -1.0 } else { 1.0 };
    let make = |m: &[u8], e: i32| -> f64 {
        let s = format!("{}.{}e{e}", m[0] as char, std::str::from_utf8(&m[1..]).expect("digits"));
        sign * s.parse::<f64>().expect("a decimal")
    };
    let back = |c: f64| c as f32 == v;
    for p in 1..=9usize {
        if digits.len() <= p {
            return d; // exact in p digits: reads back as itself
        }
        let down: Vec<u8> = digits[..p].to_vec();
        // up: down plus one unit in the last place, a carry past the first digit raising the exponent
        let mut up = down.clone();
        let mut e_up = exp;
        let mut i = p;
        loop {
            if i == 0 {
                up.insert(0, b'1');
                up.pop();
                e_up += 1;
                break;
            }
            i -= 1;
            if up[i] == b'9' {
                up[i] = b'0';
            } else {
                up[i] += 1;
                break;
            }
        }
        let rest = &digits[p..];
        let tie = rest == b"5";
        let above = rest[0] > b'5' || (rest[0] == b'5' && rest.len() > 1);
        if tie {
            let even = (down[p - 1] - b'0').is_multiple_of(2);
            let (a, b) = (make(&down, exp), make(&up, e_up));
            for c in if even { [a, b] } else { [b, a] } {
                if back(c) {
                    return c;
                }
            }
        } else {
            let c = if above { make(&up, e_up) } else { make(&down, exp) };
            if back(c) {
                return c;
            }
        }
    }
    d
}

/// A value as the defaults merge sees it: an object (a struct or a text) as its members.
fn object_members(v: &Value) -> Option<Props> {
    match v {
        Value::Struct(p) => Some(p.clone()),
        Value::Text(t) => Some(text_members(t)),
        _ => None,
    }
}

fn text_members(t: &TextValue) -> Props {
    let s = |v: &str| Value::Str(v.to_owned());
    let os = |v: &Option<String>| v.as_deref().map_or(Value::Null, s);
    match t {
        TextValue::Base { source, namespace, key } => Props::from([
            ("source".to_owned(), s(source)),
            ("namespace".to_owned(), s(namespace)),
            ("key".to_owned(), s(key)),
        ]),
        TextValue::Table { source, table, key } => {
            Props::from([("source".to_owned(), os(source)), ("table".to_owned(), s(table)), ("key".to_owned(), s(key))])
        }
        TextValue::Invariant { source } => Props::from([("source".to_owned(), os(source))]),
    }
}

/// Merges `over` onto `base` in place: an object (a struct, or a text as its members) member by member, anything
/// else replaces.
pub fn merge_defaults(base: &mut Props, over: &Props) {
    for (k, v) in over {
        match (base.get_mut(k), v) {
            (Some(Value::Struct(b)), Value::Struct(o)) => merge_defaults(b, o),
            (Some(b), v) if object_members(b).is_some() && object_members(v).is_some() => {
                let mut m = object_members(b).expect("checked");
                merge_defaults(&mut m, &object_members(v).expect("checked"));
                *b = Value::Struct(m);
            }
            _ => {
                base.insert(k.clone(), v.clone());
            }
        }
    }
}

/// The merged defaults of a class default object of class `cls` with its own properties `own`: up the class's
/// SuperStruct chain while it is a Blueprint class (an export of some package), each class's `Default__<class>`
/// export of its package. An error when a parent's package is not in the paks (its super stays an unresolved import
/// of a /Game path): its defaults would be missing silently.
pub fn class_defaults(cache: &PackageCache, cls: &Resolved, own: &Props) -> Result<Props, ObjectError> {
    let err = |m: String| ObjectError(m);
    let resolve = |p: &Arc<Package>, i: i32| cache.try_resolve(p, i).map_err(|e| err(format!("{}: {e}", p.path)));
    let mut chain: Vec<Arc<UObj>> = Vec::new();
    let mut c = match cls {
        Resolved::Export { pkg, index } => resolve(pkg, pkg.exports[*index].super_index)?,
        _ => None,
    };
    let mut depth = 0;
    while let Some(r) = c {
        if depth > 64 {
            return Err(err(format!("typed: class chain of {} is deeper than 64", cls.object_path(cache))));
        }
        depth += 1;
        let Resolved::Export { pkg, index } = &r else {
            let p = r.object_path(cache);
            if !p.starts_with("/Script/") {
                return Err(err(format!("typed: parent class {p} of {} did not resolve", cls.object_path(cache))));
            }
            break; // a native class: its defaults are code
        };
        let name = format!("Default__{}", pkg.exports[*index].object_name);
        let Some(at) = pkg.exports.iter().position(|e| e.outer == 0 && e.object_name == name) else {
            return Err(err(format!("typed: {} has no {name}", pkg.path)));
        };
        let o = cache.object(pkg, at);
        if !o.issues.is_empty() {
            return Err(err(issues_text(&pkg.path, &name, &o.issues.iter().collect::<Vec<_>>())));
        }
        check_members_of(&o.props).map_err(ObjectError)?;
        chain.push(o);
        c = resolve(pkg, pkg.exports[*index].super_index)?;
    }
    let mut out = Props::new();
    for o in chain.iter().rev() {
        merge_defaults(&mut out, &o.props);
    }
    merge_defaults(&mut out, own);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_names() {
        let g = "0123456789ABCDEF0123456789ABCDEF";
        assert_eq!(member_name(&format!("Speed_12_{g}")), "Speed");
        assert_eq!(member_name(&format!("A_1_2_{g}")), "A_1");
        assert_eq!(member_name(&format!("_3_{g}")), &format!("_3_{g}"));
        assert_eq!(member_name(&format!("Speed_x_{g}")), &format!("Speed_x_{g}"));
        assert_eq!(member_name(&format!("Speed_1_{}", g.to_lowercase())), &format!("Speed_1_{}", g.to_lowercase()));
        assert_eq!(member_name("Speed"), "Speed");
    }

    #[test]
    fn member_name_multibyte() {
        // a two-byte character across the GUID's first byte
        let s = format!("A_1_é{}", "0".repeat(31));
        assert_eq!(member_name(&s), s);
    }

    #[test]
    fn floats_as_dotnet() {
        // where the f32's shortest text (ryu-js) and .NET's differ: values as the object kind writes them
        for (bits, want) in [
            (0x0F80_0000u32, "1.26217745e-29"),
            (0x6B00_0000, "1.54742505e+26"),
            (0x6C80_0000, "1.23794004e+27"),
            (0x15AE_43FD, "7.0385307e-26"),
        ] {
            let mut out = Vec::new();
            let n = f32_dotnet(f32::from_bits(bits));
            let mut b = ryu_js::Buffer::new();
            out.extend_from_slice(b.format_finite(n).as_bytes());
            assert_eq!(String::from_utf8(out).unwrap(), want, "{bits:#x}");
        }
        assert_eq!(f32_dotnet(0.1), 0.1);
        assert_eq!(f32_dotnet(-2.5), -2.5);
        assert_eq!(f32_dotnet(16777216.0), 16777216.0);
    }

    #[test]
    fn merge_text_into_struct() {
        let mut base = Props::from([(
            "T".to_owned(),
            Value::Text(TextValue::Base { source: "a".into(), namespace: "n".into(), key: "k".into() }),
        )]);
        let over = Props::from([(
            "T".to_owned(),
            Value::Text(TextValue::Table { source: None, table: "t".into(), key: "k2".into() }),
        )]);
        merge_defaults(&mut base, &over);
        let m = base["T"].as_struct().unwrap();
        let keys: Vec<&str> = m.keys().map(String::as_str).collect();
        assert_eq!(keys, ["source", "namespace", "key", "table"]);
        assert!(matches!(m["source"], Value::Null));
    }
}
