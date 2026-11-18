//! The sound kinds as the game reads them: `soundbank` (one typed entry per AkAudioEvent's bank, and the InitBank's)
//! and `sound` (one Wwise medium as its own Vorbis packets, no Ogg and no PCM). The records hold what the build's
//! SoundBanks hold of the actor-mixer hierarchy, the music hierarchy, the buses and their effects, as Wwise's
//! `CAkBankMgr` reads them.
//!
//! A hierarchy id is the object's Wwise id (`AkUniqueID`); a name the game posts or sets is its ShortID
//! ([`short_id`]). Maps keyed by id are in ascending id order: the order a JavaScript object's integer keys iterate in,
//! so a walk over them visits them in that order.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// A Wwise object id or ShortID.
pub type Id = u32;

/// The media kind.
pub const SOUND_KIND: &str = "sound";
/// Its version this reader reads.
pub const SOUND_VERSION: u64 = 2;
/// The bank kind.
pub const BANK_KIND: &str = "soundbank";
/// Its version this reader reads.
pub const BANK_VERSION: u64 = 4;
/// The InitBank's entry: the bus tree, the attenuations, the game parameters' defaults and the event index.
pub const INIT_BANK: &str = "/Game/WwiseAudio/InitBank.InitBank";

/// Wwise's ShortID of a name (`AK::SoundEngine::GetIDFromString`): FNV-1 32 of the ASCII-lowercased name.
pub fn short_id(name: &str) -> Id {
    let mut h: u32 = 2_166_136_261;
    for b in name.bytes() {
        h = h.wrapping_mul(16_777_619) ^ u32::from(b.to_ascii_lowercase());
    }
    h
}

// ---- curves -----------------------------------------------------------------------------------------------------

/// AkCurveInterpolation, by the build's names (the Wwise numbering 0..9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Curve {
    Log3,
    Sine,
    Log1,
    InvSCurve,
    #[default]
    Linear,
    SCurve,
    Exp1,
    SineRecip,
    Exp3,
    Constant,
}

/// AkCurveScaling of a conversion table: what its y values are stored as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scaling {
    #[default]
    None,
    /// Linear gain - 1 (the target is in dB).
    #[serde(rename = "dB")]
    Db,
    /// log10 of the value (frequencies).
    Log,
}

/// A conversion table's point: from x to y, and the interpolation to the next point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
    pub curve: Curve,
}

/// RtpcAccum of an effect parameter's curve.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Accum {
    #[default]
    Additive,
    Exclusive,
}

/// A game-parameter curve of a node, a bus or an effect (an RtpcList entry of type GameParameter): the game
/// parameter, the target (AkRTPC_ParameterID: 0 Volume, 2 Pitch, 3 LPF, 4 HPF, 5 BusVolume, 7 MakeUpGain, 16
/// MaxNumInstances, 17 Priority, ...; an effect's own parameter index on an effect) and its conversion table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rtpc {
    pub id: Id,
    pub param: u16,
    pub scaling: Scaling,
    pub pts: Vec<Pt>,
    /// An effect parameter's accumulation (None on a node's or a bus's curve).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accum: Option<Accum>,
}

// ---- the actor-mixer hierarchy -------------------------------------------------------------------------------------

/// A node's property bundle: (AkPropID, value), in the bank's order. The ids the port reads: 0 Volume, 2 Pitch,
/// 3 LPF, 4 HPF, 6 MakeUpGain, 7 Priority, 19-22 the user aux send volumes 0-3, 23 GameAuxSendVolume,
/// 33 HDRActiveRange, 58 Loop (0: infinite), 59 InitialDelay; on an action 15 DelayTime (ms), 16
/// TransitionTime (ms), 17 Probability.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Props(pub Vec<(u16, f64)>);

