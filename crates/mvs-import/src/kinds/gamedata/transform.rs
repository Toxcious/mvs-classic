//! Transforms as UE composes them in double: rotators, quaternions, a component's transform over its parent's. A
//! component's world transform is an FTransform (loc, quat, scale), composed along the AttachParent chain as
//! USceneComponent::UpdateComponentToWorldWithParent does (EF 512).
//!
//! Rotation parent x child, translation parentRot(parentScale x childLoc) + parentLoc, scale the per-axis product (no
//! shear); with a negative scale on either side the matrix path (FTransform::MultiplyUsingMatrixWithScale 140c5f7c0 ->
//! 140885f60): the product of the scaled matrices, its scale removed, each axis signed by the desired scale's sign, the
//! rotation that matrix's, the scale the per-axis product. Doubles, the arithmetic in a fixed order (a sum adds
//! from 0 left to right).

use mvs_math::{js, libm};

use super::shape::fail;
use crate::kind::KindError;

pub type Vec3 = [f64; 3];
/// X, Y, Z, W.
pub type Quat = [f64; 4];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub loc: Vec3,
    pub q: Quat,
    pub scale: Vec3,
}

/// CPython's math.degrees: radToDeg * x.
const RAD_TO_DEG: f64 = 180.0 / std::f64::consts::PI;

