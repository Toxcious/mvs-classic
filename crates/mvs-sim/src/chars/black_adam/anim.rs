//! C021_AnimBP's sim side: the four hover BlendListByInt (MoveBack, MoveForward,
//! MoveDown, MoveUp) on Hover Direction, the AimGrab blends on the aim direction, and the slot mix composed for the
//! pose (AirState Fall's blend space where no slot of this graph plays).
//!
//! The montages whose tracks sit in the MoveUp / MoveDown / MoveForward / MoveBack slots
//! (C021_Special_Flight_Version2, C021_Special_Ground_Flight_Version2) or the AimGrabUp / AimGrabDown slots
//! (C021_Special_LightningBall) show the weighted sum of them, so the hit and hurt shapes and the drawn pose are one
//! mix. The AnimGraph from the base layer up (LinkID L is ChildProperties[L + 1]):
//!   Slot_7 DefaultSlot (over Slot_8 JumpSlot, over the BaseMovementMachine layer) -> cached "BeforeHover" (BH)
//!   BlendListByInt_3 [BH, Slot_3 MoveUp(BH)]       ActiveChildIndex = FTrunc(Hover Direction.Y)        @1999
//!   BlendListByInt_2 [that, Slot_2 MoveDown(BH)]   ActiveChildIndex = FTrunc(Hover Direction.Y) x -1   @1888
//!   BlendListByInt_1 [that, Slot_1 MoveForward(BH)] ActiveChildIndex = FTrunc(Hover Direction.X)       @2438
//!   BlendListByInt [that, Slot MoveBack(BH)]       ActiveChildIndex = FTrunc(Hover Direction.X) x -1   @2591
//!   -> cached "BeforeAim" (BA)
//!   TwoWayBlend_1 (A Slot_6 AimGrabBase(BA), B Slot_5 AimGrabUp(BA), Alpha <- Aim Direction)
//!   TwoWayBlend (A TwoWayBlend_1, B Slot_4 AimGrabDown(BA), Alpha <- Aim Direction Inverted)
//! The four lists: BlendTime 0.2 s for both poses, StandardBlend, linear; the four move slots have
//! bAlwaysUpdateSourcePose, the three aim slots do not. Aim Direction Inverted = Aim Direction x -1 @1596-1638.
//!
//! Native rules (FAnimNode_BlendListByInt, FAlphaBlend / FAnimNode_BlendListBase, FAnimNode_TwoWayBlend):
//!   BlendListByInt: an index under 0 is child 0, else min(index, 1). A change of child blends linearly over
//!   BlendTime x (1 - the new child's weight): the weight moves at 1 / 0.2 s. The first update after the instance's
//!   Initialize snaps (LastActiveChildIndex INDEX_NONE). A child at or under 0.00001 is not updated, so an inner
//!   list holds its weights (and its first snap) while the outer list shows the move slot alone.
//!   TwoWayBlend: a float alpha, scale 1, bias 0; clamped to 0..1, A alone at 0.00001 and under, B alone at 0.99999
//!   and over; no blend time.
//!   Slot (no bAlwaysUpdateSourcePose): the source is updated only with a source weight above 0.00001, so BA (the
//!   four lists) is not updated while every aim slot the TwoWayBlends update shows a full-weight montage track.
//! The lists step by the instance's delta (the hit pause's dilation in it) after the montage advance of the tick.
//! Weights: Back b, Forward (1 - b) f, Down (1 - b)(1 - f) d, Up (1 - b)(1 - f)(1 - d) u, BH the rest; then Base
//! (1 - D)(1 - U'), AimGrabUp (1 - D) U', AimGrabDown D with U' = band(Aim Direction), D = band(Aim Direction
//! Inverted). A slot without a track in the montage passes its source on: a move slot BH (the DefaultSlot track),
//! an aim slot BA (the hover mix).
// Deviation: the nested blends are one weighted sum, and the shapes mix the sampled box tracks linearly with the
// same weights.
// Deviation: the port does not update a dead pawn's AnimBP (the build's mesh still ticks): the lists hold their
// weights through a death and the respawn, and a death empties the mix.
// Deviation: the hold of BA reads the shown montage's full weight and tracks; a second montage blending out under it
// is not counted (no montage of his has the aim tracks beside another one that does).
// Unverified: how the leans look against the live game; the graph is read, the pictures are the port's.

