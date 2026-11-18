//! The Shop: UI_WWShop, UI_SectionBar_Shop, WBP_MO_HorizontalButton_Shop, the two page
//! classes and UI_WWGetMoreButton. A page's products come from the port's ShopManager stand-in (shop_products.rs) in
//! place of Hydra's answer; the cards, grids and filter are in shop_cells.rs, the details modals in shop_details.rs.
//! Nothing can be bought. The get-more button opens the Gleamium shop (currency_shop.rs).

use std::any::Any;

use serde_json::{Map, Value, json};

use super::common::{ModalBase, text};
use super::controls::bar_buttons::{BarButton, P_ICON, P_ID, P_LABEL};
use super::controls::interactor::{FocusedCommon, InteractorState, batch_set_context, set_focused};
use super::controls::{bind, find, ist, prop_bool, prop_f64, quad_color, quad_wobble, wd};
use super::currency_shop::CurrencyShop;
use super::nav::GlobalSlideOut;
use super::section_bar::index_of_id;
use super::shop_cells::{FilterGrid, ProductGrid, catalogue};
use super::shop_products::{ShopProduct, fetch_all_products, fetch_products_with_any_tags, split_featured};
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::manager::{Manager, Modal, NULL_GUID};
use crate::menu::scroll::{scroll_list, scroll_to_start};
use crate::menu::view::MenuView;
use crate::slate::{HAlign, Margin, VAlign, Vis, WidgetId, resolve};

fn class_name(c: Option<&Value>) -> Option<String> {
    let s = c?.as_str().filter(|s| !s.is_empty() && *s != "None")?;
    Some(s.strip_suffix("_C").unwrap_or(s).to_string())
}

fn set_vis(cx: &mut Cx<'_>, name: &str, v: Vis) {
    let w = find(cx, name);
    wd(cx, w).vis = v;
}

fn interactor_of(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
    cx.view.tree().find_in(w, "UI_Interactor")
}

// ---------- WBP_MO_HorizontalButton_Shop (a shop tab) ----------

const TAB_LIGHT: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
const TAB_WHITE: Rgba = [1.0, 1.0, 1.0, 1.0];
const TAB_DARK: Rgba = [0.009134, 0.01033, 0.029557, 1.0];

const H_TAB_FOCUS: u32 = 1;
const H_TAB_UNFOCUS: u32 = 2;

/// WBP_MO_HorizontalButton_Shop. Not ported: UpdateQuadShape (@1891, the end tabs' OffIndicator corners), which no graph
/// of the bar calls.
#[derive(Debug, Default)]
pub struct MOHorizontalButtonShop {
    is_on: bool,
    is_focus: bool,
}

impl MOHorizontalButtonShop {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(MOHorizontalButtonShop::default())
    }

    /// Update Visual @3468: on: dark text and icon, the yellow fill, the wobbling outline while focused; off: dark on
    /// the white focus fill, else white.
    fn update_visual(&mut self, cx: &mut Cx<'_>) {
        if self.is_on {
            Self::set_text_and_icon_colour(cx, false);
            Self::set_background_visibility(cx, false, true, self.is_focus);
        } else {
            Self::set_text_and_icon_colour(cx, !self.is_focus);
            Self::set_background_visibility(cx, self.is_focus, false, false);
        }
    }

    /// Set Text And Icon Colour @2603: ColorAndOpacity of TextLabel and IconImage.
    fn set_text_and_icon_colour(cx: &mut Cx<'_>, white: bool) {
        let c = if white { TAB_WHITE } else { TAB_DARK };
        let (t, i) = (find(cx, "TextLabel"), find(cx, "IconImage"));
        wd(cx, t).color = c;
        wd(cx, i).color = c;
    }

    /// Set Background Visibility @3204 (Conditional Visibility: SelfHitTestInvisible or Hidden).
    fn set_background_visibility(cx: &mut Cx<'_>, white: bool, yellow: bool, wobble: bool) {
        let v = |b: bool| if b { Vis::SelfHitTestInvisible } else { Vis::Hidden };
        set_vis(cx, "FocusIndicator", v(white));
        set_vis(cx, "OnIndicator", v(yellow));
        set_vis(cx, "OnAndFocusIndicator", v(wobble));
    }

    /// EnableVerticalDivider: SelfHitTestInvisible or Collapsed.
    pub fn enable_vertical_divider(&mut self, cx: &mut Cx<'_>, on: bool) {
        set_vis(cx, "VerticalDivider", if on { Vis::SelfHitTestInvisible } else { Vis::Collapsed });
    }
}

impl BarButton for MOHorizontalButtonShop {
    /// InitBarButton @2336: the upper-case label, the icon when valid, then Set On(isOn).
    fn init_bar_button(&mut self, cx: &mut Cx<'_>, params: &Value) {
        let t = find(cx, "TextLabel");
        wd(cx, t).text = text(params.get(P_LABEL)).to_uppercase();
        if let Some(icon) = params.get(P_ICON).and_then(Value::as_str).filter(|s| !s.is_empty()) {
            let tex = resolve(&cx.view.bundle, icon);
            let i = find(cx, "IconImage");
            wd(cx, i).tex = Some(tex);
        }
        let on = self.is_on;
        self.set_on(cx, on);
    }

    /// Set On @2288: Update Text Visibility @3085 (the label shows on the selected tab only), Update Visual.
    fn set_on(&mut self, cx: &mut Cx<'_>, v: bool) {
        self.is_on = v;
        let shown = if v { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        set_vis(cx, "TextLabel", shown);
        set_vis(cx, "TextSizeBox", shown);
        self.update_visual(cx);
    }

    fn is_on(&self) -> bool {
        self.is_on
    }
}

impl Controller for MOHorizontalButtonShop {
    /// The UI_Interactor's bound OnFocus @2262 / OnUnfocus @2587.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = interactor_of(cx, cx.w);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, H_TAB_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, H_TAB_UNFOCUS);
    }

