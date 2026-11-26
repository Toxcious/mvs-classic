//! (the port's own, dev) The 3D feel test's drawing (`mvs_sim::world::depth3d`): what the sim places on its 2D fight
//! plane (UE X = 0, sim x along UE -Y) is drawn on the turned plane, off it by its depth.
//!
//! A [`Turn`] is the rigid move of UE space that takes the 2D picture of a point on the plane (sim x `pivot`) to its
//! place on the floor: about UE Z by the plane's angle, then out along the plane's normal by the point's depth. The
//! floor's (a, b) of the sim are UE (Y = -a, X = b): a turn of the floor by theta is a turn about UE Z by theta, and
//! the plane's normal at theta 0 is UE +X (toward the camera). A fighter's turn takes its own place as the pivot, the
//! camera's and an actor's O.

use crate::camera::Pov;
use crate::math::Mat4;

/// UE axis i is three's AXIS[i] (math.rs).
const AXIS: [usize; 3] = [0, 2, 1];

/// A rigid move of UE space: q -> t + R (q - p), R the turn about UE Z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turn {
    cos: f64,
    sin: f64,
    /// The pivot's 2D place, UE cm.
    p: [f64; 3],
    /// The pivot's turned place, UE cm.
    t: [f64; 3],
}

impl Turn {
    /// The move of a point at sim x `pivot` with depth `d` on the plane at `theta` (radians) through O at sim x
    /// `origin`.
    pub fn of(theta: f64, origin: f64, pivot: f64, d: f64) -> Turn {
        let (sin, cos) = theta.sin_cos();
        let s = pivot - origin;
        let (a, b) = (origin + s * cos - d * sin, s * sin + d * cos);
        Turn { cos, sin, p: [0.0, -pivot, 0.0], t: [b, -a, 0.0] }
    }

    /// The UE turn's matrix entry (row r, column c).
    fn r(&self, r: usize, c: usize) -> f64 {
        match (r, c) {
            (0, 0) | (1, 1) => self.cos,
            (0, 1) => -self.sin,
            (1, 0) => self.sin,
            (2, 2) => 1.0,
            _ => 0.0,
        }
    }

    pub fn dir(&self, v: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|r| (0..3).map(|c| self.r(r, c) * v[c]).sum())
    }

    pub fn point(&self, q: [f64; 3]) -> [f64; 3] {
        let d = self.dir([0, 1, 2].map(|k| q[k] - self.p[k]));
        [0, 1, 2].map(|k| self.t[k] + d[k])
    }

    /// The move in three space (the views' model matrices: column-major, metres, three's axes).
    pub fn three(&self) -> Mat4 {
        let o = self.point([0.0; 3]);
        let mut m = crate::math::IDENTITY;
        for c in 0..3 {
            for r in 0..3 {
                m[c * 4 + r] = self.r(AXIS[r], AXIS[c]);
            }
            m[12 + c] = o[AXIS[c]] / 100.0;
        }
        m
    }

    /// The move put ahead of a component's LocalToWorld as FMatrix3x4 (rows: three linear entries and the
    /// translation, cm).
    pub fn rows(&self, m: &mut [f32; 12]) {
        let o = self.point([0.0; 3]);
        let old = m.map(f64::from);
        for r in 0..3 {
            for c in 0..4 {
                let v: f64 = (0..3).map(|k| self.r(r, k) * old[k * 4 + c]).sum();
                #[allow(clippy::cast_possible_truncation, reason = "a drawn matrix's entry")]
                {
                    m[r * 4 + c] = (v + if c == 3 { o[r] } else { 0.0 }) as f32;
                }
            }
        }
    }

    /// The move of a point of view.
    pub fn pov(&self, v: &Pov) -> Pov {
        Pov { pos: self.point(v.pos), right: self.dir(v.right), up: self.dir(v.up), front: self.dir(v.front) }
    }
}

/// The 3D test's turn of a fighter at sim x `x` with depth `d`; None with the test off.
pub fn fighter_turn(w: &mvs_sim::World, x: f32, d: f32) -> Option<Turn> {
    let s = w.depth3d.as_ref()?;
    Some(Turn::of(f64::from(s.theta), f64::from(s.origin), f64::from(x), f64::from(d)))
}

/// The 3D test's turn of what the sim places on the plane with no depth of its own (or `d`); None with it off.
pub fn plane_turn(w: &mvs_sim::World, d: f32) -> Option<Turn> {
    let s = w.depth3d.as_ref()?;
    Some(Turn::of(f64::from(s.theta), f64::from(s.origin), f64::from(s.origin), f64::from(d)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: [f64; 3], b: [f64; 3]) {
        for k in 0..3 {
            assert!((a[k] - b[k]).abs() < 1e-6, "{a:?} {b:?}");
        }
    }

    #[test]
    fn no_turn_and_no_depth_is_the_2d_picture() {
        let t = Turn::of(0.0, 30.0, 500.0, 0.0);
        near(t.point([10.0, -480.0, 90.0]), [10.0, -480.0, 90.0]);
        let m = t.three();
        for (k, v) in m.iter().enumerate() {
            assert!((v - crate::math::IDENTITY[k]).abs() < 1e-12);
        }
    }

    #[test]
    fn the_pivot_lands_on_its_floor_point() {
        // sim x 400 on a plane at a quarter turn through O at 0: floor (0, 400) -> UE X 400, Y 0; depth 50 along
        // n = (-1, 0) -> floor a -50 -> UE Y +50
        let t = Turn::of(std::f64::consts::FRAC_PI_2, 0.0, 400.0, 50.0);
        near(t.point([0.0, -400.0, 70.0]), [400.0, 50.0, 70.0]);
        // its forward (sim +x, UE -Y) turns to UE +X
        near(t.dir([0.0, -1.0, 0.0]), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn the_three_matrix_is_the_ue_move() {
        let t = Turn::of(0.7, -20.0, 300.0, -80.0);
        let q = [35.0, -250.0, 120.0];
        let m = t.three();
        let q3 = [q[0] / 100.0, q[2] / 100.0, q[1] / 100.0];
        let o3: [f64; 3] = [0, 1, 2].map(|r| (0..3).map(|c| m[c * 4 + r] * q3[c]).sum::<f64>() + m[12 + r]);
        let ue = t.point(q);
        near([o3[0] * 100.0, o3[2] * 100.0, o3[1] * 100.0], ue);
    }
}
