//! `compare --kind soundbank`: the kind's pack against the oracle store's. The oracle's entries are
//! JSON; each is converted to the typed records ([`mvs_data::sound`]) and
//! compared with ours by `==`. What the records do not hold is dropped in the conversion (a layer container's layers,
//! a switch container's other raw fields, an effect's raw tree beyond its parameters). The InitBank's `aux` flag has
//! no oracle value (it is the HIRC type): it is compared by the bus ids alone. A difference is located by a JSON walk
//! of the two records and counted by its path.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;

use mvs_data::sound::*;
use mvs_store::Pack;
use serde::Deserialize;
use serde::de::{DeserializeOwned, MapAccess, Visitor};
use serde_json::Value;

type R<T> = Result<T, String>;

// ---- the oracle's JSON as values ---------------------------------------------------------------------------------

fn g<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
    v.get(k).filter(|x| !x.is_null())
}

fn num(v: &Value, what: &str) -> R<f64> {
    v.as_f64().ok_or_else(|| format!("{what}: {v} is not a number"))
}

fn req_num(v: &Value, k: &str) -> R<f64> {
    num(g(v, k).ok_or_else(|| format!("no {k} in {v}"))?, k)
}

fn uint<T: TryFrom<u64>>(v: &Value, k: &str) -> R<Option<T>> {
    match g(v, k) {
        None => Ok(None),
        Some(x) => match x.as_u64().and_then(|n| T::try_from(n).ok()) {
            Some(n) => Ok(Some(n)),
            None => Err(format!("{k}: {x} is not an integer in range")),
        },
    }
}

fn req_uint<T: TryFrom<u64>>(v: &Value, k: &str) -> R<T> {
    uint(v, k)?.ok_or_else(|| format!("no {k} in {v}"))
}

fn flag(v: &Value, k: &str) -> bool {
    match g(v, k) {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(_) => true,
    }
}

fn arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    g(v, k).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn ids(v: &Value, k: &str) -> R<Vec<Id>> {
    arr(v, k)
        .iter()
        .map(|x| x.as_u64().and_then(|n| Id::try_from(n).ok()).ok_or_else(|| format!("{k}: id {x}")))
        .collect()
}

fn en<T: DeserializeOwned>(v: Option<&Value>, what: &str) -> R<T> {
    let v = v.ok_or_else(|| format!("{what}: missing"))?;
    serde_json::from_value(v.clone()).map_err(|e| format!("{what}: {v}: {e}"))
}

fn pts(v: &Value) -> R<Vec<Pt>> {
    let Some(a) = v.as_array() else { return Err(format!("points {v}")) };
    a.iter()
        .map(|p| {
            let p = p.as_array().filter(|p| p.len() == 3).ok_or_else(|| format!("point {p}"))?;
            Ok(Pt { x: num(&p[0], "x")?, y: num(&p[1], "y")?, curve: en(Some(&p[2]), "interpolation")? })
        })
        .collect()
}

fn rtpcs(v: &Value, k: &str) -> R<Vec<Rtpc>> {
    arr(v, k)
        .iter()
        .map(|x| {
            Ok(Rtpc {
                id: req_uint(x, "id")?,
                param: req_uint(x, "param")?,
                scaling: en(g(x, "scaling"), "scaling")?,
                pts: pts(g(x, "pts").ok_or("rtpc without pts")?)?,
                accum: match g(x, "accum") {
                    Some(a) => Some(en(Some(a), "accum")?),
                    None => None,
                },
            })
        })
        .collect()
}

/// The effect's own curves: the tree's RTPCs, game parameters only.
fn raw_rtpcs(list: &[Value]) -> R<Vec<Rtpc>> {
    let mut out = Vec::new();
    for x in list {
        if g(x, "RtpcType").and_then(Value::as_str) != Some("GameParameter") {
            continue;
        }
        let t = g(x, "ConversionTable").ok_or("an RTPC without a table")?;
        let gp: Vec<Value> = arr(t, "GraphPoints")
            .iter()
            .map(|p| Value::Array(vec![p["From"].clone(), p["To"].clone(), p["Interpolation"].clone()]))
            .collect();
        out.push(Rtpc {
            id: req_uint(x, "RtpcId")?,
            param: req_uint(x, "ParamId")?,
            scaling: en(g(t, "Scaling"), "scaling")?,
            pts: pts(&Value::Array(gp))?,
            accum: Some(en(g(x, "RtpcAccum"), "accum")?),
        });
    }
    Ok(out)
}

// ---- the actor-mixer and music objects -----------------------------------------------------------------------------------