    /// PreConstruct @413. @855 IconImage.SetBrushTintColor(TAB_LIGHT): the brush's tint, which the slate multiplies into
    /// the draw with ColorAndOpacity (not multiplied into the one colour its Image had).
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let q = |cx: &Cx<'_>, n: &str| find(cx, n);
        let (border, off, on) = (q(cx, "QuadBorder"), q(cx, "OffIndicator"), q(cx, "OnIndicator"));
        quad_color(cx, border, [0.262251, 0.708376, 0.715693, 1.0]);
        quad_color(cx, off, [0.020289, 0.028426, 0.072272, 1.0]);
        quad_color(cx, on, [1.0, 0.527115, 0.0, 1.0]);
        let (vd, icon, fi, ofi) =
            (q(cx, "VerticalDivider"), q(cx, "IconImage"), q(cx, "FocusIndicator"), q(cx, "OnAndFocusIndicator"));
        wd(cx, vd).color = TAB_LIGHT;
        let x = wd(cx, icon);
        x.color = TAB_LIGHT;
        x.brush_tint = TAB_LIGHT;
        wd(cx, fi).color = TAB_LIGHT;
        wd(cx, ofi).color = TAB_LIGHT;
        for n in ["Overlay_Outerpadd", "Overlay_Padd1", "Overlay_Padd2"] {
            let w = find(cx, n);
            wd(cx, w).pad = Margin { l: 0.0, t: 0.0, r: 0.0, b: 0.0 };
        }
        self.set_on(cx, false);
        set_vis(cx, "QuadBorder", Vis::Collapsed);
        set_vis(cx, "OffIndicator", Vis::Hidden);
        quad_wobble(cx, ofi, true);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            H_TAB_FOCUS => {
                self.is_focus = true;
                self.update_visual(cx);
            }
            H_TAB_UNFOCUS => {
                self.is_focus = false;
                self.update_visual(cx);
            }
            _ => {}
        }
    }

    fn as_bar_button(&mut self) -> Option<&mut dyn BarButton> {
        Some(self)
    }
}

// ---------- UI_SectionBar_Shop (parent UserWidget: not UI_SectionBar) ----------

const B_BUTTON_CLICKED: u32 = 1;
const B_SWITCH_PAGE: u32 = 2;
const BR_PAGING: u32 = 1;

/// UI_SectionBar_Shop.
#[derive(Debug, Default)]
pub struct SectionBarShop {
    /// (index, id).
    pub on_click: Event<(i32, String)>,
    pub on_populate: Event<()>,
    pub on_tab_change: Event<()>,
    pub interactors: Vec<WidgetId>,
    pub buttons: Vec<WidgetId>,
    pub active_tab_interactor: Option<WidgetId>,
    pub active_tab_button: Option<WidgetId>,
    params: Vec<Value>,
    on: Vec<bool>,
    paging_bound: bool,
}

impl SectionBarShop {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(SectionBarShop::default())
    }

    fn grid(cx: &Cx<'_>) -> WidgetId {
        find(cx, "HorizontalGridButtons")
    }

    /// Populate @8024 -> @3678: one Horizontal Button per entry, AddChildToGrid(button, 0, i) with a ColumnFill of 1.0
    /// each; then Update Vertical Dividers and OnPopulate. (The bar's own SetAutoSize as a canvas slot does nothing
    /// here: it sits in a GridPanel.) The vertical form is not used by the shop.
    pub fn populate(&mut self, cx: &mut Cx<'_>, params: Vec<Value>) {
        for b in std::mem::take(&mut self.buttons) {
            MenuView::discard(cx, b);
        }
        let grid = Self::grid(cx);
        wd(cx, grid).col_fill = Vec::new();
        self.interactors.clear();
        self.params = params;
        let cls = class_name(cx.view.tree().props(cx.w).get("Horizontal Button"));
        let me = cx.w;
        if let Some(cls) = cls {
            for (i, bp) in self.params.clone().iter().enumerate() {
                let w = MenuView::create(cx, &cls);
                let iw = interactor_of(cx, w);
                let id = bp.get(P_ID).and_then(Value::as_str).unwrap_or_default().to_string();
                if let Some(s) = cx.view.manager.interactor_mut(iw) {
                    s.section_bar_id = id;
                    s.on_click_with_source.add(Handler { owner: me, id: B_BUTTON_CLICKED });
                }
                cx.visit_dyn(w, |c, cx| {
                    if let Some(b) = c.as_bar_button() {
                        b.init_bar_button(cx, bp);
                    }
                });
                self.buttons.push(w);
                self.interactors.push(iw);
                wd(cx, grid).col_fill.push(1.0);
                // AddChildToGrid: a GridSlot's defaults (Fill, Fill, no padding)
                let x = wd(cx, w);
                x.h_align = HAlign::Fill;
                x.v_align = VAlign::Fill;
                x.row = 0;
                x.col = i as i32;
                MenuView::add_child(cx, grid, w, None);
            }
        }
        self.on = vec![false; self.buttons.len()];
        self.update_vertical_dividers(cx);
        cx.emit(self, |c| &c.on_populate, &());
    }

    fn set_button_on(&mut self, cx: &mut Cx<'_>, i: usize, v: bool) {
        self.on[i] = v;
        cx.visit_dyn(self.buttons[i], |c, cx| {
            if let Some(b) = c.as_bar_button() {
                b.set_on(cx, v);
            }
        });
    }

    /// SetAllOff @8647.
    fn set_all_off(&mut self, cx: &mut Cx<'_>) {
        for i in 0..self.buttons.len() {
            self.set_button_on(cx, i, false);
        }
    }

    /// SetCurrentIndex @7391: UI_Functions.SetTabOn (this one on, the others off), Set Button Fill Coefficient, then the
    /// tab's interactor is focused (SetFocused(false)) and OnTabChange. An index with no button (PreConstruct's
    /// SetCurrentIndex(0) before a Populate) stops at the failed cast.
    pub fn set_current_index(&mut self, cx: &mut Cx<'_>, index: usize) {
        if index >= self.buttons.len() {
            return;
        }
        for i in 0..self.buttons.len() {
            self.set_button_on(cx, i, i == index);
        }
        self.set_button_fill_coefficient(cx, index);
        let iw = self.interactors[index];
        self.active_tab_interactor = Some(iw);
        set_focused(cx, iw, false);
        self.active_tab_button = Some(self.buttons[index]);
        cx.emit(self, |c| &c.on_tab_change, &());
    }

    /// Set Button Fill Coefficient @646: every column of the grid 1.0, the selected one 2.0 (SetColumnFill).
    fn set_button_fill_coefficient(&mut self, cx: &mut Cx<'_>, index: usize) {
        let grid = Self::grid(cx);
        let fill = &mut wd(cx, grid).col_fill;
        for f in fill.iter_mut() {
            *f = 1.0;
        }
        while fill.len() <= index {
            fill.push(0.0);
        }
        fill[index] = 2.0;
    }

    /// SetCurrentIndexByID @8237: ParamsForID, else SetCurrentIndex(0).
    pub fn set_current_index_by_id(&mut self, cx: &mut Cx<'_>, id: &str) {
        let i = index_of_id(&self.params, id);
        self.set_current_index(cx, usize::try_from(i).unwrap_or(0));
    }

    /// GetCurrentIndex: the first button that is on, else 0.
    pub fn current_index(&self) -> usize {
        self.on.iter().position(|&b| b).unwrap_or(0)
    }

    /// Update Vertical Dividers @2456: a divider after every tab but the last, none beside the selected tab (its own and
    /// the one before it).
    fn update_vertical_dividers(&mut self, cx: &mut Cx<'_>) {
        let last = self.buttons.len().wrapping_sub(1);
        let divider = |cx: &mut Cx<'_>, w: WidgetId, on: bool| {
            cx.visit::<MOHorizontalButtonShop, _>(w, |b, cx| b.enable_vertical_divider(cx, on));
        };
        for (i, &b) in self.buttons.clone().iter().enumerate() {
            divider(cx, b, i != last);
        }
        let cur = self.current_index();
        if let Some(&b) = self.buttons.get(cur) {
            divider(cx, b, false);
        }
        if let Some(&b) = self.buttons.get(cur.saturating_sub(1)) {
            divider(cx, b, false);
        }
    }

    /// ButtonClicked(Interactor, ID, Do Not Focus) @5935. GetTooltipOnlyMode is false on every shop tab.
    fn button_clicked(&mut self, cx: &mut Cx<'_>, iw: WidgetId, id: &str, do_not_focus: bool) {
        let Some(index) = self.interactors.iter().position(|&i| i == iw) else { return };
        if self.current_index() != index || !prop_bool(cx, "Suppress Same Tab Clicks") {
            cx.emit(self, |c| &c.on_click, &(index as i32, id.to_string()));
        }
        self.set_current_index(cx, index);
        self.update_vertical_dividers(cx);
        if do_not_focus {
            return;
        }
        // FocusSelected: an empty graph
        self.active_tab_interactor = Some(iw);
        cx.emit(self, |c| &c.on_tab_change, &());
    }

    /// OnSwitchPage(Player Index, Direction) @6735 (UIPrevPage / UINextPage: Q and E): a horizontal bar that is visible,
    /// for its player, while its first interactor's context is the current one; the target wraps (WrapInt) and is
    /// clicked with Do Not Focus when its interactor is enabled.
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
        self.button_clicked(cx, target, &id, true);
    }

    /// UpdateContext(guid, Player Index) @8531: BatchSetContext, BatchSetPlayerIndex.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str, player: i32) {
        batch_set_context(cx, &self.interactors, ctx);
        for &i in &self.interactors {
            if let Some(s) = ist(cx, i) {
                s.player_index = player;
            }
        }
    }
}

