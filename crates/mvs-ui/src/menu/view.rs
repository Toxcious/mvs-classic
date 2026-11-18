//! The menus' view: the viewport's widget tree, the user widgets' controllers,
//! the widget animations and delays, the input routed to the manager and the widgets, the layout and the paint.
//!
//! A widget is a `WidgetId` in the view's one tree; a user widget's controller is kept by its widget's id. What
//! makes, adds or removes a widget calls controllers (OnInitialized, Construct, Destruct) and so is an associated
//! function taking the context (`MenuView::create(cx, cls)`); the rest is a method.
//!
//! The scroll bars (scroll.rs) and the rich texts (rich_text.rs) are parts of their own the view drives through
//! `ViewBars` and `ViewRich`; the view starts with neither (`NoBars`, `NoRich`) until their track installs them.
//! Deviation: the background blur (blur.rs) is not driven yet: a BackgroundBlur paints nothing behind it, and the
//! animations' BlurStrength tracks are not applied.

use std::collections::VecDeque;
use std::sync::Arc;

use indexmap::IndexMap;

use super::controller::{Controller, Cx, Deferred, Handler, TextEdit};
use super::cursor::Cursor;
use super::manager::{ContextMenu, Dialog, DialogId, Manager, Modal, Notification, UI_GLOBAL_Z, Waiting};
use super::pictures::Pictures;
use super::registry;
use crate::bundle::{AnimData, Bundle, ClassInfo, FighterInfo, GlossaryRow, MapInfo, Rgba, RosterEntry};
use crate::element::{BoxEl, DrawEffects, Element, ElementList, PaintGeometry};
use crate::hud::dpi_scale;
use crate::slate::{
    self, Geometry, Margin, RenderTransform, Tree, Vis, WidgetId, hit_test, layout_widget, measure_widget, paint_widget,
};
use crate::text::{Fonts, GameText};

/// The context's widget of a call the view makes for no widget of its own (an input event, the frame).
pub const NO_WIDGET: WidgetId = WidgetId::NONE;

/// The screens the build has no class for (the port's own), each drawn on the tree of a build class it looks like:
/// CreateWidget of the name takes that class's tree and puts the name's controller on its root.
const TREE_OF: [(&str, &str); 2] =
    [("CustomGameBrowser", "WBP_CustomGame"), ("CustomGameSettings", "UI_QuickMatchSettings")];

/// A controller in the view: in place, or taken out for a call (with the calls deferred for it meanwhile).
enum Slot {
    Here(Box<dyn Controller>),
    Busy(VecDeque<Deferred>),
}

struct Ctl {
    slot: Slot,
    /// Construct ran and Destruct has not since (Controller.constructed).
    constructed: bool,
    /// Destruct ran and Construct has not run again since: its delays and animations are dropped, its hover is not
    /// heard (Controller.destroyed). A widget added again (Construct runs again) is live again.
    // Unverified: the delays a widget had pending at its Destruct stay dropped; whether the build's latent
    // actions of a removed widget still complete is not read.
    destroyed: bool,
}

/// The controllers by widget: a slot per widget index (the tree's ids are dense indices, a freed one taken again
/// under the next generation), looked up a few times per controller each frame. A slot answers only the id it was
/// put in under: a stale id (its widget destroyed) has no controller.
#[derive(Default)]
struct Ctls {
    slots: Vec<Option<(WidgetId, Ctl)>>,
    /// Counts the controllers put in (`MenuView::tick_order` walks the tree again when it moved).
    made: u64,
}

impl Ctls {
    fn get(&self, w: &WidgetId) -> Option<&Ctl> {
        self.slots.get(w.index())?.as_ref().filter(|(id, _)| id == w).map(|(_, c)| c)
    }

    fn get_mut(&mut self, w: &WidgetId) -> Option<&mut Ctl> {
        self.slots.get_mut(w.index())?.as_mut().filter(|(id, _)| id == w).map(|(_, c)| c)
    }

    /// The controller of `w` taken out for good (its widget is destroyed).
    fn remove(&mut self, w: WidgetId) -> Option<Ctl> {
        let s = self.slots.get_mut(w.index())?;
        if s.as_ref().is_some_and(|(id, _)| *id == w) { s.take().map(|(_, c)| c) } else { None }
    }

    fn contains_key(&self, w: &WidgetId) -> bool {
        self.get(w).is_some()
    }

    fn insert(&mut self, w: WidgetId, c: Ctl) {
        let i = w.index();
        if i >= self.slots.len() {
            self.slots.resize_with(i + 1, || None);
        }
        self.slots[i] = Some((w, c));
        self.made += 1;
    }
}

/// The controllers `MenuView::tick` runs, children first, and what they were collected at: the roots, the tree's
/// shape count and the controllers' count.
struct TickOrder {
    roots: Vec<WidgetId>,
    shape: u64,
    made: u64,
    ctls: Arc<[WidgetId]>,
}

/// A viewport layer: a root widget and its Z order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub w: WidgetId,
    pub z: i32,
}

/// A screen-space WidgetComponent: under the layers, at its desired
/// size (or `size`, the component's DrawSize when bDrawAtDesiredSize is off) around its point (pivot 0.5, 0.5) in
/// Slate units; None: not on screen. Its canvas slot's ZOrder is -distance (Slate.WorldWidgetZOrder 1): the nearer
/// one paints over the farther one, equal ones in the order they were added.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldWidget {
    pub w: WidgetId,
    pub at: Option<[f64; 2]>,
    pub size: Option<[f64; 2]>,
    /// The distance from the view origin to the component's world location (`place_world_widget`).
    pub distance: f64,
    /// When it was last shown, counted over the view's world widgets: a hidden component leaves the screen layer
    /// and is added again when shown (UWidgetComponent 4.26), so equal ZOrders paint in the order shown.
    pub shown: u64,
}

/// The shown world widgets in their paint order: by the canvas slot's ZOrder, -distance as its 32-bit integer (the
/// float truncated), ascending (the farther first), equal ones in the order they were shown (the layer's add order).
fn world_order(world: &[WorldWidget]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..world.len()).filter(|&i| world[i].at.is_some()).collect();
    // (`as` saturates where the engine's float to int32 would not need to: distances fit)
    order.sort_by_key(|&i| (-(world[i].distance as i32), world[i].shown));
    order
}

/// A pre-animated value of one widget property, saved for bRestoreState.
#[derive(Clone, Debug)]
enum Saved {
    Xf(RenderTransform),
    Vis(Vis),
    Opacity(f64),
    Color(Rgba),
    Shadow(Rgba),
    Offsets(Margin),
    Pad(Margin),
    Scalar(String, Option<f32>),
}

impl Saved {
    fn kind(&self) -> String {
        match self {
            Saved::Xf(_) => "xf".into(),
            Saved::Vis(_) => "vis".into(),
            Saved::Opacity(_) => "opacity".into(),
            Saved::Color(_) => "color".into(),
            Saved::Shadow(_) => "shadow".into(),
            Saved::Offsets(_) => "offsets".into(),
            Saved::Pad(_) => "pad".into(),
            Saved::Scalar(n, _) => format!("S:{n}"),
        }
    }
}

/// A playing widget animation.
#[derive(Clone, Debug)]
struct Playing {
    owner: WidgetId,
    class: String,
    name: String,
    end: f64,
    time: f64,
    dir: f64,
    speed: f64,
    /// NumLoopsToPlay (0 = forever), the passes done, PlayMode PingPong.
    loops: u32,
    done: u32,
    ping_pong: bool,
    /// bRestoreState and the values saved before the first write (None: none saved yet).
    restore: bool,
    saved: Option<Vec<(WidgetId, Saved)>>,
}

/// The EditableTextBox with the keyboard focus: the box, its text child, the controller it reports
/// to, the caret (chars).
#[derive(Clone, Copy, Debug)]
struct Edit {
    edit_box: WidgetId,
    text: WidgetId,
    owner: WidgetId,
    caret: usize,
}

/// The scroll boxes' bars of the layers, driven by the view.
pub trait ViewBars {
    /// The layers changed: the bars collected again.
    fn collect(&mut self, tree: &Tree, layers: &[Layer]);
    /// The frame's step (the thumbs' drag, the animated scrolls).
    fn tick(&mut self, tree: &mut Tree, dt: f64, mouse: Option<[f64; 2]>);
    /// The mouse is over a bar of layer `hit_layer` or above.
    fn hover(&mut self, tree: &Tree, mouse: [f64; 2], hit_layer: i32) -> bool;
    /// A thumb is dragged (the bar has the mouse).
    fn dragging(&self) -> bool;
    /// The left button went down: true when a bar took it.
    fn mouse_down(&mut self, tree: &mut Tree, mouse: Option<[f64; 2]>) -> bool;
    fn mouse_up(&mut self);
    /// The wheel over ScrollBox `sb`: false when it has nothing to scroll (an outer one takes it).
    fn wheel(&mut self, tree: &mut Tree, sb: WidgetId, notches: f64) -> bool;
    /// ScrollWidgetIntoView of `w` in `sb`, its centre to the view's centre.
    fn scroll_into_view_centre(&mut self, tree: &mut Tree, sb: WidgetId, w: WidgetId, animate: bool);
    /// Layer `i` was laid out: its bars follow (true: one changed).
    fn prepare(&mut self, tree: &mut Tree, i: usize) -> bool;
    /// Layer `i`'s bars painted after it.
    fn paint(&mut self, tree: &Tree, out: &mut ElementList, i: usize);
}

/// No scroll bars.
#[derive(Debug, Default)]
pub struct NoBars;

