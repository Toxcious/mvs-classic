//! The stats file (`stats.mvs` in the data folder): the player's
//! records of online matches, their lifetime totals and their stat-tracker (badge) counts, which the build kept on its
//! Hydra server, the local arcade record, the local tutorial record (the build's TutorialData save slot), the
//! toasts the player has to give at the podium (the build's InventoryConsumables match_toasts; the port's rule) and
//! the port's own Waves records.
//!
//! The format: "MVSS", a format byte, the first 16 bytes of a SHA-256 of a fixed salt, the header and the body, then
//! the body: the data as JSON, zlib-compressed, XOR-ed with a fixed pattern. Not encryption (the source is public): a
//! text editor shows noise and an edit by hand breaks the check. A file whose check fails is set aside as
//! `stats.<time>.bad` and the last good copy (`stats.mvs.bak`) is used; with none the stats start empty. A file of a
//! newer format is left as it is and not written over. Each save goes to `stats.mvs.part`, flushed, then renamed
//! over the file.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use mvs_protocol::names::NAME_LIMIT;
use mvs_protocol::profile::{
    HistoryMode, MatchRecord, MatchRecordPlayer, PROFILE_MATCHES, ProfileTotals, TRACKER_TOTALS, TrackerCounts,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// The stats file's name in the data folder.
pub const FILE: &str = "stats.mvs";
/// The format byte this release writes and reads.
pub const FORMAT_VERSION: u8 = 1;
/// The records kept: the newest this many (the old online server kept each account's newest 100).
pub const RECORDS_KEPT: usize = 100;
/// (the port's own) The largest stats file read, and the most its body inflates to: a full file is some 100 KB.
pub const FILE_MAX: usize = 4 * 1024 * 1024;
/// (the port's own) Every sum the file keeps stops here: no player's matches reach it, and it stays far below the
/// numbers JSON keeps whole.
pub const SUM_MAX: f64 = 1e12;
/// The toasts a new stats file starts with.
pub const TOASTS_START: u64 = 5;

/// One record as the file keeps it: `slot` the own player's slot in it when it was added (-1: none), so a new id of
/// the player still finds them in their old records.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KeptRecord {
    pub slot: i32,
    pub record: MatchRecord,
}

/// The local arcade record (the game's arcade ArcadeRecord): per CharacterData slug the best difficulty of a run won
/// (1 Easy, 2 Normal, 3 Hard), and the Hard runs won.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsArcade {
    /// By slug, in the order JSON.parse gives the file's.
    pub stars: IndexMap<String, u8>,
    pub hard_completions: u32,
}

/// The local tutorial record (the build's TutorialData save slot: TutorialQuests' fields, by their names in the
/// file). The defaults are TutorialQuests' (a save slot that does not exist).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct StatsTutorial {
    pub basic_tutorial_complete: bool,
    pub basic_tutorial_claimed: bool,
    pub advanced_tutorial_complete: bool,
    pub advanced_tutorial_claimed: bool,
    pub coop_tutorial_complete: bool,
    pub coop_tutorial_claimed: bool,
    pub training_mode_complete: bool,
    pub training_mode_claimed: bool,
    #[serde(rename = "XP")]
    pub xp: f64,
    pub reward_claimed: bool,
    /// In the order JSON.parse gives the file's.
    pub tutorials_complete: IndexMap<String, bool>,
}

/// (the port's own) The Waves mode's local records: per character key, each variant's best
/// and its runs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatsWaves {
    /// By the character's key (the roster's port key), in the file's order.
    pub fighters: IndexMap<String, WavesRecord>,
}

/// One fighter's Waves records.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WavesRecord {
    /// Endless: the most ring-outs of a run (0: none yet).
    pub endless_best: u32,
    /// Endless: the runs played.
    pub endless_runs: u32,
    /// 100 Ring-Outs: the fastest cleared run, in sim ticks (None: none cleared).
    pub ring_outs_100_ticks: Option<u32>,
    /// 100 Ring-Outs: the runs played and the runs cleared.
    pub ring_outs_100_runs: u32,
    pub ring_outs_100_cleared: u32,
}

/// The file's data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatsData {
    pub totals: ProfileTotals,
    pub trackers: TrackerCounts,
    /// Newest first, at most RECORDS_KEPT.
    pub records: Vec<KeptRecord>,
    /// The local arcade record (a file written before it has none: empty).
    pub arcade: StatsArcade,
    /// The local tutorial record (a file written before it has none: the struct defaults).
    pub tutorial: StatsTutorial,
    /// The toasts to give (a file written before them has none: TOASTS_START).
    pub toasts: u64,
    /// The local Waves records (a file written before them has none: empty).
    #[serde(default)]
    pub waves: StatsWaves,
}

impl Default for StatsData {
    /// An empty stats file's data.
    fn default() -> StatsData {
        StatsData {
            totals: ProfileTotals::default(),
            trackers: TrackerCounts::default(),
            records: Vec::new(),
            arcade: StatsArcade::default(),
            tutorial: StatsTutorial::default(),
            toasts: TOASTS_START,
            waves: StatsWaves::default(),
        }
    }
}

/// Why the file was set aside, said once at the first read after it: `Bad` the last good copy is used, `Empty` there
/// was none and the stats start again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatsReset {
    Bad,
    Empty,
}

/// The player's own profile as the stats file holds it: the caller adds its id, name and icon. `matches` newest
/// first, at most PROFILE_MATCHES, each record's own player's `id` the `me` asked with.
#[derive(Clone, Debug, PartialEq)]
pub struct StatsProfile {
    pub totals: ProfileTotals,
    pub trackers: TrackerCounts,
    pub matches: Vec<MatchRecord>,
    pub reset: Option<StatsReset>,
    /// The toasts to give at the podium (the port's rule: TOASTS_START at first, +1 per online match won with
    /// another human in it, no maximum).
    pub toasts: u64,
}

/// A stats file's bytes read.
#[derive(Clone, Debug, PartialEq)]
pub enum StatsRead {
    /// The data of a file of this format.
    Data(Box<StatsData>),
    /// A file of a later format version (not read).
    Newer,
    /// Not a stats file: another header, a failed check, a body that does not read or inflates past FILE_MAX.
    Bad,
}

/// "MVSS": the file's first four bytes.
const MAGIC: &[u8; 4] = b"MVSS";
const HEADER: usize = MAGIC.len() + 1;
const CHECK: usize = 16;
const PATTERN: [u8; 32] = [
    0x7c, 0x11, 0x4b, 0xf2, 0xb4, 0x1c, 0xe0, 0x86, 0xd5, 0xe6, 0x4c, 0x6f, 0xa8, 0xf1, 0xc0, 0xdb, 0x9e, 0x5f, 0xd3,
    0xe4, 0x37, 0xab, 0x52, 0xd7, 0x6c, 0x85, 0x61, 0x82, 0x88, 0x79, 0x81, 0x5b,
];
const SALT: [u8; 16] = [0x18, 0xc8, 0x84, 0x86, 0x6a, 0x83, 0x89, 0xd9, 0x27, 0xc6, 0xdf, 0x33, 0x18, 0x9f, 0x1e, 0xb0];

/// The keys the file's per-character counts may name: the roster's.
const ROSTER: &[&str] = &mvs_protocol::roster::CHARACTERS;

// (the port's own caps on a slot's reported numbers: far past any match the rules let run)
const DAMAGE_MAX: f64 = 1_000_000.0;
const COUNT_MAX: f64 = 999.0;
// (the port's own caps, from protocol/: a match's humans and bots, the big matches' eight included, a player's name;
// an id is an EOS product user id of 32 characters, a record's the core's)
const PLAYERS_MAX: usize = mvs_protocol::lobby_rules::MAX_LOBBY_PLAYERS;
const ID_MAX: usize = 64;
const MAP_MAX: usize = 128;
// (the port's own caps) the slugs an arcade record keeps: the build's CharacterData slugs are some 20 characters
const SLUG_MAX: usize = 64;
const STARS_MAX: usize = 256;
// (the port's own caps) the keys a tutorial record's TutorialsComplete keeps: the advanced tutorials' twelve
// `<Type>TutorialComplete` keys are some 30 characters
const TUTORIAL_KEY_MAX: usize = 64;
const TUTORIAL_KEYS_MAX: usize = 64;
// (the port's own caps) the fighters a Waves record keeps, and its counts: a run's ticks fit a u32
const WAVES_FIGHTERS_MAX: usize = 256;
const WAVES_COUNT_MAX: f64 = u32::MAX as f64;
/// JavaScript's Number.MAX_SAFE_INTEGER.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

