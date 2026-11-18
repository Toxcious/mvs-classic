//! UI_TextInput, UI_SetUsername, UI_Spinner, UI_Waiting, UI_Dialog and UI_Notification.

use std::any::Any;

use serde_json::{Value, json};

use super::common::{cover_page, enum_index, fill_page, rgba};
use super::controls::background_blur::set_blur_strength;
use super::controls::buttons::{ButtonCartoon, ButtonMain, ButtonSecondary, IconButton};
use super::controls::interactor::{InteractorState, batch_set_context, fire_unfocus, set_focused, set_interactable};
use super::controls::{bind, find, prop, prop_bool, prop_f64, quad_color, wd};
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Event, Handler, TextEdit};
use crate::menu::manager::{
    Dialog, DialogId, DialogParams, Manager, Modal, NULL_GUID, Notification, NotificationParams, Waiting,
};
use crate::menu::online::OnlineCmd;
use crate::menu::view::MenuView;
use crate::menu::widgets::social::{SocialPlayer, SocialPlayerData};
use crate::slate::{HAlign, VAlign, Vis, WidgetId};

/// The interactor of the button / interactor child `name` of the controller's widget.
fn interactor_in(cx: &Cx<'_>, name: &str) -> WidgetId {
    let w = find(cx, name);
    cx.view.tree().find_opt(w, "UI_Interactor").unwrap_or(w)
}

// ---------- UI_TextInput ----------

const T_CANCEL: u32 = 1;
const T_INTERACTOR_FOCUSED: u32 = 2;
const T_CLICK: u32 = 3;
const T_FOCUS: u32 = 4;
const T_UNFOCUS: u32 = 5;

/// The PlayStation box (TextBoxSony) stays collapsed.
/// Deviation: FocusCheck (the quarter-second check that ends the editing when the game loses the window's focus) only
/// tests the box's visibility: the window's focus is not known to the menu.
#[derive(Debug, Default)]
pub struct TextInput {
    pub on_text_changed: Event<String>,
    /// (text, ETextCommit).
    pub on_text_committed: Event<(String, u8)>,
    pub on_text_focused: Event<()>,
    pub on_text_unfocused: Event<()>,
    /// port: the Escape key while the box has the keyboard focus (the build's box does nothing on it).
    pub on_escape: Event<()>,
    editing_change_time: f64,
    /// Time to the next OnQuarterSecond while the check is bound; < 0: not bound.
    focus_check: f64,
}

impl TextInput {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(TextInput { focus_check: -1.0, ..TextInput::default() })
    }

    fn text_box(cx: &Cx<'_>) -> WidgetId {
        find(cx, "TextBox")
    }

    fn text_widget(cx: &Cx<'_>) -> Option<WidgetId> {
        cx.view.tree().widget(Self::text_box(cx)).children.first().copied()
    }

    pub fn interactor(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
        cx.view.tree().find_in(w, "UI_Interactor")
    }

    pub fn get_text(cx: &Cx<'_>) -> String {
        Self::text_widget(cx).map(|t| cx.view.tree().widget(t).text.clone()).unwrap_or_default()
    }

    pub fn set_text(cx: &mut Cx<'_>, t: &str) {
        if let Some(w) = Self::text_widget(cx) {
            wd(cx, w).text = t.to_string();
        }
    }

    pub fn has_keyboard_focus(cx: &Cx<'_>) -> bool {
        cx.view.has_keyboard_focus(Self::text_box(cx))
    }

    /// ForceStartInput.
    pub fn force_start_input(&mut self, cx: &mut Cx<'_>) {
        self.toggle_editing(cx, true);
        MenuView::set_keyboard_focus(cx, Self::text_box(cx), cx.w);
    }

    pub fn toggle_editing(&mut self, cx: &mut Cx<'_>, editing: bool) {
        self.editing_change_time = cx.view.now();
        cx.view.manager.text_entry_mode = editing;
        self.focus_check = if editing { 0.25 } else { -1.0 };
    }

    fn clamp(cx: &Cx<'_>, t: &str) -> (String, bool) {
        let limit = prop_f64(cx, "CharacterLimit", 50.0) as usize;
        if t.chars().count() <= limit { (t.to_string(), false) } else { (t.chars().take(limit).collect(), true) }
    }

    /// HandleTextChanged (after AdjustText, which only filters for NumbersOnly). A change within 0.1 s of the editing
    /// toggle is the key that toggled it: the text loses its end (RemoveLastCharacter takes two characters off a
    /// text longer than one).
    fn changed(&mut self, cx: &mut Cx<'_>, raw: &str) {
        let t: String = if prop_bool(cx, "NumbersOnly") {
            raw.chars().filter(char::is_ascii_digit).collect()
        } else {
            raw.to_string()
        };
        let (c, too_long) = Self::clamp(cx, &t);
        if cx.view.now() - self.editing_change_time > 0.1 {
            if too_long {
                cx.post("ui_prematch_perk_block_error");
            }
            cx.emit(self, |x| &x.on_text_changed, &c);
            Self::set_text(cx, &c);
        } else {
            let n = c.chars().count();
            Self::set_text(cx, &if n <= 1 { String::new() } else { c.chars().take(n - 2).collect() });
        }
    }

    /// HandleTextCommitted: ignored within 0.1 s of the editing toggle. Text entry mode ends on a default or
    /// moved-focus commit, and on Enter or a cleared focus when bReleaseFocusOnEnterCommit (else the input starts
    /// again).
    fn committed(&mut self, cx: &mut Cx<'_>, t: &str, method: u8) {
        if cx.view.now() - self.editing_change_time <= 0.1 {
            return;
        }
        let c = Self::clamp(cx, t).0;
        cx.emit(self, |x| &x.on_text_committed, &(c, method));
        if method == 0 || method == 2 || prop_bool(cx, "bReleaseFocusOnEnterCommit") {
            cx.view.manager.text_entry_mode = false;
        } else {
            self.force_start_input(cx);
        }
    }
}