fn node(o: &Value) -> R<Node> {
    let mut n = Node::default();
    if let Some(p) = g(o, "p").and_then(Value::as_object) {
        let mut m = BTreeMap::new();
        for (k, v) in p {
            m.insert(k.parse::<u16>().map_err(|e| e.to_string())?, num(v, "prop")?);
        }
        n.p = Props(m.into_iter().collect());
    }
    if let Some(r) = g(o, "r").and_then(Value::as_object) {
        let mut m = BTreeMap::new();
        for (k, v) in r {
            let v = v.as_array().filter(|v| v.len() == 2).ok_or("a range")?;
            m.insert(k.parse::<u16>().map_err(|e| e.to_string())?, [num(&v[0], "min")?, num(&v[1], "max")?]);
        }
        n.r = Ranges(m.into_iter().collect());
    }
    n.parent = uint(o, "parent")?;
    n.bus = uint(o, "bus")?;
    n.max_inst = uint(o, "maxInst")?.unwrap_or(0);
    n.lim = uint(o, "lim")?.unwrap_or(0);
    n.below = uint(o, "below")?;
    n.vq = uint(o, "vq")?.unwrap_or(0);
    n.prio_ov = flag(o, "prioOv");
    n.hdr = uint(o, "hdr")?.unwrap_or(0);
    n.rtpc = rtpcs(o, "rtpc")?;
    n.fx = ids(o, "fx")?;
    n.aux = ids(o, "aux")?;
    n.aux_ov = flag(o, "auxOv");
    n.gaux = uint(o, "gaux")?.unwrap_or(0);
    n.att = uint(o, "att")?;
    if let Some(p) = g(o, "pos") {
        let p = p.as_array().filter(|p| p.len() == 2).ok_or("a positioning")?;
        n.pos = Some([
            p[0].as_u64().and_then(|x| u8::try_from(x).ok()).ok_or("pos")?,
            p[1].as_u64().and_then(|x| u8::try_from(x).ok()).ok_or("pos")?,
        ]);
    }
    Ok(n)
}

fn meter(o: &Value) -> R<Option<Meter>> {
    let Some(m) = g(o, "meter") else { return Ok(None) };
    let m = m.as_array().filter(|m| m.len() == 5).ok_or("a meter")?;
    Ok(Some(Meter {
        tempo: num(&m[0], "tempo")?,
        beats: m[1].as_u64().and_then(|x| u16::try_from(x).ok()).ok_or("beats")?,
        beat_value: m[2].as_u64().and_then(|x| u16::try_from(x).ok()).ok_or("beat value")?,
        grid_period: num(&m[3], "grid period")?,
        grid_offset: num(&m[4], "grid offset")?,
    }))
}

fn fade(o: &Value, t: &str, off: &str, curve: &str) -> R<MusicFade> {
    Ok(MusicFade { t: req_num(o, t)?, off: req_num(o, off)?, curve: en(g(o, curve), "fade curve")? })
}

fn rules(o: &Value) -> R<Vec<MusicRule>> {
    let ints = |v: &Value, k: &str| -> R<Vec<i64>> {
        arr(v, k).iter().map(|x| x.as_i64().ok_or_else(|| format!("rule ids {x}"))).collect()
    };
    arr(o, "rules")
        .iter()
        .map(|r| {
            let (s, d) = (g(r, "s").ok_or("rule s")?, g(r, "d").ok_or("rule d")?);
            Ok(MusicRule {
                src: ints(r, "src")?,
                dst: ints(r, "dst")?,
                s: RuleSrc {
                    fade: fade(s, "t", "off", "curve")?,
                    sync: en(g(s, "sync"), "sync")?,
                    post: flag(s, "post"),
                },
                d: RuleDst {
                    fade: fade(d, "t", "off", "curve")?,
                    entry: en(g(d, "entry"), "entry")?,
                    pre: flag(d, "pre"),
                },
                trans: match g(r, "trans") {
                    None => None,
                    Some(t) => Some(RuleTrans {
                        seg: req_uint(t, "seg")?,
                        fade_in: fade(g(t, "fi").ok_or("fi")?, "t", "off", "curve")?,
                        fade_out: fade(g(t, "fo").ok_or("fo")?, "t", "off", "curve")?,
                        pre: flag(t, "pre"),
                        post: flag(t, "post"),
                    }),
                },
            })
        })
        .collect()
}

fn decision(v: &Value) -> R<DecisionNode> {
    let a = v.as_array().filter(|a| a.len() == 3).ok_or("a decision node")?;
    Ok(DecisionNode {
        key: a[0].as_u64().and_then(|x| Id::try_from(x).ok()).ok_or("key")?,
        node: a[1].as_u64().and_then(|x| Id::try_from(x).ok()).ok_or("node")?,
        children: a[2].as_array().ok_or("children")?.iter().map(decision).collect::<R<_>>()?,
    })
}

fn music_item(v: &Value) -> R<MusicItem> {
    Ok(MusicItem {
        seg: req_uint(v, "seg")?,
        id: req_uint(v, "id")?,
        loop_count: req_uint(v, "loop")?,
        loop_range: match (uint(v, "lmin")?, uint(v, "lmax")?) {
            (Some(a), Some(b)) => Some([a, b]),
            _ => None,
        },
        weight: uint(v, "w")?.unwrap_or(50000),
        avoid: uint(v, "avoid")?.unwrap_or(0),
        shuffle: flag(v, "shuffle"),
        rs: uint(v, "rs")?.unwrap_or(0),
        children: arr(v, "ch").iter().map(music_item).collect::<R<_>>()?,
    })
}

