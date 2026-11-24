//! The net service (app/src/sessions.ts NetService and Page, net.ts, netproc.ts): the pages' sessions over the
//! backends signed in (Epic's, the relay's, the LAN host), none of them (online off: every open but a code that names
//! a link of its own is `Closed`), and over the sessions' links of their own.
//!
//! Sans-IO like the host core: the owner passes the clock's `now` (ms) to each call, runs `pump(now)` at `next_due()`,
//! and drains each page's events. `spawn` runs one on a thread of its own for the game, the pages' ports channels to
//! it (the TS's net process and its MessagePorts).
//!
//! The routing: a backend's events go to the sessions and parties on it alone. A call into a session or a party
//! takes it out of its page for the call (`&mut NetService` beside it) and puts it back after, unless it ended (then
//! it is kept aside while timers of its own are still set: its byes' BYE_MS). A backend's answer
//! (`BackendEvent::Answered`) is offered to the service's own asks first (a browse's search, a who, a presence's set),
//! then to each session and party until one takes it. Each session and party keeps its own timers; the service's pump
//! runs everyone's earliest first, two at once in the order they were set (page::Timers). A party and a queue session
//! talk through notes: each queues its own, the service hands them across after each call. Who may open a link to
//! this player (the backend's accept set) is the members of the lobbies of every session and party on that backend,
//! set on it after each step.
//!
//! Deviation: the TS's awaited steps (a direct host's start, a direct code's join, a relay code's own link, a host
//! name's lookup) are blocking calls on the service's thread here; their answers come in the same call.

pub mod api;
pub mod custom;
pub mod hosting;
pub mod page;
pub mod party;
pub mod path;
pub mod queue;
mod refereed;
pub mod relay_host;
pub mod session;
pub mod signin;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};

use indexmap::{IndexMap, IndexSet};
use serde_json::Value;

use mvs_host::bucket::Bucket;
use mvs_host::{HostLog, HostTimes};
use mvs_protocol::match_wire::MAX_SLOTS;
use mvs_protocol::names::{BAD_WORDS, Profanity, clean_name};
use mvs_protocol::online::QueueMode;
use mvs_protocol::p2p_frame::FRAME_PARTY;
use mvs_protocol::relay_wire::{KnownRelay, Presence as RelayPresence, WHO_LIMIT, Who, parse_address};
use mvs_protocol::version::PROTOCOL;

use crate::backend::{Answer, Ask, Backend, BackendEvent, BackendKind, Channel, FoundLobby, LobbyChange, RelayMode};
use crate::codes::{CodeMakeWhy, LobbyCode, decode_lobby_code, encode_lobby_code, normal_relay, same_relay};
use crate::lan::announce::{ANNOUNCE_EVERY_MS, DISCOVERY_PORT, LanListener, ListenerOptions};
use crate::lan::direct::HostStartWhy;
use crate::lan::direct_code::direct_text;
use crate::link::relay::{NetFaults, RelayClient, RelayError};
use crate::pending::Pending;
use api::{
    BackendOff, BackendState, BackendText, BrowseWhy, BrowsedLobby, ClosedWhy, DirectOptions, DirectStatus, InviteKind,
    JoinWhy, LanOptions, LocalWhy, MenuFighter, NetEvent, NetRequest, NetState, OffWhy, OnlineBackends, PartyJoinWhy,
    SessionTarget, StatusWhy,
};
use custom::{GuestSession, GuestVia, HostSession};
use page::{AskFor, Page, TimerId, Timers};
use party::Party;
use queue::QueueSession;
use relay_host::RelayHosted;
use session::{MatchRoute, Session, Via};

pub use page::PageId;

/// A custom lobby's bucket. The compatibility id is not in it: the host core refuses a joiner of another one
/// (lobby.join's compat), and a lobby is found by its code only.
pub const CUSTOM_BUCKET: &str = "mvs:custom";
/// A custom lobby's places (`lobby_rules::MAX_LOBBY_PLAYERS`: a Versus of 4v4, a Free For All of eight).
pub const CUSTOM_MAX: u32 = 8;
const _: () = assert!(CUSTOM_MAX as usize == mvs_protocol::lobby_rules::MAX_LOBBY_PLAYERS);
/// With LAN on, the LAN lobbies are listened for from a browse until this long after the last one (the browser's own
/// refresh, every 15 s, keeps it open while it is up). Unverified: the port's own number.
pub const BROWSE_LISTEN_MS: f64 = 30000.0;
/// A browse within this long of the listener's start waits until then: one announcement period and a little, so
/// that the first browse has the lobbies already announced.
pub const FIRST_LISTEN_MS: f64 = ANNOUNCE_EVERY_MS + 200.0;
/// A browse asks for this many public custom lobbies (the relay gives 100 at the most, EOS 200).
pub const BROWSE_LIMIT: u32 = 100;
/// A browse within this long of the last one's answer on the same backend gets that answer again, so a page's
/// refreshes cannot run through the backend's search limit. Unverified: the port's own number.
pub const BROWSE_AGAIN_MS: f64 = 2000.0;
/// A hosted public lobby's listing goes to the backend at most this often, the latest one winning; a failed update
/// is tried again after LIST_RETRY_MS. Unverified: the port's own numbers (EOS's lobby update limits are not known
/// here).
pub const LIST_GAP_MS: f64 = 2000.0;
pub const LIST_RETRY_MS: f64 = 10000.0;
/// A queue lobby's bucket: one per mode and compatibility id, so a search finds only games it may meet.
pub fn queue_bucket(mode: QueueMode, compat: &str) -> String {
    format!("mvs:queue:{}:{compat}", mode.id())
}
/// A queuer whose own lobbies have no other member searches for an older lobby this often. Unverified: the port's
/// own; EOS shows a new listed lobby to a search 2 to 3 s after its create, so two queuers who start within that may
/// each miss the other's, and the next search settles it.
pub const SEARCH_EVERY_MS: f64 = 3000.0;
/// The lobbies one search asks for, at the most. Unverified: the port's own number (a session only moves to an older
/// lobby, so the pairing still settles when the oldest is not among them).
pub const SEARCH_LIMIT: u32 = 10;
/// A guest's FRAME_OPEN goes again this often until the host's first message, for this long at the most (the
/// owner's process may hear of the new member after its link request: it refuses the request until it does).
pub const OPEN_EVERY_MS: f64 = 1000.0;
pub const OPEN_FOR_MS: f64 = 10000.0;
/// A link's end waits this long after its FRAME_BYE (or the lobby's close) before the link and the lobby go, so that
/// the messages before it arrive first. Unverified: the SDK's order of a lobby's notification against the P2P packets
/// sent before it is not known; this is the margin taken.
pub const BYE_MS: f64 = 500.0;
/// A member of a hosted lobby that has not linked (FRAME_OPEN) this long after the host heard of it is kicked from the
/// lobby. Unverified: the port's own number. Deviation (from the TS's 5000): the guest knocks for OPEN_FOR_MS, and an
/// Epic forced relay can take more than 5 s to carry its first knock, so the host waits out the whole window and a
/// margin.
pub const KNOCK_WAIT_MS: f64 = OPEN_FOR_MS + 2000.0;
/// A member that left a hosted lobby's link is not linked again for this long. Unverified: the port's own number,
/// under OPEN_FOR_MS less a knock.
pub const RELINK_MS: f64 = 5000.0;
/// Invites another player sent this one: one per inviter per INVITE_AGAIN_MS, INVITES_PER_MIN a minute from everyone
/// together; past either an invite is dropped. Unverified: the port's own numbers.
pub const INVITE_AGAIN_MS: f64 = 15000.0;
pub const INVITES_PER_MIN: f64 = 6.0;
/// Invites this player sends: one per invitee per INVITE_AGAIN_MS, INVITES_SENT_PER_MIN a minute in all (under the
/// relay's 20 a minute); past either the page is told (InviteRefused). Unverified: the port's own number.
pub const INVITES_SENT_PER_MIN: f64 = 8.0;
/// An away queue session goes home when the owner's core has not answered its join within QUEUE_STATE_WAIT_MS, or
/// sent no queue.found QUEUE_FOUND_GRACE_MS after this player's bot wait. Unverified: the port's own margins.
pub const QUEUE_STATE_WAIT_MS: f64 = 10000.0;
pub const QUEUE_FOUND_GRACE_MS: f64 = 15000.0;
/// A party's relay lobby: invite-only (never listed), PARTY_MAX places.
pub const PARTY_BUCKET: &str = "mvs:party";
/// A party's packets taken from each other player: PARTY_PER_S a second, PARTY_BURST at once. Unverified: the
/// port's own numbers.
pub const PARTY_PER_S: f64 = 5.0;
pub const PARTY_BURST: f64 = 10.0;
/// A member takes its leader's queue follows this often a minute at the most. Unverified: the port's own number.
pub const PARTY_FOLLOWS_PER_MIN: f64 = 6.0;
/// The party invites' codes kept with their inviters, the newest this many.
pub const PARTY_INVITES_KEPT: usize = 32;
/// A 'who' within this long of the last one's answer gets that answer again (the relay answers 12 a minute).
/// Unverified: the port's own number.
pub const WHO_AGAIN_MS: f64 = 5000.0;
/// This player's presence goes to a relay at most this often, the latest one winning; a refused one is tried again
/// PRESENCE_RETRY_MS later. Unverified: the port's own margin.
pub const PRESENCE_GAP_MS: f64 = 1100.0;
pub const PRESENCE_RETRY_MS: f64 = 3000.0;
/// A session's own link is let go this long after its end: its byes, then (BYE_MS later) its lobby's leave, go first.
/// Unverified: the port's own margin over BYE_MS.
pub const RELEASE_MS: f64 = BYE_MS + 200.0;
/// Epic's relays stay forced this long after a queue session on Epic ended. Unverified: the port's own margin.
pub const QUEUE_HOLD_MS: f64 = RELEASE_MS + 2000.0;
/// With LAN on, this PC's addresses are looked at this often (the launcher's line). Unverified: the port's own number.
pub const ADDRESSES_EVERY_MS: f64 = 5000.0;
/// A refereed match's Referee.pump runs this often on the service's clock (the deadlines and the clock packets; an
/// input it takes is passed on at once). Unverified: the port's own number.
pub const REFEREE_PUMP_MS: f64 = 4.0;

/// The wall clock: ms since 1970 (a queue lobby's `at`, which other PCs compare).
pub type Wall = Box<dyn Fn() -> f64 + Send>;

/// A host name's address (std's resolver by default; tests give their own).
pub type Lookup = Box<dyn Fn(&str) -> Result<String, String> + Send>;

/// A backend as the service holds it: shared with the host cores' links (a member's send is made inside the core's
/// call), each call made on the service's thread.
pub type Shared = Arc<Mutex<Box<dyn Backend>>>;

/// A shared backend's lock (a backend never panics inside a call of ours, so a poisoned lock is taken as it is).
pub fn lock(b: &Shared) -> MutexGuard<'_, Box<dyn Backend>> {
    b.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// How a service is made (sessions.ts ServiceOptions). The clock is the caller's `now`.
pub struct ServiceOptions {
    /// The wall clock (None: the system's).
    pub wall: Option<Wall>,
    pub log: Option<HostLog>,
    /// The host cores' waits (the tests' shorter ones).
    pub host_times: HostTimes,
    /// The port LAN lobbies are announced and heard on (None: DISCOVERY_PORT; tests give their own).
    pub discovery_port: Option<u16>,
    /// The host cores' draws, one per core made (None: the system's).
    pub host_rand: Option<Box<dyn FnMut() -> Box<dyn mvs_host::HostRand> + Send>>,
    /// A host name's address (None: the system's).
    pub lookup: Option<Lookup>,
    /// The clock's zero: `now` is ms since it (a relay link's client counts from it too).
    pub epoch: std::time::Instant,
    /// Where the service's thread writes the net state for the launcher (`api::NetState`; None: nowhere).
    pub state_file: Option<PathBuf>,
    /// A dev run's config (`--online-config`): its relay is a scratch one, the key it proves at a sign-in never
    /// recorded (`ServiceNote::RelayKnown`).
    pub dev: bool,
}

impl Default for ServiceOptions {
    fn default() -> Self {
        ServiceOptions {
            state_file: None,
            dev: false,
            wall: None,
            log: None,
            host_times: HostTimes::default(),
            discovery_port: None,
            host_rand: None,
            lookup: None,
            epoch: std::time::Instant::now(),
        }
    }
}

/// The config's values the sessions read (sessions.ts ServiceConfig): the config's relay, direct friend links
/// allowed, the release's default relay, the relays this player has said yes to.
#[derive(Clone, Debug, PartialEq)]
pub struct ServiceConfig {
    /// (dev) the faults the relay clients of this config run under.
    pub relay_faults: NetFaults,
    pub relay: Option<String>,
    pub allow_direct: bool,
    pub default_relay: Option<String>,
    pub known_relays: Vec<KnownRelay>,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        ServiceConfig {
            relay_faults: NetFaults::default(),
            relay: None,
            allow_direct: true,
            default_relay: None,
            known_relays: Vec::new(),
        }
    }
}

/// What the service tells its owner beside the pages' events (netproc.ts's messages to main).
#[derive(Clone, Debug, PartialEq)]
pub enum ServiceNote {
    /// The player said yes to a relay, a relay said yes to proved its key, or the config's relay signed in to without a
    /// pin proved one (not in a dev run, `ServiceOptions::dev`): main keeps it in Settings.online.
    RelayKnown(KnownRelay),
    /// The LAN host's state changed (None: it is off).
    Direct(Option<DirectStatus>),
    /// A backend's own state changed (signin.rs's line for it follows).
    Backend(BackendKind),
    /// A backend of the service signed out by itself (signin.rs takes it when it drives the service).
    SignedOut(BackendKind, String),
}

/// The service's own asks of a backend, waiting for their answers.
#[derive(Clone, Debug, PartialEq)]
enum SvcAsk {
    Search(BackendKind),
    Who,
    Presence {
        text: String,
        name: String,
        epoch: u32,
    },
    /// A session's invite: its page and the invitee.
    Invite {
        page: PageId,
        to: String,
    },
}

/// The service's own timers.
#[derive(Clone, Debug, PartialEq)]
enum SvcDue {
    /// A link of a session's own let go (RELEASE_MS after the session's end).
    Release(u32),
    /// The presence's gap is over (on that relay sign-in).
    Presence(u32),
    /// Epic's queue hold is over.
    QueueHold,
    /// The LAN listener stops (BROWSE_LISTEN_MS after the last browse).
    StopHearing,
    /// A browse waiting for the LAN listener may read it.
    Heard,
    /// This PC's addresses looked at again.
    Addresses,
}

