//! The tornado: SetTornadoMovementMode, EndTornadoTimer, the ally hit and the
//! reflect, AnimNotify_C015TornadoMovement, the conditions, Buff_C015Tornado, and its hooks (hitAlly,
//! reflectedProjectile, its part of the joined timerTick).
//!
//! Taz's tornado (C015 side special, ground and air: C015_Side_Special_Ground_Montage2 / _Air_Montage2; no movement
//! mode of its own). Offsets are Pawn_C015's ubergraph's unless a class is named.
//! The montage carries the motion (its launches, SetGravity, the control windows, the MoveCommand; the shared code
//! plays them). This file holds what the pawn adds:
//! - AnimNotify_C015TornadoMovement (18.17..122.72 of both montages): Begin / End -> BPI_SetTornadoMovementMode(true /
//!   false) on a BPI_C015 owner.
//! - SetTornadoMovementMode @5441-6417: TornadoActive, the ability check of slot 4, Buff_C015Tornado on himself, the
//!   tornado timer UI and EndTornadoTimer (TornadoBaseDuration 1.5 ready / TornadoCooldownDuration 0.45 on
//!   cooldown); off: TriggerAbilityActual(4) for a ready tornado, the buff destroyed, the UI cleared. TornadoActive is
//!   cleared by the timer alone (EndTornadoTimer @17218 -> MulticastTornadoActive(false) @17234): the montage's
//!   Condition_C015NotTornadoing poll (39.84..122.90) then jumps to TornadoBuffer.
//! - OnAllyHit @14629-15600 (the pawn's own HitBoxHandler HitAlly, NotifyAllyOfHit @886) and MulticastHitAlly @16439
//!   (its HitAllyLight, @923 -> @12451): an ally hit by his tornado extends the timer once by
//!   TornadoHitAllyBonusDuration (1 s) and gets Buff_AllyTornadoed (AllyReflectBuffDuration 3 s); HitAlly = true.
//! - BPI_CheckForAllyInFront (OnlyAllyInFront_Condition) and BPI_ListenForAllyHit.
//! - OnProjectileReflectedServer / Multicast (@14605, @16691-16956: his handler's ReflectedProjectile @15462,
//!   CharHooks.reflected_projectile): a projectile his handler reflected flies level in his facing.
//!
//! Not here: HitAllyProjectile (@16409: an empty body), the TornadoReflect perk's branches (Buff_C015Tornado @2691,
//! OnAllyHit @15339-15564, Buff_AllyTornadoed: perks_reflect.rs, called from here), TornadoLastFrameFacingLeft
//! (@5522: no reader), AnimNotify_C015TornadoConsumeEat (legacy montage only; TornadoConsumingEat has no reader).

use super::perks_reflect::{ALLY_BUFF, TORNADO_BUFF, activate_ally_reflect, has_tornado_reflect};
use super::perks_reflect::{tornado_reflect_begin, tornado_reflect_end};
use super::state::{TazState, taz, taz_mut, taz_of, taz_w};
use crate::Real;
use crate::actors::ActorRef;
use crate::combat::buffs::{buff, end_buff, inst_view, spawn_buff};
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::reflect::pmc_active;
use crate::combat::{BuffClass, BuffRef};
use crate::consts::DELTA_SECONDS;
use crate::events::{NotifyRef, UiStateOp, UiStateParams};
use crate::fighter::common::{data_of, peers, warn_text};
use crate::fighter::control::{data_name, pawn_prop, trigger_ability, trigger_ability_check};
use crate::fighter::moves::set_max_accel;
use crate::fighter::traces::{OBJ_PAWN, TraceHit, sphere_trace_multi_for_objects};
use crate::fighter::{FighterId, actor_y, facing};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks, Condition, NotifyHandler};
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;
use mvs_data::value::Value;

/// The side special's ability slot (TriggerAbilityCheck(4) @5552, TriggerAbilityActual(4) @6096).
const SLOT: i32 = 4;

