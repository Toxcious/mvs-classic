//! The port's own Dodgeball rule (`mvs_sim::world::dodgeball`) spares an opponent everything a fighter deals: every
//! roster fighter's every move, on the ground and in the air, with the opponent near, at mid range and far in front,
//! leaves the opponent as he would be had the attacker stood still, and the attacker is back to an actionable state
//! within a bound. A control run of the same set with the rule off must change the opponent for most moves. Then the
//! paths a move of the set cannot show: a thrown item the stage dropped still hits (and gives its buffs), one a
//! fighter's move made does not, and a team mate is treated as today. On fresh Worlds with the barrier off; without a
//! store they check nothing.

use mvs_sim::fighter::FighterId;
use mvs_sim::input::NO_INPUT;
use mvs_sim::setup::{Dodgeball, MatchPlayer, MatchSetup};
use mvs_sim::{InputState, Real, World};
use mvs_sim_trace::harness::{self, Harness};

const ROSTER: [&str; 23] = [
    "arya",
    "batman",
    "blackadam",
    "bugs",
    "creature",
    "finn",
    "garnet",
    "gizmo",
    "harley",
    "irongiant",
    "jake",
    "lebron",
    "marvin",
    "morty",
    "rick",
    "shaggy",
    "steven",
    "stripe",
    "superman",
    "taz",
    "tomandjerry",
    "velma",
    "wonderwoman",
];

/// The opponent's distance in front of the attacker: near, mid range, far.
const DISTANCES: [Real; 3] = [150.0, 450.0, 1000.0];
/// Ticks watched from the press.
const WATCH: usize = 300;
/// Ticks the button is held (a short charge where the move charges).
const HOLD: usize = 4;

fn player(c: &str, team: u8) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), team: Some(team), bot: false, ..MatchPlayer::default() }
}

fn setup(players: &[(&str, u8)], rule: bool) -> MatchSetup {
    MatchSetup {
        players: players.iter().map(|&(c, t)| player(c, t)).collect(),
        dodgeball: rule.then_some(Dodgeball { barrier: false }),
        ..MatchSetup::default()
    }
}

/// A fresh World of these players, landed.
fn fresh(players: &[(&str, u8)], rule: bool) -> Option<Harness> {
    let mut h = harness::fresh(&setup(players, rule), 1)?;
    let none = vec![NO_INPUT; players.len()];
    for _ in 0..90 {
        h.step(&none);
    }
    Some(h)
}

