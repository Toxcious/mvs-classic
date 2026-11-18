//! The `soundbank` kind: one entry per AkAudioEvent (the hierarchy its own bank holds for it,
//! [`mvs_data::sound::EventBank`]) and the InitBank's ([`mvs_data::sound::InitBank`]: the bus tree with its effects
//! and AkRoomVerb's early reflection taps, the attenuations, the game parameters' defaults and ramps, the event
//! index), each a typed record ([`mvs_data::sound::encode_bank`]). Every event of the build and the whole music tree,
//! English(US) only:
//!
//! - an event's bank is its AkAudioEventData's. A localized event (LocalizedPlatformAssetDataMap) always takes its
//!   English(US) package's data, never its own package's; one with no such key, or whose package the paks lack, has
//!   no bank and no entry;
//! - the Event is the one whose id is the ShortID of the asset's name (a post sends the name's id, never the
//!   event's own ShortID); the other Events the bank embeds (PlayEvent targets) are `posted`;
//! - the objects are compacted (`compact.rs`); within one bank the first copy of an id is kept;
//! - each medium of MediaList is its path and its wem's meta (`media.rs`); a medium the paks hold no data for is
//!   left out, as the sound engine finds no media for it;
//! - the InitBank entry is built from the notes of every event entry (its index line, its bank's attenuations): an
//!   event that fails is left out of it and fails under its own path.

use std::collections::BTreeMap;
use std::sync::Arc;

use indexmap::IndexMap;
use mvs_data::sound::*;
use mvs_pak::paths::split_object_path;
use mvs_pak::setup::{build_list, check_table, read_build_file};
use mvs_uasset::natives::wwise::bank_hidden;
use mvs_uasset::value::Value;
use mvs_uasset::{Package, PackageCache};
use rayon::prelude::*;

use crate::kind::{Cx, Item, Kind, KindError, Memo};
use crate::objview::{field, num, string};

mod compact;
mod er;
mod fx;
mod init;
mod media;
mod tree;

use compact::{Compact, compact, hirc_id, hirc_type};
use tree::{R, fail, list};

const AK_EVENT: &str = "/Script/AkAudio.AkAudioEvent";
/// The culture a localized event's bank is taken from.
const AUDIO_CULTURE: &str = "English(US)";

pub struct Soundbank;

impl Kind for Soundbank {
    fn id(&self) -> &'static str {
        BANK_KIND
    }

    fn version(&self) -> u64 {
        BANK_VERSION
    }

    fn content_type(&self, _bytes: &[u8]) -> &'static str {
        "application/octet-stream"
    }

    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        let mut out: Vec<Item> = event_paths(cx)?.iter().map(Item::new).collect();
        out.push(Item::new(INIT_BANK));
        Ok(out)
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        if item.path == INIT_BANK {
            return Ok(encode_bank(&init_bank(cx)?));
        }
        let notes = cx.shared.get_or_init(Notes::default);
        let mut bytes = None;
        let note = notes.0.get(&item.path, || {
            let (bank, note) = event_entry(cx, &item.path)?;
            bytes = Some(encode_bank(&bank));
            Ok(Arc::new(note))
        });
        note?;
        match bytes {
            Some(b) => Ok(b),
            // noted by the InitBank's build first: its bank was not kept
            None => Ok(encode_bank(&event_entry(cx, &item.path)?.0)),
        }
    }
}

// ---- the events ---------------------------------------------------------------------------------------------------

/// What the InitBank reads of an event entry that built.
struct Note {
    name: String,
    localized: bool,
    dur: [Option<f64>; 2],
    pairs: bool,
    atts: Vec<(Id, Att)>,
}

/// The notes by event path, made once per run.
#[derive(Default)]
struct Notes(Memo<Result<Arc<Note>, KindError>>);

/// Every medium's meta by path, made once per run (a medium is shared by many events); `None`: the paks hold no data.
#[derive(Default)]
struct Metas(Memo<Result<Option<Arc<MediaRef>>, KindError>>);

/// The events that get an entry, in catalog order; computed once per run.
struct EventPaths(Result<Vec<String>, KindError>);

fn event_paths(cx: &Cx<'_>) -> Result<Vec<String>, KindError> {
    cx.shared
        .get_or_init(|| {
            let all = cx.catalog.paths_of(&[AK_EVENT]);
            let ok: Result<Vec<bool>, KindError> = all.par_iter().map(|p| has_bank(cx, p)).collect();
            EventPaths(ok.map(|ok| all.into_iter().zip(ok).filter(|(_, ok)| *ok).map(|(p, _)| p).collect()))
        })
        .0
        .clone()
}

