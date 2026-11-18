//! (the port's own) The Lab Tools panel: the tools of the running Lab (the app's
//! lab_tools.rs holds them and drives this panel), drawn at the top right of the match. It is built of the build's
//! own menu widgets: the backing quad of UI_LAB_PlayerPanel, the "Settings" header of UI_LAB_Panel,
//! UI_ToggleSwitchRework, UI_PlusMinusHorizontal, UI_ButtonSecondary, and the Lab's fighter card and team bar
//! (UI_LAB_PlayerPanel_Character, teamContainer). The build has no such panel: its words are typed (Show Hitboxes
//! and Show Hurtboxes excepted, the labels of UI_LAB_Panel's two collapsed toggles).
//!
//! Deviation: no row holds getters and setters on the match; here a row is data with an id, the host gives the
//! panel its state (`ToolsState`) at each refresh and takes what the controls asked (`ToolCmd`) after the frame.

use std::any::Any;
use std::collections::HashMap;

use serde_json::{Value, json};

use crate::bundle::{Rgba, WData};
use crate::menu::controller::{Controller, Cx, Handler};
use crate::menu::pictures::skin_icon;
use crate::menu::screens::lab::LabPlayerPanelCharacter;
use crate::menu::view::MenuView;
use crate::menu::widgets::common::{palette_entry, rgba};
use crate::menu::widgets::controls::buttons::{ButtonSecondary, PlusMinusHorizontal, ToggleSwitch};
use crate::menu::widgets::controls::input_label::InputLabel;
use crate::menu::widgets::controls::interactor::{FocusedCommon, batch_set_context, set_focused, set_interactable};
use crate::slate::{HAlign, Margin, VAlign, Vis, WidgetId};

/// The panel's inner width and its rows' height, in the menu's units (a 2560 x 1440 screen's pixels).
const WIDTH: f64 = 560.0;
const ROW: f64 = 88.0;
const PAD: f64 = 26.0;
/// The fold button's border: the buttons' colour this much darker (it sits alone in the header, over the panel's
/// dark backing, where the buttons' own colour stands out).
const FOLD_DIM: f64 = 0.25;
/// The rows' enter: each slides in from the right and fades in, one after the other (the port's own: the build's
/// UI_ToggleSwitchRework and UI_PlusMinusHorizontal have no animation; the slide is that of the keybind rows' enter
/// as the eye reads it, not a build value).
const ENTER_TIME: f64 = 0.22;
const ENTER_STAGGER: f64 = 0.03;
const ENTER_SLIDE: f64 = 40.0;
/// The fighter card's size (UI_LAB_PlayerPanel_Character, at the portrait's 300 x 350 aspect).
const CARD_W: f64 = 170.0;
const CARD_H: f64 = 218.0;
/// A team's colour (screens/lab.rs team_color: UI_LAB_PlayerPanel @5083-5302).
const TEAM_PALETTE: [u8; 4] = [0, 1, 3, 4];
/// The owner of the fighters' portraits the card holds.
const PICTURES_OWNER: &str = "labtools:card";

/// An sRGB colour (a CSS hex colour's bytes) as the linear colour a widget is drawn in.
pub fn linear(c: [u8; 3]) -> Rgba {
    let f = |b: u8| {
        let v = f64::from(b) / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    [f(c[0]), f(c[1]), f(c[2]), 1.0]
}

/// A row's label: typed words, or a text of the build's (an FText prop of its widgets).
#[derive(Clone, Debug, PartialEq)]
pub enum ToolLabel {
    Typed(String),
    Build(Value),
}

impl ToolLabel {
    fn text(&self) -> Value {
        match self {
            ToolLabel::Typed(t) => invariant(t),
            ToolLabel::Build(v) => v.clone(),
        }
    }
}

/// What a stepper row shows for a value.
pub type StepText = fn(f64) -> String;

/// A row of a page; the host's state for it is read again at each refresh, so a row shows the state wherever it was
/// changed. `id` names a control to the host.
#[derive(Clone, Debug)]
pub enum ToolRow {
    Heading(ToolLabel),
    /// A switch; `swatch`: a colour the row stands for (the shapes' colours, sRGB), drawn before its label. The host
    /// gives it its state and whether it can be changed (the Lab's leader's settings).
    Toggle {
        id: u32,
        label: ToolLabel,
        swatch: Option<[u8; 3]>,
    },
    /// A value between `min` and `max`, by `step`; `text`: what the row shows for a value.
    Stepper {
        id: u32,
        label: String,
        min: f64,
        max: f64,
        step: f64,
        text: StepText,
    },
    /// A line of buttons: (id, label) each.
    Buttons(Vec<(u32, String)>),
    /// One of the host's fighters, the chosen one's card: its character and skin, its team.
    Fighter,
}

#[derive(Clone, Debug)]
pub struct ToolPage {
    pub name: String,
    pub rows: Vec<ToolRow>,
}

/// A fighter's card: its character (None: none) and skin (SkinData name), its team index.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FighterInfo {
    pub character: Option<String>,
    pub skin: String,
    pub team: i32,
}

/// What the panel shows of the host (what the rows' getters read, and the page and fold).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolsState {
    pub page: usize,
    pub folded: bool,
    /// Each switch's (on, enabled).
    pub toggles: HashMap<u32, (bool, bool)>,
    /// Each stepper's value.
    pub values: HashMap<u32, f64>,
    /// The fighters (None: a place with no fighter's data).
    pub fighters: Vec<Option<FighterInfo>>,
    /// The chosen fighter.
    pub sel: usize,
}