impl Props {
    /// The value of property `id`, if the bundle has it.
    pub fn get(&self, id: u16) -> Option<f64> {
        self.0.iter().find(|p| p.0 == id).map(|p| p.1)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A node's ranged properties (RandomizerModifier): (AkPropID, [min, max]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ranges(pub Vec<(u16, [f64; 2])>);

impl Ranges {
    pub fn get(&self, id: u16) -> Option<[f64; 2]> {
        self.0.iter().find(|p| p.0 == id).map(|p| p.1)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}
fn is_zero_u16(v: &u16) -> bool {
    *v == 0
}
fn is_false(v: &bool) -> bool {
    !*v
}

/// What every node of the actor-mixer and music hierarchies carries (NodeBaseParams): its parent, properties,
/// output bus, playback limit, virtual voice behaviour, HDR flags, game-parameter curves, live insert effects,
/// aux sends and positioning.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Node {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<Id>,
    #[serde(skip_serializing_if = "Props::is_empty")]
    pub p: Props,
    #[serde(skip_serializing_if = "Ranges::is_empty")]
    pub r: Ranges,
    /// OverrideBusId.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bus: Option<Id>,
    /// MaxNumInstance (0: no limit).
    #[serde(skip_serializing_if = "is_zero_u16")]
    pub max_inst: u16,
    /// Limit flags (CAkParameterNode::SetAdvSettingsParams): 1 kill newest, 2 use virtual voice behaviour, 4 global
    /// limit, 8 ignore parent limit, 16 virtual voice options override parent.
    #[serde(skip_serializing_if = "is_zero_u8")]
    pub lim: u8,
    /// AkBelowThresholdBehavior: 0 continue to play, 1 kill, 2 virtual, 3 kill if one-shot else virtual.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub below: Option<u8>,
    /// AkVirtualQueueBehavior: 0 from beginning, 1 from elapsed time, 2 resume.
    #[serde(skip_serializing_if = "is_zero_u8")]
    pub vq: u8,
    /// PriorityOverrideParent.
    #[serde(skip_serializing_if = "is_false")]
    pub prio_ov: bool,
    /// HdrEnvelopeFlags: 1 override envelope, 2 override analysis, 4 normalize loudness, 8 enable envelope.
    #[serde(skip_serializing_if = "is_zero_u8")]
    pub hdr: u8,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rtpc: Vec<Rtpc>,
    /// The live (not rendered) insert effects, FxShareSet / FxCustom ids, in slot order; empty when bypassed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fx: Vec<Id>,
    /// The user aux send buses as the bank lists them (0: an unused slot); empty without HasAux.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aux: Vec<Id>,
    /// OverrideUserAuxSends.
    #[serde(skip_serializing_if = "is_false")]
    pub aux_ov: bool,
    /// Game-defined aux sends: 1 overrides the parent's setting, 2 the sounds use them.
    #[serde(skip_serializing_if = "is_zero_u8")]
    pub gaux: u8,
    /// The attenuation share set (property 70's id).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub att: Option<Id>,
    /// Positioning override: [bits, 3D bits]. bits: 1 override parent, 2 listener-relative routing, 4 | 8 speaker
    /// panning type; 3D bits: 1 | 2 spatialization mode (0 none, 1 position, 2 position + orientation), 8
    /// attenuation enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos: Option<[u8; 2]>,
}

/// AkBankSourceData's stream type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SrcType {
    #[default]
    Data,
    Streaming,
    PrefetchStreaming,
}

/// CAkSound: one medium (or a source plugin).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sound {
    pub node: Node,
    /// The medium id (the `sound` kind's entry, named by the bank's [`MediaRef`]).
    pub src: Id,
    pub src_type: SrcType,
    /// A source plugin other than Vorbis (AkFxSrcSilenceSource), else None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
}

/// A random / sequence container's play mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayMode {
    #[default]
    Random,
    Sequence,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RandomMode {
    #[default]
    Normal,
    Shuffle,
}

/// A continuous container's transition mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionMode {
    #[default]
    Disabled,
    CrossFadeAmp,
    CrossFadePower,
    Delay,
    SampleAccurate,
    TriggerRate,
}

/// A playlist entry: the child and its weight.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaylistItem {
    pub id: Id,
    pub weight: u32,
}

/// CAkRanSeqCntr.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RanSeq {
    pub node: Node,
    pub children: Vec<Id>,
    pub playlist: Vec<PlaylistItem>,
    pub mode: PlayMode,
    pub random_mode: RandomMode,
    pub avoid_repeat: u16,
    pub loop_count: u16,
    /// PlaylistFlags: IsContinuous, IsGlobal, ResetPlayListAtEachPlay.
    pub continuous: bool,
    pub global: bool,
    pub reset_each_play: bool,
    pub transition: TransitionMode,
    /// TransitionTime and its modifiers, ms.
    pub transition_time: f64,
    pub transition_mod: [f64; 2],
}

/// A switch container's children for one switch value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SwitchPackage {
    pub switch: Id,
    pub nodes: Vec<Id>,
}

/// CAkSwitchCntr.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SwitchCntr {
    pub node: Node,
    pub children: Vec<Id>,
    /// 0 switch group (per game object), 1 state group.
    pub group_type: u8,
    pub group: Id,
    pub default: Id,
    pub packages: Vec<SwitchPackage>,
}

/// A layer container or an actor-mixer: a node and its children.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Parent {
    pub node: Node,
    pub children: Vec<Id>,
}

// ---- events and actions ---------------------------------------------------------------------------------------------

/// An action's type (AkActionType by the dump's names).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionKind {
    #[default]
    Play,
    PlayEvent,
    Stop,
    Pause,
    Resume,
    Mute,
    UnMute,
    SetState,
    SetSwitch,
    SetVoiceVolume,
    ResetGameParameter,
    Seek,
    Break,
}

/// An action's scope.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionScope {
    #[default]
    GameObjectId,
    GameObject,
    Global,
    GlobalGameObject,
    All,
}

