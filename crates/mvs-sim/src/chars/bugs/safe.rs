//! Bugs Bunny's safe: BugsBunnySafe_C ([`BugsBunnySafe`]), its data-only child
//! BugsBunnyNeutralSafe_C, and BugsBunny_SafeDrop_C ([`SafeDrop`]), the target the ground special leaves.
//! BugsBunny_Attack_Air_PaintTarget2_Montage spawns the safe itself (socket Root + (0, 0, 50), straight down at 5000);
//! BugsBunny_Attack_PaintTarget_Montage spawns the target, which spawns the safe 1.5 s on.
//!
//! Both are direct SpawnedActorParent children, projectiles of the world's list: the parent's movement
//! (ProjectileMovement, no bounce), lifetime, blast-box check and spawner list. Neither has a component tagged HitBox
//! that deals the parent's hit (the safe's Hitbox is tagged IgnoreHitstun / collision; the target's HitBox is a box
//! the parent's circle test cannot take), so HitBoxOn? stays false and each class deals its own hit in `overlap` (the
//! fighters' root capsules against the box).
//!
//! The safe's Capsule (radius = half height = 170) moves by the movement component until it lands or is knocked;
//! from then on it simulates (Capsule.SetSimulatePhysics(true)): a sphere of the stage block
//! (`mvs_collision::rigid_sphere` `sphere_step`: X translation and X rotation locked, so it slides and cannot roll;
//! the DamageCollision box welded into its body for the mass: 21000 kg, k 2.4282), stepped behind the actor's tick
//! (the physics scene steps after the tick groups). The Capsule ignores Platform, so no stage platform meets it.
//! Sim space: x = -UE Y, y = UE Z. The body's velocity is the safe's own (`bvx`, `bvy`); `vx` / `vy` stay the
//! ProjectileMovement's (a reflect writes those, which the deactivated movement never uses).
//!
//! The safe's graph, by entry:
//! - ReceiveBeginPlay @3180 (behind the parent's first: NumberOfActorsAllowed 1 @5864-6515 destroys the oldest safe of
//!   the owner's SpawnedActors, ForceSpawnPositionInsideBlastBox clamps into the blast box, the ability trigger of
//!   index 2): OriginalInstigator = the Instigator, CurrentHP = MaxHP, the DamageManager bindings (KnockedBack ->
//!   OnHitByFighter, HitByAlly -> OnHitByAlly, HitByEnemy -> OnHitByEnemy), SetHitbox(true); BPI_IgnoreWallbounce
//!   Collision(self, true) on every BP_FighterCharacter that implements it @2749-2795 (Pawn_C017's alone, whose
//!   implementation @7484 is an empty pop_flow: nothing).
//! - ReceiveTick @5376: HitboxActive && now - TimeSinceLastHit >= BufferForHitboxActivate (.35) && |Capsule's
//!   GetComponentVelocity| <= StoppingSpeed (500) -> SetVelocityMulticast((0, 0, .1), false), SetHitbox(false); then
//!   the GameState's BPI_GetWallbounceCollisions, each one's BPI_IgnoreWallbounceCollision(self, true) @1583-1629
//!   (those walls ignore the safe: the body meets the stage block alone, so nothing to do).
//! - The Hitbox's begin overlap @4248: not in HitActors -> pfgDoDamage(Hitbox, the actor, HitBoxDamage) and on its
//!   success HitActors[actor] = now, CurrentHP -= 3, HandleDamageTaken.
//! - ReceiveAnyDamage @6603: CurrentHP -= Damage, HandleDamageTaken.
//! - HandleDamageTaken @6690: HP <= 0 -> a gate closed for good, Delay(.2) @1694 -> ReleaseToPool @153 (CanBePooled
//!   false: K2_DestroyActor); else SafeDamageShake.
//! - ReceiveEndPlay / OnReleasedToPool @1765: SpawnableItems non-empty -> ActorClassSpawnedAtDeath = a random one
//!   (RandomIntegerInRange(0, n - 1)), then the parent's EndPlay: RandomBoolWithWeight(SpawnAtDeathChance) and the
//!   owner's Comp_Spawner.CompSpawnActor(that class, the actor's transform, direction (0, 0, 0), speed 0).
//! - The Capsule's hit @7833 -> @2120: !HasLanded -> HasLanded, Delay(.02) @2200 -> @168: Z -= the persistent Hit's
//!   Distance (BPI_Teleport, no sweep), SetHitbox(false), SimulatePhysics(true), physics velocity 0, rotation 0,
//!   ProjectileMovement Velocity 0 and Deactivate, LandVFXMulticast (sfx_cha_bugs_painttarget_impact).
//! - OnHitByAlly @9355, OnHitByEnemy @11301, OnHitByFighter @4188, Handle Hit, KnockbackSafe @9697,
//!   SetVelocityMulticast @7340, SetInstigator @6957, Blocked @11285 -> @2800, Reflected @11055: at their functions.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-keeping negated tests

use indexmap::IndexSet;
use mvs_collision::Ground;
use mvs_collision::rigid_sphere::{Body, Settings, Solver, Static};
use mvs_data::value::Props;

use crate::Real;
use crate::State;
use crate::actors::projectile::{
    OwnerOverride, ProjectileExt, class_def, class_p, hitboxes_off, pause_max_lifetime, unpause_max_lifetime,
};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::boxes::WorldSphere;
use crate::combat::elem::{charge_kb, charge_pct, elem_buff_mut, instigator_charge};
use crate::combat::hits::{Dealer, actor_hit_enemy, ally_hit, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::{DataVec, HitPacket};
use crate::consts::{DELTA_SECONDS, SOLID_ITEM};
use crate::events::Event;
use crate::fighter::common::{actor_is_a, actor_sound_from, latent_frames, lookup_owner, owner_team, warn_text};
use crate::fighter::control::{attack_dodged, data_name, dodge_over};
use crate::fighter::moves::V2;
use crate::fighter::traces::solids::sphere_static;
use crate::fighter::traces::{
    TRACE_PROJECTILE_GROUND, TRACE_PROJECTILE_GROUND_NO_PLATFORM, TraceBody, body_info, box_trace_single,
    sphere_trace_single,
};
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, facing, in_hitstun, invulnerable, no_collision};
use crate::math::ue::{KINDA_SMALL_NUMBER, normal, safe_normal2d, size2d};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::body_settings::{body_mass, box_mass, sphere_settings, sphyl_mass, welded_mass};
use crate::registry::entries::{
    ActorEntry, ActorHooks, CarryHooks, ProjectileHooks, ProjectileReceiveHit, ReflectHooks,
};
use crate::registry::props::{flag, num, obj, text};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;
use crate::world::spawning::{comp_spawn_item, comp_spawn_projectile};
use crate::world::stage_query::{StageShapes, rigid_statics, static_solid_shapes};

pub const SAFE: &str = "BugsBunnySafe_C";
pub const SAFE_NEUTRAL: &str = "BugsBunnyNeutralSafe_C";
pub const SAFE_DROP: &str = "BugsBunny_SafeDrop_C";
/// Handle Hit's Superman_Ice_Breath cast @1133 (its Educated child passes it).
const ICE_BREATH: Name = name!("Superman_IceBreath_C");

// ---------------------------------------------------------------- the class's body

/// `SphereSettings` of the Capsule's FBodyInstance, as state numbers.
#[derive(State, Copy, Debug, Default)]
pub struct BodySet {
    pub linear_damping: Real,
    pub angular_damping: Real,
    pub restitution: Real,
    pub friction: Real,
    pub lock_spin: bool,
    pub lock_y: bool,
    pub lock_z: bool,
}

impl BodySet {
    fn of(s: Settings) -> Self {
        BodySet {
            linear_damping: crate::math::narrow(s.linear_damping),
            angular_damping: crate::math::narrow(s.angular_damping),
            restitution: crate::math::narrow(s.restitution),
            friction: crate::math::narrow(s.friction),
            lock_spin: s.lock_spin,
            lock_y: s.lock_y,
            lock_z: s.lock_z,
        }
    }

