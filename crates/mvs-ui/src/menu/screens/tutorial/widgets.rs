//! The tutorial's widgets over the match: TutorialIntroScreen (the title
//! card), TutorialWidget (the concept card and its task lists), TutorialTaskWidget (a task row with its glyph row of
//! UI_InputLabel and InputButtonWidget), UI_TutorialEndScreen and UI_MinigameScore_Row. The TutorialActor's calls
//! into them are made by `TutorialScreens` (mod.rs); what they ask of the match goes through `Cx::tutorial`.

use std::any::Any;

use mvs_data::tutorial::{TutorialInput, TutorialText};

use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::manager::Manager;
use crate::menu::tutorial::TutorialPopText;
use crate::menu::view::MenuView;
use crate::menu::widgets::controls::buttons::{ButtonMain, ButtonSecondary};
use crate::menu::widgets::controls::input_label::{InputLabel, InputLabelParams};
use crate::menu::widgets::controls::interactor::{batch_set_context, set_focused, set_interactable};
use crate::menu::widgets::controls::{find, wd};
use crate::slate::{Vis, WidgetId};

/// An FText of the actor data in the player's language: the locres string of its namespace and key, else the
/// asset's source string.
pub fn tutorial_text(view: &MenuView, t: &TutorialText) -> String {
    if !t.key.is_empty()
        && let Some(s) = view.text.get(&t.namespace, &t.key)
    {
        return s.to_string();
    }
    t.source.clone()
}

/// The interactor of the button widget `w` (UI_ButtonMain and UI_ButtonSecondary alike).
fn interactor(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
    ButtonSecondary::interactor(cx, w)
}

/// `w` focused (SetFocused(false)) and its context made the level's root context.
fn focus_button(cx: &mut Cx<'_>, w: WidgetId) {
    let i = interactor(cx, w);
    set_focused(cx, i, false);
    let ctx = cx.view.manager.interactor(i).map(|s| s.context.clone()).unwrap_or_default();
    cx.view.manager.set_root_context_for_level(&ctx);
}

fn on_main_click(cx: &mut Cx<'_>, button: WidgetId, h: Handler) {
    cx.visit::<ButtonMain, _>(button, |b, _| b.on_click.add(h));
}

fn on_secondary_click(cx: &mut Cx<'_>, button: WidgetId, h: Handler) {
    cx.visit::<ButtonSecondary, _>(button, |b, _| b.on_click.add(h));
}

// ---------- TutorialIntroScreen ----------

const I_START: u32 = 1;
const I_SUBMIT3: u32 = 2;
const I_FOCUS: u32 = 1;

/// TutorialIntroScreen.
#[derive(Debug, Default)]
pub struct TutorialIntroScreen {
    submit_bound: bool,
}

impl TutorialIntroScreen {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<TutorialIntroScreen>::default()
    }

    /// Construct @15-320: AssignInputToWidget (the menu screen takes the page's keys: no counterpart),
    /// SetCursorMode(0, false), buttonStart focused, OnSubmit3 += TutorialIntroSubmit3Click (AddUnique).
    fn focus(&mut self, cx: &mut Cx<'_>) {
        cx.view.manager.cursor_mode[0] = false;
        let start = find(cx, "buttonStart");
        focus_button(cx, start);
        if !self.submit_bound {
            let me = cx.w;
            cx.view.manager.on_submit3.add(Handler { owner: me, id: I_SUBMIT3 });
            self.submit_bound = true;
        }
    }

    /// SetDetails @636 / @681.
    pub fn set_details(&mut self, cx: &mut Cx<'_>, title: &str, desc: &str) {
        let n = find(cx, "TutorialName");
        title.clone_into(&mut wd(cx, n).text);
        let t = find(cx, "TutorialText");
        desc.clone_into(&mut wd(cx, t).text);
    }

    /// buttonStart OnClick @787 -> @727.
    fn on_start(&mut self, cx: &mut Cx<'_>) {
        cx.post("ui_tutorial_start");
        // @456 BPI_IntroComplete
        if let Some(t) = cx.tutorial.as_deref_mut() {
            t.intro_complete();
        }
        // @539
        cx.view.manager.set_handle_input(false);
        if let Some(t) = cx.tutorial.as_deref_mut() {
            t.set_game_input(true);
        }
        // @576
        cx.post("ui_menu_hover_generic");
        // @362
        let me = cx.w;
        MenuView::remove_from_viewport(cx, me);
    }
}