/// Pawn_C015 class defaults (taz.json pawn): TornadoBaseDuration 1.5, TornadoCooldownDuration 0.45,
/// TornadoHitAllyBonusDuration 1, AllyReflectBuffDuration 3.
fn pawn_num(w: &World, f: FighterId, k: &str) -> Real {
    match pawn_prop(w, f, k) {
        Some(v) => v,
        None => {
            warn_text(w, &format!("taz: Pawn_C015 class default {k} missing"));
            0.0
        }
    }
}

/// IsValid(TazTornadoBuff): the entry is one of his live buffs.
fn tornado_buff_live(w: &World, s: &TazState) -> Option<BuffRef> {
    s.tornado_buff.filter(|&b| buff(w, b).is_some_and(|x| x.frames > 0.0))
}

// ------------------------------------------------------------------ the tornado timer

/// K2_SetTimerDelegate(EndTornadoTimer, rate, no loop) from the pawn's tick groups (a notify, a hit): pending, so it
/// fires in the timer pass of tick + timerTicks(rate); a running EndTornadoTimer is cleared and replaced
/// (K2_FindDynamicTimerHandle); Rate <= 0 clears it and returns handle 0.
fn set_tornado_timer(w: &mut World, f: FighterId, rate: Real) {
    let tick = w.fighter(f).clock.tick;
    let s = taz_w(w, f);
    if rate <= 0.0 {
        s.timer_valid = false;
        s.timer_at = -1;
        return;
    }
    s.timer_valid = true;
    s.timer_set = tick;
    s.timer_rate = rate;
    #[allow(clippy::cast_possible_truncation, reason = "a finite tick count (rate > 0)")]
    let at = (crate::real(tick) + timer_ticks(Some(rate))) as i32;
    s.timer_at = at;
}

/// K2_GetTimerRemainingTimeHandle(TornadoTimer) (142b26730): 0 for an invalid handle, -1 when the handle names no
/// timer (it fired), the Rate while pending (set this frame), else ExpireTime - InternalTime: activated in the timer
/// pass of the frame it was set in (that frame's delta not counted), so (tick - set - 1) deltas have gone by at a
/// pawn tick.
fn timer_remaining(tick: i32, s: &TazState) -> Real {
    if !s.timer_valid {
        return 0.0;
    }
    if s.timer_at < 0 {
        return -1.0;
    }
    if tick == s.timer_set {
        return s.timer_rate;
    }
    s.timer_rate - crate::real(tick - s.timer_set - 1) * DELTA_SECONDS
}

// ------------------------------------------------------------------ the tornado timer UI

/// UI_StateType's byte of a data enumerator (UI_StateType.json): NewEnumerator0 is 1, 1 is 2, 2 is 0.
fn ui_state_type(s: Option<&str>) -> i32 {
    match s {
        Some("UI_StateType::NewEnumerator0") => 1,
        Some("UI_StateType::NewEnumerator1") => 2,
        Some("UI_StateType::NewEnumerator2") => 0,
        _ => 1,
    }
}

/// UI_StateDisplayType: NewEnumerator n is n (the trailing digits of the data's name; 2 without).
fn ui_display_type(s: Option<&str>) -> i32 {
    let digits: String = s.unwrap_or("").chars().rev().take_while(char::is_ascii_digit).collect();
    let digits: String = digits.chars().rev().collect();
    if digits.is_empty() { 2 } else { digits.parse().unwrap_or(2) }
}

/// The class default struct 'Tornado UI State Params' (id TornadoTimer, MI_State_Tornado).
fn ui_params(w: &World, f: FighterId) -> Option<&mvs_data::value::Props> {
    data_of(w, f).fighter.pawn.get("Tornado UI State Params").and_then(Value::as_obj)
}

fn ui_id(w: &World, p: Option<&mvs_data::value::Props>) -> Name {
    p.and_then(|p| p.get("id")).and_then(Value::as_str).map_or(name!("TornadoTimer"), |s| data_name(w, s))
}

