//! The float32 vector, quaternion and rotator routines the sim runs, in the exe's operation order: the shapes inline
//! in the exe (Size, GetSafeNormal, Normalize, GetClampedToMaxSize, IsExceedingMaxSpeed and the Blueprint vector
//! nodes) and the routines built on them (AdvanceTime, MakeRotFromX / MakeRotFromXZ, FMatrix::Rotator, FQuat(Axis,
//! Angle), the quaternion rotate, DegAcos / DegAsin). Plain f32 operations, never fused or reordered; each
//! comparison takes the branch the exe takes on a NaN (`!(a >= b)` where the exe jumps on "below"). Vectors are
//! [x, y, z], quaternions [x, y, z, w], rotators [pitch, yaw, roll] in degrees, a matrix its rows X, Y, Z.
//!
//! Sums of squares are always `(x x + y y) + z z`: the exe adds `y y + x x` in some places, the same bits.

// `!(a >= b)` is the exe's jump on "below or unordered": the NaN branch is the point.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::crt;
use super::estimate::inv_sqrt;
use super::fmath::{atan2, fmod, quat_rotator, sin_cos, vector_sin_cos};

type V3 = [f32; 3];

/// 1e-8 (SMALL_NUMBER): the tolerance of the C++ GetSafeNormal and of Normalize
pub const SMALL_NUMBER: f32 = f32::from_bits(0x322b_cc77);
/// 1e-4 (KINDA_SMALL_NUMBER)
pub const KINDA_SMALL_NUMBER: f32 = f32::from_bits(0x38d1_b717);
/// pi / 180
pub const DEG_TO_RAD: f32 = f32::from_bits(0x3c8e_fa35);
/// pi / 360: the half angle of FQuat(Axis, Angle) in one multiply
pub const DEG_TO_RAD_HALF: f32 = f32::from_bits(0x3c0e_fa35);
/// 180 / pi
pub const RAD_TO_DEG: f32 = f32::from_bits(0x4265_2ee0);
/// 1 / pi: FMatrix::Rotator multiplies by 180 and then by this
const INV_PI: f32 = f32::from_bits(0x3ea2_f983);
/// |X.z| from here up makes MakeFromX turn to the X axis as the up vector
const UP_SWITCH: f32 = f32::from_bits(0x3f7f_f972);
/// IsExceedingMaxSpeed's 1.01 (MaxSpeed squared times this)
const EXCEEDING_SCALE: f32 = f32::from_bits(0x3f81_47ae);

