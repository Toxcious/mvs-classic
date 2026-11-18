//! A stage's scenery entry, bound from its levels.
//! The core the parts hook in by; the parts are [`super::parts`].
//!
//! What it reads: the stage's manifest (its levels and their roles) and data (the lights, for the forward lights rule
//! of the material choice; the pass-through platforms' drawn meshes), each level's World, its _BuiltData registry
//! (the lighting kind's header: lightmaps, shadow maps, the foliage's per-instance biases), the meshes' headers and,
//! for each material it names, whether its entry is built (`materials.rs`: an entry refused is never named).
//!
//! The scenery (`mvs-render` stage.rs), less what other kinds hold (the particle systems: the particles kind; the
//! materials: the material kind):
//! - `levels`: the scenery levels (the MapData's LabLevelsToLoad that are not its gameplay, lighting or audio level).
//! - `lightmaps`: the lightmap and shadow map textures (the levels' _BuiltData registries' inner objects), by build
//!   object path; `meshes`: name -> { path } of each static mesh drawn (a mesh by its asset name, a second asset of
//!   the same name in another folder by its folder and name).
//! - `instances`: every StaticMeshComponent of a StaticMeshActor that is not hidden (the actor's bHidden, the
//!   component's bVisible false or bHiddenInGame), in the levels' order (the scenery levels, then the gameplay level,
//!   then the lighting levels), then the gameplay level's pass-through platforms' meshes (`platform`: the index in the
//!   stage data's platforms, which moves it). The gameplay and lighting levels give what they show that is not the
//!   stage's: a shown StaticMeshActor whose mesh is not one of the engine's BasicShapes (those are the stage data's),
//!   or a BasicShape with a material not of Test_Gym's; the foliage; the components of the Blueprint classes a part
//!   enters. Per instance its mesh, place (UE: loc, rot as pitch / yaw / roll degrees, scale), materials (DrawRefs
//!   per slot, null for a slot not drawn), lightmap (`lm`: the HQ lightmap texture's index in `lightmaps`,
//!   LightMapCoordinateScaleBias, LightMapScale / Add (the high quality pair), and with a shadow map its texture, its
//!   ShadowMapCoordinateScaleBias, StaticShadowMapMasks (bChannelValid) and InvUniformPenumbraSize), lighting
//!   channels, CastShadow, Mobility, TranslucencySortPriority (`sort`) and the parts' fields. A StaticMeshActor's mesh
//!   attached to another actor's component (Tree Fort's Trees2 / Trees3) is placed at its world transform up the
//!   chain, `attachedTo` the actor its chain ends at; one a part's Blueprint attaches at BeginPlay
//!   (`SceneryPart::begin_attach`) likewise by the attachment rules.
//! - `foliage`: each FoliageInstancedStaticMeshComponent of an InstancedFoliageActor with instances (`foliage_of`).
//! - `landscape`: a scenery level's landscapes (`landscape.rs`).
//! - the parts' level blocks, then `skeletal` (the SkeletalMeshActors and the parts' skeletal meshes: `add_skeletal`),
//!   then the parts' actor blocks.
//! - `emitters`: the levels' Emitter actors (a template at a transform; the parts' too, `add_emitter`), by the
//!   system's name (its package's): the header's `systems` holds their paths.
//! - `blastBorder`: the blast box's drawn edge (`border.rs`).
//! - `decals`, `decalReceivers`: the levels' deferred decals.
//!
//! Every stage has an entry: a map with no scenery level still draws its gameplay level's blast border, platforms and
//! shown meshes (the Training Rooms, Classic 3 Platform).
//! Deviation: the sky occlusion textures are not written (the sky light casts no shadows: the shader's bent normal
//! branch is off); the lightmaps' lower mips are not used.
//! Deviation: an instance's `bounds` (the Primitive rows a material reads) and `wpo` (the slots whose vertex offset a
//! part enters) are not written: they were decided from translated shaders, which the native renderer does not use, and
//! it reads neither.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};

use super::SCENERY_MAGIC;
use super::border::blast_border_of;
use super::landscape::landscapes_of;
use super::materials::{SceneryMaterials, bare};
use super::part::{
    InstanceActor, JsonProps, OwnProps, PropsOf, SceneryBeginAttach, SceneryBindCtx, SceneryLevelRun, SceneryPart,
    SceneryPartRun, SceneryPlace, SkeletalOpts, SlotLightmap, StaticMeshOut,
};
use super::parts::util::{
    Xform, as_props, class_of, component_props, json_truthy, name_of, outer_name, props_of, rel_of, rot_of, tags_of,
    vec, world_of, xf_mul, xf_out,
};
use super::parts::{SceneryRegistry, registry};
use crate::game::{MAPS_ROOT, SCENERY_BIND_VERSION, STAGE_BIND_VERSION, stages};
use crate::json::Json;
use crate::jsonbin::{json_bin, lay_out_body};
use crate::kind::{Cx, KindError, Memo};
use crate::kinds::lighting::{LIGHTING_KIND_VERSION, entry_header};
use crate::kinds::material::resolve::resolve_material;
use crate::kinds::mesh::{self, MESH_KIND_VERSION, MeshHeader};
use crate::kinds::object::OBJECT_KIND_VERSION;
use crate::objview::{ObjView, field, string};
use crate::stage_levels::{StageLevel, level_built_data};

/// The entry's layout.
pub const SCENERY_FORMAT_VERSION: u64 = 1;

const BASIC_SHAPES: &str = "/Engine/BasicShapes/";
/// The stage materials' folder.
const STAGE_MATERIALS: &str = "/Game/Panda_Main/Maps/Test_Gym/Materials/";

// The levels' deferred decals whose material's opacity is 0 on every pixel in the build, so a decal of theirs adds
// nothing and is left out (drawing them cost 9,781 blended triangles a frame for Batcave). Spotlight_Decal_MAT / _MAT2
// (Batcave's 12 spotlights): base Materials whose FDeferredDecalPS opacity is the edge fade x the decal fade x
// saturate(the spotlight textures' lerp x Intensity x a flicker), Intensity a scalar parameter of default 0 that no
// instance, MID or Blueprint sets.
const ZERO_OPACITY_DECALS: [&str; 2] = ["Spotlight_Decal_MAT", "Spotlight_Decal_MAT2"];

// A FoliageInstancedStaticMeshComponent's properties that are read (StaticMesh, AttachParent, Mobility,
// LightingChannels, CastShadow) or read and left (foliage_of); a property not listed fails.
const FOLIAGE_PROPS: [&str; 21] = [
    "StaticMesh",
    "AttachParent",
    "Mobility",
    "LightingChannels",
    "CastShadow",
    "bReceivesDecals",
    "SortedInstances",
    "NumBuiltInstances",
    "BuiltInstanceBounds",
    "CacheMeshExtendedBounds",
    "InstanceCountToRender",
    "InstancingRandomSeed",
    "InstanceReorderTable",
    "OverriddenLightMapRes",
    "bHasPerInstanceHitProxies",
    "bAffectDynamicIndirectLighting",
    "bAffectDistanceFieldLighting",
    "bHasCustomNavigableGeometry",
    "VisibilityId",
    "BodyInstance",
    "StreamingTextureData",
];

const EMITTER_PROPS: [&str; 10] = [
    "Template",
    "bOldPositionValid",
    "OldPosition",
    "RelativeLocation",
    "RelativeRotation",
    "RelativeScale3D",
    "PrimaryComponentTick",
    "TranslucencySortPriority",
    "InstanceParameters",
    "LightingChannels",
];

const DECAL_PROPS: [&str; 5] =
    ["DecalMaterial", "RelativeLocation", "RelativeRotation", "RelativeScale3D", "AttachParent"];

/// A component's lightmap as the entry writes it (`lm`), and whether it has a shadow map.
#[derive(Clone, Debug)]
pub struct SceneryLm {
    pub json: Json,
    pub shadow: bool,
}

/// A package's object path of its main export (`/Game/A/B.B`), whatever the case of the reference's export name.
pub fn main_path(p: &str) -> String {
    let pkg = &p[..p.find('.').unwrap_or(p.len())];
    format!("{pkg}.{}", &pkg[pkg.rfind('/').map_or(0, |i| i + 1)..])
}

/// A package key (`Panda_Main/...`, `Engine/Content/...`: the stage data's platform meshes) as its main export's
/// object path.
fn key_path(k: &str) -> String {
    let pkg = match k.strip_prefix("Engine/Content/") {
        Some(rest) => format!("/Engine/{rest}"),
        None => format!("/Game/{k}"),
    };
    let name = &pkg[pkg.rfind('/').map_or(0, |i| i + 1)..];
    format!("{pkg}.{name}")
}

/// A package's name.
fn pkg_name(p: &str) -> &str {
    let pkg = &p[..p.find('.').unwrap_or(p.len())];
    &pkg[pkg.rfind('/').map_or(0, |i| i + 1)..]
}

/// The path of the object an inner object is under.
fn parent_path(p: &str) -> &str {
    &p[..p.rfind('.').unwrap_or(p.len())]
}

/// The last name of a class reference (after its last `.`, `/` or `:`).
fn last_name(s: &str) -> &str {
    &s[s.rfind(['.', '/', ':']).map_or(0, |i| i + 1)..]
}