/// SetTornadoTimerUI(Duration) @17918-18148: Tornado UI State Params.Timer = Duration, then the fighter's UI broker
/// AddStateForPlayer(Params).
fn set_tornado_timer_ui(w: &mut World, f: FighterId, duration: Real) {
    let p = ui_params(w, f);
    let str_of = |k: &str| p.and_then(|p| p.get(k)).and_then(Value::as_str);
    let num_of = |k: &str, or: Real| p.and_then(|p| p.get(k)).and_then(Value::as_f64).unwrap_or(f64::from(or));
    let params = UiStateParams {
        id: ui_id(w, p),
        state_type: ui_state_type(str_of("StateType")),
        display_type: ui_display_type(str_of("DisplayType")),
        current_value: crate::math::narrow(num_of("CurrentValue", -1.0)),
        max_value: crate::math::narrow(num_of("MaxValue", -1.0)),
        timer: duration,
        icon_material: str_of("IconMaterial").map_or(name!("MI_State_Tornado"), |s| data_name(w, s)),
        active: p.and_then(|p| p.get("Active")).and_then(Value::as_bool).unwrap_or(true),
        pop_text: Name::NONE,
        auto_remove_time: crate::math::narrow(num_of("AutoRemoveTime", -1.0)),
        subicon: str_of("Subicon").map(|s| data_name(w, s)),
    };
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Add(params));
}

/// ClearTornadoTimerUI @18154-18357: RemoveStateForPlayer(Params.id).
fn clear_tornado_timer_ui(w: &mut World, f: FighterId) {
    let id = ui_id(w, ui_params(w, f));
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(id));
}

// ------------------------------------------------------------------ SetTornadoMovementMode

/// BPI_SetTornadoMovementMode(On) @5441-6417 (the port is the authority: both HasAuthority gates pass).
pub fn set_tornado_movement_mode(w: &mut World, f: FighterId, on: bool) {
    match taz_of(w, f) {
        Some(s) if s.tornado_movement_on != on => {} // @5479: a changed mode only
        _ => return,
    }
    taz_w(w, f).tornado_movement_on = on;
    if on {
        taz_w(w, f).tornado_active = true; // @5541
        // @5552-5602 (on false: the cooldown VO, SFX and UI error)
        let ready = trigger_ability_check(w, f, SLOT);
        taz_w(w, f).is_non_cooldown_tornado = ready;
        // @5651 SpawnBuffActor(Buff_C015Tornado, self, self, 0, 0): a live one (CanRefresh, no life timer) is not
        // replaced and the call returns none, so the cast @5784 fails and the flow ends: no UI, no timer.
        let Some(b) = spawn_buff(w, f, TORNADO_BUFF, 0.0, Some(0), Some(f), true, None) else { return };
        taz_w(w, f).tornado_buff = Some(b); // @5794
        let ready = taz(w, f).is_non_cooldown_tornado;
        let rate = pawn_num(w, f, if ready { "TornadoBaseDuration" } else { "TornadoCooldownDuration" });
        if ready {
            set_tornado_timer_ui(w, f, rate); // @5813-5882
        }
        taz_w(w, f).ally_hit_closed = false; // ResetTornadoHitAlly @5905 -> @17186
        set_tornado_timer(w, f, rate); // @5919-6054
        return;
    }
    if taz(w, f).is_non_cooldown_tornado {
        trigger_ability(w, f, SLOT); // @6082-6096 TriggerAbilityActual(4)
    }
    if let Some(b) = tornado_buff_live(w, taz(w, f)) {
        // @6219-6255: with the buff's Reflecting (the perk's SetReflecting alone writes it) his Light Projectile
        // Armor off.
        if inst_view(w, b).flag("Reflecting") {
            w.fighter_mut(f).invuln.light_projectile_armor = Some(false);
        }
        end_buff(w, f, b); // @6288 K2_DestroyActor
    }
    // @6324-6367 AllyTornadoReflect: no graph writes it (always invalid).
    clear_tornado_timer_ui(w, f); // @6403
}

