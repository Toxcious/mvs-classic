//! Iron Giant's ally turret, a path no trace runs (no trace match has a teammate): a fresh 2v2 World (Iron Giant 0 and
//! his teammate Shaggy 2 against two Shaggys), the turret switched on through `toggle_ally_turret` as Pawn_C017's
//! weapon mode does; the teammate walks into him, climbs in with the item button and aims the reticle. Then either
//! the bomb (its lead-in, its flight to the reticle, its growth and damage over the enemy, its end) and a jump out, or
//! the tentacle shots (a press, the auto fire, a second press, a dodge out with the attack held). Each tick writes one
//! line of the turret, the pilot, the actors and the enemy's damage; the lines asserted are the oracle's (a probe of
//! the oracle's turret code on the same setup and inputs).

use std::collections::BTreeSet;

use mvs_sim::actors::{WorldActorClass, WorldActorState};
use mvs_sim::chars::iron_giant::state::giant;
use mvs_sim::chars::iron_giant::turret::{ally_turret_reset, pilot_stage, toggle_ally_turret};
use mvs_sim::setup::{MatchPlayer, MatchSetup, TeamStyle};
use mvs_sim::{InputState, Real};
use mvs_sim_trace::harness::{self, Harness};

/// The probe's number: rounded to 3 places (Math.round: halves up) and written as JavaScript's number formatting writes
/// it.
fn r(v: Real) -> String {
    let k = (v * 1000.0 + 0.5).floor() / 1000.0;
    if k == 0.0 { "0".into() } else { format!("{k}") }
}

/// One tick's line. `shots`: the fast shots seen so far (their ids).
fn line(h: &Harness, tag: &str, shots: &mut BTreeSet<i32>) -> String {
    let w = &h.w;
    let (g, e, a) = (h.fighter(0), h.fighter(1), h.fighter(2));
    let t = giant(w, g).ally_turret.as_ref().expect("the turret");
    let ret = w.actors.actors.iter().filter(|(_, x)| x.class == WorldActorClass::C017PilotReticle && !x.dead).count();
    let fast = w.names.get("C017_TenticalFastShot_C");
    shots.extend(w.actors.projectiles.iter().filter(|(_, p)| Some(p.cls) == fast).map(|(_, p)| p.seq));
    let live: Vec<String> = w
        .actors
        .projectiles
        .iter()
        .filter(|(_, p)| Some(p.cls) == fast && !p.dead)
        .map(|(_, p)| format!("{},{}", r(p.x), r(p.y)))
        .collect();
    let live = if live.is_empty() { "-".to_owned() } else { live.join(";") };
    let bomb = w.actors.actors.iter().find_map(|(_, x)| match &x.state {
        WorldActorState::TurretBomb(b) => Some(b),
        _ => None,
    });
    let b = bomb.map_or("-".to_owned(), |b| format!("{},{},{},{}", r(b.x), r(b.y), r(b.scale), i32::from(b.exploding)));
    let fa = w.fighter(a);
    [
        w.flow.tick.to_string(),
        tag.to_owned(),
        t.jump_on_ally.map_or(-1, |o| w.fighter(o).id).to_string(),
        i32::from(t.has_pilot).to_string(),
        i32::from(t.reticle).to_string(),
        r(t.aim_x),
        r(t.aim_y),
        pilot_stage(w, a).to_string(),
        r(fa.body.x),
        r(fa.body.y),
        t.kick.to_string(),
        t.bomb.to_string(),
        t.shot.to_string(),
        t.fire.to_string(),
        i32::from(t.held).to_string(),
        ret.to_string(),
        format!("{}:{live}", shots.len()),
        b,
        r(w.fighter(e).stats.damage),
    ]
    .join(" ")
}

/// The run's phases: ticks, tag, the teammate's input.
fn phases(bomb: bool) -> Vec<(usize, &'static str, InputState)> {
    let i = InputState::default;
    let mut v = vec![
        (1, "seed", i()),
        (12, "walk", InputState { x: 1.0, ..i() }),
        (2, "rest", i()),
        (1, "item", InputState { item: true, ..i() }),
        (8, "wait", i()),
        (33, "right", InputState { x: 1.0, ..i() }),
        (20, "down", InputState { y: -1.0, ..i() }),
        (4, "still", i()),
    ];
    if bomb {
        v.extend([
            (1, "bomb", InputState { special: true, ..i() }),
            (120, "boom", i()),
            (1, "jump", InputState { jump: true, ..i() }),
            (50, "out", i()),
        ]);
    } else {
        let at = InputState { attack: true, ..i() };
        v.extend([
            (1, "fire", at),
            (40, "hold", at),
            (4, "up", i()),
            (1, "fire", at),
            (30, "hold", at),
            (1, "dodge", InputState { dodge: true, ..at }),
            (30, "out", at),
        ]);
    }
    v
}

