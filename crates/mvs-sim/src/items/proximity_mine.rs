//! Item_ProximityMine_C (an ItemActorParent child of the random items pool).
//! Durability 2, CanUseItem false (ItemUse drops it), Capsule 35 / 35, HitBoxDamage 4 / 1250 / 10;
//! ExplosionHitBoxDamage 7 / 1700 / 10 (IgnoreFacing, DoLOSCheck, HitPauseTime 0.15); ProximityCheck, a sphere of 250
//! on the Capsule (profile DetectPawn).
//! - Thrown @1947 (the parent's, then) / Drop @2041: CanActivate = true, Capsule.SetCollisionResponseToChannel(20
//!   Platform, Block) @1977, ItemIdleLife = 25 @2017 (nothing reads it again: the pickup cleared the LifeTimer).
//! - ReceiveHit @1390: without CanActivate the parent's @1864; with it (no parent call): CanBePickedUp = false @1404,
//!   K2_AttachToComponent(OtherComp, KeepWorld x3, weld) @1415, the Capsule's world rotation
//!   MakeRotator(roll of MakeRotFromXZ((1, 0, 0), HitNormal), 0, 0) @1636, IsArned = true @1688, HitBoxOn? = false and
//!   UpdateHitbox @1699, and with a DestructibleObjectBP as Other a binding of its ObjectDestroyed @1799.
//! - ProximityCheck's begin overlap @2061: IsArned, authority, a BPI_BaseFighter whose
//!   Comp_Actor_Destructible.TeamIndex is not the mine's -> Detonate @2056: a DoOnce @30 -> ExplosionVisuals @56
//!   (PS_TomJerryRocket_Explosion at its location with Team from the Instigator's colour index,
//!   sfx_cha_tomandjerry_rocket_hit there, and PF_Post_Event(sfx_cha_tomandjerry_rocket_loop_stop) on itself @2841);
//!   SphereOverlapActors(its location, 450, [PhysicsBody, Pawn, WorldDynamic, HurtBox], self ignored) @140; for each
//!   actor with a Comp_Actor_Destructible IsVulnerableTo the mine's TeamIndex @651:
//!   ExplosionHitBoxDamage.KnockBackDirectionVector = (Y, Z) of GetDirectionUnitVector(the mine, the actor) @1012,
//!   pfgDoDamage(no hit box, the actor, the packet, no buffs) @1107; then ReleaseToPool @15.
//!
//! The attach: on a parent that does not simulate (every solid the port has) it is a plain attach: the body's
//! simulation and the ProjectileMovement stop, the mine stays where it touched ([`ProximityMine::attached`]).
//! Deviation: a moving parent is not followed (the stage block and the platforms move nothing).
//! The DestructibleObjectBP Other (a Cromulons speaker, a Batcave car: the stage solid the contact met,
//! [`ItemContact::solid`]) has ObjectDestroyed += DestroyWithAttached @1799-1822 ([`wall_bind`]): when it breaks,
//! DestroyWithAttached -> @2907 -> @1355 (HasAuthority) -> Detonate's DoOnce @1317 -> @30, the blast, ReleaseToPool.
//! The bind is ported from the decode.
//! Not ported: the weld into a simulating body (a physics item's root).
//! The loop stop event goes on the mine's own object, where no loop plays: not posted.
//! Unverified: the Platform Block of the throw is the flight's (the parent's IsThrown ignore lifted for the move,
//! `Item::step`); IsThrown itself stays as the parent left it.
//! Unverified: the HitEnemyPlayer the mine's own handler broadcasts for a fighter it hurts (the parent's spend) is not
//! sent: the release follows in the same call. An item met that no fighter has picked up has no owner team in the port
//! and is not hit (warned), as `items/dynamite.rs`.