/// A member set, as `??` takes it (not null, not absent).
fn set<'p>(p: &'p JsonProps, k: &str) -> Option<&'p Json> {
    p.get(k).filter(|v| !matches!(v, Json::Null))
}

fn truthy(p: &JsonProps, k: &str) -> bool {
    p.get(k).is_some_and(json_truthy)
}

/// `p[k] === false`.
fn is_false(p: &JsonProps, k: &str) -> bool {
    matches!(p.get(k), Some(Json::Bool(false)))
}

fn nums(v: &[f64]) -> Json {
    Json::Arr(v.iter().map(|x| Json::Num(*x)).collect())
}

fn refs_json(refs: &[Option<String>]) -> Json {
    Json::Arr(refs.iter().map(|r| r.clone().map_or(Json::Null, Json::Str)).collect())
}

/// A component's LightingChannels (ULightComponent / UPrimitiveComponent defaults: channel 0 only).
fn channels_of(q: &JsonProps) -> Vec<bool> {
    let ch = q.get("LightingChannels");
    let at = |k: &str, d: bool| ch.and_then(|c| c.get(k)).filter(|v| !matches!(v, Json::Null)).map_or(d, json_truthy);
    vec![at("bChannel0", true), at("bChannel1", false), at("bChannel2", false)]
}

fn bools(v: &[bool]) -> Json {
    Json::Arr(v.iter().map(|b| Json::Bool(*b)).collect())
}

/// An array property of object paths (null entries kept).
fn object_paths(v: Option<&Json>) -> Vec<Option<String>> {
    match v {
        Some(Json::Arr(a)) => a.iter().map(|x| x.as_str().map(str::to_owned)).collect(),
        _ => Vec::new(),
    }
}

/// `{ ...into, ...from }` in place: a key already there keeps its place.
fn spread(into: &mut JsonProps, from: JsonProps) {
    for (k, v) in from {
        into.insert(k, v);
    }
}

#[derive(Default)]
struct Built(Memo<Result<Arc<IndexMap<String, Json>>, KindError>>);

/// A _BuiltData registry's meshes' build data (the lighting kind's header `meshes`), by MapBuildDataId, once per run.
fn built_meshes(cx: &Cx<'_>, registry: &str) -> Result<Arc<IndexMap<String, Json>>, KindError> {
    cx.shared.get_or_init(Built::default).0.get(registry, || {
        let h = entry_header(cx, registry)?;
        let meshes = h.get("meshes").cloned().unwrap_or(serde_json::Value::Null);
        let j: Json = serde_json::from_value(meshes)
            .map_err(|e| KindError::Failed(format!("scenery: lighting {registry}: {e}")))?;
        match j {
            Json::Obj(m) => Ok(Arc::new(m)),
            Json::Null => Ok(Arc::default()),
            _ => Err(KindError::Failed(format!("scenery: lighting {registry}: meshes is no object"))),
        }
    })
}

/// A decal to write once the levels are read.
struct DecalPlace {
    name: String,
    material: String,
    loc: [f64; 3],
    rot: [f64; 3],
    scale: [f64; 3],
}

/// One stage's bind state (the bind's locals), behind interior mutability: the parts' hooks reach it through
/// [`LevelCtx`] while the core loops.
struct Bind<'c, 'a> {
    cx: &'c Cx<'a>,
    key: String,
    reg: &'static SceneryRegistry,
    mats: SceneryMaterials<'c, 'a>,
    /// name -> path
    meshes: RefCell<IndexMap<String, String>>,
    /// path -> name
    mesh_names: RefCell<HashMap<String, String>>,
    lightmaps: RefCell<IndexSet<String>>,
    /// name -> path
    systems: RefCell<IndexMap<String, String>>,
    emitters: RefCell<Vec<Json>>,
    skeletal: RefCell<Vec<Json>>,
    instances: RefCell<Vec<JsonProps>>,
    foliage: RefCell<Vec<Json>>,
    rows: RefCell<Vec<u8>>,
    landscape: RefCell<Option<Json>>,
    decal_places: RefCell<Vec<DecalPlace>>,
    no_decals: RefCell<IndexSet<String>>,
    /// The DrawRefs named (the header's lists), in the order first named.
    named: RefCell<IndexSet<String>>,
    /// The parts' runs, in the order their first class is met.
    runs: RefCell<IndexMap<&'static str, Box<dyn SceneryPartRun<'a> + 'a>>>,
}

impl<'c, 'a> Bind<'c, 'a> {
    fn fail(&self, msg: impl std::fmt::Display) -> KindError {
        KindError::Failed(format!("scenery {}: {msg}", self.key))
    }

    /// A static mesh, by its main export's object path: its name in `meshes`.
    fn add_mesh(&self, r: &str) -> Result<String, KindError> {
        let p = main_path(r);
        if let Some(had) = self.mesh_names.borrow().get(&p) {
            return Ok(had.clone());
        }
        let mut meshes = self.meshes.borrow_mut();
        let mut name = pkg_name(&p).to_owned();
        if meshes.contains_key(&name) {
            let dir = &p[..p.rfind('/').unwrap_or(0)];
            name = format!("{}_{name}", &dir[dir.rfind('/').map_or(0, |i| i + 1)..]);
        }
        if meshes.contains_key(&name) {
            return Err(self.fail(format!("two meshes named {name}")));
        }
        self.mesh_names.borrow_mut().insert(p.clone(), name.clone());
        meshes.insert(name.clone(), p);
        Ok(name)
    }

    fn mesh_header(&self, r: &str) -> Result<Arc<MeshHeader>, KindError> {
        mesh::header(self.cx, &main_path(r))
    }

    // LOD 0's sections and the slots: a slot no LOD 0 section draws with is null (the cloud meshes' second slot is
    // their LOD 1's: an opaque unlit M011_CloudsLow instance below a screen size of 0.1, which no cloud of the level
    // reaches with the match camera's field of view), else its override, else the mesh's material.
    fn slots_of(&self, r: &str, over: &[Option<String>]) -> Result<Vec<Option<String>>, KindError> {
        let h = self.mesh_header(r)?;
        let Some(lod) = h.lods.first() else { return Err(self.fail(format!("{r}: no LOD 0"))) };
        if h.lightmap_uv.unwrap_or(0.0) >= 8.0 {
            return Err(self.fail(format!("{r}: lightmap UV channel {:?}", h.lightmap_uv)));
        }
        Ok(h.slots
            .iter()
            .enumerate()
            .map(|(k, def)| {
                if lod.sections.iter().any(|x| x.slot == k as f64) {
                    over.get(k).cloned().flatten().or_else(|| def.material.clone())
                } else {
                    None
                }
            })
            .collect())
    }

    /// A particle system, by its package's name (two of one name fail).
    fn add_system(&self, r: &str) -> Result<String, KindError> {
        let p = main_path(r);
        let n = pkg_name(&p).to_owned();
        let mut systems = self.systems.borrow_mut();
        if systems.get(&n).is_some_and(|had| *had != p) {
            return Err(self.fail(format!("two systems named {n}")));
        }
        systems.insert(n.clone(), p);
        Ok(n)
    }

    fn name(&self, refs: &[Option<String>]) {
        let mut named = self.named.borrow_mut();
        for r in refs.iter().flatten() {
            named.insert(r.clone());
        }
    }

    /// An animation a scenery mesh plays, by build object path (the store's animation kind, whose entry holds its
    /// RateScale and bForceRootLock / RootMotionRootLock: the renderer locks the root as the build does). Root motion
    /// is refused (the renderer extracts none for a scenery mesh) unless the caller's block takes it out itself
    /// (`root_motion`: Trophy's crate montages).
    fn anim_path(&self, anim: &str, root_motion: bool) -> Result<String, KindError> {
        let a = self.cx.object(&main_path(anim))?;
        if truthy(&props_of(&a), "bEnableRootMotion") && !root_motion {
            return Err(self.fail(format!("{anim}: root motion")));
        }
        Ok(main_path(anim))
    }

    fn skeleton_of(&self, p: &str) -> Result<Option<Json>, KindError> {
        Ok(props_of(&self.cx.object(&main_path(p))?).get("Skeleton").cloned())
    }

    fn with_run<R>(
        &self,
        part: &'static dyn SceneryPart,
        f: impl FnOnce(&mut (dyn SceneryPartRun<'a> + 'a)) -> R,
    ) -> R {
        let mut runs = self.runs.borrow_mut();
        let r = runs.entry(part.name()).or_insert_with(|| part.run());
        f(r.as_mut())
    }
}

/// What the parts' hooks get of one level and the level's own lookups.
struct LevelCtx<'b, 'c, 'a> {
    b: &'b Bind<'c, 'a>,
    lv: &'b StageLevel<'a>,
    by_path: HashMap<&'b str, &'b ObjView<'a>>,
    /// The level's actors' paths.
    actors: HashSet<&'b str>,
    built: Arc<IndexMap<String, Json>>,
}

impl<'b, 'c, 'a> LevelCtx<'b, 'c, 'a> {
    /// A lightmap or shadow map texture (an inner object of the level's _BuiltData registry): its index in
    /// `lightmaps`.
    fn lm_texture(&self, p: Option<&Json>) -> Result<usize, KindError> {
        let Some(p) = p.and_then(Json::as_str).filter(|s| !s.is_empty()) else {
            return Err(self.b.fail(format!("{}: a lightmap with no texture", self.lv.level)));
        };
        let mut l = self.b.lightmaps.borrow_mut();
        Ok(l.insert_full(p.to_owned()).0)
    }

