//! The action key bindings: UInputSettings' runtime mappings "{Action}-P{player}"
//! as PreferencesManager LoadInputBindings makes them from the player's save or the DataTable InputDefaults, the
//! Controls page's edits of them and their save, and the FKey names of the XInput buttons. The match's input
//! devices and the menus (their input labels and binding rows) read the same bindings.
//!
//! The save (InputBindingsSaveGame Mappings, the build's "InputBindings_v0" slot) is the settings file's
//! `input.bindings` (mvs-settings): `load` takes it, `save_input_bindings` gives it back.

use indexmap::IndexMap;
use mvs_data::InputDefaults;

const PLAYERS: usize = 4;

/// (the port's own: no EInputAction of the build) the Lab Tools panel's open and close.
pub const LAB_TOOLS_ACTION: &str = "Lab Tools";

/// (the port's own) The actions the build has no row for, with their keys: player 0's when its defaults lack them. They
/// join the defaults of player 0 and a save made without them.
const PORT_ACTIONS: [(&str, &[&str]); 1] = [(LAB_TOOLS_ACTION, &["F1", "Gamepad_RightThumbstick"])];

/// The FKey names of the XInput buttons in the standard mapping's order.
pub const PAD_BUTTONS: [&str; 16] = [
    "Gamepad_FaceButton_Bottom",
    "Gamepad_FaceButton_Right",
    "Gamepad_FaceButton_Left",
    "Gamepad_FaceButton_Top",
    "Gamepad_LeftShoulder",
    "Gamepad_RightShoulder",
    "Gamepad_LeftTrigger",
    "Gamepad_RightTrigger",
    "Gamepad_Special_Left",
    "Gamepad_Special_Right",
    "Gamepad_LeftThumbstick",
    "Gamepad_RightThumbstick",
    "Gamepad_DPad_Up",
    "Gamepad_DPad_Down",
    "Gamepad_DPad_Left",
    "Gamepad_DPad_Right",
];

pub fn is_gamepad_key(key: &str) -> bool {
    key.starts_with("Gamepad_")
}

/// PreferencesManager StoredInputMappingName / InputBindingsSaveGame ActionToMappingName.
pub fn mapping_name(action: &str, player: usize) -> String {
    format!("{action}-P{player}")
}

/// InputBindingsSaveGame PlayerIndexFromActionName.
fn player_of(name: &str) -> Option<usize> {
    (0..PLAYERS).find(|p| name.contains(&format!("-P{p}")))
}

/// FilteredInputMappingArrayStruct: players 1..3 keep their gamepad keys only.
fn filtered(keys: &[String], player: Option<usize>) -> Vec<String> {
    keys.iter().filter(|k| player == Some(0) || is_gamepad_key(k)).cloned().collect()
}

