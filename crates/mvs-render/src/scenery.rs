//! The stage's scenery: the scenery kind's entry read into what the renderer draws
//! (static and skeletal mesh components, foliage, landscape, decals, the level's Emitter actors, the blast border).

pub mod batcave;
mod blocks;
mod border;
pub mod cromulons;
pub mod host;
pub mod parallax;
pub mod platform_blend;
mod run;
pub mod scoobys_haunted_mansion;
pub mod sky_arena;
pub mod the_court;
pub mod throne_room;
pub mod tree_fort;
pub mod trophys_edge;
pub mod tutorial;
pub mod widget;

use std::collections::HashMap;
use std::sync::Arc;

use mvs_material::jsonbin::JsonBin;
use mvs_view::mesh::MeshInfo;
use mvs_view::{MaterialRef, MeshPose};
use serde::Deserialize;

pub use self::blocks::SceneryBlock;
pub use self::border::BlastBorder;
use self::border::{BlastBorderEntry, blast_border};
pub use self::host::{
    InstanceId, NullSound, PopSpec, SceneryAnim, SceneryHost, SceneryMatch, SkinRef, SkinnedId, SoundSink, stage_actor,
};
pub use self::run::{SceneryLink, SceneryRun};
pub use self::widget::{WidgetPaint, WidgetQuad, WorldWidget};
use crate::Result;
use crate::draw::LightmapTextures;
use crate::ism::SceneryFoliage;
use crate::landscape::SceneryLandscape;
use crate::renderer::{StaticLightmap, StaticMesh};
use crate::stage::{bad, channel_mask, stage_key, to_world, transform};
use crate::stagedraw::StageDraw;
use crate::view::LightmapParams;

#[derive(Deserialize)]
struct SceneryHeader {
    scenery: Scenery,
    /// The particle systems the level's emitters and blocks spawn, by build object path.
    #[serde(default)]
    systems: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Scenery {
    lightmaps: Vec<String>,
    meshes: HashMap<String, SceneryMesh>,
    instances: Vec<Instance>,
    #[serde(default)]
    skeletal: Vec<Skeletal>,
    /// The InstancedFoliageActors' components (the instanced static mesh's).
    #[serde(default)]
    foliage: Vec<SceneryFoliage>,
    /// The level's landscapes (the landscape's).
    landscape: Option<SceneryLandscape>,
    /// The levels' Emitter actors: the particle player's (Fx::begin_level), not drawn here.
    #[serde(default)]
    emitters: Vec<LevelEmitter>,
    /// The blast box's drawn edge (BlastBoxPlayerExit_C's border meshes).
    blast_border: Option<BlastBorderEntry>,
    /// The levels' DecalActors (the decal pass's).
    #[serde(default)]
    decals: Vec<StageDecal>,
    /// The instances that receive the decals (by index in `instances`); present with `decals`.
    #[serde(default)]
    decal_receivers: Vec<u32>,
    /// The entry's other keys, in the file's order: the stage's blocks (blocks.rs) and those this reads nowhere.
    #[serde(flatten)]
    rest: indexmap::IndexMap<String, serde_json::Value>,
}

/// A level's DecalActor: its material's draw reference (`?ctx=decal`), its world
/// place (cm, rotation in degrees) and the decal box's half extents (DecalSize 128 x 256 x 256 by the scale).
#[derive(Clone, Debug, Deserialize)]
pub struct StageDecal {
    pub name: String,
    pub material: String,
    pub loc: [f64; 3],
    pub rot: [f64; 3],
    pub half: [f64; 3],
}

/// A level's Emitter actor: its ParticleSystemComponent's template at its place (cm,
/// rotation in degrees), its InstanceParameters and TranslucencySortPriority.
#[derive(Clone, Debug, Deserialize)]
pub struct LevelEmitter {
    pub name: String,
    pub system: String,
    pub loc: [f64; 3],
    pub rot: [f64; 3],
    pub scale: [f64; 3],
    /// InstanceParameters by name, in the component's order.
    #[serde(default)]
    pub params: indexmap::IndexMap<String, EmitterParam>,
    #[serde(default)]
    pub sort: i32,
}

/// An Emitter actor's instance parameter: PSPT_Scalar or PSPT_Vector.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum EmitterParam {
    Scalar(f64),
    Vector([f64; 3]),
}