    fn lightmap_by_id(&self, what: &str, id: Option<&str>) -> Result<Option<SceneryLm>, KindError> {
        let Some(b) = id.and_then(|id| self.built.get(id)) else { return Ok(None) };
        let Some(l) = b.get("LightMap").filter(|l| json_truthy(l)) else { return Ok(None) };
        let sh = b.get("ShadowMap").filter(|s| json_truthy(s));
        if l.get("VirtualTextures").and_then(Json::as_array).is_some_and(|a| a.iter().any(json_truthy)) {
            return Err(self.b.fail(format!("{what}: virtual texture lightmap")));
        }
        let v4 = |v: &Json| {
            Json::Arr(["X", "Y", "Z", "W"].iter().map(|k| v.get(k).cloned().unwrap_or(Json::Null)).collect())
        };
        let st = |o: &Json| {
            let at = |s: &str, k: &str| o.get(s).and_then(|v| v.get(k)).cloned().unwrap_or(Json::Null);
            Json::Arr(vec![
                at("CoordinateScale", "X"),
                at("CoordinateScale", "Y"),
                at("CoordinateBias", "X"),
                at("CoordinateBias", "Y"),
            ])
        };
        let two =
            |k: &str| Json::Arr(l.get(k).and_then(Json::as_array).unwrap_or(&[]).iter().take(2).map(v4).collect());
        let tex = self.lm_texture(l.get("Textures").and_then(|t| t.as_array()).and_then(|t| t.first()))?;
        let mut out = JsonProps::new();
        out.insert("tex".into(), Json::Num(tex as f64));
        out.insert("st".into(), st(l));
        out.insert("scale".into(), two("ScaleVectors"));
        out.insert("add".into(), two("AddVectors"));
        if let Some(sh) = sh {
            let masks = sh.get("bChannelValid").and_then(Json::as_array).unwrap_or(&[]);
            let masks = Json::Arr(masks.iter().map(|m| Json::Num(if json_truthy(m) { 1.0 } else { 0.0 })).collect());
            out.insert(
                "shadow".into(),
                Json::object([
                    ("tex", Json::Num(self.lm_texture(sh.get("Texture"))? as f64)),
                    ("st", st(sh)),
                    ("masks", masks),
                    ("invPenumbra", v4(sh.get("InvUniformPenumbraSize").unwrap_or(&Json::Null))),
                    ("lights", sh.get("LightGuids").cloned().unwrap_or(Json::Null)),
                ]),
            );
        }
        Ok(Some(SceneryLm { json: Json::Obj(out), shadow: sh.is_some() }))
    }

    fn lightmap_of(&self, e: &ObjView<'a>) -> Result<Option<SceneryLm>, KindError> {
        self.lightmap_by_id(name_of(e), build_data_id(e)?.as_deref())
    }

    fn actor_of(&self, o: &ObjView<'_>) -> Option<&'b ObjView<'a>> {
        let p = parent_path(o.path());
        self.by_path.get(p).copied().filter(|_| self.actors.contains(p))
    }

    // A skeletal mesh component playing one animation (EAnimationMode::AnimationSingleNode;
    // FSingleAnimationPlayData's defaults: looping, playing, rate 1, from 0). An animation of another skeleton than
    // the mesh's is not played (the reference pose: Tree Fort's scaffolding names the boat's Treefort_Boat_Idle).
    // Unverified: from memory of 4.26 UAnimSingleNodeInstance::SetAnimationAsset, which refuses an asset whose
    // skeleton is not the mesh's. The component's OverrideMaterials replace its mesh's slots (a null entry keeps the
    // mesh's).
    // `anim_bp` (the AnimBlueprint class a part read the graph of): the component's own AnimClass
    // (EAnimationMode::AnimationBlueprint, the default; its AnimationData is not read), written with no animation: the
    // reference pose, which the part's block drives; with `play`, the one looping sequence the part read that graph's
    // pose to be. `attach`: [skeletal mesh, bone] it is attached to (`at` its transform under the bone); `hidden`: it
    // starts hidden (a block shows it); `fields`: the part's own fields.
    // The single-node play data (FSingleAnimationPlayData::Initialize: the animation at SavedPosition, SavedPlayRate,
    // bSavedLooping, playing when bSavedPlaying: read in the exe, 142d83450). Unverified: the single node's own tick
    // holding the last frame is 4.26 source, only the AdvanceTime clamp 142816220 is read. One not playing
    // (bSavedPlaying false) is written at rate 0: the animation's pose at SavedPosition, `savedRate` the rate a later
    // Play takes; one not looping (`loop` false) holds its last pose; `start` the SavedPosition it starts from.
    // `paused`: the part says it is not playing (checked). Unverified: a single-node component with no animation is
    // the reference pose (UAnimSingleNodeInstance with no asset, from memory of 4.26). Unverified: the
    // AnimationBlueprint mode with no AnimClass and no PostProcessAnimBlueprint on the mesh has no anim instance, the
    // reference pose (4.26 from memory, not read in the exe).
    fn add_skeletal_impl(
        &self,
        nm: &str,
        q: &JsonProps,
        at: &SceneryPlace,
        op: &SkeletalOpts,
    ) -> Result<Option<String>, KindError> {
        let b = self.b;
        let Some(sk) = q.get("SkeletalMesh").and_then(Json::as_str) else {
            return Err(b.fail(format!("{nm}: no skeletal mesh")));
        };
        let mk = main_path(sk);
        let mesh = props_of(&b.cx.object(&mk)?);
        let h = b.mesh_header(&mk)?;
        let d = q.get("AnimationData").and_then(Json::as_object).cloned().unwrap_or_default();
        let mode = bare(set(q, "AnimationMode").and_then(Json::as_str).unwrap_or("EAnimationMode::AnimationBlueprint"));
        let anim_class = q.get("AnimClass").and_then(Json::as_str).map(last_name);
        let anim_bp = op.anim_bp.as_deref();
        let is_bare = anim_bp.is_none()
            && mode == "AnimationBlueprint"
            && !truthy(q, "AnimClass")
            && !truthy(&d, "AnimToPlay")
            && !truthy(&mesh, "PostProcessAnimBlueprint");
        let single_node = anim_bp.is_none() && mode == "AnimationSingleNode";
        let playing = !is_bare && single_node && !is_false(&d, "bSavedPlaying");
        let single = is_bare
            || match anim_bp {
                Some(bp) => mode == "AnimationBlueprint" && anim_class == Some(bp),
                None => single_node && (!op.paused || !playing),
            };
        if !single || is_false(q, "bVisible") || truthy(q, "bHiddenInGame") {
            return Err(b.fail(format!("{nm}: skeletal mesh component")));
        }
        // (`materials` per LOD 0 section, in section order, as the mesh entry's sections; a LOD 0 LODMaterialMap remaps
        // a section's slot: no built mesh has one, so one is refused, not drawn wrong)
        let lod_info = mesh.get("LODInfo").and_then(Json::as_array).and_then(<[Json]>::first);
        if lod_info.and_then(|l| l.get("LODMaterialMap")).and_then(Json::as_array).is_some_and(|a| !a.is_empty()) {
            return Err(b.fail(format!("{nm}: LOD 0 LODMaterialMap (not taken)")));
        }
        let over = object_paths(q.get("OverrideMaterials"));
        let Some(lod0) = h.lods.first() else { return Err(b.fail(format!("{nm}: no LOD 0"))) };
        let mut materials = Vec::new();
        for x in &lod0.sections {
            let k = x.slot as usize;
            let m = over.get(k).cloned().flatten().or_else(|| h.slots.get(k).and_then(|s| s.material.clone()));
            let Some(m) = m else { return Err(b.fail(format!("{nm}: section of slot {k} with no material"))) };
            materials.push(self.skin_material(&m, nm)?);
        }
        let mut anim = match anim_bp {
            Some(_) => op.play.as_ref().map(|p| main_path(&p.anim)),
            None => d.get("AnimToPlay").and_then(Json::as_str).map(main_path),
        };
        if let Some(a) = &anim
            && b.skeleton_of(a)? != b.skeleton_of(&mk)?
        {
            anim = None;
        }
        let mut s = JsonProps::new();
        s.insert("name".into(), nm.into());
        s.insert("mesh".into(), mk.clone().into());
        if let Some(a) = &anim {
            s.insert("anim".into(), b.anim_path(a, false)?.into());
        }
        let rate = match &op.play {
            Some(p) => Json::Num(p.rate),
            None if single_node && !playing => Json::Num(0.0),
            None => set(&d, "SavedPlayRate").cloned().unwrap_or(Json::Num(1.0)),
        };
        s.insert("rate".into(), rate);
        if single_node {
            if is_false(&d, "bSavedLooping") {
                s.insert("loop".into(), Json::Bool(false));
            }
            if truthy(&d, "SavedPosition") {
                s.insert("start".into(), d["SavedPosition"].clone());
            }
            if let Some(r) = d.get("SavedPlayRate").filter(|_| !playing) {
                s.insert("savedRate".into(), r.clone());
            }
        }
        s.insert("loc".into(), nums(&vec(at.get("RelativeLocation"), [0.0; 3])));
        s.insert("rot".into(), nums(&rot_of(at.get("RelativeRotation"))));
        s.insert("scale".into(), nums(&vec(at.get("RelativeScale3D"), [1.0; 3])));
        s.insert("materials".into(), refs_json(&materials));
        s.insert("channels".into(), bools(&channels_of(q)));
        if let Some(a) = &op.attach {
            s.insert("attach".into(), a.clone());
        }
        if op.hidden {
            s.insert("hidden".into(), Json::Bool(true));
        }
        if let Some(f) = &op.fields {
            spread(&mut s, f.clone());
        }
        b.skeletal.borrow_mut().push(Json::Obj(s));
        Ok(anim)
    }

