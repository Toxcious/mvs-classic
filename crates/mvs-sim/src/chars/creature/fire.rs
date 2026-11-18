//! Reindog's spit and firebolt: CreatureSpit_C (a plain projectile with its kind),
//! Firebolt_C and Firebolt_Child_C ([`Firebolt`], a projectile's `ext`, one graph: the child adds no function), the
//! firebolt's gate wall (its ActorClassSpawnedAtDeath) and the bolt's scale and bodies.
//!
//! Reindog's (Creature, C009) shots: CreatureSpit_C (SG / SA) and Firebolt_C / Firebolt_Child_C (FG / FA), all
//! ParentProjectilePhysics children. Graphs CreatureSpit and Firebolt.
//!
//! What the parent and the data give with no code here: the spit's bounce (bShouldBounce, Bounciness 1, Friction 0,
//! BounceVelocityStopSimulatingThreshold 0, gravity scale 1) and its deaths (DestroyOnHitFighter /
//! DestroyOnHitProjectile / DestroyOnBlocked, Health 1); the firebolt's arc (ProjectileGravityScale 0.6, no
//! bShouldBounce) and its death on the stage (DestroyOnCollideWithEnvironment), Firebolt_C's ability slot 1
//! (TriggerAbilityOnSpawnIndex, `fighter::control::actor_spawn_ability`; the child's index is off), the child's
//! MaxLifetime 0.5 (its InitialLifeSpan 0.5 ends it at the same tick, through the same EndPlay), and Firebolt_C's
//! FireWallHazard_C at its death (SpawnActorAtDeath, `comp_spawn_projectile`, `actors/firewall.rs`).
//!
//! The pool: none of the three classes has CanBePooled, so ReleaseToPool (SpawnedActorParent @22789) is
//! K2_DestroyActor @23658 and every spawn is a new actor: Comp_Spawner's pool branch (InitFromPool @3399, which
//! skips FindTeleportSpot) and Firebolt's InitFromPool @4744 are not reached. The port has no pool.
//!
//! A reflect (`combat/reflect.rs`): CreatureSpit's Reflected @962 and Firebolt's MulticastReflect @5924 (the kinds'
//! reflect hooks). Not here: RotatingMovement (yaw 2000 a second, both classes): ParentProjectilePhysics'
//! ReceiveTick writes the look-at rotation behind both movement components (AimDownProjectileMovement), so it never
//! shows; nothing of the sim reads the actor's rotation. Firebolt's SphereForGround (radius 48, Z -33.33) is read by
//! no graph of the class. The firebolt's mesh growth (Tick @1254: SM_Projectile's scale + 0.02 a tick while below
//! MaxSize 7) and the particles (BaseVFX, the team colour, the impact's PS_Creature_FireWall_Impact) are the drawing
//! side's: `Projectile::age` gives the growth.
//!
//! `Firebolt`'s method overrides are called from the shared code (`actors::projectile`):
//! `receiveDamage` ([`damaged`]) and `hitNonPlayer` ([`hit_non_player`]).

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated comparisons keep NaN")]

use crate::Real;
use crate::State;
use crate::actors::projectile::{NOT_SPAWNED_ACTOR, class_p, truthy};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::events::{Event, SoundKey};
use crate::fighter::common::actor_components;
use crate::fighter::control::data_name;
use crate::fighter::facing;
use crate::fighter::traces::{TraceBody, body_info};
use crate::math::ue::{quat_rotate_vector, rotate_angle_axis, rotator_quaternion, vector_rotation};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks, ReflectHooks};
use crate::registry::props::{num, num_in, obj, text};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::spawning::{comp_spawn_projectile, destroy_projectile};

/// `SPIT`.
pub const SPIT: Name = name!("CreatureSpit_C");
/// `FIREBOLT` (read by perks.rs: FireStorm's afterSpawnedActor).
pub const FIREBOLT: Name = name!("Firebolt_C");
/// `FIREBOLT_CHILD`.
pub const FIREBOLT_CHILD: Name = name!("Firebolt_Child_C");

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

// ---- CreatureSpit_C