/// A run: the World, the fast shots seen, its lines.
struct Run {
    h: Harness,
    shots: BTreeSet<i32>,
    out: Vec<String>,
}

impl Run {
    /// The 2v2 World after 10 ticks with no input, the turret switched on, the first line written.
    fn new() -> Option<Run> {
        let player = |c: &str| MatchPlayer { character: Some(c.to_owned()), ..MatchPlayer::default() };
        let setup = MatchSetup {
            team_style: TeamStyle::TwoVsTwo,
            players: vec![player("irongiant"), player("shaggy"), player("shaggy"), player("shaggy")],
            ..MatchSetup::default()
        };
        let mut h = harness::fresh(&setup, 1)?;
        let none = [InputState::default(); 4];
        for _ in 0..10 {
            h.step(&none);
        }
        let g = h.fighter(0);
        toggle_ally_turret(&mut h.w, g, true);
        let mut r = Run { h, shots: BTreeSet::new(), out: Vec::new() };
        r.line("start");
        Some(r)
    }

    fn line(&mut self, tag: &str) {
        let l = line(&self.h, tag, &mut self.shots);
        self.out.push(l);
    }

    /// `n` ticks with the teammate's input `inp`, a line each.
    fn steps(&mut self, n: usize, tag: &str, inp: InputState) {
        for _ in 0..n {
            self.h.step(&[InputState::default(), InputState::default(), inp, InputState::default()]);
            self.line(tag);
        }
    }

    fn dump(&self, name: &str) {
        if let Ok(dir) = std::env::var("MVS_TURRET_DUMP") {
            std::fs::write(format!("{dir}/{name}"), self.out.join("\n") + "\n").expect("the dump");
        }
    }
}

/// The lines of a run, from the turret's switch on.
fn run(bomb: bool) -> Option<Vec<String>> {
    let mut r = Run::new()?;
    for (n, tag, inp) in phases(bomb) {
        r.steps(n, tag, inp);
    }
    r.dump(if bomb { "rs_bomb.txt" } else { "rs_shots.txt" });
    Some(r.out)
}

/// Each oracle line is the run's line of its tick and phase.
fn check(lines: &[String], want: &[&str]) {
    for w in want {
        let key: Vec<&str> = w.split(' ').take(2).collect();
        let got = lines.iter().find(|l| l.split(' ').take(2).eq(key.iter().copied()));
        assert_eq!(got.map(String::as_str), Some(*w), "tick {} {}", key[0], key[1]);
    }
}

// Columns: tick, phase, jump_on_ally, has_pilot, reticle, aim_x, aim_y, the pilot's stage, his x, y, kick, bomb, shot,
// fire, held, live reticles, fast shots spawned and the live ones' x, y, the bomb (x, y, scale, exploding), the enemy's
// damage.
// (The oracle agrees on every tick of both runs as written; these are its turns.)

