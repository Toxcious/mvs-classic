//! The pawn core of Pawn_C023A: BeginPlay's binds and Delays, the arrow-fire
//! visibility, BPI_C023A (Set COM Angle, the last dash target, the AllyAttach component), the small notifies
//! (OverrideFallLean, StopBoneOverrides, SetRolling, ResetAim) and C023A_AnimBP's sim part; its hooks (`pawnHooks`).
//! Its variables are `GizmoState::pawn` (state.rs).
//!
//! The pawn API the other files call is here: the Comp_HoldCharge handlers bow.rs binds ([`hold_charge_reached_max`],
//! [`hold_charge_ended`], [`hold_charge_used`]) and [`set_com_angle`]. `setAllyAttachComp` is a
//! direct call: the `ally_attach` hook calls attach.rs's `ally_attach`.
//!
//! Ability slots: gizmo.json slot 0 (bow) MaxCount 4, StartCount 4, CountCooldown 8; slot 1 (car) Cooldown -1,
//! AlternateCooldowns [15]; slot 2 (popcorn) MaxCount 3, StartCount 3, CountCooldown 10. The shared abilities code takes
//! the charges from the row and the alternate cooldown through `begin_alternate_cooldown` (car.rs calls it from the car's
//! own graph). No shared gap.
//!
//! Not here: the AI, HeadScale / the Trails / CopyBone of the AnimBP (the view's).

use super::{attach, gizmo, gizmo_mut, gizmo_w};
use crate::Real;
use crate::actors::ActorRef;
use crate::combat::disguise::{copy_running, set_copy_aim};
use crate::events::NotifyRef;
use crate::fighter::anim::slot_blend::{slot_blend_reset, slot_players_reset};
use crate::fighter::anim::{SlotMix, graph_player_time_from_end, graph_relevant_anim_time_remaining, set_sub_mesh};
use crate::fighter::common::latent_frames;
use crate::fighter::moves::is_falling;
use crate::fighter::{FighterId, actor_y, facing};
use crate::math::to_int32;
use crate::name;
use crate::names::Name;
use crate::registry::char_hooks::{AllyAttach, DashTarget};
use crate::registry::{CharHooks, NotifyHandler, Registry, RegistryError};
use crate::world::World;

// ---------------------------------------------------------------- the hooks

/// `pawnHooks`' hooks that are not joined with another file's (`setComAngle`, `lastDashTarget`, `allyAttach`,
/// `slotInput`, `slotLayer`, `slotMix`, `tick`, `turned`, `death`): `matchReset` and `timerTick` are composed in
/// mod.rs.
pub fn hooks(h: &mut CharHooks) {
    h.set_com_angle = Some(set_com_angle);
    h.last_dash_target = Some(last_dash_target);
    h.ally_attach = Some(ally_attach);
    h.slot_input = Some(slot_input);
    h.slot_layer = Some(slot_layer);
    h.slot_mix = Some(slot_mix);
    h.slot_mix_mut = Some(slot_mix_mut);
    h.tick = Some(tick);
    h.turned = Some(turned);
    h.death = Some(death);
}

/// pawn.rs's part of the joined matchReset (after mod.rs's new state): ReceiveBeginPlay @4685 -> 1030 (a new pawn:
/// mod.rs's match_reset made the state new). Delay(1.0) @1040 -> 35 is the attach bar widget (UI_FinnCharge, the HUD's:
/// nothing of the sim); Delay(0.2) @1095 -> 473 counts from here (`tick`); the HoldCharge binds @1150-1278 are bow.rs's.
/// The anim instance is new: its lists and players at INDEX_NONE.
pub fn match_reset(w: &mut World, f: FighterId) {
    let p = &mut gizmo_w(w, f).pawn;
    if let Some(b) = p.slot_mix.slot_blend.as_mut() {
        slot_blend_reset(b);
    }
    if let Some(b) = p.slot_mix.slot_players.as_mut() {
        slot_players_reset(b);
    }
    p.vis_delay = to_int32(latent_frames(0.2));
}

