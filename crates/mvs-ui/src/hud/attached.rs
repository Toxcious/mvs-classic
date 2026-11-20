//! BaseFighter's "Attached UI v3 Bottom" widget component: UI_IGv3_AttachedBottom with its
//! PlayerMarker, Username, DodgeMeter, Percentage, Flame, Charge and Cooldowns children, driven as UI_Broker drives
//! them (OnColor, OnLocality, OnPercentage, OnEvadeDecay) with their Blueprint graphs ported (UI_IGv3_*). Latent
//! Delay(0) loops run once per drawn frame on game time; UMG animations run on real time.

use crate::bundle::{Bundle, Rgba};
use crate::element::ElementList;
use crate::slate::{Geometry, Tree, Vis, WidgetId, instantiate, layout, paint};
use crate::text::Fonts;

use mvs_sim::events::UiAbilityOp;

use super::Res;
use super::common::{
    Anim, BrokerPlayer, LOCATION_ATTACHED, LOCATION_FIXED, LocalPlayName, PlayerNumbers, Prefs, c4, clamp01, grouped,
    js_num, lerp_color, vis_if,
};
use super::custom_ui::{CustomUi, FinnCharge};

const BLACK: Rgba = [0.0, 0.0, 0.0, 1.0];
/// 141629
pub const DARK: Rgba = [0.006995, 0.008023, 0.022174, 1.0];
/// F3FCF0
pub const DIGIT: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
/// With a grey percentage (Comp_Armor temp health): UI_IGv3_PlayerMarker SetGreyPercentage @2868 (rectangle and
/// triangle) and UI_IGv3_Username OnGreyPercent @1954 (bg).
const GREY_MARKER: Rgba = [0.262251, 0.366253, 0.396755, 1.0];
const GREY_NAME: Rgba = [0.066626, 0.104616, 0.155926, 1.0];

/// The percentage's grey animation (SetGreyPercentage @11105).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GreyAnim {
    Legacy,
    Mine,
    Other,
}

// ---------- UI_IGv3_Percentage ----------

/// A leg of AnimateShadowColor's chain: 1 Team -> TeamOutline, 2 back; started at game time `start`.
#[derive(Clone, Copy, Debug)]
struct Leg {
    id: u32,
    leg: u8,
    start: f64,
}

/// UI_IGv3_Percentage.
#[derive(Debug)]
pub struct Percentage {
    w: WidgetId,
    percentage: i32,
    digits: [WidgetId; 4],
    glyph: WidgetId,
    container: WidgetId,
    gray: WidgetId,
    overlay: WidgetId,
    location: u8,
    /// DamageAnimation: one latent chain; a new call restarts it with the new magnitude (start, magnitude).
    spiral: Option<(f64, f64)>,
    /// AnimateShadowColor: legs of 0.2 s (Team -> TeamOutline, then back); a call restarts the first leg. A chain in
    /// its second leg keeps running and, when done, re-enters (and so merges with) the first leg.
    legs: Vec<Leg>,
    leg_seq: u32,
    /// Real time; Pulse loops until StopAnimation (-1).
    pulse_start: f64,
    /// Grey Percentage (SetGreyPercentage @10712): FTrunc of the Comp_Armor temp health total.
    grey: i32,
    /// ShowGreyHealth / ShowMyGreyHealth / ShowGreyHealth_Legacy: one loop, forward or in reverse (anim, start,
    /// reverse).
    show: Option<(GreyAnim, f64, bool)>,
    /// Real time; PulseWhileGrey loops until StopAnimation (-1).
    grey_pulse_start: f64,
    team: Rgba,
    focus: Rgba,
    team_outline: Rgba,
    focus_outline: Rgba,
    is_me: bool,
    /// World-show Percentage (class default false), which only an Attached RespondToPreferences writes (@9851).
    world_show: bool,
}

impl Percentage {
    pub fn new(t: &mut Tree, w: WidgetId, location: u8, ui: &Prefs, ctx5: bool) -> Percentage {
        let digits = ["digit1", "digit2", "digit3", "digit4"].map(|n| t.find_in(w, n));
        let glyph = t.find_in(w, "percentGlyph");
        // PreConstruct: Show Percent Glyph defaults false.
        t.widget_mut(glyph).vis = Vis::Collapsed;
        let mut p = Percentage {
            w,
            percentage: 0,
            digits,
            glyph,
            container: t.find_in(w, "container"),
            gray: t.find_in(w, "grayPercentage"),
            overlay: t.find_in(w, "OverlayContainer"),
            location,
            spiral: None,
            legs: Vec::new(),
            leg_seq: 0,
            pulse_start: -1.0,
            grey: 0,
            show: None,
            grey_pulse_start: -1.0,
            team: [0.0; 4],
            focus: [0.0; 4],
            team_outline: [0.0; 4],
            focus_outline: [0.0; 4],
            is_me: false,
            world_show: false,
        };
        p.respond_to_preferences(t, ui, ctx5);
        // grayPercentage is Collapsed in the asset; the broker's GreyPercentage starts at 0 (UI_Broker @391), so the
        // OnGreyPercent of SetupInitialValues @6850 changes nothing (NotEqual @10712).
        t.widget_mut(p.gray).vis = Vis::Collapsed;
        p
    }

    /// RespondToPreferences (@9727): Fixed stores hud-showPercentage (@9818), Attached world-showPercentage (@9851);
    /// visbilityPreference HitTestInvisible when the stored value (Attached: OR context 5) holds, else Collapsed.
    fn respond_to_preferences(&mut self, t: &mut Tree, ui: &Prefs, ctx5: bool) {
        let show = match self.location {
            LOCATION_FIXED => ui.hud_show_percentage,
            LOCATION_ATTACHED => {
                self.world_show = ui.world_show_percentage;
                self.world_show || ctx5
            }
            _ => return,
        };
        let v = t.find_in(self.w, "visbilityPreference");
        t.widget_mut(v).vis = vis_if(show, Vis::HitTestInvisible, Vis::Collapsed);
    }

    /// SetGreyPercentage (ubergraph @10712): nothing when the value is the one held; else grayPercentage's text
    /// (grouped, @10787) and Conditional Visibility (0 Collapsed, else SelfHitTestInvisible @10915); the grey
    /// animation from 0, one loop, Reverse at 0 (from the last valid frame: PlayInternal 1425e7110); at 0
    /// StopAnimation(PulseWhileGrey), StopAnimation(Pulse), OverlayContainer's scale 1 (@11339); else
    /// PulseWhileGrey endless when Percentage >= 100 (@11434).
    pub fn set_grey_percentage(&mut self, t: &mut Tree, g: i32, real: f64) {
        if g == self.grey {
            return;
        }
        self.grey = g;
        let gw = t.widget_mut(self.gray);
        gw.text = grouped(i64::from(g));
        gw.vis = vis_if(g == 0, Vis::Collapsed, Vis::SelfHitTestInvisible);
        let anim = if !self.world_show {
            GreyAnim::Legacy
        } else if self.is_me {
            GreyAnim::Mine
        } else {
            GreyAnim::Other
        };
        self.show = Some((anim, real, g == 0));
        if g == 0 {
            self.grey_pulse_start = -1.0;
            self.pulse_start = -1.0;
            t.widget_mut(self.overlay).transform.scale = [1.0, 1.0];
        } else if self.percentage >= 100 {
            self.grey_pulse_start = real;
        }
    }

    fn all(&self) -> [WidgetId; 5] {
        [self.digits[0], self.digits[1], self.digits[2], self.digits[3], self.glyph]
    }

    pub fn on_color(&mut self, t: &mut Tree, team: Rgba, focus: Rgba, is_me: bool) {
        self.is_me = is_me;
        self.team = team;
        self.focus = focus;
        self.focus_outline = lerp_color(focus, DARK, 1.0);
        self.team_outline = lerp_color(team, BLACK, 0.85);
        self.update_digits_appearance(t);
    }

