//! The tutorial's keys of the World's `rest` as the oracle writer writes them (its
//! `tutorialRest`), present only in a World with a mode: `tutorial` (the TutorialGameMode and its TutorialActor in one,
//! [`TutorialRun`]), `runtimeSolids` while there are any, `pawnCtl` (the pawns' controllers, in the map's order) and
//! `newPawns`. A tutorial object is `'mode'`, `'actor'`, `{demo: i}` (its place in the spawn list), `{minigame: i}`
//! (its place in the actor's live list, -1 once destroyed) or `{stage: i}` (a stage actor's index). A delegate's
//! binds are written one entry per bound handler, its owner alone. A demo controller's or minigame actor's own fields
//! are `payload.rs`'s.
//!
//! The loads set what the World already has a place for: a demo, a minigame or a run-time solid the World has not
//! made is a miss (their state's variant is their class's), as is a controller that differs (its hooks are the
//! controller's code).

mod geometry;
pub(crate) mod meeseeks;
mod payload;

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::match_side::TutorialEvent;
use mvs_sim::input::{AttackBtn, Btn, Dir};
use mvs_sim::modes::tutorial::classes::RunList;
use mvs_sim::modes::tutorial::demo::{Demo, DemoPad};
use mvs_sim::modes::tutorial::latent::{DelayEntry, Latent, MoveEntry, MoveTarget, TimerEntry};
use mvs_sim::modes::tutorial::minigame::Minigame;
use mvs_sim::modes::tutorial::{Timeline, TutorialActor, TutorialBind, TutorialObj, TutorialRun, TutorialScore};
use mvs_sim::setup::MatchMode;
use mvs_sim::world::World;
use mvs_sim::world::pawns::{FirstTicks, PawnCtl};
use mvs_sim::world::stage::{ComponentMove, RuntimeBox, RuntimeSolid};
use mvs_sim::{Real, names::Name};
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, as_list, as_obj, check_keys, int, num, obj, text};
use crate::cx::{Lx, Wx};

/// The World's rest keys of a tutorial (none outside one).
pub fn rest_put(w: &World, x: &Wx, o: &mut Vec<(&'static str, Value)>) {
    let Some(run) = &w.tutorial else { return };
    o.push(("tutorial", run_put(w, run, x)));
    if let Some(rs) = &w.runtime_solids {
        o.push(("runtimeSolids", Value::List(rs.iter().map(|s| solid_put(s, x)).collect())));
        if !rs.is_empty() {
            x.unrecorded("a run-time solid's spec and flags");
        }
    }
    let pc = &w.pawn_ctl;
    let ctl = pc
        .ctl
        .iter()
        .map(|(f, c)| {
            let c = match c {
                PawnCtl::Player => obj(vec![("kind", s("player"))]),
                PawnCtl::Bot(_) => {
                    x.unrecorded("a mode pawn's bot controller state");
                    obj(vec![("kind", s("bot"))])
                }
                PawnCtl::Demo(d) => obj(vec![("demo", n(demo_index(run, d.id))), ("kind", s("demo"))]),
                PawnCtl::None => obj(vec![("kind", s("none"))]),
            };
            Value::List(vec![f.to_value(x), c])
        })
        .collect();
    o.push(("pawnCtl", Value::List(ctl)));
    let born = pc
        .new_pawns
        .iter()
        .map(|p| {
            let at = match p.at {
                FirstTicks::OwnerTick => "ownerTick",
                FirstTicks::Leftover => "leftover",
            };
            obj(vec![("at", s(at)), ("m", p.m.to_value(x))])
        })
        .collect();
    o.push(("newPawns", Value::List(born)));
}

/// [`rest_put`]'s inverse.
pub fn rest_load(w: &mut World, ob: &Obj, l: &mut Lx) {
    let tut = ob.get("tutorial");
    if tut.is_some() != w.tutorial.is_some() {
        l.miss(format_args!("the tutorial in the record {}, in the World {}", tut.is_some(), w.tutorial.is_some()));
    }
    if let (Some(v), true) = (tut, w.tutorial.is_some()) {
        l.at("tutorial", |l| make_objects(w, v, l));
    }
    if let (Some(v), Some(mut run)) = (tut, w.tutorial.take()) {
        let solids = w.runtime_solids.iter().flatten().map(|s| s.spec).collect();
        l.at("tutorial", |l| run_load(&mut run, solids, v, l));
        w.tutorial = Some(run);
    }
    match (ob.get("runtimeSolids"), &mut w.runtime_solids) {
        (Some(v), Some(rs)) => l.at("runtimeSolids", |l| {
            let Some(a) = as_list(v, l) else { return };
            if a.len() != rs.len() {
                l.miss(format_args!("{} run-time solids in the record, {} in the World", a.len(), rs.len()));
            }
            for (s, v) in rs.iter_mut().zip(a) {
                solid_load(s, v, l);
            }
        }),
        (None, None) => {}
        (v, _) => l.miss(format_args!("runtimeSolids in the record {}, in the World not", v.is_some())),
    }
    if let Some(v) = ob.get("pawnCtl") {
        l.at("pawnCtl", |l| ctl_load(w, v, l));
    }
    if let Some(v) = ob.get("newPawns") {
        l.at("newPawns", |l| {
            let Some(a) = as_list(v, l) else { return };
            if a.len() != w.pawn_ctl.new_pawns.len() {
                l.miss(format_args!(
                    "{} new pawns in the record, {} in the World",
                    a.len(),
                    w.pawn_ctl.new_pawns.len()
                ));
            }
        });
    }
}

fn s(v: &str) -> Value {
    Value::Str(v.to_owned())
}
fn n(v: i32) -> Value {
    Value::Num(f64::from(v))
}
fn b(v: bool) -> Value {
    Value::Bool(v)
}

/// A name the record holds, as the sim's `&'static str` (each distinct one kept once for the run).
fn intern(v: &str) -> &'static str {
    static NAMES: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut set = NAMES.get_or_init(Mutex::default).lock().expect("the names' lock");
    if let Some(k) = set.get(v) {
        return k;
    }
    let k: &'static str = Box::leak(v.to_owned().into_boxed_str());
    set.insert(k);
    k
}

fn key<'v>(ob: &'v Obj, k: &str, l: &mut Lx) -> Option<&'v Value> {
    let v = ob.get(k);
    if v.is_none() {
        l.miss(format_args!("key {k} absent"));
    }
    v
}

fn demo_index(run: &TutorialRun, id: u32) -> i32 {
    run.demos.iter().position(|d| d.id == id).and_then(|i| i32::try_from(i).ok()).unwrap_or(-1)
}

fn minigame_index(run: &TutorialRun, id: u32) -> i32 {
    run.actor
        .as_ref()
        .and_then(|a| a.minigames.iter().position(|m| m.id == id))
        .and_then(|i| i32::try_from(i).ok())
        .unwrap_or(-1)
}

