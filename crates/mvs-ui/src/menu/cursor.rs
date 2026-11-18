//! UI_Cursor of player 0: the game's own pointer picture, the left stick's movement of the
//! pointer, and what the cursor keeps of the interactor under it. UI_Global makes one per player at its start; the
//! port has one local player and a manager per menu view, so each view that takes input has one
//! (`MenuView::enable_cursor`). Its widget is no layer: the view paints it over everything at the pointer.
//!
//! OffsetView: MousePosNormalized (`Pointer::mouse_pos_normalized`, what BP_Banner_PM's parallax reads), MPC_MousePOS
//! (`Pointer::mpc_mouse_pos`, what M_ProgressionParallax reads) and the Parallaxers' RenderTranslation (@993-2660;
//! UI_ParallaxItems and WBP_GameModeCard fill the list). Not ported: Clear Context from the manager's DismissModal,
//! the cursors of players 1 to 3.

use std::any::Any;

use serde_json::Value;

use super::controller::{Controller, Cx, Handler};
use super::manager::{Manager, TransitionParams};
use crate::slate::{Vis, WidgetId};

/// FMath::FInterpTo: `current` toward `target` by the fraction dt x speed of the distance (clamped to 1); the target
/// itself when the squared distance is below SMALL_NUMBER (1e-8), or at a speed of 0 or below.
fn f_interp_to(current: f64, target: f64, dt: f64, speed: f64) -> f64 {
    if speed <= 0.0 {
        return target;
    }
    let dist = target - current;
    if dist * dist < 1e-8 {
        return target;
    }
    current + dist * (dt * speed).clamp(0.0, 1.0)
}

/// Class defaults (UI_Cursor's): velocity, frictionMultiplier, defaultFriction.
const VELOCITY: f64 = 1100.0;
const FRICTION_MULTIPLIER: f64 = 0.5;
const DEFAULT_FRICTION: f64 = 0.9;
/// VisibilityCheckLoop's interval (Tick @3447).
const VISIBILITY_INTERVAL: f64 = 0.25;
/// OnInitialized: Delay(0.1) @2084, then Init.
const INIT_DELAY: f64 = 0.1;
/// ProcessMovementDirection: the pointer stays this many pixels inside each viewport edge.
const EDGE: f64 = 10.0;
/// (the port's own) the hide fade Lerp(opacity, 0, dt * 10) never reaches 0; under a step of an 8-bit alpha (the
/// circle's fill is 0.25 of it) nothing of it shows.
const OPACITY_GONE: f64 = 1.0 / 1024.0;
/// Class defaults: Parallax Speed, shiftMultiplier.
const PARALLAX_SPEED: f64 = 5.0;
const SHIFT_MULTIPLIER: f64 = 1.0;
/// The box the picture is laid out in, around the pointer (the widget tree's units): wider than the circle at its
/// hover scale (75 x 1.5) and than the arrow, which hangs right and down of the centre (64).
pub const BOX: f64 = 160.0;

const ON_HOVER: u32 = 1;
const ON_TRANSITION: u32 = 2;

/// UKismetMathLibrary::Round.
fn round(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// UI_ParallaxItemStruct: a widget OffsetView moves with the pointer, by ParallaxMultiplier. `owner`: the user widget
/// whose Destruct ends the entry.
/// Unverified: the build drops an entry once IsValid(Widget) fails, after the garbage collection that follows the
/// widget's removal; the port drops it at the next OffsetView after its owner's Destruct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parallaxer {
    pub widget: WidgetId,
    pub multiplier: i32,
    pub owner: WidgetId,
}

