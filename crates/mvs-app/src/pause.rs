//! The match pause: the pause menu over an offline match that is not
//! the Lab, each local PlayerController's pause key and hold timer, UI_Game.MatchSetPause and Leave Match. The same
//! menu screen helpers run the Lab's menu over its match (match_lab.rs).
//!
//! PFGPlayerController is PC, UI_Game is UG, PandaGameInstance is PGI in the offsets below.

use mvs_sim::setup::{MatchSetup, TeamStyle, player_team};
use mvs_ui::bindings::Bindings;
use mvs_ui::menu::controller::MenuRun;
use mvs_ui::menu::input::MenuPad;
use mvs_ui::menu::local_play_match::{self as devices, InputSource};
use mvs_ui::menu::match_pause::MatchPauseSession;
use mvs_ui::menu::training_config::LabSession;
use mvs_ui::menu::{MenuExit, MenuInput, MenuScreen, MenuView};
use mvs_ui::slate::{Vis, WidgetId};

use crate::app::{MatchKind, Shell};
use crate::menu_audio::MenuAudio;
use crate::{Error, Result};

/// PFGPlayerController's CDO PauseHoldTimeMax.
pub const PAUSE_HOLD_TIME_MAX: f64 = 1.0;
/// Gamepad_Special_Right: the standard mapping's button 9 (Start / Menu / Options).
pub const SPECIAL_RIGHT: usize = 9;
/// Gamepad_Special_Left: the standard mapping's button 8 (Back / View / Share).
pub const SPECIAL_LEFT: usize = 8;

/// UI_Game's start-up chain (UG @3679-3897): UI_PauseMenu is made when the game mode is not Training, player 0 is not
/// a spectator and IsOnlinePlayer() is false. IsPlayingOnline (exe 1412219d0) is false for a standalone match alone:
/// Local Play and the port's other offline lobby matches. Online and every match a server holds have none. The Lab
/// (no lobby match) has UI_LAB_PauseMenu. (An arcade round and a tutorial are offline lobby matches.)
pub fn has_pause_menu(kind: &MatchKind) -> bool {
    matches!(kind, MatchKind::Custom { .. } | MatchKind::LocalPlay | MatchKind::Arcade | MatchKind::Tutorial)
}

/// The pause menu's Skip Step calls on the running tutorial, run by the loop on the World after the menu's frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TutorialSkip {
    /// BPI_SkipTutorialStep on every tutorial actor (the match runs one).
    Step,
    /// BPI_SkipToTutorialEndStep on every tutorial actor.
    ToEnd,
}

/// One local PlayerController: the local player's controller id, its pawn's player index and its own bHoldingPause.
#[derive(Clone, Debug)]
pub struct Ctrl {
    pub id: usize,
    pub slot: usize,
    pub holding: bool,
    /// The pad's Special Right at the last frame (its pressed / released edges).
    pub start_down: bool,
}

/// The match as its pause menu sees it and the pause's own state.
#[derive(Debug)]
pub struct MatchPause {
    pub ctrls: Vec<Ctrl>,
    /// SetGamePaused: the whole match sim, its camera and its fighters stand; the controllers' frame pass goes on.
    pub paused: bool,
    /// Delay(0.0) -> SetGamePaused(true) (UG @773 -> @828): at the next frame, before its ticks.
    // Unverified: where the latent action's next-frame call sits among that frame's ticks is native; taken as before
    // them.
    pending_game_pause: bool,
    /// UG @4785, Delay(0.0) -> false (@3917): the loop clears it at the next frame.
    pub unpaused_this_frame: bool,
    /// UI_Game MatchEnded: bMatchEnded.
    pub game_ended: bool,
    /// CanPause (PC, CDO false): BPI_SetCanPause(true) when the countdown card ends (UI_Countdown @5786).
    pub can_pause: bool,
    /// The shared PauseHoldTimer (PGI BPI_Get / SetPauseHoldTimer).
    pub hold_timer: f64,
    local_play: bool,
    team_style: i32,
    local_team: i32,
    /// The menu's `pause(on)` calls, taken by the loop (the widget runs when it calls).
    request: Option<bool>,
    reset_inputs: bool,
    /// Leave Match was asked: the transition to the main menu runs on the screen.
    pub leave: bool,
    is_pie: bool,
    /// Escape held at the last frame (its release edge).
    escape_held: bool,
    /// A tutorial match: BPI_GetGameModeType 4 (TutorialGameMode), else 1 (PandaGameMode).
    pub tutorial: bool,
    /// UI_Game Can Pause's tutorial tests (@599-1210, the loop's from the tutorial widgets each frame): false while
    /// the intro card, an interactable concept card or the end screen is up.
    pub tutorial_can_pause: bool,
    /// GameState BPI_IsTutorialStepSkippable (TutorialActors[0].BPI_IsSkippable), the loop's from the World each
    /// frame.
    pub tutorial_skippable: bool,
    /// The Skip Step calls the menu made, taken by the loop.
    tutorial_skips: Vec<TutorialSkip>,
}

