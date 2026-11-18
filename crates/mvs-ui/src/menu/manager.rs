//! The UI manager: each player's context and focus over the registered
//! interactors, the navigation and the UI keys, the modal stack, the dialogs, the notifications, the waiting
//! indicator, the context menus, the timers and the level transition.
//!
//! The manager lives in the view (`MenuView::manager`); what makes or touches widgets or calls a controller is an
//! associated function taking the context (`Manager::show_modal(cx, ...)`), the rest a method. The interactors'
//! state is kept here (`InteractorState`, widgets/controls/interactor.rs). A player is an `i32`
//! (-1: none, nothing done).
//!
//! FindUIManager (the Lab's match): code outside a controller reaches the manager of the screen on
//! show through `MenuScreen::with_cx` (Deviation: one manager per menu screen, the build one for the
//! game's life).

use indexmap::IndexMap;
use std::collections::HashMap;

use super::controller::{Controller, Cx, Event, Handler};
use super::view::MenuView;
use super::widgets::controls::interactor::{self, InteractorState};
use crate::bindings::is_gamepad_key;
use crate::slate::WidgetId;

/// An FGuid's string of zeros.
pub const NULL_GUID: &str = "00000000-00000000-00000000-00000000";
/// The Z order of the widgets added to the view over the viewport's own (UI_GLOBAL_Z): the game instance adds
/// UI_Global at ZOrder 5000 (PandaGameInstance @30311); every child of its root canvas (context menus 5,
/// notifications 250, the waiting indicator 150) draws over what goes on the viewport itself (modals 3,
/// handholding 4, tooltips 5 / 100), in the order of its own slot ZOrder.
pub const UI_GLOBAL_Z: i32 = 5000;
/// SetAnalogMovement's dead zone of the left stick.
pub const ANALOG_DEAD_ZONE: f64 = 0.25;
/// Detect Scroll's dead zone of the right stick.
pub const SCROLL_DEAD_ZONE: f64 = 0.15;

const INTERACTION_INTERVAL: f64 = 1.5;
const NAVIGATION_INTERVAL: f64 = 0.125;
const MOUSE_CLICK_INTERVAL: f64 = 0.025;
const RESTORE_INTERVAL: f64 = 0.033333;

/// Transition Time 3 x Time Multiplier 0.1.
const TRANSITION_TIME: f64 = 0.3;
/// The Fader's render opacity by transition type (FadeOut, FadeIn, the countdown, Black, None): at the call
/// (@3702-3853) and at Complete (@3078-3229).
const FADER_START: [f64; 5] = [0.0, 1.0, 1.0, 1.0, 0.0];
const FADER_END: [f64; 5] = [1.0, 0.0, 0.0, 1.0, 0.0];
/// SetRTPCValue's interpolation: FTrunc(1000 x Transition Time).
const TRANSITION_RTPC_MS: f64 = 3000.0;

/// The UI keys (UI_Manager's Keys-* lists of the CDO).
struct Keys {
    submit: &'static [&'static str],
    submit2: &'static [&'static str],
    submit3: &'static [&'static str],
    submit5: &'static [&'static str],
    cancel: &'static [&'static str],
    page_right: &'static [&'static str],
    page_left: &'static [&'static str],
    item_right: &'static [&'static str],
    item_left: &'static [&'static str],
    up: &'static [&'static str],
    down: &'static [&'static str],
    left: &'static [&'static str],
    right: &'static [&'static str],
    reset: &'static [&'static str],
    scroll_up: &'static [&'static str],
    scroll_down: &'static [&'static str],
    ignore: &'static [&'static str],
}

const KEYS: Keys = Keys {
    submit: &["SpaceBar", "Gamepad_FaceButton_Bottom", "C"],
    submit2: &["Tab", "Gamepad_FaceButton_Top"],
    submit3: &["V", "Gamepad_FaceButton_Left"],
    submit5: &["Enter", "Gamepad_FaceButton_Bottom"],
    cancel: &["Escape", "Gamepad_FaceButton_Right"],
    page_right: &["E", "Gamepad_RightShoulder"],
    page_left: &["Q", "Gamepad_LeftShoulder"],
    item_right: &["J", "Gamepad_RightTrigger"],
    item_left: &["H", "Gamepad_LeftTrigger"],
    up: &["W", "Up", "Gamepad_DPad_Up"],
    down: &["S", "Down", "Gamepad_DPad_Down"],
    left: &["A", "Left", "Gamepad_DPad_Left"],
    right: &["D", "Right", "Gamepad_DPad_Right"],
    reset: &["Home", "Gamepad_Special_Left"],
    scroll_up: &["PageUp"],
    scroll_down: &["PageDown"],
    ignore: &["Tilde"],
};

const LEFT_STICK: &[&str] = &[
    "Gamepad_LeftStick_Down",
    "Gamepad_LeftStick_Up",
    "Gamepad_LeftStick_Left",
    "Gamepad_LeftStick_Right",
    "Gamepad_LeftY",
    "Gamepad_LeftX",
];
const DPAD: &[&str] = &["Gamepad_DPad_Left", "Gamepad_DPad_Right", "Gamepad_DPad_Down", "Gamepad_DPad_Up"];

fn has(list: &[&str], key: &str) -> bool {
    list.contains(&key)
}

/// Player 0's last interaction as UI_Manager keeps it: LastInteractionType (0 gamepad, 1 keyboard, 2 mouse),
/// InputType and cursorMode. The build has one UI_Manager for the game's life (UI_Global outlives the levels); the
/// port has one per menu view, so the window keeps these (`Pointer::interaction`) and every screen's manager
/// starts each frame from them (screen.rs). The defaults are the class's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Interaction {
    pub last_type: i32,
    pub input_type: i32,
    pub cursor_mode: bool,
}

impl Interaction {
    /// A key or pad button pressed that no manager took (the port's own: the match's play, the title, a closed
    /// panel): the device type alone. No manager handles input then (handleInput off), so
    /// ProcessPreviewKeyDown's InputType and DetermineCursorMode's cursorMode stay the menus' own.
    pub fn played_key(&mut self, key: &str) {
        self.last_type = if is_gamepad_key(key) { 0 } else { 1 };
    }

    /// A mouse move of more than a pixel or a mouse button no manager took: the device type alone, as `played_key`.
    pub fn played_mouse(&mut self) {
        self.last_type = 2;
    }
}

fn is_nav_key(k: &str) -> bool {
    has(KEYS.up, k) || has(KEYS.down, k) || has(KEYS.left, k) || has(KEYS.right, k)
}

/// UI_InputLabel's key for a UI-bound action: UI_Functions GetFirstKeyOfType over the manager's key list.
/// Keys-Submit4 is empty in the CDO.
fn ui_action_list(action: &str) -> Option<&'static [&'static str]> {
    Some(match action {
        "UISubmit" => KEYS.submit,
        "UISubmit2" => KEYS.submit2,
        "UISubmit3" => KEYS.submit3,
        "UISubmit5" => KEYS.submit5,
        "UICancel" => KEYS.cancel,
        "UIReset" => KEYS.reset,
        "UINextPage" => KEYS.page_right,
        "UIPrevPage" => KEYS.page_left,
        "ItemRight" => KEYS.item_right,
        "ItemLeft" => KEYS.item_left,
        _ => return None,
    })
}

/// The action is one the manager's key lists hold (UISubmit4 too, with no key).
pub fn is_ui_action(action: &str) -> bool {
    ui_action_list(action).is_some() || action == "UISubmit4"
}

/// The first key of the action's list of the type (type 0 gamepad, else keyboard / mouse).
pub fn ui_action_key(action: &str, gamepad: bool) -> Option<&'static str> {
    ui_action_list(action)?
        .iter()
        .copied()
        .find(|k| (k.starts_with("Gamepad_") || k.starts_with("GenericUSBController_")) == gamepad)
}

fn valid_guid(g: &str) -> bool {
    !g.is_empty() && g != NULL_GUID
}

/// A new FGuid's string (NewGuid), from a random source of [0, 1).
pub fn new_guid(random: &mut dyn FnMut() -> f64) -> String {
    let mut h = || format!("{:08X}", (random() * 4294967296.0).floor() as u64);
    let (a, b, c, d) = (h(), h(), h(), h());
    format!("{a}-{b}-{c}-{d}")
}

/// A modal (BPI_Modal); its controller returns itself from `Controller::as_modal`.
pub trait Modal {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str);
    fn post_show(&mut self, cx: &mut Cx<'_>, instructions: &str, extra_data: &str);
    fn pre_dismiss(&mut self, cx: &mut Cx<'_>);
    fn post_dismiss(&mut self, cx: &mut Cx<'_>);
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, reason: &str, from_cancel: bool);
    fn get_modal_context(&self) -> String;
}

/// ShowDialog's Struct_DialogParams (CurrencyCost, the shop's price map, is not ported).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DialogParams {
    pub title: String,
    pub body: String,
    pub button_left_label: String,
    pub button_middle_label: String,
    pub button_right_label: String,
    pub button_prominent_label: String,
    pub tag: String,
}

/// A dialog shown by ShowDialog (UI_Dialog_Base); its id on the manager's stack. Its controller:
/// `MenuView::dialog`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DialogId(pub WidgetId);

