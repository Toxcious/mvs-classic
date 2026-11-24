//! The custom lobby's rules: its settings, their bounds and defaults, the team and slot
//! rules a lobby keeps, shared by the menus and the host core.
//!
//! Unverified: the native rules (slot counts, what a team style change does to the players) are not decoded; the
//! ones here are the port's. The start blockers are the build's (getStartMatchBlockers).
//!
//! `LobbyData` is the lobby as it travels from the host core to its members (lobby.state).

use serde::{Deserialize, Serialize};

use crate::setup::{FREE_FOR_ALL, ONE_VS_ONE, TWO_VS_TWO, TeamStyle, WheelWire};

/// The lobby's style, its Game Mode drop: on the wire its number (`teamStyle`, `lobby.setTeamStyle`, a listing's
/// `style`). The match it starts has the ETeamStyle `match_setup` derives from its teams.
/// Deviation: the build's lobby offers ETeamStyle's 1 vs 1, Teams (2 vs 2) and Free For All; the port's offers
/// Versus (two teams of up to 4, any split with a player on each side) and Free For All (4 to 8 players).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum LobbyStyle {
    #[default]
    Versus = 0,
    FreeForAll = 1,
}

impl LobbyStyle {
    pub const ALL: [LobbyStyle; 2] = [LobbyStyle::Versus, LobbyStyle::FreeForAll];

    /// The Game Mode drop's item id.
    pub fn id(self) -> &'static str {
        match self {
            LobbyStyle::Versus => "versus",
            LobbyStyle::FreeForAll => "ffa",
        }
    }

    pub fn from_id(id: &str) -> Option<LobbyStyle> {
        LobbyStyle::ALL.into_iter().find(|s| s.id() == id)
    }

    /// Players a team takes; Free For All: the lobby's total.
    /// Deviation: the port's own big matches; the build's lobby takes 2 a team and a Free For All of 4.
    pub fn cap(self) -> usize {
        match self {
            LobbyStyle::Versus => MAX_LOBBY_PLAYERS / 2,
            LobbyStyle::FreeForAll => MAX_LOBBY_PLAYERS,
        }
    }
}

/// The most players a lobby holds, bots included: the sim's `MAX_PLAYERS` and the wire's `MAX_SLOTS`.
pub const MAX_LOBBY_PLAYERS: usize = crate::match_wire::MAX_SLOTS;
/// The fewest players of a Free For All (the build's lobby: exactly this many).
pub const FFA_MIN_PLAYERS: usize = 4;
/// The most players of a match the build has (a lobby of more is a big match: only the maps that hold it).
pub const BUILD_MAX_PLAYERS: usize = 4;

impl TryFrom<u8> for LobbyStyle {
    type Error = String;

    fn try_from(n: u8) -> Result<LobbyStyle, String> {
        match n {
            0 => Ok(LobbyStyle::Versus),
            1 => Ok(LobbyStyle::FreeForAll),
            _ => Err(format!("no lobby style {n}")),
        }
    }
}

impl From<LobbyStyle> for u8 {
    fn from(s: LobbyStyle) -> u8 {
        s as u8
    }
}

/// EBotDifficulty of a lobby's bot: Very Easy, the lowest.
pub const BOT_DIFFICULTY_MIN: i32 = 2;
/// (the port's own) The lobby's difficulty of the Expert bot, after EBotDifficulty's Hard (5), the highest a lobby's
/// bot takes: its match level is difficulty - 2, the Expert level (4). A host core older than it clamps it to Hard.
pub const BOT_DIFFICULTY_EXPERT: i32 = 6;

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
    /// EBotDifficulty (2 very easy .. 5 hard, 6 the port's own Expert); 0 for a human.
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
    /// An experimental rule: the match's mutators, a set of `LOBBY_MUTATORS` (`MutatorSet`), each run at once.
    Mutators,
    /// The Chaos Wheel (`wheel_rule`): a mutator of `mvs_mutators::ALL` rolled for every fighter every so often, as
    /// the lobby's `WheelOptions` set it.
    Wheel,
}

impl OptionKey {
    pub const ALL: [OptionKey; 5] =
        [OptionKey::Public, OptionKey::Stocks, OptionKey::InvitedOnly, OptionKey::Mutators, OptionKey::Wheel];
}

/// One setting's value: a bool row's true or false, a set row's set, the Chaos Wheel's choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionValue {
    Bool(bool),
    Set(MutatorSet),
    Wheel(WheelOptions),
}

/// The Chaos Wheel's window lengths the lobby offers, seconds.
pub const WHEEL_SECONDS_CHOICES: [u32; 6] = [10, 15, 20, 30, 45, 60];
/// The Chaos Wheel's warnings the lobby offers, seconds ahead of a change.
pub const WHEEL_WARN_CHOICES: [u32; 2] = [3, 5];

