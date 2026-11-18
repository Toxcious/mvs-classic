//! The match camera against a pose tape the oracle recorded: a World stepped through the trace match the tape was
//! recorded on, the camera stepped after each step, and each tape frame's camera (origin, view rotation, projection)
//! compared at the frame's alpha. The tape is `MVS_POSE_TAPE` (default `.scratch/tapes/lab-picture.tape` of the main
//! tree, beside the traces), the trace `<MVS_TRACES>/<the tape's run>.trace`; without them, or without the dev store,
//! the test says so and passes.

use std::path::PathBuf;

use mvs_view::{Camera, MatchCamera, PoseTape};

/// Within this of the tape: cm (the origin), unitless (the rotation), relative (the projection).
const TOL_CM: f64 = 0.001;
const TOL_ROT: f64 = 1e-5;
const TOL_PROJ: f64 = 1e-5;
/// The largest summed amplitude on one axis of the build's camera shakes, cm.
const SHAKE_MAX: f64 = 48.0 + 22.0 + 20.0 + 12.0 + 6.0 + 5.0;

/// (origin cm, rotation, projection relative) differences of two cameras.
fn diff(a: &Camera, b: &Camera) -> (f64, f64, f64) {
    let o = (0..3).map(|i| (a.origin[i] - b.origin[i]).abs()).fold(0.0, f64::max);
    let mut r: f64 = 0.0;
    let mut p: f64 = 0.0;
    for i in 0..4 {
        for j in 0..4 {
            r = r.max((a.translated_world_to_view[i][j] - b.translated_world_to_view[i][j]).abs());
            let (x, y) = (a.view_to_clip[i][j], b.view_to_clip[i][j]);
            p = p.max((x - y).abs() / x.abs().max(y.abs()).max(1e-3));
        }
    }
    (o, r, p)
}

#[test]
fn the_camera_matches_the_pose_tape() {
    let traces = mvs_sim_trace::run::default_traces();
    let tape_path =
        std::env::var_os("MVS_POSE_TAPE").map_or_else(|| traces.join("../tapes/lab-picture.tape"), PathBuf::from);
    let Ok(tape) = PoseTape::open(&tape_path) else {
        eprintln!("no pose tape at {}: skipped", tape_path.display());
        return;
    };
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("no dev store: skipped");
        return;
    };
    let trace_path = traces.join(format!("{}.trace", tape.header.run));
    let Ok(trace) = mvs_trace::Trace::read(&trace_path) else {
        eprintln!("no trace at {}: skipped", trace_path.display());
        return;
    };
    let (mut w, book) = mvs_sim_trace::world_for(&trace.header, &store).expect("the trace's World");
    let [sw, sh] = tape.header.size;
    let mut cam = MatchCamera::new(&w, f64::from(sw) / f64::from(sh));
    let (mut frames, mut bad, mut stepped, mut shaken) = (0, 0, 0usize, 0);
    let mut worst = (0.0f64, 0.0f64, 0.0f64);
    let mut first: Option<String> = None;
    for tick in tape.ticks() {
        let Some(tf) = tape.frame(tick).expect("the tape's frame") else { continue };
        // one step per trace frame (frame i is tick i + 1), the camera stepped after each as the oracle's presentTick
        while i64::from(w.flow.tick) < i64::from(tf.tick) && stepped < trace.len() {
            let fr = trace.frame(stepped).expect("the trace's frame");
            let inputs: Vec<_> = fr.inputs.iter().map(|r| mvs_sim_trace::input_of(r, &w, &book)).collect();
            w.step(&inputs);
            stepped += 1;
            cam.step(&w);
            w.events.clear();
        }
        assert_eq!(i64::from(w.flow.tick), i64::from(tf.tick), "the trace ends before the tape");
        let got = cam.camera(tf.alpha);
        // A shake's oscillator with a random initial offset cannot match the oracle's (Math.random): along each camera
        // axis such a shake plays on (forward X, up Z), both the camera's and the tape's own shake are taken out (the
        // tape's within the shakes' amplitude); the rest of the lens, a zero-offset oscillator's axis included, is
        // compared.
        let (rx, rz) = cam.rig.shaker.random_axes();
        let mut want = tf.camera.clone();
        let mut ours = got.clone();
        for (on, axis, own) in [(rx, cam.rig.pov.front, cam.rig.shaker.x), (rz, cam.rig.pov.up, cam.rig.shaker.z)] {
            if !on {
                continue;
            }
            let tape: f64 = (0..3).map(|i| (tf.camera.origin[i] - cam.rig.root[i]) * axis[i]).sum();
            assert!(tape.abs() <= SHAKE_MAX, "t{}: the tape's shake is {tape} cm", tf.tick);
            for (i, a) in axis.iter().enumerate() {
                want.origin[i] -= tape * a;
                ours.origin[i] -= own * a;
            }
        }
        shaken += usize::from(rx || rz);
        let (o, r, p) = diff(&ours, &want);
        worst = (worst.0.max(o), worst.1.max(r), worst.2.max(p));
        frames += 1;
        if o > TOL_CM || r > TOL_ROT || p > TOL_PROJ {
            bad += 1;
            first.get_or_insert_with(|| {
                format!(
                    "t{}: origin {:?} / tape {:?}, rotation {r:.3e}, projection {p:.3e}",
                    tf.tick, got.origin, tf.camera.origin
                )
            });
        }
    }
    if let Some(f) = &first {
        eprintln!("first: {f}");
    }
    eprintln!(
        "{frames} frames ({shaken} with a random shake taken out), {bad} with a difference; largest: origin {:.6} cm, rotation {:.3e}, projection {:.3e}",
        worst.0, worst.1, worst.2
    );
    assert_eq!(bad, 0, "frames whose camera differs from the tape");
}
