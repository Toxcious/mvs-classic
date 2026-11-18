//! The blend spaces an AnimBP plays through its BlendSpacePlayer nodes, bound from the build as BlendSpacePlayerData: a
//! BlendSpace or BlendSpace1D's axes, samples and cooked grid, and a node's player members. The boxes bind's skel block
//! reads the spaces (`boxes/skel.rs`); the fighter kind writes the players of the port's table
//! (`fighter_port::BLEND_SPACES`) into its entry's `blendSpaces`, which the view's player runs.
//!
//! Per node the class default's AnimGraphNode_BlendSpacePlayer members (bLoop, PlayRate, StartPosition,
//! bResetPlayTimeWhenBlendSpaceChanges, GroupName), its BlendSpace / BlendSpace1D object (BlendParameters, SampleData,
//! GridSamples) and its samples' AnimSequence flags (AdditiveAnimType, RefPoseType, RefPoseSeq, RefFrameIndex: the
//! tagged properties). Per axis Min, Max and GridNum (the constructor's 0, 100 and 4 where the asset serialises none),
//! the SampleData rows and the cooked GridSamples rows as they are (three sample indices and three weights a grid
//! point: the runtime does not triangulate). Anything else an asset serialises is a feature the player does not have
//! (input interpolation with a time, target weight interpolation, per-bone blend, axis scaling, sync markers, a notify
//! trigger mode other than the default, the AimOffset classes' mesh space blend), and a node in a sync group is not
//! ported: the bind stops on each instead of dropping it.

use indexmap::IndexMap;
pub use mvs_anim::blend_space::{BlendAxis, BlendSample, BlendSpaceData, BlendSpacePlayerData, player_key};
use mvs_data::{Props, Value};

use super::boxes::export_name;
use super::shape::{cdo_path, defaults_or_properties, fail, properties};
use crate::kind::{Cx, KindError};

/// FBlendParameter's defaults where the asset serialises no member (UBlendSpaceBase's constructor).
const AXIS_DEFAULT: (&str, f64, f64, f64) = ("None", 0.0, 100.0, 4.0);

/// The classes the player covers, by their dimension count (an AimOffsetBlendSpace / AimOffsetBlendSpace1D is
/// refused).
fn dims_of(class: &str) -> Option<usize> {
    match class {
        "/Script/Engine.BlendSpace1D" => Some(1),
        "/Script/Engine.BlendSpace" => Some(2),
        _ => None,
    }
}

/// The properties the player covers; `Skeleton` is not read, `InterpolationParam` is checked below.
const KNOWN: [&str; 7] = [
    "SampleData",
    "GridSamples",
    "BlendParameters",
    "BlendParameters[1]",
    "BlendParameters[2]",
    "Skeleton",
    "InterpolationParam",
];
const AXIS_KEYS: [&str; 4] = ["DisplayName", "Min", "Max", "GridNum"];
const SAMPLE_KEYS: [&str; 3] = ["Animation", "SampleValue", "RateScale"];

fn num(v: Option<&Value>, d: f64) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(d)
}

/// An enum value with or without its type's prefix (`EAdditiveAnimationType::AAT_None` / `AAT_None`).
fn enum_name<'a>(v: Option<&'a Value>, d: &'a str) -> &'a str {
    match v.and_then(Value::as_str) {
        Some(s) => s.rsplit_once(':').map_or(s, |(_, n)| n),
        None => d,
    }
}

/// The nodes a key's drawing runs (`fighter_port::BLEND_SPACES`), by `<AnimBP class>:<node>` in its order.
pub fn bind_blend_space_players(
    cx: &Cx<'_>,
    nodes: &[(&str, &str)],
) -> Result<IndexMap<String, BlendSpacePlayerData>, KindError> {
    let mut out = IndexMap::new();
    for &(anim_bp, node) in nodes {
        let path = cdo_path(anim_bp);
        let cdo = defaults_or_properties(&cx.object(&path)?)?;
        out.insert(player_key(anim_bp, node), player_of(cx, &cdo, &path, node)?);
    }
    Ok(out)
}

