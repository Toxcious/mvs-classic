//! The words of the port's own menu features, the ones the beta has no line for (web/src/custom/text.ts): the custom
//! lobby's experimental settings and the rules they turn on. English only; the beta's own text comes from the
//! player's Game.locres. The HUD's lines are `hud::custom_text`.

pub const EXPERIMENTAL_BUTTON: &str = "EXPERIMENTAL SETTINGS";
pub const EXPERIMENTAL_TITLE: &str = "Experimental";
pub const STOCK_MODE: &str = "Stock Mode";
pub const OFF: &str = "Off";
pub const ON: &str = "On";
pub const MUTATOR: &str = "Mutators";
/// The mutators row's label with more than one ticked.
pub fn mutator_count(n: usize) -> String {
    format!("{n} Mutators")
}
/// The mutators row's values, in the order of mvs_protocol::lobby_rules::LOBBY_MUTATORS: Off clears the list.
pub const MUTATORS: [&str; 18] = [
    "Off",
    "Silly (Random)",
    "Big Head",
    "Grow",
    "Giant",
    "Miniature",
    "Low Gravity",
    "Instant Kill",
    "Grey Health",
    "Unlimited Jumps",
    "Faster Cooldowns",
    "Attacks Break Armor",
    "Extra Air Special",
    "Weaken on Hit",
    "Charge Freeze",
    "Ice Gauntlets",
    "Random Items",
    "Final Point Shrink",
];
/// The lobby's ring-out drop under the stock rule.
pub const STOCKS: &str = "Stocks";
/// The custom game browser's tag on a row: the backend the lobby was found on.
pub const BACKEND_RELAY: &str = "Relay";
pub const BACKEND_EPIC: &str = "Epic";
pub const BACKEND_LAN: &str = "LAN";
pub const INVITED_ONLY: &str = "Invited Players Only";
pub const INVITED_ONLY_REFUSED: &str = "This lobby lets in invited players only. Ask a player in it to invite you.";
pub const INVITE_REFUSED: &str = "The invite was not sent: too many invites at once. Wait a moment and invite again.";
pub const LIMITED: &str = "Too many requests to the server. Wait a minute and try again.";
/// The Graphics page's Max Framerate row (the build has no frame rate limit row): its label, the item of no limit
/// (after the display's rate when it is known) and its tooltip.
pub const FRAME_RATE_LABEL: &str = "Max Framerate";
pub const FRAME_RATE_NONE: &str = "Display Rate";
pub const FRAME_RATE_UNLIMITED: &str = "Unlimited";
pub const FRAME_RATE_TOOLTIP: &str = "Set the most frames drawn per second. Display Rate draws one for every refresh \
     of your display. A limit above your display's refresh, or Unlimited, does not wait for the display and may tear.";

/// The name of your own record in a match without usernames (the command-line match; the oracle's harness names it
/// so).
pub const LOCAL_PLAYER_NAME: &str = "Player";
/// A bot's level tag on the pre-match cards, in place of the build's "LVL {number}".
pub const BOT_TAG: &str = "BOT";
/// The profile screen's rename (pencil) button's tooltip (the beta has no rename route there: widgets/profile.rs).
pub const CHANGE_NAME: &str = "CHANGE NAME";
/// The home screen's background switch (widgets/home.rs): its tip, before the shown background's name (the names
/// are the player's Game.locres lines).
pub const MENU_BACKGROUND: &str = "Menu Background";
/// The Expert bot, the port's own level above Hard: its entry in the Lab's bot menu and the title of the Arcade's
/// Expert card.
pub const EXPERT: &str = "Expert";
/// Its entry in the custom game's bot difficulty menu (the build's rows read "Very Easy Difficulty" .. "Hard
/// Difficulty").
pub const EXPERT_DIFFICULTY: &str = "Expert Difficulty";
/// The pre-match header of an Expert Arcade run (the build's lines read "Arcade - Easy" .. "Arcade - Hard").
pub const ARCADE_EXPERT: &str = "Arcade - Expert";

