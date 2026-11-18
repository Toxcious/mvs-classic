//! The loaded banks merged (BankSet): an object is its first-loaded bank's copy, a container's children the sorted
//! union of its loaded banks' lists restricted to loaded objects; a bank of a name loaded already is refused. A
//! removed bank takes away what no other loaded bank holds. The scopes hold banks by name; a bank no scope holds
//! stays loaded, loose, the least recently used going first past [`LOOSE_BANKS`].
//!
//! The merge runs off the mixer thread: [`Banks`] (the game side's, `crate::send`) takes the scopes' holds and
//! gives a [`BankView`], which the mixer swaps in whole (`Cmd::Banks`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use indexmap::IndexSet;
use mvs_data::sound::{EventBank, Id, MediaRef, Obj, SrcType};

/// Loaded banks no scope holds any more stay loaded, the least recently used going first past this many: a voice
/// still playing reads its objects after its scope moved on.
pub const LOOSE_BANKS: usize = 256;

/// A streamed medium decodes to at least this many bytes (float, at the build's rate).
pub const STREAM_MIN: f64 = 16.0 * 1_048_576.0;

/// The merge as the mixer reads it: built by [`Banks`], never changed once made.
#[derive(Clone, Default)]
pub struct BankView {
    /// The merged objects (the first holder's copy, its children the merge's).
    objs: HashMap<Id, Arc<Obj>>,
    /// The Event objects (actions by event id): the banks' own and those they post.
    events: HashMap<Id, Arc<[Id]>>,
    /// The loaded banks' own events.
    own: HashSet<Id>,
    /// Every medium a loaded bank has named, kept when its bank goes (a voice can outlive its bank).
    media: HashMap<Id, Arc<MediaRef>>,
    /// The media of the sounds that may stream, and those a loaded sound plays decoded (a medium once seen played
    /// decoded stays so).
    stream_ok: HashSet<Id>,
    decoded: HashSet<Id>,
}

impl std::fmt::Debug for BankView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BankView({} objects, {} events, {} own)", self.objs.len(), self.events.len(), self.own.len())
    }
}

impl BankView {
    /// A hierarchy object as the merge has it.
    pub fn obj(&self, id: Id) -> Option<&Obj> {
        self.objs.get(&id).map(|o| &**o)
    }

    /// The merge of no banks, one for all: the mixer's first, never freed there.
    pub fn empty() -> Arc<BankView> {
        static EMPTY: std::sync::LazyLock<Arc<BankView>> = std::sync::LazyLock::new(Arc::default);
        EMPTY.clone()
    }

    /// An event's actions when its own bank is loaded (a post's lookup).
    pub fn event(&self, id: Id) -> Option<&[Id]> {
        if !self.own.contains(&id) {
            return None;
        }
        Some(self.events.get(&id).map_or(&[][..], |a| &a[..]))
    }

    /// An Event object's actions as the loaded banks have it (a PlayEvent action's target).
    pub fn ev_obj(&self, id: Id) -> Option<&[Id]> {
        self.events.get(&id).map(|a| &a[..])
    }

    /// A medium a loaded bank has named.
    pub fn media(&self, id: Id) -> Option<&MediaRef> {
        self.media.get(&id).map(|m| &**m)
    }

    /// Whether a medium plays streamed (it may, and no loaded sound plays it decoded).
    pub fn streamed(&self, media: Id) -> bool {
        self.stream_ok.contains(&media) && !self.decoded.contains(&media)
    }
}

/// A loaded bank.
struct Unit {
    name: String,
    bank: Arc<EventBank>,
}

/// The merge of the loaded event banks and the scopes' holds on them.
#[derive(Default)]
pub struct Banks {
    /// The merge.
    view: BankView,
    /// The loaded banks by unit number (load order), and their numbers by index name.
    units: BTreeMap<u32, Unit>,
    by_name: HashMap<String, u32>,
    next_unit: u32,
    /// The units holding each object and Event object (the first one's copy is the merge's).
    holders: HashMap<Id, BTreeSet<u32>>,
    ev_holders: HashMap<Id, BTreeSet<u32>>,
    /// Per container, the children its holders' copies list, with how many list each.
    listed: HashMap<Id, BTreeMap<Id, u32>>,
    /// The loaded banks' own events: event id -> index name.
    own: HashMap<Id, String>,
    /// The scopes' banks by scope name hash, and the loose banks, least recently used first.
    held: HashMap<u64, HashSet<String>>,
    loose: IndexSet<String>,
}

fn children_mut(o: &mut Obj) -> Option<&mut Vec<Id>> {
    match o {
        Obj::RandomSequenceContainer(c) => Some(&mut c.children),
        Obj::SwitchContainer(c) => Some(&mut c.children),
        Obj::LayerContainer(c) | Obj::ActorMixer(c) => Some(&mut c.children),
        Obj::MusicSwitchContainer(c) => Some(&mut c.children),
        Obj::MusicRandomSequenceContainer(c) => Some(&mut c.children),
        Obj::MusicSegment(c) => Some(&mut c.children),
        _ => None,
    }
}

