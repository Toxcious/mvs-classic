//! The Controls page: UI_InputBinding (a binding row),
//! UI_InputBindingSet (a player's rows and the two presets), UI_PreferencesControls-Toggles ("Additional Options",
//! the input preferences) and UI_PreferencesControlsRework (the page around them). The Settings panel hosts the page
//! (`preferences.rs`) and so does Local Play's per-player modal (`input_binding_modal.rs`).
//!
//! A rebinding writes the runtime bindings (`Cx::bindings`, `crate::bindings`) and the settings file's copy of them
//! (`Settings::input.bindings`), which the next match's devices load; the input preferences are written into
//! `Settings::input` (`menu::prefs`), which the next match's set-up and devices read.

use std::any::Any;
use std::collections::HashSet;

use serde_json::{Value, json};

use super::buttons::{ButtonSecondary, CLEAR, HOVER_BACKING, PlusMinusHorizontal, ToggleSwitch};
use super::drop_menu::DropMenu;
use super::input_label::{ACTION_NAMES, InputLabel, InputLabelParams, Slots, action_label, action_name, enum_name};
use super::interactor::{batch_set_context, batch_set_scroll_box, set_focused};
use super::{find, prop, prop_bool, prop_str, wd};
use crate::bindings::{LAB_TOOLS_ACTION, is_gamepad_key, mapping_name};
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Event, Handler, InputCapture};
use crate::menu::manager::{DialogParams, Manager, NULL_GUID};
use crate::menu::prefs::{self, InputBool};
use crate::menu::screens::lab_selectors::run_on;
use crate::menu::scroll::{scroll_list, set_scrollbar_padding};
use crate::menu::view::MenuView;
use crate::menu::widgets::common::text;
use crate::slate::{HAlign, Margin, VAlign, Vis, WidgetId};

use super::input_label::break_apart_mappings;

// ---------- UI_InputBinding ----------

/// A row's four slots: gamepad 1 / 2, keyboard 1 / 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    G1,
    G2,
    K1,
    K2,
}

/// (slot, button, label, throbber)
const BIND_SLOTS: [(Slot, &str, &str, &str); 4] = [
    (Slot::G1, "UI_Gamepad_1", "UI_InputLabel_GP1", "listeningG1"),
    (Slot::G2, "UI_Gamepad_2", "UI_InputLabel_GP2", "listeningG2"),
    (Slot::K1, "UI_Keyboard_1", "UI_InputLabel_KB1", "listeningK1"),
    (Slot::K2, "UI_Keyboard_2", "UI_InputLabel_KB2", "listeningK2"),
];
const BIND_BUTTON: Rgba = [0.194618, 0.571125, 0.552012, 1.0];
const BIND_TINT_GP: Rgba = [0.191202, 0.571125, 0.552012, 1.0];
/// The stick axes and the left stick's directions never bind and are not consumed.
const STICK_KEYS: [&str; 8] = [
    "Gamepad_LeftX",
    "Gamepad_LeftY",
    "Gamepad_RightX",
    "Gamepad_RightY",
    "Gamepad_LeftStick_Up",
    "Gamepad_LeftStick_Down",
    "Gamepad_LeftStick_Left",
    "Gamepad_LeftStick_Right",
];

fn slot_key(s: &Slots, k: Slot) -> Option<&str> {
    match k {
        Slot::G1 => s.g1.as_deref(),
        Slot::G2 => s.g2.as_deref(),
        Slot::K1 => s.k1.as_deref(),
        Slot::K2 => s.k2.as_deref(),
    }
}

