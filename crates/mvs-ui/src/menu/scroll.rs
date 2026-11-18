//! The scroll offset and the scroll bar of the menu's ScrollBoxes.
//!
//! From the widget data (the bundle's ScrollBox properties, data/ui): ScrollBarVisibility, ScrollbarThickness,
//! ScrollbarPadding, AlwaysShowScrollbar, bAnimateWheelScrolling, WheelScrollMultiplier, ConsumeMouseWheel, Clipping
//! and the WidgetBarStyle brushes (tints, DrawAs). From the Blueprints: UI_Functions ScrollList (SetScrollOffset of
//! GetScrollOffset - 7 x value, clamped to GetScrollOffsetOfEnd), UI_Interactor @3240 ScrollWidgetIntoView(self,
//! animate true), UI_ContextMenu @1171 and UI_SocialPanel @8582..10369 SetScrollBarVisibility /
//! SetScrollbarPadding. UI_ContextMenu @1002 adds 22 to the menu's width for its bar: thickness 18 + the default
//! padding 2 + 2, which is the bar slot's width used here.
//! Unverified: SScrollBox, SScrollBar and SScrollBarTrack are not decoded in the exe; what follows is the 4.26
//! source's behaviour, with the build's own files for the default brushes (Engine/Content/Slate/Common
//! Scrollbar_Thumb.png and Scrollbar_Background_Vertical.png are in the paks):
//!   - layout: SHorizontalBox of the scroll panel (fill) and the bar (auto width). The bar is Collapsed while the
//!     content fits (SScrollBar::ShouldBeVisible: needed or AlwaysShowScrollbar, else Collapsed), so the content has
//!     the whole width then, and width - (thickness X + padding left + right) while it scrolls. Here the slot's width
//!     is added to the right padding of the box's children, which gives both the arranged width and the box's
//!     desired width (slate.rs arranges a ScrollBox's children over its whole width).
//!   - the bar: SBorder with the padding around the track; the track's width is the thumb's (thickness X). Thumb
//!     start = (track - 35) x offset / content, length = 35 + (track - 35) x view / content (FTrackSizeInfo, min
//!     thumb size 35). Needed: view / content < 1 - 1e-4. The background brush is drawn inside the padding, over the
//!     track only (the live game: WBP_MatchOptions' bar, padding 50 left and right, darkens x 25 to 1255 of 1280 and
//!     nothing outside).
//!   - brushes: UScrollBox takes FCoreStyle "ScrollBar": thumb = Box brush of Scrollbar_Thumb, margin 4/16; top and
//!     bottom slot = Scrollbar_Background_Vertical, 8 units wide, centred; no background. The asset's WidgetBarStyle
//!     overrides tint and DrawAs per brush.
//!   - opacity: the bar's content at 1 while the bar is hovered or its thumb dragged, else 0.75 (GetThumbOpacity
//!     without bHideWhenNotInUse); the slot images x 1 while hovered or dragged, 0.5 with AlwaysShowScrollbar, else 0
//!     (GetTrackOpacity). Thumb image: dragged, hovered (the thumb), normal.
//!   - mouse: a press on the thumb drags it by the grab point, a press elsewhere on the bar puts the thumb's middle
//!     there and drags; offset = clamp(mouse - grab, 0, track - 35) / (track - 35) x content, at most the end
//!     (ExecuteOnUserScrolled, ScrollBar_OnUserScrolled).
//!   - wheel: handled when the bar is needed (ConsumeMouseWheel WhenScrollingPossible) or Always; ScrollBy(-delta x
//!     32 x WheelScrollMultiplier, no overscroll, bAnimateWheelScrolling); 32 = Slate.GlobalScrollAmount.
//!   - Tick: the shown offset is FInterpTo(shown, desired, dt, 15) while the last ScrollBy / ScrollWidgetIntoView
//!     asked for animation (snaps under 1e-4 of the content), else the desired offset. SetScrollOffset sets the
//!     desired offset only and leaves the animation flag; GetScrollOffset returns the desired offset.
//!   - horizontal boxes (Orientation Orient_Horizontal: six in the bundle): the same along x. SScrollBox's
//!     ConstructHorizontalLayout puts the bar in a row under the scroll panel (an SVerticalBox: panel fill, bar auto
//!     height), so the bar's row is Widget.bar_h, which slate.rs takes off the panel's height and adds to the box's
//!     desired height (no padding trick: the children are aligned vertically in the panel, VAlign_Center would move
//!     by the whole row). The thumb's cross size is ScrollbarThickness.Y (the thickness spacer's height), the bar's
//!     brushes the Horizontal* ones of WidgetBarStyle. The default slot image of a horizontal bar
//!     (Common/Scrollbar_Background_Horizontal) is not in the bundle: no horizontal box shows it (WBP_MatchOptions,
//!     the one whose bar can show, sets both slot images NoDrawType). The left / right shadows are NoDrawType in all
//!     six and are not drawn. ScrollWidgetIntoView and the wheel work along x. Unverified as the rest.
//!
//! Deviation, not modelled: overscroll and right-button / touch drag scrolling (inertia), the scroll panel's 1 unit
//! bottom padding (the view is the box's height in slate.rs), the bottom shadow brush (NoDrawType in every box of
//! the bundle but UI_ContextMenu's, whose list never scrolls in the build), and the draw order inside a layer (the
//! bars and the shadow of a layer are painted after its widgets).
//!   - the top shadow (WidgetStyle.TopShadowBrush; drawn by one box, the Glossary's ScrollBox_0): an SImage of the
//!     brush over the scroll panel (not the bar), HAlign Fill, VAlign Top, so the panel's width by the brush's
//!     ImageSize.Y, tinted by the brush, at opacity clamp(shown offset / 32, 0, 1) (GetStartShadowOpacity,
//!     ShadowFadeDistance 32). Unverified: from the 4.26 source as the rest. UI_ContextMenu's box stores no
//!     WidgetStyle (the engine's default shadow, ScrollBoxShadowTop): not drawn, its list never scrolls.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use serde_json::Value;

