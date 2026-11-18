//! The tutorial's widgets on the menu bundle with a fake tutorial session: the intro card and the concept card with
//! its task rows and glyphs, the task counter's text, the end screen with its score rows, and the advanced tutorials'
//! tile modal with its badges and launches. Needs the dev store; without one each test says so and passes.

use std::sync::Arc;

use mvs_data::TutorialActorData;
use mvs_data::tutorial::{TutorialInput, TutorialStep, TutorialTask, TutorialText};
use mvs_settings::Settings;
use mvs_settings::stats::StatsFile;
use mvs_sim::events::TutorialUiCall;
use mvs_sim::modes::tutorial::TutorialScore;
use mvs_ui::bindings::Bindings;
use mvs_ui::menu::controller::{Cx, MenuRun};
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::screens::tutorial::TutorialScreens;
use mvs_ui::menu::tutorial::{TutorialPopText, TutorialSession};
use mvs_ui::menu::widgets::controls::interactor;
use mvs_ui::menu::widgets::match_options::GameModeCard;
use mvs_ui::menu::widgets::modals::show_modal_generic;
use mvs_ui::menu::widgets::nav::live;
use mvs_ui::menu::{MenuScreen, MenuView, NoSound};
use mvs_ui::slate::{Vis, WidgetId};
use mvs_ui::text::{Fonts, GameText};
use mvs_ui::{Bundle, ElementList};

#[derive(Default)]
struct Fake {
    data: Option<TutorialActorData>,
    scores: Vec<TutorialScore>,
    calls: Vec<String>,
    pops: Vec<TutorialPopText>,
    game_input: Option<bool>,
    paused: bool,
    clock: f64,
}

impl TutorialSession for Fake {
    fn intro_complete(&mut self) {
        self.calls.push("intro_complete".into());
    }
    fn start_pressed(&mut self) {
        self.calls.push("start_pressed".into());
    }
    fn advance(&mut self) {
        self.calls.push("advance".into());
    }
    fn start_next(&mut self) {
        self.calls.push("start_next".into());
    }
    fn desaturate_environment(&mut self) {
        self.calls.push("desaturate".into());
    }
    fn actor_data(&self) -> Option<&TutorialActorData> {
        self.data.as_ref()
    }
    fn minigame_scores(&self) -> &[TutorialScore] {
        &self.scores
    }
    fn is_game_paused(&self) -> bool {
        self.paused
    }
    fn set_game_input(&mut self, on: bool) {
        self.game_input = Some(on);
    }
    fn exit(&mut self, adv_tut_shortcut: bool) {
        self.calls.push(format!("exit({adv_tut_shortcut})"));
    }
    fn pop_text(&mut self, p: TutorialPopText) {
        self.pops.push(p);
    }
}

struct Env {
    bundle: Arc<Bundle>,
    text: Arc<GameText>,
    fonts: Fonts,
    prefs: Settings,
    bindings: Bindings,
    sink: NoSound,
    pointer: Pointer,
}

fn env() -> Option<Env> {
    let store = match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) if s.version("font").is_some() && s.version("ui").is_some() => s,
        _ => {
            eprintln!("no dev store with the font and ui kinds: skipped");
            return None;
        }
    };
    let bundle = match Bundle::load(&store, "menu") {
        Ok(b) => b,
        Err(e) => {
            eprintln!("no menu bundle ({e}): skipped");
            return None;
        }
    };
    if !bundle.widgets.contains_key("TutorialWidget") || !bundle.widgets.contains_key("UI_TutorialEndScreen") {
        eprintln!("no tutorial widgets in the bundle: skipped");
        return None;
    }
    let fonts = Fonts::new(&store, &bundle).unwrap();
    let text = GameText::load(&store).unwrap_or_default();
    Some(Env {
        bundle: Arc::new(bundle),
        text: Arc::new(text),
        fonts,
        prefs: Settings::default(),
        bindings: Bindings::default(),
        sink: NoSound,
        pointer: Pointer::default(),
    })
}

