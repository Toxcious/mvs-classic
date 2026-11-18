//! Marvin's bubble: C018BubbleProj_C ([`BubbleProj`], a projectile that
//! captures another), C018BubblePop_C (a plain projectile), Buff_C018BubbledProjectile_C (an actor buff on the captured
//! actor: [`BubbledExt`]) and Buff_C018BubbleHandleIncomingDamage_C (an actor buff whose takenCond is false). No pawn
//! hook and no pawn state: the state is the BubbleProj's and the buff's ext.
//
// C018BubbleProj, by entry:
// - ReceiveBeginPlay @1400: the three sounds on the actor ([`begin`]). Its
//   Comp_Pawn_PassiveBuffManager row (Buff_C018BubbleHandleIncomingDamage_C, BuffSelf) is the spawn's passive buffs.
// - The ProjectileMovement's OnProjectileBounce @1271 (bShouldBounce, Bounciness 1): the bounce sound ([`bounce`]).
// - The HitBox's begin overlap @1185 / HitBox2D's @1606 -> HandleProjectileOverlap @1663: a valid BubbledProjectile ->
//   nothing; the actor not a SpawnedActorParent, or a C018BubbleProj -> nothing; the Instigator's
//   BPI_IsViableForProjectileTweaks false -> nothing. The same TeamIndex -> @2144; another team: the actor's
//   BPI_CanBeReflected and the Instigator's Comp_ReflectProjectile valid, else nothing; then ReflectProjectile(the
//   actor, a zero hit struct) @2462, Success not read -> @2144: FindScale(the actor), MulticastBubbledProjectile(the
//   actor, Scale), HitBoxDamage.ReflectProjectiles = false, UpdateHitbox.
// - MulticastBubbledProjectile @297: HasProjectile = true, SetActorScale3D(GetActorScale3D x Scale) @382; (authority)
//   BubbledProjectile = the actor, its Comp_State_Manager.SpawnBuffActor(Buff_C018BubbledProjectile_C, the actor, the
//   Instigator, 0, 0.0) @545, ServerSetBubbled(self) on it @726, BubbledBuff = it.
// - ReceiveEndPlay @783 (authority): BubbledProjectile and BubbledBuff valid -> BubbledBuff.K2_DestroyActor @915; then
//   the Instigator's BeginAlternateCooldownOnAuthority(TriggerAbilityIndex, 0, true) @1133.
// - FindScale: LocalMaxScale = MinimumScale; over the actor's SceneComponents of an object type in Checked Collision
//   Types (garnet's `find_scale`); Scale = LocalMaxScale / the bubble's Capsule.GetScaledCapsuleRadius() @725-821.
// Unverified: the capture runs ahead of the hit box handler's pfgDoDamage on the same begin (a ComponentBoundEvent is
// bound in the construction, the handler binds later: multicast delegates are called in bind order); not a native
// read of this pair.
// Deviation: the begin is the port's projectile pass (the bubble's hit shape over the actor's HurtBox or a buff's
// sphere, after every move of the frame) and its item pass; a World.actors actor is not tested.
// Deviation: FindScale's MinimumScale and the shapes: garnet's `find_scale` reads the shapes, with the bubble's
// "Checked Collision Types" given to it as its CheckedCollisionTypes.

use super::pawn::is_viable_for_projectile_tweaks;
use super::state::marvin_of;
use crate::Real;
use crate::State;
use crate::actors::item::{is_owned, item_attack_type};
use crate::actors::projectile::{capsule_radius, class_p, hit_velocity, set_actor_scale, truthy};
use crate::actors::tunnel_carry::{CarryExt, carry_hooks};
use crate::actors::world_actor::{actor_dead, actor_elem, actor_p};
use crate::actors::{ActorClass, ActorRef, ProjectileClass, ProjectileExt, ProjectileId};
use crate::combat::BuffClass;
use crate::combat::buffs::mod_type;
use crate::combat::elem::{ElemExt, elem_buff, elem_buff_mut, end_actor_buff, spawn_actor_buff};
use crate::combat::holds::set_mobility_to_stationary;
use crate::combat::reflect::{can_be_reflected, reflect_projectile};
use crate::events::{Event, SoundKey};
use crate::fighter::FighterId;
use crate::fighter::common::{buff_def, owner_team};
use crate::fighter::control::begin_alternate_cooldown;
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::math::to_int32;
use crate::name;
use crate::registry::entries::{ActorBuffHooks, ActorEntry, ActorHooks, BuffEntry, BuffHooks, ProjectileHooks};
use crate::registry::props::num;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

