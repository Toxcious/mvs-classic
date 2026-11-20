//! UI_ContextButton, UI_ContextMenu and UI_DropMenuRework.

use std::any::Any;

use indexmap::IndexMap;
use serde_json::Value;

use super::bar_buttons::{P_ID, P_LABEL, P_TOOLTIP};
use super::buttons::{CHECK_BOX, CLEAR, DISABLED, HOVER_BACKING, QUAD_FILL, WHITE};
use super::interactor::{InteractorState, batch_set_context, set_focused};
use super::tooltip::{TooltipTarget, tooltip_params};
use super::{bind, find, focus_frame, prop, prop_bool, prop_f64, quad_color, set_id, set_tooltip, sound_overrides, wd};
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::manager::{
    ContextMenu as ManagedContextMenu, ContextMenuButton, ContextMenuParams, Manager, NULL_GUID,
};
use crate::menu::scroll::{scroll_list, set_scroll_bar_visibility};
use crate::menu::view::MenuView;
use crate::menu::widgets::common::{rgba, text};
use crate::menu::widgets::section_bar::{SectionBar, index_of_id, set_button_params};
use crate::slate::{HAlign, Margin, Stretch, Vis, WidgetId};

// ---------- UI_ContextButton ----------

const CONTEXT_FOCUSED: Rgba = [0.006995, 0.008023, 0.022174, 1.0];
const CONTEXT_IDLE: Rgba = [0.194618, 0.571125, 0.552012, 1.0];
const H_FOCUS: u32 = 1;
const H_UNFOCUS: u32 = 2;
const H_CLICK: u32 = 3;

fn shown(cx: &Cx<'_>, w: WidgetId) -> bool {
    cx.view.tree().widget(w).shown()
}

/// A row of a context menu. The label wraps (AutoWrapText) unless the menu has an OverrideWidth (the drop menus):
/// SetAsScaleNoWrap.
#[derive(Debug, Default)]
pub struct ContextButton {
    pub on_click: Event<()>,
}

impl ContextButton {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ContextButton::default())
    }

    pub fn interactor(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
        cx.view.tree().find_in(w, "UI_Interactor")
    }

    /// `init(label)`.
    pub fn init(cx: &mut Cx<'_>, label: &str) {
        let l = find(cx, "label");
        wd(cx, l).text = label.to_string();
        let i = find(cx, "UI_Interactor");
        set_id(cx, i, label);
    }

    /// @0-34: ScaleBox_1.SetStretch(3) and the label's AutoWrapText off.
    pub fn set_as_scale_no_wrap(cx: &mut Cx<'_>) {
        let b = find(cx, "ScaleBox_1");
        let w = wd(cx, b);
        w.stretch = Stretch::ScaleToFitX;
        w.clip = true;
        let l = find(cx, "label");
        wd(cx, l).auto_wrap = false;
    }
}

impl Controller for ContextButton {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = find(cx, "UI_Interactor");
        // @10-410: the row's own Tooltip centred under it, over the menu (ZOrder 100)
        let tip = text(Some(&prop(cx, "Tooltip")));
        set_tooltip(cx, i, tooltip_params(&tip, !tip.is_empty(), 0.0, 0.0, 1));
        if let Some(t) = cx.view.tree().find_opt(i, "UI_TooltipTarget") {
            cx.visit::<TooltipTarget, _>(t, |t, _| t.z_order = 100);
        }
        bind(cx, i, |s: &mut InteractorState| &mut s.on_click, H_CLICK);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, H_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, H_UNFOCUS);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let label = text(Some(&prop(cx, "buttonLabel")));
        if !label.is_empty() {
            ContextButton::init(cx, &label);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        let (l, f) = (find(cx, "label"), find(cx, "focused"));
        match id {
            H_CLICK => cx.emit(self, |c| &c.on_click, &()),
            H_FOCUS => {
                wd(cx, l).color = CONTEXT_FOCUSED;
                wd(cx, f).vis = Vis::SelfHitTestInvisible;
            }
            H_UNFOCUS => {
                wd(cx, l).color = CONTEXT_IDLE;
                wd(cx, f).vis = Vis::Hidden;
            }
            _ => {}
        }
    }
}

// ---------- UI_ContextMenu ----------

const CM_BUTTON_CLICKED: u32 = 1;
const CM_DISMISS_CLICK: u32 = 2;
const CM_TIMER: u32 = 3;
const CM_SCROLL: u32 = 4;
const CM_HIDE: u32 = 1;