impl ViewBars for NoBars {
    fn collect(&mut self, _: &Tree, _: &[Layer]) {}
    fn tick(&mut self, _: &mut Tree, _: f64, _: Option<[f64; 2]>) {}
    fn hover(&mut self, _: &Tree, _: [f64; 2], _: i32) -> bool {
        false
    }
    fn dragging(&self) -> bool {
        false
    }
    fn mouse_down(&mut self, _: &mut Tree, _: Option<[f64; 2]>) -> bool {
        false
    }
    fn mouse_up(&mut self) {}
    fn wheel(&mut self, _: &mut Tree, _: WidgetId, _: f64) -> bool {
        false
    }
    fn scroll_into_view_centre(&mut self, _: &mut Tree, _: WidgetId, _: WidgetId, _: bool) {}
    fn prepare(&mut self, _: &mut Tree, _: usize) -> bool {
        false
    }
    fn paint(&mut self, _: &Tree, _: &mut ElementList, _: usize) {}
}

/// The layers' RichTextBlocks: their lines after the ticks that set their texts.
pub trait ViewRich {
    fn collect(&mut self, tree: &Tree, layers: &[Layer]);
    fn tick(&mut self, tree: &mut Tree, fonts: &mut Fonts);
}

/// No rich texts.
#[derive(Debug, Default)]
pub struct NoRich;

impl ViewRich for NoRich {
    fn collect(&mut self, _: &Tree, _: &[Layer]) {}
    fn tick(&mut self, _: &mut Tree, _: &mut Fonts) {}
}

/// The menus' view.
pub struct MenuView {
    pub bundle: Arc<Bundle>,
    pub text: Arc<GameText>,
    pub manager: Manager,
    /// What the menus keep across screens (`Shell` hands it in and takes it back, session.rs).
    pub session: super::session::Session,
    tree: Tree,
    ctls: Ctls,
    /// `tick`'s controllers as last collected (the tree's walk is done again only when its shape or the controllers
    /// changed).
    tick_order: Option<TickOrder>,
    /// AddToViewport: a higher Z paints over a lower one, among equals the later over the earlier.
    layers: Vec<Layer>,
    pub world: Vec<WorldWidget>,
    /// The world widgets' opacity: 1 less the level's post fade to black, set by the host each frame.
    /// Deviation: the build's Space Screen components draw over the camera fade, so the pre-match nametags stay
    /// after the scene has faded; the port fades them with the scene.
    pub world_opacity: f64,
    /// World-space WidgetComponents drawn at the desired size into a texture of their own.
    targets: Vec<WidgetId>,
    /// The client area, device pixels.
    size: [u32; 2],
    /// The DPI scale of the size (the build's DPI curve).
    pub dpi: f64,
    /// The program's clock at the last frame, s.
    time: f64,
    /// The clock of the last tick (None before the first).
    last: Option<f64>,
    /// Player 0's pointer in device pixels (UI_Cursor's Get Position): the mouse's or the cursor's stick
    /// movement's. None while nothing placed it: no hover then.
    pub mouse: Option<[f64; 2]>,
    /// The cursor moved the pointer since the screen last looked (the screen carries it to the window's pointer).
    pub pointer_written: bool,
    /// UI_Cursor of player 0 (enable_cursor); None in a view that takes no input of its own.
    pub cursor: Option<WidgetId>,
    /// PC0.bShowMouseCursor as UI_Manager.ShowMouseCursor last set it.
    pub show_mouse_cursor: bool,
    /// A widget under the pointer names a cursor of its own (its Cursor property).
    pub over_cursor_widget: bool,
    /// The key going through `key_down` now is an auto-repeat (a keydown's `repeat`): read by a listener that ignores
    /// repeats.
    pub key_repeat: bool,
    latent: Vec<(f64, Handler)>,
    playing: Vec<Playing>,
    hovered: Vec<WidgetId>,
    hovered_buttons: Vec<WidgetId>,
    hit_layer: i32,
    /// SetUserFocus: the binding row that sees key and mouse events first while it listens.
    capture: Option<WidgetId>,
    edit: Option<Edit>,
    /// The pictures the view's widgets hold ahead (pictures.rs).
    pub pictures: Pictures,
    bars: Box<dyn ViewBars>,
    rich: Box<dyn ViewRich>,
    /// The roots the bars and the rich texts were collected from (None: collect again).
    bar_roots: Option<Vec<WidgetId>>,
    /// The tree's shape count `bar_roots` was collected at (barVersion).
    bar_shape: u64,
    /// The roster and the fighters the port can draw (set_drawable), made once per data.
    roster: Vec<RosterEntry>,
    fighters: IndexMap<String, FighterInfo>,
    /// The calls into the level behind the menus, in order, until the program takes them.
    scene_cmds: Vec<super::scene::SceneCmd>,
    online_cmds: Vec<super::online::OnlineCmd>,
    /// The widgets dismissed for good (`discard`), freed at the end of the frame's tick unless added again by then.
    doomed: Vec<WidgetId>,
}

impl std::fmt::Debug for MenuView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuView").field("size", &self.size).field("layers", &self.layers).finish()
    }
}

impl MenuView {
    /// An empty viewport of `size` device pixels on the menu bundle.
    pub fn new(bundle: Arc<Bundle>, text: Arc<GameText>, size: [u32; 2]) -> MenuView {
        let tree = Tree::empty(&bundle);
        let (roster, fighters) = match &bundle.menu {
            Some(m) => (m.roster.clone(), m.fighters.clone()),
            None => (Vec::new(), IndexMap::new()),
        };
        MenuView {
            bundle,
            text,
            manager: Manager::new(),
            session: Default::default(),
            tree,
            ctls: Ctls::default(),
            tick_order: None,
            layers: Vec::new(),
            world: Vec::new(),
            world_opacity: 1.0,
            targets: Vec::new(),
            size,
            dpi: dpi_scale(f64::from(size[0]), f64::from(size[1])),
            time: 0.0,
            last: None,
            mouse: None,
            pointer_written: false,
            cursor: None,
            show_mouse_cursor: true,
            over_cursor_widget: false,
            key_repeat: false,
            latent: Vec::new(),
            playing: Vec::new(),
            hovered: Vec::new(),
            hovered_buttons: Vec::new(),
            hit_layer: -1,
            capture: None,
            edit: None,
            pictures: Pictures::default(),
            bars: Box::new(NoBars),
            rich: Box::new(NoRich),
            bar_roots: None,
            bar_shape: 0,
            roster,
            fighters,
            scene_cmds: Vec::new(),
            online_cmds: Vec::new(),
            doomed: Vec::new(),
        }
    }

    /// The calls into the level behind the menus since the last take, in order (the program hands each to the
    /// renderer's `menu_*` call it names).
    pub fn take_scene_cmds(&mut self) -> Vec<super::scene::SceneCmd> {
        std::mem::take(&mut self.scene_cmds)
    }

    /// A call into the level behind the menus (`Cx::scene`).
    pub(crate) fn push_scene_cmd(&mut self, cmd: super::scene::SceneCmd) {
        self.scene_cmds.push(cmd);
    }

    /// The commands to online play since the last take, in order (the program's online client runs them).
    pub fn take_online_cmds(&mut self) -> Vec<super::online::OnlineCmd> {
        std::mem::take(&mut self.online_cmds)
    }

    /// A command to online play (`Cx::online`).
    pub(crate) fn push_online_cmd(&mut self, cmd: super::online::OnlineCmd) {
        self.online_cmds.push(cmd);
    }

    /// The scroll bars' and the rich texts' parts (their track's).
    pub fn set_parts(&mut self, bars: Box<dyn ViewBars>, rich: Box<dyn ViewRich>) {
        self.bars = bars;
        self.rich = rich;
        self.bar_roots = None;
    }

    // ---------- the clock and the size ----------

    /// The program's clock at this frame, s.
    pub fn now(&self) -> f64 {
        self.time
    }

    /// The frame's clock (the screen's, before the input of the frame).
    pub fn set_now(&mut self, now: f64) {
        self.time = now;
    }

    /// The client area, device pixels.
    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    /// The viewport in Slate units (the client area over the DPI scale).
    pub fn viewport_size(&self) -> [f64; 2] {
        [f64::from(self.size[0]) / self.dpi, f64::from(self.size[1]) / self.dpi]
    }

    /// The viewport in device pixels (the client area).
    pub fn viewport_pixels(&self) -> [f64; 2] {
        [f64::from(self.size[0]), f64::from(self.size[1])]
    }

    /// The client area changed.
    pub fn resize(&mut self, size: [u32; 2]) {
        self.size = size;
        self.dpi = dpi_scale(f64::from(size[0]), f64::from(size[1]));
    }

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn tree_mut(&mut self) -> &mut Tree {
        &mut self.tree
    }

    /// The viewport's layers in paint order.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    // ---------- the menu data ----------

    /// The roster and the fighters kept to what `drawable` says the port can draw: a
    /// tile whose fighter it cannot draw is a tile without a fighter, and that fighter is not in the list.
    pub fn set_drawable(&mut self, drawable: &dyn Fn(&str) -> bool) {
        let Some(m) = &self.bundle.menu else { return };
        self.roster = m
            .roster
            .iter()
            .map(|r| {
                let mut r = r.clone();
                if r.fighter.as_deref().is_some_and(|f| !drawable(f)) {
                    r.fighter = None;
                }
                r
            })
            .collect();
        self.fighters = m.fighters.iter().filter(|(k, _)| drawable(k)).map(|(k, v)| (k.clone(), v.clone())).collect();
    }

