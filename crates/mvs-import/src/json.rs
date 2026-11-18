//! JSON as `JSON.stringify` prints it, so a kind's entry is byte-equal to a JavaScript importer's: keys in
//! insertion order (an object's integer-like keys first, ascending, as a JS object orders them), numbers as JS prints
//! a double (`ryu-js`; an integer exactly up to 2^53), a non-finite number as `null`, strings with JSON.stringify's
//! escapes. A kind writes its header or entry as a `serde::Serialize` value through [`to_vec`]: a struct's fields in
//! declaration order (an `Option` field that JS leaves `undefined` is `#[serde(skip_serializing_if =
//! "Option::is_none")]`, one it sets to `null` is a plain `Option`), a map as an object, a sequence or tuple as an
//! array, a unit variant as its name, a newtype variant as `{"<variant>":<value>}` (serde_json's external tag).
//! [`write_string`] and [`write_number`] are the pieces a hand-written writer (the object kind's) prints with.
//!
//! An f32 is printed as the double it widens to (a JS number read from a Float32Array); a value rounded the
//! .NET way first is given as `mvs_uasset::typed::f32_dotnet(v)`.
//!
//! [`Json`] is a JSON value with its object members in insertion order: what a bind builds where a JavaScript bind
//! built a plain object of open shape (a scenery part's blocks, an emitter written whole, a menu field), and what it
//! reads back of another entry or of the object kind's JSON ([`value_json`]).

use std::fmt::Display;

use indexmap::IndexMap;
use mvs_uasset::PackageCache;
use mvs_uasset::value::Value;
use serde::de::{self, Deserialize, Deserializer};
use serde::ser::{self, Serialize, SerializeMap, SerializeSeq};

/// A JSON value as a JS bind holds it: a number is a double, an object keeps its members in the order they were
/// set (printed by [`to_vec`] as JSON.stringify orders a JS object: array-index keys first, ascending).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Json {
    #[default]
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(IndexMap<String, Json>),
}

impl Json {
    /// An empty object.
    pub fn obj() -> Json {
        Json::Obj(IndexMap::new())
    }