use crate::Real;
use crate::State;
use crate::actors::item::{ItemContact, ItemExt, is_owned, item_destructible_team};
use crate::actors::world_actor::world_actor_place_of;
use crate::actors::{ActorClass, ActorRef, ItemClass, ItemId};
use crate::combat::HitPacket;
use crate::combat::elem::charge_pct;
use crate::combat::hits::{DamageCall, actor_hit_enemy, apply_projectile_hit, dealer, do_damage, projectile_blocks};
use crate::combat::packet::DataVec;
use crate::combat::phases::los_blocked;
use crate::combat::spawned_hits::{damage_actor, damage_spawned_actor, is_vulnerable_to};
use crate::events::Event;
use crate::fighter::common::warn_text;
use crate::fighter::control::dodge_over;
use crate::fighter::moves::V2;
use crate::fighter::traces::{OBJ_HURT_BOX, OBJ_PAWN, OBJ_PHYSICS_BODY, OBJ_WORLD_DYNAMIC, sphere_overlap_actors};
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, no_collision};
use crate::name;
use crate::registry::entries::ItemHooks;
use crate::registry::props::obj;
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;
use crate::world::items::destroy_item;
use crate::world::pawns::match_fighters;
use crate::world::wall_bp::{WallActor, WallDelegate, solid_destructible, wall_bind, wall_delegate_fns};

const PROXIMITY: Real = 250.0; // ProximityCheck.SphereRadius
const BLAST: Real = 450.0;
/// DestroyWithAttached's bind name (`wall_delegate_fns`).
const DESTROY_WITH_ATTACHED: &str = "Item_ProximityMine_C.DestroyWithAttached";

/// ItemExt::ProximityMine: Item_ProximityMine_C's own variables (`ProximityMine`).
#[derive(State, Debug, Default)]
pub struct ProximityMine {
    /// CanActivate.
    pub can_activate: bool,
    /// IsArned.
    pub armed: bool,
    /// K2_AttachToComponent made (the body and the ProjectileMovement stopped).
    pub attached: bool,
    /// The Capsule's world roll at the attach, degrees (UE).
    pub roll: Real,
    /// CanBePickedUp = false.
    pub no_pick: bool,
    /// Detonate's DoOnce.
    pub detonated: bool,
    /// In a hand at the last step (the Drop below).
    pub was_held: bool,
    /// The fighters ProximityCheck overlaps (their ids), for the begin edges.
    pub inside: Vec<i32>,
    /// Inside the parent's step (`Item::step`): the flight's blocking hit is not spent then.
    pub in_step: bool,
}

fn state(w: &mut World, it: ItemId) -> &mut ProximityMine {
    match &mut w.actors.items.get_mut(it).expect("a live item").ext {
        ItemExt::ProximityMine(m) => m,
        _ => unreachable!("Item_ProximityMine_C holds a ProximityMine"),
    }
}

/// ReceiveHit with CanActivate @1390 (`c`: the hit's normal, sim, and its Other's stage solid).
fn arm(w: &mut World, it: ItemId, c: ItemContact) {
    let (nx, ny) = (c.nx, c.ny);
    let i = w.actors.items.get_mut(it).expect("a live item");
    // The attach: no ProjectileMovement, no simulation. A damping that zeroes the body's every move stands for it (the
    // parent's step has no other switch for a held body that is not a hand's).
    i.flying = false;
    i.vx = 0.0;
    i.vy = 0.0;
    i.lin_damp = Real::INFINITY;
    // HitBoxOn? = false, UpdateHitbox: HitBoxesOff clears HasHitAFighter
    i.hit_on = false;
    i.has_hit_a_fighter = false;
    // Deviation: the arm leaves a reflect's boxes on, where the build's HitBoxesOff clears them (a
    // ledger FLAG).
    let ItemExt::ProximityMine(m) = &mut i.ext else { unreachable!("Item_ProximityMine_C holds a ProximityMine") };
    m.no_pick = true;
    m.attached = true;
    // MakeRotFromXZ((1, 0, 0), n).Roll: the rotation about UE X whose Z axis (0, sin R, cos R) is n (UE (0, -nx, ny)).
    m.roll = crate::math::ue::rot_from_xz([1.0, 0.0, 0.0], [0.0, -nx, ny])[2];
    m.armed = true;
    // Other cast to DestructibleObjectBP @1724: ObjectDestroyed += DestroyWithAttached @1799-1822
    if let Some(a) = c.solid.and_then(|s| solid_destructible(w, i64::from(s))) {
        wall_bind(w, a, WallDelegate::ObjectDestroyed, ActorRef::Item(it), DESTROY_WITH_ATTACHED);
    }
}

