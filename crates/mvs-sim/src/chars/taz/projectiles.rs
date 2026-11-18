//! Taz's (C015) projectiles: C015StewSpit (the grounded neutral's stew,
//! spawned by its ProjectileAimer), C015StewSpitAir (the aerial neutral's, attached to him, [`StewSpitAir`]) and
//! C015Trash (the trash specials' apple). All three are ParentProjectilePhysics children: the movement, lifetime, hit
//! box, OnHitBuffs rows (Buff_ApplyTasty, Buff_Tasty: tasty.rs's entries, looked up by name), clank and deaths are the
//! parent's and the class data's (`actors::projectile`). Per-actor state lives on the actor (the snapshot walks it);
//! nothing here is a field of the fighter.
//!
//! C015Boot is not here: no asset of the build names it (Taz's data has no entry), so nothing spawns it.
//!
//! The grounded neutral's aim (C015_Neutral_Normal_Ground_Montage): an AnimNotify_Spawner with AimOn, AimType 0
//! (StartAimDirection (0, 1, 0), angles -60..60, AimSpeed 100, UseRightStickToAim) and AnimNotify_ActivateSpawned
//! (ProjectileAimer_C, ActivateAllObjects) are the shared ProjectileAimer's (`registry::spawner_notifies`,
//! `fighter::match_side` activate_aimer): the stew is spawned at the activation along the arrow, nothing of it here.

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_def, max_lifetime_at, yaw_x};
use crate::actors::{ActorClass, Projectile, ProjectileClass, ProjectileId};
use crate::combat::boxes::WorldSphere;
use crate::fighter::common::{latent_frames, warn_text};
use crate::math::max;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::props::num;
use crate::registry::{Registry, RegistryError};
use crate::world::World;

const STEW: &str = "C015StewSpit_C";
const STEW_AIR: &str = "C015StewSpitAir_C";
const TRASH: &str = "C015Trash_C";

// ---------------------------------------------------------------- C015StewSpit

// HitEnemyPlayer @805: the parent's (the hit, its OnHitBuffs row Buff_ApplyTasty 1 stack), then @10-146 the
// Instigator's BPI_C015.BPI_HitWithFoodAttack, whose Pawn_C015 body (@8357) calls OnHitEnemyPlayer(null, null) with
// authority, and that event's link @17207 is a bare pop_flow: nothing. So the class adds no sim behaviour to its
// parent. SetTeamColor @151-761 (the Team parameter, the particle Scale = FClamp(charge value 4, 0.5, 3)) is the
// drawing's. A wearer of a Disguise who is no Taz (foreign_owner) fails the BPI_C015 cast @66: the same nothing.

// ---------------------------------------------------------------- C015StewSpitAir

/// `StewSpitAir extends Projectile` (C015StewSpitAir_C): the child (4 functions: ReceiveBeginPlay, ReceiveTick,
/// SetTeamColor and the ubergraph). Spawned by the aerial neutral's AnimNotify_Spawner with AttachToActor on Root at
/// (200, 0, 200), InitialMovementDirection (0, -1, 0) (behind him), speed 2250: an attached actor (`Projectile.attached`),
/// whose ProjectileMovement was deactivated by the parent's BeginPlay and keeps the spawner's velocity (the base's
/// `attached_vel`), which the parent's tick looks along (AimDownProjectileMovement true: `aim_down_tick`), so its yaw,
/// the ComingFromLeft of its hits, is the velocity's side.
/// ReceiveBeginPlay @212: the parent's; GetOwner() as BPI_Base_Fighter -> FacingLeft = BPI_IsFacingLeft (false without
/// one) @221-367, which nothing reads (xref reads: no reader); Delay(ReverseDirectionTIme 0.175) @386 -> @15:
/// ProjectileMovement.Velocity *= (1, -1, 1) @15-87 (UE Y is the play plane's x: the velocity's side turns over), so
/// from the next tick's look-at on its hits come from the other side, the spawner's forward.
/// ReceiveTick @192: the parent's alone. SetTeamColor @461: C015StewSpit's, then the particle Scale 1: the drawing's.
/// RotateSpeed (360) is read nowhere.
/// Its hit box (class data): the HitBox sphere has no tag and no collision; the dealers are the components tagged
/// HitBox2D (Taz's Use 2D Hitboxes; Supports2DHitboxes true): HitBox2D (radius 120, the depth-long capsule: a circle in
/// the play plane, the projectile's own shape) and CapsuleHitBox (tags HitBox and HitBox2D: half height 150, radius 60,
/// pitch 90 under the root Capsule: along the root's X, the yaw's side). Without the 2D boxes (`sphere_3d`) the dealer is
/// CapsuleHitBox alone (tag HitBox).
// Deviation: CapsuleHitBox against a fighter's hurt shapes is met as nine spheres of its radius along its core segment
// (as Bugs's RocketUp): the hurt test has no capsule dealer shape.
// Deviation: against another actor's sphere or box (hitNear: a clank, a hurt box) the HitBox2D circle alone is tested
// in 2D: the capsule's reach past it (120 to 150 along the yaw, 60 thick) is not.
#[derive(State, Debug, Default)]
pub struct StewSpitAir {
    /// The age Delay(ReverseDirectionTIme) resumes at.
    pub reverse_at: Real,
    pub dealer_half: Real,
    pub dealer_r: Real,
    /// ProjectileMovement.Velocity of the deactivated movement (the spawner's write; @87 writes it).
    pub stew_vx: Real,
    pub stew_vy: Real,
}

