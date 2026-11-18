//! (the port's own) What keeps this copy offline before it meets anyone: a check under way, a newer release on the
//! feed, an import older than this version reads, or a self check that differs from the release's. While a gate is
//! set the hub holds the command line's commands, the online backends stay off (`glue::apply_gate`) and the menus'
//! line says why.
//!
//! A player's run (`glue::start` with `dev` false) arms the checks at the game's start: the update feed asked on a
//! thread of its own (`FeedCheck`), the store's kinds read once the store is open (`import_current`). The menus'
//! frame folds them and the self check's state (`crate::self_check`) into the hub's gate (`poll`, its one writer).

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use mvs_store::Manifest;

use super::hub::OnlineHub;

/// Why online play waits or stays off (`OnlineHub::set_gate`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gate {
    /// The update feed or the self check has not answered yet.
    Checking,
    /// The feed offers a newer release (`version` its).
    Update { version: String },
    /// The store's kinds are older than this version reads.
    Import,
    /// The fixed check match's hash on this PC (`mine`) is not the release's.
    SelfCheck { mine: u32, release: u32 },
}

/// The longest the feed's answer holds online play: past it a missing answer counts as no newer release (a PC with
/// no network, a feed that does not answer). WinHTTP's own failure (no network at all) ends the wait at once.
// Unverified: the port's own number, 4 s: long enough for a feed on another continent, short enough not to hold a
// start that no update holds.
pub const FEED_WAIT: Duration = Duration::from_secs(4);

/// Every stored kind of this version's importer (mvs_import::kinds::KINDS) is in the store's manifest at the version
/// this importer writes: else the store is an older import's, and the game's tables may not be the release's.
pub fn import_current(m: &Manifest) -> bool {
    mvs_import::kinds::KINDS
        .iter()
        .filter(|k| k.stored())
        .all(|k| m.kinds.get(k.id()).is_some_and(|r| r.version == k.version()))
}

/// The update feed's answer so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Feed {
    /// Asked, no answer yet (and not past `FEED_WAIT`).
    Asking,
    /// A newer release (its version).
    Newer(String),
    /// No newer release: none on the feed, no feed in this build, a failure or no answer in time.
    Done,
}

/// The update feed asked on a thread of its own (update::latest).
#[derive(Debug)]
pub struct FeedCheck {
    rx: Option<Receiver<Option<String>>>,
    until: Instant,
    answer: Option<Feed>,
}

impl FeedCheck {
    /// The feed asked for a release newer than `version`; no feed: answered at once (no gate). Past `wait` the
    /// answer is `Done` whatever comes later.
    pub fn start(feed: Option<&str>, version: &str, wait: Duration) -> FeedCheck {
        let until = Instant::now() + wait;
        let Some(feed) = feed else { return FeedCheck { rx: None, until, answer: Some(Feed::Done) } };
        let (tx, rx) = mpsc::channel();
        let (feed, version) = (feed.to_owned(), version.to_owned());
        let started = std::thread::Builder::new().name("mvs-feed".into()).spawn(move || {
            let newer = match crate::update::latest(&feed, &version) {
                Ok(r) => r.map(|r| r.version),
                Err(e) => {
                    eprintln!("mvs: online: the update feed: {e}");
                    None
                }
            };
            let _ = tx.send(newer);
        });
        match started {
            Ok(_) => FeedCheck { rx: Some(rx), until, answer: None },
            Err(e) => {
                eprintln!("mvs: online: the update feed's thread: {e}");
                FeedCheck { rx: None, until, answer: Some(Feed::Done) }
            }
        }
    }

    /// The answer as it stands (polled each frame).
    pub fn answer(&mut self) -> Feed {
        if let Some(a) = &self.answer {
            return a.clone();
        }
        let got = match self.rx.as_ref().map(Receiver::try_recv) {
            Some(Ok(Some(v))) => Some(Feed::Newer(v)),
            Some(Ok(None) | Err(TryRecvError::Disconnected)) | None => Some(Feed::Done),
            Some(Err(TryRecvError::Empty)) if Instant::now() >= self.until => {
                // Unverified: a late answer is dropped (the launcher offers the update all the same): online play
                // that came on is not taken off again.
                eprintln!(
                    "mvs: online: the update feed did not answer in {} s: online play goes on",
                    FEED_WAIT.as_secs()
                );
                Some(Feed::Done)
            }
            Some(Err(TryRecvError::Empty)) => None,
        };
        match got {
            Some(a) => {
                self.rx = None;
                self.answer = Some(a.clone());
                a
            }
            None => Feed::Asking,
        }
    }
}

/// The self check as the gate reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfChecked {
    /// Under way.
    Running,
    /// Over and differs from the release's (this PC's hash, the release's).
    Differs { mine: u32, release: u32 },
    /// Over and the same, or no check (no reference to compare with).
    Done,
}

