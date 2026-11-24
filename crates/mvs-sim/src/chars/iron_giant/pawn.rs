//! Pawn_C017's own logic: the conditions, the notifies, the torso flip, the
//! giant crouch and the turret aim, the hover (readHoverInput, hoverChanged, the ground hover jump), weapon mode
//! (setAlternateMode, outOfRage, weaponInput), the knock-back and death handlers, and the character's hooks (init,
//! tick, pressed, montageEnded, knockedBack, allyHitByEnemy, allyExitedArena, death, reset, matchReset). The bot
//! injections (`botInjections`) come with the bots' crate.

use super::hover_rage::{self, HoverRage};
use super::pose::GiantAnim;
use super::state::{giant, giant_mut, giant_of, giant_w};
use super::turret::{ally_turret_reset, toggle_ally_turret};
use crate::Real;
use crate::chars::batman::pawn::stop_wall_jump_anim;
use crate::combat::buffs::{buff_of, buff_signal};
use crate::combat::defense::{add_temp_health_pack, remove_temp_health_pack};
use crate::combat::holds::HoldHit;
use crate::consts::{AIR_CONTROL, GRAVITY_SCALE, MAX_EVADE_COUNT};
use crate::events::{Event, NotifyRef};
use crate::fighter::anim::grouped::{play_grouped, stop_grouped};
use crate::fighter::anim::montages::{montage_is_active, start_montage};
use crate::fighter::anim::set_sub_mesh;
use crate::fighter::common::{data_of, default_terminal, latent_frames, warn_text};
use crate::fighter::control::{press_dir, trigger_ability_check};
use crate::fighter::moves::{
    Impulse, clear_gravity_override, fast_fall_end, is_falling, net_impulse, reset_jump_counts, set_terminal, settle,
    sweep_move, update_air_control,
};
use crate::fighter::{
    FighterId, active_montage_name, actor_y, facing, grapple_off, grounded, in_hitstun, is_attacking, max_walk,
    set_tenticle_override,
};
use crate::input::{AttackBtn, Dir};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::CondFn;
use crate::registry::props::{arr, flag, name as data_name, num, num_or, obj, text};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{CharHooks, Condition, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use mvs_anim::montage::PROXY_NONE;
use mvs_data::value::{Props, Value};

// ---- the conditions

/// C017_Fuel_Check_Condition: FuelAmount >= FuelPercent (CDO 0.25) x FuelMaxAmount; without the component false.
fn fuel_check(w: &mut World, f: FighterId) -> bool {
    giant_of(w, f).is_some_and(|g| g.hover_rage.as_ref().expect("the hover rage").has_fuel(0.25))
}

/// C017_Fuel_Check_False_Condition: its inverse, false without the component.
fn fuel_check_false(w: &mut World, f: FighterId) -> bool {
    giant_of(w, f).is_some_and(|g| !g.hover_rage.as_ref().expect("the hover rage").has_fuel(0.25))
}

/// Pawn_C017's IsMaxRage / AlternateMode (a cast to the pawn: false for both on another pawn).
fn rage(w: &mut World, f: FighterId) -> bool {
    giant_of(w, f).is_some_and(|g| g.is_max_rage)
}
fn rage_false(w: &mut World, f: FighterId) -> bool {
    giant_of(w, f).is_some_and(|g| !g.is_max_rage)
}
fn weapon_mode(w: &mut World, f: FighterId) -> bool {
    giant_of(w, f).is_some_and(|g| g.alternate_mode)
}
fn not_weapon_mode(w: &mut World, f: FighterId) -> bool {
    giant_of(w, f).is_some_and(|g| !g.alternate_mode)
}

/// Check @79-168: IsValid(BPI_GetCurrentHoldBuff), and its inverse (Buff_HeldBy_C017 sets and clears it).
fn holding_enemy(w: &mut World, f: FighterId) -> bool {
    w.fighter(f).holds.hold_buff.is_some()
}
fn not_holding_enemy(w: &mut World, f: FighterId) -> bool {
    w.fighter(f).holds.hold_buff.is_none()
}

/// !BPI_CheckIsHovering.
fn not_hover(w: &mut World, f: FighterId) -> bool {
    !giant_of(w, f).is_some_and(|g| g.gliding)
}

/// CheckHasBuff(Buff_C017_AllyBarrierGrayHealth), a perk's marker (perks.rs) and its inverse. No asset of the build
/// uses either condition.
fn has_gray_health(w: &mut World, f: FighterId) -> bool {
    buff_of(w, f, name!("Buff_C017_AllyBarrierGrayHealth_C")).is_some()
}
fn not_has_gray_health(w: &mut World, f: FighterId) -> bool {
    !has_gray_health(w, f)
}

// ---- the notifies

/// Begin / End: the owner cast to Pawn_C017 (@42-107), nothing for another pawn (a Disguise's copied
/// C017_Evade_Air_Montage runs it with the wearer as owner). Begin also BindHoverJump(true): hoverJumpPress, land.
fn can_hover_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(g) = giant_mut(w, f) {
        g.can_attack_hover = true;
    }
}
fn can_hover_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(g) = giant_mut(w, f) {
        g.can_attack_hover = false;
    }
}

/// BPI_ForceHover(true, IgnoreFuel) / (false, false).
fn force_hover_begin(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    let ignore = flag(crate::fighter::match_side::notify_props(w, f, n), "IgnoreFuel");
    let g = giant_w(w, f);
    g.attack_force_hover = true;
    g.hover_ignore_fuel = ignore;
}
fn force_hover_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    let g = giant_w(w, f);
    g.attack_force_hover = false;
    g.hover_ignore_fuel = false;
}

/// Received_NotifyBegin: with BPI_EvadePercent (1 - EvadeCount / MaxEvadeCount) above 0.04, AddTempHealthPack(
/// BlockTempHealth, Lerp(1, 15, the percent clamped to 0..1), Time 0, CanOverride).
fn evade_temp_health_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    let pc = 1.0 - (w.fighter(f).evade.evade_count / MAX_EVADE_COUNT);
    if pc > 0.04 {
        let amount = 1.0 + (min(max(pc, 0.0), 1.0) * 14.0);
        add_temp_health_pack(w, f, name!("BlockTempHealth"), amount, true, 0.0);
    }
}
/// Received_NotifyEnd.
fn evade_temp_health_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    remove_temp_health_pack(w, f, name!("BlockTempHealth"));
}

/// IsBurst: UseFuel(UsedFuelPercent) once; else AttackUsingFuel(NoFuelSectionJump, the montage, UsedFuelPercent, true)
/// until the end.
fn use_fuel_begin(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    if giant_of(w, f).is_none() {
        return;
    }
    let p = crate::fighter::match_side::notify_props(w, f, n);
    let burst = flag(p, "IsBurst");
    let percent = num_or(p, "UsedFuelPercent", 0.0);
    let section = text(p, "NoFuelSectionJump").map_or(name!("None"), |s| data_name(w, s));
    if burst {
        hover_rage::use_fuel(w, f, percent);
    } else {
        let montage = w.fighter(f).montages.montage.as_ref().map_or(Name::NONE, |m| m.montage.name);
        hover_rage::set_attack_fuel(w, f, true, percent, section, montage);
    }
}
fn use_fuel_end(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    if giant_of(w, f).is_some() && !flag(crate::fighter::match_side::notify_props(w, f, n), "IsBurst") {
        hover_rage::set_attack_fuel(w, f, false, 0.0, Name::NONE, Name::NONE);
    }
}