fn auto_type(v: Option<&Value>) -> R<AutoType> {
    Ok(match v.and_then(Value::as_str) {
        Some("Volume") => AutoType::Volume,
        Some("LPF") => AutoType::Lpf,
        Some("HPF") => AutoType::Hpf,
        Some("FadeIn") => AutoType::FadeIn,
        Some("FadeOut") => AutoType::FadeOut,
        t => return Err(format!("automation type {t:?}")),
    })
}

fn track(o: &Value) -> R<MusicTrack> {
    let clips = arr(o, "clips")
        .iter()
        .map(|c| {
            let c = c.as_array().filter(|c| c.len() == 6).ok_or("a clip")?;
            Ok(Clip {
                src: c[0].as_u64().and_then(|x| Id::try_from(x).ok()).ok_or("clip src")?,
                play_at: num(&c[1], "play at")?,
                begin_trim: num(&c[2], "begin")?,
                end_trim: num(&c[3], "end")?,
                src_dur: num(&c[4], "dur")?,
                held: c[5].as_u64().and_then(|x| u8::try_from(x).ok()).ok_or("held")?,
            })
        })
        .collect::<R<_>>()?;
    let auto = arr(o, "auto")
        .iter()
        .map(|a| {
            Ok(ClipAuto {
                clip: req_uint(a, "clip")?,
                kind: auto_type(g(a, "type"))?,
                pts: pts(g(a, "pts").ok_or("automation points")?)?,
            })
        })
        .collect::<R<_>>()?;
    Ok(MusicTrack { node: node(o)?, clips, auto, look: g(o, "look").map_or(Ok(0.0), |v| num(v, "look"))? })
}

fn action(o: &Value) -> R<Action> {
    let mut a = Action {
        kind: en(g(o, "EventActionType"), "action type")?,
        scope: en(g(o, "EventActionScope"), "action scope")?,
        target: uint(o, "ReferencedId")?,
        group: uint(o, "grp")?,
        is_bus: flag(o, "isBus"),
        except: ids(o, "ex")?,
        ..Action::default()
    };
    if let Some(p) = g(o, "p").and_then(Value::as_object) {
        let mut m = BTreeMap::new();
        for (k, v) in p {
            m.insert(k.parse::<u16>().map_err(|e| e.to_string())?, num(v, "prop")?);
        }
        a.p = Props(m.into_iter().collect());
    }
    if let Some(c) = g(o, "params").and_then(|p| g(p, "FadeCurve")) {
        a.fade_curve = Some(en(Some(c), "fade curve")?);
    }
    Ok(a)
}

// ---- effects -----------------------------------------------------------------------------------------------------------

fn filt(v: Option<&Value>) -> R<FilterType> {
    en(v, "filter type")
}

fn room_verb(p: &Value) -> R<RoomVerbParams> {
    let (rt, inv, alg) = (&p["RTPCParams"], &p["InvariantParams"], &p["AlgoTunings"]);
    let tone = |n: u8| -> R<ToneFilter> {
        Ok(ToneFilter {
            pos: match inv[format!("Filter{n}Pos")].as_str() {
                Some("Off") => 0,
                Some("EROnly") => 1,
                Some("ReverbOnly") => 2,
                Some("ERAndReverb") => 3,
                x => return Err(format!("tone filter place {x:?}")),
            },
            curve: match inv[format!("Filter{n}Curve")].as_str() {
                Some("LowShelf") => 0,
                Some("Peaking") => 1,
                Some("HighShelf") => 2,
                x => return Err(format!("tone filter curve {x:?}")),
            },
            gain: req_num(rt, &format!("Filter{n}Gain"))?,
            freq: req_num(rt, &format!("Filter{n}Freq"))?,
            q: req_num(rt, &format!("Filter{n}Q"))?,
        })
    };
    Ok(RoomVerbParams {
        decay_time: req_num(rt, "DecayTime")?,
        hf_damping: req_num(rt, "HFDamping")?,
        diffusion: req_num(rt, "Diffusion")?,
        stereo_width: req_num(rt, "StereoWidth")?,
        filters: [tone(1)?, tone(2)?, tone(3)?],
        front_level: req_num(rt, "FrontLevel")?,
        rear_level: req_num(rt, "RearLevel")?,
        center_level: req_num(rt, "CenterLevel")?,
        lfe_level: req_num(rt, "LFELevel")?,
        dry_level: req_num(rt, "DryLevel")?,
        er_level: req_num(rt, "ERLevel")?,
        reverb_level: req_num(rt, "ReverbLevel")?,
        enable_early_reflections: flag(inv, "bEnableEarlyReflections"),
        er_pattern: req_uint(inv, "ERPattern")?,
        reverb_delay: req_num(inv, "ReverbDelay")?,
        room_size: req_num(inv, "RoomSize")?,
        er_front_back_delay: req_num(inv, "ERFrontBackDelay")?,
        density: req_num(inv, "Density")?,
        room_shape: req_num(inv, "RoomShape")?,
        num_reverb_units: req_uint(inv, "NumReverbUnits")?,
        enable_tone_controls: flag(inv, "bEnableToneControls"),
        input_center_level: req_num(inv, "InputCenterLevel")?,
        input_lfe_level: req_num(inv, "InputLFELevel")?,
        density_delay_min: req_num(alg, "DensityDelayMin")?,
        density_delay_max: req_num(alg, "DensityDelayMax")?,
        density_delay_rdm_perc: req_num(alg, "DensityDelayRdmPerc")?,
        room_shape_min: req_num(alg, "RoomShapeMin")?,
        room_shape_max: req_num(alg, "RoomShapeMax")?,
        diffusion_delay_scale_perc: req_num(alg, "DiffusionDelayScalePerc")?,
        diffusion_delay_max: req_num(alg, "DiffusionDelayMax")?,
        diffusion_delay_rdm_perc: req_num(alg, "DiffusionDelayRdmPerc")?,
        dc_filter_cut_freq: req_num(alg, "DCFilterCutFreq")?,
        reverb_unit_input_delay: req_num(alg, "ReverbUnitInputDelay")?,
        reverb_unit_input_delay_rdm_perc: req_num(alg, "ReverbUnitInputDelayRdmPerc")?,
    })
}