impl Controller for SectionBarShop {
    /// OnInitialized @6680: Delay(0) -> @270: with Paging, @15 binds OnSwitchPage to the UIManager's
    /// OnRequestSwitchPage.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        cx.view.delay(0.0, Handler { owner: cx.w, id: BR_PAGING });
    }

    /// Destruct @8374: the binding goes, Interactors is cleared.
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        if self.paging_bound {
            cx.view.manager.on_request_switch_page.remove(Handler { owner: cx.w, id: B_SWITCH_PAGE });
        }
        self.paging_bound = false;
    }

    /// PreConstruct @5342 (Populate there is for the designer only).
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let hc = find(cx, "HorizontalContainer");
        wd(cx, hc).vis = if prop_bool(cx, "Vertical") { Vis::Collapsed } else { Vis::Visible };
        let paging = if prop_bool(cx, "Paging") { Vis::HitTestInvisible } else { Vis::Collapsed };
        for n in ["inputLabelNext", "inputLabelPrev"] {
            set_vis(cx, n, paging);
        }
        self.set_all_off(cx);
        self.set_current_index(cx, 0);
        for n in ["ScrollBox_181", "LeftMask", "RightMask"] {
            set_vis(cx, n, Vis::Collapsed);
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == BR_PAGING && prop_bool(cx, "Paging") && !self.paging_bound {
            self.paging_bound = true;
            cx.view.manager.on_request_switch_page.add(Handler { owner: cx.w, id: B_SWITCH_PAGE });
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            // OnButtonClicked @9289
            B_BUTTON_CLICKED => {
                let Some(&i) = arg.downcast_ref::<WidgetId>() else { return };
                let sid = cx.view.manager.interactor(i).map(|s| s.section_bar_id.clone()).unwrap_or_default();
                if !sid.is_empty() {
                    self.button_clicked(cx, i, &sid, false);
                }
            }
            B_SWITCH_PAGE => {
                if let Some(&(p, d)) = arg.downcast_ref::<(i32, i32)>() {
                    self.on_switch_page(cx, p, d);
                }
            }
            _ => {}
        }
    }
}

// ---------- UI_WWShopPage_Home / UI_WWShopPage_Standard ----------

/// SetLoadingStatus(IsLoaded) of a page `w`: LoadedContent Visible or Collapsed, PendingLoad (the spinner and
/// "Loading...") the other way.
fn set_loading_status(cx: &mut Cx<'_>, page: WidgetId, loaded: bool) {
    let (lc, pl) = (cx.view.tree().find_in(page, "LoadedContent"), cx.view.tree().find_in(page, "PendingLoad"));
    wd(cx, lc).vis = if loaded { Vis::Visible } else { Vis::Collapsed };
    wd(cx, pl).vis = if loaded { Vis::Collapsed } else { Vis::Visible };
}

fn grid_of(cx: &Cx<'_>, name: &str) -> WidgetId {
    find(cx, name)
}