    fn settings(self) -> Settings {
        Settings {
            linear_damping: f64::from(self.linear_damping),
            angular_damping: f64::from(self.angular_damping),
            restitution: f64::from(self.restitution),
            friction: f64::from(self.friction),
            lock_spin: self.lock_spin,
            lock_y: self.lock_y,
            lock_z: self.lock_z,
        }
    }
}

/// The class's body, from its template (`SafeStatic`).
#[derive(State, Copy, Debug, Default)]
pub struct SafeStatic {
    /// The Capsule's FBodyInstance (`None`: not stepped, warned by `sphere_settings`).
    pub set: Option<BodySet>,
    /// m R^2 / I of the welded body about UE X.
    pub k: Real,
    pub radius: Real,
    /// The Hitbox's half extents in the plane (UE Y, Z).
    pub hit_hx: Real,
    pub hit_hy: Real,
    /// The DamageCollision's (UE X: its depth, Y, Z).
    pub dmg_hd: Real,
    pub dmg_hx: Real,
    pub dmg_hy: Real,
}

/// A number of the class data the graph reads (no default: a missing one warns and reads 0).
fn num_of(w: &World, cls: Name, p: Option<&Props>, key: &str) -> Real {
    p.and_then(|p| num(p, key)).unwrap_or_else(|| {
        warn_text(w, &format!("{} data missing: {key}", w.names.str(cls)));
        0.0
    })
}

/// `staticOf`: the class's body from its template; `None` for a class with no data.
fn compute_static(w: &World, p: &Projectile) -> Option<SafeStatic> {
    let a = class_def(w, p)?;
    let cls = w.names.str(p.cls);
    let comp = |n: &str| obj(&a.components, n);
    let (cap, dc, hb) = (comp("Capsule"), comp("DamageCollision"), comp("Hitbox"));
    let r = num_of(w, p.cls, cap, "CapsuleRadius");
    let hh = num_of(w, p.cls, cap, "CapsuleHalfHeight");
    let cap_bi = cap.and_then(|c| obj(c, "BodyInstance"));
    let set = sphere_settings(w, cap_bi, cls);
    if hh != r {
        warn_text(w, &format!("{cls}: the Capsule is not a sphere (half height {hh}, radius {r}): stepped as one"));
    }
    let e = dc.and_then(|d| obj(d, "BoxExtent"));
    // FBodyInstance::Weld at SetSimulatePhysics: the Capsule's sphyl and the DamageCollision box (QueryAndPhysics, at
    // the Capsule's origin) in one body; CharacterCollision (NoCollision) has none.
    let (ex, ey, ez) = (num_of(w, p.cls, e, "X"), num_of(w, p.cls, e, "Y"), num_of(w, p.cls, e, "Z"));
    let capsule = body_mass(w, cap_bi, &[sphyl_mass(r, hh, 0.0)], cls);
    let damage = body_mass(
        w,
        dc.and_then(|d| obj(d, "BodyInstance")),
        &[box_mass(ex, ey, ez, [0.0, 0.0, 0.0])],
        &format!("{cls} DamageCollision"),
    );
    let k = match (capsule, damage) {
        (Some(c), Some(d)) => welded_mass(w, &[c, d], r, cls).k,
        _ => 2.5,
    };
    let he = hb.and_then(|h| obj(h, "BoxExtent"));
    Some(SafeStatic {
        set: set.map(BodySet::of),
        k: crate::math::narrow(k),
        radius: r,
        hit_hx: num_of(w, p.cls, he, "Y"),
        hit_hy: num_of(w, p.cls, he, "Z"),
        dmg_hd: ex,
        dmg_hx: ey,
        dmg_hy: ez,
    })
}

/// The class's body: the one the safe made at its construction, else computed from the data.
fn static_of(w: &World, p: &Projectile) -> Option<SafeStatic> {
    match &p.ext {
        ProjectileExt::BugsBunnySafe(s) if s.st.is_some() => s.st,
        _ => compute_static(w, p),
    }
}

// ---------------------------------------------------------------- BugsBunnySafe_C (and BugsBunnyNeutralSafe_C)

/// `BugsBunnySafe extends Projectile`: what the class adds. The owner: the spawning fighter (a notify's, the
/// target's, Taz's burp), then whoever SetInstigator names; none for one the world spawned (MapRandomActorSpawnArea's
/// SpawnCenterActor: team -1, its owner the area) until a hit names one.
#[derive(State, Debug)]
pub struct BugsBunnySafe {
    /// OriginalInstigator (a fighter's id; -1: None).
    pub orig_inst: i32,
    /// CurrentHP.
    pub hp: Real,
    /// MaxHP (14; NeutralSafe 32).
    pub max_hp: Real,
    /// HitboxActive (SetHitbox): the Hitbox's collision QueryOnly / NoCollision.
    pub box_on: bool,
    /// HitActors (fighters' ids), cleared by SetHitbox(false).
    pub safe_hits: IndexSet<i32>,
    /// The fighters the Hitbox overlaps now (its begin events).
    pub inside: IndexSet<i32>,
    /// TimeSinceLastHit (game seconds).
    pub last_hit: Real,
    /// HasLanded.
    pub landed: bool,
    /// The persistent Hit's Distance.
    pub land_dist: Real,
    /// The age the Delay(.02) @2200 fires at.
    pub land_at: i32,
    /// The age the landing @168 ran at (the view's impact).
    pub landed_age: i32,
    /// Capsule.IsSimulatingPhysics.
    pub sim: bool,
    /// ProjectileMovement.IsActive.
    pub pm_active: bool,
    /// The body's linear velocity.
    pub bvx: Real,
    pub bvy: Real,
    /// Its spin (0 under the X rotation lock).
    pub bspin: Real,
    /// Comp_Pawn_HitBoxHandler.HitBoxHandlerFacingLeft.
    pub facing_left: bool,
    /// The death gate @1648 closed.
    pub dying: bool,
    /// The age its Delay(.2) fires at.
    pub death_at: i32,
    /// Blocked's gate @2800.
    pub block_open: bool,
    /// The age its Delay(.5) @3125 fires at.
    pub block_at: i32,
    /// The age Reflected's Delay(.5) fires at (@11175 -> @651).
    pub reflect_at: i32,
    /// The inactive branch's Delay(.5) (@11230 -> @1144).
    pub reflect_idle_at: i32,
    /// The age SafeDamageShake last started at (the view's).
    pub shake_age: i32,
    /// ActorClassSpawnedAtDeath, as @1765 chose it.
    pub loot: Option<Name>,
    /// The class's body, made at the construction.
    pub st: Option<SafeStatic>,
}