/// Whether `key` is reserved: Escape and the function keys.
// Deviation: Escape and F1..F24 do not bind but for the Lab Tools key (kept from the browser game, which could not
// own them). The build refuses only its Exclusion List.
fn is_reserved(key: &str) -> bool {
    key == "Escape" || key.strip_prefix('F').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// A widget animation of `cx.w` played when its class has it (SaveAnim's and ErrorAnim's widgets are absent from
/// the cooked tree: the animation plays with nothing to show).
fn play_own(cx: &mut Cx<'_>, name: &str, speed: f64, restore: bool) {
    let class = cx.view.tree().user_class(cx.w).map(str::to_string).unwrap_or_default();
    if cx.view.anim_length(&class, name) > 0.0 {
        cx.view.play_animation(cx.w, name, false, speed, 1, false, restore);
    }
}

const B_CLICK: u32 = 1;
const B_FOCUS: u32 = 10;
const B_UNFOCUS: u32 = 20;
const B_ROW_FOCUS: u32 = 30;
const B_ROW_UNFOCUS: u32 = 31;

/// A row of the Controls page: an action with its two gamepad or two keyboard slots; a click on a slot listens for
/// the next key.
#[derive(Debug, Default)]
pub struct InputBinding {
    /// (mapping name, player, key)
    pub input_binding_changed: Event<(String, usize, String)>,
    pub started_listening: Event<WidgetId>,
    pub player_index: usize,
    pub is_keyboard: bool,
    pub pause_saving: bool,
    listening_slot: Option<Slot>,
    listen_timer: f64,
    revert_set: Slots,
}

impl InputBinding {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(InputBinding::default())
    }

    /// The row's mapping name ('Action Name').
    pub fn action_name(cx: &Cx<'_>) -> String {
        prop_str(cx, "Action Name")
    }

    fn buttons(cx: &Cx<'_>) -> [WidgetId; 4] {
        BIND_SLOTS.map(|(_, b, _, _)| find(cx, b))
    }

    fn slots(&self, cx: &Cx<'_>) -> Slots {
        let name = Self::action_name(cx);
        break_apart_mappings(cx.bindings.action_mapping(&name, self.player_index))
    }

    pub fn interactors(cx: &Cx<'_>) -> Vec<WidgetId> {
        Self::buttons(cx).iter().map(|&b| ButtonSecondary::interactor(cx, b)).collect()
    }

    pub fn update_context(cx: &mut Cx<'_>, ctx: &str) {
        let list = Self::interactors(cx);
        batch_set_context(cx, &list, ctx);
    }

    /// Init(player, play the enter animation).
    pub fn init(&mut self, cx: &mut Cx<'_>, player: usize, play_anim: bool) {
        self.player_index = player;
        self.store_revert_set(cx);
        let base = InputLabelParams {
            action: enum_name(cx.view.tree().props(cx.w).get("InputAction")),
            modifier: "None".into(),
            label: text(cx.view.tree().props(cx.w).get("Action Label")),
            player_index: player as i32,
            automatic_input_type: false,
            fixed_input_type: 0,
            tint: BIND_TINT_GP,
            text_size: 14.0,
            key_size: 16.0,
            icon_size: [28.0, 28.0],
            custom_icon: None,
        };
        for (slot, _, label, _) in BIND_SLOTS {
            let kb = matches!(slot, Slot::K1 | Slot::K2);
            let p = InputLabelParams {
                fixed_input_type: i32::from(kb),
                tint: if kb { BIND_BUTTON } else { BIND_TINT_GP },
                ..base.clone()
            };
            let l = find(cx, label);
            cx.visit::<InputLabel, _>(l, |l, cx| l.update_params(cx, p));
        }
        self.stop_listening(cx);
        if !play_anim {
            return;
        }
        let me = cx.w;
        if !cx.view.is_animation_playing(me, "EnterAnim") {
            cx.view.play(me, "EnterAnim", false, 5.0);
        } else if !cx.view.is_animation_playing_forward(me, "EnterAnim") {
            cx.view.reverse_animation(me, "EnterAnim");
            cx.view.set_playback_speed(me, "EnterAnim", 5.0);
        }
    }

    fn start_listening(&mut self, cx: &mut Cx<'_>, slot: Slot) {
        self.listen_timer = cx.now();
        self.listening_slot = Some(slot);
        cx.view.manager.handle_input = false;
        cx.view.set_user_focus(cx.w);
        self.update_appearance(cx);
        let me = cx.w;
        cx.emit(self, |c: &InputBinding| &c.started_listening, &me);
    }

    pub fn stop_listening(&mut self, cx: &mut Cx<'_>) {
        cx.view.manager.handle_input = true;
        self.listening_slot = None;
        self.update_appearance(cx);
    }

    fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let slots = self.slots(cx);
        for (slot, _, label, throbber) in BIND_SLOTS {
            let listening = self.listening_slot == Some(slot);
            let t = find(cx, throbber);
            wd(cx, t).vis = if listening { Vis::HitTestInvisible } else { Vis::Hidden };
            let l = find(cx, label);
            wd(cx, l).vis =
                if slot_key(&slots, slot).is_some() && !listening { Vis::HitTestInvisible } else { Vis::Collapsed };
        }
        let (gp, kb) = if self.is_keyboard {
            (Vis::Collapsed, Vis::SelfHitTestInvisible)
        } else {
            (Vis::SelfHitTestInvisible, Vis::Collapsed)
        };
        for (n, v) in [("Gamepad1", gp), ("Gamepad2", gp), ("Keyboard1", kb), ("Keyboard2", kb)] {
            let w = find(cx, n);
            wd(cx, w).vis = v;
        }
        if self.listening_slot.is_none() {
            for (_, _, label, _) in BIND_SLOTS {
                let l = find(cx, label);
                cx.visit::<InputLabel, _>(l, |l, cx| l.refresh(cx));
            }
        }
    }

    fn row_visuals(cx: &mut Cx<'_>, focused: bool) {
        let title = find(cx, "ActionTitle");
        let t = wd(cx, title);
        t.opacity = if focused { 1.0 } else { 0.65 };
        t.font_size = if focused { 20.0 } else { 18.0 };
        t.letter_spacing = 0.0;
        let b = find(cx, "HoverBacking");
        wd(cx, b).color = if focused { HOVER_BACKING } else { CLEAR };
    }

    /// The focused pair of slots shows its buttons; ResetStyle runs on all four.
    fn button_focus(cx: &mut Cx<'_>, pair: [&str; 2], focused: bool) {
        for name in pair {
            let b = find(cx, name);
            cx.view.tree_mut().set_prop(b, "HideButton", json!(!focused));
        }
        for b in Self::buttons(cx) {
            cx.visit::<ButtonSecondary, _>(b, |b, cx| b.reset_style(cx, Some(BIND_BUTTON)));
        }
        Self::row_visuals(cx, focused);
    }

    /// UI_Functions TimeIntervalCheck(0.1): the press that started the listening is not the new key.
    fn interval_ready(&mut self, cx: &Cx<'_>) -> bool {
        let d = cx.now() - self.listen_timer;
        if (0.0..0.1).contains(&d) {
            return false;
        }
        self.listen_timer = cx.now();
        true
    }

    fn process_input(&mut self, cx: &mut Cx<'_>, key: &str) {
        if let Some(slot) = self.listening_slot
            && self.is_key_allowed(cx, key)
        {
            let old = slot_key(&self.slots(cx), slot).map(str::to_string);
            self.update_key(cx, old.as_deref(), Some(key));
            let a = (Self::action_name(cx), self.player_index, key.to_string());
            cx.emit(self, |c: &InputBinding| &c.input_binding_changed, &a);
        }
        self.stop_listening(cx);
    }

    /// Is Key Taken: a keyboard or mouse key of players 1 to 3 that an action of another player holds.
    fn is_key_taken(&self, cx: &Cx<'_>, key: &str) -> bool {
        if is_gamepad_key(key) || self.player_index == 0 {
            return false;
        }
        ACTION_NAMES.iter().any(|(_, name)| {
            (0..4).any(|p| p != self.player_index && cx.bindings.action_mapping(name, p).any(|k| k == key))
        })
    }

    /// A key of the other device is dropped without a message.
    fn is_key_allowed(&self, cx: &mut Cx<'_>, key: &str) -> bool {
        if STICK_KEYS.contains(&key) || is_gamepad_key(key) == self.is_keyboard {
            return false;
        }
        if self.is_key_taken(cx, key) {
            Self::show_error_message(cx, "Already taken by another player");
            return false;
        }
        let excluded = prop(cx, "Exclusion List").as_array().is_some_and(|list| {
            list.iter().any(|k| k.get("KeyName").and_then(Value::as_str).or(k.as_str()) == Some(key))
        });
        // (the port's own) the Lab Tools key may be F1 to F4, the keys its panel takes
        let lab_key = Self::action_name(cx) == LAB_TOOLS_ACTION && matches!(key, "F1" | "F2" | "F3" | "F4");
        if excluded || (is_reserved(key) && !lab_key) {
            Self::show_error_message(cx, "That input is unbindable");
            return false;
        }
        true
    }

    /// Player 0's keyboard key leaves this action of every player first.
    pub fn update_key(&mut self, cx: &mut Cx<'_>, old: Option<&str>, key: Option<&str>) {
        let action = Self::action_name(cx);
        let name = mapping_name(&action, self.player_index);
        if let Some(k) = key
            && self.player_index == 0
            && !is_gamepad_key(k)
        {
            for p in 0..4 {
                cx.bindings.remove_action_mapping(&mapping_name(&action, p), k);
            }
        }
        if let Some(o) = old {
            cx.bindings.remove_action_mapping(&name, o);
        }
        if let Some(k) = key
            && k != "None"
        {
            cx.bindings.add_action_mapping(&name, k);
        }
        let immediate = cx.view.tree().props(cx.w).get("Immediate Save").and_then(Value::as_bool).unwrap_or(true);
        if immediate {
            self.save(cx);
        }
    }

    pub fn save(&mut self, cx: &mut Cx<'_>) {
        if self.pause_saving {
            return;
        }
        cx.bindings.save_key_mappings();
        self.store_revert_set(cx);
        play_own(cx, "SaveAnim", 1.0, true); // @85, bRestoreState
    }

    pub fn store_revert_set(&mut self, cx: &Cx<'_>) {
        self.revert_set = self.slots(cx);
    }

    pub fn revert(&mut self, cx: &mut Cx<'_>) {
        let set = self.revert_set.clone();
        for k in [set.k1, set.k2, set.g1, set.g2].into_iter().flatten() {
            self.update_key(cx, Some(&k), Some(&k));
        }
    }

    /// The row's mappings of the mode's device are replaced by the preset's.
    // Deviation: the build then stores Set-Input-SwappedAttackStyleGround(Legacy) (@3624); not done here, as the
    // page's reset to defaults writes the same value right after through Set-Input-AllDefaults.
    pub fn set_default(&mut self, cx: &mut Cx<'_>, legacy: bool, keyboard: bool) {
        let action = Self::action_name(cx);
        let name = mapping_name(&action, self.player_index);
        let keys: Vec<String> = cx.bindings.mapping_by_name(&name).map(str::to_string).collect();
        let immediate = cx.view.tree().props(cx.w).get("Immediate Save").and_then(Value::as_bool).unwrap_or(true);
        for key in keys {
            if key != "None" && is_gamepad_key(&key) == keyboard {
                continue;
            }
            cx.bindings.remove_action_mapping(&name, &key);
            if immediate {
                self.save(cx);
            }
        }
        let defaults = cx.bindings.input_defaults_for_player(self.player_index, legacy);
        let d = break_apart_mappings(defaults.get(&action).map(|v| v.iter().map(String::as_str)).into_iter().flatten());
        let (a, b) = if keyboard { (d.k1, d.k2) } else { (d.g1, d.g2) };
        self.update_key(cx, None, a.as_deref());
        self.update_key(cx, None, b.as_deref());
    }

    fn show_error_message(cx: &mut Cx<'_>, message: &str) {
        let l = find(cx, "errorLabel");
        wd(cx, l).text = message.to_string();
        play_own(cx, "ErrorAnim", 1.0, true); // PlayAnimationForward @8256, bRestoreState
    }
}

