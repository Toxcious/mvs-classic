//! Tom and Jerry's hit boxes and explosions on the actors that are no fighter (pfgDoDamage on a projectile, an item):
//! the mouse trap's Detonate hit box, a stick of Dynamite's explosion, Jerry's hit box and his death hit on a heavy
//! projectile. The oracle has none of these (fighters alone); no trace match reaches them. On fresh Worlds; without a
//! store they check nothing.
//! Unverified: no oracle probe exists; the asserts are the build's pfgDoDamage rows (HitBoxHandler @7466 / @7502,
//! Dynamite_SpawnedActor @1799, Jerry_SpawnedActor @21802): the victim of Shaggy's team differs from a control run
//! where the dealer's box is off.

use std::sync::Arc;

use mvs_data::value::{Props, Value};
use mvs_sim::Real;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, ActorRef, ProjectileId};
use mvs_sim::chars::tom_and_jerry::jerry::{jerry_mut, spawn_jerry};
use mvs_sim::combat::HitPacket;
use mvs_sim::combat::hits::{DamageCall, do_damage};
use mvs_sim::events::NotifyRef;
use mvs_sim::fighter::match_side::{Foreign, SpawnRequest};
use mvs_sim::fighter::moves::V2;
use mvs_sim::input::NO_INPUT;
use mvs_sim::names::Name;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::spawning::{comp_spawn_projectile, destroy_projectile, spawn_world_actor};
use mvs_sim_trace::harness::{self, Harness};

fn player(c: &str, team: u8) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), team: Some(team), ..MatchPlayer::default() }
}

