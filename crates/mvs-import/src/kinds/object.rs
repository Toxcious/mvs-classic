//! The `object` kind: one top-level package export as a typed object (`mvs_uasset::typed`), as JSON, with its inner
//! objects (the exports whose outer chain reaches it: `objview::inner_exports`), keyed by their name chain below it.
//! An entry that holds a binary section is the json-bin container (magic `MVOB`, the JSON as its header, the
//! sections as its body).
//! The JSON is what `JSON.stringify` makes of the typed object.
//!
//! The template of a kind: a unit struct, its [`Kind`] impl (list from the catalog, build through `Cx::read`), the
//! entry built by plain functions the CLI and tests call too.

use std::sync::Arc;

use indexmap::IndexMap;
use mvs_uasset::typed::{AssetObject, BodySink, ObjectError, f32_dotnet, member_name, to_asset_object};
use mvs_uasset::value::{Props, TextValue, Value};
use mvs_uasset::{Package, PackageCache};

use crate::json;
use crate::jsonbin::{BodyRef, write_json_bin};
use crate::kind::{Cx, Item, Kind, KindError};
use crate::objview::inner_exports;

/// The kind's version.
pub const OBJECT_KIND_VERSION: u32 = 11;
/// The json-bin container's magic.
pub const OBJECT_MAGIC: &[u8; 4] = b"MVOB";

/// The `object` kind. Built only as the proof of the reader (no reader in the game reads it): `stored` false.
pub struct Object;

impl Kind for Object {
    fn id(&self) -> &'static str {
        "object"
    }

    fn version(&self) -> u64 {
        OBJECT_KIND_VERSION.into()
    }

    /// `application/json`, or `application/octet-stream` for a json-bin container.
    fn content_type(&self, bytes: &[u8]) -> &'static str {
        if bytes.first() == Some(&b'{') { "application/json" } else { "application/octet-stream" }
    }

    /// Every top-level export of the build.
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(cx.catalog.exports().map(|e| Item::new(e.object_path())).collect())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let (pkg, top) = cx.read(&item.path)?;
        Ok(entry_bytes(cx.cache, &pkg, top)?)
    }

    fn stored(&self) -> bool {
        false
    }
}

/// The entry of top-level export `top` of `pkg`.
pub fn entry_bytes(cache: &PackageCache, pkg: &Arc<Package>, top: usize) -> Result<Vec<u8>, ObjectError> {
    let mut body = BodySink::default();
    let mut out = to_asset_object(cache, pkg, top, &mut body)?;
    let mut inner: IndexMap<String, AssetObject> = IndexMap::new();
    for (i, name) in inner_exports(cache, pkg, top) {
        let o = to_asset_object(cache, pkg, i, &mut body)?;
        inner.insert(name, o);
    }
    out.inner = inner;
    let mut json = Vec::new();
    write_object_json(cache, &out, &mut json);
    if body.sections.is_empty() {
        return Ok(json);
    }
    let refs: Vec<BodyRef> = body.refs.iter().map(|&(offset, length)| BodyRef { offset, length }).collect();
    Ok(write_json_bin(OBJECT_MAGIC, &json, &body.sections, &refs))
}

/// The object as the kind's JSON: values as `mvs_uasset::value` describes, every reference as its object path.
pub fn write_object_json(cache: &PackageCache, o: &AssetObject, out: &mut Vec<u8>) {
    let mut w = Writer { cache, out };
    w.object(o);
}

/// One value as the kind's JSON writes it (a binary section, which only an entry's body holds, panics).
pub fn write_value_json(cache: &PackageCache, v: &Value, out: &mut Vec<u8>) {
    Writer { cache, out }.value(v);
}

/// Tagged properties as the kind's JSON writes an object's `properties` (members by declared name).
pub fn write_props_json(cache: &PackageCache, p: &Props, out: &mut Vec<u8>) {
    Writer { cache, out }.props(p);
}

struct Writer<'a> {
    cache: &'a PackageCache,
    out: &'a mut Vec<u8>,
}

