//! The pawn core of Pawn_C023B: BeginPlay's binds, the gun handlers,
//! LastDecay, Set COM Angle, BPI_C023B, AnimNotify_C023b_Com_Fall_Override and C023B_AnimBP's sim part; its hooks
//! (`pawnHooks`).
//!
//! Offsets are Pawn_C023B's ubergraph's. BeginPlay's dynamic subtrees and binds @971-1945 (the edge binds On Turn
//! Corner @1408, On Attach To Edge @1858, On Detach From Edge @1922 stay unbound in effect), the gun handlers
//! GunAmmoUpdate @2020-2207 / GunCooldownUpdate @2273-2458, LastDecay @2538-2649, Set COM Angle Server / Multicast
//! (BPI_C023B_Set_COM_Angle @526).
//!
//! Ability slots: stripe.json slot 0 (saw) MaxCount 3, StartCount 3, CountCooldown 12, Cooldown 1; slot 1 (gun)
//! MaxCount 3, StartCount 3, CountCooldown 14, CountRefreshedPerCooldown 3, Cooldown 0; slot 2 (dynamite) Cooldown 12.
//! The gun: every shot (AbilityAmmoUpdated with NewAmmo != 3, not Increase) calls SetAmmoCooldownOnAuthority(1, 14,
//! 14), whose SetAmmoCooldown clears and restarts the container's AmmoCooldownTimer (AbilityCooldownContainer
//! HandleCooldownByReference @0: K2_ClearAndInvalidateTimerHandle), so the three shots come back together 14 s after
//! the LAST shot spent, not after the first. The container's own ammo cooldown of TriggerAbilityTimers is not started
//! beside it: the pawn's call has set AmmoOnCooldown by then (`fighter::control` use_ability: ammoUpdated @507 ahead of
//! the ammo timer's test).
//!
//! Not here: the AI (BPI_ReadyForAI's subtrees: the bots' crate), HeadScale / OnFacingChanged / ToggleSubmeshes of the
//! AnimBP (the view's: the submesh visibility, ChainsawVisibilityMI, is the curve show_Chainsaw's, @11077, a view
//! value).

use super::state::{stripe, stripe_mut, stripe_of, stripe_w};
use crate::Real;
use crate::actors::ActorRef;
use crate::consts::DELTA_SECONDS;
use crate::events::NotifyRef;
use crate::fighter::anim::box_pose::turn_boxes;
use crate::fighter::anim::update::anim_set_switched;
use crate::fighter::anim::{SetRef, SlotMix};
use crate::fighter::control::{set_ability_cooldown_on_authority, set_ammo_cooldown_on_authority, update_ability_ammo};
use crate::fighter::match_side::sfx;
use crate::fighter::moves::is_falling;
use crate::fighter::{FighterId, actor_y, facing};
use crate::math::ue::{
    SMALL_NUMBER, deg_atan2, finterp_constant_to, finterp_to, inv_sqrt, quat_rotator, rot_from_x, rotator_quaternion,
    safe_normal,
};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::props::flag;
use crate::registry::{CharHooks, NotifyHandler, Registry, RegistryError};
use crate::world::World;

// ---------------------------------------------------------------- the hooks

/// `pawnHooks`' hooks that are not joined with another file's (`setComAngle`, `abilityAmmoUpdated`,
/// `abilityCooldownUpdated`, `attackDecayInserted`, `hitPauseTimeChanged`, `pose`, `slotInput`, `slotMix`, `animSet`):
/// `animUpdate` and `death` are composed in mod.rs.
pub fn hooks(h: &mut CharHooks) {
    h.set_com_angle = Some(set_com_angle);
    h.ability_ammo_updated = Some(gun_ammo_update);
    h.ability_cooldown_updated = Some(gun_cooldown_update);
    h.attack_decay_inserted = Some(attack_decay_inserted);
    h.hit_pause_time_changed = Some(hit_pause_time_changed);
    h.pose = Some(pose_shapes);
    h.slot_input = Some(slot_input);
    h.slot_mix = Some(slot_mix);
    h.slot_mix_mut = Some(slot_mix_mut);
    h.anim_set = Some(anim_set);
}

