//! The heal and the Watermelon: HealGrenade_C ([`HealGrenade`], a
//! projectile), HealOverTimeZoneBP_C ([`HealZone`], a World.actors class with a place), the Watermelon minion's spawn
//! and setup, Buff_Steven_GreenThumbDamageBoost ([`GreenThumbBoost`]) and the joined hooks (reset, timerTick,
//! playerKnockedOut, minionDefeated).
//!
//! HealGrenade: Steven_Special_SpitHeal_Montage's Spawner (t 0.76, socket L_Hand, InitialMovementDirection (0, 1, 0), no
//! InitialSpeed). The body is Batman's SmokeGrenade's (`actors/smoke_bomb.rs`, whose notes hold the reads): the
//! Capsule with the Box welded into it, stepped in the kind's `physics`, the launch (half the owner's velocity) a
//! velocity change of its first step. ReceiveBeginPlay: Delay(0.0667) @2087 -> @15: the loop sound, DetonateClient
//! @174, the zone through the Instigator's Comp_Spawner at the OWNER's actor location - (0, 0, 100), Delay(2.5)
//! @584 -> @639: DetonateClient again, SpawnWatermelonSteven(the actor's location) @766, StevenSpawned @811,
//! K2_DestroyActor @822. ReceiveEndPlay @837: authority, !StevenSpawned -> the Instigator's
//! BeginAlternateCooldownOnAuthority(2, 0, true) @947 (not inside the reset's pass).
//! Deviation: the Instigator taken as the owner (the port's one field).
//! Not ported: the PlayerAttachCollision end overlap @2203 (AttachedTo is never written: its test never passes), HealSelfPop
//! (no caller).
//!
//! HealZone (FogDuration 3, Die With Instigator): ReceiveBeginPlay @5975: the parent's, then ActivateFog @2931:
//! FogActivated = true, StartFogBuff (PlayerCollision's overlaps at BeginPlay find nothing: UpdateOverlaps comes after
//! it, so the spawn's begin overlaps reach OnBeginOverlap for every pawn inside: the first pass), Delay(FogDuration)
//! @3000 -> @108: FogActivated = false, EndFogBuff, DestroySpawnedActor(true) @133.
//! Deviation: the overlap is tested once a tick in the zone's step, ahead of its latent actions, against the fighters'
//! root capsules where they stand (as the smoke zone's). The attach to the ground under it (BeginPlay's
//! BoxTraceSingle down @6267, K2_AttachToActor) is not ported: the zone stays where it spawned. The trace's HitActor
//! cast to DestructibleObjectBP has ObjectDestroyed += DestroyIfPlatformDestroyed (@5999-6896, `bind_ground_destroyed`):
//! K2_DestroyActor @7594, the EndPlay at the frame's actor sweep.
//!
//! The Watermelon: SpawnWatermelonSteven @2399, OnLoaded @6612, MulticastWatermelonStevenSetUp @4392,
//! EarlyDestroyWatermelonSteven @7960.
//! Unverified: the asset load is taken to complete in the same frame's leftover latent pass (Steven has no latent
//! action of his own pending at his tick), so the minion spawns there with `FirstTicks::Leftover`. A pending latent
//! action of his (the completion in his tick of the next frame) and the first load from disk are not modelled.

use super::pawn::clear_watermelon_steven_reference;
use super::state::{PendingSpawn, steven_of, steven_w};
use crate::Real;
use crate::State;
use crate::actors::projectile::{class_def, class_p, data_fighter};
use crate::actors::smoke_bomb::{GrenadeCache, grenade_body, grenade_statics};
use crate::actors::world_actor::WorldActorState;
use crate::actors::{
    ActorClass, ActorId, ActorRef, Projectile, ProjectileClass, ProjectileExt, ProjectileId, WorldActorClass,
};
use crate::combat::BuffClass;
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{
    BuffExt, BuffRef, buff, buff_mut, buff_of, end_buff, inst_view, set_buff_values, set_size_scale_to, spawn_buff,
};
use crate::combat::elem::{charge_kb, instigator_charge};
use crate::consts::{DELTA_SECONDS, SOLID_NAV};
use crate::events::{Event, SoundKey};
use crate::fighter::common::{actor_data, latent_frames, warn_once, warn_text};
use crate::fighter::control::{begin_alternate_cooldown, enable_invulnerability};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::traces::solids::sphere_static;
use crate::fighter::traces::{TRACE_PROJECTILE_GROUND, box_trace_single, capsule_trace_single};
use crate::fighter::{FighterId, ScaledDef, actor_y, cap_radius, capsule_half, facing};
use crate::math::ue::{SMALL_NUMBER, safe_normal, safe_normal2d};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{
    ActorEntry, ActorHooks, BuffEntry, BuffHooks, ProjectileHooks, SizeHooks, WorldActorHooks,
};
use crate::registry::props::{flag, num, obj};
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::{FirstTicks, MinionSpawn, all_list, destroy_minion, put_fighters, spawn_minion};
use crate::world::wall_bp::{WallActor, bind_ground_destroyed, wall_delegate_fns};
use mvs_collision::Ground;
use mvs_collision::rigid_sphere::{Body, Solver};
use mvs_data::value::{Props, Value};

