//! The online queue's rules, shared by the host core and the game's queue. The build's
//! queue is the backend's: every rule here is the port's own unless a line names the build.

use serde_json::Value;

use crate::loadout::in_object;
use crate::online::{PARTY_MAX, QueueMode, QueuePlayer};
use crate::setup::{MatchPlayerWire, MatchRulesWire, MatchSetupWire};

/// queue.found as the game takes it: the server's QueueFoundMsg with its map resolved from the game's own pool and
/// its bots' perks, cosmetics and ring-out from their seeds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueFound {
    pub match_id: String,
    pub mode: QueueMode,
    pub map: String,
    pub players: Vec<QueuePlayer>,
    pub me: u32,
}

pub const QUEUE_MODES: [QueueMode; 4] = [QueueMode::OneVOne, QueueMode::TwoVTwo, QueueMode::Ffa, QueueMode::Chaos];

/// Fighters in a match of the mode (QUEUE_SIZE).
pub fn queue_size(mode: QueueMode) -> usize {
    match mode {
        QueueMode::OneVOne => 2,
        QueueMode::TwoVTwo | QueueMode::Ffa | QueueMode::Chaos => 4,
    }
}

/// The mode's ETeamStyle (QUEUE_TEAM_STYLE).
pub fn queue_team_style(mode: QueueMode) -> u8 {
    match mode {
        QueueMode::OneVOne => 0,
        QueueMode::TwoVTwo | QueueMode::Chaos => 1,
        QueueMode::Ffa => 2,
    }
}

// ---------- parties ----------

/// A ticket waits this long from its first member's join for the rest; then it is out, its members told queue.state
/// off `QueueWhy::PartyLate`.
pub const PARTY_WAIT_MS: f64 = 10000.0;

/// The most players a ticket of the mode may hold: 1v1 none but a solo.
/// Deviation: a party of two may queue FFA; the build's WBP_MatchOptions turns FFA off for a full party too
/// (@1300 / @1411), the port keeps it on (the user's choice).
pub fn ticket_max(mode: QueueMode) -> usize {
    if mode == QueueMode::OneVOne { 1 } else { PARTY_MAX }
}

/// The wait before a group of waiting humans is filled with bots (Deviation: the port's 60 s; the build's is the
/// backend's MatchManager.OnTimeout); a game may ask for a shorter one, never under BOT_AFTER_MIN_S.
pub const BOT_AFTER_S: f64 = 60.0;
pub const BOT_AFTER_MIN_S: f64 = 3.0;

/// A join's bot wait in seconds (botAfterS).
pub fn bot_after_s(v: &Value) -> f64 {
    match v.as_f64() {
        Some(n) => BOT_AFTER_S.min(BOT_AFTER_MIN_S.max(n)),
        None => BOT_AFTER_S,
    }
}

// ---------- pairing by distance (the port's own: the build's queue is the backend's) ----------

/// Tickets whose round trips to the relay (queue.join's `rtt`) differ by at most this (ms) are paired at once.
pub const PAIR_RTT_MS: f64 = 30.0;
/// The difference allowed grows by this (ms) for each second the group's first ticket has waited.
pub const PAIR_WIDEN_MS_PER_S: f64 = 10.0;
/// From this wait of the first ticket on (ms), any round trip pairs with any.
pub const PAIR_ANY_AFTER_MS: f64 = 20000.0;

/// The largest difference of round trips a group of tickets may hold once its first ticket has waited `waited_ms`
/// (infinite: any). Unverified: the port's own rule.
pub fn rtt_band(waited_ms: f64) -> f64 {
    if waited_ms >= PAIR_ANY_AFTER_MS {
        return f64::INFINITY;
    }
    PAIR_RTT_MS + PAIR_WIDEN_MS_PER_S * waited_ms.max(0.0) / 1000.0
}