/// BPI_InsertAttackDecay @2538: the parent's insert, then AttackDecayArray[0] -> UpdateLastDecayMulticast ->
/// LastDecay @2649 (offline the multicast runs at once). Read by C023B_ChainsawAttackHitCondition (dash.rs). The pawn
/// resets no LastDecay: a death's cleared AttackDecayArray leaves it.
fn attack_decay_inserted(w: &mut World, f: FighterId) {
    let d = w.fighter(f).decay.attack_decay.first().copied().unwrap_or(Name::NONE);
    stripe_w(w, f).last_decay = d;
}

/// C023B_AnimBP AddPauseTime @12156, bound to Comp_HitPause's OnHitPauseTimeChanged at the anim instance's init
/// (@10525-10623): COM_BlendStartTime + HitPauseTime, so the COM blend waits out the pause (GetGameTimeInSeconds runs
/// on).
fn hit_pause_time_changed(w: &mut World, f: FighterId, sec: Real) {
    let st = &mut stripe_w(w, f).pawn;
    st.com_blend_start += sec;
}

/// OverrideKB's BlendListByBool: true the LinkedAnimGraph of Base_AnimBP_AltKB_C023B (`anim_update` below).
fn anim_set(w: &World, f: FighterId) -> SetRef {
    if stripe(w, f).pawn.ev_kb_override { SetRef::Named(name!("altKB")) } else { SetRef::Base }
}

fn slot_mix(w: &World, f: FighterId) -> Option<&SlotMix> {
    Some(&stripe(w, f).pawn.slot_mix)
}

fn slot_mix_mut(w: &mut World, f: FighterId) -> Option<&mut SlotMix> {
    stripe_mut(w, f).map(|s| &mut s.pawn.slot_mix)
}

/// pawn.rs's part of the joined death: the port does not update a dead pawn's AnimBP (Finn's rule).
pub fn death(w: &mut World, f: FighterId) {
    stripe_w(w, f).pawn.slot_mix.slot_mix.clear();
}

// ---------------------------------------------------------------- the pawn API

/// BPI_C023B_Set_COM_Angle(Angle, On, BlendTime, BlendStartTime) @619 -> IsLocallyControlled @526 -> Set COM Angle
/// Server @272 -> Multicast @327 -> C023B_AnimBP's BPI_C023B_Set_COM_Angle (@11789; offline all at once): On false
/// sets COM_Override_Alpha 0 alone @11765; On true COM_Override_Angle = Angle @11803, COM_Override_Alpha = 1 @11867,
/// COM_BlendStartTime = BlendStartTime + GetGameTimeInSeconds @11969, COM_BlendTime = BlendTime @11996. Caller: the
/// COM_Rotation entry (`chars::shared::gizmo_stripe_notifies`, CharHooks `set_com_angle`).
fn set_com_angle(w: &mut World, f: FighterId, angle: Real, on: bool, blend_time: Real, blend_start_time: Real) {
    let now = w.fighter(f).clock.time;
    let Some(s) = stripe_mut(w, f) else { return };
    let st = &mut s.pawn;
    if !on {
        st.com_alpha = 0.0;
        return;
    }
    st.com_angle = angle;
    st.com_alpha = 1.0;
    st.com_blend_start = blend_start_time + now;
    st.com_blend_time = blend_time;
}

/// GunAmmoUpdate (AbilityAmmoUpdated bound @1472): Index 1, NewAmmo != 3 and not Increase (@2020-2193) ->
/// SetAmmoCooldownOnAuthority(1, 14.0, 14.0) @2207 (the header).
fn gun_ammo_update(w: &mut World, f: FighterId, slot: i32, ammo: Real, increase: bool) {
    if slot != 1 || ammo == 3.0 || increase {
        return;
    }
    set_ammo_cooldown_on_authority(w, f, 1, 14.0, 14.0);
}