const HEAL_GRENADE: Name = name!("HealGrenade_C");
const HEAL_ZONE: Name = name!("HealOverTimeZoneBP_C");
const PAWN: Name = name!("Pawn_StevenUniverse_C");
const HEAL_OVER_TIME: Name = name!("Buff_HealOverTime_C");
const WATERMELON_DEBUFF: Name = name!("Buff_StevenWatermelonDebuff_C");
const GREEN_THUMB: Name = name!("Buff_Steven_GreenThumb_C");
/// SpawnWatermelonSteven's LoadAsset @3143 (the soft class it casts to a BehaviorTree @6612): the minion's tree, run by
/// its name, a root of the bot trees.
const TREE: &str = "WatermelonSteven_BehaviorTree";
const HEAL_KEY: Name = name!("heal");
const GREEN_THUMB_BOOST: Name = name!("Buff_Steven_GreenThumbDamageBoost_C");

/// Mesh.SetWorldScale3D(0.85) @4987 (`setUp`).
fn watermelon_mesh_scale() -> Real {
    0.85
}

// ---- HealGrenade

/// `HealGrenade extends Projectile` (HealGrenade_C).
#[derive(State, Debug, Default)]
pub struct HealGrenade {
    /// The Capsule's.
    pub radius: Real,
    /// The launch, pending until the body's first step.
    pub pvx: Real,
    pub pvy: Real,
    /// CompoundBody: the body's turn and angular velocity.
    pub spin: Real,
    pub wa: Real,
    pub wb: Real,
    pub wc: Real,
    pub angle: Real,
    /// The tick its Delay(0.0667) @2087 fires at (set by `begin`).
    pub arm_at: Real,
    /// The tick its Delay(2.5) @584 fires at (-1: not started).
    pub detonate_at: Real,
    /// StevenSpawned.
    pub steven_spawned: bool,
    /// The welded body and the step's buffers. Not state.
    #[state(skip)]
    pub cache: GrenadeCache,
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn grenade(p: &Projectile) -> &HealGrenade {
    match &p.ext {
        ProjectileExt::HealGrenade(g) => g,
        e => panic!("HealGrenade_C without its state: {e:?}"),
    }
}

fn grenade_mut(p: &mut Projectile) -> &mut HealGrenade {
    match &mut p.ext {
        ProjectileExt::HealGrenade(g) => g,
        e => panic!("HealGrenade_C without its state: {e:?}"),
    }
}

fn owner_of(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("a heal grenade has an owner")
}

/// The constructor: the launch (HandleActorPostSpawn's: the direction normalised, x InitialSpeed, Y along the facing,
/// plus half the owner's velocity, as the smoke grenade's) and the Capsule's radius.
fn make(w: &World, f: FighterId, r: &SpawnRequest) -> ProjectileExt {
    let np = crate::world::spawning::request_props(w, f, r);
    let data = w.data.clone();
    let radius = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .and_then(|d| obj(&d.components, "Capsule"))
        .and_then(|c| num(c, "CapsuleRadius"))
        .unwrap_or(35.0);
    let d = obj(&np, "InitialMovementDirection");
    let (dy, dz) = (d.and_then(|d| num(d, "Y")).unwrap_or(0.0), d.and_then(|d| num(d, "Z")).unwrap_or(0.0));
    let sp = num(&np, "InitialSpeed").unwrap_or(0.0);
    // InitialSpeed: Velocity.GetSafeNormal() x InitialSpeed
    let [_, ny, nz] = safe_normal([0.0, dy, dz], SMALL_NUMBER);
    let (fvx, fvy) = (w.fighter(f).body.vx, w.fighter(f).body.vy);
    ProjectileExt::HealGrenade(HealGrenade {
        radius,
        pvx: ((ny * sp) * facing(w, f) + fvx / 2.0),
        pvy: ((nz * sp) + fvy / 2.0),
        arm_at: -1.0,
        detonate_at: -1.0,
        ..HealGrenade::default()
    })
}

/// The constructor's tail: the deactivated movement never moves the root, the body does.
fn construct(_w: &World, p: &mut Projectile) {
    p.vx = 0.0;
    p.vy = 0.0;
    p.stopped = true;
}

fn stage_platform_grounds(w: &World) -> Vec<Ground> {
    w.stage
        .stage_platforms
        .iter()
        .map(|p| Ground {
            left: f64::from(p.left),
            right: f64::from(p.right),
            bottom: f64::from(p.bottom),
            top: f64::from(p.top),
        })
        .collect()
}

/// FindTeleportSpot at the spawn (PostActorConstruction, ahead of BeginPlay) out of the stage block and its platforms,
/// as the smoke grenade's; then the arm: made in BeginPlay, ahead of the actor's first tick, which is in its spawn
/// frame: that frame counts, so the Delay(0.0667) of 5 frames runs out in the actor's 5th step counting the spawn
/// frame.
fn begin(w: &mut World, id: ProjectileId) {
    let (x, y, radius) = (pr(w, id).x, pr(w, id).y, grenade(pr(w, id)).radius);
    let mut boxes = crate::world::stage_query::static_shapes(w, SOLID_NAV, name!("heal grenade spawn"));
    boxes.boxes.extend(stage_platform_grounds(w));
    let at = crate::world::spawn_spot::find_teleport_spot(boxes.view(), x, y, radius);
    let tick = crate::real(w.flow.tick);
    let p = pm(w, id);
    #[allow(clippy::float_cmp, reason = "an exact comparison of the place it moved")]
    if at.x != x || at.y != y {
        (p.x, p.prev_x, p.y, p.prev_y) = (at.x, at.x, at.y, at.y);
    }
    grenade_mut(p).arm_at = tick + latent_frames(0.0667) - 1.0;
}

fn heal_key(w: &World, id: ProjectileId) -> SoundKey {
    SoundKey { tag: HEAL_KEY, n: pr(w, id).seq }
}

/// DetonateClient @2476 (off a dedicated server): the burst, the fuse's stop, the end sound.
fn detonate_client(w: &mut World, id: ProjectileId) {
    let (cls, f, x, y) = (pr(w, id).cls, owner_of(w, id), pr(w, id).x, pr(w, id).y);
    let fid = w.fighter(f).id;
    let key = heal_key(w, id);
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter: fid,
        vfx: Some(name!("PS_StevenWatermelon_Burst")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
    w.events.push(Event::SoundStop {
        fighter: fid,
        key,
        fade: Some(0.2),
        ev: None,
        curve: Some(name!("Linear")),
        cut: false,
    });
    w.events.push(sound(fid, name!("sfx_cha_steven_spitheal_end"), x, y));
}

fn sound(fighter: i32, ev: Name, x: Real, y: Real) -> Event {
    Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None }
}

fn step(w: &mut World, id: ProjectileId) -> bool {
    let tick = crate::real(w.flow.tick);
    let (arm_at, f) = (grenade(pr(w, id)).arm_at, owner_of(w, id));
    if tick == arm_at {
        // @15
        let (fid, attack_id) = (w.fighter(f).id, pr(w, id).attack_id);
        let key = heal_key(w, id);
        w.events.push(Event::SoundStart { fighter: fid, ev: name!("sfx_cha_steven_spitheal_loop"), key });
        detonate_client(w, id);
        let (zx, zy) = (w.fighter(f).body.x, actor_y(w, f) - 100.0);
        spawn_zone(w, f, zx, zy, attack_id);
        grenade_mut(pm(w, id)).detonate_at = tick + latent_frames(2.5);
    }
    if tick == grenade(pr(w, id)).detonate_at {
        // @639
        detonate_client(w, id);
        let (x, y) = (pr(w, id).x, pr(w, id).y);
        if steven_of(w, f).is_some() {
            spawn_watermelon_steven(w, f, x, y);
        }
        grenade_mut(pm(w, id)).steven_spawned = true;
        return true;
    }
    false
}

/// The body's step, behind the actor's tick: the pending launch as a velocity change, then the welded body against the
/// stage block, its platforms and the spawned actors' solid boxes.
fn physics(w: &mut World, id: ProjectileId) {
    let owner = data_fighter(pr(w, id));
    let data = w.data.clone();
    let mut c = std::mem::take(&mut grenade_mut(pm(w, id)).cache);
    if c.body.is_none() {
        let a = actor_data(w, &data, ActorRef::Fighter(owner), HEAL_GRENADE);
        c.body = Some((1.0, a.and_then(|a| grenade_body(w, a, "HealGrenade_C", 1.0))));
    }
    let Some(body) = c.body.and_then(|(_, b)| b) else {
        // its material or Box was refused (warned)
        grenade_mut(pm(w, id)).cache = c;
        return;
    };
    // Unverified: Morty's ship pieces are met through `grenade_statics` (kinematic boxes) as by the smoke and
    // polymorph grenades; the heal grenade's own Platform responses were not read.
    if w.data.stage.name.starts_with("Map_M011") {
        warn_once(w, name!("HealGrenade: Sky Arena's arm donut collider is not ported (profile and place unread)"));
    }
    grenade_statics(w, owner, HEAL_GRENADE, &body.bx, name!("HealGrenade_C body"), &mut c.grounds, &mut c.statics);
    let floor = sphere_static(w, w.stage.ground);
    let p = pm(w, id);
    let g = match &mut p.ext {
        ProjectileExt::HealGrenade(g) => g,
        e => panic!("HealGrenade_C without its state: {e:?}"),
    };
    p.vx += g.pvx;
    p.vy += g.pvy;
    g.pvx = 0.0;
    g.pvy = 0.0;
    let mut b = Body {
        x: f64::from(p.x),
        y: f64::from(p.y),
        vx: f64::from(p.vx),
        vy: f64::from(p.vy),
        spin: f64::from(g.spin),
        radius: f64::from(g.radius),
        angle: f64::from(g.angle),
        wa: f64::from(g.wa),
        wb: f64::from(g.wb),
        wc: f64::from(g.wc),
        ..Body::default()
    };
    Solver::default().compound_step(&mut b, &floor, Some(&c.statics), DELTA_SECONDS, &body.set, &body.bx, &body.mass);
    g.cache = c;
    (p.x, p.y, p.vx, p.vy, g.spin, g.angle, g.wa, g.wb, g.wc) = (
        crate::math::narrow(b.x),
        crate::math::narrow(b.y),
        crate::math::narrow(b.vx),
        crate::math::narrow(b.vy),
        crate::math::narrow(b.spin),
        crate::math::narrow(b.angle),
        crate::math::narrow(b.wa),
        crate::math::narrow(b.wb),
        crate::math::narrow(b.wc),
    );
}

/// The DamageCollision box at the actor (on the Capsule, unscaled; square about Z, so the body's turn is not read).
fn damage_box(w: &World, id: ProjectileId) -> Option<WorldSphere> {
    let p = pr(w, id);
    if p.no_collision {
        return None;
    }
    let e = class_def(w, p).and_then(|d| obj(&d.components, "DamageCollision")).and_then(|c| obj(c, "BoxExtent"));
    let g = |k: &str| e.and_then(|e| num(e, k)).unwrap_or(32.0);
    Some(WorldSphere {
        x: p.x,
        y: p.y,
        z: Some(0.0),
        r: 0.0,
        bx: Some([0.0, 0.0, g("X"), g("Y"), 0.0, 0.0, 0.0, g("Z"), 0.0]),
        ..WorldSphere::default()
    })
}

/// Normal2D (FVector2D::GetSafeNormal, tolerance 1e-8 on the squared size, InvSqrt): zero under it.
fn normal_2d(x: Real, y: Real) -> (Real, Real) {
    let [nx, ny] = safe_normal2d([x, y]);
    (nx, ny)
}

/// KnockbackSelf @3265 (`by`: the attacker; `bkb`: Hit.BaseKnockBack; (dx, dy): Hit.KnockBackDirectionVector as the
/// DamageManager hands it on): c = the attacker's Comp_Pawn_ChargeAttack GetChargeValue(1) when the component is valid,
/// else 1 @3443; v = dir x (BaseKnockBack x c); n = Normal2D(v), s = VSize2D(v); X clamped to
/// [HorizontalKnockbackZMinClamp, Max] (-1, 1), Y to [VerticalKnockbackZMinClamp, Max] (0.25, 1); then Normal2D(X, Y) x
/// (s x KnockbackMultiplier 2.5) -> SetVelocityMulticast -> Capsule.SetPhysicsLinearVelocity @4112: the body's linear
/// velocity replaced (its spin kept).
/// Deviation (as Bugs's safe): the direction is taken in the world's horizontal sense (World.hitBy's dx); the build's
/// MakeVector(0, X, Y) puts the vector's X on UE Y as it comes.
fn knockback_self(w: &mut World, id: ProjectileId, by: Option<ActorRef>, dx: Real, dy: Real, bkb: Real) {
    let c = match by {
        Some(ActorRef::Fighter(b)) => instigator_charge(w, b).map_or(1.0, |c| c.kb),
        Some(a @ (ActorRef::Projectile(_) | ActorRef::Item(_))) => charge_kb(w, a),
        _ => 1.0,
    };
    let k = bkb * c;
    let (vx, vy) = ((dx * k), (dy * k));
    let (nx, ny) = normal_2d(vx, vy);
    let s = ((vx * vx) + (vy * vy)).sqrt();
    let p = class_p(w, pr(w, id));
    let n = |k: &str| num(p, k).unwrap_or(0.0);
    let cy = min(max(ny, n("VerticalKnockbackZMinClamp")), n("VerticalKnockbackZMaxClamp"));
    let cx = min(max(nx, n("HorizontalKnockbackZMinClamp")), n("HorizontalKnockbackZMaxClamp"));
    let len = s * n("KnockbackMultiplier");
    let (ux, uy) = normal_2d(cx, cy);
    let q = pm(w, id);
    q.vx = ux * len;
    q.vy = uy * len;
}

/// OnHitByAlly / OnHitByEnemy -> KnockbackSelf @3199 / @3232 (bound at BeginPlay by the two flags).
#[allow(clippy::too_many_arguments, reason = "the hook's parameters")]
fn hit_by(
    w: &mut World,
    id: ProjectileId,
    ally: bool,
    dx: Real,
    dy: Real,
    bkb: Real,
    _type: i32,
    by: Option<ActorRef>,
) {
    let on = flag(class_p(w, pr(w, id)), if ally { "CanBeKnockedBackByAllies" } else { "CanBeKnockedBackByEnemies" });
    if !on {
        return;
    }
    knockback_self(w, id, by, dx, dy, bkb);
}

/// @837: authority, !StevenSpawned -> the Instigator's BeginAlternateCooldownOnAuthority(2, 0, true).
fn end(w: &mut World, id: ProjectileId) {
    if crate::world::flow::reset_pass(w) || grenade(pr(w, id)).steven_spawned {
        return;
    }
    let f = owner_of(w, id);
    begin_alternate_cooldown(w, f, 2, 0, true);
}

// ---- HealZone

/// `HealZone` (HealOverTimeZoneBP_C).
#[derive(State, Debug, Default)]
pub struct HealZone {
    pub x: Real,
    pub y: Real,
    pub attack_id: i32,
    /// PlayerCollision's at scale 1 (the root's scale is taken at the read).
    pub radius: Real,
    pub fog_activated: bool,
    /// The tick its Delay(FogDuration) fires at.
    pub fog_end_at: Real,
    /// The fighters PlayerCollision overlaps (ids).
    pub inside: Vec<i32>,
}

/// The HealZone's place (its `x`, `y`).
pub fn heal_zone_place(w: &World, a: &crate::actors::WorldActor, s: &HealZone) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

fn zone_of(w: &World, id: ActorId) -> Option<&HealZone> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::HealZone(z) => Some(z),
        _ => None,
    }
}

