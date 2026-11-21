//! The custom game's online lobby: a session of its own, hosted by this player's
//! net service or joined by its code. Every request is a message named after the native call (`lobby.<call>`); the
//! session's host core answers its members with the whole lobby and the event it stands for (`lobby.state`).
//!
//! There are no promises or listeners: the program's `OnlineView` of each frame (`feed`) and the commands the lobby
//! queues for the program (`take_cmds`) stand for them; the listeners' calls are `LobbyEvent`s the widget drains
//! (`take_events`).
//!
//! Not ported here: the match the lobby starts (NetLobby.matchSetup and `preMatch`, the pre-match session):
//! the program's online client owns the pre-match; the lobby only says a start began
//! (`LobbyEvent::MatchStarting`, `started_match`).

use indexmap::IndexMap;
use mvs_net::service::api::{ClosedWhy, SessionTarget};
use mvs_protocol::chat::{CHAT_LIMIT, ChatGate, clean_chat};
use mvs_protocol::lobby_rules::{LobbyOptions, LobbyPlayer, LobbyRules, OptionKey, OptionValue};
use mvs_protocol::messages::{ChatTo, ClientMessage, LobbyErrorWhy, OptionWire, ServerMessage};
use mvs_protocol::online::PmKind;
use serde_json::Value;

use super::{LobbyEvent, LobbyNote};
use crate::menu::custom_text;
use crate::menu::online::{OnlineCmd, OnlineEvent, OnlineView};

/// A lobby log's join / leave notes of one human: one per this long (ms) at the most (NOTE_AGAIN_MS).
const NOTE_AGAIN_MS: f64 = 10_000.0;
/// The lines a chat log keeps at the most.
pub const CHAT_KEPT: usize = 100;
/// What the page takes of the chat its host core passes on: each sender has RECEIVE_TIMES the
/// send budget, all senders together RECEIVE_ALL_TIMES it; the budgets of RECEIVE_SENDERS senders are kept.
const RECEIVE_TIMES: f64 = 2.0;
const RECEIVE_ALL_TIMES: f64 = 4.0;
const RECEIVE_SENDERS: usize = 8;

/// What a chat line is: a message in the lobby channel, or the lobby's note of a player who joined or left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatKind {
    Lobby,
    Joined,
    Left,
}

/// A line of a chat log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatLine {
    /// The sender's id.
    pub from: String,
    pub name: String,
    pub to: ChatKind,
    pub text: String,
    pub mine: bool,
}

/// The result of `NetLobby::say` (sendChat: the cleaned text as sent; '' nothing to send; null over the budget).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Say {
    /// Nothing to send (no session, or nothing left of the text).
    Nothing,
    Sent(String),
    /// Sent too fast.
    TooFast,
}

#[derive(Clone, Debug)]
struct Noted {
    here: bool,
    at: f64,
    name: String,
}

/// The session the lobby stands on once it opened.
#[derive(Clone, Debug, Default)]
struct Taken {
    me: String,
    code: String,
    /// On a backend of the sign-in (false: a direct code's join).
    chosen: bool,
}

/// The custom game's online lobby.
#[derive(Debug)]
pub struct NetLobby {
    /// The id of the last lobby.state (the lobby's).
    pub id: String,
    rules: LobbyRules,
    signed_in_as: String,
    join_code: Option<String>,
    /// A lobby joined by a code, kept after the open takes `join_code`.
    by_code: bool,
    compat: String,
    session: Option<Taken>,
    opening: bool,
    /// Closed for good (left, kicked, lost).
    gone: bool,
    attached: bool,
    /// The code the open in flight joins (None: it hosts).
    opening_join: Option<String>,
    /// The commands for the program, in order.
    out: Vec<OnlineCmd>,
    events: Vec<LobbyEvent>,
    /// The lobby's chat since this player came in, kept across its matches.
    pub chat: Vec<ChatLine>,
    noted: Option<IndexMap<String, Noted>>,
    /// The chat's budgets: each lobby has its own (the match's is mvs-app match_chat.rs ChatBudget), made at its
    /// first use.
    send_gate: Option<ChatGate>,
    received: IndexMap<String, ChatGate>,
    received_all: Option<ChatGate>,
    /// A pm.begin of kind lobby came: the match is starting (the program runs its pre-match).
    pub started_match: bool,
    /// This player's name (the sign-in's), for a chat line of its own before the lobby lists it.
    name: String,
}