/// CAkAction.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub kind: ActionKind,
    pub scope: ActionScope,
    /// ReferencedId: the node, event, bus or state it acts on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Id>,
    #[serde(default, skip_serializing_if = "Props::is_empty")]
    pub p: Props,
    /// ActionParams' FadeCurve, None where the action has no parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_curve: Option<Curve>,
    /// SetState: the state group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<Id>,
    /// The target is a bus (Pause / Resume / Mute / UnMute).
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_bus: bool,
    /// The ids the action leaves out (ExceptParams).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub except: Vec<Id>,
}

// ---- the music hierarchy ----------------------------------------------------------------------------------------------

/// MeterInfo: tempo (BPM), beats per bar, beat value, grid period and offset (ms).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Meter {
    pub tempo: f64,
    pub beats: u16,
    pub beat_value: u16,
    pub grid_period: f64,
    pub grid_offset: f64,
}

/// A transition rule's sync point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncType {
    #[default]
    Immediate,
    NextGrid,
    NextBar,
    NextBeat,
    NextMarker,
    NextUserMarker,
    EntryMarker,
    ExitMarker,
}

/// A transition rule's destination entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryType {
    #[default]
    EntryMarker,
    SameTime,
    RandomMarker,
    RandomUserMarker,
    LastExitTime,
}

/// A fade of a rule: time and offset (ms), curve.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MusicFade {
    pub t: f64,
    pub off: f64,
    pub curve: Curve,
}

/// A rule's source side: the fade-out, where it syncs, whether the post-exit plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RuleSrc {
    pub fade: MusicFade,
    pub sync: SyncType,
    pub post: bool,
}

/// A rule's destination side: the fade-in, where it enters, whether the pre-entry plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RuleDst {
    pub fade: MusicFade,
    pub entry: EntryType,
    pub pre: bool,
}

/// A rule's transition segment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RuleTrans {
    pub seg: Id,
    pub fade_in: MusicFade,
    pub fade_out: MusicFade,
    pub pre: bool,
    pub post: bool,
}

/// A music transition rule: its source and destination ids (-1 any, 0 nothing), both sides, its transition.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MusicRule {
    pub src: Vec<i64>,
    pub dst: Vec<i64>,
    pub s: RuleSrc,
    pub d: RuleDst,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trans: Option<RuleTrans>,
}

/// A node of a music switch container's decision tree: the switch value (or state), the node it plays (0 none) and
/// the tree's next argument's nodes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecisionNode {
    pub key: Id,
    pub node: Id,
    pub children: Vec<DecisionNode>,
}

/// CAkMusicSwitchCntr (Mode_BestMatch).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MusicSwitch {
    pub node: Node,
    pub children: Vec<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meter: Option<Meter>,
    /// The decision tree's arguments: switch or state groups, in order.
    pub args: Vec<Id>,
    pub tree: Vec<DecisionNode>,
    /// IsContinuePlayback.
    pub cont: bool,
    pub rules: Vec<MusicRule>,
}

/// A playlist item of a music random / sequence container: a segment (`seg` nonzero) or a group of items.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MusicItem {
    pub seg: Id,
    pub id: Id,
    pub loop_count: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_range: Option<[u16; 2]>,
    pub weight: u32,
    pub avoid: u16,
    pub shuffle: bool,
    /// eRSType of a group: 0 continuous sequence, 1 step sequence, 2 continuous random, 3 step random.
    pub rs: u8,
    pub children: Vec<MusicItem>,
}

/// CAkMusicRanSeqCntr.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MusicRanSeq {
    pub node: Node,
    pub children: Vec<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meter: Option<Meter>,
    /// The one root item.
    pub list: Vec<MusicItem>,
    pub rules: Vec<MusicRule>,
}

/// CAkMusicSegment: its tracks, duration and the entry / exit cues (ms).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MusicSegment {
    pub node: Node,
    pub children: Vec<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meter: Option<Meter>,
    pub dur: f64,
    pub entry: f64,
    pub exit: f64,
}

/// A music track's clip: its medium, play-at, begin / end trim, source duration (ms), and how its medium is held
/// (0 in memory, 1 streamed, 2 streamed with prefetch).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub src: Id,
    pub play_at: f64,
    pub begin_trim: f64,
    pub end_trim: f64,
    pub src_dur: f64,
    pub held: u8,
}

/// A clip automation's kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoType {
    #[default]
    Volume,
    Lpf,
    Hpf,
    FadeIn,
    FadeOut,
}

/// A clip automation: its clip, kind and points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClipAuto {
    pub clip: u32,
    pub kind: AutoType,
    pub pts: Vec<Pt>,
}

/// CAkMusicTrack (type Normal).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MusicTrack {
    pub node: Node,
    pub clips: Vec<Clip>,
    pub auto: Vec<ClipAuto>,
    /// LookAheadTime, ms.
    pub look: f64,
}

// ---- effects ---------------------------------------------------------------------------------------------------------

