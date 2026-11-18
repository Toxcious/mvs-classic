//! UI_IconButton, UI_ButtonSecondary, UI_PlusMinusHorizontal, UI_ToggleSwitchRework, UI_ButtonMain,
//! UI_ButtonSecondary_Vertical and UI_ButtonCartoon.

use std::any::Any;

use serde_json::Value;

use super::input_label::{InputLabel, InputLabelParams, enum_name};
use super::interactor::{InteractorState, set_interactable};
use super::tooltip::{menu_placement, tooltip_params};
use super::{
    bind, find, focus_frame, override_size, prop, prop_bool, prop_f64, quad_color, quad_wobble, set_id, set_tooltip,
    sound_overrides, wd,
};
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::manager::Manager;
use crate::menu::widgets::common::{enum_index, margin, rgba, text};
use crate::slate::{Margin, Vis, WidgetId};

const SHOWN: Vis = Vis::SelfHitTestInvisible;

fn shown(on: bool) -> Vis {
    if on { SHOWN } else { Vis::Collapsed }
}

fn hidden(on: bool) -> Vis {
    if on { Vis::Hidden } else { SHOWN }
}

/// The interactor child of the controller's widget.
fn interactor_of(cx: &Cx<'_>) -> WidgetId {
    find(cx, "UI_Interactor")
}

// The interactor's three standard events, bound by every button below (ids 1 to 3 of its own handlers).
const H_FOCUS: u32 = 1;
const H_UNFOCUS: u32 = 2;
const H_CLICK: u32 = 3;
const H_INTERACTABLE: u32 = 4;
const H_NOT_INTERACTABLE: u32 = 5;

fn bind_focus(cx: &mut Cx<'_>, i: WidgetId) {
    bind(cx, i, |s: &mut InteractorState| &mut s.on_focus, H_FOCUS);
    bind(cx, i, |s: &mut InteractorState| &mut s.on_unfocus, H_UNFOCUS);
}

fn bind_all(cx: &mut Cx<'_>, i: WidgetId) {
    bind_focus(cx, i);
    bind(cx, i, |s: &mut InteractorState| &mut s.on_click, H_CLICK);
    bind(cx, i, |s: &mut InteractorState| &mut s.on_interactable, H_INTERACTABLE);
    bind(cx, i, |s: &mut InteractorState| &mut s.on_not_interactable, H_NOT_INTERACTABLE);
}

/// The size box's overrides from OverrideSize.
fn apply_override_size(cx: &mut Cx<'_>, box_name: &str) {
    if let Some([x, y]) = override_size(cx) {
        let b = find(cx, box_name);
        let w = wd(cx, b);
        w.w_override = x;
        w.h_override = y;
    }
}

// ---------- UI_IconButton ----------

const ICON_BACKING: Rgba = [0.038204, 0.068478, 0.165132, 1.0];

#[derive(Debug, Default)]
pub struct IconButton {
    pub on_click: Event<()>,
    pub on_click_with_source: Event<WidgetId>,
    pub on_focused: Event<()>,
}

impl IconButton {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(IconButton::default())
    }

    fn kind(cx: &Cx<'_>) -> u8 {
        let t = prop(cx, "ButtonType");
        u8::from(t.as_str() == Some("NewEnumerator1") || t.as_f64() == Some(1.0))
    }

    fn can_interact(cx: &Cx<'_>) -> bool {
        prop_bool(cx, "Interactable") && !prop_bool(cx, "bTooltipOnly")
    }

    /// UpdateTooltip @4500 (Offset (0, 50)); PreConstruct's own write has (40, 100).
    pub fn update_tooltip(cx: &mut Cx<'_>, tip: &str, x: f64, y: f64) {
        let i = interactor_of(cx);
        set_tooltip(cx, i, tooltip_params(tip, !tip.is_empty(), x, y, 12));
    }

    pub fn refresh_visual(cx: &mut Cx<'_>) {
        let can = Self::can_interact(cx);
        let disabled = rgba(&prop(cx, "DisabledColor"));
        let white = rgba(&prop(cx, "WhiteColor"));
        let backing = find(cx, "QuadBacking");
        let border = find(cx, "QuadBorder");
        let icon = find(cx, "iconImage");
        let label = find(cx, "buttonText");
        if Self::kind(cx) == 0 {
            wd(cx, backing).opacity = if can { 0.4 } else { 0.5 };
            quad_color(cx, backing, if can { ICON_BACKING } else { disabled });
            wd(cx, icon).color = white;
            wd(cx, label).color = white;
            let unfocus = rgba(&prop(cx, "UnfocusQuadColor"));
            quad_color(cx, border, if can { unfocus } else { disabled });
        } else {
            wd(cx, backing).opacity = if can { 0.75 } else { 0.5 };
            let unfocus = rgba(&prop(cx, "UnfocusQuadColor"));
            quad_color(cx, backing, if can { unfocus } else { disabled });
        }
    }
}