impl Controller for TextInput {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        // OnCancel -> OnUICancel, OnInteractorFocused: both end the editing while the box has the keyboard focus
        cx.view.manager.on_cancel.add(Handler { owner: me, id: T_CANCEL });
        cx.view.manager.on_interactor_focused.add(Handler { owner: me, id: T_INTERACTOR_FOCUSED });
        let i = interactor_in_self(cx);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_click, T_CLICK);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, T_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, T_UNFOCUS);
    }

    /// Hint Text, Is Password, Read Only and Justification go to the boxes (none is set on the instances of this
    /// bundle; a hint is not drawn), then the interactor's OnUnfocus is broadcast.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let i = interactor_in_self(cx);
        fire_unfocus(cx, i, 0);
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        self.toggle_editing(cx, false);
        let me = cx.w;
        cx.view.manager.on_cancel.remove(Handler { owner: me, id: T_CANCEL });
        cx.view.manager.on_interactor_focused.remove(Handler { owner: me, id: T_INTERACTOR_FOCUSED });
    }

    /// FocusCheck on the manager's OnQuarterSecond while editing: the editing ends when the box has the keyboard
    /// focus and is not visible.
    fn timers(&mut self, cx: &mut Cx<'_>, dt: f64) {
        if self.focus_check < 0.0 {
            return;
        }
        self.focus_check -= dt;
        if self.focus_check > 0.0 {
            return;
        }
        self.focus_check += 0.25;
        if Self::has_keyboard_focus(cx) && !cx.view.visible_in_hierarchy(cx.w) {
            self.toggle_editing(cx, false);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let mid = find(cx, "mainContent");
        match id {
            T_CANCEL => {
                if Self::has_keyboard_focus(cx) {
                    self.toggle_editing(cx, false);
                }
            }
            T_INTERACTOR_FOCUSED => {
                let other = arg.downcast_ref::<WidgetId>().copied();
                if Self::has_keyboard_focus(cx) && other != Some(interactor_in_self(cx)) {
                    self.toggle_editing(cx, false);
                }
            }
            T_CLICK => {
                self.toggle_editing(cx, true);
                MenuView::set_keyboard_focus(cx, Self::text_box(cx), cx.w);
            }
            T_FOCUS => wd(cx, mid).opacity = 1.0,
            T_UNFOCUS => wd(cx, mid).opacity = 0.5,
            _ => {}
        }
    }

    fn text_edit(&mut self, cx: &mut Cx<'_>, _edit_box: WidgetId, ev: TextEdit) {
        match ev {
            TextEdit::Changed(t) => self.changed(cx, &t),
            TextEdit::Committed(t, m) => self.committed(cx, &t, m),
            TextEdit::Focused(true) => cx.emit(self, |x| &x.on_text_focused, &()),
            TextEdit::Focused(false) => cx.emit(self, |x| &x.on_text_unfocused, &()),
            TextEdit::Escape => cx.emit(self, |x| &x.on_escape, &()),
        }
    }
}

fn interactor_in_self(cx: &Cx<'_>) -> WidgetId {
    find(cx, "UI_Interactor")
}

// ---------- UI_SetUsername (modal chooseUsername) ----------

/// The answer of the name service to a name (ClientHydraManager.UpdateUsername's OnSuccess / OnFailed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsernameAnswer {
    Ok,
    Taken,
    Refused,
}

/// A name is at most this many characters.
const NAME_LIMIT: f64 = 20.0;
/// The longest lobby code the join box takes (an invitation's code of up to 64 characters).
const CODE_LIMIT: f64 = 64.0;

fn srgb(hex: u32) -> Rgba {
    let lin = |v: f64| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    let c = |s: u32| lin(f64::from((hex >> s) & 255) / 255.0);
    [c(16), c(8), c(0), 1.0]
}

fn linear(c: Rgba) -> Value {
    json!({"R": c[0], "G": c[1], "B": c[2], "A": c[3]})
}

/// Deviation: the line under the box for a name with a word it may not hold. The wording is the
/// port's; the tree's line ("Username unavailable. Try a different one.") stays for a name that is taken.
const NAME_REFUSED_TEXT: &str = "That name is not allowed. Try a different one.";
const NAME_BOX_UNFOCUSED: f64 = 0.8;

const U_GO: u32 = 1;
const U_COMMITTED: u32 = 2;
const U_ESCAPE: u32 = 3;
const U_UNFOCUS: u32 = 4;
const U_FOCUS_INPUT: u32 = 5;
const U_DISMISS: u32 = 6;

/// Deviation: the window's colours are the port's choice among the build's own menu colours, not the tree's (a slate
/// panel #121928, a pale box #E5FFEA, the orange of the main buttons). Layout, fonts, texts and logic stay the tree's.
/// Deviation: shown with the extra data "change" (the name was clicked) the input starts with the current name and a
/// cancel closes the modal; the build's modal is only shown while there is no name and cannot be cancelled.
/// Deviation: shown with the extra data "join" it is the box a lobby's code is pasted in (the PLAY screen's Join
/// Lobby card): the title is the instructions (the card's own), the input starts empty, a cancel closes it, and GO
/// hands the code to `code_entered`. The build has no such box (its Join Lobby card is off).
/// Deviation: the name service (identity.updateUsername, a request over the network) is not in this crate: GO fires
/// `on_name_submitted(name)` and the busy look until the owner calls `answered`; the name the input starts with in
/// "change" and "taken" is the instance's `CurrentName` property, written by the owner.
#[derive(Debug)]
pub struct SetUsername {
    pub code_entered: Event<String>,
    pub on_name_submitted: Event<String>,
    modal_id: String,
    context: String,
    change: bool,
    join: bool,
    /// The tree's text of UsernameTakenText.
    taken_text: String,
    /// The name asked of the online client (identity.updateUsername), until its answer.
    asked: Option<String>,
}