impl Default for BugsBunnySafe {
    /// The field initial values.
    fn default() -> Self {
        BugsBunnySafe {
            orig_inst: -1,
            hp: 0.0,
            max_hp: 0.0,
            box_on: false,
            safe_hits: IndexSet::new(),
            inside: IndexSet::new(),
            last_hit: 0.0,
            landed: false,
            land_dist: 0.0,
            land_at: -1,
            landed_age: -1,
            sim: false,
            pm_active: true,
            bvx: 0.0,
            bvy: 0.0,
            bspin: 0.0,
            facing_left: false,
            dying: false,
            death_at: -1,
            block_open: true,
            block_at: -1,
            reflect_at: -1,
            reflect_idle_at: -1,
            shake_age: -1,
            loot: None,
            st: None,
        }
    }
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn safe(p: &Projectile) -> &BugsBunnySafe {
    match &p.ext {
        ProjectileExt::BugsBunnySafe(s) => s,
        e => panic!("a safe without its state: {e:?}"),
    }
}

fn safe_mut(p: &mut Projectile) -> &mut BugsBunnySafe {
    match &mut p.ext {
        ProjectileExt::BugsBunnySafe(s) => s,
        e => panic!("a safe without its state: {e:?}"),
    }
}

fn drop_mut(p: &mut Projectile) -> &mut SafeDrop {
    match &mut p.ext {
        ProjectileExt::SafeDrop(s) => s,
        e => panic!("a safe drop without its state: {e:?}"),
    }
}

fn drop_of(p: &Projectile) -> &SafeDrop {
    match &p.ext {
        ProjectileExt::SafeDrop(s) => s,
        e => panic!("a safe drop without its state: {e:?}"),
    }
}

/// A number of the actor's class data.
fn cfg(w: &World, p: &Projectile, key: &str) -> Real {
    num_of(w, p.cls, Some(class_p(w, p)), key)
}

/// GetGameTimeInSeconds: the world clock, every fighter's.
fn now(w: &World, id: ProjectileId) -> Real {
    w.fighter(lookup_owner(w, ActorRef::Projectile(id))).clock.time
}

/// GetVelocity / the Capsule's GetComponentVelocity: the body's when it simulates, else the movement's (the stored
/// ComponentVelocity: `Target::Stored`).
fn velocity(p: &Projectile) -> (Real, Real) {
    let s = safe(p);
    if s.sim { (s.bvx, s.bvy) } else { (p.vx, p.vy) }
}

/// The same read for another module (BotDetectEnemy_BTS's VSizeSquared(GetVelocity) @4466): sim x, y.
pub fn safe_velocity(p: &Projectile) -> (Real, Real) {
    velocity(p)
}

/// A fighter's root capsule against an upright box of the play plane, centre (x, y), half extents (hx, hy); a touch is
/// no overlap.
// Deviation: the box against the root capsule (Pawn) alone, in the play plane.
fn capsule_in_box(w: &World, v: FighterId, x: Real, y: Real, hx: Real, hy: Real) -> bool {
    let r = cap_radius(w, v);
    let half = capsule_half(w, v) - r;
    let (vx, ay) = (w.fighter(v).body.x, actor_y(w, v));
    let dx = max(max(x - hx - vx, 0.0), vx - (x + hx));
    let dy = max(max(y - hy - (ay + half), 0.0), ay - half - (y + hy));
    size2d([dx, dy]) < r
}

/// SetHitbox @6356 (it does not write HitBoxOn?): the Hitbox's collision; off clears HitActors.
fn set_box(s: &mut BugsBunnySafe, on: bool) {
    s.box_on = on;
    if !on {
        s.safe_hits.clear();
        s.inside.clear();
    }
}

/// Capsule.SetSimulatePhysics(true) (@546, @9951): the PhysX actor turns dynamic.
// Unverified: the body starts at rest (the kinematic body's velocity is 0; whether SetSimulatePhysics hands the
// component's velocity on is not read), so a knock in the air reads |GetVelocity| = 0 in SetVelocityMulticast.
fn simulate(p: &mut Projectile) {
    if safe(p).sim {
        return;
    }
    let s = safe_mut(p);
    s.sim = true;
    s.bvx = 0.0;
    s.bvy = 0.0;
    s.bspin = 0.0;
    p.stopped = true; // the movement no longer moves the root
}

/// SetVelocityMulticast @7340 (rx, ry: the asked velocity in the plane).
fn set_velocity_multicast(w: &mut World, id: ProjectileId, rx: Real, ry: Real, new_instigator: bool) {
    let p = pr(w, id);
    let (vx, vy) = velocity(p);
    let ally = if new_instigator { cfg(w, p, "AllyHitMultiplier") } else { 1.0 };
    let cur = size2d([vx, vy]) * ally;
    let speed = min(cfg(w, p, "Knockspeed"), max(max(size2d([rx, ry]), cfg(w, p, "MinimumKnockbackSpeed")), cur));
    let [_, nx, ny] = normal([0.0, rx, ry], KINDA_SMALL_NUMBER); // @7377
    // Capsule.SetPhysicsLinearVelocity @7773: the root's body.
    // Unverified: on a root that does not simulate (the Tick's call in the frames between the movement's stop on
    // landing and the landing's Delay) the write is dropped; FBodyInstance::SetLinearVelocity on a kinematic actor is
    // not read (`velocity_target`'s null case).
    let s = safe_mut(pm(w, id));
    if !s.sim {
        return;
    }
    s.bvx = nx * speed;
    s.bvy = ny * speed;
}

/// KnockbackSafe @9697 (`inst`: the Pawn; `kb`: KnockbackLength; (dx, dy): KnockbackDirection in the plane).
// Unverified: the direction the DamageManager's HitByAlly / HitByEnemy hands on is taken in the world's horizontal
// sense (the hit's dx); the build's MakeVector(0, X, Y) puts the vector's X on UE Y as it comes.
fn knockback_safe(w: &mut World, id: ProjectileId, inst: FighterId, kb: Real, dx: Real, dy: Real, facing_left: bool) {
    let t = now(w, id);
    let p = pr(w, id);
    if !((t - safe(p).last_hit) >= cfg(w, p, "BufferForHitboxActivate")) {
        return;
    }
    safe_mut(pm(w, id)).last_hit = t;
    simulate(pm(w, id));
    safe_mut(pm(w, id)).pm_active = false; // ProjectileMovement.SetActive(false)
    // K2_SetActorRotation(0): the actor has no turn (the Hitbox and DamageCollision are square about Z).
    let [nx, ny] = safe_normal2d([dx, min(dy, 0.25)]); // Normal2D @10274
    let ts = min(max(min(max(kb, 0.0), 2500.0) / 2500.0, 0.0), 1.0);
    let len = 100.0 + (cfg(w, pr(w, id), "Knockspeed") - 100.0) * ts;
    let new_inst = Some(inst) != pr(w, id).owner; // NotEqual(Pawn, GetOwner) @10563
    set_velocity_multicast(w, id, nx * len, ny * len, new_inst);
    set_instigator(w, id, inst); // @10633
    let p = pm(w, id);
    safe_mut(p).facing_left = facing_left;
    facing_packet(p);
    set_box(safe_mut(p), true);
}

/// SetInstigator @6957, which writes neither the actor's Instigator nor a spawner list: with the fighter's PlayerData
/// (GetByFighter; a minion has none, and @7066 pops the flow), Comp_Actor_Destructible.TeamIndex = its GetTeamIndex
/// @7230 and SetOwner @7316. The team is the owner's, so the two go together in `owner`. GetInstigator stays the
/// spawner's (the hits' credit reads it: `instigator`), and the safe stays in the SpawnedActors of the Comp_Spawner
/// that made it (its Die With Instigator, NumberOfActorsAllowed and the bots' walk: `spawner`). A safe the world
/// spawned has neither, and keeps none (the `instigator` and `spawner` arms of the accessors).
fn set_instigator(w: &mut World, id: ProjectileId, f: FighterId) {
    let (fid, minion) = (w.fighter(f).id, w.fighter(f).life.is_minion);
    let p = pm(w, id);
    if p.owner == Some(f) || minion {
        return;
    }
    let inst = crate::actors::projectile::instigator(p);
    let list = crate::actors::projectile::spawner(p);
    p.owner = Some(f);
    p.owner_id = fid;
    let keep = |o: Option<FighterId>| match o {
        Some(o) if o != f => OwnerOverride::Fighter(o),
        _ => OwnerOverride::None,
    };
    p.inst_override = keep(inst);
    p.list_owner = keep(list);
}

/// @844: HitBoxDamage.KnockBackDirectionVector.X = FacingLeft ? -|X| : |X|.
fn facing_packet(p: &mut Projectile) {
    let left = safe(p).facing_left;
    if let Some(d) = p.hit.knock_back_direction_vector.as_mut() {
        let x = d.x.unwrap_or(0.0).abs();
        d.x = Some(if left { -x } else { x });
    }
}

/// Handle Hit (`by`: the attacker; (dx, dy), `kb`: the hit's direction and BaseKnockBack; `charge`: the attacker's
/// charge).
fn handle_hit(w: &mut World, id: ProjectileId, by: ActorRef, dx: Real, dy: Real, kb: Real, charge: Real) -> bool {
    let inst = match by {
        ActorRef::Fighter(f) => {
            // Unverified: CheckHitstunActive as inHitstun (hitstun or a buff's stun).
            if in_hitstun(w, f) {
                return false;
            }
            f
        }
        ActorRef::Projectile(b) => {
            // @1133-1358: a Superman_Ice_Breath with a valid Instigator: the team test, past the hitstun gate.
            let Some(bp) = w.actors.projectiles.get(b) else { return false };
            let Some(o) = bp.owner else { return false };
            if actor_is_a(w, ActorRef::Fighter(o), bp.cls, ICE_BREATH) != Some(true) {
                return false;
            }
            // Superman_Ice_Breath.GetInstigator @1212 (an owned breath's: never None)
            crate::actors::projectile::instigator(bp).unwrap_or(o)
        }
        _ => return false,
    };
    // @237-437: the attacker's Destructible team == the safe's
    if w.fighter(inst).team.team != owner_team(w, ActorRef::Projectile(id)) {
        return false;
    }
    knockback_safe(w, id, inst, kb * charge, dx, dy, facing(w, inst) < 0.0);
    // @672-1117: another fighter than OriginalInstigator: IncrementCoopActions (no port), Success true.
    w.fighter(inst).id != safe(pr(w, id)).orig_inst
}

/// HandleDamageTaken @6690.
fn damage_taken(p: &mut Projectile) {
    let age = p.age;
    let s = safe_mut(p);
    if s.hp <= 0.0 {
        if s.dying {
            return;
        }
        s.dying = true;
        s.death_at = to_int32(crate::real(age) + latent_frames(0.2));
        return;
    }
    s.shake_age = age; // SafeDamageShake
}

/// The dealer of the safe, and its packet (cloned: the hit functions take the World whole).
fn dealt(w: &World, id: ProjectileId) -> (Dealer, HitPacket) {
    let a = ActorRef::Projectile(id);
    (dealer(w, a), pr(w, id).hit.clone())
}

/// The safe's Hitbox against a fighter (its begin overlap @4248).
fn hitbox_overlap(w: &mut World, id: ProjectileId, vic: FighterId) {
    let a = ActorRef::Projectile(id);
    let p = pr(w, id);
    let vid = w.fighter(vic).id;
    let st = static_of(w, p);
    let on = st.is_some_and(|st| {
        safe(p).box_on
            && !p.dead
            && !p.no_collision
            && !(w.fighter(vic).life.dead > 0.0)
            && !no_collision(w, vic)
            && capsule_in_box(w, vic, p.x, p.y, st.hit_hx, st.hit_hy)
    });
    if !on {
        safe_mut(pm(w, id)).inside.shift_remove(&vid);
        return;
    }
    let s = safe_mut(pm(w, id));
    if !s.inside.insert(vid) || s.safe_hits.contains(&vid) {
        return;
    }
    // pfgDoDamage(Hitbox, the fighter, HitBoxDamage, null, [], [], false, false): Success is the hit's.
    // Deviation: the Hitbox's other overlaps (DamageDestructible, ItemDestructible, ProjectileDestructible: the
    // spawned actors' and items' damage bodies) are not tested.
    let p = pr(w, id);
    let inst = p.owner; // (none: team -1, every fighter an enemy)
    let (at, attack_id) = (V2 { x: p.x, y: p.y }, p.attack_id);
    let bkb = p.hit.base_knock_back.unwrap_or(0.0);
    if w.fighter(vic).team.team != owner_team(w, a) {
        let (d, packet) = dealt(w, id);
        if crate::combat::phases::no_hit_from(w, a, vic) || projectile_blocks(w, &d, vic) {
            return;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, a, vic, attack_id, bkb, tick, None, false);
            return;
        }
        dodge_over(w, a, vic);
        let pct = charge_pct(w, a);
        // @4493: HitSuccessful false pops
        if !apply_projectile_hit(w, &d, vic, &packet, &[], at, None, &[], pct) {
            return;
        }
        actor_hit_enemy(w, &d, vic); // pfgDoDamage @4362 -> HitEnemyPlayer @22770 (its granted buffs)
        let p = pm(w, id);
        let s = safe_mut(p);
        s.safe_hits.insert(vid);
        s.hp -= 3.0;
        damage_taken(p);
    } else if Some(vic) != inst {
        let (d, packet) = dealt(w, id);
        ally_hit(w, &d, vic, &packet);
    }
}

