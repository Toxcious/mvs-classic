//! Scopes: the banks and media a part of the game holds while it runs (a match's characters, a stage, the menus),
//! read on the game's side and handed to the mixer whole: the short media decoded, the long ones compressed (the
//! streamer decodes them as they play); a scope's lazy events' short media decoded when one is first posted.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use mvs_data::sound::{ActionKind, EventBank, Id, InitBank, Obj, short_id};

use crate::decode::cache::{Hold, MediaCache};
use crate::decode::worker::Done;
use crate::decode::{Coded, Pcm};
use crate::{AudioError, Cmd, Key, Note, Post};

/// A scope's load: its banks by index name, its short media decoded, its long and streamed media compressed for the
/// streamer to decode as they play, and its lazy events with the media a post of one decodes first (the short ones
/// not kept by the load), with their entries' paths, and its events that hold a Stop action (a post of one with a
/// stop key cancels the lazy posts of that key still waiting).
#[derive(Debug, Default)]
pub struct ScopeLoad {
    pub banks: Vec<(String, Arc<EventBank>)>,
    pub media: Vec<Arc<Pcm>>,
    pub streamed: Vec<Arc<Coded>>,
    pub lazy: Vec<(Id, Box<[Id]>)>,
    pub lazy_paths: Vec<(Id, Box<str>)>,
    pub stops: Vec<Id>,
}

