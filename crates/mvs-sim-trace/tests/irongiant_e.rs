//! Iron Giant's pose paths no trace match runs (`chars/iron_giant/pose.rs`): the pilot's tentacle pose and its aim
//! blend space under the shapes (TenticleOverride), the tentacle loop's clock, and the tentacle aim read from the
//! head socket and the pilot's reticle. The three matches never turn TenticleOverride on nor show the reticle.

use mvs_sim::chars::iron_giant::pose::anim_update;
use mvs_sim::chars::iron_giant::turret::AllyTurret;
use mvs_sim::chars::iron_giant::{giant, giant_w};
use mvs_sim::consts::DT;
use mvs_sim::fighter::anim::sampled_boxes;
use mvs_sim::fighter::set_tenticle_override;
use mvs_sim_trace::harness;

/// A frame of irongiant-shaggy with the Giant (fighter 0) standing, his GiantAnim made.
const AT: usize = 300;

#[test]
fn the_tentacle_loop_runs_and_moves_the_shapes() {
    let Some(mut h) = harness::at("irongiant-shaggy", AT) else { return };
    let f = h.fighter(0);
    assert!(giant(&h.w, f).giant_anim.is_some(), "the Giant's GiantAnim at frame {AT}");
    let off = sampled_boxes(&h.w, f).expect("the shapes without the override");
    set_tenticle_override(&mut h.w, f, true);
    let on = sampled_boxes(&h.w, f).expect("the shapes with the override");
    assert_eq!(off.len(), on.len());
    assert!(on.iter().all(|v| v.is_finite()), "a shape value is not finite");
    assert!(off.iter().zip(&on).any(|(a, b)| a != b), "the tentacle pose moves no shape");
    // SequencePlayer_8: the mesh tick's dilated step, after the montage update (pre false)
    let t0 = giant(&h.w, f).giant_anim.as_ref().unwrap().loop_t;
    anim_update(&mut h.w, f, 0.5, false);
    let t1 = giant(&h.w, f).giant_anim.as_ref().unwrap().loop_t;
    assert!(t1 > t0 && t1 - t0 <= DT * 0.5 * 4.0, "loop_t {t0} -> {t1}");
    // the loop wraps at its length: far past it comes back under it
    giant_w(&mut h.w, f).giant_anim.as_mut().unwrap().loop_t = 1000.0;
    anim_update(&mut h.w, f, 1.0, false);
    let t2 = giant(&h.w, f).giant_anim.as_ref().unwrap().loop_t;
    assert!((0.0..1000.0).contains(&t2), "loop_t after the wrap {t2}");
    // off again: the clock holds
    set_tenticle_override(&mut h.w, f, false);
    anim_update(&mut h.w, f, 1.0, false);
    assert_eq!(giant(&h.w, f).giant_anim.as_ref().unwrap().loop_t, t2);
}

#[test]
fn the_reticle_aims_the_tentacles() {
    let Some(mut h) = harness::at("irongiant-shaggy", AT) else { return };
    let f = h.fighter(0);
    let (x, y) = (h.w.fighter(f).body.x, h.w.fighter(f).body.y);
    // no reticle: the aim stays
    giant_w(&mut h.w, f).ally_turret = Some(AllyTurret::default());
    anim_update(&mut h.w, f, 1.0, true);
    let ga = giant(&h.w, f).giant_anim.as_ref().unwrap();
    assert_eq!((ga.aim_vy, ga.aim_vz, ga.aim_dist), (0.0, 0.0, 0.0));
    // a reticle far above him: the vector points up (Z), its length 1 with the head's depth in it, the distance
    // alpha at its cap
    let t = giant_w(&mut h.w, f).ally_turret.as_mut().unwrap();
    (t.reticle, t.aim_x, t.aim_y) = (true, x, y + 5000.0);
    anim_update(&mut h.w, f, 1.0, true);
    let ga = giant(&h.w, f).giant_anim.as_ref().unwrap();
    // the unit aim of a reticle straight above, tilted by the head's depth (float32)
    assert_eq!((ga.aim_vy, ga.aim_vz), (-0.014151471, 0.9996579));
    assert_eq!(ga.aim_dist, 1.0);
    // a reticle to his right (sim +x): UE Y = -x, so the vector's Y is negative; near: the alpha under 1
    let t = giant_w(&mut h.w, f).ally_turret.as_mut().unwrap();
    (t.aim_x, t.aim_y) = (x + 600.0, y + 400.0);
    anim_update(&mut h.w, f, 1.0, true);
    let ga = giant(&h.w, f).giant_anim.as_ref().unwrap();
    assert!(ga.aim_vy < 0.0, "aim_vy {}", ga.aim_vy);
    assert!(ga.aim_dist > 0.0 && ga.aim_dist < 1.0, "aim_dist {}", ga.aim_dist);
    // and the aim blend space under the shapes with the override on
    set_tenticle_override(&mut h.w, f, true);
    let aimed = sampled_boxes(&h.w, f).expect("the shapes");
    let ga = giant_w(&mut h.w, f).giant_anim.as_mut().unwrap();
    (ga.aim_vy, ga.aim_vz, ga.aim_dist) = (0.0, 0.0, 0.0);
    let centred = sampled_boxes(&h.w, f).expect("the shapes");
    assert!(aimed.iter().zip(&centred).any(|(a, b)| a != b), "the aim blend space moves no shape");
}

