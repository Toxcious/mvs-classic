//! Ground down: SetGroundPositions @7917 -> @4316 (pawn.rs forwards here),
//! the Tick's probes @7927-8580 and Do Ground Trace ([`ground_tick`]), AnimNotify_GarnetStartGroundChecking and
//! GarnetShockwaveHitBox_C (+ Left / Right) as [`ShockwaveHitBox`] (a DealerActor, a world actor).
//!
//! GarnetShockwaveHitBox (Left and Right are data children with no graph: Left MoveSpeed 4500 and its own HitBoxDamage,
//! Right IsMovingRight). A SpawnedActorParent child, spawned by Garnet_Attack_Ground_D's Spawners (no socket, no attach:
//! the actor stays at the spawn place, the owner's location - (0, 0, 100)).
//! - BeginPlay @687 (authority): StartTime = GetGameTimeInSeconds. An Instigator that does not implement BPI_Garnet:
//!   nothing more (no timeline: the HitBox stays off; the actor lives until Die With Instigator or the arena ends it).
//!   Else Locations = IsMovingRight ? RightPositions : LeftPositions (BPI_GetGroundPositions: copies), TotalDistance =
//!   Locations non-empty ? Vector_Distance(the actor location, the last one) : 0, MoveHitboxesTimeline.PlayFromStart.
//! - MoveHitboxesTimeline (length 1.0, the Alpha track linear 0 -> 1 over 0 .. 1), its update @1501: p = Alpha x
//!   MoveSpeed / TotalDistance (Kismet's divide: 0 by 0). p < 1: HitBox.K2_SetWorldLocation(Locations[trunc(p x len)],
//!   sweep) and (authority, a DoOnce @456) HitBoxOn? = true, UpdateHitbox (the handler's HitBoxesOn). p >= 1:
//!   StopMovement (the timeline stopped) and the end gate. The timeline's Finished @2317 goes to the end gate too.
//! - The end gate (a DoOnce @380 -> @30): HitBox collision off; then ReleaseToPool (CanBePooled false: K2_DestroyActor)
//!   once GetGameTimeInSeconds - StartTime >= MinLifeTime (0.3), else after Delay(MinLifeTime - elapsed) @321.
//!
//! The timeline: PlayFromStart's update at 0 inside BeginPlay (stock UTimelineComponent; unverified), then its component
//!   ticks from the spawn frame (TG_NewlySpawned), position + DeltaSeconds in float32, held at the length where the last
//!   update and Finished fire.
// Unverified: Locations empty (a stale or empty array: SetGroundPositions leaves the arrays when its FindFloor misses, a
// first probe that misses leaves one empty) gives TotalDistance 0, p = 0 and Locations[0] out of range:
// EX_ArrayGetByRef leaves the call's zeroed parameter, so the HitBox goes to the world origin and stays there, on, until
// Finished at 1.0 s (the VM's out-of-range read taken from stock UE 4.26 ScriptCore, not read in this exe).
//!
//! For the drawing (the view): (`x`, `y`) the actor, (`hx`, `hy`) the HitBox (its VFX PS_Garnet_NAD_Shockwave is attached
//! to it), `hit_on` its collision, `facing` the owner's at BeginPlay.

use indexmap::IndexSet;

