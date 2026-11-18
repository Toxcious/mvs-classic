//! The main menu's sidebar: WBP_GlobalNav and its parts: WBP_GlobalSlideOut,
//! UI_ToolbarIconButton, WBP_MainSectionButton, UI_PartyMemberSlot, WBP_PlayerDataNew, UI_CurrencyLabel.
//!
//! The sidebar online: Fighters collapsed, Collection and Glossary shown (Battle Pass and Shop taken out:
//! `port_buttons`), gold and Gleamium at 0. Owed (no owner yet): the profile icons (`profileIconMaterial`: the player's own cover and a party member's);
//! the party is never joined offline, so a member slot collapses itself.

use std::any::Any;

use mvs_net::service::api::PartyView;
use serde_json::Value;

use super::common::{enum_index, rgba, text};
use super::controls::bar_buttons::{BarButton, P_ICON, P_ID, P_LABEL, P_TOOLTIP};
use super::controls::buttons::WHITE;
use super::controls::interactor::{batch_set_context, set_focused, set_interactable};
use super::controls::tooltip::{TooltipTarget, close_all_tooltips, tooltip_params};
use super::controls::{bind, find, focus_frame, ist, prop, prop_bool, set_tooltip, wd};
use super::modals::can_show_modal;
use super::section_bar::{SectionBar, suppress_focus};
use super::social_context_menu::{player_data, show_social_actions_menu};
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::hydra::picture_name;
use crate::menu::manager::{Manager, NULL_GUID};
use crate::menu::own_profile::{my_profile_icon, profile_icon_by_asset, profile_icon_material, watch_profile_icon};
use crate::slate::{HAlign, Margin, VAlign, Vis, WidgetId};

/// The constructed widgets of user class `cls`, in creation order.
pub fn live(cx: &Cx<'_>, cls: &str) -> Vec<WidgetId> {
    let t = cx.view.tree();
    let mut out: Vec<WidgetId> =
        t.ids().filter(|&w| t.user_class(w) == Some(cls) && cx.view.is_constructed(w)).collect();
    // (an index is taken again once its widget is destroyed: the order made is kept apart)
    out.sort_by_key(|&w| t.born(w));
    out
}

/// The user widget instance `n` under `w`, its UI_Interactor child.
fn interactor_in(cx: &Cx<'_>, w: WidgetId, n: &str) -> WidgetId {
    let s = cx.view.tree().find_in(w, n);
    cx.view.tree().find_in(s, "UI_Interactor")
}

/// A button of a bar as its BarButton.
fn bar_button<R>(cx: &mut Cx<'_>, w: WidgetId, f: impl FnOnce(&mut dyn BarButton, &mut Cx<'_>) -> R) -> Option<R> {
    cx.visit_dyn(w, |c, cx| c.as_bar_button().map(|b| f(b, cx))).flatten()
}

/// FindOrCreateGlobalNav(false): the newest sidebar that is visible.
pub fn find_global_nav(cx: &Cx<'_>) -> Option<WidgetId> {
    live(cx, "WBP_GlobalNav").into_iter().rev().find(|&w| cx.view.visible_in_hierarchy(w))
}

// ---------- WBP_GlobalSlideOut ----------

const SO_DISMISS: u32 = 1;
const SO_WAIT: u32 = 2;

/// WBP_GlobalSlideOut.
#[derive(Debug)]
pub struct GlobalSlideOut {
    pub modal_id: String,
    pub stored_context: String,
    /// PreviousContext: set by SetPreviousContext only; the null GUID otherwise, which DelayedSetFocus takes as the
    /// sidebar's main context.
    pub previous_context: String,
    closing: bool,
    /// The modal id the closing wait works for.
    closing_id: String,
}

impl GlobalSlideOut {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(GlobalSlideOut {
            modal_id: String::new(),
            stored_context: NULL_GUID.into(),
            previous_context: NULL_GUID.into(),
            closing: false,
            closing_id: String::new(),
        })
    }

    pub fn open_slide_out(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.modal_id = id.to_string();
        let me = cx.w;
        if !cx.view.is_animation_playing(me, "AnimIn") {
            cx.view.play_animation(me, "AnimIn", false, 1.0, 1, false, false);
        } else if !cx.view.is_animation_playing_forward(me, "AnimIn") {
            cx.view.reverse_animation(me, "AnimIn");
            cx.view.set_playback_speed(me, "AnimIn", 1.0);
        }
    }

    /// The panel slides back at 1.5x and the modal is dismissed on the first frame the animation has ended on.
    pub fn close_slide_out(&mut self, cx: &mut Cx<'_>, id: &str) {
        if let Some(n) = find_global_nav(cx) {
            cx.visit::<GlobalNav, _>(n, |n, cx| n.restore_main_exit_button(cx));
        }
        let me = cx.w;
        if !cx.view.is_animation_playing(me, "AnimIn") {
            cx.view.play_animation(me, "AnimIn", true, 1.5, 1, false, false);
        } else if cx.view.is_animation_playing_forward(me, "AnimIn") {
            cx.view.reverse_animation(me, "AnimIn");
            cx.view.set_playback_speed(me, "AnimIn", 1.5);
        }
        if self.closing {
            return;
        }
        self.closing = true;
        self.closing_id = id.to_string();
        cx.view.delay(0.0, Handler { owner: me, id: SO_WAIT });
    }

    pub fn set_previous_context(&mut self, ctx: &str) {
        self.previous_context = ctx.to_string();
    }

    pub fn set_global_nav_available(cx: &mut Cx<'_>, w: WidgetId, b: bool) {
        let c = cx.view.tree().find_in(w, "CanvasRoot");
        cx.view.tree_mut().widget_mut(c).pad.l = if b { 127.0 } else { 0.0 };
    }

    pub fn update_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        self.stored_context = ctx.to_string();
        let back = find(cx, "Btn_Back");
        let list = [interactor_in_self(cx, back), find(cx, "dismissButton")];
        batch_set_context(cx, &list, ctx);
    }

    fn fill(cx: &mut Cx<'_>) {
        let b = find(cx, "SizeBox_2");
        super::common::fill_page(cx, b);
    }

    fn on_dismiss_click(&mut self, cx: &mut Cx<'_>) {
        let modal = cx.view.manager.get_modal(&self.modal_id);
        match modal {
            Some(m) => {
                cx.visit_dyn(m, |c, cx| {
                    if let Some(m) = c.as_modal() {
                        m.request_dismiss(cx, "close", false);
                    }
                });
            }
            None => {
                let id = self.modal_id.clone();
                self.close_slide_out(cx, &id);
                cx.post("ui_menu_global_back_escape");
            }
        }
    }
}

/// A user widget's own UI_Interactor child.
fn interactor_in_self(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
    cx.view.tree().find_in(w, "UI_Interactor")
}

impl Controller for GlobalSlideOut {
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let back = find(cx, "Btn_Back");
        let show = prop_bool(cx, "ShowBackButton");
        wd(cx, back).vis = if show { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        let c = rgba(&prop(cx, "Color"));
        let bg = find(cx, "BGColor");
        wd(cx, bg).color = c;
        let b = prop(cx, "BackButtonPadding");
        let g = |k: &str| b.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        wd(cx, back).pad = Margin { l: g("Left"), t: g("Top"), r: g("Right"), b: g("Bottom") };
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        let c = find(cx, "Overlay_Container");
        let w = wd(cx, c);
        w.transform.translation = [-1500.0, 0.0];
        let back = find(cx, "Btn_Back");
        let d = find(cx, "dismissButton");
        let bi = interactor_in_self(cx, back);
        bind(cx, d, |s| &mut s.on_click, SO_DISMISS);
        bind(cx, bi, |s| &mut s.on_click, SO_DISMISS);
        Self::fill(cx);
    }

