//! The `WallActor` part of a stage actor's keys: a Cromulons speaker's and a Batcave car's
//! shared wall fields, which the speaker's and the car's own writers call. `listeners` is written once a bind made the
//! list (the oracle's `undefined` until then), each `{d, fn, self}` with `self` by reference.

// Until the speaker's and the car's writers (their stage tracks) call them.
#![allow(dead_code, reason = "called by the stage tracks' writers once merged")]

use mvs_sim::actors::ActorRef;
use mvs_sim::world::wall_bp::{WallDelegate, WallListener, WallPart};
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, as_list, as_obj, check_keys, obj, text};
use crate::cx::{Lx, Wx};

fn delegate_name(d: WallDelegate) -> &'static str {
    match d {
        WallDelegate::ObjectDestroyed => "ObjectDestroyed",
        WallDelegate::StructureDestroyed => "StructureDestroyed",
    }
}

/// A `WallActor`'s own keys (beside the `StageActor` ones), pushed on `o`.
pub fn wall_put(p: &WallPart, x: &Wx, o: &mut Vec<(String, Value)>) {
    let mut put = |k: &str, v: Value| o.push((k.to_owned(), v));
    put("hp", p.hp.to_value(x));
    put("stage", p.stage.to_value(x));
    put("destroyed", p.destroyed.to_value(x));
    put("damageMontage", Value::Str(p.damage_montage.clone()));
    put("destroyMontage", Value::Str(p.destroy_montage.clone()));
    put("hitAt", p.hit_at.to_value(x));
    put("hitMontage", Value::Str(p.hit_montage.clone()));
    put("brokenAt", p.broken_at.to_value(x));
    put("offAt", p.off_at.to_value(x));
    if let Some(ls) = &p.listeners {
        let v = ls
            .iter()
            .map(|l| {
                obj(vec![
                    ("d", Value::Str(delegate_name(l.d).to_owned())),
                    ("fn", Value::Str(l.f.to_owned())),
                    ("self", l.listener.to_value(x)),
                ])
            })
            .collect();
        put("listeners", Value::List(v));
    }
}

/// Loads them from `ob`, each key read pushed on `known`.
pub fn wall_load(p: &mut WallPart, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    known.extend([
        "hp",
        "stage",
        "destroyed",
        "damageMontage",
        "destroyMontage",
        "hitAt",
        "hitMontage",
        "brokenAt",
        "offAt",
        "listeners",
    ]);
    let key = |n: &'static str, l: &mut Lx, set: &mut dyn FnMut(&Value, &mut Lx)| match ob.get(n) {
        Some(v) => l.at(n, |l| set(v, l)),
        None => l.miss(format_args!("key {n} absent")),
    };
    key("hp", l, &mut |v, l| p.hp.load_value(v, l));
    key("stage", l, &mut |v, l| p.stage.load_value(v, l));
    key("destroyed", l, &mut |v, l| p.destroyed.load_value(v, l));
    key("damageMontage", l, &mut |v, l| load_str(&mut p.damage_montage, v, l));
    key("destroyMontage", l, &mut |v, l| load_str(&mut p.destroy_montage, v, l));
    key("hitAt", l, &mut |v, l| p.hit_at.load_value(v, l));
    key("hitMontage", l, &mut |v, l| load_str(&mut p.hit_montage, v, l));
    key("brokenAt", l, &mut |v, l| p.broken_at.load_value(v, l));
    key("offAt", l, &mut |v, l| p.off_at.load_value(v, l));
    p.listeners = ob.get("listeners").map(|v| l.at("listeners", |l| listeners_load(v, l)));
}

fn load_str(s: &mut String, v: &Value, l: &mut Lx) {
    if let Some(t) = text(v, l) {
        t.clone_into(s);
    }
}

fn listeners_load(v: &Value, l: &mut Lx) -> Vec<WallListener> {
    let Some(a) = as_list(v, l) else { return Vec::new() };
    let table = &mvs_sim::registry::registry().wall_listeners;
    let mut out = Vec::with_capacity(a.len());
    for (i, e) in a.iter().enumerate() {
        l.at(&format!("[{i}]"), |l| {
            let Some(o) = as_obj(e, l) else { return };
            check_keys(o, &["d", "fn", "self"], l);
            let d = match o.get("d").and_then(|v| text(v, l)) {
                Some("ObjectDestroyed") => WallDelegate::ObjectDestroyed,
                Some("StructureDestroyed") => WallDelegate::StructureDestroyed,
                d => return l.miss(format_args!("a wall delegate {d:?}")),
            };
            let Some(name) = o.get("fn").and_then(|v| text(v, l)) else { return };
            let Some(f) = table.names().find(|n| *n == name) else {
                return l.miss(format_args!("wall listener {name} is not entered"));
            };
            let Some(listener) = o.get("self").and_then(|v| ActorRef::from_value(v, l)) else { return };
            out.push(WallListener { d, listener, f });
        });
    }
    out
}
