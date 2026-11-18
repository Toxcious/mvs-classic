//! Catch Up (`world/catch_up.rs`), which no trace match reaches (every one ends before its match's Duration - 120 s):
//! the game mode's flag at Duration - 120 s after the match start, a losing player's Buff_CatchUpSpeedBuff at its
//! 25 s check (speed 1.3, the "Catch Up" text), none for a match of 120 s or less, and the buff's loss on the next
//! hit its holder lands. Without a store, or on a store without the buff's class data (the oracle-format one), it
//! checks nothing.

use mvs_sim::Real;
use mvs_sim::combat::buffs::buff_of;
use mvs_sim::consts::DELTA_SECONDS;
use mvs_sim::events::{Event, PopKind, PopParams};
use mvs_sim::fighter::FighterId;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup, Rules};
use mvs_sim::world::World;
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];
const CLASS: &str = "Buff_CatchUpSpeedBuff_C";
const TEXT_KEY: &str = "FF1C98D945275C70458F999E995F211F";

/// A 1v1 of two idle Shaggys with the match flow and a `duration` s timer, team 1 a knockout ahead.
fn match_of(duration: Real) -> Option<Harness> {
    let player =
        |team| MatchPlayer { character: Some("shaggy".to_owned()), team: Some(team), ..MatchPlayer::default() };
    let setup = MatchSetup {
        players: vec![player(0), player(1)],
        rules: Some(Rules { knockouts_to_win: 99.0, duration, stocks: false }),
        flow: true,
        ..MatchSetup::default()
    };
    let mut h = harness::fresh(&setup, 1)?;
    if !has_class(&h) {
        return None;
    }
    h.w.flow.team_knockouts[1] = 1.0;
    Some(h)
}

/// The buff's class data is in the store (the oracle-format store has none: the tests need the native one).
fn has_class(h: &Harness) -> bool {
    let a = h.fighter(0);
    let ok = mvs_sim::fighter::common::buff_def(&h.w, mvs_sim::actors::ActorRef::Fighter(a), h.w.lit(CLASS)).is_some();
    if !ok {
        eprintln!("note: the store has no {CLASS} class data; nothing checked");
    }
    ok
}

fn has_buff(w: &World, f: FighterId) -> bool {
    buff_of(w, f, w.lit(CLASS)).is_some()
}

/// The "Catch Up" pop texts on fighter `id` among the events since the last call.
fn catch_up_pops(w: &mut World, id: i32) -> usize {
    let key = w.lit(TEXT_KEY);
    std::mem::take(&mut w.events)
        .iter()
        .filter(|e| {
            matches!(e, Event::PopText { fighter, kind: PopKind::NotifyText, params: Some(PopParams::Graph(g)), .. }
                if *fighter == id && g.text_key == key && !g.queued && g.color == [1.0, 1.0, 0.3, 1.0])
        })
        .count()
}

#[test]
fn the_losing_player_gets_the_buff_from_duration_less_120_s() {
    let Some(mut h) = match_of(130.0) else { return };
    let (a, b) = (h.fighter(0), h.fighter(1));
    let ida = h.w.fighter(a).id;
    while !h.w.flow.match_started {
        h.step(&NONE);
    }
    let start = h.w.flow.match_start_tick;
    assert_eq!(h.w.flow.catch_up.due, Some(start + 600), "the timer at Duration - 120 s = 10 s");
    while h.w.flow.tick < start + 599 {
        h.step(&NONE);
    }
    assert!(!h.w.flow.catch_up.on);
    h.step(&NONE);
    assert!(h.w.flow.catch_up.on, "on at the timer's tick");
    assert_eq!(h.w.flow.catch_up.due, None);
    // The first 25 s check, counted from the first spawn (the world's first tick): team 0 is not among the winners.
    catch_up_pops(&mut h.w, ida);
    let mut given = None;
    while h.w.flow.tick < 1600 {
        h.step(&NONE);
        if given.is_none() && has_buff(&h.w, a) {
            given = Some(h.w.flow.tick);
        }
    }
    let at = given.expect("the losing player has the buff");
    assert!((1499..=1501).contains(&at), "given at the 25 s check, tick {at}");
    assert!(!has_buff(&h.w, b), "a winner gets none");
    assert_eq!(h.w.fighter(a).buffs.speed_mod, 1.3, "SpeedMod 1.3");
    assert_eq!(catch_up_pops(&mut h.w, ida), 1, "one \"Catch Up\" text on its owner");
}

#[test]
fn a_match_of_120_s_or_less_has_no_catch_up() {
    let Some(mut h) = match_of(120.0) else { return };
    let a = h.fighter(0);
    while h.w.flow.tick < 1600 {
        h.step(&NONE);
    }
    assert!(h.w.flow.match_started);
    assert_eq!(h.w.flow.catch_up.due, None);
    assert!(!h.w.flow.catch_up.on);
    assert!(!has_buff(&h.w, a));
}

/// The first frame after `from` (within `n` steps) whose step has fighter 0 land a hit, by the trace's inputs.
fn first_hit(name: &str, from: usize, n: usize) -> Option<usize> {
    let mut h = harness::at_baseline(name, from)?;
    let id = h.w.fighter(h.fighter(0)).id;
    for _ in 0..n {
        h.w.events.clear();
        h.step_recorded();
        if h.w
            .events
            .iter()
            .any(|e| matches!(e, Event::Hit { attacker, victim, .. } if *attacker == id && *victim != id))
        {
            return Some(h.frame_index());
        }
    }
    None
}

#[test]
fn a_landed_hit_takes_the_buff_and_restarts_the_check() {
    const NAME: &str = "m5-combat";
    let Some(hit) = first_hit(NAME, 450, 3000) else {
        eprintln!("note: no {NAME} trace, store or hit by fighter 0; NOTHING CHECKED");
        return;
    };
    let mut h = harness::at_baseline(NAME, hit - 3).expect("the same trace");
    if !has_class(&h) {
        return;
    }
    let a = h.fighter(0);
    let id = h.w.fighter(a).id;
    let team = usize::try_from(h.w.fighter(a).team.team).expect("a team");
    // The flag on, the other team ahead, and the delay at its end: the next step's check gives the buff.
    h.w.flow.catch_up.on = true;
    h.w.flow.team_knockouts[1 - team] = h.w.flow.team_knockouts[team] + 1.0;
    h.w.flow.catch_up.delays[0] = DELTA_SECONDS * 0.5;
    h.step_recorded();
    assert!(has_buff(&h.w, a), "given at the check");
    assert_eq!(h.w.fighter(a).buffs.speed_mod, 1.3);
    let mut landed = false;
    for _ in 0..30 {
        h.w.events.clear();
        h.step_recorded();
        landed =
            h.w.events
                .iter()
                .any(|e| matches!(e, Event::Hit { attacker, victim, .. } if *attacker == id && *victim != id));
        if landed {
            break;
        }
        assert!(has_buff(&h.w, a), "kept until a hit lands");
    }
    assert!(landed, "fighter 0 lands its hit with the buff on");
    assert!(!has_buff(&h.w, a), "the landed hit destroys the buff");
    assert_eq!(h.w.fighter(a).buffs.speed_mod, 1.0);
    // RemoveCatchUpSpeedBuff's RetriggerableDelay(25.0), counted by the step's latent pass once.
    assert_eq!(h.w.flow.catch_up.delays[0], 25.0 - DELTA_SECONDS);
}
