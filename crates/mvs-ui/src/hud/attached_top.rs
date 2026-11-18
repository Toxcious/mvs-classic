//! BaseFighter's "Attached UI v3 Top" widget component: UI_IGv3_AttachedTop with its state
//! icons (UI_IGv3_States, UI_IGv3_State) and the stun bar (UI_IGv3_Struggle), driven by the broker calls.

use crate::bundle::Rgba;
use crate::element::ElementList;
use crate::slate::{Tree, Vis, WidgetId};
use crate::text::Fonts;

use super::Res;
use super::attached::{DIGIT, draw_box};
use super::common::{BrokerPlayer, c4, clamp01, js_num, lerp_color, shown};
use super::input_label::{LabelSources, PlacedInputLabel};
use super::pop::{Place, PopId, PopParams, PopTexts};

/// UI_StateParams as the feeders send them (Comp_Armor, Buff_Weakened, Buff_SlowDown), their names as text.
#[derive(Clone, Debug, PartialEq)]
pub struct StateParams {
    pub id: String,
    pub state_type: i32,
    pub display_type: i32,
    pub current_value: f64,
    pub max_value: f64,
    pub icon_material: String,
    pub active: bool,
    pub pop_text: String,
    pub auto_remove_time: f64,
    pub subicon: Option<String>,
}

const STATE_INACTIVE: Rgba = [0.052861, 0.104616, 0.147027, 1.0];

// ---------- UI_IGv3_State / UI_IGv3_States ----------

#[derive(Debug)]
struct State {
    w: WidgetId,
    params: Option<StateParams>,
    icon: WidgetId,
    overlay: WidgetId,
    /// The icon has its material (the dynamic instance made).
    mat: bool,
    color: Rgba,
    /// Game time of the FillColor fade loop (Delay(0) per frame), -1 when stopped.
    alpha_start: f64,
    /// Real time.
    pulse_start: f64,
    enter_start: f64,
    /// Pop Text.
    pop: Option<PopId>,
    /// Its viewport position, pixels.
    pop_at: Option<[f64; 2]>,
    /// Auto Remove Timer (game time).
    auto_remove_at: f64,
    /// Real time of the last tick.
    last_tick: f64,
    /// The icon's visibility in the widget tree.
    icon_vis: Vis,
}

impl State {
    fn visible(&self, t: &Tree) -> bool {
        shown(t.widget(self.w).vis)
    }

    /// PositionPopText: SetPositionInViewport(GetWidgetCenterPosition(self).ViewportPosition + (0, 75), false), in
    /// Slate units: 75 x DPI pixels below the widget's centre. GetWidgetCenterPosition (exe 14121fa10) reads the
    /// cached geometry, the last paint's: the pop text sits where the widget was painted the frame before. A widget
    /// never painted keeps the pop text where it is.
    fn position_pop_text(&mut self, t: &Tree, dpi: Option<f64>) {
        let (Some(dpi), Some(m)) = (dpi, t.painted(self.w)) else { return };
        let (cx, cy) = (m.size[0] / 2.0, m.size[1] / 2.0);
        let x = m.transform;
        self.pop_at = Some([x[0] * cx + x[2] * cy + x[4], x[1] * cx + x[3] * cy + x[5] + 75.0 * dpi]);
    }

    fn state_color(&self) -> Rgba {
        if self.params.as_ref().is_some_and(|p| p.active) { self.color } else { STATE_INACTIVE }
    }

    /// A fresh widget: no params, and none of the old one's loops, timers or animations running.
    fn clear(&mut self) {
        self.params = None;
        self.auto_remove_at = f64::INFINITY;
        self.alpha_start = -1.0;
        self.pulse_start = -1.0;
        self.enter_start = -1.0;
        self.last_tick = -1.0;
    }

