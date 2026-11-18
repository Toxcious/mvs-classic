//! The Lab's menu over its match: the screen with UI_LAB_PauseMenu, the
//! pause key and the hold, the Move List's key, and the pause and spawn logic of `match_lab` stepped for the loop.
//!
//! UI_Game creates UI_LAB_PauseMenu collapsed when the Lab starts; Escape or the gamepad's Special Right shows it and
//! pauses the game (PFGPlayerController HoldToPauseComplete -> UI_Game.MatchSetPause), Resume Training and Cancel
//! unpause through BPI_Pause(false).
//!
//! The Lab while queued (Wait in the Lab): the hub's queue runs beside it; the host shows its
//! matchmaking timer, runs TrainGameMode's match-found chain and Back To Matchmaking, and leaves the level for the
//! queue's pre-match screen at their end (`OnlineHub::lab_return` on the return path).

use indexmap::IndexMap;
use mvs_sim::{MatchSetup, World};
use mvs_ui::bindings::Bindings;
use mvs_ui::bundle::FighterInfo;
use mvs_ui::menu::input::MenuPad;
use mvs_ui::menu::manager::TransitionParams;
use mvs_ui::menu::screens::lab::LabPauseMenu;
use mvs_ui::menu::screens::lab_hotkeys::add_lab_hotkeys;
use mvs_ui::menu::screens::lab_timer::{TimerSource, add_lab_matchmaking_timer};
use mvs_ui::menu::screens::move_list::toggle_movelist;
use mvs_ui::menu::training_config::{LabConfig, LabSession};
use mvs_ui::menu::{Manager, MenuExit, MenuInput, MenuRun, MenuScreen};
use mvs_ui::slate::Vis;
use mvs_view::MatchCamera;

use crate::Result;
use crate::app::Shell;
use crate::lab_setup::{
    BotPool, LabCarry, LabInputs, LabMusic, LabRoll, fighters_of, first_config, lab_roster_skins, lab_slots,
    random_source,
};
use crate::lab_tools::{FrameIn, LabTools, ToolsCx};
use crate::match_lab::{LabControl, LabEffect, LabMenu, LabState, can_pause_lab};
use crate::menu_audio::MenuAudio;
use crate::online::queue::LabLeave;
use crate::pause::{MatchPause, make_screen, menu_frame, with_widget};
use crate::script::Fed;

/// A new Lab's menu built ahead of its match, on the loading screen's frames while the loader makes the World (the
/// UI's types are not Send): the pause menu's screen, the Lab Tools and the key bindings they were made with. The
/// match takes it when its first config is the one it was built for, else builds the menu anew.
#[derive(Debug)]
// Deviation: UI_Game creates the Lab menu when the level starts; the port builds it ahead on the loading screen.
pub struct LabAhead {
    menu: LabMenu,
    config: LabConfig,
    bindings: Bindings,
}

impl LabAhead {
    /// The menu a new Lab of `setup` owned by `name` starts with (`LabHost::new`'s, built now). `same_attack` is the
    /// World's SameAttackPenalty the match will start with.
    pub fn build(sh: &mut Shell, setup: &MatchSetup, name: &str, same_attack: bool) -> Result<LabAhead> {
        let store = sh.store().ok_or_else(|| crate::Error::Match("no store is open".into()))?;
        // (LoadInputBindings as the match's: the World's InputDefaults are this entry of the same store)
        let defaults: mvs_data::InputDefaults = mvs_data::entry(&*store.kind(mvs_data::KIND)?, mvs_data::path::INPUT)?;
        let mut bindings = Bindings::load(&defaults, sh.settings.input.bindings.as_ref());
        let fighters = fighters_of(sh);
        let (menu, config) = new_menu(sh, &fighters, setup, name, same_attack, &mut bindings)?;
        Ok(LabAhead { menu, config, bindings })
    }
}

/// A new Lab's menu: the Lab Tools' settings set back, the first config, the screen with UI_LAB_PauseMenu and the
/// Lab Tools' panel.
fn new_menu(
    sh: &mut Shell,
    fighters: &IndexMap<String, FighterInfo>,
    setup: &MatchSetup,
    name: &str,
    same_attack: bool,
    bindings: &mut Bindings,
) -> Result<(LabMenu, LabConfig)> {
    // (a Lab entered from the menus: the Lab Tools' switch and page start at the defaults)
    sh.settings.lab = mvs_settings::LabSettings::default();
    let config = first_config(fighters, &sh.settings, setup, name, same_attack);
    let mut session = LabState::new(config.clone(), None);
    let (screen, widget) = make_screen(sh, "UI_LAB_PauseMenu", bindings, Some(&mut session), None)?;
    // (the Lab Tools' panel over the match: closed at a new Lab's start)
    let tools = LabTools::new(sh, bindings);
    Ok((LabMenu { session, screen, pause_menu: widget, tools, queue_timer: None, clock: 0.0 }, config))
}