fn zone_mut(w: &mut World, id: ActorId) -> Option<&mut HealZone> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::HealZone(z) => Some(z),
        _ => None,
    }
}

/// `spawnZone`: the grenade's CompSpawnActor of the zone at (x, y) (the first overlap pass is its BeginPlay hook).
fn spawn_zone(w: &mut World, f: FighterId, x: Real, y: Real, attack_id: i32) {
    let data = w.data.clone();
    let Some(a) = actor_data(w, &data, ActorRef::Fighter(f), HEAL_ZONE) else {
        warn_text(w, "spawned actor not exported (HealOverTimeZoneBP_C)");
        return;
    };
    // Made in BeginPlay, ahead of the zone's first step in its spawn frame: the spawn frame counts (the grenade's arm).
    let z = HealZone {
        x,
        y,
        attack_id,
        radius: obj(&a.components, "PlayerCollision").and_then(|c| num(c, "CapsuleRadius")).unwrap_or(0.0),
        // ActivateFog @2975 (StartFogBuff finds no overlap yet); FogVisuals: the view's, off the actor
        fog_activated: true,
        fog_end_at: crate::real(w.flow.tick) + latent_frames(num(&a.p, "FogDuration").unwrap_or(0.0)) - 1.0,
        inside: Vec::new(),
    };
    let x = crate::world::spawning::new_world_actor(
        w,
        HEAL_ZONE,
        WorldActorClass::HealOverTimeZoneBP,
        f,
        false,
        WorldActorState::HealZone(z),
    );
    let id = w.actors.actors.insert(x);
    let charge = instigator_charge(w, f);
    crate::world::spawning::comp_spawn_actor(w, id, charge, f);
}