/// pawn.rs's part of the joined timerTick (before attach.rs's): the AnimBP's latent action (DrivingTurn's
/// RetriggerableDelay), in the world's latent pass ahead of the timers.
// Unverified: an anim instance's latent actions are taken in the frame's timer pass, and one started in the frame
// counts that frame (the convention of the spawned actors' Delays); the instance's place in the latent pass is not read.
pub fn timer_tick(w: &mut World, f: FighterId) {
    let p = &mut gizmo_w(w, f).pawn;
    if p.turn_delay > 0 {
        p.turn_delay -= 1;
        if p.turn_delay == 0 {
            p.turning = false;
        }
    }
}

/// `tick`: the pawn's latent actions after its ReceiveTick (Pawn_C023A has no ReceiveTick of its own).
// Unverified: BeginPlay's Delay(0.2) @1095 -> 473 is counted down in the `tick` hook (the pawn's tick, its latent pass),
// from latentFrames(0.2) set at the match reset; the frame it resumes in follows the port's latent convention, no game
// read.
pub fn tick(w: &mut World, f: FighterId) {
    let p = &mut gizmo_w(w, f).pawn;
    if p.vis_delay > 0 {
        p.vis_delay -= 1;
        if p.vis_delay == 0 {
            visibility_delay_done(w, f);
        }
    }
}

/// `turned`: SetRotationLogic's FacingChanged(FacingLeft) broadcast to the pawn's FacingChangeFire (bound @888 behind
/// the Delay) and to the AnimBP's DrivingTurn (bound @1783-1820 while IsDriving, unbound @1862 at a
/// BPI_DrivingCar(false)).
pub fn turned(w: &mut World, f: FighterId) {
    let left = facing(w, f) < 0.0;
    let s = gizmo_w(w, f);
    if s.pawn.visibility_delay {
        s.pawn.fire_facing = if left { 1 } else { 2 }; // FacingChangeFire @4435-4613: the drawing's rotations
    }
    if s.driving {
        // DrivingTurn @10992-11118: IsTurrning, DrivingTurning 1 left / 2 right, RetriggerableDelay(0.2) -> @576.
        s.pawn.turning = true;
        s.pawn.driving_turning = if left { 1 } else { 2 };
        s.pawn.turn_delay = to_int32(latent_frames(0.2));
    }
}

/// `death`: the port does not update a dead pawn's AnimBP (Finn's rule).
pub fn death(w: &mut World, f: FighterId) {
    gizmo_w(w, f).pawn.slot_mix.slot_mix.clear();
}

/// Delay(0.2) @1095 -> 473-930: ArrowFire's Team parameter (the drawing's), FireArrowNullMesh hidden @708,
/// VisibilityDelay = true @768, ArrowFireCharged's Team parameter, FacingChanged -> FacingChangeFire bound @888,
/// ArrowCharge hidden @930 (Comp_FighterVisibilityManager.SetSubMeshVisibility: `set_sub_mesh`; both are tagged static
/// mesh components of the pawn).
fn visibility_delay_done(w: &mut World, f: FighterId) {
    set_sub_mesh(w, f, name!("FireArrowNullMesh"), false);
    gizmo_w(w, f).pawn.visibility_delay = true;
    set_sub_mesh(w, f, name!("ArrowCharge"), false);
}

/// `lastDashTarget`: BPI_GetLastDashTarget @0 (DashTarget = LastDashTarget): `GizmoState::last_dash_target`, or the car
/// in `attach.dash_car`, a destroyed car none: attach.rs's `last_dash_target` rule.
pub fn last_dash_target(w: &World, f: FighterId) -> Option<DashTarget> {
    match attach::last_dash_target(w, f)? {
        ActorRef::Fighter(t) => {
            let b = &w.fighter(t).body;
            Some(DashTarget { x: b.x, y: b.y, actor_y: Some(actor_y(w, t)) })
        }
        ActorRef::Projectile(c) => {
            let p = w.actors.projectiles.get(c)?;
            Some(DashTarget { x: p.x, y: p.y, actor_y: None })
        }
        _ => None,
    }
}

/// `allyAttach`: Comp_C023A_AllyAttach as the shared notifies see it (attach.rs `ally_attach`).
pub fn ally_attach(w: &World, f: FighterId) -> Option<AllyAttach> {
    attach::ally_attach(w, f)
}

