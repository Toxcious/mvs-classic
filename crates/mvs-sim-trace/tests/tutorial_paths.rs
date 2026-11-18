//! Tutorial paths the tut-intro match need not run: the pause menu's Skip Step and Skip To End, the widget's skip
//! button (BPI_AdvanceTutorial) on the last step and the end of the tutorial, the bag's and the player's ring-outs,
//! on TutorialActor_Intro.

use mvs_sim::events::{Event, TutorialUiCall};
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::match_side::TutorialEvent;
use mvs_sim::input::NO_INPUT;
use mvs_sim::modes::tutorial::run;
use mvs_sim::world::World;
use mvs_sim::world::pawns::PawnCtl;
use mvs_sim_trace::harness;

/// The tut-intro match's World at its first frame (`harness::at`: the tutorial data from the trace's header),
/// TutorialActor_Intro on the tutorial level.
fn intro() -> Option<World> {
    harness::at("tut-intro", 0).map(|h| h.w)
}

fn steps(w: &mut World, n: usize) {
    for _ in 0..n {
        w.step(&[NO_INPUT]);
    }
}

fn finished(w: &World) -> bool {
    w.events.iter().any(|e| matches!(e, Event::TutorialUi { call: TutorialUiCall::Finished { .. }, .. }))
}

#[test]
fn skip_step_goes_to_the_next_concept_and_skip_to_end_to_the_last_step() {
    let Some(mut w) = intro() else { return };
    steps(&mut w, 30);
    run::intro_complete(&mut w);
    assert_eq!(run::step_index(&w), 0);
    let d = run::actor_data(&w).expect("the Intro's data").clone();
    let last = i32::try_from(d.steps.len()).expect("steps") - 1;
    steps(&mut w, 600);
    run::start_pressed(&mut w);
    steps(&mut w, 120);
    let at = run::step_index(&w);
    let next = d.steps.iter().enumerate().skip(usize::try_from(at + 1).expect("index")).find(|(_, s)| s.new_concept);
    run::skip_step(&mut w);
    match next {
        Some((i, _)) => {
            assert_eq!(run::step_index(&w), i32::try_from(i).expect("index"), "Skip Step: the next concept")
        }
        None => assert_eq!(run::step_index(&w), last, "Skip Step with no later concept: walks to the last step"),
    }
    steps(&mut w, 300);
    run::skip_to_end(&mut w);
    assert_eq!(run::step_index(&w), last, "Skip To End: the last step");
    assert!(!run::over(&w));
}

/// BPI_AdvanceTutorial saves the score and re-checks the step's counters: on an unfinished step nothing moves. The
/// last step's tasks done: the finish (Finished, TutorialOver), and the end screen's desaturation to its end.
#[test]
fn advance_rechecks_and_the_last_step_ends_the_tutorial() {
    let Some(mut w) = intro() else { return };
    steps(&mut w, 30);
    run::intro_complete(&mut w);
    steps(&mut w, 600);
    run::start_pressed(&mut w);
    steps(&mut w, 60);
    run::skip_to_end(&mut w);
    steps(&mut w, 600);
    run::start_pressed(&mut w);
    steps(&mut w, 120);
    let d = run::actor_data(&w).expect("the Intro's data").clone();
    let last = i32::try_from(d.steps.len()).expect("steps") - 1;
    assert_eq!(run::step_index(&w), last);
    let s = d.steps.last().expect("a last step");
    run::advance(&mut w);
    steps(&mut w, 10);
    if s.tasks.iter().any(|t| t.req > 0) {
        assert_eq!(run::step_index(&w), last, "advance on an unfinished step: the counters re-checked, the step stays");
        assert!(!run::over(&w));
    }
    let mut seen = finished(&w);
    for t in &s.tasks {
        for _ in 0..t.req {
            run::bus_event(&mut w, TutorialEvent::from_byte(i32::from(t.event)));
            seen |= finished(&w);
        }
    }
    for _ in 0..600 {
        if seen && run::over(&w) {
            break;
        }
        w.step(&[NO_INPUT]);
        seen |= finished(&w);
    }
    assert!(seen, "the Finished call");
    assert!(run::over(&w), "the tutorial is over");
    run::desaturate_environment(&mut w);
    steps(&mut w, 30);
    assert_eq!(run::desaturation(&w), 1.0, "the end screen's desaturation reaches the track's end");
}

fn slot_pawn(w: &World, slot: i32) -> Option<FighterId> {
    w.pawns.as_ref()?.iter().copied().find(|&f| w.fighter(f).life.slot == Some(slot))
}

fn ring_out(w: &mut World, f: FighterId) {
    let (x, y) = (w.fighter(f).body.x, mvs_sim::fighter::actor_y(w, f));
    mvs_sim::fighter::moves::recall_move(w, f, x + 30000.0, y);
}

/// The oracle walk's ring-out checks: a bag rung out on a RingOut task completes the step
/// and is not replaced while the step transitions; the player rung out comes back 1 s later, held by the player.
#[test]
fn a_bag_rung_out_is_not_replaced_and_the_player_comes_back() {
    let Some(mut w) = intro() else { return };
    steps(&mut w, 30);
    run::intro_complete(&mut w);
    let d = run::actor_data(&w).expect("the Intro's data").clone();
    let (mut bag_out, mut respawned) = (false, false);
    for _ in 0..40 {
        if run::over(&w) {
            break;
        }
        let idx = run::step_index(&w);
        let s = &d.steps[usize::try_from(idx).expect("index")];
        if run::pawn_controlled_by_tutorial(&w) == Some(true) {
            steps(&mut w, (s.start_button_disabled_time * 60.0) as usize + 240);
            run::start_pressed(&mut w);
            steps(&mut w, 30);
        }
        if !bag_out
            && s.tasks.iter().any(|t| t.event == 22)
            && let Some(bag) = slot_pawn(&w, 1)
        {
            ring_out(&mut w, bag);
            steps(&mut w, 10);
            let again = slot_pawn(&w, 1);
            let gone = !w.pawns.as_ref().is_some_and(|ps| ps.contains(&bag));
            bag_out = run::step_index(&w) != idx && gone && again.is_none();
            assert!(bag_out, "step {idx}: the bag rung out completes the step and no bag replaces it");
        }
        if idx == 1 && !respawned {
            let me = slot_pawn(&w, 0).expect("the player's pawn");
            ring_out(&mut w, me);
            steps(&mut w, 10);
            let gone = !w.pawns.as_ref().is_some_and(|ps| ps.contains(&me));
            steps(&mut w, 60);
            let again = slot_pawn(&w, 0);
            respawned =
                gone && again.is_some_and(|a| a != me && matches!(w.pawn_ctl.ctl.get(&a), Some(PawnCtl::Player)));
            assert!(respawned, "the player rung out comes back 1 s later");
            steps(&mut w, 30);
        }
        if run::step_index(&w) == idx {
            for t in &s.tasks {
                for _ in 0..t.req {
                    if run::step_index(&w) == idx {
                        run::bus_event(&mut w, TutorialEvent::from_byte(i32::from(t.event)));
                    }
                }
            }
        }
        steps(&mut w, 150);
    }
    steps(&mut w, 30);
    assert!(run::over(&w), "the Intro played to its end");
    assert!(bag_out && respawned, "bag out {bag_out}, respawned {respawned}");
}
