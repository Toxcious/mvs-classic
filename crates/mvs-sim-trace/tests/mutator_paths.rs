//! Fighter mutator paths no trace match runs: Mutator_Boost_ChargeFreeze's (Ice Charge's) charged hit (the match
//! runs none, so only its refusal is on the trace).

use mvs_sim::combat::buffs::{buff_of, buff_signal};
use mvs_sim::input::NO_INPUT;
use mvs_sim::registry::signals::{BuffSignal, BuffSignalArg};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness;

fn player(c: &str) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), ..MatchPlayer::default() }
}

#[test]
fn a_hit_charged_past_half_freezes_and_a_lighter_one_does_not() {
    let setup = MatchSetup {
        players: vec![player("superman"), player("shaggy")],
        mutators: vec!["Mutator_Boost_ChargeFreeze".to_owned()],
        ..MatchSetup::default()
    };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    let (a, v) = (h.fighter(0), h.fighter(1));
    for _ in 0..5 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    let ice = h.w.lit("Buff_Ice_C");
    let arg = || BuffSignalArg { victim: Some(v), ..BuffSignalArg::default() };
    // (Ice Charge's rework: the build's threshold is a full charge, the port's half)
    h.w.fighter_mut(a).charge.charge_pct = 0.4;
    buff_signal(&mut h.w, a, BuffSignal::HitEnemy, arg());
    assert!(buff_of(&h.w, v, ice).is_none(), "a charge under half freezes nothing");
    h.w.fighter_mut(a).charge.charge_pct = 0.5;
    buff_signal(&mut h.w, a, BuffSignal::HitEnemy, arg());
    let b = buff_of(&h.w, v, ice).expect("a half charge gives Buff_Ice");
    assert_eq!(mvs_sim::combat::buffs::buff(&h.w, b).map(|x| x.stacks), Some(15.0));
}