/// minss: the second operand unless the first is smaller (a NaN in either gives the second).
#[inline]
fn min_ss(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

/// degrees to radians, one multiply
pub fn deg_to_rad(degrees: f32) -> f32 {
    degrees * DEG_TO_RAD
}

/// radians to degrees, one multiply
pub fn rad_to_deg(radians: f32) -> f32 {
    radians * RAD_TO_DEG
}

/// (x x + y y) + z z
pub fn size_squared(v: V3) -> f32 {
    (v[0] * v[0] + v[1] * v[1]) + v[2] * v[2]
}

/// x x + y y
pub fn size_squared_2d(v: [f32; 2]) -> f32 {
    v[0] * v[0] + v[1] * v[1]
}

/// FVector::Size, VSize (142f851e0): sqrtss of the sum.
pub fn size(v: V3) -> f32 {
    size_squared(v).sqrt()
}

/// FVector::Size2D, VSize2D (142f850c0): sqrtss(x x + y y).
pub fn size2d(v: [f32; 2]) -> f32 {
    size_squared_2d(v).sqrt()
}

/// Vector_Distance (142f87ef0): the size of b - a.
pub fn dist(a: V3, b: V3) -> f32 {
    size([b[0] - a[0], b[1] - a[1], b[2] - a[2]])
}

/// Distance2D (142f87d50).
pub fn dist2d(a: [f32; 2], b: [f32; 2]) -> f32 {
    size2d([b[0] - a[0], b[1] - a[1]])
}

/// FVector::GetSafeNormal(tolerance), inline (the C++ callers pass `SMALL_NUMBER`): the vector itself when the sum of
/// squares is 1 or NaN, zero when it is below the tolerance, else the vector times `inv_sqrt(sum)`.
pub fn safe_normal(v: V3, tolerance: f32) -> V3 {
    let sum = size_squared(v);
    // ucomiss / jne: equal or unordered keeps v
    if sum == 1.0 || sum.is_nan() {
        return v;
    }
    if !(sum >= tolerance) {
        return [0.0; 3];
    }
    let s = inv_sqrt(sum);
    [v[0] * s, v[1] * s, v[2] * s]
}

/// The Normal node (142f7adf0): GetSafeNormal with the pin's tolerance (every decoded call passes 0.0001).
pub fn normal(v: V3, tolerance: f32) -> V3 {
    safe_normal(v, tolerance)
}

/// GetDirectionUnitVector (142f6db80): the safe normal of to - from, tolerance 1e-8.
pub fn direction_unit_vector(from: V3, to: V3) -> V3 {
    safe_normal([to[0] - from[0], to[1] - from[1], to[2] - from[2]], SMALL_NUMBER)
}

/// Normal2D (142f7acd0): zero unless the sum is above 1e-8, else times `inv_sqrt`; no `== 1` shortcut.
pub fn safe_normal2d(v: [f32; 2]) -> [f32; 2] {
    let sum = size_squared_2d(v);
    if !(sum > SMALL_NUMBER) {
        return [0.0; 2];
    }
    let s = inv_sqrt(sum);
    [v[0] * s, v[1] * s]
}

/// FVector::Normalize (MoveToForce): the vector times `inv_sqrt(sum)` and true when the sum is above 1e-8, else the
/// vector unchanged and false. No `== 1` shortcut.
pub fn normalize(v: V3) -> (V3, bool) {
    let sum = size_squared(v);
    if !(sum > SMALL_NUMBER) {
        return (v, false);
    }
    let s = inv_sqrt(sum);
    ([v[0] * s, v[1] * s, v[2] * s], true)
}

/// FVector::GetClampedToMaxSize(max), inline: zero when `!(max >= 1e-4)`, the vector when `!(sum > max max)`, else
/// the vector times `inv_sqrt(sum) * max` (the scale formed first).
pub fn clamped_to_max_size(v: V3, max: f32) -> V3 {
    if !(max >= KINDA_SMALL_NUMBER) {
        return [0.0; 3];
    }
    let sum = size_squared(v);
    if !(sum > max * max) {
        return v;
    }
    let s = inv_sqrt(sum) * max;
    [v[0] * s, v[1] * s, v[2] * s]
}

/// ClampVectorSize (142f66230, FVector::GetClampedToSize inline): `size = sqrtss(sum)`; the direction is zero unless
/// `size > 1e-8`, else the vector times `1 / size` (the reciprocal formed first); the size clamped as `size < min ->
/// min`, `size < max -> size`, else `max` (a NaN size takes `min`); the direction times the clamped size. Under `max`
/// the vector still goes through the reciprocal and back, so it is not returned unchanged.
pub fn clamped_to_size(v: V3, min: f32, max: f32) -> V3 {
    let size = size(v);
    let dir = if size > SMALL_NUMBER {
        let r = 1.0 / size;
        [v[0] * r, v[1] * r, v[2] * r]
    } else {
        [0.0; 3]
    };
    let c = if !(size >= min) {
        min
    } else if size < max {
        size
    } else {
        max
    };
    [dir[0] * c, dir[1] * c, dir[2] * c]
}

/// CharacterMovement IsExceedingMaxSpeed (142970eb0): `sum > maxss(max, 0)^2 * 1.01` (a NaN max counts as 0).
pub fn is_exceeding_max_speed(velocity: V3, max: f32) -> bool {
    let m = if max > 0.0 { max } else { 0.0 };
    size_squared(velocity) > (m * m) * EXCEEDING_SCALE
}

/// What FAnimationRuntime::AdvanceTime returns, as the exe's numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdvanceResult {
    /// the time stayed inside [0, end]
    Default = 0,
    /// not looping and past an end: clamped
    Finished = 1,
    /// looping and wrapped
    Looped = 2,
}

/// FAnimationRuntime::AdvanceTime (142816220): `time = move + time`; inside [0, end] it is kept; looping wraps it by
/// `fmath::fmod(time, end)` (zero when `end` is 0 or NaN) plus `end` when that is negative; else it is clamped to
/// `[0, end]`.
pub fn advance_time(looping: bool, move_by: f32, time: &mut f32, end: f32) -> AdvanceResult {
    let t = move_by + *time;
    *time = t;
    // comiss / jb: a NaN counts as below zero; comiss / ja: a NaN is not above end
    let below = t < 0.0 || t.is_nan();
    if !below && !(t > end) {
        return AdvanceResult::Default;
    }
    if looping {
        // ucomiss / je: zero and NaN both give 0
        if end == 0.0 || end.is_nan() {
            *time = 0.0;
            return AdvanceResult::Looped;
        }
        let mut wrapped = fmod(t, end);
        if !(wrapped >= 0.0) {
            wrapped += end;
        }
        *time = wrapped;
        AdvanceResult::Looped
    } else {
        *time = if below { 0.0 } else { min_ss(end, t) };
        AdvanceResult::Finished
    }
}

/// a x b
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// (a0 b0 + a1 b1) + a2 b2
fn dot(a: V3, b: V3) -> f32 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

/// The up vector MakeFromX and MakeFromXZ pick for a normalised X: the X axis when |X.z| >= 0.9999 (a NaN takes Z).
fn up_for(x: V3) -> V3 {
    if x[2].abs() >= UP_SWITCH { [1.0, 0.0, 0.0] } else { [0.0, 0.0, 1.0] }
}

/// FMatrix::MakeFromX (141363360), the rows X, Y, Z: X normalised, Y the safe normal of Up x X, Z = X x Y.
pub fn make_from_x(x: V3) -> [V3; 3] {
    let x = safe_normal(x, SMALL_NUMBER);
    let y = safe_normal(cross(up_for(x), x), SMALL_NUMBER);
    [x, y, cross(x, y)]
}

