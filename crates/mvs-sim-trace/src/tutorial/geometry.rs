//! The geometry family's own keys as the record writes them: each class's
//! fields under the oracle names, and a held run-time solid whole (the oracle writes the `RuntimeSolid` object a class
//! stores: its spec, its box and its state, destroyed or not).

use mvs_sim::Real;
use mvs_sim::fighter::StagePlatform;
use mvs_sim::modes::tutorial::advanced::geometry::solids::{Gone, Held, SolidClass};
use mvs_sim::modes::tutorial::advanced::geometry::{
    FastFall, FastFallPlatform, KbiDemo, KbiMinigame, KbiSideDemo, KbiSideMinigame, KbiUpDemo, KbiUpMinigame,
    SpawnPlatform, SpawnWall, WallSlide,
};
use mvs_sim::world::World;
use mvs_sim::world::stage::{FLOOR_FLAGS, RuntimeBox, RuntimeSolid};
use mvs_trace::{Obj, Value};

use super::payload::{place_load, place_put};
use crate::conv::{Plain, as_obj, check_keys, obj};
use crate::cx::{Lx, Wx};

fn num(v: Real, x: &Wx) -> Value {
    v.to_value(x)
}

fn list3(v: [Real; 3], x: &Wx) -> Value {
    Value::List(v.iter().map(|&c| num(c, x)).collect())
}

fn strs(v: &[&str]) -> Value {
    Value::List(v.iter().map(|s| Value::Str((*s).to_owned())).collect())
}

/// A key of `ob` loaded through `f`, when the record has it.
fn at_key(ob: &Obj, k: &str, l: &mut Lx, f: impl FnOnce(&Value, &mut Lx)) {
    if let Some(v) = ob.get(k) {
        l.at(k, |l| f(v, l));
    }
}

// ---- a held solid

/// The solid a [`Held`] reads: the live one in the World's list, or the one it was when destroyed.
fn solid_of<'a>(h: &'a Held, w: &'a World) -> Option<&'a RuntimeSolid> {
    match &h.gone {
        Some(g) => Some(&g.solid),
        None => w.runtime_solids.iter().flatten().find(|s| s.spec == h.spec && !s.destroyed),
    }
}

/// `RuntimeSolidSpec`'s record: class, profile, centre, half extent, component, mesh, materials and the mesh
/// component's own transform; `rot` only for a non-zero rotation.
fn spec_put(h: &Held, x: &Wx) -> Value {
    let c = h.class;
    let mut o = vec![
        ("center", list3(h.center, x)),
        ("cls", Value::Str(c.cls().to_owned())),
        ("component", Value::Str(x.str(c.component()).to_owned())),
        ("halfExtent", list3(c.half(), x)),
        ("materials", strs(&[c.material()])),
        ("mesh", Value::Str(c.mesh().to_owned())),
        ("meshLoc", list3([0.0; 3], x)),
        ("meshRot", list3(c.mesh_rot(), x)),
        ("meshScale", list3(c.mesh_scale(), x)),
        ("profile", Value::Str(if c.floor() { "Floor" } else { "Platform" }.to_owned())),
    ];
    if let Some(r) = h.rot {
        o.push(("rot", list3(r, x)));
    }
    obj(o)
}

/// The solid's box: a Floor box (`RuntimeBox`) or a platform of `Arena2d`.
fn box_put(h: &Held, s: &RuntimeSolid, w: &World, x: &Wx) -> Value {
    let dead = h.gone.is_some();
    match s.solid {
        RuntimeBox::Platform(i) => {
            let p = match &h.gone {
                Some(g) => g.platform.as_ref(),
                None => w.stage.stage_platforms.get(i as usize),
            };
            let Some(p) = p else { return Value::Null };
            obj(vec![
                ("bottom", num(p.bottom, x)),
                ("dead", Value::Bool(dead)),
                ("dx", num(p.dx, x)),
                ("dy", num(p.dy, x)),
                ("left", num(p.left, x)),
                ("right", num(p.right, x)),
                ("stageIndex", Value::Num(f64::from(p.stage_index))),
                ("top", num(p.top, x)),
                ("vx", num(p.vx, x)),
                ("vy", num(p.vy, x)),
            ])
        }
        RuntimeBox::Floor | RuntimeBox::None => obj(vec![
            ("b0", num(s.b0, x)),
            ("dead", Value::Bool(dead)),
            ("flags", Value::Num(f64::from(s.flags))),
            ("l0", num(s.l0, x)),
            ("r0", num(s.r0, x)),
            ("s", s.state.to_value(x)),
            ("t0", num(s.t0, x)),
        ]),
    }
}

