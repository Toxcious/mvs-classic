//! A fighter's boxes: the box index and the float32 frames of its hit, hurt and bone boxes, sampled from its
//! montages over its skeleton.
//!
//! Two halves: the index half (`index.rs`, with `shapes.rs` and `sockets.rs`) reads the pawn's components,
//! sockets and clips into a [`BoxBindInput`]; the frame half (`frames.rs`, with `skel.rs`) samples every clip over the
//! mesh's reference skeleton and lays out the index's offsets and the float32 section. Every place and rotation here is
//! UE space (cm, X fwd, Y depth, Z up), bone space for a shape on a bone; the frame half packs actor space.

use indexmap::IndexMap;
use mvs_data::boxes::BoxShape;
use mvs_data::{BoxIndex, F32s, FighterData, Value};

use crate::kind::{Cx, KindError};

use super::transform::{Quat, Vec3};

#[cfg(test)]
mod check;
pub mod consts;
pub mod frames;
pub mod index;
pub mod shapes;
pub mod skel;
pub mod sockets;

/// ACTOR_BONE_BOXES' inputs by class and component.
pub type BoneBoxes = IndexMap<String, IndexMap<String, BoneBoxInput>>;

/// A shape while the bind builds it: the index's BoxShape and its place on
/// its bone.
#[derive(Clone, Debug)]
pub struct Placed {
    pub shape: BoxShape,
    /// RelativeLocation (a component) or the socket's RelativeLocation, on `bone` (or the capsule: on_capsule).
    pub loc: Vec3,
    /// RelativeRotation as a quaternion (the identity for a socket: its own rotation is `srot`).
    pub rot: Quat,
    /// A socket on a virtual bone: the ModifyBone node's scale rule and its bone-space translation.
    pub vb: Option<VbPlace>,
    /// A socket's own RelativeRotation (BoxShape.rot holds it as Pitch, Yaw, Roll) as a quaternion.
    pub srot: Option<Quat>,
}

/// A bound shape's view-box placement: `world` the scale mode BCS_WorldSpace (else 'local').
#[derive(Clone, Copy, Debug)]
pub struct VbPlace {
    pub world: bool,
    pub scale: Vec3,
    pub add: Option<Vec3>,
}

/// A clip the bind samples, by the name the fighter data gives it (the sequence's object name) and its build path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoxClip {
    pub name: String,
    pub path: String,
}

/// What the index half gives the frame half (the index's fields the frame half keeps are
/// `mesh_scale`, `mesh_z` and the shapes').
#[derive(Clone, Debug)]
pub struct BoxBindInput {
    pub key: String,
    /// The fighter's skeletal mesh and its Skeleton (the mesh and skeleton kinds' paths).
    pub mesh: String,
    pub skeleton: String,
    pub mesh_scale: f64,
    pub mesh_z: f64,
    /// The index's shapes with their places, in the index's order.
    pub shapes: Vec<Placed>,
    /// In sampling order (then the boxDirs clips a slot track names).
    pub clips: Vec<BoxClip>,
    /// The clips Base_AnimBP's COM Replace nodes run under: fml_un_COM's rotation is the identity in their frames.
    pub com_replaced: Vec<String>,
    /// The sockets an attached actor takes its whole transform from (BoxShape.axes' order).
    pub axis_sockets: Vec<String>,
    /// By class, a spawned actor whose hit shapes ride its own skeletal mesh.
    pub actors: Option<IndexMap<String, ActorBindInput>>,
    /// By class and component, a spawned actor's BoxComponent on a bone of its own mesh.
    pub bone_boxes: Option<BoneBoxes>,
}

/// An actor's input: its mesh and Skeleton, its mesh component, scale and place, its shapes
/// and the sequences its mesh plays, in name order.
#[derive(Clone, Debug)]
pub struct ActorBindInput {
    pub mesh: String,
    pub skeleton: String,
    pub mesh_comp: String,
    pub mesh_scale: f64,
    pub mesh_z: f64,
    pub shapes: Vec<Placed>,
    pub clips: Vec<BoxClip>,
}

/// A BoxComponent on a bone of a spawned actor's skeletal mesh.
#[derive(Clone, Debug)]
pub struct BoneBoxInput {
    pub mesh: String,
    pub skeleton: String,
    pub bone: String,
    /// RelativeLocation on the bone, BoxExtent (UE cm).
    pub loc: Vec3,
    pub extent: Vec3,
    /// The sequences on the mesh's Skeleton in the mesh's folder's Animations (not its subfolders), in package name
    /// order.
    pub clips: Vec<BoxClip>,
}

/// The key's box index and frames.
pub fn bind_boxes(cx: &Cx<'_>, key: &str, fighter: &FighterData) -> Result<(BoxIndex, F32s), KindError> {
    // The index is written as the boxes entry's JSON header and the fighter data is read from JSON, where a -0 comes
    // back as 0 (the skel block's shapeRot): the bind takes both through the same
    // round trip (the float32 section is bytes, its -0s kept).
    let fighter: FighterData = js_round_trip(key, fighter)?;
    let input = index::bind_box_input(cx, key, &fighter)?;
    let (index, data) = frames::sample_box_frames(cx, &input, &fighter)?;
    let index: BoxIndex = js_round_trip(key, &index)?;
    Ok((index, F32s(data)))
}

/// `v` as JSON.stringify writes it and JSON.parse reads it back.
fn js_round_trip<T: serde::Serialize + serde::de::DeserializeOwned>(key: &str, v: &T) -> Result<T, KindError> {
    let fail = |e: String| KindError::Failed(format!("boxes {key}: the JSON round trip: {e}"));
    let json = crate::json::to_vec(v).map_err(|e| fail(e.to_string()))?;
    serde_json::from_slice(&json).map_err(|e| fail(e.to_string()))
}

// ---- the bag as the bind reads it

/// `o[k]` of a JSON object (`undefined`: not an object, or no such key).
pub(crate) fn get<'v>(o: &'v Value, k: &str) -> Option<&'v Value> {
    o.as_obj()?.get(k)
}

/// `v ?? d` for a number (`null` and `undefined` give the default; another type too).
pub(crate) fn num_or(v: Option<&Value>, d: f64) -> f64 {
    match v {
        Some(Value::Num(n)) => *n,
        _ => d,
    }
}

/// JavaScript's truthiness.
pub(crate) fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Value::Str(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

pub(crate) fn vec_of(v: Option<&Value>) -> Vec3 {
    let v = v.unwrap_or(&Value::Null);
    [num_or(get(v, "X"), 0.0), num_or(get(v, "Y"), 0.0), num_or(get(v, "Z"), 0.0)]
}

/// The strings of an array value (others left out).
pub(crate) fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_arr)
        .map_or_else(Vec::new, |a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
}

/// An object path's export name (`path.slice(path.lastIndexOf('.') + 1)`).
pub(crate) fn export_name(path: &str) -> &str {
    path.rfind('.').map_or(path, |i| &path[i + 1..])
}

/// `path.split('.')[0]`.
pub(crate) fn pkg_of(path: &str) -> &str {
    path.split('.').next().unwrap_or(path)
}
