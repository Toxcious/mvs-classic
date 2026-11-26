//! (the port's own, dev) The 3D feel test's paths (`mvs_sim::world::depth3d`), which no trace runs: a fighter off
//! the attacker's depth is not hit by the hit a recorded match lands on it, and the plane's turn keeps each
//! fighter's floor point.

use mvs_sim::world::depth3d;
use mvs_sim_trace::harness;

const MATCH: &str = "arya-combat";
/// The ticks ahead of the hit the test starts at.
const AHEAD: usize = 4;

fn damage(h: &harness::Harness, n: usize) -> f32 {
    h.w.fighter(h.fighter(n)).stats.damage
}

/// The first frame from 60 on whose step raised fighter 1's damage.
fn hit_frame() -> Option<usize> {
    let mut h = harness::at_baseline(MATCH, 60)?;
    // (the match's first hit on fighter 1 comes within its first seconds)
    for _ in 0..1200 {
        let was = damage(&h, 1);
        h.step_recorded();
        if damage(&h, 1) > was {
            return Some(h.frame_index());
        }
    }
    panic!("{MATCH}: no hit on fighter 1 in 1200 frames from 60");
}

#[test]
fn a_fighter_off_the_attackers_depth_is_not_hit() {
    let Some(at) = hit_frame() else { return };
    let from = at - AHEAD;
    // the recorded match: the hit lands
    let mut h = harness::at_baseline(MATCH, from).expect("the trace");
    let was = damage(&h, 1);
    for _ in 0..AHEAD {
        h.step_recorded();
    }
    assert!(damage(&h, 1) > was, "the recorded hit lands");
    // the same with the test on and fighter 1 a step away in depth: it does not
    let mut h = harness::at_baseline(MATCH, from).expect("the trace");
    depth3d::enable(&mut h.w);
    let v = h.fighter(1);
    h.w.fighter_mut(v).body.d = 2.0 * depth3d::DEPTH_REACH;
    let was = damage(&h, 1);
    for _ in 0..AHEAD {
        h.step_recorded();
    }
    assert_eq!(damage(&h, 1), was, "a fighter out of reach in depth is not hit");
    assert!(h.w.depth3d.as_ref().expect("on").theta != 0.0, "the plane turned toward the fighters' line");
}

#[test]
fn the_turn_keeps_each_fighters_floor_point() {
    let Some(mut h) = harness::at_baseline(MATCH, 100) else { return };
    depth3d::enable(&mut h.w);
    let v = h.fighter(1);
    h.w.fighter_mut(v).body.d = 300.0;
    let s = h.w.depth3d.as_ref().expect("on");
    let (theta, origin) = (s.theta, s.origin);
    let at = |h: &harness::Harness, n: usize, theta: f32| {
        let b = &h.w.fighter(h.fighter(n)).body;
        depth3d::floor_point(theta, origin, b.x, b.d)
    };
    let before = [at(&h, 0, theta), at(&h, 1, theta)];
    // the turn alone: no inputs, the step's first part
    depth3d::tick(&mut h.w);
    let theta2 = h.w.depth3d.as_ref().expect("on").theta;
    assert!(theta2 != theta, "the plane turns toward the fighters' line");
    for (n, b) in before.iter().enumerate() {
        let a = at(&h, n, theta2);
        assert!((a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01, "fighter {n}: {b:?} -> {a:?}");
    }
}
