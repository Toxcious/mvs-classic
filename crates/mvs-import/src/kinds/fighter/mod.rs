//! The `fighter` kind: per key of the port's fighters (`fighter_port::FIGHTER_PORT`), the materials its skins and
//! actors draw, and the BlendSpacePlayer nodes its drawing runs:
//! `{key, bindVersion, objectVersion, textVersion, culture, skinSlots, actors, blendSpaces}`, JSON.
//!
//! The entry holds the draws alone: the key's fighter data and skin audio (`data`, `skinAudio`) are read from the
//! gamedata kind; `compare` leaves the two keys out of an earlier entry.
//! Deviation: the entry holds `blendSpaces` (blend.rs BlendSpacePlayerData by `<AnimBP class>:<node>`, the nodes of
//! `fighter_port::BLEND_SPACES`), which an earlier bind took from the object kind at load: the game's store keeps
//! no object kind. Its bind version is the earlier one plus one (13); `compare` leaves `blendSpaces` out.
//!
//! Reads the fighters' data from the store's gamedata kind (`game::chars`); the skins, meshes and actors in-process
//! (`Cx::object`, `mesh::header`).

use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

use self::draws::{Actor, Draws, SkinMesh, bind_fighter_draws};
use crate::fighter_port::{CHARACTER_DATA, FIGHTER_PORT, blend_spaces, fighter_port};
use crate::game::{self, FIGHTER_BIND_VERSION, FIGHTER_KIND_VERSION, GAMEDATA, exported, fighter_entry_path};
use crate::kind::{Cx, Item, Kind, KindError};
use crate::kinds::gamedata::blend::{BlendSpacePlayerData, bind_blend_space_players};
use crate::kinds::object::OBJECT_KIND_VERSION;
use crate::kinds::text::{GAME_CULTURE, TEXT_KIND_VERSION};

pub mod draws;
pub mod table;

/// The armor overlay's two materials: named with every fighter's skin slots.
pub const ARMOR_OVERLAY: [&str; 2] = [
    "/Game/Panda_Main/Characters/Superman/VFX/ArmorBuffOverlay_MAT.ArmorBuffOverlay_MAT?ctx=skin",
    "/Game/Panda_Main/Characters/Superman/VFX/ArmorBuffOverlay_Yellow_MAT.ArmorBuffOverlay_Yellow_MAT?ctx=skin",
];

#[derive(Deserialize)]
struct Named {
    #[serde(rename = "skinSlots")]
    skin_slots: Option<IndexMap<String, IndexMap<String, SkinMesh>>>,
    actors: Option<IndexMap<String, Actor>>,
}

/// The DrawRefs an entry names (the skin slots' variants and the actors' materials and
/// variants, then the armor overlay), what the material kind's list reads.
pub fn draw_refs(bytes: &[u8], out: &mut IndexSet<String>) -> Result<(), KindError> {
    let e: Named = serde_json::from_slice(bytes).map_err(|e| KindError::Failed(format!("an entry: {e}")))?;
    // (a null slot names nothing)
    let add = |out: &mut IndexSet<String>, l: &[Option<String>]| out.extend(l.iter().flatten().cloned());
    for skin in e.skin_slots.iter().flat_map(IndexMap::values) {
        for m in skin.values() {
            for l in [&m.red, &m.blue, &m.local, &m.menu] {
                add(out, l);
            }
        }
    }
    for a in e.actors.iter().flat_map(IndexMap::values) {
        add(out, &a.materials);
        for l in a.variants.iter().flat_map(IndexMap::values) {
            add(out, l);
        }
    }
    if e.skin_slots.is_some() {
        out.extend(ARMOR_OVERLAY.map(str::to_owned));
    }
    Ok(())
}

/// The key of an entry path.
pub fn key_of(path: &str) -> Option<&'static str> {
    FIGHTER_PORT.iter().map(|(k, _)| *k).find(|k| exported(&fighter_entry_path(k)) == path)
}

/// Every key of the port's fighters, in its order.
pub fn items() -> Vec<Item> {
    FIGHTER_PORT.iter().map(|(k, _)| Item::new(exported(&fighter_entry_path(k)))).collect()
}

/// An entry as written: `{key, bindVersion, objectVersion, textVersion, culture, skinSlots, actors, blendSpaces}` (an
/// earlier entry has no `blendSpaces` and also holds `data` and `skinAudio`, between `culture` and `skinSlots`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    key: &'static str,
    bind_version: u64,
    object_version: u32,
    text_version: u64,
    culture: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    skin_slots: Option<IndexMap<String, IndexMap<String, SkinMesh>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actors: Option<IndexMap<String, Actor>>,
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    blend_spaces: IndexMap<String, BlendSpacePlayerData>,
}

pub struct Fighter;

impl Kind for Fighter {
    fn id(&self) -> &'static str {
        "fighter"
    }

    fn version(&self) -> u64 {
        FIGHTER_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/json"
    }

    /// [`items`].
    fn list(&self, _: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(items())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let key = key_of(&item.path)
            .ok_or_else(|| KindError::NotFound(format!("fighter_import: no key for {}", item.path)))?;
        let chars = game::chars(cx)?;
        let port =
            fighter_port(key).ok_or_else(|| KindError::NotFound(format!("fighter {key}: not in the port table")))?;
        let character_data = CHARACTER_DATA.iter().find(|(k, _)| *k == key).map(|(_, p)| *p).or(port.character_data);
        let Draws { skin_slots, actors } = bind_fighter_draws(cx, key, port, character_data, &chars.get(key)?.fighter)?;
        let entry = Entry {
            key,
            bind_version: FIGHTER_BIND_VERSION,
            object_version: OBJECT_KIND_VERSION,
            text_version: TEXT_KIND_VERSION,
            culture: GAME_CULTURE,
            skin_slots,
            actors,
            blend_spaces: bind_blend_space_players(cx, blend_spaces(key))?,
        };
        crate::json::to_vec(&entry).map_err(|e| KindError::Failed(format!("fighter {key}: {e}")))
    }

    fn reads(&self) -> &'static [(&'static str, u64)] {
        &[GAMEDATA]
    }
}
