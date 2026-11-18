//! The advanced tutorials' Meeseeks family (AerialCombat, DashAttack): the Tutorial_Meeseeks
//! targets (`actor.rs`, a stage actor: `actors/classes.rs` TutorialMeeseeks), the minigames
//! TutorialMinigame_AerialCombat and TutorialMinigame_PositioningTools (`minigames.rs`) and the demo controllers
//! TutorialController_AerialCombat_Basics, TutorialController_AerialCombat_HighUp and
//! TutorialController_EvadeToAttack (`demos.rs`). Their own state is the payload of `DemoState` / `MinigameState` /
//! `StageActorOwn`.

mod actor;
mod demos;
mod minigames;

use crate::registry::{Registry, RegistryError};
use crate::world::World;

use super::super::classes::Call;
use super::super::demo::DemoState;
use super::super::minigame::MinigameState;

pub use actor::{MeeseeksEnd, MeeseeksOwn};
pub use demos::{AerialCombatBasics, AerialCombatHighUp, EvadeToAttack};
pub use minigames::{AerialCombat, PositioningTools};

/// The family's stage actor class: Tutorial_Meeseeks (`stageActors`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.stage_actors.enter(actor::CLS, actor::entry())
}

// ---- the family's classes (`classes.rs` asks every family; a class entered in two is an error)

/// The demo controller of a DemoAIController class of this family (its `DemoState`), `None` for a class it lacks.
pub fn make_demo(cls: &str) -> Option<(&'static str, DemoState)> {
    use DemoState as D;
    Some(match cls {
        "TutorialController_AerialCombat_Basics" => {
            ("TutorialController_AerialCombat_Basics", D::AerialCombatBasics(AerialCombatBasics::default()))
        }
        "TutorialController_AerialCombat_HighUp" => {
            ("TutorialController_AerialCombat_HighUp", D::AerialCombatHighUp(AerialCombatHighUp::default()))
        }
        "TutorialController_EvadeToAttack" => {
            ("TutorialController_EvadeToAttack", D::EvadeToAttack(EvadeToAttack::default()))
        }
        _ => return None,
    })
}

/// A demo controller's construction (the class's constructor): EvadeToAttack binds TutorialDemoComplete.
pub fn demo_constructed(w: &mut World, id: u32) {
    demos::constructed(w, id);
}

/// The minigame actor of a CustomTutorialActors class of this family, `None` for a class it lacks.
pub fn make_minigame(cls: &str) -> Option<(&'static str, MinigameState)> {
    use MinigameState as M;
    Some(match cls {
        "TutorialMinigame_AerialCombat" => ("TutorialMinigame_AerialCombat", M::AerialCombat(AerialCombat::default())),
        "TutorialMinigame_PositioningTools" => {
            ("TutorialMinigame_PositioningTools", M::PositioningTools(PositioningTools::default()))
        }
        _ => return None,
    })
}

/// A TutorialActor child class's own ReceiveBeginPlay, after the parent's: nothing for a class this family lacks.
pub fn actor_begin_play(w: &mut World, cls: &str) {
    let _ = (w, cls);
}

/// The characters a class of this family is drawn with (`TutorialClasses.drawnWith`): Tutorial_Meeseeks' Meeseeks_SkelMesh
/// and its sequences are loaded with Rick.
pub fn drawn_with(cls: &str) -> &'static [&'static str] {
    match cls {
        "TutorialController_AerialCombat_Basics"
        | "TutorialController_AerialCombat_HighUp"
        | "TutorialController_EvadeToAttack"
        | "TutorialMinigame_AerialCombat"
        | "TutorialMinigame_PositioningTools" => &["rick"],
        _ => &[],
    }
}

/// A call on one of this family's demo controllers (`TutorialRun::demos`, by id).
pub fn demo(w: &mut World, id: u32, c: Call<'_>) {
    demos::call(w, id, c);
}

/// A call on one of this family's minigame actors (`TutorialActor::minigames`, by id).
pub fn minigame(w: &mut World, id: u32, c: Call<'_>) {
    minigames::call(w, id, c);
}

/// A call on a Tutorial_Meeseeks stage actor (`TutorialObj::Stage`).
pub fn stage(w: &mut World, i: i32, c: Call<'_>) {
    actor::call(w, i, c);
}

/// Whether the Tutorial_Meeseeks stage actor `i` is destroyed (its binds are stale).
pub fn stage_destroyed(w: &World, i: i32) -> bool {
    actor::destroyed(w, i)
}
