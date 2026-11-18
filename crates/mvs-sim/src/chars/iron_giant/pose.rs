//! C017_AnimBP's layers over the default group's pose, for the sim's shapes:
//! the hurt capsules, the hit spheres and boxes and the sockets ride the mesh's final pose, and in weapon mode most
//! of that pose is not the main montage's (the nodes' values from C017_AnimBP's CDO; the slot and blend rules as the
//! view's pose states them, which draws the same graph on the whole skeleton). The pose is a function of the
//! fighter's state: the clip and time of the default group's pose (the fighter's anim / anim time), the grouped
//! montage instances (`layers_into`), the pawn's and the AnimBP's variables, and the two the graph keeps itself
//! (`GiantAnim`, sim state). The movement split's variables and tracks are `split.rs`.
//!
//! The graph, innermost first, and what each layer moves that the sim reads (the shapes' bones, 82 with their
//! parents; `Skel::masks`):
//!   base            the default group's pose (BeforeWeaponMode: DefaultSlot and the others over the Navigation
//!                   machine), with Base_AnimBP's hit rotation of fml_un_COM in it (the hit pitch).
//!   the split       under a montage of the split's slots (split.rs: the base clip is Navigation's then, the
//!                   montage's tracks are listed with the grouped instances'): LayeredBoneBlend_11 at
//!                   fml_un_C_spine_a over the GroundStillSlot / ModifyBone_6 / Navigation base with the
//!                   UpperBodyOnly track leaned to SpinLeanB / SpinLeanF, or Slot Evade while Split is off: every
//!                   shape.
//!   LBB_1           C017_WeaponMode_Override on the branch fml_un_tentacle_att, local space, weight AlternateMode:
//!                   the three tentacle tips (the fast shot's muzzle sockets fml_un_R / L / C_tentacle8).
//!   LBB_10          the same clip on the branches of the two fml_un_?_uparm_att, fml_un_C_neck_a and the weapon
//!                   head, mesh space rotation, weight AlternateMode AND NOT Is Hit Stunned: the arms' and hands'
//!                   hurt capsules and hit spheres (HurtBox_Arm* / Hand*, the Hand, Shoulder and Elbow hit shapes),
//!                   the wrist, hold and weapon sockets, the head's shapes (HurtBox_Head, Head, EnergyDischarge) and
//!                   sockets.
//!   LBB_9           the two clavicle plate offset bones (BlendDepth 1): no shape rides them (not posed here).
//!   ModifyBone_5    fml_un_C_spine_a turned half a turn about the vertical (BMM_Additive, BCS_WorldSpace, yaw
//!                   +-180), alpha Turret Aim AND Torso Flip Alpha: every shape from the spine up (51 of the 82
//!                   bones; the legs, the pelvis and fml_un_COM's own point stay).
//!   ApplyAdditive_5 the plasma attack's additive (Slot WeaponAdditive / AimUp / AimDown mixed by AimDirection):
//!                   spine, right arm and weapon bones.
//!   LBB_5           the pilot's tentacle pose (the looping C017_WeaponMode_Override_Tenticle with Slot
//!                   TenticleAdditive on it, under Slot TenticleDefault) on the tentacle branch, mesh space rotation,
//!                   weight TenticleOverride: the three muzzle sockets.
//!   ApplyAdditive_3 C017_TenticleAim_BlendSpace's additive, alpha TenticleOverride: the three muzzle sockets.
//!   ApplyAdditive_4 the cannon attack's additive (Slot WeaponAdditive2 / AimUp2 mixed by AimAlpha2): spine and
//!                   arms (the cannon's socket fml_un_R_wrist).
//!   LBB_6 / LBB_8   C017_Weapon_Turn_Montage (Slot WeaponAdditive3, a full pose) from fml_un_C_spine_a up, the
//!                   tentacle chain kept in mesh space: every shape from the spine up.
//!   LBB_4           the grab's hold arm: SequencePlayer_7 (C017_Hold_Override, looping, a still clip) on the
//!                   branches fml_un_R_uparm_att and HitCollision01, mesh space rotation, weight HoldAlpha x
//!                   HoldingBuffValid (@10808-10891): the right arm's shapes and the hold sockets
//!                   (fml_un_R_middle_base, where the held fighter is at that weight: Buff_HeldBy_C017's track).
//!   ApplyAdditive_1 Slot FullBodyAdditive (EnergyDischarge, the missile launch): whole body; the EnergyDischarge
//!                   hit shape on the head.
//!   ApplyAdditive   Slot DiscAdditive (the energy disc): the right arm's weapon bones.
//!   LBB_2           C017_WeaponMode_Override on fml_un_C_neck_a's branch, local space, weight AlternateMode: the
//!                   head's shapes and sockets.
//!   ApplyAdditive_6 Slot Additive (the additive hit reacts): whole body.
//! Every additive slot's source is C017_Additive_HitReact_Base, the additive identity, whose scale is 0: a slot's
//! instances' scale deltas do not reach the mesh.
//!   the Transform   LayeredBoneBlend #9 over PreTransform, the outermost: Slot Transform on the whole body, or
//!   stage           from fml_un_C_spine_a up while Moving (C017_Item_Eat_Montage, the two WeaponMode montages).
//! Not here (the pose keeps the layers under them):
//!   - the hold-bone ModifyBone_4 / _3 (VB Fighter_Hold_Bone to HoldLocation, the main mesh's fml_un_R_middle_base,
//!     at HoldAlpha x HoldingBuffValid): no shape rides the virtual bone; the hold buff reads the socket's track
//!     itself at that weight (Buff_HeldBy_C017's track, hold.rs).
//!   - Deviation: BlendListByBool_4 (EyeLaserActive), the DashSlot / TurretReturn branches (BlendListByBool_6,
//!     LayeredBoneBlend_7 / _8's base), IsDestroyed and the spring bones: a dash or TurretReturn montage in the
//!     default pose lies under these layers whole.
//!   - Deviation: a stopped split montage's blend-out is not under the shapes (an interrupted instance leaves the
//!     pose at once, as every character's main montage does here; the view keeps it).
//!   - C017_WeaponMode_Override does not move on the shapes' bones (the generator's flags: no track changes), so its
//!     four players' clocks do not reach the shapes and are not kept; a clip that moved would be warned (`still`).
//!
//! Deviations and guesses of what is here:
//!   - the port's pose of a fighter facing left is the mirror image of the one facing right, so the half turn of
//!     ModifyBone_5 is the same turn for both, and a named R / L socket is the other one for a fighter facing left
//!     (its callers swap them);
//!   - Unverified: the tentacle loop's player is advanced by the mesh tick's dilated step while TenticleOverride
//!     (FAnimNode_LayeredBoneBlend updates a blend pose only at a relevant weight: stock, FUN_142493bf0 not read)
//!     and keeps its time while it is not;
//!   - Unverified: a TwoWayBlend's alpha is taken as clamped to 0..1, with A alone at 0 and B alone at 1
//!     (FAnimNode_TwoWayBlend, stock), and the blend of two additive poses as BlendPosesTogether's kernels;
//!   - Unverified: an ApplyAdditive node at alpha 1 accumulates its additive input as one more additive;
//!   - Unverified: the tentacle aim's variables are updated from the pose and the reticle as they are when the mesh
//!     tick starts (the pose the last evaluation left; the reticle's depth taken as 0), with the fighter's place of
//!     this tick.

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated comparisons, kept for their NaN result")]

