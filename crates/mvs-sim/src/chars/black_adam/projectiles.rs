//! Black Adam's homing ball: C021_Homing_Projectile_C ([`HomingBall`], a
//! projectile's `ext`: its homing toward the owner, its pickup area).
//!
//! `HomingBall`'s override `writeHoming` is called from the shared code ([`write_homing`]).
//!
//! The ParentProjectilePhysics side-special orb (C021_Homing_Projectile). What the parent and the data give with no
//! code here: InitialSpeed / MaxSpeed 0 (the spawner notify's, written over the template's 2400 by
//! BPI_SetInitialMovement: no launch, no clamp on the homing return), gravity 0, the ability slot 3
//! (TriggerAbilityOnSpawnIndex), MaxLifetime 1.6, the HitBox off at the spawn (HitBoxOn? false), the DestroyOnHit*
//! deaths, DestroyOnExitedArena?.
//! ReceiveBeginPlay @395: the parent's; PS_C021_Lightning_Intro at the Capsule @455 (scale 1, team colour
//! @628-792); with authority Delay(DelayTillHoming) @885 -> resume @30; the pop to @944:
//! sfx_cha_C021_side_special_ball_spawn at the ParticleSystem @994 (in BeginPlay, the Delay's latent entry made).
//! @30 CheckForHomingTarget (@1835-1793): a valid owner that is a BP_FighterCharacter ->
//! ProjectileMovement.HomingTargetComponent = GetOwner().K2_GetRootComponent() (his root capsule). The graph writes
//! the target alone: bIsHomingProjectile (true) and HomingAccelerationMagnitude (12000) are the template's. So the
//! homing is the shared ProjectileMovement homing (`combat::elem::set_homing`: buff_home, the target's live place,
//! `actors::homing`), not a homing of the class's own fields; the class's flag is kept here (`homing_on`) because
//! MulticastReflect @23793-23862 writes it false through clearHoming -> writeHoming(false), and with the flag off the
//! target write moves nothing (ComputeAcceleration needs both).
//! @44 push 104, Delay(0.0) @49 -> resume @233 (latent_links); the pop to @104: sfx_cha_C021_side_special_ball_move
//! at ParticleSystem.K2_GetComponentScale() @104-154 (the build passes the component's world scale as the location).
//! @233 (the Delay(0) made inside the resumed link: the actor's tick of the next frame):
//! PickupArea.SetGenerateOverlapEvents(true), HitBoxOn? = true, UpdateHitbox @15.
//! PickupArea BeginOverlap @282: authority, OtherActor == GetOwner() -> K2_DestroyActor @380. PickupArea is a sphere
//! of 50 on the Capsule (DetectPawn: object type WorldDynamic, Overlap on Pawn alone), so it meets his root capsule
//! (the pawn's CharacterMesh has no physics body). SetGenerateOverlapEvents updates no overlap itself: the first
//! test is at the next overlap update, the next frame's moves.
//! HitEnemyPlayer @1073: the parent's, PF_Post_VO_Event(Instigator, ability_generic, the victim, 0.1) @1101, then
//! NoAlly OR AllyHit (AllyHit true) -> K2_DestroyActor @1228, which the parent's DestroyOnHitFighter does in the same
//! pending call.
//! Not ported: the OnKnockedBack helper @1909 (destroy when HomingTargetComponent is invalid): no delegate or call
//! reaches it. SetTeamColor @1243-1576 and the team parameters: the drawing's.
// Deviation: the pickup is tested in the actor's step behind both moves of the frame (the fighters tick ahead of the
// projectiles in the world's step), from the frame after the enable on: an owner who moves into the sphere and out of
// it inside one frame is not seen, and the order of the BeginOverlap against the other events of that frame is the
// port's.

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_def, class_p, set_hitbox};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::elem::set_homing;
use crate::events::Event;
use crate::fighter::common::latent_frames;
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::props::{num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

const PICKUP_RADIUS: Real = 50.0;

/// `HomingBall extends Projectile`: what the class adds.
#[derive(State, Debug)]
pub struct HomingBall {
    /// ProjectileMovement.bIsHomingProjectile (the template's true).
    pub homing_on: bool,
    /// HomingAccelerationMagnitude.
    pub homing_mag: Real,
    /// The age Delay(DelayTillHoming) resumes at.
    pub home_at: i32,
    /// The age Delay(0.0) resumes at (@233), -1 none.
    pub enable_at: i32,
    /// PickupArea's bGenerateOverlapEvents.
    pub pickup_on: bool,
    /// The first age its overlap is tested at.
    pub pickup_from: i32,
}

impl Default for HomingBall {
    /// The field initial values.
    fn default() -> Self {
        HomingBall { homing_on: true, homing_mag: 0.0, home_at: 0, enable_at: -1, pickup_on: false, pickup_from: -1 }
    }
}

/// `HomingBall.writeHoming`, after the parent's: `homingOn` follows the write.
pub fn write_homing(p: &mut Projectile, on: bool) {
    if let ProjectileExt::HomingBall(b) = &mut p.ext {
        b.homing_on = on;
    }
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn ball(w: &World, id: ProjectileId) -> &HomingBall {
    match &pr(w, id).ext {
        ProjectileExt::HomingBall(b) => b,
        e => panic!("C021_Homing_Projectile_C without its state: {e:?}"),
    }
}

fn ball_mut(w: &mut World, id: ProjectileId) -> &mut HomingBall {
    match &mut pm(w, id).ext {
        ProjectileExt::HomingBall(b) => b,
        e => panic!("C021_Homing_Projectile_C without its state: {e:?}"),
    }
}

fn owner(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("a homing ball has an owner")
}

/// The constructor's tail: HomingAccelerationMagnitude of the template and `DelayTillHoming` as ticks.
fn construct(w: &World, p: &mut Projectile) {
    let mag = class_def(w, p)
        .and_then(|d| obj(&d.components, "ProjectileMovement"))
        .and_then(|c| num(c, "HomingAccelerationMagnitude"))
        .unwrap_or(0.0);
    let delay = num(class_p(w, p), "DelayTillHoming").unwrap_or(0.0);
    p.ext = ProjectileExt::HomingBall(HomingBall {
        homing_mag: mag,
        home_at: to_int32(latent_frames(delay)),
        ..HomingBall::default()
    });
}

fn emitter(w: &mut World, id: ProjectileId, vfx: Name) {
    let p = pr(w, id);
    let e = Event::ProjectileDeath {
        cls: p.cls,
        fighter: p.owner_id,
        vfx: Some(vfx),
        x: p.x,
        y: p.y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    };
    w.events.push(e);
}

fn sound(w: &mut World, id: ProjectileId, ev: Name, at: (Real, Real)) {
    let fighter = pr(w, id).owner_id;
    w.events.push(Event::Sound { fighter, ev, x: Some(at.0), y: Some(at.1), hit: None, pooled: false, key: None });
}

fn begin(w: &mut World, id: ProjectileId) {
    emitter(w, id, name!("PS_C021_Lightning_Intro"));
    // The ParticleSystem: StaticMesh (Z -67, scale 3) then (45, 0, 23): the depth and 2 units over the root.
    let at = (pr(w, id).x, pr(w, id).y);
    sound(w, id, name!("sfx_cha_C021_side_special_ball_spawn"), at);
}

/// The owner's root capsule against PickupArea (a sphere on the Capsule, scaled with it).
fn over_owner(w: &World, id: ProjectileId) -> bool {
    let p = pr(w, id);
    let f = owner(w, id);
    let r = PICKUP_RADIUS * p.scale;
    let seg = capsule_half(w, f) - cap_radius(w, f);
    let ay = actor_y(w, f);
    let dy = p.y - max(ay - seg, min(p.y, ay + seg));
    let rr = r + cap_radius(w, f);
    let dx = p.x - w.fighter(f).body.x;
    dx * dx + dy * dy <= rr * rr
}

/// `step`: true destroys the ball (the owner picks it up).
fn step(w: &mut World, id: ProjectileId) -> bool {
    let age = pr(w, id).age;
    if age == ball(w, id).home_at {
        // CheckForHomingTarget: GetOwner() (the reflector after a reflect) is always a fighter here. A buff's homing
        // (Buff_C021_SetHomingTowardsTarget) keeps its magnitude; the target becomes the owner.
        let o = owner(w, id);
        let me = ActorRef::Projectile(id);
        if let Some(h) = &pr(w, id).buff_home {
            let mag = h.mag;
            set_homing(w, me, ActorRef::Fighter(o), mag);
        } else if ball(w, id).homing_on {
            let mag = ball(w, id).homing_mag;
            set_homing(w, me, ActorRef::Fighter(o), mag);
        }
        // K2_GetComponentScale of ParticleSystem: Capsule (absolute) x StaticMesh 3 x (0.25, 0.4, 0.4); UE (X, Y, Z)
        // -> sim (-Y, Z).
        let s = pr(w, id).scale * 3.0 * 0.4;
        sound(w, id, name!("sfx_cha_C021_side_special_ball_move"), (-s, s));
        ball_mut(w, id).enable_at = age + to_int32(latent_frames(0.0));
    }
    if age == ball(w, id).enable_at {
        let b = ball_mut(w, id);
        b.enable_at = -1;
        b.pickup_on = true;
        b.pickup_from = age + 1;
        set_hitbox(pm(w, id), true);
    }
    let b = ball(w, id);
    b.pickup_on && age >= b.pickup_from && over_owner(w, id)
}

/// HitEnemyPlayer @1073: PF_Post_VO_Event(Instigator, ability_generic, the victim, 0.1).
fn hit_enemy(w: &mut World, id: ProjectileId, vic: FighterId) {
    let o = owner(w, id);
    crate::fighter::match_side::post_vo(w, o, name!("ability_generic"), Some(vic), 0.1);
}

/// This file's registry entries (the homing ball's class).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let kind = ProjectileHooks {
        construct: Some(construct),
        begin: Some(begin),
        step: Some(step),
        hit_enemy: Some(hit_enemy),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        "C021_Homing_Projectile_C",
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::C021HomingProjectile), ActorHooks::Projectile(kind)),
    )
}
