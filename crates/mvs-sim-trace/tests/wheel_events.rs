//! The Chaos Wheel's events (`mvs_sim::world::wheel_events`), each turned on by the wheel at a window's start and
//! off at its end: Damage Swap, Shuffle, Fresh Start, Underdog, Hot Potato and Body Swap.

use mvs_sim::fighter::FighterId;
use mvs_sim::setup::{MatchPlayer, MatchSetup, Rules, TeamStyle, Wheel};
use mvs_sim::world::wheel::change_to;
use mvs_sim::world::wheel_events::{self, controlled};
use mvs_sim::{Event, InputState, Real, World};
use mvs_sim_trace::harness::{self, Harness};

const POOL: [&str; 6] =
    ["Chaos_DamageSwap", "Chaos_Shuffle", "Chaos_FreshStart", "Chaos_Underdog", "Chaos_HotPotato", "Chaos_BodySwap"];

fn at(key: &str) -> usize {
    POOL.iter().position(|&k| k == key).expect("in the pool")
}

fn player(ch: &str) -> MatchPlayer {
    MatchPlayer { bot: false, character: Some(ch.to_owned()), ..MatchPlayer::default() }
}

/// A match of `style` with the pool, its clock too long to change by itself, its fighters landed.
fn landed(style: TeamStyle) -> Option<Harness> {
    let players = match style {
        TeamStyle::OneVsOne => vec![player("superman"), player("shaggy")],
        _ => vec![player("superman"), player("shaggy"), player("batman"), player("wonderwoman")],
    };
    let s = MatchSetup {
        team_style: style,
        players,
        rules: Some(Rules { knockouts_to_win: 99.0, duration: 420.0, stocks: false }),
        wheel: Some(Wheel { seconds: 10_000.0, warn: 1.0, ..Wheel::of(POOL.iter().map(|&s| s.to_owned()).collect()) }),
        ..MatchSetup::default()
    };
    let mut h = harness::fresh(&s, 11)?;
    idle(&mut h, 120);
    Some(h)
}

fn idle(h: &mut Harness, n: usize) {
    for _ in 0..n {
        h.step_recorded();
    }
}

fn damage(w: &World, f: FighterId) -> Real {
    w.fighter(f).stats.damage
}

#[test]
fn damage_swap_trades_the_most_and_the_least() {
    let Some(mut h) = landed(TeamStyle::FreeForAll) else { return };
    let fs: Vec<FighterId> = (0..4).map(|n| h.fighter(n)).collect();
    for (n, d) in [30.0, 120.0, 5.0, 60.0].into_iter().enumerate() {
        h.w.fighter_mut(fs[n]).stats.damage = d;
    }
    change_to(&mut h.w, Some(at("Chaos_DamageSwap")));
    let got: Vec<Real> = fs.iter().map(|&f| damage(&h.w, f)).collect();
    assert_eq!(got, [30.0, 5.0, 120.0, 60.0]);
}

#[test]
fn fresh_start_halves_every_damage() {
    let Some(mut h) = landed(TeamStyle::OneVsOne) else { return };
    let (a, b) = (h.fighter(0), h.fighter(1));
    h.w.fighter_mut(a).stats.damage = 80.0;
    h.w.fighter_mut(b).stats.damage = 15.0;
    change_to(&mut h.w, Some(at("Chaos_FreshStart")));
    assert_eq!((damage(&h.w, a), damage(&h.w, b)), (40.0, 7.5));
}

#[test]
fn shuffle_moves_every_fighter_to_another_place() {
    let Some(mut h) = landed(TeamStyle::FreeForAll) else { return };
    let fs: Vec<FighterId> = (0..4).map(|n| h.fighter(n)).collect();
    let before: Vec<Real> = fs.iter().map(|&f| h.w.fighter(f).body.x).collect();
    change_to(&mut h.w, Some(at("Chaos_Shuffle")));
    let after: Vec<Real> = fs.iter().map(|&f| h.w.fighter(f).body.x).collect();
    for (n, x) in after.iter().enumerate() {
        assert_ne!(*x, before[n], "fighter {n} stayed");
        assert!(before.contains(x), "fighter {n} at {x}, not a place of the others: {before:?}");
    }
    idle(&mut h, 60);
    assert!(fs.iter().all(|&f| h.w.fighter(f).life.dead <= 0.0), "all alive after the move");
}

