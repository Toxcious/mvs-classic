//! A user widget's controller: the Blueprint's
//! events as trait methods, called by the view with a context of what the controller may touch.
//!
//! A controller is owned by the view, one per user widget, keyed by the widget's id. A callback another object
//! holds (a button's click, a delay, an event's handler) is a `Handler`: the owner's widget and an id of the owner's
//! own, delivered through `Controller::handle` (with the event's argument) or `Controller::resume` (none).
//!
//! A controller is taken out of the view while one of its methods runs (it is busy). What reaches a busy
//! controller from inside that call (a Blueprint calls it at once, synchronously):
//! - its own event fired by itself: `Cx::emit` hands the handlers it owns to the controller in hand, in order, at
//!   once (an interactor's OnFocus reaching its own onFocusInternal);
//! - a call through another widget's method back to it (a handler of a screen whose button closed a modal, the
//!   modal's dismissed event firing back to the screen; a dialog that removes itself, its own Destruct): deferred,
//!   run in order when the busy call returns, before its caller goes on;
//! - a controller whose widget is gone (never made, or freed): nothing, as a removed widget's callback.
//!
//! Deviation: the build runs the deferred calls inside the call that reached them; here they run when the busy method
//! returns. A method that reads state such a call would have changed sees the state before it.

use std::any::Any;

use mvs_settings::Settings;

use super::MenuSink;
use super::input::Pointer;
use super::manager::{ContextMenu, Dialog, Manager, Modal, Notification, Waiting};
use super::match_pause::{MatchPauseSession, PauseMenuWidget};
use super::training_config::LabSession;
use super::tutorial::TutorialSession;
use super::view::MenuView;
use super::widgets::controls::interactor::TooltipParams;
use crate::bindings::Bindings;
use crate::slate::WidgetId;
use crate::text::Fonts;

/// A callback: the controller of user widget `owner`, its own id `id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Handler {
    pub owner: WidgetId,
    pub id: u32,
}

/// An event dispatcher: its handlers in the order added; a handler added again is not added
/// ("no duplicate bindings", as AddDynamic).
#[derive(Debug)]
pub struct Event<T> {
    handlers: Vec<Handler>,
    _arg: std::marker::PhantomData<fn(&T)>,
}

impl<T> Default for Event<T> {
    fn default() -> Event<T> {
        Event { handlers: Vec::new(), _arg: std::marker::PhantomData }
    }
}

impl<T> Clone for Event<T> {
    fn clone(&self) -> Event<T> {
        Event { handlers: self.handlers.clone(), _arg: std::marker::PhantomData }
    }
}

impl<T: Any + Clone> Event<T> {
    pub fn add(&mut self, h: Handler) {
        if !self.handlers.contains(&h) {
            self.handlers.push(h);
        }
    }

    /// `h` taken out (Array.splice of indexOf).
    pub fn remove(&mut self, h: Handler) {
        if let Some(i) = self.handlers.iter().position(|x| *x == h) {
            self.handlers.remove(i);
        }
    }

    /// Every handler of `owner` taken out (a destructed widget's bindings).
    pub fn remove_owner(&mut self, owner: WidgetId) {
        self.handlers.retain(|h| h.owner != owner);
    }

    /// Every handler of an owner in `gone` taken out (freed widgets' bindings).
    pub fn remove_owners(&mut self, gone: &std::collections::HashSet<WidgetId>) {
        self.handlers.retain(|h| !gone.contains(&h.owner));
    }

    pub fn clear(&mut self) {
        self.handlers.clear();
    }

    pub fn handlers(&self) -> &[Handler] {
        &self.handlers
    }

    /// Each handler of the moment called with `a` (a handler added or removed by one of them counts from the next
    /// fire: the list is a copy, as `[...this.handlers]`). An event held by the view (the manager's) or by a
    /// controller in hand is fired with `Cx::fire_manager` / `Cx::emit`, which copy the list first.
    pub fn fire(&self, cx: &mut Cx<'_>, a: &T) {
        for h in self.handlers.clone() {
            cx.call(h, a);
        }
    }
}

