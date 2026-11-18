//! The gamedata kind's encoding: an entry is "MVGD", a u32 JSON length,
//! the JSON, padding to 16, then the body (16-byte aligned sections). Numbers keep their bits: the JSON holds the
//! shortest decimal of each double and `-0`; a NaN or an infinity is `{"$num":"NaN" | "Infinity" | "-Infinity"}`; a
//! `Float32Array` is `{"$f32":[offset, bytes]}`, its little-endian floats a section of the body.
//!
//! An entry decodes to a [`Value`] tree (objects in their order), and the typed records deserialize from that tree:
//! a number that is a whole number of at most 2^53 (not -0) is offered as an integer, any other as an `f64`, so an
//! integer field takes only whole numbers and a float field takes every number with its bits.

use std::fmt;

use indexmap::IndexMap;
use serde::de::{self, DeserializeSeed, IntoDeserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

mod encode;
pub use encode::{EncodeError, encode};

/// An untyped value of the gamedata kind: a property bag's member (a class default object's property, a notify's
/// properties), kept with its exact number and its key order.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Props),
    /// A `Float32Array`.
    F32s(Vec<f32>),
}

/// A property bag (a class default object's `Record<string, any>`), in key order.
pub type Props = IndexMap<String, Value>;

impl Value {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_obj(&self) -> Option<&Props> {
        match self {
            Value::Obj(o) => Some(o),
            _ => None,
        }
    }
    pub fn as_arr(&self) -> Option<&[Value]> {
        match self {
            Value::Arr(a) => Some(a),
            _ => None,
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

/// The frames of a `Float32Array` (the hit box tables' section).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct F32s(pub Vec<f32>);

const MAGIC: &[u8; 4] = b"MVGD";
const MAX_SAFE: f64 = 9007199254740992.0;

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("not a gamedata entry: {0}")]
    Container(String),
    #[error("{path}: {msg}")]
    Json { path: String, msg: String },
}

/// The entry's value tree.
pub fn decode_entry(bytes: &[u8]) -> Result<Value, DecodeError> {
    if bytes.len() < 8 || &bytes[..4] != MAGIC {
        return Err(DecodeError::Container("no MVGD".into()));
    }
    let len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let json =
        bytes.get(8..8 + len).ok_or_else(|| DecodeError::Container(format!("a {len}-byte JSON past the end")))?;
    let body = bytes.get((8 + len + 15) & !15..).unwrap_or(&[]);
    let mut de = serde_json::Deserializer::from_slice(json);
    let v = ValueSeed { body }
        .deserialize(&mut de)
        .map_err(|e| DecodeError::Json { path: "JSON".into(), msg: e.to_string() })?;
    de.end().map_err(|e| DecodeError::Json { path: String::new(), msg: e.to_string() })?;
    Ok(v)
}

/// A typed record from a value tree, the failing member's path in the error.
pub fn from_value<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, DecodeError> {
    serde_path_to_error::deserialize(v)
        .map_err(|e| DecodeError::Json { path: e.path().to_string(), msg: e.inner().to_string() })
}

#[derive(Clone, Copy)]
struct ValueSeed<'b> {
    body: &'b [u8],
}

impl<'de> DeserializeSeed<'de> for ValueSeed<'_> {
    type Value = Value;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ValueSeed<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a gamedata value")
    }
    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Num(v as f64))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Num(v as f64))
    }
    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Num(v))
    }
    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::Str(v.to_string()))
    }
    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::Str(v))
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut out = Vec::with_capacity(a.size_hint().unwrap_or(0));
        while let Some(v) = a.next_element_seed(self)? {
            out.push(v);
        }
        Ok(Value::Arr(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut out = Props::new();
        let mut first = true;
        while let Some(k) = a.next_key::<String>()? {
            if first && k == "$num" {
                let s: String = a.next_value()?;
                let n = match s.as_str() {
                    "NaN" => f64::NAN,
                    "Infinity" => f64::INFINITY,
                    "-Infinity" => f64::NEG_INFINITY,
                    _ => return Err(de::Error::custom(format!("$num {s}"))),
                };
                if a.next_key::<String>()?.is_some() {
                    return Err(de::Error::custom("a $num tag with other members"));
                }
                return Ok(Value::Num(n));
            }
            if first && k == "$f32" {
                let (at, len): (usize, usize) = a.next_value()?;
                if a.next_key::<String>()?.is_some() {
                    return Err(de::Error::custom("a $f32 tag with other members"));
                }
                let b = self.body.get(at..at + len).filter(|_| len.is_multiple_of(4)).ok_or_else(|| {
                    de::Error::custom(format!("$f32 section {at}+{len} of a {}-byte body", self.body.len()))
                })?;
                return Ok(Value::F32s(le_floats(b)));
            }
            first = false;
            let v = a.next_value_seed(self)?;
            out.insert(k, v);
        }
        Ok(Value::Obj(out))
    }
}