impl NetLobby {
    /// `signed_in_as`: the sign-in's id; `join`: the lobby code to join (None: a lobby of this player's own);
    /// `compat`: the game's compatibility id.
    pub fn new(signed_in_as: &str, join: Option<String>, compat: &str, name: &str) -> NetLobby {
        NetLobby {
            id: String::new(),
            rules: LobbyRules::new(),
            signed_in_as: signed_in_as.to_string(),
            by_code: join.is_some(),
            join_code: join,
            compat: compat.to_string(),
            session: None,
            opening: false,
            gone: false,
            attached: true,
            opening_join: None,
            out: Vec::new(),
            events: Vec::new(),
            chat: Vec::new(),
            noted: None,
            send_gate: None,
            received: IndexMap::new(),
            received_all: None,
            started_match: false,
            name: name.to_string(),
        }
    }

    pub fn attach(&mut self) {
        self.attached = true;
    }

    /// The widget is gone (a match is on): the echoes have nobody to reach.
    pub fn detach(&mut self) {
        self.attached = false;
        self.events.clear();
    }

    pub fn rules(&self) -> &LobbyRules {
        &self.rules
    }

    /// The lobby's session is on the chosen backend, so online going off ends it (a direct code's join is not; before
    /// its session opens, a join is let run).
    pub fn on_chosen(&self) -> bool {
        self.session.as_ref().map_or(!self.by_code, |s| s.chosen)
    }

    /// This player's id in the lobby: the session's own once it opened, the sign-in's before.
    pub fn my_id(&self) -> &str {
        match &self.session {
            Some(s) if !s.me.is_empty() => &s.me,
            _ => &self.signed_in_as,
        }
    }

    pub fn alive(&self) -> bool {
        !self.gone && (self.opening || self.session.is_some())
    }

    /// The lobby's code (what a friend pastes to join), '' before the session stands.
    pub fn code(&self) -> &str {
        self.session.as_ref().map_or("", |s| &s.code)
    }

    pub fn is_open(&self) -> bool {
        self.session.is_some()
    }

    pub fn online(&self) -> bool {
        true
    }

    pub fn has_ready_button(&self) -> bool {
        !self.am_i_leader()
    }

    pub fn am_i_leader(&self) -> bool {
        self.rules.leader_id == self.my_id()
    }

    pub fn is_ready(&self, account_id: &str) -> bool {
        self.rules.ready.iter().any(|r| r == account_id)
    }

    /// Every human has to be ready (the leader: back from the last match; the others: their READY button).
    pub fn can_start_match(&self) -> bool {
        self.rules.can_start_match() && self.rules.players().iter().all(|p| p.is_bot || self.is_ready(&p.account_id))
    }

    /// The commands for the program since the last take.
    pub fn take_cmds(&mut self) -> Vec<OnlineCmd> {
        std::mem::take(&mut self.out)
    }

    /// The listeners' calls since the last take.
    pub fn take_events(&mut self) -> Vec<LobbyEvent> {
        std::mem::take(&mut self.events)
    }

    fn emit(&mut self, e: LobbyEvent) {
        if self.attached {
            self.events.push(e);
        }
    }

    fn send(&mut self, m: ClientMessage) {
        if self.session.is_some() {
            self.out.push(OnlineCmd::Send(Box::new(m)));
        }
    }

    // ---------- the program's frame ----------