/// SetAlternateModeVisuals(WeaponModeOn?). The state's Begin binds CheckForAnimInterrupt (@23804), which enters the
/// mode when C017_WeaponMode_In_Montage is interrupted: the state's end at the interruption does the same here, so
/// the binding is not kept.
fn weapon_mode_visuals_end(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    if giant_of(w, f).is_some() {
        let on = flag(crate::fighter::match_side::notify_props(w, f, n), "WeaponModeOn?");
        set_alternate_mode(w, f, on);
    }
}

/// AnimNotify_C017_IsBlocking: BPI_SetBlocking(true) at the begin, (false) at the end (@121 of each).
/// Pawn_C017 @20025: IsBlocking = Blocking AND NOT AlternateMode, then MaxWalkSpeed = OriginalWalkSpeed, x 1.5
/// while IsBlocking, x HoverSpeedMult while isGliding (Fighter.maxWalk). IsBlocking's other reader, BlockMeterTick,
/// ends in UpdateBlockValuesMulticast, which is empty (@21174); the AnimBP's IsBlocking is another variable
/// (BPI_IsBlocking, called by Buff_C017_BlockDamageMitigation).
/// The notify's Begin / End: the owner cast to BPI_C017 (@42-107), nothing for an owner without it.
fn is_blocking_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(g) = giant_mut(w, f) {
        g.giant_blocking = !g.alternate_mode;
    }
}
fn is_blocking_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(g) = giant_mut(w, f) {
        g.giant_blocking = false;
    }
}

/// AnimNotify_C017_TurretAim: SetTurretAim(true) at the begin, (false) at the end (@121 of each).
fn turret_aim_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    set_turret_aim(w, f, true);
}
fn turret_aim_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    set_turret_aim(w, f, false);
}

/// AnimNotify_GiantIsCrouched (the AnimSequence C017_Crouch carries it over its whole length; dispatched as a
/// sequence state, `fighter/anim` sequences): BPI_SetGiantCrouched(true) at the begin, (false) at the end (@121 of
/// each), the owner cast to BPI_C017 (@42-107), nothing for another pawn.
fn giant_crouched_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    set_giant_crouched(w, f, true);
}
fn giant_crouched_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    set_giant_crouched(w, f, false);
}

/// The ones below write C017_AnimBP variables that its anim graph reads.
/// AnimNotify_C017_MovementSplit: MovementSplitActive true / false (@121 of the begin / end);
/// AnimNotify_C017_MovementSplitJumpCheck: SplitMovementJumpCheck true / false (@121). C017_AnimBP: MovementSplitActive
/// OR (SplitMovementJumpCheck AND IsFalling) is BlendListByBool_2's value (@9459-9535) and LayeredBoneBlend_11's
/// weight (@12131-12244), and with NOT IsFalling BlendListByBool_7's (@15081-15224): the locomotion's lower body
/// under the montage (split.rs; the shapes ride it, pose.rs).
fn giant_anim(w: &mut World, f: FighterId) -> Option<&mut GiantAnim> {
    giant_mut(w, f).and_then(|g| g.giant_anim.as_mut())
}
fn movement_split_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(ga) = giant_anim(w, f) {
        ga.split_active = true;
    }
}
fn movement_split_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(ga) = giant_anim(w, f) {
        ga.split_active = false;
    }
}
fn jump_check_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(ga) = giant_anim(w, f) {
        ga.jump_check = true;
    }
}
fn jump_check_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(ga) = giant_anim(w, f) {
        ga.jump_check = false;
    }
}

/// AnimNotify_C017_Hold_Overide (begin only, @121-168): HoldAlpha = OverrideOn ? 1 : 0. C017_AnimBP: HoldAlpha x
/// HoldingBuffValid is ModifyBone_4's alpha (@10652-10771, the translation HoldLocation) and LayeredBoneBlend_4's
/// weight (@10808-10891). HoldingBuffValid is the hold buff's (Buff_HeldBy_C017 SetHoldOverride, which writes
/// HoldAlpha 0 at its end too). OverrideOn is off in the class defaults (C017_ThrowScrap_Montage's instance).
fn hold_override_begin(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    let on = flag(crate::fighter::match_side::notify_props(w, f, n), "OverrideOn");
    giant_w(w, f).hold_alpha = if on { 1.0 } else { 0.0 };
}

/// AnimNotify_C017_GrabDrop Received_NotifyBegin: the owner's BPI_CheckConditions(Conditions) (@121; no instance has
/// any), BPI_GetCurrentHoldBuff valid (@357) and a Buff_HeldBy_C017 -> DetachAndPosition(DropLocation, 'Hit Box
/// Damage') (@479).
fn grab_drop_begin(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    let p = crate::fighter::match_side::notify_props(w, f, n);
    if !arr(p, "Conditions").is_empty() {
        warn_text(w, "AnimNotify_C017_GrabDrop: Conditions not ported");
        return;
    }
    let (x, y) = obj(p, "DropLocation").map_or((0.0, 0.0), |d| (num_or(d, "X", 0.0), num_or(d, "Y", 0.0)));
    let packet = obj(p, "Hit Box Damage").cloned().unwrap_or_default();
    let Some(vic) = w.fighter(f).holds.hold_buff else { return };
    let held_by_giant = w
        .fighter(vic)
        .holds
        .hold
        .as_ref()
        .is_some_and(|h| matches!(h.cls, crate::combat::BuffClass::Data(c) if c == name!("Buff_HeldBy_C017_C")));
    if held_by_giant {
        detach_and_position(w, f, vic, x, y, packet);
    }
}

/// AnimNotify_C017_GrabThrow Received_NotifyBegin: LocalFinalHit = 'Hit Box Damage' (@2524; UseAimActorForLaunch, off
/// in the class defaults and in every instance, builds it from the ProjectileAimer's ArrowAimVector and the charge
/// instead, @248-838); BPI_GetCurrentHoldBuff valid (@2207) -> the owner's Comp_Pawn_HitBoxHandler.pfgDoDamage(null,
/// the buff's owner, LocalFinalHit, ...) (@2385). The buff is not touched: the hit's knockback ends it
/// (Buff_HeldByEnemy KnockedBack @7848).
fn grab_throw_begin(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    let p = crate::fighter::match_side::notify_props(w, f, n);
    if flag(p, "UseAimActorForLaunch") {
        warn_text(w, "AnimNotify_C017_GrabThrow: UseAimActorForLaunch not ported");
        return;
    }
    let packet = obj(p, "Hit Box Damage").cloned().unwrap_or_default();
    if let Some(vic) = w.fighter(f).holds.hold_buff {
        w.fighter_mut(f).holds.hold_hits.push(HoldHit { vic, p: packet });
    }
}

