//! C018_AnimBP's sim part: AimValue from the aim rotation (the slot mix's input)
//! and the slot graph's inputs. Its variable is `MarvinState::aim_value` (state.rs). pawn.rs's hooks hold its
//! `slotInput`; its `animUpdate` is the joined animUpdate's only part.
//!
//! The slot graph (marvin.json slotHolds.graph, walked by `fighter::anim` graph_slots): the root TwoWayBlend (A
//! TwoWayBlend_1, B the AimDown slot), TwoWayBlend_1 (A DefaultSlot -> JumpSlot -> the linked layer, B the AimUp slot);
//! both aim slots take the DefaultSlot node as their source. The alphas are the nodes' exposed inputs, by the node's
//! name: TwoWayBlend = FClamp(AimValue, -1, 0) x -1 (@1867-1956), TwoWayBlend_1 = FClamp(AimValue, 0, 1) (@1993-2040).
//!
//! AimValue: BlueprintUpdateAnimation @3192 -> @3216-3434: AimAngleRotation = the pawn's BPI_GetAnimAimRotation, then
//! @1102-1537: with AimAngleRotation != 0, AimValue = RotateAngleAxis(BPI_GetProjectileAimDirection, AimAngleRotation,
//! FacingLeft ? (-1, 0, 0) : (1, 0, 0)).Z x AimValueModifier (@1253-1509); else 0 (@1537). The aim vector is
//! BaseFighter's Comp_Spawner.ProjectileAimVector (`items.proj_aim`: UE Y, Z; X is 0). FVector::RotateAngleAxis about
//! (a, 0, 0), a = +-1 (`fmath::rotate_angle_axis`: FMath::SinCos of the float radians).
//
// The update's place: the anim instance's update runs after the montage advance and before the graph's evaluation;
// what it reads changes only at the aim rotation notifies' begin and end (all Queued in the data: dispatched after
// the update, so read at the next one) and in the actors' ticks (the aimer), none of them inside the mesh tick. So the
// port computes it in the update's `pre` call, ahead of the advance and the pose (graph_slots reads it through
// `slot_input`): the values the build's update reads.

use super::state::{marvin, marvin_w};
use crate::Real;
use crate::fighter::FighterId;
use crate::math::ue::rotate_angle_axis;
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::world::World;

/// C018_AnimBP's class default AimValueModifier (Default__C018_AnimBP_C: 1.5); no graph writes it. Not in the
/// character's data file: the anim instance's own defaults are not exported there.
const AIM_VALUE_MODIFIER: Real = 1.5;

/// `aimValueOf`: AimValue from the aim rotation.
pub fn aim_value_of(w: &World, f: FighterId) -> Real {
    let deg = marvin(w, f).aim_deg;
    if deg == 0.0 {
        return 0.0; // NotEqual_FloatFloat @1102 -> @1537
    }
    let fi = w.fighter(f);
    let a = if fi.body.facing_dir < 0.0 { -1.0 } else { 1.0 }; // SelectVector((-1, 0, 0), (1, 0, 0), FacingLeft) @1299
    let (y, z) = (fi.items.proj_aim.y, fi.items.proj_aim.z);
    // RotateAngleAxis @1362 (a unit axis: the node's GetSafeNormal keeps it), its Z (BreakVector @1417)
    let rz = rotate_angle_axis([0.0, y, z], deg, [a, 0.0, 0.0])[2];
    rz * AIM_VALUE_MODIFIER // @1463-1509
}

/// `slotInput` (pawn.rs's `pawnHooks.slotInput`): the value of one of the graph's inputs as the AnimBP holds it at
/// this pose update: the two TwoWayBlend alphas, by the node's name (their EvaluateGraphExposedInputs handlers).
/// pawn.rs's hooks set it.
pub fn slot_input(w: &World, f: FighterId, input: Name) -> Real {
    let v = marvin(w, f).aim_value;
    if input == name!("AnimGraphNode_TwoWayBlend") {
        -min(max(v, -1.0), 0.0) // @1867-1956
    } else if input == name!("AnimGraphNode_TwoWayBlend_1") {
        min(max(v, 0.0), 1.0) // @1993-2040
    } else {
        Real::NAN
    }
}

/// aim.rs's `animUpdate` (pawn.rs's `pawnHooks.animUpdate`, the joined animUpdate's only part):
/// BlueprintUpdateAnimation's
/// AimValue, in the `pre` call (the header).
pub fn anim_update(w: &mut World, f: FighterId, _dilation: Real, pre: bool) {
    if !pre {
        return;
    }
    let v = aim_value_of(w, f);
    marvin_w(w, f).aim_value = v;
}
