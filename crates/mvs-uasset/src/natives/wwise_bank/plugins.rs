//! The plugins a bank names (WwisePlugin) and the parameter blocks of the ones the build's banks hold (CUE4Parse
//! 1.2.2 UE4/Wwise/Plugins), and a sound's source (AkBankSourceData).

use super::enums::*;
use super::{W, obj, perr, uint};
use crate::archive::Result;
use crate::value::{Props, Value};

/// MathF.Pow(10f, x * 0.05f): the product in float, the power in double rounded to float.
fn db_to_lin(x: f32) -> Value {
    Value::Float(10f64.powf(f64::from(x * 0.05f32)) as f32)
}
fn pct(x: f32) -> Value {
    Value::Float(x * 0.01f32)
}
fn db(w: &mut W) -> Result<Value> {
    Ok(db_to_lin(w.f32()?))
}
fn pc(w: &mut W) -> Result<Value> {
    Ok(pct(w.f32()?))
}

fn filter_band(w: &mut W) -> Result<Value> {
    Ok(obj! {
        "FilterType" => AkFilterTypeOld.json(w.u32()?),
        "Gain" => w.f()?,
        "Frequency" => w.f()?,
        "QFactor" => w.f()?,
        "OnOff" => w.bv()?,
    })
}
fn filter_params(w: &mut W) -> Result<Value> {
    Ok(obj! {
        "FilterType" => AkFilterTypeOld.json(w.u32()?),
        "FilterGain" => w.f()?,
        "FilterFrequency" => w.f()?,
        "FilterQFactor" => w.f()?,
    })
}

/// A plugin's parameter block by its EAkPluginId name, for the plugins the build's banks hold.
fn plugin(name: &str) -> Option<fn(&mut W) -> Result<Value>> {
    Some(match name {
        "AkFxSrcSilenceSource" => silence,
        "AkParameterEQFX" => parameter_eq,
        "AkDelayFX" => delay,
        "AkCompressorFX" => compressor,
        "AkPeakLimiterFX" => peak_limiter,
        "AkGainFX" => gain,
        "AkMeterFX" => meter,
        "AkRoomVerbFX" => room_verb,
        "AkFlangerFX" => flanger,
        "AkHarmonizerFX" => harmonizer,
        "AkStereoDelayFX" => stereo_delay,
        _ => return None,
    })
}

fn silence(w: &mut W) -> Result<Value> {
    Ok(obj! {
        "Params" => obj! {
            "fDuration" => w.f()?,
            "fRandomizedLengthMinus" => w.f()?,
            "fRandomizedLengthPlus" => w.f()?,
        },
    })
}

fn parameter_eq(w: &mut W) -> Result<Value> {
    Ok(obj! {
        "Params" => obj! {
            "Bands" => w.arrv(3, filter_band)?,
            "OutputLevel" => w.f()?,
            "ProcessLFE" => w.bv()?,
        },
    })
}

fn delay(w: &mut W) -> Result<Value> {
    let delay_time = w.f()?;
    let feedback = pc(w)?;
    let wet_dry = pc(w)?;
    let output = db(w)?;
    let feedback_enabled = w.bv()?;
    let lfe = w.bv()?;
    Ok(obj! {
        "Params" => obj! {
            "RTPC" => obj! {
                "fFeedback" => feedback,
                "fWetDryMix" => wet_dry,
                "fOutputLevel" => output,
                "bFeedbackEnabled" => feedback_enabled,
            },
            "NonRTPC" => obj! { "fDelayTime" => delay_time, "bProcessLFE" => lfe },
        },
    })
}

fn compressor(w: &mut W) -> Result<Value> {
    let threshold = w.f()?;
    let ratio = w.f()?;
    let attack = w.f()?;
    let release = w.f()?;
    let output = db(w)?;
    let lfe = w.bv()?;
    let link = w.bv()?;
    Ok(obj! {
        "Params" => obj! {
            "Threshold" => threshold,
            "Ratio" => ratio,
            "Attack" => attack,
            "Release" => release,
            "OutputLevel" => output,
            "ChannelLinkPercentage" => Value::Int(0),
            "ProcessLFE" => lfe,
            "ChannelLink" => link,
            "SidechainGlobalScope" => Value::Bool(false),
            "SidechainId" => Value::Int(0),
        },
    })
}

fn peak_limiter(w: &mut W) -> Result<Value> {
    let threshold = w.f()?;
    let ratio = w.f()?;
    let look_ahead = w.f()?;
    let release = w.f()?;
    let output = db(w)?;
    let lfe = w.bv()?;
    let link = w.bv()?;
    Ok(obj! {
        "Params" => obj! {
            "RTPC" => obj! {
                "fThreshold" => threshold,
                "fRatio" => ratio,
                "fRelease" => release,
                "fOutputLevel" => output,
            },
            "NonRTPC" => obj! { "fLookAhead" => look_ahead, "bProcessLFE" => lfe, "bChannelLink" => link },
        },
    })
}

fn gain(w: &mut W) -> Result<Value> {
    Ok(obj! { "Params" => obj! { "fFullbandGain" => w.f()?, "fLFEGain" => w.f()? } })
}

