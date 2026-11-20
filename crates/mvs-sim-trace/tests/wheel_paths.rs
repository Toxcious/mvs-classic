//! The port's own Chaos Wheel rule (`mvs_sim::world::wheel`) where the trace match `wheel-combat` cannot show it:
//! each pool mutator on then off leaves a fighter as it found it, a Giant grown against a wall and under a ceiling,
//! the clock's start with and without a flow, and a stage with no item area.

use mvs_sim::fighter::FighterId;
use mvs_sim::setup::{MatchPlayer, MatchSetup, Rules, TeamStyle, Wheel};
use mvs_sim::world::stage::{RuntimeSolidSpec, add_runtime_solid};
use mvs_sim::world::wheel::{change_to, status};
use mvs_sim::{Event, InputState, Real, World};
use mvs_sim_trace::harness::{self, Harness};

/// `mvs_protocol::lobby_rules::WHEEL_MUTATORS`, the pool the app passes.
const POOL: [&str; 13] = [
    "Mutator_Giant",
    "Mutator_Miniature",
    "Mutator_BigHeadAbilityBoost",
    "Mutator_LowGravity",
    "Mutator_Boost_UnlimitedJumps",
    "Mutator_Boost_CooldownReductionLarge",
    "Mutator_Boost_AttacksBreakArmor",
    "Mutator_Boost_WeakenOnHit",
    "Mutator_Boost_ChargeFreeze",
    "Mutator_Boost_IceGauntlets",
    "Mutator_Boost_AdditionalAirSpecial",
    "Mutator_GreyHealth",
    "Mutator_RandomItemSpawns",
];

fn player(ch: &str) -> MatchPlayer {
    MatchPlayer { bot: false, character: Some(ch.to_owned()), ..MatchPlayer::default() }
}

fn setup(pool: &[&str], seconds: Real, flow: bool) -> MatchSetup {
    MatchSetup {
        team_style: TeamStyle::OneVsOne,
        players: vec![player("superman"), player("shaggy")],
        rules: Some(Rules { knockouts_to_win: 99.0, duration: 420.0, stocks: false }),
        flow,
        wheel: Some(Wheel { pool: pool.iter().map(|&s| s.to_owned()).collect(), seconds, warn: 1.0 }),
        ..MatchSetup::default()
    }
}

/// A World of the whole pool whose clock never changes by itself, its fighters landed.
fn landed() -> Option<Harness> {
    let mut h = harness::fresh(&setup(&POOL, 10_000.0, false), 11)?;
    for _ in 0..120 {
        h.step_recorded();
    }
    Some(h)
}

fn idle(h: &mut Harness, n: usize) {
    for _ in 0..n {
        h.step_recorded();
    }
}

/// What a mutator may change on a fighter, as text: size and capsule, head size, gravity, the move folds (jump
/// count and heights, falling acceleration, air control), cooldown and ability mods, air special max, its live buff
/// list (the armor-break flag, the on-hit hooks and the gauntlet are buffs).
fn snapshot(w: &World, f: FighterId) -> String {
    let fr = w.fighter(f);
    let buffs: Vec<&str> = fr.buffs.buffs.iter().filter(|b| b.frames > 0.0).map(|b| w.names.str(b.cls)).collect();
    format!(
        "size {:?} scaled {:?} head {:?} gravity {:?}/{:?} folds {:?} speed {:?} jumps {:?} cooldowns {:?} \
         air special {:?} buffs {buffs:?}",
        fr.body.size_scale,
        fr.scaled,
        fr.pose.head_scale.unwrap_or(1.0),
        fr.moves.gravity_override,
        fr.moves.gravity_scale,
        fr.moves.folds,
        fr.buffs.speed_mod,
        mvs_sim::fighter::moves::max_jumps(w, f),
        fr.abilities,
        fr.air.air_special_max_set,
    )
}