/// DestroyWithAttached (ObjectDestroyed of the DestructibleObjectBP the mine is stuck to) -> @2907 -> @1355: Detonate's
/// DoOnce, the blast and ReleaseToPool.
fn destroy_with_attached(w: &mut World, listener: ActorRef, _from: WallActor) {
    let ActorRef::Item(it) = listener else { return };
    if w.actors.items.get(it).is_some_and(|i| !i.dead) && detonate(w, it) {
        destroy_item(w, it);
    }
}

/// ProximityCheck (a sphere at the mine's location) against a fighter's root capsule.
fn over_fighter(w: &World, it: ItemId, f: FighterId) -> bool {
    let m = w.actors.items.get(it).expect("a live item");
    let r = cap_radius(w, f);
    let seg = capsule_half(w, f) - r;
    let ay = actor_y(w, f);
    let dy = m.y - crate::math::max(ay - seg, crate::math::min(m.y, ay + seg));
    let d = PROXIMITY + r;
    let dx = m.x - w.fighter(f).body.x;
    dx * dx + dy * dy <= d * d
}

/// `where`: the victim's place.
fn where_of(w: &World, o: ActorRef) -> V2 {
    match o {
        ActorRef::Fighter(f) => V2 { x: w.fighter(f).body.x, y: actor_y(w, f) },
        ActorRef::Projectile(p) => w.actors.projectiles.get(p).map_or(V2 { x: 0.0, y: 0.0 }, |p| V2 { x: p.x, y: p.y }),
        ActorRef::Item(i) => w.actors.items.get(i).map_or(V2 { x: 0.0, y: 0.0 }, |i| V2 { x: i.x, y: i.y }),
        ActorRef::Actor(a) => world_actor_place_of(w, a).map_or(V2 { x: 0.0, y: 0.0 }, |(x, y, _)| V2 { x, y }),
        _ => V2 { x: 0.0, y: 0.0 },
    }
}