/// UI_WWShopPage_Home. Not ported: the TickShopCells timer (no sale or expiry to count down), the grids' and the
/// carousel's Set Navigation (the manager's own spatial navigation moves between cells), GameStoreEnter Analytics.
// Deviation: the carousel's promotions are Hydra's (none in the port): it shows its empty state, NoContent over its
// spinner as Finalize leaves it with no page (@3166-3313 of UI_WWCarousel, Set Loading Status(false) before it).
#[derive(Debug, Default)]
pub struct ShopPageHome;

impl ShopPageHome {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ShopPageHome)
    }

    const GRIDS: [&'static str; 3] = ["Specials_ProductGrid", "NewRelease_ProductGrid", "MostPopular_ProductGrid"];

    /// Set Products @737: FilterFeaturedTabProducts (the stand-in's split), WeeklySpecials to Specials_ProductGrid,
    /// NewReleases to NewRelease_ProductGrid, [] to MostPopular_ProductGrid (Collapsed, @1154-1272), each grid the
    /// context; then CellsCreated @5028 -> SetLoadingStatus(true).
    pub fn set_products(&mut self, cx: &mut Cx<'_>, products: Vec<ShopProduct>, tab: &str, ctx: &str) {
        let (specials, new) = split_featured(&products);
        for (n, list) in Self::GRIDS.iter().zip([specials, new, Vec::new()]) {
            let g = grid_of(cx, n);
            cx.visit::<ProductGrid, _>(g, |grid, cx| {
                grid.set_products(cx, list, tab, true);
                grid.update_context(cx, ctx);
            });
        }
        let mp = grid_of(cx, "MostPopular_ProductGrid");
        wd(cx, mp).vis = Vis::Collapsed;
        let car = find(cx, "FeaturedCarosel");
        let nc = cx.view.tree().find_in(car, "NoContent");
        wd(cx, nc).vis = Vis::HitTestInvisible;
        self.set_loading_status(cx, true);
    }

    /// SetLoadingStatus @2766: LoadedContent and PendingLoad, then each grid's.
    pub fn set_loading_status(&mut self, cx: &mut Cx<'_>, loaded: bool) {
        let me = cx.w;
        set_loading_status(cx, me, loaded);
        for n in Self::GRIDS {
            let g = grid_of(cx, n);
            cx.visit::<ProductGrid, _>(g, |grid, cx| grid.set_loading_status(cx, loaded));
        }
    }

    /// Update Context @2513: the grids' (the carousel's and pips' interactors are not made).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        for n in Self::GRIDS {
            let g = grid_of(cx, n);
            cx.visit::<ProductGrid, _>(g, |grid, cx| grid.update_context(cx, ctx));
        }
    }
}

impl Controller for ShopPageHome {
    /// OnInitialized @3550: FeaturedItemsPips Collapsed (@4073).
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        set_vis(cx, "FeaturedItemsPips", Vis::Collapsed);
    }
}

const P_FILTER_CHANGED: u32 = 1;

/// UI_WWShopPage_Standard. SeperateAtCharChange is 0 on every tab, so SetProducts is the one grid's Set Products.
/// Not ported: the ShopTickTimer (TickShopCells), Scroll To Shop Cell, the On Display analytics and the manager's Game Store Filter.
#[derive(Debug, Default)]
pub struct ShopPageStandard {
    tab_id: String,
    filter_enabled: bool,
    loaded_once: bool,
    context: String,
}

impl ShopPageStandard {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<ShopPageStandard>::default()
    }

    fn grid(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_WWShopProductGridGeneric")
    }

    /// Set Tab Info @1825: the tab's ID, the filter grid's SetTabInfo (its change bound) or Collapsed, the grid's Max
    /// and Min Columns 24 with the filter grid, 30 without.
    pub fn set_tab_info(&mut self, cx: &mut Cx<'_>, tab: &TabInfo) {
        self.tab_id = tab.id.to_string();
        self.filter_enabled = tab.filter;
        let fg = find(cx, "UI_WWFilterGrid");
        if tab.filter {
            let me = cx.w;
            cx.visit::<FilterGrid, _>(fg, |f, cx| {
                f.set_tab_info(cx, tab.id, tab.icon, tab.has_character, tab.by_rarity, tab.sort);
                f.on_changed.add(Handler { owner: me, id: P_FILTER_CHANGED });
            });
        } else {
            // @2359-2397: the filter grid Collapsed, GridPanel_0's ColumnFill[0] the never-set Temp_float_Variable (0)
            wd(cx, fg).vis = Vis::Collapsed;
            let gp = find(cx, "GridPanel_0");
            if let Some(f) = wd(cx, gp).col_fill.first_mut() {
                *f = 0.0;
            }
        }
        let cols = if tab.filter { 24 } else { 30 };
        let g = Self::grid(cx);
        cx.visit::<ProductGrid, _>(g, |grid, _| {
            grid.max_cols = cols;
            grid.min_cols = cols;
        });
    }

    /// Set Products @525: through the filter grid's FilterSortAndReturnProducts when it is enabled, then SetProducts
    /// (the grid's Set Products and Update Context), ScrollBox.ScrollToStart; CellsCreated @3065 -> SetLoadingStatus(true).
    pub fn set_products(&mut self, cx: &mut Cx<'_>, products: Vec<ShopProduct>) {
        let list = if self.filter_enabled {
            let fg = find(cx, "UI_WWFilterGrid");
            cx.visit::<FilterGrid, _>(fg, |f, cx| f.filter_sort_and_return(cx, products)).unwrap_or_default()
        } else {
            products
        };
        self.fill(cx, list);
        self.set_loading_status(cx, true);
    }

    fn fill(&mut self, cx: &mut Cx<'_>, list: Vec<ShopProduct>) {
        let g = Self::grid(cx);
        let (tab, ctx) = (self.tab_id.clone(), self.context.clone());
        cx.visit::<ProductGrid, _>(g, |grid, cx| {
            grid.set_products(cx, list, &tab, true);
            grid.update_context(cx, &ctx);
        });
        let sb = find(cx, "ScrollBox");
        scroll_to_start(cx.view.tree_mut(), sb);
    }

    /// SetLoadingStatus @1059: LoadedContent and PendingLoad until a load has succeeded, the grid and PendingLoad_1
    /// after; then the grid's own.
    pub fn set_loading_status(&mut self, cx: &mut Cx<'_>, loaded: bool) {
        let (content, pending) = if self.loaded_once {
            ("UI_WWShopProductGridGeneric", "PendingLoad_1")
        } else {
            ("LoadedContent", "PendingLoad")
        };
        set_vis(cx, content, if loaded { Vis::Visible } else { Vis::Collapsed });
        set_vis(cx, pending, if loaded { Vis::Collapsed } else { Vis::Visible });
        let g = Self::grid(cx);
        cx.visit::<ProductGrid, _>(g, |grid, cx| grid.set_loading_status(cx, loaded));
        if loaded {
            self.loaded_once = true;
        }
    }

    /// Update Context @908: the grid's, the filter grid's when enabled.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let g = Self::grid(cx);
        cx.visit::<ProductGrid, _>(g, |grid, cx| grid.update_context(cx, ctx));
        if self.filter_enabled {
            let fg = find(cx, "UI_WWFilterGrid");
            cx.visit::<FilterGrid, _>(fg, |f, cx| f.update_context(cx, ctx));
        }
    }
}

