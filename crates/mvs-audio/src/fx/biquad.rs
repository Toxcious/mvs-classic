//! Biquads: the voices' low-pass / high-pass (the Wwise LPF / HPF value's cutoff law, a 2nd-order Butterworth) and
//! the filters the effects build on.
//!
//! The coefficient laws are those of the browser's `BiquadFilterNode` (Chromium's Biquad.cpp), in the node's
//! normalised frequency (cutoff / Nyquist).
// Unverified: Chromium's laws are written from its source as remembered; no sample of this module is compared
// with a recording.

use std::f64::consts::PI;

use crate::ctl::curves::{butterworth_q_db, eval_cutoff};
use crate::math::{cos, pow, sin};

/// A biquad's coefficients, normalised (a0 = 1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Coeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Coeffs {
    /// Passes the signal through.
    pub const PASS: Coeffs = Coeffs { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    fn normal(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Coeffs {
        let k = 1.0 / a0;
        Coeffs { b0: b0 * k, b1: b1 * k, b2: b2 * k, a1: a1 * k, a2: a2 * k }
    }

    fn gain(g: f64) -> Coeffs {
        Coeffs { b0: g, ..Coeffs::default() }
    }
}

/// A `BiquadFilterNode` type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    LowPass,
    HighPass,
    BandPass,
    Notch,
    LowShelf,
    HighShelf,
    Peaking,
}

/// The coefficients of `kind` at a normalised frequency (0..1 of Nyquist) with the node's Q (dB for the low-pass and
/// high-pass, linear for the band-pass, notch and peaking) and gain (dB, shelves and peaking).
pub fn coeffs(kind: Kind, freq: f64, q: f64, gain_db: f64) -> Coeffs {
    let freq = freq.clamp(0.0, 1.0);
    match kind {
        Kind::LowPass | Kind::HighPass => {
            let low = kind == Kind::LowPass;
            if freq == 1.0 || freq == 0.0 {
                // low-pass: 1 at full band, nothing at zero; high-pass the other way round
                return Coeffs::gain(if (freq == 1.0) == low { 1.0 } else { 0.0 });
            }
            let q = q.max(0.0);
            let g = pow(10.0, 0.05 * q);
            let d = ((4.0 - (16.0 - 16.0 / (g * g)).sqrt()) / 2.0).sqrt();
            let theta = PI * freq;
            let sn = 0.5 * d * sin(theta);
            let beta = 0.5 * (1.0 - sn) / (1.0 + sn);
            let gamma = (0.5 + beta) * cos(theta);
            let b0 = if low { 0.5 * (0.5 + beta - gamma) } else { 0.5 * (0.5 + beta + gamma) };
            let b1 = if low { 2.0 * b0 } else { -2.0 * b0 };
            Coeffs::normal(b0, b1, b0, 1.0, -2.0 * gamma, 2.0 * beta)
        }
        Kind::BandPass | Kind::Notch => {
            let band = kind == Kind::BandPass;
            if !(freq > 0.0 && freq < 1.0) {
                return Coeffs::gain(if band { 0.0 } else { 1.0 });
            }
            let q = q.max(0.0);
            if q <= 0.0 {
                // the limit of the z-transform as Q falls to zero
                return Coeffs::gain(if band { 1.0 } else { 0.0 });
            }
            let w0 = PI * freq;
            let alpha = sin(w0) / (2.0 * q);
            let k = cos(w0);
            if band {
                Coeffs::normal(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * k, 1.0 - alpha)
            } else {
                Coeffs::normal(1.0, -2.0 * k, 1.0, 1.0 + alpha, -2.0 * k, 1.0 - alpha)
            }
        }
        Kind::Peaking => {
            if !(freq > 0.0 && freq < 1.0) {
                return Coeffs::PASS;
            }
            let a = pow(10.0, gain_db / 40.0);
            let q = q.max(0.0);
            if q <= 0.0 {
                return Coeffs::gain(a * a);
            }
            let w0 = PI * freq;
            let alpha = sin(w0) / (2.0 * q);
            let k = cos(w0);
            Coeffs::normal(1.0 + alpha * a, -2.0 * k, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * k, 1.0 - alpha / a)
        }
        Kind::LowShelf | Kind::HighShelf => {
            let low = kind == Kind::LowShelf;
            let a = pow(10.0, gain_db / 40.0);
            if freq == 1.0 {
                return if low { Coeffs::gain(a * a) } else { Coeffs::PASS };
            }
            if freq <= 0.0 {
                return if low { Coeffs::PASS } else { Coeffs::gain(a * a) };
            }
            let w0 = PI * freq;
            // slope S = 1
            let alpha = 0.5 * sin(w0) * 2.0_f64.sqrt();
            let k = cos(w0);
            let k2 = 2.0 * a.sqrt() * alpha;
            let (ap, am) = (a + 1.0, a - 1.0);
            if low {
                Coeffs::normal(
                    a * (ap - am * k + k2),
                    2.0 * a * (am - ap * k),
                    a * (ap - am * k - k2),
                    ap + am * k + k2,
                    -2.0 * (am + ap * k),
                    ap + am * k - k2,
                )
            } else {
                Coeffs::normal(
                    a * (ap + am * k + k2),
                    -2.0 * a * (am + ap * k),
                    a * (ap + am * k - k2),
                    ap - am * k + k2,
                    2.0 * (am - ap * k),
                    ap - am * k - k2,
                )
            }
        }
    }
}

