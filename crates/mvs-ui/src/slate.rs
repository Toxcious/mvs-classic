//! The Slate subset: a widget class instantiated as a tree, its
//! measure and arrange, its widget animations, and its paint into an element list.
//!
//! Layout follows Slate: Overlay and box slots align a child inside the slot area minus padding, clamping a non-Fill
//! child to the space it gets; box panels give Automatic slots their desired size and share what is left between
//! Fill slots by weight. UMG slot defaults: padding 0, box size Automatic, alignment Left / Top for UOverlaySlot,
//! Center / Center for UScaleBoxSlot and UButtonSlot, Fill / Fill otherwise (the bundle stores only values that
//! differ). Render transforms (FWidgetTransform) apply scale, shear, rotation, then translation about the pivot,
//! after layout and without affecting it. Collapsed takes no space; Hidden keeps its space and draws nothing.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::Value;

use crate::bundle::{AnimData, Bundle, Channel, Rgba, Section, WData};
use crate::element::{
    BlurEl, BoxEl, DrawEffects, Element, ElementList, MaterialEl, PaintGeometry, RetainedEl, TextEl, Zone,
};
pub use crate::text::Justify;
use crate::text::{FontInfo, Fonts, GlyphPart, ShapedRun};

/// A widget of a tree: its index and the index's generation. `Tree::destroy` gives an index back with its generation
/// moved on, so an id kept past its widget's end never names the widget made under the index next (a lookup of it
/// panics; `Tree::alive` answers false).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct WidgetId {
    i: u32,
    g: u32,
}

impl WidgetId {
    /// No widget: an id no tree hands out.
    pub const NONE: WidgetId = WidgetId { i: u32::MAX, g: u32::MAX };

    /// The index (a tree's ids are dense indices, one freed taken again).
    pub fn index(self) -> usize {
        self.i as usize
    }
}

/// ESlateVisibility.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Vis {
    #[default]
    Visible,
    Collapsed,
    Hidden,
    HitTestInvisible,
    SelfHitTestInvisible,
}

impl Vis {
    /// The ESlateVisibility byte (a byte track's value).
    pub fn from_byte(v: u8) -> Vis {
        match v {
            1 => Vis::Collapsed,
            2 => Vis::Hidden,
            3 => Vis::HitTestInvisible,
            4 => Vis::SelfHitTestInvisible,
            _ => Vis::Visible,
        }
    }

    pub(crate) fn by_name(s: &str) -> Option<Vis> {
        Some(match s {
            "Visible" => Vis::Visible,
            "Collapsed" => Vis::Collapsed,
            "Hidden" => Vis::Hidden,
            "HitTestInvisible" => Vis::HitTestInvisible,
            "SelfHitTestInvisible" => Vis::SelfHitTestInvisible,
            _ => return None,
        })
    }
}

/// A slot's horizontal alignment (EHorizontalAlignment).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
    #[default]
    Fill,
}

/// A slot's vertical alignment (EVerticalAlignment).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Center,
    Bottom,
    #[default]
    Fill,
}

/// The alignment along one axis: 0 leading, 1 centre, 2 trailing, 3 fill.
fn h_axis(a: HAlign) -> u8 {
    match a {
        HAlign::Left => 0,
        HAlign::Center => 1,
        HAlign::Right => 2,
        HAlign::Fill => 3,
    }
}

fn v_axis(a: VAlign) -> u8 {
    match a {
        VAlign::Top => 0,
        VAlign::Center => 1,
        VAlign::Bottom => 2,
        VAlign::Fill => 3,
    }
}

/// A ScaleBox's EStretch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stretch {
    None,
    Fill,
    #[default]
    ScaleToFit,
    ScaleToFitX,
    ScaleToFitY,
    ScaleToFill,
    ScaleBySafeZone,
    UserSpecified,
}

impl Stretch {
    /// SScaleBox::DoesScaleRequireNormalizingPrepassOrLocalGeometry (exe 1417df7f0): EStretch 2-5.
    fn needs_area(self) -> bool {
        matches!(self, Stretch::ScaleToFit | Stretch::ScaleToFitX | Stretch::ScaleToFitY | Stretch::ScaleToFill)
    }
}

/// A ScaleBox's EStretchDirection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StretchDir {
    #[default]
    Both,
    DownOnly,
    UpOnly,
}

/// FMargin: left, top, right, bottom.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Margin {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

/// FWidgetTransform: the render transform, about the pivot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderTransform {
    pub translation: [f64; 2],
    pub scale: [f64; 2],
    pub shear: [f64; 2],
    /// Degrees.
    pub angle: f64,
    /// RenderTransformPivot, 0..1 of the widget's size.
    pub pivot: [f64; 2],
}

impl Default for RenderTransform {
    fn default() -> RenderTransform {
        RenderTransform { translation: [0.0; 2], scale: [1.0; 2], shear: [0.0; 2], angle: 0.0, pivot: [0.5; 2] }
    }
}

/// A dynamic material instance of a widget's brush (GetDynamicMaterial): its material and the parameters set on
/// it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mid {
    /// The UI material's object path.
    pub path: String,
    pub scalars: Vec<(String, f32)>,
    pub vectors: Vec<(String, [f32; 4])>,
    /// (parameter, texture object path).
    pub textures: Vec<(String, String)>,
}

impl Mid {
    /// An instance of `path` with no parameter set.
    pub fn of(path: &str) -> Mid {
        Mid { path: path.to_string(), ..Mid::default() }
    }

    pub fn set_scalar(&mut self, name: &str, v: f32) {
        match self.scalars.iter_mut().find(|p| p.0 == name) {
            Some(p) => p.1 = v,
            None => self.scalars.push((name.to_string(), v)),
        }
    }

    pub fn set_vector(&mut self, name: &str, v: [f32; 4]) {
        match self.vectors.iter_mut().find(|p| p.0 == name) {
            Some(p) => p.1 = v,
            None => self.vectors.push((name.to_string(), v)),
        }
    }

    pub fn set_texture(&mut self, name: &str, path: &str) {
        match self.textures.iter_mut().find(|p| p.0 == name) {
            Some(p) => p.1 = path.to_string(),
            None => self.textures.push((name.to_string(), path.to_string())),
        }
    }
}

/// WBP_Quadrilateral's outline (IsOutline): the RetainerBox effect material's scalars (the corners, EdgeFalloff =
/// Sharpness), its ColorA / ColorB, the retained Image's render transform scale and the instance's tint.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Retained {
    pub quad: Vec<(String, f32)>,
    pub color: Rgba,
    pub scale: [f64; 2],
    pub tint: Rgba,
}

/// A widget: its class's type, its name, its place in the tree and what the HUD sets on it (the fields the match
/// widgets write). The layout's and the paint's own state is the tree's.
#[derive(Clone, Debug)]
pub struct Widget {
    /// The widget's type (TextBlock, Image, Overlay, a user widget's class, ...).
    pub kind: String,
    pub name: String,
    pub parent: Option<WidgetId>,
    pub children: Vec<WidgetId>,
    pub vis: Vis,
    /// RenderOpacity.
    pub opacity: f64,
    /// ColorAndOpacity (a TextBlock's text colour).
    pub color: Rgba,
    /// A TextBlock's text.
    pub text: String,
    /// The font's typeface (None: the font's own).
    pub typeface: Option<String>,
    pub font_size: f64,
    /// OutlineSettings.OutlineSize and its colour.
    pub outline: f64,
    pub outline_color: Rgba,
    pub shadow_color: Rgba,
    pub letter_spacing: f64,
    /// TextTransformPolicy ToUpper.
    pub upper: bool,
    /// The brush's texture (an object path), when it is one.
    pub tex: Option<String>,
    /// The brush's material's dynamic instance, when it is one.
    pub mid: Option<Mid>,
    /// The brush's tint.
    pub brush_color: Rgba,
    /// An Image's Brush.TintColor (FSlateBrush.TintColor), which SImage multiplies into ColorAndOpacity.
    pub brush_tint: Rgba,
    /// The brush's ImageSize.
    pub brush_w: f64,
    pub brush_h: f64,
    /// The slot's padding.
    pub pad: Margin,
    /// A canvas slot's anchors (min x, min y, max x, max y) and offsets.
    pub anchors: [f64; 4],
    pub offsets: Margin,
    /// A box slot's size rule: 0 Automatic, else the Fill weight.
    pub fill: f64,
    /// A WidgetSwitcher's active index.
    pub active: i32,
    /// A SizeBox's WidthOverride / HeightOverride (NaN: none).
    pub w_override: f64,
    pub h_override: f64,
    pub transform: RenderTransform,
    /// A TextBlock's ShadowOffset and OutlineSettings.bApplyOutlineToDropShadows.
    pub shadow_offset: [f64; 2],
    pub outline_shadows: bool,
    /// The slot's alignment.
    pub h_align: HAlign,
    pub v_align: VAlign,
    /// A canvas slot's Alignment.
    pub align: [f64; 2],
    /// The font's FontMaterial (an object path) and its GetDynamicFontMaterial instance.
    pub font_mat: Option<String>,
    pub font_mid: Option<Mid>,
    /// OutlineSettings.bSeparateFillAlpha.
    pub separate_fill: bool,
    /// (the port's own) Every ASCII digit in the widest digit's width, so a changing number keeps its place.
    pub even_digits: bool,
    /// LineHeightPercentage, Justification, WrapTextAt (Slate units, 0: none), AutoWrapText.
    pub line_height: f64,
    pub justify: Justify,
    pub wrap_at: f64,
    pub auto_wrap: bool,
    /// MinimumDesiredWidth of a text.
    pub min_text_w: f64,
    /// Clipping ClipToBounds.
    pub clip: bool,
    /// The port's own: the clip reaches this far past the left and right edge, so a hover outline of a child that
    /// sits at the edge is not cut (a ScrollBox whose content fills its width).
    pub clip_bleed: f64,
    /// A Border's brush: no resource (a solid BrushColor quad) and its Box margin (None: drawn as an image).
    pub solid: bool,
    pub box_margin: Option<Margin>,
    /// A SizeBox's MinDesiredWidth / Height and MaxDesiredWidth / Height (NaN: none).
    pub min_w: f64,
    pub min_h: f64,
    pub max_w: f64,
    pub max_h: f64,
    /// A ScaleBox's Stretch, StretchDirection and UserSpecifiedScale.
    pub stretch: Stretch,
    pub stretch_dir: StretchDir,
    pub user_scale: f64,
    /// A canvas slot's bAutoSize.
    pub auto_size: bool,
    /// WBP_Quadrilateral: SetParameters' scalar parameters (the corners in unit space, EdgeFalloff = Sharpness).
    pub quad: Option<Vec<(String, f32)>>,
    /// WBP_Quadrilateral with IsOutline.
    pub retained: Option<Retained>,
    /// The port's own paint order among the siblings: equal values in child order, a higher
    /// value over a lower one; 0 for every widget the build makes.
    pub paint_order: i32,
    /// The port's own: a SizeBox kept at this aspect (width / height) grows on its short side
    /// to it; 0 for every widget the build makes.
    pub keep_aspect: f64,
    /// A Throbber's / CircularThrobber's NumberOfPieces; a Throbber's animated axes; a CircularThrobber's Radius and
    /// Period (the piece is `tex` at `brush_w` x `brush_h`).
    pub pieces: u32,
    pub throb_h: bool,
    pub throb_v: bool,
    pub throb_opacity: bool,
    pub radius: f64,
    pub period: f64,
    /// An EditableTextBox: hovered or holding the keyboard focus (the hovered background).
    pub edit_hot: bool,
    /// Its text child: the caret (a character index, -1: none) and the time it last moved.
    pub caret: i32,
    pub caret_time: f64,
    /// A UniformGridPanel's SlotPadding and minimum slot size; its slots' Row and Column.
    pub slot_pad: Margin,
    pub min_slot: [f64; 2],
    pub row: i32,
    pub col: i32,
    /// A GridPanel's ColumnFill / RowFill, its slots' ColumnSpan / RowSpan and Layer.
    pub col_fill: Vec<f64>,
    pub row_fill: Vec<f64>,
    pub col_span: i32,
    pub row_span: i32,
    pub layer: i32,
    /// A WrapBox's InnerSlotPadding and the width a line wraps at (-1: the allotted width).
    pub wrap_pad: [f64; 2],
    pub wrap_size: f64,
    /// A ScrollBox: the scroll offset and its largest value from the last arrange (along y, or x when `horiz`),
    /// Orient_Horizontal, the height of the bar's row under the panel (menu/scroll.rs writes it) and the bar's style.
    pub scroll: f64,
    pub scroll_max: f64,
    pub horiz: bool,
    pub bar_h: f64,
    pub bar_style: Option<Box<crate::menu::scroll::BarStyle>>,
    /// A ScrollBox's scroll state (menu/scroll.rs).
    pub scroll_state: crate::menu::scroll::ScrollState,
    /// A RichTextBlock's TextStyleSet table name.
    pub style_set: Option<String>,
    /// A RichTextBlock's DecoratorClasses by their short names.
    pub decorators: Vec<String>,
    /// A BackgroundBlur: the tree's BlurStrength and bApplyAlphaToBlur, and the value SetBlurStrength or an
    /// animation gave it (over the tree's).
    pub blur: f64,
    pub blur_apply_alpha: bool,
    pub blur_set: Option<f64>,
}

impl Default for Widget {
    fn default() -> Widget {
        Widget {
            kind: String::new(),
            name: String::new(),
            parent: None,
            children: Vec::new(),
            vis: Vis::Visible,
            opacity: 1.0,
            color: [1.0; 4],
            text: String::new(),
            typeface: None,
            font_size: 24.0,
            outline: 0.0,
            outline_color: [0.0, 0.0, 0.0, 1.0],
            shadow_color: [0.0; 4],
            letter_spacing: 0.0,
            upper: false,
            tex: None,
            mid: None,
            brush_color: [1.0; 4],
            brush_tint: [1.0; 4],
            brush_w: 32.0,
            brush_h: 32.0,
            pad: Margin::default(),
            anchors: [0.0; 4],
            // UCanvasPanelSlot's default offsets.
            offsets: Margin { l: 0.0, t: 0.0, r: 100.0, b: 30.0 },
            fill: 0.0,
            active: 0,
            w_override: f64::NAN,
            h_override: f64::NAN,
            transform: RenderTransform::default(),
            shadow_offset: [1.0, 1.0],
            outline_shadows: false,
            h_align: HAlign::Fill,
            v_align: VAlign::Fill,
            align: [0.0; 2],
            font_mat: None,
            font_mid: None,
            separate_fill: false,
            even_digits: false,
            line_height: 1.0,
            justify: Justify::Left,
            wrap_at: 0.0,
            auto_wrap: false,
            min_text_w: 0.0,
            clip: false,
            clip_bleed: 0.0,
            solid: false,
            box_margin: None,
            min_w: f64::NAN,
            min_h: f64::NAN,
            max_w: f64::NAN,
            max_h: f64::NAN,
            stretch: Stretch::ScaleToFit,
            stretch_dir: StretchDir::Both,
            user_scale: 1.0,
            auto_size: false,
            quad: None,
            retained: None,
            paint_order: 0,
            keep_aspect: 0.0,
            pieces: 3,
            throb_h: true,
            throb_v: true,
            throb_opacity: true,
            radius: 16.0,
            period: 0.75,
            edit_hot: false,
            caret: -1,
            caret_time: 0.0,
            slot_pad: Margin::default(),
            min_slot: [0.0; 2],
            row: 0,
            col: 0,
            col_fill: Vec::new(),
            row_fill: Vec::new(),
            col_span: 1,
            row_span: 1,
            layer: 0,
            wrap_pad: [0.0; 2],
            wrap_size: -1.0,
            scroll: 0.0,
            scroll_max: 0.0,
            horiz: false,
            bar_h: 0.0,
            bar_style: None,
            scroll_state: Default::default(),
            style_set: None,
            decorators: Vec::new(),
            blur: 0.0,
            blur_apply_alpha: true,
            blur_set: None,
        }
    }
}

impl Widget {
    /// Neither Collapsed nor Hidden.
    pub fn shown(&self) -> bool {
        !matches!(self.vis, Vis::Collapsed | Vis::Hidden)
    }

    /// The text as drawn (TextTransformPolicy).
    pub fn shown_text(&self) -> String {
        if self.upper { self.text.to_uppercase() } else { self.text.clone() }
    }
}

/// The widget types the layout and the paint tell apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Ty {
    User,
    Quad,
    #[default]
    Panel,
    VerticalBox,
    HorizontalBox,
    SizeBox,
    ScaleBox,
    CanvasPanel,
    WidgetSwitcher,
    Border,
    Image,
    TextBlock,
    Spacer,
    ScrollBox,
    GridPanel,
    UniformGridPanel,
    WrapBox,
    Throbber,
    CircularThrobber,
    EditableTextBox,
    BackgroundBlur,
    RichTextBlock,
}

/// A widget's structure and layout state (the fields the HUD does not write).
#[derive(Clone, Debug)]
struct Node {
    /// Its place in the order the tree's widgets were made (`Tree::born`).
    born: u64,
    ty: Ty,
    /// The class of a user widget instance (lookups by name stop there).
    user: Option<Arc<str>>,
    /// Content placed in a user widget's named slot: it belongs to the tree that holds the instance.
    foreign: bool,
    /// A user widget instance: the roots of its named slots' content.
    named: Vec<WidgetId>,
    /// A user widget instance's properties over its class's defaults, shared with its recipe until an owner writes
    /// one (None: a plain widget).
    props: Option<Arc<Value>>,
    // the desired size, then the arranged rect relative to the parent's rect
    dw: f64,
    dh: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    /// The layout transform scale from a ScaleBox parent.
    ls: f64,
    /// A ScaleBox: the content scale of the last prepass (None: not computed), the child's desired size at the
    /// incoming scale, the area of the last arrange (-1: none yet).
    content_scale: Option<f64>,
    norm: [f64; 2],
    last: [f64; 2],
    /// The layout scale a text is measured at.
    text_scale: f64,
    /// The widget names a cursor of its own (its Cursor property is set).
    cursor: bool,
    /// A GridPanel: the desired cell sizes of the last measure (one more entry of 0: the total) and the cells'
    /// offsets of the last arrange.
    grid_cols: Vec<f64>,
    grid_rows: Vec<f64>,
    grid_x: Vec<f64>,
    grid_y: Vec<f64>,
    /// What the last measure saw of the widget's layout inputs (None: never measured since it was made).
    key: Option<Box<LayoutKey>>,
    /// The last measure of this widget no longer holds: an input of it or of a widget below it changed since, or an
    /// arrange changed what it reads. Set up to the top by `Tree::invalidate`.
    dirty: bool,
    /// Handed out by `widget_mut` since the last layout looked (it is in `Tree::touched`).
    touched: bool,
    /// The layout scale of its last measure (the bits).
    measured_at: Option<u64>,
    /// As the root of a layout: the area it was last arranged in (the bits; None: an arrange of another root has
    /// placed it since).
    arranged: Option<[u64; 4]>,
}

impl Default for Node {
    fn default() -> Node {
        Node {
            born: 0,
            ty: Ty::Panel,
            user: None,
            foreign: false,
            named: Vec::new(),
            props: None,
            dw: 0.0,
            dh: 0.0,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            ls: 1.0,
            content_scale: None,
            norm: [0.0; 2],
            last: [-1.0; 2],
            text_scale: 1.0,
            cursor: false,
            grid_cols: Vec::new(),
            grid_rows: Vec::new(),
            grid_x: Vec::new(),
            grid_y: Vec::new(),
            key: None,
            dirty: true,
            touched: false,
            measured_at: None,
            arranged: None,
        }
    }
}

/// The widget fields measure and arrange read, as bits (`LayoutKey::nums`).
const KEY_NUMS: usize = 61;

/// A widget's layout inputs as the last measure saw them: a written widget whose inputs differ from these lays its
/// root out again. Render transforms, opacity, colours, brushes and visibility other than Collapsed are not in it.
#[derive(Clone, Debug)]
struct LayoutKey {
    nums: [u64; KEY_NUMS],
    /// A ScrollBox's offset (arrange keeps it inside the content: the clamped one goes in).
    scroll: u64,
    text: String,
    typeface: Option<String>,
    children: Vec<WidgetId>,
    col_fill: Vec<f64>,
    row_fill: Vec<f64>,
}

impl LayoutKey {
    fn of(w: &Widget) -> LayoutKey {
        LayoutKey {
            nums: LayoutKey::nums(w),
            scroll: w.scroll.to_bits(),
            text: w.text.clone(),
            typeface: w.typeface.clone(),
            children: w.children.clone(),
            col_fill: w.col_fill.clone(),
            row_fill: w.row_fill.clone(),
        }
    }

