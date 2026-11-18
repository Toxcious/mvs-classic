//! The sim's math on [`Real`] (float32): the only routines sim code calls.
//!
//! The flat names are JavaScript's number operations on float32 (`min`, `max`, `round`, `sign`, `to_int32`,
//! `to_uint32`): each widens exactly to double, applies JavaScript's rule and narrows exactly back (the result is one
//! of the inputs, an integer of the input's range or a sign). `narrow` and `real` are the one rounding of a double or
//! an integer to float32. `dbl` holds the same operations on a double (the build's clock, the collision's and the
//! data's doubles at their boundary). `ue` holds the build's own float32 routines (CRT functions, FMath, vectors,
//! rotators), which every transcendental of the sim calls.

use crate::Real;
use mvs_math::js;

#[inline]
fn w(x: Real) -> f64 {
    f64::from(x)
}

/// Rounds a double result to the sim's float32 (the one rounding of a widened routine).
#[allow(clippy::cast_possible_truncation, reason = "the one rounding of a double result to float32, as fround")]
#[inline]
pub fn narrow(x: f64) -> Real {
    x as Real
}

/// Math.min(a, b): NaN wins, -0 below +0.
#[inline]
pub fn min(a: Real, b: Real) -> Real {
    narrow(js::min(w(a), w(b)))
}

/// Math.max(a, b): NaN wins, +0 above -0.
#[inline]
pub fn max(a: Real, b: Real) -> Real {
    narrow(js::max(w(a), w(b)))
}

/// Math.round: ties toward +Infinity, -0 kept.
#[inline]
pub fn round(x: Real) -> Real {
    narrow(js::round(w(x)))
}

/// Math.sign.
#[inline]
pub fn sign(x: Real) -> Real {
    narrow(js::sign(w(x)))
}

/// ToInt32 (`x | 0`).
#[inline]
pub fn to_int32(x: Real) -> i32 {
    js::to_int32(w(x))
}

/// ToUint32 (`x >>> 0`).
#[inline]
pub fn to_uint32(x: Real) -> u32 {
    js::to_uint32(w(x))
}

/// An integer as a [`Real`]: exact up to 2^24 in magnitude, rounded to nearest above (a double holds it
/// exactly). The sim's integers that reach it (ticks, counts, ids, damage) stay far below 2^24.
#[allow(clippy::cast_precision_loss, reason = "exact below 2^24; the sim's counts and ticks stay below it")]
#[inline]
pub fn real<T: Into<f64>>(x: T) -> Real {
    narrow(x.into())
}

/// JavaScript's number operations on a double: the build's clock ([`crate::Clock`]) and the collision's and the
/// data's doubles at their boundary.
pub mod dbl {
    pub use mvs_math::js::{max, min, round, sign, to_int32, to_uint32};
    pub use mvs_math::libm;
}

/// The build's own float32 routines (`mvs_math::f32`), bit for bit: the CRT functions a
/// Blueprint math node calls, FMath's routines, the InvSqrt estimate and the FVector / FRotator / FQuat shapes. The
/// flat functions above are JavaScript's operations only.
/// Vectors are UE's [x, y, z], rotators [pitch, yaw, roll] in degrees, quaternions [x, y, z, w].
pub mod ue {
    pub use mvs_math::f32::crt::{acosf, asinf, atanf, cosf, expf, logf, powf, sinf, tanf};
    pub use mvs_math::f32::estimate::inv_sqrt;
    pub use mvs_math::f32::fmath::{
        atan2, finterp_constant_to, finterp_to, floor_to_int, fmod, normalize_axis, quat_rotator, rotate_angle_axis,
        rotator_quaternion, round_to_int, sin_cos, trunc_to_int, vector_rotation,
    };
    pub use mvs_math::f32::vector::{
        AdvanceResult, DEG_TO_RAD, KINDA_SMALL_NUMBER, RAD_TO_DEG, SMALL_NUMBER, advance_time, clamped_to_max_size,
        deg_acos, deg_asin, deg_atan2, deg_to_rad, dist, dist2d, is_exceeding_max_speed, matrix_rotator, normal,
        normalize, quat_from_axis_angle, quat_rotate_vector, rad_to_deg, rot_from_x, rot_from_xz, safe_normal, size,
        size2d,
    };