impl Controller for InputBinding {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let buttons = Self::buttons(cx);
        for (i, &b) in buttons.iter().enumerate() {
            let i = i as u32;
            cx.visit::<ButtonSecondary, _>(b, |b, _| {
                b.on_click.add(Handler { owner: me, id: B_CLICK + i });
                b.on_focus.add(Handler { owner: me, id: B_FOCUS + i });
                b.on_unfocus.add(Handler { owner: me, id: B_UNFOCUS + i });
            });
        }
        let [g1, g2, k1, k2] = buttons.map(|b| ButtonSecondary::interactor(cx, b));
        for (i, l, r) in [(g1, None, Some(g2)), (g2, Some(g1), None), (k1, None, Some(k2)), (k2, Some(k1), None)] {
            if let Some(s) = cx.view.manager.interactor_mut(i) {
                if l.is_some() {
                    s.manual_left = l;
                }
                if r.is_some() {
                    s.manual_right = r;
                }
            }
        }
        let row = find(cx, "UI_Interactor");
        super::bind(cx, row, |s| &mut s.on_focus, B_ROW_FOCUS);
        super::bind(cx, row, |s| &mut s.on_unfocus, B_ROW_UNFOCUS);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let t = text(cx.view.tree().props(cx.w).get("Action Label"));
        let w = find(cx, "ActionTitle");
        wd(cx, w).text = t;
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        if self.listening_slot.is_some() {
            self.stop_listening(cx);
        }
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        if self.listening_slot.is_some() {
            cx.view.set_user_focus(cx.w);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        let pair = |i: u32| if i < 2 { ["UI_Gamepad_1", "UI_Gamepad_2"] } else { ["UI_Keyboard_1", "UI_Keyboard_2"] };
        match id {
            n if (B_CLICK..B_CLICK + 4).contains(&n) => self.start_listening(cx, BIND_SLOTS[(n - B_CLICK) as usize].0),
            n if (B_FOCUS..B_FOCUS + 4).contains(&n) => Self::button_focus(cx, pair(n - B_FOCUS), true),
            n if (B_UNFOCUS..B_UNFOCUS + 4).contains(&n) => Self::button_focus(cx, pair(n - B_UNFOCUS), false),
            B_ROW_FOCUS => Self::row_visuals(cx, true),
            B_ROW_UNFOCUS => Self::row_visuals(cx, false),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        self.handle(cx, id, &());
    }

    fn as_input_capture(&mut self) -> Option<&mut dyn InputCapture> {
        Some(self)
    }
}

impl InputCapture for InputBinding {
    fn is_listening(&self) -> bool {
        self.listening_slot.is_some()
    }

    fn on_preview_key_down(&mut self, cx: &mut Cx<'_>, key: &str) -> bool {
        if STICK_KEYS.contains(&key) || self.listening_slot.is_none() || !self.interval_ready(cx) {
            return false;
        }
        self.process_input(cx, key);
        true
    }

    /// OnMouseButtonDown / OnMouseWheel.
    fn process_mouse_event(&mut self, cx: &mut Cx<'_>, key: &str) {
        if self.listening_slot.is_some() && self.interval_ready(cx) {
            self.process_input(cx, key);
        }
    }

    fn on_analog_value_changed(&mut self, cx: &mut Cx<'_>, value: f64) {
        if self.listening_slot.is_some() && value.abs() > 0.25 {
            self.stop_listening(cx);
        }
    }
}

// ---------- UI_InputBindingSet ----------

const S_LEGACY: u32 = 1;
const S_DEFAULT: u32 = 2;
const S_CHANGED: u32 = 3;
const S_STARTED: u32 = 4;
const S_CONFIRM_LEGACY: u32 = 5;
const S_CONFIRM_DEFAULT: u32 = 6;

/// The rows of a player's actions with the two preset buttons. A preset asks through the manager's dialog first
/// (Reset To Defaults Dialog @10260; Suppress Dialogs is false in the class defaults and on the Settings panel's
/// instance).
#[derive(Debug, Default)]
pub struct InputBindingSet {
    /// (the new keyboard mode: the set is in hand while its handlers run)
    pub mode_changed: Event<bool>,
    pub preferences_changed_dispatch: Event<()>,
    pub inputs_refreshed: Event<()>,
    /// (player, legacy)
    pub reset_to_defaults_event: Event<(usize, bool)>,
    pub keyboard_mode: bool,
    pub player_index: usize,
    pub rows: Vec<WidgetId>,
    scroll_box: Option<WidgetId>,
    bound: HashSet<WidgetId>,
}

impl InputBindingSet {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(InputBindingSet::default())
    }

    fn presets(cx: &Cx<'_>) -> [WidgetId; 2] {
        [find(cx, "Btn_Legacy"), find(cx, "Btn_Default")]
    }

    /// The Default preset button's interactor (the modal focuses it).
    pub fn default_button_interactor(cx: &Cx<'_>) -> WidgetId {
        ButtonSecondary::interactor(cx, find(cx, "Btn_Default"))
    }

    /// BuildInputs(Player Index) @13671: the rows made once (AddBinding, the enter animation unless the game is
    /// paused: loop @6407 / @8012), then re-initialised for the player (ReInitBinding(row, true), @8739 / @7604).
    pub fn build_inputs(&mut self, cx: &mut Cx<'_>, player: usize) {
        self.player_index = player;
        if self.rows.is_empty() {
            let anim = !cx.pause.as_ref().is_some_and(|p| p.is_game_paused());
            let actions: Vec<String> = prop(cx, "InputActions")
                .as_array()
                .map(|a| a.iter().map(|v| enum_name(Some(v))).collect())
                .unwrap_or_default();
            for a in actions {
                self.add_binding(cx, &a, anim);
            }
            // (the port's own) the Lab Tools panel's key, last (player 0's alone: re_init_binding)
            self.add_binding(cx, "LabTools", anim);
        } else {
            for row in self.rows.clone() {
                self.re_init_binding(cx, row, true);
            }
        }
        self.finish_setup(cx);
    }

    /// StopListening(null): no row listens.
    pub fn stop_listening(&mut self, cx: &mut Cx<'_>) {
        for row in self.rows.clone() {
            cx.visit::<InputBinding, _>(row, |r, cx| {
                if r.is_listening() {
                    r.stop_listening(cx);
                }
            });
        }
    }

    fn add_binding(&mut self, cx: &mut Cx<'_>, action: &str, play_anim: bool) {
        let Some(name) = action_name(action) else { return };
        let w = MenuView::create(cx, "UI_InputBinding");
        let t = cx.view.tree_mut();
        t.set_prop(w, "Action Name", json!(name));
        t.set_prop(w, "Action Label", json!({ "SourceString": action_label(name) }));
        t.set_prop(w, "InputAction", json!(action));
        self.rows.push(w);
        let x = wd(cx, w);
        x.h_align = HAlign::Fill;
        x.v_align = VAlign::Fill;
        x.fill = 0.0;
        let container = find(cx, "BindingsContainer");
        MenuView::add_child(cx, container, w, None);
        if self.rows.len() == 1 {
            let d = cx.view.tree().find_in(w, "DivTop");
            wd(cx, d).vis = Vis::Hidden;
        }
        self.re_init_binding(cx, w, play_anim);
    }