/// What a control asked of the host (what the rows' setters did), in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ToolCmd {
    Toggle(u32, bool),
    Value(u32, f64),
    Click(u32),
    Page(usize),
    /// The fold button: folded or not.
    Fold(bool),
    Fighter(usize),
}

const H_FOLD: u32 = 1;
const H_SWITCH_PAGE: u32 = 2;
const H_TAB: u32 = 3;
const H_TAB_UNFOCUS: u32 = 4;
const H_TOGGLE: u32 = 5;
const H_STEP: u32 = 6;
const H_BUTTON: u32 = 7;
const H_ARROW: u32 = 8;

/// A handler id: its kind, page, row and the control's place in the row.
fn hid(kind: u32, page: usize, row: usize, k: usize) -> u32 {
    (kind << 24) | ((page as u32 & 0xff) << 16) | ((row as u32 & 0xff) << 8) | (k as u32 & 0xff)
}

fn split(id: u32) -> (u32, usize, usize, usize) {
    (id >> 24, ((id >> 16) & 0xff) as usize, ((id >> 8) & 0xff) as usize, (id & 0xff) as usize)
}

/// The node of a widget class's tree by its name.
pub fn find_node<'a>(n: &'a WData, name: &str) -> Option<&'a WData> {
    if n.n == name {
        return Some(n);
    }
    n.c.iter().find_map(|c| find_node(c, name))
}

fn invariant(t: &str) -> Value {
    json!({ "CultureInvariantString": t })
}

fn rgba_value(c: Rgba) -> Value {
    json!({ "R": c[0], "G": c[1], "B": c[2], "A": c[3] })
}

/// The properties of node `name` of class `class`'s tree (an empty object without them).
fn node_props(cx: &Cx<'_>, class: &str, name: &str) -> serde_json::Map<String, Value> {
    cx.view
        .bundle
        .widgets
        .get(class)
        .and_then(|c| find_node(&c.root, name))
        .and_then(|n| n.p.as_object().cloned())
        .unwrap_or_default()
}

/// A plain widget (`new Widget(type, name, props, slotProps, slot)`).
fn plain(cx: &mut Cx<'_>, t: &str, name: &str, p: serde_json::Map<String, Value>, s: Value, slot: &str) -> WidgetId {
    let n =
        WData { t: t.to_owned(), n: name.to_owned(), p: Value::Object(p), st: slot.to_owned(), s, ..WData::default() };
    let bundle = cx.view.bundle.clone();
    cx.view.tree_mut().create_node(&bundle, &n)
}

fn obj(v: Value) -> serde_json::Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => serde_json::Map::new(),
    }
}

/// A row bound to its widgets.
#[derive(Debug)]
struct Bound {
    /// The row's widget (the enter animation's).
    w: WidgetId,
    kind: BoundKind,
}

#[derive(Debug)]
enum BoundKind {
    None,
    Toggle { id: u32, w: WidgetId, i: WidgetId },
    Stepper { id: u32, w: WidgetId, value: WidgetId, minus: WidgetId, plus: WidgetId, max: f64, text: StepText },
    Buttons,
    Fighter(Card),
}

/// The fighter card's widgets.
#[derive(Debug, Clone, Copy)]
struct Card {
    card: WidgetId,
    bg: WidgetId,
    label: WidgetId,
    name: WidgetId,
    arrows: [WidgetId; 2],
}

/// The panel, the controller of its root canvas.
#[derive(Debug)]
pub struct LabToolsPanel {
    pages: Vec<ToolPage>,
    state: ToolsState,
    cmds: Vec<ToolCmd>,
    root: WidgetId,
    /// The panel's box, for the pointer's test (`covers`).
    size_box: WidgetId,
    body: WidgetId,
    page_boxes: Vec<WidgetId>,
    tabs: Vec<WidgetId>,
    fold: WidgetId,
    /// The fold button's key.
    key: WidgetId,
    bound: Vec<Vec<Bound>>,
    interactors: Vec<WidgetId>,
    tab_color: Option<Rgba>,
    tabs_at: Option<usize>,
    /// The enter animation's start (the view's clock, s; None: none running) and whether the body was shown at the
    /// last refresh.
    enter_at: Option<f64>,
    was_open: bool,
    /// The portraits held (their names joined).
    held: Option<String>,
}

impl LabToolsPanel {
    /// The panel of `pages` built over the view's viewport (the root canvas, the panel in its top right corner) at
    /// Z 0, showing `state`: the root canvas.
    pub fn add(cx: &mut Cx<'_>, pages: Vec<ToolPage>, state: ToolsState) -> WidgetId {
        let root = plain(cx, "CanvasPanel", "RootCanvas", serde_json::Map::new(), Value::Null, "");
        let mut p = LabToolsPanel {
            pages: Vec::new(),
            state,
            cmds: Vec::new(),
            root,
            size_box: root,
            body: root,
            page_boxes: Vec::new(),
            tabs: Vec::new(),
            fold: root,
            key: root,
            bound: Vec::new(),
            interactors: Vec::new(),
            tab_color: None,
            tabs_at: None,
            enter_at: None,
            was_open: false,
            held: None,
        };
        p.build(cx, &pages);
        p.pages = pages;
        cx.view.put_controller(root, Box::new(p));
        MenuView::add_to_viewport(cx, root, 0);
        cx.visit::<LabToolsPanel, _>(root, |p, cx| p.start(cx));
        root
    }

