//! A tutorial match's program side: the level's match
//! (TutorialGameMode on Map_TrophyRoom_Tutorial) and its cast, the session the tutorial widgets call (the calls
//! on the World between two steps), the widgets' screen over the match fed the step machine's calls in order, the
//! game input gate, the PlayerController's CanPause and the RTPC tutorial_controlled_pawn after each tick, and the
//! finish's save.

use std::collections::VecDeque;

use mvs_data::{TutorialActorData, Tutorials};
use mvs_settings::stats::StatsFile;
use mvs_sim::World;
use mvs_sim::events::{Event, TutorialUiCall};
use mvs_sim::modes::tutorial::{TutorialScore, classes, run};
use mvs_sim::setup::{MatchMode, MatchPlayer, MatchSetup};
use mvs_sim::world::pawns;
use mvs_ui::element::ElementList;
use mvs_ui::hud::Hud;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::screens::tutorial::TutorialScreens;
use mvs_ui::menu::tutorial::{INTRO_TUTORIAL, TutorialPopText, TutorialSession};
use mvs_ui::menu::{MenuExit, MenuInput, MenuScreen, MenuView};

use crate::app::{MatchKind, MatchStart, Shell};
use crate::menu_audio::MenuAudio;
use crate::run::STEP;
use crate::{Error, Result};

/// The tutorial level's stage (Map_TrophyRoom_Tutorial: the level has no MapData, its stage is the level's own).
pub const TUTORIAL_STAGE: &str = "trophyroom_tutorial";

/// The Combat, Damage and IntroFinale demo controllers spawn Pawn_WonderWomanV3 themselves (their ReceivePossess
/// chain), as the advanced minigames' enemies do: a class no step names.
const DEMO_SPAWNED: [&str; 1] = ["wonderwoman"];

/// tutorialCast: the characters a tutorial actor may spawn: its steps' PlayerClass and DummyClass characters and the
/// demos' own, and those its demo controllers' and minigames' actors are drawn with (`classes::drawn_with`), in
/// roster order (a key outside the roster, a data-only pawn, after them in the order met).
pub fn tutorial_cast(data: &TutorialActorData) -> Vec<String> {
    let mut keys: Vec<String> = DEMO_SPAWNED.iter().map(|&k| k.to_owned()).collect();
    let add = |k: &str, keys: &mut Vec<String>| {
        if !keys.iter().any(|x| x == k) {
            keys.push(k.to_owned());
        }
    };
    for s in &data.steps {
        if let Some(c) = &s.character {
            add(c, &mut keys);
        }
        if let Some(c) = &s.dummy_character {
            add(c, &mut keys);
        }
        let classes = s.demo.iter().chain(s.tasks.iter().flat_map(|t| t.custom_actors.iter()));
        for cls in classes {
            for k in classes::drawn_with(cls) {
                add(k, &mut keys);
            }
        }
    }
    let roster = &mvs_protocol::roster::CHARACTERS;
    let mut out: Vec<String> = roster.iter().filter(|k| keys.iter().any(|x| x == *k)).map(|&k| k.to_owned()).collect();
    out.extend(keys.into_iter().filter(|k| !roster.contains(&k.as_str())));
    out
}

/// tutorialMatch: the tutorial level's match. TutorialGameMode's set-up (InitializePlayerDataManager @5117:
/// CreateMatchConfigObject(1, 0, 6, 0, "Tutorial"), player 0 the local player). The game mode spawns every pawn
/// itself (its actor's steps), so the set-up's one slot is the local player's, with no character; no rules (no
/// score, timer or win check: the game mode's match overrides are empty), no countdown flow. `actor`: the game
/// instance's TutorialActorClass (Intro when none was set). None when the store has no tutorial data or none of
/// `actor`.
pub fn tutorial_match(
    tutorials: &Tutorials,
    actor: Option<&str>,
    settings: &mvs_settings::Settings,
    roster: &[mvs_ui::bundle::RosterEntry],
    seed: i32,
) -> Option<MatchStart> {
    let actor = actor.unwrap_or(INTRO_TUTORIAL);
    let Some(data) = tutorials.get(actor) else {
        let why = if tutorials.is_empty() { "no tutorial data" } else { "not in the tutorial data" };
        eprintln!("mvs: tutorial {actor}: {why}");
        return None;
    };
    let mut setup = MatchSetup {
        players: vec![MatchPlayer { bot: false, team: Some(0), ..MatchPlayer::default() }],
        mode: Some(MatchMode::Tutorial { actor: actor.to_owned(), cast: tutorial_cast(data) }),
        ..MatchSetup::default()
    };
    // (the local player's ring-out and input preferences, as an offline match's player gets them)
    let own_ring_out = mvs_ui::menu::lobby::loadout::local_ring_out(&settings.hydra);
    mvs_ui::menu::lobby::loadout::with_local_loadout(
        &mut setup,
        &settings.hydra,
        roster,
        &mut |_, _| own_ring_out.clone(),
        &mut |_, _| String::new(),
        &mut |_, _| Some(mvs_ui::menu::prefs::sim_input_prefs(&settings.input, 0)),
    );
    let mut start = MatchStart::new(setup, MatchKind::Tutorial, seed);
    start.stage = TUTORIAL_STAGE.to_owned();
    start.usernames = vec![settings.profile.name.clone()];
    Some(start)
}

