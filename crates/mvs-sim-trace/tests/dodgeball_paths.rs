//! The port's own Dodgeball rule (`mvs_sim::world::dodgeball`) where its trace matches cannot show it: each pool
//! item thrown across hits with its own numbers and credits the thrower's ring-out; what passes the barrier and what
//! does not; a fighter launched, placed or respawned past it ends on its half; the drops' halves, times, cap and
//! draws; the barrier on every stage; the rule with no barrier; and the record's round trip of a Dodgeball World.

use mvs_sim::actors::ItemId;
use mvs_sim::combat::hit::Launch;
use mvs_sim::fighter::{FighterId, cap_radius};
use mvs_sim::input::NO_INPUT;
use mvs_sim::setup::{Dodgeball, MatchPlayer, MatchSetup, Rules, TeamStyle};
use mvs_sim::world::dodgeball::{
    self, BARRIER_SPEC, DROP_CAP, DROP_CLEAR, DROP_EVERY, DROP_FIRST, HALF_WIDTH, barrier, lying_on,
};
use mvs_sim::world::items::{area_item_src, spawn_area_item};
use mvs_sim::world::stage::{SOLID_ITEM, SOLID_NAV, SOLID_TRACE};
use mvs_sim::{InputState, Real, World};
use mvs_sim_trace::harness::{self, Harness};

/// The stages of the import but the tutorial's.
const STAGES: [&str; 20] = [
    "trainingroomsmall",
    "m011_largenoplat",
    "m011_smallnoplat",
    "m011",
    "m011_small",
    "classic3platform_small",
    "classic3platform_large",
    "trainingroomlarge",
    "trophyroom_largeplatform",
    "trophyroom_2platforms",
    "treehouse",
    "treehousesmall",
    "treehouse_1plat_large",
    "m009",
    "m009_small",
    "scoobydoo",
    "scoobydoo_noroof",
    "batcave",
    "m008",
    "m010",
];

const POOL: [&str; 5] = ["Item_Rock_C", "Item_Pie_C", "Item_DrumStick_C", "Item_Snowball_C", "Item_SuperSnowball_C"];

fn player(ch: &str) -> MatchPlayer {
    MatchPlayer { bot: false, character: Some(ch.to_owned()), ..MatchPlayer::default() }
}

fn setup(chars: &[&str], rule: Option<Dodgeball>, flow: bool) -> MatchSetup {
    MatchSetup {
        team_style: if chars.len() == 4 { TeamStyle::TwoVsTwo } else { TeamStyle::OneVsOne },
        players: chars.iter().map(|c| player(c)).collect(),
        rules: Some(Rules { knockouts_to_win: 99.0, duration: 420.0, stocks: false }),
        flow,
        dodgeball: rule,
        ..MatchSetup::default()
    }
}

fn on(stage: &str, chars: &[&str], rule: Option<Dodgeball>, flow: bool, seed: i32) -> Option<Harness> {
    harness::fresh_on(&setup(chars, rule, flow), seed, stage)
}

fn idle(h: &mut Harness, n: usize) {
    let none = vec![NO_INPUT; h.w.players.len()];
    for _ in 0..n {
        h.step(&none);
    }
}

/// Superman (team 0, left) against Shaggy (team 1, right) on Sky Arena with the rule, landed.
fn duel(chars: [&str; 2], rule: Dodgeball) -> Option<Harness> {
    let mut h = on("m011", &chars, Some(rule), false, 5)?;
    idle(&mut h, 60);
    Some(h)
}

/// The place of `f`'s capsule face nearest the barrier on its own half: team 0's right side, team 1's left.
fn face(w: &World, f: FighterId) -> Real {
    HALF_WIDTH + cap_radius(w, f)
}

/// `f` is on its half with its capsule clear of the barrier.
fn on_half(w: &World, f: FighterId) -> bool {
    let x = w.fighter(f).body.x;
    match w.fighter(f).team.team {
        0 => x <= -face(w, f),
        _ => x >= face(w, f),
    }
}

fn spawn(w: &mut World, cls: &str, x: Real, y: Real) -> ItemId {
    let cls = w.lit(cls);
    let src = area_item_src(w, cls).expect("a pool class of the items part, spawned with no owner");
    spawn_area_item(w, cls, src, x, y).expect("the item spawns")
}

/// The live buffs on `f`.
fn buffs(w: &World, f: FighterId) -> Vec<String> {
    w.fighter(f).buffs.buffs.iter().filter(|b| b.frames > 0.0).map(|b| w.names.str(b.cls).to_owned()).collect()
}