    /// Deviation: the slide-out's 16:9 box follows the page like the main menu's, so it stays beside the sidebar.
    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        Self::fill(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == SO_DISMISS {
            self.on_dismiss_click(cx);
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id != SO_WAIT {
            return;
        }
        let me = cx.w;
        if cx.view.is_animation_playing(me, "AnimIn") {
            cx.view.delay(0.0, Handler { owner: me, id: SO_WAIT });
            return;
        }
        self.closing = false;
        Manager::unfocus_all(cx, 0);
        // @594-912: Latest Modal ID is read before DismissModal @912 (the closing modal's own id: the nav's Update Focus
        // then focuses that modal's nav button)
        let latest = Manager::latest_modal_id(cx.view);
        let id = std::mem::take(&mut self.closing_id);
        if id.is_empty() {
            Manager::dismiss_current_modal(cx);
        } else {
            Manager::dismiss_modal(cx, &id);
        }
        // @232 FindOrCreateGlobalNav(false).DelayedSetFocus(PreviousContext, Latest Modal ID)
        if let Some(n) = find_global_nav(cx) {
            let prev = self.previous_context.clone();
            cx.visit::<GlobalNav, _>(n, |n, cx| n.delayed_set_focus(cx, &prev, &latest));
        }
    }
}

// ---------- UI_ToolbarIconButton (the sidebar's Settings button) ----------

const TOOLBAR_FOCUSED: Rgba = [0.006049, 0.009721, 0.021219, 1.0];
const TOOLBAR_OFF: Rgba = [0.194618, 0.571125, 0.552012, 1.0];

const TB_FOCUS: u32 = 1;
const TB_UNFOCUS: u32 = 2;
const TB_CLICK: u32 = 3;
const TB_WAIT: u32 = 4;

/// The tip shown with the focus is the interactor's (TooltipTxt, to the right of the button).
#[derive(Debug, Default)]
pub struct ToolbarIconButton {
    pub on_click: Event<()>,
    pub is_on: bool,
    waiting: bool,
    tooltip_txt: String,
    /// The tip's offset and placement in place of the sidebar's (60, -40), MenuRight (the port's own buttons).
    pub tip_place: Option<([f64; 2], u8)>,
}

impl ToolbarIconButton {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(ToolbarIconButton::default())
    }

    pub fn interactor(cx: &Cx<'_>, w: WidgetId) -> WidgetId {
        interactor_in_self(cx, w)
    }

    pub fn update_appearance(&mut self, cx: &mut Cx<'_>) {
        let i = find(cx, "UI_Interactor");
        let focused = cx.view.manager.interactor(i).is_some_and(|s| s.is_focused);
        let c = rgba(&prop(cx, if self.is_on || focused { "On Color" } else { "Off Color" }));
        let (icon, amount) = (find(cx, "IconWidget"), find(cx, "AmountWidget"));
        wd(cx, icon).color = c;
        wd(cx, amount).color = c;
        let size = prop(cx, "IconSize");
        let g = |k: &str| size.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let w = wd(cx, icon);
        w.brush_w = g("X");
        w.brush_h = g("Y");
        let on = find(cx, "OnIndicator_1");
        wd(cx, on).vis = if self.is_on { Vis::SelfHitTestInvisible } else { Vis::Hidden };
        self.set_tooltip(cx);
    }

    /// UpdateAppearance @3106-3457 and UpdateTexture @4308: Offset (60, -40), MenuRight.
    fn set_tooltip(&self, cx: &mut Cx<'_>) {
        let i = find(cx, "UI_Interactor");
        let ([x, y], at) = self.tip_place.unwrap_or(([60.0, -40.0], 5));
        set_tooltip(cx, i, tooltip_params(&self.tooltip_txt, !self.tooltip_txt.is_empty(), x, y, at));
    }

    /// UpdateTexture @4191: the icon; an empty text leaves the tip as it is.
    pub fn update_texture(&mut self, cx: &mut Cx<'_>, tex: &str, tooltip: &str) {
        let icon = find(cx, "IconWidget");
        wd(cx, icon).tex = Some(tex.to_string());
        if tooltip.is_empty() {
            return;
        }
        self.tooltip_txt = tooltip.to_string();
        self.set_tooltip(cx);
    }

    pub fn set_on(&mut self, cx: &mut Cx<'_>, v: bool) {
        self.is_on = v;
        if !prop_bool(cx, "UseSquareHighlight") {
            self.update_appearance(cx);
        }
    }

    pub fn update_context(&self, cx: &mut Cx<'_>, ctx: &str) {
        let i = find(cx, "UI_Interactor");
        batch_set_context(cx, &[i], ctx);
    }

    /// Hover towards the wanted direction: started when it is not playing, turned around when it plays the other way.
    fn hover(cx: &mut Cx<'_>, reverse: bool, speed: f64) -> bool {
        let me = cx.w;
        if !cx.view.is_animation_playing(me, "Hover") {
            cx.view.play_animation(me, "Hover", reverse, speed, 1, false, false);
        } else if cx.view.is_animation_playing_forward(me, "Hover") == reverse {
            cx.view.reverse_animation(me, "Hover");
            cx.view.set_playback_speed(me, "Hover", speed);
        } else {
            return false;
        }
        true
    }
}

impl Controller for ToolbarIconButton {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        self.tooltip_txt = text(Some(&prop(cx, "TooltipTxt")));
        let i = find(cx, "UI_Interactor");
        bind(cx, i, |s| &mut s.on_focus, TB_FOCUS);
        bind(cx, i, |s| &mut s.on_unfocus, TB_UNFOCUS);
        bind(cx, i, |s| &mut s.on_click, TB_CLICK);
    }

    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let amount = find(cx, "AmountWidget");
        wd(cx, amount).vis = if prop_bool(cx, "Show Amount") { Vis::Visible } else { Vis::Collapsed };
        let icon = find(cx, "IconWidget");
        let tex = prop(cx, "Icon").as_str().filter(|s| !s.is_empty() && *s != "None").map(str::to_string);
        wd(cx, icon).tex = tex;
        self.update_appearance(cx);
        let q = find(cx, "UselessQuad");
        wd(cx, q).vis = if prop_bool(cx, "UseSquareHighlight") { Vis::Hidden } else { Vis::Collapsed };
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        let (icon, amount, bg) = (find(cx, "IconWidget"), find(cx, "AmountWidget"), find(cx, "FocusBG"));
        match id {
            TB_FOCUS => {
                if !self.is_on {
                    wd(cx, icon).color = TOOLBAR_FOCUSED;
                    wd(cx, bg).vis = Vis::SelfHitTestInvisible;
                }
                wd(cx, amount).color = TOOLBAR_FOCUSED;
                Self::hover(cx, false, 5.0);
            }
            TB_UNFOCUS => {
                if !self.is_on {
                    wd(cx, icon).color = TOOLBAR_OFF;
                    wd(cx, bg).vis = Vis::Hidden;
                }
                let off = rgba(&prop(cx, "Off Color"));
                wd(cx, amount).color = off;
                if !Self::hover(cx, true, 8.0) || self.waiting {
                    return;
                }
                // when the animation has ended, a button that is on loses its focus backing
                self.waiting = true;
                let me = cx.w;
                cx.view.delay(0.0, Handler { owner: me, id: TB_WAIT });
            }
            TB_CLICK => self.on_click.clone().fire(cx, &()),
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id != TB_WAIT {
            return;
        }
        let me = cx.w;
        if cx.view.is_animation_playing(me, "Hover") {
            cx.view.delay(0.0, Handler { owner: me, id: TB_WAIT });
            return;
        }
        self.waiting = false;
        if self.is_on {
            let bg = find(cx, "FocusBG");
            wd(cx, bg).vis = Vis::Hidden;
        }
    }
}

// ---------- WBP_MainSectionButton (a section button of the sidebar) ----------

/// label and icon of a button whose interactor is not interactable (@761)
const SECTION_DISABLED: Rgba = [0.246201, 0.520996, 0.508881, 0.2];
/// the tip of a button in tooltip-only mode (the text constant @4671)
const SECTION_COMING_SOON: &str = "COMING SOON";

const MS_FOCUS: u32 = 1;
const MS_UNFOCUS: u32 = 2;
const MS_CLICK: u32 = 3;
const MS_INTERACTABLE: u32 = 4;
const MS_NOT_INTERACTABLE: u32 = 5;

/// The button class of the sidebar's UI_SectionBar: an icon over two quads (HoverQuad with the focus, SelectedQuad while
/// it is on). The colours are SelectColor(A, B, bPickA) literals. The tip is the interactor's: Tooltip Params written at
/// InitBarButton / PreConstruct (skipped for an empty Params.Tooltip) and at SetToolTipOnly: UI_Tooltip, ShowTooltip,
/// Offset (80, -30), Delay 0, FollowMouse false, Placement 2.
/// Not ported: Params.AttachmentWidget, UI_Attention's Display for the shop and Set Viewed at a click (UI_Attention
/// has no controller; its MainContainer is collapsed here), Notify's move, SetPrimaryButtonIsEnabled (@5151).
/// Unverified: the two Delay(0) loops while Hover plays (@15-122 after a focus, @161-268 after an unfocus) end in
/// pop_flow when it has stopped; where their resume points lead is not traceable.
#[derive(Debug, Default)]
pub struct MainSectionButton {
    is_on: bool,
    tooltip_only: bool,
    original_id: String,
    params: Value,
}

