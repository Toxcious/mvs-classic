//! A bank's hierarchy items as the records the game plays from (`mvs_data::sound::Obj`, `Att`): the compaction over the
//! tree `mvs_uasset::natives::wwise_bank` reads, with what the tree leaves out ([`ItemHidden`]).

use std::collections::BTreeMap;

use mvs_data::sound::*;
use mvs_uasset::natives::wwise_bank::ItemHidden;
use mvs_uasset::value::Value;

use super::fx::fx_params;
use super::tree::*;

/// The two fixed cue ids of every music segment.
const ENTRY_CUE: u32 = 43_573_010;
const EXIT_CUE: u32 = 1_539_036_744;

/// A hierarchy item of an event bank.
// one item at a time, moved straight into its bank
#[allow(clippy::large_enum_variant)]
pub(super) enum Compact {
    /// An Event: its action ids.
    Event(Vec<Id>),
    Attenuation(Att),
    Obj(Obj),
}

/// The id of a hierarchy item (a decimal string in the tree).
pub(super) fn hirc_id(h: &Value, what: &str) -> R<Id> {
    match str_of(get(h, "Id")).and_then(|s| s.parse().ok()) {
        Some(id) => Ok(id),
        None => fail(what, "a hierarchy item without a numeric Id"),
    }
}

pub(super) fn hirc_type(h: &Value) -> &str {
    str_of(get(h, "Type")).unwrap_or("")
}

/// The limit flags of an AdvSettingsFlags value as the tree writes it (names, or the number).
fn lim_bits(v: Option<&Value>, what: &str) -> R<u8> {
    let n = match v {
        Some(Value::Int(i)) => *i,
        Some(Value::UInt(u)) => i64::try_from(*u).unwrap_or(-1),
        Some(Value::Str(s)) => {
            let mut n = 0;
            for part in s.split(',') {
                n += match part.trim() {
                    "None" => 0,
                    "KillNewest" => 1,
                    "UseVirtualBehavior" => 2,
                    "UnknownFlag2" => 4,
                    "IsMaxNumInstOverrideParent" => 8,
                    "IsVVoicesOptOverrideParent" => 16,
                    k => return fail(what, format!("limit flag {k} not handled")),
                };
            }
            n
        }
        _ => 0,
    };
    match u8::try_from(n) {
        Ok(b) => Ok(b),
        Err(_) => fail(what, format!("limit flags {n}")),
    }
}

/// The limit flags of an AdvSettingsFlags value, for the bus code.
pub(super) fn limit_flags(v: Option<&Value>, what: &str) -> R<u8> {
    lim_bits(v, what)
}

/// A property bundle as a JS object holds it: by id ascending, a repeated id once with its last value.
fn props_of(list: &[Value], what: &str) -> R<Props> {
    let mut m: BTreeMap<u16, f64> = BTreeMap::new();
    for p in list {
        let (Some(id), Some(v)) = (int::<u16>(get(p, "Id")), f(get(p, "Value"))) else {
            return fail(what, "a property that is not an id and a number");
        };
        m.insert(id, v);
    }
    Ok(Props(m.into_iter().collect()))
}

fn ranges_of(list: &[Value], what: &str) -> R<Ranges> {
    let mut m: BTreeMap<u16, [f64; 2]> = BTreeMap::new();
    for p in list {
        let (Some(id), Some(lo), Some(hi)) = (int::<u16>(get(p, "Id")), f(get(p, "Min")), f(get(p, "Max"))) else {
            return fail(what, "a property range that is not an id and two numbers");
        };
        m.insert(id, [lo, hi]);
    }
    Ok(Ranges(m.into_iter().collect()))
}

fn accum_of(v: Option<&Value>, what: &str) -> R<Accum> {
    Ok(match str_of(v) {
        Some("Additive") => Accum::Additive,
        Some("Exclusive") => Accum::Exclusive,
        _ => return fail(what, format!("RTPC accumulation {v:?} not handled")),
    })
}