impl Controller for TutorialIntroScreen {
    /// OnInitialized @792: UpdateContext(NewGuid), SetHandleInput(true) @889, Delay 0.35 -> the Construct body
    /// again (@16). (the port's own: the intro holds UI input, so the match's game input is off while it is up)
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let start = find(cx, "buttonStart");
        let i = interactor(cx, start);
        let g = cx.view.manager.new_guid();
        batch_set_context(cx, &[i], &g);
        cx.view.manager.set_handle_input(true);
        if let Some(t) = cx.tutorial.as_deref_mut() {
            t.set_game_input(false);
        }
        on_main_click(cx, start, Handler { owner: me, id: I_START });
        cx.view.delay(0.35, Handler { owner: me, id: I_FOCUS });
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        self.focus(cx);
    }

    /// Destruct @1079: CancelLatentActions (the Delay goes with the widget), OnSubmit3 unbound.
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_submit3.remove(Handler { owner: me, id: I_SUBMIT3 });
        self.submit_bound = false;
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            I_START => self.on_start(cx),
            // TutorialIntroSubmit3Click: buttonStart clicked (its OnClick's one handler is this widget's)
            I_SUBMIT3 => self.on_start(cx),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == I_FOCUS {
            self.focus(cx);
        }
    }
}

// ---------- TutorialWidget (the concept card) ----------

/// FocusChecker's looping timer (OnInitialized @4063).
const FOCUS_TIMER: f64 = 0.25;

const W_START: u32 = 1;
const W_SKIP: u32 = 2;
const W_SUBMIT3: u32 = 3;
/// The Delays' kinds; a Delay's handler id is its kind and the serial of the call that made it (`delay_id`).
const R_FOCUS: u32 = 0;
const R_CONCEPT: u32 = 1;

fn delay_id(serial: u32, kind: u32) -> u32 {
    serial.wrapping_mul(2).wrapping_add(kind)
}

/// TutorialWidget.
#[derive(Debug)]
pub struct TutorialWidget {
    /// Interactable (ToggleInteractable): UI_Game Can Pause reads it.
    pub interactable: bool,
    focus_timer: f64,
    submit_bound: bool,
    /// The latent Delay of StartNewConcept and of SetPlayerFocus(true): a newer call takes its place (one latent
    /// action per node: a call while it waits restarts nothing; the port keeps the newest).
    concept_serial: u32,
}