/// GunCooldownUpdate (AbilityCooldownUpdated bound @1536): Index 1 and CooldownComplete (@2273-2345) ->
/// SetAbilityCooldownOnAuthority(1, 0, 0, false) @2359, UpdateAbilityAmmo(1, 3, false) @2411, PF_Post_Event
/// sfx_cha_C023b_gun_reload on the pawn @2458. The gun's Cooldown is 0, so its ability cooldown never runs and this
/// waits for a buff or a perk that starts one.
fn gun_cooldown_update(w: &mut World, f: FighterId, slot: i32, complete: bool) {
    if slot != 1 || !complete {
        return;
    }
    set_ability_cooldown_on_authority(w, f, 1, 0.0, 0.0, false);
    update_ability_ammo(w, f, 1, 3.0, false);
    sfx(w, f, Some(name!("sfx_cha_C023b_gun_reload")));
}

// ---------------------------------------------------------------- AnimNotify_C023b_Com_Fall_Override

/// Received_NotifyBegin / End: with ChainSaw (class default false) the anim instance's BPI_ChainSawOverride(true /
/// false) @135 (SetOverrideChainsawMulticast @12231 -> ChainSawRot = 0 @12255, ChainSawOverride = On @12279), else
/// BPI_Override_Fall_COM(true / false) @310 (COM_Fall_Override_On = On). The instance implements BPI_C023B
/// (C023B_AnimBP); both return false. Uses: C023B_Attack_Skateboard_Montage 0.521-1.5 s, C023B_ChainSaw_Montage
/// 0.742-2.301 s (ChainSaw), both queued.
fn com_fall_override(w: &mut World, f: FighterId, n: NotifyRef, on: bool) {
    let chain_saw = flag(crate::fighter::match_side::notify_props(w, f, n), "ChainSaw");
    let Some(s) = stripe_mut(w, f) else { return };
    if chain_saw {
        s.pawn.chain_saw_rot = 0.0;
        s.pawn.chain_saw_on = on;
    } else {
        s.pawn.fall_on = on;
    }
}

fn com_fall_override_begin(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    com_fall_override(w, f, n, true);
}

fn com_fall_override_end(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    com_fall_override(w, f, n, false);
}

/// This file's registry entries (AnimNotify_C023b_Com_Fall_Override_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies.enter(
        "AnimNotify_C023b_Com_Fall_Override_C",
        NotifyHandler {
            begin: Some(com_fall_override_begin),
            end: Some(com_fall_override_end),
            ..NotifyHandler::default()
        },
    )
}

// ---------------------------------------------------------------- C023B_AnimBP: the COM nodes on the shapes