fn fx_params(plugin: &str, p: &Value) -> R<FxParams> {
    Ok(match plugin {
        "AkParameterEQFX" => FxParams::AkParameterEQFX(ParamEqParams {
            bands: arr(p, "Bands")
                .iter()
                .map(|b| {
                    Ok(EqBand {
                        filter: filt(g(b, "FilterType"))?,
                        gain: req_num(b, "Gain")?,
                        frequency: req_num(b, "Frequency")?,
                        q: req_num(b, "QFactor")?,
                        on: flag(b, "OnOff"),
                    })
                })
                .collect::<R<_>>()?,
            output_level: req_num(p, "OutputLevel")?,
            process_lfe: flag(p, "ProcessLFE"),
        }),
        "AkCompressorFX" => FxParams::AkCompressorFX(CompressorParams {
            threshold: req_num(p, "Threshold")?,
            ratio: req_num(p, "Ratio")?,
            attack: req_num(p, "Attack")?,
            release: req_num(p, "Release")?,
            output_level: req_num(p, "OutputLevel")?,
            channel_link: flag(p, "ChannelLink"),
            process_lfe: flag(p, "ProcessLFE"),
        }),
        "AkPeakLimiterFX" => FxParams::AkPeakLimiterFX(PeakLimiterParams {
            threshold: req_num(&p["RTPC"], "fThreshold")?,
            ratio: req_num(&p["RTPC"], "fRatio")?,
            release: req_num(&p["RTPC"], "fRelease")?,
            output_level: req_num(&p["RTPC"], "fOutputLevel")?,
            look_ahead: req_num(&p["NonRTPC"], "fLookAhead")?,
            process_lfe: flag(&p["NonRTPC"], "bProcessLFE"),
            channel_link: flag(&p["NonRTPC"], "bChannelLink"),
        }),
        "AkMeterFX" => FxParams::AkMeterFX(MeterParams {
            attack: req_num(&p["RTPC"], "fAttack")?,
            release: req_num(&p["RTPC"], "fRelease")?,
            min: req_num(&p["RTPC"], "fMin")?,
            max: req_num(&p["RTPC"], "fMax")?,
            hold: req_num(&p["RTPC"], "fHold")?,
            infinite_hold: flag(&p["RTPC"], "bInfiniteHold"),
            peak: p["NonRTPC"]["eMode"].as_str() == Some("Peak"),
            global: p["NonRTPC"]["eScope"].as_str() == Some("Global"),
            apply_downstream_volume: flag(&p["NonRTPC"], "bApplyDownstreamVolume"),
            game_param: req_uint(&p["NonRTPC"], "uGameParamID")?,
        }),
        "AkGainFX" => {
            FxParams::AkGainFX(GainParams { fullband: req_num(p, "fFullbandGain")?, lfe: req_num(p, "fLFEGain")? })
        }
        "AkStereoDelayFX" => {
            let (i, l) = (arr(p, "eInputType"), arr(p, "StereoDelayParams"));
            let line = |k: usize| -> R<DelayLine> {
                Ok(DelayLine {
                    delay: req_num(&l[k], "fDelayTime")?,
                    feedback: req_num(&l[k], "fFeedback")?,
                    cross_feed: req_num(&l[k], "fCrossFeed")?,
                    left_or_right: i[k].as_str() == Some("LEFT_OR_RIGHT"),
                })
            };
            let f = &p["FilterParams"];
            FxParams::AkStereoDelayFX(StereoDelayParams {
                lines: [line(0)?, line(1)?],
                filter: filt(g(f, "FilterType"))?,
                filter_gain: req_num(f, "FilterGain")?,
                filter_frequency: req_num(f, "FilterFrequency")?,
                filter_q: req_num(f, "FilterQFactor")?,
                dry: req_num(p, "fDryLevel")?,
                wet: req_num(p, "fWetLevel")?,
                front_rear_balance: req_num(p, "fFrontRearBalance")?,
                enable_feedback: flag(p, "bEnableFeedback"),
                enable_cross_feed: flag(p, "bEnableCrossFeed"),
            })
        }
        "AkRoomVerbFX" => FxParams::AkRoomVerbFX(Box::new(room_verb(p)?)),
        "AkDelayFX" => FxParams::AkDelayFX(DelayParams {
            feedback: req_num(&p["RTPC"], "fFeedback")?,
            wet_dry_mix: req_num(&p["RTPC"], "fWetDryMix")?,
            output_level: req_num(&p["RTPC"], "fOutputLevel")?,
            feedback_enabled: flag(&p["RTPC"], "bFeedbackEnabled"),
            delay: req_num(&p["NonRTPC"], "fDelayTime")?,
            process_lfe: flag(&p["NonRTPC"], "bProcessLFE"),
        }),
        other => FxParams::Other { name: other.to_owned() },
    })
}