fn xor(b: &[u8]) -> Vec<u8> {
    b.iter().enumerate().map(|(i, x)| x ^ PATTERN[i % PATTERN.len()]).collect()
}

fn check_of(header: &[u8], body: &[u8]) -> [u8; CHECK] {
    let mut h = Sha256::new();
    h.update(SALT);
    h.update(header);
    h.update(body);
    let mut out = [0u8; CHECK];
    out.copy_from_slice(&h.finalize()[..CHECK]);
    out
}

/// The data as a stats file's bytes.
pub fn encode_stats(d: &StatsData) -> Vec<u8> {
    encode_json(&serde_json::to_vec(d).expect("StatsData serialises"))
}

fn encode_json(json: &[u8]) -> Vec<u8> {
    let mut header = MAGIC.to_vec();
    header.push(FORMAT_VERSION);
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(json).expect("writing to a Vec");
    let body = xor(&z.finish().expect("finishing into a Vec"));
    let mut out = header.clone();
    out.extend_from_slice(&check_of(&header, &body));
    out.extend_from_slice(&body);
    out
}

/// A stats file's bytes read.
pub fn read_stats(b: &[u8]) -> StatsRead {
    if b.len() < HEADER + CHECK || b.len() > FILE_MAX || &b[..MAGIC.len()] != MAGIC {
        return StatsRead::Bad;
    }
    if b[MAGIC.len()] > FORMAT_VERSION {
        return StatsRead::Newer;
    }
    if b[MAGIC.len()] != FORMAT_VERSION {
        return StatsRead::Bad;
    }
    let (header, body) = (&b[..HEADER], &b[HEADER + CHECK..]);
    if check_of(header, body)[..] != b[HEADER..HEADER + CHECK] {
        return StatsRead::Bad;
    }
    // (inflated to at most FILE_MAX: one byte more is a body that is too big)
    let mut inflated = Vec::new();
    let unxored = xor(body);
    let mut z = ZlibDecoder::new(&unxored[..]).take(FILE_MAX as u64 + 1);
    if z.read_to_end(&mut inflated).is_err() || inflated.len() > FILE_MAX {
        return StatsRead::Bad;
    }
    // (the key order of a malformed object is the Value's: its reader drops it anyway)
    let order = serde_json::from_slice::<KeyOrder>(&inflated).unwrap_or_default();
    match serde_json::from_slice::<Value>(&inflated).ok().and_then(|v| read_data_in(&v, &order)) {
        Some(d) => StatsRead::Data(Box::new(d)),
        None => StatsRead::Bad,
    }
}

// ---- what is read: the file's data (capped, never refused for a number: a sum at its cap reads back)

fn obj(v: &Value) -> Option<&Map<String, Value>> {
    v.as_object()
}

fn whole(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite() && n.fract() == 0.0)
}

/// A whole number from 0, capped at `max` (None: not one).
fn capped(v: &Value, max: f64) -> Option<f64> {
    whole(v).filter(|n| *n >= 0.0).map(|n| n.min(max))
}

/// A whole number within +-`max`.
fn int(v: &Value, max: f64) -> Option<f64> {
    whole(v).filter(|n| n.abs() <= max)
}

/// A string of at most `max` UTF-16 units.
fn text(v: &Value, max: usize) -> Option<&str> {
    v.as_str().filter(|s| s.encode_utf16().count() <= max)
}

fn read_player(v: &Value) -> Option<MatchRecordPlayer> {
    let o = obj(v)?;
    let id = text(o.get("id")?, ID_MAX)?;
    let name = text(o.get("name")?, NAME_LIMIT)?;
    let team = int(o.get("team")?, PLAYERS_MAX as f64)?;
    let slot = int(o.get("slot")?, (PLAYERS_MAX - 1) as f64)?;
    let character = o.get("character")?.as_str().filter(|c| ROSTER.contains(c))?;
    let damage = capped(o.get("damage")?, DAMAGE_MAX)?;
    let ringouts = capped(o.get("ringouts")?, COUNT_MAX)?;
    let deaths = capped(o.get("deaths")?, COUNT_MAX)?;
    let score = capped(o.get("score")?, COUNT_MAX)?;
    if slot < 0.0 {
        return None;
    }
    Some(MatchRecordPlayer {
        id: id.to_string(),
        name: name.to_string(),
        character: character.to_string(),
        team: team as i32,
        slot: slot as i32,
        bot: o.get("bot")?.as_bool()?,
        damage: damage as u32,
        ringouts: ringouts as u32,
        deaths: deaths as u32,
        score: score as u32,
        winner: o.get("winner")?.as_bool()?,
    })
}

/// A match record as the file has it; None when it is not one.
pub fn read_record(v: &Value) -> Option<MatchRecord> {
    let o = obj(v)?;
    let list = o.get("players")?.as_array().filter(|p| p.len() <= PLAYERS_MAX)?;
    let id = text(o.get("id")?, ID_MAX).filter(|s| !s.is_empty())?;
    let ended_at = capped(o.get("endedAt")?, MAX_SAFE_INTEGER)?;
    let map = text(o.get("map")?, MAP_MAX)?;
    let winning_team = int(o.get("winningTeam")?, PLAYERS_MAX as f64)?;
    let mode = whole(o.get("mode")?).filter(|m| (0.0..=2.0).contains(m))?;
    let (custom, checked) = (o.get("custom")?.as_bool()?, o.get("checked")?.as_bool()?);
    let mut players: Vec<MatchRecordPlayer> = Vec::new();
    for p in list {
        let r = read_player(p)?;
        if players.iter().any(|q| q.slot == r.slot) {
            return None;
        }
        players.push(r);
    }
    Some(MatchRecord {
        id: id.to_string(),
        ended_at: ended_at as u64,
        mode: HistoryMode(mode as u8),
        map: map.to_string(),
        custom,
        winning_team: winning_team as i32,
        checked,
        players,
    })
}

/// TrackerCounts as the file has them, every number capped at SUM_MAX; a name or a character it does not know is
/// dropped (a roster or a tracker list of another release), not refused.
fn read_counts(v: &Value) -> Option<TrackerCounts> {
    let o = obj(v)?;
    let (totals_in, characters_in) = (obj(o.get("totals")?)?, obj(o.get("characters")?)?);
    let mut totals = BTreeMap::new();
    for (k, n) in totals_in {
        if let (true, Some(c)) = (TRACKER_TOTALS.contains(&k.as_str()), capped(n, SUM_MAX)) {
            totals.insert(k.clone(), c);
        }
    }
    let mut characters = BTreeMap::new();
    for (k, c) in characters_in {
        let nums: Option<Vec<f64>> = match c.as_array() {
            Some(a) if ROSTER.contains(&k.as_str()) && a.len() == 4 => a.iter().map(|n| capped(n, SUM_MAX)).collect(),
            _ => None,
        };
        if let Some(n) = nums {
            characters.insert(k.clone(), [n[0], n[1], n[2], n[3]]);
        }
    }
    Some(TrackerCounts { totals, characters })
}

