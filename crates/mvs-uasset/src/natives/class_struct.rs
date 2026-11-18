//! The reflection classes: UField, UStruct (with its FField child properties), UClass, UFunction,
//! UBlueprintGeneratedClass (CUE4Parse 1.2.2 Deserialize and WriteJson of each; FField.Construct and the FProperty
//! family), and the enum and struct assets UEnum and UUserDefinedStruct.

use super::Native;
use crate::archive::{ReadError, Result};
use crate::fields::{FieldsSource, StructFields};
use crate::flags::{CLASS_FLAGS, FUNCTION_FLAGS, PROPERTY_FLAGS, bitfield, object_flags_text};
use crate::objects::{ExportArchive, RF_CLASS_DEFAULT_OBJECT, UObj, super_deserialize};
use crate::props::read_tagged_properties;
use crate::value::{Props, Value};
use crate::versions;

/// By C# class name.
pub static NATIVES: &[(&str, Native)] = &[
    ("UField", Native { deserialize: Some(field_deserialize), json: Some(field_json), extra: None }),
    ("UStruct", Native { deserialize: Some(struct_deserialize), json: Some(struct_json), extra: None }),
    ("UClass", Native { deserialize: Some(class_deserialize), json: Some(class_json), extra: None }),
    ("UFunction", Native { deserialize: Some(function_deserialize), json: Some(function_json), extra: None }),
    ("UBlueprintGeneratedClass", Native { deserialize: Some(bpgc_deserialize), json: Some(bpgc_json), extra: None }),
    ("UEnum", Native { deserialize: Some(enum_deserialize), json: Some(enum_json), extra: None }),
    (
        "UUserDefinedStruct",
        Native { deserialize: Some(user_struct_deserialize), json: Some(user_struct_json), extra: None },
    ),
];

const LIFETIME_CONDITIONS: &[&str] = &[
    "COND_None",
    "COND_InitialOnly",
    "COND_OwnerOnly",
    "COND_SkipOwner",
    "COND_SimulatedOnly",
    "COND_AutonomousOnly",
    "COND_SimulatedOrPhysics",
    "COND_InitialOrOwner",
    "COND_Custom",
    "COND_ReplayOrOwner",
    "COND_ReplayOnly",
    "COND_SimulatedOnlyNoReplay",
    "COND_SimulatedOrPhysicsNoReplay",
    "COND_SkipReplay",
    "COND_Max",
];

const CPP_FORMS: &[&str] = &["Regular", "Namespaced", "EnumClass"];

/// FFrameworkObjectVersion RemoveUField_Next.
const REMOVE_UFIELD_NEXT: i32 = 29;
/// FFrameworkObjectVersion UserDefinedStructsStoreDefaultInstance.
const USER_DEFINED_STRUCTS_STORE_DEFAULT_INSTANCE: i32 = 32;
/// FCoreObjectVersion FProperties.
const FPROPERTIES: i32 = 4;
/// FFortniteMainBranchObjectVersion BPGCCookedEditorTags.
const BPGC_COOKED_EDITOR_TAGS: i32 = 57;
const FUNC_NET: u32 = 0x40;

/// A name-keyed table's entry set as a Map's: a repeated name keeps its first place and takes the last value.
fn set_pair(m: &mut Vec<(Value, Value)>, k: String, v: Value) {
    match m.iter_mut().find(|(x, _)| x.as_str() == Some(k.as_str())) {
        Some(e) => e.1 = v,
        None => m.push((Value::Str(k), v)),
    }
}

fn non_null(v: Option<&Value>) -> bool {
    !matches!(v, None | Some(Value::Null) | Some(Value::Ref(None)))
}

fn copy(obj: &UObj, out: &mut Props, key: &str) {
    if let Some(v) = obj.data.get(key) {
        out.insert(key.to_owned(), v.clone());
    }
}

