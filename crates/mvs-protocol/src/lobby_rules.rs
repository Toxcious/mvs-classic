//! The custom lobby's rules: its settings, their bounds and defaults, the team and slot
//! rules a lobby keeps, shared by the menus and the host core.
//!
//! Unverified: the native rules (slot counts, what a team style change does to the players) are not decoded; the
//! ones here are the port's. The start blockers are the build's (getStartMatchBlockers).
//!
//! `LobbyData` is the lobby as it travels from the host core to its members (lobby.state).

use serde::{Deserialize, Serialize};

/// ETeamStyle of the lobby: the Game Mode drop's IDs by value.
pub const TEAM_STYLE_IDS: [&str; 3] = ["1v1", "2v2", "ffa"];
/// The Free For All style's value.
pub const STYLE_FFA: u8 = 2;

/// A lobby row (LobbyPlayer).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LobbyPlayer {
    pub account_id: String,
    pub username: String,
    pub is_bot: bool,
    /// Bots: the character (a roster key) in place of the build's CharacterData asset path; '' none.
    pub character: String,
    /// Bots: the skin's SkinData asset name, '' the default.
    pub skin: String,
    /// EBotDifficulty (2 very easy .. 5 hard); 0 for a human.
    pub difficulty: i32,
    /// A human's ProfileIconPath as its ProfileIconData asset name, '' none (absent on the wire: a bot, or none).
    /// Every empty icon is left out, which a reader (`icon ?? ''`) takes the same.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
}

/// The lobby's own settings beyond the build's (LOBBY_OPTIONS), by key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OptionKey {
    /// Listed in the custom game browser; private lobbies are joined by their code alone.
    Public,
    /// An experimental rule: each player has the ring-out count as lives, the last team standing wins.
    Stocks,
    /// Only the players a member invited may join.
    InvitedOnly,
    /// An experimental rule: the match's one mutator, an index in `LOBBY_MUTATORS` (0 none).
    Mutator,
}

impl OptionKey {
    pub const ALL: [OptionKey; 4] = [OptionKey::Public, OptionKey::Stocks, OptionKey::InvitedOnly, OptionKey::Mutator];
}

/// One setting's value: a bool row's true or false, a choice row's listed value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionValue {
    Bool(bool),
    Choice(usize),
}

/// The count of values of the mutator choice (LOBBY_OPTIONS.mutator.values 0..=17).
pub const MUTATOR_CHOICES: usize = 18;

/// The lobby settings' values (LobbyOptions). A new lobby starts with every option at its initial value; a lobby
/// kept across its matches keeps them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LobbyOptions {
    pub public: bool,
    pub stocks: bool,
    pub invited_only: bool,
    pub mutator: usize,
}

impl LobbyOptions {
    pub fn get(&self, key: OptionKey) -> OptionValue {
        match key {
            OptionKey::Public => OptionValue::Bool(self.public),
            OptionKey::Stocks => OptionValue::Bool(self.stocks),
            OptionKey::InvitedOnly => OptionValue::Bool(self.invited_only),
            OptionKey::Mutator => OptionValue::Choice(self.mutator),
        }
    }

    /// `set` as the lobby takes a member's request (optionValue): a bool row takes a bool, a choice row a listed
    /// value; anything else is refused (false).
    pub fn set(&mut self, key: OptionKey, value: OptionValue) -> bool {
        match (key, value) {
            (OptionKey::Public, OptionValue::Bool(b)) => self.public = b,
            (OptionKey::Stocks, OptionValue::Bool(b)) => self.stocks = b,
            (OptionKey::InvitedOnly, OptionValue::Bool(b)) => self.invited_only = b,
            (OptionKey::Mutator, OptionValue::Choice(n)) if n < MUTATOR_CHOICES => self.mutator = n,
            _ => return false,
        }
        true
    }
}