impl MainSectionButton {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(MainSectionButton::default())
    }

    /// The button's ID (Params).
    pub fn id(&self) -> String {
        self.params.get(P_ID).and_then(Value::as_str).unwrap_or_default().to_lowercase()
    }

    fn paint(cx: &mut Cx<'_>, c: Rgba) {
        let (l, i) = (find(cx, "label"), find(cx, "Icon"));
        wd(cx, l).color = c;
        wd(cx, i).color = c;
    }

    /// @3918-4070, then the pushed @1669.
    fn apply_params(&mut self, cx: &mut Cx<'_>) {
        let p = if self.params.is_null() { prop(cx, "Params") } else { self.params.clone() };
        let icon = find(cx, "Icon");
        wd(cx, icon).tex = p.get(P_ICON).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        let l = find(cx, "label");
        wd(cx, l).text = text(p.get(P_LABEL));
        self.set_on(cx, self.is_on);
        let att = find(cx, "UI_Attention");
        let mc = cx.view.tree().find_in(att, "MainContainer");
        wd(cx, mc).vis = Vis::Collapsed;
        // (a tooltip-only button keeps SetToolTipOnly's tip: PreConstruct runs again at each rebuild of the tree, which in
        // the build comes before UpdateDisabledSections)
        let tip = if self.tooltip_only { SECTION_COMING_SOON.to_string() } else { text(p.get(P_TOOLTIP)) };
        if !tip.is_empty() {
            let i = find(cx, "UI_Interactor");
            set_tooltip(cx, i, tooltip_params(&tip, true, 80.0, -30.0, 2));
        }
    }

    /// SetToolTipOnly @4555: the button keeps its tip and leaves the bar's clicks (an empty sectionBarID). Its one caller
    /// is WBP_GlobalNav UpdateSectionToolTipOnly @616. Interactable and focusable as before, drawn at full opacity in
    /// the dim colours.
    pub fn set_tool_tip_only(&mut self, cx: &mut Cx<'_>, v: bool) {
        self.tooltip_only = v;
        let i = find(cx, "UI_Interactor");
        let orig = self.original_id.clone();
        if let Some(s) = ist(cx, i) {
            s.section_bar_id = if v { String::new() } else { orig };
        }
        let tip = if v { SECTION_COMING_SOON.to_string() } else { text(self.params.get(P_TOOLTIP)) };
        set_tooltip(cx, i, tooltip_params(&tip, true, 80.0, -30.0, 2));
        // @747: the dimmed colours in tooltip-only mode (@761), else the off colour (@1103)
        Self::paint(cx, if v { SECTION_DISABLED } else { TOOLBAR_OFF });
    }
}

impl BarButton for MainSectionButton {
    /// InitBarButton @4459.
    fn init_bar_button(&mut self, cx: &mut Cx<'_>, params: &Value) {
        let i = find(cx, "UI_Interactor");
        self.original_id = cx.view.manager.interactor(i).map(|s| s.section_bar_id.clone()).unwrap_or_default();
        self.params = params.clone();
        self.apply_params(cx);
    }

    /// Set On @2706.
    fn set_on(&mut self, cx: &mut Cx<'_>, v: bool) {
        self.is_on = v;
        if self.tooltip_only {
            return;
        }
        let q = find(cx, "SelectedQuad");
        wd(cx, q).vis = if v { Vis::Visible } else { Vis::Hidden };
        let i = find(cx, "UI_Interactor");
        if !cx.view.manager.interactor(i).is_some_and(|s| s.is_enabled()) {
            return;
        }
        Self::paint(cx, if v { WHITE } else { TOOLBAR_OFF });
    }

    fn is_on(&self) -> bool {
        self.is_on
    }

    /// GetTooltipOnlyMode: Tool Tip Only (UI_SectionBar ButtonClicked returns for it).
    fn tooltip_only_mode(&self) -> bool {
        self.tooltip_only
    }
}

impl Controller for MainSectionButton {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = find(cx, "UI_Interactor");
        bind(cx, i, |s| &mut s.on_focus, MS_FOCUS);
        bind(cx, i, |s| &mut s.on_unfocus, MS_UNFOCUS);
        bind(cx, i, |s| &mut s.on_click, MS_CLICK);
        // OnInteractable @4550 -> 1103 (the off colour, on or not); OnNotInteractable @4545 -> 761
        bind(cx, i, |s| &mut s.on_interactable, MS_INTERACTABLE);
        bind(cx, i, |s| &mut s.on_not_interactable, MS_NOT_INTERACTABLE);
    }

    /// PreConstruct @4540 -> 3918 (the class default Params until InitBarButton has run).
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        self.apply_params(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        let me = cx.w;
        match id {
            // OnFocus @2066: Hover forward at speed 5 (@2361), turned around when it plays in reverse (@2630). End block
            // @465: nothing for a button that is on.
            MS_FOCUS => {
                if self.tooltip_only {
                    return;
                }
                if !cx.view.is_animation_playing(me, "Hover") {
                    cx.view.play_animation(me, "Hover", false, 5.0, 1, false, false);
                } else if !cx.view.is_animation_playing_forward(me, "Hover") {
                    cx.view.reverse_animation(me, "Hover");
                    cx.view.set_playback_speed(me, "Hover", 5.0);
                }
                if self.is_on {
                    return;
                }
                let q = find(cx, "HoverQuad");
                wd(cx, q).vis = Vis::SelfHitTestInvisible;
                // SelectColor(DARK, WHITE, isOn) @518 with isOn false
                let i = find(cx, "Icon");
                wd(cx, i).color = WHITE;
            }
            // OnUnfocus @3278: Hover in reverse at speed 8 (@3815), turned around when it plays forward (@3573).
            MS_UNFOCUS => {
                if self.tooltip_only {
                    return;
                }
                if !cx.view.is_animation_playing(me, "Hover") {
                    cx.view.play_animation(me, "Hover", true, 8.0, 1, false, false);
                } else if cx.view.is_animation_playing_forward(me, "Hover") {
                    cx.view.reverse_animation(me, "Hover");
                    cx.view.set_playback_speed(me, "Hover", 8.0);
                }
                let q = find(cx, "HoverQuad");
                wd(cx, q).vis = Vis::Collapsed;
                let i = find(cx, "Icon");
                wd(cx, i).color = if self.is_on { WHITE } else { TOOLBAR_OFF };
            }
            // OnClick @4388
            MS_CLICK => {
                if !self.tooltip_only {
                    self.set_on(cx, true);
                }
            }
            MS_INTERACTABLE => Self::paint(cx, TOOLBAR_OFF),
            MS_NOT_INTERACTABLE => Self::paint(cx, SECTION_DISABLED),
            _ => {}
        }
    }

    fn as_bar_button(&mut self) -> Option<&mut dyn BarButton> {
        Some(self)
    }
}

// ---------- WBP_PlayerDataNew and UI_PartyMemberSlot (the sidebar's profile button) ----------

/// The class `profile` opens: UI_Profile_v2 (Unverified: the beta's UI_Functions IsRankedAvailable? decides).
pub const PROFILE_CLASS: &str = "UI_Profile_v2";
/// the modals a profile click leaves open (@12084)
const PROFILE_EXCEPTIONS: [&str; 3] = ["customGame", "guilddungeon", "localPlay"];

/// Update Party Info's looping timer (Construct's SetTimer, 2 s).
const PARTY_INFO_PERIOD: f64 = 2.0;

const PM_FOCUS: u32 = 1;
const PM_UNFOCUS: u32 = 2;
const PM_CLICK: u32 = 3;