    /// The frame's view: the session's answers and messages, and a connection lost. The messages are timed by the
    /// view's clock (`OnlineView::now`), which a lobby kept across its matches can rely on.
    pub fn feed(&mut self, view: &OnlineView) {
        let now = view.now;
        for e in &view.events {
            match e {
                OnlineEvent::Opened => self.opened(view),
                OnlineEvent::Server(m) if self.session.is_some() => self.message(m, now),
                // Unverified: listeners are taken to belong to the session opened; a Closed here carries no session,
                // so while opening, the page's own close (`join`'s Close) and an open's replace (`Replaced`) are taken
                // as the previous session's and the open goes on. A later open by another screen that replaces this
                // one's leaves it opening.
                OnlineEvent::Closed(ClosedWhy::Left | ClosedWhy::Replaced) if self.opening => {}
                OnlineEvent::Closed(why) => {
                    if self.opening {
                        self.open_failed(why);
                    } else if self.session.is_some() && !self.gone {
                        self.connection_lost();
                    }
                }
                _ => {}
            }
        }
        // online going off ends a lobby on the chosen backend (online.onChanged in the widget's OnInitialized)
        if self.session.is_some() && !view.connected && self.on_chosen() {
            self.connection_lost();
        }
    }

    fn opened(&mut self, view: &OnlineView) {
        if !self.opening {
            return;
        }
        self.opening = false;
        if self.gone {
            self.out.push(OnlineCmd::Close);
            return;
        }
        let s = view.session.as_ref();
        self.session = Some(Taken {
            me: s.map(|s| s.me.clone()).unwrap_or_default(),
            code: s.map(|s| s.code.clone()).unwrap_or_default(),
            chosen: s.is_some_and(|s| s.chosen),
        });
        // the greeting of the open: lobby.join with the code, or lobby.enter (each carries the compatibility id)
        let m = match self.opening_join.take() {
            Some(lobby) => ClientMessage::LobbyJoin { lobby, compat: self.compat.clone() },
            None => ClientMessage::LobbyEnter { compat: self.compat.clone() },
        };
        self.send(m);
    }

    /// `online.session` threw: the lobby is over before it began.
    fn open_failed(&mut self, why: &ClosedWhy) {
        self.opening = false;
        if self.gone {
            return;
        }
        self.gone = true;
        self.close();
        self.emit(LobbyEvent::Error(LobbyNote::Closed(why.clone())));
    }

    fn message(&mut self, m: &ServerMessage, now: f64) {
        match m {
            ServerMessage::LobbyState { id, ev, data, who } => self.state(id, ev, data, who.as_ref(), now),
            ServerMessage::LobbyKicked => {
                self.close();
                self.emit(LobbyEvent::Kicked);
            }
            ServerMessage::LobbyError { why, name } => {
                self.emit(LobbyEvent::Error(LobbyNote::Lobby { why: *why, name: name.clone() }));
            }
            // (the port's own) the lobby's invitedOnly option kept this player out; the connection closes after it
            ServerMessage::LobbyRefused { .. } => {
                if self.gone {
                    return;
                }
                self.gone = true;
                self.close();
                self.emit(LobbyEvent::Error(LobbyNote::InvitedOnlyRefused));
            }
            ServerMessage::LobbyStartFailed => self.emit(LobbyEvent::StartMatchFailed),
            ServerMessage::Chat { from, to, text } => {
                if *to != ChatTo::Lobby {
                    return;
                }
                let Some(text) = self.receive_chat(from, text, now) else { return };
                let name = self.rules.find(from).map(|p| p.username.clone()).unwrap_or_default();
                self.add_chat(ChatLine { from: from.clone(), name, to: ChatKind::Lobby, text, mine: false });
            }
            // The match start: every client's Matchmaking_HandleOnRematchStarted: the pre-match on the rematch path.
            ServerMessage::PmBegin(pm) => {
                if pm.kind != PmKind::Lobby {
                    return;
                }
                self.started_match = true;
                self.emit(LobbyEvent::MatchStarting);
            }
            _ => {}
        }
    }