/// A lightmap texture set by its (lightmap, shadow map) texture indexes, shared by the scenery's meshes.
pub type LightmapCache = HashMap<(usize, Option<usize>), Arc<LightmapTextures>>;

/// The scenery entry as the stage takes it: the static and skeletal meshes as StaticMeshes, and what the foliage,
/// the landscape, the decal pass and the particle player draw from it.
#[derive(Debug)]
pub struct SceneryOut {
    pub statics: Vec<StaticMesh>,
    pub landscape: Option<SceneryLandscape>,
    pub foliage: Vec<SceneryFoliage>,
    pub decals: Vec<StageDecal>,
    pub emitters: Vec<LevelEmitter>,
    /// The lightmap and shadow map textures' paths, by a lightmap's `tex`.
    pub lightmaps: Vec<String>,
    /// The meshes' paths by the entry's key (a foliage component's `mesh`).
    pub meshes: HashMap<String, String>,
    /// The entry's body (the foliage's instance rows).
    pub body: Vec<u8>,
    /// The lightmap textures made for the statics, for the foliage's and the landscape's.
    pub lightmap_cache: LightmapCache,
    /// The blast box's drawn edge.
    pub blast_border: Option<BlastBorder>,
    /// The entry's instances and skeletal meshes as the file has them, and its other keys (the stage's blocks).
    pub instances: Vec<Instance>,
    pub skeletal: Vec<Skeletal>,
    pub blocks: indexmap::IndexMap<String, serde_json::Value>,
    /// Per static of `statics`, what it draws of the scenery's state.
    pub links: Vec<SceneryLink>,
    /// Each skeletal mesh's mesh (its reference skeleton).
    pub skeletal_meshes: Vec<Arc<MeshInfo>>,
}

#[derive(Deserialize)]
struct SceneryMesh {
    path: String,
}

