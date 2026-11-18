//! B10 T4's paths no trace match runs: Buff_IceSlowProjectile on a thrown item (`combat::elem::slow_projectile`), a
//! fighter's hit box on ProjectileBarrier's StopProjectiles (`actors::barrier` `hurt_box`), the music debuff's
//! On Max Stacks and Silenced on a spawned actor (`chars::gizmo::music`), and Buff_C018WarpFighterHit on an actor
//! (`chars::marvin::swap`). Without a store the tests check nothing.

use mvs_sim::actors::world_actor::{WorldActorState, actor_entry};
use mvs_sim::actors::{ActorId, ActorRef};
use mvs_sim::combat::BuffClass;
use mvs_sim::combat::elem::{elem_of, slow_hit_anything, slow_projectile, spawn_actor_buff};
use mvs_sim::events::Event;
use mvs_sim::input::NO_INPUT;
use mvs_sim::registry::entries::ActorHooks;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};

fn player(c: &str, team: u8) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), team: Some(team), ..MatchPlayer::default() }
}

fn fresh(players: Vec<MatchPlayer>) -> Option<Harness> {
    let n = players.len();
    let setup = MatchSetup { players, ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&vec![NO_INPUT; n]);
    }
    Some(h)
}

/// A pellet's slow on a thrown item scales Velocity and MaxSpeed by SpeedReduction, its end puts them back;
/// a held item's movement is not active, so nothing is scaled.
#[test]
fn the_ice_slow_scales_a_flying_items_movement_and_its_end_restores_it() {
    let Some(mut h) = fresh(vec![player("superman", 0), player("shaggy", 1)]) else { return };
    let (s, g) = (h.fighter(0), h.fighter(1));
    let cls = h.w.lit("Item_Sandwich_C");
    let (x, y) = (h.w.fighter(s).body.x, mvs_sim::fighter::actor_y(&h.w, s));
    let it =
        mvs_sim::world::items::spawn_item(&mut h.w, cls, g, x, y + 300.0, 0.0, 0.0, false, None).expect("the sandwich");
    let a = ActorRef::Item(it);
    {
        let i = h.w.actors.items.get_mut(it).unwrap();
        i.flying = true;
        i.vx = 1000.0;
        i.vy = 200.0;
        i.max_speed = 3000.0;
    }
    assert!(slow_projectile(&mut h.w, a, BuffClass::BuffIceSlowProjectile), "the buff actor spawned");
    assert!(elem_of(&h.w, a, BuffClass::BuffIceSlowProjectile).is_some());
    let i = h.w.actors.items.get(it).unwrap();
    assert!((i.vx - 50.0).abs() < 1e-3 && (i.vy - 10.0).abs() < 1e-3, "Velocity x 0.05: {} {}", i.vx, i.vy);
    assert!((i.max_speed - 150.0).abs() < 1e-3, "MaxSpeed x 0.05: {}", i.max_speed);
    slow_hit_anything(&mut h.w, a);
    let i = h.w.actors.items.get(it).unwrap();
    assert!(
        (i.vx - 1000.0).abs() < 0.1 && (i.max_speed - 3000.0).abs() < 0.1,
        "RemoveSpeedBuff: {} {}",
        i.vx,
        i.max_speed
    );
    assert!(elem_of(&h.w, a, BuffClass::BuffIceSlowProjectile).is_none(), "the buff ended");
}

#[test]
fn the_ice_slow_leaves_an_item_in_a_hand_alone() {
    let Some(mut h) = fresh(vec![player("superman", 0), player("shaggy", 1)]) else { return };
    let g = h.fighter(1);
    let cls = h.w.lit("Item_Sandwich_C");
    let it =
        mvs_sim::world::items::spawn_item(&mut h.w, cls, g, 0.0, 300.0, 0.0, 0.0, false, None).expect("the sandwich");
    {
        let i = h.w.actors.items.get_mut(it).unwrap();
        i.flying = false;
        i.vx = 1000.0;
    }
    slow_projectile(&mut h.w, ActorRef::Item(it), BuffClass::BuffIceSlowProjectile);
    assert_eq!(h.w.actors.items.get(it).unwrap().vx, 1000.0, "ApplySpeedBuff needs an active movement");
}

fn barrier(h: &Harness) -> Option<ActorId> {
    (0..)
        .map_while(|i| h.w.actors.actors.nth(i))
        .find(|&id| matches!(h.w.actors.actors.get(id).map(|a| &a.state), Some(WorldActorState::ProjectileBarrier(_))))
}

/// The barrier's `hurt_box` is StopProjectiles' sphere (r 325) at the actor; the melee test meets it through it.
#[test]
fn the_barrier_has_a_hurt_box_on_its_stop_projectiles_sphere() {
    let Some(h) = harness::at_baseline("lebron-shaggy", 1460) else { return };
    let Some(id) = barrier(&h) else { panic!("a barrier at frame 1460") };
    let e = actor_entry(&h.w, ActorRef::Actor(id)).expect("the barrier's entry");
    let ActorHooks::World(k) = &e.hooks else { panic!("a world actor kind") };
    let hb = (k.hurt_box.expect("a hurt_box hook"))(&h.w, id).expect("a placed barrier has its sphere");
    assert!((hb.r - 325.0).abs() < 1e-3, "StopProjectiles' radius: {}", hb.r);
}

/// The music debuff on an actor at its MaxStacks spawns Silenced on it (its PopText) and ends.
#[test]
fn the_music_debuff_at_max_stacks_on_an_actor_silences_and_ends() {
    let Some(mut h) = harness::at_baseline("gizmo-shaggy", 60) else { return };
    let Some(id) = barrier(&h) else { panic!("a barrier at frame 1460") };
    let a = ActorRef::Actor(id);
    let debuff = h.w.lit("C023A_Music_DeBuff_C");
    spawn_actor_buff(&mut h.w, a, debuff, 1, 0.0, true, None);
    let mut n = 0;
    while elem_of(&h.w, a, BuffClass::C023AMusicDeBuff).is_some() && n < 50 {
        spawn_actor_buff(&mut h.w, a, debuff, 1, 0.0, true, None);
        n += 1;
    }
    assert!(elem_of(&h.w, a, BuffClass::C023AMusicDeBuff).is_none(), "the debuff ended at its cap");
    let pops = (0..h.w.fighters.len())
        .map(|f| h.w.fighter(h.fighter(f)).events.iter().filter(|e| matches!(e, Event::PopText { .. })).count())
        .sum::<usize>();
    assert!(pops >= 1, "the Silenced pop text");
}

/// The warp hit on an actor is refused by its own BeginPlay (K2_DestroyActor): no entry stays.
#[test]
fn the_warp_hit_on_an_actor_ends_at_once() {
    let Some(mut h) = harness::at_baseline("marvin-marvin", 140) else { return };
    let id = (0..).map_while(|i| h.w.actors.actors.nth(i)).next().expect("a world actor at frame 140");
    let a = ActorRef::Actor(id);
    let cls = h.w.lit("Buff_C018WarpFighterHit_C");
    let by = h.fighter(1);
    let r = spawn_actor_buff(&mut h.w, a, cls, 0, 0.0, true, Some(by));
    assert!(r.is_none(), "destroyed in its BeginPlay");
    assert!(elem_of(&h.w, a, BuffClass::BuffC018WarpFighterHit).is_none());
}