/// An effect's plugin and its parameters (an FxShareSet / FxCustom's PluginParams, or a bus effect's), typed for
/// the plugins the port runs; any other plugin by its name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "plugin")]
pub enum FxParams {
    AkParameterEQFX(ParamEqParams),
    AkCompressorFX(CompressorParams),
    AkPeakLimiterFX(PeakLimiterParams),
    AkMeterFX(MeterParams),
    AkGainFX(GainParams),
    AkStereoDelayFX(StereoDelayParams),
    AkRoomVerbFX(Box<RoomVerbParams>),
    AkDelayFX(DelayParams),
    /// A plugin the port does not run (AkFlangerFX, AkHarmonizerFX, AkFxSrcSilenceSource, ...).
    Other {
        name: String,
    },
}

/// The dumped filter type names (shifted against the plugins' own enums: the effect's port maps them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterType {
    #[default]
    None,
    LowShelf,
    PeakingEQ,
    HighShelf,
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    pub filter: FilterType,
    pub gain: f64,
    pub frequency: f64,
    pub q: f64,
    pub on: bool,
}

/// AkParametricEQ: parameter ids 5b + (0 type, 1 gain, 2 frequency, 3 Q, 4 on/off) for band b, 15 output level.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ParamEqParams {
    pub bands: Vec<EqBand>,
    /// dB.
    pub output_level: f64,
    pub process_lfe: bool,
}

/// AkCompressor: parameter ids 0 threshold, 1 ratio, 2 attack, 3 release, 4 output level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompressorParams {
    pub threshold: f64,
    pub ratio: f64,
    pub attack: f64,
    pub release: f64,
    /// Linear.
    pub output_level: f64,
    pub channel_link: bool,
    pub process_lfe: bool,
}

/// AkPeakLimiter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PeakLimiterParams {
    pub threshold: f64,
    pub ratio: f64,
    pub release: f64,
    /// Linear.
    pub output_level: f64,
    /// s.
    pub look_ahead: f64,
    pub process_lfe: bool,
    pub channel_link: bool,
}

/// AkMeter: its ballistics and the game parameter it writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MeterParams {
    pub attack: f64,
    pub release: f64,
    pub min: f64,
    pub max: f64,
    pub hold: f64,
    pub infinite_hold: bool,
    /// eMode Peak (false: RMS).
    pub peak: bool,
    /// eScope Global (false: game object).
    pub global: bool,
    pub apply_downstream_volume: bool,
    pub game_param: Id,
}

/// AkGain, dB.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GainParams {
    pub fullband: f64,
    pub lfe: f64,
}

/// One line of AkStereoDelay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DelayLine {
    /// s.
    pub delay: f64,
    pub feedback: f64,
    pub cross_feed: f64,
    /// eInputType LEFT_OR_RIGHT (false: center or another input).
    pub left_or_right: bool,
}

/// AkStereoDelay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StereoDelayParams {
    pub lines: [DelayLine; 2],
    pub filter: FilterType,
    pub filter_gain: f64,
    pub filter_frequency: f64,
    pub filter_q: f64,
    pub dry: f64,
    pub wet: f64,
    pub front_rear_balance: f64,
    pub enable_feedback: bool,
    pub enable_cross_feed: bool,
}

/// AkDelay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DelayParams {
    pub feedback: f64,
    pub wet_dry_mix: f64,
    pub output_level: f64,
    pub feedback_enabled: bool,
    /// s.
    pub delay: f64,
    pub process_lfe: bool,
}

/// A RoomVerb tone filter's place: 0 off, 1 early reflections only, 2 reverb only, 3 both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ToneFilter {
    pub pos: u8,
    /// 0 low shelf, 1 peaking, 2 high shelf.
    pub curve: u8,
    pub gain: f64,
    pub freq: f64,
    pub q: f64,
}

/// AkRoomVerb's parameters: RTPCParams, InvariantParams and AlgoTunings by the dump's names, in snake case.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RoomVerbParams {
    pub decay_time: f64,
    pub hf_damping: f64,
    pub diffusion: f64,
    pub stereo_width: f64,
    pub filters: [ToneFilter; 3],
    pub front_level: f64,
    pub rear_level: f64,
    pub center_level: f64,
    pub lfe_level: f64,
    pub dry_level: f64,
    pub er_level: f64,
    pub reverb_level: f64,
    pub enable_early_reflections: bool,
    pub er_pattern: u32,
    pub reverb_delay: f64,
    pub room_size: f64,
    pub er_front_back_delay: f64,
    pub density: f64,
    pub room_shape: f64,
    pub num_reverb_units: u32,
    pub enable_tone_controls: bool,
    pub input_center_level: f64,
    pub input_lfe_level: f64,
    pub density_delay_min: f64,
    pub density_delay_max: f64,
    pub density_delay_rdm_perc: f64,
    pub room_shape_min: f64,
    pub room_shape_max: f64,
    pub diffusion_delay_scale_perc: f64,
    pub diffusion_delay_max: f64,
    pub diffusion_delay_rdm_perc: f64,
    pub dc_filter_cut_freq: f64,
    pub reverb_unit_input_delay: f64,
    pub reverb_unit_input_delay_rdm_perc: f64,
}

