//! The shared parts of `World.stageRun` and the stage's platforms as the record writes them: `StageRun`'s `timers`,
//! `solids`, `boxes` and `actors`, the pass-through platforms (StagePlatform) with a moving one's `run`.

use mvs_data::stage::PlatformMoving;
use mvs_sim::world::World;
use mvs_sim::world::stage::{PlatformRun, SolidBox, StageSolid, StageState, StageTimer};
use mvs_sim::world::stage_actors::StageActor;
use mvs_trace::{Obj, Value};

use super::{actor_own_load, actor_own_put, own_load, own_put};
use crate::conv::{Plain, as_list, as_obj, check_keys, obj, plain_keys, plain_struct, text};
use crate::cx::{Lx, Wx};

plain_struct!(StageTimer {
    "at" = at, "delay" = delay, "ev" = ev, "from" = from, "key" = key, "loop" = looping, "n" = n, "owner" = owner,
    "rate" = rate,
});

plain_struct!(StageSolid {
    "act" = m.act, "dx" = m.dx, "dy" = m.dy, "enabled" = enabled, "ev" = m.ev, "fresh" = fresh, "id" = id,
    "mx" = m.mx, "my" = m.my, "owner" = m.owner, "tpx" ~ m.tpx, "tpy" ~ m.tpy, "vx" = m.vx, "vy" = m.vy,
});

/// `StageRun` whole, or `None` where the World has none (the key is then absent from the rest).
pub fn run_put(r: &StageState, x: &Wx) -> Value {
    let boxes = r.boxes.iter().map(|b| box_put(r, b, x)).collect();
    obj(vec![
        ("actors", Value::List(r.actors.iter().map(|a| actor_put(a, x)).collect())),
        ("boxes", Value::List(boxes)),
        ("s", own_put(&r.s, x)),
        ("solids", r.solids.to_value(x)),
        ("timers", r.timers.to_value(x)),
    ])
}

/// Sets `w`'s stage run from its record: the World's construction made the actors' classes and the stage's own
/// state, the record's values go into them.
pub fn run_load(w: &mut World, v: &Value, l: &mut Lx) {
    let Some(r) = w.stage_run.as_mut() else {
        l.miss("a stage run the World has none of");
        return;
    };
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["actors", "boxes", "s", "solids", "timers"], l);
    let key = |n: &str, l: &mut Lx, f: &mut dyn FnMut(&Value, &mut Lx)| match ob.get(n) {
        Some(v) => l.at(n, |l| f(v, l)),
        None => l.miss(format_args!("key {n} absent")),
    };
    key("timers", l, &mut |v, l| r.timers.load_value(v, l));
    key("solids", l, &mut |v, l| r.solids.load_value(v, l));
    key("s", l, &mut |v, l| own_load(&mut r.s, v, l));
    key("boxes", l, &mut |v, l| boxes_load(r, v, l));
    key("actors", l, &mut |v, l| actors_load(r, v, l));
}

/// A box whole (`PlatformBox`'s keys), its `s` the solid written whole as stateOf writes an object no list holds.
pub fn box_put(r: &StageState, b: &SolidBox, x: &Wx) -> Value {
    obj(vec![
        ("b0", b.b0.to_value(x)),
        ("dead", b.dead.to_value(x)),
        ("flags", b.flags.to_value(x)),
        ("l0", b.l0.to_value(x)),
        ("r0", b.r0.to_value(x)),
        ("s", r.solids[b.solid as usize].to_value(x)),
        ("t0", b.t0.to_value(x)),
    ])
}

fn boxes_load(r: &mut StageState, v: &Value, l: &mut Lx) {
    let Some(a) = as_list(v, l) else { return };
    r.boxes.truncate(a.len());
    for (i, e) in a.iter().enumerate() {
        l.at(&format!("[{i}]"), |l| {
            let Some(ob) = as_obj(e, l) else { return };
            check_keys(ob, &["b0", "dead", "flags", "l0", "r0", "s", "t0"], l);
            // The solid is the one the box already points at; a new box finds its solid by the id the record names.
            let id =
                ob.get("s").and_then(|s| as_obj(s, l)).and_then(|s| s.get("id")).and_then(|v| i32::from_value(v, l));
            if r.boxes.len() <= i {
                let solid = r.solids.iter().position(|s| Some(s.id) == id);
                let Some(solid) = solid.and_then(|s| u32::try_from(s).ok()) else {
                    l.miss("a box whose solid the World has none of");
                    return;
                };
                r.boxes.push(SolidBox { solid, ..SolidBox::default() });
            }
            let b = &mut r.boxes[i];
            let num = |n: &str, set: &mut dyn FnMut(&Value, &mut Lx), l: &mut Lx| match ob.get(n) {
                Some(v) => l.at(n, |l| set(v, l)),
                None => l.miss(format_args!("key {n} absent")),
            };
            num("b0", &mut |v, l| b.b0.load_value(v, l), l);
            num("dead", &mut |v, l| b.dead.load_value(v, l), l);
            num("flags", &mut |v, l| b.flags.load_value(v, l), l);
            num("l0", &mut |v, l| b.l0.load_value(v, l), l);
            num("r0", &mut |v, l| b.r0.load_value(v, l), l);
            num("t0", &mut |v, l| b.t0.load_value(v, l), l);
        });
    }
}

