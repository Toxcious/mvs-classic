//! What the pre-match screen is given and gives back: the players of the
//! match in player-index order with their teams, the team style and the next map, the picks the screen ends with, and
//! what the widget hands the program (`Exit`, `PreMatchCall`: the hooks the program answers).
//!
//! The level's calls of the hooks (changeFighter, transition, skinData, preloadMap, cosmetics) are scene commands the
//! widget pushes itself (`Cx::scene`), the sound states go to the menu's sink.
//!
//! Not ported: the queue's and the online fields of `PreMatchSetup` and the hooks of their paths
//! (startMatchmaking, cancelMatchmaking, localUnlocked, perksLocked, isMatchmaking, registeredPlayers, screenUp,
//! localPreview, localLocked: what the other clients are told): the online track's.

use mvs_render::menuscene::PreviewSpec;

use crate::bundle::RosterEntry;
use crate::menu::arcade::ArcadeRun;
use crate::menu::lobby::loadout::BotLoadout;
use crate::menu::lobby::{LocalLobby, PreMatchPick, SetupSource};

/// A player of the pre-match screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PreMatchPlayer {
    pub account_id: String,
    pub username: String,
    /// TeamIndex (4: a spectator, no widget, not waited for).
    pub team: u8,
    pub is_bot: bool,
    /// A bot's lobby row: its roster key (None: a human's, or a bot with none) and SkinData name ("" the default),
    /// what its AICharacterSelector takes.
    pub character: Option<String>,
    pub skin: String,
    /// A bot's cosmetics drawn for the match (None: a human's, read from this machine's loadout).
    pub cosmetics: Option<BotLoadout>,
    /// An online player's banner, badges and icon as the server handed them on (pm.locked, pm.all).
    pub net_cosmetics: Option<mvs_protocol::online::PlayerCosmetics>,
}

/// What the pre-match screen is given (offline: the queue's and the online fields are
/// not here yet).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PreMatchSetup {
    pub players: Vec<PreMatchPlayer>,
    /// The local player's index in `players`.
    pub me: usize,
    /// The active match config's team style: 0 1v1, 1 2v2, 2 free for all.
    pub team_style: u8,
    /// The local account is the lobby's leader (Btn_BackToCustomLobby).
    pub leader: bool,
    /// The next map's MapData name (RematchMapPath), and the level the match start opens.
    pub map: Option<String>,
    pub level: String,
    /// A queue set's YourScore / TheirScore so far (UpdateSetScore); None: no set.
    pub set_score: Option<(u32, u32)>,
    /// An arcade round's pre-match: the run as it opened (its rematch flag; the screen reads the session's run for
    /// what the lock changes, the rounds drawn at the first one).
    pub arcade: Option<ArcadeRun>,
    /// An online pre-match or the queue's (the program fills it; None: offline).
    pub net: Option<crate::menu::online::PreMatchNet>,
    /// (the port's own) A single-player mode's screen: the mode's title (`SoloMode`). None: the build's layouts.
    pub solo: Option<SoloMode>,
}

/// (the port's own) The solo layout of a single-player mode (Waves): the local player alone with no opposing side,
/// and the mode's name where the screen names the mode.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SoloMode {
    /// The mode's name ("Waves - 100 Ring-Outs"), shown as the mode label's mode.
    pub title: String,
}

/// What the program opens the pre-match screen with: a local custom game's
/// lobby with the set-up it gives, or a set-up alone.
#[derive(Debug)]
pub struct PreMatchStart {
    pub setup: PreMatchSetup,
    /// The local custom game's lobby (it takes the picks, `LocalLobby::apply_picks`, and starts the match).
    pub lobby: Option<Box<LocalLobby>>,
}

/// What the pre-match screen ends with: the match's map with each player's pick,
/// or back to the lobby over the main menu.
#[derive(Debug)]
pub enum PreMatchEnd {
    Match { level: String, picks: Vec<PreMatchPick> },
    BackToLobby,
}

