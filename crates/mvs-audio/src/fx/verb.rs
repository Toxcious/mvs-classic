//! CAkRoomVerbFX, stereo (AkRoomVerb.dll: Init 180003660, SetupFDNs 18000a920, ProcessSpread2Out 180007270).
//! Mono mixdown -> tone filters -> early reflection taps and, after the
//! reverb delay and 4 diffusion allpasses, a cascade of input delays each feeding a 4-line feedback delay network
//! with a one-pole damping filter per line.

use std::f64::consts::{LN_10, SQRT_2};

use mvs_data::sound::{ErTaps, RoomVerbParams};
use mvs_math::js;

use super::SUB;
use super::ak::{Bq, Line, biquad, fbits, fr, silent, u32_of};
use crate::QUANTUM;
use crate::math::pow;
use crate::mix::Block;

/// The generator of the plugin's pseudo-random delay offsets.
fn lcg(s: u32) -> u32 {
    s.wrapping_mul(0x0bb38435).wrapping_add(0x3619636b)
}

/// ComputePrimeDelayLengths (180001fb0): each delay time (ms) to a sample count, odd, free of factors and different
/// from the one before.
fn primes(ms: &[f64], sr: f64) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::with_capacity(ms.len());
    for (i, &t) in ms.iter().enumerate() {
        let mut n = u32_of(fr(fr(t * fr(0.001)) * sr));
        if n & 1 == 0 {
            n = n.wrapping_add(1);
        }
        let bound = f64::from(n).sqrt().floor() as u32 + 1;
        loop {
            let mut d = 3;
            while d < bound {
                if n.is_multiple_of(d) {
                    n += 2;
                    d = 3;
                } else {
                    d += 2;
                }
            }
            if i > 0 && n == out[i - 1] {
                n += 2;
                continue;
            }
            break;
        }
        out.push(n);
    }
    out
}

/// One early reflection side's taps.
struct Taps {
    off: Vec<usize>,
    g: Vec<f32>,
}

struct Allpass {
    x: Line,
    y: Line,
    g: f64,
}

struct Tone {
    pos: u8,
    a: Bq,
    b: Bq,
}

pub struct RoomVerb {
    pub(crate) p: RoomVerbParams,
    dc_a1: f64,
    dc: [f64; 4],
    er_delay: Option<Line>,
    er_on: bool,
    er_buf: Vec<f32>,
    er_pos: usize,
    er_l: Taps,
    er_r: Taps,
    rev_delay: Option<Line>,
    ap: Vec<Allpass>,
    units: usize,
    f_len: Vec<usize>,
    f_pos: Vec<usize>,
    f_off: Vec<usize>,
    f_b0: Vec<f64>,
    f_p: Vec<f64>,
    f_y: Vec<f64>,
    f_buf: Vec<f32>,
    in_delay: Vec<Line>,
    mix: f64,
    tone: Vec<Tone>,
    w: f64,
    w2: f64,
    tail: f64,
    idle: f64,
    clean: bool,
}