/// A localized event's English(US) data path: `Ok(None)` for a localized event with no such key, `Err(())` for an
/// event that is not localized.
fn culture_data(cache: &PackageCache, map: Option<&Value>) -> Result<Option<String>, ()> {
    let Some(Value::Map(m)) = map else { return Err(()) };
    if m.is_empty() {
        return Err(());
    }
    Ok(m.iter().find(|(k, _)| string(cache, k).as_deref() == Some(AUDIO_CULTURE)).and_then(|(_, v)| string(cache, v)))
}

/// Whether an event has a bank in the paks: a localized event with no English(US) key, or whose package the paks do
/// not hold, loads none, so the sound engine knows no such event.
fn has_bank(cx: &Cx<'_>, path: &str) -> Result<bool, KindError> {
    let (pkg, i) = cx.read(path)?;
    let obj = cx.cache.object(&pkg, i);
    match culture_data(cx.cache, field(&obj.props, "LocalizedPlatformAssetDataMap")) {
        Err(()) => Ok(true),
        Ok(None) => Ok(false),
        Ok(Some(en)) => {
            let file = cx.paths.package_file(&en);
            Ok(file.is_some_and(|f| cx.paks.entry(&format!("{f}.uasset")).is_some()))
        }
    }
}

/// One event's bank as the build loads it: the tree, what the tree leaves out, the media paths, the durations.
struct EventData {
    hier: Vec<Value>,
    hidden: Vec<mvs_uasset::natives::wwise_bank::ItemHidden>,
    media: Vec<String>,
    dur: [Option<f64>; 2],
}

fn event_data(cx: &Cx<'_>, pkg: &Arc<Package>, what: &str) -> R<Vec<EventData>> {
    let mut out = Vec::new();
    for k in 0..pkg.exports.len() {
        let o = cx.cache.object(pkg, k);
        if o.type_name != "AkAudioEventData" {
            continue;
        }
        if let Some(e) = o.issues.first() {
            return fail(what, &e.message);
        }
        let Some(bank) = o.data.get("Data") else { return fail(what, format!("{}'s bank was not read", o.name)) };
        let (hier, hidden) = match bank {
            Value::Struct(_) => {
                let hier = list(bank, "Hierarchies").to_vec();
                let hidden = bank_hidden(&o).unwrap_or_default();
                if hidden.len() != hier.len() {
                    return fail(
                        what,
                        format!("{}'s bank: {} hidden entries for {} items", o.name, hidden.len(), hier.len()),
                    );
                }
                (hier, hidden)
            }
            _ => (Vec::new(), Vec::new()),
        };
        let mut media = Vec::new();
        if let Some(Value::Array(a)) = field(&o.props, "MediaList") {
            for m in a {
                match string(cx.cache, m) {
                    Some(s) => media.push(s),
                    None => return fail(what, "a MediaList entry is not resolved"),
                }
            }
        }
        let dur = |k: &str| field(&o.props, k).and_then(num);
        out.push(EventData { hier, hidden, media, dur: [dur("MinimumDuration"), dur("MaximumDuration")] });
    }
    Ok(out)
}

/// One event's bank as the build loads it.
struct EventSource {
    name: String,
    localized: bool,
    from: String,
    data: EventData,
}

fn event_source(cx: &Cx<'_>, path: &str) -> Result<EventSource, KindError> {
    let (pkg, i) = cx.read(path)?;
    let o = cx.object(path)?;
    let obj = cx.cache.object(&pkg, i);
    if obj.type_name != "AkAudioEvent" {
        return Err(KindError::NotFound(format!("soundbank: {path} is a {}, not an AkAudioEvent", obj.type_name)));
    }
    let culture = culture_data(cx.cache, o.prop("LocalizedPlatformAssetDataMap"));
    let (mut data, mut from) = match culture {
        Err(()) => (event_data(cx, &pkg, path)?, pkg.path.clone()),
        Ok(_) => (Vec::new(), pkg.path.clone()),
    };
    let localized = culture.is_ok();
    if localized {
        let Ok(Some(en)) = culture else {
            return Err(KindError::NotFound(format!(
                "soundbank: {path}: a localized event with no {AUDIO_CULTURE} data"
            )));
        };
        let p = split_object_path(&en).0;
        let Some(epkg) = cx.cache.package(p)? else {
            return Err(KindError::NotFound(format!("content: the paks hold no package {p}")));
        };
        data = event_data(cx, &epkg, &en)?;
        from = epkg.path.clone();
    }
    if data.len() != 1 {
        return fail(path, format!("{} AkAudioEventData in {from}", data.len()));
    }
    let data = data.remove(0);
    Ok(EventSource { name: obj.name.clone(), localized, from, data })
}

/// A medium's package name, the last component of its package path: its id.
fn media_id(path: &str) -> Option<Id> {
    let pkg = split_object_path(path).0;
    pkg[pkg.rfind('/').map_or(0, |s| s + 1)..].parse().ok()
}

