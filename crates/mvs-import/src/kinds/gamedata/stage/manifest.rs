//! The stage manifest bound from the build: per stage key, its MapData assets, its levels and their roles, its sky
//! level, its volumetric lightmap registries and its spawn areas.

use std::sync::Arc;

use indexmap::IndexMap;
use mvs_data::{SpawnArea, StageManifest};

use super::super::shape::{fail, short_name};
use super::context::{StageCtx, f, js_truthy, s};
use crate::game::MAPS_ROOT;
use crate::kind::{Cx, KindError};
use crate::kinds::lighting::entry_header;
use crate::objview::{ObjView, string};
use crate::stage_levels::level_built_data;

/// The MapData assets' folder: the build finds them by class in it (GetAssetsByClass).
const MAP_DATA_FOLDER: &str = "/Game/Panda_Main/Maps/MapDataAssets";
const MAP_DATA_CLASS: &str = "/Script/MultiVersus.MapData";

/// The stages no MapData names: a persistent level a game mode opens by name (OpenLevel), its key as a MapData
/// stage's (the level name less `Map_`, lowercased). Map_TrophyRoom_Tutorial: every tutorial's map (UI_MainMenu @8503,
/// WBP_SubOptionModal's advanced tutorials).
const MAP_ONLY_STAGES: [&str; 1] = ["TrophyRoom/Map_TrophyRoom_Tutorial"];

/// A level holding one of these is the stage's gameplay level.
const GAMEPLAY_CLASSES: [&str; 2] = ["BlastBoxPlayerExit_C", "PlayerStart"];
/// A level holding one of these (and not the gameplay level) is a lighting level.
const LIGHTING_CLASSES: [&str; 3] = ["SkyLight", "DirectionalLight", "ExponentialHeightFog"];
/// A level holding one of these (and none of the above) is the audio level.
const AUDIO_CLASSES: [&str; 3] = ["Env_Audio_Emit_C", "AkReverbVolume", "AkAmbientSound"];
/// The actors every level has, which nothing draws or runs: left out of the role rule.
const ENGINE_CLASSES: [&str; 6] =
    ["WorldSettings", "Model", "Brush", "LevelBounds", "DefaultPhysicsVolume", "LightmassImportanceVolume"];
/// The placed spawn area.
const SPAWN_AREA_CLASS: &str = "MapRandomActorSpawnArea_C";

fn base(lvl: &str) -> &str {
    lvl.rsplit('/').next().unwrap_or(lvl)
}

/// A level reference (`/Game/Panda_Main/Maps/M011/Map_M011.Map_M011`) to its manifest path (`M011/Map_M011`).
fn level_of(r: Option<&mvs_data::Value>, asset: &str) -> Result<String, KindError> {
    let Some(r) = r.and_then(mvs_data::Value::as_str) else {
        return fail(format!("stage manifest {asset}: a level reference is not an object path"));
    };
    let pkg = r.split('.').next().unwrap_or(r);
    match pkg.strip_prefix(MAPS_ROOT) {
        Some(l) => Ok(l.to_owned()),
        None => fail(format!("stage manifest {asset}: level {pkg} is not under {MAPS_ROOT}")),
    }
}

/// The gameplay level's name less `Map_` (without regard to case), lowercased.
fn key_of_level(lvl: &str) -> String {
    let b = base(lvl);
    let b = if b.len() >= 4 && b[..4].eq_ignore_ascii_case("Map_") { &b[4..] } else { b };
    b.to_lowercase()
}

/// What the role rule reads of a level.
struct LevelInfo {
    level: String,
    /// The PersistentLevel's direct children by short class name, counted, in first-seen order.
    actors: IndexMap<String, usize>,
    sky: bool,
    script: String,
    built_data: Option<String>,
}

impl LevelInfo {
    fn count(&self, list: &[&str]) -> usize {
        list.iter().map(|t| self.actors.get(*t).copied().unwrap_or(0)).sum()
    }
}

fn is_actor_chain(chain: &str) -> bool {
    chain.strip_prefix("PersistentLevel.").is_some_and(|n| !n.is_empty() && !n.contains('.'))
}

fn level_info(cx: &Cx<'_>, level: &str) -> Result<LevelInfo, KindError> {
    let world = cx.object(&format!("{MAPS_ROOT}{level}"))?;
    let objects = world.inner()?;
    let mut actors: IndexMap<String, usize> = IndexMap::new();
    for o in objects.iter().filter(|o| is_actor_chain(o.chain())) {
        *actors.entry(short_name(o.class()).to_owned()).or_default() += 1;
    }
    let sky = objects.iter().any(|o| {
        short_name(o.class()) == "SkyLightComponent"
            && !matches!(o.prop("bAffectsWorld"), Some(mvs_uasset::value::Value::Bool(false)))
    });
    Ok(LevelInfo {
        level: level.to_owned(),
        actors,
        sky,
        script: format!("{}_C", base(level)),
        built_data: level_built_data(&objects),
    })
}

