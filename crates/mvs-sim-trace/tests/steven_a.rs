//! Paths of Steven's Watermelon and Green Thumb boost that no match runs, on a World loaded from `steven-shaggy` at
//! frame 400 (fighter 0 is Steven; the harness loads no minion, so each test spawns the Watermelon by its own load
//! completion). With no baseline trace the tests check nothing and say so.

use mvs_sim::chars::steven::state::PendingSpawn;
use mvs_sim::chars::steven::{steven_of, steven_w, watermelon};
use mvs_sim::combat::buffs::spawn_buff;
use mvs_sim::events::Event;
use mvs_sim::registry::CharHooks;
use mvs_sim_trace::harness::{self, Harness};

/// Steven at frame 400 with a Watermelon spawned at his feet by the load's completion (`timer_tick`).
fn at_melon() -> Option<Harness> {
    let mut h = harness::at("steven-shaggy", 400)?;
    let f = h.fighter(0);
    let at = PendingSpawn { x: h.w.fighter(f).body.x, y: h.w.fighter(f).body.y };
    steven_w(&mut h.w, f).watermelon.pending_spawn = Some(at);
    (hooks().timer_tick.expect("the hook"))(&mut h.w, f);
    let m = steven_of(&h.w, f).and_then(|s| s.pawn.watermelon_steven).expect("the Watermelon is up");
    assert!(in_world(&h, m));
    assert!(steven_of(&h.w, m).is_some_and(|s| s.watermelon.is_watermelon_steven));
    assert_eq!(mvs_sim::fighter::weight(&h.w, m), 0.01f32);
    Some(h)
}

fn melon(h: &Harness) -> mvs_sim::fighter::FighterId {
    let f = h.fighter(0);
    steven_of(&h.w, f).and_then(|s| s.pawn.watermelon_steven).expect("a Watermelon")
}

fn hooks() -> CharHooks {
    let mut h = CharHooks::default();
    watermelon::hooks(&mut h);
    h
}

fn in_world(h: &Harness, m: mvs_sim::fighter::FighterId) -> bool {
    h.w.pawns.as_ref().is_some_and(|ps| ps.contains(&m))
}

fn destroyed_effects(h: &Harness) -> usize {
    h.w.events
        .iter()
        .filter(|e| matches!(e, Event::ProjectileDeath { vfx: Some(v), .. } if h.w.names.str(*v) == "PS_SteveWatermelon_Destroyed"))
        .count()
}

#[test]
fn a_defeated_watermelon_is_dropped_from_its_creator_and_the_world() {
    let Some(mut h) = at_melon() else { return };
    let (f, m) = (h.fighter(0), melon(&h));
    (hooks().minion_defeated.expect("the hook"))(&mut h.w, f, m);
    assert_eq!(steven_of(&h.w, f).and_then(|s| s.pawn.watermelon_steven), None);
    assert!(!in_world(&h, m));
    // a minion that is no Watermelon is left alone: Steven himself
    let f2 = h.fighter(1);
    let n = h.w.pawns.as_ref().map_or(0, Vec::len);
    (hooks().minion_defeated.expect("the hook"))(&mut h.w, f, f2);
    assert_eq!(h.w.pawns.as_ref().map_or(0, Vec::len), n);
}

#[test]
fn a_reset_destroys_the_stored_watermelon_in_the_next_timer_pass_with_its_effects() {
    let Some(mut h) = at_melon() else { return };
    let (f, m) = (h.fighter(0), melon(&h));
    watermelon::reset(&mut h.w, f);
    assert!(steven_of(&h.w, f).is_some_and(|s| s.watermelon.cleanup_pending));
    assert!(in_world(&h, m), "marked, not yet destroyed");
    h.w.events.clear();
    (hooks().timer_tick.expect("the hook"))(&mut h.w, f);
    assert!(!in_world(&h, m));
    assert_eq!(steven_of(&h.w, f).and_then(|s| s.pawn.watermelon_steven), None);
    assert!(steven_of(&h.w, f).is_some_and(|s| !s.watermelon.cleanup_pending));
    assert_eq!(destroyed_effects(&h), 1);
}

#[test]
fn only_stevens_own_ring_out_destroys_his_watermelon() {
    let Some(mut h) = at_melon() else { return };
    let (f, m, other) = (h.fighter(0), melon(&h), h.fighter(1));
    (hooks().player_knocked_out.expect("the hook"))(&mut h.w, f, other);
    assert!(in_world(&h, m), "another's ring-out leaves it");
    h.w.events.clear();
    (hooks().player_knocked_out.expect("the hook"))(&mut h.w, f, f);
    assert!(!in_world(&h, m));
    assert_eq!(destroyed_effects(&h), 1);
}

#[test]
fn the_green_thumb_boost_grows_the_owner_and_adds_weight_along_its_timeline() {
    let Some(mut h) = at_melon() else { return };
    let f = h.fighter(0);
    let boost = h.w.names.get("Buff_Steven_GreenThumbDamageBoost_C").expect("interned");
    let w0 = mvs_sim::fighter::weight(&h.w, f);
    spawn_buff(&mut h.w, f, boost, 0.0, Some(0), Some(f), true, None);
    assert_eq!(h.w.fighter(f).body.size_scale, 1.0, "alpha 0 at the begin");
    assert_eq!(mvs_sim::fighter::weight(&h.w, f), w0);
    for _ in 0..60 {
        h.step_recorded();
    }
    assert!(h.w.fighter(f).body.size_scale > 1.0, "grown");
    assert!(mvs_sim::fighter::weight(&h.w, f) > w0, "heavier");
}

/// HealGrenade's movement is forced inactive: the scaled speed, the reverse and the carry see it
/// off even for a projectile that is moving.
#[test]
fn the_heal_grenades_movement_is_never_active() {
    use mvs_sim::actors::projectile::{ProjectileExt, movement_active, scale_speed};
    use mvs_sim::actors::{Projectile, ProjectileClass};
    use mvs_sim::chars::steven::watermelon::HealGrenade;
    let mut p = Projectile::new(mvs_sim::names::Name::NONE, ProjectileClass::HealGrenade, None, 0, 0);
    p.moving = true;
    p.attached = false;
    assert!(movement_active(&p), "a moving projectile of no kit's class is active");
    p.ext = ProjectileExt::HealGrenade(HealGrenade::default());
    assert!(!movement_active(&p));
    p.vx = 10.0;
    assert!(!scale_speed(&mut p, 2.0, false));
    assert_eq!(p.vx, 10.0);
}