impl Controller for ShopPageStandard {
    fn on_initialized(&mut self, _cx: &mut Cx<'_>) {
        self.context = NULL_GUID.to_string();
    }

    /// The filter grid's change: the page's products again through its filters.
    // Unverified: the delegate the page binds @2006 was not followed past its binding; the products are filtered again
    // without a new fetch.
    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == P_FILTER_CHANGED {
            let fg = find(cx, "UI_WWFilterGrid");
            let list = cx.visit::<FilterGrid, _>(fg, |f, _| f.current()).unwrap_or_default();
            self.fill(cx, list);
        }
    }
}

// ---------- UI_WWGetMoreButton ----------

const G_FOCUS: u32 = 1;
const G_UNFOCUS: u32 = 2;
const G_CLICK: u32 = 3;
const G_SHOP_CLOSED: u32 = 4;
const GR_CLICKABLE: u32 = 1;

/// UI_WWGetMoreButton.
#[derive(Debug)]
pub struct WWGetMoreButton {
    pub modal_opened: Event<()>,
    pub modal_closed: Event<()>,
    pub context: String,
    clickable: bool,
}

impl Default for WWGetMoreButton {
    fn default() -> WWGetMoreButton {
        WWGetMoreButton {
            modal_opened: Event::default(),
            modal_closed: Event::default(),
            context: NULL_GUID.to_string(),
            clickable: true,
        }
    }
}

impl WWGetMoreButton {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<WWGetMoreButton>::default()
    }

    fn interactor(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_Interactor")
    }

    /// SetOn @941: Background and Border SelfHitTestInvisible either way.
    fn set_on(cx: &mut Cx<'_>) {
        set_vis(cx, "Background", Vis::SelfHitTestInvisible);
        set_vis(cx, "Border", Vis::SelfHitTestInvisible);
    }

    /// Click @534: ShowModal(UI_CurrencyShop, "currencyShop"), its EventClosed bound to GleamiumShopClosed,
    /// SetSlideOutPreviousContext(Context, true), then ModalOpened.
    fn click(&mut self, cx: &mut Cx<'_>) {
        if !self.clickable {
            return;
        }
        self.clickable = false;
        Self::set_on(cx);
        let me = cx.w;
        if let Some(m) = Manager::show_modal(cx, "UI_CurrencyShop", "currencyShop", "", "") {
            let ctx = self.context.clone();
            cx.visit::<CurrencyShop, _>(m, |s, cx| {
                s.event_closed.add(Handler { owner: me, id: G_SHOP_CLOSED });
                s.set_slide_out_previous_context(cx, &ctx, true);
            });
        }
        cx.emit(self, |c| &c.modal_opened, &());
    }

    /// UpdateContext @1199.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.context = ctx.to_string();
        let i = Self::interactor(cx);
        batch_set_context(cx, &[i], ctx);
    }
}

impl Controller for WWGetMoreButton {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.clickable = cx.view.tree().props(cx.w).get("Clickable").and_then(Value::as_bool).unwrap_or(true);
        // the UI_Interactor's bound OnFocus @502, OnUnfocus @518, OnClick @534
        let i = Self::interactor(cx);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, G_FOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, G_UNFOCUS);
        bind(cx, i, |s: &mut InteractorState| &mut s.on_click, G_CLICK);
    }

    /// Construct @1310: FocusedCommon.SetInteractor, ToggleUpdateCurrency(true) -> UpdateCurrency @1371:
    /// Conv_IntToText(PremiumCurrency.GetCount()) through a None inventory: "0".
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let i = Self::interactor(cx);
        let fc = find(cx, "UI_FocusedCommon");
        cx.visit::<FocusedCommon, _>(fc, |f, cx| f.set_interactor(cx, i));
        let t = find(cx, "ButtonText");
        wd(cx, t).text = "0".to_string();
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            G_FOCUS | G_UNFOCUS => Self::set_on(cx),
            G_CLICK => self.click(cx),
            // GleamiumShopClosed @1664: ModalClosed, then Delay(1.0) @15 -> Clickable
            G_SHOP_CLOSED => {
                cx.emit(self, |c| &c.modal_closed, &());
                cx.view.delay(1.0, Handler { owner: cx.w, id: GR_CLICKABLE });
            }
            _ => {}
        }
    }

    fn resume(&mut self, _cx: &mut Cx<'_>, id: u32) {
        if id == GR_CLICKABLE {
            self.clickable = true;
        }
    }
}

// ---------- UI_WWShop, modal `shop` ----------

/// GetTabList: fixed in the graph, ID = the label (CreateShopTabInfoFromButtonParam); layout 0 = UI_WWShopPage_Home,
/// 1 = UI_WWShopPage_Standard.
/// A tab's CreateShopTabInfoFromButtonParam arguments the port reads: ID, icon, IncludedTags, Filter-Enabled,
/// Filter-HasCharacter, Filter-HasSortByRarity and SortingOrder.
#[derive(Debug)]
pub struct TabInfo {
    pub id: &'static str,
    pub icon: &'static str,
    pub tags: &'static [&'static str],
    pub filter: bool,
    pub has_character: bool,
    pub by_rarity: bool,
    pub sort: u8,
}

