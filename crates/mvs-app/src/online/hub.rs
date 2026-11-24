//! The menus' side of online play (web/src/net/online.ts Online, net/identity.ts, net/party.ts, net/chat.ts): the net
//! service's sign-in, the one session the game has open (a custom game it hosts or joined, the queue's), greeted with
//! the chosen name as it opens (hello), and what the menus see of it (`OnlineView`) and ask of it (`OnlineCmd`). It
//! routes the session's messages to the pre-match hub, a queue set's end and the queue's search, and turns a found or
//! started match into the program's.
//!
//! Deviation: the TS's listeners and promises are this frame's `OnlineView::events` and state read in later frames;
//! its timers run on the owner's clock (`frame`'s `now`).
//! Deviation: no friends (online.ts): the social lists hold the players met recently (recent.rs) and the relay's
//! online list (who), and this player's presence on it (name, icon, busy, the two settings) is published through the
//! net service.

use std::collections::BTreeMap;

use mvs_net::backend::BackendKind;
use mvs_net::service::api::{
    BrowseWhy, ClosedWhy, LocalWhy, MenuFighter, NetEvent, NetRequest, OffWhy, PartySay, PartyView, SessionOn,
    SessionTarget, StatusWhy,
};
use mvs_protocol::chat::{ChatGate, clean_chat};
use mvs_protocol::loadout::NetLoadout;
use mvs_protocol::messages::{ClientMessage, PmEndWhy, ServerMessage};
use mvs_protocol::names::{BAD_WORDS, NAME_LIMIT, Profanity, clean_name};
use mvs_protocol::online::{PmKind, QueueMode, QueueParty};
use mvs_protocol::relay_wire::{Presence, PresenceStatus};
use mvs_protocol::version::PROTOCOL;
use mvs_ui::menu::online::{
    BrowseAnswer, BrowseNote, BrowsedGame, KickNote, LobbyInvite, LocalReason, OfflineWhy, OnlineCmd, OnlineEvent,
    OnlineLine, OnlineNotice, OnlinePlayer, OnlineView, PartyCmd, PreMatchCmd, PreMatchEvent, PreMatchView,
    QueueResume, SessionAsk, SessionLink, SessionView,
};

use super::client::{NetClient, News, OpenFailed};
use super::gate::Gate;
use super::port::{NetPort, NetStatus};
use super::pre_match::{HubNews, Kept, PreMatchHub, PreMatchStarted};
use super::profile::{ProfileAnswer, Profiles};
use super::queue::{PartyQueue, QOut, QueueMatch, QueueSession};
use super::queue_end::{NetQueueEnd, QeNews};
use super::recent::{MetPlayer, RecentPlayers};
use super::tables::Tables;

/// (the port's own) A host core's notice: anyone's (the queue's host is a stranger), so it loses its control and
/// format characters and is cut to NOTICE_LIMIT characters, one is shown per NOTICE_EVERY_MS at the most, and the text
/// last shown is not shown again within NOTICE_AGAIN_MS (the main menu holds a notice 12 s). An invite not sent is
/// told once per NOTICE_EVERY_MS.
pub const NOTICE_LIMIT: usize = 200;
pub const NOTICE_EVERY_MS: f64 = 5000.0;
pub const NOTICE_AGAIN_MS: f64 = 12000.0;
/// (the port's own) The online list is asked for again at most once per WHO_AGAIN_MS; a 'who' or a browse not
/// answered by BROWSE_MS is taken as failed.
pub const WHO_AGAIN_MS: f64 = 5000.0;
pub const BROWSE_MS: f64 = 15000.0;
/// hello's wait for the host core's answer to the name: none by then, the name stands.
pub const ANSWER_MS: f64 = 4000.0;
/// (the port's own) chat.ts: each sender has RECEIVE_TIMES the send budget, all senders together RECEIVE_ALL_TIMES
/// it; the budgets of RECEIVE_SENDERS senders are kept.
const RECEIVE_TIMES: f64 = 2.0;
const RECEIVE_ALL_TIMES: f64 = 4.0;
const RECEIVE_SENDERS: usize = 8;

/// What the hub reads of the settings each frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prefs {
    /// The chosen name (settings.profile.name, '' none).
    pub name: String,
    /// The equipped profile icon's ProfileIconData asset name ('' none).
    pub icon: String,
    /// ui.show_in_online_list, ui.invites.
    pub listed: bool,
    pub invites: bool,
}

/// A match the online pre-match or the queue started, for the program to load (custom-pre-match.ts matchOf).
#[derive(Clone, Debug, PartialEq)]
pub struct NetMatchStart {
    pub start: PreMatchStarted,
    /// The queue mode of a queue match (its set's end follows it), None a custom lobby's.
    pub queue: Option<QueueMode>,
}

// A hello under way: its name, its end, and whether it is a new name's claim (identity.check).
struct Hello {
    name: String,
    until: f64,
    claim: bool,
}

/// The game's online client.
pub struct OnlineHub {
    pub port: NetPort,
    /// The open session (None: none).
    pub client: Option<NetClient>,
    target: Option<SessionTarget>,
    session_me: String,
    recent: RecentPlayers,
    // the last invitation's or toast's sender by id (online.ts senders: player(id)'s last fallback)
    senders: BTreeMap<String, OnlinePlayer>,
    listed: Vec<OnlinePlayer>,
    listed_why: Option<BrowseNote>,
    who_at: f64,
    who_asking: Option<(u32, f64)>,
    whos: u32,
    browses: u32,
    browsing: Option<(u32, f64)>,
    browse: Option<BrowseAnswer>,
    presence_sent: Option<Presence>,
    queue_session: bool,
    playing: bool,
    /// The shape of the match `playing` is in, from its pm.begin (the presence's status).
    playing_as: PresenceStatus,
    prefs: Prefs,
    /// A new name the host core took, for the settings (taken by the owner).
    pub saved_name: Option<String>,
    name_taken: bool,
    name_refused: bool,
    hello: Option<Hello>,
    hello_icon: String,
    told: Option<OnlineNotice>,
    notice_text: String,
    notice_at: f64,
    refused_at: f64,
    lobby: Option<mvs_protocol::lobby_rules::LobbyData>,
    party: Option<PartyView>,
    /// The fighter this player's main menu shows (`set_menu_fighter`), and the last one sent to the service.
    menu_fighter: Option<MenuFighter>,
    fighter_sent: Option<Option<MenuFighter>>,
    /// The other party members' main-menu fighters, by id (NetEvent::PartyFighter; a member that sent none has none).
    party_fighters: BTreeMap<String, MenuFighter>,
    /// This game asked to leave its party (PartyCmd::Leave) since the party was last full: its end is this
    /// player's own, so no "Party Disbanded" note.
    party_leaving: bool,
    pub tables: Tables,
    pub hub: PreMatchHub,
    pub queue_end: Option<NetQueueEnd>,
    pub queue: Option<QueueSession>,
    // the screen took the hub's current pre-match (screenUp, the custom path)
    attached: bool,
    /// The match's path as the host core last chose it (pm.path), handed again to a pre-match screen that comes up
    /// after it (ScreenUp); dropped at the next pre-match, a found match, a pre-match's end and the match's go.
    path: Option<ServerMessage>,
    /// (the port's own) The last pm.path's referee slot and round trips by slot, kept past the match's go for its
    /// ping line (NetMatch::ping); dropped at the next pre-match, a found match and the session's end.
    match_path: Option<(i32, Vec<Option<u32>>)>,
    /// (dev) What the session's link is said to be in the menus' view, whatever the service says (glue::start's
    /// MVS_DEV_SESSION_ON): the lobby's connection line and the ping line of each way, for their pictures.
    pub dev_on: Option<SessionOn>,
    /// The match to load (taken by the owner).
    pub started: Option<NetMatchStart>,
    /// The match records of the session (match.record), for the stats file (taken by the owner).
    pub records: Vec<serde_json::Value>,
    /// The queue's pre-match screen to open on these modes (OnlineCmd::Queue, a member following its leader).
    pub open_queue: Option<Vec<QueueMode>>,
    /// A match the session started with no other human, played as this game's alone (custom-pre-match.ts's
    /// `m.local`): its result and match.leave go at its end (`local_over`).
    pub local: Option<LocalMatch>,
    /// (dev) `--queue-fake`: the queue's session is this fake host core, not the net service's (dev/fake-queue.ts).
    pub fake: Option<super::fake_queue::FakeCore>,
    /// (dev) `--fake-lobbies [n]`: the custom games list answers with this many made-up lobbies.
    pub fake_lobbies: Option<super::fake_lobbies::FakeLobbies>,
    /// (dev) `--queue-bots <s>`: the wait for bots each join asks for (?queueBots).
    pub bot_after: Option<f64>,
    /// The profile asks both ways (net/profile.ts).
    pub profiles: Profiles,
    compat: String,
    lag: f64,
    chat: ChatGate,
    received: Vec<(String, ChatGate)>,
    received_all: ChatGate,
    profanity: Profanity,
    events: Vec<OnlineEvent>,
    inbox: Option<MatchInbox>,
    /// Commands run before the menus' next ones (the command line's `--online-open` host or queue).
    pub pending: Vec<OnlineCmd>,
    /// The code of a join link or of `--online-open join:`, waiting with the pending commands: the menus then open
    /// the lobby screen on it, as the join box does (`crate::menus::hand_link_join`).
    pub link_join: Option<String>,
    /// The clock's zero: the service's too, so the hub's `now` and a relay link's are one clock.
    pub(super) epoch: std::time::Instant,
    /// An input script's clock (ms, 60 frames a second of the run's): the oracle page's stepped clock, which its fake
    /// queue's waits count on (None: the wall clock).
    pub script_clock: Option<f64>,
    /// The last `state_line` written to the log.
    logged: String,
    /// What keeps online play off before it meets anyone (None: nothing).
    gate: Option<Gate>,
    /// The self check of this copy under way (`crate::self_check::begin`; None: none, or over).
    pub self_check: Option<crate::self_check::Running>,
    /// The self check's answer once over, when this PC's hash differs from the release's: (mine, release). None while
    /// it runs, with no reference, when it could not run, or when the hashes are the same.
    pub self_check_differs: Option<(u32, u32)>,
    /// The data folder (the recent players' file's; None: a hub without one, the tests'): where a desync's report goes.
    pub data: Option<std::path::PathBuf>,
    /// (dev) `--online-desync-at <tick>`: an online match's own state hash at that confirmed tick is made wrong
    /// (Session::spoil_hash_at), for the desync report's checks.
    pub desync_at: Option<u32>,
    /// When the menus first asked whether the pending commands wait (`pending_held`): PENDING_WAIT_MS runs from it.
    pending_since: Option<f64>,
    /// The relay was not reached at its last try: its next try's "connecting" keeps the line at not reached
    /// (`line`).
    latch: LineLatch,
}

/// A local match of the session (no other human): whether it is a queue match (its result goes through the set's
/// end) and its MapData name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalMatch {
    pub queue: bool,
    pub map: String,
}

/// (the port's own) The status of a match by its shape: a queue's by its mode (Silly is two against two), a custom
/// lobby's by its match's ETeamStyle (0 one against one, 1 teams, 2 free-for-all) and its fighters, bots too; another
/// shape (a Versus lobby's two against one, the 2v2 style with three fighters) is a match with no shape named.
fn match_status(mode: Option<QueueMode>, team_style: u8, fighters: usize) -> PresenceStatus {
    match (mode, team_style, fighters) {
        (Some(QueueMode::OneVOne), ..) | (None, 0, 2) => PresenceStatus::OneVOne,
        (Some(QueueMode::TwoVTwo | QueueMode::Silly), ..) | (None, 1, 4) => PresenceStatus::TwoVTwo,
        (Some(QueueMode::Ffa), ..) | (None, 2, _) => PresenceStatus::Ffa,
        _ => PresenceStatus::Match,
    }
}

/// The menus' events kept while no menu takes them (an online match's frames).
const EVENTS_KEPT: usize = 256;

/// What the session gave an online match (app/match/net.ts's listeners): the referee's packets, the host core's
/// match messages (match.go, match.left, match.desync), and the session's end.
#[derive(Debug, Default)]
pub struct MatchInbox {
    pub packets: Vec<Vec<u8>>,
    pub messages: Vec<ServerMessage>,
    pub closed: Option<ClosedWhy>,
    /// The chats of the match (sender id, to whom, the text as it came): the match's chat model cleans and gates
    /// them (match_chat.rs `received`), the menus do not see them.
    pub chats: Vec<(String, mvs_protocol::messages::ChatTo, String)>,
}

/// The menus' line of online play from the sign-in, the gate and the way to the relay (`slow`: NetEvent::Way). The
/// first reason that holds, in order: a gate (a check under way, an update, an import, the self check), online (the
/// slower way when `slow`), a backend signing in, every backend turned off, the relay not reached, else another
/// reason (an Epic sign-in refused, its files missing, LAN not started: Settings and the launcher's test say which).
pub fn line_of(status: &NetStatus, gate: Option<&Gate>, slow: bool) -> OnlineLine {
    if let Some(g) = gate {
        return OnlineLine::Offline(match g {
            Gate::Checking => OfflineWhy::Checking,
            Gate::Update { version } => OfflineWhy::Update { version: version.clone() },
            Gate::Import => OfflineWhy::Import,
            Gate::SelfCheck { .. } => OfflineWhy::SelfCheck,
        });
    }
    if status.online {
        return if slow { OnlineLine::Slower } else { OnlineLine::Online };
    }
    let StatusWhy::Off { backends } = &status.why else { return OnlineLine::Offline(OfflineWhy::SigningIn) };
    let signing = |w: &OffWhy| {
        matches!(
            w,
            OffWhy::SigningIn
                | OffWhy::Connecting { .. }
                | OffWhy::SettingsChanged
                | OffWhy::SignInChanged
                | OffWhy::LanPortChanged
                | OffWhy::LanSettingChanged
        )
    };
    if backends.iter().any(|b| signing(&b.why)) {
        return OnlineLine::Offline(OfflineWhy::SigningIn);
    }
    if backends.iter().all(|b| matches!(b.why, OffWhy::Off | OffWhy::NotOn | OffWhy::TurnedOff)) {
        return OnlineLine::Offline(OfflineWhy::TurnedOff);
    }
    if backends.iter().any(|b| b.kind == BackendKind::Relay && matches!(b.why, OffWhy::Retrying { .. })) {
        return OnlineLine::Offline(OfflineWhy::NotReached);
    }
    OnlineLine::Offline(OfflineWhy::Other)
}

