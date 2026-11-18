//! Pawn_WonderWomanV3's own logic: the conditions WWAllyGuarded and
//! WWHasLassoGrappleCondition, the notifies AnimNotify_WW_HurtBox and AnimNotify_WonderWoman_BracerCharge, and the
//! hooks: Comp_BracerCharge's tick (componentTick), the hair sub meshes (init, turned), the AnimBP's shield walk
//! (animUpdate) and the lasso aim slots (poseUpdated: slotCompose), death, the meter's armorBreak and
//! blockedProjectile, matchReset and reset. `slotMix` is the state's (state.rs).

use super::state::{
    BRACER_CHARGE_MAX, METER_PER_ARMOR_HIT, METER_PER_PROJECTILE, add_meter, clear_meter, wonder_woman,
    wonder_woman_mut, wonder_woman_w,
};
use crate::Real;
use crate::combat::buffs::has_buff;
use crate::consts::{DELTA_SECONDS, DT};
use crate::events::NotifyRef;
use crate::fighter::anim::grouped::active_instance;
use crate::fighter::anim::montages::instance_data;
use crate::fighter::anim::slot_blend::SlotPart;
use crate::fighter::anim::slot_mix::set_sub_mesh;
use crate::fighter::anim::update::{layer_frozen, shown_uid};
use crate::fighter::anim::{mix_slots, slot_jump_to_section};
use crate::fighter::{FighterId, facing, grounded};
use crate::math::{self, max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::NotifyHandler;
use crate::registry::{CharHooks, Condition, Registry, RegistryError};
use crate::world::World;

// ---- the conditions

/// WWAllyGuarded Check @51-543: the first WW_BracerShield among Comp_Spawner.SpawnedActors gives its AllyGuarded() =
/// IsValid(GuardedAlly); without one false (@543). GuardedAlly is written only by SetUpBeamParticle, which nothing
/// calls, so the answer is false with a shield too.
fn ally_guarded(_w: &mut World, _f: FighterId) -> bool {
    false
}

/// WWHasLassoGrappleCondition Check @79-267: BPI_CheckIfHasBuff(Buff_Perk_WonderWomanLassoGrapple_C)'s HasBuff (the
/// LassoGrapple perk's buff, perks_lasso.rs).
fn has_lasso_grapple(w: &mut World, f: FighterId) -> bool {
    has_buff(w, f, name!("Buff_Perk_WonderWomanLassoGrapple_C"))
}

// ---- the notifies

/// AnimNotify_WonderWoman_BracerCharge Received_NotifyBegin @214: the owner's Comp_BracerCharge.ChargeGate = true.
fn bracer_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    wonder_woman_w(w, f).bracer_gate = true;
}

/// Received_NotifyEnd @214: ChargeGate = false.
fn bracer_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    wonder_woman_w(w, f).bracer_gate = false;
}

// ---- Comp_BracerCharge

// Comp_BracerCharge ReceiveTick @411 (authority), behind ChargeGate @392. Below ChargeMax @211: ChargeAmount =
// FClamp(ChargeAmount + DeltaSeconds, 0, ChargeMax) @263-360 and its multicast @10, then with ChargeAmount ==
// ChargeMax @33 the owner's Comp_Pawn_MontageNavigation.NotifyChangeSection('ChargeStop', null) @156; at
// ChargeMax already, that call alone, every gated tick. With no montage the jump goes to the active montage
// instance (JumpToSectionLogic @2702-2808, the jump code navJump); the two montages that carry the notify
// (Special_ShieldBuff, Special__Air_ShieldBuff) have the one section Default, so it moves nothing in them.
// Nothing reachable takes the charge away: ConsumeCharge @492-568 is called by the montages outside the closure
// only (bracerChargeAttack), so the amount adds up over her casts until it stands at 1.9.
// Unverified: the component's place among the owner's component ticks is not read (as Comp_C017_Hover_Rage's).
fn bracer_charge_tick(w: &mut World, f: FighterId, dilation: Real) {
    let s = wonder_woman_w(w, f);
    if !s.bracer_gate {
        return;
    }
    if s.bracer_charge < BRACER_CHARGE_MAX {
        s.bracer_charge = min(max(s.bracer_charge + (DELTA_SECONDS * dilation), 0.0), BRACER_CHARGE_MAX);
        if s.bracer_charge != BRACER_CHARGE_MAX {
            return;
        }
    }
    if let Some(m) = active_instance(w, f) {
        slot_jump_to_section(w, f, m, name!("ChargeStop"));
    }
}

// ---- the hair

