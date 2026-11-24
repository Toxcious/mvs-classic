//! JerryKickedRocket and TomAimRocket1: the kicked rocket
//! ([`KickedRocket`]: the 0.9 s timeline arc, Jerry back at its end, the explosion) and the aimed one ([`AimRocket`]:
//! the homing on Jerry and its BigHit when it overlaps him, Jerry's HR_03 through jerry.rs play_montage, the
//! explosion); ParentProjectilePhysics children.
//!
//! JerryKickedRocket (Tom_and_Jerry_Rocket_Kick_Montage's spawner: up at 2500, MaxSpeed 2500). ReceiveBeginPlay @5177:
//! SpawnOwner = GetOwner(), ShouldSpawnJerry (the owner's BPI_GetFighterCharacter == 15), the Instigator's
//! BPI_SetIsJerryAttached(false), FacingLeft, RocketArc.PlayFromStart @3521. RocketArc (length 0.9, the float track
//! Arc): CurrentVelocity = Velocity, SetVelocityMulticast(Normal((0, FacingLeft ? Arc : -Arc, 1 - Arc), 1e-4) x
//! VSize(CurrentVelocity)). MulticastReflect @8072: HasBeenReflected, with ShouldPause RocketArc.Stop and Delay(0.375)
//! -> Play. ReceiveEndPlay @5568: Jerry back (spawn_jerry), the explosion (PS_TomJerryRocket_Explosion at 0.5,
//! sfx_shared_item_dynamite_explode), ExplosionDamage's KnockBackDirectionVector from the velocity's normal, the
//! SphereOverlapActors(location, 200, [PhysicsBody, Pawn, HurtBox], ignore [BlockingActor]) and the handler's
//! pfgDoDamage on each actor with a Comp_Actor_Destructible vulnerable to the owner's team.
//!
//! TomAimRocket1 (Rocket_Kick_NoJerry(_Ground)_Montage's spawner: up at 2500, MaxSpeed 2500; homing 2000 in its
//! ProjectileMovement template, InitialLifeSpan 2). ReceiveBeginPlay @1624: the last Jerry of the owner's SpawnedActors
//! is JerryAcor (none: Delay(0.3) -> destroy), Delay(0.033) AimDownProjectileMovement, Delay(0.2) homing magnitude
//! 13000, HomingTargetComponent = its root, the launch and loop sounds. HitBox's begin overlap with a Jerry of its own
//! team: the HitBoxDamage variable 8 / 1650 / 16, BigHit, Jerry.PlayMontage(HR_03). ReceiveEndPlay @2950: the
//! explosion (1 / 0.5 by BigHit), SphereTraceMultiForObjects(BigHit ? 350 : 200) and the handler's pfgDoDamage.
//
// Unverified (kicked a): AActor's InitialLifeSpan (SetLifeSpan at BeginPlay, stock 4.26; not read in the exe) is the
// class's own here (the generic Projectile reads only the reflect's SetLifeSpan): the age past
// timer_ticks(InitialLifeSpan) + 1 from the spawn, in the actor's tick after its move, as the MaxLifetime end.
// Unverified (kicked b): the timeline's tick (UTimelineComponent, TG_PrePhysics) is taken after the actor's tick and
// latent actions (kind.step), undilated, as armor's ArmorShake; PlayFromStart updates at position 0 in BeginPlay.
// Deviation (kicked c): with sphere3D the kicked rocket's HitBox sphere deals; its begin overlap @7861
// (K2_DestroyActor on an enemy fighter) is not ported: a warning.
// Deviation (kicked d): TomAimRocket1's HitBox begin overlap with Jerry is tested after the rocket's move (kind.moved)
// and at the spawn after BeginPlay, not swept: the sphere against Jerry's Capsule and ProjectileHurtBox, one begin per
// component (two at once: the event twice). A begin from Jerry's own move is seen after the rocket's next move.
// Deviation (kicked e): a reflect's damage / knockback factors on HitBoxDamage after a BigHit are read back as the
// ratio of the scaled handler packet to the one before (float32 rounding of the ratio).
// Unverified (kicked f): pfgDoDamage's LocalHitLocation with no hit box is taken as the victim's location (as
// C021_ForceField's); the order of the explosion's actors is the port's trace order.
// Deviation (kicked g): the trace's WorldDynamic is left out: the port's traces gather no body of that type. Of the
// ported actors with a Comp_Actor_Destructible, Jerry has one (his Box) and is met by his Capsule (Pawn) all the same;
// an actor with a WorldDynamic body alone would be missed.

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_def, class_p, hit_collision, hit_xy, set_actor_scale};
use crate::actors::{ActorClass, ActorId, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::elem::charge_pct;
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks, truthy};
use crate::combat::spawned_hits::{comp, damage_actor, damage_spawned_actor, world_hooks};
use crate::combat::{DataVec, HitPacket};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, SoundKey};
use crate::fighter::common::{latent_frames, owner_team, warn_text};
use crate::fighter::moves::V2;
use crate::fighter::traces::{
    OBJ_HURT_BOX, OBJ_PAWN, OBJ_PHYSICS_BODY, TraceHit, sphere_overlap_actors, sphere_trace_multi_for_objects,
};
use crate::fighter::{FighterId, actor_y, facing, invulnerable};
use crate::math::ue::{SMALL_NUMBER, safe_normal};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks, ReflectHooks};
use crate::registry::props::{flag, num_or, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;

use super::jerry::{jerry_of, play_montage, spawn_jerry};
use super::pawn::set_is_jerry_attached;
use super::state::tj_of;

/// The kicked rocket's class.
pub const KICKED_ROCKET: &str = "JerryKickedRocket_C";
/// The aimed rocket's class.
pub const AIM_ROCKET: &str = "TomAimRocket1_C";

const HR_03: Name = name!("Tom_and_Jerry_Only_Jerry_HR_03_Montage");
const EXPLOSION_VFX: Name = name!("PS_TomJerryRocket_Explosion");

/// RocketArc's Arc curve [time, value, arrive, leave], every key RCIM_Cubic.
const ARC_KEYS: [[Real; 4]; 3] = [
    [0.0, 0.0, 6.7642055, 2.8191934],
    [0.38835645, 0.7448537, 0.94500446, 0.9450021],
    [0.9, 1.0, 0.02461551, 0.024615496],
];

/// `KickedRocket extends Projectile` (JerryKickedRocket_C).
#[derive(State, Debug, Default)]
pub struct KickedRocket {
    /// SpawnOwner.
    pub spawn_owner: Option<FighterId>,
    pub should_spawn_jerry: bool,
    /// FacingLeft.
    pub facing_left: bool,
    /// HasBeenReflected.
    pub reflected: bool,
    /// BlockingActor.
    pub blocking_actor: Option<ActorRef>,
    /// RocketArc's position.
    pub arc_pos: Real,
    /// RocketArc playing.
    pub arc_on: bool,
    /// The reflect's Delay(0.375) @5122: ticks left, 0 for none.
    pub arc_resume: Real,
    /// Unverified kicked a: the age the life span ends it at, -1 for none.
    pub life_span_at: i32,
    /// ReceiveEndPlay ran.
    pub exploded: bool,
}

/// `AimRocket extends Projectile` (TomAimRocket1_C).
#[derive(State, Debug, Default)]
pub struct AimRocket {
    /// JerryAcor.
    pub jerry: Option<ActorRef>,
    /// ProjectileMovement.HomingTargetComponent's actor (its root).
    pub home_target: Option<ActorRef>,
    /// ProjectileMovement.bIsHomingProjectile.
    pub home_on: bool,
    /// HomingAccelerationMagnitude.
    pub home_mag: Real,
    /// AimDownProjectileMovement.
    pub aim_down: bool,
    /// HasBeenReflected.
    pub reflected: bool,
    /// BigHit.
    pub big_hit: bool,
    /// The HitBoxDamage variable once the graph wrote it (@4255): none while it is the handler's packet (`hit`).
    pub var_hit: Option<HitPacket>,
    /// The handler's packet the variable was last sent as (Deviation kicked e).
    pub handler_hit: HitPacket,
    /// The Delay(0.3) -> K2_DestroyActor of @2821 runs.
    pub no_jerry: bool,
    /// The reflect's Delay(0.066) @4453: ticks left, 0 for none.
    pub reflect_sound: Real,
    /// HitActor.
    pub hit_actor: Option<ActorRef>,
    /// BlockingActor.
    pub blocking_actor: Option<ActorRef>,
    /// The Jerry components (id x 2 + 0 Capsule / 1 ProjectileHurtBox) its HitBox overlaps.
    pub over_jerry: Vec<i32>,
    /// Unverified kicked a: the age the life span ends it at, -1 for none.
    pub life_span_at: i32,
    /// ReceiveEndPlay ran.
    pub exploded: bool,
}

// ---------------------------------------------------------------- shared parts

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn kr(w: &World, id: ProjectileId) -> &KickedRocket {
    match &pr(w, id).ext {
        ProjectileExt::KickedRocket(s) => s,
        e => panic!("a JerryKickedRocket_C without its state: {e:?}"),
    }
}

fn kx(w: &mut World, id: ProjectileId) -> &mut KickedRocket {
    match &mut pm(w, id).ext {
        ProjectileExt::KickedRocket(s) => s,
        e => panic!("a JerryKickedRocket_C without its state: {e:?}"),
    }
}

fn ar(w: &World, id: ProjectileId) -> &AimRocket {
    match &pr(w, id).ext {
        ProjectileExt::AimRocket(s) => s,
        e => panic!("a TomAimRocket1_C without its state: {e:?}"),
    }
}

fn ax(w: &mut World, id: ProjectileId) -> &mut AimRocket {
    match &mut pm(w, id).ext {
        ProjectileExt::AimRocket(s) => s,
        e => panic!("a TomAimRocket1_C without its state: {e:?}"),
    }
}

fn owner(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("a rocket has an owner")
}

fn sound(w: &mut World, id: ProjectileId, ev: Name) {
    let fighter = w.fighter(owner(w, id)).id;
    let p = pr(w, id);
    let e = Event::Sound { fighter, ev, x: Some(p.x), y: Some(p.y), hit: None, pooled: false, key: None };
    w.events.push(e);
}

fn explosion_fx(w: &mut World, id: ProjectileId, scale: Real) {
    let fighter = w.fighter(owner(w, id)).id;
    let p = pr(w, id);
    let e = Event::ProjectileDeath {
        cls: p.cls,
        fighter,
        vfx: Some(EXPLOSION_VFX),
        x: p.x,
        y: p.y,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale,
        tint: None,
        to: None,
    };
    w.events.push(e);
}

/// Kismet Normal(V, 1e-4) of (0, UE Y, UE Z) (the node's GetSafeNormal by InvSqrt).
fn normal(y: Real, z: Real) -> (Real, Real) {
    let [_, ny, nz] = crate::math::ue::normal([0.0, y, z], 1e-4);
    (ny, nz)
}

/// The last live Jerry of the owner's Comp_Spawner SpawnedActors, by exact class (@1700-2945).
fn last_jerry(w: &World, f: FighterId) -> Option<ActorRef> {
    let mut j = None;
    for &a in &w.fighter(f).items.spawned_actors {
        let ActorRef::Actor(id) = a else { continue };
        let Some(wa) = w.actors.actors.get(id) else { continue };
        if !wa.dead
            && (wa.cls == name!("Jerry_SpawnedActor_C") || wa.cls == name!("Jerry_SpawnedActor_TempInvincibility_C"))
        {
            j = Some(a);
        }
    }
    j
}

/// The age the InitialLifeSpan timer ends the actor at (Unverified kicked a): -1 for none.
fn life_span_at(w: &World, p: &Projectile) -> i32 {
    let life = num_or(class_p(w, p), "InitialLifeSpan", 0.0);
    if life > 0.0 {
        // timer_ticks is an integer-valued float: the exact walk of the loop.
        let k = timer_ticks(Some(life));
        crate::math::to_int32(k) + 1
    } else {
        -1
    }
}

/// LifeSpanExpired: the class's own until a reflect sets the span again (`Projectile::reflect`).
fn life_over(p: &Projectile, at: i32) -> bool {
    p.reflect.is_none() && at >= 0 && p.age >= at
}

/// pfgDoDamage(no hit box, the actor, packet) of the dying rocket's handler on each actor the explosion found, once per
/// actor; `dir`: the packet's IgnoreFacing direction in sim terms (`apply_projectile_hit`), or none (the packet's own).
/// Comp_Actor_Destructible.IsVulnerableTo(the owner's TeamIndex) of a fighter, a spawned actor or a World.actors
/// actor: !IsInvulnerable?, !DamageDisabled, another team; no Comp_Actor_Destructible: skipped.
fn explode(w: &mut World, id: ProjectileId, found: &[Option<ActorRef>], packet: &HitPacket, dir: Option<V2>) {
    let me = ActorRef::Projectile(id);
    let team = w.fighter(owner(w, id)).team.team;
    let mut done: Vec<ActorRef> = Vec::new();
    for &o in found {
        let Some(o) = o else { continue };
        if o == me || done.contains(&o) {
            continue;
        }
        done.push(o);
        match o {
            ActorRef::Fighter(vic) => {
                if invulnerable(w, vic) || w.fighter(vic).invuln.damage_disabled || w.fighter(vic).team.team == team {
                    continue;
                }
                if w.fighter(vic).life.dead > 0.0 || crate::combat::phases::no_hit_by_team(w, team, vic) {
                    continue;
                }
                let d = dealer(w, me);
                if projectile_blocks(w, &d, vic) {
                    continue;
                }
                let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
                if apply_projectile_hit(w, &d, vic, packet, &[], at, dir, &[], charge_pct(w, me)) {
                    actor_hit_enemy(w, &d, vic);
                }
            }
            ActorRef::Projectile(b) => {
                let Some(bp) = w.actors.projectiles.get(b) else { continue };
                let own_damage_disabled = bp.damage_disabled;
                let Some(c) = comp(w, o, "Comp_Actor_Destructible") else { continue };
                let off = own_damage_disabled.unwrap_or_else(|| truthy(c, "DamageDisabled"));
                if off || owner_team(w, o) == team {
                    continue;
                }
                damage_spawned_actor(w, me, b, packet);
            }
            ActorRef::Actor(a) => {
                let Some(wa) = w.actors.actors.get(a) else { continue };
                let (cls, own) = (wa.cls, wa.owner);
                let hooked = world_hooks(w, cls).and_then(|k| k.destructible).and_then(|f| f(w, a));
                let Some(c) = hooked.or_else(|| comp(w, o, "Comp_Actor_Destructible")) else { continue };
                if truthy(c, "DamageDisabled") || w.fighter(own).team.team == team {
                    continue;
                }
                let at = crate::actors::world_actor::actor_place(w, o)
                    .map_or(V2 { x: pr(w, id).x, y: pr(w, id).y }, |(x, y, _)| V2 { x, y });
                damage_actor(w, me, a, packet, Some(at));
            }
            _ => {} // a stage body: no Comp_Actor_Destructible on the port's stage solids
        }
    }
}

// ---------------------------------------------------------------- JerryKickedRocket

/// FRichCurve::Eval as armor's armorShakeCurve: the Bezier of P0, P0 + Leave x gap / 3, P3 - Arrive x gap / 3, P3.
fn arc_curve(t: Real) -> Real {
    let k = &ARC_KEYS;
    if t <= k[0][0] {
        return k[0][1];
    }
    if t >= k[2][0] {
        return k[2][1];
    }
    let mut i = 0;
    while k[i + 1][0] <= t {
        i += 1;
    }
    let (a, b) = (k[i], k[i + 1]);
    let gap = b[0] - a[0];
    let u = (t - a[0]) / gap;
    let p1 = a[1] + (a[3] * gap) / 3.0;
    let p2 = b[1] - (b[2] * gap) / 3.0;
    let v = 1.0 - u;
    v * v * v * a[1] + 3.0 * v * v * u * p1 + 3.0 * v * u * u * p2 + u * u * u * b[1]
}

/// RocketArc__UpdateFunc @5592.
fn arc_update(w: &mut World, id: ProjectileId, arc: Real) {
    let left = kr(w, id).facing_left;
    let p = pm(w, id);
    let ue_y = if p.vx == 0.0 { 0.0 } else { -p.vx };
    let ue_z = p.vy;
    let size = ((ue_y * ue_y) + (ue_z * ue_z)).sqrt(); // VSize(CurrentVelocity)
    let (ny, nz) = normal(if left { arc } else { -arc }, 1.0 - arc);
    let vy = ny * size;
    p.vx = if vy == 0.0 { 0.0 } else { -vy };
    p.vy = nz * size;
}

/// The timeline's tick: the position on by the frame's DeltaSeconds; past the length the update at the length, then
/// Finished (empty) and the stop (Unverified kicked b).
fn arc_tick(w: &mut World, id: ProjectileId) {
    let length: Real = 0.9;
    let mut p = kr(w, id).arc_pos + DELTA_SECONDS;
    if p > length {
        p = length;
        kx(w, id).arc_on = false;
    }
    kx(w, id).arc_pos = p;
    arc_update(w, id, arc_curve(p));
}

fn kicked_construct(w: &World, p: &mut Projectile) {
    let at = life_span_at(w, p);
    p.ext =
        ProjectileExt::KickedRocket(KickedRocket { spawn_owner: p.owner, life_span_at: at, ..KickedRocket::default() });
}

fn kicked_begin(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    let tj = tj_of(w, f).is_some();
    let left = facing(w, f) < 0.0;
    {
        let k = kx(w, id);
        k.spawn_owner = Some(f);
        k.should_spawn_jerry = tj; // BPI_GetFighterCharacter 15: Pawn_TomAndJerry alone
        k.facing_left = left;
    }
    if tj {
        set_is_jerry_attached(w, f, false); // the Instigator's BPI_SetIsJerryAttached(false) @3854
    }
    // RocketArc.PlayFromStart @3521: the position 0 and its update there.
    {
        let k = kx(w, id);
        k.arc_pos = 0.0;
        k.arc_on = true;
    }
    arc_update(w, id, arc_curve(0.0));
    if pr(w, id).sphere_3d {
        warn_text(
            w,
            &format!(
                "{KICKED_ROCKET}: the HitBox sphere deals (sphere3D): its begin overlap @7861 (K2_DestroyActor on an enemy fighter) is not ported (Deviation kicked c)"
            ),
        );
    }
}

/// FMath::Clamp(X, Min, Max) as FClamp has it: X < Min ? Min : X < Max ? X : Max.
fn clamp(x: Real, lo: Real, hi: Real) -> Real {
    if x < lo {
        lo
    } else if x < hi {
        x
    } else {
        hi
    }
}

/// ReceiveEndPlay @5568 (however the actor ended).
fn kicked_end_play(w: &mut World, id: ProjectileId) {
    if kr(w, id).exploded {
        return;
    }
    kx(w, id).exploded = true;
    if reset_pass(w) {
        return; // the Lab reset's end pass acts on the old pawns
    }
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    if kr(w, id).should_spawn_jerry
        && let Some(so) = kr(w, id).spawn_owner
    {
        spawn_jerry(w, so, x, y, 0.0, 0.0, true, 0.0);
    }
    explosion_fx(w, id, 0.5); // BoomMulticast @6278: SelectVector(0.5, 0.5, HasBeenReflected)
    sound(w, id, name!("sfx_shared_item_dynamite_explode"));
    // @315-654: the knockback direction from the velocity's normal.
    let (vx, vy) = (pr(w, id).vx, pr(w, id).vy);
    let (ny, nz) = normal(if vx == 0.0 { 0.0 } else { -vx }, vy);
    let z = if nz >= 0.0 { clamp(nz, 0.2, 1.0) } else { clamp(nz, -0.2, -1.0) };
    let Some(base) = obj(class_p(w, pr(w, id)), "ExplosionDamage") else {
        return warn_text(w, &format!("{KICKED_ROCKET} data missing: ExplosionDamage"));
    };
    let mut packet = HitPacket::from_props(&w.names, base, None);
    packet.knock_back_direction_vector = Some(DataVec { x: Some(ny), y: Some(z), z: None });
    let dir = V2 { x: if ny == 0.0 { 0.0 } else { -ny }, y: z }; // IgnoreFacing: the vector's X is UE Y, the sim's -x
    let types = [crate::real(OBJ_PHYSICS_BODY), crate::real(OBJ_PAWN), crate::real(OBJ_HURT_BOX)];
    let block = kr(w, id).blocking_actor;
    let found = sphere_overlap_actors(w, x, y, 200.0, &types, &[block], -1.0);
    let found: Vec<Option<ActorRef>> = found.into_iter().map(Some).collect();
    explode(w, id, &found, &packet, Some(dir));
}

fn kicked_step(w: &mut World, id: ProjectileId) -> bool {
    if life_over(pr(w, id), kr(w, id).life_span_at) {
        return true;
    }
    // The reflect's Delay(0.375) -> RocketArc.Play() @1797, then the timeline's tick.
    if kr(w, id).arc_resume > 0.0 {
        let k = kx(w, id);
        k.arc_resume -= 1.0;
        if k.arc_resume == 0.0 {
            k.arc_on = true;
        }
    }
    if kr(w, id).arc_on {
        arc_tick(w, id);
    }
    false
}

fn kicked_blocked(w: &mut World, id: ProjectileId, by: ActorRef) {
    kx(w, id).blocking_actor = Some(by);
    if flag(class_p(w, pr(w, id)), "DestroyOnBlocked") {
        pm(w, id).blocked_destroy = true; // the parent's Blocked
    }
}

fn kicked_destroyed(w: &mut World, id: ProjectileId) {
    kicked_end_play(w, id);
}

fn kicked_end(w: &mut World, id: ProjectileId) {
    kicked_end_play(w, id);
}

/// @8072: the parent's MulticastReflect, then HasBeenReflected, and with ShouldPause RocketArc.Stop and the Delay.
fn kicked_multicast(w: &mut World, a: ActorRef, by: FighterId, pause: bool) {
    let _ = by;
    let ActorRef::Projectile(id) = a else { return };
    let k = kx(w, id);
    k.reflected = true;
    if !pause {
        return;
    }
    k.arc_on = false;
    if k.arc_resume == 0.0 {
        k.arc_resume = latent_frames(0.375); // a Delay ignores a call while pending
    }
}

// ---------------------------------------------------------------- TomAimRocket1

/// The aimed rocket's override of `writeHoming` (bIsHomingProjectile), behind the parent's. Called by
/// `actors::projectile::write_homing`.
pub fn write_homing(p: &mut Projectile, on: bool) {
    if let ProjectileExt::AimRocket(s) = &mut p.ext {
        s.home_on = on;
    }
}

/// A Jerry that is there and alive: his place.
fn jerry_at(w: &World, a: Option<ActorRef>) -> Option<(Real, Real)> {
    let ActorRef::Actor(id) = a? else { return None };
    if w.actors.actors.get(id)?.dead {
        return None;
    }
    jerry_of(w, id).map(|j| (j.x, j.y))
}

/// The aimed rocket's `homeAccel`: toward Jerry's root (a destroyed target: none). Called by
/// `actors::projectile::home_accel`.
pub fn home_accel(w: &World, p: &Projectile, s: &AimRocket) -> Option<V2> {
    if !s.home_on {
        return None;
    }
    let (tx, ty) = jerry_at(w, s.home_target)?;
    // ComputeHomingAcceleration: (target - location).GetSafeNormal() x HomingAccelerationMagnitude
    let [_, nx, ny] = safe_normal([0.0, tx - p.x, ty - p.y], SMALL_NUMBER);
    Some(V2 { x: nx * s.home_mag, y: ny * s.home_mag })
}

/// The aimed rocket's `aimDownOn`: its own AimDownProjectileMovement. Called by `actors::projectile::aim_down_on`.
pub fn aim_down_on(p: &Projectile, s: &AimRocket) -> bool {
    let _ = p;
    s.aim_down
}

fn aim_construct(w: &World, p: &mut Projectile) {
    let (home_on, home_mag) = match class_def(w, p).and_then(|d| obj(&d.components, "ProjectileMovement")) {
        Some(pm) => (flag(pm, "bIsHomingProjectile"), num_or(pm, "HomingAccelerationMagnitude", 0.0)),
        None => (false, 0.0),
    };
    let aim_down = matches!(class_p(w, p).get("AimDownProjectileMovement"), Some(mvs_data::value::Value::Bool(true)));
    let at = life_span_at(w, p);
    p.ext = ProjectileExt::AimRocket(AimRocket {
        home_on,
        home_mag,
        aim_down,
        handler_hit: p.hit.clone(),
        life_span_at: at,
        ..AimRocket::default()
    });
}

fn loop_key(p: &Projectile) -> SoundKey {
    SoundKey { tag: name!("tjaim"), n: p.seq }
}

/// The HitBox sphere against one of Jerry's upright capsules (Deviation kicked d).
fn over_capsule(w: &World, id: ProjectileId, jx: Real, jy: Real, c: Option<&mvs_data::value::Props>) -> bool {
    let Some(c) = c else { return false };
    let s = obj(c, "RelativeScale3D").map_or(1.0, |v| num_or(v, "Z", 1.0));
    let r = num_or(c, "CapsuleRadius", 0.0) * s;
    let hh = num_or(c, "CapsuleHalfHeight", 0.0) * s;
    let seg = crate::math::max(hh - r, 0.0);
    let p = pr(w, id);
    let (hx, hy) = hit_xy(w, p);
    let cy = crate::math::max(jy - seg, crate::math::min(hy, jy + seg));
    let (qx, qy, rr) = (hx - jx, hy - cy, p.hit_radius + r);
    qx * qx + qy * qy <= rr * rr
}

/// The HitBox's ComponentBeginOverlap with a Jerry (@4007): his team is the rocket's -> the BigHit packet and HR_03.
fn jerry_begin(w: &mut World, id: ProjectileId, j: ActorId) {
    let team = w.fighter(owner(w, id)).team.team;
    let jt = w.actors.actors.get(j).map(|a| w.fighter(a.owner).team.team);
    if jt != Some(team) {
        return; // Comp_Actor_Destructible.TeamIndex: the owner's
    }
    let mut h = ar(w, id).var_hit.clone().unwrap_or_else(|| pr(w, id).hit.clone());
    h.damage = Some(8.0);
    h.base_knock_back = Some(1650.0);
    h.knock_back_scaling = Some(16.0);
    let a = ax(w, id);
    a.var_hit = Some(h);
    a.big_hit = true;
    play_montage(w, j, HR_03); // PlayMontageOnJerryServer -> Multicast -> Jerry.PlayMontage
}

fn jerry_overlaps(w: &mut World, id: ProjectileId) {
    let list: Vec<(ActorId, Name, i32)> = w
        .actors
        .actors
        .iter()
        .filter(|(_, a)| {
            a.cls == name!("Jerry_SpawnedActor_C") || a.cls == name!("Jerry_SpawnedActor_TempInvincibility_C")
        })
        .map(|(i, a)| (i, a.cls, a.seq))
        .collect();
    for (j, cls, seq) in list {
        if pr(w, id).dead {
            return;
        }
        let Some(wa) = w.actors.actors.get(j) else { continue };
        let (own, dead) = (wa.owner, wa.dead);
        let Some((jx, jy)) = jerry_of(w, j).map(|s| (s.x, s.y)) else { continue };
        for c in 0..2 {
            let key = seq * 2 + c;
            let on = hit_collision(pr(w, id)) && !dead && {
                let cs = actor_class_def_comp(w, own, cls, if c == 0 { "Capsule" } else { "ProjectileHurtBox" });
                over_capsule(w, id, jx, jy, cs)
            };
            let k = ar(w, id).over_jerry.iter().position(|&v| v == key);
            if on && k.is_none() {
                ax(w, id).over_jerry.push(key);
                jerry_begin(w, id, j);
            } else if !on && let Some(k) = k {
                ax(w, id).over_jerry.remove(k);
            }
        }
    }
}

fn actor_class_def_comp<'a>(w: &'a World, own: FighterId, cls: Name, k: &str) -> Option<&'a mvs_data::value::Props> {
    crate::fighter::common::actor_class_def(w, own, cls).and_then(|d| obj(&d.components, k))
}