fn fx_obj(o: &Value) -> R<FxObj> {
    let raw = g(o, "raw").ok_or("an effect without raw")?;
    let plugin = raw["Plugin"]["PluginId"].as_str().unwrap_or("None");
    let params = g(raw, "PluginParams").and_then(|p| g(p, "Params")).unwrap_or(&Value::Null);
    Ok(FxObj { fx: fx_params(plugin, params)?, rtpc: raw_rtpcs(arr(raw, "RTPCs"))? })
}

// ---- the objects ----------------------------------------------------------------------------------------------------------

fn obj(o: &Value) -> R<Obj> {
    let t = g(o, "t").and_then(Value::as_str).ok_or("an object without a type")?;
    Ok(match t {
        "Event" => Obj::Event { actions: ids(o, "EventActionIds")? },
        "EventAction" => Obj::EventAction(action(o)?),
        "SoundSfxVoice" => Obj::SoundSfxVoice(Sound {
            node: node(o)?,
            src: req_uint(o, "src")?,
            src_type: en(g(o, "srcType"), "source type")?,
            plugin: g(o, "plugin").and_then(Value::as_str).map(str::to_owned),
        }),
        "RandomSequenceContainer" => {
            let flags = g(o, "PlaylistFlags").and_then(Value::as_str).unwrap_or("");
            Obj::RandomSequenceContainer(RanSeq {
                node: node(o)?,
                children: ids(o, "ChildIds")?,
                playlist: arr(o, "Playlist")
                    .iter()
                    .map(|p| Ok(PlaylistItem { id: req_uint(p, "PlayId")?, weight: req_uint(p, "Weight")? }))
                    .collect::<R<_>>()?,
                mode: en(g(o, "Mode"), "mode")?,
                random_mode: en(g(o, "RandomMode"), "random mode")?,
                avoid_repeat: req_uint(o, "AvoidRepeatCount")?,
                loop_count: req_uint(o, "LoopCount")?,
                continuous: flags.contains("IsContinuous"),
                global: flags.contains("IsGlobal"),
                reset_each_play: flags.contains("ResetPlayListAtEachPlay"),
                transition: en(g(o, "TransitionMode"), "transition mode")?,
                transition_time: req_num(o, "TransitionTime")?,
                transition_mod: [req_num(o, "TransitionTimeModMin")?, req_num(o, "TransitionTimeModMax")?],
            })
        }
        "SwitchContainer" => {
            let raw = g(o, "raw").ok_or("a switch container without raw")?;
            Obj::SwitchContainer(SwitchCntr {
                node: node(o)?,
                children: ids(o, "ChildIds")?,
                group_type: req_uint(o, "GroupType")?,
                group: req_uint(o, "GroupId")?,
                default: req_uint(o, "DefaultSwitch")?,
                packages: arr(raw, "SwitchPackages")
                    .iter()
                    .map(|p| Ok(SwitchPackage { switch: req_uint(p, "SwitchId")?, nodes: ids(p, "NodeIds")? }))
                    .collect::<R<_>>()?,
            })
        }
        "LayerContainer" => Obj::LayerContainer(Parent { node: node(o)?, children: ids(o, "ChildIds")? }),
        "ActorMixer" => Obj::ActorMixer(Parent { node: node(o)?, children: ids(o, "ChildIds")? }),
        "MusicSwitchContainer" => Obj::MusicSwitchContainer(MusicSwitch {
            node: node(o)?,
            children: ids(o, "ChildIds")?,
            meter: meter(o)?,
            args: ids(o, "args")?,
            tree: arr(o, "tree").iter().map(decision).collect::<R<_>>()?,
            cont: flag(o, "cont"),
            rules: rules(o)?,
        }),
        "MusicRandomSequenceContainer" => Obj::MusicRandomSequenceContainer(MusicRanSeq {
            node: node(o)?,
            children: ids(o, "ChildIds")?,
            meter: meter(o)?,
            list: arr(o, "list").iter().map(music_item).collect::<R<_>>()?,
            rules: rules(o)?,
        }),
        "MusicSegment" => Obj::MusicSegment(MusicSegment {
            node: node(o)?,
            children: ids(o, "ChildIds")?,
            meter: meter(o)?,
            dur: req_num(o, "dur")?,
            entry: req_num(o, "entry")?,
            exit: req_num(o, "exit")?,
        }),
        "MusicTrack" => Obj::MusicTrack(track(o)?),
        "FxShareSet" => Obj::FxShareSet(fx_obj(o)?),
        "FxCustom" => Obj::FxCustom(fx_obj(o)?),
        other => Obj::Other { name: other.to_owned() },
    })
}

