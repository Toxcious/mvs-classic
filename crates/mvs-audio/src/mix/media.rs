//! The media the mixer can play: the decoded media and the streamed media's entries of the loaded scopes (a scope's
//! command, a music preload or scope), by medium id. The table holds its own references, taken before the command
//! goes on to the interpreter; what it lets go goes back to the game thread to be freed there.

use std::sync::Arc;

use mvs_data::sound::Id;

use crate::ctl::scope::ScopeLoad;
use crate::decode::Pcm;
use crate::{Garbage, Note};

/// The most media the table holds (its capacity, reserved up front).
pub const MAX_MEDIA: usize = 32_768;

/// What the table has of a medium.
#[derive(Clone)]
pub(crate) enum Held {
    Pcm(Arc<Pcm>),
    /// A streamed medium: its entry and setup header.
    Stream {
        entry: Arc<[u8]>,
        setup: Arc<[u8]>,
    },
}

struct Entry {
    id: Id,
    /// The scope that holds it (a scope's name hash; a music event's id with the top bit set).
    owner: u64,
    held: Held,
}

pub(crate) struct Media {
    /// Sorted by id.
    list: Vec<Entry>,
}

/// A music event's media are held under its id with the top bit set (scope names are hashes of their own).
pub(crate) fn music_owner(event: Id) -> u64 {
    (1 << 63) | u64::from(event)
}

impl Media {
    pub(crate) fn new() -> Media {
        Media { list: Vec::with_capacity(MAX_MEDIA) }
    }

    /// Appends (`add` sorts the list again).
    fn push(&mut self, id: Id, owner: u64, held: Held, notes: &mut rtrb::Producer<Note>) {
        if self.list.len() == self.list.capacity() {
            let _ = notes.push(Note::Warn { what: "media table full", id: u64::from(id) });
            return;
        }
        self.list.push(Entry { id, owner, held });
    }

    /// Whether `owner` holds medium `id` among the first `sorted` entries (sorted by id).
    fn owns(&self, sorted: usize, owner: u64, id: Id) -> bool {
        let l = &self.list[..sorted];
        l[l.partition_point(|e| e.id < id)..].iter().take_while(|e| e.id == id).any(|e| e.owner == owner)
    }

    /// Holds a load's media under `owner` (a medium the owner holds already stays held once: the holds are sets).
    pub(crate) fn add(&mut self, owner: u64, load: &ScopeLoad, notes: &mut rtrb::Producer<Note>) {
        let sorted = self.list.len();
        for p in &load.media {
            if !self.owns(sorted, owner, p.id) {
                self.push(p.id, owner, Held::Pcm(p.clone()), notes);
            }
        }
        for s in &load.streamed {
            if !self.owns(sorted, owner, s.id) {
                self.push(s.id, owner, Held::Stream { entry: s.entry.clone(), setup: s.setup.clone() }, notes);
            }
        }
        // one sort in place (no allocation; which of a medium's equal entries comes first does not matter)
        self.list.sort_unstable_by_key(|e| e.id);
    }

    /// Holds decoded media under `owner` (a scope's lazy media, decoded when first posted).
    pub(crate) fn add_pcms(&mut self, owner: u64, media: &[Arc<Pcm>], notes: &mut rtrb::Producer<Note>) {
        for p in media {
            self.push(p.id, owner, Held::Pcm(p.clone()), notes);
        }
        self.list.sort_unstable_by_key(|e| e.id);
    }

    /// Lets the media held under `owner` go.
    pub(crate) fn remove(&mut self, owner: u64, notes: &mut rtrb::Producer<Note>) {
        // one pass; the note's reference is the one left once the entry drops
        self.list.retain(|e| {
            if e.owner != owner {
                return true;
            }
            free(e.held.clone(), notes);
            false
        });
    }

    /// Lets a medium held under `owner` go.
    pub(crate) fn remove_one(&mut self, owner: u64, id: Id, notes: &mut rtrb::Producer<Note>) {
        let at = self.list.partition_point(|e| e.id < id);
        let mut i = at;
        while i < self.list.len() && self.list[i].id == id {
            if self.list[i].owner == owner {
                free(self.list.remove(i).held, notes);
            } else {
                i += 1;
            }
        }
    }

    /// How many entries hold medium `id`.
    #[cfg(test)]
    pub(crate) fn count(&self, id: Id) -> usize {
        self.list.iter().filter(|e| e.id == id).count()
    }

    pub(crate) fn get(&self, id: Id) -> Option<&Held> {
        let at = self.list.partition_point(|e| e.id < id);
        self.list.get(at).filter(|e| e.id == id).map(|e| &e.held)
    }

    /// Whether the table still holds this decoded medium (so a voice's reference is not the last).
    pub(crate) fn holds(&self, p: &Arc<Pcm>) -> bool {
        let at = self.list.partition_point(|e| e.id < p.id);
        self.list[at..]
            .iter()
            .take_while(|e| e.id == p.id)
            .any(|e| matches!(&e.held, Held::Pcm(q) if Arc::ptr_eq(p, q)))
    }
}

/// Sends a reference back to the game thread (a full note ring drops it here).
pub(crate) fn free(held: Held, notes: &mut rtrb::Producer<Note>) {
    match held {
        Held::Pcm(p) => {
            let _ = notes.push(Note::Free(Garbage::Media(p)));
        }
        Held::Stream { entry, setup } => {
            let _ = notes.push(Note::Free(Garbage::Bytes(entry)));
            let _ = notes.push(Note::Free(Garbage::Bytes(setup)));
        }
    }
}