use super::view::{Layer, ViewBars};
use crate::bundle::{Bundle, Rgba};
use crate::element::{BoxEl, DrawEffects, Element, ElementList, PaintGeometry};
use crate::slate::{Margin, Tree, Vis, WidgetId, bounds, visible_in_hierarchy};

/// Slate.GlobalScrollAmount.
const GLOBAL_SCROLL_AMOUNT: f64 = 32.0;
const MIN_THUMB: f64 = 35.0;
const INTERP_SPEED: f64 = 15.0;
const NEEDED: f64 = 1.0 - 1e-4;
const SHADOW_FADE: f64 = 32.0;

pub const THUMB_TEX: &str = "Slate_Scrollbar_Thumb";
pub const SLOT_TEX: &str = "Slate_Scrollbar_Background_Vertical";
// (not in the bundle: no horizontal box of the build draws it, see the head of this file)
const SLOT_TEX_H: &str = "Slate_Scrollbar_Background_Horizontal";

fn tail(v: Option<&Value>) -> &str {
    v.and_then(Value::as_str).map_or("", |s| s.rsplit("::").next().unwrap_or(""))
}

// ---------- per ScrollBox state ----------

/// A ScrollBox's scroll state (the widget keeps it).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollState {
    /// DesiredScrollOffset.
    pub desired: f64,
    /// bAnimateScroll: set by the last ScrollBy / ScrollWidgetIntoView.
    pub animate: bool,
    /// The shown offset (Widget.scroll) as this module left it: another value is a write from outside, taken as
    /// SetScrollOffset or as the arrange's clamp to the content.
    pub seen: f64,
    /// SetScrollBarVisibility / SetScrollbarPadding; None: the asset's.
    pub bar_vis: Option<Vis>,
    pub bar_pad: Option<Margin>,
}

fn state(t: &mut Tree, sb: WidgetId) -> &mut ScrollState {
    let w = t.widget_mut(sb);
    if w.scroll != w.scroll_state.seen {
        w.scroll_state.desired = w.scroll;
        w.scroll_state.seen = w.scroll;
    }
    &mut w.scroll_state
}

/// UScrollBox::GetScrollOffset.
pub fn scroll_offset(t: &mut Tree, sb: WidgetId) -> f64 {
    state(t, sb).desired
}

/// UScrollBox::SetScrollOffset: shown from the box's next tick, animated when the last scroll was.
pub fn set_scroll_offset(t: &mut Tree, sb: WidgetId, offset: f64) {
    state(t, sb).desired = offset;
}

/// UScrollBox::ScrollToStart.
pub fn scroll_to_start(t: &mut Tree, sb: WidgetId) {
    set_scroll_offset(t, sb, 0.0);
}

/// UI_Functions.ScrollList(Scroll Box, Scroll Value): nothing for a box that is not visible in its hierarchy;
/// GetScrollOffsetOfEnd is the content's height less the view's, of the last layout.
pub fn scroll_list(t: &mut Tree, sb: WidgetId, value: f64) {
    if !visible_in_hierarchy(t, sb) {
        return;
    }
    let max = t.widget(sb).scroll_max;
    let s = state(t, sb);
    s.desired = (s.desired - value * 7.0).max(0.0).min(max);
}

/// SScrollBox::ScrollBy without overscroll: whether the desired offset moved.
pub fn scroll_by(t: &mut Tree, sb: WidgetId, amount: f64, animate: bool) -> bool {
    let max = t.widget(sb).scroll_max;
    let s = state(t, sb);
    s.animate = animate;
    let was = s.desired;
    if amount != 0.0 {
        s.desired = (s.desired + amount).max(0.0).min(max);
    }
    s.desired != was
}

