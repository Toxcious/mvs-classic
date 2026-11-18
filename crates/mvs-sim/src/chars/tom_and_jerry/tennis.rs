//! The tennis ball and the racket: Tom_and_Jerry_TennisBall and its
//! Reflected child ([`TennisBall`], a projectile that homes on Jerry's HomingAim and bounces back off his racket),
//! TomAndJerry_NoJerry_SlingShot_Aim ([`NoJerryAim`], the aimer of the slingshot without Jerry), Jerry's swing windows
//! (`tennis_window`, `force_reflect`).
//!
//! Jerry's side (Jerry_SpawnedActor: the TennisBall cast @6622, ReflectSwing @7057, the Reflected spawn @9383,
//! TennisLoop and the TennisRacket windows: `JerryActor::tennis_ball` / `racket`); TomAndJerry_NoJerry_SlingShot_Aim
//! (up to 8 balls, DestroySpawnedActors @5077, the spawn at tom_un_L_wrist @5763, BPI_FacingOverride, pawn.rs
//! `set_aim_direction`). The tennis normals' ActivateSpawned on Jerry is jerry.rs's `activate`.
//! Classes and their sim kind:
//! - Tom_and_Jerry_TennisBall_C, Tom_and_Jerry_TennisBall_Reflected_C: ParentProjectilePhysics children, projectile
//!   kinds; the live ones in `TomAndJerryState::tennis_balls`. Both override Reflected / MulticastReflect
//!   (`combat::reflect`): reflected only with their own ReflectHooks.
//! - TomAndJerry_NoJerry_SlingShot_Aim_C: a world actor; `TomAndJerryState::no_jerry_aimer` holds the live one.
//!
//! The ball (one graph for both classes; the child differs in data only: KnockBackGravityScale 0, no
//! TriggerAbilityOnSpawnIndex, bIsHomingProjectile false in its ProjectileMovement template, SFX_ReflectedHit null).
//! ReceiveBeginPlay @1290: InstigatingTom = the Instigator; JerrySpawned (CDO false, written nowhere) false: OnBounce
//! bound to ProjectileMovement.OnProjectileBounce @1381 (`ball_bounce_impact`), BounceOffHit to the handler's
//! HitEnemyPlayer @1450 (`ball_hit_enemy`); the loop @27-1239 over GetOwner()'s Comp_Spawner SpawnedActors keeps the last
//! actor whose class is exactly Jerry_SpawnedActor_C or its _TempInvincibility child as JerryActor; cast to Jerry
//! (both pass) @786: HomingTargetComponent = its HomingAim (JerryMesh Z -80, scale 1.2; HomingAim Z 94.12 under it:
//! 32.95 over Jerry's root), StopHoming bound to its OnEndPlay @966, HomingServer(true) @1022, and within 250 of it
//! (GetDistanceTo) Jerry.ForceReflect(self) @1132. No Jerry: nothing more (the ball flies straight).
//! HomingServer @4818: HomingMulticast (bIsHomingProjectile = it @5177), then with it, !IsReflected and the flag
//! Jerry.PlayMontageMulticast(TennisLoop) and Jerry.SetFacingAtOwner(); else Jerry.StopMontageMulticast(TennisLoop).
//! OnBounce @3403 / BounceOffHit @5779 (a valid JerryActor: nothing): BounceBackVelocity (Y 500, Z 1000), then @3495
//! BounceNumber + 1, sfx_cha_tomandjerry_paddleball_bounce_physics, past MaxBounceCount K2_DestroyActor; reflected:
//! UpdateHitboxonBounce (empty @4801); else once (the DoOnce @1955) the bounce back @3828: HitBoxOn? false +
//! UpdateHitbox, Velocity (0, self Y >= owner Y ? -Y : Y, Z), no homing, gravity scale 3, Bounciness 0.6
//! (UpdateVelocityMulticast @5606 writes the same again).
//! BounceBack(0) @6019 (Jerry's ForceReflect @6697): BounceNumber 1, ReflectedMulticast(false) (scale 1.25, particle
//! off), BounceBackVelocity_Z = owner Z - Z + 2000, the arc's Y @6392-7359 (FMax(.., 400)), then through the DoOnce's
//! close @1950 to the bounce back @3828.
//! BPI_Reflected: the parent's (`combat::reflect`), Reflected() @7840 (StopMontageMulticast: Jerry's TennisLoop),
//! MulticastReflect @7391: the parent's with ShouldPause = NewOwner != InstigatingTom, then for a Tom and Jerry
//! NewOwner of a ball not yet reflected @5939: IsReflected, KillOnHit bound to HitEnemyPlayer, ReflectedMulticast
//! (true) @2068 (BounceNumber 0, MaxBounceCount 2, scale 1.5 with a following Jerry else 1.25, gravity 0, Bounciness 1,
//! sfx tennis_return / tennis_ball_reflected, particle on), StopMontageMulticast, DamageDisabled false, no homing,
//! Bounciness 1, the packet @2608-3000 (7 damage, BKB 1800, KBS 20, (1, 0.5), OnHitAudio by the particle) and
//! UpdateHitBoxDamage with the scale 1.5 / 1 @3258.
//! ReceiveEndPlay @7635: !IsReflected with homing on: StopMontageMulticast (Jerry's TennisLoop).
// Deviation (tennis a): Jerry's ReceiveActorBeginOverlap with a ball (@15868) is tested once a frame after the ball's
// move (`ball_moved`) and at its spawn after BeginPlay, not swept: Jerry's Box (+-32 x +-70, WorldDynamic, overlaps
// PhysicsBody) against the ball's root Capsule (48 x scale, a sphere), and, while the ball's hit box collision is on,
// Jerry's ProjectileHurtBox (radius 48, half height 90, upright) against HitBox2D's circle. A begin from Jerry's own
// move is seen after the ball's next move.
// Unverified (tennis b): UpdateHitBoxDamage's scale on HitBox2D (1.5 with a following Jerry @3195) is taken as a
// factor on the hit radius; the parent's UpdateHitBoxDamage @4931 at the reflect is taken as scale 1
// (`combat::reflect`).
// Deviation (tennis c): Jerry's OnEndPlay (StopHoming) is seen at the ball's next tick after his end.
//
// TomAndJerry_NoJerry_SlingShot_Aim: spawned by AnimNotify_Spawner (socket None, DestroyAllActorsOnNotifyEnd) of
// Tom_and_Jerry_NoJerry_SlingShot(_Air)_Montage, activated by ActivateSpawned at 0.16.
// BeginPlay @1058: BaseFighterRef = the owner, Delay(0.1) -> @15 (not being destroyed and the owner's GetCurrentMontage
// neither sling shot montage: K2_DestroyActor), the Jerry loop @312 (JerryAcor).
// ReceiveTick @4520: SetLocationServer(Jerry + 80 Z, or the neck socket + 500 along the facing without a valid Jerry)
// -> ServerLocation @4469 and SetLocationMulticast @1276: with a valid Jerry, the angle of (ServerLocation - neck)
// picked by the side test @2783-3104 and NormalizeToRange'd into StartAimVector's angle +- AimAngleMin / Max (-90, 90)
// x 2 - 1 to BPI_SetAimDirection @3408 and BPI_FacingOverride(Jerry behind Tom) @4059; without one
// BPI_SetAimDirection(0) @4249 and BPI_FacingOverride(false) @4430.
// ActivateObject @6220: CheckHitstunActive -> nothing; else "Get Spawned Actor"(TennisBall_C, all) @5912: with 8 or
// more DestroySpawnedActors(TennisBall_C, last) @5077; the ball spawned at tom_un_L_wrist toward ServerLocation from
// the neck @5147-5763 (4000), then K2_DestroyActor @5062.
// BPI_FacingOverride writes Tom_and_Jerry_AnimBP's MirrorOverride alone (@16109): written here, as pawn.rs has no
// setter for it alone (pawn.rs's `set_facing_override` writes OverrideMirror? too).