fn obj_put(run: &TutorialRun, o: TutorialObj) -> Value {
    match o {
        TutorialObj::Mode => s("mode"),
        TutorialObj::Actor => s("actor"),
        TutorialObj::Demo(id) => obj(vec![("demo", n(demo_index(run, id)))]),
        TutorialObj::Minigame(id) => obj(vec![("minigame", n(minigame_index(run, id)))]),
        TutorialObj::Stage(i) => obj(vec![("stage", n(i))]),
    }
}

/// The ids of the run's demo controllers and live minigames in their lists' order, taken before the load (the
/// record names them by place).
struct Ids {
    demos: Vec<u32>,
    minigames: Vec<u32>,
    /// The run-time solids' specs in the World's list (a move's `solid` is the place in it).
    solids: Vec<i32>,
}

impl Ids {
    fn of(run: &TutorialRun, solids: Vec<i32>) -> Ids {
        Ids {
            solids,
            demos: run.demos.iter().map(|d| d.id).collect(),
            minigames: run.actor.as_ref().map_or_else(Vec::new, |a| a.minigames.iter().map(|m| m.id).collect()),
        }
    }
}

fn obj_make(ids: &Ids, v: &Value, l: &mut Lx) -> Option<TutorialObj> {
    match v {
        Value::Str(k) if k == "mode" => return Some(TutorialObj::Mode),
        Value::Str(k) if k == "actor" => return Some(TutorialObj::Actor),
        _ => {}
    }
    let ob = as_obj(v, l)?;
    let index = |k: &str, l: &mut Lx| ob.get(k).and_then(|v| num(v, l)).and_then(|v| int(v, l));
    if ob.get("demo").is_some() {
        let i = index("demo", l)?;
        let d = usize::try_from(i).ok().and_then(|i| ids.demos.get(i));
        if d.is_none() {
            l.miss(format_args!("demo {i}: no such demo controller in the World"));
        }
        return d.map(|&d| TutorialObj::Demo(d));
    }
    if ob.get("minigame").is_some() {
        let i = index("minigame", l)?;
        // (-1: a destroyed minigame's stale bind, by no id the World still has)
        let m = usize::try_from(i).ok().and_then(|i| ids.minigames.get(i));
        return Some(TutorialObj::Minigame(m.copied().unwrap_or(u32::MAX)));
    }
    if ob.get("stage").is_some() {
        return index("stage", l).map(TutorialObj::Stage);
    }
    l.miss(format_args!("{v:?} is no tutorial object"));
    None
}

fn objs_load(ids: &Ids, out: &mut Vec<TutorialObj>, v: &Value, l: &mut Lx) {
    let Some(a) = as_list(v, l) else { return };
    let made: Vec<_> = a.iter().filter_map(|v| obj_make(ids, v, l)).collect();
    *out = made;
}

/// A run list: one entry per bound handler, in bind order, each its owner alone (the oracle's record).
fn binds_put(run: &TutorialRun, v: &[TutorialBind]) -> Value {
    Value::List(v.iter().map(|b| obj_put(run, b.owner)).collect())
}

/// A run list's owners loaded: an entry whose owner is the World's at that place keeps the World's handler (a
/// class's second handler on one list included); any other gets the list's base handler.
fn binds_load(ids: &Ids, list: RunList, out: &mut Vec<TutorialBind>, v: &Value, l: &mut Lx) {
    let mut owners = Vec::new();
    objs_load(ids, &mut owners, v, l);
    let made = owners
        .into_iter()
        .enumerate()
        .map(|(i, owner)| match out.get(i) {
            Some(b) if b.owner == owner => *b,
            _ => TutorialBind { owner, handler: list.base_handler() },
        })
        .collect();
    *out = made;
}

fn run_put(w: &World, run: &TutorialRun, x: &Wx) -> Value {
    obj(vec![
        ("actor", run.actor.as_ref().map_or(Value::Null, |a| actor_put(w, run, a, x))),
        ("demos", Value::List(run.demos.iter().map(|d| demo_put(w, d, x)).collect())),
        ("playerRespawned", binds_put(run, &run.player_respawned)),
        ("sched", latent_put(w, run, &run.sched, x)),
    ])
}

fn run_load(run: &mut TutorialRun, solids: Vec<i32>, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["actor", "demos", "playerRespawned", "sched"], l);
    let ids = Ids::of(run, solids);
    if let Some(v) = key(ob, "demos", l) {
        l.at("demos", |l| {
            let Some(a) = as_list(v, l) else { return };
            if a.len() != run.demos.len() {
                l.miss(format_args!("{} demo controllers in the record, {} in the World", a.len(), run.demos.len()));
            }
            for (d, v) in run.demos.iter_mut().zip(a) {
                demo_load(d, v, l);
            }
        });
    }
    if let Some(v) = key(ob, "actor", l) {
        match (v, &mut run.actor) {
            (Value::Null, None) => {}
            (Value::Null, Some(_)) => l.miss("the actor: none in the record, one in the World"),
            (v, None) => l.miss(format_args!("the actor: {v:?} in the record, none in the World")),
            (v, Some(a)) => l.at("actor", |l| actor_load(&ids, a, v, l)),
        }
    }
    if let Some(v) = key(ob, "playerRespawned", l) {
        l.at("playerRespawned", |l| binds_load(&ids, RunList::PlayerRespawned, &mut run.player_respawned, v, l));
    }
    if let Some(v) = key(ob, "sched", l) {
        l.at("sched", |l| latent_load(&ids, &mut run.sched, v, l));
    }
}

fn target_put(w: &World, t: MoveTarget, x: &Wx) -> Value {
    match t {
        MoveTarget::Fighter(f) => obj(vec![("fighter", f.to_value(x))]),
        // (the record names a solid by its place in the World's list of run-time solids)
        MoveTarget::Solid(spec) => {
            let at = w.runtime_solids.iter().flatten().position(|s| s.spec == spec).and_then(|i| i32::try_from(i).ok());
            obj(vec![("solid", n(at.unwrap_or(-1)))])
        }
    }
}

fn target_make(v: &Value, l: &mut Lx) -> Option<MoveTarget> {
    let ob = as_obj(v, l)?;
    if let Some(f) = ob.get("fighter") {
        return FighterId::from_value(f, l).map(MoveTarget::Fighter);
    }
    let i = ob.get("solid").and_then(|v| num(v, l)).and_then(|v| int(v, l));
    if i.is_none() {
        l.miss(format_args!("{v:?} is no move target"));
    }
    i.map(MoveTarget::Solid)
}

