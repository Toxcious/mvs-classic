//! Tagged properties (versioned UE 4.26 FPropertyTag) read into the object kind's values. It follows
//! CUE4Parse 1.2.2's FPropertyTagType family, reading values straight into
//! [`Value`] rather than cue4dump's JSON: a map's keys are values, not their text.

use crate::archive::{ReadError, Result};
use crate::fields::{Pick, StructFields, struct_of};
use crate::objects::{ExportArchive, IssueKind};
use crate::structs::read_native_struct;
use crate::value::{Props, TextValue, Value};
use crate::versions;

/// How a value is read: CUE4Parse's ReadType.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadType {
    Normal,
    Map,
    Array,
    Raw,
}

/// A tag's type data (FPropertyTag), with the reflected fields of the class or struct that owns it where known.
#[derive(Clone, Debug, Default)]
pub struct TagData {
    pub name: String,
    pub type_name: String,
    pub struct_type: Option<String>,
    pub bool_value: Option<bool>,
    pub enum_name: Option<String>,
    pub inner_type: Option<String>,
    pub value_type: Option<String>,
    pub inner_type_data: Option<Box<TagData>>,
    pub value_type_data: Option<Box<TagData>>,
    /// The class or struct whose tagged property this is.
    pub owner: Option<StructFields>,
    /// The struct a StructProperty value is, where its reflection is known.
    pub struct_fields: Option<StructFields>,
}

impl TagData {
    /// The data of a struct of a known type (a map's or set's key or value CUE4Parse names by property).
    fn of_struct(struct_type: &str) -> TagData {
        TagData { type_name: "StructProperty".into(), struct_type: Some(struct_type.to_owned()), ..TagData::default() }
    }
}

/// One tag header.
struct Tag {
    name: String,
    size: i32,
    array_index: i32,
    data: TagData,
}

/// UE4 version PROPERTY_GUID_IN_PROPERTY_TAG.
const VER_PROPERTY_GUID_IN_PROPERTY_TAG: i32 = 503;
/// UE4 version STRUCT_GUID_IN_PROPERTY_TAG.
const VER_STRUCT_GUID_IN_PROPERTY_TAG: i32 = 441;
/// UE4 version INNER_ARRAY_TAG_INFO.
const VER_INNER_ARRAY_TAG_INFO: i32 = 500;

/// FPropertyTag: `None` at the "None" name that ends the list.
fn read_tag_header(ar: &mut ExportArchive<'_>) -> Result<Option<Tag>> {
    let name = ar.fname()?;
    if name == "None" {
        return Ok(None);
    }
    let type_name = ar.fname()?;
    let size = ar.i32()?;
    let array_index = ar.i32()?;
    let mut data = TagData { name: name.clone(), type_name: type_name.clone(), ..TagData::default() };
    match type_name.as_str() {
        "BoolProperty" => data.bool_value = Some(ar.flag()?),
        "ByteProperty" | "EnumProperty" => data.enum_name = Some(ar.fname()?),
        "SetProperty" | "ArrayProperty" => data.inner_type = Some(ar.fname()?),
        "MapProperty" => {
            data.inner_type = Some(ar.fname()?);
            data.value_type = Some(ar.fname()?);
        }
        "StructProperty" => {
            data.struct_type = Some(ar.fname()?);
            if ar.ver() >= VER_STRUCT_GUID_IN_PROPERTY_TAG {
                ar.skip(16)?;
            }
        }
        _ => {}
    }
    if ar.ver() >= VER_PROPERTY_GUID_IN_PROPERTY_TAG && ar.flag()? {
        ar.skip(16)?;
    }
    Ok(Some(Tag { name, size, array_index, data }))
}