impl TutorialWidget {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(TutorialWidget {
            interactable: false,
            focus_timer: FOCUS_TIMER,
            submit_bound: false,
            concept_serial: 0,
        })
    }

    /// GetInteractors: buttonStart, buttonSkip.
    fn interactors(cx: &Cx<'_>) -> [WidgetId; 2] {
        [interactor(cx, find(cx, "buttonStart")), interactor(cx, find(cx, "buttonSkip"))]
    }

    /// FocusChecker @3957 -> @772: with the manager handling input, the focus is dropped once the panel is hidden or
    /// faded out.
    fn focus_checker(&mut self, cx: &mut Cx<'_>) {
        if !cx.view.manager.handle_input {
            return;
        }
        let parent = find(cx, "CanvasPanel_Parent");
        let border = find(cx, "Border_0");
        let t = cx.view.tree();
        let vis = t.widget(parent).vis;
        let shown = vis != Vis::Collapsed && vis != Vis::Hidden && t.widget(border).opacity > 0.0;
        if !shown {
            self.set_player_focus(cx, false);
        }
    }

    /// SetPlayerFocus @2343.
    pub fn set_player_focus(&mut self, cx: &mut Cx<'_>, b: bool) {
        // @2399
        cx.view.manager.set_handle_input(b);
        if b {
            // @2487 UpdateContext(NewGuid), Delay 0.25 -> @126: AssignInputToWidget (no counterpart),
            // SetCursorMode(0, false), buttonStart focused @302
            let list = Self::interactors(cx);
            let g = cx.view.manager.new_guid();
            batch_set_context(cx, &list, &g);
            self.concept_serial = self.concept_serial.wrapping_add(1);
            let me = cx.w;
            cx.view.delay(0.25, Handler { owner: me, id: delay_id(self.concept_serial, R_FOCUS) });
            if let Some(t) = cx.tutorial.as_deref_mut() {
                t.set_game_input(false);
            }
        } else {
            // @2611 UnfocusAll(0); @1314 / @1485 BPI_EnableGameInput and BPI_ResetInputs on every PlayerController
            Manager::unfocus_all(cx, 0);
            if let Some(t) = cx.tutorial.as_deref_mut() {
                t.set_game_input(true);
            }
        }
        // @2697
        self.toggle_interactable(cx, b);
    }

    /// ToggleInteractable @3449. buttonSkip's own visibility follows too, under its Hidden SizeBox_3: never drawn.
    fn toggle_interactable(&mut self, cx: &mut Cx<'_>, b: bool) {
        self.interactable = b;
        // @675
        for i in Self::interactors(cx) {
            set_interactable(cx, i, b);
        }
        let v = if b { Vis::Visible } else { Vis::Collapsed };
        // @3778, @3867
        for n in ["buttonSkip", "buttonStart"] {
            let w = find(cx, n);
            wd(cx, w).vis = v;
        }
        let me = cx.w;
        let h = Handler { owner: me, id: W_SUBMIT3 };
        // @3566 / @3677
        if b && !self.submit_bound {
            cx.view.manager.on_submit3.add(h);
        }
        if !b {
            cx.view.manager.on_submit3.remove(h);
        }
        self.submit_bound = b;
    }

    /// StartNewConcept(DelayStartTime) @1597.
    pub fn start_new_concept(&mut self, cx: &mut Cx<'_>, delay_start: f64) {
        for n in ["TutorialTaskList", "TutorialSingleTaskList"] {
            let w = find(cx, n);
            wd(cx, w).vis = Vis::Hidden;
        }
        self.set_player_focus(cx, false);
        // @1688: the panel fades in
        let me = cx.w;
        cx.view.play(me, "FadeConcept", true, 1.0);
        // Delay(DelayStartTime) -> @15: while IsGamePaused, Delay 1.0 and test again (@121); then SetPlayerFocus(true)
        self.concept_serial = self.concept_serial.wrapping_add(1);
        cx.view.delay(delay_start, Handler { owner: me, id: delay_id(self.concept_serial, R_CONCEPT) });
    }

    /// UpdateConceptSprite (TutorialActor @605): SetDetails, CanvasPanel_Parent Visible.
    pub fn show_concept(&mut self, cx: &mut Cx<'_>, title: &str, desc: &str) {
        self.set_details(cx, title, desc);
        let p = find(cx, "CanvasPanel_Parent");
        wd(cx, p).vis = Vis::Visible;
    }

    /// SetDetails @2800 / @2845.
    pub fn set_details(&mut self, cx: &mut Cx<'_>, title: &str, desc: &str) {
        let h = find(cx, "TextBlock_ConceptHeaderText");
        title.clone_into(&mut wd(cx, h).text);
        let d = find(cx, "TextBlock_DescriptionText");
        desc.clone_into(&mut wd(cx, d).text);
    }

    // SetProgress @2891-3403 formats key BCAAB6BB448FC2C5867962A696AFDCAF into TextBlock_Progress, which stays
    // Collapsed: nothing shows, so the port writes nothing.

    /// buttonStart OnClick @2323 -> @1794.
    fn on_start(&mut self, cx: &mut Cx<'_>) {
        if !self.interactable {
            return;
        }
        // @2242 / @2280
        for n in ["TutorialTaskList", "TutorialSingleTaskList"] {
            let w = find(cx, n);
            wd(cx, w).vis = Vis::SelfHitTestInvisible;
        }
        // @1873 BPI_UncontrolTutorialPawn(true, true, false)
        if let Some(t) = cx.tutorial.as_deref_mut() {
            t.start_pressed();
        }
        self.set_player_focus(cx, false);
        // @1928
        cx.post("ui_tutorial_prompt_continue");
        // @1983
        let p = find(cx, "CanvasPanel_Parent");
        wd(cx, p).vis = Vis::SelfHitTestInvisible;
        // @2021: fade out
        let me = cx.w;
        cx.view.play(me, "FadeConcept", false, 1.0);
    }

    /// buttonSkip OnClick @2338: unreachable (the button never draws), kept as the build has it.
    fn on_skip(cx: &mut Cx<'_>) {
        // @2182
        cx.post("ui_tutorial_prompt_continue");
        // @2144 BPI_AdvanceTutorial
        if let Some(t) = cx.tutorial.as_deref_mut() {
            t.advance();
        }
    }
}