fn aim_begin(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    let j = last_jerry(w, f);
    {
        let a = ax(w, id);
        a.jerry = j;
        if j.is_none() {
            a.no_jerry = true; // Delay(0.3) @2821
        } else {
            a.home_target = j; // @2604
        }
    }
    sound(w, id, name!("sfx_cha_tomandjerry_rocket_launch"));
    let (fighter, key) = (w.fighter(f).id, loop_key(pr(w, id)));
    w.events.push(Event::SoundStart { fighter, ev: name!("sfx_cha_tomandjerry_rocket_loop"), key });
    jerry_overlaps(w, id); // the begin overlaps after BeginPlay
}

fn aim_moved(w: &mut World, id: ProjectileId) {
    jerry_overlaps(w, id);
}

/// ReceiveEndPlay @2950.
fn aim_end_play(w: &mut World, id: ProjectileId) {
    if ar(w, id).exploded {
        return;
    }
    ax(w, id).exploded = true;
    let fighter = w.fighter(owner(w, id)).id;
    let key = loop_key(pr(w, id));
    w.events.push(Event::SoundStop {
        fighter,
        key,
        fade: None,
        ev: Some(name!("sfx_cha_tomandjerry_rocket_loop_stop")),
        curve: None,
        cut: false,
    });
    if reset_pass(w) {
        return; // the Lab reset's end pass acts on the old pawns
    }
    let big = ar(w, id).big_hit;
    explosion_fx(w, id, if big { 1.0 } else { 0.5 }); // BoomMulticast(BigHit) @3575
    sound(w, id, name!("sfx_cha_tomandjerry_rocket_hit")); // @3431, then _rocket_loop_stop @3509 (above)
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    let types = [crate::real(OBJ_PHYSICS_BODY), crate::real(OBJ_PAWN), crate::real(OBJ_HURT_BOX)];
    let ignore = [ar(w, id).hit_actor, ar(w, id).blocking_actor];
    let mut hits: Vec<TraceHit> = Vec::new();
    sphere_trace_multi_for_objects(
        w,
        x,
        y,
        x,
        y,
        if big { 350.0 } else { 200.0 },
        &types,
        &ignore,
        Some(ActorRef::Projectile(id)),
        &mut hits,
    );
    let found: Vec<Option<ActorRef>> = hits.iter().map(|h| h.actor).collect();
    let packet = ar(w, id).var_hit.clone().unwrap_or_else(|| pr(w, id).hit.clone());
    explode(w, id, &found, &packet, None);
}