/// The mutators the lobby's mutator option offers, by its value: MutatorData_Map keys, the ones the port runs. ''
/// at 0: none.
pub const LOBBY_MUTATORS: [&str; MUTATOR_CHOICES] = [
    "",
    "Mutator_RandomMutatorSelector",
    "Mutator_BigHeadAbilityBoost",
    "Mutator_GrowWithDamage",
    "Mutator_Giant",
    "Mutator_Miniature",
    "Mutator_LowGravity",
    "Mutator_InstantKill",
    "Mutator_GreyHealth",
    "Mutator_Boost_UnlimitedJumps",
    "Mutator_Boost_CooldownReductionLarge",
    "Mutator_Boost_AttacksBreakArmor",
    "Mutator_Boost_AdditionalAirSpecial",
    "Mutator_Boost_WeakenOnHit",
    "Mutator_Boost_ChargeFreeze",
    "Mutator_Boost_IceGauntlets",
    "Mutator_RandomItemSpawns",
    "Mutator_ShrinkBlastBoxLastPoint",
];

/// The blocker texts of the build (exe strings CustomGameLobbyError_*), in the order of the native table.
pub const BLOCKER_NOT_ENOUGH: &str = "Not enough players for selected game mode";
pub const BLOCKER_TOO_MANY: &str = "Too many players for selected game mode";
pub const BLOCKER_UNBALANCED: &str = "Teams are unbalanced";
pub const BLOCKER_NO_MAPS: &str = "No maps are selected";
pub const BLOCKER_NOT_READY: &str = "Not all players are ready";

/// A bot's name in the lobby and the match, after its character.
/// Deviation: the build's is the backend's, not in its files; the lobby's row shows the CharacterData's DisplayName.
pub fn bot_name(character: &str) -> String {
    let tail = character.rsplit(['/', '.']).next().unwrap_or("");
    // ^C\d+_?
    let digits_at =
        tail.strip_prefix('C').map_or(0, |r| r.len() - r.trim_start_matches(|c: char| c.is_ascii_digit()).len());
    let name = if digits_at > 0 {
        let r = &tail[1 + digits_at..];
        r.strip_prefix('_').unwrap_or(r)
    } else {
        tail
    };
    let mut cs = name.chars();
    match cs.next() {
        Some(c) => c.to_uppercase().chain(cs).collect(),
        None => "Bot".to_string(),
    }
}

/// A player of the set-up the lobby starts (MatchSetupWire's player: a bot's difficulty, an optional character,
/// no perks yet).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupPlayer {
    pub bot: bool,
    /// EBotDifficulty - 2 (level 0..3) of a bot; 0 of a human.
    pub difficulty: i32,
    pub character: Option<String>,
}

/// The rules of a started lobby's match (MatchSetupWire.rules).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetupRules {
    pub knockouts_to_win: u32,
    pub duration: u32,
    pub stocks: bool,
}

/// The lobby's match set-up (MatchSetupWire as `matchSetup` makes it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbySetup {
    pub team_style: u8,
    pub players: Vec<SetupPlayer>,
    pub rules: SetupRules,
    pub flow: bool,
    pub allow_hazards: bool,
    /// The one mutator (MatchSetupWire mutators), when the option is on.
    pub mutators: Vec<String>,
}

/// The started match config with the account of each player index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartedMatch {
    pub setup: LobbySetup,
    pub usernames: Vec<String>,
    pub ids: Vec<String>,
}

/// The lobby's state and its rules (LobbyRules).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyRules {
    pub team_style: u8,
    pub match_duration: u32,
    pub num_ringouts_for_win: u32,
    /// Unverified: the lobby's initial hazards value is native and not decoded.
    pub allow_hazards: bool,
    /// MapData names.
    /// Deviation: the lobby's first maps are the backend's; here the one custom-rotation map the port has a stage
    /// for (Sky Arena: CustomDuos and CustomFreeForAll).
    pub enabled_maps: Vec<String>,
    pub leader_id: String,
    pub teams: [Vec<LobbyPlayer>; 2],
    /// The ready humans (CachedReadyPlayers).
    /// Deviation: the port's own READY button; the build's lobby (WBP_CustomGame) has no ready button and no call
    /// that sets ready; its ReadyUsers are the backend's. The leader is ready while their lobby screen is up (a
    /// local lobby's one human is its leader); anyone else by `set_ready`. A match start empties it.
    pub ready: Vec<String>,
    pub options: LobbyOptions,
    bots: u32,
}

