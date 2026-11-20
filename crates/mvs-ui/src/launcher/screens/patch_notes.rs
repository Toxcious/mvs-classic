//! (the port's own) The patch notes on Home: a card for each of the two latest releases under the tagline (its
//! picture, its version), and a release's notes in a window over Home, the releases listed at its left.

use super::super::notes::{Block, NoteAge, PatchNote, Run, words};
use super::super::{Model, NOTE_PICTURES, PageState, words as w};
use super::{body, close_button, dialog};
use crate::launcher::draw::*;
use crate::launcher::fonts::Face;
use crate::launcher::paint::{CLEAR, Fill, Kind, Rgba, Shape, hex, rgba};

/// The cards on Home: how many, their place under the tagline, their size and the space between them.
const CARDS: usize = 2;
const CARDS_AT: [f32; 2] = [64.0, 355.0];
const CARD: [f32; 2] = [232.0, 132.0];
const CARD_GAP: f32 = 16.0;
/// The window's list holds this many releases.
const LISTED: usize = 11;
/// The cards' pictures fade in over this long, s.
const FADE_IN: f64 = 0.4;

fn slot(n: &PatchNote) -> u32 {
    NOTE_PICTURES + n.picture as u32
}

/// A small label in capitals, tracked (the page's eyebrows); its width.
fn eyebrow(c: &mut Canvas, x: f32, y: f32, text: &str, col: Rgba) -> f32 {
    c.text_with(Face::Bold, 12.5, 1.76, x, y, 15.95, &text.to_uppercase(), Fill::Solid(col), None, 1.0)
}

/// The "New" tag of a release later than this build, its top left at `x`, `y`.
fn new_tag(c: &mut Canvas, x: f32, y: f32) {
    let label = w::NOTE_NEW.to_uppercase();
    let tw = c.width(Face::Bold, 11.0, 1.1, &label);
    c.solid([x, y, tw + 16.0 - 1.1, 20.0], ORANGE, 10.0);
    c.text_with(Face::Bold, 11.0, 1.1, x + 8.0, y, 20.0, &label, Fill::Solid(INK), None, 1.0);
}

/// Home's cards: the latest releases, the newest at the left; a click opens its notes.
pub(super) fn cards(c: &mut Canvas, m: &Model, st: &mut PageState) {
    if m.notes.is_empty() {
        return;
    }
    let [x0, y0] = CARDS_AT;
    eyebrow(c, x0, y0 - 24.0, w::PATCH_NOTES, CYAN);
    let on = st.note_pictures_at.map_or(0.0, |t0| ((c.t - t0) / FADE_IN).clamp(0.0, 1.0) as f32);
    for (k, n) in m.notes.iter().take(CARDS).enumerate() {
        let at = [x0 + k as f32 * (CARD[0] + CARD_GAP), y0];
        let hover = c.hover([at[0], at[1], CARD[0], CARD[1]]);
        // (a card under the pointer rises a little and its picture comes closer)
        let r = [at[0], at[1] - if hover { 2.0 } else { 0.0 }, CARD[0], CARD[1]];
        c.shadow([r[0], r[1] + 10.0, r[2], r[3]], rgba(2, 6, 24, 0.55), 12.0, 26.0);
        let plain = c.linear(
            &[(0.0, hex(0x1d3a8c, 1.0)), (0.55, hex(0x152a6e, 1.0)), (1.0, hex(0x0b1a48, 1.0))],
            [r[0], r[1]],
            [r[0] + r[2] * 0.4, r[1] + r[3]],
        );
        c.rect(r, plain, 12.0);
        c.picture(slot(n), r, 12.0, if hover { 1.06 } else { 1.0 }, on, None);
        // The shade the version stands on.
        let shade = c.down(
            &[
                (0.0, rgba(4, 10, 35, 0.0)),
                (0.3, rgba(4, 10, 35, 0.0)),
                (0.72, rgba(4, 10, 35, 0.7)),
                (1.0, rgba(4, 10, 35, 0.92)),
            ],
            r,
        );
        c.rect(r, shade, 12.0);
        let edge = if hover { rgba(244, 207, 122, 0.85) } else { rgba(255, 255, 255, 0.18) };
        c.framed(r, Fill::Solid(CLEAR), 1.0, edge, 12.0);
        if n.age == NoteAge::New {
            new_tag(c, r[0] + 12.0, r[1] + 12.0);
        }
        let line = c.fonts.normal(Face::Display, 34.0 * c.s) / c.s;
        let ty = r[1] + r[3] - 10.0 - line;
        c.text_with(Face::Display, 34.0, 0.34, r[0] + 14.0, ty, line, &n.version, Fill::Solid(WHITE), None, 1.0);
        let dw = c.width(Face::Body, 12.5, 0.0, &n.date);
        c.text(Face::Body, 12.5, r[0] + r[2] - 14.0 - dw, r[1] + r[3] - 12.0 - 18.0, 18.0, &n.date, DIM);
        if hover && c.pointer.click {
            st.note = Some(k);
            st.note_scroll = 0.0;
            st.menu = false;
        }
    }
}