/// UI_Dialog_Base.
pub trait Dialog {
    fn params(&self) -> &DialogParams;
    fn is_on_screen(&self) -> bool;
    fn is_exiting(&self) -> bool;
    fn button_left_clicked(&mut self) -> &mut Event<()>;
    fn button_middle_clicked(&mut self) -> &mut Event<()>;
    fn button_right_clicked(&mut self) -> &mut Event<()>;
    fn button_prominent_clicked(&mut self) -> &mut Event<()>;
    fn dialog_dismissed(&mut self) -> &mut Event<()>;
    fn delayed_init(&mut self, cx: &mut Cx<'_>, params: DialogParams, delay: f64);
    fn exit(&mut self, cx: &mut Cx<'_>);
}

/// ShowNotification's Struct_NotificationParams with the class defaults of UI_Notification's own copy
/// (screenTime 4 s, <= 0: the timer fires at once; allowDismissal; audio ui_menu_notification). Not ported: icon,
/// AccountList. timing (E_UINotificationTiming): 0 Immediate, 1 Homescreen, 2 ResourcesLoaded. extra_widget: the
/// ExtraWidget class's name ('' none), made into the notification's extraWidgetContainer.
#[derive(Clone, Debug, PartialEq)]
pub struct NotificationParams {
    pub title: String,
    pub body: String,
    /// s on screen.
    pub screen_time: f64,
    pub allow_dismissal: bool,
    /// The Wwise event posted at its show (None: none).
    pub audio: Option<String>,
    pub tag: String,
    pub button_label1: String,
    pub button_label2: String,
    pub account_id: String,
    pub show_timer: bool,
    pub timing: i32,
    pub extra_widget: String,
}

impl Default for NotificationParams {
    fn default() -> NotificationParams {
        NotificationParams {
            title: String::new(),
            body: String::new(),
            screen_time: 4.0,
            allow_dismissal: true,
            audio: Some("ui_menu_notification".into()),
            tag: String::new(),
            button_label1: String::new(),
            button_label2: String::new(),
            account_id: String::new(),
            show_timer: false,
            timing: 0,
            extra_widget: String::new(),
        }
    }
}

/// UI_Notification. Its controller: `MenuView::notification`.
pub trait Notification {
    fn params(&self) -> &NotificationParams;
    fn on_action1(&mut self) -> &mut Event<()>;
    fn on_action2(&mut self) -> &mut Event<()>;
    fn on_dismissed(&mut self) -> &mut Event<()>;
    fn on_timed_out(&mut self) -> &mut Event<()>;
    fn init(&mut self, cx: &mut Cx<'_>, params: NotificationParams);
    /// DisableAction1: the first button can no longer be pressed.
    fn disable_action1(&mut self, cx: &mut Cx<'_>);
    fn exit(&mut self, cx: &mut Cx<'_>);
}

/// The waiting indicator (UI_Waiting).
pub trait Waiting {
    fn set_waiting(&mut self, cx: &mut Cx<'_>, is_waiting: bool, label: &str, opaque: bool, countdown: f64);
}

/// A context menu's button (a button's tooltip is its row's tip).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextMenuButton {
    pub id: String,
    pub label: String,
    pub tooltip: String,
}

/// ShowContextMenu's Struct_ContextMenuParams.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextMenuParams {
    pub id: String,
    pub buttons: Vec<ContextMenuButton>,
    pub show_close_button: bool,
    pub set_context: bool,
    pub position_matcher: Option<WidgetId>,
    pub position_offset: [f64; 2],
    pub label: String,
    pub use_position_matcher_width: bool,
    pub alignment: [f64; 2],
    pub auto_close: bool,
    pub override_width: f64,
    pub enable_scroll_bar: bool,
    pub show_scroll_bar_after_count: i32,
}

/// UI_ContextMenu. Its controller: `MenuView::context_menu`.
pub trait ContextMenu {
    fn id(&self) -> &str;
    fn guid(&self) -> &str;
    fn on_dismissed(&mut self) -> &mut Event<()>;
    /// The menu's buttons made: each button's interactor by its id.
    fn init(&mut self, cx: &mut Cx<'_>, params: ContextMenuParams, player: i32) -> Vec<(String, WidgetId)>;
    fn hide(&mut self, cx: &mut Cx<'_>);
}

/// Transition's Struct_TransitionParams. kind 0 = TransitionOut (to black), 1 = TransitionIn (from black), 2
/// InToCountdown (not ported: `transition`), 3 Black, 4 None.
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionParams {
    pub next_level: String,
    pub kind: i32,
    pub delay: f64,
    pub force: bool,
    pub instant: bool,
    pub level_options: String,
}

impl Default for TransitionParams {
    fn default() -> TransitionParams {
        TransitionParams {
            next_level: String::new(),
            kind: 0,
            delay: 0.0,
            force: false,
            instant: false,
            level_options: "Lobby".into(),
        }
    }
}

/// A navigation's direction (navigate's dx, dy).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
}

impl Nav {
    fn d(self) -> (f64, f64) {
        match self {
            Nav::Up => (0.0, -1.0),
            Nav::Down => (0.0, 1.0),
            Nav::Left => (-1.0, 0.0),
            Nav::Right => (1.0, 0.0),
        }
    }
}

/// A transition under way.
#[derive(Clone, Debug)]
struct TransitionRun {
    p: TransitionParams,
    delay: f64,
    start: f64,
    failsafe: f64,
    failsafe2: f64,
    /// Counts the runs (a test that the run is still this one after an event).
    serial: u64,
}

/// UI_Manager's state.
#[derive(Debug)]
pub struct Manager {
    /// The registered interactors, the newest first (registerInteractor unshifts).
    interactors: Vec<WidgetId>,
    /// Every interactor's state by its widget (made at its OnInitialized, kept for the widget's life).
    states: HashMap<WidgetId, InteractorState>,
    scrollboxes: Vec<WidgetId>,
    pub context_stack: Vec<String>,
    pub last_focused_in_context: HashMap<String, WidgetId>,
    /// Deviation: per context, the interactor a navigation key goes to while nothing navigable is focused there.
    /// The build has no such rule (Navigate @795 takes the newest registered focusable interactor); the home
    /// screen sets it (a controller's `navigation_start` handler).
    pub navigation_start: HashMap<String, Handler>,
    pub ignore_context: String,
    current_context: [String; 4],
    currently_focused: [Option<WidgetId>; 4],
    /// 0 gamepad, 1 keyboard, 2 mouse.
    pub input_type: [i32; 4],
    pub last_interaction_type: [i32; 4],
    pub cursor_mode: [bool; 4],
    /// allowCursorMode: false in the class default, set true by UI_Cursor.Init @2443, which UI_Global's
    /// InitializeCursors @1344 calls for the cursor it creates at its start, before any menu is up; nothing sets it
    /// false. A view without the cursor (the title screen) has the value after that Init too.
    pub allow_cursor_mode: bool,
    /// GetCursor(0): UI_Global's cursor of player 0 (None in a view without one) and its currentlyHovered.
    pub cursor: Option<WidgetId>,
    pub cursor_hovered: Option<WidgetId>,
    /// The left stick as SetAnalogMovement left it (MoveX / MoveY), per player.
    pub move_x: [f64; 4],
    pub move_y: [f64; 4],
    last_cursor_move_time_x: [f64; 4],
    last_cursor_move_time_y: [f64; 4],
    last_mouse_x: [f64; 4],
    last_mouse_y: [f64; 4],
    pub handle_input: bool,
    pub is_waiting: bool,
    /// (the port's own) The waiting cover's own fade in and out, s, for the next `set_waiting` (0: it shows and goes
    /// at once, as the build's does): `set_waiting_faded`.
    pub waiting_fade: f64,
    /// UI_Transitioner: the Fader's render opacity (a black image over everything, UI_Global at ZOrder 5000).
    pub fader: f64,
    transition_run: Option<TransitionRun>,
    transition_serial: u64,
    transition_force: bool,
    pub transition_started: Event<TransitionParams>,
    pub transition_complete: Event<TransitionParams>,
    /// OpenLevel(nextLevel, levelOptions) at the end of a transition with a level, taken by the screen.
    open_level: Option<TransitionParams>,
    pub text_entry_mode: bool,
    pub internal_time: f64,
    pub last_hover_time: f64,
    pub last_modal_time: f64,
    pub last_scroll_time: f64,
    interaction_timer: [f64; 4],
    navigation_timer: [f64; 4],
    mouse_click_timer: f64,
    restore_timer: f64,
    /// The modals by id in the order shown, and the time each was shown.
    modals: IndexMap<String, WidgetId>,
    modal_times: HashMap<String, f64>,
    pub on_key_input: Event<(i32, String)>,
    /// (player, direction, amount)
    pub on_scroll: Event<(i32, i32, f64)>,
    pub on_scroll_horizontal: Event<(i32, i32, f64)>,
    pub on_reset_key: Event<i32>,
    /// (player, direction)
    pub on_request_switch_item: Event<(i32, i32)>,
    pub on_request_switch_page: Event<(i32, i32)>,
    pub on_submit: Event<i32>,
    pub on_submit2: Event<i32>,
    pub on_submit3: Event<i32>,
    pub on_submit5: Event<i32>,
    /// (player, is Escape)
    pub on_cancel: Event<(i32, bool)>,
    pub on_cancel_within_modal: Event<i32>,
    pub modal_shown: Event<String>,
    pub modal_dismissed: Event<String>,
    /// (the interactor, hovered, player)
    pub on_interactor_hover: Event<(WidgetId, bool, i32)>,
    pub on_interactor_focused: Event<WidgetId>,
    pub on_timer_event: Event<String>,
    context_menus: Vec<WidgetId>,
    timer_events: IndexMap<String, f64>,
    /// Dialog Stack, and the queueing delay of ShowDialog.
    pub dialog_stack: Vec<DialogId>,
    dialog_delay_time: f64,
    last_dialog_time: f64,
    pub ui_dialog_created: Event<()>,
    /// UINotificationCreated(the new one's height, the widget).
    pub ui_notification_created: Event<(f64, WidgetId)>,
    notifications: Vec<WidgetId>,
    notification_queue: Vec<NotificationParams>,
    waiting: Option<WidgetId>,
    /// NewGuid's random source (xorshift64*).
    rng: u64,
}

impl Default for Manager {
    fn default() -> Manager {
        Manager::new()
    }
}

impl Manager {
    pub fn new() -> Manager {
        let mut m = Manager {
            interactors: Vec::new(),
            states: HashMap::new(),
            scrollboxes: Vec::new(),
            context_stack: Vec::new(),
            last_focused_in_context: HashMap::new(),
            navigation_start: HashMap::new(),
            ignore_context: String::new(),
            current_context: std::array::from_fn(|_| NULL_GUID.to_string()),
            currently_focused: [None; 4],
            input_type: [0; 4],
            last_interaction_type: [0; 4],
            cursor_mode: [false; 4],
            allow_cursor_mode: true,
            cursor: None,
            cursor_hovered: None,
            move_x: [0.0; 4],
            move_y: [0.0; 4],
            last_cursor_move_time_x: [0.0; 4],
            last_cursor_move_time_y: [0.0; 4],
            last_mouse_x: [0.0; 4],
            last_mouse_y: [0.0; 4],
            handle_input: true,
            is_waiting: false,
            waiting_fade: 0.0,
            fader: 0.0,
            transition_run: None,
            transition_serial: 0,
            transition_force: false,
            transition_started: Event::default(),
            transition_complete: Event::default(),
            open_level: None,
            text_entry_mode: false,
            internal_time: 0.0,
            last_hover_time: 0.0,
            last_modal_time: 0.0,
            last_scroll_time: 0.0,
            interaction_timer: [-1.0; 4],
            navigation_timer: [-1.0; 4],
            mouse_click_timer: -1.0,
            restore_timer: 0.0,
            modals: IndexMap::new(),
            modal_times: HashMap::new(),
            on_key_input: Event::default(),
            on_scroll: Event::default(),
            on_scroll_horizontal: Event::default(),
            on_reset_key: Event::default(),
            on_request_switch_item: Event::default(),
            on_request_switch_page: Event::default(),
            on_submit: Event::default(),
            on_submit2: Event::default(),
            on_submit3: Event::default(),
            on_submit5: Event::default(),
            on_cancel: Event::default(),
            on_cancel_within_modal: Event::default(),
            modal_shown: Event::default(),
            modal_dismissed: Event::default(),
            on_interactor_hover: Event::default(),
            on_interactor_focused: Event::default(),
            on_timer_event: Event::default(),
            context_menus: Vec::new(),
            timer_events: IndexMap::new(),
            dialog_stack: Vec::new(),
            dialog_delay_time: 0.0,
            last_dialog_time: 0.0,
            ui_dialog_created: Event::default(),
            ui_notification_created: Event::default(),
            notifications: Vec::new(),
            notification_queue: Vec::new(),
            waiting: None,
            rng: 0x9e37_79b9_7f4a_7c15,
        };
        m.ignore_context = m.new_guid();
        m
    }

    /// A new FGuid's string from the manager's random source.
    pub fn new_guid(&mut self) -> String {
        new_guid(&mut || self.random())
    }

    /// The menus' random source (Math.random: NewGuid, RandomFloat), in [0, 1).
    pub fn random(&mut self) -> f64 {
        let mut rng = self.rng;
        rng ^= rng >> 12;
        rng ^= rng << 25;
        rng ^= rng >> 27;
        self.rng = rng;
        (rng.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// The widgets `gone` freed (`MenuView::discard`): every table entry and event handler of theirs let go, as
    /// UE's GC clears what only these held.
    pub fn forget(&mut self, gone: &std::collections::HashSet<WidgetId>) {
        let out = |w: &WidgetId| gone.contains(w);
        self.interactors.retain(|w| !out(w));
        self.states.retain(|w, _| !out(w));
        self.scrollboxes.retain(|w| !out(w));
        self.last_focused_in_context.retain(|_, w| !out(w));
        self.navigation_start.retain(|_, h| !out(&h.owner));
        for f in &mut self.currently_focused {
            if f.as_ref().is_some_and(out) {
                *f = None;
            }
        }
        if self.cursor_hovered.as_ref().is_some_and(out) {
            self.cursor_hovered = None;
        }
        let ids: Vec<String> = self.modals.iter().filter(|(_, w)| out(w)).map(|(k, _)| k.clone()).collect();
        for id in ids {
            self.modals.shift_remove(&id);
            self.modal_times.remove(&id);
        }
        self.context_menus.retain(|w| !out(w));
        self.dialog_stack.retain(|d| !out(&d.0));
        self.notifications.retain(|w| !out(w));
        if self.waiting.as_ref().is_some_and(out) {
            self.waiting = None;
        }
        self.transition_started.remove_owners(gone);
        self.transition_complete.remove_owners(gone);
        self.on_key_input.remove_owners(gone);
        self.on_scroll.remove_owners(gone);
        self.on_scroll_horizontal.remove_owners(gone);
        self.on_reset_key.remove_owners(gone);
        self.on_request_switch_item.remove_owners(gone);
        self.on_request_switch_page.remove_owners(gone);
        self.on_submit.remove_owners(gone);
        self.on_submit2.remove_owners(gone);
        self.on_submit3.remove_owners(gone);
        self.on_submit5.remove_owners(gone);
        self.on_cancel.remove_owners(gone);
        self.on_cancel_within_modal.remove_owners(gone);
        self.modal_shown.remove_owners(gone);
        self.modal_dismissed.remove_owners(gone);
        self.on_interactor_hover.remove_owners(gone);
        self.on_interactor_focused.remove_owners(gone);
        self.on_timer_event.remove_owners(gone);
        self.ui_dialog_created.remove_owners(gone);
        self.ui_notification_created.remove_owners(gone);
    }

    // ---------- the interactors' state ----------

    /// Interactor `w`'s state (None: `w` is no UI_Interactor).
    pub fn interactor(&self, w: WidgetId) -> Option<&InteractorState> {
        self.states.get(&w)
    }

    pub fn interactor_mut(&mut self, w: WidgetId) -> Option<&mut InteractorState> {
        self.states.get_mut(&w)
    }

    /// An interactor's state made (its controller's OnInitialized).
    pub fn add_interactor_state(&mut self, w: WidgetId, s: InteractorState) {
        self.states.insert(w, s);
    }

    /// The notifications on screen, oldest first (GetAllWidgetsOfClass UI_Notification of this view).
    pub fn notifications_shown(&self) -> &[WidgetId] {
        &self.notifications
    }

    // ---------- UI_Functions ----------

    /// The interactor can take the focus: enabled, on the viewport, visible in its hierarchy.
    pub fn is_focusable(cx: &Cx<'_>, i: WidgetId) -> bool {
        cx.view.manager.states.get(&i).is_some_and(InteractorState::is_enabled)
            && cx.view.in_viewport(i)
            && cx.view.visible_in_hierarchy(i)
    }

    fn is_focusable_opt(cx: &Cx<'_>, i: Option<WidgetId>) -> bool {
        i.is_some_and(|i| Manager::is_focusable(cx, i))
    }

    pub fn is_navigation_candidate(cx: &Cx<'_>, i: Option<WidgetId>) -> bool {
        let Some(i) = i else { return false };
        Manager::is_focusable(cx, i)
            && cx.view.manager.states.get(&i).is_some_and(|s| s.navigable && s.get_interactable())
    }

    /// Ready when the interval has passed since the timer (or the timer is ahead of the clock).
    fn interval_ready(now: f64, timer: f64, interval: f64) -> bool {
        let d = now - timer;
        d >= interval || d < 0.0
    }

    // ---------- contexts ----------

    pub fn get_current_context(&self, player: i32) -> &str {
        &self.current_context[player.max(0) as usize]
    }

    pub fn set_root_context_for_level(&mut self, ctx: &str) {
        if self.context_stack.is_empty() {
            self.context_stack.push(ctx.to_string());
        } else {
            self.context_stack[0] = ctx.to_string();
        }
    }

    fn set_context(&mut self, p: usize, ctx: &str, update_previous: bool) {
        if !valid_guid(ctx) || ctx == self.ignore_context {
            return;
        }
        if update_previous && self.context_stack.last().is_some_and(|l| l != ctx) {
            self.context_stack.push(ctx.to_string());
        }
        self.current_context[p] = ctx.to_string();
    }

    pub fn set_current_context(cx: &mut Cx<'_>, player: i32, ctx: &str, update_previous: bool) {
        if player < 0 {
            return;
        }
        cx.view.manager.set_context(player as usize, ctx, update_previous);
    }

    pub fn remove_context(cx: &mut Cx<'_>, ctx: &str) {
        let m = &mut cx.view.manager;
        if valid_guid(ctx) && m.context_stack.len() > 1 {
            if let Some(i) = m.context_stack.iter().position(|c| c == ctx) {
                m.context_stack.remove(i);
            }
            m.last_focused_in_context.remove(ctx);
        }
        for k in (0..cx.view.manager.interactors.len()).rev() {
            let Some(&i) = cx.view.manager.interactors.get(k) else { continue };
            if cx.view.manager.states.get(&i).is_some_and(|s| s.context == ctx) {
                Manager::unregister_interactor(cx, i);
            }
        }
    }

    pub fn fallback_to_previous_context(cx: &mut Cx<'_>, player: i32) -> bool {
        let m = &mut cx.view.manager;
        let Some(last) = m.context_stack.last().cloned() else { return false };
        m.set_context(player.max(0) as usize, &last, false);
        if m.context_stack.len() > 1 {
            m.context_stack.pop();
        }
        true
    }

    /// IsInCurrentContext(i, true if nothing focused) for `player` (< 0: the interactor's own, at least 0).
    pub fn is_in_current_context(cx: &Cx<'_>, i: WidgetId, true_if_nothing_focused: bool, player: i32) -> bool {
        let m = &cx.view.manager;
        let Some(s) = m.states.get(&i) else { return false };
        let p = if player < 0 { s.p() } else { player as usize };
        if true_if_nothing_focused && m.currently_focused[p].is_none() {
            return true;
        }
        s.context == m.current_context[p]
    }

    // ---------- scroll boxes ----------

    pub fn register_scrollbox(&mut self, sb: WidgetId) {
        if !self.scrollboxes.contains(&sb) {
            self.scrollboxes.push(sb);
        }
    }

    pub fn unregister_scrollbox(&mut self, sb: WidgetId) {
        self.scrollboxes.retain(|&s| s != sb);
    }

    // ---------- interactors ----------

    pub fn register_interactor(&mut self, i: WidgetId) {
        if !self.interactors.contains(&i) {
            self.interactors.insert(0, i);
        }
    }

    pub fn unregister_interactor(cx: &mut Cx<'_>, i: WidgetId) {
        let m = &mut cx.view.manager;
        if let Some(at) = m.interactors.iter().position(|&x| x == i) {
            m.interactors.remove(at);
        }
        for p in 0..=3 {
            Manager::set_unfocused(cx, i, p);
        }
        cx.view.manager.last_focused_in_context.retain(|_, v| *v != i);
    }

    fn in_context(&self, ctx: &str, p: i32) -> Vec<WidgetId> {
        self.interactors
            .iter()
            .copied()
            .filter(|i| self.states.get(i).is_some_and(|s| s.context == ctx && (s.player_index == p || s.all_players)))
            .collect()
    }

    pub fn get_interactors_in_current_context(cx: &mut Cx<'_>, player: i32) -> Vec<WidgetId> {
        let m = &cx.view.manager;
        let r = m.in_context(&m.current_context[player.max(0) as usize].clone(), player);
        for &i in &r {
            interactor::update_position(cx, i);
        }
        r
    }

    pub fn get_currently_focused(&self, player: i32) -> Option<WidgetId> {
        if player < 0 {
            return None;
        }
        self.currently_focused[player as usize]
    }

    fn set_currently_focused(&mut self, p: usize, i: Option<WidgetId>) {
        self.currently_focused[p] = i;
        if let Some(i) = i
            && let Some(ctx) = self.states.get(&i).map(|s| s.context.clone())
            && valid_guid(&ctx)
        {
            self.last_focused_in_context.insert(ctx, i);
        }
    }

    /// Player 0's last interaction (what the window keeps between this view's frames and the other views').
    pub fn interaction(&self) -> Interaction {
        Interaction {
            last_type: self.last_interaction_type[0],
            input_type: self.input_type[0],
            cursor_mode: self.cursor_mode[0],
        }
    }

    /// Player 0's last interaction as the window has it. cursorMode is written as it stands: ToggleCursorMode's
    /// LastMouseX / Y follow at the next UpdateMouseDelta.
    pub fn set_interaction(&mut self, i: Interaction) {
        self.last_interaction_type[0] = i.last_type;
        self.input_type[0] = i.input_type;
        self.cursor_mode[0] = i.cursor_mode && self.allow_cursor_mode;
    }

    /// RealMouseActive: cursor mode with the keyboard or the mouse the last interaction.
    pub fn real_mouse_active(&self, p: usize) -> bool {
        self.cursor_mode[p] && (self.last_interaction_type[p] == 1 || self.last_interaction_type[p] == 2)
    }

    pub fn set_focused(cx: &mut Cx<'_>, i: WidgetId) {
        let Some(s) = cx.view.manager.states.get(&i) else { return };
        let (pi, all) = (s.player_index, s.all_players);
        let p0 = pi.max(0) as usize;
        let cur = cx.view.manager.currently_focused[p0];
        if cur == Some(i) && !cx.view.manager.real_mouse_active(p0) && Manager::is_focusable(cx, i) && !all {
            return;
        }
        if Manager::is_in_current_context(cx, i, true, -1) {
            for p in 0..=3 {
                if p != pi && !all {
                    continue;
                }
                Manager::unfocus_all(cx, p);
                interactor::fire_focus(cx, i, p);
                cx.view.manager.set_currently_focused(p as usize, Some(i));
            }
        } else {
            let m = &mut cx.view.manager;
            if let Some(at) = m.interactors.iter().position(|&x| x == i) {
                m.interactors.remove(at);
                m.interactors.push(i);
            }
        }
    }

    pub fn set_unfocused(cx: &mut Cx<'_>, i: WidgetId, player: i32) {
        let m = &cx.view.manager;
        let Some(s) = m.states.get(&i) else { return };
        let p = player as usize;
        if (s.player_index < 0 || s.player_index == player) && m.currently_focused[p] == Some(i) {
            interactor::fire_unfocus(cx, i, player);
            if cx.view.manager.cursor_mode[p] {
                cx.fire_manager(|m| &m.on_interactor_hover, &(i, false, player));
            }
        }
    }

    /// UnfocusAll: in cursor mode the interactor the cursor hovers (GetCursor(p).currentlyHovered), when it is in
    /// the current context (IsInCurrentContext with trueIfNothingFocused false), is skipped and stays the focused
    /// one; SetCurrentlyFocused(null) runs only when none was kept.
    pub fn unfocus_all(cx: &mut Cx<'_>, player: i32) {
        if player < 0 {
            return;
        }
        let p = player as usize;
        let m = &cx.view.manager;
        let hovered = if m.cursor_mode[p] && p == 0 { m.cursor_hovered } else { None };
        let mut kept = false;
        for i in m.interactors.clone() {
            if hovered == Some(i) && Manager::is_in_current_context(cx, i, false, -1) {
                kept = true;
            } else {
                Manager::set_unfocused(cx, i, player);
            }
        }
        if !kept {
            cx.view.manager.set_currently_focused(p, None);
        }
    }

    pub fn restore_focused(cx: &mut Cx<'_>, player: i32) {
        let m = &cx.view.manager;
        if m.text_entry_mode || player < 0 {
            return;
        }
        let p = player as usize;
        let cur = m.currently_focused[p];
        if Manager::is_focusable_opt(cx, cur) {
            return;
        }
        let m = &cx.view.manager;
        let last = m.last_focused_in_context.get(&m.current_context[p]).copied();
        if let Some(l) = last
            && Manager::is_focusable(cx, l)
            && Some(l) != cur
            && cx
                .view
                .manager
                .states
                .get(&l)
                .is_some_and(|s| !s.prevent_fallback_focus && (s.player_index < 0 || s.player_index == player))
        {
            let cm = cx.view.manager.cursor_mode[p];
            interactor::set_focused(cx, l, cm);
            return;
        }
        Manager::fallback_to_previous_context(cx, player);
        let set = Manager::get_interactors_in_current_context(cx, player);
        for &i in set.iter().rev() {
            let pff = cx.view.manager.states.get(&i).is_some_and(|s| s.prevent_fallback_focus);
            if !pff && Manager::is_focusable(cx, i) && Some(i) != cx.view.manager.currently_focused[p] {
                let cm = cx.view.manager.cursor_mode[p];
                interactor::set_focused(cx, i, cm);
                return;
            }
        }
    }

    // ---------- navigation ----------

    pub fn navigate(cx: &mut Cx<'_>, dir: Nav, player: i32) {
        let (dx, dy) = dir.d();
        Manager::navigate_d(cx, dx, dy, player);
    }

    fn navigate_d(cx: &mut Cx<'_>, dx: f64, dy: f64, player: i32) {
        let p = player.max(0) as usize;
        if cx.view.manager.cursor_mode[p] || cx.view.manager.is_waiting {
            return;
        }
        let set = Manager::get_interactors_in_current_context(cx, player);
        let cur = cx.view.manager.currently_focused[p];
        let closest = match cur.and_then(|c| cx.view.manager.states.get(&c).map(|s| (c, s.navigable, s.manual()))) {
            Some((c, true, [ml, mr, mu, md])) => {
                if dx > 0.0 && Manager::is_navigation_candidate(cx, mr) {
                    mr
                } else if dx < 0.0 && Manager::is_navigation_candidate(cx, ml) {
                    ml
                } else if dy < 0.0 && Manager::is_navigation_candidate(cx, mu) {
                    mu
                } else if dy > 0.0 && Manager::is_navigation_candidate(cx, md) {
                    md
                } else {
                    let _ = c;
                    Manager::nearest_widget_in_direction(cx, p, dx, dy)
                }
            }
            _ => {
                // Navigate @795: the first focusable interactor of the context, in the array's order (the newest
                // first). Deviation, before it: a screen's navigationStart.
                let ctx = cx.view.manager.current_context[p].clone();
                let start = match cx.view.manager.navigation_start.get(&ctx).copied() {
                    Some(h) => cx.visit_dyn(h.owner, |c, cx| c.navigation_start(cx, h.id)).flatten(),
                    None => None,
                };
                if Manager::is_focusable_opt(cx, start) {
                    start
                } else {
                    set.iter().copied().find(|&i| Manager::is_focusable(cx, i))
                }
            }
        };
        let Some(closest) = closest else { return };
        if cx.view.manager.currently_focused[p] != Some(closest) {
            interactor::set_focused(cx, closest, false);
        } else {
            interactor::fire_focus(cx, closest, player);
        }
    }

    fn nearest_widget_in_direction(cx: &mut Cx<'_>, p: usize, dx: f64, dy: f64) -> Option<WidgetId> {
        let m = &cx.view.manager;
        let start = m.currently_focused[p]?;
        let ss = m.states.get(&start)?;
        let s = ss.position;
        let group: Vec<WidgetId> = m
            .in_context(&ss.context.clone(), ss.player_index)
            .into_iter()
            .filter(|&c| {
                if c == start || !Manager::is_focusable(cx, c) {
                    return false;
                }
                let Some(cs) = cx.view.manager.states.get(&c) else { return false };
                if !cs.navigable {
                    return false;
                }
                let q = cs.position;
                if !(q[0] > 0.0 || q[1] > 0.0) {
                    return false;
                }
                (dx > 0.0 && q[0] > s[0])
                    || (dx < 0.0 && q[0] < s[0])
                    || (dy > 0.0 && q[1] > s[1])
                    || (dy < 0.0 && q[1] < s[1])
            })
            .collect();
        if group.is_empty() {
            return Some(start);
        }
        let group: Vec<WidgetId> = group
            .into_iter()
            .filter(|c| {
                let q = cx.view.manager.states[c].position;
                !(((q[1] - s[1]).abs() < 10.0 && dy != 0.0) || ((q[0] - s[0]).abs() < 10.0 && dx != 0.0))
            })
            .collect();
        let mut closest = -1.0;
        let mut closest_base = 0.0;
        let mut best_exact = false;
        let mut current = None;
        for c in group {
            let d = distance_accounting_for_size(cx.view, start, c, dx, dy);
            let exact = (d.dot - 1.0).abs() <= 0.001;
            // Ease(3.0, 1.1, Dot, Linear)
            let score = d.dist * if exact { 1.0 } else { 3.0 + (1.1 - 3.0) * d.dot };
            let better = score < closest
                || closest < 0.0
                || (d.base < closest_base && score <= closest)
                || (exact && !best_exact);
            if d.valid && better && (exact || !best_exact) {
                best_exact = exact;
                closest = score;
                closest_base = d.base;
                current = Some(c);
            }
        }
        current
    }

    // ---------- input ----------

    /// ShowMouseCursor(Show, Player Index) @73-175: PC[PI].bShowMouseCursor = (PI == 0 && Show); the only shown
    /// pointer is player 0's, so a player above 0 leaves it as it is.
    fn show_mouse_cursor(cx: &mut Cx<'_>, show: bool, p: usize) {
        if p != 0 {
            return;
        }
        cx.view.show_cursor(show);
    }

    /// ToggleCursorMode: when the mode turns on with a navigation candidate focused, LastMouseX / Y take the
    /// cursor's position @399-621. The root canvas's render translation reset @949 is not ported: no graph sets
    /// rootCanvas (none in UI_Manager's tree), so its IsValid @906 skips it.
    /// Deviation: BPI_SetLastCursorMode @860 keeps the mode on the GameInstance for the next level's manager
    /// (Construct @8272-8318); the port's window pointer carries `cursor_mode` from screen to screen instead
    /// (`set_interaction`), which also keeps the mode SetCursorMode wrote without a toggle.
    fn toggle_cursor_mode(cx: &mut Cx<'_>, on: bool, p: usize) {
        let to = on && cx.view.manager.allow_cursor_mode;
        if !cx.view.manager.cursor_mode[p]
            && to
            && Manager::is_navigation_candidate(cx, cx.view.manager.currently_focused[p])
        {
            let at = if p == 0 { cx.view.pointer() } else { None };
            // (Get Position without success gives 0, 0)
            let m = &mut cx.view.manager;
            m.last_mouse_x[p] = at.map_or(0.0, |a| a[0]);
            m.last_mouse_y[p] = at.map_or(0.0, |a| a[1]);
        }
        cx.view.manager.cursor_mode[p] = to;
    }

    /// UpdateCursorMode(delta, p): a pointer that moved by 0.01 or more turns cursor mode on and makes the mouse
    /// the input type.
    fn update_cursor_mode(cx: &mut Cx<'_>, delta: f64, p: usize) {
        if delta.abs() < 0.01 {
            return;
        }
        Manager::toggle_cursor_mode(cx, true, p);
        cx.view.manager.input_type[p] = 2;
    }

    /// UpdateMouseDelta (Tick, while handleInput): the cursor's position against the one of the last call. The
    /// stick moves that position, so a moving stick cursor is a moving mouse here: cursor mode on, InputType 2;
    /// LastInteractionType is not written. The port's own: a pointer not known yet is skipped.
    fn update_mouse_delta(cx: &mut Cx<'_>) {
        let Some(at) = cx.view.pointer() else { return };
        let m = &cx.view.manager;
        let d = (at[0] - m.last_mouse_x[0]).abs().max((at[1] - m.last_mouse_y[0]).abs());
        Manager::update_cursor_mode(cx, d, 0);
        let m = &mut cx.view.manager;
        m.last_mouse_x[0] = at[0];
        m.last_mouse_y[0] = at[1];
    }

    /// Process Analog Value Changed for one axis key of a pad (Gamepad_LeftX / LeftY / RightX / RightY, Y up
    /// positive): SetAnalogMovement and Detect Scroll; while waiting or transitioning the stick values are zeroed
    /// and the event dropped. `frames` (the port's own): the polled frame's length in 60 Hz frames, the scroll
    /// amount scaled by it.
    pub fn analog_value(cx: &mut Cx<'_>, key: &str, value: f64, player: i32, frames: f64) {
        let now = cx.now();
        let m = &mut cx.view.manager;
        if m.is_waiting || m.is_transitioning() {
            m.move_x = [0.0; 4];
            m.move_y = [0.0; 4];
            m.last_cursor_move_time_x = [0.0; 4];
            m.last_cursor_move_time_y = [0.0; 4];
            return;
        }
        if player < 0 {
            return;
        }
        let p = player as usize;
        // (the port's own) the event's time is the clock's now, not InternalTime
        if key == "Gamepad_LeftX" {
            m.move_x[p] = if value.abs() > ANALOG_DEAD_ZONE { value } else { 0.0 };
            m.last_cursor_move_time_x[p] = now;
        } else if key == "Gamepad_LeftY" {
            m.move_y[p] = -(if value.abs() > ANALOG_DEAD_ZONE { value } else { 0.0 });
            m.last_cursor_move_time_y[p] = now;
        }
        // For a local player above 0 the left stick past 0.25 is a D-pad press of that player. Unverified:
        // SimulateGamepadDpadKeyPress is native and not read: taken as 0 Up, 1 Left, 2 Right, 3 Down, one key down
        // of that D-pad key per event, no key up.
        if p > 0 && value.abs() > ANALOG_DEAD_ZONE && (key == "Gamepad_LeftX" || key == "Gamepad_LeftY") {
            let k = if key == "Gamepad_LeftX" {
                if value > 0.0 { "Gamepad_DPad_Right" } else { "Gamepad_DPad_Left" }
            } else if value > 0.0 {
                "Gamepad_DPad_Up"
            } else {
                "Gamepad_DPad_Down"
            };
            Manager::key_down(cx, k, player);
        }
        // Detect Scroll: the right stick past 0.15 @74; cursor mode goes on directly @613.
        if value.abs() > SCROLL_DEAD_ZONE && (key == "Gamepad_RightY" || key == "Gamepad_RightX") {
            let a = (player, if value > 0.0 { 1 } else { -1 }, value * frames);
            if key == "Gamepad_RightY" {
                cx.fire_manager(|m| &m.on_scroll, &a);
            } else {
                cx.fire_manager(|m| &m.on_scroll_horizontal, &a);
            }
            let m = &mut cx.view.manager;
            m.last_scroll_time = m.internal_time;
            m.cursor_mode[p] = true;
        }
    }

    /// UpdateAnalogMove: a stick value no event refreshed starts to go to 0 at 0.033 s after its event and is 0
    /// at 0.066 s.
    fn update_analog_move(&mut self) {
        let clamp01 = |v: f64| v.clamp(0.0, 1.0);
        for p in 0..4 {
            if self.move_x[p] != 0.0 {
                self.move_x[p] *=
                    1.0 - clamp01((self.internal_time - (self.last_cursor_move_time_x[p] + 0.033)) / 0.033);
            }
            if self.move_y[p] != 0.0 {
                self.move_y[p] *=
                    1.0 - clamp01((self.internal_time - (self.last_cursor_move_time_y[p] + 0.033)) / 0.033);
            }
        }
    }

    fn determine_cursor_mode(cx: &mut Cx<'_>, key: &str, p: usize) {
        if has(LEFT_STICK, key) {
            Manager::toggle_cursor_mode(cx, true, p);
            cx.view.manager.last_interaction_type[p] = 0;
        } else if (!is_gamepad_key(key) && key != "Escape" && !has(KEYS.submit2, key)) || has(DPAD, key) {
            Manager::toggle_cursor_mode(cx, false, p);
        }
    }

    /// UI_Manager.ProcessPreviewKeyDown: true when the manager took the key. The key's repeat flag is not read.
    pub fn key_down(cx: &mut Cx<'_>, key: &str, player: i32) -> bool {
        let m = &mut cx.view.manager;
        if has(KEYS.ignore, key) || m.is_waiting || m.is_transitioning() || !m.handle_input || player < 0 {
            return false;
        }
        let p = player as usize;
        let pad = is_gamepad_key(key);
        let plain = !pad && key != "Escape" && !has(KEYS.submit2, key);
        if pad {
            m.last_interaction_type[p] = 0;
        } else if plain {
            m.last_interaction_type[p] = 1;
        }
        if pad && !has(LEFT_STICK, key) {
            m.input_type[p] = 0;
        } else if plain {
            m.input_type[p] = 1;
        }
        Manager::process_input(cx, key, p)
    }

    /// ProcessPreviewKeyDown on a screen whose keys only name the device (`MenuScreen::pointer_only`, the title):
    /// LastInteractionType, InputType and DetermineCursorMode as `key_down` writes them; no OnKeyInput, no
    /// navigation, no submit.
    pub fn key_seen(cx: &mut Cx<'_>, key: &str, player: i32) {
        let m = &mut cx.view.manager;
        if has(KEYS.ignore, key) || m.is_waiting || m.is_transitioning() || !m.handle_input || player < 0 {
            return;
        }
        let p = player as usize;
        let pad = is_gamepad_key(key);
        let plain = !pad && key != "Escape" && !has(KEYS.submit2, key);
        if pad {
            m.last_interaction_type[p] = 0;
        } else if plain {
            m.last_interaction_type[p] = 1;
        }
        if pad && !has(LEFT_STICK, key) {
            m.input_type[p] = 0;
        } else if plain {
            m.input_type[p] = 1;
        }
        if !pad && cx.view.manager.text_entry_mode {
            return;
        }
        Manager::determine_cursor_mode(cx, key, p);
    }

    /// A mouse button on a `pointer_only` screen: LastInteractionType alone, as `mouse_down` writes it.
    pub fn mouse_seen(cx: &mut Cx<'_>, player: i32) {
        if player < 0 || !cx.view.manager.handle_input {
            return;
        }
        cx.view.manager.last_interaction_type[player as usize] = 2;
    }

    fn process_input(cx: &mut Cx<'_>, key: &str, p: usize) -> bool {
        let pl = p as i32;
        cx.fire_manager(|m| &m.on_key_input, &(pl, key.to_string()));
        if cx.view.manager.is_transitioning() {
            return false;
        }
        if !is_gamepad_key(key) && cx.view.manager.text_entry_mode {
            return false;
        }
        Manager::determine_cursor_mode(cx, key, p);
        let now = cx.now();
        if is_nav_key(key) && Manager::interval_ready(now, cx.view.manager.navigation_timer[p], NAVIGATION_INTERVAL) {
            cx.view.manager.navigation_timer[p] = now;
            let dx = if has(KEYS.right, key) {
                1.0
            } else if has(KEYS.left, key) {
                -1.0
            } else {
                0.0
            };
            let dy = if has(KEYS.down, key) {
                1.0
            } else if has(KEYS.up, key) {
                -1.0
            } else {
                0.0
            };
            Manager::navigate_d(cx, dx, dy, pl);
            Manager::show_mouse_cursor(cx, false, p);
            return true;
        }
        if has(KEYS.submit, key) && Manager::interaction_ready(cx, p) {
            Manager::submit(cx, pl);
            Manager::show_mouse_cursor(cx, false, p);
            return true;
        }
        if has(KEYS.cancel, key) && Manager::interaction_ready(cx, p) {
            Manager::cancel(cx, pl, key == "Escape");
            if key == "Escape" {
                return true;
            }
            Manager::show_mouse_cursor(cx, false, p);
            return false;
        }
        if has(KEYS.page_right, key) {
            Manager::request_switch_page(cx, pl, 1);
        } else if has(KEYS.page_left, key) {
            Manager::request_switch_page(cx, pl, -1);
        } else if has(KEYS.scroll_up, key) {
            Manager::scroll(cx, p, 1, 10.0);
        } else if has(KEYS.scroll_down, key) {
            Manager::scroll(cx, p, -1, -10.0);
        } else if has(KEYS.item_right, key) {
            Manager::request_switch_item(cx, pl, 1);
        } else if has(KEYS.item_left, key) {
            Manager::request_switch_item(cx, pl, -1);
        } else if has(KEYS.submit2, key) {
            cx.fire_manager(|m| &m.on_submit2, &pl);
        } else if has(KEYS.submit3, key) {
            cx.fire_manager(|m| &m.on_submit3, &pl);
        } else if has(KEYS.submit5, key) {
            Manager::submit(cx, pl);
            cx.fire_manager(|m| &m.on_submit5, &pl);
        } else if has(KEYS.reset, key) {
            cx.fire_manager(|m| &m.on_reset_key, &pl);
        }
        false
    }

    fn interaction_ready(cx: &mut Cx<'_>, p: usize) -> bool {
        let now = cx.now();
        let m = &mut cx.view.manager;
        if !Manager::interval_ready(now, m.interaction_timer[p], INTERACTION_INTERVAL) {
            return false;
        }
        m.interaction_timer[p] = now;
        true
    }

    fn scroll(cx: &mut Cx<'_>, p: usize, dir: i32, amount: f64) {
        cx.fire_manager(|m| &m.on_scroll, &(p as i32, dir, amount));
        let m = &mut cx.view.manager;
        m.last_scroll_time = m.internal_time;
        m.cursor_mode[0] = true;
        Manager::show_mouse_cursor(cx, false, p);
    }

    /// Key up: the throttle of the key's class made ready again (ResetInputThrottleTimer).
    pub fn key_up(cx: &mut Cx<'_>, key: &str, player: i32) {
        if player < 0 {
            return;
        }
        let p = player as usize;
        let m = &mut cx.view.manager;
        if is_nav_key(key) {
            m.navigation_timer[p] = m.internal_time - NAVIGATION_INTERVAL;
        } else {
            m.interaction_timer[p] = m.internal_time - INTERACTION_INTERVAL;
        }
    }

    pub fn submit(cx: &mut Cx<'_>, player: i32) {
        if cx.view.manager.is_waiting || cx.view.manager.is_transitioning() || player < 0 {
            return;
        }
        cx.fire_manager(|m| &m.on_submit, &player);
        if let Some(cur) = cx.view.manager.currently_focused[player as usize] {
            interactor::submit(cx, cur, player);
        }
    }

    pub fn cancel(cx: &mut Cx<'_>, player: i32, is_escape: bool) {
        if cx.view.manager.is_waiting || cx.view.manager.is_transitioning() {
            return;
        }
        if cx.view.manager.has_active_context_menus() {
            Manager::dismiss_all_context_menus(cx);
            return;
        }
        let modal = Manager::latest_modal(cx.view);
        // no modal, or a dialog over one: the dialog (or the page) takes the cancel
        if modal.is_none() || Manager::is_dialog_active(cx.view) {
            cx.fire_manager(|m| &m.on_cancel, &(player, is_escape));
            cx.post("ui_menu_global_back_escape");
            return;
        }
        cx.fire_manager(|m| &m.on_cancel_within_modal, &player);
        if player == 0
            && let Some((_, w)) = modal
        {
            cx.post("ui_menu_global_back_escape");
            cx.visit_dyn(w, |c, cx| {
                if let Some(m) = c.as_modal() {
                    m.request_dismiss(cx, "", true);
                }
            });
        }
    }

    fn request_switch_page(cx: &mut Cx<'_>, p: i32, dir: i32) {
        let m = &cx.view.manager;
        if !m.is_waiting && !m.is_transitioning() && !m.has_active_context_menus() {
            cx.fire_manager(|m| &m.on_request_switch_page, &(p, dir));
        }
    }

    fn request_switch_item(cx: &mut Cx<'_>, p: i32, dir: i32) {
        let m = &cx.view.manager;
        if !m.is_waiting && !m.is_transitioning() && !m.has_active_context_menus() {
            cx.fire_manager(|m| &m.on_request_switch_item, &(p, dir));
        }
    }

    /// The mouse moved by `dist` pixels (ProcessMouseMove): UpdateCursorMode @214, then the mouse as the last
    /// interaction when the move is longer than 1.0.
    pub fn mouse_move(cx: &mut Cx<'_>, dist: f64, player: i32) {
        if player < 0 {
            return;
        }
        let p = player as usize;
        Manager::update_cursor_mode(cx, dist, p);
        if dist.abs() > 1.0 {
            cx.view.manager.last_interaction_type[p] = 2;
            Manager::show_mouse_cursor(cx, true, p);
        }
    }

    /// A mouse button (0 left, 2 right).
    pub fn mouse_down(cx: &mut Cx<'_>, button: u8, player: i32) {
        if player < 0 {
            return;
        }
        let p = player as usize;
        let m = &mut cx.view.manager;
        m.interaction_timer[p] = m.internal_time - INTERACTION_INTERVAL;
        if !m.handle_input {
            return;
        }
        if button == 2 {
            Manager::cancel(cx, player, false);
        } else if button == 0 {
            Manager::handle_left_click(cx, p);
        }
        cx.view.manager.last_interaction_type[p] = 2;
    }

    fn handle_left_click(cx: &mut Cx<'_>, p: usize) {
        let now = cx.now();
        if !Manager::interval_ready(now, cx.view.manager.mouse_click_timer, MOUSE_CLICK_INTERVAL) {
            return;
        }
        cx.view.manager.mouse_click_timer = now;
        if let Some(cur) = cx.view.manager.currently_focused[p]
            && cx.view.manager.states.get(&cur).is_some_and(InteractorState::is_hovered)
            && cx.view.visible_in_hierarchy(cur)
        {
            Manager::submit(cx, p as i32);
        }
    }

    // ---------- modals ----------

    pub fn get_modal(&self, id: &str) -> Option<WidgetId> {
        self.modals.get(id).copied()
    }

    /// The modal shown last of those visible in their hierarchy: its id and widget.
    fn latest_modal(view: &MenuView) -> Option<(String, WidgetId)> {
        let m = &view.manager;
        let mut best = None;
        let mut t = f64::NEG_INFINITY;
        for (id, &w) in &m.modals {
            let mt = m.modal_times.get(id).copied().unwrap_or(0.0);
            if view.visible_in_hierarchy(w) && mt >= t {
                best = Some((id.clone(), w));
                t = mt;
            }
        }
        best
    }

    pub fn is_modal_active(view: &MenuView) -> bool {
        Manager::latest_modal(view).is_some()
    }

    /// IsModalActive?'s Latest Modal ID ('' without one).
    pub fn latest_modal_id(view: &MenuView) -> String {
        Manager::latest_modal(view).map(|(id, _)| id).unwrap_or_default()
    }

    /// (the port's own) whether a viewport widget is a modal's.
    pub fn is_modal(&self, w: WidgetId) -> bool {
        self.modals.values().any(|&m| m == w)
    }

    /// The modal of class `cls` made, shown with `id` and pushed (None: one of that id is up). Its controller:
    /// `MenuView::modal`.
    pub fn show_modal(cx: &mut Cx<'_>, cls: &str, id: &str, instructions: &str, extra_data: &str) -> Option<WidgetId> {
        if cx.view.manager.modals.contains_key(id) {
            return None;
        }
        let w = MenuView::create(cx, cls);
        let now = cx.now();
        let m = &mut cx.view.manager;
        m.modals.insert(id.to_string(), w);
        m.modal_times.insert(id.to_string(), now);
        cx.visit_dyn(w, |c, cx| {
            if let Some(m) = c.as_modal() {
                m.pre_show(cx, id);
            }
        });
        MenuView::add_to_viewport(cx, w, 3);
        cx.visit_dyn(w, |c, cx| {
            if let Some(m) = c.as_modal() {
                m.post_show(cx, instructions, extra_data);
            }
        });
        cx.fire_manager(|m| &m.modal_shown, &id.to_string());
        Some(w)
    }

    pub fn dismiss_modal(cx: &mut Cx<'_>, id: &str) {
        let Some(w) = cx.view.manager.modals.get(id).copied() else { return };
        if !cx.view.visible_in_hierarchy(w) {
            return;
        }
        // Deviation: a modal that dismisses itself (its Back, its own button) is busy in that call, so it is
        // dismissed once the call returns, every step on its controller in hand (controller.rs: a deferred call
        // runs when the busy method returns).
        if cx.view.is_busy(w) {
            let id = id.to_string();
            cx.run(
                w,
                Box::new(move |c, cx| {
                    if cx.view.manager.modals.get(&id) == Some(&w) && cx.view.visible_in_hierarchy(w) {
                        Manager::dismiss_held(cx, &id, w, Some(c));
                    }
                }),
            );
            return;
        }
        Manager::dismiss_held(cx, id, w, None);
    }

    /// DismissModal's steps on modal `w`; `held`: its controller when the caller has it in hand (the modal's own
    /// deferred call), else it is reached through the view.
    fn dismiss_held(cx: &mut Cx<'_>, id: &str, w: WidgetId, mut held: Option<&mut dyn Controller>) {
        fn on_modal<R>(
            cx: &mut Cx<'_>,
            w: WidgetId,
            held: Option<&mut dyn Controller>,
            f: impl FnOnce(&mut dyn Modal, &mut Cx<'_>) -> R,
        ) -> Option<R> {
            match held {
                Some(c) => c.as_modal().map(|m| f(m, cx)),
                None => cx.visit_dyn(w, |c, cx| c.as_modal().map(|m| f(m, cx))).flatten(),
            }
        }
        on_modal(cx, w, held.as_deref_mut(), |m, cx| m.pre_dismiss(cx));
        MenuView::remove_from_parent(cx, w);
        // (the calls the removal deferred for the modal in hand, its Destruct among them: run now)
        if let Some(c) = held.as_deref_mut() {
            while let Some(f) = cx.view.take_deferred(w) {
                f(&mut *c, cx);
            }
        }
        let m = &mut cx.view.manager;
        m.modals.shift_remove(id);
        m.modal_times.remove(id);
        cx.fire_manager(|m| &m.modal_dismissed, &id.to_string());
        let ctx = on_modal(cx, w, held, |m, cx| {
            m.post_dismiss(cx);
            m.get_modal_context()
        });
        let m = &mut cx.view.manager;
        m.last_modal_time = m.internal_time;
        if let Some(ctx) = ctx {
            Manager::remove_context(cx, &ctx);
        }
        // (each ShowModal makes its modal anew and the table held it alone: it goes for good)
        MenuView::discard(cx, w);
    }

    pub fn dismiss_current_modal(cx: &mut Cx<'_>) {
        if let Some((id, _)) = Manager::latest_modal(cx.view) {
            Manager::dismiss_modal(cx, &id);
        }
    }

    pub fn dismiss_all_modals(cx: &mut Cx<'_>) {
        let ids: Vec<String> = cx.view.manager.modals.keys().cloned().collect();
        for id in ids {
            Manager::dismiss_modal(cx, &id);
        }
    }

    /// Dismiss All Modals with Exceptions: DismissModal on every key the list does not hold, from the last key
    /// to the first.
    pub fn dismiss_all_modals_except(cx: &mut Cx<'_>, except: &[&str]) {
        let ids: Vec<String> = cx.view.manager.modals.keys().rev().cloned().collect();
        for id in ids {
            if !except.contains(&id.as_str()) {
                Manager::dismiss_modal(cx, &id);
            }
        }
    }

    // ---------- context menus ----------

    /// ShowContextMenu: Init runs before the menu is added (AddWidgetToView without auto size, the root canvas
    /// at ZOrder 5): its widget and each button's interactor by id.
    pub fn show_context_menu(
        cx: &mut Cx<'_>,
        params: ContextMenuParams,
        player: i32,
    ) -> (WidgetId, Vec<(String, WidgetId)>) {
        let w = MenuView::create(cx, "UI_ContextMenu");
        let interactors = cx
            .visit_dyn(w, |c, cx| c.as_context_menu().map(|m| m.init(cx, params, player)))
            .flatten()
            .unwrap_or_default();
        MenuView::add_to_view(cx, w, false, 5);
        cx.view.manager.context_menus.push(w);
        (w, interactors)
    }

    /// HideContextMenu called by the menu itself (its Hide Self): the menu's controller is the one running, so it
    /// hands itself in; `hide_context_menu` cannot reach a busy controller.
    pub fn hide_own_context_menu(cx: &mut Cx<'_>, m: &mut dyn ContextMenu, player: i32) {
        let w = cx.w;
        let guid = m.guid().to_string();
        Manager::remove_context(cx, &guid);
        m.hide(cx);
        cx.view.manager.context_menus.retain(|&x| x != w);
        if player >= 0 && cx.view.manager.cursor_mode[player as usize] {
            Manager::unfocus_all(cx, player);
        }
    }

    /// HideContextMenu: every menu with the id, latest first.
    pub fn hide_context_menu(cx: &mut Cx<'_>, id: &str, player: i32) {
        for k in (0..cx.view.manager.context_menus.len()).rev() {
            let Some(&w) = cx.view.manager.context_menus.get(k) else { continue };
            let Some((mid, guid)) = cx.view.context_menu(w).map(|m| (m.id().to_string(), m.guid().to_string())) else {
                continue;
            };
            if mid != id {
                continue;
            }
            Manager::remove_context(cx, &guid);
            cx.visit_dyn(w, |c, cx| {
                if let Some(m) = c.as_context_menu() {
                    m.hide(cx);
                }
            });
            cx.view.manager.context_menus.retain(|&x| x != w);
            if player >= 0 && cx.view.manager.cursor_mode[player as usize] {
                Manager::unfocus_all(cx, player);
            }
        }
    }

    /// DismissAllContextMenus: the menus' contexts stay on the stack.
    pub fn dismiss_all_context_menus(cx: &mut Cx<'_>) {
        for k in (0..cx.view.manager.context_menus.len()).rev() {
            let Some(&w) = cx.view.manager.context_menus.get(k) else { continue };
            cx.visit_dyn(w, |c, cx| {
                if let Some(m) = c.as_context_menu() {
                    m.hide(cx);
                }
            });
            cx.view.manager.context_menus.retain(|&x| x != w);
        }
    }

    pub fn has_active_context_menus(&self) -> bool {
        !self.context_menus.is_empty()
    }

    // ---------- dialogs, notifications, the waiting cover ----------

    pub fn set_handle_input(&mut self, on: bool) {
        self.handle_input = on;
    }

    /// ShowDialog: dialogs asked for within 0.5 s of the last one queue up 1 s apart. Its controller:
    /// `MenuView::dialog`.
    pub fn show_dialog(cx: &mut Cx<'_>, params: DialogParams) -> DialogId {
        let m = &mut cx.view.manager;
        m.dialog_delay_time = if m.internal_time - m.last_dialog_time <= 0.5 { m.dialog_delay_time + 1.0 } else { 0.0 };
        let delay = m.dialog_delay_time;
        let w = MenuView::create(cx, "UI_Dialog");
        cx.visit_dyn(w, |c, cx| {
            if let Some(d) = c.as_dialog() {
                d.delayed_init(cx, params, delay);
            }
        });
        cx.fire_manager(|m| &m.ui_dialog_created, &());
        let m = &mut cx.view.manager;
        m.last_dialog_time = m.internal_time;
        m.dialog_stack.push(DialogId(w));
        DialogId(w)
    }

    /// IsDialogActive: a dialog that is on screen (from its creation to its Destroy). Unverified: a dialog busy
    /// in its own call counts as on screen.
    pub fn is_dialog_active(view: &mut MenuView) -> bool {
        let ids = view.manager.dialog_stack.clone();
        ids.into_iter().any(|d| if view.is_busy(d.0) { true } else { view.dialog(d).is_some_and(|d| d.is_on_screen()) })
    }

    pub fn get_latest_active_dialog(view: &mut MenuView) -> Option<DialogId> {
        let ids = view.manager.dialog_stack.clone();
        ids.into_iter().rev().find(|&d| view.dialog(d).is_some_and(|d| d.is_on_screen() && !d.is_exiting()))
    }

    pub fn dismiss_all_dialogs(cx: &mut Cx<'_>) {
        for d in cx.view.manager.dialog_stack.clone() {
            cx.visit_dyn(d.0, |c, cx| {
                if let Some(d) = c.as_dialog() {
                    d.exit(cx);
                }
            });
        }
    }

    /// ShowNotification: Immediate ones are created now (the widget; its controller `MenuView::notification`),
    /// the others wait for ShowQueuedNotifications(their timing).
    pub fn show_notification(cx: &mut Cx<'_>, params: NotificationParams) -> Option<WidgetId> {
        if params.timing == 0 {
            return Some(Manager::create_and_add_notification(cx, params));
        }
        cx.view.manager.notification_queue.push(params);
        None
    }

    fn create_and_add_notification(cx: &mut Cx<'_>, params: NotificationParams) -> WidgetId {
        let w = MenuView::create(cx, "UI_Notification");
        cx.visit_dyn(w, |c, cx| {
            if let Some(n) = c.as_notification() {
                n.init(cx, params);
            }
        });
        MenuView::add_to_view(cx, w, true, 250);
        cx.view.manager.notifications.push(w);
        w
    }

    pub fn show_queued_notifications(cx: &mut Cx<'_>, timing: i32) {
        let q = std::mem::take(&mut cx.view.manager.notification_queue);
        let (due, rest): (Vec<_>, Vec<_>) = q.into_iter().partition(|p| p.timing == timing);
        cx.view.manager.notification_queue = rest;
        for p in due {
            Manager::create_and_add_notification(cx, p);
        }
    }

    pub fn dismiss_all_notifications(cx: &mut Cx<'_>) {
        for n in cx.view.manager.notifications.clone() {
            cx.visit_dyn(n, |c, cx| {
                if let Some(n) = c.as_notification() {
                    n.exit(cx);
                }
            });
        }
    }

    pub fn notification_destroyed(&mut self, n: WidgetId) {
        self.notifications.retain(|&x| x != n);
    }

    /// SetWaiting: UI_Global's UI_Waiting (root canvas ZOrder 150), made here on its first use.
    pub fn set_waiting(cx: &mut Cx<'_>, is_waiting: bool, label: &str, opaque: bool, countdown: f64) {
        cx.view.manager.is_waiting = is_waiting;
        let w = match cx.view.manager.waiting {
            Some(w) => w,
            None => {
                if !is_waiting {
                    return;
                }
                let w = MenuView::create(cx, "UI_Waiting");
                cx.view.manager.waiting = Some(w);
                MenuView::add_to_view(cx, w, false, 150);
                w
            }
        };
        cx.visit_dyn(w, |c, cx| {
            if let Some(x) = c.as_waiting() {
                x.set_waiting(cx, is_waiting, label, opaque, countdown);
            }
        });
    }

    /// (the port's own) [`Manager::set_waiting`] with the cover fading in or out over `fade` s, where the build's shows
    /// and goes at once; the label's own fade in runs as the build's.
    pub fn set_waiting_faded(cx: &mut Cx<'_>, is_waiting: bool, label: &str, opaque: bool, countdown: f64, fade: f64) {
        cx.view.manager.waiting_fade = fade;
        Self::set_waiting(cx, is_waiting, label, opaque, countdown);
        cx.view.manager.waiting_fade = 0.0;
    }

    // ---------- timers ----------

    /// InternalTime brought to now (a screen kept collapsed while the game runs, before something reads it).
    pub fn sync_internal_time(&mut self, now: f64) {
        self.internal_time = now;
    }

    /// Add Timer: OnTimerEvent(id) fires on the first tick at or after the delay; its id (a new guid when `id`
    /// is not one).
    pub fn add_timer(cx: &mut Cx<'_>, id: &str, delay: f64) -> String {
        let m = &mut cx.view.manager;
        let id = if valid_guid(id) { id.to_string() } else { m.new_guid() };
        let at = m.internal_time + delay;
        m.timer_events.insert(id.clone(), at);
        id
    }

    /// Check Timers: the ids held when it starts, latest added first.
    fn check_timers(cx: &mut Cx<'_>) {
        let ids: Vec<String> = cx.view.manager.timer_events.keys().cloned().collect();
        for id in ids.into_iter().rev() {
            let m = &cx.view.manager;
            let Some(&at) = m.timer_events.get(&id) else { continue };
            if m.internal_time < at {
                continue;
            }
            cx.fire_manager(|m| &m.on_timer_event, &id);
            cx.view.manager.timer_events.shift_remove(&id);
        }
    }

    // ---------- transitions (UI_Transitioner) ----------

    /// IsTransitioning: the fader is not clear, or a transition runs.
    pub fn is_transitioning(&self) -> bool {
        self.fader != 0.0 || self.transition_run.is_some()
    }

    /// Transition UIManager: dropped while another transition runs, unless that one was forced (CanTransition
    /// reads the force of the stored params, the running transition's). Type 0 fades to black, 1 from black, 3
    /// (Black) is black from the call on and completes when the delay ends, 4 (None) leaves the fader clear and
    /// ends at failsafe 2; an accepted call restarts from its start opacity. The times are real time.
    /// Deviation: type 2 (InToCountdown, LocalPlayFacilitator @3275; TR @1479, @1532: UI_Countdown made at ZOrder 4000
    /// and the fader 1 -> 0 over it in 0.3 s, then OpenLevel) is not ported:
    /// Local Play's Begin Match stands a type 3 in. BPI_DestroyBlack on the camera actors (@811-1229) has no
    /// counterpart.
    pub fn transition(cx: &mut Cx<'_>, p: TransitionParams) {
        let now = cx.now();
        let m = &mut cx.view.manager;
        if m.transition_run.is_some() && !m.transition_force {
            return;
        }
        assert!((0..=4).contains(&p.kind) && p.kind != 2, "menu: transition type {}", p.kind);
        m.transition_force = p.force;
        m.fader = FADER_START[p.kind as usize];
        m.transition_serial += 1;
        let delay = p.delay;
        // Failsafe Time @3985-4203: the delay, the fade and one second from the call
        m.transition_run = Some(TransitionRun {
            failsafe2: now + delay.max(0.0) + TRANSITION_TIME + 1.0,
            p,
            delay: if delay > 0.0 { now + delay } else { -1.0 },
            start: -1.0,
            failsafe: -1.0,
            serial: m.transition_serial,
        });
        if delay <= 0.0 {
            Manager::start_transition(cx);
        }
    }

    fn still(&self, serial: u64) -> bool {
        self.transition_run.as_ref().is_some_and(|r| r.serial == serial)
    }

    fn start_transition(cx: &mut Cx<'_>) {
        let Some(run) = cx.view.manager.transition_run.as_mut() else { return };
        run.delay = -1.0;
        let (p, serial) = (run.p.clone(), run.serial);
        // TransitionStarted
        if p.kind == 0 {
            for pl in 0..4 {
                Manager::unfocus_all(cx, pl);
            }
        }
        cx.fire_manager(|m| &m.transition_started, &p);
        if !cx.view.manager.still(serial) {
            return;
        }
        // @1262-1997: Black completes here and None stops, both without the RTPC
        if p.kind == 3 {
            Manager::complete_transition(cx);
            return;
        }
        if p.kind == 4 {
            return;
        }
        cx.sink.set_rtpc("transiiton", if p.kind == 0 { 0.0 } else { 1.0 }, TRANSITION_RTPC_MS);
        if !cx.view.manager.still(serial) {
            return;
        }
        if p.instant {
            Manager::complete_transition(cx);
            return;
        }
        let now = cx.now();
        if let Some(run) = cx.view.manager.transition_run.as_mut() {
            run.start = now;
            run.failsafe = now + TRANSITION_TIME + 0.5;
        }
    }

    fn complete_transition(cx: &mut Cx<'_>) {
        let Some(run) = cx.view.manager.transition_run.take() else { return };
        let p = run.p;
        cx.view.manager.fader = FADER_END[p.kind as usize];
        cx.fire_manager(|m| &m.transition_complete, &p);
        if !p.next_level.is_empty() {
            Manager::dismiss_all_modals(cx);
            cx.view.manager.open_level = Some(p);
        }
    }

    /// The fade loop re-enters once per frame: linear over real time. (Also called on its own by a screen whose
    /// widgets do not show: UI_Global's transitioner ticks whatever is on the viewport.)
    pub fn tick_transition(cx: &mut Cx<'_>) {
        let now = cx.now();
        let Some(run) = cx.view.manager.transition_run.as_ref() else { return };
        if run.delay >= 0.0 {
            if now < run.delay {
                return;
            }
            Manager::start_transition(cx);
            return;
        }
        if run.p.kind == 4 {
            // Tick @4245-4423, failsafe 2: Complete, then Reset (the fader clear)
            if now <= run.failsafe2 {
                return;
            }
            Manager::complete_transition(cx);
            if cx.view.manager.transition_run.is_none() {
                cx.view.manager.fader = 0.0;
            }
            return;
        }
        let a = ((now - run.start) / TRANSITION_TIME).clamp(0.0, 1.0);
        let (kind, failsafe) = (run.p.kind, run.failsafe);
        cx.view.manager.fader = if kind == 0 { a } else { 1.0 - a };
        if a == 1.0 || now > failsafe {
            Manager::complete_transition(cx);
        }
    }

    /// BPI_OpenLevel(level, options) with no transition (PGI@33841: OpenLevel): the modals go and the level is asked
    /// for, as a finished transition leaves it.
    pub fn open_level_now(cx: &mut Cx<'_>, level: &str, options: &str) {
        Manager::dismiss_all_modals(cx);
        cx.view.manager.open_level = Some(TransitionParams {
            next_level: level.to_string(),
            level_options: options.to_string(),
            ..Default::default()
        });
    }

    /// The transition whose fade has ended with a level: its params (OpenLevel(nextLevel, levelOptions)), taken.
    pub fn take_transition(&mut self) -> Option<TransitionParams> {
        self.open_level.take()
    }

    // ---------- tick ----------

    /// The manager's frame: the clock, the mouse delta and the sticks' decay, the transition, the focus restore,
    /// the timers, the interactors' hover check.
    pub fn tick(cx: &mut Cx<'_>) {
        let now = cx.now();
        cx.view.manager.internal_time = now;
        if cx.view.manager.handle_input {
            Manager::update_mouse_delta(cx);
            cx.view.manager.update_analog_move();
        }
        Manager::tick_transition(cx);
        let now = cx.now();
        if cx.view.manager.handle_input && Manager::interval_ready(now, cx.view.manager.restore_timer, RESTORE_INTERVAL)
        {
            cx.view.manager.restore_timer = now;
            Manager::restore_focused(cx, 0);
        }
        Manager::check_timers(cx);
        for i in cx.view.manager.interactors.clone() {
            interactor::manually_check_mouse_hover(cx, i);
        }
    }
}

/// GetDistanceBetweenWidgetsAccountingForSize's result.
struct Dist {
    base: f64,
    dist: f64,
    valid: bool,
    dot: f64,
}

/// GetDistanceBetweenWidgetsAccountingForSize: the distance between the start widget's edge facing the direction
/// and the target's opposite edge. The edges are built from GetLocalTopLeft (the widget's position in its parent)
/// and passed to LocalToViewport as the widget's own local points, as the Blueprint does. The centre distance is
/// GetWidgetCenterPosition's (native, impl 14121fa10), by the last paint.
fn distance_accounting_for_size(view: &MenuView, a: WidgetId, b: WidgetId, dx: f64, dy: f64) -> Dist {
    let edge = |w: WidgetId, ex: f64, ey: f64| -> ([f64; 2], [f64; 2]) {
        let [x, y, ww, wh] = view.tree().arranged(w);
        let sx = x + ww * if ex > 0.0 { 1.0 } else { 0.0 };
        let sy = y + wh * if ey > 0.0 { 1.0 } else { 0.0 };
        (
            view.local_to_viewport(w, sx, sy),
            view.local_to_viewport(
                w,
                sx + ww * if ey != 0.0 { 1.0 } else { 0.0 },
                sy + wh * if ex != 0.0 { 1.0 } else { 0.0 },
            ),
        )
    };
    let (s1s, s1e) = edge(a, dx, dy);
    let (s2s, s2e) = edge(b, -dx, -dy);
    let [_, _, aw, ah] = view.tree().arranged(a);
    let [_, _, bw, bh] = view.tree().arranged(b);
    let ca = view.local_to_viewport(a, aw / 2.0, ah / 2.0);
    let cb = view.local_to_viewport(b, bw / 2.0, bh / 2.0);
    let base = (ca[0] - cb[0]).powi(2) + (ca[1] - cb[1]).powi(2);
    let pick = |v: [f64; 2]| if dx != 0.0 { v[0] } else { v[1] };
    let in_range = |v: f64, p: f64, q: f64| v >= p.min(q) && v <= p.max(q);
    let overlap = in_range(pick(s1e), pick(s2s), pick(s2e))
        || in_range(pick(s1s), pick(s2s), pick(s2e))
        || in_range(pick(s2s), pick(s1s), pick(s1e))
        || in_range(s2e[1], s1s[1], s1e[1]);
    let (p1, p2) = nearest_points_on_segments(s1s, s1e, s2s, s2e);
    let vx = p2[0] - p1[0];
    let vy = p2[1] - p1[1];
    let dist = vx.hypot(vy);
    let n = if dist * dist > 0.0001 { [vx / dist, vy / dist] } else { [0.0, 0.0] };
    let near = dist <= 5.0;
    let dot_raw = n[0] * dx + n[1] * dy;
    Dist { base, dist, valid: near || dot_raw > 0.1, dot: if near || overlap { 1.0 } else { dot_raw } }
}

/// FMath::SegmentDistToSegmentSafe (UKismetMathLibrary::FindNearestPointsOnLineSegments; UE 4.26 source) in the
/// plane.
fn nearest_points_on_segments(a1: [f64; 2], b1: [f64; 2], a2: [f64; 2], b2: [f64; 2]) -> ([f64; 2], [f64; 2]) {
    const SMALL: f64 = 1e-4;
    let s1 = [b1[0] - a1[0], b1[1] - a1[1]];
    let s2 = [b2[0] - a2[0], b2[1] - a2[1]];
    let s3 = [a1[0] - a2[0], a1[1] - a2[1]];
    let dot = |p: [f64; 2], q: [f64; 2]| p[0] * q[0] + p[1] * q[1];
    // GetSafeNormal: zero under a squared length of 1e-8
    let norm = |v: [f64; 2]| {
        let l2 = dot(v, v);
        if l2 > 1e-8 { [v[0] / l2.sqrt(), v[1] / l2.sqrt()] } else { [0.0, 0.0] }
    };
    let n1v = norm(s1);
    let n2v = norm(s2);
    let d11 = dot(s1, s1);
    let d22 = dot(s2, s2);
    let d12 = dot(s1, s2);
    let d13 = dot(s1, s3);
    let d23 = dot(s2, s3);
    let d_norm = dot(n1v, n1v) * dot(n2v, n2v) - dot(n1v, n2v).powi(2);
    let d = d11 * d22 - d12 * d12;
    let mut den1 = d;
    let mut den2 = d;
    let mut n1;
    let mut n2;
    if d < SMALL || d_norm < SMALL {
        n1 = 0.0;
        den1 = 1.0;
        n2 = d23;
        den2 = d22;
    } else {
        n1 = d12 * d23 - d22 * d13;
        n2 = d11 * d23 - d12 * d13;
        if n1 < 0.0 {
            n1 = 0.0;
            n2 = d23;
            den2 = d22;
        } else if n1 > den1 {
            n1 = den1;
            n2 = d23 + d12;
            den2 = d22;
        }
    }
    if n2 < 0.0 {
        n2 = 0.0;
        if -d13 < 0.0 {
            n1 = 0.0;
        } else if -d13 > d11 {
            n1 = den1;
        } else {
            n1 = -d13;
            den1 = d11;
        }
    } else if n2 > den2 {
        n2 = den2;
        if -d13 + d12 < 0.0 {
            n1 = 0.0;
        } else if -d13 + d12 > d11 {
            n1 = den1;
        } else {
            n1 = -d13 + d12;
            den1 = d11;
        }
    }
    let t1 = if n1.abs() < SMALL { 0.0 } else { n1 / den1 };
    let t2 = if n2.abs() < SMALL { 0.0 } else { n2 / den2 };
    ([a1[0] + s1[0] * t1, a1[1] + s1[1] * t1], [a2[0] + s2[0] * t2, a2[1] + s2[1] * t2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_action_keys_by_type() {
        assert_eq!(ui_action_key("UISubmit", true), Some("Gamepad_FaceButton_Bottom"));
        assert_eq!(ui_action_key("UISubmit", false), Some("SpaceBar"));
        assert_eq!(ui_action_key("UISubmit4", false), None);
        assert!(is_ui_action("UISubmit4"));
        assert!(!is_ui_action("Jump"));
    }

    #[test]
    fn guids_are_four_hex_words() {
        let mut m = Manager::new();
        let g = m.new_guid();
        assert_eq!(g.len(), 35);
        assert_ne!(g, m.new_guid());
        assert!(valid_guid(&m.ignore_context));
    }
}
