//! Gizmo's own fighter state (`GizmoState`): Pawn_C023A's own variables, those of
//! its components (Comp_C023A_AllyAttach; Comp_HoldCharge's are the fighter's hold charge) and of C023A_AnimBP the sim
//! reads, and the live actors he owns. Offsets are Pawn_C023A's ubergraph's unless a function is named. One record per
//! class file, with its class defaults; a field several files read lives in the record of the file that writes it.
//! The fields every file reads are `GizmoState`'s own and the first fields of the attach record. A spawned actor's own
//! variables are on the actor (its `ProjectileExt` variant), not here. Sim space: x = -UE Y, y = UE Z.
//!
//! No record for the bow (bow.rs): Comp_HoldCharge and AnimNotify_HoldChargeSet are shared classes, their variables
//! the fighter's hold charge for any pawn whose data carries the component; the arrow's and
//! the BowSlow's are on the actor and the buff entry.

use crate::Real;
use crate::State;
use crate::actors::ProjectileId;
use crate::chars::CharState;
use crate::combat::BuffRef;
use crate::fighter::FighterId;
use crate::fighter::anim::{SlotBlend, SlotMix, SlotPlayers};
use crate::names::Name;
use crate::world::World;

/// `GizmoPawnState` (pawn.rs): Pawn_C023A's binds @888-1278, the Delay chains @1040 -> 35 and @1095 -> 473, the
/// arrow-fire visibility @544-930, BPI_C023A, the small notifies; C023A_AnimBP's sim slot mix.
#[derive(State, Debug)]
pub struct GizmoPawnState {
    /// C023A_AnimBP's slot graph mix (the pose update writes it from pawn.rs's slot_input / slot_layer hooks), with
    /// its timed BlendListByBool nodes (`slot_blend`: _3, _4, _1) and its SequencePlayers and the Driving machine
    /// (`slot_players`; a new anim instance at a new pawn: the new state, pawn.rs match_reset).
    pub slot_mix: SlotMix,
    /// C023A_AnimBP variables the sim sets (class defaults false / 0): OverrideFallLean
    /// (BPI_OverrideFallLeanUmbrella @11999), Stop Overrides (BPI_StopBoneOverrides @12069), IsRolling
    /// (BPI_SetRolling @12089), IsTurrning and DrivingTurning (DrivingTurn @10992-11118: 1 facing left, 2 right).
    pub override_fall_lean: bool,
    pub stop_overrides: bool,
    pub rolling: bool,
    pub turning: bool,
    pub driving_turning: i32,
    /// DrivingTurn's RetriggerableDelay(0.2) @11040 -> IsTurrning = false @576: the frames left, 0 none.
    pub turn_delay: i32,
    /// COM_BlendStartTime / COM_BlendTime (Set COM Angle @11332-11434; the angle and alpha are `GizmoState`'s
    /// `com_override` / `com_override_alpha`).
    pub com_blend_start: Real,
    pub com_blend_time: Real,
    /// The Delay(0.2) of BeginPlay @1095 -> 473 (frames left, 0 done).
    pub vis_delay: i32,
    /// VisibilityDelay (set @768).
    pub visibility_delay: bool,
    /// HasMaxArrow (@1413 / @4027).
    pub has_max_arrow: bool,
    /// FacingChangeFire's last FacingLeft (bound @888 behind the Delay: 0 not called yet, 1 left, 2 right;
    /// ArrowCharge / ArrowFireCharged's relative rotations @4435-4613, the drawing's).
    pub fire_facing: i32,
}

impl Default for GizmoPawnState {
    fn default() -> Self {
        GizmoPawnState {
            slot_mix: SlotMix {
                slot_blend: Some(SlotBlend::default()),
                slot_players: Some(SlotPlayers::default()),
                ..SlotMix::default()
            },
            override_fall_lean: false,
            stop_overrides: false,
            rolling: false,
            turning: false,
            driving_turning: 0,
            turn_delay: 0,
            com_blend_start: 0.0,
            com_blend_time: 0.0,
            vis_delay: 0,
            visibility_delay: false,
            has_max_arrow: false,
            fire_facing: 0,
        }
    }
}

