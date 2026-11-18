//! The items part's drawing: what a match with random items
//! (FFAGameMode's MapRandomActorSpawnArea) spawns with no owner fighter, drawn from the stage kind's items entry
//! `draws` (each mesh component's mesh, slot materials, team sets and sequences, by '<class>.<component>'), whose
//! meshes, materials and clips no fighter's assets hold.
//!
//! An item module takes what it draws with from the host (crate::actors::ActorHost): `item_asset` (a component's
//! mesh), `item_materials` (its slots in a set), `item_clip` (its AnimToPlay or a sequence of its class), and the
//! views made of them, `StaticView::item` and `SkinnedActor::item` (crate::actor_view). A class's own drawing is a
//! mesh rule in [`RULES`] (an item or projectile of the class, or a World actor of it); what has none and comes from
//! the items part is [`class_view`]'s.

use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;
use serde::Deserialize;

use mvs_sim::World;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::common::actor_def_owner;
use mvs_sim::names::Name;

use crate::actors::{ActorComponents, ActorHost, Component};
use crate::anim::Clips;
use crate::mesh::MeshInfo;
use crate::skins::FighterDraws;
use crate::{Error, Result};

pub(crate) mod banana_guard;
pub(crate) mod bounce_pad;
pub(crate) mod car;
pub(crate) mod class_view;
pub(crate) mod meeseeks;
pub(crate) mod neptr;
pub(crate) mod snowball_head;
pub(crate) mod tv;

/// Every item module's mesh rules (resolved once into crate::actors::mesh_rule's table with the characters').
pub(crate) static RULES: [&[(&str, crate::actors::MeshRule)]; 6] =
    [tv::RULES, bounce_pad::RULES, banana_guard::RULES, neptr::RULES, car::RULES, meeseeks::RULES];

/// The clips a class's view plays from: the character whose data has the class (its clips, by name), or the items
/// part's (its draw's `clips`, by name: [`ItemAssets::load`]).
#[derive(Clone, Copy, Debug)]
pub(crate) enum ClipSet {
    Fighter(FighterId),
    Items,
}

impl ClipSet {
    /// The clip set (None: the character's or the items part's is not loaded).
    pub(crate) fn clips<'h>(self, host: &'h mut ActorHost<'_>, w: &World) -> Option<&'h mut Clips> {
        match self {
            ClipSet::Fighter(f) => host.clips_of(w, f),
            ClipSet::Items => host.items.as_deref_mut().map(|i| &mut i.clips),
        }
    }
}

/// The fighter whose character data has class `cls` (`actor_def_owner`: the owner's, else a peer's), None with
/// none: the class is drawn from the items part's draws then.
pub(crate) fn data_fighter(w: &World, owner: FighterId, cls: Name) -> Option<FighterId> {
    let from = actor_def_owner(w, owner, cls);
    w.char_data(from).fighter.actors.as_ref().is_some_and(|a| a.contains_key(w.names.str(cls))).then_some(from)
}

/// Component `name` of class `cls`'s data: `cs` (the class data the actor's view resolved), else the items part's
/// (`itemsDataOf(world).actors[cls].components`, for a class no fighter of the match has).
pub(crate) fn component(w: &World, cs: &ActorComponents, cls: Name, name: &str) -> Option<Component> {
    if let Some(c) = cs.get(name) {
        return Some(c.clone());
    }
    let def = mvs_sim::world::flow::items_data_of(w)?.actors.get(w.names.str(cls))?;
    ActorComponents::resolve(def, &[]).get(name).cloned()
}

/// One draw of the items entry: the component's mesh, the sequence an AnimationSingleNode
/// component plays (`anim`) and the sequences its class's AnimBP and graph play by name (`clips`). Its materials
/// and team sets are read through [`ItemAssets::materials`].
#[derive(Debug, Deserialize)]
struct ItemDraw {
    mesh: String,
    #[serde(default)]
    anim: Option<String>,
    #[serde(default)]
    clips: IndexMap<String, String>,
}

#[derive(Deserialize)]
struct ItemsEntry {
    draws: Option<IndexMap<String, ItemDraw>>,
}

