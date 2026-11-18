//! A finished online match's record (the build's MatchHistoryEntry): the games' match.result reports
//! read and agreed on, the host's own facts of the match beside them, an account's lifetime counts made from it
//! (Comp_Actor_EndOfGameHandler's writes) and its stat-tracker (badge) counts. The host core builds the record, each
//! game adds its share to its stats file.

use std::collections::BTreeMap;

use indexmap::{IndexMap, IndexSet};
use serde_json::{Map, Value};

use crate::match_wire::MAX_MATCH_TICKS;
use crate::profile::{HistoryMode, MatchRecord, MatchRecordPlayer, MatchReportPlayer, ProfileTotals, TrackerCounts};

/// The port's own caps on a slot's reported numbers: far past any match the rules let run (MAX_MATCH_TICKS, 2
/// hours).
pub const DAMAGE_MAX: u32 = 1_000_000;
pub const COUNT_MAX: u32 = 999;
/// A slot's attacks dodged: at most one a tick of a 2-hour match (DodgeQuest_Update counts one per gate).
pub const DODGES_MAX: u32 = MAX_MATCH_TICKS;

/// One game's match.result, read. `map` the MapData name it played on ('' none sent); `players` None when none were
/// sent.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub team: i32,
    pub map: String,
    pub players: Option<Vec<MatchReportPlayer>>,
}

/// A player of the match as the host core set it up: `id` the player's link id, '' for a bot.
#[derive(Clone, Debug, PartialEq)]
pub struct FactPlayer {
    pub id: String,
    pub name: String,
    pub character: String,
    pub team: i32,
    pub slot: i32,
    pub bot: bool,
}

/// What the host core knows of a match itself. `map`: a lobby's MapData name; '' for a queue match.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordFacts {
    pub id: String,
    pub mode: HistoryMode,
    pub map: String,
    pub custom: bool,
    pub players: Vec<FactPlayer>,
}

/// What a report is checked against: each slot's team (by slot), the team count, the match's ring-outs to win (0:
/// none, Free For All's "Highest Score Wins").
#[derive(Clone, Debug, PartialEq)]
pub struct ReportRules {
    pub slot_teams: Vec<i32>,
    pub teams: u32,
    pub knockouts_to_win: i32,
}

/// The reports agreed on: the one kept (None: an even split) and whether two or more sent it with none differing.
#[derive(Clone, Debug, PartialEq)]
pub struct Agreed {
    pub report: Option<Report>,
    pub checked: bool,
}

/// A slot's count as sent: a whole number in 0..=max (count).
fn count(v: Option<&Value>, max: u32) -> Option<u32> {
    let x = v?.as_f64()?;
    (x.fract() == 0.0 && x >= 0.0 && x <= f64::from(max)).then(|| {
        // (a whole number in 0..=max: -0 reads as 0)
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let n = x as u32;
        n
    })
}

