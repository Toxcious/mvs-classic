//! The event lists of a match's sound scopes: what
//! a fighter's data names and its sets carry, a skin's own events, the stage's, the ring-outs', the items'.

use std::collections::{HashMap, HashSet};

use mvs_data::fighter::{FighterData, VoRow};
use mvs_data::tables::{SkinAudio, SkinAudioSkin};

use super::posted;

/// The events of one scope, the system lines held as files in `lazy`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lists {
    pub keep: Vec<String>,
    pub lazy: Vec<String>,
}

/// The build's event index (InitBank.events: every event by its lower-case name, in the index's order).
#[derive(Clone, Debug, Default)]
pub struct Index {
    all: Vec<String>,
    set: HashSet<String>,
}

impl Index {
    pub fn new(names: impl IntoIterator<Item = String>) -> Index {
        let all: Vec<String> = names.into_iter().collect();
        let set = all.iter().cloned().collect();
        Index { all, set }
    }

    pub fn has(&self, event: &str) -> bool {
        self.set.contains(event)
    }

    pub fn all(&self) -> &[String] {
        &self.all
    }
}

/// The string values of a JSON text, in order, its object keys left out (JSON.stringify's replacer sees values only).
fn strings_of(json: &str) -> Vec<String> {
    let b = json.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'"' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < b.len() && b[j] != b'"' {
            j += if b[j] == b'\\' { 2 } else { 1 };
        }
        let key = b.get(j + 1) == Some(&b':');
        if !key && let Ok(s) = serde_json::from_str::<String>(&json[i..=j.min(b.len() - 1)]) {
            out.push(s);
        }
        i = j + 1;
    }
    out
}

/// eventsNamed: every event of the build that a value names somewhere (a string equal to one, without case), lower
/// case, each once, in the order they are met.
pub fn events_named(json: &str, known: &Index) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for s in strings_of(json) {
        let low = s.to_lowercase();
        if known.has(&low) && seen.insert(low.clone()) {
            out.push(low);
        }
    }
    out
}

/// A character's system lines (vo_<set>_sys_ / _announcer_): held as files, decoded when posted.
fn lazy_vo(e: &str) -> bool {
    let Some(rest) = e.strip_prefix("vo_") else { return false };
    let Some(i) = rest.find('_').filter(|&i| i > 0) else { return false };
    let tail = &rest[i + 1..];
    tail.starts_with("sys_") || tail.starts_with("announcer_")
}

/// Sound effects of a fighter's set that nothing in the port posts: named by no data and
/// no literal of the code; the build posts six of them from graphs the port has not got the sound of, no decoded graph
/// names the other two. Posted all the same, one plays late, as any event outside the scopes.
const CHAR_UNPOSTED: [&str; 8] = [
    "sfx_cha_shaggy_heavy_u_charge",
    "sfx_cha_shaggy_rage_ally_buff",
    "sfx_cha_shaggy_sandwich_hit",
    "sfx_cha_shaggy_sandwich_loop",
    "sfx_cha_shaggy_scooby_snack_buff_end",
    "sfx_cha_shaggy_scooby_snack_buff_start",
    "sfx_cha_shaggy_scooby_snack_despawn",
    "sfx_cha_shaggy_scooby_snack_spawn",
];

/// Rows of a character's VO table that no screen posts: its announcer pack's aliases and `store`.
fn vo_unposted(alias: &str) -> bool {
    let a = alias.to_ascii_lowercase();
    a.starts_with("announcer_") || a == "store"
}

/// Rows a preview actor alone plays.
fn vo_preview(alias: &str) -> bool {
    alias.eq_ignore_ascii_case("chaselect") || alias.eq_ignore_ascii_case("clickable")
}

fn split(all: impl IntoIterator<Item = String>) -> Lists {
    let mut seen = HashSet::new();
    let (mut keep, mut lazy) = (Vec::new(), Vec::new());
    for e in all {
        if !seen.insert(e.clone()) {
            continue;
        }
        if lazy_vo(&e) { lazy.push(e) } else { keep.push(e) }
    }
    Lists { keep, lazy }
}

fn row_event(r: &VoRow) -> Option<String> {
    r.3.as_ref().filter(|e| !e.is_empty()).map(|e| e.to_lowercase())
}

/// Every event a skin of the character brings (its override class's, its own VO table's rows'), lower case.
fn skin_events_of(t: Option<&SkinAudio>) -> HashSet<String> {
    let mut out = HashSet::new();
    let Some(t) = t else { return out };
    for s in t.skins.values() {
        out.extend(s.events.iter().cloned());
    }
    for rows in t.vo.values() {
        out.extend(rows.iter().filter_map(row_event));
    }
    out
}

