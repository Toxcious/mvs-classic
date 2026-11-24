//! The port's own Turbo, Slow Motion, Ice Rink, Wind and Magnet (`mvs_sim::world::own_rules`), each on all match
//! from the set-up's mutators (the Mutators row) as the wheel's window turns it on for its window.

use mvs_sim::fighter::FighterId;
use mvs_sim::setup::{MatchPlayer, MatchSetup, Rules};
use mvs_sim::{InputState, Real};
use mvs_sim_trace::harness::{self, Harness};

fn player(ch: &str) -> MatchPlayer {
    MatchPlayer { bot: false, character: Some(ch.to_owned()), ..MatchPlayer::default() }
}

/// A 1v1 with `rule` on all match (none for ""), its fighters landed.
fn landed(rule: &str) -> Option<Harness> {
    let s = MatchSetup {
        players: vec![player("superman"), player("shaggy")],
        rules: Some(Rules { knockouts_to_win: 99.0, duration: 420.0, stocks: false }),
        mutators: if rule.is_empty() { Vec::new() } else { vec![rule.to_owned()] },
        ..MatchSetup::default()
    };
    let mut h = harness::fresh(&s, 11)?;
    idle(&mut h, 120);
    Some(h)
}

fn idle(h: &mut Harness, n: usize) {
    for _ in 0..n {
        h.step(&[InputState::default(); 2]);
    }
}

fn x(h: &Harness, f: FighterId) -> Real {
    h.w.fighter(f).body.x
}

#[test]
fn turbo_and_slow_motion_run_more_and_fewer_ticks_a_frame() {
    for (rule, ticks) in [("", 100), ("Chaos_Turbo", 130), ("Chaos_SlowMotion", 70)] {
        let Some(mut h) = landed(rule) else { return };
        h.w.pace.owed = 0.0;
        let t0 = h.w.flow.tick;
        idle(&mut h, 100);
        let ran = h.w.flow.tick - t0;
        assert!((ran - ticks).abs() <= 1, "{rule}: {ran} ticks in 100 frames");
    }
}

#[test]
fn slow_motion_keeps_a_press_of_a_frame_it_does_not_run() {
    let Some(mut h) = landed("Chaos_SlowMotion") else { return };
    let f = h.fighter(0);
    assert!(h.w.fighter(f).body.on_floor);
    h.w.pace.owed = 0.0; // the next frame owes 0.7: not run
    let t0 = h.w.flow.tick;
    let mut i = [InputState::default(); 2];
    i[0].jump = true;
    h.step(&i);
    assert_eq!(h.w.flow.tick, t0, "the frame is not run");
    idle(&mut h, 20);
    assert!(!h.w.fighter(f).body.on_floor, "the held press jumped");
}

fn slide(rule: &str) -> Option<Real> {
    let mut h = landed(rule)?;
    let f = h.fighter(0);
    let mut i = [InputState::default(); 2];
    i[0].x = 1.0;
    for _ in 0..40 {
        h.step(&i);
    }
    let x0 = x(&h, f);
    idle(&mut h, 40);
    Some(x(&h, f) - x0)
}

#[test]
fn ice_rink_slides_further_after_a_walk() {
    let (Some(dry), Some(ice)) = (slide(""), slide("Chaos_IceRink")) else { return };
    assert!(ice > dry * 3.0 && ice > 100.0, "slid {ice} on ice, {dry} without");
}

#[test]
fn wind_blows_a_standing_fighter_left_then_right() {
    let Some(mut h) = landed("Chaos_Wind") else { return };
    let f = h.fighter(1);
    let t = h.w.flow.tick;
    let into = t.rem_euclid(mvs_sim::world::own_rules::WIND_TICKS);
    let left = (t / mvs_sim::world::own_rules::WIND_TICKS).rem_euclid(2) == 0;
    let x0 = x(&h, f);
    let n = 30.min(usize::try_from(mvs_sim::world::own_rules::WIND_TICKS - into - 1).expect("ticks"));
    idle(&mut h, n);
    let moved = x(&h, f) - x0;
    assert!(moved.abs() > 3.0, "blown: {moved}");
    assert_eq!(moved < 0.0, left, "blown the beat's way: {moved}");
}

#[test]
fn magnet_draws_fighters_to_the_middle() {
    let Some(mut h) = landed("Chaos_Magnet") else { return };
    let mid = (h.w.stage.blast.left + h.w.stage.blast.right) / 2.0;
    let fs = [h.fighter(0), h.fighter(1)];
    let d0: Vec<Real> = fs.iter().map(|&f| (x(&h, f) - mid).abs()).collect();
    idle(&mut h, 30);
    for (n, &f) in fs.iter().enumerate() {
        let d = (x(&h, f) - mid).abs();
        assert!(d < d0[n] - 3.0 || d < 1.0, "fighter {n}: {} -> {d}", d0[n]);
    }
}
