//! ScoobySnackBox_C, Comp_Pawn_Rage's heal box (`SnackBox`), a SpawnedActorParent
//! child dropped by SpawnScoobySnack, a world actor on `mvs-collision`'s capsule step.
//! The state below is the class's own fields; the `SimActor` ones are the [`crate::actors::WorldActor`]'s.
//!
//! Data: Capsule r 50, half height 70, mass 145, LinearDamping 0.5, AngularDamping 1, X translation and Y / Z rotation
//! locked, Custom profile overlapping Pawn, HitBox and WorldDynamic; Collectable_Timer 0.75. Comp_Pawn_DamageManager
//! CanTakeDamage false, Comp_Actor_Destructible IsProjectile. Die With Instigator (the world's), DestroyOnExitedArena?
//! (the parent's 0.1 s timer).
//!
//! ReceiveBeginPlay -> @3340 (the parent's), then on authority @2419: Capsule.SetPhysicsLinearVelocity((0,
//! RandomFloatInRange(-200, 200), 800)) @2518, SetPhysicsAngularVelocityInDegrees((RandomFloatInRange(-200, 200), 0, 0))
//! @2676, Init Visuals @328 (PF_Post_Event(sfx_cha_shaggy_scooby_snack_spawn, the owner) @100), Delay(Collectable_Timer)
//! @269 -> @174: CanPickUp = true, Delay(7.0) @185 -> @240: TimeOutVisuals (PS_ShadowClone_Impact at the Capsule, the
//! despawn sound @2954), K2_DestroyActor @254. No overlap is looked for again when CanPickUp comes on.
//! Comp_Spawner HandleActorPostSpawn @10992-11329: AddImpulse(the owner's GetVelocity / 2, velocity change).
//! Capsule begin overlap -> @573 (authority): the owner and the other actor are BaseFighters, CanPickUp, the other is
//! not the owner, both have player data @1097; PickupVisuals @347 (the sound at the collector @397, the emitter at the
//! box @503), then the collector's SpawnBuffActor(Buff_HealOverTime_C, the collector, the collector, 1 stack,
//! SelectFloat(2.0, 1.0, the same team) @1795, BreakIfBlocked true) @1842, K2_DestroyActor @2068.
//! ReceiveAnyDamage @3385: a bare pop_flow.

// Deviation: the pickup tests the fighters' root capsules (Pawn) against the Capsule, as segments in the play plane,
//   once a frame behind the body's step; BaseFighter's own Capsule (OverlapAllDynamic) and an OverlapAllDynamic hit
//   shape would begin an overlap too and are not tested.
// Deviation: the order inside the actor's frame (the body's step, the blast box timer, the overlaps, then the Delays)
//   is the port's.
// Unverified: GetOverlappingActors' order is not read: the frame's begin overlaps are taken in the world's fighter
//   order, the first one that takes the box ending it.
// Unverified: a minion fighter (notPlayerCharacter) is taken to have no player data (GetByFighter fails): it does not
//   pick the box up.

use crate::Real;
use crate::State;
use crate::actors::world_actor::WorldActorState;
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::buffs::spawn_buff;
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, NotifyRef};
use crate::fighter::FighterId;
use crate::fighter::common::{actor_data, latent_frames, sound_at_for, warn_once};
use crate::fighter::match_side::{Foreign, SpawnRequest, spawn_origin};
use crate::fighter::moves::V2;
use crate::fighter::traces::{TraceBody, body_info, sphere_static};
use crate::fighter::{actor_y, cap_radius, capsule_half};
use crate::math::to_int32;
use crate::math::ue::{deg_to_rad, quat_rotate_vector, rad_to_deg, rotator_quaternion};
use crate::name;
use crate::names::Name;
use crate::registry::body_settings::capsule_settings;
use crate::registry::entries::{ActorEntry, ActorHooks, WorldActorHooks};
use crate::registry::props::{flag_or, num, obj};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, check_blast_box_exit, spawn_box};
use crate::world::pawns::{match_fighters_list, put_fighters};
use crate::world::spawned_platform::BlockKind;
use crate::world::spawning::{CapsuleShapeC, capsule_shape, root_body_blockers};
use indexmap::IndexSet;
use mvs_collision::capsule_overlap::{FighterCapsule, OverlapCapsule, capsule_over};
use mvs_collision::rigid_sphere::{Body, Solver};
use mvs_data::value::{Props, Value};
use std::sync::Arc;