/// The text of `runs` wrapped to `wd` from `x`, `y`, when `show` (else only measured); its height.
#[allow(clippy::too_many_arguments)] // a paragraph's place, size and colour
fn flow(c: &mut Canvas, runs: &[Run], px: f32, lh: f32, x: f32, y: f32, wd: f32, col: Rgba, show: bool) -> f32 {
    let face = |bold: bool| if bold { Face::Bold } else { Face::Body };
    let space = c.width(Face::Body, px, 0.0, " ");
    let (mut lx, mut ly) = (x, y);
    for word in words(runs) {
        let ww: f32 = word.iter().map(|(bold, part)| c.width(face(*bold), px, 0.0, part)).sum();
        if lx > x && lx + space + ww > x + wd {
            lx = x;
            ly += lh;
        } else if lx > x {
            lx += space;
        }
        for (bold, part) in word {
            let pw = c.width(face(bold), px, 0.0, part);
            if show {
                c.text(face(bold), px, lx, ly, lh, part, if bold { WHITE } else { col });
            }
            lx += pw;
        }
    }
    ly + lh - y
}

/// A release's notes from `top` down in the column `x`, `wd`; their height. Only the parts between `see`'s top and
/// bottom are drawn.
fn blocks(c: &mut Canvas, n: &PatchNote, x: f32, top: f32, wd: f32, see: [f32; 2]) -> f32 {
    let (px, lh) = (14.5, 14.5 * 1.5);
    let text = rgba(232, 240, 255, 0.86);
    let mut y = top;
    if n.blocks.is_empty() {
        c.text(Face::Body, px, x, y, lh, w::NOTE_EMPTY, FAINT);
        return lh;
    }
    for (k, b) in n.blocks.iter().enumerate() {
        match b {
            Block::Heading(h) => {
                if k > 0 {
                    y += 12.0;
                }
                eyebrow(c, x, y, h, CYAN);
                y += 15.95 + 8.0;
            }
            Block::Item { depth, runs } => {
                let dx = 16.0 + f32::from(*depth) * 18.0;
                // (measured first: the dot and the words are drawn only where they show)
                let h = flow(c, runs, px, lh, x + dx, y, wd - dx, text, false);
                if y + h > see[0] && y < see[1] {
                    c.dot([x + dx - 10.0, y + lh * 0.5], 2.2, if *depth == 0 { ORANGE } else { FAINT });
                    flow(c, runs, px, lh, x + dx, y, wd - dx, text, true);
                }
                y += h + 6.0;
            }
            Block::Para(runs) => {
                let h = flow(c, runs, px, lh, x, y, wd, text, false);
                if y + h > see[0] && y < see[1] {
                    flow(c, runs, px, lh, x, y, wd, text, true);
                }
                y += h + 10.0;
            }
            Block::Quote(runs) => {
                let h = flow(c, runs, px, lh, x + 16.0, y + 10.0, wd - 30.0, text, false) + 20.0;
                if y + h > see[0] && y < see[1] {
                    c.solid([x, y, wd, h], rgba(242, 189, 92, 0.08), 8.0);
                    c.solid([x, y + 8.0, 3.0, h - 16.0], WARN, 1.5);
                    flow(c, runs, px, lh, x + 16.0, y + 10.0, wd - 30.0, text, true);
                }
                y += h + 12.0;
            }
        }
    }
    y - top
}

