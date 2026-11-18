//! The pawn core of Pawn_Finn (offsets of its ubergraph): BeginPlay's caches @1941-2051
//! (anim instance, OriginalGravityScale @2002, OriginalWalkSpeed @2051), the Delay 0.1 retry (@544 -> 599) and Delay 0.25
//! (@2215 -> 15), BPI_Cleanup @7786-7833 (isGliding false, the momentum's OnFighterReset @7797, DestroyAllSpawnedActors
//! @7833), HandleBarSwitch / UpdateRunUI @7465-7508, TurnOnRunUi @8829 / TurnOffRunUI @8845, BPI_Use2DHitboxes @8861;
//! Finn_AnimBP's sim slot mix (IsFalling @1673, Moving @1840; `FinnPawnState::slot_mix`); Combo_01's Buff_SpeedUp; the
//! shoulder bash (below, no code).
//! Classes: Pawn_Finn (all but what his other files name), BPI_Finn, Finn_AnimBP (its sim part).
//! Hooks: [`hooks`] (the pawn hooks), entered by mod.rs as they are: matchReset runs behind mod.rs's new state.
//!
//! Not here: the AI (AssignDefaultBehaviors @7870-8713, bots.rs), HeadScale @7619-7736 (the view's), the debug bind
//! @2270-2339 / DebugSetTreasure / OnDebugCommand (dev only), LostTreasure (0 callers).
//!
//! What needs no code:
//! - BeginPlay @1941-2051 caches the anim instance (HulkAnimBP), OriginalGravityScale @2002 and OriginalWalkSpeed
//!   @2051: no function of Pawn_Finn or of any class reads the three (xref reads), so nothing is kept.
//! - BeginPlay @15-543 (a locally controlled PlayerController's pawn only) makes UI_FinnCharge and binds the momentum
//!   component's NewMomentumStarted / MomentumEnd to TurnOnRunUi / TurnOffRunUI, retried by a Delay 0.1 (@544 -> 599)
//!   until the controller is there, and the Delay 0.25 (@2215 -> 15) starts it: the widget is the HUD's; the broadcasts
//!   are momentum.rs's run_ui writes (its Deviation: every Finn). HandleBarSwitch / UpdateRunUI @7465-7508 is the
//!   widget's, on `FinnState::run_ui`.
//! - BPI_Use2DHitboxes @8861 -> BPI_Toggle2DHitboxes: a debug toggle; the 2D shapes are the character data's.
//! - Combo_01's ApplyBuff Buff_SpeedUp_C (BuffSelf, t 0.101): a shared buff, its def in the character data's buffs.
//! - The shoulder bash: Pawn_Finn implements BPI_Finn alone, so WW_ShieldActor's BPI_BlockedProjectile (a
//!   BPI_WonderWoman call) does nothing on him: no blocked_projectile hook. The two ConditionSectionJumpSingleCheck
//!   notifies are each the shared handler's (which gives their OR); ActivateSpawned names Finn_ShoulderBashActor_C,
//!   which no spawned actor of his is: nothing. WW_ShieldActor_C stays Wonder Woman's entry.
//! - BPI_Cleanup's isGliding = false @7786: Pawn_Finn's own variable, read nowhere (Finn has no glide).
//!
//! The world: every hook here takes it. A body that has to act on the world (a spawn, a trace, a hit) takes it where
//! its caller has it.

use super::coins;
use super::momentum;
use super::state::{finn, finn_w};
use crate::Real;
use crate::actors::{ActorRef, world_actor};
use crate::combat::HitPacket;
use crate::fighter::FighterId;
use crate::fighter::anim::SlotMix;
use crate::fighter::anim::slot_blend::slot_blend_reset;
use crate::fighter::moves::is_falling;
use crate::name;
use crate::names::Name;
use crate::registry::CharHooks;
use crate::world::World;

/// pawn.rs's part of the joined matchReset (after mod.rs's new state): a new pawn's anim instance, the lists at
/// INDEX_NONE.
pub fn match_reset(w: &mut World, f: FighterId) {
    if let Some(b) = finn_w(w, f).pawn.slot_mix.slot_blend.as_mut() {
        slot_blend_reset(b);
    }
}

/// pawn.rs's part of the joined hitAnything (before high_five.rs's): Comp_Pawn_HitBoxHandler HitAnything of his own
/// handler: ReceiveBeginPlay binds it to SpawnTreasure on authority (@2100-2153), ahead of any FinnHighFiveListener's
/// bind (high_five.rs joins the hook behind this one: the build's bind order). The body is coins.rs's.
pub fn hit_anything(w: &mut World, f: FighterId, a: ActorRef, hit: &HitPacket, boxed: bool) {
    coins::spawn_treasure(w, f, a, hit, boxed);
}