/// The climb in (the overlap's begin, the item press, the buff's two stages and the place at (0, 500)), the aim and its
/// clamp to the blast box less BlastBoxBuffer, the bomb's lead-in, flight, arrival, growth and tick damage, its end
/// (the last hit, the actor gone), the jump out (the buff's end puts the pilot at the Giant, clamped to the blast
/// limits), NewPilotDelay and HasPilot off.
#[test]
fn a_teammate_climbs_in_aims_and_bombs_as_the_oracle() {
    let Some(lines) = run(true) else { return };
    check(
        &lines,
        &[
            "20 walk 2 0 0 0 0 -1 -994.282 210.412 0 0 0 0 0 0 0:- - 0",
            "26 item -1 1 1 -300 870.412 0 -803.982 210.412 0 0 0 0 0 1 0:- - 0",
            "27 wait -1 1 1 -300 870.412 1 -791.711 210.412 0 0 0 0 0 1 0:- - 0",
            "29 wait -1 1 1 -300 870.412 2 0 500 0 0 0 0 0 1 0:- - 0",
            "60 right -1 1 1 483.333 870.412 2 0 500 0 0 0 0 0 1 0:- - 0",
            "75 down -1 1 1 841.912 728.991 2 0 500 0 0 0 0 0 1 0:- - 0",
            "92 bomb -1 1 1 841.912 328.991 2 0 500 0 18 0 0 0 1 0:- - 0",
            "110 boom -1 1 1 841.912 328.991 2 0 500 0 0 0 0 0 1 0:- -679.226,682.709,1,0 0",
            "134 boom -1 1 1 841.912 328.991 2 0 500 0 0 0 0 0 1 0:- 841.912,328.991,1.5,1 3",
            "160 boom -1 1 1 841.912 328.991 2 0 500 0 0 0 0 0 1 0:- 841.912,328.991,2.222,1 10",
            "190 boom -1 1 1 841.912 328.991 2 0 500 0 0 0 0 0 1 0:- 841.912,328.991,3.056,1 17",
            "213 jump -1 1 0 841.912 328.991 -1 -800 470.412 45 0 0 0 0 0 0:- 841.912,328.991,3.694,1 23",
            "222 out -1 1 0 841.912 328.991 -1 -800 412.531 36 0 0 0 0 0 0:- 841.912,328.991,3.944,1 26",
            "224 out -1 1 0 841.912 328.991 -1 -800 383.947 34 0 0 0 0 0 0:- - 34",
            "258 out -1 0 0 841.912 328.991 -1 -800 210.412 0 0 0 0 0 0 0:- - 34",
        ],
    );
}

/// The first press (a shot at once, AutoFireTimer pending), the timer's activation and its calls (one shot per
/// AutoFireRate while held, AttackMontageNumber cycling), the timer cleared at a call with the button released, a
/// second press, and the evade press out of the turret (the kick in weapon mode: NewPilotDelay) with the attack still
/// held. The shots' places (the sockets on the tentacles of the Giant's composed pose: TenticleOverride, the turret
/// aim flags and the anim state from his pawn code) and the enemy's damage are the oracle's too.
#[test]
fn a_teammate_fires_the_tentacle_shots_as_the_oracle() {
    let Some(lines) = run(false) else { return };
    check(
        &lines,
        &[
            "92 fire -1 1 1 841.912 328.991 2 0 500 0 0 1 2 1 1 1:-521.943,913.723 - 0",
            "104 hold -1 1 1 841.912 328.991 2 0 500 0 0 1 2 1 1 1:204.777,579.402 - 0",
            "109 hold -1 1 1 841.912 328.991 2 0 500 0 0 1 2 1 1 1:503.089,430.682 - 0",
            "110 hold -1 1 1 841.912 328.991 2 0 500 0 0 2 2 1 1 2:562.424,400.288;-727.324,1060.4 - 0",
            "116 hold -1 1 1 841.912 328.991 2 0 500 0 0 2 2 1 1 2:-366.669,887.423 - 1",
            "127 hold -1 1 1 841.912 328.991 2 0 500 0 0 2 2 1 1 2:284.354,549.97 - 1",
            "128 hold -1 1 1 841.912 328.991 2 0 500 0 0 0 2 1 1 3:342.864,518.017;-841.477,965.268 - 1",
            "133 up -1 1 1 841.912 328.991 2 0 500 0 0 0 2 0 1 3:633.685,355.132;-530.814,844.454 - 1",
            "137 fire -1 1 1 841.912 328.991 2 0 500 0 0 0 2 1 1 3:-283.967,743.577 - 2",
            "140 hold -1 1 1 841.912 328.991 2 0 500 0 0 0 2 1 1 3:-99.844,665.485 - 2",
            "150 hold -1 1 1 841.912 328.991 2 0 500 0 0 1 2 1 1 4:507.37,390.378;-352.648,844.623 - 3.25",
            "152 hold -1 1 1 841.912 328.991 2 0 500 0 0 1 2 1 1 4:627.567,332.671;-231.323,789.324 - 3.25",
            "164 hold -1 1 1 841.912 328.991 2 0 500 0 0 2 2 1 1 5:487.771,438.896;-710.939,1064.542 - 4.25",
            "167 hold -1 1 1 841.912 328.991 2 0 500 0 0 2 2 1 1 5:665.094,346.398;-530.667,977.93 - 4.25",
            "168 dodge -1 1 0 841.912 328.991 -1 -800 470.412 45 0 2 0 1 0 5:-470.79,948.618 - 5.25",
            "176 out -1 1 0 841.912 328.991 -1 -800 424.679 37 0 2 0 1 0 5:4.3,706.31 - 6.4",
            "188 out -1 1 0 841.912 328.991 -1 -800 210.412 25 0 2 0 1 0 5:703.373,317.467 - 6.4",
            "198 out -1 1 0 841.912 328.991 -1 -800 210.412 15 0 2 0 1 0 5:- - 7.4",
        ],
    );
}

