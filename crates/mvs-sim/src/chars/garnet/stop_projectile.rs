//! Buff_GarnetStopProjectile, the stopped projectile: the Heavy test,
//! collision off and on, the added sphere (AdditionalHurtBox, radius FindScale + HurtboxAdditionalBuffer), Stationary
//! mobility, the reverse branch with its 1.0 s Delay (the projectile's reverse hook), the reflect by a hit
//! (`HitByEnemy`) or by an overlap of Garnet's team with ReflectHit (`HitboxOverlap`,
//! `combat::melee_actors` melee_hitbox_sphere_test), EndPlay's restore @1522-1774, TakenDamageModCondition, FindScale
//! over CheckedCollisionTypes; the overlap end it causes goes to sing.rs's sing_sphere_end_overlap.
//! An actor buff on a spawned actor's Comp_State_Manager (`combat::elem`); no pawn hook, no pawn state: its state is
//! the buff's extension [`StopExt`] (an `ElemExt` variant, so `Copy`).
//!
//! Buff_GarnetStopProjectile (a BuffParentActor child; CanOverride, TakenDamageMods Damage / KB / pause / stun
//! Multiply 0 with ReflectProjectiles, HurtboxAdditionalBuffer 50, NonReflectableProjectileBuffLifetime 1,
//! CheckedCollisionTypes [HitBox, WorldStatic, ItemDestructible, ProjectileDestructible, Item, PhysicsBody]).
//! - ReceiveBeginPlay @2942 (after the parent's): the owner no SpawnedActorParent -> nothing. Its
//!   BPI_GetProjectileAttackType 1 (Enum_ProjectileType NewEnumerator2 "Heavy") -> K2_DestroyActor @3412. Else with
//!   its Comp_Actor_Destructible.IsProjectile @3235 and CanBeReflected @3293: SetActorEnableCollision(false) @3329,
//!   then (authority) @927: AdjustedCompDestructible = true, the binds (ReflectedDispatch -> OnReflectedServer, the
//!   owner's Comp_Pawn_DamageManager HitByEnemy -> OnHitByEnemyServer), the buff put at the owner's location
//!   (teleport), FindScale, AdditionalHurtBox = AddComponent(sphere) on the buff itself, radius Scale +
//!   HurtboxAdditionalBuffer, its BeginOverlap bound -> OnHitboxOverlap, CreateVFX(Scale) (the view's:
//!   PS_Garnet_StopProjectile on the owner's root, Size Scale + 50, Team), MulticastMobility (the owner's root
//!   SetMobility(Stationary) @4363). Otherwise ReverseInsteadOfStop @3397 -> @4165: the owner's
//!   BPI_ReverseMoveDirection @4260, then (authority) @2271 CanOverride = false and
//!   Delay(NonReflectableProjectileBuffLifetime) @2312 -> @897 K2_DestroyActor.
//! - OnHitboxOverlap @15 (authority): OtherActor a BP_FighterCharacter with a valid Comp_Actor_Destructible, the
//!   Instigator's valid too, the same TeamIndex; a DoOnce; OnHitByEnemyServer(that fighter). The sphere's
//!   (NODE_AddSphereComponent-0's BodyInstance: ProjectileDestructible, QueryAndPhysics, Overlap HitBox alone) begin
//!   with any HitBox body of a fighter that overlaps ProjectileDestructible: a Hitframe's hit shapes and the pawn's
//!   always-on HitBox spheres (Garnet's own Sing Sphere: she sings, stops a shot, walks out of reach and back in, and
//!   it is reflected her way), once per begin (the overlaps kept in the ext's `keys`).
//! - OnHitByEnemyServer @2376: the attacker a BP_FighterCharacter with a valid Comp_ReflectProjectile: ReflectHit's
//!   KnockBackDirectionVector = (BPI_IsFacingLeft ? 1 : -1, 0) and ReflectProjectile(the owner, ReflectHit) @2862.
//! - OnReflectedServer @2371 -> @912: K2_DestroyActor.
//! - ReceiveEndPlay @1980: the VFX and AdditionalHurtBox destroyed, then @1522 (the owner a SpawnedActorParent) its
//!   root SetMobility(Movable) @1659, SetActorEnableCollision(true) @1697, Adjusted -> IsProjectile = true @1774 (it
//!   was: no change), the delegates removed.
//! - TakenDamageModCondition: the attacker is a BP_FighterCharacter.
//!
//! The sphere is the buff actor's, which has no Comp_Actor_Destructible: an enemy's hit box on it deals nothing
//! (pfgDoDamage's HitActor is the buff), so it is no hurt sphere of the owner; with its owner's collision off the
//! HitByEnemy binding has no hit to come from in the port either (Projectile no_collision, HurtBox radius 0).
// Unverified: an attacker's pfgDoDamage on the buff actor (HitActor without Comp_Actor_Destructible / DamageManager)
// is read as doing nothing; Comp_Pawn_HitBoxHandler's handling of such a HitActor past @923 is not followed through.

