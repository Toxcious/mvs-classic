//! Pawn_StevenUniverse's own logic: BeginPlay, the glide input, the up shield
//! ammo, BPI_Steven, the conditions (NoWatermelonSpawned, StevenHasShieldAmmoCondition), the spawner filter and the
//! pawn hooks (tick, endJump, landedOnSolidGround, spawnerFiltered, matchReset and reset, which mod.rs and the joined
//! files share).
//!
//! The pawn core of Pawn_StevenUniverse (offsets of its ubergraph): lifecycle, glide input and values (ReadGlideInput,
//! AssignGlideValues, SetGlideValueMulticast, the terminal velocity select @4284-4335, Is Valid Slowfall Gravity;
//! compare `chars/batman/pawn.rs`), RestoreShieldAmmo and the shield ammo, and the conditions NoWatermelonSpawned and
//! StevenHasShieldAmmoCondition.
//!
//! The pawn's parts in other files: watermelon.rs (SpawnWatermelonSteven, the minion's set-up, EarlyDestroy, the
//! defeat binding), shield.rs (BPI_DecrementShieldCharge, Comp_StevenShield.OnFighterReset), glide.rs and the view
//! (Steven_AnimBP.Gliding).
//!
//! Not ported, as nothing in the sim acts on it: Tick's IsDedicatedServer test (@6228: not one), StartJump @4218 (the
//! parent's alone), SpeedVisuals @4361 (empty), SetOutline @6264 (the view's), HeadScale @8477 (the view's), the
//! construction script's master poses, BPI_Cleanup's UpperTeeth visibility @8701 (the view's), FindMirrorSocket (no
//! caller in his graphs; the view's), BPI_ReadyForAI @9309 (his bot subtrees), BPI_GetFighterCharacter 14
//! (`combat::disguise`), BPI_Use2DHitboxes / BPI_Toggle2DHitboxes @10384 (the data's "Use 2D Hitboxes" true; the one
//! writer is Comp_Pawn_HitBoxHandler's debug path @219, behind IsLiveBuild false @61). BFTaunt @8736 is BaseFighter's
//! own plus, with a valid stored Watermelon, its AIController cast to Base_AIController_C @9121 and the blackboard's
//! QueueTaunt: the Watermelon's controller is a stock AAIController, so the cast fails and the override is the
//! parent's.
//! The abilities are the data's Comp_Pawn_AbilityManager rows (slot 0 the barrier 14 s, 1 the tether 16 s, 2 the heal
//! zone (-1: on until BeginAlternateCooldownOnAuthority(2, 0) gives AlternateCooldowns[0] 20 s, from HealGrenade's End
//! or the Watermelon's defeat, watermelon.rs), 3 unused 18 s, 4 the bubble shield (MaxCount 3, StartCount 3,
//! CountCooldown 5, spent 1, refreshed 1; Comp_StevenShield's SetAmmoCooldownOnAuthority(4, 8.5, 8.5) over it is
//! shield.rs's)). Each slot is started by its actor's TriggerAbilityOnSpawnIndex. No code of his.

use super::state::{steven_of, steven_w};
use crate::Real;
use crate::actors::ActorRef;
use crate::actors::world_actor::{actor_cls, actor_dead, actor_entry};
use crate::consts::AIR_CONTROL;
use crate::fighter::anim::montages::instance_data;
use crate::fighter::anim::{nav_of, stop_dash_montage};
use crate::fighter::common::{data_of, default_terminal};
use crate::fighter::moves::{is_falling, jump_velocity, set_terminal, update_air_control};
use crate::fighter::{FighterId, in_hitstun};
use crate::name;
use crate::names::Name;
use crate::registry::props::num;
use crate::registry::{CharHooks, Condition, Registry, RegistryError};
use crate::world::World;

