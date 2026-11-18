//! Tom_and_Jerry_AnimBP's sim part: the slot graph's inputs, the anim set
//! by WithJerry, the turn's fade and JerryWallOffset; read from `TomAndJerryState.anim` and the pawn state's slot mix.
//! pawn.rs enters these functions in its hooks.
//!
//! The slot graph (`FighterData.slotHolds.graph`; `fighter::anim` graph_slots, `fighter::anim::slot_blend`), from the
//! root:
//!   ApplyAdditive (Base: the OverrideSlot chain; Additive: AdditiveSlot over the Tom_and_Jerry_AdditiveComand
//!     sequence, Alpha the constant 0.01): its Base alone (slotInput 0 for the node, as LayeredBoneBlend's rule).
//!     Deviation: the additive's 1 % (the Comand_* montages' AdditiveSlot, or the sequence's own pose) is not mixed.
//!   Slot OverrideSlot <- LayeredBoneBlend (Base: the aim tree; layer 0: BlendListByBool_1 by RocketControlBool
//!     [0.1, 0.1] over Slot ControlL / ControlR, BlendWeights [1], per-bone filter): its Base alone for the walk
//!     (slotInput 0, the rule C015's and Taz's graphs use); the layer is laid over the shapes by arms.rs (CharHooks.pose,
//!     the list's 0.1 s fade stepped there) and over the drawn pose by the render's pose module (the bones from
//!     tom_un_L_clav / tom_un_R_clav down).
//!   The aim tree: TwoWayBlend_1 (alpha AimDirectionInverted = AimDirection x -1, @992-1034) over A TwoWayBlend_2 (alpha
//!     AimDirection) and B Slot SlingShotAimDown; TwoWayBlend_2 over A TwoWayBlend and B Slot SlingShotAimUp;
//!     TwoWayBlend (alpha Conv_BoolToFloat(OverrideMirror? ? MirrorOverride : false), EvaluateGraphExposedInputs
//!     @15320-15431) over A Slot SlingShotAim and B Slot SlingShotAimBack; SlingShotAim's source BlendListByBool_3
//!     (Moving [0.1, 0.1]) over Slot GroundMovingSlot / GroundStillSlot <- BlendListByBool (IsFalling [0.1, 0.1]) over
//!     Slot FallingBlend / OnGround <- Slot DefaultSlot <- Slot JumpSlot <- BlendListByBool_2 (WithJerry [0.1, 0.1])
//!     over the LinkedAnimLayer (the anim set) / the LinkedAnimGraph (Base_AnimBP_Tom_and_Jerry_No_Jerry:
//!     animSets.noJerry).
//!
//! The inputs (BlueprintUpdateAnimation @15468 -> @1062-1691, behind IsValid(Fighter)): IsFalling =
//! MovementComponent.IsFalling() @1193 and Moving = Velocity.Y != 0 @1516, read live as Finn's (the same rule;
//! Deviation: a dead pawn's AnimBP is not updated); WithJerry = BPI_IsJerryVisible @1614-1660, written in the update
//! ([`anim_update`]) and read there; the aim, mirror and rocket inputs are the setters' (pawn.rs).
//!
//! JerryWallOffset = FMax(TurnAlpha, WallAlpha) @1295-1341 after TurnAlpha = FInterpTo(TurnAlpha, 0, DeltaTimeX, 10)
//! @1212-1268; OnFacingChanged (@1735 bound to the fighter's FacingChanged, and @1928 -> @2053 at
//! BlueprintInitializeAnimation with BPI_IsFacingLeft): TurnAlpha = 1 when FacingLeft != OldFacingLeft (class default
//! true), then OldFacingLeft = FacingLeft. ModifyBone_6 (jerry_un_COM, translation (0, 0, 0) BMM_Replace, component
//! space) and ModifyBone_7 (jerry_un_COM, (0, 100, 0) BMM_Additive, component space), alpha
//! SelectFloat(0, JerryWallOffset, IgnoreJerryWallOffset) @7341 / @7809: Jerry pulled back onto the component origin
//! (Tom) and 100 cm aside in depth while a wall or a ledge is near or Tom just turned.
//! The shapes: JerryHit / JerryHit2D (on jerry_un_C_spine_b) are the only shapes on Jerry's bones, and every hitframe
//! that turns them on (PaddleBall, PaddleBall_No_Jerry) lies inside an AnimNotify_JerryIgnoreWall window
//! (TurnOffOnEnd? true): the alpha is 0 there, so the sim does not pose the two nodes into the shapes (no `pose` hook
//! for them).
//! Deviation: a JerryHit window outside an IgnoreWall window (none in the data) would want the shift. The sockets the
//! pawn reads on Jerry's bones (pawn.rs `jerry_socket`) take ModifyBone_6's shift; ModifyBone_7's is along the depth.

use super::state::{tj, tj_w};
use crate::Real;
use crate::consts::DELTA_SECONDS;
use crate::fighter::anim::{SetRef, anim_set_switched};
use crate::fighter::common::data_of;
use crate::fighter::moves::is_falling;
use crate::fighter::{FighterId, facing};
use crate::math::max;
use crate::math::ue::finterp_to;
use crate::name;
use crate::names::Name;
use crate::world::World;