/// The body's step (the physics scene, behind the actor's tick).
fn body_step(w: &mut World, id: ProjectileId) {
    let p = pr(w, id);
    let Some(set) = static_of(w, p).and_then(|st| st.set.map(|s| (s.settings(), st.k, st.radius))) else {
        return;
    };
    // Unverified: a reflect's SetMobility(Stationary) (the grapple's setMobilityToStationary) holds the body still
    // (UPrimitiveComponent::SetMobility on a simulating body is not read).
    if p.mobility.is_some() {
        return;
    }
    let (settings, k, radius) = set;
    let s = safe(p);
    let mut body = Body {
        x: f64::from(p.x),
        y: f64::from(p.y),
        vx: f64::from(s.bvx),
        vy: f64::from(s.bvy),
        spin: f64::from(s.bspin),
        radius: f64::from(radius),
        ..Body::default()
    };
    // SetActorEnableCollision(false) (the tunnel's carry): the body meets nothing.
    let nowhere = p.no_collision;
    // (the stage solids that block Item, the Capsule's object type, as the floor)
    let mut solids = StageShapes::default();
    if !nowhere {
        static_solid_shapes(w, SOLID_ITEM, name!("BugsBunnySafe body"), &mut solids);
    }
    let list: Vec<Static> = rigid_statics(w, &solids);
    let floor = if nowhere {
        sphere_static(w, Ground { left: 0.0, right: 0.0, top: -1e9, bottom: -1e9 - 1.0 })
    } else {
        sphere_static(w, w.stage.ground)
    };
    let statics = if list.is_empty() { None } else { Some(list.as_slice()) };
    let imp = Solver::default().sphere_step(&mut body, &floor, statics, DELTA_SECONDS, &settings, k);
    let p = pm(w, id);
    p.x = crate::math::narrow(body.x);
    p.y = crate::math::narrow(body.y);
    let age = p.age;
    let s = safe_mut(p);
    s.bvx = crate::math::narrow(body.vx);
    s.bvy = crate::math::narrow(body.vy);
    s.bspin = crate::math::narrow(body.spin);
    // The Capsule's rigid-body hit (bNotifyRigidBodyCollision) ahead of a landing: the same handler @7833.
    // Unverified: a contact of the step is the hit event (its threshold is not read), its Hit.Distance 0 (a physics
    // hit's FHitResult carries none), and the Delay counts from the next frame.
    if imp > 0.0 && !s.landed {
        land(s, 0.0, to_int32(crate::real(age) + latent_frames(0.02)));
    }
}

/// @2120: the Capsule's hit, the first one.
fn land(s: &mut BugsBunnySafe, dist: Real, at: i32) {
    s.landed = true;
    s.land_dist = dist;
    s.land_at = at;
}

/// `velocityTarget`'s answers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    /// The write sets the root body's linear velocity, the read gives it.
    Body,
    /// The write is dropped, the read is zero.
    Nothing,
    /// The read of a root that does not simulate: its stored ComponentVelocity.
    Stored,
}

