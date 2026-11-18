//! The Controls screen's saves: Struct_Preferences_Input
//! of each local player 0..3, and the player's key bindings (the build's "InputBindings_v0" save slot,
//! InputBindingsSaveGame Mappings: "{Action}-P{n}" -> keys).

use indexmap::IndexMap;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::{any, section, take};

/// The local players the input preferences and the binding sets cover.
pub const LOCAL_PLAYERS: usize = 4;

/// Struct_Preferences_Input's entry of one local player (a SizeToFit Array_Set per field in the build). Not kept:
/// controlGuid (analytics only) and lockMouseToViewport (its Graphics row is not shown).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct InputPlayerSettings {
    pub vibration: bool,
    pub swapped_attack_style_ground: bool,
    /// Stored only: its row is collapsed in Construct and nothing reads it.
    pub deadzone_large: bool,
    pub double_crouch_jump_down: bool,
    pub up_to_jump: bool,
    pub attack_picks_up_items: bool,
    /// E_FastfallType: 0 Hold, 1 Tap.
    pub fastfall_type: u8,
    pub swapped_attack_style_air: bool,
    /// E_RightStickType: 0 Attacks, 1 Specials.
    pub right_stick_style: u8,
    /// Percent, 10..=90.
    pub horizontal_deadzone: i32,
    pub vertical_deadzone: i32,
    /// Frames 0..=31; above 30 is no cap (the default 10000, stored as 31 once the row shows it).
    pub input_buffer_cap: i32,
    pub input_buffer_hold: bool,
    pub ignore_right_stick_diagonals: bool,
}

impl Default for InputPlayerSettings {
    /// The struct's defaults (LocalPreferences.json's).
    fn default() -> InputPlayerSettings {
        InputPlayerSettings {
            vibration: true,
            swapped_attack_style_ground: false,
            deadzone_large: false,
            double_crouch_jump_down: true,
            up_to_jump: false,
            attack_picks_up_items: true,
            fastfall_type: 0,
            swapped_attack_style_air: false,
            right_stick_style: 0,
            horizontal_deadzone: 30,
            vertical_deadzone: 30,
            input_buffer_cap: 10000,
            input_buffer_hold: false,
            ignore_right_stick_diagonals: false,
        }
    }
}

impl InputPlayerSettings {
    /// Get-Input-Horizontal / VerticalDeadzone: the percent / 100.
    pub fn dead_zone(&self) -> [f64; 2] {
        [f64::from(self.horizontal_deadzone) / 100.0, f64::from(self.vertical_deadzone) / 100.0]
    }

    /// Get-Input-InputBufferCap: 10000 for a stored value above 30.
    pub fn buffer_cap(&self) -> i32 {
        if self.input_buffer_cap > 30 { 10000 } else { self.input_buffer_cap }
    }
}

/// The input saves.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct InputSettings {
    /// Local players 0..3.
    pub players: [InputPlayerSettings; LOCAL_PLAYERS],
    /// The saved key bindings ("{Action}-P{n}" -> keys, in the order PopulateFromRuntime wrote them); None: never
    /// saved (LoadInputBindings populates from the defaults).
    pub bindings: Option<IndexMap<String, Vec<String>>>,
}

/// The saved input section over the defaults: each field of its own type and range.
pub(crate) fn input_from(root: &Map<String, Value>, empty: &Map<String, Value>) -> InputSettings {
    let s = section(root, "input", empty);
    let saved = s.get("players").and_then(Value::as_array);
    let players = std::array::from_fn(|i| {
        let d = InputPlayerSettings::default();
        let Some(p) = saved.and_then(|a| a.get(i)).and_then(Value::as_object) else { return d };
        let pct = |v: &i32| (10..=90).contains(v);
        InputPlayerSettings {
            vibration: take(p, "vibration", d.vibration, any),
            swapped_attack_style_ground: take(p, "swapped_attack_style_ground", d.swapped_attack_style_ground, any),
            deadzone_large: take(p, "deadzone_large", d.deadzone_large, any),
            double_crouch_jump_down: take(p, "double_crouch_jump_down", d.double_crouch_jump_down, any),
            up_to_jump: take(p, "up_to_jump", d.up_to_jump, any),
            attack_picks_up_items: take(p, "attack_picks_up_items", d.attack_picks_up_items, any),
            fastfall_type: take(p, "fastfall_type", d.fastfall_type, |v| *v <= 1),
            swapped_attack_style_air: take(p, "swapped_attack_style_air", d.swapped_attack_style_air, any),
            right_stick_style: take(p, "right_stick_style", d.right_stick_style, |v| *v <= 1),
            horizontal_deadzone: take(p, "horizontal_deadzone", d.horizontal_deadzone, pct),
            vertical_deadzone: take(p, "vertical_deadzone", d.vertical_deadzone, pct),
            input_buffer_cap: take(p, "input_buffer_cap", d.input_buffer_cap, |v| *v >= 0),
            input_buffer_hold: take(p, "input_buffer_hold", d.input_buffer_hold, any),
            ignore_right_stick_diagonals: take(p, "ignore_right_stick_diagonals", d.ignore_right_stick_diagonals, any),
        }
    });
    // (a name of a player 0..3 and a list of strings each; anything else is dropped)
    let bindings = s.get("bindings").and_then(Value::as_object).map(|m| {
        m.iter()
            .filter(|(name, _)| (0..LOCAL_PLAYERS).any(|p| name.contains(&format!("-P{p}"))))
            .filter_map(|(name, keys)| {
                let keys = keys.as_array()?.iter().filter_map(Value::as_str).map(str::to_owned).collect();
                Some((name.clone(), keys))
            })
            .collect()
    });
    InputSettings { players, bindings }
}
