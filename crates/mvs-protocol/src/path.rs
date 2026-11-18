//! A match's path (online plan v4 section 5): whether the relay referees a match or one human's app does, over Epic's
//! links. The host core asks [`should_measure`] at the pre-match's start; when it says yes each game pings every other
//! human over Epic ([`crate::match_wire::path_ping`]) and reports its smallest round trips (`path.report`), and the
//! core's [`choose`] takes the referee from the reports and the round trips to the relay.
//!
//! A pair's round trip is the larger of its two sides' reports (a game that says less cannot shorten a pair), and a
//! missing report is a link that did not stand. A human is a candidate only when its link to every other human stood
//! (the packets go in a star through its app); its cost is the worst pair's one-way time through it, half the sum of
//! the two round trips to it (its own 0). The relay's is the worst pair's half sum of round trips to the relay. The
//! best candidate is taken when it is at least [`PATH_GAIN_MS`] under the relay's; else the relay.
//!
//! Deviation: the port's own rule and constants (the build's matches go through its own servers).

use crate::match_wire::{DEADLINE, MAX_LOCAL_DELAY, TICK_MS};

/// A match whose humans all have a round trip to the relay at or under this (ms) is not measured: the relay referees.
pub const NEAR_MS: f64 = 60.0;
/// A human's app referees only when its worst pair is at least this much (ms) shorter than the relay's.
pub const PATH_GAIN_MS: f64 = 20.0;
/// The pings a game sends each other human.
pub const PATH_PINGS: u32 = 8;
/// The gap between two pings to one peer (ms); the report goes this long after the last.
pub const PATH_PING_MS: f64 = 250.0;
/// The core waits this long (ms) from the peers' message for the reports; then it chooses with those it has.
pub const PATH_WAIT_MS: f64 = 5000.0;
/// The one-way time (ms) at which a game's local delay reaches [`MAX_LOCAL_DELAY`] (233 ms): a player past it on every
/// way is told before the match.
pub const FAR_ONE_WAY_MS: f64 = (MAX_LOCAL_DELAY + DEADLINE - 2) as f64 * TICK_MS;

/// Whether a match is measured: 2 humans or more, every one on Epic with a round trip to the relay known, one of
/// them over [`NEAR_MS`]. By human index: `relay_rtt` each one's round trip to the relay (ms), `epic` whether it is
/// on Epic.
pub fn should_measure(relay_rtt: &[Option<f64>], epic: &[bool]) -> bool {
    relay_rtt.len() >= 2
        && epic.len() == relay_rtt.len()
        && epic.iter().all(|e| *e)
        && relay_rtt.iter().all(Option::is_some)
        && relay_rtt.iter().flatten().any(|ms| *ms > NEAR_MS)
}

/// What the choice is made from, by human index: each one's round trip to the relay (ms), and `links[i][j]` the
/// smallest round trip human i reported to human j (None: the link did not stand, or no report; a row or a column
/// past the list is None too).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathInput {
    pub relay_rtt: Vec<Option<f64>>,
    pub links: Vec<Vec<Option<f64>>>,
}

/// Who referees a match: the relay, or the app of the human at this index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathChoice {
    Relay,
    Player(usize),
}

/// A choice and what it was made on, by human index.
#[derive(Clone, Debug, PartialEq)]
pub struct PathOutcome {
    pub choice: PathChoice,
    /// The relay's worst pair's one-way time (ms); None with a round trip to the relay not known or under 2 humans.
    pub relay_worst: Option<f64>,
    /// Each human's worst pair's one-way time through its app (ms); None where it is no candidate.
    pub worst: Vec<Option<f64>>,
    /// Each human's round trip to the chosen referee (ms; the referee's own 0). None where it is not known (the
    /// relay chosen with that human's round trip to it unknown).
    // Deviation: the design's `Vec<f64>` holds an Option: with the relay chosen a human's round trip to it may be
    // unknown, and no number stands for that.
    pub rtt_to_referee: Vec<Option<f64>>,
    /// Each human past [`FAR_ONE_WAY_MS`] one way on every way it has: the relay and every candidate's app (a way not
    /// known is not counted; a human with none known is not far).
    pub far: Vec<bool>,
}

/// The round trip reported by `i` for `j`.
fn report(i: &PathInput, a: usize, b: usize) -> Option<f64> {
    i.links.get(a).and_then(|row| row.get(b)).copied().flatten()
}

/// A pair's round trip: the larger of the two reports, None if either is missing.
fn pair_rtt(i: &PathInput, a: usize, b: usize) -> Option<f64> {
    if a == b {
        return Some(0.0);
    }
    match (report(i, a, b), report(i, b, a)) {
        (Some(x), Some(y)) => Some(if y > x { y } else { x }),
        _ => None,
    }
}

/// The largest of a list, None when it is empty.
fn largest(v: impl Iterator<Item = f64>) -> Option<f64> {
    v.reduce(|a, b| if b > a { b } else { a })
}

/// Every pair of human indices (a < b) under `n`.
fn pairs(n: usize) -> impl Iterator<Item = (usize, usize)> {
    (0..n).flat_map(move |a| (a + 1..n).map(move |b| (a, b)))
}