use super::state::{garnet, garnet_of, garnet_w};
use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::WorldSphere;
use crate::combat::elem::charge_pct;
use crate::combat::hit::hurt_hit;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::phases::{alive, give_vbuf, los_blocked, victim_pose};
use crate::combat::spawned_hits::class_rows;
use crate::consts::{DELTA_SECONDS, HITPAUSE_DILATION};
use crate::events::NotifyRef;
use crate::fighter::common::{actor_data, latent_frames};
use crate::fighter::control::{attack_dodged, dodge_over, pawn_prop};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::tick::FrameMode;
use crate::fighter::traces::{OBJ_PLATFORM, OBJ_WORLD_STATIC, TraceHit, sphere_trace_multi_for_objects};
use crate::fighter::{FighterId, actor_y, invulnerable};
use crate::math::ue::{dist2d, size2d, trunc_to_int};
use crate::math::{self, to_int32};
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, obj};
use crate::registry::{NotifyHandler, Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::pawns::match_fighters;
use crate::world::spawn_spot::{spawn_place, spawn_spot};
use crate::world::spawning::{SpawnSpotA, SpawnSpotR, request_props};

const SHOCKWAVE: &str = "GarnetShockwaveHitBox_C";
const SHOCKWAVE_LEFT: &str = "GarnetShockwaveHitBoxLeft_C";
const SHOCKWAVE_RIGHT: &str = "GarnetShockwaveHitBoxRight_C";

// ---------------------------------------------------------------- the probes on the pawn

/// BPI_SetGroundPositions @7917 -> @4316 (authority): CharacterMovement.K2_FindFloor at the actor location; with a valid
/// HitActor both arrays cleared, both probes put at the actor location and both turned on. pawn.rs's
/// set_ground_positions (the BPI) calls this; so does the notify below (its BPI_SetGroundPositions on a BPI_Garnet owner).
// Unverified: K2_FindFloor is taken as the fighter's grounded state (the port has no FindFloor query of its own: a
// falling Garnet within the floor sweep's reach of a top would find it in the build). Every montage that names the
// notify is a ground attack (Garnet_Attack_Ground_D_Montage(_v3), at t 0.0001).
pub fn set_ground_positions_body(w: &mut World, f: FighterId) {
    if garnet_of(w, f).is_none() || !crate::fighter::grounded(w, f) {
        return;
    }
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
    let s = &mut garnet_w(w, f).ground;
    s.left_positions.clear();
    s.right_positions.clear();
    s.left_x = x;
    s.right_x = x;
    s.left_y = y;
    s.right_y = y;
    s.checking_left = true;
    s.checking_right = true;
}

/// AnimNotify_GarnetStartGroundChecking: Received_NotifyBegin, the mesh owner's BPI_SetGroundPositions when it implements
/// BPI_Garnet (another owner: nothing; Black Adam's own notify is AnimNotify_C021StartGroundChecking).
fn start_ground_checking(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    set_ground_positions_body(w, f);
}

/// Do Ground Trace(Location) (Pawn_Garnet function): SphereTraceMultiForObjects from Location to Location - (0, 0,
/// CastLength), radius 90, WorldStatic and Platform ([0, 10]), ignoring self; the first hit decides: a blocking one gives
/// Success and its Location (the sphere's centre there), a non-blocking one or none failure. A multi sweep for objects
/// reports every hit blocking, in time order: the first hit's place.
fn do_ground_trace(w: &mut World, f: FighterId, x: Real, y: Real, cast_length: Real) -> Option<(Real, Real)> {
    let mut hits: Vec<TraceHit> = Vec::new();
    let types = [crate::real(OBJ_WORLD_STATIC), crate::real(OBJ_PLATFORM)];
    sphere_trace_multi_for_objects(
        w,
        x,
        y,
        x,
        y - cast_length,
        90.0,
        &types,
        &[],
        Some(ActorRef::Fighter(f)),
        &mut hits,
    );
    let first = hits.first().filter(|h| h.blocking)?;
    Some((first.x, first.y))
}

/// The Tick's ground probes @7927-8580 (authority), each side while it is on: Do Ground Trace from the probe; a hit
/// appends the CastDestination to the side's array and moves the probe by its direction x (DeltaSeconds x
/// GroundCastMoveSpeed) (left (0, 1, 0) @8095-8237: sim -x; right (0, -1, 0) @8410-8552: sim +x); a miss turns the side
/// off (@8265 / @8580). Left first. DeltaSeconds is the pawn's (its CustomTimeDilation: the hit pause's 0.005).
pub fn ground_tick(w: &mut World, f: FighterId) {
    let g = &garnet(w, f).ground;
    if !g.checking_left && !g.checking_right {
        return;
    }
    let cast = pawn_prop(w, f, "CastLength").unwrap_or(500.0);
    let paused = crate::fighter::mode(w, f) == FrameMode::Paused;
    let dt = DELTA_SECONDS * if paused { HITPAUSE_DILATION } else { 1.0 };
    let step = dt * pawn_prop(w, f, "GroundCastMoveSpeed").unwrap_or(2000.0);
    if garnet(w, f).ground.checking_left {
        let (x, y) = (garnet(w, f).ground.left_x, garnet(w, f).ground.left_y);
        match do_ground_trace(w, f, x, y, cast) {
            Some((hx, hy)) => {
                let s = &mut garnet_w(w, f).ground;
                s.left_positions.push(hx);
                s.left_positions.push(hy);
                s.left_x -= step;
            }
            None => garnet_w(w, f).ground.checking_left = false,
        }
    }
    if garnet(w, f).ground.checking_right {
        let (x, y) = (garnet(w, f).ground.right_x, garnet(w, f).ground.right_y);
        match do_ground_trace(w, f, x, y, cast) {
            Some((hx, hy)) => {
                let s = &mut garnet_w(w, f).ground;
                s.right_positions.push(hx);
                s.right_positions.push(hy);
                s.right_x += step;
            }
            None => garnet_w(w, f).ground.checking_right = false,
        }
    }
}

// ---------------------------------------------------------------- GarnetShockwaveHitBox (+ Left / Right)

/// `ShockwaveHitBox` (GarnetShockwaveHitBox_C, its Left and Right children).
#[derive(State, Debug)]
pub struct ShockwaveHitBox {
    /// HitBoxDamage.
    pub hit: HitPacket,
    pub facing: Real,
    pub attack_id: i32,
    /// GetGameTimeInSeconds at BeginPlay (the owner's clock).
    pub start_time: Real,
    /// The actor's place (the spawn place).
    pub x: Real,
    pub y: Real,
    /// Ticks, the spawn frame counted.
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// The HitBox component's world place (at the actor until the first update moves it).
    pub hx: Real,
    pub hy: Real,
    /// Its place before this tick's move (the sweep's start).
    pub prev_hx: Real,
    pub prev_hy: Real,
    /// HitBoxOn? (collision on).
    pub hit_on: bool,
    /// HitEachActorOnlyOnce, by fighter id.
    pub hit_set: IndexSet<i32>,
    /// Locations: sim (x, y) pairs.
    pub locations: Vec<Real>,
    pub total_distance: Real,
    /// MoveHitboxesTimeline: playing, and its playback position.
    pub playing: bool,
    pub position: Real,
    /// The end gate's DoOnce.
    pub ended: bool,
    /// The end gate's Delay fires at this age (-1: none).
    pub release_age: i32,
}

impl Default for ShockwaveHitBox {
    /// The field initial values.
    fn default() -> Self {
        ShockwaveHitBox {
            hit: HitPacket::default(),
            facing: 0.0,
            attack_id: 0,
            start_time: 0.0,
            x: 0.0,
            y: 0.0,
            age: 0,
            blast_at: f64::from(0.1_f32),
            blast_bounds: new_blast_bounds(),
            hx: 0.0,
            hy: 0.0,
            prev_hx: 0.0,
            prev_hy: 0.0,
            hit_on: false,
            hit_set: IndexSet::new(),
            locations: Vec::new(),
            total_distance: 0.0,
            playing: false,
            position: 0.0,
            ended: false,
            release_age: -1,
        }
    }
}

/// The ShockwaveHitBox's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &ShockwaveHitBox) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The ShockwaveHitBox as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, s: &ShockwaveHitBox) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, s.facing, s.attack_id)
}