/// The Move List's modal id (UI_CharacterMoveList's training_movelist).
const MOVELIST: &str = "training_movelist";

/// ShowMapTransitionUI's label (TrainingGameState @5553 EX_TextConst "Loading New Level").
const TEXT_LOADING_NEW_LEVEL: &str = "ECE2133846B8C7D4F8652F9A20A57A9D";

/// (the port's own) The map load's cover fades in and out over this long, s; the build's shows and goes at once.
pub const COVER_FADE: f64 = 0.25;

/// What a restart brings the new match from the one before it.
#[derive(Debug, Default)]
pub struct Taken {
    /// The camera rig and the music flow go on unless the map was changed (the new map begins them anew).
    pub camera: Option<MatchCamera>,
    pub music: Option<LabMusic>,
    pub bot_idle: bool,
}

/// What one frame of the Lab's menu leaves for the loop.
#[derive(Debug, Default)]
pub struct LabOut {
    pub effects: Vec<LabEffect>,
    /// UI_LAB_Panel's quit: the transition's end opened the main menu.
    pub leave: bool,
    /// The config changed (the World's same-attack penalty and instant cooldowns follow it).
    pub config_changed: bool,
}

/// The Lab's menu, session and control, on the match.
pub struct LabHost {
    pub menu: LabMenu,
    pub control: LabControl,
    /// The controller's hold timer (the gamepad's Start).
    pub pause: MatchPause,
    pub bindings: Bindings,
    fighters: IndexMap<String, FighterInfo>,
    pool: BotPool,
    rng: Box<dyn FnMut() -> f64>,
    /// A restart's match: no 3 s wait before the pause key works.
    restarted: bool,
    /// LabMapLoadTracker.MapLoadActive: a map load's waiting screen is up (from the unpause that asked for the map to
    /// the cover's dismissal in the next match).
    pub map_load_active: bool,
    /// The map load's Delay 2.0 before ShowMapTransitionUI(false), in the world's ticks left (0: none running).
    reveal_left: u32,
    /// The menu's clock when the cover is fully in (its own fade's end): the old match ends no sooner.
    cover_in: f64,
    /// The menu's clock when the cover's fade out ends: the screen goes down then.
    cover_out: Option<f64>,
    /// The Move List was shown over the running match (the screen is up for it).
    list_up: bool,
    /// The first pad's Start at the last frame (the Lab's pause is on its press).
    start_prev: bool,
    /// The first pad's Back (Gamepad_Special_Left, UIReset) at the last frame.
    back_prev: bool,
    /// A scripted run's pad state, from its last pad event on.
    script_pad: Option<MenuPad>,
    /// The level's first frame has been through the queue's step (the timer is added at the Lab's open).
    queue_opened: bool,
    /// UI_LAB_Hotkeys is still to be added (a new Lab's first frame; a restart keeps the menu's).
    hotkeys_due: bool,
    /// The menu's clock at this match's start (`LabMenu::clock`): the match's own clock starts at 0.
    clock_base: f64,
    /// Each fighter's PlayerIndex: its config slot (a restart's `lab_slots`; the first match's fighters in order).
    pub slot_of: Vec<usize>,
}

impl std::fmt::Debug for LabHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LabHost").field("paused", &self.control.paused).finish_non_exhaustive()
    }
}