/// AnimNotify_Hitframe_C017_Throw: AnimNotify_Hitframe's graph with LocalIgnorActor = the hold buff's owner when the
/// buff is valid (@210-252), which goes into the packet's IgnoreActor (@1856), and HitEachActorOnlyOnce on;
/// NotifyEnd: HitBoxesOff(HitBoxTags) (@229). The ignored actor is fixed at the begin, as a fighter already hit.
fn hitframe_throw_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    crate::registry::notifies::hitframe_begin(w, f, n, idx);
    if let Some(vic) = w.fighter(f).holds.hold_buff {
        let id = w.fighter(vic).id;
        if let Some(h) = w.fighter_mut(f).montages.active_hits.get_mut(&idx) {
            h.hit_set.insert(id);
        }
    }
}
fn hitframe_throw_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    crate::registry::notifies::hitframe_end(w, f, n, idx);
}

/// AnimNotify_C017_TenticleOveride (begin only, Received_NotifyBegin @121): TenticleOverride = OverrideOn, off when
/// the instance has no key (C017_WeaponMode_Out_Tenticle_Montage's). C017_AnimBP: it is ApplyAdditive_3's alpha
/// (@7377-7414) and LayeredBoneBlend_5's weight (@10957-10994), the pilot's pose; the renderer reads
/// Fighter.tenticleOverride. Its montages are the ally turret's.
fn tenticle_override_begin(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    let on = flag(crate::fighter::match_side::notify_props(w, f, n), "OverrideOn");
    set_tenticle_override(w, f, on);
}

// ---- the turret aim, the giant crouch, the knock back

/// The WeaponMode3 group's turn montage.
const WEAPON_TURN: Name = name!("C017_Weapon_Turn_Montage");

/// Pawn_C017 SetTurretAim (@10153), counted by its callers. On: TurretAimNumber + 1 and, at 1, 'Turrent Aim On',
/// TurretAimStartFacingLeft = FacingLeft, FacingChange bound (@10359), the AnimBP's BPI_TurretAim(true) and
/// BPI_TorsoFlip(false), Montage_Stop(0, C017_Weapon_Turn_Montage). Off (@11827): the count - 1 and, at 0, the flag
/// off, FacingChange unbound (@10808), BPI_TurretAim(false) and BPI_TorsoFlip(false), then in weapon mode WeaponMesh
/// and WeaponMeshTop shown and WeaponMeshReverse hidden (@11178-11298), and C017_Weapon_Turn_Montage played when the
/// facing is no longer the start's (@11358-11406).
/// FacingChange (@10091, bound to the fighter's FacingChanged while the aim is on) -> WeaponModeTorsoFlip(FacingLeft
/// != TurretAimStartFacingLeft) (@9752): in weapon mode WeaponMeshTop shown and WeaponMeshReverse hidden, or the
/// reverse (@9791 / @9859), and the AnimBP's BPI_TorsoFlip (@10044): `torso_flip` below.
/// The aimers: C017_CannonAim reads BPI_CheckTurretFacing (actors.rs). C017_PlasmaAimer's activation (@6586-7266)
/// reads both, drops BPI_CheckTurretFacing's result and selects the arrow's Y by (FacingLeft == InitiallyLeft ?
/// InitiallyLeft : !InitiallyLeft), which is FacingLeft: firePlasma's rule as it is.
/// C017_Weapon_Turn_Montage (0.3 s on the slot WeaponAdditive3, group WeaponMode3, no notifies) is an instance beside
/// the main montage (`play_grouped`): stopped by the first on (@10761), and played by the last off in weapon mode
/// with a changed facing through the PlayMontage proxy (@11406: rate 1, start 0, no section), which calls
/// Montage_Play with bStopAllMontages on (stock UE 4.26: its own group alone is stopped). The five delegates bound
/// after it (@11506-11785) have no handler with an effect.
/// BPI_TurretAim / BPI_TorsoFlip write only AnimBP variables; the shapes take the half turn and the aim additives
/// from the sim's own pose (pose.rs).
// Deviation: the flip is read once a tick from the facing, not at each FacingChanged broadcast.
fn torso_flip(w: &mut World, f: FighterId) {
    let g = giant(w, f);
    if !g.turret_aim_on || !g.alternate_mode {
        return;
    }
    let reverse = (facing(w, f) < 0.0) != g.turret_aim_start_left;
    set_sub_mesh(w, f, name!("WeaponMeshTop"), !reverse);
    set_sub_mesh(w, f, name!("WeaponMeshReverse"), reverse);
}

/// Pawn_C017 BPI_SetGiantCrouched -> @24211 Comp_Pawn_Giant.SetGiantCrouched: GiantCrouched = the value @0, then
/// broadcast GiantCrouchChanged(GiantCrouched, its owner) @39, on every call (a write of the same value too). The
/// bound functions: the smoke zones' HandleGiantCrouch (`GiantState.crouch_listeners`, the actor hook
/// `giant_crouch_changed`; a dead actor is dropped), then his occlusion buffs' OnGiantCrouch (Buff_Invisibility @1795
/// -> @9597: BuffSignal GiantCrouchChanged, the buffs live when the signal is sent).
// Deviation: the build's broadcast calls one copied invocation list from index 0 up in bind order, a function bound
// during it not called, one unbound during it still called (exe 14079aa20). Here two lists, the actors' and then the
// buffs': the usual case binds them in that order (a zone binds at its OnBeginOverlap @94-168, ahead of the Fog buff
// it spawns @209 -> SpawnBuffsOnActor); a buff a zone handler spawned in this broadcast is called too (the build
// does not call it), its AttemptToHidePlayer finding what its BeginPlay's just did; an unbind keeps the order where
// 4.26's Remove swaps the last entry in (not read in the exe). No case built differs in outcome.
fn set_giant_crouched(w: &mut World, f: FighterId, crouched: bool) {
    let Some(g) = giant_mut(w, f) else { return };
    g.giant_crouched = Some(crouched);
    let list = g.crouch_listeners.clone();
    if let Some(l) = list.filter(|l| !l.is_empty()) {
        let dead = |w: &World, a| w.actors.actors.get(a).is_none_or(|x| x.dead);
        if l.iter().any(|&a| dead(w, a)) {
            let live: Vec<_> = l.iter().copied().filter(|&a| !dead(w, a)).collect();
            giant_w(w, f).crouch_listeners = Some(live);
        }
        for &a in &l {
            let Some(act) = w.actors.actors.get(a).filter(|x| !x.dead) else { continue };
            let h = match w.registry.actors.get(w.names.str(act.cls)).map(|k| &k.hooks) {
                Some(crate::registry::entries::ActorHooks::World(h)) => h.giant_crouch_changed,
                _ => None,
            };
            if let Some(h) = h {
                h(w, a, f, crouched);
            }
        }
    }
    buff_signal(
        w,
        f,
        BuffSignal::GiantCrouchChanged,
        BuffSignalArg { crouched: Some(crouched), ..BuffSignalArg::default() },
    );
}

