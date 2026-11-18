//! The shared controls.

use serde_json::Value;

use super::common::Quadrilateral;
use crate::bundle::Rgba;
use crate::menu::controller::{Cx, Event, Handler};
use crate::menu::widgets::controls::interactor::{InteractorState, TooltipParams};
use crate::slate::{Widget, WidgetId};

pub mod background_blur;
pub mod bar_buttons;
pub mod buttons;
pub mod color_picker;
pub mod drop_menu;
pub mod grid_item;
pub mod input_binding;
pub mod input_label;
pub mod interactor;
pub mod parallax_items;
pub mod pips;
pub mod tooltip;

// What a controller reads of its own tree (`find`, `child`, `props`).

/// `find(name)` under the controller's widget.
pub fn find(cx: &Cx<'_>, name: &str) -> WidgetId {
    cx.view.tree().find_in(cx.w, name)
}

/// CanvasPanel slot ZOrders, which the bundle does not carry into the paint order: each named widget of this one
/// painted and hit at `z`, over its siblings at 0 (among equals the child order still holds).
pub fn slot_z_order(cx: &mut Cx<'_>, names: &[&str], z: i32) {
    for n in names {
        let w = find(cx, n);
        wd(cx, w).paint_order = z;
    }
}

/// The widget `w` of the tree.
pub fn wd<'a>(cx: &'a mut Cx<'_>, w: WidgetId) -> &'a mut Widget {
    cx.view.tree_mut().widget_mut(w)
}

/// The controller's own property `k` (Null when it has none).
pub fn prop(cx: &Cx<'_>, k: &str) -> Value {
    cx.view.tree().props(cx.w).get(k).cloned().unwrap_or(Value::Null)
}

pub fn prop_bool(cx: &Cx<'_>, k: &str) -> bool {
    cx.view.tree().props(cx.w).get(k).and_then(Value::as_bool).unwrap_or(false)
}

pub fn prop_f64(cx: &Cx<'_>, k: &str, d: f64) -> f64 {
    cx.view.tree().props(cx.w).get(k).and_then(Value::as_f64).unwrap_or(d)
}

/// A string property, '' when it has none (an enum's name is cut at its `::`).
pub fn prop_str(cx: &Cx<'_>, k: &str) -> String {
    cx.view.tree().props(cx.w).get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// A Vector2D property that is set and not (0, 0) within 1e-4 (OverrideSize): its X and Y.
pub fn override_size(cx: &Cx<'_>) -> Option<[f64; 2]> {
    let o = cx.view.tree().props(cx.w).get("OverrideSize")?;
    let x = o.get("X").and_then(Value::as_f64).unwrap_or(0.0);
    let y = o.get("Y").and_then(Value::as_f64).unwrap_or(0.0);
    (!(x.abs() <= 1e-4 && y.abs() <= 1e-4)).then_some([x, y])
}

// What a controller does to its interactor child (the state is the manager's: interactor.rs).

/// The interactor `iw`'s state.
pub fn ist<'a>(cx: &'a mut Cx<'_>, iw: WidgetId) -> Option<&'a mut InteractorState> {
    cx.view.manager.interactor_mut(iw)
}

/// `interactor.id = id`.
pub fn set_id(cx: &mut Cx<'_>, iw: WidgetId, id: &str) {
    if let Some(s) = ist(cx, iw) {
        s.id = id.to_string();
    }
}

/// `interactor.tooltipParams = p`.
pub fn set_tooltip(cx: &mut Cx<'_>, iw: WidgetId, p: TooltipParams) {
    if let Some(s) = ist(cx, iw) {
        s.tooltip_params = p;
    }
}

/// The Hover / Confirm Sound Override properties of the controller's widget given to its interactor.
pub fn sound_overrides(cx: &mut Cx<'_>, iw: WidgetId) {
    let hover = super::common::sound(cx.view.tree().props(cx.w).get("Hover Sound Override"));
    let confirm = super::common::sound(cx.view.tree().props(cx.w).get("Confirm Sound Override"));
    if let Some(s) = ist(cx, iw) {
        if hover.is_some() {
            s.hover_sound = hover;
        }
        if confirm.is_some() {
            s.confirm_sound = confirm;
        }
    }
}

/// A handler of the controller (`id`) added to one of the interactor's events (`interactor.onFocus.add(...)`).
pub fn bind<T>(cx: &mut Cx<'_>, iw: WidgetId, ev: fn(&mut InteractorState) -> &mut Event<T>, id: u32)
where
    T: std::any::Any + Clone,
{
    let h = Handler { owner: cx.w, id };
    if let Some(s) = ist(cx, iw) {
        ev(s).add(h);
    }
}

/// The colour of a WBP_Quadrilateral child (UpdateColor).
pub fn quad_color(cx: &mut Cx<'_>, q: WidgetId, c: Rgba) {
    cx.visit::<Quadrilateral, _>(q, |q, cx| q.update_color(cx, c));
}

/// WBP_Quadrilateral Wobble / UpdateSharpness of a child.
pub fn quad_wobble(cx: &mut Cx<'_>, q: WidgetId, on: bool) {
    cx.visit::<Quadrilateral, _>(q, |q, cx| q.wobble(cx, on));
}

/// FocusedCommon.setInteractor(i) of the child `fc`.
pub fn focus_frame(cx: &mut Cx<'_>, fc: WidgetId, i: WidgetId) {
    cx.visit::<interactor::FocusedCommon, _>(fc, |f, cx| f.set_interactor(cx, i));
}