/// The player's own slot is the online one (the interactor Visible). A party member's slot (Party Member Index >= 1)
/// follows the party (Update Party Info, every 2 s), which offline has no members: it collapses. The own slot's cover
/// is the equipped profile icon (`own_profile`), a member's its presence icon.
// Not ported: a member slot's focus Zoom (the port binds its click only), its level
// (Fetch User Level Data @8609, Set Level Info: the port fetches no other player's level; the tree's TextBlock_Level
// text stays) and OnPartyChanged's UpdateLeaderIndicator (@10600).
#[derive(Debug, Default)]
pub struct PartyMemberSlot {
    /// The texture the own slot's cover shows (the instance is made again only for another texture).
    cover_tex: String,
    /// The profile icon slug `set_cover` last looked up (None: none yet).
    cover_slug: Option<String>,
    /// A member slot's: the texture of the member's picture on ProfileIcon ("" none).
    member_tex: String,
    /// Time to Update Party Info's next run (Construct's 2 s looping timer).
    party_timer: f64,
    /// A member slot's: the party as last seen (OnPartyChanged when it changes) and whether the page was queued
    /// (MatchStateChanged("Matchmaking") when it becomes so).
    seen_party: Option<Option<PartyView>>,
    seen_queued: bool,
    /// A member slot's: the member's account id as Update Party Info last set it ('' none).
    account_id: String,
}

impl PartyMemberSlot {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PartyMemberSlot::default())
    }

    /// Set Cover @5382 for the player's own slot: an empty path returns (index 0, @5500), keeping what ProfileIcon
    /// shows; else LoadAsset(the ProfileIconData) -> @12368: ProfileIcon's dynamic material takes its TextureRef as
    /// Texture (@12601), then ProfileIcon.SetVisibility(4) SelfHitTestInvisible (@12655 -> @11039). Update My Info
    /// runs it at Construct (@15 -> @595) and at OnProfileIconChanged (@10798 -> @1272); the port's store has no
    /// change event, so it runs on each tick that sees another icon.
    fn set_cover(&mut self, cx: &mut Cx<'_>) {
        // (only a change of the icon's slug is looked up: the tick calls this each frame)
        let slug = &cx.prefs.hydra.profile_icon.slug;
        if self.cover_slug.as_deref() == Some(slug.as_str()) {
            return;
        }
        self.cover_slug = Some(slug.clone());
        let Some(item) = my_profile_icon(cx) else { return };
        let Some(tex) = item.icon.as_deref().and_then(picture_name) else { return };
        if tex == self.cover_tex {
            return;
        }
        let Some((path, mid)) = profile_icon_material(cx.view, item.icon.as_deref()) else { return };
        cx.view.load_texture(&tex);
        self.cover_tex = tex;
        let img = find(cx, "ProfileIcon");
        let x = wd(cx, img);
        x.tex = Some(path);
        x.mid = Some(mid);
        x.vis = Vis::SelfHitTestInvisible;
    }

    fn index(cx: &Cx<'_>) -> i64 {
        prop(cx, "Party Member Index").as_i64().unwrap_or(0)
    }

    /// Update Party Info @6430-8542 for a member's slot: JoinedMembers without this player (GetClientGUID); with a
    /// member at the slot's index, Set Cover(Username, ProfileIconPath) (@7791) and Update Appearance(State == 0,
    /// State == 1, false) (@8061); without, Update Appearance(false, false, ..) (@8474, @8525). HideInvite only
    /// collapses InviteButtonContainer, which the tree already has collapsed.
    // (an unanswered invitee is in the build's InvitedMembers, not JoinedMembers, so its slot never shows pending
    // either: the port's party holds joined members only)
    fn update_party_info(&mut self, cx: &mut Cx<'_>) {
        let index = Self::index(cx);
        let member = cx.view.session.online.party.as_ref().and_then(|p| {
            let at = usize::try_from(index - 1).ok()?;
            p.members.iter().filter(|m| m.id != p.me).nth(at).map(|m| (m.id.clone(), m.name.clone(), m.icon.clone()))
        });
        let Some((id, name, icon)) = member else {
            self.account_id.clear();
            Self::update_appearance(cx, false);
            return;
        };
        self.account_id = id;
        self.set_member_cover(cx, &name, &icon);
        Self::update_appearance(cx, true);
    }

    /// Set Cover(Username, ProfileIconPath) @5382 for a member's slot: an empty path (@5409), the interactor's
    /// tooltip is the username (UI_Tooltip, offset 25, 25, placement 0, shown when not empty, @5501-5889) and
    /// ProfileIcon keeps what it shows; else LoadAsset -> @12368: a ProfileIconData's texture on ProfileIcon
    /// (@12601), then ProfileIcon SelfHitTestInvisible (@12655), and a path that loads no ProfileIconData stops there
    /// (@12433), ProfileIcon as it was. The path is the member's presence icon, an asset name looked up in the
    /// catalogue (`profile_icon_by_asset`): one not in it is a path that loads nothing.
    fn set_member_cover(&mut self, cx: &mut Cx<'_>, username: &str, icon: &str) {
        if icon.is_empty() {
            let i = find(cx, "UI_Interactor");
            set_tooltip(cx, i, tooltip_params(username, !username.is_empty(), 25.0, 25.0, 0));
            return;
        }
        let img = find(cx, "ProfileIcon");
        let Some(item) = profile_icon_by_asset(cx.view, icon) else { return };
        let Some(tex) = item.icon.as_deref().and_then(picture_name) else { return };
        if tex == self.member_tex {
            return;
        }
        let Some((path, mid)) = profile_icon_material(cx.view, item.icon.as_deref()) else { return };
        cx.view.load_texture(&tex);
        self.member_tex = tex;
        let x = wd(cx, img);
        x.tex = Some(path);
        x.mid = Some(mid);
        x.vis = Vis::SelfHitTestInvisible;
    }

    /// Update Appearance(Info Populated, Pending, HideInvite) @8683 with Pending false: the slot SelfHitTestInvisible
    /// (Collapsed when not populated), InviteContainer_New the same, UI_Interactor Visible (else Hidden),
    /// UserInfoContainer Visible (else Collapsed).
    pub fn update_appearance(cx: &mut Cx<'_>, populated: bool) {
        let me = cx.w;
        wd(cx, me).vis = if populated { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        for (n, on, off) in [
            ("InviteContainer_New", Vis::SelfHitTestInvisible, Vis::Collapsed),
            ("UI_Interactor", Vis::Visible, Vis::Hidden),
            ("UserInfoContainer", Vis::Visible, Vis::Collapsed),
        ] {
            let w = find(cx, n);
            wd(cx, w).vis = if populated { on } else { off };
        }
    }

    /// ShowXPBar(Show) @13280: XPBar HitTestInvisible or Collapsed; shown, AnimInXPBar plays once (@13438).
    pub fn show_xp_bar(&mut self, cx: &mut Cx<'_>, show: bool) {
        let x = find(cx, "XPBar");
        wd(cx, x).vis = if show { Vis::HitTestInvisible } else { Vis::Collapsed };
        if show {
            let me = cx.w;
            cx.view.play_animation(me, "AnimInXPBar", false, 1.0, 1, false, false);
        }
    }

    /// OnClick @11745 for the player's own slot: an open profile closes (Close Slide Outs @13207: CloseSlideOut on every
    /// WBP_GlobalSlideOut), else every modal but the exceptions is dismissed and the profile is shown.
    fn clicked(cx: &mut Cx<'_>) {
        if cx.view.manager.get_modal("profile").is_some() {
            for s in live(cx, "WBP_GlobalSlideOut") {
                cx.visit::<GlobalSlideOut, _>(s, |s, cx| s.close_slide_out(cx, ""));
            }
            return;
        }
        Manager::dismiss_all_modals_except(cx, &PROFILE_EXCEPTIONS);
        if can_show_modal(cx, PROFILE_CLASS, "profile") {
            Manager::show_modal(cx, PROFILE_CLASS, "profile", "", "");
        }
    }

    /// OnClick @11745-12308 for a member's slot (Party Member Index >= 1): nothing in a rematch (@11791), else
    /// ShowSocialActionsMenu(the member, the interactor's viewport TopLeft + (50, 50)).
    /// Unverified: the build reads the matchmaking handler's bIsRematch; the port reads a queue set's rematch begun.
    fn member_clicked(&self, cx: &mut Cx<'_>) {
        if self.account_id.is_empty() || cx.view.session.online.queue_end.as_ref().is_some_and(|q| q.rematch_begun) {
            return;
        }
        let i = find(cx, "UI_Interactor");
        let [x, y] = cx.view.local_to_viewport(i, 0.0, 0.0);
        let d = player_data(cx, &self.account_id);
        show_social_actions_menu(cx, d, [x + 50.0, y + 50.0], "", &[]);
    }
}

