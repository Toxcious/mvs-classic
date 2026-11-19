//! Online sessions over the in-memory network: every peer runs its own World and
//! reads its own masher, sends its inputs to the match's referee (`mvs_protocol::referee`, the real one on the
//! simulated clock) and takes every slot's ruled stream from it. The checks are `common::session_test`'s. Without a
//! store nothing is checked.

mod common;

use common::{At, Fault, HARD, Net, check, setup, versus};
use mvs_sim::ai::expert::EXPERT_LEVEL;
use mvs_sim::setup::InputPrefs;

fn net(lat: &[f64], jitter: f64) -> Net {
    Net { lat: lat.to_vec(), jitter, ..Net::default() }
}

fn fault(peer: usize, from: f64, to: f64) -> Fault {
    Fault { peer, from, to }
}

fn at(peer: usize, at: f64) -> At {
    At { peer, at }
}

fn to_a_win() -> serde_json::Value {
    let mut v = versus(2, None);
    v["rules"] = serde_json::json!({ "knockoutsToWin": 2, "duration": 420 });
    v
}

#[test]
fn two_peers_at_20_ms() {
    let n = Net { fine: vec![0, 1], twin: Some(0), ..net(&[1.2, 1.2], 0.5) };
    check("session, 2 peers at 20 + 20 ms", &setup(&versus(2, None)), 3600, &n);
}

/// Peers that start apart and whose clocks run apart: each keeps to the referee's clock (a frame left out, a second
/// tick).
#[test]
fn two_peers_late_start_clocks_apart() {
    let n = Net { start: vec![0.0, 6.0], pace: vec![1.0, 1.003], ..net(&[3.0, 3.0], 1.5) };
    check("session, 2 peers at 50 + 50 ms, late start, clocks apart", &setup(&versus(2, None)), 3600, &n);
}

#[test]
fn two_peers_at_120_ms() {
    let n = Net { fine: vec![0, 1], ..net(&[7.0, 7.0], 2.0) };
    check("session, 2 peers at 120 + 120 ms", &setup(&versus(2, None)), 2400, &n);
}

#[test]
fn four_peers_mixed() {
    let n = Net { start: vec![0.0, 2.0, 1.0, 4.0], twin: Some(3), ..net(&[0.5, 1.5, 3.0, 2.0], 2.0) };
    check("session, 4 peers, mixed, packets overtaking", &setup(&versus(4, None)), 3600, &n);
}

/// The bots' levels the bot checks run at: Hard and the port's own Expert (its look-ahead's scratch worlds and the
/// decision under way are the bot's own, outside the World the session rolls back).
fn two_peers_and_two_bots_at(level: i32) {
    let n = Net { fine: vec![0, 1], ..net(&[2.0, 2.5], 1.0) };
    check(&format!("session, 2 peers and 2 bots at level {level}"), &setup(&versus(4, Some(level))), 3600, &n);
}

#[test]
fn two_peers_and_two_bots() {
    two_peers_and_two_bots_at(HARD);
}

#[test]
fn two_peers_and_two_expert_bots() {
    two_peers_and_two_bots_at(EXPERT_LEVEL);
}

/// A free-for-all of one peer and three Expert bots (every bot's decisions on the workers but the reference's).
#[test]
fn one_peer_and_three_expert_bots() {
    let mut v = versus(4, Some(EXPERT_LEVEL));
    v["teamStyle"] = serde_json::json!(2);
    v["players"][2]["bot"] = serde_json::json!(true);
    let n = Net { twin: Some(0), ..net(&[2.0], 1.0) };
    check("session, 1 peer and 3 Expert bots, free-for-all", &setup(&v), 2400, &n);
}

/// Bots over a lossy way whose packets overtake each other, the twin going back every frame.
fn two_peers_and_two_bots_lossy_at(level: i32) {
    let n = Net { loss: 0.1, twin: Some(1), ..net(&[1.5, 3.0], 3.0) };
    let name = format!("session, 2 peers and 2 bots at level {level}, 10 % of packets lost, overtaking");
    check(&name, &setup(&versus(4, Some(level))), 2400, &n);
}