impl Env {
    fn run<'a>(&'a mut self, t: &'a mut Fake) -> MenuRun<'a> {
        MenuRun {
            fonts: &mut self.fonts,
            prefs: &mut self.prefs,
            bindings: &mut self.bindings,
            sink: &mut self.sink,
            pointer: &mut self.pointer,
            lab: None,
            pause: None,
            tutorial: Some(t),
        }
    }

    fn screen(&mut self, t: &mut Fake) -> MenuScreen {
        let mut view = MenuView::new(self.bundle.clone(), self.text.clone(), [1600, 900]);
        view.session.stats = StatsFile::memory();
        MenuScreen::new(view, &mut self.run(t))
    }
}

fn tick(s: &mut MenuScreen, e: &mut Env, t: &mut Fake, until: f64) {
    while t.clock < until {
        t.clock = (t.clock + 0.03).min(until);
        s.view.set_now(t.clock);
        s.with_cx(&mut e.run(t), MenuView::tick);
    }
}

fn apply(s: &mut MenuScreen, e: &mut Env, t: &mut Fake, screens: &mut TutorialScreens, call: TutorialUiCall) {
    s.with_cx(&mut e.run(t), |cx| screens.apply(cx, &call));
}

fn submit3(s: &mut MenuScreen, e: &mut Env, t: &mut Fake) {
    s.with_cx(&mut e.run(t), |cx| cx.fire_manager(|m| &m.on_submit3, &0i32));
}

fn tt(source: &str) -> TutorialText {
    TutorialText { namespace: String::new(), key: String::new(), source: source.into() }
}

fn input(widget: u8, action: &str) -> TutorialInput {
    TutorialInput {
        widget,
        texture: if widget == 0 { "Comma-outline".into() } else { String::new() },
        action: action.into(),
        modifier: String::new(),
        label: tt(""),
    }
}

fn task(text: &str, counter: bool, req: i32, inputs: Vec<TutorialInput>) -> TutorialTask {
    TutorialTask {
        text: tt(text),
        event: 0,
        req,
        counter,
        custom_actors: Vec::new(),
        minigame_counter: false,
        save_score: false,
        minigame_text: tt("Targets"),
        inputs,
    }
}

fn step(header: &str, tasks: Vec<TutorialTask>) -> TutorialStep {
    TutorialStep {
        new_concept: true,
        header: tt(header),
        text: tt("Concept text"),
        start_button_disabled_time: 0.5,
        player: String::new(),
        character: None,
        player_tag: String::new(),
        dummy: None,
        dummy_character: None,
        spawn_target_dummy: false,
        dummy_controller: None,
        bag_percent: 0.0,
        bag_tag: String::new(),
        disable_cooldowns: false,
        demo: None,
        vo: None,
        tasks,
    }
}

fn data() -> TutorialActorData {
    TutorialActorData {
        actor: "TutorialActor_Test".into(),
        name: tt("Test Tutorial"),
        intro_text: tt("Intro text"),
        intro_vo: None,
        next: None,
        complete_key: None,
        congratulations: vec![tt("Nice")],
        steps: vec![
            step(
                "Jumping",
                vec![task("Jump", true, 3, vec![input(1, "GameJump"), input(0, ""), input(1, "GameJump")])],
            ),
            step(
                "Two tasks",
                vec![task("Attack", false, 1, vec![input(1, "GameAttack")]), task("Special", true, 2, vec![])],
            ),
        ],
    }
}

fn text_of(cx: &Cx<'_>, under: WidgetId, name: &str) -> String {
    cx.view.tree().widget(cx.view.tree().find_in(under, name)).text.clone()
}

