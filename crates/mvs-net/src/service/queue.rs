//! The online queue (SessionTarget Queue; the plan of it: pairing by the games
//! themselves through lobby search). The session starts on a host core of kind Queue in this process (home), so the
//! page's hello is answered at once. The page's texts are read only as far as the session needs: its last hello and
//! its last queue.join, kept to be replayed to the next core it moves to (the join with `waited`, the time it has
//! searched; the next core's `server`, the welcome of the hello and the queue.state of the join are not passed to the
//! page again). The core's queue.state { on: true } starts the search (its start the lobbies' `at`), and from then on
//! the session is in one place at a time:
//!   home  its own core, and one listed lobby per mode it queues for (queue_bucket, queue_size places), every member
//!         linked to that one core (Hosting);
//!   away  a member of one found lobby, linked to its owner's core (Guesting); its replayed join names that lobby's
//!         mode alone.
//! The moves:
//!   1. At the join, and every SEARCH_EVERY_MS while its own lobbies have no other member, it searches each mode's
//!      bucket. A lobby older than its own of that mode (`before`) with a free place: it joins it (a full or gone
//!      one: the next), then lets its own core and lobbies go (a member that came meanwhile is told and searches
//!      again).
//!   2. The owner's side that ends the link before a found match (its bye, the lobby closed, the owner gone, a
//!      replayed hello or join it refused) sends it home, searching again with `waited`; a lobby whose core refused
//!      the replay is not joined again in this session. A queue.cancel while away goes home, with no lobby, and the
//!      home core answers it.
//!   3. A found match at home (the core's Found) takes the lobbies with a human of the match out of the searches,
//!      destroys the others, closes the link of every member who is not in the match (told why; it searches again)
//!      and kicks a member who joins after it.
//!   4. From a found match on, the session runs the pre-match, the set and its votes on that core as a custom one
//!      does; the end of the link to it ends the session. A queue.join after a found match is a new search, from a
//!      new home.
//!
//! The session ends when the page closes it (the queue left, the set over).
//!
//! Since wire 5 a relay runs one queue core for every queuer (`relay_host`): this session is the queue on Epic alone,
//! where no relay is reached.
//!
//! Sans-IO: the backend's answers come back by `answered`, the timers (its own, its place's, and the places it let go
//! that still have their byes' BYE_MS) run by `pump`; what the page's party is to hear (`PartyNote`) is drained by
//! `take_notes`. Where a step waits for an answer, the session keeps the step it was at in its ask and goes on at the
//! answer.

use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use mvs_host::SessionKind;
use mvs_protocol::messages::QueueWhy;
use mvs_protocol::online::{PARTY_MAX, QueueMode, compat_of};
use mvs_protocol::queue_rules::{QUEUE_MODES, bot_after_s, queue_size};
use mvs_protocol::relay_wire::Listed;

use crate::backend::{Answer, Ask, BackendKind, FoundLobby, LobbyChange};
use crate::service::api::ClosedWhy;
use crate::service::hosting::{GuestNote, Guesting, Hosting, HostingEvent};
use crate::service::page::{PageId, TimerId, Timers};
use crate::service::party::{PartyFollow, PartyNote, SessionNote};
use crate::service::session::{SessionBase, Via};
use crate::service::{NetService, SEARCH_EVERY_MS, SEARCH_LIMIT, lock, queue_bucket};

/// An away queue session (a member of another queuer's lobby) goes home, searching on without that lobby and its
/// owner, when the owner's core has not answered its replayed join with queue.state on within QUEUE_STATE_WAIT_MS of
/// the owner's first message, or has sent no queue.found QUEUE_FOUND_GRACE_MS after this player's own bot wait
/// (queue.join botAfter) is over (and that long after the link at least): a core that holds its members without a
/// match. Its next core fills bots at once (the join's `waited` is past the wait). Unverified: the port's own
/// margins.
const QUEUE_STATE_WAIT_MS: f64 = 10000.0;
const QUEUE_FOUND_GRACE_MS: f64 = 15000.0;

/// The host core's queue.state whys that take a party's ticket out: the party's search ends there (the other side's
/// too, told over the party), it does not search on.
const PARTY_WHYS: [&str; 2] = ["party", "partyLate"];

/// A JSON value that keeps its objects' key order (the page's queue.join is replayed with its keys in that order).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum Json {
    Null,
    Bool(bool),
    Num(serde_json::Number),
    Str(String),
    Arr(Vec<Json>),
    Obj(IndexMap<String, Json>),
}

/// A found lobby comes before another (older): by its `at`, then its id, the same order for every searcher.
fn before(a: (f64, &str), b: (f64, &str)) -> bool {
    a.0 < b.0 || (a.0 == b.0 && a.1 < b.1)
}

/// The first messages of a place's core the page has had from an earlier place already: its `server`, the welcome
/// of the replayed hello, the queue.state of the replayed join.
#[derive(Clone, Copy, Debug, Default)]
struct Quiet {
    server: bool,
    welcome: bool,
    state: bool,
}

struct Home {
    h: Hosting,
    /// Its listed lobby of each mode, and the modes whose lobby is being made.
    own: IndexMap<QueueMode, String>,
    making: IndexSet<QueueMode>,
}

struct Away {
    g: Guesting,
    /// The modes its replayed join names: a found lobby's mode alone, or a party leader's follow's.
    modes: Vec<QueueMode>,
    /// The owner's core has sent its first message: the link stands, the hello and the join are replayed.
    ready: bool,
    /// The watchdog's waits for the owner's core (QUEUE_STATE_WAIT_MS, QUEUE_FOUND_GRACE_MS).
    state_wait: Option<TimerId>,
    found_wait: Option<TimerId>,
}

enum PlaceKind {
    Home(Box<Home>),
    Away(Box<Away>),
}

/// The place the session is in, by its number (`pn`: a step that waited on an answer goes on only in the place it
/// began in).
struct Place {
    pn: u64,
    kind: PlaceKind,
    quiet: Quiet,
}

impl Place {
    fn next_due(&self) -> Option<f64> {
        match &self.kind {
            PlaceKind::Home(x) => x.h.next_due(),
            PlaceKind::Away(x) => x.g.next_due(),
        }
    }
}

/// The session's own deadlines.
#[derive(Clone, Debug, PartialEq)]
enum QueueDue {
    /// SEARCH_EVERY_MS: a search at home, a follow told again.
    Beat,
    /// The watchdog of an away place: no queue.state on, no queue.found by the wait.
    StateWait(u64),
    FoundWait(u64),
}