/// pawn.rs's part of the joined animUpdate (before gun.rs's): the anim instance's update, at `pre`:
/// BlueprintUpdateAnimation's flows (@7413-7438, in the order they run): IsFalling = CharacterMovement IsFalling()
/// @1635, VelocityZ = GetVelocity().Z @7727, then @9236-9510: with COM_Fall_Override_On, COM_FallOverride =
/// FMin(FInterpTo(COM_FallOverride, IsFalling ? FMin(|VelocityZ|, 30) : 0, DeltaTimeX, 15), 30), else 0; @9534-9911:
/// ChainSawRot = FInterpTo_Constant(ChainSawRot, FindLookAtRotation(location, location + Velocity).Pitch, DeltaTimeX,
/// 400) (SelectVector(EdgeDirection, Velocity, WallRiding) with WallRiding false: BPI_ToggleWallRide's caller is the
/// edge bind that stays unbound), on every update. Then the graph's exposed inputs (@1190 / @1347, @6646-7092,
/// @7154-7325) as this update leaves them: the values the evaluation reads (the `ev_*` fields). Every writer of these
/// variables is an event of a queued notify (COM_Rotation, Com_Fall_Override), dispatched after the build's update of
/// its frame: taken at `pre`, ahead of the port's dispatch, the update sees the writes of earlier frames alone, as the
/// build's does.
// Deviation: the velocity and IsFalling are read at the mesh tick's start (the build's update reads them in its own
// tick).
pub fn anim_update(w: &mut World, f: FighterId, dilation: Real, pre: bool) {
    if !pre || stripe_of(w, f).is_none() {
        return;
    }
    // OverrideKB (BPI_C023B_SetKnockBackOverride: Comp_C023B_Bomb ExplosionLaunch @2345 true, TurnOffHitBoxes @330
    // false: StripeBombState.kb_override) as this update reads it: BlendListByBool (BlendTime 0 / 0) shows the
    // LinkedAnimGraph of Base_AnimBP_AltKB_C023B (LinkAnimGraphByTag(MovementAndHitReact) @81) on true, the
    // LinkedAnimLayer of Base_AnimBP_C023B (LinkAnimClassLayers @62) on false. The two sets differ in the fly poses
    // (Anim_HitReact_FlyBack / FlyForward and both intros: C023B_Special_Spiral), the alternate idle (C023B_Idle_01),
    // AlternateIdle_TimeDelay and RunRateEffectsLandRun (unset: Base_AnimBP's). A change switches the anim set
    // (`anim_set_switched`: the instance shown anew from its entry states).
    // Unverified: the switch is taken at the update after the write (the writers run between updates: the launch in
    // a notify's dispatch or the bomb's timer, TurnOffHitBoxes at HitStunEnded); a write inside an update's montage
    // advance would show in that update's graph in the build. The node's own update under BlendListByBool_1
    // (IsAttached) is not followed (IsAttached is never true here: slot_input).
    let want = stripe(w, f).bomb.kb_override;
    if want != stripe(w, f).pawn.ev_kb_override {
        let from = crate::fighter::anim_set(w, f);
        stripe_w(w, f).pawn.ev_kb_override = want;
        anim_set_switched(w, f, from);
    }
    let dt = DELTA_SECONDS * dilation;
    // IsFalling: CharacterMovement IsFalling() @1635.
    let falling = is_falling(w, f);
    let (vx, vy, now, left) = {
        let fi = w.fighter(f);
        (fi.body.vx, fi.body.vy, fi.clock.time, facing(w, f) < 0.0)
    };
    let st = &mut stripe_w(w, f).pawn;
    if st.fall_on {
        let target = if falling { min(vy.abs(), 30.0) } else { 0.0 };
        st.com_fall = min(finterp_to(st.com_fall, target, dt, 15.0), 30.0);
    } else {
        st.com_fall = 0.0;
    }
    // FindLookAtRotation (142f6cad0): MakeFromX(target - start), FMatrix::Rotator's pitch; the play plane has X 0.
    let pitch = rot_from_x([0.0, vx, vy])[0];
    st.chain_saw_rot = finterp_constant_to(st.chain_saw_rot, pitch, dt, 400.0);
    // @6646-7092: ModifyBone_9 Alpha = FMin(COM_Override_Alpha, Lerp(1, 0, FClamp(NormalizeToRange(now -
    // COM_BlendStartTime, 0, COM_BlendTime), 0, 1))), its pitch (FacingLeft ? -1 : 1) x COM_Override_Angle.
    // NormalizeToRange over an empty range (BlendTime 0: no use in the data) is 0 below the start, else 1
    // (Unverified: stock).
    let since = now - st.com_blend_start;
    let k = if st.com_blend_time == 0.0 { if since < 0.0 { 0.0 } else { 1.0 } } else { since / st.com_blend_time };
    let blend = 1.0 - min(max(k, 0.0), 1.0);
    st.ev_com_alpha = min(st.com_alpha, blend);
    st.ev_com_pitch = if left { -st.com_angle } else { st.com_angle };
    st.ev_fall_on = st.fall_on;
    st.ev_fall = st.com_fall;
    st.ev_saw_on = st.chain_saw_on;
    st.ev_saw_rot = st.chain_saw_rot;
}

