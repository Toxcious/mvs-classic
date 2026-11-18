//! Scooby's Haunted Mansion (`stages/scoobys-haunted-mansion/`, Map_ScoobyDoo, Map_ScoobyDoo_NoRoof): the entry, its own
//! state and its placed actors' classes (`actors.rs`): the two breakable walls, the two elevators and their two
//! switches. The three ScoobyFloors are static solids; the level scripts' ghost and music, the lightning and the
//! statues are the drawing's and the audio's. Nothing ticks: the switches' moves and Delays and the elevators' Delays
//! run in the leftover pass. No reset: only a new level restores the switches, walls and roots, and a match is a new
//! World.

mod actors;

use std::sync::Arc;

use crate::State;
use crate::actors::StageActorClass;
use crate::registry::entries::StageEntry;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::stage::StageOwn;

pub use actors::{ScoobyElevator, ScoobySwitch, ScoobyWall};

/// `MansionState`: the entry's own state (`StageRun.s`), its `StageOwn`
/// variant: the elevators' owner numbers (StageData.placed index) in level order (the dust test's order).
#[derive(State, Debug, Default)]
pub struct MansionOwn {
    pub elevators: Vec<i32>,
}

/// The levels with this entry.
const LEVELS: [&str; 2] = ["Map_ScoobyDoo", "Map_ScoobyDoo_NoRoof"];

/// The level entries (`stages({ ... })`) and the classes' (`stageActors({ ... })`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.stage_actors.enter(actors::WALL, actors::wall_entry())?;
    r.stage_actors.enter(actors::ELEVATOR, actors::elevator_entry())?;
    r.stage_actors.enter(actors::SWITCH, actors::switch_entry())?;
    for name in LEVELS {
        r.stages.enter(
            name,
            StageEntry { create: Some(create), tick: Some(tick), timer: Some(timer), ..StageEntry::default() },
        )?;
    }
    Ok(())
}

/// The switches' ConnectedScoobyElevator / ConnectedScoobyWall (the level's references: Switch1 -> Elevator2 and the
/// left wall, Switch2 -> Elevator1 and the right wall), then BeginPlay's AllowHazards test (@2630 -> @389 -> @501).
fn create(w: &mut World) {
    let data = Arc::clone(&w.data);
    let placed = data.stage.placed.as_deref().unwrap_or(&[]);
    let by_name = |n: &str| -> i32 {
        if n.is_empty() {
            return -1;
        }
        placed.iter().position(|d| d.name == n).map_or(-1, |i| i32::try_from(i).expect("a placed index"))
    };
    let allow = w.setup.allow_hazards;
    let n = w.stage_run.as_ref().expect("a stage state").actors.len();
    let mut elevators = Vec::new();
    for slot in 0..n {
        let slot = u32::try_from(slot).expect("a list index");
        let a = &w.stage_run.as_ref().expect("a stage state").actors[slot as usize];
        let index = a.index;
        if a.cls == StageActorClass::ScoobyElevator {
            elevators.push(i32::try_from(index).expect("a placed index"));
        }
        if a.cls != StageActorClass::ScoobyElevatorSwitch {
            continue;
        }
        let d = &placed[index as usize];
        let s = actors::switch_mut(w, slot);
        s.elevator = by_name(actors::ref_actor(d.p.get("ConnectedScoobyElevator")));
        s.wall = by_name(actors::ref_actor(d.p.get("ConnectedScoobyWall")));
        if !allow {
            actors::destroy_actor(w, slot);
        }
    }
    w.stage_run.as_mut().expect("a stage state").s = StageOwn::Mansion(MansionOwn { elevators });
}

/// The elevators' DustVFXCollision overlaps, behind the frame's moves (`actors::elevator_dust`).
fn tick(w: &mut World) {
    let elevators = match &w.stage_run.as_ref().expect("a stage state").s {
        StageOwn::Mansion(s) => s.elevators.clone(),
        _ => panic!("Scooby's Haunted Mansion: the stage's own state is another stage's"),
    };
    for i in elevators {
        if let Some(slot) = actors::slot_of(w, i) {
            actors::elevator_dust(w, slot);
        }
    }
}

/// The links: a switch's MoveComponentTo, Delay(0) and Delay(0.35); an elevator's Delay(1.5).
fn timer(w: &mut World, ev: i32, owner: i32) {
    let Some(slot) = actors::slot_of(w, owner) else { return };
    let cls = w.stage_run.as_ref().expect("a stage state").actors[slot as usize].cls;
    if cls == StageActorClass::ScoobyElevatorSwitch {
        match ev {
            actors::MOVE_LINK => actors::switch_move_done(w, slot),
            actors::STOP_LINK => actors::switch_stop_link(w, slot),
            actors::CHAIN_LINK => actors::switch_chain_link(w, slot),
            _ => {}
        }
    } else if cls == StageActorClass::ScoobyElevator && ev == actors::DUST_LINK {
        actors::elevator_dust_link(w, slot);
    }
}