/// UPrimitiveComponent::SetPhysicsLinearVelocity (142d03690) and GetComponentVelocity (142cf5610) on a component of a
/// body actor: the write goes to GetBodyInstance(welded): the weld parent's body for a welded child, else its own;
/// the read is the body's velocity when IsSimulatingPhysics, else the component's stored ComponentVelocity. A child is
/// welded into the simulating root when its CollisionEnabled is PhysicsOnly or QueryAndPhysics
/// (SetInstanceSimulatePhysics 142c7d100, at the root's SetSimulatePhysics); a NoCollision one has no body (no
/// physics state: UPrimitiveComponent::ShouldCreatePhysicsState), so the write does nothing and, as no movement
/// component updates a child, the read is (0, 0, 0). `None` (warned): a case not read.
fn velocity_target(w: &World, p: &Projectile, call: &str, name: &str, root: &str) -> Option<Target> {
    let cls = w.names.str(p.cls);
    let no = |why: &str| {
        warn_text(w, &format!("{cls}: {call} velocity on {name}: {why} (not read; velocityTarget)"));
        None
    };
    let comps = class_def(w, p).map(|d| &d.components);
    let Some(c) = comps.and_then(|c| obj(c, name)) else { return no("no such component") };
    let root_sim = safe(p).sim;
    if name == root {
        return if root_sim {
            Some(Target::Body)
        } else if call == "get" {
            Some(Target::Stored)
        } else {
            no("the root does not simulate")
        };
    }
    let bi = obj(c, "BodyInstance");
    if let Some(prof) = bi.and_then(|b| text(b, "CollisionProfileName")).filter(|s| !s.is_empty() && *s != "Custom") {
        return no(&format!("collision profile {prof}"));
    }
    let Some(en) = bi.and_then(|b| text(b, "CollisionEnabled")).filter(|s| !s.is_empty()) else {
        return no("CollisionEnabled not in the data");
    };
    if en == "ECollisionEnabled::QueryOnly" {
        return no("QueryOnly"); // a physics state with no weld: its own kinematic body
    }
    if en == "ECollisionEnabled::NoCollision" {
        return Some(Target::Nothing);
    }
    let mut seen: Vec<&str> = Vec::new();
    let mut at = text(c, "AttachParent").filter(|s| !s.is_empty());
    while let Some(n) = at
        && n != root
        && !seen.contains(&n)
    {
        seen.push(n);
        at = comps.and_then(|c| obj(c, n)).and_then(|c| text(c, "AttachParent")).filter(|s| !s.is_empty());
    }
    if at != Some(root) {
        return no(&format!("not attached under {root}"));
    }
    if root_sim { Some(Target::Body) } else { no("the root does not simulate yet: its own body") }
}

/// Reflected's and @651's read / write on CharacterCollision (NoCollision: no body); a class whose data differs is
/// warned.
fn warn_if_body(w: &World, id: ProjectileId, call: &str) {
    let p = pr(w, id);
    if velocity_target(w, p, call, "CharacterCollision", "Capsule") == Some(Target::Body) {
        warn_text(w, &format!("{}: CharacterCollision reaches the body (not ported)", w.names.str(p.cls)));
    }
}

// ---------------------------------------------------------------- the kind

/// The constructor's tail: OriginalInstigator, MaxHP, CurrentHP and the packet the graph writes
/// (KnockBackDirectionVector.X @844): the actor's own copy.
fn construct_safe(w: &World, p: &mut Projectile) {
    let max_hp = cfg(w, p, "MaxHP");
    let hp = cfg(w, p, "CurrentHP");
    if p.hit.knock_back_direction_vector.is_none() {
        p.hit.knock_back_direction_vector = Some(DataVec { x: Some(0.0), y: Some(0.0), z: None });
    }
    let orig_inst = if p.owner.is_some() { p.owner_id } else { -1 };
    p.ext = ProjectileExt::BugsBunnySafe(BugsBunnySafe {
        orig_inst,
        hp,
        max_hp,
        st: compute_static(w, p),
        ..BugsBunnySafe::default()
    });
}

fn begin_safe(w: &mut World, id: ProjectileId) {
    // The parent's NumberOfActorsAllowed (the oldest safe's K2_DestroyActor: its EndPlay drops its loot) and the
    // blast-box clamp (ForceSpawnPositionInsideBlastBox) came ahead.
    let s = safe_mut(pm(w, id));
    s.hp = s.max_hp; // @3180 authority
    set_box(s, true);
}

/// The movement's sweep meets the stage block (the Capsule blocks WorldStatic and PlatformDetection, ignores
/// Platform): no other box.
fn no_boxes(_w: &mut World, _id: ProjectileId, out: &mut crate::world::stage_query::StageShapes) {
    out.boxes.clear();
    out.polys.clear();
}

/// The hit inside the movement's tick, ahead of the actor's: its Delay(.02) counts this frame. Hit.Distance: the
/// sweep's travel to the hit (TraceStart to Location). HandleImpact then stops the movement (StopSimulating).
fn receive_hit(w: &mut World, id: ProjectileId, _nx: Real, _ny: Real, _t: Real, _pen: bool) -> bool {
    let p = pm(w, id);
    let (dist, at) = (size2d([p.x - p.prev_x, p.y - p.prev_y]), to_int32(crate::real(p.age - 1) + latent_frames(0.02)));
    let s = safe_mut(p);
    if !s.landed {
        land(s, dist, at);
    }
    false
}

/// ReceiveTick @5376 and the actor's latent actions. True: K2_DestroyActor.
fn step_safe(w: &mut World, id: ProjectileId) -> bool {
    let t = now(w, id);
    let p = pr(w, id);
    if safe(p).box_on && (t - safe(p).last_hit) >= cfg(w, p, "BufferForHitboxActivate") {
        let (vx, vy) = velocity(p);
        if size2d([vx, vy]) <= cfg(w, p, "StoppingSpeed") {
            set_velocity_multicast(w, id, 0.0, 0.1, false);
            set_box(safe_mut(pm(w, id)), false);
        }
    }
    // The latent actions, in the order of their links' age.
    let a = pr(w, id).age;
    if a == safe(pr(w, id)).land_at {
        // @168: BPI_Teleport((X, Y, Z - Distance), no sweep, teleport) @506, SetHitbox(false), SimulatePhysics,
        // physics velocity 0, rotation 0, the movement's Velocity 0 and Deactivate, LandVFXMulticast.
        let p = pm(w, id);
        p.y -= safe(p).land_dist;
        set_box(safe_mut(p), false);
        simulate(p);
        p.vx = 0.0;
        p.vy = 0.0;
        let s = safe_mut(p);
        s.bvx = 0.0;
        s.bvy = 0.0;
        s.bspin = 0.0;
        s.pm_active = false;
        s.landed_age = a;
        let (x, y) = (p.x, p.y);
        let from = actor_sound_from(w, ActorRef::Projectile(id));
        w.events.push(Event::Sound {
            fighter: from.fighter,
            ev: name!("sfx_cha_bugs_painttarget_impact"),
            x: Some(x),
            y: Some(y),
            hit: None,
            pooled: from.pooled,
            key: None,
        });
    }
    let s = safe_mut(pm(w, id));
    if a == s.block_at {
        s.block_open = true;
    }
    if a == s.reflect_at {
        // @651: CharacterCollision.SetPhysicsLinearVelocity(-VelocityBeforeReflect): the NoCollision box has no body
        // (`Target::Nothing`); FacingLeft toggled, the packet's X sign, SetHitbox(true).
        warn_if_body(w, id, "set");
        let p = pm(w, id);
        let s = safe_mut(p);
        s.facing_left = !s.facing_left;
        facing_packet(p);
        set_box(safe_mut(p), true);
    }
    let s = safe(pr(w, id));
    if a == s.reflect_idle_at {
        set_velocity_multicast(w, id, 0.0, 550.0, false); // @1144
    }
    let s = safe(pr(w, id));
    if a == s.death_at {
        return true; // ReleaseToPool @153: K2_DestroyActor
    }
    if s.sim {
        body_step(w, id);
    }
    false
}

fn overlap_safe(w: &mut World, id: ProjectileId, vic: FighterId) {
    hitbox_overlap(w, id, vic);
}

/// The DamageCollision box (DamageDestructible, HitBox Overlap) at the actor, square about Z.
fn damage_box_safe(w: &World, id: ProjectileId) -> Option<WorldSphere> {
    let p = pr(w, id);
    let st = static_of(w, p)?;
    if p.no_collision {
        return None;
    }
    Some(WorldSphere {
        x: p.x,
        y: p.y,
        z: Some(0.0),
        r: 0.0,
        bx: Some([0.0, 0.0, st.dmg_hd, st.dmg_hx, 0.0, 0.0, 0.0, st.dmg_hy, 0.0]),
        ..WorldSphere::default()
    })
}

