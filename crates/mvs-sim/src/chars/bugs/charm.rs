//! Buff_Charm: the charm of the ground kiss and of the Lingering Love kiss (its
//! extension [`CharmExt`]). Given by Bugs Bunny's ground kiss (BugsBunny_Attack_BlowKiss's on-hit enemy buff, 3.5 s) and
//! LingeringLoveProjectile (1.6 s, the Lingering Love perk, perks.rs): no other character's data names the class, so it
//! lives here.
//! The class data the shared code takes as it is: BlockerGroups [CC Immune] (spawnBuff's sweep), DealtDamageMods
//! KnockBackMod 0.75 (the dealt pass), SpeedMod 0.75 (recalcSpeedMod), HasBuffDurationDecay with time 4 and levels
//! [3, 1, 0.5, 0.1] (spawnBuff's AddBuffDurationDecay / GetDecayedBuffDuration: the second Charm within the decay
//! lasts 3 s, then 1, 0.5, 0.1), IsDebuff, BuffDuration 5, Stun (StunDuration 0, SuspendPositionInAir true).
//!
//! ReceiveBeginPlay @466, after BuffParentActor's:
//! - the owner's Comp_Pawn_DamageManager.KnockedBack -> BreakCharm @661 and Comp_Pawn_HitBoxHandler.HitByAlly ->
//!   DestroyAndBreak @910: both K2_DestroyActor (@15 / @6457), no test of the hit;
//! - CharacterMovement.bCanWalkOffLedges = false @1159; BPI_DisableJump @1291, DisableEvade @1427, DisableAttack @1563;
//! - the owner a BP_Fighter_Character (@1685; else the rest pops): UI @1695 (Remove, then Add the Charmed state:
//!   StateType 1, Display 0, values -1, MI_State_Charm, Active, PopText "Charmed", @5630-6397);
//!   PS_BugsBunny_CharmedBuff on the mesh's fml_un_C_head @1709 with its Team parameter @2051 (drawn by the render,
//!   from the buff);
//! - authority, the DamageManager valid and the Instigator a fighter: SetMostRecentAttacker(Instigator, false) @2427;
//! - Delay(0) @2475 -> CheckIsInAir @30.
//! CheckIsInAir @30: not IsMovingOnGround -> IsInAir = true @186, MulticastRemoveHitstun @197 (-> @6462:
//! HitstunHandler RemoveHitstunFromKnockBack @6647, SetMovementMode(5 Flying) @6864, StopMovementImmediately @6975),
//! Stun.StunDuration = BuffDuration @211, Comp_State_Manager.UpdatePlayerStatus(null) @428: the stun pick takes this
//! actor's stun (unless a longer one is on), and with SuspendPositionInAir it holds him in the air for the rest of the
//! buff (the buff's end ends the stun: its EndPlay's pick).
//! Nothing in the class sets the mode back (ReceiveDestroyed writes none). The stun that suspends him ends Flying with
//! the suspension (HitstunHandler @908-1032 SetMovementMode(3 Falling), only while CurrentlySuspended). Under a longer
//! stun that does not suspend he stays Flying (MaxFlySpeed 0: still in the air) until another mode write: that stun's
//! buff ending first makes this actor's stun the pick (the suspension, then Falling at its end); a knockback
//! (ApplyKnockback, after BreakCharm), a move command's end or the respawn's reset. When this buff ends first, the
//! longer stun ends with no suspension and nothing writes a mode: he hovers in place after every stun has ended,
//! until a launch with some Z (a jump, a knockback; engine facts: only HandlePendingLaunch leaves Flying natively)
//! frees him.
//! ReceiveTick @2569: IsInAir -> nothing; else CheckIsInAir @2603 and, still on the ground, BPI_SetInputOverride(true,
//! X, true, 0) @3684 with X = -1 when the Instigator's world Y > own Y + CharmGoalDistance (250), +1 when it is < own Y
//! - 250, else 0 (@2945-3665). World Y is the sim's -x and MoveX +1 is toward -Y (+x): the stick toward the
//! Instigator beyond 250, the vertical held at 0. SetActorTickEnabled(authority or locally controlled) @2831: true
//! here.
//! ReceiveDestroyed @3826 (after the parent's ReceiveEndPlay: UWorld::DestroyActor's order):
//! bCanWalkOffLedges = true @4026; the owner a pawn (@4144); with a PFGPlayerController SetInputOverride(false,
//! MoveXInput1 + AxisMoveX2, false, MoveYInput2 + AxisMoveY2) @4628, any other controller (false, 0, false, 0)
//! @5189; EnableJump @4784, EnableEvade @4920, EnableAttack @5052; then (push 5243) the emitter destroyed @5286 and
//! RemoveStateForPlayer Charmed @5575.
// Deviation: the port has one controller kind, so every owner gets the live stick back (the frame's input), a bot's
// too; the build's AI controller restore writes 0 (input-override's Deviation on a bot's MoveX / MoveY calls then
// holds).
// Unverified: the buff's tick is the port's tickBuffs pass (BuffHooks::tick); the Delay(0)'s CheckIsInAir after it in
// the same frame finds what the tick found, so it is not run on its own.
// BuffParentActor's EndPlay (UpdateCurrentStates: the stun's end) is made here ahead of the restore, the order of the
// build's EndPlay and ReceiveDestroyed (tickBuffs then skips its own pick).