fn actor_put(a: &StageActor, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = vec![
        ("cls".to_owned(), Value::Str(a.cls.name().to_owned())),
        ("dead".to_owned(), a.dead.to_value(x)),
        ("index".to_owned(), Value::Num(f64::from(a.index))),
        ("team".to_owned(), a.team.to_value(x)),
        ("x".to_owned(), a.x.to_value(x)),
        ("y".to_owned(), a.y.to_value(x)),
    ];
    actor_own_put(&a.own, x, &mut o);
    Value::Obj(Obj::new(o))
}

/// The actors by their order in the list: the World's create made each one's class and own variant; a run-time actor
/// the record holds past them is a miss.
fn actors_load(r: &mut StageState, v: &Value, l: &mut Lx) {
    let Some(a) = as_list(v, l) else { return };
    if a.len() != r.actors.len() {
        l.miss(format_args!("{} stage actors where the World holds {}", a.len(), r.actors.len()));
    }
    for (i, e) in a.iter().enumerate() {
        l.at(&format!("[{i}]"), |l| {
            let Some(ob) = as_obj(e, l) else { return };
            let Some(a) = r.actors.get_mut(i) else { return };
            let mut known: Vec<&'static str> = vec!["cls", "dead", "index", "team", "x", "y"];
            match ob.get("cls") {
                Some(c) => {
                    if text(c, l).is_some_and(|c| c != a.cls.name()) {
                        l.miss("a stage actor of another class than the World's");
                    }
                }
                None => l.miss("key cls absent"),
            }
            let key = |n: &str, l: &mut Lx, set: &mut dyn FnMut(&Value, &mut Lx)| match ob.get(n) {
                Some(v) => l.at(n, |l| set(v, l)),
                None => l.miss(format_args!("key {n} absent")),
            };
            key("dead", l, &mut |v, l| a.dead.load_value(v, l));
            key("index", l, &mut |v, l| {
                let mut i = 0i32;
                i.load_value(v, l);
                a.index = u32::try_from(i).unwrap_or(0);
            });
            key("team", l, &mut |v, l| a.team.load_value(v, l));
            key("x", l, &mut |v, l| a.x.load_value(v, l));
            key("y", l, &mut |v, l| a.y.load_value(v, l));
            actor_own_load(&mut a.own, ob, l, &mut known);
            check_keys(ob, &known, l);
        });
    }
}

/// `World.stagePlatforms`: each platform's keys, a moving one's `run` with its data `d`.
pub fn platforms_put(w: &World, x: &Wx) -> Value {
    Value::List((0..w.stage.stage_platforms.len()).map(|i| platform_put(w, i, x)).collect())
}

/// One platform whole (a fighter's `base` holds the same object).
pub fn platform_put(w: &World, i: usize, x: &Wx) -> Value {
    let p = &w.stage.stage_platforms[i];
    let mut o: Vec<(&str, Value)> = vec![
        ("bottom", p.bottom.to_value(x)),
        ("dead", Value::Bool(false)),
        ("dx", p.dx.to_value(x)),
        ("dy", p.dy.to_value(x)),
        ("left", p.left.to_value(x)),
        ("right", p.right.to_value(x)),
        ("stageIndex", p.stage_index.to_value(x)),
        ("top", p.top.to_value(x)),
        ("vx", p.vx.to_value(x)),
        ("vy", p.vy.to_value(x)),
    ];
    if let Some(m) = p.moving {
        o.push(("moving", Value::Bool(m)));
    }
    let run =
        w.stage_run.as_ref().and_then(|r| r.platforms.iter().find(|r| i32::try_from(r.index) == Ok(p.stage_index)));
    if let Some(r) = run {
        let data = w.data.stage.platforms.as_ref().and_then(|l| l.get(i)?.moving.as_ref());
        o.push(("run", run_of_put(r, data, x)));
    }
    obj(o)
}

