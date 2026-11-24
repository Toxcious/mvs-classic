//! One session's rules: the owner hands it each player's link as they join, the messages
//! that come on a link and the link's close, and pumps its deadlines; a match's packets never reach it. This file
//! holds the core's state, its entry points and the steps they run; the rules are in lobby.rs, queue.rs,
//! prematch.rs and play.rs.

use indexmap::{IndexMap, IndexSet};
use mvs_protocol::chat::ChatGate;
use mvs_protocol::loadout::NetLoadout;
use mvs_protocol::lobby_listing::{LISTING_V, ListingState, LobbyListing, listing_text};
use mvs_protocol::lobby_rules::LobbyRules;
use mvs_protocol::messages::{HostWhy, ServerMessage};
use mvs_protocol::names::{BAD_WORDS, Profanity};
use mvs_protocol::online::{PmKind, PmPick, QueueMode, QueuePlayer};
use mvs_protocol::record::{RecordFacts, Report, ReportRules};
use mvs_protocol::setup::MatchSetupWire;
use mvs_protocol::version::PROTOCOL;
use serde_json::{Map, Value};

use crate::bucket::Bucket;
use crate::draws::CombinedDraws;
use crate::link::{ConnKey, Connection, MatchReferee};
use crate::rand::HostRand;
use crate::timers::{TimerId, Timers};
use crate::{HostEvent, HostLog, HostOptions, HostTimes, PlayerInfo, SessionKind};

/// A message's size at the most, in UTF-8 bytes.
pub(crate) const MAX_MESSAGE: usize = 4096;
// ---------- limits against a hostile guest (the port's own) ----------
// A link's budget of messages, each a token bucket (a second's count, and what may come at once: a line that stood
// still delivers its backlog in one go). Over it the link is closed.
// Text: a game sends a ping every 2 s (1 s in a match) and a message per click in the lobby.
const TEXT_PER_SEC: f64 = 20.0;
const TEXT_BURST: f64 = 40.0;
// The queue: a link's queue.join has a budget of its own (a game sends one per search; a burst of 4, then one every
// 2 s; past it the join is answered queue.state off, 'error'). The waiting groups are looked at for their bot fill
// every QUEUE_BEAT_MS.
const JOIN_PER_SEC: f64 = 0.5;
const JOIN_BURST: f64 = 4.0;
pub(crate) const QUEUE_BEAT_MS: f64 = 250.0;
// log lines a minute about dropped links and handler errors
const NOTE_LINES: u32 = 20;
// profile.get: a game asks for another player's profile on a click (the lobby, the results screen); a burst of 6,
// then one a second. Past it the request is not answered. The asked player's game has PROFILE_ANSWER_MS to answer
// (the Career screen's own wait for an answer); none in that time is answered with no profile.
const PROFILE_PER_SEC: f64 = 1.0;
const PROFILE_BURST: f64 = 6.0;
// A member's own lobby changes that go to every member (lobby.setReady, lobby.joinTeam, a new name or icon): a burst
// of 8, then 4 a second. Past it the change is dropped, the link stays: a ready or team request is answered to its
// sender alone with the lobby as it is, a new icon is kept and shows with the lobby's next change. A new name is
// taken once per RENAME_MS: a hello with another name sooner is answered welcome with the name the core keeps, which
// the game reads as the new name refused. The core answers, so the game's own wait (4 s) does not let
// the new name stand while the lobby keeps the old one.
const CHANGE_PER_SEC: f64 = 4.0;
const CHANGE_BURST: f64 = 8.0;
pub(crate) const RENAME_MS: f64 = 2000.0;
pub(crate) const PROFILE_ANSWER_MS: f64 = 5000.0;
/// Unverified: the port's own cap on a profile.tell's profile, as UTF-8 JSON bytes: a profile of PROFILE_MATCHES
/// four-player records, every character's totals and trackers comes to about 12 KB. The tell itself may be that much
/// longer than MAX_MESSAGE; the profile message the asker gets stays under a P2P message's MAX_JOINED.
pub(crate) const PROFILE_MAX: usize = 32768;
const TELL_MAX: usize = PROFILE_MAX + 1024;
/// (the port's own) the most ids a lobby keeps as invited (lobby.invite): past it the oldest invite goes.
pub(crate) const INVITED_MAX: usize = 256;

/// A match of the core: its number (Match.id, the referee's).
pub(crate) type MatchId = u32;
/// A pre-match record of the core.
pub(crate) type PmId = u32;
/// A queue ticket of the core.
pub(crate) type TicketId = u32;
/// A match record to be.
pub(crate) type PlayedId = u32;

/// What a deadline of the core does.
#[derive(Clone, Debug)]
pub(crate) enum Due {
    /// The queue's bot fill (QUEUE_BEAT_MS, repeating).
    QueueBeat,
    /// A match's start gate (loadMs).
    Gate(MatchId),
    /// A match that runs too long (matchTicks).
    Limit(MatchId),
    /// A ticket whose members did not all join in PARTY_WAIT_MS.
    PartyLate(TicketId),
    /// A profile ask with no answer in PROFILE_ANSWER_MS.
    ProfileAnswer { asked: String, asker: String },
    /// A pre-match record's own (pmLater).
    Pm(PmId, PmDue),
    /// A measured path's wait for its reports (path.rs).
    PathWait(PmId),
}