/// The constructor's tail (the subclass constructor body).
fn construct(w: &World, p: &mut Projectile) {
    let d = class_def(w, p);
    let reverse_at = latent_frames(d.and_then(|d| num(&d.p, "ReverseDirectionTIme")).unwrap_or(0.0));
    let c = d.and_then(|d| d.components.get("CapsuleHitBox")).and_then(|c| c.as_obj());
    let pitch =
        c.and_then(|c| c.get("RelativeRotation")).and_then(|r| r.as_obj()).and_then(|r| num(r, "Pitch")).unwrap_or(0.0);
    if c.is_none() || (pitch - 90.0).abs() > 0.01 {
        warn_text(w, "C015StewSpitAir_C: CapsuleHitBox is not the data this kind was written from");
    }
    let half = c.and_then(|c| num(c, "CapsuleHalfHeight")).unwrap_or(0.0);
    let radius = c.and_then(|c| num(c, "CapsuleRadius")).unwrap_or(0.0);
    let mut s =
        StewSpitAir { reverse_at, dealer_half: max(half - radius, 0.0), dealer_r: radius, ..StewSpitAir::default() };
    match p.attached_vel {
        Some((vx, vy)) => (s.stew_vx, s.stew_vy) = (vx, vy),
        None if !p.attached => warn_text(w, "C015StewSpitAir_C: spawned unattached (its notify attaches it)"),
        None => {}
    }
    p.ext = ProjectileExt::StewSpitAir(s);
}

/// `attachedVelocity` (the class's override): the velocity the reversal turns over.
pub fn attached_velocity(p: &Projectile) -> Option<(Real, Real)> {
    match &p.ext {
        ProjectileExt::StewSpitAir(s) => Some((s.stew_vx, s.stew_vy)),
        _ => p.attached_vel,
    }
}

/// `hitShapes` (the class's override): HitBox2D's circle (2D) and CapsuleHitBox's spheres along the yaw (pitch: the
/// look-at's, 0 for a level velocity).
pub fn hit_shapes(w: &World, p: &Projectile, out: &mut Vec<WorldSphere>) {
    let ProjectileExt::StewSpitAir(s) = &p.ext else { return };
    if !p.sphere_3d {
        let (x, y) = crate::actors::projectile::hit_xy(w, p);
        out.push(WorldSphere { x, y, z: None, r: p.hit_radius, ..WorldSphere::default() });
    }
    let (k, half) = (p.scale, s.dealer_half * p.scale);
    let [cx, cy] = crate::math::ue::turn_by_pitch(p.pitch, [1.0, 0.0]);
    let cx = cx * yaw_x(p);
    for i in 0..9 {
        let u = (crate::real(i) / 4.0 - 1.0) * half;
        out.push(WorldSphere {
            x: p.x + cx * u,
            y: p.y + cy * u,
            z: Some(0.0),
            r: s.dealer_r * k,
            ..WorldSphere::default()
        });
    }
}

/// `step`: SpawnedActorParent's MaxLifetimeTimer (0.3) of the attached actor. Deviation: the port's attached move has no
/// expiry (`Projectile.step`), so the kind ends it here as K2_DestroyActor, at the age the timer would (as
/// C021_ElectricChain_Starter's), behind the frame's move; the 0.1 s blast-box check of an attached actor is not run
/// (none of its places leaves the jump-out area while Taz is in it).
fn step(w: &mut World, id: ProjectileId) -> bool {
    let p = w.actors.projectiles.get_mut(id).expect("a live projectile");
    let life = max_lifetime_at(p);
    if p.attached && life > 0 && p.age >= life {
        return true;
    }
    let ProjectileExt::StewSpitAir(s) = &mut p.ext else { return false };
    if crate::real(p.age) == s.reverse_at {
        // @15-87: the multiply lands on the deactivated component's Velocity (attached) or the live one.
        if p.attached {
            s.stew_vx = -s.stew_vx;
        } else {
            p.vx = -p.vx;
        }
    }
    false
}

// ---------------------------------------------------------------- C015Trash

// The trash specials' apple (C015_Special_Trash_Ground / _Air, ConditionalSpawner), also the class
// BPI_GetStoredProjectile substitutes for None (projectile_eat.rs). Its graph is SetTeamColor alone (@494, @10-450: the
// apple material by team: the drawing's). The rest is class data the parent reads: one allowed (NumberOfActorsAllowed),
// TakesDamage, ability slot 3 at the spawn (TriggerAbilityOnSpawnIndex), DestroyOnCollideWithEnvironment, gravity 3,
// the hit (VelocityInsteadOfFacing, DestroyProjectiles, Buff_Tasty 1 stack). ProjectileMovement's bConstrainToPlane
// with PlaneConstraintNormal (1, 0, 0): the depth axis, which the port's projectiles do not have (they move in the play
// plane): nothing to do. Its Comp_Pawn_PassiveBuffManager row (fireproof) has every recipient flag false and 0 stacks.

/// This file's registry entries (C015StewSpit_C, C015StewSpitAir_C, C015Trash_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let plain = |class| ActorEntry {
        foreign_owner: true,
        ..ActorEntry::new(ActorClass::Projectile(class), ActorHooks::Projectile(ProjectileHooks::default()))
    };
    r.actors.enter(STEW, plain(ProjectileClass::C015StewSpit))?;
    r.actors.enter(
        STEW_AIR,
        ActorEntry {
            parents: &[STEW],
            foreign_owner: true,
            ..ActorEntry::new(
                ActorClass::Projectile(ProjectileClass::C015StewSpitAir),
                ActorHooks::Projectile(ProjectileHooks {
                    construct: Some(construct),
                    step: Some(step),
                    ..ProjectileHooks::default()
                }),
            )
        },
    )?;
    r.actors.enter(TRASH, plain(ProjectileClass::C015Trash))
}
