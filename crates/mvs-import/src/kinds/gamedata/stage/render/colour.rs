//! The colour arithmetic of the stage data's render part: `srgb_linear`, `byte_col`, `col` and `temperature_col`, in
//! double arithmetic.
//!
//! `pow`, `sin` and `cos` are taken correctly rounded (a JavaScript importer's own bigint arithmetic: V8's misround by
//! an ulp at some values). `sin` and `cos` are evaluated here in double-double arithmetic (about 106 bits) and rounded
//! once; the platform's `powf` stands for `pow`.
// Unverified: the platform's `powf` is taken to round correctly at every colour byte a stage's lights and atmosphere
// give (the whole-build compare of the stage entries is the check, not every byte value); `sin` and `cos` round
// wrongly only where the exact value lies within 2^-100 of a midpoint between two doubles.

use mvs_data::{Props, Value};

use super::super::context::{Vec3, f};

/// `x ** p` (correctly rounded, for a positive normal x).
pub fn pow_round(x: f64, p: f64) -> f64 {
    if p == 0.0 || x == 1.0 { 1.0 } else { x.powf(p) }
}

/// An unevaluated sum `hi + lo` of two doubles, `|lo| <= ulp(hi) / 2`.
#[derive(Clone, Copy)]
struct Dd(f64, f64);

/// `a + b` as the rounded sum and its exact error.
fn two_sum(a: f64, b: f64) -> Dd {
    let s = a + b;
    let v = s - a;
    Dd(s, (a - (s - v)) + (b - v))
}

/// `a * b` as the rounded product and its exact error.
fn two_prod(a: f64, b: f64) -> Dd {
    let p = a * b;
    Dd(p, a.mul_add(b, -p))
}

impl Dd {
    fn add(self, o: Dd) -> Dd {
        let s = two_sum(self.0, o.0);
        let t = two_sum(self.1, o.1);
        let u = two_sum(s.0, s.1 + t.0);
        two_sum(u.0, u.1 + t.1)
    }

    fn neg(self) -> Dd {
        Dd(-self.0, -self.1)
    }

    fn mul(self, o: Dd) -> Dd {
        let p = two_prod(self.0, o.0);
        two_sum(p.0, p.1 + (self.0 * o.1 + self.1 * o.0))
    }

    /// `self / k` for an integer-valued double `k`.
    fn div_int(self, k: f64) -> Dd {
        let q1 = self.0 / k;
        let r = self.add(two_prod(q1, k).neg());
        let q2 = r.0 / k;
        let r = r.add(two_prod(q2, k).neg());
        let q3 = r.0 / k;
        two_sum(q1, q2).add(Dd(q3, 0.0))
    }
}

/// pi / 2 as three doubles (their sum is pi / 2 to about 160 bits).
const HALF_PI: [f64; 3] = [std::f64::consts::FRAC_PI_2, 6.123233995736766e-17, -1.4973849048591698e-33];

/// `(sin x, cos x)` in double-double, for a finite `x` (the reduction is good to about 2^-100 for `|x|` under 1e5).
fn sin_cos(x: f64) -> (Dd, Dd) {
    // x = n pi/2 + r, |r| <= pi/4
    let n = (x / HALF_PI[0]).round();
    let mut r = Dd(x, 0.0);
    for p in HALF_PI {
        r = r.add(two_prod(n, p).neg());
    }
    let r2 = r.mul(r);
    let (mut sin, mut cos) = (r, Dd(1.0, 0.0));
    let (mut ts, mut tc) = (r, Dd(1.0, 0.0));
    for k in 1..=30 {
        let k2 = f64::from(2 * k);
        tc = tc.mul(r2).div_int(k2 * (k2 - 1.0)).neg();
        ts = ts.mul(r2).div_int(k2 * (k2 + 1.0)).neg();
        cos = cos.add(tc);
        sin = sin.add(ts);
    }
    // the quadrant: sin, cos, -sin, -cos of r for n = 0 .. 3 (mod 4)
    match (n.rem_euclid(4.0)) as u8 {
        0 => (sin, cos),
        1 => (cos, sin.neg()),
        2 => (sin.neg(), cos.neg()),
        _ => (cos.neg(), sin),
    }
}

/// sin(x), correctly rounded.
pub fn sin_round(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() { x.sin() } else { sin_cos(x).0.0 }
}

