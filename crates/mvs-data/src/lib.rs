//! The game's bound data as the sim reads it: the store's `gamedata` kind,
//! decoded into typed records. A match's data is put together from one entry per character, the stage's and the
//! shared tables, as the sim's `makeGameData` puts its `GameData` together
//! and its modules hold their tables beside it.
//!
//! The kind's entries (`<path>.<name>`):
//!   /GameData/Index      the lists: the roster, the data-only keys, what a character's match needs, the stages
//!   /GameData/Char/<k>   a character key's [`CharData`]
//!   /GameData/Stage/<k>  a stage's [`StageEntry`]
//!   /GameData/Items      the items part
//!   /GameData/Perks      the general perk table
//!   /GameData/RingOuts, /GameData/Announcer, /GameData/Arcade, /GameData/Mutators, /GameData/BotTrees
//!   /GameData/AnnouncerPacks  every announcer pack (the native importer's)
//!   /GameData/Input      the DataTable InputDefaults
//!   /GameData/Rumble     the ForceFeedbackEffects
//!   /GameData/Tutorials  every TutorialActor class's data (tutorial.rs)
//!   /GameData/BuffSupers every buff class's Blueprint parents (tables.rs [`BuffSupers`])
//!
//! Every value keeps the bits the bind gave it (value.rs); the canonical dump (dump.rs) of a record equal to the
//! dump of its entry's own value is the check that the types lose nothing.

pub mod boxes;
pub mod def;
pub mod dump;
pub mod fighter;
pub mod sound;
pub mod stage;
pub mod tables;
pub mod tutorial;
pub mod value;

use std::collections::HashSet;
use std::sync::Arc;

use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub use boxes::BoxIndex;
pub use def::{CharDef, DefExtra};
pub use fighter::FighterData;
pub use stage::{ItemsData, SpawnArea, StageData, StageManifest};
pub use tables::{
    AnnouncerPack, AnnouncerPacks, ArcadeBuffs, BotTrees, BuffSupers, Index, InputDefaults, MutatorsData, PerkTable,
    RingOutTable, RumbleChannel, RumbleEffect, RumbleTable, SkinAudio,
};
pub use tutorial::{TutorialActorData, Tutorials};
pub use value::{F32s, Props, Value};

/// The store kind.
pub const KIND: &str = "gamedata";
/// The kind's format this reader reads: the low three digits of the
/// store's version of the kind ([`format_of`]).
pub const KIND_VERSION: u64 = 2;

/// The format of a store's gamedata version: its low three digits. The native importer's version is
/// `fold * 1000 + KIND_VERSION` (the fold of its bind's and its in-process reads' versions, so a code change
/// rebuilds the kind), an earlier store's is `KIND_VERSION` alone; both are read alike.
pub const fn format_of(version: u64) -> u64 {
    version % 1000
}

/// The entry paths.
pub mod path {
    pub const INDEX: &str = "/GameData/Index.Index";
    pub const ITEMS: &str = "/GameData/Items.Items";
    pub const PERKS: &str = "/GameData/Perks.Perks";
    pub const RING_OUTS: &str = "/GameData/RingOuts.RingOuts";
    pub const ANNOUNCER: &str = "/GameData/Announcer.Announcer";
    pub const ANNOUNCER_PACKS: &str = "/GameData/AnnouncerPacks.AnnouncerPacks";
    pub const ARCADE: &str = "/GameData/Arcade.Arcade";
    pub const MUTATORS: &str = "/GameData/Mutators.Mutators";
    pub const BOT_TREES: &str = "/GameData/BotTrees.BotTrees";
    pub const INPUT: &str = "/GameData/Input.Input";
    pub const RUMBLE: &str = "/GameData/Rumble.Rumble";
    pub const TUTORIALS: &str = "/GameData/Tutorials.Tutorials";
    pub const BUFF_SUPERS: &str = "/GameData/BuffSupers.BuffSupers";
    pub fn char(key: &str) -> String {
        format!("/GameData/Char/{key}.{key}")
    }
    pub fn stage(key: &str) -> String {
        format!("/GameData/Stage/{key}.{key}")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error(transparent)]
    Store(#[from] mvs_store::StoreError),
    #[error("gamedata: the store holds version {found:?}, this reader reads format {KIND_VERSION} (run the import)")]
    Version { found: Option<u64> },
    #[error("gamedata {path}: {source}")]
    Decode { path: String, source: value::DecodeError },
    #[error("gamedata: no character {0}")]
    NoChar(String),
    #[error("gamedata: no stage {0}")]
    NoStage(String),
}

/// One character key's data: `CharData` (its fighter data as `makeGameData` leaves it, the box index and
/// frames, the definition), what `charDef` keeps beside the definition, its perk part and its skin audio (`None`
/// for a data-only key).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CharData {
    pub key: String,
    pub fighter: FighterData,
    pub boxes: BoxIndex,
    pub box_data: F32s,
    pub def: CharDef,
    pub def_extra: DefExtra,
    pub perks: Option<PerkTable>,
    pub skin_audio: Option<SkinAudio>,
}