impl SetUsername {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(SetUsername {
            code_entered: Event::default(),
            on_name_submitted: Event::default(),
            modal_id: String::new(),
            context: NULL_GUID.to_string(),
            change: false,
            join: false,
            taken_text: String::new(),
            asked: None,
        })
    }

    /// Port: the colours (see the type). The button's label colours are its properties, read at each focus change.
    fn recolour(cx: &mut Cx<'_>) {
        let panel = srgb(0x121928);
        let text_c = srgb(0xf3fcf0);
        let boxc = srgb(0xe5ffea);
        let button: Rgba = [1.0, 0.223228, 0.002428, 1.0];
        let b = find(cx, "Border_56");
        wd(cx, b).brush_color = panel;
        let t = find(cx, "Title");
        wd(cx, t).color = text_c;
        let input = find(cx, "inputUsername");
        let tb = cx.view.tree().find_in(input, "TextBox");
        wd(cx, tb).brush_color = boxc;
        if let Some(&c) = cx.view.tree().widget(tb).children.first() {
            wd(cx, c).color = panel;
        }
        let go = find(cx, "buttonGO");
        for (k, c) in [
            ("FocusedColor", button),
            ("UnFocusedColor", button),
            ("FocusedLabelColor", text_c),
            ("UnFocusedLabelColor", text_c),
        ] {
            cx.view.tree_mut().set_prop(go, k, linear(c));
        }
        for (n, c) in [("IdleQuad", button), ("SelectedQuad", boxc), ("ClickQuad", boxc), ("label", text_c)] {
            let q = cx.view.tree().find_in(go, n);
            wd(cx, q).color = c;
        }
    }

    fn input(cx: &Cx<'_>) -> WidgetId {
        find(cx, "inputUsername")
    }

    fn busy(cx: &mut Cx<'_>, on: bool) {
        let (l, b) = (find(cx, "LoadingIndicator"), find(cx, "BtnContainer"));
        wd(cx, l).vis = if on { Vis::Visible } else { Vis::Collapsed };
        wd(cx, b).vis = if on { Vis::Hidden } else { Vis::Visible };
    }

    /// OnFailed's line, with the port's text for a refused name.
    fn failed(&self, cx: &mut Cx<'_>, refused: bool) {
        let l = find(cx, "UsernameTakenText");
        let x = wd(cx, l);
        x.text = if refused { NAME_REFUSED_TEXT.to_string() } else { self.taken_text.clone() };
        x.vis = Vis::Visible;
    }

    fn set_input_limit(cx: &mut Cx<'_>, limit: f64) {
        let i = Self::input(cx);
        cx.view.tree_mut().set_prop(i, "CharacterLimit", Value::from(limit));
    }

    /// GO pressed (the menu tape driver's hook).
    #[doc(hidden)]
    pub fn press_go(&mut self, cx: &mut Cx<'_>) {
        self.submit(cx);
    }

    /// ClientHydraManager.UpdateUsername(text, OnSuccess, OnFailed): the owner answers through `answered`.
    fn submit(&mut self, cx: &mut Cx<'_>) {
        let input = Self::input(cx);
        let t = cx.visit::<TextInput, _>(input, |_, cx| TextInput::get_text(cx)).unwrap_or_default();
        if self.join {
            let code = t.trim().to_string();
            if code.is_empty() {
                return;
            }
            let id = self.modal_id.clone();
            Manager::dismiss_modal(cx, &id);
            cx.emit(self, |c| &c.code_entered, &code);
            return;
        }
        // identity.updateUsername: the name is the text without the spaces around it, at most NAME_LIMIT UTF-16
        // units, and not empty (an empty one is answered 'taken' at once); the host core checks the rest, its answer
        // a later frame's view
        let t = utf16_head(t.trim(), mvs_protocol::names::NAME_LIMIT);
        if t.is_empty() {
            self.answered(cx, UsernameAnswer::Taken);
            return;
        }
        Self::busy(cx, true);
        self.asked = Some(t.clone());
        cx.online(OnlineCmd::SetName(t.clone()));
        cx.emit(self, |c| &c.on_name_submitted, &t);
    }

    /// The name service's answer: OnFailed shows the line, OnSuccess dismisses 0.1 s later (the game instance's
    /// OnUsernameChanged has fired).
    pub fn answered(&mut self, cx: &mut Cx<'_>, a: UsernameAnswer) {
        if a != UsernameAnswer::Ok {
            self.failed(cx, a == UsernameAnswer::Refused);
            Self::busy(cx, false);
            return;
        }
        let me = cx.w;
        cx.view.delay(0.1, Handler { owner: me, id: U_DISMISS });
    }
}

/// `text.slice(0, n)`: the first `n` UTF-16 units of `text` (a pair cut in two is dropped).
fn utf16_head(text: &str, n: usize) -> String {
    let mut units = 0;
    text.chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= n
        })
        .collect()
}

impl Modal for SetUsername {
    fn pre_show(&mut self, _cx: &mut Cx<'_>, id: &str) {
        self.modal_id = id.to_string();
    }

    fn post_show(&mut self, cx: &mut Cx<'_>, instructions: &str, extra_data: &str) {
        self.join = extra_data == "join";
        self.change = self.join || extra_data == "change";
        let input = Self::input(cx);
        if self.join {
            let t = find(cx, "Title");
            wd(cx, t).text = instructions.to_string();
            Self::set_input_limit(cx, CODE_LIMIT);
            cx.visit::<TextInput, _>(input, |_, cx| TextInput::set_text(cx, ""));
        } else if self.change || extra_data == "taken" || extra_data == "refused" {
            let mut name = prop(cx, "CurrentName").as_str().unwrap_or_default().to_string();
            if name.is_empty() {
                name = cx.view.session.online.name.clone();
            }
            cx.visit::<TextInput, _>(input, |_, cx| TextInput::set_text(cx, &name));
        }
        if extra_data == "taken" || extra_data == "refused" {
            self.failed(cx, extra_data == "refused");
        }
        // UpdateContext
        self.context = cx.view.manager.new_guid();
        let list = [TextInput::interactor(cx, input), ButtonSecondary::interactor(cx, find(cx, "buttonGO"))];
        batch_set_context(cx, &list, &self.context);
        let me = cx.w;
        cx.view.delay(0.1, Handler { owner: me, id: U_FOCUS_INPUT });
    }

    fn pre_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn post_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, from_cancel: bool) {
        if from_cancel && !self.change {
            return;
        }
        let id = self.modal_id.clone();
        Manager::dismiss_modal(cx, &id);
    }

    fn get_modal_context(&self) -> String {
        self.context.clone()
    }
}

impl Controller for SetUsername {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let go = find(cx, "buttonGO");
        cx.visit::<ButtonCartoon, _>(go, |b, _| b.on_click.add(Handler { owner: me, id: U_GO }));
        let input = Self::input(cx);
        cx.visit::<TextInput, _>(input, |t, _| {
            t.on_text_committed.add(Handler { owner: me, id: U_COMMITTED });
            t.on_escape.add(Handler { owner: me, id: U_ESCAPE });
        });
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        Self::recolour(cx);
        // Deviation: a name has NAME_LIMIT characters, not the 50 of UI_TextInput's default.
        Self::set_input_limit(cx, NAME_LIMIT);
        let t = find(cx, "UsernameTakenText");
        self.taken_text = cx.view.tree().widget(t).text.clone();
        // Port, with the colours: the box without the focus is at 0.8 of its look, not UI_TextInput's 0.5 (half of a
        // pale box over the dark panel is grey). Added after the input's own handler, so it has the last word.
        let input = Self::input(cx);
        let me = cx.w;
        let i = TextInput::interactor(cx, input);
        if let Some(s) = cx.view.manager.interactor_mut(i) {
            s.on_unfocus.add(Handler { owner: me, id: U_UNFOCUS });
        }
        let mc = cx.view.tree().find_in(input, "mainContent");
        wd(cx, mc).opacity = NAME_BOX_UNFOCUSED;
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let Some(asked) = self.asked.clone() else { return };
        let (taken, refused, now) = {
            let o = &cx.view.session.online;
            // (a rename's refusal is this frame's event alone: the flags hold only the session's own name refused)
            let told = o.events.iter().find_map(|e| match e {
                crate::menu::online::OnlineEvent::NameTaken { refused } => Some(*refused),
                _ => None,
            });
            (o.name_taken || told.is_some(), o.name_refused || told == Some(true), o.name.clone())
        };
        let answer = if refused {
            UsernameAnswer::Refused
        } else if taken {
            UsernameAnswer::Taken
        } else if now == asked {
            UsernameAnswer::Ok
        } else {
            return;
        };
        self.asked = None;
        self.answered(cx, answer);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            U_GO => self.submit(cx),
            U_COMMITTED => {
                if arg.downcast_ref::<(String, u8)>().is_some_and(|a| a.1 == 1) {
                    self.submit(cx);
                }
            }
            U_ESCAPE => {
                if self.change {
                    let id = self.modal_id.clone();
                    Manager::dismiss_modal(cx, &id);
                }
            }
            U_UNFOCUS => {
                let mc = cx.view.tree().find_in(Self::input(cx), "mainContent");
                wd(cx, mc).opacity = NAME_BOX_UNFOCUSED;
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            U_FOCUS_INPUT => {
                let i = TextInput::interactor(cx, Self::input(cx));
                set_focused(cx, i, false);
            }
            U_DISMISS => {
                self.request_dismiss(cx, "", false);
                Self::busy(cx, false);
            }
            _ => {}
        }
    }

    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }
}