/// CharHooks.pose: C023B_AnimBP's ModifyBone nodes on fml_un_COM, which sit above the slots (the montage's pose and
/// Base_AnimBP's hit pitch below them), in the order the ComponentPose chain runs them:
/// - ModifyBone_7: rotation Replace in parent-bone space, (0, COM_FallOverride, 0), alpha float(COM_Fall_Override_On);
/// - ModifyBone_9: rotation Replace in parent-bone space, (0, +-COM_Override_Angle, 0), alpha min(...) (`anim_update`);
/// - ModifyBone_5: rotation Replace in world space, (0, ChainSawRot, FacingLeft ? 90 : -90), alpha
///   float(ChainSawOverride).
///
/// fml_un_COM's parent is Root, and neither turns in any of his 159 clips (component rotation the identity at every
/// frame: checked over the .mva files), so a parent-space Replace is the component-space one and the clip's own COM
/// rotation is the identity under each: the turn of the shapes on the bone about its point (`turn_boxes`, the
/// rotator's sense). A Replace at 0 < alpha < 1 blends from the pose below it in the bone's local space on the
/// shorter arc, normalised (FAnimNode_ModifyBone): from the hit pitch or the fall pitch. ModifyBone_5's yaw is the
/// actor's own facing yaw (facing right -90, the play plane's +x), so its world Replace is the pitch ChainSawRot about
/// the facing's axis.
// Unverified: the world-space Replace read as the facing-relative pitch (no mirror node in C023B_AnimBP's chain is
// assumed to change it); the pitch's sense under the facing as the AnimBP's own SelectFloat gives it.
// While none is on the hook leaves the shapes to the box sampler (with the slot graph's mix, `slot_mix`).
fn pose_shapes(w: &World, f: FighterId, s: &mut [f32]) -> bool {
    let Some(sp) = stripe_of(w, f) else { return false };
    let st = &sp.pawn;
    let a = st.ev_com_alpha;
    let com = a > 1e-5; // not evaluated at alpha <= 1e-5
    if !st.ev_fall_on && !com && !st.ev_saw_on {
        return false;
    }
    let mut pitch = if st.ev_fall_on { st.ev_fall } else { crate::fighter::anim::hit_pitch(w, f).unwrap_or(0.0) };
    if com {
        pitch = if a >= 0.99999 { st.ev_com_pitch } else { nlerp_pitch(pitch, st.ev_com_pitch, a) };
    }
    if st.ev_saw_on {
        pitch = st.ev_saw_rot;
    }
    if pitch != 0.0 {
        turn_boxes(w, f, s, pitch);
    }
    true
}

/// Two pitches as quaternions (FRotator::Quaternion), blended as FQuat's shortest-path nlerp at alpha (the second
/// negated when their dot is below 0), normalised (InvSqrt): the pitch of the result (FQuat::Rotator), in degrees.
fn nlerp_pitch(from: Real, to: Real, a: Real) -> Real {
    let q0 = rotator_quaternion([from, 0.0, 0.0]);
    let mut q1 = rotator_quaternion([to, 0.0, 0.0]);
    if ((q0[0] * q1[0] + q0[1] * q1[1]) + q0[2] * q1[2]) + q0[3] * q1[3] < 0.0 {
        q1 = q1.map(|v| -v);
    }
    let b = 1.0 - a;
    let q: [Real; 4] = std::array::from_fn(|k| b * q0[k] + a * q1[k]);
    let s = inv_sqrt(((q[0] * q[0] + q[1] * q[1]) + q[2] * q[2]) + q[3] * q[3]);
    quat_rotator(q.map(|v| v * s))[0]
}

// ---------------------------------------------------------------- C023B_AnimBP's slot graph