#[test]
fn two_peers_and_two_bots_lossy() {
    two_peers_and_two_bots_lossy_at(HARD);
}

#[test]
fn two_peers_and_two_expert_bots_lossy() {
    two_peers_and_two_bots_lossy_at(EXPERT_LEVEL);
}

#[test]
fn two_peers_to_a_win() {
    let n = Net { fine: vec![0, 1], twin: Some(1), ..net(&[2.0, 2.0], 1.0) };
    check("session, 2 peers to a win", &setup(&to_a_win()), 3600, &n);
}

/// The referee in the host's own game: the host's way is nothing, a guest's the whole way to the host.
#[test]
fn refereed_by_one_peer() {
    let n = Net { fine: vec![0, 1], ..net(&[0.0, 6.0], 1.0) };
    check("session, refereed by one of 2 peers, the other 100 ms away", &setup(&versus(2, None)), 2400, &n);
}

/// Packets lost on the way, both ways: the acknowledgements bring every stream whole.
#[test]
fn two_peers_ten_percent_lost() {
    let n = Net { loss: 0.1, fine: vec![0, 1], ..net(&[2.0, 2.0], 1.0) };
    check("session, 2 peers, 10 % of packets lost, overtaking", &setup(&versus(2, None)), 2400, &n);
}

#[test]
fn four_peers_twenty_percent_lost() {
    let n = Net { loss: 0.2, ..net(&[1.0, 2.0, 1.5, 2.5], 1.0) };
    check("session, 4 peers, 20 % of packets lost", &setup(&versus(4, None)), 2400, &n);
}

/// One game stands still mid-match (a hitch of 500 ms, of 3 s): its inputs of that time are substituted, it jumps to
/// the referee's clock when it is back, and nobody else waits or leaves a frame out.
#[test]
fn two_peers_one_stands_still() {
    let n = Net {
        hold: vec![fault(1, 600.0, 630.0), fault(1, 1200.0, 1380.0)],
        fine: vec![0],
        twin: Some(1),
        ..net(&[1.2, 1.5], 0.5)
    };
    check("session, 2 peers, one game stands still for 30 and for 180 ticks", &setup(&versus(2, None)), 2400, &n);
}

#[test]
fn four_peers_one_stands_still() {
    let n = Net {
        loss: 0.05,
        hold: vec![fault(2, 600.0, 630.0), fault(2, 1200.0, 1380.0)],
        fine: vec![0, 1, 3],
        ..net(&[1.0, 2.0, 1.5, 2.5], 1.0)
    };
    check("session, 4 peers, one game stands still for 30 and for 180 ticks", &setup(&versus(4, None)), 2400, &n);
}

fn two_peers_two_bots_one_stands_still_at(level: i32) {
    let n = Net { hold: vec![fault(0, 600.0, 780.0)], fine: vec![1], ..net(&[1.5, 2.0], 0.5) };
    let name = format!("session, 2 peers and 2 bots at level {level}, one game stands still for 180 ticks");
    check(&name, &setup(&versus(4, Some(level))), 2400, &n);
}

#[test]
fn two_peers_two_bots_one_stands_still() {
    two_peers_two_bots_one_stands_still_at(HARD);
}

#[test]
fn two_peers_two_expert_bots_one_stands_still() {
    two_peers_two_bots_one_stands_still_at(EXPERT_LEVEL);
}

/// Its way stands still instead (the game runs on, on guesses, and waits at the prediction limit): the same for the
/// others.
#[test]
fn four_peers_one_way_stands_still() {
    let n = Net {
        cut: vec![fault(2, 300.0, 340.0), fault(2, 900.0, 1140.0)],
        fine: vec![0, 1, 3],
        twin: Some(2),
        ..net(&[1.0, 2.0, 1.5, 2.5], 1.0)
    };
    check("session, 4 peers, one way stands still for 40 and for 240 ticks", &setup(&versus(4, None)), 2400, &n);
}