fn latent_put(w: &World, run: &TutorialRun, s: &Latent, x: &Wx) -> Value {
    let delays = s
        .delays
        .iter()
        .map(|d| obj(vec![("at", n(d.at)), ("node", s_(d.node)), ("owner", obj_put(run, d.owner))]))
        .collect();
    let moves = s
        .moves
        .iter()
        .map(|e| {
            obj(vec![
                ("m", e.m.to_value(x)),
                ("node", s_(e.node)),
                ("owner", obj_put(run, e.owner)),
                ("t", target_put(w, e.t, x)),
            ])
        })
        .collect();
    let timers = s
        .timers
        .iter()
        .map(|e| {
            obj(vec![
                ("at", n(e.at)),
                ("from", n(e.from)),
                ("loop", b(e.looping)),
                ("n", n(e.n)),
                ("name", s_(e.name)),
                ("owner", obj_put(run, e.owner)),
                ("rate", e.rate.to_value(x)),
            ])
        })
        .collect();
    obj(vec![
        ("delays", Value::List(delays)),
        ("moves", Value::List(moves)),
        ("passed", n(s.passed)),
        ("timers", Value::List(timers)),
    ])
}

fn s_(v: &str) -> Value {
    s(v)
}

fn int_of(ob: &Obj, k: &str, l: &mut Lx) -> Option<i32> {
    key(ob, k, l).and_then(|v| num(v, l)).and_then(|v| int(v, l))
}

fn latent_load(ids: &Ids, s: &mut Latent, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["delays", "moves", "passed", "timers"], l);
    if let Some(p) = int_of(ob, "passed", l) {
        s.passed = p;
    }
    if let Some(a) = key(ob, "delays", l).and_then(|v| as_list(v, l)) {
        let mut out = Vec::with_capacity(a.len());
        for v in a {
            let Some(e) = as_obj(v, l) else { continue };
            let owner = key(e, "owner", l).and_then(|v| obj_make(ids, v, l));
            let node = key(e, "node", l).and_then(|v| text(v, l)).map(intern);
            if let (Some(owner), Some(node), Some(at)) = (owner, node, int_of(e, "at", l)) {
                out.push(DelayEntry { owner, node, at });
            }
        }
        s.delays = out;
    }
    if let Some(a) = key(ob, "moves", l).and_then(|v| as_list(v, l)) {
        // (a move's action is matched by its owner and node: a new one the World has not is a miss)
        for v in a {
            let Some(e) = as_obj(v, l) else { continue };
            let owner = key(e, "owner", l).and_then(|v| obj_make(ids, v, l));
            let node = key(e, "node", l).and_then(|v| text(v, l));
            let t = key(e, "t", l).and_then(|v| target_make(v, l));
            let (Some(owner), Some(node), Some(t)) = (owner, node, t) else { continue };
            match s.moves.iter_mut().find(|m: &&mut MoveEntry| m.owner == owner && m.node == node) {
                Some(m) => {
                    // (a solid's number is the spawner's own, the record's the list place: the World's stays)
                    if !matches!((m.t, t), (MoveTarget::Solid(_), MoveTarget::Solid(_))) {
                        m.t = t;
                    }
                    if let Some(v) = key(e, "m", l) {
                        l.at("moves.m", |l| m.m.load_value(v, l));
                    }
                }
                // A move the World has not made (its first update was before the frame): the action as the record has
                // it.
                None => {
                    let t = match t {
                        MoveTarget::Solid(i) => match usize::try_from(i).ok().and_then(|i| ids.solids.get(i)) {
                            Some(&spec) => MoveTarget::Solid(spec),
                            None => {
                                l.miss(format_args!("a move {node} of a run-time solid the World has not"));
                                continue;
                            }
                        },
                        t => t,
                    };
                    let mut m = ComponentMove::default();
                    if let Some(v) = key(e, "m", l) {
                        l.at("moves.m", |l| m.load_value(v, l));
                    }
                    s.moves.push(MoveEntry { owner, node: intern(node), t, m });
                }
            }
        }
        // (the World's moves the record has not are dropped: their action ended before the frame)
        let mut named: Vec<(Option<TutorialObj>, &str)> = Vec::new();
        for v in a {
            if let Value::Obj(e) = v
                && let Some(Value::Str(node)) = e.get("node")
            {
                named.push((e.get("owner").and_then(|v| obj_make(ids, v, l)), node.as_str()));
            }
        }
        s.moves.retain(|m| named.iter().any(|(o, n)| *o == Some(m.owner) && *n == m.node));
        if a.len() != s.moves.len() {
            l.miss(format_args!("{} moves in the record, {} in the World", a.len(), s.moves.len()));
        }
    }
    if let Some(a) = key(ob, "timers", l).and_then(|v| as_list(v, l)) {
        let mut out = Vec::with_capacity(a.len());
        for v in a {
            let Some(e) = as_obj(v, l) else { continue };
            let owner = key(e, "owner", l).and_then(|v| obj_make(ids, v, l));
            let name = key(e, "name", l).and_then(|v| text(v, l)).map(intern);
            let rate = key(e, "rate", l).and_then(|v| Real::from_value(v, l));
            let looping = matches!(key(e, "loop", l), Some(Value::Bool(true)));
            if let (Some(owner), Some(name), Some(rate), Some(from), Some(n), Some(at)) =
                (owner, name, rate, int_of(e, "from", l), int_of(e, "n", l), int_of(e, "at", l))
            {
                out.push(TimerEntry { owner, name, from, rate, n, at, looping });
            }
        }
        s.timers = out;
    }
}

/// `TutorialScore` as the oracle holds it: the task's MinigameText (its source) and the score.
fn score_put(w: &World, actor: &str, sc: &TutorialScore, x: &Wx) -> Value {
    let text = w
        .data
        .tutorials
        .get(actor)
        .and_then(|d| d.steps.get(usize::try_from(sc.step).ok()?))
        .and_then(|st| st.tasks.get(usize::try_from(sc.task).ok()?))
        .map(|t| t.minigame_text.source.clone());
    if text.is_none() {
        x.unmapped("a minigame score of no task in the data");
    }
    obj(vec![("descriptor", Value::Str(text.unwrap_or_default())), ("score", n(sc.score))])
}

fn events_put(v: &[TutorialEvent]) -> Value {
    Value::List(v.iter().map(|e| n(e.byte())).collect())
}

fn actor_put(w: &World, run: &TutorialRun, a: &TutorialActor, x: &Wx) -> Value {
    let timeline = a.timeline.as_ref().map_or(Value::Null, |t| obj(vec![("dir", n(t.dir)), ("t", t.t.to_value(x))]));
    obj(vec![
        ("actor", s(&a.actor)),
        ("allTasksComplete", b(a.all_tasks_complete)),
        ("audioStep", n(a.audio_step)),
        ("bag", a.bag.to_value(x)),
        ("bound", a.bound.to_value(x)),
        ("conceptCount", n(a.concept_count)),
        ("conceptIndex", n(a.concept_index)),
        ("counters", a.counters.to_value(x)),
        ("demoComplete", binds_put(run, &a.demo_complete)),
        ("desaturation", a.desaturation.to_value(x)),
        ("loopTask", n(a.loop_task)),
        ("managed", a.managed.to_value(x)),
        ("minigameCounters", a.minigame_counters.to_value(x)),
        ("minigameScores", Value::List(a.minigame_scores.iter().map(|sc| score_put(w, &a.actor, sc, x)).collect())),
        ("minigames", Value::List(a.minigames.iter().map(|m| minigame_put(m, w, x)).collect())),
        ("over", b(a.over)),
        ("pawnControlledByTutorial", b(a.pawn_controlled_by_tutorial)),
        ("queued", events_put(&a.queued)),
        ("stepCleanup", binds_put(run, &a.step_cleanup)),
        ("stepComplete", binds_put(run, &a.step_complete)),
        ("stepIndex", n(a.step_index)),
        ("timeline", timeline),
        ("transitioning", b(a.transitioning)),
    ])
}

