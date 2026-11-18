//! (the port's own: the beta has no chat) The custom game lobby's chat
//! (`NetLobby::chat`), under the player cards and down to the lobby's buttons: a heading, the log (each message as its
//! sender's name and the text, a note when a human joins or leaves) and the input, which Enter opens and Enter sends.
//! Built of the menu's own widgets in the lobby's tree, so the drop menus and the modals over the lobby cover it.
//!
//! The chat is a value of the custom game page, which routes the input's events to `handle` (the ids below) and
//! sends the committed text through the lobby.

use std::any::Any;

use serde_json::{Map, Value, json};

use super::controls::{find, wd};
use super::dialogs::TextInput;
use crate::bundle::{Rgba, WData};
use crate::menu::controller::{Cx, Handler};
use crate::menu::custom_text;
use crate::menu::lobby::net::{CHAT_INPUT_LIMIT, ChatKind, ChatLine};
use crate::menu::scroll::{scroll_offset, set_scroll_offset};
use crate::menu::view::MenuView;
use crate::slate::{Vis, WidgetId};

/// The handler ids the page forwards to `LobbyChat::handle`.
pub const H_COMMITTED: u32 = 0x200;
pub const H_ESCAPE: u32 = 0x201;
pub const H_FOCUSED: u32 = 0x202;
pub const H_UNFOCUSED: u32 = 0x203;
pub const H_CHANGED: u32 = 0x204;

/// An sRGB colour as the widgets' linear one.
fn srgb(hex: u32, a: f64) -> Rgba {
    let lin = |v: f64| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    let c = |s: u32| lin(f64::from((hex >> s) & 255) / 255.0);
    [c(16), c(8), c(0), a]
}

fn specified(c: Rgba) -> Value {
    json!({ "SpecifiedColor": { "R": c[0], "G": c[1], "B": c[2], "A": c[3] } })
}

fn linear(c: Rgba) -> Value {
    json!({ "R": c[0], "G": c[1], "B": c[2], "A": c[3] })
}

/// A plain widget (`new Widget(type, name, props, slotProps, slot)`).
pub fn plain(cx: &mut Cx<'_>, t: &str, name: &str, p: Value, s: Value, slot: &str) -> WidgetId {
    let n = WData { t: t.to_owned(), n: name.to_owned(), p, st: slot.to_owned(), s, ..WData::default() };
    let bundle = cx.view.bundle.clone();
    cx.view.tree_mut().create_node(&bundle, &n)
}

fn font(face: &str, size: f64) -> Value {
    json!({ "FontObject": "PFG", "TypefaceFontName": face, "Size": size })
}

#[allow(clippy::too_many_arguments)] // one TextBlock of the chat: name, text, font, colour, props and slot.
fn text(
    cx: &mut Cx<'_>,
    name: &str,
    t: &str,
    face: &str,
    size: f64,
    color: Rgba,
    extra: Value,
    slot: Value,
) -> WidgetId {
    let mut p = Map::new();
    p.insert("Text".into(), json!({ "CultureInvariantString": t }));
    p.insert("Font".into(), font(face, size));
    p.insert("ColorAndOpacity".into(), specified(color));
    if let Value::Object(e) = extra {
        p.extend(e);
    }
    plain(cx, "TextBlock", name, Value::Object(p), slot, "HorizontalBoxSlot")
}

fn fill() -> Value {
    json!({ "Size": { "SizeRule": "ESlateSizeRule::Fill" } })
}

/// What the page does after the chat's event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatAction {
    None,
    /// The input committed this text with Enter: the page sends it (`sent`, or `refused`).
    Send(String),
}

#[derive(Debug)]
pub struct LobbyChat {
    pub root: WidgetId,
    scroll: WidgetId,
    lines: WidgetId,
    input: WidgetId,
    /// The input's EditableTextBox and its text: read straight from the tree, not through the input's controller,
    /// which is busy while its own events reach the page (the focus, the commit).
    text_box: WidgetId,
    text: Option<WidgetId>,
    hint: WidgetId,
    shake_left: f64,
    /// The log follows its newest line while it is scrolled to the end; scrolled up, it stays where it was put.
    stuck: bool,
    last_max: f64,
    /// A message went out: the input takes the keys again at the next tick (its controller is busy while its own
    /// commit reaches the page), so several lines go out in a row without Enter between them.
    reopen: bool,
}

