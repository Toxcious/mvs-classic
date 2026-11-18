//! Pawn_BugsBunny's own logic: BeginPlay's writes, the glide input, EndJump,
//! BPI_Cleanup, and the BPI_BugsBunny functions his other classes call (offsets of its ubergraph).
//!
//! Not ported, as nothing in the sim acts on it: Tick's IsDedicatedServer test (@510: not one), OnMeshActivated /
//! SpawnInvulnerability / StartJump (@3147 / @3158 / @3199: the parent's alone), the construction script's master
//! poses and team Brush scalar (the view's), HeadScale (state.rs), SpeedVisuals (@3146: empty), RandomItem /
//! HolsterAbilityItem / UnHolsterAbilityItem (@3393 / @3595 / @3762: no caller), BPI_ReadyForAI @4178 (his bot
//! subtrees), BPI_GetFighterCharacter (12: `combat::disguise`), BPI_Use2DHitboxes (CharDef.use2D), the Pie's holster
//! (`findMirrorSocket`, `pieHolsterSocket`: the view's, nothing in the sim reads where the prop sits).

use std::sync::Arc;

use super::state::{bugs, bugs_w};
use crate::Real;
use crate::actors::ProjectileId;
use crate::chars::batman::pawn::stop_wall_jump_anim;
use crate::consts::AIR_CONTROL;
use crate::events::NotifyRef;
use crate::fighter::anim::montages::start_montage;
use crate::fighter::common::{data_of, default_terminal};
use crate::fighter::match_side::{Foreign, SpawnRequest};
use crate::fighter::moves::{is_falling, jump_velocity, set_terminal, update_air_control};
use crate::fighter::{FighterId, in_hitstun, unequip_object};
use crate::name;
use crate::names::Name;
use crate::registry::CharHooks;
use crate::world::World;
use mvs_anim::montage::PROXY_NONE;
use mvs_data::value::{Props, Value};

/// NetGravity(1.15) @2542; IsValidSlowfallGravity's NearlyEqual @68.
const GLIDE_GRAVITY: Real = 1.15;
/// SetTerminalVelocity(false, 1000, false) @2579, the select @3069.
const GLIDE_TERMINAL: Real = 1000.0;
const DISPENSER: Name = name!("ItemDispenser_Bugs_C");
const LINGERING_KISS: Name = name!("LingeringLoveProjectile_C");
const TUNNEL_PUNCH: Name = name!("BugsBunny_Attack_TunnelPunch_Montage");

/// The hooks of this file in Bugs Bunny's CharHooks (mod.rs's `tick`, `endJump`, `reset`).
pub fn hooks(h: &mut CharHooks) {
    h.tick = Some(tick);
    h.end_jump = Some(end_jump);
    h.reset = Some(cleanup);
}

// ---------------------------------------------------------------- BeginPlay

/// ReceiveBeginPlay @561 (a new pawn: the World's reset, the match's first spawn included): the parent's, then the
/// pushed flows run last first: @581-691 HulkAnimBP, OriginalGravityScale = CharacterMovement.GravityScale @642 and
/// OriginalWalkSpeed @691 (state.rs: read by nothing of his); @741-764 OnFacingChange bound to FacingChanged (the
/// view's); @35 on authority (every pawn of the port's sim), Comp_Spawner.CompSpawnActor(ItemDispenser_Bugs_C,
/// AttachToActor true, ActorToAttachTo self, AttachSocket None, WorldTransforms true, the identity transform,
/// direction (0, 0, 0), InitialSpeed 0, MaxSpeed 0, no charge, AttackID 0) @65. The dispenser is dispenser.rs's
/// class: the request goes to the world's spawn pass by name. No deferred setup timer here.
pub fn begin_play(w: &mut World, f: FighterId) {
    let g = w.fighter(f).moves.gravity_scale;
    bugs_w(w, f).original_gravity_scale = g;
    let mut p = Props::new();
    p.insert("AttachToActor".to_owned(), Value::Bool(true));
    p.insert("Socket".to_owned(), Value::Str("None".to_owned()));
    p.insert("WorldTransforms".to_owned(), Value::Bool(true));
    p.insert("InitialSpeed".to_owned(), Value::Num(0.0));
    p.insert("MaxSpeed".to_owned(), Value::Num(0.0));
    let r = SpawnRequest {
        cls: DISPENSER,
        n: NotifyRef { montage: Name::NONE, index: 0 },
        attack_id: 0,
        foreign: Foreign::None,
        copy: None,
        charge: None,
        aim: None,
        props: Some(Arc::new(p)),
        instigator: None,
        origin: None,
    };
    w.fighter_mut(f).requests.spawn_requests.push(r);
}