// ---------- UI_Spinner ----------

/// PreConstruct: SetColor(Color), the icon's brush size = Desired Size. Tick: the icon turns 300 degrees a second
/// counter-clockwise.
#[derive(Debug, Default)]
pub struct Spinner;

impl Spinner {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(Spinner)
    }
}

impl Controller for Spinner {
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let icon = find(cx, "Icon");
        let c = rgba(&prop(cx, "Color"));
        let size = prop(cx, "Desired Size");
        let x = wd(cx, icon);
        x.color = c;
        x.brush_w = size.get("X").and_then(Value::as_f64).unwrap_or(0.0);
        x.brush_h = size.get("Y").and_then(Value::as_f64).unwrap_or(0.0);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, dt: f64) {
        let icon = find(cx, "Icon");
        wd(cx, icon).transform.angle -= dt * 300.0;
    }
}

// ---------- UI_Waiting ----------

/// The manager's waiting cover. Opaque picks the Flipbook page (the loading picture with a label), else the Cover page
/// (a 75% black fader with a spinner). The Flipbook page's FlipbookEmote (Brush 280 x 280, bottom right above the
/// label row) draws one of LoopingEmotes, picked at Construct; each is a SpriteSheet_MAT (SpriteSheet_Sequence_MAT for
/// 008_Mallet) instance whose slate shader steps its cells on View GameTime, the slate renderer's clock.
#[derive(Debug)]
pub struct WaitingCover {
    fade_start: f64,
    pub time: i32,
    /// (the port's own, `Manager::set_waiting_faded`) The whole cover fading: its start on the view's clock, its
    /// length, and in (true) or out.
    cover_fade: Option<(f64, f64, bool)>,
    /// A `set_waiting` has run: the cover the manager has just made shows its Blueprint's own visibility until then.
    set: bool,
}

impl WaitingCover {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(WaitingCover { fade_start: -1.0, time: 0, cover_fade: None, set: false })
    }

    /// The cover's own fade at the view's time (the port's own): its opacity, and its end when it went out.
    fn step_cover_fade(&mut self, cx: &mut Cx<'_>) {
        let Some((start, len, fade_in)) = self.cover_fade else { return };
        let a = ((cx.view.now() - start) / len).clamp(0.0, 1.0);
        let me = cx.w;
        wd(cx, me).opacity = if fade_in { a } else { 1.0 - a };
        if a >= 1.0 {
            self.cover_fade = None;
            if !fade_in {
                wd(cx, me).vis = Vis::Collapsed;
                wd(cx, me).opacity = 1.0;
            }
        }
    }

    /// FlipbookEmote.SetBrushFromMaterial(LoopingEmotes[RandomInteger(Length)]). UImage::SetBrushFromMaterial sets
    /// Brush.ResourceObject alone: the brush keeps its ImageSize. The pick is the page's random, not the match's.
    fn pick_emote(cx: &mut Cx<'_>) {
        let emotes: Vec<String> = prop(cx, "LoopingEmotes")
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        if emotes.is_empty() {
            return;
        }
        let i = ((cx.view.manager.random() * emotes.len() as f64) as usize).min(emotes.len() - 1);
        let w = find(cx, "FlipbookEmote");
        let x = wd(cx, w);
        x.tex = Some(emotes[i].clone());
        x.mid = None;
    }

    /// Deviation: the Flipbook page's picture (LoadingScreen_BG, ImageSize 1920 x 1080, under the ScaleBox Flipbook)
    /// covers a window of any shape, as the native loading screen's does, and the label row stays in the page's
    /// corner (cover_page).
    fn cover(cx: &mut Cx<'_>) {
        let bg = find(cx, "LoadingScreen_BG");
        cover_page(cx, bg, 1920.0, 1080.0);
    }

    /// UpdateTimer (no caller in the dumped Blueprints): the count goes down by one and shows while it is >= 0.
    pub fn update_timer(&mut self, cx: &mut Cx<'_>) {
        self.time -= 1;
        let t = find(cx, "Timer");
        let x = wd(cx, t);
        if self.time >= 0 {
            x.text = self.time.to_string();
        }
        x.vis = if self.time >= 0 { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
    }
}

impl Waiting for WaitingCover {
    fn set_waiting(&mut self, cx: &mut Cx<'_>, is_waiting: bool, label: &str, opaque: bool, countdown: f64) {
        // Deviation: the build makes a new UI_Waiting with each level's UI_Global (its Construct picks again); the
        // port keeps one, so it picks again each time the cover shows after being hidden.
        let me = cx.w;
        let fade = cx.view.manager.waiting_fade;
        let shown = self.set && wd(cx, me).vis == Vis::Visible;
        self.set = true;
        // (the port's own) a faded cover going out keeps its pages and label as they are until its fade's end
        if !is_waiting && fade > 0.0 {
            if shown {
                let from = self.cover_fade.map_or(1.0, |_| wd(cx, me).opacity);
                self.cover_fade = Some((cx.view.now() - (1.0 - from) * fade, fade, false));
            }
            return;
        }
        let fading_out = matches!(self.cover_fade, Some((_, _, false)));
        if is_waiting && (!shown || fading_out) {
            let from = if shown { wd(cx, me).opacity } else { 0.0 };
            self.cover_fade = (fade > 0.0).then(|| (cx.view.now() - from * fade, fade, true));
            wd(cx, me).opacity = if fade > 0.0 { from } else { 1.0 };
        }
        if is_waiting && !shown {
            Self::pick_emote(cx);
        }
        if !is_waiting {
            self.cover_fade = None;
            wd(cx, me).opacity = 1.0;
        }
        self.time = countdown as i32;
        let sw = find(cx, "WidgetSwitcher_0");
        wd(cx, sw).active = if opaque { 0 } else { 1 };
        let l = find(cx, "Label");
        wd(cx, l).text = label.to_string();
        wd(cx, me).vis = if is_waiting { Vis::Visible } else { Vis::Collapsed };
        // the label row fades in over 0.5 s of real time, from 0 at once
        self.fade_start = cx.view.now();
        let row = find(cx, "CanvasPanel_0");
        wd(cx, row).opacity = 0.0;
        Self::cover(cx);
    }
}