use mvs_data::value::{Props, Value};

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::actors::tunnel_carry::{CarryExt, carry_hooks};
use crate::actors::world_actor::{actor_elem, actor_elem_mut, actor_p, warn};
use crate::combat::buff_class::BuffClass;
use crate::combat::elem::{ElemBuff, ElemExt, actor_inst_def, elem_buff, elem_buff_mut};
use crate::combat::holds::set_root_mobility;
use crate::combat::phases::SphereKeys;
use crate::combat::reflect::{can_be_reflected, reflect_projectile};
use crate::combat::reverse::reverse_move_direction;
use crate::fighter::FighterId;
use crate::fighter::common::{actor_def, latent_frames, lookup_owner};
use crate::fighter::moves::V2;
use crate::fighter::traces::{body_info, ecc};
use crate::math::{max, min};
use crate::registry::entries::{ActorBuffHooks, BuffHooks};
use crate::registry::props::{num, truthy};
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal};
use crate::registry::{BuffEntry, Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::match_fighters_iter;

/// The class's name.
pub const STOP_PROJECTILE: &str = "Buff_GarnetStopProjectile_C";

/// Buff_GarnetStopProjectile_C's extension (its `Ext`, saved with the actor): `instigator` the Instigator's
/// fighter id (-1: none; sing.rs's end overlap reads it), `adjusted` AdjustedCompDestructible (the binds made), `x`
/// / `y` / `r` the AdditionalHurtBox (the buff's place, radius) and `find_scale` FindScale's (the VFX's Size is
/// find_scale + 50), all four written with `adjusted`; `done` the overlap's DoOnce, `delay_at` the reverse branch's
/// Delay (the world tick it fires at), `can_override` the instance's CanOverride (`combat::elem` spawn_actor_buff),
/// `keys` the AdditionalHurtBox's overlaps (its `o<fighter>.*` keys), `carry` the tunnel carry's `hurt`.
#[derive(State, Copy, PartialEq, Debug)]
pub struct StopExt {
    pub instigator: i32,
    pub adjusted: bool,
    pub x: Real,
    pub y: Real,
    pub r: Real,
    pub find_scale: Real,
    pub done: bool,
    pub delay_at: Option<Real>,
    pub can_override: Option<bool>,
    pub keys: SphereKeys,
    /// The tunnel carry's keys (`hurt`) on the same ext.
    pub carry: CarryExt,
}

impl Default for StopExt {
    fn default() -> Self {
        StopExt {
            instigator: -1,
            adjusted: false,
            x: 0.0,
            y: 0.0,
            r: 0.0,
            find_scale: 0.0,
            done: false,
            delay_at: None,
            can_override: None,
            keys: SphereKeys::default(),
            carry: CarryExt::default(),
        }
    }
}

// ---- the buff on its actor, by serial (the entry is held: a call may move it in the list)

fn entry_of(w: &World, a: ActorRef, serial: u32) -> Option<&ElemBuff> {
    actor_elem(w, a)?.as_ref()?.buffs.iter().find(|b| b.serial == serial)
}

fn ext_by_serial(w: &mut World, a: ActorRef, serial: u32) -> Option<&mut StopExt> {
    match &mut actor_elem_mut(w, a)?.as_mut()?.buffs.iter_mut().find(|b| b.serial == serial)?.ext {
        ElemExt::GarnetStopProjectile(e) => Some(e),
        _ => None,
    }
}

fn ext(w: &World, a: ActorRef, i: u32) -> Option<StopExt> {
    match elem_buff(w, a, i)?.ext {
        ElemExt::GarnetStopProjectile(e) => Some(e),
        _ => None,
    }
}

fn ext_mut(w: &mut World, a: ActorRef, i: u32) -> Option<&mut StopExt> {
    match &mut elem_buff_mut(w, a, i)?.ext {
        ElemExt::GarnetStopProjectile(e) => Some(e),
        _ => None,
    }
}

/// A Projectile or an Item (a Projectile or an Item).
fn is_pi(a: ActorRef) -> bool {
    matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_))
}