/// What the widget leaves for the screen (hooks startMatch, backToLobby, kickBackToMainMenu); the widget has begun
/// the fade to the level itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exit {
    /// BPI_CleanUpAndStartMatch PM@8700 with the selections the character select manager holds.
    StartMatch(Vec<PreMatchPick>),
    /// Btn_BackToCustomLobby PM@111338.
    BackToLobby,
    /// BPI_KickBackToMainMenu(text): the main menu's notification text.
    Kick(String),
}

/// The announcer's two lines of the screen (PreMatch @16172-16531, @29035-29394).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnouncerLine {
    Intro,
    Outro,
}

/// What the widget asks of the program's sound side (hooks playVo, announcer).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreMatchCall {
    /// The preview actor's BPI_PlayVo(SoundAlias, ExtraVoTags, AlwaysPlayVo): ("chaselect", "", true) PM@22900 and
    /// ("clickable", "affinity_level_01", false) PM@23826.
    PlayVo { preview: usize, alias: String, tags: String, always: bool },
    /// The game state's BPI_PlayVO of the announcer's line.
    Announcer(AnnouncerLine),
}

/// A preview's fighter as the level takes it: None for a fighter the port has no data for.
pub fn spec_of(c: &RosterEntry, skin: &str, team: u8) -> Option<PreviewSpec> {
    c.fighter.as_ref().map(|key| PreviewSpec { key: key.clone(), skin: skin.to_string(), team })
}

impl LocalLobby {
    /// The started match's players with their teams (PlayerIndex % team count), a
    /// bot's row and cosmetics, the team style and the map. A human's row carries no character (the pick is made on
    /// the screen); a bot's carries its own. The local account is the leader of a local lobby.
    pub fn pre_match_setup(&mut self, src: &SetupSource<'_>, random: &mut dyn FnMut() -> f64) -> PreMatchSetup {
        let started = self.rules.match_setup();
        let teams = if started.setup.team_style == 2 { 4 } else { 2 };
        let mut players = Vec::new();
        for (i, id) in started.ids.iter().enumerate() {
            let row = self.rules.players().into_iter().find(|p| &p.account_id == id).cloned();
            let is_bot = row.as_ref().is_some_and(|p| p.is_bot);
            let cosmetics = if is_bot { Some(self.bot_loadout(id, src.bot_pool, random)) } else { None };
            players.push(PreMatchPlayer {
                account_id: id.clone(),
                username: started.usernames[i].clone(),
                team: u8::try_from(i % teams).unwrap_or(0),
                is_bot,
                character: row.as_ref().filter(|_| is_bot).map(|p| p.character.clone()).filter(|c| !c.is_empty()),
                skin: row.as_ref().filter(|_| is_bot).map(|p| p.skin.clone()).unwrap_or_default(),
                cosmetics,
                net_cosmetics: None,
            });
        }
        let map = self.starting.as_ref().and_then(|s| s.map.clone());
        PreMatchSetup {
            players,
            me: started.ids.iter().position(|id| id == Self::LOCAL_ID).unwrap_or(0),
            team_style: started.setup.team_style,
            leader: self.am_i_leader(),
            level: map.clone().unwrap_or_default(),
            map,
            set_score: None,
            net: None,
            arcade: None,
            solo: None,
        }
    }

    /// The match the screen's picks start (BPI_CleanUpAndStartMatch): the picks go in, the lobby's match is made with
    /// this machine's loadout, the lobby goes with it for the match's end.
    pub fn start_with_picks(
        mut self: Box<Self>,
        picks: Vec<PreMatchPick>,
        src: &SetupSource<'_>,
        random: &mut dyn FnMut() -> f64,
    ) -> crate::menu::lobby::LobbyMatch {
        self.apply_picks(picks);
        let mut m = self.match_setup(src, random);
        self.detach();
        m.lobby = Some(self);
        m
    }
}
