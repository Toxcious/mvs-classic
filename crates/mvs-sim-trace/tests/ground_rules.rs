//! The port's own Ice Rink, Wind and Black Hole (`mvs_sim::world::own_rules`), each on all match
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
fn wind_gusts_blow_a_standing_fighter_the_gusts_way() {
    use mvs_sim::world::own_rules::gust_at;
    let Some(mut h) = landed("Chaos_Wind") else { return };
    let f = h.fighter(1);
    // (to the next gust's start)
    let mut n = 0;
    while gust_at(&h.w, 0.0).is_none_or(|g| g.blow <= 0.0) {
        idle(&mut h, 1);
        n += 1;
        assert!(n < 600, "no gust in 10 s");
    }
    let dir = gust_at(&h.w, 0.0).expect("a gust").dir;
    let x0 = x(&h, f);
    idle(&mut h, 60);
    let moved = x(&h, f) - x0;
    assert!(moved * dir > 100.0, "blown {moved}, the gust's way {dir}");
    // (past its end: calm)
    idle(&mut h, 80);
    assert!(gust_at(&h.w, 0.0).is_none_or(|g| g.blow <= 0.0), "a gust lasts its length");
}

#[test]
fn black_hole_pulls_fighters_in_and_throws_them_out() {
    use mvs_sim::world::own_rules::{HOLE_OPEN, hole_at};
    let Some(mut h) = landed("Chaos_Magnet") else { return };
    let hole = hole_at(&h.w, 0.0).expect("the hole open");
    let fs = [h.fighter(0), h.fighter(1)];
    let d0: Vec<Real> = fs.iter().map(|&f| (x(&h, f) - hole.x).abs()).collect();
    idle(&mut h, 30);
    for (n, &f) in fs.iter().enumerate() {
        let d = (x(&h, f) - hole.x).abs();
        assert!(d < d0[n] - 3.0 || d < 1.0, "fighter {n}: {} -> {d}", d0[n]);
    }
    // A fighter put in its core is hit and thrown out.
    idle(&mut h, usize::try_from(HOLE_OPEN).expect("ticks"));
    let f = fs[1];
    let dmg = h.w.fighter(f).stats.damage;
    {
        let b = &mut h.w.fighter_mut(f).body;
        b.x = hole.x + 20.0;
        b.y = hole.y;
    }
    idle(&mut h, 20);
    assert!(h.w.fighter(f).stats.damage > dmg, "the core hurts");
    assert!(h.w.fighter(f).body.x > hole.x + 60.0, "thrown out its side: {}", h.w.fighter(f).body.x);
}
