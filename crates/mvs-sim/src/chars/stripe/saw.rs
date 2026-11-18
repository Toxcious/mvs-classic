//! Stripe's saw: C023B_SawBlade_EdgeTrace and its children ([`Saw`], a projectile
//! that rides the edges: `fighter::traces::edges`), C023B_AllySaw ([`AllySaw`], a DealerActor); joined to timerTick.
//!
//! Classes and their sim kind:
//! - C023B_SawBlade_EdgeTrace_C, C023B_SawBlade_EdgeTrace_StartOnGround_C (a data child with no graph of its own:
//!   StartAttachedToGround true) and C023B_SawBlade_FromAlly_EdgeTrace_C (BeginPlay: the parent's, then ToggleVFX, the
//!   view's; CanWallRoll false in its defaults): ParentProjectilePhysics children, projectile kinds. StripeState.saws
//!   holds their ids.
//! - C023B_AllySaw_C: a SpawnedActorParent child, a World.actors kind.
//! - C023B_SawBlade_C (and _FromAlly): dormant: no entry.
//!
//! What is not run (the view's or nothing): the Decal and its team material, ToggleVFX / Rotate VFX /
//! SetSparksRotation, ReceiveTick's Sawblade spin, the owner's SetSubMeshVisibilityMulticast, SetTeamColor.
//! Unreached: EnableProjectile (Multicast Toggle Projectile is only ever called with false, @4768), AttachToPlatform
//! (no caller), ReflectedProjectile @10593.
//!
//! Space: sim x = -UE Y, sim y = UE Z. The edge API takes UE vectors (X depth, Y, Z).
// Unverified: `sawHbd` and `hit` are two packets here: every
// write of the variable is followed at once by `hit = sawPacket`, so they differ only where another writer of `hit`
// (a charge's scale) would also have reached the variable.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::too_many_arguments,
    reason = "polygon and tag ids are small non-negative integers; negated comparisons keep their NaN behaviour; the calls' parameters"
)]

use crate::math as det;
use crate::world::stage_query::StageShape;
use indexmap::IndexSet;
use mvs_collision::Ground;

use crate::Real;
use crate::State;
use crate::actors::projectile::{
    class_p, clear_max_lifetime, max_lifetime_at, pm_initial_speed, reflect_boxes_on, set_gravity_scale, set_hitbox,
    stop_simulating, truthy, update_max_lifetime, yaw_x,
};
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, Projectile, ProjectileClass, ProjectileExt, ProjectileId};
use crate::actors::{WorldActor, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::WorldSphere;
use crate::combat::elem::{charge_pct, instigator_charge};
use crate::combat::hit::hurt_hit;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::packet::DataVec;
use crate::combat::phases::{alive, give_vbuf, no_hit, victim_pose};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, SoundKey};
use crate::fighter::common::{actor_data, warn_text};
use crate::fighter::control::{actor_spawn_ability, attack_dodged, dodge_over};
use crate::fighter::moves::V2;
use crate::fighter::traces::edges::{
    EdgePend, EdgeRef, EdgeRider, attach_to_edge, detach_from_edge, edge_poly_live, edge_poly_of, rider_begin_play,
    rider_latent,
};
use crate::fighter::traces::{TraceHit, sphere_trace_multi_for_objects};
use crate::fighter::{FighterId, actor_y, facing, invulnerable};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks, ReflectHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{num, obj, text};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::match_fighters;
use crate::world::spawned_platform::BlockKind;
use crate::world::spawning::{comp_spawn_actor, comp_spawn_projectile, destroy_projectile, new_world_actor};

const SAW: &str = "C023B_SawBlade_EdgeTrace_C";
const SAW_ON_GROUND: &str = "C023B_SawBlade_EdgeTrace_StartOnGround_C";
const SAW_FROM_ALLY: &str = "C023B_SawBlade_FromAlly_EdgeTrace_C";
const ALLY_SAW: &str = "C023B_AllySaw_C";

/// HitLaunch @7662.
const HIT_LAUNCH_SPEED: Real = 1500.0;
/// Reflected's Delay(0.05) @10233 (resume @321 -> @15) and the Delay(0.0) @228 (resume @15).
const N_REFLECT: i32 = 0;
const N_POLL: i32 = 1;

/// `Saw extends Projectile` (C023B_SawBlade_EdgeTrace_C and its children): C023B_SawBlade_EdgeTrace's own variables
/// (class defaults from the data) with its Comp_RunAlongEdgeTrace.
#[derive(State, Debug, Default)]
pub struct Saw {
    pub rider: EdgeRider,
    /// HitBoxDamage, the variable (UE axes): UpdateHitBoxDamage copies it into the boxes' packet (`hit`).
    pub saw_hbd: HitPacket,
    pub attach_gate: bool,
    pub bounce_gate: bool,
    pub can_set_timer: bool,
    pub can_wall_roll: bool,
    /// IsMovingOnGround: UserConstructionScript's StartAttachedToGround, then DisableProjectile's true.
    pub on_ground: bool,
    pub initial_direction_left: bool,
    pub floor_speed: Real,
    pub max_floor_speed: Real,
    /// 'Attack ID'.
    pub saw_attack_id: i32,
    pub paused_edge_attach: bool,
    pub paused_clockwise: bool,
    /// StationaryMobilityCount as the leftover pass last saw it.
    pub seen_stationary: i32,
    /// The delegates its graph binds: HitByAlly -> HitLaunch, On Run Along Disabled -> GroundTimeEvent, On Turn
    /// Corner -> UpdateHitOnTurnCorner.
    pub launch_bound: bool,
    pub ground_bound: bool,
    pub turn_bound: bool,
    /// The DoOnce @326.
    pub bounce_sound_done: bool,
    pub start_attached: bool,
    /// Its own latent actions, and the tick its tick last found one pending.
    pub saw_pend: Vec<EdgePend>,
    pub latent_at: i32,
    /// The Stripe whose StripeState.saws lists it (its spawner; a reflect does not move it).
    pub lister: Option<FighterId>,
    /// The saw's own actions run now (`passing === pr`).
    #[state(skip)]
    pub passing: bool,
}

/// `AllySaw` (C023B_AllySaw_C): a SpawnedActorParent child attached to the ally its saw hit, the owner's (Stripe's)
/// spawned actor. RotatingMovement rolls its root 260 deg/s; its HitBox2D (radius 50) and SawBlade sit at relative X
/// 217.06 on the root.
// Unverified: the orbit is taken as the offset (217.06 x the ally's facing, 0) at the spawn turned counterclockwise in
// the sim's view at 260 deg/s from its first tick, and mirrored in x when the ally turns; ComingFromLeft is the
// blade's side, so the hit's facing is the sign of the offset's x.
// Unverified: the root rotates and the blade tests its hits from the spawn frame on.
#[derive(State, Debug)]
pub struct AllySaw {
    pub ally: FighterId,
    pub attack_id: i32,
    pub hit: HitPacket,
    /// The actor's location: the ally's.
    pub x: Real,
    pub y: Real,
    /// The blade from the actor's location, sim axes.
    pub ox: Real,
    pub oy: Real,
    pub last_facing: Real,
    /// The MaxLifetimeTimer's tick (0: none).
    pub die_at: i32,
    /// The DoOnce @358.
    pub hit_done: bool,
    /// At scale 1.
    pub radius: Real,
    pub hit_set: IndexSet<i32>,
    /// The HitBox2D sphere as the last read wrote it.
    pub sphere_x: Real,
    pub sphere_y: Real,
    pub sphere_r: Real,
}

