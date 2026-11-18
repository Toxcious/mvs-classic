//! Wwise's interpolation curves, conversion tables, stop fades and the filters' cutoff law
//!.

use std::f64::consts::{PI, SQRT_2};

use mvs_data::sound::{Curve, Id, Pt, Rtpc, Scaling};

use crate::math::{cos, log10, pow, sin};

/// A dB value as a linear gain.
pub fn db_to_gain(db: f64) -> f64 {
    pow(10.0, db / 20.0)
}

/// AkInterpolation (wwiser's port of the SDK): t in 0..1 from a to b.
// Deviation: the sine-based shapes use the exact functions instead of the SDK's polynomial approximations.
pub fn interp(kind: Curve, t: f64, a: f64, b: f64) -> f64 {
    match kind {
        Curve::Log3 => (1.0 - t) * (1.0 - t) * (1.0 - t) * (a - b) + b,
        Curve::Log1 => (t - 3.0) * t * 0.5 * (a - b) + a,
        Curve::Exp1 => (t + 1.0) * t * 0.5 * (b - a) + a,
        Curve::Exp3 => t * t * t * (b - a) + a,
        Curve::Sine => sin(t * PI / 2.0) * (b - a) + a,
        Curve::SineRecip => (1.0 - cos(t * PI / 2.0)) * (b - a) + a,
        Curve::SCurve => (1.0 - cos(t * PI)) / 2.0 * (b - a) + a,
        Curve::InvSCurve => {
            let s = if t < 0.5 { sin(t * PI) / 2.0 } else { 1.0 - sin(t * PI) / 2.0 };
            s * (b - a) + a
        }
        Curve::Constant => a,
        Curve::Linear => (b - a) * t + a,
    }
}

/// AkCurveInterpolation in the Wwise numbering (EAkCurveInterpolation table 143b68510).
const CURVES: [Curve; 10] = [
    Curve::Log3,
    Curve::Sine,
    Curve::Log1,
    Curve::InvSCurve,
    Curve::Linear,
    Curve::SCurve,
    Curve::Exp1,
    Curve::SineRecip,
    Curve::Exp3,
    Curve::Constant,
];

/// The fade of a stop with a transition time: CAkPBI::_Stop (18002c2c0) -> CreateTransition (180029720) of the
/// play / stop ratio to 0 with bUseReciprocalCurve; CAkTransition::InitParameters (18003e6b0) takes curve 8 - c on
/// the fade-out (InvSCurve and SCurve kept; Constant: silent at once) over N = (ms - 1 + 21) / 21 audio frames
/// (integer division, g_msPerBufferTick 21; a frame is 1024 samples), and ComputeTransition (18003e530) sets the
/// ratio interp(k / N) at frame k; at frame N the voice stops. The ratio is a linear gain on the voice. Writes the
/// ratios of frames 0..=N (the last 0) to `out`; false for a stop at once (0 ms, or Constant).
pub fn stop_ratios(curve: Curve, ms: f64, out: &mut Vec<f64>) -> bool {
    out.clear();
    let c = CURVES.iter().position(|&k| k == curve).expect("every curve is in the table");
    let n = ((ms.trunc() - 1.0 + 21.0) / 21.0).floor();
    if n <= 0.0 || c == 9 {
        return false;
    }
    let rev = if c == 3 || c == 5 { curve } else { CURVES[8 - c] };
    let mut k = 0.0;
    while k <= n {
        out.push(interp(rev, k / n, 1.0, 0.0));
        k += 1.0;
    }
    true
}

/// A conversion table's value at x (clamped to the end points), in the curve's own units.
pub fn curve_at(r: &Rtpc, x: f64) -> f64 {
    pts_at(&r.pts, x)
}

/// The same for a bare list of points (an attenuation's curve).
pub fn pts_at(p: &[Pt], x: f64) -> f64 {
    if x <= p[0].x {
        return p[0].y;
    }
    for i in 0..p.len() - 1 {
        if x <= p[i + 1].x {
            let w = p[i + 1].x - p[i].x;
            let w = if w == 0.0 { 1.0 } else { w };
            return interp(p[i].curve, (x - p[i].x) / w, p[i].y, p[i + 1].y);
        }
    }
    p[p.len() - 1].y
}