fn aim_step(w: &mut World, id: ProjectileId) -> bool {
    if life_over(pr(w, id), ar(w, id).life_span_at) {
        return true;
    }
    // BeginPlay's Delays, from the spawn frame's tick on.
    let age = crate::real(pr(w, id).age);
    if age == latent_frames(0.033) {
        ax(w, id).aim_down = true;
    }
    if age == latent_frames(0.2) {
        ax(w, id).home_mag = 13000.0; // and AcelerationGate (read nowhere)
    }
    if ar(w, id).no_jerry && age == latent_frames(0.3) {
        return true;
    }
    if ar(w, id).reflect_sound > 0.0 {
        let a = ax(w, id);
        a.reflect_sound -= 1.0;
        if a.reflect_sound == 0.0 {
            sound(w, id, name!("sfx_cha_tomandjerry_rocket_reflected"));
        }
    }
    false
}

/// @5137: HitActor, then the parent's HitEnemyPlayer.
fn aim_hit_enemy(w: &mut World, id: ProjectileId, v: FighterId) {
    ax(w, id).hit_actor = Some(ActorRef::Fighter(v));
}

fn aim_blocked(w: &mut World, id: ProjectileId, by: ActorRef) {
    ax(w, id).blocking_actor = Some(by);
    if flag(class_p(w, pr(w, id)), "DestroyOnBlocked") {
        pm(w, id).blocked_destroy = true; // the parent's Blocked
    }
}

