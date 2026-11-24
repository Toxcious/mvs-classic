//! The match set-up as it crosses the wire: the server makes it (a lobby's start, a queue match)
//! and every game builds its World from it. Its source: the game mode's active match config and the
//! PlayerDataManager.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// GetTeamStyle: 0 "1v1", 1 "2v2", 2 "FreeForAll".
pub type TeamStyle = u8;
pub const ONE_VS_ONE: TeamStyle = 0;
pub const TWO_VS_TWO: TeamStyle = 1;
pub const FREE_FOR_ALL: TeamStyle = 2;

/// A player of the match. `difficulty`: a bot's lobby choice, 0 VeryEasy .. 3 Hard (MatchPlayerData BotBehavior
/// 2..5), 4 the port's own Expert; `character` a roster name (absent: the default one); `team`, `start` and `starting_damage` the Lab's slots'
/// own values (TrainGameMode.PopulateMatchPlayerData); `perks` MatchPlayerData.AssignPerks' SelectedPerks in slot
/// order; `taunts` the taunt slots 0-3 by TauntData / EmoteData asset name; `ring_out` the RingOutVfxData asset name
/// (absent: ROV_Default); `banner` the BannerData slug; `skin` the SkinData asset name; `input` the fight-read
/// input preferences (a bot's is not read).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchPlayerWire {
    pub bot: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starting_damage: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perks: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taunts: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ring_out: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<InputPrefsWire>,
}

/// Struct_Preferences_Input's fight-read fields, each absent at its default: `fastfall` E_FastfallType (1 Tap; absent
/// 0 Hold), `double_crouch_jump_down` (false; absent true), `ignore_right_stick_diagonals`, `swapped_ground` /
/// `swapped_air` (true; absent false), `input_buffer_cap` (frames 0..30; absent no cap), `input_buffer_hold` (true),
/// `attack_picks_up_items` (false; absent true).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputPrefsWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fastfall: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub double_crouch_jump_down: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore_right_stick_diagonals: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swapped_ground: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swapped_air: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_buffer_cap: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_buffer_hold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attack_picks_up_items: Option<bool>,
}

/// An InputPrefsWire as a peer sent it, cleaned: only the non-default values the shape allows; None when none is
/// left (readInputPrefs).
pub fn read_input_prefs(v: &Value) -> Option<InputPrefsWire> {
    let m = v.as_object()?;
    let is = |key: &str, want: bool| m.get(key).and_then(Value::as_bool) == Some(want);
    let cap = m.get("inputBufferCap").and_then(|c| int_in(c, 0.0, 30.0));
    let out = InputPrefsWire {
        fastfall: m.get("fastfall").and_then(Value::as_f64).filter(|f| *f == 1.0).map(|_| 1),
        double_crouch_jump_down: is("doubleCrouchJumpDown", false).then_some(false),
        ignore_right_stick_diagonals: is("ignoreRightStickDiagonals", true).then_some(true),
        swapped_ground: is("swappedGround", true).then_some(true),
        swapped_air: is("swappedAir", true).then_some(true),
        input_buffer_cap: cap,
        input_buffer_hold: is("inputBufferHold", true).then_some(true),
        attack_picks_up_items: is("attackPicksUpItems", false).then_some(false),
    };
    (out != InputPrefsWire::default()).then_some(out)
}

/// A JSON number that is an integer in `lo ..= hi` (`typeof v === 'number' && Number.isInteger(v)` and the range).
pub(crate) fn int_in(v: &Value, lo: f64, hi: f64) -> Option<u32> {
    let f = v.as_f64()?;
    if f.fract() != 0.0 || f < lo || f > hi {
        return None;
    }
    // (a whole number from 0 up: the cast saturates a larger one)
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a whole number, at least 0; the cast saturates"
    )]
    Some(f as u32)
}

/// PandaGameMode scoring (BPI_SetUpModeScoring): `knockouts_to_win` GetNumRingoutsForWin (0 or less never ends the
/// match by ring-outs), `duration` GetMatchDuration in seconds; `stocks` the experimental stock rule.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchRulesWire {
    pub knockouts_to_win: i32,
    pub duration: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stocks: Option<bool>,
}

/// The port's own Chaos Wheel rule of a match (an experimental rule): `pool` the MutatorData_Map keys it may roll,
/// `seconds` a window's length (the first one plays plain), `warn` how many seconds ahead of a change the next
/// mutator is drawn.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WheelWire {
    pub pool: Vec<String>,
    pub seconds: u32,
    pub warn: u32,
}

/// The match set-up: `players` in player-index order; `rules` absent for the Lab (TrainGameMode); `flow` the world
/// runs the countdown card itself; `allow_hazards` UMatchConfig AllowHazards (absent true); `local_play` a Local Play
/// match; `mutators` UMatchConfig GetMutatorData's MutatorBuffName list; `wheel` the experimental Chaos Wheel rule.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSetupWire {
    pub team_style: TeamStyle,
    pub players: Vec<MatchPlayerWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<MatchRulesWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_hazards: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_play: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutators: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wheel: Option<WheelWire>,
}