    fn update_digits_appearance(&self, t: &mut Tree) {
        let c = if self.is_me { self.focus_outline } else { self.team_outline };
        for d in self.all() {
            let w = t.widget_mut(d);
            w.color = DIGIT;
            w.outline_color = c;
            w.shadow_color = c;
            w.shadow_offset = [7.0, 7.0];
            w.pad.l = -9.0;
            w.pad.r = -9.0;
        }
    }

    pub fn set_percentage(&mut self, t: &mut Tree, p: i32, now: f64, real: f64) {
        let delta = p - self.percentage;
        self.percentage = p;
        let v = p.clamp(0, 9999);
        let (th, hu, te, on) = (v / 1000, (v / 100) % 10, (v / 10) % 10, v % 10);
        let [d1, d2, d3, d4] = self.digits;
        t.widget_mut(d1).text = on.to_string();
        let mut digit = |d: WidgetId, hide: bool, n: i32| {
            let w = t.widget_mut(d);
            if hide {
                w.vis = Vis::Collapsed;
            } else {
                w.text = n.to_string();
                w.vis = Vis::SelfHitTestInvisible;
            }
        };
        digit(d2, hu == 0 && te == 0 && th == 0, te);
        digit(d3, th == 0 && hu == 0, hu);
        digit(d4, th == 0, th);
        // Container grows 1 -> 1.2 over 0..200 %.
        let s = 1.0 + 0.2 * clamp01(f64::from(p.clamp(0, 200)) / 200.0);
        t.widget_mut(self.container).transform.scale = [s, s];
        if p >= 100 {
            self.pulse_start = real;
            self.animate_shadow_color(t, now);
        } else {
            // StopAnimation(Pulse): the section completes with KeepState, so the scale set above stands.
            self.pulse_start = -1.0;
            self.update_digits_appearance(t);
        }
        if delta != 0 {
            // MapRangeClamped 1..10 -> 5..60
            let mag = 5.0 + ((f64::from(delta.abs()).clamp(1.0, 10.0) - 1.0) / 9.0) * 55.0;
            self.spiral = Some((now, mag));
            self.tick_spiral(t, now);
        }
    }

    fn animate_shadow_color(&mut self, t: &mut Tree, now: f64) {
        if self.percentage < 100 {
            return;
        }
        match self.legs.iter_mut().find(|l| l.leg == 1) {
            Some(l) => l.start = now,
            None => {
                self.leg_seq += 1;
                self.legs.push(Leg { id: self.leg_seq, leg: 1, start: now });
            }
        }
        self.tick_legs(t, now, false);
    }

    fn apply_leg(&self, t: &mut Tree, leg: u8, a: f64) {
        let (from, to) = match (leg, self.is_me) {
            (1, true) => (self.focus, self.focus_outline),
            (1, false) => (self.team, self.team_outline),
            (_, true) => (self.focus_outline, self.focus),
            (_, false) => (self.team_outline, self.team),
        };
        let c = lerp_color(from, to, a);
        for d in self.all() {
            let w = t.widget_mut(d);
            w.shadow_color = c;
            if leg == 1 {
                w.shadow_offset = [9.0, 9.0];
                w.pad.l = -10.0;
                w.pad.r = -10.0;
            }
        }
    }

    fn tick_legs(&mut self, t: &mut Tree, now: f64, advance: bool) {
        // Over the legs there are now, by identity: a leg the pass ends and the first leg it starts again
        // (animate_shadow_color) change the list as it goes.
        let ids: Vec<u32> = self.legs.iter().map(|l| l.id).collect();
        for id in ids {
            let Some(k) = self.legs.iter().position(|l| l.id == id) else { continue };
            let Leg { leg, start, .. } = self.legs[k];
            let a = clamp01((now - start) / 0.2);
            self.apply_leg(t, leg, a);
            if !advance || a < 1.0 {
                continue;
            }
            if leg == 1 {
                // Second leg starts the same frame at alpha 0.
                self.legs[k].leg = 2;
                self.legs[k].start = now;
                self.apply_leg(t, 2, 0.0);
            } else {
                self.legs.remove(k);
                if self.percentage >= 100 {
                    self.animate_shadow_color(t, now);
                }
            }
        }
    }

    fn tick_spiral(&mut self, t: &mut Tree, now: f64) {
        let Some((start, mag)) = self.spiral else { return };
        let a = clamp01((now - start) / 0.35);
        let r = (1.0 - a) * if self.location == 1 { 2.0 * mag } else { mag };
        let ang = (720.0 * a).to_radians();
        let [d1, d2, d3, d4] = self.digits;
        for d in [d4, d2, self.glyph] {
            t.widget_mut(d).transform.translation = [ang.sin() * r, ang.cos() * r];
        }
        for d in [d3, d1] {
            t.widget_mut(d).transform.translation = [ang.cos() * r, ang.sin() * r];
        }
        if a >= 1.0 {
            self.spiral = None;
        }
    }

    pub fn tick(&mut self, t: &mut Tree, a: &Res, now: f64, real: f64) {
        if self.spiral.is_some() {
            self.tick_spiral(t, now);
        }
        if !self.legs.is_empty() {
            self.tick_legs(t, now, true);
        }
        let pulsing = self.pulse_start >= 0.0;
        if pulsing {
            a.pct_pulse.at(t, self.w, (real - self.pulse_start) % a.pct_pulse.period());
        }
        if let Some((which, start, reverse)) = self.show {
            let anim = match which {
                GreyAnim::Legacy => &a.pct_grey_legacy,
                GreyAnim::Mine => &a.pct_grey_mine,
                GreyAnim::Other => &a.pct_grey,
            };
            // One loop to the last valid frame, the player finished in the frame of that evaluation.
            let len = anim.end();
            let el = (real - start).clamp(0.0, len);
            // Pulse and this animation both key container's scale and shear: absolute sections of weight 1 in one
            // linker, so the property gets their average (the float blender).
            let c = self.container;
            let before = shared(t, c);
            if pulsing {
                set_shared(t, c, [f64::NAN; 4]);
            }
            anim.at(t, self.w, if reverse { len - el } else { el });
            if pulsing {
                let x = shared(t, c);
                set_shared(
                    t,
                    c,
                    [0, 1, 2, 3].map(|i| if x[i].is_nan() { before[i] } else { (x[i] + before[i]) / 2.0 }),
                );
            }
            if el >= len {
                self.show = None;
            }
        }
        if self.grey_pulse_start >= 0.0 {
            let p = &a.pct_pulse_grey;
            p.at(t, self.w, (real - self.grey_pulse_start) % p.period());
        }
    }
}

/// container's channels that Pulse and a ShowGreyHealth animation both key: scale x, y, shear x, y.
fn shared(t: &Tree, w: WidgetId) -> [f64; 4] {
    let x = &t.widget(w).transform;
    [x.scale[0], x.scale[1], x.shear[0], x.shear[1]]
}

fn set_shared(t: &mut Tree, w: WidgetId, v: [f64; 4]) {
    let x = &mut t.widget_mut(w).transform;
    x.scale = [v[0], v[1]];
    x.shear = [v[2], v[3]];
}

// ---------- UI_IGv3_DodgeMeter ----------

/// UI_IGv3_DodgeMeter.
#[derive(Debug)]
pub struct DodgeMeter {
    w: WidgetId,
    bar: WidgetId,
    delta: WidgetId,
    fade_start: f64,
    responds_to_prefs: bool,
    location: u8,
}

