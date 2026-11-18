//! UI_Tooltip, UI_TooltipTarget and UI_CalloutTooltip.

use std::any::Any;
use std::rc::Rc;

use serde_json::Value;

use super::interactor::{TooltipParams, no_tooltip, set_interactable};
use super::{find, focus_frame, prop, prop_bool, wd};
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::view::MenuView;
use crate::slate::{Vis, WidgetId};

// The tip protocol: a tip class's controller answers these through `Controller::handle`
// (UI_Tooltip here, the perk tooltip of the perk page and the glossary tips in their own files).
/// `set_text`: the argument is a `String`.
pub const TIP_SET_TEXT: u32 = 0xFFFF_0010;
/// `show`: no argument (an empty tuple).
pub const TIP_SHOW: u32 = 0xFFFF_0011;
/// `set_data`: the argument is the params' `Rc<dyn Any>`.
pub const TIP_SET_DATA: u32 = 0xFFFF_0012;

/// EMenuPlacement of a property: the names in the engine's order (UE 4.26 SlateEnums.h; the select of
/// UI_TooltipTarget @2517 has the 13 cases).
const MENU_PLACEMENTS: [&str; 13] = [
    "BelowAnchor",
    "CenteredBelowAnchor",
    "BelowRightAnchor",
    "ComboBox",
    "ComboBoxRight",
    "MenuRight",
    "AboveAnchor",
    "CenteredAboveAnchor",
    "AboveRightAnchor",
    "MenuLeft",
    "Center",
    "RightLeftCenter",
    "MatchBottomLeft",
];

/// EMenuPlacement of a property: a number, or an enumerator's name (an unknown name: 0).
pub fn menu_placement(v: Option<&Value>) -> u8 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) as u8,
        Some(Value::String(s)) => {
            let name = s.rsplit("MenuPlacement_").next().unwrap_or_default();
            MENU_PLACEMENTS.iter().position(|p| *p == name).unwrap_or(0) as u8
        }
        _ => 0,
    }
}

/// A struct member's name without the editor's generated suffix (`_<n>_<32 hex digits>`).
fn member_stem(k: &str) -> &str {
    let b = k.as_bytes();
    if b.len() > 34 && b[b.len() - 33] == b'_' && b[b.len() - 32..].iter().all(u8::is_ascii_hexdigit) {
        let rest = &k[..k.len() - 33];
        if let Some(i) = rest.rfind('_')
            && rest[i + 1..].bytes().all(|c| c.is_ascii_digit())
        {
            return &rest[..i];
        }
    }
    k
}

/// An instance's own Tooltip Params (UI_TooltipParams in the tree): the struct's members by their name before the
/// generated suffix.
pub fn tooltip_params_of(v: &Value) -> Option<TooltipParams> {
    let Value::Object(o) = v else { return None };
    let m = |stem: &str| o.iter().find(|(k, _)| member_stem(k) == stem).map(|(_, v)| v);
    let tip = m("Tooltip")
        .and_then(|t| {
            t.get("LocalizedString").or_else(|| t.get("SourceString")).or_else(|| t.get("CultureInvariantString"))
        })
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let cls = m("TooltipClass")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty() && *c != "None")
        .map(|c| c.strip_suffix("_C").unwrap_or(c).to_string());
    let off = m("Offset");
    let f = |v: Option<&Value>, k: &str| v.and_then(|v| v.get(k)).and_then(Value::as_f64).unwrap_or(0.0);
    let default_placement = Value::String("MenuPlacement_CenteredBelowAnchor".into());
    Some(TooltipParams {
        tooltip: tip,
        cls,
        show: m("ShowTooltip").and_then(Value::as_bool).unwrap_or(false),
        offset: [f(off, "X"), f(off, "Y")],
        delay: m("Delay").and_then(Value::as_f64).unwrap_or(0.0),
        placement: menu_placement(Some(m("Placement").unwrap_or(&default_placement))),
        data: None,
    })
}