#[test]
fn underdog_is_giant_for_the_window() {
    let Some(mut h) = landed(TeamStyle::OneVsOne) else { return };
    let (a, b) = (h.fighter(0), h.fighter(1));
    h.w.fighter_mut(a).stats.ring_outs = 2;
    let size = h.w.fighter(b).body.size_scale;
    change_to(&mut h.w, Some(at("Chaos_Underdog")));
    idle(&mut h, 5);
    assert!(h.w.fighter(b).body.size_scale > size, "the one with no ring-out grows");
    assert_eq!(h.w.fighter(a).body.size_scale, size, "the leader does not");
    change_to(&mut h.w, None);
    idle(&mut h, 5);
    assert_eq!(h.w.fighter(b).body.size_scale, size);
}

#[test]
fn hot_potato_passes_on_damage_and_blasts_its_holder_at_the_end() {
    let Some(mut h) = landed(TeamStyle::OneVsOne) else { return };
    let fs = [h.fighter(0), h.fighter(1)];
    h.w.events.clear();
    change_to(&mut h.w, Some(at("Chaos_HotPotato")));
    let holder = |w: &World| w.wheel.as_ref().map_or(-1, |s| s.potato);
    let first = holder(&h.w);
    assert!(first == 0 || first == 1, "a holder: {first}");
    let (from, to) = (fs[usize::try_from(first).expect("a slot")], fs[usize::try_from(1 - first).expect("a slot")]);
    mvs_sim::combat::hit::apply_direct_damage(&mut h.w, to, 5.0, Some(from));
    assert_eq!(holder(&h.w), 1 - first, "passed on by the damage");
    let pops =
        h.w.fighters
            .iter()
            .flat_map(|(_, f)| f.events.iter())
            .filter(|e| matches!(e, Event::PopText { kind: mvs_sim::events::PopKind::HotPotato, .. }));
    assert!(pops.count() >= 1, "a pop text over the holder");
    let before = damage(&h.w, to);
    change_to(&mut h.w, None);
    assert_eq!(holder(&h.w), -1);
    assert!(damage(&h.w, to) > before, "the blast's damage on the holder");
    let mut top = h.w.fighter(to).body.y;
    let y0 = top;
    for _ in 0..60 {
        idle(&mut h, 1);
        top = top.max(h.w.fighter(to).body.y);
    }
    assert!(top > y0 + 300.0, "launched up: {y0} -> {top}");
}

#[test]
fn body_swap_trades_teammates_inputs_for_the_window() {
    let Some(mut h) = landed(TeamStyle::TwoVsTwo) else { return };
    let fs: Vec<FighterId> = (0..4).map(|n| h.fighter(n)).collect();
    let mate = |w: &World, s: usize| {
        let t = w.fighter(fs[s]).team.team;
        (0..4).find(|&o| o != s && w.fighter(fs[o]).team.team == t).expect("a teammate")
    };
    let m0 = mate(&h.w, 0);
    change_to(&mut h.w, Some(at("Chaos_BodySwap")));
    assert_eq!(controlled(&h.w, 0), fs[m0]);
    assert_eq!(wheel_events::input_slot(&h.w, m0), 0);
    // slot 0 holds right: its teammate moves, slot 0's own fighter does not
    let (x0, xm) = (h.w.fighter(fs[0]).body.x, h.w.fighter(fs[m0]).body.x);
    let mut i = vec![InputState::default(); 4];
    i[0].x = 1.0;
    for _ in 0..30 {
        h.step(&i);
    }
    assert!(h.w.fighter(fs[m0]).body.x > xm + 50.0, "the teammate walks right");
    assert!((h.w.fighter(fs[0]).body.x - x0).abs() < 1.0, "the own fighter stands");
    change_to(&mut h.w, None);
    assert_eq!(controlled(&h.w, 0), fs[0]);
}

#[test]
fn body_swap_is_drawn_only_in_a_2v2() {
    let Some(h) = landed(TeamStyle::OneVsOne) else { return };
    assert!(!wheel_events::runnable(&h.w, wheel_events::WheelEvent::BodySwap));
    let Some(h) = landed(TeamStyle::TwoVsTwo) else { return };
    assert!(wheel_events::runnable(&h.w, wheel_events::WheelEvent::BodySwap));
}