use std::sync::Arc;

use mvs_data::value::{Props, Value};

use super::follower::rotate;
use super::jerry::{self, jerry_of};
use super::pawn::set_aim_direction;
use super::state::{tj_mut, tj_of};
use crate::Real;
use crate::State;
use crate::actors::projectile::{
    ProjectileExt, class_p, hit_collision, hit_xy, reflect_boxes_on, set_actor_scale, set_bounciness,
    set_gravity_scale, set_hitbox,
};
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{
    ActorClass, ActorId, ActorRef, Projectile, ProjectileClass, ProjectileId, WorldActor, WorldActorClass,
};
use crate::combat::HitPacket;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::packet::{DataEnum, DataVec, VacuumInfo};
use crate::events::{Event, NotifyRef};
use crate::fighter::common::{actor_data, actor_def, latent_frames, warn_text};
use crate::fighter::control::{actor_spawn_ability, attack_dodged, dodge_over, trigger_ability};
use crate::fighter::match_side::{Foreign, SpawnRequest};
use crate::fighter::moves::V2;
use crate::fighter::traces::{OBJ_PAWN, OBJ_PHYSICS_BODY, sphere_overlap_actors};
use crate::fighter::{
    FighterId, active_montage_name, actor_y, facing, in_hitstun, invulnerable, mesh_wx, mesh_wy, no_hit,
};
use crate::math::max;
use crate::math::ue::{SMALL_NUMBER, deg_atan2, safe_normal, size};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks, ReflectHooks, WorldActorHooks};
use crate::registry::props::{flag, num, num_in, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::is_match_fighter;
use crate::world::spawn_spot::spawn_place;
use crate::world::spawning::{add_projectile, comp_spawn_projectile, destroy_projectile, request_props};

/// The ball's class.
pub const TENNIS_BALL: &str = "Tom_and_Jerry_TennisBall_C";
/// Its reflected child.
pub const TENNIS_BALL_REFLECTED: &str = "Tom_and_Jerry_TennisBall_Reflected_C";
/// The aimer of the slingshot without Jerry.
pub const NO_JERRY_AIM: &str = "TomAndJerry_NoJerry_SlingShot_Aim_C";

const JERRY_CLS: Name = name!("Jerry_SpawnedActor_C");
const JERRY_TI_CLS: Name = name!("Jerry_SpawnedActor_TempInvincibility_C");
const TENNIS_LOOP: Name = name!("Tom_and_Jerry_Only_Jerry_TennisLoop_Montage");
const REFLECT_SWING: Name = name!("Tom_and_Jerry_Only_Jerry_ReflectSwing_Montage");
const CHAMPAGNE_B: Name = name!("Tom_and_Jerry_Champagne_B_Montage");
const CHAMPAGNE_F: Name = name!("Tom_and_Jerry_Champagne_F_Montage");
const SLING: Name = name!("Tom_and_Jerry_NoJerry_SlingShot_Montage");
const SLING_AIR: Name = name!("Tom_and_Jerry_NoJerry_SlingShot_Air_Montage");
const BOUNCE_SFX: Name = name!("sfx_cha_tomandjerry_paddleball_bounce_physics");
const RETURN_SFX: Name = name!("sfx_cha_tomandjerry_tennis_return");
const REFLECTED_SFX: Name = name!("sfx_cha_tomandjerry_tennis_ball_reflected");
const COMBO_HIT_SFX: Name = name!("sfx_cha_tomandjerry_attack_combo_03_hit");
const BALL_HIT_SFX: Name = name!("sfx_cha_tomandjerry_tennis_ball_hit");

/// `TennisBall extends Projectile`: what the class adds (the projectile's `ext`).
#[derive(State, Debug)]
pub struct TennisBall {
    /// InstigatingTom.
    pub instigating_tom: Option<FighterId>,
    /// JerryActor (the BeginPlay loop's) and "As Jerry Spawned Actor" (its cast).
    pub jerry: Option<ActorRef>,
    /// ProjectileMovement.HomingTargetComponent's actor (its HomingAim).
    pub home_target: Option<ActorRef>,
    /// ProjectileMovement.bIsHomingProjectile.
    pub home_on: bool,
    /// HomingAccelerationMagnitude.
    pub home_mag: Real,
    /// HomingAim over the target's root.
    pub aim_z: Real,
    /// StopHoming bound to JerryActor's OnEndPlay.
    pub end_bound: bool,
    pub is_reflected: bool,
    /// BounceNumber.
    pub bounce_number: i32,
    /// MaxBounceCount.
    pub max_bounce: Real,
    /// The DoOnce @1955 (closed by its run or by BounceBack's close @1950).
    pub do_once_closed: bool,
    /// KillOnHit bound to HitEnemyPlayer (@5950).
    pub kill_on_hit: bool,
    pub particle_visible: bool,
    /// HitBox2D's relative scale from UpdateHitBoxDamage (Unverified tennis b).
    pub hit_rel: Real,
    /// The Jerry actors (their ids) its components overlap (Deviation tennis a).
    pub over_jerry: Vec<i32>,
}

impl Default for TennisBall {
    /// The field initial values.
    fn default() -> Self {
        TennisBall {
            instigating_tom: None,
            jerry: None,
            home_target: None,
            home_on: false,
            home_mag: 0.0,
            aim_z: 0.0,
            end_bound: false,
            is_reflected: false,
            bounce_number: 0,
            max_bounce: 0.0,
            do_once_closed: false,
            kill_on_hit: false,
            particle_visible: false,
            hit_rel: 1.0,
            over_jerry: Vec::new(),
        }
    }
}

/// `NoJerryAim` (TomAndJerry_NoJerry_SlingShot_Aim_C): what the view reads (its place, ServerLocation, the aim it last
/// gave BPI_SetAimDirection) and its latent action.
#[derive(State, Debug, Default)]
pub struct NoJerryAim {
    /// Its place (the spawn's: socket None; not attached).
    pub x: Real,
    pub y: Real,
    /// ServerLocation (SetLocationServer @4469): the point it aims at.
    pub aim_x: Real,
    pub aim_y: Real,
    /// What it last gave BPI_SetAimDirection.
    pub aim_direction: Real,
    /// JerryAcor.
    pub jerry: Option<ActorRef>,
    /// Delay(0.1) @1193 -> @15: frames left, 0 none.
    pub wait: Real,
}

/// The TomAndJerry_NoJerry_SlingShot_Aim_C's place (its `x`, `y`).
pub fn aim_place(_w: &World, _a: &WorldActor, s: &NoJerryAim) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// The ball's override of `writeHoming` (bIsHomingProjectile), behind the parent's. Called by
/// `actors::projectile::write_homing`.
pub fn write_homing(p: &mut Projectile, on: bool) {
    if let ProjectileExt::TennisBall(b) = &mut p.ext {
        b.home_on = on;
    }
}

/// The ball's `homeAccel`: toward Jerry's HomingAim (a destroyed target: none). Called by
/// `actors::projectile::home_accel`.
pub fn home_accel(w: &World, p: &Projectile, s: &TennisBall) -> Option<V2> {
    let t = valid(w, s.home_target)?;
    if !s.home_on {
        return None;
    }
    let j = jerry_of(w, t)?;
    // ComputeHomingAcceleration: (target - location).GetSafeNormal() x HomingAccelerationMagnitude
    let [_, nx, ny] = safe_normal([0.0, j.x - p.x, j.y + s.aim_z - p.y], SMALL_NUMBER);
    Some(V2 { x: nx * s.home_mag, y: ny * s.home_mag })
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn ball(w: &World, id: ProjectileId) -> &TennisBall {
    match &pr(w, id).ext {
        ProjectileExt::TennisBall(b) => b,
        e => panic!("a Tom_and_Jerry_TennisBall_C without its state: {e:?}"),
    }
}

fn ball_mut(w: &mut World, id: ProjectileId) -> &mut TennisBall {
    match &mut pm(w, id).ext {
        ProjectileExt::TennisBall(b) => b,
        e => panic!("a Tom_and_Jerry_TennisBall_C without its state: {e:?}"),
    }
}

/// `valid(j)`: the actor is a live Jerry (a gone one is dead).
fn valid(w: &World, j: Option<ActorRef>) -> Option<ActorId> {
    let Some(ActorRef::Actor(id)) = j else { return None };
    (w.actors.actors.get(id).is_some_and(|a| !a.dead) && jerry_of(w, id).is_some()).then_some(id)
}

fn sound(w: &mut World, id: ProjectileId, ev: Name) {
    let p = pr(w, id);
    let e = Event::Sound { fighter: p.owner_id, ev, x: Some(p.x), y: Some(p.y), hit: None, pooled: false, key: None };
    w.events.push(e);
}

/// The last live Jerry of the owner's Comp_Spawner SpawnedActors, by exact class (the ball's @27-785, the aim's @312-983).
fn last_jerry(w: &World, f: FighterId) -> Option<ActorId> {
    let mut j = None;
    for a in &w.fighter(f).items.spawned_actors {
        let ActorRef::Actor(id) = *a else { continue };
        let Some(x) = w.actors.actors.get(id) else { continue };
        if !x.dead && (x.cls == JERRY_CLS || x.cls == JERRY_TI_CLS) {
            j = Some(id);
        }
    }
    j
}

/// Kismet Divide_FloatFloat (0 for a zero divisor).
fn div(a: Real, b: Real) -> Real {
    if b == 0.0 { 0.0 } else { a / b }
}

/// Kismet Sqrt (0 for a negative input).
fn sqrt0(a: Real) -> Real {
    if a >= 0.0 { a.sqrt() } else { 0.0 }
}

// ---------------------------------------------------------------- Jerry's side

/// The test of SetFacingAtOwner @29840-30307 and of the overlap @16044-16511: Jerry faces away from Tom (owner's UE Y >
/// Jerry's while FacingLeft is false, or not greater while it is true) and does not follow an ally.
fn faces_away(w: &World, j: ActorId) -> bool {
    let Some(s) = jerry_of(w, j) else { return false };
    let Some(a) = w.actors.actors.get(j) else { return false };
    let g = w.fighter(a.owner).body.x < s.x; // owner UE Y > Jerry's
    let fl = s.facing < 0.0;
    ((!g && fl) || (g && !fl)) && s.following.is_none()
}

/// SetFacingAtOwner @29840: RotateServer(!FacingLeft) @6293 when facing away.
fn set_facing_at_owner(w: &mut World, j: ActorId) {
    if faces_away(w, j) {
        let left = jerry_of(w, j).is_some_and(|s| s.facing < 0.0);
        rotate(w, j, !left);
    }
}

/// The swing's packet (`SWING`): 6 damage, BKB 1350, KBS 16, IgnoreFacing, HitReaction 2, no hit pause,
/// ForceKnockback; the AttackID and the direction go on per hit.
fn swing(attack_id: i32, kx: Real) -> HitPacket {
    HitPacket {
        sparse: true,
        damage: Some(6.0),
        base_knock_back: Some(1350.0),
        knock_back_scaling: Some(16.0),
        knock_back_direction_vector: Some(DataVec { x: Some(kx), y: Some(0.5), z: None }),
        attack_tag: Some(name!("None")),
        hit_reaction: Some(DataEnum::Value(2)),
        hit_pause_time: Some(0.0),
        hit_stun_mult: Some(1.0),
        on_hit_audio: None,
        directional_knockback: Some(false),
        velocity_instead_of_facing: Some(false),
        knock_back_mirror: Some(false),
        ignore_facing: Some(true),
        attack_id: Some(attack_id),
        do_los_check: Some(false),
        force_knockback: Some(true),
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
        hit_each_actor_only_once: Some(false),
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

/// The swing's hit (@6738-7000 -> @3864-5944 at Jerry + 25 Z; @9505-9561 -> @9624-11743 at Jerry): SphereOverlapActors
/// (100, [PhysicsBody, Pawn, WorldDynamic]), each BPI_BaseFighter of another TeamIndex than Jerry's (his Instigator's)
/// gets Jerry's handler's pfgDoDamage(null, it, the packet): 6 damage, BKB 1350, KBS 16, (victim UE Y > Jerry's ? 0.5 :
/// -0.5, 0.5) IgnoreFacing, HitReaction 2, no hit pause, ForceKnockback. AttackID 0: pfgDoDamage @5331-5581 takes the
/// handler owner's BPI_GetAttackID (Jerry's attackId, jerry.rs's actor) when it is not 0.
// Deviation (tennis e): the sphere's order is the traces'; its WorldDynamic type is left out (the traces gather no
// WorldDynamic body, and only a fighter is kept, whose root capsule is Pawn); Jerry's dealt-damage buffs are not
// applied to the swing (the dealer is a stand-in for his handler).
fn swing_hit(w: &mut World, j: ActorId, x: Real, y: Real) {
    let Some(owner) = w.actors.actors.get(j).map(|a| a.owner) else { return };
    let id = jerry_of(w, j).map_or(0, |s| s.attack_id);
    let types = [crate::real(OBJ_PHYSICS_BODY), crate::real(OBJ_PAWN)];
    let hits = sphere_overlap_actors(w, x, y, 100.0, &types, &[Some(ActorRef::Actor(j))], -1.0);
    for a in hits {
        let ActorRef::Fighter(vic) = a else { continue };
        if !is_match_fighter(w, vic) {
            continue; // DoesImplementInterface(BPI_BaseFighter)
        }
        if w.fighter(vic).team.team == w.fighter(owner).team.team {
            continue;
        }
        if w.fighter(vic).life.dead > 0.0 || no_hit(w, vic) {
            continue;
        }
        let jx = jerry_of(w, j).map_or(0.0, |s| s.x);
        let kx = if w.fighter(vic).body.x < jx { 0.5 } else { -0.5 };
        let hit = swing(id, kx);
        // The dealer is a stand-in for Jerry's handler: no velocity, charge or elemental of its own.
        let mut d = dealer(w, ActorRef::Actor(j));
        d.attack_id = id;
        d.vx = 0.0;
        d.vy = 0.0;
        d.hit_velocity = None;
        d.has_elem = false;
        d.charged = false;
        d.inst = None;
        d.lookup_fighter = None;
        d.yaw_x = None;
        d.has_hit_a_fighter = false;
        d.age = 0;
        if projectile_blocks(w, &d, vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, ActorRef::Actor(j), vic, id, 1350.0, tick, None, false); // DodgingFighters: Jerry's handler
            continue;
        }
        dodge_over(w, ActorRef::Actor(j), vic);
        let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
        if apply_projectile_hit(w, &d, vic, &hit, &[], at, Some(V2 { x: -kx, y: 0.5 }), &[], 0.0) {
            let real = dealer(w, ActorRef::Actor(j));
            actor_hit_enemy(w, &real, vic);
        }
    }
}

/// Jerry_SpawnedActor's ForceReflect(Ball) @30322 -> the overlap's ball branch @15868: a TennisBall (either class) not
/// yet reflected (JerrySpawned false): when he faces away and plays no champagne montage RotateServer(!FacingLeft)
/// @7034, without a champagne montage PlayMontageMulticast(ReflectSwing) @7057; then @6622 the ball's BounceBack(0)
/// and the swing's hit at Jerry + 25 Z. Callers: the ball's BeginPlay (within 250), Jerry's ReceiveActorBeginOverlap
/// with a ball (Deviation tennis a: `ball_moved` and the spawn's overlap test here; jerry.rs's overlap does not test the
/// balls).
fn force_reflect(w: &mut World, j: ActorId, b: ProjectileId) {
    if !matches!(pr(w, b).ext, ProjectileExt::TennisBall(_)) || ball(w, b).is_reflected {
        return;
    }
    let m = crate::actors::actor_montage::montage_active(w, ActorRef::Actor(j));
    if m != Some(CHAMPAGNE_B) && m != Some(CHAMPAGNE_F) {
        if faces_away(w, j) {
            let left = jerry_of(w, j).is_some_and(|s| s.facing < 0.0);
            rotate(w, j, !left);
        }
        jerry::play_montage(w, j, REFLECT_SWING);
    }
    bounce_back(w, b);
    let Some(s) = jerry_of(w, j) else { return };
    let (x, y) = (s.x, s.y + 25.0);
    swing_hit(w, j, x, y);
}

/// Jerry's PlayMontageNotifyWindow switches for the tennis windows: TennisRacket begin @26761 / end @25036
/// (Tenis_Rachet_Mesh), TennisBall begin @26796 / end @25071 (the TennisBall mesh); the TennisBall end triggers Tom's
/// ability 3 (TriggerAbilityActual @25176) and runs @8387: the Reflected ball spawned at Jerry + 40 Z toward Tom
/// (CompSpawnActor with a world transform, the direction GetDirectionUnitVector(Jerry, Tom) mirrored for a Tom facing
/// right, which the spawner mirrors back: the world direction), speed 4000, then the swing's hit at Jerry. True: the
/// window is a tennis one. Called by jerry.rs's window switches.
/// Only Tom_and_Jerry_Only_Jerry_TennisServe_Montage has a TennisBall window, and no graph or notify of the data
/// plays it: the Reflected spawn is not reached in a match.
pub fn tennis_window(w: &mut World, j: ActorId, name: Name, begin: bool) -> bool {
    if name == name!("TennisRacket") {
        if let Some(s) = jerry::jerry_mut(w, j) {
            s.racket = begin;
        }
        return true;
    }
    if name != name!("TennisBall") {
        return false;
    }
    if let Some(s) = jerry::jerry_mut(w, j) {
        s.tennis_ball = begin;
    }
    if begin {
        return true;
    }
    let Some(f) = w.actors.actors.get(j).map(|a| a.owner) else { return true };
    trigger_ability(w, f, 3);
    let Some(s) = jerry_of(w, j) else { return true };
    let (jx, jy) = (s.x, s.y);
    let [_, sx, sz] = safe_normal([0.0, w.fighter(f).body.x - jx, actor_y(w, f) - jy], SMALL_NUMBER);
    let dir = [Some(0.0), Some(sx * facing(w, f)), Some(sz)];
    comp_spawn_projectile(
        w,
        f,
        name!("Tom_and_Jerry_TennisBall_Reflected_C"),
        jx,
        jy + 40.0,
        dir,
        4000.0,
        4000.0,
        None,
    );
    swing_hit(w, j, jx, jy);
    true
}

// ---------------------------------------------------------------- the ball

fn stop_loop(w: &mut World, b: ProjectileId) {
    if let Some(j) = valid(w, ball(w, b).jerry) {
        crate::actors::actor_montage::montage_stop(w, ActorRef::Actor(j), Some(TENNIS_LOOP));
    }
}

/// HomingServer(IsHoming) @4818.
fn homing_server(w: &mut World, b: ProjectileId, on: bool) {
    ball_mut(w, b).home_on = on; // HomingMulticast @5177
    if on && !ball(w, b).is_reflected {
        // (`pr.homeOn` is `on` here)
        let Some(j) = valid(w, ball(w, b).jerry) else { return }; // Accessed None
        jerry::play_montage(w, j, TENNIS_LOOP);
        set_facing_at_owner(w, j);
    } else {
        stop_loop(w, b);
    }
}

/// ReflectedMulticast(Reflected) @2068.
fn reflected_multicast(w: &mut World, b: ProjectileId, r: bool) {
    if !r {
        set_actor_scale(pm(w, b), 1.25); // @1844
        ball_mut(w, b).particle_visible = false;
        return;
    }
    {
        let s = ball_mut(w, b);
        s.bounce_number = 0;
        s.max_bounce = 2.0;
    }
    let following = valid(w, ball(w, b).jerry).and_then(|j| jerry_of(w, j)).is_some_and(|s| s.following.is_some());
    set_actor_scale(pm(w, b), if following { 1.5 } else { 1.25 }); // IsFollowing && !JerrySpawned @2236
    ball_mut(w, b).is_reflected = true; // @1556
    set_gravity_scale(pm(w, b), 0.0);
    set_bounciness(pm(w, b), 1.0);
    // AimDownProjectileMovement = true @1657: the class default already.
    sound(w, b, RETURN_SFX);
    ball_mut(w, b).particle_visible = true;
    sound(w, b, REFLECTED_SFX);
}

/// The bounce back @3828 (HitBoxOn? off, the velocity toward the owner, no homing, gravity 3, Bounciness 0.6).
fn bounce_back_move(w: &mut World, b: ProjectileId, vy: Real, vz: Real) {
    let owner_x = pr(w, b).owner.map_or(0.0, |o| w.fighter(o).body.x);
    ball_mut(w, b).home_on = false; // a plain write (no HomingServer: Jerry's TennisLoop is left)
    let p = pm(w, b);
    set_hitbox(p, false);
    p.vx = if p.x <= owner_x { vy } else { -vy }; // self UE Y >= owner UE Y: -Y (sim +x)
    p.vy = vz;
    set_gravity_scale(p, 3.0);
    set_bounciness(p, 0.6);
}

/// @3495 with BounceBackVelocity (500, 1000) (OnBounce, BounceOffHit). True: K2_DestroyActor.
fn on_bounce(w: &mut World, b: ProjectileId) -> bool {
    let n = {
        let s = ball_mut(w, b);
        s.bounce_number += 1;
        s.bounce_number
    };
    sound(w, b, BOUNCE_SFX);
    if crate::real(n) > ball(w, b).max_bounce {
        return true;
    }
    if ball(w, b).is_reflected || ball(w, b).do_once_closed {
        return false;
    }
    ball_mut(w, b).do_once_closed = true;
    bounce_back_move(w, b, 500.0, 1000.0);
    false
}

/// BounceBack(0) @6019.
fn bounce_back(w: &mut World, b: ProjectileId) {
    ball_mut(w, b).bounce_number = 1;
    reflected_multicast(w, b, false);
    let Some(o) = pr(w, b).owner else { return };
    let (oy, ox) = (actor_y(w, o), w.fighter(o).body.x);
    let p = pr(w, b);
    let (px, py) = (p.x, p.y);
    let kgs = num(class_p(w, p), "KnockBackGravityScale").unwrap_or(0.0);
    let z = (oy - py) + 2000.0;
    let g = (9.8 * kgs) * 100.0;
    let h = (py + div(z * z, g * 2.0)) - oy;
    let t = div(z + (g * sqrt0(div(h * 2.0, g))), g);
    let y = max(div((px - ox).abs() - 200.0, t), 400.0);
    ball_mut(w, b).do_once_closed = true;
    bounce_back_move(w, b, y, z);
}

/// Jerry's components against the ball's (Deviation tennis a).
fn touches(w: &World, b: ProjectileId, j: ActorId) -> bool {
    let Some(a) = w.actors.actors.get(j) else { return false };
    let Some(def) = actor_def(w, ActorRef::Fighter(a.owner), a.cls) else { return false };
    let Some(s) = jerry_of(w, j) else { return false };
    let c = &def.components;
    let ex = obj(c, "Box").and_then(|b| num_in(b, "BoxExtent", "Y")).unwrap_or(0.0);
    let ez = obj(c, "Box").and_then(|b| num_in(b, "BoxExtent", "Z")).unwrap_or(0.0);
    let p = pr(w, b);
    let r = crate::actors::projectile::capsule_radius(p);
    let dx = max((p.x - s.x).abs() - ex, 0.0);
    let dy = max((p.y - s.y).abs() - ez, 0.0);
    if dx * dx + dy * dy <= r * r {
        return true;
    }
    if !hit_collision(p) {
        return false;
    }
    let hb = obj(c, "ProjectileHurtBox");
    let sc = hb.and_then(|h| num_in(h, "RelativeScale3D", "Z")).unwrap_or(1.0);
    let hr = hb.and_then(|h| num(h, "CapsuleRadius")).unwrap_or(0.0) * sc;
    let hh = hb.and_then(|h| num(h, "CapsuleHalfHeight")).unwrap_or(0.0) * sc;
    let seg = max(hh - hr, 0.0);
    let (hx, hy) = hit_xy(w, p);
    let cy = max(s.y - seg, crate::math::min(hy, s.y + seg));
    let (qx, qy, rr) = (hx - s.x, hy - cy, p.hit_radius + hr);
    qx * qx + qy * qy <= rr * rr
}

/// Jerry's ReceiveActorBeginOverlap with the ball: ForceReflect's path @15868 at each begin.
fn jerry_overlaps(w: &mut World, b: ProjectileId) {
    let ids: Vec<ActorId> = w
        .actors
        .actors
        .iter()
        .filter(|(_, a)| {
            (a.cls == JERRY_CLS || a.cls == JERRY_TI_CLS) && matches!(a.state, WorldActorState::JerryActor(_))
        })
        .map(|(id, _)| id)
        .collect();
    for j in ids {
        if w.actors.projectiles.get(b).is_none_or(|p| p.dead) {
            return;
        }
        let seq = w.actors.actors.get(j).map_or(-1, |a| a.seq);
        let dead = w.actors.actors.get(j).is_none_or(|a| a.dead);
        let k = ball(w, b).over_jerry.iter().position(|&x| x == seq);
        let now = !dead && touches(w, b, j);
        if now && k.is_none() {
            ball_mut(w, b).over_jerry.push(seq);
            force_reflect(w, j, b);
        } else if !now && let Some(k) = k {
            ball_mut(w, b).over_jerry.remove(k);
        }
    }
}

/// HomingAim's height over Jerry's root: JerryMesh's Z plus HomingAim's Z times JerryMesh's scale.
fn homing_aim_z(w: &World, f: FighterId, j: ActorId) -> Real {
    let Some(a) = w.actors.actors.get(j) else { return 0.0 };
    let comps = actor_def(w, ActorRef::Fighter(f), a.cls).map(|d| &d.components);
    let jm = comps.and_then(|c| obj(c, "JerryMesh"));
    let ha = comps.and_then(|c| obj(c, "HomingAim"));
    let (Some(jm), Some(ha)) = (jm, ha) else {
        warn_text(w, &format!("{}: JerryMesh / HomingAim missing (the ball homes on his root)", w.names.str(a.cls)));
        return 0.0;
    };
    num_in(jm, "RelativeLocation", "Z").unwrap_or(0.0)
        + num_in(ha, "RelativeLocation", "Z").unwrap_or(0.0) * num_in(jm, "RelativeScale3D", "Z").unwrap_or(1.0)
}

/// The constructor: InstigatingTom, the homing flag and magnitude of the template, MaxBounceCount.
fn ball_make(w: &World, f: FighterId, r: &SpawnRequest) -> ProjectileExt {
    let a = actor_data(w, &w.data, ActorRef::Fighter(f), r.cls);
    let pm = a.and_then(|a| obj(&a.components, "ProjectileMovement"));
    let mb = a.and_then(|a| num(&a.p, "MaxBounceCount"));
    if mb.is_none() {
        warn_text(w, &format!("{} data missing: MaxBounceCount", w.names.str(r.cls)));
    }
    ProjectileExt::TennisBall(TennisBall {
        instigating_tom: Some(f),
        home_on: pm.is_some_and(|p| flag(p, "bIsHomingProjectile")),
        home_mag: pm.and_then(|p| num(p, "HomingAccelerationMagnitude")).unwrap_or(0.0),
        max_bounce: mb.unwrap_or(0.0),
        ..TennisBall::default()
    })
}

fn ball_begin(w: &mut World, b: ProjectileId) {
    let Some(owner) = pr(w, b).owner else { return };
    ball_mut(w, b).instigating_tom = Some(owner);
    if let Some(j) = last_jerry(w, owner) {
        let az = homing_aim_z(w, owner, j);
        let s = ball_mut(w, b);
        s.jerry = Some(ActorRef::Actor(j));
        s.home_target = Some(ActorRef::Actor(j));
        s.aim_z = az;
        s.end_bound = true;
        homing_server(w, b, true);
        let (p, js) = (pr(w, b), jerry_of(w, j));
        if js.is_some_and(|s| size([0.0, p.x - s.x, p.y - s.y]) <= 250.0) {
            force_reflect(w, j, b);
        }
    }
    if let Some(s) = tj_mut(w, owner) {
        s.tennis_balls.push(b);
    }
    jerry_overlaps(w, b); // the begin overlaps after BeginPlay
}

/// OnBounce @3403 / BounceOffHit @5779 through ProjectileMovement.OnProjectileBounce.
fn ball_bounce_impact(w: &mut World, b: ProjectileId, _nx: Real, _ny: Real, _ivx: Real, _ivy: Real) -> bool {
    on_bounce(w, b)
}

/// StopHoming @4802 on JerryActor's OnEndPlay (Deviation tennis c): HomingServer(false).
fn ball_step(w: &mut World, b: ProjectileId) -> bool {
    let s = ball(w, b);
    if s.end_bound && valid(w, s.jerry).is_none() {
        ball_mut(w, b).end_bound = false;
        homing_server(w, b, false);
    }
    false
}

fn ball_moved(w: &mut World, b: ProjectileId) {
    jerry_overlaps(w, b);
}

fn ball_hit_enemy(w: &mut World, b: ProjectileId, _v: FighterId) {
    // BounceOffHit @5779 (bound at BeginPlay), then KillOnHit @7576 (bound at the reflect).
    if valid(w, ball(w, b).jerry).is_none() && !ball(w, b).is_reflected && on_bounce(w, b) {
        destroy_projectile(w, b, false);
    }
    if ball(w, b).kill_on_hit && !pr(w, b).dead {
        destroy_projectile(w, b, false);
    }
}

fn ball_end(w: &mut World, b: ProjectileId) {
    let s = ball(w, b);
    if !s.is_reflected && s.home_on {
        stop_loop(w, b);
    }
    let Some(tom) = ball(w, b).instigating_tom else { return };
    if let Some(s) = tj_mut(w, tom)
        && let Some(i) = s.tennis_balls.iter().position(|&x| x == b)
    {
        s.tennis_balls.remove(i);
    }
}

/// Reflected() @7840: StopMontageMulticast. The parent's UpdateHitBoxDamage @4931 before it put HitBox2D at scale 1
/// (Unverified tennis b).
fn ball_reflected(w: &mut World, a: ActorRef) {
    let ActorRef::Projectile(b) = a else { return };
    let rel = ball(w, b).hit_rel;
    pm(w, b).hit_radius /= rel;
    ball_mut(w, b).hit_rel = 1.0;
    stop_loop(w, b);
}

fn ball_multicast_pause(w: &World, a: ActorRef, by: FighterId, _pause: bool) -> bool {
    let ActorRef::Projectile(b) = a else { return false };
    ball(w, b).instigating_tom != Some(by)
}

/// @7457, behind the parent's MulticastReflect (which dropped HomingTargetComponent @23793).
fn ball_multicast(w: &mut World, a: ActorRef, by: FighterId, _pause: bool) {
    let ActorRef::Projectile(b) = a else { return };
    ball_mut(w, b).home_target = None;
    if ball(w, b).is_reflected || tj_of(w, by).is_none() {
        return;
    }
    {
        let s = ball_mut(w, b);
        s.is_reflected = true;
        s.kill_on_hit = true;
    }
    reflected_multicast(w, b, true);
    stop_loop(w, b);
    // Comp_Actor_Destructible.DamageDisabled = false @2434 (the class data has it true): an enemy's DestroyProjectiles
    // hit now damages the reflected ball (CanBeHitBy, HitByEnemy, Hit true).
    let rel = if valid(w, ball(w, b).jerry).and_then(|j| jerry_of(w, j)).is_some_and(|s| s.following.is_some()) {
        1.5
    } else {
        1.0
    };
    let audio = if ball(w, b).particle_visible { COMBO_HIT_SFX } else { BALL_HIT_SFX };
    let old_rel = ball(w, b).hit_rel;
    ball_mut(w, b).home_on = false;
    let p = pm(w, b);
    p.damage_disabled = Some(false);
    set_bounciness(p, 1.0);
    p.hit.damage = Some(7.0);
    p.hit.base_knock_back = Some(1800.0);
    p.hit.knock_back_scaling = Some(20.0);
    p.hit.knock_back_direction_vector = Some(DataVec { x: Some(1.0), y: Some(0.5), z: None });
    p.hit.hit_stun_mult = Some(1.0);
    p.hit.on_hit_audio = Some(audio);
    p.hit.velocity_instead_of_facing = Some(false);
    p.hit.ignore_facing = Some(false);
    p.hit.hit_each_actor_only_once = Some(true);
    p.hit_radius *= rel / old_rel;
    ball_mut(w, b).hit_rel = rel;
    reflect_boxes_on(pm(w, b)); // UpdateHitBoxDamage's HitBoxesOn, as the parent's
}

// ---------------------------------------------------------------- TomAndJerry_NoJerry_SlingShot_Aim

fn aim_of(w: &World, id: ActorId) -> Option<&NoJerryAim> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::NoJerryAim(s) => Some(s),
        _ => None,
    }
}

fn aim_mut(w: &mut World, id: ActorId) -> Option<&mut NoJerryAim> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::NoJerryAim(s) => Some(s),
        _ => None,
    }
}