/// ReceiveAnyDamage @6603: HitVfx, CurrentHP -= Damage, HandleDamageTaken.
fn any_damage_safe(w: &mut World, id: ProjectileId, dmg: Real, _by: ActorRef) -> bool {
    let p = pm(w, id);
    let s = safe_mut(p);
    s.hp -= dmg;
    damage_taken(p);
    false
}

/// OnHitByAlly @9355 / OnHitByEnemy @11301 (KnockedBack -> OnHitByFighter @4188 -> Handle Hit fails for every enemy
/// at the team test: nothing).
#[allow(clippy::too_many_arguments, reason = "the hook's signature")]
fn hit_by(
    w: &mut World,
    id: ProjectileId,
    ally: bool,
    dx: Real,
    dy: Real,
    bkb: Real,
    _kind: i32,
    by: Option<ActorRef>,
) {
    if ally {
        let Some(by) = by else {
            warn_text(w, &format!("{SAFE}: HitByAlly without its Attacker (World.hitBy): no knock"));
            return;
        };
        // GetComponentByClass(Comp_Pawn_ChargeAttack).GetChargeValue(1), unguarded: a fighter's, a spawned actor's own
        // copy; none (Accessed None: 0) for another actor.
        let charge = match by {
            ActorRef::Fighter(f) => instigator_charge(w, f).map_or(1.0, |c| c.kb),
            ActorRef::Projectile(_) | ActorRef::Item(_) => charge_kb(w, by),
            _ => 0.0,
        };
        handle_hit(w, id, by, dx, dy, bkb, charge);
        let p = pm(w, id);
        let s = safe_mut(p);
        if s.hp >= 1.0 {
            s.hp -= 0.5;
            damage_taken(p);
        }
        return;
    }
    if !flag(class_p(w, pr(w, id)), "CanBeKnockedBackByEnemies") {
        return;
    }
    let Some(ActorRef::Fighter(f)) = by else { return };
    // KnockbackSafe(attacker, charge(1) x Hit.BaseKnockBack, Hit.KnockBackDirectionVector, IsFacingLeft);
    // ChangeVFXColorOnAllyHit (the view's).
    let charge = instigator_charge(w, f).map_or(1.0, |c| c.kb);
    knockback_safe(w, id, f, charge * bkb, dx, dy, facing(w, f) < 0.0);
}

/// Blocked @11285 -> @2800 (the parent's with null first: DestroyOnBlocked false, nothing).
fn blocked(w: &mut World, id: ProjectileId, _by: ActorRef) {
    let s = safe(pr(w, id));
    if !s.box_on || !s.block_open {
        return;
    }
    let (vx, vy) = velocity(pr(w, id));
    let s = safe_mut(pm(w, id));
    s.block_open = false;
    set_box(s, false);
    set_velocity_multicast(w, id, -vx, vy, false); // the velocity's Y (sim x) flipped
    let age = pr(w, id).age;
    safe_mut(pm(w, id)).block_at = to_int32(crate::real(age) + latent_frames(0.5));
}

/// @1765 (EndPlay and OnReleasedToPool both; the actor ends once).
// Deviation: RandomIntegerInRange and RandomBoolWithWeight (FMath::Rand / FRand) are taken from the world's stream
// (`World::random_int`), the pick ahead of the roll, neither inside the Lab reset's pass (as `destroy_projectile`'s
// roll); the item is spawned ahead of the death VFX and sound events.
fn destroyed(w: &mut World, id: ProjectileId) {
    if reset_pass(w) {
        return;
    }
    let p = pr(w, id);
    let items: Vec<String> = class_p(w, p)
        .get("SpawnableItems")
        .and_then(|v| v.as_arr())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    if !items.is_empty() {
        let n = crate::real(u32::try_from(items.len()).expect("a short list"));
        let k = usize::try_from(to_int32(w.random_int(n))).expect("an index");
        let loot = data_name(w, &items[k]);
        safe_mut(pm(w, id)).loot = Some(loot);
    }
    // SpawnedActorParent EndPlay @16070-16137: SpawnActorAtDeath wants NOT DestroySpawnedActorBlocked; the loot pick
    // (@1765) is ahead of that gate, so its draw stays.
    let p = pr(w, id);
    if p.destroy_spawn_blocked == Some(true) {
        return;
    }
    let props = class_p(w, p);
    let Some(loot) = safe(p).loot.filter(|_| flag(props, "SpawnActorAtDeath")) else { return };
    let chance = num(props, "SpawnAtDeathChance").unwrap_or(0.0);
    // RandomBoolWithWeight(w): FRand() <= w, FRand from the stream as RandomInteger (2^24 steps).
    if !(chance > 0.0 && w.random_int(16_777_216.0) / 16_777_216.0 <= chance) {
        return;
    }
    // CompSpawnActor(the class, the actor's transform, direction (0, 0, 0), speed 0); E1's item classes. The owner's
    // Comp_Spawner: a world-spawned safe no hit has given an owner has none (SpawnedActorParent @5646-5746), so
    // nothing.
    let p = pr(w, id);
    let (Some(owner), x, y) = (p.owner, p.x, p.y) else { return };
    let charge = instigator_charge(w, owner);
    comp_spawn_item(w, loot, owner, x, y, 0.0, 0.0, false, charge);
}

/// Reflected @11055 (no own authority gate), behind the parent's BPI_Reflected UpdateHitBoxDamage, which activates the
/// boxes tagged HitBox: the safe has none, so the parent's hit stays off (`reflect_boxes_on` undone).
fn reflected(w: &mut World, a: ActorRef) {
    let ActorRef::Projectile(id) = a else { return };
    hitboxes_off(pm(w, id));
    let age = pr(w, id).age;
    let at = to_int32(crate::real(age) + latent_frames(0.5));
    if safe(pr(w, id)).box_on {
        set_box(safe_mut(pm(w, id)), false);
        // @11094: VelocityBeforeReflect = CharacterCollision.GetComponentVelocity: 0 (`Target::Nothing`).
        warn_if_body(w, id, "get");
        let s = safe_mut(pm(w, id));
        if s.reflect_at < age {
            s.reflect_at = at;
        }
    } else {
        let s = safe_mut(pm(w, id));
        if s.reflect_idle_at < age {
            s.reflect_idle_at = at;
        }
    }
}

// ---------------------------------------------------------------- the carry

fn carry_id(a: ActorRef) -> ProjectileId {
    match a {
        ActorRef::Projectile(id) => id,
        _ => unreachable!("a BugsBunnySafe carry hook is called with its own actor"),
    }
}

/// Buff_ProjectileThroughTunnel's calls on the safe: the shared Projectile hooks but for the root that simulates.
/// IsSimulatingPhysics || ProjectileMovement.IsActive.
fn carry_moving(w: &World, a: ActorRef) -> bool {
    let s = safe(pr(w, carry_id(a)));
    s.sim || s.pm_active
}

fn carry_at(w: &World, a: ActorRef) -> V2 {
    let p = pr(w, carry_id(a));
    V2 { x: p.x, y: p.y }
}

/// K2_SetActorLocation(teleport): the body moves with it, its velocity kept.
fn carry_place(w: &mut World, a: ActorRef, x: Real, y: Real) {
    let p = pm(w, carry_id(a));
    p.x = x;
    p.y = y;
}

/// SelectVector(ProjectileMovement.Velocity, GetVelocity(), Velocity != 0).
fn carry_velocity(w: &World, a: ActorRef) -> V2 {
    let p = pr(w, carry_id(a));
    let (x, y) = if p.vx != 0.0 || p.vy != 0.0 { (p.vx, p.vy) } else { velocity(p) };
    V2 { x, y }
}

/// MulticastProjectileVelocity: an active movement's Velocity, else the root's SetPhysicsLinearVelocity.
fn carry_set_velocity(w: &mut World, a: ActorRef, vx: Real, vy: Real) {
    let p = pm(w, carry_id(a));
    let s = safe_mut(p);
    if s.pm_active {
        p.vx = vx;
        p.vy = vy;
    } else if s.sim {
        s.bvx = vx;
        s.bvy = vy;
    }
}

