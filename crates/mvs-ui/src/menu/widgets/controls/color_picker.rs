//! UI_ColorButton and UI_ColorPicker: the Interface page's colour
//! rows. A colour is a CentralPalette row, named by the row's name.

use std::any::Any;

use super::buttons::{CLEAR, HOVER_BACKING, WHITE};
use super::interactor::{batch_set_context, set_focused};
use super::tooltip::tooltip_params;
use super::{bind, find, focus_frame, ist, quad_color, wd};
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::view::MenuView;
use crate::menu::widgets::common::text;
use crate::slate::{Vis, WidgetId};

// ---------- UI_ColorButton ----------

const B_CLICK: u32 = 1;
const B_FOCUS: u32 = 2;
const B_UNFOCUS: u32 = 3;

/// A swatch of a colour picker's grid. Its Palette Entry is a CentralPalette row.
#[derive(Debug, Default)]
pub struct ColorButton {
    pub on_click_with_source: Event<WidgetId>,
    pub on_click: Event<()>,
    pub palette_entry: String,
}

impl ColorButton {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ColorButton::default())
    }

    pub fn interactor(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
        cx.view.tree().find_in(w, "UI_Interactor")
    }

    /// The row's UI colour (magenta for a row the palette lacks).
    fn colour(&self, cx: &Cx<'_>) -> Rgba {
        cx.view.palette().get(&self.palette_entry).copied().unwrap_or([1.0, 0.0, 1.0, 1.0])
    }

    /// Init: the Palette Entry and the swatch's colour.
    pub fn init(&mut self, cx: &mut Cx<'_>, row: &str) {
        self.palette_entry = row.to_string();
        let c = self.colour(cx);
        let q = find(cx, "WBP_Quadrilateral");
        quad_color(cx, q, c);
    }

    fn focus_anim(&self, cx: &mut Cx<'_>, play: &str, stop: &str) {
        let c = self.colour(cx);
        let q = find(cx, "WBP_Quadrilateral");
        quad_color(cx, q, c);
        let me = cx.w;
        if cx.view.is_animation_playing(me, stop) {
            cx.view.stop_animation(me, stop);
        }
        cx.view.play(me, play, false, 1.0);
    }
}

impl Controller for ColorButton {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = Self::interactor(cx, cx.w);
        // OnClick @207 -> @10: OnClickWithSource(self), then OnClick
        bind(cx, i, |s| &mut s.on_click, B_CLICK);
        // OnFocus @212 / OnUnfocus @380: the colour again, and the focus animation (the other one stopped)
        bind(cx, i, |s| &mut s.on_focus, B_FOCUS);
        bind(cx, i, |s| &mut s.on_unfocus, B_UNFOCUS);
    }

    /// PreConstruct @54: the focus wobble SelfHitTestInvisible, Hidden with Hide Wobble.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let hide = super::prop_bool(cx, "Hide Wobble");
        let w = find(cx, "UI_FocusedCommon");
        wd(cx, w).vis = if hide { Vis::Hidden } else { Vis::SelfHitTestInvisible };
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let (fc, i) = (find(cx, "UI_FocusedCommon"), Self::interactor(cx, cx.w));
        focus_frame(cx, fc, i);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            B_CLICK => {
                let me = cx.w;
                cx.emit(self, |c: &ColorButton| &c.on_click_with_source, &me);
                cx.emit(self, |c: &ColorButton| &c.on_click, &());
            }
            B_FOCUS => self.focus_anim(cx, "OnFocus", "OnUnfocus"),
            B_UNFOCUS => self.focus_anim(cx, "OnUnfocus", "OnFocus"),
            _ => {}
        }
    }
}

// ---------- UI_ColorPicker ----------

const P_CLICK: u32 = 1;
const P_FOCUS: u32 = 2;
const P_UNFOCUS: u32 = 3;
const P_INTERACTOR_FOCUSED: u32 = 4;
const P_SWATCH: u32 = 5;

/// A row with its label and the current colour; a click opens the grid of swatches in place (menuContainer, below
/// the row). The grid's swatches are made by Populate Colors, five to a row. A swatch's click selects its colour,
/// collapses the grid and broadcasts OnClosed and OnColorSelected; the row keeps its open look (Open stays true) until
/// the next click on it or a focus outside it.
#[derive(Debug, Default)]
pub struct ColorPicker {
    /// The CentralPalette row picked.
    pub on_color_selected: Event<String>,
    pub on_closed: Event<()>,
    open: bool,
    color_buttons: Vec<WidgetId>,
    color_interactors: Vec<WidgetId>,
    /// Selected Palette Entry (a CentralPalette row name; '' = the class default, no row).
    pub selected: String,
}