/// What an FField class reads after FProperty's fields; the type its WriteJson names.
fn field_kind(t: &str) -> Option<&'static str> {
    Some(match t {
        "ArrayProperty"
        | "BoolProperty"
        | "ByteProperty"
        | "ClassProperty"
        | "DelegateProperty"
        | "EnumProperty"
        | "FieldPathProperty"
        | "DoubleProperty"
        | "FloatProperty"
        | "Int16Property"
        | "Int64Property"
        | "Int8Property"
        | "IntProperty"
        | "InterfaceProperty"
        | "MapProperty"
        | "MulticastDelegateProperty"
        | "MulticastInlineDelegateProperty"
        | "NameProperty"
        | "ObjectProperty"
        | "SetProperty"
        | "SoftClassProperty"
        | "SoftObjectProperty"
        | "StrProperty"
        | "StructProperty"
        | "TextProperty"
        | "UInt16Property"
        | "UInt32Property"
        | "UInt64Property"
        | "WeakObjectProperty" => {
            return FIELD_TYPES.iter().copied().find(|k| *k == t);
        }
        "ObjectPtrProperty" => "ObjectProperty",
        _ => return None,
    })
}

static FIELD_TYPES: &[&str] = &[
    "ArrayProperty",
    "BoolProperty",
    "ByteProperty",
    "ClassProperty",
    "DelegateProperty",
    "EnumProperty",
    "FieldPathProperty",
    "DoubleProperty",
    "FloatProperty",
    "Int16Property",
    "Int64Property",
    "Int8Property",
    "IntProperty",
    "InterfaceProperty",
    "MapProperty",
    "MulticastDelegateProperty",
    "MulticastInlineDelegateProperty",
    "NameProperty",
    "ObjectProperty",
    "SetProperty",
    "SoftClassProperty",
    "SoftObjectProperty",
    "StrProperty",
    "StructProperty",
    "TextProperty",
    "UInt16Property",
    "UInt32Property",
    "UInt64Property",
    "WeakObjectProperty",
];

/// FField.Construct + Deserialize (FField: Name, Flags; FProperty: the rest) for a field of serialized type `t`, as
/// its WriteJson writes it.
fn read_field(ar: &mut ExportArchive<'_>, t: &str) -> Result<Value> {
    let Some(kind) = field_kind(t) else {
        return Err(ReadError::parse(format!("Unsupported serialized property type {t}")));
    };
    let mut o = Props::new();
    o.insert("Type".into(), Value::Str(kind.to_owned()));
    o.insert("Name".into(), Value::Str(ar.fname()?));
    let flags = ar.u32()?;
    if flags != 0 {
        o.insert("Flags".into(), Value::Str(object_flags_text(flags)));
    }
    // every type CUE4Parse constructs here is an FProperty
    let array_dim = ar.i32()?;
    let element_size = ar.i32()?;
    let property_flags = ar.u64()?;
    let rep_index = ar.u16()?;
    let rep_notify = ar.fname()?;
    let cond = ar.u8()?;
    if array_dim != 1 {
        o.insert("ArrayDim".into(), Value::Int(array_dim.into()));
    }
    if element_size != 0 {
        o.insert("ElementSize".into(), Value::Int(element_size.into()));
    }
    if property_flags != 0 {
        o.insert("PropertyFlags".into(), Value::Str(bitfield(property_flags, PROPERTY_FLAGS)));
    }
    if rep_index != 0 {
        o.insert("RepIndex".into(), Value::Int(rep_index.into()));
    }
    if rep_notify != "None" {
        o.insert("RepNotifyFunc".into(), Value::Str(rep_notify));
    }
    if cond != 0 {
        let c = match LIFETIME_CONDITIONS.get(usize::from(cond)) {
            Some(n) => Value::Str((*n).to_owned()),
            None => Value::Int(cond.into()),
        };
        o.insert("BlueprintReplicationCondition".into(), c);
    }
    let put = |o: &mut Props, k: &str, v: Value| {
        o.insert(k.to_owned(), v);
    };
    match t {
        "ArrayProperty" => put(&mut o, "Inner", read_single_field(ar)?),
        "BoolProperty" => {
            for k in ["FieldSize", "ByteOffset", "ByteMask", "FieldMask", "BoolSize"] {
                put(&mut o, k, Value::Int(ar.u8()?.into()));
            }
            put(&mut o, "bIsNativeBool", Value::Bool(ar.flag()?));
        }
        "ByteProperty" => put(&mut o, "Enum", Value::Ref(ar.index()?)),
        "ClassProperty" | "SoftClassProperty" => {
            put(&mut o, "PropertyClass", Value::Ref(ar.index()?));
            put(&mut o, "MetaClass", Value::Ref(ar.index()?));
        }
        "DelegateProperty" | "MulticastDelegateProperty" | "MulticastInlineDelegateProperty" => {
            put(&mut o, "SignatureFunction", Value::Ref(ar.index()?));
        }
        "EnumProperty" => {
            put(&mut o, "Enum", Value::Ref(ar.index()?));
            put(&mut o, "UnderlyingProp", read_single_field(ar)?);
        }
        // FFieldPathProperty reads PropertyClass but its WriteJson does not write it
        "FieldPathProperty" => {
            ar.fname()?;
        }
        "InterfaceProperty" => put(&mut o, "InterfaceClass", Value::Ref(ar.index()?)),
        "MapProperty" => {
            put(&mut o, "KeyProp", read_single_field(ar)?);
            put(&mut o, "ValueProp", read_single_field(ar)?);
        }
        "ObjectPtrProperty" | "ObjectProperty" | "SoftObjectProperty" | "WeakObjectProperty" => {
            put(&mut o, "PropertyClass", Value::Ref(ar.index()?));
        }
        "SetProperty" => put(&mut o, "ElementProp", read_single_field(ar)?),
        "StructProperty" => put(&mut o, "Struct", Value::Ref(ar.index()?)),
        _ => {}
    }
    Ok(Value::Struct(o))
}