impl Controller for PartyMemberSlot {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = find(cx, "UI_Interactor");
        if Self::index(cx) >= 1 {
            bind(cx, i, |s| &mut s.on_click, PM_CLICK);
            return;
        }
        // Focus plays Zoom at speed 5, unfocus reverses it at speed 6.
        bind(cx, i, |s| &mut s.on_focus, PM_FOCUS);
        bind(cx, i, |s| &mut s.on_unfocus, PM_UNFOCUS);
        bind(cx, i, |s| &mut s.on_click, PM_CLICK);
        // (the catalogue's arrival seeds an empty icon: watch_profile_icon, then the cover)
        watch_profile_icon(cx);
        self.set_cover(cx);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        if Self::index(cx) >= 1 {
            // Construct: Update Party Info, then its timer every 2 s
            self.update_party_info(cx);
            self.party_timer = PARTY_INFO_PERIOD;
        } else {
            let i = find(cx, "UI_Interactor");
            wd(cx, i).vis = Vis::Visible;
        }
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        if Self::index(cx) < 1 {
            self.set_cover(cx);
        }
    }

    /// A member slot's: Construct's looping timer (K2_SetTimerDelegate(2.0, looping) @2774, run whatever the slot's
    /// visibility: a collapsed slot shows when a member arrives), OnPartyChanged @10600-10614 (CloseAllTooltips) and
    /// MatchStateChanged @12684-12736 (Update Party Info when the state is "Matchmaking": the port's page becoming
    /// queued).
    fn timers(&mut self, cx: &mut Cx<'_>, dt: f64) {
        if Self::index(cx) < 1 {
            return;
        }
        let party = cx.view.session.online.party.clone();
        if self.seen_party.as_ref().is_some_and(|p| *p != party) {
            close_all_tooltips(cx);
        }
        self.seen_party = Some(party);
        let queued = cx.view.session.online.queued();
        let matchmaking = queued && !self.seen_queued;
        self.seen_queued = queued;
        if matchmaking {
            self.update_party_info(cx);
        }
        self.party_timer -= dt;
        if self.party_timer <= 0.0 {
            self.party_timer += PARTY_INFO_PERIOD;
            self.update_party_info(cx);
        }
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        let me = cx.w;
        match id {
            PM_FOCUS => cx.view.play_animation(me, "Zoom", false, 5.0, 1, false, false),
            PM_UNFOCUS => cx.view.play_animation(me, "Zoom", true, 6.0, 1, false, false),
            PM_CLICK if Self::index(cx) >= 1 => self.member_clicked(cx),
            PM_CLICK => Self::clicked(cx),
            _ => {}
        }
    }
}

/// WBP_PlayerDataNew.
#[derive(Debug, Default)]
pub struct PlayerDataNew;

impl PlayerDataNew {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(PlayerDataNew)
    }

    /// The slots' interactors (UI_MyProfileIcon, UI_PartyMemberProfileIcon: the invite one and the slot's).
    pub fn interactors(cx: &Cx<'_>, w: WidgetId) -> Vec<WidgetId> {
        let mut list = Vec::new();
        for slot in ["UI_MyProfileIcon", "UI_PartyMemberProfileIcon"] {
            let s = cx.view.tree().find_in(w, slot);
            list.push(cx.view.tree().find_in(s, "InviteInteractor"));
            list.push(cx.view.tree().find_in(s, "UI_Interactor"));
        }
        list
    }

    /// BPI_SetContext.
    pub fn set_context(&mut self, cx: &mut Cx<'_>, ctx: &str) {
        let w = cx.w;
        let list = Self::interactors(cx, w);
        batch_set_context(cx, &list, ctx);
    }
}

impl Controller for PlayerDataNew {}

// ---------- UI_CurrencyLabel (the sidebar's gold, Friend Points and Gleamium) ----------

/// ECurrencyType (native): 0 Gold, 1 FriendPoints, 2 gems (Gleamium), 3 CharTickets; the order Set Amount's switch @1019 and
/// UI_Functions CurrencyDisplayName take.
const CURRENCY_NAMES: [&str; 7] =
    ["Gold", "Friend Points", "Gleamium", "Character Ticket", "Candy", "Holiday Cookies", "Candy"];
/// AmountWidget's colours while the Gleamium label has the focus (@7675) and after (@7848).
const CURRENCY_FOCUSED: Rgba = [0.009721, 0.011612, 0.033105, 1.0];
const CURRENCY_UNFOCUSED: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
const DOOBERS: [&str; 5] = ["DooberWidget1", "DooberWidget2", "DooberWidget3", "DooberWidget4", "DooberWidget5"];

const CL_FOCUS: u32 = 1;
const CL_UNFOCUS: u32 = 2;
const CL_CLICK: u32 = 3;

/// "1,234,567" (Conv_IntToText with grouping).
fn grouped(n: i64) -> String {
    let s = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// The amount comes from the Hydra InventoryManager. Construct's Toggle Auto Update runs a Delay(0) loop that waits for
/// ClientHydraManager, which the port has not: AmountWidget keeps the tree's "000" (Unverified: whether
/// ClientHydraManager is valid without a backend). Not ported: the gain animations (Try Animate, Play Doober) and the
/// game instance delegates. The sidebar sets 0 itself (`set_amount`).
#[derive(Debug, Default)]
pub struct CurrencyLabel {
    kind: usize,
    clickable: bool,
}

impl CurrencyLabel {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(CurrencyLabel::default())
    }

    /// Set Amount @4581-4631 without Try Animate, then Update Display @0-616: grouping; a vertical label above 9999 shows
    /// "{Number} K" with one decimal at most. Unverified: the group separator and the K form's rounding are the port's
    /// English reading.
    pub fn set_amount(&mut self, cx: &mut Cx<'_>, n: i64) {
        let t = if n > 9999 && prop_bool(cx, "IsVertical") {
            let k = (n as f64 / 100.0).round() / 10.0;
            let whole = k.trunc() as i64;
            let frac = ((k - k.trunc()) * 10.0).round() as i64;
            if frac == 0 { format!("{} K", grouped(whole)) } else { format!("{}.{frac} K", grouped(whole)) }
        } else {
            grouped(n)
        };
        let a = find(cx, "AmountWidget");
        wd(cx, a).text = t;
    }

    /// SetClickable @6017: the interactor is interactable and Visible only on a clickable label.
    pub fn set_clickable(&mut self, cx: &mut Cx<'_>, b: bool) {
        self.clickable = b;
        let i = find(cx, "UI_Interactor");
        set_interactable(cx, i, b);
        wd(cx, i).vis = if b { Vis::Visible } else { Vis::Collapsed };
    }

    /// @3333: IconOverlay and then ScaleBox_0 move into VerticalBox_96 (Fill, Fill, automatic size), both centred; the icon
    /// 44 x 44, the doobers 30 x 30; the amount 10 below the icon for the types 0 to 2.
    fn lay_out_vertical(&self, cx: &mut Cx<'_>) {
        let (vb, overlay, scale) = (find(cx, "VerticalBox_96"), find(cx, "IconOverlay"), find(cx, "ScaleBox_0"));
        if cx.view.tree().widget(overlay).parent != Some(vb) {
            for w in [overlay, scale] {
                cx.view.tree_mut().remove(w);
                let x = wd(cx, w);
                x.h_align = HAlign::Fill;
                x.v_align = VAlign::Fill;
                x.fill = 0.0;
                x.pad = Margin::default();
                cx.view.tree_mut().add(vb, w);
            }
        }
        for w in [scale, overlay] {
            wd(cx, w).h_align = HAlign::Center;
        }
        wd(cx, scale).v_align = VAlign::Center;
        let icon = find(cx, "IconWidget");
        let x = wd(cx, icon);
        x.brush_w = 44.0;
        x.brush_h = 44.0;
        for n in DOOBERS {
            let d = find(cx, n);
            let x = wd(cx, d);
            x.brush_w = 30.0;
            x.brush_h = 30.0;
        }
        if self.kind <= 2 {
            wd(cx, scale).pad = Margin { l: 0.0, t: 10.0, r: 0.0, b: 0.0 };
        }
    }

    /// InitializeTooltip @6323: the currency's name, Offset (80, -30), Placement 2, for the own UI_TooltipTarget (shown on a
    /// label that is not clickable) and the interactor's (a clickable label's tip comes with the focus).
    fn initialize_tooltip(&self, cx: &mut Cx<'_>) {
        let show = prop_bool(cx, "Show Tooltip");
        let params = tooltip_params(CURRENCY_NAMES.get(self.kind).copied().unwrap_or(""), show, 80.0, -30.0, 2);
        let t = find(cx, "UI_TooltipTarget");
        let p = params.clone();
        cx.visit::<TooltipTarget, _>(t, |t, _| t.init(p, 0));
        let i = find(cx, "UI_Interactor");
        set_tooltip(cx, i, params);
        wd(cx, t).vis = if show && !self.clickable { Vis::Visible } else { Vis::Collapsed };
    }
}