fn media(o: &Value) -> R<MediaRef> {
    Ok(MediaRef {
        path: g(o, "path").and_then(Value::as_str).ok_or("a medium without a path")?.to_owned(),
        frames: req_uint(o, "samples")?,
        rate: req_uint(o, "rate")?,
        channels: req_uint(o, "channels")?,
        mask: uint(o, "mask")?,
        loop_points: match (uint(o, "loop_start")?, uint(o, "loop_end")?) {
            (Some(a), Some(b)) => Some([a, b]),
            _ => None,
        },
        norm: g(o, "norm").map(|v| num(v, "norm")).transpose()?,
        peak: g(o, "peak").map(|v| num(v, "peak")).transpose()?,
        env: arr(o, "env")
            .iter()
            .map(|p| {
                let p = p.as_array().filter(|p| p.len() == 2).ok_or("an envelope point")?;
                Ok([num(&p[0], "frame")?, num(&p[1], "dB")?])
            })
            .collect::<R<_>>()?,
    })
}

fn keyed<T>(o: &Value, k: &str, f: impl Fn(&Value) -> R<T>) -> R<BTreeMap<Id, T>> {
    let mut m = BTreeMap::new();
    if let Some(x) = g(o, k).and_then(Value::as_object) {
        for (key, v) in x {
            m.insert(
                key.parse::<Id>().map_err(|e| format!("{k}: {key}: {e}"))?,
                f(v).map_err(|e| format!("{k} {key}: {e}"))?,
            );
        }
    }
    Ok(m)
}

/// An oracle event bank as records.
pub fn event_bank(o: &Value) -> R<EventBank> {
    let dur = |i: usize| -> R<Option<f64>> {
        match o["dur"].get(i) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => Ok(Some(num(v, "dur")?)),
        }
    };
    Ok(EventBank {
        name: o["name"].as_str().ok_or("no name")?.to_owned(),
        id: req_uint(o, "id")?,
        dur: [dur(0)?, dur(1)?],
        actions: ids(&o["evobj"], "EventActionIds")?,
        posted: keyed(o, "posted", |e| ids(e, "EventActionIds"))?,
        objs: keyed(o, "objs", obj)?,
        media: keyed(o, "media", media)?,
    })
}

// ---- the InitBank ---------------------------------------------------------------------------------------------------------

fn bus(o: &Value) -> R<Bus> {
    let mut b = Bus {
        parent: uint(o, "parent")?,
        vol: g(o, "vol").map_or(Ok(0.0), |v| num(v, "vol"))?,
        max_inst: uint(o, "maxInst")?.unwrap_or(0),
        lim: uint(o, "lim")?.unwrap_or(0),
        rtpc: rtpcs(o, "rtpc")?,
        ..Bus::default()
    };
    if let Some(h) = g(o, "hdr") {
        let h = h.as_array().filter(|h| h.len() == 4).ok_or("a bus hdr")?;
        b.hdr = Some(Hdr {
            threshold: num(&h[0], "threshold")?,
            ratio: num(&h[1], "ratio")?,
            release: num(&h[2], "release")?,
            exponential: h[3].as_bool().ok_or("exponential")?,
        });
    }
    for f in arr(o, "fx") {
        let plugin = f["plugin"].as_str().ok_or("a bus effect without a plugin")?;
        let er = match g(f, "er") {
            None => None,
            Some(e) => {
                let side = |k: &str| -> R<Vec<[f64; 2]>> {
                    arr(e, k)
                        .iter()
                        .map(|t| {
                            let t = t.as_array().filter(|t| t.len() == 2).ok_or("a tap")?;
                            Ok([num(&t[0], "ms")?, num(&t[1], "gain")?])
                        })
                        .collect()
                };
                Some(ErTaps { left: side("L")?, right: side("R")? })
            }
        };
        b.fx.push(BusFx {
            fx: fx_params(plugin, g(f, "params").unwrap_or(&Value::Null))?,
            rtpc: rtpcs(f, "rtpc")?,
            er,
        });
    }
    if let Some(st) = g(o, "states").and_then(Value::as_object) {
        for (gid, ss) in st {
            let mut m = BTreeMap::new();
            for (sid, s) in ss.as_object().ok_or("states")? {
                let f = |k: &str| g(s, k).map_or(Ok(0.0), |v| num(v, k));
                m.insert(
                    sid.parse::<Id>().map_err(|e| e.to_string())?,
                    StateSet { vol: f("vol")?, lpf: f("lpf")?, mk: f("mk")?, bypass: f("bypass")? },
                );
            }
            b.states.insert(gid.parse::<Id>().map_err(|e| e.to_string())?, m);
        }
    }
    Ok(b)
}

