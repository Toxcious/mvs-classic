//! What the machine keeps of the player's loadout and the menus' own saves:
//! the Hydra preferences (the Collection's equipped ring-out, announcer, banner, stat trackers, profile icon, each
//! character's taunts and selected skin), the perks the pre-match screen remembers per character, and the Local Play
//! preferences; the Lab Tools' switch and page are held here for one Lab visit, never saved.
//!
//! Deviation: the build keeps the Hydra preferences on the Hydra server; the port keeps them in the settings file
//! with the rest.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{any, section, take};

/// An equipped asset: the build's {Slug, AssetPath}; an empty slug is an empty slot. The path is the asset's path
/// name, "/Game/.../Name.Name", "" with an empty slug.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetRef {
    pub slug: String,
    pub path: String,
}

/// One character's CharacterPreferences entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CharacterPrefs {
    /// TauntDatas, slots 0-3 (shorter while a slot past the last set is never written).
    pub taunts: Vec<AssetRef>,
    /// SkinData: the selected skin (None until one is set).
    pub skin: Option<AssetRef>,
}

/// HydraPreferences.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct HydraSettings {
    pub ring_out_vfx: AssetRef,
    pub announcer_pack: AssetRef,
    pub banner: AssetRef,
    /// The stat trackers, slots 0-2.
    pub stat_trackers: [AssetRef; 3],
    pub profile_icon: AssetRef,
    /// CharacterPreferences by the character's HydraCharacterData slug.
    pub characters: BTreeMap<String, CharacterPrefs>,
    /// The run's gates of PreferencesManager's SetDefault* rules (not saved: each run starts with them open).
    #[serde(skip)]
    pub defaults_run: HydraDefaultsRun,
}

/// The DoOnce gates of PreferencesManager's SetDefault* rules for this run, and DefaultsAdjusted (menu/hydra.rs
/// writes them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HydraDefaultsRun {
    pub ring_out: bool,
    pub announcer: bool,
    pub banner: bool,
    pub stat_tracker: bool,
    pub profile_icon: bool,
    pub taunts: bool,
    /// DefaultsAdjusted: a gate passed (the preferences are saved after the load's defaults).
    pub adjusted: bool,
}

impl HydraSettings {
    /// A character's taunt slots 0-3 (empty refs where nothing is stored).
    pub fn character_taunts(&self, character: &str) -> [AssetRef; 4] {
        let t = self.characters.get(character).map_or(&[][..], |c| c.taunts.as_slice());
        std::array::from_fn(|i| t.get(i).cloned().unwrap_or_default())
    }

    /// UI_Functions GetTauntForSlot(slot, character slug): the slot's slug, "" for a slot outside 0-3, a character
    /// with no entry or a slot past its TauntDatas.
    pub fn taunt_for_slot(&self, slot: usize, character: &str) -> &str {
        if slot > 3 {
            return "";
        }
        self.characters.get(character).and_then(|c| c.taunts.get(slot)).map_or("", |r| r.slug.as_str())
    }

    /// SetCharacterTaunt-Rework: a slot outside 0-3 is refused, an unchanged slug returns, else the slot is set
    /// (the array grown with empty entries). True when it changed.
    pub fn set_character_taunt(&mut self, character: &str, slot: usize, r: AssetRef) -> bool {
        if slot > 3 {
            return false;
        }
        let e = self.characters.entry(character.to_string()).or_default();
        if e.taunts.get(slot).is_some_and(|t| t.slug == r.slug) {
            return false;
        }
        while e.taunts.len() < slot {
            e.taunts.push(AssetRef::default());
        }
        if slot < e.taunts.len() {
            e.taunts[slot] = r;
        } else {
            e.taunts.push(r);
        }
        true
    }

    /// The character's SkinData slug ("" for none: GetSelectedSkinForCharacter then takes the IsDefaultSkin one).
    pub fn character_skin_slug(&self, character: &str) -> &str {
        self.characters.get(character).and_then(|c| c.skin.as_ref()).map_or("", |r| r.slug.as_str())
    }

    /// PreferencesManager SetCharacterSkin: the entry's slug compared without case with the new one's (an invalid
    /// asset, an empty slug, with an empty entry) leaves it; else the ref goes into the entry, made with no taunts
    /// when the character has none. True when it changed.
    pub fn set_character_skin(&mut self, character: &str, r: AssetRef) -> bool {
        let had = self.character_skin_slug(character).to_string();
        if self.characters.contains_key(character)
            && (if r.slug.is_empty() { had.is_empty() } else { had.eq_ignore_ascii_case(&r.slug) })
        {
            return false;
        }
        let e = self.characters.entry(character.to_string()).or_default();
        e.skin = Some(if r.slug.is_empty() { AssetRef::default() } else { r });
        true
    }
}

