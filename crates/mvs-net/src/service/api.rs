//! What the game's pages and the net service say to each other (SessionTarget .. NetEvent, RelayOptions ..
//! BackendState): a page's requests, the service's events, and the backends the
//! service is told to sign in to.
//!
//! Every text for a player (a session's 'closed' why, a backend's off reason, a
//! browse's or the online list's reason, the relay question) is a code here, worded by the UI
//! (mvs-ui menu/custom_text.rs); each code's doc quotes the words it stands for. A reason the port does not
//! word (a backend's refusal, EOS's result name, a lobby code's parse error) stays a `text` field: anyone's words until
//! the UI shows them.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use mvs_protocol::messages::HostWhy;
use mvs_protocol::online::QueueMode;
use mvs_protocol::relay_wire::{KnownRelay, Presence as RelayPresence, Who};

use crate::backend::BackendKind;
use crate::backend::eos::EosOptions;
use crate::codes::CodeMakeWhy;
use crate::fail::NetFail;
use crate::lan::direct::HostStartWhy;
use crate::link::relay::NetFaults;

/// What a page's 'open' opens: a custom lobby hosted by this player (a host core of the
/// service, its lobby on the chosen backend), a join by a lobby's code (the backend the code names, or a direct
/// host's link), or the queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionTarget {
    Host,
    Join(String),
    Queue,
}

/// A page's messages to the net service. A page has one session at a time; a message for a
/// session that is not open is dropped.
#[derive(Clone, Debug, PartialEq)]
pub enum NetRequest {
    /// Opens the page's session (one still open is closed first); answered by `Opened` or `Closed`.
    Open(SessionTarget),
    /// A message to the session's host core: a JSON text `{ t, ... }` (mvs_protocol::messages::ClientMessage).
    Msg(String),
    /// The page leaves its session (the host's leaving ends it for every member).
    Close,
    /// A match's packets start: match.go's `peers`, this page's slot and `referee`; the service sends to the referee
    /// and takes packets from it alone (its own referee when this player hosts).
    Match { peers: Vec<String>, slot: u32, referee: String },
    /// One of this page's match packets (mvs_protocol::match_wire), to the match's referee, in the order made.
    Send(Vec<u8>),
    /// The match is over for this page: its packets stop (the session stays open).
    MatchEnd,
    /// Invites a player (their backend id) to this page's open session; `name` this player's, for the invite.
    Invite { to: String, name: String },
    /// Searches the public custom lobbies of every backend on; answered by `Browsed` with this `id`.
    Browse { id: u32 },
    /// The player's answer to `Ask` `id`.
    Answer { id: u32, yes: bool },
    /// What the online list shows of this player, and their two settings (sent on each change; published on every
    /// relay signed in to, now and at each sign-in).
    Presence(RelayPresence),
    /// The online list; answered by `Who` with this `id`.
    Who { id: u32 },
    /// Invites a player (a relay id) to this page's party, making the party (this player its leader) when there is
    /// none; `name` this player's. Refused (`InviteRefused`) when the party is full or this player is not its leader.
    PartyInvite { to: String, name: String },
    /// Accepts a party invite (`Invited` kind Party): leaves the party this player is in, joins that one.
    PartyJoin { code: String },
    /// Leaves the party (the leader's leaving ends it for the member).
    PartyLeave,
    /// A message to the other members of the party, passed on as it is.
    PartySay(PartySay),
    /// The fighter this player's main menu shows (None: none yet), with its card to the party's other members (sent
    /// on each change; kept for a party made later).
    MenuFighter(Option<MenuFighter>),
}

/// (the port's own) The fighter a player's main menu shows on its Player1 actor: a fighter key and a SkinData name
/// ("" the default skin). A party's members send theirs with their cards (`PartyWire::Hi`); the other member's main
/// menu shows it on its Player3 actor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuFighter {
    pub key: String,
    pub skin: String,
}

