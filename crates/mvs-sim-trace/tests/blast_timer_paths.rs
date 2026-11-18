//! SpawnedActorParent's CheckBlastBoxTimer on the actors that gained it (`world/blast.rs` blast_timer_exit): Steven's
//! barrier, Superman's aim marker and Jake's aim punch eat listener. No match moves one of them out of its stored
//! bounds after its spawn; each test stores bounds the actor is outside of and an expired timer, and the next tick
//! destroys it. A control keeps the actor's own bounds and sees it live on.

use mvs_sim::actors::ActorId;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::world::blast::BlastBounds;
use mvs_sim_trace::harness::{self, Harness};

/// Bounds no place of a match is inside.
const AWAY: BlastBounds = BlastBounds { left: 1.0e8, right: 2.0e8, bottom: 1.0e8, top: 2.0e8 };

/// The first world actor the predicate picks.
fn find(h: &Harness, pick: fn(&WorldActorState) -> bool) -> Option<ActorId> {
    h.w.actors.actors.iter().find(|(_, a)| !a.dead && pick(&a.state)).map(|(id, _)| id)
}

/// Sets the actor's stored bounds and timer, steps once with the trace's inputs, and says whether it is gone.
fn gone_after_expiry(h: &mut Harness, id: ActorId, away: bool) -> bool {
    let a = h.w.actors.actors.get_mut(id).expect("the actor");
    let (at, b) = match &mut a.state {
        WorldActorState::StevenShield(s) => (&mut s.blast_at, &mut s.blast_bounds),
        WorldActorState::AimMarker(s) => (&mut s.blast_at, &mut s.blast_bounds),
        WorldActorState::JakeAimPunchEatListener(s) => {
            // the oracle's record has no age for it: past the spawn frame
            s.age = s.age.max(5);
            (&mut s.blast_at, &mut s.blast_bounds)
        }
        _ => panic!("not an actor of the timer"),
    };
    *at = 0.0;
    *b = if away { AWAY } else { BlastBounds { left: -1.0e8, right: 1.0e8, bottom: -1.0e8, top: 1.0e8 } };
    h.step_recorded();
    h.w.actors.actors.get(id).is_none_or(|a| a.dead)
}

fn check(name: &str, frame: usize, pick: fn(&WorldActorState) -> bool) {
    for away in [false, true] {
        let Some(mut h) = harness::at(name, frame) else {
            eprintln!("note: no {name} trace or store; NOTHING CHECKED");
            return;
        };
        let id = find(&h, pick).unwrap_or_else(|| panic!("{name} frame {frame} holds the actor"));
        assert_eq!(gone_after_expiry(&mut h, id, away), away, "{name}: destroyed only outside its bounds");
    }
}

#[test]
fn the_barrier_outside_its_bounds_is_destroyed_at_the_timer() {
    check("steven-shaggy", 258, |s| matches!(s, WorldActorState::StevenShield(_)));
}

#[test]
fn the_aim_marker_outside_its_bounds_is_destroyed_at_the_timer() {
    check("superman-combat", 580, |s| matches!(s, WorldActorState::AimMarker(_)));
}

#[test]
fn the_eat_listener_outside_its_bounds_is_destroyed_at_the_timer() {
    check("jake-jake", 38, |s| matches!(s, WorldActorState::JakeAimPunchEatListener(_)));
}