/// The Chaos Wheel's choices a lobby holds (the CHAOS WHEEL screen); on the wire an object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WheelOptions {
    pub on: bool,
    /// A window's length, one of `WHEEL_SECONDS_CHOICES`.
    pub seconds: u32,
    /// How far ahead of a change the next one is spun, one of `WHEEL_WARN_CHOICES`.
    pub warn: u32,
    /// `mvs_mutators::ChaosRate` by its place in `ChaosRate::ALL`.
    pub chaos: u8,
    /// Two at once late in the match.
    pub two: bool,
    /// The pool's left-out entries: bit n is `mvs_mutators::ALL[n]`, so a mutator added later is in.
    pub off: u64,
}

impl Default for WheelOptions {
    fn default() -> WheelOptions {
        WheelOptions { on: false, seconds: WHEEL_SECONDS, warn: WHEEL_WARN, chaos: 2, two: false, off: 0 }
    }
}

impl WheelOptions {
    /// The default choices (left off the wire, as the option was before it had choices).
    pub fn is_default(&self) -> bool {
        *self == WheelOptions::default()
    }

    /// The rate of Chaos mutators.
    pub fn rate(self) -> mvs_mutators::ChaosRate {
        mvs_mutators::ChaosRate::ALL[usize::from(self.chaos.min(3))]
    }

    /// Whether `mvs_mutators::ALL[i]` is in the pool.
    pub fn has(self, i: usize) -> bool {
        i < mvs_mutators::ALL.len() && self.off & (1 << i) == 0
    }

    /// The options with `mvs_mutators::ALL[i]` in (`on`) or out.
    #[must_use]
    pub fn with(self, i: usize, on: bool) -> WheelOptions {
        if i >= mvs_mutators::ALL.len() {
            return self;
        }
        let off = if on { self.off & !(1 << i) } else { self.off | (1 << i) };
        WheelOptions { off, ..self }
    }

    /// The pool's keys, in `mvs_mutators::ALL`'s order.
    pub fn pool(self) -> Vec<String> {
        mvs_mutators::ALL.iter().enumerate().filter(|&(i, _)| self.has(i)).map(|(_, m)| m.key.to_owned()).collect()
    }

    /// Choices a member may send: a listed window and warning, a rate, and a pool of at least one listed mutator.
    pub fn valid(self) -> bool {
        let all = if mvs_mutators::ALL.len() >= 64 { u64::MAX } else { (1u64 << mvs_mutators::ALL.len()) - 1 };
        WHEEL_SECONDS_CHOICES.contains(&self.seconds)
            && WHEEL_WARN_CHOICES.contains(&self.warn)
            && self.chaos <= 3
            && self.off & !all == 0
            && self.off & all != all
    }
}

/// The count of `LOBBY_MUTATORS`' entries, the empty one at 0 with them.
pub const MUTATOR_CHOICES: usize = 28;

/// A set of `LOBBY_MUTATORS`, by index: bit n is entry n (bit 0, the empty entry, never set). On the wire, the
/// number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MutatorSet(pub u32);

impl MutatorSet {
    /// Every listed mutator's bit.
    pub const ALL: u32 = ((1 << MUTATOR_CHOICES) - 1) & !1;

    /// The set of these bits, None when one is not a listed mutator's.
    pub fn of(bits: u32) -> Option<MutatorSet> {
        (bits & !Self::ALL == 0).then_some(MutatorSet(bits))
    }

    pub fn has(self, n: usize) -> bool {
        n < MUTATOR_CHOICES && self.0 & (1 << n) != 0
    }

    /// The set with entry `n` in (`on`) or out; a value past the list is ignored.
    #[must_use]
    pub fn with(self, n: usize, on: bool) -> MutatorSet {
        if n == 0 || n >= MUTATOR_CHOICES {
            return self;
        }
        MutatorSet(if on { self.0 | (1 << n) } else { self.0 & !(1 << n) })
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    /// The indexes in the set, in the list's order.
    pub fn indexes(self) -> impl Iterator<Item = usize> {
        (1..MUTATOR_CHOICES).filter(move |&n| self.has(n))
    }

    /// The MutatorData_Map keys of the set, in the list's order: the match's mutators.
    pub fn names(self) -> Vec<String> {
        self.indexes().map(|n| LOBBY_MUTATORS[n].to_string()).collect()
    }
}

/// The lobby settings' values (LobbyOptions). A new lobby starts with every option at its initial value; a lobby
/// kept across its matches keeps them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LobbyOptions {
    pub public: bool,
    pub stocks: bool,
    pub invited_only: bool,
    pub mutators: MutatorSet,
    #[serde(default, skip_serializing_if = "WheelOptions::is_default")]
    pub wheel: WheelOptions,
}

