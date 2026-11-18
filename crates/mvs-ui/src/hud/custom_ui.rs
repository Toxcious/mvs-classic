//! The custom UI a fighter's pawn makes for the broker under its Attached UI v3 Bottom: UI_FinnCharge as Iron Giant,
//! Wonder Woman, Superman, Taz, Finn, Velma and Gizmo use it, and each pawn's feed of it, read off the World after each
//! step.

use mvs_sim::World;
use mvs_sim::fighter::FighterId;

use crate::bundle::Rgba;
use crate::slate::{Tree, Vis, WidgetId};

use super::Res;

/// The widget the pawns make and its name (Pawn_C017's MeterUI).
pub const CLASS: &str = "UI_FinnCharge";
pub const NAME: &str = "ThisUI IronGiant Hover";

/// UpdateRunChargeProgressBar's marker colours (@442 / @494) and thresholds (@408-1549, float literals).
const FINN_OFF: Rgba = [0.119538, 0.165132, 0.191202, 1.0];
const FINN_ON: Rgba = [0.89627, 0.973446, 0.871367, 1.0];
const FINN_MARKERS: [(&str, f64); 5] = [
    ("WBP_QuadMarker_Left25", 0.25),
    ("WBP_QuadMarker_Left33", 0.33),
    ("WBP_QuadMarker_Left50", 0.5),
    ("WBP_QuadMarker_Left66", 0.66),
    ("WBP_QuadMarker_Left75", 0.75),
];
/// UpdateBarDivisionColors' markers, in the order their steps turn them on.
const FINN_RIGHT_MARKERS: [&str; 4] =
    ["WBP_QuadMarker_Right20", "WBP_QuadMarker_Right40", "WBP_QuadMarker_Right60", "WBP_QuadMarker_Right80"];
/// MaxChargeExplosion's colour at a full bar and below it (@1677).
const FINN_FULL: Rgba = [1.0, 0.401978, 0.000304, 1.0];
const FINN_NOT_FULL: Rgba = [0.048172, 0.061246, 0.088656, 0.0];
/// HighFiveExplosion's below a full bar (@2354: the same colour with alpha 1, the tree's own).
const FINN_NOT_FULL_HIGH: Rgba = [0.048172, 0.061246, 0.088656, 1.0];

/// A pawn's own feed of its widget (CustomUi.observe); Iron Giant's and Wonder Woman's are the HUD's
/// (`FinnCharge::observe_value`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feed {
    None,
    Ice,
    Eat,
    Finn,
    Velma,
    Gizmo,
}

/// How a character's pawn makes its UI_FinnCharge: the left icon (a texture of the hud bundle; none: the tree's
/// brush stays) and its render scale, the right one, the HandleBarSwitch(value) it calls after the create, whether
/// it makes it Collapsed, whether it shows MarkerContainer_Right, and its feed.
#[derive(Clone, Copy, Debug)]
pub struct CustomUi {
    pub icon: Option<&'static str>,
    pub icon_scale: Option<f64>,
    pub right_icon: Option<&'static str>,
    pub right_icon_scale: Option<f64>,
    pub bar_switch: Option<bool>,
    pub collapsed: bool,
    pub right_markers: bool,
    pub feed: Feed,
}

const NO_UI: CustomUi = CustomUi {
    icon: None,
    icon_scale: None,
    right_icon: None,
    right_icon_scale: None,
    bar_switch: None,
    collapsed: false,
    right_markers: false,
    feed: Feed::None,
};