/// The gate from its inputs, fresh each call (the one rule; `poll` its one writer): a newer release, then a stale
/// import (None: the store not open yet), then a self check that differs, then Checking while the feed or the self
/// check has not answered or the store is not open; else none.
pub fn gate_of(feed: &Feed, import: Option<bool>, self_check: SelfChecked) -> Option<Gate> {
    if let Feed::Newer(v) = feed {
        return Some(Gate::Update { version: v.clone() });
    }
    if import == Some(false) {
        return Some(Gate::Import);
    }
    if let SelfChecked::Differs { mine, release } = self_check {
        return Some(Gate::SelfCheck { mine, release });
    }
    if *feed == Feed::Asking || import.is_none() || self_check == SelfChecked::Running {
        return Some(Gate::Checking);
    }
    None
}

/// The self check's state from the hub (crate::self_check keeps it there).
fn self_checked(hub: &OnlineHub) -> SelfChecked {
    if crate::self_check::running(hub) {
        return SelfChecked::Running;
    }
    match crate::self_check::differs(hub) {
        Some((mine, release)) => SelfChecked::Differs { mine, release },
        None => SelfChecked::Done,
    }
}

/// A player's run's checks (`arm`): the feed's and the store's.
#[derive(Debug)]
struct Checks {
    feed: FeedCheck,
    /// The store's import read (once, at the first frame with the store open).
    import: Option<bool>,
}

/// The run's checks (one game a process).
static CHECKS: Mutex<Option<Checks>> = Mutex::new(None);

/// Arms the gate's checks for a player's run (glue::start): the feed asked now; the hub's gate Checking until `poll`
/// has both answers.
pub fn arm(hub: &mut OnlineHub, feed: Option<&str>, version: &str) {
    let feed = FeedCheck::start(feed, version, FEED_WAIT);
    *CHECKS.lock().unwrap_or_else(PoisonError::into_inner) = Some(Checks { feed, import: None });
    hub.set_gate(Some(Gate::Checking));
}

/// Each frame of the menus: the feed's answer and the store's import (`manifest`, once the store is open) folded
/// into the hub's gate. A run that did not arm the checks (a dev config's) is left as it is.
pub fn poll(hub: &mut OnlineHub, manifest: Option<&Manifest>) {
    let mut checks = CHECKS.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(c) = checks.as_mut() else { return };
    if c.import.is_none()
        && let Some(m) = manifest
    {
        let current = import_current(m);
        if !current {
            eprintln!("mvs: online: the store is an older import's: online play is off until it is imported again");
        }
        c.import = Some(current);
    }
    let next = gate_of(&c.feed.answer(), c.import, self_checked(hub));
    if next.as_ref() != hub.gate() {
        hub.set_gate(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(kinds: &[(&str, u64)]) -> Manifest {
        let kinds: serde_json::Map<String, serde_json::Value> = kinds
            .iter()
            .map(|(id, v)| {
                let r = serde_json::json!({ "version": v, "file": "x.pack", "entries": 0, "bytes": 0, "failed": 0 });
                ((*id).to_owned(), r)
            })
            .collect();
        serde_json::from_value(serde_json::json!({ "format": 1, "buildId": "b", "kinds": kinds })).unwrap()
    }

    fn stored() -> Vec<(&'static str, u64)> {
        mvs_import::kinds::KINDS.iter().filter(|k| k.stored()).map(|k| (k.id(), k.version())).collect()
    }

    #[test]
    fn a_store_of_this_importer_is_current_and_an_older_kind_is_not() {
        let now = stored();
        assert!(import_current(&manifest(&now)));
        let mut old = now.clone();
        old[0].1 = old[0].1.wrapping_sub(1);
        assert!(!import_current(&manifest(&old)), "a kind of an older version");
        assert!(!import_current(&manifest(&now[1..])), "a kind missing");
        assert!(!import_current(&manifest(&[])));
    }

    #[test]
    fn the_gate_takes_its_reasons_in_order() {
        use SelfChecked::{Differs, Done, Running};
        let v = || Feed::Newer("1.2.0".to_owned());
        let up = Some(Gate::Update { version: "1.2.0".into() });
        let differs = Differs { mine: 1, release: 2 };
        assert_eq!(gate_of(&v(), Some(false), differs), up);
        assert_eq!(gate_of(&v(), None, Running), up);
        assert_eq!(gate_of(&Feed::Asking, Some(false), differs), Some(Gate::Import));
        assert_eq!(gate_of(&Feed::Asking, Some(true), differs), Some(Gate::SelfCheck { mine: 1, release: 2 }));
        assert_eq!(gate_of(&Feed::Asking, Some(true), Done), Some(Gate::Checking));
        assert_eq!(gate_of(&Feed::Done, Some(true), Running), Some(Gate::Checking));
        assert_eq!(gate_of(&Feed::Done, None, Done), Some(Gate::Checking));
        assert_eq!(gate_of(&Feed::Done, Some(true), Done), None);
    }

    #[test]
    fn no_feed_answers_at_once() {
        let mut f = FeedCheck::start(None, "1.0.0", FEED_WAIT);
        assert_eq!(f.answer(), Feed::Done);
    }
}
