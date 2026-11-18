//! The `stagelight` kind: per stage, what the renderer reads of its light product (JSON): the engine shapes the
//! stage data's meshes name, each stage mesh's lightmap and shadow map from its sky level's build data, and the
//! textures its materials sample. Read by `mvs-render`
//! (stage.rs).
//!
//! Reads the stages (manifest `sky`, StageData meshes and materials) from the store's gamedata kind; the sky level and
//! the textures through `Cx::object`, the registries' mesh build data from the lighting kind's build in-process
//! (`lighting::entry_header`).

use std::collections::BTreeSet;

use indexmap::IndexMap;
use mvs_uasset::value::{Props, Value};
use serde::Serialize;
use serde_json::Value as Json;

use super::lighting::entry_header;
use crate::game::{GAMEDATA, MAPS_ROOT, STAGELIGHT_KIND_VERSION, exported, stagelight_entry_path, stages};
use crate::kind::{Cx, Item, Kind, KindError};
use crate::objview::{ObjView, field, short_name, string, truthy};
use crate::stage_levels::level_built_data;

/// The engine shapes the stage data's meshes name (a StaticMeshActor's mesh), by the name
/// the stage data gives.
const SHAPES: [(&str, &str); 2] =
    [("Cube", "/Engine/BasicShapes/Cube.Cube"), ("Plane", "/Engine/BasicShapes/Plane.Plane")];
const BASIC_SHAPES: &str = "/Engine/BasicShapes/";
const STAGE_TEXTURES: &str = "/Game/Panda_Main/Maps/Test_Gym/Textures/";

/// The textures a stage material's parent samples without a parameter.
fn parent_textures(parent: &str) -> &'static [&'static str] {
    match parent {
        "DEV_Outline_01_M" => &["DEV_VertLine_A_01"],
        _ => &[],
    }
}

/// One lightmapped mesh's lightmap and shadow map (`StageLightmap`): the numbers as the lighting header's JSON holds
/// them.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Lightmap {
    lightmap: LightmapTexture,
    scale: Vec<Vec<Json>>,
    add: Vec<Vec<Json>>,
    coord_scale: Vec<Json>,
    coord_bias: Vec<Json>,
    shadow: Shadow,
}