/// (the port's own) What a party's pages tell each other, through their net services (on the
/// wire as `{ k, .. }`). The leader's: Open its queue's pre-match screen opened for these modes, Queue its
/// search started, Custom it is in this custom lobby. The member's: Ready its pick is locked for the queue. Either's:
/// Cancel stops the queue for both, Home it is back in the menus.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "lowercase")]
pub enum PartySay {
    Open { modes: Vec<QueueMode> },
    Queue { modes: Vec<QueueMode> },
    Custom { code: String },
    Home,
    Cancel,
    Ready,
}

/// A party member as the pages show it (its presence's name and icon).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartyMember {
    pub id: String,
    pub name: String,
    pub icon: String,
}

/// A page's party (NetEvent 'party'): its leader's id, its members (the leader first), this player's id in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartyView {
    pub leader: String,
    pub members: Vec<PartyMember>,
    pub me: String,
}

/// A public custom lobby a browse found: the backend it was found on, its code (joined by `Open(Join(code))`), its
/// members of its max, and its listing's text (mvs_protocol::lobby_listing: anyone's text, read through listing_of).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowsedLobby {
    pub backend: BackendKind,
    pub code: String,
    pub members: u32,
    pub max: u32,
    pub info: String,
}

/// Why a browse or the online list found nothing to show, or not all.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum BrowseWhy {
    /// '' every backend on was searched.
    All,
    /// 'offline', 'online play is off: ' + the status's why.
    Offline { why: StatusWhy },
    /// 'offline', "the online list is the relay's, and the relay is not connected (<why>): Epic and LAN have none".
    ListOff { why: OffWhy },
    /// 'failed', 'this relay has no online list (an older relay)'.
    NoList,
    /// 'failed', 'the relay did not give the online list: ' + the relay's reason.
    ListFailed { text: String },
    /// 'failed', each failed backend's '<Name>: <reason>' joined by '; ' (the other backends' rows still given; LAN's
    /// reason its port's that could not be listened on).
    Failed { backends: Vec<BackendText> },
}

/// A backend's own reason (its words, not the port's).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendText {
    pub kind: BackendKind,
    pub text: String,
}

/// A backend that is off and why (the status's '<Name>: <why>').
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendOff {
    pub kind: BackendKind,
    pub why: OffWhy,
}

/// Why a backend is off, its backend's name the UI's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum OffWhy {
    /// The plain 'off' reason (and the service's `why || 'off'`).
    Off,
    /// '<Name> is not turned on in the launcher'.
    NotOn,
    /// 'signing in to Epic Online Services'.
    SigningIn,
    /// 'connecting to the relay at <address>'.
    Connecting { address: String },
    /// 'the sign-in to <Name> changed'.
    SignInChanged,
    /// 'LAN did not start'.
    LanFailed,
    /// EOS's credentials or SDK are missing.
    Missing { why: EpicMissing },
    /// The backend's own reason (a sign-in refused, a link lost, LAN's port not listened on).
    Backend { text: String },
    /// The relay's reason, then '(trying again in <s> s)' or '(trying again in <ms> ms)'.
    Retrying { text: String, in_ms: u32 },
    /// The relay at the address proved another key than the one this install expects of it (`RelayOptions::pin`):
    /// `release` the release's own relay (online.json's relayId), else one the player said yes to; tried again in
    /// `in_ms`.
    RelayKey { release: bool, in_ms: u32 },
    /// 'the online settings changed' (signin.rs: its options changed, it signs in again).
    SettingsChanged,
    /// 'turned off in the online settings'.
    TurnedOff,
    /// 'the LAN port changed'.
    LanPortChanged,
    /// 'the LAN setting changed' (the LAN host's options changed while it was on).
    LanSettingChanged,
}

/// Why Epic's options are missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpicMissing {
    /// 'the online files are missing from this install'
    NotInstalled,
    /// 'not set up in this dev run (MVS_EOS_DLL and MVS_EOS_CREDENTIALS are not set)'
    NotSetUp,
}

/// The status's why: '' online; 'signing in to online play' with no backend's reason yet;
/// else each backend off with its reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum StatusWhy {
    Online,
    SigningIn,
    Off { backends: Vec<BackendOff> },
}

