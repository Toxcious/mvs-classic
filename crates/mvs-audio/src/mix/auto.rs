//! A parameter's automation on the audio clock: the value it holds and the ramp it is on, moved by
//! [`ParamChange`]s at their sample (the Web Audio AudioParam events: setValueAtTime,
//! linearRampToValueAtTime, setTargetAtTime, and a fade curve's per-frame steps).

use mvs_data::sound::Curve;

use crate::QUANTUM;
use crate::ctl::curves::interp;
use crate::spec::{Param, ParamChange, Ramp, Target};

/// How the value moves from `v0` at `t0` towards `to`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Hold,
    /// Reaches `to` at `t1`.
    Linear {
        t1: u64,
    },
    /// Approaches `to` with a time constant of `tau` samples.
    Target {
        tau: f64,
    },
    /// Along an AkInterpolation curve, reaching `to` at `t1`.
    Curve {
        t1: u64,
        curve: Curve,
    },
}

/// A target approach counts as there after this many time constants (the value is within 2e-9 of it).
const TARGET_DONE: f64 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Auto {
    v0: f64,
    t0: u64,
    to: f64,
    kind: Kind,
}

/// Seconds as samples of the mixer's rate.
pub(crate) fn samples(secs: f64) -> u64 {
    let s = (secs * f64::from(crate::RATE)).round();
    if s > 0.0 { s as u64 } else { 0 }
}

impl Auto {
    pub(crate) fn new(v: f64) -> Auto {
        Auto { v0: v, t0: 0, to: v, kind: Kind::Hold }
    }

    /// The value at audio clock `t` (not before the last change).
    pub(crate) fn value(&self, t: u64) -> f64 {
        let e = t.saturating_sub(self.t0);
        match self.kind {
            Kind::Hold => self.to,
            Kind::Linear { t1 } => {
                if t >= t1 {
                    self.to
                } else {
                    let span = (t1 - self.t0) as f64;
                    self.v0 + (self.to - self.v0) * (e as f64 / span)
                }
            }
            Kind::Target { tau } => {
                let x = e as f64 / tau;
                if x >= TARGET_DONE { self.to } else { self.to + (self.v0 - self.to) * crate::math::exp(-x) }
            }
            Kind::Curve { t1, curve } => {
                if t >= t1 {
                    self.to
                } else {
                    interp(curve, e as f64 / (t1 - self.t0) as f64, self.v0, self.to)
                }
            }
        }
    }

    /// The sample the value stops moving at (None: it holds).
    pub(crate) fn settles(&self) -> Option<u64> {
        match self.kind {
            Kind::Hold => None,
            Kind::Linear { t1 } | Kind::Curve { t1, .. } => Some(t1),
            Kind::Target { tau } => Some(self.t0 + (tau * TARGET_DONE).ceil() as u64),
        }
    }

    /// Whether the value can change at or after `t`.
    pub(crate) fn moving(&self, t: u64) -> bool {
        match self.kind {
            Kind::Hold => false,
            Kind::Linear { t1 } | Kind::Curve { t1, .. } => t < t1,
            Kind::Target { tau } => ((t.saturating_sub(self.t0)) as f64 / tau) < TARGET_DONE,
        }
    }

    /// Takes a change at `at`: the ramp starts from the value the automation has there (Web Audio's
    /// cancelScheduledValues then setValueAtTime of the held value); a joined ramp runs from where it began.
    pub(crate) fn change(&mut self, at: u64, value: f64, ramp: Ramp) {
        if let Ramp::Joined { from, start, secs, curve } = ramp {
            let t1 = start + samples(secs);
            *self = if t1 > at {
                Auto { v0: from, t0: start, to: value, kind: Kind::Curve { t1, curve } }
            } else {
                Auto { v0: value, t0: at, to: value, kind: Kind::Hold }
            };
            return;
        }
        let v0 = self.value(at);
        let kind = match ramp {
            Ramp::Set => Kind::Hold,
            Ramp::Linear(s) => match samples(s) {
                0 => Kind::Hold,
                n => Kind::Linear { t1: at + n },
            },
            Ramp::Target(s) => {
                let tau = s * f64::from(crate::RATE);
                if tau > 0.0 { Kind::Target { tau } } else { Kind::Hold }
            }
            Ramp::Curve { secs, curve } => match samples(secs) {
                0 => Kind::Hold,
                n => Kind::Curve { t1: at + n, curve },
            },
            Ramp::Joined { .. } => unreachable!("taken above"),
        };
        *self = Auto { v0, t0: at, to: value, kind };
    }

