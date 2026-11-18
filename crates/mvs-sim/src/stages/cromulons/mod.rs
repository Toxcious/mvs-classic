//! Cromulons (Map_M009, Map_M009_small): the entry, its own state and its placed actors'
//! classes. The two speakers ([`speaker`]) take hits and break; the floor and the floating platform are static; the
//! level script's BeginPlay poll, tick and music are the drawing's and the audio's; ResetLevelObjects has no caller: a
//! broken speaker stays broken.

pub mod speaker;

pub use speaker::Speaker;

use crate::State;
use crate::actors::StageActorClass;
use crate::registry::entries::StageEntry;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::stage::StageOwn;
use crate::world::stage_actors::StageActorOwn;
use crate::world::wall_bp::{WALL_DELAY, wall_delayed, wall_of_owner};

/// `CromulonsState`: the entry's own state (`StageRun.s`), its `StageOwn` variant: the
/// speakers that tick (ReceiveTick, PrimaryActorTick bCanEverTick), by owner number (their StageData.placed index), in
/// level order. A broken speaker stays alive and ticking.
// Unverified: Map_M009_C ticks too (its order among the level's actors not read); its tick has no sim.
#[derive(State, Debug, Default)]
pub struct CromulonsOwn {
    pub tick: Vec<i32>,
}

fn create(w: &mut World) {
    let r = w.stage_run.as_mut().expect("a stage run");
    let tick = r
        .actors
        .iter()
        .filter(|a| matches!(a.own, StageActorOwn::Speaker(_)))
        .map(|a| i32::try_from(a.index).expect("a placed index"))
        .collect();
    r.s = StageOwn::Cromulons(CromulonsOwn { tick });
}

fn tickers(w: &World) -> &[i32] {
    match w.stage_run.as_ref().map(|r| &r.s) {
        Some(StageOwn::Cromulons(s)) => &s.tick,
        _ => &[],
    }
}

/// WallBP's Delay(0.1) link (@34 -> @89): the components off.
fn timer(w: &mut World, ev: i32, owner: i32) {
    if ev == WALL_DELAY
        && let Some(a) = wall_of_owner(w, owner)
    {
        wall_delayed(w, a);
    }
}

/// The level entries (`stages({ ... })`) and the classes' (`stageActors({ ... })`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.stage_actors
        .enter("M009_DestructiblePlatformBP_C", speaker::entry(StageActorClass::M009DestructiblePlatformBP, false))?;
    r.stage_actors.enter(
        "M009_DestructiblePlatformBP_Child_C",
        speaker::entry(StageActorClass::M009DestructiblePlatformBPChild, true),
    )?;
    let cromulons =
        StageEntry { create: Some(create), tickers: Some(tickers), timer: Some(timer), ..StageEntry::default() };
    for map in ["Map_M009", "Map_M009_small"] {
        r.stages.enter(map, cromulons)?;
    }
    Ok(())
}