/// What a session is connected through (`NetEvent::Opened`): the lobby's connection line.
/// Deviation: the port's own (the build's lobbies are all on its servers).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum SessionOn {
    /// The relay hosts it.
    Relay,
    /// A player's game hosts it, over Epic Online Services.
    Epic,
    /// A player's game hosts it on a direct link (LAN, or a direct code's join): `at` the host's `address:port`.
    Lan { at: String },
    /// This game hosts it with no connection under it: this PC's players and bots alone.
    Local { why: LocalWhy },
}

/// Why a custom game is local (`SessionOn::Local`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum LocalWhy {
    /// The relay is signed in and runs another version of the game's protocol, and no other connection is on.
    RelayVersion,
    /// No connection is signed in: the status's why (signing in, or each backend off with its reason).
    Status { why: StatusWhy },
}

/// Why a session (NetEvent 'closed') or a party (NetEvent 'party' null) is over for this page. On the wire too: a
/// host's FRAME_BYE carries its why as this code's JSON (`Other` the text of a peer that sent none of these).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ClosedWhy {
    /// 'left': the page's own close (a party: the player left it).
    Left,
    /// 'replaced': a new 'open' while the session was open.
    Replaced,
    /// 'the net process stopped'.
    Stopped,
    /// 'no such session': an 'open' of no target.
    NoSession,
    /// 'online play is off: ' + the status's why.
    Offline { why: StatusWhy },
    /// 'online play is off: ' + NO_QUEUE ('the queue needs the relay or Epic, and neither is signed in (LAN has no
    /// queue): host a custom lobby, or turn on the relay or Epic in the launcher').
    NoQueue,
    /// The queue's relay runs another version of the game's protocol and Epic is not signed in. Never on the wire.
    RelayVersion,
    /// QUEUE_NOT_RELAYED ('Epic could not keep this match relayed: ') + EOS's reason.
    NotRelayed { text: String },
    /// '<Name> is off: <why>'.
    BackendOff { kind: BackendKind, why: OffWhy },
    /// 'the link to the relay was lost: ' + the relay's reason.
    RelayLost { text: String },
    /// WHY_LIMITED 'limited': the backend refused the lobby's making or join for its rate (EOS_LimitExceeded).
    Limited,
    /// 'the lobby could not be made: ' + the backend's reason.
    MakeFailed {
        #[serde(rename = "text")]
        why: NetFail,
    },
    /// 'the lobby could not be made: ' + why its LAN host did not start.
    MakeHostFailed { why: HostStartWhy },
    /// 'the lobby could not be made: ' + why its code cannot be written.
    MakeCodeFailed { why: CodeMakeWhy },
    /// 'the lobby could not be joined: ' + its why.
    JoinFailed { why: JoinWhy },
    /// 'the lobby has no host'.
    NoHost,
    /// 'the host did not answer'.
    HostSilent,
    /// 'a bad packet from the host'.
    BadPacket,
    /// 'bad packet': the host's word to a member whose packet broke its link.
    BadGuestPacket,
    /// 'the host closed the link'.
    HostClosed,
    /// 'the host left'.
    HostLeft,
    /// 'kicked'.
    Kicked,
    /// 'the lobby closed'.
    LobbyClosed,
    /// 'the lobby closed: ' + the backend's why (a hosting player's own lobby).
    LobbyGone { text: String },
    /// 'the link to the host was lost'.
    HostLost,
    /// 'the link to the host was lost: ' + the direct link's reason.
    HostLinkLost { text: String },
    /// 'the direct host stopped: ' + its reason.
    DirectStopped { text: String },
    /// 'the link to the host failed': the backend refused the join's text.
    SendFailed,
    /// The host core's end or its close of this player's link (mvs_host HostWhy): its why as the core says it.
    Host { why: HostWhy },
    /// 'the party could not be made: ' + the relay's reason.
    PartyMakeFailed {
        #[serde(rename = "text")]
        why: NetFail,
    },
    /// 'the party could not be made: ' + why its code cannot be written.
    PartyMakeCodeFailed { why: CodeMakeWhy },
    /// 'the party has no leader'.
    NoLeader,
    /// 'the party could not be joined: ' + its why.
    PartyJoinFailed { why: PartyJoinWhy },
    /// 'the party leader did not answer'.
    LeaderSilent,
    /// 'the party leader left'.
    LeaderLeft,
    /// 'the link to the party leader was lost'.
    LeaderLost,
    /// A peer's bye text that is none of these.
    Other { text: String },
    /// 'the party ended': the party's lobby closed under its member.
    PartyEnded,
    /// 'the party ended: ' + the why of the leader's bye (its ClosedWhy on the wire; anyone's code until worded).
    PartyEndedBy { why: Box<ClosedWhy> },
    /// 'the party lobby closed: ' + the backend's why (the leader's own lobby).
    PartyLobbyClosed { text: String },
    /// 'the queue moved on': a queue session let its place go (its members search again).
    QueueMoved,
    /// 'the queue found a match without you': a queue session's found match has no place for this member.
    QueueUnmatched,
    /// 'the lobby could not be made: the relay's address is not in the online config'.
    MakeNoRelay,
    /// 'the party could not be made: the relay's address is not in the online config'.
    PartyMakeNoRelay,
    /// The backend answered a lobby's make or join with another kind of answer (its asks are typed).
    BadAnswer,
}