/// NetGravity(1.15) @1553; Is Valid Slowfall Gravity's NearlyEqual @68.
const GLIDE_GRAVITY: Real = 1.15;
/// SetTerminalVelocity(false, 1000, false) @1590, the select @4284.
const GLIDE_TERMINAL: Real = 1000.0;
const HEAL_GRENADE: Name = name!("HealGrenade_C");
const HEAL_ZONE: Name = name!("HealOverTimeZoneBP_C");

/// The hooks of this file in Steven's CharHooks (`pawnHooks`): matchReset and reset are composed in mod.rs.
pub fn hooks(h: &mut CharHooks) {
    // ReceiveTick @6209: ReadGlideInput.
    h.tick = Some(tick);
    // EndJump @4362: the parent's with FastFallMult false.
    h.end_jump = Some(end_jump);
    // RestoreShieldAmmo, bound to OnLandedOnSolidGround at BeginPlay @2254.
    h.landed_on_solid_ground = Some(restore_shield_ammo);
    // Comp_Spawner.SpawnedActors_ClassChecked (BPI_HasWatermelonSpawned's).
    h.spawner_filtered = Some(spawner_filtered);
}

// ---- BeginPlay

/// ReceiveBeginPlay @6189 (a new pawn: the World's reset, the match's first spawn included, and a Watermelon minion's
/// spawn, inside SpawnAIFromClass before its set-up): the parent's, then @2061 the pushed flows run last first:
/// @2071-2230 HulkAnimBP, OriginalGravityScale = CharacterMovement.GravityScale @2132 and OriginalWalkSpeed @2181 (not
/// kept); @2274 off a dedicated server WatermelonVisuals bound to the skin manager's SkinApplied @2334-2357 (the
/// view's); @2231-2254 RestoreShieldAmmo bound to OnLandedOnSolidGround (`restore_shield_ammo`). UpShieldAmmo /
/// UpShieldAmmoMax are the class defaults of a new pawn (the data's).
pub fn begin_play(w: &mut World, f: FighterId) {
    let g = w.fighter(f).moves.gravity_scale;
    let pawn = &data_of(w, f).fighter.pawn;
    let (ammo, max) = (num(pawn, "UpShieldAmmo").unwrap_or(2.0), num(pawn, "UpShieldAmmoMax").unwrap_or(2.0));
    let s = &mut steven_w(w, f).pawn;
    s.original_gravity_scale = g;
    s.up_shield_ammo = ammo;
    s.up_shield_ammo_max = max;
}

// ---- the glide

/// ReadGlideInput's predicate (@63-544, computed again @621-1057 on a change): no active hitstun
/// (Comp_Pawn_HitstunManager.CheckHitstunActive), the Jump button pressed (IsButtonPressed(2)), MovementMode Falling
/// (EqualEqual_ByteByte(MovementMode, 3) @229 / @742), the gravity not modified (GetIsGravityModified:
/// GravityModifierCounter > 0), not Jumping, not wall sliding, not evading. Unlike Bugs's no vertical-speed term.
fn want_glide(w: &World, f: FighterId) -> bool {
    let fi = w.fighter(f);
    !in_hitstun(w, f)
        && fi.controls.inp.jump
        && is_falling(w, f)
        && fi.moves.gravity_mod_count == 0
        && !fi.moves.cmd_jumped
        && !fi.wall.wall_sliding
        && !fi.evade.evading
}

/// StopAnimMontage(WallJumpAnim) @2014: the active instance of the wall jump's plain montage, stopped with its
/// BlendOut when it is not stopped already (as Bugs's and Batman's; the wall code plays it).
fn stop_wall_jump_anim(w: &mut World, f: FighterId) {
    let Some(d) = &w.fighter(f).montages.dash_montage else { return };
    let (uid, name, out) = (d.montage.uid, d.montage.name, d.blending_out());
    if out || nav_of(w, f).wall_jump.as_deref() != Some(w.names.str(name)) {
        return;
    }
    let data = w.data.clone();
    let Some(blend) = instance_data(&data, w, f, uid).map(|m| m.blend_out) else { return };
    stop_dash_montage(w, f, blend);
}

