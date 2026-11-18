//! The pawn core of Pawn_Jake: BeginPlay's charge binds, ReceiveTick's ReachCheck
//! and charge value, ChargeStarted / ChargeEnd, Cleanup; the conditions JakeWeaponFist / JakeWeaponHammer,
//! JakeEat_Hold / JakeEat_NotHold and JakeHasBouncyHousePerk; Jake_AnimBP's setters the other files call; its hooks
//! (`pawnHooks`: the slot mix and pose ones are anim.rs's).
//!
//! Pawn_Jake's own logic (offsets of its ubergraph), the conditions of his pawn and Jake_AnimBP's sim slot mix
//! (anim.rs).
//!
//! What is here: BeginPlay @2464 (the AnimBP and SkinManager refs, Delay 0 @2663; the Delay 0.5 @15 and the bindings
//! @70-157, whose resume offsets latent_links.js gives; the S01 intro swap @238-308 is a skin's), ReceiveTick
//! @1985-2417 in its order (the ReachCheck section test @2004-2181, then GetChargeServer @2192 -> GetChargeMulticast
//! @2231-2276 -> BPI_SetChargeValue @2417), ChargeStarted @2207 / ChargeEnd @2219, Cleanup @2718, HeadScale
//! @2741-2848, BPI_Use2DHitboxes @4430; the conditions JakeWeaponFist / JakeWeaponHammer / JakeEat_Hold /
//! JakeEat_NotHold / JakeHasBouncyHousePerk (below); Jake_AnimBP's sim slot mix (anim.rs) through `slot_mix` /
//! `slot_input`; the Bounce Check as ignoredOk.
//! Classes: Pawn_Jake (all but what the other files name), Jake_AnimBP (its sim part).
//! Hooks: [`hooks`] below, entered by mod.rs as they are (any hook of mod.rs's owned list but state, and any joinable
//! hook: matchReset runs behind mod.rs's new state; tick is the pawn's ReceiveTick). Its state: `JakePawnState`
//! (`jake(w, f).pawn`) and the shared fields of `JakeState` (anim, reach_check, head_scale).
//!
//! Not here: the AI (AssignDefaultBehaviors @2898-3741, bots.rs), AddVFX @3814-4429 (the view: PS_JakeWeaponNAU's
//! Size by the charge; its PrintString @4368 is debug), ValidateCustomJump (offline unreached).
//! Not either: BPI_Use2DHitboxes @4430 (a debug toggle of the hit box drawing), the BPI getters
//! (BPI_GetFighterCharacter 9: `combat::disguise` EFIGHTER), Cleanup's isGliding @2729 (read by nothing), BeginPlay's
//! Jake Anim BP and Comp_Pawn_SkinManager refs @2596 / @2644 (the port's `jake`), the Delay 0 @2663 -> @199 (the S01
//! intro swap: a skin's, not the shipped default's), the Bounce Check @328-1984 (its box's bGenerateOverlapEvents is
//! false, so the bound overlap never fires).

use super::anim;
use super::state::{jake, jake_of, jake_w};
use crate::Real;
use crate::combat::buffs::buff_of;
use crate::consts::DELTA_SECONDS;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::anim::SlotMix;
use crate::fighter::anim::grouped::active_instance;
use crate::fighter::anim::instance_section;
use crate::fighter::anim::update::nm;
use crate::fighter::control::notify_def;
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::props::text;
use crate::registry::{CharHooks, Condition, Registry, RegistryError};
use crate::world::World;
use mvs_anim::montage::same_name;

// ---------------------------------------------------------------- ReceiveTick and the charge
// ReceiveTick @1985-2206 (after the parent's), whatever the state: while ReachCheck, the anim instance's
// Montage_GetCurrentSection(null) (the active instance's section at its position, None without one) against
// Comp_Pawn_ChargeAttackManager.MontageSectionOverride @2014-2167 (equal: ReachCheck = false @2181), then
// GetChargeServer @2192 -> @2231-2276 -> GetChargeMulticast -> @2300-2417: BPI_SetChargeValue(the component's
// GetUpdatedPercentCharged) on the mesh's anim instance (Jake_AnimBP @8846: ChargeAmount = it).
// GetUpdatedPercentCharged @0-70 = NewMultiplier(2.0) - 1.0; NewMultiplier @0-418: TotalChargedTime = now -
// ChargeStartTime (@98: the fighter's total_charged_time, which the Hitframe's hit pause reads), (2 - 1) x FMin((now -
// ChargeStartTime) / ChargeTotalTime, 1) + 1, float32 (chars/rick/pawn.rs charge_end's formula).
// Then the actor's latent actions (AActor::Tick after ReceiveTick, on the world's delta):
// BeginPlay's Delay 0.5 @15, whose resume @70-157 binds ChargeStarted / ChargeEnd. A respawn keeps the pawn,
// so it runs once per pawn (mod.rs match_reset's new state).

