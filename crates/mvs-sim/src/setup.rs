//! The match's set-up as the sim reads it: `MatchSetupWire` with the local parts (`arcade`, `mode`), and the input
//! preferences. A set-up is the World's input, not its state: the World keeps it behind an `Arc`, outside the hash and
//! the snapshot.
//!
//! The wire form and its checks belong with the protocol; the types here are the sim's view of it.

use crate::Real;
use crate::names::Name;

/// `TeamStyle`: 0 1v1, 1 2v2, 2 free for all.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum TeamStyle {
    #[default]
    OneVsOne = 0,
    TwoVsTwo = 1,
    FreeForAll = 2,
}

/// `rules`: the custom game rules.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rules {
    pub knockouts_to_win: Real,
    pub duration: Real,
    /// `stocks?: true`.
    pub stocks: bool,
}

/// `MatchPlayerWire`.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct MatchPlayer {
    pub bot: bool,
    pub difficulty: Option<Real>,
    /// A roster key; `None` the default character.
    pub character: Option<String>,
    pub team: Option<u8>,
    pub start: Option<u8>,
    pub starting_damage: Option<Real>,
    pub perks: Vec<String>,
    pub taunts: Vec<String>,
    pub ring_out: Option<String>,
    pub banner: Option<String>,
    pub skin: Option<String>,
    pub input: InputPrefs,
}

/// `InputPrefs` (`Struct_Preferences_Input`); `Default` is `DEFAULT_INPUT_PREFS`, the wire's absent fields.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputPrefs {
    /// E_FastfallType: 0 Hold, 1 Tap.
    pub fastfall: u8,
    pub double_crouch_jump_down: bool,
    pub ignore_right_stick_diagonals: bool,
    pub swapped_ground: bool,
    pub swapped_air: bool,
    /// Frames: 0..30, or 10000 (no cap).
    pub input_buffer_cap: u16,
    pub input_buffer_hold: bool,
    pub attack_picks_up_items: bool,
}

impl Default for InputPrefs {
    fn default() -> Self {
        InputPrefs {
            fastfall: 0,
            double_crouch_jump_down: true,
            ignore_right_stick_diagonals: false,
            swapped_ground: false,
            swapped_air: false,
            input_buffer_cap: 10000,
            input_buffer_hold: false,
            attack_picks_up_items: true,
        }
    }
}

/// `ArcadeRound`: the run's difficulty (1..3) and the 1-based round.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArcadeRound {
    pub difficulty: u8,
    pub stage: u8,
}

/// `MatchMode`: a match whose mode spawns its own pawns.
#[derive(Clone, PartialEq, Debug)]
pub enum MatchMode {
    /// `TutorialMode`: the TutorialActor class the level runs and the character keys it may spawn, in roster order.
    /// The `tutorials` (every TutorialActor class's data) are the match's data, not its set-up, here.
    Tutorial { actor: String, cast: Vec<String> },
    /// The port's own Waves mode (`modes/waves/`): the player, set-up player 0, against a stream of bots drawn from
    /// `cast` (`modes::waves::draw_cast`), which the match loads up front.
    Waves { variant: WavesVariant, cast: Vec<String> },
}

impl MatchMode {
    /// The character keys the mode may spawn past the set-up's players, loaded with them.
    pub fn cast(&self) -> &[String] {
        match self {
            MatchMode::Tutorial { cast, .. } | MatchMode::Waves { cast, .. } => cast,
        }
    }
}

/// A Waves run's goal: the run ends at the player's ring-out either way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WavesVariant {
    /// 100 Ring-Outs: the run is cleared at the hundredth ring-out; its record is the time.
    RingOuts100,
    /// Endless: the record is the count of ring-outs.
    Endless,
}

/// `MatchSetup`.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct MatchSetup {
    pub team_style: TeamStyle,
    pub players: Vec<MatchPlayer>,
    pub rules: Option<Rules>,
    /// `flow`: the intro and the match start run (a custom game); false in the Lab.
    pub flow: bool,
    pub allow_hazards: bool,
    pub local_play: bool,
    pub mutators: Vec<String>,
    pub arcade: Option<ArcadeRound>,
    pub mode: Option<MatchMode>,
}

/// `MATCH_START_TICKS`: BPI_SetMatchStarted, counted from the first game tick.
pub const MATCH_START_TICKS: u32 = 420;
/// `INTRO_SEQUENCE_TICKS`: the IntroSequence starts.
pub const INTRO_SEQUENCE_TICKS: u32 = 150;
/// `INTRO_END_TICKS`: SetCameraMovement(true), EndIntro.
pub const INTRO_END_TICKS: u32 = 330;
/// `INPUT_OPEN_TICKS`.
pub const INPUT_OPEN_TICKS: u32 = 369;
/// ParentGameMode.GetPlayerStart: the PlayerStart actor tag per team style.
pub const START_TAGS: [&str; 3] = ["1v1", "2v2", "FreeForAll"];

/// `teamCount`.
pub fn team_count(style: TeamStyle) -> u8 {
    if style == TeamStyle::FreeForAll { 4 } else { 2 }
}

/// `playerTeam`.
pub fn player_team(setup: &MatchSetup, i: usize) -> u8 {
    let n = usize::from(team_count(setup.team_style));
    setup.players[i].team.unwrap_or_else(|| u8::try_from(i % n).expect("a team below the team count"))
}

/// The player's preferences, the defaults for a bot. The wire's absent
/// fields are the defaults already (`MatchPlayer::input`).
/// Deviation: the build reads the local machine's preferences, some at the literal index 0 (BaseFighter
/// FastfallMethod, DoubleCrouchJumpDown, IgnoreRightStickDiagonals), so a second local player or a remote fighter can
/// run on another player's values; the port gives each fighter its own player's.
pub fn resolve_input_prefs(p: &MatchPlayer) -> InputPrefs {
    if p.bot { InputPrefs::default() } else { p.input }
}