/// The box's size along its scroll axis (the view).
fn along(t: &Tree, sb: WidgetId) -> f64 {
    let [_, _, w, h] = t.arranged(sb);
    if t.widget(sb).horiz { w } else { h }
}

/// EDescendantScrollDestination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollDestination {
    IntoView,
    TopOrLeft,
    Center,
    BottomOrRight,
}

/// UScrollBox::ScrollWidgetIntoView(widget, animate, destination, padding), by the last layout, along the box's
/// axis (y, or x for a horizontal box). SScrollBox::InternalScrollDescendantIntoView: the widget's position in the
/// box (P), its size (S), the box's size (V), MyPosition = the padding: IntoView scrolls by P - pad when P < pad, by
/// P + S - V when the widget ends past the view, else not at all; TopOrLeft by P - pad; Center by P + S / 2 - V / 2;
/// BottomOrRight by P + S - V. It starts from the shown offset (DesiredScrollOffset = PhysicalOffset) and scrolls by
/// that distance without overscroll. A widget that is not inside the box does nothing.
pub fn scroll_into_view(
    t: &mut Tree,
    sb: WidgetId,
    target: WidgetId,
    animate: bool,
    dest: ScrollDestination,
    pad: f64,
) {
    let horiz = t.widget(sb).horiz;
    let [_, _, tw, th] = t.arranged(target);
    let size = if horiz { tw } else { th };
    // (Center: from the widget's middle, the sum in the order the vertical-only version had)
    let centre = dest == ScrollDestination::Center;
    let mut p = if centre { size / 2.0 } else { 0.0 };
    let mut c = Some(target);
    while let Some(x) = c.filter(|x| *x != sb) {
        let [cx, cy, ..] = t.arranged(x);
        p += if horiz { cx } else { cy };
        c = t.widget(x).parent;
    }
    if c.is_none() {
        return;
    }
    let view = along(t, sb);
    let by = match dest {
        ScrollDestination::Center => p - view / 2.0,
        ScrollDestination::IntoView => {
            if p < pad {
                p - pad
            } else if p + size > view {
                p + size - view
            } else {
                0.0
            }
        }
        ScrollDestination::TopOrLeft => p - pad,
        ScrollDestination::BottomOrRight => p + size - view,
    };
    if by == 0.0 {
        return;
    }
    let shown = t.widget(sb).scroll;
    state(t, sb).desired = shown;
    scroll_by(t, sb, by, animate);
}

/// ScrollWidgetIntoView(widget, animate, Center).
pub fn scroll_into_view_centre(t: &mut Tree, sb: WidgetId, target: WidgetId, animate: bool) {
    scroll_into_view(t, sb, target, animate, ScrollDestination::Center, 0.0);
}

/// UScrollBox::SetScrollBarVisibility.
pub fn set_scroll_bar_visibility(t: &mut Tree, sb: WidgetId, vis: Vis) {
    state(t, sb).bar_vis = Some(vis);
}

/// UScrollBox::SetScrollbarPadding.
pub fn set_scrollbar_padding(t: &mut Tree, sb: WidgetId, pad: Margin) {
    state(t, sb).bar_pad = Some(pad);
}

// ---------- the asset's bar ----------

/// What a brush draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Draw {
    None,
    Image,
    Box,
}

/// A bar brush: what it draws, its texture's bundle name (None: a solid quad of its tint), its tint and its Box
/// margin as a fraction of the image.
#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    draw: Draw,
    tex: Option<String>,
    color: Rgba,
    margin: f64,
}