    /// SetParams
    #[allow(clippy::too_many_arguments)]
    fn set_params(
        &mut self,
        t: &mut Tree,
        res: &Res,
        p: StateParams,
        now: f64,
        real: f64,
        pops: &mut PopTexts,
        fighter: usize,
        order: &mut u64,
        dpi: Option<f64>,
    ) {
        let w = self.w;
        let set = |t: &mut Tree, n: &str, v: Vis| {
            let x = t.find_in(w, n);
            t.widget_mut(x).vis = v;
        };
        // Labels by DisplayType, then hidden when the values are negative.
        let d = p.display_type.clamp(0, 3) as usize;
        use Vis::{Collapsed as C, Visible as V};
        set(t, "multiplierLabel", [C, V, V, C][d]);
        set(t, "valueLabel", [V, V, C, C][d]);
        set(t, "denominatorLabel", [V, C, C, C][d]);
        set(t, "subicon", [C, C, C, V][d]);
        let vl = t.find_in(w, "valueLabel");
        t.widget_mut(vl).text = js_num(p.current_value);
        let dl = t.find_in(w, "denominatorLabel");
        t.widget_mut(dl).text = format!("/{}", js_num(p.max_value));
        self.pulse_start = real;
        if p.current_value < 0.0 {
            for n in ["valueLabel", "multiplierLabel", "denominatorLabel"] {
                set(t, n, C);
            }
        }
        if p.max_value < 0.0 {
            set(t, "denominatorLabel", C);
        }
        let s = if p.state_type == 2 { 0.8 } else { 1.0 };
        t.widget_mut(w).transform.scale = [s, s];
        // Conditional Visibility(timerContainer, Timer > 0, Hidden, Hidden).
        set(t, "timerContainer", Vis::Hidden);
        // SetBrushFromMaterial + GetDynamicMaterial: a fresh instance of the icon material.
        // (the port's own) A material the hud bundle lacks: the state is shown without its icon.
        t.widget_mut(self.icon).mid = None;
        if let Some(path) = res.brush(&p.icon_material) {
            let i = t.widget_mut(self.icon);
            i.tex = Some(path);
            i.vis = self.icon_vis;
            self.mat = true;
            let m = t.dynamic_material(self.icon);
            m.set_vector("FillColor", c4(self.color));
            m.set_vector("BackgroundColor", [1.0, 1.0, 1.0, 0.6]);
            if !p.active {
                m.set_vector("FillColor", c4(STATE_INACTIVE));
            }
            let pct = if p.max_value > 0.0 { clamp01(p.current_value / p.max_value) } else { 1.0 };
            m.set_scalar("Percentage", pct as f32);
        } else {
            self.mat = false;
            t.widget_mut(self.icon).vis = Vis::Hidden;
        }
        let size = if p.id == "SlowDown" { 90.0 } else { 70.0 };
        let i = t.widget_mut(self.icon);
        i.brush_w = size;
        i.brush_h = size;
        // The fade loop (F3FCF0 -> state colour over 0.2 s game time) restarts.
        self.alpha_start = now;
        // AutoRemoveTime > 0: a timer -> OnAutoRemove -> Broker.RemoveStateForPlayer(id).
        self.auto_remove_at = if p.auto_remove_time > 0.0 { now + p.auto_remove_time } else { f64::INFINITY };
        // Active with a PopText: DestroyExistingPopText, then UI_Functions PopText (upper-cased, lifetime 1, colour =
        // StateColor, space 1, scale 1, centred) and PositionPopText; Tick repositions it every frame.
        if p.active && !p.pop_text.is_empty() {
            if let Some(id) = self.pop.take() {
                pops.destroy(id);
            }
            let mut pp = PopParams::attached(fighter, &p.pop_text.to_uppercase(), 1.0, self.color, [0.0; 3]);
            pp.place = Place::Screen;
            let id = pops.add(pp, now, order);
            self.pop = Some(id);
            self.pop_at = None;
            self.position_pop_text(t, dpi);
            if let Some(at) = self.pop_at {
                pops.place(id, at);
            }
        }
        if let Some(sub) = p.subicon.as_deref().and_then(|s| res.brush(s)) {
            let x = t.find_in(w, "subicon");
            t.widget_mut(x).tex = Some(sub);
        }
        self.params = Some(p);
    }

    /// Enter: PlayAnimationForward(EnterAnim); a player still playing is left alone.
    fn enter(&mut self, real: f64) {
        if self.enter_start < 0.0 {
            self.enter_start = real;
        }
    }

