//! A menu viewport with its input and sound: keys, mouse, wheel and the pads go
//! to the view (a listening binding row first) and on to UI_Manager; one frame is the input, the pads' poll, the
//! view's tick and its paint; the opened level's fade in from black.
//!
//! The pointer is the window's: the mouse and the pad's left stick move it (the stick moves
//! the OS pointer with it, as the build's SetMouseLocation does), the view paints the game's cursor at it
//! (cursor.rs), and the OS cursor is not shown over the window while the screen is active.
//! Player 0's last interaction is the window's too (`Pointer::interaction`): the view's manager starts each frame
//! from it and an active screen that took input writes it back, so every screen, the ones made later included, shows
//! the cursor of the device the player last touched.
//!
//! Deviation: no listener takes the events as they come and no frame runs from its own animation
//! frame; here the program hands the frame's input and its clock to `frame`, which takes the input first, in
//! order. The canvas's own show / hide and the view's picture release (the still, the blurs) have
//! no counterpart: the native renderer paints the whole window each frame.

use std::collections::HashSet;

use super::MenuExit;
use super::controller::{Cx, MenuRun};
use super::input::{MenuInput, MenuPad, PadEvent, PadPoller};
use super::local_play_match::{InputSource, local_player_of, set_last_menu_device};
use super::manager::{Manager, TransitionParams};
use super::tape::MenuFrame;
use super::view::{MenuView, NO_WIDGET};
use crate::element::ElementList;
use crate::slate::WidgetId;
use crate::text::Fonts;

/// What runs each frame before the view's tick: the scene's frame, `dt` s.
pub type OnFrame = Box<dyn FnMut(&mut Cx<'_>, f64)>;

/// A menu viewport.
pub struct MenuScreen {
    /// Boxed: the view is a large value, and a screen moves by value through the program's start-up frames (a debug
    /// build's stack held a copy of it in each).
    pub view: Box<MenuView>,
    active: bool,
    /// The view's layers that still show while the screen is not active (the matchmaking timer), ticked and
    /// painted alone then.
    pub alone: Vec<WidgetId>,
    /// Called each frame before the view's tick.
    pub on_frame: Option<OnFrame>,
    pads: PadPoller,
    /// The keys whose press this screen saw (a key that went down while the screen was not active, the pause key
    /// that opens the Lab's menu, is not the menu's: nor is its auto-repeat).
    seen: HashSet<&'static str>,
    /// The Control keys held (a shortcut with Control is not the menu's; the modifier keys themselves are).
    ctrl: [bool; 2],
    /// The clock of the last active frame (None: the next is a first).
    last: Option<f64>,
    /// The clock of the last frame of the `alone` layers.
    alone_last: Option<f64>,
    /// The write of the window's pointer this screen has seen.
    seen_pointer: Option<u64>,
    /// The frames painted (the tape's frame number).
    frames: u32,
    removed: bool,
    /// The view has the game's cursor (UI_Cursor): it owns the pointer while active.
    cursor: bool,
    /// The keys, buttons and wheel only name the device last touched (`pointer_only`).
    pointer_only: bool,
}

impl std::fmt::Debug for MenuScreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuScreen")
            .field("active", &self.active)
            .field("alone", &self.alone)
            .field("frames", &self.frames)
            .field("removed", &self.removed)
            .finish_non_exhaustive()
    }
}

impl MenuScreen {
    /// The screen of `view`: the game's cursor enabled, the OS cursor hidden while it is active.
    pub fn new(view: MenuView, run: &mut MenuRun<'_>) -> MenuScreen {
        let mut s = MenuScreen::without_cursor(view);
        MenuView::enable_cursor(&mut run.cx(&mut s.view, NO_WIDGET));
        s.cursor = true;
        s
    }

    /// The view's manager takes the pointer and the sticks as ever, while a key, a pad button or a mouse button only
    /// names the device last touched and turns cursor mode as the manager's key input does (`Manager::key_seen`):
    /// the title, whose press the program's Welcome driver takes.
    pub fn pointer_only(&mut self) {
        self.pointer_only = true;
    }