/// The game-parameter curves of an RtpcList (`accum`: an effect's, which carry their accumulation).
pub(super) fn rtpcs(rl: &[Value], accum: bool, what: &str) -> R<Vec<Rtpc>> {
    let mut out = Vec::new();
    for x in rl {
        if str_of(get(x, "RtpcType")) != Some("GameParameter") {
            continue;
        }
        let Some(table) = get(x, "ConversionTable") else { return fail(what, "an RTPC without a conversion table") };
        out.push(Rtpc {
            id: req_int(x, "RtpcId", what)?,
            param: req_int(x, "ParamId", what)?,
            scaling: scaling(get(table, "Scaling"), what)?,
            pts: pts(list(table, "GraphPoints"), what)?,
            accum: if accum { Some(accum_of(get(x, "RtpcAccum"), what)?) } else { None },
        });
    }
    Ok(out)
}

/// A node's parameters (`compact`'s NodeBaseParams part) and, for a non-music node, `addRaw`'s attenuation and
/// positioning.
fn node(d: &Value, hid: &ItemHidden, music: bool, what: &str) -> R<Node> {
    let mut n = Node::default();
    let Some(bp) = get(d, "BaseParams").or_else(|| get(d, "ContainerHierarchy")) else { return Ok(n) };
    if let Some(pb) = get(bp, "PropBundle") {
        n.p = props_of(list(pb, "Props"), what)?;
        n.r = ranges_of(list(pb, "PropRanges"), what)?;
    }
    n.parent = int(get(bp, "DirectParentId")).filter(|&i: &Id| i != 0);
    n.bus = int(get(bp, "OverrideBusId")).filter(|&i: &Id| i != 0);
    let adv = get(bp, "AdvSettingsParams");
    let advf = |k: &str| adv.and_then(|a| get(a, k));
    n.max_inst = int(advf("MaxNumInstance")).unwrap_or(0);
    n.lim = lim_bits(advf("AdvSettingsFlags"), what)?;
    // a behaviour named is kept whatever it is; a number only when nonzero
    n.below = match advf("BelowThresholdBehavior") {
        Some(Value::Str(s)) => Some(match s.as_str() {
            "ContinueToPlay" => 0,
            "KillVoice" => 1,
            "SetAsVirtualVoice" => 2,
            "KillIfOneShotElseVirtual" => 3,
            k => return fail(what, format!("below threshold behaviour {k}")),
        }),
        v => int::<u8>(v).filter(|&b| b != 0),
    };
    n.vq = match advf("VirtualQueueBehavior") {
        Some(Value::Int(i)) => u8::try_from(*i).unwrap_or(0),
        Some(Value::UInt(u)) => u8::try_from(*u).unwrap_or(0),
        Some(Value::Str(s)) => match s.rsplit("::").next().unwrap_or("") {
            "FromBeginning" => 0,
            "FromElapsedTime" => 1,
            "Resume" => 2,
            k => return fail(what, format!("virtual queue behaviour {k}")),
        },
        _ => 0,
    };
    n.prio_ov = is_true(get(bp, "PriorityOverrideParent"));
    let hf = text(advf("HdrEnvelopeFlags")).unwrap_or_default();
    for (name, bit) in
        [("OverrideHdrEnvelope", 1), ("OverrideAnalysis", 2), ("NormalizeLoudness", 4), ("EnableEnvelope", 8)]
    {
        if hf.contains(name) {
            n.hdr += bit;
        }
    }
    n.rtpc = rtpcs(list(bp, "RtpcList"), false, what)?;
    // insert effects that run live on the node's voices; rendered ones are baked into the media
    if let Some(fxp) = get(bp, "FxParams")
        && !is_true(get(fxp, "BypassAll"))
    {
        for e in list(fxp, "Effects") {
            if !is_true(get(e, "IsRendered")) {
                n.fx.push(req_int(e, "FXId", what)?);
            }
        }
    }
    // AuxParams byte (CAkParameterNodeBase::SetAuxParams): 1 override game aux, 2 use game aux (CUE4Parse's
    // 'OverridePriority'), 4 override user aux, 8 has aux ids
    if let Some(ax) = get(bp, "AuxParams") {
        let fl = text(get(ax, "AuxParams")).unwrap_or_default();
        let aux_ids = id_list(get(ax, "AuxIds"), what)?;
        if fl.contains("HasAux") && aux_ids.iter().any(|&x| x != 0) {
            n.aux = aux_ids;
        }
        n.aux_ov = fl.contains("OverrideUserAuxSends");
        n.gaux = u8::from(fl.contains("OverrideGameAux")) | (u8::from(fl.contains("OverridePriority")) << 1);
    }
    if !music {
        // addRaw: the attenuation share set (property 70's id) and the positioning bytes
        n.att = hid.att;
        if let Some((bits, b3d)) = hid.pos
            && bits & 1 != 0
        {
            n.pos = Some([bits, if bits & 2 != 0 { b3d } else { 0 }]);
        }
    }
    Ok(n)
}