/// What a controller's method may touch: its own widget, the view (its tree, the other controllers, the manager),
/// the fonts, the preferences and the key bindings, the sound, the pointer, and the running match's sessions.
pub struct Cx<'a> {
    /// The widget of the controller called.
    pub w: WidgetId,
    pub view: &'a mut MenuView,
    pub fonts: &'a mut Fonts,
    pub prefs: &'a mut Settings,
    pub bindings: &'a mut Bindings,
    pub sink: &'a mut dyn MenuSink,
    /// The window's pointer as the screens share it.
    pub pointer: &'a mut Pointer,
    /// The running Lab's session (None outside the Lab).
    pub lab: Option<&'a mut dyn LabSession>,
    /// The running match's pause session (None outside an offline match with a
    /// pause menu).
    pub pause: Option<&'a mut dyn MatchPauseSession>,
    /// The running tutorial's session (tutorial.rs; None outside the tutorial widgets' screen of a tutorial match).
    pub tutorial: Option<&'a mut dyn TutorialSession>,
}

/// A call held for a busy controller (run when its call returns).
pub(crate) type Deferred = Box<dyn FnOnce(&mut dyn Controller, &mut Cx<'_>)>;

impl Cx<'_> {
    /// `h` called with `a`: its owner's `handle` (deferred while the owner is busy; nothing when it is gone).
    pub fn call<T: Any + Clone>(&mut self, h: Handler, a: &T) {
        let a = a.clone();
        self.run(h.owner, Box::new(move |c, cx| c.handle(cx, h.id, &a)));
    }

    /// `h` called with no argument (a delay's end, a click).
    pub fn resume(&mut self, h: Handler) {
        self.run(h.owner, Box::new(move |c, cx| c.resume(cx, h.id)));
    }

    /// `f` run on the controller of `w` with its context: at once when it is in the view, deferred while it is
    /// busy, dropped when it is gone.
    pub(crate) fn run(&mut self, w: WidgetId, f: Deferred) {
        if self.view.is_busy(w) {
            self.view.defer(w, f);
            return;
        }
        let Some(mut c) = self.view.take_controller(w) else { return };
        f(c.as_mut(), &mut self.with(w));
        self.put_back(w, c);
    }

    /// `c` back in the view, then each call deferred for it while it was out, in order.
    pub(crate) fn put_back(&mut self, w: WidgetId, mut c: Box<dyn Controller>) {
        while let Some(f) = self.view.take_deferred(w) {
            f(c.as_mut(), &mut self.with(w));
        }
        self.view.put_controller(w, c);
    }

    /// `f` on the controller of `w` as its own type, with that widget's context.
    /// None: no such controller, another type, or one busy further up (its method is running: a debug build
    /// panics there, the call is lost).
    pub fn visit<T: Controller, R>(&mut self, w: WidgetId, f: impl FnOnce(&mut T, &mut Cx<'_>) -> R) -> Option<R> {
        let Some(mut c) = self.view.take_controller(w) else {
            debug_assert!(!self.view.is_busy(w), "visit: the controller of {w:?} is busy, the call is dropped");
            return None;
        };
        let r = {
            let any: &mut dyn Any = c.as_mut();
            any.downcast_mut::<T>().map(|t| f(t, &mut self.with(w)))
        };
        self.put_back(w, c);
        r
    }

    /// `f` on the controller of `w` as a trait object (an interactor, a modal); None as `visit`.
    pub fn visit_dyn<R>(&mut self, w: WidgetId, f: impl FnOnce(&mut dyn Controller, &mut Cx<'_>) -> R) -> Option<R> {
        let Some(mut c) = self.view.take_controller(w) else {
            debug_assert!(!self.view.is_busy(w), "visit_dyn: the controller of {w:?} is busy, the call is dropped");
            return None;
        };
        let r = f(c.as_mut(), &mut self.with(w));
        self.put_back(w, c);
        Some(r)
    }

    /// The controller in hand `me` (the context's own, `cx.w`) fires its own event `ev` with `a`: its own handlers
    /// called on `me` at once, the others through `call`, in the event's order.
    pub fn emit<C: Controller, T: Any + Clone>(&mut self, me: &mut C, ev: fn(&C) -> &Event<T>, a: &T) {
        let own = self.w;
        for h in ev(me).handlers.clone() {
            if h.owner == own {
                me.handle(self, h.id, a);
            } else {
                self.call(h, a);
            }
        }
    }

    /// An event of the manager fired with `a` (the handlers copied first).
    pub fn fire_manager<T: Any + Clone>(&mut self, ev: fn(&Manager) -> &Event<T>, a: &T) {
        let e = ev(&self.view.manager).clone();
        e.fire(self, a);
    }

    /// The same context for another widget's controller.
    pub fn with(&mut self, w: WidgetId) -> Cx<'_> {
        Cx {
            w,
            view: self.view,
            fonts: self.fonts,
            prefs: self.prefs,
            bindings: self.bindings,
            sink: self.sink,
            pointer: self.pointer,
            lab: self.lab.as_deref_mut().map(|l| l as _),
            pause: self.pause.as_deref_mut().map(|p| p as _),
            tutorial: self.tutorial.as_deref_mut().map(|t| t as _),
        }
    }

    /// PF_Post_Global_Event, kept to the events the soundbanks hold.
    pub fn post(&mut self, event: &str) {
        if self.sink.has_event(event) {
            self.sink.post(event);
        }
    }

    /// The view's real time, s.
    pub fn now(&self) -> f64 {
        self.view.now()
    }

    /// A call into the level behind the menus, taken by the program after the frame (`MenuView::take_scene_cmds`).
    pub fn scene(&mut self, cmd: super::scene::SceneCmd) {
        self.view.push_scene_cmd(cmd);
    }

    /// A command to online play, run by the program after the frame (`MenuView::take_online_cmds`).
    pub fn online(&mut self, cmd: super::online::OnlineCmd) {
        self.view.push_online_cmd(cmd);
    }
}

/// What a view's frame needs from outside it.
pub struct MenuRun<'a> {
    pub fonts: &'a mut Fonts,
    pub prefs: &'a mut Settings,
    pub bindings: &'a mut Bindings,
    pub sink: &'a mut dyn MenuSink,
    pub pointer: &'a mut Pointer,
    pub lab: Option<&'a mut dyn LabSession>,
    pub pause: Option<&'a mut dyn MatchPauseSession>,
    pub tutorial: Option<&'a mut dyn TutorialSession>,
}

