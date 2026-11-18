//! The gamedata kind's encoder: a typed record (or a [`Value`](super::Value) tree) as the entry
//! the `encodeGameData` of a JavaScript importer writes of its value, so that [`decode_entry`](super::decode_entry)
//! reads it back with every bit. Members in the order the record serializes them (a struct's in field order, a
//! member a `skip_serializing_if` skips left out as an `undefined` one); a map's keys in a JS object's order (the
//! canonical array-index keys first, ascending, then the rest in insertion order); a number as JS `String(n)`
//! (`ryu-js`), `-0` as `-0`, a NaN or an infinity as its `$num` tag; a `serialize_bytes` (a Float32Array's bytes) as a
//! `$f32` tag naming its section of the body, each section 16-byte aligned.

use std::fmt;

use serde::ser::{self, Serialize};

use crate::dump::js_string;

const MAGIC: &[u8; 4] = b"MVGD";
/// The keys of the encoding's tags: an object whose only key is one would read back as the tag.
const TAGS: [&str; 2] = ["$num", "$f32"];

#[derive(Debug, thiserror::Error)]
#[error("gamedata encode: {0}")]
pub struct EncodeError(String);

impl ser::Error for EncodeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        EncodeError(msg.to_string())
    }
}

/// The entry's bytes for `v`: "MVGD", the JSON's length (u32 LE), the JSON, padding to 16, the body.
pub fn encode<T: Serialize + ?Sized>(v: &T) -> Result<Vec<u8>, EncodeError> {
    let mut o = Out::default();
    v.serialize(&mut o)?;
    // the JSON with each section's tag where it stands, the sections laid out in that order
    let mut json = Vec::with_capacity(o.text.len() + o.f32s.len() * 24);
    let mut body: Vec<u8> = Vec::new();
    let mut last = 0;
    for (at, bytes) in &o.f32s {
        json.extend_from_slice(&o.text[last..*at]);
        last = *at;
        json.extend_from_slice(format!("{{\"$f32\":[{},{}]}}", body.len(), bytes.len()).as_bytes());
        body.extend_from_slice(bytes);
        body.resize(align16(body.len()), 0);
    }
    json.extend_from_slice(&o.text[last..]);
    let len = u32::try_from(json.len()).map_err(|_| EncodeError(format!("a JSON of {} bytes", json.len())))?;
    let mut out = Vec::with_capacity(align16(8 + json.len()) + body.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&json);
    out.resize(align16(out.len()), 0);
    out.extend_from_slice(&body);
    Ok(out)
}

fn align16(n: usize) -> usize {
    (n + 15) & !15
}