/// An FxShareSet / FxCustom of a bank (a node's insert effect).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FxObj {
    pub fx: FxParams,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rtpc: Vec<Rtpc>,
}

// ---- objects ----------------------------------------------------------------------------------------------------------

/// One object of a bank's hierarchy, by its HIRC type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum Obj {
    Event {
        actions: Vec<Id>,
    },
    EventAction(Action),
    SoundSfxVoice(Sound),
    RandomSequenceContainer(RanSeq),
    SwitchContainer(SwitchCntr),
    LayerContainer(Parent),
    ActorMixer(Parent),
    MusicSwitchContainer(MusicSwitch),
    MusicRandomSequenceContainer(MusicRanSeq),
    MusicSegment(MusicSegment),
    MusicTrack(MusicTrack),
    FxShareSet(FxObj),
    FxCustom(FxObj),
    /// A type nothing plays from (TimeMod, State, LFO, ...): kept by its name, so an id that names one is found.
    Other {
        name: String,
    },
}

impl Obj {
    /// The node parameters of a hierarchy object (None for an event, an action, an effect or another type).
    pub fn node(&self) -> Option<&Node> {
        match self {
            Obj::SoundSfxVoice(s) => Some(&s.node),
            Obj::RandomSequenceContainer(c) => Some(&c.node),
            Obj::SwitchContainer(c) => Some(&c.node),
            Obj::LayerContainer(c) | Obj::ActorMixer(c) => Some(&c.node),
            Obj::MusicSwitchContainer(c) => Some(&c.node),
            Obj::MusicRandomSequenceContainer(c) => Some(&c.node),
            Obj::MusicSegment(c) => Some(&c.node),
            Obj::MusicTrack(c) => Some(&c.node),
            _ => None,
        }
    }

    /// The children of a container (ChildIds), empty for any other object.
    pub fn children(&self) -> &[Id] {
        match self {
            Obj::RandomSequenceContainer(c) => &c.children,
            Obj::SwitchContainer(c) => &c.children,
            Obj::LayerContainer(c) | Obj::ActorMixer(c) => &c.children,
            Obj::MusicSwitchContainer(c) => &c.children,
            Obj::MusicRandomSequenceContainer(c) => &c.children,
            Obj::MusicSegment(c) => &c.children,
            _ => &[],
        }
    }
}

// ---- attenuations, buses ------------------------------------------------------------------------------------------------

/// An attenuation curve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttCurve {
    pub scaling: Scaling,
    pub pts: Vec<Pt>,
}

/// An attenuation share set: the curve index per purpose (0 volume dry, 1 aux game-defined, 2 aux user-defined,
/// 3 LPF, 4 HPF, 5 spread, 6 focus; -1 none) and the curves. A cone is not read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Att {
    pub uses: [i8; 7],
    pub curves: Vec<AttCurve>,
}

/// A bus's HDR settings: HDRBusThreshold (dB), HDRBusRatio, HDRBusReleaseTime (s), exponential release.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hdr {
    pub threshold: f64,
    pub ratio: f64,
    pub release: f64,
    pub exponential: bool,
}

/// AkRoomVerb's early reflection taps of one side: [ms, gain].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ErTaps {
    pub left: Vec<[f64; 2]>,
    pub right: Vec<[f64; 2]>,
}

/// One effect of a bus's chain, its parameters' game-parameter curves and, for RoomVerb, its pattern's taps
/// (AkRoomVerb.dll's ER pattern table).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusFx {
    pub fx: FxParams,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rtpc: Vec<Rtpc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub er: Option<ErTaps>,
}

/// A state's settings on a bus: BusVolume (dB), the voices' low-pass (0..100), MakeUpGain (dB), BypassAllFX.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StateSet {
    pub vol: f64,
    pub lpf: f64,
    pub mk: f64,
    pub bypass: f64,
}

/// An InitBank bus (AudioBus or AuxiliaryBus).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<Id>,
    /// BusVolume, dB.
    pub vol: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hdr: Option<Hdr>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fx: Vec<BusFx>,
    /// Game-parameter curves on the bus (5 BusVolume; 0 Volume and 7 MakeUpGain belong to the voices routed in).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rtpc: Vec<Rtpc>,
    #[serde(skip_serializing_if = "is_zero_u16")]
    pub max_inst: u16,
    /// 1 kill newest, 2 use virtual voice behaviour, 4 ignore parent.
    #[serde(skip_serializing_if = "is_zero_u8")]
    pub lim: u8,
    /// State group -> state -> settings.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub states: BTreeMap<Id, BTreeMap<Id, StateSet>>,
    /// AuxiliaryBus (false: AudioBus).
    #[serde(skip_serializing_if = "is_false")]
    pub aux: bool,
}