    fn tick(&mut self, t: &mut Tree, a: &Res, now: f64, real: f64, pops: &mut PopTexts, dpi: Option<f64>) {
        // Tick: PositionPopText (IsValid(Pop Text)); a Collapsed widget is not painted and does not tick.
        if let Some(id) = self.pop {
            if !pops.available(id) {
                self.pop = None;
            } else if self.visible(t) {
                self.position_pop_text(t, dpi);
                if let Some(at) = self.pop_at {
                    pops.place(id, at);
                }
            }
        }
        if self.alpha_start >= 0.0 && self.mat {
            let k = clamp01((now - self.alpha_start) / 0.2);
            let c = lerp_color(DIGIT, self.state_color(), k);
            t.dynamic_material(self.icon).set_vector("FillColor", c4(c));
            if k >= 1.0 {
                self.alpha_start = -1.0;
            }
        }
        // Pulse and EnterAnim, both on Overlay_0's transform. A Collapsed state's players stand still
        // (UUMGSequenceTickManager::TickWidgetAnimations, exe 142619f30, test at 1425d63b0).
        let dt = if self.last_tick < 0.0 { 0.0 } else { real - self.last_tick };
        self.last_tick = real;
        if !self.visible(t) {
            if self.pulse_start >= 0.0 {
                self.pulse_start += dt;
            }
            if self.enter_start >= 0.0 {
                self.enter_start += dt;
            }
            return;
        }
        // While both play, the scale they share is their average (one linker per widget, absolute sections of
        // weight 1: exe 1425b5a20 / 142536370).
        let o = self.overlay;
        let prev = t.widget(o).transform.scale;
        let (mut sum, mut n) = ([0.0; 2], 0);
        for (start, anim) in [(&mut self.pulse_start, &a.state_pulse), (&mut self.enter_start, &a.state_enter)] {
            if *start < 0.0 {
                continue;
            }
            let len = anim.end();
            t.widget_mut(o).transform.scale = [f64::NAN; 2];
            anim.at(t, self.w, (real - *start).min(len));
            let s = t.widget(o).transform.scale;
            if !s[0].is_nan() && !s[1].is_nan() {
                sum[0] += s[0];
                sum[1] += s[1];
                n += 1;
            }
            if real - *start >= len {
                *start = -1.0;
            }
        }
        t.widget_mut(o).transform.scale = if n > 0 { [sum[0] / f64::from(n), sum[1] / f64::from(n)] } else { prev };
    }
}

/// UI_IGv3_States: the pool of six states.
#[derive(Debug)]
pub struct States {
    pool: Vec<State>,
}

impl States {
    fn new(t: &mut Tree, w: WidgetId, color: Rgba) -> States {
        // Construct: the pool, all Collapsed.
        let pool = (0..6)
            .map(|i| {
                let s = t.find_in(w, &format!("UI_IGv3_State{i}"));
                let icon = t.find_in(s, "icon");
                let st = State {
                    w: s,
                    params: None,
                    icon,
                    overlay: t.find_in(s, "Overlay_0"),
                    mat: false,
                    color,
                    alpha_start: -1.0,
                    pulse_start: -1.0,
                    enter_start: -1.0,
                    pop: None,
                    pop_at: None,
                    auto_remove_at: f64::INFINITY,
                    last_tick: -1.0,
                    icon_vis: t.widget(icon).vis,
                };
                t.widget_mut(s).vis = Vis::Collapsed;
                st
            })
            .collect();
        States { pool }
    }

    /// A respawned pawn's fresh widgets: every state Collapsed with no params.
    pub fn reset(&mut self, t: &mut Tree) {
        for s in &mut self.pool {
            t.widget_mut(s.w).vis = Vis::Collapsed;
            s.clear();
        }
    }

    /// OnRemoveState
    pub fn remove_state(&mut self, t: &mut Tree, id: &str) {
        if let Some(s) = self.pool.iter().find(|x| x.params.as_ref().is_some_and(|p| p.id == id)) {
            t.widget_mut(s.w).vis = Vis::Collapsed;
        }
    }
}

// ---------- UI_IGv3_Struggle ----------

/// The stun bar over a fighter in a buff stun that can be struggled out of (UI_IGv3_Struggle). OnInitialized
/// @1244-1270: SetVisibility(Hidden), PlayAnimation(InputPulse, endless).
// Unverified: barBG, deltaFill, barFill and Image_84 have a brush with no resource, painted as plain boxes of the
// brush's size in the widget's tint; deltaFill's tint and inputContainer's horizontal alignment are the class
// defaults as the slate module has them, not read in the exe.
#[derive(Debug)]
pub struct Struggle {
    w: WidgetId,
    time_left: f64,
    total_time: f64,
    /// Input Label Params.Action (GameEvade = 2 in the class defaults) and PlayerIndex.
    label_action: u8,
    label_player: usize,
    /// InputPulse's position, seconds; DeltaAnim's (-1: not playing).
    pulse_at: f64,
    delta_at: f64,
    last_tick: f64,
    bar: WidgetId,
    delta: WidgetId,
    input: WidgetId,
    label: PlacedInputLabel,
    is_bot: bool,
    player: usize,
    /// What the two animations and OnStruggle write, as the tree has it (reset).
    initial: Vec<(WidgetId, crate::slate::RenderTransform, f64, Vis)>,
}