const fn tab(
    id: &'static str,
    icon: &'static str,
    tags: &'static [&'static str],
    [filter, has_character, by_rarity]: [bool; 3],
    sort: u8,
) -> TabInfo {
    TabInfo { id, icon, tags, filter, has_character, by_rarity, sort }
}

/// Each tab's info; the flags are Filter-Enabled, Filter-HasCharacter and Filter-HasSortByRarity.
const TAB_INFO: [TabInfo; 10] = [
    tab("featured", "ui_icons_featured", &[], [false, false, false], 3),
    tab("bundles", "ui_icons_chest", &["bundle"], [false, false, false], 3),
    tab("variants", "ui_icons_skins", &["skin"], [true, true, true], 1),
    tab("ringouts", "ui_icons_ringout", &["ring_out_vfx"], [true, false, true], 2),
    tab("emotes", "ui_icons_emote", &["taunt", "emote"], [true, true, true], 2),
    tab("banners", "ui_icons_banner", &["banner"], [true, false, true], 2),
    tab("icons", "ui_icons_profileicon", &["profile_icon"], [true, false, true], 2),
    tab("announcers", "ui_icons_announcer", &["announcer_pack"], [true, false, true], 2),
    tab("badges", "ui_icons_tracker", &["stat_tracking_bundle"], [true, false, true], 2),
    tab("fighters", "ui_icons_fighters", &["character"], [true, false, false], 2),
];

const TABS: [(&str, &str, u8); 10] = [
    ("featured", "ui_icons_featured", 0),
    ("bundles", "ui_icons_chest", 1),
    ("variants", "ui_icons_skins", 1),
    ("ringouts", "ui_icons_ringout", 1),
    ("emotes", "ui_icons_emote", 1),
    ("banners", "ui_icons_banner", 1),
    ("icons", "ui_icons_profileicon", 1),
    ("announcers", "ui_icons_announcer", 1),
    ("badges", "ui_icons_tracker", 1),
    ("fighters", "ui_icons_fighters", 1),
];
const PICTURES_SHOP: &str = "shop";

const S_GLEAMIUM_OPENED: u32 = 1;
const S_GLEAMIUM_CLOSED: u32 = 2;
const S_MODAL_DISMISSED: u32 = 3;
const S_BAR_CLICK: u32 = 4;
const S_SCROLL: u32 = 5;
const SR_FOCUS_FIRST: u32 = 1;
const SR_DISMISS: u32 = 2;
const SR_FETCH: u32 = 3;

/// UI_WWShop. Not ported: the ShopManager's RefreshShop binding (None), the cvar WWShop.DebugProductInfo path
/// (FetchAllProducts), the two Delay(0) Post Initialize passes (@935, @950: the Home page's PopulateCarousel on a None
/// manager), the game instance's inventory delegate (no inventory), the manager's Selected / InteractableObject writes
/// of ModeTypeSelected (@5116-5195) and its CurrentShopTab / PreviousShopTab.
#[derive(Debug)]
pub struct WWShop {
    base: ModalBase,
    pages: Vec<WidgetId>,
    /// Per page: its products have been set (the stand-in's answer is a success, so a page loads once).
    loaded: Vec<bool>,
    /// The page whose Delay(0) fetch is pending.
    fetching: Option<usize>,
    initial_shop_open: bool,
    tab_scroll_box: Option<WidgetId>,
    scroll_bound: bool,
}

impl Default for WWShop {
    fn default() -> WWShop {
        WWShop {
            base: ModalBase::default(),
            pages: Vec::new(),
            loaded: Vec::new(),
            fetching: None,
            initial_shop_open: true,
            tab_scroll_box: None,
            scroll_bound: false,
        }
    }
}