impl Default for AllySaw {
    /// (the record loader's blank: every field is then loaded)
    fn default() -> Self {
        AllySaw {
            ally: FighterId::new(0, 0),
            attack_id: 0,
            hit: HitPacket::default(),
            x: 0.0,
            y: 0.0,
            ox: 0.0,
            oy: 0.0,
            last_facing: 1.0,
            die_at: 0,
            hit_done: false,
            radius: 0.0,
            hit_set: IndexSet::new(),
            sphere_x: 0.0,
            sphere_y: 0.0,
            sphere_r: 0.0,
        }
    }
}

const ORBIT_R: Real = 217.06151;

fn orbit_step() -> Real {
    det::ue::deg_to_rad(260.0 * DELTA_SECONDS)
}

// ------------------------------------------------------------------ access

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live saw")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live saw")
}

fn saw(w: &World, id: ProjectileId) -> &Saw {
    match &pr(w, id).ext {
        ProjectileExt::Saw(s) => s,
        _ => panic!("a saw's id names a projectile of another class"),
    }
}

fn saw_mut(w: &mut World, id: ProjectileId) -> &mut Saw {
    match &mut pm(w, id).ext {
        ProjectileExt::Saw(s) => s,
        _ => panic!("a saw's id names a projectile of another class"),
    }
}

/// The live saws, in the list's order.
pub fn live_saws(w: &World) -> Vec<ProjectileId> {
    w.actors
        .projectiles
        .iter()
        .filter(|(_, p)| !p.dead && matches!(p.ext, ProjectileExt::Saw(_)))
        .map(|(id, _)| id)
        .collect()
}

fn owner(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("a saw has its owner")
}

fn cls_num(w: &World, id: ProjectileId, k: &str) -> Real {
    num(class_p(w, pr(w, id)), k).unwrap_or(0.0)
}

fn sound(w: &mut World, id: ProjectileId, ev: Name) {
    let p = pr(w, id);
    let e = Event::Sound { fighter: p.owner_id, ev, x: Some(p.x), y: Some(p.y), hit: None, pooled: false, key: None };
    w.events.push(e);
}

fn key(tag: Name, n: i32) -> SoundKey {
    SoundKey { tag, n }
}

fn sound_start(w: &mut World, id: ProjectileId, ev: Name, tag: Name) {
    let p = pr(w, id);
    let e = Event::SoundStart { fighter: p.owner_id, ev, key: key(tag, p.seq) };
    w.events.push(e);
}

fn sound_stop(w: &mut World, id: ProjectileId, tag: Name, ev: Option<Name>) {
    let p = pr(w, id);
    let e = Event::SoundStop { fighter: p.owner_id, key: key(tag, p.seq), fade: None, ev, curve: None, cut: false };
    w.events.push(e);
}

/// UpdateMaxLifetimeWithAuthority's `ahead`: a call ahead of the actor's move of the frame (its spawn, a fighter's hit
/// in the fighters' pass, a mover's mid-frame overlap) or behind it.
fn ahead_of(w: &World, id: ProjectileId, fighter_pass: bool) -> i32 {
    i32::from(fighter_pass || w.overlap_mid_frame || pr(w, id).age == 0)
}

/// `w.projectilesMoved`: a call behind the projectiles' movement of the frame.
fn projectiles_moved(w: &World) -> bool {
    w.scratch.step_at == 3
}

/// UE location of the saw.
fn loc(w: &World, id: ProjectileId) -> [Real; 3] {
    let p = pr(w, id);
    [0.0, -p.x, p.y]
}

// ------------------------------------------------------------------ the boxes' packet

/// The boxes' packet from the variable. Deviation: World.projectileHitDir does not read IgnoreFacing (X x the actor's
/// yaw sign); with IgnoreFacing the build's ComingFromLeft is false and the vector's X is UE Y (sim -x), so the packet
/// carries -X x yawX for the world's product to give -X.
fn saw_packet(w: &World, id: ProjectileId) -> HitPacket {
    let h = &saw(w, id).saw_hbd;
    let mut out = h.clone();
    if h.ignore_facing != Some(true) {
        return out;
    }
    let d = h.knock_back_direction_vector.unwrap_or(DataVec { x: Some(1.0), y: Some(0.0), z: None });
    out.knock_back_direction_vector =
        Some(DataVec { x: Some(-d.x.unwrap_or(0.0) * yaw_x(pr(w, id))), y: Some(d.y.unwrap_or(0.0)), z: None });
    out
}

/// Comp_Pawn_HitBoxHandler.UpdateHitBoxDamage([HitBox], HitBoxDamage, ...): the packet, the boxes on.
fn update_hit_box_damage(w: &mut World, id: ProjectileId) {
    let h = saw_packet(w, id);
    let p = pm(w, id);
    p.hit = h;
    if !p.dead {
        reflect_boxes_on(p);
    }
}

/// HitBoxDamage.KnockBackDirectionVector = (X, 1) (and IgnoreFacing true where the graph writes it).
fn write_kb(w: &mut World, id: ProjectileId, x: Real, ignore_facing: bool) {
    let h = &mut saw_mut(w, id).saw_hbd;
    h.knock_back_direction_vector = Some(DataVec { x: Some(x), y: Some(1.0), z: None });
    if ignore_facing {
        h.ignore_facing = Some(true);
    }
}

/// @6483-6845 / @8765-9105: X = Y == 0 ? the old X : (Y > 0 ? 1 : -1) of a travel direction (UE).
fn kb_from_travel(w: &mut World, id: ProjectileId, dir: [Real; 3]) {
    let y = dir[1];
    let old = saw(w, id).saw_hbd.knock_back_direction_vector.and_then(|d| d.x).unwrap_or(0.0);
    write_kb(
        w,
        id,
        if y == 0.0 {
            old
        } else if y > 0.0 {
            1.0
        } else {
            -1.0
        },
        true,
    );
}

// ------------------------------------------------------------------ what the bounce hit

/// The actor a hit at (x, y) is on, as an edge owner names it, with its shape.
struct BoxRef {
    r: Option<EdgeRef>,
    bx: StageShape,
}

/// The square of the distance from (x, y) to the shape (0 inside it): a polygon solid's by its nearest point.
fn rect_dist(b: &StageShape, x: Real, y: Real) -> Real {
    let b = match b {
        StageShape::Box(g) => g,
        StageShape::Poly(q) => {
            let (cx, cy) = q.closest(x, y);
            return (cx - x) * (cx - x) + (cy - y) * (cy - y);
        }
    };
    // the box is the collision's (double): the distance in double,
    let (x, y) = (f64::from(x), f64::from(y));
    let dx = if x < b.left {
        b.left - x
    } else if x > b.right {
        x - b.right
    } else {
        0.0
    };
    let dy = if y < b.bottom {
        b.bottom - y
    } else if y > b.top {
        y - b.top
    } else {
        0.0
    };
    crate::math::narrow(dx * dx + dy * dy)
}

