//! The enums a bank's tree writes by name (CUE4Parse 1.2.2 UE4/Wwise), each its flags attribute and its
//! `(name, value)` pairs in declaration order, and the two ways the tree writes one: Newtonsoft's StringEnumConverter
//! ([`Enum::json`]) and .NET's Enum.ToString ([`Enum::text`]).
#![allow(non_upper_case_globals)]

use std::sync::OnceLock;

use crate::value::Value;

/// One .NET enum.
pub struct Enum {
    flags: bool,
    items: &'static [(&'static str, u32)],
    /// .NET's enum info: the values sorted unsigned, a tie kept in declaration order.
    sorted: OnceLock<(Vec<u32>, Vec<&'static str>)>,
}

impl Enum {
    pub const fn new(flags: bool, items: &'static [(&'static str, u32)]) -> Enum {
        Enum { flags, items, sorted: OnceLock::new() }
    }

    fn info(&self) -> (&[u32], &[&'static str]) {
        let (v, n) = self.sorted.get_or_init(|| {
            let mut m: Vec<(u32, &'static str)> = self.items.iter().map(|&(n, v)| (v, n)).collect();
            m.sort_by_key(|&(v, _)| v);
            m.into_iter().unzip()
        });
        (v, n)
    }

    /// Array.BinarySearch over the sorted values: among equal values, the one the search meets first.
    fn exact(&self, v: u32) -> Option<&'static str> {
        let (values, names) = self.info();
        let (mut lo, mut hi) = (0i64, values.len() as i64 - 1);
        while lo <= hi {
            let mid = lo + ((hi - lo) >> 1);
            let c = values[mid as usize];
            if c == v {
                return Some(names[mid as usize]);
            }
            if c < v {
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
        None
    }

    /// The greedy flags format .NET and Newtonsoft share: names from the highest value down, joined in ascending
    /// order; none when bits are left over.
    fn greedy(&self, v: u32) -> Option<String> {
        let (values, names) = self.info();
        let mut r = v;
        let mut parts: Vec<&str> = Vec::new();
        for i in (0..values.len()).rev() {
            let c = values[i];
            if i == 0 && c == 0 {
                break;
            }
            if r & c == c {
                r -= c;
                parts.push(names[i]);
            }
        }
        parts.reverse();
        (r == 0).then(|| parts.join(", "))
    }

    /// Newtonsoft's StringEnumConverter: the name, or the number when none fits.
    pub fn json(&self, v: u32) -> Value {
        let s = if !self.flags {
            self.exact(v).map(str::to_owned)
        } else if v == 0 {
            let (values, names) = self.info();
            (values.first() == Some(&0)).then(|| names[0].to_owned())
        } else {
            self.greedy(v)
        };
        match s {
            Some(s) => Value::Str(s),
            None => Value::UInt(u64::from(v)),
        }
    }

    /// .NET's Enum.ToString(): the name, or the number as text.
    pub fn text(&self, v: u32) -> String {
        if !self.flags {
            return self.exact(v).map_or_else(|| v.to_string(), str::to_owned);
        }
        let (values, names) = self.info();
        if v == 0 {
            return if values.first() == Some(&0) { names[0].to_owned() } else { "0".to_owned() };
        }
        for i in (0..values.len()).rev() {
            if values[i] < v {
                break;
            }
            if values[i] == v {
                return names[i].to_owned();
            }
        }
        self.greedy(v).unwrap_or_else(|| v.to_string())
    }

    /// [`Enum::text`] as a value.
    pub fn text_value(&self, v: u32) -> Value {
        Value::Str(self.text(v))
    }
}

pub static AkCompanyID: Enum = Enum::new(
    false,
    &[
        ("PluginDevMin", 64),
        ("PluginDevMax", 255),
        ("Audiokinetic", 0),
        ("AudiokineticExternal", 1),
        ("McDsp", 256),
        ("WaveArts", 257),
        ("PhoneticArts", 258),
        ("Izotope", 259),
        ("CrankcaseAudio", 261),
        ("Iosono", 262),
        ("AuroTechnologies", 263),
        ("Dolby", 264),
        ("TwoBigEars", 265),
        ("Oculus", 266),
        ("BlueRippleSound", 267),
        ("Enzien", 268),
        ("Krotos", 269),
        ("Nurulize", 270),
        ("SuperPowered", 271),
        ("Google", 272),
        ("Nvidia", 273),
        ("Reserved", 274),
        ("Microsoft", 275),
        ("Yamaha", 276),
        ("Visisonics", 277),
    ],
);
pub static AkFilterCurveType: Enum = Enum::new(false, &[("LowShelf", 0), ("Peaking", 1), ("HighShelf", 2)]);
pub static AkFilterInsertType: Enum =
    Enum::new(false, &[("Off", 0), ("EROnly", 1), ("ReverbOnly", 2), ("ERAndReverb", 3)]);
pub static AkFilterTypeOld: Enum = Enum::new(
    false,
    &[
        ("LowShelf", 0),
        ("PeakingEQ", 1),
        ("HighShelf", 2),
        ("LowPass", 3),
        ("HighPass", 4),
        ("BandPass", 5),
        ("Notch", 6),
    ],
);
pub static AkInputChannelType: Enum =
    Enum::new(false, &[("LEFT_OR_RIGHT", 0), ("CENTER", 1), ("DOWNMIX", 2), ("NONE", 3)]);
pub static AkInputType: Enum = Enum::new(
    false,
    &[("AsInput", 0), ("Center", 1), ("Stereo", 2), ("ThreePointZero", 3), ("FourPointZero", 4), ("FivePointZero", 5)],
);
pub static AkMeterMode: Enum = Enum::new(false, &[("Peak", 0), ("RMS", 1)]);
pub static AkMeterScope: Enum = Enum::new(false, &[("Global", 0), ("GameObject", 1)]);
pub static DSPLfoWaveform: Enum = Enum::new(
    false,
    &[("First", 0), ("Sine", 0), ("Triangle", 1), ("Square", 2), ("SawUp", 3), ("SawDown", 4), ("Num", 5)],
);
pub static DSPPhaseMode: Enum = Enum::new(false, &[("LeftRight", 0), ("FrontRear", 1), ("Circular", 2), ("Random", 3)]);
pub static EAKBKHircType: Enum = Enum::new(
    false,
    &[
        ("State", 1),
        ("SoundSfxVoice", 2),
        ("EventAction", 3),
        ("Event", 4),
        ("RandomSequenceContainer", 5),
        ("SwitchContainer", 6),
        ("ActorMixer", 7),
        ("AudioBus", 8),
        ("LayerContainer", 9),
        ("MusicSegment", 10),
        ("MusicTrack", 11),
        ("MusicSwitchContainer", 12),
        ("MusicRandomSequenceContainer", 13),
        ("Attenuation", 14),
        ("DialogueEvent", 15),
        ("FxShareSet", 16),
        ("FxCustom", 17),
        ("AuxiliaryBus", 18),
        ("LFO", 19),
        ("Envelope", 20),
        ("AudioDevice", 21),
        ("TimeMod", 22),
        ("SidechainMix", 23),
        ("FeedbackBus", 128),
        ("FeedbackNode", 129),
    ],
);
pub static EAKBKSourceType: Enum = Enum::new(false, &[("Data", 0), ("PrefetchStreaming", 1), ("Streaming", 2)]);
pub static EAdvSettingsFlags: Enum = Enum::new(
    true,
    &[
        ("None", 0),
        ("KillNewest", 1),
        ("UseVirtualBehavior", 2),
        ("UnknownFlag2", 4),
        ("IgnoreParentMaxNumInst", 8),
        ("VVoicesOptOverrideParent", 16),
    ],
);
pub static EAkActionScope: Enum = Enum::new(
    false,
    &[
        ("None", 0),
        ("GameObject", 1),
        ("Global", 2),
        ("GameObjectId", 3),
        ("All", 4),
        ("GlobalGameObject", 5),
        ("AllExceptId", 9),
        ("Ducking", 32),
    ],
);
pub static EAkAdvSettingsFlags: Enum = Enum::new(
    true,
    &[
        ("None", 0),
        ("KillNewest", 1),
        ("UseVirtualBehavior", 2),
        ("IgnoreParentMaxNumInst", 8),
        ("IsVVoicesOptOverrideParent", 16),
        ("IsMaxNumInstOverrideParent", 8),
    ],
);
pub static EAkBankTypeEnum: Enum = Enum::new(false, &[("User", 0), ("Event", 30), ("Bus", 31)]);
pub static EAkBelowThresholdBehavior: Enum = Enum::new(
    false,
    &[("ContinueToPlay", 0), ("KillVoice", 1), ("SetAsVirtualVoice", 2), ("KillIfOneShotElseVirtual", 3)],
);
pub static EAkBuiltInParam: Enum = Enum::new(
    false,
    &[
        ("None", 0),
        ("Start", 1),
        ("Distance", 1),
        ("Azimuth", 2),
        ("Elevation", 3),
        ("EmitterCone", 4),
        ("Obstruction", 5),
        ("Occlusion", 6),
        ("ListenerCone", 7),
        ("Diffraction", 8),
        ("TransmissionLoss", 9),
        ("Max", 10),
    ],
);
pub static EAkChannelConfig: Enum = Enum::new(
    false,
    &[
        ("SameasAudioDevice", 0),
        ("SameasMainMix", 3584),
        ("SameasPassThroughMix", 3840),
        ("AudioObjects", 768),
        ("Audio_1_0", 16641),
        ("Audio_2_0", 12546),
        ("Audio_2_1", 45315),
        ("Audio_3_0", 28931),
        ("Audio_4_0", 6304004),
        ("Audio_5_1", 6353158),
        ("Audio_7_1", 6549768),
        ("Audio_5_1_2", 90239240),
        ("Audio_5_1_4", 761327882),
        ("Audio_7_1_2", 90435850),
        ("Audio_7_1_4", 761524492),
        ("Ambisonics1storder", 516),
        ("Ambisonics2ndorder", 521),
        ("Ambisonics3rdorder", 528),
        ("Ambisonics4thorder", 537),
        ("Ambisonics5thorder", 548),
        ("Auro10_1", 769716491),
        ("Auro11_1", 803270924),
        ("Auro13_1", 803467534),
        ("LFE", 33025),
    ],
);
pub static EAkChannelConfigType: Enum = Enum::new(
    false,
    &[
        ("Anonymous", 0),
        ("Standard", 1),
        ("Ambisonic", 2),
        ("Objects", 3),
        ("Last", 4),
        ("UseDeviceMain", 14),
        ("UseDevicePassthrough", 15),
    ],
);
pub static EAkClipAutomationType: Enum =
    Enum::new(false, &[("Volume", 0), ("LPF", 1), ("HPF", 2), ("FadeIn", 3), ("FadeOut", 4)]);
pub static EAkContainerMode: Enum = Enum::new(false, &[("Random", 0), ("Sequence", 1)]);
pub static EAkCurveInterpolation: Enum = Enum::new(
    false,
    &[
        ("Log3", 0),
        ("Sine", 1),
        ("Log1", 2),
        ("InvSCurve", 3),
        ("Linear", 4),
        ("SCurve", 5),
        ("Exp1", 6),
        ("SineRecip", 7),
        ("Exp3", 8),
        ("LastFadeCurve", 8),
        ("Constant", 9),
    ],
);
pub static EAkCurveScaling: Enum = Enum::new(false, &[("None", 0), ("dB", 2), ("Log", 3), ("dBToLin", 4)]);
pub static EAkDecisionTreeMode: Enum = Enum::new(false, &[("Mode_BestMatch", 0), ("Mode_Weighted", 1)]);
pub static EAkEntryType: Enum = Enum::new(
    false,
    &[("EntryMarker", 0), ("SameTime", 1), ("RandomMarker", 2), ("RandomUserMarker", 3), ("LastExitTime", 4)],
);
pub static EAkFilterBehavior: Enum = Enum::new(false, &[("Additive", 0), ("Maximum", 1)]);
pub static EAkGameSyncType: Enum = Enum::new(
    false,
    &[("GameParameter", 0), ("MIDIParameter", 1), ("Switch", 2), ("State", 3), ("Modulator", 4), ("Count", 5)],
);
pub static EAkGroupType: Enum = Enum::new(false, &[("Switch", 0), ("State", 1)]);
pub static EAkHdrEnvelopeFlags: Enum = Enum::new(
    true,
    &[
        ("None", 0),
        ("OverrideHdrEnvelope", 1),
        ("OverrideAnalysis", 2),
        ("NormalizeLoudness", 4),
        ("EnableEnvelope", 8),
    ],
);
pub static EAkJumpToSelType: Enum =
    Enum::new(false, &[("StartOfPlaylist", 0), ("SpecificItem", 1), ("LastPlayedSegment", 2), ("NextSegment", 3)]);
pub static EAkMusicTrackType: Enum = Enum::new(false, &[("Normal", 0), ("Random", 1), ("Sequence", 2), ("Switch", 3)]);
pub static EAkPathMode: Enum =
    Enum::new(false, &[("StepSequence", 0), ("StepRandom", 1), ("ContinuousSequence", 2), ("ContinuousRandom", 3)]);
pub static EAkPluginId: Enum = Enum::new(
    false,
    &[
        ("None", 0),
        ("BANK", 1),
        ("PCM", 65537),
        ("ADPCM", 131073),
        ("XMA", 196609),
        ("VORBIS", 262145),
        ("WIIADPCM", 327681),
        ("PCMEX", 458753),
        ("EXTERNAL_SOURCE", 524289),
        ("XWMA", 589825),
        ("AAC", 655361),
        ("FILE_PACKAGE", 720897),
        ("ATRAC9", 786433),
        ("VAG_HE_VAG", 851969),
        ("PROFILERCAPTURE", 917505),
        ("ANALYSISFILE", 983041),
        ("MIDI", 1048577),
        ("OPUSNX", 1114113),
        ("CAF", 1179649),
        ("OPUS", 1245185),
        ("OPUS_WEM", 1310721),
        ("OPUS_WEM1", 1376257),
        ("SONY360", 1441793),
        ("AkFxSrcSineSource", 6553602),
        ("AkFxSrcSilenceSource", 6619138),
        ("AkToneSource", 6684674),
        ("AkParameterEQFX", 6881283),
        ("AkDelayFX", 6946819),
        ("AkCompressorFX", 7077891),
        ("AkExpanderFX", 7143427),
        ("AkPeakLimiterFX", 7208963),
        ("AkMatrixReverbFX", 7536643),
        ("AkSoundSeedImpactFX", 7602179),
        ("AkRoomVerbFX", 7733251),
        ("AkSoundSeedWind", 7798786),
        ("AkSoundSeedWoosh", 7864322),
        ("AkFlangerFX", 8192003),
        ("AkGuitarDistortionFX", 8257539),
        ("AkConvolutionReverbFX", 8323075),
        ("AkMeterFX", 8454147),
        ("AkTimeStretchFX", 8519683),
        ("AkTremoloFX", 8585219),
        ("AkRecorderFX", 8650755),
        ("AkStereoDelayFX", 8847363),
        ("AkPitchShifterFX", 8912899),
        ("AkHarmonizerFX", 9043971),
        ("AkGainFX", 9109507),
        ("AkSynthOne", 9699330),
        ("ASIOSink", 11010055),
        ("MicrosoftHRTFSink", 11141127),
        ("AkReflectFX", 11206659),
        ("AkRouterMixer", 11272198),
        ("SystemSink", 11403271),
        ("DVRByPassSink", 11468807),
        ("CommunicationSink", 11534343),
        ("ControllerHeadphonesSink", 11599879),
        ("VoiceSink", 11665415),
        ("ControllerSpeakerSink", 11730951),
        ("AuxiliarySink", 11796487),
        ("NoOutputSink", 11862023),
        ("RemoteSystemSink", 11927559),
        ("AkSoundSeedGrain", 11993090),
        ("AkImpacterSource", 12058626),
        ("MasteringSuiteFX", 12189699),
        ("Ak3DAudioBedMixerFX", 12451843),
        ("AkChannelRouterFX", 12517379),
        ("AkSidechainSendFX", 12648451),
        ("AkSidechainRecvFX", 12713987),
        ("AkMultibandMeterFX", 12845059),
        ("AkRecorder_ADM", 12976131),
        ("AkAudioInputSource", 13107202),
        ("ASIOSource", 13369346),
        ("AkMotionGeneratorSource", 26542082),
        ("AkMotionGeneratorMotionSource", 26542085),
        ("AkMotionSourceSource", 26804226),
        ("AkMotionSource", 26804229),
        ("AkMotionSink", 33226759),
        ("AkSystemOutputMeta", 58982409),
        ("AkChannelRouterMeta", 59179017),
        ("atmokyEars", 70339),
        ("AudioSpectrumFX", 3026899),
        ("AuroHeadphoneFX", 72093811),
        ("AuroPannerMixer", 72159350),
        ("AuroPannerFX", 72224883),
        ("Bitcrush", 1027),
        ("bnsRadio", 60501971),
        ("CrankcaseAudioREVModelPlayer", 27267154),
        ("IgniterLive", 5312722),
        ("IgniterLiveSynth", 5378258),
        ("iZHybridReverbFX", 135219),
        ("iZTrashDistortionFX", 200755),
        ("iZTrashDelayFX", 266291),
        ("iZTrashDynamicsFX", 331827),
        ("iZTrashFiltersFX", 397363),
        ("iZTrashBoxModelerFX", 462899),
        ("iZTrashMultibandDistortionFX", 593971),
        ("AudioDataPassbackFX", 592184323),
        ("BarbDelayFX", 891945987),
        ("BarbRecorderFX", 2085946371),
        ("DrunkPMSource", 199218),
        ("McDSPLimiterFX", 6754307),
        ("McDSPFutzBoxFX", 7213059),
        ("TritonAcousticsSpatializer", 266547),
        ("TritonAcousticsSpread", 332083),
        ("TritonAcousticsReverb", 201017),
        ("MsSpatialSink", 11145527),
        ("OculusAttachableMixerInputFX", 32837795),
        ("OculusEndpointSink", 9703591),
        ("OculusEndpointMetadata", 1030361257),
        ("OculusEndpointExperimentalMetadata", 1030426793),
        ("OculusSpatializerMixer", 9572518),
        ("PolyspectralMBC", 79434099),
        ("ResonanceAudioRendererFX", 6557955),
        ("ResonanceAudioRoomEffectMixer", 13111558),
        ("ResonanceAudioRoomEffectFX", 13177091),
        ("SpeechEngine", 70530),
        ("TencentGMESendFX", 590090771),
        ("TencentGMESource", 589894162),
        ("TencentGMEReceiveSource", 590025234),
        ("TencentGMESessionFX", 590156306),
    ],
);
pub static EAkPluginType: Enum = Enum::new(
    false,
    &[
        ("None", 0),
        ("Codec", 1),
        ("Source", 2),
        ("Effect", 3),
        ("MotionDevice", 4),
        ("MotionSource", 5),
        ("Mixer", 6),
        ("Sink", 7),
        ("GlobalExtension", 8),
        ("Metadata", 9),
        ("Last", 10),
        ("Mask", 15),
    ],
);
pub static EAkPropID: Enum = Enum::new(
    false,
    &[
        ("FirstRtpc", 0),
        ("Volume", 0),
        ("Pitch", 1),
        ("LPF", 2),
        ("HPF", 3),
        ("BusVolume", 4),
        ("MakeUpGain", 5),
        ("Priority", 6),
        ("MuteRatio", 7),
        ("UserAuxSendVolume0", 8),
        ("UserAuxSendVolume1", 9),
        ("UserAuxSendVolume2", 10),
        ("UserAuxSendVolume3", 11),
        ("GameAuxSendVolume", 12),
        ("OutputBusVolume", 13),
        ("OutputBusHPF", 14),
        ("OutputBusLPF", 15),
        ("OutputBusHSF", 16),
        ("UserAuxSendLPF0", 17),
        ("UserAuxSendLPF1", 18),
        ("UserAuxSendLPF2", 19),
        ("UserAuxSendLPF3", 20),
        ("UserAuxSendHPF0", 21),
        ("UserAuxSendHPF1", 22),
        ("UserAuxSendHPF2", 23),
        ("UserAuxSendHPF3", 24),
        ("GameAuxSendLPF", 25),
        ("GameAuxSendHPF", 26),
        ("GameAuxSendHSF", 27),
        ("ReflectionBusVolume", 28),
        ("HDRBusThreshold", 29),
        ("HDRBusRatio", 30),
        ("HDRBusReleaseTime", 31),
        ("HDRActiveRange", 32),
        ("MidiTransposition", 33),
        ("MidiVelocityOffset", 34),
        ("PlaybackSpeed", 35),
        ("InitialDelay", 36),
        ("Positioning_Pan_X_2D", 37),
        ("Positioning_Pan_Y_2D", 38),
        ("Positioning_Pan_Z_2D", 39),
        ("Positioning_Pan_X_3D", 40),
        ("Positioning_Pan_Y_3D", 41),
        ("Positioning_Pan_Z_3D", 42),
        ("Positioning_CenterPercent", 43),
        ("Positioning_TypeBlend", 44),
        ("Positioning_EnableAttenuation", 45),
        ("Positioning_Cone_AttenuationOnOff", 46),
        ("Positioning_Cone_Attenuation", 47),
        ("Positioning_Cone_LPF", 48),
        ("Positioning_Cone_HPF", 49),
        ("BypassFX", 50),
        ("BypassAllFX", 51),
        ("Available_0", 52),
        ("MaxNumInstances", 53),
        ("BypassAllMetadata", 54),
        ("PlayMechanismSpecialTransitionsValue", 55),
        ("AttenuationDistanceScaling", 56),
        ("FirstNonRtpc", 57),
        ("LastRtpc", 56),
        ("PriorityDistanceOffset", 57),
        ("DelayTime", 58),
        ("TransitionTime", 59),
        ("Probability", 60),
        ("DialogueMode", 61),
        ("HDRBusGameParam", 62),
        ("HDRBusGameParamMin", 63),
        ("HDRBusGameParamMax", 64),
        ("MidiTrackingRootNote", 65),
        ("MidiPlayOnNoteType", 66),
        ("MidiKeyRangeMin", 67),
        ("MidiKeyRangeMax", 68),
        ("MidiVelocityRangeMin", 69),
        ("MidiVelocityRangeMax", 70),
        ("MidiChannelMask", 71),
        ("MidiTempoSource", 72),
        ("MidiTargetNode", 73),
        ("Loop", 74),
        ("AttenuationID", 75),
        ("NUM", 76),
    ],
);
pub static EAkRandomMode: Enum = Enum::new(false, &[("Normal", 0), ("Shuffle", 1)]);
pub static EAkRtpcAccum: Enum = Enum::new(
    false,
    &[("None", 0), ("Exclusive", 1), ("Additive", 2), ("Multiply", 3), ("Boolean", 4), ("Maximum", 5), ("Filter", 6)],
);
pub static EAkSyncType: Enum = Enum::new(
    false,
    &[
        ("Immediate", 0),
        ("NextGrid", 1),
        ("NextBar", 2),
        ("NextBeat", 3),
        ("NextMarker", 4),
        ("NextUserMarker", 5),
        ("EntryMarker", 6),
        ("ExitMarker", 7),
        ("ExitNever", 8),
        ("LastExitPosition", 9),
    ],
);
pub static EAkTransitionMode: Enum = Enum::new(
    false,
    &[
        ("Disabled", 0),
        ("CrossFadeAmp", 1),
        ("CrossFadePower", 2),
        ("Delay", 3),
        ("SampleAccurate", 4),
        ("TriggerRate", 5),
    ],
);
pub static EAkTransitionRampingType: Enum = Enum::new(false, &[("None", 0), ("SlewRate", 1), ("FilteringOverTime", 2)]);
pub static EAkValueMeaning: Enum = Enum::new(false, &[("Default", 0), ("Independent", 1), ("Offset", 2)]);
pub static EAkVirtualQueueBehavior: Enum =
    Enum::new(false, &[("FromBeginning", 0), ("FromElapsedTime", 1), ("Resume", 2)]);
pub static EAltValuesFlags: Enum = Enum::new(true, &[("None", 0), ("UAlignment", 16), ("BDeviceAllocated", 65536)]);
pub static EAuxParamsFlags: Enum = Enum::new(
    true,
    &[
        ("None", 0),
        ("OverrideGameAux", 1),
        ("OverridePriority", 2),
        ("OverrideUserAuxSends", 4),
        ("HasAux", 8),
        ("OverrideReflections", 16),
    ],
);
pub static EBankSourceFlags: Enum = Enum::new(
    true,
    &[
        ("None", 0),
        ("IsLanguageSpecific", 1),
        ("Prefetch", 2),
        ("ExternallySupplied", 4),
        ("NonCachable", 8),
        ("HasSource", 128),
    ],
);
pub static EBitsPositioningFlags: Enum = Enum::new(
    true,
    &[
        ("PositioningInfoOverrideParent", 1),
        ("HasListenerRelativeRouting", 2),
        ("Unknown2d_2", 4),
        ("Unknown2d_3", 8),
        ("Unknown3d_4", 16),
        ("Unknown3d_5", 32),
        ("Unknown3d_6", 64),
        ("Unknown3d_7", 128),
        ("Is3DPositioningAvailable_122", 8),
        ("Is3DPositioningAvailable_129", 16),
        ("PannerTypeMask", 12),
        ("PositionTypeMask", 96),
    ],
);
pub static EEventActionType_v72_to_v150: Enum = Enum::new(
    false,
    &[
        ("Stop", 1),
        ("Pause", 2),
        ("Resume", 3),
        ("Play", 4),
        ("PlayAndContinue", 5),
        ("Mute", 6),
        ("UnMute", 7),
        ("SetVoicePitch", 8),
        ("ResetVoicePitch", 9),
        ("SetVoiceVolume", 10),
        ("ResetVoiceVolume", 11),
        ("SetBusVolume", 12),
        ("ResetBusVolume", 13),
        ("SetVoiceLowPassFilter", 14),
        ("ResetVoiceLowPassFilter", 15),
        ("EnableState", 16),
        ("DisableState", 17),
        ("SetState", 18),
        ("SetGameParameter", 19),
        ("ResetGameParameter", 20),
        ("Event", 21),
        ("SetSwitch", 25),
        ("ToggleBypassEffect", 26),
        ("ResetBypassEffect", 27),
        ("Break", 28),
        ("Trigger", 29),
        ("Seek", 30),
        ("Release", 31),
        ("SetHighPassFilter", 32),
        ("PlayEvent", 33),
        ("ResetPlaylist", 34),
        ("PlayEventUnknown", 35),
        ("SetHighPassFilter2", 48),
        ("SetEffect", 49),
        ("ResetEffect", 50),
        ("BypassEffect", 51),
        ("ResetBypassEffect2", 52),
        ("BypassEffect2", 53),
        ("ResetBypassEffect3", 54),
        ("ToggleBypassEffect2", 55),
    ],
);
pub static EHdrEnvelopeFlags: Enum = Enum::new(
    true,
    &[
        ("None", 0),
        ("OverrideHdrEnvelope", 1),
        ("OverrideAnalysis", 2),
        ("NormalizeLoudness", 4),
        ("EnableEnvelope", 8),
    ],
);
pub static EHierarchyParameterType: Enum = Enum::new(
    false,
    &[
        ("VoiceVolume", 0),
        ("VoicePitch", 2),
        ("VoiceLowPass", 3),
        ("VoiceHighPass", 4),
        ("BusVolume", 5),
        ("MakeUpGain", 6),
        ("PlaybackPriority", 7),
        ("PlaybackPriorityOffset", 8),
        ("MotionToVolumeOffset", 9),
        ("MotionLowPass", 10),
        ("PositioningPannerX", 12),
        ("PositioningPannerY", 13),
        ("PositioningCenterPercentage", 14),
        ("ActionDelay", 15),
        ("ActionFadeInTime", 16),
        ("Probability", 17),
        ("OverrideAuxBus0Volume", 19),
        ("OverrideAuxBus1Volume", 20),
        ("OverrideAuxBus2Volume", 21),
        ("OverrideAuxBus3Volume", 22),
        ("GameDefinedAuxSendVolume", 23),
        ("OverrideBusVolume", 24),
        ("OverrideBusHighPassFilter", 25),
        ("OverrideBusLowPassFilter", 26),
        ("HdrThreshold", 27),
        ("HdrRatio", 28),
        ("HdrReleaseTime", 29),
        ("HdrOutputGameParam", 30),
        ("HdrOutputGameParamMin", 31),
        ("HdrOutputGameParamMax", 32),
        ("HdrEnvelopeActiveRange", 33),
        ("MidiNoteTrackingUnknown", 46),
        ("MidiTranspositionInt", 47),
        ("MidiVelocityOffsetInt", 48),
        ("MidiFiltersKeyRangeMin", 49),
        ("MidiFiltersKeyRangeMax", 50),
        ("MidiFiltersVelocityRangeMin", 51),
        ("MidiFiltersVelocityRangeMax", 52),
        ("PlaybackSpeed", 54),
        ("MidiClipTempoSourceIsFile", 55),
        ("LoopTimeUInt", 58),
        ("InitialDelay", 59),
    ],
);
pub static EMidiBehaviorFlags: Enum = Enum::new(
    true,
    &[
        ("None", 0),
        ("PriorityOverrideParent", 1),
        ("PriorityApplyDistFactor", 2),
        ("OverrideMidiEventsBehavior", 4),
        ("OverrideMidiNoteTracking", 8),
        ("EnableMidiNoteTracking", 16),
        ("IsMidiBreakLoopOnNoteOff", 32),
    ],
);
pub static EMusicFlags: Enum = Enum::new(
    true,
    &[("None", 0), ("OverrideParentMidiTempo", 2), ("OverrideParentMidiTarget", 4), ("MidiTargetTypeBus", 8)],
);
pub static EOnSwitchMode: Enum = Enum::new(false, &[("PlayToEnd", 0), ("Stop", 1)]);
pub static EPauseOptionsFlags: Enum = Enum::new(
    true,
    &[("None", 0), ("PausePendingResume", 1), ("ApplyToStateTransitions", 2), ("ApplyToDynamicSequence", 4)],
);
pub static EPlayListFlags: Enum = Enum::new(
    true,
    &[
        ("None", 0),
        ("IsUsingWeight", 1),
        ("ResetPlayListAtEachPlay", 2),
        ("IsRestartBackward", 4),
        ("IsContinuous", 8),
        ("IsGlobal", 16),
    ],
);
pub static EResumeOptionsFlags: Enum = Enum::new(
    true,
    &[("None", 0), ("IsMasterResume", 1), ("ApplyToStateTransitions", 2), ("ApplyToDynamicSequence", 4)],
);
