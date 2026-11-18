//! A character's fighter data (`FighterData`)
//! and the montage records the items part and the stages share with it. Members in the order of the entry's keys; a
//! class default object or a notify's properties is a [`Props`] bag.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::value::{Props, Value, nullable};

pub type Map<T> = IndexMap<String, T>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FighterData {
    pub pawn: Props,
    pub base: Props,
    pub components: Components,
    pub anim_set: Props,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim_sets: Option<Map<Props>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim_set_blend: Option<AnimSetBlendNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polymorph_anim_sets: Option<Map<PolymorphAnimSet>>,
    pub anim_assets: Map<AnimAsset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fall_space: Option<FallSpace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_spaces: Option<WallSpaces>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq_notifies: Option<Map<Vec<Notify>>>,
    pub branch_groups: Map<BranchGroup>,
    pub montages: Map<Montage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffs: Option<Map<Props>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actors: Option<Map<ActorDef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_montages: Option<Map<Montage>>,
    pub slot_holds: SlotHolds,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim_nodes: Option<Map<AnimNodeData>>,
    pub skeleton: Skeleton,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vo: Option<Vec<VoRow>>,
    pub vo_tags: VoTags,
    pub skin: DefaultSkin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skins: Option<Map<SkinLook>>,
    pub taunts: Vec<Taunt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taunt_table: Option<Map<TauntEntry>>,
    #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
    pub dance_montage: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_composites: Option<Map<MeshComposite>>,
    pub paths: Paths,
}