use super::split::{Layers, lean_back, lean_forward, nav_tracks_into, split_update};
use super::state::{giant_mut, giant_of};
use crate::Real;
use crate::State;
use crate::consts::DT;
use crate::fighter::anim::grouped::layers_into;
use crate::fighter::anim::{SkelWork, hit_pitch, put_skel_work, take_skel_work};
use crate::fighter::common::warn_parts;
use crate::fighter::{FighterId, has_grouped, in_hitstun, mesh_wx, mesh_wy, tenticle_override};
use crate::math::ue::{SMALL_NUMBER, rotator_quaternion, safe_normal, size2d};
use crate::math::{max, min};
use crate::registry::CharHooks;
use crate::world::World;
use mvs_anim::skel_pose::{
    Skel, SkelClip, SkelPose, accumulate, add_weighted_pose, blend_delta, blend_rows, forward, identity_delta,
    layered_in_place, normalize_rotations, replace, sample_clip, turn_bone, weight_pose, write_shapes,
};

/// `GiantAnim`: C017_AnimBP's own variables and clock under the shapes, on the Giant as `GiantState.giant_anim`
/// (made by `init`, pawn.rs: all zero).
#[derive(State, Debug, Default)]
pub struct GiantAnim {
    /// SequencePlayer_8's time in C017_WeaponMode_Override_Tenticle.
    pub loop_t: Real,
    /// TenticleAimVector's Y and Z (the build's axes: Y = -x).
    pub aim_vy: Real,
    pub aim_vz: Real,
    /// TenticleAimDistanceAlpha.
    pub aim_dist: Real,
    /// AimAlpha2 (BPI_SetAimAlpha2).
    pub aim_alpha2: Real,
    // The movement split's variables (split.rs)
    /// MovementSplitActive (AnimNotify_C017_MovementSplit).
    pub split_active: bool,
    /// SplitMovementJumpCheck (AnimNotify_C017_MovementSplitJumpCheck).
    pub jump_check: bool,
    /// The handlers' value at the last update.
    pub split: bool,
    /// IsFalling.
    pub falling: bool,
    /// Moving.
    pub moving: bool,
    /// ModifyBone_6's turn at the last update: he moves against his facing.
    pub back: bool,
    /// AttackLean.
    pub lean: Real,
}

