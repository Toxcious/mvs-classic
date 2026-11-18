//! TutorialMinigame_Parent: the advanced tutorials' minigame actors (a task's
//! CustomTutorialActors). UncontrolTutorialPawn spawns each task's classes deferred at the origin: their BeginPlay,
//! then BPI_SetupTutorialMinigameActor(self, TutorialStepIndex, the task's index). They run on their Delays, timers
//! and the delegates they bind; K2_DestroyActor drops their latent entries and binds and takes them off the list.
//!
//! Each is a [`Minigame`] in `TutorialActor::minigames`; its own state is its class's [`MinigameState`] variant,
//! defined in the family file that ports it (`advanced/*`).

use crate::State;
use crate::fighter::match_side::TutorialEvent;
use crate::world::World;

use super::classes::{self, Call, RunList};
use super::{TutorialObj, latent};

use super::advanced::{batman, elements, fighters, geometry, meeseeks};

/// A minigame actor: the base class's fields and its class's state.
#[derive(State, Debug)]
pub struct Minigame {
    /// `TutorialObj::Minigame` id.
    pub id: u32,
    /// The class (TutorialMinigame_DodgeBasics ...).
    pub cls: &'static str,
    pub destroyed: bool,
    /// TutorialStepIndex / TutorialTaskIndex (the setup's; -1 before it).
    pub step_index: i32,
    pub task_index: i32,
    pub state: MinigameState,
}

/// A minigame class's own state, a variant per class (`advanced/*/minigames.rs`, `advanced/elements/*.rs`).
#[derive(State, Debug)]
pub enum MinigameState {
    // advanced/fighters/minigames.rs
    DodgeBasics(fighters::DodgeBasicsMinigame),
    DodgeRefill(fighters::DodgeRefill),
    ArmorBreak(fighters::ArmorBreakMinigame),
    // advanced/geometry/minigames.rs
    SpawnWall(geometry::SpawnWall),
    SpawnPlatform(geometry::SpawnPlatform),
    KbiSide(geometry::KbiSideMinigame),
    KbiUp(geometry::KbiUpMinigame),
    // advanced/meeseeks/minigames.rs
    AerialCombat(meeseeks::AerialCombat),
    PositioningTools(meeseeks::PositioningTools),
    // advanced/elements/{downspike,fire}.rs
    DownSpike(elements::DownSpike),
    ExtinguishFire(elements::ExtinguishFire),
    Ignite(elements::Ignite),
    // advanced/batman.rs (no class of the family)
    BatmanBatarang(batman::BatarangMinigame),
    BatmanRetrieve(batman::RetrieveMinigame),
    BatmanDestroy(batman::DestroyMinigame),
}

// ---- the base class's graph (TutorialMinigame_Parent)

/// The base's ReceiveBeginPlay: the binds of the step delegates a class has handlers for, TutorialStepComplete then
/// TutorialStepCleanup (each class's own BeginPlay binds the ones it has, ahead of its own work). A class's BeginPlay
/// (`Call::BeginPlay`) calls this first; its handlers are `"onStepComplete"` and `"onStepCleanup"`.
pub fn bind_steps(w: &mut World, id: u32, complete: bool, cleanup: bool) {
    let me = TutorialObj::Minigame(id);
    if complete {
        classes::bind_run(w, RunList::StepComplete, me, RunList::StepComplete.base_handler());
    }
    if cleanup {
        classes::bind_run(w, RunList::StepCleanup, me, RunList::StepCleanup.base_handler());
    }
}

/// BPI_SetupTutorialMinigameActor(TutorialActor, TutorialStepIndex, TutorialTaskIndex) (@10-56).
pub fn setup(w: &mut World, id: u32, step: i32, task: i32) {
    if let Some(m) = classes::minigame_mut(w, id) {
        m.step_index = step;
        m.task_index = task;
    }
}

/// IncrementMinigameCounter(ShowRewardPopText) (@0-1039) on the setup's step and task.
pub fn increment_counter(w: &mut World, id: u32, pop: bool) {
    let Some(m) = classes::minigame_of(w, id) else { return };
    let (step, task) = (m.step_index, m.task_index);
    super::run::increment_minigame_counter(w, step, task, pop);
}

/// GameState.BPI_ProcessTutorialEvent(byte): forwarded to every TutorialActor (the game state's list: the one).
pub fn process_event(w: &mut World, ev: TutorialEvent) {
    crate::world::flow::tutorial_event(w, ev);
}

/// TutorialActor.BPI_HandleTutorialEvent(byte) called directly (TutorialMinigame_DodgeBasics @90).
pub fn handle_event(w: &mut World, ev: TutorialEvent) {
    super::run::bus_event(w, ev);
}

/// K2_DestroyActor(self): its latent actions and timers go, its bound handlers are stale from here on (never called
/// again), its class's own ReceiveEndPlay (`Call::Destroyed`), then it leaves the actor's list. The rest of a
/// function that destroyed its own actor still runs (a pending-kill self: ProcessEvent returns at entry only for a
/// new call), as in the build.
/// Not ported: OnDestroyed's broadcast (a minigame is no `ActorRef`; nothing binds a minigame's OnDestroyed).
pub fn destroy(w: &mut World, id: u32) {
    let Some(m) = classes::minigame_mut(w, id) else { return };
    if m.destroyed {
        return;
    }
    m.destroyed = true;
    latent::drop_owner(w, TutorialObj::Minigame(id));
    classes::call(w, TutorialObj::Minigame(id), Call::Destroyed);
    if let Some(a) = w.tutorial.as_mut().and_then(|t| t.actor.as_mut())
        && let Some(i) = a.minigames.iter().position(|m| m.id == id)
    {
        a.minigames.remove(i);
    }
}