fn meter(w: &mut W) -> Result<Value> {
    let rtpc = obj! {
        "fAttack" => w.f()?,
        "fRelease" => w.f()?,
        "fMin" => w.f()?,
        "fMax" => w.f()?,
        "fHold" => w.f()?,
        "bInfiniteHold" => Value::Bool(false),
    };
    let non_rtpc = obj! {
        "eMode" => AkMeterMode.json(u32::from(w.u8()?)),
        "eScope" => AkMeterScope.json(u32::from(w.u8()?)),
        "bApplyDownstreamVolume" => w.bv()?,
        "uGameParamID" => w.u32v()?,
    };
    Ok(obj! {
        "Params" => obj! { "RTPC" => rtpc, "NonRTPC" => non_rtpc },
        "BallisticParams" => Value::Null,
        "MeterParams" => Value::Null,
    })
}

fn room_verb(w: &mut W) -> Result<Value> {
    let mut rtpc = Props::new();
    for k in [
        "DecayTime",
        "HFDamping",
        "Diffusion",
        "StereoWidth",
        "Filter1Gain",
        "Filter1Freq",
        "Filter1Q",
        "Filter2Gain",
        "Filter2Freq",
        "Filter2Q",
        "Filter3Gain",
        "Filter3Freq",
        "Filter3Q",
    ] {
        rtpc.insert(k.to_owned(), w.f()?);
    }
    for k in ["FrontLevel", "RearLevel", "CenterLevel", "LFELevel", "DryLevel", "ERLevel"] {
        rtpc.insert(k.to_owned(), db(w)?);
    }
    // (float)Math.Pow(10.0, (double)x * 0.05 - 0.15): in double
    let x = w.f32()?;
    rtpc.insert("ReverbLevel".to_owned(), Value::Float(10f64.powf(f64::from(x) * 0.05 - 0.15) as f32));
    let mut inv = Props::new();
    inv.insert("bEnableEarlyReflections".to_owned(), w.bv()?);
    inv.insert("ERPattern".to_owned(), w.u32v()?);
    for k in ["ReverbDelay", "RoomSize", "ERFrontBackDelay", "Density", "RoomShape"] {
        inv.insert(k.to_owned(), w.f()?);
    }
    inv.insert("NumReverbUnits".to_owned(), w.u32v()?);
    inv.insert("bEnableToneControls".to_owned(), w.bv()?);
    for k in ["1", "2", "3"] {
        inv.insert(format!("Filter{k}Pos"), AkFilterInsertType.json(w.u32()?));
        inv.insert(format!("Filter{k}Curve"), AkFilterCurveType.json(w.u32()?));
    }
    inv.insert("InputCenterLevel".to_owned(), db(w)?);
    inv.insert("InputLFELevel".to_owned(), db(w)?);
    let mut algo = Props::new();
    for k in [
        "DensityDelayMin",
        "DensityDelayMax",
        "DensityDelayRdmPerc",
        "RoomShapeMin",
        "RoomShapeMax",
        "DiffusionDelayScalePerc",
        "DiffusionDelayMax",
        "DiffusionDelayRdmPerc",
        "DCFilterCutFreq",
        "ReverbUnitInputDelay",
        "ReverbUnitInputDelayRdmPerc",
    ] {
        algo.insert(k.to_owned(), w.f()?);
    }
    Ok(obj! {
        "Params" => obj! {
            "RTPCParams" => Value::Struct(rtpc),
            "InvariantParams" => Value::Struct(inv),
            "AlgoTunings" => Value::Struct(algo),
        },
    })
}

fn flanger(w: &mut W) -> Result<Value> {
    let delay_time = w.f()?;
    let dry = w.f()?;
    let ffwd = w.f()?;
    let fback = w.f()?;
    let mod_depth = pc(w)?;
    let frequency = w.f()?;
    let waveform = DSPLfoWaveform.json(w.u32()?);
    let smooth = pc(w)?;
    let pwm = pc(w)?;
    let phase = obj! {
        "PhaseOffset" => w.f()?,
        "PhaseMode" => DSPPhaseMode.json(w.u32()?),
        "PhaseSpread" => w.f()?,
    };
    let output = db(w)?;
    let wet_dry = w.f()?;
    let lfo = w.bv()?;
    let center = w.bv()?;
    let lfe = w.bv()?;
    Ok(obj! {
        "Params" => obj! {
            "RTPC" => obj! {
                "DryLevel" => dry,
                "FfwdLevel" => ffwd,
                "FbackLevel" => fback,
                "ModDepth" => mod_depth,
                "ModParams" => obj! {
                    "LfoParams" => obj! { "Frequency" => frequency, "Waveform" => waveform, "Smooth" => smooth, "PWM" => pwm },
                    "PhaseParams" => phase,
                },
                "OutputLevel" => output,
                "WetDryMix" => wet_dry,
                "HasChanged" => Value::Bool(false),
            },
            "NonRTPC" => obj! {
                "DelayTime" => delay_time,
                "EnableLFO" => lfo,
                "ProcessCenter" => center,
                "ProcessLFE" => lfe,
                "HasChanged" => Value::Bool(false),
            },
        },
    })
}