const ACTOR_KEYS: [&str; 23] = [
    "actor",
    "allTasksComplete",
    "audioStep",
    "bag",
    "bound",
    "conceptCount",
    "conceptIndex",
    "counters",
    "demoComplete",
    "desaturation",
    "loopTask",
    "managed",
    "minigameCounters",
    "minigameScores",
    "minigames",
    "over",
    "pawnControlledByTutorial",
    "queued",
    "stepCleanup",
    "stepComplete",
    "stepIndex",
    "timeline",
    "transitioning",
];

fn actor_load(ids: &Ids, a: &mut TutorialActor, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &ACTOR_KEYS, l);
    let flag = |k: &str, out: &mut bool, l: &mut Lx| match key(ob, k, l) {
        Some(Value::Bool(v)) => *out = *v,
        Some(v) => l.miss(format_args!("{k}: {v:?} is not a boolean")),
        None => {}
    };
    flag("allTasksComplete", &mut a.all_tasks_complete, l);
    flag("over", &mut a.over, l);
    flag("pawnControlledByTutorial", &mut a.pawn_controlled_by_tutorial, l);
    flag("transitioning", &mut a.transitioning, l);
    for (k, out) in [
        ("audioStep", &mut a.audio_step),
        ("conceptCount", &mut a.concept_count),
        ("conceptIndex", &mut a.concept_index),
        ("loopTask", &mut a.loop_task),
        ("stepIndex", &mut a.step_index),
    ] {
        if let Some(v) = int_of(ob, k, l) {
            *out = v;
        }
    }
    if let Some(v) = key(ob, "actor", l).and_then(|v| text(v, l))
        && v != a.actor
    {
        l.miss(format_args!("the actor {v} in the record, {} in the World", a.actor));
    }
    if let Some(v) = key(ob, "bag", l) {
        l.at("bag", |l| a.bag.load_value(v, l));
    }
    if let Some(v) = key(ob, "managed", l) {
        l.at("managed", |l| a.managed.load_value(v, l));
    }
    if let Some(v) = key(ob, "bound", l) {
        l.at("bound", |l| a.bound.load_value(v, l));
    }
    if let Some(v) = key(ob, "counters", l) {
        l.at("counters", |l| a.counters.load_value(v, l));
    }
    if let Some(v) = key(ob, "minigameCounters", l) {
        l.at("minigameCounters", |l| a.minigame_counters.load_value(v, l));
    }
    if let Some(v) = key(ob, "desaturation", l) {
        l.at("desaturation", |l| a.desaturation.load_value(v, l));
    }
    if let Some(q) = key(ob, "queued", l).and_then(|v| as_list(v, l)) {
        a.queued = q.iter().filter_map(|v| num(v, l).and_then(|v| int(v, l))).map(TutorialEvent::from_byte).collect();
    }
    if let Some(v) = key(ob, "timeline", l) {
        a.timeline = match v {
            Value::Null => None,
            v => as_obj(v, l).and_then(|t| {
                let tt = key(t, "t", l).and_then(|v| Real::from_value(v, l))?;
                Some(Timeline { t: tt, dir: int_of(t, "dir", l)? })
            }),
        };
    }
    if let Some(sc) = key(ob, "minigameScores", l).and_then(|v| as_list(v, l)) {
        // (the record names a score's task by its text: the World's entries keep their step and task)
        if sc.len() != a.minigame_scores.len() {
            l.miss(format_args!(
                "{} minigame scores in the record, {} in the World",
                sc.len(),
                a.minigame_scores.len()
            ));
        }
        for (s, v) in a.minigame_scores.iter_mut().zip(sc) {
            if let Some(v) = as_obj(v, l).and_then(|o| int_of(o, "score", l)) {
                s.score = v;
            }
        }
    }
    for (k, list, out) in [
        ("demoComplete", RunList::DemoComplete, &mut a.demo_complete),
        ("stepCleanup", RunList::StepCleanup, &mut a.step_cleanup),
        ("stepComplete", RunList::StepComplete, &mut a.step_complete),
    ] {
        if let Some(v) = key(ob, k, l) {
            l.at(k, |l| binds_load(ids, list, out, v, l));
        }
    }
    if let Some(m) = key(ob, "minigames", l).and_then(|v| as_list(v, l)) {
        if m.len() != a.minigames.len() {
            l.miss(format_args!("{} minigames in the record, {} in the World", m.len(), a.minigames.len()));
        }
        for (g, v) in a.minigames.iter_mut().zip(m) {
            l.at("minigames", |l| minigame_load(g, v, l));
        }
    }
}

fn minigame_put(m: &Minigame, w: &World, x: &Wx) -> Value {
    obj(vec![
        ("cls", s(m.cls)),
        ("destroyed", b(m.destroyed)),
        ("state", payload::minigame_put(&m.state, w, x)),
        ("stepIndex", n(m.step_index)),
        ("taskIndex", n(m.task_index)),
    ])
}

fn minigame_load(m: &mut Minigame, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["cls", "destroyed", "state", "stepIndex", "taskIndex"], l);
    if let Some(c) = key(ob, "cls", l).and_then(|v| text(v, l))
        && c != m.cls
    {
        l.miss(format_args!("the minigame {c} in the record, {} in the World", m.cls));
    }
    if let Some(Value::Bool(d)) = key(ob, "destroyed", l) {
        m.destroyed = *d;
    }
    if let Some(v) = int_of(ob, "stepIndex", l) {
        m.step_index = v;
    }
    if let Some(v) = int_of(ob, "taskIndex", l) {
        m.task_index = v;
    }
    if let Some(v) = key(ob, "state", l) {
        l.at("state", |l| payload::minigame_load(&mut m.state, v, l));
    }
}

fn btns_put(v: &[Btn], x: &Wx) -> Value {
    Value::List(v.iter().map(|b| b.to_value(x)).collect())
}

fn pad_put(p: &DemoPad, x: &Wx) -> Value {
    obj(vec![
        ("atkDir", p.atk_dir.map_or(Value::Null, |d| d.to_value(x))),
        ("callX", b(p.call_x)),
        ("down", b(p.down)),
        ("forcePress", p.force_press.map_or(Value::Null, |f| f.to_value(x))),
        ("gap", btns_put(&p.gap, x)),
        ("held", Value::List(p.held.iter().map(|&h| b(h)).collect())),
        ("order", btns_put(&p.order, x)),
        ("pressX", p.press_x.map_or(Value::Null, |v| v.to_value(x))),
        ("releaseNext", btns_put(&p.release_next, x)),
        ("x", p.x.to_value(x)),
    ])
}