/// `sectionNow`: the active instance's current section name, `None` without one.
fn section_now(w: &World, f: FighterId) -> Name {
    let sec = active_instance(w, f).and_then(|m| instance_section(w, f, m)).map_or(Name::NONE, |(_, s)| s);
    if sec == Name::NONE { name!("None") } else { sec }
}

/// `getCharge`.
fn get_charge(w: &mut World, f: FighterId) {
    let fi = w.fighter(f);
    let el = fi.clock.time - fi.charge.charge_start_time;
    let total = jake(w, f).pawn.charge_total_time;
    let m = (1.0 * min(el / max(total, 1e-6), 1.0)) + 1.0;
    w.fighter_mut(f).charge.total_charged_time = el;
    jake_w(w, f).anim.charge_amount = m - 1.0;
}

/// `tick` (CharHooks.tick: the pawn's ReceiveTick).
fn tick(w: &mut World, f: FighterId) {
    if jake(w, f).reach_check {
        let now = section_now(w, f);
        let over = jake(w, f).pawn.section_override;
        if same_name(w.names.str(now), w.names.str(over)) {
            jake_w(w, f).reach_check = false;
        }
        get_charge(w, f);
    }
    let p = &mut jake_w(w, f).pawn;
    if !p.charge_bound {
        p.bind_delay -= DELTA_SECONDS;
        if p.bind_delay <= 0.0 {
            p.charge_bound = true;
        }
    }
}

/// Comp_Pawn_ChargeAttack's ChargeStarted broadcast (AnimNotify_ChargingAttack's begin, CharHooks.chargeStarted): the
/// component stores the notify's MontageSectionOveride and ChargeTotalTime whatever is bound
/// (StartAttackChargeMultipliersServer); the pawn's ChargeStarted @2207 (ReachCheck = true) once the Delay bound it.
fn charge_started(w: &mut World, f: FighterId, n: NotifyRef) {
    let data = w.data.clone();
    let nd = notify_def(&data, w, f, n);
    let sec = nd.and_then(|x| text(&x.p, "MontageSectionOveride")).filter(|s| !s.is_empty());
    let section = match sec {
        Some(s) => nm(&w.names, s),
        None => name!("None"),
    };
    // the component's (`start_charge_totals`: n is the one begun)
    let total = w.fighter(f).charge.charge_total_time.unwrap_or_else(|| nd.map_or(0.0, |x| crate::math::narrow(x.dur)));
    let j = jake_w(w, f);
    j.pawn.section_override = section;
    j.pawn.charge_total_time = total;
    if j.pawn.charge_bound {
        j.reach_check = true;
    }
}

/// pawn.rs's part of the joined chargeEnd (ChargeEnd @2219: ReachCheck = false, once bound), before aim.rs's.
pub fn charge_end(w: &mut World, f: FighterId, n: NotifyRef) {
    let _ = n;
    let j = jake_w(w, f);
    if j.pawn.charge_bound {
        j.reach_check = false;
    }
}

fn slot_mix(w: &World, f: FighterId) -> Option<&SlotMix> {
    Some(&jake(w, f).pawn.slot_mix)
}

fn slot_mix_mut(w: &mut World, f: FighterId) -> Option<&mut SlotMix> {
    Some(&mut jake_w(w, f).pawn.slot_mix)
}

/// Jake_AnimBP OnMontageStarted_Event_0 @8713 (bound to the instance's OnMontageStarted): ChargeAmount = 0.
fn montage_started(w: &mut World, f: FighterId, _montage: Name) {
    jake_w(w, f).anim.charge_amount = 0.0;
}

/// BPI_Cleanup @2718 (ResetFighterLogic's, inside the respawn's reset): ReachCheck = false (isGliding @2729: unread).
fn reset(w: &mut World, f: FighterId) {
    jake_w(w, f).reach_check = false;
}

/// `death`: the port does not update a dead pawn's AnimBP.
fn death(w: &mut World, f: FighterId) {
    jake_w(w, f).pawn.slot_mix.slot_mix.clear();
}

/// The hooks of this file in Jake's CharHooks (`pawnHooks`: `slot_input`, `slot_mix`, `pose`, `layer_held`,
/// `anim_update`, `pose_updated` are anim.rs's; `tick`, `charge_started`, `montage_started`, `reset`, `death`; the
/// joined `charge_end` is mod.rs's).
pub fn hooks(h: &mut CharHooks) {
    h.slot_input = Some(anim::slot_input);
    h.slot_mix = Some(slot_mix);
    h.slot_mix_mut = Some(slot_mix_mut);
    h.pose = Some(anim::pose_shapes);
    h.layer_held = Some(anim::layer_held);
    h.anim_update = Some(anim::anim_update);
    h.pose_updated = Some(anim::origins_stand_in);
    h.tick = Some(tick);
    h.charge_started = Some(charge_started);
    h.montage_started = Some(montage_started);
    h.reset = Some(reset);
    h.death = Some(death);
}

