//! The `stagedraw` kind's reader: per stage, what the stage draws with by object path: its
//! meshes' slot materials, the post-process volumes' blendables, the scenery entry's DrawRefs with the vertex factory
//! each draws with, and the stage's sky, stagelight, lighting and atmosphere entries. A reader only: the GPU side
//! loads what it names.

use serde::Deserialize;

use crate::{Error, Result};

/// The kind's name in the store.
pub const KIND: &str = "stagedraw";

/// The kind's version this reader reads.
pub const KIND_VERSION: u64 = 1;

/// A stage's entry path: `/StageDraw/<key>.<key>`.
pub fn entry_path(key: &str) -> String {
    format!("/StageDraw/{key}.{key}")
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageDraw {
    pub version: u64,
    /// The stage key.
    pub stage: String,
    /// The stage meshes in the stage data's order.
    pub meshes: Vec<StageMesh>,
    /// The shown PostProcessVolumes of the gameplay and lighting levels, in actor order.
    #[serde(rename = "postProcess")]
    pub post_process: Vec<PostProcessVolume>,
    pub scenery: Scenery,
    /// The sky kind's entry.
    pub sky: String,
    /// The stagelight kind's entry.
    pub stagelight: String,
    pub lighting: Lighting,
    /// The atmosphere kind's entry, for a stage with an AtmosphericFog.
    pub atmosphere: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageMesh {
    /// The StaticMeshActor's name.
    pub name: String,
    /// The StaticMesh's object path (the mesh kind's entry).
    pub mesh: String,
    /// Per material slot: the material's object path (UMeshComponent::GetMaterial), None for none.
    pub materials: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostProcessVolume {
    pub name: String,
    /// The level the actor is in.
    pub level: String,
    /// Settings.WeightedBlendables, in order.
    pub blendables: Vec<Blendable>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blendable {
    pub path: String,
    pub weight: f32,
    /// The object's class path (`/Script/Engine.MaterialInstanceConstant`, ...).
    pub class: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenery {
    /// The scenery kind's entry.
    pub entry: String,
    /// Every DrawRef the entry names, once per vertex factory it draws with.
    pub draws: Vec<SceneryDraw>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneryDraw {
    /// The DrawRef: `<material>?ctx=<context>`.
    #[serde(rename = "ref")]
    pub draw_ref: String,
    /// The material's object path.
    pub material: String,
    pub context: String,
    /// The vertex factory type's name; None for a material shader draw (a deferred decal).
    #[serde(rename = "vertexFactory")]
    pub vertex_factory: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lighting {
    /// The MapBuildDataRegistry of each gameplay and lighting level that has one, in level order.
    pub levels: Vec<LevelRegistry>,
    /// The volumetric lightmap registries, in merge order.
    pub vlm: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelRegistry {
    pub level: String,
    pub registry: String,
}

impl StageDraw {
    /// The stage `key`'s entry, from a store whose stagedraw kind this reader reads.
    pub fn load(store: &mvs_store::Store, key: &str) -> Result<StageDraw> {
        let found = store.version(KIND);
        if found != Some(KIND_VERSION) {
            return Err(Error::Format {
                what: KIND.into(),
                why: format!(
                    "the store's kind is version {found:?}, this reader reads {KIND_VERSION} (run the import)"
                ),
            });
        }
        Self::read(&*store.kind(KIND)?, key)
    }

    /// The stage `key`'s entry from the kind's pack.
    pub fn read(pack: &mvs_store::Pack, key: &str) -> Result<StageDraw> {
        let path = entry_path(key);
        let bytes = pack.bytes(&path)?;
        let e = Self::parse(&bytes).map_err(|why| Error::Format { what: path.clone(), why })?;
        if e.stage != key {
            return Err(Error::Format { what: path, why: format!("names the stage {}", e.stage) });
        }
        Ok(e)
    }

    /// An entry's bytes.
    pub fn parse(bytes: &[u8]) -> std::result::Result<StageDraw, String> {
        let e: StageDraw = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if e.version != KIND_VERSION {
            return Err(format!("entry version {}, this reader reads {KIND_VERSION}", e.version));
        }
        Ok(e)
    }

    /// Every material object path the entry names: the meshes' slots, the volumes' material blendables and the
    /// scenery draws', each once, in that order.
    pub fn materials(&self) -> Vec<&str> {
        let meshes = self.meshes.iter().flat_map(|m| m.materials.iter().flatten());
        let volumes = self
            .post_process
            .iter()
            .flat_map(|v| v.blendables.iter().filter(|b| is_material_class(&b.class)).map(|b| &b.path));
        let scenery = self.scenery.draws.iter().map(|d| &d.material);
        let mut out: Vec<&str> = Vec::new();
        for p in meshes.chain(volumes).chain(scenery) {
            if !out.contains(&p.as_str()) {
                out.push(p);
            }
        }
        out
    }
}

/// The material classes the material kind binds (the importer's stage draw materials).
fn is_material_class(c: &str) -> bool {
    matches!(
        c,
        "/Script/Engine.Material" | "/Script/Engine.MaterialInstance" | "/Script/Engine.MaterialInstanceConstant"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"version":1,"stage":"x","meshes":[{"name":"Floor","mesh":"/Game/M.M","materials":["/Game/A.A",null]}],
        "postProcess":[{"name":"PostProcessVolume_2","level":"L","blendables":[{"path":"/Game/O.O","weight":1,"class":"/Script/Engine.MaterialInstanceConstant"}]}],
        "scenery":{"entry":"/Scenery/x.x","draws":[{"ref":"/Game/A.A?ctx=static","material":"/Game/A.A","context":"static","vertexFactory":"FLocalVertexFactory"},
        {"ref":"/Game/D.D?ctx=decal","material":"/Game/D.D","context":"decal","vertexFactory":null}]},
        "sky":"/Sky/x.x","stagelight":"/StageLight/x.x","lighting":{"levels":[{"level":"L","registry":"/Game/R.R"}],"vlm":[]},"atmosphere":null}"#;

    #[test]
    fn parses_an_entry() {
        let e = StageDraw::parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(e.meshes[0].materials, vec![Some("/Game/A.A".to_string()), None]);
        assert_eq!(e.scenery.draws[1].vertex_factory, None);
        assert_eq!(e.materials(), vec!["/Game/A.A", "/Game/O.O", "/Game/D.D"]);
        assert!(StageDraw::parse(SAMPLE.replace("\"version\":1", "\"version\":2").as_bytes()).is_err());
    }

    /// The developer's store's Lab and The Court entries read, and every material they name is a material entry.
    #[test]
    fn reads_the_store() {
        let dir = mvs_store::dev_store_dir();
        let Ok(store) = mvs_store::Store::open(&dir) else {
            eprintln!("no store at {}: nothing to check", dir.display());
            return;
        };
        if store.version(KIND) != Some(KIND_VERSION) {
            eprintln!("the store at {} has no stagedraw kind {KIND_VERSION} (run the import)", dir.display());
            return;
        }
        let materials = store.kind("material").unwrap();
        for key in ["trainingroomsmall", "m010"] {
            let e = StageDraw::load(&store, key).unwrap();
            for m in e.materials() {
                assert!(materials.has(m), "{key}: {m} is no material entry");
            }
        }
        let lab = StageDraw::load(&store, "trainingroomsmall").unwrap();
        assert!(!lab.meshes.is_empty() && !lab.post_process.is_empty());
    }
}