impl ColorPicker {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ColorPicker::default())
    }

    fn main_interactor(cx: &Cx<'_>) -> WidgetId {
        find(cx, "mainInteractor")
    }

    pub fn interactors(&self, cx: &Cx<'_>) -> Vec<WidgetId> {
        let mut list = vec![Self::main_interactor(cx)];
        list.extend(&self.color_interactors);
        list
    }

    /// UpdateContext @916.
    pub fn update_context(&self, cx: &mut Cx<'_>, ctx: &str) {
        let list = self.interactors(cx);
        batch_set_context(cx, &list, ctx);
    }

    /// ToggleOpen(Open) @1109: opens when the argument is false, else closes and broadcasts OnClosed.
    pub fn toggle_open(&mut self, cx: &mut Cx<'_>, is_open: bool) {
        let label = find(cx, "Label");
        let fill = find(cx, "QuadFill");
        let arrow = find(cx, "arrow");
        let menu = find(cx, "menuContainer");
        let backing = find(cx, "HoverBacking");
        let border = find(cx, "CurrentColorQuad_Border");
        if !is_open {
            wd(cx, menu).vis = Vis::SelfHitTestInvisible;
            let l = wd(cx, label);
            l.opacity = 1.0;
            l.font_size = 20.0;
            wd(cx, backing).color = HOVER_BACKING;
            self.focus_selected_color(cx);
            wd(cx, border).vis = Vis::SelfHitTestInvisible;
            wd(cx, fill).opacity = 1.0;
            wd(cx, arrow).transform.scale = [1.0, -1.0];
            self.open = true;
            Self::raise(cx, true);
            return;
        }
        wd(cx, menu).vis = Vis::Collapsed;
        Self::raise(cx, false);
        let l = wd(cx, label);
        l.opacity = 0.65;
        l.font_size = 18.0;
        wd(cx, backing).color = CLEAR;
        wd(cx, border).vis = Vis::Hidden;
        wd(cx, fill).opacity = 0.4;
        wd(cx, arrow).transform.scale = [1.0, 1.0];
        self.open = false;
        cx.emit(self, |c: &ColorPicker| &c.on_closed, &());
    }

    /// Slate paints by layer id, and the grid (a CanvasPanel child, MaxLayerId + 1) sits deeper than the later rows
    /// of the page's ScrollBox, so it draws and hit-tests over them.
    // Deviation: the port's paint is in sibling order: the open picker's row is raised over its ScrollBox siblings.
    fn raise(cx: &mut Cx<'_>, on: bool) {
        let mut w = cx.w;
        while let Some(parent) = cx.view.tree().widget(w).parent {
            wd(cx, w).paint_order = i32::from(on);
            if cx.view.tree().widget(parent).kind == "ScrollBox" {
                break;
            }
            w = parent;
        }
    }

    /// ButtonClicked @1855.
    fn button_clicked(&mut self, cx: &mut Cx<'_>, b: WidgetId) {
        let Some(row) = cx.visit::<ColorButton, _>(b, |b, _| b.palette_entry.clone()) else { return };
        self.set_selected_palette_entry(cx, &row);
        let menu = find(cx, "menuContainer");
        wd(cx, menu).vis = Vis::Collapsed;
        Self::raise(cx, false);
        cx.emit(self, |c: &ColorPicker| &c.on_closed, &());
        cx.emit(self, |c: &ColorPicker| &c.on_color_selected, &row);
    }

    fn focus_visuals(cx: &mut Cx<'_>, on: bool) {
        let fill = find(cx, "QuadFill");
        wd(cx, fill).opacity = if on { 1.0 } else { 0.4 };
        let label = find(cx, "Label");
        let l = wd(cx, label);
        l.opacity = if on { 1.0 } else { 0.65 };
        l.font_size = if on { 20.0 } else { 18.0 };
        let b = find(cx, "HoverBacking");
        wd(cx, b).color = if on { HOVER_BACKING } else { CLEAR };
        // @2252
        let arrow = find(cx, "arrow");
        let a = wd(cx, arrow);
        a.color = WHITE;
        a.transform.scale = [1.0, 1.0];
    }

    /// FocusSelectedColor (@3003 -> @572): the swatch of the selected row takes the focus.
    fn focus_selected_color(&self, cx: &mut Cx<'_>) {
        for &b in &self.color_buttons {
            let row = cx.visit::<ColorButton, _>(b, |b, _| b.palette_entry.clone());
            if row.as_deref() == Some(self.selected.as_str()) {
                let i = ColorButton::interactor(cx, b);
                set_focused(cx, i, false);
                return;
            }
        }
    }

    /// Populate Colors: a UI_ColorButton per entry, its click bound, added to menuGrid at (Row, Column), five columns.
    pub fn populate_colors(&mut self, cx: &mut Cx<'_>, entries: &[String]) {
        let grid = find(cx, "menuGrid");
        for b in std::mem::take(&mut self.color_buttons) {
            MenuView::discard(cx, b);
        }
        self.color_interactors.clear();
        let me = cx.w;
        for (n, e) in entries.iter().enumerate() {
            let w = MenuView::create(cx, "UI_ColorButton");
            cx.visit::<ColorButton, _>(w, |b, cx| {
                b.on_click_with_source.add(Handler { owner: me, id: P_SWATCH });
                b.init(cx, e);
            });
            self.color_buttons.push(w);
            let x = wd(cx, w);
            x.row = (n / 5) as i32;
            x.col = (n % 5) as i32;
            MenuView::add_child(cx, grid, w, None);
            let i = ColorButton::interactor(cx, w);
            self.color_interactors.push(i);
        }
    }

    /// SetSelectedPaletteEntry: the current colour quad takes the row's UI colour.
    fn set_selected_palette_entry(&mut self, cx: &mut Cx<'_>, row: &str) {
        self.selected = row.to_string();
        if let Some(c) = cx.view.palette().get(row).copied() {
            let q = find(cx, "CurrentColorQuad");
            quad_color(cx, q, c);
        }
    }

    /// SetSelectedColorByColorID: the palette choice of that row, when there is one.
    pub fn set_selected_color_by_color_id(&mut self, cx: &mut Cx<'_>, row: &str) {
        if cx.view.palette_choices().iter().any(|r| r == row) {
            self.set_selected_palette_entry(cx, row);
        }
    }
}

