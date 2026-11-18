//! Steven_AnimBP's glide under the shapes: the SlowFallState machine and the
//! glide BlendListByBool, its animUpdate, pose and slotSource hooks. His hit boxes are bone shapes read from the sim
//! pose, so the pose under them follows the list as `chars/batman/glide.rs` does. The machine's restart after a
//! missed update (bReinitializeOnBecomingRelevant, bSkipFirstUpdateTransition) is this file's.
//!
//! The graph under the shapes, input first (Steven_AnimBP; FighterData.slotHolds.graph): Slot DefaultSlot
//! (bAlwaysUpdateSourcePose true) -> Slot JumpSlot (false) -> BlendListByBool (bActiveValue a property-access copy of
//! Gliding; BlendTime [0.25, 0.1], linear, bResetChildOnActivation false): pose 0 the machine SlowFallState, pose 1
//! the linked layer (the shared graph: the sim's clip and time). Above the slots only cosmetic nodes (HeadScale
//! ModifyBones, SpringBone, Trail, the mirror cache: the view's), so the shapes follow the list's pose.
//!   SlowFallState (MaxTransitionsPerFrame 3, bSkipFirstUpdateTransition, bReinitializeOnBecomingRelevant): Into, the
//!   SequencePlayer of Steven_Nav_SlowFall_Enter (0.3333 s, not looping, rate 1) -> Loop, the SequencePlayer of
//!   Steven_Nav_SlowFall (1.6667 s, looping, rate 1) when GetRelevantAnimTimeRemaining(Into) == 0.0 exactly
//!   (ubergraph @1960-2032; crossfade 0 s, so no blend of the two). Neither clip has notifies.
//! The list's update is Bugs's FAlphaBlend rule: on a change of child the weights go linearly from where they are over
//! BlendTime[new] x |1 - weight[new]|, the first step in the update of the change; the instance's first update snaps.
//! A child at or under 0.00001 is not updated. Unlike Bugs's list the bool has no vertical-speed term. The list sits
//! under JumpSlot, which does not update its source under a JumpSlot montage at full weight (`layer_frozen`): then
//! neither the list nor the machine is updated (Batman's rule).
//! The restart: a machine whose update counter missed a frame (the list's child 0 at weight 0, or the JumpSlot hold)
//! starts at Into again at time 0 (Initialize_AnyThread -> SetState(entry)).
//! Gliding: the AnimBP's variable, written only by BPI_SetIsGliding @3554 (the pawn's SetGlideValueMulticast @3450):
//! `StevenPawnState::anim_gliding` (`gliding` is the pawn's own isGliding, which BPI_Cleanup @8654 clears alone).
//! Deviation (as Batman's glide_under): the two children are mixed shape by shape (positions, box axes and capsule
//! ends lerped), not bone by bone.

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated comparisons, kept for their NaN result")]

use super::state::{steven, steven_w};
use crate::Real;
use crate::combat::boxes::sample_of;
use crate::consts::DT;
use crate::fighter::FighterId;
use crate::fighter::anim::box_pose::turn_boxes;
use crate::fighter::anim::update::shown_uid;
use crate::fighter::anim::{
    AnimKey, hit_pitch, layer_frozen, layer_pitch, loco_clip, put_pose, slot_source_share, take_pose,
};
use crate::fighter::common::{char_data, warn_parts};
use crate::math::ue::advance_time;
use crate::math::{max, min};
use crate::registry::CharHooks;
use crate::world::World;

/// Into's player.
const ENTER: &str = "Steven_Nav_SlowFall_Enter";
/// Loop's player.
const LOOP: &str = "Steven_Nav_SlowFall";
/// The list's BlendTime per child.
const BLEND_TIME: [Real; 2] = [0.25, 0.1];
/// ZERO_ANIMWEIGHT_THRESH.
const ZERO_W: Real = 0.00001;
/// The list's share of the finished pose under the two Slot nodes (`slot_source_share`, unverified there).
const STAGES: [&[&str]; 2] = [&["DefaultSlot"], &["JumpSlot"]];

/// A Float32Array store.
#[allow(clippy::cast_possible_truncation, reason = "a Float32Array store rounds to float32")]
fn f32_of(x: Real) -> f32 {
    x
}

/// The hooks of this file in Steven's CharHooks (`glideHooks`).
pub fn hooks(h: &mut CharHooks) {
    h.anim_update = Some(glide_update);
    h.pose = Some(glide_shapes);
    h.slot_source = Some(glide_under);
}

/// SlowFallState's update by dt (the list's child 0 is relevant): the restart after a missed update, then the
/// transition (on the player's time before this update's advance; none can pass on the first update, Into at 0), then
/// the state's player.
fn machine_update(w: &mut World, f: FighterId, dt: Real) {
    let ch = w.fighter(f).ch;
    let anims = &char_data(w, ch).boxes.anims;
    let (Some(enter), Some(lp)) = (anims.get(ENTER), anims.get(LOOP)) else {
        warn_parts(w, &["steven glide: the box index has no ", ENTER, " / ", LOOP, ": SlowFallState holds"]);
        return;
    };
    let (enter_len, enter_rate, loop_len, loop_rate) =
        (crate::math::narrow(enter.length), enter.rate_scale, crate::math::narrow(lp.length), lp.rate_scale);
    let s = &mut steven_w(w, f).glide;
    if s.sm_first {
        s.sm = 0;
        s.sm_time = 0.0;
    } else if s.sm == 0 && (enter_len - s.sm_time) == 0.0 {
        s.sm = 1;
        s.sm_time = 0.0;
    }
    s.sm_first = false;
    if s.sm == 0 {
        s.sm_time = min(s.sm_time + (dt * crate::math::narrow(enter_rate)), enter_len);
    } else {
        // the looping player's FAnimationRuntime::AdvanceTime (FMath::Fmod wrap)
        advance_time(true, dt * crate::math::narrow(loop_rate), &mut s.sm_time, loop_len);
    }
}