    // A FoliageInstancedStaticMeshComponent of an InstancedFoliageActor (a UHierarchicalInstancedStaticMeshComponent):
    // its mesh's LOD 0 drawn once per instance of PerInstanceSMData, in that order, as the cooked level stores them.
    // An instance's transform is the cooked FMatrix (the component's native PerInstanceSMData, 16 float32 an instance:
    // rows X, Y, Z axis times the scale, then the translation, in the component's space), written as the three rows
    // of its 3 x 4 matrix (UE axes, column vectors), then the instance's LightmapUVBias: 14 floats an instance at
    // `data` in the rows. The component sits under the actor's root (RootComponent0, its relative transform the
    // component's: the components store none of their own). Its lightmap (`lm`, a component with build data): the
    // component's LightMap with CoordinateScale (the ISM vertex factory's lightmap UV is the mesh's lightmap channel x
    // LightMapCoordinateScaleBias.xy + the instance's LightmapUVBias: the cooked TBasePassVSTLightMapPolicyHQ of the
    // ISM factory, TROPHY_Grass_A_01_Mat) and PerInstanceLightmapData[i] for instance i. Unverified: the per-instance
    // build data is taken in PerInstanceSMData's order (UInstancedStaticMeshComponent::BuildRenderData, from memory of
    // 4.26; the reorder table maps to the render order the GPU buffer uses, which draws the same set). A shadow map or
    // a shadow map bias fails (none here). Read and left: the cluster tree, sorted instances and reorder table (the
    // hierarchical culling and the draw order, no part of the picture with every instance drawn), the bounds caches,
    // the hit proxies, the random seed (PerInstanceRandom, which the four foliage materials' pixel shaders do not
    // read), the lightmap resolution, the nav and collision settings (NoCollision), decals (none on the scenery), the
    // built texture streaming data. No cull distance is stored (0: no cull).
    fn foliage_of(&self, e: &ObjView<'a>) -> Result<Option<Json>, KindError> {
        let b = self.b;
        let fail = |m: String| b.fail(format!("{}: {m}", name_of(e)));
        let native = e.native()?;
        let Some(data) = native.as_ref().and_then(|n| field(n, "PerInstanceSMData")) else { return Ok(None) };
        let mvs_uasset::value::Value::Bytes(data) = data else {
            return Err(fail("PerInstanceSMData is no section".into()));
        };
        let p = props_of(e);
        let other: Vec<&str> = p.keys().map(String::as_str).filter(|k| !FOLIAGE_PROPS.contains(k)).collect();
        if !other.is_empty() {
            return Err(fail(format!("foliage properties {}", other.join(","))));
        }
        let owner = self.actor_of(e);
        let parent = p.get("AttachParent").and_then(Json::as_str).and_then(|a| self.by_path.get(a).copied());
        let placed = match (owner, parent) {
            (Some(owner), Some(parent)) => {
                let op = props_of(owner);
                let pp = props_of(parent);
                (class_of(owner) == "InstancedFoliageActor"
                    && class_of(parent) == "SceneComponent"
                    && op.get("RootComponent").and_then(Json::as_str) == Some(parent.path())
                    && parent_path(parent.path()) == owner.path()
                    && pp
                        .keys()
                        .all(|k| ["RelativeLocation", "RelativeRotation", "RelativeScale3D"].contains(&k.as_str())))
                .then_some((owner, pp))
            }
            _ => None,
        };
        let Some((owner, q)) = placed else { return Err(fail("foliage component placement".into())) };
        if data.len() % 64 != 0 {
            return Err(fail(format!("PerInstanceSMData of {} bytes", data.len())));
        }
        let n = data.len() / 64;
        let lods = match native.as_ref().and_then(|x| field(x, "LODData")) {
            Some(mvs_uasset::value::Value::Array(a)) => a.len(),
            _ => 0,
        };
        let count = |k: &str| p.get(k).and_then(Json::as_f64);
        if count("NumBuiltInstances") != Some(n as f64) || count("InstanceCountToRender") != Some(n as f64) || lods > 1
        {
            return Err(fail("instance counts".into()));
        }
        let Some(sm) = p.get("StaticMesh").and_then(Json::as_str) else { return Err(fail("no mesh".into())) };
        let mesh = b.add_mesh(sm)?;
        let slots = b.slots_of(sm, &[])?;
        let lm = self.lightmap_of(e)?;
        let id = build_data_id(e)?;
        let built = id.as_deref().and_then(|id| self.built.get(id));
        if built.is_some() != lm.is_some() || lm.as_ref().is_some_and(|l| l.shadow) {
            return Err(fail("foliage build data".into()));
        }
        let bias = built.and_then(|x| x.get("PerInstanceLightmapData")).and_then(Json::as_array);
        let shadow_bias = |x: &Json| {
            let s = x.get("ShadowmapUVBias");
            let c = |k: &str| s.and_then(|s| s.get(k)).is_some_and(json_truthy);
            c("X") || c("Y")
        };
        if built.is_some() && bias.is_none_or(|a| a.len() != n || a.iter().any(shadow_bias)) {
            return Err(fail("per-instance lightmap data".into()));
        }
        let mut f = vec![0f32; n * 14];
        let m = |i: usize, r: usize, c: usize| {
            let at = i * 64 + (r * 4 + c) * 4;
            f32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
        };
        for i in 0..n {
            for row in 0..3 {
                for c in 0..4 {
                    f[i * 14 + row * 4 + c] = m(i, c, row);
                }
            }
            if let Some(bias) = bias {
                let uv = bias[i].get("LightmapUVBias");
                let at = |k: &str| uv.and_then(|u| u.get(k)).and_then(Json::as_f64).unwrap_or(0.0);
                f[i * 14 + 12] = at("X") as f32;
                f[i * 14 + 13] = at("Y") as f32;
            }
        }
        let channels = channels_of(&p);
        let slot_lm = lm.as_ref().map(|l| SlotLightmap { shadow: l.shadow });
        let materials = b.mats.build_for(&slots, slot_lm.as_ref(), Some(&channels))?;
        b.name(&materials);
        if materials.iter().zip(&slots).any(|(r, s)| s.is_some() && r.as_ref().is_none_or(|r| b.mats.is_effect(r))) {
            return Err(fail("a foliage material that is not opaque or masked".into()));
        }
        let data_at = {
            let mut rows = b.rows.borrow_mut();
            let at = rows.len();
            for v in &f {
                rows.extend_from_slice(&v.to_le_bytes());
            }
            at
        };
        // (a root at the origin stores no properties: Scooby's Background foliage)
        let mut out = JsonProps::new();
        out.insert("name".into(), format!("{}.{}", name_of(owner), name_of(e)).into());
        out.insert("mesh".into(), mesh.into());
        out.insert("loc".into(), nums(&vec(q.get("RelativeLocation"), [0.0; 3])));
        out.insert("rot".into(), nums(&rot_of(q.get("RelativeRotation"))));
        out.insert("scale".into(), nums(&vec(q.get("RelativeScale3D"), [1.0; 3])));
        out.insert("materials".into(), refs_json(&materials));
        if let Some(lm) = lm {
            out.insert("lm".into(), lm.json);
        }
        out.insert("channels".into(), bools(&channels));
        out.insert("castShadow".into(), set(&p, "CastShadow").cloned().unwrap_or(Json::Bool(true)));
        if let Some(m) = p.get("Mobility").filter(|v| json_truthy(v)).and_then(Json::as_str) {
            out.insert("mobility".into(), bare(m).into());
        }
        out.insert("count".into(), Json::Num(n as f64));
        out.insert("data".into(), Json::Num(data_at as f64));
        Ok(Some(Json::Obj(out)))
    }
}

/// A component's MapBuildDataId (its native LODData[0]'s).
fn build_data_id(e: &ObjView<'_>) -> Result<Option<String>, KindError> {
    let native = e.native()?;
    Ok(native
        .as_ref()
        .and_then(|n| field(n, "LODData"))
        .and_then(|l| match l {
            mvs_uasset::value::Value::Array(a) => a.first(),
            _ => None,
        })
        .and_then(mvs_uasset::value::Value::as_struct)
        .and_then(|s| field(s, "MapBuildDataId"))
        .and_then(mvs_uasset::value::Value::as_str)
        .map(str::to_owned))
}

