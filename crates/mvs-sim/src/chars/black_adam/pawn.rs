//! Pawn_C021's own logic: ReceiveTick's Hover Direction (tick), the
//! match reset's new state, the conditions C021BubblePerkCondition / C021NoBubblePerkCondition, the notifies
//! AnimNotify_C021StartGroundChecking (nothing), AnimNotify_HandleFly and AnimNotify_MoveIntoFly (Comp_Pawn_Fly's Set
//! Is Flying; MoveIntoFly is AnimNotify_MoveCommand's child), and the composition of the files' joined hooks (the
//! module's header): `reset` (chain.rs `clear_manager`), `match_reset` (the new state, then chain.rs
//! `clear_manager`, then grab_hit.rs `match_reset`), `hit_enemy` / `knocked_back` (grab_hit.rs), `anim_update`,
//! `pose_updated`, `death` (anim.rs), `slot_mix`.
//!
//! BeginPlay @38-604: the AnimBP reference @63-180, SupermanCapsuleHight / Radius @200-249 (not kept), the
//! OnSkinApplied binding off a dedicated server @299-423 (HeartSkin: the view's) and OnFighterReset ->
//! ResetLightningManager @424-466 (chain.rs, the `reset` hook). No sim write of its own. BPI_ReadyForAI @660-1287:
//! the bots' (their injections come with the bots' crate). BPI_GetFighterCharacter returns EFighter C021 (25);
//! BPI_Use2DHitboxes returns the CharDef's use2D.

use super::state::{BlackAdamState, black_adam_w};
use super::{anim, chain, grab_hit};
use crate::Real;
use crate::actors::ActorRef;
use crate::chars::CharState;
use crate::combat::buffs::buff_of;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::anim::montages::notify_data;
use crate::fighter::moves::set_is_flying;
use crate::name;
use crate::registry::notifies::{move_command_begin, move_command_end};
use crate::registry::props::flag_or;
use crate::registry::{CharHooks, Condition, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use mvs_data::fighter::NotifyTick;

/// The hooks of this file in Black Adam's CharHooks (`mod.rs` own: slotMix, matchReset, tick; the joined ones,
/// composed here).
pub fn hooks(h: &mut CharHooks) {
    h.slot_mix = Some(anim::slot_mix);
    h.slot_mix_mut = Some(anim::slot_mix_mut);
    h.match_reset = Some(match_reset);
    h.tick = Some(tick);
    h.reset = Some(chain::clear_manager);
    h.hit_enemy = Some(grab_hit::hit_enemy);
    h.knocked_back = Some(knocked_back);
    h.anim_update = Some(anim::anim_update);
    h.layer_held = Some(anim::layer_held);
    h.pose_updated = Some(anim::slot_compose);
    h.death = Some(anim::death);
}

/// ReceiveTick @1827-2309 (the parent's first: CharHooks.tick). GetAnimInstance cast to C021_AnimBP @1846-1953,
/// BPI_IsFacingLeft @1963, BreakVector(Comp_Pawn_Fly.Last Direction) @1986, SelectFloat(Y, -Y, FacingLeft) @2054-2096,
/// MakeVector2D(that, Z) @2151, Hover Direction = it @2197. In the port's frame Last Direction is (last_x, last_y) =
/// (-UE Y, UE Z), so X = FacingLeft ? -last_x : last_x = last_x x facing, Y = last_y. Before his first flight (no
/// fly state) Last Direction is the template's (0, 0, 0). Then HeartSkinOpacityMax == 0 @2246 -> return @2294: the
/// port's skin has no heart material.
fn tick(w: &mut World, f: FighterId) {
    let facing = w.fighter(f).body.facing_dir;
    let (x, y) = match &w.fighter(f).moves.fly {
        Some(fl) => (fl.last_x * facing, fl.last_y),
        None => (0.0, 0.0),
    };
    let s = black_adam_w(w, f);
    s.hover_x = x;
    s.hover_y = y;
}

/// A match reset spawns a new pawn: a new state (its BeginPlay writes nothing the sim keeps). The files' joined
/// matchReset run after this one and find the new state: chain.rs (the manager cleared), then grab_hit.rs.
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::BlackAdam(BlackAdamState::default());
    chain::clear_manager(w, f);
    grab_hit::match_reset(w, f);
}

/// The joined `knockedBack`: each live grab hit of his is destroyed.
fn knocked_back(w: &mut World, f: FighterId, _vx: Real, _vy: Real, _a: Real, _by: Option<ActorRef>) {
    grab_hit::knocked_back(w, f);
}