#[test]
fn each_pool_mutator_on_then_off_leaves_a_fighter_as_it_found_it() {
    let mut bad = Vec::new();
    for (i, key) in POOL.iter().enumerate() {
        let Some(mut h) = landed() else { return };
        let fs = [h.fighter(0), h.fighter(1)];
        let before: Vec<String> = fs.iter().map(|&f| snapshot(&h.w, f)).collect();
        change_to(&mut h.w, Some(i));
        idle(&mut h, 20);
        let on: Vec<String> = fs.iter().map(|&f| snapshot(&h.w, f)).collect();
        if on == before && *key != "Mutator_RandomItemSpawns" {
            bad.push(format!("{key}: nothing changed while on"));
        }
        change_to(&mut h.w, None);
        idle(&mut h, 20);
        for (n, &f) in fs.iter().enumerate() {
            let after = snapshot(&h.w, f);
            if after != before[n] {
                bad.push(format!("{key} fighter {n}:\n  before {}\n  after  {after}", before[n]));
            }
        }
        if *key == "Mutator_RandomItemSpawns" {
            let ar = h.w.item_area.as_ref().expect("the wheel's items give the match an item area");
            assert!(ar.timer_at < 0 && ar.pending_at < 0, "the spawner stops with its window");
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn extra_air_special_comes_back_after_a_respawn_inside_its_window() {
    let Some(mut h) = landed() else { return };
    let f = h.fighter(0);
    let air = POOL.iter().position(|&k| k == "Mutator_Boost_AdditionalAirSpecial").expect("in the pool");
    let before = h.w.fighter(f).air.air_special_max_set;
    change_to(&mut h.w, Some(air));
    assert_eq!(mvs_sim::fighter::control::air_special_max(&h.w, f), 3.0);
    // a ring-out and the respawn inside the window: the build's each-life +1 on a reset pawn
    mvs_sim::combat::buffs::defeat_buffs(&mut h.w, f);
    mvs_sim::world::flow::respawn(&mut h.w, f);
    assert_eq!(mvs_sim::fighter::control::air_special_max(&h.w, f), 3.0);
    change_to(&mut h.w, None);
    assert_eq!(h.w.fighter(f).air.air_special_max_set, before);
}

#[test]
fn the_ice_gauntlets_window_ends_its_own_gauntlet_only() {
    let Some(mut h) = landed() else { return };
    let (sup, sh) = (h.fighter(0), h.fighter(1));
    let ice = POOL.iter().position(|&k| k == "Mutator_Boost_IceGauntlets").expect("in the pool");
    let gauntlet = h.w.lit("Buff_SupermanAllyIceGauntlet_C");
    change_to(&mut h.w, Some(ice));
    let live = |w: &World, f: FighterId| {
        w.fighter(f).buffs.buffs.iter().filter(|b| b.cls == gauntlet && b.frames > 0.0).count()
    };
    assert_eq!(live(&h.w, sup), 1);
    assert_eq!(live(&h.w, sh), 1);
    change_to(&mut h.w, None);
    assert_eq!(live(&h.w, sup), 0);
    assert_eq!(live(&h.w, sh), 0);
}

/// A Floor box of the play plane with its sim left, right, bottom and top (sim x = -UE Y, sim y = UE Z).
fn wall(w: &mut World, spec: i32, l: Real, r: Real, b: Real, t: Real) {
    let d = RuntimeSolidSpec {
        cls: "TutorialWall_C".to_owned(),
        floor: true,
        center: [0.0, -(l + r) / 2.0, (b + t) / 2.0],
        half_extent: [500.0, (r - l) / 2.0, (t - b) / 2.0],
        rot: None,
        component: mvs_sim::names::Name::NONE,
    };
    assert!(add_runtime_solid(w, spec, &d), "the box is made");
}

/// How far the fighter's capsule box reaches into the box (l, r, b, t), 0 when it does not.
fn depth(w: &World, f: FighterId, bx: [Real; 4]) -> Real {
    let fr = w.fighter(f);
    let (x, y, rad, half) = (fr.body.x, fr.body.y, fr.scaled.cap_radius, fr.scaled.cap_half);
    let dx = (x + rad).min(bx[1]) - (x - rad).max(bx[0]);
    let dy = (y + half).min(bx[3]) - (y - half).max(bx[2]);
    if dx > 0.0 && dy > 0.0 { dx.min(dy) } else { 0.0 }
}

fn press(h: &mut Harness, x: Real, n: usize) {
    let mut i = vec![InputState::default(); h.w.players.len()];
    i[0].x = x;
    for _ in 0..n {
        h.step(&i);
    }
}

/// Giant on a fighter standing against a wall, then under a low ceiling: grown in place, the capsule overlaps the
/// box for as long as the fighter stands still (nothing pushes it out at the size change); the first ticks of
/// movement free it (into the wall: pushed out; under the ceiling: it walks out from under it). The test asks only
/// that a few ticks of movement leave it outside and free to move.
#[test]
fn a_giant_against_a_wall_and_under_a_ceiling_is_not_left_inside() {
    let Some(mut h) = landed() else { return };
    let f = h.fighter(0);
    assert!(h.w.fighter(f).body.on_floor, "landed");
    let (x, y) = (h.w.fighter(f).body.x, h.w.fighter(f).body.y);
    let (rad, half) = (h.w.fighter(f).scaled.cap_radius, h.w.fighter(f).scaled.cap_half);
    // a wall to the right, its face 2 units from the capsule, from the floor up
    let wbox = [x + rad + 2.0, x + rad + 400.0, y - half + 1.0, y + half + 800.0];
    wall(&mut h.w, 9001, wbox[0], wbox[1], wbox[2], wbox[3]);
    idle(&mut h, 5);
    assert_eq!(depth(&h.w, f, wbox), 0.0, "not inside before");
    change_to(&mut h.w, Some(0));
    let grown = h.w.fighter(f).body.size_scale;
    let mut log = vec![format!("wall: grown {grown}, depth at once {}", depth(&h.w, f, wbox))];
    idle(&mut h, 10);
    log.push(format!("wall: depth after 10 ticks {} (x {} -> {})", depth(&h.w, f, wbox), x, h.w.fighter(f).body.x));
    press(&mut h, 1.0, 30);
    log.push(format!("wall: depth after 30 ticks of right {}", depth(&h.w, f, wbox)));
    press(&mut h, -1.0, 30);
    let moved_left = h.w.fighter(f).body.x < x - 50.0;
    log.push(format!("wall: depth after 30 ticks of left {} (moved left: {moved_left})", depth(&h.w, f, wbox)));
    let wall_ok = depth(&h.w, f, wbox) == 0.0 && moved_left;
    change_to(&mut h.w, None);
    idle(&mut h, 10);

    // a ceiling over a fresh fighter: its bottom 2 units over the head
    let Some(mut h) = landed() else { return };
    let f = h.fighter(0);
    let (x, y) = (h.w.fighter(f).body.x, h.w.fighter(f).body.y);
    let cbox = [x - 600.0, x + 600.0, y + half + 2.0, y + half + 300.0];
    wall(&mut h.w, 9002, cbox[0], cbox[1], cbox[2], cbox[3]);
    idle(&mut h, 5);
    assert_eq!(depth(&h.w, f, cbox), 0.0, "not inside before");
    change_to(&mut h.w, Some(0));
    log.push(format!("ceiling: depth at once {}", depth(&h.w, f, cbox)));
    idle(&mut h, 10);
    log.push(format!("ceiling: depth after 10 ticks {} (y {} -> {})", depth(&h.w, f, cbox), y, h.w.fighter(f).body.y));
    press(&mut h, -1.0, 40);
    let moved = (h.w.fighter(f).body.x - x).abs() > 50.0;
    let fr = h.w.fighter(f);
    log.push(format!(
        "ceiling: depth after 40 ticks of left {} (moved: {moved}, at {} {}, on floor {}, box {cbox:?})",
        depth(&h.w, f, cbox),
        fr.body.x,
        fr.body.y,
        fr.body.on_floor
    ));
    let ceiling_ok = depth(&h.w, f, cbox) == 0.0 && moved;
    println!("{}", log.join("\n"));
    assert!(wall_ok && ceiling_ok, "a Giant left inside geometry or stuck:\n{}", log.join("\n"));
}

/// The draws over many windows of `w`: the pool indexes that came on, in order.
fn changes(h: &mut Harness, ticks: usize) -> Vec<i32> {
    let mut out = Vec::new();
    for _ in 0..ticks {
        h.w.events.clear();
        h.step_recorded();
        out.extend(h.w.events.iter().filter_map(|e| match e {
            Event::WheelChange { index } => Some(*index),
            _ => None,
        }));
    }
    out
}

/// The steps until the first change, and the clock's start tick then.
fn first_change(h: &mut Harness) -> (usize, i32) {
    for n in 1..2000 {
        if !changes(h, 1).is_empty() {
            return (n, h.w.wheel.as_ref().expect("a wheel").from);
        }
    }
    panic!("no change");
}

#[test]
fn the_clock_counts_from_the_match_start_or_the_first_step() {
    // no flow (the Lab, TrainGameMode): from the first step, the plain window a whole window long
    let Some(mut h) = harness::fresh(&setup(&POOL, 1.0, false), 3) else { return };
    let tick0 = h.w.flow.tick;
    assert_eq!(status(&h.w).and_then(|s| s.ticks_left), None, "not started before the first step");
    let (n, from) = first_change(&mut h);
    assert_eq!((n, from), (61, tick0 + 1), "the plain window from the first step");
    // with its flow: nothing before BPI_SetMatchStarted, then a plain window from it
    let Some(mut h) = harness::fresh(&setup(&POOL, 1.0, true), 3) else { return };
    let tick0 = h.w.flow.tick;
    let (n, from) = first_change(&mut h);
    assert_eq!(from, h.w.flow.match_start_tick);
    assert_eq!(i32::try_from(n).expect("short") + tick0, from + 60, "the plain window from the match start");
    assert_eq!(status(&h.w).and_then(|s| s.ticks_left), Some(60));
}

#[test]
fn a_stage_with_no_item_area_never_rolls_random_items() {
    let Some(mut h) = harness::fresh(&setup(&POOL, 0.5, false), 5) else { return };
    std::sync::Arc::make_mut(&mut h.w.data).spawn_areas.clear();
    let c = changes(&mut h, 30 * 120);
    let items = POOL.iter().position(|&k| k == "Mutator_RandomItemSpawns").expect("in the pool") as i32;
    assert!(c.len() > 100, "{} changes", c.len());
    assert!(!c.contains(&items), "Random Items rolled on a stage with no item area");
    assert!(c.windows(2).all(|p| p[0] != p[1]), "the one on is never drawn again");
}

#[test]
fn the_draw_never_gives_a_mutator_the_set_up_lists() {
    let mut s = setup(&POOL, 0.5, false);
    s.mutators = vec!["Mutator_Giant".to_owned(), "Mutator_RandomItemSpawns".to_owned()];
    let Some(mut h) = harness::fresh(&s, 5) else { return };
    let c = changes(&mut h, 30 * 120);
    assert!(!c.contains(&0) && !c.contains(&12), "{c:?}");
}