/// The list UI_Manager.ShowContextMenu puts over the screen: a full-screen dismissTarget and the `menu` box placed
/// by its canvas slot. Not ported, none of it reachable: the close button entry (Init @1257-1412: id closeButton,
/// label "CLOSE", click = Hide Self, the manager's OnCancel = HideIfFocused @3577-3760) since every decoded caller
/// passes showCloseButton false, and the ForSCOnly typeface of the row with the id zh-Hans (@1950-2412, the
/// language menu). The list's scroll bar (the asset has it Collapsed) shows only with more rows than
/// ShowScrollBarAfterCount and EnableScrollBar (@1171), which no drop menu of the bundle sets.
/// Deviation: Hide Self (the dismiss target's click, a button click with Auto Close) runs on the next frame: the
/// manager's hide call cannot reach this controller while its own handler runs.
#[derive(Debug)]
pub struct ContextMenu {
    pub on_dismissed: Event<()>,
    /// buttonClickedEvent(id).
    pub button_clicked_event: Event<String>,
    guid: String,
    id: String,
    params: Option<ContextMenuParams>,
    interactors: IndexMap<String, WidgetId>,
    focus_timer: String,
    screen_offset: [f64; 2],
    first_position_update: bool,
    viewport: [f64; 2],
    /// DetectViewportChange, looping 0.25 s.
    detect_timer: f64,
    /// UpdatePosition 0.1 s after a viewport change.
    move_timer: f64,
    scroll_bar_item_count: i32,
    adjust_scroll_box: bool,
    /// (the port's own) The list was fitted to the room below its top (`fit_to_screen`).
    fitted: bool,
}

impl ContextMenu {
    pub fn make(cx: &mut Cx<'_>) -> Box<dyn Controller> {
        let _ = cx;
        Box::new(ContextMenu {
            on_dismissed: Event::default(),
            button_clicked_event: Event::default(),
            guid: NULL_GUID.to_string(),
            id: String::new(),
            params: None,
            interactors: IndexMap::new(),
            focus_timer: NULL_GUID.to_string(),
            screen_offset: [0.0; 2],
            first_position_update: true,
            viewport: [0.0; 2],
            detect_timer: -1.0,
            move_timer: -1.0,
            scroll_bar_item_count: 0,
            adjust_scroll_box: false,
            fitted: false,
        })
    }

    fn dismiss_target(cx: &Cx<'_>) -> WidgetId {
        find(cx, "dismissTarget")
    }

    pub fn set_context(&mut self, cx: &mut Cx<'_>, ctx: &str, p: i32) {
        self.guid = ctx.to_string();
        let list: Vec<WidgetId> = self.interactors.values().copied().collect();
        batch_set_context(cx, &list, ctx);
        let dt = Self::dismiss_target(cx);
        for &i in list.iter().chain([&dt]) {
            if let Some(s) = cx.view.manager.interactor_mut(i) {
                s.player_index = p;
            }
        }
        // (the dismiss target's context)
        if let Some(s) = cx.view.manager.interactor_mut(dt) {
            s.context = ctx.to_string();
        }
        let t = self.focus_timer.clone();
        self.focus_timer = Manager::add_timer(cx, &t, 0.1);
    }

    /// The matcher's centre on the viewport plus the offset, pushed back inside the viewport by `screenPadding`. The
    /// tests use the slot position without the alignment, and the offset accumulates over calls.
    pub fn update_position(&mut self, cx: &mut Cx<'_>) {
        let Some(params) = self.params.clone() else { return };
        const SCREEN_PADDING: f64 = 20.0;
        let dpi = cx.view.dpi;
        let [mut x, mut y] = params.position_offset;
        if let Some(m) = params.position_matcher {
            let [mw, mh] = cx.view.tree().arranged_size(m);
            let c = cx.view.local_to_viewport(m, mw / 2.0, mh / 2.0);
            x += c[0];
            y += c[1];
        }
        let [vw, vh] = cx.view.viewport_pixels();
        let (vx, vy) = (vw / dpi, vh / dpi);
        let menu = find(cx, "menu");
        let [sx, sy] = cx.view.tree().arranged_size(menu);
        let o = &mut self.screen_offset;
        if y + sy > vy {
            o[1] += vy - (y + sy) - SCREEN_PADDING;
        }
        if y < 0.0 {
            o[1] += -y + SCREEN_PADDING;
        }
        if x + sx > vx {
            o[0] += vx - (x + sx) - SCREEN_PADDING;
        }
        if x < 0.0 {
            o[0] += -x + SCREEN_PADDING;
        }
        let (ox, oy) = (o[0], o[1]);
        let m = wd(cx, menu);
        m.offsets.l = x + ox;
        m.offsets.t = y + oy;
        if self.first_position_update {
            let me = cx.w;
            wd(cx, me).opacity = 0.0;
            cx.view.play(me, "EnterAnim", false, 1.0);
        }
        self.first_position_update = false;
    }

