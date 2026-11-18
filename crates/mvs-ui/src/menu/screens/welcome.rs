//! The title screen at start-up: what the WelcomeScreen level Blueprint does once its
//! critical screens are done: Create(UI_WelcomeScreen) + AddToViewport(0); on the widget's Pressed, the global event
//! ui_menu_welcome_play. The login / EULA flow behind the press is not ported; the program goes on to the main menu
//! when the widget has faded out and removed itself. The music's start is the program's (`MenuMusic::title`).
//!
//! Deviation: the build goes on at any key or mouse button. Here only the "ENTER THE MULTIVERSE" button does: a mouse
//! press on it, or Enter / Space / the pad's bottom face button (the button has the focus and nothing else on the
//! screen can take it). A browser's audio unlock (a first press that only starts the sound) has no counterpart: the
//! native sound is never locked.

use crate::menu::controller::{Cx, MenuRun};
use crate::menu::input::MenuInput;
use crate::menu::screen::MenuScreen;
use crate::menu::view::MenuView;
use crate::menu::widgets::welcome::WelcomeScreen;
use crate::slate::WidgetId;

const ACCEPT: [&str; 3] = ["Enter", "SpaceBar", "Gamepad_FaceButton_Bottom"];

/// The title screen's driver: its widget, the pointer's place and the pad's held buttons.
#[derive(Debug)]
pub struct Welcome {
    pub widget: WidgetId,
    at: Option<[f64; 2]>,
    pad_held: bool,
    pressed: bool,
}

impl Welcome {
    /// Create(UI_WelcomeScreen) + AddToViewport(0).
    pub fn show(screen: &mut MenuScreen, run: &mut MenuRun<'_>) -> Welcome {
        let widget = screen.with_cx(run, |cx| MenuView::add(cx, "UI_WelcomeScreen"));
        Welcome { widget, at: None, pad_held: false, pressed: false }
    }

    fn press(&mut self, cx: &mut Cx<'_>) {
        let w = self.widget;
        cx.visit::<WelcomeScreen, _>(w, |c, cx| c.handle_key(cx));
    }

    /// The frame's input: the accept keys, the pad's bottom face button and a mouse press on the button press it.
    pub fn input(&mut self, screen: &mut MenuScreen, run: &mut MenuRun<'_>, input: &[MenuInput]) {
        let widget = self.widget;
        for ev in input {
            match *ev {
                MenuInput::Key { key, down: true, repeat: false, .. } if ACCEPT.contains(&key) => {
                    screen.with_cx(run, |cx| {
                        self.press(cx);
                    });
                }
                MenuInput::MouseMove { x, y } => self.at = Some([x, y]),
                MenuInput::MouseButton { button: 0, down: true } => {
                    let at = self.at;
                    screen.with_cx(run, |cx| {
                        let over = at.is_some_and(|[x, y]| WelcomeScreen::over_button(cx, widget, x, y));
                        if over {
                            self.press(cx);
                        }
                    });
                }
                MenuInput::Pad { state, .. } => {
                    let held = state.buttons & 1 != 0;
                    if held && !self.pad_held {
                        screen.with_cx(run, |cx| self.press(cx));
                    }
                    self.pad_held = held;
                }
                _ => {}
            }
        }
    }

    /// Pressed (once): the widget's Pressed event; the caller posts ui_menu_welcome_play.
    pub fn take_pressed(&mut self, screen: &mut MenuScreen) -> bool {
        let widget = self.widget;
        let p = screen.view.controller::<WelcomeScreen>(widget).is_some_and(|c| c.pressed);
        if p && !self.pressed {
            self.pressed = true;
            return true;
        }
        false
    }

    /// FadeOut ended and the widget removed itself.
    pub fn removed(&self, screen: &mut MenuScreen) -> bool {
        let widget = self.widget;
        screen.view.controller::<WelcomeScreen>(widget).is_none_or(|c| c.removed)
    }
}