impl DodgeMeter {
    /// location: the widget's Location (UnifyChildren's copy of its owner's); ui: the preferences at build.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        t: &mut Tree,
        w: WidgetId,
        height: f64,
        width: f64,
        show_label: bool,
        responds: bool,
        location: u8,
        ui: &Prefs,
    ) -> DodgeMeter {
        let bar = t.find_in(w, "evadeBar");
        let delta = t.find_in(w, "evadeDelta");
        let bx = t.find_in(w, "EvadeSizeBox");
        let b = t.widget_mut(bx);
        b.h_override = height;
        b.w_override = if width > 0.0 { width } else { f64::NAN };
        let m = DodgeMeter { w, bar, delta, fade_start: -1.0, responds_to_prefs: responds, location };
        m.set_show_label(t, show_label);
        // OnInitialized
        t.widget_mut(bar).transform.scale = [1.0, 1.0];
        t.widget_mut(delta).opacity = 0.0;
        m.respond_to_preferences(t, ui);
        m
    }

    /// RespondToPreferences (@2715), only with Responds To Prefs: Attached world-showDodgeMeters, Fixed
    /// hud-showDodgeMeters -> visibilityPreference HitTestInvisible, else Hidden (@586 / @547).
    fn respond_to_preferences(&self, t: &mut Tree, ui: &Prefs) {
        if !self.responds_to_prefs {
            return;
        }
        let show = match self.location {
            LOCATION_ATTACHED => ui.world_show_dodge_meters,
            LOCATION_FIXED => ui.hud_show_dodge_meters,
            _ => return,
        };
        let v = t.find_in(self.w, "visibilityPreference");
        t.widget_mut(v).vis = vis_if(show, Vis::HitTestInvisible, Vis::Hidden);
    }

    pub fn set_height(&self, t: &mut Tree, h: f64) {
        let b = t.find_in(self.w, "EvadeSizeBox");
        t.widget_mut(b).h_override = h;
    }

    pub fn set_show_label(&self, t: &mut Tree, b: bool) {
        let l = t.find_in(self.w, "label");
        t.widget_mut(l).vis = vis_if(b, Vis::SelfHitTestInvisible, Vis::Hidden);
    }

    /// SetupInitialValues @2255-2356: label = Custom Name Set ? Username : PlayerNumberShort.
    pub fn set_player(&self, t: &mut Tree, name: &LocalPlayName) {
        let l = t.find_in(self.w, "label");
        t.widget_mut(l).text = if name.custom { name.username.clone() } else { name.short.clone() };
    }

    pub fn on_color(&self, t: &mut Tree, team: Rgba) {
        t.widget_mut(self.bar).color = team;
        let bg = t.find_in(self.w, "evadeBG");
        t.widget_mut(bg).color = team;
    }

    pub fn on_evade_decay(&mut self, t: &mut Tree, decay: f64, now: f64) {
        let s = t.widget(self.bar).transform.scale;
        t.widget_mut(self.delta).transform.scale = s;
        t.widget_mut(self.bar).transform.scale = [clamp01(decay), 1.0];
        self.fade_start = now;
        self.tick(t, now);
    }

    pub fn tick(&mut self, t: &mut Tree, now: f64) {
        if self.fade_start < 0.0 {
            return;
        }
        let a = clamp01((now - self.fade_start) / 0.25);
        t.widget_mut(self.delta).opacity = 1.0 - a;
        if a >= 1.0 {
            self.fade_start = -1.0;
        }
    }
}

// ---------- UI_IGv3_Flame ----------

/// UI_IGv3_Flame.
#[derive(Debug)]
pub struct Flame {
    flame: WidgetId,
}

impl Flame {
    pub fn new(t: &mut Tree, w: WidgetId, b: &BrokerPlayer, location: u8, ui: &Prefs) -> Flame {
        let flame = t.find_in(w, "Flame");
        // GetFlameMaterial
        t.dynamic_material(flame);
        let f = Flame { flame };
        // RespondToPreferences (@2126): world-showPercentage (Attached) or hud-showPercentage (Fixed) ->
        // visibilityPreference HitTestInvisible, else Hidden (@2217..2318).
        let show = match location {
            LOCATION_ATTACHED => Some(ui.world_show_percentage),
            LOCATION_FIXED => Some(ui.hud_show_percentage),
            _ => None,
        };
        if let Some(show) = show {
            let v = t.find_in(w, "visibilityPreference");
            t.widget_mut(v).vis = vis_if(show, Vis::HitTestInvisible, Vis::Hidden);
        }
        // RespondToLocality
        let y = if (b.is_me || b.is_local) && location == 1 { -15.0 } else { 10.0 };
        t.widget_mut(flame).transform.translation = [0.0, y];
        f.on_color(t, b);
        f
    }

    /// OnColor
    pub fn on_color(&self, t: &mut Tree, b: &BrokerPlayer) {
        let m = t.dynamic_material(self.flame);
        if b.is_ally {
            m.set_vector("Color2", c4(b.focus));
            m.set_vector("Color1", [0.0; 4]);
        } else {
            m.set_vector("Color2", [1.0, 0.60864, 0.0, 1.0]);
            m.set_vector("Color1", [1.0, 0.0, 0.019753, 1.0]);
        }
    }

    /// OnPercentage: shown from 100 %, scale 1 -> 1.5 over 100..200 %.
    pub fn on_percentage(&self, t: &mut Tree, p: i32) {
        let w = t.widget_mut(self.flame);
        w.vis = if p >= 100 { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        let s = 1.0 + 0.5 * clamp01(f64::from(p - 100) / 100.0);
        w.transform.scale = [s, s];
    }
}

// ---------- UI_IGv3_Charge ----------

/// UI_IGv3_Charge.
#[derive(Debug)]
pub struct Charge {
    w: WidgetId,
    meter: WidgetId,
    home: [f64; 2],
    start_time: f64,
    total_time: f64,
    time_based: bool,
    specific: f64,
    /// DoOnce gate of IntroAnim, reopened when the meter leaves (0, 1).
    closed: bool,
    /// Real time IntroAnim started (-1: none).
    intro_start: f64,
}

impl Charge {
    pub fn new(t: &mut Tree, w: WidgetId, b: &BrokerPlayer) -> Charge {
        // Construct: HomeTranslation = RenderTransform.Translation (the AttachedBottom slot's).
        let home = t.widget(w).transform.translation;
        // OnInitialized
        let meter = t.find_in(w, "Meter");
        t.dynamic_material(meter);
        t.widget_mut(w).opacity = 0.0;
        let c = Charge {
            w,
            meter,
            home,
            start_time: 0.0,
            total_time: 0.0,
            time_based: false,
            specific: -1.0,
            closed: false,
            intro_start: -1.0,
        };
        c.on_color(t, b);
        let m = t.dynamic_material(meter);
        m.set_vector("BackgroundColor", c4(DARK));
        m.set_vector("OutlineColor", c4(DARK));
        c
    }

    /// OnColor
    pub fn on_color(&self, t: &mut Tree, b: &BrokerPlayer) {
        t.dynamic_material(self.meter).set_vector("FillColor", c4(if b.is_me { b.focus } else { b.team }));
    }

    /// OnChargeAttack
    pub fn on_charge_attack(&mut self, start: f64, total: f64, reset: bool, specific: f64) {
        self.start_time = start;
        self.total_time = total;
        self.time_based = specific < 0.0;
        self.specific = if reset { -1.0 } else { specific };
    }

    fn percentage(&self, now: f64) -> f64 {
        if !self.time_based {
            return self.specific;
        }
        if self.total_time.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return 0.0;
        }
        clamp01((now - self.start_time) / self.total_time)
    }