/// Map key and value structs CUE4Parse knows by property name (VersionContainer.MapStructTypes).
fn map_struct_types(name: &str) -> Option<(Option<&'static str>, Option<&'static str>)> {
    Some(match name {
        "BindingIdToReferences" => (Some("Guid"), None),
        "UserParameterRedirects" => (Some("NiagaraVariable"), Some("NiagaraVariable")),
        "Tracks" => (Some("MovieSceneTrackIdentifier"), None),
        "SubSequences" => (Some("MovieSceneSequenceID"), None),
        "Hierarchy" => (Some("MovieSceneSequenceID"), None),
        "TrackSignatureToTrackIdentifier" => (Some("Guid"), Some("MovieSceneTrackIdentifier")),
        _ => return None,
    })
}

/// DeserializePropertiesTagged: tags until "None", each value read within its size, into `out` as `Name` or
/// `Name[i]`. A value that fails to parse is null (CUE4Parse catches its ParserException and seeks past the tag); a
/// reader the port lacks is reported on the archive (`IssueKind::NotPorted`), then null. `owner`: the reflected
/// fields of the class or struct the properties belong to.
pub fn read_tagged_properties(ar: &mut ExportArchive<'_>, out: &mut Props, owner: Option<&StructFields>) -> Result<()> {
    while let Some(mut tag) = read_tag_header(ar)? {
        if let Some(o) = owner {
            tag.data.owner = Some(o.clone());
        }
        // a negative size reads as `pos + size` (as a JavaScript reader's does): a position before the value
        let end = ar.pos.checked_add_signed(tag.size as isize).unwrap_or(0);
        let size = usize::try_from(tag.size).unwrap_or(0);
        let value = match read_value(ar, &tag.data.type_name.clone(), Some(&tag.data), ReadType::Normal, size) {
            Ok(v) => v,
            Err(ReadError::NotPorted(m)) => {
                ar.report(IssueKind::NotPorted, format!("{}: {m}", tag.name));
                Value::Null
            }
            Err(ReadError::Parse(_)) => Value::Null,
            Err(e) => return Err(e),
        };
        ar.pos = end;
        let key = if tag.array_index > 0 { format!("{}[{}]", tag.name, tag.array_index) } else { tag.name };
        out.insert(key, value);
    }
    Ok(())
}

/// FStructFallback: tagged properties as a struct.
pub fn read_struct_fallback(ar: &mut ExportArchive<'_>, owner: Option<&StructFields>) -> Result<Props> {
    let mut o = Props::new();
    read_tagged_properties(ar, &mut o, owner)?;
    Ok(o)
}

/// One value of a property type.
pub fn read_value(
    ar: &mut ExportArchive<'_>,
    type_name: &str,
    data: Option<&TagData>,
    rt: ReadType,
    size: usize,
) -> Result<Value> {
    Ok(match type_name {
        "ArrayProperty" => Value::Array(read_array(ar, data, rt, size)?),
        "BoolProperty" => Value::Bool(match rt {
            ReadType::Normal => data.and_then(|d| d.bool_value) == Some(true),
            ReadType::Raw => ar.bool()?,
            _ => ar.flag()?,
        }),
        "ByteProperty" => {
            let named = data.and_then(|d| d.enum_name.as_deref()).is_some_and(|n| !n.eq_ignore_ascii_case("none"));
            if named || (rt == ReadType::Map && test_read_fname(ar)) {
                read_enum(ar, data, rt)?
            } else {
                Value::Int(i64::from(ar.u8()?))
            }
        }
        "EnumProperty" => read_enum(ar, data, rt)?,
        "ClassProperty" | "ObjectProperty" | "WeakObjectProperty" | "InterfaceProperty" => Value::Ref(ar.index()?),
        "LazyObjectProperty" => Value::Str(ar.guid()?.to_string()),
        "DelegateProperty" => Value::Struct(delegate(ar)?),
        "MulticastDelegateProperty" | "MulticastInlineDelegateProperty" | "MulticastSparseDelegateProperty" => {
            let n = ar.i32()?;
            let mut list = Vec::new();
            for _ in 0..n {
                list.push(Value::Struct(delegate(ar)?));
            }
            Value::Struct(Props::from([("InvocationList".to_owned(), Value::Array(list))]))
        }
        "DoubleProperty" => Value::Double(ar.f64()?),
        "FloatProperty" => Value::Float(ar.f32()?),
        "Int8Property" => Value::Int(i64::from(ar.i8()?)),
        "Int16Property" => Value::Int(i64::from(ar.i16()?)),
        "IntProperty" => Value::Int(i64::from(ar.i32()?)),
        "Int64Property" => Value::Int(ar.i64()?),
        "UInt16Property" => Value::Int(i64::from(ar.u16()?)),
        "UInt32Property" => Value::Int(i64::from(ar.u32()?)),
        "UInt64Property" => Value::UInt(ar.u64()?),
        "NameProperty" => Value::Str(ar.fname()?),
        "StrProperty" => Value::Str(ar.fstring()?),
        "TextProperty" => read_text(ar)?,
        "SoftObjectProperty" | "SoftClassProperty" => read_soft_object_path(ar)?,
        "FieldPathProperty" => read_field_path(ar)?,
        "SetProperty" => Value::Array(read_set(ar, data, rt)?),
        "MapProperty" => Value::Map(read_map(ar, data, rt)?),
        "StructProperty" => {
            let fields = match data {
                Some(d) => d.struct_fields.clone().or_else(|| match &d.owner {
                    Some(o) if !d.name.is_empty() => Some(o.member(&d.name, Pick::Itself)),
                    _ => None,
                }),
                None => None,
            };
            read_struct(ar, data.and_then(|d| d.struct_type.as_deref()), rt, fields.as_ref())?
        }
        _ => return Err(ReadError::parse(format!("unknown property type {type_name}"))),
    })
}