/// Mesh.GetSocketLocation(tom_un_C_neck_a).
fn neck(w: &World, f: FighterId) -> (Real, Real) {
    let sampled = (|| {
        let b = w.fixed.samplers.get(&w.fighter(f).ch)?;
        let i = *b.by_lower.get("socket:tom_un_c_neck_a")?;
        let s = crate::fighter::anim::sampled_boxes(w, f)?;
        let o = usize::try_from(b.offsets[i as usize]).expect("a box offset");
        let (x, y) = (crate::real(s[o]), crate::real(s[o + 1]));
        crate::fighter::anim::put_pose(w, s);
        Some((mesh_wx(w, f) + x * facing(w, f), mesh_wy(w, f) + y))
    })();
    sampled.unwrap_or_else(|| {
        warn_text(
            w,
            "TomAndJerry_NoJerry_SlingShot_Aim_C: socket tom_un_C_neck_a not sampled (the actor location taken)",
        );
        (w.fighter(f).body.x, actor_y(w, f))
    })
}

/// UKismetMathLibrary::NormalizeToRange.
fn normalize_to_range(v: Real, lo: Real, hi: Real) -> Real {
    if lo == hi {
        return if v < lo { 0.0 } else { 1.0 };
    }
    let (lo, hi) = if lo > hi { (hi, lo) } else { (lo, hi) };
    (v - lo) / (hi - lo)
}

