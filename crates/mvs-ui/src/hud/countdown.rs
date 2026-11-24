//! UI_Countdown: the versus card PandaGameState shows before a match. StartCountdown fills the
//! texts and plays BackgroundIn (6.75 s); the animation's event keys post the card sound and count 3, 2, 1, GO!;
//! PandaGameInstance hides the card 6.65 s after StartCountdown and the widget destroys itself when the animation
//! ends.

use serde_json::Value;

use crate::bundle::{Bundle, Rgba};
use crate::element::ElementList;
use crate::slate::{Geometry, Tree, Vis, WidgetId, instantiate, layout, paint};
use crate::text::{Fonts, GameText};

use super::common::Anim;
use super::custom_text::{WHEEL_RULE, stock_win_condition};
use super::game_text;

/// PandaGameInstance: the Delay chain from BPI_ReportStartedCountdown to HideCountdown (2.5 + 3.65 + 0.5 s).
const HIDE_AT: f64 = 6.65;

/// The win condition line's FText by TeamStyle 0 (1v1), 1 (2v2), 2 (Free For All): UI_Countdown's ubergraph
/// EX_TextConst @2885, @3201, @2586.
const WIN_CONDITION: [&str; 3] =
    ["EEC142474D744F6165B50894AEA13623", "11E20DA64250F4D272E1DC947FCED463", "999BEFFE475D2687C5D2559D9D0B39AD"];
/// The VS numbers' FText by TeamStyle: @420 and @645; Free For All's (@870) is an empty text.
const VS_NUMBER: [Option<&str>; 3] =
    [Some("FF5C2F8342ECA91A647F1D841C047488"), Some("3FC4A4774A3A8225D8F94CB0E45F660B"), None];
/// VS_Text in Free For All: @1055.
const FFA: &str = "B3A5B1844653D3F99BAEC787B9E7478C";
/// DecrementCountdown @152.
const GO: &str = "4ED0178B4B58E0352B023E9CE896C52A";

fn color(v: &Value) -> Rgba {
    let c = &v["SpecifiedColor"];
    ["R", "G", "B", "A"].map(|k| c[k].as_f64().unwrap_or(0.0))
}