    /// Tick (GameTime = now).
    pub fn tick(&mut self, t: &mut Tree, a: &Res, now: f64, real: f64, facing_left: bool) {
        let p = self.percentage(now);
        let on = p > 0.0 && p < 1.0;
        t.widget_mut(self.w).opacity = if on { 1.0 } else { 0.0 };
        if on {
            let v = if self.time_based { 0.2 + 0.8 * p } else { p };
            t.dynamic_material(self.meter).set_scalar("Percentage", v as f32);
            if !self.closed {
                self.closed = true;
                self.intro_start = real;
            }
        } else {
            self.closed = false;
        }
        if self.intro_start >= 0.0 {
            let el = real - self.intro_start;
            // The playback range end is exclusive: a finished animation holds its last frame.
            let len = a.charge_intro.end();
            a.charge_intro.at(t, self.w, el.min(len));
            if el >= len {
                self.intro_start = -1.0;
            }
        }
        let x = if facing_left { self.home[0] } else { -self.home[0] };
        t.widget_mut(self.w).transform.translation = [x, self.home[1]];
    }
}

// ---------- UI_IGv3_Cooldown ----------

/// An ability row as the cooldown widgets read it (the fighter's `abilityRow`, the
/// template's or the one a graph wrote over it): its icon's object path ("" for none) and the options.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudAbility {
    pub icon: String,
    pub cooldown: f64,
    pub max_count: f64,
    pub start_count: f64,
    pub count_cooldown: f64,
    pub cooldown_always_visible: bool,
    pub ally_cooldown_visible: bool,
    pub foe_cooldown_visible: bool,
    pub static_ability_cooldown: bool,
    pub static_ammo_cooldown: bool,
}

impl HudAbility {
    /// The row of the sim's manager, its icon (a texture of the hud bundle, by its name) as the bundle's object path
    /// (`brush`: the bundle's lookup; "" for none or one the bundle lacks).
    pub fn of(
        w: &mvs_sim::World,
        r: &mvs_sim::fighter::control::AbilityRow,
        brush: impl Fn(&str) -> Option<String>,
    ) -> HudAbility {
        HudAbility {
            icon: r.icon.and_then(|n| brush(w.names.str(n))).unwrap_or_default(),
            cooldown: f64::from(r.cooldown),
            max_count: f64::from(r.max_count),
            start_count: f64::from(r.start_count),
            count_cooldown: f64::from(r.count_cooldown),
            cooldown_always_visible: r.cooldown_always_visible,
            ally_cooldown_visible: r.ally_cooldown_visible,
            foe_cooldown_visible: r.foe_cooldown_visible,
            static_ability_cooldown: r.static_ability_cooldown,
            static_ammo_cooldown: r.static_ammo_cooldown,
        }
    }
}

/// A cooldown UI call of the broker: Comp_Pawn_AbilityManager's UpdateLocalUI /
/// ClearCooldownClockMulticast ops, or CreateAbilityUI's options and icon (`icon`: the texture's object path, ""
/// for none or one the bundle lacks).
#[derive(Clone, Debug, PartialEq)]
pub enum HudAbilityOp {
    Create { icon: String, max_count: i32 },
    Op(UiAbilityOp),
}

/// UI_IGv3_Cooldown's animations, in the order they are evaluated (Res.cooldown).
pub const COOLDOWN_ANIMS: [&str; 4] = ["AmmoAttentionAnim", "ExitAnim", "EnterAnim", "ErrorAnim"];
const AMMO_ATTENTION: usize = 0;
const EXIT: usize = 1;
const ENTER: usize = 2;
const ERROR: usize = 3;
const COOLDOWN_DARK: Rgba = [0.009721, 0.011612, 0.033105, 1.0];
/// PlayAnimation's PlaybackSpeed: PlayError passes 4 (UI_IGv3_Cooldown ubergraph 10126), the others 1.
const COOLDOWN_SPEED: [f64; 4] = [1.0, 1.0, 1.0, 4.0];

/// visibilityContainer's transform channels that two playing animations share (Cooldown.tick): translation x, y,
/// scale x, y.
fn cd_shared(c: &crate::slate::RenderTransform) -> [f64; 4] {
    [c.translation[0], c.translation[1], c.scale[0], c.scale[1]]
}

fn set_cd_shared(c: &mut crate::slate::RenderTransform, v: [f64; 4]) {
    c.translation = [v[0], v[1]];
    c.scale = [v[2], v[3]];
}

/// One ability slot of UI_IGv3_Cooldowns, driven by Comp_Pawn_AbilityManager through the broker (the sim's
/// 'uiAbility' ops). CreateAbilityUI gives it the entry's MaxCount and Icon; OnCharacter collapses its
/// visibilityContainer until the first enter.
///
/// The broker's cooldown delegates reach every UI_IGv3_Cooldown bound to the player: the attached one and, with a
/// UI_IGv3_Fixed slot of the player in the scoreboard (UI_IGv3_Base Init binds its children with the slot's Player
/// Index), that slot's (fixed.rs Fixed). Both get the same calls and both tick their animations: the tick manager
/// (UUMGSequenceTickManager::TickWidgetAnimations, exe 142619f30; Widget.UseParallelAnimation registers with default
/// true, exe 1406f3656) tests only the user widget's own visibility (exe 1425d63b0), not its Hidden
/// visibilityPreference ancestor. So each makes its own PlayExit decisions and posts ui_ability_ready (@5513, no
/// visibility test), seen or not.
#[derive(Debug)]
struct Cooldown {
    w: WidgetId,
    container: WidgetId,
    icon: WidgetId,
    ammo_container: WidgetId,
    ammo_label: WidgetId,
    timer: WidgetId,
    ammo_fill: WidgetId,
    def: HudAbility,
    location: u8,
    /// OnInitialized: Cooldown Start / End Time -1.
    start: f64,
    end: f64,
    ammo_start: f64,
    ammo_end: f64,
    current_ammo: f64,
    exiting: bool,
    /// ExitAnim's proxy has set its timer: the Finished pin fires at the next tick (see tick).
    exit_finished: bool,
    /// Real start times by COOLDOWN_ANIMS.
    playing: [Option<f64>; 4],
    color: Rgba,
}

impl Cooldown {
    /// location: Attached or Fixed (UnifyChildren's copy of the owner's); ui: the preferences at build.
    fn new(t: &mut Tree, w: WidgetId, b: &BrokerPlayer, def: &HudAbility, location: u8, ui: &Prefs) -> Cooldown {
        // WBP_Quadrilateral GetMaterial: the quad's dynamic material with SetParameters' corners.
        let timer = t.find_in(w, "timer");
        t.dynamic_material(timer);
        let ammo_fill = t.find_in(w, "ammoFill");
        t.dynamic_material(ammo_fill);
        let c = Cooldown {
            w,
            container: t.find_in(w, "visibilityContainer"),
            icon: t.find_in(w, "icon"),
            ammo_container: t.find_in(w, "ammoContainer"),
            ammo_label: t.find_in(w, "ammoLabel"),
            timer,
            ammo_fill,
            def: def.clone(),
            location,
            start: -1.0,
            end: -1.0,
            ammo_start: 0.0,
            ammo_end: 0.0,
            current_ammo: 0.0,
            exiting: false,
            exit_finished: false,
            playing: [None; 4],
            color: if b.is_me { b.focus } else { b.team },
        };
        c.respond_to_preferences(t, ui);
        // OnCooldownOptions (ammoContainer Visible / Hidden), OnCooldownIcon (SetBrushFromTexture without MatchSize).
        t.widget_mut(c.ammo_container).vis = vis_if(def.max_count > 0.0, Vis::Visible, Vis::Hidden);
        if !def.icon.is_empty() {
            t.widget_mut(c.icon).tex = Some(def.icon.clone());
        }
        // OnColor: DoColor(0) (the colour above); OnCharacter: SetCooldownPercentage(1), ToggleVisibility(false).
        c.set_cooldown_percentage(t, 1.0);
        t.widget_mut(c.container).vis = Vis::Collapsed;
        c
    }