impl Controller for TutorialWidget {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let start = find(cx, "buttonStart");
        on_main_click(cx, start, Handler { owner: me, id: W_START });
        let skip = find(cx, "buttonSkip");
        on_main_click(cx, skip, Handler { owner: me, id: W_SKIP });
    }

    /// OnInitialized @4063: Set Timer by Function Name(FocusChecker, 0.25, looping).
    fn timers(&mut self, cx: &mut Cx<'_>, dt: f64) {
        self.focus_timer -= dt;
        if self.focus_timer > 0.0 {
            return;
        }
        self.focus_timer += FOCUS_TIMER;
        self.focus_checker(cx);
    }

    /// Destruct @4167: the timer cleared (it goes with the widget), OnSubmit3 unbound.
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_submit3.remove(Handler { owner: me, id: W_SUBMIT3 });
        self.submit_bound = false;
        self.interactable = false;
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            W_START => self.on_start(cx),
            W_SKIP => Self::on_skip(cx),
            // TutorialProceedSubmit3 @4298: buttonStart's OnClick broadcast (its one handler is this widget's)
            W_SUBMIT3 => self.on_start(cx),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        // a Delay of an older call does nothing
        if id != delay_id(self.concept_serial, id % 2) {
            return;
        }
        match id % 2 {
            R_FOCUS => {
                // @126
                cx.view.manager.cursor_mode[0] = false;
                let start = find(cx, "buttonStart");
                focus_button(cx, start);
            }
            R_CONCEPT => {
                // @121: while the game is paused, Delay 1.0 and test again
                if cx.tutorial.as_deref().is_some_and(|t| t.is_game_paused()) {
                    let me = cx.w;
                    cx.view.delay(1.0, Handler { owner: me, id });
                    return;
                }
                // @105
                self.set_player_focus(cx, true);
            }
            _ => {}
        }
    }
}

// ---------- TutorialTaskWidget (a task row) and its glyphs ----------

/// The UIInputLabelParams every TaskControllerInputs entry of the TutorialActor classes' defaults holds besides its
/// Action, Modifier and Label: AutomaticInputType true, player 0, Tint (0.68, 0.8, 0.85, 1), label font Display 1 /
/// 16, key font NormsCond-ExtraBoldItalic / 16, SizeIconSize 32 x 32, no custom icon.
const GLYPH_TINT: Rgba = [0.68, 0.8, 0.85, 1.0];

const T_CONFIRM: u32 = 1;

/// TutorialTaskWidget.
#[derive(Debug, Default)]
pub struct TutorialTaskWidget {}

