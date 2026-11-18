//! Anim root motion: the montage's root motion extraction and its use by the character's
//! movement (UAnimMontage, UAnimInstance, PerformMovement). The rules and their sources are in the comments of the
//! functions.

// A written-out `!(a > b)` is true for a NaN where `a <= b` is not.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::MontageSlot;
use super::montages::{char_data, fdata};
use crate::Real;
use crate::fighter::FighterId;
use crate::fighter::common::warn_text;
use crate::world::World;
use mvs_anim::clip::STEP_EPS;
use mvs_anim::montage::Montage;

/// UAnimMontage::HasRootMotion (142857840): a segment of any slot track whose sequence has bEnableRootMotion (the
/// box index's `rootMotion`).
pub fn has_root_motion(w: &World, f: FighterId, m: &Montage) -> bool {
    let Some(c) = char_data(&w.data, &w.names, w.fighter(f).ch) else { return false };
    let anims = &c.boxes.anims;
    m.slots.iter().any(|s| s.segments.iter().any(|g| anims.get(&g.anim).is_some_and(|a| a.root_motion)))
}

/// `rootMontage`: the AnimInstance's RootMotionMontageInstance, the attack montage while
/// it is not stopped.
/// Unverified: DisableRootMotionCount is taken as 0; a montage of another slot group or a plain montage with root
/// motion is not the instance here (`not_root_instance` warns).
pub fn root_montage(w: &mut World, f: FighterId) -> Option<MontageSlot> {
    let m = w.fighter(f).montages.montage.as_ref()?;
    if m.ended || m.blending_out() {
        return None;
    }
    let md = fdata(w, f).montages.get(w.names.str(m.montage.name))?;
    if has_root_motion(w, f, md) { Some(MontageSlot::Main) } else { None }
}

/// `notRootInstance`: a montage played outside the attack instance would be the root-motion instance in the build.
pub fn not_root_instance(w: &World, f: FighterId, m: &Montage) {
    if has_root_motion(w, f, m) {
        warn_text(w, &format!("root motion montage outside the attack instance: {}", m.name));
    }
}

/// `rootInto`: the root track (fwd, up) of a sequence at a time, the key lookup of the box
/// sample (Step sequences take the earlier frame). None without the sequence.
pub fn root_into(w: &World, f: FighterId, anim: &str, time: Real) -> Option<(Real, Real)> {
    let c = char_data(&w.data, &w.names, w.fighter(f).ch)?;
    let a = c.boxes.anims.get(anim)?;
    let stride = c.boxes.stride as usize;
    let data = &c.box_data.0;
    let n = crate::real(a.num_frames);
    let fr = if n > 1.0 && a.length > 0.0 {
        crate::math::min(crate::math::max(time / crate::math::narrow(a.length), 0.0), 1.0) * (n - 1.0)
    } else {
        0.0
    };
    if fr.is_nan() {
        // an index of NaN reads nothing (undefined in JavaScript): the sums are NaN
        return Some((Real::NAN, Real::NAN));
    }
    let at = |i: Real| a.offset as usize + usize::try_from(crate::math::to_int32(i)).unwrap_or(0) * stride;
    if a.step == Some(true) {
        let b = at(crate::math::min((fr + crate::math::narrow(STEP_EPS)).floor(), n - 1.0));
        return Some((crate::real(data[b]), crate::real(data[b + 1])));
    }
    let i0 = fr.floor();
    let i1 = crate::math::min(i0 + 1.0, n - 1.0);
    let wt = fr - i0;
    let (b0, b1) = (at(i0), at(i1));
    Some((
        crate::real(data[b0]) * (1.0 - wt) + crate::real(data[b1]) * wt,
        crate::real(data[b0 + 1]) * (1.0 - wt) + crate::real(data[b1 + 1]) * wt,
    ))
}

/// `rootStep` (UAnimMontage::ExtractRootMotionFromTrackRange 14281e730) for one step a -> b of instance `uid`'s
/// position, added to the fighter's ExtractedRootMotion in mesh space.
/// Unverified: the range algebra is stock (translation only); a Step sequence's root track is read by the pose's key
/// lookup; a segment played backwards gives nothing.
pub fn root_step(w: &mut World, f: FighterId, uid: u32, m: &Montage, a: Real, b: Real) {
    if w.fighter(f).root.extracting != Some(uid) {
        return;
    }
    w.fighter_mut(f).root.root_has = true;
    let Some(slot) = m.slots.first() else { return };
    let data = w.data.clone();
    let Some(c) = char_data(&data, &w.names, w.fighter(f).ch) else { return };
    let n = crate::math::narrow;
    for g in &slot.segments {
        // the segment's floats (FAnimSegment StartPos, AnimStartTime / AnimEndTime, AnimPlayRate, LoopingCount)
        let (start, a0, a1, rate) = (n(g.start), n(g.a0), n(g.a1), n(g.rate));
        let span = (a1 - a0).abs();
        if !(span > 0.0) || !(rate > 0.0) || !c.boxes.anims.get(&g.anim).is_some_and(|x| x.root_motion) {
            continue;
        }
        let loops = crate::math::max(n(g.loops), 1.0);
        let lo = crate::math::max(a, start);
        let hi = crate::math::min(b, start + (span * loops) / rate);
        if !(hi > lo) {
            continue;
        }
        let l0 = (lo - start) * rate;
        let l1 = (hi - start) * rate;
        let dir = if a1 >= a0 { 1.0 } else { -1.0 };
        let mut k = crate::math::min((l0 / span).floor(), loops - 1.0);
        while k < loops && k * span < l1 {
            let s = crate::math::max(l0 - k * span, 0.0);
            let e = crate::math::min(l1 - k * span, span);
            if e > s {
                let (Some(r0), Some(r1)) =
                    (root_into(w, f, &g.anim, a0 + dir * s), root_into(w, f, &g.anim, a0 + dir * e))
                else {
                    break;
                };
                let r = &mut w.fighter_mut(f).root;
                r.root_dx += r1.0 - r0.0;
                r.root_dy += r1.1 - r0.1;
            }
            k += 1.0;
        }
    }
}

/// `consumeRoot`: TickCharacterPose's consume and PerformMovement's use of it.
/// Unverified: ConvertLocalRootMotionToWorld's algebra is stock; AnimRootMotionTranslationScale is taken as 1.
pub fn consume_root(w: &mut World, f: FighterId, dt: Real, walking: bool, falling: bool, always: bool) -> bool {
    let fi = w.fighter_mut(f);
    if !fi.root.root_has && !always {
        return false;
    }
    let dx = fi.root.root_dx * fi.body.facing_dir * fi.body.size_scale;
    let dy = fi.root.root_dy * fi.body.size_scale;
    fi.root.root_dx = 0.0;
    fi.root.root_dy = 0.0;
    fi.root.root_has = false;
    if !(dt > 0.0) {
        return false;
    }
    fi.root.root_vel = (dx / dt, dy / dt);
    fi.body.vx = fi.root.root_vel.0;
    if walking {
        fi.body.vy = 0.0;
    } else if !falling {
        fi.body.vy = fi.root.root_vel.1;
    }
    true
}
