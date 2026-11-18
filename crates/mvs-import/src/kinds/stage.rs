//! The `stage` kind: the items part's entry alone, `{key, bindVersion, objectVersion, configVersion, lightingVersion,
//! draws}`, JSON, at the earlier path (`game::stage_items_path`; its version, `game::STAGE_KIND_VERSION`, is a new
//! number). It holds what the items
//! part's actors are drawn with, every mesh component of every class the part reaches (`gamedata/items.rs`), keyed
//! `<class>.<component>`, from the class data alone, so the view draws any of them whichever module ports the class.
//!   - materials: the mesh's slots, each the component's OverrideMaterials entry of that index where it names one
//!     (UMeshComponent::GetMaterial), else the mesh's own (a slot with none: WorldGridMaterial, as the skins');
//!   - variants: SpawnedActorParent SetTeamColor's sets (@20946-21922: the instigator's SelectMaterialColor pick of the
//!     class default's Materials on slot 0 of the actor's first skeletal, else static, mesh component), on that
//!     component alone and only where the class's Materials names all three;
//!   - anim: an AnimationSingleNode component's AnimationData.AnimToPlay (Item_QuestionMark_Idle, BouncePad_01_Idle);
//!   - clips: a skeletal component's sequences its class's AnimBP and graph play (ITEM_CLIPS: every AnimSequence of the
//!     folders named), by name.
//!
//! A component whose class sets no mesh (Item_Snowball's SnowballMesh) has no entry. A polymorph buff of the part (its
//! defaults name a Polymorph Mesh: Buff_Chicken) has one under `<buff>.Polymorph`, the SkeletalMeshComponent
//! Buff_PolymorphParent adds on its owner (@4988-5120: SetSkeletalMesh(Polymorph Mesh)): the mesh's slots, its variants
//! the buff's Materials on slot 0 (@5535-5612 SelectMaterialColor(Materials, true), SetMaterial(0, ...)), its clips
//! ITEM_CLIPS'.
//!
//! The entry holds no stage data or manifest entries (`data`): the native game reads them from the gamedata kind, so
//! this kind holds the Items entry alone, and the compare leaves `data` out of an earlier entry. It holds no stage: it
//! is renamed with the cutover, when that compare goes.
//! Unverified: the first of the class's components, in the order as bound, is SetTeamColor's (the actor's component
//! array order is native, not read).

use indexmap::IndexMap;
use mvs_data::{Props, Value};
use serde::Serialize;

use super::fighter::draws::{Slots, WORLD_GRID, mesh_info, slot_refs};
use super::gamedata::items::reach_items;
use crate::game::{self, STAGE_BIND_VERSION, STAGE_KIND_VERSION};
use crate::kind::{Cx, Item, Kind, KindError};
use crate::kinds::config::CONFIG_KIND_VERSION;
use crate::kinds::lighting::LIGHTING_KIND_VERSION;
use crate::kinds::object::OBJECT_KIND_VERSION;

const ANIM_SEQUENCE: &str = "/Script/Engine.AnimSequence";
const SKELETAL: &str = "/Script/Engine.SkeletalMeshComponent";
const STATIC: &str = "/Script/Engine.StaticMeshComponent";
const SINGLE_NODE: &str = "EAnimationMode::AnimationSingleNode";

/// The folders whose AnimSequences a class's skeletal mesh plays, by class: the helpers' AnimBPs (BananaGuard_AnimBp's
/// state clips, Neptr_AnimBlueprint's machine and Neptr_Throw_Montage's sequence), the pad's (BouncePad_01_Skeleton_
/// AnimBlueprint's SequencePlayer BouncePad_01_Idle under the DefaultSlot of BouncePad_01_Bounce_Montage's
/// BouncePad_01_Bounce), the Meeseek runner's (Meeseeks_Attack_Run_Montage's Meeseeks_Poses and FlailingRun /
/// FlailingFall), the FFA cars' (Batmobile_Car_SkelMesh_Skeleton_AnimBlueprint's players and the sequences of
/// BatMobile_SpawnedActor_Child TurnVisuals' Batmobile_Turn_Montage / Batmobile_Turn_02_Montage; MysteryMachine_AnimBP's
/// Velma_PoliceCar_* players) and, by the buff, the chicken's (Chicken_AnimBP's set and Buff_Chicken's montages).
const ITEM_CLIPS: &[(&str, &str)] = &[
    ("ItemHelper_BananaGuard_C", "/Game/Panda_Main/Characters/ItemHelpers/BananaGuard"),
    ("ItemHelper_Nepter_C", "/Game/Panda_Main/Characters/ItemHelpers/Neptr"),
    ("BouncePad_SpawnedActor_C", "/Game/Panda_Main/Blueprints/Items/Item_BouncePad"),
    ("SelfActivate_Meeseeks_Run_C", "/Game/Panda_Main/Blueprints/Tutorial/TempMeeseeksMove"),
    ("BatMobile_SpawnedActor_Child_C", "/Game/Panda_Main/Blueprints/Items/Item_Cars/Batmobile"),
    ("MysteryMachine_SpawnedCar_C", "/Game/Panda_Main/Characters/Velma/Animation/Car"),
    ("Buff_Chicken_C", "/Game/Panda_Main/Characters/C015/Chicken"),
];