/// FFAGameMode's random items (StartNewMatch @2755-3054): FreeForAll with rules has them; the Lab's style 2 (no
/// rules) has none. Mutator_RandomItemSpawns and Mutator_RandomMutatorSelector start the spawner too.
const ITEM_MUTATORS: [&str; 2] = ["Mutator_RandomItemSpawns", "Mutator_RandomMutatorSelector"];

/// `itemsOn`.
pub fn items_on(setup: &MatchSetup) -> bool {
    crate::world::flow::ffa_items_on(setup) || setup.mutators.iter().any(|m| ITEM_MUTATORS.contains(&m.as_str()))
}

/// `KO_POINTS`: TryUpdateTeamScore's points for the attacker's team per ring-out (before KOPointMultiplier), by
/// team style.
pub const KO_POINTS: [Real; 3] = [1.0, 1.0, 2.0];
/// FFAGameMode CDO DoublePointsStartPercent.
pub const DOUBLE_POINTS_START: Real = 0.6;
/// FFAGameMode CDO BonusPointsMultiplier.
pub const BONUS_POINTS_MULTIPLIER: Real = 2.0;
/// FFAGameMode CDO ShrinkBlastBoxStartPercent (of the match duration).
pub const SHRINK_BLAST_BOX_START: Real = 0.75;
/// FFAGameMode CDO ShrinkBlastBoxTime (seconds).
pub const SHRINK_BLAST_BOX_TIME: Real = 30.0;
/// `blast_shrink_by` of FFAGameMode's shrink.
pub const FFA_SHRINK: Name = crate::name!("FFAGameMode");
/// Buff_FFABonusObjectiveTrackers CDO KnockOutsForSpree.
pub const BONUS_SPREE_KNOCKOUTS: i32 = 3;
/// Buff_FFABonusObjectiveTrackers CDO SpreePoints.
pub const BONUS_SPREE_POINTS: Real = 3.0;
/// Buff_FFABonusObjectiveTrackers CDO SurvivalTime (seconds).
pub const BONUS_SURVIVAL_TIME: Real = 60.0;
/// Buff_FFABonusObjectiveTrackers CDO SurvivalPoints.
pub const BONUS_SURVIVAL_POINTS: Real = 2.0;

/// `END_PHASES`: the end of a match from HandleWinLoss, in seconds after the step before: VictoryTimerActor
/// (RespawnDelay) -> GameEnd / MatchEnd, BroadcastPreTravel, Cleanup, the travel.
pub const END_PHASES: [(crate::events::EndPhase, Real); 4] = {
    use crate::events::EndPhase as P;
    [(P::GameEnd, 2.0), (P::PreTravel, 4.0), (P::Cleanup, 1.0), (P::Travel, 0.2)]
};

/// `Math.round(s * 60)`: a time in whole game ticks.
fn ticks(s: Real) -> i32 {
    crate::math::to_int32(crate::math::round(s * 60.0))
}

/// `endPhaseTicks`: game ticks from HandleWinLoss to `phase`.
pub fn end_phase_ticks(phase: crate::events::EndPhase) -> i32 {
    let mut at = 0;
    for (p, d) in END_PHASES {
        at += ticks(d);
        if p == phase {
            break;
        }
    }
    at
}

/// `matchInputOpen`: whether fighters take input (BaseFighter InputDisabledCounter == 0): closed by ReadyForIntro
/// (@161718), opened by EndIntro's HardEnableInput; nothing in GameEnd, MatchEndImpl, the game state's MatchEnded or
/// the controller's BPI_MatchEnded closes it again, until CleanupBeforeSwitchMaps (PandaGameMode @15).
/// `ticks_since_over`: game ticks since HandleWinLoss, negative while the match runs.
/// Unverified: a native gate (APlayerController / AFighter input) is not checked.
pub fn match_input_open(intro_ticks: i32, ticks_since_over: i32) -> bool {
    i64::from(intro_ticks) >= i64::from(INPUT_OPEN_TICKS)
        && ticks_since_over < end_phase_ticks(crate::events::EndPhase::Cleanup)
}

/// PandaGameMode Countdown's lead: the match duration - 10 s.
pub const END_WARNING_LEAD: Real = 10.0;
/// The game state's Match End Warning: the events of each step (PF_Post_Global_Event), a Delay(1.0) between them.
pub fn end_warning(step: usize) -> &'static [Name] {
    const FIRST: [Name; 2] = [crate::name!("ui_map_countdown_ten_seconds"), crate::name!("ui_map_countdown")];
    const NEXT: [Name; 1] = [crate::name!("ui_map_countdown")];
    const LAST: [Name; 1] = [crate::name!("ui_map_countdown_complete")];
    match step {
        0 => &FIRST,
        1..=9 => &NEXT,
        _ => &LAST,
    }
}
/// The number of Match End Warning steps.
pub const END_WARNING_STEPS: usize = 11;
/// PFGPlayerController MatchCompleteVODelay (class default).
pub const MATCH_COMPLETE_VO_DELAY: Real = 1.8;

/// `otherTeam`: the switch KillPlayerFighter and TryUpdateTeamScore use: {0: 1, 1: 0}, and an uninitialised 0 for
/// any other team (FreeForAll teams 2 and 3).
pub fn other_team(team: i32) -> i32 {
    if team == 0 { 1 } else { 0 }
}

/// `perkList`: a perk list as the match takes it, each empty slot dropped.
pub fn perk_list(slots: &[String]) -> Vec<String> {
    slots.iter().filter(|p| !p.is_empty()).cloned().collect()
}
