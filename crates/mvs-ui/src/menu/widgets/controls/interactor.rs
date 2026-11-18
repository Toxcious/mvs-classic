//! UI_Interactor and UI_FocusedCommon.
//!
//! An interactor's state is the manager's (`Manager::interactor`, `InteractorState`), not its controller's: the
//! manager reads and fires it while the interactor's own method runs (an interactor's SetFocused calls the
//! manager's, which fires its OnFocus), and a controller is out of the view while its method runs
//! (controller.rs). Its methods are functions of the context and its widget (`set_focused(cx, w, ..)`); its own
//! handlers (OnInitialized's OnFocus / OnUnfocus / OnClick bindings, which run before any other: a child
//! initialises first) are run by `fire_focus` / `fire_unfocus` / `fire_click` ahead of the events' handlers.

use std::any::Any;
use std::rc::Rc;

use serde_json::Value;

use crate::menu::controller::{Controller, Cx, Event, Handler};
use crate::menu::manager::{Manager, NULL_GUID};
use crate::menu::widgets::common::{Quadrilateral, sound};
use crate::slate::{Vis, WidgetId};

/// UI_TooltipParams as the port keeps it: the interactor's Tooltip Params, given to
/// the UI_TooltipTarget of its hitbox at each toggle.
#[derive(Clone, Default)]
pub struct TooltipParams {
    pub tooltip: String,
    /// The tip's class (None: none).
    pub cls: Option<String>,
    pub show: bool,
    pub offset: [f64; 2],
    pub delay: f64,
    /// EMenuPlacement.
    pub placement: u8,
    /// Data (UI_TooltipDataInterface): given to the tip by Set Data.
    pub data: Option<Rc<dyn Any>>,
}

impl std::fmt::Debug for TooltipParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TooltipParams")
            .field("tooltip", &self.tooltip)
            .field("cls", &self.cls)
            .field("show", &self.show)
            .finish()
    }
}

/// NO_TOOLTIP: the struct's defaults (no class, ShowTooltip false, placement CenteredBelowAnchor).
pub fn no_tooltip() -> TooltipParams {
    TooltipParams { placement: 1, ..TooltipParams::default() }
}

/// An interactor (UI_Interactor's variables and dispatchers).
#[derive(Debug)]
pub struct InteractorState {
    pub navigable: bool,
    /// The widget's centre on the viewport (device pixels), kept for the navigation.
    pub position: [f64; 2],
    pub size: [f64; 2],
    pub context: String,
    pub player_index: i32,
    pub all_players: bool,
    pub prevent_fallback_focus: bool,
    /// ManualLeft / Right / Up / Down.
    pub manual_left: Option<WidgetId>,
    pub manual_right: Option<WidgetId>,
    pub manual_up: Option<WidgetId>,
    pub manual_down: Option<WidgetId>,
    pub id: String,
    pub section_bar_id: String,
    /// BPI_SetScrollBox.
    pub scroll_box: Option<WidgetId>,
    /// SetInteractable's value (the interactor's own bIsEnabled).
    pub enabled: bool,
    /// An ancestor widget has bIsEnabled false: Slate disables the whole subtree, so the interactor takes no clicks
    /// or focus whatever SetInteractable says (set at registration and by `set_is_enabled`).
    pub inherited_off: bool,
    pub rate_limited: bool,
    pub rate_limit: f64,
    pub hover_sound: Option<String>,
    pub confirm_sound: Option<String>,
    pub last_hover_time: f64,
    pub is_focused: bool,
    /// The hitbox Button's hover state, kept by the view.
    pub hovered: bool,
    /// Tooltip Params: written by the owner.
    pub tooltip_params: TooltipParams,
    rate_timer: f64,
    submit_timer: f64,
    prepass_forced: bool,
    /// SubmitTimerInterval.
    submit_interval: f64,
    /// Normalized Position Override (None: unset or negative).
    position_override: Option<[f64; 2]>,
    pub on_focus: Event<i32>,
    pub on_unfocus: Event<i32>,
    pub on_click: Event<i32>,
    /// OnClickWithSource: the interactor's widget.
    pub on_click_with_source: Event<WidgetId>,
    pub on_interactable: Event<()>,
    pub on_not_interactable: Event<()>,
    pub on_rate_limit_reset: Event<()>,
}