impl WWShop {
    /// The controller of the context's widget.
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::<WWShop>::default()
    }

    fn slide_out(cx: &Cx<'_>) -> WidgetId {
        find(cx, "WBP_GlobalSlideOut")
    }

    fn bar(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_SectionBar_Shop")
    }

    fn get_more(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_WWGetMoreButton")
    }

    /// UpdateTabList: the bar's buttons, then UpdateTabBoxArray -> CreateTabContent: one page per tab in
    /// WidgetSwitcher_0 (Set Tab Info fills content that stays collapsed).
    fn update_tab_list(&mut self, cx: &mut Cx<'_>) {
        let params: Vec<Value> = TABS
            .iter()
            .map(|(id, icon, _)| {
                let mut m = Map::new();
                m.insert(P_LABEL.to_string(), json!({ "SourceString": id }));
                m.insert(P_ICON.to_string(), json!(icon));
                m.insert(P_ID.to_string(), json!(id));
                Value::Object(m)
            })
            .collect();
        let bar = Self::bar(cx);
        cx.visit::<SectionBarShop, _>(bar, |b, cx| b.populate(cx, params));
        let sw = find(cx, "WidgetSwitcher_0");
        self.pages = TABS
            .iter()
            .map(|(_, _, layout)| {
                let w =
                    MenuView::create(cx, if *layout == 0 { "UI_WWShopPage_Home" } else { "UI_WWShopPage_Standard" });
                let x = wd(cx, w);
                x.h_align = HAlign::Fill;
                x.v_align = VAlign::Fill;
                MenuView::add_child(cx, sw, w, None);
                w
            })
            .collect();
        self.loaded = vec![false; self.pages.len()];
        for (&p, info) in self.pages.clone().iter().zip(TAB_INFO.iter()) {
            cx.visit::<ShopPageStandard, _>(p, |page, cx| page.set_tab_info(cx, info));
        }
    }

    /// Every page's Update Context: UngroupGUID, the current page the modal's context.
    fn update_pages_context(&mut self, cx: &mut Cx<'_>, current: Option<WidgetId>) {
        let ctx = self.base.context.clone();
        for &p in &self.pages.clone() {
            let c = if Some(p) == current { ctx.as_str() } else { NULL_GUID };
            cx.visit::<ShopPageHome, _>(p, |page, cx| page.update_context(cx, c));
            cx.visit::<ShopPageStandard, _>(p, |page, cx| page.update_context(cx, c));
        }
    }

    /// @1328-1608 and @8275-8498: the tab's products (FetchAllProducts with no IncludedTags, else
    /// FetchProductsWithAnyTags), then the page's Set Products. The stand-in answers with success at once.
    // Unverified: ChooseProductsWithNoneOfTags (no ExcludedTags on any tab) and Sort Products (native, NQ1) keep the
    // catalogue's order here; the filter grid sorts the tabs that have one.
    fn fetch_tab(&mut self, cx: &mut Cx<'_>, at: usize) {
        let (Some(&page), Some(info)) = (self.pages.get(at), TAB_INFO.get(at)) else { return };
        let products = if info.tags.is_empty() {
            fetch_all_products(catalogue(cx))
        } else {
            fetch_products_with_any_tags(catalogue(cx), info.tags)
        };
        let ctx = self.base.context.clone();
        let id = info.id;
        cx.visit::<ShopPageHome, _>(page, |p, cx| p.set_products(cx, products.clone(), id, &ctx));
        cx.visit::<ShopPageStandard, _>(page, |p, cx| p.set_products(cx, products));
        if let Some(l) = self.loaded.get_mut(at) {
            *l = true;
        }
    }

    /// UpdateContext @4824 (GetInteractors: the get-more button's interactor).
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.base.context = ctx.to_string();
        let more = Self::get_more(cx);
        let mi = interactor_of(cx, more);
        batch_set_context(cx, &[mi], ctx);
        let bar = Self::bar(cx);
        cx.visit::<SectionBarShop, _>(bar, |b, cx| b.update_context(cx, ctx, 0));
        let so = Self::slide_out(cx);
        cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.update_context(cx, ctx));
        cx.visit::<WWGetMoreButton, _>(more, |g, cx| g.update_context(cx, ctx));
    }

    /// ModeTypeSelected(Index, ID) @5070. `index`: the bar's click's own (the bar is busy while it fires, so it cannot be
    /// asked).
    fn mode_type_selected(&mut self, cx: &mut Cx<'_>, id: &str, index: Option<usize>) {
        let bar = Self::bar(cx);
        let asked = cx.visit::<SectionBarShop, _>(bar, |b, cx| {
            b.set_current_index_by_id(cx, id);
            b.current_index()
        });
        let cur = index.or(asked).unwrap_or(0);
        let page = self.pages.get(cur).copied();
        // SetActiveWidgetAndFocus(WidgetSwitcher_0, the page)
        if let Some(at) = page.and_then(|p| self.pages.iter().position(|&x| x == p)) {
            let sw = find(cx, "WidgetSwitcher_0");
            wd(cx, sw).active = at as i32;
        }
        self.bind_current_widgets_scroll_box(cx, page, true);
        // every page gets UngroupGUID, the current one the modal's context
        self.update_pages_context(cx, page);
        // Update Tab Page Products @7979: a page not yet loaded shows its spinner, and Delay(0) @8142 -> @1328 asks the
        // shop manager (the port's stand-in) for its products
        if let Some(p) = page
            && !self.loaded.get(cur).copied().unwrap_or(true)
        {
            cx.visit::<ShopPageHome, _>(p, |page, cx| page.set_loading_status(cx, false));
            cx.visit::<ShopPageStandard, _>(p, |page, cx| page.set_loading_status(cx, false));
            self.fetching = Some(cur);
            cx.view.delay(0.0, Handler { owner: cx.w, id: SR_FETCH });
        }
        if self.initial_shop_open {
            // @6405 Delay(0.2) -> @790
            cx.view.delay(0.2, Handler { owner: cx.w, id: SR_FOCUS_FIRST });
        } else {
            self.update_shop_explicit_navigation(cx);
        }
    }

    /// Update Shop Explicit Navigation @8947 (OnPageChange). Unverified (UI_Functions.ConnectUINavigation, read in
    /// part): each From interactor gets the To widget's interactor as its manual target in that direction. The page's
    /// connections (Set Navigation In Direction, Return Top Interactable Widget) have no cells to reach.
    fn update_shop_explicit_navigation(&mut self, cx: &mut Cx<'_>) {
        let bar = Self::bar(cx);
        let (tab, tabs) = cx
            .visit::<SectionBarShop, _>(bar, |b, _| (b.active_tab_button, b.interactors.clone()))
            .unwrap_or((None, Vec::new()));
        let tab = tab.map(|t| interactor_of(cx, t));
        let more = interactor_of(cx, Self::get_more(cx));
        // @9063 / @9122: the get-more button Down and Left to the active tab
        if let Some(s) = ist(cx, more) {
            s.manual_down = tab;
            s.manual_left = tab;
        }
        // @9181: every tab Up to the get-more button
        for i in tabs {
            if let Some(s) = ist(cx, i) {
                s.manual_up = Some(more);
            }
        }
    }

    /// BindCurrentWidgetsScrollBox(ScrollToTop) @9581 -> BindSubScrollBoxToUIManager @6676.
    fn bind_current_widgets_scroll_box(&mut self, cx: &mut Cx<'_>, page: Option<WidgetId>, scroll_to_top: bool) {
        self.unbind_sub_scroll_box(cx);
        let sb = page.map(|p| cx.view.tree().find_in(p, "ScrollBox"));
        self.tab_scroll_box = sb;
        let Some(sb) = sb else { return };
        cx.view.manager.on_scroll.add(Handler { owner: cx.w, id: S_SCROLL });
        cx.view.manager.register_scrollbox(sb);
        self.scroll_bound = true;
        if scroll_to_top {
            scroll_to_start(cx.view.tree_mut(), sb);
        }
    }

    /// UnbindSubScrollBoxToUIManager @6992.
    fn unbind_sub_scroll_box(&mut self, cx: &mut Cx<'_>) {
        let Some(sb) = self.tab_scroll_box.filter(|_| self.scroll_bound) else { return };
        cx.view.manager.on_scroll.remove(Handler { owner: cx.w, id: S_SCROLL });
        cx.view.manager.unregister_scrollbox(sb);
        self.scroll_bound = false;
    }

    /// The tail @89: the delegates, the sub scroll box, the manager's CurrentShopTab / PreviousShopTab (not kept by the
    /// port's manager).
    fn unbind(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.modal_dismissed.remove(Handler { owner: me, id: S_MODAL_DISMISSED });
        let bar = Self::bar(cx);
        cx.visit::<SectionBarShop, _>(bar, |b, _| b.on_click.remove(Handler { owner: me, id: S_BAR_CLICK }));
        self.unbind_sub_scroll_box(cx);
        cx.view.release_pictures(PICTURES_SHOP);
    }
}