/// A hierarchy item as its record. `hid`: what the tree leaves out of it.
pub(super) fn compact(h: &Value, hid: &ItemHidden, what: &str) -> R<Compact> {
    let ty = hirc_type(h);
    let Some(d) = get(h, "Data") else { return fail(what, format!("{ty} {} has no Data", hirc_id(h, what)?)) };
    let music = matches!(ty, "MusicSwitchContainer" | "MusicRandomSequenceContainer" | "MusicSegment" | "MusicTrack");
    let n = |what: &str| node(d, hid, music, what);
    let children = || id_list(get(d, "ChildIds"), what);
    Ok(match ty {
        "Event" => Compact::Event(id_list(get(d, "EventActionIds"), what)?),
        "Attenuation" => Compact::Attenuation(attenuation(h, d, hid, what)?),
        "EventAction" => Compact::Obj(Obj::EventAction(action(d, what)?)),
        "SoundSfxVoice" => {
            let Some(src) = get(d, "Source") else { return fail(what, "a sound without a Source") };
            let src_type = match str_of(get(src, "SourceType")) {
                Some("Data") => SrcType::Data,
                Some("Streaming") => SrcType::Streaming,
                Some("PrefetchStreaming") => SrcType::PrefetchStreaming,
                t => return fail(what, format!("source type {t:?}")),
            };
            let plugin = get(src, "Plugin")
                .and_then(|p| str_of(get(p, "PluginId")))
                .filter(|&p| p != "VORBIS")
                .map(str::to_owned);
            Compact::Obj(Obj::SoundSfxVoice(Sound {
                node: n(what)?,
                src: req_int(src, "SourceId", what)?,
                src_type,
                plugin,
            }))
        }
        "RandomSequenceContainer" => {
            Compact::Obj(Obj::RandomSequenceContainer(ran_seq(d, n(what)?, children()?, what)?))
        }
        "SwitchContainer" => {
            let mut packages = Vec::new();
            for p in list(d, "SwitchPackages") {
                packages.push(SwitchPackage {
                    switch: req_int(p, "SwitchId", what)?,
                    nodes: id_list(get(p, "NodeIds"), what)?,
                });
            }
            Compact::Obj(Obj::SwitchContainer(SwitchCntr {
                node: n(what)?,
                children: children()?,
                group_type: req_int(d, "GroupType", what)?,
                group: req_int(d, "GroupId", what)?,
                default: req_int(d, "DefaultSwitch", what)?,
                packages,
            }))
        }
        "LayerContainer" => Compact::Obj(Obj::LayerContainer(Parent { node: n(what)?, children: children()? })),
        "ActorMixer" => Compact::Obj(Obj::ActorMixer(Parent { node: n(what)?, children: children()? })),
        "MusicSwitchContainer" => {
            Compact::Obj(Obj::MusicSwitchContainer(music_switch(d, n(what)?, children()?, h, what)?))
        }
        "MusicRandomSequenceContainer" => {
            Compact::Obj(Obj::MusicRandomSequenceContainer(music_ran_seq(d, hid, n(what)?, children()?, h, what)?))
        }
        "MusicSegment" => Compact::Obj(Obj::MusicSegment(music_segment(d, n(what)?, children()?, h, what)?)),
        "MusicTrack" => Compact::Obj(Obj::MusicTrack(music_track(d, n(what)?, h, what)?)),
        "FxShareSet" => Compact::Obj(Obj::FxShareSet(fx_obj(d, what)?)),
        "FxCustom" => Compact::Obj(Obj::FxCustom(fx_obj(d, what)?)),
        other => Compact::Obj(Obj::Other { name: other.to_owned() }),
    })
}

