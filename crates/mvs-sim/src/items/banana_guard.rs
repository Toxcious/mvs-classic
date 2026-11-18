//! ItemHelper_BananaGuard_C: a plain ACharacter, one of the two helpers of
//! Bugs's ACME TV (helper.rs), made by Item_Helper's BeginDeferredActorSpawnFromClass / FinishSpawningActor at
//! (0, Y, Z) of the TV (@595-1089), so it has no spawn request (the kind's `deferred`). Not a SpawnedActorParent:
//! Comp_Spawner does not list it (`add_actor`'s spawner false). Offsets are the ubergraph's.
//!
//! Sim space: x = -UE Y, y = UE Z; (x, y) the actor location, the CollisionCylinder's centre (210 / 100).
//! - BeginPlay @1307: the owner a BP_Fighter_Character -> the handler's Instigator and SetTeamIndexOnSpawn(its team)
//!   @1317-1548; OriginalScale = Mesh.RelativeScale3D @1616; the capsule's world yaw -90 @1665 (constant: its local X
//!   is sim +x); SetFacing(the owner's BPI_IsFacingLeft) @1911 (MoveAlpha still 0: no input, no hit boxes);
//!   Delay(RunDelayTime) @2219 -> @15 and Delay(LifeTime) @2278 -> @246; BPI_IgnorePlatformCollision(self, true) on
//!   every platform of the game state's BPI_GetIgnorablePlatforms @664-659; SpawnDefaultController @2132 (a plain
//!   AIController, possession -> SetDefaultMovementMode); PF_Post_Event(sfx_cha_bananaguard_spawn) @127.
//! - @15 (the run): MoveInpute = SelectFloat(1, -1, FacingLeft), MoveAlpha 1, TurnOnHitBoxes.
//! - ReceiveTick -> @692: AddInputVector((0, MoveInpute, 0)); RunGate && |Velocity.Y| < 100: RunGate false,
//!   MoveInpute 0, HitBoxesOff(HitBoxTags), Delay(StopDelayTime) @1085 -> @193; else |Velocity.Y| >= 500: RunGate
//!   true.
//! - @193: SetFacing(!FacingLeft) -> SetFacingMulticast @2358: FacingLeft, the mesh's X scale (-OriginalScale.X
//!   facing left), MoveInpute = MoveAlpha x SelectFloat(1, -1, FacingLeft), MoveAlpha > 0 -> TurnOnHitBoxes @261.
//! - @246 (LifeTime): K2_DestroyActor; ReceiveEndPlay @2785 -> DeathEffectMulticast @2800: PS_TomJerryPoof00 at the
//!   location, PF_Post_Event_At_Location(sfx_cha_bananaguard_death).
//! - TurnOnHitBoxes: HitBoxDamage.KnockBackDirectionVector = (SelectFloat(-1, 1, FacingLeft), 1),
//!   HitBoxesOn(HitBoxTags, HitBoxDamage, HitBoxScale, ElementalBuffs, On Hit Buffs).
//!
//! BPI_BlastBoxExit @3004 (K2_DestroyActor) is never called for it: GameModes/BlastBoxPlayerExit HandleBlastBoxExit
//! @247-410 takes the root capsule through a BPI_Elemental_System cast, which the guard does not implement (it has
//! BPI_BlastBox alone), so the null capsule fails the compare and the handler returns. Only LifeTime ends it.
//! Deviation: the stage's KillZ (AActor::FellOutOfWorld) is not read: a guard that falls off waits out its LifeTime.
//!
//! The banana guard holds no Buff_ class: its hits' OnHitBuffs rows are the class data's, applied by the
//! shared hit path.

use std::sync::Arc;

use indexmap::IndexSet;
use mvs_data::fighter::ActorDef;
use mvs_data::value::{Props, Value};