/// SetLocationServer(NewLocation) @4469 -> SetLocationMulticast @1276.
fn set_location(w: &mut World, id: ActorId, nx: Real, ny: Real) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    if let Some(m) = aim_mut(w, id) {
        m.aim_x = nx;
        m.aim_y = ny;
    }
    if tj_of(w, f).is_none() {
        return; // the BPI_AnimBP casts of another pawn's AnimBP: nothing of Tom and Jerry's to write
    }
    let jerry = aim_of(w, id).and_then(|m| valid(w, m.jerry));
    let Some(j) = jerry else {
        set_aim_direction(w, f, 0.0);
        super::state::tj_w(w, f).anim.mirror_override = false;
        if let Some(m) = aim_mut(w, id) {
            m.aim_direction = 0.0;
        }
        return;
    };
    let fl = facing(w, f) < 0.0;
    let p = actor_p(w, ActorRef::Actor(id));
    let sv = obj(p, "StartAimVector");
    let (svy, svz) = (sv.and_then(|s| num(s, "Y")).unwrap_or(0.0), sv.and_then(|s| num(s, "Z")).unwrap_or(0.0));
    let a1 = deg_atan2(svz, svy);
    let sel = if fl { a1 } else { -a1 };
    let amax = sel + num(p, "AimAngleMax").unwrap_or(0.0);
    let amin = sel + num(p, "AimAngleMin").unwrap_or(0.0);
    let (n_x, n_y) = neck(w, f);
    // GetDirectionUnitVector(neck, NewLocation) in UE (Y, Z); the other one @2253 is its opposite.
    let [_, uy, uz] = safe_normal([0.0, -(nx - n_x), ny - n_y], SMALL_NUMBER);
    let (d1, d2) = (deg_atan2(uz, uy), deg_atan2(uz, -uy));
    let jx = jerry_of(w, j).map_or(0.0, |s| s.x);
    let g = jx < w.fighter(f).body.x; // Jerry UE Y > Tom UE Y
    let behind = (g && !fl) || (!g && fl);
    let pick = if behind { g } else { fl };
    let angle = if pick { d1 } else { d2 };
    let aim = (normalize_to_range(angle, amin, amax) * 2.0) - 1.0;
    set_aim_direction(w, f, aim);
    if let Some(m) = aim_mut(w, id) {
        m.aim_direction = aim;
    }
    super::state::tj_w(w, f).anim.mirror_override = behind; // BPI_FacingOverride
}

