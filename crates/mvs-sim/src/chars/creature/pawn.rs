//! Pawn_Reindog's own logic (and mod.rs's conditions, notifies and hooks): BeginPlay
//! (matchReset), BPI_Cleanup (reset), ReceiveTick (the rtpc and the wall bounce's Delay), the capsule's
//! OnComponentHit and HACKCreatureWallBounceMulticast, the crouch's capsule (StartCrouch / EndCrouch),
//! SetReindogSpeedBoost, Creature_AnimBP's update (COM_Spin, Spin_Rotation, Is Hit Stunned, Is Respawning), its
//! ModifyBone spin on the box pose, StopRotation (OnMontageBlendingOut), the timers; the conditions TetherOut /
//! TetherNotOut, IsBuddy / IsNotBuddy, ReindogIce / NoReindogIce; the notifies AnimNotify_ReindogSpeedBoost and
//! AnimNotify_ReindogSpin.
//!
//! The pawn's SetDynamicSubtree calls (the bot injections of `mod.rs`) come with the bots' crate.

use super::state::{get_tether_info, reindog, reindog_of, reindog_w};
use crate::Real;
use crate::chars::CharState;
use crate::combat::buffs::{buff_of, has_buff, inst_view, set_buff_values, spawn_buff, update_states};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, NotifyRef};
use crate::fighter::FighterId;
use crate::fighter::anim::box_pose::turn_boxes;
use crate::fighter::anim::montages::start_montage;
use crate::fighter::common::latent_frames;
use crate::fighter::moves::{set_capsule_half_height, set_capsule_radius, set_capsule_size};
use crate::math::ue::finterp_constant_to;
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::{CharHooks, Condition, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use mvs_anim::montage::PROXY_NONE;
use mvs_data::value::{Props, Value};

const SPEED_BOOST: Name = name!("ReindogSpeedBoost_C");
const REINDOG_ICE: Name = name!("Buff_ReindogIce_C");
/// K2_OnStartCrouch @1503 / @1541 and VisualCrouch @1799 / @1837: radius, then half height.
const CROUCH_SIZE: Real = 100.0;
/// VisualCrouch @1970 / @2042: Mesh.RelativeLocation + (0, 0, 40).
const VISUAL_CROUCH_Z: Real = 40.0;
/// UnCrouch's test half height: current + 0.001 + adjust (native).
const UNCROUCH_TEST_GAP: f64 = 0.001;
/// Creature_AnimBP SpinSpeed (class default).
const SPIN_SPEED: Real = 200.0;
/// `SelectFloat(360.0, -360.0, false)` @1542.
const SPIN_TARGET: Real = -360.0;
/// `NormalizeToRange(HorizontalSpeed, 0.0, 1500.0)` @1623.
const SPIN_FULL_SPEED: Real = 1500.0;
/// SetIsRespawning's Delay @21988.
const RESPAWN_FLAG: Real = 0.1;

/// The hooks of this file in Reindog's CharHooks (`mod.rs`: matchReset, tick, timerTick, reset, capsuleHit,
/// crouchChanged, animUpdate, pose, montageBlendingOut).
pub fn hooks(h: &mut CharHooks) {
    h.match_reset = Some(match_reset);
    h.tick = Some(tick);
    h.timer_tick = Some(timer_tick);
    h.reset = Some(cleanup);
    h.capsule_hit = Some(capsule_hit);
    h.crouch_changed = Some(crouch_changed);
    h.anim_update = Some(anim_update);
    h.pose = Some(pose_shapes);
    h.montage_blending_out = Some(montage_blending_out);
}

// ---- BeginPlay, BPI_Cleanup

/// A match reset spawns a new pawn: a new state, then its BeginPlay (the World's reset calls this behind the
/// fighter's spawn; also the first spawn of a match). A file's joined matchReset runs after this one and finds the
/// new state.
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::Creature(super::ReindogState::default());
    begin_play(w, f);
}

