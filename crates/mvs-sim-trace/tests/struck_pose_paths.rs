//! The hurt shapes LeBron's ball's overlap meets (`fighter::anim::struck_boxes`): the pose the step found, while the
//! fighter's own update and a test outside a step see the pose of now.

use mvs_sim::fighter::anim::{drop_struck_poses, sample_boxes, struck_boxes, take_struck_poses};
use mvs_sim_trace::harness;

/// lebron-shaggy frame 1295: LeBron (fighter 0) at 0.2167 s of his forward attack, his shapes moving.
const MATCH: &str = "lebron-shaggy";
const FRAME: usize = 1295;

#[test]
fn an_actors_overlap_meets_the_pose_the_step_found() {
    let Some(mut h) = harness::at_baseline(MATCH, FRAME) else { return };
    let f = h.fighter(0);
    let mut found = Vec::new();
    assert!(sample_boxes(&h.w, f, &mut found));
    take_struck_poses(&mut h.w);
    // the mesh tick's advance
    h.w.fighter_mut(f).anim.anim_time += 0.05;
    let mut now = Vec::new();
    assert!(sample_boxes(&h.w, f, &mut now));
    assert_ne!(found, now, "the advance moves the shapes");

    let mut met = Vec::new();
    assert!(struck_boxes(&h.w, f, &mut met));
    assert_eq!(met, found, "an actor's overlap: the pose the step found");

    h.w.overlap_mid_frame = true;
    assert!(struck_boxes(&h.w, f, &mut met));
    assert_eq!(met, now, "the fighter's own update: his pose of now");
    h.w.overlap_mid_frame = false;

    drop_struck_poses(&mut h.w);
    assert!(struck_boxes(&h.w, f, &mut met));
    assert_eq!(met, now, "outside a step: the pose of now");
}
