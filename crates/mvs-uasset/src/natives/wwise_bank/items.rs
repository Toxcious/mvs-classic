//! The HIRC items by type (EAKBKHircType): each reads its Data after the item's Id (CUE4Parse 1.2.2 UE4/Wwise/Objects/
//! HIRC and their WriteJson).

use super::enums::*;
use super::plugins::{bank_source, plugin_id, plugin_json, plugin_params};
use super::{
    W, base_hierarchy, empty, graph_point, int, js_object, obj, perr, prop_ranges, props, rtpcs, state_groups, uint,
};
use crate::archive::Result;
use crate::value::{Props, Value};

/// The reader of an item's Data by its type byte; none for a type no bank of the build holds.
pub(super) fn item(ty: u8) -> Option<fn(&mut W) -> Result<Value>> {
    Some(match ty {
        1 => state,
        2 => sound,
        3 => event_action,
        4 => event,
        5 => random_sequence,
        6 => switch_container,
        7 => actor_mixer,
        8 | 18 => bus,
        9 => layer_container,
        10 => music_segment,
        11 => music_track,
        12 => music_switch,
        13 => music_random_sequence,
        14 => attenuation,
        16 | 17 => fx_base,
        19 | 20 | 22 => modulator,
        21 => audio_device,
        _ => return None,
    })
}

fn state(w: &mut W) -> Result<Value> {
    let n = w.u16()?;
    let types = w.arr(i64::from(n), |w| w.u16())?;
    let values = w.arr(i64::from(n), |w| w.f())?;
    // Setting.WriteJson writes one property per setting: a repeated type keeps its first place, its last value
    let settings =
        types.into_iter().zip(values).map(|(t, v)| (EHierarchyParameterType.text(u32::from(t)), v)).collect();
    Ok(obj! { "Settings" => Value::Struct(js_object(settings)) })
}

fn sound(w: &mut W) -> Result<Value> {
    Ok(obj! { "Source" => bank_source(w)?, "BaseParams" => base_hierarchy(w)? })
}

fn except_params(w: &mut W) -> Result<Value> {
    let n = w.v7()?;
    Ok(obj! {
        "ExceptionElements" => w.arrv(i64::from(n), |w| Ok(obj! { "Id" => w.u32v()?, "IsBus" => w.bv()? }))?,
    })
}
fn action_params(w: &mut W) -> Result<Value> {
    Ok(obj! {
        "TTime" => Value::Int(0),
        "TTimeMin" => Value::Int(0),
        "TTimeMax" => Value::Int(0),
        "FadeCurve" => EAkCurveInterpolation.json(u32::from(w.u8()? & 0x1f)),
    })
}
fn randomizer(w: &mut W) -> Result<Value> {
    Ok(obj! { "Base" => w.f()?, "Min" => w.f()?, "Max" => w.f()? })
}

