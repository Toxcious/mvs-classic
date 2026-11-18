//! Comp_Follower and BPI_Follower on a spawned actor.
//!
//! Comp_Follower (four delegate signatures, no graph of its own), BPI_Follower (four getters), their one
//! implementer Jerry_SpawnedActor (BPI_GetFollowerMesh @1705: JerryMesh; BPI_GetIsAttacking: its IsAttacking,
//! written beside the FollowerIsAttacking broadcasts @29518-29582; BPI_GetIsFlying / BPI_GetIsFalling:
//! JerryAnimBlueprint.IsFlying / IsFalling; the FollowerHitReact broadcasts @22120 / @22270), and their readers:
//! Buff_Invisibility (@2357-3038 the bindings, @4704 / @6023 / @8650 the follower branches), SmokeOcclusionParent
//! CanActorReceiveBuffs @0-141 and FogOcclusionBP (@249, @1004, @2134-3821, @4322, @5787). Nothing else of the
//! build names either (xref calls).
//!
//! A class is a follower by its kind's entry (`ActorEntry::follower`): DoesImplementInterface(BPI_Follower) is
//! `follower_of(a).is_some()`. (BPI_IsAttachedToAlly, which Jerry answers false @1700, is another interface.)

use crate::Real;
use crate::State;
use crate::actors::ActorId;
use crate::actors::ActorRef;
use crate::actors::world_actor::actor_entry;
use crate::combat::elem::actor_buff_signal;
use crate::math;
use crate::registry::signals::ActorBuffSignal;
use crate::world::World;

/// The live world actor of a key.
pub fn follower_by_key(w: &World, key: Real) -> Option<ActorRef> {
    let id = -1.0 - key;
    w.actors.actors.iter().find(|(_, x)| crate::real(x.seq) == id && !x.dead).map(|(i, _)| ActorRef::Actor(i))
}

/// The occlusion zones keep fighters and followers in one list
/// (BuffedActors / BuffedFighters, a TSet of actors, in insertion order): a follower as -1 - its actor id (a
/// fighter's id is 0 or more).
pub fn follower_key(w: &World, a: ActorId) -> Real {
    -1.0 - crate::real(w.actors.actors.get(a).map_or(0, |x| x.seq))
}

/// The zones' overlap test against a follower's shapes: a sphere of
/// radius r at (x, y) and z off the play plane, or (line) an upright segment of half length `line` and radius r (the
/// Chimney's capsule): within the sum of radii.
pub fn follower_touches(w: &World, a: ActorId, x: Real, y: Real, z: Real, r: Real, line: Real) -> bool {
    let Some(k) = follower_of(w, ActorRef::Actor(a)) else { return false };
    if w.actors.actors.get(a).is_none_or(|x| x.dead) {
        return false;
    }
    // The World's buffer, taken out for the call (a nested call finds it empty and uses its own).
    let mut shapes = std::mem::take(&mut *w.scratch.follower_shapes.borrow_mut());
    shapes.clear();
    (k.shapes)(w, ActorRef::Actor(a), &mut shapes);
    let hit = shapes.iter().any(|s| {
        let h = math::max(s.half - s.r, 0.0);
        let gap = math::max(math::max(s.y - h - (y + line), 0.0), y - line - (s.y + h));
        let dx = s.x - x;
        (dx * dx + gap * gap + z * z).sqrt() <= r + s.r
    });
    *w.scratch.follower_shapes.borrow_mut() = shapes;
    hit
}

/// The follower hooks of an actor's class (DoesImplementInterface
/// BPI_Follower_C), from the registry's actor entry.
pub fn follower_of(w: &World, a: ActorRef) -> Option<crate::registry::entries::FollowerHooks> {
    actor_entry(w, a)?.follower
}

/// Comp_Follower's four broadcasts, made by the follower's own graph
/// (Jerry @22120, @22270, @29529, @29582): each buff of the actor that bound the delegate gets it.
pub fn follower_broadcast(w: &mut World, a: ActorRef, s: ActorBuffSignal) {
    if !follower_of(w, a).is_some_and(|k| k.comp) {
        return;
    }
    actor_buff_signal(w, a, s, None);
}

/// `FollowerMeshVis`: what a reader's SetVisibility on BPI_GetFollowerMesh gives the mesh: `All` for
/// SetVisibility(true) (Buff_Invisibility @6332), `Seers` for SetVisibility(IsVisibleLocally) (@160, after the Delay
/// 0.2): seen only by a viewer whose DetermineLocalVisibility of the actor's Instigator is true.
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum FollowerMeshVis {
    All,
    Seers,
}

/// `FollowerShape`: an upright capsule of the actor in the play plane (x, y its centre; r its radius; half its half
/// height, the radius included, UE's convention; a sphere has half = r): a component of object type Pawn or HurtBox
/// with its query on, which the occlusion zones' overlap spheres (Pawn and HurtBox: Overlap) meet.
#[derive(Clone, Copy, Debug)]
pub struct FollowerShape {
    pub x: Real,
    pub y: Real,
    pub r: Real,
    pub half: Real,
}
