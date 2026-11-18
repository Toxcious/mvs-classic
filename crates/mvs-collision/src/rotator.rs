//! FRotator / FQuat conversions, as the build's float32 routines: FRotator::Quaternion,
//! FRotator::NormalizeAxis and FQuat::Rotator. f64 at the interface, float32 inside.

use mvs_math::f32::fmath;

pub type Quat = [f64; 4]; // X, Y, Z, W
pub type Rot = [f64; 3]; // pitch, yaw, roll (degrees)

/// FRotator::Quaternion (14136a6d0).
pub fn quat_of(p: f64, y: f64, r: f64) -> Quat {
    fmath::rotator_quaternion([narrow(p), narrow(y), narrow(r)]).map(f64::from)
}

/// FRotator::NormalizeAxis (142b2a8a0): into (-180, 180].
pub fn normalize_axis(a: f64) -> f64 {
    f64::from(fmath::normalize_axis(narrow(a)))
}

/// FQuat::Rotator (14136e550): pitch by FastAsin away from the poles, +-90 near them.
pub fn rotator_of(q: &Quat) -> Rot {
    fmath::quat_rotator(q.map(narrow)).map(f64::from)
}

#[inline]
#[allow(clippy::cast_possible_truncation, reason = "the narrowing to the build's float32 is the point")]
pub(crate) fn narrow(x: impl Into<f64>) -> f32 {
    let x: f64 = x.into();
    x as f32
}
