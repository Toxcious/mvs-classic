//! Matches of more than four slots online over the in-memory network: six and eight peers, each with its own session
//! and World, the referee ruling every stream (`common::session_test`'s checks), and a big match's prediction limit
//! (`BIG_MATCH_PREDICTION`): a peer whose way stands still waits at it, then is level again, and no rollback runs more
//! than 16 ticks. Every slot is a human with a fighter of its own on the trace stage (a Versus of 3v3 and 4v4). Without
//! a store nothing is checked.

mod common;

use common::{Fault, Net, check, setup, versus};
use mvs_net::session::{
    BIG_MATCH_KEEP_BEHIND, BIG_MATCH_PREDICTION, KEEP_BEHIND, MAX_PREDICTION, SessionConfig, SlotKind,
};

fn net(lat: &[f64], jitter: f64) -> Net {
    Net { lat: lat.to_vec(), jitter, ..Net::default() }
}

/// The prediction limit and the snapshot's lag by the match's slots: the deepest rollback (the two together) is 24
/// ticks up to four slots and 16 past them. `session_test` checks every peer's deepest against its own two.
#[test]
fn the_deepest_rollback_is_16_ticks_past_four_slots() {
    let of = |n: usize| {
        let c = SessionConfig::new(vec![SlotKind::Remote; n]);
        (c.max_prediction, c.keep_behind)
    };
    assert_eq!((of(2), of(4)), ((MAX_PREDICTION, KEEP_BEHIND), (MAX_PREDICTION, KEEP_BEHIND)));
    for n in [5, 6, 8] {
        assert_eq!(of(n), (BIG_MATCH_PREDICTION, BIG_MATCH_KEEP_BEHIND));
    }
    assert_eq!(BIG_MATCH_PREDICTION + BIG_MATCH_KEEP_BEHIND, 16);
}

#[test]
fn six_peers_mixed() {
    let n = Net { start: vec![0.0, 2.0, 1.0, 4.0, 0.0, 3.0], ..net(&[0.5, 1.5, 3.0, 2.0, 1.0, 2.5], 2.0) };
    check("session, 6 peers, mixed, packets overtaking", &setup(&versus(6, None)), 1800, &n);
}

#[test]
fn eight_peers_mixed() {
    let n = Net {
        start: vec![0.0, 2.0, 1.0, 4.0, 0.0, 3.0, 1.0, 2.0],
        ..net(&[0.5, 1.5, 3.0, 2.0, 1.0, 2.5, 1.2, 0.8], 2.0)
    };
    check("session, 8 peers, mixed, packets overtaking", &setup(&versus(8, None)), 1800, &n);
}

/// A peer whose way stands still runs on on guesses up to BIG_MATCH_PREDICTION ticks and waits there; once its way
/// goes again it is level within a second. Peers 2 and 5 are held. The peers near the referee
/// (one tick each way) go on undisturbed; one further away waits now and then while the still slot's ticks are ruled
/// at the deadline: its confirmed line trails the clock by DEADLINE and its own way down, close to the limit of 10.
#[test]
fn eight_peers_one_held_past_the_limit_waits_then_catches_up() {
    let n = Net {
        cut: vec![
            Fault { peer: 2, from: 300.0, to: 330.0 },
            Fault { peer: 5, from: 500.0, to: 540.0 },
            Fault { peer: 2, from: 700.0, to: 820.0 },
        ],
        fine: vec![0, 3, 7],
        ..net(&[1.0, 1.5, 2.0, 1.0, 1.5, 1.2, 2.0, 1.0], 1.0)
    };
    check("session, 8 peers, one way stands still for 30, 40 and 120 ticks", &setup(&versus(8, None)), 1200, &n);
}

/// A measure, not a check: the referee's bytes a second out, each peer's in and out, and how far a frame's shown line
/// runs past its confirmed one, at four and eight slots, one tick and six ticks one way, the referee pumped after every
/// packet it takes (as the relay's core and an app's referee do); and at eight slots, six ticks, with 5% of the packets
/// lost both ways. `cargo test -p mvs-net --test big_match -- --ignored --nocapture` prints them on each summary's
/// notes.
#[test]
#[ignore = "a measure: run it by name"]
fn bytes_a_second_by_slots_and_way() {
    for (slots, lat, loss, pace) in [
        (4, 1.0, 0.0, 1.0),
        (4, 6.0, 0.0, 1.0),
        (8, 1.0, 0.0, 1.0),
        (8, 6.0, 0.0, 1.0),
        (8, 6.0, 0.0, 2.0),
        (8, 6.0, 0.05, 1.0),
    ] {
        let n = Net { pump_each: true, loss, pace: vec![pace; slots], ..net(&vec![lat; slots], 0.0) };
        let name = format!("bytes, {slots} slots, {lat} ticks one way, loss {loss}, {pace} ticks a frame");
        check(&name, &setup(&versus(slots, None)), 900, &n);
    }
}
