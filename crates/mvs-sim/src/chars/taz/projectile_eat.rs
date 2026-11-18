//! Taz's projectile eat: Buff_C015HitProjectile on the spawned actor his
//! eat Hitframes give it (an actor buff on its Comp_State_Manager, `combat::elem`; its extension [`HitProjExt`], an
//! `ElemExt` variant, so `Copy`), and Pawn_C015 SetIsEatingProjectile with the storage it fills (@6521-7307, @30-441)
//! and ResetStoredProjectile (@442-546). The burp that spends the store is burp.rs.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::actors::tunnel_carry::{CarryExt, carry_hooks};
use crate::actors::world_actor::{actor_cls, actor_dead, actor_entry, actor_p, instance_attack_type, warn};
use crate::combat::buff_class::BuffClass;
use crate::combat::buffs::{BuffRef, buff_of, end_buff};
use crate::combat::elem::{ElemExt, actor_inst_def, elem_buff_mut, set_destroy_blocked};
use crate::combat::reflect::can_be_reflected;
use crate::consts::DELTA_SECONDS;
use crate::fighter::FighterId;
use crate::fighter::common::data_of;
use crate::fighter::control::update_ability_ammo;
use crate::math::max;
use crate::name;
use crate::registry::entries::{ActorBuffHooks, BuffHooks};
use crate::registry::props::{arr, num, truthy};
use crate::registry::{BuffEntry, Registry, RegistryError};
use crate::world::World;

use super::pawn::update_eat_level;
use super::state::{taz_of, taz_w};

/// The class's name.
pub const HIT_PROJECTILE: &str = "Buff_C015HitProjectile_C";
/// The stored projectile's class when none is stored.
pub const TRASH: &str = "C015Trash_C";

// ---- Pawn_C015
// BPI_GetStoredProjectile's class is pawn.rs's storedProjectileClass, BPI_IsEatingProjectile pawn.rs's
// isEatingProjectile.

/// The storage (@30-441, from SetIsEatingProjectile @7127): FlushNetDormancy; StoredProjectile = the actor's class,
/// C015Anvil_C when the CheckHasBuff(Buff_C015_EatProjectiles) of @6661 held (@69-107: the EatProjectiles perk,
/// perks.rs; the speeds below are still the eaten actor's); then, the actor a SpawnedActorParent (every actor the eat
/// takes), StoredStartingSpeed / StoredMaxSpeed = its ProjectileMovement's InitialSpeed / MaxSpeed and
/// UpdateEatLevelServer(EatFromProjectile, true, true) @416.
// Deviation: an Item's and a world actor's ProjectileMovement speeds are not kept by the port: 0 / 0 (the burp then
// takes the notify's own speeds).
fn store_projectile(w: &mut World, f: FighterId, a: ActorRef, perk: bool) {
    let cls = if perk { name!("C015Anvil_C") } else { actor_cls(w, a).unwrap_or_default() };
    let speeds = match a {
        ActorRef::Projectile(id) => {
            w.actors.projectiles.get(id).map(|p| (crate::actors::projectile::pm_initial_speed(p), p.max_speed))
        }
        _ => None,
    };
    let (start, top) = speeds.unwrap_or((0.0, 0.0));
    let t = taz_w(w, f);
    t.stored_projectile = cls;
    t.stored_starting_speed = start;
    t.stored_max_speed = top;
    let eat = num(&data_of(w, f).fighter.pawn, "EatFromProjectile").unwrap_or(0.0);
    update_eat_level(w, f, eat, true, true);
}

/// `resetStoredProjectile`: ResetStoredProjectile (@442-546; BPI_ResetStoredProjectile, BPI_Cleanup @9914):
/// C015Trash_C, the speeds 0 and UpdateEatLevelServer(1.0, false, true).
pub fn reset_stored_projectile(w: &mut World, f: FighterId) {
    let t = taz_w(w, f);
    t.stored_projectile = name!("C015Trash_C");
    t.stored_starting_speed = 0.0;
    t.stored_max_speed = 0.0;
    update_eat_level(w, f, 1.0, false, true);
}