fn att(o: &Value) -> R<Att> {
    let u = arr(o, "use");
    let uses: [i8; 7] = u
        .iter()
        .map(|x| x.as_i64().and_then(|n| i8::try_from(n).ok()).ok_or("a curve map entry"))
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| "a curve map of not 7 entries")?;
    Ok(Att {
        uses,
        curves: arr(o, "curves")
            .iter()
            .map(|c| {
                Ok(AttCurve { scaling: en(g(c, "scaling"), "scaling")?, pts: pts(g(c, "pts").ok_or("curve points")?)? })
            })
            .collect::<R<_>>()?,
    })
}

/// A JSON object's members in text order.
struct Ordered(Vec<(String, Value)>);

impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Ordered, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Ordered;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Ordered, A::Error> {
                let mut out = Vec::new();
                while let Some(kv) = a.next_entry::<String, Value>()? {
                    out.push(kv);
                }
                Ok(Ordered(out))
            }
        }
        d.deserialize_map(V)
    }
}

#[derive(Deserialize)]
struct InitEvents {
    events: Ordered,
}

/// An oracle InitBank as records; the buses' `aux` is left as `false` (see the module doc).
pub fn init_bank(text: &[u8]) -> R<InitBank> {
    let o: Value = serde_json::from_slice(text).map_err(|e| e.to_string())?;
    let order: InitEvents = serde_json::from_slice(text).map_err(|e| e.to_string())?;
    let mut events = indexmap::IndexMap::new();
    for (k, e) in order.events.0 {
        let dur = |i: usize| -> R<Option<f64>> {
            match e["dur"].get(i) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => Ok(Some(num(v, "dur")?)),
            }
        };
        events.insert(
            k,
            EventEntry {
                path: e["path"].as_str().ok_or("an event without a path")?.to_owned(),
                localized: flag(&e, "localized"),
                dur: [dur(0)?, dur(1)?],
                pairs: flag(&e, "pairs"),
            },
        );
    }
    let plain = |k: &str| -> R<BTreeMap<Id, f64>> {
        let mut m = BTreeMap::new();
        if let Some(x) = g(&o, k).and_then(Value::as_object) {
            for (key, v) in x {
                m.insert(key.parse::<Id>().map_err(|e| e.to_string())?, num(v, k)?);
            }
        }
        Ok(m)
    };
    Ok(InitBank {
        buses: keyed(&o, "buses", bus)?,
        atts: keyed(&o, "atts", att)?,
        rtpc_defaults: plain("rtpcDefaults")?,
        rtpc_ramps: keyed(&o, "rtpcRamps", |r| {
            let r = r.as_array().filter(|r| r.len() == 3).ok_or("a ramp")?;
            Ok(RtpcRamp {
                kind: r[0].as_u64().and_then(|x| u8::try_from(x).ok()).ok_or("ramp kind")?,
                up: num(&r[1], "up")?,
                down: num(&r[2], "down")?,
            })
        })?,
        state_groups: plain("stateGroups")?,
        volume_threshold: req_num(&o, "volumeThreshold")?,
        max_voices: req_uint(&o, "maxVoices")?,
        events,
    })
}

// ---- the compare ------------------------------------------------------------------------------------------------------------

/// Where two JSON values differ (up to `limit` places): the path, ours, the oracle's.
fn walk(a: &Value, b: &Value, path: &mut String, out: &mut Vec<(String, String, String)>, limit: usize) {
    if out.len() >= limit {
        return;
    }
    let shown = |v: &Value| {
        let s = v.to_string();
        if s.len() > 140 { format!("{}...", &s[..140]) } else { s }
    };
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let n = path.len();
                let _ = write!(path, ".{k}");
                match (x.get(k), y.get(k)) {
                    (Some(p), Some(q)) => walk(p, q, path, out, limit),
                    (p, q) => {
                        out.push((path.clone(), p.map_or("(absent)".into(), shown), q.map_or("(absent)".into(), shown)))
                    }
                }
                path.truncate(n);
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                let n = path.len();
                let _ = write!(path, "[{i}]");
                walk(p, q, path, out, limit);
                path.truncate(n);
            }
        }
        (Value::Number(x), Value::Number(y)) if x.as_f64() == y.as_f64() => {}
        _ if a == b => {}
        _ => out.push((path.clone(), shown(a), shown(b))),
    }
}

