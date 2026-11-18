//! The frame rate line: `stat fps` drawn top right on every screen while the Graphics screen's
//! Show Framerate is on (`GraphicsSettings::show_fps`). The rate is the perf readout's, counted
//! over 0.5 s of frames (perf.rs). The launcher has its own screens and draws none.
// Deviation: the engine's stat text (its font, colour and lines) is not drawn; one line, "<n> fps", coloured by the rate
// (green at 90 fps or more, yellow down to the sim's 60 Hz, red below).

use mvs_ui::element::{DrawEffects, Element, ElementList, PaintGeometry, TextEl};
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
const GREEN: [f64; 4] = [0.2, 1.0, 0.2, 1.0];
const YELLOW: [f64; 4] = [1.0, 1.0, 0.0, 1.0];
const RED: [f64; 4] = [1.0, 0.25, 0.2, 1.0];

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

/// The line pushed into `list` on `layer`, top right of a frame of `size`.
pub fn draw(list: &mut ElementList, fonts: &mut Fonts, size: [u32; 2], layer: i32, line: &Line) {
    let info = FontInfo { size: SIZE, ..FontInfo::default() };
    let width = fonts.measure(&info, &line.text, 1.0).width;
    let run = fonts.shape(&info, &line.text, 1.0);
    let geometry = PaintGeometry {
        size: [width, 16.0],
        transform: [1.0, 0.0, 0.0, 1.0, f64::from(size[0]) - width - MARGIN[0], MARGIN[1]],
    };
    let el = TextEl { geometry, run, color: line.color, outline_color: [0.0, 0.0, 0.0, 0.0], material: None };
    list.push(layer, None, DrawEffects::NONE, Element::Text(el));
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
    fn the_colour_follows_the_rate() {
        assert_eq!(color_of(144.0), GREEN);
        assert_eq!(color_of(90.0), GREEN);
        assert_eq!(color_of(89.0), YELLOW);
        assert_eq!(color_of(59.94), YELLOW);
        assert_eq!(color_of(59.0), RED);
    }
}