fn read_totals(v: &Value) -> Option<ProfileTotals> {
    let o = obj(v)?;
    let results_in = o.get("results")?.as_array().filter(|r| r.len() <= 3)?;
    let characters_in = obj(o.get("characters")?)?;
    let matches = capped(o.get("matches")?, SUM_MAX)?;
    let ringouts = capped(o.get("ringouts")?, SUM_MAX)?;
    let damage = capped(o.get("damage")?, SUM_MAX)?;
    let sets = capped(o.get("sets")?, SUM_MAX)?;
    let mut results = Vec::new();
    for r in results_in {
        let a = r.as_array().filter(|a| a.len() == 3)?;
        let n: Option<Vec<f64>> = a.iter().map(|n| capped(n, SUM_MAX)).collect();
        let n = n?;
        results.push([n[0] as u64, n[1] as u64, n[2] as u64]);
    }
    let mut characters = BTreeMap::new();
    for (k, c) in characters_in {
        let nums: Option<Vec<f64>> = match c.as_array() {
            Some(a) if ROSTER.contains(&k.as_str()) && a.len() == 2 => a.iter().map(|n| capped(n, SUM_MAX)).collect(),
            _ => None,
        };
        if let Some(n) = nums {
            characters.insert(k.clone(), [n[0] as u64, n[1] as u64]);
        }
    }
    Some(ProfileTotals {
        matches: matches as u64,
        ringouts: ringouts as u64,
        damage: damage as u64,
        sets: sets as u64,
        results,
        characters,
    })
}

/// An arcade record as the file has it: None when it is not one; a slug or a difficulty
/// that does not read is dropped, the count capped at SUM_MAX.
pub fn read_arcade(v: &Value) -> Option<StatsArcade> {
    read_arcade_in(v, None)
}

/// `read_arcade` with the stars' keys in the order the file gave them (None: the Value's).
fn read_arcade_in(v: &Value, order: Option<&[String]>) -> Option<StatsArcade> {
    let o = obj(v)?;
    let stars_in = obj(o.get("stars")?)?;
    let hard = capped(o.get("hardCompletions")?, SUM_MAX)?;
    let mut stars = IndexMap::new();
    for (k, n) in in_order(stars_in, order).take(STARS_MAX) {
        let d = whole(n).filter(|d| (1.0..=3.0).contains(d));
        if let (true, Some(d)) = (k.encode_utf16().count() <= SLUG_MAX, d) {
            stars.insert(k.clone(), d as u8);
        }
    }
    Some(StatsArcade { stars, hard_completions: hard as u32 })
}

/// A tutorial record as the file has it: None when it is not one; a TutorialsComplete
/// entry that does not read is dropped, XP capped at SUM_MAX.
pub fn read_tutorial(v: &Value) -> Option<StatsTutorial> {
    read_tutorial_in(v, None)
}

/// `read_tutorial` with TutorialsComplete's keys in the order the file gave them (None: the Value's).
fn read_tutorial_in(v: &Value, order: Option<&[String]>) -> Option<StatsTutorial> {
    let o = obj(v)?;
    let done_in = obj(o.get("TutorialsComplete")?)?;
    let xp = capped(o.get("XP")?, SUM_MAX)?;
    let flag = |k: &str| o.get(k).and_then(Value::as_bool);
    let mut tutorials_complete = IndexMap::new();
    for (k, b) in in_order(done_in, order).take(TUTORIAL_KEYS_MAX) {
        if let (true, Some(b)) = (k.encode_utf16().count() <= TUTORIAL_KEY_MAX, b.as_bool()) {
            tutorials_complete.insert(k.clone(), b);
        }
    }
    Some(StatsTutorial {
        basic_tutorial_complete: flag("BasicTutorialComplete")?,
        basic_tutorial_claimed: flag("BasicTutorialClaimed")?,
        advanced_tutorial_complete: flag("AdvancedTutorialComplete")?,
        advanced_tutorial_claimed: flag("AdvancedTutorialClaimed")?,
        coop_tutorial_complete: flag("CoopTutorialComplete")?,
        coop_tutorial_claimed: flag("CoopTutorialClaimed")?,
        training_mode_complete: flag("TrainingModeComplete")?,
        training_mode_claimed: flag("TrainingModeClaimed")?,
        xp,
        reward_claimed: flag("RewardClaimed")?,
        tutorials_complete,
    })
}

/// A Waves record as the file has it: None when it is not one; a fighter whose key is too long or whose entry does
/// not read is dropped, each count capped at WAVES_COUNT_MAX.
pub fn read_waves(v: &Value) -> Option<StatsWaves> {
    read_waves_in(v, None)
}

/// `read_waves` with the fighters' keys in the order the file gave them (None: the Value's).
fn read_waves_in(v: &Value, order: Option<&[String]>) -> Option<StatsWaves> {
    let o = obj(v)?;
    let fighters_in = obj(o.get("fighters")?)?;
    let mut fighters = IndexMap::new();
    for (k, r) in in_order(fighters_in, order).take(WAVES_FIGHTERS_MAX) {
        if let (true, Some(r)) = (k.encode_utf16().count() <= SLUG_MAX, read_waves_record(r)) {
            fighters.insert(k.clone(), r);
        }
    }
    Some(StatsWaves { fighters })
}

fn read_waves_record(v: &Value) -> Option<WavesRecord> {
    let o = obj(v)?;
    let count = |k: &str| o.get(k).and_then(|n| capped(n, WAVES_COUNT_MAX)).map(|n| n as u32);
    let ticks = match o.get("ringOuts100Ticks") {
        None | Some(Value::Null) => None,
        Some(n) => Some(capped(n, WAVES_COUNT_MAX)? as u32),
    };
    Some(WavesRecord {
        endless_best: count("endlessBest")?,
        endless_runs: count("endlessRuns")?,
        ring_outs_100_ticks: ticks,
        ring_outs_100_runs: count("ringOuts100Runs")?,
        ring_outs_100_cleared: count("ringOuts100Cleared")?,
    })
}

/// The file's toasts: a whole number from 0 (no maximum but SUM_MAX's); none, or one that is not a whole number,
/// TOASTS_START.
fn read_toasts(v: Option<&Value>) -> u64 {
    match v.and_then(whole) {
        Some(n) => n.clamp(0.0, SUM_MAX) as u64,
        None => TOASTS_START,
    }
}

/// The keys of the objects whose first entries a reader keeps, in the file's order (a Value's map sorts them;
/// JavaScript's Object.entries keeps JSON.parse's order).
#[derive(Debug, Default, Deserialize)]
struct KeyOrder {
    #[serde(default)]
    arcade: Option<ArcadeOrder>,
    #[serde(default)]
    tutorial: Option<TutorialOrder>,
    #[serde(default)]
    waves: Option<WavesOrder>,
}

#[derive(Debug, Deserialize)]
struct WavesOrder {
    fighters: IndexMap<String, serde::de::IgnoredAny>,
}

#[derive(Debug, Deserialize)]
struct ArcadeOrder {
    stars: IndexMap<String, serde::de::IgnoredAny>,
}

#[derive(Debug, Deserialize)]
struct TutorialOrder {
    #[serde(rename = "TutorialsComplete")]
    done: IndexMap<String, serde::de::IgnoredAny>,
}

/// An object's keys in JavaScript's property order: the array-index keys ("0" .. "4294967294", canonical) first in
/// ascending order, then the others as given (OrdinaryOwnPropertyKeys).
fn js_order<'a>(keys: impl Iterator<Item = &'a String>) -> Vec<String> {
    let index = |k: &str| -> Option<u32> {
        let n: u32 = k.parse().ok()?;
        (n != u32::MAX && n.to_string() == k).then_some(n)
    };
    let mut ix: Vec<(u32, String)> = Vec::new();
    let mut rest = Vec::new();
    for k in keys {
        match index(k) {
            Some(n) => ix.push((n, k.clone())),
            None => rest.push(k.clone()),
        }
    }
    ix.sort_by_key(|e| e.0);
    ix.into_iter().map(|e| e.1).chain(rest).collect()
}

/// `o`'s entries in `order` (the keys `o` lacks skipped), else in the Value's order.
fn in_order<'a>(
    o: &'a Map<String, Value>,
    order: Option<&'a [String]>,
) -> Box<dyn Iterator<Item = (&'a String, &'a Value)> + 'a> {
    match order {
        Some(keys) => Box::new(keys.iter().filter_map(move |k| o.get_key_value(k))),
        None => Box::new(o.iter()),
    }
}