/// Pawn_WonderWomanV3 SetRotationLogic, after the parent's (@2985): SetSubMeshVisibility(LeftHair, FacingLeft) @3004
/// and (RightHair, !FacingLeft) @3101. The spawn's SetRotationLogic sets the pair the first time.
fn hair(w: &mut World, f: FighterId) {
    let fc = facing(w, f);
    set_sub_mesh(w, f, name!("LeftHair"), fc < 0.0);
    set_sub_mesh(w, f, name!("RightHair"), fc > 0.0);
}

// ---- WonderWoman_AnimBPV2's slots

// WonderWoman_AnimBPV2's ShieldWalk lists, with every anim update of her mesh, montage or not.
// BlueprintUpdateAnimation @1935-2294 (behind IsValid(Fighter) @1877):
//   IsMoving = VSize(CharacterMovement.Velocity) >= 50                       (the whole vector: falling counts)
//   MovingForward = FacingLeft ? Velocity.Y > 0 : Velocity.Y < 0             (sim: vx x facing > 0)
// AnimGraph: Slot_10 (OverrideSlot) <- Slot_9 (DefaultSlot) <- BlendListByBool_2(IsMoving; true: BlendListByBool_1,
// false: Slot_5 ShieldWalk_N) and BlendListByBool_1(MovingForward; true: Slot_4 ShieldWalk_F, false: Slot_3
// ShieldWalk_B); BlendTime 0.1 s for every pose, linear. Each slot's source is the cached AfterLassoAim pose, so
// without a track in them the lists mix three copies of one pose.
// Native rules: a list steps by the instance's delta (the hit pause's dilation in it) from the tick of the flip on;
// nothing under a full-weight OverrideSlot montage is updated (layer_frozen); the inner list is updated only while its
// weight in the outer one is above 0.00001, holds its weights otherwise, and snaps to its bool at its first update.
// The montage's three tracks at its position, weights N = 1 - m, F = m x f, B = m x (1 - f): the heaviest is the
// fighter's anim / anim_time and the others her slot mix (mix_slots). Start and End play one sequence in all three,
// so the mix is empty there.
// The mix is composed wherever the pose is written (poseUpdated: update_pose, a hit's among them) and again after the
// step here (slot_compose, with the LassoAim layers); a death empties it (no anim update while dead in the port).
// Unverified: the step is placed after the montage advance of the tick with the velocity of that moment (the stock
// order: montages, BlueprintUpdateAnimation, the graph's update); the build's order is not read.
// Deviation: the nested lists are flattened to one weighted sum, and the hurt boxes mix the sampled box tracks linearly
// with the same weights.
// Deviation: the port does not update a dead pawn's AnimBP (the build's mesh still ticks, with its movement off), so
// the respawn puts the IsMoving weight at 0 (`reset` below) and leaves the inner list where it was.

const SHIELD_BLEND: Real = 0.1;

/// `shieldWalkUpdate` (animUpdate).
fn shield_walk_update(w: &mut World, f: FighterId, dilation: Real, pre: bool) {
    if pre {
        return;
    }
    if !layer_frozen(w, f) {
        let step = (DT * dilation) / SHIELD_BLEND;
        let (vx, vy, fc) = (w.fighter(f).body.vx, w.fighter(f).body.vy, facing(w, f));
        let is_moving = math::ue::size([0.0, vx, vy]) >= 50.0;
        let moving_forward = vx * fc > 0.0;
        let s = wonder_woman_w(w, f);
        s.shield_move_w = min(max(s.shield_move_w + if is_moving { step } else { -step }, 0.0), 1.0);
        if s.shield_move_w > 1e-5 {
            s.shield_fwd_w = if s.shield_fwd_w < 0.0 {
                if moving_forward { 1.0 } else { 0.0 }
            } else {
                min(max(s.shield_fwd_w + if moving_forward { step } else { -step }, 0.0), 1.0)
            };
        }
    }
    slot_compose(w, f);
}