/// A number as the encoding writes it.
fn write_num(out: &mut Vec<u8>, n: f64) {
    if n.is_nan() {
        out.extend_from_slice(br#"{"$num":"NaN"}"#);
    } else if n == f64::INFINITY {
        out.extend_from_slice(br#"{"$num":"Infinity"}"#);
    } else if n == f64::NEG_INFINITY {
        out.extend_from_slice(br#"{"$num":"-Infinity"}"#);
    } else if n == 0.0 && n.is_sign_negative() {
        out.extend_from_slice(b"-0");
    } else {
        out.extend_from_slice(ryu_js::Buffer::new().format_finite(n).as_bytes());
    }
}

fn write_str(out: &mut Vec<u8>, s: &str) {
    let mut q = String::with_capacity(s.len() + 2);
    js_string(&mut q, s);
    out.extend_from_slice(q.as_bytes());
}

/// The JSON written so far, and each Float32Array met: where its tag goes in the text and its bytes. The tags are
/// written once the sections' order is final (a map's members are reordered after they are written).
#[derive(Default)]
struct Out {
    text: Vec<u8>,
    f32s: Vec<(usize, Vec<u8>)>,
}

impl Out {
    fn append(&mut self, o: Out) {
        let base = self.text.len();
        self.text.extend_from_slice(&o.text);
        self.f32s.extend(o.f32s.into_iter().map(|(at, b)| (at + base, b)));
    }
}

/// A canonical array index (`0`, `17`; not `01`, not 2^32 - 1): a key a JS object puts first, in ascending order.
fn array_index(k: &str) -> Option<u32> {
    if k.is_empty() || (k.len() > 1 && k.starts_with('0')) || !k.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    k.parse::<u32>().ok().filter(|&n| n != u32::MAX)
}

type R = Result<(), EncodeError>;

fn variant(what: &str) -> EncodeError {
    EncodeError(format!("{what}: the encoding has no tagged enums"))
}

impl<'a> ser::Serializer for &'a mut Out {
    type Ok = ();
    type Error = EncodeError;
    type SerializeSeq = Seq<'a>;
    type SerializeTuple = Seq<'a>;
    type SerializeTupleStruct = Seq<'a>;
    type SerializeTupleVariant = ser::Impossible<(), EncodeError>;
    type SerializeMap = Map<'a>;
    type SerializeStruct = Struct<'a>;
    type SerializeStructVariant = ser::Impossible<(), EncodeError>;

    fn serialize_bool(self, v: bool) -> R {
        self.text.extend_from_slice(if v { b"true" } else { b"false" });
        Ok(())
    }
    fn serialize_i8(self, v: i8) -> R {
        self.serialize_f64(v.into())
    }
    fn serialize_i16(self, v: i16) -> R {
        self.serialize_f64(v.into())
    }
    fn serialize_i32(self, v: i32) -> R {
        self.serialize_f64(v.into())
    }
    // (a JS number: an integer past 2^53 is the double nearest it)
    #[allow(clippy::cast_precision_loss)]
    fn serialize_i64(self, v: i64) -> R {
        self.serialize_f64(v as f64)
    }
    fn serialize_u8(self, v: u8) -> R {
        self.serialize_f64(v.into())
    }
    fn serialize_u16(self, v: u16) -> R {
        self.serialize_f64(v.into())
    }
    fn serialize_u32(self, v: u32) -> R {
        self.serialize_f64(v.into())
    }
    #[allow(clippy::cast_precision_loss)]
    fn serialize_u64(self, v: u64) -> R {
        self.serialize_f64(v as f64)
    }
    // (a number read from a Float32Array: the double it widens to)
    fn serialize_f32(self, v: f32) -> R {
        self.serialize_f64(v.into())
    }
    fn serialize_f64(self, v: f64) -> R {
        write_num(&mut self.text, v);
        Ok(())
    }
    fn serialize_char(self, v: char) -> R {
        self.serialize_str(v.encode_utf8(&mut [0; 4]))
    }
    fn serialize_str(self, v: &str) -> R {
        write_str(&mut self.text, v);
        Ok(())
    }
    // (a Float32Array's little-endian bytes)
    fn serialize_bytes(self, v: &[u8]) -> R {
        if !v.len().is_multiple_of(4) {
            return Err(EncodeError(format!("a Float32Array of {} bytes", v.len())));
        }
        self.f32s.push((self.text.len(), v.to_vec()));
        Ok(())
    }
    fn serialize_none(self) -> R {
        self.text.extend_from_slice(b"null");
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, v: &T) -> R {
        v.serialize(self)
    }
    fn serialize_unit(self) -> R {
        self.serialize_none()
    }
    fn serialize_unit_struct(self, _: &'static str) -> R {
        self.serialize_none()
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, variant: &'static str) -> R {
        self.serialize_str(variant)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T) -> R {
        v.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, n: &'static str, _: u32, _: &'static str, _: &T) -> R {
        Err(variant(n))
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Seq<'a>, EncodeError> {
        self.text.push(b'[');
        Ok(Seq { out: self, first: true })
    }
    fn serialize_tuple(self, len: usize) -> Result<Seq<'a>, EncodeError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<Seq<'a>, EncodeError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        n: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, EncodeError> {
        Err(variant(n))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Map<'a>, EncodeError> {
        Ok(Map { out: self, members: Vec::new(), key: None })
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Struct<'a>, EncodeError> {
        self.text.push(b'{');
        Ok(Struct { out: self, count: 0, first: None })
    }
    fn serialize_struct_variant(
        self,
        n: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, EncodeError> {
        Err(variant(n))
    }
}

pub struct Seq<'a> {
    out: &'a mut Out,
    first: bool,
}

impl Seq<'_> {
    fn element<T: Serialize + ?Sized>(&mut self, v: &T) -> R {
        if !self.first {
            self.out.text.push(b',');
        }
        self.first = false;
        v.serialize(&mut *self.out)
    }
    fn close(self) -> R {
        self.out.text.push(b']');
        Ok(())
    }
}

impl ser::SerializeSeq for Seq<'_> {
    type Ok = ();
    type Error = EncodeError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> R {
        self.element(v)
    }
    fn end(self) -> R {
        self.close()
    }
}

impl ser::SerializeTuple for Seq<'_> {
    type Ok = ();
    type Error = EncodeError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> R {
        self.element(v)
    }
    fn end(self) -> R {
        self.close()
    }
}