    pub fn roster(&self) -> &[RosterEntry] {
        &self.roster
    }

    pub fn fighters(&self) -> &IndexMap<String, FighterInfo> {
        &self.fighters
    }

    /// The MapData `name` with its name (an own entry only).
    pub fn map(&self, name: &str) -> Option<MapInfo> {
        let m = self.bundle.menu.as_ref()?.maps.get(name)?;
        let level = m.levels.first().map_or("", String::as_str);
        let stage = self.bundle.stage_of_level(level).map(str::to_owned);
        Some(MapInfo { name: name.to_string(), data: m.clone(), stage })
    }

    /// The Lab's first map (the training config's SelectedMap).
    pub fn lab_map(&self) -> Option<MapInfo> {
        self.map(self.bundle.menu.as_ref()?.lab_map.as_deref()?)
    }

    /// UI_Functions GetSoftMaps(Rotations): the maps of each rotation in turn, each once. Unverified: native
    /// GetMapsForRotation is read as "the MapData assets whose MapRotations hold it", in asset name order.
    pub fn soft_maps(&self, rotations: &[&str]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let Some(m) = &self.bundle.menu else { return out };
        for r in rotations {
            for (n, d) in &m.maps {
                if d.rotations.iter().any(|x| x == r) && !out.contains(n) {
                    out.push(n.clone());
                }
            }
        }
        out
    }

    pub fn offline_maps(&self) -> &[String] {
        self.bundle.menu.as_ref().map_or(&[], |m| &m.offline_maps)
    }

    pub fn offline_maps_local(&self) -> &[String] {
        self.bundle.menu.as_ref().map_or(&[], |m| &m.offline_maps_local)
    }

    pub fn class_info(&self, class: &str) -> Option<&ClassInfo> {
        self.bundle.menu.as_ref()?.class_info.get(class)
    }

    /// The glossary table's rows (none: a table the bundle lacks, warned).
    pub fn glossary(&self, table: &str) -> &[GlossaryRow] {
        match self.bundle.menu.as_ref().and_then(|m| m.glossary.get(table)) {
            Some(r) => r,
            None => {
                eprintln!("menu: glossary table {table} is not in the bundle");
                &[]
            }
        }
    }

    /// The Game target's string `key` the graphs set at run time ('' when neither the bundle nor the text has it).
    pub fn game_text(&self, key: &str) -> &str {
        if let Some(s) = self.bundle.texts.get(key) {
            return s;
        }
        match self.text.get("", key) {
            Some(s) => s,
            None => {
                eprintln!("menu: text {key} is not in the bundle");
                ""
            }
        }
    }

    /// DataTable InputButtonIconsTable: a gamepad key's glyph (its texture's store entry).
    pub fn input_icon(&self, key: &str) -> Option<&str> {
        self.bundle.input_icon(key)
    }

    /// CentralPalette's UI column by row name.
    pub fn palette(&self) -> &IndexMap<String, Rgba> {
        &self.bundle.palette
    }

    /// UI_Functions GetPaletteChoices: the CentralPalette rows a colour picker offers.
    pub fn palette_choices(&self) -> &[String] {
        self.bundle.menu.as_ref().map_or(&[], |m| &m.palette_choices)
    }

    // ---------- the pictures ----------