/// FMatrix::MakeFromXZ (141363b50): X and Z normalised, Z replaced by the up vector when `||X.Z| - 1| <= 1e-8`, Y the
/// safe normal of Z x X, Z = X x Y.
pub fn make_from_xz(x: V3, z: V3) -> [V3; 3] {
    let x = safe_normal(x, SMALL_NUMBER);
    let mut z = safe_normal(z, SMALL_NUMBER);
    let parallel = (dot(z, x).abs() - 1.0).abs();
    if !(parallel > SMALL_NUMBER) {
        z = up_for(x);
    }
    let y = safe_normal(cross(z, x), SMALL_NUMBER);
    [x, y, cross(x, y)]
}

/// The Y row of FRotationMatrix(rotator) (140da5540): the packed VectorSinCos of each angle times pi/180, no
/// wrapping, then `[(SR SP) CY - CR SY, CR CY + (SR SP) SY, -(CP SR)]`.
fn rotation_matrix_y_row(r: V3) -> V3 {
    let (sp, cp) = vector_sin_cos(r[0] * DEG_TO_RAD);
    let (sy, cy) = vector_sin_cos(r[1] * DEG_TO_RAD);
    let (sr, cr) = vector_sin_cos(r[2] * DEG_TO_RAD);
    let srsp = sr * sp;
    [srsp * cy - cr * sy, cr * cy + srsp * sy, -(cp * sr)]
}

/// Radians to degrees as FMatrix::Rotator does it: two multiplies, `* 180` and then `* (1 / pi)`.
fn matrix_degrees(radians: f32) -> f32 {
    (radians * 180.0) * INV_PI
}

/// FMatrix::Rotator (14136e3c0) of the rows X, Y, Z: Yaw = atan2(X.y, X.x), Pitch = atan2(X.z, sqrt(X.y^2 + X.x^2)),
/// Roll from the Y row of FRotationMatrix(Pitch, Yaw, 0): atan2(Z . SY, Y . SY).
pub fn matrix_rotator(m: [V3; 3]) -> V3 {
    let [x, y, z] = m;
    let yaw = matrix_degrees(atan2(x[1], x[0]));
    let pitch = matrix_degrees(atan2(x[2], (x[1] * x[1] + x[0] * x[0]).sqrt()));
    let sy = rotation_matrix_y_row([pitch, yaw, 0.0]);
    let roll = matrix_degrees(atan2(dot(z, sy), dot(y, sy)));
    [pitch, yaw, roll]
}

/// MakeRotFromX (142f74290).
pub fn rot_from_x(x: V3) -> V3 {
    matrix_rotator(make_from_x(x))
}

/// MakeRotFromXZ (142f74460).
pub fn rot_from_xz(x: V3, z: V3) -> V3 {
    matrix_rotator(make_from_xz(x, z))
}

/// FQuat(Axis, Angle in degrees) (142b2df90, inside RotatorFromAxisAndAngle): the axis safe-normalised (1e-8), the
/// half angle `angle * (pi / 360)` through `sin_cos`, the quaternion (axis sin, cos).
pub fn quat_from_axis_angle(axis: V3, angle_deg: f32) -> [f32; 4] {
    let a = safe_normal(axis, SMALL_NUMBER);
    let (s, c) = sin_cos(angle_deg * DEG_TO_RAD_HALF);
    [a[0] * s, a[1] * s, a[2] * s, c]
}

/// RotatorFromAxisAndAngle (142f815e0 -> 142b2df90): the rotator of `quat_from_axis_angle`.
pub fn rotator_from_axis_angle(axis: V3, angle_deg: f32) -> V3 {
    quat_rotator(quat_from_axis_angle(axis, angle_deg))
}

/// FQuat::RotateVector as the Quat_RotateVector node runs it (142f7e680, scalar): T = 2 (Q x V),
/// R = Q x T + (W T + V).
pub fn quat_rotate_vector(q: [f32; 4], v: V3) -> V3 {
    let [qx, qy, qz, qw] = q;
    let t = cross([qx, qy, qz], v);
    let t = [t[0] + t[0], t[1] + t[1], t[2] + t[2]];
    let c = cross([qx, qy, qz], t);
    [c[0] + (qw * t[0] + v[0]), c[1] + (qw * t[1] + v[1]), c[2] + (qw * t[2] + v[2])]
}

/// DegAcos (142f68610): the argument clamped (`!(a >= -1)` gives -1, else minss(a, 1)), the CRT acosf, one multiply.
pub fn deg_acos(a: f32) -> f32 {
    let c = if !(a >= -1.0) { -1.0 } else { min_ss(a, 1.0) };
    crt::acosf(c) * RAD_TO_DEG
}

/// DegAsin (142f686c0): as `deg_acos` with asinf.
pub fn deg_asin(a: f32) -> f32 {
    let c = if !(a >= -1.0) { -1.0 } else { min_ss(a, 1.0) };
    crt::asinf(c) * RAD_TO_DEG
}

/// DegAtan2 (142f68770): FMath::Atan2 times 180 / pi.
pub fn deg_atan2(y: f32, x: f32) -> f32 {
    atan2(y, x) * RAD_TO_DEG
}
