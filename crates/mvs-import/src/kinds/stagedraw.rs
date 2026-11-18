//! The `stagedraw` kind: per stage of the stage manifest, what the native renderer draws the stage with by object
//! path (JSON): the stage meshes and their slot materials, the post process volumes'
//! blendables, the scenery entry's DrawRefs with the vertex factory each is drawn with, and the stage's entries of the
//! sky, stagelight, lighting and atmosphere kinds. Read by `mvs-render`
//! (stagedraw.rs). The material kind lists the materials its entries name ([`stage_draw_materials`]).
//!
//! Reads the stages from the store's gamedata kind and each stage's scenery entry from the store's scenery kind; the
//! levels, meshes and blendables through `Cx::object` (`stage_levels.rs`).
//! Its reads of the sky, stagelight and atmosphere kinds take nothing from their packs: the paths it writes of their
//! entries hold their versions, so it is rebuilt after them.

use indexmap::IndexMap;
use mvs_data::{StageData, StageManifest};
use mvs_material::draw::parse_draw_ref;
use mvs_uasset::value::Value;
use serde::{Deserialize, Serialize};

use crate::game::{
    ATMOSPHERE, GAMEDATA, SCENERY, SKY, STAGEDRAW_KIND_VERSION, STAGELIGHT, exported, scenery_entry_path,
    sky_entry_path, stagedraw_path, stagelight_entry_path, stages,
};
use crate::kind::{Cx, Item, Kind, KindError};
use crate::kinds::atmosphere::atmosphere_entry;
use crate::objview::{field, num, short_name, string, truthy};
use crate::stage_levels::{StageLevels, level_built_data, stage_levels};

/// One stage's entry (`StageDrawEntry`), its members in the order they are written.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageDrawEntry {
    pub version: u64,
    /// The stage key.
    pub stage: String,
    pub meshes: Vec<StageDrawMesh>,
    pub post_process: Vec<StageDrawVolume>,
    pub scenery: StageDrawScenery,
    pub sky: String,
    pub stagelight: String,
    pub lighting: StageDrawLighting,
    pub atmosphere: Option<String>,
}

/// A stage mesh (`StageDrawMesh`): its actor's name, its StaticMesh's object path and per material slot the
/// material's object path (`None` for none).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageDrawMesh {
    pub name: String,
    pub mesh: String,
    pub materials: Vec<Option<String>>,
}

/// A post process volume (`StageDrawVolume`): its name, the level it is in (below MAPS_ROOT) and its blendables.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageDrawVolume {
    pub name: String,
    pub level: String,
    pub blendables: Vec<StageDrawBlendable>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageDrawBlendable {
    pub path: String,
    pub weight: f64,
    pub class: String,
}

/// The stage's scenery entry and every DrawRef it names, once per vertex factory it is drawn with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageDrawScenery {
    pub entry: String,
    pub draws: Vec<StageDrawRef>,
}

/// A DrawRef with its parts and the vertex factory it is drawn with (`None`: a material shader draw).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageDrawRef {
    #[serde(rename = "ref")]
    pub draw_ref: String,
    pub material: String,
    pub context: String,
    pub vertex_factory: Option<String>,
}

/// The lighting kind's registries of the gameplay and lighting levels that have one, and the manifest's volumetric
/// lightmap registries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageDrawLighting {
    pub levels: Vec<StageDrawLevel>,
    pub vlm: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageDrawLevel {
    pub level: String,
    pub registry: String,
}

/// The material object paths an entry names, in its order (`stageDrawMaterials`: the material kind's list): the stage
/// meshes' slots and the volumes' blendables of a material class. The scenery draws' materials reach the material
/// kind through the scenery kind's DrawRefs already.
pub fn stage_draw_materials(bytes: &[u8]) -> Result<Vec<String>, KindError> {
    let e: StageDrawEntry =
        serde_json::from_slice(bytes).map_err(|e| KindError::Failed(format!("stagedraw: an entry: {e}")))?;
    let mut out = Vec::new();
    for m in e.meshes {
        out.extend(m.materials.into_iter().flatten().filter(|p| !p.is_empty()));
    }
    for v in e.post_process {
        for b in v.blendables {
            if matches!(
                b.class.as_str(),
                "/Script/Engine.Material"
                    | "/Script/Engine.MaterialInstance"
                    | "/Script/Engine.MaterialInstanceConstant"
            ) {
                out.push(b.path);
            }
        }
    }
    Ok(out)
}

