//! Batman: Batman_AnimBP's glide stack in the sim, the pose his bone hit and hurt shapes ride while he glides with no
//! montage over the source (Batman_AnimBP's graph and the node values of its CDO). The one
//! port of the stack's state (the weights, the machine, the players, SpringBone_4 / _3). Here the pose under the
//! shapes, on the skeleton of the box index (the shapes' bones with their parents, `mvs_anim::skel_pose`):
//!   BlendListByBool (IsGliding; BlendTime [0, 0.1], linear): child 0, the glide stack: ApplyAdditive(Base =
//!     ApplyAdditive_1(Base = BlendSpacePlayer_1 Batman_Nav_GlideLoop_BlendSpace (full poses, X <- SpringJoint1_offsetY),
//!     Additive = the machine GlideAdditive: GlideEnter (Batman_Nav_GlideEnterAdd) -> GlideLoop (Batman_Nav_
//!     GlideLoopAdd)), Additive = BlendSpacePlayer Batman_Nav_GlideAdd_BlendSpace (additive, X <- SpringJoint2_offsetY));
//!     child 1: the linked layer (the fighter's clip with the Fall blend space's mix, `BatmanState.slot_mix`, and the
//!     layer's own pitch of fml_un_COM, the hit pitch).
//!   -> ModifyBone_4 (Root, + DepthVector in world space, alpha IsGlidingAlpha): the actor's depth +210 at both
//!     facings (DepthVector (FacingLeft ? -210 : 210, 0, 0) under the actor's yaw), so every shape on a bone leaves
//!     the play plane by 2.1 m while he glides; HurtBoxGlide (on the capsule, no bone) stays.
//!   -> SpringBone_4 (SpringJoint1) and _3 (SpringJoint2), whose places the next update reads back as the two blend
//!     space inputs: (Root socket Y - SpringJointN socket Y) x (FacingLeft ? 1 : -1) on the pose evaluated last.
//!   Then the foot marker springs and the two LegIKs (alpha IsGlidingAlpha), the aim slots and the Slot nodes.
//! The shapes are posed from this while the glide child or ModifyBone_4 has a weight (else the box tracks as before):
//! whole with no montage shown (CharHooks.pose, the stopped instances still blending out mixed over it), mixed under a
//! shown montage by the source's share of the Slot nodes (CharHooks.slot_source, `glide_under`; the source's share:
//! GetSlotWeight over every instance with a track in the slot, the blending-out ones too; at full weight the
//! montage's shapes alone).
//!
//! The springs. Both joints hang on Root and sit on it in every frame of every clip of his (the box tracks:
//! SpringJoint1Socket and SpringJoint2Socket = RootSocket, 0 cm), so a joint's target is the root's world place:
//! UE Y = -(mesh world x), Z the mesh's height with the clip's root track, X (depth) ModifyBone_4's shift. Each is
//! FAnimNode_SpringBone (float32 steps, the fixed 1/120 s substeps, the reset and speed thresholds, the displacement
//! limit), on translation Y alone (bTranslateY; no rotate bit), with no owner velocity. The update adds the mesh
//! tick's dilated DeltaTime while the source updates; the evaluation runs while the source is evaluated (the Slot
//! node's source weight above 1e-5). The first evaluation of a new pawn takes the joint where its target is (the
//! exe's InitAnim tick of 0, RemainingTime 0). The joint's place in the finished pose is the spring's, mixed with the
//! montage's (at Root) by the Slot weight (the source's share: the montage's blend-in and the stopped instances'
//! blend-out count too, where the source is evaluated under them).
//!
//! Not sim state here: the skeleton of the index (`Fixed::skels`), its scratch poses (`SkelWork`) and the shape
//! buffers (the World's pose pool).
//! Deviation: the foot marker springs and the two LegIKs (IsGlidingAlpha) are not run on the shapes: the feet and
//! lower legs follow the glide stack's own pose, where the build pulls them toward the lagging markers.
//! Deviation: the shapes' rule of the skeleton poses (a frame lerp of the cooked keys, the additive deltas recovered
//! from the export, no float32 steps of the exe) and the glide clips as Step sequences hold here.
//! Deviation: the spring's vertical target takes the fighter clip's root track (the glide stack's own root is not
//! composed for it); it reaches only the reset test (an error above 256 cm).
//! Deviation: the joints' places read back at the update are the world positions of the last evaluation with the
//! facing of the update (the build reads the sockets' world Y of the finished pose: the same away from a turn).

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated comparisons, kept for their NaN result")]