/// A biquad's state per channel: x1, x2, y1, y2.
#[derive(Clone, Debug, Default)]
pub struct Biquad {
    pub c: Coeffs,
    pub(crate) z: Vec<[f64; 4]>,
}

impl Biquad {
    pub fn new(channels: usize) -> Biquad {
        Biquad { c: Coeffs::PASS, z: vec![[0.0; 4]; channels] }
    }

    /// Filters channel `ch` of a quantum in place.
    pub fn run(&mut self, ch: usize, x: &mut [f32]) {
        let c = self.c;
        let [mut x1, mut x2, mut y1, mut y2] = self.z[ch];
        for s in x.iter_mut() {
            let xi = f64::from(*s);
            let y = c.b0 * xi + c.b1 * x1 + c.b2 * x2 - c.a1 * y1 - c.a2 * y2;
            x2 = x1;
            x1 = xi;
            y2 = y1;
            y1 = y;
            *s = y as f32;
        }
        self.z[ch] = [x1, x2, y1, y2];
    }
}

/// The node's parameter value: an AudioParam holds a float.
pub(crate) fn param(v: f64) -> f64 {
    f64::from(v as f32)
}

/// The voice's low-pass at a Wwise LPF value (0..100; 0 is off).
pub fn voice_lpf(value: f64, rate: u32) -> Coeffs {
    let nyquist = f64::from(rate) * 0.5;
    let cutoff = if value > 0.0 { eval_cutoff(value.min(100.0), f64::from(rate)) } else { nyquist };
    coeffs(Kind::LowPass, param(cutoff) / nyquist, param(butterworth_q_db()), 0.0)
}

/// The voice's high-pass at a Wwise HPF value (0..100; 0 is off).
pub fn voice_hpf(value: f64, rate: u32) -> Coeffs {
    let nyquist = f64::from(rate) * 0.5;
    let cutoff = if value > 0.0 { eval_cutoff(100.0 - value.min(100.0), f64::from(rate)) } else { 0.0 };
    coeffs(Kind::HighPass, param(cutoff) / nyquist, param(butterworth_q_db()), 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_passes() {
        let mut b = Biquad::new(1);
        b.c = voice_lpf(0.0, 48_000);
        assert_eq!(b.c, Coeffs::PASS);
        b.c = voice_hpf(0.0, 48_000);
        assert_eq!(b.c, Coeffs::PASS);
        let mut x = [0.5f32, -0.25, 1.0];
        b.run(0, &mut x);
        assert_eq!(x, [0.5, -0.25, 1.0]);
    }

    #[test]
    fn low_pass_has_unit_dc_gain_and_butterworth_corner() {
        let c = voice_lpf(50.0, 48_000);
        let dc = (c.b0 + c.b1 + c.b2) / (1.0 + c.a1 + c.a2);
        assert!((dc - 1.0).abs() < 1e-12, "{dc}");
        // |H| at the cutoff is 1/sqrt(2) for a Butterworth
        let fc = eval_cutoff(50.0, 48_000.0);
        let w = 2.0 * PI * fc / 48_000.0;
        let (re_n, im_n) = (c.b0 + c.b1 * w.cos() + c.b2 * (2.0 * w).cos(), -(c.b1 * w.sin() + c.b2 * (2.0 * w).sin()));
        let (re_d, im_d) = (1.0 + c.a1 * w.cos() + c.a2 * (2.0 * w).cos(), -(c.a1 * w.sin() + c.a2 * (2.0 * w).sin()));
        let h = ((re_n * re_n + im_n * im_n) / (re_d * re_d + im_d * im_d)).sqrt();
        assert!((h - std::f64::consts::FRAC_1_SQRT_2).abs() < 2e-3, "{h}");
    }

    #[test]
    fn peaking_at_zero_gain_is_identity() {
        let c = coeffs(Kind::Peaking, 0.25, 1.0, 0.0);
        assert!((c.b0 - 1.0).abs() < 1e-15 && (c.b1 - c.a1).abs() < 1e-15 && (c.b2 - c.a2).abs() < 1e-15);
    }
}