/// UI_Cursor's state.
#[derive(Debug)]
pub struct Cursor {
    /// currentlyHovered, lastValidHovered, isHovering, interactorFriction.
    pub currently_hovered: Option<WidgetId>,
    last_valid_hovered: Option<WidgetId>,
    is_hovering: bool,
    interactor_friction: f64,
    /// currentlyVisible (VisibilityCheckLoop), isTransitioningOut (OnTransitionStarted).
    pub currently_visible: bool,
    is_transitioning_out: bool,
    /// Current Direction (SmoothMovementDirection), SavedPositionX / Y.
    dir: [f64; 2],
    saved: [f64; 2],
    visibility_timer: f64,
    init_in: f64,
    inited: bool,
    /// The widget's render opacity (OnInitialized @2139: 0).
    pub opacity: f64,
    /// Parallaxers.
    pub parallaxers: Vec<Parallaxer>,
    w: WidgetId,
    scale: WidgetId,
    circle: WidgetId,
    arrow: WidgetId,
}

impl Cursor {
    /// The cursor of the context's widget (a UI_Cursor tree): its own material instances for Fill and Outline
    /// (the Hover animation changes their colours), PlayerIndicator hidden (OnInitialized @2024: Show Number is false
    /// in the class defaults).
    pub fn new(cx: &mut Cx<'_>) -> Cursor {
        let w = cx.w;
        let t = cx.view.tree_mut();
        let scale = t.find_in(w, "ScaleContainer");
        let circle = t.find_in(w, "StyleCircle");
        let arrow = t.find_in(w, "StyleArrow");
        let fill = t.find_in(w, "Fill");
        let outline = t.find_in(w, "Outline");
        t.dynamic_material(fill);
        t.dynamic_material(outline);
        let ind = t.find_in(w, "PlayerIndicator");
        t.widget_mut(ind).vis = Vis::Hidden;
        Cursor {
            currently_hovered: None,
            last_valid_hovered: None,
            is_hovering: false,
            interactor_friction: 0.0,
            currently_visible: false,
            is_transitioning_out: false,
            dir: [0.0; 2],
            saved: [0.0; 2],
            visibility_timer: 0.0,
            init_in: INIT_DELAY,
            inited: false,
            opacity: 0.0,
            parallaxers: Vec::new(),
            w,
            scale,
            circle,
            arrow,
        }
    }

    /// Init (3547), 0.1 s after OnInitialized: the pointer at the viewport centre when nothing placed it yet (the
    /// port's views are one per screen; the build's cursor outlives the levels), the manager's delegates bound,
    /// HoverChange(None, false).
    fn init(&mut self, cx: &mut Cx<'_>) {
        if cx.view.mouse.is_none() {
            let [w, h] = cx.view.size();
            cx.view.set_position((f64::from(w) / 2.0).trunc(), (f64::from(h) / 2.0).trunc());
        }
        let me = self.w;
        let m = &mut cx.view.manager;
        m.allow_cursor_mode = true;
        m.on_interactor_hover.add(Handler { owner: me, id: ON_HOVER });
        m.transition_started.add(Handler { owner: me, id: ON_TRANSITION });
        self.hover_change(cx, None, false, 0);
        self.is_hovering = false;
        self.inited = true;
    }

    /// Destruct (3552): ClearReferences (Parallaxers emptied @6564), the bindings removed.
    pub fn remove(&mut self, cx: &mut Cx<'_>) {
        self.currently_hovered = None;
        self.last_valid_hovered = None;
        self.parallaxers.clear();
        let me = self.w;
        let m = &mut cx.view.manager;
        m.on_interactor_hover.remove_owner(me);
        m.transition_started.remove_owner(me);
        if m.cursor == Some(me) {
            m.cursor = None;
            m.cursor_hovered = None;
        }
        cx.view.stop_all_animations(me);
    }