// ReceiveBeginPlay: with authority and a Comp_Pawn_ChargeAttack on the instigator, GetChargeValue(4) ->
// MulticastSetProjectileScale -> Capsule.SetWorldScale3D(VLerp(NoChargeScale 1.2, FullChargeScale 2, alpha))
// @68-123 (the root has bAbsoluteScale; HitBox2D, HitBox and HurtBox are its children); then the parent's BeginPlay
// last @1307, whose MaxLifetime timer takes GetSpawnMaxLifetime: Lerp(MinLifetimeClamp 0.75, MaxLifetimeClamp 3,
// GetChargeValue(4)). Both read the owner's charge at the spawn (the aimer's activation of SG f116 / SA f78 comes
// while the charge's montage still plays): `make_projectile` takes both for this class beside the splitter's.

fn spit_loop(w: &World, id: ProjectileId) -> SoundKey {
    SoundKey { tag: name!("spit"), n: pr(w, id).seq }
}

/// The spawn's FindTeleportSpot (`push_out_root`) runs at PostActorConstruction, ahead of this BeginPlay, so before
/// the charge scale of @68-123: the root sphere 80 at its template scale 1 (the SG / SA notifies' Scale3D is 1 too).
/// Then OnProjectileBounce bound @1772 and PF_Post_Event(sfx_cha_reindog_fireball_loop) on itself @896.
fn spit_begin(w: &mut World, id: ProjectileId) {
    let (fighter, key) = (pr(w, id).owner_id, spit_loop(w, id));
    w.events.push(Event::SoundStart { fighter, ev: name!("sfx_cha_reindog_fireball_loop"), key });
}

/// OnProjectileBounce @830: PF_Post_Event(sfx_cha_reindog_fireball_bounce) on itself.
// Deviation: the build posts it on the actor (the voice follows the spit); the UI gets a one-shot at the bounce's
// place. The loop_stop below is posted on the actor too; here it is the stop event of the loop's key (no place of its
// own).
fn spit_bounce(w: &mut World, id: ProjectileId, _nx: Real, _ny: Real, _ivx: Real, _ivy: Real) -> bool {
    let p = pr(w, id);
    let e = Event::Sound {
        fighter: p.owner_id,
        ev: name!("sfx_cha_reindog_fireball_bounce"),
        x: Some(p.x),
        y: Some(p.y),
        hit: None,
        pooled: false,
        key: None,
    };
    w.events.push(e);
    false
}

/// ReceiveEndPlay @1047: the parent's; PF_Post_Event_At_Location(sfx_cha_reindog_fireball_end) @1108;
/// PF_Post_Event(sfx_cha_reindog_fireball_loop_stop) on itself @1186 (the loop's stop event); the particles off.
fn spit_end(w: &mut World, id: ProjectileId) {
    let p = pr(w, id);
    let (fighter, x, y) = (p.owner_id, p.x, p.y);
    let key = spit_loop(w, id);
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_reindog_fireball_end"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    w.events.push(Event::SoundStop {
        fighter,
        key,
        fade: None,
        ev: Some(name!("sfx_cha_reindog_fireball_loop_stop")),
        curve: None,
        cut: false,
    });
}

/// Reflected @962: the parent's (empty), K2_PauseTimerHandle(LifetimeTimer), Delay(0.375) (latent link 992 -> 47) then
/// K2_UnPauseTimerHandle(LifetimeTimer). Nothing writes LifetimeTimer (xref writes: none): the handle is invalid and
/// both calls do nothing.
fn spit_reflected(_w: &mut World, _a: ActorRef) {}

fn spit_kind() -> ProjectileHooks {
    ProjectileHooks {
        begin: Some(spit_begin),
        bounce_impact: Some(spit_bounce),
        end: Some(spit_end),
        reflect: ReflectHooks { reflected: Some(spit_reflected), ..ReflectHooks::default() },
        ..ProjectileHooks::default()
    }
}

// ---- Firebolt_C, Firebolt_Child_C (one graph: the child class adds no function)

/// `Firebolt extends Projectile`: what the class adds.
#[derive(State, Debug, Default)]
pub struct Firebolt {
    /// FacingLeft: the owner's BPI_IsFacingLeft at BeginPlay @945-991.
    pub left: bool,
    /// The DoOnce of the wall spawn @15-50.
    pub gate_closed: bool,
    /// The gate was entered where the port has no world (receiveDamage, hitNonPlayer): its wall.
    pub gate_wall: bool,
    /// SpawnActorAtDeath = false (Blocked @6019).
    pub no_death_spawn: bool,
}