/// C021BubblePerkCondition Check @79-226: the actor's BPI_CheckIfHasBuff(Perks/Buffs/C021/Buff_C021_Bubble_C)'s
/// HasBuff (Comp_State_Manager CheckHasBuff: a buff actor of that class); no fighter interface: false @250. The buff
/// is the Bubble perk's marker (perks.rs; nothing else gives it): with it the down special's spawner takes
/// C021_ForceField_Perk_C, without it C021_ForceField_C (NoBubble).
// Unverified: IsBuffOfClass is not read (as HasEducatedBuff, `combat/conditions`): the exact class is tested (the
// perk's buff is that class; nothing gives a child of it).
fn bubble_perk(w: &mut World, f: FighterId) -> bool {
    buff_of(w, f, name!("Buff_C021_Bubble_C")).is_some()
}

/// C021NoBubblePerkCondition @79-255: Not of the same HasBuff; no fighter interface: Not(false) = true @279-324.
fn no_bubble_perk(w: &mut World, f: FighterId) -> bool {
    buff_of(w, f, name!("Buff_C021_Bubble_C")).is_none()
}

/// A queued notify state's NotifyEnd from a montage stopped after this frame's dispatch waits for the next dispatch
/// `pending_ends`, run again from there with the deferral off.
fn late(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) -> bool {
    if !w.fighter(f).branching.defer_queued_ends {
        return false;
    }
    let data = w.data.clone();
    if notify_data(w, &data, f, n).tick == NotifyTick::BranchingPoint {
        return false;
    }
    w.fighter_mut(f).branching.pending_ends.push((n, idx));
    true
}

/// AnimNotify_HandleFly Received_NotifyBegin: with StartFlyOnNotifyStart (class default true) @0, the owner's
/// Comp_Pawn_Fly (GetComponentByClass @56, IsValid @107) Set Is Flying(true) @243. Both flight montages have it with
/// StartFlyOnNotifyStart false: the flight starts at MoveIntoFly's end.
fn handle_fly_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    if flag_or(&notify_data(w, &data, f, n).p, "StartFlyOnNotifyStart", true) {
        set_is_flying(w, f, true);
    }
}

/// Received_NotifyEnd: with EndFlyOnNotifyEnd (default true) @0, Set Is Flying(false) @243. Also at a stop (a hit, a
/// ring-out's defeat tail: the stopped instance's queued states end at the next dispatch, in the order they began,
/// so MoveIntoFly's (Set Is Flying(true)) comes before this one's: he never stays flying past a stopped flight).
fn handle_fly_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if late(w, f, n, idx) {
        return;
    }
    let data = w.data.clone();
    if flag_or(&notify_data(w, &data, f, n).p, "EndFlyOnNotifyEnd", true) {
        set_is_flying(w, f, false);
    }
}

/// AnimNotify_MoveIntoFly is an AnimNotify_MoveCommand child: its Begin is the parent's (the class has no
/// Received_NotifyBegin of its own); its Received_NotifyEnd calls the parent's @0 (BPI_CompleteMoveCommand), then the
/// owner's Comp_Pawn_Fly (IsValid @131) Set Is Flying(true) @267. The command manager's end logic of the next
/// manager tick then writes Falling and MaxAcceleration 4096 after Update Is Flying's 9000, and the Fly tick sets
/// Flying again. The late end runs both halves at the next dispatch.
fn move_into_fly_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    if late(w, f, n, idx) {
        return;
    }
    move_command_end(w, f, n, idx);
    set_is_flying(w, f, true);
}

/// This file's registry entries (the two conditions, the three notifies). AnimNotify_C021StartGroundChecking is a
/// notify state with Received_NotifyBegin alone: the owner implements BPI_C021 @42, then a cast to BPI_Garnet @136
/// fails (Pawn_C021's Interfaces list BPI_C021 alone), so BPI_SetGroundPositions @215 is not called: it does nothing
/// (C021_Attack_Normal_Down_Montage 0-0.227 s).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.conditions.enter("C021BubblePerkCondition_C", Condition::Fn(bubble_perk))?;
    r.conditions.enter("C021NoBubblePerkCondition_C", Condition::Fn(no_bubble_perk))?;
    r.notifies.enter("AnimNotify_C021StartGroundChecking_C", NotifyHandler::default())?;
    r.notifies.enter(
        "AnimNotify_HandleFly_C",
        NotifyHandler { begin: Some(handle_fly_begin), end: Some(handle_fly_end), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_MoveIntoFly_C",
        NotifyHandler { begin: Some(move_command_begin), end: Some(move_into_fly_end), ..NotifyHandler::default() },
    )
}
