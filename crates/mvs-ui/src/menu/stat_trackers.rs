//! Stat trackers: a badge's count. What a badge shows for a count is
//! `widgets::badges` (badge_look, badge_text).
//!
//! The build keeps the counts on its Hydra server (StatTrackerDataManager, `stat_trackers.<Name>` and the
//! per-character keys) and counts only on a dedicated server (IncrementStatTracker returns at once without a
//! ServerHydraManager). Deviation: every game counts every slot of an online match in its sim and reports them; the
//! session's host core compares the reports and applies the rules (which matches count, the bots, the wins and
//! leaders, HighestDamageDealt), and each player's game keeps its own counts in its stats file
//! (`mvs_settings::stats`). The caller passes the counts of the stats file's own profile (none: empty, the counts 0).

use mvs_protocol::profile::TrackerCounts;

use crate::bundle::{CosmeticItem, Cosmetics};
use crate::menu::widgets::badges::BadgeItem;

/// EStatTrackerTotalVector by byte (FEnumParams 144c14c70), the names the profile keeps them under.
pub const TOTAL_VECTOR: [&str; 29] = [
    "InvalidTotalStatTrackerVector",
    "TotalWins",
    "TotalRingouts",
    "TotalDoubleRingouts",
    "TotalAssists",
    "TotalAttacksDodged",
    "TotalRingoutLeader",
    "TotalMaxedCharacters",
    "TotalPointsScored",
    "EVO2022_1stPlace",
    "EVO2022_2ndPlace",
    "EVO2022_3rdPlace",
    "EVO2022_4thPlace",
    "EVO2022_Top8",
    "EVO2022_Top16",
    "EVO2022_Top32",
    "EVO2022_Top64",
    "TotalArcadeHardCompletions",
    "HighestDamageDealt",
    "CharacterWins",
    "CharacterHighestDamageDealt",
    "CharacterTotalDamageDealt",
    "CharacterTotalRingouts",
    "Winter2022Currency",
    "Valentines2023Currency",
    "Season3Rank1v1",
    "Season3Rank2v2",
    "TotalToastsGiven",
    "TotalToastsReceived",
];

/// Vectors 19-22 are the build's per-character profile keys (GetStatTrackerCount 1411e88e0, `stat_trackers.` + key +
/// CharacterSlug): `character_wins.`, `character_highest_damage_dealt.`, `character_total_damage_dealt.`,
/// `character_ringouts.`. TrackerCounts holds them in that order, by character.
const FIRST_CHARACTER_VECTOR: u32 = 19;

/// StatTrackerDataManager.GetStatTrackerCount (exec 14126e270 -> impl 1411e88e0, E.2) of a bundle. `c` gives a
/// CharacterData slug's roster key (the server's id of the character). `arcade_hard`: TotalArcadeHardCompletions
/// (vector 17) is this machine's: the build's Comp_Actor_EndOfGameHandler HandleRewards counts a Hard arcade run won
/// (IncrementStatTracker(17) @18699); the port's arcade runs are offline and solo, so the local arcade record keeps
/// the count (`StatsArcade::hard_completions`), not the online totals.
pub fn stat_tracker_count(item: &CosmeticItem, c: &Cosmetics, t: &TrackerCounts, arcade_hard: u32) -> f64 {
    let v = item.vec.unwrap_or(0);
    let of_character = |vec: u32| -> f64 {
        let key =
            item.char_slug.as_deref().filter(|s| !s.is_empty()).and_then(|s| c.characters.get(s)).map(|ch| &ch.key);
        key.and_then(|k| t.characters.get(k)).map_or(0.0, |r| r[(vec - FIRST_CHARACTER_VECTOR) as usize])
    };
    match v {
        17 => f64::from(arcade_hard),
        1..=18 | 23..=28 => t.totals.get(TOTAL_VECTOR[v as usize]).copied().unwrap_or(0.0),
        19..=22 => of_character(v),
        // the default case (vector 0): CharacterWins of a CharacterSlug, else the SeasonSlug's seasonal struct (the
        // port has no season data: 0), else 0
        _ if item.char_slug.as_deref().is_some_and(|s| !s.is_empty()) => of_character(FIRST_CHARACTER_VECTOR),
        _ => 0.0,
    }
}

/// What the badge widgets read of a catalogue item.
pub fn badge_item(e: &CosmeticItem) -> BadgeItem {
    BadgeItem {
        name: e.name.clone(),
        rarity: e.rarity.clone(),
        stars: e.stars.clone(),
        rev: e.rev,
        show: e.show,
        badge: e.thumb.clone(),
        tex: e.tex.clone(),
        tiers: e.tiers.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::CosCharacter;

    #[test]
    fn counts_by_vector() {
        let mut c = Cosmetics::default();
        c.characters
            .insert("character_shaggy".into(), CosCharacter { key: "shaggy".into(), ..CosCharacter::default() });
        let mut t = TrackerCounts::default();
        t.totals.insert("TotalRingouts".into(), 7.0);
        t.characters.insert("shaggy".into(), [3.0, 4.0, 5.0, 6.0]);
        let it =
            |vec: Option<u32>, ch: &str| CosmeticItem { vec, char_slug: Some(ch.into()), ..CosmeticItem::default() };
        assert_eq!(stat_tracker_count(&it(Some(2), ""), &c, &t, 9), 7.0);
        assert_eq!(stat_tracker_count(&it(Some(17), ""), &c, &t, 9), 9.0);
        assert_eq!(stat_tracker_count(&it(Some(21), "character_shaggy"), &c, &t, 9), 5.0);
        assert_eq!(stat_tracker_count(&it(None, "character_shaggy"), &c, &t, 9), 3.0);
        assert_eq!(stat_tracker_count(&it(Some(20), "character_nobody"), &c, &t, 9), 0.0);
        assert_eq!(stat_tracker_count(&it(None, ""), &c, &t, 9), 0.0);
    }
}
