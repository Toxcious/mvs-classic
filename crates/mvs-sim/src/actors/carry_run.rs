//! The pure jump helpers of the runner actors that carry their owner along the ground:
//! JakeHorse_Actor3 (`chars::jake::horse`) and VelmaCarryRun_Actor
//! (`chars::velma::carry`). Both classes hold the same two functions, byte for byte but for the variable names
//! (`CalculateJumpStopTime`, `ComputeJumpHeight`). Every Blueprint float operation is a float32 one: each step is
//! rounded as the graph rounds it.

use crate::Real;
use crate::math::max;
use crate::math::ue;

/// CalculateJumpStopTime(JumpVelocity, MaxJumpHeight) @0-909,
/// with the class's JumpStopVelocityMultiplier (`stop_mult`) and the owner's BFDefaultGravityScale (`gravity_scale`:
/// OwningFighter.BFDefaultGravityScale x 9.8 x 100, g in cm/s^2):
///   u = JumpVelocity x JumpStopVelocityMultiplier; t0 = u / g; h0 = u x t0 + t0^2 x g / 2
///   result = (JumpVelocity - Sqrt(FMax(JumpVelocity^2 - (MaxJumpHeight - h0) x 2g, 0))) / g
/// The time after which the rise at JumpVelocity has climbed MaxJumpHeight less the climb a launch at the stop speed u
/// still makes (h0): the Delay the runner's jump waits before its NetImpulse to Z = u.
pub fn calculate_jump_stop_time(
    jump_velocity: Real,
    max_jump_height: Real,
    stop_mult: Real,
    gravity_scale: Real,
) -> Real {
    let sq = jump_velocity * jump_velocity;
    let u = jump_velocity * stop_mult;
    let g1 = (gravity_scale * 9.8) * 100.0;
    let g2 = (gravity_scale * 9.8) * 100.0;
    let t0 = u / g1;
    let two_g = g2 * 2.0;
    let ut = u * t0;
    let half = ((t0 * t0) * g1) / 2.0;
    let h0 = ut + half;
    let d = (max_jump_height - h0) * two_g;
    let root = max(sq - d, 0.0).sqrt();
    (jump_velocity - root) / g2
}

/// ComputeJumpHeight, an empty function in both classes (`return`
/// @0); nothing calls it.
pub fn compute_jump_height() {}

/// KismetMathLibrary ClampVectorSize(A, Min, Max) on the owner's
/// velocity in the play plane (UE X, the depth, is 0 for a fighter): `ue::clamped_to_size` on (0, x, y). The runners
/// pass Min = -GalopSpeed, under any length, so only Max acts. Returns the clamped (x, y).
pub fn clamp_vector_size(x: Real, y: Real, lo: Real, hi: Real) -> (Real, Real) {
    let c = ue::clamped_to_size([0.0, x, y], lo, hi);
    (c[1], c[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The horse's jump (JumpVelocity 1800, MaxJumpHeight 400 clamp, multiplier 0.3, gravity scale 3.5 as the
    /// fighters' BFDefaultGravityScale): the closed form, each step in float32.
    #[test]
    fn jump_stop_time() {
        let t = calculate_jump_stop_time(1800.0, 400.0, 0.3, 3.5);
        let g: Real = (3.5 * 9.8) * 100.0;
        let u: Real = 1800.0 * 0.3;
        let h0 = u * (u / g) + (u / g) * (u / g) * g / 2.0;
        let exact = (1800.0 - (1800.0_f64 * 1800.0 - f64::from((400.0 - h0) * 2.0 * g)).sqrt()) / f64::from(g);
        assert!((t - mvs_sim::math::narrow(exact)).abs() < 1e-5, "{t} {exact}");
        assert_eq!(t, t);
        // a height past the whole rise: the root of a negative is 0, the full rise time
        assert_eq!(calculate_jump_stop_time(100.0, 10000.0, 1.0, 1.0), (100.0 / (9.8 * 100.0)));
    }

    #[test]
    fn clamp_size() {
        assert_eq!(clamp_vector_size(0.0, 0.0, -10.0, 10.0), (0.0, 0.0));
        // V x (1 / 50) x 10: the reciprocal 0.02 rounds below, so not (6, 8)
        assert_eq!(clamp_vector_size(30.0, 40.0, -100.0, 10.0), (5.999_999_5, 7.999_999_5));
        assert_eq!(clamp_vector_size(3.0, 4.0, -100.0, 10.0), (3.0, 4.0));
        // under Max the velocity still goes through the reciprocal and back
        assert_eq!(clamp_vector_size(1.0, 1.0, -100.0, 10.0), (0.999_999_94, 0.999_999_94));
        compute_jump_height();
    }
}