/// A backend call in flight, and the step it goes on to.
#[derive(Clone, Debug)]
enum QueueAsk {
    /// One mode's search of a merge; `rest` the modes after it.
    Search { pn: u64, mode: QueueMode, rest: Vec<QueueMode> },
    /// The join of a found older lobby; `older` the ones after it, `rest` the modes after its mode.
    Move { pn: u64, lobby: FoundLobby, mode: QueueMode, older: Vec<FoundLobby>, rest: Vec<QueueMode> },
    /// An own lobby of `mode`, listed with the search's start `at`.
    Create { pn: u64, mode: QueueMode, at: f64 },
    /// (a party's member) The follow numbered `n` into the leader's lobby.
    Follow { n: u32, f: PartyFollow },
    /// An own lobby out of the searches.
    Unlist(String),
}

/// The queue.
pub struct QueueSession {
    pub base: SessionBase,
    place: Option<Place>,
    pn: u64,
    /// The places let go, until their byes' timers are done.
    gone: Vec<Place>,
    /// The page's last hello and queue.join (the join's body as it sent it, and as read).
    hello: String,
    join: Option<IndexMap<String, Json>>,
    join_v: Value,
    /// The search's start (wall clock ms; 0: not searching).
    at: f64,
    searching: bool,
    found: bool,
    /// A search is under way.
    looking: bool,
    /// The lobbies whose core refused the replay, and the owners of those it held without a match: not joined again.
    refused: IndexSet<String>,
    refused_owners: IndexSet<String>,
    beat: Option<TimerId>,
    /// The page has been told queue.state on in this search (a later core's is not passed on again).
    state_told: bool,
    /// (a party's leader) The follow last told the member; (a member) the follows asked, and the lobby of the one
    /// under way ('' none).
    told: String,
    follows: u32,
    following_to: String,
    /// (a party's member) The party's last follow from its leader.
    followed: Option<PartyFollow>,
    timers: Timers<QueueDue>,
    asks: Vec<(Via, Ask, QueueAsk)>,
    notes: Vec<PartyNote>,
}

impl QueueSession {
    /// The queue of `page` on `backend` (the relay or Epic): its home core made and the page's link connected to it,
    /// the page told `Opened` (Epic told of the session first: its relays forced).
    pub fn open(svc: &mut NetService, page: PageId, backend: BackendKind, now: f64) -> QueueSession {
        let via = Via::Backend(backend);
        let mut base = SessionBase::new(page, Some(backend), true, svc.config().allow_direct);
        base.me = svc.me_on(via);
        if backend == BackendKind::Epic {
            svc.epic_opened(page, true, base.direct, now);
        }
        let mut s = QueueSession {
            base,
            place: None,
            pn: 0,
            gone: Vec::new(),
            hello: String::new(),
            join: None,
            join_v: Value::Null,
            at: 0.0,
            searching: false,
            found: false,
            looking: false,
            refused: IndexSet::new(),
            refused_owners: IndexSet::new(),
            beat: None,
            state_told: false,
            told: String::new(),
            follows: 0,
            following_to: String::new(),
            followed: None,
            timers: Timers::default(),
            asks: Vec::new(),
            notes: Vec::new(),
        };
        s.new_home(svc, false, now);
        svc.post(page, s.base.opened());
        if let Some(Place { kind: PlaceKind::Home(h), .. }) = &mut s.place {
            h.h.connect_local(svc, &s.base, now);
        }
        s.after_home(svc, now);
        s.beat = Some(s.timers.every(now, SEARCH_EVERY_MS, QueueDue::Beat));
        s
    }

    fn me(&self) -> &str {
        &self.base.me
    }

    fn via(&self) -> Via {
        self.base.via.unwrap_or(Via::Backend(self.base.backend.unwrap_or(BackendKind::Epic)))
    }

    fn is_place(&self, pn: u64) -> bool {
        self.place.as_ref().is_some_and(|p| p.pn == pn)
    }

    fn place_gen(&self) -> Option<u64> {
        self.place.as_ref().map(|p| p.pn)
    }

    fn at_home(&self) -> bool {
        matches!(self.place, Some(Place { kind: PlaceKind::Home(_), .. }))
    }

    fn home_mut(&mut self, pn: u64) -> Option<&mut Home> {
        match &mut self.place {
            Some(Place { pn: g, kind: PlaceKind::Home(h), .. }) if *g == pn => Some(h),
            _ => None,
        }
    }

    fn home(&self) -> Option<&Home> {
        match &self.place {
            Some(Place { kind: PlaceKind::Home(h), .. }) => Some(h),
            _ => None,
        }
    }

    fn away(&self) -> Option<&Away> {
        match &self.place {
            Some(Place { kind: PlaceKind::Away(a), .. }) => Some(a),
            _ => None,
        }
    }

    fn away_mut(&mut self) -> Option<&mut Away> {
        match &mut self.place {
            Some(Place { kind: PlaceKind::Away(a), .. }) => Some(a),
            _ => None,
        }
    }

    /// The home core and the match referee, None while away.
    pub fn hosting_mut(&mut self) -> Option<&mut Hosting> {
        match &mut self.place {
            Some(Place { kind: PlaceKind::Home(h), .. }) => Some(&mut h.h),
            _ => None,
        }
    }

    // a new home core (not connected yet): its number
    fn new_home(&mut self, svc: &mut NetService, replay: bool, now: f64) -> u64 {
        self.pn += 1;
        let h = Hosting::new(svc, &self.base, SessionKind::Queue, None, now);
        let quiet = Quiet { server: replay, welcome: replay && !self.hello.is_empty(), state: false };
        let home = Home { h, own: IndexMap::new(), making: IndexSet::new() };
        self.place = Some(Place { pn: self.pn, kind: PlaceKind::Home(Box::new(home)), quiet });
        self.pn
    }

    pub fn in_lobby(&self, id: &str) -> bool {
        match &self.place {
            None => false,
            Some(Place { kind: PlaceKind::Home(h), .. }) => h.h.lobbies.contains_key(id),
            Some(Place { kind: PlaceKind::Away(a), .. }) => !a.g.done && a.g.lobby == id,
        }
    }

    pub fn has_peer(&self, peer: &str) -> bool {
        match &self.place {
            None => false,
            Some(Place { kind: PlaceKind::Home(h), .. }) => h.h.has_peer(peer),
            Some(Place { kind: PlaceKind::Away(a), .. }) => !a.g.done && a.g.members.contains(peer),
        }
    }

