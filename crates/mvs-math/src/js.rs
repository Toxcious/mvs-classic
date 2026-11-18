//! JavaScript number operations with their own rules, for ported code: Math.min / max (NaN wins, -0 below +0),
//! Math.round (ties toward +Infinity, -0 kept), Math.fround, Math.sign, and the ToInt32 / ToUint32 of `| 0` and `>>>
//! 0`. Math.floor / ceil / trunc / abs / sqrt and `%` are IEEE-exact: `f64`'s own give the same bits.

/// Math.min(a, b).
#[inline]
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == b {
        // equal: only the zeros differ, and -0 is the smaller
        return if a.is_sign_negative() { a } else { b };
    }
    if a < b { a } else { b }
}

/// Math.max(a, b).
#[inline]
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == b {
        return if a.is_sign_negative() { b } else { a };
    }
    if a > b { a } else { b }
}

/// Math.round: the nearest integral value, ties toward +Infinity; -0 for -0.5 <= x < 0.
#[inline]
pub fn round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let r = x.floor();
    // (x - floor(x) is exact for every double)
    let v = if x - r >= 0.5 { r + 1.0 } else { r };
    if v == 0.0 && x.is_sign_negative() { -0.0 } else { v }
}

/// Math.fround.
#[inline]
pub fn fround(x: f64) -> f64 {
    x as f32 as f64
}

/// Math.sign.
#[inline]
pub fn sign(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 {
        x
    } else if x > 0.0 {
        1.0
    } else {
        -1.0
    }
}

const TWO32: f64 = 4294967296.0;

/// ToUint32 (`x >>> 0`): NaN and the infinities 0, else the integer part modulo 2^32.
#[inline]
pub fn to_uint32(x: f64) -> u32 {
    if !x.is_finite() {
        return 0;
    }
    let m = x.trunc() % TWO32;
    (if m < 0.0 { m + TWO32 } else { m }) as u32
}

/// ToInt32 (`x | 0`).
#[inline]
pub fn to_int32(x: f64) -> i32 {
    to_uint32(x) as i32
}