    fn state(
        &mut self,
        id: &str,
        ev: &str,
        data: &mvs_protocol::lobby_rules::LobbyData,
        who: Option<&LobbyPlayer>,
        now: f64,
    ) {
        let was_leader = self.rules.leader_id.clone();
        self.id = id.to_string();
        self.rules.load(data);
        self.note_members(now);
        let r = &self.rules;
        let e = match ev {
            "started" => {
                self.started();
                None
            }
            "teamStyle" => Some(LobbyEvent::TeamStyleUpdated(r.team_style)),
            "matchDuration" => Some(LobbyEvent::MatchDurationUpdated(r.match_duration)),
            "numRingoutsForWin" => Some(LobbyEvent::NumRingoutsForWinUpdated(r.num_ringouts_for_win)),
            "allowHazards" => Some(LobbyEvent::AllowHazardsUpdated(r.allow_hazards)),
            "enabledMaps" => Some(LobbyEvent::EnabledMapsUpdated(r.enabled_maps.clone())),
            "options" => Some(LobbyEvent::OptionsUpdated),
            "ready" => Some(LobbyEvent::ReadyPlayersUpdated),
            "kicked" if who.is_some() => who.cloned().map(LobbyEvent::PlayerKicked),
            _ => Some(LobbyEvent::TeamsUpdated),
        };
        if let Some(e) = e {
            self.emit(e);
        }
        if ev != "started" && self.rules.leader_id != was_leader {
            self.emit(LobbyEvent::NewLeader(self.rules.leader_id.clone()));
        }
    }

    /// The lobby's creation answer: the teams, then the settings.
    fn started(&mut self) {
        let r = &self.rules;
        let es = [
            LobbyEvent::LobbyStarted,
            LobbyEvent::TeamStyleUpdated(r.team_style),
            LobbyEvent::MatchDurationUpdated(r.match_duration),
            LobbyEvent::NumRingoutsForWinUpdated(r.num_ringouts_for_win),
            LobbyEvent::AllowHazardsUpdated(r.allow_hazards),
            LobbyEvent::EnabledMapsUpdated(r.enabled_maps.clone()),
            LobbyEvent::OptionsUpdated,
        ];
        for e in es {
            self.emit(e);
        }
    }

    /// The session ended under the lobby (the host left, the link was lost, online went off).
    pub fn connection_lost(&mut self) {
        if self.gone {
            return;
        }
        self.gone = true;
        self.close();
        self.emit(LobbyEvent::Closed);
    }

    fn close(&mut self) {
        // (Lobby.kept is not touched here: the carry's net slot is the widget's to clear)
        self.session = None;
    }

    // ---------- the chat ----------

    /// Sends a chat message to the lobby: the cleaned text (the lobby reads no channel).
    pub fn say(&mut self, text: &str, now: f64) -> Say {
        if self.session.is_none() {
            return Say::Nothing;
        }
        let t = clean_chat(&Value::from(text));
        if t.is_empty() {
            return Say::Nothing;
        }
        if !self.send_gate.get_or_insert_with(|| ChatGate::new(1.0, now)).allow(now) {
            return Say::TooFast;
        }
        self.out.push(OnlineCmd::Send(Box::new(ClientMessage::Chat { text: t.clone(), all: true })));
        // (`rules.find(myId)?.username ?? identity.name ?? ''`: a listed player's name even when it is '')
        let name = self.rules.find(self.my_id()).map_or_else(|| self.name.clone(), |p| p.username.clone());
        let mine = self.my_id().to_string();
        self.add_chat(ChatLine { from: mine, name, to: ChatKind::Lobby, text: t.clone(), mine: true });
        Say::Sent(t)
    }

    fn add_chat(&mut self, line: ChatLine) {
        self.chat.push(line.clone());
        if self.chat.len() > CHAT_KEPT {
            let over = self.chat.len() - CHAT_KEPT;
            self.chat.drain(..over);
        }
        self.emit(LobbyEvent::Chat(line));
    }

    /// A message the host core passed on, as it is shown: the cleaned text, or None when it is empty, its sender is
    /// not named, or it is past the sender's or all senders' budget.
    fn receive_chat(&mut self, from: &str, text: &str, now: f64) -> Option<String> {
        if from.is_empty() {
            return None;
        }
        let t = clean_chat(&Value::from(text));
        if t.is_empty() {
            return None;
        }
        if !self.received.contains_key(from) {
            if self.received.len() >= RECEIVE_SENDERS {
                self.received.shift_remove_index(0);
            }
            self.received.insert(from.to_string(), ChatGate::new(RECEIVE_TIMES, now));
        }
        let all = self.received_all.get_or_insert_with(|| ChatGate::new(RECEIVE_ALL_TIMES, now));
        let a = self.received.get_mut(from).is_some_and(|g| g.allow(now));
        // (`gate() && receivedAll()`: the all-senders budget is spent only when the sender's allows)
        (a && all.allow(now)).then_some(t)
    }