pub const SNACK_BOX: &str = "ScoobySnackBox_C";
/// PickupVisuals @503 (drawn without a Team: the class's ownEmitter).
const PICKUP_VFX: Name = crate::name!("PS_ShaggyScoobySnacksPickup");
/// TimeOutVisuals @2800.
const TIMEOUT_VFX: Name = crate::name!("PS_ShadowClone_Impact");
/// Delay(7.0) @185.
const TIMEOUT: Real = 7.0;

/// `SnackBox`.
#[derive(State, Debug, Default)]
pub struct SnackBox {
    /// Capsule.CapsuleRadius.
    pub radius: Real,
    /// Capsule.CapsuleHalfHeight.
    pub half: Real,
    pub x: Real,
    pub y: Real,
    pub vx: Real,
    pub vy: Real,
    /// rad/s about X, the sim's sign.
    pub wc: Real,
    /// rad, the sim's sign.
    pub angle: Real,
    /// The Capsule's roll in degrees, UE's sign (`SnackBoxFields.spin`).
    pub spin: Real,
    /// CanPickUp.
    pub can_pick_up: bool,
    /// Ticks left of the running Delay (Collectable_Timer, then 7.0).
    pub wait: i32,
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// The fighters whose root capsule overlaps the Capsule, by id.
    pub over: IndexSet<i32>,
}

/// RandomFloatInRange(lo, hi): one draw of the world's stream, Min + (Max - Min) x FRand, float32.
fn range_f(w: &mut World, lo: Real, hi: Real) -> Real {
    let r = w.random_float();
    lo + ((hi - lo) * r)
}

fn box_of(w: &World, id: ActorId) -> Option<&SnackBox> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::SnackBox(s) => Some(s),
        _ => None,
    }
}

fn box_mut(w: &mut World, id: ActorId) -> Option<&mut SnackBox> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::SnackBox(s) => Some(s),
        _ => None,
    }
}

fn sound(fighter: i32, ev: Name) -> Event {
    Event::Sound { fighter, ev, x: None, y: None, hit: None, pooled: false, key: None }
}

fn death(cls: Name, fighter: i32, vfx: Name, x: Real, y: Real) -> Event {
    Event::ProjectileDeath {
        cls,
        fighter,
        vfx: Some(vfx),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    }
}

