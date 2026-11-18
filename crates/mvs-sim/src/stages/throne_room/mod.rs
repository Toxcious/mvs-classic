//! Throne Room (`stages/throne-room/`, Map_M008): its level script (`script.rs`: three Map_M008_C instances, the move
//! schedule, the frames of the presentation calls; the stage is silent) and its five movers (`elevator.rs`,
//! M008_Elevator_BP_C). The central Stage box is a static solid; the four fire pillars
//! (M008_Pillar_01_SkelMesh*_Blueprint_C: a PS_M008_Fire at a socket, no hit) and the fold animations are the drawing's.
//! Nothing ticks: the movers' moves and the scripts' Delays run in the leftover pass. No ring-out handler and no reset:
//! a ring-out, a respawn or a new round changes nothing, and a match is a new World.

mod elevator;
mod script;

use crate::State;
use crate::registry::entries::StageEntry;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::stage_actors::StageActorOwn;

pub use elevator::M008Elevator;
pub use script::{M008Presentation, M008Script};

/// `M008State`: the entry's own state (`StageRun.s`), its `StageOwn` variant.
#[derive(State, Debug, Default)]
pub struct ThroneRoomOwn {
    pub scripts: Vec<M008Script>,
    /// The M008_Elevator_BP_C owner numbers (StageData.placed index) by role (`elevator::SIDE_LEFT` ..), -1 where none.
    pub movers: Vec<i32>,
    /// The match's AllowHazards (read once per loop after the side down requests).
    pub allow_hazards: bool,
}

/// The level entries (`stages({ ... })`) and the classes' (`stageActors({ ... })`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.stage_actors.enter(elevator::ELEVATOR, elevator::entry())?;
    r.stages.enter("Map_M008", StageEntry { create: Some(create), timer: Some(timer), ..StageEntry::default() })
}

fn create(w: &mut World) {
    let actors = &w.stage_run.as_ref().expect("a stage state").actors;
    let movers = (0..elevator::ROLE_NAMES.len())
        .map(|r| {
            actors
                .iter()
                .find_map(|a| match &a.own {
                    StageActorOwn::M008Elevator(e) if usize::try_from(e.role) == Ok(r) => Some(a.index),
                    _ => None,
                })
                .map_or(-1, |i| i32::try_from(i).expect("a placed index"))
        })
        .collect();
    script::script_create(w, movers);
}

fn timer(w: &mut World, ev: i32, owner: i32) {
    if script::is_script(w, owner) {
        if ev >= script::SCRIPT_EV {
            script::script_timer(w, owner, ev);
        }
        return;
    }
    let slot = u32::try_from(owner).ok().and_then(|o| {
        w.stage_run
            .as_ref()?
            .actors
            .iter()
            .position(|a| a.index == o && matches!(a.own, StageActorOwn::M008Elevator(_)))
    });
    if let Some(slot) = slot
        && ev == elevator::MOVE_LINK
    {
        elevator::elevator_move_done(w, u32::try_from(slot).expect("a list index"));
    }
}
