//! The tutorial's widgets on a menu screen over the match: the
//! TutorialActor's calls into TutorialIntroScreen, TutorialWidget, TutorialTaskWidget and UI_TutorialEndScreen,
//! carried out against the running actor's data, and UI_Game's Can Pause tests of them (UI_Game Can Pause
//! @599-1210).

pub mod widgets;

use mvs_data::TutorialActorData;
use mvs_data::tutorial::{TutorialInput, TutorialStep, TutorialTask};
use mvs_sim::events::TutorialUiCall;

use self::widgets::{TutorialEndScreen, TutorialIntroScreen, TutorialTaskWidget, TutorialWidget, tutorial_text};
use crate::bundle::Rgba;
use crate::menu::controller::Cx;
use crate::menu::tutorial::TutorialPopText;
use crate::menu::view::MenuView;
use crate::slate::{Vis, WidgetId};

/// SetupTutorialWidget's task text with a counter (@1573): Format of key 232EB89140CC5A45D114598D7C671A8B with
/// {task}, {count} (TaskRepetitionCounter) and {total} (TaskRepetitionReq).
const TASK_COUNTER_TEXT: &str = "232EB89140CC5A45D114598D7C671A8B";
/// The congratulation PopText (TutorialActor @3632-4094).
const CONGRATS_COLOR: Rgba = [0.008023, 0.693872, 0.258183, 1.0];
/// TutorialMinigame_Parent IncrementMinigameCounter's "+1" PopText (@448-973): its text's key, colour, offset from
/// the attach actor (space 3), lifetime 1, scale 3, centred.
const REWARD_POP_TEXT: &str = "F8DFE9064A0D7A07670CB1AB5262D4BB";
const REWARD_POP_COLOR: Rgba = [0.038204, 0.83077, 0.309469, 1.0];

