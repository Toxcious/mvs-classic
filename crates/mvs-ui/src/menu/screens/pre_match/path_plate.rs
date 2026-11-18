//! (the port's own) The match's path on the pre-match screen (pm.path): a notice to this player when it is past the
//! far limit on every way (it may still play). The beta's matches all ran on its own servers, so it has no widget of
//! the build: a layer over UI_PreMatch under its title, seen from pm.path on. With no pm.path (an offline game, a core
//! that does not send it) nothing is made and the screen is the build's. pm.path's round trips by slot are not shown
//! (no connection bars: the user's choice).

use mvs_protocol::messages::ServerMessage;
use serde_json::{Value, json};

use crate::bundle::{Rgba, WData};
use crate::menu::controller::Cx;
use crate::menu::custom_text::PATH_FAR;
use crate::menu::online::OnlineEvent;
use crate::menu::view::MenuView;
use crate::menu::widgets::controls::wd;
use crate::slate::{Vis, WidgetId};

/// The notice's top below the screen's top (Slate units): under the title (SELECT A FIGHTER, MATCH FOUND!).
const NOTICE_TOP: f64 = 74.0;
/// The notice's layer: over UI_PreMatch (0) and its select panels, under the modals.
const NOTICE_Z: i32 = 1;

/// pm.path as the screen shows it: whether each fighter slot is far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathShown {
    pub far: Vec<bool>,
}

impl PathShown {
    /// The notice of `me` (this game's fighter slot): None when it is not far.
    pub fn far_line(&self, me: usize) -> Option<&'static str> {
        self.far.get(me).copied().unwrap_or(false).then_some(PATH_FAR)
    }
}

/// The path of a pm.path message (None: another message).
pub fn read(m: &ServerMessage) -> Option<PathShown> {
    let ServerMessage::PmPath { far, .. } = m else { return None };
    Some(PathShown { far: far.clone() })
}

/// An sRGB colour as the widgets' linear one.
fn srgb(hex: u32, a: f64) -> Rgba {
    let lin = |v: f64| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    let c = |s: u32| lin(f64::from((hex >> s) & 255) / 255.0);
    [c(16), c(8), c(0), a]
}

fn linear(c: Rgba) -> Value {
    json!({ "R": c[0], "G": c[1], "B": c[2], "A": c[3] })
}

fn node(cx: &mut Cx<'_>, t: &str, name: &str, p: Value, s: Value, slot: &str) -> WidgetId {
    let n = WData { t: t.to_owned(), n: name.to_owned(), p, st: slot.to_owned(), s, ..WData::default() };
    let bundle = cx.view.bundle.clone();
    cx.view.tree_mut().create_node(&bundle, &n)
}

/// The plate: the path last told (None: none, nothing shown) and the notice made at the first far.
#[derive(Debug, Default)]
pub struct PathPlate {
    shown: Option<PathShown>,
    notice: Option<WidgetId>,
}

impl PathPlate {
    /// The frame's online events: a pm.path is shown from now on, a new pre-match or a found match drops it.
    pub fn take(&mut self, events: &[OnlineEvent]) {
        for e in events {
            let OnlineEvent::Server(m) = e else { continue };
            match &**m {
                ServerMessage::PmBegin(_) | ServerMessage::QueueFound(_) => self.shown = None,
                m => {
                    if let Some(p) = read(m) {
                        self.shown = Some(p);
                    }
                }
            }
        }
    }

    pub fn shown(&self) -> Option<&PathShown> {
        self.shown.as_ref()
    }

    /// No path told and nothing made: the frame has nothing to do.
    pub fn idle(&self) -> bool {
        self.shown.is_none() && self.notice.is_none()
    }

    /// Shows the notice on this player's far: `me` this game's fighter slot (None: the players are not mapped yet).
    pub fn frame(&mut self, cx: &mut Cx<'_>, me: Option<usize>) {
        if self.idle() {
            return;
        }
        let far = self.shown.as_ref().zip(me).and_then(|(p, me)| p.far_line(me));
        if far.is_some() && self.notice.is_none() {
            self.notice = Some(Self::make_notice(cx));
        }
        if let Some(n) = self.notice {
            wd(cx, n).vis = if far.is_some() { Vis::HitTestInvisible } else { Vis::Collapsed };
        }
    }

    // The notice: a dark box with PATH_FAR, centred under the screen's title, on a layer of its own.
    fn make_notice(cx: &mut Cx<'_>) -> WidgetId {
        let layer = node(
            cx,
            "Overlay",
            "PathFarLayer",
            json!({ "Visibility": "ESlateVisibility::HitTestInvisible" }),
            json!({}),
            "",
        );
        let root = node(
            cx,
            "Border",
            "PathFarNotice",
            json!({ "Background": {}, "BrushColor": linear(srgb(0x0b0e1f, 0.75)) }),
            json!({
                "Padding": { "Top": NOTICE_TOP },
                "HorizontalAlignment": "HAlign_Center",
                "VerticalAlignment": "VAlign_Top",
            }),
            "OverlaySlot",
        );
        let text = node(
            cx,
            "TextBlock",
            "PathFarText",
            json!({
                "Text": { "CultureInvariantString": PATH_FAR },
                "Font": { "FontObject": "PFG", "TypefaceFontName": "NormsCond-Medium", "Size": 26.0 },
                "ColorAndOpacity": { "SpecifiedColor": { "R": 1.0, "G": 1.0, "B": 1.0, "A": 1.0 } },
            }),
            json!({ "Padding": { "Left": 22, "Top": 8, "Right": 22, "Bottom": 8 } }),
            "BorderSlot",
        );
        cx.view.tree_mut().add(root, text);
        cx.view.tree_mut().add(layer, root);
        MenuView::add_to_viewport(cx, layer, NOTICE_Z);
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(v: serde_json::Value) -> ServerMessage {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_pm_path_gives_the_far_notice() {
        let m =
            msg(json!({ "t": "pm.path", "referee": 1, "ms": [120, 0, null, 500], "far": [false, false, false, true] }));
        let p = read(&m).unwrap();
        assert_eq!(p.far_line(3), Some(PATH_FAR));
        assert_eq!(p.far_line(0), None);
        // (a slot past the list: not far)
        assert_eq!(p.far_line(7), None);
    }

    #[test]
    fn no_pm_path_shows_nothing() {
        let mut plate = PathPlate::default();
        plate.take(&[OnlineEvent::Server(Box::new(msg(json!({ "t": "pm.ready", "slot": 1 }))))]);
        assert!(plate.shown().is_none() && plate.idle());
        assert!(read(&msg(json!({ "t": "pm.ready", "slot": 1 }))).is_none());
        let path = msg(json!({ "t": "pm.path", "referee": -1, "ms": [30, 90], "far": [false, true] }));
        plate.take(&[OnlineEvent::Server(Box::new(path))]);
        assert_eq!(plate.shown().map(|p| p.far_line(1)), Some(Some(PATH_FAR)));
        // a new pre-match drops it
        let begin =
            msg(json!({ "t": "pm.begin", "id": "pm2", "kind": "lobby", "mode": null, "teamStyle": 0, "map": "",
            "me": 0, "players": [] }));
        plate.take(&[OnlineEvent::Server(Box::new(begin))]);
        assert!(plate.shown().is_none());
    }
}
