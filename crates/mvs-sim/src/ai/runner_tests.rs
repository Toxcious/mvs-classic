//! The tree runner's checks (its synthetic trees and the pad): the engine
//! rules `runner.rs` ports, on trees made of the test's own node classes (entered in `classes.rs` for tests only).
//! The expected lines are the ones the oracle's test gave.

use crate::Real;
use crate::actors::ActorRef;
use crate::ai::blackboard::BbVal;
use crate::ai::bt::BtBot;
use crate::ai::cx::{Cx, Dec, Resume, Step, Svc, Task};
use crate::ai::pad::{Btn, Pad};
use crate::ai::props;
use crate::fighter::FighterId;
use crate::input::{Dir, InputState};
use crate::name;
use crate::setup::{MatchPlayer, MatchSetup};
use crate::world::World;
use indexmap::IndexMap;
use mvs_data::BotTrees;
use mvs_data::tables::{BtAuxDef, BtChildDef, BtNodeDef, BtTree};
use mvs_data::value::{Props, Value};
use std::sync::Arc;

/// The test's flags and counts: kept in the bot while it runs (`BtBot.test`), in the [`Bench`] between runs.
#[derive(Clone, Debug, Default)]
pub struct TestState {
    /// The flags D_Flag reads and T_FlagOff / T_Blink write.
    flags: IndexMap<String, bool>,
    /// D_Flag's evaluations per flag.
    evals: IndexMap<String, u32>,
    /// The thinks S_Count ticked in.
    svc_ticks: Vec<i32>,
    think: i32,
}

impl TestState {
    pub(super) fn set(&mut self, f: &str, v: bool) {
        self.flags.insert(f.into(), v);
    }

    pub(super) fn svc_ticks_clear(&mut self) {
        self.svc_ticks.clear();
    }

    pub(super) fn set_think(&mut self, k: i32) {
        self.think = k;
    }

    fn evals(&self, f: &str) -> u32 {
        self.evals.get(f).copied().unwrap_or(0)
    }
}

/// A world of two fighters and the test state between runs.
struct Bench {
    w: World,
    st: TestState,
}

/// T_Ok: succeeds at once.
#[derive(Clone, Debug, Default)]
pub struct TOk;

impl Task for TOk {
    fn run(&mut self, _cx: &mut Cx, _r: Resume) -> Step {
        Step::Finish(true)
    }
}

/// T_Fail: fails at once.
#[derive(Clone, Debug, Default)]
pub struct TFail;

impl Task for TFail {
    fn run(&mut self, _cx: &mut Cx, _r: Resume) -> Step {
        Step::Finish(false)
    }
}

/// T_Hang: never ends.
#[derive(Clone, Debug, Default)]
pub struct THang;

impl Task for THang {
    fn run(&mut self, _cx: &mut Cx, _r: Resume) -> Step {
        Step::Hang
    }
}

/// T_Delay: a Delay of `s` seconds, then success.
#[derive(Clone, Debug, Default)]
pub struct TDelay;

impl Task for TDelay {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        match r {
            Resume::Execute => Step::Delay { secs: props::num(cx.props, "s").unwrap_or(0.0), node: 0 },
            Resume::Delay => Step::Finish(true),
        }
    }
}

/// T_Two: two Delays, of one Delay node (`same`) or of two.
#[derive(Clone, Debug, Default)]
pub struct TTwo {
    pc: u8,
}

impl Task for TTwo {
    fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
        if r == Resume::Execute {
            self.pc = 0;
        }
        self.pc += 1;
        match self.pc {
            1 => Step::Delay { secs: 0.05, node: 0 },
            2 => Step::Delay { secs: 0.2, node: if props::truthy(cx.props, "same") { 0 } else { 1 } },
            _ => Step::Finish(true),
        }
    }
}

/// T_FlagOff: sets a flag to false and never ends.
#[derive(Clone, Debug, Default)]
pub struct TFlagOff;

impl Task for TFlagOff {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        cx.bot.test.set(props::str(cx.props, "flag").unwrap_or(""), false);
        Step::Hang
    }
}

/// T_Blink: clears a blackboard key and writes it back (two changes), then never ends; its second execution drops a
/// flag.
#[derive(Clone, Debug, Default)]
pub struct TBlink {
    n: u32,
}