/// A pre-match record's deadlines.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PmDue {
    /// The match starts (queueStartMs from queue.found, perksMs from pm.all).
    Start,
    /// The bots lock their picks.
    BotLock,
    /// The pick's time is over.
    PickTimeUp,
    /// The wait for the humans' results is over.
    Result,
    /// The vote's window is over.
    VoteOver,
    /// A bot's (or a leaver's slot's) vote.
    BotVote { slot: usize, yes: bool },
    /// The rematch's pm.begin.
    Rematch,
}

pub(crate) struct Client {
    pub conn: Box<dyn Connection>,
    /// the link's
    pub id: String,
    pub name: String,
    /// the game's release version (hello.app: online::app_of; 'dev' from a browser, '' none): words the note when
    /// two players' compatibility ids differ, and the desync log
    pub app: String,
    /// the game's compatibility id as its latest join sent it ('' none yet): compared as an opaque string, never read
    pub compat: String,
    /// the equipped profile icon as the hello named it: a ProfileIconData asset name, '' none (a lobby row shows it)
    pub icon: String,
    /// said hello with a name that was free
    pub named: bool,
    /// still the core's: nothing is sent to it or read from it once it is not
    pub open: bool,
    /// in the core's list of links (gone takes it out; its record lives on until the end of the call)
    pub listed: bool,
    /// a member of the session's lobby
    pub in_lobby: bool,
    pub match_id: Option<MatchId>,
    pub slot: i32,
    pub loaded: bool,
    // the budgets of what it sends (the limits above)
    pub text: Bucket,
    pub join: Bucket,
    pub profile: Bucket,
    pub chat: ChatGate,
    pub change: Bucket,
    /// the clock's time of its last new name (-Infinity none)
    pub renamed_at: f64,
    /// the ids it may still toast: the other humans of the last match.record it got, each taken out by its toast
    /// (replaced at its next record)
    pub toastable: IndexSet<String>,
    /// waiting in a queue
    pub queued: Option<Queued>,
    /// in a pre-match (a found queue match's or a lobby's start, to match.start), or a queue set's match, vote and
    /// rematch wait
    pub pm: Option<PmId>,
    /// its hello's Epic product user id (path.rs)
    pub epic: Option<String>,
    /// its round trip to the relay (ms): its hello's, then its latest queue.join's
    pub relay_rtt: Option<f64>,
    /// its hello's Direct connections setting
    pub direct: Option<bool>,
}

/// A match in play: its peers by fighter slot. From match.go on each peer's packets go to the match's referee, which
/// rules every human's stream and sends the ruled streams to every peer.
/// local (the port's rule): a match with one human in it, the rest bots. Its game plays it by itself (no gate, no
/// packets); the core only keeps the lobby and the player as in a match until match.leave.
pub(crate) struct Match {
    pub peers: Vec<ConnKey>,
    pub local: bool,
    pub going: bool,
    /// the clock's time of the go
    pub go_at: f64,
    /// the session's lobby's match
    pub lobby: bool,
    /// the start gate's time limit, until the go or the end
    pub gate: Option<TimerId>,
    /// from the go: the end of a match that runs too long (matchTicks)
    pub limit: Option<TimerId>,
    /// a queue set's record while this is its game: the match's end waits for its result
    pub pm: Option<PmId>,
    /// its record to be
    pub played: Option<PlayedId>,
    /// from the go to its end: the referee has it
    pub refereed: bool,
    /// each slot's team (the setup's, else slot % team count): a team chat's readers
    pub teams: Vec<u32>,
    /// the human its path chose to referee it and that one's Epic id (path.rs), from its start
    pub chosen: Option<crate::path::Chosen>,
    /// from the go: the chosen human whose app referees it (with `refereed`); None the core's referee
    pub by: Option<ConnKey>,
}

/// A match's record to be (record.rs): the core's facts of it, what a report is checked against, the humans' slots
/// whose result may still come and those that left before theirs, the results read.
pub(crate) struct Played {
    pub facts: RecordFacts,
    /// the game of its queue set (a lobby's match: 1)
    pub game: u32,
    pub rules: ReportRules,
    pub pending: IndexSet<i32>,
    pub left: IndexSet<i32>,
    pub reports: Vec<Report>,
    pub done: bool,
    /// written even with no report: the human whose app refereed the match left it before its result, and the
    /// others could send none
    pub forfeit: bool,
}

/// A player waiting in a queue: its pick and its ticket.
pub(crate) struct Queued {
    pub ticket: TicketId,
    pub character: String,
    pub skin: String,
    /// what the game sent with its pick: its taunts are this character's
    pub loadout: Option<NetLoadout>,
    /// its share of the found match's random choices (draws.rs): the join's, else the core's
    pub draw: u32,
}

