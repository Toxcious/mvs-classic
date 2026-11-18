//! UGameplayStatics::Blueprint_PredictProjectilePath_ByTraceChannel with bTracePath false
//! (exec 142f32020 -> 142a73ea0 -> PredictProjectilePath 142a8dab0): the
//! path of a point launched at (vx, vy) from (sx, sy) under the gravity, with no trace. Its callers:
//! ProjectileAimer_SpitJake @9510 (`chars::jake::spit`), JakeEatAlly_Aim @4288 (`chars::jake::ally_spit`).
//!
//! Sim space (x = -UE Y, y = UE Z): the horizontal axis keeps its velocity, so the UE Y axis's rule holds for sim x
//! as it is (a negation is exact in float32).
//! - dt = 1.0f / SimFrequency; nothing (0 points) when SimFrequency <= 1e-4.
//! - PathData[0] = the start; then while t < MaxSimTime: step = min(MaxSimTime - t, dt), t += step, h = step x 0.5,
//!   x += h x (vx x 2), vz' = step x g + vz, z += (vz' + vz) x h, vz = vz'; each point appended. With SimFrequency
//!   15 and MaxSimTime 2: 31 points, the last at exactly t = 2.0 (the 30th step 0.0666659).
//! - Float32 throughout (each operation rounded, as the native code's float math).
//! - g = OverrideGravityZ unless |OverrideGravityZ| <= 1e-8, then the world's GetGravityZ (142eadfa0, not read).
//!
//! bTracePath false: no sweep, the return value of the build's call is false (not given here).

use crate::Real;
use crate::fighter::common::warn_text;
use crate::math::min;
use crate::world::World;

/// The points go into `out` flat (`[x0, y0, x1, y1, ...]`,
/// `out` grown as needed and never shortened); the return is the point count.
// Unverified: no caller of the port passes an override gravity of zero; one that does is warned and gets the override
// (0), not the world's gravity.
#[allow(clippy::too_many_arguments, reason = "the build's call's parameters")]
pub fn predict_projectile_path(
    w: &World,
    sx: Real,
    sy: Real,
    vx: Real,
    vy: Real,
    sim_frequency: Real,
    max_sim_time: Real,
    override_gravity_z: Real,
    out: &mut Vec<Real>,
) -> usize {
    if sim_frequency > (1e-4) && override_gravity_z.abs() <= (1e-8) {
        warn_text(w, "PredictProjectilePath: an override gravity of zero takes the world gravity, not ported");
    }
    path_points(sx, sy, vx, vy, sim_frequency, max_sim_time, override_gravity_z, out)
}

/// [`predict_projectile_path`] without its warning.
#[allow(clippy::too_many_arguments, reason = "the build's call's parameters")]
fn path_points(
    sx: Real,
    sy: Real,
    vx: Real,
    vy: Real,
    sim_frequency: Real,
    max_sim_time: Real,
    override_gravity_z: Real,
    out: &mut Vec<Real>,
) -> usize {
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the test is `!(a > b)`: a NaN frequency gives no point")]
    if !(sim_frequency > (1e-4)) {
        return 0;
    }
    let g = override_gravity_z;
    let dt = 1.0 / sim_frequency;
    let max_t = max_sim_time;
    let (mut x, mut y) = (sx, sy);
    let hv = vx * 2.0; // the horizontal velocity, doubled once (vx + vx)
    let mut vz = vy;
    let put = |out: &mut Vec<Real>, n: usize, x: Real, y: Real| {
        if out.len() < n * 2 + 2 {
            out.resize(n * 2 + 2, 0.0);
        }
        out[n * 2] = x;
        out[n * 2 + 1] = y;
    };
    put(out, 0, x, y);
    let mut n = 1;
    let mut t = 0.0;
    while t < max_t {
        let step = min(max_t - t, dt);
        t += step;
        let h = step * 0.5;
        x += h * hv;
        let vz2 = (step * g) + vz;
        y += (vz2 + vz) * h;
        vz = vz2;
        put(out, n, x, y);
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 31 points of the spits' call (SimFrequency 15, MaxSimTime 2), the last at exactly t = 2.0; `out` is
    /// written in place and never shortened.
    #[test]
    fn spit_path_points() {
        let mut out = vec![9.0; 70];
        let n = path_points(10.0, 20.0, -300.0, 900.0, 15.0, 2.0, -2450.0, &mut out);
        assert_eq!(n, 31);
        assert_eq!(out.len(), 70);
        assert_eq!((out[0], out[1]), (10.0, 20.0));
        // the float32 steps' sums at t = 2: x -> 10 - 300 t (-590 ideal), y -> 20 + 900 t - 1225 t^2 (-3080 ideal)
        assert_eq!((out[60], out[61]), (-589.9998, -3079.997));
        assert_eq!(out[62], 9.0);
        let mut short = Vec::new();
        assert_eq!(path_points(0.0, 0.0, 1.0, 1.0, 15.0, 2.0, -1.0, &mut short), 31);
        assert_eq!(short.len(), 62);
        assert_eq!(path_points(0.0, 0.0, 1.0, 1.0, 1e-4, 2.0, -1.0, &mut short), 0);
    }
}