/// One draw of the items entry.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ItemDraw {
    mesh: String,
    materials: Slots,
    #[serde(skip_serializing_if = "Option::is_none")]
    variants: Option<IndexMap<&'static str, Slots>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    white_vertex_color: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anim: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    clips: Option<IndexMap<String, String>>,
}

/// The entry as written (an earlier entry also holds `data`, between `lightingVersion` and `draws`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    key: &'static str,
    bind_version: u64,
    object_version: u32,
    config_version: u64,
    lighting_version: u64,
    draws: IndexMap<String, ItemDraw>,
}

/// A non-empty string value (`str`).
fn str_of(v: Option<&Value>) -> Option<&str> {
    match v {
        Some(Value::Str(s)) if !s.is_empty() => Some(s),
        _ => None,
    }
}

/// The three team sets of a class's (or buff's) `Materials` struct, where it names all three.
fn team_sets(raw: Option<&Value>) -> Option<[String; 3]> {
    let set = raw.and_then(Value::as_obj);
    let get = |k: &str| str_of(set.and_then(|s| s.get(k))).map(str::to_owned);
    Some([get("RedMaterialOverride")?, get("BlueMaterialOverride")?, get("LocalPlayerMaterialOverride")?])
}

/// The three variants of `mats` with slot 0 replaced by each team's material (`with0`).
fn variants(
    cx: &Cx<'_>,
    sets: &[String; 3],
    mats: &[String],
    ctx: &str,
) -> Result<IndexMap<&'static str, Slots>, KindError> {
    let mut out = IndexMap::new();
    for (name, p) in ["red", "blue", "local"].into_iter().zip(sets) {
        let mut l = vec![p.clone()];
        l.extend(mats.iter().skip(1).cloned());
        out.insert(name, slot_refs(cx, &l, ctx)?);
    }
    Ok(out)
}

/// A class's clips: every AnimSequence of its ITEM_CLIPS folder, by name.
fn clips_of(cx: &Cx<'_>, cls: &str, what: &str) -> Result<Option<IndexMap<String, String>>, KindError> {
    let mut pkgs: Vec<String> = Vec::new();
    for (_, d) in ITEM_CLIPS.iter().filter(|(c, _)| *c == cls) {
        pkgs.extend(cx.catalog.list_packages(d, true, Some(ANIM_SEQUENCE)));
    }
    if pkgs.is_empty() {
        return Ok(None);
    }
    let mut clips = IndexMap::new();
    for p in pkgs {
        let n = p[p.rfind('/').map_or(0, |i| i + 1)..].to_owned();
        if clips.insert(n.clone(), format!("{p}.{n}")).is_some() {
            return Err(KindError::Failed(format!("item draws {what}: two sequences named {n}")));
        }
    }
    Ok(Some(clips))
}