/// The sub-level bricks a registry's volumetric lightmap holds (SubLevelBrickPositions).
fn sub_level_bricks(cx: &Cx<'_>, built_data: &str) -> Result<usize, KindError> {
    let h = entry_header(cx, built_data)?;
    let Some(vlm) = h.get("vlm").and_then(|v| v.as_object()) else { return Ok(0) };
    Ok(vlm.values().map(|x| x.get("SubLevelBrickPositions").and_then(|b| b.as_array()).map_or(0, |b| b.len())).sum())
}

/// A stage of the MapData set.
struct MapStage {
    key: String,
    map_data: Vec<String>,
    /// The Lab levels, the assets' union in the first asset's order.
    lab: Vec<String>,
    /// The first asset's gameplay level: its Level where LabLevelsToLoad holds it, else its first Lab level.
    declared: String,
}

/// A map-only stage: its persistent level, then the levels its World streams, in the order of its LevelStreaming
/// objects (the store's World holds no StreamingLevels; the streaming objects in export order are that list).
fn map_only_stage(cx: &Cx<'_>, level: &str) -> Result<MapStage, KindError> {
    let world = cx.object(&format!("{MAPS_ROOT}{level}"))?;
    let mut lab = vec![level.to_owned()];
    for o in world.inner()? {
        if o.class().starts_with("/Script/Engine.LevelStreaming") && !o.chain().contains('.') {
            let wa = o.prop("WorldAsset").and_then(|v| string(o.cache(), v));
            lab.push(level_of(wa.map(mvs_data::Value::Str).as_ref(), level)?);
        }
    }
    Ok(MapStage { key: key_of_level(level), map_data: Vec::new(), lab, declared: level.to_owned() })
}

/// The stages of the MapData assets by key, in the assets' code-unit name order.
fn map_stages(cx: &Cx<'_>) -> Result<IndexMap<String, MapStage>, KindError> {
    let mut names: Vec<String> = cx
        .catalog
        .list_packages(MAP_DATA_FOLDER, false, Some(MAP_DATA_CLASS))
        .iter()
        .map(|p| base(p).to_owned())
        .collect();
    // (code-unit order: the registry's own puts map_training before Map_TrainingRoomSmall)
    names.sort_unstable();
    let mut out: IndexMap<String, MapStage> = IndexMap::new();
    for name in &names {
        let o = cx.object(&format!("{MAP_DATA_FOLDER}/{name}.{name}"))?;
        let p = super::super::shape::properties(&o)?;
        let lab: Vec<String> = match p.get("LabLevelsToLoad") {
            Some(mvs_data::Value::Arr(refs)) => {
                refs.iter().map(|r| level_of(Some(r), name)).collect::<Result<_, _>>()?
            }
            _ => Vec::new(),
        };
        let level = level_of(p.get("Level"), name)?;
        let gameplay = if lab.contains(&level) { Some(level) } else { lab.first().cloned() };
        let Some(gameplay) = gameplay else { return fail(format!("stage manifest {name}: no LabLevelsToLoad")) };
        let key = key_of_level(&gameplay);
        let st = out.entry(key.clone()).or_insert_with(|| MapStage {
            key,
            map_data: Vec::new(),
            lab: Vec::new(),
            declared: gameplay,
        });
        // (the MapData export's own name: Map_trophy_room_large_platform's is map_trophy_room_large_platform)
        st.map_data.push(short_name(o.path()).to_owned());
        for l in lab {
            if !st.lab.contains(&l) {
                st.lab.push(l);
            }
        }
    }
    Ok(out)
}

/// The registries of the stage's volumetric lightmap: the gameplay level's, then each other Lab level's whose volume
/// holds sub-level bricks, in LabLevelsToLoad order.
fn vlm_of(cx: &Cx<'_>, st: &MapStage, info: &IndexMap<String, LevelInfo>) -> Result<Vec<String>, KindError> {
    let mut subs = Vec::new();
    for l in &st.lab {
        let b = info[l].built_data.as_deref();
        if *l != st.declared
            && let Some(b) = b
            && sub_level_bricks(cx, b)? > 0
        {
            subs.push(b.to_owned());
        }
    }
    match info[&st.declared].built_data.clone() {
        Some(main) => Ok(std::iter::once(main).chain(subs).collect()),
        None if !subs.is_empty() => fail(format!(
            "stage manifest {}: volumetric lightmap sub-levels of a gameplay level with no build data",
            st.key
        )),
        None => Ok(Vec::new()),
    }
}

