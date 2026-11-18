//! Tom_and_Jerry_AnimBP's rocket arms under the sim's shapes:
//! AnimGraphNode_LayeredBoneBlend under Slot OverrideSlot (the class data: BasePose the aim tree, BlendPoses[0]
//! BlendListByBool_1, LayerSetup[0].BranchFilters tom_un_L_clav / tom_un_R_clav with BlendDepth 0, BlendWeights [1]
//! unbound, bMeshSpaceRotationBlend / bMeshSpaceScaleBlend false): the two clavicles and every bone under them take
//! the layer whole, in local space. The layer is BlendListByBool_1 (bActiveValue <- RocketControlBool,
//! BPI_SetControlBool @16129; BlendTime [0.1, 0.1]) over Slot ControlL (true) / Slot ControlR (false), each over the
//! aim tree. Only Tom_and_Jerry_Rocket_Ground_Montage has ControlL / ControlR tracks. The slot walk
//! (`fighter::anim` graph_slots) takes the node's base alone (aim.rs `slot_input`); this file steps the list (sim
//! state: `TomAndJerryAnim::arms`, which the view reads for the drawn arms) and lays the layer over the shapes' pose
//! (CharHooks.pose), on the bones the boxes bind keeps for the shapes (the skeleton's mask `arms`).
//!
//! The shapes' pose while a ControlL / ControlR track is shown: the aim tree as the slot walk mixed it (the fighter's
//! anim / anim time, then `TomAndJerryPawnState::slot_mix` in order, each pose = lerp(pose, mix, w), here as local
//! transforms: BlendTwoPosesTogether's kernels), the arms' bones from the list's two slot poses mixed by child 0's
//! weight, then the shapes placed on it (`write_shapes`). The hit rotation is null with a montage shown.
//! Deviation: the slot poses are the montage's tracks whole, its blend-in and blend-out weight aside, as every
//! character's shown montage under the shapes (`sample_boxes` without a slot_source hook); the view mixes them over
//! the aim tree by the montage's weight.
//! Deviation: the aim mix of the shapes' other bones is the local-space blend here and the box tracks' lerp outside
//! the rocket's tracks (`sample_boxes`' slot mix).

use super::state::{tj, tj_w};
use crate::Real;
use crate::consts::DELTA_SECONDS;
use crate::fighter::FighterId;
use crate::fighter::anim::montages::{instance_data, state_ref};
use crate::fighter::anim::slot_blend::step_list;
use crate::fighter::anim::update::shown_uid;
use crate::fighter::anim::{SkelWork, SlotPose, put_skel_work, take_skel_work};
use crate::fighter::common::warn_text;
use crate::fighter::control::char_data_in;
use crate::world::World;
use mvs_anim::skel_pose::{
    Skel, SkelClip, SkelPose, add_weighted_pose, normalize_rotations, replace, sample_clip, weight_pose, write_shapes,
};
use mvs_data::fighter::{Montage, Segment, Slot, SlotGraph, SlotGraphNode};

/// The layer's BlendListByBool node in the data's slot graph.
pub const ARMS_LIST: &str = "AnimGraphNode_BlendListByBool_1";
/// BlendPose[0] / [1] of the list.
pub const ARMS_SLOTS: [&str; 2] = ["ControlL", "ControlR"];
const MASK: &str = "arms";
const ZERO_W: Real = 1e-5;
const FULL_W: Real = 0.99999;

/// BlendListByBool_1 of the data's slot graph: its node index and the node, none (with a warning) when the graph lacks
/// it or its two poses are not the Slots ControlL / ControlR.
fn arms_list(w: &World, f: FighterId) -> Option<&SlotGraphNode> {
    let found = arms_node(&char_data_in(&w.data, w, f).fighter.slot_holds.graph);
    if found.is_some() {
        return found;
    }
    warn_text(
        w,
        &format!(
            "Tom and Jerry arms: the data's slot graph has no {ARMS_LIST} over {} / {}: the rocket arms are not posed",
            ARMS_SLOTS[0], ARMS_SLOTS[1]
        ),
    );
    None
}

