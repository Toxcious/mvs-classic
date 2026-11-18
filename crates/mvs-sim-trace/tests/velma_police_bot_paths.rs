//! The police minion's tree on a fresh World (`chars/velma/police.rs` `target_read`): no baseline match calls the police
//! (VelmaState.police stays -1 in velma-police-combat), so the car's read of its bot's blackboard EnemyActor is checked
//! here. Without a store the tests check nothing and say so.

use mvs_sim::chars::velma::police::call_police;
use mvs_sim::chars::velma::{police_w, velma};
use mvs_sim::fighter::FighterId;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};
use std::sync::Arc;

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn player(c: &str) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), ..MatchPlayer::default() }
}

fn pair() -> Option<(Harness, FighterId, FighterId)> {
    let setup = MatchSetup { players: vec![player("velma"), player("shaggy")], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    h.w.bot_trees = Some(Arc::new(h.w.data.bot_trees.clone()));
    for _ in 0..10 {
        h.step(&NONE);
    }
    let (v, s) = (h.fighter(0), h.fighter(1));
    Some((h, v, s))
}

/// The car's tree sets EnemyActor (VelmaPolice_DetectEnemy_BTS); Pawn_VelmaAIBotPolice @492 reads it into
/// TargetedEnemy and stops its target loop.
#[test]
fn the_car_reads_its_bots_enemy() {
    let Some((mut h, v, s)) = pair() else { return };
    call_police(&mut h.w, v);
    let m = velma(&h.w, v).police.expect("a police spawned");
    assert!(police_w(&mut h.w, m).target_loop);
    for _ in 0..300 {
        h.step(&NONE);
        if !police_w(&mut h.w, m).target_loop {
            break;
        }
    }
    let p = police_w(&mut h.w, m);
    assert!(!p.target_loop, "the target loop never read an EnemyActor");
    assert_eq!(p.targeted_enemy, Some(s));
}
