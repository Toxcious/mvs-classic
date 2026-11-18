//! The host core's JSON messages: every message is `{ t: <name>, ...body }`, one enum variant
//! per name. The core writes `ServerMessage`, the game writes `ClientMessage`; each side reads what it receives from
//! `serde_json::Value` field by field, as a JavaScript handler reads it, so these enums are for writing. A match's
//! binary packets are `match_wire`'s.
//!
//! The wire carries a code instead of the words a player reads (a lobby error, the compatibility note);
//! each game words it in its own UI.

use serde::{Deserialize, Serialize};

use crate::loadout::NetLoadout;
use crate::lobby_rules::{LobbyData, LobbyPlayer};
use crate::names::clean_name;
use crate::online::{MatchStartMsg, PlayerCosmetics, PmBegin, PmPick, QueueFoundMsg, QueueMode, QueueParty};
use crate::profile::{MatchRecord, MatchReportPlayer, ProfileWire, TrackerCounts};
use crate::setup::MatchSetupWire;

/// A lobby setting's value as `lobby.setOption` carries it: a bool row's true or false, a choice row's number.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OptionWire {
    Bool(bool),
    Number(u32),
}

/// What a game sends the host core.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum ClientMessage {
    /// Comes online under this name, or takes a new one; `proto` the game's PROTOCOL (another is refused), `app` its
    /// release version. Who the player is comes from the link, never from the message. The net service adds, never
    /// the game's own menus: `epic` this game's Epic product user id while Epic is signed in, `rtt` its smallest round
    /// trip to the relay (ms), `direct` the player's Direct connections setting (a match's path, `crate::path`).
    #[serde(rename = "hello")]
    Hello {
        proto: u32,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        icon: Option<String>,
        app: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        epic: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rtt: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direct: Option<bool>,
    },
    #[serde(rename = "profile.icon")]
    ProfileIcon { icon: String },
    #[serde(rename = "ping")]
    Ping { n: u32 },
    #[serde(rename = "lobby.enter")]
    LobbyEnter { compat: String },
    #[serde(rename = "lobby.join")]
    LobbyJoin { lobby: String, compat: String },
    #[serde(rename = "lobby.leave")]
    LobbyLeave,
    #[serde(rename = "lobby.joinTeam")]
    LobbyJoinTeam { team: u32 },
    #[serde(rename = "lobby.setReady")]
    LobbySetReady { on: bool },
    #[serde(rename = "lobby.setTeamStyle")]
    LobbySetTeamStyle { style: u8 },
    #[serde(rename = "lobby.setMatchDuration")]
    LobbySetMatchDuration { sec: u32 },
    #[serde(rename = "lobby.setNumRingoutsForWin")]
    LobbySetNumRingoutsForWin { n: u32 },
    #[serde(rename = "lobby.setAllowHazards")]
    LobbySetAllowHazards { on: bool },
    #[serde(rename = "lobby.setMaps")]
    LobbySetMaps { maps: Vec<String> },
    /// One of the lobby's own settings (`lobby_rules::OptionKey`, by its key): the leader's.
    #[serde(rename = "lobby.setOption")]
    LobbySetOption { key: String, value: OptionWire },
    #[serde(rename = "lobby.addBot")]
    LobbyAddBot { team: u32, character: String, skin: String, difficulty: i32 },
    #[serde(rename = "lobby.updateSettingsForBot")]
    LobbyUpdateSettingsForBot { id: String, character: String, difficulty: i32, skin: String },
    #[serde(rename = "lobby.kickPlayer")]
    LobbyKickPlayer { id: String },
    #[serde(rename = "lobby.promoteNewLeader")]
    LobbyPromoteNewLeader { id: String },
    #[serde(rename = "lobby.startMatch")]
    LobbyStartMatch,
    /// `draw`: a random whole number in [0, 2^32) the game draws for each join; the core combines every human's into
    /// the found match's draws. `waited`: ms this player has queued already under another host (the bot wait counts
    /// from then). `party`: the sender queues with its party. `rtt`: the game's smallest round trip to the relay (ms),
    /// which the relay's queue core pairs by (queue_rules::rtt_band); None: matched with anyone.
    #[serde(rename = "queue.join")]
    QueueJoin {
        modes: Vec<QueueMode>,
        character: String,
        skin: String,
        compat: String,
        #[serde(rename = "botAfter", default, skip_serializing_if = "Option::is_none")]
        bot_after: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        loadout: Option<NetLoadout>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        draw: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        waited: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        party: Option<QueueParty>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rtt: Option<u32>,
    },
    /// The sender invited this id to the lobby: a lobby whose invitedOnly option is on lets in only the ids its
    /// members invited.
    #[serde(rename = "lobby.invite")]
    LobbyInvite { id: String },
    #[serde(rename = "queue.cancel")]
    QueueCancel,
    #[serde(rename = "pm.preview")]
    PmPreview {
        character: String,
        skin: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        loadout: Option<NetLoadout>,
    },
    #[serde(rename = "pm.lock")]
    PmLock {
        character: String,
        skin: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        loadout: Option<NetLoadout>,
    },
    #[serde(rename = "pm.perks")]
    PmPerks { perks: Vec<String> },
    #[serde(rename = "pm.leave")]
    PmLeave,
    #[serde(rename = "match.loaded")]
    MatchLoaded,
    #[serde(rename = "match.desync")]
    MatchDesync {
        tick: u32,
        slot: u32,
        mine: u32,
        theirs: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        config: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        statics: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        build: Option<String>,
    },
    /// `players`: every slot's numbers as the sender's sim saw them, for the match record; `map` the MapData name it
    /// played on.
    #[serde(rename = "match.result")]
    MatchResult {
        team: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        players: Option<Vec<MatchReportPlayer>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        map: Option<String>,
    },
    /// Another player's profile: `id` a player of this session.
    #[serde(rename = "profile.get")]
    ProfileGet { id: String },
    /// The answer to profile.ask: this player's own profile (None: none to give) for the asker `to`.
    #[serde(rename = "profile.tell")]
    ProfileTell { to: String, profile: Option<ProfileWire> },
    /// The sender's rematch vote in a queue set, the first one only.
    #[serde(rename = "match.vote")]
    MatchVote { yes: bool },
    #[serde(rename = "match.leave")]
    MatchLeave,
    /// A chat message: in a going match `all` says all chat (else the sender's team), in the custom lobby's screen
    /// it goes to the lobby.
    #[serde(rename = "chat")]
    Chat { text: String, all: bool },
    /// A toast at the podium for `to`, a human of the sender's last match.record.
    #[serde(rename = "toast")]
    Toast { to: String },
    /// A path's measure (`crate::path`): the sender's round trip to the relay (ms) and to each other human of the
    /// path.peers.
    #[serde(rename = "path.report")]
    PathReport {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relay: Option<u32>,
        peers: Vec<PathLink>,
    },
    /// The news of a slot of the match this game's app referees (from the referee's game alone).
    #[serde(rename = "ref.news")]
    RefNews { r#match: u32, slot: u32, news: RefNewsWire },
    /// A game of a match a player's app referees has had no ruled packet for `match_wire::RESUME_MS`: the referee is
    /// lost.
    #[serde(rename = "ref.lost")]
    RefLost { r#match: u32 },
}

/// One link of a path.report: the other human's slot, the smallest round trip measured to it (ms; None: the link did
/// not stand), and whether the link went through Epic's relays (None: not known).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathLink {
    pub slot: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rtt: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relayed: Option<bool>,
}