/// The graph's first BlendListByBool_1 when its two poses are the Slots ControlL / ControlR (no warning).
pub fn arms_node(g: &SlotGraph) -> Option<&SlotGraphNode> {
    let found = g.nodes.iter().find(|n| matches!(n, SlotGraphNode::Bool { node, .. } if node == ARMS_LIST));
    let ok = found.is_some_and(|n| match n {
        SlotGraphNode::Bool { poses, .. } => {
            poses.len() == 2
                && poses.iter().enumerate().all(|(i, &c)| {
                    usize::try_from(c)
                        .ok()
                        .and_then(|c| g.nodes.get(c))
                        .is_some_and(|s| matches!(s, SlotGraphNode::Slot { slot, .. } if slot == ARMS_SLOTS[i]))
                })
        }
        _ => false,
    });
    if ok { found } else { None }
}

/// `armsUpdate` (CharHooks.animUpdate after the montage update; a live pawn: the layer's weight is 1, so the list is
/// updated each update): BlendListByBool_1's FAlphaBlend by the instance's delta, the hit pause's dilation in it. The
/// list's state sits at index 0 of `TomAndJerryAnim::arms`. Called by pawn.rs's hooks.
pub fn arms_update(w: &mut World, f: FighterId, dilation: Real) {
    let data = w.data.clone();
    let g = &char_data_in(&data, w, f).fighter.slot_holds.graph;
    let Some(n) = g.nodes.iter().find(|n| matches!(n, SlotGraphNode::Bool { node, .. } if node == ARMS_LIST)) else {
        // (the check of `arms_list`, with its warning)
        arms_list(w, f);
        return;
    };
    if arms_list(w, f).is_none() {
        return;
    }
    let a = &mut tj_w(w, f).anim;
    let on = a.rocket_control;
    step_list(&mut a.arms, 0, n, on, DELTA_SECONDS * dilation);
}

/// `armsLeftWeight`: ControlL's (child 0's) weight in the list as last updated.
pub fn arms_left_weight(w: &World, f: FighterId) -> Real {
    tj(w, f).anim.arms.w.first().copied().unwrap_or(0.0)
}

/// The list's two slot tracks of a montage: the slot's first track with segments and not additive, none for a slot
/// without one.
fn arm_tracks(m: &Montage) -> [Option<&Slot>; 2] {
    let of = |n: &str| m.slots.iter().find(|x| x.slot == n && !x.segments.is_empty() && x.additive != Some(true));
    [of(ARMS_SLOTS[0]), of(ARMS_SLOTS[1])]
}

fn plays_clip(m: &Montage, anim: &str) -> bool {
    m.slots.iter().any(|x| x.segments.iter().any(|g| g.anim == anim))
}

/// The skeleton's clip of `name`; a clip missing from the skel block warns and gives none.
fn clip_of<'a>(w: &World, sk: &'a Skel, m: &Montage, name: &str) -> Option<&'a SkelClip> {
    let c = sk.clip_index.get(name).map(|&i| &sk.clips[i]);
    if c.is_none() {
        warn_text(
            w,
            &format!(
                "Tom and Jerry arms: clip not in the skel block: {name} (the box index's skel nav): the arm shapes of {} keep the aim tree",
                m.name
            ),
        );
    }
    c
}

/// The inputs of one arm pose: the world, the fighter, the shown instance and its data, the skeleton.
struct Ctx<'a> {
    w: &'a World,
    f: FighterId,
    uid: u32,
    m: &'a Montage,
    sk: &'a Skel,
}

impl Ctx<'_> {
    /// A slot's pose at the montage's position into `out`: its track's clip, or the aim tree `p` (no track). False:
    /// the clip is not in the skel block.
    fn slot_pose(&self, slot: Option<&Slot>, p: &SkelPose, out: &mut SkelPose) -> bool {
        let st = state_ref(self.w, self.f, self.uid);
        let seg = slot.and_then(|s| st.and_then(|st| st.seg_at(&s.segments).map(|i| (st, &s.segments[i]))));
        let Some((st, seg)) = seg else {
            out.copy(p);
            return true;
        };
        let Some(c) = clip_of(self.w, self.sk, self.m, &seg.anim) else { return false };
        sample_clip(self.sk, c, st.seg_time(seg), out);
        true
    }
}

