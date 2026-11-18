//! The scrub shield: C019ScrubShield_C ([`ScrubShield`], a world actor the Cleaner perk's
//! down special spawns: it follows its owner, cleans what it overlaps and has its sphere body).
//!
//! The hazard cleaner of Morty's down attack, spawned only with the perk Buff_C019_Cleaner (the ConditionalSpawner of
//! C019_Attack_Down_Scrub_Montage, t 0.5 for 0.611 s: condition C019CleanerPerkEquipped, AttachToActor, Socket Root,
//! Translation (100, 0, 0), DestroyAllActorsOnNotifyEnd: the world ends it at the notify's end).
//! BeginPlay spawns PS_C019_Plumbus_SparkleTrail on its Capsule off a dedicated server (drawn by the render).
//! A SpawnedActorParent child: HitBoxOn? false, no hit boxes, DestroyedOnHit false, the root Capsule NoCollision. Its
//! Sphere: radius 100, object type ProjectileDestructible, QueryAndPhysics, Overlap to WorldDynamic and HitBox.
//! - The Sphere's begin overlap @1341 -> @236, authority: the other actor cast to SpawnedActorParent @266, its
//!   Comp_Actor_Destructible.TeamIndex other than the shield's @341, cast to BPI_EnvironmentalHazard @455 (else
//!   nothing), BPI Clean(Success) @534 (the hazard class's own function: `ActorEntry.hazard.clean`); Success -> the
//!   owner's BPI_IsFacingLeft @689, the owner's location + (0, +-400, 0) (in front of him), LineTraceSingleForObjects
//!   from there 500 down, object types [10, 0] (Platform, WorldStatic), bIgnoreSelf @979, and CleanedVisuals(the hit's
//!   Location) @1285 whether the trace hit or not.
//! - CleanedVisuals @1346 (a multicast): the two emitters (drawn by the render, the Vacuum at `hits`),
//!   sfx_cha_C019_ground_down_clean on the actor @1897; Delay(0.2) -> PF_Post_VO_Event with a null VO Actor: nothing
//!   plays (not posted here).
//!
//! What the Sphere meets (DefaultEngine.ini, ProjectileBarrier_C): a component of object type WorldDynamic or HitBox
//! that answers ProjectileDestructible with Overlap. The classes with BPI_EnvironmentalHazard that the port spawns
//! enter their own `hazard` (an overlap test against the sphere and a BPI Clean).
//! Model (as `actors::barrier`): the sphere at the actor, in the play plane, against each hazard's own test; a begin
//! overlap is a hazard that was not inside at the test before. The test runs at the spawn (inside the owner's notify)
//! and in the actor's step (after the fighters' and the projectiles' moves).
// Deviation: one begin per hazard actor and pass, not one per component pair; the test once a tick, after the moves;
// the translation's X (100, the Root socket's own X: the stage depth under the mesh's yaw, as every socket offset of
// the port) is dropped; DestroyOnExitedArena is not tested (it is attached to its owner, whose ring-out ends it).
//
// The shared code reaches its place (`actors::world_actor`: [`place`]).

// The negated comparisons are `!(a < b)`, true for a NaN too.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActor, WorldActorState};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::events::Event;
use crate::fighter::anim::sample_boxes;
use crate::fighter::common::{actor_def, owner_team, warn_text};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::traces::{OBJ_PLATFORM, OBJ_WORLD_STATIC, TraceBody, body_info, box_trace_single_for_objects};
use crate::fighter::{FighterId, actor_y, facing, mesh_wx, mesh_wy};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, WorldActorHooks};
use crate::registry::props::{flag, num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

const CLS: &str = "C019ScrubShield_C";

/// `ScrubShield` (C019ScrubShield_C).
#[derive(State, Debug, Default)]
pub struct ScrubShield {
    /// The Sphere's centre: the actor's location (the Sphere has no offset of its own).
    pub x: Real,
    pub y: Real,
    pub age: i32,
    /// The Clean calls that answered Success (the port's own count: tests).
    pub cleaned: i32,
    /// The Sphere at the root's scale.
    pub radius: Real,
    /// The notify's translation: Y along the facing, Z up.
    pub up: Real,
    pub fwd: Real,
    /// The hazards the Sphere overlapped at the last test, the projectiles' and the world actors' numbers apart.
    pub in_proj: Vec<i32>,
    pub in_actors: Vec<i32>,
    /// CleanedVisuals' HitLocation per Success, in order (`cleaned` entries): the floor trace's Location (@1122), the
    /// origin when it hit nothing. The render's PS_C019_Plumbus_Vacuum (@1502). Not in the record (it has no
    /// reader): a load keeps it.
    pub hits: Vec<(Real, Real)>,
}

/// The scrub shield's place (its `x`, `y`), `None` for none.
pub fn place(_w: &World, _a: &WorldActor, s: &ScrubShield) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

fn shield(w: &World, id: ActorId) -> Option<&ScrubShield> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::ScrubShield(s) => Some(s),
        _ => None,
    }
}

