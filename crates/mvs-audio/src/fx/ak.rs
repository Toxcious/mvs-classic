//! What AkStereoDelay.dll and AkRoomVerb.dll share: their biquad coefficients (DSP::ComputeBiquadCoefs),
//! a float delay line and the biquad's running state.

use std::f64::consts::{PI, SQRT_2};

use crate::math::{cos, sin, tan};

/// Math.fround.
pub(crate) fn fr(x: f64) -> f64 {
    f64::from(x as f32)
}

/// The float with the bits `u`, as a double.
pub(crate) fn fbits(u: u32) -> f64 {
    f64::from(f32::from_bits(u))
}

/// DSP::ComputeBiquadCoefs (AkRoomVerb.dll 1800011e0, same code in AkStereoDelay.dll): type 0 low shelf, 1 peaking,
/// 2 high shelf, 3 low pass, 4 high pass, 5 band pass, 6 notch (any other value: the notch's second branch). The
/// shelf / peak amplitude 10^(gain/40) comes from the SDK's polynomial power approximation, kept because it sets the
/// filter gain. Normalised: [b0, b1, b2, a1, a2] over a0.
pub(crate) fn biquad(kind: i32, sr: f64, f: f64, gain: f64, q: f64) -> [f64; 5] {
    let w = 2.0 * PI * f / sr;
    let (cs, sn) = (cos(w), sin(w));
    let mut a = 0.0;
    let x = fr(gain * 0.025);
    if x >= -37.0 {
        let u = u32_of(fr(fr(x * 27866352.0) + 1065353216.0));
        let m = fbits((u & 0x7fffff) + 0x3f800000);
        a = fr(((m * 0.3251898 + 0.020805772) * m + 0.653043) * fbits(u & 0xff800000));
    }
    let (b0, b1, b2, a0, a1, a2);
    if kind == 0 || kind == 2 {
        let beta = sn * a.sqrt() * SQRT_2;
        let s = if kind == 0 { 1.0 } else { -1.0 };
        b0 = a * ((a + 1.0) - s * (a - 1.0) * cs + beta);
        b1 = s * 2.0 * a * ((a - 1.0) - s * (a + 1.0) * cs);
        b2 = a * ((a + 1.0) - s * (a - 1.0) * cs - beta);
        a0 = (a + 1.0) + s * (a - 1.0) * cs + beta;
        a1 = -s * 2.0 * ((a - 1.0) + s * (a + 1.0) * cs);
        a2 = (a + 1.0) + s * (a - 1.0) * cs - beta;
    } else if kind == 1 {
        let al = sn * 0.5 / q;
        b0 = 1.0 + al * a;
        b1 = -2.0 * cs;
        b2 = 1.0 - al * a;
        a0 = 1.0 + al / a;
        a1 = -2.0 * cs;
        a2 = 1.0 - al / a;
    } else if kind == 3 || kind == 4 {
        let t = tan(PI * f / sr);
        let low = kind == 3;
        let k = if low { 1.0 / t } else { t };
        let n = 1.0 / (1.0 + SQRT_2 * k + k * k);
        a0 = 1.0;
        b0 = n;
        b1 = if low { 2.0 * n } else { -2.0 * n };
        b2 = n;
        a1 = (if low { 2.0 * (1.0 - k * k) } else { 2.0 * (k * k - 1.0) }) * n;
        a2 = (1.0 - SQRT_2 * k + k * k) * n;
    } else {
        let al = sn * 0.5 / q;
        if kind == 5 {
            b0 = al;
            b1 = 0.0;
            b2 = -al;
        } else {
            b0 = 1.0;
            b1 = -2.0 * cs;
            b2 = 1.0;
        }
        a0 = 1.0 + al;
        a1 = -2.0 * cs;
        a2 = 1.0 - al;
    }
    [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0]
}

/// `x >>> 0`.
pub(crate) fn u32_of(x: f64) -> u32 {
    mvs_math::js::to_uint32(x)
}

/// AkBq: a direct form I biquad in doubles.
#[derive(Clone, Debug)]
pub(crate) struct Bq {
    c: [f64; 5],
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Bq {
    pub(crate) fn new(c: [f64; 5]) -> Bq {
        Bq { c, x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 }
    }

    pub(crate) fn run(&mut self, x: f64) -> f64 {
        let c = &self.c;
        let y = c[0] * x + c[1] * self.x1 + c[2] * self.x2 - c[3] * self.y1 - c[4] * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub(crate) fn clear(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// AkLine: a float delay line of `n` samples.
#[derive(Clone, Debug)]
pub(crate) struct Line {
    pub(crate) b: Vec<f32>,
    pub(crate) p: usize,
}

impl Line {
    pub(crate) fn new(n: usize) -> Line {
        Line { b: vec![0.0; n], p: 0 }
    }

    pub(crate) fn run(&mut self, x: f64) -> f64 {
        let y = self.b[self.p];
        self.b[self.p] = x as f32;
        self.p += 1;
        if self.p >= self.b.len() {
            self.p = 0;
        }
        f64::from(y)
    }

    /// The sample the next `run` returns.
    pub(crate) fn head(&self) -> f64 {
        f64::from(self.b[self.p])
    }

    pub(crate) fn clear(&mut self) {
        self.b.fill(0.0);
    }
}

/// The worklets' check that the samples of `r` carry no signal.
pub(crate) fn silent(block: &crate::mix::Block, r: &std::ops::Range<usize>) -> bool {
    block.ch.iter().all(|c| c[r.clone()].iter().all(|&x| !(f64::from(x) > 1e-9 || f64::from(x) < -1e-9)))
}
