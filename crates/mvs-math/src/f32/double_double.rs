//! Double-double arithmetic (a value as the unevaluated sum hi + lo of two f64, about 106 bits) and the functions
//! the correctly rounded float routines fall back to when their f64 result sits too near a rounding boundary.
//! Products split by Dekker's method (no FMA), so the bits are the same on every platform.

use crate::libm;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Dd {
    pub hi: f64,
    pub lo: f64,
}

#[inline]
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

/// `two_sum` for |a| >= |b| (or a == 0).
#[inline]
fn fast_two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    (s, b - (s - a))
}

#[inline]
fn split(a: f64) -> (f64, f64) {
    // 2^27 + 1: the high half keeps 26 bits, the low half the rest.
    let t = 134_217_729.0 * a;
    let hi = t - (t - a);
    (hi, a - hi)
}

#[inline]
fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    let (ah, al) = split(a);
    let (bh, bl) = split(b);
    (p, ((ah * bh - p) + ah * bl + al * bh) + al * bl)
}

impl Dd {
    pub const fn new(hi: f64, lo: f64) -> Dd {
        Dd { hi, lo }
    }

    pub const fn from(x: f64) -> Dd {
        Dd { hi: x, lo: 0.0 }
    }

    fn norm(hi: f64, lo: f64) -> Dd {
        let (h, l) = fast_two_sum(hi, lo);
        Dd { hi: h, lo: l }
    }

    pub fn neg(self) -> Dd {
        Dd { hi: -self.hi, lo: -self.lo }
    }

    pub fn add(self, b: Dd) -> Dd {
        let (s, e) = two_sum(self.hi, b.hi);
        let (t, f) = two_sum(self.lo, b.lo);
        let (s, e) = fast_two_sum(s, e + t);
        Dd::norm(s, e + f)
    }

    pub fn sub(self, b: Dd) -> Dd {
        self.add(b.neg())
    }

    pub fn add_f64(self, b: f64) -> Dd {
        let (s, e) = two_sum(self.hi, b);
        Dd::norm(s, e + self.lo)
    }

    pub fn mul(self, b: Dd) -> Dd {
        let (p, e) = two_prod(self.hi, b.hi);
        Dd::norm(p, e + (self.hi * b.lo + self.lo * b.hi))
    }

    pub fn mul_f64(self, b: f64) -> Dd {
        let (p, e) = two_prod(self.hi, b);
        Dd::norm(p, e + self.lo * b)
    }

    pub fn div(self, b: Dd) -> Dd {
        let q1 = self.hi / b.hi;
        let r = self.sub(b.mul_f64(q1));
        let q2 = r.hi / b.hi;
        let r = r.sub(b.mul_f64(q2));
        let q3 = r.hi / b.hi;
        Dd::norm(q1, q2).add_f64(q3)
    }

    pub fn div_f64(self, b: f64) -> Dd {
        self.div(Dd::from(b))
    }

    /// Exact for a power of two that keeps both parts normal.
    pub fn scale(self, s: f64) -> Dd {
        Dd { hi: self.hi * s, lo: self.lo * s }
    }

    pub fn sqrt(self) -> Dd {
        if self.hi <= 0.0 {
            return Dd::from(0.0);
        }
        let s = self.hi.sqrt();
        let (p, e) = two_prod(s, s);
        let r = self.sub(Dd::new(p, e));
        Dd::norm(s, r.hi / (2.0 * s))
    }
}

pub(crate) const PI: Dd = Dd::new(core::f64::consts::PI, 1.224_646_799_147_353_2e-16);
pub(crate) const PI_2: Dd = Dd::new(core::f64::consts::FRAC_PI_2, 6.123_233_995_736_766e-17);

/// e^z, relative error about 2^-100 for |z| < 750 (beyond it the f64 result overflows or flushes to zero).
pub(crate) fn exp(z: Dd) -> Dd {
    if z.hi > 710.0 {
        return Dd::from(f64::INFINITY);
    }
    if z.hi < -746.0 {
        return Dd::from(0.0);
    }
    // ln 2 in three parts; the first two hold 32 bits each, so k * part is exact for |k| < 2^21.
    const LN2_1: f64 = 0.693_147_180_369_123_8;
    const LN2_2: f64 = 1.908_214_928_843_070_3e-10;
    const LN2_3: f64 = 4.275_175_589_747_649e-20;
    const INV_LN2: f64 = core::f64::consts::LOG2_E;
    let k = (z.hi * INV_LN2 + 0.5).floor();
    let r = z.add_f64(-(k * LN2_1)).add_f64(-(k * LN2_2)).sub(Dd::from(k).mul_f64(LN2_3));
    // Taylor series to the 24th term: |r| <= 0.35, so the tail is below 2^-120.
    let mut p = Dd::from(1.0);
    for n in (1..=24).rev() {
        p = p.mul(r).div_f64(f64::from(n)).add_f64(1.0);
    }
    // 2^k in two steps, each a normal power of two.
    let k1 = (k * 0.5).floor();
    p.scale(pow2(k1)).scale(pow2(k - k1))
}