/// (the port's own) A ticket: the joins of one party list (queue.join `party`; a solo's join is a ticket of one),
/// matched whole. It waits in its queues from its first member's join on and is matched once every member's join is
/// in (complete), else it is out PARTY_WAIT_MS after that first join. Until it is complete it is in no group and no
/// bot fill, so a ticket whose members never come holds nobody up.
pub(crate) struct Ticket {
    /// its members' ids, in the list's order, joined by spaces
    pub key: String,
    pub members: Vec<String>,
    /// the members whose join is in, in the order they joined
    pub joined: Vec<ConnKey>,
    /// every queue it waits in (the build's multiqueue): the first to fill takes it out of all. Its first join's
    /// modes, cut to each later member's.
    pub modes: Vec<QueueMode>,
    /// when its wait for bots is over (the clock's): its first join's time less what that one waited under another
    /// host
    pub due: f64,
    /// when its wait began (the clock's): its first join's time less what that one waited under another host
    pub since: f64,
    /// the largest of its members' round trips to the relay as their joins sent them (queue.join `rtt`, ms); None:
    /// no join sent one
    pub rtt: Option<f64>,
    /// PARTY_WAIT_MS from its first join, until it is complete
    pub late: Option<TimerId>,
}

/// A slot's loadout as its game sent it with a pick (queue.join, pm.preview, pm.lock), and the character its taunts
/// are for: the taunts go into the setup only when that character is the slot's fighter.
#[derive(Clone, Debug)]
pub(crate) struct SlotLoadout {
    pub character: String,
    pub loadout: NetLoadout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PmPhase {
    Pick,
    Perks,
    Match,
    Vote,
    Rematch,
}

/// One pre-match record for a found queue match and a custom lobby's start. A lobby enters at phase pick (pm.begin),
/// the queue at perks (queue.found: its picks were made before the join); both share perks onward. A queue match's
/// record lives on through its match (phase match), its vote, the wait for its rematch and the rematches of its
/// best-of-3 set; a lobby's ends at match.start.
pub(crate) struct PreMatch {
    /// the pre-match's, and its match's (match.start's `match`); a new one per pm.begin
    pub id: String,
    pub kind: PmKind,
    /// a queue's
    pub mode: Option<QueueMode>,
    /// a lobby's (kept after the record ends)
    pub lobby: bool,
    pub team_style: u8,
    /// a lobby's: drawn from its enabled maps
    pub map: String,
    /// a queue's: each game takes the map of the pick from its pool
    pub map_pick: u32,
    /// a queue's: the generator of its humans' combined draws, for the map pick, the bots and every match seed of
    /// the set; a lobby's None (its leader's host draws them)
    pub rand: Option<CombinedDraws>,
    /// by fighter slot; a bot's character is its pick
    pub players: Vec<QueuePlayer>,
    /// the match's set-up but for the picks (the bots' level, the rules)
    pub setup: MatchSetupWire,
    /// the humans still in it, each at its slot
    pub humans: Vec<ConnKey>,
    pub phase: PmPhase,
    /// pm.begin was sent: a leave is told pm.end (the queue's first pre-match: queue.state off)
    pub begun: bool,
    pub picks: Vec<Option<PmPick>>,
    pub previews: Vec<Option<PmPick>>,
    pub perks: Vec<Option<Vec<String>>>,
    pub loadouts: Vec<Option<SlotLoadout>>,
    /// every pending deadline: cleared at each phase's end
    pub timers: IndexSet<TimerId>,
    pub alive: bool,
    /// the result's wait (resultMs) runs
    pub result_due: bool,
    // the set (queue only)
    /// the game being played (1..setGames)
    pub game: u32,
    /// by team
    pub wins: Vec<u32>,
    /// a human's match.result by slot
    pub results: IndexMap<i32, i32>,
    /// a human's vote sent before the vote opened (its game saw the end first)
    pub early: IndexMap<i32, bool>,
    pub votes: Vec<Option<bool>>,
    /// the slots of humans who left during the match (their slot declines the vote)
    pub gone: IndexSet<i32>,
    /// its path on a core that measures (path.rs), from queue.found or pm.begin
    pub path: Option<crate::path::PathState>,
}

/// The custom game lobby: the rules' state and its human members by id.
pub(crate) struct Lobby {
    pub id: String,
    pub rules: LobbyRules,
    pub members: IndexMap<String, ConnKey>,
    /// the members whose lobby screen is up (lobby.enter; nobody's once a match starts)
    pub up: IndexSet<String>,
    pub match_id: Option<MatchId>,
    /// its start's pre-match (pm.begin to match.start)
    pub pm: Option<PmId>,
}

/// One session's rules.
pub struct Core {
    pub(crate) custom: bool,
    /// the relay's open queue (SessionKind::OpenQueue): nobody hosts it
    pub(crate) open: bool,
    pub(crate) host: String,
    name: Option<String>,
    pub(crate) lobby_id: Option<String>,
    pub(crate) times: HostTimes,
    pub(crate) referee: Option<Box<dyn MatchReferee>>,
    /// HostOptions::paths
    pub(crate) paths: bool,
    pub(crate) rand: Box<dyn HostRand>,
    log_out: Option<HostLog>,
    pub(crate) name_refused: Profanity,
    /// the time of the call that runs (the clock's now)
    pub(crate) now: f64,
    /// every link's record by its key, in the order they came (`listed` the core's list of links)
    pub(crate) clients: IndexMap<ConnKey, Client>,
    keys: u64,
    /// each queue's tickets in the order they joined (by QUEUE_MODES' order)
    pub(crate) waiting: [Vec<TicketId>; 4],
    /// every waiting ticket by its key
    pub(crate) tickets: IndexMap<String, TicketId>,
    pub(crate) ticket_list: IndexMap<TicketId, Ticket>,
    pub(crate) ticket_ids: TicketId,
    pub(crate) timers: Timers<Due>,
    /// a custom session's, from the host's lobby.enter on
    pub(crate) lobby: Option<Lobby>,
    /// the ids kicked from it: their lobby.join is refused
    pub(crate) kicked: IndexSet<String>,
    /// the ids its members invited (lobby.invite), in the order they were: let in while its invitedOnly option is on
    pub(crate) invited: IndexSet<String>,
    /// the records to be, by id; `recording` those whose results may still come
    pub(crate) played: IndexMap<PlayedId, Played>,
    pub(crate) recording: IndexSet<PlayedId>,
    played_ids: PlayedId,
    /// the open profile asks: by the asked player's id, each asker's id and the deadline of its answer
    pub(crate) asks: IndexMap<String, IndexMap<String, TimerId>>,
    pub(crate) ended: bool,
    /// the listing the owner was last told
    listed: Option<String>,
    /// the matches it has made (Match ids)
    pub(crate) match_count: MatchId,
    pub(crate) matches: IndexMap<MatchId, Match>,
    pub(crate) pms: IndexMap<PmId, PreMatch>,
    pm_ids: PmId,
    /// the links whose send failed in this call: closed at its end
    faulty: Vec<ConnKey>,
    notes: u32,
    notes_from: f64,
    events: Vec<HostEvent>,
}

/// QUEUE_MODES' index of a mode.
pub(crate) fn mode_index(m: QueueMode) -> usize {
    match m {
        QueueMode::OneVOne => 0,
        QueueMode::TwoVTwo => 1,
        QueueMode::Ffa => 2,
        QueueMode::Chaos => 3,
    }
}

/// A message's size in UTF-8 bytes is over `max` (tooBig).
pub(crate) fn too_big(s: &str, max: usize) -> bool {
    s.len() > max
}

/// A whole number of a message clamped to lo..=hi; `d` when it is not one (int).
pub(crate) fn int(v: Option<&Value>, lo: i64, hi: i64, d: i64) -> i64 {
    match v.and_then(Value::as_f64) {
        Some(x) if x.fract() == 0.0 => {
            // (a whole number: clamped before the cast)
            #[allow(clippy::cast_possible_truncation)]
            let n = x.clamp(lo as f64, hi as f64) as i64;
            n
        }
        _ => d,
    }
}

/// A message's string cut to `max` UTF-16 units; '' when it is not one (text).
pub(crate) fn text(v: Option<&Value>, max: usize) -> String {
    match v.and_then(Value::as_str) {
        Some(s) => utf16_slice(s, max),
        None => String::new(),
    }
}

/// The first `max` UTF-16 units of a string (String.prototype.slice(0, max)); a surrogate pair cut in two loses its
/// high half (a Rust string holds no lone surrogate).
pub(crate) fn utf16_slice(s: &str, max: usize) -> String {
    let mut n = 0;
    let mut out = String::new();
    for c in s.chars() {
        n += c.len_utf16();
        if n > max {
            break;
        }
        out.push(c);
    }
    out
}

// The names every JavaScript object has (`v in {}`): an asset name is never one.
const OBJECT_NAMES: [&str; 12] = [
    "constructor",
    "__proto__",
    "toString",
    "toLocaleString",
    "valueOf",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

/// A character's, a skin's or a map's name as a member sends it: the games look it up in their data, so it is
/// letters, digits, underscores and '-' (ProfileIcon_AS_RAM_Morty_Infini-Morty), at most ASSET_NAME_MAX (the longest
/// cosmetic's is 66: a longer one is refused, never cut, as a cut name matches nothing), and no name every JavaScript
/// object has (constructor, toString, __proto__); a map's is a MapData name (map_...). Not a string: ''. None: not such
/// a name. The core has no list of the game's names: a name no game has falls back to the default on each of them
/// (assetName).
pub(crate) fn asset_name(v: Option<&Value>) -> Option<String> {
    let Some(s) = v.and_then(Value::as_str) else { return Some(String::new()) };
    let ok = s.encode_utf16().count() <= ASSET_NAME_MAX
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && !OBJECT_NAMES.contains(&s);
    ok.then(|| s.to_string())
}
const ASSET_NAME_MAX: usize = 96;

/// A MapData name as a member sends it (mapName): never a level name of the game's own (net, TrainingContainer).
pub(crate) fn map_name(v: &Value) -> Option<String> {
    let s = asset_name(Some(v))?;
    (s.len() >= 4 && s[..4].eq_ignore_ascii_case("map_")).then_some(s)
}

impl Core {
    /// A session at `now`.
    pub fn new(o: HostOptions, now: f64) -> Core {
        let custom = o.kind == SessionKind::Custom;
        let mut core = Core {
            custom,
            open: o.kind == SessionKind::OpenQueue,
            host: o.host,
            name: o.name,
            lobby_id: o.lobby_id,
            times: o.times,
            referee: o.referee,
            paths: o.paths,
            rand: o.rand,
            log_out: o.log,
            name_refused: Profanity::new(&BAD_WORDS),
            now,
            clients: IndexMap::new(),
            keys: 0,
            waiting: Default::default(),
            tickets: IndexMap::new(),
            ticket_list: IndexMap::new(),
            ticket_ids: 0,
            timers: Timers::default(),
            lobby: None,
            kicked: IndexSet::new(),
            invited: IndexSet::new(),
            played: IndexMap::new(),
            recording: IndexSet::new(),
            played_ids: 0,
            asks: IndexMap::new(),
            ended: false,
            listed: None,
            match_count: 0,
            matches: IndexMap::new(),
            pms: IndexMap::new(),
            pm_ids: 0,
            faulty: Vec::new(),
            notes: 0,
            notes_from: f64::NEG_INFINITY,
            events: Vec::new(),
        };
        // (the queue's bot fill: a queue session's beat)
        if !custom {
            core.later(QUEUE_BEAT_MS, Due::QueueBeat, true);
        }
        core
    }

    /// A player's link is open: the core sends it `server`. A second link of the same id takes the first one's place
    /// (the first is told `replaced` and closed).
    pub fn connect(&mut self, mut conn: Box<dyn Connection>, now: f64) -> ConnKey {
        self.now = now;
        self.keys += 1;
        let key = ConnKey(self.keys);
        if self.ended {
            conn.close(HostWhy::Ended);
            return key;
        }
        // the old link is told first, so that its game does not try again
        let old: Vec<ConnKey> =
            self.clients.iter().filter(|(_, c)| c.listed && c.id == conn.id()).map(|(&k, _)| k).collect();
        for x in old {
            self.send(x, &ServerMessage::Replaced);
            self.drop_link(x, HostWhy::Replaced);
        }
        // (the host's own link replaced: the session ended with it)
        if self.ended {
            conn.close(HostWhy::Ended);
            self.settle();
            return key;
        }
        let name = if conn.name().is_empty() { "Player".to_string() } else { conn.name().to_string() };
        let c = Client {
            id: conn.id().to_string(),
            conn,
            name,
            app: String::new(),
            compat: String::new(),
            icon: String::new(),
            named: false,
            open: true,
            listed: true,
            in_lobby: false,
            match_id: None,
            slot: -1,
            loaded: false,
            text: Bucket::new(TEXT_PER_SEC, TEXT_BURST, now),
            join: Bucket::new(JOIN_PER_SEC, JOIN_BURST, now),
            profile: Bucket::new(PROFILE_PER_SEC, PROFILE_BURST, now),
            chat: ChatGate::new(1.0, now),
            change: Bucket::new(CHANGE_PER_SEC, CHANGE_BURST, now),
            renamed_at: f64::NEG_INFINITY,
            toastable: IndexSet::new(),
            queued: None,
            pm: None,
            epic: None,
            relay_rtt: None,
            direct: None,
        };
        self.clients.insert(key, c);
        // the first message: this host's protocol and its name; the game closes on another protocol
        let name = self.name.clone().unwrap_or_else(|| "MVS Classic".to_string());
        self.send(key, &ServerMessage::Server { proto: PROTOCOL, name });
        self.settle();
        key
    }

    /// A message on the link (JSON text). No message ends the session.
    pub fn message(&mut self, key: ConnKey, data: &str, now: f64) {
        self.now = now;
        self.message_step(key, data);
        self.settle();
    }

    fn message_step(&mut self, key: ConnKey, data: &str) {
        let Some(c) = self.clients.get_mut(&key) else { return };
        if !c.listed || !c.open {
            return;
        }
        // (a profile.tell may be longer, to TELL_MAX: a game's message names its type first)
        if too_big(data, MAX_MESSAGE) && (!data.starts_with(r#"{"t":"profile.tell""#) || too_big(data, TELL_MAX)) {
            return self.drop_link(key, HostWhy::MessageTooBig);
        }
        if !c.text.take(1.0, self.now) {
            return self.drop_link(key, HostWhy::TooManyMessages);
        }
        let Ok(Value::Object(m)) = serde_json::from_str::<Value>(data) else { return };
        let Some(t) = m.get("t").and_then(Value::as_str) else { return };
        if !HANDLERS.contains(&t) {
            return;
        }
        // (the text's start is not its type: JSON keeps the last of two keys of one name)
        if t != "profile.tell" && too_big(data, MAX_MESSAGE) {
            return self.drop_link(key, HostWhy::MessageTooBig);
        }
        let t = t.to_string();
        self.handle(key, &t, &m, data);
    }

    /// The link closed by itself; the player leaves whatever they were in.
    pub fn closed(&mut self, key: ConnKey, now: f64) {
        self.now = now;
        if !self.clients.get(&key).is_some_and(|c| c.listed) {
            return;
        }
        self.conn_gone(key);
        self.settle();
    }

    /// The referee's gone human of a going match (`SlotNews::Gone`): out of the match.
    pub fn gone(&mut self, match_id: u32, slot: u32, now: f64) {
        self.now = now;
        self.silent_step(match_id, slot);
        self.settle();
    }

    /// The referee's away human of a going match (`SlotNews::Away`): the others are told, the match goes on.
    pub fn away(&mut self, match_id: u32, slot: u32, now: f64) {
        self.now = now;
        self.away_step(match_id, slot, true);
        self.settle();
    }

    /// The referee's human of a going match is back (`SlotNews::Back`): the others are told.
    pub fn back(&mut self, match_id: u32, slot: u32, now: f64) {
        self.now = now;
        self.away_step(match_id, slot, false);
        self.settle();
    }

    /// Runs every deadline due by `now`, in time order (two at once in the order they were set), each one a call of
    /// its own.
    pub fn pump(&mut self, now: f64) {
        self.now = now;
        self.timers.begin();
        while !self.ended {
            let Some((id, what)) = self.timers.take_due(now) else { break };
            self.run_due(id, what);
            self.settle();
        }
    }

    /// When the next deadline is due (None: none set).
    pub fn next_due(&self) -> Option<f64> {
        self.timers.next_due()
    }

    /// The players who said hello with a name that was free.
    pub fn players(&self) -> Vec<PlayerInfo> {
        self.clients
            .values()
            .filter(|c| c.listed && c.named)
            .map(|c| PlayerInfo {
                id: c.id.clone(),
                name: c.name.clone(),
                busy: c.match_id.is_some() || c.queued.is_some() || c.pm.is_some(),
            })
            .collect()
    }

    /// Ends the session: every link closed, every deadline dropped.
    pub fn end(&mut self, why: HostWhy, now: f64) {
        self.now = now;
        self.end_session(why);
        self.settle();
    }

    /// The session is over.
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// What the core told its owner since the last call.
    pub fn take_events(&mut self) -> Vec<HostEvent> {
        std::mem::take(&mut self.events)
    }

    // ---------- the core's steps ----------

    /// A deadline of the core runs.
    fn run_due(&mut self, id: TimerId, what: Due) {
        match what {
            Due::QueueBeat => self.fill_queues(),
            Due::Gate(m) => self.gate_over(m),
            Due::Limit(m) => self.limit_over(m),
            Due::PartyLate(t) => self.party_late(t),
            Due::ProfileAnswer { asked, asker } => self.profile_timeout(&asked, &asker),
            Due::PathWait(pm) => self.path_wait(pm, id),
            Due::Pm(pm, due) => {
                let alive = self.pms.get_mut(&pm).is_some_and(|p| {
                    p.timers.shift_remove(&id);
                    p.alive
                });
                if alive {
                    self.run_pm_due(pm, due);
                }
            }
        }
    }

    /// A deadline of the core, `ms` from now (repeat: every `ms`).
    pub(crate) fn later(&mut self, ms: f64, what: Due, repeat: bool) -> TimerId {
        self.timers.add(self.now, ms, if repeat { ms } else { 0.0 }, what)
    }

    pub(crate) fn cancel(&mut self, t: TimerId) {
        self.timers.cancel(t);
    }

    /// The end of a call: the links whose send failed on the way are closed and out of everything, the records of
    /// what is over are let go, and the owner hears of a new listing.
    fn settle(&mut self) {
        while !self.faulty.is_empty() {
            let k = self.faulty.remove(0);
            if let Some(c) = self.clients.get_mut(&k) {
                c.conn.close(HostWhy::LinkError);
            }
            self.conn_gone(k);
        }
        // (what nothing reaches any more)
        self.clients.retain(|_, c| c.listed);
        self.pms.retain(|_, p| p.alive);
        let played = &self.played;
        self.matches
            .retain(|_, m| !m.peers.is_empty() || m.played.is_some_and(|p| played.get(&p).is_some_and(|p| !p.done)));
        self.played.retain(|_, p| !p.done);
        let tickets = &self.tickets;
        self.ticket_list.retain(|id, t| tickets.get(&t.key) == Some(id));
        if !self.ended {
            let t = self.listing();
            if t != self.listed {
                self.listed.clone_from(&t);
                self.events.push(HostEvent::Listing(t));
            }
        }
    }

    /// The custom lobby as the browser shows it, while it is public.
    fn listing(&self) -> Option<String> {
        let l = self.lobby.as_ref()?;
        let host = self.clients.get(l.members.get(&self.host)?)?;
        if !l.rules.options.public {
            return None;
        }
        let r = &l.rules;
        let players = r.players();
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Some(listing_text(&LobbyListing {
            v: LISTING_V,
            name: host.name.clone(),
            icon: host.icon.clone(),
            compat: host.compat.clone(),
            app: utf16_slice(&host.app, 32),
            style: r.team_style,
            maps: r.enabled_maps.clone(),
            map_count: count(r.enabled_maps.len()),
            players: count(players.len()),
            humans: count(players.iter().filter(|p| !p.is_bot).count()),
            slots: count(players.len() + r.get_available_player_slot_count()),
            state: if l.match_id.is_some() || l.pm.is_some() { ListingState::Match } else { ListingState::Lobby },
            time: r.match_duration,
            ringouts: r.num_ringouts_for_win,
            hazards: r.allow_hazards,
        }))
    }

    /// A log line (the owner's sink; stderr with the time by default).
    pub(crate) fn log(&mut self, line: &str) {
        match &mut self.log_out {
            Some(f) => f(line),
            None => {
                let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                eprintln!("{:02}:{:02}:{:02} {line}", s / 3600 % 24, s / 60 % 60, s % 60);
            }
        }
    }

    /// A log line about something a guest can cause any number of times: NOTE_LINES a minute and no more.
    pub(crate) fn note(&mut self, line: &str) {
        if self.now - self.notes_from > 60000.0 {
            if self.notes > NOTE_LINES {
                let more = format!("({} more lines like these were not logged)", self.notes - NOTE_LINES);
                self.log(&more);
            }
            self.notes_from = self.now;
            self.notes = 0;
        }
        self.notes += 1;
        if self.notes <= NOTE_LINES {
            self.log(line);
        }
    }

    pub(crate) fn event(&mut self, e: HostEvent) {
        self.events.push(e);
    }

    // ---------- links ----------

    /// A message to a player.
    pub(crate) fn send(&mut self, k: ConnKey, m: &ServerMessage) {
        self.send_text(k, &m.text());
    }

    /// A message to a player as its JSON text. A link whose send fails is that link's fault: it is shut at once
    /// (nothing more goes to it) and closed at the end of the call (settle), so that the call runs to its end for
    /// everyone else.
    pub(crate) fn send_text(&mut self, k: ConnKey, text: &str) {
        let Some(c) = self.clients.get_mut(&k) else { return };
        if !c.open {
            return;
        }
        if let Err(e) = c.conn.send(text) {
            c.open = false;
            let line = format!("link error {} {e}", c.name);
            self.note(&line);
            self.faulty.push(k);
        }
    }

    /// The core closes a link: nothing more is read from it or sent to it, and the player leaves what they were in.
    pub(crate) fn drop_link(&mut self, k: ConnKey, why: HostWhy) {
        let Some(c) = self.clients.get_mut(&k) else { return };
        if !c.open {
            return;
        }
        c.open = false;
        let line = format!("dropped {} {}", c.name, why.log_name());
        self.note(&line);
        if let Some(c) = self.clients.get_mut(&k) {
            c.conn.close(why);
        }
        self.conn_gone(k);
    }

    /// A player's link is gone (closed by itself or by the core): out of everything. The host's ends the session.
    pub(crate) fn conn_gone(&mut self, k: ConnKey) {
        let Some(c) = self.clients.get_mut(&k) else { return };
        if !c.listed {
            return;
        }
        c.listed = false;
        c.open = false;
        let id = c.id.clone();
        self.leave_queue(k);
        self.leave_pre_match(k);
        self.leave_lobby(k);
        self.leave_match(k, mvs_protocol::messages::LeftWhy::Disconnected);
        self.drop_asks(&id);
        if !self.open && id == self.host {
            self.end_session(HostWhy::HostLeft);
        }
    }

    /// The session ends: the records whose results are in go out first, then every link is closed.
    pub(crate) fn end_session(&mut self, why: HostWhy) {
        if self.ended {
            return;
        }
        self.ended = true;
        // (the records whose results are in go out first, to every human still linked: one still without a result
        // counts as having played on)
        for pl in self.recording.clone() {
            self.played_end(pl);
        }
        self.asks.clear();
        self.timers.clear();
        let all: Vec<ConnKey> = self.clients.iter().filter(|(_, c)| c.listed).map(|(&k, _)| k).collect();
        for &k in &all {
            if let Some(c) = self.clients.get_mut(&k) {
                c.listed = false;
            }
        }
        self.faulty.clear();
        for &k in &all {
            if let Some(m) = self.clients.get(&k).and_then(|c| c.match_id) {
                self.unreferee(m);
            }
        }
        for &k in &all {
            if let Some(c) = self.clients.get_mut(&k) {
                c.open = false;
                c.conn.close(why.clone());
            }
        }
        self.log(&format!("session over: {}", why.log_name()));
        self.event(HostEvent::End(why));
    }

    /// The open link of a player id, named or not (linked).
    pub(crate) fn linked(&self, id: &str) -> Option<ConnKey> {
        self.clients.iter().find(|(_, c)| c.listed && c.open && c.id == id).map(|(&k, _)| k)
    }

    /// One more match may be found (MatchReferee::room with the core's pending ones; no referee: always).
    pub(crate) fn room(&self) -> bool {
        self.referee.as_ref().is_none_or(|r| r.room(self.pending()))
    }

    /// A match at its go may start: the referee has a place for it now (the others pending wait for their own go).
    pub(crate) fn room_to_go(&self) -> bool {
        self.referee.as_ref().is_none_or(|r| r.room(0))
    }

    /// The matches found that the referee does not have yet: a pre-match of more than one human in its pick or perks,
    /// a match loading (not going, not local).
    fn pending(&self) -> usize {
        let pms = self
            .pms
            .values()
            .filter(|p| p.alive && p.humans.len() > 1 && matches!(p.phase, PmPhase::Pick | PmPhase::Perks))
            .count();
        let loading = self.matches.values().filter(|m| !m.going && !m.local && !m.peers.is_empty()).count();
        pms + loading
    }

    /// The hosting player's open link (hostClient).
    pub(crate) fn host_client(&self) -> Option<ConnKey> {
        self.clients.iter().find(|(_, c)| c.listed && c.open && c.id == self.host).map(|(&k, _)| k)
    }

    pub(crate) fn c(&self, k: ConnKey) -> &Client {
        self.clients.get(&k).unwrap_or_else(|| panic!("host core: no link {k:?}"))
    }

    pub(crate) fn cm(&mut self, k: ConnKey) -> &mut Client {
        self.clients.get_mut(&k).unwrap_or_else(|| panic!("host core: no link {k:?}"))
    }

    pub(crate) fn new_pm_id(&mut self) -> PmId {
        self.pm_ids += 1;
        self.pm_ids
    }

    pub(crate) fn new_played_id(&mut self) -> PlayedId {
        self.played_ids += 1;
        self.played_ids
    }

    /// A message handler, by the message's `t`.
    fn handle(&mut self, k: ConnKey, t: &str, m: &Map<String, Value>, data: &str) {
        match t {
            "hello" => {
                self.on_hello(k, m);
                if self.clients.get(&k).is_some_and(|c| c.listed && c.open) {
                    self.path_hello(k, m);
                }
            }
            "ping" => self.on_ping(k, m),
            "profile.icon" => self.on_profile_icon(k, m),
            "profile.get" => self.on_profile_get(k, m),
            "profile.tell" => self.on_profile_tell(k, m),
            "lobby.enter" => self.on_lobby_enter(k, m),
            "lobby.join" => self.on_lobby_join(k, m),
            "lobby.leave" => self.on_lobby_leave(k),
            "lobby.setTeamStyle" => self.on_lobby_set_team_style(k, m),
            "lobby.setMatchDuration" => self.on_lobby_set_match_duration(k, m),
            "lobby.setNumRingoutsForWin" => self.on_lobby_set_ringouts(k, m),
            "lobby.setAllowHazards" => self.on_lobby_set_hazards(k, m),
            "lobby.setMaps" => self.on_lobby_set_maps(k, m),
            "lobby.setOption" => self.on_lobby_set_option(k, m),
            "lobby.addBot" => self.on_lobby_add_bot(k, m),
            "lobby.updateSettingsForBot" => self.on_lobby_update_bot(k, m),
            "lobby.kickPlayer" => self.on_lobby_kick(k, m),
            "lobby.invite" => self.on_lobby_invite(k, m),
            "lobby.promoteNewLeader" => self.on_lobby_promote(k, m),
            "lobby.setReady" => self.on_lobby_set_ready(k, m),
            "lobby.joinTeam" => self.on_lobby_join_team(k, m),
            "lobby.startMatch" => self.on_lobby_start(k),
            "queue.join" => self.on_queue_join(k, m),
            "queue.cancel" => self.on_queue_cancel(k),
            "pm.preview" => self.on_pm_preview(k, m),
            "pm.lock" => self.on_pm_lock(k, m),
            "pm.perks" => self.on_pm_perks(k, m),
            "pm.leave" => self.on_pm_leave(k),
            "match.result" => self.on_match_result(k, m),
            "match.vote" => self.on_match_vote(k, m),
            "match.loaded" => self.on_match_loaded(k),
            "match.desync" => self.on_match_desync(k, data),
            "match.leave" => self.on_match_leave(k),
            "chat" => self.on_chat(k, m),
            "toast" => self.on_toast(k, m),
            "path.report" => self.on_path_report(k, m),
            "ref.news" => self.on_ref_news(k, m),
            "ref.lost" => self.on_ref_lost(k),
            _ => {}
        }
    }
}

/// The message types the core answers (the handlers).
const HANDLERS: [&str; 38] = [
    "hello",
    "ping",
    "profile.icon",
    "profile.get",
    "profile.tell",
    "lobby.enter",
    "lobby.join",
    "lobby.leave",
    "lobby.setTeamStyle",
    "lobby.setMatchDuration",
    "lobby.setNumRingoutsForWin",
    "lobby.setAllowHazards",
    "lobby.setMaps",
    "lobby.setOption",
    "lobby.addBot",
    "lobby.updateSettingsForBot",
    "lobby.kickPlayer",
    "lobby.invite",
    "lobby.promoteNewLeader",
    "lobby.setReady",
    "lobby.joinTeam",
    "lobby.startMatch",
    "queue.join",
    "queue.cancel",
    "pm.preview",
    "pm.lock",
    "pm.perks",
    "pm.leave",
    "match.result",
    "match.vote",
    "match.loaded",
    "match.desync",
    "match.leave",
    "chat",
    "toast",
    "path.report",
    "ref.news",
    "ref.lost",
];