/// The teammate walks through the Giant (the overlap's begin, then its end: the label off) and back into him, climbs
/// in, aims down to the clamp (the blast box's bottom plus BlastBoxBuffer, below the stage) and bombs: the bomb's trace
/// meets the stage block and it stops on its top. Then the mode's end (KickPilot(false): HasPilot off at once, the
/// pilot put back), the mode again with the teammate overlapping (the seed: no begin, so his item press does nothing),
/// and the owner's reset (the turret gone, the reticle with it).
#[test]
fn the_overlap_the_mode_end_and_the_reset_as_the_oracle() {
    let Some(mut k) = Run::new() else { return };
    let i = InputState::default;
    let g = k.h.fighter(0);
    k.steps(1, "seed", i());
    k.steps(22, "walk", InputState { x: 1.0, ..i() });
    k.steps(8, "back", InputState { x: -1.0, ..i() });
    k.steps(2, "rest", i());
    k.steps(1, "item", InputState { item: true, ..i() });
    k.steps(8, "wait", i());
    k.steps(40, "down", InputState { y: -1.0, ..i() });
    k.steps(1, "bomb", InputState { special: true, ..i() });
    k.steps(40, "boom", i());
    toggle_ally_turret(&mut k.h.w, g, false);
    k.line("off");
    k.steps(5, "after", i());
    toggle_ally_turret(&mut k.h.w, g, true);
    k.line("on");
    k.steps(3, "again", i());
    k.steps(1, "item", InputState { item: true, ..i() });
    k.steps(3, "wait", i());
    k.dump("rs_edge.txt");
    ally_turret_reset(&mut k.h.w, g);
    let w = &k.h.w;
    let a = k.h.fighter(2);
    let ret = w.actors.actors.iter().filter(|(_, x)| x.class == WorldActorClass::C017PilotReticle && !x.dead).count();
    let fa = w.fighter(a);
    let reset = format!(
        "{} reset {} {} {} {} {}",
        w.flow.tick,
        if giant(w, g).ally_turret.is_none() { "none" } else { "some" },
        pilot_stage(w, a),
        r(fa.body.x),
        r(fa.body.y),
        ret
    );
    let lines = &k.out;
    check(
        lines,
        &[
            "20 walk 2 0 0 0 0 -1 -994.282 210.412 0 0 0 0 0 0 0:- - 0",
            "32 walk -1 0 0 0 0 -1 -548.854 210.412 0 0 0 0 0 0 0:- - 0",
            "41 back 2 0 0 0 0 -1 -610.927 210.412 0 0 0 0 0 0 0:- - 0",
            "44 item -1 1 1 -300 870.412 0 -685.221 210.412 0 0 0 0 0 1 0:- - 0",
            "47 wait -1 1 1 -300 870.412 2 0 500 0 0 0 0 0 1 0:- - 0",
            "93 bomb -1 1 1 -300 -379.588 2 0 500 0 18 0 0 0 1 0:- - 0",
            "111 boom -1 1 1 -300 -379.588 2 0 500 0 0 0 0 0 1 0:- -718.752,636.174,1,0 0",
            "124 boom -1 1 1 -300 -379.588 2 0 500 0 0 0 0 0 1 0:- -463.907,18,1.556,1 0",
            "133 off -1 0 0 -300 -379.588 -1 -800 470.412 0 0 0 0 0 0 0:- -463.907,18,1.806,1 0",
            "138 on -1 0 0 -300 -379.588 -1 -800 452.547 0 0 0 0 0 0 0:- -463.907,18,1.944,1 0",
            "145 wait -1 0 0 -300 -379.588 -1 -800 367.512 0 0 0 0 0 0 0:- -463.907,18,2.139,1 0",
        ],
    );
    assert_eq!(reset, "145 reset none -1 -800 367.512 0");
}