/// PlayerCollision (the 360 sphere) against a fighter's root capsule.
fn touches(w: &World, o: FighterId, id: ActorId) -> bool {
    if w.fighter(o).life.dead > 0.0 || crate::fighter::no_collision(w, o) {
        return false;
    }
    let Some(z) = zone_of(w, id) else { return false };
    let k = w.actors.actors.get(id).and_then(|a| a.size_scale).unwrap_or(1.0);
    let (cr, cy) = (cap_radius(w, o), actor_y(w, o));
    let s = max(capsule_half(w, o) - cr, 0.0);
    let gap = max(max(cy - s - z.y, 0.0), z.y - (cy + s));
    let gx = w.fighter(o).body.x - z.x;
    (gx * gx + gap * gap).sqrt() <= z.radius * k + cr
}

fn begin_overlap(w: &mut World, id: ActorId, o: FighterId) {
    let Some(z) = zone_of(w, id) else { return };
    if !z.fog_activated {
        return;
    }
    let Some(owner) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    if w.fighter(o).team.team != w.fighter(owner).team.team {
        return; // @4768
    }
    if w.fighter(o).life.not_player_character {
        return;
    }
    spawn_buff(w, o, HEAL_OVER_TIME, 6.0, Some(1), Some(owner), true, None);
}