/// A relay-hosted session's match path while it wants its Epic links (`RelayHosted::path_open`), as the service last
/// saw it: Epic's relay control counts it as a session open on Epic (`NetService::relay_check`).
#[derive(Clone, Debug, PartialEq)]
struct PathWay {
    /// The links may be direct (a custom lobby whose humans all said so); false: forced through Epic's relays, as a
    /// queue's.
    direct: bool,
    /// The Epic ids it links to.
    peers: Vec<String>,
}

/// What a link of a session's own is, for its loss and its release.
pub enum OwnKind {
    /// A direct host's own player (its listener closed at the release).
    DirectHost(Box<crate::lan::direct::DirectHost>),
    /// A direct code's join.
    DirectJoin,
    /// A relay code's own relay link, by its address (normalRelay), and the id of the key the relay proved: shared by
    /// every session on that relay.
    Relay(String, String),
}

/// A link of a session's own, by its number.
pub struct OwnLink {
    pub backend: Shared,
    pub users: u32,
    pub kind: OwnKind,
}

/// A search of one backend for a browse: when it was answered (None: still out) and its answer.
struct Searching {
    at: Option<f64>,
    answer: Option<Result<Vec<FoundLobby>, String>>,
}

/// Why a lobby's code cannot be written (sessions.ts codeOn's throw).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CodeOnWhy {
    /// 'the relay's address is not in the online config'.
    NoRelay,
    /// encode_lobby_code's why.
    Encode(CodeMakeWhy),
}

impl CodeOnWhy {
    /// The why for the log.
    pub(crate) fn log_text(&self) -> String {
        match self {
            CodeOnWhy::NoRelay => "the relay's address is not in the online config".to_string(),
            CodeOnWhy::Encode(e) => e.to_string(),
        }
    }

    /// A custom lobby's (or a party's, `party`) end for it.
    pub(crate) fn closed(self, party: bool) -> ClosedWhy {
        match (self, party) {
            (CodeOnWhy::NoRelay, false) => ClosedWhy::MakeNoRelay,
            (CodeOnWhy::NoRelay, true) => ClosedWhy::PartyMakeNoRelay,
            (CodeOnWhy::Encode(why), false) => ClosedWhy::MakeCodeFailed { why },
            (CodeOnWhy::Encode(why), true) => ClosedWhy::PartyMakeCodeFailed { why },
        }
    }
}

/// A browse waiting for its backends' searches and the LAN listener.
struct Browse {
    page: PageId,
    id: u32,
    kinds: Vec<BackendKind>,
    /// The LAN listener may be read from then (None: LAN is off).
    heard_at: Option<f64>,
}

/// A who waiting for the relay's answer.
struct WhoWait {
    page: PageId,
    id: u32,
}

/// This player's presence on the relay signed in: its epoch (a sign-in's number), the text it has, a set out.
#[derive(Default)]
struct Published {
    epoch: u32,
    has: Option<String>,
    busy: bool,
}

/// The LAN host on: its options, the install's id and its key's fingerprint.
struct LanOn {
    o: LanOptions,
    me: String,
    own: String,
}

/// The net service.
pub struct NetService {
    pub(crate) o: ServiceOptions,
    /// The signed-in backends (Epic's, the relay's); `signing` those still signing in.
    pub(crate) backends: IndexMap<BackendKind, Shared>,
    lan: Option<LanOn>,
    heard: Option<LanListener>,
    heard_since: f64,
    heard_stop: Option<TimerId>,
    /// Why each backend told of is off (None while on), in the order they were told.
    whys: IndexMap<BackendKind, Option<OffWhy>>,
    pub(crate) direct: Option<DirectOptions>,
    pub(crate) config: ServiceConfig,
    pub(crate) pages: IndexMap<PageId, Page>,
    page_n: u32,
    /// Sessions and parties over, kept while timers of their own are set.
    retired: Vec<Session>,
    retired_parties: Vec<Party>,
    pub(crate) own: IndexMap<u32, OwnLink>,
    own_n: u32,
    /// The hosted LAN lobbies' listeners (their own link's number, `address:port`), and why the last one tried could
    /// not listen.
    listeners: IndexMap<u32, String>,
    listen_why: Option<HostStartWhy>,
    last_direct: Option<Option<DirectStatus>>,
    address_timer: Option<TimerId>,
    timers: Timers<SvcDue>,
    asks_out: IndexMap<(Via, Ask), SvcAsk>,
    browsing: IndexMap<BackendKind, Searching>,
    browses: Vec<Browse>,
    whoing: Option<Searching>,
    who_answer: Option<Result<Vec<Who>, String>>,
    whos: Vec<WhoWait>,
    /// The last question's number (NetEvent Ask).
    asks: u32,
    on_epic: IndexSet<PageId>,
    epic_mode: Option<RelayMode>,
    direct_queue_peers: IndexMap<PageId, IndexSet<String>>,
    /// The relay-hosted sessions' match paths that want their Epic links, by page.
    paths: IndexMap<PageId, PathWay>,
    queue_hold: f64,
    relay_check_queued: bool,
    /// The events of the pages the shutdown let go, until their owner takes them.
    gone_events: Vec<(PageId, Vec<NetEvent>)>,
    invites_in: IndexMap<String, f64>,
    invites_out: IndexMap<String, f64>,
    invite_in_budget: Option<Bucket>,
    invite_out_budget: Option<Bucket>,
    presence_now: Option<RelayPresence>,
    /// The fighter this player's main menu shows, as the page last sent it (the parties' cards carry it).
    fighter_now: Option<MenuFighter>,
    name_taken: String,
    party_invites: IndexMap<String, String>,
    published: Published,
    relay_epoch: u32,
    notes: Vec<ServiceNote>,
    accepts: IndexMap<Via, IndexSet<String>>,
    name_refused: Profanity,
    /// signin.rs drives the service: a backend's own sign-out is its to take.
    pub(crate) signin: bool,
    /// The links of its own to relays under way, each shared by the joins to that relay meanwhile; the last ticket's
    /// number.
    relay_connects: Vec<RelayConnect>,
    connect_n: u32,
    /// The way to the relay as last told the pages (`NetEvent::Way`): slower, by TCP alone.
    slow: bool,
    /// The signed-in relay's cores run another version of the game's protocol (the one its sign-in's welcome named,
    /// else the one a core's first message named): custom games and the queue skip it until the relay's sign-in or its
    /// address changes.
    relay_other: Option<String>,
    /// The pages whose session left a relay of another version: their open, taken on to the next connection once the
    /// session is back out of the call.
    reopens: Vec<(PageId, SessionTarget)>,
    /// Backends handed over while a session was open (`NetHandle::backends_when_free`), taken once none is.
    held_backends: Option<(Box<OnlineBackends>, DirectOptions)>,
}

/// A link of its own to a relay under way (sessions.ts relayLinks' entry before its link is up): every join to that
/// relay meanwhile waits on it, as a relay takes one sign-in per key.
struct RelayConnect {
    /// The relay's address (normalRelay).
    key: String,
    p: Pending<Result<RelayClient, RelayError>>,
    /// Its outcome once over: the link's number and the id of the key the relay proved, or the relay's refusal.
    done: Option<Result<(u32, String), String>>,
    /// The joins waiting on it, by their ticket.
    waiting: Vec<u32>,
}

/// The words of a backend's name in the logs.
fn name_of(kind: BackendKind) -> &'static str {
    match kind {
        BackendKind::Epic => "Epic",
        BackendKind::Relay => "the relay",
        BackendKind::Lan => "LAN",
    }
}

/// The backend refused a request for its rate (a relay's, EOS's own result code).
pub(crate) fn limited(t: &str) -> bool {
    t.contains("LimitExceeded")
}