/// (the port's own) The lobby's connection line's link from the session's way, the menus' line now and whether
/// online play was on when the session opened (`opened_online`, NetClient::opened_online).
/// - A local lobby made while no connection was up says so once online play is on: a new lobby would be online (it
///   stays local until the player leaves it). Else its reason is the line's, the gate's included.
/// - A local lobby made while online play was on was made so because the relay, signed in and the only connection
///   on, cannot host a custom game here (Epic or LAN would have hosted it): a new lobby would be local again, so it
///   says that and not "made while offline".
/// - The relay of another version, the same: its reason stays.
///
/// Unverified: the app cannot see a new lobby turn online under a relay that is signed in (the status says online
/// alone, not which connections are; Epic or LAN signing in after it, or the relay's version or hosting cleared, tell
/// nothing), so those two reasons stay until the player leaves the lobby.
pub fn link_of(on: &SessionOn, line: &OnlineLine, opened_online: bool) -> SessionLink {
    match on {
        SessionOn::Relay => SessionLink::Relay,
        SessionOn::Epic => SessionLink::Epic,
        SessionOn::Lan { at } => SessionLink::Lan { at: at.clone() },
        SessionOn::Local { why: LocalWhy::RelayVersion } => SessionLink::Local(LocalReason::RelayVersion),
        SessionOn::Local { why: LocalWhy::Status { why } } if opened_online || matches!(why, StatusWhy::Online) => {
            SessionLink::Local(LocalReason::CannotHost)
        }
        SessionOn::Local { why: LocalWhy::Status { .. } } => SessionLink::Local(match line {
            OnlineLine::Online | OnlineLine::Slower => LocalReason::MadeOffline,
            OnlineLine::Offline(why) => LocalReason::Offline(why.clone()),
        }),
    }
}

/// Whether the relay is signing in again (its retry after a try that did not reach it).
fn relay_connecting(status: &NetStatus) -> bool {
    let StatusWhy::Off { backends } = &status.why else { return false };
    backends.iter().any(|b| b.kind == BackendKind::Relay && matches!(b.why, OffWhy::Connecting { .. }))
}

/// Whether the relay's last try failed on its key (it proved another than the one expected).
fn relay_other_key(status: &NetStatus) -> bool {
    let StatusWhy::Off { backends } = &status.why else { return false };
    backends.iter().any(|b| b.kind == BackendKind::Relay && matches!(b.why, OffWhy::RelayKey { .. }))
}

/// The line kept through the relay's next try: the relay goes Retrying (or RelayKey), Connecting, Retrying at each
/// try, and the line would flip with it (the menus' line, the launcher's from the game's net state).
#[derive(Clone, Debug, Default)]
pub struct LineLatch {
    /// The line of the relay's last try that failed: not reached, or another reason when it proved another key.
    unreached: Option<OnlineLine>,
}

impl LineLatch {
    /// `line` (line_of on `status`) as shown: the failed try's line while the relay tries again after it.
    pub fn follow(&mut self, line: OnlineLine, status: &NetStatus) -> OnlineLine {
        let not_reached = OnlineLine::Offline(OfflineWhy::NotReached);
        if line == not_reached || (line == OnlineLine::Offline(OfflineWhy::Other) && relay_other_key(status)) {
            self.unreached = Some(line.clone());
        } else if let Some(held) = &self.unreached
            && line == OnlineLine::Offline(OfflineWhy::SigningIn)
            && relay_connecting(status)
        {
            return held.clone();
        } else {
            self.unreached = None;
        }
        line
    }
}

/// The longest the screens' pending commands (a join link's, `--online-open`) wait for a sign-in under way or a
/// relay trying again, from the menus' first frame: then they run and fail with the true reason.
pub const PENDING_WAIT_MS: f64 = 10_000.0;

/// Whether a backend of the status is still on its way in: signing in, connecting, or the relay trying again.
pub fn still_signing(status: &NetStatus) -> bool {
    if status.online {
        return false;
    }
    match &status.why {
        StatusWhy::SigningIn => true,
        StatusWhy::Off { backends } => backends.iter().any(|b| {
            matches!(
                b.why,
                OffWhy::SigningIn
                    | OffWhy::Connecting { .. }
                    | OffWhy::Retrying { .. }
                    | OffWhy::SettingsChanged
                    | OffWhy::SignInChanged
            )
        }),
        StatusWhy::Online => false,
    }
}

/// A host core's notice as it is shown ('' none): control and format characters and line breaks made spaces, runs of
/// spaces one, cut to NOTICE_LIMIT characters.
pub fn clean_notice(v: &str) -> String {
    let head: String = v.chars().take(NOTICE_LIMIT * 4).collect();
    // (\p{Cc}\p{Cf}\p{Zl}\p{Zp}: the controls, the format characters, the line and paragraph separators)
    let unshown = |c: char| {
        c.is_control()
            || matches!(c, '\u{2028}' | '\u{2029}' | '\u{00ad}' | '\u{061c}' | '\u{180e}' | '\u{feff}')
            || ('\u{200b}'..='\u{200f}').contains(&c)
            || ('\u{202a}'..='\u{202e}').contains(&c)
            || ('\u{2060}'..='\u{2064}').contains(&c)
            || ('\u{2066}'..='\u{206f}').contains(&c)
            || ('\u{fff9}'..='\u{fffb}').contains(&c)
    };
    let spaced: String = head.chars().map(|c| if unshown(c) { ' ' } else { c }).collect();
    let mut out = String::new();
    for c in spaced.chars() {
        if c == ' ' && out.ends_with(' ') {
            continue;
        }
        out.push(c);
    }
    out.trim().chars().take(NOTICE_LIMIT).collect()
}

impl OnlineHub {
    /// The client over `port`; `recent` the recent players' file, `compat` the compatibility id, `lag` a test delay
    /// on this side's match packets (ms).
    pub fn new(port: NetPort, recent: Option<std::path::PathBuf>, compat: String, tables: Tables, lag: f64) -> Self {
        let data = recent.as_ref().and_then(|r| r.parent()).map(std::path::Path::to_path_buf);
        OnlineHub {
            port,
            client: None,
            target: None,
            session_me: String::new(),
            recent: RecentPlayers::open(recent),
            senders: BTreeMap::new(),
            listed: Vec::new(),
            listed_why: None,
            who_at: f64::NEG_INFINITY,
            who_asking: None,
            whos: 0,
            browses: 0,
            browsing: None,
            browse: None,
            presence_sent: None,
            queue_session: false,
            playing: false,
            playing_as: PresenceStatus::Match,
            prefs: Prefs::default(),
            saved_name: None,
            name_taken: false,
            name_refused: false,
            hello: None,
            hello_icon: String::new(),
            told: None,
            notice_text: String::new(),
            notice_at: f64::NEG_INFINITY,
            refused_at: f64::NEG_INFINITY,
            lobby: None,
            party: None,
            menu_fighter: None,
            fighter_sent: None,
            party_fighters: BTreeMap::new(),
            party_leaving: false,
            tables,
            hub: PreMatchHub::default(),
            queue_end: None,
            queue: None,
            attached: false,
            path: None,
            match_path: None,
            dev_on: None,
            started: None,
            records: Vec::new(),
            open_queue: None,
            profiles: Profiles::default(),
            fake: None,
            fake_lobbies: None,
            bot_after: None,
            local: None,
            compat,
            lag,
            chat: ChatGate::new(1.0, 0.0),
            received: Vec::new(),
            received_all: ChatGate::new(RECEIVE_ALL_TIMES, 0.0),
            profanity: Profanity::new(&BAD_WORDS),
            events: Vec::new(),
            inbox: None,
            pending: Vec::new(),
            link_join: None,
            epoch: std::time::Instant::now(),
            script_clock: None,
            logged: String::new(),
            gate: None,
            self_check: None,
            self_check_differs: None,
            data,
            desync_at: None,
            latch: LineLatch::default(),
            pending_since: None,
        }
    }

    /// The hub's clock (ms since it was made, or the script's): the owner's `now`.
    pub fn clock(&self) -> f64 {
        self.script_clock.unwrap_or_else(|| self.epoch.elapsed().as_secs_f64() * 1000.0)
    }

    /// The compatibility id every join carries (made once the store is open).
    pub fn compat(&self) -> &str {
        &self.compat
    }

    pub fn set_compat(&mut self, c: String) {
        self.compat = c;
    }

    /// Sets or clears what keeps online play off (the update and import checks, the self check).
    pub fn set_gate(&mut self, gate: Option<Gate>) {
        self.gate = gate;
    }

    pub fn gate(&self) -> Option<&Gate> {
        self.gate.as_ref()
    }

    /// The menus' line (line_of): a relay that was not reached stays so through its next try's "connecting", so the
    /// line does not flip at each retry.
    pub fn line(&mut self) -> OnlineLine {
        let line = line_of(&self.port.status, self.gate.as_ref(), self.port.slow);
        self.latch.follow(line, &self.port.status)
    }

    /// Whether the screens' pending commands wait this frame: while the gate is set, and while a backend is still on
    /// its way in (a relay trying again included) for at most PENDING_WAIT_MS from the first frame that asked.
    pub fn pending_held(&mut self, now: f64) -> bool {
        let since = *self.pending_since.get_or_insert(now);
        self.gate.is_some() || (still_signing(&self.port.status) && now - since < PENDING_WAIT_MS)
    }

    /// A command of the command line or of a join link, run once the pending commands no longer wait: a join's code
    /// is kept for the lobby screen (`link_join`, the later one wins), the rest goes in `pending`.
    pub fn push_open(&mut self, cmd: OnlineCmd) {
        match cmd {
            OnlineCmd::Open(SessionTarget::Join(code)) => self.link_join = Some(code),
            c => self.pending.push(c),
        }
    }

    /// A note for the menus, in the next view's events (the desync report's).
    pub fn notice(&mut self, n: OnlineNotice) {
        self.events.push(OnlineEvent::Notice(n));
    }

    /// This player's id as the host cores know it: the last session's own, else the sign-in's.
    pub fn me(&self) -> String {
        if self.session_me.is_empty() { self.port.status.me.clone() } else { self.session_me.clone() }
    }

    /// The session is the queue's (online.ts queued).
    pub fn queued(&self) -> bool {
        self.queue_session && self.client.as_ref().is_some_and(|c| c.open)
    }

    /// A player by id: one met recently, else one on the online list, else one who sent an invitation or a toast.
    pub fn player(&self, id: &str) -> Option<OnlinePlayer> {
        self.players()
            .into_iter()
            .find(|p| p.id == id)
            .or_else(|| self.listed.iter().find(|p| p.id == id).cloned())
            .or_else(|| self.senders.get(id).cloned())
    }

    fn players(&self) -> Vec<OnlinePlayer> {
        self.recent
            .players()
            .iter()
            .map(|p| {
                let now = self.listed.iter().find(|l| l.id == p.id);
                OnlinePlayer {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    busy: now.is_some_and(|l| l.busy),
                    icon: p.icon.clone(),
                    present: now.is_some(),
                    status: now.and_then(|l| l.status),
                }
            })
            .collect()
    }

    // ---------- the frame ----------

    /// One frame: the settings read, the service's events taken, the timers run, the presence published. What the
    /// menus see now (its events drained).
    pub fn frame(&mut self, prefs: Prefs, now: f64) -> OnlineView {
        if prefs.icon != self.prefs.icon {
            self.set_icon(prefs.icon.clone());
        }
        self.prefs = prefs;
        self.poll(now);
        self.view()
    }

    /// The service's events taken and the timers run, as `frame` without its view (an online match's frames: the
    /// menus' events kept for the next view, the newest EVENTS_KEPT).
    pub fn poll(&mut self, now: f64) {
        for (e, of) in self.port.poll() {
            self.event(e, of, now);
        }
        self.pump(now);
        self.publish();
        self.send_menu_fighter();
        self.log_state();
        if self.events.len() > EVENTS_KEPT {
            self.events.drain(..self.events.len() - EVENTS_KEPT);
        }
    }

    /// The session's state as one line, written to the log when it changes (the two-exe runs follow it); none
    /// without the net service (online off).
    fn log_state(&mut self) {
        if !self.port.has_link() {
            return;
        }
        let line = self.state_line();
        if line != self.logged {
            eprintln!("mvs: online {line}");
            self.logged = line;
        }
    }

    fn state_line(&self) -> String {
        let mut l = format!("connected={}", self.port.status.online);
        if let Some(c) = self.client.as_ref().filter(|c| c.open) {
            l += &format!(" session={} code={} me={} links={}", c.session, c.code, c.me, c.links.len());
        }
        if let Some(q) = &self.queue {
            l += &format!(
                " queue={:?} searching={} found={} started={} live={}",
                q.modes,
                q.searching,
                q.found.is_some(),
                q.started.is_some(),
                q.live
            );
        }
        if let Some(pm) = &self.hub.current {
            l += &format!(" pm={} pm_started={} pm_ended={:?}", pm.begin.id, pm.started.is_some(), pm.ended);
        }
        if self.playing {
            l += " playing";
        }
        if self.local.is_some() {
            l += " local";
        }
        if let Some(q) = &self.queue_end {
            let v = q.view();
            l += &format!(
                " queue_end votes={:?} set={:?} rematch={} ended={:?}",
                v.votes, v.set, v.rematch_begun, v.ended
            );
        }
        l
    }

    /// The online match's inbox kept open (the match begun: what came since its start was taken stays) or closed
    /// (its end, or a start that made no online match).
    pub fn match_inbox(&mut self, on: bool) {
        self.inbox = if on { Some(self.inbox.take().unwrap_or_default()) } else { None };
    }

    /// A match started: its inbox opened now, so a match.left or the session's end before the program takes the
    /// start (the pre-match's last screens, the loading cover) reaches the match's start gate.
    fn start(&mut self, m: NetMatchStart) {
        self.inbox = Some(MatchInbox::default());
        self.started = Some(m);
    }