/// The Lab Tools panel's switch and page, held with the settings for one visit of
/// the Lab: a Lab entered from the menus starts them at the defaults (mvs-app lab_host.rs), and they are not saved.
// Deviation: they and the Display screen's switches (the shapes, Hide HUD, Hide Effects) are not saved;
// they start at their defaults with each Lab entered from the menus.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LabSettings {
    /// The Lab menu's Lab Tools switch.
    pub tools_on: bool,
    /// The panel's page.
    pub page: u32,
}

/// LocalPlaySettingsFromPlayer's fields: one slot of the Local Play screen.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LocalPlayPlayer {
    pub character_slug: String,
    pub skin_slug: String,
    pub skin_reference: String,
    pub team_index: u8,
    /// Four entries, "" for none.
    pub perk_slugs: [String; 4],
    pub ringout_slug: String,
    pub banner_slug: String,
    /// Taunt 1-4 (array entries 0-3).
    pub taunts: [String; 4],
    /// "" = none (GetLocalPlayUsername: "Player n").
    pub username: String,
}

/// The LocalPlay preferences (PreferencesManager LocalPreferences.LocalPlayPreferences).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LocalPlaySettings {
    /// ETeamStyle: 0 1v1, 1 Teams (2v2), 2 FFA, 3 none.
    pub team_style: u8,
    pub ringouts: u32,
    /// Seconds, 0 = no time limit.
    pub time: u32,
    pub hazards: bool,
    /// The map selector's slugs (Set-LocalPlay-Maps).
    pub maps: Vec<String>,
    /// By slot (Player Index) 0..3.
    pub players: [LocalPlayPlayer; 4],
}

impl Default for LocalPlaySettings {
    /// The LocalPreferences defaults (the screen's settings line reads them: Teams, 4 ring-outs, 7 MIN, hazards on).
    /// Unverified: the struct's defaults are taken from what the screen showed before the store existed; the struct
    /// asset's own defaults are not read.
    fn default() -> LocalPlaySettings {
        LocalPlaySettings {
            team_style: 1,
            ringouts: 4,
            time: 420,
            hazards: true,
            maps: Vec::new(),
            players: Default::default(),
        }
    }
}

impl LocalPlaySettings {
    /// InitLocalPlayPreferences (PreferencesManager @3477): every slot's character cleared; and OnResourcesLoaded
    /// (@10792): every slot's username cleared, so a name lasts one run of the game. Done once when the file is
    /// read.
    pub fn start_of_run(&mut self) {
        for p in &mut self.players {
            p.character_slug.clear();
            p.username.clear();
        }
    }
}

/// The perks the pre-match screen remembers per character: the four slots in order, an
/// empty one None, by the port's character name.
pub type RememberedPerks = BTreeMap<String, Vec<Option<String>>>;

fn asset(o: &Map<String, Value>, key: &str) -> AssetRef {
    take(o, key, AssetRef::default(), any)
}

pub(crate) fn hydra_from(root: &Map<String, Value>, empty: &Map<String, Value>) -> HydraSettings {
    let s = section(root, "hydra", empty);
    let mut h = HydraSettings {
        ring_out_vfx: asset(s, "ring_out_vfx"),
        announcer_pack: asset(s, "announcer_pack"),
        banner: asset(s, "banner"),
        profile_icon: asset(s, "profile_icon"),
        ..HydraSettings::default()
    };
    if let Some(a) = s.get("stat_trackers").and_then(Value::as_array) {
        for (i, v) in a.iter().take(3).enumerate() {
            h.stat_trackers[i] = serde_json::from_value(v.clone()).unwrap_or_default();
        }
    }
    if let Some(c) = s.get("characters").and_then(Value::as_object) {
        for (k, v) in c {
            let Some(o) = v.as_object() else { continue };
            let taunts: Vec<AssetRef> = take(o, "taunts", Vec::new(), |t: &Vec<AssetRef>| t.len() <= 4);
            let skin: Option<AssetRef> = take(o, "skin", None, any);
            h.characters.insert(k.clone(), CharacterPrefs { taunts, skin });
        }
    }
    h
}

