//! The perk TornadoReflect: Buff_C015_TornadoReflect and Buff_AllyTornadoed.
//!
//! Perk_C015_TornadoReflect (IsUnavailableOffline: not on the port's perk page; ported all the same) gives him
//! Buff_C015_TornadoReflect, which has no graph of its own: the other classes ask BPI_CheckIfHasBuff for it.
//! - Buff_C015Tornado's perk branch: BeginPlay @2691 with the perk held: SpeedMod = ReflectBuffSpeedModifier (0.6)
//!   @2864, the owner's UpdatePlayerStatus @3072, ReflectVisuals @3109 (the arrows: the renderer's), FacingChanged
//!   bound @3218 (the arrows' flip alone), SetReflecting(true) on authority @1904. EndPlay @2076: Reflecting ->
//!   SetReflecting(false) @208, TurnVFX destroyed @412.
//!   SetReflecting(Active) @1523-1827, on a change alone: Reflecting, the buff's own DealtDamageMods.DestroyProjectiles
//!   @1590 and .ReflectProjectiles @1618 (its GetDamageMod returns them: not CanStack), and the owner DamageManager's
//!   Light Projectile Armor @1827. The dealt flags reach his tornado's Hitframes on a projectile through the dealt
//!   pass (the melee projectile test), the reflect @15158 first, DestroyProjectiles @15491 after a failed one.
//! - Pawn_C015 OnAllyHit @15339-15564 (tornado.rs ally_hit_body): with the perk on Taz and a Buff_AllyTornadoed the
//!   spawn returned, its MulticastActivateReflect (Buff_AllyTornadoed @2764): Reflecting @2803, the pop text "REFLECT
//!   PROJECTILES" on the ally @1478-1685 (the HUD's pop text kind reflectProjectiles), the arrows on the ally
//!   @648-1422, its HitByEnemy bound @2220 and Light Projectile Armor on @2355. OnHitByEnemy @2389: the Attacker
//!   (DamageAndLaunch's DamageDealer: the projectile itself) cast to SpawnedActorParent, BPI_CanBeReflected @2464
//!   (the reflect reads it first), the ally's Comp_ReflectProjectile.ReflectProjectile(Attacker, ReflectHit) @2700:
//!   KnockBackDirectionVector (-1, 0) is the one field RP reads. EndPlay @1736: Reflecting -> armor off @226,
//!   HitByEnemy unbound @330, the arrows destroyed @570.
//!   An Attacker of World.actors (a SpawnedActorParent kind that is no Projectile or Item) passes the cast; its
//!   BPI_CanBeReflected is false (`combat::reflect::reflect_world_actor`: no such class of the data sets it), so
//!   nothing more happens. A buff actor's own hit (its handler's owner, no SpawnedActorParent) fails the cast.

use crate::Real;
use crate::actors::ActorRef;
use crate::combat::buffs::{buff_of, inst_view, set_buff_values, update_states};
use crate::combat::reflect::{reflect_projectile, reflect_world_actor};
use crate::combat::{BuffClass, BuffRef};
use crate::events::{Event, PopKind};
use crate::fighter::FighterId;
use crate::fighter::common::actor_is_a;
use crate::fighter::moves::V2;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

/// The perk's marker buff.
pub const REFLECT_PERK_BUFF: Name = name!("Buff_C015_TornadoReflect_C");
pub const TORNADO_BUFF: Name = name!("Buff_C015Tornado_C");
pub const ALLY_BUFF: Name = name!("Buff_AllyTornadoed_C");

/// BPI_CheckIfHasBuff(Buff_C015_TornadoReflect) on Taz.
pub fn has_tornado_reflect(w: &World, f: FighterId) -> bool {
    buff_of(w, f, REFLECT_PERK_BUFF).is_some()
}

/// Buff_C015Tornado SetReflecting(Active) @1523-1827.
fn set_reflecting(w: &mut World, f: FighterId, b: BuffRef, on: bool) {
    let d = inst_view(w, b);
    if d.flag("Reflecting") == on {
        return; // @1523-1561
    }
    let mut mods = d.obj("DealtDamageMods").cloned().unwrap_or_default();
    mods.insert("DestroyProjectiles".into(), Value::Bool(on));
    mods.insert("ReflectProjectiles".into(), Value::Bool(on));
    let mut v = Props::new();
    v.insert("Reflecting".into(), Value::Bool(on));
    v.insert("DealtDamageMods".into(), Value::Obj(mods));
    set_buff_values(w, b, &v);
    // @1666-1827 (a fighter's DamageManager is always valid)
    w.fighter_mut(f).invuln.light_projectile_armor = Some(on);
}