/// The eight presses: attack and special, neutral, side (toward the opponent), up and down.
fn presses(toward: Real) -> [(&'static str, InputState); 8] {
    let i = || NO_INPUT;
    [
        ("attack", InputState { attack: true, ..i() }),
        ("side attack", InputState { attack: true, x: toward, ..i() }),
        ("up attack", InputState { attack: true, y: 1.0, ..i() }),
        ("down attack", InputState { attack: true, y: -1.0, ..i() }),
        ("special", InputState { special: true, ..i() }),
        ("side special", InputState { special: true, x: toward, ..i() }),
        ("up special", InputState { special: true, y: 1.0, ..i() }),
        ("down special", InputState { special: true, y: -1.0, ..i() }),
    ]
}

/// The attacker is free: no attack running, no attack montage, no hold on him.
fn actionable(w: &World, f: FighterId) -> bool {
    !mvs_sim::fighter::is_attacking(w, f) && !mvs_sim::fighter::has_attack_inst(w, f)
}

/// The World at the press: the opponent `dist` in front of the attacker on the ground, the attacker on the ground or
/// a few ticks into a jump.
fn start(base: &World, a: FighterId, o: FighterId, dist: Real, air: bool) -> World {
    let mut w = base.clone();
    let side = w.fighter(a).body.facing_dir;
    let (ax, ay) = (w.fighter(a).body.x, w.fighter(a).body.y);
    let b = &mut w.fighter_mut(o).body;
    (b.x, b.y, b.prev_x, b.prev_y) = (ax + side * dist, ay, ax + side * dist, ay);
    for _ in 0..20 {
        w.step(&[NO_INPUT, NO_INPUT]);
    }
    if air {
        w.step(&[InputState { jump: true, ..NO_INPUT }, NO_INPUT]);
        for _ in 0..8 {
            w.step(&[NO_INPUT, NO_INPUT]);
        }
    }
    w
}

/// The opponent's record at each watched tick, the attacker pressing `press` (None: standing still), and the tick the
/// attacker was free again (None: not within the watch).
fn run(h: &mut Harness, w0: &World, press: Option<InputState>, a: FighterId) -> (Vec<mvs_trace::Frame>, Option<usize>) {
    h.w = w0.clone();
    let mut out = Vec::with_capacity(WATCH);
    let mut free = None;
    for t in 0..WATCH {
        let i = match press {
            Some(p) if t < HOLD => p,
            _ => NO_INPUT,
        };
        h.step(&[i, NO_INPUT]);
        out.push(h.record());
        if free.is_none() && t > HOLD && actionable(&h.w, a) {
            free = Some(t);
        }
    }
    (out, free)
}

/// Fields of a fighter's record that are not his own state: `rest.comboReset` is the World's one shared combo reset
/// (any fighter's stun change writes it), `plat_ignored` the stage platforms his capsule ignores, recomputed while a
/// platform a move spawned stands (geometry, not something dealt to him).
const NOT_HIS: [&str; 2] = ["comboReset", "plat_ignored"];

/// The fields of fighter `n`'s record that differ between the two runs, at the first tick one does.
fn first_change(a: &[mvs_trace::Frame], b: &[mvs_trace::Frame], n: usize) -> Option<String> {
    let key = format!("fighters[{n}]");
    for (t, (x, y)) in a.iter().zip(b).enumerate() {
        let d = mvs_trace::diff_frames(x, y);
        let his: Vec<String> = d
            .iter()
            .filter(|d| d.path.contains(&key) && !NOT_HIS.iter().any(|k| d.path.contains(k)))
            .take(6)
            .map(|d| format!("{}: {} -> {}", d.path, d.expected, d.actual))
            .collect();
        if !his.is_empty() {
            return Some(format!("tick {t} {}", his.join("; ")));
        }
    }
    None
}

/// Every move of `who` against a still opponent; per move, whether the opponent changed and the attacker's free tick.
struct Outcome {
    changed: Vec<String>,
    /// Changes that begin with the opponent standing on a spawned platform (Morty's ship): geometry, not a hit.
    carried: Vec<String>,
    stuck: Vec<String>,
    unmoved: Vec<String>,
    moves: usize,
    moved: usize,
}

fn sweep(who: &str, rule: bool) -> Option<Outcome> {
    let dummy = if who == "shaggy" { "superman" } else { "shaggy" };
    let mut h = fresh(&[(who, 0), (dummy, 1)], rule)?;
    let (a, o) = (h.fighter(0), h.fighter(1));
    let base = h.w.clone();
    let toward = base.fighter(a).body.facing_dir;
    let mut res = Outcome {
        changed: Vec::new(),
        carried: Vec::new(),
        stuck: Vec::new(),
        unmoved: Vec::new(),
        moves: 0,
        moved: 0,
    };
    let mut moved: Vec<bool> = vec![false; 16];
    for (k, air) in [false, true].into_iter().enumerate() {
        for dist in DISTANCES {
            let w0 = start(&base, a, o, dist, air);
            let (still, _) = run(&mut h, &w0, None, a);
            for (m, (name, p)) in presses(toward).into_iter().enumerate() {
                let (got, free) = run(&mut h, &w0, Some(p), a);
                let at = format!("{who} {}{name} at {dist}", if air { "air " } else { "" });
                if let Some(c) = first_change(&still, &got, 1) {
                    moved[k * 8 + m] = true;
                    if c.contains("rest.base: null -> {id: 0 (0x0000000000000000), ref: \"ships\"}") {
                        res.carried.push(format!("{at}: {c}"));
                    } else {
                        res.changed.push(format!("{at}: {c}"));
                    }
                }
                if free.is_none() {
                    res.stuck.push(at);
                }
            }
        }
    }
    for (i, m) in moved.iter().enumerate() {
        if !m {
            let (name, _) = presses(toward)[i % 8];
            res.unmoved.push(format!("{}{name}", if i >= 8 { "air " } else { "" }));
        }
    }
    res.moves = moved.len();
    res.moved = moved.iter().filter(|&&m| m).count();
    Some(res)
}

fn only() -> Vec<&'static str> {
    match std::env::var("MVS_DODGEBALL_ONLY") {
        Ok(k) => ROSTER.iter().copied().filter(|r| k.split(',').any(|x| x == *r)).collect(),
        Err(_) => ROSTER.to_vec(),
    }
}

