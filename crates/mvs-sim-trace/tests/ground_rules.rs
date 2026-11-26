//! The port's own Ice Rink, Wind, Black Hole and Flood (`mvs_sim::world::own_rules`), each on all match
//! from the set-up's mutators (the Mutators row) as the wheel's window turns it on for its window.

use mvs_sim::fighter::FighterId;
use mvs_sim::setup::{MatchPlayer, MatchSetup, Rules};
use mvs_sim::world::own_rules;
use mvs_sim::{Event, InputState, Real};
use mvs_sim_trace::harness::{self, Harness};

fn player(ch: &str) -> MatchPlayer {
    MatchPlayer { bot: false, character: Some(ch.to_owned()), ..MatchPlayer::default() }
}

/// A 1v1 with `rule` on all match (none for ""), its fighters landed.
fn landed(rule: &str) -> Option<Harness> {
    let s = MatchSetup {
        players: vec![player("superman"), player("shaggy")],
        rules: Some(Rules { knockouts_to_win: 99.0, duration: 420.0, stocks: false }),
        mutators: if rule.is_empty() { Vec::new() } else { vec![rule.to_owned()] },
        ..MatchSetup::default()
    };
    let mut h = harness::fresh(&s, 11)?;
    idle(&mut h, 120);
    Some(h)
}

fn idle(h: &mut Harness, n: usize) {
    for _ in 0..n {
        h.step(&[InputState::default(); 2]);
    }
}

fn x(h: &Harness, f: FighterId) -> Real {
    h.w.fighter(f).body.x
}

fn slide(rule: &str) -> Option<Real> {
    let mut h = landed(rule)?;
    let f = h.fighter(0);
    let mut i = [InputState::default(); 2];
    i[0].x = 1.0;
    for _ in 0..40 {
        h.step(&i);
    }
    let x0 = x(&h, f);
    idle(&mut h, 40);
    Some(x(&h, f) - x0)
}

#[test]
fn ice_rink_slides_further_after_a_walk() {
    let (Some(dry), Some(ice)) = (slide(""), slide("Chaos_IceRink")) else { return };
    assert!(ice > dry * 3.0 && ice > 100.0, "slid {ice} on ice, {dry} without");
}

#[test]
fn wind_gusts_blow_a_standing_fighter_the_gusts_way() {
    use mvs_sim::world::own_rules::gust_at;
    let Some(mut h) = landed("Chaos_Wind") else { return };
    let f = h.fighter(1);
    // (to the next gust's start)
    let mut n = 0;
    while gust_at(&h.w, 0.0).is_none_or(|g| g.blow <= 0.0) {
        idle(&mut h, 1);
        n += 1;
        assert!(n < 600, "no gust in 10 s");
    }
    let dir = gust_at(&h.w, 0.0).expect("a gust").dir;
    let x0 = x(&h, f);
    idle(&mut h, 60);
    let moved = x(&h, f) - x0;
    assert!(moved * dir > 100.0, "blown {moved}, the gust's way {dir}");
    // (past its end: calm)
    idle(&mut h, 80);
    assert!(gust_at(&h.w, 0.0).is_none_or(|g| g.blow <= 0.0), "a gust lasts its length");
}

#[test]
fn black_hole_pulls_fighters_in_and_throws_them_out() {
    use mvs_sim::world::own_rules::{HOLE_OPEN, hole_at};
    let Some(mut h) = landed("Chaos_Magnet") else { return };
    let hole = hole_at(&h.w, 0.0).expect("the hole open");
    let fs = [h.fighter(0), h.fighter(1)];
    let d0: Vec<Real> = fs.iter().map(|&f| (x(&h, f) - hole.x).abs()).collect();
    idle(&mut h, 30);
    for (n, &f) in fs.iter().enumerate() {
        let d = (x(&h, f) - hole.x).abs();
        assert!(d < d0[n] - 3.0 || d < 1.0, "fighter {n}: {} -> {d}", d0[n]);
    }
    // A fighter put in its core is hit and thrown out.
    idle(&mut h, usize::try_from(HOLE_OPEN).expect("ticks"));
    let f = fs[1];
    let dmg = h.w.fighter(f).stats.damage;
    {
        let b = &mut h.w.fighter_mut(f).body;
        b.x = hole.x + 20.0;
        b.y = hole.y;
    }
    idle(&mut h, 20);
    assert!(h.w.fighter(f).stats.damage > dmg, "the core hurts");
    assert!(h.w.fighter(f).body.x > hole.x + 60.0, "thrown out its side: {}", h.w.fighter(f).body.x);
}

/// A 1v1 with the flood `rule` on all match (none for ""), its water settled over the main floor.
fn flooded(rule: &str) -> Option<Harness> {
    let mut h = landed(rule)?;
    let mut n = 0;
    while !rule.is_empty() && own_rules::water(&h.w).is_none_or(|wa| wa.risen < 1.0) {
        idle(&mut h, 1);
        n += 1;
        assert!(n < 600, "the water settles");
    }
    Some(h)
}

/// Puts `f` still and off the ground beside the stage's right side, its middle `below` under where a flood settles;
/// that surface.
fn off_stage(h: &mut Harness, f: FighterId, below: Real) -> Real {
    let g = &h.w.stage.ground;
    let top = mvs_sim::math::narrow(g.top) + own_rules::WATER_OVER;
    let x = mvs_sim::math::narrow(g.right) + 600.0;
    assert!(top - below - 400.0 > h.w.stage.blast.bottom && x + 400.0 < h.w.stage.blast.right, "room off the stage");
    mvs_sim::fighter::set_grounded(&mut h.w, f, false);
    let b = &mut h.w.fighter_mut(f).body;
    b.x = x;
    b.y = top - below;
    b.vx = 0.0;
    b.vy = 0.0;
    top
}