impl TutorialTaskWidget {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<TutorialTaskWidget>::default()
    }

    /// BPI_SetTutorialTaskText @62.
    pub fn set_text(&mut self, cx: &mut Cx<'_>, t: &str) {
        let w = find(cx, "TutorialTaskText");
        t.clone_into(&mut wd(cx, w).text);
    }

    /// BPI_JiggleTutorialTask @499 -> @10.
    pub fn jiggle(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.play(me, "JiggleTutorialTaskInfo", false, 1.0);
    }

    /// BPI_FadeInTutorialTask @428.
    pub fn fade_in(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.stop_animation(me, "FadeOutTutorialTaskText");
        cx.view.play(me, "FadeInTutorialTask", false, 1.0);
    }

    /// BPI_FadeOutTutorialTaskText @376. The animation's event track runs SoundConfirmation (ui_menu_confirm, @112)
    /// at its key (tick 3000): the menu's animations fire no event track, so a Delay of the key's time stands in.
    pub fn fade_out(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.play(me, "FadeOutTutorialTaskText", false, 1.0);
        let a = cx.view.bundle.widgets.get("TutorialTaskWidget").and_then(|c| c.anims.get("FadeOutTutorialTaskText"));
        let key = a.and_then(|a| a.events.first()).map_or(3000.0, |e| e.0);
        let res = a.map_or(60000.0, |a| a.res);
        cx.view.delay(key / res, Handler { owner: me, id: T_CONFIRM });
    }

    /// BPI_SetMinigameCounterVisibility @326.
    pub fn set_minigame_counter_visibility(&mut self, cx: &mut Cx<'_>, v: Vis) {
        let w = find(cx, "MinigameCounterContainer");
        wd(cx, w).vis = v;
    }

    /// BPI_SetMinigameCounter @227: Conv_IntToText(n).
    pub fn set_minigame_counter(&mut self, cx: &mut Cx<'_>, n: i32) {
        let w = find(cx, "TextBlock_MinigameCounter");
        wd(cx, w).text = n.to_string();
    }

    /// BPI_SetMinigameCounterText @172.
    pub fn set_minigame_counter_text(&mut self, cx: &mut Cx<'_>, t: &str) {
        let w = find(cx, "TextBlock_MiniGameText");
        t.clone_into(&mut wd(cx, w).text);
    }

    /// TutorialActor SetupInputButtons @0-2205, one task's entries into BPI_GetControlsIconsHolder (the
    /// TutorialTaskControlsIconsHolder): InputButtonWidget with its texture @880 / @1120, or UI_InputLabel with the
    /// entry's params, Refresh, added @1719-1990. (any other widget type: a PrintString)
    pub fn setup_inputs(&mut self, cx: &mut Cx<'_>, inputs: &[TutorialInput]) {
        let holder = find(cx, "TutorialTaskControlsIconsHolder");
        for e in inputs {
            if e.widget == 0 {
                let w = MenuView::create(cx, "InputButtonWidget");
                let img = cx.view.tree().find_in(w, "Image_InputButton");
                let tex = texture_entry(cx.view, &e.texture);
                wd(cx, img).tex = Some(tex);
                MenuView::add_child(cx, holder, w, None);
                continue;
            }
            let w = MenuView::create(cx, "UI_InputLabel");
            let label = tutorial_text(cx.view, &e.label);
            cx.visit::<InputLabel, _>(w, |l, cx| {
                let params = InputLabelParams {
                    action: e.action.clone(),
                    modifier: if e.modifier.is_empty() { "None".to_string() } else { e.modifier.clone() },
                    label,
                    player_index: 0,
                    automatic_input_type: true,
                    fixed_input_type: 0,
                    tint: GLYPH_TINT,
                    text_size: 16.0,
                    key_size: 16.0,
                    icon_size: [32.0, 32.0],
                    custom_icon: None,
                };
                l.update_params(cx, params);
            });
            MenuView::add_child(cx, holder, w, None);
        }
    }
}

/// A texture's brush as the widgets name it: the bundle's material path or texture entry of `name`, else the name.
fn texture_entry(view: &MenuView, name: &str) -> String {
    let b = &view.bundle;
    b.material_path(name)
        .map(str::to_string)
        .or_else(|| b.textures.get(name).map(|t| t.entry().to_string()))
        .unwrap_or_else(|| name.to_string())
}

impl Controller for TutorialTaskWidget {
    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == T_CONFIRM {
            cx.post("ui_menu_confirm");
        }
    }
}

// ---------- UI_MinigameScore_Row ----------

/// UI_MinigameScore_Row.
#[derive(Debug, Default)]
pub struct MinigameScoreRow {}

impl MinigameScoreRow {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<MinigameScoreRow>::default()
    }

    /// SetMinigameScoreboardRowValues @110.
    pub fn set_values(&mut self, cx: &mut Cx<'_>, label: &str, value: &str) {
        let l = find(cx, "MinigameLabel");
        label.clone_into(&mut wd(cx, l).text);
        let v = find(cx, "MinigameScoreCell");
        value.clone_into(&mut wd(cx, v).text);
    }
}

impl Controller for MinigameScoreRow {}

// ---------- UI_TutorialEndScreen ----------

/// Init's PopText (@3117-3624): key A4F1502B49B07E69282BFF8846B4519B, colour, place, scale, justification.
const END_POP_TEXT: &str = "A4F1502B49B07E69282BFF8846B4519B";
const END_POP_COLOR: Rgba = [0.038, 0.831, 0.309, 1.0];

const E_MAIN: u32 = 1;
const E_ADV: u32 = 2;
const E_NEXT: u32 = 3;
const E_SUBMIT3: u32 = 4;
const R_SHOW: u32 = 1;
const R_FOCUS_END: u32 = 2;
const R_EXIT: u32 = 3;
const R_EXIT_ADV: u32 = 4;

/// UI_TutorialEndScreen.
#[derive(Debug, Default)]
pub struct TutorialEndScreen {
    submit_bound: bool,
    leaving: bool,
}

