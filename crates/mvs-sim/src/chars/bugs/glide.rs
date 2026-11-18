//! BugsBunny_AnimBP's glide list in the sim: the BlendListByBool over the Slot
//! nodes, child 0 the SequencePlayer of BugsBunny_Nav_PropellerEars, child 1 the linked layer (the shared graph).
//!
//! The list's update (FAnimNode_BlendListBase::Update with the FAlphaBlend rule): bActiveValue = IsGliding AND
//! VerticalVelocity < 500 (@1088-1160): IsGliding the AnimBP's own (`BugsState::anim_gliding`), VerticalVelocity the
//! owner's GetVelocity().Z taken in BlueprintUpdateAnimation (@3062-3162; the sim's vy, as a float). BlendTime
//! [0.2, 0.1] linear, bResetChildOnActivation false. On a change of child the weights go linearly from where they are
//! over BlendTime[new] x |1 - weight[new]|, the first step in the update of the change; the first update after the
//! instance's Initialize snaps (LastActiveChildIndex INDEX_NONE). A child at or under 0.00001 is not updated: the
//! player keeps its time. A 0 s change from child 0 updates the departed player once at weight 0. The player (looping,
//! PlayRate 1) moves by RateScale x PlayRate x delta: the asset's RateScale is 1.5.
//! The list sits under Slot DefaultSlot (bAlwaysUpdateSourcePose true) and Slot Override, so it is updated in every
//! update of the instance, a montage's included: the mesh tick's update after the montages (`anim_update`, not `pre`),
//! at the hit pause's dilation. Not while dead: the port does not update a dead pawn's AnimBP, so the list holds
//! through a death.
//! The notifies (as `seq_notify_tick`'s): an updated player lists its notifies by GetAnimNotifiesFromDeltaPositions
//! from its time before the update to the one after (in pieces at the loop); one passes when max(weight, 0.00001) >=
//! TriggerWeightThreshold, 0.00001 on both of this asset's notifies: every update of the player fires, at any weight.
//! Ak_OneShot sfx_cha_bugs_foley_propeller is an instant (once per loop of the 0.2 s clip); AnimNotify_PlayVFX
//! PS_BugsBunnyPropellersEars a state from 0.0001 to 0.2001, past the clip's end, so a looping player lists it in every
//! update. The advance of the player not yet dispatched is `glide_seq_from` / `glide_seq_delta`.
// Unverified: the queue's order puts child 0's notifies before the linked layer's (the list updates its children in
// index order); the port lists them first in seq_notify_tick, and the montages' dispatch before both, as for the
// layer's.

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated comparisons, kept for their NaN result")]

use super::glide_box::GLIDE_SEQ;
use super::state::{BugsState, bugs, bugs_w};
use crate::Real;
use crate::consts::DT;
use crate::fighter::FighterId;
use crate::fighter::anim::{seq_base, seq_fire};
use crate::fighter::common::{char_data, warn_text};
use crate::math::ue::advance_time;
use crate::math::{max, min};
use crate::name;
use crate::registry::CharHooks;
use crate::world::World;

/// The list's BlendTime per child.
const BLEND_TIME: [Real; 2] = [0.2, 0.1];
/// VerticalVelocity < 500 (@1088-1160).
const GLIDE_VZ: Real = 500.0;
/// ZERO_ANIMWEIGHT_THRESH.
const ZERO_W: Real = 0.00001;

/// The hooks this file joins in Bugs Bunny's CharHooks (`anim_update`, `seq_players`, `layer_held`).
pub fn hooks(h: &mut CharHooks) {
    h.anim_update = Some(glide_update);
    h.seq_players = Some(glide_notifies);
    h.layer_held = Some(layer_held);
}

/// The clip's length and rate scale in the box index, `None` (warned) when the data has no such clip.
fn clip(w: &mut World, f: FighterId) -> Option<(Real, Real)> {
    let ch = w.fighter(f).ch;
    match char_data(w, ch).boxes.anims.get(GLIDE_SEQ) {
        Some(a) => Some((crate::math::narrow(a.length), crate::math::narrow(a.rate_scale))),
        None => {
            warn_text(w, &format!("bugs: the glide list plays {GLIDE_SEQ}, which the data does not have"));
            None
        }
    }
}

/// The player's update by dt (the instance's delta): its time, and the advance its notifies are listed over.
fn step_player(w: &mut World, f: FighterId, dt: Real) {
    let Some((len, rate)) = clip(w, f) else { return };
    let d = dt * rate;
    let s: &mut BugsState = bugs_w(w, f);
    if s.glide_seq_delta == 0.0 {
        s.glide_seq_from = s.glide_time;
    }
    s.glide_seq_delta += d;
    advance_time(true, d, &mut s.glide_time, len);
}