/// The sounds named `name` among the step's events.
fn sounds(h: &Harness, name: &str) -> usize {
    let names = &h.w.names;
    h.w.events.iter().filter(|e| matches!(e, Event::Sound { ev, .. } if names.str(*ev) == name)).count()
}

#[test]
fn flood_floats_a_fighter_up_to_its_surface() {
    let Some(mut h) = flooded("Chaos_Flood") else { return };
    let f = h.fighter(1);
    // Dropped in from over the surface: a splash.
    off_stage(&mut h, f, -300.0);
    idle(&mut h, 1); // (its way out of the water it stood in)
    let mut splash = 0;
    for _ in 0..30 {
        h.w.events.clear();
        idle(&mut h, 1);
        splash += sounds(&h, own_rules::WATER_SPLASH_SOUND) + sounds(&h, own_rules::WATER_SPLASH_BIG_SOUND);
    }
    assert!(h.w.fighter(f).body.wet && splash == 1, "wet, a splash in: {splash}");
    // Deep under it: floated up to rest at the surface.
    let top = off_stage(&mut h, f, 600.0);
    let y0 = h.w.fighter(f).body.y;
    idle(&mut h, 150);
    let fr = h.w.fighter(f);
    assert!(fr.body.y > y0 + 400.0, "floated from {y0} to {}", fr.body.y);
    assert!(fr.body.y < top + 60.0, "rests at the surface {top}: {}", fr.body.y);
    let d = own_rules::water_depth(&h.w, f);
    assert!(d > 0.2 && d < 1.0, "head and shoulders out: {d}");
}

/// The fastest `f` falls diving (the stick held down) from deep in the water, or the same place without one.
fn dive(rule: &str) -> Option<Real> {
    let mut h = flooded(rule)?;
    let f = h.fighter(1);
    off_stage(&mut h, f, 150.0);
    let mut i = [InputState::default(); 2];
    i[1].y = -1.0;
    let mut low: Real = 0.0;
    for _ in 0..30 {
        h.step(&i);
        low = low.min(h.w.fighter(f).body.vy);
    }
    Some(low)
}

#[test]
fn flood_slows_a_fall_and_a_dive_sinks() {
    let (Some(dry), Some(wet)) = (dive(""), dive("Chaos_Flood")) else { return };
    assert!(wet < -200.0, "a dive sinks: {wet}");
    assert!(wet >= -own_rules::WATER_FALL - 1.0, "at most the water's fall: {wet}");
    assert!(dry < 2.0 * wet, "out of the water {dry}");
}

/// How far a sideways launch sends the second fighter from deep in the water, or the same place without one.
fn launch(rule: &str) -> Option<Real> {
    use mvs_sim::combat::{DataEnum, HitPacket};
    use mvs_sim::fighter::moves::V2;
    let mut h = flooded(rule)?;
    let f = h.fighter(1);
    off_stage(&mut h, f, 400.0);
    let p = HitPacket {
        damage: Some(10.0),
        base_knock_back: Some(1800.0),
        knock_back_scaling: Some(0.0),
        hit_reaction: Some(DataEnum::Value(2)),
        hit_pause_time: Some(0.1),
        ignore_facing: Some(true),
        attack_id: Some(0),
        additive_kb_mode: Some(DataEnum::Value(0)),
        kb_scaling_starting_dmg: Some(0.0),
        armor_pierce: Some(true),
        kbi_multiplier: Some(1.0),
        ..HitPacket::default()
    };
    let b = &h.w.fighter(f).body;
    let at = V2 { x: b.x, y: b.y };
    let x0 = b.x;
    let cls = h.w.lit("Chaos_LavaFloor"); // (a rule's level hit, as the lava's)
    let dealer = mvs_sim::actors::ActorRef::Fighter(f);
    mvs_sim::combat::hits::apply_level_hit(&mut h.w, dealer, cls, f, &p, at, V2 { x: 0.8, y: 0.6 }, &[]);
    idle(&mut h, 40);
    Some(h.w.fighter(f).body.x - x0)
}

#[test]
fn flood_shortens_a_launch_but_still_sends_it() {
    let (Some(dry), Some(wet)) = (launch(""), launch("Chaos_Flood")) else { return };
    assert!(wet > 100.0, "a launch in the water still sends: {wet}");
    assert!(wet < dry * 0.8, "shorter in the water: {wet} against {dry}");
}

#[test]
fn flood_makes_a_jump_in_the_water_a_stroke_as_often_as_pressed() {
    let Some(mut h) = flooded("Chaos_Flood") else { return };
    let f = h.fighter(1);
    off_stage(&mut h, f, 600.0);
    idle(&mut h, 2);
    h.w.fighter_mut(f).jump.jumps = 9; // every air jump spent
    let mut i = [InputState::default(); 2];
    let mut strokes = 0;
    for _ in 0..3 {
        off_stage(&mut h, f, 600.0);
        let vy0 = h.w.fighter(f).body.vy;
        i[1].jump = true;
        h.w.events.clear();
        h.step(&i);
        strokes += sounds(&h, own_rules::WATER_STROKE_SOUND);
        i[1].jump = false;
        h.w.events.clear();
        h.step(&i);
        strokes += sounds(&h, own_rules::WATER_STROKE_SOUND);
        let vy = h.w.fighter(f).body.vy;
        assert!(vy > vy0 + 300.0, "a stroke pushes up: {vy0} -> {vy}");
        assert!(h.w.fighter(f).jump.jumps <= 2, "a stroke is the air jump's: {}", h.w.fighter(f).jump.jumps);
        idle(&mut h, 12);
    }
    assert_eq!(strokes, 3, "a stroke's sound each");
}