/// Whether a ticket of round trip `rtt` may join a group whose round trips run from lo to hi (`span`; None: no
/// ticket of it has one yet) under `band` (rtt_band). A ticket with no round trip goes with any group, and any group
/// with it.
pub fn rtt_fits(span: Option<(f64, f64)>, rtt: Option<f64>, band: f64) -> bool {
    match (span, rtt) {
        (Some((lo, hi)), Some(r)) => hi.max(r) - lo.min(r) <= band,
        _ => true,
    }
}

/// The pre-match after queue.found (UI_PreMatch: Match Found 2.0 s, ShowVS at 1.4667 s, Delay 2.0, the map's
/// reveal 1.5 s and 0.55 s, the header's fade 0.5 s, the perk screen's 40 s timer) and 2 s of margin:
/// ceil((2.0 + 1.4667 + 2.0 + 1.5 + 0.55 + 0.5 + 40 + 2) * 1000).
pub const PREMATCH_MS: f64 = 50017.0;
pub const MAX_PERKS: usize = 4;

/// The pick's time from pm.begin: the rematch flow's Delay 1.0 (PGI_MatchmakingHandler @29602), 0.2 (UI_PreMatch
/// @61163), the 1.5 s intro (@49817), the 45 s pick budget (@44985) and 3 s of margin:
/// ceil((1.0 + 0.2 + 1.5 + 45 + 3) * 1000).
pub const PM_PICK_MS: f64 = 50700.0;
/// The perks' time from pm.all on the rematch path (UI_PreMatch) and 2 s of margin:
/// ceil((2.0 + 1.4667 + 2.0 + 0.5 + 40 + 2) * 1000).
pub const PM_PERKS_MS: f64 = 47967.0;
/// Bots lock at pm.begin + 0.4 s (PreMatchWidget botSelects' 0.2 + 0.2); on a rematch they relock 3.0 s in
/// (CoopVsAIManager HandleRematch CO@5385 -> LockInBotCharacters).
pub const PM_BOT_LOCK_MS: f64 = 400.0;
pub const PM_BOT_RELOCK_MS: f64 = 3000.0;

/// The best-of-3 set of a queue match (GetSetLengthForConfig: MaximumWins 2 in context 1 Matchmade).
pub const SET_WINS: u32 = 2;
/// The games of a set at most: a game with no winner counts as played.
pub const SET_GAMES: u32 = 3;

/// FFA's set is one game (GetSetLengthForConfig MaximumWins 1 for team style FFA in context 1; the port's own: a game
/// of it with no winner ends the set too), and so is Chaos's (GetSetLengthForConfig: queue type 3, Casual, one game).
fn one_game(mode: QueueMode) -> bool {
    matches!(mode, QueueMode::Ffa | QueueMode::Chaos)
}

/// A mode's wins that decide its set (FFA and Chaos one game).
pub fn set_wins(mode: QueueMode) -> u32 {
    if one_game(mode) { 1 } else { SET_WINS }
}

/// A mode's games of a set at most.
pub fn set_games(mode: QueueMode) -> u32 {
    if one_game(mode) { 1 } else { SET_GAMES }
}

/// The vote (PodiumGameMode): TimeToVoteForRematch 30 s (PGM@18484); the game's own step 12 s (UI_EOG_RematchStatus
/// RS@7858); the bots' votes from Delay 2.0 (PGM@6841), one slot after another 0.25-1.0 s apart (BotsVoteForRematch
/// PGM@947); all yes: Delay 5.5 (PGM@7110) and StartPlayAgain's Delay 0.5.
pub const VOTE_MS: f64 = 30000.0;
pub const VOTE_PAGE_S: f64 = 12.0;
pub const VOTE_BOT_MS: f64 = 2000.0;
pub const VOTE_BOT_GAP_MS: [f64; 2] = [250.0, 1000.0];
pub const REMATCH_MS: f64 = 5500.0 + 500.0;

/// The humans' match.result: one missing this long after the first result or the match's end, or two that differ,
/// end the set.
pub const RESULT_MS: f64 = 15000.0;