    /// Every player `has_peer` says yes to (the backend's accept set).
    pub fn peers(&self) -> Vec<String> {
        let mut out: IndexSet<String> = IndexSet::new();
        match &self.place {
            Some(Place { kind: PlaceKind::Home(h), .. }) => {
                for m in h.h.lobbies.values() {
                    out.extend(m.iter().filter(|x| h.h.has_peer(x)).cloned());
                }
            }
            Some(Place { kind: PlaceKind::Away(a), .. }) if !a.g.done => out.extend(a.g.members.iter().cloned()),
            _ => {}
        }
        out.into_iter().collect()
    }

    // ---------- the page's join, read as the session needs it ----------

    fn modes(&self) -> Vec<QueueMode> {
        let asks = self.join_v.get("modes").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        QUEUE_MODES.iter().copied().filter(|m| asks.iter().any(|a| a.as_str() == Some(m.id()))).collect()
    }

    /// The join's party (QueueParty: its members, the leader first, this player among them; empty with none). The
    /// page writes it; the session reads it as it is.
    fn party_ids(&self) -> Vec<String> {
        let Some(m) = self.join_v.get("party").and_then(|p| p.get("members")).and_then(Value::as_array) else {
            return Vec::new();
        };
        let ids: Option<Vec<String>> = m.iter().map(|x| x.as_str().map(str::to_string)).collect();
        match ids {
            Some(ids) if ids.len() > 1 && ids.len() <= PARTY_MAX && ids.iter().any(|x| x == self.me()) => ids,
            _ => Vec::new(),
        }
    }

    /// A party's member: it never searches, it follows its leader.
    fn follower(&self) -> bool {
        self.party_ids().first().is_some_and(|x| x != self.me())
    }

    /// A party's leader: it searches for lobbies with room for the party, and tells the member each move.
    fn leads_party(&self) -> bool {
        self.party_ids().first().is_some_and(|x| x == self.me())
    }

    fn bot_wait_ms(&self) -> f64 {
        bot_after_s(self.join_v.get("botAfter").unwrap_or(&Value::Null)) * 1000.0
    }

    fn compat(&self) -> String {
        compat_of(self.join_v.get("compat").unwrap_or(&Value::Null))
    }

    /// The kept join for the place: away, its lobby's mode alone; with the time searched so far.
    fn join_text(&self, svc: &NetService) -> String {
        let modes = match self.away() {
            Some(a) => a.modes.clone(),
            None => self.modes(),
        };
        let mut m = self.join.clone().unwrap_or_default();
        m.insert("t".into(), Json::Str("queue.join".into()));
        m.insert("modes".into(), Json::Arr(modes.iter().map(|x| Json::Str(x.id().into())).collect()));
        // (Math.round: half up)
        let waited = ((svc.wall() - self.at) + 0.5).floor().max(0.0);
        let waited = serde_json::Number::from_f64(waited).map_or(Json::Null, |n| {
            // (an integral value written as JavaScript writes it, without ".0")
            if waited.fract() == 0.0 && waited.abs() < 9.007_199_254_740_992e15 {
                #[allow(clippy::cast_possible_truncation)]
                Json::Num(serde_json::Number::from(waited as i64))
            } else {
                Json::Num(n)
            }
        });
        m.insert("waited".into(), waited);
        serde_json::to_string(&m).unwrap_or_default()
    }

    // ---------- the place's core ----------

    // a text to the place's core
    fn send_place(&mut self, svc: &mut NetService, pn: u64, text: &str, now: f64) {
        if !self.is_place(pn) {
            return;
        }
        let ok = match &mut self.place {
            Some(Place { kind: PlaceKind::Home(h), .. }) => {
                if !h.h.core.ended() {
                    h.h.local_message(svc, &self.base, text, now);
                }
                true
            }
            Some(Place { kind: PlaceKind::Away(a), .. }) => a.g.send(svc, &self.base, text),
            None => true,
        };
        if self.at_home() {
            self.after_home(svc, now);
        } else if !ok {
            self.lost(svc, pn, ClosedWhy::SendFailed, now);
        }
    }

    // the home core's events since the last call (its texts to the page's link, its end, a found match)
    fn after_home(&mut self, svc: &mut NetService, now: f64) {
        loop {
            let Some(pn) = self.place_gen() else { return };
            let Some(h) = self.home_mut(pn) else { return };
            let events = h.h.take_events();
            if events.is_empty() {
                return;
            }
            for e in events {
                if !self.is_place(pn) || self.base.closed {
                    break;
                }
                match e {
                    HostingEvent::Text(t) => self.core_said(svc, pn, &t, now),
                    HostingEvent::LocalClosed(why) | HostingEvent::End(why) => self.end(svc, why, now),
                    HostingEvent::Found(humans) => self.found_at(svc, pn, &humans, now),
                    HostingEvent::Listing(_) => {}
                }
            }
        }
    }

    // an away place's notes (the owner's texts, the link's end)
    fn guest_notes(&mut self, svc: &mut NetService, pn: u64, notes: Vec<GuestNote>, now: f64) {
        for n in notes {
            if !self.is_place(pn) {
                return;
            }
            match n {
                GuestNote::Text(t) => self.core_said(svc, pn, &t, now),
                GuestNote::Lost(why) => self.lost(svc, pn, why, now),
            }
        }
    }

    // the hello and, while it searches, the join, to a new place's core
    fn replay(&mut self, svc: &mut NetService, pn: u64, now: f64) {
        if !self.hello.is_empty() {
            let hello = self.hello.clone();
            self.send_place(svc, pn, &hello, now);
        }
        // (a party member's home core never has the join: it waits there for its leader's follow)
        if self.join.is_some() && self.searching && self.is_place(pn) && !(self.at_home() && self.follower()) {
            let told = self.state_told;
            if let Some(p) = &mut self.place {
                p.quiet.state = told;
            }
            let text = self.join_text(svc);
            self.send_place(svc, pn, &text, now);
        }
    }

    pub fn from_page(&mut self, svc: &mut NetService, text: &str, now: f64) {
        if self.base.closed {
            return;
        }
        // (not one: the core drops it)
        let m: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let t = m.get("t").and_then(Value::as_str).unwrap_or_default().to_string();
        if t == "hello" {
            self.hello = text.to_string();
        }
        if t == "queue.join" {
            self.join = serde_json::from_str(text).ok();
            self.join_v = m;
            // (a new search after a found match: from a new home; the set there, if any, is left)
            if self.found {
                self.found = false;
                self.searching = false;
                self.state_told = false;
                self.at = 0.0;
                self.told.clear();
                self.refused.clear();
                self.refused_owners.clear();
                let pn = self.go_home(svc, now);
                if self.follower() {
                    self.start_following(svc, now);
                } else {
                    self.send_place(svc, pn, text, now);
                }
                return;
            }
        }
        if self.place.is_none() {
            return;
        }
        if t == "queue.join" && self.follower() {
            self.start_following(svc, now);
            if self.at_home() {
                return;
            }
        }
        if t == "queue.cancel" && self.away().is_some() && !self.found {
            self.stop_search(svc, now);
            let pn = self.go_home(svc, now);
            self.send_place(svc, pn, text, now);
            return;
        }
        let Some(pn) = self.place_gen() else { return };
        // (before the owner's first message nothing goes to it: the hello and the join are replayed then)
        if self.away().is_some_and(|a| !a.ready) {
            return;
        }
        let text = if t == "queue.join" && self.away().is_some() { self.join_text(svc) } else { text.to_string() };
        self.send_place(svc, pn, &text, now);
    }

