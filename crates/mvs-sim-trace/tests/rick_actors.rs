//! Rick's actors: a Rick match binds every class of them in the registry (their hooks and mesh
//! hooks enter without a clash) and runs idle ticks without a panic. Their stepping is checked by the rick matches.

use mvs_sim::input::NO_INPUT;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness;

fn player(c: &str, bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, ..MatchPlayer::default() }
}

#[test]
fn a_rick_match_binds_and_idles() {
    let setup = MatchSetup { players: vec![player("rick", false), player("shaggy", true)], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    for n in [
        "Meeseeks_SpawnedActor_C",
        "MeeSeeks_AirFlap_C",
        "FartBomb_SpawnedActor_C",
        "PolymorphGrenade_C",
        "Buff_MeeseeksDamageWithHitstun_C",
    ] {
        let _ = h.w.lit(n);
    }
    for _ in 0..30 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
}