impl Brush {
    fn of(b: Option<&Value>, def: (Draw, Option<&str>, f64)) -> Brush {
        let draw = match tail(b.and_then(|b| b.get("DrawAs"))) {
            "NoDrawType" => Draw::None,
            "Box" => Draw::Box,
            "Image" => Draw::Image,
            "" => def.0,
            _ => Draw::Image,
        };
        let c = b.and_then(|b| b.get("TintColor")).and_then(|t| t.get("SpecifiedColor"));
        let g = |k: &str| c.and_then(|c| c.get(k)).and_then(Value::as_f64);
        let color = if c.is_some() {
            [g("R").unwrap_or(0.0), g("G").unwrap_or(0.0), g("B").unwrap_or(0.0), g("A").unwrap_or(1.0)]
        } else {
            [1.0; 4]
        };
        Brush {
            draw,
            tex: b
                .and_then(|b| b.get("ResourceObject"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or(def.1.map(str::to_string)),
            color,
            margin: b
                .and_then(|b| b.get("Margin"))
                .and_then(|m| m.get("Left"))
                .and_then(Value::as_f64)
                .unwrap_or(def.2),
        }
    }
}

/// ConsumeMouseWheel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Consume {
    WhenScrollingPossible,
    Always,
    Never,
}

/// A ScrollBox's bar as its asset has it.
#[derive(Clone, Debug, PartialEq)]
pub struct BarStyle {
    /// Orientation Orient_Horizontal: the bar runs along x under the panel.
    horiz: bool,
    vis: Vis,
    always: bool,
    /// The thumb's cross size: ScrollbarThickness X (vertical) or Y (horizontal).
    thick: f64,
    pad: Margin,
    anim_wheel: bool,
    wheel_mul: f64,
    consume: Consume,
    /// The box clips its bar (UScrollBox's ClipToBounds; an asset that stores Inherit keeps the inner panel's clip
    /// only).
    own_clip: bool,
    /// Normal, hovered, dragged.
    thumb: [Brush; 3],
    background: Brush,
    top: Brush,
    bottom: Brush,
    /// WidgetStyle.TopShadowBrush when it draws an image, and its ImageSize.Y.
    shadow: Option<Brush>,
    shadow_h: f64,
}

impl BarStyle {
    /// The style of a ScrollBox's widget properties.
    pub fn of(p: &Value) -> BarStyle {
        let null = Value::Null;
        let ws = p.get("WidgetBarStyle").unwrap_or(&null);
        let pad = p.get("ScrollbarPadding");
        let thumb = (Draw::Box, Some(THUMB_TEX), 4.0 / 16.0);
        let horiz = tail(p.get("Orientation")) == "Orient_Horizontal";
        let slot = (Draw::Image, Some(if horiz { SLOT_TEX_H } else { SLOT_TEX }), 0.0);
        // (a horizontal box's shadows are its Left / RightShadowBrush: NoDrawType in all six of the bundle, not drawn)
        let sh = if horiz { None } else { p.get("WidgetStyle").and_then(|s| s.get("TopShadowBrush")) };
        let shadow = sh
            .filter(|s| s.get("ResourceObject").is_some_and(Value::is_string))
            .map(|s| Brush::of(Some(s), (Draw::None, None, 0.0)));
        let h = if horiz { "Horizontal" } else { "Vertical" };
        let g = |v: Option<&Value>, k: &str| v.and_then(|v| v.get(k)).and_then(Value::as_f64);
        let thickness = p.get("ScrollbarThickness");
        BarStyle {
            horiz,
            vis: Vis::by_name(tail(p.get("ScrollBarVisibility"))).unwrap_or(Vis::Visible),
            always: p.get("AlwaysShowScrollbar").and_then(Value::as_bool).unwrap_or(false),
            thick: g(thickness, if horiz { "Y" } else { "X" }).unwrap_or(9.0),
            pad: match pad {
                Some(_) => Margin {
                    l: g(pad, "Left").unwrap_or(0.0),
                    t: g(pad, "Top").unwrap_or(0.0),
                    r: g(pad, "Right").unwrap_or(0.0),
                    b: g(pad, "Bottom").unwrap_or(0.0),
                },
                None => Margin { l: 2.0, t: 2.0, r: 2.0, b: 2.0 },
            },
            anim_wheel: p.get("bAnimateWheelScrolling").and_then(Value::as_bool).unwrap_or(false),
            wheel_mul: p.get("WheelScrollMultiplier").and_then(Value::as_f64).unwrap_or(1.0),
            consume: match tail(p.get("ConsumeMouseWheel")) {
                "Always" => Consume::Always,
                "Never" => Consume::Never,
                _ => Consume::WhenScrollingPossible,
            },
            own_clip: tail(p.get("Clipping")) != "Inherit",
            thumb: [
                Brush::of(ws.get("NormalThumbImage"), thumb),
                Brush::of(ws.get("HoveredThumbImage"), thumb),
                Brush::of(ws.get("DraggedThumbImage"), thumb),
            ],
            background: Brush::of(ws.get(format!("{h}BackgroundImage")), (Draw::None, None, 0.0)),
            top: Brush::of(ws.get(format!("{h}TopSlotImage")), slot),
            bottom: Brush::of(ws.get(format!("{h}BottomSlotImage")), slot),
            shadow: shadow.filter(|s| s.draw == Draw::Image),
            shadow_h: g(sh.and_then(|s| s.get("ImageSize")), "Y").unwrap_or(0.0),
        }
    }
}

static DEFAULT_STYLE: LazyLock<BarStyle> = LazyLock::new(|| BarStyle::of(&Value::Null));

fn style_of(t: &Tree, sb: WidgetId) -> &BarStyle {
    t.widget(sb).bar_style.as_deref().unwrap_or(&DEFAULT_STYLE)
}

// ---------- the bars of a view ----------

type M = [f64; 6];

fn mul(p: &M, q: [f64; 6]) -> M {
    [
        p[0] * q[0] + p[2] * q[1],
        p[1] * q[0] + p[3] * q[1],
        p[0] * q[2] + p[2] * q[3],
        p[1] * q[2] + p[3] * q[3],
        p[0] * q[4] + p[2] * q[5] + p[4],
        p[1] * q[4] + p[3] * q[5] + p[5],
    ]
}

/// A ScrollBox of a layer and what its last prepare made of its bar (the box's local units, along the bar's axis: y
/// of a vertical bar, x of a horizontal one; across it: x, or y).
#[derive(Clone, Debug, Default)]
struct Entry {
    w: WidgetId,
    layer: usize,
    /// The bar slot's width that the children's right padding holds (a horizontal box: the bar row's height, bar_h).
    padded: f64,
    /// Painted at all, the bar's border (across: start; its slot's width), the track (across start, along start,
    /// length) and the thumb (along: start, length).
    shown: bool,
    bx: f64,
    slot: f64,
    tx: f64,
    ty: f64,
    tl: f64,
    thumb_top: f64,
    thumb_h: f64,
    /// The top shadow's opacity of the last prepare (0: not drawn).
    shadow_a: f64,
    /// The mouse: 0 off the bar, 1 on it, 2 on its thumb.
    hot: u8,
    /// What the last prepare's picture was made of (None: no prepare yet).
    sig: Option<Sig>,
}

/// What a bar's picture is made of: shown, then its geometry, the mouse and the drag (0 when not shown), then the
/// shadow's opacity and width (0 without a shadow).
type Sig = [f64; 13];

/// The bars of a view's layers.
#[derive(Debug)]
pub struct ScrollBars {
    bundle: Arc<Bundle>,
    entries: Vec<Entry>,
    /// The thumb's drag: the box and the grab point along the bar.
    drag: Option<(WidgetId, f64)>,
    /// The bar slot's width in a child's right padding, and the padding it was written into (a controller that sets
    /// the slot's padding afterwards replaces it: the bar's part is added again).
    padded: HashMap<WidgetId, (f64, Margin)>,
}

impl ScrollBars {
    pub fn new(bundle: Arc<Bundle>) -> ScrollBars {
        ScrollBars { bundle, entries: Vec::new(), drag: None, padded: HashMap::new() }
    }