    /// HoverChange(Interactor, Hovered, Player), bound to Manager.OnInteractorHover: only for the cursor's player.
    fn hover_change(&mut self, cx: &mut Cx<'_>, i: Option<WidgetId>, hovered: bool, p: i32) {
        if p != 0 {
            return;
        }
        if self.currently_hovered.is_some() {
            self.last_valid_hovered = self.currently_hovered;
        }
        self.currently_hovered = if hovered { i } else { None };
        cx.view.manager.cursor_hovered = self.currently_hovered;
        self.is_hovering = hovered;
        // valid, enabled, not NoFriction and navigable @462; otherwise not hovering and no friction @541
        let ok = i.is_some_and(|i| {
            let no_friction = cx.view.tree().props(i).get("NoFriction").is_some_and(truthy);
            cx.view.has_controller(i)
                && cx.view.manager.interactor(i).is_some_and(|s| s.is_enabled() && s.navigable)
                && !no_friction
        });
        if ok {
            self.interactor_friction = if self.is_hovering { DEFAULT_FRICTION } else { 0.0 };
        } else {
            self.is_hovering = false;
            self.interactor_friction = 0.0;
        }
        // StopAnimation(Hover) @576, PlayAnimation(Hover, 0, 1, Forward or Reverse, 1.0) @654. Unverified: the
        // rate is taken at the play, not followed when the pointer is shown or hidden during the animation.
        let speed = if self.drawn_as_cursor(cx) { 2.0 } else { 1.0 };
        cx.view.stop_all_animations(self.w);
        cx.view.play(self.w, "Hover", !self.is_hovering, speed);
    }

    /// Tick (2712), every frame of an active screen, shown or not. Native: the build paints UI_Cursor twice a frame
    /// (the viewport child and the software cursor) and SWidget::Paint ticks on every call: the whole Tick runs
    /// twice while the cursor is drawn.
    pub fn tick(&mut self, cx: &mut Cx<'_>, dt: f64) {
        if !self.inited {
            self.init_in -= dt;
            if self.init_in > 0.0 {
                return;
            }
            self.init(cx);
        }
        let passes = if self.drawn_as_cursor(cx) { 2 } else { 1 };
        for _ in 0..passes {
            self.tick_once(cx, dt);
        }
    }

    /// DrawCursor paints the widget when the pointer is shown (bShowMouseCursor, or a widget under it asks for a
    /// cursor); its opacity does not count.
    fn drawn_as_cursor(&self, cx: &Cx<'_>) -> bool {
        cx.view.mouse.is_some() && (cx.view.show_mouse_cursor || cx.view.over_cursor_widget)
    }

    fn tick_once(&mut self, cx: &mut Cx<'_>, dt: f64) {
        // ScaleContainer.SetRenderScale(GetViewportScale) @2712-2824
        let dpi = cx.view.dpi;
        cx.view.tree_mut().widget_mut(self.scale).transform.scale = [dpi, dpi];
        if self.currently_visible {
            self.opacity = 1.0;
            self.process_movement_direction(cx, dt);
            self.offset_view(cx, dt);
            // SetStyle by the manager's LastInteractionType: 0 the circle, 1 and 2 the arrow @3034-3221
            let arrow = cx.view.manager.last_interaction_type[0] != 0;
            let t = cx.view.tree_mut();
            t.widget_mut(self.circle).vis = if arrow { Vis::Hidden } else { Vis::Visible };
            t.widget_mut(self.arrow).vis = if arrow { Vis::Visible } else { Vis::Hidden };
        } else if self.opacity > 0.0 {
            // SetRenderOpacity(Lerp(current, 0, dt * 10)) @3306-3427
            self.opacity += (0.0 - self.opacity) * dt * 10.0;
            if self.opacity < OPACITY_GONE {
                self.opacity = 0.0;
            }
        }
        // TimeIntervalCheck(VisibilityCheckTimer, 0.25) @3447
        let now = cx.view.manager.internal_time;
        if now - self.visibility_timer >= VISIBILITY_INTERVAL {
            self.visibility_timer = now;
            self.visibility_check_loop(cx);
        }
    }

    /// AddParallaxItem(Item): Array_AddUnique.
    pub fn add_parallax_item(&mut self, p: Parallaxer) {
        if !self.parallaxers.contains(&p) {
            self.parallaxers.push(p);
        }
    }