/// What each box of `World.blockers(Nav)` is an edge owner as: the stage platforms, the ships' pieces, the spawned
/// platforms' parts, a projectile's boxes (none) and the stage solids, in the order `blockers` lists them; a box past
/// those (a run-time solid) has none.
fn nav_refs(w: &mut World) -> Vec<Option<EdgeRef>> {
    let mut v: Vec<Option<EdgeRef>> =
        w.stage.stage_platforms.iter().map(|p| Some(EdgeRef::Platform(p.stage_index))).collect();
    let mut tmp = Vec::new();
    let ships: Vec<_> = w.actors.ships.iter().filter(|(_, s)| !s.dead).map(|(id, s)| (id, s.seq)).collect();
    for (sh, seq) in ships {
        tmp.clear();
        crate::chars::morty::ship_boxes(w, sh, BlockKind::Nav, &mut tmp);
        v.extend(tmp.iter().map(|_| Some(EdgeRef::Actor(seq))));
    }
    for (_, a) in w.actors.actors.iter() {
        let Some(pl) = &a.plat else { continue };
        if a.dead {
            continue;
        }
        for s in pl.solids() {
            if s.on && s.nav {
                v.push(Some(EdgeRef::Actor(a.seq)));
            }
        }
    }
    for (_, p) in w.actors.projectiles.iter() {
        let Some(solids) = &p.solids else { continue };
        if p.dead || p.no_collision {
            continue;
        }
        for s in solids {
            if s.on && s.nav {
                v.push(None);
            }
        }
    }
    if w.stage_run.as_ref().is_some_and(|r| !r.solids.is_empty()) {
        let all = crate::fighter::traces::stage_solids(w);
        for s in &all.all {
            if let crate::fighter::traces::Solid::Box(_, b) = s {
                let run = w.stage_run.as_ref().expect("checked");
                if b.flags & crate::consts::SOLID_NAV != 0 && run.solids[b.solid as usize].enabled {
                    v.push(Some(EdgeRef::Solid(run.solids[b.solid as usize].id)));
                }
            }
        }
    }
    v
}

/// Deviation: the port's bounce does not carry its HitResult's actor; it is taken as the box of World.ground and
/// World.blockers('nav') (the boxes the move bounces on) nearest to the capsule at the hit; a polygon solid that blocks
/// the move after them, by its nearest point.
fn box_at(w: &mut World, x: Real, y: Real) -> BoxRef {
    let mut best = BoxRef { r: Some(EdgeRef::Floor), bx: StageShape::Box(w.stage.ground) };
    let mut bd = rect_dist(&best.bx, x, y);
    let all = w.blocker_shapes(BlockKind::Nav);
    let refs = nav_refs(w);
    for (i, b) in all.boxes.iter().enumerate() {
        let d = rect_dist(&StageShape::Box(*b), x, y);
        if !(d < bd) {
            continue;
        }
        bd = d;
        best = BoxRef { r: refs.get(i).copied().flatten(), bx: StageShape::Box(*b) };
    }
    for q in all.polys {
        let id = w.stage_run.as_ref().expect("a polygon solid with the stage's run").solids[q.solid as usize].id;
        let q = StageShape::Poly(q);
        let d = rect_dist(&q, x, y);
        if !(d < bd) {
            continue;
        }
        bd = d;
        best = BoxRef { r: Some(EdgeRef::Solid(id)), bx: q };
    }
    best
}

/// HitActor.GetComponentByClass(Comp_EdgeTrace_C) valid, its Tag equal to the rider's (every Tag is None) and the
/// actor not being destroyed (@3332-3831).
// Deviation: a level with a generator: the owners the port's polygon set holds (`edge_poly_of`; the two owners with no
// tagged box, ThroneRoomForegroundElevator and TreeHouseSmallUpperPlatformCollision, are not in it). A level without
// one (Classic 3 Platform): the stage platforms and the SpawnedPlatformParent children pass; Attach To Edge then
// finds no polygon (Generator None) and the saw stays where it stopped until its MaxFloorLife timer.
fn has_edge_trace(w: &World, id: ProjectileId, b: &BoxRef) -> bool {
    let Some(r) = b.r else { return false };
    if !saw(w, id).rider.generated {
        return matches!(r, EdgeRef::Platform(_) | EdgeRef::Actor(_));
    }
    let pid = edge_poly_of(w, r);
    pid >= 0 && edge_poly_live(w, pid)
}

// ------------------------------------------------------------------ the rider's host

/// The host's On Run Along Disabled (`EdgeHost.onDisabled`): bound to GroundTimeEvent.
pub fn on_disabled(w: &mut World, id: ProjectileId) {
    if saw(w, id).ground_bound && !pr(w, id).dead {
        ground_time_event(w, id);
    }
}

/// The host's On Turn Corner (`EdgeHost.onTurnCorner`): bound to UpdateHitOnTurnCorner; `dir` the direction after the
/// turn.
pub fn on_turn_corner(w: &mut World, id: ProjectileId, dir: [Real; 3]) {
    if saw(w, id).turn_bound && !pr(w, id).dead {
        update_hit_on_turn_corner(w, id, dir);
    }
}

// ------------------------------------------------------------------ its graph

/// GroundTimeEvent @7242 -> @727 (authority): K2_ClearAndInvalidateTimerHandle(MaxLifetimeTimer), K2_DestroyActor.
fn ground_time_event(w: &mut World, id: ProjectileId) {
    clear_max_lifetime(pm(w, id));
    destroy_projectile(w, id, false);
}

/// UpdateHitOnTurnCorner @8631 (bound to On Turn Corner @7067): the knockback X from the direction after the turn,
/// IgnoreFacing, UpdateHitBoxDamage.
fn update_hit_on_turn_corner(w: &mut World, id: ProjectileId, dir: [Real; 3]) {
    kb_from_travel(w, id, dir);
    update_hit_box_damage(w, id);
}