fn aim_destroyed(w: &mut World, id: ProjectileId) {
    aim_end_play(w, id);
}

fn aim_end(w: &mut World, id: ProjectileId) {
    aim_end_play(w, id);
}

/// BPI_Reflected scaled the variable HitBoxDamage and sent it to the handler (UpdateHitBoxDamage @4931): with a BigHit
/// the variable is the 8 / 1650 / 16 packet, scaled by the factors the port applied to the handler's (Deviation
/// kicked e).
fn aim_reflected(w: &mut World, a: ActorRef) {
    let ActorRef::Projectile(id) = a else { return };
    let h = pr(w, id).hit.clone();
    let old = ar(w, id).handler_hit.clone();
    if let Some(v) = ar(w, id).var_hit.clone() {
        let kb = match old.base_knock_back {
            Some(o) if o != 0.0 => h.base_knock_back.unwrap_or(0.0) / o,
            _ => 1.0,
        };
        let dmg = match old.damage {
            Some(o) if o != 0.0 => h.damage.unwrap_or(0.0) / o,
            _ => 1.0,
        };
        let mut n = v.clone();
        n.base_knock_back = Some(v.base_knock_back.unwrap_or(0.0) * kb);
        n.damage = Some(v.damage.unwrap_or(0.0) * dmg);
        ax(w, id).var_hit = Some(n.clone());
        pm(w, id).hit = n;
    }
    let cur = pr(w, id).hit.clone();
    ax(w, id).handler_hit = cur;
}