    fn nums(w: &Widget) -> [u64; KEY_NUMS] {
        let b = |v: bool| u64::from(v);
        let i = |v: i32| u64::from(v.cast_unsigned());
        [
            b(w.vis == Vis::Collapsed),
            w.font_size.to_bits(),
            w.letter_spacing.to_bits(),
            w.outline.to_bits(),
            w.shadow_offset[0].to_bits(),
            w.shadow_offset[1].to_bits(),
            b(w.auto_wrap),
            w.wrap_at.to_bits(),
            b(w.separate_fill) | b(w.even_digits) << 1,
            b(w.upper),
            w.min_text_w.to_bits(),
            w.line_height.to_bits(),
            w.brush_w.to_bits(),
            w.brush_h.to_bits(),
            w.radius.to_bits(),
            u64::from(w.pieces),
            w.pad.l.to_bits(),
            w.pad.t.to_bits(),
            w.pad.r.to_bits(),
            w.pad.b.to_bits(),
            w.anchors[0].to_bits(),
            w.anchors[1].to_bits(),
            w.anchors[2].to_bits(),
            w.anchors[3].to_bits(),
            w.offsets.l.to_bits(),
            w.offsets.t.to_bits(),
            w.offsets.r.to_bits(),
            w.offsets.b.to_bits(),
            w.fill.to_bits(),
            i(w.active),
            w.w_override.to_bits(),
            w.h_override.to_bits(),
            w.min_w.to_bits(),
            w.min_h.to_bits(),
            w.max_w.to_bits(),
            w.max_h.to_bits(),
            w.keep_aspect.to_bits(),
            u64::from(h_axis(w.h_align)),
            u64::from(v_axis(w.v_align)),
            w.align[0].to_bits(),
            w.align[1].to_bits(),
            b(w.auto_size),
            w.stretch as u64,
            w.stretch_dir as u64,
            w.user_scale.to_bits(),
            w.bar_h.to_bits(),
            b(w.horiz),
            w.slot_pad.l.to_bits(),
            w.slot_pad.t.to_bits(),
            w.slot_pad.r.to_bits(),
            w.slot_pad.b.to_bits(),
            w.min_slot[0].to_bits(),
            w.min_slot[1].to_bits(),
            i(w.row),
            i(w.col),
            i(w.col_span),
            i(w.row_span),
            i(w.layer),
            w.wrap_pad[0].to_bits(),
            w.wrap_pad[1].to_bits(),
            w.wrap_size.to_bits(),
        ]
    }

    fn matches(&self, w: &Widget) -> bool {
        let same =
            |a: &[f64], b: &[f64]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits());
        self.nums == LayoutKey::nums(w)
            && self.scroll == w.scroll.to_bits()
            && self.text == w.text
            && self.typeface == w.typeface
            && self.children == w.children
            && same(&self.col_fill, &w.col_fill)
            && same(&self.row_fill, &w.row_fill)
    }

    /// The key taken again from `w`, keeping its buffers.
    fn set(&mut self, w: &Widget) {
        self.nums = LayoutKey::nums(w);
        self.scroll = w.scroll.to_bits();
        self.text.clone_from(&w.text);
        self.typeface.clone_from(&w.typeface);
        self.children.clone_from(&w.children);
        self.col_fill.clone_from(&w.col_fill);
        self.row_fill.clone_from(&w.row_fill);
    }
}

/// No widget (a link's parent or active child).
const NO_LINK: u32 = u32::MAX;
/// A link's active child when its widget is not a WidgetSwitcher: every child counts.
const ANY_CHILD: u32 = u32::MAX - 1;

/// What `visible_in_hierarchy` reads of a widget, kept small beside the widgets: its parent, shown, and the child a
/// WidgetSwitcher shows.
#[derive(Clone, Copy, Debug)]
struct Link {
    parent: u32,
    shown: bool,
    active: u32,
}

/// The widgets' links, brought up to date from the widgets written since the last walk (`stale`).
#[derive(Clone, Debug, Default)]
struct Links {
    up: Vec<Link>,
    stale: Vec<WidgetId>,
    is_stale: Vec<bool>,
}

impl Links {
    fn mark(&mut self, w: WidgetId) {
        let i = w.index();
        if self.is_stale.len() <= i {
            self.is_stale.resize(i + 1, false);
        }
        if !self.is_stale[i] {
            self.is_stale[i] = true;
            self.stale.push(w);
        }
    }

    fn update(&mut self, widgets: &[Widget], nodes: &[Node]) {
        if self.up.len() < widgets.len() {
            self.up.resize(widgets.len(), Link { parent: NO_LINK, shown: false, active: ANY_CHILD });
        }
        for w in self.stale.drain(..) {
            let i = w.index();
            self.is_stale[i] = false;
            let x = &widgets[i];
            let active = if nodes[i].ty == Ty::WidgetSwitcher {
                usize::try_from(x.active).ok().and_then(|a| x.children.get(a)).map_or(NO_LINK, |c| c.i)
            } else {
                ANY_CHILD
            };
            self.up[i] = Link { parent: x.parent.map_or(NO_LINK, |p| p.i), shown: x.shown(), active };
        }
    }
}

/// What a tree needs of its bundle after the instantiate.
#[derive(Debug, Default)]
struct Refs {
    /// The materials' object paths (a brush that names one is a material brush).
    materials: HashSet<String>,
    /// The Slate style's brush textures (the scroll bar's, the text box's, the throbber's piece): bundle name -> entry.
    styles: Vec<(&'static str, String)>,
    /// MI_Quad_Masked's object path: an outline quad's RetainerBox effect material.
    quad_masked: String,
    /// The bundle's material and texture names -> the object path or entry each resolves to (`resolve`).
    paths: HashMap<String, String>,
}

/// The bundle's names of the engine Slate style's textures.
pub const SLATE_TEXTURES: [&str; 5] = [
    "Slate_Scrollbar_Thumb",
    "Slate_Scrollbar_Background_Vertical",
    "Slate_TextBox",
    "Slate_TextBox_Hovered",
    "Slate_Throbber_Piece",
];

impl Refs {
    fn of(b: &Bundle) -> Refs {
        let styles =
            SLATE_TEXTURES.into_iter().filter_map(|n| b.textures.get(n).map(|t| (n, t.entry().to_string()))).collect();
        let quad_masked = resolve(b, "MI_Quad_Masked");
        let paths = b
            .materials
            .iter()
            .map(|(n, p)| (n.clone(), p.clone()))
            .chain(b.textures.iter().map(|(n, t)| (n.clone(), t.entry().to_string())))
            .collect();
        Refs { materials: b.materials.values().cloned().collect(), styles, quad_masked, paths }
    }

    /// A brush's resource as the batches draw it: a bundle name a controller set at run time (UI_SectionBarVertButton's
    /// icon, UI_IconButton's) resolved as the bundle's brushes are
    /// at the instantiate; an object path or entry as it is.
    fn path<'a>(&'a self, tex: &'a str) -> &'a str {
        self.paths.get(tex).map_or(tex, String::as_str)
    }

    /// The entry of a Slate style texture by its bundle name.
    fn slate(&self, name: &str) -> Option<&str> {
        self.styles.iter().find(|(n, _)| *n == name).map(|(_, e)| e.as_str())
    }
}

/// The last paint of a widget: its accumulated render transform (device pixels; x' = x a + y c + e) and its
/// arranged size (Slate units).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Painted {
    pub transform: [f64; 6],
    pub size: [f64; 2],
}

/// A widget class's tree, its widgets by index; the root is the first made.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    widgets: Vec<Widget>,
    nodes: Vec<Node>,
    painted: RefCell<Vec<Option<Painted>>>,
    refs: Arc<Refs>,
    /// The view's DPI scale of the last layout.
    scale: f64,
    /// The program's clock at the paint, s (the throbbers' phase: PaintEnv.time).
    time: f64,
    /// The ids `destroy` gave back (their generations moved on), taken again by the next widgets made.
    free: Vec<WidgetId>,
    /// Each index's generation: even while a widget holds it, odd while it is free (no id is handed out under an
    /// odd one); an id of another generation is stale.
    gens: Vec<u32>,
    /// The widgets made so far (each widget's `Node::born`).
    births: u64,
    /// A WrapBox's slots while it arranges (kept: no allocation per layout).
    wrap_slots: Vec<WrapSlot>,
    /// Counts the changes of the tree's shape: a widget added to a parent or taken out of one (Tree.version).
    shape: u64,
    /// The widgets `widget_mut` handed out since the last layout compared them with their layout keys.
    touched: Vec<WidgetId>,
    /// The measures and arranges run (a layout that held is not run).
    layouts: u64,
    /// The parent links of the visibility walk.
    links: RefCell<Links>,
    /// The widget classes' recipes, each made on the class's first CreateWidget (the tree's bundle is fixed).
    recipes: Recipes,
}

/// One widget of a recipe: its widget and node made from the bundle's data once, copied into a tree by each
/// CreateWidget that makes it (`Tree::stamp`, which takes the steps building it from the data would).
#[derive(Debug)]
enum Item {
    /// A widget of an engine type, then its children's (an EditableTextBox's text child first).
    Plain { w: Widget, n: Node, kids: Vec<Item> },
    /// A user widget instance: its own widget, its class's tree (None: WBP_Quadrilateral, or a class the bundle
    /// lacks), the instance's UUserWidget Padding and its named slots' content.
    User { w: Widget, n: Node, class: Option<Arc<Item>>, pad: Option<Margin>, named: Vec<(String, Item)> },
}

/// The recipes of the widget classes by class name: `classes` the class's own tree (under an instance's widget),
/// `bare` an instance with no instance properties (CreateWidget's).
#[derive(Clone, Debug, Default)]
struct Recipes {
    classes: HashMap<String, Arc<Item>>,
    bare: HashMap<String, Arc<Item>>,
}

static NULL: Value = Value::Null;

impl Tree {
    pub fn root(&self) -> WidgetId {
        WidgetId { i: 0, g: self.gens.first().copied().unwrap_or(0) }
    }

    /// The index of `w`; panics on an id whose widget was destroyed (a holder that outlived it: a bug of the port).
    fn at(&self, w: WidgetId) -> usize {
        let i = w.index();
        match self.gens.get(i) {
            Some(&g) if g == w.g => i,
            _ => panic!("ui: widget id {w:?} is stale or of no widget of this tree (its widget was destroyed)"),
        }
    }

    /// `w` names a widget of the tree (not one destroyed).
    pub fn alive(&self, w: WidgetId) -> bool {
        self.gens.get(w.index()).is_some_and(|&g| g == w.g)
    }

    /// The widgets of the tree in index order (the destroyed ones left out).
    pub fn ids(&self) -> impl Iterator<Item = WidgetId> + '_ {
        self.gens
            .iter()
            .enumerate()
            .filter(|&(_, &g)| g % 2 == 0)
            .map(|(i, &g)| WidgetId { i: u32::try_from(i).expect("widgets past u32"), g })
    }

    /// The widgets in use (made and not destroyed).
    pub fn in_use(&self) -> usize {
        self.widgets.len() - self.free.len()
    }

    /// The widgets made so far, the destroyed ones counted.
    pub fn births(&self) -> u64 {
        self.births
    }

    /// The order `w` was made in among the tree's widgets (an index taken again is no order).
    pub fn born(&self, w: WidgetId) -> u64 {
        self.node(w).born
    }

    /// The first widget named `name` in the tree, depth first; panics when it has none (Widget.find).
    pub fn find(&self, name: &str) -> WidgetId {
        self.find_in(self.root(), name)
    }

    /// The first widget named `name` under `under` in its own tree (not inside nested user widgets), depth first;
    /// panics when it has none.
    pub fn find_in(&self, under: WidgetId, name: &str) -> WidgetId {
        self.lookup(under, under, name)
            .unwrap_or_else(|| panic!("hud: no widget {name} in {}", self.widget(under).name))
    }

    /// Widget.find's walk (findIn).
    fn lookup(&self, root: WidgetId, w: WidgetId, name: &str) -> Option<WidgetId> {
        if self.widget(w).name == name {
            return Some(w);
        }
        let n = &self.nodes[self.at(w)];
        let own = n.user.is_none() || w == root;
        let list = if own { &self.widget(w).children } else { &n.named };
        for &c in list {
            if own && self.nodes[c.index()].foreign {
                continue;
            }
            if let Some(r) = self.lookup(root, c, name) {
                return Some(r);
            }
        }
        None
    }

    pub fn widget(&self, w: WidgetId) -> &Widget {
        &self.widgets[self.at(w)]
    }

    /// `w` to write: the next layout compares it with what its last measure saw, the next visibility walk reads
    /// it again.
    pub fn widget_mut(&mut self, w: WidgetId) -> &mut Widget {
        self.touch(w);
        &mut self.widgets[w.index()]
    }

    fn touch(&mut self, w: WidgetId) {
        let i = self.at(w);
        let n = &mut self.nodes[i];
        if !n.touched {
            n.touched = true;
            self.touched.push(w);
        }
        self.links.get_mut().mark(w);
    }

    /// The widgets written since the last layout compared with their layout keys: one whose inputs changed
    /// invalidates the layout of every widget above it. A widget never measured has no layout to invalidate (it was
    /// just made, or is under a Collapsed one: the write that attaches or shows it changes a measured widget's key).
    fn settle(&mut self) {
        let mut touched = std::mem::take(&mut self.touched);
        for w in touched.drain(..) {
            let i = w.index();
            let n = &mut self.nodes[i];
            n.touched = false;
            let x = &self.widgets[i];
            if let Some(k) = &mut n.key
                && !k.matches(x)
            {
                k.set(x);
                self.invalidate(w);
            }
        }
        self.touched = touched;
    }

    /// A measure's visit of `w` at the layout `scale`: its layout key taken, its measure valid, its arrange as a
    /// root to run again.
    fn seen(&mut self, w: WidgetId, scale: f64) {
        let i = w.index();
        let (n, x) = (&mut self.nodes[i], &self.widgets[i]);
        match &mut n.key {
            Some(k) => k.set(x),
            None => n.key = Some(Box::new(LayoutKey::of(x))),
        }
        n.dirty = false;
        n.measured_at = Some(scale.to_bits());
        n.arranged = None;
    }

    /// A layout of root `w` run, arranged in `rect` (None: measured only). Every widget above it is invalid: an
    /// enclosing root's last layout did not leave its subtree so.
    fn laid_out(&mut self, w: WidgetId, rect: Option<[u64; 4]>) {
        self.layouts += 1;
        self.nodes[w.index()].arranged = rect;
        if let Some(p) = self.widgets[w.index()].parent {
            self.invalidate(p);
        }
    }

    /// The layout of `w` and every widget above it no longer holds.
    fn invalidate(&mut self, w: WidgetId) {
        let mut c = Some(w);
        while let Some(x) = c {
            self.nodes[x.index()].dirty = true;
            c = self.widgets[x.index()].parent;
        }
    }

    /// GetDynamicMaterial: the brush's material's dynamic instance, made on first use. A WBP_Quadrilateral's gets
    /// SetParameters' corner scalars.
    pub fn dynamic_material(&mut self, w: WidgetId) -> &mut Mid {
        let refs = self.refs.clone();
        let x = self.widget_mut(w);
        if x.mid.is_none() {
            let path =
                x.tex.as_deref().map(|t| refs.path(t)).filter(|t| refs.materials.contains(*t)).map(str::to_string);
            let path = path.unwrap_or_else(|| panic!("hud: {} has no material brush", x.name));
            let mut m = Mid::of(&path);
            for (k, v) in x.quad.iter().flatten() {
                m.set_scalar(k, *v);
            }
            x.mid = Some(m);
        }
        x.mid.as_mut().expect("made above")
    }

    /// UTextBlock GetDynamicFontMaterial: the font's material as a dynamic instance, made on first use.
    pub fn dynamic_font_material(&mut self, w: WidgetId) -> &mut Mid {
        let x = self.widget_mut(w);
        if x.font_mid.is_none() {
            let path = x.font_mat.clone().unwrap_or_else(|| panic!("hud: {} has no font material", x.name));
            x.font_mid = Some(Mid::of(&path));
        }
        x.font_mid.as_mut().expect("made above")
    }

    /// A user widget instance's properties as the bundle holds them (its class's defaults under the instance's).
    pub fn props(&self, w: WidgetId) -> &Value {
        if !self.alive(w) {
            return &NULL;
        }
        self.nodes.get(w.index()).map_or(&NULL, |n| n.props.as_deref().unwrap_or(&NULL))
    }

    /// Takes `w` out of its parent's children (Widget.remove); its id stays good.
    pub fn remove(&mut self, w: WidgetId) {
        let Some(p) = self.widget(w).parent else { return };
        self.shape += 1;
        // (a widget destroyed and a new one made under the same id in the same slot leaves the parent's children as
        // they were: the parent's key would match, so it is marked here)
        self.invalidate(p);
        self.widget_mut(p).children.retain(|&c| c != w);
        self.widget_mut(w).parent = None;
    }

    /// `w` and every widget under it taken out of the tree for good: their ids are given back for the next widgets
    /// made, so nothing may hold them (a widget the tree made for its own content, as a RichTextBlock's lines).
    pub fn destroy(&mut self, w: WidgetId) {
        self.remove(w);
        let mut stack = vec![w];
        while let Some(x) = stack.pop() {
            let i = self.at(x);
            let widget = &mut self.widgets[i];
            stack.append(&mut widget.children);
            widget.parent = None;
            widget.vis = Vis::Collapsed;
            widget.text = String::new();
            self.nodes[i] = Node::default();
            self.painted.borrow_mut()[i] = None;
            self.links.get_mut().mark(x);
            // (the index is free under an odd generation and its next widget gets the even one after it: `x` and
            // every copy of it go stale)
            self.gens[i] = self.gens[i].wrapping_add(1);
            self.free.push(WidgetId { i: x.i, g: self.gens[i].wrapping_add(1) });
        }
    }

    /// Appends `child` to `parent`'s children (Widget.add).
    pub fn add(&mut self, parent: WidgetId, child: WidgetId) {
        self.remove(child);
        self.widget_mut(child).parent = Some(parent);
        let at = self.slot_index(parent, child, None);
        self.widget_mut(parent).children.insert(at, child);
        self.shape += 1;
    }

    /// The count of the tree's shape changes (Tree.version): a widget added to a parent or taken out of one.
    pub fn shape(&self) -> u64 {
        self.shape
    }

    /// CreateWidget of `class` named `name`, added as the last child of `parent`.
    pub fn instantiate_in(&mut self, b: &Bundle, parent: WidgetId, class: &str, name: &str) -> WidgetId {
        self.create_user(b, Some(parent), class, name)
    }

    /// The program's clock for the paint, s (the throbbers' phase).
    pub fn set_time(&mut self, time: f64) {
        self.time = time;
    }

    /// The desired size of the last layout.
    pub fn desired_size(&self, w: WidgetId) -> [f64; 2] {
        let n = self.node(w);
        [n.dw, n.dh]
    }

    /// The arranged rect of the last layout relative to the parent's: x, y, width, height (Slate units).
    pub fn arranged(&self, w: WidgetId) -> [f64; 4] {
        let n = self.node(w);
        [n.x, n.y, n.w, n.h]
    }

    /// The last paint's transform and size of `w` (None: not painted yet).
    pub fn painted(&self, w: WidgetId) -> Option<Painted> {
        self.painted.borrow().get(self.at(w)).copied().flatten()
    }

    fn node(&self, w: WidgetId) -> &Node {
        &self.nodes[self.at(w)]
    }

    fn node_mut(&mut self, w: WidgetId) -> &mut Node {
        let i = self.at(w);
        &mut self.nodes[i]
    }

    fn push(&mut self, parent: Option<WidgetId>, mut w: Widget, n: Node) -> WidgetId {
        w.parent = parent;
        let id = self.place(w, n);
        self.links.get_mut().mark(id);
        if let Some(p) = parent {
            let at = self.slot_index(p, id, None);
            self.widget_mut(p).children.insert(at, id);
            self.shape += 1;
        }
        id
    }

    /// `w` and `n` stored under a free id (the last one `destroy` gave back) or a new one.
    fn place(&mut self, w: Widget, mut n: Node) -> WidgetId {
        n.born = self.births;
        self.births += 1;
        if let Some(id) = self.free.pop() {
            self.widgets[id.index()] = w;
            self.nodes[id.index()] = n;
            self.gens[id.index()] = id.g;
            id
        } else {
            let id = WidgetId { i: u32::try_from(self.widgets.len()).expect("widgets past u32"), g: 0 };
            self.widgets.push(w);
            self.nodes.push(n);
            self.gens.push(0);
            self.painted.borrow_mut().push(None);
            id
        }
    }

    /// CreateWidget of user widget class `class` named `name` with no instance properties, in `parent` (as its last
    /// child) or in none: the class's recipe stamped (`stamp`).
    fn create_user(&mut self, b: &Bundle, parent: Option<WidgetId>, class: &str, name: &str) -> WidgetId {
        if self.refs.materials.is_empty() {
            self.refs = Arc::new(Refs::of(b));
        }
        let it = match self.recipes.bare.get(class) {
            Some(it) => it.clone(),
            None => {
                let empty = Value::Object(Default::default());
                let it = Arc::new(user_item(&mut self.recipes, b, class, class, &empty, &empty, "", None));
                self.recipes.bare.insert(class.to_string(), it.clone());
                it
            }
        };
        let w = self.stamp(&it, parent);
        if name != class {
            name.clone_into(&mut self.widget_mut(w).name);
        }
        w
    }

    /// Recipe item `it` made into the tree in `parent` (as its last child) or in none, as building its data would:
    /// each widget pushed in the same order (the same ids, children orders and shape count), a user widget's
    /// Padding and named slots set after its class's tree.
    fn stamp(&mut self, it: &Item, parent: Option<WidgetId>) -> WidgetId {
        match it {
            Item::Plain { w, n, kids } => {
                let id = self.push(parent, w.clone(), n.clone());
                for k in kids {
                    self.stamp(k, Some(id));
                }
                id
            }
            Item::User { w, n, class, pad, named } => {
                let id = self.push(parent, w.clone(), n.clone());
                if let Some(c) = class {
                    self.stamp(c, Some(id));
                }
                if let Some(m) = pad {
                    set_user_padding(self, id, *m);
                }
                // UUserWidget::Initialize: each NamedSlotBinding's content becomes the content of the NamedSlot of
                // that name.
                for (slot, c) in named {
                    let at = self.find_in(id, slot);
                    let c = self.stamp(c, Some(at));
                    self.node_mut(c).foreign = true;
                    self.node_mut(id).named.push(c);
                }
                id
            }
        }
    }

    /// Where `child` goes among `parent`'s children: `at` (the end when None or past it), but in a GridPanel after
    /// the last slot of its Layer or lower (SGridPanel::InsertSlot: paint and hit order go by Layer).
    fn slot_index(&self, parent: WidgetId, child: WidgetId, at: Option<usize>) -> usize {
        let kids = || self.widget(parent).children.iter().filter(move |c| **c != child);
        let n = kids().count();
        if self.node(parent).ty == Ty::GridPanel {
            // (after the last of the children not above its Layer: those above it, at the end, stay after it)
            let layer = self.widget(child).layer;
            let above = kids().rev().take_while(|c| self.widget(**c).layer > layer).count();
            return n - above;
        }
        at.map_or(n, |i| i.min(n))
    }

    /// The children that take part in layout: not Collapsed; a WidgetSwitcher's active child only.
    fn kids(&self, w: WidgetId) -> Vec<WidgetId> {
        let x = self.widget(w);
        if self.node(w).ty == Ty::WidgetSwitcher {
            return usize::try_from(x.active)
                .ok()
                .and_then(|i| x.children.get(i))
                .filter(|c| self.widget(**c).vis != Vis::Collapsed)
                .copied()
                .into_iter()
                .collect();
        }
        x.children.iter().copied().filter(|c| self.widget(*c).vis != Vis::Collapsed).collect()
    }
}