    /// The lobby's humans against the notes so far: a note per human who came in or went (none at the first state).
    fn note_members(&mut self, t: f64) {
        let now: IndexMap<String, String> = self
            .rules
            .players()
            .into_iter()
            .filter(|p| !p.is_bot)
            .map(|p| (p.account_id.clone(), p.username.clone()))
            .collect();
        let Some(noted) = &mut self.noted else {
            self.noted = Some(
                now.iter()
                    .map(|(id, name)| (id.clone(), Noted { here: true, at: f64::NEG_INFINITY, name: name.clone() }))
                    .collect(),
            );
            return;
        };
        let mut ids: Vec<String> = now.keys().cloned().collect();
        for k in noted.keys() {
            if !now.contains_key(k) {
                ids.push(k.clone());
            }
        }
        let mut lines = Vec::new();
        for id in ids {
            let last =
                noted.get(&id).cloned().unwrap_or(Noted { here: false, at: f64::NEG_INFINITY, name: String::new() });
            let here = now.contains_key(&id);
            let name = now.get(&id).cloned().unwrap_or_else(|| last.name.clone());
            if here == last.here {
                if !here && t - last.at >= NOTE_AGAIN_MS {
                    noted.shift_remove(&id);
                } else {
                    noted.insert(id, Noted { name, ..last });
                }
                continue;
            }
            if t - last.at < NOTE_AGAIN_MS {
                continue;
            }
            noted.insert(id.clone(), Noted { here, at: t, name: name.clone() });
            lines.push(ChatLine {
                from: id,
                name,
                to: if here { ChatKind::Joined } else { ChatKind::Left },
                text: String::new(),
                mine: false,
            });
        }
        for l in lines {
            self.add_chat(l);
        }
    }

    // ---------- the requests ----------

    /// Opens the lobby's session and enters it: this player's net service makes the lobby, a guest's joins it by its
    /// code.
    fn open(&mut self, join: Option<String>) {
        self.opening = true;
        self.opening_join = join.clone();
        self.out.push(OnlineCmd::Open(match join {
            Some(code) => SessionTarget::Join(code),
            None => SessionTarget::Host,
        }));
    }

    /// The lobby screen is up: creates the lobby (BPI_StartCustomGameLobby), joins the one of an invitation
    /// (JoinCustomGameLobby), or is back in it after a match.
    pub fn start(&mut self) {
        let join = self.join_code.take();
        if self.session.is_some() && join.is_none() {
            let compat = self.compat.clone();
            self.send(ClientMessage::LobbyEnter { compat });
        } else if !self.opening {
            self.open(join);
        }
    }

    /// An invitation accepted while this lobby's screen is up: this session is left for the other lobby's.
    pub fn join(&mut self, code: &str) {
        if self.session.is_some() {
            self.out.push(OnlineCmd::Close);
        }
        self.session = None;
        self.open(Some(code.to_string()));
    }