/// Finn_AnimBP's inputs to its slot graph (`fighter::anim` graph_slots): BlueprintUpdateAnimation @5915 -> @1542 (behind
/// the pawn's validity): IsFalling = CharacterMovement.IsFalling() @1673, Moving = Velocity.Y != 0 @1840 (the sim's vx).
/// The graph: DefaultSlot <- BlendListByBool(IsFalling; BlendTime [1.0, 0.1]) <- true: Slot AirAttack, false:
/// BlendListByBool_1(Moving; [0.1, 0.1]) <- Slot GroundMovingSlot / GroundStillSlot. The lists' state is
/// `FinnPawnState::slot_mix.slot_blend` (a new anim instance at a new pawn: `match_reset`).
// Deviation: the port does not update a dead pawn's AnimBP (the build's mesh still ticks), so the lists come back from a
// respawn where the death left them, and step on from there.
fn slot_input(w: &World, f: FighterId, input: Name) -> Real {
    if input == name!("IsFalling") {
        if is_falling(w, f) { 1.0 } else { 0.0 }
    } else if input == name!("Moving") {
        if w.fighter(f).body.vx != 0.0 { 1.0 } else { 0.0 }
    } else {
        Real::NAN
    }
}

fn slot_mix(w: &World, f: FighterId) -> Option<&SlotMix> {
    Some(&finn(w, f).pawn.slot_mix)
}

fn slot_mix_mut(w: &mut World, f: FighterId) -> Option<&mut SlotMix> {
    Some(&mut finn_w(w, f).pawn.slot_mix)
}

/// `death`: the port does not update a dead pawn's AnimBP.
fn death(w: &mut World, f: FighterId) {
    finn_w(w, f).pawn.slot_mix.slot_mix.clear();
}

/// BPI_Cleanup @7786-7833 (BaseFighter ResetFighterLogic's BPI_Cleanup, inside the respawn's reset: `reset`): isGliding
/// = false (read nowhere), Comp_FinnMomentumCharge.OnFighterReset @7797, Comp_Spawner.DestroyAllSpawnedActors @7833
/// (Comp_Spawner @8593-8891 -> @3833-4004: K2_DestroyActor on every actor of SpawnedActors, the last first). Ahead of
/// the respawn's destroy-with-instigator (Comp_Spawner OnBaseFighterReset), as the build's BPI_Cleanup comes inside
/// ResetFighterLogic. A projectile or an item goes through `destroy_spawned` (its EndPlay paths); a world actor is
/// marked dead and its kind's end runs in the frame's list pass, as the destroy-with-instigator does. A reflected
/// projectile the owner's list no longer holds (`reflect.unlisted`) is left, as there.
fn cleanup(w: &mut World, f: FighterId) {
    momentum::on_fighter_reset(w, f);
    let mut i = w.fighter(f).items.spawned_actors.len();
    while i > 0 {
        i -= 1;
        // (the list can shorten while an end path runs: `get`)
        let Some(&a) = w.fighter(f).items.spawned_actors.get(i) else { continue };
        if world_actor::actor_dead(w, a) {
            continue;
        }
        match a {
            ActorRef::Projectile(id) => {
                if !w.actors.projectiles.get(id).is_some_and(|p| p.reflect.as_ref().is_some_and(|r| r.unlisted)) {
                    crate::world::spawning::destroy_spawned(w, a);
                }
            }
            ActorRef::Item(id) => {
                if !w.actors.items.get(id).is_some_and(|p| p.reflect.as_ref().is_some_and(|r| r.unlisted)) {
                    crate::world::spawning::destroy_spawned(w, a);
                }
            }
            _ => world_actor::mark_spawned_dead(w, a),
        }
    }
}

/// `resetHighFive` (BPI_Finn_ResetHighFive): Comp_FinnMomentumCharge ResetHighFive @2343 (HighFiveCharge = 0, then @15:
/// HighFivePercent = 0, UpdateHighFiveUI). Caller: AnimNotify_FinnHighFive_Hitframe's NotifyEnd (high_five.rs).
pub fn reset_high_five(w: &mut World, f: FighterId) {
    momentum::reset_high_five(w, f);
}

/// The hooks of this file in Finn's CharHooks (`slot_input`, `slot_mix`, `death`, `component_tick`, `reset`, with
/// hit_anything and match_reset composed in mod.rs).
pub fn hooks(h: &mut CharHooks) {
    h.slot_input = Some(slot_input);
    h.slot_mix = Some(slot_mix);
    h.slot_mix_mut = Some(slot_mix_mut);
    h.death = Some(death);
    h.component_tick = Some(momentum::momentum_tick); // Comp_FinnMomentumCharge's ReceiveTick
    h.reset = Some(cleanup);
}