impl RoomVerb {
    /// None when the pattern has no taps.
    pub(crate) fn new(p: &RoomVerbParams, er: &ErTaps, rate: u32) -> Option<RoomVerb> {
        let sr = f64::from(rate);
        let (el, erg) = (&er.left, &er.right);
        if el.is_empty() || erg.is_empty() {
            return None;
        }
        let dc_a1 = fr(1.0 - fr(fr(fbits(0x40c90fdb) * p.dc_filter_cut_freq) / sr));
        // early reflections (ERUnitDual::Init 18000c170): tap times scale with 2^(RoomSize / 100); the common lead-in
        // is a delay line of its own
        let scale = fr(pow(2.0, fr(p.room_size * fr(0.01))));
        let tmin = fr(js::min(el[0][0], erg[0][0]) * scale);
        let er_lead = fr(fr(tmin * sr) * fr(0.001)).trunc() as i64;
        let er_delay = (er_lead > 0).then(|| Line::new(er_lead.max(4) as usize));
        let last = js::max(el[el.len() - 1][0], erg[erg.len() - 1][0]);
        let er_len = (fr(fr(fr(last * scale) - tmin) * fr(0.001)) * sr).trunc() as i64;
        let er_len = (er_len.max(4) & 0xfffc) as usize;
        let taps = |list: &[[f64; 2]]| {
            let mut off: Vec<usize> = Vec::new();
            let mut g = Vec::new();
            for &[t, gain] in list {
                let k = js::to_int32((fr(fr(fr(scale * t) - tmin) * fr(0.001)) * sr).trunc()) & !3;
                let mut k = k.max(0) as usize;
                if k >= er_len {
                    k = er_len - 4;
                }
                if off.last() == Some(&k) {
                    continue;
                }
                off.push(k);
                g.push(gain as f32);
            }
            Taps { off, g }
        };
        let er_l = taps(el);
        let er_r = taps(erg);
        let rd = fr(fr(p.reverb_delay * fr(0.001)) * sr).trunc() as i64;
        let rev_delay = (rd > 0).then(|| Line::new(rd.max(4) as usize));
        let rdm = fbits(0x2c23d70a); // 0.01 / 2^32: percent of a 32-bit random value
        // diffusion allpasses (ComputeDiffusionFiltersDelayTimes 180001ad0)
        let mut seed: u32 = 0x2ee007;
        let mut t = p.diffusion_delay_max;
        let mut dt = Vec::with_capacity(4);
        for i in 0..4 {
            if i > 0 {
                t = fr(fr(p.diffusion_delay_scale_perc * fr(0.01)) * t);
            }
            seed = lcg(seed);
            dt.push(fr(fr(fr(fr(f64::from(seed) * t) * p.diffusion_delay_rdm_perc) * rdm) + t));
        }
        dt.sort_by(|x, y| x.total_cmp(y));
        let ap: Vec<Allpass> = primes(&dt, sr)
            .iter()
            .enumerate()
            .map(|(i, &n)| {
                let n = n.max(1) as usize;
                let g = js::min(
                    js::max(
                        fr(fr(p.diffusion * fbits(0x3cca841d)) - fr(f64::from(3 - i as i32) * fbits(0x3f1e3737))),
                        0.0,
                    ),
                    fbits(0x3f1e3737),
                );
                Allpass { x: Line::new(n), y: Line::new(n), g }
            })
            .collect();
        // feedback delay networks (ComputeFDNDelayTimes 180001cf0, FDN4::Init 18000d0b0)
        let units = p.num_reverb_units as usize;
        let n_lines = units * 4;
        let d = fr(fr(fr(fr(p.density_delay_max - p.density_delay_min) * fr(0.01)) * p.density) + p.density_delay_min);
        let lo =
            fr(fr(fr(fr(fr(p.room_shape_max - p.room_shape_min) * fr(0.01)) * p.room_shape) + p.room_shape_min) * d);
        let hi = fr(fr(d - lo) + d);
        let inv_n = fr(1.0 / (n_lines as f64 - 1.0));
        let mut ft = Vec::with_capacity(n_lines);
        seed = 0x6b4cc7;
        for i in 0..n_lines {
            seed = lcg(seed);
            let base = fr(fr(fr(i as f64 * fr(hi - lo)) * inv_n) + lo);
            ft.push(fr(fr(fr(fr(f64::from(seed) * base) * p.density_delay_rdm_perc) * rdm) + base));
        }
        ft.sort_by(|x, y| x.total_cmp(y));
        let pr = primes(&ft, sr);
        let mut f_len = vec![0; n_lines];
        let f_pos = vec![0; n_lines];
        let mut f_off = vec![0; n_lines];
        let mut f_b0 = vec![0.0; n_lines];
        let mut f_p = vec![0.0; n_lines];
        let mut total = 0;
        let ln10_4 = LN_10 / 4.0;
        for u in 0..units {
            let len = [pr[u], pr[units + u], pr[2 * units + u], pr[3 * units + u]].map(f64::from);
            // ComputeMaxStableHFRatio on the unit's longest line
            let y = -3.0 * len[3] / (p.decay_time * sr) * ln10_4;
            let mut c = 1.0 - p.hf_damping * p.hf_damping;
            if y * c > 1.0 {
                c = 1.0 / y;
            }
            for (k, &lk) in len.iter().enumerate() {
                let j = u * 4 + k;
                let e = -3.0 * lk / (p.decay_time * sr);
                let pp = js::min(e * ln10_4 * c, 0.999);
                f_len[j] = lk as usize;
                f_off[j] = total;
                total += f_len[j];
                f_p[j] = pp;
                f_b0[j] = (1.0 - pp) * pow(10.0, e);
            }
        }
        let f_buf = vec![0.0; total];
        let in_base = fr(p.reverb_unit_input_delay / units as f64);
        let mut in_delay = Vec::with_capacity(units);
        seed = 0x1bc0f99;
        for _ in 0..units {
            seed = lcg(seed);
            let ms = fr(fr(fr(fr(f64::from(seed) * p.reverb_unit_input_delay_rdm_perc) * rdm) * in_base) + in_base);
            let n = fr(fr(ms * fr(0.001)) * sr).trunc() as i64;
            in_delay.push(Line::new(n.max(4) as usize));
        }
        let mix = (if units > 1 { 1.0 / (units as f64).sqrt() } else { 1.0 }) * SQRT_2;
        // tone filters: position 1 early reflections only, 2 reverb only, 3 both
        let mut tone = Vec::new();
        if p.enable_tone_controls {
            for f in &p.filters {
                if f.pos == 0 {
                    continue;
                }
                let c = biquad(i32::from(f.curve), sr, f.freq, f.gain, f.q);
                tone.push(Tone { pos: f.pos, a: Bq::new(c), b: Bq::new(c) });
            }
        }
        let w = fr(fr(p.stereo_width * fbits(0x3ad54767)) + fbits(0x3f3504e6));
        let w2 = js::max(0.0, 1.0 - w * w).sqrt();
        let tail = ((p.reverb_delay * 0.001 + p.decay_time) * sr).trunc();
        Some(RoomVerb {
            p: p.clone(),
            dc_a1,
            dc: [0.0; 4],
            er_delay,
            er_on: p.enable_early_reflections,
            er_buf: vec![0.0; er_len],
            er_pos: 0,
            er_l,
            er_r,
            rev_delay,
            ap,
            units,
            f_len,
            f_pos,
            f_off,
            f_b0,
            f_p,
            f_y: vec![0.0; n_lines],
            f_buf,
            in_delay,
            mix,
            tone,
            w,
            w2,
            tail,
            idle: tail + 1.0,
            clean: true,
        })
    }