impl LabHost {
    /// The Lab's menu for a match of `setup` whose owner is `name`; `carry`: the Lab before a restart, its menu kept;
    /// `ahead`: a new Lab's menu built on the loading screen.
    pub fn new(
        sh: &mut Shell,
        setup: &MatchSetup,
        name: &str,
        carry: Option<LabCarry>,
        ahead: Option<Box<LabAhead>>,
        bindings: Bindings,
        same_attack: bool,
    ) -> Result<(LabHost, Taken)> {
        let fighters = fighters_of(sh);
        let pool = BotPool::default();
        let mut rng: Box<dyn FnMut() -> f64> = Box::new(random_source());
        let mut bindings = bindings;
        let slot_of = carry.as_ref().map_or_else(|| (0..setup.players.len()).collect(), |c| lab_slots(&c.config));
        let restarted_menu = carry.is_some();
        let (menu, config, rolled, taken, map_load) = match carry {
            Some(c) => {
                let LabCarry { config, toggles, camera, music, bot_idle, rolled, mut menu, map_load, .. } = c;
                // After a restart the session and the menu are the ones before it (the menu's widgets hold the
                // session): its config is this level's.
                menu.session.config = config.clone();
                menu.session.toggles = toggles;
                if let Some(t) = &mut menu.tools {
                    t.restarted();
                }
                let taken =
                    Taken { camera: (!map_load).then_some(camera), music: (!map_load).then_some(music), bot_idle };
                (menu, config, rolled, taken, map_load)
            }
            None => {
                let first = first_config(&fighters, &sh.settings, setup, name, same_attack);
                let (menu, config) = match ahead.filter(|a| a.config == first) {
                    Some(a) => {
                        bindings = a.bindings;
                        (a.menu, a.config)
                    }
                    None => new_menu(sh, &fighters, setup, name, same_attack, &mut bindings)?,
                };
                (menu, config, LabRoll::default(), Taken::default(), false)
            }
        };
        let restarted = taken.camera.is_some() || map_load;
        let clock_base = menu.clock;
        let roster = {
            let i = LabInputs {
                settings: &sh.settings,
                fighters: &fighters,
                pool: &pool,
                random: &mut *rng,
                first: [None, None],
            };
            lab_roster_skins(&i, &config, &rolled)
        };
        let mut pause = MatchPause::new(setup, 0, None);
        pause.can_pause = true;
        let mut host = LabHost {
            menu,
            control: LabControl::new(roster, rolled),
            pause,
            bindings,
            fighters,
            pool,
            rng,
            restarted,
            map_load_active: map_load,
            reveal_left: 0,
            cover_in: 0.0,
            cover_out: None,
            list_up: false,
            start_prev: false,
            back_prev: false,
            script_pad: None,
            queue_opened: false,
            hotkeys_due: !restarted_menu,
            clock_base,
            slot_of,
        };
        // (a map load's cover, raised in the match before, stays up over this one's start)
        if map_load {
            host.set_active(sh, true);
        }
        Ok((host, taken))
    }

    /// The HUD's combo texts: the session's toggles and the fighters' PlayerIndex.
    pub fn combos(&self) -> mvs_ui::hud::LabCombos {
        let t = &self.menu.session.toggles;
        mvs_ui::hud::LabCombos { juggle: t.juggle_combo, hitstun: t.hitstun_combo, slot_of: self.slot_of.clone() }
    }

    /// The Lab's config as the menu holds it.
    pub fn config(&self) -> &LabConfig {
        &self.menu.session.config
    }

    /// The screen is up (the pause menu or the Move List).
    pub fn active(&self) -> bool {
        self.menu.screen.active()
    }

    fn set_active(&mut self, sh: &mut Shell, on: bool) {
        let Self { menu, bindings, .. } = self;
        crate::pause::set_screen_active(sh, &mut menu.screen, bindings, Some(&mut menu.session), on);
    }

    /// UI_Game.MatchSetPause(on) as the Lab runs it: the control's step, and the menu shown or hidden.
    fn match_set_pause(&mut self, sh: &mut Shell, on: bool, out: &mut Vec<LabEffect>) {
        let effects = {
            let mut i = LabInputs {
                settings: &sh.settings,
                fighters: &self.fighters,
                pool: &self.pool,
                random: &mut *self.rng,
                first: [None, None],
            };
            self.control.set_pause(&self.menu.session, on, &mut i)
        };
        for e in effects {
            if let LabEffect::Paused(on) = e {
                self.show(sh, on);
                self.pause.paused = on;
                // the presses stay the menu's or the match's
                sh.devices.resume_all();
            }
            if let LabEffect::MapLoad(_) = e {
                self.cover(sh);
            }
            out.push(e);
        }
    }