/// The bubble's class.
pub const BUBBLE: &str = "C018BubbleProj_C";

/// The bubble's pop.
pub const BUBBLE_POP: &str = "C018BubblePop_C";

/// Buff_C018BubbledProjectile's class.
pub const BUBBLED_PROJECTILE: &str = "Buff_C018BubbledProjectile_C";

/// Buff_C018BubbleHandleIncomingDamage's class.
pub const BUBBLE_INCOMING_DAMAGE: &str = "Buff_C018BubbleHandleIncomingDamage_C";

/// `BubbleProj extends Projectile` (C018BubbleProj_C).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct BubbleProj {
    /// BubbledProjectile: the actor held in it (dead once it is gone: IsValid).
    pub captured: Option<ActorRef>,
    /// HasProjectile (@297): written, read by the drawing alone.
    pub has_projectile: bool,
}

/// Buff_C018BubbledProjectile_C's extension (`ElemBuff.ext`, `ElemExt::C018Bubbled`): `carry` what the carry keeps
/// (`hurt`, the HurtBox radius the collision-off keeps, and the paused timer; tunnel_carry.rs, read through
/// `ElemExt::carry`), `bubble` the bubble's `seq` (MulticastSetBubbled's Bubble, the attach parent), `vx` / `vy`
/// VelocityBeforeStop, `activated` Activated, `wait` the Delay(0) retry @671 pending.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct BubbledExt {
    pub carry: CarryExt,
    pub bubble: Option<i32>,
    pub vx: Option<Real>,
    pub vy: Option<Real>,
    pub activated: bool,
    pub wait: bool,
}

fn bubble_ref(w: &World, id: ProjectileId) -> Option<&BubbleProj> {
    match &w.actors.projectiles.get(id)?.ext {
        ProjectileExt::BubbleProj(b) => Some(b),
        _ => None,
    }
}

fn bubble_mut(w: &mut World, id: ProjectileId) -> Option<&mut BubbleProj> {
    match &mut w.actors.projectiles.get_mut(id)?.ext {
        ProjectileExt::BubbleProj(b) => Some(b),
        _ => None,
    }
}

fn ext_ref(w: &World, a: ActorRef, i: u32) -> Option<&BubbledExt> {
    match &elem_buff(w, a, i)?.ext {
        ElemExt::C018Bubbled(e) => Some(e),
        _ => None,
    }
}

fn ext_mut(w: &mut World, a: ActorRef, i: u32) -> Option<&mut BubbledExt> {
    match &mut elem_buff_mut(w, a, i)?.ext {
        ElemExt::C018Bubbled(e) => Some(e),
        _ => None,
    }
}

/// `valid(pr.captured)`: the held actor, while it is not dead.
fn captured(w: &World, id: ProjectileId) -> Option<ActorRef> {
    bubble_ref(w, id)?.captured.filter(|&a| !actor_dead(w, a))
}

/// `bubbledBuff`: the buff the capture spawned (its ext.bubble the bubble's id), while it lives: the held actor and
/// the buff's place in its list.
fn bubbled_buff(w: &World, id: ProjectileId) -> Option<(ActorRef, u32)> {
    let t = captured(w, id)?;
    let seq = w.actors.projectiles.get(id)?.seq;
    let st = actor_elem(w, t)?.as_ref()?;
    let i = st.buffs.iter().position(|b| {
        b.class == BuffClass::BuffC018BubbledProjectile
            && matches!(&b.ext, ElemExt::C018Bubbled(e) if e.bubble == Some(seq))
    })?;
    Some((t, u32::try_from(i).ok()?))
}