    /// The screen of `view` without the game's cursor (a level UI_Cursor's level list does not name); the OS cursor
    /// shows over it.
    pub fn without_cursor(view: MenuView) -> MenuScreen {
        MenuScreen {
            view: Box::new(view),
            active: true,
            alone: Vec::new(),
            on_frame: None,
            pads: PadPoller::new(),
            seen: HashSet::new(),
            ctrl: [false; 2],
            last: None,
            alone_last: None,
            seen_pointer: None,
            frames: 0,
            removed: false,
            cursor: false,
            pointer_only: false,
        }
    }

    /// The context of the view (no widget's: the screen's own calls).
    pub fn with_cx<R>(&mut self, run: &mut MenuRun<'_>, f: impl FnOnce(&mut Cx<'_>) -> R) -> R {
        f(&mut run.cx(&mut self.view, NO_WIDGET))
    }

    /// false: a collapsed viewport (the Lab's menu during play): no input, no tick, nothing painted but `alone`.
    pub fn active(&self) -> bool {
        self.active
    }

    /// An active screen owns the pointer: the OS cursor is not shown over the window (from its next frame).
    pub fn set_active(&mut self, _run: &mut MenuRun<'_>, on: bool) {
        let was = self.active;
        self.active = on;
        if was && !on && !self.removed && !self.alone.is_empty() {
            // the `alone` layers go on showing from this call: their clock starts now
            self.last = None;
            self.alone_last = Some(self.view.now());
        }
    }

    /// The window's pointer copied in when this screen has not seen its last write.
    fn take_pointer(&mut self, run: &mut MenuRun<'_>) {
        if self.seen_pointer == Some(run.pointer.version) {
            return;
        }
        let Some([x, y]) = run.pointer.at else { return };
        self.seen_pointer = Some(run.pointer.version);
        self.view.set_pointer(x, y);
    }

    /// The view's write of the pointer (the cursor's Set Position) given to the window, which moves the OS pointer
    /// there.
    fn give_pointer(&mut self, run: &mut MenuRun<'_>) {
        if !self.view.pointer_written {
            return;
        }
        self.view.pointer_written = false;
        let Some([x, y]) = self.view.mouse else { return };
        run.pointer.set_by_game(x, y);
        self.seen_pointer = Some(run.pointer.version);
    }

    /// The frame's input given to the view and the manager, in order. `pads`: the pads'
    /// states, polled after.
    fn take_input(&mut self, run: &mut MenuRun<'_>, input: &[MenuInput], pads: &mut Vec<(usize, usize, MenuPad)>) {
        for ev in input {
            match *ev {
                MenuInput::Key { key, down: true, repeat, player } => {
                    if let Some(i) = ctrl_index(key) {
                        self.ctrl[i] = true;
                    }
                    if repeat && !self.seen.contains(key) {
                        continue;
                    }
                    if !self.active {
                        continue;
                    }
                    self.seen.insert(key);
                    // (the function keys stay the program's, but for a binding row that listens: the manager's
                    // handleInput is off then)
                    if is_function_key(key) && self.view.manager.handle_input {
                        continue;
                    }
                    // a shortcut with Control stays the program's; the Control keys themselves are the game's
                    if self.ctrl.iter().any(|&c| c) && ctrl_index(key).is_none() {
                        continue;
                    }
                    set_last_menu_device(InputSource::Kbm);
                    if self.pointer_only {
                        Manager::key_seen(&mut run.cx(&mut self.view, NO_WIDGET), key, player as i32);
                        continue;
                    }
                    self.view.key_repeat = repeat;
                    MenuView::key_down(&mut run.cx(&mut self.view, NO_WIDGET), key, player as i32);
                    self.view.key_repeat = false;
                }
                MenuInput::Key { key, down: false, player, .. } => {
                    if let Some(i) = ctrl_index(key) {
                        self.ctrl[i] = false;
                    }
                    self.seen.remove(key);
                    if self.active && !self.pointer_only {
                        Manager::key_up(&mut run.cx(&mut self.view, NO_WIDGET), key, player as i32);
                    }
                }
                MenuInput::MouseMove { x, y } => {
                    // (the event's movementX / Y: from the mouse's own last point, not from where the game's cursor
                    // put the pointer)
                    let from = run.pointer.mouse.replace([x, y]);
                    // (the mouse writes the point whatever shows; a move the pad's
                    // stick made through the OS never comes: the window drops it)
                    run.pointer.set(x, y);
                    if !self.active {
                        continue;
                    }
                    self.take_pointer(run);
                    let dist = from.map_or(0.0, |[fx, fy]| (x - fx).hypot(y - fy));
                    Manager::mouse_move(&mut run.cx(&mut self.view, NO_WIDGET), dist, kbm_player());
                }
                MenuInput::MouseButton { button, down: true } => {
                    if self.active && self.pointer_only {
                        set_last_menu_device(InputSource::Kbm);
                        Manager::mouse_seen(&mut run.cx(&mut self.view, NO_WIDGET), kbm_player());
                    } else if self.active {
                        set_last_menu_device(InputSource::Kbm);
                        MenuView::mouse_down(&mut run.cx(&mut self.view, NO_WIDGET), button, kbm_player());
                    }
                }
                // (the release of a scroll bar's thumb: heard also while the screen is not active, or a drag would
                // never end)
                MenuInput::MouseButton { button, down: false } => self.view.mouse_up(button),
                MenuInput::Wheel { notches } => {
                    if self.active && !self.pointer_only {
                        MenuView::wheel(&mut run.cx(&mut self.view, NO_WIDGET), notches);
                    }
                }
                MenuInput::Char(ch) => {
                    if self.active && !self.pointer_only {
                        MenuView::char_typed(&mut run.cx(&mut self.view, NO_WIDGET), ch);
                    }
                }
                MenuInput::Analog { key, value, player } => {
                    if self.active {
                        Manager::analog_value(&mut run.cx(&mut self.view, NO_WIDGET), key, value, player as i32, 1.0);
                    }
                }
                MenuInput::Pad { pad, player, state } => pads.push((pad, player, state)),
            }
        }
    }

    /// The pads' poll: presses to the view, releases and the sticks to the manager.
    fn poll_pads(&mut self, run: &mut MenuRun<'_>, pads: &[(usize, usize, MenuPad)], first: bool, dt: f64) {
        let now = self.view.now();
        let mut evs = Vec::new();
        self.pads.poll(pads, first, dt, now, &mut |e| evs.push(e));
        let mut cx = run.cx(&mut self.view, NO_WIDGET);
        for e in evs {
            match e {
                PadEvent::Press { key, player, pad, .. } => {
                    set_last_menu_device(InputSource::Pad(pad));
                    if self.pointer_only {
                        Manager::key_seen(&mut cx, key, player as i32);
                    } else {
                        MenuView::key_down(&mut cx, key, player as i32);
                    }
                }
                PadEvent::Release { key, player } if !self.pointer_only => Manager::key_up(&mut cx, key, player as i32),
                PadEvent::Release { .. } => {}
                PadEvent::Raw { value } => MenuView::analog(&mut cx, value),
                PadEvent::Analog { key, value, player, frames } => {
                    Manager::analog_value(&mut cx, key, value, player as i32, frames);
                }
            }
        }
    }

    /// One frame at the program's clock `now` (s): the input given to the view, the pads' poll, `on_frame`, the
    /// view's tick and its paint to `out`; what the menus ask of the program when the frame ends (a transition's
    /// level). A screen that is not active paints its `alone` layers, or the fader while a transition runs.
    pub fn frame(
        &mut self,
        run: &mut MenuRun<'_>,
        input: &[MenuInput],
        now: f64,
        out: &mut ElementList,
    ) -> Option<MenuExit> {
        if self.removed {
            return None;
        }
        self.view.set_now(now);
        // (the window's last interaction: another screen's, or the touches of frames no screen took input in)
        self.view.manager.set_interaction(run.pointer.interaction);
        let mut pads = Vec::new();
        self.take_input(run, input, &mut pads);
        if !self.active {
            self.last = None;
            let mut cx = run.cx(&mut self.view, NO_WIDGET);
            if !self.alone.is_empty() {
                // the layers that show alone, with UI_Global's Fader over them while a transition runs
                if cx.view.manager.is_transitioning() {
                    Manager::tick_transition(&mut cx);
                }
                let dt = self.alone_last.map_or(0.0, |l| (now - l).min(0.1));
                self.alone_last = Some(now);
                let alone = self.alone.clone();
                MenuView::tick_alone(&mut cx, &alone, dt);
                self.view.paint_alone(run.fonts, out, &alone);
            } else {
                self.alone_last = None;
                // UI_Global's transitioner and its Fader are over the viewport whatever widgets show: a transition
                // goes on while this screen's widgets are collapsed, painted as the fader alone
                if cx.view.manager.is_transitioning() {
                    Manager::tick_transition(&mut cx);
                    self.view.paint_fader(out);
                }
            }
            return self.exit();
        }
        self.alone_last = None;
        if self.cursor {
            run.pointer.claim_os_cursor();
        }
        if !input.is_empty() {
            run.pointer.screen_took_input();
        }
        let first = self.last.is_none();
        let dt = self.last.map_or(0.0, |l| (now - l).min(0.1));
        self.last = Some(now);
        if self.view.size()[0] == 0 || self.view.size()[1] == 0 {
            return None;
        }
        self.take_pointer(run);
        self.poll_pads(run, &pads, first, dt);
        if let Some(mut f) = self.on_frame.take() {
            f(&mut run.cx(&mut self.view, NO_WIDGET), dt);
            if self.on_frame.is_none() {
                self.on_frame = Some(f);
            }
        }
        if !self.removed {
            MenuView::tick(&mut run.cx(&mut self.view, NO_WIDGET));
        }
        self.give_pointer(run);
        run.pointer.interaction = self.view.manager.interaction();
        if !self.removed {
            self.view.paint(run.fonts, out);
            self.frames += 1;
        }
        self.exit()
    }

    /// The transition whose fade ended with a level: OpenLevel.
    fn exit(&mut self) -> Option<MenuExit> {
        let p = self.view.manager.take_transition()?;
        if p.next_level == "PreMatch"
            && let Some(start) = crate::menu::lobby::with_carry(|c| c.pre_match.take())
        {
            return Some(MenuExit::PreMatch(start));
        }
        Some(MenuExit::OpenLevel { level: p.next_level, options: p.level_options })
    }

    /// The tape line of the frame just painted (menu/tape.rs; `frame` counts from 1).
    pub fn tape_frame(&self) -> MenuFrame {
        self.view.tape_frame(self.frames)
    }

    /// The opened level's BeginPlay transition from black. TrainingContainer ReceiveBeginPlay (level Blueprint
    /// @2712-2932) is the first thing the Lab's level does: FindUIManager, Transition UIManager {nextLevel "",
    /// transitionType 1, delay 0, force false, instant false, levelOptions Lobby}: the Fader goes from 1 to 0 over
    /// 0.3 s of real time and the RTPC transition to 1 over 3000 ms. Called at the level's first shown frame; a
    /// screen that is not active paints the fader alone meanwhile (`frame`). `now`: the program's clock (s), the
    /// fade's start.
    pub fn level_fade_in(&mut self, run: &mut MenuRun<'_>, now: f64) {
        self.view.set_now(now);
        let p = TransitionParams {
            next_level: String::new(),
            kind: 1,
            delay: 0.0,
            force: false,
            instant: false,
            level_options: "Lobby".to_string(),
        };
        Manager::transition(&mut run.cx(&mut self.view, NO_WIDGET), p);
    }

    /// Only the fader painted (a screen not active while its level fades in).
    pub fn paint_fader(&mut self, _fonts: &mut Fonts, out: &mut ElementList) {
        self.view.paint_fader(out);
    }

    /// The screen is gone: the OS cursor back, the cursor's Destruct, every picture let go.
    pub fn remove(&mut self, run: &mut MenuRun<'_>) {
        if self.removed {
            return;
        }
        self.removed = true;
        MenuView::remove_cursor(&mut run.cx(&mut self.view, NO_WIDGET));
        self.view.release_held();
    }
}

/// LeftControl 0, RightControl 1.
fn ctrl_index(key: &str) -> Option<usize> {
    match key {
        "LeftControl" => Some(0),
        "RightControl" => Some(1),
        _ => None,
    }
}

/// F1 .. F24.
fn is_function_key(key: &str) -> bool {
    key.strip_prefix('F').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The local player holding the keyboard and mouse, -1 none.
fn kbm_player() -> i32 {
    local_player_of(InputSource::Kbm, -1).map_or(-1, |p| p as i32)
}