fn btns_load(out: &mut Vec<Btn>, v: &Value, l: &mut Lx) {
    if let Some(a) = as_list(v, l) {
        *out = a.iter().filter_map(|v| Btn::from_value(v, l)).collect();
    }
}

fn pad_load(p: &mut DemoPad, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["atkDir", "callX", "down", "forcePress", "gap", "held", "order", "pressX", "releaseNext", "x"], l);
    if let Some(v) = key(ob, "atkDir", l) {
        p.atk_dir = if *v == Value::Null { None } else { Dir::from_value(v, l) };
    }
    if let Some(v) = key(ob, "forcePress", l) {
        p.force_press = if *v == Value::Null { None } else { AttackBtn::from_value(v, l) };
    }
    if let Some(v) = key(ob, "pressX", l) {
        p.press_x = if *v == Value::Null { None } else { Real::from_value(v, l) };
    }
    if let Some(v) = key(ob, "x", l) {
        p.x.load_value(v, l);
    }
    if let Some(Value::Bool(v)) = key(ob, "callX", l) {
        p.call_x = *v;
    }
    if let Some(Value::Bool(v)) = key(ob, "down", l) {
        p.down = *v;
    }
    if let Some(h) = key(ob, "held", l).and_then(|v| as_list(v, l)) {
        for (o, v) in p.held.iter_mut().zip(h) {
            *o = *v == Value::Bool(true);
        }
    }
    for (k, out) in [("gap", &mut p.gap), ("order", &mut p.order), ("releaseNext", &mut p.release_next)] {
        if let Some(v) = key(ob, k, l) {
            btns_load(out, v, l);
        }
    }
}

/// A demo controller. `pawn` is the pawn its controller holds now and still in the match (the oracle walks pawnCtl);
/// `Demo::pawn`, the last pawn it possessed, stays set after the unpossess, which finds the controller by it.
fn demo_put(w: &World, d: &Demo, x: &Wx) -> Value {
    let held = w
        .pawn_ctl
        .ctl
        .iter()
        .filter(|(_, c)| matches!(c, PawnCtl::Demo(c) if c.id == d.id))
        .map(|(&f, _)| f)
        .next_back();
    let pawn = held.filter(|f| w.pawns.as_ref().is_some_and(|ps| ps.contains(f)));
    obj(vec![
        ("cls", s(d.cls)),
        ("pad", pad_put(&d.pad, x)),
        ("pawn", pawn.to_value(x)),
        ("state", payload::demo_put(&d.state, w, x)),
    ])
}

fn demo_load(d: &mut Demo, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["cls", "pad", "pawn", "state"], l);
    if let Some(c) = key(ob, "cls", l).and_then(|v| text(v, l))
        && c != d.cls
    {
        l.miss(format_args!("the demo controller {c} in the record, {} in the World", d.cls));
    }
    if let Some(v) = key(ob, "pad", l) {
        l.at("pad", |l| pad_load(&mut d.pad, v, l));
    }
    // (a null pawn leaves the World's last possessed one: the controller's unpossess finds it by that)
    if let Some(v) = key(ob, "pawn", l)
        && *v != Value::Null
    {
        l.at("pawn", |l| d.pawn.load_value(v, l));
    }
    if let Some(v) = key(ob, "state", l) {
        l.at("state", |l| payload::demo_load(&mut d.state, v, l));
    }
}

fn solid_put(r: &RuntimeSolid, x: &Wx) -> Value {
    let bx = match r.solid {
        RuntimeBox::Platform(i) => obj(vec![("platform", Value::Num(f64::from(i)))]),
        RuntimeBox::Floor | RuntimeBox::None => s("floor"),
    };
    obj(vec![
        ("b0", r.b0.to_value(x)),
        ("box", bx),
        ("component", r.component.to_value(x)),
        ("destroyed", b(r.destroyed)),
        ("l0", r.l0.to_value(x)),
        ("r0", r.r0.to_value(x)),
        ("state", r.state.to_value(x)),
        ("t0", r.t0.to_value(x)),
        ("x0", r.x0.to_value(x)),
        ("y0", r.y0.to_value(x)),
    ])
}

fn solid_load(r: &mut RuntimeSolid, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["b0", "box", "component", "destroyed", "l0", "r0", "state", "t0", "x0", "y0"], l);
    for (k, out) in [
        ("b0", &mut r.b0),
        ("l0", &mut r.l0),
        ("r0", &mut r.r0),
        ("t0", &mut r.t0),
        ("x0", &mut r.x0),
        ("y0", &mut r.y0),
    ] {
        if let Some(v) = key(ob, k, l) {
            out.load_value(v, l);
        }
    }
    if let Some(v) = key(ob, "component", l)
        && Name::from_value(v, l).is_some_and(|c| c != r.component)
    {
        l.miss("a run-time solid of another component");
    }
    if let Some(Value::Bool(d)) = key(ob, "destroyed", l) {
        r.destroyed = *d;
    }
    if let Some(v) = key(ob, "state", l) {
        l.at("state", |l| r.state.load_value(v, l));
    }
}

/// What the header's World has not made yet and the frame has: the TutorialActor (Start Tutorial's own literal), the
/// demo controllers (`classes::make_demo`, by their class in the record's order) and the minigame actors
/// (`classes::make_minigame`). Their ids are the next of `obj_seq` in that order: no id is written, the record names
/// a demo or a minigame by its place in its list.
// Deviation: the ids of a loaded run are not the ones the oracle's run had (a destroyed minigame's id is gone from the
// record); nothing reads an id but its own list, so no step differs.
fn make_objects(w: &mut World, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    let actor_v = ob.get("actor").filter(|a| **a != Value::Null);
    if actor_v.is_some() && w.tutorial.as_ref().is_some_and(|r| r.actor.is_none()) {
        match &w.setup.mode {
            Some(MatchMode::Tutorial { actor, .. }) => {
                let key = actor.clone();
                match w.data.tutorials.get(&key) {
                    Some(d) => {
                        let a = new_actor(key, d);
                        w.tutorial.as_mut().expect("checked above").actor = Some(a);
                    }
                    None => l.miss(format_args!("no data for the tutorial actor {key}")),
                }
            }
            None => l.miss("a tutorial actor in the record, no tutorial mode in the set-up"),
        }
    }
    if let Some(Value::List(a)) = ob.get("demos") {
        let have = w.tutorial.as_ref().map_or(0, |r| r.demos.len());
        for (i, d) in a.iter().enumerate().skip(have) {
            let cls = as_obj(d, l).and_then(|o| o.get("cls")).and_then(|c| text(c, l)).map(str::to_owned);
            let Some(cls) = cls else { continue };
            if mvs_sim::modes::tutorial::classes::make_demo(w, Some(class_key(&cls))).is_none() {
                l.at(&format!("demos[{i}]"), |l| l.miss(format_args!("no demo controller class {cls}")));
            }
        }
    }
    let Some(rec) = actor_v.and_then(|a| as_obj(a, l)) else { return };
    // The minigame actors: the World's list follows the record's (a loaded World can hold ones the record's frame has
    // already destroyed, or another class at a place): from the first place that differs the World's are dropped, the
    // record's made.
    if let Some(Value::List(m)) = rec.get("minigames") {
        let classes: Vec<Option<&str>> =
            m.iter().map(|g| as_obj(g, l).and_then(|o| o.get("cls")).and_then(|c| text(c, l))).collect();
        if let Some(a) = w.tutorial.as_mut().and_then(|r| r.actor.as_mut()) {
            let same = a.minigames.iter().zip(&classes).take_while(|(g, c)| Some(g.cls) == **c).count();
            a.minigames.truncate(same);
        }
        let have = w.tutorial.as_ref().and_then(|r| r.actor.as_ref()).map_or(0, |a| a.minigames.len());
        for (i, cls) in classes.iter().enumerate().skip(have) {
            let Some(cls) = cls else { continue };
            if w.tutorial.as_ref().is_some_and(|r| r.actor.is_some())
                && mvs_sim::modes::tutorial::classes::make_minigame(w, class_key(cls)).is_none()
            {
                l.at(&format!("actor.minigames[{i}]"), |l| l.miss(format_args!("no minigame class {cls}")));
            }
        }
    }
    if let Some(Value::List(sc)) = rec.get("minigameScores") {
        scores_make(w, sc, l);
    }
}

