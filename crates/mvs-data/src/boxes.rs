//! A character's hit box tables: the boxes kind's index over the
//! frames, a Float32Array.

use serde::{Deserialize, Serialize};

use crate::fighter::Map;
use crate::value::nullable;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoxIndex {
    pub mesh_scale: f64,
    pub mesh_z: f64,
    pub stride: u32,
    #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
    pub com: Option<Option<BoxCom>>,
    pub shapes: Vec<BoxShape>,
    pub anims: Map<BoxAnim>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skel: Option<SkelIndex>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket_axes: Option<SocketAxes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actors: Option<Map<ActorBoxIndex>>,
    /// Taken off as the data loads (laid onto the fighter data's actor components as `boneBox`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bone_boxes: Option<Map<Map<String>>>,
}

/// The tracks of a spawned actor's own skeletal mesh (offsets from the start of its character's section).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorBoxIndex {
    pub mesh: String,
    pub mesh_scale: f64,
    pub mesh_z: f64,
    pub stride: u32,
    #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
    pub com: Option<Option<BoxCom>>,
    pub shapes: Vec<BoxShape>,
    pub anims: Map<BoxAnim>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skel: Option<SkelIndex>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket_axes: Option<SocketAxes>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BoxCom {
    pub bone: String,
    pub offset: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BoxKind {
    Hit,
    Hurt,
    Socket,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoxShape {
    pub name: String,
    pub kind: BoxKind,
    pub bone: Option<String>,
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    #[serde(rename = "is2D")]
    pub is_2d: bool,
    pub capsule: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abs_scale: Option<bool>,
    pub radius: f64,
    pub half_height: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vbone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_capsule: Option<bool>,
    #[serde(rename = "box", default, skip_serializing_if = "Option::is_none")]
    pub is_box: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extent: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abs_rot: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlaps: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unscaled: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rot: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub com: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axes_const: Option<Vec<f64>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoxAnim {
    pub num_frames: u32,
    pub length: f64,
    pub rate_scale: f64,
    pub root_motion: bool,
    pub offset: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkelIndex {
    pub offset: u32,
    pub bones: Vec<String>,
    pub parents: Vec<i32>,
    pub rest: Vec<f64>,
    pub shape_bone: Vec<i32>,
    pub shape_loc: Vec<f64>,
    pub shape_rot: Vec<f64>,
    pub com: i32,
    pub masks: Map<Vec<i32>>,
    pub clips: Map<SkelClip>,
    pub blend: Map<SkelBlend>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkelClip {
    pub o: u32,
    pub v: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkelBlend {
    pub axes: Vec<BlendAxis>,
    pub samples: Vec<BlendSample>,
    pub grid: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlendAxis {
    pub min: f64,
    pub max: f64,
    pub grid: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlendSample {
    pub clip: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SocketAxes {
    pub offset: u32,
    pub stride: u32,
    pub names: Vec<String>,
}
