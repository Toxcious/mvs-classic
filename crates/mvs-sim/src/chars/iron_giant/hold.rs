//! Buff_HeldBy_C017 (a child of Buff_HeldByEnemy,
//! `combat::holds`): the grab's hold, and the Giant's HoldingBuffValid / HoldAlpha it sets. Its kind is read by
//! `combat::holds::hold_kind`; the class is in the holds' list there (`isHold`), so it enters nothing in the
//! registry.

use super::state::{giant, giant_w};
use crate::combat::holds::{Hold, HoldKind};
use crate::fighter::FighterId;
use crate::fighter::moves::is_falling;
use crate::name;
use crate::names::Name;
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// Buff_HeldBy_C017_C's class.
pub const CLASS: Name = name!("Buff_HeldBy_C017_C");

/// ReceiveBeginPlay @3005: the parent's; the owner is a BP_FighterCharacter -> SuccessfullyApplied (@3110); the
/// instigator's BPI_SetIsHoldingEnemy(true) (@3220); SetHoldOverride(true) (@2727 -> 1736: the instigator's AnimBP
/// BPI_GrabOverride(true) @1992, HoldingBuffValid); BPI_SetCurrentHoldBuff(self) (@2817).
fn begin(w: &mut World, vic: FighterId) {
    let Some(h) = &mut w.fighter_mut(vic).holds.hold else { return };
    h.applied = true;
    let by = h.by;
    w.fighter_mut(by).holds.holding_enemy = true;
    giant_w(w, by).hold_valid = true;
    w.fighter_mut(by).holds.hold_buff = Some(vic);
}

/// ReceiveTick @3434: InstigatorRef valid and its CharacterMovement.IsFalling -> once, K2_DestroyActor (@15). Hover is
/// Falling too: Pawn_C017 writes no movement mode anywhere (it only reads it, @2878 / @4372 / @5502 / @19532); the
/// hover is NetGravity(-4) (@6459) and ThrusterLaunchSelf's LaunchCharacter (@6829 -> @25637). So a jump, a walk off an
/// edge or the hover ends the hold at the buff's next tick.
fn tick(w: &mut World, vic: FighterId) -> bool {
    let Some(h) = &w.fighter(vic).holds.hold else { return false };
    is_falling(w, h.by)
}

/// ReceiveEndPlay @3263: the parent's; with SuccessfullyApplied: BPI_SetIsHoldingEnemy(false) (@3391),
/// SetHoldOverride(false) (@2876: HoldAlpha 0 @2148, BPI_GrabOverride(false) @2310), BPI_SetCurrentHoldBuff(null)
/// (@2966).
fn end(w: &mut World, _vic: FighterId, h: &Hold) {
    if !h.applied {
        return;
    }
    let by = h.by;
    w.fighter_mut(by).holds.holding_enemy = false;
    let g = giant_w(w, by);
    g.hold_alpha = 0.0;
    g.hold_valid = false;
    w.fighter_mut(by).holds.hold_buff = None;
}

/// AttachSocket 'VB Fighter_Hold_Bone': a virtual bone on HitCollision01 (source and target), which C017_AnimBP's
/// ModifyBone_4 moves to HoldLocation, the main mesh's socket fml_un_R_middle_base in the play plane (@14853-15017),
/// by HoldAlpha x HoldingBuffValid (@10652-10735).
/// Product 0: where the animation has HitCollision01 (the track of Fighter_Hold_Socket, a socket on that bone with no
/// offset). Product 1 (the two are only ever 0 or 1): the socket's track.
/// The bone's track comes from the boxes bind's GRAPH_SOCKETS; data built without it falls back to the wrist of the
/// same hand.
#[allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-keeping negated tests
fn track(w: &World, by: FighterId) -> Name {
    let g = giant(w, by);
    if !(g.hold_alpha * (if g.hold_valid { 1.0 } else { 0.0 }) >= 1.0) {
        return name!("socket:Fighter_Hold_Socket");
    }
    let middle = name!("socket:fml_un_R_middle_base");
    let has = w.fixed.samplers.get(&w.fighter(by).ch).is_some_and(|b| b.by_name.contains_key(&middle));
    if has { middle } else { name!("socket:fml_un_R_wrist") }
}

/// The kind `combat::holds::hold_kind` returns for Buff_HeldBy_C017_C.
pub const KIND: HoldKind = HoldKind {
    before_begin: None,
    begin: Some(begin),
    tick: Some(tick),
    end: Some(end),
    track: Some(track),
    warning: None,
    exited_arena: None,
    latent: None,
};

/// This file's registry entries: none (the class is a hold, listed in `combat::holds`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let _ = r;
    Ok(())
}
