//! BugsBunny_AnimBP's glide list under his shapes: the pose and slotSource
//! hooks (`glideShapes`, `glideUnder`). The graph, input first: the BlendListByBool (child 0 the SequencePlayer of
//! BugsBunny_Nav_PropellerEars, child 1 the linked layer BaseMovementMachine), then Slot DefaultSlot
//! (bAlwaysUpdateSourcePose true), then Slot Override, then the physics chain (tails, carrot, hair, belt: no bone a
//! hurt shape rides) and the mirror. His hurt capsules ride bones (the shared BaseFighter rule) and Pawn_BugsBunny swaps
//! no HurtBox for the glide, so the shapes follow the list's pose: PropellerEars's box tracks at glideW, the layer's at
//! 1 - glideW, under the slots. The list's state is the sim's (glide.rs), updated in the mesh tick's anim update after
//! the montages.
// Deviation (as Batman's glide_under and the aim slots' mix of the box sampler): the two children are mixed shape by
// shape (positions, box axes and capsule ends lerped), not bone by bone.

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated comparisons, kept for their NaN result")]

use super::state::bugs;
use crate::Real;
use crate::combat::boxes::sample_of;
use crate::fighter::FighterId;
use crate::fighter::anim::box_pose::turn_boxes;
use crate::fighter::anim::montages::instance_data;
use crate::fighter::anim::update::{inst_pose, shown_uid};
use crate::fighter::anim::{
    AnimKey, fade_weight, hit_pitch, layer_pitch, loco_clip, put_pose, slot_source_share, take_pose,
};
use crate::fighter::common::warn_parts;
use crate::registry::CharHooks;
use crate::world::World;

/// The glide list's child 0: the SequencePlayer of BugsBunny_Nav_PropellerEars.
pub const GLIDE_SEQ: &str = "BugsBunny_Nav_PropellerEars";
/// ZERO_ANIMWEIGHT_THRESH.
const ZERO_W: Real = 0.00001;
/// The list's share of the finished pose under the two Slot nodes (`slot_source_share`): Slot DefaultSlot and Slot
/// Override.
const STAGES: [&[&str]; 2] = [&["DefaultSlot"], &["Override"]];

/// A Float32Array store.
#[allow(clippy::cast_possible_truncation, reason = "a Float32Array store rounds to float32")]
fn f32_of(x: Real) -> f32 {
    x
}

/// The hooks of this file in Bugs Bunny's CharHooks (`pose`, `slot_source`).
pub fn hooks(h: &mut CharHooks) {
    h.pose = Some(glide_shapes);
    h.slot_source = Some(glide_under);
}