/// A static mesh component of the entry, at its UE place (cm, rotation in degrees).
#[derive(Clone, Debug, Deserialize)]
pub struct Instance {
    /// The actor's name and the component's (`<actor>.<component>`).
    pub name: String,
    pub mesh: String,
    pub loc: [f64; 3],
    pub rot: [f64; 3],
    pub scale: [f64; 3],
    pub(crate) materials: Vec<Option<String>>,
    pub(crate) lm: Option<InstanceLightmap>,
    pub(crate) channels: [bool; 3],
    /// Hidden at the start (a Blueprint actor's component its block shows).
    #[serde(default)]
    pub hidden: bool,
    /// The skeletal mesh and bone it is attached to.
    pub attach: Option<serde_json::Value>,
    /// A pass-through platform's index in StageData.platforms, which the sim moves.
    pub platform: Option<u32>,
    /// TranslucencySortPriority.
    pub sort: Option<f64>,
    /// The slots whose material's vertex offset a stage part's parameters enter.
    pub(crate) wpo: Option<serde_json::Value>,
    /// A stage's own fields (its part's: `parallax`, `floating`, `destroyAt`), which its blocks read.
    #[serde(flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// A skeletal mesh component of the entry, at its UE place.
#[derive(Clone, Debug, Deserialize)]
pub struct Skeletal {
    pub name: String,
    pub mesh: String,
    pub loc: [f64; 3],
    pub rot: [f64; 3],
    pub scale: [f64; 3],
    pub(crate) materials: Vec<Option<String>>,
    pub(crate) channels: [bool; 3],
    /// Hidden at the start.
    #[serde(default)]
    pub hidden: bool,
    /// The skeletal mesh and bone it is attached to.
    pub attach: Option<serde_json::Value>,
    /// Its single-node animation (`anim`, `rate`, `loop`, `start`, `savedRate`) and a stage's own fields.
    #[serde(flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// A component's HQ lightmap in the scenery kind: its texture's index in the
/// scenery's `lightmaps`, LightMapCoordinateScaleBias, the scale and add rows, the shadow map.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct InstanceLightmap {
    tex: usize,
    st: [f32; 4],
    scale: [[f32; 4]; 2],
    add: [[f32; 4]; 2],
    shadow: Option<InstanceShadowMap>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstanceShadowMap {
    tex: usize,
    st: [f32; 4],
    masks: [f32; 4],
    inv_penumbra: [f32; 4],
}

/// A material reference of a scenery slot: its object path, the draw context after `?` dropped.
fn material_ref(path: &Option<String>) -> MaterialRef {
    let path = path.as_deref().map_or("", |p| p.split_once('?').map_or(p, |(p, _)| p));
    MaterialRef { path: Arc::from(path), ..Default::default() }
}

/// The sections a skeletal mesh draws: `materials` is one per LOD 0 section, in section order (one
/// skinned mesh per section; a null material, a disabled section, is not drawn).
fn skeletal_shown(info: &MeshInfo, materials: &[Option<String>]) -> Vec<bool> {
    info.sections
        .iter()
        .enumerate()
        .map(|(i, x)| !x.disabled && materials.get(i).is_some_and(Option::is_some))
        .collect()
}

/// A skeletal mesh's `MeshPose.materials`: by slot (a section's `slot`), from the per-section list.
fn skeletal_slot_materials(info: &MeshInfo, materials: &[Option<String>]) -> Vec<MaterialRef> {
    let n = info.sections.iter().map(|x| x.slot + 1).max().unwrap_or(0);
    let mut out = vec![MaterialRef::default(); n];
    for (i, x) in info.sections.iter().enumerate() {
        if let Some(m) = materials.get(i) {
            out[x.slot] = material_ref(m);
        }
    }
    out
}

/// How a scenery slot draws (its DrawRef's `?ctx=`): with the HQ
/// lightmap (`static`), with it and the shadow map (`shadowmap`), or as a movable component (`locallm`, `local`).
/// None: a slot not drawn (no DrawRef).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SlotLight {
    Movable,
    Lightmap,
    ShadowMap,
}

fn slot_light(r: &Option<String>, what: &str) -> Result<Option<SlotLight>> {
    let Some(r) = r else { return Ok(None) };
    let ctx = r.split_once("?ctx=").map(|(_, c)| c).ok_or_else(|| bad(what, format!("{r}: no draw context")))?;
    Ok(Some(match ctx {
        "static" => SlotLight::Lightmap,
        "shadowmap" => SlotLight::ShadowMap,
        "locallm" | "local" => SlotLight::Movable,
        _ => return Err(bad(what, format!("{r}: a draw context the scenery does not draw"))),
    }))
}

/// The FMatrix3x4 of identity: a bone in its reference pose.
const IDENTITY_3X4: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// The level's Emitter actors of the stage `map` (MapData or map name), read without a renderer (a match's particle
/// player spawns them before its first step). Each one's `system` is given as its build object path from the
/// header's `systems` (the level's systems by path, an emitter's by its short name), the path
/// the particle player loads a system outside its index by (VfxPlayer.addSystems).
pub fn level_emitters(store: &mvs_store::Store, map: &str) -> Result<Vec<LevelEmitter>> {
    let draw = StageDraw::load(store, &stage_key(map))?;
    let entry = &draw.scenery.entry;
    let bytes = store.kind("scenery")?.bytes(entry)?;
    let h: SceneryHeader = JsonBin::split(b"MVSC", &bytes, entry)?.parse(entry)?;
    // (the package's last part)
    let short = |path: &str| -> String {
        let package = path.split('.').next().unwrap_or(path);
        package[package.rfind('/').map_or(0, |k| k + 1)..].to_string()
    };
    let mut emitters = h.scenery.emitters;
    for e in &mut emitters {
        if let Some(path) = h.systems.iter().find(|p| short(p) == e.system) {
            e.system.clone_from(path);
        }
    }
    Ok(emitters)
}

/// The sound events the scenery of the stage `map` names and posts, read without a renderer (what
/// [`Renderer::scenery_sound_events`] gives once the stage is set).
pub fn sound_events(store: &mvs_store::Store, map: &str) -> Result<Vec<String>> {
    let draw = StageDraw::load(store, &stage_key(map))?;
    let entry = &draw.scenery.entry;
    let bytes = store.kind("scenery")?.bytes(entry)?;
    let h: SceneryHeader = JsonBin::split(b"MVSC", &bytes, entry)?.parse(entry)?;
    Ok(run::block_sound_events(&h.scenery.rest))
}

/// The scenery entry's static mesh components: each placed with its slots' materials; one
/// whose materials draw with the `static` or `shadowmap` context takes its HQ lightmap
/// (and the shadowmap context its shadow map too); a `locallm` or `local` one draws as a movable component, its
/// lightmap unread. Then its skeletal mesh components, a section with no material not drawn. Each one, hidden and
/// attached ones included, is linked to the scenery's state (SceneryRun::apply: shown, placed, posed), and a
/// stage's block changes that state.
// Unverified: an instance receives the decals when the scenery has none or lists it in `decalReceivers`.
pub(crate) fn scenery(assets: &crate::Assets, entry: &str) -> Result<SceneryOut> {
    let pack = assets.store().kind("scenery")?;
    let bytes = pack.bytes(entry)?;
    let bin = JsonBin::split(b"MVSC", &bytes, entry)?;
    let h: SceneryHeader = bin.parse(entry)?;
    let mut s = h.scenery;
    // (the tutorial level's solid instances, one per live solid: scenery/tutorial.rs)
    tutorial::clone_solids(&mut s.instances, s.rest.get("tutorialSolids"));
    let border = blast_border(&s, entry)?;
    let mut lightmaps: LightmapCache = HashMap::new();
    let mut out = Vec::new();
    let mut links = Vec::new();
    for (k, i) in s.instances.iter().enumerate() {
        let receives_decals = s.decals.is_empty() || s.decal_receivers.contains(&(k as u32));
        // (a pass-through platform's instance is moved per frame by the World's platforms: Renderer::scenery_platforms
        // and platforms_at.) Deviation: the vertex offset is the cooked vertex shader's, where each
        // material's was ported by hand, so the `wpo` slots' values are not read.
        let _ = &i.wpo;
        let what = format!("{entry}: {}", i.mesh);
        let path = &s.meshes.get(&i.mesh).ok_or_else(|| bad(entry, format!("no mesh {}", i.mesh)))?.path;
        let mesh = assets.mesh(path)?;
        let slots = i.materials.iter().map(|m| slot_light(m, &what)).collect::<Result<Vec<_>>>()?;
        // Each slot's light as its DrawRef has it: one StaticMesh per kind, showing that kind's sections.
        let sections = &mesh.lods.first().ok_or_else(|| bad(&what, "no LOD"))?.sections;
        let of = |k: usize| slots.get(sections[k].material as usize).copied().flatten();
        for kind in [SlotLight::Movable, SlotLight::Lightmap, SlotLight::ShadowMap] {
            let shown: Vec<bool> = (0..sections.len()).map(|k| of(k) == Some(kind)).collect();
            if !shown.contains(&true) {
                continue;
            }
            let pose = MeshPose {
                mesh: Arc::from(path.as_str()),
                to_world: to_world(&transform(i.loc, i.rot, i.scale)),
                materials: i.materials.iter().map(material_ref).collect(),
                shown,
                ..Default::default()
            };
            let channels = channel_mask(&i.channels);
            let lightmap = match kind {
                SlotLight::Movable => None,
                _ => {
                    let lm = i.lm.as_ref().ok_or_else(|| bad(&what, "a lightmapped slot without a lightmap"))?;
                    Some(scenery_lightmap(
                        assets,
                        &s.lightmaps,
                        &mut lightmaps,
                        lm,
                        kind == SlotLight::ShadowMap,
                        &what,
                    )?)
                }
            };
            let sort = i.sort.unwrap_or(0.0) as f32;
            links.push(SceneryLink::Instance { id: k, shown: pose.shown.clone() });
            out.push(StaticMesh { pose, lightmap, channels, instances: None, receives_decals, sort });
        }
    }
    let mut skeletal_meshes = Vec::new();
    for (n, k) in s.skeletal.iter().enumerate() {
        let mesh = assets.mesh(&k.mesh)?;
        let info = MeshInfo::shared(assets.store(), &k.mesh)?;
        let sections = &mesh.lods.first().ok_or_else(|| bad(&k.mesh, "no LOD"))?.sections;
        // ("is not a skeletal mesh")
        if info.skeleton.is_none() {
            return Err(bad(&k.mesh, "not a skeletal mesh (no skeleton)"));
        }
        if info.sections.len() != sections.len() || info.inv_ref.len() != mesh.bones as usize {
            return Err(bad(&k.mesh, "its header's sections or bones are not its LOD 0's"));
        }
        // (one skinned mesh per LOD 0 section; a null material, a disabled section, is not drawn: The
        // Court's surface-lit Mystery Machine windows)
        let shown = skeletal_shown(&info, &k.materials);
        let materials = skeletal_slot_materials(&info, &k.materials);
        links.push(SceneryLink::Skeletal { k: n, shown: shown.clone() });
        skeletal_meshes.push(info);
        out.push(StaticMesh {
            pose: MeshPose {
                mesh: Arc::from(k.mesh.as_str()),
                to_world: to_world(&transform(k.loc, k.rot, k.scale)),
                bones: vec![IDENTITY_3X4; mesh.bones as usize],
                materials,
                shown,
                ..Default::default()
            },
            lightmap: None,
            channels: channel_mask(&k.channels),
            instances: None,
            receives_decals: true,
            sort: 0.0,
        });
    }
    Ok(SceneryOut {
        statics: out,
        landscape: s.landscape,
        foliage: s.foliage,
        decals: s.decals,
        emitters: s.emitters,
        lightmaps: s.lightmaps,
        meshes: s.meshes.into_iter().map(|(k, m)| (k, m.path)).collect(),
        body: bin.body.to_vec(),
        lightmap_cache: lightmaps,
        blast_border: border,
        instances: s.instances,
        skeletal: s.skeletal,
        blocks: s.rest,
        links,
        skeletal_meshes,
    })
}

/// A lightmapped slot's StaticLightmap: the HQ lightmap, and the shadow map for the `shadowmap` context.
pub(crate) fn scenery_lightmap(
    assets: &crate::Assets,
    lightmaps: &[String],
    cache: &mut LightmapCache,
    lm: &InstanceLightmap,
    shadow_map: bool,
    what: &str,
) -> Result<StaticLightmap> {
    let shadow = match (&lm.shadow, shadow_map) {
        (Some(sh), true) => Some(sh),
        (None, true) => return Err(bad(what, "the shadowmap context without its shadow map")),
        _ => None,
    };
    let key = (lm.tex, shadow.map(|sh| sh.tex));
    let textures = match cache.get(&key) {
        Some(t) => t.clone(),
        None => {
            let tex = |k: usize| {
                let p = lightmaps.get(k).ok_or_else(|| bad(what, format!("no lightmap texture {k}")))?;
                assets.texture(p)
            };
            let t = tex(lm.tex)?;
            // The sky occlusion and AO mask registers take the lightmap texture: the pass never samples them
            // (View.SkyLightApplyPrecomputedBentNormalShadowingFlag 0).
            let t = Arc::new(LightmapTextures {
                lightmap: Some(t.clone()),
                sky_occlusion: Some(t.clone()),
                ao_material_mask: Some(t),
                static_shadow: shadow.map(|sh| tex(sh.tex)).transpose()?,
            });
            cache.insert(key, t.clone());
            t
        }
    };
    Ok(StaticLightmap { params: lightmap_params(lm, shadow_map), textures, shadow_map })
}

/// A component's lightmap rows (FPrecomputedLightingUniformParameters): the HQ lightmap's, the shadow map's for
/// `shadow_map`.
pub(crate) fn lightmap_params(lm: &InstanceLightmap, shadow_map: bool) -> LightmapParams {
    let shadow = lm.shadow.as_ref().filter(|_| shadow_map);
    LightmapParams {
        static_shadow_map_masks: shadow.map_or([1.0; 4], |sh| sh.masks),
        inv_uniform_penumbra_sizes: shadow.map_or([0.0; 4], |sh| sh.inv_penumbra),
        lightmap_coordinate_scale_bias: lm.st,
        shadow_map_coordinate_scale_bias: shadow.map_or([0.0; 4], |sh| sh.st),
        lightmap_scale: lm.scale,
        lightmap_add: lm.add,
    }
}

// (FRichCurve::Eval: mvs-view curves.rs, where the actors' timelines read it too)
pub use mvs_view::curves::{CurveKey, rich_curve};

#[cfg(test)]
mod tests {
    use super::*;