/// HierarchyEventAction's ActionData by its EAkActionType; none for a type with none (PlayEvent among them).
fn action_data(w: &mut W, ty: u8) -> Result<Option<Value>> {
    Ok(Some(match ty {
        26 | 27 | 51..=54 => obj! {
            "bIsBypass" => w.bv()?,
            "TargetMask" => w.u8v()?,
            "byFxSlot" => Value::Int(0),
            "ExceptParams" => except_params(w)?,
        },
        4 => obj! {
            "ActionParams" => action_params(w)?,
            "BankId" => w.u32v()?,
            "BankType" => EAkBankTypeEnum.json(0),
        },
        1 => {
            let params = action_params(w)?;
            let b = w.u8()?;
            obj! {
                "ActionParams" => params,
                "ApplyToStateTransitions" => Value::Bool(b & 2 != 0),
                "ApplyToDynamicSequence" => Value::Bool(b & 4 != 0),
                "ExceptParams" => except_params(w)?,
            }
        }
        19 | 20 => obj! {
            "ActionParams" => action_params(w)?,
            "BypassTransition" => w.bv()?,
            "ValueMeaning" => EAkValueMeaning.json(u32::from(w.u8()?)),
            "RandomizerModifier" => randomizer(w)?,
            "ExceptParams" => except_params(w)?,
        },
        8..=15 | 32 | 48 => obj! {
            "ActionParams" => action_params(w)?,
            "ValueMeaning" => EAkValueMeaning.json(u32::from(w.u8()?)),
            "RandomizerModifier" => randomizer(w)?,
            "ExceptParams" => except_params(w)?,
        },
        30 => obj! {
            "IsSeekRelativeToDuration" => w.u8v()?,
            "RandomizerModifier" => randomizer(w)?,
            "SnapToNearestMarker" => w.u8v()?,
            "ExceptParams" => except_params(w)?,
        },
        25 => obj! { "SwitchGroupId" => w.u32v()?, "SwitchStateId" => w.u32v()? },
        18 => obj! { "StateGroupId" => w.u32v()?, "TargetStateId" => w.u32v()? },
        49 | 50 => return Err(perr(format!("{}: SetEffect / ResetEffect actions are not ported", w.ar.name))),
        6 | 7 | 34 => obj! { "ActionParams" => action_params(w)?, "ExceptParams" => except_params(w)? },
        3 => obj! {
            "ActionParams" => action_params(w)?,
            "ResumeOptions" => EResumeOptionsFlags.json(u32::from(w.u8()?)),
            "ExceptParams" => except_params(w)?,
        },
        2 => obj! {
            "ActionParams" => action_params(w)?,
            "PauseOptions" => EPauseOptionsFlags.json(u32::from(w.u8()?)),
            "ExceptParams" => except_params(w)?,
        },
        _ => return Ok(None),
    }))
}

fn event_action(w: &mut W) -> Result<Value> {
    let scope = w.u8()?;
    let ty = w.u8()?;
    let referenced = w.u32()?;
    let is_bus = w.bv()?;
    let bundle = obj! { "Props" => props(w)?, "PropRanges" => prop_ranges(w)? };
    let data = action_data(w, ty)?;
    let mut o = Props::new();
    o.insert("EventActionScope".to_owned(), EAkActionScope.text_value(u32::from(scope)));
    o.insert("EventActionType".to_owned(), EEventActionType_v72_to_v150.text_value(u32::from(ty)));
    if referenced != 0 {
        o.insert("ReferencedId".to_owned(), uint(referenced));
    }
    o.insert("IsBus".to_owned(), is_bus);
    o.insert("PropBundle".to_owned(), bundle);
    if let Some(d) = data {
        o.insert("ActionData".to_owned(), d);
    }
    Ok(Value::Struct(o))
}

fn event(w: &mut W) -> Result<Value> {
    let n = w.v7()?;
    Ok(obj! { "EventActionIds" => w.arrv(i64::from(n), |w| w.u32v())? })
}

fn random_sequence(w: &mut W) -> Result<Value> {
    Ok(obj! {
        "BaseParams" => base_hierarchy(w)?,
        "LoopCount" => w.u16v()?,
        "LoopModMin" => w.u16v()?,
        "LoopModMax" => w.u16v()?,
        "TransitionTime" => w.f()?,
        "TransitionTimeModMin" => w.f()?,
        "TransitionTimeModMax" => w.f()?,
        "AvoidRepeatCount" => w.u16v()?,
        "TransitionMode" => EAkTransitionMode.text_value(u32::from(w.u8()?)),
        "RandomMode" => EAkRandomMode.text_value(u32::from(w.u8()?)),
        "Mode" => EAkContainerMode.text_value(u32::from(w.u8()?)),
        "PlaylistFlags" => EPlayListFlags.text_value(u32::from(w.u8()?)),
        "ChildIds" => { let n = w.u32()?; w.u32s(n)? },
        "Playlist" => {
            let n = w.u16()?;
            w.arrv(i64::from(n), |w| Ok(obj! { "PlayId" => w.u32v()?, "Weight" => w.i32v()? }))?
        },
    })
}