impl MatchPause {
    /// The pause of a match of `setup` whose local player is `me`; `humans`: the (slot, binding set) of every Local
    /// Play human. The controller id is the human's binding set (its local player index): outside Local Play PC0 on
    /// set 0, whatever `me` is.
    pub fn new(setup: &MatchSetup, me: usize, humans: Option<&[(usize, usize)]>) -> MatchPause {
        // The local PlayerControllers, one per local player: in Local Play every player who is not a bot, with its
        // human's binding set; otherwise PC0 alone.
        // Unverified: their tick order is native; taken as the controller ids' order.
        let mut ctrls: Vec<Ctrl> = if setup.local_play {
            setup
                .players
                .iter()
                .enumerate()
                .filter(|(_, p)| !p.bot)
                .map(|(slot, _)| Ctrl {
                    id: humans.and_then(|h| h.iter().find(|(s, _)| *s == slot)).map_or(slot, |h| h.1),
                    slot,
                    holding: false,
                    start_down: false,
                })
                .collect()
        } else {
            vec![Ctrl { id: 0, slot: me, holding: false, start_down: false }]
        };
        ctrls.sort_by_key(|c| c.id);
        let local_team = ctrls.first().map_or(0, |c| i32::from(player_team(setup, c.slot)));
        MatchPause {
            ctrls,
            paused: false,
            pending_game_pause: false,
            unpaused_this_frame: false,
            game_ended: false,
            can_pause: false,
            hold_timer: 0.0,
            local_play: setup.local_play,
            team_style: match setup.team_style {
                TeamStyle::OneVsOne => 0,
                TeamStyle::TwoVsTwo => 1,
                TeamStyle::FreeForAll => 2,
            },
            local_team,
            request: None,
            reset_inputs: false,
            leave: false,
            is_pie: false,
            escape_held: false,
            tutorial: false,
            tutorial_can_pause: true,
            tutorial_skippable: false,
            tutorial_skips: Vec::new(),
        }
    }

    /// The Skip Step calls the menu made since the last call, in order.
    pub fn take_tutorial_skips(&mut self) -> Vec<TutorialSkip> {
        std::mem::take(&mut self.tutorial_skips)
    }

    /// AllowInstantPause? (PC @110-241): mode type Training 3 or Tutorial 4 pauses at the press.
    fn instant(&self) -> bool {
        self.tutorial
    }

    /// The pause the menu asked for since the last call.
    pub fn take_request(&mut self) -> Option<bool> {
        self.request.take()
    }

    /// ResetInputs asked since the last call (PC @0-1796): the loop drops the devices' queued presses.
    pub fn take_reset_inputs(&mut self) -> bool {
        std::mem::take(&mut self.reset_inputs)
    }

    /// The frame's start: the unpause's flag cleared, a pause the menu's Init asked for taken (`pending_game_pause`).
    pub fn begin_frame(&mut self) {
        if self.pending_game_pause {
            self.pending_game_pause = false;
            self.paused = true;
        }
        self.unpaused_this_frame = false;
    }

    /// The menu is shown: the game pauses at the next frame.
    pub fn game_pause_next_frame(&mut self) {
        self.pending_game_pause = true;
    }

    /// The pause key pressed (PC @291): AllowInstantPause? (@110-241: mode type Training 3 or Tutorial 4) pauses at
    /// once (@328 HoldToPauseComplete); a versus match (type 1) holds: bHoldingPause = true (@343). Returns whether
    /// HoldToPauseComplete is to run (it needs the menu's state: modals, transition).
    pub fn pause_pressed(&mut self, ctrl: usize, instant: bool) -> bool {
        if instant {
            return true;
        }
        self.ctrls[ctrl].holding = true;
        false
    }

    /// The Escape key's release (PC @4079 -> @355-414).
    pub fn pause_released(&mut self, ctrl: usize) {
        self.ctrls[ctrl].holding = false;
    }

