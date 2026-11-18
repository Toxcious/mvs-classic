//! A plain value (`Value`): an event's fields and a record's `rest`, a small tree as the oracle sim's plain objects
//! hold them. An object's keys are sorted (byte order) and each is there once (`Obj` holds them so and the reader
//! refuses a file whose keys are not), so a writer need not keep the oracle object's key order; an absent or
//! `undefined` key is not written.

use std::fmt::Write as _;

use crate::TraceError;
use crate::field::{Cursor, Difference, Field, push_path, put_varint, show_f64, split_path};

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<Value>),
    Obj(Obj),
}

/// An object's entries, sorted by key, each key once: only `Obj::new`, `insert` and the reader make one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Obj(Vec<(String, Value)>);

impl Obj {
    /// From entries in any order: sorted by key; of a key given twice the last value is kept (an oracle assignment).
    pub fn new(entries: Vec<(String, Value)>) -> Self {
        let mut o = Obj(Vec::with_capacity(entries.len()));
        for (k, v) in entries {
            o.insert(k, v);
        }
        o
    }

    pub fn insert(&mut self, key: String, value: Value) {
        match self.0.binary_search_by(|e| e.0.as_str().cmp(&key)) {
            Ok(i) => self.0[i].1 = value,
            Err(i) => self.0.insert(i, (key, value)),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.binary_search_by(|e| e.0.as_str().cmp(key)).ok().map(|i| &self.0[i].1)
    }

    /// The entries in key order.
    pub fn entries(&self) -> &[(String, Value)] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn put(&self, w: &mut Vec<u8>) {
        put_varint(w, self.0.len());
        for (k, v) in &self.0 {
            k.put(w);
            v.put(w);
        }
    }

    fn get_from(r: &mut Cursor) -> Result<Self, TraceError> {
        let n = r.varint()?;
        let mut e: Vec<(String, Value)> = Vec::with_capacity(n.min(64));
        for _ in 0..n {
            let at = r.position();
            let k = String::get(r)?;
            if e.last().is_some_and(|(last, _)| *last >= k) {
                return Err(TraceError::Bad(at, "object keys not sorted or given twice"));
            }
            e.push((k, Value::get(r)?));
        }
        Ok(Obj(e))
    }

    fn show(&self) -> String {
        let parts: Vec<String> = self.0.iter().map(|(k, v)| format!("{k}: {}", v.show())).collect();
        format!("{{{}}}", parts.join(", "))
    }

    fn show_tree(&self, path: &mut String, out: &mut String) {
        if self.0.is_empty() {
            let _ = writeln!(out, "{path} = {{}}");
        }
        for (k, v) in &self.0 {
            let at = push_path(path, k);
            v.show_tree(path, out);
            path.truncate(at);
        }
    }

    /// Two objects by key: a key on one side only is a difference (`absent` on the other).
    fn diff(a: &Obj, b: &Obj, path: &mut String, out: &mut Vec<Difference>) {
        let (a, b) = (&a.0, &b.0);
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            let ka = a.get(i).map(|e| e.0.as_str());
            let kb = b.get(j).map(|e| e.0.as_str());
            match (ka, kb) {
                (Some(x), Some(y)) if x == y => {
                    let at = push_path(path, x);
                    Value::diff(&a[i].1, &b[j].1, path, out);
                    path.truncate(at);
                    i += 1;
                    j += 1;
                }
                (Some(x), y) if y.is_none_or(|y| x < y) => {
                    let at = push_path(path, x);
                    out.push(Difference { path: path.clone(), expected: a[i].1.show(), actual: "absent".to_string() });
                    path.truncate(at);
                    i += 1;
                }
                (_, Some(y)) => {
                    let at = push_path(path, y);
                    out.push(Difference { path: path.clone(), expected: "absent".to_string(), actual: b[j].1.show() });
                    path.truncate(at);
                    j += 1;
                }
                _ => unreachable!("one side has a key"),
            }
        }
    }

    fn find(&self, path: &str) -> Option<String> {
        if path.is_empty() {
            return Some(self.show());
        }
        let (key, rest) = split_path(path);
        Some(self.get(key).map_or_else(|| "absent".to_string(), |v| v.find(rest).unwrap_or_else(|| v.show())))
    }
}

impl Value {
    /// An object from its entries in any order (`Obj::new`).
    pub fn obj(entries: Vec<(String, Value)>) -> Self {
        Value::Obj(Obj::new(entries))
    }

    /// The value of `key` when this is an object that has it.
    pub fn key(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(o) => o.get(key),
            _ => None,
        }
    }
}