/// The ShockwaveHitBox's HitBoxDamage packet (`hit`).
pub fn hit(s: &ShockwaveHitBox) -> &HitPacket {
    &s.hit
}

fn sw_of(w: &World, id: ActorId) -> Option<&ShockwaveHitBox> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::ShockwaveHitBox(s) => Some(s),
        _ => None,
    }
}

fn sw_mut(w: &mut World, id: ActorId) -> Option<&mut ShockwaveHitBox> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::ShockwaveHitBox(s) => Some(s),
        _ => None,
    }
}

/// The class values the timeline reads from the actor's data (`a.p`).
struct Cfg {
    move_speed: Real,
    min_life: Real,
    doe: bool,
    /// The owner's clock now (GetGameTimeInSeconds).
    now: Real,
}

fn cfg_of(w: &World, id: ActorId, f: FighterId) -> Cfg {
    let p = actor_p(w, ActorRef::Actor(id));
    Cfg {
        move_speed: num(p, "MoveSpeed").unwrap_or(4000.0),
        min_life: num(p, "MinLifeTime").unwrap_or(0.3),
        doe: flag(p, "DestroyOnExitedArena?"),
        now: w.fighter(f).clock.time,
    }
}

/// @380 -> @30, once. Returns true when ReleaseToPool ran now.
fn end_gate(s: &mut ShockwaveHitBox, c: &Cfg) -> bool {
    if s.ended {
        return false;
    }
    s.ended = true;
    s.hit_on = false; // HitBox.SetCollisionEnabled(NoCollision)
    let elapsed = c.now - s.start_time;
    if elapsed >= c.min_life {
        return true; // ReleaseToPool
    }
    s.release_age = s.age + to_int32(latent_frames(c.min_life - elapsed));
    false
}