/// `tooltipParams(tooltip, show, x, y, placement)`: the class UI_Tooltip, no delay.
pub fn tooltip_params(tooltip: &str, show: bool, x: f64, y: f64, placement: u8) -> TooltipParams {
    TooltipParams {
        tooltip: tooltip.to_string(),
        cls: Some("UI_Tooltip".into()),
        show,
        offset: [x, y],
        delay: 0.0,
        placement,
        data: None,
    }
}

// ---------- UI_Tooltip ----------

/// The tip: a quad behind a text that wraps at 400. Show @91 makes it HitTestInvisible and plays ShowAnim (the
/// opacity 0 to 1 in 0.15 s); Hide / Destruct @159 hide it and stop the animation. FollowMouse (@224) is off in
/// every params struct of the bundle's widgets: not ported.
#[derive(Debug, Default)]
pub struct Tooltip;

impl Tooltip {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(Tooltip)
    }

    fn set_text(cx: &mut Cx<'_>, t: &str) {
        let l = find(cx, "TipLabel");
        wd(cx, l).text = t.to_string();
    }

    fn show(cx: &mut Cx<'_>) {
        let me = cx.w;
        wd(cx, me).vis = Vis::HitTestInvisible;
        cx.view.play(me, "ShowAnim", false, 1.0);
    }

    /// Hide @159.
    pub fn hide(cx: &mut Cx<'_>) {
        let me = cx.w;
        wd(cx, me).vis = Vis::Hidden;
        cx.view.stop_all_animations(me);
    }
}

impl Controller for Tooltip {
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        Tooltip::hide(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            TIP_SET_TEXT => {
                if let Some(t) = arg.downcast_ref::<String>() {
                    Tooltip::set_text(cx, t);
                }
            }
            TIP_SHOW => Tooltip::show(cx),
            _ => {}
        }
    }
}

/// UI_Functions CloseAllTooltips: Hide on every tip that is up. The tip stays where it is, unseen: its
/// UI_TooltipTarget still counts it as open and removes it at its own CloseTooltip (the next unfocus, unhover or
/// click), so the same target shows no tip again before that. Of the interface's classes the port has UI_Tooltip.
pub fn close_all_tooltips(cx: &mut Cx<'_>) {
    for w in crate::menu::widgets::nav::live(cx, "UI_Tooltip") {
        cx.visit::<Tooltip, _>(w, |_, cx| Tooltip::hide(cx));
    }
}

// ---------- UI_TooltipTarget ----------

/// The opener of a tip: a hitbox Button (no look) around a MenuAnchor (TooltipAnchor) that marks the place. The one
/// in every UI_Interactor is Manually Triggered and sits at the centre of the interactor's hitbox; the interactor
/// opens it. One that is not opens on its own hitbox's hover. Use Menu Anchor? is false on every instance: the tip
/// goes on the viewport (ZOrder 5, over the modals at 3). Auto Close When Modals closes it at the manager's
/// ModalShownEvent (@3770). The close at Collapsed / Hidden (@3820) is bound to the target's own
/// OnVisibilityChanged, which only its own SetVisibility fires; Auto Close When Levels closes a tip that outlives a
/// level, and a menu view of the port ends with its screen, taking its targets through Destruct (@3530).
#[derive(Debug)]
pub struct TooltipTarget {
    pub params: TooltipParams,
    params_written: bool,
    pub player_index: i32,
    pub z_order: i32,
    should_show: bool,
    is_open: bool,
    tip: Option<WidgetId>,
    modal_bound: bool,
}

const H_MODAL_SHOWN: u32 = 1;
const R_AFTER_DELAY: u32 = 1;

impl TooltipTarget {
    pub fn make(cx: &mut Cx<'_>) -> Box<dyn Controller> {
        let z = cx.view.tree().props(cx.w).get("ZOrder").and_then(Value::as_i64).unwrap_or(5) as i32;
        Box::new(TooltipTarget {
            params: no_tooltip(),
            params_written: false,
            player_index: 0,
            z_order: z,
            should_show: false,
            is_open: false,
            tip: None,
            modal_bound: false,
        })
    }