fn remove_heal(w: &mut World, o: FighterId) {
    let live: Vec<BuffRef> = w
        .fighter(o)
        .buffs
        .buffs
        .iter()
        .filter(|b| b.cls == HEAL_OVER_TIME && b.frames > 0.0)
        .map(|b| BuffRef { fighter: o, serial: b.serial })
        .collect();
    for b in live {
        end_buff(w, o, b);
    }
}

fn end_fog_buff(w: &mut World, id: ActorId) {
    let Some(z) = zone_of(w, id) else { return };
    for k in z.inside.clone() {
        if let Some(o) = w.by_id(k) {
            remove_heal(w, o);
        }
    }
}

fn overlap_pass(w: &mut World, id: ActorId) {
    let all = all_list(w);
    for &o in &all {
        let on = touches(w, o, id);
        let oid = w.fighter(o).id;
        let Some(z) = zone_of(w, id) else { break };
        let (at, fog) = (z.inside.iter().position(|&v| v == oid), z.fog_activated);
        match (on, at) {
            (true, None) => {
                if let Some(z) = zone_mut(w, id) {
                    z.inside.push(oid);
                }
                begin_overlap(w, id, o);
            }
            (false, Some(at)) => {
                if let Some(z) = zone_mut(w, id) {
                    z.inside.remove(at);
                }
                if fog {
                    remove_heal(w, o); // @2083
                }
            }
            _ => {}
        }
    }
    put_fighters(w, all);
}

/// DestroyIfPlatformDestroyed's bind name (`wall_delegate_fns`).
const ZONE_DESTROY: &str = "HealOverTimeZoneBP_C.DestroyIfPlatformDestroyed";

fn zone_destroy(w: &mut World, a: ActorRef, _wall: WallActor) {
    if let ActorRef::Actor(id) = a
        && let Some(x) = w.actors.actors.get_mut(id)
    {
        x.dead = true;
    }
}

fn zone_begin(w: &mut World, id: ActorId) {
    // @5999-6896
    if let Some(z) = zone_of(w, id) {
        let (x, y) = (z.x, z.y);
        let me = ActorRef::Actor(id);
        let hit =
            box_trace_single(w, x, y - 50.0, x, y - 150.0, 100.0, 0.0, 100.0, TRACE_PROJECTILE_GROUND, &[], Some(me));
        bind_ground_destroyed(w, hit.as_ref(), me, ZONE_DESTROY);
    }
    overlap_pass(w, id); // the spawn's begin overlaps
}

fn zone_step(w: &mut World, id: ActorId) {
    overlap_pass(w, id);
    let Some(z) = zone_of(w, id) else { return };
    if crate::real(w.flow.tick) == z.fog_end_at {
        // @108
        if let Some(z) = zone_mut(w, id) {
            z.fog_activated = false;
        }
        end_fog_buff(w, id);
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true; // DestroySpawnedActor(true)
        }
    }
}

/// @7410
fn zone_end(w: &mut World, id: ActorId) {
    let Some(z) = zone_of(w, id) else { return };
    if !z.fog_activated {
        return;
    }
    if let Some(z) = zone_mut(w, id) {
        z.fog_activated = false;
    }
    end_fog_buff(w, id);
}

/// Its bodies: the root Capsule (SpawnedActorParent's template: no size and no profile serialized, so the
/// UShapeComponent constructor's OverlapAllDynamic, WorldDynamic) and PlayerCollision (profile Fog: WorldDynamic,
/// blocks no trace channel). WorldDynamic bodies are among the types the traces do not gather, and neither blocks a
/// trace channel: no body to add. Entered so the gather does not walk the class data.
fn zone_bodies(_w: &World, _id: ActorId, _out: &mut Vec<crate::fighter::traces::TraceBody>) {}

