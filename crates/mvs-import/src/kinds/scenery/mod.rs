//! The `scenery` kind: per stage of the manifest, its scenery as the native renderer draws it (`mvs-render`
//! stage.rs), bound from its levels: a json-bin container (SCENERY_MAGIC) whose header holds the levels, lightmaps,
//! meshes, instances, emitters, skeletal meshes and the parts' blocks, and whose body holds the binary sections. Its
//! maps' own Blueprint classes and level Blueprints are in [`parts`] ([`part`] the shapes they hook in by).
//!
//! Reads the stages (manifest and data) from the store's gamedata kind; the levels, meshes, animations and skeletons
//! in-process (`Cx::object`, `mesh::header`, `animation::header`, `skeleton::entry`).

mod bind;
mod border;
mod landscape;
mod materials;
pub mod part;
pub mod parts;

use indexmap::IndexSet;
use mvs_material::jsonbin::JsonBin;
use serde::Deserialize;

use crate::game::{GAMEDATA, SCENERY_KIND_VERSION, exported, scenery_entry_path, stages};
use crate::kind::{Cx, Item, Kind, KindError};

/// The entry's magic.
pub const SCENERY_MAGIC: &[u8; 4] = b"MVSC";

/// The header members other kinds read (`SceneryHeader`'s `materials`, `effects`, `decals`, `systems`).
#[derive(Deserialize)]
struct NamedHeader {
    materials: Vec<String>,
    effects: Vec<String>,
    decals: Vec<String>,
    systems: Vec<String>,
}

fn named_header(bytes: &[u8]) -> Result<NamedHeader, KindError> {
    let jb = JsonBin::split(SCENERY_MAGIC, bytes, "scenery").map_err(|e| KindError::Failed(e.to_string()))?;
    jb.parse("scenery").map_err(|e| KindError::Failed(e.to_string()))
}

/// The DrawRefs an entry names (`sceneryDrawRefs`: the header's materials, effects and decals), what the material
/// kind's list reads.
pub fn draw_refs(bytes: &[u8], out: &mut IndexSet<String>) -> Result<(), KindError> {
    let h = named_header(bytes)?;
    out.extend(h.materials.into_iter().chain(h.effects).chain(h.decals));
    Ok(())
}

/// The particle systems an entry's emitters and blocks spawn (`sceneryParticleSystems`: its header's `systems`):
/// the particles kind binds them.
pub fn particle_systems(bytes: &[u8]) -> Result<Vec<String>, KindError> {
    Ok(named_header(bytes)?.systems)
}

/// Every stage of the manifest (`sceneryStages`: every stage has a scenery entry).
pub fn items(cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
    Ok(stages(cx)?.keys.iter().map(|k| Item::new(exported(&scenery_entry_path(k)))).collect())
}

pub struct Scenery;

impl Kind for Scenery {
    fn id(&self) -> &'static str {
        "scenery"
    }

    fn version(&self) -> u64 {
        SCENERY_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/octet-stream"
    }

    /// [`items`].
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        items(cx)
    }

    /// The entry of the stage the item's path names (its last name), [`bind::bind`].
    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let seg = &item.path[item.path.rfind('/').map_or(0, |i| i + 1)..];
        let key = &seg[..seg.find('.').unwrap_or(seg.len())];
        if item.path != exported(&scenery_entry_path(key)) {
            return Err(KindError::NotFound(format!("scenery: {} is no entry of the kind", item.path)));
        }
        bind::bind(cx, key)
    }

    fn reads(&self) -> &'static [(&'static str, u64)] {
        &[GAMEDATA]
    }
}