fn aim_tick(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    let jerry = aim_of(w, id).and_then(|m| valid(w, m.jerry));
    if let Some(j) = jerry {
        let (x, y) = jerry_of(w, j).map_or((0.0, 0.0), |s| (s.x, s.y));
        return set_location(w, id, x, y + 80.0);
    }
    let (x, y) = neck(w, f);
    set_location(w, id, x + 500.0 * facing(w, f), y);
}

fn ball_class(c: Name) -> bool {
    c == name!("Tom_and_Jerry_TennisBall_C") || c == name!("Tom_and_Jerry_TennisBall_Reflected_C")
}

fn activate_aim(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    if in_hitstun(w, f) {
        return; // CheckHitstunActive @6103
    }
    // "Get Spawned Actor"(TennisBall_C, all) @5912: the class list (a child passes), as the spawned list keeps it.
    let mut count = 0;
    let mut last = None;
    for (pid, p) in w.actors.projectiles.iter() {
        if p.owner != Some(f) || p.dead || !ball_class(p.cls) || p.reflect.as_ref().is_some_and(|r| r.unlisted) {
            continue;
        }
        count += 1;
        last = Some(pid);
    }
    if count >= 8
        && let Some(l) = last
    {
        destroy_projectile(w, l, false); // DestroySpawnedActors(TennisBall_C, last) @5077
    }
    let cls = name!("Tom_and_Jerry_TennisBall_C");
    let Some(ap) = actor_def(w, ActorRef::Fighter(f), cls).map(|a| a.p.clone()) else {
        warn_text(w, "spawned actor not implemented (Tom_and_Jerry_TennisBall_C)");
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true; // K2_DestroyActor @5062
        }
        return;
    };
    // GetDirectionUnitVector(neck, ServerLocation), mirrored for a Tom facing right @5589-5686 and back by the spawner.
    let (n_x, n_y) = neck(w, f);
    let (ax, ay) = aim_of(w, id).map_or((0.0, 0.0), |m| (m.aim_x, m.aim_y));
    let [_, sx, sz] = safe_normal([0.0, ax - n_x, ay - n_y], SMALL_NUMBER);
    actor_spawn_ability(w, f, &ap);
    let mut dir = Props::new();
    dir.insert("X".into(), Value::Num(0.0));
    dir.insert("Y".into(), Value::Num(f64::from(sx * facing(w, f))));
    dir.insert("Z".into(), Value::Num(f64::from(sz)));
    let mut np = Props::new();
    np.insert("Socket".into(), Value::Str("tom_un_L_wrist".into()));
    np.insert("InitialMovementDirection".into(), Value::Obj(dir));
    np.insert("InitialSpeed".into(), Value::Num(4000.0));
    np.insert("MaxSpeed".into(), Value::Num(4000.0));
    let attack_id = w.fighter(f).decay.attack_count;
    let req = SpawnRequest {
        cls,
        n: NotifyRef { montage: Name::NONE, index: 0 },
        attack_id,
        foreign: Foreign::default(),
        copy: None,
        charge: None,
        aim: None,
        props: Some(Arc::new(np.clone())),
        instigator: None,
        origin: None,
    };
    let mut made = crate::actors::projectile::make_projectile(
        w,
        cls,
        ProjectileClass::TomAndJerryTennisBall,
        Some(f),
        attack_id,
        &np,
        None,
        crate::actors::projectile::MakeOpts::default(),
    );
    made.ext = ball_make(w, f, &req);
    let b = w.actors.projectiles.insert(made);
    add_projectile(w, b, Some(ball_begin));
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true; // K2_DestroyActor @5062
    }
}