/// The store's tutorial data (`/GameData/Tutorials`): empty when the store has none.
pub fn stored_tutorials(store: &mvs_store::Store) -> std::sync::Arc<Tutorials> {
    let has = store.kind(mvs_data::KIND).is_ok_and(|p| p.has(mvs_data::path::TUTORIALS));
    if !has {
        return std::sync::Arc::default();
    }
    mvs_data::shared::<Tutorials>(store, mvs_data::path::TUTORIALS).unwrap_or_else(|e| {
        eprintln!("mvs: the tutorial data: {e}");
        std::sync::Arc::default()
    })
}

/// The actor class's FinishTutorial, the save. TutorialActor_Intro FinishTutorial @365 (finishIntroTutorial):
/// BPI_LoadTutorialQuestData (@470), then BPI_SaveTutorialQuestData (@977) with a MakeStruct (@616-940) of
/// BasicTutorialComplete true and the other bools and XP copied; TutorialsComplete is no pin of that MakeStruct:
/// saved empty, as the build wipes the advanced tutorials' map. TutorialActor_Advanced FinishTutorial (ubergraph @169;
/// every advanced child calls it, finishAdvancedTutorial): nothing more when TutorialsComplete already holds the
/// class's tutorialCompleteKey (@384-398), else Map_Add(key, true), the rest kept, and the save (@472-746);
/// AdvancedTutorialComplete / Claimed are not written. The analytics after it (@814-3146) are not ported.
// Deviation: the Shaggy reward of the Intro's step 2 (an account owning neither Shaggy nor Wonder Woman, connected)
// is never presented: the port owns the roster and has no backend reward.
pub fn finish_tutorial(stats: &mut StatsFile, tutorials: &Tutorials, actor: &str) {
    let mut r = stats.tutorial();
    if actor == INTRO_TUTORIAL {
        r.basic_tutorial_complete = true;
        r.tutorials_complete.clear();
    } else if let Some(key) = tutorials.get(actor).and_then(|d| d.complete_key.as_ref()) {
        if r.tutorials_complete.contains_key(key) {
            return;
        }
        r.tutorials_complete.insert(key.clone(), true);
    } else {
        eprintln!("mvs: tutorial {actor}: its FinishTutorial is not ported");
        return;
    }
    if !stats.set_tutorial(r) {
        eprintln!("mvs: tutorial {actor}: the tutorial record was not saved");
    }
}

/// What the widgets' calls leave for the program, and the state kept between ticks.
#[derive(Debug)]
struct State {
    /// The step machine's calls a page call raised, applied once the call has returned, in order.
    pending: VecDeque<TutorialUiCall>,
    /// A call's other events: they go with the next tick's.
    carried: Vec<Event>,
    /// The local controllers' game input (TutorialWidget SetPlayerFocus turns it off and on).
    game_input: bool,
    /// Game input came back on: every device's queued presses dropped (PFGPlayerController ResetInputs).
    reset_inputs: bool,
    /// The end screen's way back to the main menu (`Some(adv_tut_shortcut)`).
    exit: Option<bool>,
    /// PFGPlayerController CanPause as the tutorial sets it (TutorialActor UncontrolTutorialPawn @2056
    /// BPI_SetCanPause(true)).
    pc_can_pause: bool,
    /// The run's pawn was held by the tutorial at an earlier tick.
    held: bool,
    /// The pawn and value the RTPC was last set on (-1: none).
    rtpc_pawn: i32,
    rtpc_value: i32,
}

