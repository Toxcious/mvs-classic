//! Mutator_GrowWithDamage (a fighter mutator: Mutator_Parent child, so
//! BuffDuration 0 and DestroyBuffWhenOwnerDefeated: one life each, its count starting again at 0 in the next) and
//! Buff_GrowWithDamageProjectile, the buff it gives the owner's spawned actors. Class defaults: the mutators' data,
//! read only in a match with mutators.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::buffs::{BuffExt, BuffRef, buff_mut, inst_view, set_buff_values};
use crate::combat::elem::{ElemExt, actor_size_update, set_elem_ext, spawn_actor_buff};
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::common::actor_is_a;
use crate::math::{min, ue};
use crate::name;
use crate::registry::entries::{ActorBuffHooks, BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

const GROW_PROJECTILE: &str = "Buff_GrowWithDamageProjectile_C";

/// The buff's own variable: DamageDealt (float, 0 at BeginPlay), `BuffExt::GrowWithDamage`.
#[derive(State, Debug, Default)]
pub struct GrowExt {
    pub dealt: Real,
}

/// A struct of the class defaults copied with one field written (a graph's struct member set).
fn with_field(s: Option<&Props>, k: &str, v: Real) -> Value {
    let mut o = s.cloned().unwrap_or_default();
    o.insert(k.to_owned(), Value::Num(f64::from(v)));
    Value::Obj(o)
}

/// CalculateScaleFromDamage @0-405: n = FFloor(DamageDealt / DealtDamageNeededForIncrease); NewScale = FMin(n x
/// ScaleIncreaseIncrements + 1, MaxScaleIncrease), NewDamage = FMin(n x DamageIncreaseIncrements + 1, MaxDamageIncrease),
/// float32 steps (Multiply_IntFloat: the int taken as a float).
fn scale_from_damage(d: crate::combat::buffs::Def, dealt: Real) -> (Real, Real) {
    let num = |k: &str, dflt: Real| d.num(k).unwrap_or(dflt);
    let n = crate::real(ue::floor_to_int(dealt / num("DealtDamageNeededForIncrease", 5.0)));
    let s = min((n * num("ScaleIncreaseIncrements", 0.05)) + 1.0, num("MaxScaleIncrease", 2.5));
    let m = min((n * num("DamageIncreaseIncrements", 0.025)) + 1.0, num("MaxDamageIncrease", 1.5));
    (s, m)
}

/// The class defaults' MultSizeMod.X and DealtDamageMods.DamageMod as the instance holds them now.
fn current_values(w: &World, b: BuffRef) -> (Real, Real) {
    let d = inst_view(w, b);
    let x = d.obj("SizeMods").and_then(|s| s.get("MultSizeMod")).and_then(Value::as_obj).and_then(|m| m.get("X"));
    let m = d.obj("DealtDamageMods").and_then(|s| s.get("DamageMod"));
    (
        crate::math::narrow(x.and_then(Value::as_f64).unwrap_or(1.0)),
        crate::math::narrow(m.and_then(Value::as_f64).unwrap_or(1.0)),
    )
}

/// The SizeMods struct a size change writes: MakeStruct(MultSizeMod (s, s, s)).
fn size_mods(s: Real) -> Value {
    let mut v = Props::default();
    for k in ["X", "Y", "Z"] {
        v.insert(k.to_owned(), Value::Num(f64::from(s)));
    }
    let mut o = Props::default();
    o.insert("MultSizeMod".to_owned(), Value::Obj(v));
    Value::Obj(o)
}

/// MulticastSizeChange(Scale, DamageMultiplier) @1852: SizeMods = MakeStruct(MultSizeMod (s, s, s)) (the struct
/// replaced whole) @1943, DealtDamageMods.DamageMod = m @1970 (Multiply in the class defaults: the owner's later hits
/// x m), the owner's Comp_State_Manager UpdatePlayerStatus(null) @2191 (`update_states`: the size through
/// `recalc_size_mod` with the owner's other size buffs); then, off a dedicated server and with the owner's
/// Comp_Pawn_MeshManager valid, PS_ShadowClone_Impact at VFX_Root with its Team (the view's: the `GrowFx` event).
fn size_change(w: &mut World, f: FighterId, b: BuffRef, s: Real, m: Real) {
    let dmg = {
        let d = inst_view(w, b);
        with_field(d.obj("DealtDamageMods"), "DamageMod", m)
    };
    let mut p = Props::default();
    p.insert("SizeMods".to_owned(), size_mods(s));
    p.insert("DealtDamageMods".to_owned(), dmg);
    set_buff_values(w, b, &p);
    crate::combat::buffs::update_states(w, f);
    let fighter = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::GrowFx { fighter });
}