/// The pose's sum, its sum of squares and 14 of its values, as the oracle's probe wrote them.
fn pose_of(h: &harness::Harness, f: mvs_sim::fighter::FighterId) -> (usize, f64, f64, Vec<f64>) {
    let s = sampled_boxes(&h.w, f).expect("the shapes");
    let sum = s.iter().map(|&v| f64::from(v)).sum();
    let sq = s.iter().map(|&v| f64::from(v) * f64::from(v)).sum();
    let at = [0usize, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377];
    (s.len(), sum, sq, at.iter().filter(|&&i| i < s.len()).map(|&i| f64::from(s[i])).collect())
}

// a tolerance on purpose: the float32 sim against the oracle's double probe values
fn close(got: f64, want: f64, tol: f64, what: &str) {
    assert!((got - want).abs() <= tol, "{what}: {got} against the oracle's {want}");
}

/// The composed pose against the oracle (a probe of the oracle's pose code on a fresh Giant against Shaggy, 12 ticks
/// in): the tentacle loop's shapes, the clock's step and wrap, and the aim blend's shapes for
/// reticles above, to the right, to the left and near.
#[test]
fn the_pose_values_are_the_oracles() {
    use mvs_sim::setup::{MatchPlayer, MatchSetup};
    let player = |c: &str| MatchPlayer { character: Some(c.to_owned()), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![player("irongiant"), player("shaggy")], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    for _ in 0..12 {
        h.step(&[mvs_sim::InputState::default(); 2]);
    }
    let f = h.fighter(0);
    let pose = |h: &harness::Harness, tag: &str, sum: f64, sq: f64, vals: &[f64]| {
        let (n, s, q, v) = pose_of(h, f);
        assert_eq!(n, 390, "{tag}: the pose's length");
        close(s, sum, 0.05, &format!("{tag} sum"));
        close(q, sq, 2.0, &format!("{tag} sum of squares"));
        for (i, (g, w)) in v.iter().zip(vals).enumerate() {
            close(*g, *w, 2e-3, &format!("{tag} value {i}"));
        }
    };
    let v_off = [0.0, 0.0, 0.0, -238.47704, 1.0, 0.0, 1.0, 1.0, 2.94456, -453.0, 1.0, 135.44209, 1.0, -427.64981];
    let v_on = [0.0, 0.0, 0.0, -238.47704, 1.0, 0.0, 1.0, 1.0, 2.94456, -453.0, 1.0, 135.44208, 1.0, -427.64978];
    pose(&h, "off", -7.55403, 14509943.67548, &v_off);
    set_tenticle_override(&mut h.w, f, true);
    pose(&h, "on", -1196.77393, 12880943.29022, &v_on);
    let ga = |h: &harness::Harness| {
        let a = giant(&h.w, f).giant_anim.as_ref().unwrap();
        [a.loop_t, a.aim_vy, a.aim_vz, a.aim_dist].map(f64::from)
    };
    let want = |h: &harness::Harness, tag: &str, w: [f64; 4]| {
        for (i, (g, x)) in ga(h).iter().zip(w).enumerate() {
            close(*g, x, 2e-4, &format!("{tag} giant_anim field {i}"));
        }
    };
    want(&h, "init", [0.0; 4]);
    anim_update(&mut h.w, f, 0.5, false);
    want(&h, "upd1", [0.00833, 0.0, 0.0, 0.0]);
    pose(&h, "upd1", -1197.21892, 12880613.46754, &v_on);
    for _ in 0..3 {
        anim_update(&mut h.w, f, 1.0, false);
    }
    want(&h, "upd4", [0.05833, 0.0, 0.0, 0.0]);
    pose(&h, "upd4", -1200.13974, 12878626.12714, &v_on);
    giant_w(&mut h.w, f).giant_anim.as_mut().unwrap().loop_t = 1000.0;
    anim_update(&mut h.w, f, 1.0, false);
    want(&h, "wrap", [0.01667, 0.0, 0.0, 0.0]);
    pose(&h, "wrap", -1197.66392, 12880283.81135, &v_on);
    // the reticle: the head socket's world place, the vector's two axes and the distance alpha
    let (x, y) = (h.w.fighter(f).body.x, h.w.fighter(f).body.y);
    giant_w(&mut h.w, f).ally_turret = Some(AllyTurret::default());
    set_tenticle_override(&mut h.w, f, false);
    anim_update(&mut h.w, f, 1.0, true);
    want(&h, "noret", [0.01667, 0.0, 0.0, 0.0]);
    let aim = |h: &mut harness::Harness, ax: mvs_sim::Real, ay: mvs_sim::Real| {
        let t = giant_w(&mut h.w, f).ally_turret.as_mut().unwrap();
        (t.reticle, t.aim_x, t.aim_y) = (true, ax, ay);
        anim_update(&mut h.w, f, 1.0, true);
    };
    aim(&mut h, x, y + 5000.0);
    want(&h, "up", [0.01667, -0.00136, 0.99999, 1.0]);
    aim(&mut h, x + 600.0, y + 400.0);
    want(&h, "right", [0.01667, -0.92087, -0.3893, 0.50496]);
    set_tenticle_override(&mut h.w, f, true);
    pose(&h, "right", -1055.84355, 12838984.7983, &v_on);
    aim(&mut h, x - 900.0, y - 100.0);
    want(&h, "left", [0.01667, 0.76349, -0.64571, 0.85617]);
    pose(&h, "left", -1799.16338, 12762802.81539, &v_on);
    aim(&mut h, x + 100.0, y + 60.0);
    want(&h, "near", [0.01667, -0.17489, -0.98433, 0.30167]);
    pose(&h, "near", -1269.93339, 12786390.44721, &v_on);
}
