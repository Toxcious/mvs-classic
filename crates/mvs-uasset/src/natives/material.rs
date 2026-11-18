//! Materials: UMaterialInterface, UMaterial, UMaterialInstance (CUE4Parse 1.2.2, with ReadShaderMaps on for the tail).
//!
//! The native tail of a UMaterial, and of a UMaterialInstance with bHasStaticPermutationResource (a
//! MaterialInstanceConstant, a level's LandscapeMaterialInstanceConstant), is DeserializeInlineShaderMaps
//! (`frozen::read_inline_shader_maps`): it is read here in its archive form and has to end at the export's end; where
//! it starts is kept in the object's data ([`SHADER_MAPS_AT`]), from which the shadermap and shader kinds read it
//! again. The object kind writes no shader map (LoadedMaterialResources is the empty list, as cue4dump wrote it with
//! ReadShaderMaps off).

use super::Native;
use super::frozen::read_inline_shader_maps;
use crate::archive::{ReadError, Result};
use crate::objects::{ExportArchive, UObj, super_deserialize};
use crate::value::{Props, Value};

/// The data key of where a material's inline shader maps start in its package's export data (the .uexp), a
/// `Value::UInt`; absent when the export serializes none.
pub const SHADER_MAPS_AT: &str = "ShaderMapsAt";

/// By C# class name.
pub static NATIVES: &[(&str, Native)] = &[
    ("UMaterialInterface", Native { deserialize: None, json: Some(interface_json), extra: None }),
    ("UMaterial", Native { deserialize: Some(material_deserialize), json: None, extra: None }),
    ("UMaterialInstance", Native { deserialize: Some(instance_deserialize), json: None, extra: None }),
];

// CachedExpressionData is serialized from UE5 (FUE5ReleaseStreamObjectVersion); UMaterial takes it from its property
fn interface_json(obj: &UObj, o: &mut Props) {
    o.insert("LoadedMaterialResources".into(), Value::Array(Vec::new()));
    if let Some(c) = obj.data.get("CachedExpressionData")
        && !matches!(c, Value::Null)
    {
        o.insert("CachedExpressionData".into(), c.clone());
    }
}

/// The inline shader maps, read to the export's end; where they start goes in the object's data.
fn read_tail(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize) -> Result<()> {
    let at = ar.pos;
    read_inline_shader_maps(&mut ar.ar)?;
    if ar.pos != valid_pos {
        return Err(ReadError::parse(format!(
            "{}: the inline shader maps end at {}, the export at {valid_pos}",
            ar.name, ar.pos
        )));
    }
    obj.data.insert(SHADER_MAPS_AT.into(), Value::UInt(at as u64));
    Ok(())
}

fn material_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    if let Some(c) = obj.props.get("CachedExpressionData") {
        let c = c.clone();
        obj.data.insert("CachedExpressionData".into(), c);
    }
    read_tail(obj, ar, valid_pos)
}

fn instance_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    // FRenderingObjectVersion is past MaterialAttributeLayerParameters in 4.26: no StaticParameters before the maps
    if matches!(obj.props.get("bHasStaticPermutationResource"), Some(Value::Bool(true))) {
        read_tail(obj, ar, valid_pos)?;
    }
    Ok(())
}