    /// Each local controller's frame (they tick while paused, on the frame's delta): the pad's Special Right edge,
    /// the hold timer (@166212-166523: timer = max(0, timer + (bHoldingPause AND CanPause AND NOT IsPlayingOnline ?
    /// dt : dt / -5))) and the meter (@166715-166883: UIBroker.SetPauseHoldForPlayer(clamp(timer /
    /// PauseHoldTimeMax, 0, 1))). `start_down`: each controller's pad button now. Returns the (controller index, meter
    /// fraction) of each controller and the one whose hold completed (@167030-167227: the timer reset, HoldToPause
    /// Complete to run).
    pub fn controllers_frame(&mut self, dt: f64, start_down: &[bool]) -> (Vec<(usize, f64)>, Option<usize>) {
        let mut meters = Vec::new();
        let mut complete = None;
        for n in 0..self.ctrls.len() {
            // Gamepad_Special_Right of the controller's pad (PC @171375 pressed -> @291, @171431 released ->
            // bHoldingPause false).
            let down = start_down.get(n).copied().unwrap_or(false);
            let instant = self.instant();
            let c = &mut self.ctrls[n];
            let (was, id) = (c.start_down, c.id);
            c.start_down = down;
            if down && !was {
                // (the game mode's type: a versus match holds; a tutorial pauses at once, @328 HoldToPauseComplete)
                if instant {
                    complete = Some(n);
                } else {
                    c.holding = true;
                }
            } else if !down && was {
                c.holding = false;
            }
            let holding = self.ctrls[n].holding;
            self.hold_timer = (self.hold_timer + if holding && self.can_pause { dt } else { dt / -5.0 }).max(0.0);
            let frac = (self.hold_timer / PAUSE_HOLD_TIME_MAX).clamp(0.0, 1.0);
            meters.push((id, frac));
            if frac >= 1.0 {
                self.ctrls[n].holding = false;
                self.hold_timer = 0.0;
                complete = Some(n);
            }
        }
        (meters, complete)
    }
}

impl MatchPauseSession for MatchPause {
    /// PC0's BPI_Pause (PC @164407-164483): ResetInputs, then UI_Game.MatchSetPause(Pause, -1, false), which the loop
    /// runs when the menu's call returns.
    fn pause(&mut self, on: bool) {
        self.reset_inputs = true;
        self.request = Some(on);
    }

    fn reset_inputs(&mut self) {
        self.reset_inputs = true;
        if let Some(c) = self.ctrls.first_mut() {
            c.holding = false;
        }
    }

    fn is_game_paused(&self) -> bool {
        self.paused
    }

    fn is_local_play(&self) -> bool {
        self.local_play
    }

    /// GetPlayerController(i) valid and its pawn locally controlled: the local controllers, in their order.
    fn is_local_player(&self, i: i32) -> bool {
        usize::try_from(i).is_ok_and(|i| i < self.ctrls.len())
    }

    fn team_style(&self) -> i32 {
        self.team_style
    }

    fn local_team(&self) -> i32 {
        self.local_team
    }

    /// Leave Match steps 4-6 (PM @3735-4135): BPI_RecreateParty (no party offline), the transition to New_MainMenu
    /// "Lobby", BPI_PlayMusOST; the loop runs the transition on the screen.
    fn leave_match(&mut self) {
        self.leave = true;
    }

    /// BPI_GetGameModeType: TutorialGameMode 4, PandaGameMode 1 (every other match here).
    fn game_mode_type(&self) -> i32 {
        if self.tutorial { 4 } else { 1 }
    }

    /// GameState BPI_IsTutorialStepSkippable: TutorialActors[0].BPI_IsSkippable (`run::is_skippable`).
    fn is_tutorial_step_skippable(&self) -> bool {
        self.tutorial && self.tutorial_skippable
    }

    /// BPI_SkipTutorialStep on every tutorial actor (`run::skip_step`), run by the loop after the menu's frame.
    fn skip_tutorial_step(&mut self) {
        if self.tutorial {
            self.tutorial_skips.push(TutorialSkip::Step);
        }
    }

    /// BPI_SkipToTutorialEndStep on every tutorial actor (`run::skip_to_end`), run by the loop after the menu's
    /// frame.
    fn skip_to_tutorial_end_step(&mut self) {
        if self.tutorial {
            self.tutorial_skips.push(TutorialSkip::ToEnd);
        }
    }

    fn is_play_in_editor(&self) -> bool {
        self.is_pie
    }
}

// ---------- the menu screen over a match ----------