/// The graph's inputs (stripe.json slotHolds.graph; CharHooks.slotInput), as the AnimBP holds them at this update:
///   IsFalling: CharacterMovement IsFalling() @1635 (`anim_update`'s reading).
///   AimDirection: BPI_SetAimDirection @10665 (the pose's `aim_direction`: the ProjectileAimer of the air saw throw
///     writes it); AimDirectionInverted = AimDirection x -1 @1655-1697. The BaseAim* slots of
///     C023B_Air_Saw_Throw_Montage under TwoWayBlend_9 / _8.
///   GunAimDirection, GunAimDirectionInverted, AimBack: BlueprintUpdateAnimation @7755-9220 ([`gun_aim`]). The Aim*
///     and AimBack* slots of the four gun montages under TwoWayBlend_11 / _10 (air), _13 / _12 (ground), _2 / _1
///     (back, air), _5 / _4 (back, ground); AnimGraphNode_TwoWayBlend and _3 (front / back) take float(AimBack) from
///     their bound handlers @6004-6041 / @6078-6115.
///   ChainAimDirection, ChainAimDirectionInverted: no writer in the build (xref writes: none), so the class default
///     0: the root's TwoWayBlend_7 / _6 show ChainLeanBase alone (ChainLeanUp / ChainLeanDown never show).
///   IsAttached: BPI_C023B_SetAttached @11740, whose one caller is Comp_C023B_EnemyAttach's Attach to Actor, on the
///     dormant path: false, so BlendListByBool_1 never shows C023B_Special_Bite_Loop.
///   OverrideKB: the value `anim_update` took (`ev_kb_override`); both poses of its list are the locomotion in the
///     sim (the anim set the `anim_set` hook names).
/// The alphas are float inputs with the identity scale and bias (the CDO's AlphaScaleBias); the walk clamps them.
// Deviation: as `fighter::anim::slot_mix::graph_slots`, the posed tracks are mixed linearly in the shapes and the
// drawn pose.
fn slot_input(w: &World, f: FighterId, n: Name) -> Real {
    if n == name!("IsFalling") {
        if is_falling(w, f) { 1.0 } else { 0.0 }
    } else if n == name!("AimDirection") {
        w.fighter(f).pose.aim_direction
    } else if n == name!("AimDirectionInverted") {
        -w.fighter(f).pose.aim_direction
    } else if n == name!("GunAimDirection") {
        gun_aim(w, f).0
    } else if n == name!("GunAimDirectionInverted") {
        -gun_aim(w, f).0
    } else if n == name!("AnimGraphNode_TwoWayBlend") || n == name!("AnimGraphNode_TwoWayBlend_3") {
        gun_aim(w, f).1
    } else if n == name!("ChainAimDirection") || n == name!("ChainAimDirectionInverted") || n == name!("IsAttached") {
        0.0
    } else if n == name!("OverrideKB") {
        if stripe(w, f).pawn.ev_kb_override { 1.0 } else { 0.0 }
    } else {
        Real::NAN
    }
}

/// `gunAim`: BlueprintUpdateAnimation's aim part (@7755-9220), read where the graph is updated (the walk's inputs) and
/// by gun.rs's anim update (`StripeState::aim_direction` / `aim_back`, the view's): UpdateAim (true on the CDO) and
/// IsValid(AimActor) (the TargetTracker's target, BPI_C023B_UpdateAimTarget @11701: `StripeState::target`;
/// HasAimTarget is written with it): AimBack = FacingLeft ? P.Y >= A.Y : P.Y < A.Y (P the pawn's location, A the aim
/// actor's; UE Y = -sim x: the target behind him); u = GetDirectionUnitVector(P, A); the angle DegAtan2(u.Z, u.Y)
/// when (FacingLeft && !AimBack) || (!FacingLeft && AimBack), else DegAtan2(u.Z, -u.Y); GunAimDirection =
/// NormalizeToRange(angle, -90, 90) x 2 - 1 (no clamp: the walk clamps the alphas). No valid aim actor:
/// GunAimDirection 0 and AimBack false @9163-9209. Returns (`dir`, `back`).
pub fn gun_aim(w: &World, f: FighterId) -> (Real, Real) {
    let Some(t) = stripe(w, f).target else { return (0.0, 0.0) };
    let (tx, ty) = match t {
        ActorRef::Fighter(id) => (w.fighter(id).body.x, actor_y(w, id)),
        other => match crate::actors::world_actor::actor_place(w, other) {
            Some((x, y, _)) => (x, y),
            None => (0.0, 0.0),
        },
    };
    let left = facing(w, f) < 0.0;
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let (p_y, a_y) = (-fx, -tx);
    let back = if left { p_y >= a_y } else { p_y < a_y };
    let (d_y, d_z) = (a_y - p_y, ty - fy);
    let [_, u_y, u_z] = safe_normal([0.0, d_y, d_z], SMALL_NUMBER);
    let ang = if (left && !back) || (!left && back) { deg_atan2(u_z, u_y) } else { deg_atan2(u_z, -u_y) };
    ((((ang + 90.0) / 180.0) * 2.0 - 1.0), if back { 1.0 } else { 0.0 })
}