#![allow(clippy::doc_lazy_continuation, reason = "comment blocks kept as written")]

use crate::Real;
use crate::State;
use crate::combat::buffs::{buff, buff_mut, buff_of, inst_def_of, set_buff_values, update_states};
use crate::combat::hit::{remove_hitstun_from_knock_back, set_most_recent_attacker};
use crate::combat::holds::apply_stun_buffs;
use crate::combat::{BuffClass, BuffExt, BuffRef};
use crate::events::{UiStateOp, UiStateParams};
use crate::fighter::FighterId;
use crate::fighter::grounded;
use crate::fighter::moves::{
    disable_attack, disable_evade, disable_jump, enable_attack, enable_evade, enable_jump, set_can_walk_off_ledges,
    set_flying_mode, set_input_override,
};
use crate::name;
use crate::names::Name;
use crate::registry::entries::BuffHooks;
use crate::registry::props::{num, obj};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{BuffEntry, Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

/// `CHARM`.
pub const CHARM: Name = name!("Buff_Charm_C");
/// The state's id in the UI.
const CHARMED: Name = name!("Charmed");

/// Buff_Charm_C's extension (`CharmExt`): `by`, the actor's Instigator; `dur`, BuffDuration (the decayed duration of
/// the spawn); `air`, IsInAir.
#[derive(State, Debug, Default)]
pub struct CharmExt {
    pub by: Option<FighterId>,
    pub dur: Real,
    pub air: bool,
}

fn ext(w: &World, b: BuffRef) -> Option<&CharmExt> {
    match &buff(w, b)?.ext {
        BuffExt::Charm(e) => Some(e),
        _ => None,
    }
}

/// `charmed`: the Charmed status, for the HUD and the render (the head emitter lives while this is true).
pub fn charmed(w: &World, f: FighterId) -> bool {
    buff_of(w, f, CHARM).is_some()
}

/// CheckIsInAir @30. True: in the air (now or already).
fn check_is_in_air(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(dur) = ext(w, b).map(|e| e.dur) else { return false };
    if grounded(w, f) {
        return false;
    }
    if let Some(BuffExt::Charm(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
        e.air = true;
    }
    // MulticastRemoveHitstun @6462: RemoveHitstunFromKnockBack @6647, SetMovementMode(5 Flying) @6864,
    // StopMovementImmediately @6975
    remove_hitstun_from_knock_back(w, f);
    set_flying_mode(w, f, true);
    let fi = w.fighter_mut(f);
    fi.body.vx = 0.0;
    fi.body.vy = 0.0;
    fi.platforms.base = None;
    // @211 Stun.StunDuration = BuffDuration, @428 UpdatePlayerStatus(null)
    let mut stun = inst_def_of(w, b).and_then(|d| obj(&d, "Stun").cloned()).unwrap_or_default();
    stun.insert("StunDuration".to_owned(), Value::Num(f64::from(dur)));
    set_buff_values(w, b, &Props::from_iter([("Stun".to_owned(), Value::Obj(stun))]));
    update_states(w, f);
    true
}

fn ui(w: &mut World, f: FighterId) {
    let ops = &mut w.fighter_mut(f).controls.ui_state_ops;
    ops.push(UiStateOp::Remove(CHARMED));
    ops.push(UiStateOp::Add(UiStateParams {
        id: CHARMED,
        state_type: 1,
        display_type: 0,
        current_value: -1.0,
        max_value: -1.0,
        timer: -1.0,
        icon_material: name!("MI_State_Charm"),
        active: true,
        pop_text: name!("Charmed"),
        auto_remove_time: -1.0,
        subicon: None,
    }));
}

fn begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, duration: Option<Real>) {
    let d = inst_def_of(w, b);
    let dur = duration.or_else(|| d.as_ref().and_then(|d| num(d, "BuffDuration"))).unwrap_or(0.0);
    drop(d);
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::Charm(CharmExt { by: from, dur, air: false });
        x.ally_hit = true; // HitByAlly bound @910
        if from.is_some() {
            x.stun_by = from; // ApplyStunBuffs' SetMostRecentAttacker reads the actor's Instigator
        }
    }
    set_can_walk_off_ledges(w, f, false);
    disable_jump(w, f);
    disable_evade(w, f);
    disable_attack(w, f);
    ui(w, f);
    if let Some(from) = from {
        set_most_recent_attacker(w, f, from);
    }
}

