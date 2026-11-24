//! Black Adam's double strike: C021_DoubleStrikeProjectile_C
//! ([`DoubleStrike`], a projectile's `ext`): its blocking hit and its explosion.
//!
//! The air down special's spark. The parent and the data: HitBoxOn? true with a HitBoxDamage of zeros (no effects),
//! MaxLifetime 0.06, DestroyOnHitFighter, DestroyOnCollideWithEnvironment, gravity 0, the speed from the spawner.
//! ReceiveTick @2472 -> @2117: the parent's, then authority, not being destroyed, Vector_Distance(GetOwner()'s
//! location, its own) > Throw Distance (900) -> K2_DestroyActor @2426. Blocked @3022: HitActor = the blocking
//! fighter, then SpawnedActorParent's Blocked @2097. ReceiveEndPlay @3007 -> Explode @2496 (no parent EndPlay
//! forwarding, every end reason): PS_C021_DoubleStrike_Impact at the actor with Explosion Scale @2524,
//! sfx_cha_C021_air_down_special_impact at it @2924; with authority @1727 SphereTraceMultiForObjects(self, location,
//! location, Explosion Scale.X x 230, [Pawn], ignore [HitActor], bIgnoreSelf) @1917; for each hit @1307-1529 its own
//! Comp_Pawn_HitBoxHandler's pfgDoDamage(null, HitActor, EXPLOSION, no buffs). The Pawn bodies are the fighters' root
//! capsules, his own among them: a fighter of his team is an ally's hit (nothing for himself), the others go through
//! the gates (blocked by projectile armor, an invulnerable one's dodge) to the hit. The other Pawn body is
//! SeismicReactor_C's DamageBox: pfgDoDamage on it is `damage_spawned_actor`. The packet's tag is None, the HitBox's
//! Hit: TrackActorsHitByTag keeps them apart, so the fighter the box hit (DestroyOnHitFighter ends the spark) is in
//! the explosion too.
//! The explosion runs inside K2_DestroyActor (`destroyed`); an end that does not pass there (the instigator's death,
//! a despawn) runs it from `end`, at the end of the frame.
// Unverified: the order of the hits of a zero-length sweep is PhysX's, not read: the gather order (fighters in sim
// order). Only two fighters take part in a 1v1, so it decides the order of two hits of one frame.

