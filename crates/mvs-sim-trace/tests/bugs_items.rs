//! Paths of Bugs Bunny's items that no trace runs: Rick's-style runner of Item_Meeseek (SelfActivate_Meeseeks_Run_C)
//! spawned from a fighter and stepped. They reach `montage_play` (the shared montage part), so each test waits for
//! that part and is ignored until it lands.

use mvs_sim::input::NO_INPUT;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::spawning::comp_spawn_projectile;
use mvs_sim_trace::harness;

fn player(c: &str, bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, ..MatchPlayer::default() }
}

#[test]
fn the_meeseeks_runner_spawns_runs_and_ends() {
    let setup = MatchSetup {
        players: vec![player("bugs", false), player("shaggy", true)],
        mutators: vec!["Mutator_RandomItemSpawns".to_owned()],
        ..MatchSetup::default()
    };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    for _ in 0..10 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    let f = h.fighter(0);
    let cls = h.w.lit("SelfActivate_Meeseeks_Run_C");
    let (x, y) = (h.w.fighter(f).body.x, h.w.fighter(f).body.y);
    let id =
        comp_spawn_projectile(&mut h.w, f, cls, x + 68.0, y, [Some(1.0), Some(0.0), Some(0.0)], 1200.0, 1200.0, None)
            .expect("the runner spawns");
    for _ in 0..600 {
        h.step(&[NO_INPUT, NO_INPUT]);
        if h.w.actors.projectiles.get(id).is_none_or(|p| p.dead) {
            return;
        }
    }
    panic!("the runner never ended");
}
