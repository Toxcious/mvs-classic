//! The Meeseeks family's own keys as the record writes them: each
//! class's fields under the oracle names, and Tutorial_Meeseeks' own keys beside the `StageActor` ones. What the oracle
//! holds in its `Binds` object (the hit list, the minigame, the OnDestroyed flag) is not written, and stays the World's
//! in a load.

use mvs_sim::modes::tutorial::advanced::meeseeks::{
    AerialCombat, AerialCombatBasics, AerialCombatHighUp, EvadeToAttack, MeeseeksEnd, MeeseeksOwn, PositioningTools,
};
use mvs_trace::{Obj, Value};

use super::payload::{place_load, place_put};
use crate::conv::{Plain, as_obj, check_keys, obj};
use crate::cx::{Lx, Wx};

/// A key of `ob` loaded through `f`, when the record has it.
fn at_key(ob: &Obj, k: &str, l: &mut Lx, f: impl FnOnce(&Value, &mut Lx)) {
    if let Some(v) = ob.get(k) {
        l.at(k, |l| f(v, l));
    }
}

// ---- the demo controllers

pub fn basics_put(d: &AerialCombatBasics, x: &Wx) -> Value {
    obj(vec![("at", place_put(d.at, x)), ("p", d.p.to_value(x))])
}

pub fn basics_load(d: &mut AerialCombatBasics, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["at", "p"], l);
    at_key(ob, "at", l, |v, l| place_load(&mut d.at, v, l));
    at_key(ob, "p", l, |v, l| d.p.load_value(v, l));
}

pub fn high_up_put(d: &AerialCombatHighUp, x: &Wx) -> Value {
    obj(vec![("p", d.p.to_value(x))])
}

pub fn high_up_load(d: &mut AerialCombatHighUp, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["p"], l);
    at_key(ob, "p", l, |v, l| d.p.load_value(v, l));
}

pub fn evade_put(d: &EvadeToAttack, x: &Wx) -> Value {
    obj(vec![
        ("at", place_put(d.at, x)),
        ("cast", d.cast.to_value(x)),
        ("hit", d.hit.to_value(x)),
        ("last", d.last.to_value(x)),
    ])
}

pub fn evade_load(d: &mut EvadeToAttack, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["at", "cast", "hit", "last"], l);
    at_key(ob, "at", l, |v, l| place_load(&mut d.at, v, l));
    at_key(ob, "cast", l, |v, l| d.cast.load_value(v, l));
    at_key(ob, "hit", l, |v, l| d.hit.load_value(v, l));
    at_key(ob, "last", l, |v, l| d.last.load_value(v, l));
}

// ---- the minigames

pub fn aerial_combat_put(d: &AerialCombat, x: &Wx) -> Value {
    obj(vec![("targets", d.targets.to_value(x))])
}

pub fn aerial_combat_load(d: &mut AerialCombat, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["targets"], l);
    at_key(ob, "targets", l, |v, l| d.targets.load_value(v, l));
}

pub fn positioning_put(d: &PositioningTools, x: &Wx) -> Value {
    obj(vec![
        ("caught", d.caught.to_value(x)),
        ("last", d.last.to_value(x)),
        ("minTime", d.min_time.to_value(x)),
        ("once", d.once.to_value(x)),
        ("over", d.over.to_value(x)),
        ("stage", d.stage.to_value(x)),
        ("start", d.start.to_value(x)),
        ("survival", d.survival.to_value(x)),
    ])
}

pub fn positioning_load(d: &mut PositioningTools, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &["caught", "last", "minTime", "once", "over", "stage", "start", "survival"], l);
    at_key(ob, "caught", l, |v, l| d.caught.load_value(v, l));
    at_key(ob, "last", l, |v, l| d.last.load_value(v, l));
    at_key(ob, "minTime", l, |v, l| d.min_time.load_value(v, l));
    at_key(ob, "once", l, |v, l| d.once.load_value(v, l));
    at_key(ob, "over", l, |v, l| d.over.load_value(v, l));
    at_key(ob, "stage", l, |v, l| d.stage.load_value(v, l));
    at_key(ob, "start", l, |v, l| d.start.load_value(v, l));
    at_key(ob, "survival", l, |v, l| d.survival.load_value(v, l));
}

// ---- the stage actor

/// A Tutorial_Meeseeks' own keys, pushed on `o`.
pub fn actor_put(a: &MeeseeksOwn, x: &Wx, o: &mut Vec<(String, Value)>) {
    o.push(("end".to_owned(), a.end.code().to_value(x)));
    o.push(("flying".to_owned(), a.flying.to_value(x)));
    o.push(("shown".to_owned(), a.shown.to_value(x)));
    o.push(("spawned".to_owned(), a.spawned.to_value(x)));
}

/// Loads them from `ob`, each key read pushed on `known`.
pub fn actor_load(a: &mut MeeseeksOwn, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    known.extend(["end", "flying", "shown", "spawned"]);
    at_key(ob, "end", l, |v, l| {
        let mut c = 0u8;
        c.load_value(v, l);
        match MeeseeksEnd::of_code(c) {
            Some(e) => a.end = e,
            None => l.miss("a Meeseeks end that is none of 0 to 3"),
        }
    });
    at_key(ob, "flying", l, |v, l| a.flying.load_value(v, l));
    at_key(ob, "shown", l, |v, l| a.shown.load_value(v, l));
    at_key(ob, "spawned", l, |v, l| a.spawned.load_value(v, l));
}