/// `setIsEatingProjectile`: BPI_SetIsEatingProjectile(IsEating, Projectile) @6521. True: ProjectilesBeingEaten + 1,
/// then (authority) the actor goes to the storage when its BPI_CanBeReflected (false for an actor without the
/// interface), the perk's buff (CheckHasBuff(Buff_C015_EatProjectiles) @6661, perks.rs) or its class in
/// SpecialProjectilesThatCanBeSpit holds, else UpdateAbilityAmmo(3, 1, false) @7132. False: the count - 1, clamped at
/// 0 @7196-7280.
pub fn set_is_eating_projectile(w: &mut World, f: FighterId, on: bool, a: Option<ActorRef>) {
    let t = taz_w(w, f);
    if !on {
        t.projectiles_being_eaten = max(t.projectiles_being_eaten - 1.0, 0.0);
        return;
    }
    t.projectiles_being_eaten += 1.0;
    let Some(a) = a else { return };
    let perk = buff_of(w, f, name!("Buff_C015_EatProjectiles_C")).is_some();
    let cls = actor_cls(w, a).map(|c| w.names.str(c));
    let listed = arr(&data_of(w, f).fighter.pawn, "SpecialProjectilesThatCanBeSpit")
        .iter()
        .any(|v| cls.is_some() && v.as_str() == cls);
    if can_be_reflected(w, a) || perk || listed {
        store_projectile(w, f, a, perk);
    } else {
        update_ability_ammo(w, f, 3, 1.0, false);
    }
}

// ---- Buff_C015HitProjectile
// Its owner: a Projectile, an Item, or a world actor whose kind has carry hooks (registry CarryHooks: the place and
// SetActorEnableCollision this graph writes; HarleyBomb's, chars/harley/bomb.rs).

/// Buff_C015HitProjectile_C's extension (its `Ext`, saved with the actor's buff list); `None`: the key absent.
/// It is made `{}` at its first read (the tick or the end of a buff whose begin stopped early).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct HitProjExt {
    /// Activated.
    pub act: Option<bool>,
    /// ShrinkDownProjectileTimeline's position.
    pub t: Option<Real>,
    /// The timeline's play rate.
    pub rate: Option<Real>,
    /// StartingScale.
    pub sx: Option<Real>,
    /// The Instigator's BPI_IsFacingLeft at the begin.
    pub fl: Option<bool>,
    /// The Instigator's fighter id (-1: none).
    pub inst: Option<i32>,
    /// The tunnel carry's keys (`hurt`) on the same ext.
    pub carry: CarryExt,
}

/// The buff's ext, made empty at the first read.
fn ext_mut(w: &mut World, a: ActorRef, i: u32) -> Option<&mut HitProjExt> {
    let b = elem_buff_mut(w, a, i)?;
    if !matches!(b.ext, ElemExt::C015HitProjectile(_)) {
        b.ext = ElemExt::C015HitProjectile(HitProjExt::default());
    }
    match &mut b.ext {
        ElemExt::C015HitProjectile(e) => Some(e),
        _ => None,
    }
}

fn is_actor(a: ActorRef) -> bool {
    !matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_))
}

/// The owner as the eat takes it: a projectile, an item, or a world actor whose kind has carry hooks.
fn eaten(w: &World, a: ActorRef) -> Option<ActorRef> {
    if !is_actor(a) {
        return Some(a);
    }
    actor_entry(w, a).and_then(|e| e.carry).map(|_| a)
}

/// BPI_GetProjectileAttackType: a world actor class's own field where it keeps one, else SpawnedActorParent's
/// ProjectileAttackType of the class data (0 unless saved).
fn attack_type_of(w: &World, a: ActorRef) -> Real {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or(0.0, |p| crate::real(p.attack_type)),
        ActorRef::Item(id) => crate::real(crate::actors::item::item_attack_type(w, id)),
        ActorRef::Actor(id) => match instance_attack_type(w, id) {
            Some(t) => crate::real(t),
            None => num(actor_p(w, a), "ProjectileAttackType").unwrap_or(0.0),
        },
        _ => num(actor_p(w, a), "ProjectileAttackType").unwrap_or(0.0),
    }
}