// ---- the Watermelon

fn in_world(w: &World, m: FighterId) -> bool {
    w.players.contains(&m) || w.pawns.as_ref().is_some_and(|ps| ps.contains(&m))
}

/// SpawnWatermelonSteven @2399 (on Steven f, from HealGrenade @766): OnExitedArena bound to DestroyWatermelonOnArenaExit
/// (`player_knocked_out`); every Pawn_StevenUniverse whose Instigator is f and isWatermelonSteven ->
/// EarlyDestroyWatermelonSteven @2930; a valid WatermelonSteven -> K2_DestroyActor @3084 (no effects); LoadAsset (the
/// tree) @3143, whose completion runs OnLoaded @6612.
fn spawn_watermelon_steven(w: &mut World, f: FighterId, x: Real, y: Real) {
    let all = all_list(w);
    for &m in &all {
        if w.fighter(m).life.minion_creator == Some(f)
            && steven_of(w, m).is_some_and(|s| s.watermelon.is_watermelon_steven)
        {
            early_destroy(w, m);
        }
    }
    put_fighters(w, all);
    let old = steven_of(w, f).and_then(|s| s.pawn.watermelon_steven);
    if let Some(old) = old
        && in_world(w, old)
    {
        destroy(w, old); // @3084, plain
    }
    steven_w(w, f).watermelon.pending_spawn = Some(PendingSpawn { x, y });
}

/// OnLoaded @6612: CapsuleTraceSingle from SpawnLocation + (0, 0, 250) down to it with Steven's own capsule size
/// (CapsuleComponent.CapsuleRadius / CapsuleHalfHeight: the unscaled values), ProjectileGroundTrace, ignoring self. No
/// initial overlap: a hit -> its Location @7928, else SpawnLocation @4104. An initial overlap: the trace again from
/// SpawnLocation - (0, 0, 250) up to it @7162; an initial overlap again -> SpawnLocation @4136; a hit -> its Location
/// @7719; else SpawnLocation @3497. Then SpawnAIFromClass(Pawn_StevenUniverse, the tree, there, Steven's rotation)
/// @3572, MulticastWatermelonStevenSetUp @3639 (`set_up`), then on the minion Buff_StevenWatermelonDebuff (Steven, 1
/// stack, 10000 s) @3737 and, with Buff_Steven_GreenThumb on Steven @3835, Buff_Steven_GreenThumbDamageBoost (Steven,
/// 0 stacks, no duration) @4005.
fn on_loaded(w: &mut World, f: FighterId, at: PendingSpawn) {
    let (r, hh) = (w.fighter(f).capsule.cap_radius_u, w.fighter(f).capsule.cap_half_u);
    let (mut px, mut py) = (at.x, at.y);
    let own = Some(ActorRef::Fighter(f));
    let down = capsule_trace_single(w, at.x, at.y + 250.0, at.x, at.y, r, hh, 3, &[], own);
    if !down.as_ref().is_some_and(|d| d.initial) {
        if let Some(d) = down.filter(|d| d.blocking) {
            (px, py) = (d.x, d.y);
        }
    } else {
        let up = capsule_trace_single(w, at.x, at.y - 250.0, at.x, at.y, r, hh, 3, &[], own);
        if let Some(u) = up.filter(|u| !u.initial && u.blocking) {
            (px, py) = (u.x, u.y);
        }
    }
    // The tree is in the bot trees; a World with no bot trees gives the minion a controller with no tree (no input).
    let tree = w
        .bot_trees
        .as_ref()
        .is_some_and(|b| b.trees.contains_key(TREE))
        .then(|| name!("WatermelonSteven_BehaviorTree"));
    let m = spawn_minion(
        w,
        MinionSpawn {
            character: name!("steven"),
            owner: f,
            x: px,
            y: py,
            facing: facing(w, f),
            tree,
            at: FirstTicks::Leftover,
        },
    );
    set_up(w, f, m);
    spawn_buff(w, m, WATERMELON_DEBUFF, 10000.0, Some(1), Some(f), true, None);
    if buff_of(w, f, GREEN_THUMB).is_some() {
        spawn_buff(w, m, GREEN_THUMB_BOOST, 0.0, Some(0), Some(f), true, None);
    }
}

/// MulticastWatermelonStevenSetUp @4392, past what `spawn_minion` writes (NotPlayerCharacter, the team, IsMinion,
/// MinionCreator, KeepOnCamera.Unregister): WatermelonSteven = m @4392, isWatermelonSteven @4621, WatermelonVisuals
/// @4654 (the view's, from isWatermelonSteven), EnableInvulnerability(false, false) @4731, Mesh.SetWorldScale3D(0.85)
/// @4987, Comp_Actor_Damage.Weight = 0.01 @5054, the OnBFDefeated binding @5448 (`minion_defeated`).
/// Deviation: the Mesh's world scale 0.85 is taken as the fighter's `scaled.mesh_scale` (what CharDef.meshScale
/// holds); the run rate and the hit shapes read the character data's, so they keep Steven's.
/// Not ported: SetRTPCValue(mix_duck_off_vo_steven, 1) on its Ak @5287, SpotDodgeAnim = null @5340, AIControllerClass
/// @4690 (read by nothing after the spawn).
fn set_up(w: &mut World, f: FighterId, m: FighterId) {
    steven_w(w, f).pawn.watermelon_steven = Some(m);
    steven_w(w, m).watermelon.is_watermelon_steven = true;
    enable_invulnerability(w, m, false, false);
    let k = w.fighter(m).body.size_scale;
    let b = ScaledDef {
        mesh_scale: watermelon_mesh_scale(),
        ..ScaledDef::of(crate::combat::hit::def_of(w, w.fighter(m).base_ch))
    };
    let fm = w.fighter_mut(m);
    fm.base_mesh_scale = Some(b.mesh_scale); // baseCh = { ...baseCh, meshScale } (the size changes keep it)
    fm.scaled = if k == 1.0 {
        b
    } else {
        ScaledDef {
            cap_half: (b.cap_half * k),
            cap_radius: (b.cap_radius * k),
            crouched_half: (b.crouched_half * k),
            mesh_scale: (b.mesh_scale * k),
        }
    };
    fm.body.weight_set = Some(0.01);
}