#[test]
fn the_intro_card_and_the_concept_card_take_input_in_turn() {
    let Some(mut e) = env() else { return };
    let mut t = Fake { data: Some(data()), ..Fake::default() };
    let mut s = e.screen(&mut t);
    let mut screens = TutorialScreens::new();
    assert!(!screens.up(&s.view) && screens.can_pause(&s.view));

    // the intro card: SetDetails, UI input held, no pause; Submit3 presses its Start
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::Intro);
    assert!(screens.up(&s.view) && !screens.can_pause(&s.view));
    assert_eq!(t.game_input, Some(false));
    let intro = s.with_cx(&mut e.run(&mut t), |cx| live(cx, "TutorialIntroScreen")[0]);
    let name = s.with_cx(&mut e.run(&mut t), |cx| text_of(cx, intro, "TutorialName"));
    assert_eq!(name, "Test Tutorial");
    tick(&mut s, &mut e, &mut t, 0.5);
    submit3(&mut s, &mut e, &mut t);
    tick(&mut s, &mut e, &mut t, 0.6);
    assert_eq!(t.calls, ["intro_complete"]);
    assert_eq!(t.game_input, Some(true));
    assert!(!screens.up(&s.view) && screens.can_pause(&s.view));

    // the concept card and the first step's one task row (the single list), its glyph row of three
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::Widget);
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::Setup { step: 0, index: 0, count: 2 });
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::Concept { step: 0, start_disabled: 0.5 });
    let concept = s.with_cx(&mut e.run(&mut t), |cx| live(cx, "TutorialWidget")[0]);
    let (header, single, many, glyphs, task_text) = s.with_cx(&mut e.run(&mut t), |cx| {
        let tree = cx.view.tree();
        let single = tree.find_in(concept, "TutorialSingleTaskList");
        let many = tree.find_in(concept, "TutorialTaskList");
        let row = tree.widget(single).children[0];
        let holder = tree.find_in(row, "TutorialTaskControlsIconsHolder");
        (
            text_of(cx, concept, "TextBlock_ConceptHeaderText"),
            tree.widget(single).children.len(),
            tree.widget(many).children.len(),
            tree.widget(holder).children.len(),
            text_of(cx, row, "TutorialTaskText"),
        )
    });
    assert_eq!(header, "Jumping");
    assert_eq!((single, many, glyphs), (1, 0, 3));
    assert!(task_text.contains("Jump") && task_text.contains('0') && task_text.contains('3'), "{task_text}");
    // the Continue button waits StartButtonDisabledTime, then the card holds input and the pause key does nothing
    assert!(screens.can_pause(&s.view));
    tick(&mut s, &mut e, &mut t, 1.3);
    assert!(!screens.can_pause(&s.view));
    assert_eq!(t.game_input, Some(false));
    submit3(&mut s, &mut e, &mut t);
    assert_eq!(t.calls, ["intro_complete", "start_pressed"]);
    assert!(screens.can_pause(&s.view));
    assert_eq!(t.game_input, Some(true));
    let vis = s.view.tree().widget(s.view.tree().find_in(concept, "CanvasPanel_Parent")).vis;
    assert_eq!(vis, Vis::SelfHitTestInvisible);
}

#[test]
fn a_concept_waits_while_the_game_is_paused() {
    let Some(mut e) = env() else { return };
    let mut t = Fake { data: Some(data()), paused: true, ..Fake::default() };
    let mut s = e.screen(&mut t);
    let mut screens = TutorialScreens::new();
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::Widget);
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::Setup { step: 0, index: 0, count: 2 });
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::Concept { step: 0, start_disabled: 0.5 });
    tick(&mut s, &mut e, &mut t, 2.0);
    assert!(screens.can_pause(&s.view), "paused: the card does not take input");
    t.paused = false;
    tick(&mut s, &mut e, &mut t, 3.0);
    assert!(!screens.can_pause(&s.view));
}