    /// A component's local X / Z vector (UE X forward, Z up: the sim's forward and up) turned by the rotator
    /// (pitch, 0, 0) in degrees, as the build composes a component transform: FRotator::Quaternion, then the
    /// quaternion turn. A positive pitch turns X toward +Z: (x, z) -> (x cos p - z sin p, x sin p + z cos p).
    pub fn turn_by_pitch(pitch_deg: f32, v: [f32; 2]) -> [f32; 2] {
        let r = quat_rotate_vector(rotator_quaternion([pitch_deg, 0.0, 0.0]), [v[0], 0.0, v[1]]);
        [r[0], r[2]]
    }

    pub use mvs_math::f32::vector::clamped_to_size;
    pub use mvs_math::f32::vector::{direction_unit_vector, safe_normal2d, size_squared};

    #[cfg(test)]
    mod tests {
        use super::*;
        use mvs_math::f32 as m;

        #[test]
        fn re_exports_are_the_build_routines() {
            for x in [-3.5_f32, -1.0, -0.25, 0.0, 0.3, 1.0, 2.75, 1000.0] {
                assert_eq!(sinf(x).to_bits(), m::crt::sinf(x).to_bits());
                assert_eq!(cosf(x).to_bits(), m::crt::cosf(x).to_bits());
                assert_eq!(atan2(x, 0.7).to_bits(), m::fmath::atan2(x, 0.7).to_bits());
                assert_eq!(inv_sqrt(x.abs() + 0.5).to_bits(), m::estimate::inv_sqrt(x.abs() + 0.5).to_bits());
                assert_eq!(normalize_axis(x * 100.0).to_bits(), m::fmath::normalize_axis(x * 100.0).to_bits());
                assert_eq!(rad_to_deg(x).to_bits(), m::vector::rad_to_deg(x).to_bits());
            }
            let v = [3.0, -4.0, 12.0];
            assert_eq!(size(v).to_bits(), m::vector::size(v).to_bits());
            assert_eq!(safe_normal(v, SMALL_NUMBER), m::vector::safe_normal(v, m::vector::SMALL_NUMBER));
        }

        /// The quaternion route against the composition it stands for, on the sign convention of the port's
        /// `cos` / `sin` forms (actors/item.rs pickup_x / pickup_y, actors/projectile_move.rs mtd): a positive pitch
        /// turns forward toward up.
        #[test]
        fn turn_by_pitch_is_the_rotator_quaternion_turn() {
            for p in [-170.0_f32, -90.0, -33.3, 0.0, 12.5, 45.0, 90.0, 135.0, 400.0] {
                for v in [[1.0_f32, 0.0], [0.0, 1.0], [30.0, -12.5]] {
                    let q = m::fmath::rotator_quaternion([p, 0.0, 0.0]);
                    let r = m::vector::quat_rotate_vector(q, [v[0], 0.0, v[1]]);
                    assert_eq!(turn_by_pitch(p, v), [r[0], r[2]]);
                    let (s, c) = (m::crt::sinf(p * DEG_TO_RAD), m::crt::cosf(p * DEG_TO_RAD));
                    let t = turn_by_pitch(p, v);
                    // a tolerance on purpose: the quaternion turn against the sin / cos composition, two different computations
                    let tol = 1e-4 * (1.0 + v[0].abs() + v[1].abs());
                    assert!((t[0] - (v[0] * c - v[1] * s)).abs() < tol, "{p} {v:?} {t:?}");
                    assert!((t[1] - (v[0] * s + v[1] * c)).abs() < tol, "{p} {v:?} {t:?}");
                }
            }
        }
    }
}
