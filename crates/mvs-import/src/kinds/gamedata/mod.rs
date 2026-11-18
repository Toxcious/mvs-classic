//! The `gamedata` kind: everything the sim reads at match start, bound from the build at import (`mvs_data` reads it
//! at load): the lists (`/GameData/Index`), per character key its data (`Char/<k>`), per stage its data and manifest
//! entry (`Stage/<k>`), the items part and the shared tables. Each
//! entry is its `mvs_data` record written by `mvs_data::value::encode` (an earlier store's entries are the check, byte
//! for byte).
//!
//! Every input is read in-process (`Cx::object`, the text, config, lighting, mesh, skeleton and animation reads), so
//! the kind reads no other kind of the store; their versions are in its own (`game::GAMEDATA_KIND_VERSION`).

use mvs_data::path;
use serde::Serialize;

use crate::kind::{Cx, Item, Kind, KindError};

pub mod actor_class;
pub mod announcer;
pub mod arcade;
pub mod blend;
pub mod bot_trees;
pub mod boxes;
pub mod buff_supers;
pub mod char;
pub mod def;
pub mod fighter;
pub mod input;
pub mod items;
pub mod mode_reach;
pub mod mutators;
pub mod perks;
pub mod profiles;
pub mod reach;
pub mod ring_outs;
pub mod rumble;
pub mod shape;
pub mod skin_audio;
pub mod stage;
pub mod transform;
pub mod tutorial;
pub mod vo_rows;

/// The stages: a key per gameplay level of the MapData assets, then the levels no MapData names that a game mode
/// opens by name (every tutorial's map), in the order every stage table keys them (the sim's stage table).
pub const STAGES: [&str; 21] = [
    "trainingroomsmall",
    "m011_largenoplat",
    "m011_smallnoplat",
    "m011",
    "m011_small",
    "classic3platform_small",
    "classic3platform_large",
    "trainingroomlarge",
    "trophyroom_largeplatform",
    "trophyroom_2platforms",
    "treehouse",
    "treehousesmall",
    "treehouse_1plat_large",
    "m009",
    "m009_small",
    "scoobydoo",
    "scoobydoo_noroof",
    "batcave",
    "m008",
    "m010",
    "trophyroom_tutorial",
];

/// The stage a match falls back on.
pub const DEFAULT_STAGE: &str = "trainingroomsmall";

/// The characters a match with this one needs beside its players': Rick's
/// Buff_C020Polymorph plays the Creature's mesh and montages on its victim; Velma's police car
/// (Character_VelmaAIBotPolice, spawned by Pawn_Velma's SpawnAIFromClass @5963) is the data-only key velmapolice.
pub const MATCH_NEEDS: &[(&str, &[&str])] = &[("rick", &["creature"]), ("velma", &["velmapolice"])];

/// The needed keys outside the roster, sorted.
pub fn data_only() -> Vec<&'static str> {
    let mut out: Vec<&str> = Vec::new();
    for (_, needs) in MATCH_NEEDS {
        for n in *needs {
            if !mvs_protocol::roster::CHARACTERS.contains(n) && !out.contains(n) {
                out.push(n);
            }
        }
    }
    out.sort_unstable();
    out
}

/// The Index entry (gameDataKind's INDEX bind).
pub fn index() -> mvs_data::Index {
    let s = |l: &[&str]| l.iter().map(|k| (*k).to_owned()).collect::<Vec<_>>();
    mvs_data::Index {
        characters: s(&mvs_protocol::roster::CHARACTERS),
        data_only: s(&data_only()),
        match_needs: MATCH_NEEDS.iter().map(|(k, n)| ((*k).to_owned(), s(n))).collect(),
        default_character: mvs_protocol::roster::DEFAULT_CHARACTER.to_owned(),
        stages: s(&STAGES),
        default_stage: DEFAULT_STAGE.to_owned(),
    }
}