impl InteractorState {
    /// The state from the instance's properties.
    pub fn of(p: &Value) -> InteractorState {
        let b = |k: &str, d: bool| p.get(k).and_then(Value::as_bool).unwrap_or(d);
        let n = |k: &str, d: f64| p.get(k).and_then(Value::as_f64).unwrap_or(d);
        let o = p.get("Normalized Position Override");
        let ox = o.and_then(|o| o.get("X")).and_then(Value::as_f64).unwrap_or(-1.0);
        let oy = o.and_then(|o| o.get("Y")).and_then(Value::as_f64).unwrap_or(-1.0);
        InteractorState {
            navigable: b("navigable", true),
            position: [0.0; 2],
            size: [0.0; 2],
            context: p.get("context").and_then(Value::as_str).unwrap_or(NULL_GUID).to_string(),
            player_index: n("Player Index", 0.0) as i32,
            all_players: b("AllPlayers", false),
            prevent_fallback_focus: b("Prevent Fallback Focus", false),
            manual_left: None,
            manual_right: None,
            manual_up: None,
            manual_down: None,
            id: String::new(),
            section_bar_id: String::new(),
            scroll_box: None,
            enabled: b("bIsEnabled", true),
            inherited_off: false,
            rate_limited: b("rateLimited", true),
            rate_limit: n("rateLimit", 0.0),
            hover_sound: sound(p.get("Hover Sound")),
            confirm_sound: sound(p.get("Confirm Sound")),
            last_hover_time: 0.0,
            is_focused: false,
            hovered: false,
            tooltip_params: no_tooltip(),
            rate_timer: -1.0,
            submit_timer: -1.0,
            prepass_forced: false,
            submit_interval: n("SubmitTimerInterval", 0.1),
            position_override: (ox >= 0.0 && oy >= 0.0).then_some([ox, oy]),
            on_focus: Event::default(),
            on_unfocus: Event::default(),
            on_click: Event::default(),
            on_click_with_source: Event::default(),
            on_interactable: Event::default(),
            on_not_interactable: Event::default(),
            on_rate_limit_reset: Event::default(),
        }
    }

    /// The player index as an index (`this.p`: at least 0).
    pub fn p(&self) -> usize {
        self.player_index.max(0) as usize
    }

    /// Enabled in Slate: its own flag and every ancestor's.
    pub fn is_enabled(&self) -> bool {
        self.enabled && !self.inherited_off
    }

    pub fn get_interactable(&self) -> bool {
        self.is_enabled() && !self.rate_limited
    }

    pub fn is_hovered(&self) -> bool {
        self.hovered
    }

    /// ManualLeft / Right / Up / Down.
    pub fn manual(&self) -> [Option<WidgetId>; 4] {
        [self.manual_left, self.manual_right, self.manual_up, self.manual_down]
    }
}

fn st<'a>(cx: &'a mut Cx<'_>, w: WidgetId) -> Option<&'a mut InteractorState> {
    cx.view.manager.interactor_mut(w)
}

/// A field of the interactor `w`'s state (default when it has none).
fn read<T: Default>(cx: &Cx<'_>, w: WidgetId, f: impl FnOnce(&InteractorState) -> T) -> T {
    cx.view.manager.interactor(w).map(f).unwrap_or_default()
}

/// SetInteractable.
pub fn set_interactable(cx: &mut Cx<'_>, w: WidgetId, b: bool) {
    let Some(s) = st(cx, w) else { return };
    let changed = b != s.enabled;
    s.enabled = b;
    if changed {
        let ev = if b { s.on_interactable.clone() } else { s.on_not_interactable.clone() };
        // (no handler of the port reads the flag, the order kept as the event's)
        ev.fire(cx, &());
    }
}

/// RegisterWithManager: registered, the own handlers bound, the latent chain started: the next frame the focused
/// interactor re-broadcasts OnFocus, 0.2 s later the position is taken and the rate limit of the class default
/// ends. Unverified: the resume points of the chain's delays are not traceable in the bytecode.
pub fn register_with_manager(cx: &mut Cx<'_>, w: WidgetId) {
    let off = ancestor_off(cx, w);
    if let Some(s) = st(cx, w) {
        s.inherited_off = off;
    }
    cx.view.manager.register_interactor(w);
    cx.view.delay(0.0, Handler { owner: w, id: RESUME_REFOCUS });
}