    /// OnMapLoadRequested (TrainingContainer @3435) from the unpause: LabMapLoadTracker.MapLoadActive, and when the
    /// UIManager is not waiting ShowMapTransitionUI(true) (TrainingGameState @5553): UI_Waiting "Loading New Level",
    /// opaque, its label fading in over 0.5 s; the screen is up for it. Destroy All Players and the load are the
    /// loop's (`LabEffect::MapLoad`).
    fn cover(&mut self, sh: &mut Shell) {
        self.map_load_active = true;
        self.cover_out = None;
        self.cover_in = self.menu.clock + COVER_FADE;
        let Self { menu, bindings, .. } = self;
        with_widget(sh, &mut menu.screen, bindings, Some(&mut menu.session), None, |cx| {
            if cx.view.manager.is_waiting {
                return;
            }
            let label = cx.view.game_text(TEXT_LOADING_NEW_LEVEL).to_owned();
            Manager::set_waiting_faded(cx, true, &label, true, 0.0, COVER_FADE);
        });
        self.set_active(sh, true);
    }

    /// The map load's cover is fully in (its own fade ended): what is under it is not seen.
    pub fn covered(&self) -> bool {
        self.map_load_active && self.menu.clock >= self.cover_in
    }

    /// ShowMapTransitionUI(false), bShouldDismissWaiting and MapLoadActive false: the cover goes (over its own
    /// fade, the screen up until its end).
    fn dismiss_cover(&mut self, sh: &mut Shell) {
        self.map_load_active = false;
        self.reveal_left = 0;
        self.cover_out = Some(self.menu.clock + COVER_FADE);
        let Self { menu, bindings, .. } = self;
        with_widget(sh, &mut menu.screen, bindings, Some(&mut menu.session), None, |cx| {
            Manager::set_waiting_faded(cx, false, "", false, 0.0, COVER_FADE);
        });
    }

    /// The menu shown (its panels asked for the config, the pause sound mix on) or hidden.
    fn show(&mut self, sh: &mut Shell, on: bool) {
        let w = self.menu.pause_menu;
        let Self { menu, bindings, .. } = self;
        with_widget(sh, &mut menu.screen, bindings, Some(&mut menu.session), None, |cx| {
            cx.view.tree_mut().widget_mut(w).vis = if on { Vis::Visible } else { Vis::Collapsed };
            if on {
                // (the manager is ticked in an active screen's frames only: brought to now before the menu's timers)
                let now = cx.view.now();
                cx.view.manager.sync_internal_time(now);
            }
            cx.visit::<LabPauseMenu, _>(w, |m, cx| if on { m.shown(cx) } else { m.hidden(cx) });
            cx.post(if on { "mix_pause_on" } else { "mix_pause_off" });
        });
        self.list_up = false;
        self.set_active(sh, on);
    }

    /// HoldToPauseComplete (PC @162570): the key's pause, behind the gates of `can_pause_lab`.
    fn hold_to_pause_complete(&mut self, sh: &mut Shell, world_tick: u32, now: f64, out: &mut Vec<LabEffect>) {
        let m = &self.menu.screen.view.manager;
        let modal = Manager::is_modal_active(&self.menu.screen.view);
        let since = self.menu.screen.view.now() - m.last_modal_time;
        let since = if m.last_modal_time == 0.0 { f64::INFINITY } else { since };
        let _ = now;
        // PFGPlayerController CanPause: cleared by the match-found chain and Back To Matchmaking
        // (BPI_SetCanPause(false), TGM@2216 / @3885)
        if sh.online.as_ref().is_some_and(|h| h.borrow().lab_queue().is_some_and(|q| q.lab_leave.is_some())) {
            return;
        }
        if !can_pause_lab(world_tick, self.restarted, self.control.paused, self.map_load_active, modal, since) {
            return;
        }
        self.match_set_pause(sh, true, out);
    }

    /// One frame: the controller's hold and the keys, the menu's frame painted into the shell's element list, the
    /// menu's own calls (Resume, Cancel) answered.
    /// The frame's input: the window's and the first pad's, or a scripted run's events (the window's dropped); and
    /// whether Escape and Tab went down.
    /// The frame's input, and whether Escape, Tab and Home were pressed.
    fn frame_input(&mut self, sh: &mut Shell) -> (FrameIn, bool, bool, bool) {
        let window = sh.window.take_menu_input();
        let Some(s) = &mut sh.script else {
            sh.devices.poll();
            let pads = sh.devices.pads_mut();
            // (the pad's Y is up positive, the menus' down)
            let pad = pads
                .first()
                .and_then(|k| pads.state[k])
                .map(|p| MenuPad { axes: [p.lx, -p.ly, p.rx, -p.ry], buttons: u32::from(p.buttons) });
            let (escape, tab, home) =
                (sh.window.pressed("Escape"), sh.window.pressed("Tab"), sh.window.pressed("Home"));
            return (FrameIn { menu: window, pad }, escape, tab, home);
        };
        let (mut escape, mut tab, mut home) = (false, false, false);
        let mut menu = Vec::new();
        for f in s.drive(true) {
            match f {
                Fed::Input(ev) => {
                    if let MenuInput::Key { key, down: true, repeat: false, .. } = ev {
                        escape |= key == "Escape";
                        tab |= key == "Tab";
                        home |= key == "Home";
                    }
                    menu.push(ev);
                }
                Fed::Pad(p) => self.script_pad = Some(MenuPad { axes: p.axes, buttons: p.buttons }),
            }
        }
        (FrameIn { menu, pad: self.script_pad }, escape, tab, home)
    }