/// FText Format of the named arguments the call passes ({name} replaced; an unknown one left as it is).
fn format(pattern: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close =
            after.find('}').filter(|&c| c > 0 && after[..c].chars().all(|ch| ch.is_alphanumeric() || ch == '_'));
        match close.and_then(|c| args.iter().find(|(k, _)| *k == &after[..c]).map(|(_, v)| (c, v))) {
            Some((c, v)) => {
                out.push_str(v);
                rest = &after[c + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The widgets the step machine made and the current step's task rows.
#[derive(Debug)]
pub struct TutorialScreens {
    intro: Option<WidgetId>,
    concept: Option<WidgetId>,
    end: Option<WidgetId>,
    /// The current step's task widgets (TutorialSteps[step].TutorialTasks[i].TaskWidget) and their repetition counts.
    tasks: Vec<WidgetId>,
    counts: Vec<i32>,
    step: i32,
}

impl Default for TutorialScreens {
    fn default() -> TutorialScreens {
        TutorialScreens { intro: None, concept: None, end: None, tasks: Vec::new(), counts: Vec::new(), step: -1 }
    }
}

/// A tutorial widget made by the screens is on the view's viewport and not destructed.
fn alive(view: &MenuView, w: Option<WidgetId>) -> bool {
    w.is_some_and(|w| !view.is_destroyed(w) && view.in_viewport(w))
}

impl TutorialScreens {
    pub fn new() -> TutorialScreens {
        TutorialScreens::default()
    }

    /// The widgets that left the viewport forgotten.
    fn changed(&mut self, view: &MenuView) {
        for w in [&mut self.intro, &mut self.concept, &mut self.end] {
            if !alive(view, *w) {
                *w = None;
            }
        }
    }

    /// CreateWidget(cls) and AddToViewport(0).
    fn add(cx: &mut Cx<'_>, cls: &str) -> WidgetId {
        let w = MenuView::create(cx, cls);
        MenuView::add_to_viewport(cx, w, 0);
        w
    }

    /// The running actor's data (`cx.tutorial`).
    fn data<'b>(cx: &'b Cx<'_>) -> Option<&'b TutorialActorData> {
        cx.tutorial.as_deref().and_then(|t| t.actor_data())
    }

    /// TutorialSteps[step] of the running actor.
    fn step_of<'b>(cx: &'b Cx<'_>, step: i32) -> Option<&'b TutorialStep> {
        Self::data(cx).and_then(|d| usize::try_from(step).ok().and_then(|s| d.steps.get(s)))
    }

    /// The task's text at set-up: with a counter, the Format of the task's text, `count` and its TaskRepetitionReq.
    fn task_text(view: &MenuView, t: &TutorialTask, count: i32) -> String {
        let text = tutorial_text(view, &t.text);
        if !t.counter {
            return text;
        }
        let (count, total) = (count.to_string(), t.req.to_string());
        format(view.game_text(TASK_COUNTER_TEXT), &[("task", &text), ("count", &count), ("total", &total)])
    }

    /// One step machine call (the World's `Event::TutorialUi`, in the order the World made them), against the
    /// running actor's data (`cx.tutorial`). `Finished` (the save) and `Scoreboard` (UI_Game's
    /// UI_HUD_Scoreboard_Container) are the program's and do nothing here.
    pub fn apply(&mut self, cx: &mut Cx<'_>, call: &TutorialUiCall) {
        self.changed(cx.view);
        // Each arm reads only the words it sets out of the actor's data, before it changes the widgets.
        if Self::data(cx).is_none() {
            return;
        }
        let task = |tasks: &[WidgetId], i: i32| usize::try_from(i).ok().and_then(|i| tasks.get(i)).copied();
        match call {
            TutorialUiCall::Intro => {
                // @9480-9643: Create TutorialIntroScreen, SetDetails(TutorialName, IntroText), AddToViewport(0)
                let Some(d) = Self::data(cx) else { return };
                let (name, intro) = (tutorial_text(cx.view, &d.name), tutorial_text(cx.view, &d.intro_text));
                let w = Self::add(cx, "TutorialIntroScreen");
                cx.visit::<TutorialIntroScreen, _>(w, |s, cx| s.set_details(cx, &name, &intro));
                self.intro = Some(w);
            }
            TutorialUiCall::Widget => {
                // @10082-10191: Create TutorialWidget, AddToViewport(0)
                self.concept = Some(Self::add(cx, "TutorialWidget"));
            }
            TutorialUiCall::Setup { step, .. } => {
                // SetupTutorialWidget @715: past the last step the widget goes (@1774)
                let Some(concept) = self.concept else { return self.changed(cx.view) };
                let Some(s) = Self::step_of(cx, *step) else {
                    MenuView::discard(cx, concept);
                    self.concept = None;
                    return self.changed(cx.view);
                };
                // per task: Create TutorialTaskWidget, into TutorialTaskList with more than one task, else
                // TutorialSingleTaskList (@609 / @2006); its text (@1674), then SetDetails (@1811) and
                // SetupInputButtons (@1903).
                // Unverified: the counter's value at set-up is taken as 0 (the step's tasks start uncounted; the call
                // carries no count).
                let list_name = if s.tasks.len() > 1 { "TutorialTaskList" } else { "TutorialSingleTaskList" };
                let texts: Vec<String> = s.tasks.iter().map(|t| Self::task_text(cx.view, t, 0)).collect();
                let inputs: Vec<Vec<TutorialInput>> = s.tasks.iter().map(|t| t.inputs.clone()).collect();
                let (header, text) = (tutorial_text(cx.view, &s.header), tutorial_text(cx.view, &s.text));
                self.step = *step;
                let list = cx.view.tree().find_in(concept, list_name);
                self.tasks.clear();
                self.counts = vec![0; texts.len()];
                for text in &texts {
                    let w = MenuView::create(cx, "TutorialTaskWidget");
                    MenuView::add_child(cx, list, w, None);
                    cx.visit::<TutorialTaskWidget, _>(w, |t, cx| t.set_text(cx, text));
                    self.tasks.push(w);
                }
                cx.visit::<TutorialWidget, _>(concept, |c, cx| c.set_details(cx, &header, &text));
                for (inputs, w) in inputs.iter().zip(self.tasks.clone()) {
                    cx.visit::<TutorialTaskWidget, _>(w, |tw, cx| tw.setup_inputs(cx, inputs));
                }
                // UpdateProgressText (@1917): SetProgress, whose text never shows
            }
            TutorialUiCall::TaskCount { task: i, count, .. } => {
                // UpdateTutorialText: every task with a counter gets "<text> (<count>/<req>)", a string
                // concatenation (not the Format of the set-up), then SetProgress (invisible)
                if let Some(c) = usize::try_from(*i).ok().and_then(|i| self.counts.get_mut(i)) {
                    *c = *count;
                }
                let Some(s) = Self::step_of(cx, self.step) else { return self.changed(cx.view) };
                let texts: Vec<(WidgetId, String)> = s
                    .tasks
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.counter)
                    .filter_map(|(k, t)| {
                        let w = *self.tasks.get(k)?;
                        let n = self.counts.get(k).copied().unwrap_or(0);
                        Some((w, format!("{} ({n}/{})", tutorial_text(cx.view, &t.text), t.req)))
                    })
                    .collect();
                for (w, text) in texts {
                    cx.visit::<TutorialTaskWidget, _>(w, |tw, cx| tw.set_text(cx, &text));
                }
            }
            TutorialUiCall::Jiggle { task: i } => {
                if let Some(w) = task(&self.tasks, *i) {
                    cx.visit::<TutorialTaskWidget, _>(w, |t, cx| t.jiggle(cx));
                }
            }
            TutorialUiCall::FadeOut { task: i } => {
                if let Some(w) = task(&self.tasks, *i) {
                    cx.visit::<TutorialTaskWidget, _>(w, |t, cx| t.fade_out(cx));
                }
            }
            TutorialUiCall::FadeIn => {
                // @1228: BPI_FadeInTutorialTask on every task widget
                for w in self.tasks.clone() {
                    cx.visit::<TutorialTaskWidget, _>(w, |t, cx| t.fade_in(cx));
                }
            }
            TutorialUiCall::Concept { step, start_disabled } => {
                // @541 StartNewConcept(StartButtonDisabledTime), @605 UpdateConceptSprite
                let (Some(concept), Some(s)) = (self.concept, Self::step_of(cx, *step)) else {
                    return self.changed(cx.view);
                };
                let (header, text) = (tutorial_text(cx.view, &s.header), tutorial_text(cx.view, &s.text));
                cx.visit::<TutorialWidget, _>(concept, |c, cx| {
                    c.start_new_concept(cx, f64::from(*start_disabled));
                    c.show_concept(cx, &header, &text);
                });
            }
            TutorialUiCall::Focus { on } => {
                if let Some(concept) = self.concept {
                    cx.visit::<TutorialWidget, _>(concept, |c, cx| c.set_player_focus(cx, *on));
                }
            }
            TutorialUiCall::Cleanup => {
                // StepCleanup @377 / @435: both task lists cleared
                if let Some(concept) = self.concept {
                    for n in ["TutorialTaskList", "TutorialSingleTaskList"] {
                        let list = cx.view.tree().find_in(concept, n);
                        for c in cx.view.tree().widget(list).children.clone() {
                            MenuView::discard(cx, c);
                        }
                    }
                }
                self.tasks.clear();
                self.counts.clear();
            }
            TutorialUiCall::RemoveWidget => {
                // FinishTutorial @2100
                if let Some(concept) = self.concept.take() {
                    MenuView::discard(cx, concept);
                }
            }
            TutorialUiCall::Congrats { text } => {
                let t = Self::data(cx).and_then(|d| usize::try_from(*text).ok().and_then(|i| d.congratulations.get(i)));
                if let Some(text) = t.map(|t| tutorial_text(cx.view, t)) {
                    let p = TutorialPopText {
                        text,
                        lifetime: 1.0,
                        color: CONGRATS_COLOR,
                        location: [0.5, 0.2, 0.0],
                        attach: None,
                        scale: 2.5,
                        justification: 1,
                    };
                    if let Some(s) = cx.tutorial.as_deref_mut() {
                        s.pop_text(p);
                    }
                }
            }
            TutorialUiCall::EndScreen { has_next } => {
                // @110-257: Create UI_TutorialEndScreen, Init(IsValidClass(NextTutorial), self), AddToViewport(0)
                let w = Self::add(cx, "UI_TutorialEndScreen");
                self.end = Some(w);
                cx.visit::<TutorialEndScreen, _>(w, |e, cx| e.init(cx, *has_next));
            }
            TutorialUiCall::MinigameCounter { task: i, visible, .. } => {
                // the minigame function @3040 / @3237: BPI_SetMinigameCounterText, BPI_SetMinigameCounterVisibility.
                // The call carries the text's source string; the words are the step's task's MinigameCounterText.
                let Some(w) = task(&self.tasks, *i) else { return self.changed(cx.view) };
                let text = Self::step_of(cx, self.step)
                    .and_then(|s| usize::try_from(*i).ok().and_then(|i| s.tasks.get(i)))
                    .map(|t| tutorial_text(cx.view, &t.minigame_text))
                    .unwrap_or_default();
                let v = if *visible { Vis::Visible } else { Vis::Collapsed };
                cx.visit::<TutorialTaskWidget, _>(w, |t, cx| {
                    t.set_minigame_counter_text(cx, &text);
                    t.set_minigame_counter_visibility(cx, v);
                });
            }
            TutorialUiCall::MinigameCount { task: i, count } => {
                // TutorialMinigame_Parent IncrementMinigameCounter @328: the task widget's BPI_SetMinigameCounter
                if let Some(w) = task(&self.tasks, *i) {
                    cx.visit::<TutorialTaskWidget, _>(w, |t, cx| t.set_minigame_counter(cx, *count));
                }
            }
            TutorialUiCall::RewardPop { fighter } => {
                // Unverified: with no TutorialManagedPawn (attach actor None) the widget is taken as never placed:
                // not drawn.
                if *fighter >= 0 {
                    let p = TutorialPopText {
                        text: cx.view.game_text(REWARD_POP_TEXT).to_string(),
                        lifetime: 1.0,
                        color: REWARD_POP_COLOR,
                        location: [0.0, 0.0, 300.0],
                        attach: Some(*fighter),
                        scale: 3.0,
                        justification: 1,
                    };
                    if let Some(s) = cx.tutorial.as_deref_mut() {
                        s.pop_text(p);
                    }
                }
            }
            // UI_Game's UI_HUD_Scoreboard_Container is the HUD's, the save the match's
            TutorialUiCall::Scoreboard { .. } | TutorialUiCall::Finished { .. } => {}
        }
        self.changed(cx.view);
    }

    /// UI_Game Can Pause's tutorial tests: false while a TutorialIntroScreen exists (@599), a TutorialWidget is
    /// Interactable (@781-1109) or a UI_TutorialEndScreen exists (@1210).
    pub fn can_pause(&self, view: &MenuView) -> bool {
        let interactable = |w: WidgetId| view.controller_ref::<TutorialWidget>(w).is_some_and(|c| c.interactable);
        !(alive(view, self.intro)
            || (alive(view, self.concept) && self.concept.is_some_and(interactable))
            || alive(view, self.end))
    }

    /// A tutorial widget is in the viewport: the screen is up (it takes the page's keys, draws and runs the
    /// widgets' Delays and animations). The program sets the screen active by it after each frame and each `apply`.
    pub fn up(&self, view: &MenuView) -> bool {
        alive(view, self.intro) || alive(view, self.concept) || alive(view, self.end)
    }
}

#[cfg(test)]
mod tests {
    use super::format;

    #[test]
    fn format_fills_the_named_arguments() {
        let a = [("task", "Jump"), ("count", "1"), ("total", "3")];
        assert_eq!(format("{task} ({count}/{total})", &a), "Jump (1/3)");
        assert_eq!(format("{task} {other} {", &a), "Jump {other} {");
    }
}