    /// An object of these members, in this order.
    pub fn object<K: Into<String>>(members: impl IntoIterator<Item = (K, Json)>) -> Json {
        Json::Obj(members.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    /// The member `k` of an object.
    pub fn get(&self, k: &str) -> Option<&Json> {
        match self {
            Json::Obj(m) => m.get(k),
            _ => None,
        }
    }

    /// Sets member `k` of an object (a new key goes last, an old one keeps its place, as a JS object's); `None` when
    /// this is no object.
    pub fn set(&mut self, k: impl Into<String>, v: Json) -> Option<()> {
        match self {
            Json::Obj(m) => {
                m.insert(k.into(), v);
                Some(())
            }
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&IndexMap<String, Json>> {
        match self {
            Json::Obj(m) => Some(m),
            _ => None,
        }
    }

    /// JSON.parse of `bytes`.
    pub fn parse(bytes: &[u8]) -> std::result::Result<Json, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

impl From<bool> for Json {
    fn from(v: bool) -> Json {
        Json::Bool(v)
    }
}

impl From<f64> for Json {
    fn from(v: f64) -> Json {
        Json::Num(v)
    }
}

impl From<&str> for Json {
    fn from(v: &str) -> Json {
        Json::Str(v.to_owned())
    }
}

impl From<String> for Json {
    fn from(v: String) -> Json {
        Json::Str(v)
    }
}

impl<T: Into<Json>> From<Option<T>> for Json {
    fn from(v: Option<T>) -> Json {
        v.map_or(Json::Null, Into::into)
    }
}

impl<T: Into<Json>> From<Vec<T>> for Json {
    fn from(v: Vec<T>) -> Json {
        Json::Arr(v.into_iter().map(Into::into).collect())
    }
}

impl Serialize for Json {
    fn serialize<S: ser::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Json::Null => s.serialize_unit(),
            Json::Bool(b) => s.serialize_bool(*b),
            Json::Num(n) => s.serialize_f64(*n),
            Json::Str(v) => s.serialize_str(v),
            Json::Arr(a) => {
                let mut q = s.serialize_seq(Some(a.len()))?;
                for x in a {
                    q.serialize_element(x)?;
                }
                q.end()
            }
            Json::Obj(m) => {
                let mut q = s.serialize_map(Some(m.len()))?;
                for (k, v) in m {
                    q.serialize_entry(k, v)?;
                }
                q.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Json, D::Error> {
        struct V;
        impl<'de> de::Visitor<'de> for V {
            type Value = Json;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON value")
            }
            fn visit_bool<E>(self, v: bool) -> std::result::Result<Json, E> {
                Ok(Json::Bool(v))
            }
            fn visit_i64<E>(self, v: i64) -> std::result::Result<Json, E> {
                Ok(Json::Num(v as f64))
            }
            fn visit_u64<E>(self, v: u64) -> std::result::Result<Json, E> {
                Ok(Json::Num(v as f64))
            }
            fn visit_f64<E>(self, v: f64) -> std::result::Result<Json, E> {
                Ok(Json::Num(v))
            }
            fn visit_str<E>(self, v: &str) -> std::result::Result<Json, E> {
                Ok(Json::Str(v.to_owned()))
            }
            fn visit_string<E>(self, v: String) -> std::result::Result<Json, E> {
                Ok(Json::Str(v))
            }
            fn visit_unit<E>(self) -> std::result::Result<Json, E> {
                Ok(Json::Null)
            }
            fn visit_none<E>(self) -> std::result::Result<Json, E> {
                Ok(Json::Null)
            }
            fn visit_some<D: Deserializer<'de>>(self, d: D) -> std::result::Result<Json, D::Error> {
                Json::deserialize(d)
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut a: A) -> std::result::Result<Json, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element()? {
                    v.push(x);
                }
                Ok(Json::Arr(v))
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut a: A) -> std::result::Result<Json, A::Error> {
                // (a key met twice keeps its first place and its last value, as JSON.parse)
                let mut m = IndexMap::new();
                while let Some((k, v)) = a.next_entry::<String, Json>()? {
                    m.insert(k, v);
                }
                Ok(Json::Obj(m))
            }
        }
        d.deserialize_any(V)
    }
}

/// A value of an object's properties as the object kind's JSON holds it (what a bind reads of `o.properties`):
/// references as object paths, a text as its record, numbers as the kind prints them. A binary section (an entry's
/// body only) panics, as `kinds::object::write_value_json`.
pub fn value_json(cache: &PackageCache, v: &Value) -> Json {
    let mut out = Vec::new();
    crate::kinds::object::write_value_json(cache, v, &mut out);
    Json::parse(&out).unwrap_or_else(|e| panic!("the object kind's JSON of a value reads back: {e}"))
}

/// Tagged properties as the object kind's JSON holds an object's `properties` (`o.properties`).
pub fn props_json(cache: &PackageCache, p: &mvs_uasset::value::Props) -> IndexMap<String, Json> {
    let mut out = Vec::new();
    crate::kinds::object::write_props_json(cache, p, &mut out);
    match Json::parse(&out) {
        Ok(Json::Obj(m)) => m,
        Ok(_) => unreachable!("properties write as an object"),
        Err(e) => panic!("the object kind's JSON of properties reads back: {e}"),
    }
}

/// A value [`to_vec`] cannot print as JSON.stringify would (bytes, a map key that is no string or number, a
/// tuple or struct variant).
#[derive(Debug, thiserror::Error)]
#[error("json: {0}")]
pub struct JsonError(pub String);

impl ser::Error for JsonError {
    fn custom<T: Display>(msg: T) -> JsonError {
        JsonError(msg.to_string())
    }
}

type Result<T> = std::result::Result<T, JsonError>;

/// The JSON of `v`, as `JSON.stringify` prints the same value.
pub fn to_vec<T: Serialize + ?Sized>(v: &T) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    v.serialize(&mut Ser { out: &mut out })?;
    Ok(out)
}

/// A JSON string as JSON.stringify writes it: `"`, `\` and the C0 controls escaped (`\b \f \n \r \t`, the rest
/// `\u00xx`), everything else as it is.
pub fn write_string(out: &mut Vec<u8>, s: &str) {
    out.push(b'"');
    let mut start = 0;
    for (i, c) in s.bytes().enumerate() {
        let esc: &[u8] = match c {
            b'"' => b"\\\"",
            b'\\' => b"\\\\",
            b'\n' => b"\\n",
            b'\r' => b"\\r",
            b'\t' => b"\\t",
            0x08 => b"\\b",
            0x0c => b"\\f",
            0..0x20 => b"",
            _ => continue,
        };
        out.extend_from_slice(&s.as_bytes()[start..i]);
        if esc.is_empty() {
            out.extend_from_slice(format!("\\u{c:04x}").as_bytes());
        } else {
            out.extend_from_slice(esc);
        }
        start = i + 1;
    }
    out.extend_from_slice(&s.as_bytes()[start..]);
    out.push(b'"');
}

/// A number as JSON.stringify writes a double: the shortest text that reads back (`ryu-js`, JS's own rules for
/// exponents), `null` for a NaN or an infinity.
pub fn write_number(out: &mut Vec<u8>, d: f64) {
    if d.is_finite() {
        let mut b = ryu_js::Buffer::new();
        out.extend_from_slice(b.format_finite(d).as_bytes());
    } else {
        out.extend_from_slice(b"null");
    }
}

/// An integer as JSON.stringify writes the double it is: exact up to 2^53, past that the double's text.
pub fn write_int(out: &mut Vec<u8>, v: i128) {
    if v.unsigned_abs() <= 1 << 53 {
        out.extend_from_slice(v.to_string().as_bytes());
    } else {
        write_number(out, v as f64);
    }
}

/// A key a JS object moves ahead of the others: a canonical array index (`0`, `17`; not `01`, not 2^32 - 1).
fn array_index(k: &str) -> Option<u32> {
    if k.is_empty() || (k.len() > 1 && k.starts_with('0')) || !k.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    k.parse::<u32>().ok().filter(|&n| n != u32::MAX)
}

struct Ser<'a> {
    out: &'a mut Vec<u8>,
}

/// A sequence, tuple or struct being written.
struct Compound<'a, 'b> {
    ser: &'b mut Ser<'a>,
    first: bool,
}

/// A map being written: its entries held until the end, which orders them as a JS object does.
struct MapState<'a, 'b> {
    ser: &'b mut Ser<'a>,
    entries: Vec<(String, Vec<u8>)>,
    key: Option<String>,
}

impl<'a, 'b> ser::Serializer for &'b mut Ser<'a> {
    type Ok = ();
    type Error = JsonError;
    type SerializeSeq = Compound<'a, 'b>;
    type SerializeTuple = Compound<'a, 'b>;
    type SerializeTupleStruct = Compound<'a, 'b>;
    type SerializeTupleVariant = ser::Impossible<(), JsonError>;
    type SerializeMap = MapState<'a, 'b>;
    type SerializeStruct = Compound<'a, 'b>;
    type SerializeStructVariant = ser::Impossible<(), JsonError>;