/// A slot's news in ref.news (`referee::SlotNews`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RefNewsWire {
    Away,
    Back,
    Gone,
}

/// A human of a path's match: its slot and its Epic product user id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathPeer {
    pub slot: u32,
    pub epic: String,
}

/// Who a chat message went to (`ServerMessage::Chat`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatTo {
    Team,
    All,
    Lobby,
}

/// Why the host core closed a player's link, or ended its session: what its link's last word (a guest's FRAME_BYE)
/// carries (a code: the words a player reads are the UI's), and the core's log line (`log_name`). Each code's doc
/// quotes its text.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum HostWhy {
    /// 'host left': the hosting player's link is gone, or (a custom session) they left its lobby.
    HostLeft,
    /// 'kicked': the lobby's leader kicked the player (the owner closes the link).
    Kicked,
    /// 'replaced': a second link of the same player id took this one's place.
    Replaced,
    /// 'protocol': a hello of another PROTOCOL.
    Protocol,
    /// 'not invited': a lobby.join while the lobby's invitedOnly option is on, by nobody invited.
    NotInvited,
    /// 'message too big': a message over MAX_MESSAGE bytes (a profile.tell over its own cap).
    MessageTooBig,
    /// 'too many messages': past the link's message budget.
    TooManyMessages,
    /// 'link error': the link's send failed.
    LinkError,
    /// 'ended': a link that came after the session's end.
    Ended,
    /// The session's owner ended it (Core::end): its own text.
    Owner { text: String },
}

