//! Marvin's UFO timers no trace match runs on its own (`chars/marvin/ufo.rs`): K2_SetTimerDelegate's firing tick
//! (`timer_at`). Against the oracle's `timerAt` read line by line (no oracle probe of this path).

use mvs_sim::chars::marvin::ufo::timer_at;
use mvs_sim::input::InputState;
use mvs_sim_trace::harness;

#[test]
fn a_timer_without_a_rate_never_fires_and_one_with_a_rate_fires_after_it() {
    let Some(mut h) = harness::at("marvin-marvin", 5) else { return };
    let idle = vec![InputState::default(); h.w.players.len()];
    h.step(&idle);
    assert_eq!(timer_at(&h.w, Some(0.0)), 0.0);
    let now = mvs_sim::real(h.w.flow.tick);
    let a = timer_at(&h.w, Some(1.0));
    let b = timer_at(&h.w, Some(2.0));
    assert!(a > now && b > a, "a longer rate fires later: {now} {a} {b}");
}