/// A medium's meta, by path (`None`: the paks hold no data for it).
fn meta_of(cx: &Cx<'_>, path: &str) -> Result<Option<Arc<MediaRef>>, KindError> {
    let metas = cx.shared.get_or_init(Metas::default);
    metas.0.get(path, || {
        let Some(wem) = media::read_wem(cx, path)? else { return Ok(None) };
        Ok(Some(Arc::new(media::media_ref(path, &wem, path)?)))
    })
}

/// An event's entry, and what the InitBank reads of it.
fn event_entry(cx: &Cx<'_>, path: &str) -> Result<(EventBank, Note), KindError> {
    let src = event_source(cx, path)?;
    let what = format!("{path} ({})", src.from);
    let name_id = short_id(&src.name);
    let mut ev = None;
    for h in &src.data.hier {
        if hirc_type(h) == "Event" && hirc_id(h, &what)? == name_id {
            ev = Some(h);
            break;
        }
    }
    let Some(ev) = ev else { return fail(&what, format!("no Event {name_id} (its name's id) in its bank")) };
    let ev_actions = match compact(ev, &Default::default(), &what)? {
        Compact::Event(a) => a,
        _ => return fail(&what, "the event is not an Event"),
    };
    let mut posted: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
    let mut objs: BTreeMap<Id, Obj> = BTreeMap::new();
    let mut atts: Vec<(Id, Att)> = Vec::new();
    for (h, hid) in src.data.hier.iter().zip(&src.data.hidden) {
        let id = hirc_id(h, &what)?;
        match compact(h, hid, &what)? {
            Compact::Event(actions) => {
                if id != name_id && !posted.contains_key(&id) {
                    posted.insert(id, actions);
                }
            }
            Compact::Attenuation(a) => atts.push((id, a)),
            Compact::Obj(o) => {
                objs.entry(id).or_insert(o);
            }
        }
    }
    let mut media: BTreeMap<Id, MediaRef> = BTreeMap::new();
    for m in &src.data.media {
        let Some(mid) = media_id(m) else { return fail(&what, format!("medium {m}: its package name is not an id")) };
        if media.contains_key(&mid) {
            continue;
        }
        if let Some(meta) = meta_of(cx, m)? {
            media.insert(mid, (*meta).clone());
        }
    }
    let bank =
        EventBank { name: src.name.clone(), id: name_id, dur: src.data.dur, actions: ev_actions, posted, objs, media };
    let note = Note { name: src.name, localized: src.localized, dur: bank.dur, pairs: event_pairs(&bank), atts };
    Ok((bank, note))
}

/// Whether an event starts or ends something that does not end by itself: one of its actions is a Stop, or it plays
/// (through PlayEvent too) a sound whose loop property (58) is 0, the test WwiseAudio admits and streams a voice by.
/// Read over the event's own bank, which holds everything the event plays and the events it posts.
pub fn event_pairs(bank: &EventBank) -> bool {
    struct Walk<'a> {
        bank: &'a EventBank,
        seen: std::collections::HashSet<Id>,
        seen_ev: std::collections::HashSet<Id>,
    }
    impl Walk<'_> {
        fn walk(&mut self, id: Id) -> bool {
            if !self.seen.insert(id) {
                return false;
            }
            let Some(o) = self.bank.objs.get(&id) else { return false };
            match o {
                Obj::EventAction(a) => match a.kind {
                    ActionKind::Stop => true,
                    ActionKind::Play => a.target.is_some_and(|t| self.walk(t)),
                    ActionKind::PlayEvent => {
                        let Some(e) = a.target else { return false };
                        if !self.seen_ev.insert(e) {
                            return false;
                        }
                        let bank = self.bank;
                        bank.posted.get(&e).is_some_and(|acts| acts.iter().any(|&x| self.walk(x)))
                    }
                    _ => false,
                },
                Obj::SoundSfxVoice(s) => s.node.p.get(58) == Some(0.0),
                o => o.children().iter().any(|&c| self.walk(c)),
            }
        }
    }
    let mut w = Walk { bank, seen: Default::default(), seen_ev: [bank.id].into() };
    bank.actions.iter().any(|&a| w.walk(a))
}

// ---- the InitBank -------------------------------------------------------------------------------------------------

