//! The items part: what a match with random items reaches outside every fighter's data
//! and the stage's. FFAGameMode's Items and its centre class (`mode_reach.rs`), then
//! each class's spawns and buffs in the order of reaching: its class default members that name classes
//! (ActorClassSpawnedAtDeath, ITEM_MEMBERS), its graph literals (`reach.rs` GRAPH_SPAWNS / GRAPH_BUFFS, then
//! `mode_reach.rs` ITEM_SPAWNS / ITEM_BUFFS), its on-hit buffs and its Comp_Pawn_PassiveBuffManager Buffs; then the
//! mode's respawn catch-up buffs; then what those buffs lead to. Each actor class is bound as a fighter's spawned actor,
//! each buff as its class defaults: the lookups fall back to it as to another fighter's data. A class of the boxes
//! bind's ACTOR_BONE_BOXES gets its components' bone box tracks laid on as a fighter's do (`char.rs`): the items part
//! has no boxes entry of its own.
//!
//! What those classes take from a character's folder comes with them (montages, actorMontages, polymorphAnimSets,
//! animAssets), bound as a fighter's data binds it: the montages its polymorph buffs play (POLYMORPH_MONTAGE_MEMBERS),
//! its actors' graph montages (GRAPH_MONTAGES, by the class and its supers), the polymorph props' anim sets
//! (POLYMORPH_ANIM_LAYERS) and their blend spaces, found by name under the AnimBP's folder.

use std::sync::Arc;

use indexmap::IndexMap;
use mvs_data::fighter::ActorDef;
use mvs_data::{ItemsData, Props, Value};

use super::actor_class::{ActorClass, ActorClasses, actor_passive_buffs};
use super::boxes::frames::actor_bone_boxes;
use super::fighter::actors::{ON_HIT_MEMBERS, applied_buffs};
use super::fighter::body::{AnimAssetKind, POLYMORPH_ANIM_LAYERS, anim_asset_entry, polymorph_anim_set};
use super::fighter::montages::{MontageBind, asset_of, bind_montage};
use super::mode_reach::{FFA_CENTER_ACTOR, FFA_MODE_CDO, FFA_MODE_MEMBER, ITEM_BUFFS, ITEM_MEMBERS, ITEM_SPAWNS};
use super::reach::{BUFF_SPAWN_MEMBERS, GRAPH_BUFFS, GRAPH_MONTAGES, GRAPH_SPAWNS, MODE_BUFFS_FROM};
use super::shape::{defaults_or_properties, fail, properties, short_name};
use crate::kind::{Cx, KindError};

const BLEND_SPACE_1D: &str = "/Script/Engine.BlendSpace1D";
const ANIM_COMPOSITE: &str = "/Script/Engine.AnimComposite";

/// The items part and its actor classes as bound (their raw values: the stage kind's Items draws), by class name.
pub struct ItemsReach {
    pub data: ItemsData,
    pub classes: IndexMap<String, Arc<ActorClass>>,
    pub polymorphs: IndexMap<String, Props>,
}

fn spawns_of(k: &str) -> Result<Vec<&'static str>, KindError> {
    Ok(GRAPH_SPAWNS.get(k)?.iter().chain(ITEM_SPAWNS.get(k)?).copied().collect())
}

fn buffs_of(k: &str) -> Result<Vec<&'static str>, KindError> {
    Ok(GRAPH_BUFFS.get(k)?.iter().chain(ITEM_BUFFS.get(k)?).copied().collect())
}

/// The montages Buff_PolymorphParent plays on its Polymorph Mesh, by its class default members (a buff whose defaults
/// name a Polymorph Mesh): EvadeTriggered's Evades[].AttackMontage in the air, GroundSpotEvade / GroundForwardEvade on
/// the ground (@9068), OnDash's DashMontage / DashTurnMontage (@10830), the taunt's Taunt (@12609, PlayMontage @12160).
const POLYMORPH_MONTAGE_MEMBERS: [&str; 5] =
    ["GroundSpotEvade", "GroundForwardEvade", "DashMontage", "DashTurnMontage", "Taunt"];

fn polymorph_montages(d: &Props) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(Value::Arr(evades)) = d.get("Evades") {
        for e in evades {
            if let Some(Value::Str(m)) = e.as_obj().and_then(|e| e.get("AttackMontage")) {
                out.push(m.clone());
            }
        }
    }
    for k in POLYMORPH_MONTAGE_MEMBERS {
        if let Some(Value::Str(m)) = d.get(k) {
            out.push(m.clone());
        }
    }
    out
}

fn class_list(v: Option<&Value>, what: impl Fn() -> String) -> Result<Vec<String>, KindError> {
    let Some(Value::Arr(l)) = v else { return fail(what()) };
    l.iter()
        .map(|x| match x {
            Value::Str(s) => Ok(s.clone()),
            _ => fail(what()),
        })
        .collect()
}