// ------------------------------------------------------------------ the ally hit

/// OnAllyHit(HitAlly, Actor) @14629-15600, bound on authority to his HitBoxHandler's HitAlly (BeginPlay @10512, after
/// Buff_C015Eating's spawn and cast @9988-10121), then MulticastHitAlly @16439 through HitAllyLight
/// (NotifyAllyOfHit's HitAllyMulticast @923, bound @10576).
fn on_ally_hit(w: &mut World, f: FighterId, vic: FighterId) {
    ally_hit_body(w, f, vic);
    taz_w(w, f).hit_ally = true; // MulticastHitAlly @16439
}

fn ally_hit_body(w: &mut World, f: FighterId, vic: FighterId) {
    if !taz(w, f).tornado_movement_on {
        return; // @14659
    }
    // @14669-14719 (every ally hit asks; on false the cooldown VO and UI error)
    if !trigger_ability_check(w, f, SLOT) {
        return;
    }
    if taz(w, f).ally_hit_closed {
        return; // the DoOnce @14729-14789
    }
    taz_w(w, f).ally_hit_closed = true;
    // @14800-15091: the remaining time + TornadoHitAllyBonusDuration to the UI and as a new timer (the set replaces
    // the running one; ClearAndInvalidate of the old handle then finds nothing).
    let left = timer_remaining(w.fighter(f).clock.tick, taz(w, f));
    let rate = left + pawn_num(w, f, "TornadoHitAllyBonusDuration");
    set_tornado_timer_ui(w, f, rate);
    set_tornado_timer(w, f, rate);
    // @15118-15259: the ally's Comp_State_Manager SpawnBuffActor(Buff_AllyTornadoed, ally, self, 0,
    // AllyReflectBuffDuration).
    let dur = pawn_num(w, f, "AllyReflectBuffDuration");
    let nb = spawn_buff(w, vic, ALLY_BUFF, dur, Some(0), Some(f), true, None);
    // @15339-15564: with the TornadoReflect perk on him and a NewBuff returned (a refreshed live one returns none: no
    // call), its MulticastActivateReflect.
    if let Some(nb) = nb
        && has_tornado_reflect(w, f)
    {
        activate_ally_reflect(w, vic, nb);
    }
}

/// BPI_ListenForAllyHit @13864 -> @13852 (AnimNotify_C015HandleEatenFighters' Begin; no montage of the data has that
/// notify): ListeningForAllyHits has no reader.
pub fn listen_for_ally_hit(w: &mut World, f: FighterId) {
    if let Some(s) = taz_mut(w, f) {
        s.listening_for_ally_hits = true;
    }
}

// ------------------------------------------------------------------ BPI_CheckForAllyInFront

/// BPI_CheckForAllyInFront @0-1349 (authority; a client also asks the server, CheckForAllyInFrontServer @16667, which
/// runs the same body): SphereTraceMultiForObjects(location, location + (0, +-200, 0) by FacingLeft, radius 100,
/// [Pawn], simple, ignore [self], bIgnoreSelf). Per hit: a BP_FighterCharacter with a Comp_Actor_Destructible; an
/// enemy returns false at once (@1338); an ally sets LocalFoundAlly and joins HitAllies (AddUnique),
/// MulticastHitAlly when the AddUnique's index is 1 (@1162-1206: the second ally the list takes). Returns
/// LocalFoundAlly (false at the start: the local of a function call).
/// Unverified (`fighter::traces`): the order of the hits is the sweep's (by time), the gather order among equal
/// times.
pub fn check_for_ally_in_front(w: &mut World, f: FighterId) -> bool {
    if taz_of(w, f).is_none() {
        return false;
    }
    let (x0, y0) = (w.fighter(f).body.x, actor_y(w, f));
    let x1 = x0 + 200.0 * facing(w, f); // UE +Y is screen left: FacingLeft's +200 is sim -x
    let me = Some(ActorRef::Fighter(f));
    let mut hits: Vec<TraceHit> = Vec::new();
    sphere_trace_multi_for_objects(w, x0, y0, x1, y0, 100.0, &[crate::real(OBJ_PAWN)], &[me], me, &mut hits);
    let team = w.fighter(f).team.team;
    let mut found = false;
    for h in &hits {
        // the BP_FighterCharacter cast @807-836
        let Some(ActorRef::Fighter(v)) = h.actor else { continue };
        if !peers(w).contains(&v) {
            continue;
        }
        if w.fighter(v).team.team != team {
            return false;
        }
        found = true;
        let s = taz_w(w, f);
        if s.hit_allies.contains(&v) {
            continue;
        }
        s.hit_allies.push(v);
        if s.hit_allies.len() == 2 {
            s.hit_ally = true; // MulticastHitAlly @1206 (the AddUnique's index 1)
        }
    }
    found
}

