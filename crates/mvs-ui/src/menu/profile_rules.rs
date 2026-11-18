//! The profile page's values made of the profile answer (`mvs_protocol::profile`) and the level table
//! (`bundle::AccountLevelRow`): what the build's backend handed UI_Profile_v2 in its PlayerProfile, by the port's own
//! rules where the build has none.

use mvs_protocol::profile::{MatchRecord, MatchRecordPlayer, ProfileTotals};

use crate::bundle::AccountLevelRow;

/// EGameMode as the history reads it.
pub const MODE_SOLO: usize = 0;
pub const MODE_TEAMS: usize = 1;
/// TeamIndex of a spectator (UI_MatchHistoryEntry @2019, MakeLiteralByte(4)).
pub const SPECTATOR_TEAM: i32 = 4;

/// Unverified: (the port's own) a win rate is wins / games of the mode, a draw a game and no win; -1 ("N/A",
/// GetWinRateText) with no game of the mode. PlayerProfile TeamsWinRate / SoloWinRate are the backend's, whose rule
/// the build does not hold.
pub fn win_rate(t: &ProfileTotals, mode: usize) -> f64 {
    let [w, l, d] = t.results.get(mode).copied().unwrap_or([0; 3]);
    let games = w + l + d;
    if games > 0 { w as f64 / games as f64 } else { -1.0 }
}

/// Unverified: (the port's own) MostPlayedCharacterSlug, the backend's in the build: the character of the most
/// games; a tie goes to the one with more wins, then to the lower id in code-point order. "" with no game.
pub fn most_played(t: &ProfileTotals) -> String {
    let mut best = String::new();
    let (mut bg, mut bw) = (0, 0);
    for (id, &[games, wins]) in &t.characters {
        if games == 0 {
            continue;
        }
        // (the map iterates in code-point order of the key, as the tie rule compares: `id < best`)
        if games > bg || (games == bg && (wins > bw || (wins == bw && id.as_str() < best.as_str()))) {
            best = id.clone();
            bg = games;
            bw = wins;
        }
    }
    best
}

/// UI_Profile_v2 GetLevelData / UI_AccountLevel_XPBar Init @1672: LevelUpData[level - 1], else (an invalid index)
/// the last row.
pub fn level_row(levels: &[AccountLevelRow], level: i64) -> AccountLevelRow {
    usize::try_from(level - 1).ok().and_then(|i| levels.get(i)).or(levels.last()).copied().unwrap_or_default()
}

/// An XP bar's fill: `xp` over the row's `required`.
/// Deviation: a row of no XP (an empty level table: the bundle without the account levels) fills nothing; the build's
/// table is never empty.
pub fn xp_fraction(xp: f64, required: f64) -> f64 {
    if required > 0.0 { xp / required } else { 0.0 }
}

/// The highest level shown.
const LEVEL_CAP: usize = 99;

/// Deviation: the account level and XP the page shows. The build's are the account's (CurrentXP and Level, raised by
/// Comp_Actor_EndOfGameHandler's level-up loop on the game server); the port keeps no XP and shows every account at
/// the table's top level (its last row's, 49 in this build), at most 99, its XP that row's whole: the bar full and
/// no XP left. Returns (level, xp).
pub fn shown_level(levels: &[AccountLevelRow]) -> (i64, f64) {
    let level = levels.len().min(LEVEL_CAP) as i64;
    (level, level_row(levels, level).xp)
}

/// A record as UI_MatchHistoryEntry and UI_MatchBreakdown read it (MatchHistoryEntry): the profile owner's own line
/// (PlayerStats; TeamIndex 4 when the owner is not among the players, the build's spectator) and its result.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    pub record: MatchRecord,
    pub player_stats: MatchRecordPlayer,
    /// EMatchResult: 0 Win, 1 Loss, 2 Draw.
    pub match_result: usize,
}

/// The owner's line and result. Unverified: (the port's own) the record holds no result per player: a draw when no
/// team won (winningTeam -1), else a win for the winning team's players and a loss for the others.
pub fn history_entry(r: &MatchRecord, owner: &str) -> HistoryEntry {
    let me = r.players.iter().find(|p| !p.bot && p.id == owner);
    let player_stats = me.cloned().unwrap_or_else(|| MatchRecordPlayer {
        id: owner.to_string(),
        team: SPECTATOR_TEAM,
        slot: -1,
        ..MatchRecordPlayer::default()
    });
    let match_result = if r.winning_team < 0 {
        2
    } else if player_stats.team == r.winning_team {
        0
    } else {
        1
    };
    HistoryEntry { record: r.clone(), player_stats, match_result }
}