/// The slot's track is the aim tree's own clip at the aim tree's own time (or there is no track).
fn same_as_base(w: &World, f: FighterId, uid: u32, slot: Option<&Slot>) -> bool {
    let Some(st) = state_ref(w, f, uid) else { return true };
    let seg: Option<&Segment> = slot.and_then(|s| st.seg_at(&s.segments).map(|i| &s.segments[i]));
    let fa = &w.fighter(f).anim;
    seg.is_none_or(|seg| w.names.str(fa.anim) == seg.anim && st.seg_time(seg) == f64::from(fa.anim_time))
}

/// `armShapes` (CharHooks.pose): `s` holds the fighter's anim's box tracks at its anim time. The layer over the shapes'
/// pose; false (`s` as it is) unless the shown montage has a ControlL or ControlR track. Called by pawn.rs's hooks.
pub fn arm_shapes(w: &World, f: FighterId, s: &mut [f32]) -> bool {
    let Some(uid) = shown_uid(w, f) else { return false };
    let Some(m) = instance_data(&w.data, w, f, uid) else { return false };
    let [tl, tr] = arm_tracks(m);
    if tl.is_none() && tr.is_none() {
        return false;
    }
    // the slot walk has not yet put this montage's pose (the tick it started): the box path keeps the clip it has
    if !plays_clip(m, w.names.str(w.fighter(f).anim.anim)) {
        return false;
    }
    // the layer is the identity when the aim tree is one clip and every weighted slot of the list plays it at the same
    // time (the montage's Default section: Tom_and_Jerry_Rocket_Ground in all five slots): s is that pose already
    let wl = arms_left_weight(w, f);
    let mix = &tj(w, f).pawn.slot_mix.slot_mix;
    if mix.is_empty() && (wl <= ZERO_W || same_as_base(w, f, uid, tl)) && (wl >= FULL_W || same_as_base(w, f, uid, tr))
    {
        return false;
    }
    let Some(sk) = w.fixed.skels.get(&w.fighter(f).ch) else {
        warn_text(
            w,
            "Tom and Jerry arms: the box index has no skel block (the skeleton of the boxes bind): the arm shapes keep the aim tree",
        );
        return false;
    };
    let Some(mask) = sk.masks.get(MASK) else {
        warn_text(
            w,
            &format!(
                "Tom and Jerry arms: the box index has no skel block with the mask {MASK}: the arm shapes keep the aim tree"
            ),
        );
        return false;
    };
    let cx = Ctx { w, f, uid, m, sk };
    let mut wk = take_skel_work(w, sk.n);
    let ok = {
        let SkelWork { sc, p: [p, sp, l, r, o], .. } = &mut wk;
        compose(&cx, mix, wl, [tl, tr], mask, [p, sp, l, r, o]) && {
            write_shapes(sk, sc, p, s);
            true
        }
    };
    put_skel_work(w, wk);
    ok
}

/// The arms' pose into `p`: the aim tree, the list's two slots mixed by child 0's weight, replaced onto the mask.
fn compose(
    cx: &Ctx,
    mix: &[SlotPose],
    wl: Real,
    [tl, tr]: [Option<&Slot>; 2],
    mask: &[f32],
    [p, sp, l, r, o]: [&mut SkelPose; 5],
) -> bool {
    let (w, sk) = (cx.w, cx.sk);
    let fa = &w.fighter(cx.f).anim;
    // the aim tree as the slot walk mixed it
    let Some(base) = clip_of(w, sk, cx.m, w.names.str(fa.anim)) else { return false };
    sample_clip(sk, base, fa.anim_time, p);
    for x in mix {
        let Some(c) = clip_of(w, sk, cx.m, w.names.str(x.anim)) else { return false };
        sample_clip(sk, c, x.time, sp);
        weight_pose(p, 1.0 - x.w, o);
        add_weighted_pose(sp, x.w, o);
        normalize_rotations(o);
        p.copy(o);
    }
    // the list's two slots mixed by child 0's weight
    if wl >= FULL_W {
        if !cx.slot_pose(tl, p, l) {
            return false;
        }
    } else if wl <= ZERO_W {
        if !cx.slot_pose(tr, p, l) {
            return false;
        }
    } else {
        if !cx.slot_pose(tl, p, l) || !cx.slot_pose(tr, p, r) {
            return false;
        }
        weight_pose(r, 1.0 - wl, o);
        add_weighted_pose(l, wl, o);
        normalize_rotations(o);
        l.copy(o);
    }
    replace(p, l, mask);
    true
}