fn switch_container(w: &mut W) -> Result<Value> {
    Ok(obj! {
        "BaseParams" => base_hierarchy(w)?,
        // writer.WriteValue((object)GroupType): the number
        "GroupType" => w.u8v()?,
        "GroupId" => w.u32v()?,
        "DefaultSwitch" => w.u32v()?,
        "IsContinuousValidation" => w.bv()?,
        "ChildIds" => { let n = w.u32()?; w.u32s(n)? },
        "SwitchPackages" => {
            let n = w.u32()?;
            w.arrv(i64::from(n), |w| {
                Ok(obj! { "SwitchId" => w.u32v()?, "NodeIds" => { let n = w.u32()?; w.u32s(n)? } })
            })?
        },
        "SwitchParams" => {
            let n = w.u32()?;
            w.arrv(i64::from(n), |w| {
                let node = w.u32v()?;
                let b2 = w.u8()?;
                let b3 = w.u8()?;
                Ok(obj! {
                    "NodeId" => node,
                    "IsFirstOnly" => Value::Bool(b2 & 1 != 0),
                    "ContinuePlayback" => Value::Bool(b2 & 2 != 0),
                    "OnSwitchMode" => EOnSwitchMode.json(u32::from(b3 & 1)),
                    "FadeOutTime" => w.i32v()?,
                    "FadeInTime" => w.i32v()?,
                })
            })?
        },
    })
}

fn actor_mixer(w: &mut W) -> Result<Value> {
    Ok(obj! { "BaseParams" => base_hierarchy(w)?, "ChildIds" => { let n = w.u32()?; w.u32s(n)? } })
}

fn bus_fx(w: &mut W) -> Result<Value> {
    let n = w.u8()?;
    let mut bypass = 0u8;
    let mut chunks = Vec::new();
    if n > 0 {
        bypass = w.u8()?;
        for _ in 0..n {
            let index = w.u8v()?;
            let id = w.u32v()?;
            let share = w.u8v()?;
            w.u8()?;
            chunks.push(obj! { "FxIndex" => index, "FxId" => id, "IsShareSet" => share });
        }
    }
    Ok(obj! {
        "BitsFxBypass" => uint(bypass),
        "FxChunks" => Value::Array(chunks),
        "FxParams" => Value::Null,
        "FxId0" => w.u32v()?,
        "IsShareSet0" => w.bv()?,
    })
}

/// AudioBus and AuxiliaryBus.
fn bus(w: &mut W) -> Result<Value> {
    let override_bus = w.u32()?;
    let device = if override_bus == 0 { w.u32()? } else { 0 };
    let props = props(w)?;
    let positioning = super::positioning(w)?;
    let aux = super::aux_params(w)?;
    let adv = EAdvSettingsFlags.text_value(u32::from(w.u8()?));
    let max = w.u16v()?;
    let cc = w.u32()?;
    let channel = obj! {
        "NumChannels" => uint(cc & 0xff),
        "ConfigTypePacked" => EAkChannelConfig.json(cc),
        "ConfigType" => EAkChannelConfigType.json((cc >> 8) & 0xf),
        "ChannelMask" => uint((cc >> 12) & 0xfffff),
    };
    let hdr = w.u8v()?;
    let recovery = w.i32v()?;
    w.f32()?; // MaxDuckVolume: not written
    let n = w.u32()?;
    let duck = w.arrv(i64::from(n), |w| {
        Ok(obj! {
            "BusId" => w.u32v()?,
            "DuckVolume" => w.f()?,
            "FadeOutTime" => w.u32v()?,
            "FadeInTime" => w.u32v()?,
            "FadeCurve" => EAkCurveInterpolation.json(u32::from(w.u8()? & 0x1f)),
            "TargetProp" => EAkPropID.json(u32::from(w.u8()?)),
        })
    })?;
    let fx = bus_fx(w)?;
    let attachment = w.u8v()?;
    let rtpc = rtpcs(w)?;
    let groups = state_groups(w)?;
    Ok(obj! {
        "OverrideBusId" => uint(override_bus),
        "DeviceSharesetId" => uint(device),
        "Props" => props,
        "PositioningParams" => positioning,
        "AuxParams" => aux,
        "AdvSettingsParams" => adv,
        "MaxNumInstance" => max,
        "ChannelConfig" => channel,
        "HdrEnvelopeFlags" => hdr,
        "RecoveryTime" => recovery,
        "DuckInfo" => duck,
        "FxBusParams" => fx,
        "OverrideAttachmentParams" => attachment,
        "FxChunks" => empty(),
        "MetadataParams" => empty(),
        "RTPCs" => rtpc,
        "StateGroups" => groups,
    })
}

