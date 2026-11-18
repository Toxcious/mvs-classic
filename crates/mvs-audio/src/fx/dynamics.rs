//! The compressor, the peak limiter and the meter (the plugin algorithms decoded from
//! AkSoundEngineDLL.dll, CAkCompressorFX, CAkPeakLimiterFX, CAkMeterFX, with exact log10 / pow in place of the SDK's
//! fast-math polynomials). The state is kept in doubles; the output is a float.

use mvs_data::sound::{CompressorParams, MeterParams, PeakLimiterParams};
use mvs_math::js;

use super::biquad::param;
use crate::QUANTUM;
use crate::math::{exp, log10, pow};
use crate::mix::Block;

/// CAkCompressorFX: RMS detector (mean power over the channels when linked), gain smoothed in dB.
pub struct Compressor {
    pub(crate) p: CompressorParams,
    rate: f64,
    rms: Vec<f64>,
    g: Vec<f64>,
    rms_c: f64,
    /// The worklet's AudioParams (floats): threshold, ratio, attack, release, output level in dB.
    prm: [f64; 5],
}

impl Compressor {
    pub(crate) fn new(p: &CompressorParams, rate: u32, channels: usize) -> Compressor {
        let rate = f64::from(rate);
        let out_db = 20.0 * log10(if p.output_level != 0.0 { p.output_level } else { 1.0 });
        Compressor {
            p: *p,
            rate,
            rms: vec![0.0; channels],
            g: vec![0.0; channels],
            rms_c: exp(-43.0663 / rate),
            prm: [param(p.threshold), param(p.ratio), param(p.attack), param(p.release), param(out_db)],
        }
    }

    pub(crate) fn base(&self, id: u16) -> Option<f64> {
        self.prm.get(usize::from(id)).copied()
    }

    pub(crate) fn set(&mut self, id: u16, value: f64) {
        if let Some(p) = self.prm.get_mut(usize::from(id)) {
            *p = param(value);
        }
    }

    pub(crate) fn process(&mut self, block: &mut Block) {
        let nc = block.channels();
        let [thr, ratio, attack, release, out_db] = self.prm;
        let k = (1.0 / js::max(ratio, 1.0) - 1.0) / 20.0;
        let lvl = pow(10.0, out_db / 20.0);
        let a_c = exp(-2.2 / (js::max(attack, 0.001) * self.rate));
        let r_c = exp(-2.2 / (js::max(release, 0.001) * self.rate));
        let rms_c = self.rms_c;
        if self.p.channel_link {
            let (mut rms, mut g) = (self.rms[0], self.g[0]);
            for i in 0..QUANTUM {
                let mut pw = 0.0;
                for c in 0..nc {
                    let x = f64::from(block.ch[c][i]);
                    pw += x * x;
                }
                pw = pw / nc as f64 + 1e-25;
                rms = (rms - pw) * rms_c + pw;
                let over = js::max(0.0, 10.0 * log10(rms) - thr);
                g = (g - over) * (if over - g >= 0.0 { a_c } else { r_c }) + over;
                let gain = pow(10.0, g * k) * lvl;
                for c in 0..nc {
                    block.ch[c][i] = (f64::from(block.ch[c][i]) * gain) as f32;
                }
            }
            self.rms[0] = rms;
            self.g[0] = g;
        } else {
            for c in 0..nc {
                let (mut rms, mut g) = (self.rms[c], self.g[c]);
                for s in block.ch[c].iter_mut() {
                    let v = f64::from(*s);
                    let pw = v * v + 1e-25;
                    rms = (rms - pw) * rms_c + pw;
                    let over = js::max(0.0, 10.0 * log10(rms) - thr);
                    g = (g - over) * (if over - g >= 0.0 { a_c } else { r_c }) + over;
                    *s = (v * pow(10.0, g * k) * lvl) as f32;
                }
                self.rms[c] = rms;
                self.g[c] = g;
            }
        }
    }
}

/// CAkPeakLimiterFX (channels linked): look-ahead delay line, peak held for the look-ahead time.
pub struct PeakLimiter {
    rate: f64,
    la: usize,
    buf: Vec<Vec<f32>>,
    pos: usize,
    a_c: f64,
    cur: f64,
    next: f64,
    timer: usize,
    over: f64,
    g: f64,
    /// threshold, ratio, release, output level in dB (floats).
    prm: [f64; 4],
}

impl PeakLimiter {
    pub(crate) fn new(p: &PeakLimiterParams, rate: u32, channels: usize) -> PeakLimiter {
        let rate = f64::from(rate);
        let la = js::max(1.0, (rate * p.look_ahead).floor()) as usize;
        let out_db = 20.0 * log10(if p.output_level != 0.0 { p.output_level } else { 1.0 });
        PeakLimiter {
            rate,
            la,
            buf: vec![vec![0.0; la]; channels],
            pos: 0,
            a_c: exp(-4.4 / la as f64),
            cur: 0.0,
            next: 0.0,
            timer: 0,
            over: 0.0,
            g: 0.0,
            prm: [param(p.threshold), param(p.ratio), param(p.release), param(out_db)],
        }
    }