/// A held `RuntimeSolid | null` as the oracle writes the object.
pub fn held_put(h: &Option<Held>, w: &World, x: &Wx) -> Value {
    let Some(h) = h else { return Value::Null };
    let Some(s) = solid_of(h, w) else {
        x.unrecorded("a held solid the World lacks");
        return Value::Null;
    };
    obj(vec![
        ("b0", num(s.b0, x)),
        ("box", box_put(h, s, w, x)),
        ("destroyed", Value::Bool(h.gone.is_some())),
        ("l0", num(s.l0, x)),
        ("r0", num(s.r0, x)),
        ("spec", spec_put(h, x)),
        ("state", s.state.to_value(x)),
        ("t0", num(s.t0, x)),
        ("x0", num(s.x0, x)),
        ("y0", num(s.y0, x)),
    ])
}

/// A [`Held`] of the record's object: a live one points at the solid `make_solids` made for its box, a destroyed one
/// holds the solid as it was (its fields are loaded after).
fn held_make(ob: &Obj, dead: bool, l: &mut Lx) -> Option<Held> {
    let sv = ob.get("spec").and_then(|v| as_obj(v, l))?;
    let class = match sv.get("cls").and_then(|v| crate::conv::text(v, l)) {
        Some("TutorialWall_C") => SolidClass::Wall,
        Some("TutorialWallShort_C") => SolidClass::WallShort,
        Some("TutorialPlatform_C") => SolidClass::Platform,
        c => {
            l.miss(format_args!("no rule to make a held solid of class {c:?}"));
            return None;
        }
    };
    let triple = |k: &str, l: &mut Lx| -> Option<[Real; 3]> {
        let Value::List(a) = sv.get(k)? else { return None };
        let c: Vec<f64> = a.iter().filter_map(|v| crate::conv::num(v, l)).collect();
        Some([mvs_sim::math::narrow(*c.first()?), mvs_sim::math::narrow(*c.get(1)?), mvs_sim::math::narrow(*c.get(2)?)])
    };
    let center = triple("center", l)?;
    let rot = triple("rot", l);
    if dead {
        let solid = RuntimeSolid {
            spec: i32::MIN,
            component: class.component(),
            solid: if class.floor() { RuntimeBox::Floor } else { RuntimeBox::Platform(0) },
            flags: if class.floor() { FLOOR_FLAGS } else { 0 },
            destroyed: true,
            ..RuntimeSolid::default()
        };
        let platform = (!class.floor()).then(StagePlatform::default);
        return Some(Held { class, center, rot, spec: i32::MIN, gone: Some(Box::new(Gone { solid, platform })) });
    }
    let at = ["l0", "r0", "t0", "b0"].map(|k| ob.get(k));
    let place = l
        .runtime_places
        .iter()
        .position(|p| p.iter().zip(&at).all(|(c, v)| matches!(v, Some(Value::Num(x)) if x == c)));
    let Some(place) = place else {
        l.miss("a live held solid whose box is in no entry of the frame's runtimeSolids");
        return None;
    };
    Some(Held { class, center, rot, spec: super::spec_at(place), gone: None })
}

