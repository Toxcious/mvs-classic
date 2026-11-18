//! Superman's bot tasks on the tree runner: each task run by a one-task tree on
//! a Superman of a set-up world, headless. The expected sticks, presses and tick counts are worked out in the oracle's
//! test
//! from the graphs' numbers: a Delay of s seconds is s x 60 thinks.
//!
//! Not ported from the oracle's test (the decorators are `ai/decorators.rs`'s, the rest are long flights asserted on
//! the marker's place): the registry and injection-map checks (the Rust registry is a table checked at bind),
//! TargetInHorizontalRange / PointInFrontOfMe, the two full flights to a point, the whole FlyGrab at an enemy and
//! the Superpunch marker-steering numbers.

use crate::actors::ActorRef;
use crate::ai::blackboard::{BbVal, BbVec};
use crate::ai::bt::BtBot;
use crate::fighter::{FighterId, actor_y, set_grounded};
use crate::input::{InputState, NO_INPUT};
use crate::name;
use crate::setup::{MatchPlayer, MatchSetup};
use crate::world::World;
use indexmap::IndexMap;
use mvs_data::BotTrees;
use mvs_data::tables::{BtAuxDef, BtChildDef, BtNodeDef, BtTree};
use mvs_data::value::{Props, Value};
use std::sync::Arc;

const FLY: &str = "Superman_Flying_Montage";
const AIM: &str = "Superman_GroundAimPunch_Montage";

struct R {
    w: World,
    f: FighterId,
    v: FighterId,
}

/// `make`: he (fighter 0) at x 0 on the floor facing `fc`, the other fighter `dx` ahead of him; `up`: he is put that
/// far above the floor, falling.
fn make(fc: f64, dx: f64, up: f64) -> Option<R> {
    let dir = mvs_store::dev_store_dir();
    let Ok(store) = mvs_store::Store::open(&dir) else {
        eprintln!("no store at {}: skipped", dir.display());
        return None;
    };
    let data = Arc::new(
        mvs_data::GameData::load(&store, &["superman", "shaggy"], "trainingroomsmall").expect("the gamedata loads"),
    );
    let p = |c: &str| MatchPlayer { character: Some(c.into()), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![p("superman"), p("shaggy")], ..MatchSetup::default() };
    let mut w = World::new(data, 1, &setup).expect("binds");
    let (f, v) = (w.players[0], w.players[1]);
    w.fighter_mut(f).body.x = 0.0;
    w.fighter_mut(v).body.x = mvs_sim::math::narrow(dx * fc);
    w.fighter_mut(f).body.facing_dir = mvs_sim::math::narrow(fc);
    for _ in 0..40 {
        w.step(&[NO_INPUT, NO_INPUT]);
        w.events.clear();
    }
    w.fighter_mut(f).body.facing_dir = mvs_sim::math::narrow(fc);
    if up != 0.0 {
        w.fighter_mut(f).body.y += mvs_sim::math::narrow(up);
        set_grounded(&mut w, f, false);
        w.fighter_mut(f).body.vy = 0.0;
    }
    Some(R { w, f, v })
}

fn props_of(kv: &[(&str, Value)]) -> Props {
    kv.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
}

fn sel(key: &str) -> Value {
    Value::Obj(props_of(&[("key", Value::Str(key.into()))]))
}

fn node(id: &str, ty: &str, p: &[(&str, Value)], children: Option<Vec<BtChildDef>>) -> BtNodeDef {
    BtNodeDef { id: id.into(), ty: ty.into(), props: props_of(p), services: None, children }
}

fn child(n: BtNodeDef, guard: bool) -> BtChildDef {
    let decorators = guard.then(|| {
        vec![BtAuxDef {
            id: "g".into(),
            ty: "BTDecorator_Blackboard".into(),
            props: props_of(&[
                ("BlackboardKey", sel("EnemyActor")),
                ("FlowAbortMode", Value::Str("EBTFlowAbortMode::Self".into())),
            ]),
        }]
    });
    BtChildDef { node: n, decorators, ops: None }
}

/// `tok`: the frame's input as the oracle's test writes it.
fn tok(i: &InputState) -> String {
    let mut s = format!("{}", i.x + 0.0);
    if i.y > 0.0 {
        s.push('u');
    } else if i.y < 0.0 {
        s.push('d');
    }
    if i.special {
        s.push('S');
        if let Some(d) = i.atk_dir {
            s.push_str(&(d as u8).to_string());
        }
    }
    if i.attack {
        s.push('A');
    }
    if i.jump {
        s.push('J');
    }
    if i.dodge {
        s.push('E');
    }
    s
}