/// The actor's TutorialMinigameScores as many as the record's: an entry's step and task are those of the first
/// SaveMinigameScore task, not yet taken (else the first), whose MinigameText is the entry's `descriptor`.
// Unverified: two tasks with one MinigameText are told apart by their order only.
fn scores_make(w: &mut World, sc: &[Value], l: &mut Lx) {
    let Some(a) = w.tutorial.as_ref().and_then(|r| r.actor.as_ref()) else { return };
    let Some(data) = w.data.tutorials.get(&a.actor) else { return };
    let mut made: Vec<TutorialScore> = Vec::new();
    for (i, v) in sc.iter().enumerate() {
        if let Some(old) = a.minigame_scores.get(i) {
            made.push(*old);
            continue;
        }
        let descriptor = as_obj(v, l).and_then(|o| o.get("descriptor")).and_then(|d| text(d, l));
        let find = |d: &str, untaken: bool| {
            data.steps.iter().enumerate().find_map(|(si, st)| {
                st.tasks.iter().enumerate().find_map(|(ti, t)| {
                    let (si, ti) = (i32::try_from(si).ok()?, i32::try_from(ti).ok()?);
                    let taken = untaken && made.iter().any(|m| m.step == si && m.task == ti);
                    (t.minigame_text.source == d && !taken).then_some(TutorialScore { step: si, task: ti, score: 0 })
                })
            })
        };
        let found = descriptor.and_then(|d| find(d, true).or_else(|| find(d, false)));
        match found {
            Some(s) => made.push(s),
            None => l.miss(format_args!("minigameScores[{i}]: no task of the data has its descriptor")),
        }
    }
    let a = w.tutorial.as_mut().and_then(|r| r.actor.as_mut()).expect("checked above");
    a.minigame_scores = made;
}

/// What a tutorial frame has that the World lacks before the load reaches it: the stage actors and the run-time solids.
pub fn make_parts(w: &mut World, rest: &Obj, l: &mut Lx) {
    if w.tutorial.is_none() {
        return;
    }
    make_solids(w, rest, l);
    make_stage_actors(w, rest, l);
}

/// The spec number of the live run-time solid the record lists at `place`: held solids are numbered by it, from -1
/// down (the spawner's own numbers, `spec_of`, are never negative).
// Deviation: a loaded World's solids carry these numbers, not the spawner's; the record has none, and nothing reads
// one but a held reference's own solid.
pub fn spec_at(place: usize) -> i32 {
    -1 - i32::try_from(place).expect("a short list")
}

/// The held solids of a frame: each `{spec, box, l0, r0, t0, b0, destroyed}` object under `v` that is live.
fn held_records<'v>(v: &'v Value, out: &mut Vec<&'v Obj>) {
    match v {
        Value::Obj(o) => {
            if matches!(o.get("spec"), Some(Value::Obj(_)))
                && o.get("l0").is_some()
                && o.get("destroyed") == Some(&Value::Bool(false))
            {
                out.push(o);
                return;
            }
            for (_, v) in o.entries() {
                held_records(v, out);
            }
        }
        Value::List(a) => a.iter().for_each(|v| held_records(v, out)),
        _ => {}
    }
}

/// The World's run-time solids made those of the record's `runtimeSolids`, in its order, each from the spec the held
/// reference of the frame names (the entry holds only its box and state: the spec is the class's, `geometry.rs`
/// `spec_put`). The solids the World had are removed first.
fn make_solids(w: &mut World, rest: &Obj, l: &mut Lx) {
    use mvs_sim::modes::tutorial::advanced::geometry::solids::SolidClass;
    use mvs_sim::world::stage::{RuntimeSolidSpec, add_runtime_solid, remove_runtime_solid};
    let have: Vec<i32> = w.runtime_solids.iter().flatten().map(|s| s.spec).collect();
    for spec in have {
        remove_runtime_solid(w, spec);
    }
    let Some(Value::List(entries)) = rest.get("runtimeSolids") else { return };
    let mut places = Vec::new();
    for v in entries {
        let g = |k: &str, l: &mut Lx| as_obj(v, l).and_then(|o| o.get(k)).and_then(|v| num(v, l));
        let p = ["l0", "r0", "t0", "b0"].map(|k| g(k, l));
        places.push(p.map(|c| c.unwrap_or(f64::NAN)));
    }
    l.runtime_places = places.clone();
    let mut helds = Vec::new();
    if let Some(t) = rest.get("tutorial") {
        held_records(t, &mut helds);
    }
    for (k, p) in places.iter().enumerate() {
        let found = helds.iter().copied().find(|h| {
            ["l0", "r0", "t0", "b0"].iter().zip(p).all(|(n, c)| matches!(h.get(n), Some(Value::Num(x)) if x == c))
        });
        let Some(h) = found else {
            l.at(&format!("runtimeSolids[{k}]"), |l| l.miss("a run-time solid no held reference of the frame names"));
            continue;
        };
        let Some(sv) = h.get("spec").and_then(|s| as_obj(s, l)) else { continue };
        let class = match sv.get("cls").and_then(|c| text(c, l)) {
            Some("TutorialWall_C") => SolidClass::Wall,
            Some("TutorialWallShort_C") => SolidClass::WallShort,
            Some("TutorialPlatform_C") => SolidClass::Platform,
            c => {
                l.miss(format_args!("no rule to make a run-time solid of class {c:?}"));
                continue;
            }
        };
        let triple = |k: &str, l: &mut Lx| -> Option<[Real; 3]> {
            let Value::List(a) = sv.get(k)? else { return None };
            let c: Vec<f64> = a.iter().filter_map(|v| num(v, l)).collect();
            Some([
                mvs_sim::math::narrow(*c.first()?),
                mvs_sim::math::narrow(*c.get(1)?),
                mvs_sim::math::narrow(*c.get(2)?),
            ])
        };
        let Some(center) = triple("center", l) else { continue };
        let d = RuntimeSolidSpec {
            cls: class.cls().to_owned(),
            floor: class.floor(),
            center,
            half_extent: class.half(),
            rot: triple("rot", l),
            component: class.component(),
        };
        if !add_runtime_solid(w, spec_at(k), &d) {
            l.miss("a run-time solid of the record the World refuses");
        }
    }
}

