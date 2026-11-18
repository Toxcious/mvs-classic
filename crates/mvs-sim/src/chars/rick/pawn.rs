//! Pawn_C020's own logic and the parts of C020_AnimBPV2 the sim reads: the
//! megaseed's hit listener, the charge and aim the slot graph reads, ClearUnactivatedProjectiles; mod.rs's hooks.
//!
//! BeginPlay (ReceiveBeginPlay -> @1109): the parent's; SkinApplied -> OnSkinApplied @1129 (caches the mesh's
//! C020_AnimBPV2 @2347-2464: the port's state is that instance's, `RickState`); on authority Comp_Pawn_ChargeAttack's
//! ChargeEnd -> ChargeEnd @866-919 (`CharHooks::charge_end`); the hit box handler's HitAlly and HitEnemyPlayer -> OnHit
//! @1253-1340, with no authority test (`hit_ally`, `hit_enemy`); Delay(1.0) @1198 -> @35, the PlasmaBlades' team
//! material (the view's). The pawn has no ReceiveTick and no BPI_Cleanup / reset of its own; a new pawn (a match reset)
//! is a new `RickState` (mod.rs `match_reset`).
//! Not the sim's: the construction script's SetMasterPoseComponent of the sub meshes (the view's); HeadScale @1382
//! (BaseFighter's SetHeadScale -> the AnimBP's HeadScale, the ModifyBone of the head: the fighter's head scale, drawn by
//! the view); BPI_ReadyForAI @1549 (his six bot subtrees); BPI_Toggle2DHitboxes / Use2DHitboxes (the data's flag as for
//! every pawn); BPI_GetFighterCharacter 7 (EFighter C020); BPI_SetSwinging (empty), BPI_CheckSwinging false,
//! BPI_HasSpawnedPortalReady false (read by no live graph: Condition_C020PortalReadyForPartner reads the portal
//! component, aimer.rs); BPI_GetSafePortalLocation (teleport.rs).

use super::state::{rick, rick_mut, rick_of, rick_w};
use crate::Real;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::anim::SlotMix;
use crate::fighter::anim::montages::resolve;
use crate::fighter::control::{charge_total, notify_def, trigger_ability};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::CharHooks;
use crate::world::World;

/// OnHitAbilityClient @2296: TriggerAbilityActual(2).
const MEGASEED: i32 = 2;

// ---------------------------------------------------------------- the megaseed's hit listener

/// BPI_ListenForAbilityHit(On) @2177: ListeningForAbilityHit = On (AnimNotify_C020ListenForAbilityHit's begin true, end
/// false; mod.rs).
pub fn listen_for_ability_hit(w: &mut World, f: FighterId, on: bool) {
    rick_w(w, f).listening = on;
}

/// OnHit (a server RPC; bound to his own handler's HitAlly and HitEnemyPlayer) @2197: while ListeningForAbilityHit,
/// OnHitAbilityClient (a client RPC) @2207 -> the ability manager's TriggerAbilityActual(2) @2296. No team, count or
/// victim test and no clear: each hit of a listening Hitframe on an enemy or an ally spends the slot again (the second
/// trigger of one swing finds the cooldown set: the manager's own rules). The RPCs run at once: one simulation holds the
/// server and the owning client.
fn on_hit(w: &mut World, f: FighterId, _victim: FighterId) {
    if rick(w, f).listening {
        trigger_ability(w, f, MEGASEED);
    }
}

// ---------------------------------------------------------------- the AnimBP's inputs

/// ChargeEnd @2222 (Comp_Pawn_ChargeAttack's ChargeEnd: EndChargeAttackServer in AnimNotify_ChargingAttack's NotifyEnd):
/// GetUpdatedPercentCharged @2222 = NewMultiplier(2.0) - 1.0, NewMultiplier = (2 - 1) x FMin((now - ChargeStartTime) /
/// ChargeTotalTime, 1) + 1 (Comp_Pawn_ChargeAttack NewMultiplier @0-418, float32), then GetChargeMulticast @2267 ->
/// @1020: a valid cached C020_AnimBPV2 gets ChargeAmount = it @1059. The elapsed time is the component's clock at the
/// end, not at the release (the release's section jump ends the notify in the same update: the same time).
/// ChargeTotalTime is the component's (StartAttackChargeMultipliersServer: TotalDuration x ChargeMultiplier of the last
/// begun charge, the fighter's `charge_total_time` once the multiplier has been written). Before that, the last begun
/// charge's TotalDuration: the fighter's tracked notify, else the ending one (a montage start or the blend-out cleared
/// the port's key; the component keeps the last charge's time).
// Unverified: with the key cleared, no multiplier written and the ending notify not the last begun charge (a late end
// of an older charge), its duration stands for the component's ChargeTotalTime.
fn charge_end(w: &mut World, f: FighterId, n: NotifyRef) {
    let c = &w.fighter(f).charge;
    let tracked = if c.charge_total_time.is_none() && c.charge_notify >= 0 {
        resolve(w, f, crate::real(c.charge_notify)).map(|r| r.n)
    } else {
        None
    };
    let data = w.data.clone();
    let dur = notify_def(&data, w, f, tracked.unwrap_or(n)).map_or(0.0, |nd| nd.dur);
    let total = charge_total(w, f, crate::math::narrow(dur));
    let elapsed = w.fighter(f).clock.time - w.fighter(f).charge.charge_start_time;
    let m = min(elapsed / total, 1.0) + 1.0;
    rick_w(w, f).charge_amount = m - 1.0;
}

