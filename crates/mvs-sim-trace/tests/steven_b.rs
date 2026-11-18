//! Steven's barrier paths no trace match runs (`chars/steven/barrier.rs`): the bounce count and the perk's multiplier
//! of a fighter in hitstun against the barrier's boxes, a fighter's damage to its health, and the barrier's life.
//! steven-shaggy holds the barrier from frame 258 on; the matches never bounce a fighter off it nor hurt it.

use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, ActorRef};
use mvs_sim::chars::steven::barrier::StevenShield;
use mvs_sim::registry::entries::ActorHooks;
use mvs_sim::world::spawned_platform::spawned_wall_bounce_multiplier;
use mvs_sim_trace::harness::{self, Harness};

/// A frame of steven-shaggy with the barrier standing.
const AT: usize = 258;

fn shield(h: &Harness) -> (ActorId, &StevenShield) {
    let (id, a) = h.w.actors.actors.iter().next().expect("the barrier of the frame");
    match &a.state {
        WorldActorState::StevenShield(s) => (id, s),
        _ => panic!("not a Steven_ShieldActor"),
    }
}

fn shield_mut(h: &mut Harness) -> &mut StevenShield {
    let (id, _) = shield(h);
    match &mut h.w.actors.actors.get_mut(id).expect("the barrier").state {
        WorldActorState::StevenShield(s) => s,
        _ => unreachable!(),
    }
}

#[test]
fn a_fighter_in_hitstun_counts_the_bounces_and_ends_the_barrier_at_the_max() {
    let Some(mut h) = harness::at("steven-shaggy", AT) else { return };
    let (id, _) = shield(&h);
    let f = h.fighter(1);
    h.w.fighter_mut(f).hit.hitstun = 30.0;
    for n in 1..=3 {
        let m = spawned_wall_bounce_multiplier(&mut h.w, ActorRef::Actor(id), 0, f).expect("a part's answer");
        assert_eq!(m, 1.0, "no perk: the plain 1.0");
        assert_eq!(shield(&h).1.bounce_counter, n);
        assert!(!shield(&h).1.pending_death);
    }
    spawned_wall_bounce_multiplier(&mut h.w, ActorRef::Actor(id), 0, f);
    assert_eq!(shield(&h).1.bounce_counter, 4);
    assert!(shield(&h).1.pending_death, "MaxBounces met: destroyed at the next tick");
    // a bounce counted already: no more
    spawned_wall_bounce_multiplier(&mut h.w, ActorRef::Actor(id), 0, f);
    assert_eq!(shield(&h).1.bounce_counter, 4);
    h.step_recorded();
    assert!(h.w.actors.actors.get(id).is_none_or(|a| a.dead), "the barrier is gone the tick after its fourth bounce");
}

#[test]
fn a_fighter_out_of_hitstun_counts_nothing_and_the_perk_boosts_an_enemy_only() {
    let Some(mut h) = harness::at("steven-shaggy", AT) else { return };
    let (id, _) = shield(&h);
    let (enemy, owner) = (h.fighter(1), h.fighter(0));
    assert_ne!(h.w.fighter(enemy).team.team, h.w.fighter(owner).team.team);
    let m = spawned_wall_bounce_multiplier(&mut h.w, ActorRef::Actor(id), 0, enemy).expect("a part's answer");
    assert_eq!(m, 1.0);
    assert_eq!(shield(&h).1.bounce_counter, 0, "no hitstun, no count");
    shield_mut(&mut h).bounce_perk = true;
    let m = spawned_wall_bounce_multiplier(&mut h.w, ActorRef::Actor(id), 0, enemy).expect("a part's answer");
    assert_eq!(m, 1.3_f32, "PerkBounceMultiplier as a float32");
    let m = spawned_wall_bounce_multiplier(&mut h.w, ActorRef::Actor(id), 0, owner).expect("a part's answer");
    assert_eq!(m, 1.0, "the owner's team: no boost");
}

#[test]
fn a_fighters_damage_lowers_the_health_and_a_dead_barrier_leaves() {
    let Some(mut h) = harness::at("steven-shaggy", AT) else { return };
    let (id, s) = shield(&h);
    let (health, cls) = (s.health, h.w.actors.actors.get(id).expect("the barrier").cls);
    let ActorHooks::World(k) = h.w.registry.actors.get(h.w.names.str(cls)).expect("the class's entry").hooks else {
        panic!("a world actor")
    };
    let hurt = k.any_damage.expect("ReceiveAnyDamage");
    let f = h.fighter(1);
    hurt(&mut h.w, id, 1.0, ActorRef::Fighter(f));
    assert_eq!(shield(&h).1.health, health - 1.0);
    assert_eq!(shield(&h).1.hit_heavy, Some(true), "a fighter's damage is the heavy visual");
    assert!(!h.w.actors.actors.get(id).expect("the barrier").dead);
    // a damage causer that is no fighter does nothing
    hurt(&mut h.w, id, 5.0, ActorRef::Actor(id));
    assert_eq!(shield(&h).1.health, health - 1.0);
    // the rest of the health: destroyed (no clamp)
    hurt(&mut h.w, id, health, ActorRef::Fighter(f));
    assert!(shield(&h).1.health <= 0.0);
    assert!(h.w.actors.actors.get(id).expect("the barrier").dead);
}