const WEAPON_OVERRIDE: &str = "C017_WeaponMode_Override";
const TENTACLE_LOOP: &str = "C017_WeaponMode_Override_Tenticle";
const HEAD: &str = "fml_un_C_head";
const SPINE: &str = "fml_un_C_spine_a";
/// SequencePlayer_7.
const HOLD_CLIP: &str = "C017_Hold_Override";
/// LayeredBoneBlend_4 BranchFilters (BlendDepth 0).
const HOLD_BRANCHES: [&str; 2] = ["fml_un_R_uparm_att", "HitCollision01"];
/// ZERO_ANIMWEIGHT_THRESH.
const MIN_W: Real = 1e-5;
/// DAT_143ce1700.
const FULL_W: Real = 0.99999;
/// DAT_144210d9c: SlotEvaluatePose scales the weights past this total.
const OVER_W: Real = 1.00001;

/// The hooks of this file in Iron Giant's CharHooks (`mod.rs` character: animUpdate giantAnimUpdate, pose
/// giantShapes).
pub fn hooks(h: &mut CharHooks) {
    h.anim_update = Some(anim_update);
    h.pose = Some(giant_shapes);
}

/// The skeleton's clip of that name.
fn clip<'a>(sk: &'a Skel, name: &str) -> Option<&'a SkelClip> {
    sk.clip_index.get(name).map(|&i| &sk.clips[i])
}

/// One of the skeleton's masks (the boxes bind's: spine, tentacle, arm, neck).
fn mask<'a>(sk: &'a Skel, name: &str) -> &'a [f32] {
    sk.masks.get(name).unwrap_or_else(|| panic!("C017 pose: the box index's skel has no mask {name}"))
}

/// A bone's index, -1 when the skeleton has none of that name (`turn_bone` writes nothing then).
fn bone(sk: &Skel, name: &str) -> i32 {
    sk.bone.get(name).map_or(-1, |&b| i32::try_from(b).expect("a bone index"))
}

/// `holdMask`: LayeredBoneBlend_4's bones, made from the skeleton's parents (the skel's own masks are the boxes
/// bind's, which has no entry for this node). `None` when a branch bone is not in the skeleton.
// Made at each use (a walk of the skeleton's bones), not cached per skeleton.
fn hold_mask(sk: &Skel) -> Option<Vec<f32>> {
    let tops = [sk.bone.get(HOLD_BRANCHES[0])?, sk.bone.get(HOLD_BRANCHES[1])?];
    let mut m = vec![0.0f32; sk.n];
    for b in 0..sk.n {
        let pa = sk.parents[b];
        let on = tops.contains(&&b) || (pa >= 0 && m[usize::try_from(pa).expect("a parent index")] != 0.0);
        m[b] = if on { 1.0 } else { 0.0 };
    }
    Some(m)
}

/// The pose being made: the fighter, the skeleton and the grouped instances' (and the split's) slot tracks in play
/// order.
struct Graph<'a> {
    w: &'a World,
    sk: &'a Skel,
    l: Layers,
}