/// SetGlideValueMulticast @4168: isGliding = it, then @3333 the mesh AnimBP's BPI_SetIsGliding @3450 (the cast
/// succeeds: Steven_AnimBP implements BPI_AnimBP).
fn set_glide_value(w: &mut World, f: FighterId, g: bool) {
    let s = &mut steven_w(w, f).pawn;
    s.gliding = g;
    s.anim_gliding = g;
}

/// AssignGlideValues (server, reliable; @4223): UpdateAirControl(AirControl), SetGlideValueMulticast(isGliding)
/// @4246; while wall sliding nothing more @4269; else SetTerminalVelocity(false, SelectFloat(1000,
/// DefaultTerminalVelocity, isGliding), false) @4284-4335. No gravity write.
fn assign_glide_values(w: &mut World, f: FighterId, g: bool, air_control: Real) {
    update_air_control(w, f, air_control);
    set_glide_value(w, f, g);
    if !w.fighter(f).wall.wall_sliding {
        let v = if g { GLIDE_TERMINAL } else { default_terminal(data_of(w, f)) };
        set_terminal(w, f, false, v, false);
    }
}

/// ReadGlideInput (owning client, reliable; from ReceiveTick @6249 off a dedicated server, then IsLocallyControlled
/// @15: every pawn here, bots and a ticking Watermelon included). Nothing while the predicate equals isGliding
/// (@582-611). On a change: isGliding = it @1095, then the flows pushed @1114 / @1119 run last first: the AnimBP's
/// BPI_SetIsGliding @1245, UpdateAirControl(isGliding ? 1 : BaseAirControl) @1342, AssignGlideValues(isGliding, the
/// same) @1416 (offline and on the host it runs at once); the local gravity @1449 (nothing while wall sliding: on,
/// the FighterMovement cast (it succeeds), NetGravity(1.15) @1553 and SetTerminalVelocity(false, 1000, false) @1590;
/// off, only with the gravity not modified @1635, NetGravity(OriginalGravityScale) @1725 and
/// SetTerminalVelocity(false, DefaultTerminalVelocity 2900, false) @1766); then the once gate @1792 (entering closes
/// it, and with IsFalling PF_Post_Event(null) @1957, which posts nothing, and StopAnimMontage(WallJumpAnim) @2014;
/// leaving opens it @2038). BaseAirControl is BaseFighter's 0.9 (his data has no other); DefaultTerminalVelocity
/// BaseFighter's 2900.
pub fn read_glide_input(w: &mut World, f: FighterId) {
    let g = want_glide(w, f);
    if g == steven_w(w, f).pawn.gliding {
        return;
    }
    set_glide_value(w, f, g);
    let air_control = if g { 1.0 } else { AIR_CONTROL };
    update_air_control(w, f, air_control);
    assign_glide_values(w, f, g, air_control);
    if !w.fighter(f).wall.wall_sliding {
        if g {
            w.fighter_mut(f).moves.net_gravity = Some(GLIDE_GRAVITY);
            set_terminal(w, f, false, GLIDE_TERMINAL, false);
        } else if w.fighter(f).moves.gravity_mod_count == 0 {
            let o = steven_w(w, f).pawn.original_gravity_scale;
            w.fighter_mut(f).moves.net_gravity = Some(o);
            let v = default_terminal(data_of(w, f));
            set_terminal(w, f, false, v, false);
        }
    }
    let s = &mut steven_w(w, f).pawn;
    if !g {
        s.glide_once_closed = false;
    } else if !s.glide_once_closed {
        s.glide_once_closed = true;
        if is_falling(w, f) {
            stop_wall_jump_anim(w, f);
        }
    }
}