    fn bar_vis(t: &mut Tree, sb: WidgetId) -> Vis {
        let own = state(t, sb).bar_vis;
        own.unwrap_or(style_of(t, sb).vis)
    }

    fn bar_pad(t: &mut Tree, sb: WidgetId) -> Margin {
        let own = state(t, sb).bar_pad;
        own.unwrap_or(style_of(t, sb).pad)
    }

    fn needed(t: &Tree, sb: WidgetId) -> bool {
        let view = along(t, sb);
        let content = view + t.widget(sb).scroll_max;
        style_of(t, sb).always || (content > 0.0 && view / content < NEEDED)
    }

    /// A point of the target in the box's local units, by the last paint.
    fn local(t: &Tree, sb: WidgetId, x: f64, y: f64) -> Option<[f64; 2]> {
        let m = t.painted(sb)?.transform;
        let det = m[0] * m[3] - m[1] * m[2];
        if det == 0.0 {
            return None;
        }
        let (dx, dy) = (x - m[4], y - m[5]);
        Some([(m[3] * dx - m[2] * dy) / det, (m[0] * dy - m[1] * dx) / det])
    }

    /// The texture entry of a bar brush's bundle name.
    fn texture(&self, name: &str) -> Option<String> {
        self.bundle.textures.get(name).map(|t| t.entry().to_string())
    }

    /// A brush at (x, y) in the box's local units, w x h, at `alpha`: a solid quad of its tint
    /// without a texture, else the tinted image or Box.
    #[allow(clippy::too_many_arguments)]
    fn brush(
        &self,
        out: &mut ElementList,
        clip: Option<[f64; 4]>,
        m: &M,
        b: &Brush,
        at: [f64; 2],
        size: [f64; 2],
        alpha: f64,
    ) {
        if b.draw == Draw::None || size[0] <= 0.0 || size[1] <= 0.0 {
            return;
        }
        let a = (alpha * b.color[3]).min(1.0);
        if a <= 0.0 {
            return;
        }
        let geometry = PaintGeometry { size, transform: mul(m, [1.0, 0.0, 0.0, 1.0, at[0], at[1]]) };
        let tint = [b.color[0], b.color[1], b.color[2], a];
        let layer = i32::try_from(out.elements.len()).unwrap_or(i32::MAX);
        let el = |texture: Option<String>, margin: Option<Margin>| {
            Element::Box(BoxEl { geometry, texture, margin, tint, uv: [0.0, 0.0, 1.0, 1.0], tile: [false; 2] })
        };
        let Some(name) = &b.tex else {
            out.push(layer, clip, DrawEffects::NONE, el(None, None));
            return;
        };
        match self.texture(name) {
            Some(tex) => {
                let margin =
                    (b.draw == Draw::Box).then_some(Margin { l: b.margin, t: b.margin, r: b.margin, b: b.margin });
                out.push(layer, clip, DrawEffects::NONE, el(Some(tex), margin));
            }
            // (a bundle built before the bars' images were listed: the tint alone)
            None if b.draw == Draw::Box => out.push(layer, clip, DrawEffects::NONE, el(None, None)),
            None => {}
        }
    }