/// The banks of `names` and of the events their PlayEvent actions post, in order (each name's, then the ones it
/// posts): an event's bank holds the hierarchy of the events it posts but not their media, which are in the posted
/// event's own bank. Names the index does not have are left out.
pub(crate) fn banks(
    store: &mvs_store::Store,
    init: &InitBank,
    names: &[&str],
) -> Result<Vec<(String, Arc<EventBank>)>, AudioError> {
    let by_id: HashMap<Id, &str> = init.events.keys().map(|n| (short_id(n), n.as_str())).collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = Vec::new();
    for n in names {
        let n = n.to_lowercase();
        if init.events.contains_key(&n) && seen.insert(n.clone()) {
            queue.push(n);
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < queue.len() {
        let n = queue[i].clone();
        i += 1;
        // (the store's parsed copy: a scope of the same events again reads and parses nothing)
        let b = mvs_data::sound::shared_bank(store, &init.events[&n].path)?;
        for id in b.posted.keys() {
            if let Some(p) = by_id.get(id)
                && seen.insert((*p).to_owned())
            {
                queue.push((*p).to_owned());
            }
        }
        out.push((n, b));
    }
    Ok(out)
}

/// The media an event can play: its actions' Play targets down the hierarchy to the
/// sounds, and the events its PlayEvent actions post, over the banks of a load merged in order.
pub(crate) fn media_for(merged: &super::bank::Banks, banks: &[(String, Arc<EventBank>)], ev: &str) -> Vec<Id> {
    let Some((_, own)) = banks.iter().find(|(n, _)| n == ev) else { return Vec::new() };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut seen_ev = HashSet::from([own.id]);
    let mut stack: Vec<Id> = merged.ev_obj(own.id).unwrap_or_default().iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        match merged.obj(id) {
            Some(Obj::EventAction(a)) => match (a.kind, a.target) {
                (ActionKind::Play, Some(t)) => stack.push(t),
                (ActionKind::PlayEvent, Some(t)) if seen_ev.insert(t) => {
                    if let Some(acts) = merged.ev_obj(t) {
                        stack.extend(acts.iter().rev());
                    }
                }
                _ => {}
            },
            Some(Obj::SoundSfxVoice(s)) => {
                if s.plugin.as_deref() != Some("AkFxSrcSilenceSource") && !out.contains(&s.src) {
                    out.push(s.src);
                }
            }
            Some(o) => stack.extend(o.children().iter().rev()),
            None => {}
        }
    }
    out
}

/// Reads the banks of the events of `keep` and `lazy` and holds their media: a long medium of either, and a
/// streamed one, compressed (decoded as it plays), a short one of `keep`'s decoded, and a short one of `lazy`'s
/// listed for its first post. A medium `media` holds already is taken from it; with the load, how many media it
/// decoded.
pub fn load(
    store: &mvs_store::Store,
    init: &InitBank,
    media: &MediaCache,
    keep: &[&str],
    lazy: &[&str],
) -> Result<(ScopeLoad, usize), AudioError> {
    let names: Vec<&str> = keep.iter().chain(lazy).copied().collect();
    let banks = banks(store, init, &names)?;
    let mut merged = super::bank::Banks::default();
    merged.add(banks.iter().cloned());
    let pack = mvs_data::sound::sound_pack(store)?;
    let mut load = ScopeLoad::default();
    let mut decoded = 0;
    let mut done = HashSet::new();
    let call = |m: Id| move |e: crate::decode::DecodeError| AudioError::Call(format!("medium {m}: {e}"));
    // (the kept media in the order the events list them, read and decoded on the loader's threads: the cache's
    // misses at once)
    let mut wanted: Vec<(Id, &str, bool)> = Vec::new();
    for ev in keep {
        for m in media_for(&merged, &banks, &ev.to_lowercase()) {
            if !done.insert(m) {
                continue;
            }
            let Some(r) = merged.media(m) else { continue };
            wanted.push((m, r.path.as_str(), merged.streamed(m)));
        }
    }
    let got = mvs_load::map(&wanted, |&(m, path, streamed)| media.hold(&pack, m, path, streamed));
    for (got, &(m, ..)) in got.into_iter().zip(&wanted) {
        match got.map_err(call(m))? {
            Hold::Pcm(got) => {
                decoded += usize::from(got.decoded);
                load.media.push(got.pcm);
            }
            Hold::Coded(c) => load.streamed.push(c),
        }
    }
    // (the lazy media: a long one held at once, a short one listed under each lazy event that plays it, its path
    // once)
    let mut lazy_media: Vec<(Id, &str, bool)> = Vec::new();
    let mut seen = HashSet::new();
    let lazy: Vec<(String, Vec<Id>)> = lazy
        .iter()
        .map(|ev| {
            let ev = ev.to_lowercase();
            let list: Vec<Id> = media_for(&merged, &banks, &ev).into_iter().filter(|m| !done.contains(m)).collect();
            for &m in &list {
                if let Some(r) = merged.media(m)
                    && seen.insert(m)
                {
                    lazy_media.push((m, r.path.as_str(), merged.streamed(m)));
                }
            }
            (ev, list)
        })
        .collect();
    let got = mvs_load::map(&lazy_media, |&(m, path, streamed)| {
        if streamed { media.coded(&pack, m, path).map(Some) } else { media.long(&pack, m, path) }
    });
    for (got, &(m, path, _)) in got.into_iter().zip(&lazy_media) {
        match got.map_err(call(m))? {
            Some(c) => {
                done.insert(m);
                load.streamed.push(c);
            }
            None => load.lazy_paths.push((m, path.into())),
        }
    }
    for (ev, list) in lazy {
        let list: Vec<Id> = list.into_iter().filter(|m| !done.contains(m) && merged.media(*m).is_some()).collect();
        if !list.is_empty() {
            load.lazy.push((short_id(&ev), list.into_boxed_slice()));
        }
    }
    for (n, _) in &banks {
        if has_stop(&merged, short_id(n)) {
            load.stops.push(short_id(n));
        }
    }
    load.banks = banks;
    Ok((load, decoded))
}

/// Whether an event's actions, or those of the events its PlayEvent actions post, hold a Stop: a post of it with a
/// stop key stops that key's sounds (ctl/post.rs).
fn has_stop(merged: &super::bank::Banks, ev: Id) -> bool {
    let mut seen = HashSet::from([ev]);
    let mut stack = vec![ev];
    while let Some(e) = stack.pop() {
        for &a in merged.ev_obj(e).unwrap_or_default() {
            let Some(Obj::EventAction(x)) = merged.obj(a) else { continue };
            match (x.kind, x.target) {
                (ActionKind::Stop, _) => return true,
                (ActionKind::PlayEvent, Some(t)) if seen.insert(t) => stack.push(t),
                _ => {}
            }
        }
    }
    false
}

/// A post held for the lazy media it plays, and when it was posted.
struct Waiting {
    tick: u32,
    post: Post,
    need: Vec<Id>,
    at: Instant,
}

/// A scope as the game side holds it: the media its load kept or streams, its lazy events' media and its events
/// that hold a Stop.
#[derive(Default)]
struct Held {
    keep: HashSet<Id>,
    lazy: HashMap<Id, Box<[Id]>>,
    stops: HashSet<Id>,
}

/// How late a post waiting for its media may go out (past it the sound would be badly out of
/// sync, and it is dropped).
pub const LATE: Duration = Duration::from_millis(150);

/// The game side's view of the scopes' media and the posts of lazy
/// events waiting for theirs. A post of a lazy event whose media are not decoded yet decodes them (on the decode
/// worker, or on the calling thread offline), sends them to the mixer under the scope that lists them and then the
/// post; a post still waiting [`LATE`] after it was made is dropped. A
/// post dropped or cancelled here gives its EventEnd note at once.
// Deviation: a lazy sound starts late by its decode. The build holds a sound by its srcType: of the 575 lazy lines
// of the first two characters' 200 are Streaming (read from disk when posted, no prefetch: all Shaggy's), 14
// PrefetchStreaming and 361 Data (in the bank's memory: all of Morty's); the stream's start latency in the build is
// not measured.
// Unverified: the whole post waits for every medium the event can play and is taken with the game syncs and
// emitters of when it goes out, and a late post drops the event's sounds that had their media too; the build's wait
// per voice for the medium its pick plays is not read.
// Unverified: a stop cancels a waiting post only by key (stopKey, stopAll, a post with a stop key of an event that
// holds a Stop action, the action's target not checked); a Stop action's target and a Stop by object do not.
// A lazy medium decoded stays held under its scope until the scope is set again or let go.
// Deviation: offline (the checks' renders) the decode is on the calling thread and the post is never late: the run
// has no wall clock of its own.
pub(crate) struct Lazy {
    /// By scope (its name's hash), in the order first made.
    scopes: IndexMap<u64, Held>,
    /// The lazy media's entries in the `sound` pack.
    paths: HashMap<Id, Box<str>>,
    /// The lazy media decoded and sent to the mixer, by the scope they are held under.
    sent: HashMap<Id, u64>,
    decoding: HashSet<Id>,
    waiting: Vec<Waiting>,
    /// Started on the first lazy post of a live run.
    worker: Option<crate::decode::worker::Worker>,
    /// Decoded on the worker and the posts held; else on the calling thread.
    pub(crate) live: bool,
    pack: Option<Arc<mvs_store::Pack>>,
    /// The decoded media shared with the scopes' loads.
    media: Arc<MediaCache>,
    pub(crate) late: Duration,
    /// What the game side could not do, and the ends of the posts it let go, for [`crate::Audio::take_notes`].
    pub(crate) notes: Vec<Note>,
}

/// Sends a command to the mixer (`crate::send`: nothing is lost).
pub(crate) type ToMixer<'a> = &'a mut dyn FnMut(u32, Cmd);

impl Lazy {
    pub(crate) fn new(live: bool, media: Arc<MediaCache>) -> Lazy {
        Lazy {
            scopes: IndexMap::new(),
            paths: HashMap::new(),
            sent: HashMap::new(),
            decoding: HashSet::new(),
            waiting: Vec::new(),
            worker: None,
            live,
            pack: None,
            media,
            late: LATE,
            notes: Vec::new(),
        }
    }

    /// A scope's load is sent: its lazy lists are taken out of it (the mixer has no use for them). A scope set
    /// again replaces its media in the mixer, the lazy ones it was sent among them.
    pub(crate) fn scoped(&mut self, owner: u64, load: &mut ScopeLoad) {
        let keep = load.media.iter().map(|p| p.id).chain(load.streamed.iter().map(|s| s.id)).collect();
        let lazy = std::mem::take(&mut load.lazy).into_iter().collect();
        let stops = std::mem::take(&mut load.stops).into_iter().collect();
        self.paths.extend(std::mem::take(&mut load.lazy_paths));
        self.scopes.insert(owner, Held { keep, lazy, stops });
        self.sent.retain(|_, o| *o != owner);
    }

    /// A scope let go: the mixer lets its media go, the lazy ones it was sent among them.
    pub(crate) fn unscoped(&mut self, owner: u64) {
        self.scopes.shift_remove(&owner);
        self.sent.retain(|_, o| *o != owner);
    }

    /// Whether a scope's event holds a Stop action.
    pub(crate) fn stops(&self, event: Id) -> bool {
        self.scopes.values().any(|h| h.stops.contains(&event))
    }

    /// The first scope whose lazy events play this medium.
    fn owner_of(&self, m: Id) -> Option<u64> {
        self.scopes.iter().find(|(_, h)| h.lazy.values().any(|l| l.contains(&m))).map(|(o, _)| *o)
    }

    fn pinned(&self, m: Id) -> bool {
        self.scopes.values().any(|h| h.keep.contains(&m))
    }

    /// Posts an event: at once, or once the lazy media it plays are decoded and sent.
    pub(crate) fn post(&mut self, store: &Arc<mvs_store::Store>, tick: u32, post: Post, send: ToMixer) {
        let need: Vec<Id> = match self.scopes.values().find_map(|h| h.lazy.get(&post.event)) {
            Some(list) => list.iter().copied().filter(|m| !self.pinned(*m) && !self.sent.contains_key(m)).collect(),
            None => Vec::new(),
        };
        if need.is_empty() {
            send(tick, Cmd::Post(post));
            return;
        }
        if !self.live {
            let mut ready = Vec::new();
            for &m in &need {
                let pcm = self.decode_here(store, m);
                self.finished(Done { id: m, pcm }, &mut ready);
            }
            self.send_media(ready, tick, send);
            send(tick, Cmd::Post(post));
            return;
        }
        let worker =
            self.worker.get_or_insert_with(|| crate::decode::worker::Worker::start(store.clone(), self.media.clone()));
        for &m in &need {
            match self.paths.get(&m) {
                Some(path) if self.decoding.insert(m) => worker.submit(m, path),
                _ => {}
            }
        }
        let need = need.into_iter().filter(|m| self.decoding.contains(m)).collect();
        self.waiting.push(Waiting { tick, post, need, at: Instant::now() });
    }

    fn decode_here(&mut self, store: &mvs_store::Store, m: Id) -> Result<Arc<Pcm>, String> {
        let Some(path) = self.paths.get(&m) else { return Err("no entry".to_owned()) };
        if self.pack.is_none() {
            self.pack = Some(mvs_data::sound::sound_pack(store).map_err(|e| e.to_string())?);
        }
        let pack = self.pack.as_ref().expect("the sound pack, just opened");
        self.media.medium(pack, m, path).map(|g| g.pcm).map_err(|e| e.to_string())
    }

    /// A medium decoded: held under the first scope that lists it lazy (dropped when none does any more).
    fn finished(&mut self, d: Done, ready: &mut Vec<(u64, Arc<Pcm>)>) {
        self.decoding.remove(&d.id);
        match d.pcm {
            Ok(p) => {
                if let Some(o) = self.owner_of(d.id) {
                    self.sent.insert(d.id, o);
                    ready.push((o, p));
                }
            }
            Err(e) => {
                eprintln!("audio: medium {}: {e}", d.id);
                self.notes.push(Note::Warn { what: "lazy medium not decoded", id: u64::from(d.id) });
            }
        }
    }

    /// The media for the mixer, one command per scope.
    fn send_media(&mut self, mut ready: Vec<(u64, Arc<Pcm>)>, tick: u32, send: ToMixer) {
        while let Some(&(owner, _)) = ready.first() {
            let (mine, rest): (Vec<_>, Vec<_>) = ready.into_iter().partition(|(o, _)| *o == owner);
            ready = rest;
            send(tick, Cmd::Media { owner, media: mine.into_iter().map(|(_, p)| p).collect() });
        }
    }

    /// A post let go here: its end, as the mixer gives one for a post that starts no voice.
    fn let_go(&mut self, w: &Waiting) {
        self.notes.push(Note::EventEnd { post: w.post.id });
    }

    /// Sends the media the worker decoded, then the posts that waited for them; drops those still waiting [`LATE`]
    /// after they were made, whether or not their media are in.
    pub(crate) fn poll(&mut self, send: ToMixer) {
        if self.decoding.is_empty() && self.waiting.is_empty() {
            return;
        }
        let mut ready = Vec::new();
        if let Some(w) = self.worker.take() {
            for d in w.finished() {
                self.finished(d, &mut ready);
            }
            self.worker = Some(w);
        }
        // (the media go with the first post that waited for them)
        let tick = self.waiting.iter().map(|w| w.tick).min().unwrap_or(0);
        self.send_media(ready, tick, send);
        for w in std::mem::take(&mut self.waiting) {
            let ready = w.need.iter().all(|m| !self.decoding.contains(m));
            if w.at.elapsed() >= self.late {
                let id = w.need.first().map_or(w.post.event, |m| *m);
                self.notes.push(Note::Warn { what: "late load, post dropped", id: u64::from(id) });
                self.let_go(&w);
            } else if ready {
                send(w.tick, Cmd::Post(w.post));
            } else {
                self.waiting.push(w);
            }
        }
    }

    /// A stop by key: the posts waiting whose key it matches go with it (they have no voice to stop yet).
    pub(crate) fn cancel(&mut self, stops: &dyn Fn(Option<Key>) -> bool) {
        for w in std::mem::take(&mut self.waiting) {
            if stops(w.post.key) {
                self.let_go(&w);
            } else {
                self.waiting.push(w);
            }
        }
    }

    /// Posts waiting for their media.
    pub(crate) fn waiting(&self) -> usize {
        self.waiting.len()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::tape::post::{Call, Line};
    use crate::{Audio, Backend, Note, QUANTUM};

    /// An offline Audio whose lazy posts take the live path (the worker, held posts), a lazy scope of three of
    /// Shaggy's lines of one short medium each (a long one streams: not lazy), and the lines; None without a store.
    fn live() -> Option<(Audio, Vec<String>)> {
        let store = mvs_store::Store::open(mvs_store::dev_store_dir()).ok()?;
        let mut a = Audio::new(std::sync::Arc::new(store), Backend::Offline).ok()?;
        a.lazy.live = true;
        let mut names: Vec<&String> = a.init().events.keys().filter(|e| e.starts_with("vo_shaggy_")).collect();
        names.sort();
        let short = |e: &str| {
            let (l, _) = super::load(a.store(), a.init(), &super::MediaCache::new(), &[], &[e]).ok()?;
            (l.streamed.is_empty() && l.lazy.len() == 1 && l.lazy[0].1.len() == 1).then_some(())
        };
        let mut lines: Vec<String> = names.into_iter().filter(|e| short(e).is_some()).take(3).cloned().collect();
        assert_eq!(lines.len(), 3, "three of Shaggy's lines with short media");
        lines.sort();
        a.call(&Line { tick: 0, call: Call::Scope { name: "match".into(), keep: Vec::new(), lazy: lines.clone() } })
            .unwrap();
        Some((a, lines))
    }

    fn post(a: &mut Audio, tick: u32, ev: &str, key: Option<&str>) {
        post_stop(a, tick, ev, key, None);
    }

    fn post_stop(a: &mut Audio, tick: u32, ev: &str, key: Option<&str>, stop_key: Option<&str>) {
        let (key, stop_key) = (key.map(Into::into), stop_key.map(Into::into));
        a.call(&Line { tick, call: Call::Post { ev: ev.into(), owner: Some(-1), key, stop_key, at: None } }).unwrap();
    }

    /// Polls until no post waits (the decode worker's answers), at most 5 s.
    fn settle(a: &mut Audio) {
        let t = std::time::Instant::now();
        while a.waiting() > 0 && t.elapsed() < Duration::from_secs(5) {
            a.poll();
            std::thread::yield_now();
        }
        assert_eq!(a.waiting(), 0, "the decode worker did not answer in 5 s");
    }

    fn warns(notes: &[Note]) -> Vec<&'static str> {
        notes.iter().filter_map(|n| if let Note::Warn { what, .. } = n { Some(*what) } else { None }).collect()
    }

    #[test]
    fn a_held_post_goes_once_decoded() {
        let Some((mut a, lines)) = live() else {
            eprintln!("no store: nothing checked");
            return;
        };
        a.lazy.late = Duration::from_secs(60);
        post(&mut a, 1, &lines[0], None);
        assert_eq!(a.waiting(), 1, "the post waits for its media");
        settle(&mut a);
        a.pump(1, 48 * QUANTUM);
        // posted again: its media are held, so it goes at once
        post(&mut a, 2, &lines[0], None);
        assert_eq!(a.waiting(), 0);
        a.pump(2, 48 * QUANTUM);
        assert!(a.take_output().iter().any(|s| s.abs() > 1e-4), "the line made no sound");
        assert_eq!(warns(&a.take_notes()), Vec::<&str>::new());
    }

    #[test]
    fn a_scope_set_again_lets_its_lazy_media_go() {
        let Some((mut a, lines)) = live() else {
            eprintln!("no store: nothing checked");
            return;
        };
        a.lazy.live = false;
        post(&mut a, 1, &lines[0], None);
        a.pump(1, 48 * QUANTUM);
        assert_eq!(a.lazy.sent.len(), 1);
        a.call(&Line { tick: 2, call: Call::Scope { name: "match".into(), keep: Vec::new(), lazy: lines.clone() } })
            .unwrap();
        assert!(a.lazy.sent.is_empty(), "the mixer let the scope's media go");
        a.pump(2, 4 * QUANTUM);
        let _ = a.take_output();
        // the next post decodes them again and plays
        post(&mut a, 3, &lines[0], None);
        a.pump(3, 48 * QUANTUM);
        assert!(a.take_output().iter().any(|s| s.abs() > 1e-4), "the line made no sound");
        assert_eq!(warns(&a.take_notes()), Vec::<&str>::new());
    }

    #[test]
    fn a_late_post_is_dropped_and_a_stop_cancels() {
        let Some((mut a, lines)) = live() else {
            eprintln!("no store: nothing checked");
            return;
        };
        a.lazy.late = Duration::ZERO;
        post(&mut a, 1, &lines[1], None);
        settle(&mut a);
        a.pump(1, 48 * QUANTUM);
        assert!(a.take_output().iter().all(|s| *s == 0.0), "a late post played");
        let notes = a.take_notes();
        assert_eq!(warns(&notes), ["late load, post dropped"]);
        assert!(notes.iter().any(|n| matches!(n, Note::EventEnd { .. })), "no end for the dropped post");
        // a waiting post that a stop of its key meets is let go with no note
        a.lazy.late = Duration::from_secs(60);
        post(&mut a, 2, &lines[2], Some("VO:sys"));
        assert_eq!(a.waiting(), 1);
        // a play event with the key as its stop key (the scenery's posts) holds no Stop: the post stays
        // (held: a medium that never comes)
        a.lazy.decoding.insert(1);
        a.lazy.waiting[0].need.push(1);
        post_stop(&mut a, 2, &lines[2], Some("VO:sys"), Some("VO:sys"));
        assert!(a.lazy.waiting.iter().any(|w| w.need.contains(&1)), "a play post's stop key cancelled");
        a.call(&Line { tick: 2, call: Call::StopKey { key: "VO:sys".into(), fade: 0.0, curve: None } }).unwrap();
        assert_eq!(a.waiting(), 0);
        a.pump(2, 48 * QUANTUM);
        assert!(a.take_output().iter().all(|s| *s == 0.0), "a stopped post played");
    }
}