/// EarlyDestroyWatermelonSteven @7960 (on the minion): PS_SteveWatermelon_Destroyed at its location with the team
/// colour, @6424 sfx_cha_steven_watermelon_destroy at its location, K2_DestroyActor.
fn early_destroy(w: &mut World, m: FighterId) {
    if !in_world(w, m) {
        return;
    }
    let (fid, x, y) = (w.fighter(m).id, w.fighter(m).body.x, actor_y(w, m));
    w.events.push(Event::ProjectileDeath {
        cls: PAWN,
        fighter: fid,
        vfx: Some(name!("PS_SteveWatermelon_Destroyed")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
    w.events.push(sound(fid, name!("sfx_cha_steven_watermelon_destroy"), x, y));
    destroy(w, m);
}

/// K2_DestroyActor of the minion. Its creator's WatermelonSteven is dropped with it (the build nulls it only at
/// BPI_ClearWatermelonStevenReference @10081 and keeps the stale reference, whose IsValid every reader asks first:
/// then false, the same answer; a destroyed pawn is not kept in the sim state).
fn destroy(w: &mut World, m: FighterId) {
    if let Some(c) = w.fighter(m).life.minion_creator
        && steven_of(w, c).is_some_and(|s| s.pawn.watermelon_steven == Some(m))
    {
        steven_w(w, c).pawn.watermelon_steven = None;
    }
    destroy_minion(w, m);
}

// ---- Buff_Steven_GreenThumbDamageBoost

/// Buff_Steven_GreenThumbDamageBoost_C's extension (`{ orig, last, pos, rate }`).
#[derive(State, Debug, Default)]
pub struct GreenThumbBoost {
    pub orig: Real,
    pub last: Real,
    pub pos: Real,
    pub rate: Real,
}

fn boost(w: &World, b: BuffRef) -> Option<&GreenThumbBoost> {
    match &buff(w, b)?.ext {
        BuffExt::GreenThumbBoost(e) => Some(e),
        _ => None,
    }
}

/// The timeline's update @531: a Weight other than WeightLastFrame -> OriginaWeight += Weight - WeightLastFrame;
/// DealtDamageMods.DamageMod = Lerp(1, MaxDamageBoost 3, alpha) @939; the owner's SetActorScale3D(VLerp(1, MaxScale
/// 1.5, alpha)) @1054; Weight = OriginaWeight + alpha x WeightAddition 10 @1258; then WeightLastFrame = Weight @477.
fn boost_update(w: &mut World, f: FighterId, b: BuffRef, alpha: Real) {
    let Some(e) = boost(w, b) else { return };
    let (mut orig, last) = (e.orig, e.last);
    let wt = crate::fighter::weight(w, f);
    #[allow(clippy::float_cmp, reason = "an exact comparison")]
    if wt != last {
        orig += wt - last;
    }
    let d = inst_view(w, b);
    let (max_boost, add) = (d.num("MaxDamageBoost").unwrap_or(0.0), d.num("WeightAddition").unwrap_or(0.0));
    let scale = d.obj("MaxScale").and_then(|o| num(o, "X")).unwrap_or(1.0);
    let mut mods = d.obj("DealtDamageMods").cloned().unwrap_or_default();
    mods.insert("DamageMod".into(), Value::Num(f64::from(1.0 + (alpha * (max_boost - 1.0)))));
    let mut values = Props::new();
    values.insert("DealtDamageMods".into(), Value::Obj(mods));
    set_buff_values(w, b, &values);
    set_size_scale_to(w, f, 1.0 + (alpha * (scale - 1.0)));
    w.fighter_mut(f).body.weight_set = Some(orig + (alpha * add));
    let now = crate::fighter::weight(w, f);
    if let Some(x) = buff_mut(w, b)
        && let BuffExt::GreenThumbBoost(e) = &mut x.ext
    {
        e.orig = orig;
        e.last = now;
    }
}

/// ReceiveBeginPlay @10: DamageBoostTimeline's play rate 1 / TimeTillMaxDamage (10); with the owner's Comp_Actor_Damage
/// OriginaWeight = its Weight @288, WeightLastFrame = it, PlayFromStart @364 (the update at position 0 fires inside it).
fn boost_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    let t = inst_view(w, b).num("TimeTillMaxDamage").unwrap_or(0.0);
    let wt = crate::fighter::weight(w, f);
    let e = GreenThumbBoost { orig: wt, last: wt, pos: 0.0, rate: if t == 0.0 { 0.0 } else { 1.0 / t } };
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::GreenThumbBoost(e);
    }
    boost_update(w, f, b, 0.0);
}

/// The timeline (length 1, one linear key pair 0 -> 1) ticks in the buff's tick: position + dt x rate, clamped at 1.
/// Finished @401: nothing (the values stay at the end's). No EndPlay of its own: the scale and weight stay after the
/// buff.
fn boost_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(e) = boost(w, b) else { return false };
    if e.pos >= 1.0 {
        return false;
    }
    let pos = min(e.pos + (DELTA_SECONDS * e.rate), 1.0);
    if let Some(x) = buff_mut(w, b)
        && let BuffExt::GreenThumbBoost(e) = &mut x.ext
    {
        e.pos = pos;
    }
    boost_update(w, f, b, pos);
    false
}