// WonderWoman_AnimBPV2's LassoAim layers (the CDO's nodes in WonderWoman_AnimBPV2). The cached pose AfterLassoAim, the
// source of the ShieldWalk slots above, is a BlendListByBool on IsFalling:
//   falling:  TwoWayBlend_2 (Alpha <- LassoAimD) { A = TwoWayBlend_3 (Alpha <- LassoAimU) { A = slot LassoAimN,
//             B = slot LassoAimUp }, B = slot LassoAimD }
//   grounded: TwoWayBlend (Alpha <- LassoAimD) { A = TwoWayBlend_1 (Alpha <- LassoAimU) { A = slot LassoAimNGround,
//             B = slot LassoAimUp_Ground }, B = slot LassoAimDown_Ground }
// Each blend: a float alpha, scale 1, bias 0, no clamp, no blend time; TwoWayBlend's update (1424946e0; Evaluate
// 1424732f0) clamps it to 0..1 and takes A alone at 0.00001 and under, B alone at 0.99999 and over. The alphas are the
// AnimBP's LassoAimU / LassoAimD (state.rs), which the lasso actor writes (actors.rs). With the class defaults 1 and 1
// (a match's first throw until the lasso's BeginPlay) the Down slot is the whole pose.
// The three tracks of the branch at the montage's position, weights N = (1 - U)(1 - D), Up = U(1 - D), Down = D: the
// heaviest is the fighter's anim / anim_time and the others her slot mix (mix_slots), so the hit and hurt shapes, the
// wrist the lasso reads and the drawn pose are one mix. A branch without tracks in the montage (the ground throw in
// the air, the two Slow throws on the other side) passes its source on, the locomotion clip (mix_slots).
// Unverified (native, not read): the lasso's Tick writes the alphas after her AnimBP's update of the same frame, so the
// pose of a frame is mixed by what the lasso wrote the frame before (the port's order: the fighters' mesh ticks, then
// World's projectiles). Taken as the likely order: both tick in TG_PrePhysics with no prerequisite between them and the
// pawn is registered long before the actor it spawns. The other order would put this compose after the actor's
// `moved`. The BeginPlay reset is read the same way (the spawner notify is dispatched after the pose of its tick is
// composed), so a match's first throw is the Down track up to and with the tick the lasso appears in.
// Deviation: the nested blend is one weighted sum (exact for two poses, and Up and Down never have a weight together),
// and the shapes mix the sampled box tracks linearly.
// Deviation: the wrist the sim reads is the linear mix of the two tracks' socket points, the drawn wrist the socket of
// blended bone rotations: inside the band (an alpha between 0.00001 and 0.99999) they are not the same point.
// Deviation: the IsFalling list's 0.05 s is not stepped, and the branch is `grounded` of the tick where the AnimBP reads
// IsFalling(): the two differ under MOVE_Flying / MOVE_None (no such case found in the five lasso montages).
// Deviation: a branch without tracks passes the bare locomotion clip on; the build's source is the cached AfterFall
// (Navigation with the WonderWoman_Nav_FallAdd additive while falling).
// As the build's graph has it, and so not a Unverified: WonderWoman_Attack_LassoDash_Montage before any lasso of the
// pawn (1 / 1) takes the Down variant's pose and shapes.

const AIM_AIR: [&str; 3] = ["LassoAimN", "LassoAimUp", "LassoAimD"];
const AIM_GROUND: [&str; 3] = ["LassoAimNGround", "LassoAimUp_Ground", "LassoAimDown_Ground"];

/// TwoWayBlend's relevance band.
fn aim_alpha(a: Real) -> Real {
    if a >= 0.99999 {
        1.0
    } else if a > 0.00001 {
        a
    } else {
        0.0
    }
}

/// The two slot graphs a montage of hers plays in, by its first slot track.
enum Graph {
    Shield,
    Aim,
    None,
}