/// A stage's data, its manifest entry and the spawn areas the map's spawn area actors read.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageEntry {
    pub key: String,
    pub data: StageData,
    pub manifest: StageManifest,
    pub spawn_areas: Vec<SpawnArea>,
}

/// One match's data: its characters' and its stage's, the items part and the shared tables.
#[derive(Clone, Debug)]
pub struct GameData {
    /// By key, in code-unit order: the default character, the match's and
    /// the data-only keys they need. Each is the store's one copy ([`char_data`]).
    pub chars: IndexMap<String, Arc<CharData>>,
    pub stage: StageData,
    pub items: Option<ItemsData>,
    pub default_character: String,
    pub stage_key: String,
    pub stage_manifest: StageManifest,
    /// The spawn areas of the stage's map.
    pub spawn_areas: Vec<SpawnArea>,
    pub perks: PerkTable,
    pub ring_outs: RingOutTable,
    pub announcer: AnnouncerPack,
    pub arcade: ArcadeBuffs,
    pub mutators: MutatorsData,
    pub bot_trees: BotTrees,
    pub input_defaults: InputDefaults,
    pub rumble: RumbleTable,
    /// Every TutorialActor class's data; empty where the store has none (sim-trace takes a tutorial
    /// match's data from its header).
    pub tutorials: Tutorials,
    /// Every buff class's Blueprint parents; empty where the store has none (a buff is then of its own
    /// class alone).
    pub buff_supers: BuffSupers,
}

impl GameData {
    /// The DataTable InputDefaults (the key bindings' defaults).
    pub fn input_defaults(&self) -> &InputDefaults {
        &self.input_defaults
    }

    /// The ForceFeedbackEffect `name` (a sim `rumble` event's effect), None when the build has none of that name.
    pub fn rumble(&self, name: &str) -> Option<&RumbleEffect> {
        self.rumble.get(name)
    }

