//! The Blueprint tasks run in a tree (the Blueprint tasks and BotMoveTo with the
//! waypoint tasks): the pad each task writes. The expected lines are the ones the oracle's test gave.
//! A test without the dev store says so and passes.

use super::runner_tests::{TestState, ch, comp, flag, leaf, num, props_of, s, sel, task, world};
use crate::Real;
use crate::actors::ActorRef;
use crate::ai::blackboard::{BbType, BbVal, BbVec};
use crate::ai::bt::BtBot;
use crate::ai::classes::TaskSt;
use crate::ai::cx::{Cx, Resume, Step};
use crate::ai::props;
use crate::fighter::FighterId;
use crate::input::{InputState, NO_INPUT};
use crate::name;
use crate::world::World;
use mvs_data::BotTrees;
use mvs_data::tables::{BtNodeDef, BtTree};
use mvs_data::value::{Props, Value};
use std::sync::Arc;

type At<'a> = &'a mut dyn FnMut(i32, &mut BtBot, &mut World, FighterId, FighterId);

struct Rig {
    w: World,
    st: TestState,
    fails: Vec<String>,
}

struct Out {
    ev: String,
    pad: String,
    bot: BtBot,
}

/// Stick x, u / d, D for the down button, A S J E for attack, special, jump, evade (+ the attack direction).
fn tok(i: &InputState) -> String {
    let dir = || i.atk_dir.map_or(String::new(), |d| (d as u8).to_string());
    format!(
        "{}{}{}{}{}{}{}",
        i.x,
        if i.y > 0.0 {
            "u"
        } else if i.y < 0.0 {
            "d"
        } else {
            ""
        },
        if i.down_btn { "D" } else { "" },
        if i.attack { format!("A{}", dir()) } else { String::new() },
        if i.special { format!("S{}", dir()) } else { String::new() },
        if i.jump { "J" } else { "" },
        if i.dodge { "E" } else { "" },
    )
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

impl Rig {
    fn me(&self) -> FighterId {
        self.w.players[1]
    }

    fn foe(&self) -> FighterId {
        self.w.players[0]
    }

    fn x(&self, f: FighterId) -> Real {
        self.w.fighter(f).body.x
    }

    fn facing(&self) -> Real {
        self.w.fighter(self.me()).body.facing_dir
    }

    fn check(&mut self, name: &str, got: &str, want: &str) {
        if got != want {
            self.fails.push(format!("{name}: {got} (expect {want})"));
        }
    }

    /// `world.reset()` and 30 idle frames.
    fn settle(&mut self) {
        crate::world::reset::reset(&mut self.w);
        for _ in 0..30 {
            self.w.step(&[NO_INPUT, NO_INPUT]);
        }
        self.w.events.clear();
    }

    /// `runTree`: `root` for `n` thinks, `at` before each; the runner's events and the pad's output.
    fn run(&mut self, root: BtNodeDef, n: i32, at: Option<At>, seed: i32, difficulty: Real, step: bool) -> Out {
        let real = &self.w.data.bot_trees;
        let mut trees = mvs_data::fighter::Map::new();
        trees.insert("Base_BehaviorTree".to_owned(), BtTree { root });
        let mut inject = mvs_data::fighter::Map::new();
        inject.insert("shaggy".to_owned(), mvs_data::fighter::Map::new());
        let data = BotTrees { trees, inject, ..real.clone() };
        let mut bot = BtBot::new(seed, Arc::new(data), difficulty, Some(name!("Base_BehaviorTree")), None);
        bot.t.log = Some(Vec::new());
        let (me, foe) = (self.me(), self.foe());
        bot.test = std::mem::take(&mut self.st);
        bot.test.svc_ticks_clear();
        let (mut ev, mut io) = (Vec::new(), Vec::new());
        let mut at = at;
        for k in 0..n {
            bot.test.set_think(k);
            if let Some(at) = at.as_mut() {
                at(k, &mut bot, &mut self.w, me, foe);
            }
            let out = bot.think(&mut self.w, me);
            io.push(tok(&out));
            for e in bot.t.log.as_mut().expect("the log").drain(..) {
                let e = e.strip_prefix("exec ").unwrap_or(&e).replace("tree finished", "END");
                ev.push(format!("{k}:{e}"));
            }
            if step {
                self.w.step(&[NO_INPUT, out]);
                self.w.events.clear();
            }
        }
        self.st = std::mem::take(&mut bot.test);
        Out { ev: ev.join(" "), pad: rle(&io), bot }
    }

    fn go(&mut self, root: BtNodeDef, n: i32, at: Option<At>) -> Out {
        self.run(root, n, at, 7, 1.0, false)
    }

    fn pad_of(&mut self, class: &str, p: &[(&str, Value)], n: i32, at: Option<At>) -> String {
        self.go(then(class, p), n, at).pad
    }
}

/// The task, then a task that never ends.
fn then(class: &str, p: &[(&str, Value)]) -> BtNodeDef {
    comp("Sequence", vec![leaf(task("t", class, p)), leaf(task("h", "T_Hang", &[]))], &[], &[])
}

fn or_else(class: &str, p: &[(&str, Value)]) -> BtNodeDef {
    comp("Selector", vec![leaf(task("t", class, p)), leaf(task("h", "T_Hang", &[]))], &[], &[])
}

fn set_obj(bot: &mut BtBot, w: &mut World, me: FighterId, key: &str, a: FighterId) {
    let k = bot.bb.key(key);
    bot.set_bb(w, me, k, BbVal::Obj(ActorRef::Fighter(a)));
}

fn set_vec(bot: &mut BtBot, w: &mut World, me: FighterId, key: &str, v: BbVec) {
    let k = bot.bb.key(key);
    bot.set_bb(w, me, k, BbVal::Vec(v));
}

fn set_num(bot: &mut BtBot, w: &mut World, me: FighterId, key: &str, v: Real) {
    let k = bot.bb.key(key);
    bot.set_bb(w, me, k, BbVal::Num(v));
}

fn vec3(x: Real, y: Real, z: Real) -> BbVec {
    BbVec { x, y, z }
}

/// `then` of the match's time of the first think: the enemy key set at think 0.
fn set_foe(k: i32, bot: &mut BtBot, w: &mut World, me: FighterId, foe: FighterId) {
    if k == 0 {
        set_obj(bot, w, me, "EnemyActor", foe);
    }
}

fn first_h(ev: &str) -> Option<i32> {
    ev.split(' ').find_map(|e| e.strip_suffix(":h").and_then(|n| n.parse().ok()))
}

fn self_abort_tree(task_def: BtNodeDef) -> BtNodeDef {
    comp(
        "Selector",
        vec![ch(task_def, vec![flag("f", Some("Self"), false)], None), leaf(task("h", "T_Hang", &[]))],
        &[],
        &[],
    )
}

#[test]
#[allow(clippy::too_many_lines, reason = "the checks in their order, one world")]
fn tasks_on_a_tree() {
    let Some(w) = world() else { return };
    let mut r = Rig { w, st: TestState::default(), fails: Vec::new() };
    r.settle();
    let f = r.facing();
    let to_foe: Real = if r.x(r.foe()) > r.x(r.me()) { 1.0 } else { -1.0 };
    let to_center: Real = if r.x(r.me()) < 0.0 { 1.0 } else { -1.0 };
    let dir = |d: Real| [("InputDirection", num(d))];

    let got = r.pad_of(
        "BotAttack_BTT_C",
        &[("InputDirection", num(2.0)), ("ChargeTime", num(0.2)), ("HoldStickInDirection", Value::Bool(true))],
        14,
        None,
    );
    r.check("BotAttack forward, stick held", &got, &format!("{f}A2 {f}A*10 0*3"));
    r.st.set("f", true);
    let t = task(
        "t",
        "BotAttack_BTT_C",
        &[("InputDirection", num(2.0)), ("ChargeTime", num(1.0)), ("HoldStickInDirection", Value::Bool(true))],
    );
    let got = r
        .go(
            self_abort_tree(t),
            6,
            Some(&mut |k, bot, _, _, _| {
                if k == 3 {
                    bot.test.set("f", false);
                }
            }),
        )
        .pad;
    r.check("BotAttack aborted", &got, &format!("{f}A2 {f}A*2 0*3"));
    let got = r.pad_of("BotAttackForward_BTT_C", &[], 17, None);
    r.check("BotAttackForward", &got, &format!("{f}*2 {f}A2 {f}A*11 0*3"));
    let got = r.pad_of("BotHeavyAttack_BTT_C", &[("Attack Direction", num(2.0))], 3, None);
    r.check("BotHeavyAttack", &got, "0A2 0*2");
    // DoPreAttackRotation: a Forward / Back press with the stick centred takes LocalAttackFacing +1 / -1, an
    // absolute side, and turns him there whichever way he faced.
    let mut turns = Vec::new();
    for d in [2.0, 3.0] {
        r.settle();
        let before = r.facing();
        r.run(then("BotHeavyAttack_BTT_C", &[("Attack Direction", num(d))]), 3, None, 7, 1.0, true);
        turns.push(format!("{before}>{}", r.facing()));
    }
    r.check("BotHeavyAttack Forward / Back, stick centred: the facing", &turns.join(" "), &format!("{f}>1 {f}>-1"));
    r.settle();
    let got = r.pad_of("BotDownSpike_BTT_C", &[], 5, None);
    r.check("BotDownSpike", &got, "0A1 0A 0*3");
    let got = r.pad_of("BotSpecial_BTT_C", &[("InputDirection", num(4.0)), ("ChargeTime", num(0.1))], 8, None);
    r.check("BotSpecial neutral 0.1 s", &got, "0S4 0S*4 0*3");
    let got = r.pad_of("BotUpSpecial_BTT_C", &[], 14, None);
    r.check("BotUpSpecial", &got, "0uS0 0uS*10 0u*3");
    let got =
        format!("{} | {}", r.pad_of("BotMoveUp_BTT_C", &[], 3, None), r.pad_of("BotMoveDown_BTT_C", &[], 3, None));
    r.check("BotMoveUp / BotMoveDown", &got, "0u*3 | 0d*3");
    // The abort zeroes the stick and releases the special; with CancelEvadeOnAbort it presses the evade, and the
    // 0.1 s Delay that would release it is removed with the task: the evade stays held.
    let mut up = Vec::new();
    for cancel in [false, true] {
        r.st.set("f", true);
        let t = task(
            "t",
            "BotUpSpecialToPoint_BTT_C",
            &[
                ("LastGroundPosition", sel("TargetPoint")),
                ("HoldDuration", num(1.4)),
                ("CancelEvadeOnAbort", Value::Bool(cancel)),
            ],
        );
        up.push(
            r.go(
                self_abort_tree(t),
                14,
                Some(&mut |k, bot, _, _, _| {
                    if k == 3 {
                        bot.test.set("f", false);
                    }
                }),
            )
            .pad,
        );
    }
    r.check(
        "BotUpSpecialToPoint aborted, CancelEvadeOnAbort off / on",
        &up.join(" | "),
        "0uS0 0uS*2 0*11 | 0uS0 0uS*2 0E*11",
    );
    // 1.5 s in 0.05 steps is 31 steps in single precision. A target at the same height reads as below.
    let got = r.pad_of(
        "BotSideSpecialToActor_BTT_C",
        &[("Actor", sel("EnemyActor")), ("Duration", num(1.5)), ("FollowTargetVertically", Value::Bool(true))],
        95,
        Some(&mut set_foe),
    );
    r.check("BotSideSpecialToActor following", &got, &format!("{to_foe}S2 {to_foe}S {to_foe}dS*90 {to_foe}*3"));
    let got = r.pad_of("BotShortJump_BTT_C", &[("Duration", num(0.1))], 8, None);
    r.check("BotShortJump 0.1 s", &got, "0J*5 0*3");
    let got = r.pad_of("BotJumpUp_BTT_C", &[], 16, None);
    r.check("BotJumpUp: second press after a one-frame gap", &got, "0J*11 0 0J*4");
    let got = r.pad_of("BotJumpForward_BTT_C", &[], 14, None);
    r.check("BotJumpForward", &got, &format!("{f}J*11 {f}*3"));
    let got = r.pad_of("BotJumpBackward_BTT_C", &[], 14, None);
    r.check("BotJumpBackward", &got, &format!("{}J*11 {}*3", -f, -f));
    let got = r.pad_of("BotFastFall_BTT_C", &[], 32, None);
    r.check("BotFastFall", &got, "0d*29 0*3");
    let got =
        r.pad_of("BotCrouch_BTT_C", &[("CrouchTime", num(0.5)), ("EnemyBeingDodged", sel("TargetPoint"))], 32, None);
    r.check("BotCrouch, no fighter in the key", &got, "0D*29 0*3");
    // 0.4 - 4 x 0.1 is still above 0 in single precision: five 0.1 s steps.
    let got = r.pad_of(
        "BotCrouch_BTT_C",
        &[("CrouchTime", num(2.5)), ("EnemyBeingDodged", sel("EnemyActor"))],
        32,
        Some(&mut set_foe),
    );
    r.check("BotCrouch, the fighter is not attacking", &got, "0D*29 0*3");
    let got = r.pad_of(
        "BotCrouchUnderProjectile_BTT_C",
        &[("CrouchTime", num(0.2)), ("ProjectileBeingDodged", sel("EnemyProjectile"))],
        14,
        None,
    );
    r.check("BotCrouchUnderProjectile, no projectile", &got, "0D*11 0*3");
    let got = r.pad_of("BotEvadeUp_BTT_C", &[], 3, None);
    r.check("BotEvadeUp", &got, "0uE*3");
    let got = r.pad_of("BotEvadeNeutral_BTT_C", &[], 14, None);
    r.check("BotEvadeNeutral", &got, "0E*11 0*3");
    let got = r.pad_of("BotEvadeToCenter_BTT_C", &[], 3, None);
    r.check("BotEvadeToCenter", &got, &format!("{to_center}E {to_center}*2"));
    let got = r.pad_of("BotEvadeToTarget_BTT_C", &[("EnemyActor", sel("EnemyActor"))], 14, Some(&mut set_foe));
    r.check("BotEvadeToTarget", &got, &format!("{to_foe}E*11 0*3"));
    let got = r.pad_of("BotEvadeAwayFromTarget_BTT_C", &[("EnemyActor", sel("EnemyActor"))], 14, Some(&mut set_foe));
    r.check("BotEvadeAwayFromTarget", &got, &format!("{}E*11 0*3", -to_foe));
    let mut seen = std::collections::BTreeSet::new();
    for sd in 1..=80 {
        let out = r.run(then("BotEvadeRandomDirection_BTT_C", &[]), 1, None, sd, 1.0, false);
        seen.insert(out.pad);
    }
    let got = seen.into_iter().collect::<Vec<_>>().join(" ");
    r.check("BotEvadeRandomDirection never goes up", &got, "-1E -1dE 0E 0dE 1E 1dE");

    // BotExecutePause and the turns.
    let pause = [("PauseTimeBB", sel("PauseTime")), ("TargetEnemyBB", sel("EnemyActor"))];
    let mut at_pause = |k: i32, bot: &mut BtBot, w: &mut World, me: FighterId, foe: FighterId| {
        set_foe(k, bot, w, me, foe);
        if k == 0 {
            set_num(bot, w, me, "PauseTime", 1.0);
        }
    };
    let out = r.go(then("BotExecutePause_BTT_C", &pause), 40, Some(&mut at_pause));
    let left = |o: &Out| {
        let k = o.bot.bb.key("PauseTime");
        format!("{:.2}", o.bot.bb.num(k))
    };
    let got = format!("{} | {} | left {}", out.pad, out.ev, left(&out));
    r.check("BotExecutePause", &got, "0*40 | 0:t 30:h | left 0.50");
    let me = r.me();
    r.w.fighter_mut(me).body.facing_dir = -f;
    let out = r.go(then("BotExecutePause_BTT_C", &pause), 24, Some(&mut at_pause));
    // The mock never turns: from the second pair on the turn Delay's link runs after the loop's (latent slot order).
    let got = format!("{} | {} | left {}", out.pad, out.ev, left(&out));
    r.check(
        "BotExecutePause turning (a turn costs double; stuck turn: stick released after the loop link)",
        &got,
        &format!("{to_foe}*5 0*19 | 0:t 18:h | left 0.45"),
    );
    // The 0.05 s Delay ends in the latent pass of its third frame: the stick reaches two pawn ticks.
    let got = r.pad_of("BotTurnToward_BTT_C", &[("Enemy", sel("EnemyActor"))], 5, Some(&mut set_foe));
    r.check("BotTurnToward: the tap holds the stick 2 pawn ticks", &got, &format!("{to_foe}*2 0*3"));
    let got = r.pad_of("BotTurnAround_BTT_C", &[], 2, None);
    r.check("BotTurnAround", &got, &format!("{to_foe}*2"));
    r.w.fighter_mut(me).body.facing_dir = f;
    let got = r.go(then("BotTurnToward_BTT_C", &[("Enemy", sel("EnemyActor"))]), 3, Some(&mut set_foe)).ev;
    r.check("BotTurnToward, already facing", &got, "0:t 1:h");
    // BotTurnAwayFromEnemy: the same 0.05 s tap, when the enemy is in front; no enemy in the key: no result.
    let away = [("Enemy", sel("EnemyActor"))];
    let got = r.pad_of("BotTurnAwayFromEnemy_BTT_C", &away, 5, Some(&mut set_foe));
    r.check("BotTurnAwayFromEnemy, facing the enemy", &got, &format!("{}*2 0*3", -to_foe));
    r.w.fighter_mut(me).body.facing_dir = -f;
    let back = r.go(then("BotTurnAwayFromEnemy_BTT_C", &away), 3, Some(&mut set_foe));
    r.w.fighter_mut(me).body.facing_dir = f;
    let none = r.go(then("BotTurnAwayFromEnemy_BTT_C", &away), 5, None).ev;
    r.check(
        "BotTurnAwayFromEnemy, facing away / no enemy (hangs)",
        &format!("{} {} | {none}", back.ev, back.pad),
        "0:t 1:h 0*3 | 0:t",
    );
    let got = [1.0, 1.5, 3.0]
        .iter()
        .map(|&d| r.run(then("DifficultyDelay_BTT_C", &[]), 30, None, 7, d, false).ev)
        .collect::<Vec<_>>()
        .join(" | ");
    r.check("DifficultyDelay at 1, 1.5, 3", &got, "0:t 24:h | 0:t 18:h | 0:t 1:h");
    let out = r.go(then("DifficultyPause_BTD_C", &[("PauseTimeBB", sel("PauseTime"))]), 2, None);
    let k = out.bot.bb.key("PauseTime");
    let v = out.bot.bb.num(k);
    let got = if (0.525..=0.875).contains(&v) { "in 0.7 +- 25%".to_owned() } else { v.to_string() };
    r.check("DifficultyPause easy", &got, "in 0.7 +- 25%");
    let out = r.go(
        then("BotFlee_BTT_C", &[("Duration", num(1.0)), ("DurationVariance", num(0.3)), ("FleeTime", sel("FleeTime"))]),
        2,
        None,
    );
    let k = out.bot.bb.key("FleeTime");
    let t = out.bot.bb.num(k);
    let got = if (0.7..=1.3).contains(&t) { "in 1 +- 0.3".to_owned() } else { t.to_string() };
    r.check("BotFlee", &got, "in 1 +- 0.3");
    let got = r.go(then("DifficultyCancel_BTT_C", &[]), 2, None).ev;
    r.check("DifficultyCancel never cancels", &got, "0:t 1:h");
    let out = r.go(
        or_else("ClearBlackboardValue_BTT_C", &[("BlackboardKey", sel("Item"))]),
        2,
        Some(&mut |k, bot, w, me, foe| {
            if k == 0 {
                set_obj(bot, w, me, "Item", foe);
            }
        }),
    );
    let k = out.bot.bb.key("Item");
    r.check(
        "ClearBlackboardValue fails",
        &format!("{} item {}", out.ev, out.bot.bb.obj(k).is_none()),
        "0:t 1:h item true",
    );
    // BFTaunt(0) starts Shaggy's default taunt on the ground, so the task succeeds.
    let got = r.go(or_else("BotTaunt_BTT_C", &[]), 2, None).ev;
    r.check("BotTaunt plays slot 0", &got, "0:t 1:END");
    r.settle();
    let got = r.go(or_else("BotThrowItemAtAnyEnemy_C", &[]), 2, Some(&mut set_foe)).ev;
    r.check("BotThrowItemAtAnyEnemy fails on the ground", &got, "0:t 1:h");
    let got = r.go(or_else("BotDI_BTD_C", &[("DI_X", num(1.0)), ("DI_Y", num(-1.0))]), 2, None).pad;
    r.check("BotDI fails with the stick set", &got, &format!("{f}d*2"));
    // ChooseMovementMode 75:25 over 800 picks.
    let choice = |k: &str, v: Real| Value::Obj(props_of(&[("Key", s(k)), ("Value", num(v))]));
    let choices = Value::Arr(vec![
        choice("Enum_BTMovementMode::NewEnumerator1", 75.0),
        choice("Enum_BTMovementMode::NewEnumerator0", 25.0),
    ]);
    let (mut ones, mut n) = (0.0, 0.0);
    r.go(
        comp(
            "Sequence",
            vec![leaf(task("t", "ChooseMovementMode_BTT_C", &[("Key", sel("MovementMode")), ("Choices", choices)]))],
            &[],
            &[],
        ),
        1600,
        Some(&mut |k, bot, _, _, _| {
            if k % 2 == 1 {
                n += 1.0;
                let key = bot.bb.key("MovementMode");
                ones += bot.bb.num(key);
            }
        }),
    );
    let share = ones / n;
    let got = format!(
        "{n} picks, mode 1 {}",
        if share > 0.7 && share < 0.8 { "near 3/4".to_owned() } else { format!("{share:.3}") }
    );
    r.check("ChooseMovementMode 75:25", &got, "800 picks, mode 1 near 3/4");
    // The Vector key reads None as an actor: the bot walks to the world origin, not to the point.
    let mx = r.x(me);
    let out = r.go(
        then("BotMoveToward_BTT_C", &[("TargetPointBB", sel("RecoveryPoint"))]),
        2,
        Some(&mut |k, bot, w, me, _| {
            if k == 0 {
                set_vec(bot, w, me, "RecoveryPoint", vec3(0.0, -mx * 3.0, 0.0));
            }
        }),
    );
    r.check("BotMoveToward", &format!("{} | {}", out.pad, out.ev), &format!("{to_center}*2 | 0:t 1:h"));
    let out = r.go(then("SetTargetPointForSuicide_BTT_C", &[("TargetPoint", sel("TempPoint"))]), 1, None);
    let k = out.bot.bb.key("TempPoint");
    let p = out.bot.bb.vec(k).expect("the point");
    r.check("SetTargetPointForSuicide", &format!("{} {}", p.y, p.z), &format!("{} 99999", to_center * 99999.0));
    let out = r.go(
        then("UpdateTargetPosition_BTT_C", &[("EnemyActor", sel("EnemyActor")), ("TargetPoint", sel("TargetPoint"))]),
        1,
        Some(&mut set_foe),
    );
    let k = out.bot.bb.key("TargetPoint");
    let p = out.bot.bb.vec(k).expect("the point");
    let foe = r.foe();
    let (fx, fy) = (r.x(foe), r.w.fighter(foe).body.y);
    r.check("UpdateTargetPosition", &format!("{} {}", p.y, p.z), &format!("{} {}", -fx, fy));

    // With the world stepped by the bot's output.
    let mut air = 0;
    let out = r.run(
        comp(
            "Sequence",
            vec![
                leaf(task("j", "BotShortJump_BTT_C", &[("Duration", num(0.1))])),
                leaf(task("w", "WaitUntilOnGround_BTD_C", &[])),
                leaf(task("h", "T_Hang", &[])),
            ],
            &[],
            &[],
        ),
        150,
        Some(&mut |_, _, w, me, _| {
            if !crate::fighter::grounded(w, me) {
                air += 1;
            }
        }),
        7,
        1.0,
        true,
    );
    let h = first_h(&out.ev).unwrap_or(-1);
    let got = format!(
        "{}, {}",
        if air > 10 { "jumped" } else { "no jump" },
        if h > air { "continued after landing".to_owned() } else { format!("h at {h}") }
    );
    r.check("short jump, WaitUntilOnGround", &got, "jumped, continued after landing");
    r.settle();
    // The stick stays away from the foe until the landing. With the air attack held the dash jump carries the
    // fighter off TrainingRoomSmall, so the distance is taken while it is alive.
    r.w.fighter_mut(me).body.x = -200.0;
    let (mut air, mut far, mut ko) = (0, 0.0, false);
    let gap0 = (r.x(me) - r.x(foe)).abs();
    let out = r.run(
        then("BotEvadeJumpFlee_BTD_C", &[("EnemyActor", sel("EnemyActor")), ("NeutralAttackInAir", Value::Bool(true))]),
        300,
        Some(&mut |k, bot, w, me, foe| {
            set_foe(k, bot, w, me, foe);
            if !crate::fighter::grounded(w, me) {
                air += 1;
            }
            if w.fighter(me).life.dead > 0.0 {
                ko = true;
            } else if !ko {
                far = crate::math::max(far, (w.fighter(me).body.x - w.fighter(foe).body.x).abs());
            }
        }),
        7,
        1.0,
        true,
    );
    let got = format!(
        "{}, {}, {}, {}",
        if air > 10 { "jumped" } else { "no jump" },
        if out.pad.contains("A4") { "attacked" } else { "no attack" },
        if out.ev.contains(":h") { "finished" } else { "running" },
        if far > gap0 { "further away" } else { "not further" },
    );
    r.check("BotEvadeJumpFlee", &got, "jumped, attacked, finished, further away");
    r.settle();
    let out = r.run(
        then(
            "BotAttackWithAbort_BTT_C",
            &[
                ("InputDirection", num(4.0)),
                ("ChargeTime", num(4.0)),
                ("ChargeTimeRandomDeviation", num(1.0)),
                ("Enemy", sel("EnemyActor")),
                ("AbortDistance", num(300.0)),
            ],
        ),
        330,
        Some(&mut set_foe),
        7,
        1.0,
        true,
    );
    let h = first_h(&out.ev).unwrap_or(-1);
    let got = if (6..=306).contains(&h) { "released within the charge time".to_owned() } else { format!("h at {h}") };
    r.check("BotAttackWithAbort releases", &got, "released within the charge time");
    r.settle();
    let _ = dir;
    assert!(r.fails.is_empty(), "{}", r.fails.join("\n"));
}

/// One task's `run` on a bot that is not thinking: its result and the Waypoint key.
fn direct(r: &mut Rig, bot: &mut BtBot, class: &str, over: &[(&str, Value)], key: &str) -> String {
    let me = r.me();
    let data = bot.data.clone();
    let p: Props = props::instance_props(data.defaults.get(class), Some(&props_of(over)));
    let mut t = TaskSt::of(class).expect("a ported class");
    let step = {
        let mut cx = Cx { w: &mut r.w, me, bot, props: &p, stale: false };
        t.run(&mut cx, Resume::Execute)
    };
    let k = bot.bb.key(key);
    let v = match bot.bb.vec(k) {
        Some(v) => format!("{},{},{}", v.x, v.y, v.z),
        None => "unset".to_owned(),
    };
    let res = match step {
        Step::Finish(b) => b.to_string(),
        other => format!("{other:?}"),
    };
    format!("{res} {v}")
}

#[test]
#[allow(clippy::too_many_lines, reason = "the checks in their order, one world")]
fn move_to_and_the_waypoint_tasks() {
    let Some(w) = world() else { return };
    let mut r = Rig { w, st: TestState::default(), fails: Vec::new() };
    r.settle();
    let me = r.me();
    let foe = r.foe();
    let t_key = || sel("TempPoint");
    let pt = |y: Real, z: Real| {
        move |k: i32, bot: &mut BtBot, w: &mut World, me: FighterId, _foe: FighterId| {
            if k == 0 {
                set_vec(bot, w, me, "TempPoint", vec3(0.0, y, z));
            }
        }
    };
    let my_y = r.w.fighter(me).body.y;
    let mx = r.x(me);

    let got = r.go(or_else("BotMoveTo_BTT_C", &[("Target", sel("EnemyActor"))]), 4, Some(&mut set_foe)).pad;
    r.check("BotMoveTo, one pass toward the enemy: fails with the stick set", &got, "-1*4");
    let got = r
        .go(
            or_else("BotMoveTo_BTT_C", &[("Target", sel("EnemyActor")), ("MoveAwayFrom", Value::Bool(true))]),
            4,
            Some(&mut set_foe),
        )
        .pad;
    r.check("BotMoveTo away from the enemy", &got, "1*4");
    let out = r.go(
        then("BotMoveTo_BTT_C", &[("Target", t_key()), ("Radius", num(200.0))]),
        4,
        Some(&mut pt(-mx, my_y - 142.5)),
    );
    let got = format!("{}, pad {}", if out.ev.contains("1:h") { "next task" } else { out.ev.as_str() }, out.pad);
    r.check("BotMoveTo at the point: succeeds", &got, "next task, pad 0*4");
    // Steep target (ratio above 0.7): from the ground only Medium and Hard jump. Straight above (x / 0 = 0): no jump.
    let steep = |r: &mut Rig, difficulty: Real, y: Real| {
        r.run(or_else("BotMoveTo_BTT_C", &[("Target", t_key())]), 22, Some(&mut pt(y, 1000.0)), 7, difficulty, false)
            .pad
    };
    let got =
        format!("{} | {} | {}", steep(&mut r, 1.0, -900.0), steep(&mut r, 2.0, -900.0), steep(&mut r, 2.0, -800.0));
    r.check("BotMoveTo jump at a steep target", &got, "1*22 | 1J*17 1*5 | 0*22");
    // 500 / s toward the stage edge 100 ahead: no ground under the point 200 ahead.
    let mut ledge = Vec::new();
    for p in [vec![], vec![("JumpAtLedge", Value::Bool(false))]] {
        r.w.fighter_mut(me).body.x = 1900.0;
        r.w.fighter_mut(me).body.vx = 500.0;
        let mut props = vec![("Target", t_key())];
        props.extend(p);
        ledge.push(r.go(or_else("BotMoveTo_BTT_C", &props), 22, Some(&mut pt(-3000.0, my_y - 142.5))).pad);
        r.settle();
    }
    r.check("BotMoveTo jumps off a ledge", &ledge.join(" | "), "1J*17 1*5 | 1*22");
    // 870 from the side blast bound in the travel direction counts as reached.
    r.w.fighter_mut(me).body.x = 3600.0;
    let out = r.go(then("BotMoveTo_BTT_C", &[("Target", t_key())]), 3, Some(&mut pt(-4400.0, my_y - 142.5)));
    r.settle();
    r.check(
        "BotMoveTo stops 1000 before the side blast bound",
        if out.ev.contains("1:h") { "reached" } else { out.ev.as_str() },
        "reached",
    );
    let out = r.run(
        then("BotMoveTo_BTT_C", &[("Target", t_key()), ("Radius", num(300.0)), ("Blocking", Value::Bool(true))]),
        90,
        Some(&mut pt(-1300.0, 150.0)),
        7,
        1.0,
        true,
    );
    let h = first_h(&out.ev).unwrap_or(-1);
    let x = r.x(me);
    let got = format!(
        "{}, {}",
        if h > 0 { "reached" } else { "not reached" },
        if x > 1000.0 && x < 1600.0 { "closer".to_owned() } else { format!("x {x:.0}") }
    );
    r.check("BotMoveTo blocking: walks until inside the radius", &got, "reached, closer");
    r.settle();
    let out = r.go(
        or_else(
            "BotMoveTo_BTT_C",
            &[("Target", t_key()), ("Blocking", Value::Bool(true)), ("BlockingDuration", num(0.5))],
        ),
        40,
        Some(&mut pt(1000.0, 150.0)),
    );
    r.check(
        "BotMoveTo blocking: time out after three passes, the stick released",
        &format!("{} {}", first_h(&out.ev).unwrap_or(-1), out.pad),
        "24 -1*23 0*17",
    );

    // The waypoint tasks, called directly on a bot that is not thinking.
    let mk_bot = |r: &Rig| {
        let mut bot = BtBot::new(7, r.w.data.bot_trees.clone().into(), 1.0, None, None);
        let (me, foe) = (r.me(), r.foe());
        let mut w2 = r.w.clone();
        set_obj(&mut bot, &mut w2, me, "SelfActor", me);
        set_obj(&mut bot, &mut w2, me, "EnemyActor", foe);
        bot
    };
    let mut bot = mk_bot(&r);
    let (e, wp) = (sel("EnemyActor"), sel("Waypoint"));
    let a = direct(
        &mut r,
        &mut bot,
        "FindClosestWaypointToActor_BTT_C",
        &[("Actor", e.clone()), ("Waypoint", wp.clone())],
        "Waypoint",
    );
    let b = direct(
        &mut r,
        &mut bot,
        "FindClosestWaypointToActor_BTT_C",
        &[("Actor", e.clone()), ("Waypoint", wp.clone()), ("EdgeOnly", Value::Bool(true))],
        "Waypoint",
    );
    r.check("FindClosestWaypointToActor", &format!("{a} | {b}"), "true 0,1000,150 | true 10,1770,150");
    let furthest = [("Actor", e.clone()), ("Waypoint", wp.clone()), ("PreventCrossingActor", Value::Bool(true))];
    let got = direct(&mut r, &mut bot, "FindFurthestWaypointFromActor_BTT_C", &furthest, "Waypoint");
    r.check("FindFurthestWaypointFromActor, not across the enemy", &got, "true 20,-1920,150");
    r.w.fighter_mut(me).body.x = 1500.0;
    r.w.fighter_mut(foe).body.x = 1000.0;
    let got = direct(&mut r, &mut bot, "FindFurthestWaypointFromActor_BTT_C", &furthest, "Waypoint");
    r.check("FindFurthestWaypointFromActor: the enemy between", &got, "true 20,-1920,150");
    r.settle();
    let got = direct(&mut r, &mut bot, "SetWaypointForRecoveryOnly_BTT_C", &[("WaypointBB", wp.clone())], "Waypoint");
    r.check("SetWaypointForRecoveryOnly: player 2", &got, "true 20,-600,130");
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..200 {
        seen.insert(direct(&mut r, &mut bot, "FindRandomWaypoint_BTT_C", &[("Waypoint", wp.clone())], "Waypoint"));
    }
    r.check(
        "FindRandomWaypoint: the nine waypoints and the world origin",
        &format!("{} {}", seen.len(), seen.contains("true 0,0,0")),
        "10 true",
    );
    let mut w2 = r.w.clone();
    set_obj(&mut bot, &mut w2, me, "Safe", foe);
    set_obj(&mut bot, &mut w2, me, "EnemyActor", me);
    let safe_props = [("EnemyActor", e.clone()), ("Safe", sel("Safe")), ("TargetPoint", t_key())];
    let near = direct(&mut r, &mut bot, "GetSafeAttackPoint_BTT_C", &safe_props, "TempPoint");
    r.w.fighter_mut(foe).body.x = -1900.0;
    let far = direct(&mut r, &mut bot, "GetSafeAttackPoint_BTT_C", &safe_props, "TempPoint");
    let fy = r.w.fighter(foe).body.y;
    r.check(
        "GetSafeAttackPoint: 350 beyond the safe, or the safe at the edge",
        &format!("{near} | {far}"),
        &format!("true 0,1150,{fy} | true 0,1900,{fy}"),
    );
    r.settle();
    let out = r.go(
        or_else(
            "BotPathToPoint_BTT_C",
            &[("TargetLocation", wp), ("Radius", num(350.0)), ("RecoveryPoint", sel("RecoveryPoint"))],
        ),
        10,
        Some(&mut |k, bot, w, me, _| {
            if k == 0 {
                set_vec(bot, w, me, "Waypoint", vec3(20.0, 600.0, 130.0));
            }
        }),
    );
    let k = out.bot.bb.key("RecoveryPoint");
    let p = out.bot.bb.vec(k).map_or("unset".to_owned(), |v| v.y.to_string());
    r.check(
        "BotPathToPoint: no navmesh, fails; the tick copies the target",
        &format!("{} {p}", first_h(&out.ev).unwrap_or(-1)),
        "6 600",
    );
    let _ = (BbType::Float, &Real::NAN);
    assert!(r.fails.is_empty(), "{}", r.fails.join("\n"));
}
