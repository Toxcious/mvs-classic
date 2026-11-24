//! The layout file: a custom stage as a base stage of the build with pieces hidden, moved and added, and the
//! play shapes (floor, platforms, solids, blast box, starts, camera, waypoints, item area) of its own. Names and
//! numbers only: places in UE units (cm, X depth, Y across, Z up), rotations in degrees (pitch, yaw, roll).

use mvs_data::fighter::Map;
use mvs_data::stage::{
    BlastBox, BlastBoxCorners, NumOrBool, PlatformMoving, StageCenter, StageSolidData, StageStart, Waypoint,
};
use serde::{Deserialize, Serialize};

use crate::LayoutError;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Layout {
    /// The map's own key: its sim name is `Custom_<key>`.
    pub key: String,
    /// The base stage's key (`m011`): its sky, lights, fog, post, music and scenery are the map's.
    pub base: String,
    /// The base scenery's blocks (its own animated parts) kept on, by their entry key; the others are stripped.
    #[serde(default)]
    pub blocks: Vec<String>,
    /// The base scenery's instances not drawn, by name.
    #[serde(default)]
    pub hide: Vec<String>,
    /// The pieces added, or moved in place.
    #[serde(default)]
    pub pieces: Vec<Piece>,
    pub floor: FloorSpec,
    /// The Expert bot's reference box, read only when present.
    // Unverified: nothing reads it yet (the Expert bot's own box of the stage is a later step).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_ground: Option<BoxSpec>,
    /// The solids, in the stage data's own shape.
    #[serde(default)]
    pub solids: Vec<StageSolidData>,
    #[serde(default)]
    pub platforms: Vec<PlatformSpec>,
    /// The blast boxes by component name; `CollisionComp` is the one the match plays by.
    pub blast: Map<BlastBox>,
    pub blast_box_corners: BlastBoxCorners,
    /// The PlayerStarts and the respawn starts (`Player<n>` / `Respawn<n>` with the team styles' tags).
    pub spawns: Vec<StageStart>,
    pub respawns: Vec<StageStart>,
    /// The camera's numbers, its centre and the stage centre: the base's where absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<Map<NumOrBool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_center: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<StageCenter>,
    #[serde(default)]
    pub waypoints: Vec<Waypoint>,
    /// The item spawn areas (MapRandomActorSpawnArea); the first is the one the match spawns in.
    #[serde(default)]
    pub items: Vec<ItemArea>,
}

/// A piece: a copy of an instance of a stage's scenery (`stage`, the base when absent), or the base's own instance
/// moved in place (the base's, with no `name` or its own name).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Piece {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    /// The source instance's name in its stage's scenery.
    pub instance: String,
    /// The new instance's name (a platform's piece is named `<platform name>.StaticMesh` whatever this says).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub loc: [f64; 3],
    #[serde(default)]
    pub rot: [f64; 3],
    #[serde(default = "one")]
    pub scale: [f64; 3],
    pub light: Light,
    /// Moved by the base's `floating` block (Sky Arena's level Blueprint), when it is kept.
    #[serde(default)]
    pub floating: bool,
    /// The platform (its name) this piece draws: it moves with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

fn one() -> [f64; 3] {
    [1.0; 3]
}

/// How a piece is lit: with its own baked lightmap (a base piece only: the lightmap rect travels with it), or as a
/// movable piece lit by the base's lights and volumetric lightmap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Light {
    Baked,
    Movable,
}

/// A box in UE units: given by hand (`center`, `halfExtent`), or tied to a piece (`piece`): the piece's mesh bounds
/// where it stands, cut to its top `thickness` when given.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoxSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub piece: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub half_extent: Option<[f64; 3]>,
}

/// The floor: a box, or `parked` (no main floor: a tiny one out of reach, which the code that reads the ground
/// finds).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FloorSpec {
    #[serde(default)]
    pub parked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub piece: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub half_extent: Option<[f64; 3]>,
}

impl FloorSpec {
    /// Its box, None when parked.
    pub fn box_spec(&self) -> Option<BoxSpec> {
        (!self.parked).then(|| BoxSpec {
            piece: self.piece.clone(),
            thickness: self.thickness,
            center: self.center,
            half_extent: self.half_extent,
        })
    }
}

/// A pass-through platform: its box and its moves (the build's BasePlatformBP list, looped).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlatformSpec {
    pub name: String,
    #[serde(rename = "box")]
    pub bx: BoxSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moving: Option<PlatformMoving>,
}

/// An item spawn area: its box (origin and half extent) and the big objects' Y offsets from its origin.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemArea {
    pub origin: [f64; 3],
    pub extent: [f64; 3],
    #[serde(default = "zero_x")]
    pub big_object_x: Vec<f64>,
}

fn zero_x() -> Vec<f64> {
    vec![0.0]
}

impl Layout {
    /// A layout from its JSON text.
    pub fn read(json: &str) -> Result<Layout, LayoutError> {
        serde_json::from_str(json).map_err(|e| LayoutError::Json(e.to_string()))
    }

    /// The sim's name of the map: `Custom_<key>`.
    pub fn stage_name(&self) -> String {
        format!("{}{}", crate::CUSTOM_PREFIX, self.key)
    }

    /// A piece's name in the composed scenery: its platform's instance name, its own name, or (moved in place) its
    /// source's.
    pub fn piece_name(&self, p: &Piece) -> String {
        match (&p.platform, &p.name) {
            (Some(pl), _) => format!("{pl}.StaticMesh"),
            (None, Some(n)) => n.clone(),
            (None, None) => p.instance.clone(),
        }
    }

    /// Whether the piece is the base's own instance moved in place (no other name, not a platform's).
    pub fn moved_in_place(&self, p: &Piece) -> bool {
        p.stage.as_deref().is_none_or(|s| s == self.base)
            && p.platform.is_none()
            && p.name.as_deref().is_none_or(|n| n == p.instance)
    }

    /// The piece's source stage key.
    pub fn piece_stage<'a>(&'a self, p: &'a Piece) -> &'a str {
        p.stage.as_deref().unwrap_or(&self.base)
    }

    /// A platform's index in the composed stage data, by its name.
    pub fn platform_index(&self, name: &str) -> Option<u32> {
        self.platforms.iter().position(|p| p.name == name).and_then(|i| u32::try_from(i).ok())
    }
}
