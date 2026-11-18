//! UI_SectionBar: the vertical bar and the horizontal one (the
//! Glossary's tabs). A bar whose button class is not ported stays empty.

use std::any::Any;

use serde_json::Value;

use super::common::text;
use super::controls::bar_buttons::P_ID;
use super::controls::interactor::batch_set_context;
use super::controls::{find, prop, prop_bool, prop_f64, wd};
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::manager::Manager;
use crate::menu::registry;
use crate::menu::view::MenuView;
use crate::slate::{HAlign, Margin, VAlign, Vis, WidgetId};

/// A tree property set by an owner while the bar's own call runs (`bar.suppressFocus = true` from OnClick's
/// handler): the owner cannot reach the busy controller, so the flag lives in the tree.
const SUPPRESS: &str = "port:suppressFocus";

/// `bar.suppressFocus = true`: OnClick's handler asks the bar to leave the selection and the focus alone.
pub fn suppress_focus(cx: &mut Cx<'_>, bar: WidgetId) {
    cx.view.tree_mut().set_prop(bar, SUPPRESS, Value::Bool(true));
}

/// `bar.props['Button Params'] = items` (a caller's write before `initialize`).
pub fn set_button_params(cx: &mut Cx<'_>, bar: WidgetId, items: Value) {
    cx.view.tree_mut().set_prop(bar, "Button Params", items);
}

fn class_name(c: Option<&Value>) -> Option<String> {
    let s = c?.as_str().filter(|s| !s.is_empty() && *s != "None")?;
    Some(s.strip_suffix("_C").unwrap_or(s).to_string())
}

const H_BUTTON_CLICKED: u32 = 1;
const H_SWITCH_PAGE: u32 = 2;
const R_PAGING: u32 = 1;

#[derive(Debug, Default)]
pub struct SectionBar {
    /// (index, id).
    pub on_click: Event<(i32, String)>,
    pub on_populate: Event<()>,
    pub interactors: Vec<WidgetId>,
    pub buttons: Vec<WidgetId>,
    params: Vec<Value>,
    paging_wanted: bool,
    paging_bound: bool,
    /// The bar's buttons that are on, by index (the buttons' own is_on is theirs).
    on: Vec<bool>,
}

