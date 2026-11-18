//! The fighter as a notify sink: the begin, end and instant calls of its montages'
//! notifies to the registry's handlers, with the classes it polls, defers or takes as doing nothing.

// A written-out `!(a > b)` is true for a NaN where `a <= b` is not.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::montages::truthy;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::common::warn_text;
use crate::world::World;
use mvs_anim::montage::{Notify, NotifyTick};

/// `DEFERRED_ENDS`: the queued states whose NotifyEnd waits for the next dispatch when the montage is stopped after
/// this frame's one (each class's reason is in the comment of its entry).
const DEFERRED_ENDS: &[&str] = &[
    "AnimNotify_Crouch_C",
    "Ak_Continuous_C",
    "Ak_VO_Effort_Continuous_C",
    "AnimNotify_PlayVFX_C",
    "AnimNotify_ConditionPlayVFX_C",
    "AnimNotifyState_TimedParticleEffect",
    "AnimNotifyState_PlayTimedVFX",
    "AnimNotify_Spawner_C",
    "AnimNotify_ConditionalSpawner_C",
    "AnimNotify_SetPlayerInvulnerable_C",
    "AnimNotify_ChargingAttack_C",
    "AnimNotify_isAttackingFlag_C",
    "AnimNotify_ConditionalIsAttacking_C",
    "AnimNotify_ChargeHoldUI_C",
    "AnimNotify_SetGravity_C",
    "AnimNotify_HitPausePoseFrames_C",
    "AnimNotify_TurnOffOutline_C",
    "AnimNotify_Armor_C",
    "AnimNotify_RageCharge_C",
    "AnimNotify_RemoveRage_C",
    "AnimNotify_Equip_C",
    "AnimNotify_IncrementAirSpecial_C",
    "AnimNotify_SetFlyingMoveMode_C",
    "AnimNotify_RemoveBuff_C",
    "AnimNotify_C020ListenForAbilityHit_C",
    "AnimNotify_ClearUnactivatedMeeseeks_C",
    "AnimNotify_C018DamageFromDisintegration_C",
];

/// `deferredEnd`: AnimNotify_ActivateSpawned only with ActivateOnEnd?.
fn deferred_end(n: &Notify) -> bool {
    DEFERRED_ENDS.contains(&n.cls.as_str())
        || (n.cls == "AnimNotify_ActivateSpawned_C" && truthy(n.p.get("ActivateOnEnd?")))
}

/// `POLLED_NOTIFIES`: states the sim polls with `has_active` / `active_of`: no warning when they start.
const POLLED_NOTIFIES: [&str; 9] = [
    "AnimNotify_AerialAttack_C",
    "AnimNotify_BlockFastFall_C",
    "AnimNotify_BranchWindow_C",
    "AnimNotify_ConditionSectionJump_C",
    "AnimNotify_CrouchStopAnim_C",
    "AnimNotify_MovementStopAnim_C",
    "AnimNotify_ButtonHoldSectionJump_C",
    "AnimNotify_OffGroundStopAnim_C",
    "AnimNotify_RemoveRage_C",
];

/// `NOOP_NOTIFIES`: notifies decoded as doing nothing in this build with a keyboard player (each
/// one's reason).
const NOOP_NOTIFIES: [&str; 9] = [
    "FaceOverride_C",
    "AnimNotify_SetRotationRightStick_C",
    "AnimNotify_PlaySound",
    "SpawnShockWaveOffset",
    "PlayVFXWithCondition_C",
    "EquipHolster_C",
    "AnimNotify_FadePlayerUI_C",
    "AnimNotify_IgnorePlatforms_C",
    "AnimNotify_ResocketItem_C",
];

/// `begin` with the notify's data at hand.
pub(crate) fn begin_of(w: &mut World, f: FighterId, n: &Notify, r: NotifyRef, idx: i32) {
    let h = w.registry.notifies.get(&n.cls).copied();
    match h {
        Some(h) if h.begin.is_some() || !h.strict => {
            if let Some(b) = h.begin {
                b(w, f, r, idx);
            }
        }
        _ => {
            let c = n.cls.as_str();
            if !POLLED_NOTIFIES.contains(&c) && !NOOP_NOTIFIES.contains(&c) {
                warn_text(w, &format!("notify class not handled: {c}"));
            }
        }
    }
}

/// `end` with the notify's data at hand.
pub(crate) fn end_of(w: &mut World, f: FighterId, n: &Notify, r: NotifyRef, idx: i32) {
    if w.fighter(f).branching.defer_queued_ends && n.tick != NotifyTick::BranchingPoint && deferred_end(n) {
        w.fighter_mut(f).branching.pending_ends.push((r, idx));
        return;
    }
    if let Some(e) = w.registry.notifies.get(&n.cls).and_then(|h| h.end) {
        e(w, f, r, idx);
    }
}

/// `instant` with the notify's data at hand.
pub(crate) fn instant_of(w: &mut World, f: FighterId, n: &Notify, r: NotifyRef) {
    let h = w.registry.notifies.get(&n.cls).copied();
    match h {
        Some(h) if h.instant.is_some() || !h.strict => {
            if let Some(i) = h.instant {
                i(w, f, r);
            }
        }
        _ => {
            if !NOOP_NOTIFIES.contains(&n.cls.as_str()) {
                warn_text(w, &format!("notify class not handled: {}", n.cls));
            }
        }
    }
}

/// `begin`.
pub fn begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let Some(nd) = notify_in(&data, w, f, n) else { return };
    begin_of(w, f, nd, n, idx);
}

/// The notify `n` names in the fighter's data (its montages', else its sequence notifies), off the World's borrow.
fn notify_in<'d>(d: &'d mvs_data::GameData, w: &World, f: FighterId, n: NotifyRef) -> Option<&'d Notify> {
    let c = super::montages::char_data(d, &w.names, w.fighter(f).ch)?;
    let name = w.names.str(n.montage);
    match c.fighter.montages.get(name) {
        Some(m) => m.notifies.get(n.index as usize),
        None => c
            .def
            .seq_notifies
            .get(name)
            .and_then(|s| s.get(n.index as usize))
            .or_else(|| super::montages::any_notify_in(d, w, f, n)),
    }
}

/// `end`.
pub fn end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    // (an end held for the next dispatch may be of a second mesh's montage: the notify itself is held)
    let Some(nd) = super::montages::any_notify_in(&data, w, f, n) else { return };
    end_of(w, f, nd, n, idx);
}

/// `instant`.
pub fn instant(w: &mut World, f: FighterId, n: NotifyRef) {
    let data = w.data.clone();
    let Some(nd) = notify_in(&data, w, f, n) else { return };
    instant_of(w, f, nd, n);
}