/// @4386, behind the parent's MulticastReflect (its clearHoming turned the homing off: `write_homing`).
fn aim_multicast(w: &mut World, a: ActorRef, by: FighterId, pause: bool) {
    let _ = (by, pause);
    let ActorRef::Projectile(id) = a else { return };
    ax(w, id).reflected = true;
    set_actor_scale(pm(w, id), 1.5); // Capsule.SetWorldScale3D: the root
    let s = ax(w, id);
    s.aim_down = false;
    if s.reflect_sound == 0.0 {
        s.reflect_sound = latent_frames(0.066); // a Delay ignores a call while pending
    }
}

/// This file's registry entries (JerryKickedRocket_C, TomAimRocket1_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let kicked = ProjectileHooks {
        construct: Some(kicked_construct),
        begin: Some(kicked_begin),
        step: Some(kicked_step),
        blocked: Some(kicked_blocked),
        destroyed: Some(kicked_destroyed),
        end: Some(kicked_end),
        reflect: ReflectHooks { multicast: Some(kicked_multicast), ..ReflectHooks::default() },
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        KICKED_ROCKET,
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::JerryKickedRocket), ActorHooks::Projectile(kicked)),
    )?;
    let aim = ProjectileHooks {
        construct: Some(aim_construct),
        begin: Some(aim_begin),
        moved: Some(aim_moved),
        step: Some(aim_step),
        hit_enemy: Some(aim_hit_enemy),
        blocked: Some(aim_blocked),
        destroyed: Some(aim_destroyed),
        end: Some(aim_end),
        reflect: ReflectHooks {
            reflected: Some(aim_reflected),
            multicast: Some(aim_multicast),
            ..ReflectHooks::default()
        },
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        AIM_ROCKET,
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::TomAimRocket1), ActorHooks::Projectile(aim)),
    )?;
    Ok(())
}
