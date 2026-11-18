//! The profile (Career) screen's data on the wire: what each game reports at a match's end and
//! what the server answers a profile request with. Each game keeps its own records and the protocol holds no game
//! data: characters and maps travel as the ids the pre-match already uses, the XP and level are the game's own.
//! Types and limits only; the JSON names are camelCase.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One slot's numbers at the end of a match, as a game's sim saw them (PFGPlayerState DamageDealt, RingOuts,
/// Deaths; Score the slot's team's knockouts in PandaGameState TeamKnockouts). The rest feed the stat trackers,
/// counted in the sim's own state: `assists` the player state's Assists (the assist leader), `assisted` its ring-outs
/// whose loser had a MostRecentAssistAttacker, `doubles` its double ring-outs, `dodges` its attacks dodged
/// (DodgeQuest_Update).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchReportPlayer {
    pub slot: u32,
    pub damage: u32,
    pub ringouts: u32,
    pub deaths: u32,
    pub score: u32,
    pub assists: u32,
    pub assisted: u32,
    pub doubles: u32,
    pub dodges: u32,
}

/// EGameMode as the match history shows it: 0 Solo ("1 vs 1"), 1 Teams, 2 FreeForAll.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HistoryMode(pub u8);

impl HistoryMode {
    pub const SOLO: HistoryMode = HistoryMode(0);
    pub const TEAMS: HistoryMode = HistoryMode(1);
    pub const FREE_FOR_ALL: HistoryMode = HistoryMode(2);
    /// The modes a record can hold (ProfileTotals.results has one entry per mode).
    pub const COUNT: usize = 3;
}

/// One player of a match record (MatchPlayerInfo + IsWinner; `id` the public account id, "" for a bot).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchRecordPlayer {
    pub id: String,
    pub name: String,
    pub character: String,
    pub team: i32,
    pub slot: i32,
    pub bot: bool,
    pub damage: u32,
    pub ringouts: u32,
    pub deaths: u32,
    pub score: u32,
    pub winner: bool,
}

/// One finished online match (MatchHistoryEntry). `checked` false: its numbers rest on one game's report alone, or
/// the reports disagreed and the most common one was kept.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchRecord {
    pub id: String,
    /// Milliseconds since 1970 (order only: the build shows no date).
    pub ended_at: u64,
    pub mode: HistoryMode,
    /// The MapData name it was played on ("" when the reports split).
    pub map: String,
    pub custom: bool,
    /// -1: none.
    pub winning_team: i32,
    pub checked: bool,
    pub players: Vec<MatchRecordPlayer>,
}

/// An account's lifetime counts (PlayerProfile's Lifetime fields and the counts the win rates and the most played
/// fighter are made from: the port's own rule). Whole numbers; a sum stops at the stats file's cap (1e12), past
/// `u32`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileTotals {
    pub matches: u64,
    pub ringouts: u64,
    pub damage: u64,
    pub sets: u64,
    /// By HistoryMode: [wins, losses, draws].
    pub results: Vec<[u64; 3]>,
    /// By character slug: [games, wins].
    pub characters: BTreeMap<String, [u64; 2]>,
}

impl Default for ProfileTotals {
    /// Every count 0, one result row per mode.
    fn default() -> ProfileTotals {
        ProfileTotals {
            matches: 0,
            ringouts: 0,
            damage: 0,
            sets: 0,
            results: vec![[0; 3]; HistoryMode::COUNT],
            characters: BTreeMap::new(),
        }
    }
}

/// The stat-tracker totals the server counts (EStatTrackerTotalVector's names, the build's `stat_trackers.<Name>`).
pub const TRACKER_TOTALS: [&str; 7] = [
    "TotalWins",
    "TotalRingouts",
    "TotalDoubleRingouts",
    "TotalAssists",
    "TotalAttacksDodged",
    "TotalRingoutLeader",
    "HighestDamageDealt",
];

/// An account's stat-tracker counts (the build's StatTrackerDataManager, kept on its Hydra server): the totals by
/// name (a name of TRACKER_TOTALS; absent: 0) and, by character slug, the per-character vectors 19-22 in order:
/// [wins, highest damage dealt, total damage dealt, ring-outs].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerCounts {
    pub totals: BTreeMap<String, f64>,
    pub characters: BTreeMap<String, [f64; 4]>,
}

/// The answer to profile.get. `matches` newest first, at most PROFILE_MATCHES.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileWire {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub totals: ProfileTotals,
    pub trackers: TrackerCounts,
    pub matches: Vec<MatchRecord>,
}

/// GetPlayerProfile's NumMatchesToLoad (UI_Profile_v2's class default): the matches a profile answer carries.
pub const PROFILE_MATCHES: usize = 10;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_names_are_the_ts_ones() {
        let r = MatchRecord { ended_at: 5, mode: HistoryMode::TEAMS, winning_team: -1, ..MatchRecord::default() };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["endedAt"], 5);
        assert_eq!(v["mode"], 1);
        assert_eq!(v["winningTeam"], -1);
        let t = serde_json::to_value(ProfileTotals::default()).unwrap();
        assert_eq!(t["results"], serde_json::json!([[0, 0, 0], [0, 0, 0], [0, 0, 0]]));
        let back: ProfileTotals = serde_json::from_value(t).unwrap();
        assert_eq!(back, ProfileTotals::default());
    }
}