    /// The pictures `names` held for `owner` (a screen one step away draws them).
    pub fn hold_pictures<'a>(&mut self, owner: &str, names: impl IntoIterator<Item = &'a str>) {
        self.pictures.hold(owner, names);
    }

    pub fn release_pictures(&mut self, owner: &str) {
        self.pictures.release(owner);
    }

    /// The view's screen is gone: what its widgets held lets go.
    pub fn release_held(&mut self) {
        self.pictures.release_all();
    }

    /// A soft texture asked for (SetBrushFromSoftTexture: the renderer loads it on its next draw).
    pub fn load_texture(&mut self, name: &str) {
        self.pictures.want(name);
    }

    // ---------- the controllers ----------

    /// The controller of user widget `w`, as its own type (None: none, another type, or busy).
    pub fn controller<T: Controller>(&mut self, w: WidgetId) -> Option<&mut T> {
        let c: &mut dyn std::any::Any = match &mut self.ctls.get_mut(&w)?.slot {
            Slot::Here(c) => c.as_mut(),
            Slot::Busy(_) => return None,
        };
        c.downcast_mut::<T>()
    }

    /// The controller of `w` as its own type, to read (None as `controller`).
    pub fn controller_ref<T: Controller>(&self, w: WidgetId) -> Option<&T> {
        let c: &dyn std::any::Any = match &self.ctls.get(&w)?.slot {
            Slot::Here(c) => c.as_ref(),
            Slot::Busy(_) => return None,
        };
        c.downcast_ref::<T>()
    }

    fn here(&mut self, w: WidgetId) -> Option<&mut dyn Controller> {
        match &mut self.ctls.get_mut(&w)?.slot {
            Slot::Here(c) => Some(c.as_mut()),
            Slot::Busy(_) => None,
        }
    }

    /// The controller of `w` taken out for a call (put back with `put_controller`); None when it has none or it is
    /// out already.
    pub fn take_controller(&mut self, w: WidgetId) -> Option<Box<dyn Controller>> {
        let s = self.ctls.get_mut(&w)?;
        match std::mem::replace(&mut s.slot, Slot::Busy(VecDeque::new())) {
            Slot::Here(c) => Some(c),
            busy => {
                s.slot = busy;
                None
            }
        }
    }

    pub fn put_controller(&mut self, w: WidgetId, c: Box<dyn Controller>) {
        match self.ctls.get_mut(&w) {
            Some(s) => s.slot = Slot::Here(c),
            // (a widget destroyed meanwhile: its controller goes with it)
            None if !self.tree.alive(w) => {}
            None => {
                self.ctls.insert(w, Ctl { slot: Slot::Here(c), constructed: false, destroyed: false });
            }
        }
    }

    /// The controller of `w` is out for a call (its method is running).
    pub fn is_busy(&self, w: WidgetId) -> bool {
        self.ctls.get(&w).is_some_and(|s| matches!(s.slot, Slot::Busy(_)))
    }

    pub(crate) fn defer(&mut self, w: WidgetId, f: Deferred) {
        if let Some(Ctl { slot: Slot::Busy(q), .. }) = self.ctls.get_mut(&w) {
            q.push_back(f);
        }
    }

    pub(crate) fn take_deferred(&mut self, w: WidgetId) -> Option<Deferred> {
        match self.ctls.get_mut(&w) {
            Some(Ctl { slot: Slot::Busy(q), .. }) => q.pop_front(),
            _ => None,
        }
    }

    /// `w` has a controller (made by CreateWidget or `put_controller`).
    pub fn has_controller(&self, w: WidgetId) -> bool {
        self.ctls.contains_key(&w)
    }

    /// Construct ran on `w`'s controller and Destruct has not since.
    pub fn is_constructed(&self, w: WidgetId) -> bool {
        self.ctls.get(&w).is_some_and(|s| s.constructed)
    }

    /// `w`'s controller was destructed (or `w` has none).
    pub fn is_destroyed(&self, w: WidgetId) -> bool {
        self.ctls.get(&w).is_none_or(|s| s.destroyed)
    }

    /// The modal controller of `w`.
    pub fn modal(&mut self, w: WidgetId) -> Option<&mut dyn Modal> {
        self.here(w)?.as_modal()
    }

    pub fn dialog(&mut self, d: DialogId) -> Option<&mut dyn Dialog> {
        self.here(d.0)?.as_dialog()
    }

    pub fn notification(&mut self, w: WidgetId) -> Option<&mut dyn Notification> {
        self.here(w)?.as_notification()
    }

    pub fn waiting(&mut self, w: WidgetId) -> Option<&mut dyn Waiting> {
        self.here(w)?.as_waiting()
    }

    pub fn context_menu(&mut self, w: WidgetId) -> Option<&mut dyn ContextMenu> {
        self.here(w)?.as_context_menu()
    }

    // ---------- widgets ----------

    /// CreateWidget: the tree with a controller on every widget of a class that has one (`registry::make`), in no
    /// parent; OnInitialized runs children first.
    pub fn create(cx: &mut Cx<'_>, cls: &str) -> WidgetId {
        let tree_cls = TREE_OF.iter().find(|(k, _)| *k == cls).map_or(cls, |(_, t)| *t);
        let bundle = cx.view.bundle.clone();
        assert!(bundle.widgets.contains_key(tree_cls), "menu: no widget class {tree_cls}");
        let w = cx.view.tree.create(&bundle, tree_cls, tree_cls);
        let all = cx.view.tree.post_order(w);
        let mut made = Vec::new();
        for &c in &all {
            let k = if c == w { Some(cls) } else { cx.view.tree.user_class(c) };
            let Some(make) = k.and_then(registry::make) else { continue };
            let ctl = make(&mut cx.with(c));
            cx.view.ctls.insert(c, Ctl { slot: Slot::Here(ctl), constructed: false, destroyed: false });
            made.push(c);
        }
        for c in made {
            cx.run(c, Box::new(|ctl, cx| ctl.on_initialized(cx)));
        }
        w
    }

    /// A lazy nested instance built now. The Rust tree builds every nested instance at
    /// CreateWidget: nothing to do.
    pub fn expand(_cx: &mut Cx<'_>, _w: WidgetId) {}

    /// PreConstruct and Construct per widget not yet constructed, children first.
    fn construct_tree(cx: &mut Cx<'_>, w: WidgetId) {
        for c in cx.view.tree.post_order(w) {
            let Some(s) = cx.view.ctls.get_mut(&c) else { continue };
            if s.constructed {
                continue;
            }
            // (live again before its PreConstruct: the delays and animations Construct starts are kept)
            s.destroyed = false;
            cx.run(
                c,
                Box::new(move |ctl, cx| {
                    ctl.pre_construct(cx);
                    ctl.construct(cx);
                    if let Some(s) = cx.view.ctls.get_mut(&c) {
                        s.constructed = true;
                    }
                }),
            );
        }
    }

    /// Destruct per constructed widget, parents first; the keyboard focus and the hover leave them.
    fn destruct_tree(cx: &mut Cx<'_>, w: WidgetId) {
        let mut all = cx.view.tree.post_order(w);
        all.reverse();
        for c in all {
            if cx.view.edit.is_some_and(|e| e.edit_box == c) {
                MenuView::clear_keyboard_focus(cx, 3, false);
            }
            let Some(s) = cx.view.ctls.get_mut(&c) else { continue };
            let was = s.constructed;
            s.constructed = false;
            s.destroyed = true;
            cx.view.hovered.retain(|&h| h != c);
            if was {
                cx.run(c, Box::new(|ctl, cx| ctl.destruct(cx)));
            }
        }
        let view = &mut *cx.view;
        let ctls = &view.ctls;
        view.playing.retain(|p| ctls.get(&p.owner).is_none_or(|s| !s.destroyed));
    }

    /// AddToViewport at Z order `z` (Construct runs).
    pub fn add_to_viewport(cx: &mut Cx<'_>, w: WidgetId, z: i32) {
        if cx.view.in_viewport(w) {
            return;
        }
        let layers = &mut cx.view.layers;
        let mut at = layers.len();
        while at > 0 && layers[at - 1].z > z {
            at -= 1;
        }
        layers.insert(at, Layer { w, z });
        MenuView::construct_tree(cx, w);
    }

    /// AddWidgetToView: a child of a canvas over the viewport filling it (`auto_size`: its desired size), over the
    /// viewport's own widgets (UI_GLOBAL_Z + z).
    pub fn add_to_view(cx: &mut Cx<'_>, w: WidgetId, auto_size: bool, z: i32) {
        if cx.view.in_viewport(w) {
            return;
        }
        let canvas = cx.view.root_canvas(w);
        let x = cx.view.tree.widget_mut(w);
        x.anchors = [0.0, 0.0, 1.0, 1.0];
        x.offsets = Margin::default();
        x.align = [0.0, 0.0];
        x.auto_size = auto_size;
        MenuView::add_to_viewport(cx, canvas, UI_GLOBAL_Z + z);
    }

    /// AddToViewport with SetPositionInViewport(position, false) (a tooltip): anchors 0, the offset the position
    /// (Slate units), its desired size.
    pub fn add_to_viewport_at(cx: &mut Cx<'_>, w: WidgetId, z: i32, x: f64, y: f64) {
        if cx.view.in_viewport(w) {
            return;
        }
        let canvas = cx.view.root_canvas(w);
        let s = cx.view.tree.widget_mut(w);
        s.anchors = [0.0; 4];
        s.offsets = Margin { l: x, t: y, r: 0.0, b: 0.0 };
        s.align = [0.0, 0.0];
        s.auto_size = true;
        MenuView::add_to_viewport(cx, canvas, z);
    }

    /// A new CanvasPanel 'RootCanvas' holding `w`.
    fn root_canvas(&mut self, w: WidgetId) -> WidgetId {
        let bundle = self.bundle.clone();
        let canvas = self.tree.create_plain(&bundle, "CanvasPanel", "RootCanvas");
        self.tree.add(canvas, w);
        canvas
    }

    /// ForceLayoutPrepass and GetDesiredSize (Slate units, at the viewport's scale).
    pub fn desired_size(&mut self, fonts: &mut Fonts, w: WidgetId) -> [f64; 2] {
        measure_widget(&mut self.tree, fonts, w, self.dpi)
    }

    /// A panel's AddChild (at `at`: InsertChildAt); Construct runs when the parent is on the viewport.
    pub fn add_child(cx: &mut Cx<'_>, parent: WidgetId, child: WidgetId, at: Option<usize>) {
        cx.view.tree.insert(parent, child, at);
        if cx.view.in_viewport(parent) {
            MenuView::construct_tree(cx, child);
        }
    }

    /// RemoveChild: Destruct runs, the child leaves its parent.
    pub fn remove_child(cx: &mut Cx<'_>, child: WidgetId) {
        MenuView::destruct_tree(cx, child);
        cx.view.tree.remove(child);
    }

    /// RemoveFromParent of a viewport widget (or of the widget of a RootCanvas): the layer goes, Destruct runs. A
    /// RootCanvas wrapper, which only its layer holds, is destroyed for good (each tooltip shown made a new one);
    /// `w` leaves it and stays to be added again.
    pub fn remove_from_parent(cx: &mut Cx<'_>, w: WidgetId) {
        let parent = cx.view.tree.widget(w).parent;
        let tree = &cx.view.tree;
        let at = cx
            .view
            .layers
            .iter()
            .position(|l| l.w == w || (Some(l.w) == parent && tree.widget(l.w).name == "RootCanvas"));
        let Some(at) = at else { return };
        let layer = cx.view.layers.remove(at);
        MenuView::destruct_tree(cx, w);
        if layer.w != w {
            cx.view.tree.remove(w);
            cx.view.tree.destroy(layer.w);
        }
    }

    pub fn remove_from_viewport(cx: &mut Cx<'_>, w: WidgetId) {
        MenuView::remove_from_parent(cx, w);
    }

    /// `w` dismissed for good: taken off the viewport or out of its parent (Destruct runs), then freed with every
    /// widget under it at the end of the frame's tick, its ids given back and its controllers dropped. The caller
    /// holds that nothing will use `w` or a widget under it again: what UE's GC collects once the last reference
    /// to the widget is gone (the manager's tables, the view's lists, a delay or a handler of a freed widget are let
    /// go here; a stale id that is still used panics in the tree, `Tree::alive`). A widget added again before the
    /// tick's end is kept.
    // Deviation: UE's GC collects a dismissed widget; the port frees what a caller hands here.
    pub fn discard(cx: &mut Cx<'_>, w: WidgetId) {
        if !cx.view.tree.alive(w) {
            return;
        }
        let parent = cx.view.tree.widget(w).parent;
        let layer = cx
            .view
            .layers
            .iter()
            .any(|l| l.w == w || (Some(l.w) == parent && cx.view.tree.widget(l.w).name == "RootCanvas"));
        if layer {
            MenuView::remove_from_parent(cx, w);
        } else if parent.is_some() {
            MenuView::remove_child(cx, w);
        } else if cx.view.is_constructed(w) {
            MenuView::destruct_tree(cx, w);
        }
        if !cx.view.doomed.contains(&w) {
            cx.view.doomed.push(w);
        }
    }

    /// The discarded widgets freed (the end of `tick`: no controller is in a call). One added again since its
    /// discard, or under a controller still in a call, is kept.
    fn free_discarded(&mut self) {
        if self.doomed.is_empty() {
            return;
        }
        let mut gone: Vec<WidgetId> = Vec::new();
        for w in std::mem::take(&mut self.doomed) {
            if !self.tree.alive(w) || self.tree.widget(w).parent.is_some() || self.in_viewport(w) {
                continue;
            }
            let all = self.tree.post_order(w);
            if all.iter().any(|&x| self.is_busy(x)) {
                self.doomed.push(w);
                continue;
            }
            for &x in &all {
                self.ctls.remove(x);
            }
            self.tree.destroy(w);
            gone.extend(all);
        }
        if gone.is_empty() {
            return;
        }
        let gone: std::collections::HashSet<WidgetId> = gone.into_iter().collect();
        let out = |w: &WidgetId| gone.contains(w);
        self.hovered.retain(|w| !out(w));
        self.hovered_buttons.retain(|w| !out(w));
        if self.capture.as_ref().is_some_and(out) {
            self.capture = None;
        }
        if self.edit.is_some_and(|e| out(&e.edit_box) || out(&e.text) || out(&e.owner)) {
            self.edit = None;
        }
        self.latent.retain(|(_, h)| !out(&h.owner));
        self.playing.retain(|p| !out(&p.owner));
        for p in &mut self.playing {
            if let Some(saved) = &mut p.saved {
                saved.retain(|(w, _)| !out(w));
            }
        }
        self.world.retain(|x| !out(&x.w));
        self.targets.retain(|w| !out(w));
        if self.cursor.as_ref().is_some_and(out) {
            self.cursor = None;
        }
        self.tick_order = None;
        self.bar_roots = None;
        self.manager.forget(&gone);
    }

    /// The widgets the tree holds now and every widget it has made (with no widget freed, both would be the second).
    pub fn widget_counts(&self) -> (usize, u64) {
        (self.tree.in_use(), self.tree.births())
    }

    /// The root of `w` is a layer, a world widget or a target (a freed widget is on none).
    pub fn in_viewport(&self, w: WidgetId) -> bool {
        if !self.tree.alive(w) {
            return false;
        }
        let root = self.tree.top(w);
        self.layers.iter().any(|l| l.w == root)
            || self.world.iter().any(|l| l.w == root)
            || self.targets.contains(&root)
    }

    /// The world widget `i` (`add_world_widget`'s index) at the component's projected place: `at` its point in
    /// pixels of the view and its distance from the view origin, None off the screen or hidden (SWorldWidgetScreenLayer
    /// Tick 142617ff0: the point rounded to whole pixels, converted to Slate units; ZOrder -(int32) distance).
    pub fn place_world_widget(&mut self, i: usize, at: Option<([f64; 2], f64)>) {
        let dpi = self.dpi;
        let next = self.world.iter().map(|e| e.shown).max().unwrap_or(0) + 1;
        let Some(e) = self.world.get_mut(i) else { return };
        if e.at.is_none() && at.is_some() {
            e.shown = next;
        }
        e.at = at.map(|(p, _)| [p[0].round() / dpi, p[1].round() / dpi]);
        if let Some((_, d)) = at {
            e.distance = d;
        }
    }

    fn world_order(&self) -> Vec<usize> {
        world_order(&self.world)
    }

    /// A world-space WidgetComponent drawn at its desired size into a texture of its own (Construct runs).
    pub fn add_target_widget(cx: &mut Cx<'_>, w: WidgetId) {
        cx.view.targets.push(w);
        MenuView::construct_tree(cx, w);
    }

    /// A screen-space WidgetComponent (Construct runs); its index in `world`.
    pub fn add_world_widget(cx: &mut Cx<'_>, w: WidgetId) -> usize {
        cx.view.world.push(WorldWidget { w, at: None, size: None, distance: 0.0, shown: 0 });
        MenuView::construct_tree(cx, w);
        cx.view.world.len() - 1
    }

    /// The dev view's entry: a widget on the viewport at ZOrder 0.
    pub fn add(cx: &mut Cx<'_>, cls: &str) -> WidgetId {
        let w = MenuView::create(cx, cls);
        MenuView::add_to_viewport(cx, w, 0);
        w
    }

    /// UI_Functions.IsVisibleInHierarchy.
    pub fn visible_in_hierarchy(&self, w: WidgetId) -> bool {
        slate::visible_in_hierarchy(&self.tree, w)
    }

    /// USlateBlueprintLibrary::LocalToViewport's ViewportPosition (Slate units) of a point of `w`'s local space, by
    /// the last paint ((0, 0) when it was never painted).
    pub fn local_to_viewport(&self, w: WidgetId, x: f64, y: f64) -> [f64; 2] {
        let Some(p) = self.tree.painted(w) else { return [0.0, 0.0] };
        let m = p.transform;
        [(m[0] * x + m[2] * y + m[4]) / self.dpi, (m[1] * x + m[3] * y + m[5]) / self.dpi]
    }

    /// ScrollWidgetIntoView of `w` in ScrollBox `sb`, its centre to the view's centre.
    pub fn scroll_into_view_centre(&mut self, sb: WidgetId, w: WidgetId, animate: bool) {
        self.bars.scroll_into_view_centre(&mut self.tree, sb, w, animate);
    }

    // ---------- delays ----------

    /// Delay: `h` resumed after `sec` seconds of the view's clock (every call adds one; a destructed owner's are
    /// dropped).
    pub fn delay(&mut self, sec: f64, h: Handler) {
        self.latent.push((sec, h));
    }

    // ---------- widget animations ----------

    fn anim_index(&self, owner: WidgetId, name: &str) -> Option<usize> {
        self.playing.iter().position(|p| p.owner == owner && p.name == name)
    }

    /// PlayAnimation(anim, 0, loops, mode, speed) of `owner`'s animation `name`: forward from the start, reverse from
    /// the last valid frame; nothing applied before the next tick. `loops`: NumLoopsToPlay (0 forever); `ping_pong`:
    /// each pass the other way; `restore`: bRestoreState (a replay keeps the values saved first). Unverified: the
    /// loops and ping-pong from UUMGSequencePlayer's public 4.26 source.
    #[allow(clippy::too_many_arguments)]
    pub fn play_animation(
        &mut self,
        owner: WidgetId,
        name: &str,
        reverse: bool,
        speed: f64,
        loops: u32,
        ping_pong: bool,
        restore: bool,
    ) {
        let class = self.tree.user_class(owner).unwrap_or_else(|| panic!("menu: {owner:?} is no user widget"));
        let class = class.to_string();
        let anim = self.bundle.anim(&class, name);
        let end = (anim.range[1] - 1.0 - anim.range[0]) / anim.res;
        let was = self.anim_index(owner, name).map(|i| self.playing.remove(i));
        let saved = if restore { was.and_then(|p| p.saved) } else { None };
        self.playing.push(Playing {
            owner,
            class,
            name: name.to_string(),
            end,
            time: if reverse { end } else { 0.0 },
            dir: if reverse { -1.0 } else { 1.0 },
            speed,
            loops,
            done: 0,
            ping_pong,
            restore,
            saved,
        });
    }

    /// A widget animation's length in seconds (the playback range, as `play_animation` takes it); 0 for a class
    /// without it.
    pub fn anim_length(&self, class: &str, name: &str) -> f64 {
        self.bundle
            .widgets
            .get(class)
            .and_then(|c| c.anims.get(name))
            .map_or(0.0, |a| (a.range[1] - 1.0 - a.range[0]) / a.res)
    }

    /// PlayAnimation with the defaults (one pass, no ping-pong, no restore).
    pub fn play(&mut self, owner: WidgetId, name: &str, reverse: bool, speed: f64) {
        self.play_animation(owner, name, reverse, speed, 1, false, false);
    }

    /// UUMGSequencePlayer::Stop: with bRestoreState the saved values come back.
    fn stop_player(&mut self, i: usize) {
        let p = self.playing.remove(i);
        if p.restore
            && let Some(saved) = p.saved
        {
            restore_anim(&mut self.tree, &saved);
        }
    }

    pub fn stop_all_animations(&mut self, owner: WidgetId) {
        while let Some(i) = self.playing.iter().position(|p| p.owner == owner) {
            self.stop_player(i);
        }
    }

    pub fn stop_animation(&mut self, owner: WidgetId, name: &str) {
        if let Some(i) = self.anim_index(owner, name) {
            self.stop_player(i);
        }
    }

    pub fn reverse_animation(&mut self, owner: WidgetId, name: &str) {
        if let Some(i) = self.anim_index(owner, name) {
            self.playing[i].dir = -self.playing[i].dir;
        }
    }

    pub fn set_playback_speed(&mut self, owner: WidgetId, name: &str, speed: f64) {
        if let Some(i) = self.anim_index(owner, name) {
            self.playing[i].speed = speed;
        }
    }

    pub fn is_animation_playing(&self, owner: WidgetId, name: &str) -> bool {
        self.anim_index(owner, name).is_some()
    }

    pub fn is_animation_playing_forward(&self, owner: WidgetId, name: &str) -> bool {
        self.anim_index(owner, name).is_some_and(|i| self.playing[i].dir > 0.0)
    }

    fn tick_animations(&mut self, dt: f64) {
        let bundle = self.bundle.clone();
        let list: Vec<(WidgetId, String)> = self.playing.iter().map(|p| (p.owner, p.name.clone())).collect();
        for (owner, name) in list {
            let Some(i) = self.anim_index(owner, &name) else { continue };
            let p = &mut self.playing[i];
            let anim: &AnimData = bundle.anim(&p.class, &p.name);
            let from = p.time;
            p.time += p.dir * dt * p.speed;
            let mut done = if p.dir > 0.0 { p.time >= p.end } else { p.time <= 0.0 };
            if done {
                p.done += 1;
                if p.loops == 0 || p.done < p.loops {
                    // another pass: the overshoot carries over, mirrored for ping-pong, from the start for a loop
                    done = false;
                    let over = if p.dir > 0.0 { p.time - p.end } else { -p.time };
                    let o = if p.end > 0.0 { (over % p.end).min(p.end) } else { 0.0 };
                    if p.ping_pong {
                        p.dir = -p.dir;
                    }
                    p.time = if p.dir > 0.0 { o } else { p.end - o };
                } else {
                    p.time = if p.dir > 0.0 { p.end } else { 0.0 };
                }
            }
            if p.restore && p.saved.is_none() {
                p.saved = Some(capture_anim(&self.tree, owner, anim));
            }
            let time = self.playing[i].time;
            slate::apply_anim(&mut self.tree, owner, anim, time, from);
            if done {
                // the natural end restores as Stop does, after the last evaluation
                self.stop_player(i);
            }
        }
    }

    // ---------- the pointer and the cursor ----------

    /// UI_Manager.ShowMouseCursor: PC0.bShowMouseCursor.
    pub fn show_cursor(&mut self, show: bool) {
        self.show_mouse_cursor = show;
    }

    /// UI_Cursor.Get Position for the manager: the pointer while the view has a cursor.
    pub fn pointer(&self) -> Option<[f64; 2]> {
        self.cursor.and(self.mouse)
    }

    /// The pointer from the window (a mouse move, or the place another screen left it), device pixels.
    pub fn set_pointer(&mut self, x: f64, y: f64) {
        self.mouse = Some([x, y]);
    }

    /// UI_Cursor.Set Position (SetMouseLocation of player 0).
    pub fn set_position(&mut self, x: f64, y: f64) {
        self.set_pointer(x, y);
        self.pointer_written = true;
    }

    /// UI_Global.InitializeCursors for player 0: the view draws the game's cursor and the left stick moves the
    /// pointer. The tree is no layer: it is painted over them at the pointer.
    pub fn enable_cursor(cx: &mut Cx<'_>) {
        if cx.view.cursor.is_some() || !cx.view.bundle.widgets.contains_key("UI_Cursor") {
            return;
        }
        let bundle = cx.view.bundle.clone();
        let w = cx.view.tree.create(&bundle, "UI_Cursor", "UI_Cursor");
        let c = Cursor::new(&mut cx.with(w));
        cx.view.ctls.insert(w, Ctl { slot: Slot::Here(Box::new(c)), constructed: true, destroyed: false });
        cx.view.cursor = Some(w);
        cx.view.manager.cursor = Some(w);
    }

    /// The cursor's Destruct (the screen is removed).
    pub fn remove_cursor(cx: &mut Cx<'_>) {
        let Some(w) = cx.view.cursor.take() else { return };
        cx.visit::<Cursor, _>(w, |c, cx| c.remove(cx));
        if let Some(s) = cx.view.ctls.get_mut(&w) {
            s.destroyed = true;
            s.constructed = false;
        }
    }

    // ---------- the keyboard focus ----------

    /// SetUserFocus: the binding row `row` sees key and mouse events first while it listens.
    pub fn set_user_focus(&mut self, row: WidgetId) {
        self.capture = Some(row);
    }

    /// The listening row (its controller's `as_input_capture` listening), if any.
    fn listening(&mut self) -> Option<WidgetId> {
        let c = self.capture?;
        if self.is_destroyed(c) {
            return None;
        }
        let listening = self.here(c)?.as_input_capture()?.is_listening();
        listening.then_some(c)
    }

    /// SetKeyboardFocus on EditableTextBox `edit_box`: typing edits its text child, and the controller of `owner`
    /// hears the edits (`Controller::text_edit`); the text child shows the caret. Deviation: no selection, no copy,
    /// no caret placed by the mouse; a paste (Ctrl+V, the port's own) comes as the characters typed (`pasted`).
    pub fn set_keyboard_focus(cx: &mut Cx<'_>, edit_box: WidgetId, owner: WidgetId) {
        if cx.view.edit.is_some_and(|e| e.edit_box == edit_box) {
            return;
        }
        MenuView::clear_keyboard_focus(cx, 2, true);
        let Some(&text) = cx.view.tree.widget(edit_box).children.first() else { return };
        let caret = cx.view.tree.widget(text).text.chars().count();
        cx.view.edit = Some(Edit { edit_box, text, owner, caret });
        cx.view.show_caret();
        MenuView::report(cx, owner, edit_box, TextEdit::Focused(true));
    }

    pub fn has_keyboard_focus(&self, edit_box: WidgetId) -> bool {
        self.edit.is_some_and(|e| e.edit_box == edit_box)
    }

    /// The focus leaves the box: OnTextCommitted with the ETextCommit method (2 the user moved the focus, 3
    /// cleared) when `commit`, then the focus event.
    pub fn clear_keyboard_focus(cx: &mut Cx<'_>, method: u8, commit: bool) {
        let Some(e) = cx.view.edit.take() else { return };
        cx.view.tree.widget_mut(e.text).caret = -1;
        if commit {
            let t = cx.view.tree.widget(e.text).text.clone();
            MenuView::report(cx, e.owner, e.edit_box, TextEdit::Committed(t, method));
        }
        MenuView::report(cx, e.owner, e.edit_box, TextEdit::Focused(false));
    }

    fn report(cx: &mut Cx<'_>, owner: WidgetId, edit_box: WidgetId, ev: TextEdit) {
        cx.run(owner, Box::new(move |c, cx| c.text_edit(cx, edit_box, ev)));
    }

    /// A key while a box has the keyboard focus: the box's (true), the view's otherwise.
    fn edit_key(cx: &mut Cx<'_>, key: &str) -> bool {
        let Some(mut e) = cx.view.edit else { return false };
        let mut chars: Vec<char> = cx.view.tree.widget(e.text).text.chars().collect();
        e.caret = e.caret.min(chars.len());
        let mut changed = false;
        match key {
            "Enter" => {
                // SEditableText commits on Enter and gives up the keyboard focus, which commits again as cleared
                let t = chars.iter().collect::<String>();
                MenuView::report(cx, e.owner, e.edit_box, TextEdit::Committed(t, 1));
                if cx.view.edit.is_some_and(|x| x.edit_box == e.edit_box) {
                    MenuView::clear_keyboard_focus(cx, 3, true);
                }
                return true;
            }
            "Escape" => {
                MenuView::report(cx, e.owner, e.edit_box, TextEdit::Escape);
                return true;
            }
            "BackSpace" if e.caret > 0 => {
                chars.remove(e.caret - 1);
                e.caret -= 1;
                changed = true;
            }
            "Delete" if e.caret < chars.len() => {
                chars.remove(e.caret);
                changed = true;
            }
            "Left" => e.caret = e.caret.saturating_sub(1),
            "Right" => e.caret = (e.caret + 1).min(chars.len()),
            "Home" => e.caret = 0,
            "End" => e.caret = chars.len(),
            _ => {}
        }
        cx.view.edit = Some(e);
        cx.view.show_caret();
        if changed {
            MenuView::edited(cx, e, chars.into_iter().collect());
        }
        true
    }

    /// A character typed into the box with the keyboard focus.
    fn edit_char(cx: &mut Cx<'_>, ch: char) {
        let Some(mut e) = cx.view.edit else { return };
        if ch.is_control() {
            return;
        }
        let mut chars: Vec<char> = cx.view.tree.widget(e.text).text.chars().collect();
        e.caret = e.caret.min(chars.len());
        chars.insert(e.caret, ch);
        e.caret += 1;
        cx.view.edit = Some(e);
        cx.view.show_caret();
        MenuView::edited(cx, e, chars.into_iter().collect());
    }

    /// The focused box's caret on its text child (slate.rs draws it), its blink started again when it moved.
    fn show_caret(&mut self) {
        let Some(e) = self.edit else { return };
        let at = i32::try_from(e.caret).unwrap_or(i32::MAX);
        let time = self.time;
        let w = self.tree.widget_mut(e.text);
        if w.caret != at {
            w.caret = at;
            w.caret_time = time;
        }
    }

    /// OnTextChanged: the text written, the handler told (it may set another text).
    fn edited(cx: &mut Cx<'_>, e: Edit, text: String) {
        cx.view.tree.widget_mut(e.text).text = text.clone();
        MenuView::report(cx, e.owner, e.edit_box, TextEdit::Changed(text));
    }

    // ---------- input ----------

    /// The topmost widget under the pointer: the layers', the last added first, then the world widgets', the last
    /// painted first.
    fn hit(&mut self) -> Option<WidgetId> {
        let m = self.mouse?;
        self.hit_layer = -1;
        for i in (0..self.layers.len()).rev() {
            if let Some(h) = hit_test(&self.tree, self.layers[i].w, m[0], m[1]) {
                self.hit_layer = i32::try_from(i).unwrap_or(i32::MAX);
                return Some(h);
            }
        }
        for i in self.world_order().into_iter().rev() {
            if let Some(h) = hit_test(&self.tree, self.world[i].w, m[0], m[1]) {
                return Some(h);
            }
        }
        None
    }

    fn under(&mut self, target: WidgetId) -> bool {
        let mut w = self.hit();
        while let Some(x) = w {
            if x == target {
                return true;
            }
            w = self.tree.widget(x).parent;
        }
        false
    }

    /// A key press of local player `p` (-1 none): the box with the keyboard focus takes every key, a listening
    /// row's OnPreviewKeyDown tunnels first, then the manager's key handling.
    pub fn key_down(cx: &mut Cx<'_>, key: &str, p: i32) -> bool {
        if MenuView::edit_key(cx, key) {
            return true;
        }
        if let Some(c) = cx.view.listening() {
            let key = key.to_string();
            let took =
                cx.visit_dyn(c, move |ctl, cx| ctl.as_input_capture().is_some_and(|r| r.on_preview_key_down(cx, &key)));
            if took == Some(true) {
                return true;
            }
        }
        Manager::key_down(cx, key, p)
    }

    /// (the port's own) What a Ctrl+V types of the clipboard's `text`: its first line without the spaces around it
    /// and without control characters. The box's owner filters and clips the rest as it does a typed character (the
    /// room code box: TextInput's CharacterLimit).
    pub fn pasted(text: &str) -> impl Iterator<Item = char> + '_ {
        text.lines().next().unwrap_or("").trim().chars().filter(|c| !c.is_control())
    }

    /// A character typed (the box with the keyboard focus).
    pub fn char_typed(cx: &mut Cx<'_>, ch: char) {
        MenuView::edit_char(cx, ch);
    }

    /// A mouse button of player `p` went down: the focused box loses the focus to a press outside it, a scroll bar
    /// takes the left button, a listening row the buttons pressed over it, the manager the rest. Unverified: what a
    /// press outside the listening row does to the focus in the build.
    pub fn mouse_down(cx: &mut Cx<'_>, button: u8, p: i32) {
        if let Some(e) = cx.view.edit {
            let area = cx.view.tree.widget(e.edit_box).parent.unwrap_or(e.edit_box);
            if !cx.view.under(area) {
                MenuView::clear_keyboard_focus(cx, 2, true);
            }
        }
        if button == 0 {
            let view = &mut *cx.view;
            if view.bars.mouse_down(&mut view.tree, view.mouse) {
                return;
            }
        }
        if let Some(c) = cx.view.listening()
            && cx.view.under(c)
        {
            let key = super::input::mouse_button_key(button);
            cx.visit_dyn(c, move |ctl, cx| {
                if let Some(r) = ctl.as_input_capture() {
                    r.process_mouse_event(cx, key);
                }
            });
            return;
        }
        Manager::mouse_down(cx, button, p);
    }

    /// A mouse button went up (the release of a scroll bar's thumb).
    pub fn mouse_up(&mut self, button: u8) {
        if button == 0 {
            self.bars.mouse_up();
        }
    }

    /// An analog stick axis for a listening row (OnAnalogValueChanged).
    pub fn analog(cx: &mut Cx<'_>, value: f64) {
        if let Some(c) = cx.view.listening() {
            cx.visit_dyn(c, move |ctl, cx| {
                if let Some(r) = ctl.as_input_capture() {
                    r.on_analog_value_changed(cx, value);
                }
            });
        }
    }

    /// The mouse wheel: a listening row under the pointer takes it as a key, else the innermost ScrollBox under
    /// the pointer that has something to scroll. Unverified: SScrollBox::OnMouseWheel is not decoded.
    pub fn wheel(cx: &mut Cx<'_>, notches: f64) {
        if let Some(c) = cx.view.listening()
            && cx.view.under(c)
        {
            let key = if notches > 0.0 { "MouseScrollUp" } else { "MouseScrollDown" };
            cx.visit_dyn(c, move |ctl, cx| {
                if let Some(r) = ctl.as_input_capture() {
                    r.process_mouse_event(cx, key);
                }
            });
            return;
        }
        let mut w = cx.view.hit();
        while let Some(x) = w {
            let view = &mut *cx.view;
            if view.tree.widget(x).kind == "ScrollBox" && view.bars.wheel(&mut view.tree, x, notches) {
                return;
            }
            w = view.tree.widget(x).parent;
        }
    }

    // ---------- hover ----------

    /// The widgets under the pointer are the topmost hit widget and its parents; a UI_Interactor is hovered while
    /// its hitbox Button is one of them. A Button of another user widget tells its owner. The widgets the pointer
    /// left hear it before those it entered.
    fn update_hover(cx: &mut Cx<'_>) {
        let mut now: Vec<WidgetId> = Vec::new();
        let mut buttons: Vec<WidgetId> = Vec::new();
        if let Some(m) = cx.view.mouse {
            let top = cx.view.hit();
            let view = &mut *cx.view;
            let on_bar = view.bars.hover(&view.tree, m, view.hit_layer) || view.bars.dragging();
            let mut own = false;
            let mut w = top;
            while let Some(x) = w {
                if view.tree.has_cursor(x) {
                    own = true;
                    break;
                }
                w = view.tree.widget(x).parent;
            }
            view.over_cursor_widget = own;
            let mut w = if on_bar { None } else { top };
            while let Some(x) = w {
                let parent = view.tree.widget(x).parent;
                if view.tree.widget(x).kind == "Button" {
                    match parent.filter(|p| view.manager.interactor(*p).is_some() && view.has_controller(*p)) {
                        Some(i) => {
                            if !now.contains(&i) {
                                now.push(i);
                            }
                        }
                        None => buttons.push(x),
                    }
                }
                w = parent;
            }
        }
        for i in cx.view.hovered.clone() {
            if !now.contains(&i) {
                cx.view.hovered.retain(|&h| h != i);
                if let Some(s) = cx.view.manager.interactor_mut(i) {
                    s.hovered = false;
                }
                if !cx.view.is_destroyed(i) {
                    super::widgets::controls::interactor::on_unhovered(cx, i);
                }
            }
        }
        for b in cx.view.hovered_buttons.clone() {
            if !buttons.contains(&b) {
                cx.view.hovered_buttons.retain(|&h| h != b);
                MenuView::button_hover(cx, b, false);
            }
        }
        for &b in &buttons {
            if !cx.view.hovered_buttons.contains(&b) {
                cx.view.hovered_buttons.push(b);
                MenuView::button_hover(cx, b, true);
            }
        }
        for i in now {
            if !cx.view.hovered.contains(&i) && !cx.view.is_destroyed(i) {
                cx.view.hovered.push(i);
                if let Some(s) = cx.view.manager.interactor_mut(i) {
                    s.hovered = true;
                }
                super::widgets::controls::interactor::on_hovered(cx, i);
            }
        }
    }

    /// The user widget whose tree holds Button `b` hears of it; one that was removed hears nothing.
    fn button_hover(cx: &mut Cx<'_>, b: WidgetId, hovered: bool) {
        let mut w = cx.view.tree.widget(b).parent;
        while let Some(x) = w {
            if cx.view.has_controller(x) {
                break;
            }
            w = cx.view.tree.widget(x).parent;
        }
        let Some(owner) = w else { return };
        if cx.view.is_destroyed(owner) {
            return;
        }
        let name = cx.view.tree.widget(b).name.clone();
        cx.run(owner, Box::new(move |c, cx| c.on_button_hover(cx, &name, hovered)));
    }

    // ---------- frame ----------

    /// `controllers_of(roots)`, collected again only when the roots, the tree's shape or the controllers changed.
    fn tick_order(&mut self, roots: Vec<WidgetId>) -> Arc<[WidgetId]> {
        let (shape, made) = (self.tree.shape(), self.ctls.made);
        if let Some(o) = &self.tick_order
            && o.roots == roots
            && o.shape == shape
            && o.made == made
        {
            return o.ctls.clone();
        }
        let ctls: Arc<[WidgetId]> = self.controllers_of(&roots).into();
        self.tick_order = Some(TickOrder { roots, shape, made, ctls: ctls.clone() });
        ctls
    }

    /// The controllers of the trees under `roots`, children first.
    fn controllers_of(&self, roots: &[WidgetId]) -> Vec<WidgetId> {
        let mut out = Vec::new();
        for &r in roots {
            for w in self.tree.post_order(r) {
                if self.ctls.contains_key(&w) {
                    out.push(w);
                }
            }
        }
        out
    }

    /// One frame of the view: the edit box, hover, the manager's tick, the delays, the timers, the
    /// animations, the widget ticks, the cursor, the scroll bars and the rich texts. The frame's dt is the clock's
    /// step since the last tick, at most 0.1 s (0 at the first).
    pub fn tick(cx: &mut Cx<'_>) {
        let t = cx.view.time;
        let dt = cx.view.last.map_or(0.0, |l| (t - l).min(0.1));
        cx.view.last = Some(t);
        if let Some(mut e) = cx.view.edit {
            e.caret = e.caret.min(cx.view.tree.widget(e.text).text.chars().count());
            cx.view.edit = Some(e);
            cx.view.show_caret();
        }
        MenuView::update_hover(cx);
        Manager::tick(cx);
        let due = std::mem::take(&mut cx.view.latent);
        for (left, h) in due {
            if cx.view.is_destroyed(h.owner) {
                continue;
            }
            let left = left - dt;
            if left <= 0.0 {
                cx.resume(h);
            } else {
                cx.view.latent.push((left, h));
            }
        }
        let mut roots: Vec<WidgetId> = cx.view.targets.clone();
        roots.extend(cx.view.world.iter().map(|w| w.w));
        roots.extend(cx.view.layers.iter().map(|l| l.w));
        let ctls = cx.view.tick_order(roots);
        for &c in ctls.iter() {
            if cx.view.is_constructed(c) {
                cx.run(c, Box::new(move |ctl, cx| ctl.timers(cx, dt)));
            }
        }
        cx.view.tick_animations(dt);
        // the toast service's Delay (one per session: the menu view that holds the session ticks it)
        cx.view.session.toasts.tick(dt);
        crate::menu::widgets::main_menu::online_frame(cx);
        for &c in ctls.iter() {
            if cx.view.is_constructed(c) && cx.view.visible_in_hierarchy(c) {
                cx.run(c, Box::new(move |ctl, cx| ctl.tick(cx, dt)));
            }
        }
        if let Some(cw) = cx.view.cursor {
            cx.visit::<Cursor, _>(cw, |c, cx| c.tick(cx, dt));
        }
        // the scroll boxes after the user widgets; collected again only when a tree or the layers changed (a page
        // built into a layer already shown brings its own boxes)
        let layer_roots: Vec<WidgetId> = cx.view.layers.iter().map(|l| l.w).collect();
        let view = &mut *cx.view;
        if view.bar_roots.as_deref() != Some(&layer_roots) || view.bar_shape != view.tree.shape() {
            view.bars.collect(&view.tree, &view.layers);
            view.rich.collect(&view.tree, &view.layers);
            view.bar_roots = Some(layer_roots);
            view.bar_shape = view.tree.shape();
        }
        view.bars.tick(&mut view.tree, dt, view.mouse);
        view.rich.tick(&mut view.tree, cx.fonts);
        view.free_discarded();
    }

    /// The layers that show while the screen's others are collapsed: their controllers' timers
    /// and ticks, children first.
    pub fn tick_alone(cx: &mut Cx<'_>, roots: &[WidgetId], dt: f64) {
        let ctls = cx.view.controllers_of(roots);
        for &c in &ctls {
            if cx.view.is_constructed(c) {
                cx.run(c, Box::new(move |ctl, cx| ctl.timers(cx, dt)));
            }
        }
        for &c in &ctls {
            if cx.view.is_constructed(c) && cx.view.visible_in_hierarchy(c) {
                cx.run(c, Box::new(move |ctl, cx| ctl.tick(cx, dt)));
            }
        }
        // (an inactive screen frees what its controllers discarded as the active one does)
        cx.view.free_discarded();
    }

    /// The scale matrix of a root painted at the DPI scale.
    fn root_matrix(&self) -> [f64; 6] {
        [self.dpi, 0.0, 0.0, self.dpi, 0.0, 0.0]
    }

    fn layout_layer(&mut self, fonts: &mut Fonts, w: WidgetId) {
        let area = Geometry { pos: [0.0, 0.0], size: self.viewport_size() };
        layout_widget(&mut self.tree, fonts, w, area, self.dpi);
    }

    /// The viewport's world widgets, layers (each with its scroll bars), the fader and the cursor laid out and
    /// painted to `out`.
    pub fn paint(&mut self, fonts: &mut Fonts, out: &mut ElementList) {
        self.dpi = dpi_scale(f64::from(self.size[0]), f64::from(self.size[1]));
        self.tree.set_time(self.time);
        for i in 0..self.world.len() {
            let WorldWidget { w, at, size, .. } = self.world[i];
            let Some(at) = at else { continue };
            let d = measure_widget(&mut self.tree, fonts, w, self.dpi);
            let [sw, sh] = size.unwrap_or(d);
            let area = Geometry { pos: [at[0] - sw / 2.0, at[1] - sh / 2.0], size: [sw, sh] };
            layout_widget(&mut self.tree, fonts, w, area, self.dpi);
        }
        for i in 0..self.layers.len() {
            let w = self.layers[i].w;
            self.layout_layer(fonts, w);
            self.bars.prepare(&mut self.tree, i);
        }
        let m = self.root_matrix();
        let world_opacity = self.world_opacity.clamp(0.0, 1.0);
        for i in self.world_order() {
            if world_opacity > 0.0 {
                paint_widget(&self.tree, fonts, out, self.world[i].w, m, world_opacity);
            }
        }
        for i in 0..self.layers.len() {
            paint_widget(&self.tree, fonts, out, self.layers[i].w, m, 1.0);
            self.bars.paint(&self.tree, out, i);
        }
        self.paint_fader(out);
        self.paint_cursor(fonts, out);
    }

    /// The `alone` layers among the layers (in the layers' order) and the fader.
    pub fn paint_alone(&mut self, fonts: &mut Fonts, out: &mut ElementList, roots: &[WidgetId]) {
        self.dpi = dpi_scale(f64::from(self.size[0]), f64::from(self.size[1]));
        self.tree.set_time(self.time);
        let list: Vec<WidgetId> = self.layers.iter().map(|l| l.w).filter(|w| roots.contains(w)).collect();
        for &w in &list {
            self.layout_layer(fonts, w);
        }
        let m = self.root_matrix();
        for &w in &list {
            paint_widget(&self.tree, fonts, out, w, m, 1.0);
        }
        self.paint_fader(out);
    }

    /// A target widget laid out at scale 1 at its desired size and painted to `out` (the component's texture);
    /// its size in pixels. The layout runs three times (a DownOnly ScaleBox takes its scale from the
    /// area it was last arranged in, so the desired size settles over the passes).
    pub fn paint_target(&mut self, fonts: &mut Fonts, out: &mut ElementList, w: WidgetId) -> [u32; 2] {
        self.tree.set_time(self.time);
        let mut d = [0.0; 2];
        for _ in 0..3 {
            d = measure_widget(&mut self.tree, fonts, w, 1.0);
            layout_widget(&mut self.tree, fonts, w, Geometry { pos: [0.0, 0.0], size: d }, 1.0);
        }
        paint_widget(&self.tree, fonts, out, w, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 1.0);
        let px = |v: f64| v.ceil().max(1.0) as u32;
        [px(d[0]), px(d[1])]
    }

    /// UI_Transitioner's Fader: black over every layer.
    pub fn paint_fader(&self, out: &mut ElementList) {
        let f = self.manager.fader;
        if f <= 0.0 {
            return;
        }
        let e = BoxEl {
            geometry: PaintGeometry {
                size: [f64::from(self.size[0]), f64::from(self.size[1])],
                transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            },
            tint: [0.0, 0.0, 0.0, f],
            uv: [0.0, 0.0, 1.0, 1.0],
            ..BoxEl::default()
        };
        let layer = i32::try_from(out.elements.len()).unwrap_or(i32::MAX);
        out.push(layer, None, DrawEffects::NONE, Element::Box(e));
    }

    /// The game's cursor over everything.
    fn paint_cursor(&mut self, fonts: &mut Fonts, out: &mut ElementList) {
        let Some(cw) = self.cursor else { return };
        let Some(at) = self.mouse else { return };
        let shown = self.show_mouse_cursor || self.over_cursor_widget;
        let Some(opacity) = self.controller::<Cursor>(cw).map(|c| c.opacity) else { return };
        if opacity.is_nan() || opacity <= 0.0 || !shown {
            return;
        }
        self.tree.widget_mut(cw).opacity = opacity;
        let b = super::cursor::BOX;
        layout_widget(&mut self.tree, fonts, cw, Geometry { pos: [0.0, 0.0], size: [b, b] }, 1.0);
        paint_widget(&self.tree, fonts, out, cw, [1.0, 0.0, 0.0, 1.0, at[0] - b / 2.0, at[1] - b / 2.0], 1.0);
    }

    /// The frame's tape line (menu/tape.rs), after the paint.
    pub fn tape_frame(&self, frame: u32) -> super::tape::MenuFrame {
        let world: Vec<WidgetId> = self.world.iter().filter(|w| w.at.is_some()).map(|w| w.w).collect();
        let layers: Vec<WidgetId> = self.layers.iter().map(|l| l.w).collect();
        super::tape::frame_of(frame, self.time, self.mouse, self.manager.fader, &self.tree, &world, &layers)
    }
}