impl Graph<'_> {
    fn warn(&self, parts: &[&str]) {
        warn_parts(self.w, parts);
    }

    fn has(&self, slot: &str) -> bool {
        self.l.slot.iter().any(|s| s == slot)
    }

    /// `slotWeights`: GetSlotWeight FUN_142856c40 and SlotEvaluatePose FUN_1428641a0's weights for a slot: `None`
    /// when the node passes its source through; else (norm, source_w): norm the factor on every instance's weight
    /// (Total > 1.00001), source_w the source's weight (1 - the full poses' share, the share over Total when
    /// Total > 1).
    fn slot_weights(&self, slot: &str) -> Option<(Real, Real)> {
        let (mut total, mut full) = (0.0, 0.0);
        for k in 0..self.l.len() {
            if self.l.slot[k] != slot {
                continue;
            }
            total += self.l.w[k];
            if clip(self.sk, &self.l.anim[k]).is_some_and(|c| !c.additive) {
                full += self.l.w[k];
            }
        }
        if total > 1.0 {
            full /= total;
        }
        let source_w = min(max(1.0 - full, 0.0), 1.0);
        let norm = if total > OVER_W { 1.0 / total } else { 1.0 };
        if total > MIN_W { Some((norm, source_w)) } else { None }
    }

    /// `accumulateSlot`: the additive instances of a slot accumulated on p in play order, each at its weight x norm.
    /// scale: p is a full pose (a full-pose clip of the slot is `slot_pose`'s then); else p stands for an additive
    /// pose (scale 0).
    fn accumulate_slot(&self, slot: &str, p: &mut SkelPose, scale: bool, d: &mut SkelPose, norm: Real) {
        for k in 0..self.l.len() {
            if self.l.slot[k] != slot {
                continue;
            }
            let anim = &self.l.anim[k];
            let c = clip(self.sk, anim);
            let Some(c) = c.filter(|c| c.additive) else {
                if c.is_none() {
                    self.warn(&["C017 pose: ", anim, ": no such clip"]);
                } else if !scale {
                    self.warn(&["C017 pose: ", anim, ": a full pose in the additive slot ", slot]);
                }
                continue;
            };
            sample_clip(self.sk, c, self.l.time[k], d);
            accumulate(d, &c.bones, self.l.w[k] * norm, scale, p);
        }
    }

    /// `slotAdditive`: a slot node whose source is the additive identity, applied by its ApplyAdditive node
    /// (alpha 1) to p: each instance accumulated on p, the scale left (the module comment).
    fn slot_additive(&self, slot: &str, p: &mut SkelPose, d: &mut SkelPose) {
        if let Some((norm, _)) = self.slot_weights(slot) {
            self.accumulate_slot(slot, p, false, d, norm);
        }
    }

    /// `slotDelta`: the same slot node's output as a delta pose, for a node that blends additive poses before one
    /// is applied.
    fn slot_delta(&self, slot: &str, out: &mut SkelPose, d: &mut SkelPose) {
        identity_delta(out);
        if let Some((norm, _)) = self.slot_weights(slot) {
            self.accumulate_slot(slot, out, false, d, norm);
        }
    }

    /// `slotPose`: a slot node over the full pose p, in place: the full-pose instances in play order, each at its
    /// weight, then the source p at its own weight (only above 1e-5), blended (BlendPosesTogetherIndirect); then
    /// the additive instances. m, b, d: scratch poses.
    fn slot_pose(&self, slot: &str, p: &mut SkelPose, m: &mut SkelPose, b: &mut SkelPose, d: &mut SkelPose) {
        let Some((norm, source_w)) = self.slot_weights(slot) else { return };
        let mut first = true;
        for k in 0..self.l.len() {
            if self.l.slot[k] != slot {
                continue;
            }
            let Some(c) = clip(self.sk, &self.l.anim[k]).filter(|c| !c.additive) else { continue };
            sample_clip(self.sk, c, self.l.time[k], m);
            if first {
                first = false;
                weight_pose(m, self.l.w[k] * norm, b);
            } else {
                add_weighted_pose(m, self.l.w[k] * norm, b);
            }
        }
        if !first {
            if source_w > MIN_W {
                add_weighted_pose(p, source_w, b);
            }
            normalize_rotations(b);
            p.copy(b);
        }
        self.accumulate_slot(slot, p, true, d, norm);
    }

    /// `twoWay`: FAnimNode_TwoWayBlend of two additive slot nodes into a (alpha clamped; the module comment): A's
    /// output at 0, B's at 1, else A x (1 - alpha) + B x alpha.
    #[allow(clippy::too_many_arguments)]
    fn two_way(
        &self,
        a: &mut SkelPose,
        slot_b: &str,
        alpha: Real,
        b: &mut SkelPose,
        t: &mut SkelPose,
        d: &mut SkelPose,
    ) {
        let x = min(max(alpha, 0.0), 1.0);
        if !(x > MIN_W) {
            return;
        }
        self.slot_delta(slot_b, b, d);
        if x >= 1.0 - MIN_W {
            a.copy(b);
            return;
        }
        weight_pose(a, 1.0 - x, t);
        add_weighted_pose(b, x, t);
        normalize_rotations(t);
        a.copy(t);
    }
}