fn rle(t: &[String]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let mut k = i;
        while k < t.len() && t[k] == t[i] {
            k += 1;
        }
        out.push(if k - i > 1 { format!("{}*{}", t[i], k - i) } else { t[i].clone() });
        i = k;
    }
    out.join(" ")
}

struct Run {
    ev: String,
    pad: String,
    io: Vec<String>,
    mont: Vec<String>,
}

type At<'a> = &'a mut dyn FnMut(i32, &mut BtBot, &mut World, FighterId, FighterId);

/// `run`: the task, then a wait that never ends (a Sequence); `guard`: the task under a Blackboard decorator on
/// EnemyActor that aborts it (a Selector, FlowAbortMode Self). `at(k)` runs before think k.
fn run(r: &mut R, ty: &str, p: &[(&str, Value)], n: i32, guard: bool, mut at: Option<At>) -> Run {
    let t = child(node("t", ty, p, None), guard);
    let wait = child(node("w", "BTTask_Wait", &[("WaitTime", Value::Num(100.0))], None), false);
    let root = node("r", if guard { "BTComposite_Selector" } else { "BTComposite_Sequence" }, &[], Some(vec![t, wait]));
    let real = &r.w.data.bot_trees;
    let mut trees = IndexMap::new();
    trees.insert("Main".to_owned(), BtTree { root });
    let data = BotTrees { trees, inject: IndexMap::new(), ..real.clone() };
    let mut bot = BtBot::new(7, Arc::new(data), 1.0, Some(name!("Main")), None);
    bot.t.log = Some(Vec::new());
    let (f, v) = (r.f, r.v);
    let k = bot.bb.key("EnemyActor");
    bot.set_bb(&mut r.w, f, k, BbVal::Obj(ActorRef::Fighter(v)));
    let (mut ev, mut io, mut mont) = (Vec::new(), Vec::new(), Vec::<String>::new());
    for k in 0..n {
        if let Some(at) = at.as_mut() {
            at(k, &mut bot, &mut r.w, f, v);
        }
        let out = bot.think(&mut r.w, f);
        io.push(tok(&out));
        r.w.step(&[out, NO_INPUT]);
        r.w.events.clear();
        for e in bot.t.log.as_mut().expect("the log").drain(..) {
            ev.push(format!("{k}:{}", e.strip_prefix("exec ").unwrap_or(&e).replace("tree finished", "END")));
        }
        let m = r.w.fighter(f).montages.montage.as_ref().map_or("-", |m| r.w.names.str(m.montage.name)).to_owned();
        if mont.last() != Some(&m) {
            mont.push(m);
        }
    }
    Run { ev: ev.join(" "), pad: rle(&io), io, mont }
}

fn set_vec(bot: &mut BtBot, w: &mut World, me: FighterId, key: &str, v: BbVec) {
    let k = bot.bb.key(key);
    bot.set_bb(w, me, k, BbVal::Vec(v));
}

fn set_enemy(bot: &mut BtBot, w: &mut World, me: FighterId, v: Option<FighterId>) {
    let k = bot.bb.key("EnemyActor");
    bot.set_bb(w, me, k, v.map_or(BbVal::Null, |v| BbVal::Obj(ActorRef::Fighter(v))));
}

/// The point `dx` ahead and `dz` above him, as a blackboard vector.
fn pt(w: &World, f: FighterId, dx: f64, dz: f64) -> BbVec {
    BbVec {
        x: 0.0,
        y: -(w.fighter(f).body.x + mvs_sim::math::narrow(dx)),
        z: actor_y(w, f) + mvs_sim::math::narrow(dz),
    }
}

