//! The launcher's screens (app/launcher/index.html, styles.css, launcher.ts's drawing): the splash, setup's three
//! steps and the guide over them, Home with PLAY and the more menu, and the settings over Home.

use std::f32::consts::PI;

use super::draw::*;
use super::fonts::Face;
use super::paint::{Fill, Kind, Rgba, Shape, hex, rgba};
use super::sky::{self, Star};
use super::{Action, HomeState, Model, PageState, PhaseState, Screen, TITLE_BAR, Tone, WINDOW_BUTTON, words as w};

fn tone(t: Tone, plain: Rgba) -> Rgba {
    match t {
        Tone::Plain => plain,
        Tone::Ok => GOOD,
        Tone::Warn => WARN,
        Tone::Bad => BAD,
    }
}

/// The page: its screen, then the overlays over it.
pub(super) fn page(c: &mut Canvas, m: &Model, st: &mut PageState, stars: &[Vec<Star>; 3], out: &mut Vec<Action>) {
    c.blocked = st.guide || st.settings;
    let shut = (!st.guide, !st.settings);
    match m.screen {
        Screen::Splash => splash(c, m, &stars[0]),
        Screen::Install | Screen::Beta | Screen::Build => setup(c, m, st, &stars[1], out),
        Screen::Home => home(c, m, st, &stars[2], out),
    }
    // (the title bar over the screens, under the overlays: styles.css .titlebar's z-index 20, .overlay's 25)
    title_bar(c, out);
    c.blocked = false;
    // The click that opened an overlay is the opener's: the overlay, drawn this same frame, would take it for a
    // click outside its box and close at once.
    if (shut.0 && st.guide) || (shut.1 && st.settings) {
        c.pointer.click = false;
    }
    if st.guide {
        guide(c, st, out);
    }
    if st.settings && m.screen == Screen::Home {
        settings(c, m, st, out);
    }
    if let Some(p) = &m.app_problem {
        banner(c, 52.0, 760.0, p, None, rgba(60, 8, 24, 0.92));
    }
}

/// The title bar's window buttons, each 50 x 48 at the top right (styles.css .titlebar, .wbtn): Minimize, then
/// Close. The rest of the bar drags the window (the app's drag band: `TITLE_BAR`, `TITLE_BAR_KEEP`).
fn title_bar(c: &mut Canvas, out: &mut Vec<Action>) {
    let x = c.size[0] - 2.0 * WINDOW_BUTTON;
    for (k, action) in [Action::Minimize, Action::Close].into_iter().enumerate() {
        let r = [x + k as f32 * WINDOW_BUTTON, 0.0, WINDOW_BUTTON, TITLE_BAR];
        let hover = c.hover(r);
        let close = action == Action::Close;
        if hover {
            c.solid(r, if close { hex(0xe81123, 1.0) } else { rgba(255, 255, 255, 0.08) }, 0.0);
        }
        // (a 12 px icon of 10 units, stroke 1.3, its drop shadow 1 px down)
        let (cx, cy) = (r[0] + WINDOW_BUTTON * 0.5, r[1] + TITLE_BAR * 0.5);
        let u = 1.2;
        let lines: &[[f32; 4]] =
            if close { &[[0.0, 0.0, 10.0, 10.0], [10.0, 0.0, 0.0, 10.0]] } else { &[[0.0, 5.0, 10.0, 5.0]] };
        for (dy, col) in [(1.0, rgba(0, 0, 0, 0.5)), (0.0, WHITE)] {
            for l in lines {
                let a = [cx + (l[0] - 5.0) * u, cy + (l[1] - 5.0) * u + dy];
                let b = [cx + (l[2] - 5.0) * u, cy + (l[3] - 5.0) * u + dy];
                c.seg(a, b, 1.3 * u, col);
            }
        }
        if hover && c.pointer.click {
            out.push(action);
        }
    }
}

/// A problem line centred at `y` (.app-problem, .problem-banner): its button's click.
fn banner(c: &mut Canvas, y: f32, max: f32, text: &str, button: Option<&str>, bg: Rgba) -> bool {
    let bw = button.map_or(0.0, |b| c.btn_width(b, true, Btn::Plain) + 12.0);
    let tw = c.width(Face::Body, 14.5, 0.0, text).min(max - 32.0 - 20.0 - bw);
    let lines = c.lines(Face::Body, 14.5, tw + 1.0, text);
    let lh = 14.5 * 1.45;
    let h = (lines.len() as f32 * lh).max(if button.is_some() { 30.0 } else { 0.0 }) + 20.0;
    let wd = 16.0 + 8.0 + 12.0 + tw + bw + 14.0;
    let x = (c.size[0] - wd) * 0.5;
    c.framed([x, y, wd, h], Fill::Solid(bg), 1.0, rgba(255, 84, 112, 0.5), 12.0);
    c.shadow([x + 16.0 - 3.0, y + h * 0.5 - 7.0, 14.0, 14.0], BAD, 7.0, 10.0);
    c.dot([x + 20.0, y + h * 0.5], 4.0, BAD);
    let ty = y + (h - lines.len() as f32 * lh) * 0.5;
    for (k, l) in lines.iter().enumerate() {
        c.text(Face::Body, 14.5, x + 36.0, ty + k as f32 * lh, lh, l, WHITE);
    }
    button.is_some_and(|b| c.button(x + 36.0 + tw + 12.0, y + (h - 30.0) * 0.5, b, true, Btn::Plain, false).0)
}

/// Page pixels a wheel notch scrolls.
// Unverified: Chromium's step on Windows at the default three lines a notch (100 px), not read from the page.
const WHEEL_PX: f32 = 100.0;

/// A panel (.panel): the night's glass.
// Deviation: no backdrop-filter blur under the panels, the dialogs and the log tab: the page behind shows sharp.
fn panel(c: &mut Canvas, r: [f32; 4], bg: Rgba, border: Rgba) {
    c.framed(r, Fill::Solid(bg), 1.0, border, 14.0);
}

fn splash(c: &mut Canvas, m: &Model, stars: &[Star]) {
    sky::simple(c, stars);
    let wm_line = c.fonts.normal(Face::Wordmark, 128.0 * c.s) / c.s;
    let h = wm_line + 56.0 + 4.0 + 12.0 + 18.0;
    let y = (c.size[1] - h) * 0.5;
    let ww = c.width(Face::Wordmark, 128.0, -2.56, "MVS") + 10.0;
    sky::wordmark(c, (c.size[0] - ww) * 0.5, y, 128.0);
    let bx = (c.size[0] - 220.0) * 0.5;
    let by = y + wm_line + 56.0;
    indeterminate(c, [bx, by, 220.0, 4.0]);
    let text = if m.checking { w::CHECKING_UPDATES } else { w::STARTING };
    let tw = c.width(Face::Body, 14.0, 0.0, text);
    c.text(Face::Body, 14.0, (c.size[0] - tw) * 0.5, by + 4.0 + 12.0, 18.0, text, FAINT);
}

/// The amber bar's fill (.bar > i: its gradient flowing).
/// `moving`: the gradient sweeps along (a bar that shows work going on), else it stands still.
fn bar_fill(c: &mut Canvas, r: [f32; 4], stops: &[(f32, Rgba)], radius: f32, moving: bool) {
    let grad = c.p.gradient(stops);
    let shift = if moving { (c.t / 2.4).fract() as f32 * 2.0 } else { 0.0 };
    let fill = Fill::Linear { grad, from: [r[0], 0.0], to: [r[0] + 2.0 * r[2].max(1.0), 0.0], repeat: true, shift };
    c.rect(r, fill, radius);
}

fn amber() -> [(f32, Rgba); 5] {
    [(0.0, hex(0xd98a38, 1.0)), (0.25, GOLD), (0.5, hex(0xfff0c4, 1.0)), (0.75, GOLD), (1.0, hex(0xd98a38, 1.0))]
}

/// A bar of `frac` (.bar): its track and its fill, swept along when `moving`.
fn bar(c: &mut Canvas, r: [f32; 4], frac: f32, stops: &[(f32, Rgba)], moving: bool) {
    c.solid(r, rgba(160, 190, 240, 0.12), r[3]);
    if frac > 0.0 {
        let fr = [r[0], r[1], r[2] * frac.min(1.0), r[3]];
        let pc = c.p.clip;
        c.clip(Some(r));
        let mut clip = c.p.clip;
        clip = [clip[0].max(pc[0]), clip[1].max(pc[1]), clip[2].min(pc[2]), clip[3].min(pc[3])];
        c.p.clip = clip;
        bar_fill(c, fr, stops, r[3], moving);
        c.p.clip = pc;
    }
}

/// The splash's sliding bar (.bar.indeterminate).
fn indeterminate(c: &mut Canvas, r: [f32; 4]) {
    c.solid(r, rgba(160, 190, 240, 0.12), r[3]);
    let p = (c.t / 1.6).fract() as f32;
    // slide: translateX(-100%) to (290%) of its 35% width, ease-in-out
    let q = p * p * (3.0 - 2.0 * p);
    let iw = 0.35 * r[2];
    let x = r[0] + (-1.0 + 3.9 * q) * iw;
    let pc = c.p.clip;
    c.clip(Some(r));
    bar_fill(c, [x, r[1], iw, r[3]], &amber(), r[3], true);
    c.p.clip = pc;
}

// ---------------------------------------------------------------------------------------------------------------
// Setup

const CARD_W: f32 = 610.0;

fn setup(c: &mut Canvas, m: &Model, st: &mut PageState, stars: &[Star], out: &mut Vec<Action>) {
    sky::simple(c, stars);
    let step = match m.screen {
        Screen::Install => 1,
        Screen::Beta => 2,
        _ => 3,
    };
    brand(c, step);
    let card = [c.size[0] - 48.0 - CARD_W, 70.0, CARD_W, c.size[1] - 110.0];
    panel(c, card, PANEL, LINE);
    let inner = [card[0] + 34.0, card[1] + 30.0, CARD_W - 68.0, card[3] - 60.0];
    match m.screen {
        Screen::Install => install(c, m, st, inner, out),
        Screen::Beta => beta(c, m, st, inner, out),
        _ => build(c, m, st, inner, out),
    }
}