    /// RespondToPreferences (@8958): the icon's brush size, Attached 64 with world-showCooldowns else 44 (@9186),
    /// Fixed 44 with hud-showCooldowns else 64 (@9049): a scoreboard that shows the cooldowns has the small icons.
    fn respond_to_preferences(&self, t: &mut Tree, ui: &Prefs) {
        let big = match self.location {
            LOCATION_ATTACHED => ui.world_show_cooldowns,
            LOCATION_FIXED => !ui.hud_show_cooldowns,
            _ => return,
        };
        let s = if big { 64.0 } else { 44.0 };
        let i = t.widget_mut(self.icon);
        (i.brush_w, i.brush_h) = (s, s);
    }

    /// Comp_Pawn_AbilityManager CreateAbilityUI on a slot that has its widget (ResetAbility @11533, after
    /// OverrideAbility: Buff_Disguise's slot 0): UI_Broker SetCooldownOptionsForPlayer(slot, MaxCount) @10537 and
    /// SetCooldownIconForPlayer(slot, Icon) @10657, the constructor's OnCooldownOptions / OnCooldownIcon again.
    /// def: the slot's row from now on (what the manager's later UI calls read).
    fn create(&mut self, t: &mut Tree, def: &HudAbility, icon: &str, max_count: i32) {
        self.def = def.clone();
        t.widget_mut(self.ammo_container).vis = vis_if(max_count > 0, Vis::Visible, Vis::Hidden);
        if !icon.is_empty() {
            t.widget_mut(self.icon).tex = Some(icon.to_string());
        }
    }

    /// DoColor
    fn do_color(&self, t: &mut Tree, v: f64) {
        let k = (v + 0.2) * 0.833333 * 0.85;
        t.dynamic_material(self.ammo_fill).set_vector("FillColor", c4(lerp_color(COOLDOWN_DARK, self.color, k)));
        let (opacity, fill) = if t.widget(self.ammo_container).vis == Vis::Visible {
            let o = if self.current_ammo == 0.0 { 0.5 } else { (v + 0.5) * 1.25 };
            (o, lerp_color(COOLDOWN_DARK, self.color, 0.85))
        } else {
            ((v + 0.5) * 1.25, lerp_color(COOLDOWN_DARK, self.color, k))
        };
        t.widget_mut(self.icon).opacity = opacity;
        t.dynamic_material(self.timer).set_vector("FillColor", c4(fill));
    }

    fn set_cooldown_percentage(&self, t: &mut Tree, v: f64) {
        let c = clamp01(v);
        t.dynamic_material(self.timer).set_scalar("Percentage", c as f32);
        self.do_color(t, c);
    }

    fn set_ammo_percentage(&self, t: &mut Tree, v: f64) {
        let c = clamp01(v);
        let p = if c >= 1.0 { 0.0 } else { c };
        t.dynamic_material(self.ammo_fill).set_scalar("Percentage", p as f32);
        self.do_color(t, p);
    }

    /// StopAnimationsExcept: stopping ExitAnim clears Exiting first, so its Finished proxy leaves the widget shown.
    fn stop_except(&mut self, keep: usize) {
        for a in 0..4 {
            if a == keep {
                continue;
            }
            if a == EXIT {
                self.exiting = false;
            }
            self.playing[a] = None;
        }
    }

    /// PlayEnter (OnCooldownEnter)
    fn play_enter(&mut self, t: &mut Tree, real: f64) {
        if self.playing[ENTER].is_some() {
            return;
        }
        self.stop_except(ENTER);
        t.widget_mut(self.container).vis = Vis::Visible;
        self.playing[ENTER] = Some(real);
    }

    /// PlayExit (OnCooldownExit @10313 -> @5824): IsVisible() AND NOT IsAnimationPlaying(ExitAnim) (@5824-5940),
    /// then StopAnimationsExcept(ExitAnim) @5950, Exiting @5973, ExitAnim through the proxy @5350 and
    /// ui_ability_ready @5513 with no test of its own. IsVisible() is the user widget's own Visibility (the slot's in
    /// UI_IGv3_Cooldowns, read here off the widget): no graph writes it, so neither a Collapsed visibilityContainer
    /// (a slot that never entered, or one that left), nor the Hidden cooldownContainer of a player that is not local
    /// (UI_IGv3_Cooldowns RespondToLocality @1010), nor the Collapsed AttachedBottom of a defeated pawn keeps the exit
    /// or its sound away. So ResetAbilities' exit to every slot at a respawn (ClearCooldownClockMulticast,
    /// Comp_Pawn_AbilityManager @6698 -> @7823-7869) posts once per slot, and every player's exits are heard on
    /// every screen (UpdateAbilityMulticast @405 runs wherever IsDedicatedServer is false, PF_Post_Global_Event tests
    /// the dedicated server alone, Lib_Audio @0).
    // Unverified: as the bytecode reads; not heard in the build.
    fn play_exit(&mut self, t: &Tree, real: f64, sounds: &mut Vec<String>) {
        if !t.widget(self.w).shown() || self.playing[EXIT].is_some() {
            return;
        }
        self.stop_except(EXIT);
        self.exiting = true;
        self.playing[EXIT] = Some(real);
        sounds.push("ui_ability_ready".to_string());
    }

    /// PlayError (OnCooldownError, UI_Broker.SetCooldownErrorForPlayer from the manager's ShowAbilityUINotReady):
    /// PlayAnimation(ErrorAnim, 0, 1, Forward, 4) alone; it stops no other animation and shows nothing by itself,
    /// and a call while it plays starts it again from 0. With EnterAnim or ExitAnim playing too, see tick.
    fn play_error(&mut self, real: f64) {
        self.playing[ERROR] = Some(real);
    }

    /// Comp_Pawn_AbilityManager UpdateLocalUI: the enter pulse, SetAbilityUI, SetAmmoUI with an ammo count, and the
    /// exit when nothing is left to wait for (CooldownAlwaysVisible off). OnStartTime's own run of the tick body is
    /// left to this frame's Tick.
    #[allow(clippy::too_many_arguments)]
    fn update(
        &mut self,
        t: &mut Tree,
        (ammo, cd, cd_max, ammo_cd, ammo_max): (f64, f64, f64, f64, f64),
        pulse: bool,
        pulse_ammo: bool,
        now: f64,
        real: f64,
        sounds: &mut Vec<String>,
    ) {
        if pulse {
            self.play_enter(t, real);
        }
        if cd > 0.0 {
            self.start = now + cd - cd_max;
            self.end = now + cd;
        } else {
            self.set_cooldown_percentage(t, if cd == 0.0 { 1.0 } else { 0.0 });
        }
        if self.def.max_count > 0.0 {
            t.widget_mut(self.ammo_label).text = js_num(ammo);
            self.current_ammo = ammo;
            let rem = if ammo_max < 0.0 { 1e8 } else { ammo_cd };
            self.ammo_start = now + rem - if ammo_max < 0.0 { 1e8 } else { ammo_max };
            self.ammo_end = now + rem;
            if pulse_ammo {
                // PlayAmmoAttention
                self.playing[AMMO_ATTENTION] = Some(real);
            }
        }
        if !self.def.cooldown_always_visible && ammo_cd == 0.0 && cd == 0.0 {
            self.play_exit(t, real, sounds);
        }
    }