/// charEvents: a character's own events: those its data names (less the VO rows no screen posts), and of its sets
/// what a match can post: the sound effects of the set its data names most; the system voice lines held as files.
/// `posted` is POSTED (the areas of each event the code posts by a literal: CHAR_UNPOSTED's test), `extra` the
/// events its graphs post by a literal its data does not name (`render_sounds`).
pub fn char_events(
    name: &str,
    default_character: &str,
    fighter: &FighterData,
    table: Option<&SkinAudio>,
    known: &Index,
    posted: &[(&str, &[&str])],
    extra: &[&str],
) -> Lists {
    // ({ ...fighter, vo }: the filtered rows in the table's own place of the data)
    let mut shown = fighter.clone();
    shown.vo = fighter.vo.as_ref().map(|rows| {
        rows.iter()
            .filter(|r| {
                !vo_unposted(&r.0)
                    && (!vo_preview(&r.0) || (name == default_character && r.0.eq_ignore_ascii_case("clickable")))
            })
            .cloned()
            .collect()
    });
    let text = serde_json::to_string(&shown).unwrap_or_default();
    let mut named = events_named(&text, known);
    named.extend(extra.iter().filter(|e| known.has(e)).map(|e| (*e).to_owned()));
    // (the set its data names most: the first of the largest counts)
    let mut counts: Vec<(String, usize)> = Vec::new();
    for e in &named {
        let Some(rest) = e.strip_prefix("sfx_cha_") else { continue };
        let Some(i) = rest.find('_').filter(|&i| i > 0) else { continue };
        match counts.iter_mut().find(|(s, _)| s == &rest[..i]) {
            Some(c) => c.1 += 1,
            None => counts.push((rest[..i].to_owned(), 1)),
        }
    }
    let sfx = counts.iter().fold(None::<&(String, usize)>, |best, c| match best {
        Some(b) if b.1 >= c.1 => Some(b),
        _ => Some(c),
    });
    let mut own: HashSet<String> = named.iter().cloned().collect();
    own.extend(fighter.vo.iter().flatten().filter_map(row_event));
    let skin_own: HashSet<String> = skin_events_of(table).into_iter().filter(|e| !own.contains(e)).collect();
    let prefix = sfx.map(|(s, _)| format!("sfx_cha_{s}_"));
    // (a sound effect of the set that nothing posts stays out unless the code names it: CHAR_UNPOSTED)
    let set_events = known
        .all()
        .iter()
        .filter(|e| prefix.as_ref().is_some_and(|p| e.starts_with(p.as_str())))
        .filter(|e| !(CHAR_UNPOSTED.contains(&e.as_str()) && !posted.iter().any(|(p, _)| p == e)))
        .cloned();
    split(named.iter().cloned().chain(set_events).filter(|e| !skin_own.contains(e)))
}

/// skinEvents: the events of a skin applied to a match's fighter, split as the character's. A skin whose AudioName is
/// the default's posts none of its override class's events.
pub fn skin_events(t: &SkinAudio, s: &SkinAudioSkin, known: &Index) -> Lists {
    let mut evs: Vec<String> =
        if s.audio_name.eq_ignore_ascii_case(&t.default) { Vec::new() } else { s.events.clone() };
    if let Some(rows) = t.vo.get(&s.vo) {
        evs.extend(rows.iter().filter(|r| !vo_unposted(&r.0) && !vo_preview(&r.0)).filter_map(row_event));
    }
    split(evs.into_iter().filter(|e| known.has(e)))
}

/// The base scope's events: the shared events (none of a character's,
/// map's, music's or class's, MATCH_UI's, nor the title's) that code outside the menus posts (`posted::POSTED`) or
/// that the HUD bundle names (`hud_named`), in the index's order, but the Chaos Wheel card's (the scope a match with
/// the wheel loads, `mvs_ui::hud::WHEEL_SOUNDS`) and Rising Lava's (`lava::SOUNDS`). The HUD's names are at hand at once, so only the second is set.
pub fn base_events(known: &Index, hud_named: &[String]) -> Vec<String> {
    let hud: HashSet<&str> = hud_named.iter().map(String::as_str).collect();
    known
        .all()
        .iter()
        .filter(|e| is_shared(e) && !is_tutorial_sound(e) && (code_posted(e) || hud.contains(e.as_str())))
        .filter(|e| !mvs_ui::hud::WHEEL_SOUNDS.contains(&e.as_str()) && !super::lava::SOUNDS.contains(&e.as_str()))
        .cloned()
        .collect()
}