    fn re_init_binding(&mut self, cx: &mut Cx<'_>, row: WidgetId, play_anim: bool) {
        let (kb, player) = (self.keyboard_mode, self.player_index);
        // (the row whose new key reached here is in hand: its own Init waits for its call to return)
        run_on::<InputBinding>(cx, row, move |r, cx| {
            r.is_keyboard = kb;
            r.init(cx, player, play_anim);
        });
        let exclusive: Vec<String> = prop(cx, "KeyboardExclusive")
            .as_array()
            .map(|a| a.iter().map(|v| enum_name(Some(v))).collect())
            .unwrap_or_default();
        let action = enum_name(cx.view.tree().props(row).get("InputAction"));
        if exclusive.contains(&action) {
            wd(cx, row).vis = if kb { Vis::Visible } else { Vis::Collapsed };
        }
        let name = cx.view.tree().props(row).get("Action Name").and_then(Value::as_str) == Some(LAB_TOOLS_ACTION);
        if name {
            wd(cx, row).vis = if player == 0 { Vis::Visible } else { Vis::Collapsed };
        }
    }

    fn finish_setup(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        for row in self.rows.clone() {
            if !self.bound.insert(row) {
                continue;
            }
            cx.visit::<InputBinding, _>(row, |r, _| {
                r.input_binding_changed.add(Handler { owner: me, id: S_CHANGED });
                r.started_listening.add(Handler { owner: me, id: S_STARTED });
            });
        }
        if let Some(sb) = self.scroll_box {
            self.set_scroll_box(cx, sb);
        }
        cx.emit(self, |c: &InputBindingSet| &c.inputs_refreshed, &());
    }

    fn stop_listening_except(&mut self, cx: &mut Cx<'_>, except: WidgetId) {
        for row in self.rows.clone() {
            if row == except {
                continue;
            }
            cx.visit::<InputBinding, _>(row, |r, cx| {
                if r.is_listening() {
                    r.stop_listening(cx);
                }
            });
        }
    }

    pub fn context_interactors(&self, cx: &mut Cx<'_>) -> Vec<WidgetId> {
        let mut list: Vec<WidgetId> = Self::presets(cx).iter().map(|&b| ButtonSecondary::interactor(cx, b)).collect();
        for &row in &self.rows {
            list.extend(InputBinding::interactors(&cx.with(row)));
        }
        list
    }

    pub fn set_scroll_box(&mut self, cx: &mut Cx<'_>, sb: WidgetId) {
        self.scroll_box = Some(sb);
        let list = self.context_interactors(cx);
        batch_set_scroll_box(cx, &list, sb);
    }

    /// UpdateContext(Context, Player Index) @13853: BatchSetContext, BatchSetPlayerIndex over Get Context
    /// Interactors.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str, player: i32) {
        let list = self.context_interactors(cx);
        batch_set_context(cx, &list, ctx);
        for i in list {
            if let Some(s) = cx.view.manager.interactor_mut(i) {
                s.player_index = player;
            }
        }
    }

    fn refresh_view(&mut self, cx: &mut Cx<'_>) {
        for row in self.rows.clone() {
            self.re_init_binding(cx, row, false);
        }
        self.finish_setup(cx);
    }

    pub fn set_mode(&mut self, cx: &mut Cx<'_>, keyboard: bool) {
        if keyboard == self.keyboard_mode {
            return;
        }
        self.keyboard_mode = keyboard;
        self.refresh_view(cx);
        cx.emit(self, |c: &InputBindingSet| &c.mode_changed, &keyboard);
    }

    /// SaveKeyMappings, then SaveInputBindings: the settings file's copy of the bindings.
    fn save_changes(cx: &mut Cx<'_>) {
        cx.bindings.save_key_mappings();
        cx.prefs.input.bindings = Some(cx.bindings.save_input_bindings());
    }

    /// A key belongs to one action of a player: the new binding takes it from the others. The row the key came
    /// from is in hand: what is called on it here runs when its own call returns (`refresh_view`'s Init stores its
    /// revert set and draws its labels then).
    fn validate_new_binding(&mut self, cx: &mut Cx<'_>, action: &str, player: usize, key: &str) {
        for (_, name) in ACTION_NAMES {
            if *name != action {
                cx.bindings.remove_action_mapping(&mapping_name(name, player), key);
            }
        }
        for row in self.rows.clone() {
            run_on::<InputBinding>(cx, row, |r, cx| r.store_revert_set(cx));
        }
        Self::save_changes(cx);
        self.refresh_view(cx);
        cx.emit(self, |c: &InputBindingSet| &c.preferences_changed_dispatch, &());
    }

    /// Btn_Default @14703 / Btn_Legacy @14708: the dialog is asked for, then (@14338 / @14042, whatever the answer)
    /// the preset's Guid is stored as the player's controlGuid and PreferencesChangedDispatch fires. The table's
    /// Guids are all zero and nothing reads controlGuid (the analytics event's), so it is not stored.
    fn preset_clicked(&mut self, cx: &mut Cx<'_>, legacy: bool) {
        self.reset_to_defaults_dialog(cx, legacy);
        cx.emit(self, |c: &InputBindingSet| &c.preferences_changed_dispatch, &());
    }

    /// Reset To Defaults Dialog @10260: only the left button is bound (OnConfirmedLegacy @15365 /
    /// OnConfirmedDefaults @14648); NO closes the dialog and nothing else.
    pub fn reset_to_defaults_dialog(&mut self, cx: &mut Cx<'_>, legacy: bool) {
        if prop_bool(cx, "Suppress Dialogs") {
            self.confirmed(cx, legacy);
            return;
        }
        let params = DialogParams {
            title: "SAVE CONTROLS".into(),
            body: format!(
                "Restore {} controls for player {}?",
                if legacy { "legacy" } else { "default" },
                self.player_index + 1
            ),
            button_left_label: "YES".into(),
            button_right_label: "NO".into(),
            ..DialogParams::default()
        };
        let d = Manager::show_dialog(cx, params);
        let h = Handler { owner: cx.w, id: if legacy { S_CONFIRM_LEGACY } else { S_CONFIRM_DEFAULT } };
        if let Some(d) = cx.view.dialog(d) {
            d.button_left_clicked().add(h);
        }
    }

    fn confirmed(&mut self, cx: &mut Cx<'_>, legacy: bool) {
        self.reset_to_defaults(cx, legacy);
        // Accepted @13604
        Self::save_changes(cx);
        for row in self.rows.clone() {
            cx.visit::<InputBinding, _>(row, |r, cx| r.store_revert_set(cx));
        }
    }

    /// Reset to Defaults @12605.
    pub fn reset_to_defaults(&mut self, cx: &mut Cx<'_>, legacy: bool) {
        let kb = self.keyboard_mode;
        for row in self.rows.clone() {
            cx.visit::<InputBinding, _>(row, |r, cx| {
                r.pause_saving = true;
                r.set_default(cx, legacy, kb);
                r.pause_saving = false;
            });
        }
        for row in self.rows.clone() {
            cx.visit::<InputBinding, _>(row, |r, cx| r.store_revert_set(cx));
        }
        Self::save_changes(cx);
        self.refresh_view(cx);
        let a = (self.player_index, legacy);
        cx.emit(self, |c: &InputBindingSet| &c.reset_to_defaults_event, &a);
    }

    /// The keyboard rows' revert (the Settings modal's mkbRevert is empty in the build: not called).
    pub fn revert(&mut self, cx: &mut Cx<'_>) {
        for row in self.rows.clone() {
            cx.visit::<InputBinding, _>(row, |r, cx| r.revert(cx));
        }
    }
}