/// One BlendSpace / BlendSpace1D asset by its object path, bound as a player's `space`.
pub fn bind_blend_space(cx: &Cx<'_>, path: &str) -> Result<BlendSpaceData, KindError> {
    space_of(cx, path)
}

fn player_of(cx: &Cx<'_>, cdo: &Props, cdo_path: &str, node: &str) -> Result<BlendSpacePlayerData, KindError> {
    let where_ = format!("{}.{node}", export_name(cdo_path));
    let n = cdo.get(node).and_then(Value::as_obj);
    let Some((n, space)) = n.and_then(|n| Some((n, n.get("BlendSpace")?.as_str()?))) else {
        return fail(format!("{where_}: no BlendSpacePlayer node with a blend space"));
    };
    let group = enum_name(n.get("GroupName"), "None");
    if group != "None" {
        return fail(format!("{where_}: in the sync group {group} (not ported)"));
    }
    Ok(BlendSpacePlayerData {
        node: node.to_owned(),
        looped: n.get("bLoop").is_none_or(|v| v.as_bool() == Some(true)),
        play_rate: num(n.get("PlayRate"), 1.0),
        start_position: num(n.get("StartPosition"), 0.0),
        reset_on_change: n.get("bResetPlayTimeWhenBlendSpaceChanges").is_none_or(|v| v.as_bool() == Some(true)),
        space: space_of(cx, space)?,
    })
}