/// The mesh tick's update after the montages (`CharHooks.anim_update`, not `pre`): the list, then the machine.
fn glide_update(w: &mut World, f: FighterId, dilation: Real, pre: bool) {
    if pre {
        return;
    }
    if layer_frozen(w, f) {
        // no update reached the machine: its next one is a first update
        steven_w(w, f).glide.sm_first = true;
        return;
    }
    let dt = DT * dilation;
    let st = steven_w(w, f);
    let child: i32 = if st.pawn.anim_gliding { 0 } else { 1 };
    let s = &mut st.glide;
    if child != s.last {
        let wc = if child == 0 { s.w } else { 1.0 - s.w };
        s.from_w = s.w;
        s.to_w = if child == 0 { 1.0 } else { 0.0 };
        s.blend = if s.last < 0 {
            0.0
        } else {
            BLEND_TIME[usize::try_from(child).expect("a child index")] * min((1.0 - wc).abs(), 1.0)
        };
        s.elapsed = 0.0;
        s.last = child;
    }
    s.elapsed += dt;
    let a = if s.blend > 0.0 { min(max(s.elapsed / s.blend, 0.0), 1.0) } else { 1.0 };
    s.w = s.from_w + (s.to_w - s.from_w) * a;
    if s.w > ZERO_W {
        machine_update(w, f, dt);
    } else {
        s.sm_first = true;
    }
}

/// `s` (child 1's shapes) mixed toward the machine state's clip by `wt`. False: the clip has no box track (a warning).
fn mix_glide(w: &World, f: FighterId, s: &mut [f32], wt: Real) -> bool {
    let g = &steven(w, f).glide;
    let clip = if g.sm == 0 { ENTER } else { LOOP };
    let mut buf = take_pose(w);
    buf.resize(s.len(), 0.0);
    let ok = sample_of(w, w.fighter(f).ch, clip, g.sm_time, &mut buf);
    if !ok {
        warn_parts(w, &["steven glide: ", clip, " has no box track: the shapes keep the layer's"]);
    } else if wt >= 1.0 {
        s.copy_from_slice(&buf);
    } else {
        for (v, &b) in s.iter_mut().zip(&buf) {
            let sv = crate::real(*v);
            *v = f32_of(sv + (crate::real(b) - sv) * wt);
        }
    }
    put_pose(w, buf);
    ok
}

/// `CharHooks.pose`: no montage shown and the list's child 0 has a weight. `s` holds the layer's clip (not yet at the
/// actor's scale); child 1 is that clip with the layer's own pitch of fml_un_COM (`hit_pitch`), child 0 the machine's
/// state's sequence.
fn glide_shapes(w: &World, f: FighterId, s: &mut [f32]) -> bool {
    // A montage's pose: the shown one, or the one this tick's pose update took (anim key Montage) that a stop has
    // taken off since (Batman's glide_shapes). The slots hide the list.
    if shown_uid(w, f).is_some() || matches!(w.fighter(f).anim.anim_key, AnimKey::Montage(_)) {
        return false;
    }
    let wt = steven(w, f).glide.w;
    if !(wt > ZERO_W) {
        return false;
    }
    if let Some(p) = hit_pitch(w, f)
        && p != 0.0
    {
        turn_boxes(w, f, s, p);
    }
    mix_glide(w, f, s, wt);
    true
}

/// `CharHooks.slot_source`: the Slot nodes' source under a shown montage below full weight (its blend-in or
/// blend-out) while child 0 has a weight: the list's pose (the state machines' clip under the montage, `loco_clip`,
/// with the layer's own pitch `layer_pitch`, mixed toward the machine's clip by w) mixed into `s` (the montage's
/// shapes at the actor's scale) by the source's share.
fn glide_under(w: &World, f: FighterId, s: &mut [f32]) {
    let wt = steven(w, f).glide.w;
    if !(wt > ZERO_W) {
        return;
    }
    let u = slot_source_share(w, f, &STAGES);
    if !(u > ZERO_W) {
        return;
    }
    let mut src = take_pose(w);
    src.resize(s.len(), 0.0);
    let ok = glide_source(w, f, &mut src, wt);
    if ok {
        let size = w.fighter(f).body.size_scale;
        if size != 1.0 {
            w.fixed.samplers[&w.fighter(f).ch].scale(&mut src, size);
        }
        for (v, &b) in s.iter_mut().zip(&src) {
            let sv = crate::real(*v);
            *v = f32_of(sv + (crate::real(b) - sv) * u);
        }
    }
    put_pose(w, src);
}

/// `glideUnder`'s source pose in `src` (not at the actor's scale); false when a clip has no box track.
fn glide_source(w: &World, f: FighterId, src: &mut [f32], wt: Real) -> bool {
    if wt < 1.0 {
        let (anim, time) = loco_clip(w, f);
        if !sample_of(w, w.fighter(f).ch, w.names.str(anim), time, src) {
            warn_parts(
                w,
                &["steven glide: ", w.names.str(anim), " has no box track: the montage's shapes are taken alone"],
            );
            return false;
        }
        if let Some(p) = layer_pitch(w, f)
            && p != 0.0
        {
            turn_boxes(w, f, src, p);
        }
    }
    mix_glide(w, f, src, wt)
}