// ---------- Waves (menu/waves.rs): the mode's tab, its cards and its result page ----------

/// Match Options' tab of the mode.
pub const WAVES: &str = "WAVES";
/// The variants: the cards' titles and the result page's.
pub const WAVES_100: &str = "100 Ring-Outs";
pub const WAVES_ENDLESS: &str = "Endless";
/// The cards' line under the title with no record yet.
pub const WAVES_100_HINT: &str = "Ring out 100 enemies, fast";
pub const WAVES_ENDLESS_HINT: &str = "One life against endless enemies";
/// The cards' line with a record: the best of every fighter, and whose it is.
pub fn waves_card_best(best: &str, fighter: &str) -> String {
    format!("Best: {best} ({fighter})")
}
/// A run's time as the pages show it: minutes, seconds and hundredths, as the match's clock shows it.
pub fn waves_time(seconds: f64) -> String {
    crate::hud::custom_text::waves_clock(seconds, true)
}
/// A count of ring-outs.
pub fn waves_ring_outs(n: u32) -> String {
    if n == 1 { "1 Ring-Out".to_string() } else { format!("{n} Ring-Outs") }
}
/// The result page's title, before the variant.
pub const WAVES_RESULT_TITLE: &str = "Waves";
/// The result page's lines.
pub const WAVES_CLEARED: &str = "Cleared in";
pub const WAVES_FELL: &str = "Rung out at";
pub const WAVES_NEW_RECORD: &str = "NEW RECORD";
pub fn waves_best(fighter: &str, best: &str) -> String {
    format!("{fighter}'s best: {best}")
}
/// The best line with none: 100 Ring-Outs with no cleared run, Endless with no ring-out.
pub const WAVES_NO_BEST: &str = "No cleared run yet";
pub const WAVES_NO_RING_OUTS: &str = "No ring-outs yet";
/// The result page's buttons.
pub const WAVES_PLAY_AGAIN: &str = "Play Again";
pub const WAVES_BACK: &str = "Back";

// ---------- online: the words of the codes the net service sends (online_text.rs maps a code to one) ----------

/// The Settings panel's section of the port's own settings: its tab on the section bar and its page's title.
pub const CLASSIC_SECTION: &str = "CLASSIC";
pub const CLASSIC_TITLE: &str = "Classic";
/// The Classic page's rows (Settings > Classic): chat in online matches, the online list and party invites.
pub const CHAT_SETTING: &str = "Chat in Match";
pub const CHAT_SETTING_TOOLTIP: &str = "Show the chat of online matches: the messages at the players' portraits, and the chat box on Enter (team) or Shift+Enter (all).";
pub const ONLINE_LIST_SETTING: &str = "Show Me in the Online List";
pub const ONLINE_LIST_SETTING_TOOLTIP: &str =
    "List you in other players' Online list (your name, icon and whether you are in a match), so they can invite you.";
pub const INVITES_SETTING: &str = "Invites";
pub const INVITES_SETTING_TOOLTIP: &str =
    "Let other players invite you to their party or their custom game. Off: their invites do not reach you.";