/// ReceiveBeginPlay @30: the parent's, the AnimBP reference @45-87 (no sim effect), Original Capsule Half Height /
/// Radius = CapsuleComponent's CapsuleHalfHeight / CapsuleRadius @106 / @155 (the unscaled properties: the
/// template's, no graph has resized it yet), then @205.
fn begin_play(w: &mut World, f: FighterId) {
    let (half, radius) = (w.fighter(f).capsule.cap_half_u, w.fighter(f).capsule.cap_radius_u);
    let s = reindog_w(w, f);
    s.original_cap_half = half;
    s.original_cap_radius = radius;
    spawn_speed_boost(w, f);
}

/// @205: on authority, Comp_State_Manager.SpawnBuffActor(ReindogSpeedBoost_C, self, self, stack count 0, duration
/// 0.0, true, false) @235. Duration 0 sets no life timer (the class's BuffDuration 5 is not read); the buff is
/// destroyed at the owner's defeat (DestroyBuffWhenOwnerDefeated) and spawned again by BPI_Cleanup. A live one (a
/// Cleanup without a defeat before it) meets the shared spawn rules (`spawn_buff`).
fn spawn_speed_boost(w: &mut World, f: FighterId) {
    spawn_buff(w, f, SPEED_BOOST, 0.0, Some(0), Some(f), true, None);
}

/// ResetFighterLogic (CharHooks.reset): BPI_Cleanup @554 -> Pawn_Reindog @1794 -> @205, the spawn again (nothing
/// else: Is Held, Tether and the capsule are left). Then OnFighterReset @1106, which Creature_AnimBP binds to
/// SetIsRespawning (@2812-2835): Is Respawning = true @21977, Delay(0.1) @21988 -> @2031 false.
fn cleanup(w: &mut World, f: FighterId) {
    spawn_speed_boost(w, f);
    let s = reindog_w(w, f);
    s.is_respawning = true;
    s.respawn_delay = latent_frames(RESPAWN_FLAG);
}

// ---- ReceiveTick

/// ReceiveTick @924: the parent's, then Velocity.Z == 0 -> SetRTPCValue(cha_reindog_velocity, VelocityClamped, 50
/// ms, null) @1073, else SetRTPCValue(cha_reindog_velocity, 0.0, 100 ms, null) @1143 (that call's RTPC name string is
/// cha_bugs_velocity; the AkRtpc object, which the call takes first, is cha_reindog_velocity). The actor is null: the
/// global scope (owner -1). VelocityClamped is the parent's PF_Velocity_Ak of this tick. Then the actor's latent
/// actions (AActor::Tick after ReceiveTick, on the world's delta): the bounce Delay.
fn tick(w: &mut World, f: FighterId) {
    let fi = w.fighter(f);
    let still = fi.body.vy == 0.0;
    let value = if still { fi.body.vel_clamped } else { 0.0 };
    let interp_ms = if still { 50.0 } else { 100.0 };
    w.fighter_mut(f).events.push(Event::Rtpc { fighter: -1, name: name!("cha_reindog_velocity"), value, interp_ms });
    let s = reindog_w(w, f);
    if s.bounce_delay {
        s.bounce_delay = false;
        wall_bounce_effects(w, f);
    }
}

// ---- the HACK bounce montages

/// ComponentBoundEvent_1, the capsule's OnComponentHit (CharHooks.capsule_hit, at the end of BFBounce) @1425 ->
/// Delay(0.01) @1370 (latent_links: resumes @15, HACKCreatureWallBounceMulticast -> @1430
/// HACKCreatureWallBounceEffects). A second call while it is pending is ignored. The Delay is the pawn's own: made in
/// the CMC's move, ahead of the pawn's tick of the frame, it is counted in that tick (a Delay made on an object
/// before the object's own tick is counted in that tick, on the world's delta, 0.0167 > 0.01), so it resumes in this
/// frame's `tick` above.
// Unverified: a capsule hit raised after the pawn's tick of the frame would resume in the leftover pass of that
// frame (no entry at the tick) where the port takes the next tick; every try_bounce caller is inside the CMC tick.
fn capsule_hit(w: &mut World, f: FighterId) {
    reindog_w(w, f).bounce_delay = true;
}