/// SetActorEnableCollision: no Hitbox, no DamageCollision, no body contact while off (`no_collision`).
fn carry_collision(w: &mut World, a: ActorRef, _buff: u32, on: bool) {
    pm(w, carry_id(a)).no_collision = !on;
}

fn carry_pause_life(w: &mut World, a: ActorRef, buff: u32, paused: bool) {
    let id = carry_id(a);
    if paused {
        let left = pause_max_lifetime(pm(w, id));
        if let Some(x) = elem_buff_mut(w, a, buff).and_then(|b| b.ext.carry_mut()) {
            x.life = Some(crate::real(left));
        }
        return;
    }
    let left = elem_buff_mut(w, a, buff).and_then(|b| b.ext.carry_mut()).and_then(|x| x.life).unwrap_or(0.0);
    unpause_max_lifetime(pm(w, id), to_int32(left));
}

// ---------------------------------------------------------------- BugsBunny_SafeDrop_C

/// `SafeDrop extends Projectile`: what the class adds. Placed on the ground under the hand at BeginPlay, it strikes
/// 1.5 s on (ActivateObject) and spawns the safe high above, then ends (Delay(LandDestroyDelay 0)). `Activated` is
/// the projectile's own.
#[derive(State, Debug)]
pub struct SafeDrop {
    /// HitBoxOn? with UpdateHitbox: the HitBox's collision.
    pub strike: bool,
    /// DestinationDistance (the strike's length).
    pub dest: Real,
    /// The age the Delay(LandDestroyDelay) @2934 fires at.
    pub end_at: i32,
    /// The strike's hit actors (HitEachActorOnlyOnce), fighters' ids.
    pub struck: IndexSet<i32>,
}

impl Default for SafeDrop {
    /// The field initial values.
    fn default() -> Self {
        SafeDrop { strike: false, dest: 0.0, end_at: -1, struck: IndexSet::new() }
    }
}

fn construct_drop(_w: &World, p: &mut Projectile) {
    p.ext = ProjectileExt::SafeDrop(SafeDrop::default());
}

/// The fighter's socket in the current pose (Mesh.GetSocketLocation), the actor's location when it is not sampled.
fn socket_point(w: &World, f: FighterId, socket: &str) -> V2 {
    let ch = w.fighter(f).ch;
    let index = crate::chars::iron_giant::actor_state::socket_index(w, f, socket);
    let pose = index.and_then(|_| crate::fighter::anim::sampled_boxes(w, f));
    let (Some(i), Some(pose)) = (index, pose) else {
        warn_text(w, &format!("{SAFE_DROP}: socket {socket} not sampled (the actor location taken)"));
        return V2 { x: w.fighter(f).body.x, y: actor_y(w, f) };
    };
    let o = w.fixed.samplers.get(&ch).and_then(|b| b.offsets.get(usize::try_from(i).expect("a box index")).copied());
    let o = usize::try_from(o.expect("a socket offset")).expect("a box offset");
    let at = V2 {
        x: crate::fighter::mesh_wx(w, f) + crate::real(pose[o]) * facing(w, f),
        y: crate::fighter::mesh_wy(w, f) + crate::real(pose[o + 1]),
    };
    crate::fighter::anim::put_pose(w, pose);
    at
}

fn ground_channel(w: &World, p: &Projectile) -> i32 {
    if flag(class_p(w, p), "HitPlatforms") { TRACE_PROJECTILE_GROUND } else { TRACE_PROJECTILE_GROUND_NO_PLATFORM }
}

/// @3026 (authority, an owner BP_FighterCharacter): SphereTraceSingle(r 250) from the hand socket (FacingLeft ?
/// R_Hand : L_Hand) + 100 to - 5000; SetActorLocation((0, ImpactPoint.Y, ImpactPoint.Z), sweep, teleport) whether it
/// hit or not (no hit: ImpactPoint (0, 0, 0)), attached to the hit component (the stage's: none moves), then
/// Delay(TimeTillActivation) @4339 -> ActivateObject.
fn begin_drop(w: &mut World, id: ProjectileId) {
    let f = pr(w, id).owner.expect("a safe drop has an owner");
    let at = socket_point(w, f, if facing(w, f) < 0.0 { "R_Hand" } else { "L_Hand" });
    let ch = ground_channel(w, pr(w, id));
    let h =
        sphere_trace_single(w, at.x, at.y + 100.0, at.x, at.y - 5000.0, 250.0, ch, &[], Some(ActorRef::Projectile(id)));
    let p = pm(w, id);
    let (x, y) = h.map_or((0.0, 0.0), |h| (h.ix, h.iy));
    p.x = x;
    p.prev_x = x;
    p.y = y;
    p.prev_y = y;
    p.stopped = true; // attached, the movement's gravity 0 and no speed
}

/// ActivateObject @45 (authority, once).
fn activate_drop(w: &mut World, id: ProjectileId) {
    let a = ActorRef::Projectile(id);
    if pr(w, id).activated {
        return;
    }
    pm(w, id).activated = true;
    let p = pr(w, id);
    let f = p.owner.expect("a safe drop has an owner");
    let (x, y, age, fid) = (p.x, p.y, p.age, w.fighter(f).id);
    // AudioMulticast @4414
    w.events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_bugs_painttarget_appear"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    // HitBoxOn? = true, UpdateHitbox: the strike, and the variable a viability read takes (BPI_IsViableForProjectileTweaks:
    // a Marvin bubble or a reflect meets the marker from here on).
    // The strike sets hitVar (the build's HitBoxOn? = true @501).
    drop_mut(pm(w, id)).strike = true;
    pm(w, id).hit_var = true;
    // GetChargeValue(PercentCharged 4): Lerp(Min, Max, charge) = 2500 with Min = Max.
    let pct = min(max(instigator_charge(w, f).map_or(0.0, |c| c.pct), 0.0), 1.0);
    let p = pr(w, id);
    let (min_l, max_l) = (cfg(w, p, "MinLengthOfStrike"), cfg(w, p, "MaxLengthOfStrike"));
    let len = min_l + (max_l - min_l) * pct;
    let ch = ground_channel(w, p);
    // LineTraceSingle down `len` @623 (a start on the surface is the shared trace's).
    let h = sphere_trace_single(w, x, y, x, y - len, 0.0, ch, &[], Some(a));
    let p = pm(w, id);
    drop_mut(p).dest = h.map_or(len, |h| h.distance); // SetDestinationMultcast: the distance, the movement's Velocity 0 and Deactivate
    p.vx = 0.0;
    p.vy = 0.0;
    p.stopped = true;
    // CalculatedDuration / the Timeline's rate: ImmediateLightningLength -> HitBoxUpdateMulticast(1) @1767 (the box
    // from the actor down `dest`), no Timeline. Not ported: the other branch (Timeline.PlayFromStart, its update and
    // finish); the class data takes the immediate one.
    let p = pr(w, id);
    if !flag(class_p(w, p), "ImmediateLightningLength") {
        warn_text(w, &format!("{SAFE_DROP}: the Timeline strike is not ported (immediate taken)"));
    }
    if flag(class_p(w, p), "SpawnProjectile") {
        // BoxTraceSingle(Z + 1300 -> Z + 5000, half (200, 200, 10), rotation 0, channel 5, ignore self) @2172: UE (X,
        // Y, Z) half extents are the depth, hx and hy; the safe at its Location (the box's centre at the hit), else
        // at the upper end.
        let up = box_trace_single(
            w,
            x,
            y + 1300.0,
            x,
            y + 5000.0,
            200.0,
            10.0,
            200.0,
            TRACE_PROJECTILE_GROUND_NO_PLATFORM,
            &[],
            Some(a),
        );
        spawn_safe(w, f, x, up.map_or(y + 5000.0, |u| u.y));
    }
    let p = pr(w, id);
    let delay = num(class_p(w, p), "LandDestroyDelay").unwrap_or(0.0); // @2934 (the class default 0, not in the exported CDO)
    let end = to_int32(crate::real(age) + latent_frames(delay));
    drop_mut(pm(w, id)).end_at = end;
}