impl Task for TBlink {
    fn run(&mut self, cx: &mut Cx, _r: Resume) -> Step {
        let k = cx.bot.bb.key(props::str(cx.props, "key").unwrap_or(""));
        let v = cx.bot.bb.get(k);
        cx.clear(k);
        cx.set(k, v);
        self.n += 1;
        if self.n == 2 {
            cx.bot.test.set(props::str(cx.props, "off").unwrap_or(""), false);
        }
        Step::Hang
    }
}

/// D_Flag: the flag `flag` is set (counted per flag).
#[derive(Clone, Debug, Default)]
pub struct DFlag;

impl Dec for DFlag {
    fn test(&mut self, cx: &mut Cx) -> bool {
        let f = props::str(cx.props, "flag").unwrap_or("");
        *cx.bot.test.evals.entry(f.into()).or_insert(0) += 1;
        cx.bot.test.flags.get(f).copied().unwrap_or(false)
    }
}

/// S_Count: notes the think it ticks in.
#[derive(Clone, Debug, Default)]
pub struct SCount;

impl Svc for SCount {
    fn tick(&mut self, cx: &mut Cx, _dt: Real) {
        let t = &mut cx.bot.test;
        t.svc_ticks.push(t.think);
    }
}

// --- the trees ---

pub(super) fn num(v: Real) -> Value {
    Value::Num(f64::from(v))
}

pub(super) fn s(v: &str) -> Value {
    Value::Str(v.into())
}

pub(super) fn sel(key: &str) -> Value {
    Value::Obj(props_of(&[("key", s(key))]))
}

pub(super) fn props_of(kv: &[(&str, Value)]) -> Props {
    kv.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
}

pub(super) fn task(id: &str, ty: &str, p: &[(&str, Value)]) -> BtNodeDef {
    BtNodeDef { id: id.into(), ty: ty.into(), props: props_of(p), services: None, children: None }
}

pub(super) fn ok(id: &str) -> BtNodeDef {
    task(id, "T_Ok", &[])
}

pub(super) fn comp(kind: &str, children: Vec<BtChildDef>, p: &[(&str, Value)], services: &[Props]) -> BtNodeDef {
    BtNodeDef {
        id: kind.into(),
        ty: format!("BTComposite_{kind}"),
        props: props_of(p),
        services: (!services.is_empty()).then(|| {
            services
                .iter()
                .enumerate()
                .map(|(i, p)| BtAuxDef {
                    id: format!("svc{i}"),
                    ty: p["type"].as_str().expect("a service's type").into(),
                    props: p.clone(),
                })
                .collect()
        }),
        children: Some(children),
    }
}

/// A child link: its decorators as (type, props).
pub(super) fn ch(node: BtNodeDef, decs: Vec<(&str, Props)>, ops: Option<Vec<(&str, Real)>>) -> BtChildDef {
    let decorators = decs
        .into_iter()
        .enumerate()
        .map(|(i, (ty, props))| BtAuxDef { id: format!("{}.d{i}", node.id), ty: ty.into(), props })
        .collect();
    BtChildDef {
        node,
        decorators: Some(decorators),
        ops: ops.map(|o| o.into_iter().map(|(op, n)| (op.to_owned(), f64::from(n))).collect()),
    }
}

pub(super) fn leaf(node: BtNodeDef) -> BtChildDef {
    ch(node, Vec::new(), None)
}

pub(super) fn flag(f: &str, mode: Option<&str>, inv: bool) -> (&'static str, Props) {
    let mut p = props_of(&[("flag", s(f))]);
    if let Some(m) = mode {
        p.insert("FlowAbortMode".into(), s(&format!("EBTFlowAbortMode::{m}")));
    }
    if inv {
        p.insert("bInverseCondition".into(), Value::Bool(true));
    }
    ("D_Flag", p)
}

fn bbdec(p: &[(&str, Value)]) -> (&'static str, Props) {
    ("BTDecorator_Blackboard", props_of(p))
}

/// A world of two fighters and the build's bot data; `None` without a dev store.
pub(super) fn world() -> Option<World> {
    let dir = mvs_store::dev_store_dir();
    let Ok(store) = mvs_store::Store::open(&dir) else {
        eprintln!("no store at {}: skipped", dir.display());
        return None;
    };
    let data =
        Arc::new(mvs_data::GameData::load(&store, &["shaggy"], "trainingroomsmall").expect("the gamedata loads"));
    let p = || MatchPlayer { character: Some("shaggy".into()), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![p(), p()], ..MatchSetup::default() };
    Some(World::new(data, 5, &setup).expect("binds"))
}

