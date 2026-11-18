//! How one field of the record is written, read, compared and shown: the `Field` trait, its leaves (`f64`, `bool`,
//! `String`), `Option`, `Vec`, and the `record!` macro that gives a struct of fields all of it in field order.
//!
//! The bytes, little-endian: an `f64` its 8 bytes (every NaN written as the one quiet NaN 0x7ff8000000000000, so
//! the bytes of two records are equal exactly when their floats have the same bits, the sign of zero kept), a
//! `bool` one byte 0 / 1, a `String` its UTF-8 length as a LEB128 varint and the bytes, an `Option` a byte 0 / 1
//! and the value, a `Vec` its length as a varint and the items. The oracle writer
//! writes the same.

use std::fmt::Write as _;

use crate::TraceError;

/// The one NaN a trace holds.
pub const NAN_BITS: u64 = 0x7ff8_0000_0000_0000;

/// A difference between two records: the field's path and both values as `show` writes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difference {
    pub path: String,
    pub expected: String,
    pub actual: String,
}

/// The bytes of a record being read.
pub struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub fn done(&self) -> bool {
        self.at == self.bytes.len()
    }

    pub fn position(&self) -> usize {
        self.at
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8], TraceError> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.bytes.len()).ok_or(TraceError::Short(self.at))?;
        let s = &self.bytes[self.at..end];
        self.at = end;
        Ok(s)
    }

    pub fn u8(&mut self) -> Result<u8, TraceError> {
        Ok(self.take(1)?[0])
    }

    pub fn u32(&mut self) -> Result<u32, TraceError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    pub fn varint(&mut self) -> Result<usize, TraceError> {
        let mut v: u64 = 0;
        for shift in (0..64).step_by(7) {
            let b = self.u8()?;
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return usize::try_from(v).map_err(|_| TraceError::Bad(self.at, "varint too large"));
            }
        }
        Err(TraceError::Bad(self.at, "varint too long"))
    }
}

pub fn put_varint(w: &mut Vec<u8>, mut v: usize) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            w.push(b);
            return;
        }
        w.push(b | 0x80);
    }
}

/// A field of the record. `diff` appends a `Difference` per leaf that differs (by bits for a float); `schema` names
/// each leaf below `path` with its kind, the list the file's header holds and a reader checks.
pub trait Field: Sized {
    fn put(&self, w: &mut Vec<u8>);
    fn get(r: &mut Cursor) -> Result<Self, TraceError>;
    fn diff(a: &Self, b: &Self, path: &mut String, out: &mut Vec<Difference>);
    /// The value on one line (a leaf, or a short form of a list or a struct).
    fn show(&self) -> String;
    /// The value as indented lines below `path`, one leaf a line.
    fn show_tree(&self, path: &mut String, out: &mut String) {
        let _ = writeln!(out, "{path} = {}", self.show());
    }
    /// The value at `path` below this field (a `Difference`'s path with this field's own taken off) as `show`
    /// writes it: `len` of a list, `none` of an option with nothing in it, the other variant or kind of an enum or
    /// an event, `absent` for a key or an item the value does not have; `None` for a path it has no such field at.
    fn find(&self, path: &str) -> Option<String> {
        path.is_empty().then(|| self.show())
    }
    fn schema(path: &str, out: &mut Vec<String>);
}

/// A path's first part and the rest after it: `a` of `a.b`, `[3]` of `[3].b`, `<hit>` of `<hit>.damage`.
pub fn split_path(path: &str) -> (&str, &str) {
    let end = if path.starts_with('[') {
        path.find(']').map_or(path.len(), |i| i + 1)
    } else if path.starts_with('<') {
        path.find('>').map_or(path.len(), |i| i + 1)
    } else {
        path.find(['.', '[', '<']).unwrap_or(path.len())
    };
    let (head, rest) = path.split_at(end);
    (head, rest.strip_prefix('.').unwrap_or(rest))
}

/// `x` of `path.x`: a field's path, the parent's with `.name` (or `[i]`) appended, taken off again by `pop`.
pub fn push_path(path: &mut String, part: &str) -> usize {
    let len = path.len();
    if !path.is_empty() && !part.starts_with('[') && !part.starts_with('<') {
        path.push('.');
    }
    path.push_str(part);
    len
}

pub fn show_f64(v: f64) -> String {
    format!("{v} (0x{:016x})", v.to_bits())
}