/// An ancestor of `w` has bIsEnabled false (the build's UWidget::SetIsEnabled reaches the subtree through Slate's
/// IsEnabled chain).
fn ancestor_off(cx: &Cx<'_>, w: WidgetId) -> bool {
    let t = cx.view.tree();
    let mut at = t.widget(w).parent;
    while let Some(p) = at {
        if t.props(p).get("bIsEnabled").and_then(Value::as_bool) == Some(false) {
            return true;
        }
        at = t.widget(p).parent;
    }
    false
}

/// UWidget::SetIsEnabled on `w`, an ancestor of interactors: its bIsEnabled written, and every interactor below it
/// enabled or disabled with it in Slate. Their own Interactable and its events are untouched (SetIsEnabled is not
/// SetInteractable: a UI_ButtonMain keeps its look).
pub fn set_is_enabled(cx: &mut Cx<'_>, w: WidgetId, on: bool) {
    cx.view.tree_mut().set_prop(w, "bIsEnabled", Value::Bool(on));
    let mut stack = cx.view.tree().widget(w).children.clone();
    while let Some(i) = stack.pop() {
        stack.extend(cx.view.tree().widget(i).children.iter().copied());
        if cx.view.manager.interactor(i).is_none() {
            continue;
        }
        let off = ancestor_off(cx, i);
        if let Some(s) = st(cx, i) {
            s.inherited_off = off;
        }
    }
}

/// The interactor controller's resume ids (its delays).
const RESUME_REFOCUS: u32 = 1;
const RESUME_POSITION: u32 = 2;

/// Toggle Tooltip @2825: the params go to the UI_TooltipTarget in the hitbox, which opens or closes the tip.
/// It follows the focus: shown at OnFocus (@3071), closed at OnUnfocus (@3332), at a click before OnClick (@3348)
/// and at Destruct. Deviation: the close when the interactor turns Collapsed / Hidden (OnVisibility @6062) is not
/// ported, the port has no visibility events; DelayedShowTooltip (@5896) has no caller in the port's widgets.
pub fn toggle_tooltip(cx: &mut Cx<'_>, w: WidgetId, show: bool) {
    let Some(s) = cx.view.manager.interactor(w) else { return };
    let params = s.tooltip_params.clone();
    let player = s.player_index;
    let Some(target) = cx.view.tree().find_opt(w, "UI_TooltipTarget") else { return };
    cx.visit_dyn(target, |c, cx| c.tooltip_target(cx, &params, player, show));
}

/// The rate limit's timer (the controller's timers).
fn timers(cx: &mut Cx<'_>, w: WidgetId, dt: f64) {
    let Some(s) = st(cx, w) else { return };
    if s.rate_timer < 0.0 {
        return;
    }
    s.rate_timer -= dt;
    if s.rate_timer <= 0.0 {
        clear_rate_limit(cx, w);
    }
}

fn trigger_rate_limit(s: &mut InteractorState) {
    if s.rate_limit > 0.0 {
        s.rate_limited = true;
        s.rate_timer = s.rate_limit;
    }
}

fn clear_rate_limit(cx: &mut Cx<'_>, w: WidgetId) {
    let Some(s) = st(cx, w) else { return };
    s.rate_timer = -1.0;
    if s.rate_limited {
        s.rate_limited = false;
        let ev = s.on_rate_limit_reset.clone();
        ev.fire(cx, &());
    }
}

/// OnFocus fired for player `p`: the interactor's own onFocusInternal, then the event's handlers.
pub fn fire_focus(cx: &mut Cx<'_>, w: WidgetId, p: i32) {
    let Some(s) = st(cx, w) else { return };
    let sound = if !s.is_focused { s.hover_sound.clone() } else { None };
    s.is_focused = true;
    let sb = s.scroll_box;
    let pi = s.p();
    if let Some(e) = sound {
        cx.post(&e);
    }
    cx.fire_manager(|m| &m.on_interactor_focused, &w);
    toggle_tooltip(cx, w, true);
    // @3240 ScrollBox.ScrollWidgetIntoView(self, animate true, ScrollDestination): the widget's centre goes to the
    // centre of the view, animated (Unverified: by the last layout, SScrollBox is not decoded)
    if let Some(sb) = sb
        && !cx.view.manager.cursor_mode[pi]
    {
        cx.view.scroll_into_view_centre(sb, w, true);
    }
    let ev = read(cx, w, |s| s.on_focus.clone());
    ev.fire(cx, &p);
}

