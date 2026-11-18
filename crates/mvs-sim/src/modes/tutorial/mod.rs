//! The tutorial level's sim: TutorialGameMode and the TutorialActor step / task
//! machine it spawns (`run.rs`), the latent actions and timers of its objects (`latent.rs`), the demo controllers
//! (`demo.rs`, the beginner tutorial's; `advanced/*`), the minigame actors (`minigame.rs`, `advanced/*`), the fighter
//! calls they share (`pawn.rs`) and the class registry (`classes.rs`).
//!
//! The state is [`TutorialRun`] on `World::tutorial` (`World.mode`, the TutorialGameMode, and
//! `World.tutorial`, the TutorialActor, in one): set by the game mode's begin play (`run::start_tutorial`), its
//! `actor` by the game mode's Start Tutorial 0.2 s later. A tutorial is offline and never rolled back: the World's
//! clone carries it all the same.
//!
//! Closures are commands here: a latent entry, a delegate bind or a controller names its object
//! ([`TutorialObj`]) and its node or handler, and the object's class answers it (`classes.rs`). An object's own
//! state is its class's variant ([`demo::DemoState`], [`minigame::MinigameState`]), defined in the family file that
//! ports the class.

pub mod advanced;
pub mod classes;
pub mod demo;
pub mod latent;
pub mod minigame;
pub mod pawn;
pub mod run;

use crate::Real;
use crate::State;
use crate::fighter::FighterId;
use crate::fighter::match_side::TutorialEvent;

pub use run::{
    actor_data, advance, desaturate_environment, desaturation, intro_complete, is_skippable, minigame_scores, over,
    pawn_controlled_by_tutorial, skip_step, skip_to_end, start_next, start_pressed, step_index,
};

/// Whether a TutorialActor runs on the World (`World.tutorial` set: the actor's BeginPlay,
/// BPI_AddTutorialActor): the fighters' tutorial events and reads go to it from then on. The game mode alone (its
/// first 0.2 s) is no tutorial World yet.
pub fn running(w: &crate::world::World) -> bool {
    w.tutorial.as_ref().is_some_and(|t| t.actor.is_some())
}

/// A tutorial object's bind on a run-time delegate (`Multicast.add(owner, fn)`, `world::flow::Bound::Tutorial`): the
/// object and its handler, by the method's name (AddUnique compares both).
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub struct TutorialBind {
    pub owner: TutorialObj,
    pub handler: &'static str,
}

impl TutorialRun {
    /// TutorialGameMode's state at its construction.
    pub fn new() -> Self {
        TutorialRun {
            sched: latent::Latent::new(),
            actor: None,
            player_respawned: Vec::new(),
            demos: Vec::new(),
            obj_seq: 0,
        }
    }

    /// A new tutorial object id.
    pub fn next_id(&mut self) -> u32 {
        let id = self.obj_seq;
        self.obj_seq += 1;
        id
    }
}

impl Default for TutorialRun {
    fn default() -> Self {
        Self::new()
    }
}

/// A tutorial object that owns latent actions and timers, binds delegates or is bound: the game mode, the actor,
/// a demo controller or a minigame actor by its id, or a stage actor a tutorial class spawned ([`TutorialRun::obj_seq`]
/// order: the spawn order of the
/// objects).
#[derive(State, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TutorialObj {
    /// The TutorialGameMode.
    Mode,
    /// The TutorialActor (and its child class's own graph).
    Actor,
    /// A demo controller (`TutorialRun::demos`), by `Demo::id`.
    Demo(u32),
    /// A minigame actor (`TutorialActor::minigames`), by `Minigame::id`.
    Minigame(u32),
    /// A stage actor a tutorial class spawned (Tutorial_Meeseeks), by its index in
    /// the stage run's actors.
    Stage(i32),
}

/// The running tutorial (`World::tutorial`): the TutorialGameMode's state and, once its Start Tutorial ran, the
/// TutorialActor's.
#[derive(State, Debug)]
pub struct TutorialRun {
    /// TutorialGameMode `sched`: every tutorial object's Delays, MoveComponentTo and timers.
    pub sched: latent::Latent,
    /// TutorialGameMode `actor` (and `World.tutorial`): `None` until Start Tutorial.
    pub actor: Option<TutorialActor>,
    /// Parent_Game_Mode's PlayerRespawned(PlayerIndex) delegate: the handlers bound to it, in bind order.
    pub player_respawned: Vec<TutorialBind>,
    /// The demo controllers made so far, in spawn order: a controller outlives its pawn (`demo.rs`), so none leaves
    /// the list.
    pub demos: Vec<demo::Demo>,
    /// The next tutorial object id (demo controllers and minigame actors share it).
    pub obj_seq: u32,
}

/// TutorialActor and its child classes' state (`run.rs` `TutorialActor`): the fields, in their order. The
/// actor's
/// data is `World::data.tutorials[actor]`.
#[derive(State, Debug)]
pub struct TutorialActor {
    /// The actor class (the data's key: TutorialActor_Intro, TutorialActor_DodgeMeter ...).
    pub actor: String,
    pub step_index: i32,
    /// TutorialConceptIndex (CDO 1).
    pub concept_index: i32,
    pub concept_count: i32,
    /// AudioStepCounter.
    pub audio_step: i32,
    /// TutorialIsTransitioning.
    pub transitioning: bool,
    /// PawnControlledByTutorial (CDO true).
    pub pawn_controlled_by_tutorial: bool,
    pub all_tasks_complete: bool,
    /// TutorialOver.
    pub over: bool,
    /// MPC TutorialEnvironmentDesat's Desaturation.
    pub desaturation: Real,
    /// TutorialMinigameScores, in the order the SaveMinigameScore tasks added them.
    pub minigame_scores: Vec<TutorialScore>,
    /// TaskRepetitionCounter of each step's tasks.
    pub counters: Vec<Vec<i32>>,
    /// The events queued while the step transitions.
    pub queued: Vec<TutorialEvent>,
    /// TutorialManagedPawn.
    pub managed: Option<FighterId>,
    /// PunchingBag.
    pub bag: Option<FighterId>,
    /// The player pawns whose HitAnything is bound to OnHitAnything (never unbound).
    pub bound: Vec<FighterId>,
    /// TutorialActorTimeline: its position and direction, `None` when stopped.
    pub timeline: Option<Timeline>,
    /// TaskMinigameCounter of each step's tasks.
    pub minigame_counters: Vec<Vec<i32>>,
    /// The handle loop's task index (BPI_AdvanceTutorial's score save reads it).
    pub loop_task: i32,
    /// TutorialStepComplete, TutorialStepCleanup, TutorialDemoComplete: the handlers bound to each, in bind order
    /// (the actor's own handlers run first, outside these lists).
    pub step_complete: Vec<TutorialBind>,
    pub step_cleanup: Vec<TutorialBind>,
    pub demo_complete: Vec<TutorialBind>,
    /// The live minigame actors, in spawn order.
    pub minigames: Vec<minigame::Minigame>,
}

/// TutorialActorTimeline's play state: its position (seconds, 0 .. 0.25) and direction (1 PlayFromStart, -1
/// ReverseFromEnd).
#[derive(State, Copy, PartialEq, Debug)]
pub struct Timeline {
    pub t: Real,
    pub dir: i32,
}

/// One TutorialMinigameScores entry: the SaveMinigameScore task it came from (its MinigameCounterText is the
/// ScoreDescriptor UI_MinigameScore_Row shows) and the Score.
#[derive(State, Copy, PartialEq, Debug)]
pub struct TutorialScore {
    pub step: i32,
    pub task: i32,
    pub score: i32,
}