/// `twoWayFull`: FAnimNode_TwoWayBlend of two full poses: a alone at alpha <= 1e-5, b alone from 0.99999, else
/// BlendTwoPosesTogether (a x (1 - alpha), b added x alpha on the side of the sum, rotations normalised). The result
/// is in a; t: scratch.
fn two_way_full(a: &mut SkelPose, b: &SkelPose, alpha: Real, t: &mut SkelPose) {
    if !(alpha > MIN_W) {
        return;
    }
    if alpha >= FULL_W {
        a.copy(b);
        return;
    }
    weight_pose(a, 1.0 - alpha, t);
    add_weighted_pose(b, alpha, t);
    normalize_rotations(t);
    a.copy(t);
}

/// The scratch of [`compose`]: the World's skeleton scratch (its FK buffers and five poses) and the two more poses
/// the graph uses.
struct Work {
    k: SkelWork,
    d: SkelPose,
    t: SkelPose,
}

impl Work {
    fn take(w: &World, n: usize) -> Work {
        Work { k: take_skel_work(w, n), d: SkelPose::new(n), t: SkelPose::new(n) }
    }

    fn put(self, w: &World) {
        put_skel_work(w, self.k);
    }
}

/// `compose`: the graph on the shapes' bones into `k.k.p[0]`. False: nothing lies over the default pose (and
/// `force` is off), the box tracks of the clip are the pose.
#[allow(clippy::too_many_lines, reason = "the graph's layers in their order, as the graph composes them")]
fn compose(w: &World, f: FighterId, sk: &Skel, wk: &mut Work, force: bool) -> bool {
    let mut l = Layers::default();
    if has_grouped(w, f) {
        layers_into(w, f, &mut l.slot, &mut l.anim, &mut l.time, &mut l.w);
    }
    let lg = l.len();
    nav_tracks_into(w, f, &mut l);
    // the base clip is Navigation's and a montage of the split's slots lies over it
    let nav = l.len() > lg;
    let fi = w.fighter(f);
    let facing = fi.body.facing_dir;
    let gs = giant_of(w, f);
    let wm = gs.is_some_and(|g| g.alternate_mode);
    let over = tenticle_override(w, f);
    let flip = gs.is_some_and(|g| g.turret_aim_on && (facing < 0.0) != g.turret_aim_start_left);
    let hold_w = gs.map_or(0.0, |g| g.hold_alpha * if g.hold_valid { 1.0 } else { 0.0 });
    if !force && !wm && !over && l.is_empty() && !flip && !(hold_w > MIN_W) {
        return false;
    }
    let Some(base) = clip(sk, w.names.str(fi.anim.anim)).filter(|c| !c.additive) else { return false };
    let ga = gs.and_then(|g| g.giant_anim.as_ref());
    let g = Graph { w, sk, l };
    let Work { k: SkelWork { sc, p: [p, ww, s, m, b], .. }, d, t } = wk;
    sample_clip(sk, base, fi.anim.anim_time, p);
    // Base_AnimBP's ModifyBone on fml_un_COM (BMM_Replace, component space): the pitch about the depth axis (the
    // boxes' turn has the rotator's sense).
    if let Some(pitch) = hit_pitch(w, f)
        && sk.sk.com >= 0
    {
        // FRotator(pitch, 0, 0).Quaternion() = (0, -sin P/2, 0, cos P/2) in UE space: the skeleton's depth axis
        let q = rotator_quaternion([pitch, 0.0, 0.0]);
        turn_bone(sk, p, sk.sk.com, 0.0, 0.0, -q[1], q[3], false);
    }
    // BeforeUperbodySplit and the Evade bypass (split.rs; p is Navigation). Scratch: s the blend pose, ww a lean
    // track.
    if nav && let Some(ga) = ga {
        if ga.split {
            s.copy(p);
            g.slot_pose("UpperBodyOnly", s, m, b, d);
            let (back, fwd) = (lean_back(ga), lean_forward(ga));
            if back > MIN_W {
                ww.copy(p);
                g.slot_pose("SpinLeanB", ww, m, b, d);
                two_way_full(s, ww, back, t); // TwoWayBlend_4
            }
            if fwd > MIN_W {
                ww.copy(p);
                g.slot_pose("SpinLeanF", ww, m, b, d);
                two_way_full(s, ww, fwd, t); // TwoWayBlend_3
            }
            if !ga.falling {
                if !ga.moving {
                    g.slot_pose("GroundStillSlot", p, m, b, d); // BlendListByBool_3's true side
                } else if sk.sk.com >= 0 {
                    // ModifyBone_6: identity toward the facing, half a turn about the vertical against it
                    let (y, wq) = if ga.back { (1.0, 0.0) } else { (0.0, 1.0) };
                    turn_bone(sk, p, sk.sk.com, 0.0, y, 0.0, wq, false);
                }
            }
            layered_in_place(sk, sc, p, s, mask(sk, "spine"), 1.0); // LayeredBoneBlend_11
        } else {
            g.slot_pose("Evade", p, m, b, d); // BlendListByBool_2's false side
        }
    }
    // item 16: the weapon mode override on the tentacle chain, then (not hit stunned) the arms, the neck and head
    let weapon = if wm { clip(sk, WEAPON_OVERRIDE) } else { None };
    if wm && !weapon.is_some_and(|c| c.still) {
        if weapon.is_some() {
            g.warn(&["C017 pose: ", WEAPON_OVERRIDE, " moves on the shapes' bones (its players' clocks are not kept)"]);
        } else {
            g.warn(&["C017 pose: ", WEAPON_OVERRIDE, ": no such clip"]);
        }
    }
    if let Some(weapon) = weapon {
        sample_clip(sk, weapon, 0.0, ww);
        replace(p, ww, mask(sk, "tentacle")); // LayeredBoneBlend_1
        if !in_hitstun(w, f) {
            layered_in_place(sk, sc, p, ww, mask(sk, "arm"), 1.0); // LayeredBoneBlend_10
        }
    }
    // ModifyBone_5: the torso's half turn about the vertical
    if flip {
        turn_bone(sk, p, bone(sk, SPINE), 0.0, 1.0, 0.0, 0.0, true);
    }
    // item 15, ApplyAdditive_5: the plasma attack
    if g.has("WeaponAdditive") || g.has("WeaponAdditiveAimUp") || g.has("WeaponAdditiveAimDown") {
        let aim = fi.pose.aim_direction;
        g.slot_delta("WeaponAdditive", m, d);
        g.two_way(m, "WeaponAdditiveAimUp", aim, b, t, d); // TwoWayBlend_2 (AimDirection)
        g.two_way(m, "WeaponAdditiveAimDown", -aim, b, t, d); // TwoWayBlend_1 (AimDirectionInverted)
        accumulate(m, &sk.all, 1.0, false, p);
    }
    // item 14, LayeredBoneBlend_5: the pilot's tentacle pose
    let lp = if over { clip(sk, TENTACLE_LOOP) } else { None };
    if over && lp.is_none() {
        g.warn(&["C017 pose: ", TENTACLE_LOOP, ": no such clip"]);
    }
    if let Some(lp) = lp {
        sample_clip(sk, lp, ga.map_or(0.0, |a| f64::from(a.loop_t)), s);
        g.slot_additive("TenticleAdditive", s, d); // ApplyAdditive_2
        g.slot_pose("TenticleDefault", s, m, b, d); // In / Out_Tenticle over it
        layered_in_place(sk, sc, p, s, mask(sk, "tentacle"), 1.0);
    }
    // item 13, ApplyAdditive_3 (alpha TenticleOverride as 0 / 1): the aim blend space's additive. Inputs
    // (@6558-6974): y' = TenticleAimVector.Y toward the facing, negated when Turret Aim and Torso Flip Alpha;
    // X = Lerp(0, y', TenticleAimDistanceAlpha), Y = Lerp(0, TenticleAimVector.Z, TenticleAimDistanceAlpha).
    let aim = if over { sk.blend_index.get("tenticleAim").map(|&i| &sk.blend[i]) } else { None };
    if over && !aim.is_some_and(|a| a.ok) {
        g.warn(&["C017 pose: the tentacle aim blend space is not in the data, or a sample of it moves"]);
    }
    if let Some(aim) = aim.filter(|a| a.ok) {
        let mut y = ga.map_or(0.0, |a| if facing < 0.0 { a.aim_vy } else { -a.aim_vy });
        if flip {
            y = -y;
        }
        let a = ga.map_or(0.0, |a| a.aim_dist);
        blend_rows(sc, aim, y * a, ga.map_or(0.0, |a| a.aim_vz) * a);
        blend_delta(sk, sc, aim, b, m);
        accumulate(b, &aim.bones, 1.0, true, p);
    }
    // item 12, ApplyAdditive_4: the cannon attack
    if g.has("WeaponAdditive2") || g.has("WeaponAdditiveAimUp2") {
        g.slot_delta("WeaponAdditive2", m, d);
        g.two_way(m, "WeaponAdditiveAimUp2", ga.map_or(0.0, |a| a.aim_alpha2), b, t, d);
        accumulate(m, &sk.all, 1.0, false, p);
    }
    // item 11: the turn montage from spine_a up, the tentacle chain kept in mesh space
    if g.has("WeaponAdditive3") {
        s.copy(p);
        g.slot_pose("WeaponAdditive3", s, m, b, d); // over Slot WeaponTurn (no montage has that track)
        layered_in_place(sk, sc, s, p, mask(sk, "tentacle"), 1.0); // LayeredBoneBlend_6
        layered_in_place(sk, sc, p, s, mask(sk, "spine"), 1.0); // LayeredBoneBlend_8
    }
    // item 11, LayeredBoneBlend_4: the hold arm over that (AfterHold)
    if hold_w > MIN_W {
        let hold = clip(sk, HOLD_CLIP);
        match (hold, hold_mask(sk)) {
            (Some(hold), Some(hm)) => {
                if !hold.still {
                    g.warn(&["C017 pose: ", HOLD_CLIP, " moves on the shapes' bones (its player's clock is not kept)"]);
                }
                sample_clip(sk, hold, 0.0, s);
                layered_in_place(sk, sc, p, s, &hm, hold_w);
            }
            (Some(_), None) => {
                g.warn(&["C017 pose: a bone of ", HOLD_BRANCHES[0], " / ", HOLD_BRANCHES[1], " is not in the skeleton"])
            }
            (None, _) => g.warn(&["C017 pose: ", HOLD_CLIP, ": no such clip"]),
        }
    }
    // item 10: the full body and disc additives, then the weapon mode override on the neck
    g.slot_additive("FullBodyAdditive", p, d); // ApplyAdditive_1
    g.slot_additive("DiscAdditive", p, d); // ApplyAdditive
    if weapon.is_some() {
        replace(p, ww, mask(sk, "neck")); // LayeredBoneBlend_2
    }
    // item 9: the additive hit reacts, the outermost additive
    g.slot_additive("Additive", p, d); // ApplyAdditive_6
    // The Transform stage: LayeredBoneBlend #9 (spine_a, weight 1) of Slot Transform over this pose, its base the
    // same slot (the whole body is the track's) or, while Moving, this pose.
    if nav
        && let Some(ga) = ga
        && g.has("Transform")
    {
        s.copy(p);
        g.slot_pose("Transform", s, m, b, d);
        if ga.moving {
            layered_in_place(sk, sc, p, s, mask(sk, "spine"), 1.0);
        } else {
            p.copy(s);
        }
    }
    true
}