/// `GizmoAttach` (attach.rs): Comp_C023A_AllyAttach, DashToAlly, DettachFromAlly (Gizmo's side),
/// ApplyBuffToAttached, MoveToActorHit, StopMoveToAlly, the four conditions, C023A_AttachCooldown / ShareDamageBuff /
/// Buff_C023A_AttachShrink. The component's record: its first fields are the shared ones (the conditions, the AnimBP's
/// AttachedToAlly, car.rs, popcorn.rs, the view and the HUD read them), then the others (AttachOffset, AttachTimer...).
#[derive(State, Debug)]
pub struct GizmoAttach {
    /// IsAttached.
    pub attached: bool,
    /// CarryingActor: the ally carried (Attach to Actor), `None` none.
    pub carrying: Option<FighterId>,
    /// MoveToAllyBuff: the live Buff_MoveToActor2 of AtemptToReachAlly @466-569, on
    /// the owner; `None` none.
    pub move_buff: Option<BuffRef>,
    /// TrackedMeter: the attach meter (Get Charge UI, Toggle_UI_Bar / UpdateMeter: the HUD draws it).
    pub meter: Real,
    /// AttachOffset (Attach to Actor @3710: (-2 x radius, 0, 2 x half height) of the ally's original capsule) and the
    /// owner's RelativeLocation under the ally's mesh (K2_SetActorRelativeLocation @3900: AttachOffset /
    /// AttachOffsetScale, Y 0; then Pawn_C023A's CarryTurnTimeline). X is the stage depth under the mesh's yaw (the
    /// view reads it), Z the height above the Root socket (attach.rs follow).
    pub off_x: Real,
    pub off_z: Real,
    pub rel_x: Real,
    pub rel_z: Real,
    /// Pawn_C023A BPI_AttachTurn's Attach Offset and CarryTurnTimeline (length 0.2, TurnAlpha): playing, its position.
    pub turn_x: Real,
    pub turn_z: Real,
    pub turn_on: bool,
    pub turn_pos: Real,
    /// DamageShareBuff (on the carried ally, @4498) and ShrinkBuff (on the owner, @4672), `None` none.
    pub share_buff: Option<BuffRef>,
    pub shrink_buff: Option<BuffRef>,
    /// The owner's InputTriggered bound (@4302, unbound @2259); the ally's FacingChanged / OnExitedArena /
    /// NewBuffSpawned bound (@4938-5066, unbound @1689-1962).
    pub input_bound: bool,
    pub ally_bound: bool,
    /// BuffClassesToCheck (OnCarryingActorBuffSpawned @15) and its Delay(0.0) pending (@112 -> @167).
    pub buff_queue: Vec<Name>,
    pub queue_due: bool,
    /// AttachTimer (K2_SetTimer(AttachTimerTick, 0.5, looping) @5154): the World tick it was set at (-1: none) and
    /// its next expiry in counted seconds.
    pub timer_set: i32,
    pub timer_next: Real,
    /// SetActorEnableCollision(false) @2430 .. (true) @799: the owner's whole collision off (the shrink buff's
    /// `actor_collision_off` reads it).
    pub collision_off: bool,
    /// The fighters whose buffs AnimNotify_C023A_ApplyBuffToAttached filed under its notify (AddNotifySpawnedBuffs
    /// @482).
    pub notify_holders: Vec<FighterId>,
    /// Pawn_C023A LastDashTarget when it is the car (BPI_FindDashTarget @2029 with a C023A_Car_SpawnedActor):
    /// `GizmoState::last_dash_target` holds a fighter; attach.rs last_dash_target reads the two.
    pub dash_car: Option<ProjectileId>,
}

impl Default for GizmoAttach {
    fn default() -> Self {
        GizmoAttach {
            attached: false,
            carrying: None,
            move_buff: None,
            meter: 0.0,
            off_x: 0.0,
            off_z: 0.0,
            rel_x: 0.0,
            rel_z: 0.0,
            turn_x: 0.0,
            turn_z: 0.0,
            turn_on: false,
            turn_pos: 0.0,
            share_buff: None,
            shrink_buff: None,
            input_bound: false,
            ally_bound: false,
            buff_queue: Vec::new(),
            queue_due: false,
            timer_set: -1,
            timer_next: 0.0,
            collision_off: false,
            notify_holders: Vec::new(),
            dash_car: None,
        }
    }
}

