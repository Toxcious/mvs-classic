//! The custom lobby: the offline lobby's players and rules, the match it starts.
//!
//! The lobby holds its echoes as a queue (`Echo`), which the widget that shows it drains each frame
//! (`LocalLobby::take_events`) and turns into its own calls, as listeners would: a request changes the lobby's
//! rules at once and its echo reaches the widget on the next drain, with the rules' values of that moment.
//!
//! Not ported: NetLobby and the online parts of the lobby (the wire, the code, the chat, the invitations): R4.

pub mod any;
pub mod loadout;
pub mod net;

pub use any::Lobby;
pub use net::{ChatKind, ChatLine, NetLobby};

use std::cell::RefCell;

use indexmap::IndexMap;
use mvs_protocol::lobby_rules::{
    LobbyOptions, LobbyPlayer, LobbyRules, LobbyStyle, OptionKey, OptionValue, StartedMatch,
};
use mvs_settings::HydraSettings;
use mvs_sim::MatchSetup;
use mvs_sim::setup::{MatchPlayer, Rules, TeamStyle};

use crate::bundle::RosterEntry;
use loadout::{BotLoadout, BotPool};

/// A local human of a Local Play match: player slot `slot` on `device` (the menus'
/// local player `slot`'s), with binding set `set`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalHuman {
    pub slot: usize,
    pub device: crate::menu::local_play_match::Device,
    pub set: usize,
}

/// A match the menus start (less its online fields).
#[derive(Clone, Debug, Default)]
pub struct LobbyMatch {
    pub setup: MatchSetup,
    /// The local player's slot.
    pub me: usize,
    pub usernames: Vec<String>,
    /// The match's MapData, one of the lobby's enabled maps (None: a dev room).
    pub map: Option<String>,
    /// Local Play: the local humans and their devices; None: `me` alone on the keyboard and pad 0.
    pub humans: Option<Vec<LocalHuman>>,
    /// A custom lobby's match: the lobby it was started from, shown again at the match's end (Lobby.kept). None for
    /// Local Play and the others.
    pub lobby: Option<Box<LocalLobby>>,
}

/// The listeners' calls (Lobby.onLobbyStarted ... onMatchStarting), the local lobby's: each with the value the
/// rules hold when the echo is drained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LobbyEvent {
    LobbyStarted,
    TeamsUpdated,
    TeamStyleUpdated(LobbyStyle),
    MatchDurationUpdated(u32),
    NumRingoutsForWinUpdated(u32),
    AllowHazardsUpdated(bool),
    EnabledMapsUpdated(Vec<String>),
    /// (the port's own) the lobby's settings (LOBBY_OPTIONS) changed.
    OptionsUpdated,
    StartMatchFailed,
    MatchStarting,
    // ---- an online lobby's listeners, which the local lobby never calls ----
    ReadyPlayersUpdated,
    NewLeader(String),
    PlayerKicked(LobbyPlayer),
    /// This player was kicked: the lobby is over.
    Kicked,
    /// onError: a note for the page.
    Error(LobbyNote),
    /// The lobby is gone (the connection dropped).
    Closed,
    /// A line of the lobby's chat.
    Chat(net::ChatLine),
}

/// What a lobby's error says, before it is worded (net.rs `note_text`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LobbyNote {
    /// lobby.refused: the lobby's invitedOnly option kept this player out.
    InvitedOnlyRefused,
    /// lobby.error.
    Lobby { why: mvs_protocol::messages::LobbyErrorWhy, name: Option<String> },
    /// The session could not open or ended before the lobby's first answer.
    Closed(mvs_net::service::api::ClosedWhy),
}

/// A deferred echo, before it takes the rules' values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Echo {
    Started,
    TeamStyle,
    MatchDuration,
    NumRingouts,
    AllowHazards,
    EnabledMaps,
    Options,
    Teams,
    StartMatch,
}

/// A pre-match screen's selection of one player (PreMatchPick).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreMatchPick {
    pub account_id: String,
    pub character: String,
    pub skin: String,
    pub perks: Vec<Option<String>>,
}