    fn build(&mut self, cx: &mut Cx<'_>, pages: &[ToolPage]) {
        let root = self.root;
        let size = plain(
            cx,
            "SizeBox",
            "LabTools",
            obj(json!({ "WidthOverride": WIDTH + 2.0 * PAD, "bOverride_WidthOverride": true })),
            Value::Null,
            "",
        );
        cx.view.tree_mut().add(root, size);
        {
            let w = cx.view.tree_mut().widget_mut(size);
            w.anchors = [1.0, 0.0, 1.0, 0.0];
            w.offsets = Margin { l: -60.0, t: 40.0, r: 0.0, b: 0.0 };
            w.align = [1.0, 0.0];
            w.auto_size = true;
        }
        self.size_box = size;
        let over = plain(
            cx,
            "Overlay",
            "Overlay_0",
            obj(json!({ "Visibility": "ESlateVisibility::Visible" })),
            Value::Null,
            "",
        );
        cx.view.tree_mut().add(size, over);
        // UI_LAB_PlayerPanel's PanelBacking, the Lab menu's columns, in its colour without its gradient (the panel's
        // lower rows are read over the match) and square: the panel's height changes with its page and its fold,
        // and the column's cut top left corner would change its slant with it
        let mut back = node_props(cx, "UI_LAB_PlayerPanel", "PanelBacking");
        if !back.is_empty() {
            back.remove("MaterialOverride");
            back.insert("TopLeft".into(), json!({ "X": 0, "Y": 0 }));
            let q = plain(
                cx,
                "WBP_Quadrilateral",
                "PanelBacking",
                back,
                json!({ "HorizontalAlignment": "HAlign_Fill", "VerticalAlignment": "VAlign_Fill" }),
                "OverlaySlot",
            );
            // (the panel's size is its content's: the quad's brush asks for none)
            let w = cx.view.tree_mut().widget_mut(q);
            w.brush_w = 0.0;
            w.brush_h = 0.0;
            cx.view.tree_mut().add(over, q);
        }
        let col = plain(
            cx,
            "VerticalBox",
            "Content",
            serde_json::Map::new(),
            json!({ "Padding": { "Left": PAD, "Top": PAD + 12.0, "Right": PAD, "Bottom": PAD } }),
            "OverlaySlot",
        );
        cx.view.tree_mut().widget_mut(col).h_align = HAlign::Fill;
        cx.view.tree_mut().add(over, col);

        // the header: UI_LAB_Panel's "Settings" text style, and the fold button
        let head = plain(
            cx,
            "HorizontalBox",
            "Header",
            serde_json::Map::new(),
            json!({ "Padding": { "Bottom": 12 } }),
            "VerticalBoxSlot",
        );
        cx.view.tree_mut().add(col, head);
        let mut style = node_props(cx, "UI_LAB_Panel", "TextBlock_73");
        style.insert("Text".into(), invariant("Lab Tools"));
        let title = plain(
            cx,
            "TextBlock",
            "Title",
            style,
            json!({ "Size": { "SizeRule": "ESlateSizeRule::Fill" }, "VerticalAlignment": "VAlign_Center" }),
            "HorizontalBoxSlot",
        );
        cx.view.tree_mut().add(head, title);
        let fold = self.button(cx, "", 250.0, 56.0, H_FOLD);
        {
            let w = cx.view.tree_mut().widget_mut(fold);
            w.h_align = HAlign::Right;
            w.v_align = VAlign::Center;
        }
        cx.view.tree_mut().add(head, fold);
        // its key before its label: the button's own input label (Show Input Label, the host's action), the key of
        // the device of the last input as the game's other labels show it
        cx.view.tree_mut().set_prop(fold, "Show Input Label", Value::Bool(true));
        cx.view.tree_mut().set_prop(fold, "Input Action", Value::String("LabTools".into())); // (input_label.rs ACTION_NAMES)
        self.fold = fold;

        let body = plain(cx, "VerticalBox", "Body", serde_json::Map::new(), Value::Null, "VerticalBoxSlot");
        cx.view.tree_mut().add(col, body);
        self.body = body;
        let tab_row = plain(
            cx,
            "HorizontalBox",
            "Tabs",
            serde_json::Map::new(),
            json!({ "Padding": { "Bottom": 14 } }),
            "VerticalBoxSlot",
        );
        cx.view.tree_mut().add(body, tab_row);
        let n = pages.len().max(1) as f64;
        for (i, p) in pages.iter().enumerate() {
            let b = self.button(cx, &p.name, ((WIDTH - (n - 1.0) * 8.0) / n).floor(), 64.0, hid(H_TAB, 0, 0, i));
            let w = cx.view.tree_mut().widget_mut(b);
            w.pad = Margin { l: if i > 0 { 8.0 } else { 0.0 }, t: 0.0, r: 0.0, b: 0.0 };
            w.fill = 1.0;
            cx.view.tree_mut().add(tab_row, b);
            self.tabs.push(b);
        }
        for (pi, p) in pages.iter().enumerate() {
            let pb = plain(
                cx,
                "VerticalBox",
                &format!("Page_{}", p.name),
                serde_json::Map::new(),
                Value::Null,
                "VerticalBoxSlot",
            );
            cx.view.tree_mut().add(body, pb);
            self.page_boxes.push(pb);
            let rows = p.rows.iter().enumerate().map(|(ri, r)| self.row(cx, pb, pi, ri, r)).collect();
            self.bound.push(rows);
        }
    }