impl Controller for IconButton {
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let hide = prop_bool(cx, "Hide Button");
        let i = interactor_of(cx);
        let tag = prop(cx, "tag").as_str().unwrap_or_default().to_string();
        set_id(cx, i, &tag);
        let l = prop(cx, "buttonLabel");
        let label = text(Some(&l));
        let t = find(cx, "buttonText");
        let w = wd(cx, t);
        w.text = label.clone();
        w.vis = shown(!label.is_empty());
        let icon_tex = prop(cx, "icon").as_str().filter(|s| !s.is_empty()).map(str::to_string);
        let icon = find(cx, "iconImage");
        let w = wd(cx, icon);
        w.vis = shown(icon_tex.is_some());
        w.tex = icon_tex;
        let backing = find(cx, "QuadBacking");
        let border = find(cx, "QuadBorder");
        wd(cx, backing).vis = hidden(hide);
        wd(cx, border).vis = hidden(hide);
        let fc = find(cx, "UI_FocusedCommon");
        let pad = if hide { 15.0 } else { 5.0 };
        let no_wobble = prop_bool(cx, "Hide Wobble");
        let w = wd(cx, fc);
        w.pad = Margin { l: pad, t: pad, r: pad, b: pad };
        w.vis = hidden(no_wobble);
        Self::refresh_visual(cx);
        // @758, @1891: the tip is the Tooltip, or without one the label (UI_Functions Platform Button Params
        // Output gives its Default Tooltip, the buttonLabel: no instance is platform specific); it opens by the
        // quadrant
        let tip = text(Some(&prop(cx, "Tooltip")));
        let tip = if tip.is_empty() { label } else { tip };
        Self::update_tooltip(cx, &tip, 40.0, 100.0);
        apply_override_size(cx, "SizeBox_0");
        sound_overrides(cx, i);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let i = interactor_of(cx);
        let fc = find(cx, "UI_FocusedCommon");
        focus_frame(cx, fc, i);
        bind_all(cx, i);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        let backing = find(cx, "QuadBacking");
        let icon = find(cx, "iconImage");
        match id {
            H_FOCUS => {
                if prop_bool(cx, "bTooltipOnly") {
                    return;
                }
                cx.emit(self, |c| &c.on_focused, &());
                wd(cx, backing).opacity = 1.0;
                if prop_bool(cx, "SetFocusColor") {
                    let key = if prop_bool(cx, "Hide Button") { "FocusIconColor" } else { "WhiteColor" };
                    wd(cx, icon).color = rgba(&prop(cx, key));
                }
            }
            H_UNFOCUS => {
                wd(cx, backing).opacity = if Self::kind(cx) == 0 { 0.4 } else { 0.75 };
                if prop_bool(cx, "SetFocusColor") {
                    wd(cx, icon).color = rgba(&prop(cx, "WhiteColor"));
                }
            }
            H_CLICK => {
                if prop_bool(cx, "bTooltipOnly") {
                    return;
                }
                cx.emit(self, |c| &c.on_click, &());
                let me = cx.w;
                cx.emit(self, |c| &c.on_click_with_source, &me);
            }
            _ => {}
        }
    }
}

// ---------- UI_ButtonSecondary ----------

pub const WHITE: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
pub const DARK: Rgba = [0.006995, 0.008023, 0.022174, 1.0];
pub const DISABLED: Rgba = [0.262251, 0.366253, 0.396755, 1.0];
pub const HOVER_BACKING: Rgba = [0.026241, 0.046665, 0.08022, 1.0];
pub const CLEAR: Rgba = [0.0, 0.0, 0.0, 0.0];
const TERTIARY_BACKING: Rgba = [0.035294, 0.066667, 0.164706, 1.0];

/// E_Buttontype 0 Tertiary, 1 Secondary; 2 Primary has no styling of its own. The border is an outline quad (not
/// drawn). The input label (UI_InputLabel, a UI action's key) shows with Show Input Label.
#[derive(Debug)]
pub struct ButtonSecondary {
    pub on_click: Event<()>,
    pub on_click_with_source: Event<WidgetId>,
    pub on_focus: Event<()>,
    pub on_unfocus: Event<()>,
    interactable: bool,
    /// ResetStyle(ButtonColor) stores the colour first (None: the property's).
    button_color: Option<Rgba>,
    /// Init(label): the buttonLabel PreConstruct reads (None: the property's).
    label: Option<String>,
}