impl Field for Value {
    fn put(&self, w: &mut Vec<u8>) {
        match self {
            Value::Null => w.push(0),
            Value::Bool(b) => w.push(if *b { 2 } else { 1 }),
            Value::Num(v) => {
                w.push(3);
                v.put(w);
            }
            Value::Str(s) => {
                w.push(4);
                s.put(w);
            }
            Value::List(l) => {
                w.push(5);
                l.put(w);
            }
            Value::Obj(o) => {
                w.push(6);
                o.put(w);
            }
        }
    }
    fn get(r: &mut Cursor) -> Result<Self, TraceError> {
        Ok(match r.u8()? {
            0 => Value::Null,
            1 => Value::Bool(false),
            2 => Value::Bool(true),
            3 => Value::Num(f64::get(r)?),
            4 => Value::Str(String::get(r)?),
            5 => Value::List(Vec::get(r)?),
            6 => Value::Obj(Obj::get_from(r)?),
            _ => return Err(TraceError::Bad(r.position() - 1, "value tag")),
        })
    }
    fn diff(a: &Self, b: &Self, path: &mut String, out: &mut Vec<Difference>) {
        match (a, b) {
            (Value::Num(x), Value::Num(y)) => f64::diff(x, y, path, out),
            (Value::List(x), Value::List(y)) => Vec::diff(x, y, path, out),
            (Value::Obj(x), Value::Obj(y)) => Obj::diff(x, y, path, out),
            _ if a == b => {}
            _ => out.push(Difference { path: path.clone(), expected: a.show(), actual: b.show() }),
        }
    }
    fn show(&self) -> String {
        match self {
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Num(v) => show_f64(*v),
            Value::Str(s) => format!("{s:?}"),
            Value::List(l) => l.show(),
            Value::Obj(o) => o.show(),
        }
    }
    fn show_tree(&self, path: &mut String, out: &mut String) {
        match self {
            Value::Obj(o) => o.show_tree(path, out),
            Value::List(l) => l.show_tree(path, out),
            _ => {
                let _ = writeln!(out, "{path} = {}", self.show());
            }
        }
    }
    fn find(&self, path: &str) -> Option<String> {
        match self {
            Value::Obj(o) => o.find(path),
            Value::List(l) => l.find(path),
            _ if path.is_empty() => Some(self.show()),
            _ => None,
        }
    }
    fn schema(path: &str, out: &mut Vec<String>) {
        out.push(format!("{path}:value"));
    }
}

/// An event of the tick: the oracle event's `type` and its other keys.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EventRecord {
    /// `SimEvent.type`
    pub kind: String,
    /// The event's other keys; an object a key holds is written by value, a notify by its identity
    /// (the oracle's `plain`, which refuses a sim object).
    pub fields: Obj,
}

impl Field for EventRecord {
    fn put(&self, w: &mut Vec<u8>) {
        self.kind.put(w);
        w.push(6);
        self.fields.put(w);
    }
    fn get(r: &mut Cursor) -> Result<Self, TraceError> {
        let kind = String::get(r)?;
        let at = r.position();
        match Value::get(r)? {
            Value::Obj(fields) => Ok(Self { kind, fields }),
            _ => Err(TraceError::Bad(at, "event fields not an object")),
        }
    }
    fn diff(a: &Self, b: &Self, path: &mut String, out: &mut Vec<Difference>) {
        if a.kind != b.kind {
            let at = push_path(path, "type");
            out.push(Difference { path: path.clone(), expected: a.show(), actual: b.show() });
            path.truncate(at);
            return;
        }
        let at = push_path(path, &format!("<{}>", a.kind));
        Obj::diff(&a.fields, &b.fields, path, out);
        path.truncate(at);
    }
    fn show(&self) -> String {
        format!("{} {}", self.kind, self.fields.show())
    }
    fn show_tree(&self, path: &mut String, out: &mut String) {
        let at = push_path(path, &format!("<{}>", self.kind));
        self.fields.show_tree(path, out);
        path.truncate(at);
    }
    /// `type`, or `<kind>.key` of an event of that kind (another kind: the event as `show` writes it).
    fn find(&self, path: &str) -> Option<String> {
        if path.is_empty() || path == "type" {
            return Some(self.show());
        }
        let (head, rest) = split_path(path);
        if head.strip_prefix('<').and_then(|h| h.strip_suffix('>')) == Some(self.kind.as_str()) {
            self.fields.find(rest)
        } else {
            Some(self.show())
        }
    }
    fn schema(path: &str, out: &mut Vec<String>) {
        out.push(format!("{path}:event"));
    }
}