/// A path with its numeric ids and indexes replaced by `#`.
fn collapse(path: &str) -> String {
    let mut out = String::new();
    for seg in path.split_inclusive(['.', '[']) {
        let (body, tail) = seg.split_at(seg.len() - usize::from(seg.ends_with(['.', '['])));
        let body_trim = body.trim_end_matches(']');
        if !body_trim.is_empty() && body_trim.bytes().all(|c| c.is_ascii_digit()) {
            out.push('#');
            out.push_str(&body[body_trim.len()..]);
        } else {
            out.push_str(body);
        }
        out.push_str(tail);
    }
    out
}

#[derive(Default)]
struct Group {
    count: usize,
    sample: Option<(String, String, String, String)>,
}

/// Compares the pack at `ours` with the oracle's; prints the numbers, writes the groups to `report`.
pub fn run(ours: &Pack, oracle: &Pack, report: Option<&Path>) -> Result<(), String> {
    let mut paths: Vec<&str> = oracle.paths().collect();
    paths.sort_unstable();
    let mut only_oracle = Vec::new();
    let (mut equal, mut differ, mut broken) = (0usize, 0usize, 0usize);
    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut convert_failed: Vec<(String, String)> = Vec::new();
    let mut objects = 0usize;
    for path in &paths {
        let Some(mine) = ours.bytes(path).ok() else {
            only_oracle.push(*path);
            continue;
        };
        let theirs = oracle.bytes(path).map_err(|e| format!("{path}: {e}"))?;
        let pair: Option<(Value, Value)> = if *path == INIT_BANK {
            let mut ours: InitBank = decode_bank(&mine, path).map_err(|e| e.to_string())?;
            let theirs = match init_bank(&theirs) {
                Ok(b) => b,
                Err(e) => {
                    broken += 1;
                    convert_failed.push((path.to_string(), e));
                    continue;
                }
            };
            for b in ours.buses.values_mut() {
                b.aux = false;
            }
            objects += ours.buses.len();
            // an IndexMap's `==` ignores its order: the index's order is compared too
            if ours == theirs && ours.events.keys().eq(theirs.events.keys()) {
                None
            } else {
                Some((to_json(&ours)?, to_json(&theirs)?))
            }
        } else {
            let ours: EventBank = decode_bank(&mine, path).map_err(|e| e.to_string())?;
            let json: Value = serde_json::from_slice(&theirs).map_err(|e| format!("{path}: {e}"))?;
            let theirs = match event_bank(&json) {
                Ok(b) => b,
                Err(e) => {
                    broken += 1;
                    convert_failed.push((path.to_string(), e));
                    continue;
                }
            };
            objects += ours.objs.len();
            if ours == theirs { None } else { Some((to_json(&ours)?, to_json(&theirs)?)) }
        };
        // `==` on the records decides; the walk only says where
        let Some((a, b)) = pair else {
            equal += 1;
            continue;
        };
        differ += 1;
        let mut diffs = Vec::new();
        walk(&a, &b, &mut String::new(), &mut diffs, 3);
        if diffs.is_empty() {
            diffs.push((
                String::new(),
                "(the same JSON, not the same records: the events' order or a -0)".into(),
                String::new(),
            ));
        }
        for (p, x, y) in diffs {
            let g = groups.entry(collapse(&p)).or_default();
            g.count += 1;
            if g.sample.is_none() {
                g.sample = Some((path.to_string(), p, x, y));
            }
        }
    }
    let mut only_ours: Vec<&str> = ours.paths().filter(|p| !oracle.has(p)).collect();
    only_ours.sort_unstable();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "entries: ours {}, oracle {}; only in ours {}, only in the oracle {}",
        ours.len(),
        oracle.len(),
        only_ours.len(),
        only_oracle.len()
    );
    let _ = writeln!(
        out,
        "compared {}: equal {equal}, differing {differ}, oracle entries not converted {broken}; {objects} objects / buses",
        equal + differ
    );
    let mut sorted: Vec<_> = groups.into_iter().collect();
    sorted.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(a.0.cmp(&b.0)));
    for (k, g) in &sorted {
        let _ = writeln!(out, "{:>7}  {k}", g.count);
        if let Some((entry, p, x, y)) = &g.sample {
            let _ = writeln!(out, "         e.g. {entry} {p}\n           ours:   {x}\n           oracle: {y}");
        }
    }
    for (p, e) in convert_failed.iter().take(10) {
        let _ = writeln!(out, "not converted: {p}: {e}");
    }
    for p in only_ours.iter().take(10) {
        let _ = writeln!(out, "only ours: {p}");
    }
    for p in only_oracle.iter().take(10) {
        let _ = writeln!(out, "only oracle: {p}");
    }
    print!("{}", out.lines().take(60).collect::<Vec<_>>().join("\n"));
    println!();
    if let Some(r) = report {
        std::fs::write(r, &out).map_err(|e| format!("{}: {e}", r.display()))?;
    }
    if differ == 0 && broken == 0 && only_ours.is_empty() && only_oracle.is_empty() {
        Ok(())
    } else {
        Err("the soundbank kind differs".into())
    }
}

fn to_json<T: serde::Serialize>(v: &T) -> R<Value> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}