/// GetActorScale3D (X): a world actor root's is its size_scale.
fn scale_of(w: &World, a: ActorRef) -> Real {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or(1.0, |p| p.scale),
        ActorRef::Item(id) => w.actors.items.get(id).map_or(1.0, |i| i.scale),
        ActorRef::Actor(id) => w.actors.actors.get(id).and_then(|x| x.size_scale).unwrap_or(1.0),
        _ => 1.0,
    }
}

/// SetActorScale3D(s): a world actor root's is its size_scale, with its kind's size entry for the shapes made at the
/// old scale (ActorEntry `size`).
fn set_scale(w: &mut World, a: ActorRef, s: Real) {
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                crate::actors::projectile::set_actor_scale(p, s);
            }
        }
        ActorRef::Item(id) => {
            if let Some(i) = w.actors.items.get_mut(id) {
                i.scale = s;
            }
        }
        _ => {
            let k = actor_entry(w, a).and_then(|e| match e.hooks {
                crate::registry::entries::ActorHooks::World(h) => h.size,
                _ => None,
            });
            let (ActorRef::Actor(id), Some(k)) = (a, k) else {
                warn("Buff_C015HitProjectile_C: SetActorScale3D not ported (its kind has no size entry)");
                return;
            };
            let Some(x) = w.actors.actors.get_mut(id) else { return };
            let old = x.size_scale.unwrap_or(1.0);
            x.size_scale = Some(s);
            if let Some(f) = k.scale {
                f(w, id, s, old);
            }
        }
    }
}

/// The Instigator (`w.fighters.find(o => o.id === x.inst)`).
fn inst_of(w: &World, x: &HitProjExt) -> Option<FighterId> {
    let id = x.inst?;
    w.players.iter().copied().find(|&o| w.fighter(o).id == id)
}

/// ShrinkDownProjectileTimeline Update (@1851-2370): the owner's scale VLerp(StartingScale, 0, Alpha); its location
/// VLerp(the Instigator root + (0, FacingLeft ? 350 : -350, 0), the Instigator root, Alpha) by K2_SetActorLocation
/// with teleport (sim x = -UE Y: the start EatingPositionOffset in front of him). The curve is linear 0..1 over the
/// track (the template's), so Alpha is the position. VLerp (A + Alpha x (B - A)) and Add_VectorVector are float32
/// per operation (the X component, sim x; Z is the root's own).
fn update(w: &mut World, a: ActorRef, i: u32, x: HitProjExt) {
    let alpha = x.t.unwrap_or(0.0);
    let sx = x.sx.unwrap_or(0.0);
    set_scale(w, a, sx + (alpha * (0.0 - sx)));
    let Some(f) = inst_of(w, &x) else { return }; // GetInstigator null: K2_GetRootComponent fails, no move
    let d = actor_inst_def(w, a, i).and_then(|d| num(&d, "EatingPositionOffset")).unwrap_or(0.0);
    let off = d * if x.fl == Some(true) { -1.0 } else { 1.0 };
    let fx = w.fighter(f).body.x;
    let s = fx + off;
    let y = crate::fighter::actor_y(w, f);
    if let Some(k) = carry_hooks(w, a) {
        (k.place)(w, a, s + (alpha * (fx - s)), y);
    }
}

/// On a fighter: ReceiveBeginPlay @644 (the parent's first) finds an owner that is no SpawnedActorParent @739 ->
/// K2_DestroyActor @1820; its ReceiveEndPlay (@2376) does nothing more with Activated false.
fn fighter_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _duration: Option<Real>) {
    end_buff(w, f, b);
}