/// A match.result: None when it is not one (a team out of -1..teams-1, a `players` list that does not hold every
/// slot once with whole numbers in range, or numbers no match of these rules can end with: `plausible`). `map`
/// passes through the caller's name rule ('' when it fails) (readReport).
pub fn read_report(
    m: &Map<String, Value>,
    rules: &ReportRules,
    map_rule: &dyn Fn(&Value) -> Option<String>,
) -> Option<Report> {
    let slots = rules.slot_teams.len();
    let team = m.get("team").and_then(Value::as_f64)?;
    if team.fract() != 0.0 || team < -1.0 || team >= f64::from(rules.teams) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    let team = team as i32;
    let map = map_rule(m.get("map").unwrap_or(&Value::Null)).unwrap_or_default();
    let Some(list) = m.get("players") else { return Some(Report { team, map, players: None }) };
    let list = list.as_array()?;
    if list.len() != slots {
        return None;
    }
    let last = u32::try_from(slots).ok()?.checked_sub(1)?;
    let mut players: Vec<MatchReportPlayer> = Vec::new();
    for v in list {
        // (an array is read as an object with no fields)
        let o = match v {
            Value::Object(o) => o,
            Value::Array(_) => return None,
            _ => return None,
        };
        let slot = count(o.get("slot"), last);
        let damage = count(o.get("damage"), DAMAGE_MAX);
        let ringouts = count(o.get("ringouts"), COUNT_MAX);
        let deaths = count(o.get("deaths"), COUNT_MAX);
        let score = count(o.get("score"), COUNT_MAX);
        // (the stat trackers' numbers: an assisted ring-out and a double ring-out are each one of the slot's
        // ring-outs)
        let assists = count(o.get("assists"), COUNT_MAX);
        let dodges = count(o.get("dodges"), DODGES_MAX);
        let assisted = ringouts.and_then(|r| count(o.get("assisted"), r));
        let doubles = ringouts.and_then(|r| count(o.get("doubles"), r));
        let (Some(slot), Some(damage), Some(ringouts), Some(deaths), Some(score)) =
            (slot, damage, ringouts, deaths, score)
        else {
            return None;
        };
        if players.iter().any(|p| p.slot == slot) {
            return None;
        }
        let (Some(assists), Some(assisted), Some(doubles), Some(dodges)) = (assists, assisted, doubles, dodges) else {
            return None;
        };
        players.push(MatchReportPlayer { slot, damage, ringouts, deaths, score, assists, assisted, doubles, dodges });
    }
    players.sort_by_key(|p| p.slot);
    plausible(team, &players, rules).then_some(Report { team, map, players: Some(players) })
}

/// A slot's Score is its team's TeamKnockouts (PandaGameMode HandleWinLoss @5398): team-mates' are equal, and with a
/// ring-out count to win none passes it (the match ends at it) and a team at it is the winner. A ring-out credited
/// to a team (its players' RingOuts) is a death of the other team's, so in a two-team match a team's ring-outs are
/// at most the other's deaths.
/// Unverified: that the match ends with no team past knockoutsToWin (no knockout taken after the winning one
/// counts), and that no ring-out is credited for a team-mate's or one's own fall.
fn plausible(team: i32, players: &[MatchReportPlayer], rules: &ReportRules) -> bool {
    let k = rules.knockouts_to_win;
    let mut score: IndexMap<i32, u32> = IndexMap::new();
    let mut ringouts: IndexMap<i32, u32> = IndexMap::new();
    let mut deaths: IndexMap<i32, u32> = IndexMap::new();
    for p in players {
        let t = rules.slot_teams[p.slot as usize];
        if score.get(&t).is_some_and(|&s| s != p.score) {
            return false;
        }
        score.insert(t, p.score);
        *ringouts.entry(t).or_insert(0) += p.ringouts;
        *deaths.entry(t).or_insert(0) += p.deaths;
        if k > 0 && (i64::from(p.score) > i64::from(k) || i64::from(p.ringouts) > i64::from(k)) {
            return false;
        }
    }
    if k > 0 && score.iter().any(|(&t, &s)| i64::from(s) == i64::from(k) && t != team) {
        return false;
    }
    if rules.teams == 2 {
        for t in [0, 1] {
            if ringouts.get(&t).copied().unwrap_or(0) > deaths.get(&(1 - t)).copied().unwrap_or(0) {
                return false;
            }
        }
    }
    true
}