/// The aimer made from the spawn notify: its place the spawn's (socket None), JerryAcor, Delay(0.1).
fn aim_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls);
    let np = request_props(w, f, r);
    let at = spawn_place(w, f, &np);
    let sl = a.and_then(|a| obj(&a.p, "ServerLocation"));
    let (sly, slz) = (sl.and_then(|s| num(s, "Y")).unwrap_or(0.0), sl.and_then(|s| num(s, "Z")).unwrap_or(0.0));
    let m = NoJerryAim {
        x: at.x,
        y: at.y,
        aim_x: -sly,
        aim_y: slz,
        aim_direction: 0.0,
        jerry: last_jerry(w, f).map(ActorRef::Actor),
        wait: latent_frames(0.1),
    };
    (WorldActorState::NoJerryAim(m), false)
}

/// The rest of `spawn` that needs the actor's id: TomAndJerryState.noJerryAimer.
fn aim_spawned(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    if let Some(s) = tj_mut(w, f) {
        s.no_jerry_aimer = Some(ActorRef::Actor(id));
    }
}

fn aim_step(w: &mut World, id: ActorId) {
    if w.actors.actors.get(id).is_none_or(|a| a.dead) {
        return;
    }
    aim_tick(w, id);
    // Its latent action after the tick (the BeginPlay Delay counts the spawn frame).
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    let fire = aim_mut(w, id).is_some_and(|m| {
        if m.wait > 0.0 {
            m.wait -= 1.0;
            m.wait == 0.0
        } else {
            false
        }
    });
    if fire {
        let cur = active_montage_name(w, f);
        if cur != Some(SLING)
            && cur != Some(SLING_AIR)
            && let Some(a) = w.actors.actors.get_mut(id)
        {
            a.dead = true;
        }
    }
}