impl ButtonSecondary {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ButtonSecondary {
            on_click: Event::default(),
            on_click_with_source: Event::default(),
            on_focus: Event::default(),
            on_unfocus: Event::default(),
            interactable: true,
            button_color: None,
            label: None,
        })
    }

    /// The button's interactor widget of the button widget `w`.
    pub fn interactor(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
        cx.view.tree().find_in(w, "UI_Interactor")
    }

    fn kind(cx: &Cx<'_>) -> i32 {
        enum_index(cx.view.tree().props(cx.w).get("E_Buttontype"))
    }

    fn set_content_color(cx: &mut Cx<'_>, c: Rgba) {
        for n in ["iconImage", "iconImage_Mirror", "label"] {
            let w = find(cx, n);
            wd(cx, w).color = c;
        }
    }

    fn label_text(&self, cx: &Cx<'_>) -> String {
        match &self.label {
            Some(l) => l.clone(),
            None => text(cx.view.tree().props(cx.w).get("buttonLabel")),
        }
    }

    /// ResetStyle(ButtonColor): the colour is stored first.
    pub fn reset_style(&mut self, cx: &mut Cx<'_>, color: Option<Rgba>) {
        if color.is_some() {
            self.button_color = color;
        }
        let on = self.interactable;
        let disabled = rgba(&prop(cx, "DisabledColor"));
        let kind = Self::kind(cx);
        if kind > 1 {
            return;
        }
        let bc = self.button_color.unwrap_or_else(|| rgba(&prop(cx, "ButtonColor")));
        let button = if on { bc } else { disabled };
        let backing = find(cx, "QuadBacking");
        let border = find(cx, "QuadBorder");
        wd(cx, backing).opacity = if on { if kind == 0 { 0.4 } else { 0.75 } } else { 0.5 };
        let tertiary = if on { TERTIARY_BACKING } else { disabled };
        quad_color(cx, backing, if kind == 0 { tertiary } else { button });
        quad_color(cx, border, button);
        let white = rgba(&prop(cx, "WhiteColor"));
        Self::set_content_color(cx, if on { white } else { disabled });
        let numbers = prop_bool(cx, "NumbersOnlyText");
        let l = find(cx, "label");
        let size = cx.view.tree().props(cx.w).get("FontSize").and_then(Value::as_f64);
        let w = wd(cx, l);
        w.typeface = Some(if numbers { "Norms-ExtraBoldItalic" } else { "NormsCond-ExtraBoldItalic" }.to_string());
        if let Some(s) = size {
            w.font_size = s;
        }
        w.letter_spacing = if numbers { -60.0 } else { -30.0 };
        w.transform.translation = [0.0, if numbers { 1.0 } else { 0.0 }];
        let icon = prop(cx, "icon").as_str().filter(|s| !s.is_empty()).map(str::to_string);
        self.set_icon(cx, icon);
        let label = self.label_text(cx);
        self.set_label(cx, &label);
        let vis = hidden(prop_bool(cx, "HideButton"));
        wd(cx, backing).vis = vis;
        wd(cx, border).vis = vis;
        let i = interactor_of(cx);
        set_id(cx, i, &label);
        apply_override_size(cx, "SizeBox_0");
        // an instance stores only the members that differ from the class default (30, 10, 30, 10)
        let pad = margin(cx.view.tree().props(cx.w).get("LabelPadding"), Margin { l: 30.0, t: 10.0, r: 30.0, b: 10.0 });
        let hb = find(cx, "HorizontalBox_0");
        wd(cx, hb).pad = pad;
    }

    /// Init(label): the text, the interactor's id, and the buttonLabel PreConstruct reads.
    pub fn init(&mut self, cx: &mut Cx<'_>, t: &str) {
        self.label = Some(t.to_string());
        let l = find(cx, "label");
        wd(cx, l).text = t.to_string();
        let i = interactor_of(cx);
        set_id(cx, i, t);
    }

    pub fn set_label(&mut self, cx: &mut Cx<'_>, t: &str) {
        let l = find(cx, "label");
        wd(cx, l).text = t.to_string();
        let vis = shown(!t.is_empty());
        for n in ["ScaleBox_Label", "IconSpacer", "IconSpacer_Mirror"] {
            let w = find(cx, n);
            wd(cx, w).vis = vis;
        }
    }

    pub fn set_icon(&mut self, cx: &mut Cx<'_>, tex: Option<String>) {
        let mirrored = prop_bool(cx, "IsMirrored");
        let left = find(cx, "IconSize");
        let right = find(cx, "IconSize_R");
        for n in ["iconImage", "iconImage_Mirror"] {
            let w = find(cx, n);
            wd(cx, w).tex = tex.clone();
        }
        if tex.is_none() {
            wd(cx, left).vis = Vis::Collapsed;
            wd(cx, right).vis = Vis::Collapsed;
            return;
        }
        wd(cx, left).vis = shown(!mirrored);
        wd(cx, right).vis = shown(mirrored);
        let spacer = if prop_bool(cx, "IsIconOnly") { Vis::Collapsed } else { shown(!mirrored) };
        for n in ["IconSpacer", "IconSpacer_Mirror"] {
            let w = find(cx, n);
            wd(cx, w).vis = spacer;
        }
        let (sw, sh) = (prop_f64(cx, "IconSizeW", 0.0), prop_f64(cx, "IconSizeH", 0.0));
        for b in [left, right] {
            let w = wd(cx, b);
            w.w_override = sw;
            w.h_override = sh;
        }
    }
}

impl Controller for ButtonSecondary {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.interactable = cx.view.tree().props(cx.w).get("Interactable").and_then(Value::as_bool).unwrap_or(true);
        let i = interactor_of(cx);
        bind_all(cx, i);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        self.reset_style(cx, None);
        // @3075 (after the border's own PreConstruct: its SizeInitialized reads the new thickness)
        let thickness = prop(cx, "OutlineThickness");
        let border = find(cx, "QuadBorder");
        cx.view.tree_mut().set_prop(border, "OutlineThickness", thickness);
        let show_label = prop_bool(cx, "Show Input Label");
        let sb = find(cx, "ScaleBox_InputLabel");
        wd(cx, sb).vis = shown(show_label);
        // @590-1365: the label's Params are rebuilt from the button's action and player, key font size 16, no
        // custom icon; Auto Refresh = Show Input Label, and ForceRefresh only when it is shown
        let hint = find(cx, "UI_InputLabel");
        let action = enum_name(cx.view.tree().props(cx.w).get("Input Action"));
        let player = prop_f64(cx, "Player Index", 0.0) as i32;
        cx.visit::<InputLabel, _>(hint, |h, cx| {
            let params = InputLabelParams {
                action,
                player_index: player,
                key_size: 16.0,
                custom_icon: None,
                ..h.params.clone()
            };
            h.auto_refresh = show_label;
            if show_label {
                h.update_params(cx, params);
            } else {
                h.params = params;
            }
        });
        // the tag wins over the label, also when it is empty
        let i = interactor_of(cx);
        let tag = prop(cx, "tag").as_str().unwrap_or_default().to_string();
        set_id(cx, i, &tag);
        sound_overrides(cx, i);
    }

    /// Construct @4836; a Tooltip (@4922) is the interactor's tip at TooltipOffset by TooltipPlacement (the class
    /// defaults (0, 45), BelowRightAnchor).
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let i = interactor_of(cx);
        let fc = find(cx, "UI_FocusedCommon");
        focus_frame(cx, fc, i);
        let p = cx.view.tree().props(cx.w).clone();
        let tip = text(p.get("Tooltip"));
        if !tip.is_empty() {
            let off = p.get("TooltipOffset");
            let f = |k: &str| off.and_then(|o| o.get(k)).and_then(Value::as_f64).unwrap_or(0.0);
            set_tooltip(cx, i, tooltip_params(&tip, true, f("X"), f("Y"), menu_placement(p.get("TooltipPlacement"))));
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        let kind = Self::kind(cx);
        let backing = find(cx, "QuadBacking");
        match id {
            H_CLICK => {
                if prop_bool(cx, "Tool Tip Only") {
                    return;
                }
                cx.emit(self, |c| &c.on_click, &());
                let i = interactor_of(cx);
                cx.emit(self, |c| &c.on_click_with_source, &i);
            }
            // OnFocus @5308: types 0 and 1 (@5744 / @5477) the backing opaque and the icons, the input label
            // (@5596 / @5863) and the label WhiteColor; type 2 nothing more.
            H_FOCUS => {
                if prop_bool(cx, "Tool Tip Only") {
                    return;
                }
                cx.emit(self, |c| &c.on_focus, &());
                wd(cx, backing).vis = SHOWN;
                if kind > 1 {
                    return;
                }
                wd(cx, backing).opacity = 1.0;
                let white = rgba(&prop(cx, "WhiteColor"));
                Self::set_content_color(cx, white);
                let hint = find(cx, "UI_InputLabel");
                wd(cx, hint).color = white;
            }
            // OnUnfocus @6026: the label @6309, the icons @6429 / @6531 and the input label @6712 by E_Buttontype
            // (WhiteColor for 0 and 1, (0, 0, 0, 0) for 2, whether or not the button is interactable), then the
            // backing's opacity 0.4 for type 0 @6943, 0.75 for type 1 @6905 (type 2 keeps it).
            H_UNFOCUS => {
                cx.emit(self, |c| &c.on_unfocus, &());
                wd(cx, backing).vis = hidden(prop_bool(cx, "HideButton"));
                let c = if kind > 1 { CLEAR } else { rgba(&prop(cx, "WhiteColor")) };
                Self::set_content_color(cx, c);
                let hint = find(cx, "UI_InputLabel");
                wd(cx, hint).color = c;
                match kind {
                    0 => wd(cx, backing).opacity = 0.4,
                    1 => wd(cx, backing).opacity = 0.75,
                    _ => {}
                }
            }
            H_INTERACTABLE => {
                self.interactable = true;
                self.reset_style(cx, None);
            }
            H_NOT_INTERACTABLE => {
                self.interactable = false;
                self.reset_style(cx, None);
            }
            _ => {}
        }
    }
}