/// OnHitAnything @850-1847 and OnDamageTaken @3376-3714 after the add: CalculateScaleFromDamage, and
/// MulticastSizeChange when its scale differs from GetSizeMod().MultSizeMod.X (BuffParentActor GetSizeMod: SizeMods,
/// the class does not stack) or its damage from DealtDamageMods.DamageMod (NotEqual_FloatFloat, exact).
fn add_dealt(w: &mut World, f: FighterId, b: BuffRef, damage: Real) {
    let Some(x) = buff_mut(w, b) else { return };
    let BuffExt::GrowWithDamage(e) = &mut x.ext else { return };
    e.dealt += damage;
    let dealt = e.dealt;
    let (s, m) = scale_from_damage(inst_view(w, b), dealt);
    let (cs, cm) = current_values(w, b);
    if s != cs || m != cm {
        size_change(w, f, b, s, m);
    }
}

/// OnHitAnything(HitBoxInfo, HitActor) @850: HitActor cast to BP_FighterCharacter @850-915 (a fighter of the match),
/// not the owner @949, both with a valid Comp_Actor_Destructible @1001-1195 (every fighter), TeamIndex unequal @1331;
/// then DamageDealt += HitBoxInfo.HitBoxDamage.Damage @1427.
fn on_hit_anything(w: &mut World, f: FighterId, b: BuffRef, a: &BuffSignalArg) {
    let Some(ActorRef::Fighter(v)) = a.hit_actor else { return };
    if v == f || !w.players.contains(&v) || w.fighter(v).team.team == w.fighter(f).team.team {
        return;
    }
    add_dealt(w, f, b, a.damage.unwrap_or(0.0));
}

/// The Comp_State_Manager test of OnActorSpawned @2925-3005: every SpawnedActorParent has one in its components (a
/// Projectile, an Item, an actor of World.actors whose class is a child).
fn has_state_manager(w: &World, f: FighterId, a: ActorRef) -> bool {
    matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_))
        || crate::actors::world_actor::actor_cls(w, a)
            .is_some_and(|c| actor_is_a(w, ActorRef::Fighter(f), c, name!("SpawnedActorParent_C")) == Some(true))
}

/// OnActorSpawned(SpawnedActor) @2925: with a valid Comp_State_Manager on the actor, its SpawnBuffActor(
/// Buff_GrowWithDamageProjectile_C, the actor, Instigator the owner, 0, 0.0, true, false) @3090; the new buff (the cast
/// @3174 fails on a refused spawn: nothing more) gets MulticastGrowthValues(SizeMods.MultSizeMod.X, DealtDamageMods.DamageMod)
/// @3308, read from the mutator's own variables (not GetSizeMod: the same, it does not stack).
fn on_actor_spawned(w: &mut World, f: FighterId, b: BuffRef, a: ActorRef) {
    if !has_state_manager(w, f, a) {
        return;
    }
    let cls = name!("Buff_GrowWithDamageProjectile_C");
    let Some(serial) = spawn_actor_buff(w, a, cls, 0, 0.0, true, Some(f)) else { return };
    let (s, m) = current_values(w, b);
    growth_values(w, a, serial, s, m);
}

