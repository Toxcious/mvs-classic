//! JerryRocket: the rocket Tom rides ([`JerryRocket`], a
//! ParentProjectilePhysics child): the stick steering in RocketAir / RocketGround (300 deg/s), BPI_SetControlBool
//! (pawn.rs), its self-hit on the owner, its area hit, the TempInvincibility Jerry at its end (jerry.rs spawn_jerry);
//! and Jerry's LaunchRocket (Jerry_Rocket_Montage).
//!
//! Class data (tomandjerry.json actors.JerryRocket_C): ControlSpeed 300, Aceleration 3000, StartAimVector (0, 1, 0),
//! InitialLifeSpan 2 (no MaxLifetime), DestroyOnBlocked, every DestroyOnHit*, DestroyOnExitedArena?, CanBeReflected,
//! Supports2DHitboxes, HitBoxOn?; the HitBox sphere 70 (bGenerateOverlapEvents false in the template), HitBox2D radius
//! 70; HitBoxDamage 14 / 1400 / 17 (1, 1), VelocityInsteadOfFacing; TomSelfHitDamage 0 / 1800 / 0 (0, 1), IgnoreFacing,
//! ForceKnockback, HitPauseTime 0.05, HitStunMult 0.
//
// Unverified: the stick is the Instigator's input of this frame (`controls.inp`), as chars/garnet/gauntlets steer.
// Unverified: the engine timer of InitialLifeSpan: the exact pass is not measured for this actor (counted as the
// parent's MaxLifetime timer is).
// Deviation: ReceiveActorBeginOverlap is the begin of an overlap of any of the actor's components with any of the
// owner's: the port tests the HitBox shapes against the owner's hurt shapes alone (his melee hit boxes against the
// rocket's HurtBox are left out), in the rocket's step behind its move; the dealing shape's overlap events as
// [`overlaps_live`] has them (HitBox2D's from the spawn, HitBox's from the 0.1 s Delay).
// Unverified: HitBox2D's template collision is not in the class data; its overlap events on from the spawn rest on the
// recorded spawn-tick hit.
// Deviation: `refused_ally_hit` takes the owner as the dealer (his dealt mods, his charge multipliers, the taken
// conditions' Attacker); the build's dealer is the rocket's handler (passed for the taken pass's self test alone). An
// invulnerable owner (pfgDoDamage's dodge branch @7381) takes no hit here and its dodge side is not ported.
// Unverified (native): GetComponentByClass(SkeletalMeshComponent) is taken as the main mesh (Tom's anim instance).
// Unverified: the montage starts as a graph's play does (`start_montage`, begin_attack false: chars/creature/pawn.rs).
// Deviation: the order of the actors of the area hit is the port's trace order, not PhysX's.

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_p, hit_shapes};
use crate::actors::world_actor::{actor_cls, actor_owner, actor_place};
use crate::actors::{ActorClass, ActorId, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::HitPacket;
use crate::combat::elem::end_actor_children;
use crate::combat::hits::{
    actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks, reflecting_projectiles, refused_ally_hit,
};
use crate::combat::phases::{give_vbuf, victim_pose};
use crate::combat::spawned_hits::{
    can_be_hit_by_d, comp, damage_actor, damage_spawned_actor, projectile_hits_projectile, world_hooks,
};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, SoundKey};
use crate::fighter::anim::MontageSlot;
use crate::fighter::anim::grouped::{active_instance, stop_grouped};
use crate::fighter::anim::montages::{montage_is_active, slot_name, start_montage, stop_dash_montage, stop_montage};
use crate::fighter::common::{actor_class_def, latent_frames, lookup_owner, owner_team, warn_text};
use crate::fighter::moves::V2;
use crate::fighter::traces::{OBJ_HURT_BOX, OBJ_PAWN, OBJ_PHYSICS_BODY, sphere_overlap_actors};
use crate::fighter::{FighterId, actor_y, invulnerable};
use crate::math::ue::{SMALL_NUMBER, deg_atan2, normal, rotate_angle_axis, safe_normal};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks, ReflectHooks};
use crate::registry::props::{num_or, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;
use crate::world::stage_actors::can_be_hit_by;
use mvs_anim::montage::PROXY_NONE;

use super::jerry::{play_montage, spawn_jerry};
use super::pawn::{set_aim_direction, set_control_bool, set_is_jerry_attached};
use super::state::tj_of;

/// The rocket's class.
pub const ROCKET: &str = "JerryRocket_C";

const ROCKET_AIR: Name = name!("Tom_and_Jerry_Rocket_Air_Montage");
const ROCKET_GROUND: Name = name!("Tom_and_Jerry_Rocket_Ground_Montage");
const NO_JERRY_HIT: Name = name!("Tom_and_Jerry_Rocket_NoJerry_Hit_Montage");
const JERRY_ROCKET: Name = name!("Tom_and_Jerry_Jerry_Rocket_Montage");
const LOOP: Name = name!("sfx_cha_tomandjerry_rocket_loop");
const LOOP_STOP: Name = name!("sfx_cha_tomandjerry_rocket_loop_stop");

/// The BeginPlay Delays (0.4 s -> @15 AcelerationGate, 1.0 s -> @491 HitSelfGate, 0.1 s -> @503
/// HitBox.SetGenerateOverlapEvents(true)): the age they resume at.
fn at_accel() -> Real {
    latent_frames(0.4)
}
fn at_self() -> Real {
    latent_frames(1.0)
}
fn at_overlap() -> Real {
    latent_frames(0.1)
}

/// `JerryRocket extends Projectile` (JerryRocket_C): the actor's variables.
#[derive(State, Debug, Default)]
pub struct JerryRocket {
    /// SpawnOwner (BeginPlay: GetOwner(); also the Instigator).
    pub spawn_owner: Option<FighterId>,
    /// ShouldSpawnJerry (the owner's BPI_GetFighterCharacter == 15).
    pub should_spawn_jerry: bool,
    pub jerry_spawned: bool,
    /// AcelerationGate.
    pub accel_gate: bool,
    pub hit_self_gate: bool,
    /// HitBox's GenerateOverlapEvents.
    pub overlap_on: bool,
    /// The HitBox over the owner at the last test (a begin overlap is the edge).
    pub owner_over: bool,
    /// The DoOnce @557.
    pub self_hit_done: bool,
    pub has_been_reflected: bool,
    /// HitActor (HitEnemyPlayer).
    pub hit_actor: Option<ActorRef>,
    /// BlockingActor (Blocked).
    pub blocking_actor: Option<ActorRef>,
    /// The age the InitialLifeSpan timer ends it at.
    pub end_age: i32,
    /// ReceiveEndPlay ran (destroyed and end both reach it).
    pub end_played: bool,
    /// HitBoxDamage as the actor's variable holds it: Reflected's override @10876-10940 writes it after the parent's
    /// UpdateHitBoxDamage, so the hit boxes keep the packet they were given (`Projectile::hit`). The end's area hit,
    /// the clank and a later reflect's scaling read this one.
    pub box_damage: HitPacket,
}

/// The rocket's part of `hitbox_active`: its hit boxes begin overlaps only while this holds (the template's HitBox
/// bGenerateOverlapEvents, turned on by its graph; HitBox2D's from the spawn). Called by
/// `actors::projectile::hitbox_active`.
pub fn overlaps_live(p: &Projectile, s: &JerryRocket) -> bool {
    !p.sphere_3d || (s.overlap_on && crate::real(p.age) > at_overlap())
}

/// The rocket's packet in a clank (its HitBoxDamage variable, `boxDamage`). Called by `combat::spawned_hits`.
pub fn clank_hit(s: &JerryRocket) -> &HitPacket {
    &s.box_damage
}

/// `launchRocket` (ActivateObject's condition 2: Jerry_Rocket_Montage). Called by jerry.rs.
pub fn launch_rocket(w: &mut World, j: ActorId) {
    play_montage(w, j, JERRY_ROCKET);
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn jr(w: &World, id: ProjectileId) -> &JerryRocket {
    match &pr(w, id).ext {
        ProjectileExt::JerryRocket(r) => r,
        e => panic!("a JerryRocket_C without its state: {e:?}"),
    }
}

fn jx(w: &mut World, id: ProjectileId) -> &mut JerryRocket {
    match &mut pm(w, id).ext {
        ProjectileExt::JerryRocket(r) => r,
        e => panic!("a JerryRocket_C without its state: {e:?}"),
    }
}

fn owner(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("a JerryRocket_C has an owner")
}

fn loop_key(p: &Projectile) -> SoundKey {
    SoundKey { tag: name!("jerryrocket"), n: p.seq }
}

fn sound(w: &mut World, id: ProjectileId, ev: Name) {
    let fighter = w.fighter(owner(w, id)).id;
    let p = pr(w, id);
    let e = Event::Sound { fighter, ev, x: Some(p.x), y: Some(p.y), hit: None, pooled: false, key: None };
    w.events.push(e);
}

fn sound_stop_loop(w: &mut World, id: ProjectileId) {
    let fighter = w.fighter(owner(w, id)).id;
    let key = loop_key(pr(w, id));
    w.events.push(Event::SoundStop { fighter, key, fade: None, ev: Some(LOOP_STOP), curve: None, cut: false });
}

// ---------------------------------------------------------------- the math

/// VSize of (0, Y, Z): sqrtss of the float sum.
fn vsize(x: Real, y: Real) -> Real {
    ((x * x) + (y * y)).sqrt()
}

fn clamp(v: Real, lo: Real, hi: Real) -> Real {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// UKismetMathLibrary NormalizeToRange (4.26): equal ends give 0 below, 1 at or above; reversed ends are swapped.
fn normalize_to_range(v: Real, a: Real, b: Real) -> Real {
    if a == b {
        return if v < a { 0.0 } else { 1.0 };
    }
    let (lo, hi) = (min(a, b), max(a, b));
    (v - lo) / (hi - lo)
}

/// Normal(V, 1e-4) x (|V| + add) (the Normal node: FVector::GetSafeNormal by InvSqrt).
fn grow(vx: Real, vy: Real, add: Real) -> (Real, Real) {
    let [_, ny, nz] = normal([0.0, -vx, vy], 1e-4);
    let s = vsize(vx, vy) + add;
    (-(ny * s), nz * s)
}

// ---------------------------------------------------------------- the montage

/// Montage_Stop(blend, GetCurrentActiveMontage()) on the fighter's anim instance: the active instance, whichever its
/// group.
fn stop_active(w: &mut World, f: FighterId, blend: Real) {
    let Some(slot) = active_instance(w, f) else { return };
    match slot {
        MontageSlot::Main => stop_montage(w, f, Some(f64::from(blend)), None),
        MontageSlot::Dash => stop_dash_montage(w, f, f64::from(blend)),
        other => {
            let n = slot_name(w, f, other);
            stop_grouped(w, f, Some(n), blend);
        }
    }
}

/// StopMontage @27-474: SpawnOwner's main mesh's anim instance, Montage_IsPlaying(Rocket_Air) or (Rocket_Ground) ->
/// Montage_Stop(0.0, GetCurrentActiveMontage()).
fn stop_rocket_montage(w: &mut World, f: Option<FighterId>) {
    let Some(f) = f else { return };
    if montage_is_active(w, f, ROCKET_AIR) || montage_is_active(w, f, ROCKET_GROUND) {
        stop_active(w, f, 0.0);
    }
}

// ---------------------------------------------------------------- the tick

/// SetVelocityMulticast @8696-9950: Velocity, then on SpawnOwner's AnimBP (a BPI_AnimBP: Tom_and_Jerry_AnimBP)
/// BPI_SetAimDirection(Lerp(0, NormalizeToRange(DegAtan2(dir.Z, |dir.Y|), start - 50, start + 90) x 2 - 1,
/// FClamp(NormalizeToRange(|Velocity|, 0, 1000), 0, 1))), start = DegAtan2(StartAimVector.Z, .Y) and dir the unit
/// vector from SpawnOwner's location to the rocket's; then BPI_SetControlBool(ControlBool).
fn set_velocity(w: &mut World, id: ProjectileId, vx: Real, vy: Real, control: bool) {
    {
        let p = pm(w, id);
        p.vx = vx;
        p.vy = vy;
    }
    let Some(f) = jr(w, id).spawn_owner else { return };
    if tj_of(w, f).is_none() {
        return;
    }
    let w8 = clamp(normalize_to_range(vsize(vx, vy), 0.0, 1000.0), 0.0, 1.0);
    let (sz, sy) = match obj(class_p(w, pr(w, id)), "StartAimVector") {
        Some(s) => (num_or(s, "Z", 0.0), num_or(s, "Y", 0.0)),
        None => (0.0, 1.0),
    };
    let start = deg_atan2(sz, sy);
    // GetDirectionUnitVector(From, To): (To - From).GetSafeNormal() (UE Y = -x, Z = y).
    let (px, py) = (pr(w, id).x, pr(w, id).y);
    let [_, dy, dz] = safe_normal([0.0, -(px - w.fighter(f).body.x), py - actor_y(w, f)], SMALL_NUMBER);
    let n = normalize_to_range(deg_atan2(dz, dy.abs()), start - 50.0, start + 90.0);
    let aim = w8 * ((n * 2.0) - 1.0); // Lerp(0, B, A) = 0 + A x (B - 0)
    set_aim_direction(w, f, aim);
    set_control_bool(w, f, control);
}

/// ReceiveTick @7145 -> @1727 (with authority and !HasBeenReflected; the parent's tick first). GetOwner() a
/// FighterCharacter whose GetCurrentMontage is Rocket_Air or Rocket_Ground: the Instigator's BPI_GetMoveAxisValues
/// (0, 0 when the cast fails) steer the velocity @2264-4153; else the speed alone grows @4186-4454. In UE (Y, Z) with
/// Y = -x, Z = y: thetaV = DegAtan2(-V.Y, -V.Z) + 180, thetaIn = DegAtan2(MoveX, -MoveY) + 180, d = thetaV - thetaIn;
/// of d and d - 360 the one of the strictly smaller |.| (else d - 360), of it and d + 360 likewise (else d + 360);
/// step = ControlSpeed x DeltaTime. d == 0 or no stick keeps V; step > |d| snaps to (0, -MoveX, MoveY) x |V| (not
/// normalised); else V turned about +X by (d < 0 ? -step : step). Then Normal(S, 1e-4) x (|S| + DeltaTime x
/// (AcelerationGate ? 3000 : 0)), SetVelocityMulticast(it, d >= 0).
fn tick(w: &mut World, id: ProjectileId) {
    if jr(w, id).has_been_reflected {
        return;
    }
    let f = owner(w, id);
    let cp = class_p(w, pr(w, id));
    let add = DELTA_SECONDS * if jr(w, id).accel_gate { num_or(cp, "Aceleration", 0.0) } else { 0.0 };
    let control_speed = num_or(cp, "ControlSpeed", 0.0);
    let (vx, vy) = (pr(w, id).vx, pr(w, id).vy);
    let cur = active_instance(w, f).map(|s| slot_name(w, f, s));
    if cur != Some(ROCKET_AIR) && cur != Some(ROCKET_GROUND) {
        let (ox, oy) = grow(vx, vy, add);
        set_velocity(w, id, ox, oy, false);
        return;
    }
    let inst = jr(w, id).spawn_owner; // GetInstigator(): the spawning pawn
    let (mx, my) = match inst {
        Some(i) => {
            let inp = &w.fighter(i).controls.inp;
            (inp.x, inp.y)
        }
        None => (0.0, 0.0),
    };
    let size = vsize(vx, vy);
    let theta_v = deg_atan2(vx, -vy) + 180.0;
    let theta_in = deg_atan2(mx, -my) + 180.0;
    let d = theta_v - theta_in;
    let (dm, dp) = ((d - 360.0), (d + 360.0));
    let s1 = if d.abs() < dm.abs() { d } else { dm };
    let sel = if s1.abs() < dp.abs() { s1 } else { dp };
    let step = control_speed * DELTA_SECONDS;
    let (sx, sy);
    if sel == 0.0 || (mx == 0.0 && my == 0.0) {
        sx = vx;
        sy = vy;
    } else if step > sel.abs() {
        sx = mx * size; // UE Y = -MoveX x |V|
        sy = my * size;
    } else {
        let deg = if sel < 0.0 { -step } else { step };
        // RotateAngleAxis(V, deg, (1, 0, 0)): FMath::SinCos
        let [_, y2, z2] = rotate_angle_axis([0.0, -vx, vy], deg, [1.0, 0.0, 0.0]);
        sx = -y2;
        sy = z2;
    }
    let (ox, oy) = grow(sx, sy, add);
    set_velocity(w, id, ox, oy, sel >= 0.0);
}

/// The HitBox (sphere 70; HitBox2D's circle has the same radius) over the owner's hurt shapes, the test the boxes make
/// on an enemy: BaseFighter's construction sets the root capsule to Ignore HitBox when the pawn has HurtBox capsules,
/// so the begin overlap with the owner comes from those.
fn over_owner(w: &mut World, id: ProjectileId, f: FighterId) -> bool {
    if w.fighter(f).life.dead > 0.0 {
        return false;
    }
    let Some(vp) = victim_pose(w, f) else { return false };
    let mut shapes = Vec::new();
    hit_shapes(w, pr(w, id), &mut shapes);
    let met = crate::combat::hit::hurt_hit(w, f, &shapes, &vp).is_some();
    give_vbuf(w, vp);
    met
}

/// ReceiveActorBeginOverlap @6816: OtherActor == GetOwner(), !HasBeenReflected, not the owner's handler
/// IsReflectingProjectiles, HitSelfGate -> the DoOnce @557: the rocket's handler's pfgDoDamage(null, the owner,
/// TomSelfHitDamage) @603, LaunchTomMulticast @726 (@8204: CreateProxyObjectForPlayMontage(the owner's
/// GetComponentByClass(SkeletalMeshComponent), Rocket_NoJerry_Hit, 1, 0); its five callbacks only store the name),
/// K2_DestroyActor @740. True: destroyed. pfgDoDamage on the owner: the same team, ForceKnockback (`refused_ally_hit`):
/// CanBeHitBy false, the knockback path; NotifyAllyOfHit needs Victim != Instigator, so no ally signal.
fn self_overlap(w: &mut World, id: ProjectileId) -> bool {
    let f = owner(w, id);
    if !overlaps_live(pr(w, id), jr(w, id)) {
        return false;
    }
    let over = over_owner(w, id, f);
    let begin = over && !jr(w, id).owner_over;
    jx(w, id).owner_over = over;
    let s = jr(w, id);
    if !begin || s.has_been_reflected || reflecting_projectiles(w, f) || !s.hit_self_gate || s.self_hit_done {
        return false;
    }
    jx(w, id).self_hit_done = true;
    if !invulnerable(w, f) {
        let empty = mvs_data::value::Props::new();
        let packet =
            HitPacket::from_props(&w.names, obj(class_p(w, pr(w, id)), "TomSelfHitDamage").unwrap_or(&empty), None);
        refused_ally_hit(w, f, f, &packet, false, ActorRef::Projectile(id)); // the rocket's handler deals
    }
    start_montage(w, f, NO_JERRY_HIT, &PROXY_NONE, None);
    w.fighter_mut(f).charge.begin_attack = false;
    true
}

// ---------------------------------------------------------------- the end

/// ReceiveEndPlay @7224: the parent's (SpawnedActorParent EndPlay: its buff children), StopMontage, BoomVisuals @7545
/// (PS_TomJerryRocket_Explosion at the actor, scale 0.5, Team; sfx_cha_tomandjerry_rocket_hit at the location;
/// sfx_cha_tomandjerry_rocket_loop_stop on the actor), with authority SpawnJerry @10499 (!JerrySpawned and
/// ShouldSpawnJerry: JerrySpawned, SpawnOwner's Comp_Spawner spawns Jerry_SpawnedActor_TempInvincibility at the
/// rocket, direction (0, 1, 1), speeds 0, AttackID -1337), then the area hit @767-1550. Comp_Spawner's
/// SetInitialMovement with InitialSpeed 0 and MaxSpeed 0 gives Jerry no velocity and no MaxSpeed clamp (spawnJerry's
/// maxSpeed 0), whatever the class's own MaxSpeed: LimitVelocity clamps only for a MaxSpeed above 0.
fn end_play(w: &mut World, id: ProjectileId) {
    if jr(w, id).end_played {
        return;
    }
    jx(w, id).end_played = true;
    if reset_pass(w) {
        sound_stop_loop(w, id);
        return;
    }
    let me = ActorRef::Projectile(id);
    end_actor_children(w, me);
    let so = jr(w, id).spawn_owner;
    stop_rocket_montage(w, so);
    let (cls, fighter, x, y) = {
        let p = pr(w, id);
        (p.cls, w.fighter(owner(w, id)).id, p.x, p.y)
    };
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter,
        vfx: Some(name!("PS_TomJerryRocket_Explosion")),
        x,
        y,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: 0.5,
        tint: None,
        to: None,
    });
    sound(w, id, name!("sfx_cha_tomandjerry_rocket_hit"));
    sound_stop_loop(w, id);
    if !jr(w, id).jerry_spawned
        && jr(w, id).should_spawn_jerry
        && let Some(so) = so
    {
        jx(w, id).jerry_spawned = true;
        spawn_jerry(w, so, x, y, 0.0, 0.0, true, 0.0);
    }
    area_hit(w, id);
}

/// The area hit @767-1550: SphereOverlapActors(the actor's location, 200, [PhysicsBody, Pawn, HurtBox], no class,
/// ignoring BlockingActor and HitActor); each actor with a Comp_Actor_Destructible, while GetOwner() (the current
/// owner) has one, whose IsVulnerableTo(the owner's TeamIndex) holds (!IsInvulnerable?, !DamageDisabled, another
/// team): the rocket's handler pfgDoDamage(null, it, HitBoxDamage) @1357, its success not read. The handler's
/// pfgDoDamage on a fighter as Bugs' rocket burst: a block, the hit and its HitEnemyPlayer. A projectile takes the
/// projectile branch (no clank), another spawned actor DamageAndLaunch; an actor of World.actors `damage_actor`.
/// The hit boxes' packet stays the one they were given; the area hit deals the variable (`box_damage`), which differs
/// only after a reflect (Reflected's override writes it alone): `hit` is the variable for the loop and the boxes'
/// again behind it (a contact hit still pending in the frame deals the boxes').
fn area_hit(w: &mut World, id: ProjectileId) {
    let boxes = pr(w, id).hit.clone();
    let var = jr(w, id).box_damage.clone();
    pm(w, id).hit = var;
    area_loop(w, id);
    pm(w, id).hit = boxes;
}

fn area_loop(w: &mut World, id: ProjectileId) {
    let me = ActorRef::Projectile(id);
    let f = owner(w, id);
    let team = w.fighter(f).team.team;
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    let ignore = [jr(w, id).blocking_actor, jr(w, id).hit_actor, Some(me)];
    let types = [crate::real(OBJ_PHYSICS_BODY), crate::real(OBJ_PAWN), crate::real(OBJ_HURT_BOX)];
    let found = sphere_overlap_actors(w, x, y, 200.0, &types, &ignore, -1.0);
    for o in found {
        if o == me {
            continue;
        }
        let packet = pr(w, id).hit.clone();
        match o {
            ActorRef::Fighter(vic) => {
                // IsVulnerableTo: !IsInvulnerable?, !DamageDisabled, another team (FriendlyFire false)
                if invulnerable(w, vic) || w.fighter(vic).invuln.damage_disabled || w.fighter(vic).team.team == team {
                    continue;
                }
                if w.fighter(vic).life.dead > 0.0 || crate::combat::phases::no_hit_by(w, f, vic) {
                    continue;
                }
                if projectile_blocks(w, &dealer(w, me), vic) {
                    continue;
                }
                let d = dealer(w, me);
                if apply_projectile_hit(
                    w,
                    &d,
                    vic,
                    &packet,
                    &[],
                    V2 { x, y },
                    None,
                    &[],
                    crate::combat::elem::charge_pct(w, me),
                ) {
                    actor_hit_enemy(w, &d, vic);
                }
            }
            ActorRef::Projectile(b) => {
                if pr_dead(w, b) {
                    continue;
                }
                let own = lookup_owner(w, o);
                let c = actor_class_def(w, own, w.actors.projectiles.get(b).expect("a live projectile").cls)
                    .and_then(|d| obj(&d.components, "Comp_Actor_Destructible"));
                let Some(c) = c else { continue };
                if !can_be_hit_by(team, owner_team(w, o), can_be_hit_by_d(Some(c))) {
                    continue;
                }
                if crate::combat::hits::truthy(c, "IsProjectile") {
                    projectile_hits_projectile(w, id, b, false);
                } else {
                    damage_spawned_actor(w, me, b, &packet);
                }
            }
            ActorRef::Actor(a) => {
                let Some(wa) = w.actors.actors.get(a) else { continue };
                if wa.dead {
                    continue;
                }
                let (cls, own) = (wa.cls, wa.owner);
                let hooked = world_hooks(w, cls).and_then(|k| k.destructible).and_then(|f| f(w, a));
                let c = hooked.or_else(|| comp(w, o, "Comp_Actor_Destructible"));
                let Some(c) = c else { continue };
                if !can_be_hit_by(team, w.fighter(own).team.team, can_be_hit_by_d(Some(c))) {
                    continue;
                }
                if crate::combat::hits::truthy(c, "IsProjectile") {
                    let cn = w.names.str(cls);
                    warn_text(
                        w,
                        &format!(
                            "{ROCKET}: the end's area hit on {cn} (an IsProjectile actor of World.actors) is not ported"
                        ),
                    );
                    continue;
                }
                damage_actor(w, me, a, &packet, None);
            }
            other => {
                if actor_owner(w, other).is_some_and(|t| w.fighter(t).team.team == team) {
                    continue;
                }
                let _ = (actor_cls, actor_place);
                warn_text(
                    w,
                    &format!(
                        "{ROCKET}: the end's area hit met an actor that is no fighter, projectile or World.actors entry (not hit)"
                    ),
                );
            }
        }
    }
}

fn pr_dead(w: &World, b: ProjectileId) -> bool {
    w.actors.projectiles.get(b).is_none_or(|p| p.dead)
}

// ---------------------------------------------------------------- the kind

/// The constructor's tail (`make`): the InitialLifeSpan timer's age, and HitBoxDamage's variable the box's
/// packet. AActor InitialLifeSpan: a timer from the spawn whose expiry destroys the actor, counted as the parent's
/// MaxLifetime timer is: the first pass whose time is above the span, and the pass after.
fn construct(w: &World, p: &mut Projectile) {
    let life = num_or(class_p(w, p), "InitialLifeSpan", 0.0);
    let mut k = 0;
    if life > 0.0 {
        #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "!(k * dt > life)")]
        while !(crate::consts::clock(crate::real(k)) > f64::from(life)) {
            k += 1;
        }
    }
    p.ext = ProjectileExt::JerryRocket(JerryRocket {
        end_age: if life > 0.0 { k + 1 } else { 0 },
        box_damage: p.hit.clone(),
        ..JerryRocket::default()
    });
}

/// ReceiveBeginPlay @5720 (SpawnedActorParent's BeginPlay, not ParentProjectilePhysics'): SpawnOwner = GetOwner(),
/// ShouldSpawnJerry = BPI_GetFighterCharacter == 15 (Pawn_TomAndJerry), the Instigator's BPI_SetIsJerryAttached(false),
/// the three Delays, the launch and loop sounds on the actor.
fn begin(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    jx(w, id).spawn_owner = Some(f);
    let tj = tj_of(w, f).is_some();
    jx(w, id).should_spawn_jerry = tj;
    if tj {
        set_is_jerry_attached(w, f, false);
    }
    sound(w, id, name!("sfx_cha_tomandjerry_rocket_launch"));
    let (fighter, key) = (w.fighter(f).id, loop_key(pr(w, id)));
    w.events.push(Event::SoundStart { fighter, ev: LOOP, key });
}

fn step(w: &mut World, id: ProjectileId) -> bool {
    // the begin overlap inside this frame's move, then ReceiveTick, then the latent actions and the life span
    if self_overlap(w, id) {
        return true;
    }
    tick(w, id);
    let a = crate::real(pr(w, id).age);
    if a == at_accel() {
        jx(w, id).accel_gate = true;
    }
    if a == at_self() {
        jx(w, id).hit_self_gate = true;
    }
    if a == at_overlap() {
        jx(w, id).overlap_on = true;
    }
    let end = jr(w, id).end_age;
    end > 0 && pr(w, id).age >= end
}

/// HitEnemyPlayer @10790: HitActor, then the parent's.
fn hit_enemy(w: &mut World, id: ProjectileId, v: FighterId) {
    jx(w, id).hit_actor = Some(ActorRef::Fighter(v));
}

/// Blocked @10766 -> @537: BlockingActor, then SpawnedActorParent's (DestroyOnBlocked).
fn blocked(w: &mut World, id: ProjectileId, by: ActorRef) {
    jx(w, id).blocking_actor = Some(by);
    if crate::registry::props::flag(class_p(w, pr(w, id)), "DestroyOnBlocked") {
        pm(w, id).blocked_destroy = true;
    }
}

fn destroyed(w: &mut World, id: ProjectileId) {
    end_play(w, id);
}

fn end(w: &mut World, id: ProjectileId) {
    end_play(w, id);
}

/// BPI_Reflected scales the variable and UpdateHitBoxDamage gives the boxes the scaled copy (@4749-4931).
fn hit_box_damage(w: &mut World, a: ActorRef) -> &mut HitPacket {
    let ActorRef::Projectile(id) = a else { panic!("a JerryRocket_C reflect hook on {a:?}") };
    &mut jx(w, id).box_damage
}

/// Reflected @10866 (after the parent's BPI_Reflected scaling and UpdateHitBoxDamage): HitBoxDamage's Damage 18,
/// BaseKnockBack 1650, KnockBackScaling 18 (the variable alone: `box_damage`).
fn reflected(w: &mut World, a: ActorRef) {
    let ActorRef::Projectile(id) = a else { return };
    let mut b = pr(w, id).hit.clone();
    b.damage = Some(18.0);
    b.base_knock_back = Some(1650.0);
    b.knock_back_scaling = Some(18.0);
    jx(w, id).box_damage = b;
}

/// MulticastReflect @10592: the parent's, HasBeenReflected; GetOwner() (the new owner) other than SpawnOwner stops
/// SpawnOwner's rocket montage (StopMontage).
fn multicast(w: &mut World, a: ActorRef, by: FighterId, pause: bool) {
    let _ = (by, pause);
    let ActorRef::Projectile(id) = a else { return };
    jx(w, id).has_been_reflected = true;
    let so = jr(w, id).spawn_owner;
    if pr(w, id).owner != so {
        stop_rocket_montage(w, so);
    }
}

/// This file's registry entries (JerryRocket_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let kind = ProjectileHooks {
        construct: Some(construct),
        begin: Some(begin),
        step: Some(step),
        hit_enemy: Some(hit_enemy),
        blocked: Some(blocked),
        destroyed: Some(destroyed),
        end: Some(end),
        reflect: ReflectHooks {
            hit_box_damage: Some(hit_box_damage),
            reflected: Some(reflected),
            multicast: Some(multicast),
            ..ReflectHooks::default()
        },
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        ROCKET,
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::JerryRocket), ActorHooks::Projectile(kind)),
    )?;
    Ok(())
}