/// A foliage component's draws: FFoliageInstancedStaticMeshSceneProxy over FInstancedStaticMeshVertexFactory (4.26
/// source).
// Unverified: the cooked shader maps of the foliage materials name the factory; the class is not read from the exe.
const FOLIAGE_VF: &str = "FInstancedStaticMeshVertexFactory";

/// A JSON value as the scenery header holds it: what `sceneryDraws` walks (strings, arrays, objects), an object's
/// members in the order `JSON.parse` gives them (array-index keys first, ascending).
enum Json {
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
    Other,
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Json, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Json;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON value")
            }
            fn visit_str<E>(self, v: &str) -> Result<Json, E> {
                Ok(Json::Str(v.to_owned()))
            }
            fn visit_bool<E>(self, _: bool) -> Result<Json, E> {
                Ok(Json::Other)
            }
            fn visit_i64<E>(self, _: i64) -> Result<Json, E> {
                Ok(Json::Other)
            }
            fn visit_u64<E>(self, _: u64) -> Result<Json, E> {
                Ok(Json::Other)
            }
            fn visit_f64<E>(self, _: f64) -> Result<Json, E> {
                Ok(Json::Other)
            }
            fn visit_unit<E>(self) -> Result<Json, E> {
                Ok(Json::Other)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> Result<Json, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element()? {
                    v.push(x);
                }
                Ok(Json::Arr(v))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut a: A) -> Result<Json, A::Error> {
                let mut v: Vec<(String, Json)> = Vec::new();
                while let Some((k, x)) = a.next_entry::<String, Json>()? {
                    match v.iter_mut().find(|e| e.0 == k) {
                        Some(e) => e.1 = x,
                        None => v.push((k, x)),
                    }
                }
                // an integer index key (a canonical array index) comes first, ascending, as a JS object orders them
                let index = |k: &str| k.parse::<u32>().ok().filter(|&n| n != u32::MAX && n.to_string() == k);
                v.sort_by_key(|e| index(&e.0).map_or((1, 0), |n| (0, n)));
                Ok(Json::Obj(v))
            }
        }
        d.deserialize_any(V)
    }
}

/// The scenery entry's header members the draws are read from (`SceneryHeader`).
#[derive(Deserialize)]
struct SceneryHeader {
    materials: Vec<String>,
    effects: Vec<String>,
    decals: Vec<String>,
    scenery: Json,
}

/// The scenery entry's DrawRefs, each once per vertex factory it draws with: a ref under the `foliage` part draws with
/// the instanced factory, any other with its context's (`sceneryDraws`).
fn scenery_draws(h: &SceneryHeader) -> Vec<StageDrawRef> {
    let mut out: IndexMap<String, StageDrawRef> = IndexMap::new();
    fn add(out: &mut IndexMap<String, StageDrawRef>, r: &str, foliage: bool) {
        let Some(d) = parse_draw_ref(r) else { return };
        let vertex_factory = if foliage { Some(FOLIAGE_VF) } else { d.context.vertex_factory };
        let k = format!("{r} {}", vertex_factory.unwrap_or("null"));
        out.entry(k).or_insert_with(|| StageDrawRef {
            draw_ref: r.to_owned(),
            material: d.material.to_owned(),
            context: d.context.name.to_owned(),
            vertex_factory: vertex_factory.map(str::to_owned),
        });
    }
    fn walk(out: &mut IndexMap<String, StageDrawRef>, v: &Json, foliage: bool) {
        match v {
            Json::Str(s) => add(out, s, foliage),
            Json::Arr(a) => a.iter().for_each(|x| walk(out, x, foliage)),
            Json::Obj(o) => o.iter().for_each(|(_, x)| walk(out, x, foliage)),
            Json::Other => {}
        }
    }
    if let Json::Obj(parts) = &h.scenery {
        for (k, v) in parts {
            walk(&mut out, v, k == "foliage");
        }
    }
    // (the header's lists: a ref no part of the scenery names, a part's block material, keeps its context's)
    for r in h.materials.iter().chain(&h.effects).chain(&h.decals) {
        if !out.values().any(|d| d.draw_ref == *r) {
            add(&mut out, r, false);
        }
    }
    out.into_values().collect()
}

