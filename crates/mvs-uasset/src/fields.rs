//! The reflected properties of a class or struct: its UStruct ChildProperties and its supers', read on first use
//! from the export they belong to (a native class or struct, an import of /Script, has none). A versioned tag names
//! a map's key and value property types only, not their structs; the owner's MapProperty KeyProp / ValueProp does.
//!
//! What it reads, as the struct natives (`natives`) keep them on a class's or struct's object: `data["ChildProperties"]`
//! a `Value::Array` of `Value::Struct`, each field as FProperty's WriteJson writes it (`Type`, `Name`, `Struct` a
//! `Value::Ref`, `Inner` / `KeyProp` / `ValueProp` / `ElementProp` a field struct or null, ...), and
//! `data["SuperStructIndex"]` a `Value::Int` package index of the object's package.

use std::cell::RefCell;
use std::sync::{Arc, OnceLock};

use crate::objects::UObj;
use crate::package::{PackageCache, Resolved};
use crate::value::{Props, Value};

/// Which part of a field names the struct [`StructFields::member`] gives the fields of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// The field itself (a StructProperty).
    Itself,
    /// An ArrayProperty's `Inner`.
    Inner,
    /// A MapProperty's `KeyProp`.
    Key,
    /// A MapProperty's `ValueProp`.
    Value,
}

impl Pick {
    fn of(self, f: &Props) -> Option<&Props> {
        match self {
            Pick::Itself => Some(f),
            Pick::Inner => f.get("Inner").and_then(Value::as_struct),
            Pick::Key => f.get("KeyProp").and_then(Value::as_struct),
            Pick::Value => f.get("ValueProp").and_then(Value::as_struct),
        }
    }
}

/// Where a StructFields reads its fields from.
#[derive(Clone, Debug)]
pub enum FieldsSource {
    /// Nothing known: every lookup finds nothing.
    None,
    /// A class or struct object (an export: read through the cache; anything else has none).
    Object(Resolved),
    /// An object being read (a UserDefinedStruct reading its own default instance): its ChildProperties and its
    /// super struct, taken from its data so far.
    Own { child_properties: Value, super_struct: Option<Resolved> },
    /// The struct a field of another StructFields names, found on first use.
    Member { of: StructFields, name: String, pick: Pick },
}

/// The fields of one class or struct, by name, found once and shared (clones share the lookup).
#[derive(Clone, Debug)]
pub struct StructFields {
    pub source: Arc<FieldsSource>,
    by_name: Arc<OnceLock<Props>>,
}

thread_local! {
    /// The struct exports this thread reads for their fields, by package pointer and index.
    static READING: RefCell<Vec<(usize, usize)>> = const { RefCell::new(Vec::new()) };
}

/// Pops a READING entry, also when the read panics.
struct Reading;

impl Drop for Reading {
    fn drop(&mut self) {
        READING.with(|r| r.borrow_mut().pop());
    }
}

/// A struct export's object, read; `None` for an import (a native struct) or one this thread already reads for its
/// fields.
// The guard is a set of its own (the struct reads for fields), not
// `PackageCache::is_reading` (every export on the stack): an object read as itself whose fields are asked for inside
// that read is read again nested.
fn struct_object(cache: &PackageCache, r: &Resolved) -> Option<Arc<UObj>> {
    let (pkg, i) = r.as_export()?;
    let key = (Arc::as_ptr(pkg) as usize, i);
    if READING.with(|s| s.borrow().contains(&key)) {
        return None;
    }
    READING.with(|s| s.borrow_mut().push(key));
    let _guard = Reading;
    Some(cache.object(pkg, i))
}

/// The struct a StructProperty field names, resolved.
pub fn struct_of(f: Option<&Props>) -> Option<Resolved> {
    let f = f?;
    if f.get("Type").and_then(Value::as_str) != Some("StructProperty") {
        return None;
    }
    f.get("Struct").and_then(Value::as_ref).cloned()
}

impl StructFields {
    pub fn new(source: FieldsSource) -> StructFields {
        StructFields { source: Arc::new(source), by_name: Arc::new(OnceLock::new()) }
    }

    pub fn of(at: Option<Resolved>) -> StructFields {
        StructFields::new(at.map_or(FieldsSource::None, FieldsSource::Object))
    }

    /// The field named `name` (its FProperty struct), up the super chain; the nearest wins. A struct whose read
    /// reaches itself again (a struct's default instance naming a struct that names the first) finds no fields the
    /// second time.
    pub fn field(&self, cache: &PackageCache, name: &str) -> Option<&Props> {
        self.by_name.get_or_init(|| self.collect(cache)).get(name).and_then(Value::as_struct)
    }

    fn collect(&self, cache: &PackageCache) -> Props {
        let mut m = Props::new();
        let add = |m: &mut Props, cp: Option<&Value>| {
            if let Some(Value::Array(list)) = cp {
                for p in list {
                    if let Some(name) = p.as_struct().and_then(|s| s.get("Name")).and_then(Value::as_str)
                        && !m.contains_key(name)
                    {
                        m.insert(name.to_owned(), p.clone());
                    }
                }
            }
        };
        let mut obj = match &*self.source {
            FieldsSource::None => None,
            FieldsSource::Object(r) => struct_object(cache, r),
            FieldsSource::Own { child_properties, super_struct } => {
                add(&mut m, Some(child_properties));
                super_struct.as_ref().and_then(|r| struct_object(cache, r))
            }
            FieldsSource::Member { of, name, pick } => {
                let r = struct_of(of.field(cache, name).and_then(|f| pick.of(f)));
                r.and_then(|r| struct_object(cache, &r))
            }
        };
        let start = usize::from(matches!(&*self.source, FieldsSource::Own { .. }));
        for _ in start..64 {
            let Some(o) = obj else { break };
            add(&mut m, o.data.get("ChildProperties"));
            let sup = o.data.get("SuperStructIndex").and_then(Value::as_i64).unwrap_or(0);
            let r = if sup == 0 {
                None
            } else {
                i32::try_from(sup).ok().and_then(|i| cache.try_resolve(&o.pkg, i).ok().flatten())
            };
            obj = r.and_then(|r| struct_object(cache, &r));
        }
        m
    }

    /// The fields of the struct a property of this one names: the StructProperty `pick` finds from the field
    /// (the field itself, its `Inner`, its `KeyProp` ...), looked up on first use.
    pub fn member(&self, name: &str, pick: Pick) -> StructFields {
        StructFields::new(FieldsSource::Member { of: self.clone(), name: name.to_owned(), pick })
    }
}