// ---- media -----------------------------------------------------------------------------------------------------------------

/// A medium as a bank names it: its `sound` entry (the AkMediaAsset's build path; a localized one's English(US)
/// asset) and what the wem says of it: frames, rate, channels, the speaker mask of a source with more than two
/// channels, the 'smpl' loop points (frames, end exclusive), the 'akd ' analysis (loudness normalisation gain and
/// peak, dB; the envelope's [frame, attenuation dB] points).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MediaRef {
    pub path: String,
    pub frames: u32,
    pub rate: u32,
    pub channels: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_points: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub norm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<[f64; 2]>,
}

// ---- the entries -----------------------------------------------------------------------------------------------------------

/// The `soundbank` entry of an AkAudioEvent: what its own bank holds for the event. Banks merge at run time as the
/// sound engine merges loaded banks (an object is its first-loaded bank's copy; a container's children the union of
/// the loaded banks' lists).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EventBank {
    pub name: String,
    /// The event's ShortID.
    pub id: Id,
    /// MinimumDuration / MaximumDuration (s), None where the event data leaves one out.
    pub dur: [Option<f64>; 2],
    /// The event's own actions.
    pub actions: Vec<Id>,
    /// The other events the bank holds (those its PlayEvent actions post): their actions, by event id.
    pub posted: BTreeMap<Id, Vec<Id>>,
    /// The rest of the hierarchy the bank holds.
    pub objs: BTreeMap<Id, Obj>,
    /// The media its sounds play, by medium id.
    pub media: BTreeMap<Id, MediaRef>,
}

/// An event of the index: its bank's entry path, whether it is localized, its durations, and whether it starts or
/// ends something that does not end by itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventEntry {
    pub path: String,
    pub localized: bool,
    pub dur: [Option<f64>; 2],
    pub pairs: bool,
}

/// RTPCRampingParams of a game parameter: 1 slew rate (up, down per s), 2 filtering over time (up, down s).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RtpcRamp {
    pub kind: u8,
    pub up: f64,
    pub down: f64,
}

/// The InitBank's entry: what every scope holds, and the event index.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InitBank {
    pub buses: BTreeMap<Id, Bus>,
    pub atts: BTreeMap<Id, Att>,
    pub rtpc_defaults: BTreeMap<Id, f64>,
    pub rtpc_ramps: BTreeMap<Id, RtpcRamp>,
    /// State group -> DefaultTransitionTime (ms).
    pub state_groups: BTreeMap<Id, f64>,
    /// VolumeThreshold, dB.
    pub volume_threshold: f64,
    /// MaxNumVoicesLimitInternal.
    pub max_voices: u32,
    /// Every event of the build by its lower-case name, in index order.
    pub events: IndexMap<String, EventEntry>,
}

/// The magic of a bank entry: "MVSB", a u32 JSON length, the record as JSON.
pub const BANK_MAGIC: &[u8; 4] = b"MVSB";

