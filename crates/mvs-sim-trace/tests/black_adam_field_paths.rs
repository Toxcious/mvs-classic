//! A path no trace match runs: an enemy actor of the World's actors (Tom's mouse trap) inside Black Adam's force
//! field (C021_ForceField_C). Destroy On Enter's pfgDoDamage(null, the entrant, Damage 1, DestroyProjectiles, ...)
//! @7202 passes empty ElementalBuffsParam and OnHitBuffsParam (`struct{[]}`, as the begin trace's call @2937): the
//! route's row reads take the call's none and never ask for the field's own HitBoxDamage. The port panicked there
//! before the rows were passed down. On a fresh World of Black Adam against Tom; without a store it checks nothing.

use std::sync::Arc;

use mvs_data::value::Props;
use mvs_sim::Real;
use mvs_sim::actors::ActorId;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::events::{Event, NotifyRef};
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::match_side::{Foreign, SpawnRequest};
use mvs_sim::fighter::moves::V2;
use mvs_sim::input::NO_INPUT;
use mvs_sim::names::Name;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::spawning::spawn_world_actor;
use mvs_sim_trace::harness::{self, Harness};

fn player(c: &str, team: u8) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), team: Some(team), ..MatchPlayer::default() }
}

fn step(h: &mut Harness) {
    h.step(&[NO_INPUT, NO_INPUT]);
}

fn spawn(h: &mut Harness, f: FighterId, cls: &str, x: Real, y: Real) -> ActorId {
    let r = SpawnRequest {
        cls: h.w.lit(cls),
        n: NotifyRef { montage: Name::NONE, index: 0 },
        attack_id: h.w.fighter(f).decay.attack_count,
        foreign: Foreign::None,
        copy: None,
        charge: None,
        aim: None,
        props: Some(Arc::new(Props::new())),
        instigator: None,
        origin: Some(V2 { x, y }),
    };
    spawn_world_actor(&mut h.w, f, &r).expect("the actor spawns")
}

#[test]
fn an_enemy_trap_inside_the_force_field_is_destroyed() {
    let setup = MatchSetup { players: vec![player("blackadam", 0), player("tomandjerry", 1)], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    for _ in 0..10 {
        step(&mut h);
    }
    let (adam, tom) = (h.fighter(0), h.fighter(1));
    // high above Black Adam, out of both fighters' reach: the field's begin trace and the trap's trigger meet no one
    let (x, y) = (h.w.fighter(adam).body.x, h.w.fighter(adam).body.y + 1200.0);
    let t = spawn(&mut h, tom, "MouseTrap_SpawnedActor_C", x, y);
    if let WorldActorState::MouseTrap(s) = &mut h.w.actors.actors.get_mut(t).expect("the trap lives").state {
        (s.x, s.y) = (x, y);
    }
    step(&mut h);
    let ff = spawn(&mut h, adam, "C021_ForceField_C", x, y);
    if let WorldActorState::ForceField(s) = &mut h.w.actors.actors.get_mut(ff).expect("the field lives").state {
        (s.x, s.y) = (x, y);
    }
    let hit = h.w.lit("sfx_cha_C021_barrier_projectile_hit");
    let mut met = false;
    for _ in 0..30 {
        h.w.events.clear();
        step(&mut h);
        met |= h.w.events.iter().any(|e| matches!(e, Event::Sound { ev, .. } if *ev == hit));
        if h.w.actors.actors.get(t).is_none_or(|a| a.dead) {
            break;
        }
    }
    assert!(met, "the field's Destroy On Enter met the trap");
    assert!(h.w.actors.actors.get(t).is_none_or(|a| a.dead), "the trap is destroyed by the field");
}