/// Saves, once per widget and property kind, what the animation's tracks are about to write.
/// Unverified: the opacity, visibility, slot offsets and padding tracks are taken to capture as the transform,
/// colour and material tracks do.
fn capture_anim(t: &Tree, owner: WidgetId, anim: &AnimData) -> Vec<(WidgetId, Saved)> {
    let mut out: Vec<(WidgetId, Saved)> = Vec::new();
    let mut save = |w: WidgetId, s: Saved| {
        let k = s.kind();
        if !out.iter().any(|(x, y)| *x == w && y.kind() == k) {
            out.push((w, s));
        }
    };
    for tr in &anim.tracks {
        let w = if tr.widget == "$root" { Some(owner) } else { t.find_opt(owner, &tr.widget) };
        let Some(w) = w else { continue };
        let x = t.widget(w);
        let prop = tr.prop.as_deref().unwrap_or("");
        match tr.kind.as_str() {
            "MovieScene2DTransformTrack" => save(w, Saved::Xf(x.transform)),
            "MovieSceneByteTrack" if prop == "Visibility" => save(w, Saved::Vis(x.vis)),
            "MovieSceneFloatTrack" if prop == "RenderOpacity" => save(w, Saved::Opacity(x.opacity)),
            "MovieSceneColorTrack" if prop == "ShadowColorAndOpacity" => save(w, Saved::Shadow(x.shadow_color)),
            "MovieSceneColorTrack" => save(w, Saved::Color(x.color)),
            "MovieSceneMarginTrack" if prop == "LayoutData.Offsets" => save(w, Saved::Offsets(x.offsets)),
            "MovieSceneMarginTrack" if prop == "Padding" && t.user_class(w).is_some() => {
                if let Some(&c) = x.children.first() {
                    save(w, Saved::Pad(t.widget(c).pad));
                }
            }
            "MovieSceneWidgetMaterialTrack" => {
                let Some(mid) = &x.mid else { continue };
                for s in &tr.sections {
                    for k in s.ch.keys() {
                        if let Some(n) = k.strip_prefix("S:") {
                            let v = mid.scalars.iter().find(|(a, _)| a == n).map(|(_, v)| *v);
                            save(w, Saved::Scalar(n.to_string(), v));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// RestorePreAnimatedState: every saved value back, the last saved first.
fn restore_anim(t: &mut Tree, saved: &[(WidgetId, Saved)]) {
    for (w, s) in saved.iter().rev() {
        match s {
            Saved::Xf(v) => t.widget_mut(*w).transform = *v,
            Saved::Vis(v) => t.widget_mut(*w).vis = *v,
            Saved::Opacity(v) => t.widget_mut(*w).opacity = *v,
            Saved::Color(v) => t.widget_mut(*w).color = *v,
            Saved::Shadow(v) => t.widget_mut(*w).shadow_color = *v,
            Saved::Offsets(v) => t.widget_mut(*w).offsets = *v,
            Saved::Pad(v) => {
                if let Some(&c) = t.widget(*w).children.first() {
                    t.widget_mut(c).pad = *v;
                }
            }
            Saved::Scalar(n, v) => {
                let Some(mid) = &mut t.widget_mut(*w).mid else { continue };
                match v {
                    Some(v) => mid.set_scalar(n, *v),
                    // (no value was set before: the material's own default comes back)
                    None => mid.scalars.retain(|(k, _)| k != n),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The world widgets paint the farther first by the integer distance, equal ones in the order shown, and the
    /// hidden ones not at all.
    #[test]
    fn world_widgets_paint_far_to_near() {
        let w = |at: Option<[f64; 2]>, distance, shown| WorldWidget {
            w: WidgetId::default(),
            at,
            size: None,
            distance,
            shown,
        };
        let on = Some([0.0; 2]);
        let world = [w(on, 500.4, 3), w(on, 900.0, 1), w(None, 2000.0, 0), w(on, 500.9, 2), w(on, 100.0, 4)];
        assert_eq!(world_order(&world), [1, 3, 0, 4]);
    }

    #[test]
    fn a_paste_types_the_first_line_trimmed() {
        let t: String = MenuView::pasted("  AB12-cd \t\r\nsecond line").collect();
        assert_eq!(t, "AB12-cd");
        assert_eq!(MenuView::pasted("").count(), 0);
    }
}