// ---------------------------------------------------------------- the glide

/// ReadGlideInput's predicate (@1052-1533, computed again @1610-2046 on a change): no active hitstun
/// (Comp_Pawn_HitstunManager.CheckHitstunActive), the Jump button pressed (IsButtonPressed(2)), MovementMode Falling,
/// the gravity not modified (GetIsGravityModified: GravityModifierCounter > 0), not Jumping, not wall sliding, not
/// evading.
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

/// SetGlideValueMulticast @3210: isGliding = it, then the mesh AnimBP's BPI_SetIsGliding @3346.
fn set_glide_value(w: &mut World, f: FighterId, g: bool) {
    let s = bugs_w(w, f);
    s.gliding = g;
    s.anim_gliding = g;
}

/// AssignGlideValues (server, reliable; @3008): UpdateAirControl(AirControl), SetGlideValueMulticast(isGliding)
/// @3031; while wall sliding nothing more @3054; else SetTerminalVelocity(false, isGliding ? 1000 :
/// DefaultTerminalVelocity, false) @3120. No gravity write and no velocity clamp (Batman's @4628 has one; his does
/// not).
fn assign_glide_values(w: &mut World, f: FighterId, g: bool, air_control: Real) {
    update_air_control(w, f, air_control);
    set_glide_value(w, f, g);
    if !w.fighter(f).wall.wall_sliding {
        let v = if g { GLIDE_TERMINAL } else { default_terminal(data_of(w, f)) };
        set_terminal(w, f, false, v, false);
    }
}

/// ReadGlideInput (owning client, reliable; from ReceiveTick @546 off a dedicated server, then IsLocallyControlled
/// @1004: every pawn here, bots included). Nothing while the predicate equals isGliding (@1571-1600). On a change:
/// isGliding = it @2084, then the flows pushed @2103 / @2108 run last first: the AnimBP's BPI_SetIsGliding @2234,
/// UpdateAirControl(isGliding ? 1 : BaseAirControl) @2331, AssignGlideValues(isGliding, the same) @2405 (offline and
/// on the host it runs at once); the local gravity @2438 (nothing while wall sliding: on, the FighterMovement cast
/// (it succeeds), NetGravity(1.15) @2542 and SetTerminalVelocity(false, 1000, false) @2579; off, only with the
/// gravity not modified @2624, NetGravity(OriginalGravityScale) @2714 and SetTerminalVelocity(false,
/// DefaultTerminalVelocity 2900, false) @2755); then the once gate @2781 (entering closes it, and with IsFalling
/// PF_Post_Event(null) @2911, which posts nothing, and StopAnimMontage(WallJumpAnim) @2968; leaving opens it @2992).
/// BaseAirControl is BaseFighter's 0.9 (his data has no other).
pub fn read_glide_input(w: &mut World, f: FighterId) {
    let g = want_glide(w, f);
    if g == bugs(w, f).gliding {
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
            let o = bugs(w, f).original_gravity_scale;
            w.fighter_mut(f).moves.net_gravity = Some(o);
            let v = default_terminal(data_of(w, f));
            set_terminal(w, f, false, v, false);
        }
    }
    let s = bugs_w(w, f);
    if !g {
        s.glide_once_closed = false;
    } else if !s.glide_once_closed {
        s.glide_once_closed = true;
        if is_falling(w, f) {
            stop_wall_jump_anim(w, f);
        }
    }
}