use super::state::{batman, batman_w};
use crate::Real;
use crate::combat::boxes::sample_of;
use crate::consts::DT;
use crate::fighter::anim::montages::instance_data;
use crate::fighter::anim::root_motion::root_into;
use crate::fighter::anim::update::{inst_pose, shown_uid};
use crate::fighter::anim::{
    AnimKey, SkelWork, SlotPose, fade_weight, hit_pitch, layer_frozen, layer_pitch, loco_clip, put_pose, put_skel_work,
    slot_source_share, take_pose, take_skel_work,
};
use crate::fighter::common::{data_of, warn_parts};
use crate::fighter::{FighterId, mesh_wx, mesh_wy};
use crate::math::ue::{SMALL_NUMBER, fmod, rotator_quaternion, safe_normal};
use crate::math::{max, min};
use crate::names::Name;
use crate::registry::CharHooks;
use crate::world::World;
use mvs_anim::skel_pose::{
    Skel, SkelBlendSpace, SkelClip, SkelPose, SkelScratch, accumulate, add_weighted_pose, blend_rows,
    normalize_rotations, sample_clip, turn_bone, weight_pose, write_shapes,
};

const ENTER: &str = "Batman_Nav_GlideEnterAdd";
const LOOP: &str = "Batman_Nav_GlideLoopAdd";
/// BlendListByBool BlendTime[1] (child 1 activated); BlendTime[0] (the glide) is 0.
const GLIDE_OUT: Real = 0.1;
/// OnFacingChanged's DepthVector, along the actor's +Y (the box tracks' depth) at both facings.
const DEPTH: Real = 210.0;
/// ZERO_ANIMWEIGHT_THRESH.
const MIN_W: Real = 1e-5;
/// The blend space players' input filter.
const MIN_DT: Real = 1e-4;

/// One FAnimNode_SpringBone's constants: Stiffness, Damping, ErrorResetThresh, MaxDisplacement.
struct Spring {
    k: Real,
    d: Real,
    th: Real,
    mx: Real,
}

/// SpringBone_4 (SpringJoint1) and SpringBone_3 (SpringJoint2), Batman_AnimBP's CDO (Default__Batman_AnimBP_C:
/// AnimGraphNode_SpringBone_4 / _3; bTranslateY alone, bLimitDisplacement, alpha 1).
const SPRINGS: [Spring; 2] =
    [Spring { k: 200.0, d: 15.0, th: 256.0, mx: 5000.0 }, Spring { k: 200.0, d: 5.0, th: 256.0, mx: 500_000.0 }];

/// FixedTimeStep = the world's dilation (1) x 1/120 (float32).
fn fixed_step() -> Real {
    1.0 * 0.008_333_334
}

/// A Float32Array store.
#[allow(clippy::cast_possible_truncation, reason = "a Float32Array store rounds to float32")]
fn f32_of(x: Real) -> f32 {
    x
}

/// The hooks of this file in Batman's CharHooks.
pub fn hooks(h: &mut CharHooks) {
    h.pose = Some(glide_shapes);
    h.slot_source = Some(glide_under);
}

/// The glide stack's parts of the box index's skeleton (`GlideData`).
struct Glide<'a> {
    sk: &'a Skel,
    loop_bs: &'a SkelBlendSpace,
    add: &'a SkelBlendSpace,
    enter: &'a SkelClip,
    loop_add: &'a SkelClip,
}