    /// Init @830.
    pub fn init(&mut self, params: TooltipParams, player: i32) {
        self.params = params;
        self.params_written = true;
        self.player_index = player;
    }

    /// Toggle Tooltip @885: a show is dropped while Tooltips Disabled or while the manager is in a transition; with
    /// a Delay the tip opens after it when it is still wanted (@422).
    pub fn toggle_tooltip(&mut self, cx: &mut Cx<'_>, show: bool) {
        if show && (prop_bool(cx, "Tooltips Disabled") || cx.view.manager.is_transitioning()) {
            return;
        }
        self.should_show = show;
        if !show {
            return self.close_tooltip(cx);
        }
        if self.params.delay > 0.0 {
            cx.view.delay(self.params.delay, Handler { owner: cx.w, id: R_AFTER_DELAY });
        } else {
            self.after_delay(cx);
        }
    }

    fn after_delay(&mut self, cx: &mut Cx<'_>) {
        if self.should_show {
            self.open_tooltip(cx);
        } else {
            self.close_tooltip(cx);
        }
    }

    /// OpenTooltip @2848 and @1419: UI_Functions Get Tooltip makes the widget (Set Text, Show); it goes on the
    /// viewport at PaddedViewportPosition(50) and is moved by its render translation: the params' Offset plus a
    /// part of its desired size by the Placement. Unverified: SetPositionInViewport(position, false) is read from
    /// the UE 4.26 source (slot offset in Slate units, anchors 0, the desired size), not in this build's exe.
    fn open_tooltip(&mut self, cx: &mut Cx<'_>) {
        let p = self.params.clone();
        let Some(cls) = p.cls.as_deref().filter(|_| p.show && !self.is_open) else { return };
        let tip = MenuView::create(cx, cls);
        cx.visit_dyn(tip, |c, cx| {
            c.handle(cx, TIP_SET_TEXT, &p.tooltip);
            c.handle(cx, TIP_SHOW, &());
            // UI_Functions Get Tooltip: Set Data after Show
            if let Some(d) = &p.data {
                let d: Rc<dyn Any> = d.clone();
                c.handle(cx, TIP_SET_DATA, &d);
            }
        });
        let at = self.padded_viewport_position(cx, 50.0);
        MenuView::add_to_viewport_at(cx, tip, self.z_order, at[0], at[1]);
        wd(cx, tip).vis = Vis::SelfHitTestInvisible;
        let [dx, dy] = cx.view.desired_size(cx.fonts, tip);
        let by = self.placement_offset(cx, p.placement, dx, dy);
        wd(cx, tip).transform.translation = [p.offset[0] + by[0], p.offset[1] + by[1]];
        self.tip = Some(tip);
        self.is_open = true;
    }

    /// GetTooltipWidget: the open tip.
    pub fn tooltip_widget(&self) -> Option<WidgetId> {
        self.tip
    }

    /// CloseTooltip @3168 (the tip's WillHide does nothing in UI_Tooltip).
    pub fn close_tooltip(&mut self, cx: &mut Cx<'_>) {
        if !self.is_open {
            return;
        }
        if let Some(t) = self.tip.take() {
            MenuView::discard(cx, t);
        }
        self.is_open = false;
    }

    /// The select @2517 by EMenuPlacement: 1 CenteredBelowAnchor, 6 AboveAnchor, 7 CenteredAboveAnchor, 9 MenuLeft,
    /// 10 Center, 12 MatchBottomLeft (by the quadrant of the viewport); the others add nothing.
    fn placement_offset(&self, cx: &Cx<'_>, placement: u8, dx: f64, dy: f64) -> [f64; 2] {
        match placement {
            1 => [-0.5 * dx, 0.0],
            6 => [0.0, -dy],
            7 => [-0.5 * dx, -dy],
            9 => [-dx, -0.5 * dy],
            10 => [-0.5 * dx, -0.5 * dy],
            12 => self.offset_by_quadrant(cx, dx, dy),
            _ => [0.0, 0.0],
        }
    }

