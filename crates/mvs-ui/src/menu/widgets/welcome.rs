//! UI_WelcomeScreen: the title screen, created by the WelcomeScreen level
//! Blueprint. Its own tree is a 2560 x 1440 SizeBox scaled to fit; the port scales the background image up to cover
//! the whole viewport (Deviation), the content stays as the build lays it out.

use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::view::MenuView;
use crate::menu::widgets::controls::interactor::FocusedCommon;
use crate::slate::{Vis, WidgetId};

const WELCOME_FADE_IN: f64 = 0.75;
const WELCOME_FADE_OUT: f64 = 1.0;
/// ScaleBox_0 (ScaleToFitX) in the Content overlay with slot padding -5: the square image is 2570 units wide.
const WELCOME_BG: f64 = 2570.0;

// Deviation: the announcement is the port's own, not the build's ("MultiVersus Will Return Early 2024!" and its three
// paragraphs). The tag, the fonts and the button's text are the tree's.
const ANNOUNCEMENT_TITLE: &str = "MultiVersus Has Returned in All Its Glory!";
const ANNOUNCEMENT_BODY: [&str; 4] = [
    "The MultiVersus experience you know and love is finally back! After everything the game has been through, MVS Classic brings back the version that started it all: the original Open Beta, restored so you can play MultiVersus the way it was meant to be played.",
    "MVS Classic goes back to June 25, 2023. It is built around the final Open Beta patch, from before the original game was taken offline and relaunched in 2024 with significant changes. From the original gameplay mechanics and character movesets to the overall feel of combat, everything that made the Open Beta so special is being brought back.",
    "MVS Classic is an unofficial fan project, built by a single fan for the community. I made it out of passion for the game, to preserve what made the Open Beta special and to give players another chance to enjoy the MultiVersus they know and love. This project is not affiliated with, endorsed by, or associated with Warner Bros. Games or Player First Games in any way.",
    "The wait is over. Welcome back to the MultiVerse!",
];

const DELAY_FOCUS: u32 = 1;
const STEP_IN: u32 = 2;
const STEP_OUT: u32 = 3;

/// UI_WelcomeScreen.
#[derive(Debug, Default)]
pub struct WelcomeScreen {
    can_handle_press: bool,
    handled_press: bool,
    /// The Pressed event, read by the screen that holds the widget (a listener).
    pub pressed: bool,
    /// FadeOut ended and the widget removed itself (the start-up flow goes on from here).
    pub removed: bool,
    /// The clock of the running fade.
    t0: f64,
}

impl WelcomeScreen {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(WelcomeScreen::default())
    }

    fn content(cx: &Cx<'_>) -> WidgetId {
        cx.view.tree().find_in(cx.w, "Content")
    }

    fn set_opacity(cx: &mut Cx<'_>, a: f64) {
        let c = Self::content(cx);
        cx.view.tree_mut().widget_mut(c).opacity = a;
    }

    /// Port: is a point of the canvas (device pixels) on the "ENTER THE MULTIVERSE" button of the widget `w`, by its
    /// last paint?
    pub fn over_button(cx: &mut Cx<'_>, w: WidgetId, x: f64, y: f64) -> bool {
        let Some(armed) = cx.view.controller::<WelcomeScreen>(w).map(|c| c.can_handle_press && !c.handled_press) else {
            return false;
        };
        let b = cx.view.tree().find_in(w, "FakeButton");
        let Some(p) = cx.view.tree().painted(b) else { return false };
        if !armed {
            return false;
        }
        let m = p.transform;
        let det = m[0] * m[3] - m[1] * m[2];
        if det == 0.0 {
            return false;
        }
        let (dx, dy) = (x - m[4], y - m[5]);
        let lx = (m[3] * dx - m[2] * dy) / det;
        let ly = (m[0] * dy - m[1] * dx) / det;
        lx >= 0.0 && ly >= 0.0 && lx < p.size[0] && ly < p.size[1]
    }

    /// OnPreviewKeyDown (any key but the gamepad stick directions) and OnPreviewMouseButtonDown.
    // Deviation: the screen calls it for a press of the button only (a click on it, or the accept key of the focused
    // button), not for any key.
    pub fn handle_key(&mut self, cx: &mut Cx<'_>) {
        if !self.can_handle_press || self.handled_press {
            return;
        }
        self.handled_press = true;
        self.pressed = true;
        self.fade_out(cx);
    }

    fn fade_in(&mut self, cx: &mut Cx<'_>) {
        Self::set_opacity(cx, 0.0);
        self.t0 = cx.now();
        cx.view.delay(0.0, Handler { owner: cx.w, id: STEP_IN });
    }

    fn fade_out(&mut self, cx: &mut Cx<'_>) {
        Self::set_opacity(cx, 1.0);
        self.t0 = cx.now();
        cx.view.delay(0.0, Handler { owner: cx.w, id: STEP_OUT });
    }

    // Deviation: the image's render scale that covers the viewport at any aspect ratio.
    fn cover(cx: &mut Cx<'_>) {
        let [w, h] = cx.view.viewport_size();
        if w == 0.0 || h == 0.0 {
            return;
        }
        let r = (w / 2560.0).min(h / 1440.0);
        let bg = cx.view.tree().find_in(cx.w, "BG");
        let s = 1.0f64.max(w.max(h) / (WELCOME_BG * r));
        cx.view.tree_mut().widget_mut(bg).transform.scale = [s, s];
    }
}