impl MenuRun<'_> {
    /// The context of widget `w` of `view`.
    pub fn cx<'b>(&'b mut self, view: &'b mut MenuView, w: WidgetId) -> Cx<'b> {
        Cx {
            w,
            view,
            fonts: self.fonts,
            prefs: self.prefs,
            bindings: self.bindings,
            sink: self.sink,
            pointer: self.pointer,
            lab: self.lab.as_deref_mut().map(|l| l as _),
            pause: self.pause.as_deref_mut().map(|p| p as _),
            tutorial: self.tutorial.as_deref_mut().map(|t| t as _),
        }
    }
}

/// What an EditableTextBox with the keyboard focus reports to the controller that gave it the focus: OnTextChanged,
/// OnTextCommitted (ETextCommit: 0 default, 1 Enter, 2 the user moved the focus, 3 cleared), the focus path events, and
/// the Escape key (which the build's box leaves alone).
#[derive(Clone, Debug, PartialEq)]
pub enum TextEdit {
    Changed(String),
    Committed(String, u8),
    Focused(bool),
    Escape,
}

/// A user widget's Blueprint: every event defaulted to nothing.
pub trait Controller: Any {
    /// OnInitialized: once, when the widget is made.
    fn on_initialized(&mut self, _cx: &mut Cx<'_>) {}
    /// PreConstruct.
    fn pre_construct(&mut self, _cx: &mut Cx<'_>) {}
    /// Construct: each time the widget is added to a parent or the viewport.
    fn construct(&mut self, _cx: &mut Cx<'_>) {}
    /// Destruct: each time it is taken out.
    fn destruct(&mut self, _cx: &mut Cx<'_>) {}
    /// The widget's Tick, `dt` seconds: while the widget is painted.
    fn tick(&mut self, _cx: &mut Cx<'_>, _dt: f64) {}
    /// The controller's own timers (Set Timer by Event): every frame while the widget is constructed.
    fn timers(&mut self, _cx: &mut Cx<'_>, _dt: f64) {}
    /// A named button of the widget hovered or left (OnHovered / OnUnhovered).
    fn on_button_hover(&mut self, _cx: &mut Cx<'_>, _name: &str, _hovered: bool) {}
    /// A handler of this controller called with an argument (an Event's fire).
    fn handle(&mut self, _cx: &mut Cx<'_>, _id: u32, _arg: &dyn Any) {}
    /// A handler of this controller called with none (a delay's end).
    fn resume(&mut self, _cx: &mut Cx<'_>, _id: u32) {}
    /// The EditableTextBox `edit_box` this controller gave the keyboard focus to (MenuView::set_keyboard_focus)
    /// reports `ev`.
    fn text_edit(&mut self, _cx: &mut Cx<'_>, _edit_box: WidgetId, _ev: TextEdit) {}
    /// UI_TooltipTarget's Init(params, player) then ToggleTooltip(show), as an interactor's Toggle Tooltip calls
    /// them on the target in its hitbox (the tooltip controllers implement it).
    fn tooltip_target(&mut self, _cx: &mut Cx<'_>, _params: &TooltipParams, _player: i32, _show: bool) {}
    /// A context's navigation start (manager.navigationStart, the port's own): the interactor a navigation key
    /// goes to while nothing navigable is focused in the context (`id`: the handler's).
    fn navigation_start(&mut self, _cx: &mut Cx<'_>, _id: u32) -> Option<WidgetId> {
        None
    }
    /// The controller as the manager's modal, when it is one.
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        None
    }
    /// The controller as a section bar's button, when it is one.
    fn as_bar_button(&mut self) -> Option<&mut dyn crate::menu::widgets::controls::bar_buttons::BarButton> {
        None
    }
    fn as_dialog(&mut self) -> Option<&mut dyn Dialog> {
        None
    }
    fn as_notification(&mut self) -> Option<&mut dyn Notification> {
        None
    }
    fn as_waiting(&mut self) -> Option<&mut dyn Waiting> {
        None
    }
    fn as_context_menu(&mut self) -> Option<&mut dyn ContextMenu> {
        None
    }
    /// The controller as UI_PauseMenu, when it is one.
    fn as_pause_menu(&mut self) -> Option<&mut dyn PauseMenuWidget> {
        None
    }
    /// The controller as a binding row that takes the user focus, when it is one.
    fn as_input_capture(&mut self) -> Option<&mut dyn InputCapture> {
        None
    }
    /// The controller as a tile of a UI_GridItemList, when it is one.
    fn as_grid_item(&mut self) -> Option<&mut dyn crate::menu::screens::lab_selectors::Tile> {
        None
    }
}

/// A binding row given the user focus: while it listens it
/// sees the keys, the mouse buttons pressed over it, the wheel over it and the left stick first.
pub trait InputCapture {
    fn is_listening(&self) -> bool;
    /// OnPreviewKeyDown: true when the row took the key.
    fn on_preview_key_down(&mut self, cx: &mut Cx<'_>, key: &str) -> bool;
    /// OnPreviewMouseButtonDown / OnMouseWheel as a key (LeftMouseButton, MouseScrollUp...).
    fn process_mouse_event(&mut self, cx: &mut Cx<'_>, key: &str);
    /// OnAnalogValueChanged (the raw left stick axes).
    fn on_analog_value_changed(&mut self, cx: &mut Cx<'_>, value: f64);
}