// ---------- instantiate ----------

fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k)?.as_f64()
}

fn xy(v: Option<&Value>) -> Option<[Option<f64>; 2]> {
    let v = v?;
    Some([num(v, "X"), num(v, "Y")])
}

/// The tail of an enum value's name (`ESlateVisibility::Collapsed` -> `Collapsed`); empty for none.
fn tail(v: Option<&Value>) -> &str {
    v.and_then(Value::as_str).map_or("", |s| s.rsplit("::").next().unwrap_or(""))
}

fn margin_of(m: Option<&Value>) -> Margin {
    let g = |k| m.and_then(|m| num(m, k)).unwrap_or(0.0);
    Margin { l: g("Left"), t: g("Top"), r: g("Right"), b: g("Bottom") }
}

/// A linear colour (FLinearColor's members; a missing one 0, alpha 1); `d` when there is none.
fn rgba_of(c: Option<&Value>, d: Rgba) -> Rgba {
    match c {
        Some(c) if c.is_object() => [
            num(c, "R").unwrap_or(0.0),
            num(c, "G").unwrap_or(0.0),
            num(c, "B").unwrap_or(0.0),
            num(c, "A").unwrap_or(1.0),
        ],
        _ => d,
    }
}

/// A brush's ResourceObject (a bundle name) as the object path the batches draw.
pub(crate) fn resolve(b: &Bundle, name: &str) -> String {
    if let Some(p) = b.materials.get(name) {
        return p.clone();
    }
    if let Some(t) = b.textures.get(name) {
        return t.entry().to_string();
    }
    name.to_string()
}

// Widget classes whose constructor sets Visibility SelfHitTestInvisible (panels, content boxes, user widgets); the
// others start Visible.
// Unverified: from the engine's public sources, not read from the exe's constructors.
fn self_invisible(t: &str) -> bool {
    matches!(
        t,
        "User"
            | "Quad"
            | "Overlay"
            | "CanvasPanel"
            | "HorizontalBox"
            | "DynamicEntryBox"
            | "VerticalBox"
            | "GridPanel"
            | "UniformGridPanel"
            | "WrapBox"
            | "SizeBox"
            | "ScaleBox"
            | "WidgetSwitcher"
            | "InvalidationBox"
            | "RetainerBox"
            | "NamedSlot"
            | "Spacer"
            | "BackgroundBlur"
            | "SafeZone"
    )
}

fn h_align(s: &str) -> Option<HAlign> {
    Some(match s {
        "HAlign_Left" => HAlign::Left,
        "HAlign_Center" => HAlign::Center,
        "HAlign_Right" => HAlign::Right,
        "HAlign_Fill" => HAlign::Fill,
        _ => return None,
    })
}

fn v_align(s: &str) -> Option<VAlign> {
    Some(match s {
        "VAlign_Top" => VAlign::Top,
        "VAlign_Center" => VAlign::Center,
        "VAlign_Bottom" => VAlign::Bottom,
        "VAlign_Fill" => VAlign::Fill,
        _ => return None,
    })
}

fn stretch_of(s: &str) -> Stretch {
    match s {
        "None" => Stretch::None,
        "Fill" => Stretch::Fill,
        "ScaleToFitX" => Stretch::ScaleToFitX,
        "ScaleToFitY" => Stretch::ScaleToFitY,
        "ScaleToFill" => Stretch::ScaleToFill,
        "ScaleBySafeZone" => Stretch::ScaleBySafeZone,
        "UserSpecified" => Stretch::UserSpecified,
        _ => Stretch::ScaleToFit,
    }
}

/// The Widget constructor: the widget of type `t` from its properties `p`, its slot's `s` and
/// the slot's type.
fn make_widget(b: &Bundle, t: &str, name: &str, p: &Value, s: &Value, slot: &str) -> Widget {
    let mut w = Widget { kind: t.to_string(), name: name.to_string(), ..Widget::default() };
    // UButtonSlot's default padding is (4, 2); the other slots' is 0.
    w.pad = if slot == "ButtonSlot" && s.get("Padding").is_none() {
        Margin { l: 4.0, t: 2.0, r: 4.0, b: 2.0 }
    } else {
        margin_of(s.get("Padding"))
    };
    let (dh, dv) = match slot {
        "OverlaySlot" | "UniformGridSlot" => (HAlign::Left, VAlign::Top),
        "ScaleBoxSlot" | "ButtonSlot" => (HAlign::Center, VAlign::Center),
        _ => (HAlign::Fill, VAlign::Fill),
    };
    w.h_align = h_align(tail(s.get("HorizontalAlignment"))).unwrap_or(dh);
    w.v_align = v_align(tail(s.get("VerticalAlignment"))).unwrap_or(dv);
    let size = s.get("Size");
    w.fill = if tail(size.and_then(|z| z.get("SizeRule"))) == "Fill" {
        size.and_then(|z| num(z, "Value")).unwrap_or(1.0)
    } else {
        0.0
    };
    w.vis = Vis::by_name(tail(p.get("Visibility"))).unwrap_or(if self_invisible(t) {
        Vis::SelfHitTestInvisible
    } else {
        Vis::Visible
    });
    if let Some(rt) = p.get("RenderTransform") {
        let tr = xy(rt.get("Translation")).unwrap_or([None; 2]);
        let sc = xy(rt.get("Scale")).unwrap_or([None; 2]);
        let sh = xy(rt.get("Shear")).unwrap_or([None; 2]);
        w.transform.translation = [tr[0].unwrap_or(0.0), tr[1].unwrap_or(0.0)];
        w.transform.scale = [sc[0].unwrap_or(1.0), sc[1].unwrap_or(1.0)];
        w.transform.shear = [sh[0].unwrap_or(0.0), sh[1].unwrap_or(0.0)];
        w.transform.angle = num(rt, "Angle").unwrap_or(0.0);
    }
    if let Some(pv) = xy(p.get("RenderTransformPivot")) {
        w.transform.pivot = [pv[0].unwrap_or(0.5), pv[1].unwrap_or(0.5)];
    }
    w.opacity = num(p, "RenderOpacity").unwrap_or(1.0);
    let c = p.get("ColorAndOpacity");
    let c = c.and_then(|c| c.get("SpecifiedColor")).or(c.filter(|c| c.get("R").is_some()));
    w.color = rgba_of(c, [1.0; 4]);
    if let Some(br) = p.get("Brush") {
        let sz = xy(br.get("ImageSize")).unwrap_or([None; 2]);
        w.brush_w = sz[0].unwrap_or(32.0);
        w.brush_h = sz[1].unwrap_or(32.0);
        w.tex = br.get("ResourceObject").and_then(Value::as_str).map(|n| resolve(b, n));
        w.brush_tint = rgba_of(br.get("TintColor").and_then(|c| c.get("SpecifiedColor")), [1.0; 4]);
    }
    match t {
        "TextBlock" => {
            let tx = p.get("Text");
            w.text = tx
                .and_then(|x| x.get("CultureInvariantString").or(x.get("SourceString")))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let f = p.get("Font").unwrap_or(&NULL);
            w.typeface = f.get("TypefaceFontName").and_then(Value::as_str).map(str::to_string);
            w.font_size = num(f, "Size").unwrap_or(24.0);
            let o = f.get("OutlineSettings").unwrap_or(&NULL);
            w.outline = num(o, "OutlineSize").unwrap_or(0.0);
            w.outline_color = rgba_of(o.get("OutlineColor"), [0.0, 0.0, 0.0, 1.0]);
            w.outline_shadows = o.get("bApplyOutlineToDropShadows").and_then(Value::as_bool).unwrap_or(false);
            w.separate_fill = o.get("bSeparateFillAlpha").and_then(Value::as_bool).unwrap_or(false);
            let so = xy(p.get("ShadowOffset")).unwrap_or([None; 2]);
            w.shadow_offset = [so[0].unwrap_or(1.0), so[1].unwrap_or(1.0)];
            w.shadow_color = rgba_of(p.get("ShadowColorAndOpacity"), [0.0; 4]);
            w.letter_spacing = num(f, "LetterSpacing").unwrap_or(0.0);
            w.font_mat = f.get("FontMaterial").and_then(Value::as_str).map(|n| resolve(b, n));
            w.upper = tail(p.get("TextTransformPolicy")) == "ToUpper";
            w.line_height = num(p, "LineHeightPercentage").unwrap_or(1.0);
            w.justify = match tail(p.get("Justification")) {
                "Center" => Justify::Center,
                "Right" => Justify::Right,
                _ => Justify::Left,
            };
            w.wrap_at = num(p, "WrapTextAt").unwrap_or(0.0);
            w.auto_wrap = p.get("AutoWrapText").and_then(Value::as_bool).unwrap_or(false);
        }
        "Border" => {
            // The content padding is the BorderSlot's (UBorder keeps the two in sync).
            let bg = p.get("Background").unwrap_or(&NULL);
            let draw_as = tail(bg.get("DrawAs"));
            if draw_as != "NoDrawType" {
                let res = bg.get("ResourceObject").and_then(Value::as_str);
                w.solid = res.is_none();
                w.tex = res.map(|n| resolve(b, n));
                let sz = xy(bg.get("ImageSize")).unwrap_or([None; 2]);
                w.brush_w = sz[0].unwrap_or(32.0);
                w.brush_h = sz[1].unwrap_or(32.0);
                w.box_margin = (draw_as == "Box").then(|| margin_of(bg.get("Margin")));
            }
            w.brush_color = rgba_of(p.get("BrushColor"), [1.0; 4]);
            // A brush without a resource: its own tint multiplies in (FSlateBrush.TintColor x BrushColor).
            let tint = bg.get("TintColor").and_then(|c| c.get("SpecifiedColor"));
            if w.solid && tint.is_some() {
                let k = rgba_of(tint, [1.0; 4]);
                w.brush_color = std::array::from_fn(|i| w.brush_color[i] * k[i]);
            }
        }
        "SizeBox" => {
            let ov = |k: &str, v: &str| {
                if p.get(k).and_then(Value::as_bool).unwrap_or(false) { num(p, v).unwrap_or(0.0) } else { f64::NAN }
            };
            w.w_override = ov("bOverride_WidthOverride", "WidthOverride");
            w.h_override = ov("bOverride_HeightOverride", "HeightOverride");
            w.min_w = ov("bOverride_MinDesiredWidth", "MinDesiredWidth");
            w.min_h = ov("bOverride_MinDesiredHeight", "MinDesiredHeight");
            w.max_w = ov("bOverride_MaxDesiredWidth", "MaxDesiredWidth");
            w.max_h = ov("bOverride_MaxDesiredHeight", "MaxDesiredHeight");
        }
        "ScaleBox" => {
            w.stretch = stretch_of(tail(p.get("Stretch")));
            w.stretch_dir = match tail(p.get("StretchDirection")) {
                "DownOnly" => StretchDir::DownOnly,
                "UpOnly" => StretchDir::UpOnly,
                _ => StretchDir::Both,
            };
            w.user_scale = num(p, "UserSpecifiedScale").unwrap_or(1.0);
            // SScaleBox::OnPaint (exe 1417f63f0): these stretches clip the content to the box when its own
            // clipping is Inherit.
            if matches!(w.stretch, Stretch::ScaleToFitX | Stretch::ScaleToFitY | Stretch::ScaleToFill) {
                w.clip = true;
            }
        }
        "Spacer" => {
            let sz = xy(p.get("Size")).unwrap_or([None; 2]);
            w.brush_w = sz[0].unwrap_or(1.0);
            w.brush_h = sz[1].unwrap_or(1.0);
        }
        "WidgetSwitcher" => w.active = num(p, "ActiveWidgetIndex").unwrap_or(0.0) as i32,
        "Throbber" => {
            let img = p.get("Image").unwrap_or(&NULL);
            let sz = xy(img.get("ImageSize")).unwrap_or([None; 2]);
            w.brush_w = sz[0].unwrap_or(32.0);
            w.brush_h = sz[1].unwrap_or(32.0);
            w.tex = img.get("ResourceObject").and_then(Value::as_str).map(|n| resolve(b, n));
            w.color = rgba_of(img.get("TintColor").and_then(|c| c.get("SpecifiedColor")), [1.0; 4]);
            w.pieces = num(p, "NumberOfPieces").map_or(3, |v| v as u32);
            w.throb_h = p.get("bAnimateHorizontally").and_then(Value::as_bool).unwrap_or(true);
            w.throb_v = p.get("bAnimateVertically").and_then(Value::as_bool).unwrap_or(true);
            w.throb_opacity = p.get("bAnimateOpacity").and_then(Value::as_bool).unwrap_or(true);
        }
        "CircularThrobber" => {
            // UCircularThrobber's defaults (as SCircularThrobber's, exe 14120e240): 6 pieces, period 0.75, radius 16;
            // the piece is FCoreStyle "Throbber.CircleChunk" (Common/Throbber_Piece, 8 x 8) unless the Image brush
            // names one.
            let img = p.get("Image").unwrap_or(&NULL);
            let sz = xy(img.get("ImageSize")).unwrap_or([None; 2]);
            w.brush_w = sz[0].unwrap_or(8.0);
            w.brush_h = sz[1].unwrap_or(8.0);
            w.tex = Some(
                img.get("ResourceObject")
                    .and_then(Value::as_str)
                    .map_or_else(|| slate_entry(b, "Slate_Throbber_Piece"), |n| resolve(b, n)),
            );
            w.color = rgba_of(img.get("TintColor").and_then(|c| c.get("SpecifiedColor")), [1.0; 4]);
            w.pieces = num(p, "NumberOfPieces").map_or(6, |v| v as u32);
            w.radius = num(p, "Radius").unwrap_or(16.0);
            w.period = num(p, "Period").unwrap_or(0.75);
        }
        "EditableTextBox" => {
            w.brush_color = rgba_of(
                p.get("WidgetStyle").and_then(|s| s.get("BackgroundColor")).and_then(|c| c.get("SpecifiedColor")),
                [1.0; 4],
            );
            w.clip = true;
        }
        "RichTextBlock" => {
            w.text = p
                .get("Text")
                .and_then(|x| x.get("CultureInvariantString").or(x.get("SourceString")))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            w.style_set = p.get("TextStyleSet").and_then(Value::as_str).map(str::to_string);
            w.decorators = p
                .get("DecoratorClasses")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(|c| c.rsplit(['.', '/']).next().unwrap_or("").to_string())
                        .collect()
                })
                .unwrap_or_default();
            w.line_height = num(p, "LineHeightPercentage").unwrap_or(1.0);
            w.justify = match tail(p.get("Justification")) {
                "Center" => Justify::Center,
                "Right" => Justify::Right,
                _ => Justify::Left,
            };
            w.wrap_at = num(p, "WrapTextAt").unwrap_or(0.0);
            w.auto_wrap = p.get("AutoWrapText").and_then(Value::as_bool).unwrap_or(false);
        }
        "UniformGridPanel" => {
            w.slot_pad = margin_of(p.get("SlotPadding"));
            w.min_slot = [num(p, "MinDesiredSlotWidth").unwrap_or(0.0), num(p, "MinDesiredSlotHeight").unwrap_or(0.0)];
        }
        "WrapBox" => {
            let ip = xy(p.get("InnerSlotPadding")).unwrap_or([None; 2]);
            w.wrap_pad = [ip[0].unwrap_or(0.0), ip[1].unwrap_or(0.0)];
            // UWrapBox: bExplicitWrapSize takes WrapSize (constructor default 500), else the allotted width
            w.wrap_size = if p.get("bExplicitWrapSize").and_then(Value::as_bool).unwrap_or(false) {
                num(p, "WrapSize").unwrap_or(500.0)
            } else {
                -1.0
            };
        }
        "GridPanel" => {
            let list = |k: &str| -> Vec<f64> {
                p.get(k)
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_f64).collect())
                    .unwrap_or_default()
            };
            w.col_fill = list("ColumnFill");
            w.row_fill = list("RowFill");
        }
        "ScrollBox" => {
            w.horiz = tail(p.get("Orientation")) == "Orient_Horizontal";
            w.bar_style = Some(Box::new(crate::menu::scroll::BarStyle::of(p)));
        }
        "BackgroundBlur" => {
            // The class defaults of UBackgroundBlur (the constructor 1425913f0): BlurStrength 0, bApplyAlphaToBlur true.
            w.blur = num(p, "BlurStrength").unwrap_or(0.0);
            w.blur_apply_alpha = p.get("bApplyAlphaToBlur").and_then(Value::as_bool).unwrap_or(true);
        }
        _ => {}
    }
    w.row = num(s, "Row").map_or(0, |v| v as i32);
    w.col = num(s, "Column").map_or(0, |v| v as i32);
    w.row_span = num(s, "RowSpan").map_or(1, |v| (v as i32).max(1));
    w.col_span = num(s, "ColumnSpan").map_or(1, |v| (v as i32).max(1));
    w.layer = num(s, "Layer").map_or(0, |v| v as i32);
    if let Some(ld) = s.get("LayoutData") {
        let a = ld.get("Anchors").unwrap_or(&NULL);
        let mn = xy(a.get("Minimum")).unwrap_or([None; 2]);
        let mx = xy(a.get("Maximum")).unwrap_or([None; 2]);
        w.anchors = [mn[0].unwrap_or(0.0), mn[1].unwrap_or(0.0), mx[0].unwrap_or(0.0), mx[1].unwrap_or(0.0)];
        if let Some(o) = ld.get("Offsets") {
            w.offsets = margin_of(Some(o));
        }
        let al = xy(ld.get("Alignment")).unwrap_or([None; 2]);
        w.align = [al[0].unwrap_or(0.0), al[1].unwrap_or(0.0)];
    }
    w.auto_size = s.get("bAutoSize").and_then(Value::as_bool).unwrap_or(false);
    // A ScrollBox clips: UScrollBox's constructor (1425966f0) sets ClipToBounds. Three of the menu bundle store Inherit
    // (UI_SocialPanel's left and search lists, UI_ContextMenu); those keep the clip of the inner scroll panel, which
    // SScrollBox::Construct builds with its own ClipToBounds argument.
    // Unverified: that part is from the 4.26 source, not read in the exe.
    w.clip = w.clip || tail(p.get("Clipping")).starts_with("ClipToBounds") || t == "ScrollBox";
    w
}