/// An icon as a presence carries it: a ProfileIconData asset name or ''.
pub(crate) fn icon_ok(s: &str) -> bool {
    s.len() <= 96 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl NetService {
    pub fn new(o: ServiceOptions, now: f64) -> NetService {
        let _ = now;
        NetService {
            o,
            backends: IndexMap::new(),
            lan: None,
            heard: None,
            heard_since: 0.0,
            heard_stop: None,
            whys: IndexMap::new(),
            direct: None,
            config: ServiceConfig::default(),
            pages: IndexMap::new(),
            page_n: 0,
            retired: Vec::new(),
            retired_parties: Vec::new(),
            own: IndexMap::new(),
            own_n: 0,
            listeners: IndexMap::new(),
            listen_why: None,
            last_direct: None,
            address_timer: None,
            timers: Timers::default(),
            asks_out: IndexMap::new(),
            browsing: IndexMap::new(),
            browses: Vec::new(),
            whoing: None,
            who_answer: None,
            whos: Vec::new(),
            asks: 0,
            on_epic: IndexSet::new(),
            epic_mode: None,
            direct_queue_peers: IndexMap::new(),
            paths: IndexMap::new(),
            queue_hold: f64::NEG_INFINITY,
            relay_check_queued: false,
            gone_events: Vec::new(),
            invites_in: IndexMap::new(),
            invites_out: IndexMap::new(),
            invite_in_budget: None,
            invite_out_budget: None,
            presence_now: None,
            fighter_now: None,
            name_taken: String::new(),
            party_invites: IndexMap::new(),
            published: Published::default(),
            relay_epoch: 0,
            notes: Vec::new(),
            accepts: IndexMap::new(),
            name_refused: Profanity::new(&BAD_WORDS),
            signin: false,
            relay_connects: Vec::new(),
            connect_n: 0,
            slow: false,
            relay_other: None,
            reopens: Vec::new(),
            held_backends: None,
        }
    }

    // ---------- the pages ----------

    /// A page's port: told the status at once (its first event).
    pub fn open_page(&mut self, now: f64) -> PageId {
        self.page_n += 1;
        let id = PageId(self.page_n - 1);
        self.pages.insert(id, Page::new(id));
        let st = self.status();
        self.post(id, st);
        if self.slow {
            self.post(id, NetEvent::Way { slow: true });
        }
        self.settle(now);
        id
    }

    /// The page is gone: its session and party end as on its `Close`.
    pub fn close_page(&mut self, page: PageId, now: f64) {
        if !self.pages.contains_key(&page) {
            return;
        }
        self.with_session(page, now, |svc, s| s.end(svc, ClosedWhy::Left, now));
        self.with_party(page, now, |svc, p| p.end(svc, ClosedWhy::Left, now));
        if let Some(mut p) = self.pages.shift_remove(&page) {
            if let Some(s) = p.session.take() {
                self.retire(s);
            }
            if let Some(x) = p.party.take() {
                self.retire_party(x);
            }
        }
        self.settle(now);
    }

    /// The events for a page since the last call, in order.
    pub fn events(&mut self, page: PageId) -> Vec<NetEvent> {
        if let Some(i) = self.gone_events.iter().position(|g| g.0 == page) {
            return self.gone_events.swap_remove(i).1;
        }
        self.pages.get_mut(&page).map(|p| std::mem::take(&mut p.events)).unwrap_or_default()
    }

    /// What the service told its owner since the last call.
    pub fn take_notes(&mut self) -> Vec<ServiceNote> {
        std::mem::take(&mut self.notes)
    }

    /// A page's request (sessions.ts Page.take).
    pub fn take(&mut self, page: PageId, m: NetRequest, now: f64) {
        if !self.pages.contains_key(&page) {
            return;
        }
        match m {
            NetRequest::Open(target) => {
                self.with_session(page, now, |svc, s| s.end(svc, ClosedWhy::Replaced, now));
                self.open(page, target, now);
            }
            NetRequest::Msg(text) => {
                self.with_session(page, now, |svc, s| s.from_page(svc, &text, now));
            }
            NetRequest::Close => {
                self.with_session(page, now, |svc, s| s.end(svc, ClosedWhy::Left, now));
            }
            NetRequest::Match { peers, slot, referee } => {
                let ok = peers.len() <= MAX_SLOTS && (slot as usize) < peers.len() && !referee.is_empty();
                if ok {
                    let mut m = MatchRoute { peers, slot, referee: referee.clone(), on: None };
                    let on = self.with_session(page, now, |svc, s| {
                        m.on = svc.match_on(s, &m.referee);
                        let on = m.on;
                        s.set_match(svc, Some(m));
                        on
                    });
                    // (a link to the referee app made before the match, while the path measured, was told to no
                    // page: Epic's word on it now)
                    let relayed = match on.flatten() {
                        Some(Via::Backend(k)) => self.eos_of(k).and_then(|b| lock(&b).relayed(&referee)),
                        _ => None,
                    };
                    if let Some(r) = relayed {
                        self.post(page, NetEvent::Link { peer: referee, relayed: Some(r) });
                    }
                }
            }
            NetRequest::Send(bytes) => {
                self.with_session(page, now, |svc, s| s.send_match(svc, &bytes, now));
            }
            NetRequest::MatchEnd => {
                self.with_session(page, now, |svc, s| s.set_match(svc, None));
            }
            NetRequest::Invite { to, name } => {
                self.with_session(page, now, |svc, s| s.base().invite(svc, &to, &name, now));
            }
            NetRequest::Browse { id } => self.browse(page, id, now),
            NetRequest::Answer { id, yes } => {
                let a = self.pages.get_mut(&page).and_then(|p| p.asks.shift_remove(&id));
                if let Some(AskFor::Relay) = a {
                    self.with_session(page, now, |svc, s| s.asked(svc, yes, now));
                }
            }
            NetRequest::Presence(p) => self.set_presence(&p, now),
            NetRequest::Who { id } => self.who(page, id, now),
            NetRequest::PartyInvite { to, name } => self.party_invite(page, &to, &name, now),
            NetRequest::PartyJoin { code } => self.party_join(page, &code, now),
            NetRequest::PartyLeave => {
                self.with_party(page, now, |svc, p| p.end(svc, ClosedWhy::Left, now));
            }
            NetRequest::PartySay(say) => {
                self.with_party(page, now, |svc, p| p.say(svc, say, now));
            }
            NetRequest::MenuFighter(f) => self.set_menu_fighter(f.as_ref(), now),
        }
        self.settle(now);
    }

    /// sessions.ts Page.open.
    fn open(&mut self, page: PageId, target: SessionTarget, now: f64) {
        let host = match target {
            SessionTarget::Join(code) => return self.join(page, &code, now),
            SessionTarget::Host => true,
            SessionTarget::Queue => false,
        };
        let epic = self.eos_of(BackendKind::Epic).is_some();
        let signed = self.eos_of(BackendKind::Relay).is_some();
        // (a relay whose cores run another version of the game's protocol is skipped: `RelayHosted::skip`)
        let other = self.relay_other.clone().filter(|_| signed);
        let relay = signed && other.is_none();
        let lan = self.lan.is_some();
        let other_text = other.as_ref().map(|p| format!("it runs protocol {p} (this game {PROTOCOL})"));
        if host {
            // (the relay first, which hosts the lobby's core itself and lets the match's path go over Epic where that
            // is shorter; then Epic: direct links and the host's own core and referee; then LAN, whose direct host's
            // relay core hosts it as the relay's does; then a local lobby, this PC's alone)
            // Deviation: online plan v4 section 5 turns v3's order (Epic first) round; the TS hosts on Epic first.
            let relay_why = if relay { self.relay_host_why() } else { None };
            if let Some(w) = relay_why.as_ref().map(CodeOnWhy::log_text).or(other_text) {
                let then = if epic {
                    "hosted on Epic"
                } else if lan {
                    "hosted on LAN"
                } else {
                    "hosted locally"
                };
                self.log(&format!("custom: not hosted on the relay: {w}; {then}"));
            }
            let s = if relay && relay_why.is_none() {
                let via = Via::Backend(BackendKind::Relay);
                Session::Hosted(Box::new(RelayHosted::open_custom(self, page, via, now)))
            } else if epic {
                Session::Host(Box::new(HostSession::open(self, page, BackendKind::Epic, now)))
            } else if lan {
                let o = self.lan.as_ref().map(|l| l.o.clone()).expect("LAN is on");
                Session::Hosted(Box::new(RelayHosted::open_lan(page, &o)))
            } else {
                let why =
                    if other.is_some() { LocalWhy::RelayVersion } else { LocalWhy::Status { why: self.status_why() } };
                Session::Host(Box::new(HostSession::open_local(self, page, why, now)))
            };
            return self.place(page, s, now);
        }
        // (the relay first: its one queue core holds every queuer; never LAN)
        if let Some(w) = other_text {
            let then = if epic { "on Epic" } else { "not queued" };
            self.log(&format!("queue: not on the relay: {w}; {then}"));
        }
        if relay {
            let s = RelayHosted::open_queue(self, page, now);
            self.place(page, Session::Hosted(Box::new(s)), now);
        } else if let Some(e) = self.eos_of(BackendKind::Epic) {
            match self.set_epic_mode(&e, RelayMode::Force) {
                Err(why) => self.post(page, NetEvent::Closed(ClosedWhy::NotRelayed { text: why })),
                Ok(()) => {
                    let s = QueueSession::open(self, page, BackendKind::Epic, now);
                    self.place(page, Session::Queue(s), now);
                }
            }
        } else if other.is_some() {
            self.post(page, NetEvent::Closed(ClosedWhy::RelayVersion));
        } else if lan {
            self.post(page, NetEvent::Closed(ClosedWhy::NoQueue));
        } else {
            let why = self.status_why();
            self.post(page, NetEvent::Closed(ClosedWhy::Offline { why }));
        }
    }

    /// sessions.ts Page.join: a join goes through the backend its code names, whatever else is signed in.
    fn join(&mut self, page: PageId, text: &str, now: f64) {
        let fail = |svc: &mut NetService, why: JoinWhy| svc.post(page, NetEvent::Closed(ClosedWhy::JoinFailed { why }));
        let c = match decode_lobby_code(text, self.config.default_relay.as_deref()) {
            Ok(c) => c,
            Err(e) => return fail(self, JoinWhy::BadCode { why: e }),
        };
        let code = direct_text(text);
        match c {
            // (a hosted lobby on the release's relay, named by its short code: the relay reads it at the join, and
            // the answer names the lobby)
            LobbyCode::Short { code: short } => {
                let Some(address) = self.config.default_relay.clone().filter(|d| !d.is_empty()) else {
                    return fail(self, JoinWhy::NoDefaultRelay);
                };
                if self.config.relay.is_some() && !self.on_release_relay() {
                    return fail(self, JoinWhy::ShortOtherRelay);
                }
                if self.config.relay.is_none() || self.eos_of(BackendKind::Relay).is_none() {
                    let why = self.off_why(BackendKind::Relay);
                    let address = normal_relay(&address).unwrap_or(address);
                    return fail(self, JoinWhy::RelayOff { address, why });
                }
                let via = GuestVia::Short { code: short };
                let s = GuestSession::open(self, page, Some(BackendKind::Relay), &code, via, now);
                self.place(page, Session::Guest(s), now);
            }
            LobbyCode::Direct { .. } => {
                let Some(o) = self.direct.clone() else { return fail(self, JoinWhy::NoDirect) };
                let s = GuestSession::open(self, page, None, &code, GuestVia::Direct { o }, now);
                self.place(page, Session::Guest(s), now);
            }
            LobbyCode::Epic { lobby } => {
                if self.eos_of(BackendKind::Epic).is_none() {
                    let why = self.off_why(BackendKind::Epic);
                    return fail(self, JoinWhy::EpicOff { why });
                }
                let s = GuestSession::open(self, page, Some(BackendKind::Epic), &code, GuestVia::On { lobby }, now);
                self.place(page, Session::Guest(s), now);
            }
            LobbyCode::Relay { lobby, relay } => {
                let Some(address) = relay.or_else(|| self.config.default_relay.clone()) else {
                    return fail(self, JoinWhy::NoDefaultRelay);
                };
                if same_relay(Some(&address), self.config.relay.as_deref()) {
                    if self.eos_of(BackendKind::Relay).is_none() {
                        let why = self.off_why(BackendKind::Relay);
                        let address = normal_relay(&address).unwrap_or(address);
                        return fail(self, JoinWhy::RelayOff { address, why });
                    }
                    let s =
                        GuestSession::open(self, page, Some(BackendKind::Relay), &code, GuestVia::On { lobby }, now);
                    return self.place(page, Session::Guest(s), now);
                }
                let Some(o) = self.direct.clone() else { return fail(self, JoinWhy::NoRelayLink) };
                let ask = !self.relay_known(&address);
                let address = normal_relay(&address).unwrap_or(address);
                let s = GuestSession::open(self, page, None, &code, GuestVia::Relay { address, o, lobby, ask }, now);
                self.place(page, Session::Guest(s), now);
            }
        }
    }

    /// A page's party invite (sessions.ts Page.partyInvite).
    fn party_invite(&mut self, page: PageId, to: &str, name: &str, now: f64) {
        let relay = self.eos_of(BackendKind::Relay).is_some();
        let (has, leading) = match self.pages.get(&page).and_then(|p| p.party.as_ref()) {
            Some(p) => (true, p.leading()),
            None => (false, false),
        };
        if to.is_empty() || (has && !leading) || (!has && !relay) {
            return self.post(page, NetEvent::InviteRefused { to: to.to_string() });
        }
        if !has {
            let p = Party::lead(self, page, now);
            if let Some(pg) = self.pages.get_mut(&page) {
                pg.party = Some(p);
            }
        }
        self.with_party(page, now, |svc, p| p.invite(svc, to, name, now));
    }

    /// An accepted party invite (sessions.ts Page.partyJoin).
    fn party_join(&mut self, page: PageId, text: &str, now: f64) {
        let code = direct_text(text);
        if self.pages.get(&page).and_then(|p| p.party.as_ref()).is_some_and(|p| p.code() == code) {
            return;
        }
        self.with_party(page, now, |svc, p| p.end(svc, ClosedWhy::Left, now));
        let fail = |svc: &mut NetService, why: PartyJoinWhy| {
            svc.post(page, NetEvent::Party { party: None, why: ClosedWhy::PartyJoinFailed { why } });
        };
        let inviter = self.party_inviter(&code);
        if inviter.is_empty() {
            return fail(self, PartyJoinWhy::NoInvite);
        }
        let c = match decode_lobby_code(text, self.config.default_relay.as_deref()) {
            Ok(c) => c,
            Err(why) => return fail(self, PartyJoinWhy::BadCode { why }),
        };
        let LobbyCode::Relay { lobby, relay } = c else { return fail(self, PartyJoinWhy::NotRelay) };
        let address = relay.or_else(|| self.config.default_relay.clone());
        if !address.as_deref().is_some_and(|a| same_relay(Some(a), self.config.relay.as_deref())) {
            let address = address.map(|a| normal_relay(&a).unwrap_or(a));
            return fail(self, PartyJoinWhy::OtherRelay { address });
        }
        if self.eos_of(BackendKind::Relay).is_none() {
            let why = self.off_why(BackendKind::Relay);
            return fail(self, PartyJoinWhy::RelayOff { why });
        }
        let p = Party::joining(self, page, &lobby, &code, &inviter, now);
        if let Some(pg) = self.pages.get_mut(&page) {
            pg.party = Some(p);
        }
    }

    /// Asks the player (NetEvent Ask) for the page's session; its answer comes to the session's `asked`.
    pub(crate) fn ask(&mut self, page: PageId, what: api::AskWhat) {
        self.asks += 1;
        let id = self.asks;
        if let Some(p) = self.pages.get_mut(&page) {
            p.asks.insert(id, AskFor::Relay);
        }
        self.post(page, NetEvent::Ask { id, what });
    }

    // ---------- a session or a party out of its page for a call ----------

    /// Runs `f` on the page's session taken out of its page, then puts it back (or aside, when it ended).
    pub(crate) fn with_session<R>(
        &mut self,
        page: PageId,
        now: f64,
        f: impl FnOnce(&mut NetService, &mut Session) -> R,
    ) -> Option<R> {
        let mut s = self.pages.get_mut(&page)?.session.take()?;
        let r = f(self, &mut s);
        let notes = s.take_notes();
        self.put_back(page, s, now);
        self.party_notes(page, notes, now);
        Some(r)
    }

    /// A session's notes to its page's party (not generic: with_session and with_party call each other).
    fn party_notes(&mut self, page: PageId, notes: Vec<party::PartyNote>, now: f64) {
        for n in notes {
            self.with_party(page, now, |svc, p| p.note(svc, n, now));
        }
    }

    /// A party's notes to its page's session.
    fn session_notes(&mut self, page: PageId, notes: Vec<party::SessionNote>, now: f64) {
        for n in notes {
            self.with_session(page, now, |svc, s| s.note(svc, n, now));
        }
    }

    /// Runs `f` on the page's party taken out of its page, then puts it back (or aside, when it is over).
    pub(crate) fn with_party<R>(
        &mut self,
        page: PageId,
        now: f64,
        f: impl FnOnce(&mut NetService, &mut Party) -> R,
    ) -> Option<R> {
        let mut p = self.pages.get_mut(&page)?.party.take()?;
        let r = f(self, &mut p);
        let notes = p.take_notes();
        if p.over() {
            self.retire_party(p);
        } else if let Some(pg) = self.pages.get_mut(&page) {
            pg.party = Some(p);
        } else {
            self.retire_party(p);
        }
        self.session_notes(page, notes, now);
        Some(r)
    }

    /// A session made for a page goes in it (one over already is kept aside).
    fn place(&mut self, page: PageId, s: Session, now: f64) {
        self.put_back(page, s, now);
    }

    /// The session goes back in its page (or aside, when it ended or the page has another); its match path's Epic
    /// links as they are now go to Epic's relay control (`path_seen`).
    fn put_back(&mut self, page: PageId, mut s: Session, now: f64) {
        if let Some(via) = s.base().via
            && let Some(b) = self.backend(via)
        {
            s.base_mut().held = Some(session::Held(via, b));
        }
        let way = s
            .path()
            .filter(|_| !s.base().closed)
            .and_then(|p| p.open_as().map(|direct| PathWay { direct, peers: p.epic_peers() }));
        let placed = !s.base().closed && self.pages.get(&page).is_some_and(|p| p.session.is_none());
        if placed {
            if let Some(p) = self.pages.get_mut(&page) {
                p.session = Some(s);
            }
        } else {
            self.retire(s);
        }
        self.path_seen(page, way.filter(|_| placed), now);
        // (a session that left a relay of another version: the page's open on the next connection)
        if let Some(i) = self.reopens.iter().position(|r| r.0 == page) {
            let (_, target) = self.reopens.remove(i);
            if self.pages.get(&page).is_some_and(|p| p.session.is_none()) {
                self.open(page, target, now);
            }
        }
    }

    /// The session of `page` left a relay of another version: its open goes on to the next connection once it is
    /// back out of its call.
    pub(crate) fn reopen(&mut self, page: PageId, target: SessionTarget) {
        self.reopens.push((page, target));
    }

    /// A relay's core named another version of the game's protocol (`proto`, as it wrote it): the relay is skipped
    /// until its sign-in or its address changes.
    pub(crate) fn relay_other_version(&mut self, proto: &str) {
        self.relay_other = Some(proto.to_string());
    }

    /// The game protocol the signed-in relay's cores run when it is another than this game's (its sign-in's welcome
    /// said it, or a core's first message did); None: this game's, not known yet, not a number, or no relay signed
    /// in.
    pub fn relay_other_proto(&self) -> Option<u32> {
        self.relay_other.as_deref().filter(|_| self.eos_of(BackendKind::Relay).is_some()).and_then(|p| p.parse().ok())
    }

    fn retire(&mut self, s: Session) {
        if s.next_due().is_some() {
            self.retired.push(s);
        }
    }

    fn retire_party(&mut self, p: Party) {
        if p.next_due().is_some() {
            self.retired_parties.push(p);
        }
    }

    /// The pages whose open sessions are on `via`, in order.
    fn pages_on(&self, via: Via) -> Vec<PageId> {
        self.pages
            .values()
            .filter(|p| p.session.as_ref().is_some_and(|s| s.base().via == Some(via)))
            .map(|p| p.id)
            .collect()
    }

    fn parties_on(&self, via: Via) -> Vec<PageId> {
        self.pages.values().filter(|p| p.party.as_ref().is_some_and(|x| x.via() == via)).map(|p| p.id).collect()
    }

    /// The way a match under `referee` goes when it is not the session's own: Epic, for a session with a match path
    /// whose referee is one of the path's humans or this player's app (match.go names its Epic id).
    fn match_on(&self, s: &Session, referee: &str) -> Option<Via> {
        let path = s.path()?;
        let epic = Via::Backend(BackendKind::Epic);
        let me = self.me_on(epic);
        let on = path.epic_peers().iter().any(|e| e == referee) || (!me.is_empty() && me == referee);
        on.then_some(epic)
    }

    /// The pages whose session's match path links to `peer` on Epic, in order.
    fn path_pages(&self, peer: &str) -> Vec<PageId> {
        // (the paths that want their links, as last seen: none, the common case, costs nothing)
        self.paths
            .keys()
            .copied()
            .filter(|id| match self.pages.get(id).and_then(|p| p.session.as_ref()) {
                Some(s) => !s.base().closed && s.path().is_some_and(|p| p.epic_peers().iter().any(|e| e == peer)),
                None => false,
            })
            .collect()
    }

    /// Epic's word on a link to `peer` (None: it went down) to the match paths that link to it; the page whose match
    /// that peer referees over Epic is told, as a page on Epic is of its referee's link.
    fn path_link(&mut self, peer: &str, relayed: Option<bool>, now: f64) {
        for p in self.path_pages(peer) {
            let told = self.with_session(p, now, |_, s| {
                let Some(path) = s.path_mut() else { return false };
                path.link(peer, relayed);
                s.base().match_route.as_ref().is_some_and(|m| m.on.is_some() && m.referee == peer)
            });
            if told == Some(true) {
                self.post(p, NetEvent::Link { peer: peer.to_string(), relayed });
            }
        }
    }

    /// The session of `page` is still on `via`.
    fn on_via(&self, page: PageId, via: Via) -> bool {
        self.pages.get(&page).and_then(|p| p.session.as_ref()).is_some_and(|s| s.base().via == Some(via))
    }

    // ---------- the backends, all at once ----------

    /// A backend signed in (signin.rs): its sessions on the old one, if any, end first.
    pub fn start_backend(&mut self, kind: BackendKind, b: Box<dyn Backend>, now: f64) {
        self.start_shared(kind, Arc::new(Mutex::new(b)), now);
    }

    /// As start_backend, for a backend its sign-in keeps a handle on (to shut it down).
    pub fn start_shared(&mut self, kind: BackendKind, b: Shared, now: f64) {
        if kind == BackendKind::Lan {
            return self.log("startBackend: LAN is started by startLan");
        }
        let had = self.backends.contains_key(&kind);
        if had {
            self.end_on(kind, OffWhy::SignInChanged, now);
            self.forget_backend(kind);
        }
        self.backends.insert(kind, b);
        self.accepts.shift_remove(&Via::Backend(kind));
        self.signed_in(kind, now);
        if had {
            self.ask_again(now);
        }
        self.settle(now);
    }

    fn signed_in(&mut self, kind: BackendKind, now: f64) {
        self.whys.insert(kind, None);
        // (not taken as known: relay_check sets it on the platform)
        if kind == BackendKind::Epic {
            self.epic_mode = None;
            self.relay_check(now);
        }
        // (a new sign-in knows nothing of this player's presence, and may be a relay redeployed: its version is the
        // one its welcome names, this game's or another; a relay that names none is tried again, its first lobby's
        // core saying it)
        if kind == BackendKind::Relay {
            let proto = self.backends.get(&kind).and_then(|b| lock(b).relay_proto());
            self.relay_other = proto.filter(|p| *p != PROTOCOL).map(|p| p.to_string());
            if let Some(p) = &self.relay_other {
                self.log(&format!("relay: signed in, its cores run protocol {p} (this game {PROTOCOL}): not used"));
            }
            self.relay_epoch += 1;
            self.published = Published { epoch: self.relay_epoch, has: None, busy: false };
            self.publish(now);
        }
        self.tell_status();
        self.notes.push(ServiceNote::Backend(kind));
    }

    /// A backend is off (it did not sign in, dropped, or left the config): its sessions end with `why`.
    pub fn stop_backend(&mut self, kind: BackendKind, why: OffWhy, now: f64) {
        if kind == BackendKind::Lan {
            return self.stop_lan(why, now);
        }
        self.backends.shift_remove(&kind);
        self.accepts.shift_remove(&Via::Backend(kind));
        self.whys.insert(kind, Some(why.clone()));
        self.end_on(kind, why, now);
        self.forget_backend(kind);
        self.ask_again(now);
        self.tell_status();
        self.notes.push(ServiceNote::Backend(kind));
        self.settle(now);
    }

    /// The backend of `kind` is gone (stopped, or another sign-in in its place): what was asked of it is let go, the
    /// service's own (searches, the online list, the presence, invites) and the sessions' and parties' (sessions.ts
    /// keeps them per Eos object, which goes with it). Its answers never come, and a new sign-in counts its asks from
    /// the start again: none of them may meet an old ask's owner.
    fn forget_backend(&mut self, kind: BackendKind) {
        let via = Via::Backend(kind);
        let mut refused = Vec::new();
        self.asks_out.retain(|(v, _), a| {
            if *v != via {
                return true;
            }
            if let SvcAsk::Invite { page, to } = a {
                refused.push((*page, to.clone()));
            }
            false
        });
        // (an invite out: refused, as the old backend's rejection tells the page whose session is still open)
        for (page, to) in refused {
            let open = self.pages.get(&page).and_then(|p| p.session.as_ref()).is_some_and(|s| !s.base().closed);
            if open {
                self.post(page, NetEvent::InviteRefused { to });
            }
        }
        self.browsing.shift_remove(&kind);
        if kind == BackendKind::Relay {
            self.whoing = None;
            self.who_answer = None;
        }
        for p in self.pages.values_mut() {
            if let Some(s) = p.session.as_mut() {
                s.forget(via);
            }
            if let Some(x) = p.party.as_mut() {
                x.forget(via);
            }
        }
        for s in &mut self.retired {
            s.forget(via);
        }
        for x in &mut self.retired_parties {
            x.forget(via);
        }
    }

    /// The browses and online lists that waited on a backend gone are asked again of the backends on now, in order:
    /// one with none left on fails as a new one would.
    // Deviation: sessions.ts's browse and who wait on the old Eos object's promise, which its shutdown rejects: a
    // browse lists that backend as failed with the SDK's words, a who fails with them. Here a browse leaves the
    // stopped backend out (the status says it is off), and both ask the backend signed in in its place.
    fn ask_again(&mut self, now: f64) {
        for b in std::mem::take(&mut self.browses) {
            self.browse(b.page, b.id, now);
        }
        for w in std::mem::take(&mut self.whos) {
            self.who(w.page, w.id, now);
        }
    }

    /// The backend of `kind` as signed in, to shut down after its sessions ended (signin.rs).
    pub fn take_backend(&mut self, kind: BackendKind) -> Option<Shared> {
        self.backends.get(&kind).cloned()
    }

    /// The LAN host on (its options and port); off, with the reason, when the install's key cannot be read.
    pub fn start_lan(&mut self, o: LanOptions, now: f64) {
        if self.lan.as_ref().is_some_and(|l| l.o == o) {
            return;
        }
        // (other options still in use: their hosted lobbies end first, the listeners with them)
        if self.lan.is_some() {
            self.end_on(BackendKind::Lan, OffWhy::LanSettingChanged, now);
        }
        let k = match crate::link::relay::key::RelayKey::load(&o.direct.key_file) {
            Ok(k) => k,
            Err(e) => return self.stop_lan(OffWhy::Backend { text: e.to_string() }, now),
        };
        let (me, own) = (k.id().to_string(), crate::link::relay::key::key_fingerprint(k.raw()));
        self.stop_hearing();
        self.lan = Some(LanOn { o, me, own });
        self.whys.insert(BackendKind::Lan, None);
        self.listen_why = None;
        if self.address_timer.is_none() {
            self.address_timer = Some(self.timers.every(now, ADDRESSES_EVERY_MS, SvcDue::Addresses));
        }
        self.tell_direct();
        self.tell_status();
        self.notes.push(ServiceNote::Backend(BackendKind::Lan));
        self.settle(now);
    }

    /// The LAN host off: its hosted lobbies end with `why`.
    pub fn stop_lan(&mut self, why: OffWhy, now: f64) {
        self.lan = None;
        self.stop_hearing();
        self.whys.insert(BackendKind::Lan, Some(why.clone()));
        if let Some(t) = self.address_timer.take() {
            self.timers.cancel(t);
        }
        self.end_on(BackendKind::Lan, why, now);
        self.tell_direct();
        self.tell_status();
        self.notes.push(ServiceNote::Backend(BackendKind::Lan));
        self.settle(now);
    }

    /// The config's values the sessions read, on each change.
    pub fn configure(&mut self, c: ServiceConfig, now: f64) {
        if c.relay != self.config.relay {
            self.relay_other = None;
        }
        self.config = c;
        // (turned off: forced from the next link on; turned on: for the friend games opened from now on)
        self.relay_check(now);
        self.settle(now);
    }

    /// A custom lobby or a queue is open (or a page's open is on its way to the next connection): a change of the
    /// backends then waits (`hold_backends`).
    pub fn session_open(&self) -> bool {
        !self.reopens.is_empty() || self.pages.values().any(|p| p.session.is_some())
    }

    /// Backends the player changed while the game runs: given back to be set now when no session is open, else
    /// held (a newer change replacing it) until `free_backends` gives them, once the session is over.
    pub fn hold_backends(
        &mut self,
        b: Box<OnlineBackends>,
        d: DirectOptions,
    ) -> Option<(Box<OnlineBackends>, DirectOptions)> {
        self.held_backends = Some((b, d));
        self.free_backends()
    }

    /// The backends held, once no session is open.
    pub fn free_backends(&mut self) -> Option<(Box<OnlineBackends>, DirectOptions)> {
        if self.session_open() {
            return None;
        }
        self.held_backends.take()
    }

    /// Backends set at once (`NetHandle::backends`, the update gate's): a change held before is dropped.
    pub fn drop_held_backends(&mut self) {
        self.held_backends = None;
    }

    /// A change of the backends waits for the session to close.
    pub fn backends_held(&self) -> bool {
        self.held_backends.is_some()
    }

    /// This install's key and names, for a link of a session's own whatever is signed in.
    pub fn set_direct(&mut self, d: Option<DirectOptions>) {
        self.direct = d;
    }

    /// The player has said yes to that relay, or it is the config's own.
    pub(crate) fn relay_known(&self, address: &str) -> bool {
        same_relay(Some(address), self.config.relay.as_deref()) || self.known_relay(address).is_some()
    }

    /// The relay the player said yes to at that address, with the key id it proved then.
    pub(crate) fn known_relay(&self, address: &str) -> Option<&KnownRelay> {
        self.config.known_relays.iter().find(|r| same_relay(Some(&r.address), Some(address)))
    }

    /// The player said yes to that relay, or a relay said yes to (or the config's, signed in to unpinned) proved its
    /// key: kept, its id replacing the one before (main's next configure has it).
    pub(crate) fn know_relay(&mut self, k: KnownRelay) {
        let at = self.config.known_relays.iter().position(|r| same_relay(Some(&r.address), Some(&k.address)));
        match at {
            Some(i) if k.id.is_none() || self.config.known_relays[i].id == k.id => return,
            Some(i) => self.config.known_relays[i].id.clone_from(&k.id),
            None => self.config.known_relays.push(k.clone()),
        }
        self.notes.push(ServiceNote::RelayKnown(k));
    }

    /// The sessions on that backend end (`why` its reason), and the parties with the relay.
    fn end_on(&mut self, kind: BackendKind, why: OffWhy, now: f64) {
        let why = ClosedWhy::BackendOff { kind, why };
        let pages: Vec<PageId> = self.pages.keys().copied().collect();
        for p in pages {
            let on =
                self.pages.get(&p).and_then(|x| x.session.as_ref()).is_some_and(|s| s.base().backend == Some(kind));
            if on {
                let w = why.clone();
                self.with_session(p, now, |svc, s| s.end(svc, w, now));
            }
            // (a party is on the relay alone)
            if kind == BackendKind::Relay {
                let w = why.clone();
                self.with_party(p, now, |svc, x| x.lost(svc, w, now));
            }
        }
    }

    /// A backend's own state: on (signed in, the LAN host up) or why it is not.
    pub fn backend_state(&self, kind: BackendKind) -> (bool, Option<OffWhy>) {
        let online = match kind {
            BackendKind::Lan => self.lan.is_some(),
            k => self.backends.contains_key(&k),
        };
        (online, if online { None } else { Some(self.off_why(kind)) })
    }

    /// Why a backend is off: the one it was told, or that it is not in the config.
    pub(crate) fn off_why(&self, kind: BackendKind) -> OffWhy {
        self.whys.get(&kind).cloned().flatten().unwrap_or(OffWhy::NotOn)
    }

    /// This player's id on the backend a custom game is hosted on ('' with none on).
    pub fn me(&self) -> String {
        for k in [BackendKind::Epic, BackendKind::Relay] {
            if let Some(b) = self.eos_of(k) {
                return lock(&b).me().to_string();
            }
        }
        self.lan.as_ref().map(|l| l.me.clone()).unwrap_or_default()
    }

    fn status_why(&self) -> StatusWhy {
        let backends: Vec<BackendOff> =
            self.whys.iter().filter_map(|(k, w)| w.clone().map(|why| BackendOff { kind: *k, why })).collect();
        if backends.is_empty() { StatusWhy::SigningIn } else { StatusWhy::Off { backends } }
    }

    /// Online while any backend can host a custom game.
    pub fn status(&self) -> NetEvent {
        let online =
            self.eos_of(BackendKind::Epic).is_some() || self.eos_of(BackendKind::Relay).is_some() || self.lan.is_some();
        NetEvent::Status { online, me: self.me(), why: if online { StatusWhy::Online } else { self.status_why() } }
    }

    /// The sign-ins' own reasons for a relay or Epic not started (connecting, a sign-in failed and tried again,
    /// missing files) into the status, told when one changed: without them a relay never reached stayed "signing in".
    /// A lane the config does not have is NotOn ("not turned on in the launcher"), unless it was just turned off.
    // Deviation: the TS sent the sign-in lanes' states to the launcher alone; its pages' status had only the reasons
    // of backends that started and stopped.
    pub(crate) fn signin_follows(&mut self, states: &[api::BackendState]) {
        let mut changed = false;
        for s in states {
            if s.kind == BackendKind::Lan || s.online || self.backends.contains_key(&s.kind) {
                continue;
            }
            let why = if s.on {
                let Some(why) = s.why.clone() else { continue };
                why
            } else if self.whys.get(&s.kind) == Some(&Some(OffWhy::TurnedOff)) {
                continue;
            } else {
                OffWhy::NotOn
            };
            if self.whys.get(&s.kind) != Some(&Some(why.clone())) {
                self.whys.insert(s.kind, Some(why));
                changed = true;
            }
        }
        if changed {
            self.tell_status();
        }
    }

    fn tell_status(&mut self) {
        let st = self.status();
        let pages: Vec<PageId> = self.pages.keys().copied().collect();
        for p in pages {
            self.post(p, st.clone());
        }
    }

    /// A lobby's code on a backend (codes.rs): a relay's names the config's relay; Err when it cannot be written.
    pub(crate) fn code_on(&self, kind: BackendKind, lobby: &str) -> Result<String, CodeOnWhy> {
        let d = self.config.default_relay.as_deref();
        if kind == BackendKind::Epic {
            return encode_lobby_code(&LobbyCode::Epic { lobby: lobby.to_string() }, d).map_err(CodeOnWhy::Encode);
        }
        let Some(relay) = self.config.relay.clone() else { return Err(CodeOnWhy::NoRelay) };
        encode_lobby_code(&LobbyCode::Relay { lobby: lobby.to_string(), relay: Some(relay) }, d)
            .map_err(CodeOnWhy::Encode)
    }

    /// This app's relay is the release's default relay, whose hosted lobbies' codes are its short codes (a short
    /// code names a lobby there and nowhere else).
    pub(crate) fn on_release_relay(&self) -> bool {
        same_relay(self.config.relay.as_deref(), self.config.default_relay.as_deref())
    }

    /// Why a custom game cannot be hosted on the relay though it is signed in (None: it can).
    fn relay_host_why(&self) -> Option<CodeOnWhy> {
        self.code_on(BackendKind::Relay, &format!("l{}", "0".repeat(20))).err()
    }

    /// Whether two relay addresses reach the same relay: the same port, and the same host name or the same address it
    /// resolves to (a lookup that fails: not the same).
    pub(crate) fn same_relay_at(&self, a: &str, b: &str) -> bool {
        let (Some(x), Some(y)) = (parse_address(a), parse_address(b)) else { return false };
        if x.port != y.port {
            return false;
        }
        if x.host.eq_ignore_ascii_case(&y.host) {
            return true;
        }
        let find = |h: &str| -> Result<String, String> {
            match &self.o.lookup {
                Some(f) => f(h),
                None => {
                    use std::net::ToSocketAddrs;
                    (h, 0u16)
                        .to_socket_addrs()
                        .map_err(|e| e.to_string())?
                        .next()
                        .map(|a| a.ip().to_string())
                        .ok_or_else(|| "no address".to_string())
                }
            }
        };
        matches!((find(&x.host), find(&y.host)), (Ok(p), Ok(q)) if p == q)
    }

    // ---------- the links of a session's own ----------

    /// A link of a session's own: its number.
    pub(crate) fn add_own(&mut self, backend: Box<dyn Backend>, kind: OwnKind) -> u32 {
        self.own_n += 1;
        self.own.insert(self.own_n, OwnLink { backend: Arc::new(Mutex::new(backend)), users: 1, kind });
        self.own_n
    }

    /// The relay link of its own to `address` that other sessions use already: one more user, its number and the id
    /// of the key the relay proved.
    pub(crate) fn relay_link_in_use(&mut self, address: &str) -> Option<(u32, String)> {
        let key = normal_relay(address).unwrap_or_else(|| address.to_string());
        let (n, id) = self.own.iter().find_map(|(n, l)| match &l.kind {
            OwnKind::Relay(k, id) if *k == key => Some((*n, id.clone())),
            _ => None,
        })?;
        self.own[&n].users += 1;
        Some((n, id))
    }

    /// A ticket on the link to `address` another join is making (None: none under way).
    pub(crate) fn relay_connect_join(&mut self, address: &str) -> Option<u32> {
        let key = normal_relay(address).unwrap_or_else(|| address.to_string());
        let i = self.relay_connects.iter().position(|c| c.key == key && c.done.is_none())?;
        self.connect_n += 1;
        self.relay_connects[i].waiting.push(self.connect_n);
        Some(self.connect_n)
    }

    /// A link to `address` begun (`p`), shared by the joins to it until it is up: the first ticket on it.
    pub(crate) fn relay_connect_begin(&mut self, address: &str, p: Pending<Result<RelayClient, RelayError>>) -> u32 {
        let key = normal_relay(address).unwrap_or_else(|| address.to_string());
        self.connect_n += 1;
        self.relay_connects.push(RelayConnect { key, p, done: None, waiting: vec![self.connect_n] });
        self.connect_n
    }

    /// The outcome of the link a ticket waits on, once it is over: the link's number (one user each ticket) and the
    /// id of the key the relay proved, or the relay's refusal. The ticket is spent then.
    pub(crate) fn relay_connect_poll(&mut self, ticket: u32) -> Option<Result<(u32, String), String>> {
        let i = self.relay_connects.iter().position(|c| c.waiting.contains(&ticket))?;
        if self.relay_connects[i].done.is_none() {
            let r = self.relay_connects[i].p.poll()?;
            let done = match r {
                Ok(c) => {
                    let key = self.relay_connects[i].key.clone();
                    let users = u32::try_from(self.relay_connects[i].waiting.len()).unwrap_or(u32::MAX);
                    let id = c.relay_id();
                    let n = self.add_own(Box::new(c), OwnKind::Relay(key, id.clone()));
                    self.own[&n].users = users;
                    Ok((n, id))
                }
                Err(e) => Err(e.to_string()),
            };
            self.relay_connects[i].done = Some(done);
        }
        let c = &mut self.relay_connects[i];
        c.waiting.retain(|t| *t != ticket);
        let done = c.done.clone();
        if c.waiting.is_empty() {
            self.relay_connects.remove(i);
        }
        done
    }

    /// A join waiting on a link lets it go: its user of the link once up; the link's start cancelled when it was the
    /// last (sessions.ts relayLink's release).
    pub(crate) fn relay_connect_cancel(&mut self, ticket: u32) {
        let Some(i) = self.relay_connects.iter().position(|c| c.waiting.contains(&ticket)) else { return };
        let c = &mut self.relay_connects[i];
        c.waiting.retain(|t| *t != ticket);
        let up = match c.done {
            Some(Ok((n, _))) => Some(n),
            _ => None,
        };
        if c.waiting.is_empty() {
            if c.done.is_none() {
                c.p.cancel();
            }
            self.relay_connects.remove(i);
        }
        if let Some(n) = up {
            self.release(n);
        }
    }

    /// A user of a link of its own lets it go: at its last, it is shut down (a direct host's listener closed).
    pub(crate) fn release(&mut self, n: u32) {
        let Some(l) = self.own.get_mut(&n) else { return };
        l.users = l.users.saturating_sub(1);
        if l.users > 0 {
            return;
        }
        let Some(mut l) = self.own.shift_remove(&n) else { return };
        self.accepts.shift_remove(&Via::Own(n));
        if let OwnKind::DirectHost(d) = &mut l.kind {
            d.close();
            self.listening(None, Some(n), None);
        }
        let _ = lock(&l.backend).shutdown();
    }

    /// A hosted LAN lobby listens (`up`: its link and address), could not (`why`), or stopped (`down`).
    pub(crate) fn listening(&mut self, up: Option<(u32, String)>, down: Option<u32>, why: Option<HostStartWhy>) {
        let up_or_why = up.is_some() || why.is_some();
        if let Some((n, a)) = up {
            self.listeners.insert(n, a);
        }
        if let Some(n) = down {
            self.listeners.shift_remove(&n);
        }
        if up_or_why {
            self.listen_why = why;
        }
        self.tell_direct();
    }

    /// The LAN host's state to the owner, when it changed.
    fn tell_direct(&mut self) {
        let d = self.lan.as_ref().map(|h| DirectStatus {
            addresses: crate::lan::direct::local_addresses(h.o.direct.loopback, None),
            port: h.o.port,
            listening: self.listeners.values().cloned().collect(),
            why: self.listen_why.clone(),
        });
        if self.last_direct.as_ref() == Some(&d) {
            return;
        }
        self.last_direct = Some(d.clone());
        self.notes.push(ServiceNote::Direct(d));
    }

    /// The port LAN lobbies are announced on.
    pub(crate) fn discovery_port(&self) -> u16 {
        self.o.discovery_port.unwrap_or(DISCOVERY_PORT)
    }

    // ---------- Epic's relay control ----------

    /// A session of `page` opened on Epic: a queue's, or a friend game's that may not link directly, forces relays at
    /// once (before it links); the rest is looked at once the session stands.
    pub(crate) fn epic_opened(&mut self, page: PageId, queue: bool, direct: bool, now: f64) {
        let _ = now;
        self.on_epic.insert(page);
        if (queue || !direct)
            && let Some(e) = self.eos_of(BackendKind::Epic)
        {
            let _ = self.set_epic_mode(&e, RelayMode::Force);
        }
        self.relay_check_queued = true;
    }

    /// A session of `page` on Epic is over: a queue's holds the relays forced QUEUE_HOLD_MS more.
    pub(crate) fn epic_closed(&mut self, page: PageId, queue: bool, now: f64) {
        if !self.on_epic.shift_remove(&page) {
            return;
        }
        self.direct_queue_peers.shift_remove(&page);
        if queue {
            self.queue_hold = now + QUEUE_HOLD_MS;
            self.timers.after(now, QUEUE_HOLD_MS, SvcDue::QueueHold);
        }
        self.relay_check(now);
    }

    /// A relay-hosted session's match path as it stands (None: it wants no Epic link): Epic's relay control looked at
    /// again when it changed. Any forced path that ends (a queue's, or a custom lobby's whose links may not be direct)
    /// holds the relays forced QUEUE_HOLD_MS more, as `epic_closed` does for a queue session on Epic.
    fn path_seen(&mut self, page: PageId, way: Option<PathWay>, now: f64) {
        if self.paths.get(&page) == way.as_ref() {
            return;
        }
        let was_forced = match way {
            Some(w) => {
                self.paths.insert(page, w);
                false
            }
            None => self.paths.shift_remove(&page).is_some_and(|o| !o.direct),
        };
        if was_forced {
            self.queue_hold = now + QUEUE_HOLD_MS;
            self.timers.after(now, QUEUE_HOLD_MS, SvcDue::QueueHold);
        }
        self.relay_check(now);
    }

    /// The match path of `page`'s relay-hosted session wants its Epic links to `peers` (`direct`: they may be
    /// direct), told by the path before its first packet while its session is out of its page: Epic's relay control
    /// is set at once, so that a forced path is on Epic's relays before it links (`epic_relay_mode` then says
    /// whether Force could be had). Its end is seen when its session is put back with no path open.
    pub fn path_wanted(&mut self, page: PageId, direct: bool, peers: &[String], now: f64) {
        self.path_seen(page, Some(PathWay { direct, peers: peers.to_vec() }), now);
    }

    /// Epic's relay control by the sessions open on it (sessions.ts relayCheck), set on EOS at each check; the queue
    /// sessions on Epic end when Force cannot be had. A relay-hosted session's match path that wants its Epic links
    /// (`paths`) counts as a session open on Epic: a forced one as a queue's, a direct one as a friend game's whose
    /// peers are its links' (a path whose Force cannot be had is not ended here: its relay still referees).
    pub(crate) fn relay_check(&mut self, now: f64) {
        let Some(epic) = self.eos_of(BackendKind::Epic) else { return };
        let via = Via::Backend(BackendKind::Epic);
        let open: Vec<PageId> = self
            .on_epic
            .iter()
            .copied()
            .filter(|p| {
                self.pages
                    .get(p)
                    .and_then(|x| x.session.as_ref())
                    .is_some_and(|s| s.base().via == Some(via) && !s.base().closed)
            })
            .collect();
        let session = |p: &PageId| self.pages.get(p).and_then(|x| x.session.as_ref());
        let friends: Vec<PageId> =
            open.iter().copied().filter(|p| session(p).is_some_and(|s| !s.base().queue && s.base().direct)).collect();
        // Unverified: a relay-hosted match path counts as a session open on Epic (online plan v4 section 5: the path's
        // links forced as a queue's, direct only for a custom lobby whose humans all said so); no EOS run checked it.
        let paths: Vec<&PathWay> =
            self.paths.iter().filter(|(p, _)| self.pages.contains_key(*p)).map(|(_, w)| w).collect();
        let links = lock(&epic).linked_peers();
        let allow = self.config.allow_direct
            && (!open.is_empty() || !paths.is_empty())
            && friends.len() == open.len()
            && paths.iter().all(|w| w.direct)
            && now >= self.queue_hold
            && links.iter().all(|l| {
                friends.iter().any(|p| session(p).is_some_and(|s| s.has_peer(l)))
                    || paths.iter().any(|w| w.peers.contains(l))
            });
        let r = self.set_epic_mode(&epic, if allow { RelayMode::Allow } else { RelayMode::Force });
        for p in open {
            let Some(s) = self.pages.get(&p).and_then(|x| x.session.as_ref()) else { continue };
            if !s.base().queue {
                continue;
            }
            if let Err(why) = &r
                && !allow
            {
                let w = ClosedWhy::NotRelayed { text: why.clone() };
                self.with_session(p, now, |svc, s| s.end(svc, w, now));
                continue;
            }
            // (a player this process links to directly for a friend game has this PC's address already: the queue
            // keeps the link rather than end that game)
            let mut lines = Vec::new();
            for l in &links {
                let has = self.pages.get(&p).and_then(|x| x.session.as_ref()).is_some_and(|s| s.has_peer(l));
                let told = self.direct_queue_peers.get(&p).is_some_and(|t| t.contains(l));
                if lock(&epic).relayed(l) != Some(false) || !has || told {
                    continue;
                }
                self.direct_queue_peers.entry(p).or_default().insert(l.clone());
                lines.push(format!(
                    "{l}: a queue match's player this PC links to directly already (a friend game's link): the queue uses it"
                ));
            }
            for l in lines {
                self.log(&l);
            }
        }
    }

    /// Sets Epic's relay control on EOS (every time: EOS's is the one that counts); Err EOS's why not (the mode is
    /// then not known). The TS's 'Epic is not signed in' is the caller's: it passes Epic as signed in.
    fn set_epic_mode(&mut self, epic: &Shared, mode: RelayMode) -> Result<(), String> {
        let was = self.epic_mode;
        let r = lock(epic).set_relay_mode(mode);
        if let Err(e) = r {
            self.epic_mode = None;
            self.log(&format!("Epic relay setting: {e}"));
            return Err(e);
        }
        self.epic_mode = Some(mode);
        if was != Some(mode) {
            self.log(if mode == RelayMode::Allow { "Epic: direct links allowed" } else { "Epic: relays forced" });
        }
        Ok(())
    }

    /// Epic's relay control as set (tests; None: not known).
    pub fn epic_relay_mode(&self) -> Option<RelayMode> {
        self.epic_mode
    }

    // ---------- presence, the online list, invites ----------

    /// This player's presence as a page sent it: kept, published on the relay, and the parties' cards changed with it.
    fn set_presence(&mut self, p: &RelayPresence, now: f64) {
        self.presence_now = Some(party::presence_of(p));
        if self.eos_of(BackendKind::Relay).is_some() {
            self.publish(now);
        }
        let pages: Vec<PageId> = self.pages.keys().copied().collect();
        for pg in pages {
            self.with_party(pg, now, |svc, x| x.card_changed(svc, now));
        }
    }

    /// The fighter this player's main menu shows as a page sent it (None: none, or not a fighter's names): kept, and
    /// the parties' cards changed with it.
    fn set_menu_fighter(&mut self, f: Option<&MenuFighter>, now: f64) {
        let f = f.and_then(party::menu_fighter_of);
        if f == self.fighter_now {
            return;
        }
        self.fighter_now = f;
        let pages: Vec<PageId> = self.pages.keys().copied().collect();
        for pg in pages {
            self.with_party(pg, now, |svc, x| x.card_changed(svc, now));
        }
    }

    // the presence to the relay: one set at a time, PRESENCE_GAP_MS apart, the latest winning
    fn publish(&mut self, now: f64) {
        let _ = now;
        let Some(cur) = self.presence_now.clone() else { return };
        // (the relay refuses a presence of no name: a refused one goes as the last it took)
        let name = if cur.name.is_empty() { self.name_taken.clone() } else { cur.name.clone() };
        let Some(relay) = self.eos_of(BackendKind::Relay) else { return };
        if name.is_empty() {
            return;
        }
        let want = RelayPresence { name: name.clone(), ..cur };
        let text = serde_json::to_string(&want).unwrap_or_default();
        if self.published.busy || self.published.has.as_deref() == Some(&text) {
            return;
        }
        let ask = {
            let mut g = lock(&relay);
            match g.presence() {
                Some(p) => p.set(want),
                None => return,
            }
        };
        self.published.busy = true;
        let epoch = self.published.epoch;
        self.asks_out.insert((Via::Backend(BackendKind::Relay), ask), SvcAsk::Presence { text, name, epoch });
    }

    /// The online list (sessions.ts who): the relay's listed players, this player left out, their names cleaned.
    fn who(&mut self, page: PageId, id: u32, now: f64) {
        let Some(relay) = self.eos_of(BackendKind::Relay) else {
            let why = if self.eos_of(BackendKind::Epic).is_none() && self.lan.is_none() {
                BrowseWhy::Offline { why: self.status_why() }
            } else {
                BrowseWhy::ListOff { why: self.off_why(BackendKind::Relay) }
            };
            return self.post(page, NetEvent::Who { id, players: Vec::new(), why });
        };
        let has_list = lock(&relay).presence().is_some();
        if !has_list {
            return self.post(page, NetEvent::Who { id, players: Vec::new(), why: BrowseWhy::NoList });
        }
        self.whos.push(WhoWait { page, id });
        let fresh = self.whoing.as_ref().is_none_or(|b| b.at.is_some_and(|at| now - at >= WHO_AGAIN_MS));
        if fresh {
            let ask = lock(&relay).presence().map(|p| p.who(WHO_LIMIT));
            if let Some(ask) = ask {
                self.whoing = Some(Searching { at: None, answer: None });
                self.who_answer = None;
                self.asks_out.insert((Via::Backend(BackendKind::Relay), ask), SvcAsk::Who);
            }
        }
        self.answer_whos();
    }

    fn answer_whos(&mut self) {
        let Some(a) = self.who_answer.clone() else { return };
        let me = self.eos_of(BackendKind::Relay).map(|b| lock(&b).me().to_string()).unwrap_or_default();
        for w in std::mem::take(&mut self.whos) {
            let (players, why) = match &a {
                Ok(rows) => {
                    let players = rows
                        .iter()
                        .filter(|r| !r.id.is_empty() && r.id != me)
                        .map(|r| {
                            let said = clean_name(&r.name);
                            let name = if self.name_refused.refuses(&said) { String::new() } else { said };
                            Who {
                                id: r.id.clone(),
                                name,
                                icon: if icon_ok(&r.icon) { r.icon.clone() } else { String::new() },
                                busy: r.busy,
                                status: r.status,
                            }
                        })
                        .collect();
                    (players, BrowseWhy::All)
                }
                Err(e) => (Vec::new(), BrowseWhy::ListFailed { text: e.clone() }),
            };
            self.post(w.page, NetEvent::Who { id: w.id, players, why });
        }
    }

    /// An invite from or to `id` is taken: none of that id within INVITE_AGAIN_MS, and the budget has room.
    fn invite_gate(seen: &mut IndexMap<String, f64>, budget: &mut Bucket, id: &str, now: f64) -> bool {
        seen.retain(|_, at| now - *at < INVITE_AGAIN_MS);
        if seen.contains_key(id) || !budget.take(1.0, now) {
            return false;
        }
        seen.insert(id.to_string(), now);
        true
    }

    /// An invite this player sends to `to` may go (INVITES_SENT_PER_MIN).
    pub(crate) fn may_invite(&mut self, to: &str, now: f64) -> bool {
        let b = self
            .invite_out_budget
            .get_or_insert_with(|| Bucket::new(INVITES_SENT_PER_MIN / 60.0, INVITES_SENT_PER_MIN, now));
        Self::invite_gate(&mut self.invites_out, b, to, now)
    }

    /// An invite from another player (sessions.ts invited).
    fn invited(&mut self, from: &str, payload: &str, now: f64) {
        let Ok(Value::Object(m)) = serde_json::from_str::<Value>(payload) else { return };
        let Some(code) = m.get("code").and_then(Value::as_str) else { return };
        if code.is_empty() || code.encode_utf16().count() > 64 {
            return;
        }
        // (a payload of no kind is an older app's: a custom game's)
        let kind = match m.get("kind") {
            Some(Value::String(k)) if k == "party" => InviteKind::Party,
            None => InviteKind::Custom,
            Some(Value::String(k)) if k == "custom" => InviteKind::Custom,
            _ => return,
        };
        if self.presence_now.as_ref().is_some_and(|p| !p.invites) {
            return;
        }
        let b = self.invite_in_budget.get_or_insert_with(|| Bucket::new(INVITES_PER_MIN / 60.0, INVITES_PER_MIN, now));
        if !Self::invite_gate(&mut self.invites_in, b, from, now) {
            return;
        }
        let said = clean_name(m.get("name").and_then(Value::as_str).unwrap_or(""));
        let name = if self.name_refused.refuses(&said) { String::new() } else { said };
        if kind == InviteKind::Party {
            let c = direct_text(code);
            self.party_invites.shift_remove(&c);
            self.party_invites.insert(c, from.to_string());
            while self.party_invites.len() > PARTY_INVITES_KEPT {
                self.party_invites.shift_remove_index(0);
            }
        }
        let e = NetEvent::Invited { kind, code: code.to_string(), from_id: from.to_string(), from_name: name };
        let pages: Vec<PageId> = self.pages.keys().copied().collect();
        for p in pages {
            self.post(p, e.clone());
        }
    }

    // ---------- the browser ----------

    /// With LAN on, the listener up (opened now if it was not) until BROWSE_LISTEN_MS from now; the time a browse may
    /// read it from.
    fn hear(&mut self, now: f64) -> Option<f64> {
        let l = self.lan.as_ref()?;
        if self.heard.is_none() {
            let (own, loopback) = (l.own.clone(), l.o.direct.loopback);
            let mut heard =
                LanListener::new(ListenerOptions { port: Some(self.discovery_port()), own, loopback, log: None });
            let port = self.discovery_port();
            match heard.start() {
                Ok(()) => self.log(&format!("lan: hearing lobbies on port {port}")),
                Err(e) => self.log(&format!("lan: {e}")),
            }
            self.heard = Some(heard);
            self.heard_since = now;
        }
        if let Some(t) = self.heard_stop.take() {
            self.timers.cancel(t);
        }
        self.heard_stop = Some(self.timers.after(now, BROWSE_LISTEN_MS, SvcDue::StopHearing));
        let at = self.heard_since + FIRST_LISTEN_MS;
        if at > now {
            self.timers.after(now, at - now, SvcDue::Heard);
        }
        Some(at)
    }

    fn stop_hearing(&mut self) {
        if let Some(t) = self.heard_stop.take() {
            self.timers.cancel(t);
        }
        if let Some(mut h) = self.heard.take() {
            h.close();
        }
    }

    /// The public custom lobbies of every signed-in backend that lists them (sessions.ts browse); answered `Browsed`.
    fn browse(&mut self, page: PageId, id: u32, now: f64) {
        let kinds: Vec<BackendKind> =
            [BackendKind::Epic, BackendKind::Relay].into_iter().filter(|k| self.eos_of(*k).is_some()).collect();
        self.browsing.retain(|k, _| kinds.contains(k));
        if kinds.is_empty() && self.lan.is_none() {
            let why = BrowseWhy::Offline { why: self.status_why() };
            return self.post(page, NetEvent::Browsed { id, lobbies: Vec::new(), why });
        }
        // (the LAN lobbies listened for meanwhile: a first browse waits one announcement period beside the searches)
        let heard_at = self.hear(now);
        for k in &kinds {
            let fresh = self.browsing.get(k).is_none_or(|b| b.at.is_some_and(|at| now - at >= BROWSE_AGAIN_MS));
            if fresh {
                let b = self.eos_of(*k).expect("signed in");
                let ask = lock(&b).search_lobbies(CUSTOM_BUCKET, BROWSE_LIMIT, true);
                self.browsing.insert(*k, Searching { at: None, answer: None });
                self.asks_out.insert((Via::Backend(*k), ask), SvcAsk::Search(*k));
            }
        }
        self.browses.push(Browse { page, id, kinds, heard_at });
        self.answer_browses(now);
    }

    /// Every browse whose searches are answered and whose LAN wait is over gets its answer, in order.
    fn answer_browses(&mut self, now: f64) {
        let mut i = 0;
        while i < self.browses.len() {
            let b = &self.browses[i];
            let ready = b.heard_at.is_none_or(|at| now >= at)
                && b.kinds.iter().all(|k| self.browsing.get(k).is_some_and(|s| s.answer.is_some()));
            if !ready {
                i += 1;
                continue;
            }
            let b = self.browses.remove(i);
            let mut lobbies = Vec::new();
            let mut failed = Vec::new();
            for k in &b.kinds {
                match self.browsing[k].answer.clone().expect("answered") {
                    Ok(found) => {
                        for x in found {
                            if x.info.is_empty() {
                                continue;
                            }
                            // (the code its host wrote, which its core takes, when it names this lobby; else this
                            // app's code of it, a host that listed none)
                            let own = match decode_lobby_code(&x.code, None) {
                                Ok(LobbyCode::Epic { lobby }) if *k == BackendKind::Epic && lobby == x.id => {
                                    direct_text(&x.code)
                                }
                                Ok(LobbyCode::Relay { lobby, .. }) if *k == BackendKind::Relay && lobby == x.id => {
                                    direct_text(&x.code)
                                }
                                _ => String::new(),
                            };
                            let code = if own.is_empty() {
                                match self.code_on(*k, &x.id) {
                                    Ok(c) => c,
                                    Err(e) => {
                                        self.log(&format!("browse: {}", e.log_text()));
                                        continue;
                                    }
                                }
                            } else {
                                own
                            };
                            lobbies.push(BrowsedLobby {
                                backend: *k,
                                code,
                                members: x.members,
                                max: x.max,
                                info: x.info,
                            });
                        }
                    }
                    Err(e) => {
                        self.log(&format!("browse on {}: {e}", name_of(*k)));
                        failed.push(BackendText { kind: *k, text: e });
                    }
                }
            }
            if let Some(h) = self.heard.as_mut() {
                if !h.why.is_empty() {
                    failed.push(BackendText { kind: BackendKind::Lan, text: h.why.clone() });
                } else {
                    let seen: IndexSet<String> = lobbies.iter().map(|r: &BrowsedLobby| r.code.clone()).collect();
                    for x in h.heard(now) {
                        if !seen.contains(&x.code) {
                            lobbies.push(BrowsedLobby {
                                backend: BackendKind::Lan,
                                code: x.code,
                                members: x.members,
                                max: x.max,
                                info: x.info,
                            });
                        }
                    }
                }
            }
            let why = if failed.is_empty() { BrowseWhy::All } else { BrowseWhy::Failed { backends: failed } };
            self.post(b.page, NetEvent::Browsed { id: b.id, lobbies, why });
        }
    }

    // ---------- the backends' events ----------

    /// Every backend's and own link's events by `now`, each handed on (sessions.ts attach).
    fn poll_backends(&mut self, now: f64) -> bool {
        let mut any = false;
        let vias: Vec<Via> =
            self.backends.keys().map(|k| Via::Backend(*k)).chain(self.own.keys().map(|n| Via::Own(*n))).collect();
        for via in vias {
            loop {
                let Some(b) = self.backend(via) else { break };
                let e = lock(&b).poll(now);
                let Some(e) = e else { break };
                any = true;
                self.event(via, e, now);
            }
        }
        any
    }

    /// One event of a backend (sessions.ts attach's handlers); the accept sets after it. A relay check it queues
    /// (the TS's relayCheckSoon, a microtask) runs at the end of the owner's step: `pump`, a page's request, `flush`.
    pub fn event(&mut self, via: Via, e: BackendEvent, now: f64) {
        self.event_in(via, e, now);
        self.settle_accepts();
    }

    /// The end of the owner's step: a queued relay check runs, the accept sets are set (the tests' replay calls it
    /// where the TS's microtasks ran: after its clock's advance).
    pub fn flush(&mut self, now: f64) {
        self.settle(now);
    }

    fn event_in(&mut self, via: Via, e: BackendEvent, now: f64) {
        let epic = via == Via::Backend(BackendKind::Epic);
        match e {
            BackendEvent::Lobby(c) => {
                let lobby = match &c {
                    LobbyChange::Members { lobby, .. } | LobbyChange::Closed { lobby, .. } => lobby.clone(),
                };
                for p in self.pages_on(via) {
                    if self.pages[&p].session.as_ref().is_some_and(|s| s.in_lobby(&lobby)) {
                        self.with_session(p, now, |svc, s| s.lobby_changed(svc, &c, now));
                    }
                }
                for p in self.parties_on(via) {
                    if self.pages[&p].party.as_ref().is_some_and(|x| x.lobby() == lobby) {
                        self.with_party(p, now, |svc, x| x.lobby_changed(svc, &c, now));
                    }
                }
                if epic {
                    self.relay_check_queued = true;
                }
            }
            BackendEvent::Packet { from, channel, bytes } => {
                // (a party's packet is the party's alone: a session's joiner would take it as a broken link)
                if channel == Channel::Messages && bytes.first() == Some(&FRAME_PARTY) {
                    for p in self.parties_on(via) {
                        if self.pages[&p].party.as_ref().is_some_and(|x| x.has_peer(&from)) {
                            self.with_party(p, now, |svc, x| x.message(svc, &from, &bytes, now));
                        }
                    }
                    return;
                }
                // (a match path's packet on Epic, from one of its humans, is the path's first: its pings and pongs,
                // its referee's ruled packets and a human's packets to the referee here)
                if epic && channel == Channel::Match {
                    for p in self.path_pages(&from) {
                        let took = self.with_session(p, now, |svc, s| s.path_packet(svc, &from, &bytes, now));
                        if took == Some(true) {
                            return;
                        }
                    }
                }
                for p in self.pages_on(via) {
                    if !self.on_via(p, via) {
                        continue;
                    }
                    if channel == Channel::Match {
                        self.with_session(p, now, |svc, s| s.match_packet(svc, &from, &bytes, now));
                    } else if self.pages[&p].session.as_ref().is_some_and(|s| s.has_peer(&from)) {
                        self.with_session(p, now, |svc, s| s.message(svc, &from, &bytes, now));
                    }
                }
            }
            BackendEvent::PeerClosed(peer) => {
                if epic {
                    self.path_link(&peer, None, now);
                }
                for p in self.pages_on(via) {
                    if self.on_via(p, via) {
                        self.with_session(p, now, |svc, s| s.peer_closed(svc, &peer, now));
                    }
                }
                for p in self.parties_on(via) {
                    self.with_party(p, now, |svc, x| x.peer_closed(svc, &peer, now));
                }
                if epic {
                    self.relay_check_queued = true;
                }
            }
            BackendEvent::Link { peer, relayed } => {
                if epic {
                    self.path_link(&peer, relayed, now);
                }
                if relayed.is_none() {
                    // (its sessions may have let the peer go already: every page on the backend is told)
                    for p in self.pages_on(via) {
                        self.post(p, NetEvent::Link { peer: peer.clone(), relayed });
                    }
                    return;
                }
                self.log(&format!("{peer}: {}", if relayed == Some(true) { "relayed" } else { "direct" }));
                for p in self.pages_on(via) {
                    let s = self.pages[&p].session.as_ref().expect("on via");
                    if s.has_peer(&peer) || s.base().match_route.as_ref().is_some_and(|m| m.referee == peer) {
                        self.post(p, NetEvent::Link { peer: peer.clone(), relayed });
                    }
                }
                if epic {
                    self.relay_check(now);
                }
            }
            BackendEvent::Invite { from, payload } => self.invited(&from, &payload, now),
            BackendEvent::Answered(ask, r) => self.answered(via, ask, r, now),
            // (a hosted lobby's listing: its leader's session lists it on a relay, announces it on LAN)
            BackendEvent::Listing { lobby, info } => {
                for p in self.pages_on(via) {
                    self.with_session(p, now, |svc, s| {
                        if let Session::Hosted(s) = s {
                            s.listing(svc, &lobby, info.clone(), now);
                        }
                    });
                }
            }
            BackendEvent::SignedOut(why) => self.signed_out(via, why, now),
            // (signin.rs holds a backend loaded signing in until its SignedIn: one started before it is taken as
            // signed in again here, its `me` now holding)
            BackendEvent::SignedIn => {
                if let Via::Backend(k) = via {
                    self.log(&format!("{}: signed in", k.id()));
                    self.signed_in(k, now);
                    self.notes.push(ServiceNote::Backend(k));
                }
            }
        }
    }

    fn signed_out(&mut self, via: Via, why: String, now: f64) {
        match via {
            Via::Backend(k) => {
                if self.signin {
                    self.notes.push(ServiceNote::SignedOut(k, why));
                } else {
                    self.stop_backend(k, OffWhy::Backend { text: why }, now);
                }
            }
            Via::Own(n) => {
                let Some(l) = self.own.get(&n) else { return };
                let end = match &l.kind {
                    OwnKind::Relay(..) => ClosedWhy::RelayLost { text: why },
                    OwnKind::DirectHost(_) => ClosedWhy::DirectStopped { text: why },
                    OwnKind::DirectJoin => ClosedWhy::HostLinkLost { text: why },
                };
                if let OwnKind::Relay(..) = l.kind {
                    // (gone: no session takes this link again)
                    if let Some(l) = self.own.get_mut(&n) {
                        l.kind = OwnKind::Relay(String::new(), String::new());
                    }
                }
                for p in self.pages_on(via) {
                    let w = end.clone();
                    self.with_session(p, now, |svc, s| s.end(svc, w, now));
                }
            }
        }
    }

    /// A backend's answer: the service's own ask's, else a session's, else a party's.
    fn answered(&mut self, via: Via, ask: Ask, r: Result<Answer, String>, now: f64) {
        if let Some(a) = self.asks_out.shift_remove(&(via, ask)) {
            return self.own_answer(a, r, now);
        }
        let pages: Vec<PageId> = self.pages.keys().copied().collect();
        for p in &pages {
            if self.with_session(*p, now, |svc, s| s.answered(svc, via, ask, &r, now)) == Some(true) {
                return;
            }
        }
        for p in &pages {
            if self.with_party(*p, now, |svc, x| x.answered(svc, via, ask, &r, now)) == Some(true) {
                return;
            }
        }
        let mut retired = std::mem::take(&mut self.retired);
        let mut taken = retired.iter_mut().any(|s| s.answered(self, via, ask, &r, now));
        retired.append(&mut self.retired);
        self.retired = retired;
        if !taken {
            let mut parties = std::mem::take(&mut self.retired_parties);
            taken = parties.iter_mut().any(|x| x.answered(self, via, ask, &r, now));
            parties.append(&mut self.retired_parties);
            self.retired_parties = parties;
        }
        let _ = taken;
    }

    fn own_answer(&mut self, a: SvcAsk, r: Result<Answer, String>, now: f64) {
        match a {
            SvcAsk::Search(k) => {
                let answer = match r {
                    Ok(Answer::Found(x)) => Ok(x),
                    Ok(_) => Ok(Vec::new()),
                    Err(e) => Err(e),
                };
                if let Some(s) = self.browsing.get_mut(&k) {
                    s.at = Some(now);
                    s.answer = Some(answer);
                }
                self.answer_browses(now);
            }
            SvcAsk::Who => {
                let answer = match r {
                    Ok(Answer::Who(x)) => Ok(x),
                    Ok(_) => Ok(Vec::new()),
                    Err(e) => {
                        self.log(&format!("who: {e}"));
                        Err(e)
                    }
                };
                if let Some(s) = self.whoing.as_mut() {
                    s.at = Some(now);
                }
                self.who_answer = Some(answer);
                self.answer_whos();
            }
            SvcAsk::Invite { page, to } => {
                if let Err(e) = r {
                    self.log(&format!("invite: {e}"));
                    let open = self.pages.get(&page).and_then(|p| p.session.as_ref()).is_some_and(|s| !s.base().closed);
                    if open {
                        self.post(page, NetEvent::InviteRefused { to });
                    }
                }
            }
            SvcAsk::Presence { text, name, epoch } => {
                let gap = match r {
                    Ok(_) => {
                        if epoch == self.published.epoch {
                            self.published.has = Some(text);
                        }
                        self.name_taken = name;
                        PRESENCE_GAP_MS
                    }
                    Err(e) => {
                        self.log(&format!("presence: {e}"));
                        PRESENCE_RETRY_MS
                    }
                };
                self.timers.after(now, gap, SvcDue::Presence(epoch));
            }
        }
    }

    // ---------- the clock ----------

    /// Runs what is due by `now`: the backends' events, the sessions' and the parties' timers, the service's own.
    pub fn pump(&mut self, now: f64) {
        self.poll_backends(now);
        self.poll_pending(now);
        while let Some((at, who)) = self.first_due() {
            if at > now {
                break;
            }
            self.run_due(who, at);
            self.settle_accepts();
            self.poll_backends(now);
        }
        if let Some(h) = self.heard.as_mut() {
            h.pump(now);
        }
        self.settle(now);
        self.tell_way();
    }

    /// The signed-in relay's UDP link (Backend::udp; None: no relay signed in, or not known yet).
    pub fn udp(&self) -> Option<bool> {
        self.eos_of(BackendKind::Relay).and_then(|b| lock(&b).udp())
    }

    /// Tells every page the way to the relay when it changed: slower while the relay is signed in and its UDP link is
    /// off (Backend::udp Some(false)).
    fn tell_way(&mut self) {
        let slow = self.udp() == Some(false);
        if slow == self.slow {
            return;
        }
        self.slow = slow;
        let pages: Vec<PageId> = self.pages.keys().copied().collect();
        for p in pages {
            self.post(p, NetEvent::Way { slow });
        }
    }

    /// The sessions' starts under way (a direct host's, a link's), each one's result once it is there.
    fn poll_pending(&mut self, now: f64) {
        let waiting: Vec<PageId> =
            self.pages.values().filter(|p| p.session.as_ref().is_some_and(Session::pending)).map(|p| p.id).collect();
        for p in waiting {
            self.with_session(p, now, |svc, s| s.poll_pending(svc, now));
        }
    }

    /// Runs only the timers due by `now` (no backend is polled).
    pub fn pump_timers(&mut self, now: f64) {
        while self.run_first(now).is_some() {}
    }

    /// Runs the first timer due by `now`, if any, and gives its time (the tests' replay steps the timers one at a time
    /// with it).
    pub fn run_first(&mut self, now: f64) -> Option<f64> {
        let (at, who) = self.first_due().filter(|x| x.0 <= now)?;
        self.run_due(who, at);
        self.settle_accepts();
        Some(at)
    }

    /// When `pump` has a timer to run next (None: none). A backend's events are not in it: the owner pumps at least
    /// at its own beat (spawn: every IDLE_MS) for them, and at `backends_due`.
    pub fn next_due(&self) -> Option<f64> {
        self.first_due().map(|x| x.0)
    }

    /// When a backend's poll has something to do next (EOS: its next tick, 1 ms apart while a link is open; None:
    /// no backend says).
    pub fn backends_due(&self) -> Option<f64> {
        let vias = self.backends.keys().map(|k| Via::Backend(*k)).chain(self.own.keys().map(|n| Via::Own(*n)));
        vias.filter_map(|via| self.backend(via)).filter_map(|b| lock(&b).next_due()).reduce(f64::min)
    }

    /// Everyone's first timer: its time and whose it is.
    fn first_due(&self) -> Option<(f64, Due)> {
        let mut best: Option<((f64, u64), Due)> = None;
        let mut take = |k: Option<(f64, u64)>, who: Due| {
            if let Some(k) = k
                && best.as_ref().is_none_or(|(b, _)| k.0 < b.0 || (k.0 == b.0 && k.1 < b.1))
            {
                best = Some((k, who));
            }
        };
        take(self.timers.next_key(), Due::Service);
        for p in self.pages.values() {
            take(p.session.as_ref().and_then(Session::next_key), Due::Session(p.id));
            take(p.party.as_ref().and_then(|x| x.next_due().map(|t| (t, u64::MAX))), Due::Party(p.id));
        }
        for (i, s) in self.retired.iter().enumerate() {
            take(s.next_key(), Due::Retired(i));
        }
        for (i, x) in self.retired_parties.iter().enumerate() {
            take(x.next_due().map(|t| (t, u64::MAX)), Due::RetiredParty(i));
        }
        best.map(|((at, _), who)| (at, who))
    }

    fn run_due(&mut self, who: Due, at: f64) {
        match who {
            Due::Service => {
                while let Some((_, _, d)) = self.timers.take_due(at) {
                    self.run_own(d, at);
                    // (one at a time: another owner's timer of the same time may come first)
                    if self.timers.next_key().is_some_and(|k| k.0 <= at) {
                        break;
                    }
                }
            }
            Due::Session(p) => {
                self.with_session(p, at, |svc, s| s.pump(svc, at));
            }
            Due::Party(p) => {
                self.with_party(p, at, |svc, x| x.pump(svc, at));
            }
            Due::Retired(i) => {
                let mut s = self.retired.remove(i);
                s.pump(self, at);
                self.retire(s);
            }
            Due::RetiredParty(i) => {
                let mut x = self.retired_parties.remove(i);
                x.pump(self, at);
                self.retire_party(x);
            }
        }
    }

    fn run_own(&mut self, d: SvcDue, now: f64) {
        match d {
            SvcDue::Release(n) => self.release(n),
            SvcDue::Presence(epoch) => {
                if epoch == self.published.epoch {
                    self.published.busy = false;
                    self.publish(now);
                }
            }
            SvcDue::QueueHold => self.relay_check(now),
            SvcDue::StopHearing => {
                self.heard_stop = None;
                self.stop_hearing();
            }
            SvcDue::Heard => self.answer_browses(now),
            SvcDue::Addresses => self.tell_direct(),
        }
    }

    /// The end of a step: Epic's relay control looked at when a step asked for it (the TS's queued microtask), and
    /// every backend's accept set as the sessions and parties on it have their peers now.
    fn settle(&mut self, now: f64) {
        if self.relay_check_queued {
            self.relay_check_queued = false;
            self.relay_check(now);
        }
        self.settle_accepts();
    }

    /// Each backend's accept set from the sessions' and the parties' peers, set where it changed.
    fn settle_accepts(&mut self) {
        let vias: Vec<Via> =
            self.backends.keys().map(|k| Via::Backend(*k)).chain(self.own.keys().map(|n| Via::Own(*n))).collect();
        for via in vias {
            let mut set = IndexSet::new();
            for p in self.pages.values() {
                if let Some(s) = p.session.as_ref().filter(|s| s.base().via == Some(via)) {
                    set.extend(s.peers());
                }
                // (a session's match path links to its humans on Epic, only while it wants its links: no stranger is
                // accepted before the relay control counts the path or after it ends)
                if via == Via::Backend(BackendKind::Epic)
                    && let Some(path) = p.session.as_ref().filter(|s| !s.base().closed).and_then(Session::path)
                    && path.open_as().is_some()
                {
                    set.extend(path.epic_peers());
                }
                if let Some(x) = p.party.as_ref().filter(|x| x.via() == via) {
                    set.extend(x.peers());
                }
            }
            if self.accepts.get(&via) != Some(&set) {
                if let Some(b) = self.backend(via) {
                    lock(&b).set_accept(set.clone());
                }
                self.accepts.insert(via, set);
            }
        }
    }

    /// Every session and party ends (`Stopped`): the app's quit.
    pub fn shutdown(&mut self, now: f64) {
        let pages: Vec<PageId> = self.pages.keys().copied().collect();
        for p in pages {
            self.with_session(p, now, |svc, s| s.end(svc, ClosedWhy::Stopped, now));
            self.with_party(p, now, |svc, x| x.end(svc, ClosedWhy::Stopped, now));
        }
        for (id, mut p) in std::mem::take(&mut self.pages) {
            // (the pages' last events, their sessions' 'closed', are still handed over)
            self.gone_events.push((id, std::mem::take(&mut p.events)));
            if let Some(s) = p.session.take() {
                self.retire(s);
            }
            if let Some(x) = p.party.take() {
                self.retire_party(x);
            }
        }
        if let Some(t) = self.address_timer.take() {
            self.timers.cancel(t);
        }
        self.stop_hearing();
        self.settle(now);
    }
}

/// Whose timer is first.
#[derive(Clone, Copy, Debug)]
enum Due {
    Service,
    Session(PageId),
    Party(PageId),
    Retired(usize),
    RetiredParty(usize),
}

/// The service's own calls the sessions and parties make of it. `but`: the page whose session (or party) asks, left
/// out.
impl NetService {
    /// An event to a page (dropped when the page is gone).
    pub(crate) fn post(&mut self, page: PageId, e: NetEvent) {
        if let Some(p) = self.pages.get_mut(&page) {
            p.events.push(e);
        }
    }

    pub(crate) fn log(&mut self, line: &str) {
        if let Some(l) = self.o.log.as_mut() {
            l(line);
        }
    }

    /// The wall clock, ms since 1970.
    pub(crate) fn wall(&self) -> f64 {
        match &self.o.wall {
            Some(w) => w(),
            None => std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
                .floor(),
        }
    }

    pub(crate) fn config(&self) -> &ServiceConfig {
        &self.config
    }

    pub(crate) fn host_times(&self) -> HostTimes {
        self.o.host_times
    }

    /// A host core's draws.
    pub(crate) fn host_rand(&mut self) -> Box<dyn mvs_host::HostRand> {
        match self.o.host_rand.as_mut() {
            Some(f) => f(),
            None => Box::new(mvs_host::SystemRand),
        }
    }

    /// A log sink for a host core (the service's own lines go to the same place).
    pub(crate) fn host_log(&self) -> Option<HostLog> {
        None
    }

    /// This player's card in a party: its presence's name and icon ('' before the page sent one).
    pub(crate) fn card(&self) -> (String, String) {
        self.presence_now.as_ref().map_or((String::new(), String::new()), |p| (p.name.clone(), p.icon.clone()))
    }

    /// The fighter this player's main menu shows, on its party card (None before the page sent one).
    pub(crate) fn menu_fighter(&self) -> Option<MenuFighter> {
        self.fighter_now.clone()
    }

    /// The signed-in backend of `kind` (Epic or the relay), None while it is off.
    pub(crate) fn eos_of(&self, kind: BackendKind) -> Option<Shared> {
        self.backends.get(&kind).cloned()
    }

    /// The backend a session's `via` names (None: gone).
    pub(crate) fn backend(&self, via: Via) -> Option<Shared> {
        match via {
            Via::Backend(k) => self.backends.get(&k).cloned(),
            Via::Own(n) => self.own.get(&n).map(|l| l.backend.clone()),
        }
    }

    /// This player's id on `via` ('' none).
    pub(crate) fn me_on(&self, via: Via) -> String {
        self.backend(via).map(|b| lock(&b).me().to_string()).unwrap_or_default()
    }

    /// One packet on `via`; false when the backend refused it or is gone.
    pub(crate) fn send(&self, via: Via, to: &str, ch: Channel, bytes: &[u8]) -> bool {
        self.backend(via).is_some_and(|b| lock(&b).send(to, ch, bytes))
    }

    /// The open sessions on `via` in the pages.
    fn others_on(&self, via: Via) -> impl Iterator<Item = &Session> {
        // (the asking session is out of its page for its call, or let go: a page's new session is another)
        self.pages
            .values()
            .filter_map(|p| p.session.as_ref())
            .filter(move |s| s.base().via == Some(via) && !s.base().closed)
    }

    /// Another open session than the asking one, on `via`, is in that lobby.
    pub(crate) fn lobby_in_use(&self, via: Via, lobby: &str) -> bool {
        self.others_on(via).any(|s| s.in_lobby(lobby))
    }

    /// Another open session than the asking one, on `via`, has that peer, or a party on it does.
    pub(crate) fn peer_in_use(&self, via: Via, peer: &str) -> bool {
        self.others_on(via).any(|s| s.has_peer(peer)) || self.party_peer(via, peer, None)
    }

    /// An open session on `via` has that peer, or another party than `but`'s.
    pub(crate) fn peer_used(&self, via: Via, peer: &str, but: PageId) -> bool {
        let sessions = self
            .pages
            .values()
            .filter_map(|p| p.session.as_ref())
            .any(|s| s.base().via == Some(via) && !s.base().closed && s.has_peer(peer));
        sessions || self.party_peer(via, peer, Some(but))
    }

    fn party_peer(&self, via: Via, peer: &str, but: Option<PageId>) -> bool {
        self.pages
            .values()
            .filter(|p| Some(p.id) != but)
            .filter_map(|p| p.party.as_ref())
            .any(|x| x.via() == via && x.has_peer(peer))
    }

    /// Another session than `but`'s, on `via`, plays a match under that referee.
    pub(crate) fn referee_in_use(&self, via: Via, referee: &str, but: PageId) -> bool {
        self.pages
            .values()
            .filter(|p| p.id != but)
            .filter_map(|p| p.session.as_ref())
            .any(|s| s.base().via == Some(via) && s.base().match_route.as_ref().is_some_and(|m| m.referee == referee))
    }

    /// A session's invite out on `via`; its refusal comes to the page as InviteRefused.
    pub(crate) fn send_invite(&mut self, via: Via, page: PageId, to: &str, payload: &str) {
        let Some(b) = self.backend(via) else { return };
        let ask = lock(&b).invite(to, payload);
        self.asks_out.insert((via, ask), SvcAsk::Invite { page, to: to.to_string() });
    }

    /// The clock's zero (a relay link's client's).
    pub(crate) fn epoch(&self) -> std::time::Instant {
        self.o.epoch
    }

    /// Who sent the party invite of that code ('' none).
    pub(crate) fn party_inviter(&self, code: &str) -> String {
        self.party_invites.get(code).cloned().unwrap_or_default()
    }

    /// A link of a session's own let go `ms` from now (RELEASE_MS after its session's end).
    pub(crate) fn release_later(&mut self, n: u32, ms: f64, now: f64) {
        self.timers.after(now, ms, SvcDue::Release(n));
    }

    /// The page's questions are dropped (its session ended).
    pub(crate) fn drop_asks(&mut self, page: PageId) {
        if let Some(p) = self.pages.get_mut(&page) {
            p.asks.clear();
        }
    }
}

/// A page's port to a service on its own thread (net.ts): its requests in, its events out.
pub struct PagePort {
    pub tx: Sender<NetRequest>,
    pub rx: Receiver<NetEvent>,
}

/// What the game's handle and the pages tell the service's thread.
enum ToService {
    Page(Sender<NetEvent>, Receiver<NetRequest>),
    Backends(Box<OnlineBackends>, DirectOptions),
    /// The player's change: at once when no session is open, else once it closes.
    BackendsWhenFree(Box<OnlineBackends>, DirectOptions),
    Quit,
}

/// What the service's thread keeps for the handle: the backends' states, the relays said yes to, the LAN host's.
#[derive(Default)]
struct HandleState {
    states: Vec<BackendState>,
    known: Vec<KnownRelay>,
    direct: Option<DirectStatus>,
}

/// The game's handle on a service on its own thread (netproc.ts and main's side of it).
pub struct NetHandle {
    tx: Sender<ToService>,
    state: Arc<Mutex<HandleState>>,
    quit: Arc<AtomicBool>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl NetHandle {
    /// A new page's port.
    pub fn page(&self) -> PagePort {
        let (req_tx, req_rx) = std::sync::mpsc::channel();
        let (ev_tx, ev_rx) = std::sync::mpsc::channel();
        let _ = self.tx.send(ToService::Page(ev_tx, req_rx));
        PagePort { tx: req_tx, rx: ev_rx }
    }

    /// Every backend of the online config, and this install's direct options (at the start and on each change).
    /// A change these replace that waits for a session to close is dropped.
    pub fn backends(&self, b: OnlineBackends, direct: DirectOptions) {
        let _ = self.tx.send(ToService::Backends(Box::new(b), direct));
    }

    /// The backends the player changed while the game runs: set at once when no custom lobby or queue is open, else
    /// held until it closes (`NetState::held` says so meanwhile); a newer change replaces one held.
    pub fn backends_when_free(&self, b: OnlineBackends, direct: DirectOptions) {
        let _ = self.tx.send(ToService::BackendsWhenFree(Box::new(b), direct));
    }

    /// Every backend's sign-in, as last told (the relay, Epic and LAN).
    pub fn state(&self) -> Vec<BackendState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).states.clone()
    }

    /// The relays the player said yes to since the last call, and the ids they proved (the app keeps them in
    /// Settings.online).
    pub fn relays_known(&self) -> Vec<KnownRelay> {
        std::mem::take(&mut self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).known)
    }

    /// The LAN host's state as last told (None: off), for the launcher.
    pub fn direct_status(&self) -> Option<DirectStatus> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).direct.clone()
    }

    /// Ends the service and its thread: every session ended (its byes out), the backends shut down.
    pub fn quit(&self) {
        if self.quit.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.tx.send(ToService::Quit);
        if let Some(t) = self.thread.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
            let _ = t.join();
        }
    }
}