    fn serialize_bool(self, v: bool) -> Result<()> {
        self.out.extend_from_slice(if v { b"true" } else { b"false" });
        Ok(())
    }
    fn serialize_i8(self, v: i8) -> Result<()> {
        self.serialize_i64(i64::from(v))
    }
    fn serialize_i16(self, v: i16) -> Result<()> {
        self.serialize_i64(i64::from(v))
    }
    fn serialize_i32(self, v: i32) -> Result<()> {
        self.serialize_i64(i64::from(v))
    }
    fn serialize_i64(self, v: i64) -> Result<()> {
        write_int(self.out, i128::from(v));
        Ok(())
    }
    fn serialize_u8(self, v: u8) -> Result<()> {
        self.serialize_u64(u64::from(v))
    }
    fn serialize_u16(self, v: u16) -> Result<()> {
        self.serialize_u64(u64::from(v))
    }
    fn serialize_u32(self, v: u32) -> Result<()> {
        self.serialize_u64(u64::from(v))
    }
    fn serialize_u64(self, v: u64) -> Result<()> {
        write_int(self.out, i128::from(v));
        Ok(())
    }
    fn serialize_f32(self, v: f32) -> Result<()> {
        write_number(self.out, f64::from(v));
        Ok(())
    }
    fn serialize_f64(self, v: f64) -> Result<()> {
        write_number(self.out, v);
        Ok(())
    }
    fn serialize_char(self, v: char) -> Result<()> {
        write_string(self.out, v.encode_utf8(&mut [0; 4]));
        Ok(())
    }
    fn serialize_str(self, v: &str) -> Result<()> {
        write_string(self.out, v);
        Ok(())
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<()> {
        Err(JsonError("bytes have no JSON.stringify form (a section of the body, or an array of numbers)".into()))
    }
    fn serialize_none(self) -> Result<()> {
        self.serialize_unit()
    }
    fn serialize_some<T: Serialize + ?Sized>(self, v: &T) -> Result<()> {
        v.serialize(self)
    }
    fn serialize_unit(self) -> Result<()> {
        self.out.extend_from_slice(b"null");
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<()> {
        self.serialize_unit()
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, variant: &'static str) -> Result<()> {
        self.serialize_str(variant)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T) -> Result<()> {
        v.serialize(self)
    }
    /// An externally tagged variant (serde's default): `{"<variant>":<value>}`, as serde_json writes it.
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        v: &T,
    ) -> Result<()> {
        self.out.push(b'{');
        write_string(self.out, variant);
        self.out.push(b':');
        v.serialize(&mut *self)?;
        self.out.push(b'}');
        Ok(())
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Compound<'a, 'b>> {
        self.out.push(b'[');
        Ok(Compound { ser: self, first: true })
    }
    fn serialize_tuple(self, len: usize) -> Result<Compound<'a, 'b>> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<Compound<'a, 'b>> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        name: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Err(JsonError(format!("{name}::{variant}: a variant with data has no JSON.stringify form")))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<MapState<'a, 'b>> {
        Ok(MapState { ser: self, entries: Vec::new(), key: None })
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Compound<'a, 'b>> {
        self.out.push(b'{');
        Ok(Compound { ser: self, first: true })
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant> {
        Err(JsonError(format!("{name}::{variant}: a variant with data has no JSON.stringify form")))
    }
}

impl Compound<'_, '_> {
    fn comma(&mut self) {
        if !self.first {
            self.ser.out.push(b',');
        }
        self.first = false;
    }
}

impl ser::SerializeSeq for Compound<'_, '_> {
    type Ok = ();
    type Error = JsonError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<()> {
        self.comma();
        v.serialize(&mut *self.ser)
    }
    fn end(self) -> Result<()> {
        self.ser.out.push(b']');
        Ok(())
    }
}

impl ser::SerializeTuple for Compound<'_, '_> {
    type Ok = ();
    type Error = JsonError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<()> {
        ser::SerializeSeq::serialize_element(self, v)
    }
    fn end(self) -> Result<()> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for Compound<'_, '_> {
    type Ok = ();
    type Error = JsonError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<()> {
        ser::SerializeSeq::serialize_element(self, v)
    }
    fn end(self) -> Result<()> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeStruct for Compound<'_, '_> {
    type Ok = ();
    type Error = JsonError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, v: &T) -> Result<()> {
        self.comma();
        write_string(self.ser.out, key);
        self.ser.out.push(b':');
        v.serialize(&mut *self.ser)
    }
    fn end(self) -> Result<()> {
        self.ser.out.push(b'}');
        Ok(())
    }
}

impl ser::SerializeMap for MapState<'_, '_> {
    type Ok = ();
    type Error = JsonError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<()> {
        self.key = Some(key.serialize(KeySer)?);
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<()> {
        let key = self.key.take().ok_or_else(|| JsonError("a map value without its key".into()))?;
        let mut out = Vec::new();
        v.serialize(&mut Ser { out: &mut out })?;
        // a key given twice keeps its first place and takes the last value (a JS object's assignment)
        match self.entries.iter_mut().find(|e| e.0 == key) {
            Some(e) => e.1 = out,
            None => self.entries.push((key, out)),
        }
        Ok(())
    }
    fn end(mut self) -> Result<()> {
        if self.entries.iter().any(|e| array_index(&e.0).is_some()) {
            // integer-like keys first in ascending order, the rest in insertion order (stable)
            self.entries.sort_by_key(|e| array_index(&e.0).map_or((1, 0), |n| (0, n)));
        }
        let out = &mut *self.ser.out;
        out.push(b'{');
        for (i, (k, v)) in self.entries.iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            write_string(out, k);
            out.push(b':');
            out.extend_from_slice(v);
        }
        out.push(b'}');
        Ok(())
    }
}

/// A map key as the string JS makes of it.
struct KeySer;

impl ser::Serializer for KeySer {
    type Ok = String;
    type Error = JsonError;
    type SerializeSeq = ser::Impossible<String, JsonError>;
    type SerializeTuple = ser::Impossible<String, JsonError>;
    type SerializeTupleStruct = ser::Impossible<String, JsonError>;
    type SerializeTupleVariant = ser::Impossible<String, JsonError>;
    type SerializeMap = ser::Impossible<String, JsonError>;
    type SerializeStruct = ser::Impossible<String, JsonError>;
    type SerializeStructVariant = ser::Impossible<String, JsonError>;

    fn serialize_str(self, v: &str) -> Result<String> {
        Ok(v.to_owned())
    }
    fn serialize_char(self, v: char) -> Result<String> {
        Ok(v.to_string())
    }
    fn serialize_i8(self, v: i8) -> Result<String> {
        Ok(v.to_string())
    }
    fn serialize_i16(self, v: i16) -> Result<String> {
        Ok(v.to_string())
    }
    fn serialize_i32(self, v: i32) -> Result<String> {
        Ok(v.to_string())
    }
    fn serialize_i64(self, v: i64) -> Result<String> {
        Ok(v.to_string())
    }
    fn serialize_u8(self, v: u8) -> Result<String> {
        Ok(v.to_string())
    }
    fn serialize_u16(self, v: u16) -> Result<String> {
        Ok(v.to_string())
    }
    fn serialize_u32(self, v: u32) -> Result<String> {
        Ok(v.to_string())
    }
    fn serialize_u64(self, v: u64) -> Result<String> {
        Ok(v.to_string())
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, variant: &'static str) -> Result<String> {
        Ok(variant.to_owned())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T) -> Result<String> {
        v.serialize(self)
    }
    fn serialize_bool(self, _: bool) -> Result<String> {
        Err(key_error())
    }
    fn serialize_f32(self, _: f32) -> Result<String> {
        Err(key_error())
    }
    fn serialize_f64(self, _: f64) -> Result<String> {
        Err(key_error())
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<String> {
        Err(key_error())
    }
    fn serialize_none(self) -> Result<String> {
        Err(key_error())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<String> {
        Err(key_error())
    }
    fn serialize_unit(self) -> Result<String> {
        Err(key_error())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<String> {
        Err(key_error())
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<String> {
        Err(key_error())
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq> {
        Err(key_error())
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple> {
        Err(key_error())
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct> {
        Err(key_error())
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Err(key_error())
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap> {
        Err(key_error())
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct> {
        Err(key_error())
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant> {
        Err(key_error())
    }
}

fn key_error() -> JsonError {
    JsonError("a map key must be a string or an integer".into())
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use serde::Serialize;

    use super::*;

    fn s<T: Serialize>(v: &T) -> String {
        String::from_utf8(to_vec(v).unwrap()).unwrap()
    }

    #[derive(Serialize)]
    struct Header {
        path: String,
        bones: Vec<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        undefined: Option<u32>,
        null: Option<u32>,
        pose: Vec<f64>,
        pair: (i32, &'static str),
    }

    #[test]
    fn as_json_stringify() {
        let h = Header {
            path: "/Game/A.A".into(),
            bones: vec!["root", "a\"b\\c\n\u{1}é"],
            undefined: None,
            null: None,
            pose: vec![0.0, -0.0, 1.0, 0.1, 1e21, 1.5e-7, 123456789012345680000.0, f64::NAN, f64::INFINITY],
            pair: (-3, "x"),
        };
        assert_eq!(
            s(&h),
            r#"{"path":"/Game/A.A","bones":["root","a\"b\\c\n\u0001é"],"null":null,"pose":[0,0,1,0.1,1e+21,1.5e-7,123456789012345680000,null,null],"pair":[-3,"x"]}"#
        );
        // f32 as the double it widens to
        assert_eq!(s(&0.1f32), "0.10000000149011612");
        // integers past 2^53 as their double
        assert_eq!(s(&(1u64 << 53)), "9007199254740992");
        assert_eq!(s(&((1u64 << 53) + 1)), "9007199254740992");
        assert_eq!(s(&u64::MAX), "18446744073709552000");
        // a newtype variant under its external tag
        #[derive(Serialize)]
        enum Tagged {
            Scene(Vec<u8>),
        }
        assert_eq!(s(&Tagged::Scene(vec![1, 2])), r#"{"Scene":[1,2]}"#);
    }

    #[test]
    fn object_key_order() {
        let mut m: IndexMap<String, u32> = IndexMap::new();
        for (k, v) in [("b", 1), ("10", 2), ("a", 3), ("2", 4), ("02", 5), ("4294967295", 6), ("0", 7)] {
            m.insert(k.into(), v);
        }
        assert_eq!(s(&m), r#"{"0":7,"2":4,"10":2,"b":1,"a":3,"02":5,"4294967295":6}"#);
        let plain: IndexMap<&str, &str> = [("z", "1"), ("y", "2")].into_iter().collect();
        assert_eq!(s(&plain), r#"{"z":"1","y":"2"}"#);
    }

    #[test]
    fn json_values() {
        let text = r#"{"b":1,"2":[true,null,"x",0.1,-0],"a":{"z":1e21}}"#;
        let v = Json::parse(text.as_bytes()).unwrap();
        assert_eq!(v.as_object().unwrap().keys().collect::<Vec<_>>(), ["b", "2", "a"]);
        assert_eq!(s(&v), r#"{"2":[true,null,"x",0.1,0],"b":1,"a":{"z":1e+21}}"#);
        let mut o = Json::object([("k", Json::from("v")), ("n", Json::from(1.5))]);
        o.set("k", Json::from(vec![1.0, 2.0]));
        o.set("m", Json::from(None::<String>));
        assert_eq!(s(&o), r#"{"k":[1,2],"n":1.5,"m":null}"#);
    }
}