pub(crate) fn perks_from(root: &Map<String, Value>) -> RememberedPerks {
    let mut out = RememberedPerks::new();
    let Some(o) = root.get("perks").and_then(Value::as_object) else { return out };
    for (k, v) in o {
        if let Some(a) = v.as_array() {
            out.insert(k.clone(), a.iter().take(4).map(|p| p.as_str().map(str::to_string)).collect());
        }
    }
    out
}

pub(crate) fn local_play_from(root: &Map<String, Value>, empty: &Map<String, Value>) -> LocalPlaySettings {
    let s = section(root, "local_play", empty);
    let d = LocalPlaySettings::default();
    let mut lp = LocalPlaySettings {
        team_style: take(s, "team_style", d.team_style, |v| *v <= 3),
        ringouts: take(s, "ringouts", d.ringouts, any),
        time: take(s, "time", d.time, any),
        hazards: take(s, "hazards", d.hazards, any),
        maps: take(s, "maps", d.maps, any),
        players: d.players,
    };
    if let Some(a) = s.get("players").and_then(Value::as_array) {
        for (i, v) in a.iter().take(4).enumerate() {
            let Some(o) = v.as_object() else { continue };
            let p = &mut lp.players[i];
            let d = LocalPlayPlayer::default();
            *p = LocalPlayPlayer {
                character_slug: take(o, "character_slug", d.character_slug, any),
                skin_slug: take(o, "skin_slug", d.skin_slug, any),
                skin_reference: take(o, "skin_reference", d.skin_reference, any),
                team_index: take(o, "team_index", d.team_index, |v| *v < 4),
                perk_slugs: take(o, "perk_slugs", d.perk_slugs, any),
                ringout_slug: take(o, "ringout_slug", d.ringout_slug, any),
                banner_slug: take(o, "banner_slug", d.banner_slug, any),
                taunts: take(o, "taunts", d.taunts, any),
                username: take(o, "username", d.username, any),
            };
        }
    }
    lp.start_of_run();
    lp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Settings;
    use serde_json::json;

    fn r(slug: &str) -> AssetRef {
        AssetRef { slug: slug.into(), path: format!("/Game/{slug}.{slug}") }
    }

    #[test]
    fn taunts_and_skins_follow_the_preferences_rules() {
        let mut h = HydraSettings::default();
        assert_eq!(h.taunt_for_slot(2, "shaggy"), "");
        assert!(!h.set_character_taunt("shaggy", 4, r("t")));
        assert!(h.set_character_taunt("shaggy", 2, r("t2")));
        assert_eq!(h.characters["shaggy"].taunts.len(), 3);
        assert_eq!(h.taunt_for_slot(2, "shaggy"), "t2");
        assert!(!h.set_character_taunt("shaggy", 2, r("t2")));
        assert_eq!(h.character_taunts("shaggy")[0], AssetRef::default());
        // a skin: the same slug in another case leaves it
        assert!(h.set_character_skin("shaggy", r("Skin_A")));
        assert!(!h.set_character_skin("shaggy", r("skin_a")));
        assert_eq!(h.character_skin_slug("shaggy"), "Skin_A");
    }

    #[test]
    fn the_saved_sections_read_back_and_a_run_starts_clean() {
        let mut s = Settings::default();
        s.hydra.banner = r("b1");
        s.hydra.set_character_skin("finn", r("s1"));
        s.perks.insert("finn".into(), vec![Some("p".into()), None]);
        s.lab.tools_on = true;
        s.lab.page = 2;
        assert!(serde_json::to_value(&s).unwrap().get("lab").is_none(), "the Lab Tools' switch is saved");
        s.local_play.ringouts = 3;
        s.local_play.players[1].perk_slugs[2] = "x".into();
        s.local_play.players[0].username = "Sam".into();
        let v = serde_json::to_value(&s).unwrap();
        let back = Settings::from_value(&v);
        assert_eq!(back.hydra, s.hydra);
        assert_eq!(back.perks, s.perks);
        assert_eq!(back.lab, LabSettings::default());
        assert_eq!(back.local_play.ringouts, 3);
        assert_eq!(back.local_play.players[1].perk_slugs[2], "x");
        assert_eq!(back.local_play.players[0].username, "");
        // (an older file's Lab Tools keys are not read)
        let old = Settings::from_value(&json!({ "lab": { "tools_on": true, "page": 2, "show": { "hit": true } } }));
        assert_eq!(old.lab, LabSettings::default());
        let bad = Settings::from_value(&json!({ "local_play": { "team_style": 9, "time": "x" }, "perks": { "a": 5 } }));
        assert_eq!(bad.local_play.team_style, 1);
        assert_eq!(bad.local_play.time, 420);
        assert!(bad.perks.is_empty());
    }
}