// ---------- UI_PlusMinusHorizontal ----------

const PM_PLUS: u32 = 1;
const PM_MINUS: u32 = 2;
const PM_FOCUS: u32 = 3;
const PM_UNFOCUS: u32 = 4;

#[derive(Debug, Default)]
pub struct PlusMinusHorizontal {
    /// (the value, its fraction).
    pub on_value_changed: Event<(f64, f64)>,
    pub value: f64,
}

impl PlusMinusHorizontal {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PlusMinusHorizontal::default())
    }

    pub fn minimum(cx: &Cx<'_>) -> f64 {
        prop_f64(cx, "Minimum", 0.0)
    }

    pub fn maximum(cx: &Cx<'_>) -> f64 {
        prop_f64(cx, "Maximum", f64::NAN)
    }

    pub fn minus(cx: &Cx<'_>) -> WidgetId {
        find(cx, "ButtonMinus")
    }

    pub fn plus(cx: &Cx<'_>) -> WidgetId {
        find(cx, "ButtonPlus")
    }

    /// The row's two interactors (minus, plus).
    pub fn interactors(cx: &Cx<'_>) -> [WidgetId; 2] {
        [ButtonSecondary::interactor(cx, Self::minus(cx)), ButtonSecondary::interactor(cx, Self::plus(cx))]
    }

    pub fn set_value(&mut self, cx: &mut Cx<'_>, v: f64) {
        let (lo, hi) = (Self::minimum(cx), Self::maximum(cx));
        // Math.min(Math.max(v, lo), hi)
        let m = if v.is_nan() || lo.is_nan() { f64::NAN } else { v.max(lo) };
        self.value = if m.is_nan() || hi.is_nan() { f64::NAN } else { m.min(hi) };
        self.update_appearance(cx);
        let frac = (self.value + lo) / (hi - lo);
        let v = self.value;
        cx.emit(self, |c| &c.on_value_changed, &(v, frac));
    }

    pub fn modify(&mut self, cx: &mut Cx<'_>, a: f64) {
        self.set_value(cx, self.value + a);
    }

    pub fn reset_to_default(&mut self, cx: &mut Cx<'_>) {
        let d = prop_f64(cx, "Default", 0.0);
        self.set_value(cx, d);
    }

    pub fn set_percentage(&mut self, cx: &mut Cx<'_>, pct: f64) {
        let (lo, hi) = (Self::minimum(cx), Self::maximum(cx));
        // Math.round
        self.set_value(cx, (pct * (hi - lo) + 0.5).floor() + lo);
    }

    fn update_appearance(&self, cx: &mut Cx<'_>) {
        let lo_text = text(Some(&prop(cx, "Min Value Custom Label")));
        let hi_text = text(Some(&prop(cx, "Max Value Custom Label")));
        let (lo, hi) = (Self::minimum(cx), Self::maximum(cx));
        let v = self.value;
        let t = if v <= lo && !lo_text.is_empty() {
            lo_text
        } else if v >= hi && !hi_text.is_empty() {
            hi_text
        } else {
            format!("{}{}", js_num(v), text(Some(&prop(cx, "Value Suffix"))))
        };
        let vt = find(cx, "ValueText");
        wd(cx, vt).text = t;
        for (b, up) in [(Self::plus(cx), v < hi), (Self::minus(cx), v > lo)] {
            let i = ButtonSecondary::interactor(cx, b);
            set_interactable(cx, i, up);
            wd(cx, b).opacity = if up { 1.0 } else { 0.75 };
        }
    }
}