    /// What came for the online match since the last call.
    pub fn take_inbox(&mut self) -> MatchInbox {
        self.inbox.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// The toasts received since the last call, taken out of the menus' events (the results screen's podium shows
    /// them as they come: toasts.ts's listener ran under every page; the other events wait for the menus).
    pub fn take_toasts(&mut self) -> Vec<OnlineEvent> {
        let (toasts, rest) =
            std::mem::take(&mut self.events).into_iter().partition(|e| matches!(e, OnlineEvent::Toast { .. }));
        self.events = rest;
        toasts
    }

    /// What the menus see (the events since the last call drained).
    pub fn view(&mut self) -> OnlineView {
        let line = self.line();
        let on = self.session_on().cloned();
        let session = self.client.as_ref().filter(|c| c.open).map(|c| SessionView {
            link: on.as_ref().map(|on| link_of(on, &line, c.opened_online)),
            code: c.code.clone(),
            me: c.me.clone(),
            chosen: c.chosen,
            host: matches!(self.target, Some(SessionTarget::Host)),
            lobby: self.lobby.clone(),
            queued: self.queue_session,
        });
        let pre_match = self.queue.as_ref().map(|q| {
            Box::new(PreMatchView {
                searching: q.searching,
                registered: q.registered((&self.me_or_local(), &self.prefs.name)).map(Box::new),
            })
        });
        OnlineView {
            now: self.clock(),
            connected: self.port.status.online,
            line,
            can_join: self.port.has_link(),
            me: self.me(),
            name: self.prefs.name.clone(),
            icon: self.prefs.icon.clone(),
            name_taken: self.name_taken,
            name_refused: self.name_refused,
            players: self.players(),
            listed: self.listed.clone(),
            listed_why: self.listed_why.clone(),
            session,
            party: self.party.clone(),
            browse: self.browse.clone(),
            ask: self.client.as_ref().and_then(NetClient::asked).map(|a| SessionAsk { id: a.id, what: a.what.clone() }),
            pre_match,
            queue_end: self.queue_end.as_ref().map(|q| q.view().clone()),
            events: std::mem::take(&mut self.events),
            compat: self.compat.clone(),
        }
    }

    /// (the port's own) What the open session is connected through (None: none open yet); the dev's override first.
    pub fn session_on(&self) -> Option<&SessionOn> {
        let c = self.client.as_ref().filter(|c| c.open)?;
        self.dev_on.as_ref().or(c.on.as_ref())
    }

    /// (the port's own) The open session's host core runs in this game: a lobby it hosts over Epic, a local network
    /// or no connection (the relay's core is the relay's).
    pub fn own_core(&self) -> bool {
        matches!(self.target, Some(SessionTarget::Host))
            && matches!(self.session_on(), Some(SessionOn::Epic | SessionOn::Lan { .. } | SessionOn::Local { .. }))
    }

    /// (the port's own) The ping line's number in the menus: ms to the open session's host core and back, the last
    /// measured (None: no session, not ready yet, none measured, or a core of this game's own: a local lobby, or the
    /// host of one over Epic or a local network, whose round trip to its own core says nothing; the app holds no
    /// measure of its guests' round trips to it, which only the guests take).
    pub fn ping_ms(&self) -> Option<f64> {
        let c = self.client.as_ref().filter(|c| c.open && c.ready && c.ping >= 0.0)?;
        if self.own_core() || matches!(self.session_on(), None | Some(SessionOn::Local { .. })) {
            return None;
        }
        Some(c.ping)
    }

    /// (the port's own) The last pm.path of the session: the referee's slot (-1 the relay) and each slot's round trip
    /// to it, kept through the match it chose.
    pub fn match_path(&self) -> Option<&(i32, Vec<Option<u32>>)> {
        self.match_path.as_ref()
    }

    fn me_or_local(&self) -> String {
        let me = self.me();
        if me.is_empty() { "local".to_owned() } else { me }
    }

    /// When `frame` has a timer due next (None: nothing until an event).
    pub fn next_due(&self) -> Option<f64> {
        let mut due = [
            self.client.as_ref().and_then(NetClient::next_due),
            self.hello.as_ref().map(|h| h.until),
            self.who_asking.map(|w| w.1),
            self.browsing.map(|b| b.1),
            self.hub.current.as_ref().and_then(super::pre_match::NetPreMatch::next_due),
            self.queue_end.as_ref().and_then(NetQueueEnd::next_due),
            self.queue.as_ref().and_then(QueueSession::next_due),
            self.profiles.next_due(),
            self.fake.as_ref().and_then(super::fake_queue::FakeCore::next_due),
        ]
        .into_iter()
        .flatten();
        let first = due.next()?;
        Some(due.fold(first, f64::min))
    }

    fn pump(&mut self, now: f64) {
        if let Some(c) = &mut self.client {
            let news = c.pump(&mut self.port, now);
            self.session_news(news, now);
        }
        if self.hello.as_ref().is_some_and(|h| now >= h.until) {
            // no answer: the name stands, and the next session asks again
            self.hello_done(None);
        }
        if self.who_asking.is_some_and(|w| now >= w.1) {
            self.who_asking = None;
            self.take_listed(Vec::new(), Some(BrowseNote::NoAnswer));
        }
        if self.browsing.is_some_and(|b| now >= b.1) {
            self.browsing = None;
            self.browse = Some(BrowseAnswer { games: Vec::new(), why: BrowseNote::NoAnswer });
        }
        if let Some(pm) = &mut self.hub.current
            && let Some(m) = pm.pump(now)
        {
            self.send(&m);
        }
        if let Some(q) = &mut self.queue_end {
            let news = q.pump(&mut self.hub, now);
            self.qe_news(news);
        }
        let mut out = Vec::new();
        if let Some(q) = &mut self.queue {
            q.pump(now, &mut out);
        }
        self.q_out(out);
        self.deliver();
        self.profiles.pump(now);
        let fake = self.fake.as_mut().map(|f| f.pump(now)).unwrap_or_default();
        for m in fake {
            self.message(m, now);
        }
    }

    // ---------- the service's events ----------

    fn event(&mut self, e: NetEvent, of: u32, now: f64) {
        match &e {
            // (the port keeps the sign-in and the way: the line reads them)
            NetEvent::Status { .. } | NetEvent::Way { .. } => return,
            NetEvent::Invited { kind, code, from_id, from_name } => {
                // (the port's own) with Invites off no invite is shown, a party's or a custom game's (the relay
                // refuses them too, presence `invites`): the build's Should Show Invite? @76-357 with its
                // Popups-PartyInvitesFriendsOnly preference
                if !self.prefs.invites {
                    return;
                }
                self.sender(from_id, from_name);
                self.events.push(OnlineEvent::Invited(LobbyInvite {
                    kind: *kind,
                    lobby: code.clone(),
                    from_id: from_id.clone(),
                    from_name: from_name.clone(),
                }));
                return;
            }
            NetEvent::InviteRefused { to } => {
                if now - self.refused_at < NOTICE_EVERY_MS {
                    return;
                }
                self.refused_at = now;
                self.events.push(OnlineEvent::Notice(OnlineNotice::InviteNotSent { to: to.clone() }));
                return;
            }
            NetEvent::Who { id, players, why } => {
                if self.who_asking.is_some_and(|w| w.0 == *id) {
                    self.who_asking = None;
                    let note = (*why != BrowseWhy::All).then(|| BrowseNote::Net(why.clone()));
                    let list = players
                        .iter()
                        .map(|p| OnlinePlayer {
                            id: p.id.clone(),
                            name: p.name.clone(),
                            busy: p.busy,
                            icon: p.icon.clone(),
                            present: true,
                            status: p.status,
                        })
                        .collect();
                    self.take_listed(list, note);
                }
                return;
            }
            NetEvent::Browsed { id, lobbies, why } => {
                if self.browsing.is_some_and(|b| b.0 == *id) {
                    self.browsing = None;
                    let games = lobbies
                        .iter()
                        .filter_map(|l| {
                            let info = serde_json::Value::String(l.info.clone());
                            let listing = mvs_protocol::lobby_listing::listing_of(&info)?;
                            Some(BrowsedGame {
                                code: l.code.clone(),
                                backend: Some(l.backend),
                                members: l.members,
                                max: l.max,
                                listing,
                            })
                        })
                        .collect();
                    self.browse = Some(BrowseAnswer { games, why: BrowseNote::Net(why.clone()) });
                }
                return;
            }
            NetEvent::Party { party, why: _ } => {
                let was_full = self.party_full().is_some();
                self.party.clone_from(party);
                // (a member gone takes its fighter with it)
                let members = self.party.as_ref().map(|p| p.members.as_slice()).unwrap_or_default();
                self.party_fighters.retain(|id, _| members.iter().any(|m| m.id == *id));
                let full = self.party_full().is_some();
                // PGI OpenPartyDisbandedNotification (@49359), fired by native code: the one a full party ended under
                // without its own leave (the member its leader left, the leader its member left) is told.
                // Unverified: which side the build tells and on which party change (native, the spec's N2).
                if was_full && !full && !self.party_leaving {
                    self.events.push(OnlineEvent::Notice(OnlineNotice::PartyDisbanded));
                }
                if full || !was_full {
                    self.party_leaving = false;
                }
                // the party's search ends under this game when the party does
                if self.party.as_ref().is_none_or(|p| p.members.len() < 2) {
                    let mut out = Vec::new();
                    if let Some(q) = &mut self.queue {
                        q.party_ended(KickNote::PartySearchEnded, &mut out);
                    }
                    self.q_out(out);
                }
                return;
            }
            NetEvent::PartySay { from, say } => {
                self.party_say(from, say, now);
                return;
            }
            NetEvent::PartyFighter { from, fighter } => {
                match fighter {
                    Some(f) => self.party_fighters.insert(from.clone(), f.clone()),
                    None => self.party_fighters.remove(from),
                };
                return;
            }
            _ => {}
        }
        let Some(c) = &mut self.client else { return };
        let news = c.take(&mut self.port, &e, of, now);
        self.session_news(news, now);
    }

    fn sender(&mut self, id: &str, name: &str) {
        let p = OnlinePlayer { id: id.to_owned(), name: name.to_owned(), ..Default::default() };
        self.senders.insert(id.to_owned(), p);
    }

    fn take_listed(&mut self, players: Vec<OnlinePlayer>, why: Option<BrowseNote>) {
        let me = self.me();
        self.listed = players.into_iter().filter(|p| !p.id.is_empty() && p.id != me).collect();
        self.listed_why = why;
    }

    fn session_news(&mut self, news: Vec<News>, now: f64) {
        for n in news {
            match n {
                News::Ready => self.ready(now),
                News::Failed(f) => {
                    // a session that could not be opened answers with its close
                    let why = match f {
                        OpenFailed::Mismatch => {
                            // (the port's own) a lobby a player's game hosts names that game, the relay's the server
                            // (the client closed itself at the mismatch: its way is read off it all the same)
                            let on = self.dev_on.as_ref().or(self.client.as_ref().and_then(|c| c.on.as_ref()));
                            let player = matches!(on, Some(SessionOn::Epic | SessionOn::Lan { .. }));
                            self.tell(if player {
                                OnlineNotice::HostProtocolDiffers
                            } else {
                                OnlineNotice::ProtocolDiffers
                            });
                            ClosedWhy::Other { text: String::new() }
                        }
                        OpenFailed::NoAnswer => ClosedWhy::HostSilent,
                        OpenFailed::Closed(why) => why,
                    };
                    self.closed(why);
                }
                News::Closed(why) => self.closed(why),
                News::Ask(_) => {}
                News::Packets { from, packets } => {
                    let _ = from;
                    if let Some(i) = &mut self.inbox {
                        i.packets.extend(packets);
                    }
                }
                News::Msg(m) => self.message(*m, now),
            }
        }
    }

    // The session is open: its owner's state, its hello.
    fn ready(&mut self, now: f64) {
        let Some(c) = &self.client else { return };
        self.session_me.clone_from(&c.me);
        self.events.push(OnlineEvent::Opened);
        let local = matches!(c.on, Some(SessionOn::Local { .. }));
        let mut name = self.prefs.name.clone();
        // (the port's own) a local lobby is made with no name chosen too (the name is asked for online play): its
        // core takes no lobby request before a hello, so it greets with the local player's name
        if name.is_empty() && local {
            name = mvs_ui::menu::custom_text::LOCAL_PLAYER_NAME.to_owned();
        }
        if !name.is_empty() {
            self.send_hello(name, false, now);
        }
    }

    fn send_hello(&mut self, name: String, claim: bool, now: f64) {
        self.hello_icon.clone_from(&self.prefs.icon);
        let icon = Some(self.prefs.icon.clone());
        // (the release's semver, as platform appVersion: the host core keeps only a bare version)
        let app = mvs_release::VERSION.to_owned();
        // (epic, rtt and direct are the net service's to add)
        self.send(&ClientMessage::Hello {
            proto: PROTOCOL,
            name: name.clone(),
            icon,
            app,
            epic: None,
            rtt: None,
            direct: None,
        });
        self.hello = Some(Hello { name, until: now + ANSWER_MS, claim });
    }

    // hello's answer: None ok (or none in time), Some(refused) a taken or refused name.
    fn hello_done(&mut self, refused: Option<bool>) {
        let Some(h) = self.hello.take() else { return };
        match refused {
            None if h.claim => {
                self.name_taken = false;
                self.name_refused = false;
                self.saved_name = Some(h.name);
            }
            None => {}
            Some(r) => {
                if !h.claim {
                    self.name_taken = true;
                    self.name_refused = r;
                }
                self.events.push(OnlineEvent::NameTaken { refused: r });
            }
        }
    }

    fn closed(&mut self, why: ClosedWhy) {
        eprintln!("mvs: online session closed: {why:?}");
        if self.client.as_ref().is_some_and(|c| c.open) {
            return;
        }
        self.client = None;
        self.target = None;
        self.queue_session = false;
        self.playing = false;
        self.hello = None;
        self.lobby = None;
        self.path = None;
        self.match_path = None;
        self.session_lost();
        self.profiles.closed();
        let mut out = Vec::new();
        if let Some(q) = &mut self.queue {
            q.connection_lost(0.0, &mut out);
        }
        self.q_out(out);
        self.deliver();
        if let Some(i) = &mut self.inbox {
            i.closed = Some(why.clone());
        }
        self.events.push(OnlineEvent::Closed(why));
    }

    fn message(&mut self, m: ServerMessage, now: f64) {
        if matches!(m, ServerMessage::PmBegin(_) | ServerMessage::QueueFound(_)) {
            self.match_path = None;
        }
        match &m {
            ServerMessage::PmPath { referee, ms, .. } => {
                self.match_path = Some((*referee, ms.clone()));
                self.path = Some(m.clone());
            }
            ServerMessage::PmBegin(_)
            | ServerMessage::QueueFound(_)
            | ServerMessage::PmEnd { .. }
            | ServerMessage::MatchGo { .. }
            // (a found queue match ended before it began: the host core's prematch.rs ends it with this)
            | ServerMessage::QueueState { on: false, .. } => self.path = None,
            _ => {}
        }
        match &m {
            // a custom lobby's pre-match and match: busy from its pm.begin to its record, its pre-match's end or the
            // match's end for all
            ServerMessage::PmBegin(b) => {
                self.playing = true;
                self.playing_as = match_status(b.mode, b.team_style, b.players.len());
                let met: Vec<MetPlayer> = b
                    .players
                    .iter()
                    .filter(|p| !p.bot)
                    .map(|p| MetPlayer { id: p.id.clone(), name: p.name.clone(), icon: None })
                    .collect();
                self.recent.met(&met, &self.me(), wall_ms());
            }
            ServerMessage::PmEnd { .. } => self.playing = false,
            ServerMessage::MatchRecord { .. } => {
                self.playing = false;
                if let Ok(v) = serde_json::to_value(&m) {
                    self.records.push(v);
                }
            }
            ServerMessage::MatchLeft { slot: -1, .. } => self.playing = false,
            ServerMessage::Welcome { name, .. } => {
                self.told = None;
                if self.prefs.icon != self.hello_icon {
                    self.send(&ClientMessage::ProfileIcon { icon: self.prefs.icon.clone() });
                }
                // Deviation: a rename's welcome with another name than asked is the host core's answer to a second
                // rename within its RENAME_MS (mvs_host core.rs): the name is kept, the new one refused as taken
                let kept = self.hello.as_ref().is_some_and(|h| h.claim && clean_name(&h.name) != *name);
                self.hello_done(if kept { Some(false) } else { None });
            }
            ServerMessage::NameTaken { .. } => self.hello_done(Some(false)),
            ServerMessage::NameRefused { .. } => self.hello_done(Some(true)),
            ServerMessage::Notice { text } => {
                self.host_notice(text, now);
                return;
            }
            ServerMessage::Toast { from, name } => {
                if from.is_empty() {
                    return;
                }
                self.sender(from, name);
                self.events.push(OnlineEvent::Toast { from: from.clone(), name: name.clone() });
                return;
            }
            ServerMessage::ProfileAsk { from } => {
                self.profiles.asked(from, now);
                return;
            }
            ServerMessage::Profile { id, profile } => self.profiles.got(id, profile.clone(), now),
            ServerMessage::LobbyState { data, .. } => {
                // the humans of the lobby are met
                let met: Vec<MetPlayer> = data
                    .teams
                    .iter()
                    .flatten()
                    .filter(|p| !p.is_bot)
                    .map(|p| MetPlayer {
                        id: p.account_id.clone(),
                        name: p.username.clone(),
                        icon: Some(p.icon.clone()),
                    })
                    .collect();
                self.recent.met(&met, &self.me(), wall_ms());
                self.lobby = Some(data.clone());
            }
            ServerMessage::Chat { from, to, text } if self.inbox.is_some() => {
                if let Some(i) = &mut self.inbox {
                    i.chats.push((from.clone(), *to, text.clone()));
                }
                return;
            }
            ServerMessage::Chat { from, to, text } => {
                let Some(t) = self.receive_chat(from, text, now) else { return };
                let m = ServerMessage::Chat { from: from.clone(), to: *to, text: t };
                self.events.push(OnlineEvent::Server(Box::new(m)));
                return;
            }
            _ => {}
        }
        // (each pm.begin's session, from its first message on: pre_match.rs)
        match self.hub.take(&m, &self.tables) {
            Some(HubNews::Begun) => {
                self.attached = false;
                let begin = self.hub.current.as_ref().map(|c| c.begin.clone());
                if let (Some(q), Some(b)) = (&mut self.queue_end, begin) {
                    let news = q.begun(&b, &mut self.hub, now);
                    self.qe_news(news);
                }
            }
            Some(HubNews::LostEnd(why, name)) => {
                if let Some(q) = &mut self.queue_end {
                    let news = q.lost_end(why, name, &mut self.hub);
                    self.qe_news(news);
                }
            }
            None => {}
        }
        if let Some(q) = &mut self.queue_end {
            let news = q.take(&m, &mut self.hub, now);
            self.qe_news(news);
        }
        let me = self.me_or_local();
        let name = self.prefs.name.clone();
        let mut out = Vec::new();
        if let Some(q) = &mut self.queue {
            q.take(&m, &self.tables, (&me, &name), now, &mut out);
        }
        self.q_out(out);
        self.deliver();
        self.queue_started();
        if let Some(i) = &mut self.inbox
            && matches!(
                m,
                ServerMessage::MatchGo { .. } | ServerMessage::MatchLeft { .. } | ServerMessage::MatchDesync { .. }
            )
        {
            i.messages.push(m.clone());
        }
        // (a pong is the client's own)
        if !matches!(m, ServerMessage::Pong { .. }) {
            self.events.push(OnlineEvent::Server(Box::new(m)));
        }
    }

    // chat.ts receiveChat: the cleaned text, or None past the sender's or all senders' budget.
    fn receive_chat(&mut self, from: &str, text: &str, now: f64) -> Option<String> {
        if from.is_empty() {
            return None;
        }
        let t = clean_chat(&serde_json::Value::String(text.to_owned()));
        if t.is_empty() {
            return None;
        }
        let at = match self.received.iter().position(|(f, _)| f == from) {
            Some(i) => i,
            None => {
                if self.received.len() >= RECEIVE_SENDERS {
                    self.received.remove(0);
                }
                self.received.push((from.to_owned(), ChatGate::new(RECEIVE_TIMES, now)));
                self.received.len() - 1
            }
        };
        let one = self.received[at].1.allow(now);
        (one && self.received_all.allow(now)).then_some(t)
    }

    fn host_notice(&mut self, v: &str, now: f64) {
        let text = clean_notice(v);
        if text.is_empty()
            || now - self.notice_at < NOTICE_EVERY_MS
            || (text == self.notice_text && now - self.notice_at < NOTICE_AGAIN_MS)
        {
            return;
        }
        self.notice_text.clone_from(&text);
        self.notice_at = now;
        self.events.push(OnlineEvent::Notice(OnlineNotice::Host(text)));
    }

    fn tell(&mut self, note: OnlineNotice) {
        if self.told.as_ref() != Some(&note) {
            self.events.push(OnlineEvent::Notice(note.clone()));
        }
        self.told = Some(note);
    }

    // ---------- the presence ----------

    // (the port's own) what the online list shows of this player, sent at each change: the name, the equipped icon,
    // busy (a queue session open, or a custom lobby's pre-match or match running), the two settings and the status.
    // Not sent before the player has a name. The net service paces the sets (the newest wins), and the list is pulled
    // by the others ('who'): a change is never pushed to anyone.
    fn publish(&mut self) {
        if self.prefs.name.is_empty() || !self.port.has_link() {
            return;
        }
        let p = Presence {
            name: self.prefs.name.clone(),
            icon: self.prefs.icon.clone(),
            busy: self.queued() || self.playing,
            listed: self.prefs.listed,
            invites: self.prefs.invites,
            status: Some(self.status()),
        };
        if self.presence_sent.as_ref() == Some(&p) {
            return;
        }
        self.presence_sent = Some(p.clone());
        self.port.post(NetRequest::Presence(p));
    }

    /// The fighter this player's main menu shows on its Player1 actor (a fighter key and a SkinData name, "" the
    /// default skin): sent with its party card at the next frame, kept for a party made later.
    pub fn set_menu_fighter(&mut self, f: Option<MenuFighter>) {
        self.menu_fighter = f;
    }

    // (the port's own) the main-menu fighter to the service at each change, as the presence goes
    fn send_menu_fighter(&mut self) {
        if !self.port.has_link() || self.fighter_sent.as_ref() == Some(&self.menu_fighter) {
            return;
        }
        self.fighter_sent = Some(self.menu_fighter.clone());
        self.port.post(NetRequest::MenuFighter(self.menu_fighter.clone()));
    }

    /// (the port's own) What this player is doing, for the online list: a pre-match or match (a custom lobby's, or a
    /// queue set's from its found match to its end screen) by its shape, the queue's screen, a custom lobby, else the
    /// menus.
    pub fn status(&self) -> PresenceStatus {
        if self.playing {
            return self.playing_as;
        }
        if let Some(q) = self.queue_end.as_ref().filter(|q| !q.closed()) {
            return match_status(Some(q.mode), 0, 0);
        }
        if let Some(f) = self.queue.as_ref().and_then(|q| q.found.as_ref()) {
            return match_status(Some(f.mode), 0, 0);
        }
        if self.queued() {
            return PresenceStatus::Queue;
        }
        if self.client.as_ref().is_some_and(|c| c.open) {
            return PresenceStatus::Lobby;
        }
        PresenceStatus::Menus
    }

    fn set_icon(&mut self, icon: String) {
        if let Some(c) = &self.client
            && c.open
        {
            c.send(&mut self.port, &ClientMessage::ProfileIcon { icon: icon.clone() });
        }
        self.prefs.icon = icon;
    }

    // ---------- the commands ----------

    fn send(&mut self, m: &ClientMessage) {
        let now = self.clock();
        if self.queue_session
            && let Some(f) = &mut self.fake
        {
            f.send(m, now);
            return;
        }
        if let Some(c) = &self.client
            && c.open
        {
            c.send(&mut self.port, m);
        }
    }

    /// A local match's end (app/match/index.ts's end with `lm.over`): its result when it went to its end (`podium`,
    /// the end of game's), a queue match's through its set's end, then match.leave (the server counts a leave with no
    /// result as a quit).
    pub fn local_over(&mut self, podium: Option<&[mvs_sim::world::flow::PodiumEntry]>) {
        let Some(l) = self.local.take() else { return };
        let now = self.clock();
        if let Some(p) = podium {
            let (team, players) = super::net_match::report_of(p);
            let m = match &mut self.queue_end {
                Some(q) if l.queue => q.send_result(team, Some(players), &l.map, now),
                _ => ClientMessage::MatchResult {
                    team,
                    players: Some(players),
                    map: (!l.map.is_empty()).then(|| l.map.clone()),
                },
            };
            self.send(&m);
        }
        self.send(&ClientMessage::MatchLeave);
    }

    /// A match chat line of the match's chat model (its Submit::Send: cleaned and paced there), on the session.
    pub fn send_match_chat(&mut self, text: &str, all: bool) {
        self.send(&ClientMessage::Chat { text: text.to_owned(), all });
    }

    /// loadProfile: another player's profile, asked of their game through the open session.
    pub fn profile(&mut self, id: &str, now: f64) -> ProfileAnswer {
        let open = self.client.as_ref().is_some_and(|c| c.open);
        let mut out = Vec::new();
        let a = self.profiles.ask(id, open, now, &mut |m| out.push(m));
        for m in out {
            self.send(&m);
        }
        a
    }

    /// profile.tell: `to`'s ask answered with this game's profile (None: no stats file).
    pub fn tell_profile(&mut self, to: &str, p: Option<mvs_protocol::profile::ProfileWire>) {
        let m = super::profile::tell(to, p);
        self.send(&m);
    }

    /// online.ts onClose, run at every close of an open session (the host's, a lost connection, this game's own
    /// leave): the running pre-match and the set's end session hear it is gone.
    fn session_lost(&mut self) {
        self.hub.connection_lost();
        let now = self.clock();
        if let Some(q) = &mut self.queue_end {
            let news = q.connection_lost(&mut self.hub, now);
            self.qe_news(news);
        }
    }

    /// This game's own leave of its open session (NetClient.close: onClose told at once).
    fn leave_session(&mut self) {
        let Some(c) = self.client.as_mut().filter(|c| c.open) else { return };
        c.close(&mut self.port);
        self.session_lost();
    }

    /// Opens the game's session (the one open is left first). `asks`: its questions are shown.
    pub fn open(&mut self, target: SessionTarget, now: f64) {
        self.leave_session();
        self.session_me.clear();
        self.queue_session = matches!(target, SessionTarget::Queue);
        self.playing = false;
        self.lobby = None;
        self.hello = None;
        self.client = Some(NetClient::open(&mut self.port, target.clone(), now, true, self.lag));
        self.target = Some(target);
    }

    /// Leaves the session.
    pub fn close(&mut self) {
        self.leave_session();
        self.client = None;
        self.target = None;
        self.queue_session = false;
        self.playing = false;
        self.lobby = None;
        self.hello = None;
    }

    /// A screen's command; `loadout` this game's loadout for a character (menu/loadout.ts localNetLoadout).
    pub fn run(&mut self, cmd: OnlineCmd, loadout: &dyn Fn(&str) -> Option<NetLoadout>, now: f64) {
        match cmd {
            OnlineCmd::Open(t) => self.open(t, now),
            OnlineCmd::Send(m) => self.send(&m),
            OnlineCmd::Close => self.close(),
            OnlineCmd::Invite { to } => {
                if let Some(c) = &self.client {
                    c.invite(&mut self.port, to, self.prefs.name.clone());
                }
            }
            OnlineCmd::Browse => {
                if let Some(f) = &self.fake_lobbies {
                    let games = super::fake_lobbies::listings(f, &self.compat, &self.tables);
                    self.browse = Some(BrowseAnswer { games, why: BrowseNote::Net(BrowseWhy::All) });
                    return;
                }
                if !self.port.has_link() {
                    self.browse = Some(BrowseAnswer { games: Vec::new(), why: BrowseNote::NoApp });
                    return;
                }
                self.browses += 1;
                self.browsing = Some((self.browses, now + BROWSE_MS));
                self.browse = None;
                self.port.post(NetRequest::Browse { id: self.browses });
            }
            OnlineCmd::Who => {
                if self.who_asking.is_some() || now - self.who_at < WHO_AGAIN_MS {
                    return;
                }
                self.who_at = now;
                if !self.port.has_link() {
                    return self.take_listed(Vec::new(), Some(BrowseNote::NoApp));
                }
                self.whos += 1;
                self.who_asking = Some((self.whos, now + BROWSE_MS));
                self.port.post(NetRequest::Who { id: self.whos });
            }
            OnlineCmd::Answer { id, yes } => {
                if let Some(c) = &mut self.client {
                    c.answer(&mut self.port, id, yes, now);
                }
            }
            OnlineCmd::SetName(text) => self.claim(&text, now),
            OnlineCmd::SetIcon(icon) => self.set_icon(icon),
            OnlineCmd::Toast { to } => self.send(&ClientMessage::Toast { to }),
            OnlineCmd::Party(p) => self.party_cmd(p),
            OnlineCmd::Chat { text, all } => {
                let t = clean_chat(&serde_json::Value::String(text));
                if !t.is_empty() && self.chat.allow(now) {
                    self.send(&ClientMessage::Chat { text: t, all });
                }
            }
            OnlineCmd::Queue { modes } => self.queue_open(modes, now),
            OnlineCmd::PreMatch(c) => self.pre_match_cmd(c, loadout, now),
            OnlineCmd::Vote(yes) => {
                if let Some(m) = self.queue_end.as_mut().and_then(|q| q.vote(yes)) {
                    self.send(&m);
                }
            }
            // (queue-end.ts leave: pm.leave alone; the set's end session hears on to the screen's end)
            OnlineCmd::LeaveSet => {
                if self.queue_end.is_some() {
                    self.send(&NetQueueEnd::leave());
                }
            }
        }
    }

    // identity.ts updateUsername and online.ts claim. Deviation, the port's own rules: a name is its text without
    // the spaces around it, at most NAME_LIMIT characters, not empty; one with a bad word is refused here and by the
    // host core; and the core refuses one that someone online has. Without a session it is kept, and the next
    // session's hello carries it.
    fn claim(&mut self, text: &str, now: f64) {
        let name: String = text.trim().chars().take(NAME_LIMIT).collect();
        if name.is_empty() {
            return self.events.push(OnlineEvent::NameTaken { refused: false });
        }
        if self.profanity.refuses(&name) {
            return self.events.push(OnlineEvent::NameTaken { refused: true });
        }
        if self.client.as_ref().is_none_or(|c| !c.open || !c.ready) {
            self.name_taken = false;
            self.name_refused = false;
            self.saved_name = Some(name);
            return;
        }
        self.send_hello(name, true, now);
    }

    // ---------- the party (net/party.ts, app/party-flow.ts) ----------

    fn party_cmd(&mut self, p: PartyCmd) {
        let m = match p {
            PartyCmd::Invite { to } => NetRequest::PartyInvite { to, name: self.prefs.name.clone() },
            PartyCmd::Join { code } => NetRequest::PartyJoin { code },
            PartyCmd::Leave => {
                self.party_leaving = self.party.is_some();
                NetRequest::PartyLeave
            }
            PartyCmd::Say(s) => {
                if self.party.is_none() {
                    return;
                }
                NetRequest::PartySay(s)
            }
        };
        self.port.post(m);
    }

    /// This game's role in a full party (the queue screen's PreMatchSetup.party; None: no full party).
    pub fn view_party_role(&self) -> Option<mvs_ui::menu::online::PartyRole> {
        use mvs_ui::menu::online::PartyRole;
        self.party_full().map(|(leader, _)| if leader { PartyRole::Leader } else { PartyRole::Member })
    }

    fn party_full(&self) -> Option<(bool, QueueParty)> {
        let p = self.party.as_ref().filter(|p| p.members.len() >= 2)?;
        Some((p.leader == p.me, QueueParty { members: p.members.iter().map(|m| m.id.clone()).collect() }))
    }

    /// The party's first member other than this player (PreMatch's OnPartyChanged @22509-23164: the loop over
    /// JoinedMembers skips the one whose AccountID is GetClientGUID); None alone or with no party.
    pub fn party_partner(&self) -> Option<&mvs_net::service::api::PartyMember> {
        let p = self.party.as_ref()?;
        p.members.iter().find(|m| m.id != p.me)
    }

    /// The fighter the party partner's own main menu shows (`party_partner`'s MenuFighter); None when its game sent
    /// none (an older one) or there is no partner.
    pub fn party_partner_fighter(&self) -> Option<&MenuFighter> {
        self.party_fighters.get(&self.party_partner()?.id)
    }

    fn party_other(&self) -> String {
        self.party
            .as_ref()
            .and_then(|p| p.members.iter().find(|m| m.id != p.me))
            .map(|m| m.name.clone())
            .unwrap_or_default()
    }

    // app/party-flow.ts: the leader's 'open' opens the member's queue screen on its modes; the rest goes to the search.
    fn party_say(&mut self, from: &str, say: &PartySay, now: f64) {
        let other = self.party_other();
        let mut out = Vec::new();
        let took = self.queue.as_mut().is_some_and(|q| q.party_say(say, &other, now, &mut out));
        self.q_out(out);
        if !took
            && let PartySay::Open { modes } = say
            && self.party.as_ref().is_some_and(|p| p.leader != p.me && p.leader == from)
            && self.queue.is_none()
        {
            self.open_queue = Some(modes.clone());
        }
        self.events.push(OnlineEvent::PartySay { from: from.to_owned(), say: say.clone() });
    }

    // ---------- the queue (app/queue.ts) ----------

    fn queue_open(&mut self, modes: Vec<QueueMode>, now: f64) {
        let party = self.party_full();
        // (a full party queues no 1v1: WBP_MatchOptions @1248-1457 turns its card off)
        let modes: Vec<QueueMode> =
            if party.is_some() { modes.into_iter().filter(|m| *m != QueueMode::OneVOne).collect() } else { modes };
        if self.queue.as_ref().is_some_and(|q| q.modes != modes) {
            self.end_queue();
        }
        if self.queue.is_none() {
            // (a game in a lobby leaves it first: the queue's session is not the lobby's; the fake queue's is the
            // fake host core)
            if self.fake.is_some() {
                self.close();
                self.queue_session = true;
            } else {
                self.open(SessionTarget::Queue, now);
            }
            let party = party.map(|(leader, list)| PartyQueue { leader, list });
            let mut q = QueueSession::new(modes.clone(), party, self.compat.clone());
            q.bot_after = self.bot_after;
            let mut out = Vec::new();
            q.opened(&mut out);
            self.queue = Some(q);
            self.q_out(out);
        }
        self.open_queue = Some(modes);
    }

    /// Out of the queue (endQueue).
    pub fn end_queue(&mut self) {
        let mut out = Vec::new();
        if let Some(mut q) = self.queue.take() {
            q.end(&mut out);
        }
        self.q_out(out);
        if self.queue_session {
            self.close();
        }
    }

    fn q_out(&mut self, out: Vec<QOut>) {
        for o in out {
            match o {
                QOut::Send(m) => self.send(&m),
                QOut::Say(s) => self.party_cmd(PartyCmd::Say(s)),
                QOut::Screen(e) => self.events.push(OnlineEvent::PreMatch(e)),
            }
        }
    }

    // A queue match's start: the program's match, and its set's end session.
    fn queue_started(&mut self) {
        let Some(m) = self.queue.as_mut().and_then(|q| q.started.take()) else { return };
        let QueueMatch { setup, perks, me, usernames, map, mode, seed, local } = m;
        self.queue = None;
        self.queue_end = Some(NetQueueEnd::new(mode, &mut self.hub));
        self.start(NetMatchStart {
            start: PreMatchStarted { seed, setup, slot: me, usernames, map, local, perks },
            queue: Some(mode),
        });
    }

    fn qe_news(&mut self, news: Vec<QeNews>) {
        let _ = news;
    }

    // ---------- the online pre-match (app/custom-pre-match.ts) ----------

    fn pre_match_cmd(&mut self, c: PreMatchCmd, loadout: &dyn Fn(&str) -> Option<NetLoadout>, now: f64) {
        // (a pm.path that came before the screen: it has it again)
        if c == PreMatchCmd::ScreenUp
            && let Some(p) = &self.path
        {
            self.events.push(OnlineEvent::Server(Box::new(p.clone())));
        }
        if self.queue.is_some() {
            return self.queue_cmd(c, loadout, now);
        }
        let Some(pm) = &mut self.hub.current else { return };
        let send = match c {
            PreMatchCmd::ScreenUp => {
                pm.attach();
                self.attached = true;
                None
            }
            PreMatchCmd::LocalPreview(p) => {
                let l = loadout(&p.character);
                pm.preview(p, l, now);
                None
            }
            PreMatchCmd::LocalLocked(p) => {
                let l = loadout(&p.character);
                pm.lock(p, l)
            }
            PreMatchCmd::PerksLocked(perks) => pm.perks(&perks),
            // the leader's Back to Lobby: the server ends it for the others (pm.end); the screen's own kicks: out
            PreMatchCmd::BackToLobby | PreMatchCmd::Kick => {
                self.attached = false;
                let m = pm.leave();
                pm.detach();
                m
            }
            _ => None,
        };
        if let Some(m) = send {
            self.send(&m);
        }
        self.deliver();
    }

    fn queue_cmd(&mut self, c: PreMatchCmd, loadout: &dyn Fn(&str) -> Option<NetLoadout>, now: f64) {
        let mut out = Vec::new();
        let Some(q) = &mut self.queue else { return };
        match c {
            PreMatchCmd::ScreenUp => q.screen_up(&mut out),
            PreMatchCmd::LocalLocked(p) => {
                q.loadout = loadout(&p.character);
                q.local_locked(p);
            }
            PreMatchCmd::LocalUnlocked => q.local_unlocked(&mut out),
            PreMatchCmd::StartMatchmaking => q.start_matchmaking(now, &mut out),
            PreMatchCmd::CancelMatchmaking => q.cancel_matchmaking(&mut out),
            PreMatchCmd::PerksLocked(perks) => QueueSession::perks_locked(&perks, &mut out),
            PreMatchCmd::BackToLobby | PreMatchCmd::Kick => {
                q.live = false;
                self.end_queue();
                return;
            }
            PreMatchCmd::LocalPreview(_) | PreMatchCmd::StartMatch(_) => {}
        }
        self.q_out(out);
    }

    // What the custom path's pre-match kept, to the screen that took it (custom-pre-match.ts's sink).
    fn deliver(&mut self) {
        if !self.attached {
            return;
        }
        let Some(pm) = &mut self.hub.current else { return };
        let ids: Vec<String> = pm
            .begin
            .players
            .iter()
            .enumerate()
            .map(|(i, p)| if p.id.is_empty() { format!("slot{i}") } else { p.id.clone() })
            .collect();
        let queue_kind = pm.begin.kind == PmKind::Queue;
        let begin_mode = pm.begin.mode;
        let id = |slot: usize| ids.get(slot).cloned().unwrap_or_default();
        let mut start = None;
        for k in pm.delivered() {
            let e = match k {
                Kept::Preview { slot, pick } => PreMatchEvent::Previewed { account: id(slot), pick },
                Kept::Locked { slot, pick, cosmetics } => {
                    if let Some(c) = cosmetics {
                        self.events
                            .push(OnlineEvent::PreMatch(PreMatchEvent::Cosmetics { account: id(slot), cosmetics: c }));
                    }
                    PreMatchEvent::Locked { account: id(slot), pick }
                }
                Kept::All { picks, map, cosmetics } => {
                    for (slot, c) in cosmetics.into_iter().enumerate() {
                        if let Some(c) = c {
                            self.events.push(OnlineEvent::PreMatch(PreMatchEvent::Cosmetics {
                                account: id(slot),
                                cosmetics: c,
                            }));
                        }
                    }
                    PreMatchEvent::PlayersReady { picks, map }
                }
                Kept::Ready { slot } => PreMatchEvent::ReadyPerks { account: id(slot) },
                Kept::Start(m) => {
                    let perks = m.perks.clone();
                    start = Some(*m);
                    PreMatchEvent::AllLockedPerks { perks }
                }
                // OnPlayerAbandonedPreMatch PM@121851: every remaining client to the main menu
                Kept::End { why, name } => PreMatchEvent::Kick(if !name.is_empty() {
                    KickNote::LeftNamed(name)
                } else if why == PmEndWhy::Error && queue_kind {
                    KickNote::Error
                } else {
                    KickNote::Left
                }),
            };
            self.events.push(OnlineEvent::PreMatch(e));
        }
        if let Some(s) = start {
            // (the rematch's queue, main.ts queueMatchMode: its pm.begin's, else the one of the set-up's team style)
            let queue = queue_kind.then(|| {
                begin_mode.unwrap_or(match s.setup.team_style {
                    1 => QueueMode::TwoVTwo,
                    2 => QueueMode::Ffa,
                    _ => QueueMode::OneVOne,
                })
            });
            if let Some(mode) = queue {
                // (the rematch's set goes on: a new end session for its next game)
                self.queue_end = Some(NetQueueEnd::new(mode, &mut self.hub));
            }
            self.start(NetMatchStart { start: s, queue });
        }
    }

    /// The pre-match screen is gone (its level opened, or the main menu): the custom path's session let go of it.
    ///
    /// The queue's screen (app/queue.ts runQueue's end) at the level it opened (`level`, None: none):
    /// - Wait in the Lab (BPI_TransitionToTraining, TrainingContainer) while searching or found: nothing touches the
    ///   queue, the session goes on (the Lab reads it, `lab_queue`);
    /// - another level with the match started: the match (the queue ends with it);
    /// - else back to the main menu: out of the queue.
    pub fn screen_done(&mut self, level: Option<&str>) {
        // (a started match the program did not take: its inbox closed, the lobby's chats the menus' again)
        if matches!(level, None | Some("New_MainMenu")) && self.started.is_some() {
            self.match_inbox(false);
        }
        if self.attached {
            self.attached = false;
            if let Some(pm) = &mut self.hub.current {
                pm.detach();
            }
        }
        let Some(q) = &mut self.queue else { return };
        q.live = false;
        let Some(level) = level else { return };
        if level == "TrainingContainer" && (q.searching || q.found.is_some()) {
            return;
        }
        if level != "New_MainMenu" && q.started.is_some() {
            q.searching = false;
            return;
        }
        self.end_queue();
    }

    /// The queue set a results screen shows (main.ts's queueEnd while open; None: not a queue match's end): its mode
    /// and team style.
    pub fn eog_queue(&self) -> Option<(QueueMode, u8)> {
        self.queue_end.as_ref().filter(|q| !q.closed()).map(|q| (q.mode, q.team_style))
    }

    /// The set's end session is let go (main.ts queue?.close(): the results screen's end, or a match left with none).
    pub fn close_queue_end(&mut self) {
        if let Some(q) = &mut self.queue_end {
            q.close();
        }
    }

    /// A queue set over, back to the main menu (main.ts mainMenu, app/queue.ts leaveQueueSession): with no queue
    /// kept the queue's session is left.
    pub fn leave_queue_session(&mut self) {
        if self.queue.is_none() && self.queue_session {
            self.close();
        }
    }

    /// The online pre-match the PreMatch level opens on (custom-pre-match.ts pendingPreMatch: a queue set's rematch,
    /// or an online lobby's start), while no screen has taken it.
    pub fn pending_pre_match(&self) -> Option<&super::pre_match::PreMatchBegin> {
        if self.attached {
            return None;
        }
        self.hub.current.as_ref().filter(|c| c.open()).map(|c| &c.begin)
    }

    /// The queue a Lab entered with Wait in the Lab runs beside (app.queue as app/match/lab.ts reads it): None
    /// when there is none.
    pub fn lab_queue(&self) -> Option<&QueueSession> {
        self.queue.as_ref().filter(|q| !q.live)
    }

    pub fn lab_queue_mut(&mut self) -> Option<&mut QueueSession> {
        self.queue.as_mut().filter(|q| !q.live)
    }

    /// A match left with no results (main.ts's return path): the Lab left for the PreMatch level while queued (its
    /// find chain or Back To Matchmaking) reopens the queue's screen on the search's start and lock (app/queue.ts
    /// runQueue's resume: its modes and the count-up's start, s before `now`); a queue left another way (the search
    /// ended meanwhile: the exit is Return Home) ends, its note the main menu's.
    pub fn lab_return(&mut self, now: f64) -> LabReturn {
        let Some(q) = &mut self.queue else { return LabReturn::None };
        if q.back_to_pre_match {
            q.back_to_pre_match = false;
            q.lab_leave = None;
            q.lab_found_once = false;
            self.open_queue = None;
            let resume = QueueResume { start: -((now - q.start_ms) / 1000.0), pick: q.mine.clone() };
            return LabReturn::PreMatch { modes: q.modes.clone(), resume };
        }
        let note = q.ended.take();
        self.end_queue();
        note.map_or(LabReturn::None, LabReturn::Note)
    }
}

/// What a match left with no results does to the queue (`OnlineHub::lab_return`).
#[derive(Debug, PartialEq)]
pub enum LabReturn {
    /// No queue, or out of it with no note.
    None,
    /// Out of the queue: the main menu's note.
    Note(KickNote),
    /// The queue's pre-match screen again: its modes, the search's start (the screen's clock, s) and the lock.
    PreMatch { modes: Vec<QueueMode>, resume: QueueResume },
}

/// The wall clock in ms (the recent list's `at`).
fn wall_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::super::port::fake;
    use super::*;
    use mvs_net::service::api::{BackendOff, InviteKind, PartyMember, SessionOn, StatusWhy};