/// Adds a unit to an id's holders; true when it is the first.
fn hold(map: &mut HashMap<Id, BTreeSet<u32>>, id: Id, unit: u32) -> bool {
    let set = map.entry(id).or_default();
    set.insert(unit);
    set.len() == 1
}

/// Takes a unit from an id's holders: Some(Some(u)) the new first holder when the first went, Some(None) when none
/// is left, None when the first stays.
fn release(map: &mut HashMap<Id, BTreeSet<u32>>, id: Id, unit: u32) -> Option<Option<u32>> {
    let set = map.get_mut(&id)?;
    let first = set.first().copied();
    if !set.remove(&unit) {
        return None;
    }
    match set.first().copied() {
        None => {
            map.remove(&id);
            Some(None)
        }
        Some(n) => (first == Some(unit)).then_some(Some(n)),
    }
}

fn unit_events(b: &EventBank) -> impl Iterator<Item = (Id, &Vec<Id>)> {
    std::iter::once((b.id, &b.actions)).chain(b.posted.iter().map(|(k, v)| (*k, v)))
}

impl Banks {
    /// The merge for the mixer.
    pub fn view(&self) -> BankView {
        self.view.clone()
    }

    /// The merge as it is: the scope loads' walks.
    pub fn merged(&self) -> &BankView {
        &self.view
    }

    /// A hierarchy object as the merge has it.
    pub fn obj(&self, id: Id) -> Option<&Obj> {
        self.view.obj(id)
    }

    /// An Event object's actions as the loaded banks have it.
    pub fn ev_obj(&self, id: Id) -> Option<&[Id]> {
        self.view.ev_obj(id)
    }

    /// A medium a loaded bank has named.
    pub fn media(&self, id: Id) -> Option<&MediaRef> {
        self.view.media(id)
    }

    /// Whether a medium plays streamed.
    pub fn streamed(&self, media: Id) -> bool {
        self.view.streamed(media)
    }

    /// Loads banks under their events' index names, in order (one of a name loaded already is refused), with no
    /// scope's hold.
    pub fn add(&mut self, banks: impl IntoIterator<Item = (String, Arc<EventBank>)>) {
        let mut touched = BTreeSet::new();
        for (n, b) in banks {
            self.add_unit(&n, b, &mut touched);
        }
        self.children(&touched);
    }

    /// Loads a bank, the containers whose children move added to `touched`.
    fn add_unit(&mut self, name: &str, bank: Arc<EventBank>, touched: &mut BTreeSet<Id>) {
        if self.by_name.contains_key(name) {
            return;
        }
        let u = self.next_unit;
        self.next_unit += 1;
        let v = &mut self.view;
        let mut seen = HashSet::new();
        for (ev, acts) in unit_events(&bank) {
            if !seen.insert(ev) {
                continue;
            }
            if hold(&mut self.ev_holders, ev, u) {
                v.events.insert(ev, acts.as_slice().into());
            }
        }
        self.own.insert(bank.id, name.to_owned());
        v.own.insert(bank.id);
        for (mid, m) in &bank.media {
            v.media.entry(*mid).or_insert_with(|| Arc::new(m.clone()));
        }
        for (id, o) in &bank.objs {
            if hold(&mut self.holders, *id, u) {
                v.objs.insert(*id, Arc::new(o.clone()));
            }
            let ch = o.children();
            if !ch.is_empty() {
                let l = self.listed.entry(*id).or_default();
                for c in ch {
                    *l.entry(*c).or_default() += 1;
                }
            }
            touched.insert(*id);
            if let Some(p) = o.node().and_then(|n| n.parent) {
                touched.insert(p);
            }
        }
        // the media to stream: those of the sounds that qualify, less those a loaded sound plays decoded
        for &sid in bank.objs.keys() {
            let Some(Obj::SoundSfxVoice(s)) = self.view.obj(sid) else { continue };
            if s.plugin.as_deref() == Some("AkFxSrcSilenceSource") {
                continue;
            }
            let src = s.src;
            if self.streamed_sound(sid) {
                self.view.stream_ok.insert(src);
            } else {
                self.view.decoded.insert(src);
            }
        }
        self.by_name.insert(name.to_owned(), u);
        self.units.insert(u, Unit { name: name.to_owned(), bank });
    }

    /// Whether the build's data let a sound play streamed: a Streaming or PrefetchStreaming source that loops forever, decoding to
    /// [`STREAM_MIN`] bytes or more, with no pitch (prop, range or curve) on its chain.
    fn streamed_sound(&self, id: Id) -> bool {
        let Some(Obj::SoundSfxVoice(s)) = self.view.obj(id) else { return false };
        let Some(m) = self.view.media(s.src) else { return false };
        if !matches!(s.src_type, SrcType::Streaming | SrcType::PrefetchStreaming) || s.node.p.get(58) != Some(0.0) {
            return false;
        }
        if f64::from(m.frames) / f64::from(m.rate) * f64::from(crate::RATE) * f64::from(m.channels) * 4.0 < STREAM_MIN {
            return false;
        }
        let mut cur = Some(id);
        let mut depth = 0;
        while let (Some(c), true) = (cur, depth < 32) {
            let Some(n) = self.view.obj(c).and_then(Obj::node) else { break };
            if n.p.get(2).is_some_and(|v| v != 0.0) || n.r.get(2).is_some() || n.rtpc.iter().any(|r| r.param == 2) {
                return false;
            }
            cur = n.parent;
            depth += 1;
        }
        true
    }