impl<'a> SceneryBindCtx<'a> for LevelCtx<'_, '_, 'a> {
    fn stage(&self) -> &str {
        &self.b.key
    }
    fn level(&self) -> &StageLevel<'a> {
        self.lv
    }
    fn cx(&self) -> &Cx<'a> {
        self.b.cx
    }
    fn load(&self, path: &str) -> Result<ObjView<'a>, KindError> {
        self.b.cx.object(path)
    }
    fn root_of(&self, owner: &ObjView<'a>) -> Result<JsonProps, KindError> {
        let p = format!("{}.DefaultSceneRoot", owner.path());
        match self.by_path.get(p.as_str()) {
            Some(r) => Ok(props_of(r)),
            None => Err(self.b.fail(format!("{}: no DefaultSceneRoot", owner.path()))),
        }
    }
    fn anim_path(&self, anim: &str, root_motion: bool) -> Result<String, KindError> {
        self.b.anim_path(anim, root_motion)
    }
    fn skin_material(&self, material: &str, name: &str) -> Result<Option<String>, KindError> {
        let r = self.b.mats.skin(material, name)?;
        self.b.name(std::slice::from_ref(&r));
        Ok(r)
    }
    fn add_skeletal(
        &self,
        name: &str,
        component: &JsonProps,
        at: &SceneryPlace,
        opts: &SkeletalOpts,
    ) -> Result<Option<String>, KindError> {
        self.add_skeletal_impl(name, component, at, opts)
    }
    fn add_system(&self, system: &str) -> Result<String, KindError> {
        self.b.add_system(system)
    }
    fn mesh_header(&self, mesh: &str) -> Result<Arc<MeshHeader>, KindError> {
        self.b.mesh_header(mesh)
    }
    fn add_mesh(&self, mesh: &str) -> Result<usize, KindError> {
        let n = self.b.add_mesh(mesh)?;
        Ok(self.b.meshes.borrow().get_index_of(&n).unwrap_or(0))
    }
    // A part's mesh whose material reads the component's bounds (a block's mesh has no component bounds to give) is
    // not refused: that test read translated shaders and is not made.
    fn build_for(
        &self,
        materials: &[Option<String>],
        lightmap: Option<&SlotLightmap>,
        channels: Option<&[bool]>,
    ) -> Result<Vec<Option<String>>, KindError> {
        let refs = self.b.mats.build_for(materials, lightmap, channels)?;
        self.b.name(&refs);
        Ok(refs)
    }
    /// A level's component's place: its relative properties, or with an AttachParent its world transform composed up
    /// the chain as relative properties.
    fn place_of(&self, component: &ObjView<'a>, props: Option<&dyn PropsOf>) -> Result<SceneryPlace, KindError> {
        let pr: &dyn PropsOf = props.unwrap_or(&OwnProps);
        let p = pr.props_of(component)?;
        if p.get("AttachParent").is_some_and(json_truthy) {
            return Ok(as_props(&world_of(&self.lv.objects, component, pr)?));
        }
        Ok(p.into_owned())
    }
    fn add_emitter(&self, emitter: JsonProps) {
        self.b.emitters.borrow_mut().push(Json::Obj(emitter));
    }
    // (a part's instance names its mesh by add_mesh's index; the entry's instances name it by its name in `meshes`)
    fn add_instance(&self, instance: JsonProps) {
        let b = self.b;
        b.name(&object_paths(instance.get("materials")));
        let mut x = instance;
        let mesh = match x.get("mesh") {
            Some(Json::Num(i)) => b.meshes.borrow().get_index(*i as usize).map(|(k, _)| Json::Str(k.clone())),
            Some(m @ Json::Str(_)) => Some(m.clone()),
            _ => None,
        };
        // (a mesh that is no index or name is not met; a part's own code gives one, so this holds)
        x.insert("mesh".into(), mesh.unwrap_or(Json::Null));
        x.insert("hidden".into(), Json::Bool(true));
        b.instances.borrow_mut().push(x);
    }
}

/// The objects of a level under the maps' root, in export order, and its actors (`StageLevel`).
fn load_level<'a>(cx: &Cx<'a>, level: &str, role: &str, li: usize) -> Result<StageLevel<'a>, KindError> {
    let world = cx.object(&format!("{MAPS_ROOT}{level}"))?;
    let objects = world.inner()?;
    let actors = objects
        .iter()
        .enumerate()
        .filter(|(_, o)| o.chain().strip_prefix("PersistentLevel.").is_some_and(|n| !n.is_empty() && !n.contains('.')))
        .map(|(i, _)| i)
        .collect();
    Ok(StageLevel {
        level: level.to_owned(),
        role: role.to_owned(),
        li: i32::try_from(li).expect("a few levels"),
        world,
        objects,
        actors,
    })
}