    /// Tick (game time), then the playing UMG animations (real time); ExitAnim's Finished collapses the widget while
    /// Exiting. The pin is the play proxy's (CreatePlayAnimationProxyObject @5350): the player's finished event comes
    /// in the frame of the last evaluation, and the proxy's handler (1425dbdc0) only sets a 0.001 s one-shot world
    /// timer, so the pin fires in the next frame's timer pass, before that frame's widgets: one frame is painted at
    /// the end pose. Exiting is read when the pin fires.
    ///
    /// ErrorAnim shares visibilityContainer's translation and scale with EnterAnim and ExitAnim: while two of them
    /// play, each channel both sections drive is their average (one linker per widget, the float blender divides the
    /// absolute total by the absolute weight, exe 1425b5a20 / 142536370; these sections are absolute with weight 1).
    /// A channel only one section covers at the time is that section's alone, and a finished player no longer
    /// counts.
    // Deviation: a frame shorter than 0.001 s and a paused world's timers are not modelled.
    fn tick(&mut self, t: &mut Tree, anims: &[Anim; 4], now: f64, real: f64) {
        let c = self.container;
        if self.exit_finished {
            self.exit_finished = false;
            if self.exiting {
                t.widget_mut(c).vis = Vis::Collapsed;
            }
        }
        let blend = self.playing[ERROR].is_some() && (self.playing[ENTER].is_some() || self.playing[EXIT].is_some());
        let prev = cd_shared(&t.widget(c).transform);
        let mut sum = [0.0; 4];
        let mut count = [0u32; 4];
        if self.end > now && self.end > self.start {
            self.set_cooldown_percentage(t, (now - self.start) / (self.end - self.start));
        }
        if self.ammo_end > now && self.ammo_end > self.ammo_start {
            self.set_ammo_percentage(t, (now - self.ammo_start) / (self.ammo_end - self.ammo_start));
        }
        for (n, a) in anims.iter().enumerate() {
            let Some(s) = self.playing[n] else { continue };
            let len = a.end();
            let el = (real - s) * COOLDOWN_SPEED[n];
            if blend {
                set_cd_shared(&mut t.widget_mut(c).transform, [f64::NAN; 4]);
            }
            a.at(t, self.w, el.min(len));
            if blend {
                for (i, x) in cd_shared(&t.widget(c).transform).into_iter().enumerate() {
                    if !x.is_nan() {
                        sum[i] += x;
                        count[i] += 1;
                    }
                }
            }
            if el < len {
                continue;
            }
            self.playing[n] = None;
            if n == EXIT {
                self.exit_finished = true;
            }
        }
        if blend {
            let v: [f64; 4] =
                std::array::from_fn(|i| if count[i] > 0 { sum[i] / f64::from(count[i]) } else { prev[i] });
            set_cd_shared(&mut t.widget_mut(c).transform, v);
        }
    }
}

// ---------- UI_IGv3_Cooldowns ----------

/// The six ability slots of an Attached or a Fixed widget (UI_IGv3_Cooldowns). Every cooldown UI call comes from
/// Comp_Pawn_AbilityManager's Abilities (Shaggy's are empty), one UI_IGv3_Cooldown per entry; the widgets past them
/// never get options / icon / enter. Each UI_IGv3_Cooldown collapses its visibilityContainer in OnCharacter (the
/// broker's SetCharacterForPlayer at the fighter's UI broadcast). The orientation's frame shear is the owner's.
#[derive(Debug)]
pub struct Cooldowns {
    slots: Vec<Cooldown>,
}

impl Cooldowns {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    /// abilities: the fighter's Comp_Pawn_AbilityManager Abilities.
    pub fn new(
        t: &mut Tree,
        w: WidgetId,
        b: &BrokerPlayer,
        abilities: &[HudAbility],
        location: u8,
        ui: &Prefs,
        ctx5: bool,
    ) -> Cooldowns {
        // RespondToLocality @1010: cooldownContainer HitTestInvisible for a local player, else Hidden.
        let cc = t.find_in(w, "cooldownContainer");
        t.widget_mut(cc).vis = vis_if(b.is_local, Vis::HitTestInvisible, Vis::Hidden);
        let mut slots = Vec::new();
        for i in 0..6 {
            let c = t.find_in(w, &format!("UI_IGv3_Cooldown{i}"));
            match abilities.get(i) {
                Some(a) => slots.push(Cooldown::new(t, c, b, a, location, ui)),
                None => {
                    let v = t.find_in(c, "visibilityContainer");
                    t.widget_mut(v).vis = Vis::Collapsed;
                }
            }
        }
        // RespondToPreferences @662: Attached world-showCooldowns OR context 5, Fixed hud-showCooldowns ->
        // visibilityPreference HitTestInvisible, else Hidden (@899 / @938); each UI_IGv3_Cooldown's own ran in its
        // constructor.
        let show = match location {
            LOCATION_ATTACHED => Some(ui.world_show_cooldowns || ctx5),
            LOCATION_FIXED => Some(ui.hud_show_cooldowns),
            _ => None,
        };
        if let Some(show) = show {
            let p = t.find_in(w, "visibilityPreference");
            t.widget_mut(p).vis = vis_if(show, Vis::HitTestInvisible, Vis::Hidden);
        }
        Cooldowns { slots }
    }

    /// UI_Broker SetCooldown...ForPlayer (Comp_Pawn_AbilityManager UpdateLocalUI / ClearCooldownClockMulticast).
    /// row: the slot's row now (the sim's `ability_row`), for a create.
    #[allow(clippy::too_many_arguments)]
    pub fn ability_op(
        &mut self,
        t: &mut Tree,
        n: i32,
        op: &HudAbilityOp,
        row: Option<&HudAbility>,
        now: f64,
        real: f64,
        sounds: &mut Vec<String>,
    ) {
        let Some(c) = usize::try_from(n).ok().and_then(|n| self.slots.get_mut(n)) else { return };
        match op {
            HudAbilityOp::Create { icon, max_count } => {
                if let Some(row) = row {
                    c.create(t, row, icon, *max_count);
                }
            }
            HudAbilityOp::Op(UiAbilityOp::Exit) => c.play_exit(t, real, sounds),
            HudAbilityOp::Op(UiAbilityOp::Error) => c.play_error(real),
            HudAbilityOp::Op(UiAbilityOp::Update { ammo, cd, cd_max, ammo_cd, ammo_max, pulse, pulse_ammo }) => {
                c.update(
                    t,
                    (f64::from(*ammo), f64::from(*cd), f64::from(*cd_max), f64::from(*ammo_cd), f64::from(*ammo_max)),
                    *pulse,
                    *pulse_ammo,
                    now,
                    real,
                    sounds,
                );
            }
        }
    }

    pub fn tick(&mut self, t: &mut Tree, a: &Res, now: f64, real: f64) {
        for c in &mut self.slots {
            c.tick(t, &a.cooldown, now, real);
        }
    }
}

// ---------- UI_IGv3_AttachedBottom ----------

/// UI_IGv3_AttachedBottom.
#[derive(Debug)]
pub struct AttachedBottom {
    pub t: Tree,
    percentages: [Percentage; 2],
    meters: [DodgeMeter; 2],
    flame: Flame,
    charge: Charge,
    marker_parts: [WidgetId; 2],
    marker_color: Rgba,
    name_bg: WidgetId,
    team: Rgba,
    /// The last SetGreyPercentage.
    grey: i32,
    cooldowns: Cooldowns,
    /// UI_Broker "Set Custom UIFor Player": the pawn's own widget in customContainer (None: none).
    custom: Option<FinnCharge>,
    /// UI_IGv3_PlayerMarker.
    marker: WidgetId,
    /// The sim's count of ShowArmorUnavailableForPlayer calls last seen (`evade.armor_unavailable`).
    armor_unavailable: i32,
    /// Real time DodgeMeterEmpty started (-1: not playing).
    meter_empty: f64,
}