impl Default for LobbyRules {
    fn default() -> LobbyRules {
        LobbyRules {
            team_style: 1,
            match_duration: 420,
            num_ringouts_for_win: 4,
            allow_hazards: true,
            enabled_maps: vec!["map_m011_largenoplat".to_string()],
            leader_id: String::new(),
            teams: [Vec::new(), Vec::new()],
            ready: Vec::new(),
            options: LobbyOptions::default(),
            bots: 0,
        }
    }
}

/// The lobby as it travels from the host core to its members (LobbyData).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LobbyData {
    pub team_style: u8,
    pub match_duration: u32,
    pub num_ringouts_for_win: u32,
    pub allow_hazards: bool,
    pub enabled_maps: Vec<String>,
    pub leader_id: String,
    pub teams: Vec<Vec<LobbyPlayer>>,
    pub ready: Vec<String>,
    pub options: LobbyOptions,
}

impl LobbyRules {
    pub fn new() -> LobbyRules {
        LobbyRules::default()
    }

    /// The lobby as it travels (data).
    pub fn data(&self) -> LobbyData {
        LobbyData {
            team_style: self.team_style,
            match_duration: self.match_duration,
            num_ringouts_for_win: self.num_ringouts_for_win,
            allow_hazards: self.allow_hazards,
            enabled_maps: self.enabled_maps.clone(),
            leader_id: self.leader_id.clone(),
            teams: self.teams.to_vec(),
            ready: self.ready.clone(),
            options: self.options,
        }
    }

    /// The lobby as the host core sent it (load): every field taken, the options as sent.
    pub fn load(&mut self, d: &LobbyData) {
        self.team_style = d.team_style;
        self.match_duration = d.match_duration;
        self.num_ringouts_for_win = d.num_ringouts_for_win;
        self.allow_hazards = d.allow_hazards;
        self.enabled_maps = d.enabled_maps.clone();
        self.leader_id = d.leader_id.clone();
        let mut teams = d.teams.iter().cloned();
        self.teams = [teams.next().unwrap_or_default(), teams.next().unwrap_or_default()];
        self.ready = d.ready.clone();
        self.options = d.options;
    }

    /// Both teams' players in order.
    pub fn players(&self) -> Vec<&LobbyPlayer> {
        self.teams[0].iter().chain(self.teams[1].iter()).collect()
    }

    pub fn player_count(&self) -> usize {
        self.teams[0].len() + self.teams[1].len()
    }

    pub fn find(&self, account_id: &str) -> Option<&LobbyPlayer> {
        self.teams.iter().flatten().find(|p| p.account_id == account_id)
    }

    pub fn find_mut(&mut self, account_id: &str) -> Option<&mut LobbyPlayer> {
        self.teams.iter_mut().flatten().find(|p| p.account_id == account_id)
    }

    /// Players a team takes; ffa: the lobby's total.
    pub fn cap(&self) -> usize {
        match self.team_style {
            0 => 1,
            1 => 2,
            _ => 4,
        }
    }

    pub fn get_available_player_slot_count(&self) -> usize {
        let total = if self.team_style == STYLE_FFA { self.cap() } else { self.cap() * 2 };
        total.saturating_sub(self.player_count())
    }

    /// GetAvailableTeamIndex: the smaller team; None when the lobby is full.
    pub fn get_available_team_index(&self) -> Option<usize> {
        if self.get_available_player_slot_count() == 0 {
            return None;
        }
        Some(usize::from(self.teams[1].len() < self.teams[0].len()))
    }

    pub fn set_team_style(&mut self, style: u8) {
        if style == self.team_style {
            return;
        }
        self.team_style = style;
        if style != STYLE_FFA {
            self.rebalance();
        }
    }