fn shield_mut(w: &mut World, id: ActorId) -> Option<&mut ScrubShield> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::ScrubShield(s) => Some(s),
        _ => None,
    }
}

/// `follow`: K2_AttachToComponent(the owner's Mesh, Root) then the relative transform: the Root socket of his pose
/// (his actor location when the pose or the socket is not sampled), plus the translation turned with his facing.
fn follow_place(w: &World, f: FighterId, fwd: Real, up: Real) -> (Real, Real) {
    let ch = w.fighter(f).ch;
    let face = facing(w, f);
    let si = w.fixed.samplers.get(&ch).and_then(|b| b.by_name.get(&name!("socket:Root")).copied());
    let mut pose = Vec::new();
    let at = si.and_then(|si| {
        let o = w.fixed.samplers.get(&ch)?.offsets.get(usize::try_from(si).ok()?).copied()?;
        sample_boxes(w, f, &mut pose).then_some(usize::try_from(o).ok()?)
    });
    match at {
        Some(o) => (mesh_wx(w, f) + (crate::real(pose[o]) + fwd) * face, mesh_wy(w, f) + crate::real(pose[o + 1]) + up),
        None => {
            crate::fighter::common::warn_once(w, name!("C019ScrubShield_C: the Root socket not sampled"));
            (w.fighter(f).body.x + fwd * face, actor_y(w, f) + up)
        }
    }
}

/// The constructor: the notify's translation and scale, the Sphere's radius, the place.
fn spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let Some(def) = crate::fighter::common::actor_data(w, &data, ActorRef::Fighter(f), r.cls) else {
        return (WorldActorState::ScrubShield(ScrubShield::default()), true);
    };
    let np = crate::world::spawning::request_props(w, f, r);
    let t = obj(&np, "Transforms");
    let tr = t.and_then(|t| obj(t, "Translation"));
    let sc = t.and_then(|t| obj(t, "Scale3D"));
    let g = |p: Option<&mvs_data::value::Props>, k: &str| p.and_then(|p| num(p, k));
    let sx = g(sc, "X").unwrap_or(1.0);
    let fwd = g(tr, "Y").unwrap_or(0.0);
    let up = g(tr, "Z").unwrap_or(0.0);
    let radius = obj(&def.components, "Sphere").and_then(|c| num(c, "SphereRadius")).unwrap_or(0.0) * sx;
    let rooted = crate::registry::props::text(&np, "Socket") == Some("Root");
    if !flag(&np, "AttachToActor") || !rooted || flag(&np, "WorldTransforms") {
        warn_text(w, "C019ScrubShield_C: a spawn other than attached at the Root socket is not ported");
    }
    if sx != g(sc, "Y").unwrap_or(1.0) || sx != g(sc, "Z").unwrap_or(1.0) {
        warn_text(w, "C019ScrubShield_C: an uneven scale is not ported");
    }
    let (x, y) = follow_place(w, f, fwd, up);
    (WorldActorState::ScrubShield(ScrubShield { x, y, radius, up, fwd, ..ScrubShield::default() }), false)
}