/// The bounce event from @3140 (BounceEvent @3086 after storing Impact Result / Velocity; StartOnGround @9297 with the
/// owner's floor). (nx, ny): the impact normal; (ivx, ivy): Impact Velocity (sim). True: K2_DestroyActor @4704.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn bounce_event(
    w: &mut World,
    id: ProjectileId,
    b: BoxRef,
    nx: Real,
    ny: Real,
    ivx: Real,
    ivy: Real,
    ahead: i32,
) -> bool {
    if !saw(w, id).can_wall_roll {
        return true;
    }
    if !saw(w, id).attach_gate {
        return false;
    }
    if has_edge_trace(w, id, &b) {
        // @3845 authority, push_flow 4719 (Attach), then the timer branch first
        if saw(w, id).can_set_timer {
            let life = cls_num(w, id, "MaxFloorLife");
            let s = saw_mut(w, id);
            s.can_set_timer = false;
            s.bounce_gate = false;
            // @3957-4528: the dot is the sign of UE vY (0 for vY 0: GetSafeNormal's zero, true). UE vY = -sim vx.
            s.initial_direction_left = !(ivx > 0.0);
            s.launch_bound = true; // bind HitByAlly -> HitLaunch @4570
            let p = pm(w, id);
            set_gravity_scale(p, 0.0);
            update_max_lifetime(p, life, ahead);
        }
        attach(w, id, &b, nx, ny, ivx, ivy);
    } else if !(max_lifetime_at(pr(w, id)) > 0) {
        // @4635: no MaxLifetimeTimer active -> UpdateMaxLifetimeWithAuthority(MaxLifetime)
        let life = cls_num(w, id, "MaxLifetime");
        update_max_lifetime(pm(w, id), life, ahead);
    }
    // @326: DoOnce (the first time only) of the bounce sound
    if !saw(w, id).bounce_sound_done && !pr(w, id).dead {
        saw_mut(w, id).bounce_sound_done = true;
        sound(w, id, name!("sfx_cha_C023b_sawblade_throw_bounce"));
    }
    false
}

/// Attach @4719.
fn attach(w: &mut World, id: ProjectileId, b: &BoxRef, nx: Real, ny: Real, ivx: Real, ivy: Real) {
    let fs = saw(w, id).floor_speed;
    {
        let s = saw_mut(w, id);
        s.rider.move_speed = f64::from(fs);
        // Multicast Toggle Projectile(false) -> DisableProjectile: AttachGate false, IsMovingOnGround true,
        // StopSimulating
        s.attach_gate = false;
        s.on_ground = true;
    }
    stop_simulating(pm(w, id));
    // @4796-5376: LineTraceSingleForObjects(location, location + ProjectVectorOnToVector(ImpactVelocity x dt,
    // Cross(ImpactNormal, (left ? 1 : -1, 0, 0))), [WorldStatic], ignoring the hit actor). ProjectVectorOnToVector of
    // a zero target: zero.
    let s = if saw(w, id).initial_direction_left { 1.0 } else { -1.0 };
    let (tx, ty) = (-ny * s, nx * s);
    let (mx, my) = (ivx * DELTA_SECONDS, ivy * DELTA_SECONDS);
    let tt = tx * tx + ty * ty;
    let k = if tt > 1e-8 { (mx * tx + my * ty) / tt } else { 0.0 };
    let (px, py) = (pr(w, id).x, pr(w, id).y);
    let (ex, ey) = (px + k * tx, py + k * ty);
    // Deviation: a level body is no actor the trace can ignore by name: a hit on the bounce's box (within 0.5 of it) is
    // dropped instead; the line trace is the sphere trace of radius 0.
    let mut at = [0.0, -ex, ey];
    let mut hits: Vec<TraceHit> = Vec::new();
    sphere_trace_multi_for_objects(w, px, py, ex, ey, 0.0, &[0.0], &[], Some(ActorRef::Projectile(id)), &mut hits);
    for h in &hits {
        if !h.blocking || rect_dist(&b.bx, h.ix, h.iy) <= 0.25 {
            continue;
        }
        at = [0.0, -h.ix, h.iy];
        break;
    }
    saw_mut(w, id).ground_bound = true; // bind On Run Along Disabled -> GroundTimeEvent @5519
    let cw = !saw(w, id).initial_direction_left;
    attach_to_edge(w, id, cw, at.map(f64::from), [0.0; 3], [0.0; 3], 0.0);
    if pr(w, id).dead {
        return;
    }
    let ctd = saw(w, id).rider.ctd;
    kb_from_travel(w, id, ctd.map(crate::math::narrow));
    update_hit_box_damage(w, id);
    saw_mut(w, id).turn_bound = true; // bind On Turn Corner -> UpdateHitOnTurnCorner @7067
    sound_start(w, id, name!("sfx_cha_C023b_sawblade_missle_loop_surface"), name!("sawsurf"));
}

/// StartOnGround @9297 (from SetInitialMovementMulticast @561): with StartAttachedToGround, the owner's
/// CharacterMovement CurrentFloor.HitResult and the launch velocity into the bounce event @3140.
// Deviation: CurrentFloor is taken as the box under the owner while he is grounded (his movement base, else the
// nearest box to him), its normal up (the boxes' tops); in the air the HitResult has no actor (the else branch @4635).
fn start_on_ground(w: &mut World, id: ProjectileId) -> bool {
    if !saw(w, id).start_attached {
        return false;
    }
    let f = owner(w, id);
    let grounded = crate::fighter::grounded(w, f);
    let base = if grounded { w.fighter(f).platforms.base } else { None };
    let b = if !grounded {
        BoxRef { r: None, bx: StageShape::Box(w.stage.ground) }
    } else if let Some(crate::fighter::body::PlatformRef::Stage(i)) = base {
        let sp = &w.stage.stage_platforms[i as usize];
        BoxRef {
            r: Some(EdgeRef::Platform(sp.stage_index)),
            bx: StageShape::Box(Ground {
                left: f64::from(sp.left),
                right: f64::from(sp.right),
                bottom: f64::from(sp.bottom),
                top: f64::from(sp.top),
            }),
        }
    } else {
        let (x, y) = (w.fighter(f).body.x, w.fighter(f).body.y);
        box_at(w, x, y)
    };
    let (vx, vy) = (pr(w, id).vx, pr(w, id).vy);
    bounce_event(w, id, b, 0.0, 1.0, vx, vy, 1)
}

/// HitEnemyPlayer @2235 (no parent call; authority, BounceGate): MaxLifetime again, the velocity
/// (0, BounceOnHitVelocity.Y x (Velocity.Y >= 0 ? -1 : 1), Z) through SetVelocityMulticast @2945, CanSetTimer, the hit
/// box off (HitBoxOn? false, UpdateHitbox), HitByAlly -> HitLaunch, the air loop's stop.
fn hit_enemy_player(w: &mut World, id: ProjectileId, _f: FighterId) {
    if !saw(w, id).bounce_gate || pr(w, id).dead {
        return;
    }
    let ahead = ahead_of(w, id, false);
    let life = cls_num(w, id, "MaxLifetime");
    let bv = obj(class_p(w, pr(w, id)), "BounceOnHitVelocity");
    let (by, bz) = (num_of(bv, "Y"), num_of(bv, "Z"));
    let p = pm(w, id);
    update_max_lifetime(p, life, ahead);
    // UE Y = -sim vx: Velocity.Y >= 0 is vx <= 0, and the new UE Y is then -BounceY (sim +BounceY)
    p.vx = if !(p.vx > 0.0) { by } else { -by };
    p.vy = bz;
    set_hitbox(p, false);
    let s = saw_mut(w, id);
    s.can_set_timer = true;
    s.launch_bound = true;
    sound_stop(w, id, name!("sawair"), Some(name!("sfx_cha_C023b_sawblade_missle_loop_air_stop")));
}

fn num_of(o: Option<&mvs_data::value::Props>, k: &str) -> Real {
    o.and_then(|o| num(o, k)).unwrap_or(0.0)
}