/// The items part's drawing assets, loaded with the views of a match with random items.
#[derive(Debug)]
pub struct ItemAssets {
    draws: IndexMap<String, ItemDraw>,
    /// The draws as an actors table (FighterDraws.actor's rule: a variant's set, else the template's `materials`).
    sets: FighterDraws,
    /// Each draw's mesh, by its path.
    meshes: HashMap<String, Arc<MeshInfo>>,
    /// Every sequence the draws name, keyed by its object path.
    pub(crate) clips: Clips,
}

/// The items entry's path tail in the stage kind (`/Stage/b..o..c..l..m../Items`,
/// as the store names the export).
const ITEMS_TAIL: &str = "/Items.Items";

impl ItemAssets {
    /// `loadItemAssets`: the items entry's draws, their meshes and their sequences, from `store`.
    pub fn load(store: &mvs_store::Store) -> Result<ItemAssets> {
        let stage = store.kind("stage")?;
        let path = stage
            .paths()
            .find(|p| p.starts_with("/Stage/") && p.ends_with(ITEMS_TAIL))
            .ok_or_else(|| Error::Format { what: "the stage kind".into(), why: "no items entry".into() })?
            .to_owned();
        let bytes = stage.bytes(&path)?;
        let fmt = |why: String| Error::Format { what: path.clone(), why };
        let e: ItemsEntry = serde_json::from_slice(&bytes).map_err(|e| fmt(e.to_string()))?;
        let draws = e.draws.ok_or_else(|| fmt("the items part has no draws (a store imported before them)".into()))?;
        let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| fmt(e.to_string()))?;
        let sets = FighterDraws::of_actors(v.get("draws").cloned().unwrap_or_default(), store.kind("material")?);
        let mut meshes = HashMap::new();
        for d in draws.values() {
            if !meshes.contains_key(&d.mesh) {
                meshes.insert(d.mesh.clone(), MeshInfo::shared(store, &d.mesh)?);
            }
        }
        let mut anims: IndexMap<&str, &str> = IndexMap::new();
        for d in draws.values() {
            for p in d.anim.iter().chain(d.clips.values()) {
                anims.insert(p, p);
            }
        }
        let mut clips = Clips::load(store, anims)?;
        // The class's sequences by their names too: a machine or a montage names a sequence by it (the draw's `clips`
        // is per class, its names are the sequences' own: a name two classes share is the first's).
        for (key, d) in &draws {
            for (name, path) in &d.clips {
                let Some(&k) = clips.by_name.get(path.as_str()) else { continue };
                match clips.by_name.get(name.as_str()) {
                    Some(&o) if o != k => {
                        eprintln!(
                            "warning: items part: {key}'s sequence {name} is another class's already: not aliased"
                        );
                    }
                    _ => {
                        clips.by_name.insert(name.clone(), k);
                    }
                }
            }
        }
        Ok(ItemAssets { draws, sets, meshes, clips })
    }

    /// `mesh`: component `key`'s mesh, its path and header.
    pub fn mesh(&self, key: &str) -> Option<(&str, &Arc<MeshInfo>)> {
        let d = self.draws.get(key)?;
        Some((d.mesh.as_str(), self.meshes.get(&d.mesh)?))
    }

    /// The draws' keys, in the entry's order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.draws.keys().map(String::as_str)
    }

    /// `mats.actor(key, variant)`: component `key`'s slot materials in set `variant` ('template' or none: the
    /// component's own; 'red', 'blue', 'local': SetTeamColor's, where the class has them), None with no such draw.
    pub fn materials(&self, key: &str, variant: Option<&str>) -> Option<Vec<Option<String>>> {
        self.sets.actor(key, variant).map(|a| a.1)
    }

    /// `clip`: component `key`'s AnimToPlay with no `name`, else its class's sequence of that name: its index in
    /// [`ItemAssets::clips`].
    pub fn clip(&self, key: &str, name: Option<&str>) -> Option<usize> {
        let d = self.draws.get(key)?;
        let p = match name {
            None => d.anim.as_deref()?,
            Some(n) => d.clips.get(n)?.as_str(),
        };
        self.clips.get(p)
    }

    /// The warm-up's sets: each draw's mesh with its template's and its team sets' materials.
    pub(crate) fn warm_sets(&self) -> Vec<(String, Vec<Vec<Option<String>>>)> {
        self.sets.actor_sets()
    }
}
