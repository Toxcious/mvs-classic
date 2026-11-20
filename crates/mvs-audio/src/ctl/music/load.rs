//! What a music event holds on the game's side: the media its playlists can reach under some switch values, read from
//! the store as streamed entries (the mixer decodes them as they play) with the event's bank.

use std::collections::BTreeMap;
use std::sync::Arc;

use indexmap::IndexSet;
use mvs_data::sound::{ActionKind, BANK_KIND, EventBank, Id, Obj, load_bank, short_id, sound_pack};

use crate::ctl::scope::ScopeLoad;
use crate::decode::cache::MediaCache;
use crate::{AudioError, MusicFlow};

/// The media of the playlists the given switch values can reach under a music switch container (any value of a group
/// not named in the list), and of the transition segments of the rules that lead to something reached. `whole`:
/// below a branch a named value picked, every value of a group.
pub(crate) fn sources(objs: &BTreeMap<Id, Obj>, root: Id, values: &[Id], whole: bool) -> Vec<Id> {
    struct Walk<'a> {
        objs: &'a BTreeMap<Id, Obj>,
        values: &'a [Id],
        whole: bool,
        out: IndexSet<Id>,
        seen: IndexSet<Id>,
    }
    fn walk(w: &mut Walk, id: Id, below: bool) {
        let Some(o) = w.objs.get(&id) else { return };
        match o {
            Obj::MusicSwitchContainer(s) => {
                let named: Vec<_> = s.tree.iter().filter(|x| !below && w.values.contains(&x.key)).collect();
                let below = below || (w.whole && !named.is_empty());
                let pick: Vec<_> = if named.is_empty() { s.tree.iter().collect() } else { named };
                for x in pick {
                    if x.node != 0 {
                        walk(w, x.node, below);
                    }
                }
            }
            Obj::MusicTrack(t) => w.out.extend(t.clips.iter().map(|c| c.src)),
            _ => {
                for &c in o.children() {
                    walk(w, c, below);
                }
            }
        }
        w.seen.insert(id);
    }
    let mut w = Walk { objs, values, whole, out: IndexSet::new(), seen: IndexSet::new() };
    walk(&mut w, root, false);
    let reached: Vec<Id> = w.seen.iter().copied().collect();
    for id in reached {
        let rules = match objs.get(&id) {
            Some(Obj::MusicSwitchContainer(c)) => &c.rules[..],
            Some(Obj::MusicRandomSequenceContainer(c)) => &c.rules[..],
            _ => &[],
        };
        for r in rules {
            if let Some(t) = &r.trans
                && r.dst.iter().any(|&x| x == -1 || u32::try_from(x).is_ok_and(|x| w.seen.contains(&x)))
            {
                walk(&mut w, t.seg, false);
            }
        }
    }
    w.out.into_iter().collect()
}

/// The switch values a flow's holding covers: the values no state of the flow is, then every state the flow names.
pub(crate) fn flow_values(values: &[Id], flow: &MusicFlow) -> Vec<Id> {
    let mut states = IndexSet::new();
    for (k, list) in &flow.next {
        states.insert(*k);
        states.extend(list.iter().copied());
    }
    let mut out: Vec<Id> = values.iter().copied().filter(|v| !states.contains(v)).collect();
    out.extend(states);
    out
}

/// How the media an event's music reaches are picked.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reach {
    /// The values name what is reached; none named: every playlist.
    Preload,
    /// The values name what is reached, every value of a group below what they pick; none named: nothing.
    // The whole branch of the stage is held: the media are compressed entries the mixer streams, so nothing plays
    // late by a decode (a match's outro state, which no scope names).
    Scope,
}

/// A music event's bank, read from the store.
pub(crate) fn music_bank(
    store: &mvs_store::Store,
    init: &mvs_data::sound::InitBank,
    event: &str,
) -> Result<EventBank, AudioError> {
    let name = event.to_ascii_lowercase();
    let entry =
        init.events.get(&name).ok_or_else(|| AudioError::Call(format!("music event {event} is not in the build")))?;
    Ok(load_bank(&*store.kind(BANK_KIND)?, &entry.path)?)
}

/// The media an event's Play actions reach under `values` (none named: every playlist), from its bank: what a
/// release lets go.
pub(crate) fn reached(bank: &EventBank, values: &[Id]) -> Vec<Id> {
    let mut ids: IndexSet<Id> = IndexSet::new();
    for aid in &bank.actions {
        if let Some(Obj::EventAction(a)) = bank.objs.get(aid)
            && a.kind == ActionKind::Play
            && let Some(t) = a.target
        {
            ids.extend(sources(&bank.objs, t, values, false));
        }
    }
    ids.into_iter().collect()
}

/// The bank of a music event and the streamed entries of the media its Play actions reach under `values`.
pub(crate) fn load(
    store: &mvs_store::Store,
    init: &mvs_data::sound::InitBank,
    media: &MediaCache,
    event: &str,
    values: &[Id],
    reach: Reach,
) -> Result<ScopeLoad, AudioError> {
    let name = event.to_ascii_lowercase();
    let entry =
        init.events.get(&name).ok_or_else(|| AudioError::Call(format!("music event {event} is not in the build")))?;
    let bank = load_bank(&*store.kind(BANK_KIND)?, &entry.path)?;
    let mut ids: IndexSet<Id> = IndexSet::new();
    if reach == Reach::Preload || !values.is_empty() {
        for aid in &bank.actions {
            if let Some(Obj::EventAction(a)) = bank.objs.get(aid)
                && a.kind == ActionKind::Play
                && let Some(t) = a.target
            {
                ids.extend(sources(&bank.objs, t, values, reach == Reach::Scope));
            }
        }
    }
    let pack = sound_pack(store)?;
    let mut streamed = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(m) = bank.media.get(&id) else { continue };
        streamed.push(media.coded(&pack, id, &m.path).map_err(|e| AudioError::Call(format!("medium {id}: {e}")))?);
    }
    Ok(ScopeLoad { banks: vec![(name, Arc::new(bank))], streamed, ..ScopeLoad::default() })
}

/// An event's id as the commands carry it.
pub(crate) fn event_id(event: &str) -> Id {
    short_id(event)
}