/// The referee of a match ([`PathInput`]): see the module's rule. Any round trip to the relay not known, or under 2
/// humans: the relay. Ties between candidates go to the lowest index.
pub fn choose(i: &PathInput) -> PathOutcome {
    let n = i.relay_rtt.len();
    let relay_known = i.relay_rtt.iter().all(Option::is_some);
    let relay_worst = if relay_known && n >= 2 {
        largest(pairs(n).map(|(a, b)| (i.relay_rtt[a].unwrap_or(0.0) + i.relay_rtt[b].unwrap_or(0.0)) / 2.0))
    } else {
        None
    };
    let worst: Vec<Option<f64>> = (0..n)
        .map(|r| {
            if n < 2 {
                return None;
            }
            let to_r: Option<Vec<f64>> = (0..n).map(|j| pair_rtt(i, j, r)).collect();
            let to_r = to_r?;
            largest(pairs(n).map(|(a, b)| (to_r[a] + to_r[b]) / 2.0))
        })
        .collect();
    let mut choice = PathChoice::Relay;
    if let Some(rw) = relay_worst {
        let mut best: Option<(usize, f64)> = None;
        for (r, w) in worst.iter().enumerate() {
            if let Some(w) = *w
                && best.is_none_or(|(_, b)| w < b)
            {
                best = Some((r, w));
            }
        }
        if let Some((r, w)) = best
            && w <= rw - PATH_GAIN_MS
        {
            choice = PathChoice::Player(r);
        }
    }
    let rtt_to_referee: Vec<Option<f64>> = (0..n)
        .map(|j| match choice {
            PathChoice::Relay => i.relay_rtt[j],
            PathChoice::Player(r) => pair_rtt(i, j, r),
        })
        .collect();
    let far = (0..n)
        .map(|j| {
            let ways: Vec<f64> = std::iter::once(i.relay_rtt[j])
                .chain(worst.iter().enumerate().filter(|(_, w)| w.is_some()).map(|(r, _)| pair_rtt(i, j, r)))
                .flatten()
                .collect();
            !ways.is_empty() && ways.iter().all(|rtt| rtt / 2.0 > FAR_ONE_WAY_MS)
        })
        .collect();
    PathOutcome { choice, relay_worst, worst, rtt_to_referee, far }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(relay: &[Option<f64>], links: &[&[Option<f64>]]) -> PathInput {
        PathInput { relay_rtt: relay.to_vec(), links: links.iter().map(|r| r.to_vec()).collect() }
    }

    #[test]
    fn the_far_limit_is_the_largest_local_delay() {
        assert!((FAR_ONE_WAY_MS - 233.333).abs() < 0.001);
    }

    #[test]
    fn near_players_are_not_measured() {
        assert!(!should_measure(&[Some(40.0), Some(60.0)], &[true, true]));
        assert!(should_measure(&[Some(40.0), Some(61.0)], &[true, true]));
        assert!(!should_measure(&[Some(200.0), Some(61.0)], &[true, false]));
        assert!(!should_measure(&[Some(200.0), None], &[true, true]));
        assert!(!should_measure(&[Some(200.0)], &[true]));
    }

    #[test]
    fn a_short_epic_way_takes_the_player() {
        // (both 200 ms from the relay, 60 ms from each other: the relay's worst pair is 200, player 0's 30)
        let o = choose(&input(&[Some(200.0), Some(200.0)], &[&[None, Some(50.0)], &[Some(60.0), None]]));
        assert_eq!(o.choice, PathChoice::Player(0));
        assert_eq!(o.relay_worst, Some(200.0));
        assert_eq!(o.worst, [Some(30.0), Some(30.0)]);
        assert_eq!(o.rtt_to_referee, [Some(0.0), Some(60.0)]);
        assert_eq!(o.far, [false, false]);
    }

    #[test]
    fn a_failed_link_or_a_small_gain_keeps_the_relay() {
        let o = choose(&input(&[Some(200.0), Some(200.0)], &[&[None, Some(50.0)], &[None, None]]));
        assert_eq!(o.choice, PathChoice::Relay);
        assert_eq!(o.worst, [None, None]);
        assert_eq!(o.rtt_to_referee, [Some(200.0), Some(200.0)]);
        // (gain 19 ms: relay 100, the pair 162)
        let o = choose(&input(&[Some(100.0), Some(100.0)], &[&[None, Some(162.0)], &[Some(162.0), None]]));
        assert_eq!(o.choice, PathChoice::Relay);
        let o = choose(&input(&[Some(100.0), None], &[&[None, Some(10.0)], &[Some(10.0), None]]));
        assert_eq!(o.choice, PathChoice::Relay);
        assert_eq!(o.relay_worst, None);
        assert_eq!(o.rtt_to_referee, [Some(100.0), None]);
    }

    #[test]
    fn three_humans_take_the_middle_one() {
        // (0 and 2 are far apart, 1 between them; the relay far from all)
        let l: [&[Option<f64>]; 3] =
            [&[None, Some(40.0), Some(300.0)], &[Some(40.0), None, Some(40.0)], &[Some(300.0), Some(40.0), None]];
        let o = choose(&input(&[Some(250.0), Some(250.0), Some(250.0)], &l));
        assert_eq!(o.choice, PathChoice::Player(1));
        assert_eq!(o.worst, [Some(170.0), Some(40.0), Some(170.0)]);
        assert_eq!(o.rtt_to_referee, [Some(40.0), Some(0.0), Some(40.0)]);
    }

    #[test]
    fn a_player_far_on_every_way_is_told() {
        let o = choose(&input(&[Some(600.0), Some(40.0)], &[]));
        assert_eq!(o.choice, PathChoice::Relay);
        assert_eq!(o.far, [true, false]);
    }
}