/// [`held_put`]'s inverse. A live solid is loaded with the World's list (`runtimeSolids`); a destroyed one's last state
/// is the held snapshot's alone.
pub fn held_load(h: &mut Option<Held>, v: &Value, l: &mut Lx) {
    if *v == Value::Null {
        *h = None;
        return;
    }
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["b0", "box", "destroyed", "l0", "r0", "spec", "state", "t0", "x0", "y0"], l);
    let dead = matches!(ob.get("destroyed"), Some(Value::Bool(true)));
    // The reference is made from the record (its class, place and, live, its solid's number from the frame's list).
    let Some(made) = held_make(ob, dead, l) else { return };
    let held = h.insert(made);
    let Some(g) = &mut held.gone else { return };
    let s = &mut g.solid;
    for (k, out) in [
        ("b0", &mut s.b0),
        ("l0", &mut s.l0),
        ("r0", &mut s.r0),
        ("t0", &mut s.t0),
        ("x0", &mut s.x0),
        ("y0", &mut s.y0),
    ] {
        at_key(ob, k, l, |v, l| out.load_value(v, l));
    }
    at_key(ob, "state", l, |v, l| s.state.load_value(v, l));
    if let (Some(p), Some(bv)) = (&mut g.platform, ob.get("box").and_then(|v| as_obj(v, l))) {
        for (k, out) in [
            ("bottom", &mut p.bottom),
            ("dx", &mut p.dx),
            ("dy", &mut p.dy),
            ("left", &mut p.left),
            ("right", &mut p.right),
            ("top", &mut p.top),
            ("vx", &mut p.vx),
            ("vy", &mut p.vy),
        ] {
            at_key(bv, k, l, |v, l| out.load_value(v, l));
        }
    }
}

// ---- the demo controllers

pub fn wall_slide_put(d: &WallSlide, w: &World, x: &Wx) -> Value {
    obj(vec![("bag", d.bag.to_value(x)), ("wall", held_put(&d.wall, w, x))])
}

pub fn wall_slide_load(d: &mut WallSlide, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["bag", "wall"], l);
    at_key(ob, "bag", l, |v, l| d.bag.load_value(v, l));
    at_key(ob, "wall", l, |v, l| held_load(&mut d.wall, v, l));
}

pub fn fast_fall_put(d: &FastFall, x: &Wx) -> Value {
    obj(vec![("fighter", d.fighter.to_value(x)), ("jumpAgain", Value::Bool(d.jump_again))])
}

pub fn fast_fall_load(d: &mut FastFall, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["fighter", "jumpAgain"], l);
    at_key(ob, "fighter", l, |v, l| d.fighter.load_value(v, l));
    at_key(ob, "jumpAgain", l, |v, l| d.jump_again.load_value(v, l));
}

pub fn fast_fall_platform_put(d: &FastFallPlatform, w: &World, x: &Wx) -> Value {
    obj(vec![("bag", d.bag.to_value(x)), ("platform", held_put(&d.platform, w, x))])
}

pub fn fast_fall_platform_load(d: &mut FastFallPlatform, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["bag", "platform"], l);
    at_key(ob, "bag", l, |v, l| d.bag.load_value(v, l));
    at_key(ob, "platform", l, |v, l| held_load(&mut d.platform, v, l));
}

/// The KBI demos' shared keys, pushed on `o`.
fn kbi_demo_put(d: &KbiDemo, x: &Wx, o: &mut Vec<(&'static str, Value)>) {
    o.push(("at", place_put(d.at, x)));
    o.push(("bag", d.bag.to_value(x)));
    o.push(("complete", Value::Bool(d.complete)));
    o.push(("fighter", d.fighter.to_value(x)));
    o.push(("killed", Value::Num(f64::from(d.killed))));
}

fn kbi_demo_load(d: &mut KbiDemo, ob: &Obj, l: &mut Lx) {
    at_key(ob, "at", l, |v, l| place_load(&mut d.at, v, l));
    at_key(ob, "bag", l, |v, l| d.bag.load_value(v, l));
    at_key(ob, "complete", l, |v, l| d.complete.load_value(v, l));
    at_key(ob, "fighter", l, |v, l| d.fighter.load_value(v, l));
    at_key(ob, "killed", l, |v, l| d.killed.load_value(v, l));
}

pub fn kbi_side_demo_put(d: &KbiSideDemo, x: &Wx) -> Value {
    let mut o = Vec::new();
    kbi_demo_put(&d.k, x, &mut o);
    obj(o)
}

pub fn kbi_side_demo_load(d: &mut KbiSideDemo, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["at", "bag", "complete", "fighter", "killed"], l);
    kbi_demo_load(&mut d.k, ob, l);
}

