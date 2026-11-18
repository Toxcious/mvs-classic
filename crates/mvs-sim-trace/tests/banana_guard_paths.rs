//! ItemHelper_BananaGuard_C's paths no match runs (`items/banana_guard.rs`), on ffa-items-b's World loaded at frame
//! 8846 (the first guard running with its spear on): its spear on another guard's capsule (`damage_actor` through
//! the pawn body), the walk stopped at a ledge, and a base that is lost while walking. Without the trace or a store
//! the tests check nothing and say so.
//! Not reached: the spear on a projectile's damage box (no class with one is in the match's data), an ally's hit and
//! a stage solid's box as the base (Sky Arena has no solid boxes).

use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, ActorRef};
use mvs_sim::items::banana_guard::{BananaGuard, GuardMode, GuardSolid, spawn_at, spear_pose};
use mvs_sim_trace::harness::{self, Harness};

const AT: usize = 8846;

fn guard_ids(h: &Harness) -> Vec<ActorId> {
    h.w.actors
        .actors
        .iter()
        .filter(|(_, a)| matches!(a.state, WorldActorState::BananaGuard(_)))
        .map(|(i, _)| i)
        .collect()
}

fn guard(h: &Harness, id: ActorId) -> &BananaGuard {
    match &h.w.actors.actors.get(id).expect("the guard").state {
        WorldActorState::BananaGuard(g) => g,
        _ => unreachable!(),
    }
}

fn guard_mut(h: &mut Harness, id: ActorId) -> &mut BananaGuard {
    match &mut h.w.actors.actors.get_mut(id).expect("the guard").state {
        WorldActorState::BananaGuard(g) => g,
        _ => unreachable!(),
    }
}

fn loaded() -> Option<(Harness, ActorId)> {
    let h = harness::at("ffa-items-b", AT)?;
    let ids = guard_ids(&h);
    let id = *ids.first()?;
    assert!(guard(&h, id).hit_on, "the guard runs with its spear on at frame {AT}");
    Some((h, id))
}

#[test]
fn the_spear_hits_another_teams_guard_once() {
    let Some((mut h, id)) = loaded() else { return };
    let mine = h.w.actors.actors.get(id).expect("the guard").owner;
    let team = guard(&h, id).team;
    let other = (0..h.w.fighters.len()).map(|n| h.fighter(n)).find(|&f| h.w.fighter(f).team.team != team);
    let Some(other) = other else { return };
    assert_ne!(other, mine);
    let spear = spear_pose(&mut h.w, id).expect("the data has the spear's track");
    let cls = h.w.actors.actors.get(id).expect("the guard").cls;
    let y = guard(&h, id).y;
    let b = spawn_at(&mut h.w, other, cls, spear.x, y);
    // (the new guard's own spear is off until its RunDelayTime)
    h.step_recorded();
    let g = guard(&h, id);
    assert!(g.hit_actors.contains(&ActorRef::Actor(b)), "the spear met the other guard's capsule: {:?}", g.hit_actors);
    let n = g.hit_actors.len();
    h.step_recorded();
    assert_eq!(guard(&h, id).hit_actors.len(), n, "HitEachActorOnlyOnce keeps it to one hit");
}

#[test]
fn the_walk_stops_at_a_ledge() {
    let Some((mut h, id)) = loaded() else { return };
    let right = mvs_sim::math::narrow(h.w.stage.ground.right);
    let g = guard_mut(&mut h, id);
    assert_eq!(g.mode, GuardMode::Walk);
    let x0 = right + g.perch - 10.0;
    g.x = x0;
    g.vel_x = g.max_walk;
    g.input = 1.0;
    g.run_gate = true; // (the run past 500)
    h.step_recorded();
    let g = guard(&h, id);
    assert_eq!(g.mode, GuardMode::Walk, "bCanWalkOffLedges false: it does not fall");
    assert_eq!(g.x, x0, "the move past the perch radius is reverted");
    assert_eq!(g.vel_x, 0.0, "with the velocity zeroed");
    assert!(!g.run_gate && !g.hit_on && g.stop_at >= 0, "the stop turns the run off and sets the turn's Delay");
}

#[test]
fn a_lost_base_is_left_for_the_floor_under_it() {
    let Some((mut h, id)) = loaded() else { return };
    let g = guard_mut(&mut h, id);
    g.base = Some(GuardSolid::Static(0));
    h.step_recorded();
    let g = guard(&h, id);
    assert_eq!(g.mode, GuardMode::Walk);
    assert_eq!(g.base, Some(GuardSolid::Ground), "a stage solid's box (no `on`) is lost, the ground found under it");
}