impl AttachedBottom {
    /// proto: UI_IGv3_AttachedBottom instantiated (Hud::new); custom: the custom UI of the fighter's pawn and its
    /// root in `proto`, which then holds it in customContainer (OnCustomUI @1075, the broker's
    /// OnCustomUIForPlayerChanged: Remove Custom UI(false), then, for a local player's valid widget,
    /// customContainer.AddChildToOverlay with the slot's defaults: Left / Top, no padding; the pawn makes the widget
    /// only under a PlayerController, so a bot has none). abilities: the fighter's Abilities. names:
    /// GetLocalPlayUsername's texts.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        proto: &Tree,
        custom: Option<(&CustomUi, WidgetId)>,
        b: &BrokerPlayer,
        abilities: &[HudAbility],
        ui: &Prefs,
        ctx5: bool,
        names: &PlayerNumbers,
        res: &Res,
    ) -> AttachedBottom {
        let mut t = proto.clone();
        let t = &mut t;
        let r = t.root();
        // Construct: tempCustom.RemoveFromParent.
        let temp = t.find("tempCustom");
        t.remove(temp);
        let custom = custom.map(|(c, w)| FinnCharge::new(t, w, c, res));
        let marker = t.find("UI_IGv3_PlayerMarker");
        let username = t.find("UI_IGv3_Username");
        let own_meter = t.find("UI_IGv3_DodgeMeter");
        let own_pct = t.find("UI_IGv3_Percentage");
        let marker_meter = t.find_in(marker, "UI_IGv3_DodgeMeter");
        let marker_pct = t.find_in(marker, "UI_IGv3_Percentage");
        // Cooldowns. UI Orientation Left (UI_IGv3_Base's class default), handed by AttachedBottom's PreConstruct to
        // every broker-aware child: each UI_IGv3_Cooldown's frameContainer.SetRenderShear((10, 0)) (@5094).
        let cds = t.find("UI_IGv3_Cooldowns");
        for i in 0..6 {
            let c = t.find_in(cds, &format!("UI_IGv3_Cooldown{i}"));
            let f = t.find_in(c, "frameContainer");
            t.widget_mut(f).transform.shear[0] = 10.0;
        }
        let cooldowns = Cooldowns::new(t, cds, b, abilities, LOCATION_ATTACHED, ui, ctx5);
        let fw = t.find("UI_IGv3_Flame");
        let flame = Flame::new(t, fw, b, LOCATION_ATTACHED, ui);
        let cw = t.find("UI_IGv3_Charge");
        let charge = Charge::new(t, cw, b);
        let percentages = [
            Percentage::new(t, own_pct, LOCATION_ATTACHED, ui, ctx5),
            Percentage::new(t, marker_pct, LOCATION_ATTACHED, ui, ctx5),
        ];
        // AttachedBottom's meter: Height 8, Width 75, Show Label false, Responds To Prefs; the marker's keeps the
        // class defaults until the marker's RespondToPreferences sets its height.
        let meters = [
            DodgeMeter::new(t, own_meter, 8.0, 75.0, false, true, LOCATION_ATTACHED, ui),
            DodgeMeter::new(t, marker_meter, 20.0, -1.0, true, false, LOCATION_ATTACHED, ui),
        ];
        let marker_parts = [t.find_in(marker, "rectangle"), t.find_in(marker, "triangle")];
        let name_bg = t.find_in(username, "bg");
        let mut h = AttachedBottom {
            t: Tree::default(),
            percentages,
            meters,
            flame,
            charge,
            marker_parts,
            marker_color: [0.0; 4],
            name_bg,
            team: [0.0; 4],
            grey: 0,
            cooldowns,
            custom,
            marker,
            armor_unavailable: 0,
            meter_empty: -1.0,
        };
        h.color_parts(t, b);

        // Username
        let nl = t.find_in(username, "nameLabel");
        t.widget_mut(nl).text = b.username.clone();
        for n in ["bg", "shadow"] {
            let w = t.find_in(username, n);
            t.widget_mut(w).transform.scale[0] = if b.is_ally { 1.0 } else { -1.0 };
        }
        // UI_IGv3_Username RespondToPreferences (@1215): world-showUsername -> visbilityPreference
        // SelfHitTestInvisible, else Hidden.
        let up = t.find_in(username, "visbilityPreference");
        t.widget_mut(up).vis = vis_if(ui.world_show_username, Vis::SelfHitTestInvisible, Vis::Hidden);

        // SetupInitialValues of the marker (@654-736) and of its meters: the "P{n}" texts.
        let large = t.find_in(marker, "playerNumberLabelLarge");
        let name = names.local_play_username(b.index);
        t.widget_mut(large).text = name.short.clone();
        for m in &h.meters {
            m.set_player(t, &name);
        }
        h.marker_prefs(t, b, ui, ctx5, marker);

        // AttachedBottom RespondToLocality (@1570): Should Show Percentage? and the bottom container's height.
        let show_own_pct = if ctx5 && !b.is_me {
            true
        } else if !ui.world_show_percentage || b.is_me {
            false
        } else if b.is_bot {
            true
        } else {
            !b.is_local
        };
        t.widget_mut(own_pct).vis = vis_if(show_own_pct, Vis::SelfHitTestInvisible, Vis::Hidden);
        let show_self = should_show_self(b, ctx5);
        t.widget_mut(username).vis = vis_if(show_self, Vis::Hidden, Vis::SelfHitTestInvisible);
        let username_visible = !show_self;
        let bc = t.find("bottomContainer");
        let y = if !show_self {
            145.0
        } else if username_visible {
            190.0
        } else if ui.world_show_percentage && b.is_local {
            140.0
        } else {
            120.0
        };
        t.widget_mut(bc).transform.translation[1] = y;
        // UpdateDodgeMeterPosition (@2924): the meter Hidden under a shown marker, else SelfHitTestInvisible; Y 70
        // with world-showUsername, else 32.
        let om = t.widget_mut(own_meter);
        om.vis = vis_if(show_self, Vis::Hidden, Vis::SelfHitTestInvisible);
        om.transform.translation[1] = if ui.world_show_username { 70.0 } else { 32.0 };
        // OnHeight: the broker's Height is never set (0). UpdateDebugVisibility: DebugHiddenHUD false.
        let root = t.widget_mut(r);
        root.transform.translation[1] = 0.0;
        root.vis = Vis::SelfHitTestInvisible;

        // SetupInitialValues: OnPercent(0), OnEvadeDecay(1).
        for p in &mut h.percentages {
            p.set_percentage(t, 0, 0.0, 0.0);
        }
        h.flame.on_percentage(t, 0);
        for m in &mut h.meters {
            m.on_evade_decay(t, 1.0, -1.0);
        }
        h.t = std::mem::take(t);
        h
    }

    /// UI_IGv3_PlayerMarker RespondToPreferences (@4159 -> @3650) and its RespondToLocality (@782).
    fn marker_prefs(&self, t: &mut Tree, b: &BrokerPlayer, ui: &Prefs, ctx5: bool, marker: WidgetId) {
        let show_pct = ui.world_show_percentage || ctx5;
        let rects = [t.find_in(marker, "rectangle"), t.find_in(marker, "rectangle_1")];
        for &w in &rects {
            let w = t.widget_mut(w);
            (w.brush_w, w.brush_h) = if show_pct { (95.0, 0.0) } else { (60.0, 30.0) };
        }
        self.meters[1].set_height(t, if show_pct { 20.0 } else { 15.0 });
        t.widget_mut(marker).vis = vis_if(should_show_self(b, ctx5), Vis::Visible, Vis::Hidden);
        let mp = t.find_in(marker, "UI_IGv3_Percentage");
        t.widget_mut(mp).vis = vis_if(show_pct, Vis::HitTestInvisible, Vis::Collapsed);
        self.meters[1].set_show_label(t, b.is_local && show_pct && !b.is_me);
        let big = b.is_local && !show_pct && !b.is_me;
        let large = t.find_in(marker, "playerNumberLabelLarge");
        t.widget_mut(large).vis = vis_if(big, Vis::Visible, Vis::Collapsed);
        if big {
            for &w in &rects {
                let w = t.widget_mut(w);
                (w.brush_w, w.brush_h) = (65.0, 40.0);
            }
        }
    }

    /// The OnColor of the percentages, meters, player marker and username.
    fn color_parts(&mut self, t: &mut Tree, b: &BrokerPlayer) {
        for p in &mut self.percentages {
            p.on_color(t, b.team, b.focus, b.is_me);
        }
        for m in &self.meters {
            m.on_color(t, b.team);
        }
        // SelectColor(focus, team, Is Me?): the marker's Focus Color (@395)
        self.marker_color = if b.is_me { b.focus } else { b.team };
        self.team = b.team;
        let mc = if self.grey == 0 { self.marker_color } else { GREY_MARKER };
        for w in self.marker_parts {
            t.widget_mut(w).color = mc;
        }
        t.widget_mut(self.name_bg).color = if self.grey == 0 { self.team } else { GREY_NAME };
    }

    /// UI_Broker SetPercentageForPlayer.
    pub fn set_percentage(&mut self, p: i32, now: f64, real: f64) {
        for x in &mut self.percentages {
            x.set_percentage(&mut self.t, p, now, real);
        }
        self.flame.on_percentage(&mut self.t, p);
    }

    /// UI_Broker SetGreyPercentageForPlayer: both percentages; UI_IGv3_PlayerMarker SetGreyPercentage @2868 and
    /// UI_IGv3_Username OnGreyPercent @1954, neither comparing with the value it had.
    pub fn set_grey_percentage(&mut self, g: i32, real: f64) {
        self.grey = g;
        for x in &mut self.percentages {
            x.set_grey_percentage(&mut self.t, g, real);
        }
        let mc = if g == 0 { self.marker_color } else { GREY_MARKER };
        for w in self.marker_parts {
            self.t.widget_mut(w).color = mc;
        }
        self.t.widget_mut(self.name_bg).color = if g == 0 { self.team } else { GREY_NAME };
    }

    /// UI_Broker SetChargeAttackForPlayer.
    pub fn set_charge_attack(&mut self, start: f64, total: f64, reset: bool, specific: f64) {
        self.charge.on_charge_attack(start, total, reset, specific);
    }

    /// UI_Broker SetEvadeDecayForPlayer.
    pub fn set_evade_decay(&mut self, d: f64, now: f64) {
        for m in &mut self.meters {
            m.on_evade_decay(&mut self.t, d, now);
        }
    }

    /// UI_Broker SetVisibilityForPlayer (BaseFighter ShowOrHideUI): AttachedBottom OnVisibility.
    pub fn set_visible(&mut self, v: bool) {
        let r = self.t.root();
        self.t.widget_mut(r).vis = if v { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
    }

    /// UI_Broker SetCooldown...ForPlayer. row: the slot's row now, for a create.
    #[allow(clippy::too_many_arguments)]
    pub fn ability_op(
        &mut self,
        n: i32,
        op: &HudAbilityOp,
        row: Option<&HudAbility>,
        now: f64,
        real: f64,
        sounds: &mut Vec<String>,
    ) {
        self.cooldowns.ability_op(&mut self.t, n, op, row, now, real, sounds);
    }

    /// The pawn's own feed of its custom UI, after each step. real: the widget's real time.
    pub fn observe_custom(&mut self, w: &mvs_sim::World, f: mvs_sim::fighter::FighterId, real: f64, res: &Res) {
        if let Some(c) = &mut self.custom {
            c.observe(&mut self.t, w, f, real, res);
        }
        // UI_Broker OnShowArmorUnavailableForPlayer -> UI_IGv3_PlayerMarker OnArmorUnavailable (@3482-3602): for its
        // own Player Index, PlayAnimation(DodgeMeterEmpty, 0, 1, Forward, 4.5); a call while it plays starts it
        // again from 0.
        let n = w.fighter(f).evade.armor_unavailable;
        if n != self.armor_unavailable {
            self.armor_unavailable = n;
            self.meter_empty = real;
        }
    }

    /// DodgeMeterEmpty on the marker at playback speed 4.5 (real time); a finished player holds its last frame.
    fn tick_meter_empty(&mut self, a: &Res, real: f64) {
        if self.meter_empty < 0.0 {
            return;
        }
        let anim = &a.marker_meter_empty;
        let el = (real - self.meter_empty) * 4.5;
        if el >= anim.end() {
            anim.at(&mut self.t, self.marker, anim.end());
            self.meter_empty = -1.0;
        } else {
            anim.at(&mut self.t, self.marker, el.max(0.0));
        }
    }

    /// A value the HUD feeds the custom UI's run bar itself (Iron Giant's rage, Wonder Woman's meter).
    pub fn custom_value(&mut self, fraction: f64, real: f64) {
        if let Some(c) = &mut self.custom {
            c.observe_value(&mut self.t, fraction, real);
        }
    }

    pub fn tick(&mut self, a: &Res, now: f64, real: f64, facing_left: bool) {
        let t = &mut self.t;
        self.charge.tick(t, a, now, real, facing_left);
        if let Some(c) = &mut self.custom {
            c.tick(t, a, real);
        }
        self.cooldowns.tick(t, a, now, real);
        for p in &mut self.percentages {
            p.tick(t, a, now, real);
        }
        for m in &mut self.meters {
            m.tick(t, now);
        }
        self.tick_meter_empty(a, real);
    }

    /// The widget in its DrawSize box (400 x 110) centred (pivot 0.5) on the projected point, in Slate units scaled
    /// by the DPI scale: SWorldWidgetScreenLayer's Tick (142617ff0) gives the canvas slot Offset = the viewport
    /// position and the component's DrawSize, Alignment = Pivot.
    /// -> whether its root was painted.
    pub fn draw(&mut self, fonts: &mut Fonts, out: &mut ElementList, x: i32, y: i32, dpi: f64) -> bool {
        draw_box(&mut self.t, fonts, out, x, y, [400.0, 110.0], dpi)
    }
}

