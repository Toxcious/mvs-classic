//! C017_AnimBP's movement split: the AnimBP's variables for it
//! (`GiantAnim`'s split fields, pose.rs), which montages it takes, and their tracks for the sim's pose graph
//! (pose.rs; the view composes the same graph on the whole skeleton).
//!
//! The graph part:
//!   BeforeUperbodySplit = LayeredBoneBlend_11, branch fml_un_C_spine_a, BlendWeights[0] = Split (0 / 1), mesh
//!     space rotation:
//!       base  = Split AND NOT IsFalling ? (NOT Moving ? Slot GroundStillSlot over Navigation
//!                                                      : Navigation with ModifyBone_6 on fml_un_COM)
//!                                        : Navigation                        (BlendListByBool_7 / _3, 0 s)
//!       blend = TwoWayBlend_3(TwoWayBlend_4(Slot UpperBodyOnly, Slot SpinLeanB; "lean backward"),
//!                             Slot SpinLeanF; "lean forward"), each slot over Navigation
//!   BeforeWeaponMode    = Slot DefaultSlot over (Split ? BeforeUperbodySplit : Slot Evade over it)
//!   ... the weapon mode layers and the additive slots (PreTransform) ...
//!   the Transform stage = LayeredBoneBlend #9, branch fml_un_C_spine_a, weight 1, mesh space rotation:
//!       base  = Moving ? PreTransform : Slot Transform over PreTransform     (BlendListByBool #7, 0 s)
//!       blend = Slot Transform over PreTransform
//! A Slot node with no track for its name is its source, so a montage shows only through the nodes it has a track
//! for; UpperBodySpin has no Slot node in the graph (C017_Air_Spin_Montage shows nothing). The port's main pose is
//! one clip of the default group (the fighter's anim). For a montage whose tracks are all of these slots that clip
//! stays Navigation's (CharHooks.nav_pose) and the tracks are laid over it here, each at the montage's weight.
//!
//! The variables, written in BlueprintUpdateAnimation (@12883: the block @1946, then @4913) and read by the node
//! handlers of the same update:
//!   MovementSplitActive / SplitMovementJumpCheck: the two notify states (Queued: dispatched after the graph update
//!     of their tick, so the handlers read them one update later: a window from 0 leaves one update with Split
//!     false);
//!   IsFalling = MovementComponent.IsFalling() (@5044); Moving = abs(Velocity.Y) > 50 (@5177-5248);
//!   Split = MovementSplitActive OR (SplitMovementJumpCheck AND IsFalling) (@9459-9535);
//!   AttackLean (@3861-4842): 0 at once while the pawn's BPI_IsAttacking is false; else FInterpTo(AttackLean,
//!     IsFalling ? 0 : Lerp(FallLeanAmount, -FallLeanAmount, DownPressed ? 1 : a), DeltaTimeX, IsFalling ? 2.5 : 1.5)
//!     with a = clamp01(NormalizeToRange(the speed toward the facing, -1.5 x BaseMaxWalkSpeed, 1.5 x
//!     BaseMaxWalkSpeed)) and the IsFalling of the update before (the block @4913 runs after this one);
//!   "lean backward" (@8183-8327) = clamp(2 x AttackLean, 0, FallLeanAmount) / FallLeanAmount (SpinLeanB),
//!   "lean forward" (@8547-8775) = -clamp(2 x AttackLean, -FallLeanAmount, 0) / FallLeanAmount (SpinLeanF).
//! They are sim state (GiantAnim): the shapes ride the pose they make, and the view reads the same values.
//! Deviations and guesses:
//!   - ModifyBone_6 (BMM_Replace in BCS_WorldSpace, yaw +90 when Velocity.Y > 0, else -90, @11425-11754) on the
//!     moving ground branch: C017_AnimBP runs on CharacterMesh0, whose world rotation is the actor's yaw (+90 left,
//!     -90 right) and which is never mirrored, so fml_un_COM's component space rotation becomes the identity when he
//!     moves toward his facing and half a turn about the vertical when against it (`back`), the clip's own rotation
//!     and the hit pitch dropped. Unverified: on the tick of a facing change the proxy's component transform may
//!     still hold the old actor rotation (one update of legs the wrong way); the facing of the update is taken;
//!   - IsFalling is taken as "not on the ground", in the hover too (the hover's movement mode is not read);
//!   - BaseMaxWalkSpeed is the character's CharMoveComp MaxWalkSpeed (read once at the AnimBP's init in the build),
//!     DownPressed the tick's input (stick down or the down button), as the hover's lean takes them;
//!   - the order of the main instance's notify dispatch against the linked layer's update is not read: the
//!     variables are stepped once a mesh tick ahead of the montage update, where the tick's dispatch has not run yet;
//!   - DeltaTimeX is the mesh tick's step with the hit pause's dilation; no update while dead.

use super::pose::GiantAnim;
use super::state::{giant_mut, giant_of};
use crate::Real;
use crate::consts::DT;
use crate::fighter::anim::MontageState;
use crate::fighter::anim::montages::instance_data;
use crate::fighter::common::data_of;
use crate::fighter::{FighterId, is_attacking};
use crate::math::ue::finterp_to;
use crate::math::{max, min};
use crate::registry::CharHooks;
use crate::world::World;
use mvs_data::fighter::Montage;