fn slot_mix(w: &World, f: FighterId) -> Option<&SlotMix> {
    Some(&gizmo(w, f).pawn.slot_mix)
}

fn slot_mix_mut(w: &mut World, f: FighterId) -> Option<&mut SlotMix> {
    Some(&mut gizmo_w(w, f).pawn.slot_mix)
}

// ---------------------------------------------------------------- the HoldCharge binds (@1150-1278)
// Comp_HoldCharge's broadcasts (bow.rs) to the pawn's handlers, bound at BeginPlay.

/// ReachedMaxCharge @1343-1616: with VisibilityDelay, FireArrowNullMesh shown @1353, HasMaxArrow = true @1413,
/// MaxArrowVisuals(false) @1424, ArrowFire's Team parameter (the drawing's). Bound by bow.rs (`bindHoldCharge`).
pub fn hold_charge_reached_max(w: &mut World, f: FighterId) {
    if !gizmo(w, f).pawn.visibility_delay {
        return;
    }
    set_sub_mesh(w, f, name!("FireArrowNullMesh"), true);
    gizmo_w(w, f).pawn.has_max_arrow = true;
    max_arrow_visuals(w, f, false);
}

/// HoldChrageEnded @1714-1784: FireArrowNullMesh hidden, then with HasMaxArrow MaxArrowVisuals(true). Bound by bow.rs.
pub fn hold_charge_ended(w: &mut World, f: FighterId) {
    set_sub_mesh(w, f, name!("FireArrowNullMesh"), false);
    if gizmo(w, f).pawn.has_max_arrow {
        max_arrow_visuals(w, f, true);
    }
}

/// HoldChargeUsed @4027-4038: HasMaxArrow = false, MaxArrowVisuals(false). Bound by bow.rs.
pub fn hold_charge_used(w: &mut World, f: FighterId) {
    gizmo_w(w, f).pawn.has_max_arrow = false;
    max_arrow_visuals(w, f, false);
}

/// MaxArrowVisuals(On) @4054-4337: ArrowCharge shown when On and HasMaxArrow, ArrowFireCharged's Team parameter.
fn max_arrow_visuals(w: &mut World, f: FighterId, on: bool) {
    let show = on && gizmo(w, f).pawn.has_max_arrow;
    set_sub_mesh(w, f, name!("ArrowCharge"), show);
}

