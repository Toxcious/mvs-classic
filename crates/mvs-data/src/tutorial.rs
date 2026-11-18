//! The tutorials' data: every TutorialActor class's class default object (its TutorialSteps and the actor's own
//! fields), the store's `/GameData/Tutorials` entry Classes are the build's class names without the `_C`
//! (Pawn_Shaggy, TutorialController_Evade, Idle_AIController); `None` where the asset has none.
//!
//! Texts are no data of the port's: each is a [`TutorialText`], the locres key the player's `Game.locres` gives the
//! words for at run time, with the asset's source string (TutorialActor_Intro's Check For Attack Decay compares a
//! header's source string, so the sim reads it).

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// An FText of the asset: its locres namespace and key, and its source string (the English the asset was written
/// with). A text read from somewhere with no key (a trace header's words) has empty `namespace` and `key`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TutorialText {
    pub namespace: String,
    pub key: String,
    pub source: String,
}

/// One TaskControllerInputs entry. `widget`: TutorialInputWidgetTypes (0 InputButtonWidget, 1 UI_InputLabel).
/// InputButtonWidget: `texture`, the brush's texture name (Comma-outline), `label` its UIInputLabelParams' text if
/// any. UI_InputLabel: `action` and `modifier`, UIInputLabelParams' Action and Modifier as the build names them
/// (GameJump, Hold), empty for none.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TutorialInput {
    pub widget: u8,
    pub texture: String,
    pub action: String,
    pub modifier: String,
    pub label: TutorialText,
}

/// One TutorialTask. `text`: TaskText (the counter's Format is the widget's). `event`: TaskCompletionEvent's byte
/// (TutorialEventTypes, the sim's `TutorialEvent`). `req`: TaskRepetitionReq. `counter`: HasTaskCounter.
/// `custom_actors`: CustomTutorialActors' class names (the advanced tutorials' minigame actors).
/// `minigame_counter` / `save_score`: HasMinigameCounter / SaveMinigameScore; `minigame_text`: the counter's text
/// (empty when none).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TutorialTask {
    pub text: TutorialText,
    pub event: u8,
    pub req: i32,
    pub counter: bool,
    pub custom_actors: Vec<String>,
    pub minigame_counter: bool,
    pub save_score: bool,
    pub minigame_text: TutorialText,
    pub inputs: Vec<TutorialInput>,
}

/// One TutorialStep. `header` / `text`: ConceptHeaderText / ConceptText (`header.source` is the
/// `headerEnglish`). `player` / `dummy`: PlayerClass / DummyClass; `character` / `dummy_character`: their data keys
/// (CharacterDataByFighter as the port names them), `None` when the class is no fighter the port has.
/// `start_button_disabled_time` and `bag_percent` are the asset's float32 values.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TutorialStep {
    pub new_concept: bool,
    pub header: TutorialText,
    pub text: TutorialText,
    pub start_button_disabled_time: f32,
    pub player: String,
    pub character: Option<String>,
    pub player_tag: String,
    pub dummy: Option<String>,
    pub dummy_character: Option<String>,
    pub spawn_target_dummy: bool,
    pub dummy_controller: Option<String>,
    pub bag_percent: f32,
    pub bag_tag: String,
    pub disable_cooldowns: bool,
    /// DemoAIController's class, `None` for none.
    pub demo: Option<String>,
    /// The step's VO sound alias, `None` for none.
    pub vo: Option<String>,
    pub tasks: Vec<TutorialTask>,
}

/// One TutorialActor class's defaults. `name` / `intro_text`: TutorialName / IntroText; `intro_vo` the sound alias;
/// `next` NextTutorial's class name; `complete_key` TutorialActor_Advanced's tutorialCompleteKey (the
/// TutorialsComplete key FinishTutorial adds; `None` for the Intro); `congratulations`: RandomCongratulationText.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TutorialActorData {
    pub actor: String,
    pub name: TutorialText,
    pub intro_text: TutorialText,
    pub intro_vo: Option<String>,
    pub next: Option<String>,
    pub complete_key: Option<String>,
    pub congratulations: Vec<TutorialText>,
    pub steps: Vec<TutorialStep>,
}

/// Every TutorialActor class the import reads, by class name (TutorialActor_Intro, the advanced ones and
/// TutorialActor_Batman_Fundementals), in the import's order.
pub type Tutorials = IndexMap<String, TutorialActorData>;