/// Detonate. True: the mine is gone (ReleaseToPool); false: the DoOnce was closed.
fn detonate(w: &mut World, it: ItemId) -> bool {
    if state(w, it).detonated {
        return false;
    }
    state(w, it).detonated = true;
    if reset_pass(w) {
        return true; // the Lab reset's end pass
    }
    let me = ActorRef::Item(it);
    let (x, y, owner) = {
        let m = w.actors.items.get(it).expect("a live item");
        (m.x, m.y, m.owner)
    };
    let from = owner.map_or(-1, |o| w.fighter(o).id);
    w.events.push(Event::ProjectileDeath {
        cls: name!("Item_ProximityMine_C"),
        fighter: from,
        vfx: Some(name!("PS_TomJerryRocket_Explosion")),
        x,
        y,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: 1.0,
        tint: None,
        to: None,
    });
    w.events.push(Event::Sound {
        fighter: from,
        ev: name!("sfx_cha_tomandjerry_rocket_hit"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    let data = std::sync::Arc::clone(&w.data);
    let base = {
        let m = w.actors.items.get(it).expect("a live item");
        obj(m.def(&data).p, "ExplosionHitBoxDamage").map(|b| HitPacket::from_props(&w.names, b, None))
    };
    let Some(base) = base else {
        warn_text(w, "Item_ProximityMine_C: no ExplosionHitBoxDamage (no blast)");
        return true;
    };
    if !is_owned(w, it) {
        return true; // (armed only after a throw or a drop, which name a holder)
    }
    let team = item_destructible_team(w, it);
    let types = [
        crate::real(OBJ_PHYSICS_BODY),
        crate::real(OBJ_PAWN),
        crate::real(OBJ_WORLD_DYNAMIC),
        crate::real(OBJ_HURT_BOX),
    ];
    let found = sphere_overlap_actors(w, x, y, BLAST, &types, &[Some(me)], -1.0);
    let mut done: Vec<ActorRef> = Vec::new();
    for o in found {
        if done.contains(&o) {
            continue;
        }
        done.push(o);
        if !matches!(o, ActorRef::Fighter(_) | ActorRef::Projectile(_) | ActorRef::Item(_) | ActorRef::Actor(_))
            || !is_vulnerable_to(w, o, team)
        {
            continue;
        }
        let at = where_of(w, o);
        // GetDirectionUnitVector(mine, actor): (Y, Z) into the packet; the sim's direction is (-Y, Z).
        let (dx, dy) = (at.x - x, at.y - y);
        let [_, ux, uy] = crate::math::ue::safe_normal([0.0, dx, dy], crate::math::ue::SMALL_NUMBER);
        let mut packet = base.clone();
        packet.knock_back_direction_vector =
            Some(DataVec { x: Some(if ux == 0.0 { 0.0 } else { -ux }), y: Some(uy), z: None });
        if packet.do_los_check == Some(true) && los_blocked(w, x, y, at.x, at.y) {
            continue;
        }
        match o {
            ActorRef::Projectile(p) => {
                if w.actors.projectiles.get(p).is_some_and(|p| !p.dead) {
                    damage_spawned_actor(w, me, p, &packet);
                }
            }
            ActorRef::Actor(a) => {
                if w.actors.actors.get(a).is_some_and(|a| !a.dead) {
                    damage_actor(w, me, a, &packet, Some(at));
                }
            }
            ActorRef::Item(i) => {
                if w.actors.items.get(i).is_none_or(|i| i.dead) {
                    continue;
                }
                if is_owned(w, i) {
                    let call = DamageCall { hit_box: false, ..DamageCall::default() };
                    do_damage(w, me, o, &packet, &call);
                } else {
                    warn_text(
                        w,
                        "Item_ProximityMine_C: the blast met an item no fighter has picked up (no owner team: not hit)",
                    );
                }
            }
            ActorRef::Fighter(vic) => {
                if w.fighter(vic).life.dead > 0.0 || crate::combat::phases::no_hit_from(w, me, vic) {
                    continue;
                }
                let d = dealer(w, me);
                if projectile_blocks(w, &d, vic) {
                    continue;
                }
                dodge_over(w, me, vic);
                let pct = charge_pct(w, me);
                if apply_projectile_hit(w, &d, vic, &packet, &[], at, Some(V2 { x: ux, y: uy }), &[], pct) {
                    actor_hit_enemy(w, &d, vic);
                }
            }
            _ => {}
        }
    }
    true
}

fn sub() -> ItemExt {
    ItemExt::ProximityMine(ProximityMine::default())
}

fn thrown(w: &mut World, it: ItemId) {
    state(w, it).can_activate = true;
}

/// Drop (ItemActorParent's drop branch @10433-10598 calls it after the release): any release from a hand. A throw also
/// passes here (its Thrown set the same).
fn step(w: &mut World, it: ItemId) -> bool {
    let held = w.actors.items.get(it).expect("a live item").holder >= 0;
    let m = state(w, it);
    if m.was_held && !held {
        m.can_activate = true;
    }
    m.was_held = held;
    false
}

/// The actor's collision is off in a hand: every overlap ends.
fn grabbed(w: &mut World, it: ItemId, _f: FighterId) {
    state(w, it).inside.clear();
}

fn receive_hit(w: &mut World, it: ItemId, destroyed: bool) -> bool {
    let m = state(w, it);
    if destroyed || !m.can_activate || m.attached {
        return false;
    }
    let c = w.scratch.item_contacts.first().copied().unwrap_or(ItemContact { nx: 0.0, ny: 1.0, solid: None });
    arm(w, it, c);
    false
}

/// The simulated body's contacts fire ReceiveHit too (bNotifyRigidBodyCollision): after a hit the parent's tail set it
/// simulating, or after a drop.
fn component_hit(w: &mut World, it: ItemId, c: ItemContact) -> bool {
    let m = state(w, it);
    if !m.can_activate || m.attached {
        return false;
    }
    arm(w, it, c);
    false
}

fn overlaps(w: &mut World, it: ItemId, only: Option<FighterId>) -> bool {
    let fs = match only {
        Some(f) => vec![f],
        None => match_fighters(w),
    };
    for f in fs {
        let fid = w.fighter(f).id;
        let i = state(w, it).inside.iter().position(|&k| k == fid);
        let no_col = w.actors.items.get(it).expect("a live item").no_collision == Some(true);
        if !(w.fighter(f).life.dead <= 0.0 && !no_collision(w, f) && !no_col && over_fighter(w, it, f)) {
            if let Some(i) = i {
                state(w, it).inside.remove(i);
            }
            continue;
        }
        if i.is_some() {
            continue;
        }
        state(w, it).inside.push(fid);
        // Deviation: under the port's spare switch a fighter's mine does not go off for the opponent it spares.
        let spared = crate::combat::phases::spared_from(w, ActorRef::Item(it), f);
        if state(w, it).armed && w.fighter(f).team.team != item_destructible_team(w, it) && !spared && detonate(w, it) {
            return true;
        }
    }
    false
}

/// This file's registry entries (Item_ProximityMine_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ItemHooks {
        sub: Some(sub),
        thrown: Some(thrown),
        step: Some(step),
        grabbed: Some(grabbed),
        receive_hit: Some(receive_hit),
        component_hit: Some(component_hit),
        overlaps: Some(overlaps),
        ..ItemHooks::default()
    };
    wall_delegate_fns(r, &[(DESTROY_WITH_ATTACHED, destroy_with_attached)])?;
    // (the world's random items spawn it with no owner: the arm needs a throw or a drop first, which name the holder;
    // the blast reads the owner's team then)
    r.actors.enter(
        ItemClass::ItemProximityMine.name(),
        ActorEntry {
            ownerless: true,
            ..ActorEntry::new(ActorClass::Item(ItemClass::ItemProximityMine), ActorHooks::Item(hooks))
        },
    )
}

#[cfg(test)]
mod tests {
    /// MakeRotFromXZ((1, 0, 0), n).Roll is the turn about UE X whose Z axis is n: atan2(-nx, ny) in degrees for the
    /// sim's normal (nx, ny) = UE (0, -nx, ny).
    #[test]
    fn roll_of_the_surface_normal() {
        for (nx, ny) in [(0.0_f32, 1.0_f32), (1.0, 0.0), (-1.0, 0.0), (0.6, 0.8), (-0.28, -0.96), (0.0, -1.0)] {
            let roll = crate::math::ue::rot_from_xz([1.0, 0.0, 0.0], [0.0, -nx, ny])[2];
            let want = crate::math::ue::atan2(-nx, ny) * crate::math::ue::RAD_TO_DEG;
            eprintln!("PROBE 9 {:?}", (nx, ny, roll, want));
            // a tolerance on purpose: rot_from_xz against the closed form atan2, two different float32 computations
            let d = crate::math::ue::normalize_axis(roll - want);
            assert!(d.abs() < 1e-3, "{nx} {ny}: {roll} {want}");
        }
    }
}