/// UI_DetailedText SetupParameters: three stacked text blocks (main text, background outline, drop shadow) from the
/// instance's parameters.
fn setup_detailed_text(t: &mut Tree, w: WidgetId, text: &str) {
    let p = t.props(w).clone();
    let num = |k: &str| p[k].as_f64().unwrap_or(0.0);
    let inner = num("InnerOutlineThickness");
    let size = num("FontSize");
    let layers: [(&str, Rgba, Option<f64>, Rgba); 3] = [
        ("MainText", color(&p["FontColor"]), Some(inner), color(&p["InnerOutlineColor"])),
        (
            "BackgroundOutline",
            color(&p["OuterOutlineColor"]),
            Some(inner + num("OuterOutlineThickness")),
            color(&p["OuterOutlineColor"]),
        ),
        // (keeps its outline size)
        ("DropShadow", color(&p["DropShadowColor"]), None, color(&p["DropShadowColor"])),
    ];
    for (name, c, outline, outline_color) in layers {
        let x = t.find_in(w, name);
        let x = t.widget_mut(x);
        x.color = c;
        if let Some(o) = outline {
            x.outline = o;
        }
        x.outline_color = outline_color;
        x.font_size = size;
        x.letter_spacing = 0.0;
        x.text = text.to_string();
    }
    let shadow = t.find_in(w, "DropShadow");
    let off = ((size * num("DropShadowThickness")) / 100.0 + 0.5).floor().clamp(1.0, 999.0);
    let s = t.widget_mut(shadow);
    s.offsets.l = off;
    s.offsets.t = off;
    s.vis = if p["EnableDropShadow"].as_bool().unwrap_or(false) { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
    let bg = t.find_in(w, "BackgroundOutline");
    t.widget_mut(bg).vis = if p["EnableBackgroundOutline"].as_bool().unwrap_or(false) {
        Vis::SelfHitTestInvisible
    } else {
        Vis::Collapsed
    };
}

/// UI_Countdown.
#[derive(Debug)]
pub struct Countdown {
    pub t: Tree,
    canvas: WidgetId,
    anim: Anim,
    go: String,
    /// The real time of StartCountdown (-1: not yet).
    start: f64,
    /// `start` given by `begin`, not taken from a draw.
    begun: bool,
    last: f64,
    /// The event keys passed.
    fired: usize,
    /// NewAnimCount.
    count: i32,
    pub done: bool,
}

impl Countdown {
    /// style: the match's TeamStyle (0..2); stocks: the experimental stock rule (knockouts_to_win is each player's
    /// lives); wheel: (the port's own) the Chaos Wheel, named on a line under the win condition; stage: an arcade
    /// round's "Stage N of M".
    // Unverified: no rematch (DetermineScore's set score): the native game has none yet.
    pub fn new(
        b: &Bundle,
        text: &GameText,
        style: usize,
        knockouts_to_win: i32,
        stocks: bool,
        wheel: bool,
        stage: Option<&str>,
    ) -> Countdown {
        let mut t = instantiate(b, "UI_Countdown", "UI_Countdown");
        let anim = Anim::of(b, "UI_Countdown", "BackgroundIn");
        let canvas = t.find("CanvasPanel");
        // StartCountdown.
        t.widget_mut(canvas).opacity = 1.0;
        // WinCondition: TextToUpper of the Format result.
        let mut line = if stocks {
            stock_win_condition(style, knockouts_to_win)
        } else {
            game_text(text, WIN_CONDITION[style]).replace("{int}", &knockouts_to_win.to_string())
        };
        if wheel {
            line = format!("{line}\n{WHEEL_RULE}");
        }
        let win = t.find("WinCondition");
        t.widget_mut(win).text = line.to_uppercase();
        // In arcade (IsArcadeModeActive @3506): WinCondition = "Stage {Round} of {NumRounds}" (@4096, @4201; not
        // upper-cased), ScoreContainer Hidden (@4246), WinCondition SelfHitTestInvisible (@4284).
        if let Some(s) = stage {
            let w = t.widget_mut(win);
            w.text = s.to_string();
            w.vis = Vis::SelfHitTestInvisible;
            let sc = t.find("ScoreContainer");
            t.widget_mut(sc).vis = Vis::Hidden;
        }
        let vs = VS_NUMBER[style].map(|k| game_text(text, k)).unwrap_or_default();
        for n in ["VS_Number_Left", "VS_Number_Right"] {
            let w = t.find(n);
            setup_detailed_text(&mut t, w, &vs);
        }
        if style == 2 {
            let w = t.find("VS_Text");
            let w = t.widget_mut(w);
            w.text = game_text(text, FFA);
            w.font_size = 320.0;
        }
        // DetermineScore outside a rematch (@4364): ScoreContainer keeps its Collapsed design default.
        let o = t.find("Overlay_VsNumbers");
        t.widget_mut(o).vis = Vis::Visible;
        // The animation's material track works on the brushes' dynamic materials.
        for n in ["BackgroundColors", "BackgroundColors_1"] {
            let w = t.find(n);
            t.dynamic_material(w);
        }
        Countdown {
            t,
            canvas,
            anim,
            go: game_text(text, GO),
            start: -1.0,
            begun: false,
            last: 0.0,
            fired: 0,
            count: 4,
            done: false,
        }
    }

    /// The VS numbers of a Teams match whose sides differ in size (a custom Versus lobby's 2v1) or hold more than two
    /// (the port's big matches): each side's own count from the TeamStyle numbers ("1", "2"), a side of more than two
    /// its count (`custom_text::vs_number`), team 0's on the left as the scoreboard places it.
    // Deviation: the build sets TeamStyle's one number on both sides; its Teams match always has two a side.
    pub fn set_sides(&mut self, text: &GameText, sizes: [usize; 2]) {
        for (n, size) in ["VS_Number_Left", "VS_Number_Right"].into_iter().zip(sizes) {
            let vs = match size.checked_sub(1).and_then(|i| VS_NUMBER.get(i).copied().flatten()) {
                Some(k) => game_text(text, k),
                None if size > 2 => super::custom_text::vs_number(size),
                None => continue,
            };
            let w = self.t.find(n);
            setup_detailed_text(&mut self.t, w, &vs);
        }
    }

    /// StartCountdown's moment: where the match's ticks begin.
    pub fn begin(&mut self, real: f64) {
        self.start = real;
        self.begun = true;
    }

    /// Widget animations run on real time. hold: StartCountdown has not run yet (the level's warm-up frames): the
    /// card is at its start and no event key fires.
    pub fn tick(&mut self, real: f64, hold: bool, sounds: &mut Vec<String>) {
        if self.done {
            return;
        }
        if !self.begun && (self.start < 0.0 || hold) {
            self.start = real;
        }
        if hold {
            let r = self.t.root();
            self.anim.apply(&mut self.t, r, 0.0, 0.0);
            return;
        }
        let a = &self.anim.0;
        let t = (real - self.start).max(0.0);
        let end = (a.range[1] - a.range[0]) / a.res;
        if t < HIDE_AT {
            let sec = t.min(end);
            let r = self.t.root();
            self.anim.apply(&mut self.t, r, sec, self.last);
            self.last = sec;
            // Event keys: entry point 0 = PlayAudioCue, 1..4 = DecrementCountdown.
            let at = self.anim.0.range[0] + sec * self.anim.0.res;
            while self.anim.0.events.get(self.fired).is_some_and(|e| at >= e.0) {
                self.fired += 1;
                if self.fired == 1 {
                    sounds.push("ui_countdown_card_intro".to_string());
                } else {
                    self.decrement(sounds);
                }
            }
        } else {
            // HideCountdown: StopAnimation(BackgroundIn), canvas opacity 0.
            self.t.widget_mut(self.canvas).opacity = 0.0;
        }
        if t >= end {
            // OnAnimationFinished: Destroy
            self.done = true;
        }
    }

    fn decrement(&mut self, sounds: &mut Vec<String>) {
        self.count -= 1;
        let vs = self.t.find("Overlay_VsNumbers");
        self.t.widget_mut(vs).transform.scale = [1.0, 1.0];
        let c = self.t.find("Countdown");
        // (Conv_IntToText of the count; the last is GO!)
        self.t.widget_mut(c).text = if self.count == 0 { self.go.clone() } else { self.count.to_string() };
        sounds.push(if self.count == 0 { "ui_countdown_tick_complete" } else { "ui_countdown_tick" }.to_string());
    }

    /// AddToViewport: the root fills the viewport.
    /// -> whether its root was painted.
    pub fn draw(&mut self, fonts: &mut Fonts, out: &mut ElementList, width: f64, height: f64, dpi: f64) -> bool {
        if self.done || self.start < 0.0 {
            return false;
        }
        layout(&mut self.t, fonts, Geometry { pos: [0.0, 0.0], size: [width / dpi, height / dpi] }, dpi);
        paint(&self.t, fonts, out, 1.0);
        self.t.widget(self.t.root()).shown()
    }
}