impl Controller for PlusMinusHorizontal {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        // the row's own interactor keeps the null context: only the buttons take the focus
        for (b, click) in [(Self::plus(cx), PM_PLUS), (Self::minus(cx), PM_MINUS)] {
            cx.visit::<ButtonSecondary, _>(b, |b, _| {
                b.on_click.add(Handler { owner: me, id: click });
                b.on_focus.add(Handler { owner: me, id: PM_FOCUS });
                b.on_unfocus.add(Handler { owner: me, id: PM_UNFOCUS });
            });
        }
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let label = text(Some(&prop(cx, "Label")));
        let l = find(cx, "LabelText");
        wd(cx, l).text = label;
        for (n, k) in [("DivTop", "HideTopDiv"), ("DivBottom", "HideBottomDiv")] {
            let hide = prop_bool(cx, k);
            let w = find(cx, n);
            wd(cx, w).vis = hidden(hide);
        }
        // @442-1215: the row's Tooltip is the tip of both buttons
        let tip = text(Some(&prop(cx, "Tooltip")));
        for i in Self::interactors(cx) {
            set_tooltip(cx, i, tooltip_params(&tip, !tip.is_empty(), 0.0, -25.0, 12));
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        let inc = prop_f64(cx, "Increment", 0.0);
        match id {
            PM_PLUS => self.modify(cx, inc),
            PM_MINUS => self.modify(cx, -inc),
            PM_FOCUS | PM_UNFOCUS => {
                let f = id == PM_FOCUS;
                let hb = find(cx, "HoverBacking");
                wd(cx, hb).color = if f { HOVER_BACKING } else { CLEAR };
                let l = find(cx, "LabelText");
                let w = wd(cx, l);
                w.opacity = if f { 1.0 } else { 0.65 };
                w.font_size = if f { 20.0 } else { 18.0 };
                let v = find(cx, "ValueText");
                wd(cx, v).font_size = if f { 32.0 } else { 28.0 };
            }
            _ => {}
        }
    }
}

// ---------- UI_ToggleSwitchRework ----------

pub const CHECK_BOX: Rgba = [0.194618, 0.651406, 0.630757, 1.0];
pub const QUAD_FILL: Rgba = [0.038204, 0.068478, 0.165132, 1.0];

/// DivTop / DivBottom, the label switcher and the height are set at design time only: in game they keep the
/// designer's values, and the checkmark its designer visibility until the owner calls SetOn. hoverButton (a Button
/// with no look that fills the row under the label) shows the focus visuals while the mouse is on it. A Tooltip is
/// the interactor's tip.
#[derive(Debug)]
pub struct ToggleSwitch {
    /// ToggleChangedEvent, with the switch's new state. The build's delegate has no parameter (@1074: its listeners
    /// read isOn); here the switch is out of the view while it fires, so it hands its state to them.
    pub on_toggle_changed: Event<bool>,
    pub is_on: bool,
}

impl ToggleSwitch {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ToggleSwitch { on_toggle_changed: Event::default(), is_on: true })
    }

    pub fn set_on(&mut self, cx: &mut Cx<'_>, v: bool) {
        self.is_on = v;
        let c = find(cx, "Checkmark");
        wd(cx, c).vis = if v { Vis::HitTestInvisible } else { Vis::Hidden };
    }

    fn set_colors(cx: &mut Cx<'_>, b: Rgba, f: Rgba, c: Rgba) {
        let bx = find(cx, "CheckmarkBox");
        let fl = find(cx, "CheckmarkFill");
        let ck = find(cx, "Checkmark");
        quad_color(cx, bx, b);
        quad_color(cx, fl, f);
        wd(cx, ck).color = c;
    }

    fn visuals(cx: &mut Cx<'_>, focused: bool) {
        let fl = find(cx, "CheckmarkFill");
        wd(cx, fl).opacity = if focused { 1.0 } else { 0.5 };
        for n in ["Label", "ShortWrapLabel"] {
            let l = find(cx, n);
            let w = wd(cx, l);
            w.opacity = if focused { 1.0 } else { 0.65 };
            w.font_size = if focused { 20.0 } else { 18.0 };
        }
        if !prop_bool(cx, "HIdeHoverBacking") {
            let hb = find(cx, "HoverBacking");
            wd(cx, hb).color = if focused { HOVER_BACKING } else { CLEAR };
        }
    }
}

impl Controller for ToggleSwitch {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.is_on = cx.view.tree().props(cx.w).get("isOn").and_then(Value::as_bool).unwrap_or(true);
        let i = interactor_of(cx);
        bind_all(cx, i);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let hide = prop_bool(cx, "HIdeHoverBacking");
        let hb = find(cx, "HoverBacking");
        wd(cx, hb).vis = hidden(hide);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let i = interactor_of(cx);
        let fc = find(cx, "UI_FocusedCommon");
        focus_frame(cx, fc, i);
        let t = text(Some(&prop(cx, "LabelText")));
        for n in ["Label", "ShortWrapLabel"] {
            let l = find(cx, n);
            wd(cx, l).text = t.clone();
        }
        // @1233-1566
        let tip = text(Some(&prop(cx, "Tooltip")));
        if !tip.is_empty() {
            set_tooltip(cx, i, tooltip_params(&tip, true, 400.0, -25.0, 12));
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            H_FOCUS => Self::visuals(cx, true),
            H_UNFOCUS => Self::visuals(cx, false),
            H_CLICK => {
                let v = !self.is_on;
                self.set_on(cx, v);
                cx.emit(self, |c| &c.on_toggle_changed, &v);
            }
            H_INTERACTABLE => Self::set_colors(cx, CHECK_BOX, QUAD_FILL, WHITE),
            H_NOT_INTERACTABLE => Self::set_colors(cx, DISABLED, DISABLED, DISABLED),
            _ => {}
        }
    }

    /// hoverButton OnHovered @1682 / OnUnhovered @1687: the paths of OnFocus @468 / OnUnfocus @771.
    fn on_button_hover(&mut self, cx: &mut Cx<'_>, name: &str, hovered: bool) {
        if name == "hoverButton" {
            Self::visuals(cx, hovered);
        }
    }
}