#[test]
fn the_task_rows_count_and_clear() {
    let Some(mut e) = env() else { return };
    let mut t = Fake { data: Some(data()), ..Fake::default() };
    let mut s = e.screen(&mut t);
    let mut screens = TutorialScreens::new();
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::Widget);
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::Setup { step: 1, index: 1, count: 2 });
    let concept = s.with_cx(&mut e.run(&mut t), |cx| live(cx, "TutorialWidget")[0]);
    let rows = |s: &MenuScreen| {
        let tree = s.view.tree();
        tree.widget(tree.find_in(concept, "TutorialTaskList")).children.clone()
    };
    assert_eq!(rows(&s).len(), 2, "two tasks: the list of many");
    // UpdateTutorialText: the counter's task gets "<text> (<count>/<req>)"; the other keeps its text
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::TaskCount { task: 1, count: 1, index: 1, total: 2 });
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::Jiggle { task: 1 });
    let texts: Vec<String> = s.with_cx(&mut e.run(&mut t), |cx| {
        let tree = cx.view.tree();
        tree.widget(tree.find_in(concept, "TutorialTaskList"))
            .children
            .clone()
            .into_iter()
            .map(|r| text_of(cx, r, "TutorialTaskText"))
            .collect()
    });
    assert_eq!(texts, ["Attack", "Special (1/2)"]);
    // the minigame counter: the task's MinigameCounterText, then its count
    apply(
        &mut s,
        &mut e,
        &mut t,
        &mut screens,
        TutorialUiCall::MinigameCounter { task: 0, text: mvs_sim::names::Name::NONE, visible: true },
    );
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::MinigameCount { task: 0, count: 4 });
    let row0 = rows(&s)[0];
    let (label, count, vis) = s.with_cx(&mut e.run(&mut t), |cx| {
        let tree = cx.view.tree();
        (
            text_of(cx, row0, "TextBlock_MiniGameText"),
            text_of(cx, row0, "TextBlock_MinigameCounter"),
            tree.widget(tree.find_in(row0, "MinigameCounterContainer")).vis,
        )
    });
    assert_eq!((label.as_str(), count.as_str(), vis), ("Targets", "4", Vis::Visible));
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::FadeOut { task: 0 });
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::Congrats { text: 0 });
    assert_eq!(t.pops.last().map(|p| (p.text.as_str(), p.attach)), Some(("Nice", None)));
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::Cleanup);
    assert!(rows(&s).is_empty());
    // past the last step the widget goes
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::Setup { step: 2, index: 2, count: 2 });
    assert!(!screens.up(&s.view));
}

#[test]
fn the_end_screen_shows_its_scores_and_leaves() {
    let Some(mut e) = env() else { return };
    let scores = vec![TutorialScore { step: 0, task: 0, score: 1234 }, TutorialScore { step: 1, task: 1, score: 7 }];
    let mut t = Fake { data: Some(data()), scores, ..Fake::default() };
    let mut s = e.screen(&mut t);
    let mut screens = TutorialScreens::new();
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::EndScreen { has_next: false });
    assert!(screens.up(&s.view) && !screens.can_pause(&s.view));
    assert_eq!(t.calls, ["desaturate"]);
    assert_eq!(t.pops.last().map(|p| p.lifetime), Some(3.0));
    let end = s.with_cx(&mut e.run(&mut t), |cx| live(cx, "UI_TutorialEndScreen")[0]);
    let rows: Vec<(String, String)> = s.with_cx(&mut e.run(&mut t), |cx| {
        let tree = cx.view.tree();
        tree.widget(tree.find_in(end, "ScoreTable"))
            .children
            .clone()
            .into_iter()
            .map(|r| (text_of(cx, r, "MinigameLabel"), text_of(cx, r, "MinigameScoreCell")))
            .collect()
    });
    assert_eq!(rows, [("Targets".to_string(), "1,234".to_string()), ("Targets".to_string(), "7".to_string())]);
    // the buttons after 3.5 s: online ones collapsed; Submit3 leaves for the main menu after 0.5 s
    tick(&mut s, &mut e, &mut t, 4.0);
    let (holder, arcade) = {
        let tree = s.view.tree();
        (tree.widget(tree.find_in(end, "ButtonHolder")).vis, tree.widget(tree.find_in(end, "btn_ArcadeBtn")).vis)
    };
    assert_eq!((holder, arcade), (Vis::Visible, Vis::Collapsed));
    submit3(&mut s, &mut e, &mut t);
    tick(&mut s, &mut e, &mut t, 4.6);
    assert_eq!(t.calls, ["desaturate", "exit(false)"]);
}