// ---------------------------------------------------------------- C023A_AnimBP's slot graph
// The graph's inputs (gizmo.json slotHolds.graph; CharHooks.slotInput), as BlueprintUpdateAnimation and the bound
// functions of the ubergraph give them: IsFalling = CharacterMovement.IsFalling() @2284 (not under
// Comp_C023A_AllyAttach's DisableMovement either); VelocityY = the owner's GetVelocity().Y @2391-2491 (sim x = -UE Y);
// FacingLeft (OnFacingChanged); AimDirection (BPI_SetAimDirection: the pose's aim direction), AimDirectionInverted =
// AimDirection x -1 @1927-1969; IsDriving (BPI_DrivingCar @10968: `GizmoState::driving`, car.rs's); AttachedToAlly
// (BPI_SetAttachedToAlly @11486, called by Comp_C023A_AllyAttach Attach to Actor @1276 / @4218: `attach.attached`).
// Unverified: AttachedToAlly is read from the component's IsAttached, not from the AnimBP's own copy the call writes; the
// two are taken to change together (Attach to Actor writes both).
fn slot_input(w: &World, f: FighterId, input: Name) -> Real {
    let vy = -w.fighter(f).body.vx; // VelocityY
    let s = gizmo(w, f);
    let p = &s.pawn;
    let b = |v: bool| if v { 1.0 } else { 0.0 };
    match w.names.str(input) {
        "IsDriving" => b(s.driving),
        "IsFalling" => b(is_falling(w, f)),
        "AimDirection" => w.fighter(f).pose.aim_direction,
        "AimDirectionInverted" => -w.fighter(f).pose.aim_direction,
        "AttachedToAlly" => b(s.attach.attached),
        // The Driving machine's rules (index 82: Moving 0, Turrning 1, Start 2, Stop 3, Idle 4).
        "IsTurrning" => b(p.turning),                             // Moving -> Turrning
        "AnimGraphNode_TransitionResult" => b(vy.abs() <= 100.0), // Moving -> Stop @1620-1691
        "AnimGraphNode_TransitionResult_2" => {
            // Turrning -> Stop @1409-1590
            b(graph_relevant_anim_time_remaining(w, f, 82, 1) == 0.0 && vy.abs() <= 100.0)
        }
        "AnimGraphNode_TransitionResult_3" => {
            // Turrning -> Moving @3860-3969
            b(vy.abs() > 100.0 && graph_relevant_anim_time_remaining(w, f, 82, 1) <= 0.1)
        }
        "AnimGraphNode_TransitionResult_4" => b(vy.abs() <= 100.0), // Start -> Stop @5269-5340
        "AnimGraphNode_TransitionResult_5" => {
            // Start -> Moving: Car_Start (player 90) at its end, moving
            b(graph_player_time_from_end(w, f, 90) == 0.0 && vy.abs() > 100.0)
        }
        "AnimGraphNode_TransitionResult_6" => {
            // Stop -> Idle: Car_Stop (player 92) at its end, still
            b(graph_player_time_from_end(w, f, 92) == 0.0 && vy.abs() <= 100.0)
        }
        "AnimGraphNode_TransitionResult_7" => b(vy.abs() > 100.0), // Idle -> Start
        "AnimGraphNode_BlendListByBool_6" => b(p.driving_turning == 1), // @1720-1754
        "AnimGraphNode_BlendListByBool_1" => b(!is_falling(w, f) && vy.abs() > 50.0), // @5681-5819: the bow walk list
        "AnimGraphNode_TwoWayBlend" => {
            // @6331-6499: BowWalkB when moving away from the facing
            b(if facing(w, f) < 0.0 { 0.0 > vy } else { vy > 0.0 })
        }
        "AnimGraphNode_LayeredBoneBlend" => 1.0, // BlendWeights[0], not bound: the class data's 1
        // @6694-6798's rule (AttachedToAlly and not Stop Overrides)
        "AnimGraphNode_LayeredBoneBlend_1" | "AnimGraphNode_LayeredBoneBlend_2" => {
            b(s.attach.attached && !p.stop_overrides)
        }
        _ => Real::NAN,
    }
}

/// The pose the sim takes whole at each LayeredBoneBlend (`slotLayer`; the per-bone mix is the drawing's):
/// LayeredBoneBlend (weight 1; blend: the aim slots under spine_a / face_att / bowHandle / ear_att) its blend pose, the
/// only one of its two that reaches the air aim slots (the base's falling branch is the lantern / linked layer);
/// LayeredBoneBlend_1 (blend: DefaultSlot over the spine and head) and _2 (blend: the lantern idle / linked layer over
/// the pelvis and legs) their base, which holds DefaultSlot (an attack's shapes) in both.
// Deviation: whole poses in place of the per-bone layers; a montage with tracks in both a bow walk slot and an aim slot
// gives the aim slot's shapes.
fn slot_layer(w: &World, _f: FighterId, node: Name) -> Real {
    match w.names.str(node) {
        "AnimGraphNode_LayeredBoneBlend" => 1.0,
        "AnimGraphNode_LayeredBoneBlend_1" | "AnimGraphNode_LayeredBoneBlend_2" => 0.0,
        _ => Real::NAN,
    }
}

// ---------------------------------------------------------------- the pawn API