type At<'a> = &'a mut dyn FnMut(i32, &mut BtBot, &mut World, FighterId, FighterId);

/// `runTree`: runs `root` for `n` thinks (`at(k, bot)` before think k) and gives the runner's events as
/// "think:event" (an execution by its node's name, END for the tree's end). `me` is fighter 2.
fn run_tree(b: &mut Bench, root: BtNodeDef, n: i32, at: Option<At>, extra: Vec<(&str, BtNodeDef)>) -> String {
    let w = &mut b.w;
    let real = &w.data.bot_trees;
    let mut trees = mvs_data::fighter::Map::new();
    trees.insert("Base_BehaviorTree".to_owned(), BtTree { root });
    for (k, r) in extra {
        trees.insert(k.to_owned(), BtTree { root: r });
    }
    let mut inject = mvs_data::fighter::Map::new();
    inject.insert("shaggy".to_owned(), mvs_data::fighter::Map::new());
    let data = BotTrees { trees, inject, ..real.clone() };
    let mut bot = BtBot::new(7, Arc::new(data), 1.0, Some(name!("Base_BehaviorTree")), None);
    bot.t.log = Some(Vec::new());
    let (me, foe) = (w.players[1], w.players[0]);
    bot.test = std::mem::take(&mut b.st);
    bot.test.svc_ticks.clear();
    let mut ev = Vec::new();
    let mut at = at;
    for k in 0..n {
        bot.test.think = k;
        if let Some(at) = at.as_mut() {
            at(k, &mut bot, w, me, foe);
        }
        bot.think(w, me);
        for e in bot.t.log.as_mut().expect("the log").drain(..) {
            let e = e.strip_prefix("exec ").unwrap_or(&e).replace("tree finished", "END");
            ev.push(format!("{k}:{e}"));
        }
    }
    b.st = std::mem::take(&mut bot.test);
    ev.join(" ")
}

fn set_key(bot: &mut BtBot, w: &mut World, me: FighterId, key: &str, v: BbVal) {
    let k = bot.bb.key(key);
    assert!(k.is_some(), "the blackboard has {key}");
    bot.set_bb(w, me, k, v);
}

fn clear_key(bot: &mut BtBot, w: &mut World, me: FighterId, key: &str) {
    let k = bot.bb.key(key);
    let t = bot.bb.ty(k).expect("a key of the blackboard");
    bot.set_bb(w, me, k, BbVal::initial(t));
}

#[track_caller]
fn check(fails: &mut Vec<String>, name: &str, got: &str, want: &str) {
    if got != want {
        fails.push(format!("{name}: {got} (expect {want})"));
    }
}