fn layer_container(w: &mut W) -> Result<Value> {
    Ok(obj! {
        "BaseParams" => base_hierarchy(w)?,
        "ChildIds" => { let n = w.u32()?; w.u32s(n)? },
        "Layers" => {
            let n = w.u32()?;
            w.arrv(i64::from(n), |w| {
                Ok(obj! {
                    "LayerId" => w.u32v()?,
                    "Rtpcs" => rtpcs(w)?,
                    "RtpcId" => w.u32v()?,
                    "RtpcType" => EAkGameSyncType.json(u32::from(w.u8()?)),
                    "RtpcCrossfadingDefaultValue" => Value::Int(0),
                    "Associations" => {
                        let n = w.i32()?;
                        w.arrv(i64::from(n), |w| {
                            Ok(obj! {
                                "AssociatedChildId" => w.u32v()?,
                                "GraphPoints" => { let n = w.u32()?; w.arrv(i64::from(n), graph_point)? },
                            })
                        })?
                    },
                })
            })?
        },
        "IsContinuousValidation" => w.bv()?,
    })
}

fn meter_info(w: &mut W) -> Result<Value> {
    Ok(obj! {
        "GridPeriod" => w.d()?,
        "GridOffset" => w.d()?,
        "Tempo" => w.f()?,
        "TimeSigNumBeatsBar" => w.u8v()?,
        "TimeSigBeatValue" => w.u8v()?,
        "MeterInfoFlag" => w.bv()?,
    })
}
fn stingers(w: &mut W) -> Result<Value> {
    let n = w.u32()?;
    w.arrv(i64::from(n), |w| {
        Ok(obj! {
            "TriggerId" => w.u32v()?,
            "SegmentId" => w.u32v()?,
            "SyncPlayAt" => EAkSyncType.json(w.u32()?),
            "CueFilterHash" => w.u32v()?,
            "DontRepeatTime" => w.i32v()?,
            "NumSegmentLookAhead" => w.u32v()?,
        })
    })
}
fn music_fade(w: &mut W) -> Result<Value> {
    Ok(obj! {
        "TransitionTime" => w.i32v()?,
        "FadeCurve" => EAkCurveInterpolation.json(w.u32()?),
        "FadeOffset" => w.i32v()?,
    })
}
fn transition_rules(w: &mut W) -> Result<Value> {
    let n = w.u32()?;
    w.arrv(i64::from(n), |w| {
        let n = w.i32()?;
        let src_ids = w.arrv(i64::from(n), |w| w.i32v())?;
        let n = w.i32()?;
        let dest_ids = w.arrv(i64::from(n), |w| w.i32v())?;
        let src = obj! {
            "TransitionTime" => w.i32v()?,
            "FadeCurve" => EAkCurveInterpolation.json(w.u32()?),
            "FadeOffset" => w.i32v()?,
            "SyncType" => EAkSyncType.json(w.u32()?),
            "MarkerId" => Value::Int(0),
            "CueFilterHash" => w.u32v()?,
            "PlayPostExit" => w.bv()?,
        };
        let dest = obj! {
            "TransitionTime" => w.i32v()?,
            "FadeCurve" => EAkCurveInterpolation.json(w.u32()?),
            "FadeOffset" => w.i32v()?,
            "MarkerId" => Value::Int(0),
            "CueFilterHash" => w.u32v()?,
            "JumpToId" => w.u32v()?,
            "JumpToType" => EAkJumpToSelType.json(u32::from(w.u16()?)),
            "EntryType" => EAkEntryType.json(u32::from(w.u16()?)),
            "PlayPreEntry" => w.bv()?,
            "DestMatchSourceCueName" => w.bv()?,
        };
        let trans = if w.b()? {
            obj! {
                "SegmentId" => w.u32v()?,
                "FadeInParams" => music_fade(w)?,
                "FadeOutParams" => music_fade(w)?,
                "PlayPreEntry" => w.bv()?,
                "PlayPostExit" => w.bv()?,
            }
        } else {
            Value::Null
        };
        Ok(obj! {
            "SrcIds" => src_ids,
            "DestIds" => dest_ids,
            "SrcRules" => src,
            "DestRules" => dest,
            "TransObject" => trans,
        })
    })
}