fn set_turret_aim(w: &mut World, f: FighterId, on: bool) {
    if on {
        let left = facing(w, f) < 0.0;
        let g = giant_w(w, f);
        g.turret_aim_number += 1.0;
        if g.turret_aim_number != 1.0 {
            return;
        }
        g.turret_aim_on = true;
        g.turret_aim_start_left = left;
        stop_grouped(w, f, Some(WEAPON_TURN), 0.0); // @10761: Montage_Stop(0.0, C017_Weapon_Turn_Montage)
        return;
    }
    let g = giant_w(w, f);
    g.turret_aim_number -= 1.0;
    if g.turret_aim_number != 0.0 {
        return;
    }
    g.turret_aim_on = false;
    if !g.alternate_mode {
        return;
    }
    let start_left = g.turret_aim_start_left;
    set_sub_mesh(w, f, name!("WeaponMesh"), true);
    set_sub_mesh(w, f, name!("WeaponMeshTop"), true);
    set_sub_mesh(w, f, name!("WeaponMeshReverse"), false);
    // @11358-11406: TurretAimStartFacingLeft != FacingLeft -> the PlayMontage proxy
    if start_left != (facing(w, f) < 0.0) {
        play_grouped(w, f, WEAPON_TURN, true, false);
    }
}

/// Pawn_C017 WeaponModeTurnAnim (@11972 -> @12049): in weapon mode with the turret aim off, the same proxy play. No
/// decoded graph calls the event (xref: 0 callers), so nothing here does either.
pub fn weapon_mode_turn_anim(w: &mut World, f: FighterId) {
    let g = giant(w, f);
    if g.alternate_mode && !g.turret_aim_on {
        play_grouped(w, f, WEAPON_TURN, true, false);
    }
}

/// Pawn_C017 OnKnockBack (@20359), bound to its Comp_Pawn_DamageManager's KnockedBack (@20500-20523): with
/// GreyHealthActive or AlternateMode and Knock Back >= 100, PlayMontageServer (@19215 -> PlayMontageMulticast @13884:
/// Montage_Play(m, 1.0, 0, 0.0, true)) of an additive hit react: CharacterMovement.IsCrouching ->
/// C017_Additive_HitReact_Crouch_Montage, else |Velocity.Y| > 0 -> _Moving_Montage, else _01_Montage (@18935-19215).
/// GreyHealthActive has no writer in the decoded graphs (class default off): weapon mode alone.
/// _01 and _Moving are in AdditiveGroup (slot Additive, the outermost additive of C017_AnimBP): an instance beside
/// the others, which stops an earlier hit react alone. The broadcast comes after the hit's Montage_Stop(0.0, null)
/// (HitReactionMulticast @9984), so the react outlives it and plays through the hitstun.
/// The velocity is the one before the hit's launch (ApplyKnockback runs at the pause's end).
// Deviation, not played: the Crouch montage. Its track is on DefaultSlot, so it is a DefaultGroup instance without
// attack state that would have to advance in hitstun, which the port's montage update there does not do for
// `dashMontage`. Reached only by a Giant crouched in weapon mode (CrouchLocked keeps a crouch held at the entry).
#[allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-keeping negated tests
fn on_knock_back(
    w: &mut World,
    f: FighterId,
    base_kb: Real,
    _dx: Real,
    _dy: Real,
    _by: Option<crate::actors::ActorRef>,
) {
    if !giant(w, f).alternate_mode || !(base_kb >= 100.0) {
        return;
    }
    if w.fighter(f).capsule.cmc_crouched {
        return;
    }
    let m = if w.fighter(f).body.vx.abs() > 0.0 {
        name!("C017_Additive_HitReact_Moving_Montage")
    } else {
        name!("C017_Additive_HitReact_01_Montage")
    };
    play_grouped(w, f, m, true, false);
}

// ---- the grab

/// Buff_HeldBy_C017 DetachAndPosition(OffsetLocation, HitBoxDamage) (@106): IsHeld off, SetHoldOverride(false); with
/// authority: the owner's K2_DetachFromActor(KeepWorld) (@307) and zero rotation (@365), the owner's
/// K2_SetActorLocation((0, instigator.Y -+ Offset.X by its facing, instigator.Z + Offset.Y), sweep) (@929: Offset.X
/// ahead of the instigator), SetMovementMode(Falling) (@1062), FMC.ClearGravityOverride (@1409), AttackDecayType 5
/// (@1441) and the instigator's Comp_Pawn_HitBoxHandler.pfgDoDamage(null, owner, packet) (@1568).
/// The buff is not destroyed here: the hit's knockback ends it (Buff_HeldByEnemy KnockedBack @7848).
/// The location is a sweeping SetActorLocation from the held position (the detach comes first): the capsule stops at
/// the first blocking hit on the way to the target (`sweep_move`).
// Unverified: the detach is taken to end the grapple's hold on the position (RemoveGrapple itself runs at the buff's
// EndPlay), so the fighter's own movement runs from the next frame; a capsule the sweep left in the stage is taken
// out as that movement's first move would (`settle`).
fn detach_and_position(w: &mut World, by: FighterId, vic: FighterId, off_x: Real, off_y: Real, mut packet: Props) {
    if w.fighter(vic).holds.hold.is_none() {
        return;
    }
    let g = giant_w(w, by);
    g.hold_alpha = 0.0;
    g.hold_valid = false;
    if let Some(h) = &mut w.fighter_mut(vic).holds.hold {
        h.attached = false;
    }
    let x = w.fighter(by).body.x + off_x * facing(w, by);
    let y = actor_y(w, by) + off_y;
    sweep_move(w, vic, x, y);
    grapple_off(w, vic);
    clear_gravity_override(w, vic); // @1409
    settle(w, vic);
    packet.insert("AttackDecayType".into(), Value::Num(5.0));
    w.fighter_mut(by).holds.hold_hits.push(HoldHit { vic, p: packet });
}

// ---- the hover

/// Pawn_C017 @6470: NetGravity(-4.0) while hovering.
const HOVER_GRAVITY_SCALE: Real = -4.0;