    /// A brush by the bar's axes: along (y of a vertical bar, x of a horizontal one) and across.
    #[allow(clippy::too_many_arguments)]
    fn brush_along(
        &self,
        out: &mut ElementList,
        clip: Option<[f64; 4]>,
        m: &M,
        horiz: bool,
        b: &Brush,
        along: [f64; 2],
        across: [f64; 2],
        alpha: f64,
    ) {
        if horiz {
            self.brush(out, clip, m, b, [along[0], across[0]], [along[1], across[1]], alpha);
        } else {
            self.brush(out, clip, m, b, [across[0], along[0]], [across[1], along[1]], alpha);
        }
    }
}

impl ViewBars for ScrollBars {
    /// The ScrollBoxes of the layers' trees (when a tree or the set of layers changed).
    fn collect(&mut self, tree: &Tree, layers: &[Layer]) {
        // (a freed child gives its slot note back)
        self.padded.retain(|c, _| tree.alive(*c));
        fn walk(t: &Tree, w: WidgetId, layer: usize, was: &[Entry], now: &mut Vec<Entry>) {
            if t.widget(w).kind == "ScrollBox" {
                match was.iter().find(|e| e.w == w) {
                    Some(old) => now.push(Entry { layer, ..old.clone() }),
                    None => now.push(Entry { w, layer, ..Entry::default() }),
                }
            }
            for &c in &t.widget(w).children {
                walk(t, c, layer, was, now);
            }
        }
        let mut now = Vec::new();
        for (i, l) in layers.iter().enumerate() {
            walk(tree, l.w, i, &self.entries, &mut now);
        }
        self.entries = now;
        if self.drag.is_some_and(|(w, _)| !self.entries.iter().any(|e| e.w == w)) {
            self.drag = None;
        }
    }

    /// The frame's step, after the widgets' ticks and before the layout: the thumb's drag, the offsets' animation
    /// (SScrollBox::Tick) and the bar slot's width in the children's padding.
    fn tick(&mut self, tree: &mut Tree, dt: f64, mouse: Option<[f64; 2]>) {
        if let (Some((w, grab)), Some(m)) = (self.drag, mouse) {
            let (tl, ty) = self.entries.iter().find(|e| e.w == w).map_or((0.0, 0.0), |e| (e.tl, e.ty));
            let l = ScrollBars::local(tree, w, m[0], m[1]);
            let span = tl - MIN_THUMB;
            if let (Some(l), true) = (l, span > 0.0) {
                let horiz = style_of(tree, w).horiz;
                let off = ((if horiz { l[0] } else { l[1] }) - ty - grab).max(0.0).min(span) / span;
                let content = along(tree, w) + tree.widget(w).scroll_max;
                let max = tree.widget(w).scroll_max;
                let s = state(tree, w);
                s.desired = (off * content).min(max);
                // (ScrollBar_OnUserScrolled: the thumb's offset is shown at once)
                s.animate = false;
            }
        }
        for i in 0..self.entries.len() {
            let sb = self.entries[i].w;
            let max = tree.widget(sb).scroll_max;
            let (desired, animate) = {
                let s = state(tree, sb);
                (s.desired, s.animate)
            };
            let target = desired.max(0.0).min(max);
            let shown = tree.widget(sb).scroll;
            if shown != target {
                let content = along(tree, sb) + max;
                let dist = target - shown;
                // FInterpTo on the offset as a fraction of the content: there when the distance squared is under 1e-8
                tree.widget_mut(sb).scroll = if !animate || content <= 0.0 || (dist / content).powi(2) < 1e-8 {
                    target
                } else {
                    shown + dist * (dt * INTERP_SPEED).clamp(0.0, 1.0)
                };
            }
            let now = tree.widget(sb).scroll;
            tree.widget_mut(sb).scroll_state.seen = now;
            let pad = ScrollBars::bar_pad(tree, sb);
            let st = style_of(tree, sb);
            let (horiz, thick) = (st.horiz, st.thick);
            let slot = if ScrollBars::needed(tree, sb) && ScrollBars::bar_vis(tree, sb) != Vis::Collapsed {
                (thick + if horiz { pad.t + pad.b } else { pad.l + pad.r }).max(0.0)
            } else {
                0.0
            };
            self.entries[i].padded = slot;
            // (a horizontal box: the bar's row under the panel, read by slate.rs)
            if horiz {
                tree.widget_mut(sb).bar_h = slot;
                continue;
            }
            // (per child: one added to the box later gets it too; a slot padding a controller sets is kept, only the
            // bar's part is taken back)
            for k in 0..tree.widget(sb).children.len() {
                let c = tree.widget(sb).children[k];
                let cur = tree.widget(c).pad;
                let had = match self.padded.get(&c) {
                    Some((s, p)) if *p == cur => *s,
                    _ => 0.0,
                };
                if had == slot {
                    continue;
                }
                let pad = Margin { r: cur.r - had + slot, ..cur };
                tree.widget_mut(c).pad = pad;
                self.padded.insert(c, (slot, pad));
            }
        }
    }