/// `glideData`: the skeleton's glide stack, `None` (with a warning) when the index or the data lacks a part.
fn glide_data<'a>(w: &'a World, f: FighterId) -> Option<Glide<'a>> {
    let g = glide_parts(w, f);
    if g.is_none() {
        warn_parts(
            w,
            &[
                "Batman glide: the box index has no glide stack (skel, blend spaces glideLoop / glideAdd, ",
                ENTER,
                ", ",
                LOOP,
                ") or the data no machine GlideAdditive (slotHolds.machines): the shapes keep the clip",
            ],
        );
    }
    g
}

fn glide_parts<'a>(w: &'a World, f: FighterId) -> Option<Glide<'a>> {
    let sk = &**w.fixed.skels.get(&w.fighter(f).ch)?;
    let bs = |n: &str| sk.blend_index.get(n).map(|&i| &sk.blend[i]);
    let clip = |n: &str| sk.clip_index.get(n).map(|&i| &sk.clips[i]);
    let (loop_bs, add, enter, loop_add) = (bs("glideLoop")?, bs("glideAdd")?, clip(ENTER)?, clip(LOOP)?);
    // the machine's flags (Batman_AnimBP's GlideAdditive: bReinitializeOnBecomingRelevant and
    // bSkipFirstUpdateTransition, entry GlideEnter; the data's check refuses any other values)
    let mach = data_of(w, f).fighter.slot_holds.machines.contains_key("GlideAdditive");
    let ok = loop_bs.ok
        && loop_bs.full
        && !add.full
        && add.clips.iter().all(|c| c.is_some_and(|c| sk.clips[c].additive))
        && enter.additive
        && loop_add.additive
        && mach;
    ok.then_some(Glide { sk, loop_bs, add, enter, loop_add })
}

/// The clip's RateScale.
fn rate(sk: &Skel, c: &SkelClip) -> Real {
    sk.index.anims.get(&c.name).map_or(1.0, |a| crate::math::narrow(a.rate_scale))
}

fn clamp01(v: Real) -> Real {
    min(max(v, 0.0), 1.0)
}

/// The clip of a row of the last select.
fn row_clip<'a>(sk: &'a Skel, sc: &SkelScratch, bs: &SkelBlendSpace, k: usize) -> &'a SkelClip {
    let i = usize::try_from(sc.row_sample[k]).expect("a row's sample");
    &sk.clips[bs.clips[i].expect("a blend space sample's clip")]
}

// ---- a BlendSpacePlayer (select, length, advance; every sample rate 1, a looping player of play rate 1 and start
// 0). Its normalized time is a float (FAnimNode_BlendSpacePlayer.InternalTimeAccumulator)

fn select(sc: &mut SkelScratch, bs: &SkelBlendSpace, x: Real) -> bool {
    blend_rows(sc, bs, x, 0.0)
}

fn advance(sk: &Skel, sc: &SkelScratch, bs: &SkelBlendSpace, t: Real, dt: Real) -> Real {
    let mut len = 0.0;
    for k in 0..sc.row_count {
        let c = row_clip(sk, sc, bs, k);
        let r = rate(sk, c);
        let l = crate::math::narrow(c.length / f64::from(if r != 0.0 { r.abs() } else { 1.0 }));
        len += l * clamp01(crate::real(sc.row_w[k]));
    }
    let mut cur = (t * len) + dt;
    if cur < 0.0 || cur > len {
        if len == 0.0 {
            cur = 0.0;
        } else {
            cur = fmod(cur, len); // AdvanceTime's loop: FMath::Fmod, then + End below 0
            if cur < 0.0 {
                cur += len;
            }
        }
    }
    if len != 0.0 { cur / len } else { 0.0 }
}

/// `rowsPose`: the rows of the last select at the player's time t into out: full poses blended (pose) or additive
/// deltas blended (a delta pose). s: scratch.
fn rows_pose(sk: &Skel, sc: &SkelScratch, bs: &SkelBlendSpace, t: Real, out: &mut SkelPose, s: &mut SkelPose) {
    let u = clamp01(t);
    for k in 0..sc.row_count {
        let c = row_clip(sk, sc, bs, k);
        sample_clip(sk, c, if rate(sk, c) < 0.0 { 1.0 - u } else { u } * crate::math::narrow(c.length), s);
        let wt = clamp01(crate::real(sc.row_w[k]));
        if k == 0 {
            weight_pose(s, wt, out);
        } else {
            add_weighted_pose(s, wt, out);
        }
    }
    normalize_rotations(out);
}