// ---- FindScale @0-2118
//
// Over the owner's SceneComponents, those of an object type in CheckedCollisionTypes: a sphere's
// GetScaledSphereRadius, a box's scaled extents, a capsule's scaled radius and half height (0 for the half height of
// one tagged HitBox2D @1815-1947); the largest, from 0.
// Unverified: the shapes are the data's fields (the root Capsule the port's live size; a HurtBox without its radius
// the native 32), the world scale the relative scales down the attach chain from the root's; a component of the data
// without a BodyInstance (its archetype's object type) is skipped.

const SHAPE_SKIP: [&str; 2] = ["StaticMesh", "SkeletalMesh"];

fn vec3(c: Option<&Props>, k: &str, d: Real) -> [Real; 3] {
    let v = c.and_then(|c| c.get(k)).and_then(Value::as_obj);
    let part = |n: &str| v.and_then(|v| num(v, n)).unwrap_or(d);
    [part("X"), part("Y"), part("Z")]
}

fn world_scale(comps: &Props, name: &str, root: Real, depth: i32) -> [Real; 3] {
    let c = comps.get(name).and_then(Value::as_obj);
    let rel = vec3(c, "RelativeScale3D", 1.0);
    if c.and_then(|c| c.get("bAbsoluteScale")).is_some_and(truthy) {
        return rel;
    }
    let parent = c.and_then(|c| c.get("AttachParent")).and_then(Value::as_str);
    let Some(parent) = parent.filter(|p| comps.get(*p).is_some_and(|v| !matches!(v, Value::Null)) && depth <= 8) else {
        return rel.map(|v| v * root);
    };
    let ps = world_scale(comps, parent, root, depth + 1);
    [(rel[0] * ps[0]), (rel[1] * ps[1]), (rel[2] * ps[2])]
}