/// The BPI_EnvironmentalHazard begin overlap @236 for one hazard of another team: BPI Clean, then on Success the
/// trace and CleanedVisuals.
fn begin_overlap(w: &mut World, sid: ActorId, other: ActorRef) {
    let Some(hz) = crate::actors::world_actor::actor_entry(w, other).and_then(|e| e.hazard) else { return };
    let Some(owner) = w.actors.actors.get(sid).map(|a| a.owner) else { return };
    if owner_team(w, other) == w.fighter(owner).team.team {
        return;
    }
    if !(hz.clean)(w, other) {
        return;
    }
    let Some(s) = shield_mut(w, sid) else { return };
    s.cleaned += 1;
    let (px, py) = (s.x, s.y);
    let (sx, sy) = (w.fighter(owner).body.x + 400.0 * facing(w, owner), actor_y(w, owner));
    // CleanedVisuals(the hit's Location) @1285: the render draws the Vacuum_Trail from `cleaned` and
    // PS_C019_Plumbus_Vacuum at `hits`.
    let hit = box_trace_single_for_objects(
        w,
        sx,
        sy,
        sx,
        sy - 500.0,
        0.0,
        0.0,
        0.0,
        &[crate::real(OBJ_PLATFORM), crate::real(OBJ_WORLD_STATIC)],
        &[],
        Some(ActorRef::Actor(sid)),
    );
    // Unverified: with no blocking hit, BreakHitResult's Location is the FHitResult default (the origin); the exe's
    // no-hit OutHit was not read.
    let at = hit.filter(|h| h.blocking).map_or((0.0, 0.0), |h| (h.x, h.y));
    if let Some(s) = shield_mut(w, sid) {
        s.hits.push(at);
    }
    let fid = w.fighter(owner).id;
    w.events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_C019_ground_down_clean"),
        x: Some(px),
        y: Some(py),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// The hazard hooks' test of one actor against the Sphere now.
fn hazard_on(w: &World, o: ActorRef, x: Real, y: Real, r: Real) -> bool {
    crate::actors::world_actor::actor_entry(w, o).and_then(|e| e.hazard).is_some_and(|hz| (hz.overlaps)(w, o, x, y, r))
}

/// The Sphere's overlaps at the actor's place now: the pairs that ended leave the lists, the new ones begin. The
/// lists are read as they stand (a Clean can end an actor: it stays in the world's list until its sweep).
fn overlaps(w: &mut World, sid: ActorId) {
    let Some(s) = shield(w, sid) else { return };
    if !(s.radius > 0.0) {
        return;
    }
    let (x, y, r) = (s.x, s.y, s.radius);
    let mut k = 0;
    while let Some(pr) = w.actors.projectiles.nth(k) {
        k += 1;
        let Some(p) = w.actors.projectiles.get(pr) else { continue };
        let (seq, dead) = (p.seq, p.dead);
        let o = ActorRef::Projectile(pr);
        if crate::actors::world_actor::actor_entry(w, o).is_none_or(|e| e.hazard.is_none()) {
            continue;
        }
        let on = !dead && hazard_on(w, o, x, y, r);
        let Some(s) = shield_mut(w, sid) else { return };
        let i = s.in_proj.iter().position(|&v| v == seq);
        if !on {
            if let Some(i) = i {
                s.in_proj.remove(i);
            }
            continue;
        }
        if i.is_some() {
            continue;
        }
        s.in_proj.push(seq);
        begin_overlap(w, sid, o);
    }
    let mut k = 0;
    while let Some(a) = w.actors.actors.nth(k) {
        k += 1;
        if a == sid {
            continue;
        }
        let Some(p) = w.actors.actors.get(a) else { continue };
        let (seq, dead) = (p.seq, p.dead);
        let o = ActorRef::Actor(a);
        if crate::actors::world_actor::actor_entry(w, o).is_none_or(|e| e.hazard.is_none()) {
            continue;
        }
        let on = !dead && hazard_on(w, o, x, y, r);
        let Some(s) = shield_mut(w, sid) else { return };
        let i = s.in_actors.iter().position(|&v| v == seq);
        if !on {
            if let Some(i) = i {
                s.in_actors.remove(i);
            }
            continue;
        }
        if i.is_some() {
            continue;
        }
        s.in_actors.push(seq);
        begin_overlap(w, sid, o);
    }
}

/// The overlaps at the spawn (the end of the constructor), once the World has listed the actor.
fn spawned(w: &mut World, id: ActorId) {
    overlaps(w, id);
}

/// The actor's step: its place under the owner after the fighters' moves, then the overlaps.
fn step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let owner = a.owner;
    let Some(s) = shield(w, id) else { return };
    let (x, y) = follow_place(w, owner, s.fwd, s.up);
    let Some(s) = shield_mut(w, id) else { return };
    s.age += 1;
    s.x = x;
    s.y = y;
    overlaps(w, id);
}

/// The Sphere as a body of the traces: object type ProjectileDestructible at the actor, met by the object queries
/// for that type. The root Capsule has no collision (NoCollision): not entered.
fn bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let Some(s) = shield(w, id) else { return };
    if !(s.radius > 0.0) {
        return;
    }
    let Some(a) = w.actors.actors.get(id) else { return };
    let comp = actor_def(w, ActorRef::Fighter(a.owner), a.cls).and_then(|d| obj(&d.components, "Sphere"));
    let Some(info) = body_info(comp, None).filter(|i| i.on) else { return };
    out.push(TraceBody {
        ax: s.x,
        ay: s.y,
        bx: s.x,
        by: s.y,
        hx: 0.0,
        hy: 0.0,
        r: s.radius,
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: name!("Sphere"),
        actor: Some(ActorRef::Actor(id)),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

/// This file's registry entries (C019ScrubShield_C's kind).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = WorldActorHooks {
        spawn: Some(spawn),
        spawned: Some(spawned),
        bodies: Some(bodies),
        step: Some(step),
        ..WorldActorHooks::default()
    };
    r.actors.enter(CLS, ActorEntry::new(ActorClass::World(WorldActorClass::C019ScrubShield), ActorHooks::World(hooks)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_no_sphere() {
        let s = ScrubShield::default();
        assert!(!(s.radius > 0.0));
        assert!(s.in_proj.is_empty() && s.in_actors.is_empty());
    }
}