// ---- the springs

/// The target of both joints in world space (UE X depth, Y, Z).
fn target(w: &World, f: FighterId) -> [Real; 3] {
    let fi = w.fighter(f);
    // Unverified: a clip without a root track takes 0 (the build's value for such a clip is not read).
    let up = root_into(w, f, w.names.str(fi.anim.anim), fi.anim.anim_time).map_or(0.0, |r| r.1);
    let alpha = batman(w, f).glide_alpha;
    [
        (alpha * if fi.body.facing_dir > 0.0 { DEPTH } else { -DEPTH }),
        (-mesh_wx(w, f)),
        (mesh_wy(w, f) + up * fi.body.size_scale),
    ]
}

/// One joint's evaluation: `st` holds (B x, y, z, V x, y, z) of joint j at `st[j * 6]`.
fn spring_evaluate(st: &mut [Real; 12], j: usize, mut rem: Real, t: &[Real; 3]) {
    let Spring { k, d, th, mx } = SPRINGS[j];
    let (k, d, th, mx) = (k, d, th, mx);
    let o = j * 6;
    let dt = fixed_step();
    let (inv, th2, mx2) = ((1.0 / dt), (th * th), (mx * mx));
    let mut old = [0.0; 3];
    while dt < rem {
        // B += dt x OwnerVelocity (0)
        let (ex, ey, ez) = ((t[0] - st[o]), (t[1] - st[o + 1]), (t[2] - st[o + 2]));
        if th2 < (((ex * ex) + (ey * ey)) + (ez * ez)) {
            st[o..o + 3].copy_from_slice(t);
            st[o + 3..o + 6].fill(0.0);
        }
        for a in 0..3 {
            let acc = ((t[a] - st[o + a]) * k) - (st[o + 3 + a] * d);
            st[o + 3 + a] =
                if d <= inv { st[o + 3 + a] + (acc * dt) } else { st[o + 3 + a] + ((acc * dt) * (inv / d)) };
        }
        let (v0, v1, v2) = (st[o + 3], st[o + 4], st[o + 5]);
        let sp = ((((v0 * v0) + (v1 * v1)) + (v2 * v2)).sqrt()) * dt;
        if sp > th {
            let q = th / sp;
            st[o + 3] = v0 * q;
            st[o + 4] = v1 * q;
            st[o + 5] = v2 * q;
        }
        for a in 0..3 {
            old[a] = st[o + a];
            // translation on Y alone: X and Z take the target's
            st[o + a] = if a == 1 { st[o + a] + (st[o + 3 + a] * dt) } else { t[a] };
        }
        let (dx, dy, dz) = ((st[o] - t[0]), (st[o + 1] - t[1]), (st[o + 2] - t[2]));
        let d2 = ((dx * dx) + (dy * dy)) + (dz * dz);
        if d2 > mx2 {
            // FVector::GetSafeNormal (1e-8), the limit along it
            let n = safe_normal([dx, dy, dz], SMALL_NUMBER);
            for a in 0..3 {
                st[o + a] = t[a] + (n[a] * mx);
            }
        }
        for a in 0..3 {
            st[o + 3 + a] = (st[o + a] - old[a]) * inv;
        }
        rem -= dt;
    }
}

/// The glide stack's share of the finished pose under Batman_AnimBP's Slot nodes (all of them sit above it: the aim
/// slots' TwoWayBlends, DefaultSlot, OverrideSlot, Batarang): `slot_source_share` over these stages.
/// Deviation: an aim slot set (AimUp / AimCenter / AimDown under two TwoWayBlends, AimBack / AimForward under one) is
/// one stage, an instance with a track in any of its slots counted there (no montage of his has a part of a set).
const STAGES: &[&[&str]] = &[
    &["AimUp", "AimCenter", "AimDown"],
    &["AimBack", "AimForward"],
    &["DefaultSlot"],
    &["OverrideSlot"],
    &["Batarang"],
];

