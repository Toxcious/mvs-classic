//! What the menus see of online play and ask of it (web/src/net/online.ts Online, its getters and its listeners): the
//! program's online client (mvs-app online) fills `Session.online` before each frame and runs the screens'
//! `OnlineCmd`s after it (`Cx::online`, `MenuView::take_online_cmds`), as the scene commands go.
//!
//! Deviation: the TS's listeners (onChanged, onInvited, onNotice, onClosed, ...) are this frame's `events`, and its
//! promises (session, browse) commands whose answers come in a later frame's view.

use mvs_net::backend::BackendKind;
use mvs_net::service::api::{AskWhat, BrowseWhy, ClosedWhy, InviteKind, PartySay, PartyView, SessionTarget};
use mvs_protocol::lobby_listing::LobbyListing;
use mvs_protocol::lobby_rules::LobbyData;
use mvs_protocol::messages::{ClientMessage, PmEndWhy, ServerMessage};
use mvs_protocol::online::{PlayerCosmetics, QueueMode};
pub use mvs_protocol::relay_wire::PresenceStatus;

use crate::menu::screens::pre_match::setup::PreMatchPlayer;

/// A player the social panels and the invite list show (online.ts OnlinePlayer).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OnlinePlayer {
    pub id: String,
    pub name: String,
    /// In a match, a queue or a pre-match, as their presence says (false: not known).
    pub busy: bool,
    /// Their equipped profile icon's ProfileIconData asset name ('' none sent).
    pub icon: String,
    /// Their presence is known: they are on the relay's online list now.
    pub present: bool,
    /// (the port's own) What they are doing, as their presence says (None: not known, or a client of before it).
    pub status: Option<PresenceStatus>,
}

/// An invitation (online.ts LobbyInvite): Custom to a custom game lobby, `lobby` its code; Party to the sender's
/// party, `lobby` the party's code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyInvite {
    pub kind: InviteKind,
    pub lobby: String,
    pub from_id: String,
    pub from_name: String,
}

/// A public custom lobby the browser shows (online.ts BrowsedGame): its code (joined as a pasted one is), the
/// backend it was found on (None: not said), its members of its max, its listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowsedGame {
    pub code: String,
    pub backend: Option<BackendKind>,
    pub members: u32,
    pub max: u32,
    pub listing: LobbyListing,
}

/// Why a browse or the online list has none, or not all (online.ts BrowseAnswer `why` and `text`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowseNote {
    /// The service's answer.
    Net(BrowseWhy),
    /// 'noApp': no net service (CUSTOM_TEXT.onlineListNoApp).
    NoApp,
    /// Not answered by BROWSE_MS (CUSTOM_TEXT.onlineListNoAnswer).
    NoAnswer,
}

/// A browse's answer: the lobbies whose listing could be read, and why not all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowseAnswer {
    pub games: Vec<BrowsedGame>,
    pub why: BrowseNote,
}

/// A question of the session the player answers (online.ts SessionAsk): `OnlineCmd::Answer` with its id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionAsk {
    pub id: u32,
    pub what: AskWhat,
}

/// The page's open session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionView {
    /// Its lobby code ('' none).
    pub code: String,
    /// This player's id in it (NetClient.me).
    pub me: String,
    /// This player hosts it.
    pub host: bool,
    /// It is on a backend of the sign-in (NetEvent::Opened `chosen`; false: a direct code's join, on a link of its
    /// own, which online going off does not end).
    pub chosen: bool,
    /// The custom lobby as its last lobby.state gave it (None: none yet, or a queue session).
    pub lobby: Option<LobbyData>,
    /// The session is the queue's (online.ts queued).
    pub queued: bool,
}

/// A note the game shows (online.ts tell and hostNotice).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnlineNotice {
    /// A host core's notice: anyone's words, cleaned and paced (NOTICE_LIMIT, NOTICE_EVERY_MS).
    Host(String),
    /// PROTOCOL_NOTE: 'This server runs another version of the game's online protocol. Update the game, or pick
    /// another server'.
    ProtocolDiffers,
    /// An invite not sent (the service's InviteRefused), once per NOTICE_EVERY_MS.
    InviteNotSent { to: String },
    /// (the port's own) A match went out of sync: its report was written to `folder` (DESYNC_NOTE, its OPEN_FOLDER
    /// button opens it through MenuSink::open_folder).
    Desync { folder: String },
    /// PGI OpenPartyDisbandedNotification (@49359): a full party ended under this player, not by its own leave.
    PartyDisbanded,
}