// ---- the joined hooks

/// BPI_Cleanup @8644 (ResetFighterLogic, a respawn's): the stored Watermelon's EarlyDestroyWatermelonSteven @8360
/// (pawn.rs's cleanup, joined behind it). The hook runs inside the reset: the destroy is marked here and run in the same
/// frame's timer pass (`timer_tick`). Deviation: the effects and the destroy come in the leftover latent pass of the
/// respawn's frame, not inside ResetFighterLogic (nothing between reads the Watermelon).
pub fn reset(w: &mut World, f: FighterId) {
    if steven_of(w, f).is_some_and(|s| s.pawn.watermelon_steven.is_some()) {
        steven_w(w, f).watermelon.cleanup_pending = true;
    }
}

/// The LoadAsset completion (OnLoaded), in the frame's leftover latent pass (Unverified above), after the cleanup.
fn timer_tick(w: &mut World, f: FighterId) {
    let Some(st) = steven_of(w, f) else { return };
    if st.watermelon.cleanup_pending {
        // BPI_Cleanup @8644 -> the stored Watermelon's EarlyDestroyWatermelonSteven @8360 (IsValid first)
        let m = st.pawn.watermelon_steven;
        steven_w(w, f).watermelon.cleanup_pending = false;
        if let Some(m) = m {
            early_destroy(w, m);
        }
    }
    let Some(at) = steven_w(w, f).watermelon.pending_spawn.take() else { return };
    on_loaded(w, f, at);
}

/// DestroyWatermelonOnArenaExit (bound to Steven's OnExitedArena at his first SpawnWatermelonSteven @2399): his
/// WatermelonSteven, when valid, EarlyDestroyWatermelonSteven. Deviation: taken at the game mode's PlayerKnockedOut for
/// the loser himself (as Taz's own OnExitedArena), a little after the OnExitedArena broadcast.
fn player_knocked_out(w: &mut World, f: FighterId, loser: FighterId) {
    if loser != f {
        return;
    }
    if let Some(m) = steven_of(w, f).and_then(|s| s.pawn.watermelon_steven) {
        early_destroy(w, m);
    }
}

/// CallTriggerWatermelonCooldown (the minion's OnBFDefeated, bound in the set-up @5448) @10108: the creator's
/// GetInstigator (himself) BPI_ClearWatermelonStevenReference, then @6275 (authority) his
/// BeginAlternateCooldownOnAuthority(2, 0, true).
/// Deviation: the minion is destroyed here (`destroy_minion`, with no effects); the build leaves the
/// rung-out pawn in the level until the next SpawnWatermelonSteven or the creator's exit destroys it with
/// EarlyDestroyWatermelonSteven's effects at its rung-out place.
fn minion_defeated(w: &mut World, creator: FighterId, minion: FighterId) {
    if !steven_of(w, minion).is_some_and(|s| s.watermelon.is_watermelon_steven) {
        return;
    }
    clear_watermelon_steven_reference(w, creator);
    begin_alternate_cooldown(w, creator, 2, 0, true);
    destroy(w, minion);
}

/// The hooks of this file in Steven's CharHooks (`joinHooks('watermelon.rs', ...)`; `reset` is composed in
/// `steven/mod.rs`).
pub fn hooks(h: &mut CharHooks) {
    h.timer_tick = Some(timer_tick);
    h.player_knocked_out = Some(player_knocked_out);
    h.minion_defeated = Some(minion_defeated);
}

/// This file's registry entries (HealGrenade_C, HealOverTimeZoneBP_C, Buff_Steven_GreenThumbDamageBoost_C).
/// foreignOwner is not marked: a disguise or burp copy would run SpawnWatermelonSteven's cast on its owner (nothing for
/// a pawn that is no Steven), but the class's other reads of its owner were not checked for that case.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let grenade_hooks = ProjectileHooks {
        construct: Some(construct),
        make: Some(make),
        begin: Some(begin),
        step: Some(step),
        physics: Some(physics),
        damage_box: Some(damage_box),
        hit_by: Some(hit_by),
        end: Some(end),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        "HealGrenade_C",
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::HealGrenade), ActorHooks::Projectile(grenade_hooks)),
    )?;
    let zone = WorldActorHooks {
        bodies: Some(zone_bodies),
        // touches reads the scale; the view too
        size: Some(SizeHooks::default()),
        begin_play: Some(zone_begin),
        step: Some(zone_step),
        end: Some(zone_end),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        "HealOverTimeZoneBP_C",
        ActorEntry::new(ActorClass::World(WorldActorClass::HealOverTimeZoneBP), ActorHooks::World(zone)),
    )?;
    wall_delegate_fns(r, &[(ZONE_DESTROY, zone_destroy)])?;
    r.buffs.enter(
        "Buff_Steven_GreenThumbDamageBoost_C",
        BuffEntry::new(
            BuffClass::BuffStevenGreenThumbDamageBoost,
            BuffHooks { begin: Some(boost_begin), tick: Some(boost_tick), ..BuffHooks::default() },
        ),
    )?;
    Ok(())
}