    // a message of a place's core
    fn core_said(&mut self, svc: &mut NetService, pn: u64, text: &str, now: f64) {
        if self.base.closed || !self.is_place(pn) {
            return;
        }
        if self.away().is_some_and(|a| !a.ready) {
            if let Some(a) = self.away_mut() {
                a.ready = true;
            }
            self.watch_away(svc, pn, now);
            self.replay(svc, pn, now);
            if !self.is_place(pn) {
                return;
            }
        }
        // (passed on as it is)
        let m: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let t = m.get("t").and_then(Value::as_str).unwrap_or_default();
        let away = self.away().is_some();
        let Some(p) = &mut self.place else { return };
        if t == "server" && p.quiet.server {
            p.quiet.server = false;
            return;
        }
        if t == "welcome" && p.quiet.welcome {
            p.quiet.welcome = false;
            return;
        }
        // Unverified: (the port's own) a later core that refuses the replayed hello (its name is taken there) is a
        // lobby this player cannot be in: away, the session goes home and searches on without that lobby; at home (a
        // new core of its own, nobody else in it) it cannot be taken, and a refused name is the first core's answer
        // again.
        if (t == "name.taken" || t == "name.refused") && p.quiet.welcome {
            p.quiet.welcome = false;
            if away {
                self.refused_by(svc, pn, t, now);
            }
            return;
        }
        if t == "queue.state" && !m.is_null() {
            let quiet = p.quiet.state;
            p.quiet.state = false;
            let why = match m.get("why") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Null) => "null".into(),
                Some(v) => v.to_string(),
                None => "undefined".into(),
            };
            if m.get("on") == Some(&Value::Bool(true)) {
                if away {
                    if let Some(a) = self.away_mut()
                        && let Some(t) = a.state_wait.take()
                    {
                        self.timers.cancel(t);
                    }
                } else {
                    self.search_from(svc, pn, now);
                }
                if quiet {
                    return;
                }
                self.state_told = true;
            } else if !self.found && !self.party_ids().is_empty() && PARTY_WHYS.contains(&why.as_str()) {
                // (the core took the party's ticket out: a party join it could not take, or a member late or gone.
                // Not a lobby to search on from: home, out of the queue, the page told, the other side's session
                // too; the party stays)
                let at = self.away().map_or_else(|| "at home".to_string(), |a| a.g.lobby.clone());
                svc.log(&format!("queue: the party is out of the queue {at} {why}"));
                self.stop_search(svc, now);
                self.follows += 1;
                self.following_to.clear();
                if away {
                    self.go_home(svc, now);
                }
                let why = if why == "partyLate" { QueueWhy::PartyLate } else { QueueWhy::Party };
                self.notes.push(PartyNote::QueueOut(why));
            } else if away && !self.found {
                // (the owner's core no longer queues this player: home, where the page hears of a real refusal)
                return self.refused_by(svc, pn, &format!("queue.state {why}"), now);
            } else if !self.found {
                self.stop_search(svc, now);
            }
        }
        if t == "queue.found" {
            self.found = true;
            self.searching = false;
            self.unwatch();
        }
        svc.post(self.base.page, crate::service::api::NetEvent::Msg(text.to_string()));
    }

    // From the owner's first message: its core answers the replayed join (QUEUE_STATE_WAIT_MS) and finds a match by
    // this player's own bot wait (QUEUE_FOUND_GRACE_MS), or the session goes home without that lobby and its owner.
    fn watch_away(&mut self, svc: &NetService, pn: u64, now: f64) {
        if self.join.is_none() || !self.searching {
            return;
        }
        let due = self.at + self.bot_wait_ms() + QUEUE_FOUND_GRACE_MS - svc.wall();
        let s = self.timers.after(now, QUEUE_STATE_WAIT_MS, QueueDue::StateWait(pn));
        let f = self.timers.after(now, QUEUE_FOUND_GRACE_MS.max(due), QueueDue::FoundWait(pn));
        if let Some(a) = self.away_mut() {
            a.state_wait = Some(s);
            a.found_wait = Some(f);
        }
    }

    fn unwatch(&mut self) {
        let Some(a) = self.away_mut() else { return };
        let ids: Vec<TimerId> = a.state_wait.take().into_iter().chain(a.found_wait.take()).collect();
        for t in ids {
            self.timers.cancel(t);
        }
    }

    // the owner's core held this player without an answer: home, and not to that lobby or its owner again
    fn held(&mut self, svc: &mut NetService, pn: u64, why: &str, now: f64) {
        if !self.is_place(pn) || self.found || self.base.closed {
            return;
        }
        if let Some(a) = self.away_mut() {
            a.state_wait = None;
            a.found_wait = None;
        }
        if let Some(owner) = self.away().map(|a| a.g.owner.clone()) {
            self.refused_owners.insert(owner);
        }
        self.refused_by(svc, pn, why, now);
    }

    // the core of the lobby it is in refused it: home, searching on, and not to that lobby again
    fn refused_by(&mut self, svc: &mut NetService, pn: u64, why: &str, now: f64) {
        if !self.is_place(pn) {
            return;
        }
        let Some(lobby) = self.away().map(|a| a.g.lobby.clone()) else { return };
        svc.log(&format!("queue: refused by the lobby {lobby} {why}"));
        self.refused.insert(lobby.clone());
        if self.follower() && self.following_to.is_empty() {
            self.notes.push(PartyNote::Unfollowed(lobby));
        }
        self.go_home(svc, now);
    }

    // the home core queues the page: the search starts (or goes on)
    fn search_from(&mut self, svc: &mut NetService, pn: u64, now: f64) {
        if self.found || self.follower() {
            return;
        }
        if !self.searching {
            self.searching = true;
            if self.at == 0.0 {
                self.at = svc.wall();
            }
        }
        self.search(svc, pn, now);
    }

    // out of the search: the own lobbies destroyed (their members told: they search again)
    fn stop_search(&mut self, svc: &mut NetService, now: f64) {
        self.searching = false;
        self.state_told = false;
        self.at = 0.0;
        self.told.clear();
        let Some(Place { kind: PlaceKind::Home(h), .. }) = &mut self.place else { return };
        let own: Vec<String> = h.own.values().cloned().collect();
        for id in own {
            h.h.drop_lobby(svc, &self.base, &id, &ClosedWhy::QueueMoved, now);
        }
        h.own.clear();
        self.after_home(svc, now);
    }

    fn can_merge(&self, svc: &NetService, pn: u64) -> bool {
        // (once the player's own wait is over its core fills bots: no move into another lobby then, so a lobby that
        // never answers cannot keep it from its bots)
        let waited = svc.wall() - self.at >= self.bot_wait_ms();
        // (a party's member in the leader's own lobby is no other player: the party moves on together)
        let Some(h) = self.home().filter(|_| self.is_place(pn)) else { return false };
        !self.base.closed
            && self.searching
            && !self.found
            && !waited
            && !self.follower()
            && !h.h.has_guests(&self.party_ids())
    }

    // one search of each mode's bucket for an older lobby with a free place; then the own lobbies that are missing
    fn search(&mut self, svc: &mut NetService, pn: u64, now: f64) {
        if self.looking || self.follower() {
            return;
        }
        self.looking = true;
        if self.can_merge(svc, pn) && !self.compat().is_empty() {
            let modes = self.modes();
            return self.merge_modes(svc, pn, modes, now);
        }
        self.search_done(svc, pn, now);
    }

    // the search is over
    fn search_done(&mut self, svc: &mut NetService, pn: u64, now: f64) {
        self.looking = false;
        if self.is_place(pn) && self.at_home() && self.searching && !self.found && !self.base.closed {
            self.ensure_lobbies(svc, pn, now);
        }
    }

    // the next mode's search of a merge (none left: the search is over)
    fn merge_modes(&mut self, svc: &mut NetService, pn: u64, mut modes: Vec<QueueMode>, now: f64) {
        let compat = self.compat();
        while !modes.is_empty() {
            let mode = modes.remove(0);
            let via = self.via();
            let bucket = queue_bucket(mode, &compat);
            if let Some(b) = self.base.eos(svc) {
                let ask = lock(&b).search_lobbies(&bucket, SEARCH_LIMIT, false);
                self.asks.push((via, ask, QueueAsk::Search { pn, mode, rest: modes }));
                return;
            }
            svc.log(&format!("queue: search {} the backend is off", mode.id()));
        }
        self.search_done(svc, pn, now);
    }

    // a mode's search answered: its older lobbies with a free place, the oldest first
    fn searched(
        &mut self,
        svc: &mut NetService,
        pn: u64,
        mode: QueueMode,
        rest: Vec<QueueMode>,
        list: &[FoundLobby],
        now: f64,
    ) {
        if !self.can_merge(svc, pn) {
            return self.search_done(svc, pn, now);
        }
        // (a party moves only into a lobby with a place for each of its players)
        let size = self.party_ids().len().max(1);
        let mine_id = self.home().and_then(|h| h.own.get(&mode).cloned()).unwrap_or_else(|| "\u{ffff}".to_string());
        let mine = (self.at, mine_id.as_str());
        let mut older: Vec<FoundLobby> = list
            .iter()
            .filter(|l| {
                l.owner != self.me()
                    && l.at > 0.0
                    && l.max.saturating_sub(l.members) as usize >= size
                    && l.max >= l.members
                    && !self.refused.contains(&l.id)
                    && !self.refused_owners.contains(&l.owner)
                    && before((l.at, &l.id), mine)
            })
            .cloned()
            .collect();
        older.sort_by(|a, b| {
            if before((a.at, &a.id), (b.at, &b.id)) { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }
        });
        self.try_older(svc, pn, mode, older, rest, now);
    }

    // the next older lobby's join (none left: the next mode)
    fn try_older(
        &mut self,
        svc: &mut NetService,
        pn: u64,
        mode: QueueMode,
        mut older: Vec<FoundLobby>,
        rest: Vec<QueueMode>,
        now: f64,
    ) {
        while !older.is_empty() {
            let l = older.remove(0);
            let via = self.via();
            if let Some(b) = self.base.eos(svc) {
                let ask = lock(&b).join_lobby(&l.id);
                self.asks.push((via, ask, QueueAsk::Move { pn, lobby: l, mode, older, rest }));
                return;
            }
            svc.log(&format!("queue: join {} the backend is off", l.id));
            if !self.can_merge(svc, pn) {
                return self.search_done(svc, pn, now);
            }
        }
        self.merge_modes(svc, pn, rest, now)
    }

    // a found lobby's join answered: the move there, or the next one
    #[allow(clippy::too_many_arguments)]
    fn joined(
        &mut self,
        svc: &mut NetService,
        pn: u64,
        l: FoundLobby,
        mode: QueueMode,
        older: Vec<FoundLobby>,
        rest: Vec<QueueMode>,
        r: &Result<Answer, String>,
        now: f64,
    ) {
        let moved = match r {
            Err(e) => {
                svc.log(&format!("queue: join {} {e}", l.id));
                false
            }
            Ok(Answer::Joined { owner, members, .. }) => {
                // (a member came to its own lobby meanwhile, or the search ended: it stays)
                if !self.can_merge(svc, pn) || owner.is_empty() || owner == self.me() {
                    self.leave_lobby(svc, &l.id);
                    false
                } else {
                    svc.log(&format!("queue: to the lobby {} of {owner}", l.id));
                    // (the member hears of the move before the bye of the core it leaves: one channel, in order)
                    self.tell_follow(Some(PartyFollow { lobby: l.id.clone(), modes: vec![mode] }));
                    self.leave_place(svc, &ClosedWhy::QueueMoved, now);
                    self.new_away(svc, &l.id, owner, members, vec![mode], now);
                    true
                }
            }
            Ok(_) => false,
        };
        if moved {
            return self.search_done(svc, pn, now);
        }
        if !self.can_merge(svc, pn) {
            return self.search_done(svc, pn, now);
        }
        self.try_older(svc, pn, mode, older, rest, now);
    }

    // an away place in a joined lobby: its knock to the owner starts
    fn new_away(
        &mut self,
        svc: &mut NetService,
        lobby: &str,
        owner: &str,
        members: &[String],
        modes: Vec<QueueMode>,
        now: f64,
    ) {
        self.pn += 1;
        let quiet = Quiet { server: true, welcome: !self.hello.is_empty(), state: false };
        let g = Guesting::new(svc, &self.base, lobby, owner, owner, members, now);
        let away = Away { g, modes, ready: false, state_wait: None, found_wait: None };
        self.place = Some(Place { pn: self.pn, kind: PlaceKind::Away(Box::new(away)), quiet });
    }

    // the own lobbies of the modes that have none, listed with the search's start
    fn ensure_lobbies(&mut self, svc: &mut NetService, pn: u64, _now: f64) {
        let compat = self.compat();
        if compat.is_empty() {
            return;
        }
        for mode in self.modes() {
            let via = self.via();
            let at = self.at;
            let Some(h) = self.home_mut(pn) else { return };
            if h.own.contains_key(&mode) || h.making.contains(&mode) {
                continue;
            }
            h.making.insert(mode);
            let Some(b) = self.base.eos(svc) else { continue };
            let listed = Listed { at, info: None, code: None };
            let ask = lock(&b).create_lobby(&queue_bucket(mode, &compat), queue_size(mode) as u32, Some(listed));
            self.asks.push((via, ask, QueueAsk::Create { pn, mode, at }));
        }
    }

    // an own lobby's make answered
    fn created(
        &mut self,
        svc: &mut NetService,
        pn: u64,
        mode: QueueMode,
        at: f64,
        r: &Result<Answer, String>,
        now: f64,
    ) {
        if let Some(h) = self.home_mut(pn) {
            h.making.shift_remove(&mode);
        } else if let Some(Place { kind: PlaceKind::Home(h), .. }) = self.gone.iter_mut().find(|p| p.pn == pn) {
            h.making.shift_remove(&mode);
        }
        let id = match r {
            Ok(Answer::Created(id)) => id.clone(),
            Ok(_) => return,
            Err(e) => return svc.log(&format!("queue: the lobby could not be made {} {e}", mode.id())),
        };
        if self.base.closed
            || !self.is_place(pn)
            || !self.at_home()
            || !self.searching
            || self.found
            || self.at != at
            || !self.modes().contains(&mode)
        {
            return self.leave_lobby(svc, &id);
        }
        if let Some(h) = self.home_mut(pn) {
            h.own.insert(mode, id.clone());
            h.h.add_lobby(&id);
        }
        svc.log(&format!("queue: listed {} {id}", mode.id()));
        self.tell_follow(None);
        let _ = now;
    }

    // the home core found a match (queue.found sent to its humans)
    fn found_at(&mut self, svc: &mut NetService, pn: u64, humans: &[String], now: f64) {
        if !self.is_place(pn) {
            return;
        }
        let me = self.me().to_string();
        let Some(Place { kind: PlaceKind::Home(h), .. }) = &mut self.place else { return };
        // (a match of guests alone, which the core makes only for a guest that asked for a mode its host does not
        // wait in: not this host's to run; their links are closed and they search again)
        if !humans.contains(&me) {
            h.h.close_links(svc, &self.base, humans, &ClosedWhy::QueueUnmatched, now);
            return;
        }
        self.found = true;
        self.searching = false;
        h.h.seal(svc, &self.base, humans, &ClosedWhy::QueueUnmatched, now);
        let via = self.base.via.unwrap_or(Via::Backend(BackendKind::Epic));
        for (mode, id) in h.own.clone() {
            let has = h.h.lobbies.get(&id).is_some_and(|m| humans.iter().any(|x| *x != me && m.contains(x)));
            if has {
                if let Some(b) = self.base.eos(svc) {
                    let ask = lock(&b).set_listed(&id, None);
                    self.asks.push((via, ask, QueueAsk::Unlist(id)));
                }
                continue;
            }
            h.h.drop_lobby(svc, &self.base, &id, &ClosedWhy::QueueUnmatched, now);
            h.own.shift_remove(&mode);
        }
    }

    // the place it is in is let go (this side's leave): kept until its byes' timers are done
    fn leave_place(&mut self, svc: &mut NetService, why: &ClosedWhy, now: f64) {
        self.unwatch();
        let Some(mut p) = self.place.take() else { return };
        match &mut p.kind {
            PlaceKind::Home(h) => {
                h.h.close(svc, &self.base, why, now);
                let _ = h.h.take_events();
            }
            PlaceKind::Away(a) => a.g.end(svc, &self.base, why, now),
        }
        self.gone.push(p);
    }

    // a new home core: the hello replayed, and the join while it searches
    fn go_home(&mut self, svc: &mut NetService, now: f64) -> u64 {
        self.leave_place(svc, &ClosedWhy::QueueMoved, now);
        let pn = self.new_home(svc, true, now);
        if let Some(Place { kind: PlaceKind::Home(h), .. }) = &mut self.place {
            h.h.connect_local(svc, &self.base, now);
        }
        self.after_home(svc, now);
        self.replay(svc, pn, now);
        pn
    }

    // the owner's side ended the link: before a found match, home and searching again; after it, the session's end
    fn lost(&mut self, svc: &mut NetService, pn: u64, why: ClosedWhy, now: f64) {
        if !self.is_place(pn) {
            return;
        }
        if self.found {
            return self.end(svc, why, now);
        }
        let lobby = self.away().map(|a| a.g.lobby.clone()).unwrap_or_default();
        svc.log(&format!("queue: back from the lobby {lobby} {why:?}"));
        if self.follower() && self.following_to.is_empty() {
            self.notes.push(PartyNote::Unfollowed(lobby));
        }
        self.unwatch();
        if let Some(p) = self.place.take() {
            self.gone.push(p);
        }
        self.go_home(svc, now);
    }

    fn leave_lobby(&self, svc: &mut NetService, id: &str) {
        let keeps = !self.base.closed && self.keeps_lobby(id);
        self.base.leave_lobby(svc, id, keeps);
    }

    // ---------- a party in the queue ----------

    /// (a party's leader) Where the member follows now: the lobby it is away in, or at home its own lobby that has
    /// the member already, else the first with a place for it (its core takes the member's join for every mode).
    fn follow_target(&self) -> Option<PartyFollow> {
        match &self.place.as_ref()?.kind {
            PlaceKind::Away(a) => (!a.g.done).then(|| PartyFollow { lobby: a.g.lobby.clone(), modes: a.modes.clone() }),
            PlaceKind::Home(h) => {
                let ids = self.party_ids();
                let mut room: Option<String> = None;
                for mode in self.modes() {
                    let Some(id) = h.own.get(&mode) else { continue };
                    let Some(members) = h.h.lobbies.get(id) else { continue };
                    if ids.iter().any(|x| x != self.me() && members.contains(x)) {
                        return Some(PartyFollow { lobby: id.clone(), modes: self.modes() });
                    }
                    if room.is_none() && queue_size(mode) as i64 - members.len() as i64 >= ids.len() as i64 - 1 {
                        room = Some(id.clone());
                    }
                }
                room.map(|lobby| PartyFollow { lobby, modes: self.modes() })
            }
        }
    }

    /// (a party's leader) The member told where to follow, when that changed.
    fn tell_follow(&mut self, f: Option<PartyFollow>) {
        let Some(f) = f.or_else(|| self.follow_target()) else { return };
        if !self.leads_party() || !self.searching || self.found || self.base.closed {
            return;
        }
        let key = format!("{} {}", f.lobby, f.modes.iter().map(|m| m.id()).collect::<Vec<_>>().join(","));
        if key == self.told {
            return;
        }
        self.told = key;
        self.notes.push(PartyNote::SendFollow(f));
    }

    /// The other side of the party left the queue by a party why, or the party ended: out of the search too (a member
    /// stops following), home, and the page told queue.state off with that why.
    fn party_out(&mut self, svc: &mut NetService, why: QueueWhy, now: f64) {
        if self.base.closed || self.found || !self.searching || self.party_ids().is_empty() {
            return;
        }
        svc.log(&format!("queue: out with the party {why:?}"));
        self.stop_search(svc, now);
        self.follows += 1;
        self.following_to.clear();
        self.go_home(svc, now);
        let why = serde_json::to_string(&why).unwrap_or_default();
        let text = format!(r#"{{"t":"queue.state","on":false,"why":{why}}}"#);
        svc.post(self.base.page, crate::service::api::NetEvent::Msg(text));
    }

    fn party_changed(&mut self) {
        self.told.clear();
        self.tell_follow(None);
    }

    /// (a party's leader) The member could not follow into that lobby: away there, both go home (and not to that
    /// lobby again); at home, the member is told again at the next beat.
    fn unfollowed(&mut self, svc: &mut NetService, lobby: &str, now: f64) {
        if !self.leads_party() || self.found || self.base.closed {
            return;
        }
        let Some(pn) = self.place_gen() else { return };
        if self.away().is_some_and(|a| a.g.lobby == lobby) {
            return self.refused_by(svc, pn, "the party member could not follow", now);
        }
        if self.at_home() {
            self.told.clear();
        }
    }

    // (a party's member) its join: in the queue from now, where the leader last said
    fn start_following(&mut self, svc: &mut NetService, now: f64) {
        if !self.searching {
            self.searching = true;
            if self.at == 0.0 {
                self.at = svc.wall();
            }
        }
        if let Some(f) = self.followed.clone() {
            self.follow(svc, f, now);
        }
    }

    /// (a party's member) The leader's lobby: joined (away, never home), the join replayed there.
    fn follow(&mut self, svc: &mut NetService, f: PartyFollow, now: f64) {
        if !self.follower() || !self.searching || self.found || self.base.closed || f.lobby.is_empty() {
            return;
        }
        if self.away().is_some_and(|a| !a.g.done && a.g.lobby == f.lobby) || self.following_to == f.lobby {
            return;
        }
        self.follows += 1;
        let n = self.follows;
        self.following_to = f.lobby.clone();
        let via = self.via();
        match self.base.eos(svc) {
            Some(b) => {
                let ask = lock(&b).join_lobby(&f.lobby);
                self.asks.push((via, ask, QueueAsk::Follow { n, f }));
            }
            None => {
                svc.log(&format!("queue: follow {} the backend is off", f.lobby));
                self.followed_to(svc, n, f, None, now);
            }
        }
    }

    // a follow's join answered (None: it failed)
    fn followed_to(
        &mut self,
        svc: &mut NetService,
        n: u32,
        f: PartyFollow,
        r: Option<(String, Vec<String>)>,
        now: f64,
    ) {
        if n != self.follows {
            if r.is_some() {
                self.leave_lobby(svc, &f.lobby);
            }
            return;
        }
        self.following_to.clear();
        let live = !self.base.closed && self.searching && !self.found && self.follower();
        let ok = r.as_ref().is_some_and(|(owner, _)| !owner.is_empty() && owner != self.me());
        if !live || !ok {
            if r.is_some() {
                self.leave_lobby(svc, &f.lobby);
            }
            // (a follow that failed sends both home: the leader searches again, and says where)
            if live {
                self.notes.push(PartyNote::Unfollowed(f.lobby.clone()));
                if !self.at_home() {
                    self.go_home(svc, now);
                }
            }
            return;
        }
        let (owner, members) = r.unwrap_or_default();
        svc.log(&format!("queue: following the party to the lobby {} of {owner}", f.lobby));
        self.leave_place(svc, &ClosedWhy::QueueMoved, now);
        let modes = if f.modes.is_empty() { self.modes() } else { f.modes.clone() };
        self.new_away(svc, &f.lobby, &owner, &members, modes, now);
    }

    // ---------- the backend's events ----------

    pub fn message(&mut self, svc: &mut NetService, from: &str, bytes: &[u8], now: f64) {
        if self.base.closed {
            return;
        }
        let Some(pn) = self.place_gen() else { return };
        match &mut self.place {
            Some(Place { kind: PlaceKind::Home(h), .. }) => {
                h.h.message(svc, &self.base, from, bytes, now);
                self.after_home(svc, now);
            }
            Some(Place { kind: PlaceKind::Away(a), .. }) => {
                let notes = a.g.message(svc, &self.base, from, bytes, now);
                self.guest_notes(svc, pn, notes, now);
            }
            None => {}
        }
    }

    pub fn lobby_changed(&mut self, svc: &mut NetService, c: &LobbyChange, now: f64) {
        if self.base.closed {
            return;
        }
        match (&mut self.place, c) {
            (Some(Place { kind: PlaceKind::Away(a), .. }), _) => a.g.lobby_changed(svc, &self.base, c, now),
            (Some(Place { kind: PlaceKind::Home(h), .. }), LobbyChange::Members { lobby, members }) => {
                h.h.lobby_changed(svc, &self.base, lobby, members, now);
                self.after_home(svc, now);
            }
            // (the backend took an own lobby: its members are gone, and the next search makes it again)
            (Some(Place { kind: PlaceKind::Home(h), .. }), LobbyChange::Closed { lobby, .. }) => {
                h.own.retain(|_, id| id != lobby);
                h.h.forget_lobby(svc, &self.base, lobby, now);
                self.after_home(svc, now);
            }
            (None, _) => {}
        }
    }

    pub fn peer_closed(&mut self, svc: &mut NetService, peer: &str, now: f64) {
        if self.base.closed {
            return;
        }
        let Some(pn) = self.place_gen() else { return };
        match &mut self.place {
            Some(Place { kind: PlaceKind::Home(h), .. }) => {
                h.h.peer_closed(svc, &self.base, peer, now);
                self.after_home(svc, now);
            }
            Some(Place { kind: PlaceKind::Away(a), .. }) => {
                let notes = a.g.peer_closed(svc, &self.base, peer, now);
                self.guest_notes(svc, pn, notes, now);
            }
            None => {}
        }
    }

    /// The session is over for this page: `Closed` with why, its place let go (`Left`: as the host left).
    pub fn end(&mut self, svc: &mut NetService, why: ClosedWhy, now: f64) {
        if self.base.closed {
            return;
        }
        let leave =
            if why == ClosedWhy::Left { ClosedWhy::Host { why: mvs_host::HostWhy::HostLeft } } else { why.clone() };
        self.base.finish(svc, why, now);
        if let Some(t) = self.beat.take() {
            self.timers.cancel(t);
        }
        self.leave_place(svc, &leave, now);
    }
}