impl Controller for ColorPicker {
    /// OnInitialized @704: the focus visuals' interactor, the manager's OnInteractorFocused, the label.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = Self::main_interactor(cx);
        let fc = find(cx, "UI_FocusedCommon");
        focus_frame(cx, fc, i);
        let me = cx.w;
        cx.view.manager.on_interactor_focused.add(Handler { owner: me, id: P_INTERACTOR_FOCUSED });
        self.pre_construct(cx);
        // OnClick @680: ToggleOpen(Open)
        bind(cx, i, |s| &mut s.on_click, P_CLICK);
        bind(cx, i, |s| &mut s.on_focus, P_FOCUS);
        bind(cx, i, |s| &mut s.on_unfocus, P_UNFOCUS);
    }

    /// PreConstruct @634.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let t = text(cx.view.tree().props(cx.w).get("LabelText"));
        let l = find(cx, "Label");
        wd(cx, l).text = t;
    }

    /// Construct @2624: a Tooltip is the row's tip.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let tip = text(cx.view.tree().props(cx.w).get("Tooltip"));
        if !tip.is_empty() {
            let i = Self::main_interactor(cx);
            if let Some(s) = ist(cx, i) {
                s.tooltip_params = tooltip_params(&tip, true, 850.0, -25.0, 12);
            }
        }
    }

    /// Destruct @905 (the port lets go of the manager's binding).
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.on_interactor_focused.remove(Handler { owner: me, id: P_INTERACTOR_FOCUSED });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            P_CLICK => {
                let open = self.open;
                self.toggle_open(cx, open);
            }
            // mainInteractor OnFocus @2008 / OnUnfocus @2375: nothing while open
            P_FOCUS | P_UNFOCUS if !self.open => {
                let on = id == P_FOCUS;
                let b = find(cx, "CurrentColorQuad_Border");
                wd(cx, b).vis = if on { Vis::SelfHitTestInvisible } else { Vis::Hidden };
                Self::focus_visuals(cx, on);
            }
            // OnInteractorFocused @995: a focus outside the row and its swatches closes it
            P_INTERACTOR_FOCUSED => {
                if let Some(&i) = arg.downcast_ref::<WidgetId>()
                    && !self.interactors(cx).contains(&i)
                {
                    self.toggle_open(cx, true);
                }
            }
            P_SWATCH => {
                if let Some(&b) = arg.downcast_ref::<WidgetId>() {
                    self.button_clicked(cx, b);
                }
            }
            _ => {}
        }
    }
}