fn run_of_put(r: &PlatformRun, data: Option<&PlatformMoving>, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = Vec::new();
    plain_keys!(put r, x, o; "act" = m.act, "b0" = b0, "curve" = curve, "cx" = cx, "cy" = cy, "dx" = m.dx, "dy" = m.dy,
        "ev" = m.ev, "l0" = l0, "lastMoveZ" = last_move_z, "mx" = m.mx, "my" = m.my, "owner" = m.owner, "r0" = r0,
        "t0" = t0, "tpx" ~ m.tpx, "tpy" ~ m.tpy, "vx" = m.vx, "vy" = m.vy);
    match data {
        Some(d) => {
            let nums = |v: &[f64]| Value::List(v.iter().map(|n| Value::Num(*n)).collect());
            let curves =
                d.curves.iter().map(|c| obj(vec![("d", nums(&c.d)), ("duration", Value::Num(c.duration))])).collect();
            o.push(("d".to_owned(), obj(vec![("curves", Value::List(curves)), ("start", nums(&d.start))])));
        }
        None => x.unmapped("a moving platform's data"),
    }
    Value::Obj(Obj::new(o))
}

/// Loads the platforms into the World's own list; a moving one's `run` into its [`PlatformRun`] (its data `d` is the
/// World's, not loaded).
pub fn platforms_load(w: &mut World, v: &Value, l: &mut Lx) {
    let Some(a) = as_list(v, l) else { return };
    if a.len() != w.stage.stage_platforms.len() {
        l.miss(format_args!("{} stage platforms where the World holds {}", a.len(), w.stage.stage_platforms.len()));
    }
    for (i, e) in a.iter().enumerate() {
        l.at(&format!("[{i}]"), |l| {
            let Some(ob) = as_obj(e, l) else { return };
            let Some(p) = w.stage.stage_platforms.get_mut(i) else { return };
            check_keys(
                ob,
                &["bottom", "dead", "dx", "dy", "left", "moving", "right", "run", "stageIndex", "top", "vx", "vy"],
                l,
            );
            let key = |n: &str, l: &mut Lx, set: &mut dyn FnMut(&Value, &mut Lx)| match ob.get(n) {
                Some(v) => l.at(n, |l| set(v, l)),
                None => l.miss(format_args!("key {n} absent")),
            };
            key("bottom", l, &mut |v, l| p.bottom.load_value(v, l));
            key("dx", l, &mut |v, l| p.dx.load_value(v, l));
            key("dy", l, &mut |v, l| p.dy.load_value(v, l));
            key("left", l, &mut |v, l| p.left.load_value(v, l));
            key("right", l, &mut |v, l| p.right.load_value(v, l));
            key("stageIndex", l, &mut |v, l| p.stage_index.load_value(v, l));
            key("top", l, &mut |v, l| p.top.load_value(v, l));
            key("vx", l, &mut |v, l| p.vx.load_value(v, l));
            key("vy", l, &mut |v, l| p.vy.load_value(v, l));
            key("dead", l, &mut |v, l| {
                if !matches!(v, Value::Bool(false)) {
                    l.miss("a dead stage platform");
                }
            });
            p.moving = None;
            if let Some(m) = ob.get("moving") {
                l.at("moving", |l| {
                    let mut b = false;
                    b.load_value(m, l);
                    p.moving = Some(b);
                });
            }
            let index = p.stage_index;
            if let Some(rv) = ob.get("run") {
                l.at("run", |l| {
                    let run = w
                        .stage_run
                        .as_mut()
                        .and_then(|r| r.platforms.iter_mut().find(|r| i32::try_from(r.index) == Ok(index)));
                    match run {
                        Some(r) => run_of_load(r, rv, l),
                        None => l.miss("a platform run the World has none of"),
                    }
                });
            }
        });
    }
}

fn run_of_load(r: &mut PlatformRun, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    plain_keys!(load r, ob, l; "act" = m.act, "b0" = b0, "curve" = curve, "cx" = cx, "cy" = cy, "dx" = m.dx, "dy" = m.dy,
        "ev" = m.ev, "l0" = l0, "lastMoveZ" = last_move_z, "mx" = m.mx, "my" = m.my, "owner" = m.owner, "r0" = r0,
        "t0" = t0, "tpx" ~ m.tpx, "tpy" ~ m.tpy, "vx" = m.vx, "vy" = m.vy);
    check_keys(
        ob,
        &[
            "act",
            "b0",
            "curve",
            "cx",
            "cy",
            "d",
            "dx",
            "dy",
            "ev",
            "l0",
            "lastMoveZ",
            "mx",
            "my",
            "owner",
            "r0",
            "t0",
            "tpx",
            "tpy",
            "vx",
            "vy",
        ],
        l,
    );
}