/// What an entry path names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry<'p> {
    Index,
    Char(&'p str),
    Stage(&'p str),
    Items,
    Perks,
    RingOuts,
    Announcer,
    AnnouncerPacks,
    Arcade,
    Mutators,
    BotTrees,
    Input,
    Rumble,
    Tutorials,
    BuffSupers,
}

/// The tables' paths and entries, in the list's order after the stages.
const TABLES: [(&str, Entry<'static>); 12] = [
    (path::ITEMS, Entry::Items),
    (path::PERKS, Entry::Perks),
    (path::RING_OUTS, Entry::RingOuts),
    (path::ANNOUNCER, Entry::Announcer),
    (path::ARCADE, Entry::Arcade),
    (path::MUTATORS, Entry::Mutators),
    (path::BOT_TREES, Entry::BotTrees),
    (path::INPUT, Entry::Input),
    (path::RUMBLE, Entry::Rumble),
    (path::ANNOUNCER_PACKS, Entry::AnnouncerPacks),
    (path::TUTORIALS, Entry::Tutorials),
    (path::BUFF_SUPERS, Entry::BuffSupers),
];

/// The entry a listed path names, `None` for a path the kind does not list.
pub fn entry_of(p: &str) -> Option<Entry<'_>> {
    if p == path::INDEX {
        return Some(Entry::Index);
    }
    if let Some(&(_, e)) = TABLES.iter().find(|(t, _)| *t == p) {
        return Some(e);
    }
    let keyed = |dir: &str| {
        let rest = p.strip_prefix(dir)?;
        let (key, name) = rest.split_once('.')?;
        (key == name && !key.contains('/')).then_some(key)
    };
    if let Some(k) = keyed("/GameData/Char/").filter(|k| crate::fighter_port::fighter_port(k).is_some()) {
        return Some(Entry::Char(k));
    }
    if let Some(k) = keyed("/GameData/Stage/").filter(|k| STAGES.contains(k)) {
        return Some(Entry::Stage(k));
    }
    None
}

/// Every entry path, in gameDataKind's order: the Index, each FIGHTER_PORT key's, each stage's, then the tables.
pub fn paths() -> Vec<String> {
    let mut out = vec![path::INDEX.to_owned()];
    out.extend(crate::fighter_port::FIGHTER_PORT.iter().map(|(k, _)| path::char(k)));
    out.extend(STAGES.iter().map(|k| path::stage(k)));
    out.extend(TABLES.iter().map(|(p, _)| (*p).to_owned()));
    out
}

/// An entry's record, encoded.
fn encode<T: Serialize>(p: &str, v: &T) -> Result<Vec<u8>, KindError> {
    mvs_data::value::encode(v).map_err(|e| KindError::Failed(format!("{p}: {e}")))
}

/// The `gamedata` kind.
pub struct GameData;

impl Kind for GameData {
    fn id(&self) -> &'static str {
        mvs_data::KIND
    }

    fn version(&self) -> u64 {
        crate::game::GAMEDATA_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/octet-stream"
    }

    /// [`paths`].
    fn list(&self, _: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(paths().into_iter().map(Item::new).collect())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let p = item.path.as_str();
        let e = entry_of(p).ok_or_else(|| KindError::NotFound(format!("gamedata: no entry {p}")))?;
        match e {
            Entry::Index => encode(p, &index()),
            Entry::Char(k) => encode(p, &char::char_entry(cx, k)?),
            Entry::Stage(k) => encode(p, &stage::stage_entry(cx, k)?),
            Entry::Items => encode(p, &items::items(cx)?),
            Entry::Perks => encode(p, &*perks::general(cx)?),
            Entry::RingOuts => encode(p, &ring_outs::ring_outs(cx)?),
            Entry::Announcer => encode(p, &announcer::announcer(cx)?),
            Entry::AnnouncerPacks => encode(p, &announcer::announcer_packs(cx)?),
            Entry::Arcade => encode(p, &arcade::arcade(cx)?),
            Entry::Mutators => encode(p, &mutators::mutators(cx)?),
            Entry::BotTrees => encode(p, &bot_trees::bot_trees(cx)?),
            Entry::Input => encode(p, &input::input_defaults(cx)?),
            Entry::Rumble => encode(p, &rumble::rumble(cx)?),
            Entry::Tutorials => encode(p, &tutorial::tutorials(cx)?),
            Entry::BuffSupers => encode(p, &buff_supers::buff_supers(cx)?),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listed_paths() {
        let ps = paths();
        assert_eq!(ps.len(), 58);
        assert_eq!(ps[0], "/GameData/Index.Index");
        assert_eq!(ps[1], "/GameData/Char/shaggy.shaggy");
        assert!(ps.contains(&"/GameData/Char/velmapolice.velmapolice".to_owned()));
        assert_eq!(ps[25], "/GameData/Stage/trainingroomsmall.trainingroomsmall");
        assert_eq!(ps[54], "/GameData/Rumble.Rumble");
        assert_eq!(ps[55], "/GameData/AnnouncerPacks.AnnouncerPacks");
        assert_eq!(ps[56], "/GameData/Tutorials.Tutorials");
        assert_eq!(ps[57], "/GameData/BuffSupers.BuffSupers");
        for p in &ps {
            assert!(entry_of(p).is_some(), "{p}");
        }
        assert_eq!(entry_of("/GameData/Char/nobody.nobody"), None);
        assert_eq!(entry_of("/GameData/Stage/m011.m011"), Some(Entry::Stage("m011")));
        assert_eq!(data_only(), ["velmapolice"]);
    }

    /// The developer store's gamedata pack holds the same paths and the same Index (less the pack list, the tutorials
    /// and the buff parents, which the oracle's store has none of).
    #[test]
    fn the_oracle_entries() {
        let dir = mvs_store::dev_store_dir();
        let Some(pack) = mvs_store::Store::open(&dir).ok().and_then(|s| s.kind(mvs_data::KIND).ok()) else {
            eprintln!("{}: no gamedata pack: NOTHING CHECKED", dir.display());
            return;
        };
        // (the native importer's entries the oracle's store has none of, left out on both sides: the store may be
        // either importer's)
        let native_only = |p: &str| [path::ANNOUNCER_PACKS, path::TUTORIALS, path::BUFF_SUPERS].contains(&p);
        let mut ours = paths();
        ours.retain(|p| !native_only(p));
        let mut theirs: Vec<String> = pack.paths().filter(|p| !native_only(p)).map(str::to_owned).collect();
        ours.sort_unstable();
        theirs.sort_unstable();
        assert_eq!(ours, theirs);
        let bytes = pack.bytes(path::INDEX).expect("the Index entry");
        assert!(mvs_data::value::encode(&index()).unwrap() == *bytes, "the Index differs from the oracle's");
    }
}