impl Controller for CurrencyLabel {
    /// OnInitialized @6141.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let v = prop(cx, "Auto Update Type");
        self.kind = match &v {
            Value::Number(n) => n.as_u64().unwrap_or(0) as usize,
            Value::String(s) => match s.rsplit("::").next().unwrap_or("") {
                "Gold" => 0,
                "FriendPoints" => 1,
                "gems" => 2,
                "CharTickets" => 3,
                _ => enum_index(Some(&v)).max(0) as usize,
            },
            _ => 0,
        };
        let (fc, i) = (find(cx, "UI_FocusedCommon"), find(cx, "UI_Interactor"));
        focus_frame(cx, fc, i);
        bind(cx, i, |s| &mut s.on_focus, CL_FOCUS);
        bind(cx, i, |s| &mut s.on_unfocus, CL_UNFOCUS);
        bind(cx, i, |s| &mut s.on_click, CL_CLICK);
    }

    /// PreConstruct @3031: the icons, the colours, the vertical layout, SetClickable, InitializeTooltip.
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let icon = prop(cx, "Icon").as_str().filter(|s| !s.is_empty() && *s != "None").map(str::to_string);
        let ic = rgba(&prop(cx, "Icon Color"));
        for n in std::iter::once("IconWidget").chain(DOOBERS) {
            let w = find(cx, n);
            let x = wd(cx, w);
            x.tex = icon.clone();
            x.color = ic;
        }
        let a = find(cx, "AmountWidget");
        let c = rgba(&prop(cx, "Amount Color"));
        wd(cx, a).color = c;
        if prop_bool(cx, "IsVertical") {
            self.lay_out_vertical(cx);
        }
        let click = prop_bool(cx, "Clickable");
        self.set_clickable(cx, click);
        self.initialize_tooltip(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        let a = find(cx, "AmountWidget");
        match id {
            // OnFocus @7634 / OnUnfocus @7980: the Gleamium label's amount turns dark and back
            CL_FOCUS if self.kind == 2 => wd(cx, a).color = CURRENCY_FOCUSED,
            CL_UNFOCUS if self.kind == 2 => wd(cx, a).color = CURRENCY_UNFOCUSED,
            // OnClick @5434: a clickable Gleamium label opens the currency shop unless it is open; another type does nothing.
            CL_CLICK => {
                if !self.clickable || self.kind != 2 {
                    return;
                }
                if cx.view.manager.get_modal("currencyShop").is_some()
                    || !can_show_modal(cx, "UI_CurrencyShop", "currencyShop")
                {
                    return;
                }
                Manager::show_modal(cx, "UI_CurrencyShop", "currencyShop", "", "");
                cx.post("ui_gleamium_open");
            }
            _ => {}
        }
    }
}

// ---------- WBP_GlobalNav (the main menu's sidebar) ----------

const NAV_MODALS: [&str; 9] =
    ["fighters", "battlepass", "collection", "guilds", "shop", "profile", "glossary", "settings", "currencyShop"];
/// The section buttons the port leaves out of the bar (Battle Pass and Shop;
/// the shop's widgets stay ported, nothing opens them).
const NAV_LEFT_OUT: [&str; 2] = ["battlepass", "shop"];
/// Exception Modal IDs (the class default): what a nav click leaves open.
const NAV_MODAL_EXCEPTIONS: [&str; 3] = ["customGame", "guilddungeon", "localPlay"];
/// The modal class of a section, shown with the section's ID as its modal id. `fighters` shows nothing; Battle Pass
/// and Shop have no button (`NAV_LEFT_OUT`).
fn nav_modal_class(id: &str) -> Option<&'static str> {
    match id.to_lowercase().as_str() {
        "collection" => Some("UI_CollectionsPanel"),
        "glossary" => Some("UI_GlossaryTooltipContainer"),
        _ => None,
    }
}
/// height of a section button (WBP_MainSectionButton's quads)
const NAV_SECTION_BUTTON: f64 = 127.0;
/// the sidebar's three UI_CurrencyLabels
const NAV_CURRENCIES: [&str; 3] = ["currencySoft", "currencyFriends", "currencyHard"];

const GN_SETTINGS: u32 = 1;
const GN_EXIT: u32 = 2;
const GN_BAR_CLICK: u32 = 3;
const GN_MODAL_SHOWN: u32 = 4;
const GN_MODAL_DISMISSED: u32 = 5;
const GN_FOCUS: u32 = 6;

/// The section bar's Button Params as the port has them, or None when they are so already.
/// Deviation: no Battle Pass and no Shop button. The build's bar has
/// both; without a backend Battle Pass was a greyed tooltip-only icon and the Shop sold the stand-in's products.
fn port_buttons(params: &[Value]) -> Option<Vec<Value>> {
    let id = |p: &Value| p.get(P_ID).and_then(Value::as_str).map(str::to_lowercase).unwrap_or_default();
    let v: Vec<Value> = params.iter().filter(|p| !NAV_LEFT_OUT.contains(&id(p).as_str())).cloned().collect();
    (v != params).then_some(v)
}

/// The sidebar online: PreConstruct's UpdateDisabledSections(DisabledSections) (the class default [Fighters]) and
/// HandleConnectionChangedStatus with a connection leave Fighters Collapsed and Battle Pass, Collection, Shop and Glossary
/// shown. The three currency labels keep the tree's Collapsed in the build; Deviation (from a live-game screenshot): gold
/// and Gleamium are shown at 0, Friend Points stays Collapsed, and the Gleamium label is not clickable.
/// The bottom button (QuitButton) has two styles (UI_ExitButtonStyle): 0 the quit icon, 1 the back arrow. An instance's
/// Exit Style is set in PreConstruct (the lobby's sidebar: 1), the Settings click sets 1 and the closing slide-out puts
/// the instance's style back. Its click steps back out of the latest modal.
#[derive(Debug)]
pub struct GlobalNav {
    pub main_context: String,
    pub current_context: String,
    pub refresh_context_paused: bool,
    bound: bool,
    /// CurrentModal: the section whose modal the last click asked for.
    current_modal: String,
    exit_style: i32,
    exit_active: bool,
    settings_only: bool,
    /// DelayedSetFocus's arguments, kept for its Delay(0).
    focus_ctx: String,
    focus_modal: String,
    /// ExitEvent(Current Context): the exit button's click with no modal open.
    pub exit_event: Event<String>,
    /// The quit style shows on this instance: one whose ExitEvent the port binds (the main menu's, Quit To Desktop; the
    /// Lab's, Return Home).
    pub exit_bound: bool,
}

