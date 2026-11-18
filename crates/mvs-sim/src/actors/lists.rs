//! The actors the World keeps in typed lists of their own beside `actors`: Morty's (`World.markers`, `ships`,
//! `mortars`, `aimPunches`, `accelerators`). Each struct holds every field of its class; the behaviour is its
//! character's port.
//!
//! A class's data (`p`, the actor CDO; `comps`, its components) is the bound data's, read by the class (`cls`,
//! which the variant names where the class has one); it is no state.

use super::projectile::LifetimePause;
use crate::Real;
use crate::State;
use crate::combat::{ChargeVals, ElemState, HitPacket};
use crate::fighter::{DodgeState, FighterId};
use crate::names::Name;
use crate::world::BlastBounds;
use indexmap::IndexSet;

/// A point in the play plane (`{ x, y }`).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct Pt2 {
    pub x: Real,
    pub y: Real,
}

/// A point with its tangent; `auto` is CIM_CurveAuto (`None`: CIM_CurveUser).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct SplinePt {
    pub x: Real,
    pub y: Real,
    pub tx: Real,
    pub ty: Real,
    pub auto: Option<bool>,
}

/// One entry of a spline's reparam table: a distance and its spline key.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct ReparamEntry {
    pub d: Real,
    pub key: Real,
}

/// `Marker` (C019Marker_C).
/// Its data (`p`) is the class's.
#[derive(State, Debug)]
pub struct Marker {
    pub lifetime_pause: Option<LifetimePause>,
    pub dodge: Option<DodgeState>,
    /// Always C019Marker_C.
    pub cls: Name,
    pub elem: Option<ElemState>,
    /// `id`: the world's count of spawned actors at the spawn.
    pub seq: i32,
    pub x: Real,
    pub y: Real,
    /// `vx` and `vy` are 0 (readonly).
    pub facing: Real,
    pub age: i32,
    pub dead: bool,
    pub blast_bounds: BlastBounds,
    pub blast_at: crate::Clock,
    /// 'Hit Box Damage', the return hit.
    pub hit: HitPacket,
    pub charge: Option<ChargeVals>,
    /// The Spline's points.
    pub pts: Vec<SplinePt>,
    pub current_damage: Real,
    pub cap_radius: Real,
    pub active: bool,
    pub moving: bool,
    pub focus_x: Real,
    pub focus_y: Real,
    pub move_start: Pt2,
    pub move_end: Pt2,
    pub can_add: bool,
    pub drop_x: Real,
    pub drop_y: Real,
    pub spline_at: crate::Clock,
    pub auto_tick: i32,
    pub life_ticks: i32,
    pub tl_pos: Real,
    pub tl_rate: Real,
    pub len: Real,
    pub table: Vec<ReparamEntry>,
    pub saved: Vec<Pt2>,
    pub meshes: i32,
    pub trail_end: Pt2,
    pub owner: FighterId,
    pub attack_id: i32,
}

/// A platform box of the ship and its sensor.
#[derive(State, Debug, Default)]
pub struct ShipPiece {
    pub name: Name,
    pub dx: Real,
    pub dy: Real,
    pub hw: Real,
    pub hh: Real,
    pub sensor: Option<ShipSensor>,
    /// ImpassibleObjectOverlapCount*.
    pub count: i32,
    /// Collision enabled.
    pub on: bool,
    pub trace: bool,
    pub nav: bool,
    pub item: bool,
}

/// A `ShipPiece`'s DisableCollisionCheck* box.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct ShipSensor {
    pub dx: Real,
    pub dy: Real,
    pub hw: Real,
    pub hh: Real,
}

/// `Ship` (C019_Ship_C). Its constructor's `ground`, `blast`, `fighters` and `sensed` are read
/// at the spawn only.
/// Its data (`p`) is the class's.
#[derive(State, Debug)]
pub struct Ship {
    /// Always C019_Ship_C.
    pub cls: Name,
    /// `id`.
    pub seq: i32,
    pub x: Real,
    pub y: Real,
    pub vx: Real,
    pub vy: Real,
    pub dx: Real,
    pub dy: Real,
    pub age: i32,
    pub dead: bool,
    pub pieces: Vec<ShipPiece>,
    pub mesh_dy: Real,
    pub team: i32,
    pub appear_tick: i32,
    pub death_sound_tick: i32,
    pub stop_tick: i32,
    pub destroy_tick: i32,
    /// Comp_EdgeTrace's BeginPlay place; `None` where the level has no generator.
    pub edge_at: Option<(Real, Real)>,
    pub owner: FighterId,
}

/// `MortarAim` (C019_MortarAim_C).
/// Its data (`p`) is the class's.
#[derive(State, Debug)]
pub struct MortarAim {
    pub lifetime_pause: Option<LifetimePause>,
    pub dodge: Option<DodgeState>,
    /// Always C019_MortarAim_C.
    pub cls: Name,
    pub elem: Option<ElemState>,
    /// `id`.
    pub seq: i32,
    pub x: Real,
    pub y: Real,
    /// `vx` and `vy` are 0 (readonly).
    pub facing: Real,
    pub age: i32,
    pub dead: bool,
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    pub hit: HitPacket,
    pub charge: Option<ChargeVals>,
    pub facing_left: bool,
    pub offset: Real,
    pub active: bool,
    pub boom_x: Real,
    pub boom_y: Real,
    pub boom_in: i32,
    pub dz: Real,
    pub hurt_r: Real,
    pub hurt_seg: Real,
    pub hurt_up: Real,
    pub attack_type: i32,
    /// The hitboxes (their actors) now overlapping the Hurtbox.
    pub overlapped_by: IndexSet<super::ActorRef>,
    pub owner: FighterId,
    pub attack_id: i32,
}

/// `AimPunch` (C019AimPunch_C).
/// Its data (`p`) is the class's.
#[derive(State, Debug)]
pub struct AimPunch {
    /// Always C019AimPunch_C.
    pub cls: Name,
    /// `id`.
    pub seq: i32,
    pub x: Real,
    pub y: Real,
    pub prev_x: Real,
    pub prev_y: Real,
    pub server_x: Real,
    pub server_y: Real,
    pub age: i32,
    pub dead: bool,
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    pub check_in: i32,
    pub owner: FighterId,
}

/// `Accelerator` (ProjectileAccelerator_C). Its components (`comps`) are the class's.
/// Its data (`p`) is the class's.
#[derive(State, Debug)]
pub struct Accelerator {
    /// Always ProjectileAccelerator_C.
    pub cls: Name,
    /// `id`.
    pub seq: i32,
    pub x: Real,
    pub y: Real,
    pub facing: Real,
    pub age: i32,
    pub dead: bool,
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// The projectiles (actor `id`s) overlapping the box.
    pub inside: IndexSet<i32>,
    pub doom: Option<i32>,
    pub end_tick: i32,
    pub owner: FighterId,
}