fn source_share(w: &World, f: FighterId) -> Real {
    slot_source_share(w, f, STAGES)
}

/// `glideUpdate`: the mesh tick's update after the montages (CharHooks.anim_update, not `pre`):
/// BlueprintUpdateAnimation's glide reads (IsGlidingAlpha, the two joints' offsets from the pose evaluated last), the
/// graph's update under the Slot nodes (frozen while a shown montage at full weight holds the source), then this
/// tick's evaluation of the two springs while the source is evaluated.
pub fn glide_update(w: &mut World, f: FighterId, dilation: Real) {
    let Some(g) = glide_data(w, f) else { return };
    let (enter_len, loop_len) = (g.enter.length, g.loop_add.length);
    let dt = DT * dilation; // the mesh tick's DeltaTime (float); the weights and times below are float too
    let frozen = layer_frozen(w, f);
    let sign = if w.fighter(f).body.facing_dir < 0.0 { 1.0 } else { -1.0 }; // (FacingLeft ? 1 : -1)
    // the two players' times, advanced on the skeleton's rows (read through &World, written below)
    let s = batman(w, f);
    let (off1, off2) = ((s.glide_j1 * sign), (s.glide_j2 * sign));
    let gliding = s.anim_gliding;
    let mut glide_w = s.glide_w;
    let mut players = None;
    if !frozen {
        glide_w = if gliding { 1.0 } else { max(glide_w - (dt / GLIDE_OUT), 0.0) };
        if glide_w > MIN_W {
            // the two players (the input filter keeps the last input at a step of 1e-4 or less)
            let (lx, ax) = if dt > MIN_DT { (off1, off2) } else { (s.glide_loop_x, s.glide_add_x) };
            let mut k = take_skel_work(w, g.sk.n);
            let lt = if select(&mut k.sc, g.loop_bs, lx) {
                advance(g.sk, &k.sc, g.loop_bs, s.glide_loop_t, dt)
            } else {
                s.glide_loop_t
            };
            let at = if select(&mut k.sc, g.add, ax) {
                advance(g.sk, &k.sc, g.add, s.glide_add_t, dt)
            } else {
                s.glide_add_t
            };
            put_skel_work(w, k);
            players = Some((lx, lt, ax, at));
        }
    }
    let s = batman_w(w, f);
    // BlueprintUpdateAnimation
    s.glide_alpha = if s.anim_gliding { 1.0 } else { 0.0 };
    // The Slot nodes' hold: the shared rule of the base layer (layer_frozen: a shown montage at full weight with a
    // track in every slot of a slotHolds.stages set, here [Batarang] or [OverrideSlot]); the glide stack sits under
    // the same Slot nodes as the linked layer.
    if !frozen {
        // BlendListByBool
        let was = s.glide_w > MIN_W;
        s.glide_w = glide_w;
        if s.glide_w > MIN_W {
            // the child had no update while its weight was 0 (BlendListBase updates a child above 0.00001 alone)
            if !was {
                s.glide_sm_first = true;
            }
            // GlideAdditive: a machine whose update counter missed a frame restarts (bReinitializeOnBecomingRelevant:
            // Initialize_AnyThread -> SetState(GlideEnter), whose sequence player starts at 0), after the child's
            // weight was 0 or a hold of the Slot nodes (below); then the transition (GetRelevantAnimTimeRemaining
            // <= 0; not on the first update), then the state's player. The two blend space players are no
            // machine's: they go on from the time they had.
            if s.glide_sm_first {
                s.glide_sm = 0.0;
                s.glide_sm_time = 0.0;
            } else if s.glide_sm == 0.0 && enter_len - f64::from(s.glide_sm_time) <= 0.0 {
                s.glide_sm = 1.0;
                s.glide_sm_time = 0.0;
            }
            s.glide_sm_first = false;
            if s.glide_sm == 0.0 {
                s.glide_sm_time = min(s.glide_sm_time + dt, crate::math::narrow(enter_len));
            } else {
                s.glide_sm_time += dt;
                if s.glide_sm_time > crate::math::narrow(loop_len) {
                    s.glide_sm_time =
                        if loop_len > 0.0 { fmod(s.glide_sm_time, crate::math::narrow(loop_len)) } else { 0.0 };
                }
            }
            let (lx, lt, ax, at) = players.expect("the players of a weighted glide child");
            (s.glide_loop_x, s.glide_loop_t, s.glide_add_x, s.glide_add_t) = (lx, lt, ax, at);
        }
        // the springs' update (alpha 1)
        s.spring_rem += dt;
    } else {
        // no update reached the machine: its next one is a first update (the restart above)
        s.glide_sm_first = true;
    }
    // the evaluation: the source under the Slot node at a weight above 1e-5
    let src = source_share(w, f);
    if !(src > MIN_W) {
        // the finished pose is the montage's, whose joints sit on Root
        let s = batman_w(w, f);
        s.glide_j1 = 0.0;
        s.glide_j2 = 0.0;
        return;
    }
    let t = target(w, f);
    let s = batman_w(w, f);
    if !s.spring_init || s.spring_rem == 0.0 {
        s.spring_init = true;
        for j in 0..2 {
            s.springs[j * 6..j * 6 + 3].copy_from_slice(&t);
            s.springs[j * 6 + 3..j * 6 + 6].fill(0.0);
        }
    }
    let step = fixed_step();
    if step.abs() > 1e-4 {
        spring_evaluate(&mut s.springs, 0, s.spring_rem, &t);
        spring_evaluate(&mut s.springs, 1, s.spring_rem, &t);
        let mut rem = s.spring_rem;
        while step < rem {
            rem -= step;
        }
        s.spring_rem = rem;
    }
    // (Root Y - SpringJointN Y) of this evaluation's finished pose, the joints mixed with the montage's (on Root)
    s.glide_j1 = (t[1] - s.springs[1]) * src;
    s.glide_j2 = (t[1] - s.springs[7]) * src;
}

