//! Throne Room's state as the record writes it (`stages/throne-room/`): the entry's own state (`StageRun.s`) and its
//! placed actors' own keys.

use mvs_sim::stages::throne_room::{M008Elevator, M008Presentation, M008Script, ThroneRoomOwn};
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(M008Presentation {
    "introGears" = intro_gears, "introCentre" = intro_centre, "introSides" = intro_sides, "introThrone" = intro_throne,
    "transform" = transform, "floorFold" = floor_fold, "towerFold" = tower_fold, "wallGears" = wall_gears,
    "elevatorFold" = elevator_fold, "stageFold" = stage_fold, "reverse" = reverse,
});
plain_struct!(M008Script { "owner" = owner, "shown" = shown });
plain_struct!(ThroneRoomOwn { "scripts" = scripts, "movers" = movers, "allowHazards" = allow_hazards });

/// `StageRun.s` of Throne Room.
pub fn own_put(s: &ThroneRoomOwn, x: &Wx) -> Value {
    s.to_value(x)
}

pub fn own_load(s: &mut ThroneRoomOwn, v: &Value, l: &mut Lx) {
    s.load_value(v, l);
}

/// A `M008Elevator`'s own keys (beside the `StageActor` ones), pushed on `o`.
pub fn m008_elevator_put(a: &M008Elevator, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put a, x, o; "role" = role, "floor" = floor, "rootZ" = root_z, "edgeUpdate" = edge_update,);
}

/// Loads them from `ob`, each key read pushed on `known`.
pub fn m008_elevator_load(a: &mut M008Elevator, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load a, ob, l; "role" = role, "floor" = floor, "rootZ" = root_z, "edgeUpdate" = edge_update,);
    known.extend(plain_keys!(names "role" = role, "floor" = floor, "rootZ" = root_z, "edgeUpdate" = edge_update,));
}