/// The Instigator's Comp_ReflectProjectile is valid: the pawn's or BaseFighter's component in the data (as
/// `combat::reflect` reads it).
fn has_reflect_comp(w: &World, f: FighterId) -> bool {
    const COMP: &str = "Comp_ReflectProjectile_GEN_VARIABLE";
    let Some(c) = w.data.chars.get(w.names.str(w.fighter(f).ch)) else { return false };
    let comps = &c.fighter.components;
    [&comps.pawn, &comps.base].iter().any(|m| m.get(COMP).is_some_and(|c| c.p.as_obj().is_some()))
}

/// FindScale (the function @0-2161): garnet's `find_scale` from MinimumScale, over the bubble's own radius.
fn find_scale(w: &World, id: ProjectileId, t: ActorRef) -> Real {
    let Some(pr) = w.actors.projectiles.get(id) else { return 0.0 };
    let p = class_p(w, pr);
    let mut d = Props::new();
    if let Some(v) = p.get("Checked Collision Types") {
        d.insert("CheckedCollisionTypes".to_owned(), v.clone());
    }
    let max = crate::math::max(
        num(p, "MinimumScale").unwrap_or(0.0),
        crate::chars::garnet::stop_projectile::find_scale(w, t, Some(&d)),
    );
    let own = capsule_radius(pr);
    if own > 0.0 { max / own } else { 0.0 }
}

/// A spawned actor with an owner (`t.owner`): a projectile's or an item's.
fn owned(w: &World, t: ActorRef) -> bool {
    match t {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).is_some_and(|p| p.owner.is_some()),
        ActorRef::Item(id) => w.actors.items.get(id).is_some() && is_owned(w, id),
        _ => false,
    }
}

/// `bubbleKind.hitboxBeginOverlap`: HandleProjectileOverlap @1663.
fn hitbox_begin_overlap(w: &mut World, pr: ProjectileId, other: ActorRef) {
    if captured(w, pr).is_some() || actor_dead(w, other) || !owned(w, other) {
        return; // IsValid(BubbledProjectile)
    }
    if actor_dead(w, ActorRef::Projectile(pr)) {
        return;
    }
    if matches!(other, ActorRef::Projectile(p) if w.actors.projectiles.get(p).is_some_and(|p| p.cls == name!("C018BubbleProj_C")))
    {
        return; // the cast to C018BubbleProj
    }
    let Some(inst) = w.actors.projectiles.get(pr).and_then(|p| p.owner) else { return };
    // BPI_IsViableForProjectileTweaks of the Instigator (BPI_C018: Pawn_C018 alone).
    if marvin_of(w, inst).is_none() || !is_viable_for_projectile_tweaks(w, other) {
        return;
    }
    if w.fighter(inst).team.team != owner_team(w, other) {
        if !can_be_reflected(w, other) || !has_reflect_comp(w, inst) {
            return;
        }
        reflect_projectile(w, inst, other, V2 { x: 0.0, y: 0.0 }, None); // @2462, Success not read
        if actor_dead(w, ActorRef::Projectile(pr)) || actor_dead(w, other) {
            return;
        }
    }
    let s = find_scale(w, pr, other); // @2144
    bubbled(w, pr, other, s);
    if let Some(p) = w.actors.projectiles.get_mut(pr) {
        p.hit.reflect_projectiles = Some(false); // HitBoxDamage.ReflectProjectiles = false, UpdateHitbox
    }
}

/// MulticastBubbledProjectile @297.
fn bubbled(w: &mut World, pr: ProjectileId, t: ActorRef, s: Real) {
    let Some(p) = w.actors.projectiles.get_mut(pr) else { return };
    let seq = p.seq;
    let owner = p.owner;
    if let Some(b) = bubble_mut_of(&mut p.ext) {
        b.has_projectile = true;
    }
    set_actor_scale(p, p.scale * s);
    if let Some(b) = bubble_mut(w, pr) {
        b.captured = Some(t); // @506 (the cast to SpawnedActorParent: every actor of the port)
    }
    let Some(serial) = spawn_actor_buff(w, t, name!("Buff_C018BubbledProjectile_C"), 0, 0.0, true, owner) else {
        return; // @545
    };
    let Some(i) = crate::combat::elem::buff_place(w, t, serial) else { return };
    if let Some(e) = ext_mut(w, t, i) {
        e.bubble = Some(seq);
    }
    if set_bubbled(w, t, i, Some(pr)) {
        end_actor_buff(w, t, serial); // ServerSetBubbled(self) @726 -> MulticastSetBubbled @1344
    }
}

