//! The Tree Fort trees' bounce of a spawned actor that no match runs with a projectile (`stages/tree_fort/pillar.rs`,
//! the BPI_SpawnedActor branch of TreeFortTrampolineBounce), on the World of stage-treehouse at a baseline frame: a
//! projectile's move meets the round top (a polygon solid) and the tree sends it up.
//! Without a store or the trace the tests check nothing and say so.

use mvs_sim::events::Event;
use mvs_sim::world::spawning::comp_spawn_projectile;
use mvs_sim::world::stage_actors::StageActorOwn;
use mvs_sim_trace::harness;

/// The trees' (bounce tick, wiggle count), by slot.
fn trees(h: &harness::Harness) -> Vec<(i32, usize)> {
    let run = h.w.stage_run.as_ref().expect("a stage run");
    run.actors
        .iter()
        .filter_map(|a| match &a.own {
            StageActorOwn::Pillar(p) => Some((p.bounce_at, p.wiggles.len())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_projectile_on_a_tree_top_is_bounced() {
    let Some(mut h) = harness::at("stage-treehouse", 1500) else { return };
    let before = trees(&h);
    assert_eq!(before.len(), 2, "the two trees");
    let cls = h.w.lit("C017_Spit_Bolt_C");
    // above the right tree's round top (x 1472..2093, its top at y 1038), straight down at 1200
    let id = (0..h.w.all_ids().len())
        .find_map(|n| {
            let f = h.fighter(n);
            comp_spawn_projectile(
                &mut h.w,
                f,
                cls,
                1780.0,
                1150.0,
                [Some(0.0), Some(0.0), Some(-1.0)],
                1200.0,
                1200.0,
                None,
            )
        })
        .expect("the bolt spawns");
    let mut bounced = false;
    for _ in 0..30 {
        h.step_recorded();
        if trees(&h).iter().zip(&before).any(|(a, b)| a.0 != b.0) {
            bounced = true;
            break;
        }
        assert!(h.w.actors.projectiles.get(id).is_some_and(|p| !p.dead), "the bolt lives until it meets the top");
    }
    assert!(bounced, "the tree took the bolt's hit on its top");
    assert!(trees(&h).iter().zip(&before).any(|(a, b)| a.1 > b.1), "the bounce wiggles the tree");
    let sounds = h.w.events.iter().filter(|e| matches!(e, Event::SpawnedSound { .. })).count();
    assert_eq!(sounds, 0, "AudioBounceMulticast(0.7, null): the audio event returns on no BouncingActor");
    // up x FClamp(|1200|, 1000, 2500) turned by (Y - the tree's Y) / -420 x 20 degrees: nearly straight up
    let p = h.w.actors.projectiles.get(id).expect("the bolt");
    assert!(p.vy > 1100.0 && p.vx.abs() < 100.0, "the bolt goes up ({}, {})", p.vx, p.vy);
}