#[test]
#[allow(clippy::too_many_lines, reason = "the checks in their order, one world")]
fn synthetic_trees() {
    let Some(w) = world() else { return };
    let mut bench = Bench { w, st: TestState::default() };
    let w = &mut bench;
    let mut fails = Vec::new();
    let f = &mut fails;
    let hang = |id: &str| task(id, "T_Hang", &[]);
    let wait = |id: &str, t: Real| task(id, "BTTask_Wait", &[("WaitTime", num(t))]);

    // One task execution per tick; BTTask_Wait counts down from its execution tick; the tree loops.
    let r = run_tree(
        w,
        comp("Sequence", vec![leaf(ok("a")), leaf(wait("w", 0.1)), leaf(ok("b"))], &[], &[]),
        10,
        None,
        vec![],
    );
    check(f, "sequence + wait", &r, "0:a 1:w 7:b 8:END 9:a");
    // Blueprint Delay: fires at the end of the 12th frame, the next task runs on the tick after.
    let r = run_tree(
        w,
        comp("Sequence", vec![leaf(task("dl", "T_Delay", &[("s", num(0.2))])), leaf(ok("b"))], &[], &[]),
        14,
        None,
        vec![],
    );
    check(f, "delay task", &r, "0:dl 12:b 13:END");

    // Self abort: the branch fails when the condition drops, the selector moves on.
    w.st.set("f", true);
    let r = run_tree(
        w,
        comp("Selector", vec![ch(hang("h"), vec![flag("f", Some("Self"), false)], None), leaf(ok("c"))], &[], &[]),
        6,
        Some(&mut |k, bot, _, _, _| {
            if k == 3 {
                bot.test.set("f", false);
            }
        }),
        vec![],
    );
    check(f, "self abort", &r, "0:h 3:abort h 3:c 4:END 5:c");
    // Both: lower priority abort when the condition passes, self abort when it drops again.
    w.st.set("g", false);
    let r = run_tree(
        w,
        comp("Selector", vec![ch(hang("hi"), vec![flag("g", Some("Both"), false)], None), leaf(hang("lo"))], &[], &[]),
        8,
        Some(&mut |k, bot, _, _, _| {
            if k == 4 {
                bot.test.set("g", true);
            }
            if k == 6 {
                bot.test.set("g", false);
            }
        }),
        vec![],
    );
    check(f, "both abort", &r, "0:lo 4:abort lo 4:hi 6:abort hi 6:lo");
    // A restart that finds no task inside its branch is dropped: the running task is not aborted.
    w.st.set("g", true);
    w.st.set("x", false);
    let r = run_tree(
        w,
        comp(
            "Selector",
            vec![
                ch(
                    comp("Sequence", vec![ch(hang("hi"), vec![flag("x", None, false)], None)], &[], &[]),
                    vec![flag("g", Some("Both"), false)],
                    None,
                ),
                leaf(hang("lo")),
            ],
            &[],
            &[],
        ),
        6,
        Some(&mut |k, bot, _, _, _| {
            if k == 4 {
                bot.test.set("x", true);
            }
        }),
        vec![],
    );
    check(f, "restart outside its range is dropped", &r, "0:lo 4:abort lo 4:hi");
    // Both on a Blueprint decorator whose branch is entered from outside (ApplySearchData 143209260 ticks the added
    // decorator while the old task is still the active node): the branch is restarted on the next tick. Not with
    // Self (ticked, but only LowerPriority restarts) or LowerPriority (not an observer while its branch runs).
    w.st.set("g", true);
    let enter = |w: &mut Bench, mode: &str| {
        run_tree(
            w,
            comp(
                "Selector",
                vec![ch(hang("hi"), vec![flag("g", Some(mode), false)], None), leaf(hang("lo"))],
                &[],
                &[],
            ),
            6,
            None,
            vec![],
        )
    };
    let r = format!("{} | {} | {}", enter(w, "Both"), enter(w, "Self"), enter(w, "LowerPriority"));
    check(f, "both: a branch entered from outside restarts once", &r, "0:hi 1:abort hi 1:hi | 0:hi | 0:hi");
    // A task that ends at once runs twice: its continue request loses against the restart.
    let r = run_tree(
        w,
        comp(
            "Sequence",
            vec![
                leaf(ok("a")),
                ch(ok("b"), vec![flag("g", Some("Both"), false)], None),
                leaf(task("c", "T_FlagOff", &[("flag", s("g"))])),
            ],
            &[],
            &[],
        ),
        7,
        None,
        vec![],
    );
    check(f, "both: a task that ends at once runs twice", &r, "0:a 1:b 2:b 3:c");
    // Entered again while it is the running branch: no further restart (p ends at once, so it runs twice).
    w.st.set("g", true);
    let r = run_tree(
        w,
        comp(
            "Selector",
            vec![
                ch(
                    comp("Sequence", vec![leaf(ok("p")), leaf(hang("q"))], &[], &[]),
                    vec![flag("g", Some("Both"), false)],
                    None,
                ),
                leaf(hang("lo")),
            ],
            &[],
            &[],
        ),
        6,
        None,
        vec![],
    );
    check(f, "both: no second restart from inside", &r, "0:p 1:p 2:q");

    // Request order (RequestExecution 14321bdd0): a restart's place is the first node of its link, a failing
    // decorator's is its own, so the second decorator's failure does not replace the first one's pending restart.
    w.st.set("g", true);
    let r = run_tree(
        w,
        comp(
            "Selector",
            vec![
                ch(
                    task("t", "T_Blink", &[("key", s("Item")), ("off", s("g"))]),
                    vec![
                        flag("g", Some("Both"), false),
                        bbdec(&[("BlackboardKey", sel("Item")), ("FlowAbortMode", s("EBTFlowAbortMode::Self"))]),
                    ],
                    None,
                ),
                leaf(hang("lo")),
            ],
            &[],
            &[],
        ),
        6,
        Some(&mut |k, bot, w, me, _| {
            if k == 0 {
                set_key(bot, w, me, "Item", BbVal::Obj(ActorRef::Fighter(me)));
            }
        }),
        vec![],
    );
    check(f, "request order: decorators of one link", &r, "0:t 1:abort t 1:t 2:abort t 2:lo");

    // ConditionalFlowAbort 14320b970: a key change that leaves a waiting Blackboard observer's branch closed
    // unregisters the aux nodes inside that branch at the start of the next tick.
    w.st.set("x", false);
    {
        let mut n = [0_u32; 9];
        let r = run_tree(
            w,
            comp(
                "Selector",
                vec![
                    ch(
                        comp("Sequence", vec![ch(hang("hi"), vec![flag("x", Some("Both"), false)], None)], &[], &[]),
                        vec![bbdec(&[
                            ("BlackboardKey", sel("MovementMode")),
                            ("IntValue", num(2.0)),
                            ("FlowAbortMode", s("EBTFlowAbortMode::LowerPriority")),
                        ])],
                        None,
                    ),
                    leaf(hang("lo")),
                ],
                &[],
                &[],
            ),
            9,
            Some(&mut |k, bot, w, me, _| {
                n[usize::try_from(k).expect("k >= 0")] = bot.test.evals("x");
                if k == 0 {
                    set_key(bot, w, me, "MovementMode", BbVal::Num(2.0));
                }
                if k == 3 {
                    set_key(bot, w, me, "MovementMode", BbVal::Num(1.0));
                }
                if k == 5 {
                    bot.test.set("x", true);
                }
                if k == 7 {
                    set_key(bot, w, me, "MovementMode", BbVal::Num(2.0));
                }
            }),
            vec![],
        );
        let got = format!("{r} | x evaluated {} times in 2 ticks, then {} in 4", n[3] - n[1], n[7] - n[3]);
        check(
            f,
            "pending unregister of a closed branch",
            &got,
            "0:lo 7:abort lo 7:hi 8:abort hi 8:hi | x evaluated 2 times in 2 ticks, then 0 in 4",
        );
    }

    // A task restarted in the call that aborts it: the old Delay is still in the latent list, so the first Delay of
    // the new execution is not started when it is the same node (the task hangs) and starts when it is another one.
    let item_restart = || {
        bbdec(&[
            ("BlackboardKey", sel("Item")),
            ("FlowAbortMode", s("EBTFlowAbortMode::Self")),
            ("NotifyObserver", s("EBTBlackboardRestart::ValueChange")),
        ])
    };
    let mut blink = |k: i32, bot: &mut BtBot, w: &mut World, me: FighterId, foe: FighterId| {
        if k == 0 {
            set_key(bot, w, me, "Item", BbVal::Obj(ActorRef::Fighter(me)));
        }
        if k == 6 {
            set_key(bot, w, me, "Item", BbVal::Obj(ActorRef::Fighter(foe)));
        }
    };
    let mut two = |w: &mut Bench, same: bool| {
        run_tree(
            w,
            comp(
                "Sequence",
                vec![
                    ch(task("t", "T_Two", &[("same", Value::Bool(same))]), vec![item_restart()], None),
                    leaf(hang("b")),
                ],
                &[],
                &[],
            ),
            26,
            Some(&mut blink),
            vec![],
        )
    };
    let r = format!("{} | {}", two(w, false), two(w, true));
    check(f, "restart in a Delay: per Delay node", &r, "0:t 6:abort t 6:t 21:b | 0:t 6:abort t 6:t");
    // The same for the main task of a SimpleParallel that the restart re-enters.
    let mut par = |w: &mut Bench, same: bool| {
        run_tree(
            w,
            comp(
                "Sequence",
                vec![
                    ch(
                        comp(
                            "SimpleParallel",
                            vec![leaf(task("m", "T_Two", &[("same", Value::Bool(same))])), leaf(hang("g"))],
                            &[],
                            &[],
                        ),
                        vec![item_restart()],
                        None,
                    ),
                    leaf(hang("b")),
                ],
                &[],
                &[],
            ),
            26,
            Some(&mut blink),
            vec![],
        )
    };
    let r = format!("{} | {}", par(w, false), par(w, true));
    check(
        f,
        "restart in a Delay: SimpleParallel main task",
        &r,
        "0:m 1:g 6:abort g 6:abort m 6:m 7:g 21:abort g 21:b | 0:m 1:g 6:abort g 6:abort m 6:m 7:g",
    );
    // Blackboard decorator observer: aborts on the key change, without waiting for a tick of its own.
    let r = run_tree(
        w,
        comp(
            "Selector",
            vec![
                ch(
                    hang("h"),
                    vec![bbdec(&[("BlackboardKey", sel("Item")), ("FlowAbortMode", s("EBTFlowAbortMode::Self"))])],
                    None,
                ),
                leaf(hang("c")),
            ],
            &[],
            &[],
        ),
        6,
        Some(&mut |k, bot, w, me, _| {
            if k == 0 {
                set_key(bot, w, me, "Item", BbVal::Obj(ActorRef::Fighter(me)));
            }
            if k == 3 {
                clear_key(bot, w, me, "Item");
            }
        }),
        vec![],
    );
    check(f, "blackboard self abort", &r, "0:h 3:abort h 3:c");
    let r = run_tree(
        w,
        comp(
            "Selector",
            vec![
                ch(ok("m2"), vec![bbdec(&[("BlackboardKey", sel("MovementMode")), ("IntValue", num(2.0))])], None),
                leaf(ok("other")),
            ],
            &[],
            &[],
        ),
        4,
        Some(&mut |k, bot, w, me, _| {
            if k == 2 {
                set_key(bot, w, me, "MovementMode", BbVal::Num(2.0));
            }
        }),
        vec![],
    );
    check(f, "blackboard enum compare", &r, "0:other 1:END 2:m2 3:END");

    // ForceSuccess on a link whose decorators fail: the sequence continues.
    w.st.set("off", false);
    let r = run_tree(
        w,
        comp(
            "Sequence",
            vec![
                ch(ok("skipped"), vec![("BTDecorator_ForceSuccess", Props::new()), flag("off", None, false)], None),
                leaf(ok("d")),
            ],
            &[],
            &[],
        ),
        2,
        None,
        vec![],
    );
    check(f, "force success", &r, "0:d 1:END");
    // Cooldown: stamped when the branch is left.
    let r = run_tree(
        w,
        comp(
            "Selector",
            vec![
                ch(ok("e"), vec![("BTDecorator_Cooldown", props_of(&[("CoolDownTime", num(0.5))]))], None),
                leaf(wait("w", 0.1)),
            ],
            &[],
            &[],
        ),
        40,
        None,
        vec![],
    );
    let only =
        |r: &str, id: &str| r.split(' ').filter(|x| x.ends_with(&format!(":{id}"))).collect::<Vec<_>>().join(" ");
    check(f, "cooldown 0.5 s", &only(&r, "e"), "0:e 37:e");
    // Tag cooldown: SetTagCooldown task blocks the TagCooldown decorator for its duration.
    let r = run_tree(
        w,
        comp(
            "Selector",
            vec![
                ch(
                    comp(
                        "Sequence",
                        vec![
                            leaf(ok("t")),
                            leaf(task(
                                "set",
                                "BTTask_SetTagCooldown",
                                &[("CooldownTag", s("AI.X")), ("CooldownDuration", num(0.2))],
                            )),
                        ],
                        &[],
                        &[],
                    ),
                    vec![(
                        "BTDecorator_TagCooldown",
                        props_of(&[("CooldownTag", s("Ai.x")), ("bActivatesCooldown", Value::Bool(false))]),
                    )],
                    None,
                ),
                leaf(wait("w", 0.1)),
            ],
            &[],
            &[],
        ),
        20,
        None,
        vec![],
    );
    check(f, "tag cooldown", &only(&r, "t"), "0:t 17:t");
    // Decorator logic: And(Test0, Or(Test1, Test2)).
    {
        let ops = || vec![("And", 2.0), ("Test", 0.0), ("Or", 2.0), ("Test", 1.0), ("Test", 2.0)];
        let mut out = Vec::new();
        for [a, b, c] in [[true, false, true], [true, false, false], [false, true, true], [true, true, false]] {
            w.st.set("a", a);
            w.st.set("b", b);
            w.st.set("c", c);
            let tree = comp(
                "Selector",
                vec![
                    ch(
                        ok("yes"),
                        vec![flag("a", None, false), flag("b", None, false), flag("c", None, false)],
                        Some(ops()),
                    ),
                    leaf(ok("no")),
                ],
                &[],
                &[],
            );
            out.push(run_tree(w, tree, 1, None, vec![]));
        }
        check(f, "decorator ops", &out.join(" "), "0:yes 0:no 0:no 0:yes");
    }

    // SimpleParallel: the background tree loops while the main Wait runs, then is aborted.
    let r = run_tree(
        w,
        comp(
            "Sequence",
            vec![
                leaf(comp(
                    "SimpleParallel",
                    vec![
                        leaf(wait("main", 0.2)),
                        leaf(comp("Sequence", vec![leaf(ok("p")), leaf(hang("q"))], &[], &[])),
                    ],
                    &[],
                    &[],
                )),
                leaf(ok("r")),
            ],
            &[],
            &[],
        ),
        14,
        None,
        vec![],
    );
    check(f, "simple parallel", &r, "0:main 1:p 2:q 12:abort q 12:r 13:END");
    w.st.set("bg", false);
    let r = run_tree(
        w,
        comp(
            "Sequence",
            vec![
                leaf(comp(
                    "SimpleParallel",
                    vec![leaf(wait("main", 0.1)), ch(ok("p"), vec![flag("bg", None, false)], None)],
                    &[],
                    &[],
                )),
                leaf(ok("r")),
            ],
            &[],
            &[],
        ),
        8,
        None,
        vec![],
    );
    check(f, "simple parallel, background blocked", &r, "0:main 6:r 7:END");

    // Subtrees: the first search of a pushed subtree runs a tick after RunBehavior; a missing injection fails.
    let sub = || comp("Sequence", vec![leaf(ok("s1"))], &[], &[]);
    let r = run_tree(
        w,
        comp(
            "Sequence",
            vec![leaf(task("rb", "BTTask_RunBehavior", &[("BehaviorAsset", s("Sub"))])), leaf(ok("z"))],
            &[],
            &[],
        ),
        4,
        None,
        vec![("Sub", sub())],
    );
    check(f, "run behavior", &r, "0:rb 1:s1 2:z 3:END");
    let r = run_tree(
        w,
        comp(
            "Selector",
            vec![leaf(task("rbd", "BTTask_RunBehaviorDynamic", &[("InjectionTag", s("Ai.Nothing"))])), leaf(ok("y"))],
            &[],
            &[],
        ),
        2,
        None,
        vec![],
    );
    check(f, "run behavior dynamic, no injection", &r, "0:rbd 1:y");
    // Self abort from inside a subtree leaves the instance.
    w.st.set("f", true);
    let r = run_tree(
        w,
        comp(
            "Selector",
            vec![
                ch(
                    task("rb", "BTTask_RunBehavior", &[("BehaviorAsset", s("Sub"))]),
                    vec![flag("f", Some("Self"), false)],
                    None,
                ),
                leaf(hang("c")),
            ],
            &[],
            &[],
        ),
        6,
        Some(&mut |k, bot, _, _, _| {
            if k == 3 {
                bot.test.set("f", false);
            }
        }),
        vec![("Sub", comp("Sequence", vec![leaf(hang("s1"))], &[], &[]))],
    );
    check(f, "abort out of a subtree", &r, "0:rb 1:s1 3:abort s1 3:c");

    // Services: first tick one interval after activation, then every interval.
    run_tree(
        w,
        comp(
            "Selector",
            vec![leaf(hang("h"))],
            &[],
            &[props_of(&[("type", s("S_Count")), ("Interval", num(0.1)), ("RandomDeviation", num(0.0))])],
        ),
        20,
        None,
        vec![],
    );
    let ticks = w.st.svc_ticks.iter().map(i32::to_string).collect::<Vec<_>>().join(" ");
    check(f, "service ticks", &ticks, "5 11 17");

    // Chooser (weights 1:5; the OverrideChild overlap then runs child a once more: share (2/6 + 5/6) / 2 = 7/12) and
    // RandomSelector (every child tried once per activation).
    {
        let r = run_tree(
            w,
            comp(
                "Chooser",
                vec![leaf(ok("a")), leaf(ok("b"))],
                &[("ChoiceWeights", Value::Arr(vec![num(1.0), num(5.0)]))],
                &[],
            ),
            2400,
            None,
            vec![],
        );
        let na = r.split(' ').filter(|x| x.ends_with(":a")).count();
        let nb = r.split(' ').filter(|x| x.ends_with(":b")).count();
        #[allow(clippy::cast_precision_loss, reason = "counts of a few thousand")]
        let share = na as Real / (na + nb) as Real;
        let near = if share > 0.55 && share < 0.62 { "near 7/12".to_owned() } else { format!("{share:.3}") };
        check(f, "chooser 1:5", &format!("{} picks, a share {near}", na + nb), "1600 picks, a share near 7/12");
        // Succeeded returns at once, also on activation after a succeeded sibling.
        let r = run_tree(
            w,
            comp(
                "Sequence",
                vec![leaf(ok("s")), leaf(comp("RandomSelector", vec![leaf(ok("r"))], &[], &[])), leaf(ok("t"))],
                &[],
                &[],
            ),
            4,
            None,
            vec![],
        );
        check(f, "random selector after a success", &r, "0:s 1:t 2:END 3:s");
        let fails3 = ["f1", "f2", "f3"].iter().map(|n| leaf(task(n, "T_Fail", &[]))).collect();
        let r = run_tree(w, comp("RandomSelector", fails3, &[], &[]), 400, None, vec![]);
        // Rounds between the tree's ends, the think numbers dropped.
        let mut rounds: Vec<String> = Vec::new();
        let mut cur: Vec<&str> = Vec::new();
        for e in r.split(' ') {
            let name = e.split_once(':').map_or(e, |x| x.1);
            if name == "END" {
                if !cur.is_empty() {
                    rounds.push(cur.join(" "));
                }
                cur.clear();
            } else {
                cur.push(name);
            }
        }
        if !cur.is_empty() {
            rounds.push(cur.join(" "));
        }
        let full = rounds
            .iter()
            .filter(|x| {
                let mut v: Vec<&str> = x.split(' ').collect();
                v.sort_unstable();
                v.concat() == "f1f2f3"
            })
            .count();
        let mut orders = rounds.clone();
        orders.sort();
        orders.dedup();
        check(
            f,
            "random selector",
            &format!("{full} of {} rounds try every child, {} orders", rounds.len(), orders.len()),
            "100 of 100 rounds try every child, 6 orders",
        );
    }
    assert!(fails.is_empty(), "the runner's checks:\n{}", fails.join("\n"));
}