#[derive(Debug, thiserror::Error)]
pub enum SoundError {
    #[error(transparent)]
    Store(#[from] mvs_store::StoreError),
    #[error("{kind}: the store's kind is version {found:?}, this reader reads {want} (run the import)")]
    Version { kind: &'static str, found: Option<u64>, want: u64 },
    #[error("{0}: {1}")]
    Format(String, String),
}

/// A bank record as its entry's bytes.
pub fn encode_bank<T: Serialize>(record: &T) -> Vec<u8> {
    let json = serde_json::to_vec(record).unwrap_or_else(|e| panic!("soundbank: a record does not serialize: {e}"));
    let mut out = Vec::with_capacity(8 + json.len());
    out.extend_from_slice(BANK_MAGIC);
    out.extend_from_slice(&u32::try_from(json.len()).expect("a bank record under 4 GB").to_le_bytes());
    out.extend_from_slice(&json);
    out
}

/// An entry's record (`what`: the entry's path, for the error).
pub fn decode_bank<T: DeserializeOwned>(bytes: &[u8], what: &str) -> Result<T, SoundError> {
    let bad = |m: String| SoundError::Format(what.to_owned(), m);
    if bytes.len() < 8 || &bytes[..4] != BANK_MAGIC {
        return Err(bad("not a soundbank entry".into()));
    }
    let n = u32::from_le_bytes(bytes[4..8].try_into().expect("4 bytes")) as usize;
    let json = bytes.get(8..8 + n).ok_or_else(|| bad("truncated".into()))?;
    serde_json::from_slice(json).map_err(|e| bad(e.to_string()))
}

fn check_version(store: &mvs_store::Store, kind: &'static str, want: u64) -> Result<(), SoundError> {
    let found = store.version(kind);
    if found == Some(want) { Ok(()) } else { Err(SoundError::Version { kind, found, want }) }
}

/// The InitBank from the store.
pub fn load_init(store: &mvs_store::Store) -> Result<InitBank, SoundError> {
    check_version(store, BANK_KIND, BANK_VERSION)?;
    decode_bank(&store.kind(BANK_KIND)?.bytes(INIT_BANK)?, INIT_BANK)
}

/// An event's bank from the kind's pack, by the index's path.
pub fn load_bank(pack: &mvs_store::Pack, path: &str) -> Result<EventBank, SoundError> {
    decode_bank(&pack.bytes(path)?, path)
}

/// [`load_bank`] once per store: the store's one copy of the bank kind's entry `path`, weighed by its size.
pub fn shared_bank(store: &mvs_store::Store, path: &str) -> Result<std::sync::Arc<EventBank>, SoundError> {
    let pack = store.kind(BANK_KIND)?;
    let size = pack.record(path).map_or(0, |r| r.size as usize);
    store.cached(&format!("{BANK_KIND}:{path}"), || load_bank(&pack, path), |_| size)
}

/// The `sound` pack, its version checked.
pub fn sound_pack(store: &mvs_store::Store) -> Result<std::sync::Arc<mvs_store::Pack>, SoundError> {
    check_version(store, SOUND_KIND, SOUND_VERSION)?;
    Ok(store.kind(SOUND_KIND)?)
}

// ---- the medium's entry ---------------------------------------------------------------------------------------------------

/// The magic of a medium's entry.
pub const MEDIUM_MAGIC: &[u8; 4] = b"MVSM";

/// What a medium's entry says of it besides its packets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediumHeader {
    pub rate: u32,
    pub channels: u16,
    /// The wem's speaker mask (0: none given).
    pub mask: u32,
    /// The medium's length in frames.
    pub frames: u32,
    /// The 'smpl' loop points, frames, end exclusive.
    pub loop_points: Option<[u32; 2]>,
    /// The 'akd ' loudness normalisation gain, dB (0 without one).
    pub norm: f64,
    /// The entry path of its Vorbis setup header in the same kind (`/Sound/Setup/<hash>`): media share them.
    pub setup: String,
    /// Its Vorbis identification header, rebuilt from the wem's 'fmt ' / 'vorb' chunks.
    pub ident: Vec<u8>,
}

/// A medium's entry, read: its header and a view of its packets.
///
/// Layout (little-endian): "MVSM", u32 rate, u16 channels, u16 flags (bit 0 loop points), u32 mask, u32 frames,
/// u32 loop start, u32 loop end, f64 norm, u32 packet count n, u16 setup path length, the path, u16 ident length,
/// the ident header, padding to 4; then n u32 packet ends (byte offsets into the packet data), n u32 granules (the
/// medium's frame at the end of each packet's decoded output, the stream's first packet giving none), then the
/// packet data, the wem's own Vorbis audio packets back to back.
#[derive(Clone, Debug)]
pub struct Medium<'a> {
    pub header: MediumHeader,
    ends: &'a [u8],
    granules: &'a [u8],
    data: &'a [u8],
}

impl Medium<'_> {
    pub fn packets(&self) -> usize {
        self.ends.len() / 4
    }

    fn word(s: &[u8], i: usize) -> u32 {
        u32::from_le_bytes(s[i * 4..i * 4 + 4].try_into().expect("4 bytes"))
    }

    /// Packet `i`'s bytes.
    pub fn packet(&self, i: usize) -> &[u8] {
        let start = if i == 0 { 0 } else { Self::word(self.ends, i - 1) as usize };
        &self.data[start..Self::word(self.ends, i) as usize]
    }

    /// The frame at the end of packet `i`'s decoded output.
    pub fn granule(&self, i: usize) -> u32 {
        Self::word(self.granules, i)
    }
}

/// A medium's entry from its header and packets (each packet's bytes and its granule).
pub fn encode_medium(h: &MediumHeader, packets: &[(&[u8], u32)]) -> Vec<u8> {
    let mut o = Vec::new();
    o.extend_from_slice(MEDIUM_MAGIC);
    o.extend_from_slice(&h.rate.to_le_bytes());
    o.extend_from_slice(&h.channels.to_le_bytes());
    o.extend_from_slice(&u16::from(h.loop_points.is_some()).to_le_bytes());
    o.extend_from_slice(&h.mask.to_le_bytes());
    o.extend_from_slice(&h.frames.to_le_bytes());
    let [ls, le] = h.loop_points.unwrap_or([0, 0]);
    o.extend_from_slice(&ls.to_le_bytes());
    o.extend_from_slice(&le.to_le_bytes());
    o.extend_from_slice(&h.norm.to_le_bytes());
    o.extend_from_slice(&u32::try_from(packets.len()).expect("packet count").to_le_bytes());
    o.extend_from_slice(&u16::try_from(h.setup.len()).expect("setup path length").to_le_bytes());
    o.extend_from_slice(h.setup.as_bytes());
    o.extend_from_slice(&u16::try_from(h.ident.len()).expect("ident length").to_le_bytes());
    o.extend_from_slice(&h.ident);
    while o.len() % 4 != 0 {
        o.push(0);
    }
    let mut end = 0u32;
    for (p, _) in packets {
        end += u32::try_from(p.len()).expect("packet length");
        o.extend_from_slice(&end.to_le_bytes());
    }
    for (_, g) in packets {
        o.extend_from_slice(&g.to_le_bytes());
    }
    for (p, _) in packets {
        o.extend_from_slice(p);
    }
    o
}