use super::state::{black_adam, black_adam_mut, black_adam_w};
use crate::Real;
use crate::consts::DT;
use crate::fighter::FighterId;
use crate::fighter::anim::SlotMix;
use crate::fighter::anim::blend_spaces::fall_mix;
use crate::fighter::anim::mix_slots;
use crate::fighter::anim::montages::{instance_data, state_ref};
use crate::fighter::anim::slot_blend::SlotPart;
use crate::fighter::anim::update::shown_uid;
use crate::math::{max, min};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// The lists in the graph's order from the top: BlendListByInt (MoveBack), _1 (MoveForward), _2 (MoveDown), _3
/// (MoveUp).
const MOVE: [&str; 4] = ["MoveBack", "MoveForward", "MoveDown", "MoveUp"];
const BLEND_TIME: Real = 0.2;
/// ZERO_ANIMWEIGHT_THRESH.
const ZERO_W: Real = 0.00001;

/// TwoWayBlend's clamp and band.
fn band(a: Real) -> Real {
    if a >= 0.99999 {
        1.0
    } else if a > ZERO_W {
        a
    } else {
        0.0
    }
}

/// FTrunc (toward zero, of the float), then the list's clamp: under 0 is child 0, else min(index, 1).
fn list_child(v: Real) -> i32 {
    // trunc(fround(v)) > 0 is fround(v) >= 1
    i32::from(v >= 1.0)
}

/// Whether the shown montage has a track in `slot`, and whether it is at full weight.
fn track_in(w: &World, f: FighterId, slot: &str) -> (bool, bool) {
    let Some(uid) = shown_uid(w, f) else { return (false, false) };
    let (Some(m), Some(s)) = (instance_data(&w.data, w, f, uid), state_ref(w, f, uid)) else { return (false, false) };
    (m.slots.iter().any(|t| t.slot == slot && !t.segments.is_empty()), s.full_weight(m))
}

/// Whether a slot the TwoWayBlends update passes its source on (a Slot without bAlwaysUpdateSourcePose): not when the
/// shown montage at full weight has a track in it.
fn passes(w: &World, f: FighterId, slot: &str) -> bool {
    let (track, full) = track_in(w, f, slot);
    !(full && track)
}

/// Whether BA is updated: through an aim slot that the blends update and that passes its source on.
fn before_aim_updated(w: &World, f: FighterId) -> bool {
    let aim = w.fighter(f).pose.aim_direction;
    let (up, down) = (band(aim), band(-aim));
    (down < 1.0 && ((up < 1.0 && passes(w, f, "AimGrabBase")) || (up > 0.0 && passes(w, f, "AimGrabUp"))))
        || (down > 0.0 && passes(w, f, "AimGrabDown"))
}

/// `layerHeld` (CharHooks.layerHeld): every path from the root to the BaseMovementMachine layer goes through BA
/// (Slot_7 DefaultSlot has bAlwaysUpdateSourcePose, Slot_8 JumpSlot is the data's own set): while BA is not updated
/// (C021_Special_LightningBall at full weight with the aim at a side, its AimGrabUp or AimGrabDown track alone
/// weighted) the layer gets no update either.
// The layer under the aim slots is held as well.
pub fn layer_held(w: &World, f: FighterId) -> bool {
    !before_aim_updated(w, f)
}