    /// A team over its size hands players to the other team while that has room, its last bots first; the bots
    /// left over go. A human with no place stays (the lobby then has too many players to start).
    fn rebalance(&mut self) {
        let cap = self.cap();
        for t in 0..2 {
            let mut team = std::mem::take(&mut self.teams[t]);
            let other = &mut self.teams[1 - t];
            while team.len() > cap {
                let at = team.iter().rposition(|p| p.is_bot);
                if at.is_none() && other.len() >= cap {
                    break;
                }
                let p = team.remove(at.unwrap_or(team.len() - 1));
                if other.len() < cap {
                    other.push(p);
                }
            }
            self.teams[t] = team;
        }
    }

    pub fn add_bot(&mut self, team: usize, character: &str, skin: &str, difficulty: i32) {
        if self.get_available_player_slot_count() == 0 {
            return;
        }
        self.bots += 1;
        self.teams[team].push(LobbyPlayer {
            account_id: format!("bot-{}", self.bots),
            username: bot_name(character),
            is_bot: true,
            character: character.to_string(),
            skin: skin.to_string(),
            difficulty,
            icon: String::new(),
        });
    }

    /// A human joins: false when the lobby is full. `icon`: their profile icon's asset name ('' none).
    pub fn add_player(&mut self, account_id: &str, username: &str, icon: &str) -> bool {
        if self.find(account_id).is_some() {
            return true;
        }
        let Some(team) = self.get_available_team_index() else { return false };
        self.teams[team].push(LobbyPlayer {
            account_id: account_id.to_string(),
            username: username.to_string(),
            is_bot: false,
            character: String::new(),
            skin: String::new(),
            difficulty: 0,
            icon: icon.to_string(),
        });
        if self.leader_id.is_empty() {
            self.leader_id = account_id.to_string();
        }
        true
    }

    /// A human's profile icon changed: true when the row changed.
    pub fn set_icon(&mut self, account_id: &str, icon: &str) -> bool {
        match self.find_mut(account_id) {
            Some(p) if !p.is_bot && p.icon != icon => {
                p.icon = icon.to_string();
                true
            }
            _ => false,
        }
    }

    /// Kick and leave. When the leader goes the first human left leads.
    pub fn remove_player(&mut self, account_id: &str) -> Option<LobbyPlayer> {
        let mut gone = None;
        for team in &mut self.teams {
            if let Some(at) = team.iter().position(|p| p.account_id == account_id) {
                gone = Some(team.remove(at));
            }
        }
        self.ready.retain(|id| id != account_id);
        if account_id == self.leader_id {
            self.leader_id =
                self.players().into_iter().find(|p| !p.is_bot).map(|p| p.account_id.clone()).unwrap_or_default();
        }
        gone
    }

    pub fn join_team(&mut self, account_id: &str, team: usize) {
        let Some(at) = self.teams[1 - team].iter().position(|p| p.account_id == account_id) else { return };
        if self.team_style == STYLE_FFA || self.teams[team].len() < self.cap() {
            let p = self.teams[1 - team].remove(at);
            self.teams[team].push(p);
        }
    }

    pub fn update_settings_for_bot(&mut self, account_id: &str, character: &str, difficulty: i32, skin: &str) {
        if let Some(p) = self.find_mut(account_id)
            && p.is_bot
        {
            p.username = bot_name(character);
            p.character = character.to_string();
            p.difficulty = difficulty;
            p.skin = skin.to_string();
        }
    }

    /// (the port's own) A human who is not the leader presses READY or takes it back.
    pub fn set_ready(&mut self, account_id: &str, on: bool) {
        match self.find(account_id) {
            Some(p) if !p.is_bot && account_id != self.leader_id => {}
            _ => return,
        }
        self.ready.retain(|id| id != account_id);
        if on {
            self.ready.push(account_id.to_string());
        }
    }

    /// SetMaps.
    /// Deviation: a team style change leaves the maps as they are (the backend's rule is not in the build).
    pub fn set_maps(&mut self, maps: &[String]) {
        let mut out: Vec<String> = Vec::new();
        for m in maps {
            if !out.contains(m) {
                out.push(m.clone());
            }
        }
        self.enabled_maps = out;
    }