fn spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let o = spawn_origin(r).unwrap_or_else(|| V2 { x: w.fighter(f).body.x, y: actor_y(w, f) });
    let data = w.data.clone();
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls);
    let p = a.map(|a| &a.p);
    let cap = a.and_then(|a| obj(&a.components, "Capsule"));
    let mut bx = SnackBox {
        radius: cap.and_then(|c| num(c, "CapsuleRadius")).unwrap_or(0.0),
        half: cap.and_then(|c| num(c, "CapsuleHalfHeight")).unwrap_or(0.0),
        x: o.x,
        y: o.y,
        wait: to_int32(latent_frames(p.and_then(|p| num(p, "Collectable_Timer")).unwrap_or(0.0))),
        blast_at: f64::from(0.1_f32),
        ..SnackBox::default()
    };
    if bx.radius == 0.0 || bx.half == 0.0 {
        warn_once(w, name!("ScoobySnackBox_C: no Capsule size in the data"));
    }
    if bx.radius != 0.0 {
        // PostActorConstruction's FindTeleportSpot: the Capsule upright against what its body blocks.
        let body = cap.and_then(|c| obj(c, "BodyInstance"));
        let boxes = root_body_blockers(w, body, r.cls, name!("spawn push-out"));
        if let Some(boxes) = boxes {
            let shape = capsule_shape(
                Some(CapsuleShapeC {
                    capsule_radius: cap.and_then(|c| num(c, "CapsuleRadius")),
                    capsule_half_height: cap.and_then(|c| num(c, "CapsuleHalfHeight")),
                }),
                1.0,
                0.0,
                1.0,
            );
            let at = crate::world::spawning::find_teleport_spot_shape_in(boxes.view(), bx.x, bx.y, &shape);
            bx.x = at.x;
            bx.y = at.y;
        }
    }
    let destroy_on_exit = p.is_none_or(|p| flag_or(p, "DestroyOnExitedArena?", true));
    let sb = spawn_box(w);
    let force = p.is_some_and(|p| flag_or(p, "ForceSpawnPositionInsideBlastBox", false));
    // blastBoxLoop moves the box
    let gone = blast_box_loop(&sb, &mut bx.x, &mut bx.y, None, force, &mut bx.blast_bounds, destroy_on_exit);
    if gone {
        return (WorldActorState::SnackBox(bx), true); // the parent's BeginPlay loop
    }
    // BeginPlay @2433 / @2591.
    let lin = range_f(w, -200.0, 200.0);
    let ang = range_f(w, -200.0, 200.0);
    bx.vx = -lin;
    bx.vy = 800.0;
    bx.wc = deg_to_rad(-ang); // the angular velocity in degrees to PhysX's radians (one multiply)
    // HandleActorPostSpawn @11329: + the owner's velocity / 2 (a velocity change; LocalInitialVelocity is 0).
    let b = &w.fighter(f).body;
    let (fvx, fvy) = (b.vx, b.vy);
    bx.vx += fvx / 2.0;
    bx.vy += fvy / 2.0;
    // Init Visuals @100, on the owner.
    let id = w.fighter(f).id;
    w.events.push(sound(id, name!("sfx_cha_shaggy_scooby_snack_spawn")));
    (WorldActorState::SnackBox(bx), false)
}

/// The parent's CheckBlastBoxExit on its 0.1 s looping timer (DestroyOnExitedArena?), as Projectile.expiry counts it.
fn left_arena(p: Option<&Props>, a: &mut SnackBox) -> bool {
    let on = p.is_none_or(|p| flag_or(p, "DestroyOnExitedArena?", true));
    let out_long = (f64::from(a.age) - 1.0) * f64::from(DELTA_SECONDS) > a.blast_at;
    if !on || !out_long {
        return false;
    }
    a.blast_at += f64::from(0.1_f32);
    check_blast_box_exit(a.x, a.y, &a.blast_bounds)
}

/// A fighter with player data (PlayerDataManager GetByFighter succeeds): no minion.
fn has_player_data(w: &World, f: FighterId) -> bool {
    !w.fighter(f).life.not_player_character
}

/// The Capsule's begin overlaps of the frame, with CanPickUp: the first fighter that came in and takes the box.
fn pickups(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let owner = a.owner;
    let fs = match_fighters_list(w);
    pickups_of(w, id, owner, &fs);
    put_fighters(w, fs);
}

/// `pickups`' walk over the match's fighters.
fn pickups_of(w: &mut World, id: ActorId, owner: FighterId, fs: &[FighterId]) {
    for &f in fs {
        let Some(bx) = box_of(w, id) else { return };
        let oc = OverlapCapsule {
            x: f64::from(bx.x),
            y: f64::from(bx.y),
            angle: f64::from(bx.angle),
            radius: f64::from(bx.radius),
            half: f64::from(bx.half),
        };
        let v = w.fighter(f);
        let fc = FighterCapsule {
            dead: v.life.dead != 0.0,
            no_collision: crate::fighter::no_collision(w, f),
            cap_radius: f64::from(cap_radius(w, f)),
            capsule_half: f64::from(capsule_half(w, f)),
            actor_y: f64::from(actor_y(w, f)),
            x: f64::from(v.body.x),
        };
        let fid = v.id;
        let over = capsule_over(&oc, &fc);
        let can = bx.can_pick_up;
        let Some(bx) = box_mut(w, id) else { return };
        if !over {
            bx.over.shift_remove(&fid);
            continue;
        }
        if !bx.over.insert(fid) {
            continue;
        }
        if !can || f == owner || !has_player_data(w, f) || !has_player_data(w, owner) {
            continue;
        }
        pick_up(w, id, f);
        return;
    }
}