/// HACKCreatureWallBounceEffects: IsBouncing 0 -> nothing @0-31; 3 or 4 -> PlayAnimMontage(Creature_Floored_Montage,
/// 1.0, None) @159; any other -> PlayAnimMontage(Creature_Wall_Bounce_Montage, 1.0, None) @223. IsBouncing is read at
/// the resume (BFHitPauseEnd clears it). ACharacter::PlayAnimMontage is a plain Montage_Play on the pawn's mesh, past
/// the branch manager: the montage replaces the attack slot's as an attack's does and BeginAttack is not set (as
/// LeBron's ball return). It plays in hitstun: the hit's Montage_Stop came before the bounce.
// Unverified: what else the port's start_montage does for an attack start rides along: the attack count, the charge
// values and the hit flag reset (none read by these two montages) and the player's ground / air section choice by
// `grounded` (both montages have the one section Default).
fn wall_bounce_effects(w: &mut World, f: FighterId) {
    let t = crate::combat::hit::launch::is_bouncing(w, f);
    if t == 0 {
        return;
    }
    let m = if t == 3 || t == 4 { name!("Creature_Floored_Montage") } else { name!("Creature_Wall_Bounce_Montage") };
    start_montage(w, f, m, &PROXY_NONE, None);
    w.fighter_mut(f).charge.begin_attack = false;
}

// ---- the crouch

/// The native Crouch / UnCrouch (UpdateCharacterStateBeforeMovement 142951a30 calls Crouch only while bIsCrouched is
/// clear; Crouch 1429286c0 and UnCrouch 14294f140) with the pawn's K2_OnStartCrouch / K2_OnEndCrouch last
/// (CharHooks.crouch_changed, from the movement tick where bIsCrouched changes). The port's crouch keeps `y` the
/// standing centre and derives the actor location (the native mesh raise cancelling the drop). Pawn_Reindog's own
/// resize moves the centre: from here on `y` is the small capsule's centre and `mesh_z` the Mesh's relative Z over
/// the CDO's (the native OnStartCrouch raise and VisualCrouch's 40), which the bone shapes and the view follow.
fn crouch_changed(w: &mut World, f: FighterId, crouched: bool) {
    if crouched {
        start_crouch(w, f);
    } else {
        end_crouch(w, f);
    }
}

/// Crouch, from Reindog's standing 145 / 175 (CrouchedHalfHeight 40): Target = max(R, 40) = 145 @step 2,
/// SetCapsuleSize(145, 145); Adjust = (175 - 145) x scale = 30; walking (bCrouchMaintainsBaseLocation) the capsule
/// moves down by it @step 4; OnStartCrouch: mesh Z = CDO + 30 @step 6, then K2_OnStartCrouch @1445: on authority
/// SetCapsuleRadius(100) @1503, SetCapsuleHalfHeight(100) @1541 (the centre kept: the bottom 45 over the floor), then
/// VisualCrouch @1799: the same two sizes, and with a PlayerController (@1875-1960) Mesh.RelativeLocation + (0, 0,
/// 40) by K2_SetRelativeLocation @2198. The native OnStartCrouch has just written the mesh Z absolutely (CDO mesh Z plus
/// the scaled adjust; UnCrouch: CDO mesh Z), so the 40 does not pile up over crouches. The walking capsule then
/// meets its floor at the next move: `cap_resized` then `resolve_resize` puts it at its walking height for half height
/// 100.
// Unverified: AdjustFloorHeight is not read in the exe; the capsule is 45 over the floor, at the edge of
// MaxStepHeight 45, where a FindFloor miss would start a fall.
// Unverified: the mesh raise under a size scale. The native write is CDO mesh Z + the adjust times the shape scale,
// then VisualCrouch adds 40 in the mesh's relative space; the port multiplies (adjust + 40) by the size scale. The
// same at scale 1.
// Unverified: the controller test. A human's pawn has a PlayerController where the game runs (offline, and the
// host); a bot's has an AIController (`bot_controlled`): no 40. On an online client the other player's pawn has no
// controller there, so there the build shows its mesh 40 lower; the port's sim is one for every client.
fn start_crouch(w: &mut World, f: FighterId) {
    let fi = w.fighter(f);
    let k = fi.body.size_scale;
    let base = crate::combat::hit::def_of(w, fi.base_ch);
    let target = max(fi.capsule.cap_radius_u, crate::math::narrow(base.crouched_half));
    // OnStartCrouch's HalfHeightAdjust (CDO half height - Target)
    let adjust = crate::math::narrow(base.cap_half - f64::from(target));
    let drop = (fi.capsule.cap_half_u - target) * k;
    let grounded = fi.body.on_floor;
    let bot = fi.controls.bot_controlled;
    if grounded {
        w.fighter_mut(f).body.y -= drop;
    }
    set_capsule_radius(w, f, CROUCH_SIZE);
    set_capsule_half_height(w, f, CROUCH_SIZE);
    w.fighter_mut(f).hit.mesh_z = (adjust + if bot { 0.0 } else { VISUAL_CROUCH_Z }) * k;
}