/// Pad: one-frame pulse, gap before a re-press, attack direction on the rising edge; the stick goes out as the
/// float32 of what the task wrote (no dead zone). Each write is a MoveX / MoveY call, the same value included, for
/// the next output only; a reset is none.
#[test]
fn pad_pulses_and_calls() {
    let mut pad = Pad::default();
    let mut out = Vec::new();
    let tok = |i: InputState| {
        let t = format!(
            "{}{}{}{}",
            i.x,
            if i.y > 0.0 {
                "u"
            } else if i.y < 0.0 {
                "d"
            } else {
                ""
            },
            if i.attack {
                format!("A{}", i.atk_dir.map_or(String::new(), |d| (d as u8).to_string()))
            } else {
                String::new()
            },
            if i.jump { "J" } else { "" },
        );
        if t.is_empty() { "-".to_owned() } else { t }
    };
    pad.set_x(-0.25);
    pad.press(Btn::Attack, Some(Dir::Up), false);
    pad.release(Btn::Attack);
    out.push(tok(pad.out()));
    out.push(tok(pad.out()));
    pad.set_x(1.0);
    pad.set_y(-1.0);
    pad.press(Btn::Jump, None, false);
    out.push(tok(pad.out()));
    pad.press(Btn::Jump, None, false);
    out.push(tok(pad.out()));
    out.push(tok(pad.out()));
    pad.release(Btn::Jump);
    out.push(tok(pad.out()));
    assert_eq!(out.join(" "), "-0.25A0 -0.25 1dJ 1d 1dJ 1d");
    let mut calls = Vec::new();
    let call = |i: InputState| {
        let t = format!("{}{}", if i.call_x { "X" } else { "" }, if i.call_y { "Y" } else { "" });
        if t.is_empty() { "-".to_owned() } else { t }
    };
    pad.set_x(1.0);
    calls.push(call(pad.out()));
    calls.push(call(pad.out()));
    pad.set_x(1.0);
    pad.set_y(-1.0);
    calls.push(call(pad.out()));
    pad.reset();
    calls.push(call(pad.out()));
    assert_eq!(calls.join(" "), "X - XY -");
}
