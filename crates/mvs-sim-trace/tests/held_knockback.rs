//! A knockback hit on a fighter the grapple holds past the hit's pause, no trace match runs: batman-shaggy's first hit
//! that leaves Shaggy a launch to apply at the pause's end, Shaggy grappled from that frame and released 12 frames
//! after the pause. Under the grapple (CMC +0x4c0) ApplyKnockback's launch is dropped (the MOVE_None early exit and
//! ClearAccumulatedForces), its SetMovementMode(Custom, 2) leaves None, and RemoveGrapple writes Falling: the release
//! finds no launch, no knockback mode and the velocity 0.

use mvs_sim::combat::holds::{initiate_grapple, remove_grapple};
use mvs_sim::input::NO_INPUT;
use mvs_sim_trace::harness;

const MATCH: &str = "batman-shaggy";

/// The first frame where Shaggy (fighter 1) is in a hit's pause with a launch that moves him.
fn first_launch() -> Option<usize> {
    let mut h = harness::at_baseline(MATCH, 0)?;
    let f = h.fighter(1);
    for _ in 0..3000 {
        h.step_recorded();
        let x = &h.w.fighter(f).hit;
        if x.hitpause > 1.0 && x.launch.as_ref().is_some_and(|l| l.vx != 0.0 || l.vy != 0.0) {
            return Some(h.frame_index());
        }
    }
    None
}

#[test]
fn a_launch_under_the_grapple_is_dropped() {
    let Some(frame) = first_launch() else { return };
    let mut h = harness::at_baseline(MATCH, frame).expect("the frame loads");
    let f = h.fighter(1);
    initiate_grapple(&mut h.w, f);
    let pause = h.w.fighter(f).hit.hitpause as usize;
    for _ in 0..pause + 12 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    remove_grapple(&mut h.w, f);
    h.step(&[NO_INPUT, NO_INPUT]);
    let x = h.w.fighter(f);
    assert!(x.hit.launch.as_ref().is_none_or(|l| l.vx == 0.0 && l.vy == 0.0), "no launch left at the release");
    assert!(!x.hit.kb_mode, "no knockback mode at the release");
    // one frame of Falling from 0: gravity alone
    assert!(
        x.body.vx == 0.0 && x.body.vy <= 0.0 && x.body.vy > -100.0,
        "no knockback speed: {} {}",
        x.body.vx,
        x.body.vy
    );
}