/// UnCrouch from 100 / 100: adjust = 175 - 100 = 75 (x scale); walking: the stand-up test (radius 100, half height
/// 175.001) at Z + 75.001, then a teleport there @step 4; in the air no move. Then SetCapsuleSize(CDO 145, 175)
/// @step 7, OnEndCrouch: mesh Z = CDO, then K2_OnEndCrouch @1594 -> @1241: on authority SetCapsuleRadius(Original
/// Capsule Radius) @1271, SetCapsuleHalfHeight(Original Capsule Half Height) @1313 (the values BeginPlay read: the
/// CDO's again), VisualCrouchOff @2389 (the same two sizes).
// Deviation: the encroachment tests are not made (the port's stand-up has none for any pawn): a Reindog crouched under
// a ceiling lower than his standing height stands up into it, where the build stays crouched and tries again each
// tick. The radius grows by 45 after the test without its own check in the build too.
// Deviation: the stand-up moves the centre up by adjust + 0.001 whenever he is grounded; the build moves it only
// under bCrouchMaintainsBaseLocation, which is set while Walking (native UnCrouch): in the knockback mode he stands
// up in place. A crouch ended in the knockback mode also takes the shared in-place stand of the movement tick first;
// its gap is the walking rest height's, which the resolve of the grown capsule replaces on the ground.
fn end_crouch(w: &mut World, f: FighterId) {
    let (orig_radius, orig_half) = {
        let s = reindog(w, f);
        (s.original_cap_radius, s.original_cap_half)
    };
    let fi = w.fighter(f);
    let k = fi.body.size_scale;
    let base = crate::combat::hit::def_of(w, fi.base_ch);
    let (base_half, base_radius) = (base.cap_half, base.cap_radius);
    let up = crate::math::narrow((base_half - f64::from(fi.capsule.cap_half_u)) * f64::from(k) + UNCROUCH_TEST_GAP);
    if fi.body.on_floor {
        w.fighter_mut(f).body.y += up;
    }
    set_capsule_size(w, f, crate::math::narrow(base_radius), crate::math::narrow(base_half));
    w.fighter_mut(f).hit.mesh_z = 0.0;
    set_capsule_radius(w, f, orig_radius);
    set_capsule_half_height(w, f, orig_half);
}

// ---- BPI_SetReindogSpeedBoost