/// ReceiveBeginPlay (@644): the owner a SpawnedActorParent, else K2_DestroyActor @1820. Its
/// BPI_GetProjectileAttackType (0 without the interface) 0, 1 or 2, else nothing more (the buff sits out its
/// BuffDuration). Then Activated; the Instigator's BPI_SetIsEatingProjectile(true, owner) (a Taz: the BPI_C015 cast);
/// BPI_SetDestroyEffectsBlocked / BPI_SetDestroySpawnedActorBlocked(true) @1214 / @1251; SetActorEnableCollision(false)
/// @1289; ProjectileMovement.StopMovementImmediately @1322; StartingScale; the timeline's play rate 1 / ShrinkDownTime
/// @1576; the Instigator's facing @1716 (false without the interface); PlayFromStart @1762, whose
/// SetPlaybackPosition(0) fires the update at once.
///
/// A world actor owner (HarleyBomb: attack type 0, neither reflectable nor in SpecialProjectilesThatCanBeSpit, so the
/// eat refills the ability) has no destroy-blocked flags in the port: SpawnedActorParent's EndPlay reads them for
/// DeathVFX and SpawnActorAtDeath @15557 / @16070, which its class data does not set (warned where it does). Its
/// StopMovementImmediately is not ported: HarleyBomb's ProjectileMovement is deactivated, its simulating root keeps
/// its velocity (the timeline places it each tick).
// Deviation: an Item's StopMovementImmediately is taken as its velocity 0 while its movement is active (a simulating
// root keeps its own velocity).
// Unverified: the timeline's first advance is the actor's next buff pass (`combat::elem` actor_buff_tick, behind the
// actor's tick); the build's TG_PrePhysics tick of a component registered inside the frame is not read.
// Deviation: a world actor kind without carry hooks (C015Dogpile, HarleyJackInTheBox, the other SpawnedActorParent
// children of the world actors) is eaten in the build (@674-739 takes any SpawnedActorParent); the port has no place
// or collision write for it: refused, warned, the buff ended (the build's K2_DestroyActor of a non-SpawnedActorParent
// owner @1820).
fn begin(w: &mut World, h: ActorRef, i: u32, from: Option<FighterId>) -> bool {
    let Some(a) = eaten(w, h) else {
        warn("Buff_C015HitProjectile_C: an owner with no carry hooks (registry CarryHooks) not ported");
        return true;
    };
    let ty = attack_type_of(w, a);
    if ty != 0.0 && ty != 1.0 && ty != 2.0 {
        return false;
    }
    if let Some(x) = ext_mut(w, a, i) {
        x.act = Some(true);
    }
    if let Some(f) = from
        && taz_of(w, f).is_some()
    {
        set_is_eating_projectile(w, f, true, Some(a));
    }
    if !is_actor(a) {
        set_destroy_blocked(w, a, true, true);
        set_destroy_blocked(w, a, false, true);
    } else {
        let p = actor_p(w, a);
        if p.get("DeathVFX").is_some_and(truthy) || p.get("SpawnActorAtDeath").is_some_and(truthy) {
            warn("Buff_C015HitProjectile_C: its destroy-blocked flags not ported (DeathVFX / SpawnActorAtDeath)");
        }
    }
    if let Some(k) = carry_hooks(w, a) {
        (k.collision)(w, a, i, false);
    }
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                p.vx = 0.0;
                p.vy = 0.0;
            }
        }
        ActorRef::Item(id) => {
            if let Some(it) = w.actors.items.get_mut(id)
                && it.movement_active()
            {
                it.vx = 0.0;
                it.vy = 0.0;
            }
        }
        _ => {}
    }
    let sx = scale_of(w, a);
    let shrink = actor_inst_def(w, a, i).and_then(|d| num(&d, "ShrinkDownTime")).unwrap_or(1.0);
    let rate = 1.0 / shrink;
    let fl = from.is_some_and(|f| w.fighter(f).body.facing_dir < 0.0);
    let inst = from.map_or(-1, |f| w.fighter(f).id);
    let Some(x) = ext_mut(w, a, i) else { return false };
    x.sx = Some(sx);
    x.rate = Some(rate);
    x.fl = Some(fl);
    x.inst = Some(inst);
    x.t = Some(0.0);
    let x = *x;
    update(w, a, i, x);
    false
}