    pub fn frame(&mut self, sh: &mut Shell, world: &mut World, world_tick: u32, now: f64) -> LabOut {
        // (the kept menu's clock, never set back by a restart: `now` is the match's)
        // Deviation: the clock does not run through the next match's load, where a wall clock does
        // (the queue timer kept over a restart lags by the load's time).
        let now = self.clock_base + now;
        self.menu.clock = now;
        // (the map load's cover has faded out: its screen goes down unless the pause or the Move List is up)
        if self.cover_out.is_some_and(|t| now >= t) {
            self.cover_out = None;
            if !self.control.paused && !self.list_up {
                self.set_active(sh, false);
            }
        }
        let mut out = LabOut::default();
        self.control.frame();
        self.pause.begin_frame();
        let (input, escape, tab, home) = self.frame_input(sh);
        let was_active = self.active();
        // The Lab's pause is instant (AllowInstantPause: mode type Training, PC @110-241): on the press of the first
        // pad's Special Right.
        let start = input.pad.is_some_and(|p| p.buttons & (1 << crate::pause::SPECIAL_RIGHT) != 0);
        let pressed = start && !self.start_prev;
        self.start_prev = start;
        let back = input.pad.is_some_and(|p| p.buttons & (1 << crate::pause::SPECIAL_LEFT) != 0);
        let back_pressed = back && !self.back_prev;
        self.back_prev = back;
        let mut effects = Vec::new();
        // UI_Game @1631-1636 pushes the timer, @2463 (pop_flow @1839) the hotkeys: the hotkeys are created first and
        // the timer added after, over them
        if std::mem::take(&mut self.hotkeys_due) {
            self.with_run(sh, |screen, run| {
                add_lab_hotkeys(screen, run);
            });
        }
        self.queue_frame(sh, now, &mut effects);
        if pressed || (escape && !self.active()) {
            self.hold_to_pause_complete(sh, world_tick, now, &mut effects);
        }
        // The Move List over the running Lab (Tab, PFGPlayerController InpActEvt_Tab @132493): the Lab's menu view is
        // shown for its modal, with the pause menu collapsed, until the modal goes.
        if tab && !self.control.paused && !self.map_load_active && !self.active() {
            self.toggle_move_list(sh);
        }
        // (an inactive screen takes none of the frame's input, nor one that the frame's key or button brought up:
        // the Escape that paused is not the pause menu's Back, the Tab not the Move List's)
        let lab_input = if was_active && self.active() {
            let mut i = input.menu.clone();
            i.extend(input.pad.map(|state| MenuInput::Pad { pad: 0, player: 0, state }));
            i
        } else {
            // (the releases still: pause::releases)
            crate::pause::releases(input.menu.clone())
        };
        let exit = {
            let Self { menu, bindings, .. } = self;
            menu_frame(sh, &mut menu.screen, bindings, Some(&mut menu.session), None, &lab_input, now)
        };
        self.menu.session.end_response();
        // The Lab Tools' panel, over the match while the Lab menu is not up.
        let visible = sh.settings.lab.tools_on && !self.control.paused && !self.active();
        let Self { menu, bindings, .. } = self;
        let LabMenu { session, tools, .. } = menu;
        if let Some(t) = tools {
            let mut cx = ToolsCx { bindings, session, world: &mut *world };
            if t.frame(sh, &mut cx, &input, visible, now).reset {
                // (labReset: Spawn All Players on the fighters' starting damage as it is)
                let starting_damage =
                    world.players.iter().map(|&f| f64::from(world.fighter(f).stats.starting_damage)).collect();
                effects.push(LabEffect::Reset { starting_damage });
            }
        }
        // The hotkey strip's Reset (UIReset: Home, the pad's Back) over the running Lab: labReset, as Reset Lab.
        // Unverified: the build's UI_LAB_Hotkeys broadcasts ResetEvent, which no decoded graph binds; the reset the
        // label names is the Home key, not a decoded binding.
        if (home || back_pressed) && !self.control.paused && !self.map_load_active && !self.active() && !was_active {
            let starting_damage =
                world.players.iter().map(|&f| f64::from(world.fighter(f).stats.starting_damage)).collect();
            effects.push(LabEffect::Reset { starting_damage });
        }
        // The Move List's modal went (its close button, Cancel): the screen goes with it.
        if self.list_up && !Manager::is_modal_active(&self.menu.screen.view) && !self.control.paused {
            self.list_up = false;
            self.set_active(sh, false);
        }
        if let Some(on) = self.menu.session.take_pause_request() {
            self.match_set_pause(sh, on, &mut effects);
        }
        if self.menu.session.take_back_request() {
            self.back_to_matchmaking(sh, &mut effects);
        }
        out.leave = matches!(exit, Some(MenuExit::OpenLevel { .. }));
        out.config_changed = self.menu.session.take_config_changed();
        out.effects = effects;
        out
    }