// Is Valid Slowfall Gravity (function): GetIsInAir AND no active hitstun AND NearlyEqual(Gravity, 1.15, 1e-6) AND
// NOT Comp_Pawn_BranchManager.IsAttacking (@0-324). Its one caller is BaseFighter's ValidateNetGravity, which nothing
// calls (xref calls): not ported.

// ---- EndJump, tick, cleanup

/// EndJump @4362: the parent's EndJump(DelaySeconds, Quickfall, false), as Pawn_BugsBunny's and Pawn_BatmanV2's:
/// FastfallMult = false before the jump's cut reads ComputeJumpHeight(FastfallMult).
fn end_jump(w: &mut World, f: FighterId) {
    w.fighter_mut(f).fast_fall.ff_mult = false;
    let v = jump_velocity(w, f);
    w.fighter_mut(f).jump.jump_v = v;
}

/// ReceiveTick @6209: the parent's, then ReadGlideInput @4192 (not a dedicated server @6249). No latent action of
/// his.
fn tick(w: &mut World, f: FighterId) {
    read_glide_input(w, f);
}

/// BPI_Cleanup @8644 (BaseFighter ResetFighterLogic calls it; it does not forward to BaseFighter's own, an empty
/// event): the stored Watermelon's EarlyDestroyWatermelonSteven @8360 (watermelon.rs, composed behind this),
/// isGliding false @8654, Comp_StevenShield.OnFighterReset @8665 (shield.rs, composed behind this), UpperTeeth
/// visible @8701 (the view's). No gravity, terminal or air control write and no AnimBP call: Steven_AnimBP's Gliding
/// stays.
// Deviation: the joined parts run after the isGliding write, the graph's order has the Watermelon's destroy first;
// the steps touch nothing of each other.
pub fn cleanup(w: &mut World, f: FighterId) {
    steven_w(w, f).pawn.gliding = false;
}

// ---- the shield ammo

/// RestoreShieldAmmo @10356 (bound to OnLandedOnSolidGround at BeginPlay @2254; its NewParam argument is not read):
/// UpShieldAmmo = UpShieldAmmoMax.
fn restore_shield_ammo(w: &mut World, f: FighterId) {
    let s = &mut steven_w(w, f).pawn;
    s.up_shield_ammo = s.up_shield_ammo_max;
}

/// StevenHasShieldAmmoCondition Check @0-192: the owner cast to Pawn_StevenUniverse (failed: false @165), then
/// UpShieldAmmo > 0 @79.
fn has_shield_ammo(w: &mut World, f: FighterId) -> bool {
    steven_of(w, f).is_some_and(|s| s.pawn.up_shield_ammo > 0.0)
}

// ---- BPI_Steven

// The three functions the interface declares (BPI_Steven: empty bodies), implemented by the pawn and called by the
// other files on a fighter whose cast to BPI_Steven succeeds (`steven_of` is `Some`).

/// `decrementUpShieldAmmo` (BPI_DecrementUpShieldAmmo @10404 -> @10277): UpShieldAmmo - 1 into a temporary, then
/// UpShieldAmmo = it @10249. No floor. Caller: Steven_ShieldActor_Up's BeginPlay @721 on its owner (barrier.rs); a
/// fighter of another character does nothing.
pub fn decrement_up_shield_ammo(w: &mut World, f: FighterId) {
    if let Some(s) = super::steven_mut(w, f) {
        s.pawn.up_shield_ammo -= 1.0;
    }
}

/// IsValid of an actor reference to a fighter: not destroyed. A minion leaves its match's pawns at `destroy_minion`
/// (the World's `pawns` no longer holds it); a player is never destroyed in a match (a reset makes a new StevenState).
/// A defeated minion that is not destroyed yet is valid.
fn is_valid_pawn(w: &World, m: FighterId) -> bool {
    w.pawns.as_ref().is_some_and(|ps| ps.contains(&m))
}