impl HostWhy {
    /// The code's name in the core's log lines only (`dropped <name> <why>`, `session over: <why>`), the log's
    /// words; never shown to a player.
    pub fn log_name(&self) -> &str {
        match self {
            HostWhy::HostLeft => "host left",
            HostWhy::Kicked => "kicked",
            HostWhy::Replaced => "replaced",
            HostWhy::Protocol => "protocol",
            HostWhy::NotInvited => "not invited",
            HostWhy::MessageTooBig => "message too big",
            HostWhy::TooManyMessages => "too many messages",
            HostWhy::LinkError => "link error",
            HostWhy::Ended => "ended",
            HostWhy::Owner { text } => text,
        }
    }
}

/// Why a match ended for a peer (match.left). Rematch: the set's next game took the leaver out of this one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeftWhy {
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "disconnected")]
    Disconnected,
    #[serde(rename = "load timeout")]
    LoadTimeout,
    #[serde(rename = "too long")]
    TooLong,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "local match")]
    LocalMatch,
    #[serde(rename = "rematch")]
    Rematch,
}

/// Why the core refused a hello (refused): another PROTOCOL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusedWhy {
    Protocol,
}

/// Why a lobby did not let a player in (lobby.refused): its invitedOnly option is on and nobody in it invited them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LobbyRefusedWhy {
    #[serde(rename = "invitedOnly")]
    InvitedOnly,
}

/// What a lobby request ran into (lobby.error), worded by the game. The compatibility codes come with the other
/// player's `name`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LobbyErrorWhy {
    /// The sender is in the queue or a found match and asks for a lobby.
    QueuedSelf,
    /// No lobby to join: none yet, a queue session's, or one the joiner was kicked from.
    NoLobby,
    LobbyFull,
    /// The join carried no usable compatibility id.
    CompatMissing,
    /// The two games were made from different builds, game code or importers, with no release order to go by.
    CompatDiffers,
    /// The receiver's game is the older release: it updates.
    CompatMineOlder,
    /// The other player's game is the older release: they update.
    CompatTheirsOlder,
}

/// Why a queue.state is off. The values are the codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QueueWhy {
    /// A queue.join without a usable compatibility id.
    Compat,
    /// The party's join is not one (its list, or a mode it cannot play).
    Party,
    /// The party's members did not all join within PARTY_WAIT_MS.
    PartyLate,
    /// A player of the found match left it before its pre-match began (`name` the leaver's).
    Left,
    Error,
    Cancel,
}

/// Why a pre-match ended (pm.end): a player left it (`name`), or a step of its record failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PmEndWhy {
    Left,
    Error,
}

/// A pre-match's every pick at its reveal (pm.all).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PmAll {
    pub picks: Vec<Option<PmPick>>,
    pub map: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_pick: Option<u32>,
    pub cosmetics: Vec<Option<PlayerCosmetics>>,
}