/// The frame's menu input: the window's, in the order it came, with the first pad's state (the menus' feeder, as
/// `MenuHost` reads it); nothing while the menu is not up (the window's queue is drained).
pub fn menu_input(sh: &mut Shell, active: bool) -> Vec<MenuInput> {
    let input = sh.window.take_menu_input();
    if !active {
        return releases(input);
    }
    with_pads(sh, input)
}

/// The key and mouse button releases of `input`: what a screen that takes no input still hears, so its record of the
/// keys down does not keep a key let go while it was not active (the Escape that unpaused, held into the next pause,
/// would read as a key the screen saw go down, and its repeats as Cancels).
pub fn releases(input: Vec<MenuInput>) -> Vec<MenuInput> {
    input
        .into_iter()
        .filter(|i| matches!(i, MenuInput::Key { down: false, .. } | MenuInput::MouseButton { down: false, .. }))
        .collect()
}

/// The window's menu input with each key's local player, and each pad's state for the local player that holds it
///: outside Local Play the first pad is player 0's ('kbm+pad0') and another
/// pad has none. A pad plugged in or out since the last poll is told to the devices table first.
pub fn with_pads(sh: &mut Shell, input: Vec<MenuInput>) -> Vec<MenuInput> {
    sh.devices.poll();
    let state = sh.devices.pads_mut().state;
    let first = state.iter().position(Option::is_some).map_or(-1, |i| i as i64);
    for (i, p) in state.iter().enumerate() {
        if p.is_some() != sh.pads_seen[i] {
            sh.pads_seen[i] = p.is_some();
            devices::pad_connection_changed(i, p.is_some(), first);
        }
    }
    let kbm = devices::local_player_of(InputSource::Kbm, first);
    let mut out: Vec<MenuInput> = input
        .into_iter()
        .filter_map(|i| match i {
            MenuInput::Key { key, down, repeat, .. } => kbm.map(|player| MenuInput::Key { key, down, repeat, player }),
            i => Some(i),
        })
        .collect();
    for (i, p) in state.iter().enumerate() {
        let Some(p) = p else { continue };
        let Some(player) = devices::local_player_of(InputSource::Pad(i), first) else { continue };
        // (the pad's Y is up positive, the menus' down)
        let state = MenuPad { axes: [p.lx, -p.ly, p.rx, -p.ry], buttons: u32::from(p.buttons) };
        out.push(MenuInput::Pad { pad: i, player, state });
    }
    out
}

/// A screen of the menu bundle's view with the widget `class` created collapsed in it (UI_Game's `Create`,
/// SetVisibility(Collapsed), AddToViewport(1)); the screen is not active.
pub fn make_screen(
    sh: &mut Shell,
    class: &str,
    bindings: &mut Bindings,
    lab: Option<&mut dyn LabSession>,
    pause: Option<&mut dyn MatchPauseSession>,
) -> Result<(MenuScreen, WidgetId)> {
    let size = sh.swap.size;
    let Shell { ui, settings, audio, music, window, .. } = sh;
    let u = ui.as_mut().ok_or_else(|| Error::Match("a match menu runs with the UI".into()))?;
    let mut view = MenuView::new(u.menu.clone(), u.text.clone(), size);
    crate::menus::give_parts(&mut view, u);
    let mut sink = MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
    let mut run = MenuRun {
        fonts: &mut u.menu_fonts,
        prefs: settings,
        bindings,
        sink: &mut sink,
        pointer: &mut u.pointer,
        lab: lab.map(|l| l as &mut dyn LabSession),
        pause: pause.map(|p| p as &mut dyn MatchPauseSession),
        tutorial: None,
    };
    let mut screen = MenuScreen::new(view, &mut run);
    let w = screen.with_cx(&mut run, |cx| {
        let w = MenuView::create(cx, class);
        MenuView::add_to_viewport(cx, w, 1);
        cx.view.tree_mut().widget_mut(w).vis = Vis::Collapsed;
        w
    });
    screen.set_active(&mut run, false);
    Ok((screen, w))
}

/// One frame of a match's menu screen at the program's clock `now`, painted into the shell's element list.
#[allow(clippy::too_many_arguments)]
pub fn menu_frame(
    sh: &mut Shell,
    screen: &mut MenuScreen,
    bindings: &mut Bindings,
    lab: Option<&mut dyn LabSession>,
    pause: Option<&mut dyn MatchPauseSession>,
    input: &[MenuInput],
    now: f64,
) -> Option<MenuExit> {
    let Shell { ui, settings, audio, music, window, .. } = sh;
    let u = ui.as_mut()?;
    let mut sink = MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
    let mut run = MenuRun {
        fonts: &mut u.menu_fonts,
        prefs: settings,
        bindings,
        sink: &mut sink,
        pointer: &mut u.pointer,
        lab: lab.map(|l| l as &mut dyn LabSession),
        pause: pause.map(|p| p as &mut dyn MatchPauseSession),
        tutorial: None,
    };
    // (the list holds the match's last frame, HUD included, until the match draws again: the menu paints on its own)
    u.list.clear();
    screen.frame(&mut run, input, now, &mut u.list)
}

