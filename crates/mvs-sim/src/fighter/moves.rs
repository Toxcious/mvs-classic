//! How the fighter moves: move commands, impulses and friction, jumps, crouch, fast fall, wall slide, air control
//! and the rest of the movement.

use crate::Real;
use crate::State;
use crate::combat::BuffDecay;
use crate::combat::BuffRef;
use crate::fighter::FighterId;
use crate::fighter::anim::MontageSlot;
use crate::input::InputState;
use crate::names::Name;
use crate::world::World;
use mvs_data::value::Props;

mod attach_parent;
pub use attach_parent::*;

mod command;
pub use command::*;

mod wall;
pub use wall::*;

mod jump;
pub use jump::*;

mod folds;
pub use folds::*;

mod modes;
pub use modes::*;

mod physics;
pub use physics::*;

/// `V2` (UE's `X`, `Y`).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct V2 {
    pub x: Real,
    pub y: Real,
}

/// `MoveEnd`.
#[derive(State, Debug, Default)]
pub struct MoveEnd {
    pub stop: bool,
    pub clamp: bool,
    pub min: V2,
    pub max: V2,
}

/// `MoveCtl`.
#[derive(State, Debug, Default)]
pub struct MoveCtl {
    pub speed: Real,
    pub max: Real,
    pub min: Real,
    pub friction: Real,
    pub decay: Real,
}

/// `MoveCmd`.
#[derive(State, Debug, Default)]
pub struct MoveCmd {
    pub end: MoveEnd,
    /// The notify index; -1 for RetainVelocity.
    pub idx: i32,
    pub total: Real,
    /// The float sum of its tick (TickMoveTime's).
    pub cur: crate::Real,
    /// MoveVector: MoveDelta / TotalMoveTime at the launch (world, facing applied).
    pub vx: Real,
    pub vy: Real,
    /// MoveDelta / TotalMoveTime.
    pub bx: Real,
    pub by: Real,
    pub wall_skate: bool,
    /// CorrnerCheck.
    pub corner: bool,
    pub up_gate: bool,
    pub side_gate: bool,
    pub old_foot: bool,
    pub ctl: MoveCtl,
}

/// `Impulse`: a NetImpulse or a LaunchSelf.
#[derive(State, Debug, Default)]
pub struct Impulse {
    pub vx: Real,
    pub vy: Real,
    pub xy: bool,
    pub kind: i32,
    pub z: bool,
    pub d: Option<Real>,
}

/// `groundJump`.
#[derive(State, Debug, Default)]
pub struct GroundJump {
    pub ff: bool,
    pub v: Real,
}

/// `forceMove`: a forced move to a point.
#[derive(State, Debug, Default)]
pub struct ForceMove {
    pub id: i32,
    pub sx: Real,
    pub sy: Real,
    pub tx: Real,
    pub ty: Real,
    pub dur: Real,
    pub time: Real,
    pub done: bool,
    /// The finish velocity mode (0 none, 1 set, 2 clamp) and its values.
    pub mode: i32,
    pub clamp: Real,
    pub set_vx: Real,
    pub set_vy: Real,
    /// The EaseInOutFloatCurve time curve.
    pub ease: bool,
}

/// `forceFinish`.
#[derive(State, Debug, Default)]
pub struct ForceFinish {
    pub mode: i32,
    pub clamp: Real,
    pub set_vx: Real,
    pub set_vy: Real,
}

/// `pull`: a pull toward a point.
#[derive(State, Debug, Default)]
pub struct Pull {
    pub sx: Real,
    pub sy: Real,
    pub dx: Real,
    pub dy: Real,
    pub k: Real,
    pub n: Real,
    pub rate: Real,
    pub end: crate::names::Name,
    /// The caller's `by`, handed to its end (`PULL_ENDS`): the tether's pull keeps a fighter id there.
    pub by: i32,
}

/// Move commands, impulses, friction and gravity.
#[derive(State, Debug, Default)]
pub struct MoveCommands {
    pub move_cmds: Vec<MoveCmd>,
    pub net: Option<Impulse>,
    pub net_late: Option<Impulse>,
    pub impulse: Option<Impulse>,
    pub net_friction: Option<bool>,
    pub net_gravity: Option<Real>,
    pub gravity_override: bool,
    pub gravity_mod_count: i32,
    pub gravity_scale: Real,
    pub ground_jump: Option<GroundJump>,
    pub move_end: Option<MoveEnd>,
    pub friction_zero: bool,
    pub friction: Option<FrictionState>,
    pub input_ov: Option<InputOverrideState>,
    pub buff_decays: Option<Vec<BuffDecay>>,
    pub decay_t0: Option<Real>,
    pub fly: Option<FlyState>,
    pub max_accel: Option<Real>,
    pub speed_override: Option<Real>,
    pub folds: Option<Folds>,
    pub prev_atk_cancelled: bool,
    pub cancel_evade_buffs: bool,
    pub wall_skate: bool,
    pub flying: bool,
    pub fly_input: Option<[Real; 2]>,
    pub fluid_friction: Real,
    pub cmd_jumped: bool,
    pub jump_launched: bool,
    pub last_jump_v: Real,
    pub hit_launch: bool,
    pub move_none: bool,
    /// BPI-written: DisableDash.
    pub disable_dash: Option<bool>,
    pub force_move: Option<ForceMove>,
    pub force_finish: Option<ForceFinish>,
    pub force_move_seq: i32,
    pub pull: Option<Pull>,
}