fn aim_activate(w: &mut World, id: ActorId) {
    if w.actors.actors.get(id).is_some_and(|a| !a.dead) {
        activate_aim(w, id);
    }
}

fn aim_end(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    if let Some(s) = tj_mut(w, f)
        && s.no_jerry_aimer == Some(ActorRef::Actor(id))
    {
        s.no_jerry_aimer = None;
    }
}

/// `ballKind`.
fn ball_kind() -> ProjectileHooks {
    ProjectileHooks {
        make: Some(ball_make),
        begin: Some(ball_begin),
        bounce_impact: Some(ball_bounce_impact),
        step: Some(ball_step),
        moved: Some(ball_moved),
        hit_enemy: Some(ball_hit_enemy),
        end: Some(ball_end),
        reflect: ReflectHooks {
            reflected: Some(ball_reflected),
            multicast_pause: Some(ball_multicast_pause),
            multicast: Some(ball_multicast),
            ..ReflectHooks::default()
        },
        ..ProjectileHooks::default()
    }
}

/// This file's registry entries (the two balls, the aimer).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let ball = ActorEntry::new(
        ActorClass::Projectile(ProjectileClass::TomAndJerryTennisBall),
        ActorHooks::Projectile(ball_kind()),
    );
    r.actors.enter(TENNIS_BALL, ball)?;
    let reflected = ActorEntry {
        parents: &[TENNIS_BALL],
        ..ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::TomAndJerryTennisBallReflected),
            ActorHooks::Projectile(ball_kind()),
        )
    };
    r.actors.enter(TENNIS_BALL_REFLECTED, reflected)?;
    let aim = WorldActorHooks {
        spawn: Some(aim_spawn),
        spawned: Some(aim_spawned),
        step: Some(aim_step),
        activate: Some(aim_activate),
        end: Some(aim_end),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        NO_JERRY_AIM,
        ActorEntry::new(ActorClass::World(WorldActorClass::TomAndJerryNoJerrySlingShotAim), ActorHooks::World(aim)),
    )?;
    Ok(())
}