/// The owner's Comp_Spawner.CompSpawnActor(BugsBunnySafe, world transform at (x, y), rotation 0, direction (0, 0, -1),
/// InitialSpeed 5000, MaxSpeed 5000) @2786: the ability trigger, the class's make, then the push-out of the Capsule,
/// numbered, its BeginPlay, the test at the place (`comp_spawn_projectile`).
fn spawn_safe(w: &mut World, f: FighterId, x: Real, y: Real) {
    comp_spawn_projectile(
        w,
        f,
        name!("BugsBunnySafe_C"),
        x,
        y,
        [Some(0.0), Some(0.0), Some(-1.0)],
        5000.0,
        5000.0,
        None,
    );
}

fn step_drop(w: &mut World, id: ProjectileId) -> bool {
    let p = pr(w, id);
    let age = p.age;
    let at = latent_frames(cfg(w, p, "TimeTillActivation"));
    if crate::real(age) == at {
        activate_drop(w, id);
    } else if age == drop_of(pr(w, id)).end_at {
        return true;
    }
    false
}

/// The DamageCollision sphere as the damage shape.
fn damage_box_drop(w: &World, id: ProjectileId) -> Option<WorldSphere> {
    let p = pr(w, id);
    if p.no_collision {
        return None;
    }
    let dc = class_def(w, p).and_then(|d| obj(&d.components, "DamageCollision"));
    Some(WorldSphere {
        x: p.x,
        y: p.y,
        z: Some(0.0),
        r: num_of(w, p.cls, dc, "SphereRadius"),
        ..WorldSphere::default()
    })
}

/// The strike: the HitBox (half (100, 100), Z = dest / 2) at (0, 0, -dest / 2) under the actor, while HitBoxOn?.
fn overlap_drop(w: &mut World, id: ProjectileId, vic: FighterId) {
    let a = ActorRef::Projectile(id);
    let p = pr(w, id);
    let vid = w.fighter(vic).id;
    let d = drop_of(p);
    if !d.strike || p.dead || w.fighter(vic).life.dead > 0.0 || d.struck.contains(&vid) {
        return;
    }
    let he = class_def(w, p).and_then(|c| obj(&c.components, "HitBox")).and_then(|h| obj(h, "BoxExtent"));
    let hx = num_of(w, p.cls, he, "Y");
    let hy = d.dest / 2.0;
    if !capsule_in_box(w, vic, p.x, p.y - hy, hx, hy) {
        return;
    }
    let inst = p.owner.expect("a safe drop has an owner");
    let (at, attack_id, bkb) = (V2 { x: p.x, y: p.y }, p.attack_id, p.hit.base_knock_back.unwrap_or(0.0));
    if w.fighter(vic).team.team != w.fighter(inst).team.team {
        let (dl, packet) = dealt(w, id);
        if crate::combat::phases::no_hit_by(w, inst, vic) || projectile_blocks(w, &dl, vic) {
            return;
        }
        drop_mut(pm(w, id)).struck.insert(vid);
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, a, vic, attack_id, bkb, tick, None, false);
            return;
        }
        dodge_over(w, a, vic);
        let pct = charge_pct(w, a);
        if apply_projectile_hit(w, &dl, vic, &packet, &[], at, None, &[], pct) {
            actor_hit_enemy(w, &dl, vic);
        }
    } else if vic != inst {
        drop_mut(pm(w, id)).struck.insert(vid);
        let (dl, packet) = dealt(w, id);
        ally_hit(w, &dl, vic, &packet);
    }
}

/// The target's DamageCollision (a sphere, DamageDestructible, QueryAndPhysics) as a body of the traces.
fn bodies_drop(w: &World, a: ActorRef, out: &mut Vec<TraceBody>) {
    let ActorRef::Projectile(id) = a else { return };
    let p = pr(w, id);
    let c = class_def(w, p).and_then(|d| obj(&d.components, "DamageCollision"));
    let Some(info) = body_info(c, None).filter(|i| i.on) else { return };
    if p.no_collision {
        return;
    }
    out.push(TraceBody {
        ax: p.x,
        ay: p.y,
        bx: p.x,
        by: p.y,
        hx: 0.0,
        hy: 0.0,
        r: num_of(w, p.cls, c, "SphereRadius"),
        z: 0.0,
        object_type: info.kind,
        profile: info.profile,
        component: name!("DamageCollision"),
        actor: Some(a),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

/// The DamageCollision as a body of the traces (object type DamageDestructible): the box in the play plane.
fn bodies_safe(w: &World, a: ActorRef, out: &mut Vec<TraceBody>) {
    let ActorRef::Projectile(id) = a else { return };
    let p = pr(w, id);
    let comp = class_def(w, p).and_then(|d| obj(&d.components, "DamageCollision"));
    let info = body_info(comp, None).filter(|i| i.on);
    let (Some(info), Some(st)) = (info, static_of(w, p)) else { return };
    if p.no_collision {
        return;
    }
    out.push(TraceBody {
        ax: p.x,
        ay: p.y,
        bx: p.x,
        by: p.y,
        hx: st.dmg_hx,
        hy: st.dmg_hy,
        r: 0.0,
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: name!("DamageCollision"),
        actor: Some(a),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

/// This file's registry entries (BugsBunnySafe_C, BugsBunny_SafeDrop_C). The safe's `foreign_owner` (Taz's burp: a safe
/// he ate is spawned again with him as its owner; BugsBunnyNeutralSafe, a data-only child, takes this entry): the
/// class's code reads of the owner its clock, team, id, spawned-actor list and actor data (`actor_def` falls back to
/// the match's other fighters), and spawns its loot through the shared item classes: nothing of a Bugs pawn.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let safe_hooks = ProjectileHooks {
        construct: Some(construct_safe),
        bodies: Some(bodies_safe),
        begin: Some(begin_safe),
        receive_hit: Some(ProjectileReceiveHit { boxes: no_boxes, hit: receive_hit }),
        damage_box: Some(damage_box_safe),
        reflect: ReflectHooks { reflected: Some(reflected), ..ReflectHooks::default() },
        step: Some(step_safe),
        overlap: Some(overlap_safe),
        hit_by: Some(hit_by),
        blocked: Some(blocked),
        any_damage: Some(any_damage_safe),
        destroyed: Some(destroyed),
        // The class spawns its loot itself (`destroyed`): ActorClassSpawnedAtDeath is not in its data.
        spawns_at_death: Some(|_, _| false),
        ..ProjectileHooks::default()
    };
    let carry = CarryHooks {
        moving: carry_moving,
        team: owner_team,
        at: carry_at,
        place: carry_place,
        velocity: carry_velocity,
        set_velocity: carry_set_velocity,
        collision: carry_collision,
        pause_life: carry_pause_life,
        tick: None,
    };
    let safe_entry = ActorEntry {
        carry: Some(carry),
        foreign_owner: true,
        ownerless: true,
        ..ActorEntry::new(ActorClass::Projectile(ProjectileClass::BugsBunnySafe), ActorHooks::Projectile(safe_hooks))
    };
    r.actors.enter(SAFE, safe_entry)?;
    // BugsBunnyNeutralSafe_C (a data-only child: MaxHP 32, CanBeKnockedBackByEnemies true, TriggerAbilityOnSpawnIndex
    // false; the data-only items) takes its parent's entry, its class the child's.
    r.actors.enter(
        SAFE_NEUTRAL,
        ActorEntry { class: ActorClass::Projectile(ProjectileClass::BugsBunnyNeutralSafe), ..safe_entry },
    )?;
    let drop_hooks = ProjectileHooks {
        construct: Some(construct_drop),
        bodies: Some(bodies_drop),
        begin: Some(begin_drop),
        damage_box: Some(damage_box_drop),
        step: Some(step_drop),
        overlap: Some(overlap_drop),
        activate: Some(activate_drop),
        // ReceiveAnyDamage @5493: K2_DestroyActor (an enemy breaks the target ahead of its strike).
        any_damage: Some(|_, _, _, _| true),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        SAFE_DROP,
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::BugsBunnySafeDrop), ActorHooks::Projectile(drop_hooks)),
    )
}
