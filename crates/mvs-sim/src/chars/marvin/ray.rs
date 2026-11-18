//! Marvin's disintegration ray: C018DisintegrationRay_C and
//! C018DisintegrationRay_Down_C ([`Ray`], a World.actors DealerActor that rides the owner's L_Blaster socket) and
//! AnimNotify_C018DamageFromDisintegration_C. The ray and the notify write Marvin's `disint` through pawn.rs's setters.
//!
//! The graph (ubergraph offsets): BeginPlay @2769 (the Instigator's SetDisintegrationHit(false) @2878, then
//! MulticastDistance(Lerp(MinDistance, MaxDistance, GetChargeValue(4))) @3229 and Delay(0.0) @3252 -> HitBoxOn? true);
//! MulticastDistance @3307 (EndingDistance, the timeline's play rate 1 / TimeToFullLength, the HitBox detached and
//! turned, PlayFromStart); the GrowBeamTimeline's update @4613 -> @42 (the HitBox's scale and place, VFXOffset, the
//! beam points); HitEnemyPlayer @4427 (SetDisintegrationHit(true) @4550, the sound @4298); EndPlay @4618
//! (SetDisintegrationHit(false) @4732). The ProjectileMovement (InitialSpeed / MaxSpeed 1) is not modelled: the actor
//! rides the socket.

use indexmap::IndexSet;
use mvs_data::value::Props;

use super::pawn::{set_damage_from_disintegration, set_disintegration_hit};
use super::state::marvin_of;
use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::WorldSphere;
use crate::combat::elem::charge_pct;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::phases::{alive, give_vbuf, los_blocked, no_hit, victim_pose};
use crate::combat::spawned_hits::class_rows;
use crate::consts as C;
use crate::events::{Event, NotifyRef};
use crate::fighter::anim::sample_boxes;
use crate::fighter::common::{latent_frames, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::{FighterId, actor_y, facing, invulnerable, mesh_wx, mesh_wy, percent_charged};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, obj, text};
use crate::registry::{NotifyHandler, Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::pawns::match_fighters;

/// C018DisintegrationRay's class.
pub const RAY: &str = "C018DisintegrationRay_C";

/// C018DisintegrationRay_Down's class.
pub const RAY_DOWN: &str = "C018DisintegrationRay_Down_C";

/// PF_Post_Event_At_Location(sfx_cha_C018_ground_disintegration_hit) at the VFX.
const HIT_SFX: Name = name!("sfx_cha_C018_ground_disintegration_hit");

/// The capsule's core box width (the shape is a thin box rounded by the radius).
const THIN: Real = 0.01;

/// `Ray` (C018DisintegrationRay_C and its _Down_C child).
#[derive(State, Debug)]
pub struct Ray {
    /// HitBoxDamage.
    pub hit: HitPacket,
    /// The owner's facing at the last follow (the actor rides the owner's mesh).
    pub facing: Real,
    pub attack_id: i32,
    /// The _Down_C child (`p.Down`).
    pub down: bool,
    /// The mesh socket the actor rides.
    pub socket: Name,
    /// The actor: the socket.
    pub x: Real,
    pub y: Real,
    /// Ticks, the spawn frame counted.
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// RayFields: the actor's place and the beam's far end (VFXOffset).
    pub start_x: Real,
    pub start_y: Real,
    pub end_x: Real,
    pub end_y: Real,
    /// VFXOffset, world.
    pub vfx_dx: Real,
    pub vfx_dy: Real,
    /// The HitBox's centre.
    pub hx: Real,
    pub hy: Real,
    /// The HitBox's place on the Instigator's root (`lx` along the root's facing, `ly`).
    pub lx: Real,
    pub ly: Real,
    /// HitboxWidth (the capsule's radius).
    pub radius: Real,
    /// (L + buffer) / 2 (its half height).
    pub half: Real,
    /// HitBoxOn?.
    pub hit_on: bool,
    /// The age Delay(0.0) resumes at.
    pub hit_age: Real,
    /// HitEachActorOnlyOnce, by fighter id.
    pub hit_set: IndexSet<i32>,
    /// EndingDistance.
    pub end_dist: Real,
    /// The timeline's play rate.
    pub rate: Real,
    pub playing: bool,
    pub position: Real,
}

impl Default for Ray {
    fn default() -> Self {
        Ray {
            hit: HitPacket::default(),
            facing: 0.0,
            attack_id: 0,
            down: false,
            socket: Name::NONE,
            x: 0.0,
            y: 0.0,
            age: 0,
            blast_at: f64::from(0.1_f32),
            blast_bounds: new_blast_bounds(),
            start_x: 0.0,
            start_y: 0.0,
            end_x: 0.0,
            end_y: 0.0,
            vfx_dx: 0.0,
            vfx_dy: 0.0,
            hx: 0.0,
            hy: 0.0,
            lx: 0.0,
            ly: 0.0,
            radius: 0.0,
            half: 0.0,
            hit_on: false,
            hit_age: -1.0,
            hit_set: IndexSet::new(),
            end_dist: 0.0,
            rate: 1.0,
            playing: false,
            position: 0.0,
        }
    }
}

/// The ray's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &Ray) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The ray as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, s: &Ray) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, s.facing, s.attack_id)
}