/// The screen up or down (it takes the window's keys and the pointer while it is up).
pub fn set_screen_active(
    sh: &mut Shell,
    screen: &mut MenuScreen,
    bindings: &mut Bindings,
    lab: Option<&mut dyn LabSession>,
    on: bool,
) {
    let Shell { ui, settings, audio, music, window, .. } = sh;
    let Some(u) = ui.as_mut() else { return };
    let mut sink = MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
    let mut run = MenuRun {
        fonts: &mut u.menu_fonts,
        prefs: settings,
        bindings,
        sink: &mut sink,
        pointer: &mut u.pointer,
        lab: lab.map(|l| l as &mut dyn LabSession),
        pause: None,
        tutorial: None,
    };
    screen.set_active(&mut run, on);
}

/// A call on the menu widget `w` of `screen` (the widget's controller with its context).
#[allow(clippy::too_many_arguments)]
pub fn with_widget<R>(
    sh: &mut Shell,
    screen: &mut MenuScreen,
    bindings: &mut Bindings,
    lab: Option<&mut dyn LabSession>,
    pause: Option<&mut dyn MatchPauseSession>,
    f: impl FnOnce(&mut mvs_ui::menu::Cx<'_>) -> R,
) -> Option<R> {
    let Shell { ui, settings, audio, music, window, .. } = sh;
    let u = ui.as_mut()?;
    let mut sink = MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
    let mut run = MenuRun {
        fonts: &mut u.menu_fonts,
        prefs: settings,
        bindings,
        sink: &mut sink,
        pointer: &mut u.pointer,
        lab: lab.map(|l| l as &mut dyn LabSession),
        pause: pause.map(|p| p as &mut dyn MatchPauseSession),
        tutorial: None,
    };
    Some(screen.with_cx(&mut run, f))
}

/// The pause menu over a match that is not the Lab.
pub struct PauseHost {
    pub pause: MatchPause,
    pub screen: MenuScreen,
    pub bindings: Bindings,
    /// UI_PauseMenu.
    pub widget: WidgetId,
}

impl std::fmt::Debug for PauseHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PauseHost").field("paused", &self.pause.paused).finish_non_exhaustive()
    }
}

/// What the pause menu's frame leaves for the loop.
#[derive(Debug, Default)]
pub struct PauseOut {
    /// The menu asked to leave the match and its fade ended: the level is left (openLevelEvent).
    pub leave: bool,
    /// Each controller's (controller id, hold meter fraction), in their order: UIBroker.SetPauseHoldForPlayer, which
    /// the loop gives the HUD.
    pub meters: Vec<(usize, f64)>,
    /// The frame's menu input this screen did not take (its releases while it is up): a tutorial's widgets' screen's.
    pub rest: Vec<MenuInput>,
}

impl PauseHost {
    /// UI_Game's UI_PauseMenu on a screen of its own.
    pub fn new(
        sh: &mut Shell,
        setup: &MatchSetup,
        me: usize,
        humans: Option<&[(usize, usize)]>,
        bindings: Bindings,
    ) -> Result<PauseHost> {
        let mut pause = MatchPause::new(setup, me, humans);
        let mut bindings = bindings;
        let (screen, widget) = make_screen(sh, "UI_PauseMenu", &mut bindings, None, Some(&mut pause))?;
        Ok(PauseHost { pause, screen, bindings, widget })
    }