/// FField.SerializeSingleField: null for None.
fn read_single_field(ar: &mut ExportArchive<'_>) -> Result<Value> {
    let t = ar.fname()?;
    if t == "None" { Ok(Value::Null) } else { read_field(ar, &t) }
}

// UField: 4.26 has no SuperField (UE3 only) and no Next (FFrameworkObjectVersion >= RemoveUField_Next)
fn field_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    if ar.custom(versions::FRAMEWORK) < REMOVE_UFIELD_NEXT {
        obj.data.insert("Next".into(), Value::Ref(ar.index()?));
    }
    Ok(())
}

fn field_json(obj: &UObj, o: &mut Props) {
    if non_null(obj.data.get("Next")) {
        copy(obj, o, "Next");
    }
}

fn struct_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    let super_index = ar.i32()?;
    obj.data.insert("SuperStructIndex".into(), Value::Int(super_index.into()));
    // the SuperStruct WriteJson writes, resolved here (the writer has no cache). ResolvedObject.Equals compares the
    // package by reference and CUE4Parse loads another package anew per resolve: only an export of this same
    // package equals Super, and is not written
    if super_index != 0 {
        let r = ar.cache.try_resolve(&obj.pkg, super_index)?;
        let sup = ar.cache.try_resolve(&obj.pkg, obj.pkg.exports[obj.index].super_index)?;
        if let Some(r) = r {
            let same_here = sup.as_ref().is_some_and(|s| r.same(s))
                && r.as_export().is_some_and(|(p, _)| std::sync::Arc::ptr_eq(p, &obj.pkg));
            if !same_here {
                obj.data.insert("SuperStruct".into(), Value::Ref(Some(r)));
            }
        }
    }
    let n = ar.i32()?;
    let mut children = Vec::new();
    for _ in 0..n {
        children.push(Value::Ref(ar.index()?));
    }
    obj.data.insert("Children".into(), Value::Array(children));
    if ar.custom(versions::CORE) >= FPROPERTIES {
        let m = ar.i32()?;
        let mut props = Vec::new();
        for _ in 0..m {
            let t = ar.fname()?;
            props.push(read_field(ar, &t)?);
        }
        obj.data.insert("ChildProperties".into(), Value::Array(props));
    }
    ar.i32()?; // bytecode buffer size
    let storage = ar.i32()?;
    // where the serialized script lies in the export data (the object kind reads no bytecode)
    obj.data.insert("ScriptAt".into(), Value::Int(i64::try_from(ar.pos).unwrap_or(i64::MAX)));
    obj.data.insert("ScriptSize".into(), Value::Int(storage.into()));
    let Ok(storage) = usize::try_from(storage) else {
        return Err(ReadError::parse(format!("negative script size {storage}")));
    };
    ar.skip(storage)?;
    Ok(())
}

fn struct_json(obj: &UObj, o: &mut Props) {
    if non_null(obj.data.get("SuperStruct")) {
        copy(obj, o, "SuperStruct");
    }
    if matches!(obj.data.get("Children"), Some(Value::Array(a)) if !a.is_empty()) {
        copy(obj, o, "Children");
    }
    if matches!(obj.data.get("ChildProperties"), Some(Value::Array(a)) if !a.is_empty()) {
        copy(obj, o, "ChildProperties");
    }
}