/// The ray's HitBoxDamage packet (`hit`).
pub fn hit(s: &Ray) -> &HitPacket {
    &s.hit
}

/// The class numbers the update reads (`a.p`).
struct Cfg {
    start: Real,
    buffer: Real,
    width: Real,
    min: Real,
    max: Real,
    time_to_full: Real,
    doe: bool,
}

fn cfg(p: &Props) -> Cfg {
    Cfg {
        start: num(p, "StartingDistance").unwrap_or(0.0),
        buffer: num(p, "OverlapBufferDistance").unwrap_or(0.0),
        width: num(p, "HitboxWidth").unwrap_or(0.0),
        min: num(p, "MinDistance").unwrap_or(0.0),
        max: num(p, "MaxDistance").unwrap_or(0.0),
        time_to_full: num(p, "TimeToFullLength").unwrap_or(1.0),
        doe: flag(p, "DestroyOnExitedArena?"),
    }
}

/// The actor's place: the socket on the owner's pose (AttachToActor; the notify's translation is 0): the owner's facing,
/// x and y.
fn follow_at(w: &World, f: FighterId, socket: Name) -> (Real, Real, Real) {
    let face = facing(w, f);
    let ch = w.fighter(f).ch;
    let si = w
        .names
        .get(&format!("socket:{}", w.names.str(socket)))
        .and_then(|n| w.fixed.samplers.get(&ch)?.by_name.get(&n).copied());
    let mut pose: Vec<f32> = Vec::new();
    if let Some(si) = si
        && sample_boxes(w, f, &mut pose)
        && let Some(o) =
            w.fixed.samplers.get(&ch).and_then(|b| b.offsets.get(usize::try_from(si).expect("a box index")).copied())
    {
        let o = usize::try_from(o).expect("a box offset");
        return (face, mesh_wx(w, f) + crate::real(pose[o]) * face, mesh_wy(w, f) + crate::real(pose[o + 1]));
    }
    warn_text(w, &format!("disintegration ray: socket {} missing", w.names.str(socket)));
    (face, w.fighter(f).body.x, actor_y(w, f) - 100.0)
}

fn beam(s: &mut Ray) {
    s.start_x = s.x;
    s.start_y = s.y;
    s.end_x = s.x + s.vfx_dx;
    s.end_y = s.y + s.vfx_dy;
}

/// The update @42 at the timeline's Alpha. `first`: MulticastDistance's update, the HitBox still detached (@3417): its
/// SetRelativeScale3D @274 is its world scale. At every later one it is under the Instigator's root (attached keeping
/// its world transform @1179), so the relative scale is taken at the root's: the owner's size (`size_scale`).
// Unverified: a polymorphed owner's non-uniform actor scale is taken as its size scale.
fn update(c: &Cfg, s: &mut Ray, owner: &Owner, alpha: Real, first: bool) {
    let l = c.start + (alpha * (s.end_dist - c.start));
    let g = if first { 1.0 } else { owner.size_scale };
    s.radius = c.width * g;
    s.half = ((l + c.buffer) / 2.0) * g;
    let sub = (c.buffer / 2.0) - (l / 2.0);
    // facing 1 (not facing left): UE Y + sub, sim x - sub.
    if s.down {
        s.hx = s.x;
        s.hy = s.y + sub;
    } else {
        s.hx = s.x - sub * owner.facing;
        s.hy = s.y;
    }
    s.lx = (s.hx - owner.x) * owner.facing;
    s.ly = s.hy - owner.actor_y;
    if s.down {
        s.vfx_dx = 0.0;
        s.vfx_dy = -l;
    } else {
        s.vfx_dx = l * owner.facing;
        s.vfx_dy = 0.0;
    }
    beam(s);
}

/// What the ray reads of its owner.
struct Owner {
    x: Real,
    actor_y: Real,
    facing: Real,
    size_scale: Real,
}

