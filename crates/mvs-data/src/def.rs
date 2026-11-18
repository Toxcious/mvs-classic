//! A character's definition (made by `charDef` from the
//! fighter data) and what `charDef` keeps beside it.

use serde::{Deserialize, Serialize};

use crate::fighter::{AttackEntry, CompositeSegment, FallSpace, Map, Notify, Taunt, WallSpaces};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CharDef {
    pub anim: AnimSet,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim_sets: Option<Map<AnimSet>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim_set_blend: Option<AnimSetBlend>,
    pub max_walk: f64,
    pub max_walk_crouched: f64,
    pub crouched_half: f64,
    pub max_custom_speed: f64,
    pub max_fly_speed: f64,
    pub jump_velocity: f64,
    pub max_jump_height: f64,
    pub max_double_jump_height: f64,
    pub max_wall_jump_height: f64,
    pub fast_fall_speed: f64,
    pub key: String,
    pub max_jumps: f64,
    pub max_hover_speed: Option<f64>,
    pub hover_speed_mult: f64,
    pub cap_half: f64,
    pub rest_gap: f64,
    pub cap_radius: f64,
    pub inner_cap_radius: f64,
    pub inner_cap_half: f64,
    pub mesh_scale: f64,
    pub mesh_x: f64,
    pub sfx: Sfx,
    pub vo_tag_enemy: String,
    pub vo_tag_ally: String,
    pub pickup_ground: String,
    pub pickup_air: String,
    pub nav: NavSet,
    pub nav_sets: Map<NavSet>,
    pub throw_speed: f64,
    pub taunts: Vec<Taunt>,
    pub rock: bool,
    #[serde(rename = "use2D")]
    pub use_2d: bool,
    pub crouch_evade_recharge: Option<f64>,
    pub sub_meshes: Vec<String>,
    pub sub_hidden: Vec<String>,
    pub seq_notifies: Map<Vec<Notify>>,
    pub abilities: Vec<AbilityDef>,
    pub ability_cooldown_vo: Vec<String>,
    pub ability_cooldown_sfx: Vec<String>,
}

/// A Base_AnimBP child's anim set as the machines read it. A clip the set leaves unset is absent (`undefined` in the
/// entry's data); `hrN` and `looping` keep their unset items as `None`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnimSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_land: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_enter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_loop: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crouch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crouch_land: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_stop: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_stop2: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crouch_in: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crouch_out: Option<String>,
    pub crouch_override: bool,
    pub crouch_in_from_run: bool,
    pub run_stop_blend_time: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_land: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_enter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_idle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fall: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast_fall: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump2: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hr_front: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hr_back: Option<String>,
    #[serde(rename = "hrN")]
    pub hr_n: Vec<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recover: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fly_back: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fly_forward: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fly_back_intro: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fly_forward_intro: Option<String>,
    #[serde(rename = "rotate90OnHit")]
    pub rotate_90_on_hit: bool,
    pub fly_back_orient: f64,
    pub fly_forward_orient: f64,
    pub fall_lean_amount: f64,
    pub fall_lean_speed: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_bounce_back: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_bounce_front: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_bounce_back: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_bounce_front: Option<String>,
    pub looping: Vec<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fall_space: Option<FallSpace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_spaces: Option<WallSpaces>,
    pub composites: Map<Vec<CompositeSegment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hover: Option<Hover>,
    pub run_base_speed: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Hover {
    #[serde(rename = "in")]
    pub enter: String,
    #[serde(rename = "loop")]
    pub looping: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnimSetBlend {
    pub on_true: String,
    pub on_false: String,
    pub blend_time: Map<f64>,
    pub blend_type: String,
    pub reset_child_on_activation: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Sfx {
    pub jump: String,
    pub jump_double: String,
    pub land: String,
    pub wall_attach: String,
    pub wall_exit: String,
    pub wall_jump: String,
    pub crouch_enter: String,
    pub crouch_exit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vo_jump: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vo_land: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vo_evade: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vo_death: Option<String>,
}

/// The navigation set a pawn's graph can swap (CharDef.nav, CharDef.navSets).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NavSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash_turn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evades: Option<Vec<AttackEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fwd_evade: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spot_dodge: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_jump: Option<String>,
    pub hold: Vec<HoldOverride>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HoldOverride {
    pub air: bool,
    pub dir: i32,
    pub montage: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct AbilityDef {
    pub cooldown: f64,
    pub icon: Option<String>,
    pub max_count: f64,
    pub start_count: f64,
    pub count_cooldown: f64,
    pub count_spent_per_trigger: f64,
    pub count_refreshed_per_cooldown: f64,
    pub cooldown_always_visible: bool,
    pub ally_cooldown_visible: bool,
    pub foe_cooldown_visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate_cooldowns: Option<Vec<f64>>,
    pub static_ability_cooldown: bool,
    pub static_ammo_cooldown: bool,
    pub refresh_cooldown_on_respawn: bool,
}

/// What `charDef` keeps beside the definition (module maps by the anim set and by the fighter data).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefExtra {
    pub anim: SetExtra,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim_sets: Option<Map<SetExtra>>,
    pub knock_back_shapes: Vec<String>,
    pub default_terminal: f64,
}

/// An anim set's `altIdleDelay`, `crouchRunOf`, `edgeIdleOf` and `runRateLandRun`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetExtra {
    pub alt_idle_delay: f64,
    pub crouch_run: Option<CrouchRun>,
    pub edge_idle: Option<EdgeIdle>,
    pub run_rate_land: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CrouchRun {
    pub anim: String,
    pub base_speed: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub into: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_of: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeIdle {
    pub idle: String,
    pub crouch: String,
    pub land: String,
}