/// The team count of the mode (queueTeams): the slots alternate between the two teams of 1v1 and 2v2, each FFA slot
/// is a team of its own.
pub fn queue_teams(mode: QueueMode) -> usize {
    if queue_team_style(mode) == 2 { queue_size(mode) } else { 2 }
}

/// A slot's team: PlayerIndex % team count (AssignPlayerAndTeamIndex) (queueTeam).
pub fn queue_team(mode: QueueMode, slot: usize) -> usize {
    slot % queue_teams(mode)
}

/// The slots of a match's tickets (their sizes, in the order they joined): each ticket's slots, or None when they do
/// not fit the mode (queueSlots). In 1v1 and 2v2 a ticket's humans share a team: the larger tickets first, each on
/// the team with the fewest humans that has room for it whole (the lower team on a tie), at its lowest free slots.
/// Two solos of a 2v2 are on opposite teams, each with a bot; a party of two plays the two bots (Get Bots @770: 2
/// bots for a party of more than 1). In FFA each human takes the next slot. The bots take the slots left.
pub fn queue_slots(mode: QueueMode, tickets: &[u32]) -> Option<Vec<Vec<usize>>> {
    let size = queue_size(mode);
    let teams = queue_teams(mode);
    let total: usize = tickets.iter().map(|n| *n as usize).sum();
    if total > size || tickets.iter().any(|n| *n < 1 || *n as usize > ticket_max(mode)) {
        return None;
    }
    let mut out: Vec<Vec<usize>> = tickets.iter().map(|_| Vec::new()).collect();
    if queue_team_style(mode) == 2 {
        let mut next = 0;
        for (i, n) in tickets.iter().enumerate() {
            for _ in 0..*n {
                out[i].push(next);
                next += 1;
            }
        }
        return Some(out);
    }
    let mut taken = vec![0usize; teams];
    let room = size / teams;
    let mut order: Vec<usize> = (0..tickets.len()).collect();
    order.sort_by(|a, b| tickets[*b].cmp(&tickets[*a]).then(a.cmp(b)));
    for i in order {
        let n = tickets[i] as usize;
        let mut team: Option<usize> = None;
        for t in 0..teams {
            if taken[t] + n <= room && team.is_none_or(|b| taken[t] < taken[b]) {
                team = Some(t);
            }
        }
        let team = team?;
        for _ in 0..n {
            out[i].push(team + taken[team] * teams);
            taken[team] += 1;
        }
    }
    Some(out)
}

/// The bots' fighters (GetRandomBotLoadouts): drawn at random from the pool, none twice and none a human's pick;
/// when the pool runs dry a pick may repeat. `rand(n)` an integer in 0..n (pickBots). An empty pool gives
/// no picks.
pub fn pick_bots(pool: &[String], humans: &[String], n: usize, rand: &mut dyn FnMut(u32) -> u32) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut left: Vec<&String> = pool.iter().filter(|c| !humans.contains(c)).collect();
    for _ in 0..n {
        if left.is_empty() {
            left = pool.iter().filter(|c| !out.contains(c)).collect();
        }
        if left.is_empty() {
            left = pool.iter().collect();
        }
        if left.is_empty() {
            break;
        }
        let at = (rand(u32::try_from(left.len()).unwrap_or(u32::MAX)) as usize).min(left.len() - 1);
        let c = left[at].clone();
        left.retain(|x| **x != c);
        out.push(c);
    }
    out
}

/// The bots' level: full (MatchSetup's 0..3). Deviation: the build's class default is Hard, VeryEasy after a loss
/// streak.
pub const QUEUE_BOT_DIFFICULTY: i32 = 3;

/// Deviation: a 1v1 queue match is won at 3 ring-outs, a 2v2 or Chaos one at 4 (the custom lobby's
/// SetNumRingoutsForWin(4)): the build's queue rules are the backend's match config, not in its files. An FFA match
/// has none (0): it ends by time.
pub const QUEUE_RINGOUTS: i32 = 3;
/// The team queues' ring-outs (2v2 and Chaos).
pub const QUEUE_TEAM_RINGOUTS: i32 = 4;