/// FScriptDelegate: `{ Object, FunctionName }`.
fn delegate(ar: &mut ExportArchive<'_>) -> Result<Props> {
    let object = Value::Ref(ar.index()?);
    let function = Value::Str(ar.fname()?);
    Ok(Props::from([("Object".to_owned(), object), ("FunctionName".to_owned(), function)]))
}

/// FAssetArchive.TestReadFName: whether the next 8 bytes look like an FName of this package.
fn test_read_fname(ar: &ExportArchive<'_>) -> bool {
    if ar.pos + 8 >= ar.len() {
        return false;
    }
    let b = &ar.buf[ar.pos..ar.pos + 8];
    let i = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let n = i32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    usize::try_from(i).is_ok_and(|i| i < ar.pkg.names.len()) && (0..256).contains(&n)
}

fn read_enum(ar: &mut ExportArchive<'_>, data: Option<&TagData>, rt: ReadType) -> Result<Value> {
    if rt == ReadType::Raw {
        let idx = ar.u8()?;
        return Ok(Value::Str(match data.and_then(|d| d.enum_name.as_deref()).filter(|n| !n.is_empty()) {
            Some(e) => format!("{e}::{idx}"),
            None => idx.to_string(),
        }));
    }
    Ok(Value::Str(ar.fname()?))
}

fn read_array(ar: &mut ExportArchive<'_>, data: Option<&TagData>, rt: ReadType, size: usize) -> Result<Vec<Value>> {
    let Some(inner) = data.and_then(|d| d.inner_type.as_deref()) else {
        return Err(ReadError::parse("UScriptArray needs inner type"));
    };
    let data = data.expect("an inner type has its data");
    let n = ar.i32()?;
    if i64::from(n) > ar.len() as i64 - ar.pos as i64 {
        return Err(ReadError::parse(format!(
            "ArrayProperty element count {n} is larger than the remaining archive size"
        )));
    }
    let own: Option<TagData>;
    let inner_data: Option<&TagData> = if rt == ReadType::Raw {
        data.inner_type_data.as_deref()
    } else if inner == "StructProperty" && ar.ver() >= VER_INNER_ARRAY_TAG_INFO {
        let Some(t) = read_tag_header(ar)? else {
            return Err(ReadError::parse(format!("Couldn't read ArrayProperty with inner type {inner}")));
        };
        let mut d = t.data;
        if let Some(o) = &data.owner {
            d.struct_fields = Some(o.member(&data.name, Pick::Inner));
        }
        own = Some(d);
        own.as_ref()
    } else {
        None
    };
    let mut out = Vec::new();
    if n <= 0 {
        return Ok(out);
    }
    let rt2 = if rt == ReadType::Raw { ReadType::Raw } else { ReadType::Array };
    if inner == "ByteProperty" {
        let is_enum = (size as f64 - 4.0) / f64::from(n) > 1.0;
        for _ in 0..n {
            out.push(if is_enum { read_enum(ar, inner_data, rt2)? } else { Value::Int(i64::from(ar.u8()?)) });
        }
        return Ok(out);
    }
    for _ in 0..n {
        out.push(read_value(ar, inner, inner_data, rt2, 0)?);
    }
    Ok(out)
}