fn bubble_mut_of(e: &mut ProjectileExt) -> Option<&mut BubbleProj> {
    match e {
        ProjectileExt::BubbleProj(b) => Some(b),
        _ => None,
    }
}

// Buff_C018BubbledProjectile.
// - ServerSetBubbled @1504: a valid Bubble -> MulticastSetBubbled, else K2_DestroyActor.
// - MulticastSetBubbled @1344: the owner a SpawnedActorParent; with its EnableProjectileMovement @30: GetVelocity
//   exactly zero -> Delay(0) @671 and @30 again; else (or without EnableProjectileMovement) @140: VelocityBeforeStop =
//   GetVelocity, BPI_SetCollisionEnabled(false), BPI_SetMobilityToStationary(true), K2_AttachToActor(Bubble, None,
//   SnapToTarget location, KeepWorld rotation and scale, weld), Activated = true, (authority) BPI_SetLifetimePaused(true).
// - ReceiveTick @1586: Activated and the owner's GetAttachParentActor not valid -> K2_DestroyActor.
// - ReceiveEndPlay @1480 -> @726: BPI_SetCollisionEnabled(true), BPI_SetMobilityToStationary(false),
//   K2_DetachFromActor(KeepWorld), with EnableProjectileMovement ProjectileMovement.Velocity = VelocityBeforeStop
//   (zero for a buff that never activated), (authority) BPI_SetLifetimePaused(false).
// Unverified: GetVelocity (the root's ComponentVelocity) is `hit_velocity` (the velocity of the actor's last tick: a
// reflect's write of the movement's Velocity in the capture is not in it, as in the build) and an item's (vx, vy).
// Deviation: SpawnedActorParent's CollisionDisableCount and LifetimePausedCount are not kept (tunnel-carry's
// collision / pause_life set and clear), the StationaryMobilityCount is (holds.rs).
// Deviation: the Delay(0) retry is taken in the actor's buff pass of the next frame (`actor_buff_tick`).

fn velocity_of(w: &World, t: ActorRef) -> (Real, Real) {
    match t {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or((0.0, 0.0), hit_velocity),
        ActorRef::Item(id) => w.actors.items.get(id).map_or((0.0, 0.0), |i| (i.vx, i.vy)),
        _ => (0.0, 0.0),
    }
}

fn set_velocity(w: &mut World, t: ActorRef, vx: Real, vy: Real) {
    match t {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                p.vx = vx;
                p.vy = vy;
            }
        }
        ActorRef::Item(id) => {
            if let Some(p) = w.actors.items.get_mut(id) {
                p.vx = vx;
                p.vy = vy;
            }
        }
        _ => {}
    }
}

fn enable_movement(w: &World, t: ActorRef) -> bool {
    truthy(actor_p(w, t).get("EnableProjectileMovement"))
}

