//! The `atmosphere` kind: per AtmosphericFogComponent of the stages' gameplay and lighting levels, its precomputed
//! inscatter table for the AtmosphericFog pass (a json-bin container, ATMOSPHERE_MAGIC: the header names the component
//! and the table's size; the body is the table, four float16 per texel), at `game::atmosphere_path(level)` of the
//! level that holds it; each stage's stagedraw entry names its own.
//! The layout is read by `mvs-render` (stage.rs load_inscatter).
//!
//! An earlier header also held `pixel` and `vertex`: the fog pass's pixel shader and a vertex shader's fog
//! translated to GLSL for a browser renderer. The native renderer draws the fog with the global shader cache's own
//! shader (the material kind's global entry), so this kind writes neither; the compare leaves them out.

use mvs_uasset::value::Value;

use crate::game::{ATMOSPHERE_BIND_VERSION, ATMOSPHERE_KIND_VERSION, GAMEDATA, atmosphere_path, exported, stages};
use crate::jsonbin::{json_bin, lay_out_body};
use crate::kind::{Cx, Item, Kind, KindError};
use crate::kinds::object::OBJECT_KIND_VERSION;
use crate::kinds::shader::SHADER_KIND_VERSION;
use crate::kinds::shadermap::SHADERMAP_KIND_VERSION;
use crate::objview::{field, num, short_name, truthy};
use crate::stage_levels::{StageLevels, stage_levels};

/// The entry's magic.
pub const ATMOSPHERE_MAGIC: &[u8; 4] = b"MVAF";

/// The entry's layout.
pub const ATMOSPHERE_FORMAT_VERSION: u32 = 1;

/// A stage's AtmosphericFogComponent: its level (below MAPS_ROOT) and object path. The stage kind's choice (gamedata
/// stage/render: the gameplay and lighting levels' AtmosphericFog actors not bHidden, the last one's), so the table is
/// the component the stage's atmosphere members come from.
pub(crate) fn stage_component(levels: &StageLevels<'_>) -> Result<Option<(String, String)>, KindError> {
    let mut found = None;
    for a in levels.actors(false) {
        if short_name(a.class()) != "AtmosphericFog" || a.prop("bHidden").is_some_and(truthy) {
            continue;
        }
        let Some(c) = levels.comps(a, Some("AtmosphericFogComponent"))?.into_iter().next() else {
            return Err(KindError::Failed(format!("atmosphere: {} has no AtmosphericFogComponent", a.path())));
        };
        found = Some((levels.level_of(a)?.level.clone(), c.path().to_owned()));
    }
    Ok(found)
}

/// The atmosphere entry a stage's levels draw with (`exported(atmosphere_path(level))`); None without a component.
pub fn atmosphere_entry(levels: &StageLevels<'_>) -> Result<Option<String>, KindError> {
    Ok(stage_component(levels)?.map(|(level, _)| exported(&atmosphere_path(&level))))
}

/// Every stage's component, once per level, in the stages' order: (entry path, component path).
fn components(cx: &Cx<'_>) -> Result<Vec<(String, String)>, KindError> {
    let st = stages(cx)?;
    let mut out: Vec<(String, String)> = Vec::new();
    for key in &st.keys {
        let entry = st.get(key)?;
        if entry.data.atmosphere.is_none() {
            continue;
        }
        let levels = stage_levels(cx, key, &entry.manifest.levels)?;
        let Some((level, component)) = stage_component(&levels)? else {
            return Err(KindError::Failed(format!("atmosphere: {key}: an atmosphere and no AtmosphericFogComponent")));
        };
        let path = exported(&atmosphere_path(&level));
        match out.iter().find(|(p, _)| *p == path) {
            Some((_, c)) if *c != component => {
                return Err(KindError::Failed(format!("atmosphere: {level} holds {c} and {component}")));
            }
            Some(_) => {}
            None => out.push((path, component)),
        }
    }
    Ok(out)
}

/// The entry's header.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    version: u32,
    /// The bind's version and the kinds' an earlier bind read (the entry's path names them too).
    bind_version: u64,
    object_version: u64,
    shadermap_version: u64,
    shader_version: u64,
    component: String,
    /// The table's size in texels of four float16 (PF_FloatRGBA, little-endian), x fastest.
    size: [usize; 3],
    table: crate::jsonbin::BodyRef,
}

/// A component's inscatter table: its size and bytes (`inscatterTable`).
fn inscatter_table(cx: &Cx<'_>, component: &str) -> Result<([usize; 3], std::sync::Arc<[u8]>), KindError> {
    let o = cx.object(component)?;
    if o.class() != "/Script/Engine.AtmosphericFogComponent" {
        return Err(KindError::Failed(format!("atmosphere: {component} is a {}", o.class())));
    }
    let native = o.native()?;
    let t = native.as_ref().and_then(|n| field(n, "InscatterData")).and_then(Value::as_struct).ok_or_else(|| {
        KindError::Failed(format!("atmosphere: {component} holds no tables (object kind v{OBJECT_KIND_VERSION})"))
    })?;
    let dim = |name: &str| {
        field(t, name)
            .and_then(num)
            .and_then(|d| usize::try_from(d as i64).ok())
            .ok_or_else(|| KindError::Failed(format!("atmosphere: {component}: no {name}")))
    };
    let (x, y, z) = (dim("SizeX")?, dim("SizeY")?, dim("SizeZ")?);
    let format = field(t, "PixelFormat").and_then(Value::as_str).unwrap_or_default();
    if format != "PF_FloatRGBA" {
        return Err(KindError::Failed(format!("atmosphere: {component}: an inscatter table of {format}")));
    }
    let Some(Value::Bytes(bytes)) = field(t, "Data") else {
        return Err(KindError::Failed(format!(
            "atmosphere: {component} holds no tables (object kind v{OBJECT_KIND_VERSION})"
        )));
    };
    if bytes.len() != x * y * z * 8 {
        return Err(KindError::Failed(format!("atmosphere: {component}: {} bytes for {x} x {y} x {z}", bytes.len())));
    }
    Ok(([x, y, z], bytes.clone()))
}

/// The `atmosphere` kind.
pub struct Atmosphere;

impl Kind for Atmosphere {
    fn id(&self) -> &'static str {
        "atmosphere"
    }

    fn version(&self) -> u64 {
        ATMOSPHERE_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/octet-stream"
    }

    /// An entry per level that holds a stage's component (`exported(game::atmosphere_path(level))`).
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(components(cx)?.into_iter().map(|(p, _)| Item::new(p)).collect())
    }

    // The header has no `pixel` and `vertex` (GLSL for a browser renderer): the native renderer draws the cooked
    // shaders.
    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let Some((_, component)) = components(cx)?.into_iter().find(|(p, _)| *p == item.path) else {
            return Err(KindError::NotFound(format!("atmosphere_import: no entry {}", item.path)));
        };
        let (size, table) = inscatter_table(cx, &component)?;
        let (refs, _) = lay_out_body(&[&table[..]]);
        let header = Header {
            version: ATMOSPHERE_FORMAT_VERSION,
            bind_version: ATMOSPHERE_BIND_VERSION,
            object_version: u64::from(OBJECT_KIND_VERSION),
            shadermap_version: SHADERMAP_KIND_VERSION,
            shader_version: SHADER_KIND_VERSION,
            component,
            size,
            table: refs[0],
        };
        json_bin(ATMOSPHERE_MAGIC, &header, &[&table[..]], &refs).map_err(|e| KindError::Failed(e.to_string()))
    }

    fn reads(&self) -> &'static [(&'static str, u64)] {
        &[GAMEDATA]
    }
}