/// OnUnfocus fired for player `p`: onUnfocusInternal, then the event's handlers.
pub fn fire_unfocus(cx: &mut Cx<'_>, w: WidgetId, p: i32) {
    let Some(s) = st(cx, w) else { return };
    s.is_focused = false;
    toggle_tooltip(cx, w, false);
    let ev = read(cx, w, |s| s.on_unfocus.clone());
    ev.fire(cx, &p);
}

/// OnClick fired for player `p`: onClickInternal (the confirm sound), then the event's handlers.
fn fire_click(cx: &mut Cx<'_>, w: WidgetId, p: i32) {
    if let Some(e) = read(cx, w, |s| s.confirm_sound.clone()) {
        cx.post(&e);
    }
    let ev = read(cx, w, |s| s.on_click.clone());
    ev.fire(cx, &p);
}

/// SetContext: the context and a new registration.
pub fn set_context(cx: &mut Cx<'_>, w: WidgetId, ctx: &str) {
    let Some(s) = st(cx, w) else { return };
    s.context = ctx.to_string();
    register_with_manager(cx, w);
}

/// UpdatePosition: the first call only measures the widget; later ones store the position: the viewport size
/// times Normalized Position Override when that is set, else GetWidgetPosition's Absolute, the point
/// (GetLocalTopLeft + LocalSize / 2) x ViewportScale taken as a local point of the widget (LocalToAbsolute;
/// device pixels).
pub fn update_position(cx: &mut Cx<'_>, w: WidgetId) {
    let [x, y, ww, wh] = cx.view.tree().arranged(w);
    let vs = cx.view.size();
    let dpi = cx.view.dpi;
    let pm = cx.view.tree().painted(w).map(|p| p.transform);
    let Some(s) = st(cx, w) else { return };
    if !s.prepass_forced {
        s.size = [ww, wh];
        s.prepass_forced = true;
        return;
    }
    if let Some([ox, oy]) = s.position_override {
        s.position = [f64::from(vs[0]) * ox, f64::from(vs[1]) * oy];
        return;
    }
    let Some(m) = pm else { return };
    let px = (x + ww / 2.0) * dpi;
    let py = (y + wh / 2.0) * dpi;
    s.position = [m[0] * px + m[2] * py + m[4], m[1] * px + m[3] * py + m[5]];
}

/// SetFocused(context only). The tip: with a real mouse the build only closes it here (@751); without one it is
/// closed and shown again after the manager's SetFocused (@1287, @1302).
pub fn set_focused(cx: &mut Cx<'_>, w: WidgetId, context_only: bool) {
    let Some(s) = cx.view.manager.interactor(w) else { return };
    let (p, pi, ctx) = (s.p(), s.player_index, s.context.clone());
    Manager::unfocus_all(cx, p as i32);
    if !context_only && !cx.view.manager.cursor_mode[p] {
        let mouse = cx.view.manager.real_mouse_active(p);
        Manager::set_focused(cx, w);
        toggle_tooltip(cx, w, false);
        if !mouse {
            toggle_tooltip(cx, w, true);
        }
    }
    Manager::set_current_context(cx, p as i32, &ctx, pi == 0);
}

/// Submit(player).
pub fn submit(cx: &mut Cx<'_>, w: WidgetId, player: i32) {
    let now = cx.now();
    let Some(s) = st(cx, w) else { return };
    let d = now - s.submit_timer;
    if !(d >= s.submit_interval || d < 0.0) {
        return;
    }
    s.submit_timer = now;
    let ctx = s.context.clone();
    let m = &cx.view.manager;
    let ok = Manager::is_in_current_context(cx, w, false, -1)
        && Manager::is_focusable(cx, w)
        && read(cx, w, InteractorState::get_interactable)
        && ctx != m.ignore_context;
    if !ok || !read(cx, w, |s| s.is_focused) {
        return;
    }
    if let Some(s) = st(cx, w) {
        trigger_rate_limit(s);
    }
    toggle_tooltip(cx, w, false);
    fire_click(cx, w, player);
    let ev = read(cx, w, |s| s.on_click_with_source.clone());
    ev.fire(cx, &w);
}

