//! An effect's parameters (`FxParams`) from the plugin's tree: the plugins the port runs typed, any other by its name.

use mvs_data::sound::*;
use mvs_uasset::value::Value;

use super::tree::*;

fn sub<'a>(v: &'a Value, k: &str, what: &str) -> R<&'a Value> {
    match get(v, k) {
        Some(s) => Ok(s),
        None => fail(what, format!("an effect's parameters have no {k}")),
    }
}

fn filter_type(v: Option<&Value>, what: &str) -> R<FilterType> {
    Ok(match str_of(v) {
        Some("None") => FilterType::None,
        Some("LowShelf") => FilterType::LowShelf,
        Some("PeakingEQ") => FilterType::PeakingEQ,
        Some("HighShelf") => FilterType::HighShelf,
        Some("LowPass") => FilterType::LowPass,
        Some("HighPass") => FilterType::HighPass,
        Some("BandPass") => FilterType::BandPass,
        Some("Notch") => FilterType::Notch,
        _ => return fail(what, format!("filter type {v:?} not handled")),
    })
}

fn tone_filter(inv: &Value, rt: &Value, n: u8, what: &str) -> R<ToneFilter> {
    let pos = match str_of(get(inv, &format!("Filter{n}Pos"))) {
        Some("Off") => 0,
        Some("EROnly") => 1,
        Some("ReverbOnly") => 2,
        Some("ERAndReverb") => 3,
        p => return fail(what, format!("tone filter place {p:?} not handled")),
    };
    let curve = match str_of(get(inv, &format!("Filter{n}Curve"))) {
        Some("LowShelf") => 0,
        Some("Peaking") => 1,
        Some("HighShelf") => 2,
        c => return fail(what, format!("tone filter curve {c:?} not handled")),
    };
    Ok(ToneFilter {
        pos,
        curve,
        gain: req_f(rt, &format!("Filter{n}Gain"), what)?,
        freq: req_f(rt, &format!("Filter{n}Freq"), what)?,
        q: req_f(rt, &format!("Filter{n}Q"), what)?,
    })
}

fn room_verb(p: &Value, what: &str) -> R<RoomVerbParams> {
    let rt = sub(p, "RTPCParams", what)?;
    let inv = sub(p, "InvariantParams", what)?;
    let alg = sub(p, "AlgoTunings", what)?;
    Ok(RoomVerbParams {
        decay_time: req_f(rt, "DecayTime", what)?,
        hf_damping: req_f(rt, "HFDamping", what)?,
        diffusion: req_f(rt, "Diffusion", what)?,
        stereo_width: req_f(rt, "StereoWidth", what)?,
        filters: [tone_filter(inv, rt, 1, what)?, tone_filter(inv, rt, 2, what)?, tone_filter(inv, rt, 3, what)?],
        front_level: req_f(rt, "FrontLevel", what)?,
        rear_level: req_f(rt, "RearLevel", what)?,
        center_level: req_f(rt, "CenterLevel", what)?,
        lfe_level: req_f(rt, "LFELevel", what)?,
        dry_level: req_f(rt, "DryLevel", what)?,
        er_level: req_f(rt, "ERLevel", what)?,
        reverb_level: req_f(rt, "ReverbLevel", what)?,
        enable_early_reflections: is_true(get(inv, "bEnableEarlyReflections")),
        er_pattern: req_int(inv, "ERPattern", what)?,
        reverb_delay: req_f(inv, "ReverbDelay", what)?,
        room_size: req_f(inv, "RoomSize", what)?,
        er_front_back_delay: req_f(inv, "ERFrontBackDelay", what)?,
        density: req_f(inv, "Density", what)?,
        room_shape: req_f(inv, "RoomShape", what)?,
        num_reverb_units: req_int(inv, "NumReverbUnits", what)?,
        enable_tone_controls: is_true(get(inv, "bEnableToneControls")),
        input_center_level: req_f(inv, "InputCenterLevel", what)?,
        input_lfe_level: req_f(inv, "InputLFELevel", what)?,
        density_delay_min: req_f(alg, "DensityDelayMin", what)?,
        density_delay_max: req_f(alg, "DensityDelayMax", what)?,
        density_delay_rdm_perc: req_f(alg, "DensityDelayRdmPerc", what)?,
        room_shape_min: req_f(alg, "RoomShapeMin", what)?,
        room_shape_max: req_f(alg, "RoomShapeMax", what)?,
        diffusion_delay_scale_perc: req_f(alg, "DiffusionDelayScalePerc", what)?,
        diffusion_delay_max: req_f(alg, "DiffusionDelayMax", what)?,
        diffusion_delay_rdm_perc: req_f(alg, "DiffusionDelayRdmPerc", what)?,
        dc_filter_cut_freq: req_f(alg, "DCFilterCutFreq", what)?,
        reverb_unit_input_delay: req_f(alg, "ReverbUnitInputDelay", what)?,
        reverb_unit_input_delay_rdm_perc: req_f(alg, "ReverbUnitInputDelayRdmPerc", what)?,
    })
}

