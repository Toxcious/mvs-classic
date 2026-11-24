//! A relay's rules (relay/core.ts): its players' lobbies, searches, invites and the links that carry their packets,
//! by id, the host cores it runs itself ([`crate::hosted`]: its queue's and its hosted lobbies'), the referees of
//! those cores' matches (`mvs_protocol::referee`), and each connection's presence, which the online list ('who') is
//! made of. Synchronous and sans-IO: the server owns it, calls it with the clock, and carries what it gives its players
//! ([`CorePlayer`]).

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use indexmap::{IndexMap, IndexSet};
use mvs_host::{HostEvent, HostOptions, HostWhy, RefereeStart, SessionKind};
use mvs_protocol::match_wire::{MAX_SLOTS, RESUME_MS};
use mvs_protocol::names::{BAD_WORDS, Profanity, clean_name};
use mvs_protocol::p2p_frame::{FRAME_OPEN, MessageJoiner, Taken as Frame, host_bye, split_message};
use mvs_protocol::referee::{Referee, RefereeOut, SlotNews};
use mvs_protocol::relay_wire::{
    Presence, PresenceStatus, QUEUE_HOST, RELAY_CODE_BYTES, RELAY_INFO_BYTES, SHORT_ALPHABET, SHORT_LEN, ServerMessage,
    WHO_LIMIT, Who, host_id, is_host_id, referee_id,
};
use serde_json::{Map, Value, json};

use crate::hosted::{HostLink, HostOut, Hosted, HostedOptions, Outbox, RelayConn, RelayReferee, Step, outbox};
use crate::limits::{Buckets, P2P_MAX_PACKET, RelayLimits, TokenBucket, address_group, relay_limits};

/// A live referee's pump runs on the relay's clock at this interval (ms), for the deadlines and the clock packets (an
/// input it takes is passed on at once: each packet taken is followed by a pump). Windows does not keep it: in plain
/// Node 22 on Windows 11 they came 15.5 ms apart (the system timer's period), so there a deadline or a clock packet
/// can be ruled up to about a tick late. Unverified: the port's own number.
pub const REFEREE_PUMP_MS: f64 = 4.0;
/// A member of a lobby the relay hosts a core for that has no link to it this long after its join (or, after its
/// link ended, after the end of its relink wait) is kicked by the relay: a lobby is not filled by members its core
/// never sees. `mvs-net`'s KNOCK_WAIT_MS (a hosting process's rule, `service/hosting.rs` watch). Unverified: the port's
/// own number.
pub const HOST_KNOCK_WAIT_MS: f64 = 12000.0;
/// A player whose link to a host core ended (its bye, its closePeer, the core's close) is not linked to it again for
/// this long: its knocks meanwhile are dropped, so a bye and a knock cannot give it the core's budgets of a new link.
/// `mvs-net`'s RELINK_MS (`service/hosting.rs` relink_wait). Unverified: the port's own number.
pub const HOST_RELINK_MS: f64 = 5000.0;
/// A player sent a match's packet in this long (ms) counts toward its sender's match bytes limit (`fan_out`): a game sends
/// each player of its match a packet a tick or more, and the beat (CLOCK_MS) while nothing is new.
pub const MATCH_FAN_MS: f64 = 1000.0;
const MAX_TEXT: usize = 128;
const MAX_SEARCH: usize = 100;
const MAX_INVITE_BYTES: usize = 500;
/// A player who asks this many times the rate of searches or invites is dropped; below it, over the rate, refused.
const ASKED_TIMES: f64 = 3.0;
/// The longest ProfileIconData asset name a presence may name (host/index.ts ASSET_NAME_MAX).
const ICON_MAX: usize = 96;
/// The names every JavaScript object has (`icon in {}`): an icon is none of them.
const OBJECT_NAMES: [&str; 12] = [
    "constructor",
    "__proto__",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

/// EOS's result names, as the online interface's other implementations reject with them.
pub const NOT_FOUND: &str = "EOS_NotFound";
pub const NOT_OWNER: &str = "EOS_Lobby_NotOwner";
pub const FULL: &str = "EOS_Lobby_TooManyPlayers";
pub const BAD: &str = "EOS_InvalidParameters";
pub const TOO_MANY: &str = "EOS_LimitExceeded";
/// A request of a session whose player has gone.
pub const GONE: &str = "gone";

/// The channel-0 data frames a seat's ring keeps for a resume, at most, and their bytes at most (the oldest go
/// first). Deviation: the port's own bounds.
pub const RING_FRAMES: usize = 1024;
pub const RING_BYTES: usize = 128 * 1024;
/// The messages a seat is owed at most (`Owed`); past them a resume within the time they are kept fails (a new seat).
/// Deviation: the port's own bound.
const MAX_HELD: usize = 512;

/// What the server made of a message or a packet the core gave a player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taken {
    Yes,
    /// The player's connection could take no more (it reads too slowly): the core lets it go at once, as a leave.
    Lost,
    /// (a packet) No handle was left for one more peer: the core closes that link from this player's side, and the
    /// player is told peerClosed for it.
    NoHandle,
}

/// `CorePlayer`: how the core reaches one player. Its calls never call the core back: what they need of it they
/// answer with [`Taken`].
pub trait CorePlayer {
    /// A lobby change, an invite or a peer closed (the server's own messages, res / peer / ping, are never sent here).
    fn send(&mut self, m: &ServerMessage) -> Taken;
    /// A packet from another player (or a referee) on channel 0 or 1.
    fn packet(&mut self, channel: u8, from: &str, bytes: &[u8]) -> Taken;
    /// The player passed a limit: the server tells it refused { why } and closes (the core has let it go already).
    fn refuse(&mut self, why: &str);
}

/// One player's way in (`CoreSession`): the calls on [`Core`] name it. A session whose player is gone (left, dropped,
/// or its id signed in again) does nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    sid: u64,
}

/// Where a player's seat is. Deviation: the seat is the port's own rule (the build has no way back into a match): a
/// connection that drops without a bye parks its player for [`RESUME_MS`], its lobbies, links and matches kept, and a
/// new connection of the same key takes the seat over.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Seat {
    /// Its connection is up: what the core gives it is sent.
    Live,
    /// Its connection dropped without a bye (at `since`): nothing is sent to it; what a resume needs is owed.
    Parked { since: f64 },
    /// A new connection took the seat over ([`Core::join`]); [`Core::resume`] decides whether it is resumed.
    Joined,
    /// The seat is the new connection's, its welcome not yet sent: [`Core::resumed`] sends what it is owed (the ring
    /// from `from` on, for a resumed seat).
    Welcome { from: Option<u64> },
}

/// A message a seat may be owed at a resume, with `sent` at its time (`at`: it goes before the ring's frame of that
/// number). A closing (a lobby closed or kicked from, a link closed, a gone slot of its match) is kept for silentMs +
/// RESUME_MS, whether the seat was live or not: a connection can be dead a while before the relay finds it silent,
/// and what was written to it is lost. A resume tells again each from the hello's count on (one before it reached the
/// client: a frame after it on the same stream did); one whose thing opens again (the link, the lobby, the match)
/// is no longer owed. An invite (`once`) is owed only while the seat was not live, until it is sent.
#[derive(Clone)]
struct Owed {
    seq: u64,
    at: u64,
    when: f64,
    m: ServerMessage,
    once: bool,
}

struct Player {
    id: String,
    sid: u64,
    address: String,
    p: Box<dyn CorePlayer>,
    gone: bool,
    seat: Seat,
    /// The resume rule (`relay_wire::ClientMessage::Hello`, as the relay client keeps it too). The unit is a channel-0
    /// data frame (FT_DATA) of the seat, whatever its handle; one dropped for a dead handle still counts. `sent`: the
    /// frames given toward the seat, over all its connections and while parked (those go to no connection: the ring
    /// keeps them), the last of them in `ring` as (from, bytes), frame `sent - ring.len()` first; one that no handle
    /// was left for is taken out again, at once or in a resend. The hello's `got` is the frames the client read off its
    /// connections on the seat; a resume resends the ring from exactly `got` on, in its order, each named by the new
    /// connection's handles (the peer message first): the client may read a few more on the old connection after its
    /// hello and skips that many. `took`: the frames read from the seat's connections up to the take-over (the old
    /// connection is read no more from it), the welcome's `resumed`, from which the client resends its own ring. A
    /// `got` the ring no longer covers fails the resume (a new seat).
    sent: u64,
    ring: VecDeque<(String, Vec<u8>)>,
    ring_bytes: usize,
    took: u64,
    /// What a resume owes it, oldest first (`Owed`).
    owed: VecDeque<Owed>,
    next_owed: u64,
    /// When more than [`MAX_HELD`] were owed last: a resume while that is kept fails (a new seat).
    overflow_at: Option<f64>,
    /// The lobbies it is a member of.
    lobbies: IndexSet<String>,
    /// The players it has a link with.
    links: IndexSet<String>,
    /// The players that sent it a packet in their link's life: it may send to them though they share no lobby.
    heard: IndexSet<String>,
    /// Its searches, invites and joins taken; the searches and invites asked, taken or refused (past those: dropped).
    searches: TokenBucket,
    invites: TokenBucket,
    joins: TokenBucket,
    searches_asked: TokenBucket,
    invites_asked: TokenBucket,
    /// The invites it is sent, and when each player last sent it one (only those of the last inviteAgainMs kept).
    invited: TokenBucket,
    invited_by: IndexMap<String, f64>,
    bytes: [TokenBucket; 2],
    /// The players it sent a match's packet (channel 1) lately, and when it last did: its channel 1 rate is the limit's
    /// for each (`fan_out`).
    match_to: IndexMap<String, f64>,
    /// What the online list shows of it (`None`: none sent yet, so not listed and invited as any id), when it last
    /// changed, and its online list asks taken.
    presence: Option<Presence>,
    presence_at: f64,
    whos: TokenBucket,
}