/// Characters whose pawn makes a custom UI for the broker (Set Custom UIFor Player), by the port's character name.
///
/// Pawn_C017 @98-657 (0.25 s after BeginPlay, on a locally controlled pawn whose controller is a PlayerController):
/// Create(UI_FinnCharge), LeftSideIcon = C017_Rage_UI, RightSideIcon = C017_Fuel_UI, Set Custom UIFor Player,
/// HandleBarSwitch(true). UI_C017_Charge is not created by anything.
///
/// Pawn_WonderWomanV3 @749-1356 (from BroadcastPlayerValues @3337, while MeterUI is not valid; the same two tests):
/// Create(UI_FinnCharge) @922, LeftSideIcon = UI_WW_ShieldCharge @983 and LeftSideIcon.SetRenderScale(1, 1) @1051
/// (the tree's is 1.2), WidgetSwitcher_0.SetActiveWidget(WBP_QuadMarker_Left50) @1129, Set Custom UIFor Player
/// @1302, HandleBarSwitch(true) @616. WBP_QuadMarker_Left50 is no child of the switcher (its two children are
/// MarkerContainer_Left_50 and MarkerContainer_Left_33; the marker sits inside the first), so GetChildIndex gives
/// -1 and the Slate switcher gets the index clamped to 0: MarkerContainer_Left_50, the tree's own active child.
// Unverified: UWidgetSwitcher::SetActiveWidget's clamp is the 4.26 source's, not read in the exe.
///
/// Pawn_Superman @30-498 (0.25 s after BeginPlay @1046, retried every 0.1 s while the pawn is not locally
/// controlled @537; the same two tests): Create(UI_FinnCharge) @163, RightSideIcon = LeftSideIcon = UI_Debuff_Chill
/// @243 / @311 (MatchSize false), Set Custom UIFor Player @444, SetVisibility(Collapsed) @498. It never calls
/// HandleBarSwitch, so the tree's own visibilities stand.
///
/// Pawn_C015 (Taz) @7334-7852: Create(UI_FinnCharge) @7507, RightSideIcon = UI_AbilityIcons_C015_BurpCloud
/// (MatchSize false) @7568 and its render scale (1, 1) @7636, Set Custom UIFor Player @7779, MeterUI @7833, then
/// @6438 HandleBarSwitch(false) and UpdateHighFiveProgressBar(EatLevel) @6475.
///
/// Pawn_Finn @15-543 (RightSideIcon = UI_icon_finn_highfive @228, no HandleBarSwitch); Pawn_Velma @333-916
/// (RightSideIcon = ui_icons_search @489, scale 0.8 @557, MarkerContainer_Right SelfHitTestInvisible @635,
/// HandleBarSwitch(false) @833); Pawn_C023A @35-434 (LeftSideIcon = UI_AbilityIcons_C023A_Attach @210,
/// HandleBarSwitch(true) @397, SetVisibility(Collapsed) @434).
// Deviation: the pawns' Delays before the create (0.25 s, 1 s, the 0.1 s retries) are not kept: the widget is there
// from the HUD's start, which a match's intro covers.
pub fn custom_ui(character: &str) -> Option<&'static CustomUi> {
    const IRON_GIANT: CustomUi = CustomUi { icon: Some("C017_Rage_UI"), bar_switch: Some(true), ..NO_UI };
    const WONDER_WOMAN: CustomUi =
        CustomUi { icon: Some("UI_WW_ShieldCharge"), icon_scale: Some(1.0), bar_switch: Some(true), ..NO_UI };
    const SUPERMAN: CustomUi = CustomUi {
        icon: Some("UI_Debuff_Chill"),
        right_icon: Some("UI_Debuff_Chill"),
        collapsed: true,
        feed: Feed::Ice,
        ..NO_UI
    };
    const TAZ: CustomUi = CustomUi {
        right_icon: Some("UI_AbilityIcons_C015_BurpCloud"),
        right_icon_scale: Some(1.0),
        bar_switch: Some(false),
        feed: Feed::Eat,
        ..NO_UI
    };
    const FINN: CustomUi = CustomUi { right_icon: Some("UI_icon_finn_highfive"), feed: Feed::Finn, ..NO_UI };
    const VELMA: CustomUi = CustomUi {
        right_icon: Some("ui_icons_search"),
        right_icon_scale: Some(0.8),
        right_markers: true,
        bar_switch: Some(false),
        feed: Feed::Velma,
        ..NO_UI
    };
    const GIZMO: CustomUi = CustomUi {
        icon: Some("UI_AbilityIcons_C023A_Attach"),
        bar_switch: Some(true),
        collapsed: true,
        feed: Feed::Gizmo,
        ..NO_UI
    };
    Some(match character {
        "irongiant" => &IRON_GIANT,
        "wonderwoman" => &WONDER_WOMAN,
        "superman" => &SUPERMAN,
        "taz" => &TAZ,
        "finn" => &FINN,
        "velma" => &VELMA,
        "gizmo" => &GIZMO,
        _ => return None,
    })
}

/// What gizmoUi last saw of Comp_C023A_AllyAttach.
#[derive(Clone, Copy, Debug)]
struct GizmoSeen {
    /// The last Toggle_UI_Bar.
    on: bool,
    /// The last UpdateRunChargeProgressBar value.
    meter: f64,
    timer_next: f64,
}