/// Why a join by a code failed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum JoinWhy {
    /// The code does not read (codes.rs decode_lobby_code's why).
    BadCode { why: crate::codes::CodeWhy },
    /// 'direct play is not set up in this app'.
    NoDirect,
    /// 'the lobby is on Epic Online Services, and this app is not signed in to Epic (<why>)'.
    EpicOff { why: OffWhy },
    /// "its code names the release's relay, and this app has none".
    NoDefaultRelay,
    /// (the port's own) A short code: its lobby is on the release's relay, and this app signs in to another one.
    ShortOtherRelay,
    /// 'its relay (<address>) is not connected: <why>'.
    RelayOff { address: String, why: OffWhy },
    /// 'a link to another relay is not set up in this app'.
    NoRelayLink,
    /// 'the relay at <address> was not taken': the player said no to the relay question.
    RelayNotTaken { address: String },
    /// 'the relay at <address> did not take the link: ' + its reason.
    RelayRefused { address: String, text: String },
    /// 'the lobby is gone (its host closed it, or the code is old)': the backend found no such lobby.
    Gone,
    /// 'the lobby is full'.
    Full,
    /// The join by a direct code failed (lan/direct.rs's code: unreadable, no key, refused, unreachable, ...).
    Direct { why: crate::lan::direct::DirectJoinWhy },
    /// The backend's reason for the join (another wire version, a relay that did not answer, ...).
    Backend { text: String },
}

/// Why a party join failed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum PartyJoinWhy {
    /// 'no party invite had that code'.
    NoInvite,
    /// 'its code names no relay lobby'.
    NotRelay,
    /// "its relay (<address, or 'the default'>) is not this app's".
    OtherRelay { address: Option<String> },
    /// 'the relay is not connected: <why>'.
    RelayOff { why: OffWhy },
    /// 'its leader is not who invited this player'.
    NotInviter,
    /// The code does not read (codes.rs decode_lobby_code's why).
    BadCode { why: crate::codes::CodeWhy },
    /// The backend's reason.
    Backend { text: String },
}

/// A question the session needs the player's yes to before it goes on (NetEvent 'ask'), worded by the UI with its
/// yes and no.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum AskWhat {
    /// The ask before joining a relay not played on: 'This lobby is on the relay at <address>, which you have not
    /// played on before. Join it through that relay?', yes 'Join', no 'Cancel'.
    NewRelay { address: String },
    /// A code's relay the player said yes to proved another key than the one it proved then: join it all the same
    /// (and expect the new key from now on)?
    RelayKeyChanged { address: String },
}

/// What an invite asks to join (NetEvent 'invited' kind).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InviteKind {
    /// Another player's session: joined with `Open(Join(code))`.
    Custom,
    /// Their party: joined with `PartyJoin { code }`.
    Party,
}