/// The social panel's list header: the relay's online players (the build's line there reads online WB friends).
pub const ONLINE_LIST_HEADER: &str = "Online Players";
pub const ONLINE_LIST_NO_APP: &str = "The online list needs the app.";
pub const ONLINE_LIST_NO_ANSWER: &str = "The online list could not be fetched. Try again in a moment.";
/// The custom game browser's answers with no net service and with no answer in time.
pub const BROWSE_NO_APP: &str = "online play needs the app";
pub const BROWSE_NO_ANSWER: &str = "the online process did not answer";
/// The custom game browser's line when a search found no lobby and has no reason to give (the port's own line, in place of the build's plain "None").
pub const BROWSE_EMPTY: &str = "No custom games found.";
/// A party's queue: the main menu's note when the other member's page left the search, Match Options' Confirm on a
/// member's page, and the notes of a party's join or search that failed.
pub const PARTY_LEFT: &str = "{username} left the queue.";
pub const PARTY_LEADER_QUEUES: &str = "The Party Leader Picks the Mode";
pub const PARTY_REFUSED: &str = "Your party could not join this queue. Please matchmake again.";
pub const PARTY_SEARCH_ENDED: &str = "Your party's search ended. Please matchmake again.";
/// The match chat's channel on the input and the log's tags.
pub const CHAT_TO_TEAM: &str = "To (TEAM):";
pub const CHAT_TO_ALL: &str = "To (ALL):";
pub const CHAT_TAG_TEAM: &str = "[TEAM]";
pub const CHAT_TAG_ALL: &str = "[ALL]";
/// The names of the backends in a reason.
pub const BACKEND_NAME_RELAY: &str = "the relay";
pub const BACKEND_NAME_EPIC: &str = "Epic";
pub const BACKEND_NAME_LAN: &str = "LAN";
/// A backend's reasons for being off (sessions.ts whyOff, signin.ts).
pub const OFF_WHY_OFF: &str = "off";
pub const OFF_WHY_NOT_ON: &str = "{backend} is not turned on in the launcher";
pub const OFF_WHY_SIGNING_IN: &str = "signing in to Epic Online Services";
pub const OFF_WHY_CONNECTING: &str = "connecting to the relay at {address}";
pub const OFF_WHY_SIGN_IN_CHANGED: &str = "the sign-in to {backend} changed";
pub const OFF_WHY_LAN_FAILED: &str = "LAN did not start";
pub const OFF_WHY_NOT_INSTALLED: &str = "the online files are missing from this install";
pub const OFF_WHY_NOT_SET_UP: &str = "not set up in this dev run (MVS_EOS_DLL and MVS_EOS_CREDENTIALS are not set)";
pub const OFF_WHY_RETRYING: &str = "{text} (trying again in {in})";
pub const OFF_WHY_SETTINGS_CHANGED: &str = "the online settings changed";
pub const OFF_WHY_TURNED_OFF: &str = "turned off in the online settings";
pub const OFF_WHY_LAN_PORT: &str = "the LAN port changed";
pub const OFF_WHY_LAN_SETTING: &str = "the LAN setting changed";
/// The relay proved another key than the one expected (OffWhy::RelayKey): the release's own relay, a relay the
/// player chose.
pub const OFF_WHY_RELAY_KEY_RELEASE: &str = "the relay did not prove the key this version of the game expects, so it may not be the game's server (trying again in {in})";
pub const OFF_WHY_RELAY_KEY_KNOWN: &str = "the relay proved another key than when you last played on it, so it may not be the same server; if its owner changed it, trust the new key in the launcher's Online settings, then restart the game (trying again in {in})";
/// Why a text is not a lobby code (codes.ts, direct-code.ts).
pub const CODE_EMPTY: &str = "there is no lobby code";
pub const CODE_UNKNOWN_KIND: &str =
    "that is not a lobby code this app can read (mistyped, or made by a newer version of the app)";
pub const CODE_MISTYPED: &str = "that lobby code is mistyped (a character is wrong, missing or extra)";
pub const CODE_VERSION: &str = "that lobby code is of another version ({v}): one of the two apps needs an update";
pub const CODE_SPELLED: &str = "that lobby code is not written the way its host writes it: copy it from the host again";
pub const DIRECT_CODE_MISTYPED: &str = "that direct code is mistyped (a character is wrong, missing or extra)";
pub const DIRECT_CODE_VERSION: &str =
    "that direct code is of another version ({v}): one of the two apps needs an update";
