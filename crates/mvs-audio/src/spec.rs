//! What the interpreter tells the mixer: a voice to start ([`VoiceSpec`]) and a parameter to move
//! ([`ParamChange`]). Both are fixed-size (the mixer thread allocates nothing) and both are what the voice tape
//! compares, field for field, against the recorded voice graph.

use mvs_data::sound::{Curve, Id};
use serde::{Deserialize, Serialize};

/// The most input channels a voice pans (7.1).
pub const MAX_CHANNELS: usize = 8;
/// The most aux sends a voice has: four user-defined, four game-defined.
pub const MAX_SENDS: usize = 8;
/// The most insert effects a node chain gives a voice.
pub const MAX_INSERTS: usize = 4;

/// A voice, numbered by the interpreter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct VoiceId(pub u32);

/// A fade: its length (s) and AkInterpolation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Fade {
    pub secs: f64,
    pub curve: Curve,
}

/// An aux send: the aux bus and its gain (linear).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Send {
    pub bus: Id,
    pub gain: f64,
}

/// A voice to start. Times are the audio clock in samples at [`crate::RATE`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VoiceSpec {
    pub voice: VoiceId,
    /// When it starts (the initial delay included).
    pub at: u64,
    /// The medium (its `sound` entry by the bank's MediaRef).
    pub medium: Id,
    /// The medium's frame it starts from (a seek, a virtual voice played from its elapsed time).
    pub offset: u32,
    /// Times it plays (0: loops until stopped).
    pub loops: u16,
    /// The looped region, frames, end exclusive (None: the whole medium).
    pub loop_points: Option<[u32; 2]>,
    /// Pitch, cents.
    pub cents: f64,
    /// The output bus.
    pub bus: Id,
    /// The voice's static volume, dB: its chain's Volume, the ranges drawn, without make-up and normalisation.
    pub vol: f64,
    /// MakeUpGain, dB.
    pub makeup: f64,
    /// The medium's loudness normalisation, dB (0 without).
    pub norm: f64,
    /// The output bus volume, dB.
    pub dry: f64,
    /// The game-parameter volume at the start (linear).
    pub rtpc_gain: f64,
    /// The play action's fade-in.
    pub fade_in: Fade,
    /// The voice's low-pass and high-pass, Wwise units (0..100).
    pub lpf: f64,
    pub hpf: f64,
    /// The node chain's live insert effects, resolved by the interpreter, in order: the build's only live voice
    /// insert is AkMeter. Each meter's value goes back to the interpreter ([`crate::ctl::Ctl::meter`]); when the
    /// voice ends the meter counts once more at its minimum.
    pub meters: [Option<mvs_data::sound::MeterParams>; MAX_INSERTS],
    /// Per input channel, its gains to the left and right outputs; `channels` of them are used.
    pub channels: u8,
    pub pan: [[f64; 2]; MAX_CHANNELS],
    /// The dry path's distance volume (linear).
    pub dist: f64,
    /// The aux sends; `sends_n` of them are used.
    pub sends_n: u8,
    pub sends: [Send; MAX_SENDS],
    /// Streamed (decoded on the mixer thread as it plays) rather than from the decode cache.
    pub streamed: bool,
}

/// What a change moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    Voice(VoiceId),
    Bus(Id),
    /// A bus's effect, by its slot.
    BusFx {
        bus: Id,
        slot: u8,
    },
}

/// The parameter a change moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Param {
    /// A voice's game-parameter volume (linear).
    RtpcGain,
    /// A voice's stop / transition ratio (linear; at 0 after a stop the voice ends).
    Fade,
    /// 0 while the voice is silent (forced virtual, paused or muted), else 1.
    Mute,
    /// Pitch, cents.
    Cents,
    /// The voice's node / bus-state low-pass and high-pass, Wwise units (the first filter pair).
    Lpf,
    Hpf,
    /// The distance attenuation's low-pass and high-pass, Wwise units: a second filter pair in series after the
    /// first.
    AttLpf,
    AttHpf,
    /// A channel's gain to an output side.
    Pan {
        ch: u8,
        side: u8,
    },
    Dist,
    /// An aux send's gain, by slot.
    Send(u8),
    /// A bus's volume, dB.
    BusVolume,
    /// A bus's state low-pass, Wwise units: the mixer adds it to the low-pass of every voice routed into the bus
    /// or a bus below it.
    BusLpf,
    /// A bus's HDR gain (linear): the interpreter runs the HDR window (`ctl::bus`) from the
    /// voices it started (their level, HDRActiveRange, the medium's peak and envelope in the bank's MediaRef) and
    /// sets the gain on the HDR bus.
    BusHdrGain,
    /// A bus's state MakeUpGain, dB, applied at the bus input.
    BusMakeUp,
    /// A bus's state BypassAllFX: 1 bypasses the bus's effects, 0 runs them.
    BusBypass,
    /// An effect's parameter, by the plugin's parameter id.
    FxParam(u16),
    /// The voice stops (value unused). A music clip's end is a Stop at its start plus its duration.
    Stop,
    /// The voice pauses (value 1) or resumes (0).
    Pause,
}

/// How a parameter reaches its value.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Ramp {
    /// At once.
    Set,
    /// Linear over s.
    Linear(f64),
    /// Exponential approach with this time constant, s.
    Target(f64),
    /// Along an AkInterpolation curve over s.
    Curve { secs: f64, curve: Curve },
    /// Along a curve begun before the change: from `from` at sample `start` to the change's value over `secs`. The
    /// change joins it part-way (a voice started while a shared fade runs, an AudioParam's ramp whose start has
    /// passed).
    Joined { from: f64, start: u64, secs: f64, curve: Curve },
}

/// A parameter change at an audio clock time (samples).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamChange {
    pub at: u64,
    pub target: Target,
    pub param: Param,
    pub value: f64,
    pub ramp: Ramp,
}

/// The changes of a parameter waiting for their time from a sample on dropped (Web Audio's
/// cancelScheduledValues): what is due at or after `from` and not taken yet.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cancel {
    pub from: u64,
    pub target: Target,
    pub param: Param,
}

/// What the interpreter emits, in order.
#[expect(clippy::large_enum_variant, reason = "fixed-size: the mixer thread allocates nothing")]
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Control {
    Start(VoiceSpec),
    Change(ParamChange),
    Cancel(Cancel),
}
