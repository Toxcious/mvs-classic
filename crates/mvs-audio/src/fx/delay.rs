//! CAkStereoDelayFX (AkStereoDelay.dll: Init 180001510, CStereoDelayLine::ProcessBuffer 180002910,
//! AkStereoDelayDSPProcess 180003960): stereo with the left input into
//! line 0 and the right into line 1, each line fed input + own feedback + the other line's cross feed through the
//! filter; out = dry * in + wet * line.

use mvs_data::sound::{FilterType, StereoDelayParams};
use mvs_math::js;

use super::SUB;
use super::ak::{Bq, biquad, silent};
use crate::QUANTUM;
use crate::math::log10;
use crate::mix::Block;

pub struct StereoDelay {
    pub(crate) p: StereoDelayParams,
    len: [usize; 2],
    buf: [Vec<f32>; 2],
    pos: [usize; 2],
    bq: Option<[Bq; 2]>,
    fb: [f64; 2],
    cross: [f64; 2],
    tail: f64,
    idle: f64,
    clean: bool,
}

impl StereoDelay {
    pub(crate) fn new(p: &StereoDelayParams, rate: u32) -> StereoDelay {
        let sr = f64::from(rate);
        let frame = 1024.0 / sr;
        let len = [0, 1].map(|i| (sr * js::max(p.lines[i].delay, frame)).floor() as usize);
        let buf = [vec![0.0; len[0]], vec![0.0; len[1]]];
        // the dumped filter names are one step ahead of the plugin's enum; a low shelf and none are no filter
        let kind = match p.filter {
            FilterType::PeakingEQ => 0,
            FilterType::HighShelf => 1,
            FilterType::LowPass => 2,
            FilterType::HighPass => 3,
            FilterType::BandPass => 4,
            FilterType::Notch => 5,
            FilterType::None | FilterType::LowShelf => -1,
        };
        let bq = (kind >= 0).then(|| {
            let c = biquad(kind, sr, p.filter_frequency, p.filter_gain, p.filter_q);
            [Bq::new(c), Bq::new(c)]
        });
        let fb = [0, 1].map(|i| if p.enable_feedback { p.lines[i].feedback } else { 0.0 });
        let cross = [0, 1].map(|i| if p.enable_cross_feed { p.lines[i].cross_feed } else { 0.0 });
        // tail: the time the longer feedback loop takes to fall 60 dB, at most 60 s
        let mut tail = 0.0;
        for (c, line) in p.lines.iter().enumerate() {
            let t = js::max(line.delay, frame);
            let v = if fb[c] > 0.0 { -60.0 / js::min(20.0 * log10(fb[c]), -0.1) * t } else { t };
            tail = js::max(tail, v);
        }
        let tail = js::min(tail, 60.0) * sr;
        StereoDelay { p: *p, len, buf, pos: [0, 0], bq, fb, cross, tail, idle: tail + 1.0, clean: true }
    }

    pub(crate) fn process(&mut self, block: &mut Block) {
        for s in (0..QUANTUM).step_by(SUB) {
            self.run(block, s..s + SUB);
        }
    }

    fn run(&mut self, block: &mut Block, r: std::ops::Range<usize>) {
        let n = r.len() as f64;
        let two = block.channels() > 1;
        let quiet = silent(block, &r);
        if quiet {
            self.idle += n;
            if self.idle > self.tail {
                if !self.clean {
                    self.buf[0].fill(0.0);
                    self.buf[1].fill(0.0);
                    if let Some(bq) = &mut self.bq {
                        bq[0].clear();
                        bq[1].clear();
                    }
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
        let (fb, c, dry, wet) = (self.fb, self.cross, self.p.dry, self.p.wet);
        let [mut p0, mut p1] = self.pos;
        for i in r {
            let il = f64::from(block.ch[0][i]);
            let ir = f64::from(if two { block.ch[1][i] } else { block.ch[0][i] });
            let d0 = f64::from(self.buf[0][p0]);
            let d1 = f64::from(self.buf[1][p1]);
            let mut x0 = il + d1 * c[1] + d0 * fb[0];
            let mut x1 = ir + d0 * c[0] + d1 * fb[1];
            if let Some(bq) = &mut self.bq {
                x0 = bq[0].run(x0);
                x1 = bq[1].run(x1);
            }
            self.buf[0][p0] = x0 as f32;
            self.buf[1][p1] = x1 as f32;
            p0 += 1;
            if p0 >= self.len[0] {
                p0 = 0;
            }
            p1 += 1;
            if p1 >= self.len[1] {
                p1 = 0;
            }
            block.ch[0][i] = (dry * il + wet * d0) as f32;
            if two {
                block.ch[1][i] = (dry * ir + wet * d1) as f32;
            }
        }
        self.pos = [p0, p1];
    }
}