/// Binds the items part once per run (a `Cx::shared` slot).
pub fn reach_items(cx: &Cx<'_>) -> Result<Arc<ItemsReach>, KindError> {
    struct Slot(Result<Arc<ItemsReach>, String>);
    let s = cx.shared.get_or_init(|| Slot(bind(cx).map(Arc::new).map_err(|e| e.to_string())));
    s.0.clone().map_err(KindError::Failed)
}

fn add_buff(buffs: &mut IndexMap<String, String>, p: &str) {
    if !buffs.contains_key(short_name(p)) {
        buffs.insert(short_name(p).to_owned(), p.to_owned());
    }
}

fn bind(cx: &Cx<'_>) -> Result<ItemsReach, KindError> {
    let mb = MontageBind::new(*cx, "items");
    let classes = ActorClasses::new(*cx, "items");
    let mode = defaults_or_properties(&cx.object(FFA_MODE_CDO)?)?;
    let ffa = class_list(mode.get(FFA_MODE_MEMBER), || {
        format!("items part: {} {FFA_MODE_MEMBER} is no class list", short_name(FFA_MODE_CDO))
    })?;

    // ---- actors, in the order of reaching (breadth first from the mode's list, then the centre class)
    let mut actors: IndexMap<String, Arc<ActorClass>> = IndexMap::new();
    let mut buffs: IndexMap<String, String> = IndexMap::new();
    let mut todo: Vec<String> = ffa.clone();
    todo.push(FFA_CENTER_ACTOR.to_owned());
    let mut i = 0;
    while i < todo.len() {
        let p = todo[i].clone();
        i += 1;
        let name = short_name(&p).to_owned();
        if actors.contains_key(&name) {
            continue;
        }
        let c = classes.actor(&p)?;
        actors.insert(name.clone(), c.clone());
        let rp = &c.raw_p;
        if let Some(Value::Str(x)) = rp.get("ActorClassSpawnedAtDeath") {
            todo.push(x.clone());
        }
        for m in ITEM_MEMBERS.get(&name)? {
            todo.extend(class_list(rp.get(*m), || format!("items part: {name} {m} is no class list"))?);
        }
        todo.extend(spawns_of(&name)?.into_iter().map(str::to_owned));
        for b in buffs_of(&name)? {
            add_buff(&mut buffs, b);
        }
        if let Some(Value::Arr(l)) = rp.get("PassiveBuffs") {
            for b in l {
                if let Value::Str(b) = b {
                    add_buff(&mut buffs, b);
                }
            }
        }
        for k in ON_HIT_MEMBERS {
            for b in rp.get(k).map(applied_buffs).unwrap_or_default() {
                if let Value::Str(b) = b {
                    add_buff(&mut buffs, &b);
                }
            }
        }
        for b in actor_passive_buffs(&c) {
            add_buff(&mut buffs, &b);
        }
    }

    // FFAGameMode's RespawnCatchUpBuffs (SpawnAndInitPlayer @1454-1799: the keys of the map, in its order). Every
    // fighter's data holds them (`fighter/actors.rs`), not what their graphs give a projectile (Buff_Enlarge's two
    // projectile buffs: GRAPH_BUFFS_SKIP), which the items part reaches through ITEM_BUFFS.
    let catch_up = defaults_or_properties(&cx.object(MODE_BUFFS_FROM.cdo)?)?;
    if let Some(Value::Arr(l)) = catch_up.get(MODE_BUFFS_FROM.member) {
        for e in l {
            if let Some(Value::Str(b)) = e.as_arr().and_then(|p| p.first()) {
                add_buff(&mut buffs, b);
            }
        }
    }

    // ---- buffs: what each one leads to (BUFF_SPAWN_MEMBERS, its graph literals), in order; a buff's graph that spawns
    // an actor (Buff_Chicken's heal pickup) adds that actor, bound as the others
    let mut i = 0;
    while i < buffs.len() {
        let n = buffs.get_index(i).map(|(k, _)| k.clone()).unwrap_or_default();
        i += 1;
        let mut lead: Vec<String> = Vec::new();
        for m in BUFF_SPAWN_MEMBERS.get(&n)? {
            let v = classes.raw_defaults(&buffs[&n])?;
            if let Some(Value::Str(x)) = v.get(*m) {
                lead.push(x.clone());
            }
        }
        lead.extend(buffs_of(&n)?.into_iter().map(str::to_owned));
        for b in lead {
            add_buff(&mut buffs, &b);
        }
        for a in spawns_of(&n)? {
            if !actors.contains_key(short_name(a)) {
                actors.insert(short_name(a).to_owned(), classes.actor(a)?);
            }
        }
    }
    let mut buff_defs: IndexMap<String, Props> = IndexMap::new();
    for (n, p) in &buffs {
        buff_defs.insert(n.clone(), classes.class_defaults(p)?);
    }

    // ---- the polymorph buffs' montages, and their props' anim sets with the blend spaces and composites those name
    let mut polymorphs: IndexMap<String, Props> = IndexMap::new();
    for (n, p) in &buffs {
        let raw = classes.raw_defaults(p)?;
        if matches!(raw.get("Polymorph Mesh"), Some(Value::Str(_))) {
            polymorphs.insert(n.clone(), raw);
        }
    }
    let mut montages = IndexMap::new();
    for raw in polymorphs.values() {
        for p in polymorph_montages(raw) {
            if !montages.contains_key(short_name(&p)) {
                montages.insert(short_name(&p).to_owned(), bind_montage(&mb, &p)?);
            }
        }
    }
    let mut poly_sets = IndexMap::new();
    let mut anim_assets = IndexMap::new();
    for raw in polymorphs.values() {
        let cls = match raw.get("PolymorphAnimClass") {
            Some(Value::Str(s)) => short_name(s),
            _ => "",
        };
        let Some((_, (bp_pkg, _))) = POLYMORPH_ANIM_LAYERS.iter().find(|(c, _)| *c == cls) else { continue };
        if poly_sets.contains_key(cls) {
            continue;
        }
        let ps = polymorph_anim_set(cx, "items", cls)?;
        let dir = &bp_pkg[..bp_pkg.rfind('/').unwrap_or(0)];
        let bs = cx.catalog.list_packages(dir, true, Some(BLEND_SPACE_1D));
        let comp = cx.catalog.list_packages(dir, true, Some(ANIM_COMPOSITE));
        for v in ps.set.values() {
            let Value::Str(v) = v else { continue };
            if anim_assets.contains_key(v) {
                continue;
            }
            let named = |p: &&String| last_segment(p).to_lowercase() == v.to_lowercase();
            let b = bs.iter().find(named);
            let c = if b.is_some() { None } else { comp.iter().find(named) };
            let hit = b.map(|p| (p, AnimAssetKind::BlendSpace)).or(c.map(|p| (p, AnimAssetKind::Composite)));
            if let Some((p, kind)) = hit {
                let props = properties(&cx.object(&asset_of(p))?)?;
                anim_assets.insert(v.clone(), anim_asset_entry(&props, kind)?);
            }
        }
        poly_sets.insert(cls.to_owned(), ps);
    }

    let mut defs: IndexMap<String, ActorDef> = actors.iter().map(|(n, c)| (n.clone(), c.def.clone())).collect();
    let mesh_of = |cls: &str, comp: &str, what: &str| -> Result<String, KindError> {
        match actors.get(cls).and_then(|a| a.raw_components.get(comp)).and_then(|c| c.get("SkeletalMesh")) {
            Some(Value::Str(m)) => Ok(m.clone()),
            _ => fail(format!("items part: {what}: {comp} has no SkeletalMesh")),
        }
    };
    for (cls, comps) in actor_bone_boxes(cx, &defs, &mesh_of)? {
        for (comp, track) in comps {
            let c = defs.get_mut(&cls).and_then(|d| d.components.get_mut(&comp));
            let Some(Value::Obj(c)) = c else { return fail(format!("items part: {cls}.{comp}: no component")) };
            c.insert("boneBox".into(), Value::Str(track));
        }
    }

    // ---- the montages the actors' own graphs play (GRAPH_MONTAGES), by name, as `fighter/actors.rs` binds them
    let mut actor_montages = IndexMap::new();
    for (n, c) in &actors {
        let supers = c.def.supers.clone().unwrap_or_default();
        for k in std::iter::once(n.as_str()).chain(supers.split(' ').filter(|x| !x.is_empty())) {
            for p in GRAPH_MONTAGES.get(k)? {
                if !actor_montages.contains_key(short_name(p)) {
                    actor_montages.insert(short_name(p).to_owned(), bind_montage(&mb, p)?);
                }
            }
        }
    }

    let data = ItemsData {
        ffa_items: ffa.iter().map(|p| short_name(p).to_owned()).collect(),
        center_actor: short_name(FFA_CENTER_ACTOR).to_owned(),
        buffs: buff_defs,
        actors: defs,
        montages,
        actor_montages,
        polymorph_anim_sets: poly_sets,
        anim_assets,
    };
    Ok(ItemsReach { data, classes: actors, polymorphs })
}

/// The last segment of a package path.
fn last_segment(p: &str) -> &str {
    &p[p.rfind('/').map_or(0, |i| i + 1)..]
}

/// The Items entry.
pub fn items(cx: &Cx<'_>) -> Result<ItemsData, KindError> {
    Ok(reach_items(cx)?.data.clone())
}