impl Writer<'_> {
    fn object(&mut self, o: &AssetObject) {
        self.out.push(b'{');
        self.key_first("path");
        self.string(&o.path);
        self.key("class");
        self.string(&o.class);
        self.key("index");
        self.out.extend_from_slice(o.index.to_string().as_bytes());
        self.key("properties");
        self.props(&o.properties);
        if let Some(d) = &o.defaults {
            self.key("defaults");
            self.props(d);
        }
        if let Some(s) = &o.super_ {
            self.key("super");
            self.string(s);
        }
        if let Some(t) = &o.template {
            self.key("template");
            self.string(t);
        }
        if let Some(n) = &o.native {
            self.key("native");
            self.props(n);
        }
        if let Some(p) = &o.partial {
            self.key("partial");
            self.string(p);
        }
        if !o.inner.is_empty() {
            self.key("inner");
            self.out.push(b'{');
            for (i, (k, v)) in o.inner.iter().enumerate() {
                if i > 0 {
                    self.out.push(b',');
                }
                self.string(k);
                self.out.push(b':');
                self.object(v);
            }
            self.out.push(b'}');
        }
        self.out.push(b'}');
    }

    fn key_first(&mut self, k: &str) {
        self.string(k);
        self.out.push(b':');
    }

    fn key(&mut self, k: &str) {
        self.out.push(b',');
        self.key_first(k);
    }

    fn props(&mut self, p: &Props) {
        self.out.push(b'{');
        for (i, (k, v)) in p.iter().enumerate() {
            if i > 0 {
                self.out.push(b',');
            }
            self.string(member_name(k));
            self.out.push(b':');
            self.value(v);
        }
        self.out.push(b'}');
    }

    fn opt_string(&mut self, s: Option<&str>) {
        match s {
            Some(s) => self.string(s),
            None => self.out.extend_from_slice(b"null"),
        }
    }

    fn value(&mut self, v: &Value) {
        match v {
            Value::Null | Value::Ref(None) => self.out.extend_from_slice(b"null"),
            Value::Bool(b) => self.out.extend_from_slice(if *b { b"true" } else { b"false" }),
            Value::Int(i) => self.int(i128::from(*i)),
            Value::UInt(u) => self.int(i128::from(*u)),
            Value::Float(f) => self.float(*f),
            Value::Double(d) => self.double(*d),
            Value::Str(s) => self.string(s),
            Value::Array(a) => {
                self.out.push(b'[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        self.out.push(b',');
                    }
                    self.value(x);
                }
                self.out.push(b']');
            }
            Value::Struct(p) => self.props(p),
            Value::Map(m) => {
                self.out.push(b'[');
                for (i, (k, x)) in m.iter().enumerate() {
                    if i > 0 {
                        self.out.push(b',');
                    }
                    self.out.push(b'[');
                    self.value(k);
                    self.out.push(b',');
                    self.value(x);
                    self.out.push(b']');
                }
                self.out.push(b']');
            }
            Value::Ref(Some(r)) => {
                let p = r.object_path(self.cache);
                self.string(&p);
            }
            Value::SoftPath { asset, sub } => {
                if asset == "None" || asset.is_empty() {
                    self.out.extend_from_slice(b"null");
                } else if sub.is_empty() {
                    self.string(asset);
                } else {
                    self.string(&format!("{asset}:{sub}"));
                }
            }
            Value::Text(t) => {
                self.out.push(b'{');
                match t {
                    TextValue::Base { source, namespace, key } => {
                        self.key_first("source");
                        self.string(source);
                        self.key("namespace");
                        self.string(namespace);
                        self.key("key");
                        self.string(key);
                    }
                    TextValue::Table { source, table, key } => {
                        self.key_first("source");
                        self.opt_string(source.as_deref());
                        self.key("table");
                        self.string(table);
                        self.key("key");
                        self.string(key);
                    }
                    TextValue::Invariant { source } => {
                        self.key_first("source");
                        self.opt_string(source.as_deref());
                    }
                }
                self.out.push(b'}');
            }
            Value::Bytes(_) => panic!("typed: binary data outside a native extra (no body to lay it out in)"),
        }
    }

    /// An integer as JSON.stringify writes the double it is.
    fn int(&mut self, v: i128) {
        json::write_int(self.out, v);
    }

    fn float(&mut self, f: f32) {
        if f.is_finite() {
            self.double(f32_dotnet(f));
        } else {
            self.non_finite(f64::from(f));
        }
    }

    /// A double as JSON.stringify writes it; a NaN or an infinity as the string the kind held.
    fn double(&mut self, d: f64) {
        if d.is_finite() {
            json::write_number(self.out, d);
        } else {
            self.non_finite(d);
        }
    }

    fn non_finite(&mut self, d: f64) {
        self.string(if d.is_nan() {
            "NaN"
        } else if d > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        });
    }

    fn string(&mut self, s: &str) {
        json::write_string(self.out, s);
    }
}