fn read_set(ar: &mut ExportArchive<'_>, data: Option<&TagData>, rt: ReadType) -> Result<Vec<Value>> {
    let Some(inner) = data.and_then(|d| d.inner_type.as_deref()) else {
        return Err(ReadError::parse("UScriptSet needs inner type"));
    };
    let data = data.expect("an inner type has its data");
    let guid;
    let mut inner_data = data.inner_type_data.as_deref();
    if inner_data.is_none()
        && inner == "StructProperty"
        && (data.name == "AnimSequenceInstances" || data.name == "PostProcessInstances")
    {
        guid = TagData::of_struct("Guid");
        inner_data = Some(&guid);
    }
    if rt != ReadType::Raw {
        let removed = ar.i32()?;
        for _ in 0..removed {
            read_value(ar, inner, inner_data, ReadType::Array, 0)?;
        }
    }
    let n = ar.i32()?;
    let rt2 = if rt == ReadType::Raw { ReadType::Raw } else { ReadType::Array };
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(read_value(ar, inner, inner_data, rt2, 0)?);
    }
    Ok(out)
}

/// A map key's or value's tag data from its reflected property: a StructProperty's struct name and fields.
fn struct_data(f: Option<&Props>) -> Option<TagData> {
    let r = struct_of(f)?;
    let mut d = TagData::of_struct(r.name());
    d.struct_fields = Some(StructFields::of(Some(r)));
    Some(d)
}

fn read_map(ar: &mut ExportArchive<'_>, data: Option<&TagData>, rt: ReadType) -> Result<Vec<(Value, Value)>> {
    let (Some(data), Some(key_type), Some(value_type)) =
        (data, data.and_then(|d| d.inner_type.as_deref()), data.and_then(|d| d.value_type.as_deref()))
    else {
        return Err(ReadError::parse("Can't serialize UScriptMap without key or value type"));
    };
    let mut kd = data.inner_type_data.as_deref().cloned();
    let mut vd = data.value_type_data.as_deref().cloned();
    if let Some((k, v)) = map_struct_types(&data.name) {
        if let Some(k) = k {
            kd = Some(TagData::of_struct(k));
        }
        if let Some(v) = v {
            vd = Some(TagData::of_struct(v));
        }
    }
    // Past CUE4Parse's table: the struct the owner's MapProperty names for its key or value (KeyProp / ValueProp).
    // CUE4Parse reads such a struct as a tagged fallback, fails and writes the map null (cue4dump's JSON differs here).
    let key_struct = kd.is_none() && key_type == "StructProperty";
    let value_struct = vd.is_none() && value_type == "StructProperty";
    if key_struct || value_struct {
        let f = data.owner.as_ref().and_then(|o| o.field(ar.cache, &data.name));
        if key_struct {
            kd = struct_data(f.and_then(|f| f.get("KeyProp")).and_then(Value::as_struct));
        }
        if value_struct {
            vd = struct_data(f.and_then(|f| f.get("ValueProp")).and_then(Value::as_struct));
        }
    }
    if rt != ReadType::Raw {
        let removed = ar.i32()?;
        for _ in 0..removed {
            read_value(ar, key_type, kd.as_ref(), ReadType::Map, 0)?;
        }
    }
    let n = ar.i32()?;
    let rt2 = if rt == ReadType::Raw { ReadType::Raw } else { ReadType::Map };
    let mut out = Vec::new();
    for i in 0..n {
        let key = read_value(ar, key_type, kd.as_ref(), rt2, 0)
            .map_err(|e| e.context(&format!("Failed to read key for index {i} in map: ")))?;
        let value = read_value(ar, value_type, vd.as_ref(), rt2, 0)
            .map_err(|e| e.context(&format!("Failed to read value for index {i} in map: ")))?;
        out.push((key, value));
    }
    Ok(out)
}