/// `glideShapes` (CharHooks.pose): the glide stack and ModifyBone_4 under the shapes, while no montage is shown and
/// either has a weight. The result's shapes on bones replace s's (`write_shapes`); false: the box tracks of the clip.
pub fn glide_shapes(w: &World, f: FighterId, s: &mut [f32]) -> bool {
    // A montage's pose: the shown one, or the one this tick's pose update took (anim key Montage) and a stop has
    // taken off since (a stop after the pose update: the fighter's anim keeps that montage's clip until the next
    // update, whose shapes every fighter keeps meanwhile).
    let fi = w.fighter(f);
    if shown_uid(w, f).is_some() || matches!(fi.anim.anim_key, AnimKey::Montage(_)) {
        return false;
    }
    let b = batman(w, f);
    if !(b.glide_w > MIN_W) && !(b.glide_alpha > MIN_W) {
        return false;
    }
    let Some(g) = glide_data(w, f) else { return false };
    let mut k = take_skel_work(w, g.sk.n);
    let posed =
        source_shapes(w, f, &g, &mut k, s, fi.anim.anim, fi.anim.anim_time, &b.slot_mix.slot_mix, hit_pitch(w, f));
    put_skel_work(w, k);
    if !posed {
        return false;
    }
    // a stopped instance still blending out (the fading list) lies over the source by its slot weight
    let share = source_share(w, f);
    if share < 1.0 - MIN_W {
        fading_over(w, f, s, 1.0 - share);
    }
    true
}

/// `hasFull`: the montage has a full-pose track with a segment.
fn has_full(m: &mvs_data::fighter::Montage) -> bool {
    m.slots.iter().any(|t| !t.segments.is_empty() && t.additive != Some(true))
}

