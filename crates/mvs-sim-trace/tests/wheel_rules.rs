//! The Chaos Wheel's own rules (`mvs_sim::world::own_rules`) and the build's mutators that joined its pool (Grow,
//! Sudden Death, Shrinking Arena), each turned on by the wheel and off again: what it changes while on, and that its
//! end puts back what the wheel says it does.

use mvs_sim::fighter::FighterId;
use mvs_sim::setup::{MatchPlayer, MatchSetup, Rules, TeamStyle, Wheel};
use mvs_sim::world::own_rules::{self, Rule};
use mvs_sim::world::wheel::change_to;
use mvs_sim::{Event, InputState, Real, World};
use mvs_sim_trace::harness::{self, Harness};

/// The wheel's own rules and the pool's joins, in `mvs_mutators::ALL`'s order.
const POOL: [&str; 14] = [
    "Mutator_GrowWithDamage",
    "Mutator_InstantKill",
    "Mutator_ShrinkBlastBoxLastPoint",
    "Chaos_MoonJump",
    "Chaos_Featherweight",
    "Chaos_Heavyweight",
    "Chaos_Vampire",
    "Chaos_DodgeFrenzy",
    "Chaos_BombRain",
    "Chaos_ProjectileParty",
    "Chaos_LavaFloor",
    "Chaos_Ghosts",
    "Chaos_Flood",
    "Chaos_StormFlood",
];

fn at(key: &str) -> usize {
    POOL.iter().position(|&k| k == key).expect("in the pool")
}

fn player(ch: &str) -> MatchPlayer {
    MatchPlayer { bot: false, character: Some(ch.to_owned()), ..MatchPlayer::default() }
}

/// A 1v1 with the pool, its clock too long to change by itself, its fighters landed.
fn landed() -> Option<Harness> {
    let s = MatchSetup {
        team_style: TeamStyle::OneVsOne,
        players: vec![player("superman"), player("shaggy")],
        rules: Some(Rules { knockouts_to_win: 99.0, duration: 420.0, stocks: false }),
        wheel: Some(Wheel { seconds: 10_000.0, warn: 1.0, ..Wheel::of(POOL.iter().map(|&s| s.to_owned()).collect()) }),
        ..MatchSetup::default()
    };
    let mut h = harness::fresh(&s, 11)?;
    idle(&mut h, 120);
    Some(h)
}

fn idle(h: &mut Harness, n: usize) {
    for _ in 0..n {
        h.step_recorded();
    }
}

fn damage(w: &World, f: FighterId) -> Real {
    w.fighter(f).stats.damage
}

/// The highest the first fighter rises over a held jump.
fn jump_rise(h: &mut Harness) -> Real {
    let f = h.fighter(0);
    let y0 = h.w.fighter(f).body.y;
    let mut i = vec![InputState::default(); h.w.players.len()];
    i[0].jump = true;
    let mut top = y0;
    for _ in 0..90 {
        h.step(&i);
        top = top.max(h.w.fighter(f).body.y);
    }
    top - y0
}

#[test]
fn moon_jump_jumps_about_twice_as_high() {
    let Some(mut plain) = landed() else { return };
    let Some(mut moon) = landed() else { return };
    change_to(&mut moon.w, Some(at("Chaos_MoonJump")));
    let (a, b) = (jump_rise(&mut plain), jump_rise(&mut moon));
    assert!(a > 50.0, "a plain jump rises {a}");
    assert!((1.6..2.4).contains(&(b / a)), "plain {a}, moon {b}");
}

#[test]
fn featherweight_and_heavyweight_scale_the_knockback_taken() {
    let Some(mut h) = landed() else { return };
    let f = h.fighter(0);
    let kb = |w: &World| mvs_sim::combat::buffs::apply_knock_back_multipliers(w, f, 100.0, 100.0, true);
    let plain = kb(&h.w);
    change_to(&mut h.w, Some(at("Chaos_Featherweight")));
    assert_eq!(kb(&h.w), (plain.0 * 1.5, plain.1 * 1.5));
    change_to(&mut h.w, Some(at("Chaos_Heavyweight")));
    assert_eq!(kb(&h.w), (plain.0 * 0.6, plain.1 * 0.6));
    change_to(&mut h.w, None);
    assert_eq!(kb(&h.w), plain);
}