/// The entry of an engine Slate style texture by its bundle name (a bundle built without it: the name).
fn slate_entry(b: &Bundle, name: &str) -> String {
    b.textures.get(name).map_or_else(|| name.to_string(), |t| t.entry().to_string())
}

fn ty_of(t: &str) -> Ty {
    match t {
        "VerticalBox" => Ty::VerticalBox,
        // CommonUI's DynamicEntryBox (UI_GlyphEntries' entryBox), a horizontal row of the entries made at run time; the
        // controller gives each entry its half of EntrySpacing as slot padding.
        // Unverified: from the 4.26 source (SDynamicEntryBox), the exe's is not decoded.
        "HorizontalBox" | "DynamicEntryBox" => Ty::HorizontalBox,
        "SizeBox" => Ty::SizeBox,
        "ScaleBox" => Ty::ScaleBox,
        "CanvasPanel" => Ty::CanvasPanel,
        "WidgetSwitcher" => Ty::WidgetSwitcher,
        "Border" => Ty::Border,
        "Image" => Ty::Image,
        "TextBlock" => Ty::TextBlock,
        "Spacer" => Ty::Spacer,
        "ScrollBox" => Ty::ScrollBox,
        "GridPanel" => Ty::GridPanel,
        "UniformGridPanel" => Ty::UniformGridPanel,
        "WrapBox" => Ty::WrapBox,
        "Throbber" => Ty::Throbber,
        "CircularThrobber" => Ty::CircularThrobber,
        "EditableTextBox" => Ty::EditableTextBox,
        "BackgroundBlur" => Ty::BackgroundBlur,
        "RichTextBlock" => Ty::RichTextBlock,
        // Overlay, InvalidationBox, RetainerBox, NamedSlot, Button and the like: content panels.
        _ => Ty::Panel,
    }
}

/// An instance's properties over its class's defaults (instanceProps): structs merge member by member at any
/// depth, arrays and scalars replace; an FText is one value and replaces.
fn instance_props(base: &Value, over: &Value) -> Value {
    let mut out = match base {
        Value::Object(m) => m.clone(),
        _ => serde_json::Map::new(),
    };
    merge_props(&mut out, over);
    Value::Object(out)
}

/// `over`'s members written over `out`'s (instance_props' merge).
fn merge_props(out: &mut serde_json::Map<String, Value>, over: &Value) {
    let is_text = |o: &serde_json::Map<String, Value>| {
        ["CultureInvariantString", "SourceString", "LocalizedString", "TableId"].iter().any(|k| o.contains_key(*k))
    };
    let Value::Object(o) = over else { return };
    for (k, v) in o {
        match (v, out.get_mut(k)) {
            (Value::Object(vo), Some(Value::Object(bo))) if !is_text(vo) && !is_text(bo) => merge_props(bo, v),
            _ => {
                out.insert(k.clone(), v.clone());
            }
        }
    }
}

/// The recipe of a user widget instance (instantiate): a wrapper carrying the instance's slot and widget properties
/// around the class tree. WBP_Quadrilateral becomes a single quad: SetParameters gives its Image the
/// MaterialOverride brush at SizeOverride, ColorAndOpacity = Color (IsColorOverride off), and the dynamic material's
/// Parameters the corners and EdgeFalloff = Sharpness.
#[allow(clippy::too_many_arguments)]
fn user_item(
    r: &mut Recipes,
    b: &Bundle,
    cls: &str,
    name: &str,
    p: &Value,
    s: &Value,
    slot: &str,
    ns: Option<&indexmap::IndexMap<String, WData>>,
) -> Item {
    let cd = b.widgets.get(cls);
    let props = instance_props(cd.map_or(&NULL, |c| &c.defaults), p);
    let quad = cls == "WBP_Quadrilateral";
    let mut w = make_widget(b, if quad { "Quad" } else { "User" }, name, &props, s, slot);
    w.kind = cls.to_string();
    let node = Node {
        ty: if quad { Ty::Quad } else { Ty::User },
        user: Some(Arc::from(cls)),
        cursor: has_cursor(&props),
        ..Node::default()
    };
    if quad {
        let mut q = Vec::new();
        for k in ["TopLeft", "TopRight", "BottomLeft", "BottomRight"] {
            let c = props.get(k).unwrap_or(&NULL);
            q.push((format!("{k}_X"), num(c, "X").unwrap_or(0.0) as f32));
            q.push((format!("{k}_Y"), num(c, "Y").unwrap_or(0.0) as f32));
        }
        q.push(("EdgeFalloff".to_string(), num(&props, "Sharpness").unwrap_or(0.0) as f32));
        w.tex = props.get("MaterialOverride").and_then(Value::as_str).map(|n| resolve(b, n));
        let sz = xy(props.get("SizeOverride")).unwrap_or([None; 2]);
        w.brush_w = sz[0].unwrap_or(256.0);
        w.brush_h = sz[1].unwrap_or(256.0);
        w.color = rgba_of(props.get("Color"), [1.0; 4]);
        // the instance's own ColorAndOpacity (the user widget's tint) multiplies its Image's colour
        let tint = p.get("ColorAndOpacity").filter(|c| c.get("R").is_some());
        let outline = props.get("IsOutline").and_then(Value::as_bool).unwrap_or(false);
        let color_override = props.get("IsColorOverride").and_then(Value::as_bool).unwrap_or(false);
        // The quad's material instance with SetParameters' scalars (made at the first paint).
        let mut mid = w.tex.as_deref().map(Mid::of);
        if let Some(m) = &mut mid {
            for (k, v) in &q {
                m.set_scalar(k, *v);
            }
        }
        w.quad = Some(q);
        if outline {
            w.mid = mid;
            make_outline(&mut w, &props);
        } else {
            if color_override {
                // SetParameters @694: an IsColorOverride Image stays white; its UpdateColor(Color) @779 -> @422-524
                // writes Color to the material's ColorA and ColorB.
                let c = w.color.map(|x| x as f32);
                w.color = [1.0; 4];
                if let Some(m) = &mut mid {
                    m.set_vector("ColorA", c);
                    m.set_vector("ColorB", c);
                }
            }
            w.mid = mid;
            if let Some(k) = tint {
                let k = rgba_of(Some(k), [1.0; 4]);
                w.color = std::array::from_fn(|i| w.color[i] * k[i]);
            }
        }
        return Item::User {
            w,
            n: Node { props: Some(Arc::new(props)), ..node },
            class: None,
            pad: None,
            named: Vec::new(),
        };
    }
    let class = cd.and_then(|_| class_item(r, b, cls));
    // UUserWidget Padding: the instance's fields over the class default's.
    let pad = props.get("Padding").map(|p| margin_of(Some(p)));
    let named = ns.into_iter().flatten().map(|(k, n)| (k.clone(), item_of(r, b, n))).collect();
    Item::User { w, n: Node { props: Some(Arc::new(props)), ..node }, class, pad, named }
}

/// The recipe of class `cls`'s own tree, made on its first use.
fn class_item(r: &mut Recipes, b: &Bundle, cls: &str) -> Option<Arc<Item>> {
    if let Some(it) = r.classes.get(cls) {
        return Some(it.clone());
    }
    let it = Arc::new(item_of(r, b, &b.widgets.get(cls)?.root));
    r.classes.insert(cls.to_string(), it.clone());
    Some(it)
}

/// UUserWidget::SetPadding: the padding of the SObjectWidget's child slot around the widget tree's root (the root's
/// `pad` here).
// Unverified: from the 4.26 source, not read in the exe.
fn set_user_padding(t: &mut Tree, w: WidgetId, m: Margin) {
    if t.node(w).ty != Ty::User {
        return;
    }
    let Some(&root) = t.widget(w).children.first() else { return };
    if t.node(root).foreign {
        return;
    }
    t.widget_mut(root).pad = m;
}

/// WBP_Quadrilateral SetParameters with IsOutline: the Image goes into the RetainerBox, whose effect material is an
/// instance of MI_Quad_Masked with ColorA = ColorB = Color and the corners (@3356: EdgeFalloff = Sharpness); the
/// Image is scaled by 1 - OutlineThickness (@1832) and its own EdgeFalloff is Lerp(Sharpness x (1 + thickness),
/// Sharpness, scale.X) (@2392). The retained Image is painted without the instance's tint; the box takes it.
pub(crate) fn make_outline(w: &mut Widget, p: &Value) {
    let t = num(p, "OutlineThickness").unwrap_or(0.0);
    let sharp = num(p, "Sharpness").unwrap_or(0.0);
    let color = rgba_of(p.get("Color"), [1.0; 4]);
    w.color = if p.get("IsColorOverride").and_then(Value::as_bool).unwrap_or(false) { [1.0; 4] } else { color };
    let tint = p.get("ColorAndOpacity").filter(|c| c.get("R").is_some());
    let mut q = w.quad.clone().unwrap_or_default();
    w.retained = Some(Retained { quad: q.clone(), color, scale: [1.0 - t, 1.0 - t], tint: rgba_of(tint, [1.0; 4]) });
    let a = sharp * (1.0 + t);
    let edge = (a + (1.0 - t) * (sharp - a)) as f32;
    if let Some(e) = q.iter_mut().find(|e| e.0 == "EdgeFalloff") {
        e.1 = edge;
    }
    w.quad = Some(q);
    if let Some(m) = &mut w.mid {
        m.set_scalar("EdgeFalloff", edge);
    }
}

/// The Cursor property set.
fn has_cursor(p: &Value) -> bool {
    match p.get("Cursor") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(_) => true,
    }
}

/// The recipe of widget data `n` (a widget type or a user class, its properties, slot and children).
fn item_of(r: &mut Recipes, b: &Bundle, n: &WData) -> Item {
    if b.widgets.contains_key(&n.t) || n.t.starts_with("UI_") || n.t.starts_with("WBP_") {
        let ns = (!n.ns.is_empty()).then_some(&n.ns);
        return user_item(r, b, &n.t, &n.n, &n.p, &n.s, &n.st, ns);
    }
    let ty = ty_of(&n.t);
    let w = make_widget(b, &n.t, &n.n, &n.p, &n.s, &n.st);
    let mut kids = Vec::with_capacity(n.c.len() + 1);
    if ty == Ty::EditableTextBox {
        kids.push(editable_text_item(b, n));
    }
    kids.extend(n.c.iter().map(|c| item_of(r, b, c)));
    Item::Plain { w, n: Node { ty, cursor: has_cursor(&n.p), ..Node::default() }, kids }
}

/// SEditableTextBox (Unverified: from the 4.26 source, not read in this build's exe): an SBorder drawing the style's
/// background for the state (normal, or one image for hovered and focused; FCoreStyle "NormalEditableTextBox":
/// Common/TextBox and Common/TextBox_Hovered as Box brushes with margin 4/16) tinted BackgroundColor, around the
/// SEditableText (style Padding, default (4, 2); centred vertically) in the style's font and ForegroundColor, at
/// least MinimumDesiredWidth wide. The text is a child TextBlock named <name>.Text.
fn editable_text_item(b: &Bundle, n: &WData) -> Item {
    let st = n.p.get("WidgetStyle").unwrap_or(&NULL);
    let fg = st.get("ForegroundColor").and_then(|c| c.get("SpecifiedColor"));
    let props = serde_json::json!({
        "Font": st.get("Font").cloned().unwrap_or_else(|| serde_json::json!({})),
        "ColorAndOpacity": { "SpecifiedColor": fg.cloned().unwrap_or_else(|| serde_json::json!({"R": 0, "G": 0, "B": 0, "A": 1})) },
        "ShadowOffset": { "X": 0, "Y": 0 },
        "Visibility": "HitTestInvisible",
    });
    let slot = serde_json::json!({
        "Padding": st.get("Padding").cloned().unwrap_or_else(|| serde_json::json!({"Left": 4, "Top": 2, "Right": 4, "Bottom": 2})),
        "VerticalAlignment": "VAlign_Center",
    });
    let mut w = make_widget(b, "TextBlock", &format!("{}.Text", n.n), &props, &slot, "");
    w.min_text_w = num(&n.p, "MinimumDesiredWidth").unwrap_or(0.0);
    Item::Plain { w, n: Node { ty: Ty::TextBlock, ..Node::default() }, kids: Vec::new() }
}

/// The widget class `class` instantiated as a tree whose root is named `name`.
pub fn instantiate(b: &Bundle, class: &str, name: &str) -> Tree {
    let mut t = Tree { refs: Arc::new(Refs::of(b)), scale: 1.0, ..Tree::default() };
    t.create_user(b, None, class, name);
    t
}

// ---------- layout ----------

/// The area a tree is arranged in: its top left and size, Slate units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Geometry {
    pub pos: [f64; 2],
    pub size: [f64; 2],
}

/// Measure and arrange of `t` in `area` at the view's DPI `scale`. A ScaleBox whose
/// stretch depends on its area reads the area of the previous layout, as SScaleBox's prepass reads the last paint's.
pub fn layout(t: &mut Tree, fonts: &mut Fonts, area: Geometry, scale: f64) {
    t.scale = scale;
    if t.widgets.is_empty() {
        return;
    }
    let root = t.root();
    layout_widget(t, fonts, root, area, scale);
}

/// Measure only: each widget's desired size at the view's DPI `scale`, nothing arranged (a
/// ScaleBox keeps the area of its last layout).
pub fn measure_tree(t: &mut Tree, fonts: &mut Fonts, scale: f64) {
    t.scale = scale;
    if t.widgets.is_empty() {
        return;
    }
    let root = t.root();
    measure_widget(t, fonts, root, scale);
}

/// SScaleBox::ComputeContentScale (exe 1417cc720) for an area, from the child's desired size (cw, ch). The safe
/// zone scale (ScaleBySafeZone) is 1 here.
fn scale_box_scale(w: &Widget, width: f64, height: f64, cw: f64, ch: f64) -> f64 {
    if w.stretch == Stretch::UserSpecified {
        return w.user_scale;
    }
    if w.stretch == Stretch::ScaleBySafeZone || cw == 0.0 || ch == 0.0 {
        return 1.0;
    }
    let k = match w.stretch {
        Stretch::ScaleToFit => (width / cw).min(height / ch),
        Stretch::ScaleToFitX => width / cw,
        Stretch::ScaleToFitY => height / ch,
        Stretch::ScaleToFill => (width / cw).max(height / ch),
        _ => 1.0,
    };
    match w.stretch_dir {
        StretchDir::DownOnly => k.min(1.0),
        StretchDir::UpOnly => k.max(1.0),
        StretchDir::Both => k,
    }
}

fn pad_w(w: &Widget) -> f64 {
    w.pad.l + w.pad.r
}

fn pad_h(w: &Widget) -> f64 {
    w.pad.t + w.pad.b
}

/// A TextBlock's font as the text module takes it.
fn font_info(w: &Widget, last_width: f64) -> FontInfo {
    FontInfo {
        typeface: w.typeface.clone().unwrap_or_default(),
        size: w.font_size,
        letter_spacing: w.letter_spacing,
        outline: w.outline,
        shadow: w.shadow_offset,
        wrap: crate::text::wrap_width(w.auto_wrap, last_width, w.wrap_at),
        separate_fill_alpha: w.separate_fill,
        even_digits: w.even_digits,
    }
}