/// UI_FinnCharge. As Pawn_C017 uses it: after HandleBarSwitch(true) only the RunMeter shows, fed by
/// Comp_C017_Hover_Rage SetRageLevelUIClient(rage / max) -> UpdateRunChargeProgressBar; Pawn_WonderWomanV3 the same
/// way, fed by MulticastMeterValue @3361-3388 (MeterPercentage); the others by their feeds below.
#[derive(Debug)]
pub struct FinnCharge {
    pub root: WidgetId,
    bar: WidgetId,
    high: WidgetId,
    markers: [WidgetId; 5],
    right_markers: [WidgetId; 4],
    explosion: WidgetId,
    high_explosion: WidgetId,
    switcher: WidgetId,
    overlay: WidgetId,
    run_meter: WidgetId,
    high_meter: WidgetId,
    /// RightSideIcon, for a pawn's later SetBrushFromTexture (Taz's UpdateMeterIcon).
    right: WidgetId,
    /// Real times: MaxCharge and ExplosionPulse loop (PlayAnimation loops 0) until StopAnimation; BarShake plays
    /// once. -1: not playing.
    max_start: f64,
    pulse_start: f64,
    shake_start: f64,
    value: f64,
    feed: Feed,
    /// A pawn's feed: the last value it sent (None: none yet) and the count of its calls it has seen.
    fed: Option<f64>,
    calls: i64,
    /// The last UpdateHighFiveProgressBar value a feed sent (Construct's 0 first).
    fed_high: f64,
    /// The last HandleBarSwitch a feed made (false: the tree's RunMeter Collapsed).
    run_on: bool,
    /// Taz's icon swap made.
    right_set: bool,
    gizmo: Option<GizmoSeen>,
}

impl FinnCharge {
    /// The widget `root` (made into the AttachedBottom's customContainer) set up as the character's pawn does.
    pub fn new(t: &mut Tree, root: WidgetId, ui: &CustomUi, res: &Res) -> FinnCharge {
        let f = |t: &Tree, n: &str| t.find_in(root, n);
        let bar = f(t, "WBP_QuadProgress");
        t.dynamic_material(bar);
        let high = f(t, "WBP_QuadProgress_1");
        t.dynamic_material(high);
        let mut c = FinnCharge {
            root,
            bar,
            high,
            markers: FINN_MARKERS.map(|(n, _)| f(t, n)),
            right_markers: FINN_RIGHT_MARKERS.map(|n| f(t, n)),
            explosion: f(t, "MaxChargeExplosion"),
            high_explosion: f(t, "HighFiveExplosion"),
            switcher: f(t, "WidgetSwitcher_0"),
            overlay: f(t, "Overlay_0"),
            run_meter: f(t, "RunMeter"),
            high_meter: f(t, "HighFiveMeter"),
            right: f(t, "RightSideIcon"),
            max_start: -1.0,
            pulse_start: -1.0,
            shake_start: -1.0,
            value: -1.0,
            feed: ui.feed,
            fed: None,
            calls: -1,
            fed_high: 0.0,
            run_on: false,
            right_set: false,
            gizmo: None,
        };
        // SetBrushFromTexture(texture, MatchSize false) keeps the brush's 256 x 256.
        let left = f(t, "LeftSideIcon");
        for (w, icon, scale) in [(left, ui.icon, ui.icon_scale), (c.right, ui.right_icon, ui.right_icon_scale)] {
            if let Some(p) = icon.and_then(|n| res.brush(n)) {
                t.widget_mut(w).tex = Some(p);
            }
            if let Some(s) = scale {
                t.widget_mut(w).transform.scale = [s, s];
            }
        }
        if let Some(on) = ui.bar_switch {
            c.bar_switch(t, on);
        }
        if ui.collapsed {
            t.widget_mut(root).vis = Vis::Collapsed;
        }
        if ui.right_markers {
            let m = f(t, "MarkerContainer_Right");
            t.widget_mut(m).vis = Vis::SelfHitTestInvisible;
        }
        // Construct @63: UpdateRunChargeProgressBar(0), UpdateHighFiveProgressBar(0)
        c.update(t, 0.0, 0.0);
        c.update_high_five(t, 0.0, 0.0);
        c
    }