// The calls the service's dispatch makes beside the shape's.
impl QueueSession {
    /// The session itself still uses that lobby from its place.
    pub fn keeps_lobby(&self, id: &str) -> bool {
        self.in_lobby(id)
    }

    /// The session itself still uses the link to that peer from its place.
    pub fn keeps_peer(&self, peer: &str) -> bool {
        match &self.place {
            None => false,
            Some(Place { kind: PlaceKind::Home(h), .. }) => h.h.linked(peer),
            Some(Place { kind: PlaceKind::Away(a), .. }) => !a.g.done && a.g.owner == peer,
        }
    }

    /// A backend's answer: true when the call was this session's.
    /// The backend of `via` is gone: its asks are let go.
    pub fn forget(&mut self, via: Via) {
        self.asks.retain(|(v, _, _)| *v != via);
    }

    pub fn answered(&mut self, svc: &mut NetService, via: Via, ask: Ask, r: &Result<Answer, String>, now: f64) -> bool {
        let Some(i) = self.asks.iter().position(|(v, a, _)| *v == via && *a == ask) else { return false };
        let (_, _, what) = self.asks.remove(i);
        match what {
            QueueAsk::Search { pn, mode, rest } => match r {
                Ok(Answer::Found(list)) => self.searched(svc, pn, mode, rest, list, now),
                Ok(_) => self.merge_modes(svc, pn, rest, now),
                Err(e) => {
                    svc.log(&format!("queue: search {} {e}", mode.id()));
                    self.merge_modes(svc, pn, rest, now);
                }
            },
            QueueAsk::Move { pn, lobby, mode, older, rest } => self.joined(svc, pn, lobby, mode, older, rest, r, now),
            QueueAsk::Create { pn, mode, at } => self.created(svc, pn, mode, at, r, now),
            QueueAsk::Follow { n, f } => {
                let joined = match r {
                    Ok(Answer::Joined { owner, members, .. }) => Some((owner.clone(), members.clone())),
                    Ok(_) => None,
                    Err(e) => {
                        svc.log(&format!("queue: follow {} {e}", f.lobby));
                        None
                    }
                };
                self.followed_to(svc, n, f, joined, now);
            }
            QueueAsk::Unlist(id) => {
                if let Err(e) = r {
                    svc.log(&format!("queue: unlist {id} {e}"));
                }
            }
        }
        true
    }