/// The brand column: the wordmark, the line, the steps, the foot.
fn brand(c: &mut Canvas, step: u32) {
    let (x, top, bottom) = (64.0, 70.0, c.size[1] - 40.0);
    let wm = c.fonts.normal(Face::Wordmark, 112.0 * c.s) / c.s;
    let lead = c.lines(Face::Body, 16.5, 320.0, w::TAGLINE);
    let lh = 16.5 * 1.45;
    let li = 3.0 + 15.5 * 1.45 + 1.0 + 14.0 * 1.45;
    let h = wm + 30.0 + lead.len() as f32 * lh + 44.0 + (li + 20.0) * 2.0 + li;
    let mut y = top + ((bottom - top - 40.0) - h) * 0.5;
    sky::wordmark(c, x - 4.0, y, 112.0);
    y += wm + 30.0;
    for l in &lead {
        c.text(Face::Body, 16.5, x, y, lh, l, DIM);
        y += lh;
    }
    y += 44.0;
    for (k, (title, small)) in w::STEPS.iter().enumerate() {
        let n = k as u32 + 1;
        let (done, cur) = (n < step, n == step);
        let centre = [x + 14.0, y + 14.0];
        if k < 2 {
            let col = if done { rgba(99, 214, 160, 0.45) } else { rgba(255, 255, 255, 0.1) };
            c.solid([x + 13.0, y + 32.0, 2.0, li + 20.0 - 36.0], col, 1.0);
        }
        if cur {
            c.shadow([x, y + 4.0, 28.0, 28.0], rgba(233, 168, 74, 0.3), 14.0, 14.0);
            c.dot(centre, 18.0, rgba(233, 168, 74, 0.16));
            let f = c.down(&[(0.0, hex(0xf0ad4c, 1.0)), (1.0, hex(0xd9862c, 1.0))], [x, y, 28.0, 28.0]);
            c.rect([x, y, 28.0, 28.0], f, 14.0);
        } else if done {
            c.dot(centre, 14.0, GOOD);
        } else {
            c.ring(centre, 14.0, 2.0, rgba(255, 255, 255, 0.18), None);
        }
        if done {
            check(c, centre, 1.0, hex(0x06301a, 1.0), 2.5);
        } else {
            let num = n.to_string();
            let nw = c.width(Face::Display, 15.0, 0.0, &num);
            if cur {
                c.text(Face::Display, 15.0, centre[0] - nw * 0.5, y + 1.0, 28.0, &num, rgba(100, 52, 12, 0.8));
            }
            c.text(Face::Display, 15.0, centre[0] - nw * 0.5, y, 28.0, &num, if cur { WHITE } else { FAINT });
        }
        let tx = x + 28.0 + 14.0;
        c.text(Face::Semi, 15.5, tx, y + 3.0, 15.5 * 1.45, title, if done || cur { WHITE } else { FAINT });
        c.text(Face::Body, 14.0, tx, y + 3.0 + 15.5 * 1.45 + 1.0, 14.0 * 1.45, small, if cur { DIM } else { FAINT });
        y += li + 20.0;
    }
    let foot = c.lines(Face::Body, 13.5, 320.0, w::FOOT);
    let flh = 13.5 * 1.45;
    let mut fy = bottom - foot.len() as f32 * flh;
    for l in &foot {
        c.text(Face::Body, 13.5, x, fy, flh, l, FAINT);
        fy += flh;
    }
}

/// A check mark (the steps' and the phases' done: a rotated L) centred at `at`.
fn check(c: &mut Canvas, at: [f32; 2], k: f32, col: Rgba, width: f32) {
    let pts =
        [[at[0] - 4.0 * k, at[1] - 0.5 * k], [at[0] - 1.2 * k, at[1] + 2.3 * k], [at[0] + 4.2 * k, at[1] - 3.0 * k]];
    c.path(&pts, width, col);
}

/// A step's head: the eyebrow, the heading, the lead; the y under it.
fn head(c: &mut Canvas, r: [f32; 4], eyebrow: &str, extra: Option<&str>, title: &str, lead: Option<&str>) -> f32 {
    let mut y = r[1];
    let eb = eyebrow.to_uppercase();
    let ew = c.text_with(Face::Bold, 12.5, 1.76, r[0], y, 15.95, &eb, Fill::Solid(CYAN), None, 1.0);
    if let Some(x) = extra {
        c.text_with(Face::Bold, 12.5, 1.76, r[0] + ew, y, 15.95, &x.to_uppercase(), Fill::Solid(FAINT), None, 1.0);
    }
    y += 15.95 + 6.0;
    let hl = c.fonts.normal(Face::Display, 32.0 * c.s) / c.s;
    c.text_with(Face::Display, 32.0, 0.32, r[0], y, hl, &title.to_uppercase(), Fill::Solid(WHITE), None, 1.0);
    y += hl;
    if let Some(l) = lead {
        y += 8.0;
        y += c.para(Face::Body, 15.0, 21.75, r[0], y, r[2], l, DIM);
    }
    y
}

/// A note (.note: 13px, its tone) wrapped to `wd` at `y`; its height with its 10px margin.
fn note(c: &mut Canvas, x: f32, y: f32, wd: f32, n: &str, t: Tone) -> f32 {
    10.0 + c.para(Face::Body, 14.5, 14.5 * 1.45, x, y + 10.0, wd, n, tone(t, DIM))
}

/// A path box (.path); its height.
fn path_box(c: &mut Canvas, r: [f32; 3], text: &str, pad: [f32; 2]) -> f32 {
    let lh = c.fonts.normal(Face::Mono, 15.0 * c.s) / c.s;
    let h = lh + 2.0 * pad[1] + 2.0;
    c.framed([r[0], r[1], r[2], h], Fill::Solid(rgba(0, 0, 0, 0.3)), 1.0, LINE, 8.0);
    c.text_cut(
        Face::Mono,
        12.5,
        r[0] + pad[0] + 1.0,
        r[1] + pad[1] + 1.0,
        lh,
        text,
        hex(0xe8e3ff, 1.0),
        r[2] - 2.0 * pad[0] - 2.0,
    );
    h
}

/// A line of the Online tab's notes in its tone (the page's .desc.spaced with ok / bad); the y after it. Nothing for
/// a note with no text.
fn note_line(c: &mut Canvas, n: &super::Note, x: f32, y: f32, wd: f32) -> f32 {
    if n.text.is_empty() {
        return y;
    }
    let col = match n.tone {
        Tone::Ok => GOOD,
        Tone::Bad => BAD,
        _ => DIM,
    };
    let h = c.para(Face::Body, 14.0, 14.0 * 1.45, x, y + 10.0, wd, &n.text, col);
    y + 10.0 + h
}

/// A text box of the Online tab (styles.css .field): its text, or the placeholder, and the caret while it has the
/// focus; whether it was clicked.
#[allow(clippy::too_many_arguments)]
fn field(c: &mut Canvas, r: [f32; 4], text: &str, placeholder: &str, focused: bool, bad: bool) -> bool {
    let border = if bad {
        BAD
    } else if focused {
        ORANGE
    } else {
        LINE
    };
    c.framed(r, Fill::Solid(rgba(0, 0, 0, 0.3)), 1.0, border, 8.0);
    let pad = 12.0;
    let lh = 20.0;
    let ty = r[1] + (r[3] - lh) * 0.5;
    if text.is_empty() {
        c.text_cut(Face::Body, 15.0, r[0] + pad, ty, lh, placeholder, FAINT, r[2] - 2.0 * pad);
    } else {
        c.text_cut(Face::Body, 15.0, r[0] + pad, ty, lh, text, WHITE, r[2] - 2.0 * pad);
    }
    if focused && (c.t * 2.0).floor() % 2.0 == 0.0 {
        let tw = c.width(Face::Body, 15.0, 0.0, text).min(r[2] - 2.0 * pad);
        c.solid([r[0] + pad + tw + 1.0, ty + 1.0, 1.5, lh - 2.0], WHITE, 0.0);
    }
    c.clicked(r)
}

/// A switch with its label and words (styles.css .toggle.switch); whether it was clicked; the y after it.
fn switch(c: &mut Canvas, x: f32, y: f32, wd: f32, label: &str, text: &str, on: bool) -> (bool, f32) {
    let sw = [x + wd - 44.0, y + 2.0, 44.0, 24.0];
    c.text(Face::Bold, 15.0, x, y, 20.0, label, WHITE);
    let h = c.para(Face::Body, 14.0, 14.0 * 1.45, x, y + 22.0, wd - 64.0, text, DIM);
    let hover = c.hover(sw);
    c.solid(sw, if on { ORANGE } else { rgba(255, 255, 255, if hover { 0.22 } else { 0.16 }) }, 12.0);
    let kx = if on { sw[0] + 22.0 } else { sw[0] + 2.0 };
    c.solid([kx, sw[1] + 2.0, 20.0, 20.0], WHITE, 10.0);
    (c.clicked([x, y, wd, 22.0 + h]), y + 22.0 + h)
}

/// A test line's mark at `at` (10 px round): a check, a cross, a "!" or a dot in its tone on a tinted disc; a dot
/// that pulses while the test runs.
fn mark(c: &mut Canvas, at: [f32; 2], t: Tone, running: bool) {
    let col = tone(t, DIM);
    let mut disc = col;
    disc[3] = 0.16;
    c.dot(at, 10.0, disc);
    let [cx, cy] = at;
    match t {
        Tone::Ok => c.path(&[[cx - 4.0, cy + 0.2], [cx - 1.2, cy + 3.0], [cx + 4.2, cy - 2.8]], 2.0, col),
        Tone::Bad => {
            c.seg([cx - 3.2, cy - 3.2], [cx + 3.2, cy + 3.2], 2.0, col);
            c.seg([cx - 3.2, cy + 3.2], [cx + 3.2, cy - 3.2], 2.0, col);
        }
        Tone::Warn => {
            c.seg([cx, cy - 4.0], [cx, cy + 0.8], 2.0, col);
            c.dot([cx, cy + 3.8], 1.1, col);
        }
        Tone::Plain => {
            let mut d = col;
            if running {
                d[3] *= 0.55 + 0.45 * (c.t * 4.0).sin().abs() as f32;
            }
            c.dot(at, 3.0, d);
        }
    }
}

/// The Online tab (launcher.ts index.html data-pane="online"): the one line of online play, Test connection and its
/// lines with Copy (the port's own), then under the Advanced fold the relay's address, Epic, LAN with its port, and
/// the direct links, each with its sign-in line.
/// The pane scrolls in `r` (x, y, width, height) when the fold and the test's lines pass its foot.
fn online_pane(c: &mut Canvas, o: &super::OnlineSettingsView, st: &mut PageState, r: [f32; 4], out: &mut Vec<Action>) {
    let mut scroll = st.online_scroll;
    body(c, r, &mut scroll, |c, top| online_lines(c, o, st, r[0], r[2], top, out) - top);
    st.online_scroll = scroll;
}