    pub fn promote_new_leader(&mut self, account_id: &str) -> bool {
        match self.find(account_id) {
            Some(p) if !p.is_bot && account_id != self.leader_id => {
                self.leader_id = account_id.to_string();
                true
            }
            _ => false,
        }
    }

    pub fn set_option(&mut self, key: OptionKey, value: OptionValue) -> bool {
        self.options.set(key, value)
    }

    /// The native blocker list (exe 1410c5c20, behind CanStartMatch 1410b5063, GetStartMatchBlockers 1410c654e and
    /// StartMatch 1410e3d82), in its order:
    ///   InvalidTeams        fewer than 2 teams (never here)
    ///   NotEnoughPlayers    players (bots count: 1410c4bb0) < the style's number, which is 2 for 1v1 and 4 for
    ///                       every other style (@1410c5ebd..5ed8)
    ///   TooManyPlayers      players > the same number (@1410c5fa5..5fb1)
    ///   UnbalancedTeams     not Free For All, and the teams differ in size (@1410c5dd0..6074)
    ///   TooManySpectators   more than 4 (no spectators here)
    ///   NoMapsSelected      no enabled map (@1410c61c3)
    ///   NotAllPlayersReady  the ready count differs from the count of players that are not bots (@1410c6290..6369)
    ///   YouAreNotTheLeader  (the widget asks for the leader only)
    pub fn get_start_match_blockers(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        let n = self.player_count();
        let ffa = self.team_style == STYLE_FFA;
        let need = if self.team_style == 0 { 2 } else { 4 };
        if n < need {
            out.push(BLOCKER_NOT_ENOUGH);
        }
        if n > need {
            out.push(BLOCKER_TOO_MANY);
        }
        if !ffa && self.teams[0].len() != self.teams[1].len() {
            out.push(BLOCKER_UNBALANCED);
        }
        if self.enabled_maps.is_empty() {
            out.push(BLOCKER_NO_MAPS);
        }
        if self.ready.len() != self.players().iter().filter(|p| !p.is_bot).count() {
            out.push(BLOCKER_NOT_READY);
        }
        out
    }

    pub fn can_start_match(&self) -> bool {
        self.get_start_match_blockers().is_empty()
    }