impl LobbyChat {
    /// The chat over the lobby's log so far; its input's events go to the page (`cx.w`) under the `H_*` ids.
    #[allow(clippy::too_many_lines)] // the constructor: one tree built in order.
    pub fn new(cx: &mut Cx<'_>, log: &[ChatLine]) -> LobbyChat {
        let text_c = srgb(0xe5f8de, 1.0); // the cards' name colour (WBP_CustomGamePlayer PlayerTitle)
        let note_c = srgb(0x9aa3c0, 1.0);
        let log_bg = srgb(0x0b0e1f, 0.55);
        let input_bg = srgb(0x0b0e1f, 0.8);
        let root = plain(
            cx,
            "VerticalBox",
            "LobbyChat",
            json!({}),
            json!({ "Size": { "SizeRule": "ESlateSizeRule::Fill" }, "Padding": { "Top": 10, "Bottom": 24 } }),
            "VerticalBoxSlot",
        );
        let title = plain(
            cx,
            "TextBlock",
            "LobbyChatTitle",
            json!({
                "Text": { "CultureInvariantString": custom_text::CHAT_LOBBY_TITLE },
                "Font": font("Display 1", 30.0),
                "ColorAndOpacity": specified(text_c),
                "TextTransformPolicy": "ETextTransformPolicy::ToUpper",
            }),
            json!({ "Padding": { "Bottom": 8 } }),
            "VerticalBoxSlot",
        );
        cx.view.tree_mut().add(root, title);
        let body = plain(
            cx,
            "Border",
            "LobbyChatLog",
            json!({ "Background": {}, "BrushColor": linear(log_bg) }),
            fill(),
            "VerticalBoxSlot",
        );
        let scroll = plain(
            cx,
            "ScrollBox",
            "LobbyChatScroll",
            json!({ "ScrollBarVisibility": "ESlateVisibility::Collapsed" }),
            json!({ "Padding": { "Left": 20, "Top": 12, "Right": 20, "Bottom": 12 } }),
            "BorderSlot",
        );
        let lines = plain(cx, "VerticalBox", "LobbyChatLines", json!({}), json!({}), "ScrollBoxSlot");
        cx.view.tree_mut().add(scroll, lines);
        cx.view.tree_mut().add(body, scroll);
        cx.view.tree_mut().add(root, body);
        let row =
            plain(cx, "Overlay", "LobbyChatInputRow", json!({}), json!({ "Padding": { "Top": 8 } }), "VerticalBoxSlot");
        let input = MenuView::create(cx, "UI_TextInput");
        {
            let w = wd(cx, input);
            w.h_align = crate::slate::HAlign::Fill;
            w.v_align = crate::slate::VAlign::Fill;
        }
        cx.view.tree_mut().set_prop(input, "CharacterLimit", Value::from(CHAT_INPUT_LIMIT));
        let tb = cx.view.tree().find_in(input, "TextBox");
        wd(cx, tb).brush_color = input_bg;
        let text_w = cx.view.tree().widget(tb).children.first().copied();
        if let Some(c) = text_w {
            wd(cx, c).color = text_c;
        }
        cx.view.tree_mut().add(row, input);
        let hint = plain(
            cx,
            "TextBlock",
            "LobbyChatHint",
            json!({
                "Text": { "CultureInvariantString": custom_text::CHAT_LOBBY_HINT },
                "Font": font("NormsCond-Medium", 24.0),
                "ColorAndOpacity": specified(note_c),
                "Visibility": "ESlateVisibility::HitTestInvisible",
            }),
            json!({ "Padding": { "Left": 18 }, "VerticalAlignment": "VAlign_Center" }),
            "OverlaySlot",
        );
        cx.view.tree_mut().add(row, hint);
        cx.view.tree_mut().add(root, row);

        let me = cx.w;
        cx.visit::<TextInput, _>(input, |t, _| {
            t.on_text_committed.add(Handler { owner: me, id: H_COMMITTED });
            t.on_escape.add(Handler { owner: me, id: H_ESCAPE });
            t.on_text_focused.add(Handler { owner: me, id: H_FOCUSED });
            t.on_text_unfocused.add(Handler { owner: me, id: H_UNFOCUSED });
            t.on_text_changed.add(Handler { owner: me, id: H_CHANGED });
        });
        let chat = LobbyChat {
            root,
            scroll,
            lines,
            input,
            text_box: tb,
            text: text_w,
            hint,
            shake_left: 0.0,
            stuck: true,
            last_max: 0.0,
            reopen: false,
        };
        for l in log {
            let w = chat.line(cx, l);
            cx.view.tree_mut().add(lines, w);
        }
        cx.view.manager.register_scrollbox(scroll);
        chat
    }

    /// The input's interactor, for the lobby's context.
    pub fn interactor(&self, cx: &Cx<'_>) -> WidgetId {
        TextInput::interactor(cx, self.input)
    }

    /// Enter: the input takes the keys.
    pub fn open(&self, cx: &mut Cx<'_>) {
        cx.visit::<TextInput, _>(self.input, |t, cx| t.force_start_input(cx));
        self.show_hint(cx);
    }

    pub fn typing(&self, cx: &Cx<'_>) -> bool {
        cx.view.has_keyboard_focus(self.text_box)
    }

    fn text_is_empty(&self, cx: &Cx<'_>) -> bool {
        self.text.is_none_or(|t| cx.view.tree().widget(t).text.is_empty())
    }