fn owner_of(w: &World, f: FighterId) -> Owner {
    Owner {
        x: w.fighter(f).body.x,
        actor_y: actor_y(w, f),
        facing: facing(w, f),
        size_scale: w.fighter(f).body.size_scale,
    }
}

/// `rayShape`: the HitBox capsule as a thin box rounded by the radius: the segment of half length (half height -
/// radius) along the play plane's x (Down: upright).
// Deviation: the capsule is the exact solid but for the THIN width of the core box; the sweep of
// K2_SetWorldLocation(sweep) @1065 (what the HitBox passes over between two places) is not tested: the place of the
// frame alone.
pub fn ray_shape(s: &Ray) -> WorldSphere {
    let d = s.half - s.radius;
    let seg = if d > THIN { d } else { THIN };
    let mut b = [0.0; 9];
    if s.down {
        b[1] = seg;
        b[3] = THIN;
    } else {
        b[0] = seg;
        b[4] = THIN;
    }
    b[8] = THIN;
    WorldSphere { x: s.hx, y: s.hy, z: Some(0.0), r: s.radius, bx: Some(b), ..WorldSphere::default() }
}

fn ray_of(w: &World, id: ActorId) -> Option<&Ray> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::Ray(s) => Some(s),
        _ => None,
    }
}

fn ray_mut(w: &mut World, id: ActorId) -> Option<&mut Ray> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::Ray(s) => Some(s),
        _ => None,
    }
}

/// BeginPlay @2769 inside the spawn, MulticastDistance and the timeline's PlayFromStart update at 0 (stock
/// UTimelineComponent).
// Unverified: the timeline's PlayFromStart runs its update at position 0 inside the spawn (stock
// UTimelineComponent behaviour, not read from the build).
fn begin(w: &mut World, f: FighterId, c: &Cfg, s: &mut Ray) {
    if marvin_of(w, f).is_some() {
        set_disintegration_hit(w, f, false);
    }
    s.end_dist = c.min + ((c.max - c.min) * percent_charged(w, f));
    s.rate = 1.0 / c.time_to_full;
    s.playing = true;
    s.position = 0.0;
    let owner = owner_of(w, f);
    update(c, s, &owner, 0.0, true);
    s.hit_age = crate::real(s.age) + latent_frames(0.0);
}

/// `kind.spawn`: the follow, the blast loop, then the begin (else dead).
fn spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let np = crate::world::spawning::request_props(w, f, r);
    let data = w.data.clone();
    let none = Props::new();
    let p = crate::fighter::common::actor_data(w, &data, ActorRef::Fighter(f), r.cls).map_or(&none, |d| &d.p);
    let c = cfg(p);
    let socket = match text(&np, "Socket") {
        Some(t) if !t.is_empty() && t != "None" => w.names.get(t).unwrap_or(Name::NONE),
        _ => name!("L_Blaster"),
    };
    let mut s = Ray {
        hit: HitPacket::from_props(&w.names, obj(p, "HitBoxDamage").unwrap_or(&none), None),
        facing: facing(w, f),
        attack_id: r.attack_id,
        down: flag(p, "Down"),
        socket,
        ..Ray::default()
    };
    (s.facing, s.x, s.y) = follow_at(w, f, socket);
    let sb = spawn_box(w);
    let dead = blast_box_loop(
        &sb,
        &mut s.x,
        &mut s.y,
        None,
        flag(p, "ForceSpawnPositionInsideBlastBox"),
        &mut s.blast_bounds,
        c.doe,
    );
    if dead {
        return (WorldActorState::Ray(s), true);
    }
    begin(w, f, &c, &mut s);
    (WorldActorState::Ray(s), false)
}

