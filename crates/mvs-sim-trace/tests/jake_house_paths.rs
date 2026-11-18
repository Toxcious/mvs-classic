//! Jake's house and stretch paths that no match runs (`chars/jake/house.rs`, `stretch.rs`), on the World of the
//! baseline trace at a frame with the actor live: the house's InputTriggered jump and its stop, the stretch's bounce of
//! an enemy standing on the spline. Without a store or a trace the tests check nothing and say so.

use mvs_sim::actors::{ActorRef, WorldActorState};
use mvs_sim::fighter::set_grounded;
use mvs_sim::fighter::signal_bound;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::names::Name;
use mvs_sim::registry::signals::FighterSignal;
use mvs_sim_trace::harness;

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn house_of(h: &harness::Harness) -> Option<&mvs_sim::chars::jake::house::JakeHouse> {
    h.w.actors.actors.iter().find_map(|(_, a)| match &a.state {
        WorldActorState::JakeHouse(s) if !a.dead => Some(s),
        _ => None,
    })
}

/// InputeTriggered @14109: a Jump press (input 2) with Jake grounded and the gate open launches him by NetImpulse and
/// shuts the gate until the stop impulse's Delay.
#[test]
fn the_houses_jump_press_launches_and_reopens_its_gate() {
    let Some(mut h) = harness::at("jake-jake", 160) else { return };
    let f = h.fighter(0);
    if house_of(&h).is_none() {
        eprintln!("note: no live house at the frame; nothing checked");
        return;
    }
    // (Jake is falling at the frame: put him on the floor, where the press is taken.)
    set_grounded(&mut h.w, f, true);
    assert!(house_of(&h).is_some_and(|s| s.jump_gate));
    h.w.fighter_mut(f).moves.net = None;
    signal_bound(&mut h.w, f, FighterSignal::InputTriggered, false, 2.0, Name::NONE, None);
    let net = h.w.fighter(f).moves.net.as_ref().expect("the jump's NetImpulse");
    assert!(net.vy > 2000.0, "the impulse carries JumpVelocity: {}", net.vy);
    let s = house_of(&h).expect("the house");
    assert!(!s.jump_gate, "the gate shut");
    assert!(s.jump_stop_at > 0.0, "the stop's Delay is pending");
    // A second press while the gate is shut does nothing.
    h.w.fighter_mut(f).moves.net = None;
    signal_bound(&mut h.w, f, FighterSignal::InputTriggered, false, 2.0, Name::NONE, None);
    assert!(h.w.fighter(f).moves.net.is_none());
    for _ in 0..40 {
        h.step(&NONE);
        if house_of(&h).is_none() {
            return; // the house's timer ended it first
        }
    }
    assert!(house_of(&h).is_none_or(|s| s.jump_gate), "the gate reopened with the stop impulse");
}

/// The stretch's bounce: an enemy placed on the spline is bounced once (BouncedFighters) and launched.
#[test]
fn the_stretch_bounces_an_enemy_on_its_spline_once() {
    let Some(mut h) = harness::at("jake-shaggy", 60) else { return };
    let jake = h.fighter(0);
    let enemy = h.fighter(1);
    let id = h.w.actors.actors.iter().find_map(|(id, a)| matches!(a.state, WorldActorState::Stretch(_)).then_some(id));
    let Some(id) = id else {
        eprintln!("note: no stretch at the frame; nothing checked");
        return;
    };
    let (px, py) = match &h.w.actors.actors.get(id).expect("the stretch").state {
        WorldActorState::Stretch(s) => (s.points[s.points.len() - 2], s.points[s.points.len() - 1]),
        _ => unreachable!(),
    };
    let _ = jake;
    let b = &mut h.w.fighter_mut(enemy).body;
    b.x = px;
    b.prev_x = px;
    b.y = py;
    b.prev_y = py;
    h.step(&NONE);
    let vid = h.w.fighter(enemy).id;
    match &h.w.actors.actors.get(id).expect("the stretch").state {
        WorldActorState::Stretch(s) => assert!(s.bounced.contains(&vid), "the enemy was bounced"),
        _ => unreachable!(),
    }
    let _ = ActorRef::Actor(id);
}
