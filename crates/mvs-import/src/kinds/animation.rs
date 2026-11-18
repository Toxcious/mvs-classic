//! The `animation` kind: every AnimSequence as an MVA3 container: the header from the
//! export's tagged properties and its native tail (`mvs_uasset::natives::anim_sequence`), and the body of two
//! sections. `tracks`: per compressed track four int32 (bone, rot, trans, scale as the game's AnimTrack names them).
//! `data`: every track's blocks in the build's AKF_PerTrackCompression form (`natives::anim_codec`: the build's stream
//! as it is, AKF_VariableKeyLerp and AKF_ConstantKeyLerp rewritten into the same blocks), curve keys decoded at
//! import.//!
//! The track map (CompressedTrackToSkeletonMapTable) is checked against the sequence's Skeleton: every bone index at
//! least 0 and below the reference skeleton's bone count plus its VirtualBones (the Skeleton's tagged property), else
//! the entry fails by name. Virtual bones have tracks: 12,566 track slots of the build map past the reference
//! skeleton's raw bones (Arya's tracks 435-438 to bones 442-445 of a 442-bone skeleton with 4 virtual bones), so the
//! raw count alone would refuse them.
//!
//! A sequence with an empty bone codec handle (the three curve-only RickTeaser face sequences) has no compressed
//! data description and no bone bytes: its entry has no tracks, an empty data section, `frames` its NumFrames, and
//! its curves.
//!
//! 9 sequences fail: their Skeleton is absent from the paks (8 WonderWomanV2/TempAnimations on Hulk's,
//! Gandalf's eagle).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use mvs_uasset::PackageCache;
use mvs_uasset::natives::anim_codec::{AnimCurve, decode_curves, track_blocks};
use mvs_uasset::natives::anim_sequence::{AnimSequenceTail, read_anim_sequence_tail};
use mvs_uasset::natives::skeleton::read_skeleton;
use mvs_uasset::typed::{BodySink, f32_dotnet, to_asset_object};
use mvs_uasset::value::{Props, Value};
use serde::{Deserialize, Serialize};

use crate::json::Json;
use crate::jsonbin::{BodyRef, json_bin, lay_out_body};
use crate::kind::{Cx, Item, Kind, KindError, Memo};

/// The kind's version.
pub const ANIMATION_KIND_VERSION: u64 = 3;
/// The container's magic.
pub const ANIM_MAGIC: &[u8; 4] = b"MVA3";
/// The header's `version`.
const ANIM_FORMAT_VERSION: u32 = 2;
const ANIM_SEQUENCE: &str = "/Script/Engine.AnimSequence";

/// A decoded curve key as the header writes it.
#[derive(Serialize)]
struct CurveKey {
    time: f32,
    value: f32,
    arrive: f32,
    leave: f32,
    interp: u8,
}

#[derive(Serialize)]
struct Curve {
    name: String,
    keys: Vec<CurveKey>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Additive {
    #[serde(rename = "type")]
    type_: usize,
    ref_pose_type: usize,
    ref_pose_seq: Option<String>,
    ref_frame_index: f64,
}

/// The MVA3 header.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AnimHeader {
    version: u32,
    path: String,
    skeleton: String,
    frames: i32,
    num_frames: f64,
    length: f64,
    rate_scale: f64,
    interpolation: usize,
    root_motion: bool,
    force_root_lock: bool,
    root_lock: usize,
    normalized_root_motion_scale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    additive: Option<Additive>,
    tracks: BodyRef,
    curves: Vec<Curve>,
    data: BodyRef,
}

fn fail<T>(msg: String) -> Result<T, KindError> {
    Err(KindError::Failed(msg))
}

/// A value as the kind's JSON shows it in a message.
fn shown(v: &Value) -> String {
    match v {
        Value::Str(s) => format!("{s:?}"),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        other => format!("{other:?}"),
    }
}

/// An enum property's value name without its `Enum::` prefix, by its index in `names`; `fallback` when unset.
fn enum_index(props: &Props, key: &str, names: &[&str], fallback: usize, what: &str) -> Result<usize, KindError> {
    let Some(v) = props.get(key).filter(|v| !matches!(v, Value::Null)) else { return Ok(fallback) };
    let name = v.as_str().map_or("", |s| &s[s.rfind(':').map_or(0, |i| i + 1)..]);
    match names.iter().position(|n| *n == name) {
        Some(i) => Ok(i),
        None => fail(format!("{what}: {key} {} is none of {}", shown(v), names.join(", "))),
    }
}

/// A number property (a float as the object kind writes it); `fallback` when unset.
fn num(props: &Props, key: &str, fallback: f64, what: &str) -> Result<f64, KindError> {
    let Some(v) = props.get(key).filter(|v| !matches!(v, Value::Null)) else { return Ok(fallback) };
    match v {
        Value::Int(i) => Ok(*i as f64),
        Value::UInt(u) => Ok(*u as f64),
        Value::Float(f) if f.is_finite() => Ok(f32_dotnet(*f)),
        Value::Double(d) if d.is_finite() => Ok(*d),
        _ => fail(format!("{what}: {key} {} is not a number", shown(v))),
    }
}