    /// A match's data from the store: `keys` the characters it plays, `stage` its stage key. The default character
    /// joins them (makeGameData's `fighter`), and the data-only keys their match needs (Index.matchNeeds); the items
    /// part is read always, as the node runs' game data holds it.
    // Deviation: the trace writer's data holds the whole roster in roster order
    // and every data-only key; this holds the match's keys alone, sorted. The World picks its characters in roster
    // order and its minions reach only the keys the match needs, so no tick sees it. A mode's cast is the caller's to
    // pass in `keys`.
    pub fn load(store: &mvs_store::Store, keys: &[&str], stage: &str) -> Result<GameData, DataError> {
        let found = store.version(KIND);
        if found.map(format_of) != Some(KIND_VERSION) {
            return Err(DataError::Version { found });
        }
        let pack = store.kind(KIND)?;
        let index: Arc<Index> = shared(store, path::INDEX)?;
        let mut all: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        let mut add = |k: &str, all: &mut Vec<String>| {
            if seen.insert(k.to_string()) {
                all.push(k.to_string());
            }
        };
        add(&index.default_character, &mut all);
        for k in keys {
            add(k, &mut all);
        }
        for k in all.clone() {
            for n in index.match_needs.get(&k).into_iter().flatten() {
                add(n, &mut all);
            }
        }
        all.sort();
        let mut chars = IndexMap::new();
        for k in all {
            let p = path::char(&k);
            if !pack.has(&p) {
                return Err(DataError::NoChar(k));
            }
            chars.insert(k, shared::<CharData>(store, &p)?);
        }
        let sp = path::stage(stage);
        if !pack.has(&sp) {
            return Err(DataError::NoStage(stage.to_string()));
        }
        let st: Arc<StageEntry> = shared(store, &sp)?;
        // (the tables are the store's copies, cloned: a match's data owns them)
        fn copy<T: DeserializeOwned + Clone + Send + Sync + 'static>(
            store: &mvs_store::Store,
            path: &str,
        ) -> Result<T, DataError> {
            Ok(T::clone(&*shared::<T>(store, path)?))
        }
        Ok(GameData {
            chars,
            stage: st.data.clone(),
            items: Some(copy(store, path::ITEMS)?),
            default_character: index.default_character.clone(),
            stage_key: st.key.clone(),
            stage_manifest: st.manifest.clone(),
            spawn_areas: st.spawn_areas.clone(),
            perks: copy(store, path::PERKS)?,
            ring_outs: copy(store, path::RING_OUTS)?,
            announcer: copy(store, path::ANNOUNCER)?,
            arcade: copy(store, path::ARCADE)?,
            mutators: copy(store, path::MUTATORS)?,
            bot_trees: copy(store, path::BOT_TREES)?,
            input_defaults: copy(store, path::INPUT)?,
            rumble: copy(store, path::RUMBLE)?,
            tutorials: if pack.has(path::TUTORIALS) { copy(store, path::TUTORIALS)? } else { Tutorials::new() },
            buff_supers: if pack.has(path::BUFF_SUPERS) { copy(store, path::BUFF_SUPERS)? } else { BuffSupers::new() },
        })
    }

    /// The default character's data (the GameData's `fighter`, `boxes`, `boxData`).
    pub fn default_char(&self) -> &CharData {
        &self.chars[&self.default_character]
    }
}

/// The stages the store has, each as (its gameplay level's package name, its key), in the manifest's order: the
/// table behind a stage's lookup by its gameplay level's package
/// name. An empty list where the store has no gamedata kind of this version (a menu with no game data).
pub fn stage_levels(store: &mvs_store::Store) -> Result<Vec<(String, String)>, DataError> {
    if store.version(KIND).map(format_of) != Some(KIND_VERSION) {
        return Ok(Vec::new());
    }
    let index: Arc<Index> = shared(store, path::INDEX)?;
    let mut out = Vec::new();
    for key in &index.stages {
        let st: Arc<StageEntry> = shared(store, &path::stage(key))?;
        let gameplay = st.manifest.levels.iter().find(|l| l.1 == "gameplay");
        if let Some(l) = gameplay {
            out.push((l.0.rsplit('/').next().unwrap_or(&l.0).to_owned(), key.clone()));
        }
    }
    Ok(out)
}

/// One entry of the store's kind, decoded and typed once per store and shared after (the store's cache, weighed by
/// the entry's size): a second match of the same characters reads nothing.
pub fn shared<T: DeserializeOwned + Send + Sync + 'static>(
    store: &mvs_store::Store,
    path: &str,
) -> Result<Arc<T>, DataError> {
    let pack = store.kind(KIND)?;
    let size = pack.record(path).map_or(0, |r| r.size as usize);
    store.cached(&format!("{KIND}:{path}"), || entry::<T>(&pack, path), |_| size)
}

/// A character key's data, the store's one copy ([`shared`]).
pub fn char_data(store: &mvs_store::Store, key: &str) -> Result<Arc<CharData>, DataError> {
    let p = path::char(key);
    if !store.kind(KIND)?.has(&p) {
        return Err(DataError::NoChar(key.to_owned()));
    }
    shared(store, &p)
}