impl State {
    fn new() -> State {
        State {
            pending: VecDeque::new(),
            carried: Vec::new(),
            game_input: true,
            reset_inputs: false,
            exit: None,
            pc_can_pause: false,
            held: false,
            rtpc_pawn: -1,
            rtpc_value: -1,
        }
    }

    /// [`TutorialHost::after_tick`].
    fn after_tick(&mut self, world: &World, me: usize) -> Option<(i32, f64)> {
        let controlled = run::pawn_controlled_by_tutorial(world)?;
        if controlled {
            self.held = true;
        } else if self.held {
            self.held = false;
            self.pc_can_pause = true;
            set_game_input(self, true);
        }
        // (the managed pawn: the one bound to the local player's slot)
        let me = i32::try_from(me).unwrap_or(-1);
        let pawn = pawns::match_fighters_iter(world).find(|&f| pawns::slot_of(world, f) == me);
        let id = pawn.map_or(-1, |f| world.fighter(f).id);
        let v = i32::from(controlled);
        let set = (id >= 0 && (id != self.rtpc_pawn || v != self.rtpc_value)).then_some((id, f64::from(v)));
        self.rtpc_pawn = id;
        self.rtpc_value = v;
        set
    }
}

/// The tutorial as its widgets see it: the World between two steps, the match's HUD and the program's part.
struct Session<'a> {
    world: &'a mut World,
    hud: Option<&'a mut Hud>,
    hud_time: f64,
    paused: bool,
    st: &'a mut State,
}

impl Session<'_> {
    /// A page call on the World: the step machine's calls it raised kept for after it, its other events carried
    /// into the next tick's.
    // Unverified: the order (the calls applied once the call returns, ahead of the next tick's; the other events
    // ahead of the next tick's): both are spliced into the next tick's events; the build runs the actor's event
    // at the widget's call.
    fn page(&mut self, f: fn(&mut World)) {
        let from = self.world.events.len();
        f(self.world);
        take_page_events(self.world, from, self.st);
    }
}

/// The events a page call put in the World from `from` on, sorted into the state's lists.
fn take_page_events(world: &mut World, from: usize, st: &mut State) {
    for e in world.events.drain(from..) {
        match e {
            Event::TutorialUi { call } => st.pending.push_back(call),
            e => st.carried.push(e),
        }
    }
}

impl TutorialSession for Session<'_> {
    fn intro_complete(&mut self) {
        self.page(run::intro_complete);
    }

    fn start_pressed(&mut self) {
        self.page(run::start_pressed);
    }

    fn advance(&mut self) {
        self.page(run::advance);
    }

    fn start_next(&mut self) {
        self.page(run::start_next);
    }

    fn desaturate_environment(&mut self) {
        self.page(run::desaturate_environment);
    }

    fn actor_data(&self) -> Option<&TutorialActorData> {
        run::actor_data(self.world)
    }

    fn minigame_scores(&self) -> &[TutorialScore] {
        run::minigame_scores(self.world)
    }

    fn is_game_paused(&self) -> bool {
        self.paused
    }

    /// On again: the devices' queued presses dropped (PFGPlayerController ResetInputs
    /// ResetInputs).
    fn set_game_input(&mut self, on: bool) {
        set_game_input(self.st, on);
    }

    /// UI_TutorialEndScreen BackToMainMenu (@4858; View More Tutorials @4690 first sets the game instance's
    /// AdvTutShortcut, @4790): after the widget's Pre-Transition Cleanup and Delay 0.5, the UIManager's Transition to
    /// New_MainMenu (type 0, "Lobby", @2498-2674), run by the program after the frame. The match ends with no result.
    fn exit(&mut self, adv_tut_shortcut: bool) {
        self.st.exit = Some(adv_tut_shortcut);
    }

    /// UI_Functions PopText on the match's HUD, at the HUD's time + one step (MatchHost.pop's time).
    fn pop_text(&mut self, p: TutorialPopText) {
        if let Some(h) = self.hud.as_deref_mut() {
            h.tutorial_pop(&p, self.hud_time + STEP);
        }
    }
}

fn set_game_input(st: &mut State, on: bool) {
    if on && !st.game_input {
        st.reset_inputs = true;
    }
    st.game_input = on;
}