    fn msg(m: &ServerMessage) -> NetEvent {
        NetEvent::Msg(m.text())
    }

    fn prefs(invites: bool) -> Prefs {
        Prefs { name: "Me".into(), icon: "icon_a".into(), listed: true, invites }
    }

    fn hub() -> (OnlineHub, std::rc::Rc<std::cell::RefCell<fake::Wire>>) {
        let (p, w) = fake::port();
        (OnlineHub::new(p, None, "c1".into(), Tables::default(), 0.0), w)
    }

    fn sent(w: &std::rc::Rc<std::cell::RefCell<fake::Wire>>) -> Vec<String> {
        w.borrow()
            .posted
            .iter()
            .filter_map(|r| match r {
                NetRequest::Msg(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_session_greets_with_the_name_and_publishes_presence() {
        let (mut h, w) = hub();
        w.borrow_mut().events = vec![NetEvent::Status { online: true, me: "p1".into(), why: StatusWhy::Online }];
        let v = h.frame(prefs(true), 0.0);
        assert!(v.connected && v.me == "p1");
        assert!(w.borrow().posted.iter().any(|r| matches!(r, NetRequest::Presence(p) if p.name == "Me" && !p.busy)));
        h.run(OnlineCmd::Open(SessionTarget::Host), &|_| None, 0.0);
        w.borrow_mut().events = vec![
            NetEvent::Opened { code: "r-1".into(), me: "p1".into(), chosen: true, on: SessionOn::Relay },
            msg(&ServerMessage::Server { proto: PROTOCOL, name: "core".into() }),
        ];
        let v = h.frame(prefs(true), 10.0);
        assert_eq!(v.events, [OnlineEvent::Opened]);
        assert!(v.session.as_ref().is_some_and(|s| s.host && s.code == "r-1"));
        assert!(sent(&w).iter().any(|t| t.contains(r#""t":"hello""#) && t.contains(r#""name":"Me""#)));
        w.borrow_mut().events = vec![msg(&ServerMessage::NameTaken { name: "Me".into() })];
        let v = h.frame(prefs(true), 20.0);
        assert!(v.name_taken && !v.name_refused && v.events[0] == OnlineEvent::NameTaken { refused: false });
    }

    /// (the port's own) The lobby's link from each way: the relay, Epic, LAN as they are; a local lobby's reason
    /// from the relay's version or the menus' line (the gate's reasons included), and "made while offline" once the
    /// line is online (D1); a lobby made local while online (the relay cannot host it) says that, never "made while
    /// offline".
    #[test]
    fn the_link_of_each_way_and_each_local_reason() {
        let online = OnlineLine::Online;
        assert_eq!(link_of(&SessionOn::Relay, &online, true), SessionLink::Relay);
        assert_eq!(link_of(&SessionOn::Epic, &online, true), SessionLink::Epic);
        let lan = SessionOn::Lan { at: "192.168.1.5:7777".into() };
        assert_eq!(link_of(&lan, &online, true), SessionLink::Lan { at: "192.168.1.5:7777".into() });
        let version = SessionOn::Local { why: LocalWhy::RelayVersion };
        assert_eq!(link_of(&version, &online, true), SessionLink::Local(LocalReason::RelayVersion));
        let cannot = SessionOn::Local { why: LocalWhy::Status { why: StatusWhy::SigningIn } };
        assert_eq!(link_of(&cannot, &online, true), SessionLink::Local(LocalReason::CannotHost));
        let cannot = SessionOn::Local { why: LocalWhy::Status { why: StatusWhy::Online } };
        assert_eq!(link_of(&cannot, &online, false), SessionLink::Local(LocalReason::CannotHost));
        let off = SessionOn::Local { why: LocalWhy::Status { why: StatusWhy::SigningIn } };
        for why in [
            OfflineWhy::SigningIn,
            OfflineWhy::TurnedOff,
            OfflineWhy::NotReached,
            OfflineWhy::Other,
            OfflineWhy::Checking,
            OfflineWhy::Update { version: "0.2.0".into() },
            OfflineWhy::Import,
            OfflineWhy::SelfCheck,
        ] {
            let line = OnlineLine::Offline(why.clone());
            assert_eq!(link_of(&off, &line, false), SessionLink::Local(LocalReason::Offline(why)));
        }
        // D1: online play came up under a local lobby: it stays local, its words say to make a new one
        assert_eq!(link_of(&off, &OnlineLine::Online, false), SessionLink::Local(LocalReason::MadeOffline));
        assert_eq!(link_of(&off, &OnlineLine::Slower, false), SessionLink::Local(LocalReason::MadeOffline));
    }

    /// (the port's own) The view's session says its link and the hub its ping; a local lobby has no ping, and the gate's
    /// reason is its reason while the gate holds.
    #[test]
    fn the_view_says_the_link_and_the_ping() {
        let (mut h, w) = hub();
        let off = StatusWhy::Off { backends: Vec::new() };
        w.borrow_mut().events = vec![NetEvent::Status { online: false, me: String::new(), why: off.clone() }];
        h.frame(prefs(true), 0.0);
        h.run(OnlineCmd::Open(SessionTarget::Host), &|_| None, 0.0);
        let on = SessionOn::Local { why: LocalWhy::Status { why: off } };
        w.borrow_mut().events = vec![
            NetEvent::Opened { code: String::new(), me: "local".into(), chosen: true, on },
            msg(&ServerMessage::Server { proto: PROTOCOL, name: "core".into() }),
        ];
        let v = h.frame(prefs(true), 10.0);
        let s = v.session.expect("open");
        assert_eq!(s.link, Some(SessionLink::Local(LocalReason::Offline(OfflineWhy::TurnedOff))));
        let pong = msg(&ServerMessage::Pong { n: serde_json::json!(1) });
        w.borrow_mut().events = vec![pong];
        h.frame(prefs(true), 30.0);
        assert_eq!(h.ping_ms(), None, "a local lobby has no ping");
        h.gate = Some(Gate::Update { version: "0.2.0".into() });
        let v = h.frame(prefs(true), 40.0);
        let why = OfflineWhy::Update { version: "0.2.0".into() };
        assert_eq!(v.session.expect("open").link, Some(SessionLink::Local(LocalReason::Offline(why))));
        h.gate = None;
        w.borrow_mut().events = vec![NetEvent::Status { online: true, me: "p1".into(), why: StatusWhy::Online }];
        let v = h.frame(prefs(true), 50.0);
        assert_eq!(v.session.expect("open").link, Some(SessionLink::Local(LocalReason::MadeOffline)));

        // the relay's: its ping once measured
        let (mut h, w) = hub();
        w.borrow_mut().events = vec![NetEvent::Status { online: true, me: "p1".into(), why: StatusWhy::Online }];
        h.frame(prefs(true), 0.0);
        h.run(OnlineCmd::Open(SessionTarget::Host), &|_| None, 0.0);
        w.borrow_mut().events = vec![
            NetEvent::Opened { code: "r-1".into(), me: "p1".into(), chosen: true, on: SessionOn::Relay },
            msg(&ServerMessage::Server { proto: PROTOCOL, name: "core".into() }),
        ];
        let v = h.frame(prefs(true), 10.0);
        let s = v.session.expect("open");
        assert!(s.link == Some(SessionLink::Relay) && h.ping_ms().is_none());
        w.borrow_mut().events = vec![msg(&ServerMessage::Pong { n: serde_json::json!(1) })];
        h.frame(prefs(true), 52.4);
        assert_eq!(h.ping_ms(), Some(42.4));
        // the dev's override says another way
        h.dev_on = Some(SessionOn::Epic);
        assert_eq!(h.view().session.expect("open").link, Some(SessionLink::Epic));
        // the host of a lobby over Epic or LAN: its core is this game's, so no ping; a guest's stays
        assert!(h.own_core() && h.ping_ms().is_none());
        h.dev_on = Some(SessionOn::Lan { at: "192.168.1.5:7777".into() });
        assert!(h.own_core() && h.ping_ms().is_none());
        h.target = Some(SessionTarget::Join("d-1".into()));
        assert!(!h.own_core() && h.ping_ms() == Some(42.4));

        // opened local while online (a relay that cannot host here): its own reason, not "made while offline"
        let (mut h, w) = hub();
        w.borrow_mut().events = vec![NetEvent::Status { online: true, me: "p1".into(), why: StatusWhy::Online }];
        h.frame(prefs(true), 0.0);
        h.run(OnlineCmd::Open(SessionTarget::Host), &|_| None, 0.0);
        let on = SessionOn::Local { why: LocalWhy::Status { why: StatusWhy::SigningIn } };
        w.borrow_mut().events = vec![NetEvent::Opened { code: String::new(), me: "local".into(), chosen: false, on }];
        let v = h.frame(prefs(true), 10.0);
        assert_eq!(v.session.expect("open").link, Some(SessionLink::Local(LocalReason::CannotHost)));
    }

    /// (the port's own) A local lobby greets its core with the local player's name when no name is chosen (its core
    /// takes no lobby request before a hello); an online session still waits for a name.
    #[test]
    fn a_local_lobby_greets_with_no_name_chosen() {
        let unnamed = Prefs { name: String::new(), ..prefs(true) };
        for (on, greets) in [(SessionOn::Local { why: LocalWhy::RelayVersion }, true), (SessionOn::Relay, false)] {
            let (mut h, w) = hub();
            h.frame(unnamed.clone(), 0.0);
            h.run(OnlineCmd::Open(SessionTarget::Host), &|_| None, 0.0);
            w.borrow_mut().events = vec![
                NetEvent::Opened { code: String::new(), me: "local".into(), chosen: true, on },
                msg(&ServerMessage::Server { proto: PROTOCOL, name: "core".into() }),
            ];
            h.frame(unnamed.clone(), 10.0);
            let hello = sent(&w).iter().any(|t| t.contains(r#""t":"hello""#) && t.contains(r#""name":"Player""#));
            assert_eq!(hello, greets);
        }
    }

    /// (the port's own) Another protocol on a lobby a player's game hosts names that game; the relay's, the server.
    #[test]
    fn a_mismatch_names_the_host_or_the_server() {
        for (on, notice) in [
            (SessionOn::Relay, OnlineNotice::ProtocolDiffers),
            (SessionOn::Epic, OnlineNotice::HostProtocolDiffers),
            (SessionOn::Lan { at: "a:1".into() }, OnlineNotice::HostProtocolDiffers),
        ] {
            let (mut h, w) = hub();
            w.borrow_mut().events = vec![NetEvent::Status { online: true, me: "p1".into(), why: StatusWhy::Online }];
            h.frame(prefs(true), 0.0);
            h.run(OnlineCmd::Open(SessionTarget::Join("d-1".into())), &|_| None, 0.0);
            w.borrow_mut().events = vec![
                NetEvent::Opened { code: "d-1".into(), me: "p1".into(), chosen: false, on },
                msg(&ServerMessage::Server { proto: PROTOCOL + 1, name: "core".into() }),
            ];
            let v = h.frame(prefs(true), 10.0);
            assert!(v.events.contains(&OnlineEvent::Notice(notice.clone())), "{notice:?}: {:?}", v.events);
        }
    }

    /// (the port's own) The match's path is kept past the match's go (the ping line of a match this game referees),
    /// and dropped at the next pre-match.
    #[test]
    fn the_match_path_outlives_the_go() {
        let (mut h, w) = hub();
        w.borrow_mut().events = vec![NetEvent::Status { online: true, me: "p1".into(), why: StatusWhy::Online }];
        h.frame(prefs(true), 0.0);
        h.run(OnlineCmd::Open(SessionTarget::Host), &|_| None, 0.0);
        let path = ServerMessage::PmPath { referee: 0, ms: vec![Some(0), Some(64)], far: vec![false, false] };
        let go = ServerMessage::MatchGo { in_ms: 100, peers: vec!["p1".into(), "p2".into()], referee: "p1".into() };
        w.borrow_mut().events = vec![
            NetEvent::Opened { code: "r-1".into(), me: "p1".into(), chosen: true, on: SessionOn::Relay },
            msg(&ServerMessage::Server { proto: PROTOCOL, name: "core".into() }),
            msg(&path),
            msg(&go),
        ];
        h.frame(prefs(true), 10.0);
        assert_eq!(h.match_path(), Some(&(0, vec![Some(0), Some(64)])));
    }

    #[test]
    fn a_toast_is_taken_alone_while_the_other_events_wait_for_the_menus() {
        let (mut h, w) = hub();
        w.borrow_mut().events = vec![NetEvent::Status { online: true, me: "p1".into(), why: StatusWhy::Online }];
        h.frame(prefs(true), 0.0);
        h.run(OnlineCmd::Open(SessionTarget::Host), &|_| None, 0.0);
        w.borrow_mut().events = vec![
            NetEvent::Opened { code: "r-1".into(), me: "p1".into(), chosen: true, on: SessionOn::Relay },
            msg(&ServerMessage::Server { proto: PROTOCOL, name: "core".into() }),
        ];
        h.frame(prefs(true), 10.0);
        // (a results screen's frame: the hub polled, its toasts taken)
        w.borrow_mut().events = vec![
            msg(&ServerMessage::Toast { from: "p2".into(), name: "Two".into() }),
            msg(&ServerMessage::NameTaken { name: "Me".into() }),
        ];
        h.poll(20.0);
        assert_eq!(h.take_toasts(), [OnlineEvent::Toast { from: "p2".into(), name: "Two".into() }]);
        assert!(h.take_toasts().is_empty());
        let v = h.frame(prefs(true), 30.0);
        assert_eq!(v.events[0], OnlineEvent::NameTaken { refused: false });
        assert!(!v.events.iter().any(|e| matches!(e, OnlineEvent::Toast { .. })));
        assert_eq!(h.player("p2").map(|p| p.name), Some("Two".into()));
    }

    #[test]
    fn the_presence_carries_the_status_menus_lobby_match_and_back() {
        let (mut h, w) = hub();
        let last = |w: &std::rc::Rc<std::cell::RefCell<fake::Wire>>| {
            w.borrow().posted.iter().rev().find_map(|r| match r {
                NetRequest::Presence(p) => Some((p.status, p.busy)),
                _ => None,
            })
        };
        w.borrow_mut().events = vec![NetEvent::Status { online: true, me: "a".into(), why: StatusWhy::Online }];
        h.frame(prefs(true), 0.0);
        assert_eq!(last(&w), Some((Some(PresenceStatus::Menus), false)));
        h.run(OnlineCmd::Open(SessionTarget::Host), &|_| None, 0.0);
        w.borrow_mut().events = vec![
            NetEvent::Opened { code: "r-1".into(), me: "a".into(), chosen: true, on: SessionOn::Relay },
            msg(&ServerMessage::Server { proto: PROTOCOL, name: "core".into() }),
        ];
        h.frame(prefs(true), 10.0);
        assert_eq!(last(&w), Some((Some(PresenceStatus::Lobby), false)));
        let begin = serde_json::json!({ "t": "pm.begin", "id": "pm1", "kind": "lobby", "mode": null, "teamStyle": 2,
            "map": "", "me": 0, "players": [
                { "id": "a", "name": "Ann", "team": 0, "character": "", "skin": "", "bot": false },
                { "id": "b", "name": "Bob", "team": 1, "character": "", "skin": "", "bot": false },
                { "id": "c", "name": "Cy", "team": 2, "character": "", "skin": "", "bot": false },
            ] });
        w.borrow_mut().events = vec![NetEvent::Msg(begin.to_string())];
        h.frame(prefs(true), 20.0);
        assert_eq!(last(&w), Some((Some(PresenceStatus::Ffa), true)));
        w.borrow_mut().events = vec![msg(&ServerMessage::PmEnd { why: PmEndWhy::Left, name: String::new() })];
        h.frame(prefs(true), 30.0);
        assert_eq!(last(&w), Some((Some(PresenceStatus::Lobby), false)));
        h.run(OnlineCmd::Close, &|_| None, 40.0);
        h.frame(prefs(true), 50.0);
        assert_eq!(last(&w), Some((Some(PresenceStatus::Menus), false)));
    }

    #[test]
    fn a_match_status_follows_the_queue_mode_else_the_lobby_shape() {
        use PresenceStatus::*;
        assert_eq!(match_status(Some(QueueMode::OneVOne), 1, 4), OneVOne);
        assert_eq!(match_status(Some(QueueMode::Silly), 0, 2), TwoVTwo);
        assert_eq!(match_status(Some(QueueMode::Ffa), 0, 0), Ffa);
        assert_eq!(match_status(None, 0, 2), OneVOne);
        assert_eq!(match_status(None, 1, 4), TwoVTwo);
        assert_eq!(match_status(None, 2, 3), Ffa);
        // a lobby of another shape (three on teams, a lone fighter with bots) names no mode
        assert_eq!(match_status(None, 1, 3), Match);
        assert_eq!(match_status(None, 0, 3), Match);
    }

    #[test]
    fn a_rename_answered_with_the_kept_name_is_refused_at_once() {
        let (mut h, w) = hub();
        w.borrow_mut().events = vec![NetEvent::Status { online: true, me: "p1".into(), why: StatusWhy::Online }];
        h.frame(prefs(true), 0.0);
        h.run(OnlineCmd::Open(SessionTarget::Host), &|_| None, 0.0);
        w.borrow_mut().events = vec![
            NetEvent::Opened { code: "r-1".into(), me: "p1".into(), chosen: true, on: SessionOn::Relay },
            msg(&ServerMessage::Server { proto: PROTOCOL, name: "core".into() }),
        ];
        h.frame(prefs(true), 10.0);
        w.borrow_mut().events = vec![msg(&ServerMessage::Welcome { id: "p1".into(), name: "Me".into() })];
        h.frame(prefs(true), 20.0);
        // a rename the core takes: saved
        h.run(OnlineCmd::SetName("A  B".into()), &|_| None, 30.0);
        w.borrow_mut().events = vec![msg(&ServerMessage::Welcome { id: "p1".into(), name: "A B".into() })];
        let v = h.frame(prefs(true), 40.0);
        let taken = |v: &OnlineView| {
            v.events.iter().filter(|e| matches!(e, OnlineEvent::NameTaken { .. })).cloned().collect::<Vec<_>>()
        };
        assert!(taken(&v).is_empty(), "{:?}", v.events);
        assert_eq!(h.saved_name.take().as_deref(), Some("A  B"));
        // a second within the core's 2 s: welcome with the kept name, refused as taken without the 4 s wait
        h.run(OnlineCmd::SetName("Cee".into()), &|_| None, 50.0);
        w.borrow_mut().events = vec![msg(&ServerMessage::Welcome { id: "p1".into(), name: "A B".into() })];
        let v = h.frame(prefs(true), 60.0);
        assert_eq!(taken(&v), [OnlineEvent::NameTaken { refused: false }]);
        assert!(h.saved_name.is_none() && !v.name_taken);
    }

    #[test]
    fn hello_sends_the_bare_version_and_a_close_ends_the_open_pre_match() {
        let (mut h, w) = hub();
        h.run(OnlineCmd::Open(SessionTarget::Host), &|_| None, 0.0);
        w.borrow_mut().events = vec![
            NetEvent::Opened { code: "r-1".into(), me: "a".into(), chosen: true, on: SessionOn::Relay },
            msg(&ServerMessage::Server { proto: PROTOCOL, name: "core".into() }),
        ];
        h.frame(prefs(true), 10.0);
        let app = format!(r#""app":"{}""#, mvs_release::VERSION);
        assert!(sent(&w).iter().any(|t| t.contains(r#""t":"hello""#) && t.contains(&app)));
        let begin = serde_json::json!({ "t": "pm.begin", "id": "pm1", "kind": "lobby", "mode": null, "teamStyle": 0,
            "map": "", "me": 0, "players": [
                { "id": "a", "name": "Ann", "team": 0, "character": "", "skin": "", "bot": false },
                { "id": "b", "name": "Bob", "team": 1, "character": "", "skin": "", "bot": false },
            ] });
        w.borrow_mut().events = vec![NetEvent::Msg(begin.to_string())];
        h.frame(prefs(true), 20.0);
        assert!(h.pending_pre_match().is_some());
        // (online.ts onClose: this game's own leave ends the pre-match as a lost connection)
        h.run(OnlineCmd::Close, &|_| None, 30.0);
        assert!(h.pending_pre_match().is_none());
        assert!(h.hub.current.as_ref().is_some_and(|c| c.ended == Some((PmEndWhy::Error, String::new()))));
    }

    #[test]
    fn a_pm_path_is_handed_again_to_a_screen_that_comes_up_after_it() {
        let (mut h, w) = hub();
        h.run(OnlineCmd::Open(SessionTarget::Host), &|_| None, 0.0);
        let begin = serde_json::json!({ "t": "pm.begin", "id": "pm1", "kind": "lobby", "mode": null, "teamStyle": 0,
            "map": "", "me": 0, "players": [
                { "id": "a", "name": "Ann", "team": 0, "character": "", "skin": "", "bot": false },
                { "id": "b", "name": "Bob", "team": 1, "character": "", "skin": "", "bot": false },
            ] });
        let begin: ServerMessage = serde_json::from_value(begin).unwrap();
        let path = ServerMessage::PmPath { referee: 1, ms: vec![Some(120), Some(0)], far: vec![false, false] };
        w.borrow_mut().events = vec![
            NetEvent::Opened { code: "r-1".into(), me: "a".into(), chosen: true, on: SessionOn::Relay },
            msg(&ServerMessage::Server { proto: PROTOCOL, name: "core".into() }),
            msg(&begin),
            msg(&path),
        ];
        let v = h.frame(prefs(true), 10.0);
        let paths = |v: &OnlineView| {
            v.events
                .iter()
                .filter(|e| matches!(e, OnlineEvent::Server(m) if matches!(**m, ServerMessage::PmPath { .. })))
                .count()
        };
        assert_eq!(paths(&v), 1, "as it comes");
        h.run(OnlineCmd::PreMatch(PreMatchCmd::ScreenUp), &|_| None, 20.0);
        let v = h.frame(prefs(true), 30.0);
        assert!(v.events.contains(&OnlineEvent::Server(Box::new(path.clone()))), "again at the screen's up");
        // a new pre-match drops it: the next screen has none
        w.borrow_mut().events = vec![msg(&begin)];
        h.frame(prefs(true), 40.0);
        h.run(OnlineCmd::PreMatch(PreMatchCmd::ScreenUp), &|_| None, 50.0);
        assert_eq!(paths(&h.frame(prefs(true), 60.0)), 0);
        // a found queue match ended before it began (queue.state off) drops it too
        let off = ServerMessage::QueueState { on: false, why: None, name: None };
        w.borrow_mut().events = vec![msg(&path), msg(&off)];
        h.frame(prefs(true), 70.0);
        h.run(OnlineCmd::PreMatch(PreMatchCmd::ScreenUp), &|_| None, 80.0);
        assert_eq!(paths(&h.frame(prefs(true), 90.0)), 0);
    }

    /// "Party Disbanded" goes to the one a full party ended under, not to the one who left it.
    #[test]
    fn a_party_ended_by_the_other_member_is_told_and_a_leave_is_not() {
        let member = |id: &str| PartyMember { id: id.into(), name: id.into(), icon: String::new() };
        let party = |n: usize| {
            let members = [member("p1"), member("p2")].into_iter().take(n).collect();
            Some(PartyView { leader: "p1".into(), members, me: "p1".into() })
        };
        let disbanded = |v: &OnlineView| v.events.contains(&OnlineEvent::Notice(OnlineNotice::PartyDisbanded));
        let (mut h, w) = hub();
        // (the member leaves: the leader's party drops to one)
        w.borrow_mut().events = vec![NetEvent::Party { party: party(2), why: ClosedWhy::Left }];
        assert!(!disbanded(&h.frame(prefs(true), 0.0)));
        w.borrow_mut().events = vec![NetEvent::Party { party: party(1), why: ClosedWhy::Left }];
        assert!(disbanded(&h.frame(prefs(true), 1.0)));
        // (the member's side: its leader left)
        w.borrow_mut().events = vec![NetEvent::Party { party: party(2), why: ClosedWhy::Left }];
        h.frame(prefs(true), 2.0);
        w.borrow_mut().events = vec![NetEvent::Party { party: None, why: ClosedWhy::LeaderLeft }];
        assert!(disbanded(&h.frame(prefs(true), 3.0)));
        // (this player leaves: no note, and the next party's end is told again)
        w.borrow_mut().events = vec![NetEvent::Party { party: party(2), why: ClosedWhy::Left }];
        h.frame(prefs(true), 4.0);
        h.run(OnlineCmd::Party(PartyCmd::Leave), &|_| None, 4.0);
        assert!(w.borrow().posted.iter().any(|r| matches!(r, NetRequest::PartyLeave)));
        w.borrow_mut().events = vec![NetEvent::Party { party: None, why: ClosedWhy::Left }];
        assert!(!disbanded(&h.frame(prefs(true), 5.0)));
        w.borrow_mut().events = vec![NetEvent::Party { party: party(2), why: ClosedWhy::Left }];
        h.frame(prefs(true), 6.0);
        w.borrow_mut().events = vec![NetEvent::Party { party: party(1), why: ClosedWhy::Left }];
        assert!(disbanded(&h.frame(prefs(true), 7.0)));
    }

    /// The main-menu fighter goes to the service once per change; the partner's is kept from its PartyFighter until
    /// it leaves the party (none told: none, the placeholder's case).
    #[test]
    fn the_main_menu_fighters_go_and_come_with_the_party() {
        let member = |id: &str| PartyMember { id: id.into(), name: id.into(), icon: String::new() };
        let party = |n: usize| {
            let members = [member("p1"), member("p2")].into_iter().take(n).collect();
            Some(PartyView { leader: "p1".into(), members, me: "p1".into() })
        };
        let fighters = |w: &std::rc::Rc<std::cell::RefCell<fake::Wire>>| {
            w.borrow().posted.iter().filter(|r| matches!(r, NetRequest::MenuFighter(_))).count()
        };
        let ww = MenuFighter { key: "wonderwoman".into(), skin: String::new() };
        let (mut h, w) = hub();
        h.set_menu_fighter(Some(ww.clone()));
        h.frame(prefs(true), 0.0);
        h.frame(prefs(true), 1.0);
        assert_eq!(fighters(&w), 1);
        assert!(w.borrow().posted.iter().any(|r| matches!(r, NetRequest::MenuFighter(Some(f)) if *f == ww)));

        w.borrow_mut().events = vec![NetEvent::Party { party: party(2), why: ClosedWhy::Left }];
        h.frame(prefs(true), 2.0);
        assert_eq!(h.party_partner_fighter(), None);
        let bat = MenuFighter { key: "batman".into(), skin: "batman_S01".into() };
        w.borrow_mut().events = vec![NetEvent::PartyFighter { from: "p2".into(), fighter: Some(bat.clone()) }];
        h.frame(prefs(true), 3.0);
        assert_eq!(h.party_partner_fighter(), Some(&bat));
        w.borrow_mut().events = vec![NetEvent::Party { party: party(1), why: ClosedWhy::Left }];
        h.frame(prefs(true), 4.0);
        w.borrow_mut().events = vec![NetEvent::Party { party: party(2), why: ClosedWhy::Left }];
        h.frame(prefs(true), 5.0);
        assert_eq!(h.party_partner_fighter(), None);
    }

    #[test]
    fn a_failed_open_answers_closed_and_invites_off_hides_invites() {
        let (mut h, w) = hub();
        h.run(OnlineCmd::Open(SessionTarget::Join("r-9".into())), &|_| None, 0.0);
        w.borrow_mut().events = vec![msg(&ServerMessage::Server { proto: PROTOCOL + 1, name: "x".into() })];
        let v = h.frame(prefs(true), 1.0);
        assert_eq!(
            v.events,
            [
                OnlineEvent::Notice(OnlineNotice::ProtocolDiffers),
                OnlineEvent::Closed(ClosedWhy::Other { text: String::new() })
            ]
        );
        assert!(v.session.is_none());
        let invite = || NetEvent::Invited {
            kind: InviteKind::Custom,
            code: "r-2".into(),
            from_id: "p2".into(),
            from_name: "Two".into(),
        };
        w.borrow_mut().events = vec![invite()];
        assert!(h.frame(prefs(false), 2.0).events.is_empty());
        w.borrow_mut().events = vec![invite()];
        assert!(matches!(&h.frame(prefs(true), 3.0).events[0], OnlineEvent::Invited(i) if i.from_name == "Two"));
        // (the sender is known by id afterwards: player(id)'s fallback)
        assert_eq!(h.player("p2").map(|p| p.name), Some("Two".into()));
    }

    #[test]
    fn host_notices_are_cleaned_and_paced() {
        assert_eq!(clean_notice("  a\u{0007}\n\u{200b}b   c "), "a b c");
        assert_eq!(clean_notice(&"x".repeat(900)).len(), NOTICE_LIMIT);
        let (mut h, w) = hub();
        h.run(OnlineCmd::Open(SessionTarget::Host), &|_| None, 0.0);
        w.borrow_mut().events = vec![
            NetEvent::Opened { code: "r-1".into(), me: "p1".into(), chosen: true, on: SessionOn::Relay },
            msg(&ServerMessage::Server { proto: PROTOCOL, name: "core".into() }),
            msg(&ServerMessage::Notice { text: "hi".into() }),
            msg(&ServerMessage::Notice { text: "again".into() }),
        ];
        let v = h.frame(prefs(true), 0.0);
        let notes: Vec<_> = v.events.iter().filter(|e| matches!(e, OnlineEvent::Notice(_))).collect();
        assert_eq!(notes, [&OnlineEvent::Notice(OnlineNotice::Host("hi".into()))]);
        w.borrow_mut().events = vec![msg(&ServerMessage::Notice { text: "hi".into() })];
        assert!(h.frame(prefs(true), NOTICE_EVERY_MS + 1.0).events.is_empty());
        w.borrow_mut().events = vec![msg(&ServerMessage::Notice { text: "hi".into() })];
        assert_eq!(h.frame(prefs(true), NOTICE_AGAIN_MS + 1.0).events.len(), 1);
    }

    /// A hub on the fake host core, its queue opened on 1v1 with a lock and its search started.
    fn fake_queue() -> OnlineHub {
        use super::super::fake_queue::{FakeCore, FakeOptions};
        let (mut h, _w) = hub();
        h.fake = Some(FakeCore::new(FakeOptions { start_s: 1.0, vote_no: false, path: None }));
        h.bot_after = Some(1.0);
        h.run(OnlineCmd::Queue { modes: vec![QueueMode::OneVOne] }, &|_| None, 0.0);
        // (the fake core is the queue's session: no service session is opened)
        assert!(h.queue_session && h.client.is_none());
        let pick = mvs_ui::menu::online::NetPick { character: "shaggy".into(), skin: String::new() };
        h.run(OnlineCmd::PreMatch(PreMatchCmd::LocalLocked(pick)), &|_| None, 0.0);
        h.run(OnlineCmd::PreMatch(PreMatchCmd::StartMatchmaking), &|_| None, 0.0);
        h
    }

    fn podium() -> Vec<mvs_sim::world::flow::PodiumEntry> {
        (0..2)
            .map(|i| mvs_sim::world::flow::PodiumEntry {
                player_index: i,
                team_index: i,
                ring_outs: 1 - i,
                damage_dealt: 50.0,
                falls: i,
                score: mvs_sim::math::narrow(f64::from(1 - i)),
                is_winner: i == 0,
                assists: 0,
                off_platform_ring_outs: 0,
                assisted: 0,
                doubles: 0,
                dodges: 0,
            })
            .collect()
    }

    #[test]
    fn the_fake_queue_finds_and_starts_and_the_local_match_sends_its_result() {
        let mut h = fake_queue();
        let mut found = None;
        for k in 1..=40 {
            h.poll(f64::from(k) * 250.0);
            found = found.or_else(|| h.queue.as_ref().and_then(|q| q.found.clone()));
        }
        let found = found.expect("a found match");
        assert_eq!((found.match_id.as_str(), found.players.len()), ("fake", 2));
        // (match.start: the queue's match handed to the program, the set's end session begun)
        assert!(h.queue.is_none() && h.queue_end.is_some() && h.started.is_some());
        // (the local match's end: its result through the set's end, then match.leave)
        h.local = Some(LocalMatch { queue: true, map: found.map.clone() });
        h.local_over(Some(&podium()));
        let f = h.fake.as_ref().expect("the fake core");
        assert_eq!(f.sent, ["match.result", "match.leave"]);
        assert_eq!(f.reports.len(), 1);
        assert_eq!(f.reports[0].0, 0);
        assert!(h.local.is_none());
        // (a second end sends nothing)
        h.local_over(None);
        assert_eq!(h.fake.as_ref().expect("the fake core").sent.len(), 2);
    }

    #[test]
    fn a_local_match_left_sends_only_the_leave() {
        let mut h = fake_queue();
        h.local = Some(LocalMatch { queue: false, map: String::new() });
        h.local_over(None);
        assert_eq!(h.fake.as_ref().expect("the fake core").sent, ["match.leave"]);
    }

    #[test]
    fn wait_in_the_lab_keeps_the_queue_and_the_return_reopens_it() {
        let mut h = fake_queue();
        h.poll(500.0);
        assert!(h.queue.as_ref().is_some_and(|q| q.searching));
        h.queue.as_mut().expect("the queue").live = true;
        assert!(h.lab_queue().is_none());
        // (Wait in the Lab: the screen's level is the Lab's, the search goes on beside it)
        h.screen_done(Some("TrainingContainer"));
        let q = h.lab_queue().expect("the Lab's queue");
        assert!(q.searching && q.mine.as_ref().is_some_and(|m| m.character == "shaggy"));
        let start = q.start_ms;
        // (the find chain's end: the queue's pre-match again, on the search's start and the lock)
        let q = h.lab_queue_mut().expect("the Lab's queue");
        q.back_to_pre_match = true;
        q.lab_found_once = true;
        let LabReturn::PreMatch { modes, resume } = h.lab_return(start + 5000.0) else { panic!("no return") };
        assert_eq!(modes, [QueueMode::OneVOne]);
        assert!((resume.start + 5.0).abs() < 1e-9);
        assert_eq!(resume.pick.map(|p| p.character), Some("shaggy".to_owned()));
        let q = h.queue.as_ref().expect("the queue kept");
        assert!(!q.back_to_pre_match && !q.lab_found_once && q.lab_leave.is_none());
        // (the Lab left another way: out of the queue)
        assert_eq!(h.lab_return(start + 6000.0), LabReturn::None);
        assert!(h.queue.is_none());
    }

    /// The main menu is never up while queued, as in the build (the queue lives on the pre-match screen or in the
    /// Lab; its Back cancels, the Lab's exit returns to it): the queue and its session end there, so the main menu's
    /// CUSTOM GAMES and an invitation's Join reach no host core that holds this player queued (its QueuedSelf
    /// refusal stays a guard).
    #[test]
    fn the_main_menu_ends_the_queue_and_a_start_hands_it_on() {
        let mut h = fake_queue();
        h.poll(500.0);
        h.screen_done(Some("New_MainMenu"));
        assert!(h.queue.is_none() && !h.queue_session && !h.queued());
        let mut h = fake_queue();
        for k in 1..=40 {
            h.poll(f64::from(k) * 250.0);
        }
        h.screen_done(Some("M001"));
        assert!(h.queue.is_none() && h.started.is_some());
    }

    /// The line's reasons in their order: the gate, online (slower), signing in, turned off, not reached, other.
    #[test]
    fn the_line_names_the_first_reason() {
        use mvs_net::service::api::{BackendOff, EpicMissing};
        let off = |list: &[(BackendKind, OffWhy)]| NetStatus {
            online: false,
            me: String::new(),
            why: StatusWhy::Off {
                backends: list.iter().map(|(kind, why)| BackendOff { kind: *kind, why: why.clone() }).collect(),
            },
        };
        let on = NetStatus { online: true, me: "p1".into(), why: StatusWhy::Online };
        let signing = NetStatus { online: false, me: String::new(), why: StatusWhy::SigningIn };
        let retrying = OffWhy::Retrying { text: "nothing answers".into(), in_ms: 2000 };
        let connecting = OffWhy::Connecting { address: "relay.example.org".into() };
        let (r, e, l) = (BackendKind::Relay, BackendKind::Epic, BackendKind::Lan);
        let o = |w| OnlineLine::Offline(w);
        let update = Gate::Update { version: "9.9.9".into() };
        // a gate first, whatever the sign-in
        assert_eq!(line_of(&on, Some(&Gate::Checking), false), o(OfflineWhy::Checking));
        assert_eq!(line_of(&on, Some(&update), true), o(OfflineWhy::Update { version: "9.9.9".into() }));
        assert_eq!(line_of(&signing, Some(&Gate::Import), false), o(OfflineWhy::Import));
        let self_check = Gate::SelfCheck { mine: 1, release: 2 };
        assert_eq!(line_of(&off(&[(r, retrying.clone())]), Some(&self_check), false), o(OfflineWhy::SelfCheck));
        assert_eq!(line_of(&on, None, false), OnlineLine::Online);
        assert_eq!(line_of(&on, None, true), OnlineLine::Slower);
        assert_eq!(line_of(&signing, None, false), o(OfflineWhy::SigningIn));
        // one backend signing in: signing in, though another failed
        let s = off(&[(r, retrying.clone()), (e, OffWhy::SigningIn), (l, OffWhy::Off)]);
        assert_eq!(line_of(&s, None, false), o(OfflineWhy::SigningIn));
        let s = off(&[(r, OffWhy::NotOn), (e, OffWhy::NotOn), (l, OffWhy::Off)]);
        assert_eq!(line_of(&s, None, false), o(OfflineWhy::TurnedOff));
        let s = off(&[(r, OffWhy::TurnedOff), (e, OffWhy::NotOn), (l, OffWhy::Off)]);
        assert_eq!(line_of(&s, None, false), o(OfflineWhy::TurnedOff));
        let s = off(&[(r, retrying.clone()), (e, OffWhy::Missing { why: EpicMissing::NotSetUp }), (l, OffWhy::Off)]);
        assert_eq!(line_of(&s, None, false), o(OfflineWhy::NotReached));
        let s = off(&[(r, OffWhy::NotOn), (e, OffWhy::Missing { why: EpicMissing::NotSetUp }), (l, OffWhy::Off)]);
        assert_eq!(line_of(&s, None, false), o(OfflineWhy::Other));
        let s = off(&[(r, OffWhy::Backend { text: "taken elsewhere".into() }), (e, OffWhy::NotOn)]);
        assert_eq!(line_of(&s, None, false), o(OfflineWhy::Other));
        // the slower way does not hide a reason to be offline
        assert_eq!(line_of(&off(&[(r, retrying.clone())]), None, true), o(OfflineWhy::NotReached));
        // the hub: the way from its port; not reached kept through the relay's next try, dropped once it is in
        let (mut h, w) = hub();
        w.borrow_mut().events = vec![
            NetEvent::Status { online: true, me: "p1".into(), why: StatusWhy::Online },
            NetEvent::Way { slow: true },
        ];
        assert_eq!(h.frame(prefs(true), 0.0).line, OnlineLine::Slower);
        let status = |list: &[(BackendKind, OffWhy)]| {
            let s = off(list);
            NetEvent::Status { online: false, me: String::new(), why: s.why }
        };
        w.borrow_mut().events =
            vec![NetEvent::Way { slow: false }, status(&[(r, retrying.clone()), (e, OffWhy::NotOn)])];
        assert_eq!(h.frame(prefs(true), 10.0).line, o(OfflineWhy::NotReached));
        w.borrow_mut().events = vec![status(&[(r, connecting.clone()), (e, OffWhy::NotOn)])];
        assert_eq!(h.frame(prefs(true), 20.0).line, o(OfflineWhy::NotReached));
        w.borrow_mut().events = vec![NetEvent::Status { online: true, me: "p1".into(), why: StatusWhy::Online }];
        assert_eq!(h.frame(prefs(true), 30.0).line, OnlineLine::Online);
        w.borrow_mut().events = vec![status(&[(r, connecting.clone()), (e, OffWhy::NotOn)])];
        assert_eq!(h.frame(prefs(true), 40.0).line, o(OfflineWhy::SigningIn));
        // a relay that proved another key: another reason, kept through its next try
        let other_key = OffWhy::RelayKey { release: true, in_ms: 2000 };
        w.borrow_mut().events = vec![status(&[(r, other_key), (e, OffWhy::NotOn)])];
        assert_eq!(h.frame(prefs(true), 42.0).line, o(OfflineWhy::Other));
        w.borrow_mut().events = vec![status(&[(r, connecting), (e, OffWhy::NotOn)])];
        assert_eq!(h.frame(prefs(true), 44.0).line, o(OfflineWhy::Other));
        h.set_gate(Some(Gate::Checking));
        assert_eq!(h.frame(prefs(true), 50.0).line, o(OfflineWhy::Checking));
    }

    /// A join link's command waits through the relay's retries, at most PENDING_WAIT_MS from the first frame; at
    /// once when nothing is on its way in; always while the gate is set.
    #[test]
    fn pending_commands_wait_for_a_retrying_relay() {
        let (r, e) = (BackendKind::Relay, BackendKind::Epic);
        let retrying = OffWhy::Retrying { text: "nothing answers".into(), in_ms: 2000 };
        let status = |list: Vec<(BackendKind, OffWhy)>| NetEvent::Status {
            online: false,
            me: String::new(),
            why: StatusWhy::Off { backends: list.into_iter().map(|(kind, why)| BackendOff { kind, why }).collect() },
        };
        let (mut h, w) = hub();
        w.borrow_mut().events = vec![status(vec![(r, retrying.clone()), (e, OffWhy::NotOn)])];
        h.frame(prefs(true), 1000.0);
        assert!(h.pending_held(1000.0));
        assert!(h.pending_held(1000.0 + PENDING_WAIT_MS - 1.0));
        assert!(!h.pending_held(1000.0 + PENDING_WAIT_MS), "run to fail with the true reason");
        let (mut h, w) = hub();
        w.borrow_mut().events = vec![status(vec![(r, OffWhy::NotOn), (e, OffWhy::NotOn)])];
        h.frame(prefs(true), 0.0);
        assert!(!h.pending_held(0.0));
        h.set_gate(Some(Gate::Checking));
        assert!(h.pending_held(PENDING_WAIT_MS * 3.0));
    }
}