// ---------------------------------------------------------------- the conditions
// JakeWeaponFistCondition / JakeWeaponHammerCondition: the actor's Comp_WeaponSwap.WeaponEquiped, negated for the
// fist @51; JakeEat_HoldCondition / JakeEat_NotHoldCondition: its Comp_JakeEat.HoldingActor, negated for NotHold @51.
// GetComponentByClass on another pawn gives None, whose read is false (the access error is logged): Fist and NotHold
// true, Hammer and Hold false.
// JakeHasBouncyHousePerk @0-390: BPI_CheckIfHasBuff(Buff_Jake_BouncyHouse_C).hasBuff (Comp_State_Manager CheckHasBuff:
// a live buff of that class) on a fighter, false on another actor (every caller here is a fighter). IsBuffOfClass
// (BuffParentActor @152-228) is the class or a child; Buff_Jake_BouncyHouse_C has no child in the build, so the exact
// class.

/// JakeWeaponFistCondition_C: not the owner's Comp_WeaponSwap.WeaponEquiped (true on another pawn).
fn weapon_fist(w: &mut World, f: FighterId) -> bool {
    !jake_of(w, f).is_some_and(|j| j.weapon_equipped)
}

/// JakeWeaponHammerCondition_C: the owner's Comp_WeaponSwap.WeaponEquiped (false on another pawn).
fn weapon_hammer(w: &mut World, f: FighterId) -> bool {
    jake_of(w, f).is_some_and(|j| j.weapon_equipped)
}

/// JakeEat_HoldCondition_C: the owner's Comp_JakeEat.HoldingActor (false on another pawn).
fn eat_hold(w: &mut World, f: FighterId) -> bool {
    jake_of(w, f).is_some_and(|j| j.holding_actor)
}

/// JakeEat_NotHoldCondition_C: not the owner's Comp_JakeEat.HoldingActor (true on another pawn).
fn eat_not_hold(w: &mut World, f: FighterId) -> bool {
    !jake_of(w, f).is_some_and(|j| j.holding_actor)
}

/// JakeHasBouncyHousePerk_C @0-390: a live Buff_Jake_BouncyHouse_C on the fighter.
fn has_bouncy_house(w: &mut World, f: FighterId) -> bool {
    buff_of(w, f, name!("Buff_Jake_BouncyHouse_C")).is_some()
}

// ---- Jake_AnimBP's setters
// The BPI_Anim_BP / Jake_AnimBP events the other files call on Jake's anim instance (the cast to BPI_Anim_BP of the
// mesh's GetAnimInstance). Their bodies are the AnimBP events' own assignments (`JakeState::anim`); pawn.rs owns them
// and what reads them (anim.rs).

/// BPI_SetStretchOn(On) @8576. Caller: JakeStretch (stretch.rs).
pub fn set_stretch_on(w: &mut World, f: FighterId, on: bool) {
    jake_w(w, f).anim.stretch_on = on;
}

/// BPI_CalculateStretchAngle(True) @8596. Caller: JakeStretch (stretch.rs).
pub fn calculate_stretch_angle(w: &mut World, f: FighterId, on: bool) {
    jake_w(w, f).anim.calculate_stretch_angle = on;
}

/// BPI_SetAimDirection(AimDirection) @8544 -> @3055 (AimDirectionInverted = -AimDirection: the slot mix derives it).
/// Callers: JakeAimer, AnimNotify_ResetAimDirection (aim.rs), ProjectileAimer_SpitJake (spit.rs).
pub fn set_aim_direction(w: &mut World, f: FighterId, dir: Real) {
    jake_w(w, f).anim.aim_direction = dir;
}

/// BPI_SetAimDIstanceAlpha(AimDIstance) @8616. Caller: JakeAimer (aim.rs).
pub fn set_aim_distance_alpha(w: &mut World, f: FighterId, alpha: Real) {
    jake_w(w, f).anim.aim_distance_alpha = alpha;
}

/// BPI_HoldingFighter(HoldingFighter) @8644 (EatOverrideBool). Caller: Comp_JakeEat's AddHeldFightersVisuals @8183 /
/// RemoveHeldFighterVisuals @8801 (eat.rs).
pub fn holding_fighter(w: &mut World, f: FighterId, on: bool) {
    jake_w(w, f).anim.eat_override = on;
}

/// This file's registry entries (the five conditions).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.conditions.enter("JakeWeaponFistCondition_C", Condition::Fn(weapon_fist))?;
    r.conditions.enter("JakeWeaponHammerCondition_C", Condition::Fn(weapon_hammer))?;
    r.conditions.enter("JakeEat_HoldCondition_C", Condition::Fn(eat_hold))?;
    r.conditions.enter("JakeEat_NotHoldCondition_C", Condition::Fn(eat_not_hold))?;
    r.conditions.enter("JakeHasBouncyHousePerk_C", Condition::Fn(has_bouncy_house))
}
