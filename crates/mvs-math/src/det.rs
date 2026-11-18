//! The sim's transcendentals: the libm ports and the few the sim defines itself.

use crate::js;
pub use crate::libm::{atan, atan2, cos, pow, sin};

/// acos by atan2: exact enough for the angle tests that use it, and the same everywhere.
#[inline]
pub fn acos(x: f64) -> f64 {
    atan2(((1.0 - x) * (1.0 + x)).sqrt(), x)
}

/// sqrt(x^2 + y^2) without Math.hypot's scaling; sim lengths are far from overflow.
#[inline]
pub fn hypot(x: f64, y: f64) -> f64 {
    (x * x + y * y).sqrt()
}

/// FMath::FInterpConstantTo (UKismetMathLibrary::FInterpTo_Constant), in float32: no test of the speed (a speed 0
/// or below holds the value unless it is within sqrt(SMALL_NUMBER) of the target).
pub fn finterp_constant_to(cur: f64, target: f64, dt: f64, speed: f64) -> f64 {
    let dist = js::fround(target - cur);
    if js::fround(dist * dist) < 1e-8 {
        return target;
    }
    let step = js::fround(speed * dt);
    js::fround(cur + js::min(js::max(dist, -step), step))
}