impl Drop for NetHandle {
    fn drop(&mut self) {
        self.quit();
    }
}

/// The longest the service's thread sleeps between turns while nothing is due (its backends' sockets, the LAN
/// listener, are polled at least this often).
const IDLE_MS: f64 = 16.0;

/// Starts a service on a thread of its own (netproc.ts): its sign-ins (signin.rs), its pages' ports, its clock.
pub fn spawn(o: ServiceOptions) -> NetHandle {
    let (tx, rx) = std::sync::mpsc::channel::<ToService>();
    let state = Arc::new(Mutex::new(HandleState::default()));
    let quit = Arc::new(AtomicBool::new(false));
    let st = state.clone();
    let thread = std::thread::Builder::new()
        .name("mvs-net".into())
        .spawn(move || run(o, rx, st))
        .expect("the net service's thread starts");
    NetHandle { tx, state, quit, thread: Mutex::new(Some(thread)) }
}

/// The service's thread.
fn run(mut o: ServiceOptions, rx: Receiver<ToService>, state: Arc<Mutex<HandleState>>) {
    let epoch = o.epoch;
    let state_file = o.state_file.take();
    let record_relay = !o.dev;
    let mut told: Option<NetState> = None;
    let clock = move || epoch.elapsed().as_secs_f64() * 1000.0;
    o.epoch = epoch;
    let mut svc = NetService::new(o, clock());
    svc.signin = true;
    let mut signin =
        signin::SignIn::new(signin::SignInOptions { record_relay, ..signin::SignInOptions::system(epoch) });
    let mut pages: Vec<(PageId, Sender<NetEvent>, Receiver<NetRequest>)> = Vec::new();
    let mut quitting: Option<(f64, Vec<Shared>)> = None;
    loop {
        let now = clock();
        // (the handle's messages, then every page's requests)
        loop {
            match rx.try_recv() {
                Ok(ToService::Page(tx, rx)) => {
                    let id = svc.open_page(now);
                    pages.push((id, tx, rx));
                }
                Ok(ToService::Backends(b, d)) => {
                    svc.drop_held_backends();
                    svc.set_direct(Some(d));
                    signin.set(&mut svc, *b, now);
                }
                Ok(ToService::BackendsWhenFree(b, d)) => {
                    if let Some((b, d)) = svc.hold_backends(b, d) {
                        svc.set_direct(Some(d));
                        signin.set(&mut svc, *b, now);
                    }
                }
                Ok(ToService::Quit) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if quitting.is_none() {
                        let down = signin.quit();
                        svc.shutdown(now);
                        quitting = Some((now + BYE_MS + 200.0, down));
                    }
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
            }
        }
        let mut gone = Vec::new();
        for (id, _, rx) in &pages {
            loop {
                match rx.try_recv() {
                    Ok(m) => svc.take(*id, m, now),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        gone.push(*id);
                        break;
                    }
                }
            }
        }
        for id in gone {
            svc.close_page(id, now);
            pages.retain(|p| p.0 != id);
        }
        svc.pump(now);
        // (a change held while a session was open, once it is over)
        if let Some((b, d)) = svc.free_backends() {
            svc.set_direct(Some(d));
            signin.set(&mut svc, *b, now);
        }
        for n in svc.take_notes() {
            match n {
                ServiceNote::SignedOut(k, why) => signin.signed_out(&mut svc, k, &why, now),
                ServiceNote::Backend(BackendKind::Lan) => signin.lan_follows(&svc),
                ServiceNote::RelayKnown(a) => lock_state(&state).known.push(a),
                ServiceNote::Direct(d) => lock_state(&state).direct = d,
                ServiceNote::Backend(_) => {}
            }
        }
        signin.pump(&mut svc, now);
        if !signin.take_posts().is_empty() {
            svc.signin_follows(&signin.states());
        }
        lock_state(&state).states = signin.states();
        if let Some(file) = &state_file {
            let st = lock_state(&state);
            let now_state = NetState {
                backends: st.states.clone(),
                direct: st.direct.clone(),
                relay: svc.config.relay.clone(),
                udp: svc.udp(),
                held: svc.backends_held(),
                relay_proto: svc.relay_other_proto(),
            };
            drop(st);
            if told.as_ref() != Some(&now_state) {
                write_state(file, &now_state);
                told = Some(now_state);
            }
        }
        for (id, tx, _) in &pages {
            for e in svc.events(*id) {
                let _ = tx.send(e);
            }
        }
        if let Some((at, down)) = &quitting
            && clock() >= *at
        {
            for b in down {
                let _ = lock(b).shutdown();
            }
            // (the service's quit is the program's: EOS's platform, parked or in use, waits here for its lobby
            // leaves, then is released and the SDK shut down; nothing when EOS was never loaded)
            crate::backend::eos::end_eos();
            if let Some(file) = &state_file {
                let _ = std::fs::remove_file(file);
            }
            return;
        }
        let next = [svc.next_due(), svc.backends_due(), signin.next_due()]
            .into_iter()
            .flatten()
            .reduce(f64::min)
            .unwrap_or(now + IDLE_MS);
        let wait = (next - clock()).clamp(0.0, IDLE_MS);
        if wait > 0.0 {
            std::thread::sleep(std::time::Duration::from_secs_f64(wait / 1000.0));
        }
    }
}