/// What the host core sends a game.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum ServerMessage {
    /// The first message on every connection: the host's PROTOCOL and its name.
    #[serde(rename = "server")]
    Server { proto: u32, name: String },
    /// The connection is closed: a hello of another PROTOCOL (`server` the host's own).
    #[serde(rename = "refused")]
    Refused { why: RefusedWhy, server: u32 },
    #[serde(rename = "welcome")]
    Welcome { id: String, name: String },
    #[serde(rename = "name.taken")]
    NameTaken { name: String },
    #[serde(rename = "name.refused")]
    NameRefused { name: String },
    #[serde(rename = "replaced")]
    Replaced,
    /// The ping's `n` as it came.
    #[serde(rename = "pong")]
    Pong { n: serde_json::Value },
    /// A host's own line to its players: the hosting player's text, not the game's words (no core sends one).
    #[serde(rename = "notice")]
    Notice { text: String },
    #[serde(rename = "lobby.state")]
    LobbyState {
        id: String,
        ev: String,
        data: LobbyData,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        who: Option<LobbyPlayer>,
    },
    #[serde(rename = "lobby.kicked")]
    LobbyKicked,
    /// The lobby did not let this player in; the connection closes after it.
    #[serde(rename = "lobby.refused")]
    LobbyRefused { why: LobbyRefusedWhy },
    /// `name`: the other player's, with a compatibility code.
    #[serde(rename = "lobby.error")]
    LobbyError {
        why: LobbyErrorWhy,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    #[serde(rename = "lobby.startFailed")]
    LobbyStartFailed,
    #[serde(rename = "queue.state")]
    QueueState {
        on: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        why: Option<QueueWhy>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    #[serde(rename = "queue.found")]
    QueueFound(QueueFoundMsg),
    #[serde(rename = "pm.begin")]
    PmBegin(PmBegin),
    #[serde(rename = "pm.preview")]
    PmPreview { character: String, skin: String, slot: u32 },
    #[serde(rename = "pm.locked")]
    PmLocked {
        character: String,
        skin: String,
        slot: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cosmetics: Option<PlayerCosmetics>,
    },
    #[serde(rename = "pm.all")]
    PmAll(PmAll),
    #[serde(rename = "pm.ready")]
    PmReady { slot: u32 },
    #[serde(rename = "pm.end")]
    PmEnd { why: PmEndWhy, name: String },
    #[serde(rename = "match.start")]
    MatchStart(MatchStartMsg<MatchSetupWire>),
    /// The match starts `in` ms from now; `peers` each slot's player id ('' or past its end: a bot); `referee` the id
    /// of the player whose app referees the match: every human's game sends its packets there.
    #[serde(rename = "match.go")]
    MatchGo {
        #[serde(rename = "in")]
        in_ms: u32,
        peers: Vec<String>,
        referee: String,
    },
    /// A peer left (`slot`, -1: the match is ended for all); `tick` is 0 (the referee's streams say where the
    /// leaver's inputs end).
    #[serde(rename = "match.left")]
    MatchLeft { slot: i32, why: LeftWhy, tick: u32 },
    #[serde(rename = "match.desync")]
    MatchDesync { slot: u32 },
    /// A human of the match is away (`away`: its link dropped) or back.
    #[serde(rename = "match.away")]
    MatchAway { slot: u32, away: bool },
    /// A rematch vote of a queue set, a human's or a bot's, to all.
    #[serde(rename = "match.votes")]
    MatchVotes { slot: u32, yes: bool },
    /// A queue set's game decided: `wins` by team, `game` the games played; `rematch` true on every slot's yes.
    #[serde(rename = "match.set")]
    MatchSet { rematch: bool, wins: Vec<u32>, game: u32 },
    /// The answer to profile.get: None for an id not in the session, or no answer in time.
    #[serde(rename = "profile")]
    Profile { id: String, profile: Option<ProfileWire> },
    /// Another player of the session (`from`) asks for this player's profile: answered with profile.tell.
    #[serde(rename = "profile.ask")]
    ProfileAsk { from: String },
    /// A finished match's record, to each of its humans: `game` the set's game (1 for a custom match), `totals`
    /// whether the receiver's lifetime counts take it, `trackers` the receiver's stat-tracker gain, `left` the
    /// receiver left before its end.
    #[serde(rename = "match.record")]
    MatchRecord {
        record: MatchRecord,
        game: u32,
        totals: bool,
        trackers: Option<TrackerCounts>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        left: Option<bool>,
    },
    /// Another player's chat message (`from` the sender's id); the sender gets none back.
    #[serde(rename = "chat")]
    Chat { from: String, to: ChatTo, text: String },
    /// Another player toasted this one at the podium.
    #[serde(rename = "toast")]
    Toast { from: String, name: String },
    /// A path's measure starts (`crate::path`), with pm.begin or queue.found: the other humans to ping over Epic;
    /// `direct` a custom lobby whose humans all have Direct connections on.
    #[serde(rename = "path.peers")]
    PathPeers { peers: Vec<PathPeer>, direct: bool },
    /// The path chosen, to every human: `referee` its slot (-1: the relay), `ms` by slot the round trip to the
    /// referee (0 the referee's own, None a bot or not known), `far` by slot past `path::FAR_ONE_WAY_MS` on every way.
    #[serde(rename = "pm.path")]
    PmPath { referee: i32, ms: Vec<Option<u32>>, far: Vec<bool> },
    /// To the game whose app referees the match: it goes `in` ms from now, with these humans.
    #[serde(rename = "ref.go")]
    RefGo {
        r#match: u32,
        #[serde(rename = "in")]
        in_ms: u32,
        humans: Vec<PathPeer>,
    },
    /// To the referee's game: a human left the match.
    #[serde(rename = "ref.left")]
    RefLeft { r#match: u32, slot: u32 },
    /// To the referee's game: the match is over.
    #[serde(rename = "ref.end")]
    RefEnd { r#match: u32 },
}

impl ClientMessage {
    /// The message as its JSON text.
    pub fn text(&self) -> String {
        serde_json::to_string(self).expect("a client message is always JSON")
    }
}

impl ServerMessage {
    /// The message as its JSON text.
    pub fn text(&self) -> String {
        serde_json::to_string(self).expect("a server message is always JSON")
    }

    /// Every player name the message carries, through [`clean_name`]: a host core is
    /// another player's process (the queue's host a stranger), so a game shows no name as the host wrote it.
    pub fn clean_names(&mut self) {
        fn clean(s: &mut String) {
            *s = clean_name(s);
        }
        fn record(r: &mut MatchRecord) {
            r.players.iter_mut().for_each(|p| clean(&mut p.name));
        }
        match self {
            ServerMessage::Server { name, .. }
            | ServerMessage::Welcome { name, .. }
            | ServerMessage::NameTaken { name }
            | ServerMessage::NameRefused { name }
            | ServerMessage::PmEnd { name, .. }
            | ServerMessage::Toast { name, .. } => clean(name),
            ServerMessage::LobbyError { name, .. } | ServerMessage::QueueState { name, .. } => {
                name.iter_mut().for_each(clean);
            }
            ServerMessage::LobbyState { data, who, .. } => {
                data.teams.iter_mut().flatten().chain(who.iter_mut()).for_each(|p| clean(&mut p.username));
            }
            ServerMessage::QueueFound(f) => f.players.iter_mut().for_each(|p| clean(&mut p.name)),
            ServerMessage::PmBegin(b) => b.players.iter_mut().for_each(|p| clean(&mut p.name)),
            ServerMessage::MatchStart(s) => s.usernames.iter_mut().for_each(clean),
            ServerMessage::MatchRecord { record: r, .. } => record(r),
            ServerMessage::Profile { profile: Some(p), .. } => {
                clean(&mut p.name);
                p.matches.iter_mut().for_each(record);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_bodies() {
        assert_eq!(ClientMessage::LobbyLeave.text(), r#"{"t":"lobby.leave"}"#);
        assert_eq!(
            ClientMessage::LobbySetOption { key: "mutator".into(), value: OptionWire::Number(3) }.text(),
            r#"{"t":"lobby.setOption","key":"mutator","value":3}"#
        );
        assert_eq!(
            ServerMessage::MatchLeft { slot: -1, why: LeftWhy::LoadTimeout, tick: 0 }.text(),
            r#"{"t":"match.left","slot":-1,"why":"load timeout","tick":0}"#
        );
        assert_eq!(
            ServerMessage::QueueState { on: false, why: Some(QueueWhy::PartyLate), name: None }.text(),
            r#"{"t":"queue.state","on":false,"why":"partyLate"}"#
        );
        assert_eq!(
            ServerMessage::LobbyError { why: LobbyErrorWhy::CompatMineOlder, name: Some("Ann".into()) }.text(),
            r#"{"t":"lobby.error","why":"compat_mine_older","name":"Ann"}"#
        );
        assert_eq!(
            ServerMessage::MatchGo { in_ms: 3000, peers: vec!["a".into()], referee: "a".into() }.text(),
            r#"{"t":"match.go","in":3000,"peers":["a"],"referee":"a"}"#
        );
        let back: ServerMessage = serde_json::from_str(r#"{"t":"profile","id":"x","profile":null}"#).unwrap();
        assert_eq!(back, ServerMessage::Profile { id: "x".into(), profile: None });
    }

    #[test]
    fn a_path_s_messages() {
        let hello = ClientMessage::Hello {
            proto: 26,
            name: "A".into(),
            icon: None,
            app: "1".into(),
            epic: Some("e1".into()),
            rtt: Some(80),
            direct: Some(false),
        };
        assert_eq!(
            hello.text(),
            r#"{"t":"hello","proto":26,"name":"A","app":"1","epic":"e1","rtt":80,"direct":false}"#
        );
        let old: ClientMessage = serde_json::from_str(r#"{"t":"hello","proto":26,"name":"A","app":"1"}"#).unwrap();
        assert!(matches!(old, ClientMessage::Hello { epic: None, rtt: None, direct: None, .. }));
        let report = ClientMessage::PathReport {
            relay: Some(90),
            peers: vec![
                PathLink { slot: 1, rtt: Some(30), relayed: Some(true) },
                PathLink { slot: 2, rtt: None, relayed: None },
            ],
        };
        let t = report.text();
        assert_eq!(t, r#"{"t":"path.report","relay":90,"peers":[{"slot":1,"rtt":30,"relayed":true},{"slot":2}]}"#);
        assert_eq!(serde_json::from_str::<ClientMessage>(&t).unwrap(), report);
        assert_eq!(
            ClientMessage::RefNews { r#match: 3, slot: 1, news: RefNewsWire::Gone }.text(),
            r#"{"t":"ref.news","match":3,"slot":1,"news":"gone"}"#
        );
        assert_eq!(ClientMessage::RefLost { r#match: 3 }.text(), r#"{"t":"ref.lost","match":3}"#);
        let peers = vec![PathPeer { slot: 0, epic: "e0".into() }];
        assert_eq!(
            ServerMessage::PathPeers { peers: peers.clone(), direct: true }.text(),
            r#"{"t":"path.peers","peers":[{"slot":0,"epic":"e0"}],"direct":true}"#
        );
        let path = ServerMessage::PmPath { referee: -1, ms: vec![Some(0), None], far: vec![false, true] };
        assert_eq!(path.text(), r#"{"t":"pm.path","referee":-1,"ms":[0,null],"far":[false,true]}"#);
        let go = ServerMessage::RefGo { r#match: 2, in_ms: 250, humans: peers };
        let t = go.text();
        assert_eq!(t, r#"{"t":"ref.go","match":2,"in":250,"humans":[{"slot":0,"epic":"e0"}]}"#);
        assert_eq!(serde_json::from_str::<ServerMessage>(&t).unwrap(), go);
        assert_eq!(ServerMessage::RefLeft { r#match: 2, slot: 1 }.text(), r#"{"t":"ref.left","match":2,"slot":1}"#);
        assert_eq!(ServerMessage::RefEnd { r#match: 2 }.text(), r#"{"t":"ref.end","match":2}"#);
    }

    #[test]
    fn received_names_are_cleaned() {
        let mut m = ServerMessage::MatchStart(MatchStartMsg {
            seed: 1,
            setup: MatchSetupWire::default(),
            slot: 0,
            usernames: vec!["  A\u{202e}B  ".into(), "C\u{7}\tD".into()],
            map: String::new(),
            map_pick: None,
            local: None,
            match_id: None,
            perks: None,
        });
        m.clean_names();
        let ServerMessage::MatchStart(s) = m else { panic!("match.start") };
        assert_eq!(s.usernames, ["AB", "C D"]);
        let mut t =
            ServerMessage::Toast { from: "x".into(), name: "a\u{200b}\u{200b}bcdefghijklmnopqrstuvwxyz".into() };
        t.clean_names();
        assert_eq!(t, ServerMessage::Toast { from: "x".into(), name: "abcdefghijklmnopqrst".into() });
    }
}