    /// PaddedViewportPosition: the centre of TooltipAnchor on the viewport (Slate units), kept Padding inside the
    /// viewport (FClamp).
    fn padded_viewport_position(&self, cx: &Cx<'_>, padding: f64) -> [f64; 2] {
        let a = find(cx, "TooltipAnchor");
        let [aw, ah] = cx.view.tree().arranged_size(a);
        let dpi = cx.view.dpi;
        let [vw, vh] = cx.view.viewport_pixels();
        let c = cx.view.local_to_viewport(a, aw / 2.0, ah / 2.0);
        let clamp = |x: f64, max: f64| {
            if x < padding {
                padding
            } else if x < max {
                x
            } else {
                max
            }
        };
        [clamp(c[0], vw / dpi - padding), clamp(c[1], vh / dpi - padding)]
    }

    /// OffsetBasedOnViewportQuadrant: the tip opens towards the middle of the viewport. The point compared with the
    /// viewport's half size (pixels) is UI_Functions GetWidgetPosition's Viewport times the viewport scale: the
    /// point (GetLocalTopLeft + LocalSize / 2) x scale taken as a local point of TooltipAnchor, as the
    /// interactor's own position (update_position). A point on a half line adds nothing.
    fn offset_by_quadrant(&self, cx: &Cx<'_>, dx: f64, dy: f64) -> [f64; 2] {
        let a = find(cx, "TooltipAnchor");
        let [ax, ay, aw, ah] = cx.view.tree().arranged(a);
        let m = cx.view.tree().painted(a).map(|p| p.transform);
        let dpi = cx.view.dpi;
        let [vw, vh] = cx.view.viewport_pixels();
        let lx = (ax + aw / 2.0) * dpi;
        let ly = (ay + ah / 2.0) * dpi;
        let (x, y) = m.map_or((0.0, 0.0), |m| (m[0] * lx + m[2] * ly + m[4], m[1] * lx + m[3] * ly + m[5]));
        let (hx, hy) = (vw * 0.5, vh * 0.5);
        if x < hx && y < hy {
            [0.0, 0.0]
        } else if x < hx && y > hy {
            [0.0, -dy]
        } else if x > hx && y < hy {
            [-dx, 0.0]
        } else if x > hx && y > hy {
            [-dx, -dy]
        } else {
            [0.0, 0.0]
        }
    }
}