/// `jumpHoldTimer`.
#[derive(State, Debug, Default)]
pub struct HoldTimer {
    pub left: Real,
    pub ticks: i32,
}

/// Jumps.
#[derive(State, Debug, Default)]
pub struct Jump {
    pub jumps: i32,
    pub jump_frame: i32,
    pub jump_stop_gate: bool,
    pub end_jump_latent: bool,
    pub jump_released: bool,
    pub jump_min_time: Real,
    pub jump_start_time: Real,
    pub jump_cut_at: i32,
    pub quick_fall_at: i32,
    pub crouch_jump: bool,
    pub last_jump_time: Real,
    pub ground_jump_tick: i32,
    pub jump_v: Real,
    pub jump_hold_timer: Option<HoldTimer>,
    pub block_jump_end_launch: bool,
}

/// Crouch.
#[derive(State, Debug, Default)]
pub struct Crouch {
    pub down_stick: bool,
    pub down_btn: bool,
    pub pushing_down: bool,
    pub crouching: bool,
    pub crouch_fx: bool,
    pub stop_ahead: bool,
    pub crouch_owed: bool,
    pub block_land_reset: bool,
    pub crouch_locked: bool,
}

/// Fast fall.
#[derive(State, Debug, Default)]
pub struct FastFall {
    pub ff_triggered: bool,
    pub ff_check_tick: i32,
    pub ff_mult: bool,
    pub fast_falling: bool,
    pub anim_fast_fall_off: Option<bool>,
    pub fast_fall_start_y: Option<Real>,
    /// BPI-written: BlockFastFallTime.
    pub block_ff_time: Option<Real>,
    pub was_attacking_during_fastfall: Option<bool>,
}

/// Wall slide.
#[derive(State, Debug, Default)]
pub struct Wall {
    pub wall_sliding: bool,
    pub wall_touches: i32,
    pub wall_capped: bool,
    pub wall_mult: Real,
    pub wall_stall: i32,
    pub wall_fatigue: i32,
    pub wall_fatigue_at: crate::Clock,
    pub wall_timer_pending: bool,
    pub no_wall_slide_until: i32,
    /// BPI-written: AllowWallPlay.
    pub allow_wall_play: Option<bool>,
}

/// Air control, dash and the air counts.
#[derive(State, Debug, Default)]
pub struct AirControl {
    pub fall_lat_friction: Real,
    pub air_control_gate: bool,
    pub dash_fall_gate: bool,
    /// The float sum of its tick.
    pub time_in_air: crate::Real,
    pub was_attacking: bool,
    pub air_control: Real,
    pub air_boost_mult: Real,
    pub air_attack_available: bool,
    pub air_attack_delay: Real,
    pub air_evades: i32,
    pub air_specials: i32,
    /// The mesh's Diffuse_Mult is at HandleAirMoveDesaturation's 0.7 (`match_side::handle_air_move_desaturation`).
    /// Not in the record (Rust-only: a load keeps it).
    pub desaturated: bool,
    pub air_special_max_set: Option<Real>,
    pub dash_tick: Real,
    pub dash_game_time: Real,
    pub last_direction: Real,
    pub move_in: Real,
    pub last_move_in: Real,
}

/// `FlyProps`: a fly move's properties (the `PROPS` cache by its data: made at bind).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct FlyProps {
    /// Flying Max Acceleration.
    pub max_accel: Real,
    /// Flying Brake Deceleration.
    pub brake: Real,
    /// Flying Air Control.
    pub air_control: Real,
    /// Horizontal Flying Max Speed.
    pub horizontal: Real,
    /// Upward Flying Max Speed.
    pub upward: Real,
    /// Downward Flying Max Speed.
    pub downward: Real,
    /// Can Hover In Place.
    pub hover_in_place: bool,
}

// ---- the call surface's argument and return types

/// What `cornerTrace` takes or gives (a plain record).
#[derive(Clone, Debug)]
pub struct CornerTraceOut {
    pub x: Real,
    pub ny: Real,
}
