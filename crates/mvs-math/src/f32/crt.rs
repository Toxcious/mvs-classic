//! The C runtime's float functions the exe imports (sinf cosf tanf asinf acosf atanf atan2f powf expf logf fmodf),
//! correctly rounded (to nearest, ties to even) on every input. Each computes in f64 and keeps that result when its
//! error bound shows it rounds to one float; otherwise it recomputes in double-double (`slow`).
//!
//! Deviation: the build calls the player's Windows ucrtbase.dll, whose float functions are not correctly rounded and
//! pick an FMA3 or an SSE2 path by CPU; these differ from both on a small share of inputs (sinf about 0.07 %).

// fdlibm's constants as fdlibm writes them; `x - x` and `(x - x) / (x - x)` make the NaN the C functions return.
#![allow(clippy::excessive_precision, clippy::eq_op)]

use super::double_double::{self as dd, Dd};
use super::reduce::rem_pio2;
use crate::libm;

/// 2^-48: the fast paths' f64 results are within a few f64 ulps (fdlibm's bound is one).
const REL: f64 = 3.552_713_678_800_501e-15;
/// 2^-43, powf's bound: exp(y log x) with |y log x| <= 105 carries |y log x| 2^-51 from the log and the product.
const POW_REL: f64 = 1.136_868_377_216_16e-13;

/// `r` as a float when every value within `err` of it rounds to the same float.
#[inline]
fn settle(r: f64, err: f64) -> Option<f32> {
    let c = r as f32;
    if (r - err) as f32 == c && (r + err) as f32 == c { Some(c) } else { None }
}

#[inline]
fn fast(r: f64) -> Option<f32> {
    settle(r, r.abs() * REL)
}

pub fn sinf(x: f32) -> f32 {
    if !x.is_finite() {
        return x - x;
    }
    let (q, r) = rem_pio2(x);
    let v = match q {
        0 => libm::ksin(r.hi, r.lo, 1),
        1 => libm::kcos(r.hi, r.lo),
        2 => -libm::ksin(r.hi, r.lo, 1),
        _ => -libm::kcos(r.hi, r.lo),
    };
    fast(v).unwrap_or_else(|| slow::sinf(x))
}

pub fn cosf(x: f32) -> f32 {
    if !x.is_finite() {
        return x - x;
    }
    let (q, r) = rem_pio2(x);
    let v = match q {
        0 => libm::kcos(r.hi, r.lo),
        1 => -libm::ksin(r.hi, r.lo, 1),
        2 => -libm::kcos(r.hi, r.lo),
        _ => libm::ksin(r.hi, r.lo, 1),
    };
    fast(v).unwrap_or_else(|| slow::cosf(x))
}

pub fn tanf(x: f32) -> f32 {
    if !x.is_finite() {
        return x - x;
    }
    let (q, r) = rem_pio2(x);
    // fdlibm's kernel: 1 gives tan r, -1 gives -1 / tan r
    let v = libm::ktan(r.hi, r.lo, 1 - ((q & 1) << 1) as i32);
    fast(v).unwrap_or_else(|| slow::tanf(x))
}

pub fn asinf(x: f32) -> f32 {
    let a = x.abs();
    if a.is_nan() || a > 1.0 {
        return (x - x) / (x - x);
    }
    if a == 1.0 {
        return dd::round_to_f32(if x > 0.0 { dd::PI_2 } else { dd::PI_2.neg() });
    }
    fast(libm::asin(f64::from(x))).unwrap_or_else(|| slow::asinf(x))
}

pub fn acosf(x: f32) -> f32 {
    if x.is_nan() || x.abs() > 1.0 {
        return (x - x) / (x - x);
    }
    if x == -1.0 {
        return dd::round_to_f32(dd::PI);
    }
    fast(libm::acos(f64::from(x))).unwrap_or_else(|| slow::acosf(x))
}

pub fn atanf(x: f32) -> f32 {
    if x.is_nan() {
        return x + x;
    }
    if x.is_infinite() {
        return dd::round_to_f32(if x > 0.0 { dd::PI_2 } else { dd::PI_2.neg() });
    }
    fast(libm::atan(f64::from(x))).unwrap_or_else(|| slow::atanf(x))
}