pub const DIRECT_CODE_OUTSIDE: &str = "that direct code names an address outside a local network";
/// A direct join's refusals (direct.ts).
pub const DIRECT_CANCELLED: &str = "cancelled";
pub const DIRECT_HOST_REFUSED: &str = "the host refused the join: {why}";
pub const DIRECT_WRONG_HOST: &str = "the host is not the one its code names";
pub const DIRECT_UNREACHABLE: &str = "the host could not be reached: a network error ({each})";
/// More session ends (sessions.ts).
pub const CLOSED_BAD_GUEST_PACKET: &str = "bad packet";
pub const CLOSED_PARTY_ENDED: &str = "the party ended";
pub const CLOSED_PARTY_ENDED_BY: &str = "the party ended: ";
pub const CLOSED_PARTY_LOBBY_CLOSED: &str = "the party lobby closed: ";
pub const CLOSED_QUEUE_MOVED: &str = "the queue moved on";
pub const CLOSED_QUEUE_UNMATCHED: &str = "the queue found a match without you";
/// The status's reason: no backend has said one yet.
pub const STATUS_SIGNING_IN: &str = "signing in to online play";
/// A session or browse refused for online play being off: the prefix of the status's reason.
pub const ONLINE_OFF: &str = "online play is off: ";
pub const NO_QUEUE: &str = "the queue needs the relay or Epic, and neither is signed in (LAN has no queue): host a custom lobby, or turn on the relay or Epic in the launcher";
pub const LIST_RELAY_OFF: &str =
    "the online list is the relay's, and the relay is not connected ({why}): Epic and LAN have none";
pub const LIST_NO_LIST: &str = "this relay has no online list (an older relay)";
pub const LIST_FAILED: &str = "the relay did not give the online list: ";
/// A backend's failed search of the public lobbies ({name} the backend's, {why} its own words); LAN's is
/// '<Name>: <why>' (the port it could not listen on).
pub const SEARCH_FAILED: &str = "the search on {name} failed: {why}";
/// Why a session is over (ClosedWhy).
pub const CLOSED_LEFT: &str = "left";
pub const CLOSED_REPLACED: &str = "replaced";
pub const CLOSED_STOPPED: &str = "the net process stopped";
pub const CLOSED_NO_SESSION: &str = "no such session";
pub const CLOSED_NOT_RELAYED: &str = "Epic could not keep this match relayed: ";
pub const CLOSED_BACKEND_OFF: &str = "{backend} is off: {why}";
pub const CLOSED_RELAY_LOST: &str = "the link to the relay was lost: ";
pub const CLOSED_MAKE_FAILED: &str = "the lobby could not be made: ";
pub const CLOSED_JOIN_FAILED: &str = "the lobby could not be joined: ";
pub const CLOSED_NO_HOST: &str = "the lobby has no host";
pub const CLOSED_HOST_SILENT: &str = "the host did not answer";
pub const CLOSED_BAD_PACKET: &str = "a bad packet from the host";
pub const CLOSED_HOST_CLOSED: &str = "the host closed the link";
pub const CLOSED_HOST_LEFT: &str = "the host left";
pub const CLOSED_KICKED: &str = "kicked";
pub const CLOSED_LOBBY_CLOSED: &str = "the lobby closed";
pub const CLOSED_LOBBY_GONE: &str = "the lobby closed: ";
pub const CLOSED_HOST_LOST: &str = "the link to the host was lost";
pub const CLOSED_HOST_LINK_LOST: &str = "the link to the host was lost: ";
pub const CLOSED_DIRECT_STOPPED: &str = "the direct host stopped: ";
pub const CLOSED_SEND_FAILED: &str = "the link to the host failed";
pub const CLOSED_PARTY_MAKE_FAILED: &str = "the party could not be made: ";
pub const CLOSED_NO_LEADER: &str = "the party has no leader";
pub const CLOSED_PARTY_JOIN_FAILED: &str = "the party could not be joined: ";
pub const CLOSED_LEADER_SILENT: &str = "the party leader did not answer";
pub const CLOSED_LEADER_LEFT: &str = "the party leader left";
pub const CLOSED_LEADER_LOST: &str = "the link to the party leader was lost";
/// Why a join by a code failed (JoinWhy).
pub const JOIN_NO_DIRECT: &str = "direct play is not set up in this app";
pub const JOIN_GONE: &str = "the lobby is gone (its host closed it, or the code is old)";
pub const JOIN_FULL: &str = "the lobby is full";
pub const JOIN_EPIC_OFF: &str = "the lobby is on Epic Online Services, and this app is not signed in to Epic ({why})";
pub const JOIN_NO_DEFAULT_RELAY: &str = "its code names the release's relay, and this app has none";
pub const JOIN_SHORT_OTHER_RELAY: &str = "a short code names a lobby on the release's relay, and this app uses another";
pub const JOIN_RELAY_OFF: &str = "its relay ({address}) is not connected: {why}";
pub const JOIN_NO_RELAY_LINK: &str = "a link to another relay is not set up in this app";
pub const JOIN_RELAY_NOT_TAKEN: &str = "the relay at {address} was not taken";
pub const JOIN_RELAY_REFUSED: &str = "the relay at {address} did not take the link: ";
/// Why a party join failed (PartyJoinWhy).
pub const PARTY_JOIN_NO_INVITE: &str = "no party invite had that code";
pub const PARTY_JOIN_NOT_RELAY: &str = "its code names no relay lobby";
pub const PARTY_JOIN_OTHER_RELAY: &str = "its relay ({address}) is not this app's";
pub const PARTY_JOIN_THE_DEFAULT: &str = "the default";
pub const PARTY_JOIN_RELAY_OFF: &str = "the relay is not connected: {why}";
pub const PARTY_JOIN_NOT_INVITER: &str = "its leader is not who invited this player";
/// The session's question about a relay not played on before, and its two buttons.
pub const ASK_NEW_RELAY: &str =
    "This lobby is on the relay at {address}, which you have not played on before. Join it through that relay?";