/// The net service's messages to a page.
#[derive(Clone, Debug, PartialEq)]
pub enum NetEvent {
    /// The sign-in, on the port's open and on each change: `me` this player's id ('' while signed out).
    Status { online: bool, me: String, why: StatusWhy },
    /// The session is open: `code` its lobby code (shown to the players and pasted to join; '' a local lobby's); `me`
    /// this player's id in it; `chosen` it is on a backend of the status (false: a direct code's join, on a link of
    /// its own, and a local lobby); `on` what it is connected through. Once per session: a relay-hosted one's waits
    /// for its core's first message, so a relay of another version is passed over before the page hears of it.
    Opened { code: String, me: String, chosen: bool, on: SessionOn },
    /// A message from the session's host core: a JSON text (mvs_protocol::messages::ServerMessage).
    Msg(String),
    /// The session is over for this page.
    Closed(ClosedWhy),
    /// Match packets from the match's referee (`from` its slot in match.go's `peers`, -1 when it plays none), in the
    /// order they came.
    Packets { from: i32, packets: Vec<Vec<u8>> },
    /// An invite from `from_id` (`from_name` the name it sent).
    Invited { kind: InviteKind, code: String, from_id: String, from_name: String },
    /// The answer to `Who` `id`: the listed players (the asker's own left out), or why none.
    Who { id: u32, players: Vec<Who>, why: BrowseWhy },
    /// The page's party on each change; None: none (`why` the reason it ended, `Left` when the player left it).
    Party { party: Option<PartyView>, why: ClosedWhy },
    /// Another member's PartySay.
    PartySay { from: String, say: PartySay },
    /// Another member's main-menu fighter, at its first card that names one and at each change (None: its card names
    /// none now). A member whose game sends none (an older one) is never told.
    PartyFighter { from: String, fighter: Option<MenuFighter> },
    /// This page's invite to `to` was not sent: past the service's own pace, or the backend refused it.
    InviteRefused { to: String },
    /// The answer to `Browse` `id`.
    Browsed { id: u32, lobbies: Vec<BrowsedLobby>, why: BrowseWhy },
    /// A question the session needs the player's yes to; answered by `Answer` with this `id`; a session's end drops it.
    Ask { id: u32, what: AskWhat },
    /// A link of the session to another player stands (Epic's alone, once per link: `relayed` through Epic's relays,
    /// else direct), or (None) is closed.
    Link { peer: String, relayed: Option<bool> },
    /// The way to the relay changed: `slow` while it is signed in and reached by TCP alone (its UDP link off), which
    /// plays slower; told on each change, and at a page's open while slow.
    Way { slow: bool },
}

/// Direct play's state: this PC's addresses a code would name (best first; none: on no
/// network), the setting's port, each hosted lobby's listener as `address:port`, and why the last hosted lobby could
/// not listen (None: it could, or none was tried).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectStatus {
    pub addresses: Vec<String>,
    pub port: u16,
    pub listening: Vec<String>,
    pub why: Option<HostStartWhy>,
}

/// A relay to sign in to.
#[derive(Clone, Debug, PartialEq)]
pub struct RelayOptions {
    /// `host[:port]` (relay_wire::parse_address).
    pub address: String,
    /// This install's Ed25519 key (made on the first use, kept in the data folder).
    pub key_file: PathBuf,
    /// The product name and version the relay is told.
    pub product: String,
    pub version: String,
    /// The key a direct host's code names (the first 8 bytes of its SHA-256, hex): a relay that proves another key
    /// is refused. None for a relay chosen by its address.
    pub host_key: Option<String>,
    /// The key id the relay must prove (`OffWhy::RelayKey` when it proves another); None: any key.
    pub pin: Option<RelayPin>,
    /// (dev) the faults its client runs under.
    pub faults: NetFaults,
}

/// A relay's expected key: its id (`relay_wire::key_id`), and whether it is the release's (online.json's relayId) or
/// one the player said yes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayPin {
    pub id: String,
    pub release: bool,
}

/// This install's way to a direct host, whatever the setting, and to hosting one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectOptions {
    pub key_file: PathBuf,
    pub product: String,
    pub version: String,
    /// A hosted lobby listens on 127.0.0.1 alone, codes may name 127.x, and LAN lobbies are announced to
    /// 127.255.255.255 and heard on 127.0.0.1 alone (the two-app tests; every run of the port's tests).
    pub loopback: bool,
}

