//! (the port's own) What an online match shows of a dropped link: a plate over the scoreboard portrait of each player
//! the referee has away, and "Reconnecting..." with the seconds left on the screen of a player whose own link dropped.
//! The beta has no way back into a match, so neither has a widget of the build: both are drawn here over the HUD.

use super::custom_text::{AWAY, reconnecting};
use crate::bundle::Rgba;
use crate::element::{BoxEl, DrawEffects, Element, ElementList, PaintGeometry, TextEl};
use crate::text::{FontInfo, Fonts};

/// What the plates need of the frame: the HUD canvas size (device pixels), the layer (the text on the next) and a
/// scoreboard portrait's rectangle (`Hud::slot_rect`).
pub struct AwayOverlay<'a> {
    pub size: [u32; 2],
    pub layer: i32,
    pub slot_rect: &'a dyn Fn(usize) -> Option<[f64; 4]>,
}

fn rect(list: &mut ElementList, layer: i32, r: [f64; 4], tint: Rgba) {
    let geometry = PaintGeometry { size: [r[2], r[3]], transform: [1.0, 0.0, 0.0, 1.0, r[0], r[1]] };
    let el = BoxEl { geometry, tint, uv: [0.0, 0.0, 1.0, 1.0], ..BoxEl::default() };
    list.push(layer, None, DrawEffects::NONE, Element::Box(el));
}

/// Text centred on (cx, cy).
fn text(list: &mut ElementList, fonts: &mut Fonts, layer: i32, c: [f64; 2], size: f64, s: &str, color: Rgba) {
    let info = FontInfo { size, ..FontInfo::default() };
    let m = fonts.measure(&info, s, 1.0);
    let run = fonts.shape(&info, s, 1.0);
    let geometry = PaintGeometry {
        size: [m.width, m.height],
        transform: [1.0, 0.0, 0.0, 1.0, c[0] - m.width / 2.0, c[1] - m.height / 2.0],
    };
    let el = TextEl { geometry, run, color, outline_color: [0.0, 0.0, 0.0, 0.0], material: None };
    list.push(layer, None, DrawEffects::NONE, Element::Text(el));
}

/// Draws a plate over each `away` slot's portrait and, when `reconnecting` holds the seconds left, the line across
/// the screen's upper third.
pub fn draw(
    o: &AwayOverlay<'_>,
    away: &[usize],
    reconnecting_s: Option<u32>,
    fonts: &mut Fonts,
    list: &mut ElementList,
) {
    // (one u is a pixel of a 1280 x 720 screen)
    let u = (f64::from(o.size[1]) / 720.0).min(f64::from(o.size[0]) / 1280.0);
    for &slot in away {
        let Some(r) = (o.slot_rect)(slot) else { continue };
        rect(list, o.layer, r, [0.04, 0.05, 0.12, 0.7]);
        text(list, fonts, o.layer + 1, [r[0] + r[2] / 2.0, r[1] + r[3] / 2.0], 16.0 * u, AWAY, [1.0, 1.0, 1.0, 1.0]);
    }
    if let Some(s) = reconnecting_s {
        let line = reconnecting(s);
        let (w, h) = (360.0 * u, 48.0 * u);
        let c = [f64::from(o.size[0]) / 2.0, f64::from(o.size[1]) / 3.0];
        rect(list, o.layer, [c[0] - w / 2.0, c[1] - h / 2.0, w, h], [0.04, 0.05, 0.12, 0.75]);
        text(list, fonts, o.layer + 1, c, 22.0 * u, &line, [1.0, 1.0, 1.0, 1.0]);
    }
}