fn bolt(w: &World, id: ProjectileId) -> &Firebolt {
    match &pr(w, id).ext {
        crate::actors::projectile::ProjectileExt::Firebolt(b) => b,
        e => panic!("Firebolt_C without its state: {e:?}"),
    }
}

fn bolt_mut(w: &mut World, id: ProjectileId) -> &mut Firebolt {
    match &mut pm(w, id).ext {
        crate::actors::projectile::ProjectileExt::Firebolt(b) => b,
        e => panic!("Firebolt_C without its state: {e:?}"),
    }
}

/// The gate @15-439: DoOnce; the owner's Comp_Spawner (valid for every pawn) CompSpawnActor(
/// ActorClassSpawnedAtDeath, world transform Conv_VectorToTransform(location), direction (0, 0, 0), speeds 0) @307,
/// then ReleaseToPool @425 (K2_DestroyActor: EndPlay, whose death roll spawns a second wall where SpawnActorAtDeath
/// holds). True: entered.
fn enter_gate(w: &mut World, id: ProjectileId) -> bool {
    let b = bolt_mut(w, id);
    if b.gate_closed {
        return false;
    }
    b.gate_closed = true;
    b.gate_wall = true;
    true
}

/// `Firebolt.receiveDamage` (ReceiveAnyDamage @6014 -> @610: the gate, no parent call); `dmg` is not 0 here (the
/// shared `receive_damage` returns before; a damage of 0 raises no event: UGameplayStatics::ApplyDamage). True: the
/// actor is destroyed now.
pub fn damaged(w: &mut World, id: ProjectileId) -> bool {
    enter_gate(w, id)
}

/// `Firebolt.hitNonPlayer` past the parent's (`parent`, its answer): HitNonPlayer @5990: the parent's (neither class
/// has DestroyOnHitProjectile nor DestroyOnHitHeavyProjectile), then @500 with authority and a SpawnedActorParent as
/// the other, the gate (a World.actors pawn, `NOT_SPAWNED_ACTOR`: the parent's alone).
pub fn hit_non_player(w: &mut World, id: ProjectileId, other_type: i32, parent: bool) -> bool {
    if other_type == NOT_SPAWNED_ACTOR {
        return parent;
    }
    enter_gate(w, id) || parent
}

/// `spawnGateWall`: the gate's wall comes ahead of its ReleaseToPool, so ahead of the EndPlay's death roll
/// (`destroy_projectile` calls the kind's `destroyed` first).
fn bolt_destroyed(w: &mut World, id: ProjectileId) {
    if !bolt(w, id).gate_wall {
        return;
    }
    bolt_mut(w, id).gate_wall = false;
    // The class as the actor holds it (@307); a null class spawns nothing.
    let p = pr(w, id);
    let Some(cls) = text(class_p(w, p), "ActorClassSpawnedAtDeath").filter(|s| !s.is_empty()) else {
        let msg = format!("Firebolt gate: no ActorClassSpawnedAtDeath ({})", w.names.str(p.cls));
        crate::fighter::common::warn_text(w, &msg);
        return;
    };
    let (cls, x, y, f) = (data_name(w, cls), p.x, p.y, p.owner.expect("a firebolt has an owner"));
    comp_spawn_projectile(w, f, cls, x, y, [Some(0.0); 3], 0.0, 0.0, None);
}

fn bolt_loop(w: &World, id: ProjectileId) -> SoundKey {
    SoundKey { tag: name!("firebolt"), n: pr(w, id).seq }
}

/// The spawn's FindTeleportSpot (`push_out_root`: the root sphere 50, ProjectileNavPlatform) comes ahead of it.
/// ReceiveBeginPlay @645: the parent's (the passive rows of its Comp_Pawn_PassiveBuffManager, Buff_Elemental_Fire 5
/// stacks and the Fireproof / Iceproof rows: the world's spawn; no Box, BPI_ShouldActorOverlap the parent's false,
/// not pooled); PF_Post_Event_On_AkComponent(sfx_cha_reindog_firewall_cast_loop) @655, its playing ID kept; with
/// authority OnProjectileStop bound; FacingLeft.
/// OnProjectileStop @3491 (Spawn Impact Ground, ReleaseToPool) is not reached apart: the move's stop on the stage
/// comes with the root's blocking hit, whose DestroyOnCollideWithEnvironment ends the actor first.
fn bolt_begin(w: &mut World, id: ProjectileId) {
    let (fighter, key) = (pr(w, id).owner_id, bolt_loop(w, id));
    w.events.push(Event::SoundStart { fighter, ev: name!("sfx_cha_reindog_firewall_cast_loop"), key });
    let f = pr(w, id).owner.expect("a firebolt has an owner");
    let left = facing(w, f) < 0.0;
    bolt_mut(w, id).left = left;
}