/// HitLaunch's attacker and its hit's AttackID, where it implements BPI_C023B (Pawn_C023B, C023B_SpawnedGunShot);
/// `None` otherwise.
// Unverified: a fighter's AttackID is the one of his active hit that has met this saw (the hit's hitSet), else his
// attack count.
fn attacker_of(w: &World, id: ProjectileId, by: Option<ActorRef>) -> Option<(Option<FighterId>, i32)> {
    match by? {
        ActorRef::Fighter(f) => {
            super::state::stripe_of(w, f)?;
            let k = 1000 + pr(w, id).seq;
            let hit = w.fighter(f).montages.active_hits.values().find(|h| h.hit_set.contains(&k));
            Some((Some(f), hit.map_or(w.fighter(f).decay.attack_count, |h| h.attack_id)))
        }
        ActorRef::Projectile(g) => {
            let p = w.actors.projectiles.get(g)?;
            matches!(p.ext, ProjectileExt::GunShot(_)).then_some((None, p.attack_id))
        }
        _ => None,
    }
}

/// HitLaunch @7247 (bound to Comp_Pawn_DamageManager.HitByAlly): attacker not self, implementing BPI_C023B, with an
/// AttackID other than the stored one.
fn hit_launch(w: &mut World, id: ProjectileId, dx: Real, dy: Real, by: Option<ActorRef>) {
    let Some((att, att_id)) = attacker_of(w, id, by) else { return };
    if att_id == saw(w, id).saw_attack_id {
        return;
    }
    {
        let s = saw_mut(w, id);
        s.saw_attack_id = att_id;
        s.can_set_timer = true;
        s.bounce_gate = true;
    }
    // A fighter's hit in the fighters' pass is ahead of the saw's move; one in the hit tests behind the moves is
    // behind it.
    let ahead = ahead_of(w, id, matches!(by, Some(ActorRef::Fighter(_))) && !projectiles_moved(w));
    let life = cls_num(w, id, "MaxLifetime");
    if saw(w, id).on_ground {
        // @283: UpdateMaxLifetimeWithAuthority(MaxLifetime), Reflected()
        update_max_lifetime(pm(w, id), life, ahead);
        reflected(w, id);
        return;
    }
    {
        let p = pm(w, id);
        set_gravity_scale(p, 3.0);
        update_max_lifetime(p, life, ahead);
        // SetVelocityMulticast((0, KB.X, KB.Y) x 1500): the packet's X after pfgDoDamage's facing step is -dx of the
        // world's direction, and UE Y is -sim x: vx = dx x 1500.
        p.vx = dx * HIT_LAUNCH_SPEED;
        p.vy = dy * HIT_LAUNCH_SPEED;
    }
    saw_mut(w, id).can_wall_roll = true;
    // @7738-7789: the attacker's Comp_C023B_TargetTracker.HitProjectile(self) (Accessed None for the gun shot)
    if let Some(f) = att {
        super::gun::tracker_hit_projectile(w, f, id);
    }
    set_hitbox(pm(w, id), true);
    sound(w, id, name!("sfx_cha_C023b_sawblade_thorns_hit"));
    sound_start(w, id, name!("sfx_cha_C023b_sawblade_missle_loop_air"), name!("sawair"));
}

/// Reflected @9756 (the class's override of SpawnedActorParent's empty Reflected; also HitLaunch's @306): on an edge
/// only, the knockback X by Clockwise, UpdateHitBoxDamage, Detach From Edge, Rotate VFX (the view's), Delay(0.05).
fn reflected(w: &mut World, id: ProjectileId) {
    if !saw(w, id).on_ground {
        return;
    }
    let cw = saw(w, id).rider.clockwise;
    write_kb(w, id, if cw { 1.0 } else { -1.0 }, false);
    update_hit_box_damage(w, id);
    detach_from_edge(w, id);
    saw_delay(w, id, N_REFLECT, 0.05);
}

/// @15: CurrentVFXTimeDilation == 1 (MulticastReflect's pause at 0.5 while the reflect's pause is 2) -> Attach To
/// Edge(!Clockwise, the location) @142; else Delay(0.0) @228 back here.
fn resume_at_15(w: &mut World, id: ProjectileId) {
    if pr(w, id).reflect.as_ref().is_some_and(|r| r.pause == 2) {
        saw_delay(w, id, N_POLL, 0.0);
        return;
    }
    let cw = !saw(w, id).rider.clockwise;
    let at = loc(w, id);
    attach_to_edge(w, id, cw, at.map(f64::from), [0.0; 3], [0.0; 3], 0.0);
}

/// Its latent actions: a Delay node pending is not made again; one made while its own actions run (`passing`) counts
/// from the next frame's, one made earlier in the frame from this one (AActor::Tick's ProcessLatentActions behind
/// ReceiveTick). One made behind its tick (Reflected from a fighter's hit in the hit tests behind the moves) when that
/// tick found none pending: the frame's leftover pass counts it at once; when it found one, from the next frame's
/// tick.
fn saw_delay(w: &mut World, id: ProjectileId, node: i32, secs: Real) {
    if saw(w, id).saw_pend.iter().any(|p| p.node == node) {
        return;
    }
    let tick = w.flow.tick;
    let moved = projectiles_moved(w);
    let s = saw_mut(w, id);
    let own = s.passing;
    if own || !moved || s.latent_at == tick {
        s.saw_pend.push(EdgePend { node, left: f64::from(secs), made: tick, own });
        return;
    }
    // the leftover pass: ticked now and marked (a later Delay of this frame waits for the next tick)
    s.latent_at = tick;
    let left = secs - DELTA_SECONDS;
    if left > 0.0 {
        s.saw_pend.push(EdgePend { node, left: f64::from(left), made: tick, own: true });
        return;
    }
    let prev = std::mem::replace(&mut s.passing, true);
    resume_at_15(w, id);
    saw_mut(w, id).passing = prev;
}

fn saw_latent(w: &mut World, id: ProjectileId) {
    if saw(w, id).saw_pend.is_empty() {
        return;
    }
    let tick = w.flow.tick;
    let s = saw_mut(w, id);
    s.latent_at = tick;
    let prev = std::mem::replace(&mut s.passing, true);
    let nodes: Vec<i32> = s.saw_pend.iter().map(|p| p.node).collect();
    for node in nodes {
        if pr(w, id).dead {
            break;
        }
        let s = saw_mut(w, id);
        let Some(i) = s.saw_pend.iter().position(|p| p.node == node) else { continue };
        if s.saw_pend[i].own && s.saw_pend[i].made == tick {
            continue;
        }
        s.saw_pend[i].left = f64::from(crate::math::narrow(s.saw_pend[i].left - f64::from(DELTA_SECONDS)));
        if s.saw_pend[i].left > 0.0 {
            continue;
        }
        s.saw_pend.remove(i);
        resume_at_15(w, id); // both nodes resume at @15 (@321 goto 15)
    }
    saw_mut(w, id).passing = prev;
}