/// C020_AnimBPV2 OnMontageStarted_Event_0 @3788 (bound @1600 to the instance's OnMontageStarted): ChargeAmount = 0.
fn montage_started(w: &mut World, f: FighterId, _montage: Name) {
    rick_w(w, f).charge_amount = 0.0;
}

/// C020_AnimBPV2 BPI_SetAimDirection(AimDirection) @3600-3647: Aim Direction = FClamp(AimDirection, 0, 1). Its caller in
/// his kit is C020PortalAimer (aimer.rs). BPI_GetAimDirection returns 0 whatever the variable holds (@543): no reader.
pub fn set_aim_direction(w: &mut World, f: FighterId, v: Real) {
    if let Some(s) = rick_mut(w, f) {
        s.aim_direction = min(max(v, 0.0), 1.0);
    }
}

/// `standaloneSkin`: the skin a fighter's own pawn applies, for the AnimBP reads the sim makes
/// of it. A dedicated server applies `skin_<char>_server`; a game with no dedicated server applies the player's pick.
/// The port's matches the build plays off a dedicated server: the Lab (a set-up without `rules`) and Local Play. A queue
/// or custom-lobby match is the build's dedicated server's: the default skin there. A minion (no player of the set-up)
/// and a default pick: `None`.
// Unverified: the Lab taken as standalone (no dedicated server) from the TrainGameMode flow, not read native.
fn standalone_skin(w: &World, f: FighterId) -> Option<&str> {
    let s = &w.setup;
    if s.rules.is_some() && !s.local_play {
        return None;
    }
    let id = usize::try_from(w.fighter(f).id).ok()?;
    if id >= w.players.len() {
        return None;
    }
    s.players.get(id)?.skin.as_deref().filter(|k| !k.is_empty())
}

/// The slot graph's inputs (`CharHooks::slot_input`; `fighter::anim::slot_mix::graph_slots` walks C020_AnimBPV2's graph
/// from the data):
/// - IsSealTeamRick: AppliedSkin == C020_S02, written at the instance's initialization @439-499: the player's pick in a
///   match off a dedicated server (the Lab, Local Play), where the SealTeamSkin track of the two blaster charge shots is
///   then the pose his hurt shapes ride; false in a dedicated server's match (skin_c020_server) and for every other skin.
/// - IsPickleRick: no graph writes it (xref writes: none) and the class defaults leave it false.
/// - ChargeAmount (TwoWayBlend: ShortWhip / LongWhip), Aim Direction (TwoWayBlend_1: AimForward / AimUp).
/// - AnimGraphNode_TwoWayBlend_2 (DefaultSlot / the aim pair): its exposed input's evaluation @3676 -> @1659-1827,
///   FClamp(|Aim Direction - 0.5| x 2, 0, 1), from the variable at that pose update.
fn slot_input(w: &World, f: FighterId, n: Name) -> Real {
    let s = rick(w, f);
    if n == name!("IsSealTeamRick") {
        crate::real(standalone_skin(w, f) == Some("C020_S02"))
    } else if n == name!("IsPickleRick") {
        0.0
    } else if n == name!("ChargeAmount") {
        s.charge_amount
    } else if n == name!("Aim Direction") {
        s.aim_direction
    } else if n == name!("AnimGraphNode_TwoWayBlend_2") {
        min(max((s.aim_direction - 0.5).abs() * 2.0, 0.0), 1.0)
    } else {
        Real::NAN
    }
}

/// `slotMix: (f) => rick(f)`.
fn slot_mix(w: &World, f: FighterId) -> Option<&SlotMix> {
    rick_of(w, f).map(|s| &s.slot_mix)
}

/// `slotMix`, written.
fn slot_mix_mut(w: &mut World, f: FighterId) -> Option<&mut SlotMix> {
    rick_mut(w, f).map(|s| &mut s.slot_mix)
}

/// `death`: the port does not update a dead pawn's AnimBP.
fn death(w: &mut World, f: FighterId) {
    rick_w(w, f).slot_mix.slot_mix.clear();
}

// ---------------------------------------------------------------- ClearUnactivatedProjectiles

/// BPI_ClearUnactivatedProjectiles @962 (AnimNotify_ClearUnactivatedMeeseeks' NotifyEnd), on authority: over the owner's
/// Comp_Spawner.SpawnedActors @378-865, each actor cast to Meeseeks_Base whose Activated is false gets ReleaseToPool
/// @829. The ground special's Meeseeks_SpawnedActor is Meeseeks_Base's sibling: the cast fails. The notify sits on
/// C020_Attack_HeavyD_MeeseeksStomp, HeavyFMeeseeksRun and HeavyU_MeeseeksUpperCut, montages of branch groups the pawn's
/// attack graph does not use, whose Meeseeks_Base children (Meeseeks_Run_C, Meeseeks_Charge*) are not ported.
// Deviation: the one Meeseeks_Base child the port has, Bugs's box runner SelfActivate_Meeseeks_Run
// (`items::meeseeks`), can be Rick's for the .25 s ahead of its self activation; its release here is not ported (the
// notify's montages are reached by a sweep only).
pub fn clear_unactivated_projectiles(_w: &mut World, _f: FighterId) {}

/// The hooks of this file in Rick's CharHooks (mod.rs's `slotInput`, `slotMix`, `montageStarted`, `chargeEnd`,
/// `hitEnemy`, `hitAlly`, `death`).
pub fn hooks(h: &mut CharHooks) {
    h.slot_input = Some(slot_input);
    h.slot_mix = Some(slot_mix);
    h.slot_mix_mut = Some(slot_mix_mut);
    h.montage_started = Some(montage_started);
    h.charge_end = Some(charge_end);
    h.hit_enemy = Some(on_hit);
    h.hit_ally = Some(on_hit);
    h.death = Some(death);
}