/// FName::LexicalLess: no case.
fn lexical(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

/// The runtime bindings: per binding set (the "-P<n>" suffix) each key's actions, in the mappings' order.
#[derive(Clone, Debug, Default)]
pub struct Bindings {
    defaults: InputDefaults,
    /// UInputSettings ActionMappings, the "-P" names only: name -> keys in array order (AddActionMapping's
    /// AddUnique).
    runtime: IndexMap<String, Vec<String>>,
    by_key: Vec<IndexMap<String, Vec<String>>>,
    /// Counts the edits (a holder of a copy knows it is stale).
    revision: u64,
}

impl Bindings {
    /// LoadInputBindings with no save: InputBindingsSaveGame PopulateFromDefaults (rows Default-P0..P3, each list
    /// filtered), then PopulateToRuntime.
    pub fn new(defaults: &InputDefaults) -> Bindings {
        Bindings::load(defaults, None)
    }

    /// LoadInputBindings: the saved mappings (`saved`), or PopulateFromDefaults (the Default rows, never Legacy);
    /// then every "-P" runtime mapping is dropped and the save's are added (PopulateToRuntime). The port's actions
    /// join a save made without them.
    pub fn load(defaults: &InputDefaults, saved: Option<&IndexMap<String, Vec<String>>>) -> Bindings {
        let mut b = Bindings { defaults: defaults.clone(), ..Bindings::default() };
        let mut save: IndexMap<String, Vec<String>> = match saved {
            Some(s) => s.clone(),
            None => {
                let mut s = IndexMap::new();
                for p in 0..PLAYERS {
                    for (action, keys) in b.input_defaults_for_player(p, false) {
                        s.insert(mapping_name(&action, p), keys);
                    }
                }
                s
            }
        };
        for (action, keys) in PORT_ACTIONS {
            save.entry(mapping_name(action, 0)).or_insert_with(|| keys.iter().map(|k| (*k).to_owned()).collect());
        }
        for (name, keys) in &save {
            for k in filtered(keys, player_of(name)) {
                b.add(name, &k);
            }
        }
        b.reindex();
        b
    }

    fn add(&mut self, name: &str, key: &str) {
        let keys = self.runtime.entry(name.to_owned()).or_default();
        if !keys.iter().any(|k| k == key) {
            keys.push(key.to_owned());
        }
    }

    fn reindex(&mut self) {
        self.revision += 1;
        self.by_key = vec![IndexMap::new(); PLAYERS];
        for (name, keys) in &self.runtime {
            let Some(p) = player_of(name) else { continue };
            let action = &name[..name.rfind("-P").expect("a -P name")];
            for k in keys {
                self.by_key[p].entry(k.clone()).or_default().push(action.to_owned());
            }
        }
    }

    /// The edits so far (each add, remove and sort counts).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    // ---------- UInputSettings (engine; Unverified: from the 4.26 source, not read in this build's exe) ----------

    /// GetActionMappingByName("{action}-P{set}")'s keys: UInputSettings walks ActionMappings from the last entry to the
    /// first, so the mapping's keys come last first.
    pub fn action_mapping<'a>(
        &'a self,
        action: &str,
        set: usize,
    ) -> impl DoubleEndedIterator<Item = &'a str> + use<'a> {
        self.mapping_by_name(&mapping_name(action, set))
    }

    /// GetActionMappingByName(name): the keys last first.
    pub fn mapping_by_name<'a>(&'a self, name: &str) -> impl DoubleEndedIterator<Item = &'a str> + use<'a> {
        self.runtime.get(name).into_iter().flat_map(|k| k.iter().rev()).map(String::as_str)
    }

    /// AddActionMapping: AddUnique.
    pub fn add_action_mapping(&mut self, name: &str, key: &str) {
        self.add(name, key);
        self.reindex();
    }

    /// RemoveActionMapping: every equal entry.
    pub fn remove_action_mapping(&mut self, name: &str, key: &str) {
        if let Some(keys) = self.runtime.get_mut(name) {
            keys.retain(|k| k != key);
            self.reindex();
        }
    }

    /// SaveKeyMappings sorts ActionMappings (action name, then key name) before it writes the ini; the ini itself is
    /// not kept (LoadInputBindings replaces every "-P" mapping at start-up).
    pub fn save_key_mappings(&mut self) {
        for keys in self.runtime.values_mut() {
            keys.sort_by(|a, b| lexical(a, b));
        }
        self.reindex();
    }

    // ---------- PreferencesManager ----------

    /// GetInputDefaultsForPlayer (multi keyboard off): rows Default-P{n} / Legacy-P{n}, each list filtered; player 0's
    /// gains the port's actions its row lacks.
    pub fn input_defaults_for_player(&self, player: usize, legacy: bool) -> IndexMap<String, Vec<String>> {
        let row = format!("{}-P{player}", if legacy { "Legacy" } else { "Default" });
        let mut out: IndexMap<String, Vec<String>> = self
            .defaults
            .rows
            .get(&row)
            .map(|r| r.iter().map(|(a, keys)| (a.clone(), filtered(keys, Some(player)))).collect())
            .unwrap_or_default();
        if player == 0 {
            for (action, keys) in PORT_ACTIONS {
                out.entry(action.to_owned()).or_insert_with(|| keys.iter().map(|k| (*k).to_owned()).collect());
            }
        }
        out
    }

    /// SaveInputBindings: InputBindingsSaveGame PopulateFromRuntime (GetActionMappingByName per "-P" name): the
    /// mappings the settings file keeps.
    pub fn save_input_bindings(&self) -> IndexMap<String, Vec<String>> {
        self.runtime
            .keys()
            .map(|name| {
                let keys: Vec<String> = self.mapping_by_name(name).map(str::to_owned).collect();
                (name.clone(), filtered(&keys, player_of(name)))
            })
            .collect()
    }

    // ---------- the game's side ----------

    /// The actions of binding set "-P<set>" on `key` (the action names without the player suffix).
    pub fn actions_of(&self, key: &str, set: usize) -> &[String] {
        self.by_key.get(set).and_then(|m| m.get(key)).map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> InputDefaults {
        let mut d = InputDefaults::default();
        let row = |pairs: &[(&str, &[&str])]| {
            pairs.iter().map(|(a, ks)| (a.to_string(), ks.iter().map(|k| k.to_string()).collect())).collect()
        };
        d.rows.insert(
            "Default-P0".into(),
            row(&[("Attack", &["J", "Gamepad_FaceButton_Left"]), ("Jump", &["SpaceBar", "Gamepad_FaceButton_Bottom"])]),
        );
        d.rows.insert("Default-P1".into(), row(&[("Attack", &["K", "Gamepad_FaceButton_Left"])]));
        d.rows.insert("Legacy-P0".into(), row(&[("Attack", &["U", "Gamepad_FaceButton_Top"])]));
        d
    }

    #[test]
    fn sets_one_to_three_keep_gamepad_keys_only() {
        let b = Bindings::new(&defaults());
        assert_eq!(b.actions_of("J", 0), ["Attack"]);
        assert_eq!(b.actions_of("Gamepad_FaceButton_Left", 1), ["Attack"]);
        assert!(b.actions_of("K", 1).is_empty());
        assert_eq!(b.actions_of("SpaceBar", 0), ["Jump"]);
    }

    #[test]
    fn an_action_mapping_comes_last_key_first() {
        let b = Bindings::new(&defaults());
        assert_eq!(b.action_mapping("Attack", 0).collect::<Vec<_>>(), ["Gamepad_FaceButton_Left", "J"]);
        assert_eq!(b.action_mapping("Attack", 1).collect::<Vec<_>>(), ["Gamepad_FaceButton_Left"]);
        assert_eq!(b.action_mapping("Special", 0).count(), 0);
    }

    #[test]
    fn the_lab_tools_keys_are_player_zeros_alone() {
        let b = Bindings::new(&defaults());
        assert_eq!(b.actions_of("F1", 0), [LAB_TOOLS_ACTION]);
        assert_eq!(b.actions_of("Gamepad_RightThumbstick", 0), [LAB_TOOLS_ACTION]);
        assert!(b.actions_of("Gamepad_RightThumbstick", 1).is_empty());
    }

    #[test]
    fn an_edit_saves_and_loads_back() {
        let mut b = Bindings::new(&defaults());
        let r = b.revision();
        b.remove_action_mapping("Attack-P0", "J");
        b.add_action_mapping("Attack-P0", "L");
        b.save_key_mappings();
        assert!(b.revision() > r);
        assert_eq!(b.actions_of("L", 0), ["Attack"]);
        assert!(b.actions_of("J", 0).is_empty());
        let saved = b.save_input_bindings();
        // (sorted: Gamepad_FaceButton_Left before L; the save is GetActionMappingByName's, last first)
        assert_eq!(saved["Attack-P0"], ["L", "Gamepad_FaceButton_Left"]);
        let c = Bindings::load(&defaults(), Some(&saved));
        assert_eq!(c.actions_of("L", 0), ["Attack"]);
        assert!(c.actions_of("J", 0).is_empty());
        // (a keyboard key saved for player 1 is filtered at the load)
        let mut s = saved.clone();
        s.insert("Attack-P1".into(), vec!["K".into()]);
        assert!(Bindings::load(&defaults(), Some(&s)).actions_of("K", 1).is_empty());
    }

    #[test]
    fn the_legacy_defaults_of_player_zero_have_the_port_actions() {
        let b = Bindings::new(&defaults());
        let l = b.input_defaults_for_player(0, true);
        assert_eq!(l["Attack"], ["U", "Gamepad_FaceButton_Top"]);
        assert!(l.contains_key(LAB_TOOLS_ACTION));
        assert!(!b.input_defaults_for_player(1, false).contains_key(LAB_TOOLS_ACTION));
    }
}