/// Epic's part of the online config: its options, None with the SDK or the credentials missing (`missing` why).
#[derive(Clone, Debug, PartialEq)]
pub struct EpicBackend {
    pub options: Option<EosOptions>,
    pub missing: Option<EpicMissing>,
}

/// The LAN host's options: the direct options and its port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanOptions {
    pub direct: DirectOptions,
    pub port: u16,
}

/// Every backend of the online config, None where it is off, given at the service's
/// start and on each change (it signs in to what changed and leaves the rest as it is).
#[derive(Clone, Debug, PartialEq)]
pub struct OnlineBackends {
    pub relay: Option<RelayOptions>,
    pub epic: Option<EpicBackend>,
    pub lan: Option<LanOptions>,
    /// Friends may connect directly (EOS relay control).
    pub allow_direct: bool,
    /// The release's default relay (online.json as shipped): a relay code of it has the short form (codes.rs).
    pub default_relay: Option<String>,
    /// The relays this player has said yes to once (a code for another asks first, `NetEvent::Ask`), by address, with
    /// the key id each proved.
    pub known_relays: Vec<KnownRelay>,
}

/// One backend's sign-in, for the launcher's status lines and the pages: `on` it is in
/// the config, `online` signed in (LAN: listening), `why` the reason it is not (None when it is).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendState {
    pub kind: BackendKind,
    pub on: bool,
    pub online: bool,
    pub why: Option<OffWhy>,
}

/// What the game's net service tells the launcher: every backend's sign-in and the LAN host's
/// state. The service writes it to [`NET_STATE_FILE`] in the data folder on each change (`ServiceOptions::state_file`)
/// and removes it at its quit; the launcher reads it while the game it started runs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetState {
    pub backends: Vec<BackendState>,
    pub direct: Option<DirectStatus>,
    /// The relay address the service signs into (its config's), for the launcher's "connected at" line.
    #[serde(default)]
    pub relay: Option<String>,
    /// The signed-in relay's UDP link (None: no relay signed in, or not known yet).
    #[serde(default)]
    pub udp: Option<bool>,
    /// A change of the backends the player made waits for the open custom lobby or queue to close
    /// (`NetHandle::backends_when_free`).
    #[serde(default)]
    pub held: bool,
}

/// The net state's file in the data folder.
pub const NET_STATE_FILE: &str = "net-state.json";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn codes_have_their_wire_form() {
        let w = ClosedWhy::JoinFailed { why: JoinWhy::RelayOff { address: "a:1".into(), why: OffWhy::NotOn } };
        let v = serde_json::to_value(&w).unwrap();
        assert_eq!(
            v,
            json!({ "code": "join_failed", "why": { "code": "relay_off", "address": "a:1", "why": { "code": "not_on" } } })
        );
        assert_eq!(serde_json::from_value::<ClosedWhy>(v).unwrap(), w);
        let say = PartySay::Open { modes: vec![QueueMode::OneVOne] };
        assert_eq!(serde_json::to_value(&say).unwrap(), json!({ "k": "open", "modes": ["1v1"] }));
        assert_eq!(serde_json::to_value(PartySay::Ready).unwrap(), json!({ "k": "ready" }));
        let off = BackendOff { kind: BackendKind::Lan, why: OffWhy::Retrying { text: "x".into(), in_ms: 5000 } };
        assert_eq!(
            serde_json::to_value(&off).unwrap(),
            json!({ "kind": "lan", "why": { "code": "retrying", "text": "x", "in_ms": 5000 } })
        );
    }

    /// A relay-hosted core's bye is a hosting's: the JSON of ClosedWhy::Host.
    #[test]
    fn a_host_bye_is_closed_why_host() {
        use mvs_protocol::p2p_frame::{FRAME_BYE, host_bye, signal};
        for why in [HostWhy::Kicked, HostWhy::Owner { text: "x".into() }, HostWhy::LinkError] {
            let text = serde_json::to_string(&ClosedWhy::Host { why: why.clone() }).unwrap();
            assert_eq!(host_bye(&why), signal(FRAME_BYE, text.as_bytes()), "{why:?}");
        }
    }
}