/// `findScale`: FindScale of a projectile or an item with the buff's values `d` (0 for another kind).
pub fn find_scale(w: &World, a: ActorRef, d: Option<&Props>) -> Real {
    let (cls, scale, cap_r, cap_h) = match a {
        ActorRef::Projectile(id) => {
            let Some(p) = w.actors.projectiles.get(id) else { return 0.0 };
            use crate::actors::projectile::{capsule_half_height, capsule_radius};
            (p.cls, p.scale, capsule_radius(p), capsule_half_height(p))
        }
        ActorRef::Item(id) => {
            let Some(i) = w.actors.items.get(id) else { return 0.0 };
            (i.cls, i.scale, i.capsule_radius(), i.capsule_half_height())
        }
        _ => return 0.0,
    };
    let Some(comps) = actor_def(w, ActorRef::Fighter(lookup_owner(w, a)), cls).map(|x| &x.components) else {
        return 0.0;
    };
    let types: Vec<i32> = d
        .and_then(|d| d.get("CheckedCollisionTypes"))
        .and_then(Value::as_arr)
        .map(|l| l.iter().filter_map(|n| n.as_str().and_then(ecc)).collect())
        .unwrap_or_default();
    let mut m: Real = 0.0;
    for (name, c) in comps {
        let Some(c) = c.as_obj() else { continue };
        let info = if c.get("BodyInstance").is_some() { body_info(Some(c), None) } else { None };
        let Some(info) = info else { continue };
        if !types.contains(&info.kind) || SHAPE_SKIP.iter().any(|k| c.get(*k).is_some()) {
            continue;
        }
        let flat = c
            .get("ComponentTags")
            .and_then(Value::as_arr)
            .is_some_and(|t| t.iter().any(|x| x.as_str() == Some("HitBox2D")));
        let s = world_scale(comps, name, scale, 0);
        if name == "Capsule" && !c.get("AttachParent").is_some_and(truthy) {
            m = max(max(m, cap_r), if flat { 0.0 } else { cap_h });
        } else if c.get("SphereRadius").is_some() || name == "HurtBox" {
            let k = min(min(s[0].abs(), s[1].abs()), s[2].abs());
            m = max(m, num(c, "SphereRadius").unwrap_or(32.0) * k);
        } else if c.get("BoxExtent").is_some() {
            let e = vec3(Some(c), "BoxExtent", 32.0);
            m = max(max(max(m, e[0] * s[0]), e[1] * s[1]), e[2] * s[2]);
        } else if c.get("CapsuleRadius").is_some() || c.get("CapsuleHalfHeight").is_some() {
            // GetScaledCapsuleSize: radius x min(|X|, |Y|), half height x |Z|.
            let r = num(c, "CapsuleRadius").unwrap_or(22.0) * min(s[0].abs(), s[1].abs());
            let h = if flat { 0.0 } else { num(c, "CapsuleHalfHeight").unwrap_or(44.0) * s[2].abs() };
            m = max(max(m, r), h);
        } else {
            warn("Buff_GarnetStopProjectile_C: FindScale: a component has no shape the port reads (skipped)");
        }
    }
    m
}

/// SetActorEnableCollision(false) ends every overlap of the actor inside the call: the Sing Sphere's
/// ComponentEndOverlap (Pawn_Garnet @13578 -> @8835) of each Garnet whose sphere held it (her
/// OverlappingSingActors). The buff is no child of the actor yet in its BeginPlay (Comp_State_Manager SpawnBuffActor:
/// FinishSpawningActor @933, AddChildServer @1192), so @8835's CheckHasBuff finds it not.
/// Here its Instigator is none for the calls, which @8835's test reads the same (the only
/// buff of the class on the actor: SweepBuffState destroyed or refused another).
/// Deviation: the end overlaps of the actor's other overlaps (another sphere's, a hit box's overlap set) are not sent.
fn end_sing_overlaps(w: &mut World, a: ActorRef, serial: u32) {
    let Some(inst) = ext_by_serial(w, a, serial).map(|x| std::mem::replace(&mut x.instigator, -1)) else { return };
    let gs: Vec<FighterId> = match_fighters_iter(w)
        .filter(|&g| crate::chars::garnet::garnet_of(w, g).is_some_and(|s| s.sing.overlapping_sing_actors.contains(&a)))
        .collect();
    for g in gs {
        if crate::chars::garnet::garnet_of(w, g).is_some_and(|s| s.sing.overlapping_sing_actors.contains(&a)) {
            super::sing::sing_sphere_end_overlap(w, g, a);
        }
    }
    if let Some(x) = ext_by_serial(w, a, serial) {
        x.instigator = inst;
    }
}

/// BPI_GetProjectileAttackType of the owner: a projectile's and an item's own, a world actor's class default.
fn attack_type(w: &World, a: ActorRef) -> Real {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or(0.0, |p| crate::real(p.attack_type)),
        ActorRef::Item(id) => crate::real(crate::actors::item::item_attack_type(w, id)),
        _ => num(actor_p(w, a), "ProjectileAttackType").unwrap_or(0.0),
    }
}