impl Controller for WaitingCover {
    fn construct(&mut self, cx: &mut Cx<'_>) {
        Self::pick_emote(cx);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        Self::cover(cx);
    }

    fn timers(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        self.step_cover_fade(cx);
        if self.fade_start < 0.0 {
            return;
        }
        let a = ((cx.view.now() - self.fade_start) / 0.5).clamp(0.0, 1.0);
        let row = find(cx, "CanvasPanel_0");
        wd(cx, row).opacity = a;
        if a >= 1.0 {
            self.fade_start = -1.0;
        }
    }

    fn as_waiting(&mut self) -> Option<&mut dyn Waiting> {
        Some(self)
    }
}

// ---------- UI_Dialog ----------

const DIALOG_FADE: f64 = 0.15;
const DIALOG_BLUR: f64 = 7.5;

const G_CANCEL: u32 = 1;
const G_TIMER: u32 = 2;
const G_LEFT: u32 = 3;
const G_MIDDLE: u32 = 4;
const G_RIGHT: u32 = 5;
const G_PROMINENT: u32 = 6;

#[derive(Clone, Copy, Debug)]
enum Button {
    Left,
    Middle,
    Right,
    Prominent,
    Dismissed,
}

/// A dialog of the manager's Dialog Stack: created by ShowDialog, put on the root canvas (ZOrder 50) when its Delayed
/// Init Timer fires. Its background blur follows the fade (set_blur_strength).
/// Deviation, not ported: the shop's currency buttons, the paused-game branches (no fade; the blur at 0.75, @1743).
#[derive(Debug)]
pub struct DialogBox {
    params: DialogParams,
    guid: String,
    is_on_screen: bool,
    is_exiting: bool,
    pub button_left_clicked: Event<()>,
    pub button_middle_clicked: Event<()>,
    pub button_right_clicked: Event<()>,
    pub button_prominent_clicked: Event<()>,
    pub dialog_dismissed: Event<()>,
    focus_timer: String,
    delayed_init_timer: String,
    /// (start, out).
    fade: Option<(f64, bool)>,
}

impl DialogBox {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(DialogBox {
            params: DialogParams::default(),
            guid: NULL_GUID.to_string(),
            is_on_screen: false,
            is_exiting: false,
            button_left_clicked: Event::default(),
            button_middle_clicked: Event::default(),
            button_right_clicked: Event::default(),
            button_prominent_clicked: Event::default(),
            dialog_dismissed: Event::default(),
            focus_timer: NULL_GUID.to_string(),
            delayed_init_timer: NULL_GUID.to_string(),
            fade: None,
        })
    }

    fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        ["buttonLeft", "buttonMiddle", "buttonRight", "buttonProminent"]
            .iter()
            .map(|n| interactor_in(cx, n))
            .chain([find(cx, "focusContext")])
            .collect()
    }

    fn clicked(&mut self, cx: &mut Cx<'_>, b: Button) {
        match b {
            Button::Left => cx.emit(self, |c| &c.button_left_clicked, &()),
            Button::Middle => cx.emit(self, |c| &c.button_middle_clicked, &()),
            Button::Right => cx.emit(self, |c| &c.button_right_clicked, &()),
            Button::Prominent => cx.emit(self, |c| &c.button_prominent_clicked, &()),
            Button::Dismissed => cx.emit(self, |c| &c.dialog_dismissed, &()),
        }
        self.exit_dialog(cx);
    }

    fn init(&mut self, cx: &mut Cx<'_>) {
        let p = self.params.clone();
        let t = find(cx, "titleTITLE");
        wd(cx, t).text = p.title.to_uppercase();
        let b = find(cx, "body");
        wd(cx, b).text = p.body.clone();
        for (n, l) in [
            ("buttonLeft", &p.button_left_label),
            ("buttonMiddle", &p.button_middle_label),
            ("buttonProminent", &p.button_prominent_label),
        ] {
            let w = find(cx, n);
            let l = l.to_uppercase();
            cx.visit::<ButtonMain, _>(w, |b, cx| b.set_label(cx, &l));
        }
        let w = find(cx, "buttonRight");
        let l = p.button_right_label.to_uppercase();
        cx.visit::<ButtonSecondary, _>(w, |b, cx| b.init(cx, &l));
        let show = |cx: &mut Cx<'_>, name: &str, on: bool| {
            let w = find(cx, name);
            wd(cx, w).vis = if on { Vis::Visible } else { Vis::Collapsed };
        };
        show(cx, "buttonContainerLeft", !p.button_left_label.is_empty());
        show(cx, "buttonLeft", true);
        show(cx, "buttonContainerMiddle", !p.button_middle_label.is_empty());
        show(cx, "buttonContainerRight", !p.button_right_label.is_empty());
        show(cx, "buttonRight", true);
        show(cx, "buttonContainerProminent", !p.button_prominent_label.is_empty());
        if p.button_left_label.is_empty()
            && p.button_middle_label.is_empty()
            && p.button_right_label.is_empty()
            && p.button_prominent_label.is_empty()
        {
            let w = find(cx, "buttonMiddle");
            cx.visit::<ButtonMain, _>(w, |b, cx| b.set_label(cx, "OK"));
            show(cx, "buttonContainerMiddle", true);
        }
        cx.view.manager.set_handle_input(false);
        Self::set_interactable_all(cx, false);
        let t = self.focus_timer.clone();
        self.focus_timer = Manager::add_timer(cx, &t, 0.25);
    }

    /// FocusButton: the first shown of prominent, left, right, middle.
    fn focus_button(&mut self, cx: &mut Cx<'_>) {
        Self::set_interactable_all(cx, true);
        for (container, button) in [
            ("buttonContainerProminent", "buttonProminent"),
            ("buttonContainerLeft", "buttonLeft"),
            ("buttonContainerRight", "buttonRight"),
            ("buttonContainerMiddle", "buttonMiddle"),
        ] {
            let c = find(cx, container);
            let vis = cx.view.tree().widget(c).vis;
            if vis == Vis::Collapsed || vis == Vis::Hidden {
                continue;
            }
            // SetFocusedInteractor
            Manager::unfocus_all(cx, 0);
            cx.view.manager.set_handle_input(true);
            let i = interactor_in(cx, button);
            set_focused(cx, i, false);
            return;
        }
    }

    fn set_interactable_all(cx: &mut Cx<'_>, on: bool) {
        for i in Self::interactors(cx) {
            set_interactable(cx, i, on);
        }
    }

    fn leave_stack(cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.dialog_stack.retain(|d| *d != DialogId(me));
    }

    fn destroy(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        MenuView::discard(cx, me);
        self.is_on_screen = false;
        let g = self.guid.clone();
        Manager::remove_context(cx, &g);
        cx.view.manager.on_cancel.remove(Handler { owner: me, id: G_CANCEL });
        cx.view.manager.on_timer_event.remove(Handler { owner: me, id: G_TIMER });
        Self::leave_stack(cx);
    }

    fn exit_dialog(&mut self, cx: &mut Cx<'_>) {
        self.is_exiting = true;
        Self::set_interactable_all(cx, false);
        if cx.view.is_constructed(cx.w) {
            self.fade = Some((cx.view.now(), true));
        } else {
            self.destroy(cx);
        }
    }
}