/// atan2f(y, x), the angle of (x, y).
pub fn atan2f(y: f32, x: f32) -> f32 {
    if x.is_nan() || y.is_nan() {
        return x + y;
    }
    let negative_x = x < 0.0 || (x == 0.0 && x.is_sign_negative());
    let signed = |v: Dd| dd::round_to_f32(if y.is_sign_negative() { v.neg() } else { v });
    if y == 0.0 {
        return if negative_x { signed(dd::PI) } else { y };
    }
    if x == 0.0 {
        return signed(dd::PI_2);
    }
    if y.is_infinite() {
        return match (x.is_infinite(), negative_x) {
            (true, false) => signed(dd::PI.scale(0.25)),
            (true, true) => signed(dd::PI.mul_f64(0.75)),
            _ => signed(dd::PI_2),
        };
    }
    if x.is_infinite() {
        return if negative_x {
            signed(dd::PI)
        } else if y > 0.0 {
            0.0
        } else {
            -0.0
        };
    }
    fast(libm::atan2(f64::from(y), f64::from(x))).unwrap_or_else(|| slow::atan2f(y, x))
}

pub fn expf(x: f32) -> f32 {
    if x.is_nan() {
        return x + x;
    }
    // e^89 is past the largest float's rounding edge (e^88.72); e^-110 is below half the smallest subnormal.
    if x > 89.0 {
        return f32::INFINITY;
    }
    if x < -110.0 {
        return 0.0;
    }
    fast(exp64(f64::from(x))).unwrap_or_else(|| slow::expf(x))
}

pub fn logf(x: f32) -> f32 {
    if x.is_nan() || x < 0.0 {
        return (x - x) / (x - x);
    }
    if x == 0.0 {
        return f32::NEG_INFINITY;
    }
    if x.is_infinite() {
        return x;
    }
    fast(libm::log(f64::from(x))).unwrap_or_else(|| slow::logf(x))
}

/// powf(x, y), with C99 Annex F's special cases.
pub fn powf(x: f32, y: f32) -> f32 {
    if y == 0.0 || x == 1.0 {
        return 1.0;
    }
    if x.is_nan() || y.is_nan() {
        return x + y;
    }
    let y_int = y.trunc() == y;
    // Floats of 2^24 and above are even.
    let y_odd = y_int && y.abs() < 16_777_216.0 && (y as i32) & 1 == 1;
    let ax = x.abs();
    if y.is_infinite() {
        return if ax == 1.0 {
            1.0
        } else if (ax < 1.0) == (y < 0.0) {
            f32::INFINITY
        } else {
            0.0
        };
    }
    let neg_result = x.is_sign_negative() && y_odd;
    let sign = |v: f32| if neg_result { -v } else { v };
    if ax == 0.0 || ax.is_infinite() {
        // 0^y and inf^y: one is the other's reciprocal
        return sign(if (ax == 0.0) == (y < 0.0) { f32::INFINITY } else { 0.0 });
    }
    if x < 0.0 && !y_int {
        return (x - x) / (x - x);
    }
    let z = f64::from(y) * libm::log(f64::from(ax));
    // Past e^89.5 the result overflows, below e^-104.5 it rounds to 0, for any error in z.
    if z > 89.5 {
        return sign(f32::INFINITY);
    }
    if z < -104.5 {
        return sign(0.0);
    }
    let v = exp64(z);
    sign(settle(v, v * POW_REL).unwrap_or_else(|| slow::pow_abs(ax, y)))
}