    /// (the port's own) Once the list is laid out: when its rows reach past the viewport's bottom (less the screen
    /// padding), it keeps the rows that fit (MIN_ROWS at least) and scrolls the rest, with its scroll bar.
    /// Deviation: the build caps a list only by ShowScrollBarAfterCount, which no drop menu of the bundle sets; a
    /// list taller than the room below it ran off the screen (the custom lobby's experimental rules).
    fn fit_to_screen(&mut self, cx: &mut Cx<'_>) {
        const SCREEN_PADDING: f64 = 20.0;
        const MIN_ROWS: usize = 3;
        let (menu, sbox, container) = (find(cx, "menu"), find(cx, "SizeBox"), find(cx, "Container"));
        let t = cx.view.tree();
        let [_, mh] = t.arranged_size(menu);
        let [_, sh] = t.arranged_size(sbox);
        let first = t.widget(container).children.first().copied();
        let row = first.map_or(0.0, |c| t.arranged_size(c)[1]);
        if mh <= 0.0 || row <= 0.0 {
            return;
        }
        self.fitted = true;
        let m = t.widget(menu);
        let top = m.offsets.t - m.align[1] * mh;
        let vy = cx.view.viewport_pixels()[1] / cx.view.dpi;
        if top + mh <= vy - SCREEN_PADDING {
            return;
        }
        let extra = mh - sh;
        let room = vy - SCREEN_PADDING - top - extra;
        let mut rows = ((room / row).floor().max(0.0) as usize).max(MIN_ROWS);
        if self.adjust_scroll_box && self.scroll_bar_item_count > 0 {
            rows = rows.min(self.scroll_bar_item_count as usize);
        }
        let buttons = self.interactors.len();
        if rows >= buttons {
            return;
        }
        wd(cx, sbox).h_override = row * rows as f64;
        // (a list too tall even at MIN_ROWS is moved up to the padding)
        let over = top + extra + row * rows as f64 - (vy - SCREEN_PADDING);
        if over > 0.0 {
            wd(cx, menu).offsets.t -= over;
        }
        if !self.adjust_scroll_box {
            self.adjust_scroll_box = true;
            if self.params.as_ref().is_some_and(|p| p.use_position_matcher_width && p.position_matcher.is_some()) {
                wd(cx, menu).w_override += 22.0;
            }
            let sb = find(cx, "ScrollBox");
            set_scroll_bar_visibility(cx.view.tree_mut(), sb, Vis::Visible);
        }
        self.scroll_bar_item_count = rows as i32;
    }

    /// Hide Self / Hide Me (next frame, see the type).
    fn hide_self(cx: &mut Cx<'_>) {
        cx.view.delay(0.0, Handler { owner: cx.w, id: CM_HIDE });
    }
}

impl ManagedContextMenu for ContextMenu {
    fn id(&self) -> &str {
        &self.id
    }

    fn guid(&self) -> &str {
        &self.guid
    }

    fn on_dismissed(&mut self) -> &mut Event<()> {
        &mut self.on_dismissed
    }