impl Modal for WWShop {
    fn pre_show(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.base.pre_show(cx, id);
    }

    /// PostShow @3656. The ten tab icons are held while the modal is up.
    fn post_show(&mut self, cx: &mut Cx<'_>, instructions: &str, _extra_data: &str) {
        cx.view.hold_pictures(PICTURES_SHOP, TABS.iter().map(|t| t.1));
        let id = self.base.modal_id.clone();
        let so = Self::slide_out(cx);
        cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.open_slide_out(cx, &id));
        let g = cx.view.manager.new_guid();
        self.update_context(cx, &g);
        let me = cx.w;
        let bar = Self::bar(cx);
        cx.visit::<SectionBarShop, _>(bar, |b, _| b.on_click.add(Handler { owner: me, id: S_BAR_CLICK }));
        self.mode_type_selected(cx, instructions, None);
    }

    fn pre_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    fn post_dismiss(&mut self, _cx: &mut Cx<'_>) {}

    /// RequestDismiss @4069: Delay(0) -> @89.
    fn request_dismiss(&mut self, cx: &mut Cx<'_>, _reason: &str, _from_cancel: bool) {
        cx.view.delay(0.0, Handler { owner: cx.w, id: SR_DISMISS });
    }

    fn get_modal_context(&self) -> String {
        self.base.get_modal_context()
    }
}

impl Controller for WWShop {
    fn as_modal(&mut self) -> Option<&mut dyn Modal> {
        Some(self)
    }

    /// OnInitialized @4631 -> @4177.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.initial_shop_open =
            cx.view.tree().props(cx.w).get("InitialShopOpen").and_then(Value::as_bool).unwrap_or(true);
        let me = cx.w;
        let more = Self::get_more(cx);
        cx.visit::<WWGetMoreButton, _>(more, |g, _| {
            g.modal_opened.add(Handler { owner: me, id: S_GLEAMIUM_OPENED });
            g.modal_closed.add(Handler { owner: me, id: S_GLEAMIUM_CLOSED });
        });
        cx.view.manager.modal_dismissed.add(Handler { owner: me, id: S_MODAL_DISMISSED });
        let u = find(cx, "TabUnderline");
        wd(cx, u).color = [1.0, 0.52549, 0.0, 1.0];
        self.update_tab_list(cx);
    }

    /// PreConstruct @2073.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let so = Self::slide_out(cx);
        let clip = cx.view.tree().find_in(so, "Overlay_ClipBounds");
        let x = wd(cx, clip);
        x.offsets = Margin { l: 0.0, t: 0.0, r: 2282.0, b: 0.0 };
        x.transform.translation = [0.0, 0.0];
        x.transform.scale = [1.075, 1.0];
        x.transform.shear = [-5.0, 0.0];
        x.transform.angle = 0.0;
        let k = 1.0 / 1.075;
        let gc = find(cx, "GlossaryContainer");
        let x = wd(cx, gc);
        x.transform.translation = [0.0, 0.0];
        x.transform.scale = [k, 1.0];
        x.transform.shear = [k * 5.0, 0.0];
        x.transform.angle = 0.0;
        let header = find(cx, "WBP_MenuHeader");
        let h = |cx: &Cx<'_>, n: &str| cx.view.tree().find_in(header, n);
        for n in ["FillQuadRight", "FillQuadLeft", "QuadLeft_RSpacer", "QuadRight_LSpacer", "HalftoneGradient"] {
            let w = h(cx, n);
            wd(cx, w).vis = Vis::Collapsed;
        }
        let fill = h(cx, "SpacerFillVert");
        let x = wd(cx, fill);
        x.brush_w = 0.0;
        x.brush_h = 242.0;
        let (bg, pt) = (h(cx, "txt_BackgroundTitle"), h(cx, "txt_PageTitle"));
        wd(cx, bg).font_size = 280.0;
        wd(cx, pt).font_size = 60.0;
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            // ModalOpenedSlideout (the Gleamium shop opened over this one): the scroll box goes
            S_GLEAMIUM_OPENED => self.unbind_sub_scroll_box(cx),
            // Modal Closed @9682
            S_GLEAMIUM_CLOSED => {
                let cur = {
                    let bar = Self::bar(cx);
                    cx.visit::<SectionBarShop, _>(bar, |b, _| b.current_index()).unwrap_or(0)
                };
                let page = self.pages.get(cur).copied();
                self.bind_current_widgets_scroll_box(cx, page, false);
            }
            // OnModalDismissed(Modal ID) @4124 goes to @4069. As the Glossary's (Unverified): the port's widget is gone
            // when that Delay would fire, so the tail's unbinding runs at once.
            S_MODAL_DISMISSED => {
                if arg.downcast_ref::<String>() == Some(&self.base.modal_id) {
                    self.unbind(cx);
                }
            }
            S_BAR_CLICK => {
                if let Some((index, id)) = arg.downcast_ref::<(i32, String)>() {
                    self.mode_type_selected(cx, id, usize::try_from(*index).ok());
                }
            }
            // ScrollEvent @6534
            S_SCROLL => {
                if let Some(&(player, _, amount)) = arg.downcast_ref::<(i32, i32, f64)>()
                    && player == 0
                    && let Some(sb) = self.tab_scroll_box
                {
                    scroll_list(cx.view.tree_mut(), sb, amount * 2.0);
                }
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        match id {
            SR_FOCUS_FIRST => {
                let bar = Self::bar(cx);
                let first = cx.visit::<SectionBarShop, _>(bar, |b, _| b.interactors.first().copied()).flatten();
                if let Some(i) = first {
                    set_focused(cx, i, true);
                }
                self.initial_shop_open = false;
                self.update_shop_explicit_navigation(cx);
            }
            SR_FETCH => {
                if let Some(at) = self.fetching.take() {
                    self.fetch_tab(cx, at);
                }
            }
            SR_DISMISS => {
                let id = self.base.modal_id.clone();
                let so = Self::slide_out(cx);
                cx.visit::<GlobalSlideOut, _>(so, |s, cx| s.close_slide_out(cx, &id));
                self.unbind(cx);
            }
            _ => {}
        }
    }
}