/// A match refereed on the relay: its id ([`referee_id`]), its owner (the host id of the core that opened it) and the
/// core's number for it, its humans' ids by slot and slots by id (one taken out: out of both), the Referee. `live`
/// until it ends: an ended one sends nothing more, though a pump of it is under way. `busy` while the Referee is in a
/// call (taken out of `referee` meanwhile): a human taken out meanwhile waits in `out`.
struct Refereed {
    /// Which opening of this id it is (one ended and opened again in a call is another).
    serial: u64,
    owner: String,
    r#match: u32,
    ids: IndexMap<usize, String>,
    slots: IndexMap<String, usize>,
    referee: Option<Referee>,
    live: bool,
    busy: bool,
    out: Vec<(String, bool)>,
    /// Its pump has panicked once (logged once).
    failed: bool,
}

struct Lobby {
    id: String,
    bucket: String,
    owner: String,
    /// The owner's address group (lobbiesPerAddress).
    group: String,
    max: u32,
    members: Vec<String>,
    /// Found by searches, with its attributes; `None`: by its id alone.
    listed: Option<ListedAt>,
    /// (a hosted lobby) Its short code, which a join may name it by ([`Core::shorts`]).
    short: Option<String>,
}

/// A listing's attributes; `at` as the request gave the number.
#[derive(Clone, Debug)]
struct ListedAt {
    at: serde_json::Number,
    info: String,
    code: String,
}

/// JavaScript's `.length` of a string.
fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `text`: a string of 1 to MAX_TEXT UTF-16 units.
fn text(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty() && js_len(s) <= MAX_TEXT)
}

/// `Number.isFinite`. The server reads a JSON number past f64's range (JS's Infinity) as +-f64::MAX, so that is
/// refused too. Deviation: so is a number written as exactly +-f64::MAX, which the TS takes.
pub(crate) fn finite(f: f64) -> bool {
    f.is_finite() && f.abs() < f64::MAX
}

/// A whole number (a JSON number with no fraction, as `Number.isInteger` takes it).
fn whole(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|&f| finite(f) && f.fract() == 0.0)
}

/// `count`: a whole number from 1 to `most`.
fn count(v: Option<&Value>, most: f64) -> Option<f64> {
    whole(v).filter(|&n| (1.0..=most).contains(&n))
}

const MAX_SAFE_INTEGER: f64 = 9007199254740991.0;

/// `listedOf`: a listing's attributes as a request names them; `None` when they are not of that shape.
fn listed_of(v: Option<&Value>) -> Option<ListedAt> {
    let o = v?.as_object()?;
    let at = match o.get("at") {
        Some(Value::Number(n)) if n.as_f64().is_some_and(finite) => n.clone(),
        _ => return None,
    };
    let field = |k: &str, most: usize| -> Option<String> {
        match o.get(k) {
            None => Some(String::new()),
            Some(Value::String(s)) if s.len() <= most => Some(s.clone()),
            _ => None,
        }
    };
    Some(ListedAt { at, info: field("info", RELAY_INFO_BYTES)?, code: field("code", RELAY_CODE_BYTES)? })
}

/// `presenceOf`: a presence as a request gives it: the name cleaned as the host core cleans a hello's and none of the
/// words of the bad-word list in it, the icon '' or an asset name (letters, digits, '_' and '-', no name every object
/// has); `None` when it is not of that shape or the name is empty or refused.
fn presence_of(v: Option<&Value>, refused: &Profanity) -> Option<Presence> {
    let o = v?.as_object()?;
    let flag = |k: &str| o.get(k).and_then(Value::as_bool);
    let (busy, listed, invites) = (flag("busy")?, flag("listed")?, flag("invites")?);
    let icon = o.get("icon").and_then(Value::as_str)?;
    if icon.len() > ICON_MAX
        || !icon.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        || OBJECT_NAMES.contains(&icon)
    {
        return None;
    }
    let name = clean_name(o.get("name").and_then(Value::as_str).unwrap_or(""));
    if name.is_empty() || refused.refuses(&name) {
        return None;
    }
    // (the port's own status: optional, and an unknown word is kept as none, so an older or newer client's presence
    // is taken whole)
    let status = o.get("status").and_then(Value::as_str).and_then(PresenceStatus::of);
    Some(Presence { name, icon: icon.to_string(), busy, listed, invites, status })
}

/// JavaScript's `<` of two strings: by UTF-16 units.
fn js_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// The online list's order: by name without case, then by name, then by id (one order whatever the map's).
fn by_name(a: &Who, b: &Who) -> std::cmp::Ordering {
    js_cmp(&a.name.to_lowercase(), &b.name.to_lowercase())
        .then_with(|| js_cmp(&a.name, &b.name))
        .then_with(|| js_cmp(&a.id, &b.id))
}

fn lobby_id() -> String {
    let mut b = [0u8; 10];
    rand::fill(&mut b);
    let mut s = String::from("l");
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

/// The short-code joins of one address group that named no lobby a minute, at the most (past them a short code is
/// refused before it is read, a right one too). Its group is an IPv6 /48 (an IPv4 address alone): a /48 holds 65536 of
/// the join buckets' /64 groups, enough to guess codes at 20 joins a minute each. Unverified: the port's own number.
pub const SHORT_MISSES_PER_MINUTE: f64 = 20.0;

/// A short-code miss's group: an IPv6 address's /48, an IPv4 address itself ([`address_group`]'s /64 cut to 48 bits).
fn miss_group(address: &str) -> String {
    let g = address_group(address);
    match g.strip_suffix("::/64") {
        Some(v6) => v6.split(':').take(3).collect::<Vec<_>>().join(":") + "::/48",
        None => g,
    }
}

/// A text of a short code's form (SHORT_LEN characters of SHORT_ALPHABET).
fn is_short_form(t: &str) -> bool {
    t.len() == SHORT_LEN && t.bytes().all(|b| SHORT_ALPHABET.contains(&b))
}

/// A new short code: SHORT_LEN characters of SHORT_ALPHABET, each of the same odds (a random byte past the last whole
/// multiple of the alphabet is drawn again).
fn short_code() -> String {
    let n = SHORT_ALPHABET.len();
    let top = 256 / n * n;
    let mut s = String::with_capacity(SHORT_LEN);
    while s.len() < SHORT_LEN {
        let mut b = [0u8; 16];
        rand::fill(&mut b);
        for x in b.iter().map(|x| usize::from(*x)).filter(|x| *x < top).take(SHORT_LEN - s.len()) {
            s.push(char::from(SHORT_ALPHABET[x % n]));
        }
    }
    s
}

/// `createCore`: one relay's state.
pub struct Core {
    limits: RelayLimits,
    /// The clock of the call under way (ms).
    now: f64,
    log: Box<dyn FnMut(&str)>,
    name_refused: Profanity,
    next_sid: u64,
    next_serial: u64,
    players: IndexMap<String, Player>,
    lobbies: IndexMap<String, Lobby>,
    /// (wire 6) The hosted lobbies' ids by their short codes, made at the create and let go with the lobby.
    shorts: IndexMap<String, String>,
    /// Hosted lobbies get a short code (false: a direct host's, whose guests join by its long code).
    give_shorts: bool,
    /// The short-code joins that named no lobby, by [`miss_group`].
    short_misses: Buckets,
    /// The live referees, by referee id.
    refereed: IndexMap<String, Refereed>,
    /// The live referees each player id is a human of (by id, so a reconnect finds them).
    human_of: IndexMap<String, IndexSet<String>>,
    /// The per-address rates and counts, by address group.
    group_invites: Buckets,
    group_joins: Buckets,
    group_whos: Buckets,
    group_lobbies: IndexMap<String, u32>,
    /// A connection that drops without a bye parks its player ([`Core::park`]); false: it leaves (a direct host's
    /// guests, whose links are LAN ones).
    seats: bool,
    /// The host cores it may run (None: none, a relay that only carries packets).
    hosted: Option<HostedOptions>,
    /// The host cores it runs, by host id ([`QUEUE_HOST`], made at its first knock, and [`host_id`] of each hosted
    /// lobby).
    hosts: IndexMap<String, Hosted>,
    /// The live referees and those started by a core but not yet opened (`RelayReferee::running`): every host core's
    /// `MatchReferee::room`.
    running: Arc<AtomicUsize>,
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core")
            .field("players", &self.players.len())
            .field("lobbies", &self.lobbies.len())
            .field("matches", &self.refereed.len())
            .finish()
    }
}

/// Where a referee's packets go: the core with the referee's id (looked up again at each send, so one that ended in
/// the call sends nothing more).
struct ToHumans<'a> {
    core: &'a mut Core,
    rid: &'a str,
    serial: u64,
}

