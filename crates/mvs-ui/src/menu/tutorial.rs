//! The tutorials' ties to the menus and to a running tutorial match: UI_MainMenu BPI_LaunchTutorial, the
//! TutorialData save's reads, and what the six tutorial widgets call on the match they sit over. The app's tutorial
//! match implements the session and hands it to the tutorial screen's frame (`MenuRun::tutorial`); None otherwise.

use mvs_data::TutorialActorData;
use mvs_sim::events::Fid;
use mvs_sim::modes::tutorial::TutorialScore;

use super::controller::Cx;
use super::manager::{Manager, TransitionParams};
use super::session::Session;
use crate::bundle::Rgba;

/// The beginner tutorial's actor class (UI_Homescreen_v2 LaunchTutorial @21048, WBP_MatchOptions @52851).
pub const INTRO_TUTORIAL: &str = "TutorialActor_Intro";
/// Every tutorial's map (the level's WorldSettings DefaultGameMode is TutorialGameMode_C).
pub const TUTORIAL_MAP: &str = "Map_TrophyRoom_Tutorial";
/// The level options BPI_LaunchTutorial's OpenLevel passes.
pub const TUTORIAL_LEVEL_OPTIONS: &str = "Tutorial";

/// UI_MainMenu BPI_LaunchTutorial(actor, map, Omit_Transition) (event @10265, @350-542): the game instance's
/// TutorialActorClass (BPI_SetTutorialActorClass; it stays set until the next launch), then the UIManager's
/// transition to `map`. The program opens the level at the transition's end (`MenuExit::OpenLevel` with `map`) and
/// reads the class back from `Session::tutorial_actor` (TutorialGameMode Start Tutorial, BPI_GetTutorialActorClass).
/// Home's TUTORIAL, Match Options' Beginner card and the advanced tutorials' tiles call it.
pub fn launch_tutorial(cx: &mut Cx<'_>, actor: &str, map: &str, omit_transition: bool) {
    cx.view.session.tutorial_actor = Some(actor.to_owned());
    Manager::transition(
        cx,
        TransitionParams {
            next_level: map.to_owned(),
            kind: 0,
            delay: 0.0,
            force: false,
            instant: omit_transition,
            level_options: TUTORIAL_LEVEL_OPTIONS.to_owned(),
        },
    );
}

/// BPI_LoadTutorialQuestData's TutorialsComplete holds `save_key` (WBP_TutorialType UpdateAppearance @1140: the
/// tile's badge goes).
pub fn tutorial_complete(session: &mut Session, save_key: &str) -> bool {
    session.stats.tutorial().tutorials_complete.contains_key(save_key)
}

/// The running tutorial as its widgets see it (TutorialIntroScreen, TutorialWidget, UI_TutorialEndScreen): the
/// actor's calls they make, what they read of it, and the match around them. Each page call runs on the World
/// between two steps; the `TutorialUi` events it raises reach the widgets through `TutorialScreens::apply` after
/// the call returns, in order.
pub trait TutorialSession {
    /// TutorialIntroScreen buttonStart: the actor's BPI_IntroComplete (@456).
    fn intro_complete(&mut self);
    /// TutorialWidget buttonStart: BPI_UncontrolTutorialPawn(true, true, false) (@1873).
    fn start_pressed(&mut self);
    /// TutorialWidget buttonSkip: BPI_AdvanceTutorial (@2144).
    fn advance(&mut self);
    /// UI_TutorialEndScreen NextTutorial: the game mode's BPI_StartNextTutorial (@4032).
    fn start_next(&mut self);
    /// UI_TutorialEndScreen Init: the actor's BPI_DesaturateEnvironment (@2990).
    fn desaturate_environment(&mut self);
    /// The running actor's data (its steps, tasks and texts); None before the actor exists.
    fn actor_data(&self) -> Option<&TutorialActorData>;
    /// BPI_GetTutorialMinigameScores (UI_TutorialEndScreen PopulateScoreboard): each entry's task names its
    /// ScoreDescriptor (the task's `minigame_text`).
    fn minigame_scores(&self) -> &[TutorialScore];
    /// UGameplayStatics IsGamePaused (TutorialWidget StartNewConcept's wait @15-121).
    fn is_game_paused(&self) -> bool;
    /// TutorialWidget SetPlayerFocus(b) besides the UIManager's SetHandleInput(b): false runs BPI_EnableGameInput
    /// and BPI_ResetInputs on every local PlayerController (@1314 / @1485); true takes the game input away while the
    /// widget holds the UI's. The intro screen and the end screen hold it while they are up.
    fn set_game_input(&mut self, on: bool);
    /// UI_TutorialEndScreen BackToMainMenu's transition (@2452-2674, after its Delay 0.5): the match ends with no
    /// result and the main menu opens. `adv_tut_shortcut`: View More Tutorials (@4790 BPI_SetAdvTutShortcut(true)):
    /// the program sets `Session::adv_tut_shortcut`, which home reads and clears at its construct.
    fn exit(&mut self, adv_tut_shortcut: bool);
    /// UI_Functions PopText on the match's HUD (`Hud::tutorial_pop`).
    fn pop_text(&mut self, p: TutorialPopText);
}

/// The UI_PopTextParams the tutorial's pop texts fill (no icon, no audio, no delay): the congratulation text
/// (TutorialActor @3632-4094) and the end screen's (UI_TutorialEndScreen @3117-3624) in viewport space, the
/// minigame counter's "+1" (TutorialMinigame_Parent IncrementMinigameCounter @434-973) on its pawn.
#[derive(Clone, Debug, PartialEq)]
pub struct TutorialPopText {
    pub text: String,
    pub lifetime: f64,
    pub color: Rgba,
    /// Space 2: a fraction of the viewport (x, y); space 3: a UE offset from the attach pawn's location.
    pub location: [f64; 3],
    /// Space 3's attach pawn (a sim fighter id); None: space 2.
    pub attach: Option<Fid>,
    pub scale: f64,
    /// 0 left, 1 centre, 2 right.
    pub justification: u8,
}