/// FSoftObjectPath: an asset path name and a sub path (`Value::SoftPath`).
pub fn read_soft_object_path(ar: &mut ExportArchive<'_>) -> Result<Value> {
    let asset = ar.fname()?;
    let sub = ar.fstring()?;
    Ok(Value::SoftPath { asset, sub })
}

/// FFieldPath: `{ Path }`, and `ResolvedOwner` from FFieldPathOwnerSerialization on.
fn read_field_path(ar: &mut ExportArchive<'_>) -> Result<Value> {
    let n = ar.i32()?;
    let mut path = Vec::new();
    for _ in 0..n {
        path.push(Value::Str(ar.fname()?));
    }
    if path.len() == 1 && path[0].as_str() == Some("None") {
        path.clear();
    }
    let mut o = Props::from([("Path".to_owned(), Value::Array(path))]);
    // FFieldPathOwnerSerialization: FFortniteMainBranchObjectVersion >= 33 or FReleaseObjectVersion >= 30
    if ar.custom(versions::FORTNITE_MAIN) >= 33 || ar.custom(versions::RELEASE) >= 30 {
        o.insert("ResolvedOwner".into(), Value::Ref(ar.index()?));
    }
    Ok(Value::Struct(o))
}

/// FEditorObjectVersion CultureInvariantTextSerializationKeyStability.
const CULTURE_INVARIANT_TEXT_SERIALIZATION_KEY_STABILITY: i32 = 32;

/// FText as its TextHistory writes it (`Value::Text`); a history the port lacks is `ReadError::NotPorted`.
pub fn read_text(ar: &mut ExportArchive<'_>) -> Result<Value> {
    ar.u32()?; // flags
    let history = ar.i8()?;
    Ok(Value::Text(match history {
        -1 => {
            let has = ar.custom(versions::EDITOR) >= CULTURE_INVARIANT_TEXT_SERIALIZATION_KEY_STABILITY && ar.bool()?;
            TextValue::Invariant { source: if has { Some(ar.fstring()?) } else { None } }
        }
        0 => {
            let namespace = ar.fstring()?;
            let key = ar.fstring()?;
            let source = ar.fstring()?;
            TextValue::Base { source, namespace, key }
        }
        11 => {
            let table = ar.fname()?;
            let key = ar.fstring()?;
            let source = ar.cache.string_table(&table).and_then(|t| t.get(&key)).cloned();
            TextValue::Table { source, table, key }
        }
        h => return Err(ReadError::not_ported(format!("FText history {h} not ported"))),
    }))
}