/// An object path property: the path a reference resolves to, a soft path as the object kind writes it; `None` for
/// none.
fn reference(cache: &PackageCache, props: &Props, key: &str, what: &str) -> Result<Option<String>, KindError> {
    let Some(v) = props.get(key) else { return Ok(None) };
    match v {
        Value::Null | Value::Ref(None) => Ok(None),
        Value::Ref(Some(r)) => Ok(Some(r.object_path(cache))),
        Value::SoftPath { asset, sub } => Ok(if asset == "None" || asset.is_empty() {
            None
        } else if sub.is_empty() {
            Some(asset.clone())
        } else {
            Some(format!("{asset}:{sub}"))
        }),
        other => fail(format!("{what}: {key} {} is not an object path", shown(other))),
    }
}

fn flag(props: &Props, key: &str) -> Option<bool> {
    props.get(key).and_then(Value::as_bool)
}

/// The bones a track may map to: the reference skeleton's and the Skeleton's VirtualBones.
fn skeleton_bone_count(cx: &Cx<'_>, skeleton: &str) -> Result<usize, KindError> {
    let (pkg, i) = cx.read(skeleton)?;
    let obj = cx.cache.object(&pkg, i);
    let virtual_bones = match obj.props.get("VirtualBones") {
        None | Some(Value::Null) => 0,
        Some(Value::Array(a)) => a.len(),
        Some(_) => return fail(format!("{}: VirtualBones is not an array", pkg.path)),
    };
    Ok(read_skeleton(cx.cache, &pkg, i)?.bones.len() + virtual_bones)
}

/// Per import run, each Skeleton's bone count with its virtual bones (read once, a failure kept).
#[derive(Default)]
struct BoneCounts(Mutex<HashMap<String, BoneSlot>>);

type BoneSlot = Arc<OnceLock<Result<usize, String>>>;

fn bones_of(cx: &Cx<'_>, skeleton: &str) -> Result<usize, KindError> {
    let counts = cx.shared.get_or_init(BoneCounts::default);
    let slot = counts.0.lock().unwrap_or_else(|e| e.into_inner()).entry(skeleton.to_owned()).or_default().clone();
    slot.get_or_init(|| {
        skeleton_bone_count(cx, skeleton)
            .map_err(|e| format!("its Skeleton {skeleton} does not read, so its track map cannot be checked: {e}"))
    })
    .clone()
    .map_err(KindError::Failed)
}

/// Fails, naming the sequence, unless every track of `map` maps to a bone below `bones`.
fn check_track_map(path: &str, skeleton: &str, map: &[i32], bones: usize) -> Result<(), KindError> {
    let bad = map.iter().position(|&b| usize::try_from(b).map_or(true, |b| b >= bones));
    match bad {
        Some(i) => fail(format!(
            "{path}: track {i} maps to bone {}; its Skeleton {skeleton} has {bones} bones with the virtual ones",
            map[i]
        )),
        None => Ok(()),
    }
}

/// The entry of the sequence at `path`, from its tagged properties and its tail (the track map unchecked:
/// [`check_track_map`]).
fn anim_entry(
    cache: &PackageCache,
    path: &str,
    props: &Props,
    tail: &AnimSequenceTail<'_>,
) -> Result<Vec<u8>, KindError> {
    let cd = tail.compressed_data_structure.as_ref();
    // Unverified: the frame count of the key lookup and of the frame tables' width is taken as
    // CompressedNumberOfFrames (the 4.26 decompression context reads the compressed data's own count; the native
    // answers say "NumFrames" without naming which). They differ on one sequence of the build:
    // Creature_NoMoveFall, NumFrames 21, compressed 3. A sequence with no compressed data (no bone codec) has no
    // keys: its NumFrames.
    let compressed_frames = cd.map_or(0, |c| c.compressed_number_of_frames);
    let num_frames = num(props, "NumFrames", f64::from(compressed_frames), path)?;
    // (`frames` is the NumFrames number itself, an integer of the property)
    let frames = cd.map_or_else(|| num_frames as i32, |c| c.compressed_number_of_frames);
    let Some(skeleton) = reference(cache, props, "Skeleton", path)? else {
        return fail(format!("{path}: no Skeleton"));
    };
    let blocks = if cd.is_some() { track_blocks(tail, path)? } else { Default::default() };
    let map = if cd.is_some() { tail.compressed_track_to_skeleton_map_table.as_slice() } else { &[] };
    let mut tracks = Vec::with_capacity(map.len() * 16);
    for (bone, o) in map.iter().zip(&blocks.offsets) {
        for v in [*bone, o.rot, o.trans, o.scale] {
            tracks.extend_from_slice(&v.to_le_bytes());
        }
    }
    let sections = [tracks, blocks.data];
    let (refs, _) = lay_out_body(&sections);
    let additive_type = enum_index(
        props,
        "AdditiveAnimType",
        &["AAT_None", "AAT_LocalSpaceBase", "AAT_RotationOffsetMeshSpace"],
        0,
        path,
    )?;
    let additive = if additive_type != 0 {
        Some(Additive {
            type_: additive_type,
            ref_pose_type: enum_index(
                props,
                "RefPoseType",
                &["ABPT_None", "ABPT_RefPose", "ABPT_AnimScaled", "ABPT_AnimFrame"],
                1,
                path,
            )?,
            ref_pose_seq: reference(cache, props, "RefPoseSeq", path)?,
            ref_frame_index: num(props, "RefFrameIndex", 0.0, path)?,
        })
    } else {
        None
    };
    let header = AnimHeader {
        version: ANIM_FORMAT_VERSION,
        path: path.to_owned(),
        skeleton,
        frames,
        num_frames,
        length: num(props, "SequenceLength", 0.0, path)?,
        rate_scale: num(props, "RateScale", 1.0, path)?,
        interpolation: enum_index(props, "Interpolation", &["Linear", "Step"], 0, path)?,
        root_motion: flag(props, "bEnableRootMotion") == Some(true),
        force_root_lock: flag(props, "bForceRootLock") == Some(true),
        root_lock: enum_index(props, "RootMotionRootLock", &["RefPose", "AnimFirstFrame", "Zero"], 0, path)?,
        normalized_root_motion_scale: flag(props, "bUseNormalizedRootMotionScale") != Some(false),
        additive,
        tracks: refs[0],
        curves: decode_curves(tail, path)?.into_iter().map(curve).collect(),
        data: refs[1],
    };
    Ok(json_bin(ANIM_MAGIC, &header, &sections, &refs)?)
}