/// fmodf(x, y): x - n y with n = trunc(x / y), exact, by integer long division (the result always fits a float).
pub fn fmodf(x: f32, y: f32) -> f32 {
    let mut ux = x.to_bits();
    let mut uy = y.to_bits();
    let mut ex = ((ux >> 23) & 0xff) as i32;
    let mut ey = ((uy >> 23) & 0xff) as i32;
    let sx = ux & 0x8000_0000;
    if uy << 1 == 0 || y.is_nan() || ex == 0xff {
        return (x * y) / (x * y);
    }
    if ux << 1 <= uy << 1 {
        return if ux << 1 == uy << 1 { 0.0 * x } else { x };
    }
    // Mantissas with the leading bit at bit 23 (subnormals shifted up, their exponent below 1).
    if ex == 0 {
        let mut i = ux << 9;
        while i >> 31 == 0 {
            ex -= 1;
            i <<= 1;
        }
        ux <<= (1 - ex) as u32;
    } else {
        ux = (ux & 0x7f_ffff) | 0x80_0000;
    }
    if ey == 0 {
        let mut i = uy << 9;
        while i >> 31 == 0 {
            ey -= 1;
            i <<= 1;
        }
        uy <<= (1 - ey) as u32;
    } else {
        uy = (uy & 0x7f_ffff) | 0x80_0000;
    }
    while ex > ey {
        let i = ux.wrapping_sub(uy);
        if i >> 31 == 0 {
            if i == 0 {
                return 0.0 * x;
            }
            ux = i;
        }
        ux <<= 1;
        ex -= 1;
    }
    let i = ux.wrapping_sub(uy);
    if i >> 31 == 0 {
        if i == 0 {
            return 0.0 * x;
        }
        ux = i;
    }
    while ux >> 23 == 0 {
        ux <<= 1;
        ex -= 1;
    }
    if ex > 0 {
        ux = (ux - 0x80_0000) | ((ex as u32) << 23);
    } else {
        ux >>= (1 - ex) as u32;
    }
    f32::from_bits(ux | sx)
}

/// e^x in f64 for -746 < x < 710 (fdlibm's e_exp.c), within one f64 ulp.
fn exp64(x: f64) -> f64 {
    const LN2_HI: f64 = 6.931_471_803_691_238_164_90e-01;
    const LN2_LO: f64 = 1.908_214_929_270_587_700_02e-10;
    const INV_LN2: f64 = core::f64::consts::LOG2_E;
    const P1: f64 = 1.666_666_666_666_660_190_37e-01;
    const P2: f64 = -2.777_777_777_701_559_338_42e-03;
    const P3: f64 = 6.613_756_321_437_934_361_17e-05;
    const P4: f64 = -1.653_390_220_546_525_153_90e-06;
    const P5: f64 = 4.138_136_797_057_238_460_39e-08;
    // k = round(x / ln 2); x = k ln2 + r, |r| <= ln2 / 2 (k = 0 below that, where hi - lo is x itself)
    let k =
        if x.abs() <= 0.346_573_590_279_972_6 { 0 } else { (INV_LN2 * x + if x < 0.0 { -0.5 } else { 0.5 }) as i32 };
    let t = f64::from(k);
    let hi = x - t * LN2_HI;
    let lo = t * LN2_LO;
    let r = hi - lo;
    let rr = r * r;
    let c = r - rr * (P1 + rr * (P2 + rr * (P3 + rr * (P4 + rr * P5))));
    let y = 1.0 - ((lo - (r * c) / (2.0 - c)) - hi);
    let k1 = k / 2;
    y * pow2(k1) * pow2(k - k1)
}

/// 2^k for -1022 <= k <= 1023.
fn pow2(k: i32) -> f64 {
    f64::from_bits(((k + 1023) as u64) << 52)
}

/// The double-double paths, for inputs whose f64 result sits too near a rounding boundary. Public for the
/// exhaustive checks, which run them on many more inputs than the fast paths hand them.
#[doc(hidden)]
pub mod slow {
    use super::super::double_double::{self as dd, Dd};
    use super::super::reduce::rem_pio2;
    use super::pow2;

    pub fn sinf(x: f32) -> f32 {
        let (q, r) = rem_pio2(x);
        let (s, c) = dd::sin_cos_reduced(r);
        dd::round_to_f32(match q {
            0 => s,
            1 => c,
            2 => s.neg(),
            _ => c.neg(),
        })
    }

    pub fn cosf(x: f32) -> f32 {
        let (q, r) = rem_pio2(x);
        let (s, c) = dd::sin_cos_reduced(r);
        dd::round_to_f32(match q {
            0 => c,
            1 => s.neg(),
            2 => c.neg(),
            _ => s,
        })
    }