fn action(d: &Value, what: &str) -> R<Action> {
    let kind = match str_of(get(d, "EventActionType")) {
        Some("Play") => ActionKind::Play,
        Some("PlayEvent") => ActionKind::PlayEvent,
        Some("Stop") => ActionKind::Stop,
        Some("Pause") => ActionKind::Pause,
        Some("Resume") => ActionKind::Resume,
        Some("Mute") => ActionKind::Mute,
        Some("UnMute") => ActionKind::UnMute,
        Some("SetState") => ActionKind::SetState,
        Some("SetSwitch") => ActionKind::SetSwitch,
        Some("SetVoiceVolume") => ActionKind::SetVoiceVolume,
        Some("ResetGameParameter") => ActionKind::ResetGameParameter,
        Some("Seek") => ActionKind::Seek,
        Some("Break") => ActionKind::Break,
        t => return fail(what, format!("action type {t:?} not handled")),
    };
    let scope = match str_of(get(d, "EventActionScope")) {
        Some("GameObjectId") => ActionScope::GameObjectId,
        Some("GameObject") => ActionScope::GameObject,
        Some("Global") => ActionScope::Global,
        Some("GlobalGameObject") => ActionScope::GlobalGameObject,
        Some("All") => ActionScope::All,
        t => return fail(what, format!("action scope {t:?} not handled")),
    };
    let ad = get(d, "ActionData");
    let params = ad.and_then(|a| get(a, "ActionParams"));
    let fade_curve = match params.and_then(|p| get(p, "FadeCurve")) {
        Some(v) => Some(curve(Some(v), what)?),
        None => None,
    };
    let except = match ad.and_then(|a| get(a, "ExceptParams")) {
        Some(e) => {
            let mut ids = Vec::new();
            for x in list(e, "ExceptionElements") {
                ids.push(req_int(x, "Id", what)?);
            }
            ids
        }
        None => Vec::new(),
    };
    Ok(Action {
        kind,
        scope,
        target: int(get(d, "ReferencedId")),
        p: match get(d, "PropBundle") {
            Some(pb) => props_of(list(pb, "Props"), what)?,
            None => Props::default(),
        },
        fade_curve,
        group: ad.and_then(|a| int(get(a, "StateGroupId"))),
        is_bus: is_true(get(d, "IsBus")),
        except,
    })
}

fn ran_seq(d: &Value, node: Node, children: Vec<Id>, what: &str) -> R<RanSeq> {
    let mut playlist = Vec::new();
    for p in list(d, "Playlist") {
        playlist.push(PlaylistItem { id: req_int(p, "PlayId", what)?, weight: req_int(p, "Weight", what)? });
    }
    let flags = text(get(d, "PlaylistFlags")).unwrap_or_default();
    Ok(RanSeq {
        node,
        children,
        playlist,
        mode: match str_of(get(d, "Mode")) {
            Some("Random") => PlayMode::Random,
            Some("Sequence") => PlayMode::Sequence,
            m => return fail(what, format!("container mode {m:?}")),
        },
        random_mode: match str_of(get(d, "RandomMode")) {
            Some("Normal") => RandomMode::Normal,
            Some("Shuffle") => RandomMode::Shuffle,
            m => return fail(what, format!("random mode {m:?}")),
        },
        avoid_repeat: req_int(d, "AvoidRepeatCount", what)?,
        loop_count: req_int(d, "LoopCount", what)?,
        continuous: flags.contains("IsContinuous"),
        global: flags.contains("IsGlobal"),
        reset_each_play: flags.contains("ResetPlayListAtEachPlay"),
        transition: match str_of(get(d, "TransitionMode")) {
            Some("Disabled") => TransitionMode::Disabled,
            Some("CrossFadeAmp") => TransitionMode::CrossFadeAmp,
            Some("CrossFadePower") => TransitionMode::CrossFadePower,
            Some("Delay") => TransitionMode::Delay,
            Some("SampleAccurate") => TransitionMode::SampleAccurate,
            Some("TriggerRate") => TransitionMode::TriggerRate,
            m => return fail(what, format!("transition mode {m:?}")),
        },
        transition_time: req_f(d, "TransitionTime", what)?,
        transition_mod: [req_f(d, "TransitionTimeModMin", what)?, req_f(d, "TransitionTimeModMax", what)?],
    })
}

fn fx_obj(d: &Value, what: &str) -> R<FxObj> {
    let plugin = get(d, "Plugin").and_then(|p| str_of(get(p, "PluginId"))).unwrap_or("None");
    let params = get(d, "PluginParams").and_then(|p| get(p, "Params"));
    Ok(FxObj { fx: fx_params(plugin, params, what)?, rtpc: rtpcs(list(d, "RTPCs"), true, what)? })
}

