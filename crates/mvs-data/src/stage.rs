//! A stage's data (`StageData`), its manifest
//! entry and the items part (`ItemsData`).

use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};

use crate::fighter::{ActorDef, AnimAsset, Map, Montage, PolymorphAnimSet};
use crate::value::Props;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageData {
    pub name: String,
    /// The level's unit scale (the bind's world units).
    pub units: String,
    pub meshes: Vec<StageMesh>,
    pub materials: Map<StageMaterial>,
    pub spawns: Vec<StageStart>,
    pub respawns: Vec<StageStart>,
    pub waypoints: Vec<Waypoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_zones: Option<Vec<BotZone>>,
    pub lights: Vec<StageLight>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radial_lights: Option<Vec<RadialLight>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tlv_shadow: Option<TlvShadow>,
    pub floor: Floor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platforms: Option<Vec<Platform>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_box: Option<EdgeBox>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<Edge>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<StageCenter>,
    /// The map's post process materials.
    pub post_process: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bloom_intensity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<StageAudio>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fog: Option<Fog>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atmosphere: Option<Atmosphere>,
    pub sky_light: SkyLight,
    pub blast_boxes: Map<BlastBox>,
    pub camera: Map<NumOrBool>,
    pub camera_center: Vec<f64>,
    pub blast_box_corners: BlastBoxCorners,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blast_shrink: Option<BlastShrink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_shrunk_angle: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solids: Option<Vec<StageSolidData>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_bodies: Option<Vec<StageQueryBody>>,
    /// The complex collision of the static meshes that have no simple shapes, cut at the play plane: met only by the
    /// traces with bTraceComplex (the native importer's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complex_segments: Option<Vec<ComplexSegment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placed: Option<Vec<StagePlacedData>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_montages: Option<Map<Montage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffs: Option<Map<Props>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actors: Option<Map<ActorDef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tagged: Option<Vec<StageTagged>>,
}

