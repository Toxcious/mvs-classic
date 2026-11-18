//! Argument reduction of a float by pi/2 (Payne and Hanek): x = q pi/2 + r, |r| <= pi/4, r to about 2^-104
//! relative, for every finite float. The float's 24-bit mantissa times a 192-bit window of 2/pi's bits is exact in
//! integers; the bits of 2/pi the window skips only add multiples of 4 to x 2/pi.

use super::double_double::{Dd, PI_2};

/// 2/pi's fraction in 24-bit words (enough for the largest float's window).
pub const TWO_OVER_PI: [u32; 14] = [
    0xA2F983, 0x6E4E44, 0x1529FC, 0x2757D1, 0xF534DD, 0xC0DB62, 0x95993C, 0x439041, 0xFE5163, 0xABDEBB, 0xC561B7,
    0x246E3A, 0x424DD2, 0xE00649,
];

/// Bit `i` of 2/pi (bit 1 is the 0.5 place); 0 for i < 1.
fn bit(i: i32) -> u64 {
    if i < 1 {
        return 0;
    }
    let i = (i - 1) as usize;
    u64::from((TWO_OVER_PI[i / 24] >> (23 - i % 24)) & 1)
}

/// Bits `from .. from + 64` of 2/pi as an integer, the first one most significant.
fn bits64(from: i32) -> u64 {
    // A word-at-a-time read would be faster; this runs once per reduction of a large argument, after the sin / cos
    // kernels, and keeps the indexing plain.
    let mut w = 0u64;
    for k in 0..64 {
        w = (w << 1) | bit(from + k);
    }
    w
}

/// (q mod 4, r) with x = q pi/2 + r. |x| < pi/4 gives (0, x).
pub(crate) fn rem_pio2(x: f32) -> (u32, Dd) {
    let a = x.abs();
    if a < core::f32::consts::FRAC_PI_4 {
        return (0, Dd::from(f64::from(x)));
    }
    let bits = a.to_bits();
    let biased = (bits >> 23) as i32;
    // a = m 2^e, m an integer below 2^24 (a >= pi/4 is normal).
    let m = u64::from((bits & 0x7f_ffff) | 0x80_0000);
    let e = biased - 150;
    // W = bits e - 1 .. e + 190 of 2/pi; a 2/pi = m W 2^-190 (mod 4) up to 2^-166.
    let w = [bits64(e - 1), bits64(e + 63), bits64(e + 127)];
    // m * W, 216 bits, kept mod 2^192: three 64-bit limbs, most significant first.
    let mut limbs = [0u64; 3];
    let mut carry = 0u128;
    for k in (0..3).rev() {
        let p = u128::from(m) * u128::from(w[k]) + carry;
        limbs[k] = p as u64;
        carry = p >> 64;
    }
    // f = P 2^-190: the top two bits are the integer part (q mod 4), the rest the fraction.
    let mut q = (limbs[0] >> 62) as u32;
    let mut frac_hi = (u128::from(limbs[0] & ((1 << 62) - 1)) << 64) | u128::from(limbs[1]);
    let mut frac_lo = limbs[2];
    let mut negative = false;
    // A fraction of a half or more rounds q up and leaves 1 - frac below the point.
    if frac_hi >> 125 != 0 {
        q = (q + 1) & 3;
        negative = true;
        // 2^190 - (frac_hi : frac_lo), as two's complement over 190 bits
        let lo_neg = frac_lo.wrapping_neg();
        let borrow = u128::from(frac_lo != 0);
        frac_hi = ((1u128 << 126) - frac_hi - borrow) & ((1u128 << 126) - 1);
        frac_lo = lo_neg;
    }
    // frac = (frac_hi 2^64 + frac_lo) 2^-190, below 0.5. As a double-double: hi takes the leading 53 bits.
    let lead = frac_hi as f64;
    let rest = frac_hi as i128 - lead as i128;
    let hi_part = lead * f64::from_bits((1023 - 126) << 52);
    let lo_part =
        (rest as f64 * f64::from_bits((1023 - 126) << 52)) + frac_lo as f64 * f64::from_bits((1023 - 190) << 52);
    let f = Dd::new(hi_part, 0.0).add_f64(lo_part);
    let mut r = f.mul(PI_2);
    if negative {
        r = r.neg();
    }
    if x < 0.0 {
        q = (4 - q) & 3;
        r = r.neg();
    }
    (q, r)
}