#[cfg(test)]
fn read_data(v: &Value) -> Option<StatsData> {
    read_data_in(v, &KeyOrder::default())
}

fn read_data_in(v: &Value, order: &KeyOrder) -> Option<StatsData> {
    let o = obj(v)?;
    let records_in = o.get("records")?.as_array()?;
    let totals = read_totals(o.get("totals")?)?;
    let trackers = read_counts(o.get("trackers")?)?;
    let mut records = Vec::new();
    for r in records_in {
        let record = obj(r).and_then(|r| r.get("record")).and_then(read_record);
        let slot = obj(r).and_then(|r| r.get("slot")).and_then(|s| int(s, PLAYERS_MAX as f64));
        // (a record this release cannot read is dropped, the rest kept)
        if let (Some(record), Some(slot)) = (record, slot) {
            records.push(KeptRecord { slot: slot as i32, record });
        }
    }
    records.truncate(RECORDS_KEPT);
    let stars_order = order.arcade.as_ref().map(|a| js_order(a.stars.keys()));
    let done_order = order.tutorial.as_ref().map(|t| js_order(t.done.keys()));
    let waves_order = order.waves.as_ref().map(|w| w.fighters.keys().cloned().collect::<Vec<_>>());
    Some(StatsData {
        totals,
        trackers,
        records,
        arcade: o.get("arcade").and_then(|a| read_arcade_in(a, stars_order.as_deref())).unwrap_or_default(),
        tutorial: o.get("tutorial").and_then(|t| read_tutorial_in(t, done_order.as_deref())).unwrap_or_default(),
        toasts: read_toasts(o.get("toasts")),
        waves: o.get("waves").and_then(|w| read_waves_in(w, waves_order.as_deref())).unwrap_or_default(),
    })
}

// ---- the file

/// `file` whole on the disk: written beside as <file>.part, flushed, then moved over it (a crash before the move
/// leaves the old file as it was; a rename Windows refuses for a moment, another program holding the file, is tried
/// again: mvs-store's `rename_retried`). `crash`: the tests' crash point between the write and the rename.
fn write_whole(file: &Path, bytes: &[u8], crash: Option<&AtomicBool>) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut part = file.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let mut f = File::create(&part)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    if crash.is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Err(io::Error::other("crash"));
    }
    mvs_store::manage::rename_retried(&part, file)
}

/// An ISO-8601 time of now with `:` and `.` as `-`, 2026-11-10T12-00-00-000Z.
fn stamp() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let (secs, ms) = (d.as_secs(), d.subsec_millis());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // civil date from the day count (Howard Hinnant's days_from_civil, inverted)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}-{:02}-{:02}-{ms:03}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

/// The stats file of one folder (the data folder), or one kept in memory only. Read at the first call that needs
/// it; each change saved at once.
#[derive(Debug)]
pub struct StatsFile {
    /// `<dir>/stats.mvs`; None: in memory, never read or written.
    file: Option<PathBuf>,
    data: Option<StatsData>,
    /// The reset not yet told (StatsProfile.reset).
    reset: Option<StatsReset>,
    /// The file is of a newer format: this run keeps no stats and never writes over it.
    read_only: bool,
    /// The data holds changes the last save could not write (the next change saves them).
    pub unsaved: bool,
    /// The tests' crash: while set, a save stops between the temporary file's write and its rename.
    crash: Option<Arc<AtomicBool>>,
}

impl Default for StatsFile {
    fn default() -> StatsFile {
        StatsFile::memory()
    }
}

impl StatsFile {
    /// The stats file of `dir`: nothing is read until a call needs the data.
    pub fn open(dir: &Path) -> StatsFile {
        StatsFile { file: Some(dir.join(FILE)), ..StatsFile::memory() }
    }

    /// Stats kept in memory only (the empty stats; a test's, or a run with no data folder).
    pub fn memory() -> StatsFile {
        StatsFile { file: None, data: None, reset: None, read_only: false, unsaved: false, crash: None }
    }