/// The started match's players in player-index order, its map and the pre-match screen's picks, and each bot's
/// random cosmetics by account id.
#[derive(Clone, Debug, Default)]
pub(crate) struct Starting {
    pub(crate) map: Option<String>,
    pub(crate) picks: Vec<PreMatchPick>,
    bots: IndexMap<String, BotLoadout>,
}

/// What a match set-up reads from the menus' data and the machine's saves.
pub struct SetupSource<'a> {
    /// Whether the port can draw a character.
    pub drawable: &'a dyn Fn(&str) -> bool,
    pub hydra: &'a HydraSettings,
    /// The local players' input preferences (the Controls page's).
    pub input: &'a mvs_settings::InputSettings,
    pub roster: &'a [RosterEntry],
    pub bot_pool: &'a BotPool,
}

/// The offline custom lobby: kept through its match, shown again at its end.
#[derive(Clone, Debug)]
pub struct LocalLobby {
    pub(crate) rules: LobbyRules,
    /// The widget showing the lobby is up: the echoes have somebody to reach.
    attached: bool,
    echoes: Vec<Echo>,
    pub(crate) starting: Option<Starting>,
}

impl LocalLobby {
    pub const LOCAL_ID: &'static str = "local";

    pub fn new(username: &str) -> LocalLobby {
        let mut rules = LobbyRules::new();
        rules.add_player(Self::LOCAL_ID, username, "");
        rules.ready = vec![Self::LOCAL_ID.to_string()];
        LocalLobby { rules, attached: true, echoes: Vec::new(), starting: None }
    }

    /// A kept lobby under a new widget.
    pub fn attach(&mut self) {
        self.attached = true;
    }

    /// The widget is gone (a match is on): the echoes have nobody to reach.
    pub fn detach(&mut self) {
        self.attached = false;
        self.echoes.clear();
    }

    fn defer(&mut self, e: Echo) {
        if self.attached {
            self.echoes.push(e);
        }
    }

    /// The echoes due, in order, each with the rules' values now (the callbacks' `this.rules.x` at fire time).
    /// `random`: Math.random, for a match start's map.
    pub fn take_events(&mut self, random: &mut dyn FnMut() -> f64) -> Vec<LobbyEvent> {
        let mut out = Vec::new();
        for e in std::mem::take(&mut self.echoes) {
            match e {
                Echo::Started => {
                    out.push(LobbyEvent::LobbyStarted);
                    out.push(LobbyEvent::TeamStyleUpdated(self.rules.team_style));
                    out.push(LobbyEvent::MatchDurationUpdated(self.rules.match_duration));
                    out.push(LobbyEvent::NumRingoutsForWinUpdated(self.rules.num_ringouts_for_win));
                    out.push(LobbyEvent::AllowHazardsUpdated(self.rules.allow_hazards));
                    out.push(LobbyEvent::EnabledMapsUpdated(self.rules.enabled_maps.clone()));
                    out.push(LobbyEvent::OptionsUpdated);
                }
                Echo::TeamStyle => out.push(LobbyEvent::TeamStyleUpdated(self.rules.team_style)),
                Echo::MatchDuration => out.push(LobbyEvent::MatchDurationUpdated(self.rules.match_duration)),
                Echo::NumRingouts => out.push(LobbyEvent::NumRingoutsForWinUpdated(self.rules.num_ringouts_for_win)),
                Echo::AllowHazards => out.push(LobbyEvent::AllowHazardsUpdated(self.rules.allow_hazards)),
                Echo::EnabledMaps => out.push(LobbyEvent::EnabledMapsUpdated(self.rules.enabled_maps.clone())),
                Echo::Options => out.push(LobbyEvent::OptionsUpdated),
                Echo::Teams => out.push(LobbyEvent::TeamsUpdated),
                Echo::StartMatch => {
                    // The request's answer: the match starts, or the request fails while a blocker stands. The map is
                    // taken here: the build's start notification carries it (RematchMapPath,
                    // PGI_MatchmakingHandler @40271) and the pre-match screen shows it.
                    // Deviation: how StartMatch picks the map is the backend's; here any of the enabled maps, each as
                    // likely.
                    if !self.can_start_match() {
                        out.push(LobbyEvent::StartMatchFailed);
                    } else {
                        let map = pick_map(&self.rules.enabled_maps, random);
                        self.starting = Some(Starting { map, ..Starting::default() });
                        out.push(LobbyEvent::MatchStarting);
                    }
                }
            }
        }
        out
    }