    /// The mouse over the bars: `hit_layer` is the layer of the topmost widget under it (-1: none); a bar under that
    /// layer is covered. True while the mouse is on a bar.
    fn hover(&mut self, tree: &Tree, mouse: [f64; 2], hit_layer: i32) -> bool {
        let mut on = false;
        for i in (0..self.entries.len()).rev() {
            let e = &self.entries[i];
            let mut hot = 0u8;
            if !on && e.shown && i32::try_from(e.layer).unwrap_or(i32::MAX) >= hit_layer {
                let st = style_of(tree, e.w);
                if let Some(l) = ScrollBars::local(tree, e.w, mouse[0], mouse[1]) {
                    let (al, ac) = if st.horiz { (l[0], l[1]) } else { (l[1], l[0]) };
                    let [_, _, bw, bh] = tree.arranged(e.w);
                    let cross = if st.horiz { bh } else { bw };
                    if al >= 0.0 && al < along(tree, e.w) && (!st.own_clip || (ac >= 0.0 && ac < cross)) {
                        let on_track = ac >= e.tx && ac < e.tx + st.thick;
                        if on_track && al >= e.thumb_top && al < e.thumb_top + e.thumb_h {
                            hot = 2;
                        } else if (ac >= e.bx && ac < e.bx + e.slot) || (on_track && al >= e.ty && al < e.ty + e.tl) {
                            hot = 1;
                        }
                    }
                }
            }
            self.entries[i].hot = hot;
            if hot > 0 {
                on = true;
            }
        }
        on
    }

    fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The left button on a bar: the thumb's drag starts (SScrollBar::OnMouseButtonDown). True when a bar took it.
    fn mouse_down(&mut self, tree: &mut Tree, mouse: Option<[f64; 2]>) -> bool {
        let Some(m) = mouse else { return false };
        let Some(e) = self.entries.iter().find(|x| x.hot > 0) else { return false };
        let Some(l) = ScrollBars::local(tree, e.w, m[0], m[1]) else { return false };
        let along_pos = if style_of(tree, e.w).horiz { l[0] } else { l[1] };
        self.drag = Some((e.w, if e.hot == 2 { along_pos - e.thumb_top } else { e.thumb_h / 2.0 }));
        true
    }

    fn mouse_up(&mut self) {
        self.drag = None;
    }

    /// SScrollBox::OnMouseWheel: true when the box handled it (the wheel goes on to an outer box otherwise).
    fn wheel(&mut self, tree: &mut Tree, sb: WidgetId, notches: f64) -> bool {
        let st = style_of(tree, sb);
        let (always, consume, mul, animate) = (st.always, st.consume, st.wheel_mul, st.anim_wheel);
        let view = along(tree, sb);
        let content = view + tree.widget(sb).scroll_max;
        let needed = always || (content > 0.0 && view / content < NEEDED);
        if !((needed && consume != Consume::Never) || consume == Consume::Always) {
            return false;
        }
        scroll_by(tree, sb, -notches * GLOBAL_SCROLL_AMOUNT * mul, animate) || consume == Consume::Always
    }

    fn scroll_into_view_centre(&mut self, tree: &mut Tree, sb: WidgetId, w: WidgetId, animate: bool) {
        scroll_into_view_centre(tree, sb, w, animate);
    }

