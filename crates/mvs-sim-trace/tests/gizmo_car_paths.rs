//! Gizmo's popcorn paths no match runs (`chars/gizmo/popcorn.rs`), on a fresh World of two fighters with the popcorn
//! spawned through its owner's Comp_Spawner: its trigger box before it is shot, and the record the trace writes of it.
//! Without a store the tests check nothing and say so.
//! The Gizmo pawn is not stepped (its perks are the pawn track's): the popcorn is read where it spawns.
//! Not reached: the pops, the shotgun, the arrow's hit and the root box (they call the pawn's music, bow and perk
//! hooks, which the Gizmo pawn track owns, and no World without a Gizmo reaches the arrow).

use mvs_sim::actors::ActorRef;
use mvs_sim::chars::gizmo::popcorn::{POPCORN, box_shapes, extra_hit_boxes};
use mvs_sim::fighter::FighterId;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::spawning::comp_spawn_projectile;
use mvs_sim_trace::harness::{self, Harness};

fn player(c: &str, team: u8) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), team: Some(team), ..MatchPlayer::default() }
}

fn pair() -> Option<(Harness, FighterId)> {
    let setup = MatchSetup { players: vec![player("gizmo", 0), player("shaggy", 1)], ..MatchSetup::default() };
    let h = harness::fresh(&setup, 1)?;
    let f = h.fighter(0);
    Some((h, f))
}

#[test]
fn an_unshot_popcorn_has_only_its_trigger_box() {
    let Some((mut h, f)) = pair() else { return };
    let cls = h.w.lit(POPCORN);
    let (x, y) = (h.w.fighter(f).body.x, h.w.fighter(f).body.y);
    let Some(id) = comp_spawn_projectile(&mut h.w, f, cls, x + 300.0, y + 200.0, [None; 3], 0.0, 0.0, None) else {
        return;
    };
    assert_eq!(extra_hit_boxes(&h.w, id), 1, "the trigger box alone before the shot");
    let mut shapes = Vec::new();
    box_shapes(h.w.actors.projectiles.get(id).expect("the popcorn is in the list"), 0, &mut shapes);
    assert!(!shapes.is_empty(), "the trigger box has a shape");
    assert!(h.w.actors.projectiles.iter().any(|(i, _)| ActorRef::Projectile(i) == ActorRef::Projectile(id)));
}