/// `setComAngle(f, angle, on, blendTime, blendStartTime)`: BPI_Set_COM_Angle(Angle, On, BlendTime, BlendStartTime)
/// @2043: IsLocallyControlled -> Set COM Angle Server -> Multicast -> C023A_AnimBP's event (COM_Override_Angle
/// @11241, COM_Override_Alpha @11305 / @11462: `com_override` / `com_override_alpha`). Callers: the COM_Rotation entry
/// (`set_com_angle` hook), Comp_C023A_AllyAttach @2048 (attach.rs, on the AnimBP directly: the same call here). The
/// event @11227-11462: On -> COM_Override_Angle = Angle, COM_Override_Alpha = 1, COM_BlendStartTime = BlendStartTime +
/// GetGameTimeInSeconds (UWorld TimeSeconds: the fighter's clock), COM_BlendTime = BlendTime; Off -> COM_Override_Alpha
/// = 0 alone. ModifyBone_12 (@5848-6294) is the drawing's.
pub fn set_com_angle(w: &mut World, f: FighterId, angle: Real, on: bool, blend_time: Real, blend_start_time: Real) {
    let time = w.fighter(f).clock.time;
    let s = gizmo_w(w, f);
    if !on {
        s.com_override_alpha = 0.0;
        return;
    }
    s.com_override = angle;
    s.com_override_alpha = 1.0;
    s.pawn.com_blend_start = blend_start_time + time;
    s.pawn.com_blend_time = blend_time;
}

// ---------------------------------------------------------------- the small notifies
// Each calls the owner's AnimBP through BPI_C023A (the cast fails on any other AnimBP: nothing) or BPI_AnimBP.

/// AnimNotify_C023A_OverrideFallLean_C begin: BPI_OverrideFallLeanUmbrella(true) at Begin @121: OverrideFallLean
/// @11999 (ModifyBone_11's alpha @6536-6656, the drawing's).
fn override_fall_lean_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(s) = gizmo_mut(w, f) {
        s.pawn.override_fall_lean = true;
    }
}

/// AnimNotify_C023A_OverrideFallLean_C end: (false).
fn override_fall_lean_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(s) = gizmo_mut(w, f) {
        s.pawn.override_fall_lean = false;
    }
}

/// AnimNotify_C023A_StopBoneOverrides_C begin: BPI_StopBoneOverrides(true): Stop Overrides @12069 (the LayeredBoneBlend_1
/// / _2 weights above, the CopyBone / ModifyBone alphas @6694-7000).
fn stop_bone_overrides_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(s) = gizmo_mut(w, f) {
        s.pawn.stop_overrides = true;
    }
}

/// AnimNotify_C023A_StopBoneOverrides_C end: (false).
fn stop_bone_overrides_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(s) = gizmo_mut(w, f) {
        s.pawn.stop_overrides = false;
    }
}

/// AnimNotify_C023a_SetRolling_C begin: BPI_SetRolling(true): IsRolling @12089 (ModifyBone_1's alpha @3786-3823, the
/// drawing's).
fn set_rolling_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(s) = gizmo_mut(w, f) {
        s.pawn.rolling = true;
    }
}

/// AnimNotify_C023a_SetRolling_C end: (false).
fn set_rolling_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(s) = gizmo_mut(w, f) {
        s.pawn.rolling = false;
    }
}

/// AnimNotify_C023A_ResetAim_C begin: BPI_AnimBP.BPI_SetAimDirection(0.0) @121 (C023A_AnimBP @10696: AimDirection, then
/// AimDirectionInverted @1927), on the notify's MeshComp anim instance: a Disguise's second mesh when its montage runs
/// there (as Morty's notify), else the pawn's (the pose's aim direction, the generic value).
fn reset_aim_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if copy_running(w, f) {
        set_copy_aim(w, f, 0.0);
    } else {
        w.fighter_mut(f).pose.aim_direction = 0.0;
    }
}

/// A notify with a begin and an end.
fn both(begin: NotifyFn, end: NotifyFn) -> NotifyHandler {
    NotifyHandler { begin: Some(begin), end: Some(end), ..NotifyHandler::default() }
}

/// A fighter notify's begin or end.
type NotifyFn = fn(&mut World, FighterId, NotifyRef, i32);

/// This file's registry entries: the four notifies.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies.enter("AnimNotify_C023A_OverrideFallLean_C", both(override_fall_lean_begin, override_fall_lean_end))?;
    r.notifies
        .enter("AnimNotify_C023A_StopBoneOverrides_C", both(stop_bone_overrides_begin, stop_bone_overrides_end))?;
    r.notifies.enter("AnimNotify_C023a_SetRolling_C", both(set_rolling_begin, set_rolling_end))?;
    r.notifies.enter(
        "AnimNotify_C023A_ResetAim_C",
        NotifyHandler { begin: Some(reset_aim_begin), ..NotifyHandler::default() },
    )
}