/// (the port's own) The menus' one line of online play: online, online by a slower way (the relay reached by TCP
/// alone), or offline with why. No backend is named in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnlineLine {
    Online,
    Slower,
    Offline(OfflineWhy),
}

impl Default for OnlineLine {
    /// The line before the net service's first status (NetPort's start: signing in).
    fn default() -> Self {
        OnlineLine::Offline(OfflineWhy::SigningIn)
    }
}

/// Why online play is off, as the line says it (a headline and what to do, custom_text's LINE_* words).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfflineWhy {
    /// This copy is being checked before it meets anyone (the release feed asked for a newer version, the check
    /// match run).
    Checking,
    SigningIn,
    /// A newer release is out: online play waits for the update.
    Update {
        version: String,
    },
    /// The imported game files are older than this version reads: online play waits for the import.
    Import,
    /// This PC's run of the fixed check match differs from the release's: its matches would go out of sync.
    SelfCheck,
    /// Every way online is turned off in the launcher's settings.
    TurnedOff,
    /// The server was not reached (no network, or it is down).
    NotReached,
    Other,
}

/// What happened online this frame, in order (the TS's listeners).
#[derive(Clone, Debug, PartialEq)]
pub enum OnlineEvent {
    /// A message of the session's host core (each `c.on(..)` of the screens).
    Server(Box<ServerMessage>),
    /// The session is open (onSession).
    Opened,
    /// The session is over (onClosed).
    Closed(ClosedWhy),
    /// The host core refused the name (onNameTaken): `refused` for a word in it, else someone has it.
    NameTaken {
        refused: bool,
    },
    /// An invitation (onInvited; none with Invites off).
    Invited(LobbyInvite),
    /// A toast of another player (onToast).
    Toast {
        from: String,
        name: String,
    },
    Notice(OnlineNotice),
    /// Another party member's PartySay.
    PartySay {
        from: String,
        say: PartySay,
    },
    /// The online pre-match's news for the screen.
    PreMatch(PreMatchEvent),
}

/// What the menus see of online play this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnlineView {
    /// Online play's clock, ms: it runs on through the matches and is never set back, where a menu's own clock
    /// starts again with each menu. What lasts longer than a menu (a lobby kept across its matches) is timed by it.
    pub now: f64,
    /// Signed in to a backend: sessions can be opened.
    pub connected: bool,
    /// The one line of online play the menus show.
    pub line: OnlineLine,
    /// A lobby's code can be tried (the net service is there, whatever its sign-in).
    pub can_join: bool,
    /// This player's id as the host cores know it ('' while signed out).
    pub me: String,
    /// The chosen name ('' none yet) and the equipped profile icon ('' none).
    pub name: String,
    pub icon: String,
    /// The host core refused the name (`refused`: for a word in it).
    pub name_taken: bool,
    pub name_refused: bool,
    /// Everyone met recently, newest first.
    pub players: Vec<OnlinePlayer>,
    /// The online list (this player left out), and why it is empty when it could not be asked (None: it was).
    pub listed: Vec<OnlinePlayer>,
    pub listed_why: Option<BrowseNote>,
    pub session: Option<SessionView>,
    pub party: Option<PartyView>,
    /// The last browse's answer (None: none asked, or not answered yet).
    pub browse: Option<BrowseAnswer>,
    /// The question the session waits on.
    pub ask: Option<SessionAsk>,
    /// The online pre-match or the queue's search under way (None: none).
    pub pre_match: Option<Box<PreMatchView>>,
    /// A queue match's set as its end heard it (None: no queue match since the last set's end).
    pub queue_end: Option<QueueEndView>,
    pub events: Vec<OnlineEvent>,
    /// The game data's compatibility id a lobby join carries ('' none).
    pub compat: String,
}

/// A party command (the service's party requests).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartyCmd {
    Invite { to: String },
    Join { code: String },
    Leave,
    Say(PartySay),
}