/// The session's question about a relay played on before that now proves another key.
pub const ASK_RELAY_KEY_CHANGED: &str = "The relay at {address} proved another key than when you last played on it: its owner may have changed it, or it may not be the same server. Join it through that relay and trust its new key?";
pub const ASK_YES: &str = "Join";
pub const ASK_NO: &str = "Cancel";
/// The notes of the game: another protocol on the server, and an invite that was not sent.
pub const PROTOCOL_DIFFERS: &str =
    "This server runs another version of the game's online protocol. Update the game, or pick another server";

// ---------- the pre-match screen's notes (app/custom-pre-match.ts, protocol/online.ts COMPAT_MISSING_NOTE) ----------

pub const PM_ERROR: &str = "An error has occurred. Please matchmake again.";
pub const PM_LEFT: &str = "A player has disconnected.";
pub const PM_LEFT_NAMED: &str = "{username} has disconnected";
// ---- the online lobby (menu/lobby/net.rs, widgets/lobby_chat.rs, custom_game.rs, custom_browser.rs) ----

/// The lobby's chat: its heading, the input's hint, the notes of a player who came in or went.
pub const CHAT_LOBBY_TITLE: &str = "LOBBY CHAT";
pub const CHAT_LOBBY_HINT: &str = "Press Enter to chat";
pub const CHAT_JOINED: &str = "joined the lobby";
pub const CHAT_LEFT: &str = "left the lobby";
/// The lobby page's top buttons: the code's and the settings' labels.
pub const COPY_ROOM_CODE: &str = "COPY ROOM CODE";
/// (the port's own) The note shown when COPY ROOM CODE put the code on the clipboard.
pub const ROOM_CODE_COPIED: &str = "Room code copied to the clipboard";
/// (the port's own) A lobby slot's ready tag under the player's name.
pub const SLOT_READY: &str = "READY";
pub const SLOT_NOT_READY: &str = "NOT READY";
/// (the port's own) The invite modal's one list: every online player (the port has no friends).
pub const ONLINE_PLAYERS: &str = "ONLINE PLAYERS";
/// (the port's own) The empty line of the social panel's online list and of the invite modal (the build's names friends).
pub const NO_PLAYERS_ONLINE: &str = "No players are currently online";
/// (the port's own) The invite modal's empty block: its title and its hint (the build's name friends).
pub const NO_PLAYERS_AVAILABLE: &str = "NO PLAYERS AVAILABLE";
pub const INVITE_HINT: &str = "Players can only be invited if they are online and not already in a party.";
pub const ROOM_SETTINGS: &str = "ROOM SETTINGS";
/// The custom game browser's buttons.
pub const ENTER_ROOM_CODE: &str = "ENTER ROOM CODE";
pub const CREATE_LOBBY: &str = "CREATE LOBBY";
/// The lobby's notes (the TS's notification texts for what a lobby does).
pub const NOTE_NEW_LEADER: &str = "You are now leader!";
pub const NOTE_PLAYER_KICKED: &str = "Player kicked from custom game";
pub const NOTE_KICKED: &str = "You have been kicked";
pub const LOST_CONNECTION: &str = "Lost connection to the server";
/// lobby.error codes (host/index.ts QUEUED_SELF, NO_LOBBY, LOBBY_FULL; protocol/online.ts COMPAT_MISSING_NOTE,
/// COMPAT_NOTE).
pub const LOBBY_QUEUED_SELF: &str = "Leave matchmaking first";
pub const LOBBY_NO_LOBBY: &str = "The lobby no longer exists";
pub const LOBBY_FULL: &str = "The lobby is full";
pub const COMPAT_MISSING: &str =
    "Your game could not tell the server its game data version. Reload the page and try again";