/// A peer far from the referee sends its inputs for later ticks (the local delay); the others' delays stay theirs.
#[test]
fn one_peer_200_ms_away() {
    let n = Net { fine: vec![0], twin: Some(1), ..net(&[1.0, 12.0], 0.5) };
    check("session, 2 peers, one 200 ms from the referee", &setup(&versus(2, None)), 2400, &n);
}

/// Past the delay's limit its inputs come late: its fighter stands, the match goes on for the other.
#[test]
fn one_peer_past_the_limit() {
    let n = Net { fine: vec![0], ..net(&[1.0, 18.0], 0.5) };
    check("session, 2 peers, one 300 ms from the referee (past the limit)", &setup(&versus(2, None)), 1800, &n);
}

/// Its way goes up and down again (40 -> 300 -> 40 ms): the delay goes up with the inputs that come back substituted
/// and down a tick every 5 s without one.
#[test]
fn one_way_steps_up_and_down() {
    let n = Net { steps: vec![(1, 900.0, 9.0), (1, 2400.0, 1.2)], fine: vec![0], ..net(&[1.2, 1.2], 0.5) };
    check("session, 2 peers, one whose way steps 40 -> 300 -> 40 ms", &setup(&versus(2, None)), 4200, &n);
}

/// A peer leaves mid-match: the referee ends its stream at the ruled line, empty inputs from there on for all.
#[test]
fn three_peers_one_leaves() {
    let n = Net { leave: vec![at(1, 900.0)], fine: vec![0, 2], twin: Some(0), ..net(&[1.0, 2.0, 3.0], 2.0) };
    check("session, 3 peers, one leaves at tick 900", &setup(&versus(3, None)), 2400, &n);
}

#[test]
fn four_peers_two_leave() {
    let n = Net { loss: 0.2, leave: vec![at(3, 900.0), at(0, 1500.0)], ..net(&[1.0, 2.0, 3.0, 1.5], 2.0) };
    check("session, 4 peers, packets lost, one leaves at tick 900 and one at 1500", &setup(&versus(4, None)), 2400, &n);
}

#[test]
fn three_peers_and_a_bot_one_leaves() {
    let mut v = versus(4, None);
    v["players"] =
        serde_json::json!([{ "bot": false }, { "bot": false }, { "bot": true, "difficulty": 3 }, { "bot": false }]);
    let n = Net { leave: vec![at(1, 900.0)], fine: vec![0, 2], ..net(&[1.5, 2.0, 1.0], 0.5) };
    check("session, 3 peers and a bot, one leaves at tick 900", &setup(&v), 2400, &n);
}

/// A peer whose packets stop: the referee tells it away once after SILENT_MS (its ticks substituted, the others go on
/// without a wait), then gone once RESUME_MS after it was last heard: its owner takes it out and the match ends on
/// every peer.
#[test]
fn three_peers_one_mute() {
    let n = Net { mute: vec![at(1, 600.0)], fine: vec![0, 2], ..net(&[1.5, 1.5, 2.0], 0.5) };
    check("session, 3 peers, one whose packets stop at tick 600", &setup(&versus(3, None)), 2400, &n);
}

/// A peer that sends on but takes nothing (its way back stands still): its acknowledgements stand still, the referee
/// tells it away once BEHIND_TICKS after (keeping the streams for it), then gone GONE_TICKS after: the match ends.
#[test]
fn three_peers_one_deaf() {
    let n = Net { deaf: vec![at(1, 600.0)], fine: vec![0, 2], ..net(&[1.5, 1.5, 2.0], 0.5) };
    check("session, 3 peers, one that takes nothing from tick 600", &setup(&versus(3, None)), 2400, &n);
}

/// A peer whose way is lost (every packet, both ways) for 3, 10 and 19 s comes back: away and back past SILENT_MS,
/// its ticks of that time substituted, every stream kept for it and sent several runs a pump, level again within a
/// second; the others never wait; every peer's hashes agree.
#[test]
fn three_peers_one_way_lost_3_s() {
    let n = Net { drop: vec![fault(1, 600.0, 780.0)], fine: vec![0, 2], ..net(&[1.5, 1.5, 2.0], 0.5) };
    check("session, 3 peers, one whose way is lost for 3 s", &setup(&versus(3, None)), 2400, &n);
}

