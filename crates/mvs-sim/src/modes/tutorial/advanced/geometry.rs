//! The advanced tutorials' geometry classes (`geometry.rs`, `geometry/{demos,minigames,solids,util}.rs`):
//! the solids TutorialWall, TutorialWallShort and TutorialPlatform on the run-time solids (`solids.rs`), the minigames
//! TutorialMinigame_SpawnWall, _SpawnPlatform, _KBISide and _KBIUp (`minigames.rs`) and the demo controllers
//! TutorialController_WallSlide, _FastFall_Intro / _Jump / _Platform, _KBISide and _KBIUp (`demos.rs`). Their own
//! state is the payload of `DemoState` / `MinigameState`.

mod demos;
mod minigames;
pub mod solids;

use crate::Real;
use crate::events::Event;
use crate::name;
use crate::names::Name;
use crate::world::World;

use super::super::classes::Call;
use super::super::demo::DemoState;
use super::super::minigame::MinigameState;

pub use demos::{FastFall, FastFallPlatform, KbiDemo, KbiSideDemo, KbiUpDemo, WallSlide};
pub use minigames::{KbiMinigame, KbiSideMinigame, KbiUpMinigame, SpawnPlatform, SpawnWall};

/// `CAST_SOUND`.
const CAST_SOUND: Name = name!("sfx_cha_tutorial_meeseeks_cast");

/// `emitterAt`: SpawnEmitterAtLocation(PS_MeeseeksSpawn, location) of the class's own graph (the drawing side draws
/// the KBI ones with their graph's Team 1.0).
fn emitter_at(w: &mut World, cls: Name, x: Real, y: Real) {
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter: -1,
        vfx: Some(name!("PS_MeeseeksSpawn")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
}

// ---- the family's classes (`classes.rs` asks every family; a class entered in two is an error)

/// The demo controller of a DemoAIController class of this family (its `DemoState`), `None` for a class it lacks.
pub fn make_demo(cls: &str) -> Option<(&'static str, DemoState)> {
    use DemoState as D;
    Some(match cls {
        "TutorialController_WallSlide" => ("TutorialController_WallSlide", D::WallSlide(WallSlide::default())),
        "TutorialController_FastFall_Intro" => {
            ("TutorialController_FastFall_Intro", D::FastFall(FastFall { fighter: None, jump_again: false }))
        }
        "TutorialController_FastFall_Jump" => {
            ("TutorialController_FastFall_Jump", D::FastFall(FastFall { fighter: None, jump_again: true }))
        }
        "TutorialController_FastFall_Platform" => {
            ("TutorialController_FastFall_Platform", D::FastFallPlatform(FastFallPlatform::default()))
        }
        "TutorialController_KBISide" => ("TutorialController_KBISide", D::KbiSide(KbiSideDemo::default())),
        "TutorialController_KBIUp" => ("TutorialController_KBIUp", D::KbiUp(KbiUpDemo::default())),
        _ => return None,
    })
}

/// The minigame actor of a CustomTutorialActors class of this family, `None` for a class it lacks.
pub fn make_minigame(cls: &str) -> Option<(&'static str, MinigameState)> {
    use MinigameState as M;
    Some(match cls {
        "TutorialMinigame_SpawnWall" => ("TutorialMinigame_SpawnWall_C", M::SpawnWall(SpawnWall::default())),
        "TutorialMinigame_SpawnPlatform" => {
            ("TutorialMinigame_SpawnPlatform_C", M::SpawnPlatform(SpawnPlatform::default()))
        }
        "TutorialMinigame_KBISide" => ("TutorialMinigame_KBISide_C", M::KbiSide(KbiSideMinigame::default())),
        "TutorialMinigame_KBIUp" => ("TutorialMinigame_KBIUp_C", M::KbiUp(KbiUpMinigame::default())),
        _ => return None,
    })
}

/// A TutorialActor child class's own ReceiveBeginPlay, after the parent's: nothing for a class this family lacks.
pub fn actor_begin_play(w: &mut World, cls: &str) {
    let _ = (w, cls);
}

/// The characters a class of this family is drawn with (`TutorialClasses.drawnWith`): PS_MeeseeksSpawn, the teleport
/// puff the KBI minigames and the FastFall_Platform demo spawn, is in Rick's data.
pub fn drawn_with(cls: &str) -> &'static [&'static str] {
    match cls {
        "TutorialController_FastFall_Platform" | "TutorialMinigame_KBISide" | "TutorialMinigame_KBIUp" => &["rick"],
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
