//! Advanced-tutorial paths the tut-* matches need not run: the demo's DemoComplete cleanup of the Meeseeks, and the
//! fighters' lists of stage platforms following a run-time platform's add and remove.

use mvs_sim::modes::tutorial::advanced::meeseeks::MeeseeksEnd;
use mvs_sim::modes::tutorial::classes::{self, RunList};
use mvs_sim::world::World;
use mvs_sim::world::flow::DelegateCall;
use mvs_sim::world::stage::remove_runtime_solid;
use mvs_sim::world::stage_actors::StageActorOwn;
use mvs_sim_trace::harness;

/// The ends of the stage actors that are dead Meeseeks, and the count of live ones.
fn meeseeks(w: &World) -> (usize, Vec<MeeseeksEnd>) {
    let Some(r) = w.stage_run.as_ref() else { return (0, Vec::new()) };
    let mut live = 0;
    let mut ends = Vec::new();
    for a in &r.actors {
        if let StageActorOwn::Meeseeks(o) = &a.own {
            if a.dead {
                ends.push(o.end);
            } else {
                live += 1;
            }
        }
    }
    (live, ends)
}

/// EvadeToAttackCleanup (TutorialDemoComplete): K2_DestroyActor on every Tutorial_Meeseeks, with no effect of its own.
#[test]
fn demo_complete_destroys_every_meeseeks_with_no_effect() {
    let Some(mut h) = harness::at("tut-dashattack", 0) else { return };
    let mut found = false;
    for _ in 0..600 {
        h.step_recorded();
        if meeseeks(&h.w).0 > 0 {
            found = true;
            break;
        }
    }
    assert!(found, "a Meeseeks is spawned by the demo within 600 frames");
    let before = meeseeks(&h.w).1.len();
    classes::broadcast_run(&mut h.w, RunList::DemoComplete, &DelegateCall::DemoComplete);
    let (live, ends) = meeseeks(&h.w);
    assert_eq!(live, 0, "no live Meeseeks after the demo's cleanup");
    assert!(ends.len() > before);
    assert_eq!(ends.last(), Some(&MeeseeksEnd::Plain));
}

/// The stage's platforms are one array every fighter holds (`World.stagePlatforms`): a run-time platform joins every
/// fighter's list, a pawn spawned after it has it, and its destroy takes it out of each.
#[test]
fn a_run_time_platform_is_in_every_fighters_list_until_it_is_destroyed() {
    let Some(mut h) = harness::at("tut-fastfall", 0) else { return };
    let mut found = false;
    for _ in 0..2400 {
        h.step_recorded();
        if !h.w.stage.stage_platforms.is_empty() && h.w.pawns.as_ref().is_some_and(|p| p.len() > 1) {
            found = true;
            break;
        }
    }
    assert!(found, "the platform demo spawns its platform and a bag within 2400 frames");
    let n = h.w.stage.stage_platforms.len();
    let ids: Vec<_> = h.w.fighters.ids().collect();
    for &f in &ids {
        let want: Vec<u32> = (0..u32::try_from(n).expect("a count")).collect();
        assert_eq!(h.w.fighter(f).platforms.stage_platforms, want, "every fighter holds every platform");
    }
    let spec =
        h.w.runtime_solids.as_ref().and_then(|l| l.iter().find(|s| !s.destroyed)).map(|s| s.spec).expect("a solid");
    remove_runtime_solid(&mut h.w, spec);
    for &f in &ids {
        assert_eq!(h.w.fighter(f).platforms.stage_platforms.len(), n - 1, "the destroyed platform leaves each list");
    }
}