impl ser::SerializeTupleStruct for Seq<'_> {
    type Ok = ();
    type Error = EncodeError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> R {
        self.element(v)
    }
    fn end(self) -> R {
        self.close()
    }
}

/// A struct: its fields as they come.
pub struct Struct<'a> {
    out: &'a mut Out,
    count: usize,
    first: Option<&'static str>,
}

impl ser::SerializeStruct for Struct<'_> {
    type Ok = ();
    type Error = EncodeError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, k: &'static str, v: &T) -> R {
        if self.count > 0 {
            self.out.text.push(b',');
        }
        self.count += 1;
        self.first.get_or_insert(k);
        write_str(&mut self.out.text, k);
        self.out.text.push(b':');
        v.serialize(&mut *self.out)
    }
    fn end(self) -> R {
        if let (1, Some(k)) = (self.count, self.first)
            && TAGS.contains(&k)
        {
            return Err(EncodeError(format!("an object of the one key {k}, a tag of the encoding")));
        }
        self.out.text.push(b'}');
        Ok(())
    }
}

/// A map: its members held until the end, which orders them as a JS object does.
pub struct Map<'a> {
    out: &'a mut Out,
    members: Vec<(String, Out)>,
    key: Option<String>,
}

impl ser::SerializeMap for Map<'_> {
    type Ok = ();
    type Error = EncodeError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, k: &T) -> R {
        self.key = Some(k.serialize(KeyName)?);
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> R {
        let k = self.key.take().ok_or_else(|| EncodeError("a map value before its key".into()))?;
        let mut o = Out::default();
        v.serialize(&mut o)?;
        self.members.push((k, o));
        Ok(())
    }
    fn end(self) -> R {
        let Map { out, mut members, .. } = self;
        if members.len() == 1 && TAGS.contains(&members[0].0.as_str()) {
            return Err(EncodeError(format!("an object of the one key {}, a tag of the encoding", members[0].0)));
        }
        // (a stable sort: the array-index keys first by their value, the rest keep their order)
        members.sort_by_key(|(k, _)| array_index(k).map_or((1, 0), |i| (0, i)));
        out.text.push(b'{');
        for (i, (k, o)) in members.into_iter().enumerate() {
            if i > 0 {
                out.text.push(b',');
            }
            write_str(&mut out.text, &k);
            out.text.push(b':');
            out.append(o);
        }
        out.text.push(b'}');
        Ok(())
    }
}

/// A map key: a string (an integer key as its digits, as a JS object holds it).
struct KeyName;

fn not_a_key() -> EncodeError {
    EncodeError("a map key that is not a string or an integer".into())
}

impl ser::Serializer for KeyName {
    type Ok = String;
    type Error = EncodeError;
    type SerializeSeq = ser::Impossible<String, EncodeError>;
    type SerializeTuple = ser::Impossible<String, EncodeError>;
    type SerializeTupleStruct = ser::Impossible<String, EncodeError>;
    type SerializeTupleVariant = ser::Impossible<String, EncodeError>;
    type SerializeMap = ser::Impossible<String, EncodeError>;
    type SerializeStruct = ser::Impossible<String, EncodeError>;
    type SerializeStructVariant = ser::Impossible<String, EncodeError>;

