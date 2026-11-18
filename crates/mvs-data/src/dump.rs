//! The canonical dump of a typed record: the text a JavaScript `dumpGameData` writes of the
//! value the record was decoded from. One item a line: `n`, `t`, `f`; a number as `d` and the 16 hex digits of its
//! double's bits; a string as JSON.stringify writes it; a Float32Array as `f32x<count>:<FNV-1a 64 of its bytes>`; an
//! array as `[`, its items, `]`; an object as `{`, its members, `}`, a member as its key (a JSON string), `=` and its
//! value's first line. A record's dump equal to the dump of its entry's own value proves its types hold every value the
//! entry holds, of the same type, with every object's keys in the same order.

use std::fmt::{self, Write};

use serde::ser::{self, Serialize};

/// The canonical dump of `v`.
pub fn dump<T: Serialize + ?Sized>(v: &T) -> Result<String, DumpError> {
    let mut d = Dump { out: String::new(), lead: String::new() };
    v.serialize(&mut d)?;
    Ok(d.out)
}

#[derive(Debug, thiserror::Error)]
#[error("gamedata dump: {0}")]
pub struct DumpError(String);

impl ser::Error for DumpError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        DumpError(msg.to_string())
    }
}

/// FNV-1a 64 of `b`.
pub fn fnv64(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &x in b {
        h ^= x as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// `s` as JSON.stringify writes it (`"`, `\` and the controls escaped; the controls with no short form as `\u00xx`;
/// lone surrogates cannot be in a Rust string).
pub fn js_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Dump {
    out: String,
    // the member's `"key"=` the next line starts with
    lead: String,
}

impl Dump {
    fn line(&mut self, s: &str) {
        self.out.push_str(&self.lead);
        self.lead.clear();
        self.out.push_str(s);
        self.out.push('\n');
    }
    fn num(&mut self, n: f64) {
        let s = format!("d{:016x}", n.to_bits());
        self.line(&s);
    }
    fn key(&mut self, k: &str) {
        self.lead.clear();
        js_string(&mut self.lead, k);
        self.lead.push('=');
    }
}

type R = Result<(), DumpError>;

impl<'a> ser::Serializer for &'a mut Dump {
    type Ok = ();
    type Error = DumpError;
    type SerializeSeq = &'a mut Dump;
    type SerializeTuple = &'a mut Dump;
    type SerializeTupleStruct = &'a mut Dump;
    type SerializeTupleVariant = &'a mut Dump;
    type SerializeMap = &'a mut Dump;
    type SerializeStruct = &'a mut Dump;
    type SerializeStructVariant = &'a mut Dump;

    fn serialize_bool(self, v: bool) -> R {
        self.line(if v { "t" } else { "f" });
        Ok(())
    }
    fn serialize_i8(self, v: i8) -> R {
        self.num(v as f64);
        Ok(())
    }
    fn serialize_i16(self, v: i16) -> R {
        self.num(v as f64);
        Ok(())
    }
    fn serialize_i32(self, v: i32) -> R {
        self.num(v as f64);
        Ok(())
    }
    fn serialize_i64(self, v: i64) -> R {
        self.num(v as f64);
        Ok(())
    }
    fn serialize_u8(self, v: u8) -> R {
        self.num(v as f64);
        Ok(())
    }
    fn serialize_u16(self, v: u16) -> R {
        self.num(v as f64);
        Ok(())
    }
    fn serialize_u32(self, v: u32) -> R {
        self.num(v as f64);
        Ok(())
    }
    fn serialize_u64(self, v: u64) -> R {
        self.num(v as f64);
        Ok(())
    }
    fn serialize_f32(self, v: f32) -> R {
        self.num(v as f64);
        Ok(())
    }
    fn serialize_f64(self, v: f64) -> R {
        self.num(v);
        Ok(())
    }
    fn serialize_char(self, v: char) -> R {
        self.serialize_str(v.encode_utf8(&mut [0; 4]))
    }
    fn serialize_str(self, v: &str) -> R {
        let mut s = String::with_capacity(v.len() + 2);
        js_string(&mut s, v);
        self.line(&s);
        Ok(())
    }
    // (a Float32Array's bytes)
    fn serialize_bytes(self, v: &[u8]) -> R {
        let s = format!("f32x{}:{:016x}", v.len() / 4, fnv64(v));
        self.line(&s);
        Ok(())
    }
    fn serialize_none(self) -> R {
        self.line("n");
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, v: &T) -> R {
        v.serialize(self)
    }
    fn serialize_unit(self) -> R {
        self.line("n");
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> R {
        self.line("n");
        Ok(())
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, variant: &'static str) -> R {
        self.serialize_str(variant)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T) -> R {
        v.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, _: u32, _: &'static str, _: &T) -> R {
        Err(ser::Error::custom("an externally tagged enum"))
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self, DumpError> {
        self.line("[");
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self, DumpError> {
        self.line("[");
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, DumpError> {
        self.line("[");
        Ok(self)
    }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Result<Self, DumpError> {
        Err(ser::Error::custom("an externally tagged enum"))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self, DumpError> {
        self.line("{");
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, DumpError> {
        self.line("{");
        Ok(self)
    }
    fn serialize_struct_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Result<Self, DumpError> {
        Err(ser::Error::custom("an externally tagged enum"))
    }
}

impl ser::SerializeSeq for &mut Dump {
    type Ok = ();
    type Error = DumpError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        self.line("]");
        Ok(())
    }
}

impl ser::SerializeTuple for &mut Dump {
    type Ok = ();
    type Error = DumpError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        self.line("]");
        Ok(())
    }
}

impl ser::SerializeTupleStruct for &mut Dump {
    type Ok = ();
    type Error = DumpError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        self.line("]");
        Ok(())
    }
}

impl ser::SerializeTupleVariant for &mut Dump {
    type Ok = ();
    type Error = DumpError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &T) -> R {
        unreachable!()
    }
    fn end(self) -> R {
        unreachable!()
    }
}

impl ser::SerializeMap for &mut Dump {
    type Ok = ();
    type Error = DumpError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, k: &T) -> R {
        let k = k.serialize(KeyName)?;
        self.key(&k);
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> R {
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        self.line("}");
        Ok(())
    }
}

impl ser::SerializeStruct for &mut Dump {
    type Ok = ();
    type Error = DumpError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, k: &'static str, v: &T) -> R {
        self.key(k);
        v.serialize(&mut **self)
    }
    fn end(self) -> R {
        self.line("}");
        Ok(())
    }
}

impl ser::SerializeStructVariant for &mut Dump {
    type Ok = ();
    type Error = DumpError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, _: &T) -> R {
        unreachable!()
    }
    fn end(self) -> R {
        unreachable!()
    }
}

/// A map key: a string alone (every key of the data is one).
struct KeyName;

impl ser::Serializer for KeyName {
    type Ok = String;
    type Error = DumpError;
    type SerializeSeq = ser::Impossible<String, DumpError>;
    type SerializeTuple = ser::Impossible<String, DumpError>;
    type SerializeTupleStruct = ser::Impossible<String, DumpError>;
    type SerializeTupleVariant = ser::Impossible<String, DumpError>;
    type SerializeMap = ser::Impossible<String, DumpError>;
    type SerializeStruct = ser::Impossible<String, DumpError>;
    type SerializeStructVariant = ser::Impossible<String, DumpError>;

    fn serialize_str(self, v: &str) -> Result<String, DumpError> {
        Ok(v.to_string())
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, v: &'static str) -> Result<String, DumpError> {
        Ok(v.to_string())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T) -> Result<String, DumpError> {
        v.serialize(self)
    }
    fn serialize_bool(self, _: bool) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_i8(self, _: i8) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_i16(self, _: i16) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_i32(self, _: i32) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_i64(self, _: i64) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_u8(self, _: u8) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_u16(self, _: u16) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_u32(self, _: u32) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_u64(self, _: u64) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_f32(self, _: f32) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_f64(self, _: f64) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_char(self, v: char) -> Result<String, DumpError> {
        Ok(v.to_string())
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_none(self) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_unit(self) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<String, DumpError> {
        Err(not_a_key())
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, DumpError> {
        Err(not_a_key())
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, DumpError> {
        Err(not_a_key())
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct, DumpError> {
        Err(not_a_key())
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, DumpError> {
        Err(not_a_key())
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, DumpError> {
        Err(not_a_key())
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, DumpError> {
        Err(not_a_key())
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, DumpError> {
        Err(not_a_key())
    }
}

fn not_a_key() -> DumpError {
    DumpError("a map key that is not a string".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_vectors() {
        assert_eq!(fnv64(b""), 0xcbf29ce484222325);
        assert_eq!(fnv64(b"a"), 0xaf63dc4c8601ec8c);
        assert_eq!(fnv64(b"foobar"), 0x85944171f73967e8);
    }

    #[test]
    fn strings_as_json_stringify() {
        let mut s = String::new();
        js_string(&mut s, "a\"b\\c\n\u{1}\u{7f}é");
        assert_eq!(s, "\"a\\\"b\\\\c\\n\\u0001\u{7f}é\"");
    }
}