    fn toggle_move_list(&mut self, sh: &mut Shell) {
        let Self { menu, bindings, .. } = self;
        with_widget(sh, &mut menu.screen, bindings, Some(&mut menu.session), None, |cx| {
            // (the pause menu stays collapsed under the list)
            toggle_movelist(cx);
        });
        if self.menu.screen.view.manager.get_modal(MOVELIST).is_some() {
            self.list_up = true;
            self.set_active(sh, true);
        }
    }

    /// One tick of the unpaused match (labTick): the queue's step (tickQueue), then the spawn delay counts down and
    /// ends in a spawn.
    pub fn tick(&mut self, sh: &mut Shell) -> Option<LabEffect> {
        // (the map load's Delay 2.0 before the cover goes)
        if self.reveal_left > 0 {
            self.reveal_left -= 1;
            if self.reveal_left == 0 {
                self.dismiss_cover(sh);
            }
        }
        if let Some(e) = self.tick_queue(sh) {
            return Some(e);
        }
        let i = LabInputs {
            settings: &sh.settings,
            fighters: &self.fighters,
            pool: &self.pool,
            random: &mut *self.rng,
            first: [None, None],
        };
        self.control.tick(&self.menu.session, &i)
    }

    /// Spawn All Players is pending and will restart the match on another roster: the config and roll it restarts
    /// with (what a load ahead of the restart is made for).
    pub fn restart_ahead(&mut self, sh: &Shell) -> Option<(LabConfig, LabRoll)> {
        if !self.control.spawning() {
            return None;
        }
        let i = LabInputs {
            settings: &sh.settings,
            fighters: &self.fighters,
            pool: &self.pool,
            random: &mut *self.rng,
            first: [None, None],
        };
        let rolled = self.control.rolled().clone();
        let restarts = lab_roster_skins(&i, &self.menu.session.config, &rolled) != self.control.roster();
        restarts.then(|| (self.menu.session.config.clone(), rolled))
    }

    /// The paused menu holds another map: the config and roll its unpause will load (`LabControl::map_ahead`).
    pub fn map_ahead(&mut self, sh: &Shell) -> Option<(LabConfig, LabRoll)> {
        let mut i = LabInputs {
            settings: &sh.settings,
            fighters: &self.fighters,
            pool: &self.pool,
            random: &mut *self.rng,
            first: [None, None],
        };
        self.control.map_ahead(&self.menu.session, &mut i)
    }

    /// The first frame is drawn: after a map load the new level is in (TrainingContainer @168-287): the cover goes at
    /// once when the game is paused, else after Delay 2.0 (@1846) of game time (`tick`).
    // Deviation: the build's Delay 0.2 -> Spawn All Players (@1658-1809) after the load; the port's match has its
    // fighters from its first tick, 2.0 s under the cover where the build's spawn 1.8 s before it goes.
    pub fn reveal(&mut self, sh: &mut Shell) {
        if !self.map_load_active {
            return;
        }
        if self.control.paused {
            self.dismiss_cover(sh);
            return;
        }
        self.reveal_left = latent_ticks(2.0);
    }

    // ---------- the Lab while queued (Wait in the Lab) ----------