impl Controller for InputBindingSet {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        let [legacy, def] = Self::presets(cx);
        cx.visit::<ButtonSecondary, _>(def, |b, _| b.on_click.add(Handler { owner: me, id: S_DEFAULT }));
        cx.visit::<ButtonSecondary, _>(legacy, |b, _| b.on_click.add(Handler { owner: me, id: S_LEGACY }));
        self.build_inputs(cx, 0);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            S_LEGACY => self.preset_clicked(cx, true),
            S_DEFAULT => self.preset_clicked(cx, false),
            S_CONFIRM_LEGACY => self.confirmed(cx, true),
            S_CONFIRM_DEFAULT => self.confirmed(cx, false),
            S_CHANGED => {
                if let Some((action, player, key)) = arg.downcast_ref::<(String, usize, String)>() {
                    self.validate_new_binding(cx, action, *player, key);
                }
            }
            S_STARTED => {
                if let Some(&row) = arg.downcast_ref::<WidgetId>() {
                    self.stop_listening_except(cx, row);
                }
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        self.handle(cx, id, &());
    }
}

// ---------- UI_PreferencesControls-Toggles ----------

/// The switches that read and write one bool of the input preferences (RefreshPlayerIndex @8821-11394, the handlers
/// of the Set-on-change table); Up To Jump is apart (its dialog).
const BOOL_ROWS: [(&str, InputBool); 8] = [
    ("UI_ToggleVibration", InputBool::Vibration),
    ("UI_ToggleSwapSideNeutralGround", InputBool::SwappedAttackStyleGround),
    ("UI_ToggleSwapSideNeutralAir", InputBool::SwappedAttackStyleAir),
    ("UI_ToggleLargeDeadzone", InputBool::DeadzoneLarge),
    ("UI_ToggleInputBufferHold", InputBool::InputBufferHold),
    ("UI_ToggleDoubleCrouch", InputBool::DoubleCrouchJumpDown),
    ("UI_ToggleAttackPicksUpItems", InputBool::AttackPicksUpItems),
    ("UI_ToggleIgnoreRightStickDiagonals", InputBool::IgnoreRightStickDiagonals),
];
/// GetInteractors: the switches' UI_Interactor and the drop menus' mainInteractor in this order, then the plus /
/// minus rows' GetInteractors.
const INTERACTOR_ORDER: [&str; 11] = [
    "UI_ToggleVibration",
    "UI_ToggleSwapSideNeutralGround",
    "UI_ToggleSwapSideNeutralAir",
    "UI_ToggleLargeDeadzone",
    "UI_ToggleDoubleCrouch",
    "UI_ToggleUpWillJump",
    "UI_ToggleAttackPicksUpItems",
    "UI_DropMenu_Fastfall",
    "UI_ToggleInputBufferHold",
    "UI_DropMenu_RightStick",
    "UI_ToggleIgnoreRightStickDiagonals",
];
const PLUS_MINUS_ORDER: [&str; 3] =
    ["UI_PlusMinusHorizontalDeadzone", "UI_PlusMinusVerticalDeadzone", "UI_PlusMinusInputBufferCap"];
/// The texts of switch(GetPlatform()) @2650-8428, case 0 (the PC build): (row, member, key).
const PLATFORM_TEXTS: [(&str, &str, &str); 6] = [
    ("UI_ToggleVibration", "LabelText", "8858A4A24785FFFED504B4B1C9DFC2C0"), // @3241
    ("UI_ToggleVibration", "Tooltip", "27F02EC640D245F5386C06AC3A7B4E54"),   // @4251
    ("UI_DropMenu_RightStick", "LabelText", "5068440D43CDD2424B3DA68254BD36F4"), // @5069
    ("UI_DropMenu_RightStick", "TopTooltipText", "9269A1DC41E486AA85F3BFA95185E98D"), // @6091
    ("UI_ToggleIgnoreRightStickDiagonals", "LabelText", "ACA55CF84739D150DE1885ABA8C5AAD6"), // @7011
    ("UI_ToggleIgnoreRightStickDiagonals", "Tooltip", "ECEDFE7B4A342008C5A05B810288AC97"), // @8213
];
/// The Up To Jump dialog @1025 (left No, right Yes): title, body, left, right.
const UP_JUMP_DIALOG: [&str; 4] = [
    "C4DA066D460470B56C4A9999AAC08B4F",
    "95BD96174B89406C9AB040AB893025EC",
    "17D20BC74F546886B664438A5B01C40F",
    "DA651E3240F063F57EE9DB8236A9D6AA",
];
/// The Tap fastfall dialog @1817 (left Yes, right No).
const TAP_FASTFALL_DIALOG: [&str; 4] = [
    "9B9B040D48D24A11023730A9F01C8C48",
    "F30CAC6842DC91165C321D844F966F0D",
    "A248EA8A432B4C6CC90740BE4E2F7303",
    "E9C59BD2451E8ABFA7D0D7A82DFCD952",
];
/// E_FastfallType 0 Hold, 1 Tap.
const FASTFALL_IDS: [&str; 2] = ["hold", "tap"];
/// E_RightStickType 0 Attacks, 1 Specials.
const RIGHT_STICK_IDS: [&str; 2] = ["attacks", "specials"];

const T_BOOL: u32 = 1;
const T_HDZ: u32 = 20;
const T_VDZ: u32 = 21;
const T_CAP: u32 = 22;
const T_UP_JUMP: u32 = 23;
const T_FASTFALL: u32 = 24;
const T_RIGHT_STICK: u32 = 25;
const T_UP_JUMP_NO: u32 = 26;
const T_UP_JUMP_YES: u32 = 27;
const T_TAP_YES: u32 = 28;
const T_TAP_NO: u32 = 29;

/// "Additional Options" of the Controls page: the input preferences of one local player (Viewing Player Index, set by
/// RefreshPlayerIndex only). Every change is written at once (`menu::prefs`' Set-Input-*) and broadcast as
/// TogglesChanged.
#[derive(Debug, Default)]
pub struct PreferencesToggles {
    pub toggles_changed: Event<()>,
    /// Viewing Player Index.
    pub viewing_player: usize,
}

