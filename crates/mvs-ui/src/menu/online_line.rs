//! (the port's own) The menus' one line of online play (`OnlineLine`): its words (a headline, and for a line that
//! asks something of the player a sentence of what to do), and the two text blocks the custom game browser shows
//! them in. The build has no such line: its home screen shows OFFLINE MODE (UI_Homescreen_v2
//! OfflineModeLabel) and no reason.

use super::controller::Cx;
use super::custom_text as t;
use super::online::{OfflineWhy, OnlineLine};
use super::view::MenuView;
use crate::bundle::Rgba;
use crate::slate::{HAlign, Margin, VAlign, Vis, WidgetId};

/// How the line reads at a glance (its headline's colour, the launcher's note's tone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineTone {
    /// Online.
    Good,
    /// Online by the slower way, or waiting for a check or a sign-in.
    Plain,
    /// Offline for a reason the player can act on.
    Bad,
}

/// The line's headline: "Online", "Online, but matches may lag", a check or a sign-in under way, else "Offline: " and
/// why (online play turned off says so alone).
pub fn headline(l: &OnlineLine) -> String {
    let why = match l {
        OnlineLine::Online => return t::LINE_ONLINE.to_owned(),
        OnlineLine::Slower => return t::LINE_SLOWER.to_owned(),
        OnlineLine::Offline(why) => why,
    };
    let reason = match why {
        OfflineWhy::Checking => return t::LINE_CHECKING.to_owned(),
        OfflineWhy::SigningIn => return t::LINE_SIGNING_IN.to_owned(),
        OfflineWhy::TurnedOff => return t::LINE_TURNED_OFF.to_owned(),
        OfflineWhy::Update { .. } => t::LINE_UPDATE,
        OfflineWhy::Import => t::LINE_IMPORT,
        OfflineWhy::SelfCheck => t::LINE_SELF_CHECK,
        OfflineWhy::NotReached => t::LINE_NOT_REACHED,
        OfflineWhy::Other => t::LINE_OTHER,
    };
    format!("{}: {reason}", t::LINE_OFFLINE)
}

/// What the player can do about the line (None: online).
pub fn advice(l: &OnlineLine) -> Option<String> {
    let why = match l {
        OnlineLine::Online => return None,
        OnlineLine::Slower => return Some(t::LINE_SLOWER_DO.to_owned()),
        OnlineLine::Offline(why) => why,
    };
    Some(match why {
        OfflineWhy::Checking => t::LINE_CHECKING_DO.to_owned(),
        OfflineWhy::SigningIn => t::LINE_SIGNING_IN_DO.to_owned(),
        OfflineWhy::Update { version } => t::LINE_UPDATE_DO.replace("{version}", version),
        OfflineWhy::Import => t::LINE_IMPORT_DO.to_owned(),
        OfflineWhy::SelfCheck => t::LINE_SELF_CHECK_DO.to_owned(),
        OfflineWhy::TurnedOff => t::LINE_TURNED_OFF_DO.to_owned(),
        OfflineWhy::NotReached => t::LINE_NOT_REACHED_DO.to_owned(),
        OfflineWhy::Other => t::LINE_OTHER_DO.to_owned(),
    })
}

pub fn tone(l: &OnlineLine) -> LineTone {
    match l {
        OnlineLine::Online => LineTone::Good,
        OnlineLine::Slower | OnlineLine::Offline(OfflineWhy::Checking | OfflineWhy::SigningIn) => LineTone::Plain,
        OnlineLine::Offline(_) => LineTone::Bad,
    }
}

/// The headline in the build's menu face (UI_Homescreen_v2 OfflineModeLabel's typeface), the advice in its body
/// face; cream as the label, the headline's colour by its tone.
const HEAD_FACE: &str = "NormsCond-ExtraBoldItalic";
const ADVICE_FACE: &str = "NormsCond-Medium";
const CREAM: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
const CREAM_DIM: Rgba = [0.89627, 0.973446, 0.871367, 0.8];
/// The build's orange text (UI_Homescreen_v2 TextBlock_66, JUMP TO).
const ORANGE: Rgba = [0.982251, 0.558341, 0.056128, 1.0];
const GREEN: Rgba = [0.25, 0.85, 0.35, 1.0];
const RED: Rgba = [1.0, 0.28, 0.22, 1.0];

/// Where a screen shows the line and how big.
#[derive(Clone, Copy, Debug)]
pub struct LinePlace {
    pub h_align: HAlign,
    pub v_align: VAlign,
    pub pad: Margin,
    pub head_size: f64,
    pub advice_size: f64,
    /// The advice wraps at this width (Slate units).
    pub wrap: f64,
    /// The advice's height is kept at this many lines at least (a longer advice wraps to two: what is under the line
    /// does not move between them). 0: its own height.
    pub advice_lines: u8,
}

/// A line of the advice's text per unit of its font size (NormsCond-Medium's line height: two lines at 20 fill 70
/// on Home's picture, one line and two lines put the buttons at the same place).
// Unverified: the factor is read off the native exe's picture, not the font's metrics.
const ADVICE_LINE: f64 = 1.75;