impl Controller for WelcomeScreen {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        cx.view.delay(0.5, Handler { owner: cx.w, id: DELAY_FOCUS });
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        Self::set_opacity(cx, 0.0);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        Self::set_opacity(cx, 0.0);
        // Deviation: the background is not cut at its ScaleBox.
        let sb = cx.view.tree().find_in(cx.w, "ScaleBox_0");
        cx.view.tree_mut().widget_mut(sb).clip = false;
        Self::cover(cx);
        // Deviation: the port's announcement; no "PRESS ANY BUTTON TO CONTINUE" (the press is the button's, see
        // `handle_key`) and no row of the Player First Games logo, the legal line and the WB Games logo. Hidden, not
        // Collapsed: they keep their place, so the box and the button stay where the build has them.
        let tree = cx.view.tree_mut();
        let (title, body, press, row) = (
            tree.find_in(cx.w, "TextBlock_492"),
            tree.find_in(cx.w, "TextBlock_217"),
            tree.find_in(cx.w, "PressAnyBtn"),
            tree.find_in(cx.w, "HorizontalBox_0"),
        );
        tree.widget_mut(title).text = ANNOUNCEMENT_TITLE.to_string();
        tree.widget_mut(body).text = ANNOUNCEMENT_BODY.join("\r\n\r\n");
        tree.widget_mut(press).vis = Vis::Hidden;
        tree.widget_mut(row).vis = Vis::Hidden;
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        Self::cover(cx);
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            DELAY_FOCUS => {
                let f = cx.view.tree().find_in(cx.w, "buttonFocus");
                cx.visit::<FocusedCommon, _>(f, |c, cx| c.focus(cx));
                // PressAnyBtn: every platform with a text has the tree's own text
                self.fade_in(cx);
            }
            STEP_IN => {
                let a = ((cx.now() - self.t0) / WELCOME_FADE_IN).clamp(0.0, 1.0);
                Self::set_opacity(cx, a);
                if a < 1.0 {
                    cx.view.delay(0.0, Handler { owner: cx.w, id: STEP_IN });
                } else {
                    self.can_handle_press = true;
                }
            }
            STEP_OUT => {
                let a = ((cx.now() - self.t0) / WELCOME_FADE_OUT).clamp(0.0, 1.0);
                Self::set_opacity(cx, 1.0 - a);
                if a < 1.0 {
                    cx.view.delay(0.0, Handler { owner: cx.w, id: STEP_OUT });
                } else {
                    let w = cx.w;
                    MenuView::remove_from_viewport(cx, w);
                    self.removed = true;
                }
            }
            _ => {}
        }
    }
}