/// A random value in a ranged property's [min, max] (0 without one); `rnd` in [0, 1).
pub fn rand(r: Option<[f64; 2]>, rnd: f64) -> f64 {
    r.map_or(0.0, |[lo, hi]| lo + rnd * (hi - lo))
}

/// The playlist of a continuous random container in Normal mode without weights. GetNextToPlayContinuous
/// (1800811c0) draws from the continuation item's own CAkRandomInfo, made at the first pick of the playback
/// (CreateRandomInfo 180080950): a fresh list for each post, all items unplayed, none blocked, an avoid-repeat list
/// of min(AvoidRepeatCount, 4) (Init 1800609d0). SelectRandomly (1800818a0): a uniform pick among the items the
/// avoid-repeat list does not block (UpdateNormalAvoidRepeat 180082740); an item's first play lowers the count of
/// items left, and a pick that finds it at 0 ends a container that does not loop (AddNewContListItem 180080530:
/// LoopCount 1 = loop disabled). So the container plays until each item has played once, repeats included. A
/// looping container (LoopCount 0: for ever, else that many passes) refills at that pick instead and picks on: every
/// item unplayed again.
// Unverified: the refill keeps the avoid-repeat list, and LoopCount n > 1 makes n passes (the count's decrement read
// as one per refill; the build's looping continuous containers all have LoopCount 0).
#[derive(Clone, Debug)]
pub struct ContinuousPlaylist {
    list: Vec<Id>,
    avoid: usize,
    played: Vec<Id>,
    hist: Vec<Id>,
    /// The passes left after this one: None for ever.
    passes: Option<u16>,
}

impl ContinuousPlaylist {
    /// `loop_count`: the container's LoopCount (1 plays the list once, 0 for ever).
    pub fn new(list: Vec<Id>, avoid_repeat: u16, loop_count: u16) -> ContinuousPlaylist {
        let avoid = usize::from(avoid_repeat).min(4).min(list.len().saturating_sub(1));
        let passes = (loop_count != 0).then(|| loop_count - 1);
        let (played, hist) = (Vec::with_capacity(list.len()), Vec::with_capacity(avoid + 1));
        ContinuousPlaylist { list, avoid, played, hist, passes }
    }

    /// Each child has played and no pass is left: [`Self::next`] gives None without a draw.
    pub fn done(&self) -> bool {
        self.played.len() == self.list.len() && self.passes == Some(0)
    }

    /// The next child, None once each has played in the last pass; `rnd` in [0, 1).
    pub fn next(&mut self, rnd: f64) -> Option<Id> {
        if self.played.len() == self.list.len() {
            match &mut self.passes {
                Some(0) => return None,
                Some(n) => *n -= 1,
                None => {}
            }
            self.played.clear();
        }
        // (the k-th child not in the recent history: counted, not collected)
        let hist = &self.hist;
        let pool = || self.list.iter().copied().filter(|x| !hist.contains(x));
        let k = (rnd * pool().count() as f64).floor() as usize;
        let pick = pool().nth(k).expect("a child outside the recent history");
        if !self.played.contains(&pick) {
            self.played.push(pick);
        }
        self.hist.push(pick);
        if self.hist.len() > self.avoid {
            self.hist.remove(0);
        }
        Some(pick)
    }
}

/// The build's audio frame, s (1024 samples at 48 kHz). Parameter changes take one frame.
pub const FRAME_SECS: f64 = 1024.0 / 48_000.0;

/// The start of a continuous container's next child with transition mode Disabled: the first audio frame boundary
/// after the last sample of a child that started at `start` (a frame boundary, s) and runs `dur` s.
pub fn next_frame_after(start: f64, dur: f64) -> f64 {
    start + (dur / FRAME_SECS - 1e-6).ceil() * FRAME_SECS
}