impl SectionBar {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(SectionBar::default())
    }

    fn bind_paging(&mut self, cx: &mut Cx<'_>) {
        if !self.paging_wanted || self.paging_bound {
            return;
        }
        self.paging_bound = true;
        cx.view.manager.on_request_switch_page.add(Handler { owner: cx.w, id: H_SWITCH_PAGE });
    }

    /// PagingVisible: Paging and not the mobile feature level.
    fn paging_visible(cx: &Cx<'_>) -> bool {
        prop_bool(cx, "Paging")
    }

    /// OnSwitchPage(Direction, Player Index) @6911, bound to the manager's OnRequestSwitchPage (UIPrevPage /
    /// UINextPage: Q and E): only for a horizontal bar that is visible, for its player, while its first
    /// interactor's context is the current one. The target wraps (UI_Functions.WrapInt) and is clicked with
    /// DoNotFocus when its interactor is enabled: no click sound, the focus stays where it is.
    fn on_switch_page(&mut self, cx: &mut Cx<'_>, player: i32, direction: i32) {
        if !cx.view.visible_in_hierarchy(cx.w) {
            return;
        }
        if player != prop_f64(cx, "Player Index", 0.0) as i32 {
            return;
        }
        let Some(&first) = self.interactors.first() else { return };
        if !Manager::is_in_current_context(cx, first, false, -1) {
            return;
        }
        if prop_bool(cx, "Vertical") {
            return;
        }
        let last = self.buttons.len() as i32 - 1;
        let mut to = self.current_index() as i32 + direction;
        if to < 0 {
            to = last;
        }
        if to > last {
            to = 0;
        }
        let Some(&target) = usize::try_from(to).ok().and_then(|t| self.interactors.get(t)) else { return };
        if !cx.view.manager.interactor(target).is_some_and(|s| s.is_enabled()) {
            return;
        }
        let id = self.params.get(to as usize).and_then(|p| p.get(P_ID)).and_then(Value::as_str).unwrap_or_default();
        let id = id.to_string();
        self.button_clicked(cx, &id, true);
    }

    pub fn initialize(&mut self, cx: &mut Cx<'_>) {
        let p = prop(cx, "Button Params");
        self.populate(cx, p.as_array().cloned());
    }

    pub fn populate(&mut self, cx: &mut Cx<'_>, params: Option<Vec<Value>>) {
        self.params = params.unwrap_or_default();
        if self.params.is_empty() {
            let me = cx.w;
            wd(cx, me).vis = Vis::Collapsed;
            return;
        }
        for b in std::mem::take(&mut self.buttons) {
            MenuView::discard(cx, b);
        }
        self.interactors.clear();
        let vertical = prop_bool(cx, "Vertical");
        let bx = find(cx, if vertical { "VerticalButtons" } else { "HorizontalButtons" });
        let override_cls = class_name(Some(&prop(cx, "OverrideClass")));
        let overridden: Vec<i64> = prop(cx, "IndexesToOverride")
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_i64).collect())
            .unwrap_or_default();
        // @2335: Vertical picks 'Vertical Button' or 'Horizontal Button'
        let base = class_name(Some(&prop(cx, if vertical { "Vertical Button" } else { "Horizontal Button" })));
        let paging = Self::paging_visible(cx);
        let spacing = prop_f64(cx, "Spacing", 0.0);
        if !vertical {
            // @4125-4656: the page labels stand 20 - Spacing from the buttons
            let (prev, next) = (find(cx, "inputLabelPrev"), find(cx, "inputLabelNext"));
            wd(cx, prev).pad = Margin { l: 0.0, t: 0.0, r: 20.0 - spacing, b: 0.0 };
            wd(cx, next).pad = Margin { l: 20.0 - spacing, t: 0.0, r: 0.0, b: 0.0 };
        }
        let halign = prop(cx, "Horizontal Alignment");
        let halign = match halign.as_str().and_then(|s| s.rsplit("::").next()) {
            Some("HAlign_Left") => HAlign::Left,
            Some("HAlign_Center") => HAlign::Center,
            Some("HAlign_Right") => HAlign::Right,
            _ => HAlign::Fill,
        };
        let size_fill = prop(cx, "Size Rule").as_str().is_some_and(|s| s.ends_with("Fill"));
        let me = cx.w;
        let n = self.params.len();
        for (i, bp) in self.params.clone().iter().enumerate() {
            let cls = match &override_cls {
                Some(o) if overridden.contains(&(i as i64)) => Some(o.clone()),
                _ => base.clone(),
            };
            let Some(cls) = cls.filter(|c| registry::make(c).is_some()) else { continue };
            // (a ported class whose tree the bundle lacks: no button, and a warning)
            if !cx.view.bundle.widgets.contains_key(&cls) {
                eprintln!("menu: UI_SectionBar button class {cls} is not in the bundle");
                continue;
            }
            let w = MenuView::create(cx, &cls);
            let iw = cx.view.tree().find_in(w, "UI_Interactor");
            let id = bp.get(P_ID).and_then(Value::as_str).unwrap_or_default().to_string();
            if let Some(s) = cx.view.manager.interactor_mut(iw) {
                s.section_bar_id = id;
                s.on_click_with_source.add(Handler { owner: me, id: H_BUTTON_CLICKED });
            }
            cx.visit_dyn(w, |c, cx| {
                if let Some(b) = c.as_bar_button() {
                    b.init_bar_button(cx, bp);
                }
            });
            let last_one = i == n - 1;
            let x = wd(cx, w);
            x.h_align = halign;
            x.v_align = VAlign::Fill;
            if vertical {
                x.fill = 0.0;
                x.pad = Margin { l: 0.0, t: 0.0, r: 0.0, b: if last_one { 0.0 } else { spacing } };
            } else {
                // @3614-4026 SetSlotLayoutDetails(slot, Horizontal Alignment, VAlign Fill, 1.0, Size Rule, left,
                // 0, right, 0): left = Spacing for the first button when the page labels show, right = Spacing
                // for every button but the last, and for the last too when they show
                x.fill = if size_fill { 1.0 } else { 0.0 };
                x.pad = Margin {
                    l: if i == 0 && paging { spacing } else { 0.0 },
                    t: 0.0,
                    r: if !last_one || paging { spacing } else { 0.0 },
                    b: 0.0,
                };
            }
            MenuView::add_child(cx, bx, w, None);
            self.interactors.push(iw);
            self.buttons.push(w);
        }
        self.on = vec![false; self.buttons.len()];
        for b in self.buttons.clone() {
            cx.visit_dyn(b, |c, cx| {
                if let Some(b) = c.as_bar_button() {
                    b.set_on(cx, false);
                }
            });
        }
        cx.emit(self, |c| &c.on_populate, &());
    }

    /// EqualEqual_StrStr: case-sensitive.
    pub fn index_of_id(&self, id: &str) -> i32 {
        index_of_id(&self.params, id)
    }

    pub fn current_index(&self) -> usize {
        self.on.iter().position(|&b| b).unwrap_or(0)
    }

    pub fn current_selected_id(&self) -> String {
        self.params
            .get(self.current_index())
            .and_then(|p| p.get(P_ID))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .into()
    }

    pub fn set_current_index(&mut self, cx: &mut Cx<'_>, index: i32) {
        for (i, b) in self.buttons.clone().into_iter().enumerate() {
            let on = i as i32 == index;
            self.on[i] = on;
            cx.visit_dyn(b, |c, cx| {
                if let Some(b) = c.as_bar_button() {
                    b.set_on(cx, on);
                }
            });
        }
    }

    pub fn set_current_index_by_id(&mut self, cx: &mut Cx<'_>, id: &str) {
        let i = self.index_of_id(id);
        if i >= 0 {
            self.set_current_index(cx, i);
        }
    }

    pub fn button_by_id(&self, id: &str) -> Option<WidgetId> {
        usize::try_from(self.index_of_id(id)).ok().and_then(|i| self.buttons.get(i)).copied()
    }

    pub fn params_for_id(&self, id: &str) -> Option<&Value> {
        usize::try_from(self.index_of_id(id)).ok().and_then(|i| self.params.get(i))
    }

    /// OnClick's handler may call `suppress_focus`: the bar then leaves the selection and the focus alone.
    pub fn button_clicked(&mut self, cx: &mut Cx<'_>, id: &str, do_not_focus: bool) {
        let me = cx.w;
        cx.view.tree_mut().set_prop(me, SUPPRESS, Value::Bool(false));
        // step 2: a button in tooltip-only mode takes no click
        if let Some(b) = self.button_by_id(id) {
            let only = cx.visit_dyn(b, |c, _| c.as_bar_button().is_some_and(|b| b.tooltip_only_mode()));
            if only == Some(true) {
                return;
            }
        }
        let index = self.index_of_id(id);
        if self.current_index() as i32 != index || !prop_bool(cx, "Suppress Same Tab Clicks") {
            cx.emit(self, |c| &c.on_click, &(index, id.to_string()));
        }
        if prop_bool(cx, SUPPRESS) {
            return;
        }
        self.set_current_index(cx, index);
        if !do_not_focus {
            self.focus_selected(cx);
        }
    }

    pub fn focus_selected(&self, cx: &mut Cx<'_>) {
        if let Some(&i) = self.interactors.get(self.current_index()) {
            super::controls::interactor::set_focused(cx, i, false);
        }
    }

    /// RemoveOption(ID) @534: the ID's params, interactor and button leave the bar's arrays (by the params' index)
    /// and the button is destroyed (UI_Functions.DestroyWidget: RemoveFromParent).
    pub fn remove_option(&mut self, cx: &mut Cx<'_>, id: &str) {
        let Ok(i) = usize::try_from(self.index_of_id(id)) else { return };
        self.params.remove(i);
        if i < self.interactors.len() {
            self.interactors.remove(i);
        }
        if i < self.buttons.len() {
            MenuView::discard(cx, self.buttons.remove(i));
            self.on.remove(i);
        }
    }

    /// (the port's own) an option of the port's after the option `after` (at the end when there is none): the bar
    /// is populated again with it.
    pub fn add_option(&mut self, cx: &mut Cx<'_>, bp: Value, after: &str) {
        let i = self.index_of_id(after);
        let mut params = self.params.clone();
        let at = usize::try_from(i).map_or(params.len(), |i| i + 1);
        params.insert(at, bp);
        self.populate(cx, Some(params));
    }

    pub fn set_options_collapsed(&self, cx: &mut Cx<'_>, ids: &[&str], collapsed: bool) {
        for id in ids {
            if let Some(b) = self.button_by_id(id) {
                wd(cx, b).vis = if collapsed { Vis::Collapsed } else { Vis::Visible };
            }
        }
    }

    /// Update Context(guid, Player Index) (UI_SectionBar @8248): the buttons' context, and their player when a
    /// caller gives one.
    pub fn update_context(&self, cx: &mut Cx<'_>, ctx: &str, player: Option<i32>) {
        batch_set_context(cx, &self.interactors, ctx);
        if let Some(p) = player {
            for &i in &self.interactors {
                if let Some(s) = cx.view.manager.interactor_mut(i) {
                    s.player_index = p;
                }
            }
        }
    }
}