    /// Runs what is due by `now`: its own timers, its place's and the places let go, in time order.
    pub fn pump(&mut self, svc: &mut NetService, now: f64) {
        loop {
            let own = self.timers.next_due().filter(|t| *t <= now);
            let place = self.place.as_ref().and_then(Place::next_due).filter(|t| *t <= now);
            let gone = self
                .gone
                .iter()
                .enumerate()
                .filter_map(|(i, p)| p.next_due().filter(|t| *t <= now).map(|t| (i, t)))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let best = [own.map(|t| (0, t)), place.map(|t| (1, t)), gone.map(|(_, t)| (2, t))]
                .into_iter()
                .flatten()
                .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            let Some((who, at)) = best else { break };
            match who {
                0 => {
                    let Some((_, _, t)) = self.timers.take_due(at) else { break };
                    self.due(svc, t, at);
                }
                1 => {
                    let Some(pn) = self.place_gen() else { break };
                    match &mut self.place {
                        Some(Place { kind: PlaceKind::Home(h), .. }) => {
                            h.h.pump(svc, &self.base, at);
                            self.after_home(svc, at);
                        }
                        Some(Place { kind: PlaceKind::Away(a), .. }) => {
                            let notes = a.g.pump(svc, &self.base, at);
                            self.guest_notes(svc, pn, notes, at);
                        }
                        None => {}
                    }
                }
                _ => {
                    let Some((i, _)) = gone else { break };
                    let p = &mut self.gone[i];
                    match &mut p.kind {
                        PlaceKind::Home(h) => {
                            h.h.pump(svc, &self.base, at);
                            let _ = h.h.take_events();
                        }
                        PlaceKind::Away(a) => {
                            let _ = a.g.pump(svc, &self.base, at);
                        }
                    }
                }
            }
        }
        self.gone.retain(|p| p.next_due().is_some());
    }

