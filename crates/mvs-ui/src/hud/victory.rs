//! UI_VictoryText, the VICTORY / DEFEAT card UI_Game shows from MatchEnded (UI_Game ubergraph
//! @5994): texts, colours and trophy are set, AnimIn runs, and VictoryScreenDelay (4.0 s) later FadeOut plays; 0.5 s
//! after that the widget is collapsed.

use crate::bundle::{Bundle, Rgba};
use crate::element::ElementList;
use crate::slate::{Geometry, Tree, Vis, WidgetId, instantiate, layout, paint};
use crate::text::{Fonts, GameText};

use super::common::{Anim, c4, clamp01, lerp_color};
use super::game_text;

/// UI_Game VictoryScreenDelay.
const VICTORY_SCREEN_DELAY: f64 = 4.0;
const BLACK: Rgba = [0.0, 0.0, 0.0, 1.0];
/// UI_Game's ubergraph EX_TextConst @6768 (VICTORY) and @6833 (DEFEAT).
const VICTORY: &str = "98819F34498A0A0813D8B2926C92093A";
const DEFEAT: &str = "F8AA596F4CF78AD1EBE03F80AEF526C6";

/// Ease(0, 1, alpha, EaseOut, BlendExp 30) over `seconds` from `start`.
fn ease(now: f64, start: f64, seconds: f64) -> f64 {
    1.0 - (1.0 - clamp01((now - start) / seconds)).powi(30)
}

/// UI_VictoryText.
#[derive(Debug)]
pub struct VictoryText {
    pub t: Tree,
    /// The game time of MatchEnded (the Delays and AnimIn's loops run on game time); -1: not yet.
    start: f64,
    /// The real time of it (the widget animations).
    real_start: f64,
    fade_real: f64,
    last: f64,
    /// QuadShape_1, QuadShape_2 (first), QuadShape, QuadShape_3.
    quads: [WidgetId; 4],
    halftones: [WidgetId; 2],
    winner: WidgetId,
    text_in: Anim,
    fade_out: Anim,
    pub done: bool,
}

impl VictoryText {
    /// enemy_win: UI_Game's EnemyWin; color: the card's colour (GetPaletteEntry(1).UI on a loss, else entry 0's).
    pub fn new(b: &Bundle, text: &GameText, enemy_win: bool, color: Rgba) -> VictoryText {
        let mut t = instantiate(b, "UI_VictoryText", "UI_VictoryText");
        let label = game_text(text, if enemy_win { DEFEAT } else { VICTORY });
        for n in ["VictoryDefeatTextShadow", "VictoryDefeatTextShadow_1", "VictoryDefeatText"] {
            let w = t.find(n);
            t.widget_mut(w).text = label.clone();
        }
        // WBP_Quadrilateral SetParameters fills the corner scalars of the quad's dynamic material.
        let mid = |t: &mut Tree, w: WidgetId| {
            let quad = t.widget(w).quad.clone().unwrap_or_default();
            let m = t.dynamic_material(w);
            for (k, x) in quad {
                m.set_scalar(&k, x);
            }
        };
        let quads = ["QuadShape_1", "QuadShape_2", "QuadShape", "QuadShape_3"].map(|n| t.find(n));
        for q in quads {
            mid(&mut t, q);
            let m = t.dynamic_material(q);
            m.set_vector("LeftColor", c4(color));
            m.set_vector("RightColor", c4(color));
        }
        let halftones = ["QuadHalftone", "QuadHalftone_1"].map(|n| t.find(n));
        for q in halftones {
            mid(&mut t, q);
            t.dynamic_material(q).set_vector("ColorB", c4(lerp_color(BLACK, color, 0.4)));
        }
        // GetDynamicFontMaterial of the shimmer text.
        let shadow = t.find("VictoryDefeatTextShadow_1");
        let m = t.dynamic_font_material(shadow);
        m.set_vector("BaseColor1", c4(lerp_color(BLACK, color, 0.5)));
        m.set_vector("BaseColor2", c4(lerp_color(BLACK, color, 0.1)));
        // SetTrophyBrush.
        let sw = t.find("TrophySwitcher");
        t.widget_mut(sw).active = i32::from(enemy_win);
        if enemy_win {
            let q = t.find("WBP_Quadrilateral_1");
            let q = t.widget_mut(q);
            q.tex = b.material_path("M_Quadrilateral").map(str::to_string);
            q.mid = None;
            q.color = [0.042311, 0.057805, 0.099899, 1.0];
        }
        let winner = t.find("Overlay_Winner");
        VictoryText {
            t,
            start: -1.0,
            real_start: 0.0,
            fade_real: -1.0,
            last: 0.0,
            quads,
            halftones,
            winner,
            text_in: Anim::of(b, "UI_VictoryText", "TextAnimIn"),
            fade_out: Anim::of(b, "UI_VictoryText", "FadeOut"),
            done: false,
        }
    }

    pub fn tick(&mut self, now: f64, real: f64, sounds: &mut Vec<String>) {
        if self.done {
            return;
        }
        if self.start < 0.0 {
            // SetVisibility(HitTestInvisible), AnimIn, the match complete sound.
            self.start = now;
            self.real_start = real;
            sounds.push("ui_match_complete".to_string());
        }
        let t0 = self.start;
        // AnimIn: everything starts at 0; 0.25 s later DeltaPercentage eases over 2.0 s on QuadShape_1 / _2 and, a
        // Delay(0.03) after, on QuadShape / _3; 0.5 s after the start Percentage, the halftones' and the trophy's
        // opacity ease over 3.0 s. The Delay(0.03) (@602 -> @777, called from the first loop's body) ends on the
        // second frame after it (a Delay ends on the first frame that uses its time up: ceil(0.03 x 60) = 2), and
        // the second pair's start time is the game time read there.
        let second = t0 + 0.25 + 2.0 / 60.0;
        let a = if now >= t0 + 0.25 { ease(now, t0 + 0.25, 2.0) } else { 0.0 };
        let b = if now >= second { ease(now, second, 2.0) } else { 0.0 };
        let c = if now >= t0 + 0.5 { ease(now, t0 + 0.5, 3.0) } else { 0.0 };
        let t = &mut self.t;
        for (i, &q) in self.quads.iter().enumerate() {
            let m = t.dynamic_material(q);
            m.set_scalar("DeltaPercentage", (if i < 2 { a } else { b }) as f32);
            m.set_scalar("Percentage", c as f32);
        }
        for &h in &self.halftones {
            t.widget_mut(h).opacity = c;
        }
        t.widget_mut(self.winner).opacity = c;
        let r = t.root();
        if self.fade_real < 0.0 {
            // TextAnimIn (the Text overlay starts at opacity 0).
            let sec = (real - self.real_start).min(self.text_in.end());
            self.text_in.apply(t, r, sec, self.last);
            self.last = sec;
            if now >= t0 + VICTORY_SCREEN_DELAY {
                self.fade_real = real;
                self.last = 0.0;
            }
        }
        if self.fade_real >= 0.0 {
            let sec = (real - self.fade_real).min(self.fade_out.end());
            self.fade_out.apply(t, r, sec, self.last);
            self.last = sec;
            if now >= t0 + VICTORY_SCREEN_DELAY + 0.5 {
                // Collapsed; the transition sound. The BPI_AddLoadingScreen call that follows is an empty event in
                // the build (PandaGameInstance @28632): no cover is added.
                t.widget_mut(r).vis = Vis::Collapsed;
                sounds.push("ui_match_complete_transition".to_string());
                self.done = true;
            }
        }
    }

    /// UI_Game's canvas slot: the whole viewport.
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