/// A curve's output in the target's units: dB curves store linear gain - 1 (wwiser: 20 log10(v + 1)), Log curves
/// store log10 of the value (frequencies).
pub fn curve_value(r: &Rtpc, x: f64) -> f64 {
    let y = curve_at(r, x);
    match r.scaling {
        Scaling::Db => {
            if y <= -1.0 {
                f64::NEG_INFINITY
            } else {
                20.0 * log10(y + 1.0)
            }
        }
        Scaling::Log => pow(10.0, y),
        Scaling::None => y,
    }
}

/// AkLpfParamEval::_EvalCutoff (AkSoundEngineDLL 1800d2500): an LPF value 0..100 to a cutoff (Hz). The HPF
/// evaluates the same curve at 100 - value (AkHpfParamEval::EvalCutoff, 1800d24e0). The voice filter is a
/// 2nd-order Butterworth (K = 1 / tan(pi fc / sr), 1800bef10).
pub fn eval_cutoff(p: f64, rate: f64) -> f64 {
    let fc = if p >= 30.0 { 16.797 * pow(10.0, 0.037427 * (100.0 - p)) } else { 7000.0 + (30.0 - p) * 433.33 };
    mvs_math::js::min(fc, 0.45 * rate)
}

/// A Butterworth's Q as a Web Audio BiquadFilterNode lowpass / highpass takes it (dB).
pub fn butterworth_q_db() -> f64 {
    20.0 * log10(1.0 / SQRT_2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_data::sound::Pt;

    #[test]
    fn stop_fades() {
        let mut r = Vec::new();
        assert!(!stop_ratios(Curve::Linear, 0.0, &mut r));
        assert!(!stop_ratios(Curve::Constant, 500.0, &mut r));
        // 100 ms: (100 - 1 + 21) / 21 = 5 frames; Linear fades out along Linear (8 - 4)
        assert!(stop_ratios(Curve::Linear, 100.0, &mut r));
        assert_eq!(r, [1.0, 0.8, 0.6, 0.4, 0.19999999999999996, 0.0]);
        // Log3 (0) fades out along Exp3 (8)
        assert!(stop_ratios(Curve::Log3, 21.0, &mut r));
        assert_eq!(r, [1.0, 0.0]);
    }

    #[test]
    fn tables() {
        let pt = |x, y, curve| Pt { x, y, curve };
        let r = Rtpc {
            id: 1,
            param: 0,
            scaling: Scaling::Db,
            pts: vec![pt(0.0, -1.0, Curve::Linear), pt(10.0, 0.0, Curve::Linear)],
            accum: None,
        };
        assert_eq!(curve_value(&r, -5.0), f64::NEG_INFINITY);
        assert_eq!(curve_value(&r, 20.0), 0.0);
        assert_eq!(curve_at(&r, 5.0), -0.5);
    }

    #[test]
    fn playlist_plays_each_once() {
        let mut p = ContinuousPlaylist::new(vec![1, 2, 3], 1, 1);
        // draws 0, 0.99, 0: 1; then 3 (of 2, 3); then 1 (of 1, 2); then 2 (of 2, 3), the last unplayed
        let draws = [0.0, 0.99, 0.0, 0.0, 0.0];
        let mut seen = Vec::new();
        while let Some(x) = p.next(draws[seen.len()]) {
            seen.push(x);
        }
        assert_eq!(seen, [1, 3, 1, 2]);
    }

    #[test]
    fn playlist_loops() {
        // LoopCount 0: refills once each has played, the avoid-repeat list kept
        let mut p = ContinuousPlaylist::new(vec![1, 2], 1, 0);
        let seen: Vec<_> = (0..6).map(|_| p.next(0.0).unwrap()).collect();
        assert_eq!(seen, [1, 2, 1, 2, 1, 2]);
        assert!(!p.done());
        // LoopCount 2: two passes
        let mut p = ContinuousPlaylist::new(vec![1, 2], 1, 2);
        let mut seen = Vec::new();
        while let Some(x) = p.next(0.0) {
            seen.push(x);
        }
        assert_eq!(seen, [1, 2, 1, 2]);
        assert!(p.done());
        // one item for ever
        let mut p = ContinuousPlaylist::new(vec![7], 1, 0);
        assert!((0..4).all(|_| p.next(0.5) == Some(7)));
    }
}