/// An item of `cls` at the feet of fighter 0, the opponent (fighter 1) 450 in front: one the stage dropped (`maker`
/// None, as the item area spawns it) or one of fighter `maker`'s (as his move spawns it). Fighter 0 picks it up and
/// throws it at the opponent; the harness `after` ticks past the throw, and whether he held it.
fn thrown(
    who: &str,
    dummy: &str,
    cls: &str,
    maker: Option<usize>,
    rule: bool,
    after: usize,
) -> Option<(Harness, bool)> {
    let mut h = fresh(&[(who, 0), (dummy, 1)], rule)?;
    let (a, o) = (h.fighter(0), h.fighter(1));
    let side = h.w.fighter(a).body.facing_dir;
    let (ax, ay) = (h.w.fighter(a).body.x, h.w.fighter(a).body.y);
    let b = &mut h.w.fighter_mut(o).body;
    (b.x, b.prev_x) = (ax + side * 450.0, ax + side * 450.0);
    let name = h.w.names.get(cls).unwrap_or_else(|| panic!("{cls}'s name"));
    let by_stage = maker.is_none();
    let it = if let Some(m) = maker {
        let m = h.fighter(m);
        mvs_sim::world::items::spawn_item(&mut h.w, name, m, ax, ay + 20.0, 0.0, 0.0, false, None)
    } else {
        let src = mvs_sim::world::items::resolve_def(&h.w, None, cls).unwrap_or_else(|| panic!("{cls}'s data"));
        mvs_sim::world::items::spawn_world_item(&mut h.w, name, src, ax, ay + 20.0)
    };
    let it = it.unwrap_or_else(|| panic!("{cls} spawned"));
    assert_eq!(h.w.actors.items.get(it).map(|i| i.spawner.is_none()), Some(by_stage), "{cls}: the spawner");
    for _ in 0..20 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    let mut held = false;
    for t in 0..60 {
        let i = if t < 2 { InputState { attack: true, ..NO_INPUT } } else { NO_INPUT };
        h.step(&[i, NO_INPUT]);
        held |= h.w.fighter(a).items.held_item == Some(it);
    }
    for t in 0..after {
        let i = if t < 2 { InputState { attack: true, x: side, ..NO_INPUT } } else { NO_INPUT };
        h.step(&[i, NO_INPUT]);
    }
    Some((h, held))
}

/// The opponent's damage, buffs and most recent attacker.
fn hit_on(h: &Harness) -> (Real, Vec<String>, Option<FighterId>) {
    let w = &h.w;
    let o = w.fighter(h.fighter(1));
    let bs = o.buffs.buffs.iter().map(|b| w.names.str(b.cls).to_owned()).collect();
    (o.stats.damage, bs, o.attackers.most_recent_attacker)
}

/// A thrown item of each class of the rule's pool, dropped by the stage, hits the opponent under the rule: its damage,
/// the class's buff and the thrower as the most recent attacker (the ring-out's credit).
#[test]
fn a_thrown_stage_item_still_hits() {
    let pool = [
        ("Item_Rock_C", &[][..]),
        ("Item_Pie_C", &["Buff_SlowStackable_C"][..]),
        ("Item_DrumStick_C", &["Buff_Chicken_C"][..]),
        ("Item_Snowball_C", &["Buff_SnowballHead_C"][..]),
        ("Item_SuperSnowball_C", &["Buff_SnowballHead_C", "Buff_Ice_C"][..]),
    ];
    for (cls, buffs) in pool {
        let Some((h, held)) = thrown("superman", "shaggy", cls, None, true, 120) else { return };
        assert!(held, "{cls}: picked up");
        let (damage, got, by) = hit_on(&h);
        assert!(damage > 0.0, "{cls}: the throw hit");
        assert_eq!(by, Some(h.fighter(0)), "{cls}: the thrower is the most recent attacker");
        for b in buffs {
            assert!(got.iter().any(|g| g == b), "{cls}: {b} on the opponent ({got:?})");
        }
    }
}

/// An item a fighter's move made (Shaggy's sandwich), picked up and thrown by his opponent at him, does not hit under
/// the rule; it does without.
#[test]
fn a_fighters_item_spares_the_opponent() {
    let Some((h, held)) = thrown("superman", "shaggy", "Item_Sandwich_C", Some(1), true, 120) else { return };
    assert!(held, "the sandwich picked up");
    assert_eq!(hit_on(&h), (0.0, Vec::new(), None), "the sandwich spares the opponent");
    let Some((h, _)) = thrown("superman", "shaggy", "Item_Sandwich_C", Some(1), false, 120) else { return };
    assert!(hit_on(&h).0 > 0.0, "without the rule the sandwich hits");
}