/// The draws of the items part's classes and polymorph buffs (the items
/// part's draw table).
fn item_draws(cx: &Cx<'_>) -> Result<IndexMap<String, ItemDraw>, KindError> {
    let reach = reach_items(cx)?;
    let mut out: IndexMap<String, ItemDraw> = IndexMap::new();
    for (cls, c) in &reach.classes {
        let comps: Vec<(&String, &Props)> =
            c.raw_components.iter().filter(|(n, _)| !n.starts_with("NODE_Add")).collect();
        let is_a = |n: &str, k: &str| c.component_class.get(n).is_some_and(|x| x == k);
        let team = comps
            .iter()
            .find(|(n, _)| is_a(n, SKELETAL))
            .or_else(|| comps.iter().find(|(n, _)| is_a(n, STATIC)))
            .map(|(n, _)| n.as_str());
        let sets = team_sets(c.raw_p.get("Materials"));
        for (name, v) in &comps {
            let skeletal = str_of(v.get("SkeletalMesh")).is_some();
            let Some(mesh) = str_of(v.get("SkeletalMesh")).or_else(|| str_of(v.get("StaticMesh"))) else { continue };
            let m = mesh_info(cx, mesh)?;
            if m.skeletal != skeletal {
                return Err(KindError::Failed(format!(
                    "item draws {cls}.{name}: {mesh} is {}",
                    if m.skeletal { "skeletal" } else { "static" }
                )));
            }
            let over: &[Value] = match v.get("OverrideMaterials") {
                Some(Value::Arr(l)) => l,
                _ => &[],
            };
            let mats: Vec<String> = m
                .slots
                .iter()
                .enumerate()
                .map(|(i, sl)| {
                    str_of(over.get(i))
                        .map(str::to_owned)
                        .or_else(|| sl.clone())
                        .unwrap_or_else(|| WORLD_GRID.to_owned())
                })
                .collect();
            let ctx = if skeletal { "skin" } else { "locallm" };
            let mut d = ItemDraw {
                mesh: mesh.to_owned(),
                materials: slot_refs(cx, &mats, ctx)?,
                variants: None,
                white_vertex_color: m.white.then_some(true),
                anim: None,
                clips: None,
            };
            if let (Some(t), Some(sets)) = (team, &sets)
                && t == name.as_str()
            {
                d.variants = Some(variants(cx, sets, &mats, ctx)?);
            }
            let anim = str_of(v.get("AnimationData").and_then(Value::as_obj).and_then(|a| a.get("AnimToPlay")));
            if skeletal && v.get("AnimationMode").and_then(Value::as_str) == Some(SINGLE_NODE) {
                d.anim = anim.map(str::to_owned);
            }
            if skeletal {
                d.clips = clips_of(cx, cls, &format!("{cls}.{name}"))?;
            }
            out.insert(format!("{cls}.{name}"), d);
        }
    }
    for (buff, raw) in &reach.polymorphs {
        let Some(mesh) = str_of(raw.get("Polymorph Mesh")) else { continue };
        let m = mesh_info(cx, mesh)?;
        if !m.skeletal {
            return Err(KindError::Failed(format!("item draws {buff}.Polymorph: {mesh} is static")));
        }
        let mats: Vec<String> = m.slots.iter().map(|sl| sl.clone().unwrap_or_else(|| WORLD_GRID.to_owned())).collect();
        let mut d = ItemDraw {
            mesh: mesh.to_owned(),
            materials: slot_refs(cx, &mats, "skin")?,
            variants: None,
            white_vertex_color: m.white.then_some(true),
            anim: None,
            clips: None,
        };
        if let Some(sets) = team_sets(raw.get("Materials")) {
            d.variants = Some(variants(cx, &sets, &mats, "skin")?);
        }
        d.clips = clips_of(cx, buff, &format!("{buff}.Polymorph"))?;
        out.insert(format!("{buff}.Polymorph"), d);
    }
    Ok(out)
}

/// The `stage` kind: the items part's entry.
pub struct Stage;

impl Kind for Stage {
    fn id(&self) -> &'static str {
        game::STAGE.0
    }

    fn version(&self) -> u64 {
        STAGE_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/json"
    }

    fn list(&self, _: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(vec![Item::new(game::stage_items_path())])
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        if item.path != game::stage_items_path() {
            return Err(KindError::NotFound(format!("stage: no entry {}", item.path)));
        }
        let entry = Entry {
            key: "items",
            bind_version: STAGE_BIND_VERSION,
            object_version: OBJECT_KIND_VERSION,
            config_version: CONFIG_KIND_VERSION,
            lighting_version: LIGHTING_KIND_VERSION,
            draws: item_draws(cx)?,
        };
        crate::json::to_vec(&entry).map_err(|e| KindError::Failed(format!("stage items: {e}")))
    }
}