impl TutorialEndScreen {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<TutorialEndScreen>::default()
    }

    /// GetInteractors: Arcade, PreMatch, AdvTutorials, MainMenu, StartFTUEBotMatch.
    fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        ["btn_ArcadeBtn", "btn_PreMatch", "btn_AdvTutorials", "btn_MainMenu", "btn_StartFTUEBotMatch"]
            .iter()
            .map(|n| interactor(cx, find(cx, n)))
            .collect()
    }

    /// Init(NextAvailable, TutorialActorRef) @2911. NextAvailable is read nowhere. The actor's
    /// BPI_DesaturateEnvironment @2990 first.
    pub fn init(&mut self, cx: &mut Cx<'_>, _next_available: bool) {
        if let Some(t) = cx.tutorial.as_deref_mut() {
            t.desaturate_environment();
        }
        let empty = Self::populate_scoreboard(cx);
        // @3117-3624 UI_Functions PopText: 20 s with no scores, else 3 s
        let p = TutorialPopText {
            text: cx.view.game_text(END_POP_TEXT).to_string(),
            lifetime: if empty { 20.0 } else { 3.0 },
            color: END_POP_COLOR,
            location: [0.5, 0.2, 0.0],
            attach: None,
            scale: 3.0,
            justification: 1,
        };
        if let Some(t) = cx.tutorial.as_deref_mut() {
            t.pop_text(p);
        }
        let me = cx.w;
        cx.view.delay(3.5, Handler { owner: me, id: R_SHOW });
    }

    /// @30-1413, the FTUE test false (the port has no FTUE).
    fn show_buttons(&mut self, cx: &mut Cx<'_>) {
        // @164 / @291 Conditional Visibility(IsConnected?, Visible, Collapsed): the offline game is not connected
        for n in ["btn_PreMatch", "btn_ArcadeBtn"] {
            let w = find(cx, n);
            wd(cx, w).vis = Vis::Collapsed;
        }
        // @431-609 Visible; @757-935 the FTUE's Collapsed
        for n in ["ButtonHolder", "Buttons", "WBP_Quadrilateral_1"] {
            let w = find(cx, n);
            wd(cx, w).vis = Vis::Visible;
        }
        for n in ["FTUEButtons", "FTUEButtonOverlay", "WBP_Quadrilateral_2"] {
            let w = find(cx, n);
            wd(cx, w).vis = Vis::Collapsed;
        }
        // @1024
        Self::populate_scoreboard(cx);
        // @1094 AssignInputToWidget (no counterpart), @1164 UpdateContext(NewGuid), @1238 the cursor's override,
        // @1322 SetHandleInput(true), Delay 0.25
        let list = Self::interactors(cx);
        let g = cx.view.manager.new_guid();
        batch_set_context(cx, &list, &g);
        cx.view.manager.set_handle_input(true);
        let me = cx.w;
        cx.view.delay(0.25, Handler { owner: me, id: R_FOCUS_END });
    }

    /// PopulateScoreboard: the actor's BPI_GetTutorialMinigameScores, one UI_MinigameScore_Row per entry with its
    /// ScoreDescriptor (the task's MinigameCounterText) and Conv_IntToText(Score, false, true, 1, 324) (grouping on,
    /// @862). Returns whether the list is empty.
    fn populate_scoreboard(cx: &mut Cx<'_>) -> bool {
        let table = find(cx, "ScoreTable");
        // @5 ClearChildren
        for c in cx.view.tree().widget(table).children.clone() {
            MenuView::discard(cx, c);
        }
        let rows: Vec<(String, String)> = match cx.tutorial.as_deref() {
            Some(t) => t
                .minigame_scores()
                .iter()
                .map(|s| {
                    let d = t
                        .actor_data()
                        .and_then(|d| d.steps.get(usize::try_from(s.step).ok()?))
                        .and_then(|st| st.tasks.get(usize::try_from(s.task).ok()?))
                        .map(|task| tutorial_text(cx.view, &task.minigame_text))
                        .unwrap_or_default();
                    (d, grouped(s.score))
                })
                .collect(),
            None => Vec::new(),
        };
        // @318
        let b = find(cx, "Border_Scoreboard");
        wd(cx, b).vis = if rows.is_empty() { Vis::Collapsed } else { Vis::HitTestInvisible };
        for (label, value) in &rows {
            let row = MenuView::create(cx, "UI_MinigameScore_Row");
            // @920
            cx.visit::<MinigameScoreRow, _>(row, |r, cx| r.set_values(cx, label, value));
            // @983
            MenuView::add_child(cx, table, row, None);
        }
        rows.is_empty()
    }

    /// BackToMainMenu @4858 (btn_AdvTutorials first BPI_SetAdvTutShortcut(true) @4790, which the session's exit
    /// carries): Pre-Transition Cleanup (@6146: analytics, BPI_SetAttackDecayEnabled(true) @6334: the match's own,
    /// post ui_menu_confirm @6372), BPI_RecreateParty (none offline), Delay 0.5 -> @2452: the Transition to
    /// New_MainMenu "Lobby" (the session's exit), OnSubmit3 unbound.
    fn back_to_main_menu(&mut self, cx: &mut Cx<'_>, adv_tut_shortcut: bool) {
        if self.leaving {
            return;
        }
        self.leaving = true;
        cx.post("ui_menu_confirm");
        let me = cx.w;
        cx.view.delay(0.5, Handler { owner: me, id: if adv_tut_shortcut { R_EXIT_ADV } else { R_EXIT } });
    }

    /// NextTutorial OnClick @4630 -> @3745.
    fn on_next(&mut self, cx: &mut Cx<'_>) {
        // @4032
        if let Some(t) = cx.tutorial.as_deref_mut() {
            t.start_next();
        }
        let me = cx.w;
        cx.view.manager.on_submit3.remove(Handler { owner: me, id: E_SUBMIT3 });
        self.submit_bound = false;
    }
}