impl Struggle {
    fn new(t: &mut Tree, w: WidgetId, is_bot: bool, player: usize) -> Struggle {
        let bar = t.find_in(w, "barFill");
        let delta = t.find_in(w, "deltaFill");
        let input = t.find_in(w, "inputContainer");
        // InputPulse keys the scalar Thickness of Ring's brush material: its dynamic instance.
        let ring = t.find_in(w, "Ring");
        t.dynamic_material(ring);
        // The instance's Params in the tree: SizeIconSize (36, 36).
        let lw = t.find_in(w, "UI_InputLabel");
        let mut label = PlacedInputLabel::new(t, lw, 36.0);
        // Construct: the label of the stored params (the class defaults: GameEvade, player 0), before the app feeds
        // the players' input.
        label.set(t, 2, 0, &LabelSources::default());
        t.widget_mut(w).vis = Vis::Hidden;
        let bc = t.find_in(w, "barcontainer");
        let initial = [bc, input, ring, delta, bar]
            .into_iter()
            .map(|x| {
                let x_ = t.widget(x);
                (x, x_.transform, x_.opacity, x_.vis)
            })
            .collect();
        Struggle {
            w,
            time_left: 0.0,
            total_time: 0.0,
            label_action: 2,
            label_player: 0,
            pulse_at: 0.0,
            delta_at: -1.0,
            last_tick: -1.0,
            bar,
            delta,
            input,
            label,
            is_bot,
            player,
            initial,
        }
    }

    /// A respawned pawn's fresh widget: Hidden, InputPulse at 0, every transform and opacity back at the tree's
    /// values, the label's stored params the class defaults'.
    pub fn reset(&mut self, t: &mut Tree, src: &LabelSources) {
        t.widget_mut(self.w).vis = Vis::Hidden;
        self.time_left = 0.0;
        self.total_time = 0.0;
        self.pulse_at = 0.0;
        self.delta_at = -1.0;
        for &(x, tr, op, vis) in &self.initial {
            let w = t.widget_mut(x);
            w.transform = tr;
            w.opacity = op;
            w.vis = vis;
        }
        self.label_action = 2;
        self.label_player = 0;
        self.label.set(t, 2, 0, src);
    }

    /// OnStruggle @171.
    pub fn on_struggle(
        &mut self,
        t: &mut Tree,
        time_left: f64,
        total: f64,
        action: u8,
        hide_input: bool,
        src: &LabelSources,
    ) {
        // @311-404
        t.widget_mut(self.w).vis = if time_left > 0.0 { Vis::Visible } else { Vis::Hidden };
        // @471-509 Conditional Visibility(inputContainer, Hide Input OR Is Bot?, Hidden, Visible)
        t.widget_mut(self.input).vis = if hide_input || self.is_bot { Vis::Hidden } else { Vis::Visible };
        // @569-810: the label's params and ForceRefresh when its player or action differ
        if self.label_player != self.player || self.label_action != action {
            self.label_action = action;
            self.label_player = self.player;
            self.label.set(t, action, self.player, src);
        }
        // @815-1059: SafeDivide is 0 for a divisor of 0
        self.total_time = total;
        t.widget_mut(self.bar).transform.scale = [if total != 0.0 { time_left / total } else { 0.0 }, 1.0];
        t.widget_mut(self.delta).transform.scale = [if total != 0.0 { self.time_left / total } else { 0.0 }, 1.0];
        // @1101-1168: StopAnimation(DeltaAnim), PlayAnimation(DeltaAnim) from 0, once.
        if time_left < self.time_left {
            self.delta_at = 0.0;
        }
        // @1216
        self.time_left = time_left;
    }

    /// Tick @1318-1521 and the widget's two animations, on real time (Slate's delta, not dilated or paused). A
    /// Hidden user widget is not ticked and its animations stand still.
    // Unverified: that NativeTick stops with a Hidden widget is taken from the tick manager's test, not read.
    fn tick(&mut self, t: &mut Tree, a: &Res, real: f64) {
        let dt = if self.last_tick < 0.0 { 0.0 } else { real - self.last_tick };
        self.last_tick = real;
        if t.widget(self.w).vis == Vis::Hidden {
            return;
        }
        self.time_left = (self.time_left - dt).max(0.0);
        t.widget_mut(self.bar).transform.scale = [self.time_left / self.total_time, 1.0];
        self.label.refresh(t, &a.labels);
        // InputPulse, looping over its playback range.
        self.pulse_at = (self.pulse_at + dt) % a.struggle_pulse.period();
        a.struggle_pulse.at(t, self.w, self.pulse_at);
        if self.delta_at >= 0.0 {
            let len = a.struggle_delta.end();
            a.struggle_delta.at(t, self.w, self.delta_at.min(len));
            self.delta_at = if self.delta_at >= len { -1.0 } else { self.delta_at + dt };
        }
    }
}