    /// What the automation does from `at` on: the value it has there, and the ramp it is on (its target and the ramp
    /// as a change at `at` takes it: a line or curve joined part-way; an approach to a target has no memory, it
    /// starts again from the value reached).
    pub(crate) fn onward(&self, at: u64) -> (f64, Option<(f64, Ramp)>) {
        let secs = |t1: u64| (t1 - self.t0) as f64 / f64::from(crate::RATE);
        let ramp = match self.kind {
            Kind::Linear { t1 } if at < t1 => {
                Some(Ramp::Joined { from: self.v0, start: self.t0, secs: secs(t1), curve: Curve::Linear })
            }
            Kind::Curve { t1, curve } if at < t1 => {
                Some(Ramp::Joined { from: self.v0, start: self.t0, secs: secs(t1), curve })
            }
            Kind::Target { tau } if self.moving(at) => Some(Ramp::Target(tau / f64::from(crate::RATE))),
            _ => None,
        };
        (self.value(at), ramp.map(|r| (self.to, r)))
    }

    /// Fills `out` with the value per sample of the quantum at `now`, taking the changes of `chs` (in time order)
    /// that move `param` of `target` at their sample; `conv` maps a change's value to the automated one. Returns the
    /// value when it holds over the whole quantum (and `out` is filled with it all the same).
    pub(crate) fn fill(
        &mut self,
        now: u64,
        chs: &[ParamChange],
        target: Target,
        param: Param,
        conv: fn(f64) -> f64,
        out: &mut [f32; QUANTUM],
    ) -> Option<f32> {
        let mut mine = chs.iter().filter(|c| c.target == target && c.param == param).peekable();
        if mine.peek().is_none() && !self.moving(now) {
            let v = self.value(now) as f32;
            out.fill(v);
            return Some(v);
        }
        for (i, o) in out.iter_mut().enumerate() {
            let t = now + i as u64;
            while let Some(c) = mine.next_if(|c| c.at <= t) {
                self.change(c.at.max(now), conv(c.value), c.ramp);
            }
            *o = self.value(t) as f32;
        }
        None
    }

    /// Takes the changes of `chs` for `param` of `target` due in the quantum at `now` without a per-sample value
    /// (a parameter read once a quantum).
    pub(crate) fn take(&mut self, now: u64, chs: &[ParamChange], target: Target, param: Param, conv: fn(f64) -> f64) {
        for c in chs.iter().filter(|c| c.target == target && c.param == param) {
            self.change(c.at.max(now), conv(c.value), c.ramp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::VoiceId;

    fn ch(at: u64, value: f64, ramp: Ramp) -> ParamChange {
        ParamChange { at, target: Target::Voice(VoiceId(1)), param: Param::Fade, value, ramp }
    }

    #[test]
    fn linear_from_held_value() {
        let mut a = Auto::new(1.0);
        let mut out = [0.0; QUANTUM];
        // a linear ramp to 0 over 100 samples, starting at sample 50 of the quantum
        let chs = [ch(50, 0.0, Ramp::Linear(100.0 / 48_000.0))];
        let held = a.fill(0, &chs, Target::Voice(VoiceId(1)), Param::Fade, |v| v, &mut out);
        assert_eq!(held, None);
        assert_eq!(out[49], 1.0);
        assert_eq!(out[50], 1.0);
        assert_eq!(out[100], 0.5);
        assert_eq!(out[150], 0.0);
        // a change for another voice is not taken
        let other = [ParamChange { target: Target::Voice(VoiceId(2)), ..ch(0, 7.0, Ramp::Set) }];
        assert_eq!(a.fill(256, &other, Target::Voice(VoiceId(1)), Param::Fade, |v| v, &mut out), Some(0.0));
    }

    #[test]
    fn target_and_curve() {
        let mut a = Auto::new(0.0);
        a.change(0, 1.0, Ramp::Target(0.002));
        let tau = 0.002 * 48_000.0;
        assert!((a.value(96) - (1.0 - crate::math::exp(-96.0 / tau))).abs() < 1e-15);
        assert!(!a.moving(96 * 20));
        let mut c = Auto::new(0.0);
        c.change(10, 2.0, Ramp::Curve { secs: 100.0 / 48_000.0, curve: Curve::Log1 });
        assert_eq!(c.value(60), interp(Curve::Log1, 0.5, 0.0, 2.0));
        assert_eq!(c.value(110), 2.0);
    }
}