/// The paths a plain hit does not take, each a move that changes the opponent without the rule: Reindog's tether
/// (the cylinder's direct damage), Reindog's fire (Buff_Fire's ticks), Black Adam's chain lightning, Velma's carry (a
/// grab), Taz's Tasty and Weakened buffs. Under the rule each leaves the opponent as a still attacker does.
#[test]
fn the_special_paths_spare_the_opponent() {
    let cases = [
        ("creature", "special"),
        ("creature", "down special"),
        ("blackadam", "special"),
        ("velma", "side special"),
        ("taz", "side attack"),
        ("taz", "special"),
    ];
    for (who, name) in cases {
        for rule in [false, true] {
            let Some(mut h) = fresh(&[(who, 0), ("shaggy", 1)], rule) else { return };
            let (a, o) = (h.fighter(0), h.fighter(1));
            let base = h.w.clone();
            let toward = base.fighter(a).body.facing_dir;
            let w0 = start(&base, a, o, 450.0, false);
            let p = presses(toward).into_iter().find(|(n, _)| *n == name).expect("a press").1;
            let (still, _) = run(&mut h, &w0, None, a);
            let (got, free) = run(&mut h, &w0, Some(p), a);
            let c = first_change(&still, &got, 1);
            if rule {
                assert_eq!(c, None, "{who} {name}: the opponent changed under the rule");
                assert!(free.is_some(), "{who} {name}: the attacker is free again");
            } else {
                assert!(c.is_some(), "{who} {name}: the control changes the opponent");
            }
        }
    }
}

/// A 2v2 of `who` and a Shaggy against two Shaggys: the ally 150 in front of `who`, the opponents 600 and 700 behind
/// him. The record of every watched tick after the press, and the buffs the ally gained.
fn team_run(who: &str, rule: bool, press: usize) -> Option<(Vec<mvs_trace::Frame>, Vec<String>)> {
    let mut h = fresh(&[(who, 0), ("shaggy", 0), ("shaggy", 1), ("shaggy", 1)], rule)?;
    let a = h.fighter(0);
    let side = h.w.fighter(a).body.facing_dir;
    let (ax, ay) = (h.w.fighter(a).body.x, h.w.fighter(a).body.y);
    for (n, d) in [(1, 150.0), (2, -600.0), (3, -700.0)] {
        let f = h.fighter(n);
        let b = &mut h.w.fighter_mut(f).body;
        (b.x, b.y, b.prev_x, b.prev_y) = (ax + side * d, ay, ax + side * d, ay);
    }
    let none = [NO_INPUT; 4];
    for _ in 0..20 {
        h.step(&none);
    }
    let ally = h.fighter(1);
    let had: Vec<String> = h.w.fighter(ally).buffs.buffs.iter().map(|b| h.w.names.str(b.cls).to_owned()).collect();
    let mut gained = Vec::new();
    let mut out = Vec::with_capacity(WATCH);
    for t in 0..WATCH {
        let mut i = none;
        if t < HOLD {
            i[0] = presses(side)[press].1;
        }
        h.step(&i);
        out.push(h.record());
        for b in &h.w.fighter(ally).buffs.buffs {
            let n = h.w.names.str(b.cls).to_owned();
            if !had.contains(&n) && !gained.contains(&n) {
                gained.push(n);
            }
        }
    }
    Some((out, gained))
}

/// A team mate is treated as today: in a 2v2, every move of five fighters whose moves buff or heal an ally leaves
/// the ally's record the same with the rule on and off, and each fighter's set gives the ally a buff.
#[test]
fn a_team_mate_is_treated_as_today() {
    for who in ["steven", "wonderwoman", "velma", "blackadam", "bugs"] {
        let mut gave = Vec::new();
        for m in 0..8 {
            let Some((off, g)) = team_run(who, false, m) else { return };
            let Some((on, _)) = team_run(who, true, m) else { return };
            let c = first_change(&off, &on, 1);
            assert_eq!(c, None, "{who} {}: the ally differs under the rule", presses(1.0)[m].0);
            gave.extend(g);
        }
        assert!(!gave.is_empty(), "{who}: a move buffs the ally");
    }
}

#[test]
fn every_move_spares_the_opponent() {
    let t0 = std::time::Instant::now();
    let mut bad = Vec::new();
    let mut stuck = Vec::new();
    let (mut moves, mut moved) = (0, 0);
    for who in only() {
        let Some(on) = sweep(who, true) else { return };
        bad.extend(on.changed);
        for c in &on.carried {
            eprintln!("carried by a spawned platform under the rule: {c}");
        }
        let Some(off) = sweep(who, false) else { return };
        // a move left unfree under the rule that is free without it
        let off_stuck: Vec<&String> = off.stuck.iter().collect();
        stuck.extend(on.stuck.into_iter().filter(|s| !off_stuck.contains(&s)));
        moves += off.moves;
        moved += off.moved;
        eprintln!("{who}: control {} of {} moves change the opponent; not: {:?}", off.moved, off.moves, off.unmoved);
    }
    eprintln!("control: {moved} of {moves} moves change the opponent; {:.1} s", t0.elapsed().as_secs_f64());
    assert!(bad.is_empty(), "the opponent changed under the rule:\n{}", bad.join("\n"));
    assert!(stuck.is_empty(), "attackers not free within {WATCH} ticks under the rule:\n{}", stuck.join("\n"));
    assert!(moved * 5 >= moves * 3, "the control changes the opponent for {moved} of {moves} moves only");
}
