//! Correctly rounded float32 values of the CRT functions, independent of `mvs_math`: the double result of the
//! Rust standard library (the system's C runtime, within an ulp or two of a double) rounds to the right float
//! unless it lies within 2^-45 of a rounding boundary (the midpoint between two floats); then the value is computed
//! with 320 bits (astro-float) and compared with that midpoint. A value within 2^-280 of the midpoint is taken as
//! the midpoint itself (only powf has such exact cases; the others' values at float arguments are transcendental)
//! and rounds to the float with the even mantissa.

use astro_float::{BigFloat, Consts, RoundingMode};
use std::cell::RefCell;

const P: usize = 320;
// No rounding: a correctly rounded result would loop forever on the exact cases (powf's), and the comparison with the
// midpoint needs only the 2^-280 the precision gives.
const RM: RoundingMode = RoundingMode::None;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum F1 {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Exp,
    Log,
}

impl F1 {
    pub const ALL: [F1; 8] = [F1::Sin, F1::Cos, F1::Tan, F1::Asin, F1::Acos, F1::Atan, F1::Exp, F1::Log];

    pub fn name(self) -> &'static str {
        match self {
            F1::Sin => "sinf",
            F1::Cos => "cosf",
            F1::Tan => "tanf",
            F1::Asin => "asinf",
            F1::Acos => "acosf",
            F1::Atan => "atanf",
            F1::Exp => "expf",
            F1::Log => "logf",
        }
    }

    pub fn std(self, x: f64) -> f64 {
        match self {
            F1::Sin => x.sin(),
            F1::Cos => x.cos(),
            F1::Tan => x.tan(),
            F1::Asin => x.asin(),
            F1::Acos => x.acos(),
            F1::Atan => x.atan(),
            F1::Exp => x.exp(),
            F1::Log => x.ln(),
        }
    }

    fn exact(self, x: f64) -> BigFloat {
        with_consts(|cc| {
            // Trigonometric functions of large arguments lose the argument's exponent to the reduction.
            let p = P + 160;
            let b = BigFloat::from_f64(x, p);
            match self {
                F1::Sin => b.sin(p, RM, cc),
                F1::Cos => b.cos(p, RM, cc),
                F1::Tan => b.tan(p, RM, cc),
                F1::Asin => b.asin(p, RM, cc),
                F1::Acos => b.acos(p, RM, cc),
                F1::Atan => b.atan(p, RM, cc),
                F1::Exp => b.exp(p, RM, cc),
                F1::Log => b.ln(p, RM, cc),
            }
        })
    }
}

thread_local! {
    static CONSTS: RefCell<Consts> = RefCell::new(Consts::new().expect("astro-float constants"));
}

fn with_consts<T>(f: impl FnOnce(&mut Consts) -> T) -> T {
    CONSTS.with(|c| f(&mut c.borrow_mut()))
}

/// A float as a point of the double line, the infinities at 2^128 (where the rounding boundary above the largest
/// float sits halfway to).
fn grid(f: f32) -> f64 {
    if f.is_infinite() { f64::from_bits((1023 + 128) << 52).copysign(f64::from(f)) } else { f64::from(f) }
}

/// The float nearest the real value approximated by `v` (within 2^-45 relative), with `exact` the value to `P` bits
/// for when `v` is too close to call.
pub fn round(v: f64, exact: impl FnOnce() -> BigFloat) -> f32 {
    if v.is_nan() || v == 0.0 || v.is_infinite() {
        return v as f32;
    }
    let r = v as f32;
    let (down, up) = (r.next_down(), r.next_up());
    let mid_lo = (grid(down) + grid(r)) / 2.0;
    let mid_hi = (grid(r) + grid(up)) / 2.0;
    let guard = v.abs() * f64::from_bits((1023 - 45) << 52);
    if (v - mid_lo).abs() > guard && (v - mid_hi).abs() > guard {
        return r;
    }
    let x = exact();
    let near = if (v - mid_lo).abs() < (v - mid_hi).abs() { mid_lo } else { mid_hi };
    let (below, above) = if near == mid_lo { (down, r) } else { (r, up) };
    let m = BigFloat::from_f64(near, P);
    let d = x.sub(&m, P, RM);
    let tie = d.is_zero() || d.exponent().unwrap() + 280 < m.exponent().unwrap();
    if tie {
        if below.to_bits() & 1 == 0 { below } else { above }
    } else if d.is_positive() {
        above
    } else {
        below
    }
}