impl PreferencesToggles {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PreferencesToggles::default())
    }

    fn toggle<R>(cx: &mut Cx<'_>, name: &str, f: impl FnOnce(&mut ToggleSwitch, &mut Cx<'_>) -> R) -> Option<R> {
        let w = find(cx, name);
        cx.visit::<ToggleSwitch, _>(w, f)
    }

    fn plus_minus<R>(
        cx: &mut Cx<'_>,
        name: &str,
        f: impl FnOnce(&mut PlusMinusHorizontal, &mut Cx<'_>) -> R,
    ) -> Option<R> {
        let w = find(cx, name);
        cx.visit::<PlusMinusHorizontal, _>(w, f)
    }

    fn menu<R>(cx: &mut Cx<'_>, name: &str, f: impl FnOnce(&mut DropMenu, &mut Cx<'_>) -> R) -> Option<R> {
        let w = find(cx, name);
        cx.visit::<DropMenu, _>(w, f)
    }

    fn platform_texts(cx: &mut Cx<'_>) {
        for (row, member, key) in PLATFORM_TEXTS {
            let w = find(cx, row);
            let t = cx.view.game_text(key).to_string();
            cx.view.tree_mut().set_prop(w, member, json!({ "CultureInvariantString": t }));
        }
    }

    fn changed(&mut self, cx: &mut Cx<'_>) {
        cx.emit(self, |c: &PreferencesToggles| &c.toggles_changed, &());
    }

    /// RefreshPlayerIndex @8694. The switches' SetOn and the drop menus' SetActiveByID broadcast nothing; the plus /
    /// minus rows' SetValue clamps and broadcasts OnValueChanged, so a refresh writes the two dead zones and the
    /// buffer cap back and fires TogglesChanged three times (a stored 10000 cap shows 31 and is stored as 31).
    pub fn refresh_player_index(&mut self, cx: &mut Cx<'_>, player: usize) {
        self.viewing_player = player;
        let p = cx.prefs.input.clone();
        for (name, key) in &BOOL_ROWS[..4] {
            let on = prefs::input_bool(&p, *key, player);
            Self::toggle(cx, name, |t, cx| t.set_on(cx, on));
        }
        // @9762 / @9986: FTrunc(100 x value) in the Kismet's float32 (a stored 53 or 59 comes back one lower)
        let pct = |v: f64| f64::from((100.0f32 * v as f32).trunc());
        let h = pct(prefs::input_dead_zone(&p, false, player));
        let v = pct(prefs::input_dead_zone(&p, true, player));
        let cap = f64::from(prefs::input_buffer_cap(&p, player));
        Self::plus_minus(cx, "UI_PlusMinusHorizontalDeadzone", |r, cx| r.set_value(cx, h));
        Self::plus_minus(cx, "UI_PlusMinusVerticalDeadzone", |r, cx| r.set_value(cx, v));
        Self::plus_minus(cx, "UI_PlusMinusInputBufferCap", |r, cx| r.set_value(cx, cap));
        for (name, key) in [
            ("UI_ToggleInputBufferHold", InputBool::InputBufferHold),
            ("UI_ToggleDoubleCrouch", InputBool::DoubleCrouchJumpDown),
            ("UI_ToggleUpWillJump", InputBool::UpToJump),
            ("UI_ToggleAttackPicksUpItems", InputBool::AttackPicksUpItems),
        ] {
            let on = prefs::input_bool(&p, key, player);
            Self::toggle(cx, name, |t, cx| t.set_on(cx, on));
        }
        let ff = FASTFALL_IDS.get(usize::from(prefs::input_fastfall(&p, player))).copied().unwrap_or("");
        Self::menu(cx, "UI_DropMenu_Fastfall", |m, cx| m.set_active_by_id(cx, ff));
        let rs = RIGHT_STICK_IDS.get(usize::from(prefs::input_right_stick(&p, player))).copied().unwrap_or("");
        Self::menu(cx, "UI_DropMenu_RightStick", |m, cx| m.set_active_by_id(cx, rs));
        let on = prefs::input_bool(&p, InputBool::IgnoreRightStickDiagonals, player);
        Self::toggle(cx, "UI_ToggleIgnoreRightStickDiagonals", |t, cx| t.set_on(cx, on));
    }

    fn dialog(cx: &mut Cx<'_>, d: [&str; 4], left: u32, right: u32, dismissed: Option<u32>) {
        let t = |cx: &Cx<'_>, k: &str| cx.view.game_text(k).to_string();
        let params = DialogParams {
            title: t(cx, d[0]),
            body: t(cx, d[1]),
            button_left_label: t(cx, d[2]),
            button_right_label: t(cx, d[3]),
            ..DialogParams::default()
        };
        let id = Manager::show_dialog(cx, params);
        let me = cx.w;
        if let Some(dl) = cx.view.dialog(id) {
            if let Some(x) = dismissed {
                dl.dialog_dismissed().add(Handler { owner: me, id: x });
            }
            dl.button_right_clicked().add(Handler { owner: me, id: right });
            dl.button_left_clicked().add(Handler { owner: me, id: left });
        }
    }

    fn set_up_jump(&mut self, cx: &mut Cx<'_>, v: bool) {
        prefs::set_input_bool(&mut cx.prefs.input, InputBool::UpToJump, v, self.viewing_player);
        self.changed(cx);
    }

    /// @11871: turned off, written at once (@883); turned on, the dialog asks first unless Suppress Dialogs. No
    /// (left, OnDeclineUpToJump @12176) draws the switch off with no write; Yes (right, @12171 -> @883) writes.
    /// dialogDismissed is not bound: a dialog closed by neither button leaves the switch drawn on while the store
    /// keeps off.
    fn on_up_will_jump(&mut self, cx: &mut Cx<'_>, on: bool) {
        if !on {
            self.set_up_jump(cx, false);
        } else if prop_bool(cx, "Suppress Dialogs") {
            self.set_up_jump(cx, true);
        } else {
            Self::dialog(cx, UP_JUMP_DIALOG, T_UP_JUMP_NO, T_UP_JUMP_YES, None);
        }
    }

    fn set_fastfall(&mut self, cx: &mut Cx<'_>, v: u8) {
        prefs::set_input_fastfall(&mut cx.prefs.input, v, self.viewing_player);
        self.changed(cx);
    }

    /// @12219 (the menu has drawn the item already): hold writes 0 (@12317); tap asks first unless Suppress Dialogs
    /// (@1788). Yes (left) is OnConfirmTapFastfall @12415 (writes 1); No (right) and a dismiss are
    /// OnRejectTapFastfall @12513 (the menu back on hold, then @12317 writes 0). Another ID writes nothing.
    fn on_fastfall(&mut self, cx: &mut Cx<'_>, id: &str) {
        match id {
            "hold" => self.set_fastfall(cx, 0),
            "tap" if prop_bool(cx, "Suppress Dialogs") => self.set_fastfall(cx, 1),
            "tap" => Self::dialog(cx, TAP_FASTFALL_DIALOG, T_TAP_YES, T_TAP_NO, Some(T_TAP_NO)),
            _ => {}
        }
    }

    pub fn interactors(cx: &mut Cx<'_>) -> Vec<WidgetId> {
        let mut list = Vec::new();
        for n in INTERACTOR_ORDER {
            let w = find(cx, n);
            let inner = if n.starts_with("UI_DropMenu") { "mainInteractor" } else { "UI_Interactor" };
            list.push(cx.view.tree().find_in(w, inner));
        }
        for n in PLUS_MINUS_ORDER {
            list.extend(Self::plus_minus(cx, n, |_, cx| PlusMinusHorizontal::interactors(cx)).unwrap_or_default());
        }
        list
    }

    /// Update Context @8429: BatchSetContext and BatchSetPlayerIndex over GetInteractors, then the drop menus'
    /// UpdateContext.
    pub fn update_context(cx: &mut Cx<'_>, ctx: &str, player: i32) {
        let list = Self::interactors(cx);
        batch_set_context(cx, &list, ctx);
        for i in list {
            if let Some(s) = cx.view.manager.interactor_mut(i) {
                s.player_index = player;
            }
        }
        for n in ["UI_DropMenu_Fastfall", "UI_DropMenu_RightStick"] {
            Self::menu(cx, n, |_, cx| DropMenu::update_context(cx, ctx, Some(player)));
        }
    }

    /// SetScrollBox @12039 -> @1760.
    pub fn set_scroll_box(cx: &mut Cx<'_>, sb: WidgetId) {
        let list = Self::interactors(cx);
        batch_set_scroll_box(cx, &list, sb);
    }
}