fn stereo_delay(p: &Value, what: &str) -> R<StereoDelayParams> {
    let inputs = list(p, "eInputType");
    let lines = list(p, "StereoDelayParams");
    if inputs.len() != 2 || lines.len() != 2 {
        return fail(what, "a stereo delay without two lines");
    }
    let line = |i: usize| -> R<DelayLine> {
        Ok(DelayLine {
            delay: req_f(&lines[i], "fDelayTime", what)?,
            feedback: req_f(&lines[i], "fFeedback", what)?,
            cross_feed: req_f(&lines[i], "fCrossFeed", what)?,
            left_or_right: str_of(inputs.get(i)) == Some("LEFT_OR_RIGHT"),
        })
    };
    let flt = sub(p, "FilterParams", what)?;
    Ok(StereoDelayParams {
        lines: [line(0)?, line(1)?],
        filter: filter_type(get(flt, "FilterType"), what)?,
        filter_gain: req_f(flt, "FilterGain", what)?,
        filter_frequency: req_f(flt, "FilterFrequency", what)?,
        filter_q: req_f(flt, "FilterQFactor", what)?,
        dry: req_f(p, "fDryLevel", what)?,
        wet: req_f(p, "fWetLevel", what)?,
        front_rear_balance: req_f(p, "fFrontRearBalance", what)?,
        enable_feedback: is_true(get(p, "bEnableFeedback")),
        enable_cross_feed: is_true(get(p, "bEnableCrossFeed")),
    })
}

/// The parameters of plugin `plugin` (`params`: the tree's `Params`).
pub(super) fn fx_params(plugin: &str, params: Option<&Value>, what: &str) -> R<FxParams> {
    let typed = matches!(
        plugin,
        "AkParameterEQFX"
            | "AkCompressorFX"
            | "AkPeakLimiterFX"
            | "AkMeterFX"
            | "AkGainFX"
            | "AkStereoDelayFX"
            | "AkRoomVerbFX"
            | "AkDelayFX"
    );
    if !typed {
        return Ok(FxParams::Other { name: plugin.to_owned() });
    }
    let Some(p) = params else { return fail(what, format!("{plugin} without parameters")) };
    Ok(match plugin {
        "AkParameterEQFX" => {
            let mut bands = Vec::new();
            for b in list(p, "Bands") {
                bands.push(EqBand {
                    filter: filter_type(get(b, "FilterType"), what)?,
                    gain: req_f(b, "Gain", what)?,
                    frequency: req_f(b, "Frequency", what)?,
                    q: req_f(b, "QFactor", what)?,
                    on: is_true(get(b, "OnOff")),
                });
            }
            FxParams::AkParameterEQFX(ParamEqParams {
                bands,
                output_level: req_f(p, "OutputLevel", what)?,
                process_lfe: is_true(get(p, "ProcessLFE")),
            })
        }
        "AkCompressorFX" => FxParams::AkCompressorFX(CompressorParams {
            threshold: req_f(p, "Threshold", what)?,
            ratio: req_f(p, "Ratio", what)?,
            attack: req_f(p, "Attack", what)?,
            release: req_f(p, "Release", what)?,
            output_level: req_f(p, "OutputLevel", what)?,
            channel_link: is_true(get(p, "ChannelLink")),
            process_lfe: is_true(get(p, "ProcessLFE")),
        }),
        "AkPeakLimiterFX" => {
            let (rt, non) = (sub(p, "RTPC", what)?, sub(p, "NonRTPC", what)?);
            FxParams::AkPeakLimiterFX(PeakLimiterParams {
                threshold: req_f(rt, "fThreshold", what)?,
                ratio: req_f(rt, "fRatio", what)?,
                release: req_f(rt, "fRelease", what)?,
                output_level: req_f(rt, "fOutputLevel", what)?,
                look_ahead: req_f(non, "fLookAhead", what)?,
                process_lfe: is_true(get(non, "bProcessLFE")),
                channel_link: is_true(get(non, "bChannelLink")),
            })
        }
        "AkMeterFX" => {
            let (rt, non) = (sub(p, "RTPC", what)?, sub(p, "NonRTPC", what)?);
            FxParams::AkMeterFX(MeterParams {
                attack: req_f(rt, "fAttack", what)?,
                release: req_f(rt, "fRelease", what)?,
                min: req_f(rt, "fMin", what)?,
                max: req_f(rt, "fMax", what)?,
                hold: req_f(rt, "fHold", what)?,
                infinite_hold: is_true(get(rt, "bInfiniteHold")),
                peak: str_of(get(non, "eMode")) == Some("Peak"),
                global: str_of(get(non, "eScope")) == Some("Global"),
                apply_downstream_volume: is_true(get(non, "bApplyDownstreamVolume")),
                game_param: req_int(non, "uGameParamID", what)?,
            })
        }
        "AkGainFX" => FxParams::AkGainFX(GainParams {
            fullband: req_f(p, "fFullbandGain", what)?,
            lfe: req_f(p, "fLFEGain", what)?,
        }),
        "AkStereoDelayFX" => FxParams::AkStereoDelayFX(stereo_delay(p, what)?),
        "AkRoomVerbFX" => FxParams::AkRoomVerbFX(Box::new(room_verb(p, what)?)),
        _ => {
            let (rt, non) = (sub(p, "RTPC", what)?, sub(p, "NonRTPC", what)?);
            FxParams::AkDelayFX(DelayParams {
                feedback: req_f(rt, "fFeedback", what)?,
                wet_dry_mix: req_f(rt, "fWetDryMix", what)?,
                output_level: req_f(rt, "fOutputLevel", what)?,
                feedback_enabled: is_true(get(rt, "bFeedbackEnabled")),
                delay: req_f(non, "fDelayTime", what)?,
                process_lfe: is_true(get(non, "bProcessLFE")),
            })
        }
    })
}