    fn clear(&mut self) {
        self.dc = [0.0; 4];
        self.er_buf.fill(0.0);
        self.f_buf.fill(0.0);
        self.f_y.fill(0.0);
        if let Some(l) = &mut self.er_delay {
            l.clear();
        }
        if let Some(l) = &mut self.rev_delay {
            l.clear();
        }
        for a in &mut self.ap {
            a.x.clear();
            a.y.clear();
        }
        for l in &mut self.in_delay {
            l.clear();
        }
        for f in &mut self.tone {
            f.a.clear();
            f.b.clear();
        }
    }

    pub(crate) fn process(&mut self, block: &mut Block) {
        for s in (0..QUANTUM).step_by(SUB) {
            self.run(block, s..s + SUB);
        }
    }

    fn run(&mut self, block: &mut Block, r: std::ops::Range<usize>) {
        if silent(block, &r) {
            self.idle += r.len() as f64;
            if self.idle > self.tail {
                if !self.clean {
                    self.clear();
                    self.clean = true;
                }
                for c in block.ch.iter_mut() {
                    c[r.clone()].fill(0.0);
                }
                return;
            }
        } else {
            self.idle = 0.0;
            self.clean = false;
        }
        let two = block.channels() > 1;
        let (dry, er_lvl, rev_lvl) = (self.p.dry_level, self.p.er_level, self.p.reverb_level);
        let (a1, mix, w, w2) = (self.dc_a1, self.mix, self.w, self.w2);
        let units = self.units;
        let er_len = self.er_buf.len();
        for i in r {
            let il = f64::from(block.ch[0][i]);
            let ir = f64::from(if two { block.ch[1][i] } else { block.ch[0][i] });
            let mut m = (il + ir) * std::f64::consts::FRAC_1_SQRT_2;
            for t in &mut self.tone {
                if t.pos == 3 {
                    m = t.a.run(m);
                }
            }
            let mut rv = match &mut self.rev_delay {
                Some(l) => l.run(m),
                None => m,
            };
            let (mut el, mut er) = (0.0, 0.0);
            if self.er_on {
                let pos = self.er_pos;
                self.er_buf[pos] = match &mut self.er_delay {
                    Some(l) => l.run(m),
                    None => m,
                } as f32;
                for k in 0..self.er_l.off.len() {
                    let mut j = pos as isize - self.er_l.off[k] as isize;
                    if j < 0 {
                        j += er_len as isize;
                    }
                    el += f64::from(self.er_l.g[k]) * f64::from(self.er_buf[j as usize]);
                }
                for k in 0..self.er_r.off.len() {
                    let mut j = pos as isize - self.er_r.off[k] as isize;
                    if j < 0 {
                        j += er_len as isize;
                    }
                    er += f64::from(self.er_r.g[k]) * f64::from(self.er_buf[j as usize]);
                }
                self.er_pos = if pos + 1 >= er_len { 0 } else { pos + 1 };
                for t in &mut self.tone {
                    if t.pos == 1 {
                        el = t.a.run(el);
                        er = t.b.run(er);
                    }
                }
            }
            for a in &mut self.ap {
                let (xd, yd) = (a.x.head(), a.y.head());
                let y = a.g * (rv - yd) + xd;
                a.x.run(rv);
                a.y.run(y);
                rv = y;
            }
            for t in &mut self.tone {
                if t.pos == 2 {
                    rv = t.a.run(rv);
                }
            }
            let (mut o0, mut o1) = (0.0, 0.0);
            for u in 0..units {
                rv = self.in_delay[u].run(rv);
                let j = u * 4;
                let mut y = [0.0; 4];
                for (k, yk) in y.iter_mut().enumerate() {
                    let at = self.f_off[j + k] + self.f_pos[j + k];
                    *yk = f64::from(self.f_buf[at]) * self.f_b0[j + k] + self.f_p[j + k] * self.f_y[j + k];
                    self.f_y[j + k] = *yk;
                }
                let [y0, y1, y2, y3] = y;
                let s = -0.5 * (y0 + y1 + y2 + y3) + rv;
                if u & 1 == 1 {
                    o1 += y0 - y1 + y2 - y3;
                } else {
                    o0 += y0 - y1 + y2 - y3;
                }
                for (k, v) in [s + y1, s + y2, s + y3, s + y0].into_iter().enumerate() {
                    self.f_buf[self.f_off[j + k] + self.f_pos[j + k]] = v as f32;
                }
                for k in j..j + 4 {
                    self.f_pos[k] += 1;
                    if self.f_pos[k] >= self.f_len[k] {
                        self.f_pos[k] = 0;
                    }
                }
            }
            // OnePoleZeroHPFilter::ProcessBufferWithGain (18000cb70)
            o0 *= mix;
            o1 *= mix;
            let dc = &mut self.dc;
            let h0 = o0 - dc[0] + a1 * dc[1];
            let h1 = o1 - dc[2] + a1 * dc[3];
            *dc = [o0, h0, o1, h1];
            block.ch[0][i] = (dry * il + rev_lvl * (w * h0 + w2 * h1) + er_lvl * (w * el + w2 * er)) as f32;
            if two {
                block.ch[1][i] = (dry * ir + rev_lvl * (w2 * h0 + w * h1) + er_lvl * (w2 * el + w * er)) as f32;
            }
        }
    }
}