/// `jerryMeshVisible` (BPI_IsJerryVisible: IsValid(JerryMesh) ? JerryMesh.bVisible : false): the sub-mesh tagged
/// JerryMesh as the match view draws it: out of HiddenMeshes, and while the fighter is invisible (Comp_FighterVisibility
/// Manager AddInvisibility) only when AddInvisibility left it shown (`invis_shown`). Called by this file's animUpdate
/// (and the bots' TomAndJerry_IsJerryOut_BTD).
// Deviation: SetSubMeshVisibility(JerryMesh, true) with the invisibility count above 0 shows the mesh in the build
// (bVisible true); the port's sets keep it hidden until RemoveInvisibility.
pub fn jerry_mesh_visible(w: &World, f: FighterId) -> bool {
    if !data_of(w, f).def.sub_meshes.iter().any(|m| m == "jerrymesh") {
        return false;
    }
    let fi = w.fighter(f);
    let tag = name!("jerrymesh");
    if fi.pose.sub_hidden.contains(&tag) {
        return false;
    }
    !fi.vis.invisible || fi.vis.invis_shown.as_ref().is_some_and(|s| s.contains(&tag))
}

/// `slotInput` (CharHooks.slotInput): the graph's inputs (`fighter::anim` graph_in), NaN for a name the graph does not
/// have. Called by pawn.rs's hooks.
pub fn slot_input(w: &World, f: FighterId, input: Name) -> Real {
    let a = &tj(w, f).anim;
    let b = |v: bool| if v { 1.0 } else { 0.0 };
    if input == name!("AimDirectionInverted") {
        -a.aim_direction
    } else if input == name!("AimDirection") {
        a.aim_direction
    } else if input == name!("AnimGraphNode_TwoWayBlend") {
        b(a.override_mirror && a.mirror_override)
    } else if input == name!("Moving") {
        b(w.fighter(f).body.vx != 0.0)
    } else if input == name!("IsFalling") {
        b(is_falling(w, f))
    } else if input == name!("WithJerry") {
        b(a.with_jerry)
    } else if input == name!("RocketControlBool") {
        b(a.rocket_control)
    } else if input == name!("AnimGraphNode_LayeredBoneBlend") || input == name!("AnimGraphNode_ApplyAdditive") {
        0.0 // the Base pose alone (the header: arms.rs lays the LayeredBoneBlend's layer over the shapes)
    } else {
        Real::NAN
    }
}

/// `animSet` (CharHooks.animSet): BlendListByBool_2 on WithJerry, true the LinkedAnimLayer (the data file's animSet),
/// false the LinkedAnimGraph Base_AnimBP_Tom_and_Jerry_No_Jerry (animSets.noJerry; animSetBlend's 0.1 s each way).
/// Called by pawn.rs's hooks.
pub fn anim_set(w: &World, f: FighterId) -> SetRef {
    if tj(w, f).anim.with_jerry { SetRef::Base } else { SetRef::Named(name!("noJerry")) }
}

/// `onFacingChanged` (CharHooks.turned: the fighter's FacingChanged broadcast; OnFacingChanged(FacingLeft)): TurnAlpha
/// = 1 on a change of side. Called by pawn.rs's hooks.
pub fn on_facing_changed(w: &mut World, f: FighterId) {
    let left = facing(w, f) < 0.0;
    let a = &mut tj_w(w, f).anim;
    if left != a.old_facing_left {
        a.turn_alpha = 1.0;
    }
    a.old_facing_left = left;
}

/// `animInit` (BlueprintInitializeAnimation's facing read @15723 -> @1928-2053): a new anim instance (the pawn's
/// BeginPlay; a match reset's new state) takes the facing it is made with. Called by pawn.rs.
pub fn anim_init(w: &mut World, f: FighterId) {
    on_facing_changed(w, f);
}

/// `animUpdate` (CharHooks.animUpdate, `pre`: BlueprintUpdateAnimation, ahead of the montages' advance and the graph's
/// update): the turn's fade, JerryWallOffset and WithJerry. A change of WithJerry switches the anim set
/// (`anim_set_switched`: the node's 0.1 s crossfade from the entry states, that function's FLAGs). Called by pawn.rs's
/// hooks.
pub fn anim_update(w: &mut World, f: FighterId, dilation: Real, pre: bool) {
    if !pre {
        return;
    }
    {
        let a = &mut tj_w(w, f).anim;
        a.turn_alpha = finterp_to(a.turn_alpha, 0.0, DELTA_SECONDS * dilation, 10.0);
        a.jerry_wall_offset = max(a.turn_alpha, a.wall_alpha);
    }
    let v = jerry_mesh_visible(w, f);
    if v != tj(w, f).anim.with_jerry {
        let from = crate::fighter::anim_set(w, f);
        tj_w(w, f).anim.with_jerry = v;
        anim_set_switched(w, f, from);
    }
}

/// `jerryOffsetAlpha`: ModifyBone_6 / _7's alpha as the pose was last evaluated: SelectFloat(0, JerryWallOffset,
/// IgnoreJerryWallOffset). Called by pawn.rs (`jerry_socket`).
pub fn jerry_offset_alpha(w: &World, f: FighterId) -> Real {
    let a = &tj(w, f).anim;
    if a.ignore_wall_offset { 0.0 } else { a.jerry_wall_offset }
}