/// `step`.
fn step(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    let c = cfg(actor_p(w, ActorRef::Actor(id)));
    let owner = owner_of(w, f);
    let Some(socket) = ray_of(w, id).map(|s| s.socket) else { return };
    let (fc, px, py) = follow_at(w, f, socket);
    let mut dead = false;
    if let Some(s) = ray_mut(w, id) {
        s.age += 1;
        (s.facing, s.x, s.y) = (fc, px, py);
        // The HitBox rides the Instigator's root between the updates.
        s.hx = owner.x + s.lx * owner.facing;
        s.hy = owner.actor_y + s.ly;
        beam(s); // ReceiveTick @2471
        if crate::real(s.age) == s.hit_age {
            s.hit_on = true; // the Delay's resume @16
        }
        if s.playing {
            // FTimeline::TickTimeline: position + DeltaSeconds x PlayRate, held at the length (the update, then
            // Finished).
            let mut pos = s.position + (C::DELTA_SECONDS * s.rate);
            let end = pos >= 1.0;
            if end {
                pos = 1.0;
            }
            s.position = pos;
            update(&c, s, &owner, pos, false);
            if end {
                s.playing = false;
            }
        }
        let (age, x, y) = (crate::real(s.age - 1), s.x, s.y);
        dead = blast_timer_exit(c.doe, &mut s.blast_at, &s.blast_bounds, age, x, y);
    }
    if dead && let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// The HitBox against the enemies' hurt shapes, by the handler's pfgDoDamage gates in World.projectileTest's order.
// Deviation: the HitBox against the other spawned actors' hurt shapes is not tested, and its packet's
// DestroyProjectiles is not ported (as Superman's laser).
fn hits(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(s) = ray_of(w, id) else { return };
    if !s.hit_on || s.radius <= 0.0 {
        return;
    }
    let (attack_id, hit, hx, hy, sx, sy) = (s.attack_id, s.hit.clone(), s.hx, s.hy, s.x, s.y);
    let shape = ray_shape(s);
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    let team = w.fighter(f).team.team;
    for vic in match_fighters(w) {
        let vid = w.fighter(vic).id;
        let Some(s) = ray_of(w, id) else { return };
        if w.fighter(vic).team.team == team || !alive(w, vic) || no_hit(w, vic) || s.hit_set.contains(&vid) {
            continue;
        }
        let Some(vp) = victim_pose(w, vic) else { continue };
        let met = crate::combat::hit::hurt_hit(w, vic, std::slice::from_ref(&shape), &vp).is_some();
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
            && let Some(s) = ray_mut(w, id)
        {
            s.hit_set.insert(vid);
        }
        let rows = class_rows(w, me);
        let d = dealer(w, me);
        if apply_projectile_hit(w, &d, vic, &hit, &rows, V2 { x: hx, y: hy }, None, &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &d, vic);
            // The handler's HitEnemyPlayer @4298, then the actor's own @4427.
            let fid = w.fighter(f).id;
            w.events.push(Event::Sound {
                fighter: fid,
                ev: HIT_SFX,
                x: Some(sx),
                y: Some(sy),
                hit: None,
                pooled: false,
                key: None,
            });
            if marvin_of(w, f).is_some() {
                set_disintegration_hit(w, f, true);
            }
        }
    }
}

/// `kind.end`: the owner's DisintegrationHasHit cleared.
fn end(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    if marvin_of(w, f).is_some() {
        set_disintegration_hit(w, f, false);
    }
}

/// AnimNotify_C018DamageFromDisintegration_C's begin: the mesh owner's BPI_C018 SetDamageFromDisintegration(true,
/// In Air); another owner: nothing.
fn damage_begin(w: &mut World, f: FighterId, n: NotifyRef, i: i32) {
    let _ = i;
    if marvin_of(w, f).is_none() {
        return;
    }
    let data = w.data.clone();
    let in_air = flag(&crate::fighter::anim::notify_data(w, &data, f, n).p, "In Air");
    set_damage_from_disintegration(w, f, true, in_air);
}

/// AnimNotify_C018DamageFromDisintegration_C's end (false, false).
fn damage_end(w: &mut World, f: FighterId, n: NotifyRef, i: i32) {
    let _ = (n, i);
    if marvin_of(w, f).is_some() {
        set_damage_from_disintegration(w, f, false, false);
    }
}

/// This file's registry entries: the two rays (foreignOwner, the HitBox not scaled with the root: `size` with no
/// hook) and the notify.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let kind = WorldActorHooks {
        size: Some(SizeHooks::default()),
        spawn: Some(spawn),
        step: Some(step),
        hits: Some(hits),
        end: Some(end),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        RAY,
        ActorEntry {
            foreign_owner: true,
            ..ActorEntry::new(ActorClass::World(WorldActorClass::C018DisintegrationRay), ActorHooks::World(kind))
        },
    )?;
    r.actors.enter(
        RAY_DOWN,
        ActorEntry {
            parents: &[RAY],
            foreign_owner: true,
            ..ActorEntry::new(ActorClass::World(WorldActorClass::C018DisintegrationRayDown), ActorHooks::World(kind))
        },
    )?;
    r.notifies.enter(
        "AnimNotify_C018DamageFromDisintegration_C",
        NotifyHandler { begin: Some(damage_begin), end: Some(damage_end), ..NotifyHandler::default() },
    )
}