/// The announcer pack this machine hears (PreferencesManager LoadStoredAnnouncerPackAssetData: the pack at
/// GlobalEquippables.AnnouncerPack.AssetPath, `path`, loaded @200; DefaultAnnouncerPack for an empty path or one that
/// is no AnnouncerPackData, @390 / @414). A store whose gamedata kind
/// holds no pack list (one imported before it) gives the default pack.
pub fn stored_announcer_pack(store: &mvs_store::Store, path: &str) -> Result<AnnouncerPack, DataError> {
    let found = store.version(KIND);
    if found.map(format_of) != Some(KIND_VERSION) {
        return Err(DataError::Version { found });
    }
    let pack = store.kind(KIND)?;
    if !path.is_empty() && pack.has(path::ANNOUNCER_PACKS) {
        let packs: AnnouncerPacks = entry(&pack, path::ANNOUNCER_PACKS)?;
        if let Some(p) = packs.get(path) {
            return Ok(p.clone());
        }
    }
    entry(&pack, path::ANNOUNCER)
}

/// One entry of the kind, decoded and typed.
pub fn entry<T: DeserializeOwned>(pack: &mvs_store::Pack, path: &str) -> Result<T, DataError> {
    let bytes = pack.bytes(path)?;
    decode(&bytes).map_err(|source| DataError::Decode { path: path.to_string(), source })
}

/// An entry's bytes, typed.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, value::DecodeError> {
    value::from_value(value::decode_entry(bytes)?)
}

/// The typed record of an entry by its path, as its canonical dump (the bin gamedata-dump's half of the check).
pub fn dump_entry(path: &str, bytes: &[u8]) -> Result<String, String> {
    struct Dump;
    impl Typed for Dump {
        type Out = String;
        fn go<T: DeserializeOwned + Serialize>(&self, bytes: &[u8]) -> Result<String, String> {
            let v: T = decode(bytes).map_err(|e| e.to_string())?;
            dump::dump(&v).map_err(|e| e.to_string())
        }
    }
    by_path(path, bytes, Dump)
}

/// An entry decoded into the typed record of its path and encoded again (`value::encode`): the bytes the native
/// kind writes of the same record, equal to the entry's bytes when the types and the encoder lose nothing.
pub fn reencode_entry(path: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    struct Reencode;
    impl Typed for Reencode {
        type Out = Vec<u8>;
        fn go<T: DeserializeOwned + Serialize>(&self, bytes: &[u8]) -> Result<Vec<u8>, String> {
            let v: T = decode(bytes).map_err(|e| e.to_string())?;
            value::encode(&v).map_err(|e| e.to_string())
        }
    }
    by_path(path, bytes, Reencode)
}

/// Something done with an entry's typed record.
trait Typed {
    type Out;
    fn go<T: DeserializeOwned + Serialize>(&self, bytes: &[u8]) -> Result<Self::Out, String>;
}

/// `f` over the entry as the typed record of its path.
fn by_path<F: Typed>(path: &str, bytes: &[u8], f: F) -> Result<F::Out, String> {
    match path {
        path::INDEX => f.go::<Index>(bytes),
        path::ITEMS => f.go::<ItemsData>(bytes),
        path::PERKS => f.go::<PerkTable>(bytes),
        path::RING_OUTS => f.go::<RingOutTable>(bytes),
        path::ANNOUNCER => f.go::<AnnouncerPack>(bytes),
        path::ANNOUNCER_PACKS => f.go::<AnnouncerPacks>(bytes),
        path::ARCADE => f.go::<ArcadeBuffs>(bytes),
        path::MUTATORS => f.go::<MutatorsData>(bytes),
        path::BOT_TREES => f.go::<BotTrees>(bytes),
        path::INPUT => f.go::<InputDefaults>(bytes),
        path::RUMBLE => f.go::<RumbleTable>(bytes),
        path::TUTORIALS => f.go::<Tutorials>(bytes),
        path::BUFF_SUPERS => f.go::<BuffSupers>(bytes),
        p if p.starts_with("/GameData/Char/") => f.go::<CharData>(bytes),
        p if p.starts_with("/GameData/Stage/") => f.go::<StageEntry>(bytes),
        p => Err(format!("no entry kind for {p}")),
    }
}