/// The number `show_f64` wrote, from the bits it wrote in brackets; `None` for any other value's text.
pub fn parse_shown_f64(s: &str) -> Option<f64> {
    let hex = s.strip_suffix(')')?.rsplit_once(" (0x")?.1;
    if hex.len() != 16 {
        return None;
    }
    u64::from_str_radix(hex, 16).ok().map(f64::from_bits)
}

/// A path with its list indexes left out (`world.fighters[].x` of `world.fighters[1].x`): the key a difference is
/// counted and classed under.
pub fn general_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut skip = false;
    for c in path.chars() {
        match c {
            '[' => {
                skip = true;
                out.push('[');
            }
            ']' => {
                skip = false;
                out.push(']');
            }
            _ if skip => {}
            c => out.push(c),
        }
    }
    out
}

fn leaf<T: Field>(a: &T, b: &T, same: bool, path: &str, out: &mut Vec<Difference>) {
    if !same {
        out.push(Difference { path: path.to_string(), expected: a.show(), actual: b.show() });
    }
}

impl Field for f64 {
    fn put(&self, w: &mut Vec<u8>) {
        let bits = if self.is_nan() { NAN_BITS } else { self.to_bits() };
        w.extend_from_slice(&bits.to_le_bytes());
    }
    fn get(r: &mut Cursor) -> Result<Self, TraceError> {
        Ok(f64::from_bits(u64::from_le_bytes(r.take(8)?.try_into().expect("8 bytes"))))
    }
    fn diff(a: &Self, b: &Self, path: &mut String, out: &mut Vec<Difference>) {
        let same = a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan());
        leaf(a, b, same, path, out);
    }
    fn show(&self) -> String {
        show_f64(*self)
    }
    fn schema(path: &str, out: &mut Vec<String>) {
        out.push(format!("{path}:f64"));
    }
}

impl Field for bool {
    fn put(&self, w: &mut Vec<u8>) {
        w.push(u8::from(*self));
    }
    fn get(r: &mut Cursor) -> Result<Self, TraceError> {
        match r.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(TraceError::Bad(r.position() - 1, "bool not 0 or 1")),
        }
    }
    fn diff(a: &Self, b: &Self, path: &mut String, out: &mut Vec<Difference>) {
        leaf(a, b, a == b, path, out);
    }
    fn show(&self) -> String {
        self.to_string()
    }
    fn schema(path: &str, out: &mut Vec<String>) {
        out.push(format!("{path}:bool"));
    }
}

impl Field for String {
    fn put(&self, w: &mut Vec<u8>) {
        put_varint(w, self.len());
        w.extend_from_slice(self.as_bytes());
    }
    fn get(r: &mut Cursor) -> Result<Self, TraceError> {
        let n = r.varint()?;
        let at = r.position();
        String::from_utf8(r.take(n)?.to_vec()).map_err(|_| TraceError::Bad(at, "string not UTF-8"))
    }
    fn diff(a: &Self, b: &Self, path: &mut String, out: &mut Vec<Difference>) {
        leaf(a, b, a == b, path, out);
    }
    fn show(&self) -> String {
        format!("{self:?}")
    }
    fn schema(path: &str, out: &mut Vec<String>) {
        out.push(format!("{path}:str"));
    }
}

impl<T: Field> Field for Option<T> {
    fn put(&self, w: &mut Vec<u8>) {
        match self {
            None => w.push(0),
            Some(v) => {
                w.push(1);
                v.put(w);
            }
        }
    }
    fn get(r: &mut Cursor) -> Result<Self, TraceError> {
        match r.u8()? {
            0 => Ok(None),
            1 => Ok(Some(T::get(r)?)),
            _ => Err(TraceError::Bad(r.position() - 1, "option tag not 0 or 1")),
        }
    }
    fn diff(a: &Self, b: &Self, path: &mut String, out: &mut Vec<Difference>) {
        match (a, b) {
            (Some(x), Some(y)) => T::diff(x, y, path, out),
            (None, None) => {}
            _ => out.push(Difference { path: path.clone(), expected: a.show(), actual: b.show() }),
        }
    }
    fn show(&self) -> String {
        match self {
            None => "none".to_string(),
            Some(v) => v.show(),
        }
    }
    fn show_tree(&self, path: &mut String, out: &mut String) {
        match self {
            None => {
                let _ = writeln!(out, "{path} = none");
            }
            Some(v) => v.show_tree(path, out),
        }
    }
    fn find(&self, path: &str) -> Option<String> {
        match self {
            None => Some("none".to_string()),
            Some(v) => v.find(path),
        }
    }
    fn schema(path: &str, out: &mut Vec<String>) {
        T::schema(&format!("{path}?"), out);
    }
}