/// `giantShapes` (CharHooks.pose): the graph's pose under the shapes; the result's shapes on bones replace s's
/// (`write_shapes`). False: the box tracks of the clip.
pub fn giant_shapes(w: &World, f: FighterId, s: &mut [f32]) -> bool {
    let Some(sk) = w.fixed.skels.get(&w.fighter(f).ch) else { return false };
    let mut wk = Work::take(w, sk.n);
    let posed = compose(w, f, sk, &mut wk, false);
    if posed {
        write_shapes(sk, &mut wk.k.sc, &wk.k.p[0], s);
    }
    wk.put(w);
    posed
}

/// `giantAnimUpdate` (CharHooks.anim_update).
/// pre: BlueprintUpdateAnimation's tentacle aim (@13891-14786, only while Aim Target is valid: the pilot's
/// reticle): TenticleAimVector = the unit vector from the head socket fml_un_C_head + (0, 0, 250) to the reticle
/// (GetDirectionUnitVector: zero under a squared length of 1e-8), TenticleAimDistanceAlpha = clamp01(the YZ
/// distance from the socket to the reticle / 1200).
/// after the montage update: SequencePlayer_8 (looping, PlayRate 1, the clip's RateScale) under LayeredBoneBlend_5.
pub fn anim_update(w: &mut World, f: FighterId, dilation: Real, pre: bool) {
    if giant_of(w, f).and_then(|g| g.giant_anim.as_ref()).is_none() {
        return;
    }
    if pre {
        split_update(w, f, dilation);
    }
    let Some(sk) = w.fixed.skels.get(&w.fighter(f).ch).cloned() else { return };
    if pre {
        let Some(t) = giant_of(w, f).and_then(|g| g.ally_turret.as_ref()) else { return };
        let (aim_x, aim_y) = (t.aim_x, t.aim_y);
        let Some(&head) = sk.bone.get(HEAD) else { return };
        if !t.reticle {
            return;
        }
        let mut wk = Work::take(w, sk.n);
        if !compose(w, f, &sk, &mut wk, true) {
            wk.put(w);
            return;
        }
        let SkelWork { sc, p, .. } = &mut wk.k;
        forward(&sk, sc, &p[0]);
        let fi = w.fighter(f);
        let (facing, size) = (fi.body.facing_dir, fi.body.size_scale);
        let k = sk.index.mesh_scale * 100.0 * f64::from(size);
        let hx = mesh_wx(w, f) + crate::math::narrow(sc.wt[head * 3] * k * f64::from(facing));
        let hz = mesh_wy(w, f)
            + crate::math::narrow(
                (sc.wt[head * 3 + 1] * sk.index.mesh_scale * 100.0 + sk.index.mesh_z) * f64::from(size),
            );
        let depth = sc.wt[head * 3 + 2] * k;
        wk.put(w);
        let dy = -(aim_x - hx);
        let dz = aim_y - (hz + 250.0);
        let n = safe_normal([crate::math::narrow(depth), dy, dz], SMALL_NUMBER); // GetDirectionUnitVector
        let dist = min(max(size2d([aim_x - hx, aim_y - hz]) / 1200.0, 0.0), 1.0);
        let ga = giant_mut(w, f).and_then(|g| g.giant_anim.as_mut()).expect("the Giant's anim (checked above)");
        ga.aim_vy = n[1];
        ga.aim_vz = n[2];
        ga.aim_dist = dist;
        return;
    }
    if !tenticle_override(w, f) {
        return;
    }
    let Some(lp) = clip(&sk, TENTACLE_LOOP) else { return };
    let rate = sk
        .index
        .anims
        .get(TENTACLE_LOOP)
        .unwrap_or_else(|| panic!("C017 pose: the box index has no anim {TENTACLE_LOOP}"))
        .rate_scale;
    let len = lp.length;
    let ga = giant_mut(w, f).and_then(|g| g.giant_anim.as_mut()).expect("the Giant's anim (checked above)");
    ga.loop_t += DT * dilation * crate::math::narrow(rate);
    if len > 0.0 {
        while ga.loop_t >= crate::math::narrow(len) {
            ga.loop_t -= crate::math::narrow(len);
        }
    }
}
