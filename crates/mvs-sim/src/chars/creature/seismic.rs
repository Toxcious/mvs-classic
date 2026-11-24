//! Reindog's seismic reactor: SeismicReactor_C ([`SeismicReactor`], a projectile's
//! `ext`): its spin, its HP, its charge up, the DamageBox, the sway, the trace down to the ground, the boom and the
//! blast-box checks; the AirSupport perk's buff it reads.
//!
//! Thrown by Creature_Throw_Seismic_Montage's AnimNotify_Spawner_C (1.585 s, socket joint1, translation (0, 0, 150),
//! straight up at 2500, ChargeAffectsInitialSpeed x 4) and by Creature_AirThrow_Seismic_Montage's (R_Hand, 2000);
//! Creature_Throw_Seismic_Montage (1.297 s, ahead of its spawner) and Creature_Detonate_Seismic_Montage activate every
//! reactor of the class (ActivateAllObjects). Offsets are the ubergraph's.
//!
//! The class's graph, by entry:
//! - ReceiveBeginPlay (the parent's first: SpawnedActorParent clamps a place outside the blast box into it,
//!   ForceSpawnPositionInsideBlastBox; DestroyOnExitedArena? is false in the class data, so the parent sets no blast
//!   timer) @10085: Begin Play Visuals (PS_CreatureCrystal's Team; Delay(0.15) @11056 -> @1714), Delay(0.15) @10267 ->
//!   @1575, StartingRotation = the actor's rotation @10225, Charge Up @10252, ActivationConditions[0] = true @10126
//!   (read by nothing of the class).
//! - @1714 and @1575 (0.15 s): SetVelocityInLocalSpace(0), ProjectileGravityScale = GravityScale (0.0065),
//!   SetReplicateMovement(false); @1575 then DestroyOnExitedArena? = true @1688 and CheckForBlastBox @1699: a place
//!   outside the blast box is clamped into it, and the 0.1 s looping CheckBlastBoxTimer is set (the parent's
//!   CheckBlastBoxExit: K2_DestroyActor outside the box).
//! - Charge Up: Charge Up Visuals @8886 (sfx_cha_reindog_seismic_crystal_fire_leadin on the Instigator, the charge
//!   emitters), ProjectileGravityScale = 0 @8900, RotatingMovement.RotationRate yaw 360 @8750, Delay(0.25) @1883 ->
//!   @503: WarningVisualsMulticast, rate 900 @517, Delay(0.25) @570 -> @625: rate 2000, Delay(0.5) @678 -> @733:
//!   Delay(0.2) -> @788: ProjectileGravityScale = GravityScale, SeismicBoom @837.
//! - SeismicBoom @8950: HitActors cleared, SetActorRotation(StartingRotation) @8991, LineTraceSingle(TraceTypeQuery 3)
//!   from the actor straight down 20000 @9113; P = the hit's Location, else the trace's end @9490; Seismic Boom
//!   Visuals(P) @9545 (sfx_cha_reindog_seismic_crystal_fire at P, the beam and impact emitters);
//!   BoxTraceMultiForObjects(Pawn) from the actor to P, half extent (50, 50, 0) @9888, every hit actor once into
//!   HitActors; only with the line trace's blocking hit BoxOverlapActors(P + (0, 0, 25), extent (Blast Radius 400,
//!   400, Blast Height 50), BP_FighterCharacter, ignoring HitActors), appended. Then for each one that is a BaseFighter
//!   @3491: KnockBackDirectionVector = (Y, Z) of Normal((its location - P) x (0, 25, 2), 0.0001) @3982, a Y in
//!   [0, 0.1] taken as (0.1, Z) @4720 and one in [-0.1, 0] as (-0.1, Z) @8163 (Z: the normal's, not normalised
//!   again); the hit built in the graph @5090-6108 (Damage 5, BaseKnockBack 1000, KnockBackScaling 10, HitReaction 2,
//!   HitPauseTime 0.175, HitStunMult 1, OnHitAudio sfx_cha_reindog_seismic_crystal_hit, IgnoreFacing,
//!   HitEachActorOnlyOnce false, DestroyProjectiles false) and its OnHitBuffs (Buff_Damage_Electric x 2 on a hit
//!   enemy, duration 0; Buff_AllyElectricGauntlets x 0 for 10 s on a hit ally); Comp_Pawn_HitBoxHandler.pfgDoDamage
//!   (null, the fighter, ...) @6654; then, the fighter being the Instigator @6779, SpawnBuffActor(
//!   Buff_AllyElectricGauntlets, the Instigator, the Instigator, 0 stacks, 10 s) @6898. After the loop Delay(0.2)
//!   @4869 -> Cooldown @852: Cooldown Visuals, Delay(2.0) @1828 -> Charge Up @15, rate 0 @8832.
//! - ReceiveAnyDamage @12816: HP -= the damage; at <= 0 sfx_cha_reindog_generic_hit at the DamageBox and
//!   K2_DestroyActor. ReceiveDestroyed @13066 (DestroyVisuals, once): sfx_cha_reindog_seismic_crystal_end at the
//!   DamageBox, the destroy emitter. ActivateObject @14471: K2_DestroyActor.
//! - The sway (Perk_Creature_AirSupport): ReceiveBeginPlay @13831, behind the part above (push_flow 13871, goto
//!   10085): an owner that implements BPI_BaseFighter @13891 -> its BPI_CheckIfHasBuff(Buff_Creature_AirSupport)
//!   @14042 -> SeismicSwayOn @14221; with it and authority StartTime = the game time @14337 and SeismicStartDirection
//!   = RandomBool() @14364-14384 (an owner without the interface: SeismicSwayOn = false @14454).
//!   WarningVisualsMulticast @14915: SeismicSwayOn pops the flow, so neither the ground warning (@15440) nor the bolt
//!   warning (@30-444) is spawned (the view's). ReceiveTick @15514: with VSize(ProjectileMovement.Velocity) > 0,
//!   authority and SeismicSwayOn, and SwayFollowsInstead (true in the class data) @15704: a valid Instigator @15738,
//!   dy = Instigator.Y - self.Y (UE Y) @16009, speed = Lerp(0, FollowMaxSpeed, FClamp(|dy| / 1000, 0, 1)) @16215,
//!   MulticastVelocity((X, Lerp(Velocity.Y, dy < 0 ? -speed : speed, 0.5 x DeltaSeconds), Z)) @16431-16541 -> @17263
//!   ProjectileMovement.Velocity = it. The oscillation branch (@16565-17239, SecondsForOsscilation /
//!   MaxSpeedOnOsscilation / StartTime / SeismicStartDirection) is not reached with this data. The buff itself is a
//!   marker (`perks.rs`).
// Unverified: RandomBool (FMath::RandBool: one FRand) is drawn from the world's stream (`World::random_int`), as the
// port's other random draws; its value is read by the dead branch alone, the draw keeps the stream in step.
//!
//! Model. The reactor is a Projectile of the world's list (ParentProjectilePhysics' movement: bounce 0.1, gravity scale
//! from the graph); its HitBox is off (HitBoxOn? false) and it has no HurtBox, so no shot or hit box of the parent's
//! reaches it. Its DamageBox (extent (80, 110, 70), scale Z 1.2, object type Pawn, HitBox Overlap, at the actor) turns
//! with the actor's yaw and is what a melee hit box meets (`ProjectileHooks::damage_box`), a projectile's or a thrown
//! item's hit box too, and a Pawn query ([`damage_box_body`]). The spawn place is FindTeleportSpot's for the Capsule
//! (20 / 70, the stage block alone: it ignores the Platform channel). Its Delays count on the actor's age in its kind's
//! step, as the tether's: one made in BeginPlay counts the frame it is made in, one made in a Delay's link from the
//! next.
// Unverified: RotatingMovement's yaw step is taken ahead of the actor's latent actions in the frame (the component's
// tick order against its owner is not read), so the boom's SetActorRotation holds for the frame it is in; the step is
// a float32 yaw sum wrapped at 360 (the component's rotator / quaternion composition is not read).
// Unverified: the column's hits are taken in the order the box meets the capsules going down (the capsule's top, the
// start inside first), the overlap's in fighter order; both against the fighters' root capsules in the play plane
// (depth 0).
// Unverified: the line trace (TraceTypeQuery 3) is taken to stop on the stage block, on what blocks a trace
// (`BlockKind::Trace`) and on the stage meshes' complex triangles ([`complex_down`]); a start inside a block is a hit
// at the start.
// Unverified: Creature_Throw_Seismic_Montage's socket joint1 is neither a bone nor a socket of
// Creature_Skelmesh_New_Skeleton, the skeleton of every Reindog mesh (only the old Creature_Skelmesh's physics asset
// and the mirror table name it): the Projectile constructor takes the mesh component's place plus the notify's
// translation and warns 'spawn socket missing: joint1'; the native GetSocketTransform of an unknown name is not read.
// Unverified: the reactor's depth is taken as 0 (the spawn socket's depth is not read); Normal's tolerance as UE's
// GetSafeNormal (on the squared size).
// Track E (render): the charge, warning, beam, impact, activate and destroy emitters and the spin of the actor's yaw
// ([`SeismicReactor::spin`]) are not drawn; PS_CreatureCrystal's SnapToTarget follows the actor.

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated comparisons keep NaN")]