/// The net state written whole: to a file beside it, then renamed over it (the launcher never reads half a file).
fn write_state(file: &Path, s: &NetState) {
    let Ok(text) = serde_json::to_vec(s) else { return };
    let part = file.with_extension("json.part");
    if std::fs::write(&part, text).is_ok() {
        let _ = std::fs::rename(&part, file);
    }
}

fn lock_state(s: &Arc<Mutex<HandleState>>) -> MutexGuard<'_, HandleState> {
    s.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::api::{BackendOff, BackendState, NetEvent, OffWhy, StatusWhy};
    use super::*;

    fn relay(why: OffWhy) -> BackendState {
        BackendState { kind: BackendKind::Relay, on: true, online: false, why: Some(why) }
    }

    #[test]
    fn a_relay_never_reached_is_in_the_status() {
        let mut svc = NetService::new(ServiceOptions::default(), 0.0);
        let page = svc.open_page(0.0);
        let _ = svc.events(page);
        let retrying = OffWhy::Retrying { text: "refused".into(), in_ms: 2000 };
        svc.signin_follows(&[relay(retrying.clone())]);
        let off = StatusWhy::Off { backends: vec![BackendOff { kind: BackendKind::Relay, why: retrying.clone() }] };
        assert_eq!(svc.events(page), vec![NetEvent::Status { online: false, me: String::new(), why: off }]);
        // (the same reason again: nothing told)
        svc.signin_follows(&[relay(retrying)]);
        assert!(svc.events(page).is_empty());
        // (Epic not in the config: not turned on, not "off")
        let epic = BackendState { kind: BackendKind::Epic, on: false, online: false, why: Some(OffWhy::Off) };
        svc.signin_follows(&[epic]);
        assert_eq!(svc.off_why(BackendKind::Epic), OffWhy::NotOn);
    }
}