/// The match's parts a tutorial frame reads and writes besides the shell.
pub struct MatchParts<'a> {
    pub world: &'a mut World,
    pub hud: Option<&'a mut Hud>,
    pub hud_time: f64,
    /// The game is paused (SetGamePaused: the pause menu).
    pub paused: bool,
}

/// The tutorial widgets over a tutorial match: their menu screen, made before
/// the pause menu's and drawn under it (UI_Game's PauseMenu is added at ZOrder 1, the tutorial widgets at 0).
pub struct TutorialHost {
    pub screens: TutorialScreens,
    pub screen: MenuScreen,
    st: State,
    /// The menu input of the frame (the pause menu's frame leaves it when its own screen is not up).
    pub input: Vec<MenuInput>,
    /// The screen's last paint.
    pub list: ElementList,
}

impl std::fmt::Debug for TutorialHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TutorialHost").field("st", &self.st).finish_non_exhaustive()
    }
}

impl TutorialHost {
    /// The widgets' screen, empty and not active (the screens show and hide it).
    pub fn new(sh: &mut Shell) -> Result<TutorialHost> {
        let size = sh.swap.size;
        let Shell { ui, settings, audio, music, window, .. } = sh;
        let u = ui.as_mut().ok_or_else(|| Error::Match("a tutorial's widgets run with the UI".into()))?;
        let mut view = MenuView::new(u.menu.clone(), u.text.clone(), size);
        crate::menus::give_parts(&mut view, u);
        let mut sink =
            MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
        let mut run = MenuRun {
            fonts: &mut u.menu_fonts,
            prefs: settings,
            bindings: &mut u.bindings,
            sink: &mut sink,
            pointer: &mut u.pointer,
            lab: None,
            pause: None,
            tutorial: None,
        };
        let mut screen = MenuScreen::new(view, &mut run);
        screen.set_active(&mut run, false);
        Ok(TutorialHost {
            screens: TutorialScreens::new(),
            screen,
            st: State::new(),
            input: Vec::new(),
            list: ElementList::default(),
        })
    }

    /// The local controllers' game input (off: what the devices took is dropped).
    pub fn game_input(&self) -> bool {
        self.st.game_input
    }

    /// PFGPlayerController CanPause as the tutorial set it.
    pub fn pc_can_pause(&self) -> bool {
        self.st.pc_can_pause
    }

    /// UI_Game Can Pause's tutorial tests (@599-1210): no TutorialIntroScreen, no interactable TutorialWidget, no
    /// UI_TutorialEndScreen.
    pub fn can_pause(&self) -> bool {
        self.screens.can_pause(&self.screen.view)
    }

    /// The events a page call made outside the screen (the pause menu's Skip Step) put in the World from `from` on.
    pub fn take_page_events(&mut self, world: &mut World, from: usize) {
        take_page_events(world, from, &mut self.st);
    }