/// Tom (fighter 0, team 0) and Shaggy (fighter 1, team 1), ten ticks in.
fn pair() -> Option<Harness> {
    let setup = MatchSetup { players: vec![player("tomandjerry", 0), player("shaggy", 1)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    Some(h)
}

fn step(h: &mut Harness) {
    h.step(&[NO_INPUT, NO_INPUT]);
}

/// A place high above both fighters: none of their hurt shapes is in reach of a 300 sphere there.
fn sky(h: &Harness) -> (Real, Real) {
    let f = h.fighter(0);
    (h.w.fighter(f).body.x, h.w.fighter(f).body.y + 400.0)
}

fn trap_at(h: &mut Harness, x: Real, y: Real) -> ActorId {
    let tom = h.fighter(0);
    let r = SpawnRequest {
        cls: h.w.lit("MouseTrap_SpawnedActor_C"),
        n: NotifyRef { montage: Name::NONE, index: 0 },
        attack_id: h.w.fighter(tom).decay.attack_count,
        foreign: Foreign::None,
        copy: None,
        charge: None,
        aim: None,
        props: Some(Arc::new(Props::new())),
        instigator: None,
        origin: Some(V2 { x, y }),
    };
    let id = spawn_world_actor(&mut h.w, tom, &r).expect("the trap spawns");
    if let WorldActorState::MouseTrap(t) = &mut h.w.actors.actors.get_mut(id).expect("it lives").state {
        (t.x, t.y) = (x, y);
    }
    id
}

fn trap_on(h: &mut Harness, id: ActorId, on: bool) {
    if let WorldActorState::MouseTrap(t) = &mut h.w.actors.actors.get_mut(id).expect("it lives").state {
        t.hit_on = on;
        t.hit.destroy_projectiles = Some(true);
        t.hit.damage = Some(5.0);
    }
}

/// Shaggy's sandwich projectile (an enemy of Tom's) at (x, y), standing still.
fn enemy_projectile(h: &mut Harness, x: Real, y: Real) -> ProjectileId {
    let s = h.fighter(1);
    let cls = h.w.lit("ShaggySandwichProjectile_C");
    let p = comp_spawn_projectile(&mut h.w, s, cls, x, y, [Some(0.0), Some(0.0), Some(0.0)], 0.0, 0.0, None)
        .expect("the projectile spawns");
    // its own hit box stays off: it is a victim here, not a dealer
    h.w.actors.projectiles.get_mut(p).expect("it lives").hit_on = false;
    p
}

fn projectile_state(h: &Harness, p: ProjectileId) -> String {
    match h.w.actors.projectiles.get(p) {
        Some(p) => format!("{p:?}"),
        None => "gone".to_owned(),
    }
}

/// A detonating trap over Shaggy's projectile: the projectile differs from one under a resting trap. (An item is not
/// tried: the trap's HitBox template answers no ItemDestructible, and a thrown item's Sphere is no Item type, so no
/// item is gathered for it.)
#[test]
fn a_detonating_trap_deals_to_an_enemy_projectile() {
    let run = |on: bool| -> Option<String> {
        let mut h = pair()?;
        let (x, y) = sky(&h);
        let t = trap_at(&mut h, x, y);
        let p = enemy_projectile(&mut h, x, y);
        step(&mut h);
        trap_on(&mut h, t, on);
        step(&mut h);
        Some(projectile_state(&h, p))
    };
    let (Some(off), Some(on)) = (run(false), run(true)) else { return };
    assert_ne!(off, on, "the trap's hit box reached the projectile ({off})");
}

/// Dynamite_SpawnedActor's explosion (the actor ends: ReceiveEndPlay -> Attempt to Detonate) next to Morty's grenade
/// (HitByEnemy @3723 bounces it once more: its sound): a grenade of the other team takes the blast, one of Tom's own
/// team (IsVulnerableTo refuses it) does not. (An item is not tried: a thrown item is IsProjectile and takes no damage
/// from the explosion's hit, which has no DestroyProjectiles.)
#[test]
fn a_dynamite_explosion_deals_to_an_enemy_projectile_alone() {
    use mvs_sim::events::Event;
    let run = |morty_team: u8| -> Option<usize> {
        let setup = MatchSetup {
            players: vec![player("tomandjerry", 0), player("shaggy", 1), player("morty", morty_team)],
            ..MatchSetup::default()
        };
        let mut h = harness::fresh(&setup, 1)?;
        for _ in 0..10 {
            h.step(&[NO_INPUT; 3]);
        }
        let (tom, morty) = (h.fighter(0), h.fighter(2));
        let (x, y) = (h.w.fighter(tom).body.x, h.w.fighter(tom).body.y + 400.0);
        let cls = h.w.lit("C019_CookGrenade_C");
        let g = comp_spawn_projectile(&mut h.w, morty, cls, x, y, [Some(1.0), Some(0.0), Some(0.0)], 0.0, 0.0, None)
            .expect("the grenade spawns");
        h.w.actors.projectiles.get_mut(g).expect("it lives").hit_on = false;
        let cls = h.w.lit("Dynamite_SpawnedActor_C");
        let d =
            comp_spawn_projectile(&mut h.w, tom, cls, x - 1500.0, y, [Some(0.0), Some(0.0), Some(0.0)], 0.0, 0.0, None)
                .expect("the dynamite spawns");
        h.step(&[NO_INPUT; 3]);
        let pd = h.w.actors.projectiles.get_mut(d).expect("it lives");
        (pd.x, pd.y) = (x, y);
        h.w.events.clear();
        destroy_projectile(&mut h.w, d, false);
        let bounce = h.w.lit("sfx_cha_C019_grenade_bounce");
        Some(h.w.events.iter().filter(|e| matches!(e, Event::Sound { ev, .. } if *ev == bounce)).count())
    };
    let (Some(enemy), Some(ally)) = (run(1), run(0)) else { return };
    assert_eq!(enemy, ally + 1, "the enemy grenade bounced once more from HitByEnemy");
}

/// Jerry's hit box (on) over Shaggy's projectile, against a Jerry with it off.
#[test]
fn jerrys_hit_box_deals_to_an_enemy_projectile() {
    let run = |on: bool| -> Option<String> {
        let mut h = pair()?;
        let tom = h.fighter(0);
        let (x, y) = sky(&h);
        let p = enemy_projectile(&mut h, x, y);
        let j = spawn_jerry(&mut h.w, tom, x, y, 0.0, 0.0, false, 0.0).expect("Jerry spawns");
        let s = jerry_mut(&mut h.w, j).expect("Jerry lives");
        s.damage_disabled = true; // the enemy projectile does not end him
        s.hit_on = on;
        s.moving = false;
        step(&mut h);
        step(&mut h);
        Some(projectile_state(&h, p))
    };
    let (Some(off), Some(on)) = (run(false), run(true)) else { return };
    assert_ne!(off, on, "Jerry's hit box reached the projectile ({off})");
}

/// @21594-21802: a Heavy projectile's hit that ends Jerry while his hit box is on makes his own hit on it.
#[test]
fn a_heavy_projectile_that_ends_jerry_gets_his_death_hit() {
    let run = |on: bool| -> Option<String> {
        let mut h = pair()?;
        let tom = h.fighter(0);
        let (x, y) = sky(&h);
        let p = enemy_projectile(&mut h, x, y);
        let j = spawn_jerry(&mut h.w, tom, x, y, 0.0, 0.0, false, 0.0).expect("Jerry spawns");
        {
            let pr = h.w.actors.projectiles.get_mut(p).expect("it lives");
            let mut np = pr.own_p.as_deref().cloned().unwrap_or_default();
            np.insert("ProjectileAttackType".to_owned(), Value::Num(1.0));
            pr.own_p = Some(Arc::new(np));
        }
        let s = jerry_mut(&mut h.w, j).expect("Jerry lives");
        s.hit_on = on;
        s.moving = false;
        s.hp = 1.0;
        s.hit.destroy_projectiles = Some(true);
        s.hit.damage = Some(5.0);
        s.hit.hit_pause_time = Some(0.5);
        let killer = HitPacket { damage: Some(50.0), ..HitPacket::default() };
        let ctx = DamageCall { hit_box: false, comp: None, clank: false, rows: Some(&[]) };
        h.w.events.clear();
        do_damage(&mut h.w, ActorRef::Projectile(p), ActorRef::Actor(j), &killer, &ctx);
        Some(format!("{} {:?} {:?}", projectile_state(&h, p), h.w.events, h.record()))
    };
    let (Some(off), Some(on)) = (run(false), run(true)) else { return };
    assert_ne!(off, on, "Jerry's death hit reached the projectile ({})", off.len());
}