    // one of its own timers
    fn due(&mut self, svc: &mut NetService, t: QueueDue, now: f64) {
        match t {
            QueueDue::Beat => {
                if let Some(pn) = self.place_gen()
                    && self.at_home()
                    && self.searching
                    && !self.found
                    && !self.follower()
                {
                    self.search(svc, pn, now);
                }
                // (a follow the member could not take is told again here, at most once a beat)
                self.tell_follow(None);
            }
            QueueDue::StateWait(pn) => self.held(svc, pn, "no queue.state", now),
            QueueDue::FoundWait(pn) => self.held(svc, pn, "no queue.found by the wait", now),
        }
    }

    pub fn next_due(&self) -> Option<f64> {
        let place = self.place.as_ref().and_then(Place::next_due);
        let gone = self.gone.iter().filter_map(Place::next_due);
        self.timers.next_due().into_iter().chain(place).chain(gone).reduce(f64::min)
    }

    /// What the page's party is to hear, since the last call.
    pub fn take_notes(&mut self) -> Vec<PartyNote> {
        std::mem::take(&mut self.notes)
    }

    /// A note of the page's party.
    pub fn note(&mut self, svc: &mut NetService, n: SessionNote, now: f64) {
        match n {
            SessionNote::Changed => self.party_changed(),
            SessionNote::Out(why) => self.party_out(svc, why, now),
            SessionNote::Follow(f) => self.follow(svc, f, now),
            SessionNote::Unfollowed(lobby) => self.unfollowed(svc, &lobby, now),
            SessionNote::Followed(f) => self.followed = f,
        }
    }
}