    /// WBP_GameModeCard's loop over UI Cursor.Parallaxers (@581-1212): every entry of the widget removed, then the
    /// item added (UI_ParallaxItems.AddItem).
    pub fn replace_parallax_item(&mut self, p: Parallaxer) {
        self.parallaxers.retain(|q| q.widget != p.widget);
        self.add_parallax_item(p);
    }

    /// OffsetView(dt) @3011: MousePosNormalized = clamp(the pointer over the viewport's pixels, 0, 1); MPC_MousePOS
    /// (@515-951): R, G FInterpTo(their value, MousePosNormalized, dt, 2), B, A MousePosNormalized; then the
    /// Parallaxers.
    /// Deviation: the build's OS cursor always has a position; with no pointer placed yet the port keeps the value
    /// it had.
    fn offset_view(&mut self, cx: &mut Cx<'_>, dt: f64) {
        let [w, h] = cx.view.size();
        if let Some(m) = cx.view.mouse
            && !cx.pointer.mouse_pos_pinned
            && w != 0
            && h != 0
        {
            cx.pointer.mouse_pos_normalized =
                [(m[0] / f64::from(w)).clamp(0.0, 1.0), (m[1] / f64::from(h)).clamp(0.0, 1.0)];
        }
        let n = cx.pointer.mouse_pos_normalized;
        let c = &mut cx.pointer.mpc_mouse_pos;
        *c = [f_interp_to(c[0], n[0], dt, 2.0), f_interp_to(c[1], n[1], dt, 2.0), n[0], n[1]];
        self.move_parallaxers(cx, dt);
    }

    /// OffsetView @993-2660, the Parallaxers last to first: an entry whose widget is gone removed (Array_RemoveItem:
    /// every equal entry); each other's RenderTranslation = VLerp(its translation, MapRangeClamped(the pointer over
    /// the viewport, 0..1 -> -1..1) x -shiftMultiplier x ParallaxMultiplier, dt x Parallax Speed). The pointer is
    /// read again here, unclamped (the map clamps); without one, MousePosNormalized.
    /// Unverified: the cursor writes after the widgets' animations each frame; where an animation moves a parallax
    /// item too (UI_PM_ReadyUp's IdleAnim on IdleIndicator / LockedIndicator), the build's order of the cursor's
    /// tick and the sequence player's evaluation is not read.
    fn move_parallaxers(&mut self, cx: &mut Cx<'_>, dt: f64) {
        let [w, h] = cx.view.size();
        let norm = match cx.view.mouse {
            Some(m) if w != 0 && h != 0 => [m[0] / f64::from(w), m[1] / f64::from(h)],
            _ => cx.pointer.mouse_pos_normalized,
        };
        let map = |v: f64| -1.0 + 2.0 * v.clamp(0.0, 1.0);
        let alpha = dt * PARALLAX_SPEED;
        let mut i = self.parallaxers.len();
        while i > 0 {
            i -= 1;
            let Some(&p) = self.parallaxers.get(i) else { continue };
            if cx.view.is_destroyed(p.owner) {
                self.parallaxers.retain(|q| *q != p);
                continue;
            }
            let k = -SHIFT_MULTIPLIER * f64::from(p.multiplier);
            let target = [map(norm[0]) * k, map(norm[1]) * k];
            let t = &mut cx.view.tree_mut().widget_mut(p.widget).transform.translation;
            *t = [t[0] + (target[0] - t[0]) * alpha, t[1] + (target[1] - t[1]) * alpha];
        }
    }