/// BPI_SetMobilityToStationary @1351 (after the parent's count): on an edge with the count above 0, PausedEdgeAttach,
/// PausedClockwise and Detach From Edge; off an edge at count 0 with PausedEdgeAttach, ToggleVFX and Attach To
/// Edge(PausedClockwise, the location) @1676.
// Deviation: the count is read for a change in the frame's leftover pass instead of at the call (the reflect pause's
// calls come from reflectTick, behind the actor's tick: its own Delay poll above has detached it by then, so the pause
// takes neither branch).
fn stationary_tick(w: &mut World, id: ProjectileId) {
    let n = pr(w, id).stationary_count.unwrap_or(0);
    if n == saw(w, id).seen_stationary {
        return;
    }
    saw_mut(w, id).seen_stationary = n;
    if saw(w, id).rider.active {
        if n > 0 {
            let s = saw_mut(w, id);
            s.paused_edge_attach = true;
            s.paused_clockwise = s.rider.clockwise;
            detach_from_edge(w, id);
        }
        return;
    }
    if n != 0 || !saw(w, id).paused_edge_attach {
        return;
    }
    let (cw, at) = (saw(w, id).paused_clockwise, loc(w, id));
    attach_to_edge(w, id, cw, at.map(f64::from), [0.0; 3], [0.0; 3], 0.0);
    saw_mut(w, id).paused_edge_attach = false;
}

/// BPI_ReverseMoveDirection @975: on an edge, ToggleVFX and Attach To Edge(!Clockwise, the location); off one, the
/// parent's (the active movement's velocity flipped), then PausedClockwise flipped with PausedEdgeAttach.
pub fn reverse_move(w: &mut World, id: ProjectileId) {
    if !matches!(w.actors.projectiles.get(id).map(|p| &p.ext), Some(ProjectileExt::Saw(_))) || pr(w, id).dead {
        return;
    }
    if saw(w, id).rider.active {
        let (cw, at) = (!saw(w, id).rider.clockwise, loc(w, id));
        attach_to_edge(w, id, cw, at.map(f64::from), [0.0; 3], [0.0; 3], 0.0);
        return;
    }
    crate::combat::reverse::flip_active_velocity(w, ActorRef::Projectile(id));
    let s = saw_mut(w, id);
    if s.paused_edge_attach {
        s.paused_clockwise = !s.paused_clockwise;
    }
}

// ------------------------------------------------------------------ the kind

/// The constructor's tail: the class's variables from the data; `sawHbd` is `hit`.
fn construct(w: &World, p: &mut Projectile) {
    let d = class_p(w, p);
    let t = |k: &str| truthy(d.get(k));
    let start_attached = t("StartAttachedToGround");
    let s = Saw {
        rider: EdgeRider::default(),
        saw_hbd: p.hit.clone(),
        attach_gate: t("AttachGate"),
        bounce_gate: t("BounceGate"),
        can_set_timer: t("CanSetTimer"),
        can_wall_roll: t("CanWallRoll"),
        on_ground: start_attached,
        initial_direction_left: false,
        floor_speed: num(d, "FloorSpeed").unwrap_or(0.0),
        max_floor_speed: 0.0,
        saw_attack_id: num(d, "Attack ID").unwrap_or(0.0) as i32,
        paused_edge_attach: false,
        paused_clockwise: false,
        seen_stationary: 0,
        launch_bound: false,
        ground_bound: false,
        turn_bound: false,
        bounce_sound_done: false,
        start_attached,
        saw_pend: Vec::new(),
        latent_at: -1,
        lister: None,
        passing: false,
    };
    p.ext = ProjectileExt::Saw(s);
}

/// ReceiveBeginPlay @1773 (Comp_RunAlongEdgeTrace's BeginPlay ahead of the actor's): OnProjectileBounce ->
/// BounceEvent (the kind's bounce_impact), and without CanWallRoll HitByAlly -> HitLaunch @2170. Then
/// SetInitialMovementMulticast @8087: MaxFloorSpeed = FloorSpeedMult x InitialSpeed, FloorSpeed = it, StartOnGround,
/// the parent's (Velocity written again: StopSimulating's zero undone, the movement still stopped), the air loop.
fn begin(w: &mut World, id: ProjectileId) {
    rider_begin_play(w, id);
    if !saw(w, id).can_wall_roll {
        saw_mut(w, id).launch_bound = true;
    }
    let f = owner(w, id);
    if super::state::stripe_of(w, f).is_some() {
        super::state::stripe_mut(w, f).expect("a Stripe").saws.push(id);
        saw_mut(w, id).lister = Some(f);
    }
    let mfs = cls_num(w, id, "FloorSpeedMult") * pm_initial_speed(pr(w, id));
    {
        let s = saw_mut(w, id);
        s.max_floor_speed = mfs;
        s.floor_speed = mfs;
    }
    let (vx, vy) = (pr(w, id).vx, pr(w, id).vy);
    if start_on_ground(w, id) {
        destroy_projectile(w, id, false);
        return;
    }
    let p = pm(w, id);
    p.vx = vx;
    p.vy = vy;
    sound_start(w, id, name!("sfx_cha_C023b_sawblade_missle_loop_air"), name!("sawair"));
}

/// BounceEvent @3086 (bound to OnProjectileBounce @2091): the capsule at the hit is the box's pick.
fn bounce_impact(w: &mut World, id: ProjectileId, nx: Real, ny: Real, ivx: Real, ivy: Real) -> bool {
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    let b = box_at(w, x, y);
    bounce_event(w, id, b, nx, ny, ivx, ivy, 0)
}

fn step(w: &mut World, id: ProjectileId) -> bool {
    saw_latent(w, id);
    false
}

fn hit_by(
    w: &mut World,
    id: ProjectileId,
    ally: bool,
    dx: Real,
    dy: Real,
    _bkb: Real,
    _type: i32,
    by: Option<ActorRef>,
) {
    if ally && saw(w, id).launch_bound && !pr(w, id).dead {
        hit_launch(w, id, dx, dy, by);
    }
}

/// HitAlly @8165: with CanWallRoll, the owner's Comp_Spawner.CompSpawnActor(C023B_AllySaw, attached to the ally)
/// @8246, then K2_DestroyActor.
fn hit_ally(w: &mut World, id: ProjectileId, vic: FighterId) {
    if !saw(w, id).can_wall_roll || pr(w, id).dead {
        return;
    }
    let o = owner(w, id);
    spawn_ally_saw(w, o, vic);
    destroy_projectile(w, id, false);
}

fn end(w: &mut World, id: ProjectileId) {
    if let Some(l) = saw(w, id).lister
        && let Some(sp) = super::state::stripe_mut(w, l)
        && let Some(i) = sp.saws.iter().position(|&s| s == id)
    {
        sp.saws.remove(i);
    }
    sound_stop(w, id, name!("sawair"), None);
    sound_stop(w, id, name!("sawsurf"), None);
    // OnEndPlay.Broadcast after ReceiveEndPlay: the trackers that bound it (Comp_C023B_TargetTracker OnTargetDeath).
    super::gun::tracker_target_ended(w, id);
}

fn proj_of(a: ActorRef) -> ProjectileId {
    match a {
        ActorRef::Projectile(id) => id,
        _ => panic!("a saw hook called with another actor"),
    }
}

