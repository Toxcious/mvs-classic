//! AkGain and AkParameterEQ: a gain node, and a chain of
//! biquad nodes (the plugin's RBJ coefficients are the BiquadFilterNode's; low / high pass are Butterworth) then a
//! gain node for the output level. A node's parameter is a float.

use mvs_data::sound::{FilterType, GainParams, ParamEqParams};

use super::biquad::{Biquad, Kind, coeffs, param};
use crate::ctl::curves::{butterworth_q_db, db_to_gain};
use crate::mix::Block;

fn scale(block: &mut Block, g: f32) {
    for c in block.ch.iter_mut() {
        for s in c.iter_mut() {
            *s *= g;
        }
    }
}

pub struct Gain {
    g: f32,
}

impl Gain {
    /// None when the gain is zero dB or the plugin's gain is unset (no node is built).
    pub(crate) fn new(p: &GainParams) -> Option<Gain> {
        (p.fullband != 0.0).then(|| Gain { g: db_to_gain(p.fullband) as f32 })
    }

    pub(crate) fn process(&mut self, block: &mut Block) {
        scale(block, self.g);
    }
}

struct Band {
    index: usize,
    kind: Kind,
    gained: bool,
    /// Q is linear (not the low-pass / high-pass's fixed Butterworth dB).
    linear_q: bool,
    freq: f64,
    q: f64,
    gain: f64,
    bq: Biquad,
}

pub struct ParamEq {
    pub(crate) p: ParamEqParams,
    rate: f64,
    bands: Vec<Band>,
    out_db: f64,
    out: Option<f32>,
}

/// The BiquadFilterNode type of a dumped filter name (the dump's names are shifted against the plugin's).
fn kind_of(f: FilterType) -> Option<Kind> {
    Some(match f {
        FilterType::LowShelf => Kind::LowPass,
        FilterType::PeakingEQ => Kind::HighPass,
        FilterType::HighShelf => Kind::BandPass,
        FilterType::LowPass => Kind::Notch,
        FilterType::HighPass => Kind::LowShelf,
        FilterType::BandPass => Kind::HighShelf,
        FilterType::Notch => Kind::Peaking,
        FilterType::None => return None,
    })
}

impl ParamEq {
    /// `driven`: the parameter ids that have game-parameter curves (a band whose gain is zero stays if its gain does).
    /// None when no band and no output level remain.
    pub(crate) fn new(p: &ParamEqParams, rate: u32, channels: usize, driven: &[u16]) -> Option<ParamEq> {
        let rate = f64::from(rate);
        let has = |i: usize| driven.iter().any(|&d| usize::from(d) == i);
        let mut bands = Vec::new();
        for (b, band) in p.bands.iter().enumerate() {
            let Some(kind) = kind_of(band.filter) else { continue };
            if !band.on {
                continue;
            }
            let gained = matches!(kind, Kind::Peaking | Kind::LowShelf | Kind::HighShelf);
            if gained && band.gain == 0.0 && !has(5 * b + 1) {
                continue;
            }
            let linear_q = !matches!(kind, Kind::LowPass | Kind::HighPass);
            let mut band = Band {
                index: b,
                kind,
                gained,
                linear_q,
                freq: param(band.frequency.min(rate * 0.5)),
                q: param(if linear_q { band.q } else { butterworth_q_db() }),
                gain: param(if gained { band.gain } else { 0.0 }),
                bq: Biquad::new(channels),
            };
            band.update(rate);
            bands.push(band);
        }
        let out = (p.output_level != 0.0 || has(15)).then(|| db_to_gain(p.output_level) as f32);
        if bands.is_empty() && out.is_none() {
            return None;
        }
        Some(ParamEq { p: p.clone(), rate, bands, out_db: p.output_level, out })
    }

    pub(crate) fn base(&self, id: u16) -> Option<f64> {
        let id = usize::from(id);
        if id == 15 {
            return self.out.map(|_| self.out_db);
        }
        let band = self.bands.iter().find(|b| b.index == id / 5)?;
        match id % 5 {
            1 if band.gained => Some(self.p.bands[band.index].gain),
            2 => Some(self.p.bands[band.index].frequency),
            3 if band.linear_q => Some(self.p.bands[band.index].q),
            _ => None,
        }
    }

    pub(crate) fn set(&mut self, id: u16, value: f64) {
        let id = usize::from(id);
        if id == 15 {
            if self.out.is_some() {
                self.out_db = value;
                self.out = Some(db_to_gain(value) as f32);
            }
            return;
        }
        let rate = self.rate;
        let Some(band) = self.bands.iter_mut().find(|b| b.index == id / 5) else { return };
        match id % 5 {
            1 if band.gained => band.gain = param(value),
            2 => band.freq = param(value.min(rate * 0.5)),
            3 if band.linear_q => band.q = param(value),
            _ => return,
        }
        band.update(rate);
    }

    pub(crate) fn process(&mut self, block: &mut Block) {
        for band in &mut self.bands {
            for (c, x) in block.ch.iter_mut().enumerate() {
                band.bq.run(c, x);
            }
        }
        if let Some(g) = self.out {
            scale(block, g);
        }
    }
}

impl Band {
    fn update(&mut self, rate: f64) {
        self.bq.c = coeffs(self.kind, self.freq / (rate * 0.5), self.q, self.gain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_data::sound::EqBand;

    use crate::math::pow;

    #[test]
    fn zero_gain_band_without_a_curve_is_dropped() {
        let band = EqBand { filter: FilterType::Notch, gain: 0.0, frequency: 1000.0, q: 1.0, on: true };
        let p = ParamEqParams { bands: vec![band], output_level: 0.0, process_lfe: true };
        assert!(ParamEq::new(&p, 48_000, 2, &[]).is_none());
        let mut eq = ParamEq::new(&p, 48_000, 2, &[1]).unwrap();
        eq.set(1, 6.0);
        assert_eq!(eq.base(1), Some(0.0));
        let mut b = Block::new(2);
        b.ch[0][0] = 1.0;
        eq.process(&mut b);
        assert!(b.ch[0][0] > 1.0);
    }

    #[test]
    fn output_level_scales() {
        let p = ParamEqParams { bands: vec![], output_level: -6.0, process_lfe: true };
        let mut eq = ParamEq::new(&p, 48_000, 1, &[]).unwrap();
        let mut b = Block::new(1);
        b.ch[0].fill(1.0);
        eq.process(&mut b);
        assert_eq!(b.ch[0][5], pow(10.0, -6.0 / 20.0) as f32);
    }
}