/// Pawn_C017 ReadHoverInput (ubergraph @2813, from ReceiveTick when locally controlled). isGliding ends when the
/// movement mode is no longer Falling (with ResetJump(0)); otherwise it follows the gate @4063: no hitstun, the Jump
/// button held (IsButtonPressed(2), Enum_InputButton Jump) or AttackForceHover in Falling, Falling or CanAttackHover,
/// not Jumping (the jump's EndJump clears it), no wall slide, not evading and not attacking unless CanAttackHover
/// (evading too with BlockEvadeMontage @4640: a polymorphed giant, `combat::polymorph`), Velocity.Z at most MaxHoverSpeed
/// plus 200, and fuel left or IgnoreFuel. Then the flows pushed @2813: the gravity scale back to default when it is
/// neither the hover's nor a modifier's (@3396), ResetJump(0) while walking with jumps counted and not Jumping
/// at 3757. The rage callout (@3940) is the HUD's.
#[allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-keeping negated tests
fn read_hover_input(w: &mut World, f: FighterId) {
    let falling = is_falling(w, f); // MovementMode == 3 @2878
    if giant(w, f).gliding && !falling {
        giant_w(w, f).gliding = false;
        reset_jump_counts(w, f);
        hover_changed(w, f);
    } else {
        let g = giant(w, f);
        let hover = g.can_attack_hover;
        let (force, ignore_fuel) = (g.attack_force_hover, g.hover_ignore_fuel);
        let fuel = g.hover_rage.as_ref().expect("the hover rage").fuel;
        let max_hover = data_of(w, f).def.max_hover_speed.expect("an Iron Giant's MaxHoverSpeed");
        let fm = w.fighter(f);
        let want = !in_hitstun(w, f)
            && (fm.controls.inp.jump || (force && falling))
            && (falling || hover)
            && !fm.moves.cmd_jumped
            && !fm.wall.wall_sliding
            && (!fm.evade.evading || hover || fm.evade.block_evade_montage)
            && (hover || !is_attacking(w, f))
            && !(fm.body.vy > crate::math::narrow(max_hover + 200.0))
            && (ignore_fuel || fuel > 0.0);
        if want != giant(w, f).gliding {
            giant_w(w, f).gliding = want;
            hover_changed(w, f);
        }
    }
    // @3665, the first flow popped: SetRTPCValue(cha_C017_hover_fuel, FuelAmount, 0 ms, self) every tick. Posted when
    // the amount changed (the same value again changes nothing in the sound engine).
    let hr = giant_w(w, f).hover_rage.as_mut().expect("the hover rage");
    if hr.fuel_rtpc != hr.fuel {
        hr.fuel_rtpc = hr.fuel;
        let value = hr.fuel;
        let id = w.fighter(f).id;
        w.fighter_mut(f).events.push(Event::Rtpc {
            fighter: id,
            name: name!("cha_C017_hover_fuel"),
            value,
            interp_ms: 0.0,
        });
    }
    let fm = w.fighter(f);
    if !giant(w, f).gliding && fm.moves.gravity_mod_count == 0 && fm.moves.gravity_scale != GRAVITY_SCALE {
        w.fighter_mut(f).moves.net_gravity = Some(GRAVITY_SCALE);
    }
    let fm = w.fighter(f);
    if fm.jump.jumps > 0 && grounded(w, f) && !fm.moves.cmd_jumped {
        reset_jump_counts(w, f);
    }
}

/// The change of isGliding (@3042): BlockJumpEndLaunch = isGliding; AssignHoverValues (@26976): air control 1 or
/// BaseAirControl, the terminal velocity MaxHoverSpeed or DefaultTerminalVelocity, MaxWalkSpeed (see `max_walk`).
/// Into the hover (@6333): fast fall blocked and cleared, NetGravity(-4) (the thrust: with NewFallVelocity's cap the
/// rise settles at MaxHoverSpeed), the component's UsingFuel, and ThrusterLaunchSelf: LaunchCharacter((0,
/// Velocity.Y clamped to +-MaxWalkSpeed, Z), XY and Z override) with Z = 400 when walking, else Velocity.Z clamped to
/// 800 above and to -2000 below (a fall faster than 2000 gives FClamp a min over its max, which returns the min, 1).
/// MaxWalkSpeed is read after AssignHoverValues, so it is the hover's. The stock launch is taken by the next CMC
/// tick (HandlePendingLaunch).
/// Out of it (@6853): the default gravity scale unless a modifier holds it, UsingFuel off, the fast fall free,
/// sfx_cha_C017_foley_jump_double_end.
/// The anim instance's IsGliding (BPI_SetIsGliding @2518 / @3173) is `gliding` itself: hoverMachine (`fighter/anim`).
/// BPI_IdleFade(isGliding) (@2374) is BaseFighter's empty event (ubergraph @96958) and Pawn_C017 has no override.
///
/// Into the hover, AssignHoverValues @27368 -> @21641: StopAnimMontage(WallJumpAnim), then @1163 CurrentJumpNumber = 1
/// and, below a fall of 1000, LaunchCharacter((X, Y, -1000), XY and Z override) @1514; ThrusterLaunchSelf's launch
/// (@6829, both overrides) comes after it in the same frame and replaces the pending launch, so only the thruster's
/// is made. The same stop again in SetHoverValueMulticast (@22188 -> @2574) and in the falling DoOnce @7132 (with
/// PF_Post_Event(null) @7297, which posts nothing) finds the wall jump stopped already.
/// The foot rockets' visibility is render/chars/iron-giant/.
fn hover_changed(w: &mut World, f: FighterId) {
    let g = giant(w, f).gliding;
    let max_hover = data_of(w, f).def.max_hover_speed.expect("an Iron Giant's MaxHoverSpeed");
    w.fighter_mut(f).jump.block_jump_end_launch = g;
    update_air_control(w, f, if g { 1.0 } else { AIR_CONTROL });
    let terminal = if g { max_hover } else { f64::from(default_terminal(data_of(w, f))) };
    set_terminal(w, f, false, crate::math::narrow(terminal), false);
    if g {
        stop_wall_jump_anim(w, f);
        w.fighter_mut(f).jump.jumps = 1;
    }
    giant_w(w, f).hover_fast_fall_block = g;
    giant_w(w, f).hover_rage.as_mut().expect("the hover rage").using_fuel = g;
    if g {
        w.fighter_mut(f).fast_fall.fast_falling = false;
        fast_fall_end(w, f); // IsFastFalling = false @6362 (the FastFallLanded broadcast's)
        w.fighter_mut(f).fast_fall.ff_triggered = false;
        w.fighter_mut(f).moves.net_gravity = Some(HOVER_GRAVITY_SCALE);
        let walk = max_walk(w, f);
        let fm = w.fighter(f);
        let z = if grounded(w, f) {
            400.0
        } else if fm.body.vy < -2000.0 {
            1.0
        } else {
            min(fm.body.vy, 800.0)
        };
        let vx = min(max(fm.body.vx, -walk), walk);
        w.fighter_mut(f).moves.impulse = Some(Impulse { vx, vy: z, xy: true, z: true, kind: 0, d: None });
    } else {
        if w.fighter(f).moves.gravity_mod_count == 0 {
            w.fighter_mut(f).moves.net_gravity = Some(GRAVITY_SCALE);
        }
        sound(w, f, name!("sfx_cha_C017_foley_jump_double_end"));
    }
}

/// A sound at the fighter (`f.events.push({ type: 'sound', fighter: f.id, ev })`).
fn sound(w: &mut World, f: FighterId, ev: Name) {
    let id = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::Sound {
        fighter: id,
        ev,
        x: None,
        y: None,
        hit: None,
        pooled: false,
        key: None,
    });
}

/// Pawn_C017's death (@22296, after DeathMulticast): SetAlternateModeVisuals(false) @22306, IsMaxRage off,
/// Montage_Stop(0.0, null) @22400 (every instance of every group; the DefaultGroup ones are stopped by the world's
/// death already), then the reset's flow @21829: isGliding = false and ResetHover.
fn pawn_death(w: &mut World, f: FighterId) {
    set_alternate_mode(w, f, false);
    stop_grouped(w, f, None, 0.0);
    let g = giant_w(w, f);
    g.gliding = false;
    g.is_max_rage = false;
    hover_changed(w, f);
}