    /// The frame's queue step: bIsMatchmaking for the menu; UI_Game @1932-2421's timer when the Lab opens with a
    /// search running (once: it stays on the kept menu over a restart), taken off when the search ends (the port's
    /// own, a case the build does not have: its count would go on); a found match's chain.
    fn queue_frame(&mut self, sh: &mut Shell, now: f64, out: &mut Vec<LabEffect>) {
        let opening = !std::mem::replace(&mut self.queue_opened, true);
        let Some(hub) = sh.online.clone() else { return };
        let (searching, found, elapsed_s, modes, found_once) = {
            let h = hub.borrow();
            let clock = h.clock();
            let Some(q) = h.lab_queue() else {
                self.menu.session.matchmaking = false;
                return;
            };
            let found = q.found.is_some() && !q.lab_found_once;
            (q.searching, found, (clock - q.start_ms) / 1000.0, q.modes.clone(), q.lab_found_once)
        };
        // (a Lab restart inside the chain: the notification's screen stays up)
        if opening && found_once {
            self.notification_screen(sh);
        }
        self.menu.session.matchmaking = searching;
        if opening && searching && self.menu.queue_timer.is_none() {
            // (the search's start on the Lab view's clock: `now` less the time it has run on the hub's)
            let src = TimerSource { start_s: now - elapsed_s, modes };
            self.menu.queue_timer = self.with_run(sh, |screen, run| add_lab_matchmaking_timer(screen, run, src));
        }
        if !searching && let Some(mut t) = self.menu.queue_timer.take() {
            self.with_run(sh, |screen, run| t.remove(screen, run));
        }
        if found {
            self.found(sh, &hub, out);
        }
    }

    /// The match-found chain TGM@2966 (On Background Matchmaking Complete TGM@3775, or ReceiveTick TGM@4306 seeing
    /// AreAllPlayersRegistered): a DoOnce, then unpause TGM@2089, BPI_SetCanPause(false) TGM@2216 (`lab_leave`
    /// gates the pause key), ui_map_matchfound and the notification (LabPauseMenu.match_found), Delay 3.0 TGM@2871.
    /// The DoOnce stays closed for the level's life; Back To Matchmaking does not close it: a find during its Delay
    /// or EnterPreMatch's fade plays the sound and the notification over them, and the chain's Delay outlasts the level.
    // Unverified: the fighters, the bot and the input go on for the 3.0 s (no graph read stops them).
    fn found(
        &mut self,
        sh: &mut Shell,
        hub: &std::rc::Rc<std::cell::RefCell<crate::online::hub::OnlineHub>>,
        out: &mut Vec<LabEffect>,
    ) {
        if let Some(q) = hub.borrow_mut().lab_queue_mut() {
            q.lab_found_once = true;
        }
        self.unpause_game(sh, out);
        let w = self.menu.pause_menu;
        let Self { menu, bindings, .. } = self;
        with_widget(sh, &mut menu.screen, bindings, Some(&mut menu.session), None, |cx| {
            // (the Lab's menu view is not ticked while the match runs: its InternalTime brought to now before the
            // notification's Add Timer reads it)
            let now = cx.view.now();
            cx.view.manager.sync_internal_time(now);
            cx.visit::<LabPauseMenu, _>(w, |m, cx| m.match_found(cx));
        });
        self.notification_screen(sh);
        if let Some(q) = hub.borrow_mut().lab_queue_mut()
            && q.lab_leave.is_none()
        {
            q.lab_leave = Some((LabLeave::Found, latent_ticks(3.0)));
        }
    }

    /// The notification is the Lab menu view's: its screen shows over the running match for it, with UI_LAB_PauseMenu
    /// collapsed (the build's is: the unpause hid it); the fighters keep the game's keys.
    fn notification_screen(&mut self, sh: &mut Shell) {
        let w = self.menu.pause_menu;
        let Self { menu, bindings, .. } = self;
        with_widget(sh, &mut menu.screen, bindings, Some(&mut menu.session), None, |cx| {
            cx.view.tree_mut().widget_mut(w).vis = Vis::Collapsed;
        });
        self.set_active(sh, true);
    }

    /// SetGamePaused(false) TGM@2089 / @3978: the native unpause, read as the menu's own unpause without its config
    /// check (the menu goes, its sound mix off; UnpauseConfigChangeCheck is not run).
    // Unverified: whether it closes an open pause menu is not in Blueprint.
    fn unpause_game(&mut self, sh: &mut Shell, out: &mut Vec<LabEffect>) {
        if !self.control.paused {
            return;
        }
        self.control.paused = false;
        self.control.unpaused_this_frame = true;
        self.pause.paused = false;
        self.show(sh, false);
        //
        sh.devices.resume_all();
        out.push(LabEffect::Paused(false));
    }