    pub fn set_team_style(&mut self, style: u8) {
        self.send(ClientMessage::LobbySetTeamStyle { style });
    }
    pub fn set_match_duration(&mut self, sec: u32) {
        self.send(ClientMessage::LobbySetMatchDuration { sec });
    }
    pub fn set_num_ringouts_for_win(&mut self, n: u32) {
        self.send(ClientMessage::LobbySetNumRingoutsForWin { n });
    }
    pub fn set_allow_hazards(&mut self, on: bool) {
        self.send(ClientMessage::LobbySetAllowHazards { on });
    }
    pub fn set_maps(&mut self, maps: &[String]) {
        self.send(ClientMessage::LobbySetMaps { maps: maps.to_vec() });
    }
    pub fn set_option(&mut self, key: OptionKey, value: OptionValue) {
        let key = match key {
            OptionKey::Public => "public",
            OptionKey::Stocks => "stocks",
            OptionKey::InvitedOnly => "invitedOnly",
            OptionKey::Mutators => "mutators",
            OptionKey::Wheel => "wheel",
        };
        let value = match value {
            OptionValue::Bool(b) => OptionWire::Bool(b),
            OptionValue::Set(m) => OptionWire::Number(m.0),
        };
        self.send(ClientMessage::LobbySetOption { key: key.to_string(), value });
    }
    pub fn add_bot(&mut self, team: usize, character: &str, skin: &str, difficulty: i32) {
        self.send(ClientMessage::LobbyAddBot {
            team: u32::try_from(team).unwrap_or(0),
            character: character.to_string(),
            skin: skin.to_string(),
            difficulty,
        });
    }
    pub fn kick_player(&mut self, id: &str) {
        self.send(ClientMessage::LobbyKickPlayer { id: id.to_string() });
    }
    pub fn join_team(&mut self, _account_id: &str, team: usize) {
        self.send(ClientMessage::LobbyJoinTeam { team: u32::try_from(team).unwrap_or(0) });
    }
    pub fn update_settings_for_bot(&mut self, id: &str, character: &str, difficulty: i32, skin: &str) {
        self.send(ClientMessage::LobbyUpdateSettingsForBot {
            id: id.to_string(),
            character: character.to_string(),
            difficulty,
            skin: skin.to_string(),
        });
    }
    pub fn promote_new_leader(&mut self, id: &str) {
        self.send(ClientMessage::LobbyPromoteNewLeader { id: id.to_string() });
    }
    /// (the port's own) the keeper lets an invited id in while the lobby's invitedOnly option is on.
    pub fn invite(&mut self, id: &str) {
        self.send(ClientMessage::LobbyInvite { id: id.to_string() });
        if self.session.is_some() {
            self.out.push(OnlineCmd::Invite { to: id.to_string() });
        }
    }
    pub fn start_match(&mut self) {
        self.send(ClientMessage::LobbyStartMatch);
    }
    pub fn set_ready(&mut self, on: bool) {
        self.send(ClientMessage::LobbySetReady { on });
    }
    /// The session ends with it: the host's leaving ends it for every member.
    pub fn leave(&mut self) {
        self.gone = true;
        self.send(ClientMessage::LobbyLeave);
        if self.session.is_some() || self.opening {
            self.out.push(OnlineCmd::Close);
        }
        self.close();
    }

    pub fn options(&self) -> &LobbyOptions {
        &self.rules.options
    }
}

/// The input's character limit of the lobby chat (protocol chat CHAT_LIMIT).
pub const CHAT_INPUT_LIMIT: usize = CHAT_LIMIT;

/// What a lobby note says (worded from the code).
pub fn lobby_error_text(why: LobbyErrorWhy, name: Option<&str>) -> String {
    let name = name.unwrap_or_default();
    match why {
        LobbyErrorWhy::QueuedSelf => custom_text::LOBBY_QUEUED_SELF.to_string(),
        LobbyErrorWhy::NoLobby => custom_text::LOBBY_NO_LOBBY.to_string(),
        LobbyErrorWhy::LobbyFull => custom_text::LOBBY_FULL.to_string(),
        LobbyErrorWhy::CompatMissing => custom_text::COMPAT_MISSING.to_string(),
        LobbyErrorWhy::CompatDiffers => custom_text::COMPAT_DIFFERS.to_string(),
        LobbyErrorWhy::CompatMineOlder => custom_text::COMPAT_MINE_OLDER.replace("{name}", name),
        LobbyErrorWhy::CompatTheirsOlder => custom_text::COMPAT_THEIRS_OLDER.replace("{name}", name),
    }
}

/// The words of a note the lobby shows as an error (OnCustomGameError's text).
pub fn note_text(n: &LobbyNote) -> String {
    match n {
        LobbyNote::InvitedOnlyRefused => custom_text::INVITED_ONLY_REFUSED.to_string(),
        LobbyNote::Lobby { why, name } => lobby_error_text(*why, name.as_deref()),
        LobbyNote::Closed(why) => closed_text(why),
    }
}

/// A session's end worded for the lobby page.
fn closed_text(why: &ClosedWhy) -> String {
    crate::menu::online_text::closed_why_text(why)
}