impl LobbyOptions {
    pub fn get(&self, key: OptionKey) -> OptionValue {
        match key {
            OptionKey::Public => OptionValue::Bool(self.public),
            OptionKey::Stocks => OptionValue::Bool(self.stocks),
            OptionKey::InvitedOnly => OptionValue::Bool(self.invited_only),
            OptionKey::Mutators => OptionValue::Set(self.mutators),
            OptionKey::Wheel => OptionValue::Wheel(self.wheel),
        }
    }

    /// `set` as the lobby takes a member's request (optionValue): a bool row takes a bool, the mutators a set of
    /// listed ones, the wheel valid choices (`WheelOptions::valid`); anything else is refused (false).
    pub fn set(&mut self, key: OptionKey, value: OptionValue) -> bool {
        match (key, value) {
            (OptionKey::Public, OptionValue::Bool(b)) => self.public = b,
            (OptionKey::Stocks, OptionValue::Bool(b)) => self.stocks = b,
            (OptionKey::InvitedOnly, OptionValue::Bool(b)) => self.invited_only = b,
            (OptionKey::Mutators, OptionValue::Set(m)) if MutatorSet::of(m.0).is_some() => self.mutators = m,
            (OptionKey::Wheel, OptionValue::Wheel(o)) if o.valid() => self.wheel = o,
            _ => return false,
        }
        true
    }
}

/// The mutators the lobby's mutators option offers, by index (`MutatorSet`'s bits): MutatorData_Map keys, the ones
/// the port runs. '' at 0: none.
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
    // the port's own (`mvs_sim::world::own_rules`)
    "Chaos_MoonJump",
    "Chaos_Featherweight",
    "Chaos_Heavyweight",
    "Chaos_Vampire",
    "Chaos_DodgeFrenzy",
    "Chaos_LavaFloor",
    "Chaos_Ghosts",
    "Chaos_IceRink",
    "Chaos_Wind",
    "Chaos_Magnet",
];

/// The Chaos Wheel's window by default, seconds: the match plays one plain, then each has one mutator.
pub const WHEEL_SECONDS: u32 = 20;
/// How many seconds ahead of a change the Chaos Wheel's next mutator is drawn by default (the card names it).
pub const WHEEL_WARN: u32 = 3;

/// The Chaos Wheel rule a lobby starts its match with, None with the wheel off.
pub fn wheel_rule(o: WheelOptions) -> Option<WheelWire> {
    let rate = o.rate();
    o.on.then(|| WheelWire {
        pool: o.pool(),
        seconds: o.seconds,
        warn: o.warn,
        chaos: (rate != mvs_mutators::ChaosRate::Normal).then(|| rate.key().to_owned()),
        two: o.two,
    })
}

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
    /// EBotDifficulty - 2 (level 0..3, 4 Expert) of a bot; 0 of a human.
    pub difficulty: i32,
    pub character: Option<String>,
    /// The match's team: the lobby's column in Versus (the larger column's 0), the player index in Free For All.
    pub team: u32,
}