/// What a screen asks of online play, run by the program after the frame.
#[derive(Clone, Debug, PartialEq)]
pub enum OnlineCmd {
    /// Opens the page's session (the one open is left first) and greets it with the chosen name.
    Open(SessionTarget),
    /// A message to the session's host core.
    Send(Box<ClientMessage>),
    /// Leaves the session.
    Close,
    /// Invites a player to the open session.
    Invite {
        to: String,
    },
    /// Searches the public custom lobbies (the answer: a later frame's `browse`).
    Browse,
    /// Asks for the online list again (at most once per WHO_AGAIN_MS).
    Who,
    /// The player's answer to the session's question.
    Answer {
        id: u32,
        yes: bool,
    },
    /// A new name, asked of the open session's host core (identity.check).
    SetName(String),
    /// The equipped profile icon (setIcon).
    SetIcon(String),
    /// A toast to a player of the session (sendToast).
    Toast {
        to: String,
    },
    Party(PartyCmd),
    /// A chat line to the lobby or the match (`all`: every team), under the chat's gate.
    Chat {
        text: String,
        all: bool,
    },
    /// The queue's pre-match screen on these modes (EnterPreMatch on the queue's path: match-options.ts Confirm,
    /// PLAY AGAIN, a party leader's 'open' followed): the program opens the PreMatch level with its QueueSetup.
    Queue {
        modes: Vec<QueueMode>,
    },
    /// The online pre-match screen's hooks.
    PreMatch(PreMatchCmd),
    /// A queue match's end: the rematch vote (once, the first counts), and out of the set (PLAY AGAIN's
    /// ResetMatchFlow).
    Vote(bool),
    LeaveSet,
}

// ---------- a queue match's end (net/queue-end.ts) ----------

/// A queue set's game decided (match.set): `wins` by team, `game` the games played; `rematch` every slot's yes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetOutcome {
    pub rematch: bool,
    pub wins: Vec<u32>,
    pub game: u32,
}

/// What a queue match's end heard so far (net/queue-end.ts QueueEndSession): what came while no step listened is
/// here for a step that binds late.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueueEndView {
    /// Each vote by slot as it came (match.votes), until the set's outcome.
    pub votes: Vec<(u32, bool)>,
    pub set: Option<SetOutcome>,
    /// The rematch's pre-match began (the program opens the PreMatch level on it).
    pub rematch_begun: bool,
    /// The set ended in the rematch's wait: a human left (`Left`, the leaver's name, '' none), an error, the
    /// connection's loss, or no pm.begin in time.
    pub ended: Option<(PmEndWhy, String)>,
}

// ---------- the online pre-match screen (app/custom-pre-match.ts, app/queue.ts) ----------

/// A pick as the online pre-match carries it (net/pre-match.ts NetPick): the fighter's roster key and the skin's
/// SkinData name ('' the default).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetPick {
    pub character: String,
    pub skin: String,
}

/// A party page's role in the queue's pre-match (setup.ts PreMatchSetup.party).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartyRole {
    Leader,
    Member,
}

/// The screen entered again from the Lab (setup.ts PreMatchSetup.resume, state 0's restore path PM@4610): the
/// search's start on the menus' clock (s) and the lock the queue holds.
#[derive(Clone, Debug, PartialEq)]
pub struct QueueResume {
    pub start: f64,
    pub pick: Option<NetPick>,
}

/// The queue's part of a pre-match screen's setup (setup.ts PreMatchSetup.queue / party / resume).
#[derive(Clone, Debug, PartialEq)]
pub struct QueueSetup {
    /// The queues the search waits in (the multiqueue).
    pub modes: Vec<QueueMode>,
    pub party: Option<PartyRole>,
    pub resume: Option<QueueResume>,
}

/// What makes a pre-match screen an online one (setup.ts PreMatchSetup.online and .queue): the program fills it when
/// it opens the screen on an online pre-match (a custom lobby's start, a queue set's rematch: `queue` None) or on the
/// queue's path (`queue` set).
#[derive(Clone, Debug, PartialEq)]
pub struct PreMatchNet {
    pub queue: Option<QueueSetup>,
}

/// The found match's players as the screen takes them (app/queue.ts registered): Handle All Players Registered's
/// players, this page's slot, the map, the team style, and the perks once match.start came (None: not yet).
#[derive(Clone, Debug, PartialEq)]
pub struct Registered {
    pub players: Vec<PreMatchPlayer>,
    pub me: usize,
    pub map: String,
    pub team_style: u8,
    pub perks: Option<Vec<Vec<String>>>,
}