/// The Online tab's content from `y`; the y after it.
fn online_lines(
    c: &mut Canvas,
    o: &super::OnlineSettingsView,
    st: &mut PageState,
    x: f32,
    wd: f32,
    y: f32,
    out: &mut Vec<Action>,
) -> f32 {
    let mut y = y + 16.0;
    // (the port's own) The status card: the one line of online play behind its glowing dot, Test connection and
    // Copy result at its right, or under it when the line would wrap beside them.
    let pad = 16.0;
    let tw = c.btn_width(w::TEST_CONNECTION, true, Btn::Plain);
    let copy = !o.testing && !o.test.is_empty();
    let cw = if copy { c.btn_width(w::COPY_TEST, true, Btn::Ghost) + 8.0 } else { 0.0 };
    // (no line before the game runs or a test is done: "not tested yet", dim, behind a grey dot)
    let (head, head_col, dc) = if o.line.text.is_empty() {
        (w::LINE_UNTESTED, DIM, FAINT)
    } else {
        (o.line.text.as_str(), WHITE, tone(o.line.tone, DIM))
    };
    let (lh, dpx) = (15.0 * 1.45, 15.0);
    let dlh = dpx * 1.45;
    // (the headline and the buttons share the top row; what it means runs the card's width under them)
    let full_w = wd - 2.0 * pad - 22.0;
    let beside_w = full_w - tw - cw - 16.0;
    let beside = c.width(Face::Semi, 15.0, 0.0, head) <= beside_w;
    let head_w = if beside { beside_w } else { full_w };
    let head_h = c.lines(Face::Semi, 15.0, head_w, head).len().max(1) as f32 * lh;
    let row_h = if beside { head_h.max(30.0) } else { head_h };
    let detail_h = if o.line_detail.is_empty() {
        0.0
    } else {
        6.0 + c.lines(Face::Body, dpx, full_w, &o.line_detail).len() as f32 * dlh
    };
    let ch = row_h + detail_h + 2.0 * pad + if beside { 0.0 } else { 12.0 + 30.0 };
    c.framed([x, y, wd, ch], Fill::Solid(rgba(255, 255, 255, 0.04)), 1.0, LINE, 12.0);
    let ty = y + pad + (row_h - head_h) * 0.5;
    let dy = ty + lh * 0.5;
    if !o.line.text.is_empty() {
        c.shadow([x + pad - 1.0, dy - 5.0, 10.0, 10.0], dc, 5.0, 12.0);
    }
    c.dot([x + pad + 4.0, dy], 4.0, dc);
    c.para(Face::Semi, 15.0, lh, x + pad + 22.0, ty, head_w, head, head_col);
    if !o.line_detail.is_empty() {
        c.para(Face::Body, dpx, dlh, x + pad + 22.0, y + pad + row_h + 6.0, full_w, &o.line_detail, DIM);
    }
    let (bx, by) = if beside {
        (x + wd - pad - tw - cw, y + pad + (row_h - 30.0) * 0.5)
    } else {
        (x + pad + 22.0 - 2.0, y + pad + row_h + detail_h + 12.0)
    };
    if c.button(bx, by, w::TEST_CONNECTION, true, Btn::Plain, o.testing).0 {
        out.push(Action::TestConnection);
    }
    if copy && c.button(bx + tw + 8.0, by, w::COPY_TEST, true, Btn::Ghost, false).0 {
        out.push(Action::CopyTest);
    }
    y += ch;
    // The test's lines: one row each, a mark of its tone at the left; before a test, what it checks, dim.
    let testing = [super::Note::new(w::TESTING, Tone::Plain)];
    let hint = [super::Note::new(w::TEST_HINT, Tone::Plain)];
    let rows = if o.testing { &testing[..] } else { &o.test[..] };
    let rows: Vec<&super::Note> = rows.iter().filter(|n| !n.text.is_empty()).collect();
    let untested = rows.is_empty() && !o.testing;
    let rows: Vec<&super::Note> = if untested { hint.iter().collect() } else { rows };
    if !rows.is_empty() {
        y += 10.0;
        let (rp, px) = (14.0, 15.0);
        let rlh = px * 1.45;
        let tx = x + rp + 30.0;
        let tww = x + wd - rp - tx;
        let hs: Vec<f32> =
            rows.iter().map(|n| c.lines(Face::Body, px, tww, &n.text).len() as f32 * rlh + 22.0).collect();
        c.framed([x, y, wd, hs.iter().sum()], Fill::Solid(rgba(0, 0, 0, 0.22)), 1.0, LINE, 12.0);
        for (k, (n, h)) in rows.iter().zip(&hs).enumerate() {
            if k > 0 {
                c.hline(x + rp, y, wd - 2.0 * rp, LINE);
            }
            mark(c, [x + rp + 10.0, y + 11.0 + rlh * 0.5], n.tone, o.testing);
            let col = match n.tone {
                _ if untested => DIM,
                Tone::Bad => hex(0xffd7de, 1.0),
                _ => hex(0xe8eeff, 1.0),
            };
            c.para(Face::Body, px, rlh, tx, y + 11.0, tww, &n.text, col);
            y += h;
        }
    }
    y += 16.0;
    c.hline(x, y, wd, LINE);
    y += 12.0;
    // the fold: its label and a chevron, the switches under it while open
    let fold = [x, y, wd, 24.0];
    let hover = c.hover(fold);
    c.text(Face::Bold, 15.0, x, y + 2.0, 20.0, w::ADVANCED, if hover { WHITE } else { DIM });
    let (cx, cy) = (x + wd - 10.0, y + 12.0);
    let k = 5.0;
    let path: [[f32; 2]; 3] = if o.advanced {
        [[cx - k, cy - k * 0.5], [cx, cy + k * 0.5], [cx + k, cy - k * 0.5]]
    } else {
        [[cx - k * 0.5, cy - k], [cx + k * 0.5, cy], [cx - k * 0.5, cy + k]]
    };
    c.path(&path, 1.8, if hover { WHITE } else { DIM });
    if c.clicked(fold) {
        out.push(Action::ToggleAdvanced);
    }
    y += 24.0;
    if !o.advanced {
        // (a box folded away lets the keyboard go)
        st.focus = super::Focus::None;
        return y + 16.0;
    }
    online_switches(c, o, st, x, wd, y + 12.0, out) + 16.0
}

/// The Online tab's Advanced fold: the relay's address, Epic, LAN with its port, and the direct links, each with its
/// sign-in line; the y after it.
fn online_switches(
    c: &mut Canvas,
    o: &super::OnlineSettingsView,
    st: &mut PageState,
    x: f32,
    wd: f32,
    y: f32,
    out: &mut Vec<Action>,
) -> f32 {
    use super::Focus;
    let mut y = y;
    if st.focus != Focus::Relay {
        st.relay_box.clone_from(&o.relay);
    }
    if st.focus != Focus::Port {
        st.port_box.clone_from(&o.lan_port);
    }
    c.text(Face::Bold, 15.0, x, y, 15.0 * 1.45, w::RELAY, WHITE);
    y += 15.0 * 1.45 + 4.0;
    y += c.para(Face::Body, 14.0, 14.0 * 1.45, x, y, wd, w::RELAY_DESC, DIM);
    y += 10.0;
    let rb = [x, y, wd, 38.0];
    let relay_text = st.relay_box.clone();
    let hit = field(c, rb, &relay_text, w::RELAY_PLACEHOLDER, st.focus == Focus::Relay, o.relay_bad);
    let mut inside = hit || c.hover(rb);
    if hit {
        st.focus = Focus::Relay;
    }
    y += 38.0;
    y = note_line(c, &o.relay_status, x, y, wd);
    y = note_line(c, &o.relay_now, x, y, wd);
    y += 16.0;
    c.hline(x, y, wd, LINE);
    y += 17.0;
    let (hit, ny) = switch(c, x, y, wd, w::EOS_LABEL, w::EOS_DESC, o.eos);
    if hit {
        out.push(Action::ToggleEos);
    }
    y = note_line(c, &o.epic_now, x, ny, wd);
    y += 16.0;
    c.hline(x, y, wd, LINE);
    y += 17.0;
    let (hit, ny) = switch(c, x, y, wd, w::LAN_LABEL, w::LAN_DESC, o.lan.is_some());
    if hit {
        out.push(Action::ToggleLan);
    }
    y = ny;
    if o.lan.is_some() {
        y += 10.0;
        c.text(Face::Body, 14.0, x, y + 9.0, 14.0 * 1.45, w::PORT, DIM);
        let pb = [x + 48.0, y, 90.0, 38.0];
        let port_text = st.port_box.clone();
        let hit = field(c, pb, &port_text, "", st.focus == Focus::Port, o.port_bad);
        inside |= hit || c.hover(pb);
        if hit {
            st.focus = Focus::Port;
        }
        y += 38.0;
    }
    y = note_line(c, &o.lan_now, x, y, wd);
    y += 16.0;
    c.hline(x, y, wd, LINE);
    y += 17.0;
    let (hit, ny) = switch(c, x, y, wd, w::DIRECT_LABEL, w::DIRECT_DESC, o.direct);
    if hit {
        out.push(Action::ToggleDirect);
    }
    let y = note_line(c, &o.direct_now, x, ny, wd);
    // a click outside both boxes lets the keyboard go
    if c.pointer.click && !inside {
        st.focus = Focus::None;
    }
    y
}

/// The step's body between `top` and its actions, scrolled (`slot` the scroll kept): `draw` lays it out from a y and
/// answers its height.
fn body(c: &mut Canvas, r: [f32; 4], scroll: &mut f32, draw: impl FnOnce(&mut Canvas, f32) -> f32) {
    let pc = c.p.clip;
    c.clip(Some([r[0] - 4.0, r[1], r[2] + 16.0, r[3]]));
    let h = draw(c, r[1] - *scroll);
    c.p.clip = pc;
    let max = (h - r[3]).max(0.0);
    if c.hover([r[0], r[1], r[2] + 12.0, r[3]]) && c.pointer.wheel != 0.0 {
        *scroll -= c.pointer.wheel * WHEEL_PX;
    }
    *scroll = scroll.clamp(0.0, max);
    if max > 0.0 {
        let th = (r[3] * r[3] / h).max(24.0);
        let ty = r[1] + (r[3] - th) * (*scroll / max);
        c.solid([r[0] + r[2] + 6.0, ty + 3.0, 4.0, th - 6.0], rgba(169, 198, 255, 0.22), 2.0);
    }
}