/// The stage's meshes as the stage data names them, each with its mesh and slot materials (`stageMeshes`).
fn stage_meshes(
    cx: &Cx<'_>,
    key: &str,
    data: &StageData,
    levels: &StageLevels<'_>,
) -> Result<Vec<StageDrawMesh>, KindError> {
    let fail = |m: String| KindError::Failed(format!("stagedraw {key}: {m}"));
    let mut out = Vec::new();
    for sm in &data.meshes {
        let found: Vec<_> = levels
            .actors(false)
            .filter(|a| short_name(a.path()) == sm.name && short_name(a.class()) == "StaticMeshActor")
            .collect();
        if found.len() != 1 {
            return Err(fail(format!("{} StaticMeshActors named {}", found.len(), sm.name)));
        }
        let c = *levels
            .comps(found[0], Some("StaticMeshComponent"))?
            .first()
            .ok_or_else(|| fail(format!("{} has no StaticMeshComponent", sm.name)))?;
        let mesh = c
            .prop("StaticMesh")
            .and_then(|v| string(c.cache(), v))
            .ok_or_else(|| fail(format!("{} names no StaticMesh", sm.name)))?;
        // Unverified: the slot rule. UMeshComponent::GetMaterial: the component's OverrideMaterials entry, else
        // UStaticMesh::GetMaterial's StaticMaterials entry
        let mesh_obj = cx.object(&mesh)?;
        let own: &[Value] = match mesh_obj.prop("StaticMaterials") {
            Some(Value::Array(a)) => a,
            _ => &[],
        };
        let over: &[Value] = match c.prop("OverrideMaterials") {
            Some(Value::Array(a)) => a,
            _ => &[],
        };
        // UStaticMesh::GetNumMaterials: the mesh's own slots; an override past them draws nothing
        let materials = (0..own.len())
            .map(|i| {
                let o = over.get(i).filter(|v| truthy(v));
                let m = o.or_else(|| own[i].as_struct().and_then(|s| field(s, "MaterialInterface")));
                m.and_then(|v| string(c.cache(), v)).filter(|s| !s.is_empty())
            })
            .collect();
        out.push(StageDrawMesh { name: sm.name.clone(), mesh, materials });
    }
    Ok(out)
}

/// One stage's entry (`bindStageDraw`).
fn bind_stage_draw(
    cx: &Cx<'_>,
    key: &str,
    manifest: &StageManifest,
    data: &StageData,
) -> Result<StageDrawEntry, KindError> {
    let levels = stage_levels(cx, key, &manifest.levels)?;
    let meshes = stage_meshes(cx, key, data, &levels)?;
    let mut post_process = Vec::new();
    for a in levels.actors(false) {
        // Unverified: the volume rule is a hidden volume skipped, with bEnabled,
        // bUnbound, Priority, BlendWeight and BlendRadius not read.
        if short_name(a.class()) != "PostProcessVolume" || a.prop("bHidden").is_some_and(truthy) {
            continue;
        }
        let wb = a
            .prop("Settings")
            .and_then(Value::as_struct)
            .and_then(|s| field(s, "WeightedBlendables"))
            .and_then(Value::as_struct);
        let mut blendables = Vec::new();
        if let Some(Value::Array(arr)) = wb.and_then(|w| field(w, "Array")) {
            for b in arr {
                let Some(b) = b.as_struct() else { continue };
                let Some(path) = field(b, "Object").and_then(|v| string(a.cache(), v)) else { continue };
                let weight = field(b, "Weight").and_then(num).unwrap_or(1.0);
                let class = cx.object(&path)?.class().to_owned();
                blendables.push(StageDrawBlendable { path, weight, class });
            }
        }
        post_process.push(StageDrawVolume {
            name: short_name(a.path()).to_owned(),
            level: levels.level_of(a)?.level.clone(),
            blendables,
        });
    }
    // (every stage of the manifest has a scenery entry)
    let scenery_path = exported(&scenery_entry_path(key));
    let bytes = cx.store.entry(SCENERY.0, &scenery_path)?;
    let what = format!("scenery {scenery_path}");
    let header: SceneryHeader = mvs_material::jsonbin::JsonBin::split(b"MVSC", &bytes, &what)
        .and_then(|e| e.parse(&what))
        .map_err(|e| KindError::Failed(e.to_string()))?;
    let scenery = StageDrawScenery { entry: scenery_path, draws: scenery_draws(&header) };
    let mut lighting_levels = Vec::new();
    for lv in &levels.levels {
        if lv.role != "gameplay" && lv.role != "lighting" {
            continue;
        }
        if let Some(registry) = level_built_data(&lv.objects) {
            lighting_levels.push(StageDrawLevel { level: lv.level.clone(), registry });
        }
    }
    Ok(StageDrawEntry {
        version: STAGEDRAW_KIND_VERSION,
        stage: key.to_owned(),
        meshes,
        post_process,
        scenery,
        sky: exported(&sky_entry_path(&manifest.sky)),
        stagelight: exported(&stagelight_entry_path(key)),
        lighting: StageDrawLighting { levels: lighting_levels, vlm: manifest.vlm.clone() },
        atmosphere: match data.atmosphere {
            Some(_) => Some(atmosphere_entry(&levels)?.ok_or_else(|| {
                KindError::Failed(format!("stagedraw-import: {key}: an atmosphere and no AtmosphericFogComponent"))
            })?),
            None => None,
        },
    })
}