fn tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some((by, air)) = ext(w, b).map(|e| (e.by, e.air)) else { return false };
    if air || check_is_in_air(w, f, b) {
        return false;
    }
    // GetInstigator's K2_GetActorLocation; a None Instigator reads the zero vector.
    let iy = (-by.map_or(0.0, |o| w.fighter(o).body.x)) + 0.0;
    let oy = (-w.fighter(f).body.x) + 0.0;
    let goal = inst_def_of(w, b).and_then(|d| num(&d, "CharmGoalDistance")).unwrap_or(250.0);
    let x = if iy > (oy + goal) {
        -1.0
    } else if iy < (oy - goal) {
        1.0
    } else {
        0.0
    };
    set_input_override(w, f, true, x, true, 0.0);
    false
}

fn on(_w: &mut World, _f: FighterId, _b: BuffRef, sig: BuffSignal, _a: &BuffSignalArg) -> bool {
    matches!(sig, BuffSignal::KnockedBack | BuffSignal::HitByAlly)
}

fn end(w: &mut World, f: FighterId, b: BuffRef) {
    // BuffParentActor ReceiveEndPlay's UpdateCurrentStates, ahead of ReceiveDestroyed.
    let stunned =
        inst_def_of(w, b).and_then(|d| obj(&d, "Stun").and_then(|s| num(s, "StunDuration"))).unwrap_or(0.0) > 0.0;
    apply_stun_buffs(w, f, None, stunned);
    set_can_walk_off_ledges(w, f, true);
    let fi = w.fighter(f);
    let (rx, ry) = match &fi.moves.input_ov {
        Some(s) => (s.rx, s.ry),
        None => (fi.controls.inp.x, fi.controls.inp.y),
    };
    set_input_override(w, f, false, rx, false, ry);
    enable_jump(w, f);
    enable_evade(w, f);
    enable_attack(w, f);
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(CHARMED));
}

/// This file's registry entries (Buff_Charm_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks =
        BuffHooks { begin: Some(begin), tick: Some(tick), on: Some(on), end: Some(end), ..BuffHooks::default() };
    r.buffs.enter("Buff_Charm_C", BuffEntry::new(BuffClass::BuffCharm, hooks))
}