    /// Unloads the bank of an index name, the containers whose children move added to `touched`.
    fn remove_unit(&mut self, name: &str, touched: &mut BTreeSet<Id>) {
        let Some(u) = self.by_name.remove(name) else { return };
        let Unit { bank, .. } = self.units.remove(&u).expect("a named unit is loaded");
        self.loose.shift_remove(name);
        if self.own.get(&bank.id).is_some_and(|n| n == name) {
            self.own.remove(&bank.id);
            self.view.own.remove(&bank.id);
        }
        let mut seen = HashSet::new();
        for (ev, _) in unit_events(&bank) {
            if !seen.insert(ev) {
                continue;
            }
            match release(&mut self.ev_holders, ev, u) {
                Some(Some(n)) => {
                    let b = &self.units[&n].bank;
                    let acts = unit_events(b).find(|(e, _)| *e == ev).map_or_else(Vec::new, |(_, a)| a.clone());
                    self.view.events.insert(ev, acts.into());
                }
                Some(None) => {
                    self.view.events.remove(&ev);
                }
                None => {}
            }
        }
        for (id, o) in &bank.objs {
            match release(&mut self.holders, *id, u) {
                Some(Some(n)) => {
                    let c = self.units[&n].bank.objs[id].clone();
                    self.view.objs.insert(*id, Arc::new(c));
                }
                Some(None) => {
                    self.view.objs.remove(id);
                }
                None => {}
            }
            let ch = o.children();
            if !ch.is_empty()
                && let Some(l) = self.listed.get_mut(id)
            {
                for c in ch {
                    if let Some(k) = l.get_mut(c) {
                        *k -= 1;
                        if *k == 0 {
                            l.remove(c);
                        }
                    }
                }
                if l.is_empty() {
                    self.listed.remove(id);
                }
            }
            touched.insert(*id);
            if let Some(p) = o.node().and_then(|n| n.parent) {
                touched.insert(p);
            }
        }
    }

    /// The children of the touched containers again: the sorted union over their holders, loaded children only.
    fn children(&mut self, ids: &BTreeSet<Id>) {
        let v = &mut self.view;
        for id in ids {
            if !self.holders.contains_key(id) {
                continue;
            }
            let Some(o) = v.objs.get(id) else { continue };
            if matches!(**o, Obj::SoundSfxVoice(_)) || o.node().is_none() {
                continue;
            }
            let set: Vec<Id> = match self.listed.get(id) {
                Some(l) => l.keys().copied().filter(|c| v.objs.contains_key(c)).collect(),
                None => Vec::new(),
            };
            // (a copy a view shares is cloned only when its list moves)
            if o.children() != set.as_slice()
                && let Some(o) = v.objs.get_mut(id)
                && let Some(ch) = children_mut(Arc::make_mut(o))
            {
                *ch = set;
            }
        }
    }

    /// A scope holds these banks (its earlier ones let go): loads those not loaded, in order, then settles.
    pub fn hold(&mut self, scope: u64, banks: Vec<(String, Arc<EventBank>)>) {
        let mut names = HashSet::new();
        let mut touched = BTreeSet::new();
        for (n, b) in banks {
            names.insert(n.clone());
            self.add_unit(&n, b, &mut touched);
        }
        self.held.insert(scope, names);
        self.settle(&mut touched);
        self.children(&touched);
    }

    /// A scope lets its banks go; false when it held none (the merge is as it was).
    pub fn unhold(&mut self, scope: u64) -> bool {
        if self.held.remove(&scope).is_none() {
            return false;
        }
        let mut touched = BTreeSet::new();
        self.settle(&mut touched);
        self.children(&touched);
        true
    }

    /// A post used the own bank of an event: a loose one moves to the most recently used.
    pub fn touch(&mut self, event: Id) {
        if let Some(name) = self.own.get(&event)
            && self.loose.shift_remove(name)
        {
            self.loose.insert(name.clone());
        }
    }

    /// The loaded banks no scope holds go loose, the least recently used beyond [`LOOSE_BANKS`] out of the merge.
    fn settle(&mut self, touched: &mut BTreeSet<Id>) {
        let held: HashSet<&String> = self.held.values().flatten().collect();
        for Unit { name, .. } in self.units.values() {
            if held.contains(name) {
                self.loose.shift_remove(name);
            } else if !self.loose.contains(name) {
                self.loose.insert(name.clone());
            }
        }
        if self.loose.len() > LOOSE_BANKS {
            let k = self.loose.len() - LOOSE_BANKS;
            let gone: Vec<String> = self.loose.drain(..k).collect();
            for n in gone {
                self.remove_unit(&n, touched);
            }
        }
    }
}