    /// UI_Game.MatchSetPause(pause, playerPausing, isControllerDisconnect) (UG @14953) and SetGamePaused.
    pub fn match_set_pause(&mut self, sh: &mut Shell, on: bool, player_pausing: i32, disconnect: bool) {
        // Can Pause (UG function @0-1471): bUnpausedThisFrame, then IsOnlinePlayer() (never here) OR bMatchEnded, then
        // the tutorial widgets (@599-1210: an intro screen, an interactable concept card, an end screen).
        if self.pause.unpaused_this_frame || self.pause.game_ended || !self.pause.tutorial_can_pause {
            return;
        }
        let w = self.widget;
        let Self { pause, screen, bindings, .. } = self;
        pause.request = None;
        with_widget(sh, screen, bindings, None, Some(&mut *pause), |cx| {
            cx.visit_dyn(w, |c, cx| {
                if let Some(m) = c.as_pause_menu() {
                    m.on_paused(cx, on, player_pausing, disconnect);
                }
            });
        });
        // Unverified (a bytecode reading): OnPaused's own Unpause (the key's toggle while the menu is up) runs a
        // nested MatchSetPause(false, -1), which overwrote the argument: the outer call goes on with the unpause
        // column.
        let on = pause.request.take().unwrap_or(on);
        // @4014-4105: UIManager.SetHandleInput(Pause).
        screen.view.manager.set_handle_input(on);
        if on {
            // @611 Visible; @695 AssignInputToWidget(UI_Game, -1) (the screen takes the window's keys while it is up).
            with_widget(sh, screen, bindings, None, Some(&mut *pause), |cx| {
                cx.view.tree_mut().widget_mut(w).vis = Vis::Visible;
                // (the manager is ticked in an active screen's frames only: brought to now before Init's Add Timer)
                let now = cx.view.now();
                cx.view.manager.sync_internal_time(now);
                cx.visit_dyn(w, |c, cx| {
                    if let Some(m) = c.as_pause_menu() {
                        m.init(cx);
                    }
                });
                // @564: the pause mix (the SFX buses muted, every voice but the music's and ambience's paused)
                cx.post("mix_pause_on");
            });
            Self::set_active(sh, screen, bindings, pause, true);
            pause.game_pause_next_frame(); // @773 Delay(0.0)
        } else {
            // @4851: Collapsed; @4889 SetGamePaused(false).
            with_widget(sh, screen, bindings, None, Some(&mut *pause), |cx| {
                cx.view.tree_mut().widget_mut(w).vis = Vis::Collapsed;
                // @4738
                cx.post("mix_pause_off");
            });
            pause.paused = false;
            // (the port's own) the presses made on the menu stay the menu's: InputDevice.resume
            sh.devices.resume_all();
            // @4785: Delay(0.0) -> false (@3917): the loop clears it at the next frame.
            pause.unpaused_this_frame = true;
            Self::set_active(sh, screen, bindings, pause, false);
        }
    }

    fn set_active(sh: &mut Shell, screen: &mut MenuScreen, bindings: &mut Bindings, pause: &mut MatchPause, on: bool) {
        let Shell { ui, settings, audio, music, window, .. } = sh;
        let Some(u) = ui.as_mut() else { return };
        let mut sink =
            MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
        let mut run = MenuRun {
            fonts: &mut u.menu_fonts,
            prefs: settings,
            bindings,
            sink: &mut sink,
            pointer: &mut u.pointer,
            lab: None,
            pause: Some(pause as &mut dyn MatchPauseSession),
            tutorial: None,
        };
        screen.set_active(&mut run, on);
    }

    /// HoldToPauseComplete (PC @162570-164374): the key's toggle.
    pub fn hold_to_pause_complete(&mut self, sh: &mut Shell, ctrl: usize, world_tick: u32) {
        // @162570-162775: GetTimeSeconds > StartTime + 3.0 AND CanPause. StartTime is never written: 0.0, so game time
        // past 3.0 s (the world's ticks; frozen while paused, past 3 s long before any pause).
        if !(world_tick > 3 * 60 && self.pause.can_pause) {
            return;
        }
        // @163081-163652: no modal, no transition, no dialog, and not within 0.25 s of LastModalTime.
        let m = &self.screen.view.manager;
        let since_modal = self.screen.view.now() - m.last_modal_time;
        if mvs_ui::menu::Manager::is_modal_active(&self.screen.view)
            || m.is_transitioning()
            || mvs_ui::menu::Manager::is_dialog_active(&mut self.screen.view)
            || (since_modal > 0.0 && since_modal < 0.25)
        {
            return;
        }
        // @163862 -> @78837: BPI_RemoveCurrentControllerRumble on every fighter (the loop's), then this controller's
        // ResetInputs (@148475).
        if let Some(c) = self.pause.ctrls.get_mut(ctrl) {
            c.holding = false;
        }
        self.pause.reset_inputs = true;
        // @163907-164297: PC0's UI_Game.MatchSetPause(NOT IsGamePaused, the own pawn's player index, false).
        let (on, slot) = (!self.pause.paused, i32::try_from(self.pause.ctrls[ctrl].slot).unwrap_or(-1));
        self.match_set_pause(sh, on, slot, false);
    }