#[test]
fn vampire_heals_half_the_damage_dealt() {
    let Some(mut h) = landed() else { return };
    let (a, b) = (h.fighter(0), h.fighter(1));
    h.w.fighter_mut(a).stats.damage = 50.0;
    change_to(&mut h.w, Some(at("Chaos_Vampire")));
    mvs_sim::combat::hit::apply_direct_damage(&mut h.w, b, 20.0, Some(a));
    assert_eq!((damage(&h.w, a), damage(&h.w, b)), (40.0, 20.0));
    change_to(&mut h.w, None);
    mvs_sim::combat::hit::apply_direct_damage(&mut h.w, b, 20.0, Some(a));
    assert_eq!((damage(&h.w, a), damage(&h.w, b)), (40.0, 40.0));
}

#[test]
fn dodge_frenzy_spends_no_meter_and_never_runs_out_of_air_dodges() {
    let Some(mut h) = landed() else { return };
    let f = h.fighter(0);
    change_to(&mut h.w, Some(at("Chaos_DodgeFrenzy")));
    assert_eq!(own_rules::air_evade_max(&h.w), Real::MAX);
    let before = h.w.fighter(f).evade.evade_count;
    mvs_sim::fighter::control::update_evade_values(&mut h.w, f);
    assert_eq!(h.w.fighter(f).evade.evade_count, before);
    change_to(&mut h.w, None);
    assert_eq!(own_rules::air_evade_max(&h.w), 2.0);
    mvs_sim::fighter::control::update_evade_values(&mut h.w, f);
    assert!(h.w.fighter(f).evade.evade_count > before);
}

#[test]
fn rising_lava_floods_the_floor_and_bounces_those_it_touches() {
    let Some(mut h) = landed() else { return };
    let fs = [h.fighter(0), h.fighter(1)];
    let before: Vec<Real> = fs.iter().map(|&f| damage(&h.w, f)).collect();
    assert_eq!(own_rules::lava_level(&h.w), None);
    change_to(&mut h.w, Some(at("Chaos_LavaFloor")));
    let low = own_rules::lava_level(&h.w).expect("on");
    let floor = h.w.fighter(fs[0]).body.y;
    let mut top = floor;
    let mut hiss = 0;
    for _ in 0..own_rules::LAVA_RISE_TICKS + 120 {
        h.step_recorded();
        top = top.max(h.w.fighter(fs[0]).body.y);
        let names = &h.w.names;
        hiss +=
            h.w.events
                .iter()
                .filter(|e| matches!(e, Event::Sound { ev, .. } if names.str(*ev) == own_rules::LAVA_HIT_SOUND))
                .count();
    }
    let high = own_rules::lava_level(&h.w).expect("on");
    assert!(high > low + 500.0, "the lava rises from {low} to {high}");
    for (n, &f) in fs.iter().enumerate() {
        let d = damage(&h.w, f) - before[n];
        assert!(d >= own_rules::LAVA_DAMAGE, "fighter {n} took {d}");
        assert_eq!(d % own_rules::LAVA_DAMAGE, 0.0, "fighter {n} took {d}");
    }
    assert!(top - floor > 200.0, "a bounce rises {}", top - floor);
    assert!(hiss >= 2, "a sound per touch: {hiss}");
    change_to(&mut h.w, None);
    assert_eq!(own_rules::lava_level(&h.w), None);
}

#[test]
fn bomb_rain_and_projectile_party_run_the_spawner_with_their_share_of_the_list() {
    for (key, keep) in [
        ("Chaos_BombRain", Rule::BombRain.items().expect("a list")),
        ("Chaos_ProjectileParty", Rule::ProjectileParty.items().expect("a list")),
    ] {
        let Some(mut h) = landed() else { return };
        if h.w.item_area.is_none() {
            panic!("no item area");
        }
        change_to(&mut h.w, Some(at(key)));
        let ar = h.w.item_area.as_ref().expect("an item area");
        let items: Vec<&str> = ar.items.iter().map(|&n| h.w.names.str(n)).collect();
        assert!(!items.is_empty(), "{key}: no item");
        assert!(items.iter().all(|i| keep.contains(i)), "{key}: {items:?}");
        assert!(ar.timer_at >= 0, "{key}: the spawner runs");
        assert_eq!(mvs_sim::world::wheel::item_rate(&h.w), Some(own_rules::ITEMS_EVERY));
        change_to(&mut h.w, None);
        let ar = h.w.item_area.as_ref().expect("an item area");
        assert!(ar.timer_at < 0 && ar.pending_at < 0, "{key}: the spawner stops with its window");
    }
}