/// The prepass: `scale` is the layout scale (the view's DPI x every ScaleBox's content scale
/// above). SScaleBox::CustomPrepass (exe 1417dec80) and GetRelativeLayoutScale (1417e2930): for a stretch that
/// depends on the area the child is prepassed at the incoming scale, the content scale is computed from that and
/// the area of the last arrange (none before the first: unset, layout scale 1), and the child is prepassed again at
/// the incoming scale x the content scale.
fn measure(t: &mut Tree, fonts: &mut Fonts, w: WidgetId, scale: f64) {
    t.seen(w, scale);
    t.node_mut(w).text_scale = scale as f32 as f64;
    // SWidget::Prepass_Internal leaves a collapsed child out: nothing below it is measured.
    if t.widget(w).vis == Vis::Collapsed {
        let n = t.node_mut(w);
        n.dw = 0.0;
        n.dh = 0.0;
        return;
    }
    for c in t.widget(w).children.clone() {
        measure(t, fonts, c, scale);
    }
    let ty = t.node(w).ty;
    if ty == Ty::ScaleBox {
        let c = t.widget(w).children.iter().copied().find(|c| t.widget(*c).vis != Vis::Collapsed);
        let (cw, ch) = c.map_or((0.0, 0.0), |c| (t.node(c).dw, t.node(c).dh));
        let x = t.widget(w);
        let last = t.node(w).last;
        let k = match c {
            None => None,
            Some(_) if !x.stretch.needs_area() => Some(scale_box_scale(x, 0.0, 0.0, cw, ch)),
            Some(_) if last[0] >= 0.0 => Some(scale_box_scale(x, last[0], last[1], cw, ch)),
            Some(_) => None,
        };
        let n = t.node_mut(w);
        n.norm = [cw, ch];
        n.content_scale = k;
        if let (Some(c), Some(k)) = (c, k)
            && k != 1.0
            && k > 0.0
        {
            measure(t, fonts, c, scale * k);
        }
    }
    let kids = t.kids(w);
    let max_w = |t: &Tree| kids.iter().map(|c| t.node(*c).dw + pad_w(t.widget(*c))).fold(0.0, f64::max);
    let max_h = |t: &Tree| kids.iter().map(|c| t.node(*c).dh + pad_h(t.widget(*c))).fold(0.0, f64::max);
    let (dw, dh) = match ty {
        Ty::TextBlock => {
            // FSlateTextRun::Measure (exe 1417a6620): the shaped width plus the outline at both ends of the run and
            // |shadow X|. Each line is LineHeightPercentage x (GetMaxHeight above + below the baseline) tall
            // (FTextLayout::CreateLineViewBlocks).
            let x = t.widget(w);
            let f = font_info(x, t.node(w).w);
            let m = fonts.measure(&f, &x.shown_text(), t.node(w).text_scale);
            (
                (m.width + 2.0 * x.outline + x.shadow_offset[0].abs()).max(x.min_text_w),
                m.lines.len() as f64 * m.height * x.line_height,
            )
        }
        Ty::Image | Ty::Quad | Ty::Spacer => (t.widget(w).brush_w, t.widget(w).brush_h),
        Ty::CanvasPanel => {
            // SConstraintCanvas::ComputeDesiredSize: each child's auto or slot size, plus its offset where docked
            // to the left / top edge.
            let (mut dw, mut dh) = (0.0f64, 0.0f64);
            for &c in &kids {
                let x = t.widget(c);
                let [x0, y0, x1, y1] = x.anchors;
                let sw = if x.auto_size { t.node(c).dw } else { x.offsets.r };
                let sh = if x.auto_size { t.node(c).dh } else { x.offsets.b };
                let docked = |a: f64, b: f64| a == b && (a == 0.0 || a == 1.0);
                dw = dw.max(sw + if docked(x0, x1) { x.offsets.l.abs() } else { 0.0 });
                dh = dh.max(sh + if docked(y0, y1) { x.offsets.t.abs() } else { 0.0 });
            }
            (dw, dh)
        }
        Ty::ScaleBox => {
            // SScaleBox::ComputeDesiredSize (exe 1417cd750): a stretch that depends on the area reports the child's
            // normalised desired size (no slot padding; ScaleToFitX / ScaleToFitY scale the other axis by the
            // content scale), the others the compound widget's size x the content scale.
            let x = t.widget(w);
            let n = t.node(w);
            match kids.first() {
                None => (0.0, 0.0),
                Some(_) if x.stretch.needs_area() => {
                    let k = n.content_scale.filter(|k| *k != 0.0);
                    (
                        n.norm[0] * if x.stretch == Stretch::ScaleToFitY { k.unwrap_or(1.0) } else { 1.0 },
                        n.norm[1] * if x.stretch == Stretch::ScaleToFitX { k.unwrap_or(1.0) } else { 1.0 },
                    )
                }
                Some(&c) => {
                    let k = n.content_scale.unwrap_or(1.0);
                    ((t.node(c).dw + pad_w(t.widget(c))) * k, (t.node(c).dh + pad_h(t.widget(c))) * k)
                }
            }
        }
        Ty::CircularThrobber => (2.0 * t.widget(w).radius, 2.0 * t.widget(w).radius),
        Ty::Throbber => {
            // a row of pieces, each an SBorder (padding 2) around the piece image
            let x = t.widget(w);
            (f64::from(x.pieces) * (x.brush_w + 4.0), x.brush_h + 4.0)
        }
        Ty::GridPanel => {
            // SGridPanel::ComputeDesiredSize: the sum of the desired cell sizes (grid_cells)
            grid_cells(t, w, &kids);
            let n = t.node(w);
            (n.grid_cols.iter().sum(), n.grid_rows.iter().sum())
        }
        Ty::ScrollBox if t.widget(w).horiz => {
            // SScrollPanel::ComputeDesiredSize (horizontal): the children's widths with their padding summed, the
            // tallest; SScrollBox's vertical box adds the bar's row under it.
            // Unverified, as the arrange below.
            (kids.iter().map(|c| t.node(*c).dw + pad_w(t.widget(*c))).sum(), max_h(t) + t.widget(w).bar_h)
        }
        Ty::UniformGridPanel => {
            // SUniformGridPanel::ComputeDesiredSize: columns x rows cells of the largest child's desired size plus the
            // panel's slot padding (at least the minimum slot size).
            // Unverified: from the engine's source, the exe's SUniformGridPanel is not decoded.
            let x = t.widget(w);
            let p = x.slot_pad;
            let cw = kids.iter().map(|c| (t.node(*c).dw + p.l + p.r).max(x.min_slot[0])).fold(0.0, f64::max);
            let ch = kids.iter().map(|c| (t.node(*c).dh + p.t + p.b).max(x.min_slot[1])).fold(0.0, f64::max);
            let cols = kids.iter().map(|c| t.widget(*c).col + 1).max().unwrap_or(0).max(0);
            let rows = kids.iter().map(|c| t.widget(*c).row + 1).max().unwrap_or(0).max(0);
            (cw * f64::from(cols), ch * f64::from(rows))
        }
        Ty::WrapBox => {
            // SWrapBox::ComputeDesiredSize: the lines' extent at the wrap width (the last allotted width when that is
            // the wrap width)
            let x = t.widget(w);
            let wrap = if x.wrap_size >= 0.0 { x.wrap_size } else { t.node(w).w };
            wrap_lines(t, w, &kids, wrap, None)
        }
        Ty::VerticalBox | Ty::ScrollBox => (max_w(t), kids.iter().map(|c| t.node(*c).dh + pad_h(t.widget(*c))).sum()),
        Ty::HorizontalBox => (kids.iter().map(|c| t.node(*c).dw + pad_w(t.widget(*c))).sum(), max_h(t)),
        _ => {
            // Overlay, SizeBox, user widgets, invalidation / retainer boxes.
            let (mut dw, mut dh) = (max_w(t), max_h(t));
            let x = t.widget(w);
            // SBox::ComputeDesiredSize: a box whose child is Collapsed has no desired size at all; a box without a
            // child keeps its overrides and minimums.
            // Unverified: from the 4.26 engine source (SBox.cpp), the exe's SBox is not decoded.
            if ty == Ty::SizeBox && !(!x.children.is_empty() && kids.is_empty()) {
                // SBox::ComputeDesiredSizeForBox: an override wins; otherwise the child's size clamped up to the
                // minimum, then down to the maximum.
                if !x.w_override.is_nan() {
                    dw = x.w_override;
                } else {
                    if !x.min_w.is_nan() {
                        dw = dw.max(x.min_w);
                    }
                    if !x.max_w.is_nan() {
                        dw = dw.min(x.max_w);
                    }
                }
                if !x.h_override.is_nan() {
                    dh = x.h_override;
                } else {
                    if !x.min_h.is_nan() {
                        dh = dh.max(x.min_h);
                    }
                    if !x.max_h.is_nan() {
                        dh = dh.min(x.max_h);
                    }
                }
                // (the port's own) a box kept at an aspect grows on its short side to it: fillPage's box, whose
                // minimum and maximum have that aspect, so the grown side stays within its maximum. Not in SBox.
                if x.keep_aspect > 0.0 && x.w_override.is_nan() && x.h_override.is_nan() && dw > 0.0 && dh > 0.0 {
                    let wide = dh * x.keep_aspect;
                    if dw < wide * (1.0 - 1e-6) {
                        dw = wide;
                    } else if dw > wide * (1.0 + 1e-6) {
                        dh = dw / x.keep_aspect;
                    }
                }
            }
            (dw, dh)
        }
    };
    let n = t.node_mut(w);
    n.dw = dw;
    n.dh = dh;
}

/// SGridPanel::ComputeDesiredCellSizes: per column and row the largest (desired size + padding) / span of the
/// children that span it, with one more entry of 0 at the end (the partial sums' total). The counts come from every
/// slot, a collapsed one too (SGridPanel::NotifySlotChanged).
fn grid_cells(t: &mut Tree, w: WidgetId, kids: &[WidgetId]) {
    let (mut nc, mut nr) = (0usize, 0usize);
    for &c in &t.widget(w).children {
        let x = t.widget(c);
        nc = nc.max(usize::try_from(x.col + x.col_span).unwrap_or(0));
        nr = nr.max(usize::try_from(x.row + x.row_span).unwrap_or(0));
    }
    // (the node's vectors of the last measure, written again)
    let n = t.node_mut(w);
    let (mut cols, mut rows) = (std::mem::take(&mut n.grid_cols), std::mem::take(&mut n.grid_rows));
    cols.clear();
    cols.resize(nc + 1, 0.0);
    rows.clear();
    rows.resize(nr + 1, 0.0);
    for &c in kids {
        let x = t.widget(c);
        let cw = (t.node(c).dw + pad_w(x)) / f64::from(x.col_span);
        let ch = (t.node(c).dh + pad_h(x)) / f64::from(x.row_span);
        for i in x.col..x.col + x.col_span {
            if let Some(v) = usize::try_from(i).ok().and_then(|i| cols.get_mut(i)) {
                *v = v.max(cw);
            }
        }
        for i in x.row..x.row + x.row_span {
            if let Some(v) = usize::try_from(i).ok().and_then(|i| rows.get_mut(i)) {
                *v = v.max(ch);
            }
        }
    }
    let n = t.node_mut(w);
    n.grid_cols = cols;
    n.grid_rows = rows;
}

/// SGridPanel CalculateStretchedCellSizes and ComputePartialSums: a cell of fill coefficient 0 keeps its desired
/// size, the others share what is left of the allotted size by coefficient; then each entry becomes the offset of its
/// cell (the last one the total).
fn stretch_cells(cells: &[f64], fill: &[f64], allotted: f64, out: &mut Vec<f64>) {
    let k = |i: usize| fill.get(i).copied().unwrap_or(0.0);
    let (mut total, mut fixed) = (0.0, 0.0);
    for (i, c) in cells.iter().enumerate() {
        if k(i) == 0.0 {
            fixed += c;
        }
        total += k(i);
    }
    let free = (allotted - fixed).max(0.0);
    let mut sum = 0.0;
    out.clear();
    for (i, c) in cells.iter().enumerate() {
        let size = if k(i) > 0.0 { if total > 0.0 { k(i) / total * free } else { 0.0 } } else { *c };
        out.push(sum);
        sum += size;
    }
}

/// A WrapBox slot placed: the child, its slot's x and y, the slot's width and its line's height.
type WrapSlot = (WidgetId, f64, f64, f64, f64);

/// SWrapBox's FChildArranger: the children in lines, each in a slot of its desired size plus its slot padding; a
/// child whose right bound passes the wrap width starts the next line (never the first of a line); the slots are
/// InnerSlotPadding apart and a line's slots take its tallest one's height. Returns the extent; each slot into
/// `out` when given (cleared first).
// Unverified: from the 4.26 engine source, the exe's SWrapBox is not decoded; bFillEmptySpace and bForceNewLine of a
// slot are not modelled (the bundle's one wrap box has neither).
fn wrap_lines(t: &Tree, w: WidgetId, kids: &[WidgetId], wrap: f64, mut out: Option<&mut Vec<WrapSlot>>) -> (f64, f64) {
    let pad = t.widget(w).wrap_pad;
    let (mut x, mut y, mut line_h, mut right) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    // (the line's slots are the last `line` of `out`: their y and height are set when it ends)
    let mut line = 0usize;
    let end_line = |out: &mut Option<&mut Vec<WrapSlot>>, line: usize, y: f64, line_h: f64| {
        if let Some(o) = out {
            let n = o.len();
            for s in &mut o[n - line..] {
                (s.2, s.4) = (y, line_h);
            }
        }
    };
    if let Some(o) = &mut out {
        o.clear();
    }
    for &c in kids {
        let cw = t.node(c).dw + pad_w(t.widget(c));
        let ch = t.node(c).dh + pad_h(t.widget(c));
        if line > 0 && x + cw > wrap {
            end_line(&mut out, line, y, line_h);
            line = 0;
            x = 0.0;
            y += line_h + pad[1];
            line_h = 0.0;
        }
        if let Some(o) = &mut out {
            o.push((c, x, 0.0, cw, 0.0));
        }
        line += 1;
        right = right.max(x + cw);
        x += cw + pad[0];
        line_h = line_h.max(ch);
    }
    end_line(&mut out, line, y, line_h);
    (right, y + line_h)
}

/// AlignChild (LayoutUtils.h, as inlined in SOverlay::OnArrangeChildren, exe 1416c7870): the size is the desired
/// size clamped to the area less the padding; a centred child sits at (area - size) / 2 + leading - trailing
/// padding. Returns (offset, size).
fn align(mode: u8, allotted: f64, m0: f64, m1: f64, desired: f64) -> (f64, f64) {
    let avail = allotted - m0 - m1;
    if mode == 3 {
        return (m0, avail);
    }
    let size = desired.min(avail);
    let o = match mode {
        1 => (allotted - size) * 0.5 + m0 - m1,
        2 => allotted - m1 - size,
        _ => m0,
    };
    (o, size)
}

fn place(t: &mut Tree, w: WidgetId, x: f64, y: f64, width: f64, height: f64) {
    // (an auto-wrapping text's and a wrap box's measure read the width of the last arrange)
    let k = &t.widgets[w.index()];
    let reads_width = match t.node(w).ty {
        Ty::TextBlock => k.auto_wrap,
        Ty::WrapBox => k.wrap_size < 0.0,
        _ => false,
    };
    let n = t.node_mut(w);
    let moved = reads_width && n.w.to_bits() != width.to_bits();
    n.arranged = None;
    n.x = x;
    n.y = y;
    n.w = width;
    n.h = height;
    if moved {
        t.invalidate(w);
    }
}

/// A ScrollBox's largest offset set and its offset kept inside it (an output of the arrange, not a write that lays
/// it out again: its layout key takes the clamped offset); the offset.
fn clamp_scroll(t: &mut Tree, w: WidgetId, max: f64) -> f64 {
    let i = w.index();
    let x = &mut t.widgets[i];
    x.scroll_max = max;
    x.scroll = x.scroll.max(0.0).min(max);
    if let Some(k) = &mut t.nodes[i].key {
        k.scroll = x.scroll.to_bits();
    }
    x.scroll
}

fn arrange(t: &mut Tree, w: WidgetId, x: f64, y: f64, width: f64, height: f64) {
    place(t, w, x, y, width, height);
    let kids = t.kids(w);
    match t.node(w).ty {
        Ty::VerticalBox | Ty::HorizontalBox => {
            let vert = t.node(w).ty == Ty::VerticalBox;
            let main_pad = |c: &Widget| if vert { pad_h(c) } else { pad_w(c) };
            let main = |t: &Tree, c: WidgetId| {
                let x = t.widget(c);
                if vert { t.node(c).dh + pad_h(x) } else { t.node(c).dw + pad_w(x) }
            };
            let fixed: f64 =
                kids.iter().map(|&c| if t.widget(c).fill != 0.0 { main_pad(t.widget(c)) } else { main(t, c) }).sum();
            let weights: f64 = kids.iter().map(|c| t.widget(*c).fill).sum();
            let free = ((if vert { height } else { width }) - fixed).max(0.0);
            let mut at = 0.0;
            for c in kids {
                let x = t.widget(c);
                let span = if x.fill != 0.0 { free * x.fill / weights + main_pad(x) } else { main(t, c) };
                let (dw, dh) = (t.node(c).dw, t.node(c).dh);
                if vert {
                    let (ox, sw) = align(h_axis(x.h_align), width, x.pad.l, x.pad.r, dw);
                    let (oy, sh) = align(v_axis(x.v_align), span, x.pad.t, x.pad.b, dh);
                    arrange(t, c, ox, at + oy, sw, sh);
                } else {
                    let (ox, sw) = align(h_axis(x.h_align), span, x.pad.l, x.pad.r, dw);
                    let (oy, sh) = align(v_axis(x.v_align), height, x.pad.t, x.pad.b, dh);
                    arrange(t, c, at + ox, oy, sw, sh);
                }
                at += span;
            }
        }
        Ty::ScrollBox if t.widget(w).horiz => {
            // SScrollPanel::OnArrangeChildren (horizontal, ArrangeChildHorizontalAndReturnOffset): each child gets its
            // desired width, laid along x from -the scroll offset, and is aligned in the panel's height by its slot
            // (AlignChild<Orient_Vertical>: VAlign and the top / bottom padding). The panel is the box less the bar's
            // row under it (SScrollBox::ConstructHorizontalLayout: an SVerticalBox of the panel, fill, and the bar,
            // auto height).
            // Unverified: from the 4.26 source, the exe's SScrollBox / SScrollPanel are not decoded; the panel's 1
            // unit end padding and overscroll are not modelled.
            let total: f64 = kids.iter().map(|&c| t.node(c).dw + pad_w(t.widget(c))).sum();
            let panel_h = (height - t.widget(w).bar_h).max(0.0);
            let max = (total - width).max(0.0);
            let mut at = -clamp_scroll(t, w, max);
            for c in kids {
                let k = t.widget(c);
                let (oy, sh) = align(v_axis(k.v_align), panel_h, k.pad.t, k.pad.b, t.node(c).dh);
                let (dw, pl, pr) = (t.node(c).dw, k.pad.l, k.pad.r);
                arrange(t, c, at + pl, oy, dw, sh);
                at += dw + pl + pr;
            }
        }
        Ty::ScrollBox => {
            // SScrollPanel::OnArrangeChildren (vertical): each child gets its desired height, stacked from -the scroll
            // offset; the offset is kept inside the content.
            // Unverified: from the engine's public behaviour, the exe's SScrollBox is not decoded (the scroll bar,
            // overscroll and animated wheel scrolling are menu/scroll.rs's or not modelled).
            let total: f64 = kids.iter().map(|&c| t.node(c).dh + pad_h(t.widget(c))).sum();
            let max = (total - height).max(0.0);
            let mut at = -clamp_scroll(t, w, max);
            for c in kids {
                let k = t.widget(c);
                let (ox, sw) = align(h_axis(k.h_align), width, k.pad.l, k.pad.r, t.node(c).dw);
                let (dh, pt, pb) = (t.node(c).dh, k.pad.t, k.pad.b);
                arrange(t, c, ox, at + pt, sw, dh);
                at += dh + pt + pb;
            }
        }
        Ty::GridPanel => {
            // SGridPanel::OnArrangeChildren: the desired cell sizes of the measure stretched by the fill coefficients,
            // made partial sums; a child gets the cells it spans and is aligned in them with its padding.
            // Unverified: from the engine's source, the exe's SGridPanel is not decoded; Nudge is not modelled. The
            // children stand in Layer order (Tree::add).
            // (the node's vectors of the last arrange, written again)
            let n = t.node_mut(w);
            let (mut cols, mut rows) = (std::mem::take(&mut n.grid_x), std::mem::take(&mut n.grid_y));
            {
                let x = t.widget(w);
                stretch_cells(&t.node(w).grid_cols, &x.col_fill, width, &mut cols);
                stretch_cells(&t.node(w).grid_rows, &x.row_fill, height, &mut rows);
            }
            for c in kids {
                let k = t.widget(c);
                let at = |v: &[f64], i: i32| usize::try_from(i).ok().and_then(|i| v.get(i)).copied().unwrap_or(0.0);
                let (x0, y0) = (at(&cols, k.col), at(&rows, k.row));
                let (cw, ch) = (at(&cols, k.col + k.col_span) - x0, at(&rows, k.row + k.row_span) - y0);
                let (ox, sw) = align(h_axis(k.h_align), cw, k.pad.l, k.pad.r, t.node(c).dw);
                let (oy, sh) = align(v_axis(k.v_align), ch, k.pad.t, k.pad.b, t.node(c).dh);
                arrange(t, c, x0 + ox, y0 + oy, sw, sh);
            }
            let n = t.node_mut(w);
            n.grid_x = cols;
            n.grid_y = rows;
        }
        Ty::WrapBox => {
            let wrap = if t.widget(w).wrap_size >= 0.0 { t.widget(w).wrap_size } else { width };
            // (the tree's slot list, taken while the children arrange: a wrap box inside makes its own)
            let mut slots = std::mem::take(&mut t.wrap_slots);
            wrap_lines(t, w, &kids, wrap, Some(&mut slots));
            for &(c, sx, sy, sw, sh) in &slots {
                let k = t.widget(c);
                let (ox, cw) = align(h_axis(k.h_align), sw, k.pad.l, k.pad.r, t.node(c).dw);
                let (oy, ch) = align(v_axis(k.v_align), sh, k.pad.t, k.pad.b, t.node(c).dh);
                arrange(t, c, sx + ox, sy + oy, cw, ch);
            }
            t.wrap_slots = slots;
        }
        Ty::UniformGridPanel => {
            // SUniformGridPanel::OnArrangeChildren: the area in equal cells; a child is aligned in its cell with the
            // panel's slot padding.
            let cols = kids.iter().map(|c| t.widget(*c).col + 1).max().unwrap_or(1).max(1);
            let rows = kids.iter().map(|c| t.widget(*c).row + 1).max().unwrap_or(1).max(1);
            let (cw, ch) = (width / f64::from(cols), height / f64::from(rows));
            let p = t.widget(w).slot_pad;
            for c in kids {
                let k = t.widget(c);
                let (ox, sw) = align(h_axis(k.h_align), cw, p.l, p.r, t.node(c).dw);
                let (oy, sh) = align(v_axis(k.v_align), ch, p.t, p.b, t.node(c).dh);
                arrange(t, c, cw * f64::from(k.col) + ox, ch * f64::from(k.row) + oy, sw, sh);
            }
        }
        Ty::CanvasPanel => {
            // SConstraintCanvas::ArrangeChildren: a stretched axis spans its anchors minus the offsets; otherwise
            // the child sits at anchor + offset - alignment x size, sized by its desired size (auto) or the slot
            // size.
            for c in kids {
                let x = t.widget(c);
                let [x0, y0, x1, y1] = x.anchors;
                let o = x.offsets;
                let sw = if x.auto_size { t.node(c).dw } else { o.r };
                let sh = if x.auto_size { t.node(c).dh } else { o.b };
                let (cx, cw) = if x0 != x1 {
                    let cx = x0 * width + o.l;
                    (cx, x1 * width - cx - o.r)
                } else {
                    (x0 * width + o.l - sw * x.align[0], sw)
                };
                let (cy, ch) = if y0 != y1 {
                    let cy = y0 * height + o.t;
                    (cy, y1 * height - cy - o.b)
                } else {
                    (y0 * height + o.t - sh * x.align[1], sh)
                };
                arrange(t, c, cx, cy, cw, ch);
            }
        }
        Ty::ScaleBox => {
            // SScaleBox::OnArrangeChildren (exe 1417e8750): the scale is the prepass's content scale, or computed
            // from this area when that is unset. Stretch Fill gives the child the area; otherwise the child is
            // aligned with its desired size x the scale and keeps its desired size, a Fill alignment taking the
            // area / scale (the padding only moves it), under a layout scale. The area is remembered (OnPaint
            // 1417f63f0) for the next prepass.
            let n = t.node_mut(w);
            let moved = n.last.map(f64::to_bits) != [width.to_bits(), height.to_bits()];
            n.last = [width, height];
            if moved {
                t.invalidate(w);
            }
            let Some(&c) = kids.first() else { return };
            let (cdw, cdh) = (t.node(c).dw, t.node(c).dh);
            let k = t.node(w).content_scale.unwrap_or_else(|| scale_box_scale(t.widget(w), width, height, cdw, cdh));
            if t.widget(w).stretch == Stretch::Fill {
                arrange(t, c, 0.0, 0.0, width, height);
                t.node_mut(c).ls = k;
                return;
            }
            // AlignChild as inlined there: the scaled desired size is not clamped to the area.
            let off = |mode: u8, allotted: f64, m0: f64, m1: f64, size: f64| match mode {
                1 => (allotted - size) * 0.5 + m0 - m1,
                2 => allotted - size - m1,
                _ => m0,
            };
            let x = t.widget(c);
            let (ha, va) = (h_axis(x.h_align), v_axis(x.v_align));
            let ox = off(ha, width, x.pad.l, x.pad.r, cdw * k);
            let oy = off(va, height, x.pad.t, x.pad.b, cdh * k);
            arrange(t, c, ox, oy, if ha == 3 { width / k } else { cdw }, if va == 3 { height / k } else { cdh });
            t.node_mut(c).ls = k;
        }
        _ => {
            for c in kids {
                let x = t.widget(c);
                let (ox, sw) = align(h_axis(x.h_align), width, x.pad.l, x.pad.r, t.node(c).dw);
                let (oy, sh) = align(v_axis(x.v_align), height, x.pad.t, x.pad.b, t.node(c).dh);
                arrange(t, c, ox, oy, sw, sh);
            }
        }
    }
}