/// The note a pre-match screen kicks back to the main menu with (BPI_KickBackToMainMenu), worded by the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KickNote {
    /// custom-pre-match.ts ERROR_TEXT, UI_PreMatch's error PM@101902: 'An error has occurred. Please matchmake
    /// again.'
    Error,
    /// LEFT_TEXT PM@51907: 'A player has disconnected.'
    Left,
    /// LEFT_NAMED, OnMatchAbandonedByPlayer MH@34468: '{username} has disconnected'.
    LeftNamed(String),
    /// COMPAT_MISSING_NOTE (protocol/online.ts): the join carried no compatibility id.
    CompatMissing,
    /// CUSTOM_TEXT.partyRefused: the server refused the party's join.
    PartyRefused,
    /// CUSTOM_TEXT.partySearchEnded: the party's ticket ended under this page.
    PartySearchEnded,
    /// CUSTOM_TEXT.partyLeft with '{username}' the other member: they left the party's search.
    PartyLeft(String),
}

/// What the online pre-match tells the screen (the TS's preMatchSession.live calls), in order; account ids are the
/// screen's (`PreMatchPlayer::account_id`).
#[derive(Clone, Debug, PartialEq)]
pub enum PreMatchEvent {
    /// A team-mate's previewed fighter (playerPreviewed: OnPlayerPreviewedCharacter / Skin).
    Previewed { account: String, pick: NetPick },
    /// A player's pre-match cosmetics (playerCosmetics), with its lock or at the reveal.
    Cosmetics { account: String, cosmetics: PlayerCosmetics },
    /// A team-mate's lock (playerLocked: OnPlayerLockedCharacter).
    Locked { account: String, pick: NetPick },
    /// Every slot locked, the reveal (onlinePlayersReady: OnPlayersReadyToMatchmake): the picks by slot, the map.
    PlayersReady { picks: Vec<NetPick>, map: String },
    /// A human's perks are in (playerReadyPerks: OnPlayerLockedPerks).
    ReadyPerks { account: String },
    /// match.start (allPlayersLockedPerks: OnAllPlayersLockedPerks): every slot's four perks ('' empty).
    AllLockedPerks { perks: Vec<Vec<String>> },
    /// Out to the main menu with the note (kickBackToMainMenu: OnPlayerAbandonedPreMatch, the queue's errors).
    Kick(KickNote),
    /// The queue's join was taken (onMatchmakingStarted: OnMatchmakingStarted).
    MatchmakingStarted,
    /// The search was cancelled (onMatchmakingCancel: OnMatchmakingCancel PM@81099).
    MatchmakingCancel,
    /// The queue found the match (onAllPlayersRegistered: OnAllPlayersRegisgeredForMatch).
    AllRegistered(Box<Registered>),
    /// A party's search started on the leader's page (partyStarted: state 11).
    PartyStarted,
    /// The other member stopped the party's search (partyStopped: back to state 4, the lock kept).
    PartyStopped,
}

/// What the online pre-match shows the screen each frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PreMatchView {
    /// bIsMatchmaking (isMatchmaking): from StartMatchmaking's join to the search's end.
    pub searching: bool,
    /// The found match's players (registeredPlayers; None: none found yet).
    pub registered: Option<Box<Registered>>,
}

/// What the online pre-match screen asks of the program (the TS PreMatchHooks of its online and queue paths).
#[derive(Clone, Debug, PartialEq)]
pub enum PreMatchCmd {
    /// The screen has its players: what the session kept comes now, in order (screenUp).
    ScreenUp,
    /// HandleCharacterFocused / HandleSkinFocused (localPreview): the server's preview, debounced by the program.
    LocalPreview(NetPick),
    /// HandleCharacterConfirmed, and the timer's HandleTimerExpired (localLocked).
    LocalLocked(NetPick),
    /// The pick taken back (localUnlocked: state 4's Back, the cancel of state 5).
    LocalUnlocked,
    /// The four slots ('' an empty one: HandlePerkSelected PM@31031).
    PerksLocked(Vec<String>),
    /// OnPlayersReadyToMatchmake's StartMatchmaking PM@68200.
    StartMatchmaking,
    /// Toggle Matchmaking(false), a member's AskLeaderToCancelMatchmaking PM@80191.
    CancelMatchmaking,
    /// The leader's Back to Lobby (BPI_TransitionToMainMenu PM@111338).
    BackToLobby,
    /// The screen's own kicks (the 15 s waits): out of the pre-match.
    Kick,
    /// BPI_CleanUpAndStartMatch: this page's pick (Save Chosen Characters).
    StartMatch(NetPick),
}