#[test]
fn sudden_death_gives_200_and_takes_it_back() {
    let Some(mut h) = landed() else { return };
    let f = h.fighter(0);
    let before = damage(&h.w, f);
    change_to(&mut h.w, Some(at("Mutator_InstantKill")));
    idle(&mut h, 30);
    assert_eq!(damage(&h.w, f), before + 200.0);
    change_to(&mut h.w, None);
    idle(&mut h, 5);
    assert_eq!(damage(&h.w, f), before);
}

#[test]
fn shrinking_arena_closes_the_blast_box_and_opens_it_at_the_end() {
    let Some(mut h) = landed() else { return };
    assert!(h.w.stage.shrunk.is_some(), "the stage has a shrunk box");
    let whole = h.w.stage.ko_box;
    change_to(&mut h.w, Some(at("Mutator_ShrinkBlastBoxLastPoint")));
    idle(&mut h, 600);
    assert!(h.w.flow.blast_shrink > 0.0, "shrinking");
    assert_ne!(h.w.stage.ko_box, whole);
    change_to(&mut h.w, None);
    idle(&mut h, 2);
    assert_eq!(h.w.flow.blast_shrink, 0.0);
    assert_eq!(h.w.stage.ko_box, whole);
}

#[test]
fn ghosts_hide_a_fighter_but_for_a_flash_each_second() {
    let Some(mut h) = landed() else { return };
    let f = h.fighter(0);
    change_to(&mut h.w, Some(at("Chaos_Ghosts")));
    let mut shown = 0;
    for _ in 0..60 {
        idle(&mut h, 1);
        shown += usize::from(!own_rules::ghost_hidden(&h.w, f));
    }
    assert_eq!(shown, 8, "shown 8 ticks of 60");
    change_to(&mut h.w, None);
    assert!(!own_rules::ghost_hidden(&h.w, f));
}

#[test]
fn grow_joins_the_pool_and_leaves_a_fighter_as_found() {
    let Some(mut h) = landed() else { return };
    let f = h.fighter(0);
    let size = h.w.fighter(f).body.size_scale;
    change_to(&mut h.w, Some(at("Mutator_GrowWithDamage")));
    idle(&mut h, 10);
    let on = h.w.fighter(f).buffs.buffs.iter().any(|b| h.w.names.str(b.cls) == "Mutator_GrowWithDamage_C");
    assert!(on, "the buff is on");
    change_to(&mut h.w, None);
    idle(&mut h, 10);
    assert_eq!(h.w.fighter(f).body.size_scale, size);
}

#[test]
fn flood_and_storm_flood_raise_their_water_and_drain_it_with_the_window() {
    use own_rules::WaterStyle;
    for (key, style) in [("Chaos_Flood", WaterStyle::Tropical), ("Chaos_StormFlood", WaterStyle::Storm)] {
        let Some(mut h) = landed() else { return };
        let f = h.fighter(0);
        assert_eq!(own_rules::water_at(&h.w, 0.0), None);
        change_to(&mut h.w, Some(at(key)));
        idle(&mut h, usize::try_from(own_rules::WATER_RISE_TICKS).expect("ticks") + 10);
        let wa = own_rules::water(&h.w).expect("on");
        assert_eq!(wa.style, style, "{key}");
        assert_eq!(wa.risen, 1.0, "{key}: settled");
        assert!(own_rules::water_depth(&h.w, f) > 0.0, "{key}: a standing fighter is in it");
        assert!(own_rules::water_speed(&h.w, f) < 1.0, "{key}: and slowed");
        change_to(&mut h.w, None);
        idle(&mut h, 2);
        assert_eq!(own_rules::water_at(&h.w, 0.0), None, "{key}: gone with its window");
        assert_eq!(own_rules::water_speed(&h.w, f), 1.0, "{key}");
    }
}