// ------------------------------------------------------------------ the reflect

/// @16691-16956: the reflected actor as SpawnedActorParent_C (a projectile; an item too: ItemActorParent_C is its
/// child), ProjectileMovement.Velocity = SelectVector((0, 1, 0), (0, -1, 0), FacingLeft) x VSize(Velocity): level, in
/// his facing (UE Y = -sim x, FacingLeft = facing < 0), at the speed Comp_ReflectProjectile gave it. It runs behind
/// the reflect, so it replaces the reflect's direction (the Hitframe's KnockBackDirectionVector, the stick's aim) for
/// every reflect through his handler: his tornado's ReflectProjectiles, DodgeReflect's packet.
/// Deviation: an item lying on its body (its ProjectileMovement off) gets the movement's Velocity written, which the
/// port does not keep apart from the body's (`combat::reflect::pmc_active`): left as the reflect left it (that path
/// already warns there).
fn on_projectile_reflected(w: &mut World, f: FighterId, a: ActorRef) {
    if !pmc_active(w, a) {
        return;
    }
    let left = facing(w, f) < 0.0;
    let v = match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get_mut(id).map(|p| (&mut p.vx, &mut p.vy)),
        ActorRef::Item(id) => w.actors.items.get_mut(id).map(|i| (&mut i.vx, &mut i.vy)),
        _ => None,
    };
    let Some((vx, vy)) = v else { return };
    // VSize: X is 0 in the port's plane
    let size = ((*vx * *vx) + (*vy * *vy)).sqrt(); // sqrtss of the float sum
    *vx = if left { -size } else { size };
    *vy = 0.0;
}

// ------------------------------------------------------------------ the hooks

/// EndTornadoTimer @17218 -> MulticastTornadoActive(false) @17234, in the frame's timer pass.
pub fn timer_tick(w: &mut World, f: FighterId) {
    let tick = w.fighter(f).clock.tick;
    let s = taz_w(w, f);
    if s.timer_at >= 0 && tick >= s.timer_at {
        s.timer_at = -1;
        s.tornado_active = false;
    }
}

/// The hooks of this file in Taz's CharHooks: his Comp_Pawn_HitBoxHandler's HitAlly / HitAllyLight (the ally path of
/// the melee test), and his handler's ReflectedProjectile (pfgDoDamage @15462, bound at BeginPlay @10278-10301 on
/// authority: OnProjectileReflectedServer @14605 -> OnProjectileReflectedMulticast -> @16691-16956).
pub fn hooks(h: &mut CharHooks) {
    h.hit_ally = Some(on_ally_hit);
    h.reflected_projectile = Some(on_projectile_reflected);
}

fn movement_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    set_tornado_movement_mode(w, f, true);
}

fn movement_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    set_tornado_movement_mode(w, f, false);
}