/// What two reports must share to agree: the team, the map and every slot's numbers (keyOf).
fn key_of(r: &Report) -> String {
    let nums = r.players.as_ref().map(|ps| {
        ps.iter()
            .map(|p| {
                format!(
                    "[{},{},{},{},{},{},{},{}]",
                    p.damage, p.ringouts, p.deaths, p.score, p.assists, p.assisted, p.doubles, p.dodges
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    });
    let map = serde_json::to_string(&r.map).expect("a string is JSON");
    match nums {
        Some(n) => format!("[{},{map},[{n}]]", r.team),
        None => format!("[{},{map},null]", r.team),
    }
}

/// The humans' reports, agreed on (agree; the port's rule: the build's game server read the numbers off its own
/// player states). The report most of them sent, whole, is kept: `checked` only when two or more sent it and none
/// differs. One report is kept unchecked. Reports split evenly between the most common ones keep none: the record
/// has no winner and no numbers.
/// Deviation: a loser can turn a loss into a draw by sending a false report against one honest human (an even
/// split): the host core runs no game and cannot tell which of two reports is true.
pub fn agree(reports: &[Report]) -> Agreed {
    let mut groups: IndexMap<String, Vec<&Report>> = IndexMap::new();
    for r in reports {
        groups.entry(key_of(r)).or_default().push(r);
    }
    let mut sizes: Vec<usize> = groups.values().map(Vec::len).collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    if sizes.is_empty() || sizes.get(1) == Some(&sizes[0]) {
        return Agreed { report: None, checked: false };
    }
    let best = groups.values().find(|g| g.len() == sizes[0]).expect("the largest group is one of them");
    Agreed { report: Some(best[0].clone()), checked: groups.len() == 1 && best.len() >= 2 }
}

/// The record of a match: the host core's facts, the map a queue match's games agreed on, the agreed winning team
/// (-1: none) and each slot's numbers (0 without an agreed report). A winner is a player of the winning team
/// (buildRecord).
pub fn build_record(facts: &RecordFacts, agreed: &Agreed, ended_at: u64) -> MatchRecord {
    let r = agreed.report.as_ref();
    let winning_team = r.map_or(-1, |r| r.team);
    let players = facts
        .players
        .iter()
        .map(|p| {
            let n = r
                .and_then(|r| r.players.as_ref())
                .and_then(|ps| ps.iter().find(|x| i64::from(x.slot) == i64::from(p.slot)));
            MatchRecordPlayer {
                id: if p.bot { String::new() } else { p.id.clone() },
                name: p.name.clone(),
                character: p.character.clone(),
                team: p.team,
                slot: p.slot,
                bot: p.bot,
                damage: n.map_or(0, |n| n.damage),
                ringouts: n.map_or(0, |n| n.ringouts),
                deaths: n.map_or(0, |n| n.deaths),
                score: n.map_or(0, |n| n.score),
                winner: winning_team >= 0 && p.team == winning_team,
            }
        })
        .collect();
    let map = if !facts.map.is_empty() { facts.map.clone() } else { r.map(|r| r.map.clone()).unwrap_or_default() };
    MatchRecord {
        id: facts.id.clone(),
        ended_at,
        mode: facts.mode,
        map,
        custom: facts.custom,
        winning_team,
        checked: agreed.checked,
        players,
    }
}

/// An account's totals before its first match (emptyTotals).
pub fn empty_totals() -> ProfileTotals {
    ProfileTotals::default()
}

/// An account's totals with one more match, as its player `p` of `rec` (Comp_Actor_EndOfGameHandler: matches_played
/// += 1, lifetime_ringouts += RingOuts, lifetime_damage += DamageDealt, sets_played += 1 on a set's first game).
/// Then the port's own counts: the result by mode (a draw when the record has no winner) and the games and wins by
/// character. The build skips these writes for a custom match (ShouldSkipRewards, Context 3) and for a player who
/// left (BotFromDisconnection, HandleRewards @1100-1228): the caller does not call this for them (addMatch).
/// Deviation: the build adds FTrunc(DamageDealt) to lifetime_damage (@21459) and shows Round(DamageDealt) in the
/// history (HandleWinLoss); a game sends one whole number, used for both.
pub fn add_match(t: &ProfileTotals, rec: &MatchRecord, p: &MatchRecordPlayer, set_game: u32) -> ProfileTotals {
    let result = if rec.winning_team < 0 {
        2
    } else if p.winner {
        0
    } else {
        1
    };
    let mut results = t.results.clone();
    let mode = usize::from(rec.mode.0);
    while results.len() <= mode {
        results.push([0; 3]);
    }
    results[mode][result] += 1;
    let [games, wins] = t.characters.get(&p.character).copied().unwrap_or([0, 0]);
    let mut characters = t.characters.clone();
    characters.insert(p.character.clone(), [games + 1, wins + u64::from(result == 0)]);
    ProfileTotals {
        matches: t.matches + 1,
        ringouts: t.ringouts + u64::from(p.ringouts),
        damage: t.damage + u64::from(p.damage),
        sets: t.sets + u64::from(set_game == 1),
        results,
        characters,
    }
}

// ---- the stat trackers (badges): the counts of a match, by account
//
// The build keeps the counts on its Hydra server (StatTrackerDataManager, `stat_trackers.<Name>` and the
// per-character keys + CharacterSlug) and counts only on a dedicated server (IncrementStatTracker returns at once
// without a ServerHydraManager), for every player that is not a bot.
// Deviation: the port counts only a match set up with no bot in it (a queue match that fell back to bots, a lobby
// match with a bot count nothing; a custom lobby match counts, unlike the totals), only for the humans that played it
// to the end, and only from the agreed report's numbers (agree: one unchecked report counts, a split counts nothing,
// nor does a report with no players' numbers). The numbers are the games' sims' (the host core runs no game); the
// build read them off its own player states.

/// An account's stat-tracker counts before its first match (emptyTrackers).
pub fn empty_trackers() -> TrackerCounts {
    TrackerCounts::default()
}

/// TrackMostQuests' loop over the slots in order: index 0 seeds the peak and the leader; a greater count takes
/// both, an equal one clears the leader (the peak stays); after the loop a peak above 0 with a leader that is not a
/// bot counts. None: none.
fn leader(slots: &[(u32, bool)]) -> Option<usize> {
    let mut peak = 0;
    let mut who: Option<usize> = None;
    for (i, &(n, _)) in slots.iter().enumerate() {
        if i == 0 || n > peak {
            peak = n;
            who = Some(i);
        } else if n == peak {
            who = None;
        }
    }
    who.filter(|&w| peak > 0 && !slots[w].1)
}

/// A match's stat-tracker gains by account id (the `id` of each human it counts for), in slot order: empty when it
/// does not count (above). `left`: the slots whose humans left before their result. A gain's HighestDamageDealt and
/// its characters' highest damage are the match's own, kept as a max by add_trackers; the rest are added
/// (matchTrackers).
pub fn match_trackers(
    rec: &MatchRecord,
    report: Option<&Report>,
    left: &IndexSet<i32>,
) -> IndexMap<String, TrackerCounts> {
    let mut out = IndexMap::new();
    let Some(nums) = report.and_then(|r| r.players.as_ref()) else { return out };
    if rec.players.iter().any(|p| p.bot) {
        return out;
    }
    let of = |slot: i32| {
        nums.iter()
            .find(|n| i64::from(n.slot) == i64::from(slot))
            .unwrap_or_else(|| panic!("match_trackers: the report has no slot {slot} of record {}", rec.id))
    };
    let mut by_slot: Vec<&MatchRecordPlayer> = rec.players.iter().collect();
    by_slot.sort_by_key(|p| p.slot);
    let ringout_leader = leader(&by_slot.iter().map(|p| (of(p.slot).ringouts, p.bot)).collect::<Vec<_>>());
    let assist_leader = leader(&by_slot.iter().map(|p| (of(p.slot).assists, p.bot)).collect::<Vec<_>>());
    for (i, p) in by_slot.iter().enumerate() {
        if p.bot || p.id.is_empty() || left.contains(&p.slot) {
            continue;
        }
        let n = of(p.slot);
        let mut totals: BTreeMap<String, f64> = BTreeMap::new();
        let mut add = |k: &str, v: u32| {
            if v > 0 {
                *totals.entry(k.to_string()).or_insert(0.0) += f64::from(v);
            }
        };
        // TryUpdateTeamScore @1849 -> TrackRingOutQuestsAndAchievements: a killer who is not a bot gets
        // TotalRingouts @8878 and, when the loser had a MostRecentAssistAttacker, TotalAssists @9099, both to the
        // killer's account (GetAccountID of the killer @8765 / @8986); TotalDoubleRingouts @3802 when the game time
        // is within 6 s of the killer's LastRingOutTime @3374, which is then set to now @3879 (the game's sim counts
        // `assisted` and `doubles`).
        add("TotalRingouts", n.ringouts);
        add("TotalAssists", n.assisted);
        add("TotalDoubleRingouts", n.doubles);
        // DodgeQuest_Update(HitActor) @12471: the dodger's account gets TotalAttacksDodged @13687 (the game's sim
        // keeps the handlers' 0.8 s gates)
        add("TotalAttacksDodged", n.dodges);
        // HandleWinLoss at MatchOver: TrackWinQuest (TotalWins and the character's wins for the winning team's
        // players that are not bots), TrackDamageDealingQuests @286 (TrySetHighestDamageDealt(FTrunc(DamageDealt)))
        // and TrackMostQuests @34250 (the ring-out and assist leaders: TotalRingoutLeader @1242, TotalAssists @3141)
        if p.winner {
            add("TotalWins", 1);
        }
        if ringout_leader == Some(i) {
            add("TotalRingoutLeader", 1);
        }
        if assist_leader == Some(i) {
            add("TotalAssists", 1);
        }
        // Unverified: SetHighestDamageDealt (StatTrackerManager @321) is native, not read: kept as the max.
        // Deviation: the build takes FTrunc(DamageDealt); a game sends Round(DamageDealt) (the history's number, as
        // for the totals' lifetime damage), so a match's highest damage can be one more than the build's.
        totals.insert("HighestDamageDealt".to_string(), f64::from(n.damage));
        // The per-character vectors 19-22: CharacterWins from TrackWinQuest. CharacterTotalRingouts has no
        // Blueprint or native caller of ApplyCharacterRingouts in the build (the backend's, from MatchPlayerInfo
        // CharacterSlug @5246 / Ringouts @5340); Deviation: the port adds the match's ring-outs here.
        // Unverified: CharacterHighestDamageDealt and CharacterTotalDamageDealt have no writer in the build's client
        // either (the backend filled them, as ApplyCharacterRingouts, from MatchPlayerInfo): kept as the max and the
        // sum of the match's DamageDealt.
        let mut characters = BTreeMap::new();
        characters.insert(
            p.character.clone(),
            [f64::from(u8::from(p.winner)), f64::from(n.damage), f64::from(n.damage), f64::from(n.ringouts)],
        );
        out.insert(p.id.clone(), TrackerCounts { totals, characters });
    }
    out
}

/// An account's counts with a match's gain (match_trackers) in: HighestDamageDealt and each character's highest
/// damage the max, the rest summed (addTrackers).
pub fn add_trackers(t: &TrackerCounts, gain: &TrackerCounts) -> TrackerCounts {
    let mut totals = t.totals.clone();
    for (k, &v) in &gain.totals {
        let was = totals.get(k).copied().unwrap_or(0.0);
        totals.insert(k.clone(), if k == "HighestDamageDealt" { js_max(was, v) } else { was + v });
    }
    let mut characters = t.characters.clone();
    for (slug, &[wins, high, damage, ringouts]) in &gain.characters {
        let [w0, h0, d0, r0] = characters.get(slug).copied().unwrap_or([0.0; 4]);
        characters.insert(slug.clone(), [w0 + wins, js_max(h0, high), d0 + damage, r0 + ringouts]);
    }
    TrackerCounts { totals, characters }
}

/// Math.max of two numbers: NaN when either is.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}