    /// VisibilityCheckLoop (4422).
    fn visibility_check_loop(&mut self, cx: &mut Cx<'_>) {
        // step 1, SetCursorToPlayer @4511: PC0.bShowMouseCursor = true again
        cx.view.show_cursor(true);
        // hover upkeep @4531-5403
        if self.is_hovering {
            let keep = self.currently_hovered.is_some_and(|h| {
                Manager::is_focusable(cx, h)
                    && Manager::is_in_current_context(cx, h, false, -1)
                    && cx.view.manager.get_currently_focused(0) == Some(h)
            });
            if !keep {
                self.hover_change(cx, None, false, 0);
            }
        } else {
            let l = self.last_valid_hovered;
            let again = l.is_some_and(|l| {
                Manager::is_focusable(cx, l)
                    && Manager::is_in_current_context(cx, l, false, -1)
                    && cx.view.manager.interactor(l).is_some_and(|s| s.is_hovered() && s.navigable)
            });
            match l {
                Some(l) if again => super::widgets::controls::interactor::force_cursor_hover(cx, l),
                _ => self.last_valid_hovered = None,
            }
        }
        // @5404-6329: cursor mode, not transitioning out, the manager not waiting and handling input. A menu screen of
        // the port that is active is one of the menu levels or the paused match.
        let m = &cx.view.manager;
        self.currently_visible = m.cursor_mode[0] && !self.is_transitioning_out && !m.is_waiting && m.handle_input;
    }

    /// ProcessMovementDirection(dt), while currentlyVisible.
    fn process_movement_direction(&mut self, cx: &mut Cx<'_>, dt: f64) {
        // SmoothMovementDirection @5-237: Current Direction = (input + Current Direction) / 2, once per Tick.
        // (the port's own) A tick of the build is 1 / 60 s; the half is taken per 60 Hz frame of the frame's length.
        let keep = 0.5f64.powf(dt * 60.0);
        let (mx, my) = (cx.view.manager.move_x[0], cx.view.manager.move_y[0]);
        self.dir[0] = mx + (self.dir[0] - mx) * keep;
        self.dir[1] = my + (self.dir[1] - my) * keep;
        let friction =
            if self.is_hovering { (FRICTION_MULTIPLIER * self.interactor_friction).clamp(0.0, 1.0) } else { 0.0 };
        // velocityPrefsMultiplier: FClamp(cursorSpeedMultiplier, 0.5, 1.5)
        let k = VELOCITY * (1.0 - friction) * dt * cx.prefs.ui.cursor_speed_multiplier.clamp(0.5, 1.5);
        let (ax, ay) = (self.dir[0] * k, self.dir[1] * k);
        // IsNearlyZero2D(MoveAmount, 0.0001) @667
        if ax.abs() <= 0.0001 && ay.abs() <= 0.0001 {
            return;
        }
        let [w, h] = cx.view.size();
        let dpi = cx.view.dpi;
        let base = cx.view.mouse.unwrap_or(self.saved);
        let x = round((base[0] + ax * dpi).clamp(EDGE, f64::from(w) - EDGE));
        let y = round((base[1] + ay * dpi).clamp(EDGE, f64::from(h) - EDGE));
        self.saved = [x, y];
        cx.view.set_position(x, y);
    }
}

/// A JSON value's truthiness (`!!v`).
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

impl Controller for Cursor {
    fn handle(&mut self, cx: &mut Cx<'_>, id: u32, arg: &dyn Any) {
        match id {
            ON_HOVER => {
                if let Some(&(i, hovered, p)) = arg.downcast_ref::<(WidgetId, bool, i32)>() {
                    self.hover_change(cx, Some(i), hovered, p);
                }
            }
            ON_TRANSITION => {
                if let Some(t) = arg.downcast_ref::<TransitionParams>() {
                    self.is_transitioning_out = t.kind == 0;
                }
            }
            _ => {}
        }
    }
}

/// UI_Functions.FindUICursor(0) and a call on it: `f` on the view's cursor, deferred while the cursor's own Tick
/// runs (its hover upkeep can focus a card that calls back here). Nothing without a cursor: the build's calls on an
/// invalid cursor do nothing.
pub fn with_cursor(cx: &mut Cx<'_>, f: impl FnOnce(&mut Cursor) + 'static) {
    let Some(w) = cx.view.cursor else { return };
    cx.run(
        w,
        Box::new(move |c, _| {
            let any: &mut dyn Any = c;
            if let Some(c) = any.downcast_mut::<Cursor>() {
                f(c);
            }
        }),
    );
}
