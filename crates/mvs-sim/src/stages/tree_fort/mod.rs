//! Tree Fort (Map_TreeHouse, Map_TreeHouseSmall, Map_TreeHouse_1Plat_Large): the entry, its
//! own state and its placed actors' classes. Their placed actors: the two destructible bounce trees
//! (TreehouseSidePillar_C and its child: [`pillar`]) tick and take hits; the platforms are BasePlatformBP's own and
//! their wiggle is presentation.

pub mod pillar;

pub use pillar::Pillar;

use crate::State;
use crate::actors::StageActorClass;
use crate::registry::entries::StageEntry;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::stage::StageOwn;
use crate::world::stage_actors::StageActorOwn;

/// `TreeFortState`: the entry's own state (`StageRun.s`), its `StageOwn` variant: the
/// trees that tick (ReceiveTick; PrimaryActorTick bCanEverTick in the class), by owner number (their StageData.placed
/// index), in level order: one destroyed at BeginPlay is left out; one destroyed later stays, dead (`pre_tick` passes
/// over it).
#[derive(State, Debug, Default)]
pub struct TreeFortOwn {
    pub tick: Vec<i32>,
}

/// The slot (in `StageState::actors`) of the tree whose owner number is `owner`.
fn tree_slot(w: &World, owner: i32) -> Option<u32> {
    let r = w.stage_run.as_ref()?;
    let i = r.actors.iter().position(|a| i32::try_from(a.index) == Ok(owner))?;
    matches!(r.actors[i].own, StageActorOwn::Pillar(_)).then(|| u32::try_from(i).expect("a list index"))
}

/// BeginPlay: AllowHazards off destroys each tree at once (@5772-5888: offline, not a dedicated server).
// Deviation: AllHumansConnected (@7609: AllowHazards off -> DeathVisuals(false)) is bound on a dedicated server alone
// (@4936-5069): not ported.
fn create(w: &mut World) {
    let hazards = w.setup.allow_hazards;
    let slots: Vec<u32> = {
        let r = w.stage_run.as_ref().expect("a stage run");
        (0..r.actors.len())
            .filter(|&i| matches!(r.actors[i].own, StageActorOwn::Pillar(_)))
            .map(|i| u32::try_from(i).expect("a list index"))
            .collect()
    };
    let mut s = TreeFortOwn::default();
    for slot in slots {
        if hazards {
            let idx = i32::try_from(w.stage_run.as_ref().expect("a stage run").actors[slot as usize].index)
                .expect("a placed index");
            s.tick.push(idx);
        } else {
            pillar::destroy(w, slot);
        }
    }
    w.stage_run.as_mut().expect("a stage run").s = StageOwn::TreeFort(s);
}

fn tickers(w: &World) -> &[i32] {
    match w.stage_run.as_ref().map(|r| &r.s) {
        Some(StageOwn::TreeFort(s)) => &s.tick,
        _ => &[],
    }
}

fn pre_tick(w: &mut World, owner: i32) {
    if let Some(slot) = tree_slot(w, owner)
        && !w.stage_run.as_ref().expect("a stage run").actors[slot as usize].dead
    {
        pillar::tick(w, slot);
    }
}

/// The death Delay's K2_DestroyActor. The tree stays in `tick` (`stage_pre_tick` walks that list while this runs in a
/// tree's own latent pass); `pre_tick` passes over a dead one.
fn timer(w: &mut World, ev: i32, owner: i32) {
    if ev != pillar::DEATH_DELAY {
        return;
    }
    if let Some(slot) = tree_slot(w, owner) {
        pillar::destroy(w, slot);
    }
}

/// The level entries (`stages({ ... })`) and the classes' (`stageActors({ ... })`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.stage_actors.enter("TreehouseSidePillar_C", pillar::entry(StageActorClass::TreehouseSidePillar))?;
    r.stage_actors.enter("TreehouseSidePillar_Child_C", pillar::entry(StageActorClass::TreehouseSidePillarChild))?;
    let tree_fort = StageEntry {
        create: Some(create),
        tickers: Some(tickers),
        pre_tick: Some(pre_tick),
        timer: Some(timer),
        ..StageEntry::default()
    };
    for map in ["Map_TreeHouse", "Map_TreeHouseSmall", "Map_TreeHouse_1Plat_Large"] {
        r.stages.enter(map, tree_fort)?;
    }
    Ok(())
}
