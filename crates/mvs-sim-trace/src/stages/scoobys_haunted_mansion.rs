//! Scooby's Haunted Mansion's state as the record writes it (`stages/scoobys-haunted-mansion/`): the entry's own state
//! (`StageRun.s`) and its placed actors' own keys.

use mvs_sim::stages::scoobys_haunted_mansion::{MansionOwn, ScoobyElevator, ScoobySwitch, ScoobyWall};
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(MansionOwn { "elevators" = elevators });

/// `StageRun.s` of Scooby's Haunted Mansion.
pub fn own_put(s: &MansionOwn, x: &Wx) -> Value {
    s.to_value(x)
}

pub fn own_load(s: &mut MansionOwn, v: &Value, l: &mut Lx) {
    s.load_value(v, l);
}

/// A `ScoobyWall`'s own keys (beside the `StageActor` ones), pushed on `o`.
pub fn scooby_wall_put(a: &ScoobyWall, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put a, x, o;
        "hp" = hp, "hitAt" = hit_at, "brokenAt" = broken_at, "gearAt" = gear_at, "gearUp" = gear_up,);
}

/// Loads them from `ob`, each key read pushed on `known`.
pub fn scooby_wall_load(a: &mut ScoobyWall, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load a, ob, l;
        "hp" = hp, "hitAt" = hit_at, "brokenAt" = broken_at, "gearAt" = gear_at, "gearUp" = gear_up,);
    known.extend(plain_keys!(names
        "hp" = hp, "hitAt" = hit_at, "brokenAt" = broken_at, "gearAt" = gear_at, "gearUp" = gear_up,));
}

/// A `ScoobyElevator`'s own keys (beside the `StageActor` ones), pushed on `o`.
pub fn scooby_elevator_put(a: &ScoobyElevator, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put a, x, o;
        "moveAt" = move_at, "moveUp" = move_up, "emissive" = emissive, "dustGate" = dust_gate, "dustAt" = dust_at,
        "dustX" = dust_x, "dustY" = dust_y, "edgeUpdate" = edge_update, "dustIn" = dust_in,);
}

/// Loads them from `ob`, each key read pushed on `known`.
pub fn scooby_elevator_load(a: &mut ScoobyElevator, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load a, ob, l;
        "moveAt" = move_at, "moveUp" = move_up, "emissive" = emissive, "dustGate" = dust_gate, "dustAt" = dust_at,
        "dustX" = dust_x, "dustY" = dust_y, "edgeUpdate" = edge_update, "dustIn" = dust_in,);
    known.extend(plain_keys!(names
        "moveAt" = move_at, "moveUp" = move_up, "emissive" = emissive, "dustGate" = dust_gate, "dustAt" = dust_at,
        "dustX" = dust_x, "dustY" = dust_y, "edgeUpdate" = edge_update, "dustIn" = dust_in,));
}

/// A `ScoobySwitch`'s own keys (beside the `StageActor` ones), pushed on `o`.
pub fn scooby_switch_put(a: &ScoobySwitch, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put a, x, o;
        "state" = state, "enabled" = enabled, "overlaps" = overlaps, "elevator" = elevator, "wall" = wall,
        "moveAt" = move_at, "moveUp" = move_up, "chain1At" = chain1_at, "doneAt" = done_at,);
}

/// Loads them from `ob`, each key read pushed on `known`.
pub fn scooby_switch_load(a: &mut ScoobySwitch, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load a, ob, l;
        "state" = state, "enabled" = enabled, "overlaps" = overlaps, "elevator" = elevator, "wall" = wall,
        "moveAt" = move_at, "moveUp" = move_up, "chain1At" = chain1_at, "doneAt" = done_at,);
    known.extend(plain_keys!(names
        "state" = state, "enabled" = enabled, "overlaps" = overlaps, "elevator" = elevator, "wall" = wall,
        "moveAt" = move_at, "moveUp" = move_up, "chain1At" = chain1_at, "doneAt" = done_at,));
}