fn sel_p(k: &str, key: &str) -> [(&'static str, Value); 1] {
    [(if k == "t" { "TargetLocation" } else { "Enemy Actor" }, sel(key))]
}

#[track_caller]
fn check(fails: &mut Vec<String>, name: &str, got: &str, want: &str) {
    if got != want {
        fails.push(format!("{name}: {got} (expect {want})"));
    }
}

#[test]
#[allow(clippy::too_many_lines, reason = "the checks in their order")]
fn superman_tasks() {
    let mut fails = Vec::new();
    let f = &mut fails;
    let tl = sel_p("t", "RecoveryPoint");
    let en = sel_p("e", "EnemyActor");

    // ---- SupermanFlyToPoint_BTT.
    // In the air, the point 900 ahead and 300 above / below / 40 above: the special press with Forward (2) in the
    // first think and the stick up / down / untouched.
    for (name, dz, y0, want) in [
        ("FlyToPoint in the air, point far and 300 above", 300.0, 0.0, "0uS2 0uS true 0:t"),
        ("FlyToPoint in the air, point far and 300 below", -300.0, 0.0, "0dS2 0dS true 0:t"),
        ("FlyToPoint, 40 above: the stick is not written", 40.0, 0.0, "0S2 0S true 0:t"),
        ("FlyToPoint, 40 above, a stick left up", 40.0, 1.0, "0uS2 0uS true 0:t"),
    ] {
        let Some(mut r) = make(1.0, 2000.0, 700.0) else { return };
        let mut at = |k: i32, bot: &mut BtBot, w: &mut World, me: FighterId, _v: FighterId| {
            if k == 0 {
                let p = pt(w, me, 900.0, dz);
                set_vec(bot, w, me, "RecoveryPoint", p);
                bot.pad.set_y(y0);
            }
        };
        let g = run(&mut r, "SupermanFlyToPoint_BTT_C", &tl, 8, false, Some(&mut at));
        let got = format!("{} {} {} {}", g.io[0], g.io[4], g.mont.iter().any(|m| m == FLY), g.ev);
        check(f, name, &got, want);
    }
    if let Some(mut r) = make(1.0, 2000.0, 700.0) {
        let g = run(&mut r, "SupermanFlyToPoint_BTT_C", &tl, 8, false, None);
        check(f, "FlyToPoint in the air, the key unset (FLT_MAX): up", &format!("{} {}", g.io[0], g.ev), "0uS2 0:t");
    }
    // On the ground at the execute: the press and the release in the same event, success.
    if let Some(mut r) = make(1.0, 2000.0, 0.0) {
        let mut at = |k: i32, bot: &mut BtBot, w: &mut World, me: FighterId, _v: FighterId| {
            if k == 0 {
                let p = pt(w, me, 900.0, 300.0);
                set_vec(bot, w, me, "RecoveryPoint", p);
            }
        };
        let g = run(&mut r, "SupermanFlyToPoint_BTT_C", &tl, 6, false, Some(&mut at));
        check(
            f,
            "FlyToPoint on the ground: a one-think press, then the next task",
            &format!("{} | {}", g.pad, g.ev),
            "0S2 0*5 | 0:t 1:w",
        );
    }
    // Inside 200 of the point at the execute: the button is held for the 2 s Delay with no test in between, then
    // released; the stick is not written.
    if let Some(mut r) = make(1.0, 2000.0, 700.0) {
        let mut at = |k: i32, bot: &mut BtBot, w: &mut World, me: FighterId, _v: FighterId| {
            if k == 0 {
                let p = pt(w, me, 150.0, 100.0);
                set_vec(bot, w, me, "RecoveryPoint", p);
            }
        };
        let g = run(&mut r, "SupermanFlyToPoint_BTT_C", &tl, 126, false, Some(&mut at));
        let away = (r.w.fighter(r.f).body.x - 150.0).abs() > 200.0;
        check(
            f,
            "FlyToPoint inside 200: held 2 s, then released",
            &format!("{} | {} | left the 200 {away}", g.pad, g.ev),
            "0S2 0S*118 0*7 | 0:t 120:w | left the 200 true",
        );
    }
    // Aborted in the loop: the button is released, the stick stays.
    if let Some(mut r) = make(1.0, 2000.0, 700.0) {
        let mut at = |k: i32, bot: &mut BtBot, w: &mut World, me: FighterId, _v: FighterId| {
            if k == 0 {
                let p = pt(w, me, 900.0, 300.0);
                set_vec(bot, w, me, "RecoveryPoint", p);
            }
            if k == 8 {
                set_enemy(bot, w, me, None);
            }
        };
        let g = run(&mut r, "SupermanFlyToPoint_BTT_C", &tl, 14, true, Some(&mut at));
        check(
            f,
            "FlyToPoint aborted: released, the stick left up",
            &format!("{} | {}", g.pad, g.ev),
            "0uS2 0uS*7 0u*6 | 0:t 8:abort t 8:w",
        );
    }

    // ---- SupermanFlyGrab_BTT.
    // In the air, the enemy 900 ahead and 400 below (on the floor): the stick forward at once, the special with
    // Forward in the link of the 0.05 s Delay, the stick down from the loop's first pass in that same link.
    for fc in [1.0, -1.0] {
        let Some(mut r) = make(fc, 900.0, 400.0) else { return };
        let g = run(&mut r, "SupermanFlyGrab_BTT_C", &en, 8, false, None);
        let n = format!("{fc}");
        check(
            f,
            &format!("FlyGrab in the air facing {fc}, the enemy ahead and below"),
            &format!("{} | {} | {}", g.pad, g.mont.iter().any(|m| m == FLY), g.ev),
            &format!("{n}*2 {n}dS2 {n}dS*5 | true | 0:t"),
        );
    }
    if let Some(mut r) = make(1.0, 900.0, 300.0) {
        let v = r.v;
        r.w.fighter_mut(v).body.y += 800.0;
        set_grounded(&mut r.w, v, false);
        r.w.fighter_mut(v).body.vy = 0.0;
        let g = run(&mut r, "SupermanFlyGrab_BTT_C", &en, 5, false, None);
        check(f, "FlyGrab in the air, the enemy 500 above", &g.io[2], "1uS2");
    }
    // On the ground: the press 2 thinks in, released in the same event, the stick back to 0.
    if let Some(mut r) = make(1.0, 900.0, 0.0) {
        let g = run(&mut r, "SupermanFlyGrab_BTT_C", &en, 8, false, None);
        check(
            f,
            "FlyGrab on the ground: forward 2 thinks, a one-think press, the next task",
            &format!("{} | {}", g.pad, g.ev),
            "1*2 0S2 0*5 | 0:t 3:w",
        );
    }
    // The key holds no fighter: no FinishExecute, nothing pressed.
    if let Some(mut r) = make(1.0, 900.0, 400.0) {
        let g = run(&mut r, "SupermanFlyGrab_BTT_C", &sel_p("e", "RecoveryPoint"), 20, false, None);
        check(
            f,
            "FlyGrab with a Vector key: the task hangs with nothing pressed",
            &format!("{} | {}", g.pad, g.ev),
            "0*20 | 0:t",
        );
    }
    if let Some(mut r) = make(1.0, 900.0, 400.0) {
        let mut at = |k: i32, bot: &mut BtBot, w: &mut World, me: FighterId, _v: FighterId| {
            if k == 8 {
                set_enemy(bot, w, me, None);
            }
        };
        let g = run(&mut r, "SupermanFlyGrab_BTT_C", &en, 14, true, Some(&mut at));
        check(
            f,
            "FlyGrab aborted in the loop: released, MoveX 0, the stick stays down",
            &format!("{} | {}", g.pad, g.ev),
            "1*2 1dS2 1dS*5 0d*6 | 0:t 8:abort t 8:w",
        );
    }

    // ---- SupermanSuperpunch_BTT (no running tree reaches it: run by hand).
    // On the ground, the enemy 900 ahead: the aim punch, the marker steered for 8 passes, 0.2 s, release.
    if let Some(mut r) = make(1.0, 900.0, 0.0) {
        let g = run(&mut r, "SupermanSuperpunch_BTT_C", &en, 130, false, None);
        check(
            f,
            "Superpunch on the ground: the aim punch, the marker steered for 8 passes, 0.2 s, release",
            &format!("{} | {} | {}", g.pad, g.ev, g.mont[0]),
            &format!("0S2 0S*28 1S*18 -1S*3 1S*15 0*65 | 0:t 66:w | {AIM}"),
        );
    }
    // In the air the press is his flight and no marker comes; 0.5 s + 0.2 s, stick 0.
    if let Some(mut r) = make(1.0, 900.0, 700.0) {
        let g = run(&mut r, "SupermanSuperpunch_BTT_C", &en, 48, false, None);
        check(
            f,
            "Superpunch in the air: no marker, released after 0.7 s",
            &format!("{} | {} | {}", g.pad, g.ev, g.mont.iter().any(|m| m == FLY)),
            "0S2 0S*40 0*7 | 0:t 42:w | true",
        );
    }
    // Aborted in the steering (think 36): the button released, the stick left where it was.
    if let Some(mut r) = make(1.0, 900.0, 0.0) {
        let mut at = |k: i32, bot: &mut BtBot, w: &mut World, me: FighterId, _v: FighterId| {
            if k == 36 {
                set_enemy(bot, w, me, None);
            }
        };
        let g = run(&mut r, "SupermanSuperpunch_BTT_C", &en, 50, true, Some(&mut at));
        check(
            f,
            "Superpunch aborted: released, the stick stays",
            &format!("{} | {}", g.pad, g.ev),
            "0S2 0S*28 1S*7 1*14 | 0:t 36:abort t 36:w",
        );
    }
    // The enemy key emptied during the steering without an abort: IsValid ends the loop at the next pass.
    if let Some(mut r) = make(1.0, 900.0, 0.0) {
        let mut at = |k: i32, bot: &mut BtBot, w: &mut World, me: FighterId, _v: FighterId| {
            if k == 34 {
                set_enemy(bot, w, me, None);
            }
        };
        let g = run(&mut r, "SupermanSuperpunch_BTT_C", &en, 60, false, Some(&mut at));
        check(
            f,
            "Superpunch, the enemy gone at think 34: release 12 thinks after the pass of think 35",
            &format!("{} | {}", g.pad, g.ev),
            "0S2 0S*28 1S*18 0*13 | 0:t 48:w",
        );
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}