#[derive(Serialize)]
struct LightmapTexture {
    path: Json,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Shadow {
    path: Json,
    coord_scale: Vec<Json>,
    coord_bias: Vec<Json>,
    channel_valid: Json,
    inv_penumbra: Vec<Json>,
    light_guids: Json,
    key_channel: i64,
}

/// A stage's light product (`StageLightValue`).
#[derive(Serialize)]
struct StageLightValue {
    shapes: IndexMap<&'static str, &'static str>,
    lightmaps: IndexMap<String, Lightmap>,
    textures: IndexMap<String, String>,
}

/// A vector's members as `[X, Y, Z, W]` (`v4`); a missing one is `undefined`, which JSON writes as null in an array.
fn v4(v: &Json) -> Vec<Json> {
    ["X", "Y", "Z", "W"].iter().map(|k| v.get(k).cloned().unwrap_or(Json::Null)).collect()
}

fn v2(v: &Json) -> Vec<Json> {
    ["X", "Y"].iter().map(|k| v.get(k).cloned().unwrap_or(Json::Null)).collect()
}

/// The actor of a placed component (`...:PersistentLevel.<actor>.<component>`): the chain's last but one name.
fn actor_of(o: &ObjView<'_>) -> Option<String> {
    let chain: Vec<&str> = o.chain().split('.').collect();
    chain.len().checked_sub(2).map(|i| chain[i].to_owned())
}

/// Whether a component's LightingChannels struct member `name` is set (`!!ch.name`).
fn channel(ch: Option<&Props>, name: &str) -> bool {
    ch.and_then(|c| field(c, name)).is_some_and(truthy)
}

/// A JSON value's truth (`Array.some(Boolean)`).
fn json_truthy(v: &Json) -> bool {
    match v {
        Json::Null => false,
        Json::Bool(b) => *b,
        Json::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Json::String(s) => !s.is_empty(),
        Json::Array(_) | Json::Object(_) => true,
    }
}

/// A stage's light product (`bindStageLight`).
fn bind_stage_light(cx: &Cx<'_>, key: &str) -> Result<StageLightValue, KindError> {
    let fail = |m: String| KindError::Failed(format!("stagelight {key}: {m}"));
    let all = stages(cx)?;
    let entry = all.get(key)?;
    let (st, level) = (&entry.data, &entry.manifest.sky);
    if level.is_empty() {
        return Err(fail("not in the stage manifest".into()));
    }
    let world = cx.object(&format!("{MAPS_ROOT}{level}"))?;
    let objects = world.inner()?;
    let built_data = level_built_data(&objects);
    let header = built_data.as_ref().map(|b| entry_header(cx, b)).transpose()?;
    let meshes = header.as_ref().and_then(|h| h.get("meshes"));
    let cls = |o: &ObjView<'_>| short_name(o.class()).to_owned();
    let keys: Vec<&ObjView<'_>> = objects
        .iter()
        .filter(|o| {
            cls(o) == "DirectionalLightComponent"
                && !matches!(o.prop("bAffectsWorld"), Some(Value::Bool(false)))
                && channel(o.prop("LightingChannels").and_then(Value::as_struct), "bChannel1")
        })
        .collect();
    let key_guid = || -> Result<Option<String>, KindError> {
        if keys.len() != 1 {
            return Err(fail(format!("expected one Key light, found {}", keys.len())));
        }
        Ok(keys[0].prop("LightGuid").and_then(|v| string(keys[0].cache(), v)))
    };
    let mut lightmaps: IndexMap<String, Lightmap> = IndexMap::new();
    for o in &objects {
        let Some(meshes) = meshes else { break };
        if cls(o) != "StaticMeshComponent" {
            continue;
        }
        let native = o.native()?;
        let id = native
            .as_ref()
            .and_then(|n| field(n, "LODData"))
            .and_then(|l| match l {
                Value::Array(a) => a.first(),
                _ => None,
            })
            .and_then(Value::as_struct)
            .and_then(|s| field(s, "MapBuildDataId"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && !id.bytes().all(|b| b == b'0'));
        let Some(id) = id else { continue };
        let Some(mesh) = o.prop("StaticMesh").and_then(|v| string(o.cache(), v)) else { continue };
        if !mesh.starts_with(BASIC_SHAPES) {
            continue;
        }
        let actor = actor_of(o).unwrap_or_default();
        let m = meshes.get(id);
        let kg = key_guid()?;
        let Some(m) = m else { return Err(fail(format!("{actor}: MapBuildData {id} missing"))) };
        let (Some(l), Some(sh)) = (m.get("LightMap"), m.get("ShadowMap")) else {
            return Err(fail(format!("{actor}: MapBuildData {id} holds no lightmap and shadow map")));
        };
        let at = |v: &Json, k: &str| v.get(k).cloned().unwrap_or(Json::Null);
        if l.get("VirtualTextures").and_then(Json::as_array).is_some_and(|a| a.iter().any(json_truthy)) {
            return Err(fail(format!("{actor}: virtual texture lightmap")));
        }
        let slice2 = |k: &str| -> Vec<Vec<Json>> {
            l.get(k).and_then(Json::as_array).map(|a| a.iter().take(2).map(v4).collect()).unwrap_or_default()
        };
        let guids = at(sh, "LightGuids");
        let key_channel = match (&kg, guids.as_array()) {
            (Some(kg), Some(a)) => {
                a.iter().position(|g| g.as_str() == Some(kg.as_str())).map_or(-1, |i| i64::try_from(i).unwrap_or(-1))
            }
            _ => -1,
        };
        lightmaps.insert(
            actor,
            Lightmap {
                lightmap: LightmapTexture {
                    path: l.get("Textures").and_then(|t| t.get(0)).cloned().unwrap_or(Json::Null),
                },
                scale: slice2("ScaleVectors"),
                add: slice2("AddVectors"),
                coord_scale: v2(&at(l, "CoordinateScale")),
                coord_bias: v2(&at(l, "CoordinateBias")),
                shadow: Shadow {
                    path: at(sh, "Texture"),
                    coord_scale: v2(&at(sh, "CoordinateScale")),
                    coord_bias: v2(&at(sh, "CoordinateBias")),
                    channel_valid: at(sh, "bChannelValid"),
                    inv_penumbra: v4(&at(sh, "InvUniformPenumbraSize")),
                    light_guids: guids,
                    key_channel,
                },
            },
        );
    }
    for m in &st.meshes {
        let c =
            objects.iter().find(|o| cls(o) == "StaticMeshComponent" && actor_of(o).as_deref() == Some(m.name.as_str()));
        let ch = c.and_then(|c| c.prop("LightingChannels")).and_then(Value::as_struct);
        let ok = ch.is_some_and(|ch| {
            matches!(field(ch, "bChannel0"), Some(Value::Bool(false)))
                && channel(Some(ch), "bChannel1")
                && !channel(Some(ch), "bChannel2")
        });
        if !ok {
            return Err(fail(format!(
                "{}: stage mesh not on lighting channel 1 alone, the stage renderer lights it by the Key only",
                m.name
            )));
        }
    }
    let mut names: BTreeSet<&str> = BTreeSet::new();
    for m in st.materials.values() {
        names.extend(m.textures.values().map(String::as_str));
        names.extend(parent_textures(&m.parent));
    }
    let mut textures = IndexMap::new();
    for t in names {
        let path = format!("{STAGE_TEXTURES}{t}.{t}");
        let tex = cx.object(&path)?;
        if matches!(tex.prop("SRGB"), Some(Value::Bool(false))) {
            return Err(fail(format!("{t}: SRGB false, the stage renderer samples it as sRGB")));
        }
        textures.insert(t.to_owned(), path);
    }
    Ok(StageLightValue { shapes: SHAPES.into_iter().collect(), lightmaps, textures })
}

/// The `stagelight` kind.
pub struct StageLight;

impl Kind for StageLight {
    fn id(&self) -> &'static str {
        "stagelight"
    }

    fn version(&self) -> u64 {
        STAGELIGHT_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/json"
    }

    /// Each stage's entry, in the stages' order (`exported(game::stagelight_entry_path(key))`).
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(stages(cx)?.keys.iter().map(|k| Item::new(exported(&stagelight_entry_path(k)))).collect())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let all = stages(cx)?;
        let key = all
            .keys
            .iter()
            .find(|k| exported(&stagelight_entry_path(k)) == item.path)
            .ok_or_else(|| KindError::NotFound(format!("stage-light-import: no stage for {}", item.path)))?;
        let v = bind_stage_light(cx, key)?;
        crate::json::to_vec(&v).map_err(|e| KindError::Failed(e.to_string()))
    }

    fn reads(&self) -> &'static [(&'static str, u64)] {
        &[GAMEDATA]
    }
}