/// The hitbox Button's OnHovered.
pub fn on_hovered(cx: &mut Cx<'_>, w: WidgetId) {
    let t = cx.view.manager.internal_time;
    cx.view.manager.last_hover_time = t;
    if let Some(s) = st(cx, w) {
        s.last_hover_time = t;
    }
    force_cursor_hover(cx, w);
}

/// ForceCursorHover (also called by UI_Cursor's VisibilityCheckLoop for the interactor it last hovered).
pub fn force_cursor_hover(cx: &mut Cx<'_>, w: WidgetId) {
    let Some(s) = cx.view.manager.interactor(w) else { return };
    let (p, ctx) = (s.p(), s.context.clone());
    let m = &cx.view.manager;
    if !(m.input_type[p] == 2 || m.cursor_mode[p]) {
        return;
    }
    cx.fire_manager(|m| &m.on_interactor_hover, &(w, true, p as i32));
    if Manager::is_in_current_context(cx, w, true, -1) && Manager::is_focusable(cx, w) {
        Manager::set_focused(cx, w);
        Manager::set_current_context(cx, p as i32, &ctx, false);
    }
}

/// The hitbox Button's OnUnhovered.
pub fn on_unhovered(cx: &mut Cx<'_>, w: WidgetId) {
    let Some(s) = cx.view.manager.interactor(w) else { return };
    let p = s.p();
    let m = &cx.view.manager;
    if !(m.input_type[p] == 2 || m.cursor_mode[p]) {
        return;
    }
    cx.fire_manager(|m| &m.on_interactor_hover, &(w, false, p as i32));
    if cx.view.manager.cursor_mode[p] {
        fire_unfocus(cx, w, p as i32);
        Manager::unfocus_all(cx, p as i32);
    }
}

/// ManuallyCheckMouseHover.
pub fn manually_check_mouse_hover(cx: &mut Cx<'_>, w: WidgetId) {
    let m = &cx.view.manager;
    let Some(s) = m.interactor(w) else { return };
    if s.hovered
        && s.player_index <= 0
        && s.last_hover_time >= m.last_hover_time
        && (m.real_mouse_active(0) || m.cursor_mode[0])
        && m.get_currently_focused(0) != Some(w)
    {
        force_cursor_hover(cx, w);
    }
}

/// BatchSetContext.
pub fn batch_set_context(cx: &mut Cx<'_>, list: &[WidgetId], ctx: &str) {
    for &i in list {
        set_context(cx, i, ctx);
    }
}

/// BatchSetScrollBox.
pub fn batch_set_scroll_box(cx: &mut Cx<'_>, list: &[WidgetId], sb: WidgetId) {
    for &i in list {
        if let Some(s) = st(cx, i) {
            s.scroll_box = Some(sb);
        }
    }
}

/// UI_Interactor's controller: its state is the manager's (made at OnInitialized).
#[derive(Debug, Default)]
pub struct InteractorCtl;

impl InteractorCtl {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(InteractorCtl)
    }
}

impl Controller for InteractorCtl {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let s = InteractorState::of(cx.view.tree().props(cx.w));
        cx.view.manager.add_interactor_state(cx.w, s);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        register_with_manager(cx, cx.w);
    }

    /// Destruct @964.
    fn destruct(&mut self, cx: &mut Cx<'_>) {
        Manager::unregister_interactor(cx, cx.w);
        toggle_tooltip(cx, cx.w, false);
        clear_rate_limit(cx, cx.w);
    }

    fn timers(&mut self, cx: &mut Cx<'_>, dt: f64) {
        timers(cx, cx.w, dt);
    }

    fn resume(&mut self, cx: &mut Cx<'_>, id: u32) {
        let w = cx.w;
        match id {
            RESUME_REFOCUS => {
                let p = read(cx, w, InteractorState::p);
                if cx.view.manager.get_currently_focused(p as i32) == Some(w) {
                    fire_focus(cx, w, p as i32);
                }
                cx.view.delay(0.2, Handler { owner: w, id: RESUME_POSITION });
            }
            RESUME_POSITION => {
                update_position(cx, w);
                if let Some(s) = st(cx, w) {
                    s.rate_limited = false;
                }
            }
            _ => {}
        }
    }
}