pub const COMPAT_DIFFERS: &str = "Your game data differs from this lobby's (another game version or another import). Update the game and import again, then try again";
/// compatNote with the other player's name: `{name}` is theirs.
pub const COMPAT_MINE_OLDER: &str = "Your game is older than {name}'s. Update it, then try again";
pub const COMPAT_THEIRS_OLDER: &str = "{name} has an older version of the game and has to update it";
/// A lobby's code with no relay to name (sessions.ts codeOn), after the make's lead; a backend's answer of another
/// kind (the port's own words).
pub const CODE_ON_NO_RELAY: &str = "the relay's address is not in the online config";
pub const CLOSED_BAD_ANSWER: &str = "the online service's answer could not be read";
/// Why a LAN lobby's direct host did not start (direct.ts openDirectHost), after the make's lead: `{file}` the key's
/// file, `{why}` the system's reason, `{first}` and `{last}` the ports tried.
pub const HOST_START_KEY_READ: &str = "the relay key {file} could not be read: {why}";
pub const HOST_START_KEY_WRITE: &str = "the relay key {file} could not be written: {why}";
pub const HOST_START_NO_NETWORK: &str = "this PC is on no network (it has no address on a local network)";
pub const HOST_START_CANNOT_LISTEN: &str = "it cannot listen";
pub const HOST_START_CANNOT_LISTEN_WHY: &str = "it cannot listen: {why}";
pub const HOST_START_PORTS_IN_USE: &str = "ports {first} to {last} are all in use";
pub const HOST_START_THREAD_STOPPED: &str = "the direct host's thread stopped";
pub const HOST_START_OWN_REFUSED: &str = "the host's own player was refused: {why}";
/// A backend's reason for a failed call (mvs_net::fail::NetFail), after a lead ("the lobby could not be made: ").
pub const FAIL_HOST_CLOSED: &str = "the direct host has closed";
pub const FAIL_UNWRITABLE: &str = "a request that cannot be written";
pub const FAIL_RELAY_CLOSED: &str = "the relay connection is closed";
pub const FAIL_RELAY_FULL: &str = "relay full";
pub const FAIL_LOBBY_UNREAD: &str = "the lobby could not be read";
/// `{code}` the backend's result name (EOS_Lobby_TooManyPlayers).
pub const FAIL_CODE: &str = "{code}";
/// A reason with no words of the game's: the log holds the backend's own.
pub const FAIL_OTHER: &str = "the online service refused it (see the log)";
/// Why a lobby's code cannot be written (codes.ts encodeLobbyCode, direct-code.ts encodeDirectCode), after the
/// make's lead: `{lobby}` the lobby's id, `{relay}` the relay's address, `{address}` the host's, `{port}` its port.
pub const CODE_MAKE_DIRECT_KIND: &str = "a direct lobby code starts with d";
pub const CODE_MAKE_EPIC_ID: &str = "a lobby code cannot hold the EOS lobby id {lobby}";
pub const CODE_MAKE_RELAY_ID: &str = "a lobby code cannot hold the relay lobby id {lobby}";
pub const CODE_MAKE_NO_DEFAULT_RELAY: &str = "a lobby code of the default relay needs one: this app has none";
pub const CODE_MAKE_RELAY_ADDRESS: &str = "a lobby code cannot hold the relay address {relay}";
pub const CODE_MAKE_DIRECT_ADDRESS: &str = "a direct code cannot hold the address {address}";
pub const CODE_MAKE_DIRECT_NO_ADDRESS: &str = "a direct code needs an address";
pub const CODE_MAKE_DIRECT_PORT: &str = "a direct code cannot hold the port {port}";
pub const CODE_MAKE_DIRECT_FINGERPRINT: &str = "a direct code needs a 16-digit key fingerprint";
pub const CODE_MAKE_DIRECT_LOBBY: &str = "a direct code needs a lobby id of l and 20 hex digits";