impl Dialog for DialogBox {
    fn params(&self) -> &DialogParams {
        &self.params
    }
    fn is_on_screen(&self) -> bool {
        self.is_on_screen
    }
    fn is_exiting(&self) -> bool {
        self.is_exiting
    }
    fn button_left_clicked(&mut self) -> &mut Event<()> {
        &mut self.button_left_clicked
    }
    fn button_middle_clicked(&mut self) -> &mut Event<()> {
        &mut self.button_middle_clicked
    }
    fn button_right_clicked(&mut self) -> &mut Event<()> {
        &mut self.button_right_clicked
    }
    fn button_prominent_clicked(&mut self) -> &mut Event<()> {
        &mut self.button_prominent_clicked
    }
    fn dialog_dismissed(&mut self) -> &mut Event<()> {
        &mut self.dialog_dismissed
    }

    /// DelayedInit: Init and AddWidgetToView run on the manager's timer.
    fn delayed_init(&mut self, cx: &mut Cx<'_>, params: DialogParams, delay: f64) {
        self.params = params;
        let t = self.delayed_init_timer.clone();
        self.delayed_init_timer = Manager::add_timer(cx, &t, delay);
    }

    fn exit(&mut self, cx: &mut Cx<'_>) {
        self.exit_dialog(cx);
    }
}

impl Controller for DialogBox {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        self.guid = cx.view.manager.new_guid();
        self.is_on_screen = true;
        // UpdateContext, then the dialog's context becomes the current one
        let list = Self::interactors(cx);
        batch_set_context(cx, &list, &self.guid);
        let fc = find(cx, "focusContext");
        set_focused(cx, fc, true);
        cx.view.manager.on_cancel.add(Handler { owner: me, id: G_CANCEL });
        cx.view.manager.on_timer_event.add(Handler { owner: me, id: G_TIMER });
        for (n, id) in [("buttonLeft", G_LEFT), ("buttonMiddle", G_MIDDLE), ("buttonProminent", G_PROMINENT)] {
            let w = find(cx, n);
            cx.visit::<ButtonMain, _>(w, |b, _| b.on_click.add(Handler { owner: me, id }));
        }
        let w = find(cx, "buttonRight");
        cx.visit::<ButtonSecondary, _>(w, |b, _| b.on_click.add(Handler { owner: me, id: G_RIGHT }));
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        cx.view.manager.set_handle_input(false);
        self.fade = Some((cx.view.now(), false));
        let c = find(cx, "Container");
        wd(cx, c).opacity = 0.0;
        let sb = find(cx, "SizeBox_0");
        fill_page(cx, sb);
    }

    /// Deviation: the dialog's 16:9 box follows the page, so its backing covers a window of any shape.
    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let sb = find(cx, "SizeBox_0");
        fill_page(cx, sb);
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        cx.view.manager.set_handle_input(true);
        Self::leave_stack(cx);
    }

    fn timers(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let Some((start, out)) = self.fade else { return };
        let a = ((cx.view.now() - start) / DIALOG_FADE).clamp(0.0, 1.0);
        let c = find(cx, "Container");
        wd(cx, c).opacity = if out { 1.0 - a } else { a };
        // BackgroundBlur_191.SetBlurStrength: Lerp(0, 7.5, a) @384-431, Lerp(7.5, 0, a) @1013-1060
        let b = find(cx, "BackgroundBlur_191");
        set_blur_strength(cx, b, DIALOG_BLUR * if out { 1.0 - a } else { a });
        if a < 1.0 {
            return;
        }
        self.fade = None;
        if out {
            self.destroy(cx);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            G_LEFT => self.clicked(cx, Button::Left),
            G_MIDDLE => self.clicked(cx, Button::Middle),
            G_RIGHT => self.clicked(cx, Button::Right),
            G_PROMINENT => self.clicked(cx, Button::Prominent),
            G_CANCEL => {
                let p = arg.downcast_ref::<(i32, bool)>().map_or(0, |a| a.0);
                if self.is_exiting || cx.view.manager.get_current_context(p) != self.guid {
                    return;
                }
                self.clicked(cx, Button::Dismissed);
            }
            G_TIMER => {
                let Some(t) = arg.downcast_ref::<String>() else { return };
                if *t == self.focus_timer {
                    self.focus_button(cx);
                } else if *t == self.delayed_init_timer {
                    self.init(cx);
                    let me = cx.w;
                    MenuView::add_to_view(cx, me, false, 50);
                }
            }
            _ => {}
        }
    }

    fn as_dialog(&mut self) -> Option<&mut dyn Dialog> {
        Some(self)
    }
}

// ---------- UI_Notification ----------

const N_TIMER: u32 = 1;
const N_CREATED: u32 = 2;
const N_ACTION1: u32 = 3;
const N_ACTION2: u32 = 4;
const N_DISMISS: u32 = 5;
const N_BROADCAST: u32 = 6;

#[derive(Clone, Copy, Debug)]
enum Leave {
    Action1,
    Action2,
    Dismissed,
    TimedOut,
}

/// A toast on the root canvas (ZOrder 250): slides in at the screen's top right (the class default Screen Position; 0
/// = bottom right), moves down for each later one, leaves on its RemoveTimer, a button or the X.
/// Deviation, not ported: icon, AccountList rows, the pre-map-load exit.
/// The account row (UI_SocialPlayer) shows the player of params.account_id (@801-1047: Get or Create Social Player
/// Data For Account, SetData, Visible): one met recently or on the online list, else the toaster the toast message
/// named.
#[derive(Debug)]
pub struct NotificationToast {
    params: NotificationParams,
    guid: String,
    index: i32,
    later_height: f64,
    removing: bool,
    remove_timer: String,
    context_timer: f64,
    enter_start: f64,
    slide_start: f64,
    exit_start: f64,
    bar_start: f64,
    pub on_action1: Event<()>,
    pub on_action2: Event<()>,
    pub on_dismissed: Event<()>,
    pub on_timed_out: Event<()>,
}