    // ---------- the state ----------

    pub fn my_id(&self) -> &str {
        Self::LOCAL_ID
    }
    pub fn team_style(&self) -> LobbyStyle {
        self.rules.team_style
    }
    pub fn num_ringouts_for_win(&self) -> u32 {
        self.rules.num_ringouts_for_win
    }
    pub fn match_duration(&self) -> u32 {
        self.rules.match_duration
    }
    pub fn allow_hazards(&self) -> bool {
        self.rules.allow_hazards
    }
    pub fn enabled_maps(&self) -> &[String] {
        &self.rules.enabled_maps
    }
    pub fn leader_id(&self) -> &str {
        &self.rules.leader_id
    }
    pub fn teams(&self) -> &[Vec<LobbyPlayer>; 2] {
        &self.rules.teams
    }
    pub fn players(&self) -> Vec<&LobbyPlayer> {
        self.rules.players()
    }
    pub fn options(&self) -> &LobbyOptions {
        &self.rules.options
    }
    /// An online lobby (its settings include who may find it); a local one is this PC's alone.
    pub fn online(&self) -> bool {
        false
    }
    pub fn am_i_leader(&self) -> bool {
        self.rules.leader_id == Self::LOCAL_ID
    }
    pub fn is_ready(&self, account_id: &str) -> bool {
        self.rules.ready.iter().any(|r| r == account_id)
    }
    pub fn get_available_player_slot_count(&self) -> usize {
        self.rules.get_available_player_slot_count()
    }
    pub fn get_start_match_blockers(&self) -> Vec<&'static str> {
        self.rules.get_start_match_blockers()
    }
    pub fn can_start_match(&self) -> bool {
        self.rules.can_start_match()
    }
    /// (the port's own) The READY button of a human who is not the leader. A local lobby's one human is its
    /// leader.
    pub fn has_ready_button(&self) -> bool {
        false
    }
    pub fn set_ready(&mut self, _on: bool) {}

    /// The account's name changed while the lobby was kept.
    pub fn set_local_name(&mut self, username: &str) {
        if let Some(me) = self.rules.find_mut(Self::LOCAL_ID) {
            me.username = username.to_string();
        }
    }

    // ---------- the requests ----------

    /// The lobby's creation answer: the teams, then the settings.
    pub fn start(&mut self) {
        self.defer(Echo::Started);
    }

    pub fn set_team_style(&mut self, style: LobbyStyle) {
        if style == self.rules.team_style {
            return;
        }
        self.rules.set_team_style(style);
        self.defer(Echo::TeamStyle);
    }

    pub fn set_match_duration(&mut self, sec: u32) {
        self.rules.match_duration = sec;
        self.defer(Echo::MatchDuration);
    }

    pub fn set_num_ringouts_for_win(&mut self, n: u32) {
        self.rules.num_ringouts_for_win = n;
        self.defer(Echo::NumRingouts);
    }

    pub fn set_allow_hazards(&mut self, b: bool) {
        self.rules.allow_hazards = b;
        self.defer(Echo::AllowHazards);
    }

    pub fn set_maps(&mut self, maps: &[String]) {
        self.rules.set_maps(maps);
        self.defer(Echo::EnabledMaps);
    }

    /// (the port's own) one of the lobby's settings, the leader's.
    pub fn set_option(&mut self, key: OptionKey, value: OptionValue) {
        self.rules.set_option(key, value);
        self.defer(Echo::Options);
    }

    pub fn add_bot(&mut self, team: usize, character: &str, skin: &str, difficulty: i32) {
        self.rules.add_bot(team, character, skin, difficulty);
        self.defer(Echo::Teams);
    }

    pub fn kick_player(&mut self, account_id: &str) {
        self.rules.remove_player(account_id);
        self.defer(Echo::Teams);
    }

    pub fn join_team(&mut self, account_id: &str, team: usize) {
        self.rules.join_team(account_id, team);
        self.defer(Echo::Teams);
    }

    pub fn update_settings_for_bot(&mut self, account_id: &str, character: &str, difficulty: i32, skin: &str) {
        self.rules.update_settings_for_bot(account_id, character, difficulty, skin);
        self.defer(Echo::Teams);
    }

    /// Nobody else is here.
    pub fn promote_new_leader(&mut self, _account_id: &str) {}
    pub fn invite(&mut self, _account_id: &str) {}
    pub fn leave(&mut self) {}

    /// StartMatch: the answer comes with the next drain (a `MatchStarting` or a `StartMatchFailed`).
    pub fn start_match(&mut self) {
        self.defer(Echo::StartMatch);
    }

    /// BPI_CleanUpAndStartMatch: the selections the pre-match screen ended with are the match's fighters. They are
    /// the started match's alone: the lobby's rows keep what they had (a bot's own character, a human's none).
    pub fn apply_picks(&mut self, picks: Vec<PreMatchPick>) {
        if let Some(s) = &mut self.starting {
            s.picks = picks;
        }
    }

    /// A bot's cosmetics for the started match, drawn at its first ask (a set-up with no start draws each time).
    pub(crate) fn bot_loadout(&mut self, id: &str, pool: &BotPool, random: &mut dyn FnMut() -> f64) -> BotLoadout {
        if let Some(had) = self.starting.as_ref().and_then(|s| s.bots.get(id)) {
            return had.clone();
        }
        let r = loadout::random_bot_loadout(pool, random);
        if let Some(s) = &mut self.starting {
            s.bots.insert(id.to_string(), r.clone());
        }
        r
    }

    /// The match the lobby starts (LocalLobby.matchSetup; a start that did not come through `start_match` has no map
    /// yet: one is drawn). The lobby itself goes with the match (`LobbyMatch::lobby`) when the caller moves it in.
    pub fn match_setup(&mut self, src: &SetupSource<'_>, random: &mut dyn FnMut() -> f64) -> LobbyMatch {
        let StartedMatch { setup, usernames, ids } = self.rules.match_setup();
        let map = match &self.starting {
            Some(s) => s.map.clone(),
            None => pick_map(&self.rules.enabled_maps, random),
        };
        let mut ms = MatchSetup {
            team_style: match setup.team_style {
                0 => TeamStyle::OneVsOne,
                1 => TeamStyle::TwoVsTwo,
                _ => TeamStyle::FreeForAll,
            },
            players: setup
                .players
                .iter()
                .map(|p| MatchPlayer {
                    bot: p.bot,
                    difficulty: p.bot.then_some(mvs_sim::math::narrow(f64::from(p.difficulty))),
                    character: p.character.clone(),
                    ..MatchPlayer::default()
                })
                .collect(),
            rules: Some(Rules {
                knockouts_to_win: mvs_sim::math::narrow(f64::from(setup.rules.knockouts_to_win)),
                duration: mvs_sim::math::narrow(f64::from(setup.rules.duration)),
                stocks: setup.rules.stocks,
            }),
            flow: setup.flow,
            allow_hazards: setup.allow_hazards,
            mutators: setup.mutators.clone(),
            wheel: setup.wheel.as_ref().map(sim_wheel),
            ..MatchSetup::default()
        };
        // the perks each player locked on the pre-match screen, and their pick's fighter and skin
        for (i, p) in ms.players.iter_mut().enumerate() {
            let pick = self.starting.as_ref().and_then(|s| s.picks.iter().find(|x| x.account_id == ids[i]));
            if let Some(pick) = pick {
                p.character = Some(pick.character.clone());
                if !pick.skin.is_empty() {
                    p.skin = Some(pick.skin.clone());
                }
                p.perks = mvs_sim::setup::perk_list(
                    &pick.perks.iter().map(|x| x.clone().unwrap_or_default()).collect::<Vec<_>>(),
                );
            }
        }
        loadout::ported_setup(&mut ms, src.drawable);
        // (the loadout: this machine's taunts for every fighter, the ring-out and the banner for the local player; a
        // bot's its random ones)
        let mine: Vec<bool> = ids.iter().map(|id| id == Self::LOCAL_ID).collect();
        let mut bots: Vec<Option<BotLoadout>> = Vec::new();
        for (i, p) in ms.players.iter().enumerate() {
            bots.push(if p.bot { Some(self.bot_loadout(&ids[i], src.bot_pool, random)) } else { None });
        }
        let own_ring_out = loadout::local_ring_out(src.hydra);
        let own_banner = loadout::local_banner(src.hydra);
        loadout::with_local_loadout(
            &mut ms,
            src.hydra,
            src.roster,
            &mut |_, i| {
                if mine[i] {
                    own_ring_out.clone()
                } else {
                    bots[i].as_ref().map(|b| b.ring_out.clone()).unwrap_or_default()
                }
            },
            &mut |_, i| {
                if mine[i] {
                    own_banner.clone()
                } else {
                    bots[i].as_ref().map(|b| b.banner.clone()).unwrap_or_default()
                }
            },
            &mut |_, i| mine[i].then(|| crate::menu::prefs::sim_input_prefs(src.input, 0)),
        );
        let me = ids.iter().position(|id| id == Self::LOCAL_ID).unwrap_or(0);
        LobbyMatch { setup: ms, me, usernames, map, humans: None, lobby: None }
    }
}