/// `s` (the layer's shapes, turned as the layer turns them) mixed toward PropellerEars's at glideTime by `wt`. False:
/// the clip has no box track (a warning; `s` as it is).
fn mix_glide(w: &World, f: FighterId, s: &mut [f32], wt: Real) -> bool {
    let mut buf = take_pose(w);
    buf.resize(s.len(), 0.0);
    let ok = sample_of(w, w.fighter(f).ch, GLIDE_SEQ, bugs(w, f).glide_time, &mut buf);
    if !ok {
        warn_parts(w, &["bugs glide: ", GLIDE_SEQ, " has no box track: the shapes keep the layer's"]);
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
/// actor's scale); child 1 is that clip with the layer's own pitch of fml_un_COM (`hit_pitch`: the Fall / Jump states'
/// FallLean, the fly branch's), child 0 the sequence alone (no ModifyBone in this AnimBP). Bugs enters no slotMix, so
/// the turn is all the box sampler would have done after a false. A stopped montage still blending out (the fading
/// list: the wall jump's StopAnimMontage @2968 ahead of a glide) lies over the list's shapes by its share of the Slot
/// nodes (`fading_over`), as over Batman's glide.
fn glide_shapes(w: &World, f: FighterId, s: &mut [f32]) -> bool {
    // A montage's pose: the shown one, or the one this tick's pose update took (anim key Montage) and a stop has taken
    // off since. The slots hide the list.
    if shown_uid(w, f).is_some() || matches!(w.fighter(f).anim.anim_key, AnimKey::Montage(_)) {
        return false;
    }
    let wt = bugs(w, f).glide_w;
    if !(wt > ZERO_W) {
        return false;
    }
    if let Some(p) = hit_pitch(w, f)
        && p != 0.0
    {
        turn_boxes(w, f, s, p);
    }
    mix_glide(w, f, s, wt);
    let share = slot_source_share(w, f, &STAGES);
    if share < 1.0 - ZERO_W {
        fading_over(w, f, s, 1.0 - share);
    }
    true
}

/// `hasFull`: the montage has a full-pose track with a segment.
fn has_full(m: &mvs_data::fighter::Montage) -> bool {
    m.slots.iter().any(|t| !t.segments.is_empty() && t.additive != Some(true))
}

/// `fadingOver` (Batman's `chars/batman/glide.rs`): the stopped instances' shapes over s (not yet at the actor's
/// scale) by u: each instance's track as the pose update shows a montage at the position it holds, the instances
/// weighted by their blend weights.
/// Deviation: the position is held from the stop (the build's interrupted instance goes on advancing), and the shapes
/// are mixed shape by shape.
fn fading_over(w: &World, f: FighterId, s: &mut [f32], u: Real) {
    let (mut acc, mut one) = (take_pose(w), take_pose(w));
    acc.resize(s.len(), 0.0);
    one.resize(s.len(), 0.0);
    let ch = w.fighter(f).ch;
    let mut sum = 0.0;
    for fd in &w.fighter(f).montages.fading {
        let wt = fade_weight(fd);
        let uid = fd.m.montage.uid;
        if !(wt > ZERO_W) || !instance_data(&w.data, w, f, uid).is_some_and(has_full) {
            continue;
        }
        let Some((anim, time)) = inst_pose(&w.data, w, f, uid, None) else { continue };
        let out = if sum > 0.0 { &mut one } else { &mut acc };
        if !sample_of(w, ch, w.names.str(anim), time, out) {
            warn_parts(
                w,
                &["bugs glide: ", w.names.str(anim), " has no box track: left out of the shapes over the glide"],
            );
            continue;
        }
        sum += wt;
        if sum > wt {
            for (a, &o) in acc.iter_mut().zip(&one) {
                let av = crate::real(*a);
                *a = f32_of(av + (crate::real(o) - av) * (wt / sum));
            }
        }
    }
    if sum > 0.0 {
        for (v, &a) in s.iter_mut().zip(&acc) {
            let sv = crate::real(*v);
            *v = f32_of(sv + (crate::real(a) - sv) * u);
        }
    }
    put_pose(w, acc);
    put_pose(w, one);
}

/// `CharHooks.slot_source`: the Slot nodes' source under a shown montage below full weight (its blend-in or
/// blend-out) while child 0 has a weight. `s` holds the montage's shapes at the actor's scale; the source is the list's
/// pose (the state machines' clip under the montage, `loco_clip`, with the layer's own pitch `layer_pitch`, mixed
/// toward PropellerEars's by glideW), mixed into `s` by the source's share. With child 0 at 0 the port keeps the
/// montage's shapes whole.
fn glide_under(w: &World, f: FighterId, s: &mut [f32]) {
    let wt = bugs(w, f).glide_w;
    if !(wt > ZERO_W) {
        return;
    }
    let u = slot_source_share(w, f, &STAGES);
    if !(u > ZERO_W) {
        return;
    }
    let mut src = take_pose(w);
    src.resize(s.len(), 0.0);
    if glide_source(w, f, &mut src, wt) {
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

/// The source pose in `src` (not at the actor's scale); false when a clip has no box track.
fn glide_source(w: &World, f: FighterId, src: &mut [f32], wt: Real) -> bool {
    if wt < 1.0 {
        let (anim, time) = loco_clip(w, f);
        if !sample_of(w, w.fighter(f).ch, w.names.str(anim), time, src) {
            warn_parts(
                w,
                &["bugs glide: ", w.names.str(anim), " has no box track: the montage's shapes are taken alone"],
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