impl NotificationToast {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(NotificationToast {
            params: NotificationParams::default(),
            guid: NULL_GUID.to_string(),
            index: 0,
            later_height: 0.0,
            removing: false,
            remove_timer: NULL_GUID.to_string(),
            context_timer: 0.1,
            enter_start: -1.0,
            slide_start: -1.0,
            exit_start: -1.0,
            bar_start: -1.0,
            on_action1: Event::default(),
            on_action2: Event::default(),
            on_dismissed: Event::default(),
            on_timed_out: Event::default(),
        })
    }

    fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        ["buttonAction1", "buttonAction2", "buttonDismiss"].iter().map(|n| interactor_in(cx, n)).collect()
    }

    fn top_right(cx: &Cx<'_>) -> bool {
        enum_index(Some(&prop(cx, "Screen Position"))) == 1
    }

    fn leave(&mut self, cx: &mut Cx<'_>, which: Leave) {
        match which {
            Leave::Action1 => cx.emit(self, |c| &c.on_action1, &()),
            Leave::Action2 => cx.emit(self, |c| &c.on_action2, &()),
            Leave::Dismissed => cx.emit(self, |c| &c.on_dismissed, &()),
            Leave::TimedOut => cx.emit(self, |c| &c.on_timed_out, &()),
        }
        self.exit_toast(cx);
    }

    /// GetPositionValues: the slot's place and the offset the slide starts from: (anchor, y, from).
    fn position_values(&self, cx: &Cx<'_>) -> (f64, f64, f64) {
        let i = f64::from(self.index);
        if Self::top_right(cx) {
            (0.0, i * -10.0 + self.later_height + 50.0, -125.0)
        } else {
            (1.0, i * 10.0 - self.later_height - 50.0, 125.0)
        }
    }

    fn enter(&mut self, cx: &mut Cx<'_>) {
        let (anchor, y, from) = self.position_values(cx);
        let me = cx.w;
        let now = cx.view.now();
        let w = wd(cx, me);
        w.anchors = [1.0, anchor, 1.0, anchor];
        w.align = [1.0, anchor];
        w.offsets.l = -160.0;
        w.offsets.t = y;
        if self.index == 0 {
            self.enter_start = now;
            w.opacity = 0.0;
        }
        self.slide_start = now;
        w.transform.translation[1] = from;
        self.update_context(cx);
        if let Some(a) = self.params.audio.clone() {
            cx.post(&a);
        }
    }

    /// UpdateContext: the buttons follow the current context, so a toast can be used over any screen.
    fn update_context(&mut self, cx: &mut Cx<'_>) {
        let cur = cx.view.manager.get_current_context(0).to_string();
        let list = Self::interactors(cx);
        if cx.view.manager.interactor(list[0]).is_some_and(|s| s.context == cur) {
            return;
        }
        batch_set_context(cx, &list, &cur);
        for i in list {
            if let Some(s) = cx.view.manager.interactor_mut(i) {
                s.navigable = true;
            }
        }
    }

    /// ToggleWarning: the red look.
    pub fn toggle_warning(&mut self, cx: &mut Cx<'_>, warning: bool) {
        let c: Rgba = if warning { [1.0; 4] } else { [0.042311, 0.057805, 0.099899, 1.0] };
        for n in ["titleLabel", "bodyLabel"] {
            let w = find(cx, n);
            wd(cx, w).color = c;
        }
        let q = find(cx, "quadBG");
        let bg: Rgba = if warning { [0.904661, 0.0, 0.066626, 1.0] } else { [0.737911, 0.806952, 0.723055, 1.0] };
        quad_color(cx, q, bg);
    }

    fn unbind(cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_timer_event.remove(Handler { owner: me, id: N_TIMER });
        cx.view.manager.ui_notification_created.remove(Handler { owner: me, id: N_CREATED });
    }

    /// Exit: once; the running moves stop, the toast fades over 0.25 s and is destroyed.
    fn exit_toast(&mut self, cx: &mut Cx<'_>) {
        if self.removing {
            return;
        }
        self.removing = true;
        Self::unbind(cx);
        for i in Self::interactors(cx) {
            set_interactable(cx, i, false);
        }
        if cx.view.is_constructed(cx.w) {
            self.exit_start = cx.view.now();
        } else {
            self.destroy(cx);
        }
    }

    fn destroy(&mut self, cx: &mut Cx<'_>) {
        self.exit_start = -1.0;
        Self::unbind(cx);
        let me = cx.w;
        MenuView::discard(cx, me);
        cx.view.manager.notification_destroyed(me);
    }

    /// The account row of `id` (Init @801-1047): Collapsed with none; else the player's data and Visible. The
    /// Construct's look of the row first (@3494-3956: username at size 24 in the dark colour, the platform indicator
    /// dark, the status brush green-grey, iconContainer Collapsed), which the build sets after Init and before its own
    /// UpdateAppearance: the same end state.
    fn account_row(cx: &mut Cx<'_>, row: WidgetId, id: &str) {
        const DARK: [f64; 4] = [0.042311, 0.057805, 0.099899, 1.0];
        let data = (!id.is_empty()).then(|| {
            let online = &cx.view.session.online;
            let known = online.players.iter().chain(&online.listed).find(|p| p.id == id);
            match known {
                Some(p) => SocialPlayerData::from(p),
                None => SocialPlayerData {
                    id: id.to_string(),
                    name: cx.view.session.toasts.sender_name(id).unwrap_or_default().to_string(),
                    ..SocialPlayerData::default()
                },
            }
        });
        let Some(data) = data else {
            wd(cx, row).vis = Vis::Collapsed;
            return;
        };
        let tree = cx.view.tree();
        let (name, platform, status, icon) = (
            tree.find_opt(row, "username"),
            tree.find_opt(row, "UI_UserPlatformIndicator"),
            tree.find_opt(row, "statusIndicator"),
            tree.find_opt(row, "iconContainer"),
        );
        if let Some(n) = name {
            let x = wd(cx, n);
            x.font_size = 24.0;
            x.color = DARK;
        }
        if let Some(w) = platform {
            wd(cx, w).color = DARK;
        }
        if let Some(w) = status {
            wd(cx, w).color = [0.590619, 0.752942, 0.686686, 1.0];
        }
        if let Some(w) = icon {
            wd(cx, w).vis = Vis::Collapsed;
        }
        cx.visit::<SocialPlayer, _>(row, |r, cx| r.set_data(cx, data));
        wd(cx, row).vis = Vis::Visible;
    }
}

impl Notification for NotificationToast {
    fn params(&self) -> &NotificationParams {
        &self.params
    }
    fn on_action1(&mut self) -> &mut Event<()> {
        &mut self.on_action1
    }
    fn on_action2(&mut self) -> &mut Event<()> {
        &mut self.on_action2
    }
    fn on_dismissed(&mut self) -> &mut Event<()> {
        &mut self.on_dismissed
    }
    fn on_timed_out(&mut self) -> &mut Event<()> {
        &mut self.on_timed_out
    }