/// @4932 -> @4672: BPI_CheckIfHasBuff(ReindogSpeedBoost_C) (the last live actor of the class), a cast, then its
/// SetActive(On): ReindogSpeedBoost ubergraph @10-381: when On != Active, Active = On, SpeedMod = SelectFloat(
/// SpeedBoost 1.4, 1.0, Active) @81-132 and the owner's Comp_State_Manager.UpdatePlayerStatus(null) @344
/// (`update_states`: SpeedModType Multiply). No authority test. Its callers: AnimNotify_ReindogSpeedBoost's begin
/// (true) and end (false).
pub fn set_reindog_speed_boost(w: &mut World, f: FighterId, on: bool) {
    if reindog_of(w, f).is_none() {
        return;
    }
    let Some(b) = buff_of(w, f, SPEED_BOOST) else { return };
    let d = inst_view(w, b);
    if d.flag("Active") == on {
        return;
    }
    // SpeedBoost: the class default (1.4); no graph writes it.
    let boost = d.num("SpeedBoost").unwrap_or(1.0);
    let mut v = Props::new();
    v.insert("Active".into(), Value::Bool(on));
    v.insert("SpeedMod".into(), Value::Num(if on { f64::from(boost) } else { 1.0 }));
    set_buff_values(w, b, &v);
    update_states(w, f);
}

// ---- Creature_AnimBP

/// BlueprintUpdateAnimation (after the montage update: CharHooks.anim_update with pre false), the sim-side inputs:
/// Is Hit Stunned = Comp_Pawn_HitstunManager.CheckHitstunActive @645-712; with COM_Spin @1528: Spin_Rotation =
/// FInterpTo_Constant(-360, DeltaTimeX, Lerp(IsFalling ? SpinSpeed : 0, SpinSpeed x 4, clamp(HorizontalSpeed /
/// 1500, 0, 1))), kept when strictly inside (-360, 360), else 0 @1887-1979; without it 0 @2007. IsFalling
/// (SetHorizontalAndVerticalSpeed @43-361): CharacterMovement IsFalling or the knockback's custom mode (6, 2);
/// HorizontalSpeed: |Velocity| in the plane (the port's |vx|). DeltaTimeX carries the hit pause's dilation.
/// FInterpTo_Constant @1823 is FMath::FInterpConstantTo: the target when the distance squared is under 1e-8, else a
/// step of speed x dt toward it; no speed test, so at speed 0 (on the ground at rest, or carried by the Fastball's
/// holder: Flying, vx 0) the value stays.
/// The writes the update sees: the notify's and StopRotation's of earlier frames (queued events dispatched after their
/// frame's update), applied ahead of the step; this frame's wait for the next update. The port dispatches a frame's
/// queued notifies before this call (the mesh tick's montages), or after it when the movement ticked the pose
/// (`cmc_pose_tick`, dispatched in the mesh tick): the frame stamp tells the two apart.
/// The skin Dynamics alphas, the beard and the AnimGraph's nodes are the view's.
fn anim_update(w: &mut World, f: FighterId, dilation: Real, pre: bool) {
    if pre {
        return;
    }
    let now = w.fighter(f).clock.tick;
    if reindog(w, f).cur_at < now {
        age_spin_writes(w, f);
    }
    let hit_stunned = crate::fighter::in_hitstun(w, f);
    let fi = w.fighter(f);
    let falling = fi.hit.kb_mode || (!fi.body.on_floor && !fi.moves.flying);
    let vx = fi.body.vx;
    let s = reindog_w(w, f);
    if s.stop_old {
        s.spin_rotation = 0.0;
        s.com_spin = false;
    }
    if s.spin_old >= 0 {
        s.com_spin = s.spin_old == 1;
    }
    s.spin_old = -1;
    s.stop_old = false;
    s.is_hit_stunned = hit_stunned;
    if !s.com_spin {
        s.spin_rotation = 0.0;
        return;
    }
    let a = min(max(vx.abs() / SPIN_FULL_SPEED, 0.0), 1.0);
    let lo = if falling { SPIN_SPEED } else { 0.0 };
    let hi = SPIN_SPEED * 4.0;
    let speed = lo + (a * (hi - lo));
    let v = finterp_constant_to(s.spin_rotation, SPIN_TARGET, DELTA_SECONDS * dilation, speed);
    s.spin_rotation = if v > -360.0 && v < 360.0 { v } else { 0.0 };
}

