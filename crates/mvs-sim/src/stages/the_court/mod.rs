//! The Court (`stages/the-court/`, Map_M010): its level script (`script`: the ball chain `ball`, the powerup chain
//! `pickups`), the two walls (`wall`), the two hoops (`hoop`) and the shot clock's score (`score`).
//! The floor and the backboards are static solids; the clock hands, the ship, the spotlights, the balloon boat
//! sequence and the scoreboard widget are the drawing's. Nothing ticks: every Delay and timer runs in the leftover and
//! timer passes. No ring-out handler: nothing on this stage resets at a ring-out, a round or a match, and a match is a
//! new World. The Lab reset destroys the spawned pickup (TrainGameMode Destroy All Players, every
//! SpawnedActorParent_C), which starts its respawn.

pub mod ball;
pub mod hoop;
pub mod ids;
pub mod pickups;
pub mod score;
pub mod script;
pub mod wall;

pub use ball::Basketball;
pub use hoop::M010Hoop;
pub use wall::M010Wall;

use self::ids::{BALL_EV, LEVEL_OWNER, POWERUP_EV, SCORE_EV, SHOTCLOCK_OWNER, WALL_EV};
use crate::State;
use crate::actors::{ItemId, StageActorClass};
use crate::registry::entries::StageEntry;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::stage::StageOwn;

/// `CourtState`: the entry's own state (`StageRun.s`), its `StageOwn` variant. The
/// placed actors' owner numbers (StageData.placed index) by class, in level order.
#[derive(State, Debug, Default)]
pub struct CourtOwn {
    pub script: script::CourtScript,
    pub score: score::CourtScore,
    pub walls: Vec<i32>,
    pub hoops: Vec<i32>,
}

/// The stage's own state.
pub fn own(w: &World) -> &CourtOwn {
    match &w.stage_run.as_ref().expect("a stage state").s {
        StageOwn::Court(c) => c,
        _ => unreachable!("the Court's own state"),
    }
}

/// The stage's own state, mutable.
pub fn own_mut(w: &mut World) -> &mut CourtOwn {
    try_own_mut(w).expect("the Court's own state")
}

/// The stage's own state, none before its create or on another stage.
pub fn try_own_mut(w: &mut World) -> Option<&mut CourtOwn> {
    match &mut w.stage_run.as_mut()?.s {
        StageOwn::Court(c) => Some(c),
        _ => None,
    }
}

/// The slot (`StageState::actors` place) of the placed actor `index`.
pub fn slot_of(w: &World, index: i32) -> Option<u32> {
    let run = w.stage_run.as_ref()?;
    let at = run.actors.iter().position(|a| i32::try_from(a.index).is_ok_and(|i| i == index))?;
    u32::try_from(at).ok()
}

/// BeginPlay: the level script's, then the shot clock's (a VFX-level actor). Unverified: the BeginPlay order of the
/// gameplay level's script, its placed actors and the sub-levels' actors is not read.
fn create(w: &mut World) {
    let (walls, hoops) = {
        let actors = &w.stage_run.as_ref().expect("a stage state").actors;
        let by = |c: StageActorClass| {
            actors
                .iter()
                .filter(|a| a.cls == c)
                .map(|a| i32::try_from(a.index).expect("a placed index"))
                .collect::<Vec<_>>()
        };
        (by(StageActorClass::M010Wall), by(StageActorClass::M010HoopA01BP))
    };
    let own = CourtOwn { script: script::new(), score: score::CourtScore::default(), walls, hoops };
    w.stage_run.as_mut().expect("a stage state").s = StageOwn::Court(own);
    script::begin(w);
    let s = score::create(w);
    own_mut(w).score = s;
}

/// The walls' timelines, then the hoops' overlaps, in level order. Unverified: their order against each other is not
/// read.
fn tick(w: &mut World) {
    let walls = own(w).walls.clone();
    for i in walls {
        if let Some(s) = slot_of(w, i) {
            wall::tick(w, s);
        }
    }
    let hoops = own(w).hoops.clone();
    for i in hoops {
        if let Some(s) = slot_of(w, i) {
            hoop::tick(w, s);
        }
    }
    pickups::tick(w);
}

/// A free item's move: the hoops' sensors, in level order, until one destroys it (`hoop::item_moved`).
fn item_moved(w: &mut World, it: ItemId) {
    let hoops = own(w).hoops.clone();
    for i in hoops {
        if w.actors.items.get(it).is_none_or(|x| x.dead) {
            return;
        }
        if let Some(s) = slot_of(w, i) {
            hoop::item_moved(w, s, it);
        }
    }
}

fn reset(w: &mut World) {
    pickups::reset(w);
}

fn timer(w: &mut World, ev: i32, owner: i32) {
    if owner == LEVEL_OWNER {
        if ev >= POWERUP_EV {
            pickups::timer(w, ev);
        } else if ev >= BALL_EV {
            ball::timer(w, ev);
        }
    } else if owner == SHOTCLOCK_OWNER {
        if ev >= SCORE_EV {
            score::timer(w, ev);
        }
    } else if ev >= WALL_EV
        && let Some(s) = slot_of(w, owner)
        && w.stage_run.as_ref().is_some_and(|r| r.actors[s as usize].cls == StageActorClass::M010Wall)
    {
        wall::timer(w, s, ev);
    }
}

/// The level entries (`stages({ ... })`) and the classes' (`stageActors({ ... })`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.stage_actors.enter("m010_wall_C", wall::entry())?;
    r.stage_actors.enter("M010_Hoop_A_01_BP_C", hoop::entry())?;
    r.stages.enter(
        "Map_M010",
        StageEntry {
            create: Some(create),
            tickers: None,
            pre_tick: None,
            tick: Some(tick),
            item_moved: Some(item_moved),
            timer: Some(timer),
            ring_out: None,
            reset: Some(reset),
        },
    )?;
    ball::register(r)?;
    pickups::register(r)
}