    /// rich_curve on each mode, the ends and no keys.
    #[test]
    fn rich_curve_modes() {
        assert_eq!(rich_curve(&[], 1.0), 0.0);
        let lin = [[0.0, 0.0, 0.0, 0.0, 0.0], [2.0, 4.0, 0.0, 0.0, 0.0]];
        assert_eq!(rich_curve(&lin, -1.0), 0.0);
        assert_eq!(rich_curve(&lin, 1.0), 2.0);
        assert_eq!(rich_curve(&lin, 3.0), 4.0);
        let con = [[0.0, 1.0, 0.0, 0.0, 2.0], [2.0, 4.0, 0.0, 0.0, 0.0]];
        assert_eq!(rich_curve(&con, 1.9), 1.0);
        // flat tangents: the smoothstep 3x^2 - 2x^3 between the values
        let cub = [[0.0, 0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 0.0, 0.0, 1.0]];
        assert!((rich_curve(&cub, 0.25) - 0.15625).abs() < 1e-12);
    }

    /// Every scenery entry of the developer's store reads with its emitters, decals, foliage and landscape typed;
    /// The Court has its 31 Emitter actors and its landscape, Throne Room its four DecalActors.
    #[test]
    fn scenery_entries_read() {
        let dir = mvs_store::dev_store_dir();
        let Ok(store) = mvs_store::Store::open(&dir) else {
            eprintln!("no store at {}: nothing to check", dir.display());
            return;
        };
        let Ok(pack) = store.kind("scenery") else {
            eprintln!("the store at {} has no scenery kind", dir.display());
            return;
        };
        let (mut read, mut seen) = (0, 0);
        for path in pack.paths() {
            let bytes = pack.bytes(path).unwrap();
            let h: SceneryHeader = JsonBin::split(b"MVSC", &bytes, path).unwrap().parse(path).unwrap();
            let s = h.scenery;
            if path.to_ascii_lowercase().contains("m010") {
                assert_eq!(s.emitters.len(), 31, "{path}");
                assert!(s.landscape.is_some(), "{path}");
                seen += 1;
            }
            if path.to_ascii_lowercase().contains("m008") {
                assert_eq!(s.decals.len(), 4, "{path}");
                seen += 1;
            }
            read += 1;
        }
        eprintln!("{read} scenery entries read");
        assert_eq!(seen, 2, "The Court and Throne Room");
    }