/// `setBubbled`: ServerSetBubbled / MulticastSetBubbled on the buff `i` of `t`. True: K2_DestroyActor (the bubble
/// gone).
fn set_bubbled(w: &mut World, t: ActorRef, i: u32, bubble: Option<ProjectileId>) -> bool {
    let Some(b) = bubble.filter(|&b| w.actors.projectiles.get(b).is_some_and(|p| !p.dead)) else {
        return true; // @1571
    };
    let (vx, vy) = velocity_of(w, t);
    if enable_movement(w, t) && vx == 0.0 && vy == 0.0 {
        if let Some(e) = ext_mut(w, t, i) {
            e.wait = true; // Delay(0) @671
        }
        return false;
    }
    if let Some(e) = ext_mut(w, t, i) {
        e.wait = false;
        e.vx = Some(vx);
        e.vy = Some(vy);
    }
    let c = carry_hooks(w, t);
    if let Some(c) = c {
        (c.collision)(w, t, i, false); // BPI_SetCollisionEnabled(false)
    }
    set_mobility_to_stationary(w, t, true);
    let (bx, by) = w.actors.projectiles.get(b).map_or((0.0, 0.0), |p| (p.x, p.y));
    if let Some(c) = c {
        (c.place)(w, t, bx, by); // K2_AttachToActor, SnapToTarget location
    }
    if let Some(e) = ext_mut(w, t, i) {
        e.activated = true;
    }
    if let Some(c) = c {
        (c.pause_life)(w, t, i, true); // BPI_SetLifetimePaused(true)
    }
    false
}

/// `bubbleOf`: the bubble the buff's ext names.
fn bubble_of(w: &World, a: ActorRef, i: u32) -> Option<ProjectileId> {
    let seq = ext_ref(w, a, i)?.bubble?;
    w.actors
        .projectiles
        .iter()
        .find(|(_, p)| p.seq == seq && matches!(p.ext, ProjectileExt::BubbleProj(_)))
        .map(|(id, _)| id)
}

/// `bubbleKind.make`.
fn make(_w: &World, _f: FighterId, _r: &SpawnRequest) -> ProjectileExt {
    ProjectileExt::BubbleProj(BubbleProj::default())
}

/// The key of what the bubble posts on itself.
fn self_key(w: &World, pr: ProjectileId) -> SoundKey {
    SoundKey { tag: name!("proj"), n: w.actors.projectiles.get(pr).map_or(-1, |p| p.seq) }
}

/// ReceiveBeginPlay @1400: PF_Post_Event(.., self, self) of sfx_cha_C018_bubblegun_cast_large_bubble @1410, its _tail
/// @1475 and its _missle_loop @1540, all three on the actor: its destroy stops them ([`end`]).
fn begin(w: &mut World, pr: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let (fighter, key) = (p.owner_id, self_key(w, pr));
    for ev in
        [name!("sfx_cha_C018_bubblegun_cast_large_bubble"), name!("sfx_cha_C018_bubblegun_cast_large_bubble_tail")]
    {
        w.events.push(Event::Sound { fighter, ev, x: None, y: None, hit: None, pooled: false, key: Some(key) });
    }
    let ev = name!("sfx_cha_C018_bubblegun_cast_large_bubble_missle_loop");
    w.events.push(Event::SoundStart { fighter, ev, key });
}

/// OnProjectileBounce @1271: sfx_cha_C018_bubblegun_projectile_bounce at the ParticleSystem's location (at the root:
/// no RelativeLocation here or in ParentProjectilePhysics) on a pooled component (PF_Post_Event_At_Location @1321).
fn bounce(w: &mut World, pr: ProjectileId, _nx: Real, _ny: Real, _ivx: Real, _ivy: Real) -> bool {
    let Some(p) = w.actors.projectiles.get(pr) else { return false };
    let (x, y) = (p.x, p.y);
    // (Instigator null @1321: the plain name on a pooled component)
    crate::fighter::common::sound_at_for(w, None, name!("sfx_cha_C018_bubblegun_projectile_bounce"), x, y);
    false
}

/// The actor's destroy: PF_Post_Event set StopWhenOwnerDestroyed, so what [`begin`] posted on it stops (StopAll, taken
/// as no fade).
fn end(w: &mut World, pr: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let (fighter, key) = (p.owner_id, self_key(w, pr));
    w.events.push(Event::SoundStop { fighter, key, fade: Some(0.0), ev: None, curve: None, cut: false });
}