    /// The calls' events other than the widgets', for the next tick's (put in the World after its clear).
    pub fn take_carried(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.st.carried)
    }

    /// One tick's events (in the order the World made them): each step
    /// machine call to the widgets, the finish's save first.
    pub fn tick_events(&mut self, sh: &mut Shell, m: &mut MatchParts<'_>, events: &[Event]) {
        let calls: Vec<TutorialUiCall> = events
            .iter()
            .filter_map(|e| match e {
                Event::TutorialUi { call } => Some(call.clone()),
                _ => None,
            })
            .collect();
        if !calls.is_empty() {
            self.apply_calls(sh, m, calls.into());
        }
    }

    /// The step machine's calls in order, each with what its apply's page calls raise after it; the screen up or
    /// down by the widgets after.
    fn apply_calls(&mut self, sh: &mut Shell, m: &mut MatchParts<'_>, calls: VecDeque<TutorialUiCall>) {
        // (a page call's left since the last apply (the pause menu's Skip Step) came before these)
        let mut calls = {
            let mut all = std::mem::take(&mut self.st.pending);
            all.extend(calls);
            all
        };
        while let Some(call) = calls.pop_front() {
            // The actor class's FinishTutorial: the save (the match's data holds every actor class's).
            if let TutorialUiCall::Finished { actor } = &call {
                let actor = m.world.names.str(*actor).to_owned();
                finish_tutorial(&mut sh.session.stats, &m.world.data.tutorials, &actor);
            }
            // (Finished and Scoreboard go on to the screens, which do nothing with them: UI_HUD_Scoreboard_Container
            // is Collapsed by the HUD's context 5)
            self.with_cx(sh, m, |screens, cx| screens.apply(cx, &call));
            calls.append(&mut self.st.pending);
        }
        self.set_up(sh, m);
    }

    /// The screen active while a tutorial widget is in the viewport.
    fn set_up(&mut self, sh: &mut Shell, m: &mut MatchParts<'_>) {
        let up = self.screens.up(&self.screen.view);
        if up != self.screen.active() {
            let Self { screen, st, .. } = self;
            let Shell { ui, settings, audio, music, window, .. } = sh;
            let Some(u) = ui.as_mut() else { return };
            if up {
                // (the manager is ticked in an active screen's frames only: brought to now as the screen comes up,
                // before a widget's Delay or Add Timer reads it)
                let now = screen.view.now();
                screen.view.manager.sync_internal_time(now);
            }
            let mut sink =
                MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
            let mut session =
                Session { world: m.world, hud: m.hud.as_deref_mut(), hud_time: m.hud_time, paused: m.paused, st };
            let mut run = MenuRun {
                fonts: &mut u.menu_fonts,
                prefs: settings,
                bindings: &mut u.bindings,
                sink: &mut sink,
                pointer: &mut u.pointer,
                lab: None,
                pause: None,
                tutorial: Some(&mut session),
            };
            screen.set_active(&mut run, up);
        }
    }

    /// A call on the screens with the screen's context, the session given.
    fn with_cx<R>(
        &mut self,
        sh: &mut Shell,
        m: &mut MatchParts<'_>,
        f: impl FnOnce(&mut TutorialScreens, &mut mvs_ui::menu::Cx<'_>) -> R,
    ) -> Option<R> {
        let Self { screens, screen, st, .. } = self;
        let Shell { ui, settings, audio, music, window, .. } = sh;
        let u = ui.as_mut()?;
        let mut sink =
            MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
        let mut session =
            Session { world: m.world, hud: m.hud.as_deref_mut(), hud_time: m.hud_time, paused: m.paused, st };
        let mut run = MenuRun {
            fonts: &mut u.menu_fonts,
            prefs: settings,
            bindings: &mut u.bindings,
            sink: &mut sink,
            pointer: &mut u.pointer,
            lab: None,
            pause: None,
            tutorial: Some(&mut session),
        };
        Some(screen.with_cx(&mut run, |cx| f(screens, cx)))
    }

    /// One frame of the widgets' screen at the program's clock `now`, painted into `list`: the input the frame left
    /// it, the calls its widgets make (their step machine calls applied after the frame), the screen up or down.
    /// Returns the end screen's exit (`Some(adv_tut_shortcut)`) and whether the devices' queued presses are to go.
    pub fn frame(&mut self, sh: &mut Shell, m: &mut MatchParts<'_>, now: f64) -> FrameOut {
        let input = std::mem::take(&mut self.input);
        let exit = {
            let Self { screen, st, list, .. } = self;
            let Shell { ui, settings, audio, music, window, .. } = sh;
            match ui.as_mut() {
                Some(u) => {
                    let mut sink = MenuAudio {
                        audio,
                        tick: music.tick,
                        window: Some(crate::apply::window_info(window)),
                        equip: None,
                    };
                    let mut session = Session {
                        world: m.world,
                        hud: m.hud.as_deref_mut(),
                        hud_time: m.hud_time,
                        paused: m.paused,
                        st,
                    };
                    let mut run = MenuRun {
                        fonts: &mut u.menu_fonts,
                        prefs: settings,
                        bindings: &mut u.bindings,
                        sink: &mut sink,
                        pointer: &mut u.pointer,
                        lab: None,
                        pause: None,
                        tutorial: Some(&mut session),
                    };
                    list.clear();
                    screen.frame(&mut run, &input, now, list)
                }
                None => None,
            }
        };
        // (a transition of the widgets' own manager: none in the build's widgets; the end screen's goes through the
        // pause menu's)
        if let Some(MenuExit::OpenLevel { level, .. }) = &exit {
            eprintln!("mvs: tutorial widgets opened level {level}: not followed");
        }
        self.apply_calls(sh, m, VecDeque::new());
        FrameOut { exit: self.st.exit.take(), reset_inputs: std::mem::take(&mut self.st.reset_inputs) }
    }

    /// After each tick's events (afterTick): what follows the run's state. UncontrolTutorialPawn (@1914-2244), as it
    /// hands the pawn to the player: PC0's BPI_EnableGameInput (@1914) and BPI_SetCanPause(true) (@2056;
    /// PFGPlayerController CanPause is false on its CDO), then PawnControlledByTutorial false: read from the run's
    /// state after each tick, the first tick that finds the pawn the player's after the tutorial held it. Returns the
    /// RTPC tutorial_controlled_pawn to set, (pawn id, value), when the managed pawn or the value changed: TutorialActor
    /// ControlTutorialPawn @2096 / @2277 and UncontrolTutorialPawn @2255 / @2436 SetRTPCValue(tutorial_controlled_pawn,
    /// 1 / 0) on the spawned pawn (its actor's AkComponent and the component it returns).
    // Unverified: set at the first tick that finds the pawn or the value changed, not at the spawn itself.
    pub fn after_tick(&mut self, world: &World, me: usize) -> Option<(i32, f64)> {
        self.st.after_tick(world, me)
    }

    /// The devices' queued presses are to go (game input came back on outside a frame: after a tick).
    pub fn take_reset_inputs(&mut self) -> bool {
        std::mem::take(&mut self.st.reset_inputs)
    }
}