// ---------- UI_ButtonMain ----------

fn slate_color(c: &Value) -> Rgba {
    rgba(c.get("SpecifiedColor").unwrap_or(c))
}

/// The backing quad is a colour override instance: its Image stays white and the colour is its material's ColorA /
/// ColorB (WBP_Quadrilateral @694, @1695). The border is an outline quad (not drawn).
/// EnableShimmer (ubergraph @3582): ShimmerStrength 0.8 or 0 on the backing quad's dynamic material (MI_QuadShimmer:
/// the moving shine of its shader). PreConstruct calls it with DefaultShimmer (@2607 and, last, @2121); UpdateColors
/// turns it off for a button that is not Interactable (@3354) and nothing turns it on again.
#[derive(Debug)]
pub struct ButtonMain {
    pub on_click: Event<()>,
    pub on_click_with_source: Event<WidgetId>,
    interactable: bool,
    /// Init(label) / SetLabel(label): the buttonLabel PreConstruct reads.
    label: Option<String>,
}

impl ButtonMain {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ButtonMain {
            on_click: Event::default(),
            on_click_with_source: Event::default(),
            interactable: true,
            label: None,
        })
    }

    /// WBP_Quadrilateral GetMaterial = DynamicMat, the Image's material (the paint copies `quad` to it at its first
    /// paint). MI_QuadShimmer's own ShimmerStrength is 0: a button that never had the shimmer keeps its parameters
    /// as they are.
    pub fn enable_shimmer(cx: &mut Cx<'_>, on: bool) {
        let q = find(cx, "WBP_Quadrilateral");
        let w = wd(cx, q);
        let Some(quad) = &mut w.quad else { return };
        let has = quad.iter().any(|e| e.0 == "ShimmerStrength");
        if !on && !has {
            return;
        }
        let x = if on { 0.8 } else { 0.0 };
        match quad.iter_mut().find(|e| e.0 == "ShimmerStrength") {
            Some(e) => e.1 = x,
            None => quad.push(("ShimmerStrength".into(), x)),
        }
        if let Some(m) = &mut w.mid {
            m.set_scalar("ShimmerStrength", x);
        }
    }

    /// WBP_Quadrilateral.UpdateColor (@2833, @3094, @3380, @3486): an IsColorOverride quad, so the material's
    /// ColorA / ColorB.
    fn set_colors(cx: &mut Cx<'_>, bg: Rgba, font: Rgba) {
        let q = find(cx, "WBP_Quadrilateral");
        quad_color(cx, q, bg);
        let l = find(cx, "label");
        wd(cx, l).color = font;
    }

    fn update_colors(&self, cx: &mut Cx<'_>) {
        let on = self.interactable;
        let p = cx.view.tree().props(cx.w).clone();
        let pc = |k: &str| p.get(k).map(slate_color).unwrap_or_default();
        Self::set_colors(cx, if on { pc("BGColor") } else { DISABLED }, if on { pc("FontColor") } else { DISABLED });
        let q = find(cx, "WBP_Quadrilateral");
        wd(cx, q).opacity = if on { 1.0 } else { 0.5 };
        let b = find(cx, "WBP_QuadBorder");
        wd(cx, b).vis = if on { Vis::Collapsed } else { SHOWN };
        if !on {
            Self::enable_shimmer(cx, false);
        }
    }

    /// Init(label) and SetLabel(label) together: the text, the interactor's id, and the buttonLabel PreConstruct
    /// reads.
    pub fn set_label(&mut self, cx: &mut Cx<'_>, t: &str) {
        self.label = Some(t.to_string());
        let l = find(cx, "label");
        wd(cx, l).text = t.to_string();
        let i = interactor_of(cx);
        set_id(cx, i, t);
    }
}

