//! A property value as the object kind holds it, read straight from the package: an
//! earlier reader read cue4dump's JSON values first and turned them into these; this reader skips that step.
//!
//! How each is written (the object kind's JSON writer, `mvs_import::kinds::object`):
//! - `Float` as the shortest decimal that reads back as the same f32 (an earlier kind wrote .NET's shortest
//!   round-trip text read back as a double: the same number for every value but a few decimal ties), `Double` as the
//!   shortest f64; a NaN or an infinity as the string "NaN", "Infinity", "-Infinity";
//! - `Int` / `UInt` as JSON numbers (a 64-bit value past 2^53 loses digits, as an earlier kind's did);
//! - `Str`: names, strings, enums as the stored name (`EFighterType::Vertical`);
//! - `Struct` as an object, its members by declared name (a UserDefinedStruct member's `_<n>_<GUID>` suffix dropped
//!   at write, two members that collide an error); `Array` and a set as an array; `Map` as `[key, value]` pairs in
//!   entry order;
//! - `Ref` as the object path it resolves to (`/Game/A/B.B`, `/Script/Engine.StaticMesh`), null for none;
//! - `SoftPath` as `Asset:Sub`, `Asset`, or null for None / empty;
//! - `Text` as `{ source, namespace, key }`, `{ source: null, table, key }` or `{ source }`;
//! - `Bytes` only in a native class's extra fields: a section of the entry's body, written as its `{ offset, length }`.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::package::Resolved;

/// Named values in stored order: an export's tagged properties (`Name`, `Name[i]` for element i > 0 of a fixed
/// array), a struct's members, a native class's fields.
pub type Props = IndexMap<String, Value>;

#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    /// Every signed integer property, and the unsigned ones up to 32 bits.
    Int(i64),
    UInt(u64),
    Float(f32),
    Double(f64),
    Str(String),
    Array(Vec<Value>),
    Struct(Props),
    Map(Vec<(Value, Value)>),
    Ref(Option<Resolved>),
    SoftPath {
        asset: String,
        sub: String,
    },
    Text(TextValue),
    Bytes(Arc<[u8]>),
}

/// A text property: its source string and where the localized one is looked up (the player's locres).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextValue {
    /// A Base history: the source string, the namespace and key.
    Base { source: String, namespace: String, key: String },
    /// A StringTableEntry history: the table's id and the entry's key; `source` is the table's string where the
    /// table resolved (it never does in this build: null).
    Table { source: Option<String>, table: String, key: String },
    /// A None history: the culture-invariant string, or none.
    Invariant { source: Option<String> },
}

/// What one heap allocation costs beside its bytes (the allocator's header and rounding), in the cache's budget.
pub const ALLOC_OVERHEAD: usize = 16;

/// The heap bytes of one allocation of `capacity` bytes (none for an empty string or vector).
pub fn alloc_bytes(capacity: usize) -> usize {
    if capacity == 0 { 0 } else { capacity + ALLOC_OVERHEAD }
}

/// The heap bytes of named values: the map's entries and index, the names and the values' own.
pub fn props_heap_bytes(p: &Props) -> usize {
    if p.capacity() == 0 {
        return 0;
    }
    // an entry is its hash, name and value; the index a usize a slot (with its control byte)
    let table = p.capacity() * (size_of::<u64>() + size_of::<String>() + size_of::<Value>() + size_of::<usize>() + 1);
    let own: usize = p.iter().map(|(k, v)| alloc_bytes(k.capacity()) + v.heap_bytes()).sum();
    table + 2 * ALLOC_OVERHEAD + own
}

impl Value {
    /// The heap bytes it holds of its own (an object it refers to is not its own; a shared byte section counts in
    /// full): what the package cache counts against its budget.
    pub fn heap_bytes(&self) -> usize {
        let text = |s: &Option<String>| s.as_ref().map_or(0, |s| alloc_bytes(s.capacity()));
        match self {
            Value::Null | Value::Bool(_) | Value::Int(_) | Value::UInt(_) | Value::Float(_) | Value::Double(_) => 0,
            Value::Str(s) => alloc_bytes(s.capacity()),
            Value::Array(a) => {
                let own: usize = a.iter().map(Value::heap_bytes).sum();
                alloc_bytes(a.capacity() * size_of::<Value>()) + own
            }
            Value::Struct(p) => props_heap_bytes(p),
            Value::Map(m) => {
                let own: usize = m.iter().map(|(k, v)| k.heap_bytes() + v.heap_bytes()).sum();
                alloc_bytes(m.capacity() * 2 * size_of::<Value>()) + own
            }
            Value::Ref(Some(Resolved::Script(n))) => alloc_bytes(n.capacity()),
            Value::Ref(_) => 0,
            Value::SoftPath { asset, sub } => alloc_bytes(asset.capacity()) + alloc_bytes(sub.capacity()),
            Value::Text(TextValue::Base { source, namespace, key }) => {
                [source, namespace, key].into_iter().map(|s| alloc_bytes(s.capacity())).sum()
            }
            Value::Text(TextValue::Table { source, table, key }) => {
                text(source) + alloc_bytes(table.capacity()) + alloc_bytes(key.capacity())
            }
            Value::Text(TextValue::Invariant { source }) => text(source),
            Value::Bytes(b) => b.len() + 2 * size_of::<usize>() + ALLOC_OVERHEAD,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_struct(&self) -> Option<&Props> {
        match self {
            Value::Struct(p) => Some(p),
            _ => None,
        }
    }
    pub fn as_ref(&self) -> Option<&Resolved> {
        match self {
            Value::Ref(r) => r.as_ref(),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Value::Int(v) => Some(v),
            Value::UInt(v) => i64::try_from(v).ok(),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }
}