/// The stage actors a tutorial's graphs spawned at run time (the Meeseeks tutorials' Tutorial_Meeseeks), made in the
/// World's stage state for the places of the record's `stageRun.actors` it has not, and dropped past its last: their
/// fields are then the stage load's. A World with no stage state gets an empty one, as `spawn_stage_actor` does.
fn make_stage_actors(w: &mut World, rest: &Obj, l: &mut Lx) {
    use mvs_sim::actors::StageActorClass;
    use mvs_sim::world::stage::StageState;
    use mvs_sim::world::stage_actors::{StageActor, StageActorOwn};
    if w.tutorial.is_none() {
        return;
    }
    let Some(Value::Obj(run)) = rest.get("stageRun") else { return };
    let Some(Value::List(a)) = run.get("actors") else { return };
    let have = w.stage_run.as_ref().map_or(0, |r| r.actors.len());
    if a.len() > have {
        w.stage_run.get_or_insert_with(StageState::default);
    }
    if let Some(r) = w.stage_run.as_mut() {
        r.actors.truncate(a.len());
    }
    for (i, v) in a.iter().enumerate().skip(have) {
        let cls = as_obj(v, l).and_then(|o| o.get("cls")).and_then(|c| text(c, l));
        match cls {
            Some("Tutorial_Meeseeks_C") => {
                let own = StageActorOwn::Meeseeks(mvs_sim::modes::tutorial::advanced::meeseeks::MeeseeksOwn::default());
                let index = u32::try_from(i).expect("a list index");
                let actor = StageActor {
                    cls: StageActorClass::TutorialMeeseeks,
                    index,
                    team: 1,
                    dead: false,
                    x: 0.0,
                    y: 0.0,
                    own,
                };
                w.stage_run.as_mut().expect("made above").actors.push(actor);
            }
            c => {
                l.at(&format!("stageRun.actors[{i}]"), |l| l.miss(format_args!("no rule to make a stage actor {c:?}")))
            }
        }
    }
    l.stage_actors = w.stage_run.as_ref().map_or(0, |r| r.actors.len());
}

/// A class as the sim's `make_*` take it: the record names a generated class with its `_C`.
fn class_key(cls: &str) -> &str {
    cls.strip_suffix("_C").unwrap_or(cls)
}

/// The TutorialActor of the tutorial `data` at its Start Tutorial (`run.rs` mode_start_tutorial's literal).
fn new_actor(key: String, data: &mvs_data::tutorial::TutorialActorData) -> TutorialActor {
    let counters = || data.steps.iter().map(|s| vec![0; s.tasks.len()]).collect::<Vec<_>>();
    TutorialActor {
        actor: key,
        step_index: 0,
        concept_index: 1,
        concept_count: i32::try_from(data.steps.iter().filter(|s| s.new_concept).count()).expect("a short list"),
        audio_step: 0,
        transitioning: false,
        pawn_controlled_by_tutorial: true,
        all_tasks_complete: true,
        over: false,
        desaturation: 0.0,
        minigame_scores: Vec::new(),
        counters: counters(),
        queued: Vec::new(),
        managed: None,
        bag: None,
        bound: Vec::new(),
        timeline: None,
        minigame_counters: counters(),
        loop_task: 0,
        step_complete: Vec::new(),
        step_cleanup: Vec::new(),
        demo_complete: Vec::new(),
        minigames: Vec::new(),
    }
}

/// `run::possess_bot`'s controller of an Idle bag without its possession (the fighter's own load sets what that does).
fn idle_bot(w: &World, f: FighterId) -> Option<PawnCtl> {
    let trees = w.bot_trees.clone()?;
    let fr = w.fighter(f);
    let seed = mvs_sim::math::dbl::to_int32(f64::from(fr.id) * 7919.0 + f64::from(w.flow.tick) * 104_729.0 + 12345.0);
    let mut bot = mvs_sim::ai::bt::BtBot::controller_bot(seed, &trees, mvs_sim::ai::bt::BotController::Idle);
    bot.ticks = 1;
    bot.now = fr.clock.time;
    bot.fighter_born = fr.clock.creation_time;
    Some(PawnCtl::Bot(Box::new(mvs_sim::world::pawns::bot_ctl(bot, None, None))))
}

/// The record's controllers set as the World's `pawnCtl`, in the record's order. A player's, a demo's (by its place in
/// the run's list) and none are made; a bot's is kept from the World when it has one for the pawn (its tree and state
/// are no record: a frame whose bot the World lacks is a miss).
fn ctl_load(w: &mut World, v: &Value, l: &mut Lx) {
    let Some(a) = as_list(v, l) else { return };
    let demos: Vec<u32> = w.tutorial.as_ref().map_or_else(Vec::new, |r| r.demos.iter().map(|d| d.id).collect());
    let mut old = std::mem::take(&mut w.pawn_ctl.ctl);
    for v in a {
        let Some(pair) = as_list(v, l) else { continue };
        let (Some(fv), Some(cv)) = (pair.first(), pair.get(1)) else {
            l.miss("a controller entry is no pair");
            continue;
        };
        let Some(f) = FighterId::from_value(fv, l) else { continue };
        let Some(o) = as_obj(cv, l) else { continue };
        let kind = o.get("kind").and_then(|v| text(v, l)).unwrap_or("?").to_owned();
        let c = match kind.as_str() {
            "player" => PawnCtl::Player,
            "none" => PawnCtl::None,
            "demo" => {
                let i = o.get("demo").and_then(|v| num(v, l)).and_then(|v| int(v, l));
                match i.and_then(|i| usize::try_from(i).ok()).and_then(|i| demos.get(i)) {
                    Some(&id) => mvs_sim::modes::tutorial::demo::ctl(id),
                    None => {
                        l.miss("a demo controller of the record the World has not");
                        continue;
                    }
                }
            }
            "bot" => match old.shift_remove(&f) {
                Some(c @ PawnCtl::Bot(_)) => c,
                // Unverified: the record names no tree: a bot the World has none for is the tutorial's idle bag's
                // (Idle_AIController, the only bot controller a tutorial class possesses a pawn with), with a seed
                // and a clock of its own, so a step from this frame does not run the oracle's bot.
                _ => match idle_bot(w, f) {
                    Some(c) => c,
                    None => {
                        l.miss("a bot controller and no bot trees in the World");
                        continue;
                    }
                },
            },
            k => {
                l.miss(format_args!("a {k} controller (no rule to make it)"));
                continue;
            }
        };
        w.pawn_ctl.ctl.insert(f, c);
    }
}