/// The update @1501 at the timeline's Alpha (= its position). True: the actor was released by the end gate.
fn update(s: &mut ShockwaveHitBox, alpha: Real, c: &Cfg) -> bool {
    let n = s.locations.len() >> 1;
    let n_pairs = i32::try_from(n).unwrap_or(i32::MAX);
    let p = if s.total_distance == 0.0 { 0.0 } else { (alpha * c.move_speed) / s.total_distance };
    if p < 1.0 {
        // FTrunc (TruncToInt) of p * n, the index of the pair; out of range: the zeroed parameter (the header's
        // Unverified note).
        let i = usize::try_from(trunc_to_int(p * crate::real(n_pairs))).ok().filter(|&i| i < n);
        if let Some(i) = i {
            s.hx = s.locations[2 * i];
            s.hy = s.locations[2 * i + 1];
        } else {
            s.hx = 0.0;
            s.hy = 0.0;
        }
        if !s.hit_on && !s.ended {
            // HitBoxesOn: the overlaps begin at the new place (the move itself was made with no collision).
            s.prev_hx = s.hx;
            s.prev_hy = s.hy;
            s.hit_on = true;
        }
        return false;
    }
    s.playing = false; // StopMovement @586
    end_gate(s, c)
}

fn sw_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let c = cfg_of(w, id, f);
    let Some(s) = sw_mut(w, id) else { return };
    s.age += 1;
    s.prev_hx = s.hx;
    s.prev_hy = s.hy;
    let mut dead = false;
    if s.release_age >= 0 && s.age >= s.release_age {
        dead = true; // the Delay's resume @15: ReleaseToPool
    } else {
        if s.playing {
            // FTimeline::TickTimeline: position + DeltaSeconds; at the length: held there, the update, then Finished.
            let mut pos = s.position + DELTA_SECONDS;
            let end = pos >= 1.0;
            if end {
                pos = 1.0;
            }
            s.position = pos;
            dead = update(s, pos, &c);
            if end && s.playing {
                s.playing = false;
                dead |= end_gate(s, &c); // MoveHitboxesTimeline__FinishedFunc @2317
            }
        }
        // CheckBlastBoxExit on its 0.1 s timer: the spawn frame is not counted.
        if !dead {
            dead = blast_timer_exit(c.doe, &mut s.blast_at, &s.blast_bounds, crate::real(s.age - 1), s.x, s.y);
        }
    }
    if dead && let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// The HitBox (capsule r 60, half height 250, absolute rotation (0, 90, 90): along the depth axis, so a circle of radius