/// A medium's entry read (`what`: its path, for the error).
pub fn decode_medium<'a>(bytes: &'a [u8], what: &str) -> Result<Medium<'a>, SoundError> {
    let bad = |m: &str| SoundError::Format(what.to_owned(), m.to_owned());
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&'a [u8], SoundError> {
        let s = bytes.get(at..at + n).ok_or_else(|| bad("truncated"))?;
        at += n;
        Ok(s)
    };
    if take(4)? != MEDIUM_MAGIC {
        return Err(bad("not a medium entry"));
    }
    let u16_ = |s: &[u8]| u16::from_le_bytes(s.try_into().expect("2 bytes"));
    let u32_ = |s: &[u8]| u32::from_le_bytes(s.try_into().expect("4 bytes"));
    let rate = u32_(take(4)?);
    let channels = u16_(take(2)?);
    let flags = u16_(take(2)?);
    let mask = u32_(take(4)?);
    let frames = u32_(take(4)?);
    let ls = u32_(take(4)?);
    let le = u32_(take(4)?);
    let norm = f64::from_le_bytes(take(8)?.try_into().expect("8 bytes"));
    let n = u32_(take(4)?) as usize;
    let sl = u16_(take(2)?) as usize;
    let setup = std::str::from_utf8(take(sl)?).map_err(|_| bad("setup path not UTF-8"))?.to_owned();
    let il = u16_(take(2)?) as usize;
    let ident = take(il)?.to_vec();
    let pad = (4 - (44 + sl + il) % 4) % 4;
    take(pad)?;
    let ends = take(n * 4)?;
    let granules = take(n * 4)?;
    let len = if n == 0 { 0 } else { u32_(&ends[(n - 1) * 4..]) as usize };
    let data = take(len)?;
    let loop_points = (flags & 1 != 0).then_some([ls, le]);
    let header = MediumHeader { rate, channels, mask, frames, loop_points, norm, setup, ident };
    Ok(Medium { header, ends, granules, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_ids() {
        // an event's id in its bank (amb_map_scoobydoo_emit_creak)
        assert_eq!(short_id("amb_map_scoobydoo_emit_creak"), 1_395_959_413);
        assert_eq!(short_id("AMB_Map_ScoobyDoo_Emit_Creak"), 1_395_959_413);
    }

    #[test]
    fn medium_round_trip() {
        let h = MediumHeader {
            rate: 48_000,
            channels: 2,
            mask: 3,
            frames: 1000,
            loop_points: Some([10, 900]),
            norm: -3.25,
            setup: "/Sound/Setup/0123456789abcdef".into(),
            ident: vec![1, 2, 3, 4, 5],
        };
        let packets: [(&[u8], u32); 3] = [(&[9, 9], 0), (&[], 128), (&[7, 7, 7], 1000)];
        let bytes = encode_medium(&h, &packets);
        let m = decode_medium(&bytes, "test").unwrap();
        assert_eq!(m.header, h);
        assert_eq!(m.packets(), 3);
        for (i, (p, g)) in packets.iter().enumerate() {
            assert_eq!((m.packet(i), m.granule(i)), (*p, *g));
        }
        assert!(decode_medium(&bytes[..bytes.len() - 1], "test").is_err());
    }

    #[test]
    fn bank_round_trip() {
        let mut b = EventBank { name: "e".into(), id: short_id("e"), actions: vec![2], ..EventBank::default() };
        b.objs.insert(
            2,
            Obj::EventAction(Action { target: Some(3), fade_curve: Some(Curve::Log3), ..Action::default() }),
        );
        let node = Node { parent: Some(4), p: Props(vec![(58, 0.0)]), pos: Some([3, 10]), ..Node::default() };
        b.objs.insert(3, Obj::SoundSfxVoice(Sound { node, src: 5, ..Sound::default() }));
        b.objs.insert(6, Obj::Other { name: "TimeMod".into() });
        b.media.insert(5, MediaRef { path: "/Game/WwiseAudio/Media/5.5".into(), frames: 9, ..MediaRef::default() });
        let bytes = encode_bank(&b);
        assert_eq!(decode_bank::<EventBank>(&bytes, "test").unwrap(), b);
        let fx = BusFx { fx: FxParams::Other { name: "AkFlangerFX".into() }, rtpc: vec![], er: None };
        let bus = Bus { fx: vec![fx], ..Bus::default() };
        let init = InitBank { buses: [(1, bus)].into(), ..InitBank::default() };
        assert_eq!(decode_bank::<InitBank>(&encode_bank(&init), "test").unwrap(), init);
    }
}
