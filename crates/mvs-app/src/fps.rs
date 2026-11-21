//! The frame rate line: `stat fps` drawn top right on every screen while the Graphics screen's
//! Show Framerate is on (`GraphicsSettings::show_fps`). The rate is the perf readout's, counted
//! over 0.5 s of frames (perf.rs). The launcher has its own screens and draws none.
// Deviation: the engine's stat text (its font, colour and lines) is not drawn; one line, "<n> fps", coloured by the rate
// (green at 90 fps or more, yellow down to the sim's 60 Hz, red below).
//
// (the port's own) The ping line, "<n> ms", left of it (in its place while Show Framerate is off), whenever a session
// is online: a custom lobby on the relay, Epic or LAN, a queue's pre-match, an online match and the screens between
// them. Never in a local lobby or offline play. Its number: `OnlineHub::ping_ms` in the menus, `Online::ping` in a
// match.
// Deviation: the build draws no ping; its stat lines are the engine's, off by default.

use mvs_ui::element::{DrawEffects, Element, ElementList, PaintGeometry, TextEl};
use mvs_ui::hud::ping::{self, GREEN, RED, YELLOW};
use mvs_ui::text::{FontInfo, Fonts};

use crate::perf::Readout;

/// The line's text size, px, and its distance from the top right corner.
const SIZE: f64 = 12.0;
const MARGIN: [f64; 2] = [8.0, 4.0];

/// The layer the menus and the loading screen draw the line on: over every element of theirs.
pub const LAYER: i32 = i32::MAX;

/// The rates at or above which the line is green (headroom over the sim's 60 Hz) and yellow (every sim tick drawn: what
/// shows as "60 fps", a 59.94 Hz display included); below the second, ticks go undrawn and it is red.
const GOOD_FPS: f64 = 90.0;
const LOW_FPS: f64 = 59.5;
/// The room between the ping line and the frame rate's, px.
const GAP: f64 = 12.0;

/// The line: its text and colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub color: [f64; 4],
}

/// The colour of a rate of `fps`.
fn color_of(fps: f64) -> [f64; 4] {
    if fps >= GOOD_FPS {
        GREEN
    } else if fps >= LOW_FPS {
        YELLOW
    } else {
        RED
    }
}

/// The line while `show` is on and a readout has been taken.
pub fn line(show: bool, readout: Option<&Readout>) -> Option<Line> {
    let r = readout.filter(|r| show && r.mean_ms > 0.0)?;
    Some(Line { text: r.fps_line(), color: color_of(1000.0 / r.mean_ms) })
}

/// The ping line of a round trip of `ms` (None: no online session, or none measured yet).
pub fn ping_line(ms: Option<f64>) -> Option<Line> {
    let ms = ms.filter(|m| *m >= 0.0)?;
    // (a round trip in whole ms: small, never negative)
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let whole = ms.round() as u32;
    Some(Line { text: mvs_ui::hud::ping_line(whole), color: ping::color(ms) })
}

/// The lines pushed into `list` on `layer`, top right of a frame of `size`: the frame rate's at the corner, the
/// ping's left of it, or at the corner while the frame rate's is off.
pub fn draw(
    list: &mut ElementList,
    fonts: &mut Fonts,
    size: [u32; 2],
    layer: i32,
    fps: Option<&Line>,
    ping: Option<&Line>,
) {
    let mut right = f64::from(size[0]) - MARGIN[0];
    for line in [fps, ping].into_iter().flatten() {
        right -= draw_one(list, fonts, right, layer, line) + GAP;
    }
}

/// One line with its right edge at `right`; its width.
fn draw_one(list: &mut ElementList, fonts: &mut Fonts, right: f64, layer: i32, line: &Line) -> f64 {
    let info = FontInfo { size: SIZE, ..FontInfo::default() };
    let width = fonts.measure(&info, &line.text, 1.0).width;
    let run = fonts.shape(&info, &line.text, 1.0);
    let geometry = PaintGeometry { size: [width, 16.0], transform: [1.0, 0.0, 0.0, 1.0, right - width, MARGIN[1]] };
    let el = TextEl { geometry, run, color: line.color, outline_color: [0.0, 0.0, 0.0, 0.0], material: None };
    list.push(layer, None, DrawEffects::NONE, Element::Text(el));
    width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_shows_only_when_asked() {
        let r = Readout { frames: 30, mean_ms: 8.0, ..Readout::default() };
        assert_eq!(line(true, Some(&r)), Some(Line { text: "125 fps".into(), color: GREEN }));
        assert_eq!(line(false, Some(&r)), None);
        assert_eq!(line(true, None), None);
        assert_eq!(line(true, Some(&Readout::default())), None);
    }

    #[test]
    fn the_ping_line_shows_a_measured_round_trip() {
        assert_eq!(ping_line(Some(42.4)), Some(Line { text: "42 ms".into(), color: GREEN }));
        assert_eq!(ping_line(None), None);
        assert_eq!(ping_line(Some(-1.0)), None, "none measured yet");
    }

    #[test]
    fn the_ping_colour_follows_its_bounds() {
        assert_eq!(ping_line(Some(80.0)).unwrap().color, GREEN);
        assert_eq!(ping_line(Some(120.0)).unwrap().color, YELLOW);
        assert_eq!(ping_line(Some(150.5)).unwrap().color, RED);
    }

    #[test]
    fn the_colour_follows_the_rate() {
        assert_eq!(color_of(144.0), GREEN);
        assert_eq!(color_of(90.0), GREEN);
        assert_eq!(color_of(89.0), YELLOW);
        assert_eq!(color_of(59.94), YELLOW);
        assert_eq!(color_of(59.0), RED);
    }
}