/// `a * b`.
pub fn qmul(a: Quat, b: Quat) -> Quat {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// `v` rotated by `q`.
pub fn qrot(q: Quat, v: Vec3) -> Vec3 {
    let [x, y, z, w] = q;
    let tx = 2.0 * (y * v[2] - z * v[1]);
    let ty = 2.0 * (z * v[0] - x * v[2]);
    let tz = 2.0 * (x * v[1] - y * v[0]);
    [v[0] + w * tx + y * tz - z * ty, v[1] + w * ty + z * tx - x * tz, v[2] + w * tz + x * ty - y * tx]
}

/// FRotator(pitch, yaw, roll).Quaternion() (UE4, degrees).
pub fn rotator_quat(p: f64, y: f64, r: f64) -> Quat {
    let d = std::f64::consts::PI / 360.0;
    let (sp, cp) = (libm::sin(p * d), libm::cos(p * d));
    let (sy, cy) = (libm::sin(y * d), libm::cos(y * d));
    let (sr, cr) = (libm::sin(r * d), libm::cos(r * d));
    [
        cr * sp * sy - sr * cp * cy,
        -cr * sp * cy - sr * cp * sy,
        cr * cp * sy - sr * sp * cy,
        cr * cp * cy + sr * sp * sy,
    ]
}

/// FQuat::Rotator (degrees, 4.26): [pitch, yaw, roll].
pub fn quat_rotator(q: Quat) -> Vec3 {
    let [x, y, z, w] = q;
    let sp = z * x - w * y;
    let yaw = libm::atan2(2.0 * (w * z + x * y), 1.0 - 2.0 * (y * y + z * z)) * RAD_TO_DEG;
    if sp < -0.4999995 {
        return [-90.0, yaw, -yaw - 2.0 * (libm::atan2(x, w) * RAD_TO_DEG)];
    }
    if sp > 0.4999995 {
        return [90.0, yaw, yaw - 2.0 * (libm::atan2(x, w) * RAD_TO_DEG)];
    }
    let pitch = libm::asin(2.0 * sp) * RAD_TO_DEG;
    let roll = libm::atan2(-2.0 * (w * x + y * z), 1.0 - 2.0 * (x * x + y * y)) * RAD_TO_DEG;
    [pitch, yaw, roll]
}

/// Whether `q` is no rotation within `tol` (1e-7 by default).
pub fn identity_rot(q: Quat, tol: f64) -> bool {
    q[0].abs() < tol && q[1].abs() < tol && q[2].abs() < tol
}

/// The unit vector of axis `j`.
fn axis(j: usize) -> Vec3 {
    let mut v = [0.0; 3];
    v[j] = 1.0;
    v
}

/// The FTransform as an affine matrix's linear part: column j the scaled axis j.
fn tf_matrix(t: &Xf) -> [[f64; 3]; 3] {
    let cols = [0, 1, 2].map(|j| qrot(t.q, axis(j)));
    [0, 1, 2].map(|i| [0, 1, 2].map(|j| cols[j][i] * t.scale[j]))
}

/// `v` through `t` (scale, rotate, translate).
pub fn tf_apply(t: &Xf, v: Vec3) -> Vec3 {
    let w = qrot(t.q, [t.scale[0] * v[0], t.scale[1] * v[1], t.scale[2] * v[2]]);
    [w[0] + t.loc[0], w[1] + t.loc[1], w[2] + t.loc[2]]
}

/// The rotation of an orthonormal right-handed matrix (`m[row][col]`, columns the axes) as a quaternion.
pub fn matrix_quat(m: &[[f64; 3]; 3]) -> Quat {
    let t = m[0][0] + m[1][1] + m[2][2];
    if t > 0.0 {
        let r = (t + 1.0).sqrt() * 2.0;
        return [(m[2][1] - m[1][2]) / r, (m[0][2] - m[2][0]) / r, (m[1][0] - m[0][1]) / r, r / 4.0];
    }
    // (Python's max over range(3) by key: the first of equal diagonals)
    let mut i = 0;
    for k in [1, 2] {
        if m[k][k] > m[i][i] {
            i = k;
        }
    }
    let (j, k) = ((i + 1) % 3, (i + 2) % 3);
    let r = (1.0 + m[i][i] - m[j][j] - m[k][k]).sqrt() * 2.0;
    let mut q = [0.0, 0.0, 0.0, (m[k][j] - m[j][k]) / r];
    q[i] = r / 4.0;
    q[j] = (m[j][i] + m[i][j]) / r;
    q[k] = (m[k][i] + m[i][k]) / r;
    q
}

/// The rotation `q` seen through the mirror of scale `s`'s signs (S R S, S = diag(sign s)): a body element's own
/// rotation under a mirrored component.
pub fn mirrored_quat(q: Quat, s: Vec3) -> Quat {
    let sg = s.map(|v| if v < 0.0 { -1.0 } else { 1.0 });
    let cols = [0, 1, 2].map(|j| qrot(q, axis(j)));
    matrix_quat(&[0, 1, 2].map(|i| [0, 1, 2].map(|j| sg[i] * cols[j][i] * sg[j])))
}

/// Math.min over three.
fn min3(v: Vec3) -> f64 {
    js::min(js::min(v[0], v[1]), v[2])
}

/// FTransform `b` (relative) under `a` (its parent); a degenerate or sheared chain under a mirrored parent fails.
pub fn compose(a: &Xf, b: &Xf) -> Result<Xf, KindError> {
    let sc = [a.scale[0] * b.scale[0], a.scale[1] * b.scale[1], a.scale[2] * b.scale[2]];
    let loc = tf_apply(a, b.loc);
    if min3(a.scale) >= 0.0 && min3(b.scale) >= 0.0 {
        return Ok(Xf { loc, q: qmul(a.q, b.q), scale: sc });
    }
    let (aa, ba) = (tf_matrix(a), tf_matrix(b));
    let sum3 = |f: &dyn Fn(usize) -> f64| 0.0 + f(0) + f(1) + f(2);
    let m = [0, 1, 2].map(|i| [0, 1, 2].map(|j| sum3(&|k| aa[i][k] * ba[k][j])));
    // (`** 2`: V8's pow, x * x for an exponent of 2)
    let ln = [0, 1, 2].map(|j| sum3(&|i| m[i][j] * m[i][j]).sqrt());
    // (NaN fails, as `!(x > y)` does)
    if min3(ln).is_nan() || min3(ln) <= 1e-8 {
        return fail("stage data: a degenerate attach chain");
    }
    let u = [0, 1, 2].map(|i| [0, 1, 2].map(|j| (m[i][j] / ln[j]) * if sc[j] < 0.0 { -1.0 } else { 1.0 }));
    // (a rotated child under a mirrored parent of unequal scales shears: the engine's matrix would not be a rotation)
    for j in 0..3 {
        let k = (j + 1) % 3;
        let d = sum3(&|i| u[i][j] * u[i][k]).abs();
        if d.is_nan() || d >= 1e-4 {
            return fail("stage data: a sheared attach chain");
        }
    }
    let det = u[0][0] * (u[1][1] * u[2][2] - u[1][2] * u[2][1]) - u[0][1] * (u[1][0] * u[2][2] - u[1][2] * u[2][0])
        + u[0][2] * (u[1][0] * u[2][1] - u[1][1] * u[2][0]);
    if det.is_nan() || det <= 0.0 {
        return fail("stage data: a mirrored attach chain with no rotation");
    }
    Ok(Xf { loc, q: matrix_quat(&u), scale: sc })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations() {
        let q = rotator_quat(0.0, 90.0, 0.0);
        let v = qrot(q, [1.0, 0.0, 0.0]);
        assert!(v[0].abs() < 1e-12 && (v[1] - 1.0).abs() < 1e-12);
        let r = quat_rotator(rotator_quat(10.0, 20.0, 30.0));
        assert!((r[0] - 10.0).abs() < 1e-9 && (r[1] - 20.0).abs() < 1e-9 && (r[2] - 30.0).abs() < 1e-9);
        assert!(identity_rot(rotator_quat(0.0, 0.0, 0.0), 1e-7));
        let m = matrix_quat(&[[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]]);
        assert_eq!(m, [1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn composed() {
        let id = Xf { loc: [0.0; 3], q: [0.0, 0.0, 0.0, 1.0], scale: [1.0; 3] };
        let a = Xf { loc: [1.0, 2.0, 3.0], q: rotator_quat(0.0, 90.0, 0.0), scale: [2.0, 2.0, 2.0] };
        let c = compose(&a, &Xf { loc: [1.0, 0.0, 0.0], ..id }).unwrap();
        assert!((c.loc[0] - 1.0).abs() < 1e-12 && (c.loc[1] - 4.0).abs() < 1e-12);
        let mirrored = Xf { scale: [-1.0, 1.0, 1.0], ..id };
        let c = compose(&mirrored, &Xf { loc: [1.0, 0.0, 0.0], ..id }).unwrap();
        assert_eq!(c.loc, [-1.0, 0.0, 0.0]);
        assert_eq!(c.scale, [-1.0, 1.0, 1.0]);
        let flat = Xf { scale: [-1.0, 0.0, 1.0], ..id };
        assert!(compose(&flat, &id).is_err());
    }
}