fn init_bank(cx: &Cx<'_>) -> Result<InitBank, KindError> {
    let notes = cx.shared.get_or_init(Notes::default);
    let (pkg, _) = cx.read(INIT_BANK)?;
    let mut tree = None;
    for k in 0..pkg.exports.len() {
        let o = cx.cache.object(&pkg, k);
        if o.type_name != "AkInitBankAssetData" {
            continue;
        }
        if let Some(e) = o.issues.first() {
            return fail(INIT_BANK, &e.message);
        }
        tree = o.data.get("Data").cloned();
        break;
    }
    let Some(tree) = tree else { return fail(INIT_BANK, "no AkInitBankAssetData") };
    let dll = read_build_file(cx.paks, er::ROOMVERB_DLL).map_err(|e| KindError::Failed(e.to_string()))?;
    let (patterns, table) = er::read_er_patterns(&dll).map_err(KindError::Failed)?;
    let files = build_list().map_err(|e| KindError::Failed(e.to_string()))?.files;
    check_table(&files, er::ROOMVERB_DLL, er::TABLE, &table).map_err(|e| KindError::Failed(e.to_string()))?;
    let parts = init::init_parts(
        &tree,
        &|i| match patterns.get(i as usize) {
            Some(p) => Ok(p.clone()),
            None => fail(INIT_BANK, format!("RoomVerb pattern {i} of {}", patterns.len())),
        },
        INIT_BANK,
    )?;
    let mut atts: BTreeMap<Id, Att> = BTreeMap::new();
    let mut events: IndexMap<String, EventEntry> = IndexMap::new();
    for path in event_paths(cx)? {
        // an event that failed is left out and fails under its own path
        let note = notes.0.get(&path, || event_entry(cx, &path).map(|(_, n)| Arc::new(n)));
        // (the run trims the package cache between tasks; this one job reads every event's package and media, past
        // the cache's budget many times over without a trim of its own)
        cx.cache.trim();
        let Ok(s) = note else { continue };
        // Wwise event names are case-insensitive (the runtime looks them up in lower case)
        let key = s.name.to_lowercase();
        if let Some(was) = events.get(&key) {
            return fail(INIT_BANK, format!("two events named {key}: {} and {path}", was.path));
        }
        events.insert(key, EventEntry { path: path.clone(), localized: s.localized, dur: s.dur, pairs: s.pairs });
        for (id, a) in &s.atts {
            match atts.get(id) {
                None => {
                    atts.insert(*id, a.clone());
                }
                Some(was) if was != a => {
                    return fail(INIT_BANK, format!("attenuation {id}: {path}'s copy differs from an earlier bank's"));
                }
                Some(_) => {}
            }
        }
    }
    Ok(InitBank {
        buses: parts.buses,
        atts,
        rtpc_defaults: parts.rtpc_defaults,
        rtpc_ramps: parts.rtpc_ramps,
        state_groups: parts.state_groups,
        volume_threshold: parts.volume_threshold,
        max_voices: parts.max_voices,
        events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bank(objs: Vec<(Id, Obj)>, posted: Vec<(Id, Vec<Id>)>) -> EventBank {
        EventBank {
            id: 1,
            actions: vec![10],
            posted: posted.into_iter().collect(),
            objs: objs.into_iter().collect(),
            ..EventBank::default()
        }
    }

    fn play(target: Id) -> Obj {
        Obj::EventAction(Action { kind: ActionKind::Play, target: Some(target), ..Action::default() })
    }

    fn sound(loop_prop: Option<f64>) -> Obj {
        let p = loop_prop.map(|v| vec![(58, v)]).unwrap_or_default();
        Obj::SoundSfxVoice(Sound { node: Node { p: Props(p), ..Node::default() }, ..Sound::default() })
    }

    #[test]
    fn pairs_by_stop_or_infinite_loop() {
        // a play of a one-shot does not pair; of a sound with an infinite loop it does
        let one_shot = bank(vec![(10, play(20)), (20, sound(Some(1.0)))], vec![]);
        assert!(!event_pairs(&one_shot));
        let looping = bank(vec![(10, play(20)), (20, sound(Some(0.0)))], vec![]);
        assert!(event_pairs(&looping));
        let stop = bank(vec![(10, Obj::EventAction(Action { kind: ActionKind::Stop, ..Action::default() }))], vec![]);
        assert!(event_pairs(&stop));
        // a sound with no loop property is a one-shot
        assert!(!event_pairs(&bank(vec![(10, play(20)), (20, sound(None))], vec![])));
    }

    #[test]
    fn pairs_through_posted_events_and_containers() {
        let post = Obj::EventAction(Action { kind: ActionKind::PlayEvent, target: Some(2), ..Action::default() });
        let cntr = Obj::ActorMixer(Parent { node: Node::default(), children: vec![21] });
        let b = bank(vec![(10, post), (11, play(20)), (20, cntr), (21, sound(Some(0.0)))], vec![(2, vec![11])]);
        assert!(event_pairs(&b));
        // an event that posts itself, or one the bank does not hold, ends the walk
        let own = Obj::EventAction(Action { kind: ActionKind::PlayEvent, target: Some(1), ..Action::default() });
        assert!(!event_pairs(&bank(vec![(10, own)], vec![])));
        let missing = Obj::EventAction(Action { kind: ActionKind::PlayEvent, target: Some(3), ..Action::default() });
        assert!(!event_pairs(&bank(vec![(10, missing)], vec![])));
    }
}