    /// One frame of the pause: the controllers' pass, the key, the menu's frame painted when it shows. `keys`: the
    /// keys pressed this frame; `escape_held`: Escape held now; `start_down`: each controller's pad Start; `kb`: the
    /// controller whose device has the keyboard (none: Escape reaches no controller); `dt`: the
    /// frame's real time; `tutorial_up`: a tutorial's widgets' screen is up under this one (it takes the frame's
    /// input while this one is not; the port's own: the build's UIManager gives it the focus).
    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        sh: &mut Shell,
        keys: &[&'static str],
        escape_held: bool,
        start_down: &[bool],
        kb: Option<usize>,
        world_tick: u32,
        dt: f64,
        now: f64,
        tutorial_up: bool,
    ) -> PauseOut {
        self.pause.begin_frame();
        let (meters, complete) = self.pause.controllers_frame(dt, start_down);
        if let Some(n) = complete {
            self.hold_to_pause_complete(sh, n, world_tick);
        }
        // Escape (PC @6011 -> @232-342; both behind NOT GetIsMultikeyboardEnabled, off in the port). While the menu is
        // up the manager takes the press and the controller never sees it.
        let active = self.screen.active();
        let instant = self.pause.instant();
        if !active
            && keys.contains(&"Escape")
            && let Some(n) = kb
            && self.pause.pause_pressed(n, instant)
        {
            self.hold_to_pause_complete(sh, n, world_tick);
        }
        // Escape's release (the keyboard controller stops holding), a press and release in one frame included.
        if !escape_held
            && (self.pause.escape_held || keys.contains(&"Escape"))
            && let Some(n) = kb
        {
            self.pause.pause_released(n);
        }
        self.pause.escape_held = escape_held;
        // (a tutorial's widgets' screen under this one takes the frame's input while this one is not up: `rest`)
        // Deviation: the input is split by screen (this one while up, else the tutorial's, releases to both); the
        // build's UIManager gives the focus by widget.
        let all = menu_input(sh, active || tutorial_up);
        let (input, rest) = if active { (all.clone(), releases(all)) } else { (releases(all.clone()), all) };
        let Self { pause, screen, bindings, .. } = self;
        let exit = menu_frame(sh, screen, bindings, None, Some(&mut *pause), &input, now);
        // The menu's own calls (Resume, Leave Match) came back as requests.
        if let Some(on) = self.pause.take_request() {
            self.match_set_pause(sh, on, -1, false);
        }
        if self.pause.leave {
            self.pause.leave = false;
            let Self { pause, screen, bindings, .. } = self;
            // Transition UIManager {New_MainMenu, type 0, delay 0, force false, instant false, "Lobby"} (@3772-3994):
            // the fade to black on this screen, and at its end the level is left.
            with_widget(sh, screen, bindings, None, Some(&mut *pause), |cx| {
                mvs_ui::menu::Manager::transition(
                    cx,
                    mvs_ui::menu::manager::TransitionParams {
                        next_level: "New_MainMenu".to_owned(),
                        kind: 0,
                        delay: 0.0,
                        force: false,
                        instant: false,
                        level_options: "Lobby".to_owned(),
                    },
                );
            });
            // BPI_PlayMusOST (@4135)
            sh.music.play_mus_ost(&mut sh.audio);
        }
        // ResetInputs (PC @0-1796, asked by the pause key or the menu's own pause calls): every device's queued
        // presses and releases dropped
        if self.pause.take_reset_inputs() {
            sh.devices.drop_queued();
        }
        PauseOut { leave: matches!(exit, Some(MenuExit::OpenLevel { .. })), meters, rest }
    }

