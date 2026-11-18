//! JakeAimer's stick aim with a right stick Y (`chars/jake/aim.rs`, StickAttackY = `InputState.rsy`): no trace record
//! carries `rsy`, so no match runs this path. A fresh World of Jake against Shaggy, the aim punch's montage played and
//! the aimer's arrow read after the stick was held. Without a store the test checks nothing and says so.

use mvs_anim::montage::PROXY_NONE;
use mvs_sim::actors::WorldActorState;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::anim::montages::start_montage;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};

fn player(c: &str, bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, ..MatchPlayer::default() }
}

fn pair() -> Option<(Harness, FighterId)> {
    let setup = MatchSetup { players: vec![player("jake", false), player("shaggy", true)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    let f = h.fighter(0);
    Some((h, f))
}

/// The arrow (UE Y, Z) of the live aimer.
fn arrow(h: &Harness) -> Option<(f32, f32)> {
    h.w.actors.actors.iter().find_map(|(_, a)| match &a.state {
        WorldActorState::JakeAimer(s) if !a.dead => Some((s.ay, s.az)),
        _ => None,
    })
}

/// The arrow after `ticks` ticks of the aim punch with the right stick `(rsx, rsy)` held.
fn aimed(rsx: Option<f32>, rsy: Option<f32>, ticks: usize) -> Option<(f32, f32)> {
    let (mut h, f) = pair()?;
    let m = h.w.lit("Jake_Air_Aim_Punch_F_Montage");
    start_montage(&mut h.w, f, m, &PROXY_NONE, None);
    let held = InputState { rsx, rsy, ..NO_INPUT };
    let mut last = None;
    for _ in 0..ticks {
        h.step(&[held, NO_INPUT]);
        last = arrow(&h).or(last);
    }
    Some(last.expect("the montage's spawner notify made the aimer"))
}

/// StickAttackY alone (rsx 0, rsy up or down) turns the arrow, and the two directions turn it apart; no stick and no
/// move axes hold it where it began.
#[test]
fn the_aimer_reads_the_right_sticks_y() {
    let Some(up) = aimed(Some(0.0), Some(1.0), 20) else { return };
    let down = aimed(Some(0.0), Some(-1.0), 20).expect("the store is there");
    let none = aimed(None, None, 20).expect("the store is there");
    assert_ne!(up, none, "the stick's Y moved the arrow");
    assert_ne!(down, none, "the stick's Y moved the arrow");
    assert_ne!(up, down, "up and down turn it apart: up {up:?}, down {down:?}");
}

/// A montage with tracks in only part of a cutting slot set (Jake_Air_Aim_Punch_F_Montage has no "2" slots):
/// the layer is held under it at full weight, so the locomotion's clock stops; a trace match moves for Finn, Tom and
/// Jerry, Velma, Morty and Black Adam but none runs Jake's.
#[test]
fn a_partial_slot_set_holds_the_layer() {
    let Some((mut h, f)) = pair() else { return };
    let m = h.w.lit("Jake_Air_Aim_Punch_F_Montage");
    assert!(!mvs_sim::fighter::anim::update::source_held(&h.w, f), "no montage, no hold");
    start_montage(&mut h.w, f, m, &PROXY_NONE, None);
    let mut held = false;
    for _ in 0..20 {
        h.step(&[NO_INPUT, NO_INPUT]);
        held |= mvs_sim::fighter::anim::update::source_held(&h.w, f);
    }
    assert!(held, "the aim punch holds the layer once its weight is full");
}