/// The timeline's tick; Finished (@15): the owner's ReleaseToPool, then the buff's K2_DestroyActor (its EndPlay). A
/// world actor owner: ReleaseToPool with CanBePooled false (HarleyBomb's) is K2_DestroyActor @23658.
// Deviation: ReleaseToPool is taken as the destroy (K2_DestroyActor without CanBePooled; a pooled class's return to
// the pool is not modelled apart).
fn tick(w: &mut World, h: ActorRef, i: u32) -> bool {
    let Some(x) = ext_mut(w, h, i) else { return false };
    let x = *x;
    let a = if x.act == Some(true) { eaten(w, h) } else { None };
    let Some(a) = a else { return false };
    let mut t = x.t.unwrap_or(0.0) + (DELTA_SECONDS * x.rate.unwrap_or(0.0));
    let done = t >= 1.0;
    if done {
        t = 1.0;
    }
    let Some(e) = ext_mut(w, a, i) else { return false };
    e.t = Some(t);
    let x = *e;
    update(w, a, i, x);
    if !done {
        return false;
    }
    if !actor_dead(w, a) {
        match a {
            ActorRef::Projectile(id) => crate::world::spawning::destroy_projectile(w, id, false),
            ActorRef::Item(id) => crate::world::items::destroy_item(w, id),
            _ => {
                if actor_p(w, a).get("CanBePooled").is_some_and(truthy) {
                    warn("Buff_C015HitProjectile_C: ReleaseToPool of a poolable class not ported");
                } else {
                    crate::actors::world_actor::mark_spawned_dead(w, a);
                }
            }
        }
    }
    true
}

/// ReceiveEndPlay (@2376 -> @66, a DoOnce while Activated; the owner's death ends it too: it is a child of the
/// owner's Comp_Actor_Parent, SpawnedActorParent EndPlay @15512, `combat::elem`'s end of the actor's children): the
/// Instigator's BPI_SetIsEatingProjectile(false, null); then, with authority and the Instigator's
/// Comp_Pawn_HitBoxHandler valid, pfgDoDamage(null, the owner, Damage) and K2_DestroyActor.
// Deviation: the pfgDoDamage on an owner already released (the timeline's end, the one path the port reaches) is
// taken as no hit; one on a live owner (the buff ended some other way) is not ported (warned).
// Unverified: a world actor owner (HarleyBomb) is released by K2_DestroyActor (SpawnedActorParent @23658), so this
// EndPlay runs inside its destroy (DestroyChildren @15512) and pfgDoDamage @481 meets an actor in its own EndPlay;
// whether its ReceiveAnyDamage still runs (HarleyBomb Detonate @6408, a blast on Taz) is not read: no hit.
fn end(w: &mut World, a: ActorRef, i: u32) {
    let Some(x) = ext_mut(w, a, i) else { return };
    if x.act != Some(true) {
        return;
    }
    x.act = Some(false);
    let x = *x;
    if let Some(f) = inst_of(w, &x)
        && taz_of(w, f).is_some()
    {
        set_is_eating_projectile(w, f, false, None);
    }
    if !actor_dead(w, a) {
        warn("Buff_C015HitProjectile_C EndPlay on a live owner: its pfgDoDamage on the owner not ported");
    }
}

/// This file's registry entries (Buff_C015HitProjectile_C, its fighter begin and its actor entry).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let actor = ActorBuffHooks { begin: Some(begin), tick: Some(tick), end: Some(end), ..ActorBuffHooks::default() };
    r.buffs.enter(
        HIT_PROJECTILE,
        BuffEntry {
            actor: Some(actor),
            ..BuffEntry::new(
                BuffClass::BuffC015HitProjectile,
                BuffHooks { begin: Some(fighter_begin), ..BuffHooks::default() },
            )
        },
    )
}