// ---------- widget animations ----------

/// The bundles' tick resolution.
const TICKS: f64 = 60000.0;

/// FMovieSceneFloatChannel evaluation at a tick: constant extrapolation, linear / constant / cubic Hermite keys and
/// weighted tangents.
pub fn eval_channel(ch: &Channel, t: f64) -> Option<f64> {
    let n = ch.t.len();
    if n == 0 {
        return ch.def;
    }
    if t <= ch.t[0] {
        return Some(ch.v[0][0]);
    }
    if t >= ch.t[n - 1] {
        return Some(ch.v[n - 1][0]);
    }
    let mut i = 0;
    while t >= ch.t[i + 1] {
        i += 1;
    }
    let (k0, k1) = (&ch.v[i], &ch.v[i + 1]);
    let at = |k: &Vec<f64>, j: usize| k.get(j).copied().unwrap_or(0.0);
    let (v0, mode, leave) = (k0[0], at(k0, 1), at(k0, 3));
    let (leave_w, wm0) = (at(k0, 5), at(k0, 6));
    let (v1, arrive) = (k1[0], at(k1, 2));
    let (arrive_w, wm1) = (at(k1, 4), at(k1, 6));
    let dt = ch.t[i + 1] - ch.t[i];
    let u = (t - ch.t[i]) / dt;
    if mode == 1.0 {
        return Some(v0);
    }
    if mode == 0.0 {
        return Some(v0 + (v1 - v0) * u);
    }
    // Weighted tangents (UE 4.26 MovieSceneFloatChannel EvalForTwoKeys): key 1 weighted on its leave side (modes 2,
    // 3) or key 2 on its arrive side (1, 3). The tangent's angle and weight place each Bezier handle in (seconds,
    // value); the curve parameter is the root of the time Bezier at the normalised time.
    if wm0 >= 2.0 || wm1 == 1.0 || wm1 == 3.0 {
        let x = dt / TICKS;
        let handle = |slope: f64, w: f64, weighted: bool| {
            let a = (slope * TICKS).atan();
            let len = if weighted { w } else { (x * x + slope * TICKS * x * (slope * TICKS * x)).sqrt() / 3.0 };
            (a.cos() * len, a.sin() * len)
        };
        let (x1, y1) = handle(leave, leave_w, wm0 >= 2.0);
        let (x2, y2) = handle(arrive, arrive_w, wm1 == 1.0 || wm1 == 3.0);
        let s = bezier_time(x1 / x, (x - x2) / x, u);
        return Some(bezier(v0, v0 + y1, v1 - y2, v1, s));
    }
    let (u2, u3) = (u * u, u * u * u);
    Some(
        (2.0 * u3 - 3.0 * u2 + 1.0) * v0
            + (u3 - 2.0 * u2 + u) * leave * dt
            + (-2.0 * u3 + 3.0 * u2) * v1
            + (u3 - u2) * arrive * dt,
    )
}

fn bezier(p0: f64, p1: f64, p2: f64, p3: f64, s: f64) -> f64 {
    let r = 1.0 - s;
    r * r * r * p0 + 3.0 * r * r * s * p1 + 3.0 * r * s * s * p2 + s * s * s * p3
}

/// The parameter where the Bezier (0, x1, x2, 1) equals alpha: BezierToPower + SolveCubic (Cardano), taking the
/// single root, or the largest root inside [0, 1] of several (0 when none is).
fn bezier_time(x1: f64, x2: f64, alpha: f64) -> f64 {
    let a = x1;
    let b = x2 - x1;
    let c = 1.0 - x2;
    let d = b - a;
    let c3 = c - b - d;
    let c2 = 3.0 * d;
    let c1 = 3.0 * a;
    let c0 = -alpha;
    // The engine divides by the cubic coefficient; evenly spaced handles make it 0 and the time linear.
    if c3.abs() < 1e-12 {
        return alpha;
    }
    let (aa, bb, cc) = (c2 / c3, c1 / c3, c0 / c3);
    let p = (1.0 / 3.0) * ((-1.0 / 3.0) * aa * aa + bb);
    let q = 0.5 * ((2.0 / 27.0) * aa * aa * aa - (1.0 / 3.0) * aa * bb + cc);
    let disc = q * q + p * p * p;
    let zero = |v: f64| v.abs() < 1e-8;
    let roots: Vec<f64> = if zero(disc) {
        if zero(q) { vec![0.0] } else { vec![2.0 * (-q).cbrt(), -(-q).cbrt()] }
    } else if disc < 0.0 {
        let phi = (1.0 / 3.0) * (-q / (-p * p * p).sqrt()).acos();
        let k = 2.0 * (-p).sqrt();
        let pi = std::f64::consts::PI;
        vec![k * phi.cos(), -k * (phi + pi / 3.0).cos(), -k * (phi - pi / 3.0).cos()]
    } else {
        vec![(disc.sqrt() - q).cbrt() - (disc.sqrt() + q).cbrt()]
    };
    let roots: Vec<f64> = roots.into_iter().map(|r| r - aa / 3.0).collect();
    if roots.len() == 1 {
        return roots[0];
    }
    roots
        .into_iter()
        .filter(|r| (0.0..=1.0).contains(r))
        .fold(None, |m: Option<f64>, r| Some(m.map_or(r, |m| m.max(r))))
        .unwrap_or(0.0)
}

/// A section's channel at t; None where the section has none.
fn chan(s: &Section, k: &str, t: f64) -> Option<f64> {
    s.ch.get(k).and_then(|c| eval_channel(c, t))
}