/// A camera default: a number or UseEasedMovement's boolean.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum NumOrBool {
    Num(f64),
    Bool(bool),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StageMesh {
    pub name: String,
    pub shape: String,
    pub loc: Vec<f64>,
    pub rot: Vec<f64>,
    pub scale: Vec<f64>,
    pub material: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StageMaterial {
    pub parent: String,
    pub scalars: Map<f64>,
    pub vectors: Map<Vec<f64>>,
    pub textures: Map<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StageStart {
    pub tag: Option<String>,
    pub tags: Vec<String>,
    pub loc: Vec<f64>,
    pub yaw: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Waypoint {
    pub name: String,
    pub loc: Vec<f64>,
    pub position: f64,
    pub edge: bool,
    pub recovery: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BotZone {
    pub name: String,
    pub center: Vec<f64>,
    pub half_extent: Vec<f64>,
    pub rot: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageLight {
    pub name: String,
    pub rot: Vec<f64>,
    pub intensity: f64,
    pub color: Vec<f64>,
    pub affects_world: bool,
    pub source_angle: f64,
    pub soft_source_angle: f64,
    pub specular_scale: f64,
    pub mobility: String,
    pub channels: Vec<bool>,
    pub translucent: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RadialLight {
    pub name: String,
    pub pos: Vec<f64>,
    pub dir: Vec<f64>,
    pub color: Vec<f64>,
    pub src: Vec<f64>,
    pub att: Vec<f64>,
    pub falloff_exponent: f64,
    pub shadow_channel: f64,
    pub channels: Vec<bool>,
    pub translucent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_length: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tangent: Option<Vec<f64>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TlvShadow {
    pub size: [f64; 2],
    pub lights: Map<TlvLight>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TlvLight {
    pub at: [f64; 2],
    pub size: [f64; 2],
    pub crop: [f64; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_to_light: Option<Vec<f64>>,
    pub built_data: String,
    pub guid: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Floor {
    pub center: Vec<f64>,
    pub half_extent: Vec<f64>,
    pub top: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// The floor component's ComponentTags.
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Platform {
    pub name: String,
    pub center: Vec<f64>,
    pub half_extent: Vec<f64>,
    pub top: f64,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moving: Option<PlatformMoving>,
    /// The port's own rising platform (a layout's; the build has none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rise: Option<PlatformRise>,
    /// The platform's mesh as the render draws it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<PlatformMesh>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformMoving {
    pub start: Vec<f64>,
    pub curves: Vec<PlatformCurve>,
}

/// A layout's rising platform: its box's centre climbs at `speed` (units / s) from where it is placed and, past
/// `top`, goes back down by `top - bottom` (UE Z of the box's centre).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformRise {
    pub bottom: f64,
    pub top: f64,
    pub speed: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformCurve {
    pub d: Vec<f64>,
    pub duration: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlatformMesh {
    pub mesh: String,
    pub materials: Vec<String>,
    pub loc: Vec<f64>,
    pub rot: Vec<f64>,
    pub scale: Vec<f64>,
    pub cast_shadow: bool,
    pub channels: Vec<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EdgeBox {
    pub center: Vec<f64>,
    pub half_extent: Vec<f64>,
    pub top: f64,
    pub profile: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub owner: String,
    pub cls: String,
    pub loc: Vec<f64>,
    pub every: bool,
    pub tag: String,
    pub points: Vec<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageCenter {
    pub loc: Vec<f64>,
    pub stage_radius: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StageAudio {
    pub emitters: Vec<StageAudioEmitter>,
    pub reverb: Vec<ReverbVolume>,
    pub music: StageMusic,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageAudioEmitter {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    pub at: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attenuation_scale: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReverbVolume {
    pub name: String,
    pub bus: String,
    pub loc: Vec<f64>,
    pub scale: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StageMusic {
    pub map: String,
    pub skins: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fog {
    pub density: f64,
    pub height_falloff: f64,
    pub inscatter: Vec<f64>,
    pub start_distance: f64,
    pub height: f64,
    pub max_opacity: f64,
    pub cutoff_distance: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second: Option<SecondFog>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecondFog {
    pub density: f64,
    pub height_falloff: f64,
    pub height_offset: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Atmosphere {
    pub sun_power: f64,
    pub power: f64,
    pub density_scale: f64,
    pub density_offset: f64,
    pub ground_offset: f64,
    pub distance_scale: f64,
    pub altitude_scale: f64,
    pub height_scale_rayleigh: f64,
    pub start_distance: f64,
    pub distance_offset: f64,
    pub light_direction: Vec<f64>,
    pub light_color: Vec<f64>,
    pub altitude_samples: f64,
    pub render_mask: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkyLight {
    pub cubemap: String,
    pub intensity: f64,
    pub color: Vec<f64>,
    pub lower_hemisphere: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlastBox {
    pub center: Vec<f64>,
    pub half_extent: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlastBoxCorners {
    pub top_left: Vec<f64>,
    pub bottom_right: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlastShrink {
    pub center: Vec<f64>,
    pub half_extent: Vec<f64>,
    pub jump_out_buffer_top: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SolidKind {
    Box,
    Obox,
    Convex,
    Capsule,
    Sphere,
}

/// A solid collider of the stage (data/stage/collision). Its members' order depends on its kind (the bind writes
/// an obox's `rot` and a convex's `points` before `flags`, a capsule's `rot` after `objectType`): the serializer
/// writes that order back.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageSolidData {
    pub id: i32,
    pub name: String,
    pub kind: SolidKind,
    pub center: Vec<f64>,
    pub half_extent: Vec<f64>,
    #[serde(default)]
    pub rot: Option<Vec<f64>>,
    #[serde(default)]
    pub axis: Option<Vec<f64>>,
    #[serde(default)]
    pub radius: Option<f64>,
    #[serde(default)]
    pub half_height: Option<f64>,
    #[serde(default)]
    pub points: Option<Vec<f64>>,
    pub flags: i32,
    pub profile: String,
    #[serde(default)]
    pub object_type: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

impl Serialize for StageSolidData {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        fn opt<S: SerializeStruct, T: Serialize>(st: &mut S, k: &'static str, v: &Option<T>) -> Result<(), S::Error> {
            match v {
                Some(v) => st.serialize_field(k, v),
                None => st.skip_field(k),
            }
        }
        let capsule = self.kind == SolidKind::Capsule;
        let mut st = s.serialize_struct("StageSolidData", 15)?;
        st.serialize_field("id", &self.id)?;
        st.serialize_field("name", &self.name)?;
        st.serialize_field("kind", &self.kind)?;
        st.serialize_field("center", &self.center)?;
        st.serialize_field("halfExtent", &self.half_extent)?;
        if !capsule {
            opt(&mut st, "rot", &self.rot)?;
        }
        opt(&mut st, "points", &self.points)?;
        st.serialize_field("flags", &self.flags)?;
        st.serialize_field("profile", &self.profile)?;
        opt(&mut st, "objectType", &self.object_type)?;
        if capsule {
            opt(&mut st, "rot", &self.rot)?;
        }
        opt(&mut st, "axis", &self.axis)?;
        opt(&mut st, "radius", &self.radius)?;
        opt(&mut st, "halfHeight", &self.half_height)?;
        opt(&mut st, "tags", &self.tags)?;
        opt(&mut st, "enabled", &self.enabled)?;
        st.end()
    }
}

/// One segment of a static mesh component's collision triangles where they cross the play plane (UE X = 0), in the
/// plane's (UE Y, UE Z), with the component's blocked channels (StageSolidData.flags).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComplexSegment {
    /// The actor and component (`<actor>.<component>`).
    pub name: String,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub flags: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageQueryBody {
    pub name: String,
    pub center: Vec<f64>,
    pub half_extent: Vec<f64>,
    pub profile: String,
    pub object_type: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StagePlacedData {
    pub name: String,
    pub cls: String,
    pub loc: Vec<f64>,
    pub rot: Vec<f64>,
    pub scale: Vec<f64>,
    pub p: Props,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solids: Option<Vec<i32>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StageTagged {
    pub name: String,
    pub tags: Vec<String>,
    pub loc: Vec<f64>,
    pub rot: Vec<f64>,
}

/// A stage's manifest entry.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageManifest {
    pub map_data: Vec<String>,
    /// [level under /Game/Panda_Main/Maps, role] in LabLevelsToLoad order.
    pub levels: Vec<(String, String)>,
    pub sky: String,
    pub vlm: Vec<String>,
    pub spawn_areas: Vec<SpawnArea>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpawnArea {
    pub level: String,
    pub actor: String,
    pub origin: Vec<f64>,
    pub extent: Vec<f64>,
    pub root: Vec<f64>,
    pub big_object_x: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemsData {
    pub ffa_items: Vec<String>,
    pub center_actor: String,
    pub buffs: Map<Props>,
    pub actors: Map<ActorDef>,
    pub montages: Map<Montage>,
    pub actor_montages: Map<Montage>,
    pub polymorph_anim_sets: Map<PolymorphAnimSet>,
    pub anim_assets: Map<AnimAsset>,
}