/// `animUpdate` (joined): BlueprintUpdateAnimation and the graph's update of the four lists (after the montages).
pub fn anim_update(w: &mut World, f: FighterId, dilation: Real, pre: bool) {
    if pre {
        return;
    }
    if before_aim_updated(w, f) {
        let s = black_adam_w(w, f);
        let (x, y) = (s.hover_x, s.hover_y);
        let index = [list_child(-x), list_child(x), list_child(-y), list_child(y)];
        let step = (DT * dilation) / BLEND_TIME;
        for (k, &i) in index.iter().enumerate() {
            if s.list_index[k] < 0 {
                s.list_w[k] = crate::real(i);
            } else if i != 0 {
                s.list_w[k] = min(s.list_w[k] + step, 1.0);
            } else {
                s.list_w[k] = max(s.list_w[k] - step, 0.0);
            }
            s.list_index[k] = i;
            if 1.0 - s.list_w[k] <= ZERO_W {
                break; // child 0, the next list, not updated
            }
        }
    }
    slot_compose(w, f);
}

type Parts = Vec<(SlotPart<'static>, Real)>;

/// A part with a positive weight.
fn put(parts: &mut Parts, slot: &'static str, wt: Real) {
    if wt > 0.0 {
        parts.push((SlotPart::Slot(slot), wt));
    }
}

/// BA's parts, scaled by `wt`: a move slot without a track is BH, the DefaultSlot track (or its own source).
fn hover_parts(w: &World, f: FighterId, parts: &mut Parts, wt: Real) {
    let mut rest = wt;
    for (k, slot) in MOVE.iter().enumerate() {
        let part = rest * black_adam(w, f).list_w[k];
        put(parts, if track_in(w, f, slot).0 { slot } else { "DefaultSlot" }, part);
        rest -= part;
    }
    put(parts, "DefaultSlot", rest);
}

/// An aim slot's part: its track, or BA (the hover mix) without one.
fn aim_part(w: &World, f: FighterId, parts: &mut Parts, slot: &'static str, wt: Real) {
    if wt <= 0.0 {
        return;
    }
    if track_in(w, f, slot).0 {
        put(parts, slot, wt);
    } else {
        hover_parts(w, f, parts, wt);
    }
}

/// `slotCompose` (`poseUpdated`, joined): the mix, wherever the pose is written and after the update above.
pub fn slot_compose(w: &mut World, f: FighterId) {
    let uid = shown_uid(w, f);
    let own = uid
        .and_then(|u| instance_data(&w.data, w, f, u))
        .is_some_and(|m| m.slots.iter().any(|t| t.slot.starts_with("Move") || t.slot.starts_with("AimGrab")));
    let mut out = std::mem::take(&mut black_adam_w(w, f).slot_mix.slot_mix);
    if uid.is_none() || !own {
        fall_mix(w, f, &mut out); // no slot of this graph: AirState Fall's blend space, or nothing
    } else {
        let aim = w.fighter(f).pose.aim_direction;
        let (up, down) = (band(aim), band(-aim));
        let mut parts = Vec::with_capacity(16);
        aim_part(w, f, &mut parts, "AimGrabBase", (1.0 - down) * (1.0 - up));
        aim_part(w, f, &mut parts, "AimGrabUp", (1.0 - down) * up);
        aim_part(w, f, &mut parts, "AimGrabDown", down);
        mix_slots(w, f, uid, &parts, &mut out, None);
    }
    black_adam_w(w, f).slot_mix.slot_mix = out;
}

/// The joined `death`: the slot mix cleared (the port does not update a dead pawn's AnimBP).
pub fn death(w: &mut World, f: FighterId) {
    black_adam_w(w, f).slot_mix.slot_mix.clear();
}

/// CharHooks.slotMix.
pub fn slot_mix(w: &World, f: FighterId) -> Option<&SlotMix> {
    Some(&black_adam(w, f).slot_mix)
}

/// `slotMix`, written (CharHooks.slot_mix_mut).
pub fn slot_mix_mut(w: &mut World, f: FighterId) -> Option<&mut SlotMix> {
    black_adam_mut(w, f).map(|s| &mut s.slot_mix)
}

/// This file's registry entries (none: its hooks are composed by pawn.rs).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let _ = r;
    Ok(())
}