// (`!(a > b)`: a NaN range is refused too)
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn space_of(cx: &Cx<'_>, path: &str) -> Result<BlendSpaceData, KindError> {
    let e = cx.object(path)?;
    let name = export_name(e.path()).to_owned();
    let class = e.class();
    let Some(dims) = dims_of(class) else {
        return fail(format!("{name}: class {class} is not ported (mesh space rotation blend)"));
    };
    let p = properties(&e)?;
    // An input interpolation with no InterpolationTime (the asset serialises the type alone: Batman_Nav_GlideLoop_
    // BlendSpace, BSIT_Cubic): taken as no filter, the input as it is.
    // Unverified: the filter's GetFilteredData with a time of 0 is not read in the exe; a time above 0 stops the bind.
    let interp: Vec<&Value> = match p.get("InterpolationParam") {
        None => Vec::new(),
        Some(Value::Arr(a)) => a.iter().collect(),
        Some(v) => vec![v],
    };
    for ip in interp {
        let t = num(ip.as_obj().and_then(|o| o.get("InterpolationTime")), 0.0);
        if t > 0.0 {
            return fail(format!("{name}: an input interpolation time of {t} (the player has no filter)"));
        }
    }
    if let Some(k) = p.keys().find(|k| !KNOWN.contains(&k.as_str())) {
        return fail(format!("{name}: {k} is serialised and the player has no rule for it"));
    }
    let empty = Props::new();
    let mut axes = Vec::new();
    for a in 0..dims {
        let key = if a == 0 { "BlendParameters".to_owned() } else { format!("BlendParameters[{a}]") };
        let b = p.get(&key).and_then(Value::as_obj).unwrap_or(&empty);
        if let Some(k) = b.keys().find(|k| !AXIS_KEYS.contains(&k.as_str())) {
            return fail(format!("{name}: BlendParameters.{k}"));
        }
        let axis = BlendAxis {
            name: b.get("DisplayName").and_then(Value::as_str).unwrap_or(AXIS_DEFAULT.0).to_owned(),
            min: num(b.get("Min"), AXIS_DEFAULT.1),
            max: num(b.get("Max"), AXIS_DEFAULT.2),
            grid: num(b.get("GridNum"), AXIS_DEFAULT.3),
        };
        if !(axis.max > axis.min) || axis.grid < 1.0 {
            return fail(format!("{name}: axis {a} range"));
        }
        axes.push(axis);
    }
    for a in dims..3 {
        if p.contains_key(&format!("BlendParameters[{a}]")) {
            return fail(format!("{name}: BlendParameters[{a}] on a {}", export_name(class)));
        }
    }
    let rows = p.get("SampleData").and_then(Value::as_arr).unwrap_or(&[]);
    // (a sequence two samples share is read once)
    let mut seqs: IndexMap<String, Props> = IndexMap::new();
    let mut additive: Option<bool> = None;
    let mut samples = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        let Some(r) = r.as_obj() else { return fail(format!("{name}: SampleData[{i}] is no struct")) };
        if let Some(k) = r.keys().find(|k| !SAMPLE_KEYS.contains(&k.as_str())) {
            return fail(format!("{name}: SampleData[{i}].{k}"));
        }
        let Some(anim) = r.get("Animation").and_then(Value::as_str) else {
            return fail(format!("{name}: SampleData[{i}] has no animation"));
        };
        let sv = r.get("SampleValue").and_then(Value::as_obj).unwrap_or(&empty);
        let clip = export_name(anim).to_owned();
        if !seqs.contains_key(anim) {
            seqs.insert(anim.to_owned(), properties(&cx.object(anim)?)?);
        }
        let seq = &seqs[anim];
        let ty = enum_name(seq.get("AdditiveAnimType"), "AAT_None");
        let mut out = BlendSample {
            clip: clip.clone(),
            path: anim.to_owned(),
            x: num(sv.get("X"), 0.0),
            y: num(sv.get("Y"), 0.0),
            rate: num(r.get("RateScale"), 1.0),
            ref_pose: None,
        };
        if num(sv.get("Z"), 0.0) != 0.0 {
            return fail(format!("{name}: SampleData[{i}] has a Z"));
        }
        if ty != "AAT_None" {
            let ref_ty = enum_name(seq.get("RefPoseType"), "ABPT_None");
            let ref_seq = seq.get("RefPoseSeq").and_then(Value::as_str);
            let (true, true, Some(ref_seq)) = (ty == "AAT_LocalSpaceBase", ref_ty == "ABPT_AnimFrame", ref_seq) else {
                return fail(format!("{name}: {clip} is additive {ty} over {ref_ty}"));
            };
            let frame = num(seq.get("RefFrameIndex"), 0.0);
            if frame != 0.0 {
                return fail(format!("{name}: {clip} RefFrameIndex {frame}"));
            }
            out.ref_pose = Some(export_name(ref_seq).to_owned());
        }
        let this = ty != "AAT_None";
        if additive.is_some_and(|a| a != this) {
            return fail(format!("{name}: additive and full pose samples together"));
        }
        additive = Some(this);
        samples.push(out);
    }
    if samples.is_empty() {
        return fail(format!("{name}: no samples"));
    }
    let points = if dims == 1 { axes[0].grid + 1.0 } else { (axes[0].grid + 1.0) * (axes[1].grid + 1.0) };
    let gs = p.get("GridSamples").and_then(Value::as_arr).unwrap_or(&[]);
    if gs.len() as f64 != points {
        return fail(format!("{name}: {} GridSamples for {points} grid points", gs.len()));
    }
    let mut grid = Vec::with_capacity(gs.len() * 6);
    for g in gs {
        let o = g.as_obj().unwrap_or(&empty);
        let ix = [num(o.get("Indices"), 0.0), num(o.get("Indices[1]"), 0.0), num(o.get("Indices[2]"), 0.0)];
        for i in ix {
            if i.fract() != 0.0 || i < -1.0 || i >= samples.len() as f64 {
                return fail(format!("{name}: GridSamples index {i}"));
            }
        }
        grid.extend(ix);
        grid.extend([num(o.get("Weights"), 0.0), num(o.get("Weights[1]"), 0.0), num(o.get("Weights[2]"), 0.0)]);
    }
    Ok(BlendSpaceData { name, dims, axes, samples, grid, additive: additive == Some(true) })
}