#[cfg(test)]
mod tests {
    use mvs_sim::fighter::match_side::TutorialEvent;
    use mvs_sim::input::{AttackBtn, Btn};
    use mvs_sim::modes::tutorial::demo::{Aerial, Demo, DemoPad, DemoState};
    use mvs_sim::modes::tutorial::latent::{DelayEntry, Latent, MoveEntry, MoveTarget, TimerEntry};
    use mvs_sim::modes::tutorial::minigame::{Minigame, MinigameState};
    use mvs_sim::modes::tutorial::{Timeline, TutorialActor, TutorialBind, TutorialObj, TutorialRun, TutorialScore};
    use mvs_sim::setup::{MatchPlayer, MatchSetup};
    use mvs_sim::world::stage::ComponentMove;
    use mvs_trace::Value;

    use crate::cx::{Lx, Wx};
    use crate::harness;

    /// A run list of `owners`, each with `handler` (none when `on` is false).
    fn binds(on: bool, owners: &[TutorialObj], handler: &'static str) -> Vec<TutorialBind> {
        if on { owners.iter().map(|&owner| TutorialBind { owner, handler }).collect() } else { vec![] }
    }

    /// A run of every key's shape: a demo, a minigame, a delay, a move and a timer of each owner kind. `v` 0 is the
    /// blank one the load fills (the same demos, minigames and move, every value else at its zero).
    fn run(v: i32, me: mvs_sim::fighter::FighterId, it: mvs_sim::fighter::FighterId) -> TutorialRun {
        let on = v != 0;
        #[allow(clippy::cast_precision_loss)]
        let r = v as f32;
        let actor = TutorialActor {
            actor: "TutorialActor_Armor".to_owned(),
            step_index: v,
            concept_index: v + 1,
            concept_count: v * 2,
            audio_step: v * 3,
            transitioning: on,
            pawn_controlled_by_tutorial: on,
            all_tasks_complete: on,
            over: on,
            desaturation: r * 0.25,
            minigame_scores: vec![TutorialScore { step: 0, task: 0, score: v * 4 }],
            counters: if on { vec![vec![1, 2], vec![3]] } else { vec![] },
            queued: if on { vec![TutorialEvent::from_byte(28)] } else { vec![] },
            managed: on.then_some(me),
            bag: on.then_some(it),
            bound: if on { vec![me] } else { vec![] },
            timeline: on.then_some(Timeline { t: 0.125, dir: -1 }),
            minigame_counters: if on { vec![vec![5]] } else { vec![] },
            loop_task: v * 5,
            step_complete: binds(on, &[TutorialObj::Minigame(7), TutorialObj::Demo(3)], "onStepComplete"),
            step_cleanup: binds(on, &[TutorialObj::Minigame(7), TutorialObj::Minigame(7)], "onStepCleanup"),
            demo_complete: binds(on, &[TutorialObj::Demo(3), TutorialObj::Stage(2)], "demoComplete"),
            minigames: vec![Minigame {
                id: 7,
                cls: "TutorialMinigame_ArmorBreak",
                destroyed: on,
                step_index: v,
                task_index: v - 1,
                state: MinigameState::ArmorBreak(Default::default()),
            }],
        };
        let pad = DemoPad {
            held: [on, false, on, false],
            x: r * -0.5,
            y: r * 0.25,
            press_x: on.then_some(r),
            call_x: on,
            gap: if on { vec![Btn::Jump] } else { vec![] },
            order: if on { vec![Btn::Attack, Btn::Dodge] } else { vec![] },
            force_press: on.then_some(AttackBtn::Special),
            atk_dir: None,
            down: on,
            release_next: if on { vec![Btn::Special] } else { vec![] },
        };
        let m = ComponentMove { total: r, elapsed: r * 0.5, interpolating: on, ..ComponentMove::default() };
        let sched = Latent {
            delays: if on {
                vec![
                    DelayEntry { owner: TutorialObj::Actor, node: "@120", at: 40 },
                    DelayEntry { owner: TutorialObj::Minigame(7), node: "Delay", at: 41 },
                ]
            } else {
                vec![]
            },
            moves: vec![MoveEntry { owner: TutorialObj::Demo(3), node: "@9", t: MoveTarget::Fighter(it), m }],
            timers: if on {
                vec![TimerEntry {
                    owner: TutorialObj::Mode,
                    name: "Spawn",
                    from: 3,
                    rate: 2.0,
                    n: 1,
                    at: 99,
                    looping: true,
                }]
            } else {
                vec![]
            },
            passed: v * 6,
        };
        TutorialRun {
            sched,
            actor: Some(actor),
            player_respawned: binds(on, &[TutorialObj::Actor], "playerRespawned"),
            demos: vec![Demo {
                id: 3,
                cls: "TutorialController_AerialAttack",
                pawn: on.then_some(it),
                pad,
                state: DemoState::Aerial(Aerial { evade: false, ..Aerial::default() }),
            }],
            obj_seq: 8,
        }
    }

    /// The tutorial's rest keys written, loaded into a blank run of the same objects and written again give the same
    /// value: the loader sets every value the writer writes.
    #[test]
    fn the_tutorial_keys_load_what_they_wrote() {
        let player = |c: &str| MatchPlayer { character: Some(c.to_owned()), ..MatchPlayer::default() };
        let setup = MatchSetup { players: vec![player("shaggy"), player("wonderwoman")], ..MatchSetup::default() };
        let Some(mut h) = harness::fresh(&setup, 1) else { return };
        let (me, it) = (h.fighter(0), h.fighter(1));
        let put = |h: &harness::Harness| {
            let x = Wx::new(&h.w, &h.book);
            let mut o = Vec::new();
            super::rest_put(&h.w, &x, &mut o);
            o.sort_by(|a, b| a.0.cmp(b.0));
            crate::conv::obj(o)
        };
        h.w.tutorial = Some(Box::new(run(1, me, it)));
        let first = put(&h);
        let Value::Obj(ob) = &first else { panic!("an object") };
        assert!(ob.get("tutorial").is_some() && ob.get("pawnCtl").is_some());
        h.w.tutorial = Some(Box::new(run(0, me, it)));
        assert_ne!(put(&h), first, "the blank run writes another value");
        let mut l = Lx::new(&h.w, &h.book);
        super::rest_load(&mut h.w, ob, &mut l);
        assert!(l.misses.is_empty(), "the load missed {:?}", l.misses);
        assert_eq!(put(&h), first);
    }
}