    /// UpdateHighFiveProgressBar @2092: the HighFiveMeter's bar and the star behind its icon.
    /// UpdateBarDivisionColors @2292 writes WBP_QuadMarker_Right20 .. 80 alone (`divisions`); their
    /// MarkerContainer_Right is Collapsed in the tree and Pawn_Velma alone shows it (@635). ExplosionPulse (@2553 at
    /// exactly 1: PlayAnimation(ExplosionPulse, 0, loops 0) from its start at every call; @2601 below it
    /// StopAnimation, which leaves the star at its last evaluated pose) turns and scales the star.
    pub fn update_high_five(&mut self, t: &mut Tree, v: f64, real: f64) {
        let m = t.dynamic_material(self.high);
        m.set_scalar("Percentage", v as f32);
        m.set_scalar("DeltaPercentage", v as f32);
        let full = (v - 1.0).abs() <= 1e-6;
        t.widget_mut(self.high_explosion).color = if full { FINN_FULL } else { FINN_NOT_FULL_HIGH };
        self.divisions(t, v);
        self.pulse_start = if full { real } else { -1.0 };
    }

    /// UpdateBarDivisionColors (UI_FinnCharge @0-1997), NearlyEqual(Value, x, 1e-6) per step: at 0 the four
    /// markers OFF, SelfHitTestInvisible, opacity 1; at 0.2 Right20 ON; at 0.4 Right20 / 40 ON and Right20's opacity
    /// 0.5; at 0.6 the first three ON and the first two at 0.5; at 0.8 all four ON and the first three at 0.5; at 1
    /// all four Hidden. Any other value writes nothing.
    fn divisions(&self, t: &mut Tree, v: f64) {
        let near = |x: f64| (v - x).abs() <= 1e-6;
        if near(0.0) {
            for &m in &self.right_markers {
                let w = t.widget_mut(m);
                w.color = FINN_OFF;
                w.vis = Vis::SelfHitTestInvisible;
                w.opacity = 1.0;
            }
            return;
        }
        if near(1.0) {
            for &m in &self.right_markers {
                t.widget_mut(m).vis = Vis::Hidden;
            }
            return;
        }
        for n in 1..=4usize {
            if !near(f64::from(mvs_sim::math::narrow(n as f64 / 5.0))) {
                continue;
            }
            for &m in &self.right_markers[..n] {
                t.widget_mut(m).color = FINN_ON;
            }
            for &m in &self.right_markers[..n - 1] {
                t.widget_mut(m).opacity = 0.5;
            }
            return;
        }
    }