impl Controller for PreferencesToggles {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        // the component bound events (bound before OnInitialized runs); each handler ends at @1005 (TogglesChanged)
        let me = cx.w;
        for (i, (name, _)) in BOOL_ROWS.iter().enumerate() {
            let h = Handler { owner: me, id: T_BOOL + i as u32 };
            Self::toggle(cx, name, |t, _| t.on_toggle_changed.add(h));
        }
        for (name, id) in [
            ("UI_PlusMinusHorizontalDeadzone", T_HDZ), // @12817
            ("UI_PlusMinusVerticalDeadzone", T_VDZ),   // @12922
            ("UI_PlusMinusInputBufferCap", T_CAP),     // @13027
        ] {
            Self::plus_minus(cx, name, |r, _| r.on_value_changed.add(Handler { owner: me, id }));
        }
        Self::toggle(cx, "UI_ToggleUpWillJump", |t, _| t.on_toggle_changed.add(Handler { owner: me, id: T_UP_JUMP }));
        Self::menu(cx, "UI_DropMenu_Fastfall", |m, _| m.on_click.add(Handler { owner: me, id: T_FASTFALL }));
        Self::menu(cx, "UI_DropMenu_RightStick", |m, _| m.on_click.add(Handler { owner: me, id: T_RIGHT_STICK }));
        // OnInitialized @2627: RefreshPlayerIndex(Viewing Player Index), then the platform texts @2650. The children
        // read their label and tooltip in their PreConstruct / Construct, which run after this.
        let p = self.viewing_player;
        self.refresh_player_index(cx, p);
        Self::platform_texts(cx);
    }

    /// Construct @12687: Large Deadzone collapsed, always (nothing shows it again); the buffer cap row's Maximum
    /// 30 + 1 (its instance value already).
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let w = find(cx, "UI_ToggleLargeDeadzone");
        wd(cx, w).vis = Vis::Collapsed;
        let cap = find(cx, "UI_PlusMinusInputBufferCap");
        cx.view.tree_mut().set_prop(cap, "Maximum", json!(30 + 1));
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let player = self.viewing_player;
        match id {
            n if (T_BOOL..T_BOOL + BOOL_ROWS.len() as u32).contains(&n) => {
                // @11699-11739 -> @398-864: every local PFGPlayerController's UpdateDeadzone. Nothing reads
                // deadzoneLarge (no Blueprint does), and the row is never shown.
                if let Some(&on) = arg.downcast_ref::<bool>() {
                    prefs::set_input_bool(&mut cx.prefs.input, BOOL_ROWS[(n - T_BOOL) as usize].1, on, player);
                    self.changed(cx);
                }
            }
            T_HDZ | T_VDZ | T_CAP => {
                if let Some(&(v, _)) = arg.downcast_ref::<(f64, f64)>() {
                    let s = &mut cx.prefs.input;
                    match id {
                        T_HDZ => prefs::set_input_dead_zone(s, false, v, player),
                        T_VDZ => prefs::set_input_dead_zone(s, true, v, player),
                        _ => prefs::set_input_buffer_cap(s, v, player),
                    }
                    self.changed(cx);
                }
            }
            T_UP_JUMP => {
                if let Some(&on) = arg.downcast_ref::<bool>() {
                    self.on_up_will_jump(cx, on);
                }
            }
            T_UP_JUMP_NO => {
                Self::toggle(cx, "UI_ToggleUpWillJump", |t, cx| t.set_on(cx, false));
            }
            T_UP_JUMP_YES => self.set_up_jump(cx, true),
            T_TAP_YES => self.set_fastfall(cx, 1),
            T_TAP_NO => {
                Self::menu(cx, "UI_DropMenu_Fastfall", |m, cx| m.set_active_by_id(cx, "hold"));
                self.set_fastfall(cx, 0);
            }
            T_FASTFALL => {
                if let Some((_, id)) = arg.downcast_ref::<(i32, String)>() {
                    self.on_fastfall(cx, id);
                }
            }
            // @13455: another ID writes nothing
            T_RIGHT_STICK => {
                if let Some((_, id)) = arg.downcast_ref::<(i32, String)>()
                    && let Some(style) = RIGHT_STICK_IDS.iter().position(|s| s == id)
                {
                    prefs::set_input_right_stick(&mut cx.prefs.input, style as u8, player);
                    self.changed(cx);
                }
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        self.handle(cx, id, &());
    }
}

// ---------- UI_PreferencesControlsRework ----------

/// The rows of Toggles shown in controller mode only (@1497).
const CONTROLLER_ONLY_ROWS: [&str; 5] = [
    "UI_ToggleVibration",
    "UI_PlusMinusHorizontalDeadzone",
    "UI_PlusMinusVerticalDeadzone",
    "UI_DropMenu_RightStick",
    "UI_ToggleIgnoreRightStickDiagonals",
];

const C_TOGGLES_CHANGED: u32 = 1;
const C_SET_CHANGED: u32 = 2;
const C_RESET: u32 = 3;
const C_MODE: u32 = 4;
const C_REFRESHED: u32 = 5;
const C_PLAYER_MENU: u32 = 6;
const C_SCROLL: u32 = 7;

/// The Controls page: the binding rows and "Additional Options" (UI_PreferencesControls-Toggles) in a scroll box.
/// The top bar (title, player menu, Steam controller note): ToggleTopBar @2363 shows it with Steam (not in the port)
/// or a Title (the instance's: UI_InputBindingModal's "Controls: P0"; the Settings panel's page has none).
#[derive(Debug)]
pub struct PreferencesControls {
    pub preferences_changed_dispatch: Event<()>,
    /// Current Context.
    context: String,
    /// Context Player Index.
    context_player: i32,
    /// Viewing Player Index.
    pub viewing_player: usize,
}

impl PreferencesControls {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PreferencesControls {
            preferences_changed_dispatch: Event::default(),
            context: NULL_GUID.to_string(),
            context_player: 0,
            viewing_player: 0,
        })
    }

    /// The binding set's widget ('Bindings').
    pub fn binding_set(cx: &Cx<'_>) -> WidgetId {
        find(cx, "Bindings")
    }

    fn set<R>(cx: &mut Cx<'_>, f: impl FnOnce(&mut InputBindingSet, &mut Cx<'_>) -> R) -> Option<R> {
        let w = Self::binding_set(cx);
        cx.visit::<InputBindingSet, _>(w, f)
    }

    fn toggles<R>(cx: &mut Cx<'_>, f: impl FnOnce(&mut PreferencesToggles, &mut Cx<'_>) -> R) -> Option<R> {
        let w = find(cx, "Toggles");
        cx.visit::<PreferencesToggles, _>(w, f)
    }

    fn scrollbox(cx: &Cx<'_>) -> WidgetId {
        find(cx, "controlsScrollbox")
    }

    /// The binding set's keyboard mode.
    pub fn keyboard_mode(cx: &mut Cx<'_>) -> bool {
        Self::set(cx, |s, _| s.keyboard_mode).unwrap_or(false)
    }

    /// Set Title @2023: the label's text, Hidden when it is empty (Conditional Visibility(.., 2, 4)).
    pub fn set_title(cx: &mut Cx<'_>, title: &str) {
        let label = find(cx, "titleLabel");
        let l = wd(cx, label);
        l.text = title.to_string();
        l.vis = if title.is_empty() { Vis::Hidden } else { Vis::SelfHitTestInvisible };
    }

    /// Toggle Input Panel(Player Index): the rows stop listening and are built for that player, and Toggles shows
    /// that player's preferences.
    pub fn toggle_input_panel(&mut self, cx: &mut Cx<'_>, player: usize) {
        self.viewing_player = player;
        Self::set(cx, |s, cx| {
            s.stop_listening(cx);
            s.build_inputs(cx, player);
        });
        Self::toggles(cx, |t, cx| t.refresh_player_index(cx, player));
    }

    /// FocusFirst @1436: the player menu takes the focus; the bytecode has no test of the bar's visibility.
    pub fn focus_first(cx: &mut Cx<'_>) {
        let m = find(cx, "playerMenu");
        let i = cx.view.tree().find_in(m, "mainInteractor");
        set_focused(cx, i, false);
    }

    /// Update Context(guid, Player Index): Toggles, the binding set and the player menu with the player, then the
    /// page's own interactor (Btn_SteamSupport).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str, player: i32) {
        self.context = ctx.to_string();
        self.context_player = player;
        let t = find(cx, "Toggles");
        PreferencesToggles::update_context(&mut cx.with(t), ctx, player);
        // (the set is in hand when its own InputsRefreshed reached here: its call waits for it)
        let (set, c) = (Self::binding_set(cx), ctx.to_string());
        run_on::<InputBindingSet>(cx, set, move |s, cx| s.update_context(cx, &c, player));
        let m = find(cx, "playerMenu");
        DropMenu::update_context(&mut cx.with(m), ctx, Some(player));
        let steam = find(cx, "Btn_SteamSupport");
        let i = ButtonSecondary::interactor(cx, steam);
        batch_set_context(cx, &[i], ctx);
        if let Some(s) = cx.view.manager.interactor_mut(i) {
            s.player_index = player;
        }
    }
}