/// @1421-2068: the pickup by `f`.
fn pick_up(w: &mut World, id: ActorId, f: FighterId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (owner, cls) = (a.owner, a.cls);
    let Some(bx) = box_of(w, id) else { return };
    let (x, y) = (bx.x, bx.y);
    let ally = w.fighter(f).team.team == w.fighter(owner).team.team;
    // PickupVisuals @347: the sound at the collector @397, the emitter at the box @503.
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    sound_at_for(w, None, name!("sfx_cha_shaggy_scooby_snack_buff_start"), fx, fy);
    let oid = w.fighter(owner).id;
    w.events.push(death(cls, oid, PICKUP_VFX, x, y));
    spawn_buff(w, f, name!("Buff_HealOverTime_C"), if ally { 2.0 } else { 1.0 }, Some(1), Some(f), true, Some(f)); // @1842
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true; // K2_DestroyActor @2068
    }
}

fn step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (owner, cls) = (a.owner, a.cls);
    let data = w.data.clone();
    let def = actor_data(w, &data, ActorRef::Fighter(owner), cls);
    let set = def.and_then(|d| capsule_settings(w, obj(&d.components, "Capsule"), w.names.str(cls))).map(|c| c.set);
    let floor = sphere_static(w, w.stage.ground);
    let mut plats = std::mem::take(&mut w.scratch.sphere_statics);
    plats.clear();
    let shapes = w.blocker_shapes(BlockKind::Nav);
    for &g in &shapes.boxes {
        plats.push(sphere_static(w, g));
    }
    plats.extend(shapes.polys.iter().map(crate::world::stage_query::PolyBlock::rigid));
    let p = def.map(|d| &d.p);
    let Some(bx) = box_mut(w, id) else {
        w.scratch.sphere_statics = plats;
        return;
    };
    bx.age += 1;
    if let Some(set) = set {
        let mut b = Body {
            x: f64::from(bx.x),
            y: f64::from(bx.y),
            vx: f64::from(bx.vx),
            vy: f64::from(bx.vy),
            radius: f64::from(bx.radius),
            angle: f64::from(bx.angle),
            wc: f64::from(bx.wc),
            half: f64::from(bx.half),
            ..Body::default()
        };
        Solver::default().capsule_step(&mut b, &floor, Some(&plats), DELTA_SECONDS, &set);
        (bx.x, bx.y, bx.vx, bx.vy, bx.angle, bx.wc) = (
            crate::math::narrow(b.x),
            crate::math::narrow(b.y),
            crate::math::narrow(b.vx),
            crate::math::narrow(b.vy),
            crate::math::narrow(b.angle),
            crate::math::narrow(b.wc),
        );
        bx.spin = rad_to_deg(-bx.angle);
    }
    let gone = left_arena(p, bx);
    w.scratch.sphere_statics = plats;
    if gone {
        set_dead(w, id);
        return;
    }
    pickups(w, id);
    let Some(a) = w.actors.actors.get(id) else { return };
    if a.dead {
        return;
    }
    let Some(bx) = box_mut(w, id) else { return };
    if bx.wait <= 0 {
        return;
    }
    bx.wait -= 1;
    if bx.wait > 0 {
        return;
    }
    if !bx.can_pick_up {
        bx.can_pick_up = true; // @174
        bx.wait = to_int32(latent_frames(TIMEOUT)); // @185
        return;
    }
    // @240: TimeOutVisuals (the emitter at the Capsule @2800 with the owner's Team, the despawn sound @2954),
    // K2_DestroyActor @254.
    let (x, y) = (bx.x, bx.y);
    let oid = w.fighter(owner).id;
    w.events.push(death(cls, oid, TIMEOUT_VFX, x, y));
    w.events.push(sound(oid, name!("sfx_cha_shaggy_scooby_snack_despawn"))); // on the owner
    set_dead(w, id);
}