/// 60 in the play plane at HitBoxScale) along this tick's move: the sweep of K2_SetWorldLocation(sweep) meets what lies
/// between the two places, at the root's scale (HitBox's parent; its rotation is absolute, its scale not).
// Deviation: the swept capsule is taken as circles every half radius along the move, and the depth axis as unbounded
// (Garnet's 'Use 2D Hitboxes': the HitBox2D tag), as `chars/harley/actors_util`.
fn shockwave_shapes(s: &ShockwaveHitBox, hit_box_scale: Real, size_scale: Option<Real>) -> Vec<WorldSphere> {
    let r = 60.0 * hit_box_scale * size_scale.unwrap_or(1.0);
    let (dx, dy) = (s.hx - s.prev_hx, s.hy - s.prev_hy);
    let n = math::max(1.0, (size2d([dx, dy]) / (r / 2.0)).ceil());
    let n_i = to_int32(n);
    let mut out = Vec::new();
    for i in 0..=n_i {
        let t = crate::real(i) / n;
        out.push(WorldSphere { x: s.prev_hx + dx * t, y: s.prev_hy + dy * t, r, ..WorldSphere::default() });
    }
    out
}

/// The shapes of the actor now (`HitBoxScale.X` of its data).
fn shapes_now(w: &World, id: ActorId) -> Option<Vec<WorldSphere>> {
    let (a, s) = (w.actors.actors.get(id)?, sw_of(w, id)?);
    if !s.hit_on || a.dead {
        return None;
    }
    let scale = obj(actor_p(w, ActorRef::Actor(id)), "HitBoxScale").and_then(|v| num(v, "X")).unwrap_or(1.0);
    Some(shockwave_shapes(s, scale, a.size_scale))
}

fn sw_hit_shapes(w: &World, id: ActorId, out: &mut Vec<WorldSphere>) -> bool {
    match shapes_now(w, id) {
        Some(v) => {
            out.extend(v);
            true
        }
        None => false,
    }
}

/// The HitBox against the enemies' hurt shapes: the handler's pfgDoDamage gates in the projectile test's order. The hit's
/// place: the HitBox's.
// Deviation: the HitBox against the other spawned actors' hurt shapes is not tested (as AirFlap).
// The direction: all three trails' packets carry DirectKnockbackTowardsSocket, Root, (0, 1000) (the base trail's CDO;
// Left and Right inherit it), so pfgDoDamage's socket step (@12526-13685, World.directSocketDir) overwrites the
// IgnoreFacing vector of the Left / Right trails (@11068-11168) and the facing path: the victim goes towards a point
// 1000 above Garnet's Root, whichever trail and facing. No direction is passed.
fn sw_hits(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let Some(s) = sw_of(w, id) else { return };
    if !s.hit_on {
        return;
    }
    let (attack_id, hit, hx, hy) = (s.attack_id, s.hit.clone(), s.hx, s.hy);
    let team = w.fighter(f).team.team;
    let mut shapes: Option<Vec<WorldSphere>> = None;
    for vic in match_fighters(w) {
        let vid = w.fighter(vic).id;
        let Some(s) = sw_of(w, id) else { return };
        if w.fighter(vic).team.team == team
            || !alive(w, vic)
            || crate::combat::phases::no_hit_by_team(w, team, vic)
            || s.hit_set.contains(&vid)
        {
            continue;
        }
        let Some(vp) = victim_pose(w, vic) else { continue };
        if shapes.is_none() {
            shapes = shapes_now(w, id);
        }
        let met = shapes.as_ref().is_some_and(|sh| hurt_hit(w, vic, sh, &vp).is_some());
        give_vbuf(w, vp);
        if !met {
            continue;
        }
        let (vx, vy) = (w.fighter(vic).body.x, w.fighter(vic).body.y);
        if hit.do_los_check.unwrap_or(false) && los_blocked(w, hx, hy, vx, vy) {
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
        if hit.hit_each_actor_only_once.unwrap_or(false)
            && let Some(s) = sw_mut(w, id)
        {
            s.hit_set.insert(vid);
        }
        let rows = class_rows(w, me);
        let d = dealer(w, me);
        if apply_projectile_hit(w, &d, vic, &hit, &rows, V2 { x: hx, y: hy }, None, &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &d, vic);
        }
    }
}