// ---------- UI_IGv3_AttachedTop ----------

/// UI_IGv3_AttachedTop.
#[derive(Debug)]
pub struct AttachedTop {
    pub t: Tree,
    pub states: States,
    pub struggle: Struggle,
    /// The DPI scale of the last draw (None: never drawn).
    dpi: Option<f64>,
    fighter: usize,
}

impl AttachedTop {
    pub fn new(proto: &Tree, b: &BrokerPlayer, fighter: usize) -> AttachedTop {
        let mut t = proto.clone();
        // State OnColor: SelectColor(focus, team, Is Me?).
        let sw = t.find("UI_IGv3_States");
        let states = States::new(&mut t, sw, if b.is_me { b.focus } else { b.team });
        // Leader: BroadcastPlayerValues sends SetScoreForPlayer(IsLeadingFFA = false) -> Hidden.
        let l = t.find("UI_IGv3_Leader");
        t.widget_mut(l).vis = Vis::Hidden;
        // Struggle: its Player Index is the fighter's; Is Bot? the base's locality value (UI_IGv3_Base @2258).
        let sg = t.find("UI_IGv3_Struggle");
        let struggle = Struggle::new(&mut t, sg, b.is_bot, fighter);
        // UpdateDebugVisibility: DebugHiddenHUD false.
        let r = t.root();
        t.widget_mut(r).vis = Vis::SelfHitTestInvisible;
        AttachedTop { t, states, struggle, dpi: None, fighter }
    }

    /// UI_Broker SetVisibilityForPlayer: OnVisibility (Visible / Hidden).
    pub fn set_visible(&mut self, v: bool) {
        let r = self.t.root();
        self.t.widget_mut(r).vis = if v { Vis::SelfHitTestInvisible } else { Vis::Hidden };
    }

    /// OnAddState: the state with this id, else the last Collapsed one; SetParams, Enter if not visible, then
    /// SelfHitTestInvisible.
    pub fn add_state(&mut self, res: &Res, p: StateParams, now: f64, real: f64, pops: &mut PopTexts, order: &mut u64) {
        let t = &mut self.t;
        let pool = &mut self.states.pool;
        let k = pool
            .iter()
            .position(|x| x.params.as_ref().is_some_and(|q| q.id == p.id))
            .or_else(|| pool.iter().rposition(|x| t.widget(x.w).vis == Vis::Collapsed));
        let Some(k) = k else { return };
        let s = &mut pool[k];
        s.set_params(t, res, p, now, real, pops, self.fighter, order, self.dpi);
        if !s.visible(t) {
            s.enter(real);
        }
        t.widget_mut(s.w).vis = Vis::SelfHitTestInvisible;
    }

    pub fn remove_state(&mut self, id: &str) {
        self.states.remove_state(&mut self.t, id);
    }

    pub fn reset(&mut self, src: &LabelSources) {
        self.states.reset(&mut self.t);
        self.struggle.reset(&mut self.t, src);
    }

    pub fn on_struggle(&mut self, time_left: f64, total: f64, action: u8, hide_input: bool, src: &LabelSources) {
        self.struggle.on_struggle(&mut self.t, time_left, total, action, hide_input, src);
    }

    pub fn tick(&mut self, a: &Res, now: f64, real: f64, pops: &mut PopTexts) {
        let t = &mut self.t;
        for k in 0..self.states.pool.len() {
            let s = &mut self.states.pool[k];
            s.tick(t, a, now, real, pops, self.dpi);
            if now >= s.auto_remove_at {
                s.auto_remove_at = f64::INFINITY;
                // OnAutoRemove
                if let Some(id) = s.params.as_ref().map(|p| p.id.clone()) {
                    self.states.remove_state(t, &id);
                }
            }
        }
        self.struggle.tick(t, a, real);
    }

    /// DrawSize 250 x 160.
    /// -> whether its root was painted.
    pub fn draw(&mut self, fonts: &mut Fonts, out: &mut ElementList, x: i32, y: i32, dpi: f64) -> bool {
        self.dpi = Some(dpi);
        draw_box(&mut self.t, fonts, out, x, y, [250.0, 160.0], dpi)
    }
}