/// BPI_HasWatermelonSpawned (function, @0-682): true when the stored WatermelonSteven is valid @5-59; else over
/// Comp_Spawner.SpawnedActors_ClassChecked in order, true at the first entry that is valid and whose GetObjectClass
/// is exactly HealOverTimeZoneBP_C or HealGrenade_C (@356-576; a child class fails); else false @592. The member is
/// the last filter's (`spawner_filtered`), not the live list: a HealGrenade counts from its spawner's NotifyEnd until
/// another class is filtered, and a HealOverTimeZoneBP only where a filter by its class took it. Caller:
/// NoWatermelonSpawned's Check @131.
pub fn has_watermelon_spawned(w: &World, f: FighterId) -> bool {
    let Some(s) = steven_of(w, f) else { return false };
    if s.pawn.watermelon_steven.is_some_and(|m| is_valid_pawn(w, m)) {
        return true;
    }
    s.pawn
        .class_checked
        .iter()
        .any(|&a| !actor_dead(w, a) && actor_cls(w, a).is_some_and(|c| c == HEAL_ZONE || c == HEAL_GRENADE))
}

/// BPI_ClearWatermelonStevenReference @10081: WatermelonSteven = None. Caller: TriggerWatermelonCooldown @10207 on
/// the pawn's Instigator (watermelon.rs: the Watermelon's defeat, through its creator).
pub fn clear_watermelon_steven_reference(w: &mut World, f: FighterId) {
    if let Some(s) = super::steven_mut(w, f) {
        s.pawn.watermelon_steven = None;
    }
}

/// Comp_Spawner's SpawnedActors_ClassChecked filter (`CharHooks.spawner_filtered`; DestroySpawnedActors @15-412,
/// "Get Spawned Actor" @43-132): FilterArray(SpawnedActors, ActorClass), an IsA test (the class or a child), then on
/// the DestroySpawnedActors path the owner's Comp_Actor_Parent children of the class appended @230-412. Only the
/// entries BPI_HasWatermelonSpawned can pass are kept (exactly HealOverTimeZoneBP_C or HealGrenade_C).
// Deviation: the Comp_Actor_Parent children (actors attached to the pawn) are not taken: no heal actor attaches to
// him.
// Deviation: "Get Spawned Actor" called by a Steven_ShieldActor_Up's BeginPlay @829 on its owner's spawner (barrier.rs)
// filters by Steven_ShieldActor_Up_C too; it reaches this only where barrier.rs calls the hook.
fn spawner_filtered(w: &mut World, f: FighterId, cls: Name) {
    let mut out = Vec::new();
    let list: Vec<ActorRef> = w.fighter(f).items.spawned_actors.clone();
    for a in list {
        if actor_dead(w, a) {
            continue;
        }
        let Some(c) = actor_cls(w, a) else { continue };
        if c != HEAL_ZONE && c != HEAL_GRENADE {
            continue;
        }
        let is_a = c == cls || actor_entry(w, a).is_some_and(|e| e.parents.contains(&w.names.str(cls)));
        if is_a {
            out.push(a);
        }
    }
    steven_w(w, f).pawn.class_checked = out;
}

/// This file's registry entries (the conditions): NoWatermelonSpawned Check @0-262: an actor that does not implement
/// BPI_Steven -> true @230; else Not(BPI_HasWatermelonSpawned) @131-206 (a failed interface cast reads false: true).
/// StevenHasShieldAmmoCondition: `has_shield_ammo`. Both take no variable of the condition object (Condition2's empty
/// Reset).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.conditions.enter("NoWatermelonSpawned_C", Condition::Fn(no_watermelon_spawned))?;
    r.conditions.enter("StevenHasShieldAmmoCondition_C", Condition::Fn(has_shield_ammo))
}

fn no_watermelon_spawned(w: &mut World, f: FighterId) -> bool {
    !(steven_of(w, f).is_some() && has_watermelon_spawned(w, f))
}
