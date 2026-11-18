//! A key's name as the input labels show it (UI_Functions GetKeyLocalizedName): the Blueprint's own names, by their
//! Game.locres keys, else the key's display name (Key_GetDisplayName), the Engine target's InputKeys namespace
//! (InputCoreTypes' LOCTEXT). Both read from the player's localization (the text kind).

use std::collections::HashMap;

/// A name of GetKeyLocalizedName's KeyNameMap: a text of the Game target, or a culture-invariant literal (the
/// bytecode's EX_TextConst without a key: no localization target holds it).
#[derive(Clone, Copy, Debug)]
enum Named {
    Game(&'static str),
    Invariant(&'static str),
}

/// UI_Functions GetKeyLocalizedName's KeyNameMap, the EX_SetMap literal @0: each key's text, a Game.locres key of the
/// empty namespace or an invariant literal (53 entries, in the literal's order).
const KEY_NAME_MAP: [(&str, Named); 53] = {
    use Named::{Game as G, Invariant as I};
    [
        ("Home", G("8A00B1A646FD1D95477635B63F5A68EF")),
        ("Tab", G("97A8D58E43E2219AEDEC6D8C12AB48B2")),
        ("Enter", G("B8E3BCC54DBBE93E95543688DDB0C06B")),
        ("Pause", G("701416EB4A55A866D0535F8D46D50283")),
        ("CapsLock", G("696F05E1417A3F428EB0288B910238E3")),
        ("Escape", G("4647B1A84FAE69184B9026AE3AABE3BC")),
        ("SpaceBar", G("C4F316254D8D0111E43D58A796DCECD7")),
        ("PageUp", G("8098267D48C6819D072A859C6AF92457")),
        ("PageDown", G("B8BE268948310ECACA43209952BBE5AA")),
        ("End", G("433DBC204BC32C13420432BB1A3049D7")),
        ("Left", G("187D59254E73D01F72703EA9C666A5B9")),
        ("Up", G("E42061DA4B03F056130956976BF4FF78")),
        ("Right", G("4BC5625740C3AD003FDE8CB78BCB8D3A")),
        ("Down", G("4BB4B6D24F7E4BFEBF366EB5C93995C5")),
        ("Insert", G("51040A4447DD54AFAAE848B89A8E3448")),
        ("BackSpace", G("814A6F8940B9ECD2D5C9B48E98A2CCAA")),
        ("Delete", G("6FB6D1A44AFE84EA8A44209F3D382A2E")),
        ("NumLock", G("76C9FFC34BDED308C4769182EC2174F1")),
        ("ScrollLock", G("2B84FBD0484E3C78E7246F8A2F3D75C4")),
        ("LeftShift", G("2B3B977349538F74DF3790904CD01DDB")),
        ("RightShift", G("F02D62B34283A31CB2D132BDF01DF373")),
        ("LeftControl", G("A46588A247040DD0389035BA51073C95")),
        ("RightControl", G("8ACDBDAD4D3DA0DD5CD810840303C26D")),
        ("LeftAlt", G("1D6F3CF84EEA4A89BBCF08B0B3D16865")),
        ("RightAlt", G("9831E51A4BF00EB176F149AED39BFAF1")),
        ("LeftCommand", G("FCD221DC46E7FFC0676E3088DE97FDD7")),
        ("RightCommand", G("08E522334E2D010F65382E9FB0B9D9F8")),
        ("Semicolon", I(";")),
        ("Equals", I("=")),
        ("Comma", I(",")),
        ("Hyphen", I("-")),
        ("Underscore", I("_")),
        ("Period", I(".")),
        ("Slash", I("/")),
        ("Tilde", I("`")),
        ("LeftBracket", I("[")),
        ("RightBracket", I("]")),
        ("Backslash", I("\\")),
        ("Apostrophe", I("'")),
        ("Quote", I("\"")),
        ("LeftParantheses", I("(")),
        ("RightParantheses", I(")")),
        ("Ampersand", I("&")),
        ("Asterix", I("*")),
        ("Caret", I("^")),
        ("Dollar", I("$")),
        ("Exclamation", I("!")),
        ("Colon", I(":")),
        ("LeftMouseButton", G("8366298144B37237EA849EB3D0CCAA52")),
        ("RightMouseButton", G("2A25F7DE450327DC4CBBB2A0A2F89BB9")),
        ("MiddleMouseButton", G("44D7C84E425831FAB95D9F90D15913B4")),
        ("MouseScrollUp", G("4B37A1E345F2B2471BEE4180E9CBBCD2")),
        ("MouseScrollDown", G("F507483B426DD78DB50BBFA2DC594BA4")),
    ]
};

/// The digit keys, whose display names InputCoreTypes gives as FText::FromString of the digit (no LOCTEXT: the
/// Engine target has no line for them).
const DIGITS: [&str; 10] = ["Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine"];

/// The key names of the player's localization.
#[derive(Debug, Default)]
pub struct KeyNames {
    /// KeyNameMap's Game texts as the Game target has them, by key name (a key the target lacks is absent).
    named: HashMap<&'static str, String>,
    /// The Engine target's InputKeys namespace: a key's long display name by its LOCTEXT key.
    display: HashMap<String, String>,
}

impl KeyNames {
    /// The names of `game`, the Game target's empty namespace, and `input_keys`, the Engine target's InputKeys
    /// namespace (InputCoreTypes' LOCTEXT_NAMESPACE).
    pub fn new(game: Option<&HashMap<String, String>>, input_keys: HashMap<String, String>) -> KeyNames {
        let named = KEY_NAME_MAP
            .iter()
            .filter_map(|(k, n)| match n {
                Named::Game(id) => Some((*k, game?.get(*id)?.clone())),
                Named::Invariant(_) => None,
            })
            .collect();
        KeyNames { named, display: input_keys }
    }

    /// UI_Functions GetKeyLocalizedName(Key) @0-3288: KeyNameMap's text of the key (Map_Find @3040), else
    /// Key_GetDisplayName (@3224).
    // Deviation: a KeyNameMap text the player's Game target lacks takes the display name (the build would show the
    // literal's source string, which the port does not hold).
    pub fn name(&self, key: &str) -> String {
        match KEY_NAME_MAP.iter().find(|(k, _)| *k == key).map(|(_, n)| n) {
            Some(Named::Invariant(s)) => (*s).to_owned(),
            Some(Named::Game(_)) => self.named.get(key).cloned().unwrap_or_else(|| self.display_name(key)),
            None => self.display_name(key),
        }
    }

    /// FKey::GetDisplayName (bLongDisplayName true, Key_GetDisplayName's): the key details' LongDisplayName.
    // Unverified: InputCoreTypes' LOCTEXT key of each key is its FName (the build's exe is not read for EKeys'
    // table; every key of the namespace the labels can show is named so), and a key the namespace lacks is a
    // FText::FromString of its digit (Zero .. Nine) or of its name (the letters).
    pub fn display_name(&self, key: &str) -> String {
        if let Some(s) = self.display.get(key) {
            return s.clone();
        }
        if let Some(d) = DIGITS.iter().position(|d| *d == key) {
            return d.to_string();
        }
        key.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> KeyNames {
        let game: HashMap<String, String> = [("C4F316254D8D0111E43D58A796DCECD7", "Space")]
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .into_iter()
            .collect();
        let keys = [("SpaceBar", "Space Bar"), ("NumPadThree", "Num 3"), ("Escape", "Escape"), ("Multiply", "Num *")]
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .into_iter()
            .collect();
        KeyNames::new(Some(&game), keys)
    }

    #[test]
    fn the_map_first_then_the_display_name() {
        let n = names();
        // (the map's Game text, over the Engine target's "Space Bar")
        assert_eq!(n.name("SpaceBar"), "Space");
        assert_eq!(n.name("Backslash"), "\\");
        assert_eq!(n.name("NumPadThree"), "Num 3");
        assert_eq!(n.name("Multiply"), "Num *");
        assert_eq!(n.name("Seven"), "7");
        assert_eq!(n.name("J"), "J");
        // (a map text the Game target lacks: the display name)
        assert_eq!(n.name("Escape"), "Escape");
    }

    #[test]
    fn no_localization_reads_the_key() {
        let n = KeyNames::default();
        assert_eq!(n.name("SpaceBar"), "SpaceBar");
        assert_eq!(n.name("Comma"), ",");
        assert_eq!(n.name("Zero"), "0");
    }

    /// The names of the player's localization (needs the dev store: skipped without it).
    #[test]
    fn the_players_localization() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else { return };
        let Ok(text) = crate::GameText::load(&store) else { return };
        let n = text.key_names();
        // (every Game text of the map is in the Game target)
        let game = KEY_NAME_MAP.iter().filter(|(_, m)| matches!(m, Named::Game(_))).count();
        assert_eq!(n.named.len(), game);
        assert_eq!(n.name("SpaceBar"), "Space");
        assert_eq!(n.name("LeftMouseButton"), "LMB");
        assert_eq!(n.name("NumPadThree"), "Num 3");
        assert_eq!(n.name("Multiply"), "Num *");
        assert_eq!(n.name("F5"), "F5");
        assert_eq!(n.name("ThumbMouseButton2"), "Thumb Mouse Button 2");
        assert_eq!(n.name("Seven"), "7");
        assert_eq!(n.name("J"), "J");
    }

    #[test]
    fn the_map_has_each_key_once() {
        let mut seen: Vec<&str> = KEY_NAME_MAP.iter().map(|(k, _)| *k).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), KEY_NAME_MAP.len());
    }
}