/// What a frame of the widgets' screen leaves for the loop.
#[derive(Debug, Default)]
pub struct FrameOut {
    /// The end screen's way back to the main menu, with AdvTutShortcut.
    pub exit: Option<bool>,
    /// Game input came back on: every device's queued presses dropped.
    pub reset_inputs: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use mvs_data::tutorial::{TutorialStep, TutorialText};
    use mvs_settings::stats::StatsTutorial;

    fn step(character: Option<&str>, dummy: Option<&str>) -> TutorialStep {
        TutorialStep {
            new_concept: false,
            header: TutorialText::default(),
            text: TutorialText::default(),
            start_button_disabled_time: 0.0,
            player: String::new(),
            character: character.map(str::to_owned),
            player_tag: String::new(),
            dummy: None,
            dummy_character: dummy.map(str::to_owned),
            spawn_target_dummy: false,
            dummy_controller: None,
            bag_percent: 0.0,
            bag_tag: String::new(),
            disable_cooldowns: false,
            demo: None,
            vo: None,
            tasks: Vec::new(),
        }
    }

    fn actor(name: &str, key: Option<&str>, steps: Vec<TutorialStep>) -> TutorialActorData {
        TutorialActorData {
            actor: name.to_owned(),
            name: TutorialText::default(),
            intro_text: TutorialText::default(),
            intro_vo: None,
            next: None,
            complete_key: key.map(str::to_owned),
            congratulations: Vec::new(),
            steps,
        }
    }

    #[test]
    fn the_cast_is_in_roster_order_with_the_demos_own() {
        let a = actor(
            INTRO_TUTORIAL,
            None,
            vec![step(Some("shaggy"), Some("taz")), step(Some("batman"), None), step(Some("not_a_fighter"), None)],
        );
        let cast = tutorial_cast(&a);
        let roster = &mvs_protocol::roster::CHARACTERS;
        let pos = |k: &str| roster.iter().position(|r| *r == k).unwrap();
        assert!(cast.contains(&"wonderwoman".to_owned()));
        let ported: Vec<&String> = cast.iter().filter(|k| roster.contains(&k.as_str())).collect();
        assert!(ported.windows(2).all(|w| pos(w[0]) < pos(w[1])));
        assert_eq!(cast.last().map(String::as_str), Some("not_a_fighter"));
    }

    #[test]
    fn no_data_no_match() {
        let settings = mvs_settings::Settings::default();
        assert!(tutorial_match(&Tutorials::new(), None, &settings, &[], 1).is_none());
        let mut t: Tutorials = IndexMap::new();
        t.insert(INTRO_TUTORIAL.to_owned(), actor(INTRO_TUTORIAL, None, vec![step(Some("shaggy"), None)]));
        let s = tutorial_match(&t, None, &settings, &[], 1).expect("the intro's match");
        assert!(matches!(s.kind, MatchKind::Tutorial));
        assert_eq!(s.stage, TUTORIAL_STAGE);
        assert_eq!(s.setup.players.len(), 1);
        assert!(s.setup.players[0].character.is_none() && !s.setup.players[0].bot);
        assert!(matches!(&s.setup.mode, Some(MatchMode::Tutorial { actor, .. }) if actor == INTRO_TUTORIAL));
        assert!(tutorial_match(&t, Some("TutorialActor_Walls"), &settings, &[], 1).is_none());
    }