/// The ring-outs that win a queued match of `mode`.
pub fn queue_ringouts(mode: QueueMode) -> i32 {
    match mode {
        QueueMode::OneVOne => QUEUE_RINGOUTS,
        QueueMode::TwoVTwo | QueueMode::Chaos => QUEUE_TEAM_RINGOUTS,
        QueueMode::Ffa => 0,
    }
}

/// The match's set-up in slot order, with the mode's ring-outs and `duration` (queueSetup). Deviation: a queued
/// match's time is the backend's match config, not in the build's files.
pub fn queue_setup(mode: QueueMode, players: &[QueuePlayer], duration: u32) -> MatchSetupWire {
    MatchSetupWire {
        team_style: queue_team_style(mode),
        players: players
            .iter()
            .map(|p| {
                if p.bot {
                    MatchPlayerWire {
                        bot: true,
                        difficulty: Some(QUEUE_BOT_DIFFICULTY),
                        character: Some(p.character.clone()),
                        perks: Some(Vec::new()),
                        ..MatchPlayerWire::default()
                    }
                } else {
                    MatchPlayerWire {
                        bot: false,
                        character: (!p.character.is_empty()).then(|| p.character.clone()),
                        perks: Some(Vec::new()),
                        ..MatchPlayerWire::default()
                    }
                }
            })
            .collect(),
        rules: Some(MatchRulesWire { knockouts_to_win: queue_ringouts(mode), duration, stocks: None }),
        flow: Some(true),
        ..MatchSetupWire::default()
    }
}

/// A pm.perks list as a game sent it: the slots in order, cut to MAX_PERKS, '' for anything not a perk id: letters,
/// digits and underscores, at most 64, no name every object has (cleanPerks; UI_PreMatch HandlePerkSelected @31031:
/// each perk with its slot).
pub fn clean_perks(v: &Value) -> Vec<String> {
    let Some(list) = v.as_array() else { return Vec::new() };
    list.iter()
        .take(MAX_PERKS)
        .map(|p| match p.as_str() {
            Some(s)
                if !s.is_empty()
                    && s.len() <= 64
                    && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    && !in_object(s) =>
            {
                s.to_owned()
            }
            _ => String::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rtt_band_widens_with_the_wait() {
        assert_eq!(rtt_band(0.0), 30.0);
        assert_eq!(rtt_band(-5.0), 30.0);
        assert_eq!(rtt_band(1000.0), 40.0);
        assert_eq!(rtt_band(10000.0), 130.0);
        assert_eq!(rtt_band(19999.0), 30.0 + 10.0 * 19999.0 / 1000.0);
        assert_eq!(rtt_band(PAIR_ANY_AFTER_MS), f64::INFINITY);
        assert!(rtt_fits(Some((20.0, 40.0)), Some(50.0), 30.0));
        assert!(!rtt_fits(Some((20.0, 40.0)), Some(51.0), 30.0));
        assert!(!rtt_fits(Some((20.0, 40.0)), Some(9.0), 30.0));
        assert!(rtt_fits(Some((20.0, 40.0)), None, 0.0));
        assert!(rtt_fits(None, Some(500.0), 0.0));
        assert!(rtt_fits(Some((20.0, 400.0)), Some(30.0), f64::INFINITY));
    }

    #[test]
    fn times_as_the_ts_computes_them() {
        assert_eq!(((2.0 + 1.4667 + 2.0 + 1.5 + 0.55 + 0.5 + 40.0 + 2.0) * 1000.0_f64).ceil(), PREMATCH_MS);
        assert_eq!(((1.0 + 0.2 + 1.5 + 45.0 + 3.0) * 1000.0_f64).ceil(), PM_PICK_MS);
        assert_eq!(((2.0 + 1.4667 + 2.0 + 0.5 + 40.0 + 2.0) * 1000.0_f64).ceil(), PM_PERKS_MS);
    }
}