    /// The file's path (None in memory).
    pub fn path(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// Makes every save stop before its rename while `flag` is set (the tests' crash point).
    pub fn crash_before_rename(&mut self, flag: Arc<AtomicBool>) {
        self.crash = Some(flag);
    }

    fn bak(file: &Path) -> PathBuf {
        let mut p = file.as_os_str().to_owned();
        p.push(".bak");
        PathBuf::from(p)
    }

    fn aside(file: &Path, kind: &str) -> PathBuf {
        file.with_file_name(format!("stats.{}.{kind}", stamp()))
    }

    /// The data of a stats file's bytes of this format; None for anything else.
    fn decode(b: &[u8]) -> Option<StatsData> {
        match read_stats(b) {
            StatsRead::Data(d) => Some(*d),
            _ => None,
        }
    }

    fn load(&mut self) {
        if self.data.is_some() {
            return;
        }
        let Some(file) = self.file.clone() else {
            self.data = Some(StatsData::default());
            return;
        };
        let bak = Self::bak(&file);
        let read_bak = || fs::read(&bak).ok().and_then(|b| Self::decode(&b));
        match fs::read(&file) {
            Ok(bytes) => match read_stats(&bytes) {
                StatsRead::Newer => {
                    eprintln!(
                        "stats: {} is of a newer format; no stats are kept this run, and the file is left as it is",
                        file.display()
                    );
                    self.read_only = true;
                    self.data = Some(StatsData::default());
                }
                StatsRead::Data(d) => self.data = Some(*d),
                StatsRead::Bad => {
                    let aside = Self::aside(&file, "bad");
                    if let Err(e) = fs::rename(&file, &aside) {
                        // Deviation: an error leaves the file for the next call; this reads the stats as empty
                        // for the run and never writes over the file.
                        eprintln!("stats: {} failed its check and could not be set aside: {e}", file.display());
                        self.read_only = true;
                        self.data = Some(StatsData::default());
                        return;
                    }
                    eprintln!("stats: {} failed its check; set aside as {}", file.display(), aside.display());
                    let b = read_bak();
                    self.reset = Some(if b.is_some() { StatsReset::Bad } else { StatsReset::Empty });
                    let had = b.is_some();
                    self.data = Some(b.unwrap_or_default());
                    // (the good copy made the file again; a failed write leaves it to the next save)
                    if had {
                        self.try_save();
                    }
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // (no file: the first run, or one removed by hand; its last good copy when there is one)
                self.data = Some(read_bak().unwrap_or_default());
            }
            Err(e) => {
                // Deviation: an error tries again at the next call; this reads the stats as empty for the run
                // and never writes over the file.
                eprintln!("stats: {} could not be read: {e}", file.display());
                self.read_only = true;
                self.data = Some(StatsData::default());
            }
        }
    }

    /// Saves the data; a write that fails leaves it in memory, unsaved, for the next save.
    fn try_save(&mut self) {
        let (Some(file), Some(data)) = (self.file.as_ref(), self.data.as_ref()) else { return };
        if self.read_only {
            return;
        }
        let bytes = encode_stats(data);
        let r = (|| {
            if file.exists() {
                fs::copy(file, Self::bak(file))?;
            }
            write_whole(file, &bytes, self.crash.as_deref())
        })();
        match r {
            Ok(()) => self.unsaved = false,
            Err(e) => {
                self.unsaved = true;
                eprintln!("stats: {} could not be written (kept until the next save): {e}", file.display());
            }
        }
    }

    /// The file's data: the file, else (a failed check: the file set aside) its .bak, else empty.
    pub fn data(&mut self) -> &StatsData {
        self.load();
        self.data.as_ref().expect("load sets the data")
    }

    /// The own profile: the newest PROFILE_MATCHES records, each one's own player's `id` set to `me`; `reset` said
    /// once after the file was set aside.
    pub fn profile(&mut self, me: &str) -> StatsProfile {
        self.load();
        let reset = self.reset.take();
        let d = self.data.as_ref().expect("load sets the data");
        let matches = d
            .records
            .iter()
            .take(PROFILE_MATCHES)
            .map(|k| {
                let mut r = k.record.clone();
                for p in &mut r.players {
                    if p.slot == k.slot {
                        p.id = me.to_string();
                    }
                }
                r
            })
            .collect();
        StatsProfile { totals: d.totals.clone(), trackers: d.trackers.clone(), matches, reset, toasts: d.toasts }
    }

    /// The local arcade record.
    pub fn arcade(&mut self) -> StatsArcade {
        self.data().arcade.clone()
    }

    /// Replaces the arcade record (after a run won); false when it is not one (a difficulty past 3 is dropped; the
    /// count is capped at SUM_MAX). A save that fails keeps it in memory.
    pub fn set_arcade(&mut self, v: StatsArcade) -> bool {
        // (the record goes through the file's reader: its caps and drops)
        let order = js_order(v.stars.keys());
        let Some(a) = serde_json::to_value(&v).ok().as_ref().and_then(|a| read_arcade_in(a, Some(&order))) else {
            return false;
        };
        self.load();
        if let Some(d) = self.data.as_mut() {
            d.arcade = a;
        }
        self.try_save();
        true
    }

    /// The local tutorial record.
    pub fn tutorial(&mut self) -> StatsTutorial {
        self.data().tutorial.clone()
    }

    /// Replaces the tutorial record (at a tutorial's finish); false when it is not one (XP past SUM_MAX is capped, an
    /// entry of TutorialsComplete that is too long is dropped). A save that fails keeps it in memory.
    pub fn set_tutorial(&mut self, v: StatsTutorial) -> bool {
        let order = js_order(v.tutorials_complete.keys());
        let Some(t) = serde_json::to_value(&v).ok().as_ref().and_then(|t| read_tutorial_in(t, Some(&order))) else {
            return false;
        };
        self.load();
        if let Some(d) = self.data.as_mut() {
            d.tutorial = t;
        }
        self.try_save();
        true
    }

    /// The local Waves records.
    pub fn waves(&mut self) -> StatsWaves {
        self.data().waves.clone()
    }

    /// Replaces the Waves records (at a run's end); false when they are not one (a key past SLUG_MAX is dropped). A
    /// save that fails keeps them in memory.
    pub fn set_waves(&mut self, v: StatsWaves) -> bool {
        let order: Vec<String> = v.fighters.keys().cloned().collect();
        let Some(w) = serde_json::to_value(&v).ok().as_ref().and_then(|w| read_waves_in(w, Some(&order))) else {
            return false;
        };
        self.load();
        if let Some(d) = self.data.as_mut() {
            d.waves = w;
        }
        self.try_save();
        true
    }

    /// Spends one toast (the build's BPI_Toast, PandaGameInstance @102499: the stock less one, given whatever the
    /// send does): the count left, None when there is none. A save that fails keeps it in memory.
    pub fn spend_toast(&mut self) -> Option<u64> {
        self.load();
        let d = self.data.as_mut()?;
        if d.toasts < 1 {
            return None;
        }
        d.toasts -= 1;
        let left = d.toasts;
        self.try_save();
        Some(left)
    }

    /// A match.record the core sent this player (the message as it came), `me` their id in it: added once per
    /// record id (the record kept, newest first, at most RECORDS_KEPT); the totals take it when `totals` says so and
    /// the record has a player `me`, the trackers its gain; a toast more when the record's player `me` won and another
    /// player of it is a human (the port's rule, for the build's server-granted match_toasts: an online match won
    /// with another human in it, a custom one too, and not left before its end: `left`; no maximum). False: not a
    /// record, or one already in. A save that fails keeps it in memory.
    pub fn add(&mut self, msg: &Value, me: &str) -> bool {
        let Some(m) = obj(msg) else { return false };
        let null = Value::Null;
        let record = m.get("record").and_then(read_record);
        let game = capped(m.get("game").unwrap_or(&null), COUNT_MAX);
        let trackers = m.get("trackers").filter(|t| !t.is_null());
        let gain = trackers.and_then(read_gain);
        let (Some(record), Some(game)) = (record, game) else { return false };
        if trackers.is_some() && gain.is_none() {
            return false;
        }
        self.load();
        let Some(d) = self.data.as_mut() else { return false };
        if d.records.iter().any(|r| r.record.id == record.id) {
            return false;
        }
        let own = if me.is_empty() { None } else { record.players.iter().find(|p| !p.bot && p.id == me) };
        if let Some(p) = own
            && m.get("totals") == Some(&Value::Bool(true))
        {
            // (the count is whole and capped at COUNT_MAX)
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let set_game = game as u32;
            d.totals = mvs_protocol::record::add_match(&d.totals, &record, p, set_game);
        }
        if let Some(g) = &gain {
            d.trackers = mvs_protocol::record::add_trackers(&d.trackers, g);
        }
        let toast = own.is_some_and(|p| p.winner)
            && m.get("left") != Some(&Value::Bool(true))
            && record.players.iter().any(|p| !p.bot && !p.id.is_empty() && p.id != me);
        let slot = own.map_or(-1, |p| p.slot);
        d.records.insert(0, KeptRecord { slot, record });
        d.records.truncate(RECORDS_KEPT);
        if toast {
            d.toasts += 1;
        }
        cap_sums(d);
        self.try_save();
        true
    }
}

/// A match's gain caps (one match's numbers; TotalAssists the assisted ring-outs and the
/// assist leader's one), by TRACKER_TOTALS name.
fn gain_max(k: &str) -> f64 {
    use mvs_protocol::record::{COUNT_MAX as COUNTS, DAMAGE_MAX as DAMAGE, DODGES_MAX as DODGES};
    match k {
        "TotalWins" | "TotalRingoutLeader" => 1.0,
        "TotalAssists" => f64::from(COUNTS) + 1.0,
        "TotalAttacksDodged" => f64::from(DODGES),
        "HighestDamageDealt" => f64::from(DAMAGE),
        _ => f64::from(COUNTS),
    }
}

/// A match's tracker gain as a core sends it: None when it is not one (a name the trackers do not have, a character
/// not of the roster, more than a match's characters); its numbers capped at one match's
/// (read strictly).
fn read_gain(v: &Value) -> Option<TrackerCounts> {
    use mvs_protocol::record::{COUNT_MAX as COUNTS, DAMAGE_MAX as DAMAGE};
    let char_max = [1.0, f64::from(DAMAGE), f64::from(DAMAGE), f64::from(COUNTS)];
    let o = obj(v)?;
    let (totals_in, characters_in) = (obj(o.get("totals")?)?, obj(o.get("characters")?)?);
    let mut totals = BTreeMap::new();
    for (k, n) in totals_in {
        if !TRACKER_TOTALS.contains(&k.as_str()) {
            return None;
        }
        totals.insert(k.clone(), capped(n, gain_max(k))?);
    }
    if characters_in.len() > PLAYERS_MAX {
        return None;
    }
    let mut characters = BTreeMap::new();
    for (k, c) in characters_in {
        let a = c.as_array().filter(|a| ROSTER.contains(&k.as_str()) && a.len() == 4)?;
        let mut nums = [0.0; 4];
        for (i, n) in a.iter().enumerate() {
            nums[i] = capped(n, char_max[i])?;
        }
        characters.insert(k.clone(), nums);
    }
    Some(TrackerCounts { totals, characters })
}

/// The sums at SUM_MAX (a sum the next match would carry past it stops there).
fn cap_sums(d: &mut StatsData) {
    // (SUM_MAX is a whole number far below u64::MAX)
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let max = SUM_MAX as u64;
    let t = &mut d.totals;
    for n in [&mut t.matches, &mut t.ringouts, &mut t.damage, &mut t.sets] {
        *n = (*n).min(max);
    }
    for r in &mut t.results {
        for n in r.iter_mut() {
            *n = (*n).min(max);
        }
    }
    for c in t.characters.values_mut() {
        for n in c.iter_mut() {
            *n = (*n).min(max);
        }
    }
    for n in d.trackers.totals.values_mut() {
        *n = n.min(SUM_MAX);
    }
    for c in d.trackers.characters.values_mut() {
        for n in c.iter_mut() {
            *n = n.min(SUM_MAX);
        }
    }
    d.toasts = d.toasts.min(max);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// A folder of its own, removed at the end of the test.
    struct Dir(PathBuf);
    impl Dir {
        fn new() -> Dir {
            static N: AtomicUsize = AtomicUsize::new(0);
            let p = std::env::temp_dir().join(format!(
                "mvs-stats-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
        fn file(&self) -> PathBuf {
            self.0.join(FILE)
        }
        fn names(&self) -> Vec<String> {
            fs::read_dir(&self.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn record(id: &str, me: &str) -> MatchRecord {
        let player = |id: &str, name: &str, ch: &str, team, slot, winner| MatchRecordPlayer {
            id: id.into(),
            name: name.into(),
            character: ch.into(),
            team,
            slot,
            bot: false,
            damage: 120,
            ringouts: 3,
            deaths: 1,
            score: 3,
            winner,
        };
        MatchRecord {
            id: id.into(),
            ended_at: 1_700_000_000_000,
            mode: HistoryMode::SOLO,
            map: "map_m008".into(),
            custom: false,
            winning_team: 0,
            checked: true,
            players: vec![player(me, "Me", "shaggy", 0, 0, true), player("other", "Them", "batman", 1, 1, false)],
        }
    }

    /// A match.record message of `r`: game 1, totals counted, the tracker gain of a win.
    fn msg(r: &MatchRecord) -> Value {
        serde_json::json!({
            "record": r,
            "game": 1,
            "totals": true,
            "trackers": { "totals": { "TotalWins": 1, "TotalRingouts": 3, "HighestDamageDealt": 120 },
                "characters": { "shaggy": [1, 120, 120, 3] } },
        })
    }

    fn push(f: &mut StatsFile, r: MatchRecord, me: &str) {
        assert!(f.add(&msg(&r), me));
    }

    #[test]
    fn add_counts_a_record_once() {
        let dir = Dir::new();
        let mut a = StatsFile::open(&dir.0);
        assert!(a.add(&msg(&record("m1", "me")), "me"));
        let mut two = msg(&record("m2", "me"));
        two["game"] = serde_json::json!(2);
        assert!(a.add(&two, "me"));
        let b = StatsFile::open(&dir.0).profile("me");
        assert!(b.totals.matches == 2 && b.totals.sets == 1 && b.totals.ringouts == 6 && b.totals.damage == 240);
        assert!(b.totals.results[0][0] == 2 && b.totals.characters["shaggy"] == [2, 2]);
        assert!(b.trackers.totals["TotalWins"] == 2.0 && b.trackers.totals["HighestDamageDealt"] == 120.0);
        assert_eq!(b.trackers.characters["shaggy"][2], 240.0);
        // the same record twice
        assert!(!a.add(&msg(&record("m1", "me")), "me"));
        assert_eq!(a.profile("me").totals.matches, 2);
    }

    #[test]
    fn add_without_totals_trackers_or_own_player() {
        let mut a = StatsFile::memory();
        let mut m = msg(&record("m1", "me"));
        m["totals"] = Value::Bool(false);
        m["trackers"] = Value::Null;
        assert!(a.add(&m, "me"));
        let p = a.profile("me");
        assert!(p.matches.len() == 1 && p.totals.matches == 0 && p.trackers.totals.is_empty());
        assert!(a.add(&msg(&record("m2", "someone-else")), "me"));
        let p = a.profile("me");
        assert!(p.matches.len() == 2 && p.totals.matches == 0);
        let mut bad = msg(&record("m3", "me"));
        bad["record"] = serde_json::json!({ "id": "m3" });
        assert!(!a.add(&bad, "me") && a.profile("me").matches.len() == 2);
        // a gain of a character not on the roster, a record of more than 8 players, a name past NAME_LIMIT
        let mut odd = msg(&record("c3", "me"));
        odd["trackers"] = serde_json::json!({ "totals": {}, "characters": { "notacharacter": [0, 0, 0, 0] } });
        assert!(!a.add(&odd, "me"));
        let mut many = record("c4", "me");
        for slot in 2..9 {
            let mut p = many.players[1].clone();
            p.slot = slot;
            many.players.push(p);
        }
        assert!(!a.add(&msg(&many), "me"));
        let mut named = record("c5", "me");
        named.players[1].name = "x".repeat(NAME_LIMIT + 1);
        assert!(!a.add(&msg(&named), "me"));
    }

    #[test]
    fn add_toasts_only_a_win_with_another_human() {
        let mut a = StatsFile::memory();
        push(&mut a, record("t1", "me"), "me");
        let mut lost = record("t2", "me");
        lost.players[0].winner = false;
        lost.players[1].winner = true;
        push(&mut a, lost, "me");
        let mut bots = record("t3", "me");
        bots.players[1].id = String::new();
        bots.players[1].bot = true;
        push(&mut a, bots, "me");
        push(&mut a, record("t4", "someone-else"), "me");
        let mut left = msg(&record("t6", "me"));
        left["left"] = Value::Bool(true);
        assert!(a.add(&left, "me"));
        assert_eq!(a.profile("me").toasts, TOASTS_START + 1);
    }

    #[test]
    fn add_caps_the_damage_and_the_sums() {
        let dir = Dir::new();
        let mut a = StatsFile::open(&dir.0);
        let mut big = msg(&record("c1", "me"));
        big["record"]["players"][0]["damage"] = serde_json::json!(9_007_199_254_740_991_u64);
        big["trackers"] = Value::Null;
        assert!(a.add(&big, "me"));
        assert_eq!(a.profile("me").totals.damage, DAMAGE_MAX as u64);
        // a file whose sums are at the cap: the next match keeps them there
        let file = dir.0.join(FILE);
        let StatsRead::Data(mut d) = read_stats(&fs::read(&file).unwrap()) else { panic!("the file reads") };
        d.totals.damage = SUM_MAX as u64;
        d.trackers.totals.insert("TotalRingouts".into(), SUM_MAX);
        fs::write(&file, encode_stats(&d)).unwrap();
        let mut b = StatsFile::open(&dir.0);
        assert!(b.add(&msg(&record("c2", "me")), "me"));
        let p = StatsFile::open(&dir.0).profile("me");
        assert!(p.reset.is_none() && p.totals.damage == SUM_MAX as u64);
        assert_eq!(p.trackers.totals["TotalRingouts"], SUM_MAX);
    }

    #[test]
    fn json_names_are_the_ts_ones() {
        let v = serde_json::to_value(StatsData::default()).unwrap();
        assert_eq!(v["toasts"], 5);
        assert_eq!(v["arcade"]["hardCompletions"], 0);
        assert_eq!(v["tutorial"]["XP"], 0.0);
        assert_eq!(v["tutorial"]["BasicTutorialComplete"], false);
        assert_eq!(v["tutorial"]["TutorialsComplete"], serde_json::json!({}));
        assert_eq!(serde_json::to_value(StatsReset::Empty).unwrap(), "empty");
    }

    #[test]
    fn open_reads_nothing() {
        let f = StatsFile::open(Path::new("Z:/no/such/folder"));
        assert_eq!(f.path(), Some(Path::new("Z:/no/such/folder/stats.mvs")));
        assert!(StatsFile::default().path().is_none());
    }

    #[test]
    fn round_trip_and_file_shape() {
        let dir = Dir::new();
        let mut a = StatsFile::open(&dir.0);
        let p = a.profile("me");
        assert!(p.reset.is_none() && p.matches.is_empty());
        push(&mut a, record("m1", "me"), "me");
        push(&mut a, record("m2", "me"), "me");
        let b = StatsFile::open(&dir.0).profile("me");
        assert_eq!(b.totals.matches, 2);
        assert!(b.matches.len() == 2 && b.matches[0].id == "m2" && b.matches[1].id == "m1");
        assert!(b.reset.is_none());
        let bytes = fs::read(dir.file()).unwrap();
        assert_eq!(&bytes[..4], b"MVSS");
        assert!(!bytes.windows(6).any(|w| w == b"shaggy"));
        let bak = fs::read(dir.0.join("stats.mvs.bak")).unwrap();
        assert_eq!(StatsFile::decode(&bak).unwrap().records.len(), 1);
    }

    #[test]
    fn arcade_record() {
        let dir = Dir::new();
        let mut a = StatsFile::open(&dir.0);
        assert!(a.arcade().hard_completions == 0 && a.arcade().stars.is_empty());
        let rec = StatsArcade {
            stars: IndexMap::from([("character_shaggy".to_string(), 3), ("character_finn".to_string(), 1)]),
            hard_completions: 2,
        };
        assert!(a.set_arcade(rec));
        push(&mut a, record("m1", "me"), "me");
        let b = StatsFile::open(&dir.0).arcade();
        assert!(b.hard_completions == 2 && b.stars["character_shaggy"] == 3 && b.stars["character_finn"] == 1);
        let high = StatsArcade { stars: IndexMap::from([("character_shaggy".to_string(), 4)]), hard_completions: 2 };
        assert!(a.set_arcade(high));
        assert!(!StatsFile::open(&dir.0).arcade().stars.contains_key("character_shaggy"));
        // a file written before the record reads with an empty one
        let mut v = serde_json::to_value(StatsData::default()).unwrap();
        v.as_object_mut().unwrap().remove("arcade");
        assert_eq!(read_data(&v).unwrap().arcade.hard_completions, 0);
    }

    #[test]
    fn tutorial_record() {
        let dir = Dir::new();
        let mut a = StatsFile::open(&dir.0);
        assert!(!a.tutorial().basic_tutorial_complete && a.tutorial().tutorials_complete.is_empty());
        let t = StatsTutorial {
            basic_tutorial_complete: true,
            xp: 50.0,
            tutorials_complete: IndexMap::from([("FundamentalsTutorialComplete".to_string(), true)]),
            ..StatsTutorial::default()
        };
        assert!(a.set_tutorial(t));
        assert!(!a.set_tutorial(StatsTutorial { xp: -1.0, ..StatsTutorial::default() }));
        push(&mut a, record("m1", "me"), "me");
        let b = StatsFile::open(&dir.0).tutorial();
        assert!(b.basic_tutorial_complete && b.xp == 50.0 && b.tutorials_complete["FundamentalsTutorialComplete"]);
        let mut v = serde_json::to_value(StatsData::default()).unwrap();
        v.as_object_mut().unwrap().remove("tutorial");
        assert!(!read_data(&v).unwrap().tutorial.basic_tutorial_complete);
        // a bool that is not one refuses the record
        let mut v = serde_json::to_value(StatsTutorial::default()).unwrap();
        v["RewardClaimed"] = serde_json::json!(1);
        assert!(read_tutorial(&v).is_none());
    }

    #[test]
    fn waves_record() {
        let dir = Dir::new();
        let mut a = StatsFile::open(&dir.0);
        assert!(a.waves().fighters.is_empty());
        let rec = WavesRecord {
            endless_best: 57,
            endless_runs: 3,
            ring_outs_100_ticks: Some(36_000),
            ring_outs_100_runs: 2,
            ring_outs_100_cleared: 1,
        };
        let only_endless = WavesRecord { endless_best: 4, endless_runs: 1, ..WavesRecord::default() };
        let w =
            StatsWaves { fighters: IndexMap::from([("shaggy".to_string(), rec), ("finn".to_string(), only_endless)]) };
        assert!(a.set_waves(w.clone()));
        push(&mut a, record("m1", "me"), "me");
        let b = StatsFile::open(&dir.0).waves();
        assert_eq!(b, w);
        assert_eq!(b.fighters.keys().collect::<Vec<_>>(), ["shaggy", "finn"]);
        assert_eq!(StatsFile::open(&dir.0).data().records.len(), 1);
        // the names in the file
        let v = serde_json::to_value(&w).unwrap();
        assert_eq!(v["fighters"]["shaggy"]["ringOuts100Ticks"], 36_000);
        assert_eq!(v["fighters"]["finn"]["ringOuts100Ticks"], Value::Null);
        assert_eq!(v["fighters"]["finn"]["endlessBest"], 4);
        // a file written before the record (an older release's) reads with an empty one, the rest as it was
        let mut v = serde_json::to_value(StatsData { toasts: 9, ..StatsData::default() }).unwrap();
        v.as_object_mut().unwrap().remove("waves");
        let d = read_data(&v).unwrap();
        assert!(d.waves.fighters.is_empty() && d.toasts == 9);
        let bytes = encode_json(&serde_json::to_vec(&v).unwrap());
        assert!(matches!(read_stats(&bytes), StatsRead::Data(d) if d.waves.fighters.is_empty() && d.toasts == 9));
        // an entry that does not read is dropped, the others kept
        let mut v = serde_json::to_value(&w).unwrap();
        v["fighters"]["finn"]["endlessRuns"] = serde_json::json!(-1);
        let r = read_waves(&v).unwrap();
        assert!(r.fighters.contains_key("shaggy") && !r.fighters.contains_key("finn"));
    }

    #[test]
    fn keys_keep_the_files_order() {
        // JSON.parse's order: the array-index keys first, ascending, then the rest as written
        let mut v = serde_json::to_value(StatsData::default()).unwrap();
        v["arcade"] = serde_json::json!({ "hardCompletions": 0, "stars": {} });
        let mut text = serde_json::to_string(&v).unwrap();
        text = text.replace(r#""stars":{}"#, r#""stars":{"zed":1,"abc":2,"10":3,"2":1,"07":2}"#);
        let StatsRead::Data(d) = read_stats(&encode_json(text.as_bytes())) else { panic!("the file reads") };
        let keys: Vec<&str> = d.arcade.stars.keys().map(String::as_str).collect();
        assert_eq!(keys, ["2", "10", "zed", "abc", "07"]);
        // the first STARS_MAX in that order are kept, and a write keeps them so
        let many: String = (0..STARS_MAX + 2).rev().map(|i| format!(r#""s{i}":1"#)).collect::<Vec<_>>().join(",");
        let text = serde_json::to_string(&v).unwrap().replace(r#""stars":{}"#, &format!(r#""stars":{{{many}}}"#));
        let StatsRead::Data(d) = read_stats(&encode_json(text.as_bytes())) else { panic!("the file reads") };
        assert_eq!(d.arcade.stars.len(), STARS_MAX);
        assert_eq!(d.arcade.stars.keys().next().map(String::as_str), Some("s257"));
        assert!(!d.arcade.stars.contains_key("s0") && !d.arcade.stars.contains_key("s1"));
        let StatsRead::Data(again) = read_stats(&encode_stats(&d)) else { panic!("the file reads") };
        assert!(again.arcade.stars.keys().eq(d.arcade.stars.keys()));
    }

    #[test]
    fn toasts_past_u32_read_whole() {
        let mut v = serde_json::to_value(StatsData::default()).unwrap();
        v["toasts"] = serde_json::json!(5_000_000_000u64);
        assert_eq!(read_data(&v).unwrap().toasts, 5_000_000_000);
        v["toasts"] = serde_json::json!(1e13);
        assert_eq!(read_data(&v).unwrap().toasts, SUM_MAX as u64);
    }

    #[test]
    fn toasts() {
        let dir = Dir::new();
        let mut a = StatsFile::open(&dir.0);
        assert_eq!(a.profile("me").toasts, TOASTS_START);
        let mut v = serde_json::to_value(StatsData::default()).unwrap();
        v.as_object_mut().unwrap().remove("toasts");
        assert_eq!(read_data(&v).unwrap().toasts, TOASTS_START);
        v["toasts"] = serde_json::json!(-3);
        assert_eq!(read_data(&v).unwrap().toasts, 0);
        v["toasts"] = serde_json::json!(1.5);
        assert_eq!(read_data(&v).unwrap().toasts, TOASTS_START);
        push(&mut a, record("t1", "me"), "me");
        assert!(
            a.profile("me").toasts == TOASTS_START + 1
                && StatsFile::open(&dir.0).profile("me").toasts == TOASTS_START + 1
        );
        let mut c = StatsFile::open(&dir.0);
        for _ in 0..TOASTS_START + 1 {
            assert!(c.spend_toast().is_some());
        }
        assert!(c.profile("me").toasts == 0 && c.spend_toast().is_none());
        assert_eq!(StatsFile::open(&Dir::new().0).spend_toast(), Some(TOASTS_START - 1));
        assert_eq!(StatsFile::open(&dir.0).profile("me").toasts, 0);
    }

    #[test]
    fn a_changed_byte_sets_the_file_aside() {
        let dir = Dir::new();
        let mut a = StatsFile::open(&dir.0);
        push(&mut a, record("m1", "me"), "me");
        push(&mut a, record("m2", "me"), "me");
        let mut bytes = fs::read(dir.file()).unwrap();
        let n = bytes.len();
        bytes[n - 3] ^= 0x40;
        fs::write(dir.file(), bytes).unwrap();
        let mut b = StatsFile::open(&dir.0);
        let p = b.profile("me");
        assert_eq!(p.reset, Some(StatsReset::Bad));
        assert!(p.matches.len() == 1 && p.matches[0].id == "m1");
        assert!(dir.names().iter().any(|f| f.starts_with("stats.") && f.ends_with(".bad")));
        assert_eq!(StatsFile::decode(&fs::read(dir.file()).unwrap()).unwrap().records.len(), 1);
        assert!(b.profile("me").reset.is_none());
    }

    #[test]
    fn no_bak_starts_empty() {
        let dir = Dir::new();
        let mut a = StatsFile::open(&dir.0);
        push(&mut a, record("m1", "me"), "me");
        fs::write(dir.file(), b"MVSS\x01 of this format but not a stats file").unwrap();
        fs::remove_file(dir.0.join("stats.mvs.bak")).ok();
        let mut b = StatsFile::open(&dir.0);
        let p = b.profile("me");
        assert!(p.reset == Some(StatsReset::Empty) && p.matches.is_empty() && p.totals.matches == 0);
        assert!(b.profile("me").reset.is_none());
        assert!(!dir.file().exists());
    }

    #[test]
    fn prune_and_new_id() {
        let dir = Dir::new();
        let mut a = StatsFile::open(&dir.0);
        for i in 0..RECORDS_KEPT + 5 {
            push(&mut a, record(&format!("p{i}"), "old-id"), "old-id");
        }
        let d = StatsFile::decode(&fs::read(dir.file()).unwrap()).unwrap();
        assert_eq!(d.records.len(), RECORDS_KEPT);
        assert_eq!(d.records[0].record.id, format!("p{}", RECORDS_KEPT + 4));
        assert_eq!(d.records[RECORDS_KEPT - 1].record.id, "p5");
        let p = StatsFile::open(&dir.0).profile("new-id");
        assert_eq!(p.matches.len(), PROFILE_MATCHES);
        let own = p.matches[0].players.iter().find(|q| q.slot == 0).unwrap();
        assert!(own.id == "new-id" && p.matches[0].players[1].id == "other");
    }

    #[test]
    fn a_crash_before_the_rename_keeps_the_old_file() {
        let dir = Dir::new();
        push(&mut StatsFile::open(&dir.0), record("m1", "me"), "me");
        let before = fs::read(dir.file()).unwrap();
        let crash = Arc::new(AtomicBool::new(true));
        let mut c = StatsFile::open(&dir.0);
        c.crash_before_rename(crash.clone());
        push(&mut c, record("m2", "me"), "me");
        assert!(c.unsaved);
        assert_eq!(fs::read(dir.file()).unwrap(), before);
        let p = StatsFile::open(&dir.0).profile("me");
        assert!(p.reset.is_none() && p.matches.len() == 1);
        crash.store(false, Ordering::Relaxed);
        push(&mut c, record("m3", "me"), "me");
        assert!(!c.unsaved);
        assert_eq!(StatsFile::open(&dir.0).profile("me").matches.len(), 3);
    }

    #[test]
    fn caps_and_refusals() {
        // sums past SUM_MAX read back capped
        let mut v = serde_json::to_value(StatsData::default()).unwrap();
        v["totals"]["damage"] = serde_json::json!(1e13);
        v["trackers"]["totals"] = serde_json::json!({ "TotalRingouts": 1e13, "NotATracker": 4 });
        v["trackers"]["characters"] = serde_json::json!({ "shaggy": [1, 2, 3, 4], "notacharacter": [0, 0, 0, 0] });
        let d = read_data(&v).unwrap();
        assert_eq!(d.totals.damage, SUM_MAX as u64);
        assert_eq!(d.trackers.totals["TotalRingouts"], SUM_MAX);
        assert!(!d.trackers.totals.contains_key("NotATracker") && !d.trackers.characters.contains_key("notacharacter"));
        assert_eq!(d.trackers.characters["shaggy"], [1.0, 2.0, 3.0, 4.0]);
        // a record: a name past NAME_LIMIT, more than 8 players, a repeated slot; a big match's eight read
        let ok = serde_json::to_value(record("c", "me")).unwrap();
        assert!(read_record(&ok).is_some());
        let mut named = ok.clone();
        named["players"][1]["name"] = serde_json::json!("x".repeat(NAME_LIMIT + 1));
        assert!(read_record(&named).is_none());
        let p = ok["players"][1].clone();
        let players = |n: usize| {
            let list: Vec<Value> = (0..n)
                .map(|i| {
                    let mut q = p.clone();
                    q["slot"] = serde_json::json!(i);
                    q["team"] = serde_json::json!(i);
                    q
                })
                .collect();
            Value::Array(list)
        };
        let mut eight = ok.clone();
        eight["players"] = players(8);
        eight["winningTeam"] = serde_json::json!(7);
        assert_eq!(read_record(&eight).map(|r| r.players.len()), Some(8));
        let mut many = ok.clone();
        many["players"] = players(9);
        assert!(read_record(&many).is_none());
        let mut twice = ok.clone();
        twice["players"][1]["slot"] = serde_json::json!(0);
        assert!(read_record(&twice).is_none());
        let mut big = ok.clone();
        big["players"][0]["damage"] = serde_json::json!(u64::MAX / 4);
        assert_eq!(read_record(&big).unwrap().players[0].damage, DAMAGE_MAX as u32);
        // a body that inflates past FILE_MAX
        let mut v = serde_json::to_value(StatsData::default()).unwrap();
        v["pad"] = serde_json::json!(" ".repeat(FILE_MAX));
        let json = serde_json::to_vec(&v).unwrap();
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(&json).unwrap();
        let body = xor(&z.finish().unwrap());
        let mut header = MAGIC.to_vec();
        header.push(FORMAT_VERSION);
        let mut bomb = header.clone();
        bomb.extend_from_slice(&check_of(&header, &body));
        bomb.extend_from_slice(&body);
        assert!(bomb.len() < FILE_MAX / 100);
        assert_eq!(read_stats(&bomb), StatsRead::Bad);
    }

    #[test]
    fn a_newer_format_is_left_as_it_is() {
        let dir = Dir::new();
        push(&mut StatsFile::open(&dir.0), record("n0", "me"), "me");
        let newer = b"MVSS\x09 the body of a later format".to_vec();
        fs::write(dir.file(), &newer).unwrap();
        let mut a = StatsFile::open(&dir.0);
        let p = a.profile("me");
        a.spend_toast();
        assert!(p.reset.is_none() && p.matches.is_empty());
        assert_eq!(fs::read(dir.file()).unwrap(), newer);
        assert!(!dir.names().iter().any(|f| f.ends_with(".bad")));
    }

    #[test]
    fn the_stamp_reads_as_the_ts_one() {
        let s = stamp();
        assert_eq!(s.len(), 24, "{s}");
        assert!(s.starts_with("20") && s.ends_with('Z') && !s.contains(':') && !s.contains('.'));
    }
}