    /// Throne Room's M008_Stage_01_SkelMesh has 11 sections on slots 0,1,2,4,5,6,7,8,12,18,20 and its entry's
    /// `materials` one per section: each section shows its own material, by slot in the pose, none hidden.
    #[test]
    fn skeletal_materials_by_section() {
        let dir = mvs_store::dev_store_dir();
        let Ok(store) = mvs_store::Store::open(&dir) else {
            eprintln!("no store at {}: nothing to check", dir.display());
            return;
        };
        let Ok(pack) = store.kind("scenery") else { return };
        let mut checked = false;
        for path in pack.paths().filter(|p| p.to_ascii_lowercase().contains("m008")) {
            let bytes = pack.bytes(path).unwrap();
            let h: SceneryHeader = JsonBin::split(b"MVSC", &bytes, path).unwrap().parse(path).unwrap();
            for k in h.scenery.skeletal.iter().filter(|k| k.mesh.contains("M008_Stage_01_SkelMesh")) {
                let info = MeshInfo::load(&store, &k.mesh).unwrap();
                let slots: Vec<usize> = info.sections.iter().map(|x| x.slot).collect();
                assert_eq!(slots, [0, 1, 2, 4, 5, 6, 7, 8, 12, 18, 20]);
                assert_eq!(k.materials.len(), 11);
                let shown = skeletal_shown(&info, &k.materials);
                let by_slot = skeletal_slot_materials(&info, &k.materials);
                for (i, x) in info.sections.iter().enumerate() {
                    assert_eq!(shown[i], !x.disabled && k.materials[i].is_some(), "section {i}");
                    if let Some(m) = &k.materials[i] {
                        assert_eq!(by_slot[x.slot].path, material_ref(&Some(m.clone())).path, "section {i}");
                    }
                }
                assert!(by_slot[3].path.is_empty(), "slot 3 has no section");
                checked = true;
            }
        }
        assert!(checked, "Throne Room's skeletal mesh");
    }
}