/// Stage `key`'s scenery entry: the header and the foliage rows as a json-bin container.
pub fn bind(cx: &Cx<'_>, key: &str) -> Result<Vec<u8>, KindError> {
    let all = stages(cx)?;
    let entry = all.get(key).map_err(|_| KindError::NotFound(format!("scenery {key}: not a stage of the manifest")))?;
    let (me, st) = (&entry.manifest, &entry.data);
    let fail = |m: String| KindError::Failed(format!("scenery {key}: {m}"));
    let levels: Vec<String> =
        me.levels.iter().filter(|(_, r)| r == "scenery" || r == "vfx").map(|(l, _)| l.clone()).collect();
    let Some(gameplay) = me.levels.iter().find(|(_, r)| r == "gameplay").map(|(l, _)| l.clone()) else {
        return Err(fail("no gameplay level".into()));
    };
    let lighting: Vec<&String> = me.levels.iter().filter(|(_, r)| r == "lighting").map(|(l, _)| l).collect();
    // the scenery levels, then the gameplay and lighting levels for what the scenery draws of them
    let mut read: Vec<(StageLevel<'_>, bool)> = Vec::new();
    for l in &levels {
        let role = me.levels.iter().find(|x| x.0 == *l).map_or("scenery", |x| x.1.as_str());
        read.push((load_level(cx, l, role, read.len())?, true));
    }
    let gameplay_at = read.len();
    read.push((load_level(cx, &gameplay, "gameplay", read.len())?, false));
    for l in lighting {
        read.push((load_level(cx, l, "lighting", read.len())?, false));
    }

    // Whether a component of these lighting channels has forward lights (the renderer's forward lights: the last
    // dynamic directional in sorted order, the lights using lighting channels last, if it shares a channel; a radial
    // light sharing one), from the stage's data.
    let dirs: Vec<Vec<bool>> =
        st.lights.iter().filter(|l| l.affects_world && l.mobility != "Static").map(|l| l.channels.clone()).collect();
    let radial: Vec<Vec<bool>> = st.radial_lights.iter().flatten().map(|l| l.channels.clone()).collect();
    let fwd_lights_on = move |channels: &[bool]| {
        let shares = |l: &[bool]| l.iter().enumerate().any(|(k, on)| *on && channels.get(k).copied().unwrap_or(false));
        let uses = |l: &[bool]| !(l.first() == Some(&true) && l.get(1) != Some(&true) && l.get(2) != Some(&true));
        let winner = dirs.iter().rfind(|l| uses(l)).or_else(|| dirs.iter().rfind(|l| !uses(l)));
        winner.is_some_and(|w| shares(w)) || radial.iter().any(|l| shares(l))
    };
    let b = Bind {
        cx,
        key: key.to_owned(),
        reg: registry(),
        mats: SceneryMaterials::new(cx, Box::new(fwd_lights_on)),
        meshes: RefCell::default(),
        mesh_names: RefCell::default(),
        lightmaps: RefCell::default(),
        systems: RefCell::default(),
        emitters: RefCell::default(),
        skeletal: RefCell::default(),
        instances: RefCell::default(),
        foliage: RefCell::default(),
        rows: RefCell::default(),
        landscape: RefCell::default(),
        decal_places: RefCell::default(),
        no_decals: RefCell::default(),
        named: RefCell::default(),
        runs: RefCell::default(),
    };
    for (lv, own) in &read {
        bind_level(&b, lv, *own)?;
    }

    // The gameplay level's pass-through platforms (BasePlatformBP_Visual_C): the StaticMesh component of each one that
    // is drawn, as the stage bind writes it in the stage's data (`platforms[i].mesh`), an instance with `platform`: its
    // index there, which the renderer moves with the sim's platform. After the levels' instances. No lightmap: the
    // level's component stores no LODData, so no MapBuildDataId (the stage bind checks it). Unverified: a lit material
    // would take the volumetric lightmap policy here (locallm), unread for a Blueprint component; the one drawn now,
    // TROPHY_MovingPlatform_A_01_M, is unlit (MSM_Unlit, its texture as the emissive), one shader on every policy.
    for (i, pl) in st.platforms.iter().flatten().enumerate() {
        let Some(d) = &pl.mesh else { continue };
        let mp = key_path(&d.mesh);
        let mesh = b.add_mesh(&mp)?;
        let over: Vec<Option<String>> = d.materials.iter().map(|m| (!m.is_empty()).then(|| key_path(m))).collect();
        let slots = b.slots_of(&mp, &over)?;
        let materials = b.mats.build_for(&slots, None, None)?;
        b.name(&materials);
        let mut x = JsonProps::new();
        x.insert("name".into(), format!("{}.StaticMesh", pl.name).into());
        x.insert("mesh".into(), mesh.into());
        x.insert("loc".into(), nums(&d.loc));
        x.insert("rot".into(), nums(&d.rot));
        x.insert("scale".into(), nums(&d.scale));
        x.insert("materials".into(), refs_json(&materials));
        x.insert("channels".into(), bools(&d.channels));
        x.insert("castShadow".into(), Json::Bool(d.cast_shadow));
        x.insert("platform".into(), Json::Num(i as f64));
        b.instances.borrow_mut().push(x);
    }

    let blast_border =
        blast_border_of(cx, &read[gameplay_at].0, &b.mats, &|p: &str| b.add_mesh(p), &|p: &str| b.mesh_header(p))?;
    if let Some(parts) = blast_border.as_ref().and_then(|x| x.get("parts")).and_then(Json::as_array) {
        let refs: Vec<Option<String>> =
            parts.iter().map(|p| p.get("material").and_then(Json::as_str).map(str::to_owned)).collect();
        b.name(&refs);
    }
    let mut decals = Vec::new();
    for d in b.decal_places.borrow().iter() {
        let m = resolve_material(cx, &d.material)?;
        let blend =
            m.prop("DecalBlendMode").and_then(|v| string(cx.cache, v)).unwrap_or_else(|| "DBM_Translucent".into());
        let blend = bare(&blend);
        if blend != "DBM_Emissive" && blend != "DBM_Translucent" {
            return Err(fail(format!("{}: decal blend {blend}", d.name)));
        }
        let material = b.mats.decal(&d.material)?;
        b.name(&[Some(material.clone())]);
        let half: Vec<f64> = [128.0, 256.0, 256.0].iter().zip(d.scale).map(|(v, s)| (v * s).abs()).collect();
        decals.push(Json::object([
            ("name", Json::from(d.name.as_str())),
            ("material", material.into()),
            ("loc", nums(&d.loc)),
            ("rot", nums(&d.rot)),
            ("half", nums(&half)),
        ]));
    }
    let instances = b.instances.take();
    let no_decals = b.no_decals.take();
    let receivers: Vec<Json> = instances
        .iter()
        .enumerate()
        .filter(|(_, x)| {
            !truthy(x, "hidden")
                && !x.contains_key("platform")
                && !x.get("name").and_then(Json::as_str).is_some_and(|n| no_decals.contains(n))
        })
        .map(|(i, _)| Json::Num(i as f64))
        .collect();

    // the parts' blocks: the level Blueprints' after the instances, the Blueprint actors' after the skeletal meshes
    let mut level_blocks = JsonProps::new();
    let mut actor_blocks = JsonProps::new();
    {
        let mut runs = b.runs.borrow_mut();
        for r in runs.values_mut() {
            spread(&mut level_blocks, r.level_blocks()?);
        }
        for r in runs.values_mut() {
            spread(&mut actor_blocks, r.actor_blocks()?);
        }
    }
    let mut scenery = JsonProps::new();
    scenery.insert("levels".into(), Json::Arr(levels.into_iter().map(Json::Str).collect()));
    scenery.insert("lightmaps".into(), Json::Arr(b.lightmaps.take().into_iter().map(Json::Str).collect()));
    scenery.insert(
        "meshes".into(),
        Json::Obj(b.meshes.take().into_iter().map(|(n, p)| (n, Json::object([("path", Json::Str(p))]))).collect()),
    );
    scenery.insert("instances".into(), Json::Arr(instances.into_iter().map(Json::Obj).collect()));
    let foliage = b.foliage.take();
    if !foliage.is_empty() {
        scenery.insert("foliage".into(), Json::Arr(foliage));
    }
    if let Some(l) = b.landscape.take() {
        scenery.insert("landscape".into(), l);
    }
    spread(&mut scenery, level_blocks);
    scenery.insert("skeletal".into(), Json::Arr(b.skeletal.take()));
    spread(&mut scenery, actor_blocks);
    scenery.insert("emitters".into(), Json::Arr(b.emitters.take()));
    if let Some(x) = blast_border {
        scenery.insert("blastBorder".into(), x);
    }
    if !decals.is_empty() {
        scenery.insert("decals".into(), Json::Arr(decals));
        scenery.insert("decalReceivers".into(), Json::Arr(receivers));
    }

    let bin = b.rows.take();
    let (refs, _) = lay_out_body(&[&bin]);
    let named = b.named.take();
    let list =
        |f: &dyn Fn(&str) -> bool| Json::Arr(named.iter().filter(|r| f(r)).map(|r| Json::Str(r.clone())).collect());
    let is_decal = |r: &str| r.ends_with("?ctx=decal");
    let header = Json::object([
        ("version", Json::Num(SCENERY_FORMAT_VERSION as f64)),
        ("stage", Json::from(key)),
        ("bindVersion", Json::Num(SCENERY_BIND_VERSION as f64)),
        ("objectVersion", Json::Num(f64::from(OBJECT_KIND_VERSION))),
        ("stageVersion", Json::Num(STAGE_BIND_VERSION as f64)),
        ("lightingVersion", Json::Num(LIGHTING_KIND_VERSION as f64)),
        ("meshVersion", Json::Num(MESH_KIND_VERSION as f64)),
        ("materials", list(&|r| !b.mats.is_effect(r) && !is_decal(r))),
        ("effects", list(&|r| b.mats.is_effect(r))),
        ("decals", list(&is_decal)),
        ("systems", Json::Arr(b.systems.take().into_values().map(Json::Str).collect())),
        ("scenery", Json::Obj(scenery)),
        (
            "foliage",
            Json::object([("offset", Json::Num(refs[0].offset as f64)), ("length", Json::Num(refs[0].length as f64))]),
        ),
    ]);
    json_bin(SCENERY_MAGIC, &header, &[&bin], &refs).map_err(|e| fail(e.to_string()))
}

/// One level's part of the bind.
fn bind_level<'a>(b: &Bind<'_, 'a>, lv: &StageLevel<'a>, own: bool) -> Result<(), KindError> {
    let lvl = lv.level.as_str();
    let objects = &lv.objects;
    let lfail = |m: String| b.fail(format!("{lvl}: {m}"));
    let built = match level_built_data(objects) {
        Some(r) => built_meshes(b.cx, &r)?,
        None => Arc::default(),
    };
    let ctx = LevelCtx {
        b,
        lv,
        by_path: objects.iter().map(|o| (o.path(), o)).collect(),
        actors: lv.actors.iter().map(|&i| objects[i].path()).collect(),
        built,
    };
    let actor_of = |o: &ObjView<'_>| ctx.actor_of(o);
    // The level's Blueprint actors by the registry: an entered class's actors (path -> its part), the level
    // Blueprint's part, the ignored ones; any other class is a log line alone.
    let mut blueprints: IndexMap<&str, (&ObjView<'a>, &'static dyn SceneryPart)> = IndexMap::new();
    let mut script: Option<(String, &'static dyn SceneryPart)> = None;
    let script_cls = format!("{}_C", &lvl[lvl.rfind('/').map_or(0, |i| i + 1)..]);
    for &i in &lv.actors {
        let a = &objects[i];
        if a.class().starts_with("/Script/") {
            continue;
        }
        let cls = class_of(a);
        let is_script = cls == script_cls;
        let part = if is_script { b.reg.level.get(cls) } else { b.reg.blueprint.get(cls) }.copied();
        // (a gameplay or lighting level's Blueprint is read when a part enters it: Tree Fort's level Blueprints
        // destroy the BlackoutSphere and show a snail)
        match part {
            Some(p) if is_script => script = Some((cls.to_owned(), p)),
            Some(p) => {
                blueprints.insert(a.path(), (a, p));
            }
            None => {}
        }
    }
    // The level Blueprint's own: what it does to the level's actors (the actor tags it reads, the hidden actors it
    // shows).
    let level_run: Option<(SceneryLevelRun, &'static dyn SceneryPart)> = match &script {
        Some((cls, part)) => Some((b.with_run(*part, |r| r.level(cls, &ctx))?, *part)),
        None => None,
    };
    let shows = |actor: &ObjView<'a>| -> Result<bool, KindError> {
        match &level_run {
            Some((_, part)) => b.with_run(*part, |r| r.shows(actor)).map(|o| o.is_some()),
            None => Ok(false),
        }
    };
    // The StaticMeshActors a placed Blueprint actor attaches to one of its components at BeginPlay: actor name -> the
    // attach.
    let mut begun: IndexMap<String, (SceneryBeginAttach<'a>, &ObjView<'a>)> = IndexMap::new();
    for (owner, part) in blueprints.values() {
        for a in part.begin_attach(class_of(owner), &ctx, owner)? {
            if begun.contains_key(&a.actor) {
                return Err(b.fail(format!("{}: attached twice at BeginPlay", a.actor)));
            }
            begun.insert(a.actor.clone(), (a, *owner));
        }
    }

    // Emitter actors: the component's template at its transform; it activates itself (bAutoActivate, the component's
    // default). LowSpecEmitterDestroyer_C destroys the level's emitters at BeginPlay with the visual effect quality at
    // 1 or lower or a mobile feature level: neither here. The particle component of a Blueprint actor a part enters is
    // the part's (its particle_system hook, which writes it with add_emitter or keeps it for its block; one it does
    // not take fails). A gameplay or lighting level's are read for a class whose part names it in gameplay_particles
    // alone (Batcave's cars, BatcaveWallDestructible_C, in the gameplay level).
    for e in objects {
        if class_of(e) != "ParticleSystemComponent" {
            continue;
        }
        let bp = blueprints.get(parent_path(e.path()));
        if !own && !bp.is_some_and(|(o, _)| b.reg.gameplay_particles.contains(class_of(o))) {
            continue;
        }
        if let Some((owner, part)) = bp {
            if !b.with_run(*part, |r| r.particle_system(class_of(owner), &ctx, owner, e))? {
                return Err(lfail(format!("particle system component {} of {}", name_of(e), class_of(owner))));
            }
            continue;
        }
        let Some(owner) = actor_of(e).filter(|o| class_of(o) == "Emitter") else {
            return Err(lfail(format!("particle system component of {}", outer_name(e))));
        };
        let q = props_of(e);
        let op = props_of(owner);
        // (a component saved not visible, or its actor hidden, draws nothing: Scooby No Roof's
        // PS_Finn_CoinSparkles_12 and PS_Mansion_ChestEyes_4, which no graph of the stage names)
        if is_false(&q, "bVisible") || truthy(&op, "bHidden") {
            continue;
        }
        // (TranslucencySortPriority, `sort`: Trophy's waterfall emitters, -3, the renderer's translucent sort key.
        // Deviation: LightingChannels, The Court's waterfall splashes, are read and left; the renderer's effects light
        // no particle by channel)
        // InstanceParameters (Tree Fort night's emitters: a vector "Color"): the component's named parameters, which a
        // template's parameter distributions read by name: `params`, a vector as its three values, a scalar as one.
        // Another kind (an actor, a material, a random range) fails.
        let mut params = JsonProps::new();
        for ip in q.get("InstanceParameters").and_then(Json::as_array).unwrap_or(&[]) {
            let t = bare(ip.get("ParamType").and_then(Json::as_str).unwrap_or(""));
            let plain = !ip.get("Actor").is_some_and(json_truthy) && !ip.get("Material").is_some_and(json_truthy);
            let n = ip.get("Name").and_then(Json::as_str).unwrap_or("undefined").to_owned();
            if t == "PSPT_Vector" && plain {
                params.insert(n, nums(&vec(ip.get("Vector"), [0.0; 3])));
            } else if t == "PSPT_Scalar" && plain {
                if let Some(s) = ip.get("Scalar") {
                    params.insert(n, s.clone());
                }
            } else {
                return Err(b.fail(format!("{}: instance parameter {n} {t}", name_of(owner))));
            }
        }
        let other: Vec<&str> = q
            .keys()
            .map(String::as_str)
            .filter(|k| !EMITTER_PROPS.contains(k))
            .chain(op.keys().map(String::as_str).filter(|k| !["ParticleSystemComponent", "RootComponent"].contains(k)))
            .collect();
        if !other.is_empty() {
            return Err(b.fail(format!("{}: emitter properties {}", name_of(owner), other.join(","))));
        }
        let Some(t) = q.get("Template").and_then(Json::as_str) else {
            return Err(b.fail(format!("{}: no template", name_of(owner))));
        };
        let mut x = JsonProps::new();
        x.insert("name".into(), name_of(owner).into());
        x.insert("system".into(), b.add_system(t)?.into());
        x.insert("loc".into(), nums(&vec(q.get("RelativeLocation"), [0.0; 3])));
        x.insert("rot".into(), nums(&rot_of(q.get("RelativeRotation"))));
        x.insert("scale".into(), nums(&vec(q.get("RelativeScale3D"), [1.0; 3])));
        if let Some(s) = q.get("TranslucencySortPriority").filter(|v| json_truthy(v)) {
            x.insert("sort".into(), s.clone());
        }
        if !params.is_empty() {
            x.insert("params".into(), Json::Obj(params));
        }
        b.emitters.borrow_mut().push(Json::Obj(x));
    }
    for e in objects {
        if class_of(e) != "SkeletalMeshComponent" {
            continue;
        }
        let owner = actor_of(e);
        let sma = owner.filter(|o| class_of(o) == "SkeletalMeshActor");
        let bp = owner.and_then(|o| blueprints.get(o.path()));
        if !own && bp.is_none() && sma.is_none() {
            continue;
        }
        if let Some(owner) = sma {
            // (every level's: a gameplay level's too, Tree Fort's fence and tree. A hidden one is drawn only where the
            // level Blueprint shows it, its skeletal_shows hook giving the entry's fields: Tree Fort's snails)
            let hidden = truthy(&props_of(owner), "bHidden");
            let fields = if hidden {
                match &level_run {
                    Some((_, part)) => b.with_run(*part, |r| r.skeletal_shows(owner))?,
                    None => None,
                }
            } else {
                Some(JsonProps::new())
            };
            let Some(fields) = fields else { continue };
            // (an AnimClass a part enters, the registry's anim_bp: the part writes it, its skeletal_actor hook;
            // Scooby's statues)
            let ep = props_of(e);
            let ac = ep.get("AnimClass").and_then(Json::as_str).map_or("", last_name);
            if let Some(part) = b.reg.anim_bp.get(ac) {
                if hidden || !b.with_run(*part, |r| r.skeletal_actor(ac, &ctx, owner, e))? {
                    return Err(lfail(format!("skeletal mesh actor {}", name_of(owner))));
                }
                continue;
            }
            let at = ctx.place_of(e, None)?;
            ctx.add_skeletal_impl(
                name_of(owner),
                &ep,
                &at,
                &SkeletalOpts { hidden, fields: Some(fields), ..Default::default() },
            )?;
        } else if let Some((owner, part)) = bp {
            if !b.with_run(*part, |r| r.skeletal_mesh(class_of(owner), &ctx, owner, e))? {
                return Err(lfail(format!("skeletal mesh component of {}", class_of(owner))));
            }
        } else {
            return Err(lfail(format!("skeletal mesh component of {}", outer_name(e))));
        }
    }
    // the level's landscapes (a scenery level's only)
    if own {
        let lightmap_of = |what: &str, id: Option<&str>| ctx.lightmap_by_id(what, id);
        if let Some(ls) = landscapes_of(lv, &lightmap_of, &b.mats)? {
            if b.landscape.borrow().is_some() {
                return Err(lfail("landscapes in two levels".into()));
            }
            let mut refs = Vec::new();
            for a in ls.get("actors").and_then(Json::as_array).unwrap_or(&[]) {
                for c in a.get("components").and_then(Json::as_array).unwrap_or(&[]) {
                    refs.push(c.get("material").and_then(Json::as_str).map(str::to_owned));
                }
            }
            b.name(&refs);
            *b.landscape.borrow_mut() = Some(ls);
        }
    }
    for e in objects {
        let cls = class_of(e);
        if cls == "FoliageInstancedStaticMeshComponent" {
            if let Some(f) = ctx.foliage_of(e)? {
                b.foliage.borrow_mut().push(f);
            }
            continue;
        }
        if cls.ends_with("InstancedStaticMeshComponent") {
            return Err(lfail(format!("{cls} {}", name_of(e))));
        }
        if cls != "StaticMeshComponent" {
            continue;
        }
        let parent = actor_of(e);
        let mut actor = parent.filter(|p| class_of(p) == "StaticMeshActor");
        let mut p = props_of(e);
        let mut extra = JsonProps::new();
        let bp = blueprints.get(parent_path(e.path()));
        let owner = bp.map(|(o, _)| *o);
        if !own && owner.is_none() {
            // a gameplay or lighting level's StaticMeshActor: drawn here when shown and not a BasicShape, or a
            // BasicShape with a material that is not one of Test_Gym's (the stage data draws only those: Tree Fort
            // 1v1 / 1 Platform's BushesRight3 / 4, engine Planes with M_BackgroundBushes_Wind_Top / _Wind)
            let stage_shape = p.get("StaticMesh").and_then(Json::as_str).is_some_and(|m| m.starts_with(BASIC_SHAPES))
                && object_paths(p.get("OverrideMaterials"))
                    .iter()
                    .all(|m| m.as_ref().is_none_or(|m| m.starts_with(STAGE_MATERIALS)));
            // (a hidden one the level Blueprint shows is written, hidden by its instance fields: Batcave's steam
            // screens)
            let Some(a) = actor else { continue };
            if (truthy(&props_of(a), "bHidden") && !shows(a)?)
                || is_false(&p, "bVisible")
                || truthy(&p, "bHiddenInGame")
                || stage_shape
            {
                continue;
            }
            let floor = objects.iter().any(|x| {
                class_of(x) == "BoxComponent"
                    && parent_path(x.path()) == a.path()
                    && props_of(x)
                        .get("ComponentTags")
                        .and_then(Json::as_array)
                        .is_some_and(|t| t.iter().any(|t| t.as_str() == Some("TraceEdges")))
            });
            if floor {
                return Err(lfail(format!(
                    "{}: a shown TraceEdges floor with a mesh that is not a BasicShape",
                    name_of(a)
                )));
            }
        }
        let mut actor_name = actor.map_or_else(String::new, |a| name_of(a).to_owned());
        if let Some((owner, part)) = bp {
            p = component_props(&ctx, e)?;
            let relative =
                truthy(&p, "RelativeLocation") || truthy(&p, "RelativeRotation") || truthy(&p, "RelativeScale3D");
            let r = b.with_run(*part, |r| r.static_mesh(class_of(owner), &ctx, owner, e, &p))?;
            let m = match r {
                StaticMeshOut::NoHook => return Err(lfail(format!("static mesh component of {}", class_of(owner)))),
                StaticMeshOut::NotDrawn => continue,
                StaticMeshOut::Drawn(m) => m,
            };
            p = m.p;
            extra = m.extra;
            if relative && !m.placed {
                return Err(b.fail(format!("{}.{}: relative transform", name_of(owner), name_of(e))));
            }
            actor = None;
            actor_name = format!("{}.{}", name_of(owner), name_of(e));
        }
        if actor.is_none() && owner.is_none() {
            return Err(lfail(format!("{} of {} is not a StaticMeshActor's component", name_of(e), outer_name(e))));
        }
        if truthy(&p, "AttachParent") && owner.is_none() {
            // A StaticMeshActor's mesh attached to another actor's component (Tree Fort's Trees2 / Trees3 on
            // Mountains2's mesh): its world transform up the chain, and `attachedTo`, the actor whose root it ends at
            // (which a block may move: Mountains2's Comp_Parallax in the 1v1 maps).
            let mut top: &ObjView<'a> = e;
            let mut q = props_of(e);
            while let Some(ap) = q.get("AttachParent").filter(|v| json_truthy(v)) {
                let ap = ap.as_str().unwrap_or_default();
                let Some(t) = ctx.by_path.get(ap).copied() else {
                    return Err(lfail(format!("{}: no parent {ap}", name_of(e))));
                };
                top = t;
                q = props_of(top);
            }
            if outer_name(top) == actor_name {
                return Err(b.fail(format!("{actor_name}: attached within its own actor")));
            }
            spread(&mut p, as_props(&world_of(objects, e, &OwnProps)?));
            p.shift_remove("AttachParent");
            extra = JsonProps::new();
            extra.insert("attachedTo".into(), outer_name(top).into());
        }
        let att = if owner.is_none() { begun.get(&actor_name) } else { None };
        if let Some((att, att_owner)) = att {
            // attached at BeginPlay: per EAttachmentRule, KeepRelative (0) composes the placed value (a root's
            // relative one, which was its world one) under the parent's world transform, KeepWorld (1) keeps the
            // placed world value
            if truthy(&p, "AttachParent") {
                return Err(b.fail(format!("{actor_name}: placed attached and attached again at BeginPlay")));
            }
            let placed = rel_of(&p);
            let par = world_of(objects, &att.parent, &*att.props)?;
            if par.s.iter().any(|v| *v < 0.0) || placed.s.iter().any(|v| *v < 0.0) || att.rules.iter().any(|r| *r > 1) {
                return Err(b.fail(format!("{actor_name}: BeginPlay attach not taken (negative scale or rule)")));
            }
            let c = xf_mul(&placed, &par);
            let [lr, rr, sr] = att.rules;
            let x = Xform {
                t: if lr != 0 { placed.t } else { c.t },
                q: if rr != 0 { placed.q } else { c.q },
                s: if sr != 0 { placed.s } else { c.s },
            };
            spread(&mut p, as_props(&x));
            extra.insert("attachedTo".into(), name_of(att_owner).into());
        }
        // the Blueprint components of a StaticMeshActor that the scenery writes (the registry's components)
        if let Some(a) = actor {
            for c in objects {
                if parent_path(c.path()) != a.path() {
                    continue;
                }
                if let Some(f) = b.reg.components.get(class_of(c)) {
                    spread(&mut extra, f(&props_of(c), &ctx)?);
                }
            }
        }
        let Some(sm) = p.get("StaticMesh").filter(|v| json_truthy(v)).cloned() else { continue };
        let sm = match sm {
            Json::Str(s) => s,
            other => crate::json::to_vec(&other).map(|v| String::from_utf8_lossy(&v).into_owned()).unwrap_or_default(),
        };
        let hidden_actor = match actor {
            Some(a) => truthy(&props_of(a), "bHidden") && !shows(a)?,
            None => false,
        };
        if hidden_actor || is_false(&p, "bVisible") || truthy(&p, "bHiddenInGame") {
            continue;
        }
        let mesh = b.add_mesh(&sm)?;
        let slots = b.slots_of(&sm, &object_paths(p.get("OverrideMaterials")))?;
        let lm = ctx.lightmap_of(e)?;
        let channels = channels_of(&p);
        let slot_lm = lm.as_ref().map(|l| SlotLightmap { shadow: l.shadow });
        let materials = b.mats.build_for(&slots, slot_lm.as_ref(), Some(&channels))?;
        b.name(&materials);
        // the instance fields of the level Blueprint (a StaticMeshActor of its level) and of the Blueprint actor
        if let Some(a) = actor {
            for t in tags_of(&props_of(a)) {
                if b.reg.level_tags.contains(t.as_str())
                    && !level_run.as_ref().is_some_and(|(l, _)| l.tags.contains(&t))
                {
                    return Err(b.fail(format!("{actor_name}: {t} without the level Blueprint that reads it")));
                }
            }
        }
        if let Some((_, part)) = &level_run {
            let stand_in;
            let who = match actor {
                Some(a) => InstanceActor::Placed(a),
                None => {
                    stand_in = format!("{}.{}", owner.map_or("", |o| o.path()), name_of(e));
                    InstanceActor::Path(&stand_in)
                }
            };
            let mut fields = b.with_run(*part, |r| r.instance(who))?;
            spread(&mut fields, extra);
            extra = fields;
        }
        let mut x = JsonProps::new();
        x.insert("name".into(), actor_name.clone().into());
        x.insert("mesh".into(), mesh.into());
        x.insert("loc".into(), nums(&vec(p.get("RelativeLocation"), [0.0; 3])));
        x.insert("rot".into(), nums(&rot_of(p.get("RelativeRotation"))));
        x.insert("scale".into(), nums(&vec(p.get("RelativeScale3D"), [1.0; 3])));
        x.insert("materials".into(), refs_json(&materials));
        if let Some(lm) = lm {
            x.insert("lm".into(), lm.json);
        }
        // ULightComponent / UPrimitiveComponent defaults: channel 0 only, CastShadow true, Static mobility
        x.insert("channels".into(), bools(&channels));
        x.insert("castShadow".into(), set(&p, "CastShadow").cloned().unwrap_or(Json::Bool(true)));
        if let Some(m) = p.get("Mobility").filter(|v| json_truthy(v)).and_then(Json::as_str) {
            x.insert("mobility".into(), bare(m).into());
        }
        if let Some(s) = p.get("TranslucencySortPriority").filter(|v| json_truthy(v)) {
            x.insert("sort".into(), s.clone());
        }
        spread(&mut x, extra);
        b.instances.borrow_mut().push(x);
        if is_false(&p, "bReceivesDecals") {
            b.no_decals.borrow_mut().insert(actor_name);
        }
    }
    // The level's DecalActors: the component's world transform and material. A hidden actor or component draws
    // nothing.
    for e in objects {
        if class_of(e) != "DecalComponent" {
            continue;
        }
        let Some(owner) = actor_of(e).filter(|o| class_of(o) == "DecalActor") else {
            return Err(lfail(format!("decal component of {}", outer_name(e))));
        };
        let q = props_of(e);
        if truthy(&props_of(owner), "bHidden") || is_false(&q, "bVisible") || truthy(&q, "bHiddenInGame") {
            continue;
        }
        let other: Vec<&str> = q.keys().map(String::as_str).filter(|k| !DECAL_PROPS.contains(k)).collect();
        if !other.is_empty() {
            return Err(b.fail(format!("{}: decal properties {}", name_of(owner), other.join(","))));
        }
        let Some(material) = q.get("DecalMaterial").and_then(Json::as_str) else {
            return Err(b.fail(format!("{}: no decal material", name_of(owner))));
        };
        if ZERO_OPACITY_DECALS.contains(&pkg_name(material)) {
            continue;
        }
        let xf = xf_out(&world_of(objects, e, &OwnProps)?);
        b.decal_places.borrow_mut().push(DecalPlace {
            name: name_of(owner).to_owned(),
            material: material.to_owned(),
            loc: xf.loc,
            rot: xf.rot,
            scale: xf.scale,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(main_path("/Game/A/B.b"), "/Game/A/B.B");
        assert_eq!(main_path("/Game/A/B"), "/Game/A/B.B");
        assert_eq!(key_path("Panda_Main/Maps/M.x"), "/Game/Panda_Main/Maps/M.x.M.x");
        assert_eq!(key_path("Engine/Content/BasicShapes/Cube"), "/Engine/BasicShapes/Cube.Cube");
        assert_eq!(pkg_name("/Game/A/B.B"), "B");
        assert_eq!(last_name("/Game/A/B.B_C"), "B_C");
        assert_eq!(parent_path("/Game/M.M:PersistentLevel.A.Comp"), "/Game/M.M:PersistentLevel.A");
    }

    #[test]
    fn channels_and_spread() {
        let p: JsonProps = [("LightingChannels".to_owned(), Json::object([("bChannel1", Json::Bool(true))]))].into();
        assert_eq!(channels_of(&p), [true, true, false]);
        assert_eq!(channels_of(&JsonProps::new()), [true, false, false]);
        let mut a: JsonProps = [("x".to_owned(), Json::Num(1.0)), ("y".to_owned(), Json::Num(2.0))].into();
        spread(&mut a, [("x".to_owned(), Json::Num(3.0)), ("z".to_owned(), Json::Num(4.0))].into());
        assert_eq!(a.keys().collect::<Vec<_>>(), ["x", "y", "z"]);
        assert_eq!(a["x"], Json::Num(3.0));
    }
}