/// C017_AnimBP's CDO: FallLeanAmount.
pub const FALL_LEAN: Real = 10.0;
/// @5177: abs(Velocity.Y) > 50.
const MOVING: Real = 50.0;
/// UpdateMontageEvaluationData: BlendedValue > 1e-5.
const MIN_W: Real = 1e-5;
/// The slots whose nodes sit in the split and the Transform stage, and the one with no node at all.
const NAV_SLOTS: [&str; 7] =
    ["GroundStillSlot", "UpperBodyOnly", "SpinLeanF", "SpinLeanB", "Evade", "Transform", "UpperBodySpin"];

/// The hooks of this file in Iron Giant's CharHooks (`mod.rs` character: navPose giantNavPose, layerHeld
/// giantLayerHeld).
pub fn hooks(h: &mut CharHooks) {
    h.nav_pose = Some(nav_pose);
    h.layer_held = Some(layer_held);
}

/// `giantNavPose` (CharHooks.nav_pose): every track of the montage is one of those slots' (a montage with a
/// DefaultSlot, DashSlot or TurretReturn track stays the port's main pose).
pub fn nav_pose(m: &Montage) -> bool {
    m.group.as_deref().is_none_or(str::is_empty)
        && !m.slots.is_empty()
        && m.slots.iter().all(|s| NAV_SLOTS.contains(&s.slot.as_str()))
}

// `giantLayerHeld` (CharHooks.layer_held): C017_AnimBP's Slot nodes without bAlwaysUpdateSourcePose whose hold of
// the Navigation cache depends on a blend input (the node flags and links: FighterData.slotHolds.nodes).
// FAnimNode_Slot::Update (exe 1424945f0) leaves the source without an update at a source weight <= 0.00001, i.e.
// under a track of its slot at full weight:
//   Transform (#6, #8): LayeredBoneBlend #9's blend pose is Slot Transform #8 over PreTransform and its base is
//     BlendListByBool #7 (Moving): true reads the PreTransform cache itself, false is Slot Transform #6 over it. So
//     with Moving false both consumers of PreTransform are held slots and nothing under it is updated (the weapon
//     layers, the split, Navigation); with Moving true the cache is updated. Split does not matter.
//   Evade (#43): BlendListByBool_2's false side (Split false), over BeforeUperbodySplit, whose only other consumer
//     is the true side. With Split false and the track at full weight nothing under it is updated; with Split true
//     the node is out of the path and the UpperBodyOnly slot (bAlwaysUpdateSourcePose) updates Navigation.
//   DashSlot (#113, #106, #118): #118 is LayeredBoneBlend_8's base, whose blend pose (weight 1, no handler) reads
//     the AfterWeaponMode cache through LayeredBoneBlend_6, so that cache is always updated; under it
//     BlendListByBool_6 (Turret Aim) reads BeforeWeaponMode itself when true, and when false LayeredBoneBlend_7:
//     base Slot DashSlot #113 over BeforeWeaponMode, blend pose Slot TurretReturn over Slot DashSlot #106 over it.
//     So a DashSlot track at full weight holds BeforeWeaponMode, and all under it, while Turret Aim is false.
//   GroundStillSlot (#47) never holds: it is in the path only with Split true, where UpperBodyOnly updates the
//     cache; WeaponAdditive3 / WeaponTurn / TurretReturn sit beside a consumer that always updates.
//   DefaultSlot (#140) holds whatever the inputs are: the data's own set (Montage.holds_source).
// Moving and Split are the values of this tick's BlueprintUpdateAnimation (split_update runs ahead of the montage
// update and the machines), Turret Aim the pawn's 'Turrent Aim On' as pose.rs takes it.
// BlendListByBool #7 has bResetChildOnActivation and its true child is the UseCachedPose of PreTransform: the reset
// stops at the cache node (SaveCachedPose's Initialize returns at once), so it does not reinitialise the machines
// when Moving turns true. They missed updates while held, though, so those with bReinitializeOnBecomingRelevant
// start again at their entry states in that update, as after any hold (fighter/anim/update.rs relevancy_restart).
// Deviation: Turret Aim is read as it is this tick (the blend of BlendListByBool_6 towards its other side, during
// which both sides are updated, is not modelled).
// Unverified: "full weight" is MontageState::full_weight (the blend in over, no blend out begun), as for the shared
// rule.
const HOLD_TRANSFORM: u8 = 1;
const HOLD_EVADE: u8 = 2;
const HOLD_DASH: u8 = 4;

/// `holdBits`: which of the three slots the montage has a track in.
fn hold_bits(m: &Montage) -> u8 {
    let mut v = 0;
    for s in &m.slots {
        match s.slot.as_str() {
            "Transform" => v |= HOLD_TRANSFORM,
            "Evade" => v |= HOLD_EVADE,
            "DashSlot" => v |= HOLD_DASH,
            _ => {}
        }
    }
    v
}