    /// Each frame: the log kept at its newest line (unless scrolled up), the shake of a message sent too fast.
    pub fn tick(&mut self, cx: &mut Cx<'_>, dt: f64) {
        if std::mem::take(&mut self.reopen) {
            self.open(cx);
        }
        let max = cx.view.tree().widget(self.scroll).scroll_max;
        let at_end = scroll_offset(cx.view.tree_mut(), self.scroll) >= max - 2.0;
        // (an offset off the end while the content kept its height: the wheel or the bar moved it)
        if self.stuck && !at_end && max == self.last_max {
            self.stuck = false;
        } else if at_end {
            self.stuck = true;
        }
        if self.stuck {
            set_scroll_offset(cx.view.tree_mut(), self.scroll, max);
        }
        self.last_max = max;
        // Deviation: the input stays as bright as UI_TextInput shows it focused (opacity 1): its unfocus event dims
        // the box to 0.5 (the build's), the port's chat input is lit whether hovered or not.
        let content = cx.view.tree().find_in(self.input, "mainContent");
        wd(cx, content).opacity = 1.0;
        if self.shake_left <= 0.0 {
            return;
        }
        self.shake_left = (self.shake_left - dt).max(0.0);
        let x = if self.shake_left == 0.0 { 0.0 } else { (self.shake_left * 80.0).sin() * 8.0 };
        wd(cx, self.input).transform.translation[0] = x;
    }

    pub fn destroy(&self, cx: &mut Cx<'_>) {
        cx.view.manager.unregister_scrollbox(self.scroll);
    }

    fn show_hint(&self, cx: &mut Cx<'_>) {
        let show = !self.typing(cx) && self.text_is_empty(cx);
        wd(cx, self.hint).vis = if show { Vis::HitTestInvisible } else { Vis::Collapsed };
    }

    /// The page's answer to a committed text: shown as sent (the input emptied) or refused for its speed.
    /// (the port's own) A line that went out keeps the input open for the next (`reopen`); Enter on an empty input
    /// closes it as before. The match's chat (mvs-app match_chat.rs) still closes after each line.
    pub fn sent(&mut self, cx: &mut Cx<'_>) {
        self.reopen = !self.text_is_empty(cx);
        if let Some(t) = self.text {
            wd(cx, t).text.clear();
        }
        self.stuck = true;
        self.show_hint(cx);
    }

    pub fn shake(&mut self) {
        self.shake_left = 0.25;
    }

    /// A new line of the lobby's log; the oldest go when the log is shorter than the widget's lines.
    pub fn add(&mut self, cx: &mut Cx<'_>, l: &ChatLine, kept: usize) {
        let w = self.line(cx, l);
        MenuView::add_child(cx, self.lines, w, None);
        while cx.view.tree().widget(self.lines).children.len() > kept {
            let first = cx.view.tree().widget(self.lines).children[0];
            MenuView::discard(cx, first);
        }
    }

    fn line(&self, cx: &mut Cx<'_>, l: &ChatLine) -> WidgetId {
        let text_c = srgb(0xe5f8de, 1.0);
        let me_c = srgb(0xffd200, 1.0);
        let other_c = srgb(0x4db8ff, 1.0);
        let note_c = srgb(0x9aa3c0, 1.0);
        let row = plain(
            cx,
            "HorizontalBox",
            "LobbyChatLine",
            json!({}),
            json!({ "Padding": { "Bottom": 4 } }),
            "VerticalBoxSlot",
        );
        if l.to != ChatKind::Lobby {
            let what = if l.to == ChatKind::Joined { custom_text::CHAT_JOINED } else { custom_text::CHAT_LEFT };
            let t = text(
                cx,
                "Note",
                &format!("{} {what}", l.name),
                "NormsCond-Medium",
                24.0,
                note_c,
                json!({ "AutoWrapText": true }),
                fill(),
            );
            cx.view.tree_mut().add(row, t);
            return row;
        }
        let name = text(
            cx,
            "Name",
            &format!("{}:", l.name),
            "NormsCond-ExtraBoldItalic",
            24.0,
            if l.mine { me_c } else { other_c },
            json!({}),
            json!({ "Padding": { "Right": 8 } }),
        );
        cx.view.tree_mut().add(row, name);
        let t = text(cx, "Text", &l.text, "NormsCond-Medium", 24.0, text_c, json!({ "AutoWrapText": true }), fill());
        cx.view.tree_mut().add(row, t);
        row
    }

    /// The input's events (the page forwards `H_*`).
    pub fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) -> ChatAction {
        match id {
            H_COMMITTED => {
                if let Some((t, m)) = arg.downcast_ref::<(String, u8)>()
                    && *m == 1
                {
                    return ChatAction::Send(t.clone());
                }
            }
            // ui.clearKeyboardFocus(2)
            H_ESCAPE => MenuView::clear_keyboard_focus(cx, 2, true),
            H_FOCUSED | H_UNFOCUSED | H_CHANGED => self.show_hint(cx),
            _ => {}
        }
        ChatAction::None
    }
}

/// The widget with `name` under the lobby page (for the page's `add_chat`).
pub fn page_find(cx: &Cx<'_>, name: &str) -> WidgetId {
    find(cx, name)
}