/// The notes over Home (the settings' box and columns): the releases at the left, the chosen one's picture, name,
/// day and notes at the right, its notes scrolling.
pub(super) fn window(c: &mut Canvas, m: &Model, st: &mut PageState) {
    let Some(n) = st.note.and_then(|k| m.notes.get(k)) else { return };
    let r = dialog(c, 900.0, 560.0);
    // The releases' column.
    c.rect([r[0] + 1.0, r[1] + 1.0, 219.0, r[3] - 2.0], Fill::Solid(rgba(0, 0, 0, 0.25)), 0.0);
    c.solid([r[0] + 220.0, r[1] + 1.0, 1.0, r[3] - 2.0], LINE, 0.0);
    let (tx, mut ty) = (r[0] + 12.0, r[1] + 22.0);
    let tl = c.fonts.normal(Face::Display, 24.0 * c.s) / c.s;
    let title = w::PATCH_NOTES.to_uppercase();
    c.text_with(Face::Display, 24.0, 0.24, tx + 12.0, ty, tl, &title, Fill::Solid(WHITE), None, 1.0);
    ty += tl + 18.0;
    for (k, other) in m.notes.iter().take(LISTED).enumerate() {
        let br = [tx, ty, 220.0 - 24.0, 40.0];
        let on = st.note == Some(k);
        let hv = c.hover(br);
        if on {
            c.solid(br, rgba(233, 168, 74, 0.14), 9.0);
            let pc = c.p.clip;
            c.clip(Some([br[0], br[1], 3.0, br[3]]));
            c.solid(br, ORANGE, 9.0);
            c.p.clip = pc;
        } else if hv {
            c.solid(br, rgba(255, 255, 255, 0.04), 9.0);
        }
        c.text(Face::Medium, 15.0, br[0] + 14.0, br[1], 40.0, &other.version, if on || hv { WHITE } else { DIM });
        let (tag, col) = match other.age {
            NoteAge::New => (w::NOTE_NEW, ORANGE),
            NoteAge::Current => (w::NOTE_INSTALLED, FAINT),
            NoteAge::Old => ("", FAINT),
        };
        let tw = c.width(Face::Body, 12.5, 0.0, tag);
        c.text(Face::Body, 12.5, br[0] + br[2] - 12.0 - tw, br[1], 40.0, tag, col);
        if hv && c.pointer.click && !on {
            st.note = Some(k);
            st.note_scroll = 0.0;
        }
        ty += 42.0;
    }
    // The release's head: its picture across the pane's top, fading into the box, its name and day on it.
    let (px0, pw) = (r[0] + 221.0, r[2] - 222.0);
    let head_h = 168.0;
    let pc = c.p.clip;
    c.clip(Some([px0, r[1] + 1.0, pw, head_h]));
    // (drawn from under the column's edge: of the picture's rounded corners only the box's own top right shows)
    if c.picture(slot(n), [px0 - 20.0, r[1] + 1.0, pw + 20.0, head_h], 13.0, 1.0, 1.0, Some([0.3, 1.0])) {
        let shade = c.linear(
            &[(0.0, hex(0x0b1a48, 0.88)), (0.45, hex(0x0b1a48, 0.5)), (1.0, hex(0x0b1a48, 0.0))],
            [px0, 0.0],
            [px0 + pw * 0.8, 0.0],
        );
        let mut s = Shape::new(Kind::Rect, [px0 - 20.0, r[1] + 1.0, pw + 20.0, head_h], shade);
        s.radius = 13.0;
        c.push(s);
    }
    c.p.clip = pc;
    let (x, wd) = (px0 + 30.0, pw - 60.0);
    let hl = c.fonts.normal(Face::Display, 30.0 * c.s) / c.s;
    let mut y = r[1] + head_h - 26.0 - 18.0 - 4.0 - hl;
    if n.age == NoteAge::New {
        new_tag(c, x, y - 28.0);
    }
    let name = if n.title.is_empty() { &n.version } else { &n.title };
    c.text_with(Face::Display, 30.0, 0.3, x, y, hl, &name.to_uppercase(), Fill::Solid(WHITE), None, 1.0);
    y += hl + 4.0;
    let day = match (n.date.is_empty(), n.age) {
        (false, NoteAge::Current) => format!("{} \u{b7} {}", n.date, w::NOTE_INSTALLED),
        (true, NoteAge::Current) => w::NOTE_INSTALLED.to_owned(),
        _ => n.date.clone(),
    };
    c.text(Face::Body, 13.5, x, y, 18.0, &day, DIM);
    if close_button(c, r[0] + r[2] - 16.0 - 38.0, r[1] + 16.0) {
        st.note = None;
    }
    // The notes.
    let top = r[1] + head_h + 6.0;
    let pane = [x, top, wd, r[1] + r[3] - 18.0 - top];
    let see = [pane[1], pane[1] + pane[3]];
    body(c, pane, &mut st.note_scroll, |c, y| blocks(c, n, x, y + 4.0, wd, see) + 8.0);
    if c.pointer.click && !c.hover(r) {
        st.note = None;
    }
}