    /// The rest of the constructor once the panel is in the viewport: one context for every control, the fold
    /// button's look, the page keys, the tabs' focus look, the first refresh.
    fn start(&mut self, cx: &mut Cx<'_>) {
        let me = self.root;
        // every control in the view's one context (the view has no other widget)
        let ctx = cx.view.manager.new_guid();
        cx.view.manager.set_root_context_for_level(&ctx);
        crate::menu::Manager::set_current_context(cx, 0, &ctx, false);
        let list = self.interactors.clone();
        batch_set_context(cx, &list, &ctx);
        if let Some(c) = self.tab_color {
            let dim = [c[0] * FOLD_DIM, c[1] * FOLD_DIM, c[2] * FOLD_DIM, c[3]];
            cx.visit::<ButtonSecondary, _>(self.fold, |b, cx| b.reset_style(cx, Some(dim)));
        }
        // L1 / R1 (UI_Manager's page keys) go to the page before / after, round the ends, the focus on its button
        cx.view.manager.on_request_switch_page.add(Handler { owner: me, id: H_SWITCH_PAGE });
        // the selected page's button keeps the focus outline when the focus leaves it
        for (i, &b) in self.tabs.iter().enumerate() {
            let iw = ButtonSecondary::interactor(cx, b);
            if let Some(s) = cx.view.manager.interactor_mut(iw) {
                s.on_unfocus.add(Handler { owner: me, id: hid(H_TAB_UNFOCUS, 0, 0, i) });
            }
        }
        self.key = cx.view.tree().find_in(self.fold, "UI_InputLabel");
        self.refresh(cx);
    }

    /// The root canvas (the screen's layer).
    pub fn root(&self) -> WidgetId {
        self.root
    }

    /// What the controls asked since the last call.
    pub fn take_cmds(&mut self) -> Vec<ToolCmd> {
        std::mem::take(&mut self.cmds)
    }

    /// The fold button's key again (a binding may have changed while the panel was not shown).
    pub fn refresh_keys(&mut self, cx: &mut Cx<'_>) {
        cx.visit::<InputLabel, _>(self.key, |k, cx| k.force_refresh(cx));
    }

    /// The panel's widgets take the host's state.
    pub fn set_state(&mut self, cx: &mut Cx<'_>, state: ToolsState) {
        self.state = state;
        self.refresh(cx);
    }