fn pow2(k: f64) -> f64 {
    // Only called with -540 < k < 540.
    let e = (k + 1023.0) as u64;
    f64::from_bits(e << 52)
}

/// ln x for x > 0: one Newton step on the f64 log, x e^-y - 1 being about 2^-52.
pub(crate) fn ln(x: Dd) -> Dd {
    let y = libm::log(x.hi);
    let t = x.mul(exp(Dd::from(-y))).add_f64(-1.0);
    // ln(1 + t) = t - t^2 / 2 + ...; t^3 is below 2^-150.
    Dd::from(y).add(t.sub(t.mul(t).scale(0.5)))
}

/// sin r and cos r for |r| <= pi/4 (a little beyond is fine), by Taylor series to the 31st / 32nd term.
pub(crate) fn sin_cos_reduced(r: Dd) -> (Dd, Dd) {
    let r2 = r.mul(r);
    let mut s = Dd::from(1.0);
    let mut n = 31.0;
    while n > 1.0 {
        // s = 1 - r^2 / ((n - 1) n) * s
        s = Dd::from(1.0).sub(s.mul(r2).div_f64((n - 1.0) * n));
        n -= 2.0;
    }
    let mut c = Dd::from(1.0);
    let mut n = 32.0;
    while n > 0.0 {
        c = Dd::from(1.0).sub(c.mul(r2).div_f64((n - 1.0) * n));
        n -= 2.0;
    }
    (s.mul(r), c)
}

/// atan q for any finite q.
pub(crate) fn atan(q: Dd) -> Dd {
    if q.hi.abs() > 1.0 {
        let base = PI_2.sub(atan_small(Dd::from(1.0).div(q.abs())));
        return if q.hi < 0.0 { base.neg() } else { base };
    }
    atan_small(q)
}

impl Dd {
    fn abs(self) -> Dd {
        if self.hi < 0.0 { self.neg() } else { self }
    }
}

/// atan q for |q| <= 1: one Newton-like step on the f64 atan, atan q = y + atan((q cos y - sin y) / (cos y + q sin y)).
fn atan_small(q: Dd) -> Dd {
    let y = libm::atan(q.hi);
    let (s, c) = sin_cos_reduced(Dd::from(y));
    let d = q.mul(c).sub(s).div(c.add(q.mul(s)));
    // d is about 2^-52: atan d = d - d^3 / 3 + ... and d^3 is below 2^-150.
    Dd::from(y).add(d)
}

/// asin x for |x| < 1.
pub(crate) fn asin(x: f64) -> Dd {
    // 1 - x^2 as (1 - x)(1 + x), both exact as double-doubles for a float x.
    let one_minus = Dd::from(1.0).add_f64(-x);
    let one_plus = Dd::from(1.0).add_f64(x);
    let c = one_minus.mul(one_plus).sqrt();
    atan(Dd::from(x).div(c))
}

/// acos x for -1 < x <= 1: 2 atan(sqrt((1 - x) / (1 + x))).
pub(crate) fn acos(x: f64) -> Dd {
    let one_minus = Dd::from(1.0).add_f64(-x);
    let one_plus = Dd::from(1.0).add_f64(x);
    atan(one_minus.div(one_plus).sqrt()).scale(2.0)
}

/// The float nearest to hi + lo, ties to even; |lo| is at most half an f64 ulp of hi.
pub(crate) fn round_to_f32(v: Dd) -> f32 {
    if v.hi.is_nan() {
        return f32::NAN;
    }
    let neg = v.hi < 0.0 || (v.hi == 0.0 && v.hi.is_sign_negative());
    let (a, lo) = if neg { (-v.hi, -v.lo) } else { (v.hi, v.lo) };
    let r = round_magnitude(a, lo);
    if neg { -r } else { r }
}

fn round_magnitude(a: f64, lo: f64) -> f32 {
    if a == f64::INFINITY {
        return f32::INFINITY;
    }
    let near = a as f32;
    // The float at or below a, and the one above it.
    let lower = if f64::from(near) <= a { near } else { f32::from_bits(near.to_bits() - 1) };
    if f64::from(lower) == a {
        return lower;
    }
    let upper = f32::from_bits(lower.to_bits() + 1);
    let ulp = if lower == f32::MAX {
        // the step to 2^128, where the next float would be
        f64::from_bits((1023 + 104) << 52)
    } else {
        f64::from(upper) - f64::from(lower)
    };
    let mid = f64::from(lower) + ulp * 0.5;
    if a < mid || (a == mid && lo < 0.0) {
        lower
    } else if a > mid || lo > 0.0 {
        upper
    } else if lower.to_bits() & 1 == 0 {
        lower
    } else {
        upper
    }
}