    pub fn tanf(x: f32) -> f32 {
        let (q, r) = rem_pio2(x);
        let (s, c) = dd::sin_cos_reduced(r);
        dd::round_to_f32(if q & 1 == 0 { s.div(c) } else { c.div(s).neg() })
    }

    /// |x| < 1.
    pub fn asinf(x: f32) -> f32 {
        dd::round_to_f32(dd::asin(f64::from(x)))
    }

    /// -1 < x <= 1.
    pub fn acosf(x: f32) -> f32 {
        dd::round_to_f32(dd::acos(f64::from(x)))
    }

    pub fn atanf(x: f32) -> f32 {
        dd::round_to_f32(dd::atan(Dd::from(f64::from(x))))
    }

    /// Finite non-zero x and y.
    pub fn atan2f(y: f32, x: f32) -> f32 {
        let q = Dd::from(f64::from(y.abs())).div(Dd::from(f64::from(x.abs())));
        let mut v = dd::atan(q);
        if x < 0.0 {
            v = dd::PI.sub(v);
        }
        dd::round_to_f32(if y < 0.0 { v.neg() } else { v })
    }

    pub fn expf(x: f32) -> f32 {
        dd::round_to_f32(dd::exp(Dd::from(f64::from(x))))
    }

    /// Finite x > 0.
    pub fn logf(x: f32) -> f32 {
        dd::round_to_f32(dd::ln(Dd::from(f64::from(x))))
    }

    /// |x|^y for finite x > 0 (not 1) and finite non-zero y.
    // Unverified: no inexact result is assumed to lie within 2^-90 relative of a midpoint between two floats (the
    // double-double margin); exact results are found by `pow_exact` instead. The pairs are too many to check all.
    pub fn pow_abs(ax: f32, y: f32) -> f32 {
        if let Some(v) = pow_exact(ax, y) {
            return v as f32;
        }
        dd::round_to_f32(dd::exp(Dd::from(f64::from(y)).mul(dd::ln(Dd::from(f64::from(ax))))))
    }

    /// x = m 2^e with m odd.
    fn odd_split(v: f32) -> (i64, i64) {
        let bits = v.to_bits();
        let biased = i64::from((bits >> 23) & 0xff);
        let mut m = i64::from(bits & 0x7f_ffff);
        let mut e = if biased == 0 { -149 } else { biased - 150 };
        if biased != 0 {
            m |= 0x80_0000;
        }
        let tz = m.trailing_zeros();
        m >>= tz;
        e += i64::from(tz);
        if v < 0.0 { (-m, e) } else { (m, e) }
    }

    /// x^y when it is a dyadic number of at most 53 bits in f64's range, else None (x > 0, x != 1, y finite and
    /// non-zero). x^(n / 2^k) is dyadic only when x is a perfect 2^k-th power and, for n < 0, a power of two.
    fn pow_exact(x: f32, y: f32) -> Option<f64> {
        let (mut mx, mut ex) = odd_split(x);
        let (my, ey) = odd_split(y);
        let n: i128 = if ey >= 0 {
            if ey > 100 {
                return None;
            }
            i128::from(my) << ey
        } else {
            // y = my / 2^k: take k square roots of x
            let k = -ey;
            if k > 16 {
                return None;
            }
            for _ in 0..k {
                if ex & 1 != 0 {
                    return None;
                }
                let s = (mx as f64).sqrt() as i64;
                if s * s != mx {
                    return None;
                }
                mx = s;
                ex /= 2;
            }
            i128::from(my)
        };
        if mx == 1 {
            let e = i128::from(ex) * n;
            return (-1000..=1000).contains(&e).then(|| scale2(1.0, e as i32));
        }
        if !(1..=64).contains(&n) {
            return None;
        }
        let mut p: i64 = 1;
        for _ in 0..n {
            p = p.checked_mul(mx).filter(|&p| p <= 1 << 53)?;
        }
        let e = i128::from(ex) * n;
        (-1000..=1000).contains(&e).then(|| scale2(p as f64, e as i32))
    }

    /// v 2^e, exact for |e| <= 1000 and a result in the normal range.
    fn scale2(v: f64, e: i32) -> f64 {
        let e1 = e / 2;
        v * pow2(e1) * pow2(e - e1)
    }
}