impl GlobalNav {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(GlobalNav {
            main_context: NULL_GUID.into(),
            current_context: NULL_GUID.into(),
            refresh_context_paused: false,
            bound: false,
            current_modal: String::new(),
            exit_style: 0,
            exit_active: true,
            settings_only: false,
            focus_ctx: NULL_GUID.into(),
            focus_modal: String::new(),
            exit_event: Event::default(),
            exit_bound: false,
        })
    }

    fn settings_tab(cx: &Cx<'_>) -> WidgetId {
        find(cx, "SettingsTab")
    }
    fn quit_button(cx: &Cx<'_>) -> WidgetId {
        find(cx, "QuitButton")
    }
    fn section_bar(cx: &Cx<'_>) -> WidgetId {
        find(cx, "UI_SectionBar")
    }

    fn bar_buttons(cx: &mut Cx<'_>) -> Vec<WidgetId> {
        let bar = Self::section_bar(cx);
        cx.visit::<SectionBar, _>(bar, |b, _| b.buttons.clone()).unwrap_or_default()
    }

    fn button_id(cx: &mut Cx<'_>, b: WidgetId) -> String {
        cx.visit::<MainSectionButton, _>(b, |b, _| b.id()).unwrap_or_default()
    }

    fn init_event_bindings(&mut self, cx: &mut Cx<'_>) {
        if self.bound {
            return;
        }
        self.bound = true;
        let me = cx.w;
        cx.view.manager.modal_shown.add(Handler { owner: me, id: GN_MODAL_SHOWN });
        cx.view.manager.modal_dismissed.add(Handler { owner: me, id: GN_MODAL_DISMISSED });
    }

    /// ShowNavModal @12397: CurrentModal = the section's ID, then Show Modals (@11721).
    fn show_nav_modal(&mut self, cx: &mut Cx<'_>, id: &str) {
        self.current_modal = id.to_string();
        self.show_modals(cx);
    }

    /// Show Modals. @0-377: the section whose modal is the latest one closes it (RequestDismiss("close", false)) and
    /// nothing else runs. Else @438-484 every other modal but the exceptions is dismissed, the section's modal is shown
    /// (empty Instructions) and the exit button becomes the back arrow (@1035, "Back"), then CloseAllTooltips @1095.
    fn show_modals(&mut self, cx: &mut Cx<'_>) {
        let id = self.current_modal.clone();
        let latest = Manager::latest_modal_id(cx.view);
        if !latest.is_empty() && latest.to_lowercase() == id.to_lowercase() {
            if let Some(m) = cx.view.manager.get_modal(&latest) {
                cx.visit_dyn(m, |c, cx| {
                    if let Some(m) = c.as_modal() {
                        m.request_dismiss(cx, "close", false);
                    }
                });
            }
            return;
        }
        let cls = nav_modal_class(&id);
        if !cls.is_some_and(|c| can_show_modal(cx, c, &id)) {
            // Deviation: a section whose modal is not ported (no class here, or can_show_modal's warning). Its button is
            // left as the modal's dismissal would leave it (OnModalDismissed @11473) and the bar leaves the selection and
            // the focus alone.
            let bar = Self::section_bar(cx);
            suppress_focus(cx, bar);
            let b = cx.visit::<SectionBar, _>(bar, |b, _| b.button_by_id(&id)).flatten();
            if let Some(b) = b {
                bar_button(cx, b, |b, cx| b.set_on(cx, false));
            }
            return;
        }
        let cls = cls.unwrap_or_default();
        Manager::dismiss_all_modals_except(cx, &NAV_MODAL_EXCEPTIONS);
        Manager::show_modal(cx, cls, &id, "", "");
        self.customize_exit_button(cx, 1, "Back");
        close_all_tooltips(cx);
    }

    /// UpdateDisabledSections @9983 -> 7633: a button whose ID is in the list is not interactable and Collapsed
    /// (@5286-5610), the others interactable and Visible (@5830-6154); then each ComingSoonSections ID (@6357-7128)
    /// is tooltip-only: Guilds (ToggleGuildSystem is False), any other ID UpdateSectionToolTipOnly(ID, true) (@7034).
    fn update_disabled_sections(&mut self, cx: &mut Cx<'_>, list: &[String]) {
        for b in Self::bar_buttons(cx) {
            let id = Self::button_id(cx, b);
            let off = list.iter().any(|d| d.to_lowercase() == id);
            let i = interactor_in_self(cx, b);
            set_interactable(cx, i, !off);
            wd(cx, b).vis = if off { Vis::Collapsed } else { Vis::Visible };
        }
        let coming: Vec<String> = prop(cx, "ComingSoonSections")
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        for id in coming {
            for b in Self::bar_buttons(cx) {
                if Self::button_id(cx, b) == id.to_lowercase() {
                    cx.visit::<MainSectionButton, _>(b, |b, cx| b.set_tool_tip_only(cx, true));
                }
            }
        }
    }

    /// CustomizeExitButton @11736: QuitButton.UpdateTexture(style 0 ui_icons_quit, 1 ui_icons_backarrow, tooltip).
    pub fn customize_exit_button(&mut self, cx: &mut Cx<'_>, style: i32, tooltip: &str) {
        self.exit_style = style;
        let q = Self::quit_button(cx);
        let tex = if style == 1 { "ui_icons_backarrow" } else { "ui_icons_quit" };
        cx.visit::<ToolbarIconButton, _>(q, |b, cx| b.update_texture(cx, tex, tooltip));
        self.toggle_exit_button(cx, self.exit_active);
    }

    /// RestoreMainExitButton @11902 (also PreConstruct @8538): Exit Style and Exit Tooltip.
    pub fn restore_main_exit_button(&mut self, cx: &mut Cx<'_>) {
        let style = i32::from(prop(cx, "Exit Style").as_str().is_some_and(|s| s.ends_with('1')));
        let tip = text(Some(&prop(cx, "Exit Tooltip")));
        self.customize_exit_button(cx, style, &tip);
    }

    /// ToggleExitButton(Is Active) @11935-12136: the button's interactor enabled and the button SelfHitTestInvisible
    /// while Is Active and not Settings Only, else Collapsed. Deviation: the quit style shows only on an instance whose
    /// ExitEvent the port binds (`exit_bound`).
    pub fn toggle_exit_button(&mut self, cx: &mut Cx<'_>, active: bool) {
        self.exit_active = active;
        let show = active && !self.settings_only && (self.exit_style == 1 || self.exit_bound);
        let q = Self::quit_button(cx);
        let i = interactor_in_self(cx, q);
        if let Some(s) = ist(cx, i) {
            s.enabled = show;
        }
        wd(cx, q).vis = if show { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
    }

    /// ToggleSettingsOnly @12609-12960 (UI_PreMatch's rematch): every section button, the three currency labels, the exit
    /// button and the profile slots not interactable; the Settings tab enabled and interactable. Unverified:
    /// WBP_PlayerDataNew.SetInteractable is read as its two slots' interactors.
    pub fn toggle_settings_only(&mut self, cx: &mut Cx<'_>) {
        self.settings_only = true;
        for b in Self::bar_buttons(cx) {
            let i = interactor_in_self(cx, b);
            set_interactable(cx, i, false);
        }
        for n in NAV_CURRENCIES {
            let l = find(cx, n);
            cx.visit::<CurrencyLabel, _>(l, |l, cx| l.set_clickable(cx, false));
        }
        let q = Self::quit_button(cx);
        let qi = interactor_in_self(cx, q);
        set_interactable(cx, qi, false);
        let pd = find(cx, "WBP_PlayerDataNew");
        for i in PlayerDataNew::interactors(cx, pd) {
            set_interactable(cx, i, false);
        }
        let s = Self::settings_tab(cx);
        let si = interactor_in_self(cx, s);
        set_interactable(cx, si, true);
    }

    /// WBP_PlayerDataNew.UI_MyProfileIcon (UI_PartyMemberSlot).
    pub fn profile_icon(cx: &Cx<'_>, nav: WidgetId) -> WidgetId {
        let pd = cx.view.tree().find_in(nav, "WBP_PlayerDataNew");
        cx.view.tree().find_in(pd, "UI_MyProfileIcon")
    }

    /// QuitButton OnClick (@9377): the latest modal is asked to step back; without one ExitEvent goes to the owner (the
    /// Lab's pause menu, @9815).
    fn on_exit(&mut self, cx: &mut Cx<'_>) {
        let id = Manager::latest_modal_id(cx.view);
        if !id.is_empty() {
            if let Some(m) = cx.view.manager.get_modal(&id) {
                cx.visit_dyn(m, |c, cx| {
                    if let Some(m) = c.as_modal() {
                        m.request_dismiss(cx, "globalnavstepback", false);
                    }
                });
            }
        } else {
            let ctx = self.current_context.clone();
            self.exit_event.clone().fire(cx, &ctx);
        }
    }

    fn on_settings(&mut self, cx: &mut Cx<'_>) {
        if Manager::latest_modal_id(cx.view) == "settings" {
            if let Some(m) = cx.view.manager.get_modal("settings") {
                cx.visit_dyn(m, |c, cx| {
                    if let Some(m) = c.as_modal() {
                        m.request_dismiss(cx, "close", false);
                    }
                });
            }
            return;
        }
        // @9077 Dismiss All Modals with Exceptions: an open Glossary goes first
        Manager::dismiss_all_modals_except(cx, &NAV_MODAL_EXCEPTIONS);
        Manager::show_modal(cx, "UI_PreferencesRework", "settings", "", "");
        // @9316
        self.customize_exit_button(cx, 1, "Back");
    }

    fn refresh_context(&mut self, cx: &mut Cx<'_>) {
        if self.refresh_context_paused {
            return;
        }
        let id = Manager::latest_modal_id(cx.view);
        let on = id == "settings";
        if !id.is_empty() && NAV_MODALS.contains(&id.as_str()) {
            let ctx = cx
                .view
                .manager
                .get_modal(&id)
                .and_then(|m| cx.visit_dyn(m, |c, _| c.as_modal().map(|m| m.get_modal_context())).flatten())
                .unwrap_or_else(|| NULL_GUID.to_string());
            self.update_context_global_nav(cx, &ctx, false, false);
        } else {
            let ctx = self.main_context.clone();
            self.update_context_global_nav(cx, &ctx, false, false);
        }
        // @971-1147: the section button of a section's modal is on (a modal of the list without a button, profile or
        // currencyShop, returns at @1054 and leaves the Settings button as it is)
        if !id.is_empty() && !on && NAV_MODALS.contains(&id.as_str()) {
            let bar = Self::section_bar(cx);
            let Some(b) = cx.visit::<SectionBar, _>(bar, |b, _| b.button_by_id(&id)).flatten() else { return };
            bar_button(cx, b, |b, cx| b.set_on(cx, true));
        }
        let s = Self::settings_tab(cx);
        cx.visit::<ToolbarIconButton, _>(s, |s, cx| {
            s.set_on(cx, on);
            s.update_appearance(cx);
        });
    }

    pub fn update_context_global_nav(&mut self, cx: &mut Cx<'_>, ctx: &str, is_main: bool, force: bool) {
        if is_main {
            self.main_context = ctx.to_string();
        }
        if !force && ctx == self.current_context {
            return;
        }
        self.current_context = ctx.to_string();
        let bar = Self::section_bar(cx);
        cx.visit::<SectionBar, _>(bar, |b, cx| b.update_context(cx, ctx, None));
        let s = Self::settings_tab(cx);
        cx.visit::<ToolbarIconButton, _>(s, |s, cx| s.update_context(cx, ctx));
        let q = Self::quit_button(cx);
        cx.visit::<ToolbarIconButton, _>(q, |q, cx| q.update_context(cx, ctx));
        let pd = find(cx, "WBP_PlayerDataNew");
        cx.visit::<PlayerDataNew, _>(pd, |p, cx| p.set_context(cx, ctx));
        // @397: the three currency labels' interactors
        let list: Vec<WidgetId> = NAV_CURRENCIES.iter().map(|n| interactor_in(cx, cx.w, n)).collect();
        batch_set_context(cx, &list, ctx);
    }

    /// From a closing slide-out: on the next frame the sidebar follows the modal that is left (or the main context) and
    /// takes the focus back.
    pub fn delayed_set_focus(&mut self, cx: &mut Cx<'_>, ctx: &str, modal_id: &str) {
        let me = cx.w;
        self.focus_ctx = ctx.to_string();
        self.focus_modal = modal_id.to_string();
        cx.view.delay(0.0, Handler { owner: me, id: GN_FOCUS });
    }

    fn update_focus(&mut self, cx: &mut Cx<'_>, modal_id: &str) {
        let id = if modal_id.is_empty() { Manager::latest_modal_id(cx.view) } else { modal_id.to_string() };
        if !NAV_MODALS.contains(&id.as_str()) {
            return;
        }
        match id.as_str() {
            "settings" => {
                let s = Self::settings_tab(cx);
                let i = interactor_in_self(cx, s);
                set_focused(cx, i, false);
                cx.visit::<ToolbarIconButton, _>(s, |s, cx| s.update_appearance(cx));
            }
            "profile" => {
                let n = cx.w;
                let s = Self::profile_icon(cx, n);
                let i = interactor_in_self(cx, s);
                set_focused(cx, i, false);
            }
            "currencyShop" => {
                let i = interactor_in(cx, cx.w, "currencyHard");
                set_focused(cx, i, false);
            }
            _ => {
                let bar = Self::section_bar(cx);
                if let Some(b) = cx.visit::<SectionBar, _>(bar, |b, _| b.button_by_id(&id)).flatten() {
                    let i = interactor_in_self(cx, b);
                    set_focused(cx, i, false);
                }
            }
        }
    }
}