#[test]
fn three_peers_one_way_lost_10_s() {
    let n = Net { drop: vec![fault(1, 600.0, 1200.0)], fine: vec![0, 2], ..net(&[1.5, 1.5, 2.0], 0.5) };
    check("session, 3 peers, one whose way is lost for 10 s", &setup(&versus(3, None)), 2400, &n);
}

#[test]
fn three_peers_one_way_lost_19_s() {
    let n = Net { drop: vec![fault(1, 600.0, 1740.0)], fine: vec![0, 2], ..net(&[1.5, 1.5, 2.0], 0.5) };
    check("session, 3 peers, one whose way is lost for 19 s", &setup(&versus(3, None)), 2400, &n);
}

/// Lost for 21 s: gone RESUME_MS after it was last heard; its owner takes it out and the match ends on every peer.
#[test]
fn three_peers_one_way_lost_21_s() {
    let n = Net { drop: vec![fault(1, 600.0, 1860.0)], fine: vec![0, 2], ..net(&[1.5, 1.5, 2.0], 0.5) };
    check("session, 3 peers, one whose way is lost for 21 s", &setup(&versus(3, None)), 2400, &n);
}

/// The start gate (START_AHEAD_MS, 15 ticks): the peers' first frames at the referee's tick 0, whatever their way. A
/// peer past it (a ping over twice the gate) starts on the go and catches up.
#[test]
fn start_gate_two_peers() {
    let n = Net { go: 15.0, fine: vec![0, 1], ..net(&[1.2, 6.0], 0.5) };
    check("session, start gate, 2 peers 20 and 100 ms from the referee", &setup(&versus(2, None)), 1200, &n);
}

#[test]
fn start_gate_four_peers_one_past_it() {
    let n = Net { go: 15.0, fine: vec![0, 1, 2], twin: Some(3), ..net(&[1.0, 3.0, 6.0, 16.0], 1.0) };
    check("session, start gate, 4 peers, one past it", &setup(&versus(4, None)), 1200, &n);
}

/// Over the intro (its events from tick 150): a game that stands still from 120 to 180, a way from 140 to 170.
#[test]
fn start_gate_one_stands_still_over_the_intro() {
    let n = Net { go: 15.0, hold: vec![fault(1, 120.0, 180.0)], fine: vec![0], ..net(&[1.2, 1.5], 0.5) };
    check(
        "session, start gate, 2 peers, one game stands still from tick 120 to 180",
        &setup(&versus(2, None)),
        1200,
        &n,
    );
}

#[test]
fn start_gate_one_way_stands_still_over_the_intro() {
    let n = Net { go: 15.0, cut: vec![fault(1, 140.0, 170.0)], fine: vec![0], ..net(&[1.2, 1.5], 0.5) };
    check(
        "session, start gate, 2 peers, one way stands still from tick 140 to 170",
        &setup(&versus(2, None)),
        1200,
        &n,
    );
}

/// Peers whose players chose different input preferences: each game runs every fighter on that fighter's own values
/// from the set-up, so the peers' checksums agree.
#[test]
fn two_peers_with_different_input_preferences() {
    let mut s = setup(&versus(2, None));
    s.players[0].character = Some("irongiant".to_owned());
    s.players[0].input = InputPrefs {
        fastfall: 1,
        double_crouch_jump_down: false,
        attack_picks_up_items: false,
        ..InputPrefs::default()
    };
    s.players[1].character = Some("superman".to_owned());
    s.players[1].input = InputPrefs {
        swapped_ground: true,
        swapped_air: true,
        ignore_right_stick_diagonals: true,
        ..InputPrefs::default()
    };
    let n = Net { fine: vec![0, 1], twin: Some(0), ..net(&[1.5, 2.5], 1.0) };
    check("session, 2 peers with different input preferences", &s, 2400, &n);
}