use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActor, WorldActorState};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::{ExtraHurt, WorldSphere, shape_over_box, shape_over_capsule};
use crate::combat::elem::charge_pct;
use crate::combat::hit::hurt::hurt_rank;
use crate::combat::hits::{actor_hit_enemy, ally_hit, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::packet::DataVec;
use crate::combat::phases::{alive, give_vbuf, no_hit, victim_pose};
use crate::combat::spawned_hits::{class_rows, damage_actor, damage_spawned_actor};
use crate::consts::{DELTA_SECONDS, FLOOR_REST_GAP, GRAVITY_Z, SOLID_FLOOR};
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::body::PlatformRef;
use crate::fighter::common::{actor_data, actor_def, latent_frames, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::invulnerable;
use crate::fighter::moves::V2;
use crate::fighter::moves::calc_velocity;
use crate::fighter::traces::solids::static_solids_and_polys;
use crate::math::{max, min, sign, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::WorldActorHooks;
use crate::registry::props::{num, obj};
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;
use crate::world::pawns::{all_list, put_fighters};
use crate::world::spawn_spot::{capsule_c, capsule_shape, find_teleport_spot_shape};
use crate::world::spawned_platform::BlockKind;
use crate::world::spawning::new_world_actor;
use crate::world::stage_query::PolyBlock;
use mvs_collision::Ground;
use mvs_collision::solids::{WALKABLE_Z, poly_floor};

const CLS: &str = "ItemHelper_BananaGuard_C";

// The SpearHitBox (BoxExtent 200 / 40 / 100 at (0, 0, 25) on CharacterMesh0's bone Spear_Tip) rides the pose of
// BananaGuard_AnimBp. Its place per frame of each clip of the mesh's skeleton is the data's
// components.SpearHitBox.boneBox (boneBoxTrack: FK of Root > Spear > Spear_Tip, mesh space
// before the mesh's scale: centre (fwd, up, depth) and the half extent vectors of the box's X, Y, Z); the clips are
// 60 fps, so a frame is a tick. The AnimBP's Movement machine is ported here alone: the view draws the clip of the
// sim's state at the sim's time and runs no machine of its own.
// Movement (BakedStateMachines[0]): entry Spawn, MaxTransitionsPerFrame 3, bSkipFirstUpdateTransition, every player
// rate 1 from 0. Every transition that can be taken has crossfade 0 (a blend of duration 0 ends in the update that
// starts it), and every transition into FallTransition is skipped (its only exit is always false), so Fall and Land
// are never entered and the pose is always one state's clip, alone:
//   Spawn  -> Idle    remaining('Spawn') == 0 && !Falling (@1567)
//   Idle   -> RunIn   HorizontalSpeed >= 50 (@1290)
//   Run    -> IdleIn  HorizontalSpeed < 50 && !Falling (@975)
//   IdleIn -> Idle    remaining('IdleIn') == 0 (@1395)
//   RunIn  -> Run     remaining('RunIn') == 0 (@841)
// HorizontalSpeed = |Velocity.Y| (the sim's |vel_x|), Falling = !IsMovingOnGround (@364, @160). remaining() reads the
// state's player only once an update has given it a weight (GetRelevantAnimTimeRemaining: FLT_MAX with none), so a
// state just entered cannot be left in the same update by a remaining() rule. A state entered starts its player at 0
// (its weight is 0 at the entry: SetState re-initialises it); the update then advances the current state's player by
// dt in float32 (the player's time is a float: GetRelevantAnimTimeRemaining's length - time), by AdvanceTime
// 142816220: a time within [0, length] stays (length itself included, so a looping clip shows its last frame for an
// update before it wraps), past it a looping clip takes the fmod, another clamps.
// The update's dt is the sim's DeltaTime (DELTA_SECONDS), as every other tick of the sim.
// Unverified: the box is placed from the pose of this tick's anim update at the world's hit pass (the mesh ticks
// after the CMC; the bones' transforms and the attached box's move are taken as done within the frame).
// The depth (mesh Y, UE X) is world +X facing either way: the capsule turns to yaw -90 for both facings (@1665) and
// the guard faces by mesh scale X (@2423-2575), as a mirror-node fighter's hurt shapes; with the box's X axis (200)
// along the depth in every clip it reaches across the play plane either way.
const S_SPAWN: i32 = 0;
const S_IDLE: i32 = 1;
const S_RUN: i32 = 2;
const S_IDLE_IN: i32 = 3;
const S_RUN_IN: i32 = 4;
/// The clip of each state of the Movement machine (by its index).
pub const GUARD_STATE_CLIP: [&str; 5] =
    ["BananaGuard_Spawn", "BananaGuard_Idle", "BananaGuard_Run", "BananaGuard_Idle_In", "BananaGuard_Dash"];
const STATE_LOOP: [bool; 5] = [false, true, true, false, false];
/// The states' names (by index).
pub const GUARD_STATES: [&str; 5] = ["Spawn", "Idle", "Run", "IdleIn", "RunIn"];
const FLT_MAX: Real = 3.402_823_5e38;
const RUN_SPEED: Real = 50.0;

// Stock UCharacterMovementComponent values the class does not set:
// FallingLateralFriction 0, BrakingDecelerationFalling 0, terminal velocity 20000 (the default physics volume).
// Unverified: MaxSimulationTimeStep 0.05 and MaxSimulationIterations 8 are the 4.26 ctor's (not read in the exe): one
// substep a frame. Unverified: the perch radius clamp(radius - PerchRadiusThreshold, 0.11, radius) and the walking
// floor probe MaxStepHeight + MAX_FLOOR_DIST 2.4 are the 4.26 source; the rest gap over a floor is the fighters'
// FLOOR_REST_GAP.
const TERMINAL: Real = 20000.0;
const MAX_FLOOR_DIST: Real = 2.4;

/// One clip's SpearHitBox track: the sequence's length (float32), its frames, 12 numbers a frame.
#[derive(Clone, Debug)]
pub struct SpearClip {
    pub len: Real,
    pub n: usize,
    pub v: Vec<Real>,
}

/// The parsed boneBox track by state (`None`: the data has no clip of that name).
pub type SpearClips = [Option<SpearClip>; 5];

/// CharacterMovement's MovementMode: None until possessed.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum GuardMode {
    #[default]
    None,
    Walk,
    Fall,
}

/// A shape the capsule meets: the stage block (`World.ground`), a stage solid's box of `static_solids` (by its place
/// in that list; as a base it is lost at the next tick, see [`base_box`]), a world actor's or a projectile's box
/// ([`PlatformRef::Spawned`]) or a polygon stage solid ([`PlatformRef::Poly`]).
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum GuardSolid {
    Ground,
    Static(u32),
    Part(PlatformRef),
}

/// A box of the tick's list, or a polygon's floor found under the capsule: what it is, its sides (a polygon's: the box
/// round it, its top the floor's height) and the place its move is measured from as the base (`base_x` / `base_y`: a
/// PlatformBox's centre x and top, 0 and the top for another box, a polygon's move from its data place).
#[derive(Clone, Copy, Debug)]
struct Solid {
    key: GuardSolid,
    left: Real,
    right: Real,
    top: Real,
    bottom: Real,
    follow: (Real, Real),
}

/// What the capsule meets this tick: the boxes and the polygon stage solids.
struct Met {
    boxes: Vec<Solid>,
    polys: Vec<PolyBlock>,
}

/// `BananaGuard` (ItemHelper_BananaGuard_C).
#[derive(State, Debug)]
pub struct BananaGuard {
    /// The actor location (sim x, y), the CollisionCylinder's centre.
    pub x: Real,
    pub y: Real,
    /// HitBoxDamage.AttackID.
    pub attack_id: i32,
    /// Its own HitBoxDamage (TurnOnHitBoxes writes the direction).
    pub hit: HitPacket,
    /// Comp_Actor_Destructible.TeamIndex (SetTeamIndexOnSpawn: the owner's).
    pub team: i32,
    // The CMC.
    pub radius: Real,
    pub half: Real,
    pub gravity: Real,
    pub max_walk: Real,
    pub max_accel: Real,
    pub ground_friction: Real,
    /// GroundFriction x BrakingFrictionFactor.
    pub braking_friction: Real,
    pub braking_decel: Real,
    pub air_control: Real,
    /// The valid perch radius.
    pub perch: Real,
    pub step_height: Real,
    pub walk_off_ledges: bool,
    /// The mesh (CharacterMesh0 on the capsule): its Z and scale (OriginalScale).
    pub mesh_z: Real,
    pub mesh_scale: Real,
    pub mode: GuardMode,
    /// CharacterMovement.Velocity (sim x, y).
    pub vel_x: Real,
    pub vel_y: Real,
    /// The movement base while walking.
    pub base: Option<GuardSolid>,
    /// The base's place at the last move (UpdateBasedMovement).
    pub base_x: Real,
    pub base_y: Real,
    /// The pawn's pending input vector along sim x (AddInputVector), consumed by the CMC's next tick.
    pub input: Real,
    // The graph's variables.
    pub facing_left: bool,
    /// MoveInpute (UE Y).
    pub move_in: Real,
    pub move_alpha: Real,
    pub run_gate: bool,
    /// Comp_Pawn_HitBoxHandler: its tagged box on, the fighters hit since HitBoxesOff (HitEachActorOnlyOnce), by id.
    pub hit_on: bool,
    pub hit_set: IndexSet<i32>,
    /// The spawned actors hit since HitBoxesOff (their damage boxes, their pawn bodies).
    pub hit_actors: IndexSet<ActorRef>,
    /// The latent Delays, in ticks of `age` (the spawn frame counted): -1 when none is pending.
    pub age: i32,
    pub run_at: i32,
    pub life_at: i32,
    pub stop_at: i32,
    /// The SpawnedPlatformParent Collision boxes its capsule ignores (BPI_IgnorePlatformCollision at BeginPlay).
    /// Every stage platform is in the same list: the guard never meets one.
    pub ignored: Vec<PlatformRef>,
    /// The world tick its tick ran in (the spawn frame's runs at the spawn).
    pub ticked_at: i32,
    /// The data has the SpearHitBox's boneBox.
    pub spear: bool,
    /// BananaGuard_AnimBp's Movement machine (the SpearHitBox's pose): the state (S_*), its player's time, and
    /// whether an update has given the player a weight (GetRelevantAnimTime's relevant player).
    pub anim: i32,
    pub anim_time: Real,
    pub anim_rel: bool,
    /// The parsed boneBox tracks (static data, made at the first use). Not state.
    #[state(skip)]
    pub clips: Option<Arc<SpearClips>>,
}

impl Default for BananaGuard {
    fn default() -> Self {
        BananaGuard {
            x: 0.0,
            y: 0.0,
            attack_id: 0,
            hit: HitPacket::default(),
            team: 0,
            radius: 0.0,
            half: 0.0,
            gravity: 0.0,
            max_walk: 0.0,
            max_accel: 0.0,
            ground_friction: 0.0,
            braking_friction: 0.0,
            braking_decel: 0.0,
            air_control: 0.0,
            perch: 0.0,
            step_height: 0.0,
            walk_off_ledges: false,
            mesh_z: 0.0,
            mesh_scale: 1.0,
            mode: GuardMode::None,
            vel_x: 0.0,
            vel_y: 0.0,
            base: None,
            base_x: 0.0,
            base_y: 0.0,
            input: 0.0,
            facing_left: false,
            move_in: 0.0,
            move_alpha: 0.0,
            run_gate: false,
            hit_on: false,
            hit_set: IndexSet::new(),
            hit_actors: IndexSet::new(),
            age: 0,
            run_at: 0,
            life_at: 0,
            stop_at: -1,
            ignored: Vec::new(),
            ticked_at: -1,
            spear: false,
            anim: S_SPAWN,
            anim_time: 0.0,
            anim_rel: false,
            clips: None,
        }
    }
}

/// The guard's place (its `x`, `y`).
pub fn place(_w: &World, _a: &WorldActor, s: &BananaGuard) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// The dealer's parts: its place, facing and AttackID. A DealerActor's side for the hit's X (`projectile_hit_dir`:
/// sign(vx || facing)): the handler's direction is KnockBackDirectionVector.X as TurnOnHitBoxes wrote it
/// (DirectionalKnockback false, the guard's yaw -90 and no BPI_Spawned_Actor parent velocity), so the side is +1
/// whatever way it runs.
pub fn dealer_parts(_w: &World, _a: &WorldActor, s: &BananaGuard) -> (Real, Real, Real, i32) {
    (s.x, s.y, 1.0, s.attack_id)
}

/// Its HitBoxDamage.
pub fn hit(s: &BananaGuard) -> &HitPacket {
    &s.hit
}

fn st(w: &World, id: ActorId) -> &BananaGuard {
    match &w.actors.actors.get(id).expect("a live actor").state {
        WorldActorState::BananaGuard(g) => g,
        _ => unreachable!("ItemHelper_BananaGuard_C holds a BananaGuard"),
    }
}

fn st_mut(w: &mut World, id: ActorId) -> &mut BananaGuard {
    match &mut w.actors.actors.get_mut(id).expect("a live actor").state {
        WorldActorState::BananaGuard(g) => g,
        _ => unreachable!("ItemHelper_BananaGuard_C holds a BananaGuard"),
    }
}

fn owner(w: &World, id: ActorId) -> FighterId {
    w.actors.actors.get(id).expect("a live actor").owner
}

fn class_data(w: &World, id: ActorId) -> Option<&ActorDef> {
    let cls = w.actors.actors.get(id)?.cls;
    actor_def(w, ActorRef::Actor(id), cls)
}

/// The SpearHitBox component of the class data with its boneBox string.
fn bone_box(d: &ActorDef) -> Option<&str> {
    obj(&d.components, "SpearHitBox")?.get("boneBox")?.as_str()
}

/// JavaScript `+s` for the numbers of the boneBox string.
fn js_num(s: &str) -> Real {
    let t = s.trim();
    if t.is_empty() { 0.0 } else { t.parse::<Real>().unwrap_or(Real::NAN) }
}

/// The boneBox tracks parsed: `name,len,n,v...` per clip, `;` between clips.
fn parse_clips(w: &World, s: &str) -> SpearClips {
    let mut by: Vec<(&str, SpearClip)> = Vec::new();
    for part in s.split(';') {
        let a: Vec<&str> = part.split(',').collect();
        let n = js_num(a.get(2).copied().unwrap_or(""));
        let n = usize::try_from(to_int32(n)).unwrap_or(0);
        let v = (0..n * 12).map(|i| js_num(a.get(3 + i).copied().unwrap_or(""))).collect();
        let len = js_num(a.get(1).copied().unwrap_or(""));
        by.push((a[0], SpearClip { len, n, v }));
    }
    GUARD_STATE_CLIP.map(|name| {
        let k = by.iter().rev().find(|(n, _)| *n == name).map(|(_, c)| c.clone());
        if k.is_none() {
            warn_text(w, &format!("{CLS}: SpearHitBox boneBox has no clip {name}"));
        }
        k
    })
}

/// The parsed tracks (made at the first use from the class data); `None` without the data's track.
fn clips(w: &mut World, id: ActorId) -> Option<Arc<SpearClips>> {
    let g = st(w, id);
    if !g.spear {
        return None;
    }
    if let Some(c) = &g.clips {
        return Some(Arc::clone(c));
    }
    let s = class_data(w, id).and_then(bone_box)?.to_owned();
    let c = Arc::new(parse_clips(w, &s));
    st_mut(w, id).clips = Some(Arc::clone(&c));
    Some(c)
}

// ---------------------------------------------------------------- the graph

fn turn_on_hit_boxes(g: &mut BananaGuard) {
    g.hit.knock_back_direction_vector =
        Some(DataVec { x: Some(if g.facing_left { -1.0 } else { 1.0 }), y: Some(1.0), z: None });
    g.hit_on = true; // HitBoxesOn keeps the handler's hit list
}

fn hit_boxes_off(g: &mut BananaGuard) {
    g.hit_on = false;
    g.hit_set.clear();
    g.hit_actors.clear();
}

/// SetFacing -> SetFacingMulticast @2358.
fn set_facing(g: &mut BananaGuard, left: bool) {
    g.facing_left = left;
    g.move_in = g.move_alpha * if left { 1.0 } else { -1.0 };
    if g.move_alpha > 0.0 {
        turn_on_hit_boxes(g);
    }
}

/// ReceiveTick -> @692.
fn receive_tick(g: &mut BananaGuard, stop_delay: Real) {
    g.input += -g.move_in; // UE +Y is sim -x
    let s = g.vel_x.abs();
    if g.run_gate && s < 100.0 {
        g.run_gate = false;
        g.move_in = 0.0;
        hit_boxes_off(g);
        if g.stop_at < 0 {
            g.stop_at = g.age + to_int32(latent_frames(stop_delay)); // made in the tick: counted in it
        }
    } else if s >= 500.0 {
        g.run_gate = true;
    }
}

/// ProcessLatentActions after the tick; true: @246 K2_DestroyActor.
fn latent(g: &mut BananaGuard) -> bool {
    g.age += 1;
    if g.age == g.run_at {
        g.move_in = if g.facing_left { 1.0 } else { -1.0 }; // @15
        g.move_alpha = 1.0;
        turn_on_hit_boxes(g);
    }
    if g.age == g.stop_at {
        g.stop_at = -1;
        let left = !g.facing_left;
        set_facing(g, left); // @193
    }
    g.age == g.life_at
}

// ---------------------------------------------------------------- the AnimBP (the spear's pose)

/// The transition the state's rules take now (-1: none), in their baked order (the skipped ones into FallTransition
/// left out).
fn next_state(g: &BananaGuard, c: &SpearClips, speed: Real, falling: bool) -> i32 {
    let k = usize::try_from(g.anim).ok().and_then(|i| c[i].as_ref());
    let rem = match k {
        Some(k) if g.anim_rel => k.len - g.anim_time,
        _ => FLT_MAX,
    };
    match g.anim {
        S_SPAWN => {
            if rem == 0.0 && !falling {
                S_IDLE
            } else {
                -1
            }
        }
        S_IDLE => {
            if speed >= RUN_SPEED {
                S_RUN_IN
            } else {
                -1
            }
        }
        S_RUN => {
            if speed < RUN_SPEED && !falling {
                S_IDLE_IN
            } else {
                -1
            }
        }
        S_IDLE_IN => {
            if rem == 0.0 {
                S_IDLE
            } else {
                -1
            }
        }
        _ => {
            if rem == 0.0 {
                S_RUN
            } else {
                -1
            }
        }
    }
}

/// One update of the machine (FAnimNode_StateMachine::Update_AnyThread), with the inputs as they are now
/// (BlueprintUpdateAnimation).
fn anim_update(w: &mut World, id: ActorId, dt: Real) {
    let Some(c) = clips(w, id) else { return };
    let g = st_mut(w, id);
    let speed = g.vel_x.abs();
    let falling = g.mode != GuardMode::Walk;
    for _ in 0..3 {
        let s = next_state(g, &c, speed, falling);
        if s < 0 {
            break;
        }
        g.anim = s;
        g.anim_time = 0.0;
        g.anim_rel = false;
    }
    let i = usize::try_from(g.anim).expect("a state index");
    let len = c[i].as_ref().map_or(0.0, |k| k.len);
    // the sequence player's FAnimationRuntime::AdvanceTime 142816220 (FMath::Fmod wrap)
    crate::math::ue::advance_time(STATE_LOOP[i], dt, &mut g.anim_time, len);
    g.anim_rel = true;
}

/// The SpearHitBox of the pose: the clip's frames at the player's time (the place between two frames linearly), by
/// the mesh's scale (X negated facing left: -OriginalScale.X) on the capsule plus the mesh's Z. `None` without the
/// track. Also the cheat overlay's `hitBoxShape`.
pub fn spear_pose(w: &mut World, id: ActorId) -> Option<WorldSphere> {
    let c = clips(w, id)?;
    let g = st(w, id);
    let k = c[usize::try_from(g.anim).ok()?].as_ref()?;
    let (n, v) = (k.n, &k.v);
    let fpos = if n > 1 && k.len > 0.0 {
        min(max(g.anim_time / k.len, 0.0), 1.0) * crate::real(u32::try_from(n - 1).expect("a frame count"))
    } else {
        0.0
    };
    let f0 = fpos.floor();
    let i0 = usize::try_from(to_int32(f0)).expect("a frame index");
    let i1 = (i0 + 1).min(n - 1);
    let a = fpos - f0;
    let (o0, o1) = (i0 * 12, i1 * 12);
    let lerp = |i: usize| v[o0 + i] + (v[o1 + i] - v[o0 + i]) * a;
    let m = g.mesh_scale;
    let sx = if g.facing_left { -m } else { m };
    let mut e = [0.0; 9];
    for j in 0..3 {
        e[j * 3] = sx * lerp(3 + j * 3);
        e[j * 3 + 1] = m * lerp(4 + j * 3);
        e[j * 3 + 2] = m * lerp(5 + j * 3);
    }
    Some(WorldSphere {
        x: g.x + sx * lerp(0),
        y: g.y + g.mesh_z + m * lerp(1),
        z: Some(m * lerp(2)),
        r: 0.0,
        bx: Some(e),
        ..WorldSphere::default()
    })
}

// ---------------------------------------------------------------- the CMC

/// The boxes that block its capsule: the stage block, the stage solids that block Pawn (`static_solids`), the parts
/// of the SpawnedPlatformParent children it does not ignore (Collision and WallbounceCollision: the Pawn profile
/// blocks Platform, and nothing calls BPI_IgnoreWallbounceCollision for it) and the projectiles' boxes:
/// BugsBunnyRocketFProjectile's Collision (Platform) and BugsBunnyRocketUpProjectile's Box (Custom, WorldDynamic;
/// Pawn unlisted: Block); and beside them the polygon stage solids `static_solids` meets.
/// Deviation: C019_Ship's pieces (Platform profile; the ship's BPI_IgnorePlatformCollision is empty) are not met.
fn solids(w: &mut World, ignored: &[PlatformRef]) -> Met {
    let g = w.stage.ground;
    let mut out = vec![Solid {
        key: GuardSolid::Ground,
        left: crate::math::narrow(g.left),
        right: crate::math::narrow(g.right),
        top: crate::math::narrow(g.top),
        bottom: crate::math::narrow(g.bottom),
        follow: (0.0, crate::math::narrow(g.top)),
    }];
    let mut st_boxes: Vec<Ground> = Vec::new();
    let mut polys: Vec<PolyBlock> = Vec::new();
    static_solids_and_polys(w, SOLID_FLOOR, name!("BananaGuard capsule"), &mut st_boxes, Some(&mut polys));
    for (i, b) in st_boxes.iter().enumerate() {
        out.push(Solid {
            key: GuardSolid::Static(u32::try_from(i).expect("a short list")),
            left: crate::math::narrow(b.left),
            right: crate::math::narrow(b.right),
            top: crate::math::narrow(b.top),
            bottom: crate::math::narrow(b.bottom),
            follow: (0.0, crate::math::narrow(b.top)),
        });
    }
    for (id, a) in w.actors.actors.iter() {
        let Some(pl) = &a.plat else { continue };
        if a.dead {
            continue;
        }
        for (j, s) in pl.solids().iter().enumerate() {
            let r = PlatformRef::Spawned(ActorRef::Actor(id), u32::try_from(j).expect("a part index"));
            if s.on && !s.dead && !ignored.contains(&r) {
                out.push(Solid {
                    key: GuardSolid::Part(r),
                    left: s.left,
                    right: s.right,
                    top: s.top,
                    bottom: s.bottom,
                    follow: (s.x, s.top),
                });
            }
        }
    }
    for (id, pr) in w.actors.projectiles.iter() {
        let Some(ss) = &pr.solids else { continue };
        if pr.dead || pr.no_collision {
            continue;
        }
        for (j, s) in ss.iter().enumerate() {
            if s.on && !s.dead {
                let r = PlatformRef::Spawned(ActorRef::Projectile(id), u32::try_from(j).expect("a box index"));
                out.push(Solid {
                    key: GuardSolid::Part(r),
                    left: s.left,
                    right: s.right,
                    top: s.top,
                    bottom: s.bottom,
                    follow: (s.x, s.top),
                });
            }
        }
    }
    Met { boxes: out, polys }
}

// The polygon stage solids (Tree Fort's tree tops): the capsule meets them by the shape traces' convex sweep
// (`PolyBlock::sweep`) and stands on one where the down sweep of its round bottom rests (`poly_floor_at`).
// Unverified: a walkable polygon face is a floor up to MaxStepHeight over the capsule's bottom (MoveAlongFloor's ramp
// and StepUp taken as the walk onto it); a non-walkable face met by a move stops the move at the contact (a fall's
// velocity into the face dropped), with no slide along it.

/// The key of a polygon solid as a base.
fn poly_key(q: &PolyBlock) -> GuardSolid {
    GuardSolid::Part(PlatformRef::Poly(q.solid))
}

/// The capsule from (x, y) moved by (dx, dy) against the polygon: the shape traces' sweep of its segment.
fn sweep(g: &BananaGuard, q: &PolyBlock, x: Real, y: Real, dx: Real, dy: Real) -> Option<mvs_collision::traces::Hit> {
    q.sweep(x, y, dx, dy, 0.0, max(g.half - g.radius, 0.0), g.radius)
}

/// The polygon's floor at x, as a shape found (`None`: none there, or not walkable): FindFloor's down sweep of the
/// capsule's round bottom (its sphere's rest on the outline: the floor's height is the sphere's centre less the
/// radius), walkable where the flat base of the perch radius finds a walkable floor (`poly_floor`).
fn poly_floor_at(g: &BananaGuard, q: &PolyBlock, x: Real) -> Option<Solid> {
    if !poly_floor(&q.poly, q.ox, q.oy, x, g.perch).0 {
        return None;
    }
    let b = q.bounds();
    let (top, bottom) = (crate::math::narrow(b.top), crate::math::narrow(b.bottom));
    let (c, _, inside) = q.down(x, top + g.radius + 1.0, bottom - g.radius, g.radius)?;
    if inside {
        return None;
    }
    Some(Solid {
        key: poly_key(q),
        left: crate::math::narrow(b.left),
        right: crate::math::narrow(b.right),
        top: c - g.radius,
        bottom,
        follow: (q.ox, q.oy),
    })
}

/// FindFloor at x for a capsule whose bottom is at `bottom`: the highest box top at most `below` under it (and not
/// over it), with x inside the box widened by the perch radius; a polygon's walkable floor at most `below` under it
/// and at most MaxStepHeight over it. Deviation: a box top over the bottom is a wall (the step-up of MaxStepHeight is
/// not modelled).
fn floor_at(g: &BananaGuard, m: &Met, x: Real, bottom: Real, below: Real) -> Option<Solid> {
    let mut best: Option<Solid> = None;
    for b in &m.boxes {
        if x < b.left - g.perch || x > b.right + g.perch {
            continue;
        }
        if b.top > bottom + 1e-3 || b.top < bottom - below {
            continue;
        }
        if best.is_none_or(|o| b.top > o.top) {
            best = Some(*b);
        }
    }
    for q in &m.polys {
        let Some(f) = poly_floor_at(g, q, x) else { continue };
        if f.top > bottom + g.step_height || f.top < bottom - below {
            continue;
        }
        if best.is_none_or(|o| f.top > o.top) {
            best = Some(f);
        }
    }
    best
}

/// The floor taken as the base, the capsule kept at the rest gap over it (AdjustFloorHeight: a swept move, so a rise
/// stops under a box or a polygon over the capsule).
fn stand_on(g: &mut BananaGuard, b: &Solid, m: &Met) {
    g.base = Some(b.key);
    let mut y = b.top + g.half + FLOOR_REST_GAP;
    if y > g.y {
        for o in &m.boxes {
            if o.key != b.key && g.x + g.radius > o.left && g.x - g.radius < o.right && o.bottom >= g.y + g.half - 1e-3
            {
                y = min(y, o.bottom - g.half);
            }
        }
        let rise = y - g.y;
        for q in &m.polys {
            if poly_key(q) == b.key {
                continue;
            }
            if let Some(h) = sweep(g, q, g.x, g.y, 0.0, rise)
                && !h.initial
            {
                y = min(y, g.y + rise * crate::math::narrow(h.time));
            }
        }
        y = max(y, g.y);
    }
    g.y = y;
    (g.base_x, g.base_y) = b.follow;
}

fn start_falling(g: &mut BananaGuard) {
    g.mode = GuardMode::Fall;
    g.base = None;
}

/// SetDefaultMovementMode at the possession: Walking (FindFloor down MaxStepHeight + MAX_FLOOR_DIST, the floor
/// snapped to), else Falling with the velocity kept.
fn set_default_movement_mode(w: &mut World, id: ActorId) {
    let ignored = st(w, id).ignored.clone();
    let m = solids(w, &ignored);
    let g = st_mut(w, id);
    match floor_at(g, &m, g.x, g.y - g.half, g.step_height + MAX_FLOOR_DIST) {
        Some(fl) => {
            g.mode = GuardMode::Walk;
            g.vel_y = 0.0;
            stand_on(g, &fl, &m);
        }
        None => start_falling(g),
    }
}

/// Whether a box stands across the capsule's height at y (a wall for a horizontal move).
fn across(g: &BananaGuard, b: &Solid, y: Real) -> bool {
    b.top > y - g.half + 1e-3 && b.bottom < y + g.half
}

/// PhysWalking (stock, the same as the FMC's: `calc_velocity`, the guard's values): the move along the floor,
/// stopped at a wall; at a ledge (no floor in the perch radius) with bCanWalkOffLedges false, GetLedgeMove's sideways
/// (depth) try is turned back along the run by the next iteration's friction (GroundFriction 100: min(dt x 100, 1) =
/// 1), so the move is reverted with the velocity zeroed (RevertMove(fail)). CheckFall's bMustJump: a zero delta, or a
/// base that is gone or has its collision off (lost), and no floor: it falls at once, the move made and the velocity
/// kept (StartFalling; the move was whole, so no time is left for PhysFalling).
fn phys_walking(g: &mut BananaGuard, m: &Met, a: Real, dt: Real, lost: bool) {
    g.vel_x = calc_velocity(
        g.vel_x,
        sign(a),
        g.max_walk,
        g.ground_friction,
        a.abs(),
        g.braking_friction,
        g.braking_decel,
        dt,
    );
    let x0 = g.x;
    let dx = g.vel_x * dt;
    let bottom = g.y - g.half;
    if dx.abs() < 1e-4 {
        match floor_at(g, m, x0, bottom, g.step_height + MAX_FLOOR_DIST) {
            None => start_falling(g),
            Some(fl) => stand_on(g, &fl, m),
        }
        return;
    }
    let mut nx = x0 + dx;
    for b in &m.boxes {
        if !across(g, b, g.y) {
            continue;
        }
        if dx > 0.0 && x0 + g.radius <= b.left + 1e-3 && nx + g.radius > b.left {
            nx = b.left - g.radius;
        } else if dx < 0.0 && x0 - g.radius >= b.right - 1e-3 && nx - g.radius < b.right {
            nx = b.right + g.radius;
        }
    }
    // A polygon's face that is no walkable ramp: the move stops at the contact (none into a face it touches).
    for q in &m.polys {
        let Some(h) = sweep(g, q, x0, g.y, nx - x0, 0.0) else { continue };
        if h.ny >= WALKABLE_Z {
            continue;
        }
        if !h.initial {
            nx = x0 + (nx - x0) * crate::math::narrow(h.time);
        } else if f64::from(dx) * h.nx < 0.0 {
            nx = x0;
        }
    }
    let Some(fl) = floor_at(g, m, nx, bottom, g.step_height + MAX_FLOOR_DIST) else {
        if lost {
            g.x = nx;
            start_falling(g);
        } else {
            g.vel_x = 0.0;
        }
        return;
    };
    g.x = nx;
    stand_on(g, &fl, m);
    g.vel_x = (nx - x0) / dt;
}

/// PhysFalling (stock): FallingLateralFriction 0 and BrakingDecelerationFalling 0 (ApplyVelocityBraking returns at
/// once: no input, no change), the stock GetMaxAcceleration (MaxAcceleration x AirControl), NewFallVelocity to the
/// 20000 terminal, the midpoint move. A box met from above in the perch radius is a landing: Walking with Velocity.Z
/// 0 at the contact, then PhysWalking for the rest of the frame (StartNewPhysics); so is a polygon's walkable face met
/// on the way down where the flat base finds its floor. Deviation: a box corner met outside the perch radius pushes
/// the capsule out to the side, and a side or a ceiling stops that axis (the hemisphere's slide is not modelled).
fn phys_falling(g: &mut BananaGuard, m: &Met, a: Real, dt: Real) {
    let (vx0, vy0) = (g.vel_x, g.vel_y);
    if a != 0.0 {
        let acc = min(a.abs() * g.air_control, g.max_accel);
        g.vel_x = calc_velocity(vx0, sign(a), g.max_walk, 0.0, acc, 0.0, 0.0, dt);
    }
    let mut vy1 = vy0 - g.gravity * dt;
    if vy1 < -TERMINAL {
        vy1 = -TERMINAL;
    }
    g.vel_y = vy1;
    let (x0, y0) = (g.x, g.y);
    let mut nx = x0 + 0.5 * (vx0 + g.vel_x) * dt;
    let mut ny = y0 + 0.5 * (vy0 + vy1) * dt;
    let mut land: Option<Solid> = None;
    for b in &m.boxes {
        if nx + g.radius <= b.left || nx - g.radius >= b.right || ny - g.half >= b.top || ny + g.half <= b.bottom {
            continue;
        }
        if vy1 <= 0.0 && y0 - g.half >= b.top - 1e-3 {
            if nx >= b.left - g.perch && nx <= b.right + g.perch {
                if land.is_none_or(|l| b.top > l.top) {
                    land = Some(*b);
                }
            } else {
                nx = if nx < b.left { b.left - g.radius } else { b.right + g.radius };
                g.vel_x = 0.0;
            }
        } else if vy1 > 0.0 && y0 + g.half <= b.bottom + 1e-3 {
            ny = b.bottom - g.half;
            g.vel_y = 0.0;
        } else {
            nx = if x0 < b.left { b.left - g.radius } else { b.right + g.radius };
            g.vel_x = 0.0;
        }
    }
    // The polygons, along the move the boxes left: the first walkable face met going down with a floor under the flat
    // base there is a landing; the first other face (or a walkable one with no floor) stops the move.
    let (sx, sy) = (nx - x0, ny - y0);
    let mut p_land: Option<(Solid, Real)> = None;
    let mut stop: Option<(Real, f64, f64)> = None;
    for q in &m.polys {
        let Some(h) = sweep(g, q, x0, y0, sx, sy) else { continue };
        if h.initial && f64::from(sx) * h.nx + f64::from(sy) * h.ny >= 0.0 {
            continue;
        }
        let t = crate::math::narrow(h.time);
        if vy1 <= 0.0
            && h.ny >= WALKABLE_Z
            && let Some(f) = poly_floor_at(g, q, x0 + sx * t)
        {
            if p_land.is_none_or(|(_, pt)| t < pt) {
                p_land = Some((f, t));
            }
            continue;
        }
        if stop.is_none_or(|(st, _, _)| t < st) {
            stop = Some((t, h.nx, h.ny));
        }
    }
    if let Some((t, fx, fy)) = stop {
        nx = x0 + sx * t;
        ny = y0 + sy * t;
        let (fx, fy) = (crate::math::narrow(fx), crate::math::narrow(fy));
        let vn = g.vel_x * fx + g.vel_y * fy;
        if vn < 0.0 {
            g.vel_x -= vn * fx;
            g.vel_y -= vn * fy;
        }
    }
    let p_land = p_land.filter(|&(_, pt)| stop.is_none_or(|(st, _, _)| pt <= st));
    // The share of the move made before a box's contact.
    let box_t = |top: Real| if y0 - ny > 0.0 { min(max((y0 - g.half - top) / (y0 - ny), 0.0), 1.0) } else { 1.0 };
    let (land, t) = match (land, p_land) {
        (Some(l), Some((p, pt))) => {
            let bt = box_t(l.top);
            if pt < bt { (p, pt) } else { (l, bt) }
        }
        (Some(l), None) => (l, box_t(l.top)),
        (None, Some((p, pt))) => (p, pt),
        (None, None) => {
            g.x = nx;
            g.y = ny;
            return;
        }
    };
    g.x = x0 + (nx - x0) * t;
    g.mode = GuardMode::Walk;
    g.vel_y = 0.0;
    stand_on(g, &land, m);
    let rest = dt * (1.0 - t);
    if rest > 1e-6 {
        phys_walking(g, m, a, rest, false); // MIN_TICK_TIME
    }
}

/// UpdateBasedMovement: the base's move since the last tick as one swept move of the capsule that stops at the first
/// box or polygon it meets (the base itself and a shape it is already in left out).
fn follow_base(g: &mut BananaGuard, m: &Met, dx: Real, dy: Real) {
    let mut t: Real = 1.0;
    let (l0, r0, b0, t0) = (g.x - g.radius, g.x + g.radius, g.y - g.half, g.y + g.half);
    for b in &m.boxes {
        if Some(b.key) == g.base || (r0 > b.left && l0 < b.right && t0 > b.bottom && b0 < b.top) {
            continue;
        }
        let (mut tin, mut tout): (Real, Real) = (0.0, 1.0);
        if dx == 0.0 {
            if r0 <= b.left || l0 >= b.right {
                continue;
            }
        } else {
            let (t1, t2) = ((b.left - r0) / dx, (b.right - l0) / dx);
            tin = max(tin, min(t1, t2));
            tout = min(tout, max(t1, t2));
        }
        if dy == 0.0 {
            if t0 <= b.bottom || b0 >= b.top {
                continue;
            }
        } else {
            let (t1, t2) = ((b.bottom - t0) / dy, (b.top - b0) / dy);
            tin = max(tin, min(t1, t2));
            tout = min(tout, max(t1, t2));
        }
        if tin < tout && tin < t {
            t = tin;
        }
    }
    for q in &m.polys {
        if Some(poly_key(q)) == g.base {
            continue;
        }
        if let Some(h) = sweep(g, q, g.x, g.y, dx, dy)
            && !h.initial
        {
            t = min(t, crate::math::narrow(h.time));
        }
    }
    g.x += dx * t;
    g.y += dy * t;
}

/// A based shape now: its collision on and alive (`!dead && on`) and the place its move is measured from
/// ([`Solid`]'s `follow`); `None` for one gone (its actor or projectile left its list) or a stage solid's box.
// Deviation: a stage solid's box as the base is dropped. Its SolidBox has `on` (its `enabled`) but no `x`, so
// keeping it would write x = NaN in followBase; the base is lost and the guard falls.
fn base_box(w: &World, b: GuardSolid) -> Option<(bool, Real, Real)> {
    match b {
        GuardSolid::Part(PlatformRef::Spawned(ActorRef::Actor(id), j)) => {
            let p = w.actors.actors.get(id)?.plat.as_ref()?.parts.get(usize::try_from(j).ok()?)?;
            Some((!p.dead && p.on, p.x, p.top))
        }
        GuardSolid::Part(PlatformRef::Spawned(ActorRef::Projectile(id), j)) => {
            let p = w.actors.projectiles.get(id)?.solids.as_ref()?.get(usize::try_from(j).ok()?)?;
            Some((!p.dead && p.on, p.x, p.top))
        }
        GuardSolid::Part(PlatformRef::Poly(i)) => {
            let s = w.stage_run.as_ref()?.solids.get(usize::try_from(i).ok()?)?;
            Some((s.enabled, s.m.dx, s.m.dy))
        }
        _ => None,
    }
}

/// The CMC's tick (bTickBeforeOwner: ahead of ReceiveTick): UpdateBasedMovement, then the input of the last tick
/// (ConsumeInputVector; Acceleration = MaxAcceleration x the input clamped to 1).
fn cmc_tick(w: &mut World, id: ActorId) {
    let g = st_mut(w, id);
    let a = max(-1.0, min(1.0, g.input)) * g.max_accel;
    g.input = 0.0;
    if g.mode == GuardMode::None {
        return;
    }
    let ignored = st(w, id).ignored.clone();
    let m = solids(w, &ignored);
    let g = st(w, id);
    let mut lost = false;
    let mut follow: Option<(Real, Real)> = None;
    if g.mode == GuardMode::Walk
        && let Some(b) = g.base
        && b != GuardSolid::Ground
    {
        match base_box(w, b) {
            Some((true, x, top)) => follow = Some((x, top)),
            _ => lost = true,
        }
    }
    let g = st_mut(w, id);
    if lost {
        g.base = None;
    }
    if let Some((x, top)) = follow {
        let (dx, dy) = (x - g.base_x, top - g.base_y);
        if dx != 0.0 || dy != 0.0 {
            follow_base(g, &m, dx, dy);
        }
        g.base_x = x;
        g.base_y = top;
    }
    if g.mode == GuardMode::Walk {
        phys_walking(g, &m, a, DELTA_SECONDS, lost);
    } else {
        phys_falling(g, &m, a, DELTA_SECONDS);
    }
}

/// The kind's `step`: the CMC, ReceiveTick, the latent actions, then the mesh's anim update, after the CMC
/// (Unverified: the order within the frame), with the inputs after the move.
fn step(w: &mut World, id: ActorId) {
    let tick = w.flow.tick;
    if st(w, id).ticked_at == tick {
        return;
    }
    st_mut(w, id).ticked_at = tick;
    cmc_tick(w, id);
    let stop_delay = class_data(w, id).and_then(|d| num(&d.p, "StopDelayTime")).unwrap_or(0.0);
    let g = st_mut(w, id);
    receive_tick(g, stop_delay);
    if latent(g) {
        w.actors.actors.get_mut(id).expect("a live actor").dead = true; // @246 K2_DestroyActor
    }
    anim_update(w, id, DELTA_SECONDS);
}

// ---------------------------------------------------------------- the spawn

/// The guard made from its class data at (x, y) for the owner `f` (the constructor).
fn construct(w: &World, f: FighterId, a: &ActorDef, x: Real, y: Real) -> BananaGuard {
    static EMPTY: std::sync::LazyLock<Props> = std::sync::LazyLock::new(Props::new);
    let c = &a.components;
    let cap = obj(c, "CollisionCylinder").unwrap_or(&EMPTY);
    let mv = obj(c, "CharMoveComp").unwrap_or(&EMPTY);
    let mesh = obj(c, "CharacterMesh0").unwrap_or(&EMPTY);
    let hd = obj(&a.p, "HitBoxDamage").unwrap_or(&EMPTY);
    let mut hit = HitPacket::from_props(&w.names, hd, None);
    hit.knock_back_direction_vector = Some(DataVec { x: Some(1.0), y: Some(1.0), z: None });
    let radius = num(cap, "CapsuleRadius").unwrap_or(34.0); // the ACharacter ctor's 34 / 88
    let ground_friction = num(mv, "GroundFriction").unwrap_or(8.0);
    let walk_off_ledges = !matches!(mv.get("bCanWalkOffLedges"), Some(Value::Bool(false)));
    if walk_off_ledges {
        warn_text(w, &format!("{CLS}: bCanWalkOffLedges is not the false this port walks with"));
    }
    let spear = bone_box(a).is_some();
    if !spear {
        warn_text(w, &format!("{CLS}: no SpearHitBox boneBox in the data (the boxes' actor bone boxes): no hits"));
    }
    BananaGuard {
        x,
        y,
        attack_id: hit.attack_id.unwrap_or(0),
        hit,
        team: w.fighter(f).team.team,
        radius,
        half: num(cap, "CapsuleHalfHeight").unwrap_or(88.0),
        gravity: GRAVITY_Z * num(mv, "GravityScale").unwrap_or(1.0),
        max_walk: num(mv, "MaxWalkSpeed").unwrap_or(600.0),
        max_accel: num(mv, "MaxAcceleration").unwrap_or(2048.0),
        ground_friction,
        braking_friction: ground_friction * num(mv, "BrakingFrictionFactor").unwrap_or(2.0),
        braking_decel: num(mv, "BrakingDecelerationWalking").unwrap_or(2048.0),
        air_control: num(mv, "AirControl").unwrap_or(0.05),
        perch: min(max(radius - num(mv, "PerchRadiusThreshold").unwrap_or(0.0), 0.11), radius),
        step_height: num(mv, "MaxStepHeight").unwrap_or(45.0),
        walk_off_ledges,
        mesh_z: obj(mesh, "RelativeLocation").and_then(|l| num(l, "Z")).unwrap_or(0.0),
        mesh_scale: obj(mesh, "RelativeScale3D").and_then(|l| num(l, "X")).unwrap_or(1.0),
        spear,
        run_at: to_int32(latent_frames(num(&a.p, "RunDelayTime").unwrap_or(0.0))),
        life_at: to_int32(latent_frames(num(&a.p, "LifeTime").unwrap_or(0.0))),
        ..BananaGuard::default()
    }
}

/// The kind's `deferred`: BeginDeferredActorSpawnFromClass (collision handling 2, AdjustIfPossibleButAlwaysSpawn:
/// FindTeleportSpot against what blocks Pawn, ahead of BeginPlay and its platform ignores), the Instigator set,
/// FinishSpawningActor: BeginPlay, then its tick of the spawn frame (TG_NewlySpawned). Unverified: the boxes that
/// block Pawn are taken as the nav query's (the stage block and the boxes that block PhysicsBody).
pub fn spawn_at(w: &mut World, f: FighterId, cls: Name, x: Real, y: Real) -> ActorId {
    let data = Arc::clone(&w.data);
    let a = actor_data(w, &data, ActorRef::Fighter(f), cls)
        .unwrap_or_else(|| panic!("{CLS}: no actor data for its spawn (the importer gives every spawned class)"));
    let shape = capsule_shape(obj(&a.components, "CollisionCylinder").map(capsule_c), 1.0, 0.0, 1.0);
    let mut boxes = w.blocker_shapes(BlockKind::Nav);
    boxes.boxes.insert(0, w.stage.ground);
    let at = find_teleport_spot_shape(boxes.view(), x, y, &shape);
    let mut g = construct(w, f, a, at.x, at.y);
    set_facing(&mut g, w.fighter(f).body.facing_dir < 0.0); // @1911 (MoveAlpha 0)
    // @664: BPI_IgnorePlatformCollision(self, true) on each ignorable platform. The stage's are never met; a
    // SpawnedPlatformParent's Collision (a PlatformPart: not StevenCollisionBlock's Box) and the capsule ignore each
    // other (the parent @876-940).
    for (aid, x2) in w.actors.actors.iter() {
        let Some(pl) = &x2.plat else { continue };
        if x2.dead || matches!(x2.state, WorldActorState::StevenBlock(_)) {
            continue;
        }
        for (j, s) in pl.solids().iter().enumerate() {
            if !s.wall && !s.dead {
                g.ignored.push(PlatformRef::Spawned(ActorRef::Actor(aid), u32::try_from(j).expect("a part index")));
            }
        }
    }
    let actor =
        new_world_actor(w, cls, WorldActorClass::ItemHelperBananaGuard, f, false, WorldActorState::BananaGuard(g));
    let id = w.actors.actors.insert(actor);
    set_default_movement_mode(w, id); // SpawnDefaultController @2132
    let (gx, gy) = (st(w, id).x, st(w, id).y);
    let fighter = w.fighter(f).id;
    let ev = name!("sfx_cha_bananaguard_spawn"); // @127
    w.events.push(Event::Sound { fighter, ev, x: Some(gx), y: Some(gy), hit: None, pooled: false, key: None });
    // InitAnim: the machine's first update, DeltaTime 0 (it takes bSkipFirstUpdateTransition; no rule can pass: both
    // of Spawn's read remaining(), FLT_MAX before any update). Deviation: InitAnim reads the inputs at the
    // registration (MovementMode None: Falling true, speed 0); no rule reads them here.
    anim_update(w, id, 0.0);
    step(w, id);
    id
}

// ---------------------------------------------------------------- the hits

/// ElementalBuffs.ElementalBuffs of the class data.
fn elementals(w: &World, id: ActorId) -> Vec<Name> {
    let Some(d) = class_data(w, id) else { return Vec::new() };
    let list = obj(&d.p, "ElementalBuffs").and_then(|e| e.get("ElementalBuffs"));
    match list {
        Some(Value::Arr(a)) => a.iter().filter_map(|v| v.as_str()).filter_map(|s| w.names.get(s)).collect(),
        _ => Vec::new(),
    }
}

/// A projectile class's damage box now (`ProjectileKind.damageBox`).
fn projectile_damage_box(w: &World, p: crate::actors::ProjectileId) -> Option<WorldSphere> {
    let cls = w.actors.projectiles.get(p)?.cls;
    match w.registry.actors.get(w.names.str(cls))?.hooks {
        ActorHooks::Projectile(h) => h.damage_box.and_then(|f| f(w, p)),
        _ => None,
    }
}

/// A World.actors class's pawn body now (`ActorKind.pawnBody`).
fn actor_pawn_body(w: &World, o: ActorId) -> Option<ExtraHurt> {
    let cls = w.actors.actors.get(o)?.cls;
    match w.registry.actors.get(w.names.str(cls))?.hooks {
        ActorHooks::World(h) => h.pawn_body.and_then(|f| f(w, o)),
        _ => None,
    }
}

/// The SpearHitBox (object type HitBox; Overlap to Pawn, HurtBox, DamageDestructible, ProjectileDestructible,
/// ItemDestructible) at the world's hit pass, each overlap's begin a pfgDoDamage with the handler's gates in the
/// projectiles' test's order (DoLOSCheck false; HitEachActorOnlyOnce tracks every victim until HitBoxesOff). The box
/// is the oriented one of the pose (`spear_pose`), tested as a fighter's own box hit shapes are (`hurt_rank`):
/// - a fighter's hurt shapes: an enemy's hit or dodge; a team mate other than the Instigator gets NotifyAllyOfHit
///   (`ally_hit`: the HitByAlly a frozen Buff_Ice breaks on) once the gates pass (an invulnerable one pops);
/// - a spawned actor's damage box (ProjectileHooks::damage_box: SeismicReactor_C's DamageBox, the safe drop's
///   DamageCollision): `damage_spawned_actor` (its team's: HitByAlly, nothing applied);
/// - another World.actors pawn's root capsule (WorldActorHooks::pawn_body: another guard's CollisionCylinder, profile
///   Pawn, which the SpearHitBox overlaps): `damage_actor` (its team's: no hit).
///
/// Deviation: the projectiles' own hurt bodies (ProjectileDestructible: the clank / DestroyProjectiles path, which
/// takes a Projectile or an Item as the dealer) and the items' (ItemDestructible) are not tested. Unverified: the
/// overlap begin is taken at the hit pass, not inside the capsule's move.
fn hits(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    if !st(w, id).hit_on {
        return;
    }
    let Some(spear) = spear_pose(w, id) else { return };
    let (cx, cy) = (spear.x, spear.y);
    let o = owner(w, id);
    let all = all_list(w);
    for &vic in &all {
        let vid = w.fighter(vic).id;
        let g = st(w, id);
        if !alive(w, vic) || no_hit(w, vic) || g.hit_set.contains(&vid) {
            continue;
        }
        let ally = w.fighter(vic).team.team == g.team;
        if ally && vic == o {
            continue; // NotifyAllyOfHit needs Victim != Instigator; DamageAndLaunch fails
        }
        let Some(vp) = victim_pose(w, vic) else { continue };
        let rank = hurt_rank(w, vic, &spear, &vp);
        give_vbuf(w, vp);
        if rank < 0.0 {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        let hit = st(w, id).hit.clone();
        if invulnerable(w, vic) {
            if !ally {
                let (aid, tick) = (st(w, id).attack_id, w.flow.tick);
                attack_dodged(w, me, vic, aid, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            }
            continue;
        }
        if hit.hit_each_actor_only_once.unwrap_or(false) {
            st_mut(w, id).hit_set.insert(vid);
        }
        if ally {
            ally_hit(w, &dealer(w, me), vic, &hit);
            continue;
        }
        dodge_over(w, me, vic);
        let rows = class_rows(w, me);
        let elems = elementals(w, id);
        let d = dealer(w, me);
        let pct = charge_pct(w, me);
        if apply_projectile_hit(w, &d, vic, &hit, &rows, V2 { x: cx, y: cy }, None, &elems, pct) {
            actor_hit_enemy(w, &d, vic);
        }
    }
    put_fighters(w, all);
    let mut k = 0;
    while let Some(pr) = w.actors.projectiles.nth(k) {
        k += 1;
        let pref = ActorRef::Projectile(pr);
        if w.actors.projectiles.get(pr).is_none_or(|p| p.dead) || st(w, id).hit_actors.contains(&pref) {
            continue;
        }
        let Some(db) = projectile_damage_box(w, pr) else { continue };
        if !shape_over_box(&spear, &db) {
            continue;
        }
        let hit = st(w, id).hit.clone();
        if hit.hit_each_actor_only_once.unwrap_or(false) {
            st_mut(w, id).hit_actors.insert(pref);
        }
        damage_spawned_actor(w, me, pr, &hit);
    }
    let mut k = 0;
    while let Some(b) = w.actors.actors.nth(k) {
        k += 1;
        let bref = ActorRef::Actor(b);
        if b == id || w.actors.actors.get(b).is_none_or(|x| x.dead) || st(w, id).hit_actors.contains(&bref) {
            continue;
        }
        let Some(body) = actor_pawn_body(w, b) else { continue };
        if !shape_over_capsule(&spear, &body) {
            continue;
        }
        let hit = st(w, id).hit.clone();
        if hit.hit_each_actor_only_once.unwrap_or(false) {
            st_mut(w, id).hit_actors.insert(bref);
        }
        damage_actor(w, me, b, &hit, None);
    }
}

/// The CollisionCylinder (ACharacter's profile Pawn, which overlaps the HitBox channel) as the target of hit shapes
/// (WorldActorHooks::pawn_body: the fighters' melee test, a projectile's body overlap, another guard's spear). The
/// guard has Comp_Pawn_DamageManager (CanTakeDamage: the CDO's true), Comp_Actor_Destructible (its TeamIndex: the
/// owner's) and Comp_HitPause, no Comp_Actor_Damage, and binds none of the damage manager's delegates nor
/// ReceiveAnyDamage: a hit of another team is a Hit for the dealer (its hit pause, HitNonPlayer, HitAnything) and
/// changes nothing on the guard (no damage, no launch, no hit pause: `actor_damage_and_launch`). The capsule is
/// upright on the play plane: its segment ends at the centre +- (half height - radius).
fn pawn_body(w: &World, id: ActorId) -> Option<ExtraHurt> {
    let g = st(w, id);
    Some(ExtraHurt { x: g.x, y0: g.y - g.half + g.radius, y1: g.y + g.half - g.radius, r: g.radius })
}

/// ReceiveEndPlay @2785 -> DeathEffectMulticast @2800.
fn end(w: &mut World, id: ActorId) {
    if reset_pass(w) {
        return; // the Lab reset's end pass
    }
    let (x, y) = (st(w, id).x, st(w, id).y);
    let a = w.actors.actors.get(id).expect("a live actor");
    let (cls, fighter) = (a.cls, a.owner_id);
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter,
        vfx: Some(name!("PS_TomJerryPoof00")),
        x,
        y,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: 1.0,
        tint: None,
        to: None,
    });
    let ev = name!("sfx_cha_bananaguard_death");
    w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
}

/// This file's registry entries, (ItemHelper_BananaGuard_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = WorldActorHooks {
        deferred: Some(spawn_at),
        step: Some(step),
        hits: Some(hits),
        pawn_body: Some(pawn_body),
        end: Some(end),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        CLS,
        ActorEntry {
            // An ACharacter, no SpawnedActorParent_C: the Lab reset's GetAllActorsOfClass(SpawnedActorParent_C) does
            // not destroy it; it lives out its LifeTime. Deviation: its Instigator and owner there are the old pawn
            // (torn down a second later); the port keeps the respawned fighter of the same slot.
            keep_at_lab_reset: true,
            ..ActorEntry::new(ActorClass::World(WorldActorClass::ItemHelperBananaGuard), ActorHooks::World(hooks))
        },
    )
}