fn set_dead(w: &mut World, id: ActorId) {
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// ReceiveAnyDamage @3385: nothing.
fn any_damage(_w: &mut World, _id: ActorId, _damage: Real, _by: ActorRef) {}

/// The Capsule as the traces' body (Custom, no ObjectType saved: WorldDynamic; QueryAndPhysics): the rolled capsule in
/// the play plane, its segment along the body's up.
fn bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let Some(bx) = box_of(w, id) else { return };
    let data = w.data.clone();
    let cap = actor_data(w, &data, ActorRef::Fighter(a.owner), a.cls).and_then(|d| obj(&d.components, "Capsule"));
    let Some(info) = body_info(cap, None) else { return };
    let sized = bx.radius > 0.0;
    if a.dead || !info.on || info.kind < 0 || !sized {
        return;
    }
    let l = crate::math::max(bx.half - bx.radius, 0.0);
    // the Capsule's local Z turned by its rotator (FRotator::Quaternion) of roll angle in degrees
    let [_, y, z] = quat_rotate_vector(rotator_quaternion([0.0, 0.0, rad_to_deg(bx.angle)]), [0.0, 0.0, l]);
    let (ux, uy) = (-y, z);
    out.push(TraceBody {
        ax: bx.x + ux,
        ay: bx.y + uy,
        bx: bx.x - ux,
        by: bx.y - uy,
        hx: 0.0,
        hy: 0.0,
        r: bx.radius,
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: name!("Capsule"),
        actor: Some(ActorRef::Actor(id)),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

pub fn entry() -> ActorEntry {
    let hooks = WorldActorHooks {
        spawn: Some(spawn),
        step: Some(step),
        any_damage: Some(any_damage),
        bodies: Some(bodies),
        ..WorldActorHooks::default()
    };
    ActorEntry::new(ActorClass::World(WorldActorClass::ScoobySnackBox), ActorHooks::World(hooks))
}

/// the rage code's scoobySnack spawn request: `ScoobySnackBox_C` with `{ WorldTransforms: true,
/// InitialMovementDirection: (0, 0, 0), InitialSpeed: 0, MaxSpeed: 0 }`, the owner's attack count, no charge values (the
/// box deals no hit), at the owner's `x` and `actorY`, pushed to the owner's spawn requests.
pub fn request_snack_box(w: &mut World, f: FighterId) {
    let mut p = Props::new();
    p.insert("WorldTransforms".to_owned(), Value::Bool(true));
    let dir = [("X", 0.0), ("Y", 0.0), ("Z", 0.0)].map(|(k, v)| (k.to_owned(), Value::Num(v)));
    p.insert("InitialMovementDirection".to_owned(), Value::Obj(dir.into_iter().collect()));
    p.insert("InitialSpeed".to_owned(), Value::Num(0.0));
    p.insert("MaxSpeed".to_owned(), Value::Num(0.0));
    let origin = V2 { x: w.fighter(f).body.x, y: actor_y(w, f) };
    let r = SpawnRequest {
        cls: name!("ScoobySnackBox_C"),
        n: NotifyRef { montage: Name::NONE, index: 0 },
        attack_id: w.fighter(f).decay.attack_count,
        foreign: Foreign::None,
        copy: None,
        charge: None,
        aim: None,
        props: Some(Arc::new(p)),
        instigator: None,
        origin: Some(origin),
    };
    w.fighter_mut(f).requests.spawn_requests.push(r);
}
