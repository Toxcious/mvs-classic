//! Gizmo's bow: the Comp_HoldCharge binds (pawn.rs's handlers),
//! C023A_HoldChargeFull_Condition, C023A_Arrow_Projectile ([`Arrow`]) and C023A_BowSlow ([`BowSlowExt`]). No pawn hook.
//!
//! The binds are the hold-charge binder bindHoldCharge('gizmo', { reachedMax, ended, used }): [`bind`], called by
//! `mod.rs` on Gizmo's CharHooks before they are entered, the handlers pawn.rs's `hold_charge_reached_max` /
//! `hold_charge_ended` / `hold_charge_used`.
//!
//! Every read of the component goes to the arrow's / the condition's owner (GetOwner): under Arya's Disguise of Gizmo,
//! the copy's montages charge, read and spend her component.
//!
//! The arrow's constructor (UserConstructionScript @0-680) is the `construct` hook: its state in `ext` and, with a full
//! charge, the instance variables (DeathVFX, HitBoxDamage with BaseKnockBack 1200, one On Hit Buffs row of 3 Buff_Fire
//! stacks on the enemy hit).

use std::sync::Arc;

use crate::Real;
use crate::State;
use crate::actors::projectile::{Projectile, ProjectileExt, class_p};
use crate::actors::{ActorClass, ProjectileClass, ProjectileId};
use crate::combat::buffs::{buff, buff_mut, set_buff_values, update_states};
use crate::combat::hits::{HoldChargeHandlers, bind_hold_charge, clear_charges, get_charge_value, use_charge};
use crate::combat::{BuffClass, BuffExt, BuffRef, HitPacket};
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::common::latent_frames;
use crate::fighter::control::update_ability_ammo;
use crate::fighter::moves::{clear_gravity_override, is_falling, override_gravity};
use crate::math::to_int32;
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffEntry, BuffHooks, ProjectileHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{CharHooks, Condition, Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

use super::gizmo_of;
use super::pawn::{hold_charge_ended, hold_charge_reached_max, hold_charge_used};

/// `ARROW`.
pub const ARROW: &str = "C023A_Arrow_Projectile_C";
/// `BOW_SLOW`.
pub const BOW_SLOW: &str = "C023A_BowSlow_C";

/// `Arrow extends Projectile` (C023A_Arrow_Projectile_C): the state its class adds.
#[derive(State, Debug, Default)]
pub struct Arrow {
    /// IsFire (the drawing's: the FlameArrow template, UpdateVisualsMulticast).
    pub fire: bool,
    /// UseChargeGate (class default true).
    pub use_charge_gate: bool,
    /// The HitEnemyPlayer DoOnce.
    pub closed: bool,
}

/// C023A_BowSlow's extension (`BowSlowExt`): GravityAdjusted.
#[derive(State, Debug, Default)]
pub struct BowSlowExt {
    pub adjusted: bool,
}

/// Pawn_C023A's binds @1150-1278: HoldChargeReachedMax -> ReachedMaxCharge, HoldChargeEnd -> HoldChrageEnded,
/// HoldChargeUsed (pawn.rs), on Gizmo's CharHooks.
pub fn bind(h: &mut CharHooks) {
    bind_hold_charge(
        h,
        HoldChargeHandlers {
            reached_max: Some(hold_charge_reached_max),
            ended: Some(hold_charge_ended),
            used: Some(hold_charge_used),
        },
    );
}

/// `holdChargeClear`: C023A_PopCorn's ClearCharges on the owner's Comp_HoldCharge (popcorn.rs calls it).
pub fn hold_charge_clear(w: &mut World, f: FighterId) {
    clear_charges(w, f);
}

/// C023A_HoldChargeFull_Condition_C: the owner's Comp_HoldCharge.AtMaxCharge; none: false.
fn hold_charge_full(w: &mut World, f: FighterId) -> bool {
    w.fighter(f).charge.hold_charge.as_ref().is_some_and(|h| h.at_max)
}

// ---------------------------------------------------------------- C023A_Arrow_Projectile
// UserConstructionScript @0-680: with the owner's GetCHargeValue(0) >= 1, IsFire, the PS_C023A_FlameArrow template,
// DeathVFX PS_C023A_Arrow_Impact_Fire, HitBoxDamage.BaseKnockBack 1200 and On Hit Buffs = one row of 3 Buff_Fire stacks
// on the enemy hit. BeginPlay @539: Delay(0.3) -> ProjectileGravityScale 3.0 @233; Delay(0.033) -> UseCharge(0) and
// UseChargeGate = false @279-519 (only with a valid component); IsFire: UpdateVisualsMulticast and
// sfx_cha_C023a_bow_ignite @614-668. HitEnemyPlayer @878: an owner implementing BPI_C023A (Pawn_C023A alone: an Arya
// under Disguise does not) with IsFire: a DoOnce @35-207 of the owner's UpdateAbilityAmmo(0, 1, true) and then the
// parent's HitEnemyPlayer; a closed DoOnce runs neither. EndPlay @1022: after the parent's, with UseChargeGate,
// UseCharge(0) on the owner's component.
// The owner is GetOwner at each read (a reflect hands it on).

/// The one On Hit Buffs row of a fire arrow (`FIRE_ROW`).
fn fire_row() -> Value {
    let mut row = Props::new();
    row.insert("AppliedBuffs".to_owned(), Value::Str("Buff_Fire_C".to_owned()));
    row.insert("ApplyBuffSelf?".to_owned(), Value::Bool(false));
    row.insert("ApplyBuffHit?".to_owned(), Value::Bool(true));
    row.insert("ApplyBuffAlly?".to_owned(), Value::Bool(false));
    row.insert("ApplyBuffEnemy?".to_owned(), Value::Bool(true));
    row.insert("RequireHitToApplyBuffToSelf".to_owned(), Value::Bool(false));
    row.insert("StackCount".to_owned(), Value::Num(3.0));
    row.insert("BuffDuration".to_owned(), Value::Num(0.0));
    Value::Obj(row)
}

/// The arrow's constructor (the subclass constructor body: IsFire at a full charge).
fn arrow_construct(w: &World, p: &mut Projectile) {
    let mut a = Arrow { fire: false, use_charge_gate: true, closed: false };
    if let Some(f) = p.owner
        && get_charge_value(w, f, 0).pct >= 1.0
    {
        a.fire = true;
        let mut own = class_p(w, p).clone();
        let mut hit = match own.get("HitBoxDamage") {
            Some(Value::Obj(o)) => o.clone(),
            _ => Props::new(),
        };
        hit.insert("BaseKnockBack".to_owned(), Value::Num(1200.0));
        let mut ohb = Props::new();
        ohb.insert("OnHitBuffs".to_owned(), Value::Arr(vec![fire_row()]));
        // The world's death effect reads the string back as an interned name: the literal is interned as a code literal.
        let _ = name!("PS_C023A_Arrow_Impact_Fire");
        own.insert("DeathVFX".to_owned(), Value::Str("PS_C023A_Arrow_Impact_Fire".to_owned()));
        own.insert("HitBoxDamage".to_owned(), Value::Obj(hit.clone()));
        own.insert("On Hit Buffs".to_owned(), Value::Obj(ohb));
        p.own_p = Some(Arc::new(own));
        p.hit = HitPacket::from_props(&w.names, &hit, None);
    }
    p.ext = ProjectileExt::Arrow(a);
}

/// The arrow's state.
fn arrow_of(w: &World, pr: ProjectileId) -> Option<&Arrow> {
    match &w.actors.projectiles.get(pr)?.ext {
        ProjectileExt::Arrow(a) => Some(a),
        _ => None,
    }
}

/// The arrow's state, mutable.
fn arrow_mut(w: &mut World, pr: ProjectileId) -> Option<&mut Arrow> {
    match &mut w.actors.projectiles.get_mut(pr)?.ext {
        ProjectileExt::Arrow(a) => Some(a),
        _ => None,
    }
}

/// The arrow's `begin`.
fn arrow_begin(w: &mut World, pr: ProjectileId) {
    if !arrow_of(w, pr).is_some_and(|a| a.fire) {
        return;
    }
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let e = Event::Sound {
        fighter: p.owner_id,
        ev: name!("sfx_cha_C023a_bow_ignite"),
        x: Some(p.x),
        y: Some(p.y),
        hit: None,
        pooled: false,
        key: None,
    };
    w.events.push(e);
}

/// The arrow's `step`.
fn arrow_step(w: &mut World, pr: ProjectileId) -> bool {
    let Some(p) = w.actors.projectiles.get_mut(pr) else { return false };
    let (age, owner) = (p.age, p.owner);
    if age == to_int32(latent_frames(0.3)) {
        crate::actors::projectile::set_gravity_scale(p, 3.0);
    }
    if age == to_int32(latent_frames(0.033))
        && let Some(f) = owner
        && w.fighter(f).charge.hold_charge.is_some()
    {
        use_charge(w, f, 0);
        if let Some(a) = arrow_mut(w, pr) {
            a.use_charge_gate = false;
        }
    }
    false
}

/// The arrow's `hitEnemy`.
fn arrow_hit_enemy(w: &mut World, pr: ProjectileId, _victim: FighterId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let Some(f) = p.owner else { return };
    let Some(a) = arrow_of(w, pr) else { return };
    if !a.fire || gizmo_of(w, f).is_none() || a.closed {
        return;
    }
    if let Some(a) = arrow_mut(w, pr) {
        a.closed = true;
    }
    update_ability_ammo(w, f, 0, 1.0, true);
}

/// The arrow's `end`.
fn arrow_end(w: &mut World, pr: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let Some(f) = p.owner else { return };
    if arrow_of(w, pr).is_some_and(|a| a.use_charge_gate) {
        use_charge(w, f, 0);
    }
}

// ---------------------------------------------------------------- C023A_BowSlow
// BeginPlay @31-516: binds JumpEvent to the owner's OnIsJumping and LandEvent to OnBFLanded; IsFalling: SpeedMod 0.8 and
// SetGravityMulticast(true), else SpeedMod 0.5; UpdatePlayerStatus. JumpEvent(IsJumping) @794: only true: SpeedMod 0.8,
// with GravityAdjusted SetGravityMulticast(false), UpdatePlayerStatus. LandEvent @956: SpeedMod 0.4, the same.
// EndPlay @770: after the parent's, the unbinds, then with GravityAdjusted SetGravityMulticast(false).
// SetGravityMulticast(Modify) @989-1291: OverrideGravity(NewGravityScale) and GravityAdjusted = true, or
// ClearGravityOverride and GravityAdjusted = false. The natives: `override_gravity` (GravityScale 1.0 whatever
// NewGravityScale says, from the second move; the clear gives the BeginPlay scale 5.25 at once).

/// The slow buff's extension.
fn slow_ext(w: &World, b: BuffRef) -> Option<&BowSlowExt> {
    match &buff(w, b)?.ext {
        BuffExt::BowSlow(e) => Some(e),
        _ => None,
    }
}

/// `setGravity(f, b, on)`.
fn set_gravity(w: &mut World, f: FighterId, b: BuffRef, on: bool) {
    if on {
        override_gravity(w, f); // OverrideGravity(NewGravityScale)
        if let Some(BuffExt::BowSlow(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
            e.adjusted = true;
        }
    } else {
        if slow_ext(w, b).is_some_and(|e| e.adjusted) {
            clear_gravity_override(w, f);
        }
        if let Some(BuffExt::BowSlow(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
            e.adjusted = false;
        }
    }
}

/// `setBuffValues(b, { SpeedMod })`.
fn speed_mod(w: &mut World, b: BuffRef, v: Real) {
    let mut p = Props::new();
    p.insert("SpeedMod".to_owned(), Value::Num(f64::from(v)));
    set_buff_values(w, b, &p);
}

/// C023A_BowSlow_C begin.
fn bow_slow_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::BowSlow(BowSlowExt { adjusted: false });
    }
    if is_falling(w, f) {
        speed_mod(w, b, 0.8);
        set_gravity(w, f, b, true);
    } else {
        speed_mod(w, b, 0.5);
    }
    update_states(w, f);
}

/// C023A_BowSlow_C on (isJumping, bfLanded).
fn bow_slow_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    match s {
        BuffSignal::IsJumping => {
            if a.value != Some(true) {
                return false;
            }
            speed_mod(w, b, 0.8);
        }
        BuffSignal::BfLanded => speed_mod(w, b, 0.4),
        _ => return false,
    }
    if slow_ext(w, b).is_some_and(|e| e.adjusted) {
        set_gravity(w, f, b, false);
    }
    update_states(w, f);
    false
}

/// C023A_BowSlow_C end.
fn bow_slow_end(w: &mut World, f: FighterId, b: BuffRef) {
    if slow_ext(w, b).is_some_and(|e| e.adjusted) {
        set_gravity(w, f, b, false);
    }
}

/// This file's registry entries, in bow.rs's order: the condition, the arrow, the slow.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.conditions.enter("C023A_HoldChargeFull_Condition_C", Condition::Fn(hold_charge_full))?;
    let arrow = ProjectileHooks {
        construct: Some(arrow_construct),
        begin: Some(arrow_begin),
        step: Some(arrow_step),
        hit_enemy: Some(arrow_hit_enemy),
        end: Some(arrow_end),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        ARROW,
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::C023AArrowProjectile), ActorHooks::Projectile(arrow)),
    )?;
    r.buffs.enter(
        BOW_SLOW,
        BuffEntry::new(
            BuffClass::C023ABowSlow,
            BuffHooks {
                begin: Some(bow_slow_begin),
                on: Some(bow_slow_on),
                end: Some(bow_slow_end),
                ..BuffHooks::default()
            },
        ),
    )
}