/// UI_MatchBreakdown PopulateMatchPlayers: "You" and the players of its team first (You at 0), then the others, in
/// the record's order; spectators (team 4) left out.
pub fn breakdown_teams(e: &HistoryEntry) -> (Vec<MatchRecordPlayer>, Vec<MatchRecordPlayer>) {
    let r = &e.record;
    let spectated = e.player_stats.team == SPECTATOR_TEAM;
    // the first loop (@291-1011): a spectator's You is the first player of the winning team, else the player of the
    // owner's Username (@3224); the loop skips team 4 and stops on its hit
    let you = r.players.iter().find(|p| {
        p.team != SPECTATOR_TEAM && (if spectated { p.team == r.winning_team } else { p.name == e.player_stats.name })
    });
    let mut yours: Vec<MatchRecordPlayer> = you.cloned().into_iter().collect();
    let mut theirs = Vec::new();
    // the second loop (@2116): every other player by its team against You's (none found: an empty You, team 0)
    let you_name = you.map_or("", |p| p.name.as_str());
    let you_team = you.map_or(0, |p| p.team);
    for p in &r.players {
        if p.team == SPECTATOR_TEAM || p.name == you_name {
            continue;
        }
        if p.team == you_team { yours.push(p.clone()) } else { theirs.push(p.clone()) }
    }
    (yours, theirs)
}

#[cfg(test)]
mod tests {
    use mvs_protocol::profile::HistoryMode;

    use super::*;

    #[test]
    fn an_empty_level_table_fills_no_bar() {
        let (level, xp) = shown_level(&[]);
        assert_eq!(xp_fraction(xp, level_row(&[], level.max(1)).xp), 0.0);
        assert_eq!(xp_fraction(5.0, 10.0), 0.5);
    }

    fn player(name: &str, team: i32, slot: i32) -> MatchRecordPlayer {
        MatchRecordPlayer {
            id: format!("id-{name}"),
            name: name.into(),
            character: "shaggy".into(),
            team,
            slot,
            ..MatchRecordPlayer::default()
        }
    }

    fn record(winning_team: i32, players: Vec<MatchRecordPlayer>) -> MatchRecord {
        MatchRecord { id: "r".into(), mode: HistoryMode::TEAMS, winning_team, players, ..MatchRecord::default() }
    }

    fn rows(xps: &[f64]) -> Vec<AccountLevelRow> {
        xps.iter().map(|&xp| AccountLevelRow { xp, gold: 1.0 }).collect()
    }

    #[test]
    fn win_rate_counts_draws_as_games() {
        let mut t = ProfileTotals::default();
        assert_eq!(win_rate(&t, MODE_SOLO), -1.0);
        t.results[MODE_TEAMS] = [3, 1, 0];
        assert_eq!(win_rate(&t, MODE_TEAMS), 0.75);
        t.results[MODE_TEAMS] = [1, 0, 1];
        assert_eq!(win_rate(&t, MODE_TEAMS), 0.5);
        t.results.clear();
        assert_eq!(win_rate(&t, MODE_TEAMS), -1.0);
    }

    #[test]
    fn most_played_breaks_ties_by_wins_then_the_lower_id() {
        let mut t = ProfileTotals::default();
        assert_eq!(most_played(&t), "");
        t.characters.insert("shaggy".into(), [5, 1]);
        t.characters.insert("batman".into(), [5, 2]);
        t.characters.insert("arya".into(), [5, 2]);
        t.characters.insert("finn".into(), [0, 0]);
        assert_eq!(most_played(&t), "arya");
        t.characters.insert("jake".into(), [6, 0]);
        assert_eq!(most_played(&t), "jake");
    }

    #[test]
    fn the_level_is_the_tables_top_row_at_most_99() {
        let l = rows(&[10.0, 20.0, 30.0]);
        assert_eq!(level_row(&l, 2).xp, 20.0);
        assert_eq!(level_row(&l, 0).xp, 30.0);
        assert_eq!(level_row(&l, 9).xp, 30.0);
        assert_eq!(shown_level(&l), (3, 30.0));
        assert_eq!(shown_level(&rows(&vec![1.0; 120])).0, 99);
        assert_eq!(shown_level(&[]), (0, 0.0));
    }

    #[test]
    fn a_history_entry_reads_the_owners_line_and_result() {
        let r = record(0, vec![player("me", 0, 0), player("them", 1, 1)]);
        let e = history_entry(&r, "id-me");
        assert_eq!((e.player_stats.name.as_str(), e.match_result), ("me", 0));
        assert_eq!(history_entry(&r, "id-them").match_result, 1);
        let s = history_entry(&r, "id-nobody");
        assert_eq!((s.player_stats.team, s.match_result), (SPECTATOR_TEAM, 1));
        assert_eq!(history_entry(&record(-1, r.players.clone()), "id-me").match_result, 2);
    }

    #[test]
    fn the_breakdown_puts_you_first_and_leaves_spectators_out() {
        let r = record(1, vec![player("a", 0, 0), player("me", 1, 1), player("b", 1, 2), player("w", 4, 3)]);
        let (yours, theirs) = breakdown_teams(&history_entry(&r, "id-me"));
        assert_eq!(yours.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["me", "b"]);
        assert_eq!(theirs.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["a"]);
        // a spectated match: You is the first player of the winning team
        let (yours, _) = breakdown_teams(&history_entry(&r, "id-watcher"));
        assert_eq!(yours[0].name, "me");
    }
}