/// ReceiveTick @1254, behind the parent's: with Velocity.Y == 0 and Velocity.Z >= 0 @1273-1447, Velocity =
/// RotateAngleAxis(Velocity, 5, FacingLeft ? (-1, 0, 0) : (1, 0, 0)) @1461-1597: a shot going straight up leans 5
/// degrees forward once (its Y is no longer 0). Sim x = -UE Y.
fn bolt_step(w: &mut World, id: ProjectileId) -> bool {
    let left = bolt(w, id).left;
    let p = pm(w, id);
    if p.vx != 0.0 || !(p.vy >= 0.0) {
        return false;
    }
    let axis = if left { [-1.0, 0.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let v = rotate_angle_axis([0.0, -p.vx, p.vy], 5.0, axis);
    p.vx = -v[1];
    p.vy = v[2];
    false
}

/// HitEnemyPlayer @5957: the parent's (no DestroyOnHitFighter), then @465 with authority the gate. Its passive
/// Buff_Elemental_Fire answers the same broadcast (`projectile_test`, `elem_hit_enemy`, ahead of this).
fn bolt_hit_enemy(w: &mut World, id: ProjectileId, _vic: crate::fighter::FighterId) {
    if enter_gate(w, id) {
        destroy_projectile(w, id, false);
    }
}

/// Blocked @6019: SpawnActorAtDeath = false, then the parent's Blocked (DestroyOnBlocked: its Delay(0.0) and
/// DestroySpawnedActor, `blocked`).
fn bolt_blocked(w: &mut World, id: ProjectileId, _by: ActorRef) {
    bolt_mut(w, id).no_death_spawn = true;
    let destroy = truthy(class_p(w, pr(w, id)).get("DestroyOnBlocked"));
    if destroy {
        pm(w, id).blocked_destroy = true;
    }
}

fn bolt_spawns_at_death(w: &World, id: ProjectileId) -> bool {
    truthy(class_p(w, pr(w, id)).get("SpawnActorAtDeath")) && !bolt(w, id).no_death_spawn
}

/// ReceiveDestroyed @1027: ExecuteActionOnPlayingID(Stop, the cast loop, 500 ms, curve 4 = Linear); the particles
/// off. ReceiveEndPlay @3570: the parent's, then Spawn Impact Enemy @3054: once (SpawnedEffect),
/// PS_Creature_FireWall_Impact and PF_Post_Event_At_Location(sfx_cha_reindog_firewall_hit) at its location @3361.
fn bolt_end(w: &mut World, id: ProjectileId) {
    let (fighter, key) = (pr(w, id).owner_id, bolt_loop(w, id));
    w.events.push(Event::SoundStop {
        fighter,
        key,
        fade: Some(0.5),
        ev: None,
        curve: Some(name!("Linear")),
        cut: false,
    });
    let p = pr(w, id);
    let e = Event::Sound {
        fighter,
        ev: name!("sfx_cha_reindog_firewall_hit"),
        x: Some(p.x),
        y: Some(p.y),
        hit: None,
        pooled: false,
        key: None,
    };
    w.events.push(e);
}

/// MulticastReflect @5924: the parent's, then @5792 with ProjectileMovement.Velocity.Y != 0 -> @4618 FacingLeft =
/// Velocity.Y > 0 (UE Y is sim -x) and @4482 ProjectileParticleSystem's scale by it (drawn from `left`). The child
/// class (Firebolt_Child) has no graph of its own.
fn bolt_reflect(w: &mut World, a: ActorRef, _by: crate::fighter::FighterId, _pause: bool) {
    let ActorRef::Projectile(id) = a else { return };
    let vx = pr(w, id).vx;
    if vx != 0.0 {
        bolt_mut(w, id).left = vx < 0.0;
    }
}

/// The scale of SM_Projectile after `n` ticks (`MaxSize` `max`): 1, then + 0.02 a tick (float32) while below `max`.
pub fn bolt_scale(n: i32, max: Real) -> Real {
    let mut s = 1.0;
    let mut i = 0;
    while i < n && s < max {
        s += 0.02;
        i += 1;
    }
    s
}

/// SphereForGround as a body of the traces (`ActorKind.bodies`): a SphereComponent (Custom, ECC_PhysicsBody,
/// QueryAndPhysics, radius 48) at Z -33.33 under SM_Projectile (under the Capsule, no relative transform), which no
/// graph of the class turns off. So the object queries for PhysicsBody meet it (an object query reads the body's
/// object type alone: Bugs's rocket burst, the dynamite's blast).
/// SM_Projectile is not the root's: ReceiveTick @1254 sets its world rotation to the look-at of the velocity @1788
/// (after the parent's tick) and, the Capsule Movable (its template's default), grows its world scale by 0.02 a tick
/// while its X is under MaxSize (7) @1840-2186. So the sphere's radius is 48 x that scale and its centre 33.33 x the
/// scale along the turned -Z: in the plane, below the path, square to it. The look-at of a zero velocity (or one along
/// the depth) is the zero rotator: straight below.
// Unverified: the scale is the growth of `Projectile::age` ticks from 1 (the spawn frame's tick counted), times the
// actor's scale (the build grows the world scale by 0.02 a tick, so under an actor scale other than 1 the two part);
// the turn is the velocity's now, not the one of the last actor tick.
// The Meeseeks' OverlapCheck and Box are component overlaps, which need both sides: SphereForGround answers
// WorldDynamic (their type) with Ignore, so they pass it (the traces' responder; the Capsule still overlaps).
fn bolt_bodies(w: &World, a: ActorRef, out: &mut Vec<TraceBody>) {
    let ActorRef::Projectile(id) = a else { return };
    let p = pr(w, id);
    let owner = p.owner.expect("a firebolt has an owner");
    let Some(c) = actor_components(w, ActorRef::Fighter(owner), p.cls).and_then(|c| obj(c, "SphereForGround")) else {
        return;
    };
    let Some(info) = body_info(Some(c), None).filter(|i| i.on) else { return };
    let max = num(class_p(w, p), "MaxSize").unwrap_or(0.0);
    let s = bolt_scale(p.age, max) * p.scale;
    let oz = num_in(c, "RelativeLocation", "Z").unwrap_or(0.0) * s;
    // the relative location (0, 0, oz) turned by the actor's rotation, Velocity.Rotation() (FVector::Rotation, in
    // degrees), through FRotator::Quaternion: UE (X, Y, Z) with Y = -sim x
    let rot = vector_rotation([0.0, -p.vx, p.vy]);
    let o = quat_rotate_vector(rotator_quaternion(rot), [0.0, 0.0, oz]);
    let (x, y) = (p.x - o[1], p.y + o[2]);
    out.push(TraceBody {
        ax: x,
        ay: y,
        bx: x,
        by: y,
        hx: 0.0,
        hy: 0.0,
        r: num(c, "SphereRadius").unwrap_or(32.0) * s,
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: name!("SphereForGround"),
        actor: Some(a),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

fn construct(_w: &World, p: &mut Projectile) {
    p.ext = crate::actors::projectile::ProjectileExt::Firebolt(Firebolt::default());
}

fn bolt_kind() -> ProjectileHooks {
    ProjectileHooks {
        construct: Some(construct),
        bodies: Some(bolt_bodies),
        begin: Some(bolt_begin),
        step: Some(bolt_step),
        hit_enemy: Some(bolt_hit_enemy),
        blocked: Some(bolt_blocked),
        destroyed: Some(bolt_destroyed),
        spawns_at_death: Some(bolt_spawns_at_death),
        end: Some(bolt_end),
        reflect: ReflectHooks { multicast: Some(bolt_reflect), ..ReflectHooks::default() },
        ..ProjectileHooks::default()
    }
}

/// This file's registry entries (the spit's and the firebolt's kinds).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let spit =
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::CreatureSpit), ActorHooks::Projectile(spit_kind()));
    r.actors.enter("CreatureSpit_C", spit)?;
    let b = ActorEntry::new(ActorClass::Projectile(ProjectileClass::Firebolt), ActorHooks::Projectile(bolt_kind()));
    r.actors.enter("Firebolt_C", b)?;
    let c =
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::FireboltChild), ActorHooks::Projectile(bolt_kind()));
    r.actors.enter("Firebolt_Child_C", c)
}