/// A struct value: a native struct's, else its tagged properties (FStructFallback) with `fields` as their owner.
pub fn read_struct(
    ar: &mut ExportArchive<'_>,
    struct_type: Option<&str>,
    rt: ReadType,
    fields: Option<&StructFields>,
) -> Result<Value> {
    if let Some(t) = struct_type
        && let Some(v) = read_native_struct(ar, t, rt)?
    {
        return Ok(v);
    }
    Ok(Value::Struct(read_struct_fallback(ar, fields)?))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mvs_pak::BuildPaths;

    use super::*;
    use crate::package::{FileSource, Package, PackageCache};

    struct NoFiles;
    impl FileSource for NoFiles {
        fn read_file(&self, _: &str) -> Result<Option<Vec<u8>>> {
            Ok(None)
        }
    }

    const NAMES: &[&str] = &[
        "None",
        "A",
        "BoolProperty",
        "IntProperty",
        "TextProperty",
        "MapProperty",
        "StrProperty",
        "SetProperty",
        "NameProperty",
        "ArrayProperty",
        "StructProperty",
        "Vector",
        "Table",
    ];

    fn name_index(n: &str) -> i32 {
        i32::try_from(NAMES.iter().position(|x| *x == n).unwrap()).unwrap()
    }

    /// Little-endian bytes.
    #[derive(Default)]
    struct W(Vec<u8>);
    impl W {
        fn i32(&mut self, v: i32) -> &mut W {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        fn u32(&mut self, v: u32) -> &mut W {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        fn u8(&mut self, v: u8) -> &mut W {
            self.0.push(v);
            self
        }
        fn f32(&mut self, v: f32) -> &mut W {
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        fn name(&mut self, n: &str) -> &mut W {
            self.i32(name_index(n)).i32(0)
        }
        fn fstring(&mut self, s: &str) -> &mut W {
            self.i32(i32::try_from(s.len() + 1).unwrap());
            self.0.extend_from_slice(s.as_bytes());
            self.u8(0)
        }
        /// A tag header: name, type, size, array index, the type's data, no property GUID.
        fn tag(&mut self, name: &str, ty: &str, size: usize, index: i32, data: impl FnOnce(&mut W)) -> &mut W {
            self.name(name).name(ty).i32(i32::try_from(size).unwrap()).i32(index);
            data(self);
            self.u8(0)
        }
    }

    /// A package of NAMES and nothing else (a cooked UE 4.26 summary with no exports).
    fn package() -> Arc<Package> {
        let mut w = W::default();
        w.u32(0x9E2A_83C1).i32(-7).i32(0).i32(0).i32(0).i32(0); // tag, legacy, ue3, ue4, licensee, custom versions
        w.i32(0).fstring("").u32(0x8000_0000); // header size, folder, flags
        let name_count_at = w.0.len();
        w.i32(0).i32(0); // names
        w.i32(0).i32(0); // gatherable text
        w.i32(0).i32(0).i32(0).i32(0); // exports, imports
        w.i32(0).i32(0).i32(0).i32(0).i32(0); // depends, soft refs, searchable, thumbnails
        w.0.extend_from_slice(&[0; 16]); // guid
        w.i32(0); // generations
        for _ in 0..2 {
            w.0.extend_from_slice(&[0; 6]);
            w.u32(0).fstring("");
        }
        w.u32(0).i32(0).u32(0).i32(0); // compression, chunks, source, extra
        w.i32(0).i32(0).i32(0).i32(0).i32(0).i32(0).i32(0); // registry, bulk (i64), tile, chunk ids, preload
        let names_at = i32::try_from(w.0.len()).unwrap();
        for n in NAMES {
            w.fstring(n).u32(0);
        }
        let count = i32::try_from(NAMES.len()).unwrap();
        w.0[name_count_at..name_count_at + 4].copy_from_slice(&count.to_le_bytes());
        w.0[name_count_at + 4..name_count_at + 8].copy_from_slice(&names_at.to_le_bytes());
        Arc::new(Package::parse(&w.0, "MultiVersus/Content/T", "/Game/T", Arc::new(NoFiles)).unwrap())
    }

    fn cache() -> PackageCache {
        let paths = Arc::new(BuildPaths::new("MultiVersus", &[]).unwrap());
        PackageCache::new(Arc::new(NoFiles), paths, 1 << 20)
    }

    /// The tagged properties of `data`, and what the read reported.
    fn read(data: &[u8]) -> (Props, Vec<String>) {
        let pkg = package();
        let cache = cache();
        let mut ar = ExportArchive {
            ar: crate::archive::Archive::new(data, "t"),
            pkg: &pkg,
            cache: &cache,
            export_index: 0,
            issues: Vec::new(),
        };
        let mut out = Props::new();
        read_tagged_properties(&mut ar, &mut out, None).unwrap();
        assert_eq!(ar.pos, data.len(), "read to the end");
        (out, ar.issues.into_iter().map(|i| i.message).collect())
    }

    #[test]
    fn bool_tag_and_fixed_array() {
        let mut w = W::default();
        w.tag("A", "BoolProperty", 0, 0, |w| {
            w.u8(1);
        });
        w.tag("A", "IntProperty", 4, 1, |_| {}).i32(7);
        w.name("None");
        let (p, issues) = read(&w.0);
        assert!(issues.is_empty());
        assert!(matches!(p["A"], Value::Bool(true)));
        assert!(matches!(p["A[1]"], Value::Int(7)));
    }

    #[test]
    fn text_histories() {
        let mut w = W::default();
        w.tag("A", "TextProperty", 17, 0, |_| {}).u32(0).u8(0xff).i32(1).fstring("inv");
        w.tag("A", "TextProperty", 9, 1, |_| {}).u32(0).u8(0xff).i32(0);
        w.tag("A", "TextProperty", 26, 2, |_| {}).u32(0).u8(0).fstring("ns").fstring("k").fstring("src");
        w.tag("A", "TextProperty", 21, 3, |_| {}).u32(0).u8(11).name("Table").fstring("key");
        // an argument-format history: not ported, reported, null, the tag skipped by its size
        w.tag("A", "TextProperty", 9, 4, |_| {}).u32(0).u8(3).i32(0);
        w.name("None");
        let (p, issues) = read(&w.0);
        let text = |k: &str| match &p[k] {
            Value::Text(t) => t.clone(),
            v => panic!("{k}: {v:?}"),
        };
        assert_eq!(text("A"), TextValue::Invariant { source: Some("inv".into()) });
        assert_eq!(text("A[1]"), TextValue::Invariant { source: None });
        assert_eq!(text("A[2]"), TextValue::Base { source: "src".into(), namespace: "ns".into(), key: "k".into() });
        assert_eq!(text("A[3]"), TextValue::Table { source: None, table: "Table".into(), key: "key".into() });
        assert!(matches!(p["A[4]"], Value::Null));
        assert_eq!(issues, ["A: FText history 3 not ported"]);
    }

    #[test]
    fn map_and_set() {
        let mut w = W::default();
        let mut body = W::default();
        body.i32(0).i32(2).i32(1).fstring("one").i32(2).fstring("two");
        w.tag("A", "MapProperty", body.0.len(), 0, |w| {
            w.name("IntProperty").name("StrProperty");
        });
        w.0.extend_from_slice(&body.0);
        // a set: its removed elements are read and dropped
        let mut body = W::default();
        body.i32(1).name("Vector").i32(2).name("A").name("Table");
        w.tag("A", "SetProperty", body.0.len(), 1, |w| {
            w.name("NameProperty");
        });
        w.0.extend_from_slice(&body.0);
        w.name("None");
        let (p, issues) = read(&w.0);
        assert!(issues.is_empty());
        let Value::Map(m) = &p["A"] else { panic!("{:?}", p["A"]) };
        assert_eq!(m.len(), 2);
        assert!(matches!((&m[1].0, &m[1].1), (Value::Int(2), Value::Str(s)) if s == "two"));
        let Value::Array(s) = &p["A[1]"] else { panic!("{:?}", p["A[1]"]) };
        let names: Vec<&str> = s.iter().filter_map(Value::as_str).collect();
        assert_eq!(names, ["A", "Table"]);
    }

    #[test]
    fn array_of_native_structs() {
        let mut body = W::default();
        body.i32(1);
        body.tag("A", "StructProperty", 12, 0, |w| {
            w.name("Vector").0.extend_from_slice(&[0; 16]);
        });
        body.f32(1.0).f32(2.0).f32(3.0);
        let mut w = W::default();
        w.tag("A", "ArrayProperty", body.0.len(), 0, |w| {
            w.name("StructProperty");
        });
        w.0.extend_from_slice(&body.0);
        w.name("None");
        let (p, _) = read(&w.0);
        let Value::Array(a) = &p["A"] else { panic!("{:?}", p["A"]) };
        let v = a[0].as_struct().unwrap();
        assert!(matches!(v["Z"], Value::Float(z) if z == 3.0));
    }
}