/// `slotCompose` (poseUpdated): the slot mix of the shown montage.
fn slot_compose(w: &mut World, f: FighterId) {
    let uid = shown_uid(w, f);
    let graph = match uid.and_then(|u| instance_data(&w.data, w, f, u)).and_then(|m| m.slots.first()) {
        Some(t) if t.slot == "ShieldWalk_N" => Graph::Shield,
        // A montage of the LassoAim slots has tracks in nothing else (the five of her data), so its first names one.
        Some(t) if AIM_AIR.contains(&t.slot.as_str()) || AIM_GROUND.contains(&t.slot.as_str()) => Graph::Aim,
        _ => Graph::None,
    };
    let s = wonder_woman(w, f);
    let parts: [(SlotPart<'static>, Real); 3] = match graph {
        Graph::Shield => {
            let (m, fw) = (s.shield_move_w, max(s.shield_fwd_w, 0.0));
            [
                (SlotPart::Slot("ShieldWalk_N"), 1.0 - m),
                (SlotPart::Slot("ShieldWalk_F"), m * fw),
                (SlotPart::Slot("ShieldWalk_B"), m * (1.0 - fw)),
            ]
        }
        Graph::Aim => {
            let set = if grounded(w, f) { AIM_GROUND } else { AIM_AIR };
            let (u, d) = (aim_alpha(s.lasso_aim_u), aim_alpha(s.lasso_aim_d));
            [
                (SlotPart::Slot(set[0]), (1.0 - u) * (1.0 - d)),
                (SlotPart::Slot(set[1]), u * (1.0 - d)),
                (SlotPart::Slot(set[2]), d),
            ]
        }
        Graph::None => {
            wonder_woman_w(w, f).slot_mix.slot_mix.clear();
            return;
        }
    };
    let mut out = std::mem::take(&mut wonder_woman_w(w, f).slot_mix.slot_mix);
    mix_slots(w, f, uid, &parts, &mut out, None);
    wonder_woman_w(w, f).slot_mix.slot_mix = out;
}

// ---- the other hooks

fn death(w: &mut World, f: FighterId) {
    wonder_woman_w(w, f).slot_mix.slot_mix.clear();
}

/// Comp_Armor.ArmorBreak, bound in ReceiveBeginPlay @654-707 -> On Armor Break @4317 -> @1361: the meter gains
/// MeterPerArmorHit, whatever the pack's type and whoever broke it (no test of the delegate's arguments).
fn armor_break(w: &mut World, f: FighterId, _kind: Name, _by: Option<FighterId>, _x: bool) {
    add_meter(w, f, METER_PER_ARMOR_HIT);
}

/// BPI_BlockedProjectile @1837-1987 (BPI_WonderWoman): the meter gains MeterPerProjectile; called by WW_ShieldActor's
/// BlockCollision overlap @2793 (actors/shield_actor.rs shield_step).
fn blocked_projectile(w: &mut World, f: FighterId) {
    add_meter(w, f, METER_PER_PROJECTILE);
}

/// A new pawn (World.reset: a Lab reset, a new match) has a new AnimBP: LassoAimU / LassoAimD at the class defaults 1
/// and 1 again, whatever a lasso the reset dropped had written. A respawn keeps the pawn (`reset`). The new pawn's
/// SetupInitialVisibility shows both hair meshes (both PrimaryMesh, fighter/life.rs reset_match_state); PandaGameMode
/// SpawnCharacter's SetRotationLogic (@1812) then sets the pair.
fn match_reset(w: &mut World, f: FighterId) {
    if let Some(s) = wonder_woman_mut(w, f) {
        s.lasso_aim_u = 1.0;
        s.lasso_aim_d = 1.0;
    }
    hair(w, f);
}

/// BPI_Cleanup @2307 -> BPI_ClearMeter @2118 (BaseFighter ResetFighterLogic calls BPI_Cleanup).
fn reset(w: &mut World, f: FighterId) {
    clear_meter(w, f);
    wonder_woman_w(w, f).shield_move_w = 0.0; // see shield_walk_update's last Deviation note
}

/// The hooks of this file in Wonder Woman's CharHooks (`mod.rs`: animUpdate, poseUpdated, death, init, turned,
/// componentTick, armorBreak, blockedProjectile, matchReset, reset).
pub fn hooks(h: &mut CharHooks) {
    h.anim_update = Some(shield_walk_update);
    h.pose_updated = Some(slot_compose);
    h.death = Some(death);
    h.init = Some(hair);
    h.turned = Some(hair);
    h.component_tick = Some(bracer_charge_tick);
    h.armor_break = Some(armor_break);
    h.blocked_projectile = Some(blocked_projectile);
    h.match_reset = Some(match_reset);
    h.reset = Some(reset);
}

/// This file's registry entries (the two conditions, the two notifies).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.conditions.enter("WWAllyGuarded_C", Condition::Fn(ally_guarded))?;
    r.conditions.enter("WWHasLassoGrappleCondition_C", Condition::Fn(has_lasso_grapple))?;
    // AnimNotify_WW_HurtBox Received_NotifyBegin @121 BPI_WonderWoman_SetHurtBox(Box Number), NotifyEnd @121 with 0:
    // the pawn's SetHurtBoxMulticast @4278 returns at once while ExperimentalHurtBoxes is true (the CDO's value, never
    // written), so neither does anything.
    r.notifies.enter("AnimNotify_WW_HurtBox_C", NotifyHandler::default())?;
    r.notifies.enter(
        "AnimNotify_WonderWoman_BracerCharge_C",
        NotifyHandler { begin: Some(bracer_begin), end: Some(bracer_end), ..NotifyHandler::default() },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aim_alpha_band() {
        assert_eq!(aim_alpha(0.999995), 1.0);
        assert_eq!(aim_alpha(0.000005), 0.0);
        assert_eq!(aim_alpha(0.5), 0.5);
    }
}