pub fn f1(f: F1, x: f32) -> f32 {
    let x = f64::from(x);
    round(f.std(x), || f.exact(x))
}

/// atan2 of the C standard; its special cases (zeros, infinities) all come out of the double function exactly
/// rounded.
pub fn atan2f(y: f32, x: f32) -> f32 {
    let (y, x) = (f64::from(y), f64::from(x));
    round(y.atan2(x), || {
        with_consts(|cc| {
            let p = P + 64;
            let q = BigFloat::from_f64(y, p).div(&BigFloat::from_f64(x, p), p, RM);
            let a = q.atan(p, RM, cc);
            if x > 0.0 {
                a
            } else if y >= 0.0 {
                a.add(&cc.pi(p, RM), p, RM)
            } else {
                a.sub(&cc.pi(p, RM), p, RM)
            }
        })
    })
}

/// pow of the C standard. Its special cases come out of the double function exactly, as do overflow and underflow
/// far past the float range.
pub fn powf(x: f32, y: f32) -> f32 {
    let (xd, yd) = (f64::from(x), f64::from(y));
    round(xd.powf(yd), || {
        with_consts(|cc| {
            let p = P + 64;
            let v = BigFloat::from_f64(xd.abs(), p).pow(&BigFloat::from_f64(yd, p), p, RM, cc);
            let odd = yd.fract() == 0.0 && (yd / 2.0).fract() != 0.0;
            if xd < 0.0 && odd { v.neg() } else { v }
        })
    })
}

/// fmod is exact; the double remainder of two floats is that float.
pub fn fmodf(x: f32, y: f32) -> f32 {
    (f64::from(x) % f64::from(y)) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values() {
        assert_eq!(f1(F1::Sin, 0.0).to_bits(), 0);
        assert_eq!(f1(F1::Sin, -0.0).to_bits(), 0x8000_0000);
        assert_eq!(f1(F1::Exp, 1.0), core::f32::consts::E);
        assert_eq!(f1(F1::Atan, f32::INFINITY), core::f32::consts::FRAC_PI_2);
        assert_eq!(atan2f(0.0, -1.0), core::f32::consts::PI);
        assert!(f1(F1::Log, -1.0).is_nan());
        // (1 + 2^-12)^2 = 1 + 2^-11 + 2^-24 sits halfway between two floats: to the even one.
        assert_eq!(powf(1.0 + 1.0 / 4096.0, 2.0), 1.0 + 1.0 / 2048.0);
        assert_eq!(powf(1.0 + 3.0 / 4096.0, 2.0), f32::from_bits(0x3f80_3004));
    }

    #[test]
    fn hard_cases_take_the_exact_path() {
        // Values whose double result is a midpoint come from astro-float: the exact path agrees with the double one
        // on a sample of ordinary inputs, where both can be trusted.
        let mut s = 0x1234_5678_9abc_def0u64;
        for _ in 0..200 {
            let x = crate::float_in(&mut s, -10.0, 10.0);
            for f in [F1::Sin, F1::Exp, F1::Atan] {
                let exact = f.exact(f64::from(x));
                let e = round(f64::NAN, || exact.clone());
                assert!(e.is_nan());
                let forced = force(f.std(f64::from(x)), exact);
                assert_eq!(forced.to_bits(), f1(f, x).to_bits(), "{} {x}", f.name());
            }
        }
    }

    /// `round` with the double result moved onto the nearest midpoint, so the exact value decides.
    fn force(v: f64, exact: BigFloat) -> f32 {
        let r = v as f32;
        let (lo, hi) = ((grid(r.next_down()) + grid(r)) / 2.0, (grid(r) + grid(r.next_up())) / 2.0);
        let near = if (v - lo).abs() < (v - hi).abs() { lo } else { hi };
        round(near, || exact)
    }
}