fn harmonizer(w: &mut W) -> Result<Value> {
    // AkPitchVoiceParams reads bEnable, the pitch, the gain, then the filter; its JSON has the declaration order
    let voice = w.arrv(2, |w| {
        let enable = w.bv()?;
        // (float)Math.Pow(2.0, x * 0.00083333335f): the product in float, the power in double
        let x = w.f32()? * 0.000_833_333_35_f32;
        let pitch = Value::Float(2f64.powf(f64::from(x)) as f32);
        let gain = db(w)?;
        Ok(obj! { "Filter" => filter_params(w)?, "fPitchFactor" => pitch, "fGain" => gain, "bEnable" => enable })
    })?;
    let input = AkInputType.json(w.u32()?);
    let dry = db(w)?;
    let wet = db(w)?;
    Ok(obj! {
        "Params" => obj! {
            "Voice" => voice,
            "InputType" => input,
            "DryLevel" => dry,
            "WetLevel" => wet,
            "WindowSize" => w.u32v()?,
            "ProcessLFE" => w.bv()?,
            "SyncDry" => w.bv()?,
        },
    })
}

fn stereo_delay(w: &mut W) -> Result<Value> {
    let ch = |w: &mut W| -> Result<Value> {
        Ok(obj! { "fDelayTime" => w.f()?, "fFeedback" => db(w)?, "fCrossFeed" => db(w)? })
    };
    let in0 = AkInputChannelType.json(w.u32()?);
    let c0 = ch(w)?;
    let in1 = AkInputChannelType.json(w.u32()?);
    let c1 = ch(w)?;
    let filter = filter_params(w)?;
    let dry = db(w)?;
    let wet = db(w)?;
    let balance = w.f()?;
    let feedback = w.bv()?;
    let cross = w.bv()?;
    Ok(obj! {
        "Params" => obj! {
            "eInputType" => Value::Array(vec![in0, in1]),
            "StereoDelayParams" => Value::Array(vec![c0, c1]),
            "FilterParams" => filter,
            "fDryLevel" => dry,
            "fWetLevel" => wet,
            "fFrontRearBalance" => balance,
            "bEnableFeedback" => feedback,
            "bEnableCrossFeed" => cross,
        },
    })
}

/// WwisePlugin.GetPluginId: 0 and 0xFFFFFFFF are None (raw 0xFFFFFFFF).
pub(super) fn plugin_id(w: &mut W) -> Result<u32> {
    let n = w.u32()?;
    Ok(if n == 0 || n == u32::MAX { u32::MAX } else { n })
}

pub(super) fn plugin_json(raw: u32) -> Value {
    let valid = raw != u32::MAX && raw != 0;
    obj! {
        "PluginId" => EAkPluginId.json(raw),
        "CompanyId" => AkCompanyID.json(if valid { (raw >> 4) & 0xff } else { 0 }),
        "Type" => EAkPluginType.json(if valid { raw & 0xf } else { 0 }),
    }
}

/// WwisePlugin.TryParsePluginParams (always = false): none for None or an id past int.MaxValue, or a zero size.
pub(super) fn plugin_params(w: &mut W, raw: u32) -> Result<Option<Value>> {
    if raw == 0 || raw.cast_signed() < 0 {
        return Ok(None);
    }
    let size = w.u32()?;
    if size == 0 {
        return Ok(None);
    }
    let start = w.ar.pos;
    let end = start as u64 + u64::from(size);
    let name = match EAkPluginId.json(raw) {
        Value::Str(s) => s,
        _ => raw.to_string(),
    };
    let Some(f) = plugin(&name) else {
        return Err(perr(format!("{}: plugin {name} ({raw}) is not ported", w.ar.name)));
    };
    let p = f(w)?;
    if w.ar.pos as u64 != end {
        return Err(perr(format!("{}: plugin {name} read {} of its {size} bytes", w.ar.name, w.ar.pos - start)));
    }
    Ok(Some(p))
}

/// AkBankSourceData.
pub(super) fn bank_source(w: &mut W) -> Result<Value> {
    let raw = plugin_id(w)?;
    let source_type = EAKBKSourceType.text_value(u32::from(w.u8()?));
    let source_id = w.u32v()?;
    let media_size = w.u32()?;
    let flags = w.u8()?;
    let mut o = Props::new();
    o.insert("Plugin".to_owned(), plugin_json(raw));
    o.insert("SourceType".to_owned(), source_type);
    o.insert("SourceId".to_owned(), source_id);
    if media_size != 0 {
        o.insert("InMemoryMediaSize".to_owned(), uint(media_size));
    }
    o.insert("BankSourceFlags".to_owned(), EBankSourceFlags.text_value(u32::from(flags)));
    let has = raw != u32::MAX && raw & 0xf == 2;
    match if has { plugin_params(w, raw)? } else { None } {
        Some(p) => o.insert("PluginParams".to_owned(), p),
        None => o.insert("HasPluginParams".to_owned(), Value::Bool(has)),
    };
    Ok(Value::Struct(o))
}