/// An attenuation: the curve map and the curves. A cone is not handled.
fn attenuation(h: &Value, d: &Value, hid: &ItemHidden, what: &str) -> R<Att> {
    let id = hirc_id(h, what)?;
    if is_true(get(d, "IsConeEnabled")) {
        return fail(what, format!("attenuation {id}: cone not handled"));
    }
    let Some(uses) = hid.curve_to_use else { return fail(what, format!("attenuation {id}: no curve map")) };
    let mut curves = Vec::new();
    for x in list(d, "Curves") {
        curves.push(AttCurve { scaling: scaling(get(x, "Scaling"), what)?, pts: pts(list(x, "GraphPoints"), what)? });
    }
    Ok(Att { uses, curves })
}

// ---- the music hierarchy ----------------------------------------------------------------------------------------

fn meter(d: &Value, id: Id, what: &str) -> R<Option<Meter>> {
    let Some(m) = get(d, "MeterInfo") else { return Ok(None) };
    if !is_true(get(m, "MeterInfoFlag")) {
        return Ok(None);
    }
    let tempo = req_f(m, "Tempo", what)?;
    let beats: u16 = req_int(m, "TimeSigNumBeatsBar", what)?;
    let beat_value: u16 = req_int(m, "TimeSigBeatValue", what)?;
    if tempo <= 0.0 || beats == 0 || beat_value == 0 {
        return fail(what, format!("music node {id}: meter"));
    }
    Ok(Some(Meter {
        tempo,
        beats,
        beat_value,
        grid_period: req_f(m, "GridPeriod", what)?,
        grid_offset: req_f(m, "GridOffset", what)?,
    }))
}

fn sync_type(v: Option<&Value>, what: &str) -> R<SyncType> {
    Ok(match str_of(v) {
        Some("Immediate") => SyncType::Immediate,
        Some("NextGrid") => SyncType::NextGrid,
        Some("NextBar") => SyncType::NextBar,
        Some("NextBeat") => SyncType::NextBeat,
        Some("NextMarker") => SyncType::NextMarker,
        Some("NextUserMarker") => SyncType::NextUserMarker,
        Some("EntryMarker") => SyncType::EntryMarker,
        Some("ExitMarker") => SyncType::ExitMarker,
        _ => return fail(what, format!("sync type {v:?} not handled")),
    })
}

fn entry_type(v: Option<&Value>, what: &str) -> R<EntryType> {
    Ok(match str_of(v) {
        Some("EntryMarker") => EntryType::EntryMarker,
        Some("SameTime") => EntryType::SameTime,
        Some("RandomMarker") => EntryType::RandomMarker,
        Some("RandomUserMarker") => EntryType::RandomUserMarker,
        Some("LastExitTime") => EntryType::LastExitTime,
        _ => return fail(what, format!("entry type {v:?} not handled")),
    })
}

fn fade(p: &Value, what: &str) -> R<MusicFade> {
    Ok(MusicFade {
        t: req_f(p, "TransitionTime", what)?,
        off: req_f(p, "FadeOffset", what)?,
        curve: curve(get(p, "FadeCurve"), what)?,
    })
}

fn rule_ids(v: Option<&Value>, what: &str) -> R<Vec<i64>> {
    items(v)
        .iter()
        .map(|x| match x.as_i64() {
            Some(i) => Ok(i),
            None => fail(what, "a rule id that is not an integer"),
        })
        .collect()
}