/// Comp_Actor_Destructible.IsProjectile: an item's own, else the class data's.
fn is_projectile(w: &World, a: ActorRef) -> bool {
    if let ActorRef::Item(id) = a {
        return crate::actors::item::item_is_projectile(w, id);
    }
    let Some(cls) = crate::actors::world_actor::actor_cls(w, a) else { return false };
    actor_def(w, ActorRef::Fighter(lookup_owner(w, a)), cls)
        .and_then(|d| d.components.get("Comp_Actor_Destructible"))
        .and_then(Value::as_obj)
        .and_then(|c| c.get("IsProjectile"))
        .is_some_and(truthy)
}

fn begin(w: &mut World, a: ActorRef, i: u32, from: Option<FighterId>) -> bool {
    let Some(serial) = elem_buff(w, a, i).map(|b| b.serial) else { return false };
    let instigator = from.map_or(-1, |f| w.fighter(f).id);
    if let Some(b) = elem_buff_mut(w, a, i) {
        b.ext = ElemExt::GarnetStopProjectile(StopExt { instigator, ..StopExt::default() });
    }
    // (every spawned actor of the port is a SpawnedActorParent: the cast @2972 passes)
    if attack_type(w, a) == 1.0 {
        return true; // @3412
    }
    let (buffer, lifetime) = {
        let d = actor_inst_def(w, a, i);
        let d = d.as_deref();
        (
            d.and_then(|d| num(d, "HurtboxAdditionalBuffer")).unwrap_or(0.0),
            d.and_then(|d| num(d, "NonReflectableProjectileBuffLifetime")).unwrap_or(1.0),
        )
    };
    if is_projectile(w, a) && can_be_reflected(w, a) {
        if !is_pi(a) {
            warn("Buff_GarnetStopProjectile_C on a world actor: the stop is not ported (it idles)");
            return false;
        }
        if let Some(k) = carry_hooks(w, a) {
            (k.collision)(w, a, i, false); // @3329
        }
        end_sing_overlaps(w, a, serial);
        let Some(i) = crate::combat::elem::buff_place(w, a, serial) else { return false }; // (destroyed by a call above)
        // @927 (authority)
        let (x, y) = match a {
            ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or((0.0, 0.0), |p| (p.x, p.y)),
            ActorRef::Item(id) => w.actors.items.get(id).map_or((0.0, 0.0), |p| (p.x, p.y)),
            _ => (0.0, 0.0),
        };
        let scale = {
            let d = actor_inst_def(w, a, i);
            find_scale(w, a, d.as_deref())
        };
        if let Some(e) = ext_mut(w, a, i) {
            e.adjusted = true;
            // K2_SetActorLocation(the owner's location, teleport) @1138: the sphere's place (no attach)
            e.x = x;
            e.y = y;
            e.find_scale = scale; // (the view adds HurtboxAdditionalBuffer)
            e.r = scale + buffer;
        }
        // SetSphereRadius(r, bUpdateOverlaps true) @1378 ahead of the bind @1443: the HitBox components the sphere
        // meets now begin no OnHitboxOverlap (the Sing Sphere that stopped the shot is one).
        crate::combat::melee_actors::hitbox_sphere_begin(w, a, i);
        set_mobility(w, a, 1); // MulticastMobility @4363
        return false;
    }
    // ReverseInsteadOfStop @4165
    reverse_move_direction(w, a);
    let at = crate::real(w.flow.tick) + latent_frames(lifetime);
    if let Some(e) = ext_by_serial(w, a, serial) {
        e.can_override = Some(false); // @2301
        e.delay_at = Some(at); // @2312 -> @897
    }
    false
}

fn set_mobility(w: &mut World, a: ActorRef, m: u8) {
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                set_root_mobility(p, m);
            }
        }
        ActorRef::Item(id) => {
            if let Some(p) = w.actors.items.get_mut(id) {
                set_root_mobility(p, m);
            }
        }
        _ => {}
    }
}