/// Pawn_C017 SetAlternateModeVisuals(AlternateMode) (@7590), weapon mode on and off. On: IsMaxRage off, AlternateMode,
/// CrouchLocked, BlockTauntAnims, the component's UsingWeaponMode (the rage drains over RageTime), WeaponMeshTop and
/// WeaponMesh shown, BodyMesh hidden, CheckForAnimInterrupt unbound. Off: the flags back, BodyMesh shown and the
/// three weapon meshes hidden, Montage_Stop(0) on each of WeaponAttackMontages, isGliding false through
/// SetHoverValueMulticast (the flag alone: the hover's other values stay until its next change), ResetRage(false),
/// OutOfRageInturupted unbound.
/// On (@7694-7822): Comp_Armor.AddTempHealthPack(WeaponMode, WeaponModeHealth 15, Time 0, CanOverride false); off
/// (@9698): RemoveTempHealthPack(WeaponMode).
/// Items: on, Comp_ItemManager.BlockPickUp = true (@8390) and MulticastOverridePack(false) (@8434, the pickup pack
/// off); off, BlockPickUp = false (@9369) and TrackReachableItems(null, false) (@9402), which re-picks the closest and
/// does not put the pack back (`world::items::track_reachable`).
/// Deviation: that call is served at the world's next reach update, not inside this one.
/// Off, @8946-9348: Montage_Stop(0.0, m) for each of WeaponAttackMontages (the four weapon attacks and
/// C017_Weapon_Turn_Montage), all instances of the other slot groups (`stop_grouped`).
/// Comp_IronGiant_Ally_Turret.ToggleAllyTurret(true) @8471 / (false) @9440: turret.rs.
/// WeaponAimTimer is cleared here (@9677) and set nowhere (Pawn_C017 has no other statement with it): nothing to port.
/// Deviation, not ported: WeaponModeGlowVisuals, the AnimBP's alternate graph. BPI_TurretAim(false) (@8898) writes an
/// AnimBP variable only.
pub fn set_alternate_mode(w: &mut World, f: FighterId, on: bool) {
    w.fighter_mut(f).controls.block_taunt_anims = on;
    if on {
        giant_w(w, f).is_max_rage = false;
    }
    giant_w(w, f).alternate_mode = on;
    w.fighter_mut(f).crouch.crouch_locked = on;
    giant_w(w, f).hover_rage.as_mut().expect("the hover rage").using_weapon_mode = on;
    set_sub_mesh(w, f, name!("WeaponMeshTop"), on);
    set_sub_mesh(w, f, name!("WeaponMesh"), on);
    set_sub_mesh(w, f, name!("BodyMesh"), !on);
    if on {
        let health = num(&data_of(w, f).fighter.pawn, "WeaponModeHealth").unwrap_or(0.0);
        add_temp_health_pack(w, f, name!("WeaponMode"), health, false, 0.0);
        let items = &mut w.fighter_mut(f).items;
        items.block_pick_up = true;
        items.item_reachable = false;
        toggle_ally_turret(w, f, true);
        return;
    }
    remove_temp_health_pack(w, f, name!("WeaponMode"));
    let items = &mut w.fighter_mut(f).items;
    items.block_pick_up = false;
    items.reach_recheck = true;
    toggle_ally_turret(w, f, false);
    set_sub_mesh(w, f, name!("WeaponMeshReverse"), false);
    let stop: Vec<Name> = arr(&data_of(w, f).fighter.pawn, "WeaponAttackMontages")
        .iter()
        .filter_map(Value::as_str)
        .map(|s| data_name(w, s))
        .collect();
    for m in stop {
        stop_grouped(w, f, Some(m), 0.0); // @9232
    }
    giant_w(w, f).gliding = false;
    hover_rage::reset_rage(w, f, false);
    giant_w(w, f).out_of_rage_bound = false;
}

/// The component's OutOfRage (@14020), in weapon mode only: Montage_Play(C017_WeaponMode_Out_Montage) unless the
/// active montage is one of NonInturuptMontages (C017_CannonBall_Air_Montage), then OutOfRageInturupted is bound to
/// OnMontageEnded (@20867): the end of the Out or the air cannonball montage, interrupted or not, leaves the mode.
/// The Out montage's own WeaponModeVisuals state (0 to 0.659 s) does it first when it runs out.
// Deviation: OnMontageEnded comes when the instance is cleared (after its blend-out); taken at the stop.
// PlayLeaveWeaponOnLand (@14221) is never added to OnBFLanded in the content (only removed, @2030).
pub fn out_of_rage(w: &mut World, f: FighterId) {
    if !giant(w, f).alternate_mode {
        return;
    }
    let cur = active_montage_name(w, f); // GetCurrentActiveMontage @14072: the newest not stopped instance of any group
    let keep = cur.is_some_and(|c| {
        arr(&data_of(w, f).fighter.pawn, "NonInturuptMontages")
            .iter()
            .filter_map(Value::as_str)
            .any(|s| w.names.get(s) == Some(c))
    });
    if !keep {
        start_montage(w, f, name!("C017_WeaponMode_Out_Montage"), &PROXY_NONE, None);
    }
    giant_w(w, f).out_of_rage_bound = true;
}

// ---- the presses