impl<T: Field> Field for Vec<T> {
    fn put(&self, w: &mut Vec<u8>) {
        put_varint(w, self.len());
        for v in self {
            v.put(w);
        }
    }
    fn get(r: &mut Cursor) -> Result<Self, TraceError> {
        let n = r.varint()?;
        // (a length past the bytes left is a broken file: never reserve on it)
        let mut v = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            v.push(T::get(r)?);
        }
        Ok(v)
    }
    fn diff(a: &Self, b: &Self, path: &mut String, out: &mut Vec<Difference>) {
        if a.len() != b.len() {
            let at = push_path(path, "len");
            out.push(Difference { path: path.clone(), expected: a.len().to_string(), actual: b.len().to_string() });
            path.truncate(at);
        }
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            let at = push_path(path, &format!("[{i}]"));
            T::diff(x, y, path, out);
            path.truncate(at);
        }
    }
    fn show(&self) -> String {
        let items: Vec<String> = self.iter().map(Field::show).collect();
        format!("[{}]", items.join(", "))
    }
    fn show_tree(&self, path: &mut String, out: &mut String) {
        if self.is_empty() {
            let _ = writeln!(out, "{path} = []");
        }
        for (i, v) in self.iter().enumerate() {
            let at = push_path(path, &format!("[{i}]"));
            v.show_tree(path, out);
            path.truncate(at);
        }
    }
    fn find(&self, path: &str) -> Option<String> {
        if path.is_empty() {
            return Some(self.show());
        }
        if path == "len" {
            return Some(self.len().to_string());
        }
        let (head, rest) = split_path(path);
        let i: usize = head.strip_prefix('[')?.strip_suffix(']')?.parse().ok()?;
        Some(self.get(i).map_or_else(|| "absent".to_string(), |v| v.find(rest).unwrap_or_else(|| v.show())))
    }
    fn schema(path: &str, out: &mut Vec<String>) {
        T::schema(&format!("{path}[]"), out);
    }
}

/// A struct of fields, each `Field`: written, read, compared and shown in the order the fields are declared.
/// `#[derive(Clone, Debug, Default, PartialEq)]` is given; a field's doc comment names what it reads.
#[macro_export]
macro_rules! record {
    ($(#[$m:meta])* pub struct $name:ident { $($(#[$fm:meta])* pub $f:ident : $t:ty,)* }) => {
        $(#[$m])*
        #[derive(Clone, Debug, Default, PartialEq)]
        pub struct $name { $($(#[$fm])* pub $f: $t,)* }

        impl $crate::field::Field for $name {
            fn put(&self, w: &mut Vec<u8>) {
                $($crate::field::Field::put(&self.$f, w);)*
            }
            fn get(r: &mut $crate::field::Cursor) -> Result<Self, $crate::TraceError> {
                Ok(Self { $($f: $crate::field::Field::get(r)?,)* })
            }
            fn diff(a: &Self, b: &Self, path: &mut String, out: &mut Vec<$crate::field::Difference>) {
                $(
                    let at = $crate::field::push_path(path, stringify!($f));
                    $crate::field::Field::diff(&a.$f, &b.$f, path, out);
                    path.truncate(at);
                )*
            }
            fn show(&self) -> String {
                let parts: Vec<String> =
                    vec![$(format!("{}: {}", stringify!($f), $crate::field::Field::show(&self.$f)),)*];
                format!("{{{}}}", parts.join(", "))
            }
            fn show_tree(&self, path: &mut String, out: &mut String) {
                $(
                    let at = $crate::field::push_path(path, stringify!($f));
                    $crate::field::Field::show_tree(&self.$f, path, out);
                    path.truncate(at);
                )*
            }
            fn find(&self, path: &str) -> Option<String> {
                if path.is_empty() {
                    return Some($crate::field::Field::show(self));
                }
                let (head, rest) = $crate::field::split_path(path);
                match head {
                    $(stringify!($f) => $crate::field::Field::find(&self.$f, rest),)*
                    _ => None,
                }
            }
            fn schema(path: &str, out: &mut Vec<String>) {
                $(
                    let mut p = path.to_string();
                    $crate::field::push_path(&mut p, stringify!($f));
                    <$t as $crate::field::Field>::schema(&p, out);
                )*
            }
        }
    };
}