/// BaseHierarchyMusic's head: Flags, ContainerHierarchy, ChildIds.
fn music_base(w: &mut W) -> Result<Props> {
    let mut o = Props::new();
    o.insert("Flags".to_owned(), EMusicFlags.text_value(u32::from(w.u8()?)));
    o.insert("ContainerHierarchy".to_owned(), base_hierarchy(w)?);
    let n = w.u32()?;
    o.insert("ChildIds".to_owned(), w.u32s(n)?);
    Ok(o)
}

fn music_segment(w: &mut W) -> Result<Value> {
    let mut o = music_base(w)?;
    o.insert("MeterInfo".to_owned(), meter_info(w)?);
    stingers(w)?;
    o.insert("Duration".to_owned(), w.d()?);
    let n = w.u32()?;
    let markers = w.arrv(i64::from(n), |w| {
        let id = w.u32v()?;
        let position = w.d()?;
        let n = w.i32()?;
        Ok(obj! { "Id" => id, "Position" => position, "MarkerName" => Value::Str(w.latin1(n)?) })
    })?;
    o.insert("Markers".to_owned(), markers);
    Ok(Value::Struct(o))
}

fn music_track(w: &mut W) -> Result<Value> {
    let flags = EMusicFlags.text_value(u32::from(w.u8()?));
    let n = w.u32()?;
    let sources = w.arrv(i64::from(n), bank_source)?;
    let n = w.u32()?;
    let playlist = if n != 0 {
        let p = w.arrv(i64::from(n), |w| {
            Ok(obj! {
                "TrackId" => w.u32v()?,
                "SourceId" => w.u32v()?,
                "CacheId" => Value::Int(0),
                "EventId" => w.u32v()?,
                "PlayAt" => w.d()?,
                "BeginTrimOffset" => w.d()?,
                "EndTrimOffset" => w.d()?,
                "SrcDuration" => w.d()?,
            })
        })?;
        w.u32()?;
        p
    } else {
        empty()
    };
    let n = w.i32()?;
    let clips = w.arrv(i64::from(n), |w| {
        Ok(obj! {
            "ClipIndex" => w.u32v()?,
            "AutoType" => EAkClipAutomationType.json(w.u32()?),
            "GraphPoints" => { let n = w.u32()?; w.arrv(i64::from(n), graph_point)? },
        })
    })?;
    let base = base_hierarchy(w)?;
    let ty = w.u8()?;
    let mut o = Props::new();
    o.insert("MusicFlags".to_owned(), flags);
    o.insert("Sources".to_owned(), sources);
    o.insert("Playlist".to_owned(), playlist);
    o.insert("ClipAutomations".to_owned(), clips);
    o.insert("BaseParams".to_owned(), base);
    o.insert("MusicTrackType".to_owned(), EAkMusicTrackType.text_value(u32::from(ty)));
    if ty == 3 {
        let switch = obj! {
            "GroupType" => EAkGroupType.json(u32::from(w.u8()?)),
            "GroupId" => w.u32v()?,
            "DefaultSwitch" => w.u32v()?,
            "SwitchAssociationIds" => { let n = w.u32()?; w.u32s(n)? },
        };
        o.insert("SwitchParams".to_owned(), switch);
        let trans = obj! {
            "SourceFadeParams" => music_fade(w)?,
            "SyncType" => EAkSyncType.json(w.u32()?),
            "CueFilterHash" => w.u32v()?,
            "DestinationFadeParams" => music_fade(w)?,
        };
        o.insert("TransParams".to_owned(), trans);
    }
    let look = w.i32()?;
    if look != 0 {
        o.insert("LookAheadTime".to_owned(), int(look));
    }
    Ok(Value::Struct(o))
}