    /// The active match config of the started match, with the account of each player index. The team index of a
    /// player is PlayerIndex % team count (AssignPlayerAndTeamIndex), so the two teams' players alternate; Free
    /// For All keeps the lobby order.
    /// Unverified: the native order of the player indexes is not decoded. FreeForAll has no ring-out count
    /// ("Highest Score Wins"). A bot's EBotDifficulty 2..5 is level 0..3. AllowHazards goes into the set-up
    /// (MatchSetup.allowHazards: the stage's create hook reads it). A bot takes its lobby character; its skin goes
    /// to the pre-match screen with its row and from that screen's lock into the match (MatchPlayer.skin).
    pub fn match_setup(&self) -> StartedMatch {
        let ffa = self.team_style == STYLE_FFA;
        let stocks = self.options.stocks;
        let mutator = LOBBY_MUTATORS.get(self.options.mutator).copied().unwrap_or("");
        let mut order: Vec<&LobbyPlayer> = Vec::new();
        if ffa {
            order.extend(self.players());
        } else {
            for k in 0..self.teams[0].len().max(self.teams[1].len()) {
                for t in &self.teams {
                    if let Some(p) = t.get(k) {
                        order.push(p);
                    }
                }
            }
        }
        let character = |p: &LobbyPlayer| if p.character.is_empty() { None } else { Some(p.character.clone()) };
        StartedMatch {
            setup: LobbySetup {
                team_style: self.team_style,
                players: order
                    .iter()
                    .map(|p| SetupPlayer {
                        bot: p.is_bot,
                        difficulty: if p.is_bot { p.difficulty - 2 } else { 0 },
                        character: character(p),
                    })
                    .collect(),
                rules: SetupRules {
                    knockouts_to_win: if stocks || !ffa { self.num_ringouts_for_win } else { 0 },
                    duration: self.match_duration,
                    stocks,
                },
                flow: true,
                allow_hazards: self.allow_hazards,
                mutators: if mutator.is_empty() { Vec::new() } else { vec![mutator.to_string()] },
            },
            usernames: order.iter().map(|p| p.username.clone()).collect(),
            ids: order.iter().map(|p| p.account_id.clone()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lobby() -> LobbyRules {
        let mut r = LobbyRules::new();
        r.add_player("local", "Me", "");
        r.ready = vec!["local".into()];
        r
    }

    #[test]
    fn bot_names_follow_the_character() {
        assert_eq!(bot_name("shaggy"), "Shaggy");
        assert_eq!(bot_name("C017_IronGiant"), "IronGiant");
        assert_eq!(bot_name("/Game/Characters/C003_Batman.C003_Batman"), "Batman");
        assert_eq!(bot_name("C12"), "Bot");
        assert_eq!(bot_name(""), "Bot");
    }

    #[test]
    fn a_new_lobby_is_a_2v2_with_one_human() {
        let r = lobby();
        assert_eq!(r.leader_id, "local");
        assert_eq!(r.get_available_player_slot_count(), 3);
        assert_eq!(r.get_start_match_blockers(), vec![BLOCKER_NOT_ENOUGH, BLOCKER_UNBALANCED]);
    }

    #[test]
    fn a_full_balanced_lobby_starts_and_alternates_its_teams() {
        let mut r = lobby();
        r.add_bot(0, "shaggy", "", 3);
        r.add_bot(1, "batman", "", 5);
        r.add_bot(1, "finn", "", 2);
        assert!(r.can_start_match());
        let m = r.match_setup();
        assert_eq!(m.ids, vec!["local", "bot-2", "bot-1", "bot-3"]);
        assert_eq!(m.usernames, vec!["Me", "Batman", "Shaggy", "Finn"]);
        assert_eq!(m.setup.players[1].difficulty, 3);
        assert_eq!(m.setup.players[3].difficulty, 0);
        assert_eq!(m.setup.rules, SetupRules { knockouts_to_win: 4, duration: 420, stocks: false });
    }

    #[test]
    fn a_style_change_hands_the_extra_players_over() {
        let mut r = lobby();
        r.add_bot(0, "shaggy", "", 3);
        r.add_bot(1, "batman", "", 3);
        r.set_team_style(2);
        r.set_team_style(0);
        // 1v1 takes one per team: the bots over go
        assert_eq!(r.player_count(), 2);
        assert_eq!(r.teams[0].len(), 1);
    }

    #[test]
    fn ffa_has_no_ringout_count_unless_stocks() {
        let mut r = lobby();
        r.set_team_style(2);
        assert_eq!(r.match_setup().setup.rules.knockouts_to_win, 0);
        assert!(r.set_option(OptionKey::Stocks, OptionValue::Bool(true)));
        assert_eq!(r.match_setup().setup.rules.knockouts_to_win, 4);
        assert!(!r.set_option(OptionKey::Mutator, OptionValue::Choice(99)));
        assert!(!r.set_option(OptionKey::Public, OptionValue::Choice(1)));
    }

    #[test]
    fn leaving_moves_the_lead_and_the_ready_list() {
        let mut r = lobby();
        r.add_player("b", "B", "");
        r.set_ready("b", true);
        assert_eq!(r.ready, vec!["local", "b"]);
        r.remove_player("local");
        assert_eq!(r.leader_id, "b");
        assert_eq!(r.ready, vec!["b"]);
        assert!(!r.promote_new_leader("local"));
    }

    #[test]
    fn maps_are_kept_once_each() {
        let mut r = lobby();
        r.set_maps(&["a".into(), "b".into(), "a".into()]);
        assert_eq!(r.enabled_maps, vec!["a", "b"]);
        r.set_maps(&[]);
        assert!(r.get_start_match_blockers().contains(&BLOCKER_NO_MAPS));
    }
}