fn install(c: &mut Canvas, m: &Model, st: &mut PageState, r: [f32; 4], out: &mut Vec<Action>) {
    let s = &m.install;
    let y = head(c, r, &w::step_of(1), None, w::INSTALL_TITLE, Some(w::INSTALL_LEAD));
    let actions = r[1] + r[3] - 38.0;
    let br = [r[0], y + 18.0, r[2], actions - 16.0 - (y + 18.0)];
    body(c, br, &mut st.scroll[0], |c, mut y| {
        let y0 = y;
        c.text(Face::Semi, 13.5, r[0], y, 19.6, w::INSTALL_LABEL, FAINT);
        y += 19.6 + 6.0;
        y += path_box(c, [r[0], y, r[2]], &s.root, [10.0, 8.0]);
        y += 10.0;
        let (pick, bw) = c.button(r[0], y, w::CHANGE_LOCATION, true, Btn::Plain, false);
        if pick {
            out.push(Action::PickRoot);
        }
        let mut x = r[0] + bw + 8.0;
        if s.can_default {
            let (d, dw) = c.button(x, y, w::BACK_TO_DEFAULT, true, Btn::Ghost, false);
            if d {
                out.push(Action::DefaultRoot);
            }
            x += dw + 8.0;
        }
        if let Some(n) = &s.space {
            let left = r[0] + r[2] - x - 6.0;
            let lines = c.lines(Face::Body, 14.5, left, &n.text);
            let lh = 14.5 * 1.45;
            let ty = y + (30.0 - lines.len() as f32 * lh) * 0.5;
            for (k, l) in lines.iter().enumerate() {
                c.text(Face::Body, 14.5, x + 6.0, ty + k as f32 * lh, lh, l, tone(n.tone, DIM));
            }
        }
        y += 30.0;
        if let Some(n) = &s.problem {
            y += note(c, r[0], y, r[2], &n.text, n.tone);
        }
        y - y0
    });
    if s.can_cancel && c.button(r[0], actions, w::CANCEL, false, Btn::Ghost, false).0 {
        out.push(Action::CancelSetup);
    }
    let cw = c.btn_width(w::CONTINUE, false, Btn::Primary);
    if c.button(r[0] + r[2] - cw, actions, w::CONTINUE, false, Btn::Primary, !s.can_continue).0 {
        out.push(Action::ToBeta);
    }
}

/// An icon in an 24-unit box at `r` (x, y, side), as the page's SVG paths draw it.
fn icon(c: &mut Canvas, x: f32, y: f32, side: f32, width: f32, col: Rgba, paths: &[&[[f32; 2]]]) {
    let k = side / 24.0;
    for p in paths {
        let pts: Vec<[f32; 2]> = p.iter().map(|q| [x + q[0] * k, y + q[1] * k]).collect();
        if pts.len() == 1 {
            c.dot(pts[0], width * k * 0.5, col);
        } else {
            c.path(&pts, width * k, col);
        }
    }
}

/// An icon's strokes, each a polyline (one point: a dot).
type Icon = &'static [&'static [[f32; 2]]];

const ICON_OK: Icon = &[&[[5.0, 12.0], [10.0, 17.0], [20.0, 7.0]]];
const ICON_NO: Icon = &[&[[12.0, 8.0], [12.0, 13.0]], &[[12.0, 16.0], [12.0, 16.5]]];
const ICON_X: Icon = &[&[[6.0, 6.0], [18.0, 18.0]], &[[18.0, 6.0], [6.0, 18.0]]];
const ICON_FOLDER: Icon = &[&[[3.0, 7.0], [8.0, 7.0], [10.0, 9.0], [21.0, 9.0], [21.0, 19.0], [3.0, 19.0], [3.0, 7.0]]];
const ICON_DOWN: Icon =
    &[&[[12.0, 3.0], [12.0, 15.0]], &[[7.0, 10.0], [12.0, 15.0], [17.0, 10.0]], &[[5.0, 21.0], [19.0, 21.0]]];
/// M7 4l13 8-13 8z
const ICON_PLAY: Icon = &[&[[7.0, 4.0], [20.0, 12.0], [7.0, 20.0], [7.0, 4.0]]];
/// M21 12a9 9 0 1 1-3-6.7L21 8M21 3v5h-5: the arc as a polyline, 30 degrees a point.
const ICON_CHECK: Icon = &[
    &[
        [21.0, 12.0],
        [19.794, 16.5],
        [16.5, 19.794],
        [12.0, 21.0],
        [7.5, 19.794],
        [4.206, 16.5],
        [3.0, 12.0],
        [4.206, 7.5],
        [7.5, 4.206],
        [12.0, 3.0],
        [16.5, 4.206],
        [18.0, 5.3],
        [21.0, 8.0],
    ],
    &[[21.0, 3.0], [21.0, 8.0], [16.0, 8.0]],
];

fn beta(c: &mut Canvas, m: &Model, st: &mut PageState, r: [f32; 4], out: &mut Vec<Action>) {
    let b = &m.beta;
    let y = head(c, r, &w::step_of(2), None, w::BETA_TITLE, Some(w::BETA_LEAD));
    let actions = r[1] + r[3] - 38.0;
    let br = [r[0], y + 18.0, r[2], actions - 16.0 - (y + 18.0)];
    let mut guide = false;
    body(c, br, &mut st.scroll[1], |c, mut y| {
        let y0 = y;
        if let Some(why) = &b.why {
            y += note(c, r[0], y - 10.0, r[2], &w::beta_again(why), Tone::Warn) - 10.0 + 10.0;
        }
        match &b.sources {
            None => y += note(c, r[0], y - 10.0, r[2], w::LOOKING, Tone::Plain) - 10.0,
            Some(list) => {
                for (k, s) in list.iter().enumerate() {
                    if k > 0 {
                        y += 10.0;
                    }
                    let chosen = b.chosen.as_ref() == Some(&s.paks);
                    let lh = c.fonts.normal(Face::Mono, 15.0 * c.s) / c.s;
                    let h = 12.0 + (15.0 * 1.45 + 6.0 + lh + 12.0).max(38.0) + 12.0;
                    let row = [r[0], y, r[2], h];
                    let hover = s.usable && c.hover(row);
                    let (bg, border) = if chosen {
                        (rgba(63, 224, 138, 0.08), rgba(63, 224, 138, 0.35))
                    } else if hover {
                        (rgba(255, 255, 255, 0.03), rgba(63, 224, 138, 0.3))
                    } else {
                        (rgba(255, 255, 255, 0.03), LINE)
                    };
                    c.framed(row, Fill::Solid(bg), 1.0, border, 12.0);
                    let ic = [r[0] + 15.0, y + (h - 38.0) * 0.5, 38.0, 38.0];
                    let (ibg, icol) =
                        if s.usable { (rgba(63, 224, 138, 0.18), GOOD) } else { (rgba(255, 255, 255, 0.06), FAINT) };
                    c.solid(ic, ibg, 10.0);
                    icon(c, ic[0] + 9.0, ic[1] + 9.0, 20.0, 2.4, icol, if s.usable { ICON_OK } else { ICON_NO });
                    let tx = ic[0] + 38.0 + 14.0;
                    let tw = r[0] + r[2] - 15.0 - tx;
                    let ty = y + (h - (15.0 * 1.45 + 6.0 + lh + 12.0)) * 0.5;
                    c.text_cut(Face::Semi, 15.0, tx, ty, 15.0 * 1.45, &s.note, WHITE, tw);
                    path_box(c, [tx, ty + 15.0 * 1.45 + 6.0, tw], &s.path, [8.0, 5.0]);
                    if hover && c.pointer.click {
                        out.push(Action::Choose(s.paks.clone()));
                    }
                    y += h;
                }
                if list.is_empty() {
                    y += note(c, r[0], y - 10.0, r[2], w::NONE_FOUND, Tone::Plain) - 10.0;
                }
            }
        }
        y += 10.0;
        let (pick, bw) = c.button(r[0], y, w::CHOOSE_FOLDER, true, Btn::Plain, false);
        if pick {
            out.push(Action::PickBeta);
        }
        if c.button(r[0] + bw + 8.0, y, w::AUTO_DETECT, true, Btn::Ghost, false).0 {
            out.push(Action::Detect);
        }
        y += 30.0 + 22.0;
        // The help row.
        let sw = c.btn_width(w::SHOW_ME, true, Btn::Plain);
        let tw = r[2] - 32.0 - 20.0 - 28.0 - sw;
        let lines = c.lines(Face::Body, 14.5, tw, w::NO_BETA_TEXT);
        let text_h = 13.5 * 1.4 + 2.0 + lines.len() as f32 * 13.0 * 1.4;
        let h = text_h.max(30.0) + 28.0;
        c.framed([r[0], y, r[2], h], Fill::Solid(rgba(255, 255, 255, 0.025)), 1.0, LINE, 12.0);
        let ix = [r[0] + 16.0, y + (h - 20.0) * 0.5];
        let mut cyan = CYAN;
        cyan[3] = 0.8;
        c.ring([ix[0] + 10.0, ix[1] + 10.0], 7.5, 1.7, cyan, None);
        icon(c, ix[0], ix[1], 20.0, 2.0, cyan, ICON_NO);
        let tx = ix[0] + 20.0 + 14.0;
        let ty = y + (h - text_h) * 0.5;
        c.text(Face::Semi, 15.0, tx, ty, 13.5 * 1.4, w::NO_BETA, WHITE);
        for (k, l) in lines.iter().enumerate() {
            c.text(Face::Body, 14.5, tx, ty + 13.5 * 1.4 + 2.0 + k as f32 * 18.2, 18.2, l, rgba(226, 236, 255, 0.8));
        }
        guide = c.button(r[0] + r[2] - 16.0 - sw, y + (h - 30.0) * 0.5, w::SHOW_ME, true, Btn::Plain, false).0;
        y += h;
        y - y0
    });
    if guide {
        st.guide = true;
    }
    if c.button(r[0], actions, w::BACK, false, Btn::Ghost, false).0 {
        out.push(Action::BackToInstall);
    }
    let cw = c.btn_width(w::CONTINUE, false, Btn::Primary);
    if c.button(r[0] + r[2] - cw, actions, w::CONTINUE, false, Btn::Primary, b.chosen.is_none()).0 {
        out.push(Action::StartBuild);
    }
}