// ---- the typed records from a value tree

// (a Float32Array goes through a deserializer as the map {"$f32": its bytes}, so a value or an F32s rebuilds it as
// it was, an empty one included, and no array of numbers turns into one)
const F32_TAG: &str = "$f32";

impl<'de> Deserializer<'de> for Value {
    type Error = de::value::Error;

    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        match self {
            Value::Null => v.visit_unit(),
            Value::Bool(b) => v.visit_bool(b),
            Value::Num(n) => {
                if n.fract() == 0.0 && n.abs() <= MAX_SAFE && !(n == 0.0 && n.is_sign_negative()) {
                    if n >= 0.0 { v.visit_u64(n as u64) } else { v.visit_i64(n as i64) }
                } else {
                    v.visit_f64(n)
                }
            }
            Value::Str(s) => v.visit_string(s),
            Value::Arr(a) => {
                let len = a.len();
                let mut it = a.into_iter();
                let out = v.visit_seq(Seq { it: &mut it })?;
                if it.next().is_some() {
                    return Err(de::Error::invalid_length(len, &"fewer items"));
                }
                Ok(out)
            }
            Value::Obj(o) => v.visit_map(Map { it: o.into_iter(), value: None }),
            Value::F32s(f) => v.visit_map(F32Map { bytes: Some(f32_bytes(&f)), keyed: false }),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        match self {
            Value::Null => v.visit_none(),
            x => v.visit_some(x),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, Self::Error> {
        match self {
            Value::Str(s) => v.visit_enum(s.into_deserializer()),
            x => x.deserialize_any(v),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _name: &'static str, v: V) -> Result<V::Value, Self::Error> {
        v.visit_newtype_struct(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf unit unit_struct seq tuple
        tuple_struct map struct identifier ignored_any
    }
}

impl<'de> IntoDeserializer<'de, de::value::Error> for Value {
    type Deserializer = Value;
    fn into_deserializer(self) -> Value {
        self
    }
}

struct Seq<'a> {
    it: &'a mut std::vec::IntoIter<Value>,
}

impl<'de> SeqAccess<'de> for Seq<'_> {
    type Error = de::value::Error;
    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, s: T) -> Result<Option<T::Value>, Self::Error> {
        match self.it.next() {
            Some(v) => s.deserialize(v).map(Some),
            None => Ok(None),
        }
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.it.len())
    }
}

struct Map {
    it: indexmap::map::IntoIter<String, Value>,
    value: Option<Value>,
}

impl<'de> MapAccess<'de> for Map {
    type Error = de::value::Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, s: K) -> Result<Option<K::Value>, Self::Error> {
        match self.it.next() {
            Some((k, v)) => {
                self.value = Some(v);
                s.deserialize(de::value::StringDeserializer::new(k)).map(Some)
            }
            None => Ok(None),
        }
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, s: V) -> Result<V::Value, Self::Error> {
        s.deserialize(self.value.take().expect("a map value before its key"))
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.it.len())
    }
}

struct F32Map {
    bytes: Option<Vec<u8>>,
    keyed: bool,
}

impl<'de> MapAccess<'de> for F32Map {
    type Error = de::value::Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, s: K) -> Result<Option<K::Value>, Self::Error> {
        if self.keyed {
            return Ok(None);
        }
        self.keyed = true;
        s.deserialize(de::value::StrDeserializer::new(F32_TAG)).map(Some)
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, s: V) -> Result<V::Value, Self::Error> {
        let b = self.bytes.take().expect("the $f32 value before its key");
        s.deserialize(de::value::BytesDeserializer::new(&b))
    }
}

/// A Float32Array's bytes as a deserializer offers them.
struct Bytes(Vec<u8>);

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Bytes, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Bytes;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a Float32Array's bytes")
            }
            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Bytes, E> {
                Ok(Bytes(v.to_vec()))
            }
            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Bytes, E> {
                Ok(Bytes(v))
            }
        }
        d.deserialize_byte_buf(V)
    }
}

fn floats<E: de::Error>(b: Bytes) -> Result<Vec<f32>, E> {
    if !b.0.len().is_multiple_of(4) {
        return Err(E::custom(format!("a Float32Array of {} bytes", b.0.len())));
    }
    Ok(le_floats(&b.0))
}

fn le_floats(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

// ---- Value itself as a member of a typed record

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Value, D::Error> {
        d.deserialize_any(TreeVisitor)
    }
}

struct TreeVisitor;

