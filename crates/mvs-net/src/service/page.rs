//! A page's port to the net service (a page is one game client of the service), and the timer queue the service and its
//! sessions keep (two due at once in the order they were set; a repeating one's
//! next run is its last one's time plus its period).

use std::sync::atomic::{AtomicU64, Ordering};

use indexmap::IndexMap;

use crate::service::api::NetEvent;
use crate::service::party::Party;
use crate::service::session::Session;

/// A page of the service, by its number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageId(pub u32);

/// What a question to the player (NetEvent Ask) goes on to when it is answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskFor {
    /// A relay code's join waits for it (GuestSession).
    Relay,
}

/// A page: its session and party (each taken out for a call that needs the service), its events not yet drained,
/// and its questions out to the player, by id.
pub struct Page {
    pub id: PageId,
    pub session: Option<Session>,
    pub party: Option<Party>,
    pub events: Vec<NetEvent>,
    pub asks: IndexMap<u32, AskFor>,
}

impl Page {
    pub fn new(id: PageId) -> Page {
        Page { id, session: None, party: None, events: Vec::new(), asks: IndexMap::new() }
    }
}

/// A timer's handle on its queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimerId(pub u64);

#[derive(Clone, Debug)]
struct Entry<T> {
    id: TimerId,
    at: f64,
    n: u64,
    every: f64,
    what: T,
}

/// The order timers are set in, one count for every owner's queue (one clock for all), so that two owners' timers
/// due at once run in the order they were set.
static SET_ORDER: AtomicU64 = AtomicU64::new(0);

fn set_order() -> u64 {
    SET_ORDER.fetch_add(1, Ordering::Relaxed)
}

/// One owner's timers, `T` what each does.
#[derive(Clone, Debug)]
pub struct Timers<T> {
    due: Vec<Entry<T>>,
    ids: u64,
}

impl<T> Default for Timers<T> {
    fn default() -> Self {
        Timers { due: Vec::new(), ids: 0 }
    }
}

impl<T: Clone> Timers<T> {
    /// Once, `ms` after `now`.
    pub fn after(&mut self, now: f64, ms: f64, what: T) -> TimerId {
        self.add(now, ms, 0.0, what)
    }

    /// Every `ms` (at least 1) from `now` on, until cancelled.
    pub fn every(&mut self, now: f64, ms: f64, what: T) -> TimerId {
        self.add(now, ms, ms.max(1.0), what)
    }

    fn add(&mut self, now: f64, ms: f64, every: f64, what: T) -> TimerId {
        self.ids += 1;
        let id = TimerId(self.ids);
        let n = set_order();
        self.due.push(Entry { id, at: now + ms.max(0.0), n, every, what });
        id
    }

    pub fn cancel(&mut self, id: TimerId) {
        self.due.retain(|e| e.id != id);
    }

    /// Every timer whose payload `f` says so stops.
    pub fn cancel_where(&mut self, f: impl Fn(&T) -> bool) {
        self.due.retain(|e| !f(&e.what));
    }

    pub fn clear(&mut self) {
        self.due.clear();
    }

    pub fn is_set(&self, id: TimerId) -> bool {
        self.due.iter().any(|e| e.id == id)
    }

    pub fn next_due(&self) -> Option<f64> {
        self.next_key().map(|k| k.0)
    }

    /// The next timer's time and set order: the owners' firsts compare by it.
    pub fn next_key(&self) -> Option<(f64, u64)> {
        self.due.iter().map(|e| (e.at, e.n)).min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
    }

    /// The first timer due by `now` (its time, its handle, its payload); a repeating one is set again.
    pub fn take_due(&mut self, now: f64) -> Option<(f64, TimerId, T)> {
        let i = self
            .due
            .iter()
            .enumerate()
            .filter(|(_, e)| e.at <= now)
            .min_by(|a, b| a.1.at.total_cmp(&b.1.at).then(a.1.n.cmp(&b.1.n)))
            .map(|(i, _)| i)?;
        let e = &mut self.due[i];
        let at = e.at;
        let out = (at, e.id, e.what.clone());
        if e.every > 0.0 {
            e.at += e.every;
            e.n = set_order();
        } else {
            self.due.remove(i);
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timers_run_in_order() {
        let mut t = Timers::default();
        let a = t.every(0.0, 10.0, 'a');
        t.after(0.0, 10.0, 'b');
        t.after(0.0, 5.0, 'c');
        let mut seen = Vec::new();
        while let Some((at, _, w)) = t.take_due(25.0) {
            seen.push((at, w));
        }
        assert_eq!(seen, vec![(5.0, 'c'), (10.0, 'a'), (10.0, 'b'), (20.0, 'a')]);
        t.cancel(a);
        assert_eq!(t.next_due(), None);
    }
}