/// Pawn_C017 NormalInputeMulticast (@17718) / SpecialInputeMulticast (@16475), after the parent's press event: not dead
/// (nor polymorphed, IntroOver), then by the press event's Direction parameter. The switch is over
/// Enum_InputDirection8Way with no diagonal case, but a player's press never carries one: PFGPlayerController passes
/// BaseFighter.GetDirection(X, Y) (@38669-38732, @151280-151325 through GetInputDirection), which is Neutral at
/// (0, 0), Up / Down when |Y| >= |X|, else Forward / Back. So a diagonal stick is Up or Down here (the stick held to
/// the side, Up + Special, plays the discharge). Special: Up -> UseEnergyDischarge in weapon mode out of hitstun (and
/// without Buff_StevenBubbleEnemy, @17278 / @17558); Down -> UseCannon(false) when also IsMovingOnGround; Forward /
/// Back -> UsePlasma(false); Neutral -> EyeLaser, the bolt path (C017_AntenaBolts, no weapon mode gate).
/// Normal: Forward / Back -> UseDisk in weapon mode on the ground out of hitstun (Neutral instead with the fighter's
/// SwappedAttackStyleGround, `prefs`: @17998 / @18399, read whether in the air or not; the ground test is UseDisk's
/// own, @18218). Each plays its montage with Montage_Play(m, 1, 0, 0, true) over whatever runs:
/// PlayMontageServer -> PlayMontageMulticast (@13818-13884; UseDisk calls the multicast itself, @13778), past the
/// branch manager's attack start. The montages are of the slot groups WeaponMode, WeaponMode2 and DiscGroup, so the
/// play stops its own group's instances only and the instance is an attack instance beside the DefaultGroup montage
/// (`play_grouped`): no attack count, no BeginAttack, no charge reset. The gates are Montage_IsActive per montage
/// (`montage_is_active`).
/// What refuses a press before these tests: nothing. The controller calls the press event whatever the pawn's state,
/// the override (@17650 / @16388) calls the parent's event and then NormalInputeServer / SpecialInputeServer with no
/// test between, and the parent's own tests (hitstun, wall slide, attack / input enabled, dead: BaseFighter
/// ubergraph 117474-117876, 117925-118337) are inside its event. So a hold, an emote, the manager's CurrentMontage
/// or a hit pause refuse nothing here: under his own hit pause a weapon attack starts, and in hitstun the Neutral
/// special still reaches EyeLaser (no hitstun test on that path, @16845 -> @16432 -> @15603). `Actions.pawnPresses`
/// brings the presses of the frames `actions` does not run.
// Unverified: the parent's press event runs before this: an attack the parent starts in the same press counts its id
// first, and the weapon attack's notifies carry that id.
/// The gate @16475 / @17718: !Is Polymorphed && IntroOver && !isDead (Is Polymorphed: `shapes.is_polymorphed`,
/// `combat::polymorph`). IntroOver is not tested: no press reaches a fighter before the intro's end.
fn weapon_input(w: &mut World, f: FighterId, b: AttackBtn) {
    if w.fighter(f).life.dead > 0.0 || w.fighter(f).shapes.is_polymorphed {
        return;
    }
    let d = press_dir(w, f);
    if b == AttackBtn::Special && d == Dir::Neutral {
        // @16845: at max rage while Walking the selection is the parent's; else EyeLaser, whatever the mode.
        if !(giant(w, f).is_max_rage && grounded(w, f)) {
            super::actors::eye_laser(w, f);
        }
        return;
    }
    if !giant(w, f).alternate_mode || in_hitstun(w, f) {
        return;
    }
    let side = d == Dir::Fwd || d == Dir::Back;
    if b == AttackBtn::Attack {
        // UseDisk (@13492): not while one of the three disc montages is active (@13534-13777).
        // NormalInputeMulticast @17998 / @18399: the disc's direction, swapped by the fighter's SwappedAttackStyleGround.
        let disc = if w.fighter(f).controls.prefs.swapped_ground { d == Dir::Neutral } else { side };
        let busy = [
            name!("C017_WeaponMode_EnergyDisc_U_Montage"),
            name!("C017_WeaponMode_EnergyDisc_Montage"),
            name!("C017_WeaponMode_EnergyDisc_D_Montage"),
        ]
        .iter()
        .any(|&m| montage_is_active(w, f, m));
        if disc && grounded(w, f) && !busy {
            play_grouped(w, f, name!("C017_WeaponMode_EnergyDisc_Montage"), true, true);
        }
    } else if d == Dir::Up {
        // @17278 / @17558: not with Buff_StevenBubbleEnemy on him (Up only; the bubble's stun covers 0.5 s of its 5 s).
        if buff_of(w, f, name!("Buff_StevenBubbleEnemy_C")).is_some() {
            return;
        }
        // UseEnergyDischarge (@18654): the gates @18696-18850, then TriggerAbilityCheck(4) (@18851).
        let busy = [name!("C017_CannonBall_Air_Montage"), name!("C017_WeaponMode_EnergyDischarge_Montage")]
            .iter()
            .any(|&m| montage_is_active(w, f, m));
        if !busy && trigger_ability_check(w, f, 4) {
            play_grouped(w, f, name!("C017_WeaponMode_EnergyDischarge_Montage"), true, true);
        }
    } else if d == Dir::Down {
        // UseCannon(false) (@12638): the gates @12680-12834, then TriggerAbilityCheck(2) (@12835).
        let busy =
            [name!("C017_WeaponMode_CannonAttack_Normal_Montage"), name!("C017_WeaponMode_CannonAttack_Montage")]
                .iter()
                .any(|&m| montage_is_active(w, f, m));
        if grounded(w, f) && !busy && trigger_ability_check(w, f, 2) {
            play_grouped(w, f, name!("C017_WeaponMode_CannonAttack_Montage"), true, true);
        }
    } else if side {
        // UsePlasma(false) (@13021): the gates @13063-13306, then TriggerAbilityCheck(3) (@13307).
        let busy = [
            name!("C017_CannonBall_Air_Montage"),
            name!("C017_WeaponMode_PlasmaAttack_Montage"),
            name!("C017_WeaponMode_PlasmaAttack_Normal_Montage"),
        ]
        .iter()
        .any(|&m| montage_is_active(w, f, m));
        if !busy && trigger_ability_check(w, f, 3) {
            play_grouped(w, f, name!("C017_WeaponMode_PlasmaAttack_Montage"), true, true);
        }
    }
}

fn montage_ended(w: &mut World, f: FighterId, m: Name) {
    if giant(w, f).out_of_rage_bound
        && (m == name!("C017_CannonBall_Air_Montage") || m == name!("C017_WeaponMode_Out_Montage"))
    {
        set_alternate_mode(w, f, false);
    }
}

/// AnimNotify_C017_CanHover: CanAttackHover and BindHoverJump (GroundHoverJump on InputPreTriggered, LandLaunchHover
/// on OnBFLanded). GroundHoverJump (@19491), on a Jump press (input type 2): walking with fuel, NetImpulse((0, 0,
/// 600), Z override, type 0); else, unless gliding, NetImpulse Z 100. LandLaunchHover (@19896): with fuel and
/// gliding, NetImpulse Z 100. The anim instance's HoverSpinAttack is not ported.
fn hover_jump_press(w: &mut World, f: FighterId) {
    let g = giant(w, f);
    if !g.can_attack_hover {
        return;
    }
    let fuel = g.hover_rage.as_ref().expect("the hover rage").fuel;
    let gliding = g.gliding;
    if grounded(w, f) && fuel > 0.0 {
        net_impulse(w, f, 0.0, 600.0, false, true, 0.0, false, 0.0);
    } else if !gliding {
        net_impulse(w, f, 0.0, 100.0, false, true, 0.0, false, 0.0);
    }
}

// ---- the character's hooks

/// Comp_C017_Hover_Rage and what it calls on its owner (`init`).
fn init(w: &mut World, f: FighterId) {
    let g = giant_w(w, f);
    g.giant_anim = Some(GiantAnim::default());
    g.hover_rage = Some(HoverRage::default());
    g.ally_rage_delay = latent_frames(2.0);
}

/// Pawn_C017 ReceiveTick (@7485): the parent's tick, then ReadHoverInput.
fn tick(w: &mut World, f: FighterId) {
    let g = giant_w(w, f);
    if g.ally_rage_delay > 0.0 {
        g.ally_rage_delay -= 1.0;
    }
    read_hover_input(w, f);
    torso_flip(w, f);
}