/// A tutorial step machine's step sound (AudioStep's ui_tutorial_step_complete_0N): the tutorial scope's, not the
/// base's.
pub fn is_tutorial_sound(e: &str) -> bool {
    e.starts_with("ui_tutorial_step_complete_")
}

/// The shared events: all but the title's, MATCH_UI's and those of a character, map (but
/// sfx_map_shared_), blastbox (but sfx_blastbox_default_), class or music.
fn is_shared(e: &str) -> bool {
    const TITLE_EVENT: &str = "ui_menu_welcome_play";
    if e == TITLE_EVENT || is_match_ui(e) {
        return false;
    }
    let cut = e.starts_with("sfx_cha_")
        || (e.starts_with("vo_") && !e.starts_with("vo_announcer_"))
        || e.starts_with("amb_map_")
        || (e.starts_with("sfx_map_") && !e.starts_with("sfx_map_shared_"))
        || (e.starts_with("sfx_blastbox_") && !e.starts_with("sfx_blastbox_default_"))
        || e.starts_with("sfx_share_class_")
        || e.starts_with("mus_");
    !cut
}

fn is_match_ui(e: &str) -> bool {
    e.starts_with("ui_match_complete")
        || e.starts_with("ui_postmatch_")
        || e.starts_with("ui_characterselect_lightbeam")
}

/// codeBase's test: some area other than the menus' names the event in the code.
fn code_posted(e: &str) -> bool {
    posted::POSTED
        .binary_search_by(|(name, _)| (*name).cmp(e))
        .is_ok_and(|i| posted::POSTED[i].1.iter().any(|a| *a != "menu"))
}

/// The events of the match's UI that a match with a podium holds.
pub fn match_ui(known: &Index) -> Vec<String> {
    known
        .all()
        .iter()
        .filter(|e| {
            e.starts_with("ui_match_complete")
                || e.starts_with("ui_postmatch_")
                || e.starts_with("ui_characterselect_lightbeam")
        })
        .cloned()
        .collect()
}

/// The sound effects the sim's item modules post by a literal that no class default of the items part names.
const ITEM_POSTED: [&str; 22] = [
    "sfx_cha_tutorial_meeseeks_cast",
    "sfx_cha_tutorial_meeseeks_despawn",
    "vo_meeseeks_sys_spawn",
    "vo_meeseeks_sys_despawn",
    "sfx_cha_bananaguard_spawn",
    "sfx_cha_neptr_foley_tread_start",
    "sfx_cha_tomandjerry_rocket_hit",
    "sfx_cha_bugs_painttarget_impact",
    "sfx_cha_C023a_car_boost",
    "sfx_cha_C023a_car_damage",
    "sfx_cha_C023a_car_drop",
    "sfx_cha_C023a_car_engine_loop",
    "sfx_cha_C023a_car_engine_loop_stop",
    "sfx_cha_C023a_car_explode",
    "sfx_cha_C023a_car_hit",
    "sfx_cha_C023a_car_honk",
    "sfx_cha_C023a_car_honk_single",
    "sfx_cha_C023a_car_ignition",
    "sfx_cha_C023a_car_jump_in",
    "sfx_cha_C023a_car_jump_out",
    "sfx_cha_C023a_car_overheat",
    "sfx_cha_C023a_car_turn",
];

/// itemEvents: every event the items part's classes, buffs and montages name, and ITEM_POSTED (each the build has).
pub fn item_events(items_json: &str, known: &Index) -> Vec<String> {
    let mut seen = HashSet::new();
    let named = events_named(items_json, known);
    let posted = ITEM_POSTED.iter().map(|e| e.to_lowercase()).filter(|e| known.has(e)).collect::<Vec<_>>();
    named.into_iter().chain(posted).filter(|e| seen.insert(e.clone())).collect()
}

/// The default announcer pack's event (lower case) to its alias (lower case).
pub fn announcer_aliases(rows: &[mvs_data::tables::AnnouncerRow]) -> HashMap<String, String> {
    rows.iter().map(|r| (r.3.to_lowercase(), r.0.to_lowercase())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_skip_keys() {
        let v = strings_of(r#"{"a":"x","b":["y",{"c":"z\"q"}],"d":1}"#);
        assert_eq!(v, ["x", "y", "z\"q"]);
    }

    #[test]
    fn lazy_voice_lines() {
        assert!(lazy_vo("vo_shaggy_sys_ability_gen_1"));
        assert!(lazy_vo("vo_shaggy_announcer_x"));
        assert!(!lazy_vo("vo_shaggy_eff_attack"));
        assert!(!lazy_vo("sfx_cha_shaggy_sys_x"));
        assert!(!lazy_vo("vo__sys_x"));
    }
}