fn class_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    let n = ar.i32()?;
    let mut func_map = Vec::new(); // in the build's order
    for _ in 0..n {
        let k = ar.fname()?;
        let v = Value::Ref(ar.index()?);
        set_pair(&mut func_map, k, v);
    }
    if n > 0 {
        obj.data.insert("FuncMap".into(), Value::Map(func_map));
    }
    obj.data.insert("ClassFlags".into(), Value::Int(ar.u32()?.into()));
    obj.data.insert("ClassWithin".into(), Value::Ref(ar.index()?));
    obj.data.insert("ClassConfigName".into(), Value::Str(ar.fname()?));
    obj.data.insert("ClassGeneratedBy".into(), Value::Ref(ar.index()?));
    let ni = ar.i32()?;
    let mut interfaces = Vec::new();
    for _ in 0..ni {
        let class = Value::Ref(ar.index()?);
        let offset = Value::Int(ar.i32()?.into());
        let k2 = Value::Bool(ar.bool()?);
        interfaces.push(Value::Struct(Props::from([
            ("Class".to_owned(), class),
            ("PointerProperty".to_owned(), Value::Null),
            ("PointerOffset".to_owned(), offset),
            ("bImplementedByK2".to_owned(), k2),
        ])));
    }
    obj.data.insert("Interfaces".into(), Value::Array(interfaces));
    ar.bool()?; // bDeprecatedForceScriptOrder
    ar.fname()?; // dummy
    obj.data.insert("bCooked".into(), Value::Bool(ar.bool()?));
    obj.data.insert("ClassDefaultObject".into(), Value::Ref(ar.index()?));
    Ok(())
}

fn class_json(obj: &UObj, o: &mut Props) {
    let d = &obj.data;
    copy(obj, o, "FuncMap");
    if let Some(f) = d.get("ClassFlags").and_then(Value::as_i64).filter(|&f| f != 0) {
        o.insert("ClassFlags".into(), Value::Str(bitfield(f.cast_unsigned(), CLASS_FLAGS)));
    }
    if non_null(d.get("ClassWithin")) {
        copy(obj, o, "ClassWithin");
    }
    if d.get("ClassConfigName").and_then(Value::as_str).is_some_and(|n| n != "None") {
        copy(obj, o, "ClassConfigName");
    }
    if non_null(d.get("ClassGeneratedBy")) {
        copy(obj, o, "ClassGeneratedBy");
    }
    if matches!(d.get("Interfaces"), Some(Value::Array(a)) if !a.is_empty()) {
        copy(obj, o, "Interfaces");
    }
    if d.get("bCooked").and_then(Value::as_bool) == Some(true) {
        o.insert("bCooked".into(), Value::Bool(true));
    }
    if non_null(d.get("ClassDefaultObject")) {
        copy(obj, o, "ClassDefaultObject");
    }
}

fn function_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    let flags = ar.u32()?;
    obj.data.insert("FunctionFlags".into(), Value::Int(flags.into()));
    if flags & FUNC_NET != 0 {
        ar.i16()?;
    }
    obj.data.insert("EventGraphFunction".into(), Value::Ref(ar.index()?));
    obj.data.insert("EventGraphCallOffset".into(), Value::Int(ar.i32()?.into()));
    Ok(())
}

fn function_json(obj: &UObj, o: &mut Props) {
    let d = &obj.data;
    let flags = d.get("FunctionFlags").and_then(Value::as_i64).unwrap_or(0);
    o.insert("FunctionFlags".into(), Value::Str(bitfield(flags.cast_unsigned(), FUNCTION_FLAGS)));
    if non_null(d.get("EventGraphFunction")) {
        copy(obj, o, "EventGraphFunction");
    }
    if d.get("EventGraphCallOffset").and_then(Value::as_i64).is_some_and(|v| v != 0) {
        copy(obj, o, "EventGraphCallOffset");
    }
}

// UBlueprintGeneratedClass: EditorTags need FFortniteMainBranchObjectVersion >= BPGCCookedEditorTags (57); 4.26 has
// 43: nothing more to read
fn bpgc_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    if ar.custom(versions::FORTNITE_MAIN) >= BPGC_COOKED_EDITOR_TAGS && valid_pos as i64 - ar.pos as i64 > 4 {
        let n = ar.i32()?;
        let mut tags = Vec::new();
        for _ in 0..n {
            let k = ar.fname()?;
            let v = Value::Str(ar.fstring()?);
            set_pair(&mut tags, k, v);
        }
        if n > 0 {
            obj.data.insert("EditorTags".into(), Value::Map(tags));
        }
    }
    Ok(())
}