/// cos(x), correctly rounded.
pub fn cos_round(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() { x.cos() } else { sin_cos(x).1.0 }
}

/// A linear colour struct's R G B, or none (`col`: a member that is no number is `undefined`, which the JSON drops to
/// null: NaN here).
pub fn col(v: Option<&Value>) -> Option<Vec<f64>> {
    let Some(Value::Obj(s)) = v else { return None };
    Some(["R", "G", "B"].iter().map(|k| f(s, k).unwrap_or(f64::NAN)).collect())
}

/// An FColor's R G B over 255, or white.
pub fn byte_col(v: Option<&Value>) -> Vec<f64> {
    match v {
        Some(Value::Obj(s)) => ["R", "G", "B"].iter().map(|k| f(s, k).unwrap_or(f64::NAN) / 255.0).collect(),
        _ => vec![1.0, 1.0, 1.0],
    }
}

/// FLinearColor(FColor): the sRGB curve of an FColor's R G B (white where none).
pub fn srgb_linear(v: Option<&Value>) -> Vec<f64> {
    ["R", "G", "B"]
        .iter()
        .map(|k| {
            let c: &Props = match v {
                Some(Value::Obj(s)) => s,
                _ => return curve(255.0),
            };
            curve(f(c, k).unwrap_or(f64::NAN))
        })
        .collect()
}

fn curve(b: f64) -> f64 {
    let x = b / 255.0;
    if x <= 0.04045 { x / 12.92 } else { pow_round((x + 0.055) / 1.055, 2.4) }
}

/// FLinearColor::MakeFromColorTemperature (stock 4.26: the Planckian locus in CIE 1960 UCS, clamped to 1000..15000 K,
/// Y = 1, to linear BT.709): what a light with bUseTemperature multiplies its colour by. The exe's (141362fc0, float32)
/// clamps T and leaves a negative channel as it is; the light's proxy (1429554b0) multiplies it in with no clamp.
pub fn temperature_col(t0: f64) -> Vec3 {
    use mvs_math::js::{fround, max, min};
    let t = min(max(fround(t0), 1000.0), 15000.0);
    let u =
        (0.860117757 + 1.54118254e-4 * t + 1.28641212e-7 * t * t) / (1.0 + 8.42420235e-4 * t + 7.08145163e-7 * t * t);
    let v =
        (0.317398726 + 4.22806245e-5 * t + 4.20481691e-8 * t * t) / (1.0 - 2.89741816e-5 * t + 1.61456053e-7 * t * t);
    let x = (3.0 * u) / (2.0 * u - 8.0 * v + 4.0);
    let y = (2.0 * v) / (2.0 * u - 8.0 * v + 4.0);
    let z = 1.0 - x - y;
    let (xx, yy, zz) = (x / y, 1.0, z / y);
    [
        3.2404542 * xx - 1.5371385 * yy - 0.4985314 * zz,
        -0.969266 * xx + 1.8760108 * yy + 0.041556 * zz,
        0.0556434 * xx - 0.2040259 * yy + 1.0572252 * zz,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ulps(a: f64, b: f64) -> u64 {
        a.to_bits().abs_diff(b.to_bits())
    }

    #[test]
    fn sin_and_cos_round_correctly() {
        // the correctly rounded values (C's sin(M_PI / 6) and cos(M_PI / 3))
        assert_eq!(sin_round(std::f64::consts::PI / 6.0), 0.49999999999999994);
        assert_eq!(cos_round(std::f64::consts::PI / 3.0), 0.5000000000000001);
        assert_eq!(sin_round(std::f64::consts::FRAC_PI_2), 1.0);
        assert_eq!(cos_round(0.0), 1.0);
        // within an ulp of the platform's over the angles a light's rotation gives
        let mut x = -7.0;
        while x < 7.0 {
            assert!(ulps(sin_round(x), x.sin()) <= 1, "sin {x}");
            assert!(ulps(cos_round(x), x.cos()) <= 1, "cos {x}");
            x += 0.0137;
        }
    }

    #[test]
    fn white_is_one() {
        assert_eq!(srgb_linear(None), vec![1.0, 1.0, 1.0]);
        assert_eq!(byte_col(None), vec![1.0, 1.0, 1.0]);
    }
}
