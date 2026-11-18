//! The stage's run as the record writes it: `World.stageRun` as the `stageRun` key of the
//! World's rest, a plain value of every key: `s` (the entry's own state), `timers`, `solids`, `boxes` and `actors`
//! (each placed actor whole: the `StageActor` keys and its class's own keys). A stage actor met anywhere else is a
//! reference, `{ref: "stage", id: index}` (`cx.rs` `aref`).
//!
//! The run's shared parts, the platforms' run and the `stageRun` key of the World's rest are in `run.rs`; each stage
//! has its own file below, which the dispatch here calls; `wall.rs` holds (the `WallActor` part of a speaker's and a
//! car's keys).

mod batcave;
mod cromulons;
mod run;
mod scoobys_haunted_mansion;
mod the_court;
mod throne_room;
mod tree_fort;
pub mod wall;

pub use run::{box_put, platform_put, platforms_load, platforms_put, run_load, run_put};

use mvs_sim::world::stage::StageOwn;
use mvs_sim::world::stage_actors::StageActorOwn;
use mvs_trace::{Obj, Value};

use crate::cx::{Lx, Wx};

/// `StageRun.s`: the entry's own state, null for a stage with none.
pub fn own_put(s: &StageOwn, x: &Wx) -> Value {
    match s {
        StageOwn::None => Value::Null,
        StageOwn::TreeFort(s) => tree_fort::own_put(s, x),
        StageOwn::Cromulons(s) => cromulons::own_put(s, x),
        StageOwn::Mansion(s) => scoobys_haunted_mansion::own_put(s, x),
        StageOwn::Batcave(s) => batcave::own_put(s, x),
        StageOwn::ThroneRoom(s) => throne_room::own_put(s, x),
        StageOwn::Court(s) => the_court::own_put(s, x),
    }
}

/// Loads `StageRun.s` into the variant the World's create made.
pub fn own_load(s: &mut StageOwn, v: &Value, l: &mut Lx) {
    match s {
        StageOwn::None => {
            if !matches!(v, Value::Null) {
                l.miss("a stage's own state where the World has none");
            }
        }
        StageOwn::TreeFort(s) => tree_fort::own_load(s, v, l),
        StageOwn::Cromulons(s) => cromulons::own_load(s, v, l),
        StageOwn::Mansion(s) => scoobys_haunted_mansion::own_load(s, v, l),
        StageOwn::Batcave(s) => batcave::own_load(s, v, l),
        StageOwn::ThroneRoom(s) => throne_room::own_load(s, v, l),
        StageOwn::Court(s) => the_court::own_load(s, v, l),
    }
}

/// A stage actor's class's own keys (beside `cls`, `dead`, `index`, `team`, `x`, `y`), pushed on `o`.
pub fn actor_own_put(a: &StageActorOwn, x: &Wx, o: &mut Vec<(String, Value)>) {
    match a {
        StageActorOwn::None => {}
        StageActorOwn::Pillar(a) => tree_fort::pillar_put(a, x, o),
        StageActorOwn::Speaker(a) => cromulons::speaker_put(a, x, o),
        StageActorOwn::ScoobyWall(a) => scoobys_haunted_mansion::scooby_wall_put(a, x, o),
        StageActorOwn::ScoobyElevator(a) => scoobys_haunted_mansion::scooby_elevator_put(a, x, o),
        StageActorOwn::ScoobySwitch(a) => scoobys_haunted_mansion::scooby_switch_put(a, x, o),
        StageActorOwn::BatcaveFog(a) => batcave::batcave_fog_put(a, x, o),
        StageActorOwn::BatcaveCar(a) => batcave::batcave_car_put(a, x, o),
        StageActorOwn::BatcaveBats(a) => batcave::batcave_bats_put(a, x, o),
        StageActorOwn::M008Elevator(a) => throne_room::m008_elevator_put(a, x, o),
        StageActorOwn::M010Hoop(a) => the_court::m010_hoop_put(a, x, o),
        StageActorOwn::M010Wall(a) => the_court::m010_wall_put(a, x, o),
        StageActorOwn::Meeseeks(a) => crate::tutorial::meeseeks::actor_put(a, x, o),
    }
}

/// Loads a stage actor's own keys into the variant its create made, each key read pushed on `known`.
pub fn actor_own_load(a: &mut StageActorOwn, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    match a {
        StageActorOwn::None => {}
        StageActorOwn::Pillar(a) => tree_fort::pillar_load(a, ob, l, known),
        StageActorOwn::Speaker(a) => cromulons::speaker_load(a, ob, l, known),
        StageActorOwn::ScoobyWall(a) => scoobys_haunted_mansion::scooby_wall_load(a, ob, l, known),
        StageActorOwn::ScoobyElevator(a) => scoobys_haunted_mansion::scooby_elevator_load(a, ob, l, known),
        StageActorOwn::ScoobySwitch(a) => scoobys_haunted_mansion::scooby_switch_load(a, ob, l, known),
        StageActorOwn::BatcaveFog(a) => batcave::batcave_fog_load(a, ob, l, known),
        StageActorOwn::BatcaveCar(a) => batcave::batcave_car_load(a, ob, l, known),
        StageActorOwn::BatcaveBats(a) => batcave::batcave_bats_load(a, ob, l, known),
        StageActorOwn::M008Elevator(a) => throne_room::m008_elevator_load(a, ob, l, known),
        StageActorOwn::M010Hoop(a) => the_court::m010_hoop_load(a, ob, l, known),
        StageActorOwn::M010Wall(a) => the_court::m010_wall_load(a, ob, l, known),
        StageActorOwn::Meeseeks(a) => crate::tutorial::meeseeks::actor_load(a, ob, l, known),
    }
}