/// Each Lab level's role and the sky level (a stage the rule does not fit stops the bind).
fn roles_of(st: &MapStage, info: &IndexMap<String, LevelInfo>) -> Result<(Vec<(String, String)>, String), KindError> {
    let err = |m: String| -> Result<(), KindError> { fail(format!("stage manifest {}: {m}", st.key)) };
    let gp: Vec<&String> = st.lab.iter().filter(|l| info[*l].count(&GAMEPLAY_CLASSES) > 0).collect();
    if gp.len() != 1 {
        err(format!(
            "{} levels with gameplay actors ({}): {}",
            gp.len(),
            GAMEPLAY_CLASSES.join(", "),
            if gp.is_empty() {
                "none".to_owned()
            } else {
                gp.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            }
        ))?;
    }
    let gameplay = gp[0].clone();
    if gameplay != st.declared {
        err(format!("the gameplay actors are in {gameplay}, the MapData asset's gameplay level is {}", st.declared))?;
    }
    let mut levels: Vec<(String, String)> = Vec::new();
    for l in &st.lab {
        let i = &info[l];
        let get = |k: &str| i.actors.get(k).copied().unwrap_or(0);
        let meshes = get("StaticMeshActor") + get("SkeletalMeshActor");
        let others = i.actors.keys().filter(|t| !ENGINE_CLASSES.contains(&t.as_str()) && **t != i.script).count();
        let role = if *l == gameplay {
            "gameplay"
        } else if i.count(&LIGHTING_CLASSES) > 0 {
            "lighting"
        } else if i.count(&AUDIO_CLASSES) > 0 {
            "audio"
        } else if get("Emitter") > meshes {
            "vfx"
        } else if others > 0 {
            "scenery"
        } else {
            "empty"
        };
        levels.push((l.clone(), role.to_owned()));
    }
    let of = |role: &str| -> Vec<&String> { levels.iter().filter(|(_, r)| r == role).map(|(l, _)| l).collect() };
    let audio = of("audio");
    if audio.len() > 1 {
        err(format!("two audio levels: {}", audio.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")))?;
    }
    let sky: Vec<String> = std::iter::once(&gameplay).chain(of("lighting")).filter(|l| info[*l].sky).cloned().collect();
    if sky.len() != 1 {
        err(format!(
            "{} levels with a world-affecting SkyLightComponent: {}",
            sky.len(),
            if sky.is_empty() { "none".to_owned() } else { sky.join(", ") }
        ))?;
    }
    Ok((levels, sky[0].clone()))
}

/// Each placed MapRandomActorSpawnArea_C as the world bounds of its Box (GetComponentBounds(Box), FindRandomSpawnPoint
/// @4083), its root's location (K2_GetActorLocation, SpawnCenterActor @3144) and its BigObjectSpawnXLocations (the
/// placed actor's, else the class default's: @3038), levels in LabLevelsToLoad order, actors in export order. Every
/// placed one is unrotated with a root attached to nothing: a rotated or attached one stops the bind.
fn spawn_areas_of(
    ctx: &StageCtx<'_>,
    key: &str,
    levels: &[String],
    info: &IndexMap<String, LevelInfo>,
) -> Result<Vec<SpawnArea>, KindError> {
    let mut out = Vec::new();
    let v3 = |v: Option<&mvs_data::Value>, d: f64| -> [f64; 3] {
        let o = v.and_then(mvs_data::Value::as_obj);
        ["X", "Y", "Z"].map(|k| o.and_then(|o| f(o, k)).unwrap_or(d))
    };
    for lvl in levels {
        let i = &info[lvl];
        // (the actors of the level in export order, through the context's own load of it)
        let world = ctx.cx.object(&format!("{MAPS_ROOT}{}", i.level))?;
        let objs = world.inner()?;
        for a in objs.iter().filter(|o| is_actor_chain(o.chain())) {
            if short_name(a.class()) != SPAWN_AREA_CLASS {
                continue;
            }
            let name = short_name(a.path()).to_owned();
            let err = |m: &str| -> Result<(), KindError> { fail(format!("stage manifest {key}: {lvl} {name}: {m}")) };
            let cs: Vec<&ObjView<'_>> = objs
                .iter()
                .filter(|c| {
                    c.path().strip_prefix(a.path()).is_some_and(|r| r.starts_with('.') && !r[1..].contains('.'))
                })
                .collect();
            let box_c = cs.iter().find(|c| short_name(c.path()) == "Box");
            let root_c = cs.iter().find(|c| short_name(c.path()) == "DefaultSceneRoot");
            let (Some(box_c), Some(root_c)) = (box_c, root_c) else {
                return fail(format!("stage manifest {key}: {lvl} {name}: no Box or DefaultSceneRoot component"));
            };
            let bx = ctx.with_templates(box_c)?;
            let root = ctx.with_templates(root_c)?;
            let own = ctx.with_templates(a)?;
            let rotated = |t: &mvs_data::Props| {
                t.get("RelativeRotation")
                    .and_then(mvs_data::Value::as_obj)
                    .is_some_and(|q| ["Pitch", "Yaw", "Roll"].iter().any(|k| js_truthy(q.get(*k))))
            };
            if rotated(&bx) || rotated(&root) {
                err("a rotated spawn area")?;
            }
            if js_truthy(root.get("AttachParent")) {
                err("its root is attached")?;
            }
            let ok = s(&bx, "AttachParent")
                .is_some_and(|bp| bp.ends_with(&format!(":PersistentLevel.{name}.DefaultSceneRoot")));
            if !ok {
                err("its Box is not attached to DefaultSceneRoot")?;
            }
            let ext = v3(bx.get("BoxExtent"), 32.0);
            let (bl, bs) = (v3(bx.get("RelativeLocation"), 0.0), v3(bx.get("RelativeScale3D"), 1.0));
            let (rl, rs) = (v3(root.get("RelativeLocation"), 0.0), v3(root.get("RelativeScale3D"), 1.0));
            let big = match own.get("BigObjectSpawnXLocations") {
                Some(mvs_data::Value::Arr(l)) if l.iter().all(|x| x.as_f64().is_some()) => {
                    l.iter().filter_map(mvs_data::Value::as_f64).collect()
                }
                _ => {
                    return fail(format!(
                        "stage manifest {key}: a spawn area's BigObjectSpawnXLocations is no number list"
                    ));
                }
            };
            out.push(SpawnArea {
                level: lvl.clone(),
                actor: name.clone(),
                origin: (0..3).map(|k| rl[k] + rs[k] * bl[k]).collect(),
                extent: (0..3).map(|k| (rs[k] * bs[k] * ext[k]).abs()).collect(),
                root: rl.to_vec(),
                big_object_x: big,
            });
        }
    }
    Ok(out)
}

/// The run's manifest.
struct Manifests(Result<Arc<IndexMap<String, StageManifest>>, String>);

/// The manifest entry of each stage (`STAGES`, in that order), bound from the MapData assets, the map-only levels and
/// their levels, once per run (loadStageManifest).
pub fn stage_manifest(cx: &Cx<'_>) -> Result<Arc<IndexMap<String, StageManifest>>, KindError> {
    cx.shared
        .get_or_init(|| Manifests(load(cx).map(Arc::new).map_err(|e| e.to_string())))
        .0
        .clone()
        .map_err(KindError::Failed)
}

fn load(cx: &Cx<'_>) -> Result<IndexMap<String, StageManifest>, KindError> {
    let keys = super::super::STAGES;
    let mut stages = map_stages(cx)?;
    for l in MAP_ONLY_STAGES {
        let st = map_only_stage(cx, l)?;
        if stages.contains_key(&st.key) {
            return fail(format!("stage manifest: the map-only stage {l} has a MapData"));
        }
        stages.insert(st.key.clone(), st);
    }
    let extra: Vec<&str> = stages.keys().map(String::as_str).filter(|k| !keys.contains(k)).collect();
    let missing: Vec<&str> = keys.iter().copied().filter(|k| !stages.contains_key(*k)).collect();
    if !extra.is_empty() || !missing.is_empty() {
        return fail(format!(
            "stage manifest: the MapData set's and the map-only stages' keys are not the source list's (not listed: {}; no MapData: {})",
            if extra.is_empty() { "none".to_owned() } else { extra.join(", ") },
            if missing.is_empty() { "none".to_owned() } else { missing.join(", ") }
        ));
    }
    let mut all: Vec<&String> = Vec::new();
    for k in keys {
        for l in &stages[k].lab {
            if !all.contains(&l) {
                all.push(l);
            }
        }
    }
    let mut info: IndexMap<String, LevelInfo> = IndexMap::new();
    for l in all {
        info.insert(l.clone(), level_info(cx, l)?);
    }
    let mut out = IndexMap::new();
    for key in keys {
        let st = &stages[key];
        let (levels, sky) = roles_of(st, &info)?;
        // (the template merge is the stage context's)
        let kept: Vec<(String, String)> = levels.iter().filter(|(_, r)| r != "empty").cloned().collect();
        let ctx = StageCtx::new(cx, key, &kept)?;
        let spawn_areas = spawn_areas_of(&ctx, key, &st.lab, &info)?;
        let vlm = vlm_of(cx, st, &info)?;
        out.insert((*key).to_owned(), StageManifest { map_data: st.map_data.clone(), levels, sky, vlm, spawn_areas });
    }
    Ok(out)
}
