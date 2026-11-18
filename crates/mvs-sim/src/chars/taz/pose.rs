//! Taz's (C015) AnimBP slot mix in the sim: C015_AnimBP's slot graph
//! (FighterData.slotHolds.graph) weighs the montage tracks in its RunningSlot, Charged, Landing, Turning, InAir, AimUp
//! and AimDown slots over DefaultSlot, so the shapes the side specials' tornado (Tornado1..7, bones tornado1..7 of the
//! Tornado mesh, which copies this mesh's pose: CopyPoseFromMesh), the neutral's aim and the charged down attack
//! sample are the mixed pose, as the drawn pose is.
//!
//! The graph from the base layer up (the data's node ids):
//!   7 Slot DefaultSlot (over 8 Slot JumpSlot, over 9 the linked locomotion layer)
//!   6 TwoWayBlend_6 (A 7, B 10 LayeredBoneBlend [base 11 Slot RunningSlot(7), arms 7]), Alpha <- RunValue
//!   5 TwoWayBlend_3 (A 6, B 12 Slot Charged(6)), Alpha <- PercentCharged >= 0.5 as 1 / 0 @1725-1796
//!   4 TwoWayBlend_2 (A 5, B 13 Slot Landing(5)), Alpha <- LandingBlend
//!   3 TwoWayBlend_1 (A 4, B 14 Slot Turning(4)), Alpha <- TurningBlend
//!   2 TwoWayBlend (A 3, B 15 Slot InAir(3)), Alpha <- MovingInAirBlend
//!   16 TwoWayBlend_4 (A 2, B 17 Slot AimDown(2)), Alpha <- clamp((AimDirection - 0.5) x 2) @1557-1688
//!   0 TwoWayBlend_5 (A 1 Slot AimUp(2), B 16), Alpha <- clamp(AimDirection x 2) @1431-1520 (the root)
//! Every TwoWayBlend has a float alpha with the identity scale and bias (the CDO), so the graph is walked by
//! `graph_slots` (`fighter::anim`): clamped 0..1, A alone at 1e-5 and under, B alone at 0.99999 and over. A Slot with a
//! non-empty, non-additive track of the shown montage is that track, else its source; the locomotion layer is the
//! locomotion clip. Its inputs are mod.rs's `slot_input` (the variables below, the three bound alphas, 0 for the
//! LayeredBoneBlend).
//!
//! BlueprintUpdateAnimation (@3830: push_flow 1833, 2357, 2312, 3884, so run in the order 3884, 2312, 2357, 1833), with
//! the instance's delta (the hit pause's dilation in it); the component lookups of the init (@377-1362) find all four
//! components at once (their Delay(0.1) is the retry path), so every gate is passed:
//!   @3884-4134 RunValue = clamp(|GetVelocity().Y| / MaxRunThreshold (600, the CDO), 0, 1): UE Y is the play plane's x.
//!   @4161-4395 WasFacingLeft == FacingLeft: TurningBlend = clamp(TurningBlend - 4 dt), else WasFacingLeft =
//!     FacingLeft, TurningBlend = 1. FacingLeft is On Facing Changed's (BPI_SetIsFacingLeft, the init's
//!     BPI_IsFacingLeft): the fighter's facing. A pawn that starts facing left turns at its first update (the
//!     variable's default false), as the build's.
//!   @2237-2284 PercentCharged = Charge Comp.GetChargeValue(4): the fighter's percent charged.
//!   @2396-2822 AimDirection = FacingLeft ? |1 - c| : c, c = clamp((Z x (FacingLeft ? 1 : -1) + 0.75) / 1.5) with Z the
//!     Spawner's ProjectileAimVector.Z (the fighter's proj_aim.z): both sides are clamp((0.75 - Z) / 1.5), 0 up, 1 down.
//!   @1872-2224 IsMovingOnGround (the walking mode: grounded): MovingInAirBlend = 0 and LandingBlend =
//!     WasMovingOnGround ? clamp(LandingBlend - 4 dt) : 1, WasMovingOnGround = true; in the air MovingInAirBlend = 1,
//!     LandingBlend = 0, WasMovingOnGround = false.
//! The update is stepped after the montage advance of the tick (the stock order: montages, BlueprintUpdateAnimation,
//! the graph), as Batman's and Black Adam's (`CharHooks.anim_update`, not its `pre` call).
//!
//! Deviation: the nested blends are one weighted sum, and the shapes mix the sampled box tracks linearly with the same
//! weights.
//! Deviation: the LayeredBoneBlend (node 10: branches fml_un_L_uparm and fml_un_R_uparm, depth 0, BlendWeights[0] =
//! RunValue <= 0.9 as 1 / 0 @3662-3733) is taken as its base pose alone (RunningSlot over DefaultSlot): the two arms
//! from DefaultSlot's chain while RunValue is at or under 0.9 are not mixed, as a slot mix is whole-body. Only the
//! montages with a RunningSlot track differ (C015_Up_Normal_Ground and the three side specials); the tornado bones are
//! not under the arms.
//! Deviation: the port does not update a dead pawn's AnimBP (mod.rs `death` empties the mix); the variables hold.
//! Deviation: the shown montage is weighed at full weight (its blend-in and blend-out under the slots are not), as
//! `graph_slots`'.
//! The state: `TazState`'s sim pose section. The update joins the pawn's hooks; mod.rs owns `slot_mix` and `slot_input`,
//! through which `update_pose`'s graph_slots composes the mix wherever the pose is written.