fn build(c: &mut Canvas, m: &Model, st: &mut PageState, r: [f32; 4], out: &mut Vec<Action>) {
    let b = &m.build;
    let stopped = b.stopped.is_some();
    let title = if stopped { w::BUILD_STOPPED } else { w::BUILDING };
    let mut y = head(c, r, &w::step_of(3), (!stopped).then_some(w::ONLY_ONCE), title, None);
    // The hero: the percent, the facts beside it, the bar, the place.
    y += 26.0;
    let f = b.frac.clamp(0.0, 1.0) as f32;
    let pct = ((f * 100.0).floor() as u32).to_string();
    let pl = 84.0;
    let pw = c.width(Face::Display, 100.0, -1.0, &pct);
    // text-shadow: 0 6px 18px rgba(3,10,40,.35)
    c.text_with(
        Face::Display,
        100.0,
        -1.0,
        r[0],
        y + 2.0 + 6.0,
        pl,
        &pct,
        Fill::Solid(rgba(3, 10, 40, 0.25)),
        None,
        1.0,
    );
    c.text_with(Face::Display, 100.0, -1.0, r[0], y + 2.0, pl, &pct, Fill::Solid(hex(0xe6eeff, 1.0)), None, 1.0);
    let sl = c.fonts.normal(Face::Display, 40.0 * c.s) / c.s;
    let base = y + 2.0 + c.fonts.baseline(Face::Display, 100.0 * c.s, pl * c.s) / c.s;
    let small_top = base - c.fonts.baseline(Face::Display, 40.0 * c.s, sl * c.s) / c.s;
    c.text(Face::Display, 40.0, r[0] + pw + 2.0, small_top, sl, "%", FAINT);
    let hero_h = pl + 2.0;
    let fx = r[0] + 184.0_f32.max(pw + 30.0);
    c.solid([fx, y, 1.0, hero_h], LINE, 0.0);
    let tx = fx + 1.0 + 26.0;
    let tw = r[0] + r[2] - tx;
    let facts_h = 17.0 * 1.45 + 19.0 + 8.0 + 19.0;
    let mut fy = y + (hero_h - facts_h) * 0.5;
    c.text_cut(Face::Semi, 17.0, tx, fy, 17.0 * 1.45, &b.now, WHITE, tw);
    fy += 17.0 * 1.45;
    c.text_cut(Face::Body, 14.5, tx, fy, 19.0, &b.count, DIM, tw);
    fy += 19.0 + 8.0;
    let mut mx = tx;
    if !b.left.is_empty() {
        mx += c.text(Face::Body, 14.5, mx, fy, 19.0, &b.left, DIM);
    }
    if !b.written.is_empty() {
        if !b.left.is_empty() {
            mx += 8.0;
            mx += c.text(Face::Body, 14.5, mx, fy, 19.0, "\u{b7}", FAINT) + 8.0;
        }
        c.text(Face::Medium, 14.5, mx, fy, 19.0, &b.written, WHITE);
    }
    y += hero_h + 26.0;
    let blue = [
        (0.0, rgba(169, 198, 255, 0.45)),
        (0.25, rgba(169, 198, 255, 0.7)),
        (0.5, rgba(225, 235, 255, 0.85)),
        (0.75, rgba(169, 198, 255, 0.7)),
        (1.0, rgba(169, 198, 255, 0.45)),
    ];
    bar(c, [r[0], y, r[2], 8.0], f, &blue, true);
    y += 8.0 + 10.0;
    c.text_cut(Face::Body, 13.5, r[0], y, 19.6, &b.place, FAINT, r[2]);
    y += 19.6 + 18.0;
    c.hline(r[0], y, r[2], LINE);
    y += 1.0;
    if let Some(msg) = &b.stopped {
        y += note(c, r[0], y, r[2], msg, Tone::Bad);
    }
    let actions = r[1] + r[3] - 38.0;
    let br = [r[0], y + 12.0, r[2], actions - 14.0 - (y + 12.0)];
    let details = st.details;
    body(c, br, &mut st.scroll[2], |c, y| {
        if details {
            let lh = 13.5 * 1.6;
            let mut ly = y;
            for (time, t, text) in &b.log {
                let col = tone(*t, rgba(215, 228, 255, 0.78));
                let line = format!("{time}  {text}");
                for l in c.lines(Face::Mono, 13.5, r[2], &line) {
                    c.text(Face::Mono, 13.5, r[0], ly, lh, &l, col);
                    ly += lh;
                }
            }
            ly - y
        } else {
            // two columns of four rows, filled down
            let col_w = (r[2] - 28.0) * 0.5;
            let rh = 8.0 + 19.6 + 8.0;
            for (k, p) in b.phases.iter().enumerate() {
                let (cx, row) = (r[0] + (k / 4) as f32 * (col_w + 28.0), (k % 4) as f32);
                let py = y + row * (rh + 1.0);
                if k % 4 != 3 && k + 1 != b.phases.len() {
                    c.hline(cx, py + rh, col_w, LINE);
                }
                let centre = [cx + 9.0, py + rh * 0.5];
                match p.state {
                    PhaseState::Done => {
                        c.dot(centre, 9.0, GOOD);
                        check(c, centre, 0.75, hex(0x06301a, 1.0), 2.0);
                    }
                    PhaseState::Now => {
                        c.ring(centre, 9.0, 2.0, rgba(255, 255, 255, 0.2), None);
                        let a = (c.t / 0.9).fract() as f32 * 2.0 * PI;
                        c.ring(centre, 9.0, 2.0, CYAN, Some([a - 0.75 * PI + PI * 1.5, PI * 0.5]));
                    }
                    PhaseState::Wait => c.ring(centre, 9.0, 2.0, rgba(255, 255, 255, 0.2), None),
                }
                let col = match p.state {
                    PhaseState::Done => DIM,
                    PhaseState::Now => WHITE,
                    PhaseState::Wait => FAINT,
                };
                let nw = if p.n.is_empty() { 0.0 } else { c.width(Face::Body, 14.0, 0.0, &p.n) };
                c.text_cut(Face::Body, 15.0, cx + 32.0, py + 8.0, 19.6, &p.name, col, col_w - 32.0 - nw - 10.0);
                if !p.n.is_empty() {
                    c.text(Face::Body, 14.0, cx + col_w - nw, py + 8.0, 19.6, &p.n, FAINT);
                }
            }
            4.0 * (rh + 1.0)
        }
    });
    let label = if st.details { w::HIDE_DETAILS } else { w::SHOW_DETAILS };
    if c.button(r[0], actions, label, false, Btn::Ghost, false).0 {
        st.details = !st.details;
        st.scroll[2] = if st.details { 1e9 } else { 0.0 };
    }
    let mut x = r[0] + r[2];
    if stopped {
        x -= c.btn_width(w::TRY_AGAIN, false, Btn::Primary);
        if c.button(x, actions, w::TRY_AGAIN, false, Btn::Primary, false).0 {
            out.push(Action::Retry);
        }
        x -= 10.0 + c.btn_width(w::BACK_TO_SETUP, false, Btn::Ghost);
        if c.button(x, actions, w::BACK_TO_SETUP, false, Btn::Ghost, false).0 {
            out.push(Action::BackToSetup);
        }
    } else {
        x -= c.btn_width(w::CANCEL, false, Btn::Plain);
        if c.button(x, actions, w::CANCEL, false, Btn::Plain, false).0 {
            out.push(Action::CancelBuild);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The guide, over setup

/// A centred dialog's box (.dialog: translate(-50%, -48%)) with its scrim.
fn dialog(c: &mut Canvas, wd: f32, h: f32) -> [f32; 4] {
    let full = [0.0, 0.0, c.size[0], c.size[1]];
    c.solid(full, rgba(3, 8, 28, 0.6), 0.0);
    let r = [(c.size[0] - wd) * 0.5, c.size[1] * 0.5 - 0.48 * h, wd, h];
    c.shadow([r[0], r[1] + 30.0, r[2], r[3]], rgba(0, 0, 0, 0.6), 14.0, 80.0);
    panel(c, r, PANEL_SOLID, rgba(255, 255, 255, 0.14));
    r
}

/// The round close button (.iconbtn.close); its click.
fn close_button(c: &mut Canvas, x: f32, y: f32) -> bool {
    let r = [x, y, 38.0, 38.0];
    let hover = c.hover(r);
    c.framed(r, Fill::Solid(if hover { rgba(255, 255, 255, 0.1) } else { rgba(8, 22, 60, 0.5) }), 1.0, LINE, 19.0);
    icon(c, x + 10.0, y + 10.0, 18.0, 2.0, if hover { WHITE } else { DIM }, ICON_X);
    hover && c.pointer.click
}

/// Words with `*bold*` parts, wrapped to `wd`: each line's runs (bold, text).
fn rich_lines(c: &mut Canvas, px: f32, wd: f32, text: &str) -> Vec<Vec<(bool, String)>> {
    let mut words: Vec<(bool, String)> = Vec::new();
    for (k, part) in text.split('*').enumerate() {
        for word in part.split(' ').filter(|w| !w.is_empty()) {
            words.push((k % 2 == 1, word.to_string()));
        }
    }
    let space = c.width(Face::Body, px, 0.0, " ");
    let mut lines = vec![Vec::new()];
    let mut x = 0.0;
    for (bold, word) in words {
        let ww = c.width(if bold { Face::Bold } else { Face::Body }, px, 0.0, &word);
        let line = lines.last_mut().expect("a line");
        if !line.is_empty() && x + space + ww > wd {
            lines.push(vec![(bold, word)]);
            x = ww;
        } else {
            x += if line.is_empty() { ww } else { space + ww };
            line.push((bold, word));
        }
    }
    lines
}

#[allow(clippy::too_many_arguments)]
fn rich(c: &mut Canvas, px: f32, lh: f32, x: f32, y: f32, wd: f32, text: &str, col: Rgba) -> f32 {
    let lines = rich_lines(c, px, wd, text);
    let space = c.width(Face::Body, px, 0.0, " ");
    for (k, line) in lines.iter().enumerate() {
        let mut lx = x;
        for (bold, word) in line {
            lx += c.text(
                if *bold { Face::Bold } else { Face::Body },
                px,
                lx,
                y + k as f32 * lh,
                lh,
                word,
                if *bold { WHITE } else { col },
            ) + space;
        }
    }
    lines.len() as f32 * lh
}

fn guide(c: &mut Canvas, st: &mut PageState, out: &mut Vec<Action>) {
    let (wd, px, lh) = (640.0, 15.0, 15.0 * 1.55);
    let text = rgba(232, 240, 255, 0.86);
    let inner_w = wd - 60.0;
    let li_w = inner_w - 20.0;
    // Measure, then draw: the dialog is as tall as its text.
    let h_title = c.fonts.normal(Face::Display, 26.0 * c.s) / c.s;
    let items_h = |c: &mut Canvas| -> f32 {
        let intro = c.lines(Face::Body, px, inner_w, w::GUIDE_INTRO).len() as f32 * lh + 12.0;
        let l1 = lh + 12.0;
        let l2 = lh + 6.0 + 30.0 + 2.0 + c.lines(Face::Body, px, li_w, w::GUIDE_2_OR).len() as f32 * lh + 12.0;
        let l3 = lh + 6.0 + 30.0 + 12.0 + 12.0;
        let l4 = c.lines(Face::Body, px, li_w, w::GUIDE_4).len() as f32 * lh + 12.0;
        let l5 = rich_lines(c, px, li_w, w::GUIDE_5).len() as f32 * lh + 12.0;
        intro + l1 + l2 + l3 + l4 + l5
    };
    let body_h = items_h(c);
    let h = (26.0 + h_title + 14.0 + body_h + 14.0 + 38.0 + 26.0).min(560.0);
    let r = dialog(c, wd, h);
    let (x, mut y) = (r[0] + 30.0, r[1] + 26.0);
    c.text_with(
        Face::Display,
        26.0,
        0.26,
        x,
        y,
        h_title,
        &w::GUIDE_TITLE.to_uppercase(),
        Fill::Solid(WHITE),
        None,
        1.0,
    );
    if close_button(c, r[0] + wd - 16.0 - 38.0, r[1] + 16.0) {
        st.guide = false;
    }
    y += h_title + 14.0;
    y += c.para(Face::Body, px, lh, x, y, inner_w, w::GUIDE_INTRO, text) + 12.0;
    let lx = x + 20.0;
    let marker = |c: &mut Canvas, n: u32, y: f32| {
        let t = format!("{n}.");
        let tw = c.width(Face::Bold, px, 0.0, &t);
        c.text(Face::Bold, px, lx - 6.0 - tw, y, lh, &t, ORANGE);
    };
    marker(c, 1, y);
    c.text(Face::Body, px, lx, y, lh, w::GUIDE_1, text);
    y += lh + 12.0;
    marker(c, 2, y);
    c.text(Face::Body, px, lx, y, lh, w::GUIDE_2, text);
    y += lh + 6.0;
    if c.button(lx, y, w::STEAM_CONSOLE, true, Btn::Plain, false).0 {
        out.push(Action::SteamConsole);
    }
    y += 32.0;
    y += c.para(Face::Body, px, lh, lx, y, li_w, w::GUIDE_2_OR, text) + 12.0;
    marker(c, 3, y);
    c.text(Face::Body, px, lx, y, lh, w::GUIDE_3, text);
    y += lh + 6.0;
    let cmd = [lx, y, li_w, 30.0 + 12.0];
    c.framed(cmd, Fill::Solid(rgba(0, 0, 0, 0.3)), 1.0, LINE, 8.0);
    let ml = c.fonts.normal(Face::Mono, 15.0 * c.s) / c.s;
    c.text(Face::Mono, 14.0, lx + 11.0, y + (cmd[3] - ml) * 0.5, ml, w::DEPOT, hex(0xe8e3ff, 1.0));
    let copied = st.copied_at.is_some_and(|t| c.t - t < 2.0);
    let label = if copied { w::COPIED } else { w::COPY };
    let bw = c.btn_width(label, true, Btn::Ghost);
    if c.button(lx + li_w - 6.0 - bw, y + 6.0, label, true, Btn::Ghost, false).0 {
        st.copied_at = Some(c.t);
        out.push(Action::CopyDepot);
    }
    y += cmd[3] + 12.0;
    marker(c, 4, y);
    y += c.para(Face::Body, px, lh, lx, y, li_w, w::GUIDE_4, text) + 12.0;
    marker(c, 5, y);
    rich(c, px, lh, lx, y, li_w, w::GUIDE_5, text);
    let gw = c.btn_width(w::GOT_IT, false, Btn::Primary);
    if c.button(r[0] + wd - 30.0 - gw, r[1] + h - 26.0 - 38.0, w::GOT_IT, false, Btn::Primary, false).0 {
        st.guide = false;
    }
    // (a click on the scrim closes it)
    if c.pointer.click && !c.hover(r) {
        st.guide = false;
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Home

fn home(c: &mut Canvas, m: &Model, st: &mut PageState, stars: &[Star], out: &mut Vec<Action>) {
    let on = st.picture_at.map_or(0.0, |t0| ((c.t - t0) / 0.6).clamp(0.0, 1.0) as f32);
    sky::art(c, stars, c.p.has_picture(), on);
    moon_and_logo(c);
    let h = &m.home;
    if h.state == HomeState::Problem && banner(c, 52.0, 640.0, &h.problem, Some(w::LOCATE_IT), rgba(60, 8, 24, 0.9)) {
        out.push(Action::Locate);
    }
    play_zone(c, m, st, out);
    // The settings button, top right: left of the window buttons, 12 px from them (.topright).
    let r = [c.size[0] - 2.0 * WINDOW_BUTTON - 12.0 - 34.0, 7.0, 34.0, 34.0];
    let hover = c.hover(r);
    if hover {
        c.solid(r, rgba(255, 255, 255, 0.08), 17.0);
    }
    gear(c, [r[0] + 17.0, r[1] + 17.0], if hover { WHITE } else { DIM });
    if hover && c.pointer.click {
        st.settings = true;
        st.menu = false;
        st.tab = 0;
        out.push(Action::SettingsOpened);
    }
}

/// (the port's own) Home's left: a moon high up with its glow, a cloud drifting slowly across its foot, and under
/// them the wordmark, lit a little by the moon, with a gold rule and the tagline.
fn moon_and_logo(c: &mut Canvas) {
    let t = c.t as f32;
    // (a glow's gradient stays fixed and its breathing is the shape's opacity: the painter keeps every gradient it
    // is given, so colours that changed each frame would fill its table)
    let soft = |c: &mut Canvas, centre: [f32; 2], radii: [f32; 2], col: Rgba, o: f32| {
        let f = c.radial(&[(0.0, col), (1.0, [col[0], col[1], col[2], 0.0])], centre, radii);
        let mut s =
            Shape::new(Kind::Rect, [centre[0] - radii[0], centre[1] - radii[1], 2.0 * radii[0], 2.0 * radii[1]], f);
        s.opacity = o;
        c.push(s);
    };
    // The moon: its glow breathing slowly, a disc lit from its upper left, a few soft craters.
    let (m, r) = ([440.0, 92.0], 36.0);
    let breathe = 0.92 + 0.08 * (t * 0.35).sin();
    soft(c, m, [190.0, 190.0], rgba(150, 180, 255, 0.16), breathe);
    soft(c, m, [88.0, 88.0], rgba(205, 220, 255, 0.22), breathe);
    let disc = [m[0] - r, m[1] - r, 2.0 * r, 2.0 * r];
    let f = c.radial(
        &[(0.0, hex(0xfffaf0, 1.0)), (0.55, hex(0xeceaf4, 1.0)), (1.0, hex(0xb9c3e4, 1.0))],
        [m[0] - 0.35 * r, m[1] - 0.4 * r],
        [1.6 * r, 1.6 * r],
    );
    c.rect(disc, f, r);
    for (dx, dy, cr, a) in
        [(-0.3, 0.15, 0.2, 0.16), (0.28, -0.22, 0.13, 0.13), (0.18, 0.4, 0.16, 0.12), (-0.05, -0.45, 0.08, 0.1)]
    {
        c.dot([m[0] + dx * r, m[1] + dy * r], cr * r, rgba(120, 135, 190, a));
    }
    // The cloud, painted like the picture's at the top right: one broad dome, rounder at its left end, tapering to the
    // right, navy purple at its top and periwinkle at its gently wavy lower edge; across the moon's foot, drifting
    // 24 px each way over 90 s, at 0.82 of its size. (stacked soft shapes in one gradient: only its outline shows)
    let dx = 24.0 * (t * std::f32::consts::TAU / 90.0).sin();
    let o = [m[0] - 10.0 + dx, m[1] + 1.15 * r];
    let fill = c.down(
        &[(0.0, rgba(48, 56, 128, 1.0)), (0.5, rgba(58, 67, 152, 1.0)), (1.0, rgba(74, 84, 182, 1.0))],
        [o[0], o[1] - 34.0, 1.0, 84.0],
    );
    let parts = [
        // the dome, widest at its foot
        (-150.0, -4.0, 300.0, 48.0),
        (-130.0, -18.0, 230.0, 56.0),
        (-104.0, -30.0, 160.0, 56.0),
        (-70.0, -36.0, 92.0, 44.0),
        // the round left end
        (-176.0, -12.0, 84.0, 56.0),
        // the taper at the right
        (100.0, 8.0, 96.0, 34.0),
        (170.0, 18.0, 48.0, 22.0),
        // the lower edge: feet at different depths
        (-168.0, 24.0, 110.0, 24.0),
        (-80.0, 28.0, 120.0, 22.0),
        (20.0, 24.0, 110.0, 26.0),
        (120.0, 22.0, 70.0, 22.0),
    ];
    for (px, py, pw, ph) in parts {
        let (k, ph) = (0.82, ph * 0.82);
        let mut sh = Shape::new(Kind::Rect, [o[0] + k * px, o[1] + k * py, k * pw, ph], fill);
        sh.radius = ph.min(k * pw) * 0.5;
        sh.soft = 2.0;
        c.push(sh);
    }
    // The wordmark, its moonlight, the rule and the tagline.
    let (x, y) = (64.0, 96.0);
    soft(c, [x + 120.0, y + 40.0], [230.0, 120.0], rgba(150, 180, 255, 0.07), breathe);
    let [_, wh] = sky::wordmark(c, x - 4.0, y, 104.0);
    let ry = y + wh + 26.0;
    let f = c.linear(&[(0.0, hex(0xf9c850, 0.9)), (1.0, hex(0xe27f2c, 0.0))], [x, ry], [x + 120.0, ry]);
    c.rect([x, ry, 120.0, 2.0], f, 1.0);
    c.para(Face::Body, 17.0, 17.0 * 1.45, x, ry + 16.0, 320.0, w::TAGLINE, DIM);
}

/// The settings cogwheel: a solid wheel with a round hole and eight rounded teeth (17 px across).
fn gear(c: &mut Canvas, at: [f32; 2], col: Rgba) {
    let k = 17.0 / 24.0;
    // (the wheel: a ring's border lies inside its radius, so a band from the hole's radius 3 out to 7.4)
    c.ring(at, 7.4 * k, 4.4 * k, col, None);
    for n in 0..8 {
        let a = n as f32 * PI / 4.0 + PI / 8.0;
        let (s, co) = a.sin_cos();
        // (a segment's round caps stick out by half its width: from 7 to 10.4 with the caps)
        c.seg([at[0] + co * 8.2 * k, at[1] + s * 8.2 * k], [at[0] + co * 9.0 * k, at[1] + s * 9.0 * k], 2.8 * k, col);
    }
}

/// (the port's own) Home's foot: one bar across the page, a dark band fading in from clear at its top; at its left the
/// status line with the progress value at its right, a load bar under it (the run's progress, full and green when
/// ready or running, red on a problem, empty with an update waiting) and a detail line; at its right PLAY and the
/// more button, the more menu opening above it.
fn play_zone(c: &mut Canvas, m: &Model, st: &mut PageState, out: &mut Vec<Action>) {
    let h = &m.home;
    let [pw, ph] = c.size;
    let (bar_h, pad, btn_h, more_w, play_w) = (104.0, 32.0, 72.0, 72.0, 300.0);
    // (its contents 20 px off the page's foot; its band fades in over the 70 px above them, no hard edge)
    let top = ph - 20.0 - bar_h;
    let fade = 70.0;
    let band = [0.0, top - fade, pw, ph - top + fade];
    let f = c.down(
        &[
            (0.0, rgba(5, 12, 42, 0.0)),
            (0.25, rgba(5, 12, 42, 0.22)),
            (0.5, rgba(5, 12, 42, 0.55)),
            (0.7, rgba(4, 10, 36, 0.72)),
            (1.0, rgba(3, 8, 30, 0.9)),
        ],
        band,
    );
    c.rect(band, f, 0.0);
    let row_y = top + (bar_h - btn_h) * 0.5;
    let more_x = pw - pad - more_w;
    let x = more_x - 12.0 - play_w;
    // The left: the status line, the load bar, the detail line.
    let (lx, lw) = (pad, x - 48.0 - pad);
    let progress = match h.state {
        HomeState::Rebuild => {
            let b = &m.build;
            let pct = (b.frac.clamp(0.0, 1.0) * 100.0).floor() as u32;
            let value = if b.written.is_empty() { format!("{pct}%") } else { format!("{} \u{b7} {pct}%", b.written) };
            Some((b.now.as_str(), value, b.frac))
        }
        HomeState::Updating => {
            Some((h.progress_label.as_str(), format!("{}%", h.percent.floor()), h.percent.clamp(0.0, 100.0) / 100.0))
        }
        _ => None,
    };
    let (dot, bold, rest) = &h.status;
    let sy = top + 16.0;
    let lh = 16.0 * 1.45;
    let dc = tone(*dot, GOOD);
    c.shadow([lx - 1.0, sy + lh * 0.5 - 5.0, 10.0, 10.0], dc, 5.0, 10.0);
    c.dot([lx + 4.0, sy + lh * 0.5], 4.0, dc);
    c.text(Face::Semi, 16.0, lx + 18.0, sy, lh, bold, WHITE);
    if let Some((_, value, _)) = &progress {
        let vw = c.width(Face::Body, 14.5, 0.0, value);
        c.text(Face::Body, 14.5, lx + lw - vw, sy + 1.0, lh, value, DIM);
    }
    let green = [(0.0, rgba(70, 190, 140, 0.9)), (1.0, rgba(99, 214, 160, 0.9))];
    let red = [(0.0, rgba(255, 84, 112, 0.5)), (1.0, rgba(255, 84, 112, 0.7))];
    let by = top + 50.0;
    match (&progress, h.state) {
        (Some((_, _, frac)), _) => bar(c, [lx, by, lw, 8.0], *frac as f32, &amber(), true),
        // (the ready and problem bars stand still: a sweep there looks like work going on)
        (None, HomeState::Ready | HomeState::Running) => bar(c, [lx, by, lw, 8.0], 1.0, &green, false),
        (None, HomeState::Problem) => bar(c, [lx, by, lw, 8.0], 1.0, &red, false),
        _ => bar(c, [lx, by, lw, 8.0], 0.0, &green, false),
    }
    let dy = by + 8.0 + 8.0;
    let detail = progress.as_ref().map_or(rest.as_str(), |p| p.0);
    let hint_w = if h.state == HomeState::Running { c.width(Face::Body, 14.0, 0.0, w::PLAY_HINT) + 16.0 } else { 0.0 };
    c.text_cut(Face::Body, 14.5, lx, dy, 21.0, detail, FAINT, lw - hint_w);
    // (.playhint: while the game runs, at the detail line's right)
    if hint_w > 0.0 {
        c.text(Face::Body, 14.0, lx + lw - hint_w + 16.0, dy, 21.0, w::PLAY_HINT, DIM);
    }
    // PLAY and the more button.
    let pr = [x, row_y, play_w, btn_h];
    let sk = (-8.0f32).to_radians().tan();
    // skewX(-8deg): x += tan(-8deg) * y
    let skew = [1.0, sk, 0.0, 1.0];
    let hover = !c.blocked && c.hover(pr);
    // (the port's own) A Test connection under way holds PLAY as a rebuild does: its words say so
    let play = match h.state {
        HomeState::Ready | HomeState::Update | HomeState::Problem if h.testing => HomeState::Rebuild,
        s => s,
    };
    let lift = match (play, hover, c.pointer.down) {
        (HomeState::Ready | HomeState::Update, true, true) => 5.0,
        (HomeState::Ready | HomeState::Update, true, false) => -2.0,
        _ => 0.0,
    };
    let r = [pr[0], pr[1] + lift, pr[2], pr[3]];
    c.xf = Some((skew, [r[0] + r[2] * 0.5, r[1] + r[3] * 0.5]));
    let (text, px, col, shadow) = match play {
        HomeState::Ready => {
            let ledge = if lift > 0.0 {
                2.0
            } else if lift < 0.0 {
                7.0
            } else {
                6.0
            };
            c.shadow([r[0], r[1] + 12.0, r[2], r[3]], rgba(233, 168, 74, if hover { 0.26 } else { 0.18 }), 14.0, 28.0);
            c.solid([r[0], r[1] + ledge, r[2], r[3]], hex(0x7d4a17, 1.0), 14.0);
            let stops = if hover {
                [(0.0, hex(0xf9d685, 1.0)), (0.5, hex(0xf2b257, 1.0)), (1.0, hex(0xd98b35, 1.0))]
            } else {
                [(0.0, hex(0xf7cd78, 1.0)), (0.5, ORANGE), (1.0, hex(0xcf8432, 1.0))]
            };
            let f = c.down(&stops, r);
            c.rect(r, f, 14.0);
            c.solid([r[0] + 12.0, r[1], r[2] - 24.0, 2.0], rgba(255, 240, 210, 0.55), 1.0);
            (w::PLAY, 46.0, hex(0xfff8ea, 1.0), Some(hex(0x8a5220, 1.0)))
        }
        // .home[data-state="update"] .playbtn: gold; (the port's own) UPDATE in PLAY's white
        HomeState::Update => {
            let ledge = if lift > 0.0 {
                2.0
            } else if lift < 0.0 {
                7.0
            } else {
                6.0
            };
            c.shadow([r[0], r[1] + 12.0, r[2], r[3]], rgba(244, 207, 122, if hover { 0.24 } else { 0.16 }), 14.0, 28.0);
            c.solid([r[0], r[1] + ledge, r[2], r[3]], hex(0x7d5a1c, 1.0), 14.0);
            // (the page's hover is brightness(1.05) over the same gradient)
            let k = if hover { 1.05 } else { 1.0 };
            let lit = |c: Rgba| [(c[0] * k).min(1.0), (c[1] * k).min(1.0), (c[2] * k).min(1.0), c[3]];
            let stops = [(0.0, lit(hex(0xfbe6ad, 1.0))), (0.5, lit(GOLD)), (1.0, lit(hex(0xd9a24a, 1.0)))];
            let f = c.down(&stops, r);
            c.rect(r, f, 14.0);
            c.solid([r[0] + 12.0, r[1], r[2] - 24.0, 2.0], rgba(255, 255, 255, 0.6), 1.0);
            (w::UPDATE, 46.0, hex(0xfff8ea, 1.0), Some(hex(0x8a6420, 1.0)))
        }
        HomeState::Rebuild | HomeState::Updating => {
            c.framed(r, Fill::Solid(rgba(255, 255, 255, 0.1)), 1.0, rgba(255, 255, 255, 0.16), 14.0);
            let text = if h.state == HomeState::Rebuild {
                w::PLAY_BUILDING
            } else if h.state == HomeState::Updating {
                w::UPDATING
            } else {
                w::PLAY_TESTING
            };
            (text, 30.0, rgba(255, 255, 255, 0.7), None)
        }
        HomeState::Problem => {
            let f = c.down(&[(0.0, rgba(255, 84, 112, 0.28)), (1.0, rgba(255, 84, 112, 0.12))], r);
            c.framed(r, f, 1.0, rgba(255, 84, 112, 0.55), 14.0);
            (w::FIX_IT, 30.0, hex(0xffd7de, 1.0), None)
        }
        // .home[data-state="running"] .playbtn: green, its ledge dark
        HomeState::Running => {
            // (a box-shadow shows only outside its box: the ledge's strip below the translucent button)
            c.solid([r[0], r[1] + r[3] - 8.0, r[2], 14.0], rgba(8, 40, 40, 0.85), 14.0);
            let f = c.down(&[(0.0, rgba(99, 214, 160, 0.2)), (1.0, rgba(99, 214, 160, 0.08))], r);
            c.framed(r, f, 1.0, rgba(99, 214, 160, 0.42), 14.0);
            (w::RUNNING, 30.0, hex(0xcdf3e0, 1.0), None)
        }
    };
    c.xf = None;
    // Deviation: the page spaced PLAY's letters by 0.12 em; half that reads as one word.
    let spacing = 0.06 * px;
    let tw = c.width(Face::Display, px, spacing, text);
    let tl = c.fonts.normal(Face::Display, px * c.s) / c.s;
    let (tx, ty) = (r[0] + (r[2] - tw + spacing) * 0.5, r[1] + (r[3] - tl) * 0.5);
    if let Some(sc) = shadow {
        c.text_with(Face::Display, px, spacing, tx + 3.0, ty + 3.0, tl, text, Fill::Solid(sc), None, 1.0);
    }
    c.text_with(Face::Display, px, spacing, tx, ty, tl, text, Fill::Solid(col), None, 1.0);
    if hover && c.pointer.click {
        match play {
            HomeState::Ready | HomeState::Update | HomeState::Problem | HomeState::Running => out.push(Action::Play),
            HomeState::Rebuild | HomeState::Updating => {}
        }
    }
    let mr = [more_x, row_y, more_w, btn_h];
    c.xf = Some((skew, [mr[0] + mr[2] * 0.5, mr[1] + mr[3] * 0.5]));
    c.framed(mr, Fill::Solid(PANEL), 1.0, LINE, 14.0);
    c.xf = None;
    for k in 0..3 {
        c.dot(
            [mr[0] + mr[2] * 0.5 + (k as f32 - 1.0) * 7.0 * 26.0 / 24.0, mr[1] + btn_h * 0.5],
            2.0 * 26.0 / 24.0,
            WHITE,
        );
    }
    let more_clicked = !c.blocked && c.clicked(mr);
    // The more menu.
    if st.menu {
        let mut items: Vec<(&str, Icon, bool, Action)> = Vec::new();
        if h.state == HomeState::Update {
            items.push((w::PLAY_NOW, ICON_PLAY, !h.testing, Action::PlayNow));
        }
        // Deviation: hidden in a build with no feed (the page always showed it).
        if let Some(enabled) = h.check_updates {
            items.push((w::CHECK_UPDATES, ICON_CHECK, enabled, Action::CheckUpdates));
        }
        items.push((w::OPEN_INSTALL_FOLDER, ICON_FOLDER, true, Action::OpenRoot));
        // (the rule above the last item)
        let rule = items.len();
        items.push((w::REBUILD, ICON_DOWN, h.can_rebuild, Action::Rebuild));
        let ih = 9.0 + 21.0 + 9.0;
        let mh = 12.0 + ih * items.len() as f32 + 11.0;
        let r = [more_x + more_w - 260.0, top - 10.0 - mh, 260.0, mh];
        c.shadow([r[0], r[1] + 18.0, r[2], r[3]], rgba(0, 0, 0, 0.5), 12.0, 40.0);
        c.framed(r, Fill::Solid(PANEL_SOLID), 1.0, rgba(255, 255, 255, 0.14), 12.0);
        let mut iy = r[1] + 6.0;
        for (k, (label, ic, enabled, action)) in items.into_iter().enumerate() {
            if k == rule {
                c.hline(r[0] + 10.0, iy + 5.0, r[2] - 20.0, LINE);
                iy += 11.0;
            }
            let ir = [r[0] + 6.0, iy, r[2] - 12.0, ih];
            let hv = enabled && c.hover(ir);
            if hv {
                c.solid(ir, rgba(255, 255, 255, 0.08), 8.0);
            }
            let op = if enabled { 1.0 } else { 0.4 };
            let mut dim = DIM;
            dim[3] *= op;
            icon(c, ir[0] + 12.0, iy + (ih - 16.0) * 0.5, 16.0, 2.0, dim, ic);
            c.text_with(Face::Body, 14.5, 0.0, ir[0] + 12.0 + 16.0 + 10.0, iy, ih, label, Fill::Solid(WHITE), None, op);
            if hv && c.pointer.click {
                st.menu = false;
                out.push(action);
            }
            iy += ih;
        }
        if c.pointer.click && !c.hover(r) && !more_clicked {
            st.menu = false;
        }
    }
    if more_clicked {
        st.menu = !st.menu;
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Settings, over Home

const TABS: [&str; 4] = [w::TAB_GENERAL, w::TAB_FILES, w::TAB_ONLINE, w::TAB_ABOUT];
/// The Online tab's index in TABS.
pub(super) const ONLINE_TAB: usize = 2;

fn settings(c: &mut Canvas, m: &Model, st: &mut PageState, out: &mut Vec<Action>) {
    let s = &m.settings;
    let r = dialog(c, 900.0, 560.0);
    // The tabs' column.
    c.rect([r[0] + 1.0, r[1] + 1.0, 219.0, r[3] - 2.0], Fill::Solid(rgba(0, 0, 0, 0.25)), 0.0);
    c.solid([r[0] + 220.0, r[1] + 1.0, 1.0, r[3] - 2.0], LINE, 0.0);
    let (tx, mut ty) = (r[0] + 12.0, r[1] + 22.0);
    let tl = c.fonts.normal(Face::Display, 24.0 * c.s) / c.s;
    c.text_with(
        Face::Display,
        24.0,
        0.24,
        tx + 12.0,
        ty,
        tl,
        &w::SETTINGS.to_uppercase(),
        Fill::Solid(WHITE),
        None,
        1.0,
    );
    ty += tl + 18.0;
    for (k, name) in TABS.iter().enumerate() {
        let br = [tx, ty, 220.0 - 24.0, 40.0];
        let on = st.tab == k;
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
        let col = if on || hv { WHITE } else { DIM };
        let ic = [br[0] + 12.0, br[1] + 11.5];
        match k {
            0 => gear(c, [ic[0] + 8.5, ic[1] + 8.5], col),
            1 => icon(c, ic[0], ic[1], 17.0, 2.0, col, ICON_FOLDER),
            2 => {
                c.ring([ic[0] + 8.5, ic[1] + 8.5], 9.0 * 17.0 / 24.0, 2.0 * 17.0 / 24.0, col, None);
                icon(c, ic[0], ic[1], 17.0, 2.0, col, &[&[[3.0, 12.0], [21.0, 12.0]], &[[12.0, 3.0], [12.0, 21.0]]]);
            }
            _ => {
                c.ring([ic[0] + 8.5, ic[1] + 8.5], 9.0 * 17.0 / 24.0, 2.0 * 17.0 / 24.0, col, None);
                icon(c, ic[0], ic[1], 17.0, 2.0, col, &[&[[12.0, 11.0], [12.0, 17.0]], &[[12.0, 7.75]]]);
            }
        }
        c.text(Face::Medium, 15.0, br[0] + 12.0 + 17.0 + 10.0, br[1], 40.0, name, col);
        if hv && c.pointer.click {
            st.tab = k;
        }
        ty += 42.0;
    }
    // The pane.
    let (x, wd) = (r[0] + 221.0 + 30.0, r[2] - 221.0 - 60.0);
    let mut y = r[1] + 24.0;
    if close_button(c, r[0] + r[2] - 16.0 - 38.0, r[1] + 16.0) {
        st.settings = false;
    }
    c.text_with(Face::Display, 24.0, 0.24, x, y, tl, &TABS[st.tab].to_uppercase(), Fill::Solid(WHITE), None, 1.0);
    y += tl;
    let h3 = |c: &mut Canvas, y: &mut f32, t: &str| {
        *y += 16.0;
        c.text(Face::Bold, 15.0, x, *y, 15.0 * 1.45, t, WHITE);
        *y += 15.0 * 1.45 + 4.0;
    };
    let desc = |c: &mut Canvas, y: &mut f32, t: &str| {
        *y += c.para(Face::Body, 14.0, 14.0 * 1.45, x, *y, wd, t, DIM);
    };
    let end = |c: &mut Canvas, y: &mut f32, last: bool| {
        *y += 16.0;
        if !last {
            c.hline(x, *y, wd, LINE);
            *y += 1.0;
        }
    };
    match st.tab {
        // Deviation: General holds the shortcuts alone: the page's two switches (keep the launcher open, close it
        // when the game starts) were never wired in the TS, and this launcher has no window of its own to keep.
        0 => {
            h3(c, &mut y, w::SHORTCUTS);
            desc(c, &mut y, w::SHORTCUTS_DESC);
            y += 10.0;
            let sc = s.shortcuts;
            let (d, dw) = c.button(x, y, w::shortcut_desktop(sc.desktop), false, Btn::Plain, !sc.available);
            if d {
                out.push(Action::ShortcutDesktop);
            }
            if c.button(x + dw + 10.0, y, w::shortcut_start(sc.start_menu), false, Btn::Plain, !sc.available).0 {
                out.push(Action::ShortcutStartMenu);
            }
        }
        1 => {
            h3(c, &mut y, w::STATUS);
            y += 12.0 - 4.0;
            // Deviation: no third cell with the version (launcher.ts health('h-version', ...)): the launcher shows
            // no version.
            let cw = (wd - 10.0) / 2.0;
            let cells = [
                (w::GAME_DATA, s.data.0, s.data.1.as_str()),
                (w::OPEN_BETA, s.beta_health.0, s.beta_health.1.as_str()),
            ];
            let ch = 10.0 + 19.6 + 2.0 + 21.75 + 10.0 + 2.0;
            for (k, (label, t, text)) in cells.iter().enumerate() {
                let cx = x + k as f32 * (cw + 10.0);
                c.framed([cx, y, cw, ch], Fill::Solid(rgba(255, 255, 255, 0.04)), 1.0, LINE, 10.0);
                c.text(Face::Body, 13.5, cx + 13.0, y + 11.0, 19.6, label, DIM);
                let by = y + 11.0 + 19.6 + 2.0;
                let mut bx = cx + 13.0;
                c.dot([bx + 3.5, by + 10.15], 3.5, tone(*t, GOOD));
                bx += 7.0 + 7.0;
                c.text_cut(Face::Semi, 15.0, bx, by, 21.75, text, WHITE, cx + cw - 13.0 - bx);
            }
            y += ch + 10.0;
            if c.button(x, y, w::REBUILD, false, Btn::Plain, !s.can_rebuild || s.busy).0 {
                st.settings = false;
                out.push(Action::Rebuild);
            }
            y += 38.0;
            end(c, &mut y, false);
            h3(c, &mut y, w::INSTALL_LOCATION);
            desc(c, &mut y, w::ROOT_NOTE);
            y += 10.0;
            let mw = c.btn_width(w::MOVE, false, Btn::Plain);
            let ow = c.btn_width(w::OPEN, false, Btn::Ghost);
            let pw = wd - mw - ow - 20.0;
            let ph = path_box(c, [x, y, pw], &s.root, [10.0, 8.0]);
            let by = y + (ph - 38.0) * 0.5;
            if c.button(x + pw + 10.0, by, w::MOVE, false, Btn::Plain, s.busy).0 {
                st.settings = false;
                out.push(Action::MoveRoot);
            }
            if c.button(x + pw + 10.0 + mw + 10.0, by, w::OPEN, false, Btn::Ghost, false).0 {
                out.push(Action::OpenRoot);
            }
            y += ph.max(38.0);
            end(c, &mut y, false);
            h3(c, &mut y, w::BETA_FOLDER);
            desc(c, &mut y, w::BETA_FOLDER_DESC);
            y += 10.0;
            let cw2 = c.btn_width(w::CHANGE, false, Btn::Plain);
            let pw = wd - cw2 - 10.0;
            let ph = path_box(c, [x, y, pw], s.beta.as_deref().unwrap_or(w::NOT_CHOSEN), [10.0, 8.0]);
            if c.button(x + pw + 10.0, y + (ph - 38.0) * 0.5, w::CHANGE, false, Btn::Plain, s.busy).0 {
                st.settings = false;
                out.push(Action::ChangeBeta);
            }
        }
        ONLINE_TAB => online_pane(c, &s.online, st, [x, y, wd, r[1] + r[3] - 16.0 - y], out),
        // Deviation: no line with the version and the graphics card above these.
        _ => {
            h3(c, &mut y, w::PROJECT);
            desc(c, &mut y, w::PROJECT_DESC);
            end(c, &mut y, false);
            h3(c, &mut y, w::DISCLAIMER);
            desc(c, &mut y, w::DISCLAIMER_DESC);
        }
    }
    if c.pointer.click && !c.hover(r) {
        st.settings = false;
    }
}