/// CharHooks.pose: the AnimGraph's ModifyBone_1 (fml_un_COM, rotation BMM_Replace in BCS_ComponentSpace <-
/// MakeRotator(0, Spin_Rotation, 0), alpha float(COM_Spin)) on the shapes, with the values this frame's update left
/// (what the evaluation reads; the view reads the same two). The node comes after the layer's own ModifyBone nodes
/// on the bone (Base_AnimBP's hit / fall pitch), so while COM_Spin is on its Replace is the bone's rotation whatever
/// they gave: the hook returns true and the box sampler leaves the hit pitch out; at Spin_Rotation 0 the bone's
/// rotation is still replaced (by the identity). The Replace is the turn by the pitch about the bone's point
/// (`turn_boxes`) where the clip's own component rotation of fml_un_COM is the identity: every clip of his
/// (Creature_Attack_Air_Ball, the spin window's, among them) but Creature_Floored (up to 27.8 deg) and
/// Creature_HR_Sleep / Creature_Air_HR_Sleep (0.8 deg), which can show under COM_Spin only in the one update
/// StopRotation's queued write lags behind a montage stopped into them.
// Unverified: those keep their own rotation under the turn here (the box tracks hold no component rotation to take
// off).
fn pose_shapes(w: &World, f: FighterId, s: &mut [f32]) -> bool {
    let Some(st) = reindog_of(w, f) else { return false };
    if !st.com_spin {
        return false;
    }
    turn_boxes(w, f, s, st.spin_rotation);
    true
}

/// StopRotation @21873, bound to the anim instance's OnMontageBlendingOut (BindAnimationEvents @21828-21851): any
/// montage's blend-out: Spin_Rotation = 0, COM_Spin = false, seen by the next frame's update (`anim_update`).
fn montage_blending_out(w: &mut World, f: FighterId, _montage: Name) {
    spin_write(w, f, 0, true);
}

/// AnimNotify_ReindogSpin's begin (true) and end (false): COM_Spin, seen as StopRotation's is.
fn set_com_spin(w: &mut World, f: FighterId, on: bool) {
    spin_write(w, f, i32::from(on), false);
}

fn spin_write(w: &mut World, f: FighterId, spin: i32, stop: bool) {
    if reindog_of(w, f).is_none() {
        return;
    }
    let now = w.fighter(f).clock.tick;
    if reindog(w, f).cur_at != now {
        age_spin_writes(w, f);
        reindog_w(w, f).cur_at = now;
    }
    let s = reindog_w(w, f);
    if stop {
        s.stop_cur = true;
    }
    s.spin_cur = spin;
}

/// This frame's writes become earlier frames' (later over earlier: a stop zeroes the spin whatever came before it,
/// and the last COM_Spin write wins).
fn age_spin_writes(w: &mut World, f: FighterId) {
    let s = reindog_w(w, f);
    if s.stop_cur {
        s.stop_old = true;
    }
    if s.spin_cur >= 0 {
        s.spin_old = s.spin_cur;
    }
    s.spin_cur = -1;
    s.stop_cur = false;
}

/// The anim instance's Delay of SetIsRespawning, counted by the world's leftover latent pass (an anim instance's
/// latent actions have no object tick), from the frame of the reset.
// Unverified: the phase. A reset after this frame's leftover pass (in the timer pass) is counted from here too.
fn timer_tick(w: &mut World, f: FighterId) {
    let s = reindog_w(w, f);
    if s.respawn_delay > 0.0 {
        s.respawn_delay -= 1.0;
        if s.respawn_delay == 0.0 {
            s.is_respawning = false;
        }
    }
}

// ---- the conditions and the notifies

/// TetherOutCondition: the actor's BPI_GetTetherInfo @79 (a failed cast to BPI_Reindog: false, false @196-218):
/// Out = IsTetherOut AND IsTetherAttachedToAlly @134.
fn tether_out(w: &mut World, f: FighterId) -> bool {
    let t = get_tether_info(w, f);
    t.out && t.attached_to_ally
}