impl Controller for ButtonMain {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.interactable = cx.view.tree().props(cx.w).get("Interactable").and_then(Value::as_bool).unwrap_or(true);
        let i = interactor_of(cx);
        bind_all(cx, i);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let i = interactor_of(cx);
        let tag = prop(cx, "tag").as_str().unwrap_or_default().to_string();
        set_id(cx, i, &tag);
        sound_overrides(cx, i);
        let show = prop_bool(cx, "ShowInputLabel");
        let hint = find(cx, "UI_InputLabel");
        wd(cx, hint).vis = shown(show);
        // The label's Params are rebuilt (ubergraph @1392): the button's action and player, key font size 16, icon
        // size 64 / 48 / 32 (ShowLargeInputLabel / ShowMediumInputLabel / neither); then ForceRefresh.
        let size = if prop_bool(cx, "ShowLargeInputLabel") {
            64.0
        } else if prop_bool(cx, "ShowMediumInputLabel") {
            48.0
        } else {
            32.0
        };
        let action = enum_name(cx.view.tree().props(cx.w).get("InputAction"));
        let player = prop_f64(cx, "Player Index", 0.0) as i32;
        cx.visit::<InputLabel, _>(hint, |h, cx| {
            let params = InputLabelParams {
                action,
                player_index: player,
                key_size: 16.0,
                icon_size: [size, size],
                custom_icon: None,
                ..h.params.clone()
            };
            h.update_params(cx, params);
        });
        let q = find(cx, "WBP_Quadrilateral");
        let hide_bg = prop_bool(cx, "HideBackground");
        wd(cx, q).opacity = if hide_bg { 0.0 } else { 1.0 };
        let fc = find(cx, "UI_FocusedCommon");
        let no_wobble = prop_bool(cx, "HideFocusWobble");
        wd(cx, fc).vis = if no_wobble { Vis::Collapsed } else { SHOWN };
        self.update_colors(cx);
        let l = find(cx, "label");
        if let Some(s) = cx.view.tree().props(cx.w).get("FontSize").and_then(Value::as_f64) {
            wd(cx, l).font_size = s;
        }
        let label = match &self.label {
            Some(l) => l.clone(),
            None => text(Some(&prop(cx, "buttonLabel"))),
        };
        if !label.is_empty() {
            self.set_label(cx, &label);
        }
        apply_override_size(cx, "SizeBox_0");
        // an instance stores only the members that differ from the class default (28, 10, 30, 10)
        let pad = margin(cx.view.tree().props(cx.w).get("LabelPadding"), Margin { l: 28.0, t: 10.0, r: 30.0, b: 10.0 });
        let h = find(cx, "ButtonTextHolder");
        wd(cx, h).pad = pad;
        // @2121, after UpdateColors: the instance's DefaultShimmer whatever Interactable is
        let shimmer = cx.view.tree().props(cx.w).get("DefaultShimmer").and_then(Value::as_bool) == Some(true);
        Self::enable_shimmer(cx, shimmer);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let i = interactor_of(cx);
        let fc = find(cx, "UI_FocusedCommon");
        focus_frame(cx, fc, i);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        let p = cx.view.tree().props(cx.w).clone();
        let pc = |k: &str| p.get(k).map(slate_color).unwrap_or_default();
        match id {
            H_CLICK => {
                cx.emit(self, |c| &c.on_click, &());
                let i = interactor_of(cx);
                cx.emit(self, |c| &c.on_click_with_source, &i);
            }
            H_FOCUS if self.interactable => Self::set_colors(cx, pc("FocusBGColor"), pc("FocusFontColor")),
            H_UNFOCUS if self.interactable => Self::set_colors(cx, pc("BGColor"), pc("FontColor")),
            H_INTERACTABLE => {
                self.interactable = true;
                self.update_colors(cx);
            }
            H_NOT_INTERACTABLE => {
                self.interactable = false;
                self.update_colors(cx);
            }
            _ => {}
        }
    }
}

// ---------- UI_ButtonSecondary_Vertical ----------

const VERTICAL_DISABLED_BG: Rgba = [0.048172, 0.061246, 0.088656, 1.0];
const VERTICAL_DISABLED_CONTENT: Rgba = [0.003677, 0.007499, 0.019382, 0.498039];

#[derive(Debug)]
pub struct ButtonSecondaryVertical {
    pub on_click: Event<()>,
    pub on_focus: Event<()>,
    pub on_unfocus: Event<()>,
    interactable: bool,
}

impl ButtonSecondaryVertical {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ButtonSecondaryVertical {
            on_click: Event::default(),
            on_focus: Event::default(),
            on_unfocus: Event::default(),
            interactable: true,
        })
    }

    fn set_colors(cx: &mut Cx<'_>, bg: Rgba, font: Rgba, icon: Rgba) {
        for (n, c) in [("bg", bg), ("label", font), ("Icon", icon)] {
            let w = find(cx, n);
            wd(cx, w).color = c;
        }
    }

    fn update_colors(&self, cx: &mut Cx<'_>) {
        let p = cx.view.tree().props(cx.w).clone();
        let pc = |k: &str| p.get(k).map(slate_color).unwrap_or_default();
        if self.interactable {
            Self::set_colors(cx, pc("BGColor"), pc("FontColor"), pc("IconColor"));
        } else {
            Self::set_colors(cx, VERTICAL_DISABLED_BG, VERTICAL_DISABLED_CONTENT, VERTICAL_DISABLED_CONTENT);
        }
    }
}

impl Controller for ButtonSecondaryVertical {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.interactable = cx.view.tree().props(cx.w).get("Interactable").and_then(Value::as_bool).unwrap_or(true);
        let i = interactor_of(cx);
        bind_all(cx, i);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let i = interactor_of(cx);
        sound_overrides(cx, i);
        let bg = find(cx, "bg");
        let hide_bg = prop_bool(cx, "HideBackground");
        wd(cx, bg).opacity = if hide_bg { 0.0 } else { 1.0 };
        let fc = find(cx, "UI_FocusedCommon");
        let no_wobble = prop_bool(cx, "HideFocusWobble");
        wd(cx, fc).vis = if no_wobble { Vis::Collapsed } else { SHOWN };
        if let Some(t) = prop(cx, "IconTexture").as_str().filter(|s| !s.is_empty()) {
            let ic = find(cx, "Icon");
            wd(cx, ic).tex = Some(t.to_string());
        }
        let l = find(cx, "label");
        if let Some(s) = cx.view.tree().props(cx.w).get("FontSize").and_then(Value::as_f64) {
            wd(cx, l).font_size = s;
        }
        self.update_colors(cx);
        apply_override_size(cx, "SizeBox_0");
        let pad = margin(cx.view.tree().props(cx.w).get("LabelPadding"), Margin { l: 30.0, t: 10.0, r: 30.0, b: 10.0 });
        let h = find(cx, "ButtonTextHolder");
        wd(cx, h).pad = pad;
        // Init(buttonLabel)
        let label = text(Some(&prop(cx, "buttonLabel")));
        wd(cx, l).text = label.clone();
        set_id(cx, i, &label);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let i = interactor_of(cx);
        let fc = find(cx, "UI_FocusedCommon");
        focus_frame(cx, fc, i);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            H_CLICK => cx.emit(self, |c| &c.on_click, &()),
            H_FOCUS => {
                let p = cx.view.tree().props(cx.w).clone();
                let pc = |k: &str| p.get(k).map(slate_color).unwrap_or_default();
                Self::set_colors(cx, pc("FocusBGColor"), pc("FocusFontColor"), pc("FocusIconColor"));
                cx.emit(self, |c| &c.on_focus, &());
            }
            H_UNFOCUS => {
                self.update_colors(cx);
                cx.emit(self, |c| &c.on_unfocus, &());
            }
            H_INTERACTABLE => {
                self.interactable = true;
                self.update_colors(cx);
            }
            H_NOT_INTERACTABLE => {
                self.interactable = false;
                self.update_colors(cx);
            }
            _ => {}
        }
    }
}