/// OnHitByEnemyServer @2376 (the attacker passed the cast to BP_FighterCharacter: every fighter). reflect.rs warns
/// for a fighter without Comp_ReflectProjectile, where the build's IsValid pops.
fn hit_by_enemy(w: &mut World, a: ActorRef, by: FighterId) {
    // (BPI_IsFacingLeft ? 1 : -1, 0) @2666-2755
    let x = if w.fighter(by).body.facing_dir < 0.0 { 1.0 } else { -1.0 };
    reflect_projectile(w, by, a, V2 { x, y: 0.0 }, None);
}

/// The reverse branch's Delay; the collision-off's HurtBox kept at 0 against the class's own writes of it.
fn tick(w: &mut World, a: ActorRef, i: u32) -> bool {
    let Some(x) = ext(w, a, i) else { return false };
    if x.delay_at.is_some_and(|at| crate::real(w.flow.tick) >= at) {
        return true;
    }
    if x.adjusted
        && is_pi(a)
        && let Some(t) = carry_hooks(w, a).and_then(|k| k.tick)
    {
        t(w, a, i);
    }
    false
}

fn hitbox_sphere(w: &World, a: ActorRef, i: u32) -> Option<[Real; 3]> {
    let x = ext(w, a, i)?;
    if !x.adjusted || x.r.is_nan() || x.r <= 0.0 {
        return None;
    }
    Some([x.x, x.y, x.r])
}

/// The attacker is a fighter on every caller (`combat::elem` actor_taken_damage).
fn taken_cond(_w: &World, _a: ActorRef, _i: u32, _by: FighterId) -> bool {
    true
}

fn on(w: &mut World, a: ActorRef, i: u32, s: ActorBuffSignal, arg: Option<&ActorBuffArg>) -> bool {
    let Some(x) = ext(w, a, i) else { return false };
    if !x.adjusted || !is_pi(a) {
        return false; // not bound
    }
    if s == ActorBuffSignal::Reflected {
        return true; // OnReflectedServer: K2_DestroyActor
    }
    let Some(arg) = arg else { return false };
    match s {
        ActorBuffSignal::HitByEnemy => hit_by_enemy(w, a, arg.by),
        ActorBuffSignal::HitboxOverlap => {
            // OnHitboxOverlap @15: the Instigator's and the fighter's Comp_Actor_Destructible, the same TeamIndex.
            let inst = match_fighters_iter(w).find(|&f| w.fighter(f).id == x.instigator);
            let Some(inst) = inst else { return false };
            if w.fighter(inst).team.team != w.fighter(arg.by).team.team || x.done {
                return false;
            }
            if let Some(e) = ext_mut(w, a, i) {
                e.done = true; // the DoOnce @534
            }
            hit_by_enemy(w, a, arg.by);
        }
        _ => {}
    }
    false
}

/// @1522: the owner's root Movable, its collision on (the HurtBox's radius back), whatever the branch was.
fn end(w: &mut World, a: ActorRef, i: u32) {
    if !is_pi(a) {
        return;
    }
    set_mobility(w, a, 2);
    if let Some(k) = carry_hooks(w, a) {
        (k.collision)(w, a, i, true);
    }
}

/// The extension of the actor's buff `serial` (the harness tests read it).
pub fn stop_ext_of(w: &World, a: ActorRef, serial: u32) -> Option<StopExt> {
    match entry_of(w, a, serial)?.ext {
        ElemExt::GarnetStopProjectile(e) => Some(e),
        _ => None,
    }
}

/// This file's registry entries (Buff_GarnetStopProjectile_C's actor entry).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let actor = ActorBuffHooks {
        begin: Some(begin),
        tick: Some(tick),
        on: Some(on),
        end: Some(end),
        hitbox_sphere: Some(hitbox_sphere),
        taken_cond: Some(taken_cond),
        ..ActorBuffHooks::default()
    };
    r.buffs.enter(
        STOP_PROJECTILE,
        BuffEntry { actor: Some(actor), ..BuffEntry::new(BuffClass::BuffGarnetStopProjectile, BuffHooks::default()) },
    )
}