impl<'de> Visitor<'de> for TreeVisitor {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a value")
    }
    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Num(v as f64))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Num(v as f64))
    }
    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Num(v))
    }
    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::Str(v))
    }
    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::Str(v.to_string()))
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut out = Vec::with_capacity(a.size_hint().unwrap_or(0));
        while let Some(v) = a.next_element()? {
            out.push(v);
        }
        Ok(Value::Arr(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut out = Props::with_capacity(a.size_hint().unwrap_or(0));
        let mut first = true;
        while let Some(k) = a.next_key::<String>()? {
            if first && k == F32_TAG {
                return Ok(Value::F32s(floats(a.next_value::<Bytes>()?)?));
            }
            first = false;
            let v = a.next_value()?;
            out.insert(k, v);
        }
        Ok(Value::Obj(out))
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => s.serialize_unit(),
            Value::Bool(b) => s.serialize_bool(*b),
            Value::Num(n) => s.serialize_f64(*n),
            Value::Str(x) => s.serialize_str(x),
            Value::Arr(a) => a.serialize(s),
            Value::Obj(o) => o.serialize(s),
            Value::F32s(f) => s.serialize_bytes(&f32_bytes(f)),
        }
    }
}

impl<'de> Deserialize<'de> for F32s {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<F32s, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = F32s;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a Float32Array")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<F32s, A::Error> {
                match a.next_key::<String>()? {
                    Some(k) if k == F32_TAG => {}
                    _ => return Err(de::Error::custom("not a Float32Array")),
                }
                let f = floats(a.next_value::<Bytes>()?)?;
                if a.next_key::<String>()?.is_some() {
                    return Err(de::Error::custom("a Float32Array with other members"));
                }
                Ok(F32s(f))
            }
        }
        d.deserialize_map(V)
    }
}

impl Serialize for F32s {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(&f32_bytes(&self.0))
    }
}

fn f32_bytes(f: &[f32]) -> Vec<u8> {
    f.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// `T | null` of a member that may also be absent (`?: T | null`): `None` absent, `Some(None)` null. With
/// `#[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]`.
pub fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    // an entry as a JavaScript encoder writes {a: -0, b: NaN, c: -Infinity, d: 1e21, e: 0.1, f: Float32Array [1.5, -2],
    // g: [null, 3]}
    pub(super) fn sample() -> Vec<u8> {
        let json = br#"{"a":-0,"b":{"$num":"NaN"},"c":{"$num":"-Infinity"},"d":1e+21,"e":0.1,"f":{"$f32":[0,8]},"g":[null,3]}"#;
        let mut out = b"MVGD".to_vec();
        out.extend((json.len() as u32).to_le_bytes());
        out.extend(json);
        out.resize((out.len() + 15) & !15, 0);
        out.extend(1.5f32.to_le_bytes());
        out.extend((-2f32).to_le_bytes());
        out
    }

    #[test]
    fn numbers_keep_their_bits() {
        let v = decode_entry(&sample()).unwrap();
        let o = v.as_obj().unwrap();
        assert_eq!(o["a"].as_f64().unwrap().to_bits(), (-0f64).to_bits());
        assert!(o["b"].as_f64().unwrap().is_nan());
        assert_eq!(o["c"].as_f64(), Some(f64::NEG_INFINITY));
        assert_eq!(o["d"].as_f64(), Some(1e21));
        assert_eq!(o["e"].as_f64().unwrap().to_bits(), 0.1f64.to_bits());
        assert_eq!(o["f"], Value::F32s(vec![1.5, -2.0]));
        assert_eq!(o["g"], Value::Arr(vec![Value::Null, Value::Num(3.0)]));
    }

    #[derive(Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct Typed {
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: Value,
        f: F32s,
        g: Vec<Option<u32>>,
    }

    #[test]
    fn typed_records_and_their_dump() {
        let t: Typed = from_value(decode_entry(&sample()).unwrap()).unwrap();
        assert_eq!(t.a.to_bits(), (-0f64).to_bits());
        assert_eq!(t.f.0, vec![1.5, -2.0]);
        let d = crate::dump::dump(&t).unwrap();
        let want = [
            "{",
            "\"a\"=d8000000000000000",
            "\"b\"=d7ff8000000000000",
            "\"c\"=dfff0000000000000",
            "\"d\"=d444b1ae4d6e2ef50",
            "\"e\"=d3fb999999999999a",
            &format!("\"f\"=f32x2:{:016x}", crate::dump::fnv64(&[0, 0, 0xc0, 0x3f, 0, 0, 0, 0xc0])),
            "\"g\"=[",
            "n",
            "d4008000000000000",
            "]",
            "}",
            "",
        ]
        .join("\n");
        assert_eq!(d, want);
    }

    #[test]
    fn integer_fields_refuse_fractions_and_minus_zero() {
        let v = Value::Arr(vec![Value::Num(-0.0)]);
        assert!(from_value::<Vec<i32>>(v).is_err());
        let v = Value::Arr(vec![Value::Num(0.5)]);
        assert!(from_value::<Vec<i32>>(v).is_err());
    }

    #[test]
    fn an_unknown_member_is_refused() {
        let mut o = Props::new();
        o.insert("x".into(), Value::Num(1.0));
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        #[allow(dead_code)]
        struct Y {
            y: Option<f64>,
        }
        assert!(from_value::<Y>(Value::Obj(o)).is_err());
    }
}