use super::taz_w;
use crate::Real;
use crate::consts::DT;
use crate::fighter::FighterId;
use crate::fighter::anim::blend_spaces::fall_mix;
use crate::fighter::anim::graph_slots;
use crate::fighter::anim::update::shown_uid;
use crate::registry::CharHooks;
use crate::world::World;

/// C015_AnimBP CDO MaxRunThreshold.
const MAX_RUN_THRESHOLD: Real = 600.0;

/// The hooks of this file in Taz's CharHooks (animUpdate).
pub fn hooks(h: &mut CharHooks) {
    h.anim_update = Some(anim_update);
}

/// `v < 0 ? 0 : v > 1 ? 1 : v`.
#[allow(clippy::manual_clamp, reason = "clamp01 keeps a NaN input: v < 0 ? 0 : v > 1 ? 1 : v")]
pub(super) fn clamp01(v: Real) -> Real {
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

/// BlueprintUpdateAnimation's variables (`CharHooks.anim_update`, after the montages), then the mix again with them:
/// the shown montage through `graph_slots`, else what `update_pose`'s tail leaves without one (AirState Fall's blend
/// space, or nothing: `fall_mix`).
pub fn anim_update(w: &mut World, f: FighterId, dilation: Real, pre: bool) {
    if pre {
        return;
    }
    let dt4 = (DT * dilation) * 4.0;
    let fi = w.fighter(f);
    let left = fi.body.facing_dir < 0.0;
    let vx = fi.body.vx;
    let charged = fi.charge.charge_pct;
    let z = fi.items.proj_aim.z;
    let grounded = fi.body.on_floor;
    let s = taz_w(w, f);
    s.run_value = clamp01(vx.abs() / MAX_RUN_THRESHOLD);
    if s.was_facing_left == left {
        s.turning_blend = clamp01(s.turning_blend - dt4);
    } else {
        s.was_facing_left = left;
        s.turning_blend = 1.0;
    }
    s.percent_charged = charged;
    let c = clamp01(((z * if left { 1.0 } else { -1.0 }) + 0.75) / 1.5);
    s.aim_direction = if left { (1.0 - c).abs() } else { c };
    if grounded {
        s.moving_in_air_blend = 0.0;
        s.landing_blend = if s.was_moving_on_ground { clamp01(s.landing_blend - dt4) } else { 1.0 };
        s.was_moving_on_ground = true;
    } else {
        s.moving_in_air_blend = 1.0;
        s.landing_blend = 0.0;
        s.was_moving_on_ground = false;
    }
    match shown_uid(w, f) {
        Some(u) => graph_slots(w, f, Some(u), super::slot_input),
        None => {
            let mut out = std::mem::take(&mut taz_w(w, f).slot_mix.slot_mix);
            fall_mix(w, f, &mut out);
            taz_w(w, f).slot_mix.slot_mix = out;
        }
    }
}