/// Buff_C015Tornado BeginPlay's perk branch @2691-3241 (tornado.rs calls it behind the acceleration write):
/// `modifier` the begin's ReflectBuffSpeedModifier.
pub fn tornado_reflect_begin(w: &mut World, f: FighterId, b: BuffRef, modifier: Option<Real>) {
    if !has_tornado_reflect(w, f) {
        return; // @2854
    }
    let Some(v) = modifier else { panic!("{}: no ReflectBuffSpeedModifier", w.names.str(TORNADO_BUFF)) };
    let mut p = Props::new();
    p.insert("SpeedMod".into(), Value::Num(f64::from(v)));
    set_buff_values(w, b, &p); // @2864
    update_states(w, f); // @3072 UpdatePlayerStatus
    set_reflecting(w, f, b, true); // @1874-1904
}

/// Buff_C015Tornado EndPlay @2076 -> @15: the owner is a fighter, SetReflecting(false) @208 when Reflecting.
pub fn tornado_reflect_end(w: &mut World, f: FighterId, b: BuffRef) {
    if inst_view(w, b).flag("Reflecting") {
        set_reflecting(w, f, b, false);
    }
}

/// Buff_AllyTornadoed MulticastActivateReflect @2764 on the ally `f` (tornado.rs ally_hit_body, with the perk on Taz).
pub fn activate_ally_reflect(w: &mut World, f: FighterId, b: BuffRef) {
    if inst_view(w, b).flag("Reflecting") {
        return; // @2764-2803
    }
    let mut v = Props::new();
    v.insert("Reflecting".into(), Value::Bool(true));
    set_buff_values(w, b, &v); // @2803
    let id = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::PopText {
        fighter: id,
        kind: PopKind::ReflectProjectiles,
        stacks: 0.0,
        gain: None,
        params: None,
    }); // @1685
    // @2355 (HitByEnemy bound @2220: `ally_on` reads Reflecting)
    w.fighter_mut(f).invuln.light_projectile_armor = Some(true);
}

/// Buff_AllyTornadoed's OnHitByEnemy @2389.
fn ally_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::HitByEnemy || !inst_view(w, b).flag("Reflecting") {
        return false;
    }
    let Some(by) = a.by.as_ref() else { return false };
    if by.fighter.is_some() {
        return false; // a fighter Attacker fails the SpawnedActorParent cast @2389-2454
    }
    let t = by.actor;
    if !matches!(t, ActorRef::Projectile(_) | ActorRef::Item(_)) {
        // @2389 the cast (a buff actor's own hit: no SpawnedActorParent), @2464 BPI_CanBeReflected
        if let ActorRef::Actor(id) = t
            && let Some(sa) = w.actors.actors.get(id)
            && actor_is_a(w, ActorRef::Fighter(sa.owner), sa.cls, name!("SpawnedActorParent_C")) == Some(true)
        {
            reflect_world_actor(w, id);
        }
        return false;
    }
    let d = inst_view(w, b);
    let k = d.obj("ReflectHit").and_then(|h| h.get("KnockBackDirectionVector")).and_then(Value::as_obj);
    let (Some(x), Some(y)) = (k.and_then(|k| k.get("X")?.as_f64()), k.and_then(|k| k.get("Y")?.as_f64())) else {
        panic!("{}: ReflectHit.KnockBackDirectionVector missing from the data", w.names.str(ALLY_BUFF))
    };
    reflect_projectile(w, f, t, V2 { x: crate::math::narrow(x), y: crate::math::narrow(y) }, None); // @2700
    false
}

fn ally_end(w: &mut World, f: FighterId, b: BuffRef) {
    if inst_view(w, b).flag("Reflecting") {
        w.fighter_mut(f).invuln.light_projectile_armor = Some(false); // @226
    }
}

/// This file's registry entries (Buff_C015_TornadoReflect_C, Buff_AllyTornadoed_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    // The perk's buff: no graph of its own (a marker the classes above ask for).
    r.buffs
        .enter("Buff_C015_TornadoReflect_C", BuffEntry::new(BuffClass::BuffC015TornadoReflect, BuffHooks::default()))?;
    // Buff_AllyTornadoed: SpeedMod 1.1 Multiply and the class default DealtDamageMods.ReflectProjectiles (with or
    // without the perk: the ally's own melee reflects through the dealt pass); no BeginPlay of its own.
    r.buffs.enter(
        "Buff_AllyTornadoed_C",
        BuffEntry::new(
            BuffClass::BuffAllyTornadoed,
            BuffHooks { on: Some(ally_on), end: Some(ally_end), ..BuffHooks::default() },
        ),
    )?;
    Ok(())
}
