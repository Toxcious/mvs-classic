//! Stripe's saw paths no trace match runs (`chars/stripe/saw.rs`, `fighter/traces/edges.rs`): a BPI_ReverseMoveDirection
//! reaching a saw on its edge, and the same off it.

use mvs_sim::actors::{ActorRef, ProjectileExt};
use mvs_sim::chars::stripe::saw::live_saws;
use mvs_sim::combat::reverse::reverse_move_direction;
use mvs_sim_trace::harness;

fn rider_of(h: &harness::Harness, id: mvs_sim::actors::ProjectileId) -> (bool, bool, i32) {
    match &h.w.actors.projectiles.get(id).expect("the saw").ext {
        ProjectileExt::Saw(s) => (s.rider.active, s.rider.clockwise, s.rider.attached),
        _ => panic!("not a saw"),
    }
}

/// stripe-shaggy: the saw thrown at tick 738 is on the floor's polygon a few ticks on. A Reverse turns it round:
/// Attach To Edge with !Clockwise at its place, still attached to the same polygon.
#[test]
fn a_reverse_turns_a_saw_on_its_edge() {
    let Some(mut h) = harness::at("stripe-shaggy", 760) else { return };
    let saws = live_saws(&h.w);
    let Some(&id) = saws.first() else { panic!("a saw alive at tick 760") };
    let (active, cw, poly) = rider_of(&h, id);
    assert!(active, "the saw rides its edge");
    reverse_move_direction(&mut h.w, ActorRef::Projectile(id));
    let (active2, cw2, poly2) = rider_of(&h, id);
    assert!(active2);
    assert_eq!(cw2, !cw, "the direction is reversed");
    assert_eq!(poly2, poly, "on the same polygon");
    for _ in 0..5 {
        h.step_recorded();
    }
    assert_eq!(rider_of(&h, id).1, !cw, "it keeps the new direction");
}

/// Off its edge (detached, as a stationary pause leaves it) the parent's rule: the lateral velocity flips and the
/// PausedClockwise flips with PausedEdgeAttach.
#[test]
fn a_reverse_flips_a_detached_saw() {
    let Some(mut h) = harness::at("stripe-shaggy", 760) else { return };
    let Some(&id) = live_saws(&h.w).first() else { panic!("a saw alive at tick 760") };
    mvs_sim::fighter::traces::edges::detach_from_edge(&mut h.w, id);
    {
        let p = h.w.actors.projectiles.get_mut(id).expect("the saw");
        p.vx = 300.0;
        if let ProjectileExt::Saw(s) = &mut p.ext {
            s.paused_edge_attach = true;
            s.paused_clockwise = true;
        }
    }
    reverse_move_direction(&mut h.w, ActorRef::Projectile(id));
    let p = h.w.actors.projectiles.get(id).expect("the saw");
    let ProjectileExt::Saw(s) = &p.ext else { panic!("not a saw") };
    assert!(!s.rider.active);
    assert!(!s.paused_clockwise, "PausedClockwise flipped");
}
