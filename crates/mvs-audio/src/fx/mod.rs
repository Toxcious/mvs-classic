//! The build's effects as the buses run them (AkParametricEQ, AkCompressor, AkPeakLimiter, AkMeter, AkGain,
//! AkRoomVerb, AkStereoDelay) and the voices' low-pass / high-pass. Each runs on a [`Block`] in place; a
//! game-parameter curve on it moves a parameter by its plugin id ([`Fx::set_param`]).
//!
//! AkDelay is not run (it passes through).

mod ak;
pub mod biquad;
mod delay;
mod dynamics;
mod eq;
mod verb;

use mvs_data::sound::{ErTaps, FxParams, Id};

use crate::mix::Block;

pub use delay::StereoDelay;
pub use dynamics::{Compressor, Meter, PeakLimiter};
pub use eq::{Gain, ParamEq};
pub use verb::RoomVerb;

/// The samples of a render quantum: the delay and the reverb test their input for silence per quantum.
pub(crate) const SUB: usize = 128;

/// An effect instance on a bus.
#[allow(
    clippy::large_enum_variant,
    reason = "a bus holds a few of them; boxing would change the shape the mixer matches"
)]
pub enum Fx {
    ParamEq(ParamEq),
    Compressor(Compressor),
    PeakLimiter(PeakLimiter),
    Meter(Meter),
    Gain(Gain),
    RoomVerb(Box<RoomVerb>),
    StereoDelay(StereoDelay),
}

impl Fx {
    /// The effect of a plugin's parameters at `rate` on `channels` channels (RoomVerb with its pattern's taps);
    /// None for a plugin the port does not run, or one left out (a zero gain, a RoomVerb without
    /// taps, an equaliser with nothing to do). The equaliser leaves out a zero-gain band that no curve drives:
    /// [`Fx::with_curves`] names the ones that are.
    pub fn new(p: &FxParams, er: Option<&ErTaps>, rate: u32, channels: usize) -> Option<Fx> {
        Fx::with_curves(p, er, rate, channels, &[])
    }

    /// [`Fx::new`] with the plugin parameter ids that have game-parameter curves on them.
    pub fn with_curves(p: &FxParams, er: Option<&ErTaps>, rate: u32, channels: usize, driven: &[u16]) -> Option<Fx> {
        Some(match p {
            FxParams::AkParameterEQFX(p) => Fx::ParamEq(ParamEq::new(p, rate, channels, driven)?),
            FxParams::AkCompressorFX(p) => Fx::Compressor(Compressor::new(p, rate, channels)),
            FxParams::AkPeakLimiterFX(p) => Fx::PeakLimiter(PeakLimiter::new(p, rate, channels)),
            FxParams::AkMeterFX(p) => Fx::Meter(Meter::new(p, rate)),
            FxParams::AkGainFX(p) => Fx::Gain(Gain::new(p)?),
            FxParams::AkStereoDelayFX(p) => Fx::StereoDelay(StereoDelay::new(p, rate)),
            FxParams::AkRoomVerbFX(p) => Fx::RoomVerb(Box::new(RoomVerb::new(p, er?, rate)?)),
            FxParams::AkDelayFX(_) | FxParams::Other { .. } => return None,
        })
    }

    /// Runs one quantum in place.
    pub fn process(&mut self, block: &mut Block) {
        match self {
            Fx::ParamEq(e) => e.process(block),
            Fx::Compressor(e) => e.process(block),
            Fx::PeakLimiter(e) => e.process(block),
            Fx::Meter(e) => e.process(block),
            Fx::Gain(e) => e.process(block),
            Fx::RoomVerb(e) => e.process(block),
            Fx::StereoDelay(e) => e.process(block),
        }
    }

    /// Sets a parameter by the plugin's parameter id (a game-parameter curve's target): the value is base plus curves,
    /// in the units of [`Fx::base_param`]. An id the plugin does not take is ignored.
    pub fn set_param(&mut self, id: u16, value: f64) {
        match self {
            Fx::ParamEq(e) => e.set(id, value),
            Fx::Compressor(e) => e.set(id, value),
            _ => {}
        }
    }

    /// A parameter's value before any curve, in the units `set_param` takes (the compressor's output level in dB,
    /// the equaliser's band gain in dB, frequency in Hz after the Nyquist limit, Q, and output level in dB);
    /// None for an id the plugin does not take.
    pub fn base_param(&self, id: u16) -> Option<f64> {
        match self {
            Fx::ParamEq(e) => e.base(id),
            Fx::Compressor(e) => e.base(id),
            _ => None,
        }
    }

    /// A meter's output: the game parameter it writes and its value after the last quantum.
    pub fn meter(&self) -> Option<(Id, f64)> {
        match self {
            Fx::Meter(m) => Some((m.p.game_param, m.value())),
            _ => None,
        }
    }
}