/// `holds`: the instance (the main montage or the dash montage) at full weight holds the Navigation cache.
fn holds(w: &World, f: FighterId, s: Option<&MontageState>, ga: &GiantAnim, turret_aim: bool) -> bool {
    let Some(s) = s else { return false };
    let Some(m) = instance_data(&w.data, w, f, s.montage.uid) else { return false };
    if !s.full_weight(m) {
        return false;
    }
    let b = hold_bits(m);
    (b & HOLD_TRANSFORM != 0 && !ga.moving) || (b & HOLD_EVADE != 0 && !ga.split) || (b & HOLD_DASH != 0 && !turret_aim)
}

/// `giantLayerHeld` (CharHooks.layer_held).
pub fn layer_held(w: &World, f: FighterId) -> bool {
    let Some(gs) = giant_of(w, f) else { return false };
    let Some(ga) = &gs.giant_anim else { return false };
    let ms = &w.fighter(f).montages;
    holds(w, f, ms.montage.as_ref(), ga, gs.turret_aim_on)
        || holds(w, f, ms.dash_montage.as_ref(), ga, gs.turret_aim_on)
}

/// `splitUpdate`: BlueprintUpdateAnimation's part for the split, once a mesh tick ahead of the montage update.
/// Nothing for a Giant whose GiantAnim is not made yet.
pub fn split_update(w: &mut World, f: FighterId, dilation: Real) {
    let fi = w.fighter(f);
    let falling = !fi.body.on_floor;
    let attacking = is_attacking(w, f);
    let (vx, facing) = (fi.body.vx, fi.body.facing_dir);
    let walk = data_of(w, f).def.max_walk;
    // C017_AnimBP DownPressed (@4554; written by EventDownPressed @15592, the pawn's down press and release).
    // Unverified: taken as the stick at or under the press's -0.4 (BaseFighter @120863), without the release's own
    // threshold or the tests inside DownPressedLogic; the event's sender is not read.
    let down = fi.controls.inp.y <= -0.4 || fi.controls.inp.down_btn;
    let Some(ga) = giant_mut(w, f).and_then(|g| g.giant_anim.as_mut()) else { return };
    if attacking {
        let a = min(max((vx * facing + crate::math::narrow(1.5 * walk)) / crate::math::narrow(3.0 * walk), 0.0), 1.0);
        let target = if ga.falling { 0.0 } else { FALL_LEAN - 2.0 * FALL_LEAN * (if down { 1.0 } else { a }) };
        ga.lean = finterp_to(ga.lean, target, DT * dilation, if ga.falling { 2.5 } else { 1.5 });
    } else {
        ga.lean = 0.0;
    }
    ga.falling = falling;
    ga.moving = vx.abs() > MOVING;
    ga.back = (vx < 0.0) != (facing < 0.0);
    ga.split = ga.split_active || (ga.jump_check && falling);
}

/// `leanBack`: TwoWayBlend_4's alpha.
pub fn lean_back(ga: &GiantAnim) -> Real {
    min(max(2.0 * ga.lean, 0.0), FALL_LEAN) / FALL_LEAN
}

/// `leanForward`: TwoWayBlend_3's alpha.
pub fn lean_forward(ga: &GiantAnim) -> Real {
    -min(max(2.0 * ga.lean, -FALL_LEAN), 0.0) / FALL_LEAN
}

/// The slot tracks of a pose being made, in play order (`Fighter.layersInto`'s four lists): the slot, the clip, the
/// time in it, the instance's weight.
#[derive(Debug, Default)]
pub struct Layers {
    pub slot: Vec<String>,
    pub anim: Vec<String>,
    pub time: Vec<Real>,
    pub w: Vec<Real>,
}

impl Layers {
    /// The count of tracks listed.
    pub fn len(&self) -> usize {
        self.slot.len()
    }

    /// No track listed.
    pub fn is_empty(&self) -> bool {
        self.slot.is_empty()
    }
}

/// `navTracksInto`: the tracks of the default group's montage when it is one of these (`nav_pose`), appended to l
/// as `layers_into` lists the grouped instances' (the track's segment at the instance's position, the time in its
/// clip, the instance's weight).
pub fn nav_tracks_into(w: &World, f: FighterId, l: &mut Layers) {
    let Some(s) = &w.fighter(f).montages.montage else { return };
    if s.terminated {
        return;
    }
    let Some(m) = instance_data(&w.data, w, f, s.montage.uid) else { return };
    if !nav_pose(m) {
        return;
    }
    tracks_into(s, m, crate::math::narrow(s.weight(m)), l);
}

/// `tracksInto`: the instance's tracks at its weight mw appended to l (nothing at a weight up to 1e-5).
pub fn tracks_into(s: &MontageState, m: &Montage, mw: Real, l: &mut Layers) {
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated test, kept for its NaN result")]
    if !(mw > MIN_W) {
        return;
    }
    for t in &m.slots {
        let Some(g) = s.seg_at(&t.segments) else { continue };
        let seg = &t.segments[g];
        l.slot.push(t.slot.clone());
        l.anim.push(seg.anim.clone());
        l.time.push(crate::math::narrow(s.seg_time(seg)));
        l.w.push(mw);
    }
}