impl Controller for TooltipTarget {
    /// @1293: the instance's Tooltip Params (the class default has no class and ShowTooltip false), unless an
    /// owner's Init has written them already.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        if !self.params_written {
            self.params = tooltip_params_of(&prop(cx, "Tooltip Params")).unwrap_or_else(no_tooltip);
        }
        let manual = prop_bool(cx, "Manually Triggered");
        let hb = find(cx, "hitbox");
        wd(cx, hb).vis = if manual { Vis::SelfHitTestInvisible } else { Vis::Visible };
    }

    /// @15 (the handler does nothing without Auto Close When Modals: only those are bound here).
    fn construct(&mut self, cx: &mut Cx<'_>) {
        if !prop_bool(cx, "Auto Close When Modals") || self.modal_bound {
            return;
        }
        self.modal_bound = true;
        cx.view.manager.modal_shown.add(Handler { owner: cx.w, id: H_MODAL_SHOWN });
    }

    /// @3530.
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        self.close_tooltip(cx);
        if self.modal_bound {
            cx.view.manager.modal_shown.remove(Handler { owner: cx.w, id: H_MODAL_SHOWN });
        }
        self.modal_bound = false;
    }

    /// hitbox OnHovered @1353 / OnUnhovered @1388.
    fn on_button_hover(&mut self, cx: &mut Cx<'_>, name: &str, hovered: bool) {
        if name == "hitbox" && !prop_bool(cx, "Manually Triggered") {
            self.toggle_tooltip(cx, hovered);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == H_MODAL_SHOWN {
            self.close_tooltip(cx);
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == R_AFTER_DELAY {
            self.after_delay(cx);
        }
    }

    /// An interactor's Toggle Tooltip: Init(params, player), then ToggleTooltip(show).
    fn tooltip_target(&mut self, cx: &mut Cx<'_>, params: &TooltipParams, player: i32, show: bool) {
        self.init(params.clone(), player);
        self.toggle_tooltip(cx, show);
    }
}

// ---------- UI_CalloutTooltip ----------

const C_MODAL_SHOWN: u32 = 1;
const C_DISMISS_CLICK: u32 = 2;

/// The callout (UI_CalloutTooltip): a pointed box with a text and a dismiss button. Show @257 plays ShowAnim at
/// speed 2; Hide @899 -> @309 (also the dismiss button's click @112) fires DismissClicked, takes the button out of
/// play and removes the widget; a modal shown (the manager's ModalShownEvent, bound at OnInitialized @499 ->
/// @1126) removes it too. Set Flipped @904 turns SizeBox_0 by 180 and the label and the button back. Not ported:
/// the match state's CharSelect close (@724, no match in the menus), Set Description / Data / Player Index /
/// Clickable and GetCharacterData (@10-@35: empty bodies).
#[derive(Debug, Default)]
pub struct CalloutTooltip {
    pub dismiss_clicked: Event<()>,
}

impl CalloutTooltip {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CalloutTooltip::default())
    }

    fn dismiss_interactor(cx: &Cx<'_>) -> WidgetId {
        let d = find(cx, "buttonDismiss");
        cx.view.tree().find_in(d, "UI_Interactor")
    }

    fn set_text(cx: &mut Cx<'_>, t: &str) {
        let l = find(cx, "tooltipLabel");
        wd(cx, l).text = t.to_string();
    }

    pub fn set_flipped(cx: &mut Cx<'_>, flipped: bool) {
        let a = if flipped { 180.0 } else { 0.0 };
        for n in ["SizeBox_0", "tooltipLabel", "buttonDismiss"] {
            let w = find(cx, n);
            wd(cx, w).transform.angle = a;
        }
    }

    /// UpdateContext @131.
    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let i = Self::dismiss_interactor(cx);
        if let Some(s) = cx.view.manager.interactor_mut(i) {
            s.context = ctx.to_string();
        }
    }

    fn hide(&mut self, cx: &mut Cx<'_>) {
        cx.emit(self, |c| &c.dismiss_clicked, &());
        let i = Self::dismiss_interactor(cx);
        set_interactable(cx, i, false);
        Self::remove(cx);
    }

    fn remove(cx: &mut Cx<'_>) {
        let me = cx.w;
        MenuView::remove_from_viewport(cx, me);
    }
}

impl Controller for CalloutTooltip {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        cx.view.manager.modal_shown.add(Handler { owner: me, id: C_MODAL_SHOWN });
        // the dismiss button (an IconButton): its click hides the callout
        let d = find(cx, "buttonDismiss");
        cx.visit::<super::buttons::IconButton, _>(d, |b, _| b.on_click.add(Handler { owner: me, id: C_DISMISS_CLICK }));
    }

    /// PreConstruct @800: Set Flipped(Flip), Set Text(Label) when it is not empty.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let flip = prop_bool(cx, "Flip");
        Self::set_flipped(cx, flip);
        let l = prop(cx, "Label");
        let t = l
            .get("LocalizedString")
            .or_else(|| l.get("SourceString"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !t.is_empty() {
            Self::set_text(cx, &t);
        }
    }

    /// Construct @40: the focus frame follows the dismiss button.
    fn construct(&mut self, cx: &mut Cx<'_>) {
        let fc = find(cx, "UI_FocusedCommon");
        let i = Self::dismiss_interactor(cx);
        focus_frame(cx, fc, i);
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        cx.view.manager.modal_shown.remove(Handler { owner: cx.w, id: C_MODAL_SHOWN });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            C_MODAL_SHOWN => Self::remove(cx),
            C_DISMISS_CLICK => self.hide(cx),
            TIP_SET_TEXT => {
                if let Some(t) = arg.downcast_ref::<String>() {
                    Self::set_text(cx, t);
                }
            }
            TIP_SHOW => {
                let me = cx.w;
                cx.view.play(me, "ShowAnim", false, 2.0);
            }
            _ => {}
        }
    }
}