    /// Back To Matchmaking TGM@4005 (the quit dialog's OK while queued, UI_LAB_Panel OnQuitConfirm LP@13490): Destroy
    /// All Players, SetGamePaused(false) TGM@3978, BPI_SetCanPause(false) TGM@3885, Delay 1.0 TGM@3923, then
    /// BPI_EnterPreMatch TGM@511. No sound, no notification.
    // Deviation: Destroy All Players is the port's AbductAll (the AbductPlayer effect and sound on every fighter); the
    // pawns are not destroyed and stay drawn until the fade covers them.
    fn back_to_matchmaking(&mut self, sh: &mut Shell, out: &mut Vec<LabEffect>) {
        let Some(hub) = sh.online.clone() else { return };
        {
            let mut h = hub.borrow_mut();
            let Some(q) = h.lab_queue_mut() else { return };
            if q.lab_leave.is_some() {
                return;
            }
            q.lab_leave = Some((LabLeave::Back, latent_ticks(1.0)));
        }
        out.push(LabEffect::AbductAll);
        self.unpause_game(sh, out);
    }

    /// The Delays of the chain and of Back To Matchmaking in the world's ticks (the game is unpaused for both): at
    /// their end BPI_EnterPreMatch (PGI@30694): Transition{"", type 0, options "Lobby"} PGI@31567, then Delay 1.0
    /// PGI@31709 -> OpenLevel(PreMatch) PGI@350, the return path's screen (`LabEffect::Leave`).
    /// Not ported: EasyAntiCheat's ClientEndSession, CharacterViewerExitData, the controllers' UnPossess and
    /// PlayerDataManager.Reset (the level's end does what shows of them).
    fn tick_queue(&mut self, sh: &mut Shell) -> Option<LabEffect> {
        let hub = sh.online.clone()?;
        let phase = {
            let mut h = hub.borrow_mut();
            let q = h.lab_queue_mut()?;
            let (phase, left) = q.lab_leave.as_mut()?;
            // (at 0 the level is being left: nothing runs again)
            if *left == 0 {
                return None;
            }
            *left -= 1;
            if *left > 0 {
                return None;
            }
            if *phase == LabLeave::Fade {
                q.back_to_pre_match = true;
            }
            *phase
        };
        let Self { menu, bindings, .. } = self;
        if phase != LabLeave::Fade {
            with_widget(sh, &mut menu.screen, bindings, Some(&mut menu.session), None, |cx| {
                Manager::transition(
                    cx,
                    TransitionParams {
                        next_level: String::new(),
                        kind: 0,
                        delay: 0.0,
                        force: false,
                        instant: false,
                        level_options: "Lobby".into(),
                    },
                );
            });
            if let Some(q) = hub.borrow_mut().lab_queue_mut() {
                q.lab_leave = Some((LabLeave::Fade, latent_ticks(1.0)));
            }
            return None;
        }
        with_widget(sh, &mut menu.screen, bindings, Some(&mut menu.session), None, |cx| {
            Manager::unfocus_all(cx, 0);
            Manager::dismiss_all_modals(cx);
        });
        Some(LabEffect::Leave)
    }

    /// `f` on the Lab's screen and a menu run of the shell's parts (the timer's add and remove).
    fn with_run<R>(&mut self, sh: &mut Shell, f: impl FnOnce(&mut MenuScreen, &mut MenuRun<'_>) -> R) -> Option<R> {
        let Shell { ui, settings, audio, music, window, .. } = sh;
        let u = ui.as_mut()?;
        let Self { menu, bindings, .. } = self;
        let mut sink =
            MenuAudio { audio, tick: music.tick, window: Some(crate::apply::window_info(window)), equip: None };
        let mut run = MenuRun {
            fonts: &mut u.menu_fonts,
            prefs: settings,
            bindings,
            sink: &mut sink,
            pointer: &mut u.pointer,
            lab: Some(&mut menu.session as &mut dyn LabSession),
            pause: None,
            tutorial: None,
        };
        Some(f(&mut menu.screen, &mut run))
    }

    /// The Lab's session as the menu's widgets see it.
    pub fn session(&mut self) -> &mut dyn LabSession {
        &mut self.menu.session
    }
}

/// latentFrames(s) as a whole count of ticks.
fn latent_ticks(s: f64) -> u32 {
    // (latentFrames counts whole ticks: its value is an integer)
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = mvs_sim::fighter::common::latent_frames(mvs_sim::math::narrow(s)) as u32;
    n
}
