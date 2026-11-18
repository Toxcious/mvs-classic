//! Tom and Jerry paths no trace match runs: a Jerry gone from the World that a projectile's hit set still names (kept
//! in `gone_actors` and written whole, as the oracle writes the object), and a pack of the class data in a fighter's
//! override packs (written as `{static: "object"}`).
//! Unverified: no oracle probe of the same World was run; the asserts are the oracle's record rules.

use mvs_sim::actors::ActorRef;
use mvs_sim::chars::tom_and_jerry::jerry::spawn_jerry;
use mvs_sim::fighter::anim::OverridePack;
use mvs_sim::input::NO_INPUT;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::spawning::comp_spawn_projectile;
use mvs_sim_trace::harness;

fn player(c: &str) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), ..MatchPlayer::default() }
}

#[test]
fn a_dead_jerry_a_hit_set_names_is_kept_and_written_whole() {
    let setup = MatchSetup { players: vec![player("tomandjerry"), player("shaggy")], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    let (t, s) = (h.fighter(0), h.fighter(1));
    for _ in 0..10 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    let (x, y) = (h.w.fighter(t).body.x, h.w.fighter(t).body.y + 300.0);
    let j = spawn_jerry(&mut h.w, t, x, y, 0.0, 0.0, false, 0.0).expect("Jerry spawns");
    let sandwich = h.w.lit("ShaggySandwichProjectile_C");
    let p =
        comp_spawn_projectile(&mut h.w, s, sandwich, x + 900.0, y, [Some(0.0), Some(0.0), Some(0.0)], 0.0, 0.0, None)
            .expect("a projectile spawns");
    h.w.actors.projectiles.get_mut(p).expect("it lives").hit_actors.insert(ActorRef::Actor(j));
    h.w.actors.actors.get_mut(j).expect("Jerry lives").dead = true;
    h.step(&[NO_INPUT, NO_INPUT]);
    assert!(h.w.actors.actors.get(j).is_none(), "the dead Jerry left the list");
    assert!(h.w.actors.gone_actors.iter().any(|(g, _)| *g == j), "the hit set keeps him");
    let text = format!("{:?}", h.record());
    assert!(text.contains("Jerry_SpawnedActor_C"), "the record writes the object whole");
    h.step(&[NO_INPUT, NO_INPUT]);
    h.w.actors.projectiles.get_mut(p).expect("it lives").hit_actors.clear();
    h.step(&[NO_INPUT, NO_INPUT]);
    assert!(h.w.actors.gone_actors.is_empty(), "nothing names him now");
}

#[test]
fn a_pack_of_the_class_data_is_written_as_static() {
    let setup = MatchSetup { players: vec![player("tomandjerry"), player("shaggy")], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    let t = h.fighter(0);
    h.step(&[NO_INPUT, NO_INPUT]);
    h.w.fighter_mut(t).branching.override_packs =
        Some(vec![OverridePack { priority: 0.0, overrides: Vec::new(), from_data: true }]);
    let text = format!("{:?}", h.record());
    assert!(text.contains("\"static\""), "a data pack is written by its kind alone");
}

/// BPI_SetMobilityToStationary on a Jerry (a reflect's pause, Marvin's bubble or reverse): the root's count holds his
/// ProjectileMovement still until the last release, and the record writes the count and the mobility in his rest.
#[test]
fn a_stationary_jerry_holds_his_place() {
    use mvs_sim::combat::holds::set_mobility_to_stationary;
    let setup = MatchSetup { players: vec![player("tomandjerry"), player("shaggy")], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    let t = h.fighter(0);
    for _ in 0..10 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    let (x, y) = (h.w.fighter(t).body.x, h.w.fighter(t).body.y + 600.0);
    let j = spawn_jerry(&mut h.w, t, x, y, 800.0, 0.0, false, 0.0).expect("Jerry spawns");
    h.step(&[NO_INPUT, NO_INPUT]);
    let a = ActorRef::Actor(j);
    set_mobility_to_stationary(&mut h.w, a, true);
    set_mobility_to_stationary(&mut h.w, a, true);
    let at = format!("{:?}", h.record());
    assert!(at.contains("stationaryCount"), "the record writes the count");
    let place = |h: &harness::Harness| mvs_sim::actors::world_actor::actor_place(&h.w, a).expect("Jerry lives");
    let before = place(&h);
    h.step(&[NO_INPUT, NO_INPUT]);
    assert_eq!(place(&h), before, "a Stationary root holds his place");
    set_mobility_to_stationary(&mut h.w, a, false);
    h.step(&[NO_INPUT, NO_INPUT]);
    assert_eq!(place(&h), before, "the count is still above 0");
    set_mobility_to_stationary(&mut h.w, a, false);
    let wa = h.w.actors.actors.get(j).expect("Jerry lives");
    assert_eq!((wa.mobility, wa.stationary_count), (None, None));
    h.step(&[NO_INPUT, NO_INPUT]);
    assert_ne!(place(&h), before, "Movable again, he moves");
}