/// BeginPlay @687 inside the spawn (the owner's notify), then the timeline's PlayFromStart update at 0. The Instigator's
/// BPI_Garnet cast: another fighter gives no timeline, as the build's.
fn begin(w: &World, f: FighterId, s: &mut ShockwaveHitBox, right: bool, c: &Cfg) {
    let Some(g) = garnet_of(w, f) else { return };
    let from = if right { &g.ground.right_positions } else { &g.ground.left_positions };
    s.locations = from.clone();
    let n = s.locations.len();
    s.total_distance = if n > 0 { dist2d([s.x, s.y], [s.locations[n - 2], s.locations[n - 1]]) } else { 0.0 };
    s.playing = true;
    s.position = 0.0;
    let _ = update(s, 0.0, c);
}

/// The kind's `spawn`: the place, PostActorConstruction's FindTeleportSpot, the parent's BeginPlay blast-box loop, then
/// the class's own BeginPlay.
fn sw_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = request_props(w, f, r);
    let sp = spawn_place(w, f, &np);
    let at = spawn_spot(
        w,
        &SpawnSpotR { cls: r.cls, p: np.clone() },
        &SpawnSpotA { components: a.components.clone() },
        sp.x,
        sp.y,
    );
    let mut s = ShockwaveHitBox {
        hit: HitPacket::from_props(&w.names, obj(&a.p, "HitBoxDamage").unwrap_or(&mvs_data::value::Props::new()), None),
        facing: crate::fighter::facing(w, f),
        attack_id: r.attack_id,
        start_time: w.fighter(f).clock.time,
        x: at.x,
        y: at.y,
        hx: at.x,
        hy: at.y,
        prev_hx: at.x,
        prev_hy: at.y,
        ..ShockwaveHitBox::default()
    };
    let sb = spawn_box(w);
    let (mut bx, mut by) = (s.x, s.y);
    let dead = blast_box_loop(
        &sb,
        &mut bx,
        &mut by,
        None,
        flag(&a.p, "ForceSpawnPositionInsideBlastBox"),
        &mut s.blast_bounds,
        flag(&a.p, "DestroyOnExitedArena?"),
    );
    // (x, y are readonly here: the loop writes the actor's place only through a moved spawn, which this class
    // does not ask for; the loop's own copy is dropped.)
    let _ = (bx, by);
    if !dead {
        let c = Cfg {
            move_speed: num(&a.p, "MoveSpeed").unwrap_or(4000.0),
            min_life: num(&a.p, "MinLifeTime").unwrap_or(0.3),
            doe: flag(&a.p, "DestroyOnExitedArena?"),
            now: w.fighter(f).clock.time,
        };
        begin(w, f, &mut s, flag(&a.p, "IsMovingRight"), &c);
    }
    (WorldActorState::ShockwaveHitBox(s), dead)
}

/// This file's registry entries (AnimNotify_GarnetStartGroundChecking_C, the GarnetShockwaveHitBox classes: Left's parent
/// is the base, Right's the Left and the base). `foreign_owner`: the reads of the owner are the Instigator's BPI_Garnet
/// cast (another fighter: no timeline, as the build's), its facing, team and time.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies.enter(
        "AnimNotify_GarnetStartGroundChecking_C",
        NotifyHandler { begin: Some(start_ground_checking), ..NotifyHandler::default() },
    )?;
    let hooks = || {
        ActorHooks::World(WorldActorHooks {
            spawn: Some(sw_spawn),
            // shockwave_shapes reads the scale, the view too
            size: Some(SizeHooks::default()),
            hit_shapes: Some(sw_hit_shapes),
            step: Some(sw_step),
            hits: Some(sw_hits),
            ..WorldActorHooks::default()
        })
    };
    let entry = |class: WorldActorClass, parents: &'static [&'static str]| ActorEntry {
        foreign_owner: true,
        parents,
        ..ActorEntry::new(ActorClass::World(class), hooks())
    };
    r.actors.enter(SHOCKWAVE, entry(WorldActorClass::GarnetShockwaveHitBox, &[]))?;
    r.actors.enter(SHOCKWAVE_LEFT, entry(WorldActorClass::GarnetShockwaveHitBoxLeft, &[SHOCKWAVE]))?;
    r.actors.enter(SHOCKWAVE_RIGHT, entry(WorldActorClass::GarnetShockwaveHitBoxRight, &[SHOCKWAVE_LEFT, SHOCKWAVE]))
}