/// UI_FocusedCommon: the Wobbler shown while one of its interactors is focused.
#[derive(Debug, Default)]
pub struct FocusedCommon {
    interactors: Vec<WidgetId>,
}

const FC_FOCUS: u32 = 1;
const FC_UNFOCUS: u32 = 2;
const FC_NOT_INTERACTABLE: u32 = 3;

impl FocusedCommon {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(FocusedCommon::default())
    }

    fn wobbler(cx: &Cx<'_>) -> WidgetId {
        cx.view.tree().find_in(cx.w, "Wobbler")
    }

    pub fn set_interactor(&mut self, cx: &mut Cx<'_>, i: WidgetId) {
        self.set_interactors(cx, &[i]);
    }

    pub fn set_interactors(&mut self, cx: &mut Cx<'_>, list: &[WidgetId]) {
        self.unbind(cx);
        self.interactors = list.to_vec();
        let me = cx.w;
        for &i in list {
            if let Some(s) = st(cx, i) {
                s.on_focus.add(Handler { owner: me, id: FC_FOCUS });
                s.on_unfocus.add(Handler { owner: me, id: FC_UNFOCUS });
                s.on_not_interactable.add(Handler { owner: me, id: FC_NOT_INTERACTABLE });
            }
        }
    }

    fn unbind(&mut self, cx: &mut Cx<'_>) {
        let me = cx.w;
        for &i in &self.interactors {
            if let Some(s) = st(cx, i) {
                s.on_focus.remove(Handler { owner: me, id: FC_FOCUS });
                s.on_unfocus.remove(Handler { owner: me, id: FC_UNFOCUS });
                s.on_not_interactable.remove(Handler { owner: me, id: FC_NOT_INTERACTABLE });
            }
        }
    }

    fn off(cx: &mut Cx<'_>) {
        let wob = Self::wobbler(cx);
        cx.visit::<Quadrilateral, _>(wob, |q, cx| q.wobble(cx, false));
        cx.view.tree_mut().widget_mut(wob).vis = Vis::Hidden;
    }

    /// The Focus custom event.
    pub fn focus(&mut self, cx: &mut Cx<'_>) {
        Self::on(cx);
    }

    fn on(cx: &mut Cx<'_>) {
        let wob = Self::wobbler(cx);
        let on = cx.view.tree().props(cx.w).get("IsWobbling").and_then(Value::as_bool).unwrap_or(false);
        cx.visit::<Quadrilateral, _>(wob, |q, cx| q.wobble(cx, on));
        cx.view.tree_mut().widget_mut(wob).vis = Vis::SelfHitTestInvisible;
    }

    fn any(&self, cx: &Cx<'_>, f: fn(&InteractorState) -> bool) -> bool {
        self.interactors.iter().any(|&i| cx.view.manager.interactor(i).is_some_and(f))
    }
}

impl Controller for FocusedCommon {
    fn pre_construct(&mut self, cx: &mut Cx<'_>) {
        let wob = Self::wobbler(cx);
        cx.view.tree_mut().widget_mut(wob).vis = Vis::Hidden;
        let sharp = cx.view.tree().props(cx.w).get("Sharpness").and_then(Value::as_f64).unwrap_or(0.0);
        cx.visit::<Quadrilateral, _>(wob, |q, cx| q.update_sharpness(cx, sharp));
    }

    fn destruct(&mut self, cx: &mut Cx<'_>) {
        self.unbind(cx);
    }

    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        match id {
            FC_FOCUS if self.any(cx, |s| s.is_focused) => Self::on(cx),
            FC_UNFOCUS if !self.any(cx, |s| s.is_focused) => Self::off(cx),
            FC_NOT_INTERACTABLE if !self.any(cx, InteractorState::get_interactable) => Self::off(cx),
            _ => {}
        }
    }
}