    /// HandleBarSwitch(value) @2621: Conditional Visibility(RunMeter, value, SelfHitTestInvisible, Collapsed),
    /// Conditional Visibility(HighFiveMeter, value, Collapsed, SelfHitTestInvisible).
    fn bar_switch(&mut self, t: &mut Tree, on: bool) {
        self.run_on = on;
        t.widget_mut(self.run_meter).vis = if on { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        t.widget_mut(self.high_meter).vis = if on { Vis::Collapsed } else { Vis::SelfHitTestInvisible };
    }

    /// ShakeEvent @2740 -> @15: PlayAnimation(BarShake, 0, 1 loop, Forward, speed 1), from its start at every call.
    fn shake(&mut self, real: f64) {
        self.shake_start = real;
    }

    /// UpdateRunChargeProgressBar @208 (@102: a value above 0 calls HandleBarSwitch(true) while the HighFiveMeter's
    /// own visibility is not a visible one: after the first switch that changes nothing, and on a widget whose pawn
    /// never switched the HighFiveMeter is Visible, so no switch is made).
    pub fn update(&mut self, t: &mut Tree, v: f64, real: f64) {
        self.value = v;
        let m = t.dynamic_material(self.bar);
        m.set_scalar("Percentage", v as f32);
        m.set_scalar("DeltaPercentage", v as f32);
        // WBP_Quadrilateral UpdateColor @1668: Color, Image.SetColorAndOpacity(Color)
        for (w, (_, at)) in self.markers.iter().zip(FINN_MARKERS) {
            t.widget_mut(*w).color = if v >= f64::from(mvs_sim::math::narrow(at)) { FINN_ON } else { FINN_OFF };
        }
        // NearlyEqual(v, 1, 1e-6)
        let full = (v - 1.0).abs() <= 1e-6;
        t.widget_mut(self.explosion).color = if full { FINN_FULL } else { FINN_NOT_FULL };
        if full {
            // @1876: the markers Hidden, PlayAnimation(MaxCharge, 0, loops 0) from its start at every call
            t.widget_mut(self.switcher).vis = Vis::Hidden;
            self.max_start = real;
        } else {
            // @1962: SetRenderScale(1, 1), then StopAnimation(MaxCharge): the section completes with KeepState, so
            // the scale set here stands, as do the explosion's last rotation, scale and opacity 1 under its colour's
            // alpha 0.
            t.widget_mut(self.switcher).vis = Vis::SelfHitTestInvisible;
            t.widget_mut(self.overlay).transform.scale = [1.0, 1.0];
            self.max_start = -1.0;
        }
    }

    /// Comp_C017_Hover_Rage SetRageLevelUIClient(RageAmount / RageMaxAmount): from AddRage and ResetRage (@6227) and
    /// from each component tick in weapon mode (@3879); Pawn_WonderWomanV3 MulticastMeterValue(MeterPercentage)
    /// after each of the pawn's meter writes. Read as the fraction after each step and sent when it differs.
    // Deviation: a call that repeats the value (AddRage or a gain at a full meter) restarts MaxCharge in the build and
    // is not seen here.
    pub fn observe_value(&mut self, t: &mut Tree, fraction: f64, real: f64) {
        let v = mvs_sim::math::narrow(fraction);
        if f64::from(v) != self.value {
            self.update(t, f64::from(v), real);
        }
    }

    /// The pawn's own feed of the widget, read off the fighter after each step. real: the widget's real time.
    pub fn observe(&mut self, t: &mut Tree, w: &World, f: FighterId, real: f64, res: &Res) {
        match self.feed {
            Feed::None => {}
            Feed::Ice => self.ice(t, w, f, real),
            Feed::Eat => self.eat(t, w, f, real, res),
            Feed::Finn => self.finn(t, w, f, real),
            Feed::Velma => self.velma(t, w, f, real),
            Feed::Gizmo => self.gizmo(t, w, f, real),
        }
    }

    /// Pawn_C015's (Taz's) feed of MeterUI: SetEatLevelOwner(EatLevel) @10777 (a client RPC to the owner, from
    /// UpdateEatLevelServer with UpdateOwner) sets EatLevel and calls MeterUI.UpdateHighFiveProgressBar(EatLevel)
    /// @10804. The widget is made with EatLevel's class default 0 (@6475), so `fed` starts there. Pawn_C015 "Update
    /// Appearance" (the bar's colours and the LeftSideIcon at .5 / .99) has no caller in this build.
    /// UpdateMeterIcon (@881-924, from Buff_C015_EatProjectiles' BeginPlay, the EatProjectiles perk):
    /// MeterUI.RightSideIcon.SetBrushFromTexture(UI_AbilityIcons_C015_BurpAnvil, MatchSize false) once MeterUI is
    /// valid (a Delay(0.0) retry until then, @993); nothing sets the cloud back. Here: the first feed that finds the
    /// buff on him writes it, and the widget keeps it.
    // Deviation: read when the value differs; a call that repeats the value (a gain at a full meter) restarts
    // ExplosionPulse in the build and is not seen here.
    // Unverified: whether the swap is seen from the match's start (the retry against the widget's creation, which
    // comes from BroadcastPlayerValues) is not read; the port's widget shows it from its first feed.
    fn eat(&mut self, t: &mut Tree, w: &World, f: FighterId, real: f64, res: &Res) {
        let Some(s) = mvs_sim::chars::taz::state::taz_of(w, f) else { return };
        if !self.right_set
            && mvs_sim::combat::buffs::buff_of(w, f, mvs_sim::name!("Buff_C015_EatProjectiles_C")).is_some()
        {
            self.right_set = true;
            if let Some(p) = res.brush("UI_AbilityIcons_C015_BurpAnvil") {
                t.widget_mut(self.right).tex = Some(p);
            }
        }
        let fed = *self.fed.get_or_insert(0.0);
        if f64::from(s.eat_level) != fed {
            self.fed = Some(f64::from(s.eat_level));
            self.update_high_five(t, f64::from(s.eat_level), real);
        }
    }

    /// Pawn_Finn's feed. It binds TurnOnRunUi to the component's NewMomentumStarted (@415 / @438) and TurnOffRunUI
    /// to MomentumEnd (@479 / @502), both UpdateRunUI -> HandleBarSwitch(on) @7508 (FinnState.run_ui).
    /// Comp_FinnMomentumCharge feeds UpdateRunChargeProgressBar (UpdateChargeUI): the tick @1156-1747 while
    /// CalculateMomentum, stored below max and a speed above 0, Max == 0 ? -1 : clamp(stored / max, 0.01, 0.99);
    /// EndMomentum -1 after the MomentumEnd broadcast. The widget writes every value, -1 included. Read here: not
    /// calculating -1; calculating with stored above 0 the clamp; calculating with stored 0 (no tick fed yet) the
    /// last value stands. The high-five bar gets UpdateHighFiveProgressBar(HighFivePercent): sent when it differs.
    // Deviation: a call that repeats a value is not seen; an end and a start of momentum inside one tick read as one
    // start (the bar not reset to -1 between them); a tick that stops feeding at the max is read as its clamp.
    fn finn(&mut self, t: &mut Tree, w: &World, f: FighterId, real: f64) {
        let Some(s) = mvs_sim::chars::finn::state::finn_of(w, f) else { return };
        let m = &s.momentum;
        if s.run_ui != self.run_on {
            self.bar_switch(t, s.run_ui);
        }
        // Construct's UpdateRunChargeProgressBar(0)
        let fed = *self.fed.get_or_insert(0.0);
        let mut v = fed;
        if !m.calculating || m.max == 0.0 {
            v = -1.0;
        } else if m.stored > 0.0 {
            v = f64::from((m.stored / m.max).clamp(0.01, 0.99));
        }
        let v = mvs_sim::math::narrow(v);
        if f64::from(v) != fed {
            self.fed = Some(f64::from(v));
            self.update(t, f64::from(v), real);
        }
        let h = m.high_five_pct;
        if f64::from(h) != self.fed_high {
            self.fed_high = f64::from(h);
            self.update_high_five(t, f64::from(h), real);
        }
    }

    /// Pawn_Velma's feed (EvidenceUI): UpdateHighFiveProgressBar(EvidenceCount) @870 after the create, and
    /// BPI_SetEvidenceCount @1037 caches the count's fraction (VelmaState.evidence_ui) and calls it @1103. The widget
    /// is made with EvidenceCount's class default 0, so `fed` starts there.
    // Deviation: read when the value differs; a call that repeats it (a full count gaining again) restarts
    // ExplosionPulse in the build.
    fn velma(&mut self, t: &mut Tree, w: &World, f: FighterId, real: f64) {
        let fed = *self.fed.get_or_insert(0.0);
        let v = mvs_sim::chars::velma::state::velma_of(w, f).map_or(0.0, |s| s.evidence_ui);
        if f64::from(v) != fed {
            self.fed = Some(f64::from(v));
            self.update_high_five(t, f64::from(v), real);
        }
    }

    /// Pawn_Superman's feed ("UI IceBreath"). MulticastUpdateIceUI(BarPercent = float(IceCount) /
    /// float(IceMaxCount), IceCount) @3221, sent after every write of the count (BeginPlay @1248, a pellet @2420, a
    /// recharge point @920): UpdateHighFiveProgressBar @3248, then UpdateRunChargeProgressBar @3293, then the widget
    /// Collapsed at a percent of exactly 1 @3338-3386, else Visible @3425. BeginPlay's call (count 28) comes 0.25 s
    /// before the widget is made and reaches none, so the widget stays as it was made until the count's first
    /// change: `fed` starts at the full count. TryIceBreath @3520-3702 (Neutral + Special pressed at an ice count of
    /// exactly 0): ShakeEvent, counted by the sim (ice_shakes): the widget shakes when the count grows. A count that
    /// falls is a rollback's and shakes nothing.
    // Deviation: read once a tick: two writes inside one tick are one call here, with the same end state.
    fn ice(&mut self, t: &mut Tree, w: &World, f: FighterId, real: f64) {
        use mvs_sim::chars::superman::state::{ICE_MAX_COUNT, superman_of};
        let Some(s) = superman_of(w, f) else { return };
        let fed = *self.fed.get_or_insert(f64::from(ICE_MAX_COUNT));
        if f64::from(s.ice_count) != fed {
            self.fed = Some(f64::from(s.ice_count));
            let v = s.ice_count / ICE_MAX_COUNT;
            self.update_high_five(t, f64::from(v), real);
            self.update(t, f64::from(v), real);
            t.widget_mut(self.root).vis = if v == 1.0 { Vis::Collapsed } else { Vis::Visible };
        }
        let shakes = i64::from(s.ice_shakes);
        if self.calls >= 0 && shakes > self.calls {
            self.shake(real);
        }
        self.calls = shakes;
    }

    /// Gizmo's attach meter. Comp_C023A_AllyAttach feeds it:
    ///   - Toggle_UI_Bar(On) @3493-3752: UpdateRunChargeProgressBar(On ? 1 : 0), then SetVisibility(On ? Visible :
    ///     Collapsed); called at the attach @5247 (after TrackedMeter = 1 @5131) and at every detach @2344.
    ///   - UpdateMeter(Amount, PlayShake) @2999-3253: while IsAttached, TrackedMeter -= Amount,
    ///     UpdateRunChargeProgressBar(TrackedMeter) @3171, ShakeEvent @3253 when PlayShake. DamageTaken @2974 passes
    ///     the damage / KnockOffDamage with PlayShake true; AttachTimerTick @122 passes AttachTimerItervals /
    ///     AttachTime with false.
    ///
    /// Read off GizmoAttach after each step: `attached` for Toggle_UI_Bar, `meter` for UpdateMeter. The sim keeps no
    /// record of which call made a drop, so the shake is inferred: the drop less the timer's share (one
    /// AttachTimerTick when `timer_next` moved this tick).
    // Deviation: a hit of 0 damage (UpdateMeter(0, true): a shake, the meter unchanged) is not seen; two writes in one
    // tick are one update (the same end state).
    fn gizmo(&mut self, t: &mut Tree, w: &World, f: FighterId, real: f64) {
        // Comp_C023A_AllyAttach AttachTimerItervals / AttachTime: the amount one AttachTimerTick takes off.
        let timer_amount: f32 = 0.5 / 15.0;
        let Some(a) = mvs_sim::chars::gizmo::state::gizmo_of(w, f).map(|g| &g.attach) else { return };
        let s = self.gizmo.get_or_insert(GizmoSeen { on: false, meter: 0.0, timer_next: f64::from(a.timer_next) });
        if a.attached != s.on {
            // Toggle_UI_Bar(On)
            s.on = a.attached;
            s.meter = if a.attached { 1.0 } else { 0.0 };
            s.timer_next = f64::from(a.timer_next);
            let m = s.meter;
            self.update(t, m, real);
            t.widget_mut(self.root).vis = if a.attached { Vis::Visible } else { Vis::Collapsed };
            if !a.attached {
                return;
            }
        }
        let s = self.gizmo.as_mut().expect("set above");
        if !a.attached {
            return;
        }
        let timer = if f64::from(a.timer_next) != s.timer_next { timer_amount } else { 0.0 };
        s.timer_next = f64::from(a.timer_next);
        if f64::from(a.meter) == s.meter {
            return;
        }
        // UpdateMeter: the bar, then ShakeEvent for a damage share
        let shake = s.meter - f64::from(a.meter) - f64::from(timer) > 1e-6;
        s.meter = f64::from(a.meter);
        self.update(t, f64::from(a.meter), real);
        if shake {
            self.shake(real);
        }
    }

    /// The user widget's animations advance only while its own visibility is a visible one (Pawn_Superman's widget is
    /// Collapsed at a full meter, where MaxCharge was started @1914: it is stopped @2056 by the call that shows the
    /// widget again, so it never moves there). MaxCharge and BarShake both drive Overlay_0's transform; no user here
    /// has both playing (MaxCharge at 1, Pawn_Superman's shake at an ice count of 0), so their blend is not written.
    pub fn tick(&mut self, t: &mut Tree, res: &Res, real: f64) {
        if !t.widget(self.root).shown() {
            return;
        }
        let r = self.root;
        if self.max_start >= 0.0 {
            let a = &res.finn_max;
            a.at(t, r, (real - self.max_start) % a.end());
        }
        if self.shake_start >= 0.0 {
            // one loop: the last evaluation is at the end (Overlay_0 back at translation (0, -20), scale 1)
            let a = &res.finn_shake;
            let el = real - self.shake_start;
            a.at(t, r, el.min(a.end()));
            if el >= a.end() {
                self.shake_start = -1.0;
            }
        }
        // ExplosionPulse drives HighFiveExplosion alone, which the other two do not touch.
        if self.pulse_start >= 0.0 {
            let a = &res.finn_pulse;
            a.at(t, r, (real - self.pulse_start) % a.end());
        }
    }
}