/// `GizmoCarState` (car.rs: C023A_Car_SpawnedActor, C023A_Car_Buff, the alternate cooldown): empty; the car's
/// variables are on the actor (`car::GizmoCar`), the live one is `GizmoState::car`.
#[derive(State, Debug, Default)]
pub struct GizmoCarState {}

/// `GizmoMusicState` (music.rs: C023A_MusicPickup, C023A_Music_Buff / _DeBuff, C023A_Music_Projectile,
/// C023A_PianoNote, C023A_Buff_Crippled): empty.
#[derive(State, Debug, Default)]
pub struct GizmoMusicState {}

/// `GizmoPopcornState` (popcorn.rs, umbrella.rs: C023A_PopCorn, C023A_ProjectileBarrier, Buff_C023A_ProjectileBlock,
/// Buff_C023A_AllyProjectileBlock): empty.
#[derive(State, Debug, Default)]
pub struct GizmoPopcornState {}

/// `GizmoPerkState` (perks.rs: Buff_C023A_Backpack, Buff_C023A_Bounce, Buff_C023A_Dance, C023A_DanceCount_Debuff,
/// C023A_ForceDance_Debuff): empty.
#[derive(State, Debug, Default)]
pub struct GizmoPerkState {}

/// `GizmoState`.
#[derive(State, Debug, Default)]
pub struct GizmoState {
    /// pawn.rs.
    pub pawn: GizmoPawnState,
    /// attach.rs (Comp_C023A_AllyAttach).
    pub attach: GizmoAttach,
    /// car.rs (`car` below is the live actor).
    pub cars: GizmoCarState,
    /// music.rs.
    pub music: GizmoMusicState,
    /// popcorn.rs.
    pub popcorn: GizmoPopcornState,
    /// perks.rs.
    pub perks: GizmoPerkState,
    /// The live C023A_Car_SpawnedActor (car.rs), `None` none.
    pub car: Option<ProjectileId>,
    /// The C023A_Car_SpawnedActor he drives (car.rs: UpdateDriverInfo's Driver; his own or a team mate Gizmo's),
    /// `None` none.
    pub drives: Option<ProjectileId>,
    /// C023A_AnimBP IsDriving (its Driving event @10968; car.rs writes, the view's Driving machine reads).
    pub driving: bool,
    /// C023A_AnimBP COM_Override_Angle / COM_Override_Alpha (its Set COM Angle event @11241-11462; pawn.rs
    /// set_com_angle writes; the view reads).
    pub com_override: Real,
    pub com_override_alpha: Real,
    /// The lantern submesh shown (C023A_AnimBP ToggleSubmeshVisibility(show_lantern, Lantern) @11921; pawn.rs writes,
    /// the view reads).
    pub lantern: bool,
    /// Pawn_C023A LastDashTarget: BPI_FindDashTarget (its own function: @1976 null, @2029 the closest ally; attach.rs
    /// find_dash_target, called by C023A_AllyInRange_Condition) and the SetLastDashTargetMulticast event @4007; read
    /// by BPI_GetLastDashTarget (pawn.rs's last_dash_target hook: the COM_Rotation entry,
    /// AnimNotify_C023A_DashToAlly).
    pub last_dash_target: Option<FighterId>,
}

// ---- the call surface of the state (`gz`, `gzOf`)

/// `gzOf`: the fighter's Gizmo state, `None` for a fighter of another character (a failed cast to Pawn_C023A /
/// BPI_C023A).
pub fn gizmo_of(w: &World, f: FighterId) -> Option<&GizmoState> {
    match &w.fighter(f).cs {
        CharState::Gizmo(s) => Some(s),
        _ => None,
    }
}

/// `gzOf`, to write.
pub fn gizmo_mut(w: &mut World, f: FighterId) -> Option<&mut GizmoState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Gizmo(s) => Some(s),
        _ => None,
    }
}

/// `gz` (his own code's read: the fighter is a Gizmo). A panic is a hook entered for a fighter of another character.
pub fn gizmo(w: &World, f: FighterId) -> &GizmoState {
    gizmo_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Gizmo"))
}

/// `gz`, to write.
pub fn gizmo_w(w: &mut World, f: FighterId) -> &mut GizmoState {
    gizmo_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Gizmo"))
}