/// The music rules, whole: every rule, its ids, its transition object. Rule 0 is the fallback; a later rule with no
/// source or no destination is dropped.
fn music_rules(rl: &[Value], what: &str) -> R<Vec<MusicRule>> {
    let mut out = Vec::new();
    for (i, r) in rl.iter().enumerate() {
        let src = rule_ids(get(r, "SrcIds"), what)?;
        let dst = rule_ids(get(r, "DestIds"), what)?;
        if i != 0 && (src.is_empty() || dst.is_empty()) {
            continue;
        }
        let (Some(s), Some(e)) = (get(r, "SrcRules"), get(r, "DestRules")) else {
            return fail(what, format!("music rule {i}: no source or destination rules"));
        };
        // a destination cue filter with EntryType EntryMarker is not read (CAkMusicSegment::GetEntrySyncPos 18000f500)
        let entry = get(e, "EntryType");
        if str_of(get(e, "JumpToType")) != Some("StartOfPlaylist")
            || is_true(get(e, "DestMatchSourceCueName"))
            || is_true(get(s, "CueFilterHash"))
            || (is_true(get(e, "CueFilterHash")) && str_of(entry) != Some("EntryMarker"))
        {
            return fail(what, format!("music rule {i}: jump / cue filter not handled"));
        }
        let trans = match get(r, "TransObject") {
            Some(tr @ Value::Struct(_)) if is_true(get(tr, "SegmentId")) => {
                let (Some(fi), Some(fo)) = (get(tr, "FadeInParams"), get(tr, "FadeOutParams")) else {
                    return fail(what, format!("music rule {i}: a transition without fades"));
                };
                Some(RuleTrans {
                    seg: req_int(tr, "SegmentId", what)?,
                    fade_in: fade(fi, what)?,
                    fade_out: fade(fo, what)?,
                    pre: is_true(get(tr, "PlayPreEntry")),
                    post: is_true(get(tr, "PlayPostExit")),
                })
            }
            _ => None,
        };
        out.push(MusicRule {
            src,
            dst,
            s: RuleSrc {
                fade: fade(s, what)?,
                sync: sync_type(get(s, "SyncType"), what)?,
                post: is_true(get(s, "PlayPostExit")),
            },
            d: RuleDst { fade: fade(e, what)?, entry: entry_type(entry, what)?, pre: is_true(get(e, "PlayPreEntry")) },
            trans,
        });
    }
    Ok(out)
}

fn music_switch(d: &Value, node: Node, children: Vec<Id>, h: &Value, what: &str) -> R<MusicSwitch> {
    let id = format!("music node {}", hirc_id(h, what)?);
    if str_of(get(d, "Mode")) != Some("Mode_BestMatch") {
        return fail(what, format!("{id}: mode {:?}", get(d, "Mode")));
    }
    fn tree(n: &Value, what: &str) -> R<DecisionNode> {
        let mut kids = Vec::new();
        for c in list(n, "Children") {
            kids.push(tree(c, what)?);
        }
        Ok(DecisionNode {
            key: req_int(n, "Key", what)?,
            node: int(get(n, "AudioNodeId")).unwrap_or(0),
            children: kids,
        })
    }
    let nodes = get(d, "DecisionTree").map_or(&[][..], |t| list(t, "Nodes"));
    // the root node's own audio node and any second root would be lost to the tree (the tree is Nodes[0]'s children)
    if nodes.len() != 1 || is_true(get(&nodes[0], "AudioNodeId")) {
        return fail(what, format!("{id}: a decision tree root BA does not read"));
    }
    let mut args = Vec::new();
    for a in list(d, "Arguments") {
        args.push(req_int(a, "GroupId", what)?);
    }
    let mut tr = Vec::new();
    for c in list(&nodes[0], "Children") {
        tr.push(tree(c, what)?);
    }
    Ok(MusicSwitch {
        node,
        children,
        meter: meter(d, hirc_id(h, what)?, what)?,
        args,
        tree: tr,
        cont: is_true(get(d, "IsContinuePlayback")),
        rules: music_rules(list(d, "MusicTransitionRule"), what)?,
    })
}