impl SetupPlayer {
    /// The team the wire names: None where it is the slot's own (slot % the match's team count, which a lobby of the
    /// build's sizes always gives), so those set-ups stay as they were.
    pub fn wire_team(&self, slot: usize, team_style: u8, players: usize) -> Option<u32> {
        let own = u32::try_from(slot).unwrap_or(u32::MAX) % crate::setup::team_count(team_style, players);
        (self.team != own).then_some(self.team)
    }
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
    /// The match's ETeamStyle.
    pub team_style: TeamStyle,
    pub players: Vec<SetupPlayer>,
    pub rules: SetupRules,
    pub flow: bool,
    pub allow_hazards: bool,
    /// The ticked mutators (MatchSetupWire mutators).
    pub mutators: Vec<String>,
    /// The Chaos Wheel rule (MatchSetupWire wheel), when the option is on.
    pub wheel: Option<WheelWire>,
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
    pub team_style: LobbyStyle,
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
            team_style: LobbyStyle::Versus,
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
    pub team_style: LobbyStyle,
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
        self.team_style.cap()
    }

    pub fn get_available_player_slot_count(&self) -> usize {
        let total = match self.team_style {
            LobbyStyle::Versus => self.cap() * 2,
            LobbyStyle::FreeForAll => self.cap(),
        };
        total.saturating_sub(self.player_count())
    }

    /// GetAvailableTeamIndex: the smaller team; None when the lobby is full.
    pub fn get_available_team_index(&self) -> Option<usize> {
        if self.get_available_player_slot_count() == 0 {
            return None;
        }
        Some(usize::from(self.teams[1].len() < self.teams[0].len()))
    }

    pub fn set_team_style(&mut self, style: LobbyStyle) {
        if style == self.team_style {
            return;
        }
        self.team_style = style;
        if style == LobbyStyle::Versus {
            self.rebalance();
        }
    }

    /// A team over its size hands players to the other team while that has room, its last bots first; the bots
    /// left over go. A human with no place stays (the lobby then has too many players to start). The size: the
    /// build's 2 a team, or half the lobby (rounded up) past four players, so the sides come out even.
    fn rebalance(&mut self) {
        let half = self.player_count().div_ceil(2);
        let cap = half.clamp(BUILD_MAX_PLAYERS / 2, self.cap());
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

    /// A bot joins team `team` (0 or 1): nothing when the lobby is full, or in Versus when that team is.
    pub fn add_bot(&mut self, team: usize, character: &str, skin: &str, difficulty: i32) {
        if self.get_available_player_slot_count() == 0
            || team > 1
            || (self.team_style == LobbyStyle::Versus && self.teams[team].len() >= self.cap())
        {
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
        if self.team_style == LobbyStyle::FreeForAll || self.teams[team].len() < self.cap() {
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
    ///
    /// Deviation: Versus (the port's own style) takes any split of up to 4 a team: NotEnoughPlayers while a team is
    /// empty, TooManyPlayers for a team over 4, no UnbalancedTeams. Free For All keeps the build's NotEnoughPlayers
    /// under 4 and takes up to 8 (the port's own big matches; the build's TooManyPlayers is over 4).
    pub fn get_start_match_blockers(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        let n = self.player_count();
        match self.team_style {
            LobbyStyle::Versus => {
                if self.teams.iter().any(Vec::is_empty) {
                    out.push(BLOCKER_NOT_ENOUGH);
                }
                if self.teams.iter().any(|t| t.len() > self.cap()) {
                    out.push(BLOCKER_TOO_MANY);
                }
            }
            LobbyStyle::FreeForAll => {
                if n < FFA_MIN_PLAYERS {
                    out.push(BLOCKER_NOT_ENOUGH);
                }
                if n > self.cap() {
                    out.push(BLOCKER_TOO_MANY);
                }
            }
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

    /// (the port's own) More players than a match of the build has: a big match, played only on the maps whose stage
    /// has a start for each of them.
    pub fn is_big(&self) -> bool {
        self.player_count() > BUILD_MAX_PLAYERS
    }

    /// The enabled maps the started match draws from: every one, or for a big match the ones `holds_big` says hold
    /// one of the lobby's style (the game's own stages decide it: the host core has no game data, so the leader's
    /// game sends this list with its start). Empty: the match cannot start.
    pub fn match_maps(&self, holds_big: &dyn Fn(&str, LobbyStyle) -> bool) -> Vec<String> {
        let big = self.is_big();
        self.enabled_maps.iter().filter(|m| !big || holds_big(m, self.team_style)).cloned().collect()
    }

    /// The active match config of the started match, with the account of each player index. The team index of a
    /// player is PlayerIndex % team count (AssignPlayerAndTeamIndex), so the two teams' players alternate, the larger
    /// team's first (a tie: the lobby's first team); Free For All keeps the lobby order. (the port's own) Each
    /// player's team is the lobby's column (`SetupPlayer::team`): sides uneven by more than one (3v1, 4v2) keep
    /// their columns, where the alternation alone would not. Versus's match is a 1v1 for
    /// one against one and a 2v2 for every other split, the build's rule for teams of uneven size
    /// (TrainGameMode.GetTeamStyleFromTrainingConfiguration: a team of 2 or more is 2v2).
    /// Deviation: when the lobby's second team is the larger (1v2), its players are the match's team 0 and the
    /// lobby's first team the match's team 1: the lobby's columns swap team index in the match.
    /// Unverified: the native order of the player indexes is not decoded. FreeForAll has no ring-out count
    /// ("Highest Score Wins"). A bot's EBotDifficulty 2..5 is level 0..3, the port's own BOT_DIFFICULTY_EXPERT (6)
    /// level 4. AllowHazards goes into the set-up (MatchSetup.allowHazards: the stage's create hook reads it). A bot
    /// takes its lobby character; its skin goes to the pre-match screen with its row and from that screen's lock
    /// into the match (MatchPlayer.skin).
    pub fn match_setup(&self) -> StartedMatch {
        let ffa = self.team_style == LobbyStyle::FreeForAll;
        let stocks = self.options.stocks;
        let mut order: Vec<(&LobbyPlayer, u32)> = Vec::new();
        let sizes = [self.teams[0].len(), self.teams[1].len()];
        let team_style = if ffa {
            order.extend(self.players().into_iter().zip(0..));
            FREE_FOR_ALL
        } else {
            let first = usize::from(sizes[1] > sizes[0]);
            for k in 0..sizes[0].max(sizes[1]) {
                for (t, team) in [(first, 0), (1 - first, 1)] {
                    if let Some(p) = self.teams[t].get(k) {
                        order.push((p, team));
                    }
                }
            }
            if sizes == [1, 1] { ONE_VS_ONE } else { TWO_VS_TWO }
        };
        let character = |p: &LobbyPlayer| if p.character.is_empty() { None } else { Some(p.character.clone()) };
        StartedMatch {
            setup: LobbySetup {
                team_style,
                players: order
                    .iter()
                    .map(|&(p, team)| SetupPlayer {
                        bot: p.is_bot,
                        difficulty: if p.is_bot { p.difficulty - 2 } else { 0 },
                        character: character(p),
                        team,
                    })
                    .collect(),
                rules: SetupRules {
                    knockouts_to_win: if stocks || !ffa { self.num_ringouts_for_win } else { 0 },
                    duration: self.match_duration,
                    stocks,
                },
                flow: true,
                allow_hazards: self.allow_hazards,
                mutators: self.options.mutators.names(),
                wheel: wheel_rule(self.options.wheel),
            },
            usernames: order.iter().map(|(p, _)| p.username.clone()).collect(),
            ids: order.iter().map(|(p, _)| p.account_id.clone()).collect(),
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
    fn a_new_lobby_is_a_versus_with_one_human() {
        let r = lobby();
        assert_eq!(r.team_style, LobbyStyle::Versus);
        assert_eq!(r.leader_id, "local");
        assert_eq!(r.get_available_player_slot_count(), 7);
        // an empty team blocks
        assert_eq!(r.get_start_match_blockers(), vec![BLOCKER_NOT_ENOUGH]);
    }

    #[test]
    fn the_style_travels_as_its_number() {
        let mut r = lobby();
        assert_eq!(serde_json::to_value(r.data()).unwrap()["teamStyle"], serde_json::json!(0));
        r.set_team_style(LobbyStyle::FreeForAll);
        assert_eq!(serde_json::to_value(r.data()).unwrap()["teamStyle"], serde_json::json!(1));
        assert!(serde_json::from_value::<LobbyStyle>(serde_json::json!(2)).is_err());
        assert_eq!(LobbyStyle::from_id("versus"), Some(LobbyStyle::Versus));
        assert_eq!(LobbyStyle::from_id("ffa"), Some(LobbyStyle::FreeForAll));
        assert_eq!(LobbyStyle::from_id("1v1"), None);
    }

    /// A Versus lobby of `a` players on its first team and `b` on its second (the human first), each a bot but it.
    fn versus(a: usize, b: usize) -> LobbyRules {
        let mut r = lobby();
        for _ in 1..a {
            r.add_bot(0, "shaggy", "", 3);
        }
        for _ in 0..b {
            r.add_bot(1, "finn", "", 3);
        }
        assert_eq!((r.teams[0].len(), r.teams[1].len()), (a, b));
        r
    }

    #[test]
    fn every_versus_split_starts_with_the_larger_team_on_even_slots() {
        // (split, the match's style, the order of the player indexes)
        let cases: [((usize, usize), u8, &[&str]); 4] = [
            ((1, 1), ONE_VS_ONE, &["local", "bot-1"]),
            ((2, 1), TWO_VS_TWO, &["local", "bot-2", "bot-1"]),
            ((1, 2), TWO_VS_TWO, &["bot-1", "local", "bot-2"]),
            ((2, 2), TWO_VS_TWO, &["local", "bot-2", "bot-1", "bot-3"]),
        ];
        for ((a, b), style, ids) in cases {
            let r = versus(a, b);
            assert!(r.can_start_match(), "{a}v{b}: {:?}", r.get_start_match_blockers());
            let m = r.match_setup();
            assert_eq!(m.setup.team_style, style, "{a}v{b}");
            assert_eq!(m.ids, ids, "{a}v{b}");
            // the larger team (the first on a tie) is the match's team 0: its players on the even slots
            let larger = usize::from(b > a);
            for (slot, id) in m.ids.iter().enumerate() {
                let lobby_team = usize::from(r.teams[1].iter().any(|p| &p.account_id == id));
                assert_eq!(slot % 2 == 0, lobby_team == larger, "{a}v{b} slot {slot}");
            }
        }
    }

    #[test]
    fn an_empty_versus_team_blocks_and_a_full_lobby_takes_no_more() {
        let mut r = versus(2, 0);
        assert_eq!(r.get_start_match_blockers(), vec![BLOCKER_NOT_ENOUGH]);
        r.join_team("local", 1);
        assert!(r.can_start_match(), "1v1 after a switch");
        let mut r = versus(4, 4);
        assert!(r.can_start_match(), "4v4: {:?}", r.get_start_match_blockers());
        assert_eq!(r.get_available_player_slot_count(), 0);
        r.add_bot(0, "batman", "", 3);
        assert_eq!(r.player_count(), 8);
        assert!(!r.add_player("ninth", "Ninth", ""), "a ninth join is refused");
        // a full team takes nobody from the other
        r.join_team("local", 1);
        assert_eq!(r.teams[0][0].account_id, "local");
    }

    #[test]
    fn ffa_keeps_the_builds_rules() {
        let mut r = versus(2, 1);
        r.set_team_style(LobbyStyle::FreeForAll);
        assert_eq!(r.get_start_match_blockers(), vec![BLOCKER_NOT_ENOUGH]);
        r.add_bot(0, "batman", "", 3);
        assert!(r.can_start_match());
        let m = r.match_setup();
        assert_eq!(m.setup.team_style, FREE_FOR_ALL);
        // the lobby's order
        assert_eq!(m.ids, vec!["local", "bot-1", "bot-3", "bot-2"]);
    }

    #[test]
    fn a_full_balanced_lobby_starts_and_alternates_its_teams() {
        let mut r = lobby();
        r.add_bot(0, "shaggy", "", 3);
        r.add_bot(1, "batman", "", 5);
        r.add_bot(1, "finn", "", BOT_DIFFICULTY_EXPERT);
        assert!(r.can_start_match());
        let m = r.match_setup();
        assert_eq!(m.ids, vec!["local", "bot-2", "bot-1", "bot-3"]);
        assert_eq!(m.usernames, vec!["Me", "Batman", "Shaggy", "Finn"]);
        assert_eq!(m.setup.players[1].difficulty, 3);
        assert_eq!(m.setup.players[3].difficulty, 4, "Expert's level");
        assert_eq!(m.setup.rules, SetupRules { knockouts_to_win: 4, duration: 420, stocks: false });
    }

    #[test]
    fn a_style_change_hands_the_extra_players_over() {
        let mut r = lobby();
        r.add_bot(0, "shaggy", "", 3);
        r.add_bot(1, "batman", "", 3);
        r.set_team_style(LobbyStyle::FreeForAll);
        for _ in 0..3 {
            r.add_bot(0, "finn", "", 3);
        }
        r.join_team("bot-2", 0);
        assert_eq!(r.teams[0].len(), 6);
        r.set_team_style(LobbyStyle::Versus);
        // six: the sides even out at three, the first team's last bots going over
        assert_eq!(r.player_count(), 6);
        assert_eq!(r.teams[0].len(), 3);
        assert_eq!(
            r.teams[1].iter().map(|p| p.account_id.as_str()).collect::<Vec<_>>(),
            vec!["bot-2", "bot-5", "bot-4"]
        );
        // four: two a team, as the build's lobby
        let mut r = lobby();
        r.add_bot(0, "shaggy", "", 3);
        r.add_bot(1, "batman", "", 3);
        r.set_team_style(LobbyStyle::FreeForAll);
        r.add_bot(0, "finn", "", 3);
        r.join_team("bot-2", 0);
        assert_eq!(r.teams[0].len(), 4);
        r.set_team_style(LobbyStyle::Versus);
        assert_eq!(r.teams[0].len(), 2);
        assert_eq!(r.teams[1].iter().map(|p| p.account_id.as_str()).collect::<Vec<_>>(), vec!["bot-2", "bot-3"]);
    }

    #[test]
    fn a_versus_team_takes_four_and_a_fifth_is_refused() {
        let mut r = versus(4, 1);
        assert!(r.can_start_match(), "4v1: {:?}", r.get_start_match_blockers());
        r.add_bot(0, "batman", "", 3);
        assert_eq!(r.teams[0].len(), 4, "a fifth bot on a full team");
        r.add_player("b", "B", "");
        assert_eq!(r.teams[1].len(), 2, "a human joins the smaller team");
        r.join_team("b", 0);
        assert_eq!(r.teams[0].len(), 4, "a fifth by a team switch");
        // a fifth by way of Free For All: the change back hands a bot over
        r.set_team_style(LobbyStyle::FreeForAll);
        r.join_team("b", 0);
        assert_eq!(r.teams[0].len(), 5);
        r.set_team_style(LobbyStyle::Versus);
        assert_eq!((r.teams[0].len(), r.teams[1].len()), (3, 3), "the switch to Versus evens the sides");
        // 3v3 and 4v3: the teams alternate, the larger first, as a 2v2's
        for (a, b) in [(3, 3), (4, 3), (4, 4)] {
            let r = versus(a, b);
            let m = r.match_setup();
            assert_eq!(m.setup.team_style, TWO_VS_TWO);
            assert_eq!(m.ids.len(), a + b);
            for (slot, id) in m.ids.iter().enumerate() {
                let lobby_team = usize::from(r.teams[1].iter().any(|p| &p.account_id == id));
                assert_eq!(slot % 2, lobby_team, "{a}v{b} slot {slot}");
            }
            assert!(r.is_big());
            assert_columns(&r);
        }
    }

    /// The started match's teams are the lobby's columns: the larger column is team 0 (a tie: the first), every
    /// player of a column on its team; Free For All a team per player.
    fn assert_columns(r: &LobbyRules) {
        let m = r.match_setup();
        let first = usize::from(r.teams[1].len() > r.teams[0].len());
        for (slot, id) in m.ids.iter().enumerate() {
            let team = m.setup.players[slot].team;
            if r.team_style == LobbyStyle::FreeForAll {
                assert_eq!(team, u32::try_from(slot).unwrap(), "{id}");
                continue;
            }
            let column = usize::from(r.teams[1].iter().any(|p| &p.account_id == id));
            assert_eq!(team, u32::from(column != first), "{id}: column {column}");
        }
        let n = m.setup.players.len();
        let own = |slot: usize| u32::try_from(slot).unwrap() % crate::setup::team_count(m.setup.team_style, n);
        // (the wire names a team only where it is not the slot's own)
        for (slot, p) in m.setup.players.iter().enumerate() {
            assert_eq!(p.wire_team(slot, m.setup.team_style, n), (p.team != own(slot)).then_some(p.team));
        }
    }

    #[test]
    fn a_match_plays_the_lobbys_columns() {
        // 3v1 by a team switch: the alternation alone would play it 2v2
        let mut r = versus(2, 1);
        r.add_player("b", "B", "");
        assert_eq!((r.teams[0].len(), r.teams[1].len()), (2, 2));
        r.join_team("b", 0);
        assert_eq!((r.teams[0].len(), r.teams[1].len()), (3, 1));
        assert_columns(&r);
        let m = r.match_setup();
        assert_eq!(m.setup.players.iter().map(|p| p.team).collect::<Vec<_>>(), [0, 1, 0, 0]);
        assert_eq!(m.setup.players[3].wire_team(3, m.setup.team_style, 4), Some(0), "slot 3 would be team 1");
        // 4v2
        let r = versus(4, 2);
        assert_columns(&r);
        assert_eq!(r.match_setup().setup.players.iter().filter(|p| p.team == 0).count(), 4);
        // 1v2: the larger column is team 0
        assert_columns(&versus(1, 2));
        // a four-player Free For All to Versus: a 2v2 again
        let mut r = lobby();
        r.set_team_style(LobbyStyle::FreeForAll);
        for _ in 0..3 {
            r.add_bot(0, "shaggy", "", 3);
        }
        assert_columns(&r);
        r.set_team_style(LobbyStyle::Versus);
        assert_eq!((r.teams[0].len(), r.teams[1].len()), (2, 2));
        assert_columns(&r);
        // seven (the leader and six bots) in Free For All to Versus: 4v3
        let mut r = lobby();
        r.set_team_style(LobbyStyle::FreeForAll);
        for _ in 0..6 {
            r.add_bot(0, "shaggy", "", 3);
        }
        assert_eq!(r.teams[0].len(), 7);
        assert_columns(&r);
        r.set_team_style(LobbyStyle::Versus);
        assert_eq!((r.teams[0].len(), r.teams[1].len()), (4, 3));
        assert_eq!(r.player_count(), 7, "no bot goes");
        assert_columns(&r);
    }

    #[test]
    fn a_free_for_all_takes_eight() {
        let mut r = lobby();
        r.set_team_style(LobbyStyle::FreeForAll);
        for _ in 0..8 {
            r.add_bot(0, "finn", "", 3);
        }
        assert_eq!(r.player_count(), 8);
        assert!(!r.add_player("ninth", "Ninth", ""), "a ninth join is refused");
        assert!(r.can_start_match(), "{:?}", r.get_start_match_blockers());
        let m = r.match_setup();
        assert_eq!(m.setup.team_style, FREE_FOR_ALL);
        assert_eq!(m.ids.len(), 8);
        assert_eq!(crate::setup::team_count(m.setup.team_style, m.ids.len()), 8);
        assert_eq!(crate::setup::team_count(FREE_FOR_ALL, 4), 4);
        assert_eq!(crate::setup::team_count(TWO_VS_TWO, 8), 2);
    }

    #[test]
    fn a_big_match_draws_only_the_maps_that_hold_it() {
        let mut r = versus(2, 2);
        r.set_maps(&["small".into(), "large".into()]);
        let holds = |m: &str, s: LobbyStyle| m == "large" && s == LobbyStyle::Versus;
        assert_eq!(r.match_maps(&holds), vec!["small", "large"], "four players: every map");
        r.add_bot(1, "finn", "", 3);
        assert_eq!(r.match_maps(&holds), vec!["large"]);
        r.set_team_style(LobbyStyle::FreeForAll);
        assert!(r.match_maps(&holds).is_empty());
    }

    #[test]
    fn the_wheel_options_make_the_set_ups_rule() {
        let mut r = lobby();
        assert_eq!(r.match_setup().setup.wheel, None);
        assert!(!r.set_option(OptionKey::Wheel, OptionValue::Bool(true)));
        let on = WheelOptions { on: true, ..WheelOptions::default() };
        assert!(r.set_option(OptionKey::Wheel, OptionValue::Wheel(on)));
        let w = r.match_setup().setup.wheel.expect("the rule");
        assert_eq!(
            (w.pool.len(), w.seconds, w.warn, w.chaos.as_deref(), w.two),
            (mvs_mutators::ALL.len(), 20, 3, None, false)
        );
        // the choices: a window, a warning, a rate, two at once and a pool without its first
        let o = WheelOptions { seconds: 45, warn: 5, chaos: 0, two: true, ..on.with(0, false) };
        assert!(r.set_option(OptionKey::Wheel, OptionValue::Wheel(o)));
        let w = r.match_setup().setup.wheel.expect("the rule");
        assert_eq!(
            (w.pool.len(), w.seconds, w.warn, w.chaos.as_deref(), w.two),
            (mvs_mutators::ALL.len() - 1, 45, 5, Some("off"), true)
        );
        assert!(!w.pool.contains(&mvs_mutators::ALL[0].key.to_owned()));
        // refused: an unlisted window, an empty pool
        assert!(!r.set_option(OptionKey::Wheel, OptionValue::Wheel(WheelOptions { seconds: 7, ..on })));
        let none = (0..mvs_mutators::ALL.len()).fold(on, |o, i| o.with(i, false));
        assert!(!r.set_option(OptionKey::Wheel, OptionValue::Wheel(none)));
        // the lobby's state carries the options to its members
        let mut other = LobbyRules::new();
        other.load(&r.data());
        assert_eq!(other.options.wheel, o);
    }

    #[test]
    fn ffa_has_no_ringout_count_unless_stocks() {
        let mut r = lobby();
        r.set_team_style(LobbyStyle::FreeForAll);
        assert_eq!(r.match_setup().setup.rules.knockouts_to_win, 0);
        assert!(r.set_option(OptionKey::Stocks, OptionValue::Bool(true)));
        assert_eq!(r.match_setup().setup.rules.knockouts_to_win, 4);
        assert!(!r.set_option(OptionKey::Mutators, OptionValue::Set(MutatorSet(1))));
        assert!(!r.set_option(OptionKey::Mutators, OptionValue::Set(MutatorSet(1 << MUTATOR_CHOICES))));
        assert!(!r.set_option(OptionKey::Public, OptionValue::Set(MutatorSet(2))));
    }

    #[test]
    fn the_mutators_run_together_in_the_lists_order() {
        let mut r = lobby();
        assert!(r.match_setup().setup.mutators.is_empty());
        let m = MutatorSet::default().with(6, true).with(4, true).with(0, true).with(99, true);
        assert_eq!(m, MutatorSet((1 << 4) | (1 << 6)), "no bit for the empty entry or one past the list");
        assert!(r.set_option(OptionKey::Mutators, OptionValue::Set(m)));
        assert_eq!(r.match_setup().setup.mutators, vec!["Mutator_Giant", "Mutator_LowGravity"]);
        assert!(r.set_option(OptionKey::Mutators, OptionValue::Set(MutatorSet(MutatorSet::ALL))));
        assert_eq!(r.match_setup().setup.mutators.len(), MUTATOR_CHOICES - 1);
        assert_eq!(serde_json::to_value(r.options).unwrap()["mutators"], serde_json::json!(MutatorSet::ALL));
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