/// `bubbleKind.moved`: the captured actor rides the bubble's root (SnapToTarget): placed behind the bubble's move.
fn moved(w: &mut World, pr: ProjectileId) {
    let Some(t) = captured(w, pr) else { return };
    let Some((_, i)) = bubbled_buff(w, pr) else { return };
    if !ext_ref(w, t, i).is_some_and(|e| e.activated) {
        return;
    }
    let (x, y) = w.actors.projectiles.get(pr).map_or((0.0, 0.0), |p| (p.x, p.y));
    if let Some(c) = carry_hooks(w, t) {
        (c.place)(w, t, x, y);
    }
}

/// Buff_C018BubbleHandleIncomingDamage TakenDamageModCondition: the BuffOwner a C018BubbleProj with no valid
/// BubbledProjectile, the Attacker a SpawnedActorParent whose BPI_GetProjectileAttackType is not 1 (Heavy) and whose
/// BPI_CanBeReflected holds. Its TakenDamageMods: Damage and KnockBack Multiply 0.
// Deviation: a World.actors dealer (a DealerActor) is not counted as a SpawnedActorParent here.
fn incoming_holds(w: &World, pr: ProjectileId, attacker: ActorRef) -> bool {
    if captured(w, pr).is_some() {
        return false;
    }
    if !matches!(attacker, ActorRef::Projectile(_) | ActorRef::Item(_)) {
        return false;
    }
    if attack_type_of(w, attacker) == 1 {
        return false;
    }
    can_be_reflected(w, attacker)
}

fn attack_type_of(w: &World, a: ActorRef) -> i32 {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or(0, |p| p.attack_type),
        ActorRef::Item(id) if w.actors.items.contains(id) => item_attack_type(w, id),
        _ => 0,
    }
}

/// ReceiveAnyDamage on the IsProjectile branch, with the taken pass of a spawned dealer's hit: DamageAndLaunch's taken
/// pass (@1878) of the bubble's own buff, which `actor_taken_damage` runs for a fighter dealer alone (whose takenCond
/// is false: the cast to SpawnedActorParent fails), then ParentProjectilePhysics' ReceiveAnyDamage. A damage the pass
/// makes 0 has no ApplyDamage.
// Deviation: the taken pass for a spawned dealer is run here for this buff alone; another buff on the bubble (a
// perk's grant) takes no part in it, as on every projectile victim of a spawned dealer in the port.
fn any_damage(w: &mut World, pr: ProjectileId, dmg: Real, causer: ActorRef) -> bool {
    let mut x = dmg;
    let has = actor_elem(w, ActorRef::Projectile(pr))
        .and_then(|s| s.as_ref())
        .is_some_and(|s| s.buffs.iter().any(|b| b.class == BuffClass::BuffC018BubbleHandleIncomingDamage));
    let owner = w.actors.projectiles.get(pr).and_then(|p| p.owner);
    if has
        && let Some(owner) = owner
        && incoming_holds(w, pr, causer)
    {
        let m = buff_def(w, ActorRef::Fighter(owner), name!("Buff_C018BubbleHandleIncomingDamage_C"))
            .and_then(|d| d.get("TakenDamageMods"))
            .and_then(Value::as_obj);
        let t = mod_type(m.and_then(|m| m.get("DamageModType")));
        let v = m.and_then(|m| num(m, "DamageMod")).unwrap_or(0.0);
        if t == 1.0 {
            x *= v;
        } else if t == 2.0 {
            x += v;
        } else if t == 3.0 {
            x = v;
        }
    }
    if x == 0.0 {
        return false;
    }
    crate::actors::projectile::receive_damage(w, pr, x, attack_type_of(w, causer))
}

/// `bubbleKind.destroyed` (ReceiveEndPlay @783, authority).
fn destroyed(w: &mut World, pr: ProjectileId) {
    if let Some((t, i)) = bubbled_buff(w, pr) {
        let serial = elem_buff(w, t, i).map(|b| b.serial);
        if let Some(s) = serial {
            end_actor_buff(w, t, s); // @915
        }
    }
    if crate::world::flow::reset_pass(w) {
        return;
    }
    let Some(pm) = w.actors.projectiles.get(pr) else { return };
    let Some(owner) = pm.owner else { return };
    let slot = to_int32(num(class_p(w, pm), "TriggerAbilityIndex").unwrap_or(0.0));
    begin_alternate_cooldown(w, owner, slot, 0, true); // @1133
}