    pub(crate) fn process(&mut self, block: &mut Block) {
        let nc = block.channels();
        let la = self.la;
        let [thr, ratio, release, out_db] = self.prm;
        let rf = 0.05 / js::max(ratio, 1.0) - 0.05;
        let lvl = pow(10.0, out_db / 20.0);
        let a_c = self.a_c;
        let r_c = exp(-2.2 / (self.rate * js::max(release, 1e-4)));
        let (mut cur, mut next, mut timer, mut over, mut g, mut pos) =
            (self.cur, self.next, self.timer, self.over, self.g, self.pos);
        for i in 0..QUANTUM {
            let mut a = next;
            for c in 0..nc {
                let x = f64::from(block.ch[c][i]).abs();
                if x > a {
                    a = x;
                }
            }
            if timer == 0 || cur < a {
                next = 0.0;
                cur = a;
                timer = la;
                over = if cur > 0.0 { js::max(0.0, 20.0 * log10(cur) - thr) } else { 0.0 };
            } else {
                next = a;
                timer -= 1;
            }
            g = (g - over) * (if over - g >= 0.0 { a_c } else { r_c }) + over;
            let gain = pow(10.0, g * rf) * lvl;
            for c in 0..nc {
                let b = &mut self.buf[c];
                let input = block.ch[c][i];
                block.ch[c][i] = (f64::from(b[pos]) * gain) as f32;
                b[pos] = input;
            }
            pos += 1;
            if pos >= la {
                pos = 0;
            }
        }
        self.cur = cur;
        self.next = next;
        self.timer = timer;
        self.over = over;
        self.g = g;
        self.pos = pos;
    }
}

/// CAkMeterFX, peak mode: the peak (dB) of each 1024-sample audio frame through the meter ballistics (1800edaf0).
/// Passes the signal through; the value is [`Meter::value`].
pub struct Meter {
    pub(crate) p: MeterParams,
    dt: f64,
    cur: f64,
    target: f64,
    timer: f64,
    slots: [f64; 8],
    pk: f64,
    cnt: usize,
}

impl Meter {
    pub(crate) fn new(p: &MeterParams, rate: u32) -> Meter {
        Meter {
            p: *p,
            dt: 1024.0 / f64::from(rate),
            cur: p.min,
            target: p.min,
            timer: 0.0,
            slots: [p.min; 8],
            pk: 0.0,
            cnt: 0,
        }
    }

    /// The meter's value after the last quantum (dB, between its minimum and maximum).
    pub fn value(&self) -> f64 {
        self.cur
    }

    fn frame(&mut self) {
        let p = &self.p;
        let dt = self.dt;
        let peak_db = if self.pk > 0.0 { 20.0 * log10(self.pk) } else { -200.0 };
        let x = js::min(p.max, js::max(p.min, peak_db));
        self.pk = 0.0;
        if self.cur < x {
            self.timer = 0.0;
            self.slots = [p.min; 8];
            self.target = if p.attack != 0.0 { js::min(x, self.cur + dt / p.attack * 10.0) } else { x };
            self.cur = self.target;
        } else {
            let t;
            self.timer += dt;
            if self.timer < p.hold {
                let i = (self.timer / p.hold * 8.0 + 0.5).floor();
                if i > 0.0 {
                    let s = &mut self.slots[i as usize - 1];
                    if *s < x {
                        *s = x;
                    }
                }
                if self.target < x {
                    self.target = x;
                }
                t = self.target;
            } else {
                // hold over: fall to the highest later peak and keep the slots after it
                let s = &mut self.slots;
                let mut m = s[0];
                let mut idx = 0;
                if m < x {
                    m = x;
                    idx = 8;
                }
                for (k, v) in s.iter().enumerate().skip(1) {
                    if m <= *v {
                        m = *v;
                        idx = k;
                    }
                }
                self.timer = (8 - idx) as f64 * p.hold / 9.0;
                let old = *s;
                for (j, v) in s.iter_mut().enumerate() {
                    *v = if j + idx + 1 < 8 { old[j + idx + 1] } else { p.min };
                }
                t = m;
                self.target = m;
            }
            self.cur = if p.release != 0.0 { js::max(t, self.cur - dt / p.release * 10.0) } else { t };
        }
    }

    pub(crate) fn process(&mut self, block: &Block) {
        let mut pk = self.pk;
        for c in block.ch.iter() {
            for &s in c {
                let x = f64::from(s);
                let a = if x < 0.0 { -x } else { x };
                if a > pk {
                    pk = a;
                }
            }
        }
        self.pk = pk;
        self.cnt += QUANTUM;
        // the worklet counts its 128-sample blocks: a frame every 1024 samples
        while self.cnt >= 1024 {
            self.cnt -= 1024;
            self.frame();
        }
    }
}