pub fn kbi_up_demo_put(d: &KbiUpDemo, w: &World, x: &Wx) -> Value {
    let mut o = Vec::new();
    kbi_demo_put(&d.k, x, &mut o);
    o.push(("wall", held_put(&d.wall, w, x)));
    obj(o)
}

pub fn kbi_up_demo_load(d: &mut KbiUpDemo, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["at", "bag", "complete", "fighter", "killed", "wall"], l);
    kbi_demo_load(&mut d.k, ob, l);
    at_key(ob, "wall", l, |v, l| held_load(&mut d.wall, v, l));
}

// ---- the minigame actors

pub fn spawn_wall_put(d: &SpawnWall, w: &World, x: &Wx) -> Value {
    obj(vec![("wall", held_put(&d.wall, w, x))])
}

pub fn spawn_wall_load(d: &mut SpawnWall, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["wall"], l);
    at_key(ob, "wall", l, |v, l| held_load(&mut d.wall, v, l));
}

pub fn spawn_platform_put(d: &SpawnPlatform, w: &World, x: &Wx) -> Value {
    obj(vec![("platform", held_put(&d.platform, w, x))])
}

pub fn spawn_platform_load(d: &mut SpawnPlatform, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["platform"], l);
    at_key(ob, "platform", l, |v, l| held_load(&mut d.platform, v, l));
}

/// The KBI minigames' shared keys (`handlers` is a WeakMap: `{}`), pushed on `o`.
fn kbi_put(d: &KbiMinigame, w: &World, x: &Wx, o: &mut Vec<(&'static str, Value)>) {
    o.push(("attacked", Value::Bool(d.attacked)));
    o.push(("attackee", d.attackee.to_value(x)));
    o.push(("handlers", obj(vec![])));
    o.push(("player", d.player.to_value(x)));
    o.push(("tutorialComplete", Value::Bool(d.tutorial_complete)));
    o.push(("wall", held_put(&d.wall, w, x)));
}

fn kbi_load(d: &mut KbiMinigame, ob: &Obj, l: &mut Lx) {
    at_key(ob, "attacked", l, |v, l| d.attacked.load_value(v, l));
    at_key(ob, "attackee", l, |v, l| d.attackee.load_value(v, l));
    at_key(ob, "player", l, |v, l| d.player.load_value(v, l));
    at_key(ob, "tutorialComplete", l, |v, l| d.tutorial_complete.load_value(v, l));
    at_key(ob, "wall", l, |v, l| held_load(&mut d.wall, v, l));
}

pub fn kbi_side_put(d: &KbiSideMinigame, w: &World, x: &Wx) -> Value {
    let mut o = Vec::new();
    kbi_put(&d.k, w, x, &mut o);
    o.push(("curveIndex", Value::Num(f64::from(d.curve_index))));
    o.push(("wallCount", Value::Num(f64::from(d.wall_count))));
    obj(o)
}

pub fn kbi_side_load(d: &mut KbiSideMinigame, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(
        ob,
        &["attacked", "attackee", "curveIndex", "handlers", "player", "tutorialComplete", "wall", "wallCount"],
        l,
    );
    kbi_load(&mut d.k, ob, l);
    at_key(ob, "curveIndex", l, |v, l| d.curve_index.load_value(v, l));
    at_key(ob, "wallCount", l, |v, l| d.wall_count.load_value(v, l));
}

pub fn kbi_up_put(d: &KbiUpMinigame, w: &World, x: &Wx) -> Value {
    let mut o = Vec::new();
    kbi_put(&d.k, w, x, &mut o);
    obj(o)
}

pub fn kbi_up_load(d: &mut KbiUpMinigame, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["attacked", "attackee", "handlers", "player", "tutorialComplete", "wall"], l);
    kbi_load(&mut d.k, ob, l);
}
