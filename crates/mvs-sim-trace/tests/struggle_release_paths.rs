//! StruggleInput counts the release of every button, the Taunt's too (`combat/holds.rs` `struggle_input`; the trace
//! records carry no `taunt_held` release inside a hold). A fresh World of Shaggy against Shaggy with a Struggle
//! state set by hand. Without a store the test checks nothing and says so.

use mvs_sim::combat::buffs::Struggle;
use mvs_sim::combat::holds::struggle_input;
use mvs_sim::input::NO_INPUT;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness;

fn player(bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some("shaggy".to_owned()), bot, ..MatchPlayer::default() }
}

#[test]
fn a_taunt_release_is_one_struggle_call() {
    let setup = MatchSetup { players: vec![player(false), player(true)], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    h.step(&[NO_INPUT, NO_INPUT]);
    let f = h.fighter(0);
    let count = |h: &harness::Harness| h.w.fighter(f).buffs.struggle.as_ref().map(|s| s.count);
    h.w.fighter_mut(f).buffs.struggle =
        Some(Struggle { inc: 0.0, max: 5.0, count: 0.0, avail: 5.0, period: 0.0, left: 1.0 });
    // Held on both ticks: no release.
    h.w.fighter_mut(f).controls.prev.taunt_held = true;
    h.w.fighter_mut(f).controls.inp.taunt_held = true;
    struggle_input(&mut h.w, f);
    assert_eq!(count(&h), Some(0.0));
    // Released this tick: one call.
    h.w.fighter_mut(f).controls.inp.taunt_held = false;
    struggle_input(&mut h.w, f);
    assert_eq!(count(&h), Some(1.0));
    // Not held before: no release.
    h.w.fighter_mut(f).controls.prev.taunt_held = false;
    struggle_input(&mut h.w, f);
    assert_eq!(count(&h), Some(1.0));
}