impl RefereeOut for ToHumans<'_> {
    fn send(&mut self, slot: usize, bytes: &[u8]) {
        // (nothing once it has ended, nor to a human taken out or gone, though the pump that sends is under way)
        let Some(r) = self.core.refereed.get(self.rid).filter(|r| r.serial == self.serial && r.live) else { return };
        let Some(h) = r.ids.get(&slot).cloned() else { return };
        // (a parked human is sent nothing: the referee keeps what it lacks)
        let Some(q) = self.core.players.get_mut(&h).filter(|q| !q.gone && q.seat == Seat::Live) else { return };
        let taken = q.p.packet(1, self.rid, bytes);
        self.core.after_packet(&h, self.rid, taken);
    }

    /// To the owner, the host core in this process: an away, back or gone slot ([`Core::host_news`]).
    fn news(&mut self, slot: usize, n: SlotNews) {
        let Some(r) = self.core.refereed.get(self.rid).filter(|r| r.serial == self.serial && r.live) else {
            return;
        };
        let (owner, r#match) = (r.owner.clone(), r.r#match);
        let Ok(slot) = u32::try_from(slot) else { return };
        self.core.host_news(&owner, r#match, slot, n);
    }
}

impl Core {
    /// `createCore({ limits, log })`: the limits over the defaults (`relay_limits`); `hosted` the host cores it runs.
    pub fn new(limits: &RelayLimits, hosted: Option<HostedOptions>, log: Box<dyn FnMut(&str)>) -> Core {
        let limits = relay_limits(limits);
        Core {
            group_invites: Buckets::per_minute(limits.invites_per_address),
            group_joins: Buckets::per_minute(limits.joins_per_address),
            group_whos: Buckets::per_minute(limits.who_per_address),
            limits,
            now: 0.0,
            log,
            name_refused: Profanity::new(&BAD_WORDS),
            next_sid: 1,
            next_serial: 1,
            players: IndexMap::new(),
            lobbies: IndexMap::new(),
            shorts: IndexMap::new(),
            give_shorts: true,
            short_misses: Buckets::per_minute(SHORT_MISSES_PER_MINUTE),
            refereed: IndexMap::new(),
            human_of: IndexMap::new(),
            group_lobbies: IndexMap::new(),
            seats: true,
            hosted,
            hosts: IndexMap::new(),
            running: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// The host cores it runs (None: none).
    pub fn hosted_options(&self) -> Option<&HostedOptions> {
        self.hosted.as_ref()
    }

    pub fn limits(&self) -> &RelayLimits {
        &self.limits
    }

    /// The players signed in.
    pub fn players(&self) -> usize {
        self.players.len()
    }

    /// (tests, the log) The lobbies held.
    pub fn lobbies(&self) -> usize {
        self.lobbies.len()
    }

    /// (tests, the log) The live referees.
    pub fn matches(&self) -> usize {
        self.refereed.len()
    }

    fn log(&mut self, line: &str) {
        (self.log)(line);
    }

    /// The live player of a session.
    fn me(&self, s: &Session) -> Option<&Player> {
        self.players.get(&s.id).filter(|p| p.sid == s.sid && !p.gone)
    }

    /// (tests, the log) The players parked.
    pub fn parked(&self) -> usize {
        self.players.values().filter(|p| matches!(p.seat, Seat::Parked { .. })).count()
    }

    /// The player of that id is connected and sent what the core gives it.
    fn is_live(&self, id: &str) -> bool {
        self.players.get(id).is_some_and(|q| !q.gone && q.seat == Seat::Live)
    }

    /// `tell`: a message to a connected player; one that cannot take it is parked (or let go: [`Core::park`]). A
    /// player not live is sent nothing. A closing is owed to the seat (`Owed`) whether it went or not, an invite
    /// while it does not go; the rest a resume tells anew (its lobbies' members, its matches' away slots).
    fn tell(&mut self, id: &str, m: &ServerMessage) {
        let Some(q) = self.players.get_mut(id).filter(|q| !q.gone) else { return };
        let closing = matches!(m, ServerMessage::Lobby { closed: Some(_), .. } | ServerMessage::PeerClosed { .. });
        let mut went = false;
        if q.seat == Seat::Live {
            went = q.p.send(m) != Taken::Lost;
            if !went {
                self.park_id(id);
            }
        }
        if closing || (!went && matches!(m, ServerMessage::Invite { .. })) {
            self.owe(id, m, !closing);
        }
    }

    /// How long a closing is owed: the longest a dead connection is taken as live (silentMs) and its seat then kept.
    fn owed_ms(&self) -> f64 {
        self.limits.silent_ms + RESUME_MS
    }

    fn owe(&mut self, id: &str, m: &ServerMessage, once: bool) {
        let (now, keep) = (self.now, self.owed_ms());
        let Some(q) = self.players.get_mut(id).filter(|q| !q.gone) else { return };
        q.owed.retain(|o| o.once || now - o.when < keep);
        if q.owed.len() >= MAX_HELD {
            q.overflow_at = Some(now);
            return;
        }
        q.next_owed += 1;
        q.owed.push_back(Owed { seq: q.next_owed, at: q.sent, when: now, m: m.clone(), once });
    }

    /// What `what` names (a player linked, a lobby, a referee) is open to `id` again: its closing is owed no more.
    fn reopened(&mut self, id: &str, what: &str) {
        let Some(q) = self.players.get_mut(id) else { return };
        q.owed.retain(|o| match &o.m {
            ServerMessage::PeerClosed { id } => id != what,
            ServerMessage::Lobby { lobby, closed: Some(_), .. } => lobby != what,
            _ => true,
        });
    }

    /// A channel-0 data frame given toward `to`'s seat: its ring keeps it (the resume rule at `Player::sent`).
    fn record(&mut self, to: &str, from: &str, bytes: &[u8]) {
        let q = &mut self.players[to];
        q.sent += 1;
        q.ring.push_back((from.to_string(), bytes.to_vec()));
        q.ring_bytes += bytes.len();
        while q.ring.len() > RING_FRAMES || q.ring_bytes > RING_BYTES {
            let (_, b) = q.ring.pop_front().expect("a frame");
            q.ring_bytes -= b.len();
        }
    }

    /// The frame just recorded never went (no handle was left for its sender): out of the ring again.
    fn unrecord(&mut self, to: &str) {
        let q = &mut self.players[to];
        if let Some((_, b)) = q.ring.pop_back() {
            q.sent -= 1;
            q.ring_bytes -= b.len();
        }
    }

    /// What follows a packet given to `to` from `from`.
    fn after_packet(&mut self, to: &str, from: &str, taken: Taken) {
        match taken {
            Taken::Yes => {}
            Taken::Lost => {
                self.park_id(to);
            }
            Taken::NoHandle => {
                // no handle left for one more peer: that link is closed, both sides told
                self.close_peer_id(to, from);
                self.tell(to, &ServerMessage::PeerClosed { id: from.to_string() });
            }
        }
    }

    /// Every member but `skip` is told who is in the lobby now.
    fn tell_members(&mut self, lobby: &str, skip: &str) {
        let Some(l) = self.lobbies.get(lobby) else { return };
        let members = l.members.clone();
        for m in &members {
            if m != skip {
                self.tell(
                    m,
                    &ServerMessage::Lobby { lobby: lobby.to_string(), members: Some(members.clone()), closed: None },
                );
            }
        }
    }

    fn leave_lobby(&mut self, me: &str, lobby: &str) {
        if let Some(p) = self.players.get_mut(me) {
            p.lobbies.shift_remove(lobby);
        }
        let Some(l) = self.lobbies.get(lobby) else { return };
        if l.owner == me {
            // no host migration: the owner's leave destroys it (and ends its host core: its links told first)
            self.end_host(&host_id(lobby), HostWhy::HostLeft);
            self.destroy_lobby(lobby);
            return;
        }
        if let Some(l) = self.lobbies.get_mut(lobby) {
            l.members.retain(|m| m != me);
        }
        self.tell_members(lobby, me);
        // (a member out of a hosted lobby is out of its core: its link closed on the relay's side)
        self.host_link_gone(me, &host_id(lobby));
    }

    /// A lobby goes: each member still in it told 'destroyed' (one that left it already is not).
    fn destroy_lobby(&mut self, lobby: &str) {
        let Some(l) = self.lobbies.shift_remove(lobby) else { return };
        if let Some(s) = &l.short {
            self.shorts.shift_remove(s);
        }
        let n = self.group_lobbies.get(&l.group).copied().unwrap_or(1) - 1;
        if n > 0 {
            self.group_lobbies.insert(l.group.clone(), n);
        } else {
            self.group_lobbies.shift_remove(&l.group);
        }
        for m in &l.members {
            if !self.players.get_mut(m).is_some_and(|q| q.lobbies.shift_remove(lobby)) {
                continue;
            }
            let closed =
                ServerMessage::Lobby { lobby: lobby.to_string(), members: None, closed: Some("destroyed".into()) };
            self.tell(m, &closed);
            self.host_link_gone(m, &host_id(lobby));
        }
    }

    fn close_link(&mut self, me: &str, to: &str) {
        let Some(p) = self.players.get_mut(me) else { return };
        if !p.links.shift_remove(to) {
            return;
        }
        p.heard.shift_remove(to);
        if let Some(peer) = self.players.get_mut(to) {
            peer.links.shift_remove(me);
            peer.heard.shift_remove(me);
        }
        self.tell(to, &ServerMessage::PeerClosed { id: me.to_string() });
    }

    /// `remove`: a player gone: out of every lobby and link (a host core's link closed: the core takes it out of what
    /// it was in), the others told. The player taken out of the map, for a drop to tell.
    fn remove(&mut self, me: &str) -> Option<Player> {
        let p = self.players.get_mut(me).filter(|p| !p.gone)?;
        p.gone = true;
        let lobbies: Vec<String> =
            self.players.get(me).map(|p| p.lobbies.iter().cloned().collect()).unwrap_or_default();
        for id in lobbies {
            if self.lobbies.contains_key(&id) {
                self.leave_lobby(me, &id);
            }
        }
        let links: Vec<String> = self.players.get(me).map(|p| p.links.iter().cloned().collect()).unwrap_or_default();
        for to in links {
            if self.hosts.contains_key(&to) {
                self.host_link_gone(me, &to);
            } else {
                self.close_link(me, &to);
            }
        }
        self.players.shift_remove(me)
    }

    /// `drop`: over a limit: let go, then told.
    fn drop_player(&mut self, me: &str, why: &str) {
        let Some(p) = self.players.get(me).filter(|p| !p.gone) else { return };
        let line = format!("{} {} dropped: {why}", p.address, p.id);
        self.log(&line);
        if let Some(mut p) = self.remove(me) {
            p.p.refuse(why);
        }
    }

    /// A referee's news of a slot of its owner's match: the owner is a host core in this process, told at once.
    fn host_news(&mut self, owner: &str, r#match: u32, slot: u32, news: SlotNews) {
        self.with_host(owner, |c, now| match news {
            SlotNews::Away => c.away(r#match, slot, now),
            SlotNews::Back => c.back(r#match, slot, now),
            SlotNews::Gone => c.gone(r#match, slot, now),
        });
    }

    /// `endMatch`: the referee of the owner's match goes (and sends nothing more, though one of its pumps is under
    /// way); its humans still in it are told peerClosed for its id (their handles for it freed).
    fn end_match(&mut self, owner: &str, r#match: u32) {
        let id = referee_id(owner, r#match);
        let Some(mut r) = self.refereed.shift_remove(&id) else { return };
        self.running.fetch_sub(1, Ordering::Relaxed);
        r.live = false;
        for h in r.ids.values() {
            if let Some(s) = self.human_of.get_mut(h) {
                s.shift_remove(&id);
                if s.is_empty() {
                    self.human_of.shift_remove(h);
                }
            }
        }
        for h in r.ids.values() {
            self.tell(h, &ServerMessage::PeerClosed { id: id.clone() });
        }
    }

    /// `inReferee`: runs `f` on the referee's Referee, then takes out the humans that were taken out meanwhile. A panic
    /// in `f` puts the referee back first, then goes on up (core.ts's `finally`): the caller's connection is closed.
    fn in_referee(&mut self, rid: &str, f: impl FnOnce(&mut Referee, f64, &mut ToHumans<'_>)) {
        let now = self.now;
        let Some(r) = self.refereed.get_mut(rid).filter(|r| r.live) else { return };
        // (a call from inside a call of the same referee cannot come: the players' sinks never call the core)
        let Some(mut referee) = r.referee.take() else { return };
        r.busy = true;
        let serial = r.serial;
        let done = catch_unwind(AssertUnwindSafe(|| f(&mut referee, now, &mut ToHumans { core: self, rid, serial })));
        if let Some(r) = self.refereed.get_mut(rid).filter(|r| r.serial == serial) {
            r.referee = Some(referee);
            r.busy = false;
        } else if done.is_ok() {
            return;
        }
        if let Err(e) = done {
            resume_unwind(e);
        }
        while let Some(r) = self.refereed.get_mut(rid).filter(|r| r.serial == serial && !r.out.is_empty()) {
            let (id, tell) = r.out.remove(0);
            self.take_out(rid, &id, tell);
        }
    }

    /// `takeOut`: a human out of the referee's match: its stream ends at the ruled line (Referee::left, alike for every
    /// peer; the others get it at the next pump, as in the host's own process), it is sent nothing more (`tell`: told
    /// peerClosed for the referee's id). The referee ends with its last human.
    fn take_out(&mut self, rid: &str, id: &str, tell: bool) {
        let now = self.now;
        let Some(r) = self.refereed.get_mut(rid).filter(|r| r.live) else { return };
        if r.busy {
            r.out.push((id.to_string(), tell));
            return;
        }
        let Some(slot) = r.slots.shift_remove(id) else { return };
        r.ids.shift_remove(&slot);
        if let Some(rf) = r.referee.as_mut() {
            rf.left(slot, now);
        }
        let (owner, r#match, empty) = (r.owner.clone(), r.r#match, r.ids.is_empty());
        if let Some(s) = self.human_of.get_mut(id) {
            s.shift_remove(rid);
            if s.is_empty() {
                self.human_of.shift_remove(id);
            }
        }
        if tell {
            self.tell(id, &ServerMessage::PeerClosed { id: rid.to_string() });
        }
        if empty {
            self.end_match(&owner, r#match);
        }
    }

    fn shares_lobby(&self, me: &str, to: &str) -> bool {
        let Some(p) = self.players.get(me) else { return false };
        p.lobbies.iter().any(|l| self.lobbies.get(l).is_some_and(|l| l.members.iter().any(|m| m == to)))
    }

    /// The pump of one referee (its timer's run in the TS), after a human's packet when one is given. A panic in the
    /// pump is caught and logged once a match (core.ts `run`); one in the packet's `receive` goes on up (`in_referee`).
    fn run(&mut self, rid: &str, input: Option<(usize, &[u8])>) {
        let mut panicked = false;
        self.in_referee(rid, |referee, now, out| {
            if let Some((slot, bytes)) = input {
                referee.receive(slot, bytes, now, out);
            }
            panicked = catch_unwind(AssertUnwindSafe(|| referee.pump(now, out))).is_err();
        });
        // (logged once a match: the pump runs every few ms)
        let Some(r) = self.refereed.get_mut(rid).filter(|r| panicked && !r.failed) else { return };
        r.failed = true;
        self.log(&format!("{rid} referee: internal error: a panic"));
    }

    /// A host core's match goes (its `HostOut::Open`): refereed under `referee_id(host, match)`, its humans those the
    /// core names (counted in `running` at its start already). The same number again replaces its referee, as the host's
    /// own process does. Past the relay's `matches` it opens all the same (a start the core made though it was told no
    /// room): a referee every match.go names exists; the overshoot is logged.
    fn open_hosted(&mut self, host: &str, m: &RefereeStart) {
        let rid = referee_id(host, m.match_id);
        if self.refereed.contains_key(&rid) {
            self.end_match(host, m.match_id);
        }
        let mut ids: IndexMap<usize, String> = IndexMap::new();
        let mut slots: IndexMap<String, usize> = IndexMap::new();
        for h in &m.humans {
            let slot = h.slot as usize;
            if ids.contains_key(&slot) || slots.contains_key(&h.id) {
                continue;
            }
            ids.insert(slot, h.id.clone());
            slots.insert(h.id.clone(), slot);
        }
        let humans: Vec<usize> = ids.keys().copied().collect();
        let serial = self.next_serial;
        self.next_serial += 1;
        let x = Refereed {
            serial,
            owner: host.to_string(),
            r#match: m.match_id,
            ids,
            slots,
            referee: Some(Referee::new(usize::try_from(m.slots).unwrap_or(0), &humans, m.tick0)),
            live: true,
            busy: false,
            out: Vec::new(),
            failed: false,
        };
        let ids: Vec<String> = x.slots.keys().cloned().collect();
        for id in &ids {
            self.human_of.entry(id.clone()).or_default().insert(rid.clone());
            self.reopened(id, &rid);
        }
        self.refereed.insert(rid, x);
        if self.refereed.len() as f64 > self.limits.matches {
            let n = self.refereed.len();
            self.log(&format!("{host} match {}: past the relay's matches ({n})", m.match_id));
        }
    }

    /// A host core starts under `id` (its lobby's, or None: the queue's or a test's): its referee the relay's own
    /// ([`RelayReferee`]), its log lines the relay's, after the host id. What its making left is carried out.
    pub fn start_host(&mut self, id: &str, lobby: Option<String>, mut o: HostOptions, now: f64) {
        self.now = now;
        let out: Outbox = Outbox::default();
        o.referee = Some(Box::new(RelayReferee {
            host: id.to_string(),
            out: out.clone(),
            running: self.running.clone(),
            limit: self.limits.matches as usize,
        }));
        let logs = out.clone();
        o.log = Some(Box::new(move |line: &str| outbox(&logs).push(HostOut::Log(line.to_string()))));
        let core = mvs_host::Core::new(o, now);
        self.hosts.insert(id.to_string(), Hosted::new(core, out, lobby));
        self.with_host(id, |_, _| ());
    }

    /// (tests) Runs `f` on a host core, then carries out what it left. None: no such core, or the call panicked.
    pub fn with_host<R>(&mut self, id: &str, f: impl FnOnce(&mut mvs_host::Core, f64) -> R) -> Option<R> {
        let r = self.call_host(id, f);
        self.drain(id);
        r
    }

    /// (tests) The host cores' ids.
    pub fn hosts(&self) -> Vec<String> {
        self.hosts.keys().cloned().collect()
    }

    /// When a host core is due next (its own timers: `Core::next_due`), the soonest; None: no core waits on one.
    pub fn next_due(&self) -> Option<f64> {
        self.hosts.values().filter_map(|h| h.core.as_ref()?.next_due()).min_by(f64::total_cmp)
    }

    /// The pump of each host core due by `now`.
    pub fn pump_hosts(&mut self, now: f64) {
        self.now = now;
        let due: Vec<String> = (self.hosts.iter())
            .filter(|(_, h)| h.core.as_ref().and_then(mvs_host::Core::next_due).is_some_and(|t| t <= now))
            .map(|(id, _)| id.clone())
            .collect();
        for id in due {
            self.with_host(&id, |c, now| c.pump(now));
        }
    }

    /// One call of a host core, its outputs and events queued to carry out ([`Core::drain`]). A panic in it ends the
    /// core alone: logged, the core let go, the rest done as at its end.
    fn call_host<R>(&mut self, id: &str, f: impl FnOnce(&mut mvs_host::Core, f64) -> R) -> Option<R> {
        let now = self.now;
        let h = self.hosts.get_mut(id)?;
        let mut core = h.core.take()?;
        let r = catch_unwind(AssertUnwindSafe(|| f(&mut core, now)));
        let outs: Vec<HostOut> = std::mem::take(&mut *outbox(&h.out));
        h.steps.extend(outs.into_iter().map(Step::Out));
        let Ok(r) = r else {
            h.over = true;
            self.log(&format!("{id} host: internal error: a panic"));
            return None;
        };
        h.steps.extend(core.take_events().into_iter().map(Step::Event));
        h.over |= core.ended();
        h.core = Some(core);
        Some(r)
    }

    /// Carries out what a host core's calls left, in order; a call made meanwhile (a step's own) only adds to it.
    /// A core that is over goes once that is done.
    fn drain(&mut self, id: &str) {
        let Some(h) = self.hosts.get_mut(id).filter(|h| !h.draining) else { return };
        h.draining = true;
        let carried = catch_unwind(AssertUnwindSafe(|| {
            while let Some(step) = self.hosts.get_mut(id).and_then(|h| h.steps.pop_front()) {
                self.carry(id, step);
            }
        }));
        let Some(h) = self.hosts.get_mut(id) else { return };
        h.draining = false;
        // (a panic carrying out a step ends that core as one in its call does)
        if carried.is_err() {
            h.over = true;
            h.steps.clear();
            self.log(&format!("{id} host: internal error: a panic"));
        }
        let Some(h) = self.hosts.get_mut(id) else { return };
        if h.over {
            self.finish_host(id);
        }
    }

    fn carry(&mut self, id: &str, step: Step) {
        match step {
            Step::Out(HostOut::Send { to, text }) => {
                let max = (self.limits.packet_bytes as usize).min(P2P_MAX_PACKET);
                for piece in split_message(text.as_bytes(), max) {
                    // (a piece that found no handle closed the link: the rest go nowhere)
                    if !self.hosts.get(id).is_some_and(|h| h.links.get(&to).is_some_and(|l| !l.done)) {
                        break;
                    }
                    self.host_frame(&to, id, &piece);
                }
            }
            Step::Out(HostOut::Close { to, why }) => {
                let Some(h) = self.hosts.get_mut(id) else { return };
                let Some(l) = h.links.get_mut(&to).filter(|l| !l.done) else { return };
                l.done = true;
                h.left_at.insert(to.clone(), self.now);
                let (over, lobby) = (h.over, h.lobby.clone());
                self.host_frame(&to, id, &host_bye(&why));
                // (a member the core let go is out of its lobby too; not at the core's end, where the lobby goes)
                if let Some(l) = lobby.filter(|_| !over) {
                    self.kick_member(&l, &to);
                }
            }
            Step::Out(HostOut::Open { m }) => self.open_hosted(id, &m),
            Step::Out(HostOut::Left { match_id, slot }) => {
                let rid = referee_id(id, match_id);
                let human = self.refereed.get(&rid).and_then(|r| r.ids.get(&(slot as usize)).cloned());
                if let Some(human) = human {
                    self.take_out(&rid, &human, true);
                }
            }
            Step::Out(HostOut::End { match_id }) => self.end_match(id, match_id),
            Step::Out(HostOut::Log(line)) => self.log(&format!("{id} {line}")),
            // (the core is over: it goes after its steps)
            Step::Event(HostEvent::End(_)) | Step::Event(HostEvent::Found(_)) => {}
            Step::Event(HostEvent::Kicked(member)) => {
                let Some(lobby) = self.hosts.get(id).and_then(|h| h.lobby.clone()) else { return };
                self.kick_member(&lobby, &member);
                // (its link: the bye, then the core's close)
                if self.hosts.get(id).is_some_and(|h| h.links.get(&member).is_some_and(|l| !l.done)) {
                    self.host_frame(&member, id, &host_bye(&HostWhy::Kicked));
                }
                self.host_link_gone(&member, id);
                self.owe_link_closed(&member, id);
            }
            Step::Event(HostEvent::Listing(info)) => {
                let Some(lobby) = self.hosts.get(id).and_then(|h| h.lobby.clone()) else { return };
                let Some(l) = self.lobbies.get_mut(&lobby) else { return };
                if let Some(listed) = l.listed.as_mut() {
                    listed.info = info.clone().unwrap_or_default();
                }
                let owner = l.owner.clone();
                self.tell(&owner, &ServerMessage::Listing { lobby, info });
            }
        }
    }

    /// A host core goes (its end, or a panic): a link it did not close told 'ended', its matches ended, its lobby
    /// destroyed.
    fn finish_host(&mut self, id: &str) {
        let Some(h) = self.hosts.shift_remove(id) else { return };
        for (to, l) in &h.links {
            if !l.done && self.players.get(to).is_some_and(|q| q.links.contains(id)) {
                self.host_frame(to, id, &host_bye(&HostWhy::Ended));
            }
            if self.players.get_mut(to).is_some_and(|q| q.links.shift_remove(id)) {
                self.owe_link_closed(to, id);
            }
        }
        let matches: Vec<u32> = self.refereed.values().filter(|r| r.owner == id).map(|r| r.r#match).collect();
        for m in matches {
            self.end_match(id, m);
        }
        if let Some(l) = h.lobby {
            self.destroy_lobby(&l);
        }
    }

    /// The core with that host id ends (`Core::end`), what it leaves carried out.
    fn end_host(&mut self, id: &str, why: HostWhy) {
        self.with_host(id, |c, now| c.end(why, now));
    }

    /// The queue core, made at its first knock (and again after one ended); none on a relay with no queue.
    fn ensure_queue(&mut self) {
        if self.hosts.contains_key(QUEUE_HOST) {
            return;
        }
        let Some(o) = self.hosted.as_mut().filter(|o| o.queue) else { return };
        let o = HostOptions {
            kind: SessionKind::OpenQueue,
            host: String::new(),
            name: None,
            lobby_id: None,
            times: o.times,
            referee: None,
            rand: (o.rand)(),
            log: None,
            paths: o.paths,
        };
        self.start_host(QUEUE_HOST, None, o, self.now);
    }

    /// A channel-0 frame from a host core to a player: the seat's (its ring keeps it), sent when it is live.
    fn host_frame(&mut self, to: &str, host: &str, bytes: &[u8]) {
        let Some(q) = self.players.get_mut(to).filter(|q| !q.gone) else { return };
        q.links.insert(host.to_string());
        self.record(to, host, bytes);
        if self.players[to].seat != Seat::Live {
            return;
        }
        let taken = self.players[to].p.packet(0, host, bytes);
        if taken == Taken::NoHandle {
            self.unrecord(to);
        }
        self.after_packet(to, host, taken);
    }

    /// A player's link to a host core is over on the relay's side: the core told (`Core::closed`) unless it closed
    /// it itself (`done`). A player's closePeer, its leave of the core's lobby, its seat's end, a kick.
    fn host_link_gone(&mut self, me: &str, host: &str) {
        if let Some(q) = self.players.get_mut(me) {
            q.links.shift_remove(host);
        }
        let now = self.now;
        let Some(h) = self.hosts.get_mut(host) else { return };
        let Some(l) = h.links.shift_remove(me) else { return };
        if l.done {
            return;
        }
        h.left_at.insert(me.to_string(), now);
        self.watch_member(host, me);
        let key = l.key;
        self.with_host(host, |c, now| c.closed(key, now));
    }

    /// A packet to a host core, on channel 0 (channel 1 is dropped): from a member of its lobby, or anyone to the
    /// queue's (none on a relay with no queue); else answered as one to no one. A knock (`FRAME_OPEN`) with no live
    /// link opens one (`Core::connect`); one on a live link does nothing. On a link the core closed the rest is
    /// dropped (its bye is on the way).
    fn packet_to_host(&mut self, me: &str, channel: u8, host: &str, bytes: &[u8]) {
        if channel != 0 {
            return;
        }
        let allowed = if host == QUEUE_HOST {
            self.hosted.as_ref().is_some_and(|o| o.queue)
        } else {
            (self.hosts.get(host).and_then(|h| h.lobby.as_ref()))
                .and_then(|l| self.lobbies.get(l))
                .is_some_and(|l| l.members.iter().any(|m| m == me))
        };
        if !allowed {
            return self.tell(me, &ServerMessage::PeerClosed { id: host.to_string() });
        }
        let knock = bytes == [FRAME_OPEN];
        let link = self.hosts.get(host).and_then(|h| h.links.get(me)).map(|l| l.done);
        match link {
            Some(false) => {}
            Some(true) if !knock => return,
            _ if !knock => return self.tell(me, &ServerMessage::PeerClosed { id: host.to_string() }),
            _ => return self.knock(me, host),
        }
        let h = self.hosts.get_mut(host).expect("a live link's core");
        let l = h.links.get_mut(me).expect("a live link");
        let key = l.key;
        match l.joiner.take(bytes) {
            Frame::Open | Frame::Part => {}
            Frame::Message(b) => {
                let text = String::from_utf8_lossy(&b).into_owned();
                self.with_host(host, |c, now| c.message(key, &text, now));
            }
            Frame::Bye(_) => self.host_link_gone(me, host),
            Frame::Bad => {
                self.host_link_gone(me, host);
                self.tell(me, &ServerMessage::PeerClosed { id: host.to_string() });
            }
        }
    }

    /// A knock with no live link: a new link to the core (the queue's made first, when it is not running).
    fn knock(&mut self, me: &str, host: &str) {
        if host == QUEUE_HOST {
            self.ensure_queue();
        }
        if !self.hosts.get(host).is_some_and(|h| h.core.is_some() && !h.over) {
            return self.tell(me, &ServerMessage::PeerClosed { id: host.to_string() });
        }
        // (inside its relink wait a knock is dropped: the player knocks again, as hosting.rs's guest does)
        if self.relink_wait(host, me) > 0.0 {
            return;
        }
        let h = &self.hosts[host];
        let conn = RelayConn { id: me.to_string(), name: String::new(), out: h.out.clone(), closed: false };
        self.players[me].links.insert(host.to_string());
        self.reopened(me, host);
        let Some(key) = self.call_host(host, |c, now| c.connect(Box::new(conn), now)) else {
            self.players[me].links.shift_remove(host);
            self.tell(me, &ServerMessage::PeerClosed { id: host.to_string() });
            return self.drain(host);
        };
        // (in the links before its outputs are carried out: its first message goes in the call)
        if let Some(h) = self.hosts.get_mut(host) {
            h.links.insert(me.to_string(), HostLink { key, joiner: MessageJoiner::new(), done: false });
            h.unlinked.shift_remove(me);
        }
        self.drain(host);
    }

    /// The ms before `me` may link to the core `host` again (0: it may now).
    fn relink_wait(&mut self, host: &str, me: &str) -> f64 {
        let now = self.now;
        let Some(h) = self.hosts.get_mut(host) else { return 0.0 };
        h.left_at.retain(|_, at| now - *at < HOST_RELINK_MS);
        h.left_at.get(me).map_or(0.0, |at| at + HOST_RELINK_MS - now)
    }

    /// A member of the core's lobby with no link (its join, its link's end) is kicked unless it links within
    /// [`HOST_KNOCK_WAIT_MS`] of the end of its relink wait. Not the lobby's owner (the core's host: its leave ends
    /// the core), nor a member of the queue's (it has no lobby).
    fn watch_member(&mut self, host: &str, me: &str) {
        let wait = HOST_KNOCK_WAIT_MS + self.relink_wait(host, me);
        let now = self.now;
        let Some(h) = self.hosts.get(host).filter(|h| !h.over && !h.links.contains_key(me)) else { return };
        let Some(l) = h.lobby.as_ref().and_then(|l| self.lobbies.get(l)) else { return };
        if l.owner == me || !l.members.iter().any(|m| m == me) {
            return;
        }
        self.hosts[host].unlinked.insert(me.to_string(), now + wait);
    }

    /// The members whose knock wait has passed, kicked from their lobby by the relay ([`HOST_KNOCK_WAIT_MS`]).
    fn kick_unlinked(&mut self, now: f64) {
        let mut due: Vec<(String, String, String)> = Vec::new();
        for (host, h) in &mut self.hosts {
            let Some(lobby) = h.lobby.clone() else { continue };
            let gone: Vec<String> = (h.unlinked.iter()).filter(|(_, at)| **at <= now).map(|(m, _)| m.clone()).collect();
            for m in gone {
                h.unlinked.shift_remove(&m);
                if !h.links.contains_key(&m) {
                    due.push((host.clone(), lobby.clone(), m));
                }
            }
        }
        for (host, lobby, m) in due {
            if self.lobbies.get(&lobby).is_some_and(|l| l.owner != m && l.members.contains(&m)) {
                self.log(&format!("{host} member never linked: kicked {m}"));
                self.kick_member(&lobby, &m);
            }
        }
    }

    /// A link a host core ended on the relay's side while its player may not hear the bye (parked, or dead before the
    /// relay finds it silent): a resume replays no frame of a sender no longer linked, so the closing is owed instead.
    fn owe_link_closed(&mut self, to: &str, host: &str) {
        self.owe(to, &ServerMessage::PeerClosed { id: host.to_string() }, false);
    }

    /// A member out of a lobby by its owner's side: told 'kicked', the rest the lobby's members.
    fn kick_member(&mut self, lobby: &str, member: &str) {
        let Some(x) = self.lobbies.get_mut(lobby) else { return };
        if x.owner == member || !x.members.iter().any(|m| m == member) {
            return;
        }
        x.members.retain(|m| m != member);
        if let Some(q) = self.players.get_mut(member) {
            q.lobbies.shift_remove(lobby);
        }
        let kicked = ServerMessage::Lobby { lobby: lobby.to_string(), members: None, closed: Some("kicked".into()) };
        self.tell(member, &kicked);
        self.tell_members(lobby, member);
    }

    fn request_of(&mut self, me: &str, r: &Map<String, Value>) -> Result<Value, &'static str> {
        let now = self.now;
        let lobby_named = |r: &Map<String, Value>| r.get("lobby").and_then(Value::as_str).map(str::to_string);
        match r.get("op").and_then(Value::as_str).unwrap_or("") {
            "create" => {
                let (Some(bucket), Some(max)) =
                    (text(r.get("bucket")), count(r.get("max"), self.limits.members_per_lobby))
                else {
                    return Err(BAD);
                };
                let listed = match r.get("listed") {
                    None => None,
                    v => Some(listed_of(v).ok_or(BAD)?),
                };
                // (a lobby the relay hosts itself, by a host core of its own: none on a relay that runs none)
                let hosted = match r.get("host") {
                    None | Some(Value::Bool(false)) => false,
                    Some(Value::Bool(true)) if self.hosted.is_some() => true,
                    Some(_) => return Err(BAD),
                };
                let p = &self.players[me];
                if p.lobbies.len() as f64 >= self.limits.lobbies_per_player {
                    return Err(TOO_MANY);
                }
                let group = address_group(&p.address);
                let owned = self.group_lobbies.get(&group).copied().unwrap_or(0);
                if f64::from(owned) >= self.limits.lobbies_per_address {
                    return Err(TOO_MANY);
                }
                let mut id = lobby_id();
                while self.lobbies.contains_key(&id) {
                    id = lobby_id();
                }
                let lobby = Lobby {
                    id: id.clone(),
                    bucket: bucket.to_string(),
                    owner: me.to_string(),
                    group: group.clone(),
                    max: max as u32,
                    members: vec![me.to_string()],
                    listed,
                    short: None,
                };
                self.lobbies.insert(id.clone(), lobby);
                self.group_lobbies.insert(group, owned + 1);
                self.players[me].lobbies.insert(id.clone());
                if !hosted {
                    return Ok(json!({ "lobby": id }));
                }
                let hid = host_id(&id);
                let o = self.hosted.as_mut().expect("hosted");
                let o = HostOptions {
                    kind: SessionKind::Custom,
                    host: me.to_string(),
                    name: None,
                    lobby_id: Some(id.clone()),
                    times: o.times,
                    referee: None,
                    rand: (o.rand)(),
                    log: None,
                    paths: o.paths,
                };
                self.start_host(&hid, Some(id.clone()), o, now);
                if !self.give_shorts {
                    return Ok(json!({ "lobby": id, "host": hid }));
                }
                let mut short = short_code();
                while self.shorts.contains_key(&short) {
                    short = short_code();
                }
                self.shorts.insert(short.clone(), id.clone());
                self.lobbies[&id].short = Some(short.clone());
                Ok(json!({ "lobby": id, "host": hid, "short": short }))
            }
            "search" => {
                let full = match r.get("full") {
                    None => false,
                    Some(Value::Bool(b)) => *b,
                    Some(_) => return Err(BAD),
                };
                let (Some(bucket), Some(limit)) = (text(r.get("bucket")), count(r.get("limit"), MAX_SAFE_INTEGER))
                else {
                    return Err(BAD);
                };
                if !self.players[me].searches_asked.take(1.0, now) {
                    self.drop_player(me, "too many searches");
                    return Err(TOO_MANY);
                }
                if !self.players[me].searches.take(1.0, now) {
                    return Err(TOO_MANY);
                }
                let most = (limit as usize).min(MAX_SEARCH);
                let mut out = Vec::new();
                for l in self.lobbies.values() {
                    if out.len() >= most {
                        break;
                    }
                    let Some(listed) = l.listed.as_ref() else { continue };
                    if l.bucket != bucket || l.owner == me || (!full && l.members.len() as u32 >= l.max) {
                        continue;
                    }
                    let mut found = json!({
                        "id": l.id, "owner": l.owner, "members": l.members.len(), "max": l.max,
                        "at": Value::Number(listed.at.clone()), "info": listed.info,
                    });
                    if !listed.code.is_empty() {
                        found["code"] = Value::String(listed.code.clone());
                    }
                    out.push(found);
                }
                Ok(json!({ "lobbies": out }))
            }
            "list" => {
                let l = lobby_named(r).ok_or(NOT_FOUND)?;
                let owner = self.lobbies.get(&l).ok_or(NOT_FOUND)?.owner.clone();
                if owner != me {
                    return Err(NOT_OWNER);
                }
                let listed = match r.get("listed") {
                    Some(Value::Null) => None,
                    v => Some(listed_of(v).ok_or(BAD)?),
                };
                self.lobbies[&l].listed = listed;
                Ok(json!({}))
            }
            "join" => {
                let named = lobby_named(r);
                let by_id = named.as_deref().filter(|l| self.lobbies.contains_key(*l));
                let member = by_id.is_some_and(|l| self.lobbies[l].members.iter().any(|m| m == me));
                // (each join asked counts, of a lobby gone or full too; one of a lobby the player is in already does
                // not)
                if !member {
                    let group = address_group(&self.players[me].address);
                    let p = self.players.get_mut(me).expect("a live player");
                    let of_group = self.group_joins.get(&group, now);
                    if !p.joins.can(1.0, now) || !of_group.can(1.0, now) {
                        return Err(TOO_MANY);
                    }
                    p.joins.take(1.0, now);
                    of_group.take(1.0, now);
                }
                // (a short code is read only past the charge: each guess costs a join of both buckets, a code
                // unknown too)
                let short = named.as_deref().and_then(|s| self.shorts.get(s)).cloned();
                // (a short code's form that names no lobby is a miss of its /48: past their limit no short code is
                // read)
                if by_id.is_none() && named.as_deref().is_some_and(is_short_form) {
                    let misses = self.short_misses.get(&miss_group(&self.players[me].address), now);
                    if !misses.can(1.0, now) {
                        return Err(TOO_MANY);
                    }
                    if short.is_none() {
                        misses.take(1.0, now);
                    }
                }
                let l = by_id.map(str::to_string).or_else(|| short.clone()).ok_or(NOT_FOUND)?;
                let member = member || self.lobbies[&l].members.iter().any(|m| m == me);
                if !member {
                    let x = &self.lobbies[&l];
                    if x.members.len() as u32 >= x.max {
                        return Err(FULL);
                    }
                    if self.players[me].lobbies.len() as f64 >= self.limits.lobbies_per_player {
                        return Err(TOO_MANY);
                    }
                    self.lobbies[&l].members.push(me.to_string());
                    self.watch_member(&host_id(&l), me);
                    self.players[me].lobbies.insert(l.clone());
                    self.reopened(me, &l);
                }
                let x = &self.lobbies[&l];
                let mut answer = json!({ "owner": x.owner, "members": x.members });
                if short.is_some() {
                    answer["lobby"] = Value::String(l.clone());
                }
                if !member {
                    self.tell_members(&l, me);
                }
                Ok(answer)
            }
            "leave" => {
                if let Some(l) = lobby_named(r)
                    && self.lobbies.get(&l).is_some_and(|x| x.members.iter().any(|m| m == me))
                {
                    self.leave_lobby(me, &l);
                }
                Ok(json!({}))
            }
            "kick" => {
                let (Some(l), Some(member)) = (lobby_named(r), r.get("member").and_then(Value::as_str)) else {
                    return Ok(json!({}));
                };
                let Some(x) = self.lobbies.get_mut(&l) else { return Ok(json!({})) };
                if x.owner != me || member == me || !x.members.iter().any(|m| m == member) {
                    return Ok(json!({}));
                }
                x.members.retain(|m| m != member);
                if let Some(q) = self.players.get_mut(member) {
                    q.lobbies.shift_remove(&l);
                }
                let kicked = ServerMessage::Lobby { lobby: l.clone(), members: None, closed: Some("kicked".into()) };
                self.tell(member, &kicked);
                self.tell_members(&l, me);
                self.host_link_gone(member, &host_id(&l));
                Ok(json!({}))
            }
            "invite" => {
                let (Some(to), Some(payload)) =
                    (r.get("to").and_then(Value::as_str), r.get("payload").and_then(Value::as_str))
                else {
                    return Err(BAD);
                };
                if !self.players[me].invites_asked.take(1.0, now) {
                    self.drop_player(me, "too many invites");
                    return Err(TOO_MANY);
                }
                // (a player whose presence takes no invites is answered as an id not connected, at the same point, so
                // the sender cannot tell; one with no presence yet takes them)
                if !self.players.get(to).is_some_and(|q| !q.gone && q.presence.as_ref().is_none_or(|p| p.invites)) {
                    return Err(NOT_FOUND);
                }
                if payload.len() > MAX_INVITE_BYTES {
                    return Err(BAD);
                }
                // (an invite refused takes from no rate but the asked one)
                if let Some(&last) = self.players[to].invited_by.get(me)
                    && now - last < self.limits.invite_again_ms
                {
                    return Err(TOO_MANY);
                }
                let group = address_group(&self.players[me].address);
                let of_group = self.group_invites.get(&group, now);
                let can = self.players[me].invites.can(1.0, now) && of_group.can(1.0, now);
                if !can || !self.players[to].invited.can(1.0, now) {
                    return Err(TOO_MANY);
                }
                self.players[me].invites.take(1.0, now);
                self.group_invites.get(&group, now).take(1.0, now);
                let again = self.limits.invite_again_ms;
                let q = &mut self.players[to];
                q.invited.take(1.0, now);
                q.invited_by.retain(|_, at| now - *at < again);
                q.invited_by.insert(me.to_string(), now);
                let invite = ServerMessage::Invite { from: me.to_string(), payload: payload.to_string() };
                self.tell(to, &invite);
                Ok(json!({}))
            }
            "presence" => {
                let p = presence_of(r.get("presence"), &self.name_refused).ok_or(BAD)?;
                let every = self.limits.presence_every_ms;
                let q = &mut self.players[me];
                // (the same again is no change: answered, and the clock not started again)
                if q.presence.as_ref() == Some(&p) {
                    return Ok(json!({}));
                }
                if q.presence.is_some() && now - q.presence_at < every {
                    return Err(TOO_MANY);
                }
                q.presence = Some(p);
                q.presence_at = now;
                Ok(json!({}))
            }
            "who" => {
                let limit = count(r.get("limit"), MAX_SAFE_INTEGER).ok_or(BAD)?;
                let group = address_group(&self.players[me].address);
                let of_group = self.group_whos.get(&group, now);
                if !self.players[me].whos.can(1.0, now) || !of_group.can(1.0, now) {
                    return Err(TOO_MANY);
                }
                of_group.take(1.0, now);
                self.players[me].whos.take(1.0, now);
                let mut all: Vec<Who> = self
                    .players
                    .values()
                    .filter(|q| q.id != me && !q.gone)
                    .filter_map(|q| q.presence.as_ref().filter(|p| p.listed).map(|p| (q, p)))
                    .map(|(q, p)| Who {
                        id: q.id.clone(),
                        name: p.name.clone(),
                        icon: p.icon.clone(),
                        busy: p.busy,
                        status: p.status,
                    })
                    .collect();
                all.sort_by(by_name);
                all.truncate((limit as usize).min(WHO_LIMIT as usize));
                Ok(json!({ "players": all }))
            }
            _ => Err(BAD),
        }
    }

    /// `toReferee`: a packet to a live referee: taken on channel 1 from a human of its match (the slot the sender's id
    /// has, never the packet's), then a pump; else dropped (a channel 0 one answered as a packet to no one).
    fn packet_to_referee(&mut self, me: &str, channel: u8, rid: &str, bytes: &[u8]) {
        let slot = self.refereed.get(rid).and_then(|r| r.slots.get(me).copied());
        let Some(slot) = slot.filter(|_| channel == 1) else {
            if channel == 0 {
                self.tell(me, &ServerMessage::PeerClosed { id: rid.to_string() });
            }
            return;
        };
        // (what it ruled goes out now, not at the timer's next pump)
        self.run(rid, Some((slot, bytes)));
    }

    /// A new player's state: no lobby, link or match, its limits full.
    fn new_player(&self, id: &str, sid: u64, address: &str, p: Box<dyn CorePlayer>, now: f64) -> Player {
        let l = &self.limits;
        Player {
            id: id.to_string(),
            sid,
            address: address.to_string(),
            p,
            gone: false,
            seat: Seat::Live,
            sent: 0,
            ring: VecDeque::new(),
            ring_bytes: 0,
            took: 0,
            owed: VecDeque::new(),
            next_owed: 0,
            overflow_at: None,
            lobbies: IndexSet::new(),
            links: IndexSet::new(),
            heard: IndexSet::new(),
            searches: TokenBucket::per_minute(l.searches_per_minute, now),
            invites: TokenBucket::per_minute(l.invites_per_minute, now),
            joins: TokenBucket::per_minute(l.joins_per_minute, now),
            searches_asked: TokenBucket::per_minute(ASKED_TIMES * l.searches_per_minute, now),
            invites_asked: TokenBucket::per_minute(ASKED_TIMES * l.invites_per_minute, now),
            invited: TokenBucket::new(l.invites_to_player / 60.0, l.invites_to_player / 2.0, now),
            invited_by: IndexMap::new(),
            bytes: [
                TokenBucket::per_second(l.message_bytes_per_second, now, 2.0),
                TokenBucket::per_second(l.match_bytes_per_second, now, 2.0),
            ],
            match_to: IndexMap::new(),
            presence: None,
            presence_at: 0.0,
            whos: TokenBucket::per_minute(l.who_per_minute, now),
        }
    }

    /// `join`: a player signed in on a connection: its session and whether it took a seat over, or refused { why }.
    /// A join of an id that has a seat, parked or connected, takes the seat over (a connected one's connection is told
    /// 'resumed elsewhere'): the server then calls [`Core::resume`] with the hello's count, sends the welcome, and calls
    /// [`Core::resumed`]; meanwhile the seat is sent nothing. Deviation: the TS ends the first (told 'signed in
    /// elsewhere'); the seat is the port's own rule.
    pub fn join(
        &mut self,
        id: &str,
        address: &str,
        p: Box<dyn CorePlayer>,
        now: f64,
    ) -> Result<(Session, bool), String> {
        self.now = now;
        let sid = self.next_sid;
        if let Some(q) = self.players.get_mut(id).filter(|q| !q.gone) {
            self.next_sid += 1;
            let was = q.seat;
            let mut old = std::mem::replace(&mut q.p, p);
            let old_address = std::mem::replace(&mut q.address, address.to_string());
            q.sid = sid;
            q.seat = Seat::Joined;
            if matches!(was, Seat::Parked { .. }) {
                self.log(&format!("{address} {id} takes its seat back"));
            } else {
                self.log(&format!("{old_address} {id} ended: resumed elsewhere ({address})"));
                old.refuse("resumed elsewhere");
            }
            return Ok((Session { id: id.to_string(), sid }, true));
        }
        if self.players.len() as f64 >= self.limits.players {
            let line = format!("{address} {id} refused: relay full");
            self.log(&line);
            return Err("relay full".into());
        }
        // (a parked seat holds its place in its address's connections until it expires, so one address cannot fill
        // the relay with seats by dropping its connections; the server caps the connections themselves at accept)
        let group = address_group(address);
        let seats = self.players.values().filter(|q| !q.gone && address_group(&q.address) == group).count();
        if seats as f64 >= self.limits.connections_per_address {
            let why = "too many connections from this address";
            self.log(&format!("{address} {id} refused: {why}"));
            return Err(why.into());
        }
        self.next_sid += 1;
        let me = self.new_player(id, sid, address, p, now);
        self.players.insert(id.to_string(), me);
        Ok((Session { id: id.to_string(), sid }, false))
    }

    /// A seat a join took over: resumed when `got` (the hello's count of channel-0 data frames the client took on the
    /// seat) is within its ring, and the answer is the count of those it took from the seat (the welcome's `resumed`);
    /// else (None, a gap the ring no longer covers, more owed than it keeps, or a core that keeps no seats) the old
    /// seat leaves everything, as a leave does, and the connection has a new one (None). None too for a join that took
    /// no seat over. Nothing is sent to it until [`Core::resumed`].
    pub fn resume(&mut self, s: &Session, got: Option<u64>, now: f64) -> Option<u64> {
        self.now = now;
        let (keep, seats) = (self.owed_ms(), self.seats);
        let q = self.players.get_mut(&s.id).filter(|q| q.sid == s.sid && !q.gone && q.seat == Seat::Joined)?;
        let first = q.sent - q.ring.len() as u64;
        let overflow = q.overflow_at.is_some_and(|t| now - t < keep);
        if seats
            && !overflow
            && let Some(g) = got.filter(|&g| g >= first && g <= q.sent)
        {
            q.seat = Seat::Welcome { from: Some(g) };
            let (took, line) = (q.took, format!("{} {} resumed", q.address, q.id));
            self.log(&line);
            return Some(took);
        }
        // Unverified: a client that missed this None (its welcome lost) and comes back later with got 0 finds the new
        // seat and is resumed on it as if nothing were lost; the client is to sign out at a None it reads.
        let line = format!("{} {} not resumed: a new seat", q.address, q.id);
        self.log(&line);
        let old = self.remove(&s.id).expect("a live player");
        let mut me = self.new_player(&s.id, old.sid, &old.address, old.p, now);
        me.seat = Seat::Welcome { from: None };
        self.players.insert(s.id.clone(), me);
        None
    }

    /// The welcome of a joined session is sent: the seat is live, and is sent what it is owed (`Owed`): for a resumed
    /// seat its ring's frames from the hello's count on with the owed messages in their place among them (a frame
    /// from a player no longer linked to it is not: its link closed), then its lobbies' members; a new seat only what it was
    /// owed since its join. A drop on the way parks it again, what it is owed kept in its order.
    pub fn resumed(&mut self, s: &Session, now: f64) {
        self.now = now;
        let keep = self.owed_ms();
        let Some(q) = self.players.get_mut(&s.id).filter(|q| q.sid == s.sid && !q.gone) else { return };
        let Seat::Welcome { from } = q.seat else { return };
        q.seat = Seat::Live;
        q.owed.retain(|o| o.once || now - o.when < keep);
        let (g, end) = (from.unwrap_or(0), q.sent);
        let owed: Vec<(u64, u64)> = q.owed.iter().filter(|o| o.once || o.at >= g).map(|o| (o.at, o.seq)).collect();
        let id = s.id.clone();
        let mut owed = owed.into_iter().peekable();
        // (k numbers the frames as they were at the welcome; `shift` of them were taken out again on the way)
        let (mut k, mut shift) = (if from.is_some() { g } else { end }, 0);
        loop {
            if !self.is_live(&id) {
                return;
            }
            if let Some(&(at, seq)) = owed.peek()
                && (at <= k || k >= end)
            {
                owed.next();
                self.send_owed(&id, seq);
                continue;
            }
            if k >= end {
                break;
            }
            if !self.replay(&id, k - shift) {
                shift += 1;
            }
            k += 1;
        }
        if from.is_none() {
            return;
        }
        let lobbies: Vec<String> =
            self.players.get(&id).map(|q| q.lobbies.iter().cloned().collect()).unwrap_or_default();
        for l in lobbies {
            let Some(x) = self.lobbies.get(&l) else { continue };
            let m = ServerMessage::Lobby { lobby: l, members: Some(x.members.clone()), closed: None };
            self.tell(&id, &m);
        }
    }

    /// An owed message sent at a resume (not owed again: a closing stays owed for its time, an invite is owed no more
    /// once it went).
    fn send_owed(&mut self, id: &str, seq: u64) {
        let Some(q) = self.players.get_mut(id).filter(|q| !q.gone && q.seat == Seat::Live) else { return };
        let Some(i) = q.owed.iter().position(|o| o.seq == seq) else { return };
        if q.p.send(&q.owed[i].m) == Taken::Lost {
            self.park_id(id);
        } else if q.owed[i].once {
            q.owed.remove(i);
        }
    }

    /// The ring's frame `n` sent again to a live seat (not recorded again), unless its sender is no longer linked to
    /// it. False when no handle was left for its sender: the frame is out of the ring again, as a live one would be.
    fn replay(&mut self, id: &str, n: u64) -> bool {
        let Some(q) = self.players.get_mut(id).filter(|q| !q.gone && q.seat == Seat::Live) else { return true };
        let i = (n - (q.sent - q.ring.len() as u64)) as usize;
        let (from, bytes) = q.ring[i].clone();
        if !q.links.contains(&from) {
            return true;
        }
        let taken = q.p.packet(0, &from, &bytes);
        if taken == Taken::NoHandle {
            q.ring.remove(i);
            q.sent -= 1;
            q.ring_bytes -= bytes.len();
            for o in q.owed.iter_mut().filter(|o| o.at > n) {
                o.at -= 1;
            }
        }
        self.after_packet(id, &from, taken);
        taken != Taken::NoHandle
    }

    /// `request`: a lobby call's answer, or the reason it is refused (EOS's result names; [`GONE`] for a session whose
    /// player has gone). The request is the client's `req` message as it came, raw JSON: the checks are core.ts's on
    /// untyped values, which serde's typed `Request` would not keep (it takes a null or missing optional field and
    /// refuses a whole number written 2.0, where the TS does otherwise).
    pub fn request(&mut self, s: &Session, r: &Value, now: f64) -> Result<Value, &'static str> {
        self.now = now;
        if self.me(s).is_none() {
            return Err(GONE);
        }
        let empty = Map::new();
        self.request_of(&s.id, r.as_object().unwrap_or(&empty))
    }

    /// `packet`: a packet to an id: dropped when that id is not connected, or when the two share no lobby and that id
    /// has not sent this player a packet in their link's life (either way a channel 0 one told peerClosed). To a
    /// match's referee ([`referee_id`]): taken on channel 1 from a human of that match only, whatever lobby the two
    /// share. To a host core ([`host_id`], [`QUEUE_HOST`]): its link's frames on channel 0 (`packet_to_host`).
    pub fn packet(&mut self, s: &Session, channel: u8, to: &str, bytes: &[u8], now: f64) {
        self.now = now;
        if self.me(s).is_none() {
            return;
        }
        let me = s.id.as_str();
        if bytes.len() as f64 > self.limits.packet_bytes {
            return self.drop_player(me, "packet too long");
        }
        let times = if channel == 1 { self.fan_out(me, to, now) } else { 1.0 };
        if !self.players[me].bytes[usize::from(channel)].take_times(bytes.len() as f64, now, times) {
            // a match's packets may be lost on the way, so one over the rate is; a message may not, so its sender goes
            if channel == 1 {
                return;
            }
            return self.drop_player(me, "too much data");
        }
        // (a human's bytes to a referee count to its own bucket, above, as any packet's; the referee's sends to none:
        // it sends each human about a packet a tick while the human is in its match, sending or not)
        if is_host_id(to) {
            return self.packet_to_host(me, channel, to, bytes);
        }
        if self.refereed.contains_key(to) {
            return self.packet_to_referee(me, channel, to, bytes);
        }
        if to == me || !self.players.get(to).is_some_and(|q| !q.gone) {
            // (as EOS gives up on a link to no one: the sender hears it closed; not for each lost match packet)
            if channel == 0 {
                self.tell(me, &ServerMessage::PeerClosed { id: to.to_string() });
            }
            return;
        }
        // Deviation: EOS lets any player open a link to any other; a relay passes a packet on only between players
        // that share a lobby, or to one that has sent this player a packet in their link's life (its packet opened
        // it), so a relay's player cannot be sent to by a stranger. Dropped, answered as a packet to no one is (so a
        // stranger cannot tell whether that id is connected).
        if !self.players[me].heard.contains(to) && !self.shares_lobby(me, to) {
            if channel == 0 {
                self.tell(me, &ServerMessage::PeerClosed { id: to.to_string() });
            }
            return;
        }
        self.players[to].heard.insert(me.to_string());
        // the first packet opens the link; the other side's client refuses it by closing it
        if !self.players[me].links.contains(to) {
            self.players[me].links.insert(to.to_string());
            self.players[to].links.insert(me.to_string());
            self.reopened(me, to);
            self.reopened(to, me);
        }
        // (a channel-0 frame is the seat's whatever its connection: a parked one's ring keeps it; channel 1 is lost)
        if channel == 0 {
            self.record(to, me, bytes);
        }
        if self.players[to].seat != Seat::Live {
            return;
        }
        let taken = self.players[to].p.packet(channel, me, bytes);
        if channel == 0 && taken == Taken::NoHandle {
            self.unrecord(to);
        }
        self.after_packet(to, me, taken);
    }

    /// How many times the match bytes limit player `me` may send at `now`, a match's packet to `to` among them: once
    /// for each other player it sent one in the last [`MATCH_FAN_MS`] (`to` one when a player that shares a lobby with
    /// it or sent it first), [`MAX_SLOTS`] - 1 at the most. A guest sends its referee alone; a game that referees its
    /// own match over the relay sends each of its humans the ruled streams of all.
    fn fan_out(&mut self, me: &str, to: &str, now: f64) -> f64 {
        let known = to != me
            && self.players.get(to).is_some_and(|q| !q.gone)
            && (self.players[me].heard.contains(to) || self.shares_lobby(me, to));
        let p = self.players.get_mut(me).expect("a player");
        p.match_to.retain(|_, at| now - *at < MATCH_FAN_MS);
        if known {
            p.match_to.insert(to.to_string(), now);
        }
        #[allow(clippy::cast_precision_loss, reason = "a count of eight at the most")]
        let n = p.match_to.len().clamp(1, MAX_SLOTS - 1) as f64;
        n
    }

    /// A channel-0 data frame read from the session's connection, whatever its handle, one dropped for a dead handle
    /// too: the server counts each (the welcome's `resumed` at a resume of the seat).
    pub fn took_data(&mut self, s: &Session) {
        if self.me(s).is_some() {
            self.players[&s.id].took += 1;
        }
    }

    /// `closePeer`: ends the link to another player (it is told). A human closing its link to a referee is out of that
    /// match: the client does so at its match's end.
    pub fn close_peer(&mut self, s: &Session, to: &str, now: f64) {
        self.now = now;
        if self.me(s).is_some() {
            self.close_peer_id(&s.id, to);
        }
    }

    fn close_peer_id(&mut self, me: &str, to: &str) {
        if is_host_id(to) {
            self.host_link_gone(me, to);
        } else if self.refereed.contains_key(to) {
            self.take_out(to, me, false);
        } else {
            self.close_link(me, to);
        }
    }

    /// `leave`: the disconnect: leaves every lobby and closes every link.
    pub fn leave(&mut self, s: &Session, now: f64) {
        self.now = now;
        if self.me(s).is_some() {
            self.remove(&s.id);
        }
    }

    /// A connection that dropped without a bye: its seat is kept for [`RESUME_MS`] (parked: sent nothing, its lobbies,
    /// links and matches kept), unless the core keeps no seats ([`Core::keep_no_seats`]) or it has nothing to resume
    /// (no lobby, link or match): then it leaves. A bye is a [`Core::leave`].
    pub fn park(&mut self, s: &Session, now: f64) {
        self.now = now;
        if self.me(s).is_some() {
            self.park_id(&s.id);
        }
    }

    fn park_id(&mut self, id: &str) {
        let now = self.now;
        let Some(q) = self.players.get(id).filter(|q| !q.gone) else { return };
        if matches!(q.seat, Seat::Parked { .. }) {
            return;
        }
        if !self.seats {
            self.remove(id);
            return;
        }
        // (a seat with nothing to resume, in no lobby, link or match, is no seat to keep: it leaves at once)
        let idle = q.lobbies.is_empty() && q.links.is_empty() && !self.human_of.contains_key(id);
        let line = format!("{} {id} {}", q.address, if idle { "left: nothing to resume" } else { "parked" });
        self.log(&line);
        if idle {
            self.remove(id);
            return;
        }
        self.players[id].seat = Seat::Parked { since: now };
    }

    /// A core whose connections' drops are leaves (a direct host's: its guests' links are LAN ones, which keep no
    /// seat).
    pub fn keep_no_seats(&mut self) {
        self.seats = false;
    }

    /// A core whose hosted lobbies get no short code (a direct host's: its guests join by its long code).
    pub fn give_no_shorts(&mut self) {
        self.give_shorts = false;
    }

    /// The lobby a short code names, while it lasts.
    pub fn lobby_of_short(&self, short: &str) -> Option<&str> {
        self.shorts.get(short).map(String::as_str)
    }

    /// Lets go of the seats parked for [`RESUME_MS`]: each leaves everything, as a leave does. The server calls it at
    /// its sweep.
    pub fn expire(&mut self, now: f64) {
        self.now = now;
        let out: Vec<String> = (self.players.values())
            .filter(|q| matches!(q.seat, Seat::Parked { since } if now - since >= RESUME_MS))
            .map(|q| q.id.clone())
            .collect();
        for id in out {
            let line = format!("{} {id} left: not back in time", self.players[&id].address);
            self.log(&line);
            self.remove(&id);
        }
    }

    /// The pump of every live referee, then of each host core due: the server calls it every [`REFEREE_PUMP_MS`]
    /// (the TS's timer a referee). A panic in a referee's pump is caught and logged once a match; the referee goes on
    /// at the next pump.
    pub fn pump(&mut self, now: f64) {
        self.now = now;
        let rids: Vec<String> = self.refereed.keys().cloned().collect();
        for rid in rids {
            self.run(&rid, None);
        }
        self.kick_unlinked(now);
        self.pump_hosts(now);
    }
}