use std::cmp::Ordering;

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_p, truthy};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{buff_of, spawn_buff};
use crate::combat::elem::{charge_buff, charge_pct};
use crate::combat::hits::{
    actor_hit_enemy, ally_hit, apply_projectile_hit, dealer, hit_buffs_ally, hit_buffs_call, projectile_blocks,
};
use crate::combat::{DataEnum, DataVec, HitPacket, OnHitBuffRow};
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::common::{actor_components, latent_frames, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::moves::V2;
use crate::fighter::traces::{TraceBody, body_info};
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, invulnerable, no_collision};
use crate::math::ue::{KINDA_SMALL_NUMBER, normal, quat_rotate_vector, rotator_quaternion, size2d};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::props::{num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{blast_box_loop, check_blast_box_exit, spawn_box};
use crate::world::pawns::match_fighters;
use crate::world::spawned_platform::BlockKind;
use crate::world::spawning::destroy_projectile;
use crate::world::stage_query::Shapes;
use mvs_data::value::Props;

const REACTOR: &str = "SeismicReactor_C";
const GAUNTLETS: Name = name!("Buff_AllyElectricGauntlets_C");
/// `AIR_SUPPORT`: the AirSupport perk's buff (read by perks.rs).
pub const AIR_SUPPORT: Name = name!("Buff_Creature_AirSupport_C");

// The charge's links, in frames after its start (Charge Up): @503, @625, @733, @788 (the boom), then the boom's
// Delay(0.2) -> Cooldown, Cooldown's Delay(2.0) -> Charge Up. (The view takes the warning and the boom from these.)
/// The warning's frame (@503).
pub fn at_warn() -> Real {
    latent_frames(0.25)
}
fn at_fast() -> Real {
    at_warn() + latent_frames(0.25)
}
fn at_wait() -> Real {
    at_fast() + latent_frames(0.5)
}
/// The boom's frame (@788).
pub fn at_boom() -> Real {
    at_wait() + latent_frames(0.2)
}
fn at_cool() -> Real {
    at_boom() + latent_frames(0.2)
}
fn at_next() -> Real {
    at_cool() + latent_frames(2.0)
}
/// The two BeginPlay Delays.
fn at_begin() -> Real {
    latent_frames(0.15)
}

/// The hit SeismicBoom builds @5090-6108 (`ky`, `kz`: its KnockBackDirectionVector per target).
fn boom_hit(ky: Real, kz: Real) -> HitPacket {
    HitPacket {
        damage: Some(5.0),
        base_knock_back: Some(1000.0),
        knock_back_scaling: Some(10.0),
        attack_tag: Some(name!("None")),
        hit_reaction: Some(DataEnum::Value(2)),
        hit_pause_time: Some(0.175),
        hit_stun_mult: Some(1.0),
        on_hit_audio: Some(name!("sfx_cha_reindog_seismic_crystal_hit")),
        ignore_facing: Some(true),
        attack_id: Some(0),
        play_on_hit_effects: Some(true),
        apply_hit_pause_to_damage_dealer: Some(true),
        additive_kb_mode: Some(DataEnum::Value(0)),
        kb_scaling_starting_dmg: Some(0.0),
        hit_each_actor_only_once: Some(false),
        destroy_projectiles: Some(false),
        kbi_multiplier: Some(1.0),
        knock_back_direction_vector: Some(DataVec { x: Some(ky), y: Some(kz), z: None }),
        ..HitPacket::default()
    }
}

/// Its OnHitBuffs @6205-6618.
fn boom_buffs() -> [OnHitBuffRow; 2] {
    [
        OnHitBuffRow {
            applied_buffs: name!("Buff_Damage_Electric_C"),
            apply_buff_self: false,
            apply_buff_hit: true,
            apply_buff_enemy: true,
            apply_buff_ally: false,
            require_hit_to_apply_buff_to_self: false,
            stack_count: Some(2),
            buff_duration: Some(0.0),
        },
        OnHitBuffRow {
            applied_buffs: GAUNTLETS,
            apply_buff_self: false,
            apply_buff_hit: true,
            apply_buff_enemy: false,
            apply_buff_ally: true,
            require_hit_to_apply_buff_to_self: false,
            stack_count: Some(0),
            buff_duration: Some(10.0),
        },
    ]
}

/// A number of the class data the graph reads (no default: a missing one warns and reads 0).
fn num_w(w: &World, p: &Props, key: &str) -> Real {
    num(p, key).unwrap_or_else(|| {
        warn_text(w, &format!("{REACTOR} data missing: {key}"));
        0.0
    })
}

/// `SeismicReactor extends Projectile`: what the class adds.
#[derive(State, Debug)]
pub struct SeismicReactor {
    /// The actor's yaw less StartingRotation's, in degrees (RotatingMovement).
    pub spin: Real,
    /// RotatingMovement.RotationRate.Yaw, degrees a second.
    pub rate: Real,
    /// HP.
    pub hp: Real,
    /// The age the current Charge Up ran at.
    pub charge_at: i32,
    /// The age CheckForBlastBox set CheckBlastBoxTimer at (-1: not set).
    pub blast_from: i32,
    /// The timer's next expiry, in seconds of counted deltas.
    pub blast_next: crate::Clock,
    /// DestroyVisuals' DoOnce.
    pub end_shown: bool,
    /// SeismicSwayOn (Perk_Creature_AirSupport), set at BeginPlay only when on: a reactor of a fighter without the perk
    /// stays unset, so no state hash of a perk-less match moves.
    pub sway: bool,
}

impl Default for SeismicReactor {
    /// The field initial values.
    fn default() -> Self {
        SeismicReactor {
            spin: 0.0,
            rate: 0.0,
            hp: 0.0,
            charge_at: 0,
            blast_from: -1,
            blast_next: f64::from(0.1_f32),
            end_shown: false,
            sway: false,
        }
    }
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn rx(w: &mut World, id: ProjectileId) -> &mut SeismicReactor {
    match &mut pm(w, id).ext {
        ProjectileExt::SeismicReactor(r) => r,
        e => panic!("{REACTOR} without its state: {e:?}"),
    }
}

fn rd(w: &World, id: ProjectileId) -> &SeismicReactor {
    match &pr(w, id).ext {
        ProjectileExt::SeismicReactor(r) => r,
        e => panic!("{REACTOR} without its state: {e:?}"),
    }
}

/// The constructor's tail: HP from the class data.
fn construct(w: &World, p: &mut Projectile) {
    let hp = num_w(w, class_p(w, p), "HP");
    p.ext = ProjectileExt::SeismicReactor(SeismicReactor { hp, ..SeismicReactor::default() });
}

/// The actor's yaw in UE degrees: Comp_Spawner's FacingLeft ? 90 : -90 (sim +x is UE yaw -90), plus the spin.
fn yaw(w: &World, id: ProjectileId) -> Real {
    (if pr(w, id).facing > 0.0 { -90.0 } else { 90.0 }) + rd(w, id).spin
}

/// The DamageBox now: centre the actor (no relative location), half extents (80, 110, 70 x 1.2) along the actor's X,
/// Y and Z; the axes as (sim x, sim y, depth): UE X at yaw psi (cos, sin, 0) is (-sin, 0, cos), UE Y (-sin, cos, 0)
/// is (-cos, 0, -sin).
fn damage_box(w: &World, id: ProjectileId) -> Option<WorldSphere> {
    let p = pr(w, id);
    let owner = p.owner.expect("a reactor has an owner");
    let comp = actor_components(w, ActorRef::Fighter(owner), p.cls).and_then(|c| obj(c, "DamageBox"));
    let (Some(e), Some(s)) = (comp.and_then(|a| obj(a, "BoxExtent")), comp.and_then(|a| obj(a, "RelativeScale3D")))
    else {
        warn_text(w, &format!("{REACTOR} data missing: DamageBox BoxExtent / RelativeScale3D"));
        return None;
    };
    let g = |p: &Props, k: &str| num_w(w, p, k);
    let (hx, hy, hz) = (g(e, "X") * g(s, "X"), g(e, "Y") * g(s, "Y"), g(e, "Z") * g(s, "Z"));
    // the component transform's rotation (0, yaw, 0) (FRotator::Quaternion) turning the box's unit X and Y axes
    let q = rotator_quaternion([0.0, yaw(w, id), 0.0]);
    let (ax, ay) = (quat_rotate_vector(q, [1.0, 0.0, 0.0]), quat_rotate_vector(q, [0.0, 1.0, 0.0]));
    Some(WorldSphere {
        x: p.x,
        y: p.y,
        z: Some(0.0),
        r: 0.0,
        bx: Some([-ax[1] * hx, 0.0, ax[0] * hx, -ay[1] * hy, 0.0, ay[0] * hy, 0.0, hz, 0.0]),
        ..WorldSphere::default()
    })
}

/// The DamageBox as a body of the traces (`ActorKind.bodies`; object type Pawn, so the Pawn queries meet it:
/// C021_DoubleStrikeProjectile's explosion, C021_ForceField's begin trace, the Earth Smash pillar's overlap).
// Deviation: the box's section by the play plane (depth 0): half width min(hx / |sin|, hy / |cos|) along sim x, hz up;
// a query shape that meets a turned corner off the plane is not seen.
fn damage_box_body(w: &World, a: ActorRef, out: &mut Vec<TraceBody>) {
    let ActorRef::Projectile(id) = a else { return };
    let p = pr(w, id);
    let owner = p.owner.expect("a reactor has an owner");
    let comp = actor_components(w, ActorRef::Fighter(owner), p.cls).and_then(|c| obj(c, "DamageBox"));
    let Some(info) = body_info(comp, None) else { return };
    let db = if info.on { damage_box(w, id) } else { None };
    let (Some(db), Some(comp)) = (db, comp) else { return };
    let b = db.bx.expect("the damage box's axes");
    let (ux, uy) = (b[0].abs(), b[3].abs()); // |sin| hx, |cos| hy along sim x
    let ext = |k: &str| {
        let (e, s) = (obj(comp, "BoxExtent"), obj(comp, "RelativeScale3D"));
        let g = |p: Option<&Props>| p.map_or(0.0, |p| num_w(w, p, k));
        g(e) * g(s)
    };
    let (hx, hy) = (ext("X"), ext("Y"));
    let half = min(
        if ux > 1e-6 { (hx * hx) / ux } else { Real::INFINITY },
        if uy > 1e-6 { (hy * hy) / uy } else { Real::INFINITY },
    );
    out.push(TraceBody {
        ax: db.x,
        ay: db.y,
        bx: db.x,
        by: db.y,
        hx: half,
        hy: b[7],
        r: 0.0,
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: name!("DamageBox"),
        actor: Some(a),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

/// ReceiveTick's follow branch @15514-16541 (header): the velocity's UE Y eased toward the Instigator's side, at most
/// FollowMaxSpeed, by 0.5 x DeltaSeconds of the gap a tick. Sim x = -UE Y; float32 throughout.
fn sway(w: &mut World, id: ProjectileId) {
    let p = pr(w, id);
    if !(p.vx * p.vx + p.vy * p.vy > 0.0) {
        return;
    }
    let cp = class_p(w, p);
    if !truthy(cp.get("SwayFollowsInstead")) {
        warn_text(w, &format!("{REACTOR}: the oscillating sway (SwayFollowsInstead false) is not ported"));
        return;
    }
    let inst = w.fighter(p.owner.expect("a reactor has an owner")).body.x;
    let alpha = 0.5 * DELTA_SECONDS;
    let dy = -inst - -p.x; // Instigator.Y - self.Y
    let speed = num_w(w, cp, "FollowMaxSpeed") * min(max(dy.abs() / 1000.0, 0.0), 1.0);
    let target = if dy < 0.0 { -speed } else { speed };
    let vy = -p.vx; // Velocity.Y
    pm(w, id).vx = -(vy + (alpha * (target - vy)));
}

/// Charge Up (@8886 -> @8750): the lead-in sound on the Instigator, no gravity, the slow spin.
fn charge_up(w: &mut World, id: ProjectileId) {
    let p = pr(w, id);
    let (age, fighter) = (p.age, p.owner_id);
    let e = Event::Sound {
        fighter,
        ev: name!("sfx_cha_reindog_seismic_crystal_fire_leadin"),
        x: None,
        y: None,
        hit: None,
        pooled: false,
        key: None,
    };
    w.events.push(e);
    crate::actors::projectile::set_gravity_scale(pm(w, id), 0.0);
    let r = rx(w, id);
    r.charge_at = age;
    r.rate = 360.0;
}

/// LineTraceSingle from (x, y) to (x, y - 20000): the highest block top under the start, or the start itself inside a
/// block; `None` without a hit. A polygon solid: the start inside it, else its own line trace's hit. (The view takes it
/// for the warning and the beam.)
pub fn trace_down(s: Shapes, x: Real, y: Real) -> Option<Real> {
    let mut best: Option<Real> = None;
    for q in s.polys {
        if q.inside(x, y) {
            return Some(y);
        }
        if let Some(h) = q.ray(x, y, x, y - 20000.0) {
            let top = crate::math::narrow(h.y);
            if best.is_none_or(|t| top > t) {
                best = Some(top);
            }
        }
    }
    for b in s.boxes {
        // the stage's boxes as the build's float geometry
        let (left, right, top, bottom) = (
            crate::math::narrow(b.left),
            crate::math::narrow(b.right),
            crate::math::narrow(b.top),
            crate::math::narrow(b.bottom),
        );
        if x < left || x > right {
            continue;
        }
        if bottom <= y && y <= top {
            return Some(y);
        }
        if top <= y && top >= y - 20000.0 && best.is_none_or(|t| top > t) {
            best = Some(top);
        }
    }
    best
}

/// The complex half of the line trace from (x, y) to (x, y - 20000): the highest crossing of the stage meshes' cut
/// triangles that block ProjectileGroundTrace (StageData.complexSegments), at or under the start; `None` without one.
// Deviation: a mesh with simple shapes is met by them alone (its triangles are not imported), and a triangle meets the
// trace from either side; PhysX's complex raycast against a mesh with both, and its back-face rule for a mesh query
// (no eMESH_BOTH_SIDES in the build's flags), were not read.
pub fn complex_down(segments: &[mvs_data::stage::ComplexSegment], x: Real, y: Real) -> Option<Real> {
    use mvs_math::js;
    let (x, y) = (f64::from(x), f64::from(y));
    let mut best: Option<f64> = None;
    for g in segments {
        if g.flags & 8 == 0 {
            continue;
        }
        // sim x = -UE Y, sim y = UE Z
        let (ax, ay, bx, by) = (-g.a[0], g.a[1], -g.b[0], g.b[1]);
        if x < js::min(ax, bx) || x > js::max(ax, bx) {
            continue;
        }
        let top = if ax == bx { js::min(js::max(ay, by), y) } else { ay + (by - ay) * (x - ax) / (bx - ax) };
        if top <= y && top >= y - 20000.0 && js::min(ay, by) <= y && best.is_none_or(|t| top > t) {
            best = Some(top);
        }
    }
    best.map(crate::math::narrow)
}

/// A fighter's root capsule against an upright box of the play plane [l, r] x [b, t] (as Morty's seismicBoom).
fn in_box(w: &World, v: FighterId, l: Real, r: Real, b: Real, t: Real) -> bool {
    let half = capsule_half(w, v) - cap_radius(w, v);
    let (vx, ay) = (w.fighter(v).body.x, actor_y(w, v));
    let dx = max(max(l - vx, 0.0), vx - r);
    let dy = max(max(b - (ay + half), 0.0), ay - half - t);
    size2d([dx, dy]) <= cap_radius(w, v)
}

/// Where the column's box, coming down, first meets the capsule (the start inside: the start).
fn meet_height(w: &World, v: FighterId, x: Real, top: Real) -> Real {
    let half = capsule_half(w, v) - cap_radius(w, v);
    let r = cap_radius(w, v);
    let gx = max((w.fighter(v).body.x - x).abs() - 50.0, 0.0);
    let h = actor_y(w, v) + half + max(r * r - gx * gx, 0.0).sqrt();
    min(h, top)
}

/// SeismicBoom @8950.
fn seismic_boom(w: &mut World, id: ProjectileId) {
    let me = ActorRef::Projectile(id);
    let inst = pr(w, id).owner.expect("a reactor has an owner");
    rx(w, id).spin = 0.0; // SetActorRotation(StartingRotation)
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    let mut boxes = w.blocker_shapes(BlockKind::Trace);
    boxes.boxes.insert(0, w.stage.ground);
    // the trace is complex (bTraceComplex @9113): the stage meshes' triangles too
    let hit_y = match (
        trace_down(boxes.view(), x, y),
        complex_down(w.data.stage.complex_segments.as_deref().unwrap_or(&[]), x, y),
    ) {
        (Some(a), Some(b)) => Some(max(a, b)),
        (a, b) => a.or(b),
    };
    let py = hit_y.unwrap_or(y - 20000.0);
    let inst_id = w.fighter(inst).id;
    w.events.push(Event::Sound {
        fighter: inst_id,
        ev: name!("sfx_cha_reindog_seismic_crystal_fire"),
        x: Some(x),
        y: Some(py),
        hit: None,
        pooled: false,
        key: None,
    });
    let live: Vec<FighterId> =
        match_fighters(w).into_iter().filter(|&v| crate::combat::phases::alive(w, v) && !no_collision(w, v)).collect();
    let mut column: Vec<FighterId> =
        live.iter().copied().filter(|&v| in_box(w, v, x - 50.0, x + 50.0, py, y)).collect();
    column.sort_by(|&a, &b| {
        (meet_height(w, b, x, y) - meet_height(w, a, x, y)).partial_cmp(&0.0).unwrap_or(Ordering::Equal)
    });
    // BoxOverlapActors at P + (0, 0, Blast Height / 2) @2718-3025, half extent (Blast Radius, Blast Radius, Blast
    // Height).
    let (br, bh) = (num_w(w, class_p(w, pr(w, id)), "Blast Radius"), num_w(w, class_p(w, pr(w, id)), "Blast Height"));
    let mid = py + (bh / 2.0);
    let mut targets = column.clone();
    if hit_y.is_some() {
        targets.extend(
            live.iter().copied().filter(|v| !column.contains(v) && in_box(w, *v, x - br, x + br, mid - bh, mid + bh)),
        );
    }
    for vic in targets {
        // Normal((loc - P) x (0, 25, 2), 0.0001) in UE's (Y, Z): Y = -sim x.
        let (vx, vy) = (w.fighter(vic).body.x, actor_y(w, vic));
        let uy = -(vx - x) * 25.0;
        let uz = (vy - py) * 2.0;
        let n = normal([0.0, uy, uz], KINDA_SMALL_NUMBER);
        let (ny, nz) = (n[1], n[2]);
        let ky = if (0.0..=0.1).contains(&ny) {
            0.1
        } else if (-0.1..=0.0).contains(&ny) {
            -0.1
        } else {
            ny
        };
        // IgnoreFacing: the vector's X is UE's Y, sim -x.
        let dir = V2 { x: -ky, y: nz };
        let p = boom_hit(ky, nz);
        let rows = boom_buffs();
        let attack_id = pr(w, id).attack_id;
        if w.fighter(vic).team.team != w.fighter(inst).team.team {
            if !crate::combat::phases::no_hit_by(w, inst, vic) && !projectile_blocks(w, &dealer(w, me), vic) {
                if invulnerable(w, vic) {
                    let tick = w.flow.tick;
                    attack_dodged(w, me, vic, attack_id, 1000.0, tick, None, false);
                } else {
                    dodge_over(w, me, vic);
                    // pfgDoDamage(null HitBox, ...) @6654: HitLocation is the victim's capsule (as the shockwave's).
                    let d = dealer(w, me);
                    let at = V2 { x: vx, y: vy };
                    if apply_projectile_hit(w, &d, vic, &p, &rows, at, Some(dir), &[], charge_pct(w, me)) {
                        actor_hit_enemy(w, &dealer(w, me), vic);
                    }
                }
            }
        } else if vic != inst {
            hit_buffs_call(w, inst, &rows);
            if !crate::combat::phases::no_hit(w, vic) && !invulnerable(w, vic) {
                let mult = charge_buff(w, me);
                hit_buffs_ally(w, inst, vic, &rows, mult);
                ally_hit(w, &dealer(w, me), vic, &p);
            }
        } else {
            hit_buffs_call(w, inst, &rows);
            // @6779-6898: the Instigator's own gauntlets.
            spawn_buff(w, inst, GAUNTLETS, 10.0, Some(0), Some(inst), true, None);
        }
        if pr(w, id).dead {
            return;
        }
    }
}

/// ReceiveBeginPlay: the parent's part is the world's spawn (`push_out_root`, the clamp of
/// ForceSpawnPositionInsideBlastBox: `parent_begin_play`); then Charge Up, and the sway's switch @13871-14384: the
/// owner (a fighter: BPI_BaseFighter) holds Buff_Creature_AirSupport.
// The spawn's AdjustIfPossibleButAlwaysSpawn (Comp_Spawner): FindTeleportSpot for the Capsule came ahead of
// BeginPlay (the Capsule ignores Platform, the stage's platforms and the ship pieces, and WorldDynamic: the stage
// block alone).
fn begin(w: &mut World, id: ProjectileId) {
    charge_up(w, id);
    let owner = pr(w, id).owner.expect("a reactor has an owner");
    if buff_of(w, owner, AIR_SUPPORT).is_some() {
        rx(w, id).sway = true;
        w.random_int(2.0); // SeismicStartDirection = RandomBool() @14364 (read by the dead oscillation branch alone)
    }
}

fn step(w: &mut World, id: ProjectileId) -> bool {
    let a = pr(w, id).age;
    // ReceiveTick @15514 (ahead of the actor's latent actions, AActor::Tick): the follow sway.
    if rd(w, id).sway {
        sway(w, id);
    }
    // CheckBlastBoxTimer (the timer pass of the frame before): set inside the actor's tick, it counts from the next
    // frame's timer tick on, as the parent's (`expiry`).
    let r = rd(w, id);
    if r.blast_from >= 0 && crate::consts::clock(crate::real(a - 1 - r.blast_from)) > r.blast_next {
        rx(w, id).blast_next += f64::from(0.1_f32);
        let p = pr(w, id);
        if check_blast_box_exit(p.x, p.y, &p.blast_bounds) {
            return true;
        }
    }
    // RotatingMovement: RotationRate x DeltaTime added to the yaw, in float32.
    let r = rx(w, id);
    r.spin += r.rate * DELTA_SECONDS;
    if r.spin >= 360.0 {
        r.spin -= 360.0;
    }
    let age = crate::real(a);
    if age == at_begin() {
        // @1714, then @1575 (the Delays in the order they were made).
        let g = num_w(w, class_p(w, pr(w, id)), "GravityScale");
        let p = pm(w, id);
        p.vx = 0.0;
        p.vy = 0.0;
        crate::actors::projectile::set_gravity_scale(p, g);
        // DestroyOnExitedArena? = true (@1688), CheckForBlastBox (@1699; SpawnedActorParent @24026 = goto 9247, the
        // BeginPlay blast-box loop run again: the bounds merged, ForceSpawnPositionInsideBlastBox's clamp).
        let sb = spawn_box(w);
        let force = truthy(class_p(w, pr(w, id)).get("ForceSpawnPositionInsideBlastBox"));
        let Projectile { x, y, prev_x, prev_y, blast_bounds, .. } = pm(w, id);
        if blast_box_loop(&sb, x, y, Some((prev_x, prev_y)), force, blast_bounds, true) {
            return true;
        }
        rx(w, id).blast_from = a;
    }
    let c = crate::real(a - rd(w, id).charge_at);
    if c == at_warn() {
        rx(w, id).rate = 900.0; // WarningVisualsMulticast (the view's)
    } else if c == at_fast() {
        rx(w, id).rate = 2000.0;
    } else if c == at_boom() {
        let g = num_w(w, class_p(w, pr(w, id)), "GravityScale");
        crate::actors::projectile::set_gravity_scale(pm(w, id), g);
        seismic_boom(w, id);
    } else if c == at_cool() {
        rx(w, id).rate = 0.0; // Cooldown Visuals, Delay(2.0), rate 0
    } else if c == at_next() {
        charge_up(w, id);
    }
    false
}

fn activate(w: &mut World, id: ProjectileId) {
    if !pr(w, id).dead {
        destroy_projectile(w, id, false);
    }
}

fn damage_box_hook(w: &World, id: ProjectileId) -> Option<WorldSphere> {
    damage_box(w, id)
}

/// ReceiveAnyDamage @12816: HP -= the damage; at <= 0 sfx_cha_reindog_generic_hit at the DamageBox and
/// K2_DestroyActor.
fn any_damage(w: &mut World, id: ProjectileId, dmg: Real, _by: ActorRef) -> bool {
    let r = rx(w, id);
    r.hp -= dmg;
    if r.hp > 0.0 {
        return false;
    }
    let p = pr(w, id);
    let e = Event::Sound {
        fighter: p.owner_id,
        ev: name!("sfx_cha_reindog_generic_hit"),
        x: Some(p.x),
        y: Some(p.y),
        hit: None,
        pooled: false,
        key: None,
    };
    w.events.push(e);
    true
}

/// ReceiveDestroyed @13066 (DestroyVisuals, once): sfx_cha_reindog_seismic_crystal_end at the DamageBox.
fn destroyed(w: &mut World, id: ProjectileId) {
    if rd(w, id).end_shown {
        return;
    }
    rx(w, id).end_shown = true;
    let p = pr(w, id);
    let e = Event::Sound {
        fighter: p.owner_id,
        ev: name!("sfx_cha_reindog_seismic_crystal_end"),
        x: Some(p.x),
        y: Some(p.y),
        hit: None,
        pooled: false,
        key: None,
    };
    w.events.push(e);
}

fn kind() -> ProjectileHooks {
    ProjectileHooks {
        construct: Some(construct),
        bodies: Some(damage_box_body),
        begin: Some(begin),
        step: Some(step),
        activate: Some(activate),
        damage_box: Some(damage_box_hook),
        any_damage: Some(any_damage),
        destroyed: Some(destroyed),
        ..ProjectileHooks::default()
    }
}

/// This file's registry entries (the reactor's kind).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        REACTOR,
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::SeismicReactor), ActorHooks::Projectile(kind())),
    )
}

#[cfg(test)]
mod tests {
    use mvs_data::stage::ComplexSegment;

    fn seg(a: [f64; 2], b: [f64; 2], flags: i32) -> ComplexSegment {
        ComplexSegment { name: String::new(), a, b, flags }
    }

    #[test]
    fn complex_down_takes_the_highest_crossing_under_the_start() {
        // a ramp from sim x -100 (UE Y 100) at 0 up to sim x 100 at 100, a floor at 20, and one that blocks no trace
        let s = [
            seg([100.0, 0.0], [-100.0, 100.0], 9),
            seg([200.0, 20.0], [-200.0, 20.0], 8),
            seg([200.0, 80.0], [-200.0, 80.0], 2),
        ];
        assert_eq!(super::complex_down(&s, 0.0, 500.0), Some(50.0));
        assert_eq!(super::complex_down(&s, -90.0, 500.0), Some(20.0));
        // the start under the ramp: the floor
        assert_eq!(super::complex_down(&s, 0.0, 40.0), Some(20.0));
        assert_eq!(super::complex_down(&s, 300.0, 500.0), None);
    }
}