fn bpgc_json(obj: &UObj, o: &mut Props) {
    copy(obj, o, "EditorTags");
}

fn enum_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    let n = ar.i32()?;
    let mut names = Vec::new();
    for _ in 0..n {
        let k = ar.fname()?;
        let v = Value::Int(ar.i64()?);
        set_pair(&mut names, k, v);
    }
    obj.data.insert("Names".into(), Value::Map(names));
    obj.data.insert("CppForm".into(), Value::Int(ar.u8()?.into()));
    // EnumUnderlyingType (FFortniteMainBranchObjectVersion 255) is past 4.26
    Ok(())
}

fn enum_json(obj: &UObj, o: &mut Props) {
    o.insert("Names".into(), obj.data.get("Names").cloned().unwrap_or(Value::Map(Vec::new())));
    let f = obj.data.get("CppForm").and_then(Value::as_i64).unwrap_or(0);
    let form = match usize::try_from(f).ok().and_then(|i| CPP_FORMS.get(i)) {
        Some(n) => Value::Str((*n).to_owned()),
        None => Value::Int(f),
    };
    o.insert("CppForm".into(), form);
}

fn user_struct_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    let status = obj.props.get("Status");
    let stale = status.is_some_and(|s| s.as_str() != Some("EUserDefinedStructureStatus::UDSS_UpToDate"));
    if obj.flags & RF_CLASS_DEFAULT_OBJECT != 0 || stale {
        return Ok(());
    }
    obj.data.insert("StructFlags".into(), Value::Int(ar.u32()?.into()));
    if ar.custom(versions::FRAMEWORK) >= USER_DEFINED_STRUCTS_STORE_DEFAULT_INSTANCE {
        // its own fields, from what it read so far
        let sup = obj.data.get("SuperStructIndex").and_then(Value::as_i64).and_then(|i| i32::try_from(i).ok());
        let super_struct = sup.filter(|&i| i != 0).and_then(|i| ar.cache.try_resolve(&obj.pkg, i).ok().flatten());
        let child_properties = obj.data.get("ChildProperties").cloned().unwrap_or(Value::Null);
        let own = StructFields::new(FieldsSource::Own { child_properties, super_struct });
        let mut values = Props::new();
        read_tagged_properties(ar, &mut values, Some(&own))?;
        let mut def = Props::new();
        for (k, v) in values {
            def.insert(strip_array_index(&k).to_owned(), v);
        }
        obj.data.insert("DefaultProperties".into(), Value::Struct(def));
    }
    Ok(())
}

/// `Name[i]` -> `Name` (`/\[\d+\]$/`).
fn strip_array_index(k: &str) -> &str {
    if let Some(head) = k.strip_suffix(']')
        && let Some(at) = head.rfind('[')
        && head.len() > at + 1
        && head[at + 1..].bytes().all(|b| b.is_ascii_digit())
    {
        return &k[..at];
    }
    k
}

fn user_struct_json(obj: &UObj, o: &mut Props) {
    o.insert("StructFlags".into(), obj.data.get("StructFlags").cloned().unwrap_or(Value::Int(0)));
    if matches!(obj.data.get("DefaultProperties"), Some(Value::Struct(d)) if !d.is_empty()) {
        copy(obj, o, "DefaultProperties");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_names() {
        let mut m = Vec::new();
        set_pair(&mut m, "A".into(), Value::Int(1));
        set_pair(&mut m, "B".into(), Value::Int(2));
        set_pair(&mut m, "A".into(), Value::Int(3));
        let got: Vec<(&str, i64)> = m.iter().map(|(k, v)| (k.as_str().unwrap(), v.as_i64().unwrap())).collect();
        assert_eq!(got, [("A", 3), ("B", 2)]);
    }

    #[test]
    fn array_index_suffix() {
        assert_eq!(strip_array_index("A[2]"), "A");
        assert_eq!(strip_array_index("A[]"), "A[]");
        assert_eq!(strip_array_index("A[x]"), "A[x]");
        assert_eq!(strip_array_index("A"), "A");
    }
}