#[test]
fn view_more_tutorials_exits_with_the_shortcut() {
    let Some(mut e) = env() else { return };
    let mut t = Fake { data: Some(data()), ..Fake::default() };
    let mut s = e.screen(&mut t);
    let mut screens = TutorialScreens::new();
    apply(&mut s, &mut e, &mut t, &mut screens, TutorialUiCall::EndScreen { has_next: false });
    assert_eq!(t.pops.last().map(|p| p.lifetime), Some(20.0), "no scores: 20 s");
    tick(&mut s, &mut e, &mut t, 4.0);
    let end = s.with_cx(&mut e.run(&mut t), |cx| live(cx, "UI_TutorialEndScreen")[0]);
    s.with_cx(&mut e.run(&mut t), |cx| {
        let b = cx.view.tree().find_in(end, "btn_AdvTutorials");
        let i = cx.view.tree().find_in(b, "UI_Interactor");
        interactor::set_focused(cx, i, false);
        cx.view.manager.interactor_mut(i).unwrap().is_focused = true;
        interactor::submit(cx, i, 0);
    });
    tick(&mut s, &mut e, &mut t, 5.0);
    assert_eq!(t.calls, ["desaturate", "exit(true)"]);
}

#[test]
fn the_tile_modal_holds_twelve_tiles_without_badges_and_launches() {
    let Some(mut e) = env() else { return };
    let mut t = Fake::default();
    let mut s = e.screen(&mut t);
    let mut record = s.view.session.stats.tutorial();
    record.tutorials_complete.insert("DodgeMeterTutorialComplete".into(), true);
    assert!(s.view.session.stats.set_tutorial(record));
    s.with_cx(&mut e.run(&mut t), |cx| {
        show_modal_generic(cx, false, "WBP_SubOptionModal", "SubOption", "advancedtutorial|");
    });
    tick(&mut s, &mut e, &mut t, 1.0);
    let tiles = s.with_cx(&mut e.run(&mut t), |cx| live(cx, "WBP_TutorialType"));
    assert_eq!(tiles.len(), 12);
    let badges: Vec<Vis> = s.with_cx(&mut e.run(&mut t), |cx| {
        tiles
            .iter()
            .map(|&w| {
                let tree = cx.view.tree();
                let att = tree.find_in(w, "UI_Attention");
                tree.widget(tree.find_in(att, "MainContainer")).vis
            })
            .collect()
    });
    assert!(badges.iter().all(|v| *v == Vis::Collapsed), "no tile shows a badge (the user's choice)");

    // a tile's click launches its tutorial (the transition takes the menus away); Dodging is the first tile
    let first = tiles[0];
    s.with_cx(&mut e.run(&mut t), |cx| {
        let b = cx.view.tree().find_in(first, "UI_ButtonSecondary");
        let i = cx.view.tree().find_in(b, "UI_Interactor");
        interactor::set_focused(cx, i, false);
        cx.view.manager.interactor_mut(i).unwrap().is_focused = true;
        interactor::submit(cx, i, 0);
    });
    tick(&mut s, &mut e, &mut t, 1.5);
    assert_eq!(s.view.session.tutorial_actor.as_deref(), Some("TutorialActor_DodgeMeter"));
}

/// Frames of the screen (layout and paint) from `t0` for `secs`, 60 a second.
fn frames(s: &mut MenuScreen, e: &mut Env, t: &mut Fake, t0: f64, secs: f64) {
    let n = (secs * 60.0) as usize;
    for f in 0..=n {
        let mut out = ElementList::default();
        s.frame(&mut e.run(t), &[], t0 + f as f64 / 60.0, &mut out);
    }
}

#[test]
fn the_practice_tab_runs_the_two_tutorial_cards() {
    let Some(mut e) = env() else { return };
    let mut t = Fake::default();
    let mut s = e.screen(&mut t);
    let m = s.with_cx(&mut e.run(&mut t), |cx| show_modal_generic(cx, true, "WBP_MatchOptions", "matchoptions", ""));
    let m = m.expect("match options are shown");
    frames(&mut s, &mut e, &mut t, 0.0, 1.0);
    for (n, on) in [("Tutorial_beg", true), ("Tutorial_adv", true), ("Tutorial_chars", false), ("TheLab", true)] {
        let w = s.view.tree().find_in(m, &format!("WBP_GameModeCard_{n}"));
        let enabled = s.with_cx(&mut e.run(&mut t), |cx| cx.visit::<GameModeCard, _>(w, |c, _| c.is_enabled));
        assert_eq!(enabled, Some(on), "{n}");
    }
}