/// Fighter 0 picks an item of `cls` up at its feet and throws it right, across the barrier, at fighter 1. Returns the
/// item and its hit packet's damage; the throw has left the hand when it returns.
fn throw_across(h: &mut Harness, cls: &str) -> (ItemId, Real) {
    let f0 = h.fighter(0);
    let (x, y) = (h.w.fighter(f0).body.x, mvs_sim::fighter::actor_y(&h.w, f0));
    let it = spawn(&mut h.w, cls, x + 30.0, y);
    idle(h, 20);
    h.step(&[InputState { item: true, ..NO_INPUT }, NO_INPUT]);
    idle(h, 20);
    assert!(h.w.actors.items.get(it).is_some_and(|i| i.held), "{cls}: picked up");
    let damage = h.w.actors.items.get(it).and_then(|i| i.hit.damage).expect("a hit damage");
    h.step(&[InputState { item: true, x: 1.0, ..NO_INPUT }, NO_INPUT]);
    for _ in 0..20 {
        if h.w.actors.items.get(it).is_some_and(|i| i.flying) {
            break;
        }
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    assert!(h.w.actors.items.get(it).is_some_and(|i| i.flying), "{cls}: thrown");
    (it, damage)
}

#[test]
fn each_pool_item_thrown_across_hits_with_its_own_damage_and_effect() {
    let mut bad = Vec::new();
    for cls in POOL {
        let Some(mut h) = duel(["superman", "shaggy"], Dodgeball::PLAY) else { return };
        let (f0, f1) = (h.fighter(0), h.fighter(1));
        let before = buffs(&h.w, f1);
        let (it, damage) = throw_across(&mut h, cls);
        let mut crossed = false;
        for _ in 0..90 {
            h.step(&[NO_INPUT, NO_INPUT]);
            crossed |= h.w.actors.items.get(it).is_some_and(|i| i.x > HALF_WIDTH);
            if h.w.fighter(f1).stats.damage > 0.0 {
                break;
            }
        }
        let got = h.w.fighter(f1).stats.damage;
        let gained: Vec<String> = buffs(&h.w, f1).into_iter().filter(|b| !before.contains(b)).collect();
        eprintln!("{cls}: damage {got} of {damage}, buffs gained {gained:?}");
        if !crossed {
            bad.push(format!("{cls}: never past the barrier in flight"));
        }
        if got != damage {
            bad.push(format!("{cls}: the opponent took {got}, the item deals {damage}"));
        }
        // (the Rock's hit gives no buff; each other one's its effect: the Pie's slow, the Drumstick's Chicken, the
        // snowballs' freeze stacks)
        if cls != "Item_Rock_C" && gained.is_empty() {
            bad.push(format!("{cls}: no effect on the opponent"));
        }
        if !on_half(&h.w, f0) {
            bad.push(format!("{cls}: the thrower left its half"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn a_ring_out_by_a_thrown_item_credits_the_thrower() {
    let Some(mut h) = duel(["superman", "shaggy"], Dodgeball::PLAY) else { return };
    let (f0, f1) = (h.fighter(0), h.fighter(1));
    h.w.fighter_mut(f1).stats.damage = 900.0;
    throw_across(&mut h, "Item_Rock_C");
    for _ in 0..600 {
        h.step(&[NO_INPUT, NO_INPUT]);
        if h.w.fighter(f1).life.dead != 0.0 {
            break;
        }
    }
    assert!(h.w.fighter(f1).life.dead != 0.0, "the opponent is rung out");
    assert_eq!(h.w.fighter(f0).stats.ring_outs, 1, "the thrower scores the ring-out");
    assert_eq!(h.w.flow.team_knockouts.first().copied(), Some(1.0), "its team scores it");
}

#[test]
fn an_item_lying_or_rolling_passes_the_barrier() {
    let Some(mut h) = duel(["superman", "shaggy"], Dodgeball::PLAY) else { return };
    // dropped onto the barrier from above: it falls through it to the stage under it
    let it = spawn(&mut h.w, "Item_Rock_C", 0.0, 2000.0);
    idle(&mut h, 240);
    let i = h.w.actors.items.get(it).expect("the rock is there");
    assert!(i.x.abs() < HALF_WIDTH && i.y < 1500.0, "the rock fell through the barrier: {} {}", i.x, i.y);
    // rolling across it on the floor
    let f0 = h.fighter(0);
    let y = mvs_sim::fighter::actor_y(&h.w, f0);
    let it = spawn(&mut h.w, "Item_Rock_C", -400.0, y);
    idle(&mut h, 10);
    h.w.actors.items.get_mut(it).expect("the rock").vx = 1500.0;
    let mut most = Real::MIN;
    for _ in 0..60 {
        h.step(&[NO_INPUT, NO_INPUT]);
        most = most.max(h.w.actors.items.get(it).map_or(Real::MIN, |i| i.x));
    }
    assert!(most > HALF_WIDTH, "the rock rolled past the barrier: its furthest x {most}");
}

/// Harley's air down glove shot right from the left half: the furthest x it reaches in its life.
fn glove(rule: Dodgeball) -> Option<Real> {
    let mut h = duel(["harley", "shaggy"], rule)?;
    let f0 = h.fighter(0);
    let cls = h.w.lit("HarleyDownAirProjectile_C");
    let id = mvs_sim::world::spawning::comp_spawn_projectile(
        &mut h.w,
        f0,
        cls,
        -400.0,
        700.0,
        [Some(0.0), Some(1.0), Some(0.0)],
        3000.0,
        3000.0,
        None,
    )
    .expect("the glove spawns");
    let mut most = Real::MIN;
    for _ in 0..40 {
        h.step(&[NO_INPUT, NO_INPUT]);
        match h.w.actors.projectiles.get(id) {
            Some(p) if !p.dead => most = most.max(p.x),
            _ => break,
        }
    }
    Some(most)
}

#[test]
fn a_fighters_projectile_does_not_pass_the_barrier() {
    let Some(free) = glove(Dodgeball { barrier: false }) else { return };
    assert!(free > 300.0, "with no barrier the glove reaches the right half: {free}");
    let walled = glove(Dodgeball::PLAY).expect("the same World");
    assert!(walled < -HALF_WIDTH, "the glove stops at the barrier: its furthest x {walled}");
}

#[test]
fn a_fighter_launched_at_the_barrier_stays_on_its_half() {
    let Some(mut h) = duel(["superman", "shaggy"], Dodgeball::PLAY) else { return };
    let f0 = h.fighter(0);
    h.w.fighter_mut(f0).body.x = -400.0;
    // the fastest knockback: about 125 units a half-step
    let mut l = Launch { vx: 15_000.0, vy: 200.0, di: false, wall: false, kbi: 0.0 };
    assert!(mvs_sim::combat::hit::launch::apply_knockback(&mut h.w, f0, &mut l, true), "launched");
    let mut most = Real::MIN;
    for t in 0..30 {
        h.step(&[NO_INPUT, NO_INPUT]);
        let x = h.w.fighter(f0).body.x;
        most = most.max(x);
        assert!(on_half(&h.w, f0), "tick {t}: x {x} past the face {}", -face(&h.w, f0));
    }
    assert_eq!(most, -face(&h.w, f0), "it reached the barrier");
}

#[test]
fn a_fighter_placed_past_the_barrier_is_put_back_within_a_tick() {
    let Some(mut h) = duel(["superman", "shaggy"], Dodgeball::PLAY) else { return };
    let (f0, f1) = (h.fighter(0), h.fighter(1));
    // a teleport onto the far half, and one into the barrier past its middle
    for (f, x) in [(f0, 600.0), (f1, -600.0), (f0, 30.0), (f1, -30.0)] {
        h.w.fighter_mut(f).body.x = x;
        h.w.fighter_mut(f).body.vx = if x > 0.0 { 900.0 } else { -900.0 };
        h.step(&[NO_INPUT, NO_INPUT]);
        let b = &h.w.fighter(f).body;
        let want = if h.w.fighter(f).team.team == 0 { -face(&h.w, f) } else { face(&h.w, f) };
        assert_eq!(b.x, want, "placed at {x}: put at its face");
        assert_eq!(b.vx, 0.0, "placed at {x}: its speed across the barrier dropped");
    }
}

#[test]
fn a_respawn_clears_the_barrier() {
    // the Iron Giant's capsule is the widest; Scooby's Mansion's respawn spots are among the nearest the middle
    let Some(mut h) = on("scoobydoo", &["irongiant", "irongiant"], Some(Dodgeball::PLAY), false, 3) else { return };
    idle(&mut h, 60);
    let f0 = h.fighter(0);
    let left = h.w.stage.blast_outer.left;
    h.w.fighter_mut(f0).body.x = left - 500.0;
    let mut died = false;
    for _ in 0..900 {
        h.step(&[NO_INPUT, NO_INPUT]);
        let dead = h.w.fighter(f0).life.dead != 0.0;
        died |= dead;
        if died && !dead {
            break;
        }
    }
    assert!(died && h.w.fighter(f0).life.dead == 0.0, "rung out and respawned");
    assert!(on_half(&h.w, f0), "respawned clear of the barrier: x {}", h.w.fighter(f0).body.x);
    idle(&mut h, 1);
    assert!(on_half(&h.w, f0), "and a tick on: x {}", h.w.fighter(f0).body.x);
}

#[test]
fn the_barrier_stands_at_the_middle_of_every_stage() {
    let mut bad = Vec::new();
    for stage in STAGES {
        let chars = ["irongiant", "superman", "irongiant", "shaggy"];
        let Some(mut h) = on(stage, &chars, Some(Dodgeball::PLAY), false, 1) else { return };
        let (blast, outer) = (h.w.stage.blast, h.w.stage.blast_outer);
        match barrier(&h.w) {
            Some([l, r, b, t]) => {
                if l != -HALF_WIDTH || r != HALF_WIDTH {
                    bad.push(format!("{stage}: the barrier from {l} to {r}"));
                }
                if b >= blast.bottom.min(outer.bottom) || t <= blast.top.max(outer.top) {
                    bad.push(format!("{stage}: the barrier from {b} to {t} inside the blast lines"));
                }
            }
            None => bad.push(format!("{stage}: no barrier")),
        }
        let s = h.w.runtime_solids.as_ref().and_then(|l| l.iter().find(|s| s.spec == BARRIER_SPEC));
        if s.is_none_or(|s| s.flags & SOLID_ITEM != 0 || s.flags & SOLID_NAV == 0 || s.flags & SOLID_TRACE == 0) {
            bad.push(format!("{stage}: the barrier's bits {:?}", s.map(|s| s.flags)));
        }
        for (i, &f) in h.w.players.clone().iter().enumerate() {
            let rx = h.w.stage.respawns[i].x;
            let clear = face(&h.w, f);
            let ok = if h.w.fighter(f).team.team == 0 { rx <= -clear } else { rx >= clear };
            if !ok {
                bad.push(format!("{stage}: player {i}'s respawn spot at {rx}, its capsule's face {clear}"));
            }
        }
        idle(&mut h, 120);
        for (i, &f) in h.w.players.clone().iter().enumerate() {
            if !on_half(&h.w, f) {
                bad.push(format!("{stage}: player {i} at {} after its landing", h.w.fighter(f).body.x));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn with_no_barrier_no_solid_is_made_and_nobody_is_put_back() {
    let Some(mut h) = duel(["superman", "shaggy"], Dodgeball { barrier: false }) else { return };
    let plain = on("m011", &["superman", "shaggy"], None, false, 5).expect("the same store");
    assert!(h.w.runtime_solids.is_none() && barrier(&h.w).is_none(), "no barrier");
    assert_eq!(h.w.stage.respawns, plain.w.stage.respawns, "the respawn spots are the stage's");
    let f0 = h.fighter(0);
    h.w.fighter_mut(f0).body.x = 600.0;
    idle(&mut h, 5);
    assert!(h.w.fighter(f0).body.x > 500.0, "left where it was placed: {}", h.w.fighter(f0).body.x);
}

/// The items made since `seen`, as (class, x, y) at their first tick, and the new `seen`.
fn new_items(w: &World, seen: &mut Vec<ItemId>) -> Vec<(String, Real, Real)> {
    let mut out = Vec::new();
    for (id, i) in w.actors.items.iter() {
        if !seen.contains(&id) {
            seen.push(id);
            out.push((w.names.str(i.cls).to_owned(), i.x, i.y));
        }
    }
    out
}

/// The drops of a match of `seed` to tick `ticks`, by tick, with nobody touching them.
fn drops(stage: &str, seed: i32, ticks: usize) -> Option<Vec<(i32, String, Real, Real)>> {
    let mut h = on(stage, &["superman", "shaggy"], Some(Dodgeball::PLAY), false, seed)?;
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for _ in 0..ticks {
        idle(&mut h, 1);
        let t = h.w.flow.tick;
        out.extend(new_items(&h.w, &mut seen).into_iter().map(|(c, x, y)| (t, c, x, y)));
        // (only the rule drops: the build's spawner never starts under the rule alone)
        let ar = h.w.item_area.as_ref().expect("an item area under the rule");
        assert!(ar.timer_at < 0 && ar.new_match_at < 0 && ar.pending_at < 0, "the area's spawner runs");
    }
    Some(out)
}

fn ticks(s: Real) -> i32 {
    (s * 60.0).round() as i32
}

#[test]
fn the_drops_come_on_each_half_at_the_rules_times_and_draws() {
    let Some(a) = drops("m011", 5, 1500) else { return };
    let first = 1 + ticks(DROP_FIRST);
    let mut bad = Vec::new();
    let mut halves = [0, 0];
    for (t, c, x, _) in &a {
        if *t < first || (t - first) % ticks(DROP_EVERY) != 0 {
            bad.push(format!("a drop at tick {t}"));
        }
        if x.abs() < DROP_CLEAR {
            bad.push(format!("a drop at x {x}, inside {DROP_CLEAR} of the middle"));
        }
        if !POOL.contains(&c.as_str()) {
            bad.push(format!("a drop of {c}"));
        }
        halves[usize::from(*x > 0.0)] += 1;
    }
    let at_first: Vec<_> = a.iter().filter(|d| d.0 == first).collect();
    assert_eq!(at_first.len(), 2, "one on each half at the first drop: {at_first:?}");
    assert!(halves[0] >= 3 && halves[1] >= 3, "drops on both halves: {halves:?}");
    assert!(bad.is_empty(), "{bad:#?}");
    assert_eq!(drops("m011", 5, 1500), Some(a.clone()), "the same draws from the same seed");
    assert_ne!(drops("m011", 6, 1500), Some(a), "other draws from another seed");
    // every stage's area gives a point on each half
    for stage in STAGES {
        let d = drops(stage, 2, usize::try_from(first).expect("a tick")).expect("the same store");
        assert!(d.iter().any(|x| x.2 < 0.0) && d.iter().any(|x| x.2 > 0.0), "{stage}: the first drops {d:?}");
    }
}

#[test]
fn no_drop_on_a_half_that_holds_the_cap() {
    let Some(mut h) = on("m011", &["superman", "shaggy"], Some(Dodgeball::PLAY), false, 5) else { return };
    idle(&mut h, 60);
    for k in 0..DROP_CAP {
        spawn(&mut h.w, "Item_Rock_C", -1200.0 + 200.0 * k as Real, 600.0);
    }
    let left = lying_on(&h.w, 0);
    assert_eq!(left, DROP_CAP, "the cap's items on the left");
    let right = lying_on(&h.w, 1);
    let to = usize::try_from(1 + ticks(DROP_FIRST)).expect("a tick") - 60;
    idle(&mut h, to);
    assert_eq!(lying_on(&h.w, 0), DROP_CAP, "no drop on the full half");
    assert_eq!(lying_on(&h.w, 1), right + 1, "one on the other");
}

#[test]
fn the_clock_starts_at_the_match_start_and_stops_at_its_end() {
    let Some(mut h) = on("m011", &["superman", "shaggy"], Some(Dodgeball::PLAY), true, 5) else { return };
    while !h.w.flow.match_started {
        idle(&mut h, 1);
        let started = h.w.flow.match_started;
        assert!(started || h.w.dodgeball.as_ref().is_some_and(|s| s.from < 0), "no clock before the match starts");
    }
    let st = h.w.dodgeball.as_ref().expect("the rule's state");
    assert_eq!(st.from, h.w.flow.match_start_tick, "from the match start");
    assert_eq!(st.drop_at, st.from + ticks(DROP_FIRST), "the first drop");
    h.w.flow.match_over = true;
    idle(&mut h, 1);
    assert!(h.w.dodgeball.as_ref().is_some_and(|s| s.over), "stopped at the match's end");
}

#[test]
fn a_dodgeball_frame_reads_back() {
    let Some(mut h) = duel(["superman", "shaggy"], Dodgeball::PLAY) else { return };
    idle(&mut h, 600);
    let frame = h.record();
    let mut g = on("m011", &["superman", "shaggy"], Some(Dodgeball::PLAY), false, 5).expect("the same store");
    let loaded = mvs_sim_trace::load_frame(&mut g.w, &g.book, &frame);
    assert!(loaded.misses.is_empty(), "{:#?}", loaded.misses);
    let back = mvs_sim_trace::frame_of(&g.w, &g.book, &loaded.inputs, &[]).frame;
    let d = mvs_trace::diff_frames(&frame, &back);
    assert!(d.is_empty(), "{:#?}", d.iter().take(10).collect::<Vec<_>>());
    assert_eq!(dodgeball::barrier(&g.w), dodgeball::barrier(&h.w), "the barrier");
}