/// TetherNotOutCondition (@187-209 for a failed cast): NotOut = NOT IsTetherOut @134.
fn tether_not_out(w: &mut World, f: FighterId) -> bool {
    !get_tether_info(w, f).out
}

/// Condition_IsBuddyReindog @79: BPI_CheckIfHasBuff(Mutator_ReindogBuddyAI_C); Condition_IsNotBuddyReindog its
/// negation. The mutator is in no data file of the port and no package of either dump spawns it: IsBuddy is false and
/// IsNotBuddy true (the tether throws take TetherProjectile, never SuperSaveTetherProjectile).
fn is_buddy(_w: &mut World, _f: FighterId) -> bool {
    false
}

fn is_not_buddy(_w: &mut World, _f: FighterId) -> bool {
    true
}

/// Condition_ReindogIce @79: BPI_CheckIfHasBuff(Buff_ReindogIce_C); Condition_NoReindogIce (@0-58) its negation. Only
/// Buff_Arcade_Secret_Boss spawns the buff (an arcade round's Reign Dog). The two Firewall montages'
/// AnimNotify_ConditionalSpawner pairs (Firebolt_C / Firebolt_Child_C) are the only notifies naming either condition,
/// both NoReindogIce: under the ice buff the Firewall special plays its montage and spawns nothing, as the build's
/// data has it (no spawner names Condition_ReindogIce, and Icebolt is named by no montage or graph).
fn reindog_ice(w: &mut World, f: FighterId) -> bool {
    has_buff(w, f, REINDOG_ICE)
}

fn no_reindog_ice(w: &mut World, f: FighterId) -> bool {
    !has_buff(w, f, REINDOG_ICE)
}

/// AnimNotify_ReindogSpeedBoost: Received_NotifyBegin @121 BPI_SetReindogSpeedBoost(true), Received_NotifyEnd @121
/// (false), on a BPI_Reindog owner (MeshComp.GetOwner; another pawn, a Disguise's copy of the montage, does
/// nothing). Only Reindog_AllyItem_Montage has it.
fn speed_boost_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    set_reindog_speed_boost(w, f, true);
}

fn speed_boost_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    set_reindog_speed_boost(w, f, false);
}

/// AnimNotify_ReindogSpin: Received_NotifyBegin @121 COM_Spin = true, Received_NotifyEnd @121 false, on the mesh's
/// anim instance when it is a Creature_AnimBP (else nothing). Only Reindog_AllyItem_Montage has it. The next frame's
/// update sees it (`anim_update`).
fn spin_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    set_com_spin(w, f, true);
}

fn spin_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    set_com_spin(w, f, false);
}

/// This file's registry entries (the six conditions, the two notifies, ReindogSpeedBoost's graph).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.conditions.enter("TetherOutCondition_C", Condition::Fn(tether_out))?;
    r.conditions.enter("TetherNotOutCondition_C", Condition::Fn(tether_not_out))?;
    r.conditions.enter("Condition_IsBuddyReindog_C", Condition::Fn(is_buddy))?;
    r.conditions.enter("Condition_IsNotBuddyReindog_C", Condition::Fn(is_not_buddy))?;
    r.conditions.enter("Condition_ReindogIce_C", Condition::Fn(reindog_ice))?;
    r.conditions.enter("Condition_NoReindogIce_C", Condition::Fn(no_reindog_ice))?;
    let pair = |b, e| NotifyHandler { begin: Some(b), end: Some(e), ..NotifyHandler::default() };
    r.notifies.enter("AnimNotify_ReindogSpeedBoost_C", pair(speed_boost_begin, speed_boost_end))?;
    r.notifies.enter("AnimNotify_ReindogSpin_C", pair(spin_begin, spin_end))?;
    r.buff_graphs_ported.enter("ReindogSpeedBoost_C", ())?;
    Ok(())
}