/// `anim` of the tree's widget `owner` evaluated at `sec`: its tracks' values set on the
/// widgets; the event keys passed between `from_sec` and `sec` are the caller's (AnimData.events). Bindings resolve
/// in the owner's own tree only (UWidgetTree::FindWidget does not enter child user widgets); unresolved tracks do
/// nothing; unkeyed channels keep the widget's value.
///
/// Float and 2D transform tracks are entity sections in this build: the sequence updater imports the sections that
/// contain the frame number of the current time (FSequenceUpdater_Flat::Update, exe 1423fb870: one point, not the
/// swept range), so a section the time has left keeps the last value it gave. Its lower bound is inclusive, the
/// upper one inclusive unless excl. The other tracks (colour, byte, margin, material) run through the legacy
/// evaluator: a section the swept range [from_sec, sec] overlaps is evaluated at sec even when sec has passed its
/// end.
// Unverified: the legacy rule is not decoded.
pub fn apply_anim(t: &mut Tree, owner: WidgetId, anim: &AnimData, sec: f64, from_sec: f64) {
    let at = anim.range[0] + sec * anim.res;
    let at0 = anim.range[0] + from_sec.min(sec) * anim.res;
    // The frame number; the epsilon keeps a time meant to be a whole tick from flooring to the one before.
    let frame = (at + 1e-6).floor();
    for tr in &anim.tracks {
        let w = if tr.widget == "$root" { Some(owner) } else { t.lookup(owner, owner, &tr.widget) };
        let Some(w) = w else { continue };
        let entity = tr.kind == "MovieScene2DTransformTrack" || tr.kind == "MovieSceneFloatTrack";
        let prop = tr.prop.as_deref().unwrap_or("");
        for s in &tr.sections {
            if entity {
                if s.range[0].is_some_and(|r| frame < r) {
                    continue;
                }
                if s.range[1].is_some_and(|r| if s.excl { frame >= r } else { frame > r }) {
                    continue;
                }
            } else if s.range[0].is_some_and(|r| at < r) || s.range[1].is_some_and(|r| at0 > r) {
                continue;
            }
            let x = t.widget_mut(w);
            match tr.kind.as_str() {
                "MovieScene2DTransformTrack" => {
                    // The section's TransformMask (EMovieScene2DTransformChannel: TranslationX 1, TranslationY 2,
                    // Rotation 4, ScaleX 8, ScaleY 16, ShearX 32, ShearY 64; none = all) names the channels it
                    // animates: a channel outside it is not evaluated.
                    let mask = s.mask.unwrap_or(127);
                    let r = &mut x.transform;
                    let set = |bit: u32, k: &str, v: &mut f64| {
                        if mask & bit != 0
                            && let Some(c) = chan(s, k, at)
                        {
                            *v = c;
                        }
                    };
                    set(1, "Translation", &mut r.translation[0]);
                    set(2, "Translation[1]", &mut r.translation[1]);
                    set(4, "Rotation", &mut r.angle);
                    set(8, "Scale", &mut r.scale[0]);
                    set(16, "Scale[1]", &mut r.scale[1]);
                    set(32, "Shear", &mut r.shear[0]);
                    set(64, "Shear[1]", &mut r.shear[1]);
                }
                "MovieSceneByteTrack" if prop == "Visibility" => {
                    if let Some(v) = chan(s, "ByteCurve", at) {
                        x.vis = Vis::from_byte(v as u8);
                    }
                }
                "MovieSceneFloatTrack" if prop == "BlurStrength" => {
                    // the strength the blur's pass reads
                    if let Some(v) = chan(s, "FloatCurve", at) {
                        x.blur_set = Some(v);
                    }
                }
                "MovieSceneFloatTrack" if prop == "WidthOverride" || prop == "HeightOverride" => {
                    // USizeBox::SetWidthOverride / SetHeightOverride (what the property track calls): the value, and
                    // the override on.
                    if let Some(v) = chan(s, "FloatCurve", at) {
                        if prop == "WidthOverride" {
                            x.w_override = v;
                        } else {
                            x.h_override = v;
                        }
                    }
                }
                "MovieSceneVectorTrack" if prop == "Size" => {
                    // A Spacer's Size (a vector track, channels X "Curves" and Y "Curves[1]"; a channel without keys
                    // leaves its axis as it is).
                    // Unverified: this build's vector sections run through the legacy evaluator like the colour
                    // ones; the sections of the bundle cover their animation's whole range, where the two rules
                    // agree.
                    if let Some(v) = chan(s, "Curves", at) {
                        x.brush_w = v;
                    }
                    if let Some(v) = chan(s, "Curves[1]", at) {
                        x.brush_h = v;
                    }
                }
                "MovieSceneFloatTrack" if prop == "RenderOpacity" => {
                    if let Some(v) = chan(s, "FloatCurve", at) {
                        x.opacity = v;
                    }
                }
                "MovieSceneColorTrack" => {
                    let c = if prop == "ShadowColorAndOpacity" { &mut x.shadow_color } else { &mut x.color };
                    for (i, k) in ["RedCurve", "GreenCurve", "BlueCurve", "AlphaCurve"].into_iter().enumerate() {
                        if let Some(v) = chan(s, k, at) {
                            c[i] = v;
                        }
                    }
                }
                "MovieSceneMarginTrack" if prop == "LayoutData.Offsets" => {
                    // The widget's canvas panel slot offsets (a slot binding).
                    let o = &mut x.offsets;
                    for (k, v) in [
                        ("LeftCurve", &mut o.l),
                        ("TopCurve", &mut o.t),
                        ("RightCurve", &mut o.r),
                        ("BottomCurve", &mut o.b),
                    ] {
                        if let Some(c) = chan(s, k, at) {
                            *v = c;
                        }
                    }
                }
                "MovieSceneMarginTrack" if prop == "Padding" => {
                    // A user widget's Padding (UUserWidget::SetPadding, set_user_padding); a channel without keys
                    // leaves its side as it is.
                    if t.node(w).ty != Ty::User {
                        continue;
                    }
                    let Some(&c) = t.widget(w).children.first() else { continue };
                    let o = &mut t.widget_mut(c).pad;
                    for (k, v) in [
                        ("LeftCurve", &mut o.l),
                        ("TopCurve", &mut o.t),
                        ("RightCurve", &mut o.r),
                        ("BottomCurve", &mut o.b),
                    ] {
                        if let Some(c) = chan(s, k, at) {
                            *v = c;
                        }
                    }
                }
                "MovieSceneWidgetMaterialTrack" => {
                    // Scalar parameters of the brush's dynamic material (channels 'S:<name>').
                    let Some(m) = &mut x.mid else { continue };
                    for (k, c) in &s.ch {
                        if let Some(name) = k.strip_prefix("S:")
                            && let Some(v) = eval_channel(c, at)
                        {
                            m.set_scalar(name, v as f32);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

// ---------- paint ----------

/// Affine [a b c d e f]: x' = a x + c y + e, y' = b x + d y + f.
type M = [f64; 6];

/// p . q
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

/// The pixel rect (x0, y0, x1, y1) a transformed local rect covers.
pub(crate) fn bounds(m: &M, w: f64, h: f64) -> [f64; 4] {
    bounds_x(m, 0.0, w, h)
}

/// `bounds` of the local rect from `x0` to `x1` across and 0 to `h` down.
fn bounds_x(m: &M, x0: f64, x1: f64, h: f64) -> [f64; 4] {
    let pts =
        [(x0, 0.0), (x1, 0.0), (x0, h), (x1, h)].map(|(x, y)| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]));
    let xs = pts.map(|p| p.0);
    let ys = pts.map(|p| p.1);
    let lo = |v: [f64; 4]| v.into_iter().fold(f64::INFINITY, f64::min);
    let hi = |v: [f64; 4]| v.into_iter().fold(f64::NEG_INFINITY, f64::max);
    [lo(xs), lo(ys), hi(xs), hi(ys)]
}

/// The convex polygon `p` cut to the convex polygon `z` (Sutherland-Hodgman; either winding): empty when they do
/// not meet.
fn intersect(p: &[[f64; 2]], z: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let n = z.len();
    let area: f64 = (0..n).map(|i| z[i][0] * z[(i + 1) % n][1] - z[(i + 1) % n][0] * z[i][1]).sum();
    if area == 0.0 {
        return Vec::new();
    }
    let sign = area.signum();
    let mut out = p.to_vec();
    for i in 0..n {
        if out.is_empty() {
            break;
        }
        let (a, b) = (z[i], z[(i + 1) % n]);
        let side = |v: [f64; 2]| sign * ((b[0] - a[0]) * (v[1] - a[1]) - (b[1] - a[1]) * (v[0] - a[0]));
        let input = std::mem::take(&mut out);
        for (k, &q0) in input.iter().enumerate() {
            let q1 = input[(k + 1) % input.len()];
            let (d0, d1) = (side(q0), side(q1));
            if d0 >= 0.0 {
                out.push(q0);
            }
            if (d0 >= 0.0) != (d1 >= 0.0) {
                let t = d0 / (d0 - d1);
                out.push([q0[0] + (q1[0] - q0[0]) * t, q0[1] + (q1[1] - q0[1]) * t]);
            }
        }
    }
    out
}

/// An editable text caret's width for its line height.
fn caret_width(height: f64) -> f64 {
    (0.08 * height).clamp(1.0, 2.0)
}

/// A colour with its alpha times a.
fn tint_of(c: Rgba, a: f64) -> Rgba {
    [c[0], c[1], c[2], c[3] * a]
}

struct Painter<'a> {
    t: &'a Tree,
    fonts: &'a mut Fonts,
    out: &'a mut ElementList,
    /// The tree's last paint of each widget, written as each is painted (a widget not painted keeps its last one).
    painted: std::cell::RefMut<'a, Vec<Option<Painted>>>,
    /// The zone of the rotated or sheared ClipToBounds widgets the paint is under (their rects' intersection).
    zone: Option<Zone>,
    /// The colour the paint is under: the user widgets' ColorAndOpacity above it (SCompoundWidget::OnPaint blends
    /// its ColorAndOpacity into the widget style's ColorAndOpacityTint, which every element below multiplies in;
    /// the alpha goes with the inherited opacity).
    tint: [f64; 3],
}

impl Painter<'_> {
    fn push(&mut self, clip: Option<[f64; 4]>, effects: DrawEffects, mut e: Element) {
        if self.tint != [1.0; 3] {
            let k = self.tint;
            let mul = |c: &mut Rgba| (0..3).for_each(|i| c[i] *= k[i]);
            match &mut e {
                Element::Box(b) => mul(&mut b.tint),
                Element::Text(x) => {
                    mul(&mut x.color);
                    mul(&mut x.outline_color);
                }
                Element::Material(m) => mul(&mut m.tint),
                Element::Line(l) => mul(&mut l.color),
                Element::Retained(r) => mul(&mut r.tint),
                Element::Blur(_) => {}
            }
        }
        let layer = i32::try_from(self.out.elements.len()).unwrap_or(i32::MAX);
        self.out.push_zoned(layer, clip, self.zone.clone(), effects, e);
    }

    fn children(&mut self, w: WidgetId, m: &M, a: f64, clip: Option<[f64; 4]>) {
        let t = self.t;
        let x = t.widget(w);
        if t.node(w).ty == Ty::WidgetSwitcher {
            if let Some(&c) = usize::try_from(x.active).ok().and_then(|i| x.children.get(i))
                && t.widget(c).vis != Vis::Collapsed
            {
                self.paint(c, m, a, clip);
            }
            return;
        }
        let (lo, hi) = order_range(t, &x.children);
        if lo == hi {
            for &c in &x.children {
                self.paint(c, m, a, clip);
            }
            return;
        }
        // each paint order from the lowest, equal ones in child order
        let mut z = Some(lo);
        while let Some(at) = z {
            let mut next: Option<i32> = None;
            for &c in &x.children {
                let k = t.widget(c).paint_order;
                if k == at {
                    self.paint(c, m, a, clip);
                } else if k > at && next.is_none_or(|n| k < n) {
                    next = Some(k);
                }
            }
            z = next;
        }
    }

    fn paint(&mut self, w: WidgetId, parent: &M, alpha: f64, clip: Option<[f64; 4]>) {
        let t = self.t;
        let x = t.widget(w);
        // A user widget's ColorAndOpacity (UUserWidget::SetColorAndOpacity -> its SObjectWidget's) tints its whole
        // content.
        if t.node(w).ty == Ty::User && x.color != [1.0; 4] {
            let outer = self.tint;
            self.tint = std::array::from_fn(|i| outer[i] * x.color[i]);
            self.paint_one(w, parent, alpha * x.color[3], clip);
            self.tint = outer;
        } else {
            self.paint_one(w, parent, alpha, clip);
        }
    }

    fn paint_one(&mut self, w: WidgetId, parent: &M, alpha: f64, clip: Option<[f64; 4]>) {
        let t = self.t;
        let x = t.widget(w);
        if !x.shown() {
            return;
        }
        let n = t.node(w);
        let r = &x.transform;
        let (pxs, pys) = (r.pivot[0] * n.w, r.pivot[1] * n.h);
        // T(x, y) . layout scale . T(pivot) . T(translation) . R . Shear . S . T(-pivot)
        let mut m = mul(parent, [n.ls, 0.0, 0.0, n.ls, n.x, n.y]);
        m = mul(&m, [1.0, 0.0, 0.0, 1.0, pxs + r.translation[0], pys + r.translation[1]]);
        if r.angle != 0.0 {
            let (sn, cs) = r.angle.to_radians().sin_cos();
            m = mul(&m, [cs, sn, -sn, cs, 0.0, 0.0]);
        }
        if r.shear != [0.0, 0.0] {
            m = mul(&m, [1.0, r.shear[1].to_radians().tan(), r.shear[0].to_radians().tan(), 1.0, 0.0, 0.0]);
        }
        m = mul(&m, [r.scale[0], 0.0, 0.0, r.scale[1], -pxs * r.scale[0], -pys * r.scale[1]]);
        self.painted[w.index()] = Some(Painted { transform: m, size: [n.w, n.h] });
        let a = alpha * x.opacity;
        if a <= 0.0 {
            // Nothing is drawn, but the subtree keeps its place for the hit test: SWidget::Paint adds a widget to
            // the hit test grid by its visibility, not its opacity. (A BackgroundBlur whose strength does not follow the
            // alpha still has its pass.)
            if n.ty == Ty::BackgroundBlur {
                self.blur(w, &m, 0.0, clip);
            }
            self.children(w, &m, 0.0, clip);
            return;
        }
        // PFG.DisablePixelSnapping 1 (UI_Manager OnInitialized): no element is snapped to the pixel grid.
        let geometry = PaintGeometry { size: [n.w, n.h], transform: m };
        let none = DrawEffects::NONE;
        // SImage: ColorAndOpacity x the brush's TintColor
        let color: Rgba = std::array::from_fn(|i| x.color[i] * x.brush_tint[i]);
        let brush = x.tex.as_deref().map(|p| t.refs.path(p));
        match n.ty {
            Ty::Image => match brush {
                Some(tex) if t.refs.materials.contains(tex) => {
                    // Material brushes: vertex colour = ColorAndOpacity x the brush's tint x the inherited opacity.
                    let material = x.mid.clone().unwrap_or_else(|| Mid::of(tex));
                    let e = MaterialEl {
                        geometry,
                        material,
                        tint: tint_of(color, a),
                        margin: None,
                        image: [x.brush_w, x.brush_h],
                    };
                    self.push(clip, none, Element::Material(e));
                }
                tex => {
                    let e = BoxEl {
                        geometry,
                        texture: tex.map(str::to_string),
                        margin: None,
                        tint: tint_of(color, a),
                        uv: [0.0, 0.0, 1.0, 1.0],
                        tile: [false; 2],
                    };
                    self.push(clip, none, Element::Box(e));
                }
            },
            Ty::Quad => {
                let Some(tex) = brush else { return self.children(w, &m, a, clip) };
                let material = x.mid.clone().unwrap_or_else(|| {
                    let mut mid = Mid::of(tex);
                    for (k, v) in x.quad.iter().flatten() {
                        mid.set_scalar(k, *v);
                    }
                    mid
                });
                // An outline (IsOutline): the Image is the RetainerBox's content, drawn into a target of the
                // widget's size under its render scale about the centre, tinted by its own colour (not the inherited
                // opacity); then the box's effect material (an MI_Quad_Masked instance with the corners and ColorA =
                // ColorB = the outline's colour) reads that target as its Mask, tinted by the instance's
                // ColorAndOpacity x the opacity.
                if let Some(ol) = &x.retained {
                    let mut effect = Mid::of(&t.refs.quad_masked);
                    for (k, v) in &ol.quad {
                        effect.set_scalar(k, *v);
                    }
                    let c = ol.color.map(|v| v as f32);
                    effect.set_vector("ColorA", c);
                    effect.set_vector("ColorB", c);
                    let content = MaterialEl {
                        geometry: PaintGeometry { size: [n.w * ol.scale[0], n.h * ol.scale[1]], transform: m },
                        material,
                        tint: x.color,
                        margin: None,
                        image: [x.brush_w, x.brush_h],
                    };
                    let e =
                        RetainedEl { geometry, content, content_scale: ol.scale, effect, tint: tint_of(ol.tint, a) };
                    let fx = DrawEffects(DrawEffects::PRE_MULTIPLIED_ALPHA.0 | DrawEffects::NO_GAMMA.0);
                    self.push(clip, fx, Element::Retained(Box::new(e)));
                    return;
                }
                let e = MaterialEl {
                    geometry,
                    material,
                    tint: tint_of(x.color, a),
                    margin: None,
                    image: [x.brush_w, x.brush_h],
                };
                self.push(clip, none, Element::Material(e));
            }
            Ty::Border => {
                if let Some(tex) = brush {
                    if t.refs.materials.contains(tex) {
                        // FSlateElementBatcher::AddBoxElement: the margins are fractions of the brush's image size
                        // and keep UV 0 -> margin in the corners. Vertex colour = BrushColor x the inherited opacity.
                        let material = x.mid.clone().unwrap_or_else(|| Mid::of(tex));
                        let e = MaterialEl {
                            geometry,
                            material,
                            tint: tint_of(x.brush_color, a),
                            margin: x.box_margin,
                            image: [x.brush_w, x.brush_h],
                        };
                        self.push(clip, none, Element::Material(e));
                    } else {
                        let e = BoxEl {
                            geometry,
                            texture: Some(tex.to_string()),
                            margin: x.box_margin,
                            tint: tint_of(x.brush_color, a),
                            uv: [0.0, 0.0, 1.0, 1.0],
                            tile: [false; 2],
                        };
                        self.push(clip, none, Element::Box(e));
                    }
                } else if x.solid {
                    // A brush without a resource draws the white texture: a solid BrushColor quad.
                    let e = BoxEl {
                        geometry,
                        texture: None,
                        margin: None,
                        tint: tint_of(x.brush_color, a),
                        uv: [0.0, 0.0, 1.0, 1.0],
                        tile: [false; 2],
                    };
                    self.push(clip, none, Element::Box(e));
                }
            }
            Ty::TextBlock => self.text(w, &m, a, clip),
            Ty::Throbber => self.throbber(w, &m, a, clip),
            Ty::CircularThrobber => self.circular_throbber(w, &m, a, clip),
            Ty::EditableTextBox => {
                let name = if x.edit_hot { "Slate_TextBox_Hovered" } else { "Slate_TextBox" };
                if let Some(tex) = t.refs.slate(name) {
                    let e = BoxEl {
                        geometry,
                        texture: Some(tex.to_string()),
                        margin: Some(Margin { l: 0.25, t: 0.25, r: 0.25, b: 0.25 }),
                        tint: tint_of(x.brush_color, a),
                        uv: [0.0, 0.0, 1.0, 1.0],
                        tile: [false; 2],
                    };
                    self.push(clip, none, Element::Box(e));
                }
                return self.editable_text(w, &m, a, clip);
            }
            Ty::BackgroundBlur => self.blur(w, &m, a, clip),
            _ => {}
        }
        // ClipToBounds: the children clip to this widget's transformed rect: its pixel bounds as a
        // scissor rect and, when it is rotated or sheared, the rect itself as a zone (FSlateClippingZone, which Slate
        // draws by the stencil).
        if !x.clip {
            return self.children(w, &m, a, clip);
        }
        let bleed = x.clip_bleed;
        let b = bounds_x(&m, -bleed, n.w + bleed, n.h);
        let clip = Some(match clip {
            Some(c) => [b[0].max(c[0]), b[1].max(c[1]), b[2].min(c[2]), b[3].min(c[3])],
            None => b,
        });
        if m[1].abs() < 1e-9 && m[2].abs() < 1e-9 {
            return self.children(w, &m, a, clip);
        }
        let corners: Vec<[f64; 2]> = [(-bleed, 0.0), (n.w + bleed, 0.0), (n.w + bleed, n.h), (-bleed, n.h)]
            .iter()
            .map(|&(px, py)| [m[0] * px + m[2] * py + m[4], m[1] * px + m[3] * py + m[5]])
            .collect();
        let zone: Zone = match &self.zone {
            Some(z) => intersect(&corners, z).into(),
            None => corners.into(),
        };
        let outer = self.zone.replace(zone);
        self.children(w, &m, a, clip);
        self.zone = outer;
    }

    /// UBackgroundBlur's pass at the widget's place in the order (menu/blur.rs): what was drawn before it, blurred
    /// inside the widget's geometry. The strength is its SetBlurStrength or animated value, else its tree's, times the
    /// alpha it is painted with under bApplyAlphaToBlur; 0 or less: no pass.
    // Deviation: a rotated or sheared blur takes the clip of its bounding box, not of its geometry.
    fn blur(&mut self, w: WidgetId, m: &M, alpha: f64, clip: Option<[f64; 4]>) {
        let t = self.t;
        let x = t.widget(w);
        let n = t.node(w);
        let strength = x.blur_set.unwrap_or(x.blur) * if x.blur_apply_alpha { alpha } else { 1.0 };
        if strength.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) || n.w <= 0.0 || n.h <= 0.0 {
            return;
        }
        let e = BlurEl { geometry: PaintGeometry { size: [n.w, n.h], transform: *m }, strength };
        self.push(clip, DrawEffects::NONE, Element::Blur(e));
    }

    /// SThrobber (Unverified: from the 4.26 source, not read in this build's exe): piece i runs a 1.5 s linear curve
    /// that starts 0.05 s x i into a looping sequence (1.6 s for three pieces); its content scale on the animated axes
    /// (and its alpha with bAnimateOpacity) is sin(2 pi x the curve), so a piece is gone for the second half of its
    /// curve. The image is centred in its piece.
    fn throbber(&mut self, w: WidgetId, m: &M, a: f64, clip: Option<[f64; 4]>) {
        let t = self.t;
        let x = t.widget(w);
        let n = t.node(w);
        let Some(tex) = x.tex.as_deref().map(|p| t.refs.path(p)).filter(|p| t.refs.materials.contains(*p)) else {
            return;
        };
        let total = f64::from(x.pieces.saturating_sub(1)) * 0.05 + 1.5;
        let time = t.time % total;
        let cw = x.brush_w + 4.0;
        let x0 = (n.w - f64::from(x.pieces) * cw) / 2.0;
        for i in 0..x.pieces {
            let v = (2.0 * std::f64::consts::PI * ((time - f64::from(i) * 0.05) / 1.5).clamp(0.0, 1.0)).sin();
            let pw = x.brush_w * if x.throb_h { v } else { 1.0 };
            let ph = x.brush_h * if x.throb_v { v } else { 1.0 };
            let alpha = x.color[3] * a * if x.throb_opacity { v } else { 1.0 };
            if pw <= 0.0 || ph <= 0.0 || alpha <= 0.0 {
                continue;
            }
            let at = mul(m, [1.0, 0.0, 0.0, 1.0, x0 + f64::from(i) * cw + (cw - pw) / 2.0, (n.h - ph) / 2.0]);
            let e = MaterialEl {
                geometry: PaintGeometry { size: [pw, ph], transform: at },
                material: Mid::of(tex),
                tint: [x.color[0], x.color[1], x.color[2], alpha],
                margin: None,
                image: [pw, ph],
            };
            self.push(clip, DrawEffects::NONE, Element::Material(e));
        }
    }

    /// SCircularThrobber::OnPaint (Unverified: from the 4.26 source): piece i of N at angle 2 pi / N x i + the phase of
    /// the period, on a circle that keeps the piece inside the widget, scaled (i + 1) / N.
    fn circular_throbber(&mut self, w: WidgetId, m: &M, a: f64, clip: Option<[f64; 4]>) {
        let t = self.t;
        let x = t.widget(w);
        let n = t.node(w);
        let phase = (t.time / x.period % 1.0) * 2.0 * std::f64::consts::PI;
        let (ox, oy) = ((n.w - x.brush_w) / 2.0, (n.h - x.brush_h) / 2.0);
        for i in 0..x.pieces {
            let ang = 2.0 * std::f64::consts::PI / f64::from(x.pieces) * f64::from(i) + phase;
            let k = f64::from(i + 1) / f64::from(x.pieces);
            let at = mul(m, [1.0, 0.0, 0.0, 1.0, ox + ox * ang.sin(), oy + oy * ang.cos()]);
            let e = BoxEl {
                geometry: PaintGeometry { size: [x.brush_w * k, x.brush_h * k], transform: at },
                texture: x.tex.as_deref().map(|p| t.refs.path(p).to_string()),
                margin: None,
                tint: tint_of(x.color, a),
                uv: [0.0, 0.0, 1.0, 1.0],
                tile: [false; 2],
            };
            self.push(clip, DrawEffects::NONE, Element::Box(e));
        }
    }

    /// An EditableTextBox's text (SEditableText): cut to the box and scrolled left so the caret stays inside the
    /// box's padding.
    // Unverified: SEditableText's clipping and its scroll to the caret (FSlateEditableTextLayout's ScrollOffset) are
    // from the 4.26 source; the scroll here follows the caret alone (the text's start in view until the caret passes
    // the right edge, then the caret at that edge), not the source's offset that stays where it was.
    fn editable_text(&mut self, w: WidgetId, m: &M, a: f64, clip: Option<[f64; 4]>) {
        let t = self.t;
        let n = t.node(w);
        let mut scroll = 0.0;
        if let Some(&c) = t.widget(w).children.first() {
            let (cx, cn) = (t.widget(c), t.node(c));
            if cx.caret >= 0 {
                let f = font_info(cx, cn.w);
                let before: String = cx.text.chars().take(usize::try_from(cx.caret).unwrap_or(0)).collect();
                let at = self.fonts.measure(&f, &before, cn.text_scale).width;
                let height = self.fonts.measure(&f, "", cn.text_scale).height;
                // the text's padding is the same on both sides (the style's Padding, (4, 2) by default)
                let right = n.w - cn.x;
                scroll = (cn.x + at + caret_width(height) - right).max(0.0);
            }
        }
        let b = bounds(m, n.w, n.h);
        let clip = Some(match clip {
            Some(c) => [b[0].max(c[0]), b[1].max(c[1]), b[2].min(c[2]), b[3].min(c[3])],
            None => b,
        });
        self.children(w, &mul(m, [1.0, 0.0, 0.0, 1.0, -scroll, 0.0]), a, clip);
    }

    /// An editable text's caret (Unverified: FSlateEditableTextLayout / SlateEditableTextTypes of the 4.26 source):
    /// before the character at its index, the line's height tall and 8% of that wide (1 to 2
    /// units), in the text colour; shown for the first and last quarter of each second since it last moved.
    fn caret(&mut self, w: WidgetId, m: &M, a: f64, clip: Option<[f64; 4]>) {
        let t = self.t;
        let x = t.widget(w);
        let n = t.node(w);
        if (2.0 * std::f64::consts::PI * (t.time - x.caret_time)).cos() < 0.0 {
            return;
        }
        let f = font_info(x, n.w);
        let before: String = x.text.chars().take(usize::try_from(x.caret).unwrap_or(0)).collect();
        let at = self.fonts.measure(&f, &before, n.text_scale).width;
        let height = self.fonts.measure(&f, "", n.text_scale).height;
        let geometry =
            PaintGeometry { size: [caret_width(height), height], transform: mul(m, [1.0, 0.0, 0.0, 1.0, at, 0.0]) };
        let e = BoxEl { geometry, tint: tint_of(x.color, a), uv: [0.0, 0.0, 1.0, 1.0], ..BoxEl::default() };
        self.push(clip, DrawEffects::NONE, Element::Box(e));
    }

    /// A TextBlock's runs. FTextLayout::CreateLineViewBlocks (exe 1417988c0): line i is
    /// i x the run height x LineHeightPercentage under the top; FTextLayout::JustifyLayout (exe 1417a5fa0) and
    /// FSlateTextBlockLayout::OnPaint (14182ee20) offset a line by (allotted width - line width) x 0.5 (Center) or
    /// x 1 (Right). The drop shadow first (a non-zero offset and alpha), with the outline when
    /// bApplyOutlineToDropShadows, in the shadow colour; then the outline and the fill.
    fn text(&mut self, w: WidgetId, m: &M, a: f64, clip: Option<[f64; 4]>) {
        let t = self.t;
        let x = t.widget(w);
        let n = t.node(w);
        if x.caret >= 0 {
            self.caret(w, m, a, clip);
        }
        let text = x.shown_text();
        if text.is_empty() {
            return;
        }
        let s = n.text_scale;
        let f = font_info(x, n.w);
        let metrics = self.fonts.measure(&f, &text, s);
        let run = self.fonts.shape(&f, &text, s);
        let origins: Vec<[f64; 2]> =
            (0..metrics.lines.len()).map(|i| metrics.line_origin(i, &f, n.w, x.justify, x.line_height)).collect();
        // The glyphs in pixels at the layout scale, each line placed and justified.
        // (a shadow draws its glyphs in its own colour, an emoji as its silhouette in place of its layers)
        let placed = |dx: f64, dy: f64, outline: bool, shadow: bool| ShapedRun {
            glyphs: run
                .glyphs
                .iter()
                .filter(|g| outline || !g.outline)
                .filter(|g| g.part != if shadow { GlyphPart::Layer } else { GlyphPart::Silhouette })
                .map(|g| {
                    let o = origins.get(g.line as usize).copied().unwrap_or([0.0; 2]);
                    let mut g = *g;
                    g.x += (o[0] + dx) * s;
                    g.y += (o[1] + dy) * s;
                    if shadow {
                        g.part = GlyphPart::Whole;
                    }
                    g
                })
                .collect(),
        };
        // The element's transform puts 1 / the layout scale in front of the glyphs' pixels.
        let geometry =
            PaintGeometry { size: [n.w * s, n.h * s], transform: mul(m, [1.0 / s, 0.0, 0.0, 1.0 / s, 0.0, 0.0]) };
        let material = x.font_mat.as_ref().map(|p| x.font_mid.clone().unwrap_or_else(|| Mid::of(p)));
        let [sx, sy] = x.shadow_offset;
        if (sx != 0.0 || sy != 0.0) && x.shadow_color[3] > 0.0 {
            let e = TextEl {
                geometry,
                run: placed(sx, sy, x.outline_shadows && x.outline > 0.0, true),
                color: tint_of(x.shadow_color, a),
                outline_color: tint_of(x.shadow_color, a),
                material: material.clone(),
            };
            self.push(clip, DrawEffects::NONE, Element::Text(e));
        }
        let e = TextEl {
            geometry,
            run: placed(0.0, 0.0, x.outline > 0.0, false),
            color: tint_of(x.color, a),
            outline_color: tint_of(x.outline_color, a),
            material,
        };
        self.push(clip, DrawEffects::NONE, Element::Text(e));
    }
}

/// The tree's elements at `opacity`, after its layout: the root under the view's DPI scale.
pub fn paint(t: &Tree, fonts: &mut Fonts, out: &mut ElementList, opacity: f64) {
    if t.widgets.is_empty() {
        return;
    }
    let mut p = Painter { t, fonts, out, painted: t.painted.borrow_mut(), zone: None, tint: [1.0; 3] };
    let s = t.scale;
    p.paint(t.root(), &[s, 0.0, 0.0, s, 0.0, 0.0], opacity, None);
}

// ---------- the menus' view: user widgets made into one tree ----------

/// The lowest and highest paint order of `kids`, 0 included.
fn order_range(t: &Tree, kids: &[WidgetId]) -> (i32, i32) {
    let (mut lo, mut hi) = (0, 0);
    for &c in kids {
        let z = t.widget(c).paint_order;
        lo = lo.min(z);
        hi = hi.max(z);
    }
    (lo, hi)
}

impl Tree {
    /// A tree with no widget on bundle `b`: the menus' view makes its user widgets into it (`create`), each a root
    /// of its own until it is added to a parent.
    pub fn empty(b: &Bundle) -> Tree {
        Tree { refs: Arc::new(Refs::of(b)), scale: 1.0, ..Tree::default() }
    }

    /// CreateWidget of `class` named `name` into the tree, in no parent.
    pub fn create(&mut self, b: &Bundle, class: &str, name: &str) -> WidgetId {
        self.create_user(b, None, class, name)
    }

    /// A widget of type `kind` (a CanvasPanel) named `name` with the type's defaults, in no parent.
    pub fn create_plain(&mut self, b: &Bundle, kind: &str, name: &str) -> WidgetId {
        let empty = Value::Object(Default::default());
        let w = make_widget(b, kind, name, &empty, &empty, "");
        self.push(None, w, Node { ty: ty_of(kind), ..Node::default() })
    }

    /// The widget of node `n` (a widget type or a user class, its properties, slot and children: `new Widget(type,
    /// name, props, slotProps, slot)`) made into the tree, in no parent. A user class's controllers are not made
    /// (`MenuView::create` makes them).
    pub fn create_node(&mut self, b: &Bundle, n: &WData) -> WidgetId {
        let it = item_of(&mut self.recipes, b, n);
        self.stamp(&it, None)
    }

    /// The user widget class of `w` (a WBP_Quadrilateral's too), None for a plain widget.
    pub fn user_class(&self, w: WidgetId) -> Option<&str> {
        self.node(w).user.as_deref()
    }

    /// `w` and every widget under it, children before their parent.
    pub fn post_order(&self, w: WidgetId) -> Vec<WidgetId> {
        let mut out = Vec::new();
        self.post_order_into(w, &mut out);
        out
    }

    fn post_order_into(&self, w: WidgetId, out: &mut Vec<WidgetId>) {
        for &c in &self.widget(w).children {
            self.post_order_into(c, out);
        }
        out.push(w);
    }

    /// `find_in` that answers None for a missing name.
    pub fn find_opt(&self, under: WidgetId, name: &str) -> Option<WidgetId> {
        self.lookup(under, under, name)
    }

    /// `child` put into `parent`'s children at `at` (the end when None or past it: Widget.add(child, at)).
    pub fn insert(&mut self, parent: WidgetId, child: WidgetId, at: Option<usize>) {
        self.remove(child);
        self.widget_mut(child).parent = Some(parent);
        let i = self.slot_index(parent, child, at);
        self.widget_mut(parent).children.insert(i, child);
        self.shape += 1;
    }

    /// The root of the subtree that holds `w`.
    pub fn top(&self, w: WidgetId) -> WidgetId {
        let mut r = w;
        while let Some(p) = self.widget(r).parent {
            r = p;
        }
        r
    }

    /// The widget names a cursor of its own (its Cursor property).
    pub fn has_cursor(&self, w: WidgetId) -> bool {
        self.node(w).cursor
    }

    /// WBP_Quadrilateral `w` made an outline by its owner: IsOutline and
    /// OutlineThickness written to its properties, then SetParameters' outline (`make_outline`).
    pub fn set_outline(&mut self, w: WidgetId, thickness: f64) {
        let i = self.at(w);
        let n = &mut self.nodes[i];
        if let Some(Value::Object(m)) = n.props.as_mut().map(Arc::make_mut) {
            m.insert("IsOutline".into(), Value::Bool(true));
            m.insert("OutlineThickness".into(), thickness.into());
        }
        let p = n.props.clone();
        make_outline(self.widget_mut(w), p.as_deref().unwrap_or(&NULL));
    }

    /// An owner's write of one property of user widget `w` (`props.<key> = v`), read by `w`'s controller later.
    pub fn set_prop(&mut self, w: WidgetId, key: &str, v: Value) {
        let i = self.at(w);
        if let Some(Value::Object(m)) = self.nodes[i].props.as_mut().map(Arc::make_mut) {
            m.insert(key.into(), v);
        }
    }

    /// The layout scale the last prepass measured `w` at (a text's wrapping and metrics use it).
    pub fn text_scale(&self, w: WidgetId) -> f64 {
        self.node(w).text_scale
    }

    /// The size the last layout arranged `w` at, Slate units.
    pub fn arranged_size(&self, w: WidgetId) -> [f64; 2] {
        let n = self.node(w);
        [n.w, n.h]
    }

    /// A WidgetSwitcher.
    pub fn is_switcher(&self, w: WidgetId) -> bool {
        self.node(w).ty == Ty::WidgetSwitcher
    }

    /// The widget count (ids run from 0 to it).
    pub fn len(&self) -> usize {
        self.widgets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.widgets.is_empty()
    }
}

/// Measure and arrange of the subtree `w` in `area` at the DPI `scale` (`layout` of one root). Each runs only
/// when running it would change something, so a layout skipped leaves what running it gives: the measure when its
/// last one was at another scale or no longer holds (an input below changed: a written widget's layout key,
/// `Tree::settle`; or an arrange changed a width or area it reads), the arrange when the measure ran, the area
/// changed or another root's arrange placed the widget since.
pub fn layout_widget(t: &mut Tree, fonts: &mut Fonts, w: WidgetId, area: Geometry, scale: f64) {
    t.scale = scale;
    t.settle();
    let n = t.node(w);
    let measured = !n.dirty && n.measured_at == Some(scale.to_bits());
    let rect = [area.pos[0], area.pos[1], area.size[0], area.size[1]].map(f64::to_bits);
    if measured && n.arranged == Some(rect) {
        return;
    }
    if !measured {
        measure(t, fonts, w, scale);
    }
    arrange(t, w, area.pos[0], area.pos[1], area.size[0], area.size[1]);
    t.laid_out(w, Some(rect));
}

/// Measure only of the subtree `w` (ForceLayoutPrepass): its desired size, Slate units. Run only when its last
/// measure was at another scale or no longer holds, as `layout_widget`'s.
pub fn measure_widget(t: &mut Tree, fonts: &mut Fonts, w: WidgetId, scale: f64) -> [f64; 2] {
    t.scale = scale;
    t.settle();
    let n = t.node(w);
    if n.dirty || n.measured_at != Some(scale.to_bits()) {
        measure(t, fonts, w, scale);
        t.laid_out(w, None);
    }
    t.desired_size(w)
}

/// The subtree `w`'s elements under the transform `m` (device pixels) at `opacity`, after its layout.
pub fn paint_widget(t: &Tree, fonts: &mut Fonts, out: &mut ElementList, w: WidgetId, m: [f64; 6], opacity: f64) {
    let mut p = Painter { t, fonts, out, painted: t.painted.borrow_mut(), zone: None, tint: [1.0; 3] };
    p.paint(w, &m, opacity, None);
}

/// The topmost widget of the subtree `w` at the device pixel (x, y) by the last paint: a shown,
/// hit-testable widget whose rect holds the point, children first in reverse paint order; a clipping widget's
/// children only inside it.
pub fn hit_test(t: &Tree, w: WidgetId, x: f64, y: f64) -> Option<WidgetId> {
    let wd = t.widget(w);
    if !wd.shown() || wd.vis == Vis::HitTestInvisible {
        return None;
    }
    let p = t.painted(w)?;
    let m = p.transform;
    let det = m[0] * m[3] - m[1] * m[2];
    if det == 0.0 {
        return None;
    }
    let (dx, dy) = (x - m[4], y - m[5]);
    let lx = (m[3] * dx - m[2] * dy) / det;
    let ly = (m[0] * dy - m[1] * dx) / det;
    let n = t.node(w);
    let inside = lx >= 0.0 && ly >= 0.0 && lx < n.w && ly < n.h;
    if wd.clip && !inside {
        return None;
    }
    let kids = if n.ty == Ty::WidgetSwitcher { t.kids(w) } else { wd.children.clone() };
    let (lo, hi) = order_range(t, &kids);
    let mut z = Some(hi);
    while let Some(at) = z {
        let mut next: Option<i32> = None;
        for &c in kids.iter().rev() {
            let k = t.widget(c).paint_order;
            if k == at {
                if let Some(r) = hit_test(t, c, x, y) {
                    return Some(r);
                }
            } else if k < at && next.is_none_or(|q| k > q) {
                next = Some(k);
            }
        }
        if lo == hi {
            break;
        }
        z = next;
    }
    (inside && wd.vis == Vis::Visible).then_some(w)
}

/// UI_Functions.IsVisibleInHierarchy: `w` and every ancestor shown, each the active
/// child of a WidgetSwitcher parent.
pub fn visible_in_hierarchy(t: &Tree, w: WidgetId) -> bool {
    t.at(w);
    let mut links = t.links.borrow_mut();
    links.update(&t.widgets, &t.nodes);
    let up = &links.up;
    let mut c = w.i;
    loop {
        let l = up[c as usize];
        if !l.shown {
            return false;
        }
        if l.parent == NO_LINK {
            return true;
        }
        let a = up[l.parent as usize].active;
        if a != ANY_CHILD && a != c {
            return false;
        }
        c = l.parent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::TextureInfo;

    #[test]
    fn a_bundle_name_set_at_run_time_paints_its_entry() {
        let mut b = Bundle::default();
        let tex = "/Game/UI/ui_icons_cart.ui_icons_cart";
        b.textures.insert("ui_icons_cart".into(), TextureInfo { path: Some(tex.into()), ..TextureInfo::default() });
        b.materials.insert("MI_Profile".into(), "/Game/UI/MI_Profile.MI_Profile".into());
        let r = Refs::of(&b);
        assert_eq!(r.path("ui_icons_cart"), tex);
        assert_eq!(r.path("MI_Profile"), "/Game/UI/MI_Profile.MI_Profile");
        assert!(r.materials.contains(r.path("MI_Profile")));
        // an object path, or a name the bundle does not hold, stays as it is
        assert_eq!(r.path(tex), tex);
        assert_eq!(r.path("icon_128_15"), "icon_128_15");
    }

    /// The dev store's menu bundle and fonts; None without a store (the test says so and passes).
    fn env() -> Option<(Bundle, Fonts)> {
        let store = match mvs_store::Store::open(mvs_store::dev_store_dir()) {
            Ok(s) if s.version("font").is_some() && s.version("ui").is_some() => s,
            _ => {
                eprintln!("no dev store with the font and ui kinds: skipped");
                return None;
            }
        };
        let b = Bundle::load(&store, "menu").ok()?;
        let fonts = Fonts::new(&store, &b).ok()?;
        Some((b, fonts))
    }

    /// One tree twice: `a` laid out by `layout_widget` (which runs only what changed), `b` measured and arranged
    /// in full each frame as before the layout kept what held. Every change is made to both.
    struct Twin {
        a: Tree,
        b: Tree,
        root: WidgetId,
        area: Geometry,
        scale: f64,
    }

    impl Twin {
        fn change(&mut self, f: impl Fn(&mut Tree)) {
            f(&mut self.a);
            f(&mut self.b);
        }

        /// A frame of both; their layouts the same. Returns whether `a` ran a measure or arrange.
        fn frame(&mut self, fonts: &mut Fonts, what: &str) -> bool {
            let runs = self.a.layouts;
            layout_widget(&mut self.a, fonts, self.root, self.area, self.scale);
            let g = self.area;
            self.b.scale = self.scale;
            measure(&mut self.b, fonts, self.root, self.scale);
            arrange(&mut self.b, self.root, g.pos[0], g.pos[1], g.size[0], g.size[1]);
            assert_eq!(self.a.len(), self.b.len(), "{what}");
            for i in 0..self.a.len() {
                let (x, y) = (&self.a.nodes[i], &self.b.nodes[i]);
                let of = |n: &Node| [n.dw, n.dh, n.x, n.y, n.w, n.h, n.ls, n.text_scale].map(f64::to_bits);
                let name = &self.a.widgets[i].name;
                assert_eq!(of(x), of(y), "{what}: {name}'s layout");
                assert_eq!(x.last.map(f64::to_bits), y.last.map(f64::to_bits), "{what}: {name}'s area");
                let (p, q) = (&self.a.widgets[i], &self.b.widgets[i]);
                assert_eq!(p.scroll.to_bits(), q.scroll.to_bits(), "{what}: {name}'s scroll");
                assert_eq!(p.scroll_max.to_bits(), q.scroll_max.to_bits(), "{what}: {name}'s scroll max");
            }
            self.a.layouts != runs
        }

        /// Frames until `a`'s layout holds (at most 4), each checked.
        fn settle(&mut self, fonts: &mut Fonts, what: &str) -> usize {
            let mut n = 0;
            while self.frame(fonts, what) {
                n += 1;
                assert!(n < 4, "{what}: the layout does not settle");
            }
            n
        }
    }

    /// The widget of type `kind` named `name` added to `parent`.
    fn plain(t: &mut Tree, b: &Bundle, parent: WidgetId, kind: &str, name: &str) -> WidgetId {
        let w = t.create_plain(b, kind, name);
        t.add(parent, w);
        w
    }

    /// The tree's widgets by name (made the same in both copies, so the ids are the same).
    fn id(t: &Tree, name: &str) -> WidgetId {
        let i = t.widgets.iter().position(|w| w.name == name).unwrap_or_else(|| panic!("no {name}"));
        WidgetId { i: u32::try_from(i).expect("ids fit"), g: t.gens[i] }
    }

    /// A canvas holding a vertical box of an auto-wrapping text, a scale box, a switcher of two boxes, a scroll box
    /// of five boxes and a wrap box of four.
    fn build(b: &Bundle) -> (Tree, WidgetId) {
        let mut t = Tree::empty(b);
        let root = t.create_plain(b, "CanvasPanel", "root");
        let col = plain(&mut t, b, root, "VerticalBox", "col");
        {
            let x = t.widget_mut(col);
            x.anchors = [0.0, 0.0, 1.0, 1.0];
            x.offsets = Margin::default();
        }
        let text = plain(&mut t, b, col, "TextBlock", "text");
        {
            let x = t.widget_mut(text);
            x.text = "A line of words long enough to wrap in the box it sits in".into();
            x.auto_wrap = true;
        }
        let scale = plain(&mut t, b, col, "ScaleBox", "scale");
        let inner = plain(&mut t, b, scale, "SizeBox", "inner");
        t.widget_mut(inner).w_override = 300.0;
        t.widget_mut(inner).h_override = 40.0;
        let sw = plain(&mut t, b, col, "WidgetSwitcher", "switch");
        for (i, h) in [30.0, 70.0].into_iter().enumerate() {
            let k = plain(&mut t, b, sw, "SizeBox", &format!("page{i}"));
            t.widget_mut(k).h_override = h;
        }
        let sb = plain(&mut t, b, col, "ScrollBox", "scroll");
        t.widget_mut(sb).fill = 1.0;
        for i in 0..5 {
            let k = plain(&mut t, b, sb, "SizeBox", &format!("row{i}"));
            t.widget_mut(k).h_override = 50.0;
        }
        let wrap = plain(&mut t, b, col, "WrapBox", "wrap");
        for i in 0..4 {
            let k = plain(&mut t, b, wrap, "SizeBox", &format!("cell{i}"));
            t.widget_mut(k).w_override = 90.0;
            t.widget_mut(k).h_override = 20.0;
        }
        (t, root)
    }

    /// The layout runs only when something it reads changed, and then gives what a full layout each frame gives:
    /// over render transforms, opacity and Hidden (no layout), text, Collapsed, a child added, removed or destroyed
    /// and its id taken again, a switcher's page, a scroll offset past the end, the area and scale, and a measure of
    /// an inner widget at another scale.
    #[test]
    fn a_layout_runs_only_when_an_input_changed() {
        let Some((bundle, mut fonts)) = env() else { return };
        let (t, root) = build(&bundle);
        let area = Geometry { pos: [0.0, 0.0], size: [320.0, 400.0] };
        let mut tw = Twin { a: t.clone(), b: t, root, area, scale: 1.5 };
        assert!(tw.frame(&mut fonts, "first"));
        tw.settle(&mut fonts, "first");
        assert!(!tw.frame(&mut fonts, "nothing changed"), "a layout ran with nothing changed");

        let (text, sw, sb, inner, col) = {
            let t = &tw.a;
            (id(t, "text"), id(t, "switch"), id(t, "scroll"), id(t, "inner"), id(t, "col"))
        };
        tw.change(|t| {
            let x = t.widget_mut(text);
            x.transform.translation = [10.0, 4.0];
            x.transform.angle = 30.0;
            x.opacity = 0.5;
            x.color = [1.0, 0.0, 0.0, 1.0];
            t.widget_mut(inner).vis = Vis::Hidden;
        });
        assert!(!tw.frame(&mut fonts, "paint only"), "a render transform, opacity or Hidden laid out again");

        type Change = Box<dyn Fn(&mut Tree)>;
        let cases: Vec<(&str, Change)> = vec![
            ("text", Box::new(move |t| t.widget_mut(text).text = "Short".into())),
            (
                "longer text",
                Box::new(move |t| {
                    t.widget_mut(text).text = "Many more words again, to wrap twice or more in a narrow box".into()
                }),
            ),
            ("collapsed", Box::new(move |t| t.widget_mut(inner).vis = Vis::Collapsed)),
            ("shown", Box::new(move |t| t.widget_mut(inner).vis = Vis::Visible)),
            ("switcher page", Box::new(move |t| t.widget_mut(sw).active = 1)),
            ("scroll past the end", Box::new(move |t| t.widget_mut(sb).scroll = 900.0)),
            ("scroll back", Box::new(move |t| t.widget_mut(sb).scroll = 20.0)),
            ("padding", Box::new(move |t| t.widget_mut(inner).pad = Margin { l: 5.0, t: 2.0, r: 5.0, b: 2.0 })),
            ("a child taken out", Box::new(move |t| t.remove(sw))),
            ("a child put back", Box::new(move |t| t.insert(col, sw, Some(1)))),
        ];
        for (what, f) in &cases {
            tw.change(f);
            assert!(tw.frame(&mut fonts, what), "{what}: no layout ran");
            tw.settle(&mut fonts, what);
        }

        // a child destroyed and its id taken by a widget made after
        tw.change(|t| t.destroy(id(t, "row4")));
        tw.settle(&mut fonts, "destroyed");
        tw.change(|t| {
            let k = plain(t, &bundle, sb, "SizeBox", "row5");
            t.widget_mut(k).h_override = 120.0;
        });
        assert!(tw.frame(&mut fonts, "made"), "a new child: no layout ran");
        tw.settle(&mut fonts, "made");

        // the area and the scale
        tw.area.size = [280.0, 420.0];
        assert!(tw.frame(&mut fonts, "area"));
        tw.settle(&mut fonts, "area");
        tw.scale = 2.0;
        assert!(tw.frame(&mut fonts, "scale"));
        tw.settle(&mut fonts, "scale");

        // an inner widget measured on its own at another scale: the root's layout no longer holds
        let _ = measure_widget(&mut tw.a, &mut fonts, col, 1.0);
        measure(&mut tw.b, &mut fonts, col, 1.0);
        assert!(tw.frame(&mut fonts, "inner measure"), "an inner measure left the root's layout");
        tw.settle(&mut fonts, "inner measure");
        assert!(!tw.frame(&mut fonts, "settled"));

        // only a wrapping text's width changes (the scale box, whose area its next measure reads too, collapsed): its
        // next measure wraps at the new width
        tw.change(|t| t.widget_mut(id(t, "scale")).vis = Vis::Collapsed);
        tw.settle(&mut fonts, "scale box collapsed");
        tw.area.size = [160.0, 420.0];
        assert!(tw.frame(&mut fonts, "narrow"));
        assert_eq!(tw.settle(&mut fonts, "narrow"), 1, "the text wraps again on the next frame");
    }

    /// The visibility walk over the kept links answers as the widgets do after
    /// writes, a switcher's page, a widget taken out and one destroyed.
    #[test]
    fn visible_in_hierarchy_follows_the_writes() {
        fn walk(t: &Tree, w: WidgetId) -> bool {
            let mut c = w;
            loop {
                if !t.widget(c).shown() {
                    return false;
                }
                let Some(p) = t.widget(c).parent else { return true };
                if t.is_switcher(p) {
                    let pw = t.widget(p);
                    if usize::try_from(pw.active).ok().and_then(|i| pw.children.get(i)) != Some(&c) {
                        return false;
                    }
                }
                c = p;
            }
        }
        let b = Bundle::default();
        let (mut t, _) = build(&b);
        let check = |t: &Tree, what: &str| {
            for w in t.ids() {
                assert_eq!(visible_in_hierarchy(t, w), walk(t, w), "{what}: {}", t.widget(w).name);
            }
        };
        check(&t, "made");
        let (sw, col, inner) = (id(&t, "switch"), id(&t, "col"), id(&t, "inner"));
        t.widget_mut(sw).active = 1;
        check(&t, "page");
        t.widget_mut(col).vis = Vis::Hidden;
        check(&t, "hidden");
        t.widget_mut(col).vis = Vis::SelfHitTestInvisible;
        t.remove(inner);
        check(&t, "taken out");
        t.destroy(sw);
        check(&t, "destroyed");
        let k = t.create_plain(&b, "SizeBox", "new");
        t.add(col, k);
        check(&t, "made again");
    }

    /// A destroyed widget's ids go stale: its index is taken again under the next generation, the old id names no
    /// widget, and the counts follow.
    #[test]
    fn destroyed_ids_go_stale_and_their_index_is_taken_again() {
        let b = Bundle::default();
        let (mut t, _) = build(&b);
        let (made, held) = (t.births(), t.in_use());
        let col = id(&t, "col");
        let row = id(&t, "row4");
        t.destroy(row);
        assert!(!t.alive(row));
        assert_eq!(t.in_use(), held - 1);
        assert!(t.ids().all(|w| w != row));
        let k = t.create_plain(&b, "SizeBox", "new");
        assert_eq!(k.index(), row.index());
        assert_ne!(k, row);
        assert!(t.alive(k) && !t.alive(row));
        t.add(col, k);
        assert_eq!((t.births(), t.in_use()), (made + 1, held));
        assert!(t.born(k) > t.born(col));
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| t.widget(row).name.clone())).is_err());
    }
}
