//! The engine's rotator and quaternion conversions, shared by the pose code that runs
//! them: AnimNode_SpringBone and the Constraint / BoneDrivenController nodes (anim_nodes), and the characters' pose
//! rules that put a rotation through FQuat::Rotator and back. Quaternions are in the build's axes (X, Y, Z, W); angles
//! in degrees.

const R2D: f64 = 180.0 / std::f64::consts::PI;
const D2R: f64 = std::f64::consts::PI / 180.0;

/// FMath::FastAsin as FQuat::Rotator (14136e550) inlines it: the x term is subtracted (subss at 14136e71e, constant
/// 143dbd264) and the half pi of the last step is 1.570796305 (143dbd2cc), the polynomial's own constant.
pub fn fast_asin(v: f64) -> f64 {
    let x = v.abs();
    let omx = (1.0 - x).max(0.0);
    let mut r = ((((((-0.0012624911 * x + 0.0066700901) * x - 0.0170881256) * x + 0.030891881) * x - 0.0501743046)
        * x
        + 0.0889789874)
        * x
        - 0.2145988016)
        * x
        + 1.570796305;
    r *= omx.sqrt();
    if v >= 0.0 { 1.570796305 - r } else { r - 1.570796305 }
}

/// FRotator::NormalizeAxis: an angle in (-180, 180].
pub fn normalize_axis(a: f64) -> f64 {
    // (% takes the sign of the dividend, as in JavaScript)
    let mut a = a % 360.0;
    if a < 0.0 {
        a += 360.0;
    }
    if a > 180.0 { a - 360.0 } else { a }
}

/// FQuat::Euler (1413599c0: Rotator().Euler()) of q (UE X, Y, Z, W): (Roll, Pitch, Yaw) in degrees.
pub fn quat_euler(q: [f64; 4]) -> [f64; 3] {
    let [x, y, z, w] = q;
    let test = z * x - w * y;
    let yaw = (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z)) * R2D;
    let (pitch, roll);
    if test < -0.4999995 {
        pitch = -90.0;
        roll = normalize_axis(-yaw - 2.0 * x.atan2(w) * R2D);
    } else if test > 0.4999995 {
        pitch = 90.0;
        roll = normalize_axis(yaw - 2.0 * x.atan2(w) * R2D);
    } else {
        pitch = fast_asin(2.0 * test) * R2D;
        roll = (-2.0 * (w * x + y * z)).atan2(1.0 - 2.0 * (x * x + y * y)) * R2D;
    }
    [roll, pitch, yaw]
}

/// FRotator::Quaternion (stock 4.26) of (pitch, yaw, roll) in degrees.
pub fn rotator_quat(pitch: f64, yaw: f64, roll: f64) -> [f64; 4] {
    let p = (pitch % 360.0) * D2R * 0.5;
    let y = (yaw % 360.0) * D2R * 0.5;
    let r = (roll % 360.0) * D2R * 0.5;
    let (sp, cp, sy, cy, sr, cr) = (p.sin(), p.cos(), y.sin(), y.cos(), r.sin(), r.cos());
    [
        cr * sp * sy - sr * cp * cy,
        -cr * sp * cy - sr * cp * sy,
        cr * cp * sy - sr * sp * cy,
        cr * cp * cy + sr * sp * sy,
    ]
}

/// FQuat::MakeFromEuler (141363150: FRotator::MakeFromEuler(e).Quaternion()) of e (Roll, Pitch, Yaw).
pub fn euler_quat(e: [f64; 3]) -> [f64; 4] {
    rotator_quat(e[1], e[2], e[0])
}

/// FQuat::Rotator then FRotator::Quaternion on q.
pub fn through_rotator(q: [f64; 4]) -> [f64; 4] {
    euler_quat(quat_euler(q))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rotator_survives_the_round_trip() {
        let q = rotator_quat(30.0, -45.0, 10.0);
        let e = quat_euler(q);
        // FastAsin is good to about 1e-6 radians
        assert!((e[0] - 10.0).abs() < 1e-3 && (e[1] - 30.0).abs() < 1e-3 && (e[2] + 45.0).abs() < 1e-3);
        let b = through_rotator(q);
        assert!(q.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5));
    }

    #[test]
    fn axes_normalise() {
        assert_eq!(normalize_axis(190.0), -170.0);
        assert_eq!(normalize_axis(-190.0), 170.0);
        assert_eq!(normalize_axis(180.0), 180.0);
    }
}