/// The list's blend in an update: its weights' interpolation and the child it last activated.
struct Blend {
    from_w: Real,
    to_w: Real,
    time: Real,
    elapsed: Real,
    last: i32,
    /// A 0 s change from child 0: the departed player's update at weight 0.
    departed: bool,
}

impl Blend {
    /// Child 0's weight at the end of the update.
    fn weight(&self) -> Real {
        let a = if self.time > 0.0 { min(max(self.elapsed / self.time, 0.0), 1.0) } else { 1.0 };
        self.from_w + (self.to_w - self.from_w) * a
    }
}

/// The child the list activates in this update: IsGliding AND VerticalVelocity < 500.
fn wants_glide(w: &World, f: FighterId) -> bool {
    bugs(w, f).anim_gliding && w.fighter(f).body.vy < GLIDE_VZ
}

/// The list's blend after an update by `dt` toward `want`'s child (a read of the state; `glide_update` writes it).
fn blend_after(s: &BugsState, want: bool, dt: Real) -> Blend {
    let child: i32 = if want { 0 } else { 1 };
    let mut b = Blend {
        from_w: s.glide_from_w,
        to_w: s.glide_to_w,
        time: s.glide_blend,
        elapsed: s.glide_elapsed,
        last: s.glide_last,
        departed: false,
    };
    if child != s.glide_last {
        let wc = if child == 0 { s.glide_w } else { 1.0 - s.glide_w };
        b.time = if s.glide_last < 0 {
            0.0
        } else {
            BLEND_TIME[usize::try_from(child).expect("a child index")] * min((1.0 - wc).abs(), 1.0)
        };
        b.departed = b.time == 0.0 && s.glide_last == 0;
        b.from_w = s.glide_w;
        b.to_w = if child == 0 { 1.0 } else { 0.0 };
        b.elapsed = 0.0;
        b.last = child;
    }
    b.elapsed += dt;
    b
}

/// BlueprintUpdateAnimation's VerticalVelocity and the graph's update of the list (after the montages).
fn glide_update(w: &mut World, f: FighterId, dilation: Real, pre: bool) {
    if pre {
        return;
    }
    let dt = DT * dilation;
    let b = blend_after(bugs(w, f), wants_glide(w, f), dt);
    if b.departed {
        step_player(w, f, dt); // the departed child's update at weight 0
    }
    let s = bugs_w(w, f);
    s.glide_from_w = b.from_w;
    s.glide_to_w = b.to_w;
    s.glide_blend = b.time;
    s.glide_elapsed = b.elapsed;
    s.glide_last = b.last;
    s.glide_w = b.weight();
    if s.glide_w > ZERO_W {
        step_player(w, f, dt);
    }
}

/// `CharHooks.layer_held`: the list updates only the children whose weight is above ZERO_ANIMWEIGHT_THRESH
/// (FAnimNode_BlendListBase::Update), so while child 0 holds the whole weight the linked layer (child 1, the shared
/// BaseMachine layer) gets no update: its machines and clips stand, and the machines restart at their entry states
/// when the layer is relevant again (`layer_frozen`, `relevancy_restart`). The weight is the one this update gives
/// the list: the port updates the layer ahead of the list (`glide_update`, after the pose), the build inside it.
/// A dead pawn's list is not updated (its weight stands).
// Unverified: the update's delta is taken as the frame's (DT): the layer is updated in no dilated update.
fn layer_held(w: &World, f: FighterId) -> bool {
    let s = bugs(w, f);
    let wt = if crate::fighter::mode(w, f) == crate::fighter::tick::FrameMode::Dead {
        s.glide_w
    } else {
        blend_after(s, wants_glide(w, f), DT).weight()
    };
    !(1.0 - wt > ZERO_W)
}

/// The player's notifies of the advance since the last pass (`seq_notify_tick`'s; `listed` None: the pawn is dead, the
/// pass lists nothing and the advance is dropped).
fn glide_notifies(w: &mut World, f: FighterId, listed: Option<&mut Vec<i32>>) {
    let s = bugs_w(w, f);
    let d = s.glide_seq_delta;
    if d <= 0.0 {
        return;
    }
    s.glide_seq_delta = 0.0;
    let from = s.glide_seq_from;
    let Some(listed) = listed else { return };
    let Some((len, _)) = clip(w, f) else { return };
    let anim = name!("BugsBunny_Nav_PropellerEars");
    let base = seq_base(w, f, anim);
    seq_fire(w, f, anim, from, d, true, len, base, listed);
}