/// Condition_C015NotTornadoing: !BPI_IsTornadoActive (TornadoActive); a failed BPI_C015 cast reads false: true.
fn not_tornadoing(w: &mut World, f: FighterId) -> bool {
    !taz_of(w, f).is_some_and(|s| s.tornado_active)
}

/// Condition_C015NotTornadoingAlly: !BPI_IsTornadoingAlly (TornadoingAlly: no graph writes it); no BPI_C015: true.
/// Only the legacy C015_Side_Special_Ground_Montage polls it.
fn not_tornadoing_ally(w: &mut World, f: FighterId) -> bool {
    !taz_of(w, f).is_some_and(|s| s.tornadoing_ally)
}

/// OnlyAllyInFront_Condition: BPI_CheckForAllyInFront's AllyInFront; a failed cast false. No montage or branch of
/// the data names it.
fn only_ally_in_front(w: &mut World, f: FighterId) -> bool {
    check_for_ally_in_front(w, f)
}

/// Buff_C015Tornado: SpeedMod 0.8 Multiply, BuffDuration 0, CanRefresh: the shared buff code. Its own graph:
/// BeginPlay @2095 -> the instigator's BPI_SetTornadoIsActive(true) (an empty body, Pawn_C015 @13199), the
/// instigator CMC's MaxAcceleration kept (OriginalMaxAceleration @2423) and set to ModifiedAcelerationSpeed 2000
/// (@2543), the perk test @2691 (perks_reflect.rs tornado_reflect_begin). EndPlay @1920: SetTornadoIsActive(false)
/// (empty); Reflecting -> SetReflecting(false) @208 (perks_reflect.rs tornado_reflect_end); then @223: the restore
/// reads GetComponentByClass(CharacterMovementComponent) on the buff actor itself, which has none (@281: skipped), so
/// MaxAcceleration stays 2000 until another writer (the MoveCommandManager's end @843 sets 4096). TurnVFX
/// (PS_C015_Combo2_TornadoTrail_Arrows) is the view's. OriginalMaxAceleration is read by the skipped restore alone,
/// so it is not kept.
fn tornado_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    // GetInstigator().GetComponentByClass(CharacterMovementComponent) @2242-2342: a fighter's FMC, else pop_flow (no
    // perk test either).
    let Some(from) = from else { return };
    let d = inst_view(w, b);
    let Some(v) = d.num("ModifiedAcelerationSpeed") else {
        panic!("{}: no ModifiedAcelerationSpeed", w.names.str(TORNADO_BUFF))
    };
    let modifier = d.num("ReflectBuffSpeedModifier");
    set_max_accel(w, from, v); // @2543
    tornado_reflect_begin(w, f, b, modifier); // the perk test on the owner @2691
}

fn tornado_end(w: &mut World, f: FighterId, b: BuffRef) {
    tornado_reflect_end(w, f, b);
}

/// This file's registry entries (AnimNotify_C015TornadoMovement_C, the conditions, Buff_C015Tornado_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    // AnimNotify_C015TornadoMovement: a BPI_C015 owner's BPI_SetTornadoMovementMode(true) at Begin @215, (false) at
    // End @215; another owner nothing.
    r.notifies.enter(
        "AnimNotify_C015TornadoMovement_C",
        NotifyHandler { begin: Some(movement_begin), end: Some(movement_end), ..NotifyHandler::default() },
    )?;
    r.conditions.enter("Condition_C015NotTornadoing_C", Condition::Fn(not_tornadoing))?;
    r.conditions.enter("Condition_C015NotTornadoingAlly_C", Condition::Fn(not_tornadoing_ally))?;
    r.conditions.enter("OnlyAllyInFront_Condition_C", Condition::Fn(only_ally_in_front))?;
    r.buffs.enter(
        "Buff_C015Tornado_C",
        BuffEntry::new(
            BuffClass::BuffC015Tornado,
            BuffHooks { begin: Some(tornado_begin), end: Some(tornado_end), ..BuffHooks::default() },
        ),
    )?;
    Ok(())
}