// ---------- UI_ButtonCartoon ----------

const BC_TIMER: u32 = 10;

/// The class defaults leave the textures unset (IdleTexture / HoverTexture stay collapsed) and give the focused and
/// unfocused quad one colour, so the focus shows as the offsets and the label colour only. The corner and colour
/// properties written to IdleQuad in Init equal that quad's own.
#[derive(Debug, Default)]
pub struct ButtonCartoon {
    pub on_click: Event<()>,
}

impl ButtonCartoon {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ButtonCartoon::default())
    }

    fn offset(cx: &mut Cx<'_>, name: &str, d: f64) {
        let w = find(cx, name);
        wd(cx, w).transform.translation = [d, d];
    }
}

impl Controller for ButtonCartoon {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = interactor_of(cx);
        cx.view.manager.on_timer_event.add(Handler { owner: cx.w, id: BC_TIMER });
        bind_all(cx, i);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let shadow = find(cx, "ShadowQuad");
        let show = prop_bool(cx, "ShowShadow");
        wd(cx, shadow).vis = shown(show);
        // the shadow instance's ColorAndOpacity tints its quad (the paint keeps the quad's Color only)
        let tint = cx.view.tree().props(shadow).get("ColorAndOpacity").cloned();
        if let Some(t) = tint {
            wd(cx, shadow).color = rgba(&t);
        }
        let label = text(Some(&prop(cx, "buttonLabel")));
        let l = find(cx, "label");
        wd(cx, l).text = label.clone();
        if !label.is_empty() {
            let i = interactor_of(cx);
            set_id(cx, i, &label);
        }
        for n in ["IdleTexture", "HoverTexture"] {
            let w = find(cx, n);
            wd(cx, w).vis = Vis::Collapsed;
        }
        let fs = prop_f64(cx, "FontSize", 0.0);
        if fs != 0.0 {
            wd(cx, l).font_size = fs;
        }
        let sharp = prop_f64(cx, "Sharpness", 0.0);
        for q in ["ClickQuad", "IdleQuad", "SelectedQuad", "ShadowQuad"] {
            let w = find(cx, q);
            cx.visit::<crate::menu::widgets::common::Quadrilateral, _>(w, |q, cx| q.update_sharpness(cx, sharp));
        }
        let so = prop(cx, "SizeOverride");
        let g = |k: &str| so.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let sb = find(cx, "SizeBox");
        let b = wd(cx, sb);
        b.w_override = if g("X").abs() <= 1e-6 { -1.0 } else { g("X") };
        b.h_override = if g("Y").abs() <= 1e-6 { -1.0 } else { g("Y") };
        let left = prop_bool(cx, "LeftAlignedText");
        wd(cx, l).justify = if left { crate::text::Justify::Left } else { crate::text::Justify::Center };
        let sc = find(cx, "ScaleBox_0");
        wd(cx, sc).pad = Margin { l: if left { 25.0 } else { 10.0 }, t: 0.0, r: if left { 0.0 } else { 10.0 }, b: 0.0 };
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let i = interactor_of(cx);
        let fc = find(cx, "UI_FocusedCommon");
        focus_frame(cx, fc, i);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        let l = find(cx, "label");
        let idle = find(cx, "IdleQuad");
        match id {
            BC_TIMER => {
                let timer = prop(cx, "ClickQuadTimer");
                if arg.downcast_ref::<String>().map(String::as_str) == timer.as_str() && timer.as_str().is_some() {
                    let c = find(cx, "ClickQuad");
                    wd(cx, c).vis = Vis::Collapsed;
                }
            }
            H_UNFOCUS => {
                Self::offset(cx, "ShadowQuad", 8.0);
                Self::offset(cx, "SizeBox", 0.0);
                wd(cx, l).color = rgba(&prop(cx, "UnFocusedLabelColor"));
                wd(cx, idle).vis = SHOWN;
            }
            H_FOCUS => {
                Self::offset(cx, "SizeBox", 2.0);
                wd(cx, l).color = rgba(&prop(cx, "FocusedLabelColor"));
                Self::offset(cx, "ShadowQuad", 6.0);
                wd(cx, idle).vis = SHOWN;
            }
            H_CLICK => {
                cx.emit(self, |c| &c.on_click, &());
                let c = find(cx, "ClickQuad");
                wd(cx, c).vis = SHOWN;
                let t = prop(cx, "ClickQuadTimer").as_str().unwrap_or_default().to_string();
                Manager::add_timer(cx, &t, 0.1);
            }
            _ => {}
        }
    }
}

// (the wobble helper the callers of a button's focus frame share)
#[allow(dead_code)]
fn _wobble(cx: &mut Cx<'_>, q: WidgetId) {
    quad_wobble(cx, q, false);
}

/// A number as JavaScript's String(n) writes it for the values a slider shows (whole numbers, short fractions).
fn js_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e21 { format!("{}", n as i64) } else { format!("{n}") }
}