    /// A tutorial's end screen's BackToMainMenu (UI_TutorialEndScreen @2498-2674): Transition UIManager
    /// {New_MainMenu, type 0, delay 0, force false, instant false, "Lobby"}, drawn as this screen's fader alone; the
    /// level is left at its end (`PauseOut::leave`).
    pub fn to_main_menu(&mut self, sh: &mut Shell) {
        let Self { pause, screen, bindings, .. } = self;
        with_widget(sh, screen, bindings, None, Some(&mut *pause), |cx| {
            mvs_ui::menu::Manager::transition(
                cx,
                mvs_ui::menu::manager::TransitionParams {
                    next_level: "New_MainMenu".to_owned(),
                    kind: 0,
                    delay: 0.0,
                    force: false,
                    instant: false,
                    level_options: "Lobby".to_owned(),
                },
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_sim::setup::MatchPlayer;

    fn setup(local_play: bool) -> MatchSetup {
        let p = |bot| MatchPlayer { bot, ..MatchPlayer::default() };
        MatchSetup { players: vec![p(false), p(true), p(false)], local_play, ..MatchSetup::default() }
    }

    #[test]
    fn local_play_has_a_controller_per_human() {
        let p = MatchPause::new(&setup(true), 0, Some(&[(2, 1), (0, 0)]));
        assert_eq!(p.ctrls.iter().map(|c| (c.id, c.slot)).collect::<Vec<_>>(), [(0, 0), (1, 2)]);
        assert!(p.is_local_player(1) && !p.is_local_player(2));
        let q = MatchPause::new(&setup(false), 0, None);
        assert_eq!(q.ctrls.len(), 1);
    }

    #[test]
    fn outside_local_play_pc0_is_controller_0_on_the_local_slot() {
        // (a custom lobby seat: the default humans for `me`, set 0; the HUD's meter keeps controller 0's)
        let p = MatchPause::new(&setup(false), 2, None);
        assert_eq!(p.ctrls.iter().map(|c| (c.id, c.slot)).collect::<Vec<_>>(), [(0, 2)]);
    }

    #[test]
    fn the_hold_fills_in_a_second_and_pauses() {
        let mut p = MatchPause::new(&setup(false), 0, None);
        p.can_pause = true;
        let (_, done) = p.controllers_frame(1.0 / 60.0, &[true]);
        assert_eq!(done, None);
        let mut frames = 1;
        let done = loop {
            let (m, d) = p.controllers_frame(1.0 / 60.0, &[true]);
            frames += 1;
            if d.is_some() {
                assert_eq!(m[0].1, 1.0);
                break d;
            }
            assert!(frames < 100);
        };
        assert_eq!(done, Some(0));
        assert_eq!(p.hold_timer, 0.0);
        // (60 steps of 1/60 s: the sum reaches 1.0 on the 60th or 61st)
        assert!((60..=61).contains(&frames));
    }

    #[test]
    fn a_released_hold_drains_five_times_slower() {
        let mut p = MatchPause::new(&setup(false), 0, None);
        p.can_pause = true;
        for _ in 0..30 {
            p.controllers_frame(1.0 / 60.0, &[true]);
        }
        let held = p.hold_timer;
        p.controllers_frame(1.0 / 60.0, &[false]);
        assert!((held - p.hold_timer - 1.0 / 300.0).abs() < 1e-9);
    }

    #[test]
    fn a_screen_not_active_hears_the_releases_alone() {
        let key = |down, repeat| MenuInput::Key { key: "Escape", down, repeat, player: 0 };
        let input = vec![
            key(true, true),
            key(false, false),
            MenuInput::MouseMove { x: 1.0, y: 2.0 },
            MenuInput::MouseButton { button: 0, down: true },
            MenuInput::MouseButton { button: 0, down: false },
        ];
        assert_eq!(releases(input), [key(false, false), MenuInput::MouseButton { button: 0, down: false }]);
    }

    #[test]
    fn a_tutorial_pauses_at_the_press_and_takes_skip_calls() {
        let mut p = MatchPause::new(&setup(false), 0, None);
        assert_eq!(p.game_mode_type(), 1);
        p.skip_tutorial_step();
        assert!(p.take_tutorial_skips().is_empty());
        p.tutorial = true;
        p.can_pause = true;
        assert_eq!(p.game_mode_type(), 4);
        // (AllowInstantPause?: the press itself completes, no hold)
        let (_, done) = p.controllers_frame(1.0 / 60.0, &[true]);
        assert_eq!(done, Some(0));
        assert!(!p.ctrls[0].holding);
        assert!(p.pause_pressed(0, p.instant()));
        p.tutorial_skippable = true;
        assert!(p.is_tutorial_step_skippable());
        p.skip_tutorial_step();
        p.skip_to_tutorial_end_step();
        assert_eq!(p.take_tutorial_skips(), [TutorialSkip::Step, TutorialSkip::ToEnd]);
        assert!(p.take_tutorial_skips().is_empty());
    }

    #[test]
    fn the_menus_pause_calls_are_requests() {
        let mut p = MatchPause::new(&setup(false), 0, None);
        p.pause(false);
        assert!(p.take_reset_inputs());
        assert_eq!(p.take_request(), Some(false));
        assert_eq!(p.take_request(), None);
        p.leave_match();
        assert!(p.leave);
    }
}