/// Conv_IntToText with grouping (the en-US digit groups).
fn grouped(n: i32) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

impl Controller for TutorialEndScreen {
    /// OnInitialized @4205: the cursor shown (UI_Cursor's override: the port's cursor follows handleInput),
    /// SetHandleInput(true). (the port's own: the match's game input off while it is up)
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.set_handle_input(true);
        if let Some(t) = cx.tutorial.as_deref_mut() {
            t.set_game_input(false);
        }
        // @4843, @5532 -> @4690
        let main = find(cx, "btn_MainMenu");
        on_secondary_click(cx, main, Handler { owner: me, id: E_MAIN });
        let adv = find(cx, "btn_AdvTutorials");
        on_secondary_click(cx, adv, Handler { owner: me, id: E_ADV });
        // NextTutorial (@4630 -> @3745) is never made visible or enabled; bound as the build has it.
        let next = find(cx, "NextTutorial");
        on_main_click(cx, next, Handler { owner: me, id: E_NEXT });
        // btn_PreMatch (@5148) and btn_ArcadeBtn (@5785 -> @5014 BPI_EnterPreMatch) are the online queue's,
        // collapsed offline; btn_StartFTUEBotMatch (@5790) the FTUE's, never shown. Left unbound.
    }

    /// Destruct @4378: the cursor's override off, SetHandleInput(false).
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_submit3.remove(Handler { owner: me, id: E_SUBMIT3 });
        self.submit_bound = false;
        cx.view.manager.set_handle_input(false);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            E_MAIN => self.back_to_main_menu(cx, false),
            E_ADV => self.back_to_main_menu(cx, true),
            E_NEXT => self.on_next(cx),
            // TutorialEndScreenLeaveSubmit3 @5537: BPI_IsForcedOfflineMode (the offline game: true) -> BackToMainMenu
            E_SUBMIT3 => self.back_to_main_menu(cx, false),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        let me = cx.w;
        match id {
            R_SHOW => self.show_buttons(cx),
            R_FOCUS_END => {
                // @1460
                cx.view.manager.cursor_mode[0] = false;
                // @1944: not connected: btn_MainMenu focused (the cursor's MoveToInteractor @2207 not ported)
                let main = find(cx, "btn_MainMenu");
                focus_button(cx, main);
                // @2410
                if !self.submit_bound {
                    cx.view.manager.on_submit3.add(Handler { owner: me, id: E_SUBMIT3 });
                }
                self.submit_bound = true;
            }
            R_EXIT | R_EXIT_ADV => {
                cx.view.manager.on_submit3.remove(Handler { owner: me, id: E_SUBMIT3 });
                self.submit_bound = false;
                if let Some(t) = cx.tutorial.as_deref_mut() {
                    t.exit(id == R_EXIT_ADV);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::grouped;

    #[test]
    fn scores_are_grouped_by_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(1234567), "1,234,567");
        assert_eq!(grouped(-4200), "-4,200");
    }
}