    /// Init: runs before the menu is on the viewport.
    fn init(&mut self, cx: &mut Cx<'_>, params: ContextMenuParams, p: i32) -> Vec<(String, WidgetId)> {
        self.id = params.id.clone();
        self.scroll_bar_item_count = params.show_scroll_bar_after_count;
        self.adjust_scroll_box =
            params.buttons.len() as i32 > params.show_scroll_bar_after_count && params.enable_scroll_bar;
        let (menu, label, container) = (find(cx, "menu"), find(cx, "label"), find(cx, "Container"));
        wd(cx, menu).align = params.alignment;
        if let (true, Some(matcher)) = (params.use_position_matcher_width, params.position_matcher) {
            let mw = cx.view.tree().arranged_size(matcher)[0];
            wd(cx, menu).w_override = (if params.override_width > 0.0 { params.override_width } else { mw })
                + if self.adjust_scroll_box { 22.0 } else { 0.0 };
        }
        // @1171 ScrollBox.SetScrollBarVisibility(Visible)
        if self.adjust_scroll_box {
            let sb = find(cx, "ScrollBox");
            set_scroll_bar_visibility(cx.view.tree_mut(), sb, Vis::Visible);
        }
        for c in cx.view.tree().widget(container).children.clone() {
            MenuView::discard(cx, c);
        }
        if !params.label.is_empty() {
            // a new VerticalBoxSlot with the padding set
            let l = wd(cx, label);
            l.text = params.label.clone();
            l.pad = Margin { l: 0.0, t: 0.0, r: 0.0, b: 2.0 };
            MenuView::add_child(cx, container, label, None);
        }
        let me = cx.w;
        for button in &params.buttons {
            let bw = MenuView::create(cx, "UI_ContextButton");
            if params.override_width > 0.0 {
                cx.visit::<ContextButton, _>(bw, |_, cx| ContextButton::set_as_scale_no_wrap(cx));
            }
            let l = cx.view.tree().find_in(bw, "label");
            let x = wd(cx, l);
            x.text = button.label.clone();
            x.typeface = Some("NormsCond-ExtraBoldItalic".into());
            x.font_size = 20.0;
            x.letter_spacing = 0.0;
            let iw = ContextButton::interactor(cx, bw);
            self.interactors.insert(button.id.clone(), iw);
            // @2683, @3967-4320: a button's tooltip
            if !button.tooltip.is_empty() {
                set_tooltip(cx, iw, tooltip_params(&button.tooltip, true, 0.0, 45.0, 2));
            }
            set_id(cx, iw, &button.id);
            MenuView::add_child(cx, container, bw, None);
            if let Some(s) = cx.view.manager.interactor_mut(iw) {
                s.on_click_with_source.add(Handler { owner: me, id: CM_BUTTON_CLICKED });
            }
        }
        if params.set_context {
            let g = self.guid.clone();
            self.set_context(cx, &g, p);
        }
        self.params = Some(params);
        // InitComplete: ForceLayoutPrepass, then the position (the box's cached size is still zero here)
        self.update_position(cx);
        self.interactors.iter().map(|(k, v)| (k.clone(), *v)).collect()
    }

    fn hide(&mut self, cx: &mut Cx<'_>) {
        for &i in self.interactors.values() {
            if let Some(s) = cx.view.manager.interactor_mut(i) {
                s.on_click.clear();
                s.on_click_with_source.clear();
            }
        }
        self.button_clicked_event.clear();
        let me = cx.w;
        MenuView::discard(cx, me);
        cx.emit(self, |c| &c.on_dismissed, &());
    }
}