/// The `stagedraw` kind.
pub struct StageDraw;

impl Kind for StageDraw {
    fn id(&self) -> &'static str {
        "stagedraw"
    }

    fn version(&self) -> u64 {
        STAGEDRAW_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/json"
    }

    /// Each stage's entry, in the stages' order (`game::stagedraw_path(key)`).
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(stages(cx)?.keys.iter().map(|k| Item::new(stagedraw_path(k))).collect())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let st = stages(cx)?;
        let key = st.keys.iter().find(|k| stagedraw_path(k) == item.path).ok_or_else(|| {
            KindError::NotFound(format!("stagedraw-import: no stage of the manifest for {}", item.path))
        })?;
        let entry = st.get(key)?;
        let e = bind_stage_draw(cx, key, &entry.manifest, &entry.data)?;
        crate::json::to_vec(&e).map_err(|e| KindError::Failed(e.to_string()))
    }

    fn reads(&self) -> &'static [(&'static str, u64)] {
        // (sky, stagelight and atmosphere: their entries' paths, which hold their versions)
        &[GAMEDATA, SCENERY, SKY, STAGELIGHT, ATMOSPHERE]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenery_refs_once_per_factory() {
        let h: SceneryHeader = serde_json::from_str(
            r#"{"materials":["/G/A.A?ctx=static","/G/B.B?ctx=decal"],"effects":[],"decals":[],
            "scenery":{"foliage":{"m":["/G/A.A?ctx=static"]},"2":"/G/C.C?ctx=local","1":{"x":"/G/A.A?ctx=static"},
            "n":"plain"}}"#,
        )
        .unwrap();
        let d = scenery_draws(&h);
        let got: Vec<(&str, Option<&str>)> =
            d.iter().map(|r| (r.draw_ref.as_str(), r.vertex_factory.as_deref())).collect();
        // index keys first ("1", "2"), then the members in order; the foliage ref has its own factory; the decal
        // listed only in `materials` follows with its context's (none)
        assert_eq!(
            got,
            [
                ("/G/A.A?ctx=static", Some("FLocalVertexFactory")),
                ("/G/C.C?ctx=local", Some("FLocalVertexFactory")),
                ("/G/A.A?ctx=static", Some("FInstancedStaticMeshVertexFactory")),
                ("/G/B.B?ctx=decal", None),
            ]
        );
    }

    #[test]
    fn materials_of_an_entry() {
        let e = br#"{"version":1,"stage":"M","meshes":[{"name":"Floor","mesh":"/Engine/BasicShapes/Cube.Cube",
            "materials":["/Game/A.A",null,"/Game/B.B"]}],"postProcess":[{"name":"V","level":"M/Map_M","blendables":[
            {"path":"/Game/P.P","weight":1,"class":"/Script/Engine.MaterialInstanceConstant"},
            {"path":"/Game/L.L","weight":0.5,"class":"/Script/Engine.LightPropagationVolumeBlendable"}]}],
            "scenery":{"entry":"/Scenery/x/M.M","draws":[{"ref":"/Game/S.S?ctx=static","material":"/Game/S.S",
            "context":"static","vertexFactory":"FLocalVertexFactory"}]},"sky":"/Sky/s.s","stagelight":"/StageLight/l.l",
            "lighting":{"levels":[{"level":"M/Map_M","registry":"/Game/R.R"}],"vlm":[]},"atmosphere":null}"#;
        assert_eq!(stage_draw_materials(e).unwrap(), ["/Game/A.A", "/Game/B.B", "/Game/P.P"]);
        let back: StageDrawEntry = serde_json::from_slice(e).unwrap();
        let again = crate::json::to_vec(&back).unwrap();
        assert!(String::from_utf8(again).unwrap().starts_with(r#"{"version":1,"stage":"M","meshes":"#));
    }
}