/// Buff_GrowWithDamageProjectile MulticastGrowthValues(NewScale, NewDamage) -> ubergraph @10: SizeMods =
/// MakeStruct(MultSizeMod (s, s, s)) @101, DealtDamageMods.DamageMod = NewDamage @128, then the owner's
/// Comp_State_Manager UpdatePlayerStatus(null) @349 when it is valid (`actor_size_update`: the actor's scale). The
/// class's DealtDamageMods.DamageModType is DoNothing (NewEnumerator3) and it does not stack (CanStack false: its
/// DealtDamageModPerStack, Multiply 1, is not read), so the DamageMod it writes changes no hit of the actor: in the
/// build the actor grows with its owner and deals its own damage.
fn growth_values(w: &mut World, a: ActorRef, serial: u32, s: Real, m: Real) {
    set_elem_ext(w, a, serial, ElemExt::GrowProjectile { size: s, damage: m });
    actor_size_update(w, a);
}

/// `projectileValues`: the variables the graph wrote over the class defaults.
fn projectile_values(w: &World, a: ActorRef, serial: u32) -> Option<Props> {
    let ElemExt::GrowProjectile { size, damage } = crate::combat::elem::elem_buff(w, a, serial)?.ext else {
        return None;
    };
    let base = crate::combat::elem::bdef(w, a, crate::combat::BuffClass::BuffGrowWithDamageProjectile);
    let mut p = Props::default();
    p.insert("SizeMods".to_owned(), size_mods(size));
    p.insert(
        "DealtDamageMods".to_owned(),
        with_field(base.and_then(|d| d.get("DealtDamageMods")).and_then(Value::as_obj), "DamageMod", damage),
    );
    Some(p)
}

/// ReceiveBeginPlay @10 (the parent's first) binds, each behind IsValid of the owner's component, a failed one
/// skipping the binds after it (@120 / @369 / @618 -> @3719): Comp_Pawn_HitBoxHandler.HitAnything -> OnHitAnything
/// (@154 / @228), Comp_Pawn_DamageManager.HitByEnemy -> OnDamageTaken (@454 / @477), Comp_Spawner.AfterSpawnedActor ->
/// OnActorSpawned (@652 / @726). Every fighter has the three, so all three are bound: the skip is not reachable for a
/// fighter owner and is not modelled.
fn begin(w: &mut World, _f: FighterId, b: BuffRef, _by: Option<FighterId>, _d: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::GrowWithDamage(GrowExt { dealt: 0.0 });
    }
}

// OnDamageTaken(Hit) @3376: DamageDealt += Hit.Damage (BuffSignalArg.hit: LocalHitInformation.Damage of the
// HitByEnemy broadcast). A HitByEnemy the port sends without a packet adds nothing.
// Deviation: a BFBounce re-hit adds 1 to the instigator's count (its HitAnything: `combat::hit::launch` try_bounce,
// the bounce's Damage without the instigator's dealt mods) and nothing to the bounced fighter's (the port's bounce
// sends no HitByEnemy).
// Nothing unbinds: the bindings go with the buff (its end at the owner's defeat).
fn on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    match s {
        BuffSignal::HitAnything => on_hit_anything(w, f, b, a),
        BuffSignal::HitByEnemy => {
            if let Some(h) = &a.hit {
                add_dealt(w, f, b, h.damage.unwrap_or(0.0));
            }
        }
        BuffSignal::AfterSpawnedActor => {
            if let Some(actor) = a.actor {
                on_actor_spawned(w, f, b, actor);
            }
        }
        _ => {}
    }
    false
}

/// This file's registry entries (`buffs({ ... })`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    use crate::combat::BuffClass as C;
    let hooks = BuffHooks { begin: Some(begin), on: Some(on), ..BuffHooks::default() };
    let c = C::MutatorGrowWithDamage;
    r.buffs.enter(c.registered_name().expect("a registered class"), BuffEntry::new(c, hooks))?;
    let c = C::BuffGrowWithDamageProjectile;
    let actor = ActorBuffHooks { values: Some(projectile_values), ..ActorBuffHooks::default() };
    r.buffs.enter(GROW_PROJECTILE, BuffEntry { actor: Some(actor), ..BuffEntry::new(c, BuffHooks::default()) })?;
    Ok(())
}