fn curve(c: AnimCurve) -> Curve {
    let keys = c
        .keys
        .iter()
        .map(|k| CurveKey { time: k.time, value: k.value, arrive: k.arrive, leave: k.leave, interp: k.interp })
        .collect();
    Curve { name: c.name, keys }
}

// the header as a game kind reads it

/// An animation entry's header as a game kind reads it (parsed back, numbers as
/// JSON.parse reads them): the curves typed, the rest as written.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnimationHeader {
    pub version: f64,
    pub path: String,
    pub skeleton: String,
    pub frames: f64,
    pub num_frames: f64,
    pub length: f64,
    pub rate_scale: f64,
    pub interpolation: f64,
    pub root_motion: bool,
    pub force_root_lock: bool,
    pub root_lock: f64,
    pub normalized_root_motion_scale: bool,
    pub additive: Option<Json>,
    pub tracks: Json,
    pub curves: Vec<HeaderCurve>,
    pub data: Json,
}

/// A float curve of the header: its name and its keys.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderCurve {
    pub name: String,
    pub keys: Vec<HeaderCurveKey>,
}

/// A curve key: `interp` 0 linear, 1 constant, 2 cubic.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderCurveKey {
    pub time: f64,
    pub value: f64,
    pub arrive: f64,
    pub leave: f64,
    pub interp: f64,
}

/// The run's headers, by path.
#[derive(Default)]
struct Headers(Memo<Result<Arc<AnimationHeader>, KindError>>);

/// The header of the animation entry at `path` as the kind builds it, parsed back; built once per run (the
/// precedent of `lighting::entry_header`).
pub fn header(cx: &Cx<'_>, path: &str) -> Result<Arc<AnimationHeader>, KindError> {
    cx.shared.get_or_init(Headers::default).0.get(path, || {
        let bytes = Animation.build(cx, &Item::new(path))?;
        let what = format!("animation {path}");
        let e = mvs_material::jsonbin::JsonBin::split(ANIM_MAGIC, &bytes, &what)
            .map_err(|e| KindError::Failed(e.to_string()))?;
        e.parse::<AnimationHeader>(&what).map(Arc::new).map_err(|e| KindError::Failed(e.to_string()))
    })
}

/// The `animation` kind.
pub struct Animation;

impl Kind for Animation {
    fn id(&self) -> &'static str {
        "animation"
    }

    fn version(&self) -> u64 {
        ANIMATION_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/octet-stream"
    }

    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(cx.catalog.paths_of(&[ANIM_SEQUENCE]).into_iter().map(Item::new).collect())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let path = &item.path;
        let (pkg, i) = cx.read(path)?;
        let o = to_asset_object(cx.cache, &pkg, i, &mut BodySink::default())?;
        if o.class != ANIM_SEQUENCE {
            return Err(KindError::NotFound(format!("animation: {path} is a {}, not an AnimSequence", o.class)));
        }
        let tail = read_anim_sequence_tail(cx.cache, &pkg, i)?;
        let bytes = anim_entry(cx.cache, path, &o.properties, &tail)?;
        let skeleton = reference(cx.cache, &o.properties, "Skeleton", path)?.expect("anim_entry found a Skeleton");
        let map: &[i32] =
            if tail.compressed_data_structure.is_some() { &tail.compressed_track_to_skeleton_map_table } else { &[] };
        check_track_map(path, &skeleton, map, bones_of(cx, &skeleton)?)?;
        Ok(bytes)
    }
}