// The menus' one line of online play (OnlineLine): a headline, and for offline a sentence of what to do. No backend
// is named.
pub const LINE_ONLINE: &str = "Online";
pub const LINE_SLOWER: &str = "Online, but matches may lag";
pub const LINE_SLOWER_DO: &str = "A firewall or your network is blocking the game's faster connection, so matches use a slower one. Allowing MVS Classic through your firewall usually fixes this.";
pub const LINE_OFFLINE: &str = "Offline";
pub const LINE_CHECKING: &str = "Checking this copy of the game";
pub const LINE_CHECKING_DO: &str = "Online play starts in a moment.";
pub const LINE_SIGNING_IN: &str = "Connecting";
pub const LINE_SIGNING_IN_DO: &str = "Online play starts in a moment.";
pub const LINE_UPDATE: &str = "Update needed";
/// {version}: the newer release's.
pub const LINE_UPDATE_DO: &str = "Version {version} is out. Update it in the launcher to play online.";
pub const LINE_IMPORT: &str = "Game files out of date";
pub const LINE_IMPORT_DO: &str = "Rebuild the game files in the launcher to play online.";
pub const LINE_SELF_CHECK: &str = "This copy plays differently";
pub const LINE_SELF_CHECK_DO: &str =
    "Its online matches would go out of sync. Rebuild the game files in the launcher, or install the game again.";
pub const LINE_TURNED_OFF: &str = "Online play is turned off";
pub const LINE_TURNED_OFF_DO: &str = "Turn it on in the launcher's Settings, under Online.";
pub const LINE_NOT_REACHED: &str = "Server not reached";
pub const LINE_NOT_REACHED_DO: &str = "Check your internet connection. The game keeps trying on its own.";
pub const LINE_OTHER: &str = "Online play could not start";
pub const LINE_OTHER_DO: &str = "Test the connection in the launcher's Settings, under Online.";

/// A match that went out of sync: its report kept for a bug report (nothing is uploaded).
pub const DESYNC_NOTE: &str =
    "The match went out of sync. A report was saved on this PC: add it to a bug report to help fix it.";
pub const OPEN_FOLDER: &str = "OPEN FOLDER";

// A join link (mvsclassic://join/<code>) that came while online play is held (the gate): it waits, then joins.
pub const LINK_HELD: &str = "The join link waits until online play is on.";

/// The pre-match screen's notice to a player past the far limit on every way of the match's path (pm.path's `far`):
/// the match is still played.
pub const PATH_FAR: &str = "You are far from the other players: this match may lag for you.";

/// The online list's line under a player's name: what they are doing (their presence's status). The build's
/// Game.locres has rich-presence lines ("Lobby", "Matchmaking", "in an online match"), but none in this form.
pub const STATUS_MENUS: &str = "In Menus";
pub const STATUS_LOBBY: &str = "In Lobby";
pub const STATUS_QUEUE: &str = "In Queue";
pub const STATUS_1V1: &str = "In a 1v1 Match";
pub const STATUS_2V2: &str = "In a 2v2 Match";
pub const STATUS_FFA: &str = "In a Free-for-all Match";
pub const STATUS_MATCH: &str = "In a Match";