    /// The panel's widgets take the state it holds.
    fn refresh(&mut self, cx: &mut Cx<'_>) {
        let folded = self.state.folded;
        let tree = cx.view.tree_mut();
        tree.widget_mut(self.body).vis = if folded { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
        cx.visit::<ButtonSecondary, _>(self.fold, |b, cx| b.set_label(cx, if folded { "Show" } else { "Hide" }));
        let at = self.state.page;
        // (a style reset clears the focus look: only when the page changed)
        if Some(at) != self.tabs_at {
            for (i, &b) in self.tabs.clone().iter().enumerate() {
                self.mark_tab(cx, b, i == at);
            }
        }
        // the rows enter when the panel opens and when the page changes
        if !folded && (!self.was_open || Some(at) != self.tabs_at) {
            self.enter_at = Some(cx.now());
        }
        self.tabs_at = Some(at);
        self.was_open = !folded;
        for (i, &b) in self.page_boxes.iter().enumerate() {
            cx.view.tree_mut().widget_mut(b).vis = if i == at { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        }
        if !folded {
            for k in 0..self.bound.get(at).map_or(0, Vec::len) {
                self.sync(cx, at, k);
            }
        }
    }

    /// The enter animation at the view's clock, once a frame.
    fn animate(&mut self, cx: &mut Cx<'_>) {
        let (Some(start), Some(at)) = (self.enter_at, self.tabs_at) else { return };
        let rows: Vec<WidgetId> = self.bound.get(at).map_or_else(Vec::new, |b| b.iter().map(|b| b.w).collect());
        let t0 = cx.now() - start;
        for (k, &w) in rows.iter().enumerate() {
            let t = ((t0 - k as f64 * ENTER_STAGGER) / ENTER_TIME).clamp(0.0, 1.0);
            let e = 1.0 - (1.0 - t).powi(3);
            let x = cx.view.tree_mut().widget_mut(w);
            x.transform.translation[0] = ENTER_SLIDE * (1.0 - e);
            x.opacity = e;
        }
        if t0 > ENTER_TIME + rows.len() as f64 * ENTER_STAGGER {
            self.enter_at = None;
        }
    }

    /// The focus on the selected page's button (the panel opens with it there, not on the fold button).
    pub fn focus_page(&mut self, cx: &mut Cx<'_>) {
        let Some(&b) = self.tabs_at.and_then(|i| self.tabs.get(i)) else { return };
        let i = ButtonSecondary::interactor(cx, b);
        let mode = cx.view.manager.cursor_mode[0];
        set_focused(cx, i, mode);
    }

    /// The selected page's button: its UI_FocusedCommon outline shown as on a focused button (the others' hidden
    /// unless one has the focus).
    fn mark_tab(&mut self, cx: &mut Cx<'_>, b: WidgetId, on: bool) {
        let fc = cx.view.tree().find_in(b, "UI_FocusedCommon");
        if on {
            // (the focused look: the outline and the backing at full opacity, ButtonSecondary OnFocus)
            cx.visit::<FocusedCommon, _>(fc, |f, cx| f.focus(cx));
            let q = cx.view.tree().find_in(b, "QuadBacking");
            cx.view.tree_mut().widget_mut(q).opacity = 1.0;
            return;
        }
        let i = ButtonSecondary::interactor(cx, b);
        if cx.view.manager.interactor(i).is_some_and(|s| s.is_focused) {
            return;
        }
        cx.visit::<ButtonSecondary, _>(b, |b, cx| b.reset_style(cx, None));
        let wob = cx.view.tree().find_in(fc, "Wobbler");
        cx.view.tree_mut().widget_mut(wob).vis = Vis::Hidden;
    }

    /// The pointer (device pixels of the view) is over the panel.
    pub fn covers(&self, cx: &Cx<'_>, x: f64, y: f64) -> bool {
        let Some(m) = cx.view.tree().painted(self.size_box) else { return false };
        let t = m.transform;
        let (x0, y0) = (t[4], t[5]);
        x >= x0 && y >= y0 && x <= x0 + m.size[0] * t[0] && y <= y0 + m.size[1] * t[3]
    }

    /// A UI_ButtonSecondary of `label` at `w` x `h`, its click handler `id`.
    fn button(&mut self, cx: &mut Cx<'_>, label: &str, w: f64, h: f64, id: u32) -> WidgetId {
        let b = MenuView::create(cx, "UI_ButtonSecondary");
        let tree = cx.view.tree_mut();
        tree.set_prop(b, "buttonLabel", invariant(label));
        tree.set_prop(b, "OverrideSize", json!({ "X": w, "Y": h }));
        tree.set_prop(b, "FontSize", json!(22));
        tree.set_prop(b, "LabelPadding", json!({ "Left": 12, "Top": 6, "Right": 12, "Bottom": 6 }));
        if self.tab_color.is_none() {
            self.tab_color = cx.view.tree().props(b).get("ButtonColor").filter(|c| c.get("R").is_some()).map(rgba);
        }
        let me = self.root;
        cx.visit::<ButtonSecondary, _>(b, |s, _| s.on_click.add(Handler { owner: me, id }));
        let i = ButtonSecondary::interactor(cx, b);
        self.interactors.push(i);
        b
    }

    fn row(&mut self, cx: &mut Cx<'_>, pb: WidgetId, pi: usize, ri: usize, r: &ToolRow) -> Bound {
        let me = self.root;
        match r {
            ToolRow::Heading(label) => {
                // UI_LAB_Panel's "Settings" header (TextBlock_73) over a group of rows
                let mut style = node_props(cx, "UI_LAB_Panel", "TextBlock_73");
                style.insert("Text".into(), label.text());
                let first = cx.view.tree().widget(pb).children.is_empty();
                let w = plain(
                    cx,
                    "TextBlock",
                    "Heading",
                    style,
                    json!({ "Padding": { "Top": if first { 0.0 } else { 18.0 }, "Bottom": 6 } }),
                    "VerticalBoxSlot",
                );
                cx.view.tree_mut().add(pb, w);
                Bound { w, kind: BoundKind::None }
            }
            ToolRow::Fighter => self.fighter_card(cx, pb, pi, ri),
            ToolRow::Toggle { id, label, swatch } => {
                let w = MenuView::create(cx, "UI_ToggleSwitchRework");
                cx.view.tree_mut().set_prop(w, "LabelText", label.text());
                cx.view.tree_mut().set_prop(w, "HideTopDiv", Value::Bool(true));
                let on = self.state.toggles.get(id).is_some_and(|t| t.0);
                cx.visit::<ToggleSwitch, _>(w, |t, cx| {
                    t.set_on(cx, on);
                    t.on_toggle_changed.add(Handler { owner: me, id: hid(H_TOGGLE, pi, ri, 0) });
                });
                let tree = cx.view.tree_mut();
                // (one line between two rows: the row's bottom one)
                let div = tree.find_in(w, "DivTop");
                tree.widget_mut(div).vis = Vis::Hidden;
                let sb = tree.find_in(w, "SizeBox_1");
                let x = tree.widget_mut(sb);
                x.w_override = WIDTH;
                x.h_override = ROW;
                // the check box's hit area (UI_Interactor: the 64 box and 25 round it, for the build's rows of 120)
                // kept inside the row: one overlapping the next row's makes the pad's step down pass over that row
                let hit = (ROW - 64.0) / 2.0;
                let i = tree.find_in(w, "UI_Interactor");
                tree.widget_mut(i).pad = Margin { l: -25.0, t: -hit, r: -25.0, b: -hit };
                self.interactors.push(i);
                if let Some(c) = swatch {
                    // a chip in the colour before the label, in the shape of the row's check box (CheckmarkFill)
                    let mut fill = node_props(cx, "UI_ToggleSwitchRework", "CheckmarkFill");
                    fill.insert("Color".into(), rgba_value(linear(*c)));
                    fill.insert("SizeOverride".into(), json!({ "X": 26, "Y": 26 }));
                    fill.insert("RenderOpacity".into(), json!(1));
                    let chip = plain(
                        cx,
                        "WBP_Quadrilateral",
                        "Swatch",
                        fill,
                        json!({ "Padding": { "Right": 16 }, "VerticalAlignment": "VAlign_Center" }),
                        "HorizontalBoxSlot",
                    );
                    let line = cx.view.tree().find_in(w, "HorizontalBox_1");
                    cx.view.tree_mut().insert(line, chip, Some(0));
                }
                cx.view.tree_mut().add(pb, w);
                Bound { w, kind: BoundKind::Toggle { id: *id, w, i } }
            }
            ToolRow::Stepper { id, label, min, max, step, text } => {
                let w = MenuView::create(cx, "UI_PlusMinusHorizontal");
                let tree = cx.view.tree_mut();
                tree.set_prop(w, "Label", invariant(label));
                tree.set_prop(w, "HideTopDiv", Value::Bool(true));
                tree.set_prop(w, "Minimum", json!(min));
                tree.set_prop(w, "Maximum", json!(max));
                tree.set_prop(w, "Increment", json!(step));
                let sb = tree.find_in(w, "SizeBox_2");
                tree.widget_mut(sb).w_override = 300.0;
                let value = tree.find_in(w, "ValueText");
                let (minus, plus) = (tree.find_in(w, "ButtonMinus"), tree.find_in(w, "ButtonPlus"));
                cx.visit::<PlusMinusHorizontal, _>(w, |p, _| {
                    p.on_value_changed.add(Handler { owner: me, id: hid(H_STEP, pi, ri, 0) });
                });
                for b in [minus, plus] {
                    let i = ButtonSecondary::interactor(cx, b);
                    self.interactors.push(i);
                }
                cx.view.tree_mut().widget_mut(w).pad = Margin { l: 0.0, t: 4.0, r: 0.0, b: 4.0 };
                cx.view.tree_mut().add(pb, w);
                Bound { w, kind: BoundKind::Stepper { id: *id, w, value, minus, plus, max: *max, text: *text } }
            }
            ToolRow::Buttons(buttons) => {
                let line = plain(
                    cx,
                    "HorizontalBox",
                    "Buttons",
                    serde_json::Map::new(),
                    json!({ "Padding": { "Top": 10, "Bottom": 6 } }),
                    "VerticalBoxSlot",
                );
                let n = buttons.len().max(1) as f64;
                for (k, (_, label)) in buttons.iter().enumerate() {
                    let b =
                        self.button(cx, label, ((WIDTH - (n - 1.0) * 8.0) / n).floor(), 64.0, hid(H_BUTTON, pi, ri, k));
                    let x = cx.view.tree_mut().widget_mut(b);
                    x.pad = Margin { l: if k > 0 { 8.0 } else { 0.0 }, t: 0.0, r: 0.0, b: 0.0 };
                    x.fill = 1.0;
                    cx.view.tree_mut().add(line, b);
                }
                cx.view.tree_mut().add(pb, line);
                Bound { w: line, kind: BoundKind::Buttons }
            }
        }
    }

    /// The chosen fighter's card: UI_LAB_PlayerPanel_Character (shown, not a button) beside UI_LAB_PlayerPanel's
    /// team bar (teamContainer: teamHeaderBG in the team's colour, the two arrows), whose label is the fighter's
    /// place.
    fn fighter_card(&mut self, cx: &mut Cx<'_>, pb: WidgetId, pi: usize, ri: usize) -> Bound {
        let me = self.root;
        let line = plain(
            cx,
            "HorizontalBox",
            "FighterCard",
            serde_json::Map::new(),
            json!({ "Padding": { "Bottom": 10 } }),
            "VerticalBoxSlot",
        );
        cx.view.tree_mut().add(pb, line);
        let card_box = plain(
            cx,
            "SizeBox",
            "CardSize",
            obj(json!({
                "WidthOverride": CARD_W,
                "HeightOverride": CARD_H,
                "bOverride_WidthOverride": true,
                "bOverride_HeightOverride": true
            })),
            Value::Null,
            "HorizontalBoxSlot",
        );
        cx.view.tree_mut().add(line, card_box);
        let cw = MenuView::create(cx, "UI_LAB_PlayerPanel_Character");
        {
            let w = cx.view.tree_mut().widget_mut(cw);
            w.h_align = HAlign::Fill;
            w.v_align = VAlign::Fill;
        }
        cx.view.tree_mut().add(card_box, cw);
        let button = cx.view.tree().find_in(cw, "buttonChangeCharacter");
        set_interactable(cx, button, false);
        let tree = cx.view.tree_mut();
        let edit = tree.find_in(cw, "EditIcon");
        tree.widget_mut(edit).vis = Vis::Collapsed;
        // (its name is shown beside it: the card's own sits 120 above its foot, over the room of
        // UI_LAB_PlayerPanel's perks)
        let names = tree.find_in(cw, "VerticalBox_0");
        tree.widget_mut(names).vis = Vis::Collapsed;

        let side = plain(
            cx,
            "VerticalBox",
            "CardSide",
            serde_json::Map::new(),
            json!({ "Padding": { "Left": 16 } }),
            "HorizontalBoxSlot",
        );
        cx.view.tree_mut().widget_mut(side).fill = 1.0;
        cx.view.tree_mut().add(line, side);
        let props = |cx: &Cx<'_>, n: &str| node_props(cx, "UI_LAB_PlayerPanel", n);
        let p = props(cx, "teamContainer");
        let bar = plain(cx, "SizeBox", "teamContainer", p, Value::Null, "VerticalBoxSlot");
        cx.view.tree_mut().add(side, bar);
        let over = plain(cx, "Overlay", "Overlay_128", serde_json::Map::new(), Value::Null, "SizeBoxSlot");
        cx.view.tree_mut().add(bar, over);
        let p = props(cx, "teamHeaderBG");
        let bg = plain(
            cx,
            "Image",
            "teamHeaderBG",
            p,
            json!({ "HorizontalAlignment": "HAlign_Fill", "VerticalAlignment": "VAlign_Fill" }),
            "OverlaySlot",
        );
        cx.view.tree_mut().add(over, bg);
        let arrows = plain(
            cx,
            "HorizontalBox",
            "HorizontalBox_226",
            serde_json::Map::new(),
            json!({ "HorizontalAlignment": "HAlign_Fill", "VerticalAlignment": "VAlign_Center" }),
            "OverlaySlot",
        );
        cx.view.tree_mut().add(over, arrows);
        let arrow = |cx: &mut Cx<'_>, this: &mut LabToolsPanel, name: &str, k: usize| -> WidgetId {
            let mut p = props(cx, name);
            p.remove("Tooltip");
            p.remove("RenderTransform");
            let w = MenuView::create(cx, "UI_IconButton");
            for (key, v) in p {
                cx.view.tree_mut().set_prop(w, &key, v);
            }
            let x = cx.view.tree_mut().widget_mut(w);
            x.fill = 1.0;
            x.h_align = if k == 0 { HAlign::Left } else { HAlign::Right };
            if k == 0 {
                x.transform.scale[0] = -1.0;
            }
            cx.visit::<crate::menu::widgets::controls::buttons::IconButton, _>(w, |b, _| {
                b.on_click.add(Handler { owner: me, id: hid(H_ARROW, pi, ri, k) });
            });
            let i = cx.view.tree().find_in(w, "UI_Interactor");
            this.interactors.push(i);
            w
        };
        let left = arrow(cx, self, "buttonTeamDecrement", 0);
        cx.view.tree_mut().add(arrows, left);
        let p = props(cx, "teamLabel");
        let label = plain(
            cx,
            "TextBlock",
            "teamLabel",
            p,
            json!({
                "Size": { "SizeRule": "ESlateSizeRule::Fill" },
                "HorizontalAlignment": "HAlign_Center",
                "VerticalAlignment": "VAlign_Center"
            }),
            "HorizontalBoxSlot",
        );
        cx.view.tree_mut().add(arrows, label);
        let right = arrow(cx, self, "buttonTeamIncrement", 1);
        cx.view.tree_mut().add(arrows, right);
        // the fighter's name under the bar, in the card's name style
        let mut style = node_props(cx, "UI_LAB_PlayerPanel_Character", "CharacterName");
        style.insert("Justification".into(), json!("ETextJustify::Left"));
        let name = plain(cx, "TextBlock", "FighterName", style, json!({ "Padding": { "Top": 14 } }), "VerticalBoxSlot");
        cx.view.tree_mut().widget_mut(name).auto_wrap = true;
        cx.view.tree_mut().add(side, name);
        Bound { w: line, kind: BoundKind::Fighter(Card { card: cw, bg, label, name, arrows: [left, right] }) }
    }

    /// Row `k` of page `page` takes the state.
    fn sync(&mut self, cx: &mut Cx<'_>, page: usize, k: usize) {
        let Some(b) = self.bound.get(page).and_then(|r| r.get(k)) else { return };
        match b.kind {
            BoundKind::None | BoundKind::Buttons => {}
            BoundKind::Toggle { id, w, i } => {
                let (on, enabled) = self.state.toggles.get(&id).copied().unwrap_or((false, true));
                cx.visit::<ToggleSwitch, _>(w, |t, cx| {
                    if t.is_on != on {
                        t.set_on(cx, on);
                    }
                });
                if cx.view.manager.interactor(i).is_some_and(|s| s.is_enabled() != enabled) {
                    set_interactable(cx, i, enabled);
                }
            }
            BoundKind::Stepper { id, w, value, minus, plus, max, text } => {
                let v = self.state.values.get(&id).copied().unwrap_or(0.0);
                cx.view.tree_mut().set_prop(w, "Maximum", json!(max));
                let lo = cx.view.tree().props(w).get("Minimum").and_then(Value::as_f64).unwrap_or(0.0);
                cx.visit::<PlusMinusHorizontal, _>(w, |p, _| p.value = v);
                // (UpdateAppearance's interactable buttons, then the row's own text)
                cx.view.tree_mut().widget_mut(value).text = text(v);
                for (b, up) in [(plus, v < max), (minus, v > lo)] {
                    let i = ButtonSecondary::interactor(cx, b);
                    set_interactable(cx, i, up);
                    cx.view.tree_mut().widget_mut(b).opacity = if up { 1.0 } else { 0.75 };
                }
            }
            BoundKind::Fighter(c) => self.sync_card(cx, c),
        }
    }

    fn sync_card(&mut self, cx: &mut Cx<'_>, c: Card) {
        let n = self.state.fighters.len();
        let i = self.state.sel.min(n.saturating_sub(1));
        let f = self.state.fighters.get(i).cloned().flatten();
        cx.view.tree_mut().widget_mut(c.label).text = format!("P{}", i + 1);
        let team = f.as_ref().map_or(0, |f| f.team);
        let color = palette_entry(cx, TEAM_PALETTE.get(team.max(0) as usize).copied().unwrap_or(0));
        cx.view.tree_mut().widget_mut(c.bg).color = color;
        let (character, skin) = f.as_ref().map_or((None, String::new()), |f| (f.character.clone(), f.skin.clone()));
        let random = character.is_none();
        cx.visit::<LabPlayerPanelCharacter, _>(c.card, |card, cx| {
            card.set_team_color(cx, team);
            card.set_character(cx, character, &skin, random);
        });
        let shown = cx.view.tree().find_in(c.card, "CharacterName");
        let t = cx.view.tree().widget(shown).text.clone();
        cx.view.tree_mut().widget_mut(c.name).text = t;
        for b in c.arrows {
            let iw = cx.view.tree().find_in(b, "UI_Interactor");
            set_interactable(cx, iw, n > 1);
            cx.view.tree_mut().widget_mut(b).vis = if n > 1 { Vis::Visible } else { Vis::Hidden };
        }
        // the fighters' portraits, held while the panel is up
        let icons: Vec<Option<String>> = self
            .state
            .fighters
            .iter()
            .map(|x| x.as_ref().and_then(|x| skin_icon(cx.view, x.character.as_deref(), &x.skin)))
            .collect();
        let key = icons.iter().map(|i| i.as_deref().unwrap_or("")).collect::<Vec<_>>().join("|");
        if self.held.as_deref() != Some(&key) {
            self.held = Some(key);
            cx.view.hold_pictures(PICTURES_OWNER, icons.iter().flatten().map(String::as_str));
        }
    }

    /// The row of control `id` (its page, its row).
    fn card_of(&self, page: usize, row: usize) -> Option<Card> {
        match self.bound.get(page)?.get(row)?.kind {
            BoundKind::Fighter(c) => Some(c),
            _ => None,
        }
    }
}

impl Controller for LabToolsPanel {
    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        self.animate(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let (kind, page, row, k) = split(id);
        match id {
            H_FOLD => {
                let folded = !self.state.folded;
                self.cmds.push(ToolCmd::Fold(folded));
                return;
            }
            H_SWITCH_PAGE => {
                let n = self.pages.len();
                let dir = arg.downcast_ref::<(i32, i32)>().map_or(0, |a| a.1);
                if n == 0 {
                    return;
                }
                let at = (self.state.page as i64 + i64::from(dir)).rem_euclid(n as i64) as usize;
                self.state.page = at;
                self.cmds.push(ToolCmd::Page(at));
                self.refresh(cx);
                self.focus_page(cx);
                return;
            }
            _ => {}
        }
        match kind {
            H_TAB => {
                self.state.page = k;
                self.cmds.push(ToolCmd::Page(k));
                self.refresh(cx);
            }
            H_TAB_UNFOCUS => {
                if self.tabs_at == Some(k)
                    && let Some(&b) = self.tabs.get(k)
                {
                    self.mark_tab(cx, b, true);
                }
            }
            H_TOGGLE => {
                let Some(Bound { kind: BoundKind::Toggle { id, .. }, .. }) =
                    self.bound.get(page).and_then(|r| r.get(row))
                else {
                    return;
                };
                let id = *id;
                let Some(&on) = arg.downcast_ref::<bool>() else { return };
                self.state.toggles.entry(id).or_insert((on, true)).0 = on;
                self.cmds.push(ToolCmd::Toggle(id, on));
            }
            H_STEP => {
                let Some(Bound { kind: BoundKind::Stepper { id, .. }, .. }) =
                    self.bound.get(page).and_then(|r| r.get(row))
                else {
                    return;
                };
                let id = *id;
                let v = arg.downcast_ref::<(f64, f64)>().map_or(0.0, |a| a.0);
                self.state.values.insert(id, v);
                self.cmds.push(ToolCmd::Value(id, v));
                self.sync(cx, page, row);
            }
            H_BUTTON => {
                let Some(ToolRow::Buttons(b)) = self.pages.get(page).and_then(|p| p.rows.get(row)) else { return };
                if let Some(&(id, _)) = b.get(k) {
                    self.cmds.push(ToolCmd::Click(id));
                }
            }
            H_ARROW => {
                let n = self.state.fighters.len();
                if n > 1 {
                    let dir: i64 = if k == 0 { -1 } else { 1 };
                    let sel = (self.state.sel as i64 + dir).rem_euclid(n as i64) as usize;
                    self.state.sel = sel;
                    self.cmds.push(ToolCmd::Fighter(sel));
                }
                if let Some(c) = self.card_of(page, row) {
                    self.sync_card(cx, c);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handler_id_splits_into_its_parts() {
        let id = hid(H_BUTTON, 2, 11, 1);
        assert_eq!(split(id), (H_BUTTON, 2, 11, 1));
        // (the two single ids are below every row id)
        assert!(H_FOLD < hid(H_TAB, 0, 0, 0) && H_SWITCH_PAGE < hid(H_TAB, 0, 0, 0));
    }

    #[test]
    fn a_hex_colour_is_made_linear() {
        let c = linear([255, 0, 128]);
        assert_eq!(c[0], 1.0);
        assert_eq!(c[1], 0.0);
        assert!((c[2] - 0.2158605).abs() < 1e-6);
        assert_eq!(c[3], 1.0);
    }
}