/// Comp_C017_Hover_Rage ReceiveBeginPlay: Delay(2.0) (@5901), then for each of the owner's
/// Comp_Actor_Destructible.GetAllies (@440-1183) the ally's Comp_Pawn_DamageManager.HitByEnemy -> AllyHitRageBuild
/// (@851-874; @7108: AddRage(FClamp(OnAllyHitRageAddedMult x Hit.Damage + CurrentRageAmount, 0, MaxRageAmount))) and
/// its OnExitedArena -> OnAllyDeathRage (@1049-1072; @7356: OnAllyDeathRagePecent x MaxRageAmount). Bound once and
/// never unbound (no EndPlay in the graph): a hit on the ally counts whoever dealt it and whatever the Giant's own
/// state (waiting for his respawn too); AddRage itself ignores it in weapon mode (@6940).
// Unverified: Allies is taken as every other fighter of the team (the list at BeginPlay + 2 s: Comp_Actor_Destructible
// fills it one tick after its own BeginPlay from the players whose pawn exists, and on the game mode's respawn /
// knock-out delegates @679-766, whose first broadcast against the spawn order is not read); the Delay is counted in
// the pawn's ticks from the fighter's making, hit pause or not.
// A minion is no player's pawn: it is not on the list GetAllies gives (the players whose pawn
// exists, read once at BeginPlay + 2 s), whenever it spawned.
// Unverified: every ally is bound, so in the port's own big matches the rage builds from two or three allies' hits
// taken and ring-outs, faster than the beta's 2v2 ever let it; kept as the build's rule.
fn ally_hit_by_enemy(w: &mut World, f: FighterId, ally: FighterId, damage: Real) {
    if giant(w, f).ally_rage_delay <= 0.0 && !w.fighter(ally).life.is_minion {
        hover_rage::ally_hit(w, f, damage);
    }
}
fn ally_exited_arena(w: &mut World, f: FighterId, ally: FighterId) {
    if giant(w, f).ally_rage_delay <= 0.0 && !w.fighter(ally).life.is_minion {
        hover_rage::ally_death(w, f);
    }
}

/// Pawn_C017 ResetFighterMulticast @21829 and the component's OnOwnerReset (fuel full, ResetRage(true)).
fn reset(w: &mut World, f: FighterId) {
    let g = giant_w(w, f);
    g.gliding = false;
    g.is_max_rage = false;
    hover_changed(w, f); // ResetHover (@21862 -> 24206 -> 3042)
    hover_rage::owner_reset(w, f);
    ally_turret_reset(w, f);
}

/// A match reset spawns a new pawn (the World's reset calls this behind the fighter's spawn, ahead of the perks'
/// possession): a new Comp_C017_Hover_Rage, RespawnRage 0 and no rage, so Buff_C017_FastWeaponMode's
/// SetRespawnRage(25) adds its 25 again (perks.rs), and its ReceiveBeginPlay Delay(2.0) ahead of the ally bindings
/// runs again.
fn match_reset(w: &mut World, f: FighterId) {
    let g = giant_w(w, f);
    g.hover_rage = Some(HoverRage::default());
    g.ally_rage_delay = latent_frames(2.0);
}

/// The hooks of this file in Iron Giant's CharHooks.
pub fn hooks(h: &mut CharHooks) {
    h.init = Some(init);
    // JumpPressed also broadcasts InputPreTriggered(Jump) (@122732); the pawn binds its GroundHoverJump to it.
    h.jump_pressed = Some(hover_jump_press);
    h.tick = Some(tick);
    // NormalAttackPressed / SpecialAttackPressed overrides: the parent event, then their own handling.
    h.pressed = Some(weapon_input);
    h.montage_ended = Some(montage_ended);
    h.knocked_back = Some(on_knock_back);
    h.ally_hit_by_enemy = Some(ally_hit_by_enemy);
    h.ally_exited_arena = Some(ally_exited_arena);
    h.death = Some(pawn_death);
    h.reset = Some(reset);
    h.match_reset = Some(match_reset);
}

/// This file's registry entries (`mod.rs`'s conditions and notifies, in registration order).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    for (name, f) in [
        ("C017_Fuel_Check_Condition_C", fuel_check as CondFn),
        ("C017_Fuel_Check_False_Condition_C", fuel_check_false),
        ("C017_Rage_Condition_C", rage),
        ("C017_Rage_False_Condition_C", rage_false),
        ("C017_WeaponMode_Condition_C", weapon_mode),
        ("C017_Not_WeaponMode_Condition_C", not_weapon_mode),
        ("C017_HoldingEnemy_Condition_C", holding_enemy),
        ("C017_NotHoldingEnemy_Condition_C", not_holding_enemy),
        ("C017_NotHover_Condition_C", not_hover),
        ("C017_HasPerk_GrayHealth_C", has_gray_health),
        ("C017_Not_HasPerk_GrayHealth_C", not_has_gray_health),
    ] {
        r.conditions.enter(name, Condition::Fn(f))?;
    }
    type Fns = Option<fn(&mut World, FighterId, NotifyRef, i32)>;
    let be = |begin: Fns, end: Fns| NotifyHandler { begin, end, ..NotifyHandler::default() };
    let n = &mut r.notifies;
    n.enter("AnimNotify_C017_CanHover_C", be(Some(can_hover_begin), Some(can_hover_end)))?;
    n.enter("AnimNotify_C017_ForceHover_C", be(Some(force_hover_begin), Some(force_hover_end)))?;
    n.enter("AnimNotify_C017_EvadeTempHealth_C", be(Some(evade_temp_health_begin), Some(evade_temp_health_end)))?;
    n.enter("AnimNotify_C017_Use_Fuel_C", be(Some(use_fuel_begin), Some(use_fuel_end)))?;
    n.enter("AnimNotify_C017_WeaponModeVisuals_C", be(None, Some(weapon_mode_visuals_end)))?;
    n.enter("AnimNotify_C017_IsBlocking_C", be(Some(is_blocking_begin), Some(is_blocking_end)))?;
    n.enter("AnimNotify_C017_TurretAim_C", be(Some(turret_aim_begin), Some(turret_aim_end)))?;
    // AnimNotify_C017_SetHurtBox: BPI_SetHurtbox('Box Number') at the begin, (0) at the end (@121 of each). Pawn_C017
    // @21805 passes it to SetHurtBoxMulticast, whose event is empty (@21804): no hurtbox changes.
    n.enter("AnimNotify_C017_SetHurtBox_C", be(None, None))?;
    n.enter("AnimNotify_GiantIsCrouched_C", be(Some(giant_crouched_begin), Some(giant_crouched_end)))?;
    n.enter("AnimNotify_C017_MovementSplit_C", be(Some(movement_split_begin), Some(movement_split_end)))?;
    n.enter("AnimNotify_C017_MovementSplitJumpCheck_C", be(Some(jump_check_begin), Some(jump_check_end)))?;
    n.enter("AnimNotify_C017_Hold_Overide_C", be(Some(hold_override_begin), None))?;
    n.enter("AnimNotify_C017_GrabDrop_C", be(Some(grab_drop_begin), None))?;
    n.enter("AnimNotify_C017_GrabThrow_C", be(Some(grab_throw_begin), None))?;
    n.enter("AnimNotify_Hitframe_C017_Throw_C", be(Some(hitframe_throw_begin), Some(hitframe_throw_end)))?;
    n.enter("AnimNotify_C017_TenticleOveride_C", be(Some(tenticle_override_begin), None))?;
    Ok(())
}