impl Controller for GlobalNav {
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        self.restore_main_exit_button(cx);
    }

    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        // HandleConnectionChangedStatus @10142 sets the party member icon Visible with a connection: the port's online
        // layout (the slot collapses itself with no member)
        let pd = find(cx, "WBP_PlayerDataNew");
        let member = cx.view.tree().find_in(pd, "UI_PartyMemberProfileIcon");
        wd(cx, member).vis = Vis::Visible;
        self.init_event_bindings(cx);
        let (st, qb) = (Self::settings_tab(cx), Self::quit_button(cx));
        cx.visit::<ToolbarIconButton, _>(st, |b, _| b.on_click.add(Handler { owner: me, id: GN_SETTINGS }));
        cx.visit::<ToolbarIconButton, _>(qb, |b, _| b.on_click.add(Handler { owner: me, id: GN_EXIT }));
        let bar = Self::section_bar(cx);
        let n_buttons = cx.visit::<SectionBar, _>(bar, |b, _| b.buttons.len()).unwrap_or(0);
        // the port's buttons (`port_buttons`): a new array, the data's left alone
        let params: Vec<Value> = prop_of(cx, bar, "Button Params");
        if n_buttons > 0
            && let Some(v) = port_buttons(&params)
        {
            cx.visit::<SectionBar, _>(bar, |b, cx| b.populate(cx, Some(v)));
        }
        // the port's gold and Gleamium labels (see the class comment)
        for n in ["currencySoft", "currencyHard"] {
            let c = find(cx, n);
            wd(cx, c).vis = Vis::Visible;
            cx.visit::<CurrencyLabel, _>(c, |c, cx| c.set_amount(cx, 0));
        }
        let hard = find(cx, "currencyHard");
        cx.visit::<CurrencyLabel, _>(hard, |c, cx| c.set_clickable(cx, false));
        // (the Glossary button's place, while the bar has no buttons)
        if n_buttons == 0 {
            wd(cx, bar).pad.b += NAV_SECTION_BUTTON;
        } else {
            let disabled: Vec<String> = prop(cx, "DisabledSections")
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            self.update_disabled_sections(cx, &disabled);
            // the bar's OnClick @11273
            cx.visit::<SectionBar, _>(bar, |b, _| b.on_click.add(Handler { owner: me, id: GN_BAR_CLICK }));
        }
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        self.init_event_bindings(cx);
        // (class default true, the main menu's instance false: UI_Homescreen_v2 then calls ToggleExitButton(true) on a PC)
        let a = prop(cx, "Exit Button Active by Default").as_bool().unwrap_or(true);
        self.toggle_exit_button(cx, a);
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        if !self.bound {
            return;
        }
        self.bound = false;
        let me = cx.w;
        cx.view.manager.modal_shown.remove(Handler { owner: me, id: GN_MODAL_SHOWN });
        cx.view.manager.modal_dismissed.remove(Handler { owner: me, id: GN_MODAL_DISMISSED });
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            GN_SETTINGS => self.on_settings(cx),
            GN_EXIT => self.on_exit(cx),
            GN_BAR_CLICK => {
                if let Some((_, sid)) = arg.downcast_ref::<(i32, String)>() {
                    self.show_nav_modal(cx, sid);
                }
            }
            GN_MODAL_SHOWN => self.refresh_context(cx),
            GN_MODAL_DISMISSED => {
                if let Some(m) = arg.downcast_ref::<String>().filter(|m| NAV_MODALS.contains(&m.as_str())) {
                    let bar = Self::section_bar(cx);
                    if let Some(b) = cx.visit::<SectionBar, _>(bar, |b, _| b.button_by_id(m)).flatten() {
                        bar_button(cx, b, |b, cx| b.set_on(cx, false));
                    }
                }
            }
            _ => {}
        }
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        if id == GN_FOCUS {
            self.refresh_context(cx);
            let ctx = if self.focus_ctx != NULL_GUID { self.focus_ctx.clone() } else { self.main_context.clone() };
            Manager::set_current_context(cx, 0, &ctx, true);
            let m = std::mem::take(&mut self.focus_modal);
            self.update_focus(cx, &m);
        }
    }
}

/// A controller widget's array property (empty for none).
fn prop_of(cx: &Cx<'_>, w: WidgetId, k: &str) -> Vec<Value> {
    cx.view.tree().props(w).get(k).and_then(Value::as_array).cloned().unwrap_or_default()
}