fn music_ran_seq(d: &Value, hid: &ItemHidden, node: Node, children: Vec<Id>, h: &Value, what: &str) -> R<MusicRanSeq> {
    let id = format!("music node {}", hirc_id(h, what)?);
    let root = list(d, "Playlist");
    if root.len() != 1 {
        return fail(what, format!("{id}: {} root items", root.len()));
    }
    fn item(p: &Value, rs: &[i32], at: &mut usize, id: &str, what: &str) -> R<MusicItem> {
        let Some(&r) = rs.get(*at) else { return fail(what, format!("{id}: no eRSType for item {at}")) };
        *at += 1;
        let (Some(lp), Some(w)) = (get(p, "LoopInfo"), get(p, "WeightInfo")) else {
            return fail(what, format!("{id}: an item without loop or weight info"));
        };
        let seg: Id = req_int(p, "SegmentId", what)?;
        let loop_count: i64 = req_int(lp, "Loop", what)?;
        let lmin: i64 = req_int(lp, "LoopMin", what)?;
        let lmax: i64 = req_int(lp, "LoopMax", what)?;
        let u16_of = |v: i64| u16::try_from(v).or_else(|_| fail(what, format!("{id}: loop count {v}")));
        let mut it = MusicItem {
            seg,
            id: req_int(p, "PlaylistItemId", what)?,
            loop_count: u16_of(loop_count)?,
            loop_range: if lmin != 0 || lmax != 0 { Some([u16_of(lmin)?, u16_of(lmax)?]) } else { None },
            weight: req_int(w, "Weight", what)?,
            ..MusicItem::default()
        };
        if seg == 0 {
            it.avoid = req_int(w, "AvoidRepeatCount", what)?;
            it.shuffle = is_true(get(w, "IsShuffle"));
        }
        for c in list(p, "Children") {
            it.children.push(item(c, rs, at, id, what)?);
        }
        let bad = if seg != 0 { r != -1 } else { !(0..=3).contains(&r) };
        if bad {
            return fail(what, format!("{id}: item {} eRSType {r}", it.id));
        }
        it.rs = u8::try_from(r.max(0)).unwrap_or(0);
        Ok(it)
    }
    let mut at = 0;
    let list_ = vec![item(&root[0], &hid.rs_type, &mut at, &id, what)?];
    Ok(MusicRanSeq {
        node,
        children,
        meter: meter(d, hirc_id(h, what)?, what)?,
        list: list_,
        rules: music_rules(list(d, "MusicTransitionRule"), what)?,
    })
}

fn music_segment(d: &Value, node: Node, children: Vec<Id>, h: &Value, what: &str) -> R<MusicSegment> {
    let id = format!("music node {}", hirc_id(h, what)?);
    let (mut entry, mut exit) = (None, None);
    for mk in list(d, "Markers") {
        match int::<u32>(get(mk, "Id")) {
            Some(ENTRY_CUE) => entry = f(get(mk, "Position")),
            Some(EXIT_CUE) => exit = f(get(mk, "Position")),
            _ => {}
        }
    }
    let (Some(entry), Some(exit)) = (entry, exit) else { return fail(what, format!("{id}: no entry or exit cue")) };
    Ok(MusicSegment {
        node,
        children,
        meter: meter(d, hirc_id(h, what)?, what)?,
        dur: req_f(d, "Duration", what)?,
        entry,
        exit,
    })
}

fn music_track(d: &Value, node: Node, h: &Value, what: &str) -> R<MusicTrack> {
    let id = format!("music node {}", hirc_id(h, what)?);
    if str_of(get(d, "MusicTrackType")) != Some("Normal") {
        return fail(what, format!("{id}: type {:?}", get(d, "MusicTrackType")));
    }
    // the source's kind (CAkMusicTrack::ComputeMinSrcLookAhead 180011f40): 0 in memory, 1 streamed, 2 streamed with
    // prefetch; a clip of no listed source is taken as streamed
    let mut kind: BTreeMap<u32, u8> = BTreeMap::new();
    for s in list(d, "Sources") {
        let k = match str_of(get(s, "SourceType")) {
            Some("Streaming") => 1,
            Some("PrefetchStreaming") => 2,
            _ => 0,
        };
        kind.insert(req_int(s, "SourceId", what)?, k);
    }
    let mut clips = Vec::new();
    for p in list(d, "Playlist") {
        let src: Id = req_int(p, "SourceId", what)?;
        clips.push(Clip {
            src,
            play_at: req_f(p, "PlayAt", what)?,
            begin_trim: req_f(p, "BeginTrimOffset", what)?,
            end_trim: req_f(p, "EndTrimOffset", what)?,
            src_dur: req_f(p, "SrcDuration", what)?,
            held: kind.get(&src).copied().unwrap_or(1),
        });
    }
    let mut auto = Vec::new();
    for a in list(d, "ClipAutomations") {
        let kind = match str_of(get(a, "AutoType")) {
            Some("Volume") => AutoType::Volume,
            Some("LPF") => AutoType::Lpf,
            Some("HPF") => AutoType::Hpf,
            Some("FadeIn") => AutoType::FadeIn,
            Some("FadeOut") => AutoType::FadeOut,
            t => return fail(what, format!("{id}: automation type {t:?}")),
        };
        auto.push(ClipAuto { clip: req_int(a, "ClipIndex", what)?, kind, pts: pts(list(a, "GraphPoints"), what)? });
    }
    Ok(MusicTrack { node, clips, auto, look: f(get(d, "LookAheadTime")).unwrap_or(0.0) })
}