/// A VO table row: Alias, Tags, Priority, AkAudioEvent, its length in seconds.
pub type VoRow = (String, String, f64, Option<String>, Option<f64>);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Components {
    pub base: Map<Component>,
    pub pawn: Map<Component>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    #[serde(rename = "type")]
    pub ty: String,
    pub p: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnimSetBlendNode {
    pub node: String,
    pub on_true: String,
    pub on_false: String,
    pub blend_time: Map<f64>,
    pub blend_type: String,
    pub transition_type: String,
    pub reset_child_on_activation: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolymorphAnimSet {
    pub set: Props,
    pub head_scale: [f64; 3],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnimAsset {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub samples: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segments: Option<Vec<CompositeSegment>>,
}

/// An AnimComposite's segment.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositeSegment {
    pub anim: String,
    pub start: f64,
    pub len: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FallSpace {
    pub name: String,
    pub node: String,
    pub play_rate: f64,
    pub min: f64,
    pub max: f64,
    pub grid: f64,
    pub samples: Vec<SpaceSample>,
    pub cells: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WallSpace {
    pub name: String,
    pub node: String,
    pub play_rate: f64,
    #[serde(rename = "loop")]
    pub looping: bool,
    pub min: f64,
    pub max: f64,
    pub grid: f64,
    pub samples: Vec<SpaceSample>,
    pub cells: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WallSpaces {
    pub enter: WallSpace,
    pub idle: WallSpace,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceSample {
    pub anim: String,
    pub x: f64,
    pub rate: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub name: String,
    pub next: Option<String>,
    pub t: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Segment {
    pub anim: String,
    pub anim_type: String,
    pub start: f64,
    pub a0: f64,
    pub a1: f64,
    pub rate: f64,
    pub loops: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Slot {
    pub slot: String,
    pub segments: Vec<Segment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty_segments: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additive: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MeshComposite {
    pub length: f64,
    pub rate_scale: f64,
    pub segments: Vec<Segment>,
}

/// A notify's MontageTickType: `Queued` notifies dispatch after the montage's step, `BranchingPoint` ones inside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum NotifyTick {
    Queued,
    BranchingPoint,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Notify {
    pub cls: String,
    pub inst: Option<String>,
    pub t: f64,
    pub dur: f64,
    pub state: bool,
    pub tick: NotifyTick,
    pub track: i32,
    pub p: Props,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MachineFlags {
    pub reinit: bool,
    pub skip_first: bool,
    pub entry: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Montage {
    pub name: String,
    pub length: f64,
    pub blend_in: f64,
    pub blend_out: f64,
    pub auto_blend_out: bool,
    pub blend_out_trigger_time: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend_out_option: Option<String>,
    pub rate_scale: f64,
    pub sections: Vec<Section>,
    pub slots: Vec<Slot>,
    pub notifies: Vec<Notify>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holds_source: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keeps_nav: Option<bool>,
    /// The notifies of a slot track the AnimBP plays for one skin alone (C019_AnimBP's EvilMorty: skin C019_S02), taken
    /// off `notifies` by the bind: the sim runs the default skin,
    /// that skin's view reads them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skin_slot_notifies: Vec<Notify>,
}

/// An attack table entry (an evade's: `AttackEntry`).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct AttackEntry {
    pub required_button_input: i32,
    pub required_direction_input: i32,
    pub attack_montage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ability_slot: Option<i32>,
    pub display_name: Option<String>,
    /// (a condition the table leaves empty is null)
    pub conditions: Vec<Option<String>>,
    pub allow_pre_attack_rotation: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct Branch {
    pub attack: String,
    pub button: String,
    pub direction: String,
    /// (a condition the table leaves empty is null)
    pub conditions: Vec<Option<String>>,
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ability_slot: Option<i32>,
    pub allow_pre_attack_rotation: bool,
    pub display_name: Option<String>,
    pub unique_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct BranchGroup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_groups: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<Vec<Branch>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActorDef {
    pub p: Props,
    /// Absent in a perk table's actor with no components.
    #[serde(default, skip_serializing_if = "Props::is_empty")]
    pub components: Props,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supers: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ifaces: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SlotHolds {
    pub class: String,
    pub nodes: Vec<SlotHoldNode>,
    pub stages: Vec<Vec<String>>,
    pub machines: Map<MachineFlags>,
    pub graph: SlotGraph,
    pub layer: Map<MachineFlags>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SlotHoldNode {
    pub node: String,
    pub slot: String,
    pub always: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SlotGraph {
    pub root: i32,
    pub nodes: Vec<SlotGraphNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached: Option<Vec<i32>>,
}

/// A node of an AnimBP's slot graph, by its `k`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "k", rename_all = "lowercase", deny_unknown_fields)]
pub enum SlotGraphNode {
    Slot {
        node: String,
        slot: String,
        src: i32,
    },
    Two {
        node: String,
        #[serde(rename = "in")]
        input: Option<String>,
        #[serde(rename = "type")]
        ty: String,
        alpha: f64,
        a: i32,
        b: i32,
        #[serde(rename = "scaleBias", default, skip_serializing_if = "Option::is_none")]
        scale_bias: Option<Value>,
    },
    Bool {
        node: String,
        #[serde(rename = "in")]
        input: Option<String>,
        value: bool,
        blend: Vec<f64>,
        poses: Vec<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        opts: Option<Props>,
    },
    Src {
        node: String,
        kind: String,
    },
    Seq {
        node: String,
        anim: Option<String>,
        rate: f64,
        start: f64,
        #[serde(rename = "loop")]
        looping: bool,
        player: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ignore: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        opts: Option<Props>,
    },
    Layered {
        node: String,
        base: i32,
        blends: Vec<i32>,
        #[serde(rename = "in")]
        input: Option<String>,
        weights: Vec<f64>,
        filters: Vec<Vec<BranchFilter>>,
        #[serde(rename = "meshRot")]
        mesh_rot: bool,
        #[serde(rename = "meshScale")]
        mesh_scale: bool,
    },
    Machine(SlotMachineNode),
    Other {
        node: String,
        kind: String,
        poses: Vec<i32>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BranchFilter {
    pub bone: String,
    pub depth: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlotMachineNode {
    pub node: String,
    pub index: i32,
    pub machine: String,
    pub entry: i32,
    pub max: i32,
    pub reinit: bool,
    pub skip_first: bool,
    pub states: Vec<SlotMachineState>,
    pub trans: Vec<SlotMachineTrans>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlotMachineState {
    pub name: String,
    pub players: Vec<i32>,
    pub always_reset: bool,
    pub out: Vec<i32>,
    /// The state's pose; `None`: a conduit.
    pub pose: Option<i32>,
    #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
    pub entry_in: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_value: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SlotMachineTrans {
    pub from: i32,
    pub to: i32,
    pub xfade: f64,
    pub blend: String,
    #[serde(rename = "in")]
    pub input: Option<String>,
    pub value: bool,
    pub want: bool,
    pub auto: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opts: Option<Props>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnimNodeData {
    #[serde(rename = "type")]
    pub ty: String,
    pub props: Props,
    #[serde(rename = "in")]
    pub input: Map<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Skeleton {
    pub sockets: Map<Socket>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_bones: Option<Map<VirtualBone>>,
    pub mirror_axis: Option<String>,
    pub mirror: Map<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip: Option<Map<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror_node: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Socket {
    pub bone: String,
    pub loc: [f64; 3],
    pub rot: [f64; 3],
    pub scale: [f64; 3],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VirtualBone {
    pub source: String,
    pub target: String,
    /// The AnimBP's ModifyBone nodes on the bone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modify: Option<Vec<BoneModify>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraint: Option<BoneConstraint>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoneModify {
    pub node: String,
    pub alpha: f64,
    pub translation: Vec<f64>,
    pub translation_mode: String,
    pub translation_space: String,
    pub rotation: Vec<f64>,
    pub rotation_mode: String,
    pub rotation_space: String,
    pub scale: Vec<f64>,
    pub scale_mode: String,
    pub scale_space: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BoneConstraint {
    pub node: String,
    pub target: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoTags {
    pub enemy: String,
    pub ally: String,
}

/// The default skin's meshes (FighterData.skin).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefaultSkin {
    pub name: String,
    pub mesh: String,
    pub sub_meshes: Vec<SubMesh>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intro_montage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prop_meshes: Option<Vec<PropMesh>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubMesh {
    pub key: String,
    pub mesh: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PropMesh {
    pub key: String,
    pub mesh: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub tags: Vec<String>,
    pub skeleton: String,
    pub socket: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkinLook {
    pub name: String,
    pub slug: String,
    pub mesh: String,
    pub sub_meshes: Vec<SubMesh>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prop_meshes: Option<Vec<PropMesh>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intro_montage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub victory_montage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defeat_montage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub montages: Option<Map<Montage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_idle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homescreen_idle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Map<SkinScale>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_scale: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lighting: Option<String>,
    pub audio: SkinLookAudio,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkinScale {
    pub loc: [f64; 3],
    pub rot: [f64; 4],
    pub scale: [f64; 3],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkinLookAudio {
    pub audio_name: String,
    pub vo_table: String,
    pub override_events: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Taunt {
    pub montage: String,
    pub conds: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TauntEntry {
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub montage: Option<String>,
    pub conds: Vec<String>,
    pub priority: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticker: Option<Sticker>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Sticker {
    pub ps: String,
    pub scale: [f64; 3],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Paths {
    pub meshes: Map<String>,
    pub clips: Map<String>,
}