/// One AkDecisionTree node as read.
struct Node {
    key: u32,
    audio_node: u32,
    children_index: u16,
    children_count: u16,
    weight: u16,
    probability: u16,
    children: Vec<Node>,
}

impl Node {
    fn value(self) -> Value {
        obj! {
            "Key" => uint(self.key),
            "AudioNodeId" => uint(self.audio_node),
            "ChildrenIndex" => uint(self.children_index),
            "ChildrenCount" => uint(self.children_count),
            "Weight" => uint(self.weight),
            "Probability" => uint(self.probability),
            "Children" => Value::Array(self.children.into_iter().map(Node::value).collect()),
        }
    }
}

/// AkDecisionTree's ParseDecisionTree (12-byte items): a level of `count` nodes, then each node's children in turn.
/// A node is an audio node (a leaf) at the tree's depth, or where its children's u16 pair cannot be one.
fn decision_level(w: &mut W, count: usize, cur: u32, depth: u32, count_max: u32) -> Result<Vec<Node>> {
    let mut nodes = Vec::with_capacity(count.min(w.ar.remaining() / 12 + 1));
    for _ in 0..count {
        let key = w.u32()?;
        let peek = w.peek_u32()?;
        let (lo, hi) = (peek & 0xffff, peek >> 16);
        let audio = lo > count_max || hi > count_max || w.ar.pos + hi as usize * 12 > w.ar.len();
        let (mut audio_node, mut children_index, mut children_count) = (0, 0, 0);
        if audio || cur == depth {
            audio_node = w.u32()?;
        } else {
            children_index = w.u16()?;
            children_count = w.u16()?;
        }
        let weight = w.u16()?;
        let probability = w.u16()?;
        nodes.push(Node { key, audio_node, children_index, children_count, weight, probability, children: Vec::new() });
    }
    for n in &mut nodes {
        if n.children_count > 0 {
            n.children = decision_level(w, usize::from(n.children_count), cur + 1, depth, count_max)?;
        }
    }
    Ok(nodes)
}

fn music_switch(w: &mut W) -> Result<Value> {
    let mut o = music_base(w)?;
    o.insert("MeterInfo".to_owned(), meter_info(w)?);
    o.insert("Stingers".to_owned(), stingers(w)?);
    o.insert("MusicTransitionRule".to_owned(), transition_rules(w)?);
    o.insert("IsContinuePlayback".to_owned(), w.bv()?);
    let depth = w.u32()?;
    let ids = w.arr(i64::from(depth), |w| w.u32())?;
    let types = w.arr(i64::from(depth), |w| w.u8())?;
    let size = w.u32()?;
    o.insert("Mode".to_owned(), EAkDecisionTreeMode.text_value(u32::from(w.u8()?)));
    let args = ids
        .into_iter()
        .zip(types)
        .map(|(id, t)| obj! { "GroupId" => uint(id), "GroupType" => EAkGroupType.json(u32::from(t)) })
        .collect();
    o.insert("Arguments".to_owned(), Value::Array(args));
    let nodes = decision_level(w, 1, 0, depth, size / 12)?;
    let nodes = nodes.into_iter().map(Node::value).collect();
    o.insert("DecisionTree".to_owned(), obj! { "Nodes" => Value::Array(nodes) });
    Ok(Value::Struct(o))
}

