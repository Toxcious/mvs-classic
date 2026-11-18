//! The SSE reciprocal estimates the exe's InvSqrt and packed reciprocals start from (rsqrtss / rsqrtps, rcpss /
//! rcpps), as AMD Zen 4 computes them, then the exe's Newton steps.
//!
//! Deviation: the build's estimate is the player's CPU's (Intel's and AMD's tables differ, and the two Newton steps
//! do not wash the difference out: about 16 % of InvSqrt results move); the port uses Zen 4's on every PC.

use super::estimate_tables::{RCP, RSQRT};

/// rsqrtss / rsqrtps on Zen 4: a table of the exponent's parity and the 12 leading mantissa bits. Zero and
/// subnormal inputs give infinity of their sign, negative ones the default NaN, +inf gives +0, a NaN comes back quiet.
pub fn rsqrt_estimate(x: f32) -> f32 {
    let bits = x.to_bits();
    let biased = (bits >> 23) & 0xff;
    if x.is_nan() {
        return f32::from_bits(bits | 0x40_0000);
    }
    if biased == 0 {
        return f32::from_bits((bits & 0x8000_0000) | 0x7f80_0000);
    }
    if bits >> 31 != 0 {
        return f32::from_bits(0xffc0_0000);
    }
    if biased == 0xff {
        return 0.0;
    }
    // x = 2^(2h + p) m, m in [1, 2): the table holds rsqrt(2^p m) in [0.5, 1); rsqrt(x) is that over 2^h.
    let d = biased as i32 - 127;
    let parity = d & 1;
    let h = (d - parity) / 2;
    let index = ((parity as u32) << 12) | ((bits >> 11) & 0xfff);
    let exponent = (126 - h) as u32;
    f32::from_bits((exponent << 23) | (u32::from(RSQRT[index as usize]) << 11))
}

/// rcpss / rcpps on Zen 4: a table of the 12 leading mantissa bits. Zero and subnormal inputs give infinity of
/// their sign, infinities zero of theirs, a result below the smallest normal float is flushed to zero, a NaN comes
/// back quiet.
pub fn rcp_estimate(x: f32) -> f32 {
    let bits = x.to_bits();
    let sign = bits & 0x8000_0000;
    let biased = (bits >> 23) & 0xff;
    if x.is_nan() {
        return f32::from_bits(bits | 0x40_0000);
    }
    if biased == 0 {
        return f32::from_bits(sign | 0x7f80_0000);
    }
    if biased == 0xff {
        return f32::from_bits(sign);
    }
    // x = 2^d m, m in [1, 2): the table holds 1/m in [0.5, 1); 1/x is that over 2^d.
    let exponent = 126 - (biased as i32 - 127);
    if exponent <= 0 {
        return f32::from_bits(sign);
    }
    f32::from_bits(sign | ((exponent as u32) << 23) | (u32::from(RCP[((bits >> 11) & 0xfff) as usize]) << 11))
}

/// FMath::InvSqrt, inline in the exe (GetSafeNormal at 142b1bc8c, the Normal node 142f7af25): the estimate, then
/// two Newton steps y = y + y (0.5 - (x / 2) y^2), in the exe's order.
pub fn inv_sqrt(x: f32) -> f32 {
    let y0 = rsqrt_estimate(x);
    let h = x * 0.5;
    let y1 = y0 + y0 * (0.5 - h * (y0 * y0));
    y1 + y1 * (0.5 - h * (y1 * y1))
}

/// VectorReciprocalSqrtAccurate, one lane (the FQuat normalise at 141370e98): the same two steps on rsqrtps, whose
/// lanes equal rsqrtss.
pub fn reciprocal_sqrt_accurate(x: f32) -> f32 {
    inv_sqrt(x)
}

/// VectorReciprocalAccurate, one lane (1428f0e6d): rcpps, then y = 2y - y^2 x twice.
pub fn reciprocal_accurate(x: f32) -> f32 {
    let y0 = rcp_estimate(x);
    let y1 = (y0 + y0) - (y0 * y0) * x;
    (y1 + y1) - (y1 * y1) * x
}