/// The line's two text blocks in a VerticalBox of their own: the headline upper-case in its colour, the advice under it.
#[derive(Clone, Debug)]
pub struct LineWidget {
    pub root: WidgetId,
    head: WidgetId,
    advice: WidgetId,
    /// The line shown (None: hidden, or not shown yet).
    shown: Option<OnlineLine>,
}

impl LineWidget {
    /// The line's widgets made into `parent` at `at` (None: last), hidden until `show`.
    pub fn make(cx: &mut Cx<'_>, parent: WidgetId, at: Option<usize>, place: LinePlace) -> LineWidget {
        let bundle = cx.view.bundle.clone();
        let t = cx.view.tree_mut();
        let root = t.create_plain(&bundle, "VerticalBox", "OnlineLine");
        let head = t.create_plain(&bundle, "TextBlock", "OnlineLineHead");
        let advice = t.create_plain(&bundle, "TextBlock", "OnlineLineAdvice");
        for (w, face, size, color) in
            [(head, HEAD_FACE, place.head_size, CREAM), (advice, ADVICE_FACE, place.advice_size, CREAM_DIM)]
        {
            let x = t.widget_mut(w);
            x.typeface = Some(face.to_owned());
            x.font_size = size;
            x.color = color;
            x.h_align = HAlign::Left;
            x.v_align = VAlign::Top;
            x.vis = Vis::HitTestInvisible;
        }
        t.widget_mut(advice).wrap_at = place.wrap;
        t.widget_mut(advice).pad = Margin { l: 0.0, t: 2.0, r: 0.0, b: 0.0 };
        t.add(root, head);
        if place.advice_lines > 0 {
            let size = t.create_plain(&bundle, "SizeBox", "OnlineLineAdviceSize");
            t.widget_mut(size).min_h = place.advice_size * ADVICE_LINE * f64::from(place.advice_lines);
            t.add(size, advice);
            t.add(root, size);
        } else {
            t.add(root, advice);
        }
        {
            let x = t.widget_mut(root);
            x.h_align = place.h_align;
            x.v_align = place.v_align;
            x.pad = place.pad;
            x.vis = Vis::Collapsed;
        }
        MenuView::add_child(cx, parent, root, at);
        LineWidget { root, head, advice, shown: None }
    }

    /// Shows `line` (None: hidden), its words written when it changed.
    pub fn show(&mut self, cx: &mut Cx<'_>, line: Option<&OnlineLine>) {
        if self.shown.as_ref() == line {
            return;
        }
        self.shown = line.cloned();
        let t = cx.view.tree_mut();
        let Some(l) = line else {
            t.widget_mut(self.root).vis = Vis::Collapsed;
            return;
        };
        t.widget_mut(self.root).vis = Vis::HitTestInvisible;
        let color = match tone(l) {
            LineTone::Good => GREEN,
            LineTone::Plain if *l == OnlineLine::Slower => ORANGE,
            LineTone::Plain => CREAM,
            LineTone::Bad => RED,
        };
        let h = t.widget_mut(self.head);
        h.text = headline(l).to_uppercase();
        h.color = color;
        // (Collapsed: a line with nothing to do is the headline alone; the line stands on the page's foot, so it
        // grows upward and nothing under it moves)
        let a = advice(l);
        let x = t.widget_mut(self.advice);
        x.vis = if a.is_some() { Vis::HitTestInvisible } else { Vis::Collapsed };
        x.text = a.unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_line_has_its_words_and_names_no_backend() {
        let lines = [
            OnlineLine::Online,
            OnlineLine::Slower,
            OnlineLine::Offline(OfflineWhy::Checking),
            OnlineLine::Offline(OfflineWhy::SigningIn),
            OnlineLine::Offline(OfflineWhy::Update { version: "1.2.3".into() }),
            OnlineLine::Offline(OfflineWhy::Import),
            OnlineLine::Offline(OfflineWhy::SelfCheck),
            OnlineLine::Offline(OfflineWhy::TurnedOff),
            OnlineLine::Offline(OfflineWhy::NotReached),
            OnlineLine::Offline(OfflineWhy::Other),
        ];
        for l in &lines {
            let words = format!("{} {}", headline(l), advice(l).unwrap_or_default());
            for name in ["relay", "Relay", "Epic", "EOS", "LAN", "UDP", "TCP"] {
                assert!(!words.contains(name), "{l:?}: {words}");
            }
            assert!(!headline(l).is_empty());
        }
        assert_eq!(headline(&OnlineLine::Online), t::LINE_ONLINE);
        assert_eq!(advice(&OnlineLine::Online), None);
        assert_eq!(headline(&OnlineLine::Offline(OfflineWhy::NotReached)), "Offline: Server not reached");
        let update = OnlineLine::Offline(OfflineWhy::Update { version: "1.2.3".into() });
        assert!(advice(&update).is_some_and(|a| a.contains("1.2.3") && !a.contains('{')));
        assert_eq!(tone(&OnlineLine::Slower), LineTone::Plain);
        assert_eq!(tone(&OnlineLine::Offline(OfflineWhy::SigningIn)), LineTone::Plain);
        assert_eq!(tone(&OnlineLine::Offline(OfflineWhy::TurnedOff)), LineTone::Bad);
    }
}