/// BPI_Reflected scales the variable and gives the boxes the scaled copy (@4749-4931).
fn reflect_hbd(w: &mut World, a: ActorRef) -> &mut HitPacket {
    &mut saw_mut(w, proj_of(a)).saw_hbd
}

fn reflect_reflected(w: &mut World, a: ActorRef) {
    let id = proj_of(a);
    let (d, b) = (pr(w, id).hit.damage, pr(w, id).hit.base_knock_back);
    let h = &mut saw_mut(w, id).saw_hbd;
    h.damage = d;
    h.base_knock_back = b;
    let pk = saw_packet(w, id);
    pm(w, id).hit = pk;
    reflected(w, id);
}

/// (the packet's yaw step after MulticastReflect's owner and turn)
fn reflect_multicast(w: &mut World, a: ActorRef, _new_owner: FighterId, _pause: bool) {
    let id = proj_of(a);
    let pk = saw_packet(w, id);
    pm(w, id).hit = pk;
}

fn saw_kind() -> ProjectileHooks {
    ProjectileHooks {
        construct: Some(construct),
        begin: Some(begin),
        step: Some(step),
        bounce_impact: Some(bounce_impact),
        hit_enemy: Some(hit_enemy_player),
        hit_by: Some(hit_by),
        hit_ally: Some(hit_ally),
        reverse_move: Some(reverse_move),
        end: Some(end),
        reflect: ReflectHooks {
            reflected: Some(reflect_reflected),
            multicast: Some(reflect_multicast),
            hit_box_damage: Some(reflect_hbd),
            ..ReflectHooks::default()
        },
        ..ProjectileHooks::default()
    }
}

// ------------------------------------------------------------------ C023B_AllySaw

/// The MaxLifetimeTimer's ticks for `life` seconds (the first k with k x DELTA_SECONDS > life; 0: none).
fn life_ticks(life: Real) -> i32 {
    let t = life;
    if !(t > 0.0) {
        return 0;
    }
    let mut k = 0;
    while !(crate::consts::clock(crate::real(k)) > f64::from(t)) {
        k += 1;
    }
    k
}

fn ally_of(w: &World, id: ActorId) -> Option<&AllySaw> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::AllySaw(s) => Some(s),
        _ => None,
    }
}

fn ally_mut(w: &mut World, id: ActorId) -> Option<&mut AllySaw> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::AllySaw(s) => Some(s),
        _ => None,
    }
}

/// The C023B_AllySaw_C's place (its `x`, `y`).
pub fn place(_w: &World, _a: &WorldActor, s: &AllySaw) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// The C023B_AllySaw_C as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn dealer_parts(_w: &World, _a: &WorldActor, s: &AllySaw) -> (Real, Real, Real, i32) {
    (s.x, s.y, if s.ox > 0.0 { 1.0 } else { -1.0 }, s.attack_id)
}

/// The C023B_AllySaw_C's HitBoxDamage packet (`hit`).
pub fn hit(s: &AllySaw) -> &HitPacket {
    &s.hit
}

/// `spawnAllySaw`: CompSpawnActor with the ally as the attach parent and the identity transform; NumberOfActorsAllowed
/// 1, MaxLifetime 5, Die With Instigator.
fn spawn_ally_saw(w: &mut World, owner: FighterId, ally: FighterId) {
    let data = std::sync::Arc::clone(&w.data);
    let cls = name!("C023B_AllySaw_C");
    let Some(d) = actor_data(w, &data, ActorRef::Fighter(owner), cls) else {
        warn_text(w, &format!("{ALLY_SAW} not exported"));
        return;
    };
    // The parent's NumberOfActorsAllowed (the oldest of the owner's past the count goes): add_actor.
    actor_spawn_ability(w, owner, &d.p);
    let radius = obj(&d.components, "HitBox2D")
        .and_then(|c| num(c, "CapsuleRadius"))
        .or_else(|| obj(&d.components, "HitBox").and_then(|c| num(c, "SphereRadius")))
        .unwrap_or(50.0);
    let none = mvs_data::value::Props::new();
    let lf = facing(w, ally);
    let mut s = AllySaw {
        ally,
        attack_id: w.fighter(owner).decay.attack_count,
        hit: HitPacket::from_props(&w.names, obj(&d.p, "HitBoxDamage").unwrap_or(&none), None),
        x: 0.0,
        y: 0.0,
        ox: ORBIT_R * if lf < 0.0 { -1.0 } else { 1.0 },
        oy: 0.0,
        last_facing: lf,
        die_at: w.flow.tick + life_ticks(num(&d.p, "MaxLifetime").unwrap_or(0.0)),
        hit_done: false,
        radius,
        hit_set: IndexSet::new(),
        sphere_x: 0.0,
        sphere_y: 0.0,
        sphere_r: 0.0,
    };
    s.x = w.fighter(ally).body.x;
    s.y = actor_y(w, ally);
    let actor = new_world_actor(w, cls, WorldActorClass::C023BAllySaw, owner, false, WorldActorState::AllySaw(s));
    let id = w.actors.actors.insert(actor);
    let charge = instigator_charge(w, owner);
    comp_spawn_actor(w, id, charge, owner);
}

/// ReceiveBeginPlay @1488: buff_start at the blade, the buff loop.
fn ally_begin(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let Some(s) = ally_of(w, id) else { return };
    let (fid, seq, x, y) = (a.owner_id, a.seq, s.x + s.ox, s.y + s.oy);
    w.events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_C023b_sawblade_thorns_buff_start"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    w.events.push(Event::SoundStart {
        fighter: fid,
        ev: name!("sfx_cha_C023b_sawblade_thorns_buff_loop"),
        key: key(name!("allysaw"), seq),
    });
}

fn ally_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    if a.dead {
        return;
    }
    let Some(ally) = ally_of(w, id).map(|s| s.ally) else { return };
    let (ax, ay, af) = (w.fighter(ally).body.x, actor_y(w, ally), facing(w, ally));
    let tick = w.flow.tick;
    let (cos, sin) = (det::ue::cosf(orbit_step()), det::ue::sinf(orbit_step()));
    let s = ally_mut(w, id).expect("checked");
    s.x = ax;
    s.y = ay;
    if af != s.last_facing {
        s.last_facing = af;
        s.ox = -s.ox;
    }
    let (x, y) = (s.ox, s.oy);
    s.ox = x * cos - y * sin;
    s.oy = x * sin + y * cos;
    if s.die_at > 0 && tick >= s.die_at {
        // OnMaxLifetimeExpired -> DestroySpawnedActor: its 'Ak Death Location'
        let (px, py) = (s.x, s.y);
        let a = w.actors.actors.get_mut(id).expect("checked");
        a.dead = true;
        let fid = a.owner_id;
        let ev = text(actor_p(w, ActorRef::Actor(id)), "Ak Death Location").map(str::to_owned);
        if let Some(ev) = ev {
            let ev = crate::registry::props::name(w, &ev);
            w.events.push(Event::Sound {
                fighter: fid,
                ev,
                x: Some(px),
                y: Some(py),
                hit: None,
                pooled: false,
                key: None,
            });
        }
    }
}

