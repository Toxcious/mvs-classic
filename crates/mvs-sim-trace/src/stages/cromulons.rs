//! Cromulons's state as the record writes it (`stages/cromulons/`): the entry's own state (`StageRun.s`, `{ tick }`)
//! and a speaker's own keys (the `WallActor` ones, `wall.rs`, then the speaker's).

use mvs_sim::stages::cromulons::{CromulonsOwn, Speaker};
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(CromulonsOwn { "tick" = tick });

/// `StageRun.s` of Cromulons.
pub fn own_put(s: &CromulonsOwn, x: &Wx) -> Value {
    s.to_value(x)
}

pub fn own_load(s: &mut CromulonsOwn, v: &Value, l: &mut Lx) {
    s.load_value(v, l);
}

/// A `Speaker`'s own keys (beside the `StageActor` ones), pushed on `o`.
pub fn speaker_put(a: &Speaker, x: &Wx, o: &mut Vec<(String, Value)>) {
    super::wall::wall_put(&a.wall, x, o);
    plain_keys!(put a, x, o;
        "isChild" = is_child, "mesh" = mesh, "meshAt" = mesh_at, "thrusters" = thrusters, "sparks" = sparks,
        "causer" = causer, "once1" = once1, "once2" = once2, "onceA" = once_a, "onceB" = once_b, "edges" = edges,
    );
}

/// Loads them from `ob`, each key read pushed on `known`.
pub fn speaker_load(a: &mut Speaker, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    super::wall::wall_load(&mut a.wall, ob, l, known);
    plain_keys!(load a, ob, l;
        "isChild" = is_child, "mesh" = mesh, "meshAt" = mesh_at, "thrusters" = thrusters, "sparks" = sparks,
        "causer" = causer, "once1" = once1, "once2" = once2, "onceA" = once_a, "onceB" = once_b, "edges" = edges,
    );
    known.extend(plain_keys!(names
        "isChild" = is_child, "mesh" = mesh, "meshAt" = mesh_at, "thrusters" = thrusters, "sparks" = sparks,
        "causer" = causer, "once1" = once1, "once2" = once2, "onceA" = once_a, "onceB" = once_b, "edges" = edges,
    ));
}