impl Controller for PreferencesControls {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        // the bindings of InitControlsPanel @1831 and the component bound events: TogglesChanged -> OnTogglesChanged
        // @299, the binding set's PreferencesChangedDispatch -> OnPreferencesChanged @458, its ResetToDefaultsEvent
        // @2217, its mode change @1497, InputsRefreshed @384, the player menu @919
        let me = cx.w;
        let h = |id| Handler { owner: me, id };
        Self::toggles(cx, |t, _| t.toggles_changed.add(h(C_TOGGLES_CHANGED)));
        Self::set(cx, |s, _| {
            s.preferences_changed_dispatch.add(h(C_SET_CHANGED));
            s.reset_to_defaults_event.add(h(C_RESET));
            s.mode_changed.add(h(C_MODE));
            s.inputs_refreshed.add(h(C_REFRESHED));
        });
        let menu = find(cx, "playerMenu");
        cx.visit::<DropMenu, _>(menu, |m, cx| {
            m.on_click.add(h(C_PLAYER_MENU));
            m.set_active_by_id(cx, "player1");
        });
        let steam = find(cx, "steamControllerInfoContainer");
        wd(cx, steam).vis = Vis::Collapsed;
        // ToggleTopBar @2363: shown when IsSteamAvailable (false) OR the Title is not empty; the rows' padding top
        // 220 shown, 160 hidden (@2538-2924); SetScrollbarPadding(0, 255 shown, 185 hidden, 20, 80) (@2965-3322)
        let top_bar = !text(cx.view.tree().props(cx.w).get("Title")).is_empty();
        let hb = find(cx, "HorizontalBox_0");
        wd(cx, hb).vis = if top_bar { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        let inputs = find(cx, "inputs");
        wd(cx, inputs).pad = Margin { l: 0.0, t: if top_bar { 220.0 } else { 160.0 }, r: 20.0, b: 0.0 };
        let sb = Self::scrollbox(cx);
        let pad = Margin { l: 0.0, t: if top_bar { 255.0 } else { 185.0 }, r: 20.0, b: 80.0 };
        set_scrollbar_padding(cx.view.tree_mut(), sb, pad);
        // SetupScrolling @523
        Self::set(cx, |s, cx| s.set_scroll_box(cx, sb));
        Self::toggles(cx, |_, cx| PreferencesToggles::set_scroll_box(cx, sb));
        cx.view.manager.on_scroll.add(h(C_SCROLL));
        cx.view.manager.register_scrollbox(sb);
    }

    /// PreConstruct @1234: Set Title(Title); the Suppress Dialogs of the page given to Toggles and the binding set.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let title = text(cx.view.tree().props(cx.w).get("Title"));
        Self::set_title(cx, &title);
        let suppress = prop_bool(cx, "Suppress Dialogs");
        let (t, s) = (find(cx, "Toggles"), Self::binding_set(cx));
        cx.view.tree_mut().set_prop(t, "Suppress Dialogs", json!(suppress));
        cx.view.tree_mut().set_prop(s, "Suppress Dialogs", json!(suppress));
        let menu = find(cx, "playerMenu");
        let hb = cx.view.tree().find_in(menu, "HoverBacking");
        wd(cx, hb).vis = Vis::Hidden;
        let label = cx.view.tree().find_in(menu, "Label");
        let l = wd(cx, label);
        l.font_size = 24.0;
        l.letter_spacing = 0.0;
        l.opacity = 1.0;
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let sb = Self::scrollbox(cx);
        let me = cx.w;
        cx.view.manager.unregister_scrollbox(sb);
        cx.view.manager.on_scroll.remove(Handler { owner: me, id: C_SCROLL });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            C_TOGGLES_CHANGED => cx.emit(self, |c: &PreferencesControls| &c.preferences_changed_dispatch, &()),
            // OnPreferencesChanged @458: PreferencesChangedDispatch, then Toggles.RefreshPlayerIndex(Viewing Player
            // Index)
            C_SET_CHANGED => {
                cx.emit(self, |c: &PreferencesControls| &c.preferences_changed_dispatch, &());
                let p = self.viewing_player;
                Self::toggles(cx, |t, cx| t.refresh_player_index(cx, p));
            }
            // ResetToDefaultsEvent @2217: PreferencesManager Set-Input-AllDefaults(Legacy, Player Index), then
            // Toggles.RefreshPlayerIndex(Player Index)
            C_RESET => {
                if let Some(&(player, legacy)) = arg.downcast_ref::<(usize, bool)>() {
                    prefs::set_input_all_defaults(&mut cx.prefs.input, legacy, player);
                    Self::toggles(cx, |t, cx| t.refresh_player_index(cx, player));
                }
            }
            // OnBindingSetModeChanged @1497-1663: Conditional Visibilities(the controller-only rows, Keyboard Mode,
            // Collapsed, Visible)
            C_MODE => {
                let keyboard = arg.downcast_ref::<bool>().copied().unwrap_or(false);
                let t = find(cx, "Toggles");
                for n in CONTROLLER_ONLY_ROWS {
                    let w = cx.view.tree().find_in(t, n);
                    wd(cx, w).vis = if keyboard { Vis::Collapsed } else { Vis::Visible };
                }
            }
            C_REFRESHED => {
                let (ctx, p) = (self.context.clone(), self.context_player);
                self.update_context(cx, &ctx, p);
            }
            C_PLAYER_MENU => {
                if let Some(&(index, _)) = arg.downcast_ref::<(i32, String)>() {
                    self.toggle_input_panel(cx, usize::try_from(index).unwrap_or(0));
                }
            }
            // OnScroll @815: the context's player only, then UI_Functions.ScrollList
            C_SCROLL => {
                if let Some(&(player, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>()
                    && player == self.context_player
                {
                    let sb = Self::scrollbox(cx);
                    scroll_list(cx.view.tree_mut(), sb, amount);
                }
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        self.handle(cx, id, &());
    }
}