fn ran_seq_item(w: &mut W) -> Result<Value> {
    let segment = w.u32v()?;
    let playlist_item = w.u32v()?;
    let n = w.u32()?;
    let rs_type = w.i32()?;
    w.hid.rs_type.push(rs_type);
    let loop_info = obj! { "Loop" => int(w.i16()?), "LoopMin" => int(w.i16()?), "LoopMax" => int(w.i16()?) };
    let weight_info = obj! {
        "Weight" => uint(w.u32()? & 0xffff),
        "AvoidRepeatCount" => w.u16v()?,
        "IsUsingWeight" => w.u8v()?,
        "IsShuffle" => w.u8v()?,
    };
    let children = w.arrv(i64::from(n), ran_seq_item)?;
    Ok(obj! {
        "SegmentId" => segment,
        "PlaylistItemId" => playlist_item,
        "NumChildren" => uint(n),
        "Children" => children,
        "LoopInfo" => loop_info,
        "WeightInfo" => weight_info,
    })
}

fn music_random_sequence(w: &mut W) -> Result<Value> {
    let mut o = music_base(w)?;
    o.insert("MeterInfo".to_owned(), meter_info(w)?);
    o.insert("Stingers".to_owned(), stingers(w)?);
    o.insert("MusicTransitionRule".to_owned(), transition_rules(w)?);
    w.u32()?;
    o.insert("Playlist".to_owned(), Value::Array(vec![ran_seq_item(w)?]));
    Ok(Value::Struct(o))
}

fn attenuation(w: &mut W) -> Result<Value> {
    let cone = w.u8()? & 1 != 0;
    let mut o = Props::new();
    o.insert("IsHeightSpreadEnabled".to_owned(), Value::Bool(false));
    o.insert("IsConeEnabled".to_owned(), Value::Bool(cone));
    if cone {
        let p = obj! {
            "fInsideDegrees" => w.f()?,
            "fOutsideDegrees" => w.f()?,
            "fOutsideVolume" => w.f()?,
            "LoPass" => w.f()?,
            "HiPass" => w.f()?,
        };
        o.insert("ConeParams".to_owned(), p);
    }
    let use_curves = w.arr(7, |w| w.i8())?; // the curve to use per attenuation property
    w.hid.curve_to_use = use_curves.try_into().ok();
    let n = w.u8()?;
    o.insert("Curves".to_owned(), w.arrv(i64::from(n), super::conversion_table)?);
    o.insert("RTPCs".to_owned(), rtpcs(w)?);
    Ok(Value::Struct(o))
}

/// FxShareSet and FxCustom; AudioDevice adds to it.
fn fx_base_props(w: &mut W) -> Result<Props> {
    let raw = plugin_id(w)?;
    let params = plugin_params(w, raw)?;
    let n = w.u8()?;
    let media = w.arrv(i64::from(n), |w| Ok(obj! { "Index" => w.u8v()?, "SourceId" => w.u32v()? }))?;
    let rtpc = rtpcs(w)?;
    let groups = state_groups(w)?;
    let n = w.u16()?;
    let values = w.arrv(i64::from(n), |w| {
        Ok(obj! { "PropertyId" => int(w.v7()?), "RtpcAccum" => w.u8v()?, "Value" => w.f()? })
    })?;
    let mut o = Props::new();
    o.insert("MediaList".to_owned(), media);
    o.insert("RTPCs".to_owned(), rtpc);
    o.insert("StateGroups".to_owned(), groups);
    o.insert("RtpcInitList".to_owned(), empty());
    o.insert("PluginPropertyValues".to_owned(), values);
    if let Some(p) = params {
        o.insert("Plugin".to_owned(), plugin_json(raw));
        o.insert("PluginParams".to_owned(), p);
    }
    Ok(o)
}

fn fx_base(w: &mut W) -> Result<Value> {
    Ok(Value::Struct(fx_base_props(w)?))
}

fn audio_device(w: &mut W) -> Result<Value> {
    let mut o = fx_base_props(w)?;
    // FxParams is read only past v136: default(AkFxParams), its array null
    o.insert("FxParams".to_owned(), obj! { "BypassAll" => Value::Bool(false), "Effects" => Value::Null });
    Ok(Value::Struct(o))
}

/// LFO, Envelope and TimeMod.
fn modulator(w: &mut W) -> Result<Value> {
    Ok(obj! { "Props" => props(w)?, "PropRanges" => prop_ranges(w)?, "RtpcCurves" => rtpcs(w)? })
}
