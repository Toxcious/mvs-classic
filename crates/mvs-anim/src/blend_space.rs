//! A BlendSpacePlayer node bound from the build: the shape the
//! importer writes into the fighter kind's `blendSpaces` and the view's player reads.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlendAxis {
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub grid: f64,
}

/// A sample: its clip (the sequence's name, and its object path), its place and rate; `ref_pose`: an additive
/// sample's base clip (AAT_LocalSpaceBase over ABPT_AnimFrame, frame 0).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlendSample {
    pub clip: String,
    pub path: String,
    pub x: f64,
    pub y: f64,
    pub rate: f64,
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub ref_pose: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlendSpaceData {
    pub name: String,
    /// 1: BlendSpace1D, 2: BlendSpace.
    pub dims: usize,
    pub axes: Vec<BlendAxis>,
    pub samples: Vec<BlendSample>,
    /// A grid point's three sample indices (-1: none) and three weights, as cooked.
    pub grid: Vec<f64>,
    pub additive: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlendSpacePlayerData {
    pub node: String,
    #[serde(rename = "loop")]
    pub looped: bool,
    pub play_rate: f64,
    pub start_position: f64,
    pub reset_on_change: bool,
    pub space: BlendSpaceData,
}

/// The key of a node in the fighter kind's `blendSpaces`: `<AnimBP class>:<node>`.
pub fn player_key(anim_bp: &str, node: &str) -> String {
    format!("{anim_bp}:{node}")
}
