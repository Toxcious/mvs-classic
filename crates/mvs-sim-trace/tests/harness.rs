//! The harness's own check: a World loaded at a Rust baseline trace's frame and stepped with the recorded inputs writes the
//! frames the trace holds next (events left out: the step's own are not written back). A test of a path no trace
//! runs (a file per character, `tests/<key>_paths.rs`) starts as this one does, then changes the World where its
//! path starts and asserts on what follows.

use mvs_sim_trace::harness;

#[test]
fn a_loaded_world_steps_as_the_trace() {
    let Some(mut h) = harness::at_baseline("wonderwoman-wonderwoman", 100) else { return };
    for _ in 0..30 {
        h.step_recorded();
        let d = h.diffs();
        assert!(d.is_empty(), "frame {}:\n{}", h.frame_index(), d.join("\n"));
    }
}