/// `maps[Math.floor(Math.random() * maps.length)]`.
fn pick_map(maps: &[String], random: &mut dyn FnMut() -> f64) -> Option<String> {
    if maps.is_empty() {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = (random() * maps.len() as f64).floor() as usize;
    maps.get(i).cloned()
}

// ---------- what the menus keep across screens (the lobby's and Local Play's statics) ----------

/// What is held across screens, read by the widgets of a level and by the program between levels.
/// Unverified: kept in a per-thread slot (the menus run on the program's main thread) until the menu host gives the
/// carry a field of its own.
#[derive(Debug, Default)]
pub struct MenuCarry {
    /// The match a screen started, for the program's level loop to load (a custom lobby's Start).
    pub start: Option<Box<LobbyMatch>>,
    /// The pre-match screen a custom lobby's Start opens (the PreMatch level's options Lobby).
    pub pre_match: Option<Box<super::screens::pre_match::setup::PreMatchStart>>,
    /// Lobby.kept of an online lobby (NetLobby): shown again at a match's end while its connection lasts.
    pub kept_net: Option<Box<NetLobby>>,
    /// NetLobby.pendingJoin: the code of the lobby the next custom game page joins (an accepted invitation, a pasted
    /// code, a browser row).
    pub pending_join: Option<String>,
}

thread_local! {
    static CARRY: RefCell<MenuCarry> = RefCell::new(MenuCarry::default());
}

/// The sim's Chaos Wheel rule of a set-up's `wheel` (an unknown rate is the default one).
pub fn sim_wheel(w: &mvs_protocol::setup::WheelWire) -> mvs_sim::setup::Wheel {
    mvs_sim::setup::Wheel {
        pool: w.pool.clone(),
        seconds: mvs_sim::math::narrow(f64::from(w.seconds)),
        warn: mvs_sim::math::narrow(f64::from(w.warn)),
        chaos: w
            .chaos
            .as_deref()
            .and_then(mvs_mutators::ChaosRate::from_key)
            .unwrap_or(mvs_mutators::ChaosRate::Normal),
        two: w.two,
    }
}

/// `f` on the menus' carry.
pub fn with_carry<R>(f: impl FnOnce(&mut MenuCarry) -> R) -> R {
    CARRY.with(|c| f(&mut c.borrow_mut()))
}

/// Lobby.kept = lobby, an online one.
pub fn keep_net(lobby: Box<NetLobby>) {
    with_carry(|c| c.kept_net = Some(lobby));
}

/// The kept online lobby, taken.
pub fn take_kept_net() -> Option<Box<NetLobby>> {
    with_carry(|c| c.kept_net.take())
}

/// NetLobby.pendingJoin = code.
pub fn set_pending_join(code: Option<String>) {
    with_carry(|c| c.pending_join = code);
}

/// NetLobby.pendingJoin, taken (the page that reads it clears it).
pub fn take_pending_join() -> Option<String> {
    with_carry(|c| c.pending_join.take())
}

/// The match a screen started, taken by the program's level loop.
pub fn take_start() -> Option<Box<LobbyMatch>> {
    with_carry(|c| c.start.take())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src<'a>(d: &'a dyn Fn(&str) -> bool, h: &'a HydraSettings, p: &'a BotPool) -> SetupSource<'a> {
        static INPUT: std::sync::LazyLock<mvs_settings::InputSettings> = std::sync::LazyLock::new(Default::default);
        SetupSource { drawable: d, hydra: h, input: &INPUT, roster: &[], bot_pool: p }
    }

    fn zero() -> impl FnMut() -> f64 {
        || 0.0
    }

    #[test]
    fn a_request_echoes_on_the_next_drain_with_the_values_of_then() {
        let mut l = LocalLobby::new("Me");
        l.start();
        l.set_team_style(LobbyStyle::FreeForAll);
        l.set_match_duration(300);
        l.set_match_duration(180);
        let ev = l.take_events(&mut zero());
        assert_eq!(ev[0], LobbyEvent::LobbyStarted);
        // the creation answer reads the rules as they are by now
        assert_eq!(ev[1], LobbyEvent::TeamStyleUpdated(LobbyStyle::FreeForAll));
        assert_eq!(ev[2], LobbyEvent::MatchDurationUpdated(180));
        assert_eq!(ev[7], LobbyEvent::TeamStyleUpdated(LobbyStyle::FreeForAll));
        assert_eq!(ev[9], LobbyEvent::MatchDurationUpdated(180));
        assert!(l.take_events(&mut zero()).is_empty());
        // a detached lobby echoes to nobody
        l.detach();
        l.set_match_duration(100);
        assert!(l.take_events(&mut zero()).is_empty());
        l.attach();
        // the same style again is no request
        l.set_team_style(LobbyStyle::FreeForAll);
        assert!(l.take_events(&mut zero()).is_empty());
    }

    #[test]
    fn a_start_fails_with_a_blocker_and_goes_when_there_is_none() {
        let mut l = LocalLobby::new("Me");
        l.start_match();
        assert_eq!(l.take_events(&mut zero()), vec![LobbyEvent::StartMatchFailed]);
        l.add_bot(0, "shaggy", "", 3);
        l.add_bot(1, "finn", "", 4);
        l.add_bot(1, "batman", "", 5);
        l.start_match();
        let ev = l.take_events(&mut zero());
        assert!(ev.contains(&LobbyEvent::MatchStarting));
        let drawable = |c: &str| c == "shaggy";
        let (h, p) = (HydraSettings::default(), BotPool::default());
        let m = l.match_setup(&src(&drawable, &h, &p), &mut zero());
        assert_eq!(m.map.as_deref(), Some("map_m011_largenoplat"));
        assert_eq!(m.me, 0);
        assert_eq!(m.setup.players.len(), 4);
        assert_eq!(m.setup.team_style, TeamStyle::TwoVsTwo);
        // only a character the port draws stays
        let kept: Vec<_> = m.setup.players.iter().map(|p| p.character.as_deref()).collect();
        assert_eq!(kept, vec![None, None, Some("shaggy"), None]);
        assert_eq!(m.setup.players[1].difficulty, Some(2.0));
        assert!(m.setup.flow);
    }

    #[test]
    fn a_versus_lobby_starts_any_split_with_a_player_a_side() {
        let (h, p) = (HydraSettings::default(), BotPool::default());
        let drawable = |_: &str| true;
        let mut l = LocalLobby::new("Me");
        l.add_bot(1, "finn", "", 4);
        let m = l.match_setup(&src(&drawable, &h, &p), &mut zero());
        assert_eq!(m.setup.team_style, TeamStyle::OneVsOne);
        // two against the human: the bots' team is the match's team 0, the human the odd slot
        l.add_bot(1, "batman", "", 4);
        l.start_match();
        assert!(l.take_events(&mut zero()).contains(&LobbyEvent::MatchStarting));
        let m = l.match_setup(&src(&drawable, &h, &p), &mut zero());
        assert_eq!(m.setup.team_style, TeamStyle::TwoVsTwo);
        assert_eq!(m.setup.players.len(), 3);
        assert_eq!(m.me, 1);
        assert_eq!(m.usernames, vec!["Finn", "Me", "Batman"]);
    }
}