/// Is Valid Slowfall Gravity (function): GetIsInAir AND no active hitstun AND NearlyEqual(Gravity, 1.15, 1e-6) AND
/// NOT Comp_Pawn_BranchManager.IsAttacking (@0-324). Its one caller is BaseFighter's ValidateNetGravity, which
/// nothing calls: kept for that caller, called by nothing in the port.
pub fn is_valid_slowfall_gravity(w: &World, f: FighterId, gravity: Real) -> bool {
    crate::fighter::in_air(w, f)
        && !in_hitstun(w, f)
        && (gravity - GLIDE_GRAVITY).abs() <= 1e-6
        && !crate::fighter::is_attacking(w, f)
}

// ---------------------------------------------------------------- EndJump, tick, cleanup

/// EndJump @3169: the parent's EndJump(DelaySeconds, Quickfall, false), as Pawn_BatmanV2's @5931: FastfallMult = false
/// before the jump's cut reads ComputeJumpHeight(FastfallMult), so the cut is the unboosted JumpVelocity.
pub fn end_jump(w: &mut World, f: FighterId) {
    w.fighter_mut(f).fast_fall.ff_mult = false;
    let v = jump_velocity(w, f);
    w.fighter_mut(f).jump.jump_v = v;
}

/// ReceiveTick @491: the parent's, then ReadGlideInput @546 (not a dedicated server). No latent action of his own.
pub fn tick(w: &mut World, f: FighterId) {
    read_glide_input(w, f);
}

/// BPI_Cleanup @4046 (BaseFighter ResetFighterLogic calls it; BaseFighter's own BPI_Cleanup @96960 is an empty event,
/// so no forward is missed): isGliding, SpeedOn and BombEquipped? false (the last two: state.rs, not kept),
/// Comp_Spawner.UnequipObject(PieProp1) @4079 and UnequipObject(Pie) @4128. Nothing else: no gravity, terminal or air
/// control write, no AnimBP call (its IsGliding stays), no tunnel or lingering kiss cleared.
pub fn cleanup(w: &mut World, f: FighterId) {
    bugs_w(w, f).gliding = false;
    unequip_object(w, f, name!("PieProp1"));
    unequip_object(w, f, name!("Pie"));
}

// ---------------------------------------------------------------- BPI_BugsBunny

/// BPI_TunnelPunch @420 (no authority gate): SkipImpulseValidation = true, then PlayAnimMontage(
/// BugsBunny_Attack_TunnelPunch_Montage, 1.0, None) @431. ACharacter::PlayAnimMontage is a plain Montage_Play on the
/// pawn's mesh, past the branch manager: the montage replaces the attack slot's as an attack's does and BeginAttack
/// is not set (as Creature's wall bounce and LeBron's ball return). The montage's own isAttackingFlag state makes him
/// attacking.
// Deviation: what else the port's startMontage does for an attack start rides along (the attack count, the charge
// values and the hit flag reset, the ground / air section choice by `grounded`: the montage has the one section
// Default).
pub fn tunnel_punch(w: &mut World, f: FighterId) {
    bugs_w(w, f).skip_impulse_validation = true;
    start_montage(w, f, TUNNEL_PUNCH, &PROXY_NONE, None);
    w.fighter_mut(f).charge.begin_attack = false;
}

/// BPI_SetBugsLingeringKiss @216 (from LingeringLoveProjectile's BeginPlay, its Instigator's), on authority: the
/// argument cast to LingeringLoveProjectile @246 (failed: nothing); the stored one, when valid, K2_DestroyActor @364;
/// then the argument stored @400. No identity test: the same actor passed twice is destroyed and kept.
pub fn set_lingering_kiss(w: &mut World, f: FighterId, kiss: ProjectileId) {
    if w.actors.projectiles.get(kiss).is_none_or(|p| p.cls != LINGERING_KISS) {
        return;
    }
    let old = bugs(w, f).lingering_kiss;
    if let Some(o) = old
        && w.actors.projectiles.get(o).is_some_and(|p| !p.dead)
    {
        crate::world::spawning::destroy_projectile(w, o, false);
    }
    bugs_w(w, f).lingering_kiss = Some(kiss);
}
