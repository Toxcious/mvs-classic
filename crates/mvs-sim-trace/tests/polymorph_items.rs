//! A Polymorph whose mesh the items part names (the drumstick's Buff_Chicken, a match with items and no Taz): the
//! montages and their lengths come from the items data (`combat::polymorph::ITEMS_OWNER`), so the polymorphed
//! fighter's evade sets its reset timer and he can evade again.

use mvs_sim::combat::buffs::{buff_signal, spawn_buff};
use mvs_sim::combat::polymorph::ITEMS_OWNER;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::registry::signals::BuffSignal;
use mvs_sim::registry::signals::BuffSignalArg;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness;

fn player(c: &str, bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, ..MatchPlayer::default() }
}

#[test]
fn a_chicken_of_the_items_part_evades_twice() {
    let setup = MatchSetup {
        players: vec![player("garnet", false), player("shaggy", true)],
        mutators: vec!["Mutator_RandomItemSpawns".to_owned()],
        ..MatchSetup::default()
    };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    for _ in 0..10 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    let f = h.fighter(0);
    let cls = h.w.lit("Buff_Chicken_C");
    spawn_buff(&mut h.w, f, cls, 0.0, Some(0), Some(f), true, None);
    for _ in 0..4 {
        h.step(&[NO_INPUT, InputState::default()]);
    }
    let p = h.w.fighter(f).shapes.polymorph.as_ref().expect("the chicken's polymorph");
    assert_eq!(p.from, Some(ITEMS_OWNER), "the items part owns the mesh");
    for round in 0..2 {
        let arg = BuffSignalArg { dir: Some(6.0), duration: Some(0.2), ..BuffSignalArg::default() };
        buff_signal(&mut h.w, f, BuffSignal::EvadeTriggered, arg);
        let t = h.w.fighter(f).shapes.polymorph.as_ref().expect("polymorph").evade_timer;
        assert!(t > 0, "round {round}: the evade sets its reset timer from the montage's length");
        h.w.fighter_mut(f).evade.evading = true;
        for _ in 0..=t {
            h.step(&[NO_INPUT, InputState::default()]);
        }
        let fi = h.w.fighter(f);
        assert!(!fi.evade.evading, "round {round}: the timer ran out and reset the evade");
        assert_eq!(fi.shapes.polymorph.as_ref().expect("polymorph").evade_timer, -1);
    }
}