    /// Init: runs before the toast is on the canvas.
    fn init(&mut self, cx: &mut Cx<'_>, p: NotificationParams) {
        let (title, body) = (find(cx, "titleLabel"), find(cx, "bodyLabel"));
        let on = |s: &str| if s.is_empty() { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
        let x = wd(cx, title);
        x.text = p.title.clone();
        x.vis = on(&p.title);
        let x = wd(cx, body);
        x.text = p.body.clone();
        x.vis = on(&p.body);
        let b1 = find(cx, "buttonAction1");
        let l1 = p.button_label1.clone();
        cx.visit::<ButtonMain, _>(b1, |b, cx| b.set_label(cx, &l1));
        wd(cx, b1).vis = if p.button_label1.is_empty() { Vis::Collapsed } else { Vis::Visible };
        let b2 = find(cx, "buttonAction2");
        let l2 = p.button_label2.clone();
        cx.visit::<ButtonSecondary, _>(b2, |b, cx| b.init(cx, &l2));
        let o = find(cx, "buttonAction2Overlay");
        wd(cx, o).vis = if p.button_label2.is_empty() { Vis::Collapsed } else { Vis::Visible };
        let d = find(cx, "buttonDismiss");
        wd(cx, d).vis = if p.allow_dismissal { Vis::Visible } else { Vis::Collapsed };
        if let Some(row) = cx.view.tree().find_opt(cx.w, "UI_SocialPlayer") {
            Self::account_row(cx, row, &p.account_id);
        }
        let t = find(cx, "timer");
        wd(cx, t).vis = if p.show_timer { Vis::Visible } else { Vis::Collapsed };
        // only a title: it shows in the body's place
        if p.body.is_empty() && p.account_id.is_empty() {
            wd(cx, title).vis = Vis::Collapsed;
            let x = wd(cx, body);
            x.text = p.title.clone();
            x.vis = Vis::SelfHitTestInvisible;
        }
        // @1410-1852: the ExtraWidget made into extraWidgetContainer, its overlay slot HAlign / VAlign Fill
        if !p.extra_widget.is_empty() {
            if cx.view.bundle.widgets.contains_key(&p.extra_widget) {
                let x = MenuView::create(cx, &p.extra_widget);
                let w = wd(cx, x);
                w.h_align = HAlign::Fill;
                w.v_align = VAlign::Fill;
                let c = find(cx, "extraWidgetContainer");
                MenuView::add_child(cx, c, x, None);
            } else {
                eprintln!("menu: the notification's extra widget {} is not in the bundle", p.extra_widget);
            }
        }
        let t = self.remove_timer.clone();
        self.remove_timer = Manager::add_timer(cx, &t, p.screen_time);
        self.params = p;
        // BroadcastSize: the laid-out height goes to the earlier toasts
        let me = cx.w;
        cx.view.delay(0.06, Handler { owner: me, id: N_BROADCAST });
    }

    /// DisableAction1 (@6373).
    fn disable_action1(&mut self, cx: &mut Cx<'_>) {
        let i = interactor_in(cx, "buttonAction1");
        set_interactable(cx, i, false);
    }

    fn exit(&mut self, cx: &mut Cx<'_>) {
        self.exit_toast(cx);
    }
}

impl Controller for NotificationToast {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_timer_event.add(Handler { owner: me, id: N_TIMER });
        cx.view.manager.ui_notification_created.add(Handler { owner: me, id: N_CREATED });
        let b1 = find(cx, "buttonAction1");
        cx.visit::<ButtonMain, _>(b1, |b, _| b.on_click.add(Handler { owner: me, id: N_ACTION1 }));
        let b2 = find(cx, "buttonAction2");
        cx.visit::<ButtonSecondary, _>(b2, |b, _| b.on_click.add(Handler { owner: me, id: N_ACTION2 }));
        let d = find(cx, "buttonDismiss");
        cx.visit::<IconButton, _>(d, |b, _| b.on_click.add(Handler { owner: me, id: N_DISMISS }));
        // PlayTimerAnimation
        self.bar_start = cx.view.now();
        cx.post("ui_menu_notification");
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        self.guid = cx.view.manager.new_guid();
        self.enter(cx);
    }

    fn timers(&mut self, cx: &mut Cx<'_>, dt: f64) {
        let now = cx.view.now();
        let alpha = |start: f64, len: f64| ((now - start) / len).clamp(0.0, 1.0);
        let me = cx.w;
        if self.exit_start >= 0.0 {
            let a = alpha(self.exit_start, 0.25);
            wd(cx, me).opacity = 1.0 - a;
            if a >= 1.0 {
                self.destroy(cx);
            }
            return;
        }
        self.context_timer -= dt;
        if self.context_timer <= 0.0 {
            self.context_timer += 0.1;
            self.update_context(cx);
        }
        if self.enter_start >= 0.0 {
            let a = alpha(self.enter_start, 0.25);
            wd(cx, me).opacity = a;
            if a >= 1.0 {
                self.enter_start = -1.0;
            }
        }
        if self.slide_start >= 0.0 {
            // Ease(0, 1, alpha, EaseOut, exponent 2)
            let a = alpha(self.slide_start, 0.5);
            let from = self.position_values(cx).2;
            wd(cx, me).transform.translation[1] = from * (1.0 - a) * (1.0 - a);
            if a >= 1.0 {
                self.slide_start = -1.0;
            }
        }
        if self.bar_start >= 0.0 {
            let a = if self.params.screen_time > 0.0 { alpha(self.bar_start, self.params.screen_time) } else { 0.0 };
            let bar = find(cx, "timerBar");
            wd(cx, bar).transform.scale[0] = 1.0 - a;
            if a >= 1.0 {
                self.bar_start = -1.0;
            }
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            // a toast notification's TOAST BACK (Toasts::received): the toast back to its sender
            crate::menu::toasts::TOAST_BACK_CLICKED => {
                let from = self.params.account_id.clone();
                crate::menu::toasts::Toasts::toast(cx, &from, true);
            }
            N_ACTION1 => self.leave(cx, Leave::Action1),
            N_ACTION2 => self.leave(cx, Leave::Action2),
            N_DISMISS => self.leave(cx, Leave::Dismissed),
            N_TIMER => {
                if arg.downcast_ref::<String>() == Some(&self.remove_timer) {
                    self.leave(cx, Leave::TimedOut);
                }
            }
            N_CREATED => {
                let Some(&(height, widget)) = arg.downcast_ref::<(f64, WidgetId)>() else { return };
                if widget == cx.w {
                    return;
                }
                self.later_height += height;
                self.index += 1;
                self.enter(cx);
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == N_BROADCAST && !self.removing {
            let me = cx.w;
            let h = cx.view.tree().arranged_size(me)[1];
            cx.fire_manager(|m| &m.ui_notification_created, &(h, me));
        }
    }

    fn as_notification(&mut self) -> Option<&mut dyn Notification> {
        Some(self)
    }
}