use crate::State;
use crate::actors::projectile::ProjectileExt;
use crate::actors::projectile::class_p;
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::elem::charge_pct;
use crate::combat::hits::{actor_hit_enemy, ally_hit, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::spawned_hits::damage_spawned_actor;
use crate::combat::{DataEnum, DataVec, HitPacket, VacuumInfo};
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::common::warn_text;
use crate::fighter::control::attack_dodged;
use crate::fighter::moves::V2;
use crate::fighter::traces::{OBJ_PAWN, TraceHit, sphere_trace_multi_for_objects};
use crate::fighter::{actor_y, invulnerable};
use crate::math::ue::size2d;
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::props::{num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;

const DOUBLE_STRIKE: &str = "C021_DoubleStrikeProjectile_C";

/// `DoubleStrike extends Projectile`: what the class adds.
#[derive(State, Debug, Default)]
pub struct DoubleStrike {
    /// HitActor (the BlockingActor: a fighter, or the Steven_ShieldActor that blocked it).
    pub hit_actor: Option<ActorRef>,
    /// ReceiveEndPlay ran.
    pub exploded: bool,
}

/// `EXPLOSION`: the packet of the explosion's hits.
fn explosion() -> HitPacket {
    HitPacket {
        damage: Some(2.0),
        base_knock_back: Some(1100.0),
        knock_back_scaling: Some(4.0),
        knock_back_direction_vector: Some(DataVec { x: Some(0.0), y: Some(1.0), z: None }),
        attack_tag: Some(name!("None")),
        hit_reaction: Some(DataEnum::Value(4)),
        hit_pause_time: Some(0.15),
        hit_stun_mult: Some(0.15),
        on_hit_audio: Some(name!("sfx_cha_C021_air_down_special_hit")),
        directional_knockback: Some(false),
        velocity_instead_of_facing: Some(false),
        knock_back_mirror: Some(false),
        ignore_facing: Some(false),
        attack_id: Some(0),
        do_los_check: Some(false),
        force_knockback: Some(false),
        hit_only_one_fighter: Some(false),
        armor_pierce: Some(false),
        destroy_projectiles: Some(false),
        reflect_projectiles: Some(false),
        vacuum_knockback: Some(false),
        play_on_hit_effects: Some(true),
        apply_hit_pause_to_damage_dealer: Some(true),
        attack_decay_type: Some(DataEnum::Value(0)),
        attack_decay_name: Some(name!("None")),
        additive_kb_mode: Some(DataEnum::Value(0)),
        kb_scaling_starting_dmg: Some(0.0),
        hit_each_actor_only_once: Some(true),
        account_for_velocity_with_vacuum_knockback: Some(true),
        ignore_kbi: Some(false),
        kbi_multiplier: Some(1.0),
        vacuum_info: Some(VacuumInfo {
            use_vacuum_knock_back: Some(false),
            account_for_attacker_velocity: Some(true),
            ..VacuumInfo::default()
        }),
        ..HitPacket::default()
    }
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn ds_mut(w: &mut World, id: ProjectileId) -> &mut DoubleStrike {
    match &mut w.actors.projectiles.get_mut(id).expect("a live projectile").ext {
        ProjectileExt::DoubleStrike(d) => d,
        e => panic!("C021_DoubleStrikeProjectile_C without its state: {e:?}"),
    }
}

fn construct(_: &World, p: &mut Projectile) {
    p.ext = ProjectileExt::DoubleStrike(DoubleStrike::default());
}

/// Explode @2496: once, and not in the Lab reset's end pass (it acts on the old pawns).
fn explode(w: &mut World, id: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(id) else { return };
    let ProjectileExt::DoubleStrike(d) = &p.ext else { return };
    if d.exploded {
        return;
    }
    let hit_actor = d.hit_actor;
    ds_mut(w, id).exploded = true;
    if reset_pass(w) {
        return;
    }
    let me = ActorRef::Projectile(id);
    let (cls, x, y) = (pr(w, id).cls, pr(w, id).x, pr(w, id).y);
    let Some(f) = pr(w, id).owner else { return };
    let fid = w.fighter(f).id;
    let sc = obj(class_p(w, pr(w, id)), "Explosion Scale").and_then(|s| num(s, "X")).unwrap_or(1.0);
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter: fid,
        vfx: Some(name!("PS_C021_DoubleStrike_Impact")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: sc,
        tint: None,
        to: None,
    });
    let ev = name!("sfx_cha_C021_air_down_special_impact");
    w.events.push(Event::Sound { fighter: fid, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
    let mut out: Vec<TraceHit> = Vec::new();
    sphere_trace_multi_for_objects(
        w,
        x,
        y,
        x,
        y,
        sc * 230.0,
        &[crate::real(OBJ_PAWN)],
        &[hit_actor],
        Some(me),
        &mut out,
    );
    let hit = explosion();
    let mut done: Vec<FighterId> = Vec::new(); // TrackActorsHitByTag(None) of this handler
    let mut actors: Vec<ActorRef> = Vec::new(); // the same for a spawned actor
    let attack_id = pr(w, id).attack_id;
    for h in &out {
        let vic = match h.actor {
            Some(ActorRef::Fighter(v)) => v,
            // A spawned actor's Pawn body (SeismicReactor_C's DamageBox): pfgDoDamage on it.
            Some(o @ ActorRef::Projectile(b)) => {
                if actors.contains(&o) {
                    continue;
                }
                actors.push(o);
                damage_spawned_actor(w, me, b, &hit);
                continue;
            }
            Some(_) => {
                warn_text(
                    w,
                    "C021_DoubleStrikeProjectile_C: the explosion met a Pawn body of an actor that is no projectile (not hit)",
                );
                continue;
            }
            None => continue,
        };
        if done.contains(&vic) || w.fighter(vic).life.dead > 0.0 || crate::combat::phases::no_hit_by(w, f, vic) {
            continue;
        }
        if w.fighter(vic).team.team == w.fighter(f).team.team {
            let d = dealer(w, me);
            ally_hit(w, &d, vic, &hit);
            done.push(vic);
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        done.push(vic);
        let d = dealer(w, me);
        let at = V2 { x, y };
        if apply_projectile_hit(w, &d, vic, &hit, &[], at, None, &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &d, vic);
        }
    }
}

fn step(w: &mut World, id: ProjectileId) -> bool {
    let p = pr(w, id);
    let Some(f) = p.owner else { return false };
    let far = num(class_p(w, p), "Throw Distance").unwrap_or(0.0);
    size2d([w.fighter(f).body.x - p.x, actor_y(w, f) - p.y]) > far
}

/// Blocked @3022: HitActor = the blocking actor, then SpawnedActorParent's Blocked @2097.
fn blocked(w: &mut World, id: ProjectileId, by: ActorRef) {
    ds_mut(w, id).hit_actor = Some(by);
    if num(class_p(w, pr(w, id)), "DestroyOnBlocked").is_some_and(|v| v != 0.0)
        || class_p(w, pr(w, id)).get("DestroyOnBlocked").is_some_and(crate::registry::props::truthy)
    {
        w.actors.projectiles.get_mut(id).expect("a live projectile").blocked_destroy = true;
    }
}

fn destroyed(w: &mut World, id: ProjectileId) {
    explode(w, id);
}

fn end(w: &mut World, id: ProjectileId) {
    explode(w, id);
}

/// This file's registry entries (the double strike's class).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ProjectileHooks {
        construct: Some(construct),
        step: Some(step),
        blocked: Some(blocked),
        destroyed: Some(destroyed),
        end: Some(end),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        DOUBLE_STRIKE,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::C021DoubleStrikeProjectile),
            ActorHooks::Projectile(hooks),
        ),
    )
}