/// A screen-space widget component's widget of DrawSize `size` centred on the pixel (x, y). -> whether its root
/// was painted.
pub fn draw_box(
    t: &mut Tree,
    fonts: &mut Fonts,
    out: &mut ElementList,
    x: i32,
    y: i32,
    size: [f64; 2],
    dpi: f64,
) -> bool {
    let pos = [f64::from(x) / dpi - size[0] / 2.0, f64::from(y) / dpi - size[1] / 2.0];
    layout(t, fonts, Geometry { pos, size }, dpi);
    paint(t, fonts, out, 1.0);
    t.widget(t.root()).shown()
}

/// UI_IGv3_PlayerMarker Should Show Self?
fn should_show_self(b: &BrokerPlayer, ctx5: bool) -> bool {
    !(ctx5 && !b.is_me) && !b.is_bot && (b.is_me || b.is_local)
}

/// A class instantiated once, the prototype its widgets are cloned from.
pub fn proto(b: &Bundle, class: &str, name: &str) -> Tree {
    instantiate(b, class, name)
}

/// DefaultEngine.ini UIScaleRule ShortestSide with the UIScaleCurve keys (linear).
const DPI_KEYS: [(f64, f64); 5] = [(0.0, 0.0), (1080.0, 0.75), (1365.51, 1.0), (1441.0, 1.0), (8640.0, 8.0)];

pub fn dpi_scale(width: f64, height: f64) -> f64 {
    let s = width.min(height);
    for k in DPI_KEYS.windows(2) {
        let ((x0, y0), (x1, y1)) = (k[0], k[1]);
        if s <= x1 {
            return y0 + ((s - x0) / (x1 - x0)) * (y1 - y0);
        }
    }
    DPI_KEYS[4].1
}

#[cfg(test)]
mod tests {
    use super::dpi_scale;

    #[test]
    fn dpi_curve() {
        assert_eq!(dpi_scale(1920.0, 1080.0), 0.75);
        assert_eq!(dpi_scale(2560.0, 1440.0), 1.0);
        assert!((dpi_scale(1280.0, 720.0) - 0.5).abs() < 1e-12);
        assert_eq!(dpi_scale(8640.0 * 2.0, 9000.0), 8.0);
    }
}