/// The index of the params whose ID is `id` (-1: none).
pub fn index_of_id(params: &[Value], id: &str) -> i32 {
    params.iter().position(|p| p.get(P_ID).and_then(Value::as_str) == Some(id)).map_or(-1, |i| i as i32)
}

impl Controller for SectionBar {
    /// OnInitialized @6822: Populate, then Delay(0) @6856 -> @349: with Paging, OnSwitchPage is bound to the
    /// manager's OnRequestSwitchPage (@94-252; the retry while there is no manager does not apply). A vertical bar
    /// binds too (Paging is true in the class defaults) and its handler returns at @7127.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.initialize(cx);
        cx.view.delay(0.0, Handler { owner: cx.w, id: R_PAGING });
    }

    /// Port: the build unbinds at Destruct only with Has Initialized (@8029), which no statement of the graph
    /// sets; a destroyed widget's delegate is dropped by the engine. The port takes the handler out here and puts
    /// it back when the widget is constructed again.
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        if !self.paging_bound {
            return;
        }
        self.paging_bound = false;
        cx.view.manager.on_request_switch_page.remove(Handler { owner: cx.w, id: H_SWITCH_PAGE });
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let vertical = prop_bool(cx, "Vertical");
        let (h, v) = (find(cx, "HorizontalContainer"), find(cx, "VerticalContainer"));
        wd(cx, h).vis = if vertical { Vis::Collapsed } else { Vis::Visible };
        wd(cx, v).vis = if vertical { Vis::Visible } else { Vis::Collapsed };
        let paging = if Self::paging_visible(cx) { Vis::HitTestInvisible } else { Vis::Collapsed };
        for n in ["inputLabelPrev", "inputLabelNext"] {
            let w = find(cx, n);
            wd(cx, w).vis = paging;
        }
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        if prop_bool(cx, "Init Own Context") {
            let g = cx.view.manager.new_guid();
            self.update_context(cx, &g, None);
        }
        self.bind_paging(cx);
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == R_PAGING && prop_bool(cx, "Paging") {
            self.paging_wanted = true;
            self.bind_paging(cx);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            H_BUTTON_CLICKED => {
                let Some(&i) = arg.downcast_ref::<WidgetId>() else { return };
                let sid = cx.view.manager.interactor(i).map(|s| s.section_bar_id.clone()).unwrap_or_default();
                if !sid.is_empty() {
                    self.button_clicked(cx, &sid, false);
                }
            }
            H_SWITCH_PAGE => {
                if let Some(&(p, d)) = arg.downcast_ref::<(i32, i32)>() {
                    self.on_switch_page(cx, p, d);
                }
            }
            _ => {}
        }
    }
}

/// The text of a params member (`text(params[key])`).
pub fn param_text(p: &Value, key: &str) -> String {
    text(p.get(key))
}