    fn stats() -> (StatsFile, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("mvs-tutorial-save-{}-{}", std::process::id(), rand_tag()));
        std::fs::create_dir_all(&dir).unwrap();
        (StatsFile::open(&dir), dir)
    }

    fn rand_tag() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    }

    #[test]
    fn the_intro_sets_basic_and_wipes_the_advanced_map() {
        let (mut s, dir) = stats();
        let mut r = StatsTutorial { xp: 5.0, ..StatsTutorial::default() };
        r.tutorials_complete.insert("WallsTutorialComplete".into(), true);
        assert!(s.set_tutorial(r));
        finish_tutorial(&mut s, &Tutorials::new(), INTRO_TUTORIAL);
        let r = s.tutorial();
        assert!(r.basic_tutorial_complete);
        assert!(r.tutorials_complete.is_empty());
        assert_eq!(r.xp, 5.0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_advanced_one_adds_its_key_once() {
        let (mut s, dir) = stats();
        let mut t: Tutorials = IndexMap::new();
        t.insert("TutorialActor_Walls".into(), actor("TutorialActor_Walls", Some("WallsTutorialComplete"), Vec::new()));
        finish_tutorial(&mut s, &t, "TutorialActor_Walls");
        let r = s.tutorial();
        assert_eq!(r.tutorials_complete.get("WallsTutorialComplete"), Some(&true));
        assert!(!r.basic_tutorial_complete);
        // (an actor class with no key: nothing written)
        finish_tutorial(&mut s, &t, "TutorialActor_Batman_Fundementals");
        assert_eq!(s.tutorial(), r);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The calls through the session on the tut-intro match's World: the step machine's calls held for after
    /// the call in order, the rest carried to the next tick; afterTick hands the pawn back with the game input, the
    /// PlayerController's CanPause and the RTPC 1 then 0.
    #[test]
    fn page_calls_and_after_tick_on_the_intro() {
        use mvs_sim::input::NO_INPUT;
        use mvs_ui::menu::tutorial::TutorialSession;
        let Some(mut w) = mvs_sim_trace::harness::at("tut-intro", 0).map(|h| h.w) else { return };
        let mut st = State::new();
        let mut rtpc = Vec::new();
        let tick = |w: &mut World, st: &mut State, rtpc: &mut Vec<(i32, f64)>, n: usize| {
            for _ in 0..n {
                w.step(&[NO_INPUT]);
                rtpc.extend(st.after_tick(w, 0));
            }
        };
        tick(&mut w, &mut st, &mut rtpc, 30);
        let before = w.events.len();
        {
            let mut s = Session { world: &mut w, hud: None, hud_time: 0.0, paused: false, st: &mut st };
            s.intro_complete();
        }
        assert_eq!(w.events.len(), before, "the call's events left the World");
        assert!(matches!(st.pending.front(), Some(TutorialUiCall::Widget)), "the TutorialWidget first");
        assert!(st.pending.iter().any(|c| matches!(c, TutorialUiCall::Focus { on: false })));
        assert!(!st.carried.iter().any(|e| matches!(e, Event::TutorialUi { .. })));
        st.pending.clear();
        set_game_input(&mut st, false);
        tick(&mut w, &mut st, &mut rtpc, 600);
        assert!(st.held && !st.pc_can_pause, "the tutorial holds the pawn");
        {
            let mut s = Session { world: &mut w, hud: None, hud_time: 0.0, paused: false, st: &mut st };
            s.start_pressed();
        }
        tick(&mut w, &mut st, &mut rtpc, 60);
        assert!(st.pc_can_pause && st.game_input && st.reset_inputs, "handed back: CanPause, game input, reset");
        let values: Vec<f64> = rtpc.iter().map(|r| r.1).collect();
        assert!(values.contains(&1.0), "RTPC 1 while held: {values:?}");
        assert_eq!(values.last(), Some(&0.0), "RTPC 0 when handed back");
    }
}