    fn serialize_str(self, v: &str) -> Result<String, EncodeError> {
        Ok(v.to_owned())
    }
    fn serialize_char(self, v: char) -> Result<String, EncodeError> {
        Ok(v.to_string())
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, v: &'static str) -> Result<String, EncodeError> {
        Ok(v.to_owned())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T) -> Result<String, EncodeError> {
        v.serialize(self)
    }
    fn serialize_i8(self, v: i8) -> Result<String, EncodeError> {
        Ok(v.to_string())
    }
    fn serialize_i16(self, v: i16) -> Result<String, EncodeError> {
        Ok(v.to_string())
    }
    fn serialize_i32(self, v: i32) -> Result<String, EncodeError> {
        Ok(v.to_string())
    }
    fn serialize_i64(self, v: i64) -> Result<String, EncodeError> {
        Ok(v.to_string())
    }
    fn serialize_u8(self, v: u8) -> Result<String, EncodeError> {
        Ok(v.to_string())
    }
    fn serialize_u16(self, v: u16) -> Result<String, EncodeError> {
        Ok(v.to_string())
    }
    fn serialize_u32(self, v: u32) -> Result<String, EncodeError> {
        Ok(v.to_string())
    }
    fn serialize_u64(self, v: u64) -> Result<String, EncodeError> {
        Ok(v.to_string())
    }
    fn serialize_bool(self, _: bool) -> Result<String, EncodeError> {
        Err(not_a_key())
    }
    fn serialize_f32(self, _: f32) -> Result<String, EncodeError> {
        Err(not_a_key())
    }
    fn serialize_f64(self, _: f64) -> Result<String, EncodeError> {
        Err(not_a_key())
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<String, EncodeError> {
        Err(not_a_key())
    }
    fn serialize_none(self) -> Result<String, EncodeError> {
        Err(not_a_key())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<String, EncodeError> {
        Err(not_a_key())
    }
    fn serialize_unit(self) -> Result<String, EncodeError> {
        Err(not_a_key())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<String, EncodeError> {
        Err(not_a_key())
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<String, EncodeError> {
        Err(not_a_key())
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, EncodeError> {
        Err(not_a_key())
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, EncodeError> {
        Err(not_a_key())
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct, EncodeError> {
        Err(not_a_key())
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, EncodeError> {
        Err(not_a_key())
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, EncodeError> {
        Err(not_a_key())
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, EncodeError> {
        Err(not_a_key())
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, EncodeError> {
        Err(not_a_key())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Props, Value, decode_entry, tests::sample};
    use super::*;

    #[test]
    fn the_ts_sample_byte_for_byte() {
        let v = decode_entry(&sample()).unwrap();
        // (the encoder pads the last section to 16 as well; the hand-made sample stops at its floats)
        let mut want = sample();
        want.resize(align16(want.len()), 0);
        assert_eq!(encode(&v).unwrap(), want);
    }

    #[test]
    fn numbers_as_js_prints_them() {
        let text = |n: f64| {
            let mut o = Vec::new();
            write_num(&mut o, n);
            String::from_utf8(o).unwrap()
        };
        let want = [
            (0.0, "0"),
            (-0.0, "-0"),
            (3.0, "3"),
            (-2.5, "-2.5"),
            (0.1, "0.1"),
            (1e21, "1e+21"),
            (1e-7, "1e-7"),
            (123456789012345680000.0, "123456789012345680000"),
            (f64::from(0.1f32), "0.10000000149011612"),
            (f64::NAN, r#"{"$num":"NaN"}"#),
            (f64::NEG_INFINITY, r#"{"$num":"-Infinity"}"#),
        ];
        for (n, s) in want {
            assert_eq!(text(n), s, "{n:e}");
        }
    }

    #[test]
    fn maps_in_js_key_order_with_their_sections() {
        let mut p = Props::new();
        p.insert("b".into(), Value::F32s(vec![1.0]));
        p.insert("10".into(), Value::Num(1.0));
        p.insert("2".into(), Value::F32s(vec![2.0, 3.0]));
        p.insert("01".into(), Value::Null);
        let e = encode(&Value::Obj(p)).unwrap();
        let len = u32::from_le_bytes(e[4..8].try_into().unwrap()) as usize;
        assert_eq!(
            std::str::from_utf8(&e[8..8 + len]).unwrap(),
            r#"{"2":{"$f32":[0,8]},"10":1,"b":{"$f32":[16,4]},"01":null}"#
        );
        let v = decode_entry(&e).unwrap();
        let o = v.as_obj().unwrap();
        assert_eq!(o["2"], Value::F32s(vec![2.0, 3.0]));
        assert_eq!(o["b"], Value::F32s(vec![1.0]));
        assert_eq!(e.len(), ((8 + len + 15) & !15) + 32);
    }

    #[test]
    fn a_lone_tag_key_is_refused() {
        let mut p = Props::new();
        p.insert("$num".into(), Value::Str("NaN".into()));
        assert!(encode(&Value::Obj(p)).is_err());
    }
}