/// HitBox2D (object type HitBox) at the blade, `None` once the hit has ended it.
fn ally_sphere(w: &World, id: ActorId) -> Option<WorldSphere> {
    let a = w.actors.actors.get(id)?;
    let s = ally_of(w, id)?;
    if a.dead || s.hit_done {
        return None;
    }
    let g = a.size_scale.unwrap_or(1.0);
    Some(WorldSphere { x: s.x + s.ox * g, y: s.y + s.oy * g, z: Some(0.0), r: s.radius * g, ..WorldSphere::default() })
}

fn ally_hit_shapes(w: &World, id: ActorId, out: &mut Vec<WorldSphere>) -> bool {
    match ally_sphere(w, id) {
        Some(sp) => {
            out.push(sp);
            true
        }
        None => false,
    }
}

/// The HitBox2D circle against the enemies' hurt shapes, pfgDoDamage's gates in World.projectileTest's order; the
/// first hit ends it (K2_DestroyActor in HitEnemyPlayer).
fn ally_hits(w: &mut World, id: ActorId) {
    let Some(sphere) = ally_sphere(w, id) else { return };
    let me = ActorRef::Actor(id);
    {
        let s = ally_mut(w, id).expect("checked");
        (s.sphere_x, s.sphere_y, s.sphere_r) = (sphere.x, sphere.y, sphere.r);
    }
    let o = w.actors.actors.get(id).expect("checked").owner;
    let team = w.fighter(o).team.team;
    let (attack_id, hit) = {
        let s = ally_of(w, id).expect("checked");
        (s.attack_id, s.hit.clone())
    };
    for vic in match_fighters(w) {
        let vid = w.fighter(vic).id;
        if w.fighter(vic).team.team == team
            || !alive(w, vic)
            || no_hit(w, vic)
            || ally_of(w, id).expect("checked").hit_set.contains(&vid)
        {
            continue;
        }
        let Some(vp) = victim_pose(w, vic) else { continue };
        let met = hurt_hit(w, vic, std::slice::from_ref(&sphere), &vp).is_some();
        give_vbuf(w, vp);
        if !met {
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
        dodge_over(w, me, vic);
        if hit.hit_each_actor_only_once.unwrap_or(false) {
            ally_mut(w, id).expect("checked").hit_set.insert(vid);
        }
        let d = dealer(w, me);
        let rows = crate::combat::spawned_hits::class_rows(w, me);
        if apply_projectile_hit(w, &d, vic, &hit, &rows, V2 { x: sphere.x, y: sphere.y }, None, &[], charge_pct(w, me))
        {
            actor_hit_enemy(w, &d, vic);
            ally_hit_enemy(w, id);
            return;
        }
    }
}

/// HitEnemyPlayer @1692 -> @358 (DoOnce) -> @394: left = the owner's IsFacingLeft, ownerLeft = owner UE Y >= this UE Y;
/// (left AND ownerLeft) OR (!ownerLeft AND !left) -> (0, 0.5, 1), else (0, -0.5, 1); CompSpawnActor(FromAlly, world
/// transform at the actor's location, that direction, 1500, 1500, AttackID 0: the owner's count) @1149;
/// K2_DestroyActor; the sounds @1278.
// (comp_spawn_projectile mirrors the direction by the owner's facing (unverified there): the pick above, which turns on
// the same facing, then always sends it toward the owner.)
fn ally_hit_enemy(w: &mut World, id: ActorId) {
    let (o, fid) = {
        let a = w.actors.actors.get(id).expect("checked");
        (a.owner, a.owner_id)
    };
    let (ax, ay) = {
        let s = ally_mut(w, id).expect("checked");
        s.hit_done = true;
        (s.x, s.y)
    };
    let left = facing(w, o) < 0.0;
    let owner_left = w.fighter(o).body.x <= ax;
    let y = if (left && owner_left) || (!owner_left && !left) { 0.5 } else { -0.5 };
    comp_spawn_projectile(
        w,
        o,
        name!("C023B_SawBlade_FromAlly_EdgeTrace_C"),
        ax,
        ay,
        [Some(0.0), Some(y), Some(1.0)],
        1500.0,
        1500.0,
        None,
    );
    let a = w.actors.actors.get_mut(id).expect("checked");
    a.dead = true;
    let (seq, g) = (a.seq, a.size_scale.unwrap_or(1.0));
    let s = ally_of(w, id).expect("checked");
    let (bx, by) = (s.x + s.ox * g, s.y + s.oy * g);
    w.events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_C023b_sawblade_thorns_hit"),
        x: Some(bx),
        y: Some(by),
        hit: None,
        pooled: false,
        key: None,
    });
    w.events.push(Event::SoundStop {
        fighter: fid,
        key: key(name!("allysaw"), seq),
        fade: None,
        ev: Some(name!("sfx_cha_C023b_sawblade_thorns_buff_loop_stop")),
        curve: None,
        cut: false,
    });
}

fn ally_end(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let e = Event::SoundStop {
        fighter: a.owner_id,
        key: key(name!("allysaw"), a.seq),
        fade: None,
        ev: None,
        curve: None,
        cut: false,
    };
    w.events.push(e);
}

// ------------------------------------------------------------------ entries

/// saw.rs's part of the joined timerTick (after gun.rs's, before bomb.rs's): the leftover latent pass (the first
/// Stripe of the match, for every live saw, whoever owns it now): the saws' stationary reads, then their riders'
/// Delays.
// Deviation: a saw with no Stripe in the match (a Disguise copy of his throw) gets neither.
pub fn timer_tick(w: &mut World, f: FighterId) {
    let first = match_fighters(w).into_iter().find(|&x| super::state::stripe_of(w, x).is_some());
    if first != Some(f) {
        return;
    }
    for id in live_saws(w) {
        if pr(w, id).dead {
            continue;
        }
        stationary_tick(w, id);
        if !pr(w, id).dead {
            rider_latent(w, id);
        }
    }
}

/// This file's registry entries (the saw classes, C023B_AllySaw_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let enter = |r: &mut Registry, cls: &'static str, class: ProjectileClass| {
        r.actors.enter(cls, ActorEntry::new(ActorClass::Projectile(class), ActorHooks::Projectile(saw_kind())))
    };
    enter(r, SAW, ProjectileClass::C023BSawBladeEdgeTrace)?;
    // a data child with no graph: the parent's kind by name
    enter(r, SAW_ON_GROUND, ProjectileClass::C023BSawBladeEdgeTraceStartOnGround)?;
    enter(r, SAW_FROM_ALLY, ProjectileClass::C023BSawBladeFromAllyEdgeTrace)?;
    let ally = WorldActorHooks {
        begin_play: Some(ally_begin),
        // allySawShapes reads the scale; the view too
        size: Some(SizeHooks::default()),
        hit_shapes: Some(ally_hit_shapes),
        step: Some(ally_step),
        hits: Some(ally_hits),
        end: Some(ally_end),
        ..WorldActorHooks::default()
    };
    r.actors.enter(ALLY_SAW, ActorEntry::new(ActorClass::World(WorldActorClass::C023BAllySaw), ActorHooks::World(ally)))
}