    /// After a layer's layout: its bars' geometry. True when one of them will paint another picture than at the last
    /// call.
    fn prepare(&mut self, tree: &mut Tree, layer: usize) -> bool {
        let mut changed = false;
        for i in 0..self.entries.len() {
            if self.entries[i].layer != layer {
                continue;
            }
            let (sb, padded) = (self.entries[i].w, self.entries[i].padded);
            let vis = ScrollBars::bar_vis(tree, sb);
            let pad = ScrollBars::bar_pad(tree, sb);
            let st = style_of(tree, sb);
            let (h, own_clip, has_shadow) = (st.horiz, st.own_clip, st.shadow.is_some());
            let view = along(tree, sb);
            let [_, _, bw, bh] = tree.arranged(sb);
            let across = if h { bh } else { bw };
            let scroll = tree.widget(sb).scroll;
            let content = view + tree.widget(sb).scroll_max;
            let n = &mut self.entries[i];
            n.shown = ScrollBars::needed(tree, sb)
                && vis != Vis::Collapsed
                && vis != Vis::Hidden
                && content > 0.0
                && visible_in_hierarchy(tree, sb);
            n.slot = padded;
            n.bx = across - n.slot;
            n.tx = n.bx + if h { pad.t } else { pad.l };
            n.ty = if h { pad.l } else { pad.t };
            n.tl = view - if h { pad.l + pad.r } else { pad.t + pad.b };
            let span = n.tl - MIN_THUMB;
            if span < 0.0 {
                n.shown = false;
            }
            // a bar that its own box clips away (the lobby's lists: padding right -16 under a thickness of 16)
            if own_clip && n.tx >= across && n.bx >= across {
                n.shown = false;
            }
            if n.shown {
                n.thumb_top = n.ty + span * scroll / content;
                n.thumb_h = MIN_THUMB + span * view.min(content) / content;
            }
            n.shadow_a = if has_shadow && scroll > 0.0 && visible_in_hierarchy(tree, sb) {
                (scroll / SHADOW_FADE).min(1.0)
            } else {
                0.0
            };
            let mut sig: Sig = [0.0; 13];
            if n.shown {
                let dragged = if self.drag.is_some_and(|(w, _)| w == sb) { 1.0 } else { 0.0 };
                sig[..11].copy_from_slice(&[
                    1.0,
                    n.bx,
                    n.slot,
                    n.tx,
                    n.ty,
                    n.tl,
                    n.thumb_top,
                    n.thumb_h,
                    view,
                    f64::from(n.hot),
                    dragged,
                ]);
            }
            if has_shadow {
                sig[11] = n.shadow_a;
                if n.shadow_a > 0.0 {
                    sig[12] = bw - n.padded;
                }
            }
            if n.sig != Some(sig) {
                n.sig = Some(sig);
                changed = true;
            }
        }
        changed
    }

    /// The bars of a layer over its widgets, by the transforms its paint left: inside the clips of the box's
    /// parents (and its own, unless the asset stores Inherit) at their inherited opacity.
    fn paint(&mut self, tree: &Tree, out: &mut ElementList, layer: usize) {
        for e in &self.entries {
            if e.layer != layer || !(e.shown || e.shadow_a > 0.0) {
                continue;
            }
            let sb = e.w;
            let Some(p) = tree.painted(sb) else { continue };
            let m = p.transform;
            let st = style_of(tree, sb);
            let mut alpha = 1.0;
            let mut clip: Option<[f64; 4]> = None;
            let mut at = Some(sb);
            while let Some(c) = at {
                let x = tree.widget(c);
                alpha *= x.opacity;
                if x.clip
                    && (c != sb || st.own_clip)
                    && let Some(cp) = tree.painted(c)
                {
                    let b = bounds(&cp.transform, cp.size[0], cp.size[1]);
                    clip = Some(match clip {
                        Some(k) => [b[0].max(k[0]), b[1].max(k[1]), b[2].min(k[2]), b[3].min(k[3])],
                        None => b,
                    });
                }
                at = x.parent;
            }
            if alpha <= 0.0 {
                continue;
            }
            let [_, _, bw, _] = tree.arranged(sb);
            if e.shadow_a > 0.0
                && let Some(sh) = &st.shadow
            {
                self.brush(out, clip, &m, sh, [0.0, 0.0], [bw - e.padded, st.shadow_h], alpha * e.shadow_a);
            }
            if !e.shown {
                continue;
            }
            let dragging = self.drag.is_some_and(|(w, _)| w == sb);
            let thumb_a = alpha * if dragging || e.hot > 0 { 1.0 } else { 0.75 };
            let track_a = thumb_a
                * if dragging || e.hot > 0 {
                    1.0
                } else if st.always {
                    0.5
                } else {
                    0.0
                };
            let h = st.horiz;
            // (the background brush inside the bar's padding, over the track: see the head of this file)
            self.brush_along(out, clip, &m, h, &st.background, [e.ty, e.tl], [e.tx, st.thick], thumb_a);
            let sx = e.tx + (st.thick - 8.0) / 2.0;
            self.brush_along(out, clip, &m, h, &st.top, [e.ty, e.thumb_top - e.ty], [sx, 8.0], track_a);
            self.brush_along(
                out,
                clip,
                &m,
                h,
                &st.bottom,
                [e.thumb_top + e.thumb_h, e.ty + e.tl - e.thumb_top - e.thumb_h],
                [sx, 8.0],
                track_a,
            );
            let thumb = &st.thumb[if dragging {
                2
            } else if e.hot == 2 {
                1
            } else {
                0
            }];
            self.brush_along(out, clip, &m, h, thumb, [e.thumb_top, e.thumb_h], [e.tx, st.thick], thumb_a);
        }
    }
}