/// `fadingOver`: the stopped instances' shapes over s (not yet at the actor's scale) by u: each instance's track as
/// the pose update shows a montage at the position it holds, the instances weighted by their blend weights.
/// Deviation: the position is held from the stop (the build's interrupted instance goes on advancing), its aim
/// slots' mix is not taken, and the shapes are mixed shape by shape.
fn fading_over(w: &World, f: FighterId, s: &mut [f32], u: Real) {
    let (mut acc, mut one) = (take_pose(w), take_pose(w));
    acc.resize(s.len(), 0.0);
    one.resize(s.len(), 0.0);
    let ch = w.fighter(f).ch;
    let mut sum = 0.0;
    for fd in &w.fighter(f).montages.fading {
        let wt = fade_weight(fd);
        let uid = fd.m.montage.uid;
        if !(wt > MIN_W) || !instance_data(&w.data, w, f, uid).is_some_and(has_full) {
            continue;
        }
        let Some((anim, time)) = inst_pose(&w.data, w, f, uid, None) else { continue };
        let out = if sum > 0.0 { &mut one } else { &mut acc };
        if !sample_of(w, ch, w.names.str(anim), time, out) {
            warn_parts(
                w,
                &["Batman glide: ", w.names.str(anim), " has no box track: left out of the shapes over the glide"],
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

/// `glideUnder` (CharHooks.slot_source): the Slot nodes' source under a shown montage below full weight (its
/// blend-in or blend-out) while the glide stack or ModifyBone_4 has a weight: the build's slots take the pose under
/// them, so the shapes are the montage's (s: at the actor's scale, its aim slots' mix in; it stands for every
/// instance, a stopped one still blending out among them) mixed with the glide stack's source by the source's share
/// (1 less every instance's weight in the slots, the fading ones' included). The source is the one `glide_shapes`
/// poses, with the state machines' clip under the montage (`loco_clip`) and the layer's own pitch.
/// Deviation: the two are mixed shape by shape (positions, box axes and capsule ends lerped, as the aim slots' mix
/// of the box sample), not bone by bone; child 1 under a montage is that clip without the Fall blend space's mix
/// (`fall_mix` composes it with no montage shown alone), read only in the 0.1 s glide out.
pub fn glide_under(w: &World, f: FighterId, s: &mut [f32]) {
    let b = batman(w, f);
    if !(b.glide_w > MIN_W) && !(b.glide_alpha > MIN_W) {
        return;
    }
    let u = source_share(w, f);
    if !(u > MIN_W) {
        return;
    }
    let Some(g) = glide_data(w, f) else { return };
    let (anim, time) = loco_clip(w, f);
    let mut buf = take_pose(w);
    buf.resize(s.len(), 0.0);
    if !sample_of(w, w.fighter(f).ch, w.names.str(anim), time, &mut buf) {
        warn_parts(
            w,
            &["Batman glide: ", w.names.str(anim), " has no box track: the montage's shapes are taken alone"],
        );
        put_pose(w, buf);
        return;
    }
    let mut k = take_skel_work(w, g.sk.n);
    let posed = source_shapes(w, f, &g, &mut k, &mut buf, anim, time, &[], layer_pitch(w, f));
    put_skel_work(w, k);
    if posed {
        let size = w.fighter(f).body.size_scale;
        if size != 1.0 {
            w.fixed.samplers[&w.fighter(f).ch].scale(&mut buf, size);
        }
        for (v, &b) in s.iter_mut().zip(&buf) {
            let sv = crate::real(*v);
            *v = f32_of(sv + (crate::real(b) - sv) * u);
        }
    }
    put_pose(w, buf);
}

/// `sourceShapes`: the glide stack's source pose on s (the clip's box tracks, not yet at the actor's scale): child 1
/// is `anim` at `time` with the slot mix `mix` and the pitch of fml_un_COM.
#[allow(clippy::too_many_arguments, reason = "the function's parameters, with the skeleton and its scratch")]
fn source_shapes(
    w: &World,
    f: FighterId,
    g: &Glide<'_>,
    k: &mut SkelWork,
    s: &mut [f32],
    anim: Name,
    time: Real,
    mix: &[SlotPose],
    pitch: Option<Real>,
) -> bool {
    let b = batman(w, f);
    let (gw, ga) = (b.glide_w, b.glide_alpha);
    let sk = g.sk;
    let SkelWork { sc, p, .. } = k;
    let [l, gp, a, sp, m] = p;
    let clip = |n: &str| sk.clip_index.get(n).map(|&i| &sk.clips[i]);
    // which pose is the result: 0 child 1 (L), 1 child 0 (G), 2 their blend (S)
    let mut out = 0;
    if gw < 1.0 - MIN_W {
        // child 1: the linked layer (the clip, the Fall blend space's mix in order, the layer's pitch of fml_un_COM)
        let Some(base) = clip(w.names.str(anim)).filter(|c| !c.additive) else {
            warn_parts(
                w,
                &[
                    "Batman glide: ",
                    w.names.str(anim),
                    " is no full-pose clip of the box index's skel (batman clips): the shapes keep the clip",
                ],
            );
            return false;
        };
        sample_clip(sk, base, time, l);
        for mp in mix {
            if !(mp.w > 0.0) {
                continue;
            }
            let Some(c) = clip(w.names.str(mp.anim)).filter(|c| !c.additive) else {
                warn_parts(
                    w,
                    &[
                        "Batman glide: the slot mix's ",
                        w.names.str(mp.anim),
                        " is no full-pose clip of the box index's skel (batman clips): left out of the shapes",
                    ],
                );
                continue;
            };
            sample_clip(sk, c, mp.time, m);
            weight_pose(l, 1.0 - mp.w, sp);
            add_weighted_pose(m, mp.w, sp);
            normalize_rotations(sp);
            l.copy(sp);
        }
        if let Some(pitch) = pitch
            && sk.sk.com >= 0
        {
            // MakeRotator(0, Pitch, 0) as FRotator::Quaternion: (0, -sin, 0, cos) of the half angle; UE's Y is the
            // generator space's -Z (the depth axis)
            let q = rotator_quaternion([pitch, 0.0, 0.0]);
            turn_bone(sk, l, sk.sk.com, 0.0, 0.0, -q[1], q[3], false);
        }
    }
    if gw > MIN_W {
        // child 0: GlideLoop's rows, the machine's additive, GlideAdd's rows' delta
        select(sc, g.loop_bs, b.glide_loop_x);
        rows_pose(sk, sc, g.loop_bs, b.glide_loop_t, gp, sp);
        let c = if b.glide_sm == 0.0 { g.enter } else { g.loop_add };
        sample_clip(sk, c, b.glide_sm_time, a);
        accumulate(a, &c.bones, 1.0, true, gp); // ApplyAdditive_1 (alpha 1)
        if select(sc, g.add, b.glide_add_x) {
            rows_pose(sk, sc, g.add, b.glide_add_t, a, sp);
            accumulate(a, &g.add.bones, 1.0, true, gp); // ApplyAdditive (alpha 1)
        }
        if gw >= 1.0 - MIN_W {
            out = 1;
        } else {
            // BlendListByBool's blend: child 0 x gw + child 1 x (1 - gw)
            weight_pose(gp, gw, sp);
            add_weighted_pose(l, 1.0 - gw, sp);
            normalize_rotations(sp);
            out = 2;
        }
    }
    let out = match out {
        0 => l,
        1 => gp,
        _ => sp,
    };
    // ModifyBone_4: Root + (0, 210, 0) cm in the actor's space x IsGlidingAlpha (the root has no parent: its
    // translation)
    let shift = (DEPTH * min(ga, 1.0))
        / crate::math::narrow(sk.index.mesh_scale * 100.0 * f64::from(w.fighter(f).body.size_scale));
    if ga > MIN_W {
        out.t[2] = f32_of(crate::real(out.t[2]) + shift);
    }
    write_shapes(sk, sc, out, s);
    if ga > MIN_W {
        out.t[2] = f32_of(crate::real(out.t[2]) - shift);
    }
    true
}