/// Buff_C018BubbledProjectile's actor begin: `ext = {}`.
fn bubbled_begin(w: &mut World, a: ActorRef, b: u32, _from: Option<FighterId>) -> bool {
    if let Some(x) = elem_buff_mut(w, a, b) {
        x.ext = ElemExt::C018Bubbled(BubbledExt::default());
    }
    false
}

/// Buff_C018BubbledProjectile's actor tick: the Delay(0) retry @30, then @1586 (no attach parent: destroy) and the
/// carry's own tick.
fn bubbled_tick(w: &mut World, a: ActorRef, b: u32) -> bool {
    if !matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_)) {
        return false;
    }
    let Some(x) = ext_ref(w, a, b).copied() else { return false };
    if x.wait {
        let bubble = bubble_of(w, a, b);
        return set_bubbled(w, a, b, bubble); // the Delay(0) -> @30
    }
    if !x.activated {
        return false;
    }
    if bubble_of(w, a, b).is_none() {
        return true; // @1586: no attach parent -> K2_DestroyActor
    }
    if let Some(t) = carry_hooks(w, a).and_then(|c| c.tick) {
        t(w, a, b);
    }
    false
}

/// Buff_C018BubbledProjectile's actor end @1480.
fn bubbled_end(w: &mut World, a: ActorRef, b: u32) {
    if !matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_)) {
        return;
    }
    let x = ext_ref(w, a, b).copied().unwrap_or_default();
    let c = carry_hooks(w, a);
    if let Some(c) = c {
        (c.collision)(w, a, b, true); // BPI_SetCollisionEnabled(true)
    }
    set_mobility_to_stationary(w, a, false);
    // K2_DetachFromActor(KeepWorld): nothing to undo (the port's attach is the placing in the bubble's `moved`).
    if enable_movement(w, a) {
        set_velocity(w, a, x.vx.unwrap_or(0.0), x.vy.unwrap_or(0.0)); // @1129
    }
    if let Some(c) = c {
        (c.pause_life)(w, a, b, false); // BPI_SetLifetimePaused(false)
    }
}

/// Buff_C018BubbleHandleIncomingDamage's takenCond for a fighter's hit: false (the Attacker's cast to
/// SpawnedActorParent fails). A spawned dealer's hit is the bubble's `any_damage`.
fn incoming_taken_cond(_w: &World, _a: ActorRef, _b: u32, _attacker: FighterId) -> bool {
    false
}

/// This file's registry entries: the bubble, its pop, the two buffs.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let kind = ProjectileHooks {
        make: Some(make),
        begin: Some(begin),
        bounce_impact: Some(bounce),
        end: Some(end),
        moved: Some(moved),
        hitbox_begin_overlap: Some(hitbox_begin_overlap),
        any_damage: Some(any_damage),
        destroyed: Some(destroyed),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        BUBBLE,
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::C018BubbleProj), ActorHooks::Projectile(kind)),
    )?;
    r.actors.enter(
        BUBBLE_POP,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::C018BubblePop),
            ActorHooks::Projectile(ProjectileHooks::default()),
        ),
    )?;
    let bubbled = ActorBuffHooks {
        begin: Some(bubbled_begin),
        tick: Some(bubbled_tick),
        end: Some(bubbled_end),
        ..ActorBuffHooks::default()
    };
    r.buffs.enter(
        BUBBLED_PROJECTILE,
        BuffEntry {
            actor: Some(bubbled),
            ..BuffEntry::new(BuffClass::BuffC018BubbledProjectile, BuffHooks::default())
        },
    )?;
    let incoming = ActorBuffHooks { taken_cond: Some(incoming_taken_cond), ..ActorBuffHooks::default() };
    r.buffs.enter(
        BUBBLE_INCOMING_DAMAGE,
        BuffEntry {
            actor: Some(incoming),
            ..BuffEntry::new(BuffClass::BuffC018BubbleHandleIncomingDamage, BuffHooks::default())
        },
    )
}