impl Controller for ContextMenu {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.guid = cx.view.manager.new_guid();
        let dt = Self::dismiss_target(cx);
        bind(cx, dt, |s: &mut InteractorState| &mut s.on_click, CM_DISMISS_CLICK);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        wd(cx, me).opacity = 0.0;
        cx.view.manager.on_timer_event.add(Handler { owner: me, id: CM_TIMER });
        cx.view.manager.on_scroll.add(Handler { owner: me, id: CM_SCROLL });
        self.viewport = cx.view.viewport_size();
        self.detect_timer = 0.25;
        let sb = find(cx, "ScrollBox");
        cx.view.manager.register_scrollbox(sb);
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        self.detect_timer = -1.0;
        self.move_timer = -1.0;
        self.params = None;
        self.interactors.clear();
        cx.view.manager.on_timer_event.remove(Handler { owner: me, id: CM_TIMER });
        cx.view.manager.on_scroll.remove(Handler { owner: me, id: CM_SCROLL });
        let sb = find(cx, "ScrollBox");
        cx.view.manager.unregister_scrollbox(sb);
    }

    fn timers(&mut self, cx: &mut Cx<'_>, dt: f64) {
        if self.move_timer >= 0.0 {
            self.move_timer -= dt;
            if self.move_timer <= 0.0 {
                self.move_timer = -1.0;
                self.update_position(cx);
            }
        }
        if self.detect_timer < 0.0 {
            return;
        }
        self.detect_timer -= dt;
        if self.detect_timer > 0.0 {
            return;
        }
        self.detect_timer += 0.25;
        let [w, h] = cx.view.viewport_size();
        if (w - self.viewport[0]).abs() > 0.0001 || (h - self.viewport[1]).abs() > 0.0001 {
            self.viewport = [w, h];
            self.move_timer = 0.1;
        }
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        let sbox = find(cx, "SizeBox");
        if self.adjust_scroll_box && cx.view.tree().widget(sbox).h_override <= 0.0 {
            let container = find(cx, "Container");
            let first = cx.view.tree().widget(container).children.first().copied();
            let h = first.map_or(0.0, |c| cx.view.tree().arranged_size(c)[1]);
            wd(cx, sbox).h_override = h * f64::from(self.scroll_bar_item_count);
        }
        if !self.fitted {
            self.fit_to_screen(cx);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            CM_DISMISS_CLICK => Self::hide_self(cx),
            CM_BUTTON_CLICKED => {
                let Some(&i) = arg.downcast_ref::<WidgetId>() else { return };
                let auto_close = self.params.as_ref().is_some_and(|p| p.auto_close);
                let ids: Vec<String> =
                    self.interactors.iter().filter(|(_, v)| **v == i).map(|(k, _)| k.clone()).collect();
                for k in ids {
                    cx.emit(self, |c| &c.button_clicked_event, &k);
                }
                if auto_close {
                    Self::hide_self(cx);
                }
            }
            CM_TIMER => {
                // FocusFirst
                if arg.downcast_ref::<String>() == Some(&self.focus_timer)
                    && let Some(&i) = self.interactors.values().next()
                {
                    set_focused(cx, i, false);
                }
            }
            // The wheel over a menu: UI_Functions.ScrollList(ScrollBox, amount x 2).
            CM_SCROLL => {
                if let Some(&(p, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>()
                    && p == 0
                {
                    let sb = find(cx, "ScrollBox");
                    scroll_list(cx.view.tree_mut(), sb, amount * 2.0);
                }
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == CM_HIDE {
            Manager::hide_own_context_menu(cx, self, 0);
        }
    }

    fn as_context_menu(&mut self) -> Option<&mut dyn ManagedContextMenu> {
        Some(self)
    }
}

// ---------- UI_DropMenuRework ----------

const D_BAR_CLICK: u32 = 1;
const D_MAIN_CLICK: u32 = 2;
const D_FOCUS: u32 = 3;
const D_UNFOCUS: u32 = 4;
const D_INTERACTABLE: u32 = 5;
const D_NOT_INTERACTABLE: u32 = 6;
const D_INTERACTOR_FOCUSED: u32 = 7;
const D_CTX_CLICK: u32 = 8;
const D_CTX_DISMISSED: u32 = 9;

/// An item of a row's Items made by the port (a label that is no Game.locres line).
pub fn own_item(label: &str, id: &str) -> Value {
    serde_json::json!({ P_LABEL: { "CultureInvariantString": label }, P_ID: id })
}

/// The row with its label and current selection; a click opens the list, as a context menu of the manager (Use
/// Context Menu, every instance of the bundle) or in place. hoverButton (a Button with no look that fills the row
/// under the label) shows the label's focus visuals while the mouse is on it. TopTooltipText is the interactor's
/// tip.
/// Deviation: the bar's params are read from the row's own Items (the bar holds the same list): the bar is busy
/// while its click event runs, when the row's handler asks for them.
#[derive(Debug, Default)]
pub struct DropMenu {
    /// (index, id).
    pub on_click: Event<(i32, String)>,
    /// OnClosed, with the row's CurrentSelectedID as it fires: the row is busy then, so a handler that reads the
    /// selection (UI_LAB_PlayerPanel's) reads it here.
    pub on_closed: Event<String>,
    pub current_selected_id: String,
    open: bool,
    items: Vec<Value>,
}

impl DropMenu {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(DropMenu::default())
    }

    fn bar(cx: &Cx<'_>) -> WidgetId {
        find(cx, "menuSectionBar")
    }

    fn main_interactor(cx: &Cx<'_>) -> WidgetId {
        find(cx, "mainInteractor")
    }

    fn use_context_menu(cx: &Cx<'_>) -> bool {
        prop_bool(cx, "Use Context Menu")
    }

    fn context_menu_id(cx: &Cx<'_>) -> String {
        prop(cx, "Context Menu ID").as_str().unwrap_or_default().to_string()
    }

    /// OnInitialized @2235 and Initialize (@7670 -> @2235): the focus visuals' interactor, the bar's Button Params
    /// = Items and its Initialize (the list again: a caller that changed Items, UI_QuickMatchSettings Validate
    /// @3833), the label. (The manager's OnInteractorFocused, bound at @2411, is taken in Construct here.)
    pub fn initialize(&mut self, cx: &mut Cx<'_>) {
        let fc = find(cx, "UI_FocusedCommon");
        let main = Self::main_interactor(cx);
        focus_frame(cx, fc, main);
        let items = prop(cx, "Items");
        self.items = items.as_array().cloned().unwrap_or_default();
        let bar = Self::bar(cx);
        set_button_params(cx, bar, items);
        cx.visit::<SectionBar, _>(bar, |b, cx| b.initialize(cx));
        let l = find(cx, "Label");
        let t = text(Some(&prop(cx, "LabelText")));
        wd(cx, l).text = t;
    }

    /// (the port's own) An item of the port's own (`label`, `id`) at the end of the row `menu`'s Items: a choice the
    /// build's list has no row for. The row is initialized again with the new list and keeps its selection; nothing
    /// changes when the list has it already.
    pub fn offer_own_item(cx: &mut Cx<'_>, menu: WidgetId, label: &str, id: &str) {
        let mut items = cx.view.tree().props(menu).get("Items").and_then(Value::as_array).cloned().unwrap_or_default();
        if index_of_id(&items, id) >= 0 {
            return;
        }
        items.push(own_item(label, id));
        cx.view.tree_mut().set_prop(menu, "Items", Value::Array(items));
        cx.visit::<DropMenu, _>(menu, |m, cx| {
            let selected = m.current_selected_id.clone();
            m.initialize(cx);
            m.set_active_by_id(cx, &selected);
        });
    }

    /// The row's interactors: the main one, then the bar's.
    pub fn interactors(cx: &mut Cx<'_>) -> Vec<WidgetId> {
        let bar = Self::bar(cx);
        let mut v = vec![Self::main_interactor(cx)];
        v.extend(cx.visit::<SectionBar, _>(bar, |b, _| b.interactors.clone()).unwrap_or_default());
        v
    }

    /// Whether the interactor is the row's own (the main one or one of its bar's: in the row's own tree).
    fn owns(cx: &Cx<'_>, i: WidgetId) -> bool {
        let t = cx.view.tree();
        let mut w = Some(i);
        while let Some(x) = w {
            if x == cx.w {
                return true;
            }
            w = t.widget(x).parent;
        }
        false
    }

    pub fn toggle_open(&mut self, cx: &mut Cx<'_>, open: bool) {
        self.open = open;
        let (idle, list) = (find(cx, "IdleContainer"), find(cx, "menuContainer"));
        let use_ctx = Self::use_context_menu(cx);
        if !open {
            if use_ctx || shown(cx, idle) {
                return;
            }
            wd(cx, list).vis = Vis::Collapsed;
            wd(cx, idle).vis = Vis::SelfHitTestInvisible;
            let id = self.current_selected_id.clone();
            cx.emit(self, |c| &c.on_closed, &id);
            return;
        }
        if !use_ctx {
            if shown(cx, list) {
                return;
            }
            wd(cx, list).vis = Vis::SelfHitTestInvisible;
            wd(cx, idle).vis = Vis::Collapsed;
            return;
        }
        let up = prop_bool(cx, "OpenUp");
        let mid = prop_bool(cx, "OpenMid");
        let fill = find(cx, "QuadFill");
        let arrow = find(cx, "arrow");
        wd(cx, arrow).transform.scale[1] = -1.0;
        quad_color(cx, fill, CONTEXT_FOCUSED);
        wd(cx, fill).opacity = 0.5;
        let border = find(cx, "QuadBorder");
        wd(cx, border).vis = Vis::SelfHitTestInvisible;
        // the fill's cached geometry: absolute size in device pixels, local size in its own units
        let dpi = cx.view.dpi;
        let [qw, _] = cx.view.tree().arranged_size(fill);
        let abs_x = cx.view.tree().painted(fill).map_or(0.0, |p| qw * p.transform[0].hypot(p.transform[1]));
        let buttons = self
            .items
            .iter()
            .map(|b| ContextMenuButton {
                id: b.get(P_ID).and_then(Value::as_str).unwrap_or_default().into(),
                label: text(b.get(P_LABEL)),
                tooltip: text(b.get(P_TOOLTIP)),
            })
            .collect();
        let params = ContextMenuParams {
            id: Self::context_menu_id(cx),
            buttons,
            show_close_button: false,
            set_context: true,
            position_matcher: Some(fill),
            position_offset: [
                (abs_x * -0.5) / dpi,
                (if mid {
                    0.0
                } else if up {
                    -18.0
                } else {
                    18.0
                }) / dpi,
            ],
            label: String::new(),
            use_position_matcher_width: true,
            alignment: [
                0.0,
                if mid {
                    0.5
                } else if up {
                    1.0
                } else {
                    0.0
                },
            ],
            auto_close: false,
            override_width: if prop_bool(cx, "WWShopFilter") { 0.0 } else { qw * 1.08 + 25.0 },
            enable_scroll_bar: prop_bool(cx, "EnableScrollBar"),
            show_scroll_bar_after_count: prop_f64(cx, "ShowScrollBarAfterCount", 0.0) as i32,
        };
        let player = prop_f64(cx, "Player Index", 0.0) as i32;
        let (menu, interactors) = Manager::show_context_menu(cx, params, player);
        let me = cx.w;
        if let Some(m) = cx.view.context_menu(menu) {
            m.on_dismissed().add(Handler { owner: me, id: D_CTX_DISMISSED });
        }
        for (_, i) in interactors {
            if let Some(s) = cx.view.manager.interactor_mut(i) {
                s.on_click_with_source.add(Handler { owner: me, id: D_CTX_CLICK });
            }
        }
    }

    pub fn close(&mut self, cx: &mut Cx<'_>) {
        self.toggle_open(cx, false);
        let id = Self::context_menu_id(cx);
        Manager::hide_context_menu(cx, &id, 0);
        let idle = find(cx, "IdleContainer");
        wd(cx, idle).opacity = 1.0;
    }

    /// Update Context(guid, Player Index) (UI_DropMenu @988-1116): BatchSetContext, then BatchSetPlayerIndex when a
    /// caller gives the player.
    pub fn update_context(cx: &mut Cx<'_>, ctx: &str, player: Option<i32>) {
        let own = Self::interactors(cx);
        batch_set_context(cx, &own, ctx);
        if let Some(p) = player {
            for i in own {
                if let Some(s) = cx.view.manager.interactor_mut(i) {
                    s.player_index = p;
                }
            }
        }
    }

    /// SetActiveByID @1972: CurrentSelectedID = the ID and the bar's SetCurrentIndexByID, both whether the list has
    /// it or not; the labels only for an ID the list has (ParamsForID Found, @2116).
    pub fn set_active_by_id(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.current_selected_id = id.to_string();
        let bar = Self::bar(cx);
        let idc = id.to_string();
        cx.visit::<SectionBar, _>(bar, |b, cx| b.set_current_index_by_id(cx, &idc));
        let i = index_of_id(&self.items, id);
        let Some(params) = usize::try_from(i).ok().and_then(|i| self.items.get(i)) else { return };
        let t = text(params.get(P_LABEL));
        for n in ["WrapSelectionLabel", "AutoWrapSelectionLabel"] {
            let l = find(cx, n);
            wd(cx, l).text = t.clone();
        }
    }

    /// Clicked @2808: CurrentSelectedID is the ID before ToggleOpen(false), so an in-place list's OnClosed sees the
    /// pick.
    fn clicked(&mut self, cx: &mut Cx<'_>, index: i32, id: &str) {
        self.current_selected_id = id.to_string();
        self.toggle_open(cx, false);
        self.set_active_by_id(cx, id);
        cx.emit(self, |c| &c.on_click, &(index, id.to_string()));
        let m = Self::context_menu_id(cx);
        Manager::hide_context_menu(cx, &m, 0);
        let idle = find(cx, "IdleContainer");
        wd(cx, idle).opacity = 1.0;
    }

    fn set_colors(cx: &mut Cx<'_>, border: Rgba, fill: Rgba, content: Rgba) {
        let (b, f) = (find(cx, "QuadBorder"), find(cx, "QuadFill"));
        quad_color(cx, b, border);
        quad_color(cx, f, fill);
        for n in ["Label", "WrapSelectionLabel", "AutoWrapSelectionLabel"] {
            let l = find(cx, n);
            wd(cx, l).color = content;
        }
    }

    fn hover_color(cx: &Cx<'_>) -> Rgba {
        let h = prop(cx, "HoverColor");
        if h.is_null() { HOVER_BACKING } else { rgba(&h) }
    }

    fn visuals(cx: &mut Cx<'_>, focused: bool) {
        let fill = find(cx, "QuadFill");
        wd(cx, fill).opacity = if focused { 1.0 } else { 0.4 };
        quad_color(cx, fill, QUAD_FILL);
        if prop_bool(cx, "RemovePadding") {
            return;
        }
        let l = find(cx, "Label");
        let w = wd(cx, l);
        w.opacity = if focused { 1.0 } else { 0.65 };
        w.font_size = if focused { 20.0 } else { 18.0 };
        let hb = find(cx, "HoverBacking");
        let c = if focused { Self::hover_color(cx) } else { CLEAR };
        wd(cx, hb).color = c;
    }
}

impl Controller for DropMenu {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.initialize(cx);
        let me = cx.w;
        let bar = Self::bar(cx);
        cx.visit::<SectionBar, _>(bar, |b, _| b.on_click.add(Handler { owner: me, id: D_BAR_CLICK }));
        let i = Self::main_interactor(cx);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_click, D_MAIN_CLICK);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, D_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, D_UNFOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_interactable, D_INTERACTABLE);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_not_interactable, D_NOT_INTERACTABLE);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let label = text(Some(&prop(cx, "LabelText")));
        let l = find(cx, "Label");
        wd(cx, l).text = label.clone();
        let sb = find(cx, "SizeBox_1");
        wd(cx, sb).h_align = if label.is_empty() { HAlign::Fill } else { HAlign::Right };
        let dc = prop(cx, "DivColor");
        let div = if dc.is_null() { HOVER_BACKING } else { rgba(&dc) };
        for (n, k) in [("DivTop", "HideTopDiv"), ("DivBottom", "HideBottomDiv")] {
            let hide = prop_bool(cx, k);
            let d = find(cx, n);
            let w = wd(cx, d);
            w.vis = if hide { Vis::Hidden } else { Vis::SelfHitTestInvisible };
            w.color = div;
        }
        let rp = prop_bool(cx, "RemovePadding");
        let hb1 = find(cx, "HorizontalBox_1");
        wd(cx, hb1).pad = if rp { Margin::default() } else { Margin { l: 30.0, t: 0.0, r: 20.0, b: 0.0 } };
        let hb = find(cx, "HoverBacking");
        wd(cx, hb).vis = if rp { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
        let only = prop_bool(cx, "DropdownOnly");
        wd(cx, l).vis = if only || label.is_empty() { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
        let ws = find(cx, "WrapTextSwitcher");
        wd(cx, ws).active = i32::from(prop_bool(cx, "SetAutoWrap"));
        let main = Self::main_interactor(cx);
        sound_overrides(cx, main);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        cx.view.manager.on_interactor_focused.add(Handler { owner: cx.w, id: D_INTERACTOR_FOCUSED });
        // @6340-6669
        let tip = text(Some(&prop(cx, "TopTooltipText")));
        if !tip.is_empty() {
            let main = Self::main_interactor(cx);
            set_tooltip(cx, main, tooltip_params(&tip, true, 550.0, -25.0, 12));
        }
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        cx.view.manager.on_interactor_focused.remove(Handler { owner: cx.w, id: D_INTERACTOR_FOCUSED });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            D_BAR_CLICK => {
                if let Some((i, s)) = arg.downcast_ref::<(i32, String)>() {
                    self.clicked(cx, *i, s);
                }
            }
            D_MAIN_CLICK => {
                self.toggle_open(cx, true);
                if !Self::use_context_menu(cx) {
                    let bar = Self::bar(cx);
                    cx.visit::<SectionBar, _>(bar, |b, cx| b.focus_selected(cx));
                }
            }
            D_FOCUS => Self::visuals(cx, true),
            D_UNFOCUS => {
                if !self.open {
                    Self::visuals(cx, false);
                }
            }
            D_INTERACTABLE => Self::set_colors(cx, CHECK_BOX, QUAD_FILL, WHITE),
            D_NOT_INTERACTABLE => {
                Self::set_colors(cx, DISABLED, DISABLED, DISABLED);
                let f = find(cx, "QuadFill");
                wd(cx, f).opacity = 0.4;
            }
            // the focus went to something that is not the row's own
            D_INTERACTOR_FOCUSED => {
                if let Some(&i) = arg.downcast_ref::<WidgetId>()
                    && !Self::owns(cx, i)
                {
                    self.toggle_open(cx, false);
                }
            }
            D_CTX_CLICK => {
                let Some(&i) = arg.downcast_ref::<WidgetId>() else { return };
                let sid = cx.view.manager.interactor(i).map(|s| s.id.clone()).unwrap_or_default();
                let idx = index_of_id(&self.items, &sid);
                self.clicked(cx, idx, &sid);
            }
            D_CTX_DISMISSED => {
                self.open = false;
                let idle = find(cx, "IdleContainer");
                wd(cx, idle).opacity = 1.0;
                let main = Self::main_interactor(cx);
                set_focused(cx, main, false);
                for n in ["WrapSelectionLabel", "AutoWrapSelectionLabel", "arrow"] {
                    let w = find(cx, n);
                    wd(cx, w).color = WHITE;
                }
                let arrow = find(cx, "arrow");
                wd(cx, arrow).transform.scale[1] = 1.0;
                let id = self.current_selected_id.clone();
                cx.emit(self, |c| &c.on_closed, &id);
            }
            _ => {}
        }
    }

    /// hoverButton OnHovered @7660 -> @5918 / OnUnhovered @7665 -> @6185: the label's part of OnFocus / OnUnfocus,
    /// entered past the RemovePadding and Open tests.
    fn on_button_hover(&mut self, cx: &mut Cx<'_>, name: &str, hovered: bool) {
        if name != "hoverButton" {
            return;
        }
        let l = find(cx, "Label");
        let w = wd(cx, l);
        w.opacity = if hovered { 1.0 } else { 0.65 };
        w.font_size = if hovered { 20.0 } else { 18.0 };
        let hb = find(cx, "HoverBacking");
        let c = if hovered { Self::hover_color(cx) } else { CLEAR };
        wd(cx, hb).color = c;
    }
}
