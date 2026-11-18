//! pfgDoDamage's ForceKnockback tail on a hit CanBeHitBy refuses (@8516 goto 3222): a fighter's DestroyProjectiles
//! hit on a team mate's projectile runs the dealer's rumble (@3589-3668) and the actor's HitByEnemy (@3723) after
//! its HitByAlly (`fighterHitsProjectile`), and the rumble on an item (`fighterHitsItem`). On a fresh World; without a
//! store it checks nothing.

use mvs_sim::combat::melee_actors::{fighter_hits_item, fighter_hits_projectile};
use mvs_sim::combat::packet::HitPacket;
use mvs_sim::events::Event;
use mvs_sim::input::NO_INPUT;
use mvs_sim::name;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};

fn player(c: &str, team: u8) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), team: Some(team), bot: false, ..MatchPlayer::default() }
}

/// Morty (fighter 0) and Shaggy (fighter 1) on one team, ten ticks in.
fn pair() -> Option<Harness> {
    let setup = MatchSetup { players: vec![player("morty", 0), player("shaggy", 0)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    Some(h)
}

/// Shaggy's DestroyProjectiles hit on Morty's grenade: (rumbles of Shaggy, the grenade's bounce sounds).
fn hit_grenade(force_knockback: bool) -> Option<(usize, usize)> {
    let mut h = pair()?;
    let (m, s) = (h.fighter(0), h.fighter(1));
    let w = &mut h.w;
    let cls = w.lit("C019_CookGrenade_C");
    let (x, y) = (w.fighter(m).body.x, mvs_sim::fighter::actor_y(w, m));
    let g = mvs_sim::world::spawning::comp_spawn_projectile(
        w,
        m,
        cls,
        x,
        y + 300.0,
        [Some(1.0), Some(0.0), Some(0.0)],
        0.0,
        0.0,
        None,
    )
    .expect("the grenade spawns");
    let rumble = name!("sfx_cha_C019_grenade_bounce"); // any name: the rumble is only recorded
    w.fighter_mut(s).events.clear();
    w.events.clear();
    let p = HitPacket {
        destroy_projectiles: Some(true),
        force_knockback: Some(force_knockback),
        controller_rumble: Some(rumble),
        base_knock_back: Some(1500.0),
        ..HitPacket::default()
    };
    fighter_hits_projectile(w, s, g, &p, 1.0, 0.5, true, 0.0);
    let rumbles = w.fighter(s).events.iter().filter(|e| matches!(e, Event::Rumble { .. })).count();
    let bounce = name!("sfx_cha_C019_grenade_bounce");
    let sounds = w.events.iter().filter(|e| matches!(e, Event::Sound { ev, .. } if *ev == bounce)).count();
    Some((rumbles, sounds))
}

#[test]
fn a_refused_force_knockback_hit_rumbles_and_runs_hit_by_enemy() {
    let Some(plain) = hit_grenade(false) else { return };
    let Some(forced) = hit_grenade(true) else { return };
    assert_eq!(plain.0, 0, "no rumble on a refused hit");
    assert_eq!(forced.0, 1, "the dealer's rumble @3589");
    assert_eq!(forced.1, plain.1 + 1, "HitByEnemy @3723 bounces the grenade once more");
}

/// Morty's hit on a sandwich Shaggy owns, CanBeHitBy refusing a team mate: (rumbles of Morty).
fn hit_item(force_knockback: bool) -> Option<usize> {
    let mut h = pair()?;
    let (m, s) = (h.fighter(0), h.fighter(1));
    let w = &mut h.w;
    let cls = w.lit("Item_Sandwich_C");
    let (x, y) = (w.fighter(m).body.x, mvs_sim::fighter::actor_y(w, m));
    let it = mvs_sim::world::items::spawn_item(w, cls, s, x, y + 300.0, 0.0, 0.0, false, None).expect("the sandwich");
    let rumble = name!("sfx_cha_C019_grenade_bounce"); // any name: the rumble is only recorded
    w.fighter_mut(m).events.clear();
    let p = HitPacket {
        destroy_projectiles: Some(true), // the sandwich is IsProjectile: @19122 ends a hit without it
        force_knockback: Some(force_knockback),
        controller_rumble: Some(rumble),
        damage: Some(5.0),
        ..HitPacket::default()
    };
    let at = mvs_sim::fighter::moves::V2 { x, y: y + 300.0 };
    fighter_hits_item(w, m, it, &p, 1.0, 0.5, 0.0, true, at);
    Some(w.fighter(m).events.iter().filter(|e| matches!(e, Event::Rumble { .. })).count())
}

#[test]
fn a_refused_force_knockback_hit_on_an_item_rumbles() {
    let Some(plain) = hit_item(false) else { return };
    let Some(forced) = hit_item(true) else { return };
    assert_eq!((plain, forced), (0, 1), "the dealer's rumble @3589 on the refused hit with ForceKnockback alone");
}
