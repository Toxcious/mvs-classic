//! (the port's own) The Chaos Wheel's card under the scoreboard's clock: a small label, the mutator's name and a
//! bar that runs down over the window. While the next mutator is drawn the name line is a reel: the pool's names
//! scroll down through a window, the ones above and below showing at its edges, fast and then slowing, a click at
//! each name, and it lands on the drawn one with a punch and a burst of rays, gold and beating until the change.
//! Through the warning the bar is gold and a count of the seconds stands beside it, both pulsing at each second.
//! The new mutator comes on with a smaller punch as the gold leaves it. The card is the change's one sign: no text
//! over the screen repeats the name. The beta has no such rule, so the card is made here by hand, in the
//! scoreboard's own fonts and colours, and laid out on its own: nothing of the scoreboard moves for it.
//!
//! What the card shows is read off the rule's status each frame (`WheelHud`), the reel and the motion included: they
//! are functions of the ticks left, so a step simulated again shows the same and nothing is kept across frames but
//! what starts the sounds.

use crate::bundle::{Bundle, Rgba};
use crate::element::ElementList;
use crate::slate::{Geometry, HAlign, Tree, VAlign, Vis, Widget, WidgetId, layout, paint};
use crate::text::Fonts;

use super::WheelHud;
use super::custom_text::{WHEEL_FIRST, WHEEL_NEXT, WHEEL_RULE};

/// The card's width and the gap between it and the clock, in the clock's own units (the scoreboard's, inside its
/// ScaleBox).
const WIDTH: f64 = 660.0;
const GAP: f64 = 4.4;
/// The label's and the name's sizes, the bar's size and the gap over it.
const LABEL_SIZE: f64 = 15.4;
const NAME_SIZE: f64 = 26.4;
const BAR: [f64; 2] = [242.0, 6.6];
const BAR_GAP: f64 = 2.2;
/// The bar's ground under its fill.
const BAR_GROUND: Rgba = [0.0, 0.0, 0.0, 0.45];
/// The build's events the card posts (UI_Game's PF_Post_Global_Event): at the roll (the spin's start, the Iron
/// Giant's crank), at each name the reel passes (a menu hover's tick), at the landing (the crank's end and a blast
/// box's burst over it) and at the change (of the match's base scope). The others are no shared sounds of a match:
/// a match with the wheel loads them in a scope of its own (`SCOPE_SOUNDS`).
// Unverified: the beta has no such rule and so no sound for it; the user picked the crank's two and the burst by
// ear, the click is picked by its name and length.
pub const ROLL_SOUND: &str = "sfx_cha_c017_spin_crank";
pub const CLICK_SOUND: &str = "ui_menu_collection_character_hover";
pub const LAND_SOUNDS: [&str; 2] = ["sfx_cha_c017_spin_crank_end", "sfx_blastbox_risingstars_ally"];
pub const CHANGE_SOUND: &str = "ui_tutorial_task_complete";
/// The events a match with the wheel loads for the card (the change's is in the base scope).
pub const SCOPE_SOUNDS: [&str; 4] = [ROLL_SOUND, CLICK_SOUND, LAND_SOUNDS[0], LAND_SOUNDS[1]];
/// The fewest ticks between two clicks: at the reel's fastest a click every name would be a buzz.
const CLICK_GAP: u32 = 3;
/// The share of the warning the spin takes: it lands at half, so the name stands a moment before the change.
const SPIN_SHARE: u32 = 2;
/// The reel: the window's height (it clips the names while they scroll) and the distance between two names.
const WINDOW: f64 = NAME_SIZE * 1.6;
const ROW: f64 = NAME_SIZE * 1.15;
/// A name a row from the window's middle: its opacity and its scale lost.
const EDGE_FADE: f64 = 0.7;
const EDGE_SHRINK: f64 = 0.2;
/// The landing's punch (the name's scale over 1 at the land, back to 1 over `PUNCH_TICKS`) and the change's.
const LAND_PUNCH: f64 = 0.7;
const CHANGE_PUNCH: f64 = 0.4;
const PUNCH_TICKS: f64 = 15.0;
/// The landed name's beat until the change: its scale over 1 at the top and its period in ticks.
const BEAT: f64 = 0.06;
const BEAT_TICKS: f64 = 30.0;
/// The ticks the gold takes to leave the name after the change.
const GOLD_OUT_TICKS: f64 = 40.0;
/// The landing's flash: the ticks the name takes from white to gold.
const FLASH_TICKS: f64 = 8.0;
/// The landing's burst behind the name, a firework's ring of sparks flying out: the sparks (count, length and
/// width), the ticks it lasts, how far from the middle a spark starts and ends (every other one a share nearer), the
/// share of its length it keeps at the end and how fast the ring turns, degrees a tick.
const RAYS: usize = 16;
const RAY: [f64; 2] = [40.0, 4.4];
const BURST_TICKS: f64 = 26.0;
const BURST_FROM: f64 = 14.0;
const BURST_TO: f64 = 170.0;
const BURST_INNER: f64 = 0.7;
const BURST_END_LENGTH: f64 = 0.3;
const BURST_TURN: f64 = 0.8;
/// The ring stretched sideways, as wide as a name is long.
const BURST_WIDE: f64 = 1.8;
/// The count's seconds, in ticks; its pulse at each second (the ticks it takes to settle), the bar's height over 1
/// and the count's scale over 1 at a pulse's top; the room the count takes beside the bar.
const SECOND: u32 = 60;
const PULSE_TICKS: f64 = 14.0;
const PULSE_BAR: f64 = 1.6;
const PULSE_COUNT: f64 = 0.5;
const COUNT_W: f64 = 30.0;
const COUNT_SIZE: f64 = 20.0;
/// The drawn mutator's colour, on the name, the bar, the count and the burst.
const GOLD: Rgba = [1.0, 0.78, 0.18, 1.0];
const CLEAR: Rgba = [0.0; 4];

/// What the card reads in a frame: the label, the name, the reel's neighbours, the bar's fill, the count and the
/// motion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardLine {
    pub label: Label,
    pub name: Shown,
    /// The reel while it spins, None once it has landed and outside the warning.
    pub reel: Option<ReelLine>,
    /// The window left, 1 at its start, 0 at the change.
    pub fill: f64,
    /// The whole seconds to the change through the warning.
    pub count: Option<u32>,
    pub motion: Motion,
}

/// The spinning reel's names around the one in the middle (`CardLine::name`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReelLine {
    /// The name coming down from above and the one going out below, by `LOBBY_MUTATORS` index.
    pub above: usize,
    pub below: usize,
    /// How far the middle name has moved down from its place, in rows (0..1): the one above as far from its own.
    pub frac: f64,
}

/// The card's motion in a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    /// The name's scale over 1.
    pub punch: f64,
    /// The share of gold in the name's colour.
    pub gold: f64,
    /// The share of white over the gold (the landing's flash).
    pub flash: f64,
    /// The bar in gold (the warning).
    pub gold_bar: bool,
    /// The second's pulse on the bar and the count: 1 at a whole second, settling to 0.
    pub pulse: f64,
    /// The landing's burst: the ticks since the landing while it lasts.
    pub burst: Option<f64>,
}

/// The card's label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Label {
    /// The plain first window.
    First,
    /// A mutator is on.
    Rule,
    /// The next one is drawn.
    Next,
}

/// The card's name line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// The rule's own name (the plain first window).
    Rule,
    /// A mutator, by its index in `LOBBY_MUTATORS`.
    Mutator(usize),
}

/// A share falling from 1 at 0 ticks to 0 at `ticks`, cubic (fast, then settling).
fn fade(t: f64, ticks: f64) -> f64 {
    let x = (1.0 - t / ticks).clamp(0.0, 1.0);
    x * x * x
}

/// The card of a status, None when it shows nothing (before the match starts and once it is over). `pool`: the
/// wheel's pool by `LOBBY_MUTATORS` index, the names the spin runs through.
pub fn card_line(h: &WheelHud, pool: &[usize]) -> Option<CardLine> {
    let left = h.ticks_left?;
    let fill = if h.window_ticks == 0 { 0.0 } else { (f64::from(left) / f64::from(h.window_ticks)).clamp(0.0, 1.0) };
    if let Some(next) = h.next {
        let r = reel(next, left, h.warn_ticks, pool);
        let count = Some(left.div_ceil(SECOND)).filter(|&c| c > 0);
        let pulse = fade(f64::from((SECOND - left % SECOND) % SECOND), PULSE_TICKS);
        let (motion, reel) = match r.landed {
            Some(t) => {
                let t = f64::from(t);
                let beat = BEAT * (0.5 - 0.5 * (std::f64::consts::TAU * t / BEAT_TICKS).cos());
                let motion = Motion {
                    punch: LAND_PUNCH * fade(t, PUNCH_TICKS) + beat,
                    gold: 1.0,
                    flash: fade(t, FLASH_TICKS),
                    gold_bar: true,
                    pulse,
                    burst: (t < BURST_TICKS).then_some(t),
                };
                (motion, None)
            }
            None => {
                let motion = Motion { gold_bar: true, pulse, ..Motion::default() };
                (motion, Some(ReelLine { above: r.above, below: r.below, frac: r.frac }))
            }
        };
        return Some(CardLine { label: Label::Next, name: Shown::Mutator(r.name), reel, fill, count, motion });
    }
    let (label, name, motion) = match h.current {
        Some(c) => {
            let on = f64::from(h.window_ticks.saturating_sub(left));
            let motion = Motion {
                punch: CHANGE_PUNCH * fade(on, PUNCH_TICKS),
                gold: fade(on, GOLD_OUT_TICKS),
                ..Motion::default()
            };
            (Label::Rule, Shown::Mutator(c), motion)
        }
        None => (Label::First, Shown::Rule, Motion::default()),
    };
    Some(CardLine { label, name, reel: None, fill, count: None, motion })
}

/// The reel at a tick of the warning: the name in the middle and its neighbours, the step it is at (names passed
/// since the spin started) and how far it has moved on to the next, and once landed the ticks since the landing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reel {
    pub name: usize,
    pub above: usize,
    pub below: usize,
    pub step: u32,
    pub frac: f64,
    pub landed: Option<u32>,
}

/// The name the spin shows `left` ticks before the change (the reel's).
#[cfg(test)]
fn spin(next: usize, left: u32, warn: u32, pool: &[usize]) -> usize {
    reel(next, left, warn, pool).name
}

/// The reel `left` ticks before the change, the next mutator drawn `warn` ticks ahead of it: the pool's names, fast
/// and slowing (a quadratic ease out over a count of steps), landed on `next` at `warn / SPIN_SHARE` ticks in. A
/// name outside the pool, a pool of one, or a warning too short for a step shows landed at once.
pub fn reel(next: usize, left: u32, warn: u32, pool: &[usize]) -> Reel {
    let gone = warn.saturating_sub(left);
    let landed = |t: u32, step: u32| Reel { name: next, above: next, below: next, step, frac: 0.0, landed: Some(t) };
    let Some(at) = pool.iter().position(|&m| m == next) else { return landed(gone, 0) };
    let n = pool.len();
    let land = (warn / SPIN_SHARE).max(1);
    // (two turns of the pool, fewer in a short warning: about one name every one and a half ticks at the start)
    let steps = (2 * n).min(usize::try_from(land * 2 / 3).unwrap_or(usize::MAX));
    let steps = u32::try_from(steps).unwrap_or(u32::MAX);
    if n < 2 || steps == 0 {
        return landed(gone, 0);
    }
    if gone >= land {
        return landed(gone - land, steps);
    }
    let p = f64::from(gone) / f64::from(land);
    let x = (1.0 - (1.0 - p) * (1.0 - p)) * f64::from(steps);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a floored count in 0..steps")]
    let step = (x.floor().clamp(0.0, f64::from(steps))) as u32;
    // (step k shows the name `steps - k` before the drawn one: the last step shows it)
    let name_at = |k: i64| {
        let back = (i64::from(steps) - k).rem_euclid(i64::try_from(n).expect("a short pool"));
        pool[(at + n - usize::try_from(back).expect("a remainder")) % n]
    };
    let k = i64::from(step);
    Reel { name: name_at(k), above: name_at(k + 1), below: name_at(k - 1), step, frac: x - x.floor(), landed: None }
}

/// The card.
#[derive(Debug)]
pub struct WheelCard {
    pub t: Tree,
    label: WidgetId,
    /// The reel's window and its three names: the middle one (the card's name) and the ones above and below.
    window: WidgetId,
    name: WidgetId,
    above: WidgetId,
    below: WidgetId,
    rays: Vec<WidgetId>,
    bar: WidgetId,
    fill: WidgetId,
    count: WidgetId,
    /// The name's and the bar's own colour (the scoreboard's), under the gold.
    color: Rgba,
    /// The pool by `LOBBY_MUTATORS` index.
    pool: Vec<usize>,
    /// The upper-case names by `LOBBY_MUTATORS` index.
    names: Vec<String>,
    /// The status of the last observe, of what starts the sounds.
    seen: Option<Track>,
}

/// What the card keeps of the last observe: its mutator on, its next, whether the next had landed, the furthest
/// step of its reel seen and the ticks left at its last click.
#[derive(Clone, Copy, Debug)]
struct Track {
    current: Option<usize>,
    next: Option<usize>,
    landed: bool,
    step: u32,
    click_left: u32,
}

/// What an observe saw change since the last one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// The next mutator was drawn: the spin started.
    pub rolled: bool,
    /// The reel moved on to a name it had not shown yet.
    pub click: bool,
    /// The spin landed on the next mutator.
    pub landed: bool,
    /// The mutator that came on.
    pub changed: Option<usize>,
}

/// A text block of the card in the style of `like` (the scoreboard's own), at `size`.
fn text_like(t: &mut Tree, b: &Bundle, name: &str, like: &Widget, size: f64) -> WidgetId {
    let w = t.create_plain(b, "TextBlock", name);
    let x = t.widget_mut(w);
    x.typeface.clone_from(&like.typeface);
    x.font_mat.clone_from(&like.font_mat);
    x.color = like.color;
    x.outline_color = like.outline_color;
    x.outline = like.outline;
    x.shadow_color = like.shadow_color;
    x.shadow_offset = like.shadow_offset;
    x.letter_spacing = like.letter_spacing;
    x.font_size = size;
    x.h_align = HAlign::Center;
    x.v_align = VAlign::Top;
    x.vis = Vis::HitTestInvisible;
    w
}

/// A plain box of `size`, tinted.
fn bar_box(t: &mut Tree, b: &Bundle, name: &str, size: [f64; 2], tint: Rgba) -> WidgetId {
    let w = t.create_plain(b, "Image", name);
    let x = t.widget_mut(w);
    x.tex = None;
    x.brush_w = size[0];
    x.brush_h = size[1];
    x.brush_tint = [1.0; 4];
    x.color = tint;
    x.h_align = HAlign::Left;
    x.v_align = VAlign::Top;
    x.vis = Vis::HitTestInvisible;
    w
}

/// A plain panel of `kind`, centred in its slot.
fn panel(t: &mut Tree, b: &Bundle, kind: &str, name: &str) -> WidgetId {
    let w = t.create_plain(b, kind, name);
    let x = t.widget_mut(w);
    x.h_align = HAlign::Center;
    x.v_align = VAlign::Center;
    x.vis = Vis::HitTestInvisible;
    w
}

/// `c` blended toward `to` by `share`.
fn blend(c: Rgba, to: Rgba, share: f64) -> Rgba {
    std::array::from_fn(|i| c[i] + (to[i] - c[i]) * share)
}

impl WheelCard {
    /// The card of a match whose wheel's pool is `pool`; `label_like` and `name_like` the scoreboard's text blocks
    /// whose fonts and colours the label and the name take; `names` the menus' mutator names.
    pub fn new(b: &Bundle, pool: Vec<usize>, label_like: &Widget, name_like: &Widget, names: &[&str]) -> WheelCard {
        let mut t = Tree::empty(b);
        let root = t.create_plain(b, "VerticalBox", "WheelCard");
        let label = text_like(&mut t, b, "WheelLabel", label_like, LABEL_SIZE);
        // the reel's window: its height held by a clear box, the burst's rays behind the names
        let window = panel(&mut t, b, "Overlay", "WheelReel");
        let tall = bar_box(&mut t, b, "WheelReelHeight", [1.0, WINDOW], CLEAR);
        t.widget_mut(tall).h_align = HAlign::Center;
        t.add(window, tall);
        let mut rays = Vec::with_capacity(RAYS);
        for i in 0..RAYS {
            let r = bar_box(&mut t, b, &format!("WheelRay{i}"), RAY, GOLD);
            let x = t.widget_mut(r);
            x.h_align = HAlign::Center;
            x.v_align = VAlign::Center;
            // (turned about its inner end, placed out from the window's middle at each frame)
            x.transform.pivot = [0.0, 0.5];
            x.vis = Vis::Collapsed;
            t.add(window, r);
            rays.push(r);
        }
        let reel_text = |t: &mut Tree, name: &str| {
            let w = text_like(t, b, name, name_like, NAME_SIZE);
            t.widget_mut(w).v_align = VAlign::Center;
            t.add(window, w);
            w
        };
        let below = reel_text(&mut t, "WheelBelow");
        let above = reel_text(&mut t, "WheelAbove");
        let name = reel_text(&mut t, "WheelName");
        // the bar between the count's room on both sides, so it stays centred
        let row = panel(&mut t, b, "HorizontalBox", "WheelBarRow");
        t.widget_mut(row).pad.t = BAR_GAP;
        let room = bar_box(&mut t, b, "WheelCountRoom", [COUNT_W, 1.0], CLEAR);
        t.widget_mut(room).v_align = VAlign::Center;
        let bar = panel(&mut t, b, "Overlay", "WheelBar");
        let ground = bar_box(&mut t, b, "WheelBarGround", BAR, BAR_GROUND);
        let fill = bar_box(&mut t, b, "WheelBarFill", BAR, name_like.color);
        t.widget_mut(fill).transform.pivot = [0.0, 0.5];
        t.add(bar, ground);
        t.add(bar, fill);
        let count_box = panel(&mut t, b, "Overlay", "WheelCountBox");
        let count_w = bar_box(&mut t, b, "WheelCountWidth", [COUNT_W, 1.0], CLEAR);
        t.widget_mut(count_w).v_align = VAlign::Center;
        let count = text_like(&mut t, b, "WheelCount", label_like, COUNT_SIZE);
        {
            let x = t.widget_mut(count);
            x.v_align = VAlign::Center;
            x.color = GOLD;
        }
        t.add(count_box, count_w);
        t.add(count_box, count);
        for w in [room, bar, count_box] {
            t.add(row, w);
        }
        for w in [label, window, row] {
            t.add(root, w);
        }
        t.widget_mut(root).vis = Vis::Collapsed;
        let names = names.iter().map(|s| s.to_uppercase()).collect();
        WheelCard {
            t,
            label,
            window,
            name,
            above,
            below,
            rays,
            bar,
            fill,
            count,
            color: name_like.color,
            pool,
            names,
            seen: None,
        }
    }

    fn name_of(&self, s: Shown) -> &str {
        match s {
            Shown::Rule => WHEEL_RULE,
            Shown::Mutator(i) => self.names.get(i).map_or("", String::as_str),
        }
    }

    /// After each observe: what changed since the last one. A status that shows no card (before the start, the
    /// match over) changes nothing and is not kept: online the status is the predicted match's, and a match end a
    /// rollback takes back must not play the change again. A next that landed stays landed until it is gone, and
    /// the reel clicks only past the furthest name it showed, so a rollback into the spin neither lands it nor
    /// clicks it twice.
    pub fn observe(&mut self, h: Option<&WheelHud>) -> Seen {
        let Some((h, left)) = h.and_then(|h| Some((h, h.ticks_left?))) else { return Seen::default() };
        let (current, next) = (h.current, h.next);
        let was = self.seen.unwrap_or(Track { current: None, next: None, landed: false, step: 0, click_left: 0 });
        let same = next == was.next;
        let (was_landed, step, click_left) =
            if same { (was.landed, was.step, was.click_left) } else { (false, 0, u32::MAX) };
        let r = next.map(|n| reel(n, left, h.warn_ticks, &self.pool));
        let landed = r.is_some_and(|r| r.landed.is_some());
        let click = r.is_some_and(|r| {
            r.landed.is_none() && r.step > step && click_left.saturating_sub(left) >= CLICK_GAP && !was_landed
        });
        self.seen = Some(Track {
            current,
            next,
            landed: was_landed || landed,
            step: step.max(r.map_or(0, |r| r.step)),
            click_left: if click { left } else { click_left },
        });
        Seen {
            rolled: next.is_some() && was.next.is_none(),
            click,
            landed: landed && !was_landed,
            changed: current.filter(|_| current != was.current),
        }
    }

    /// The frame's card under the clock (`clock`: its rectangle in device pixels, the scoreboard's opacity and the
    /// clock's painted scale, device pixels a Slate unit, which the card takes so it grows and shrinks with the
    /// scoreboard's ScaleBox; None: the scoreboard is not shown), above it with the HUD at the bottom. -> whether
    /// its root was painted.
    pub fn draw(
        &mut self,
        h: Option<&WheelHud>,
        clock: Option<([f64; 4], f64, f64)>,
        bottom: bool,
        fonts: &mut Fonts,
        out: &mut ElementList,
    ) -> bool {
        let root = self.t.root();
        let (Some(line), Some((r, opacity, dpi))) = (h.and_then(|h| card_line(h, &self.pool)), clock) else {
            self.t.widget_mut(root).vis = Vis::Collapsed;
            return false;
        };
        let label = match line.label {
            Label::First => WHEEL_FIRST,
            Label::Rule => WHEEL_RULE,
            Label::Next => WHEEL_NEXT,
        };
        let name = self.name_of(line.name).to_owned();
        let (above, below) = line.reel.map_or((String::new(), String::new()), |l| {
            (self.name_of(Shown::Mutator(l.above)).to_owned(), self.name_of(Shown::Mutator(l.below)).to_owned())
        });
        let count = line.count.map(|c| c.to_string()).unwrap_or_default();
        for (w, s) in [
            (self.label, label),
            (self.name, name.as_str()),
            (self.above, above.as_str()),
            (self.below, below.as_str()),
            (self.count, count.as_str()),
        ] {
            let x = self.t.widget_mut(w);
            if x.text != s {
                s.clone_into(&mut x.text);
            }
        }
        let m = line.motion;
        let color = self.color;
        // the bar: gold through the warning, swelling and whitening at each second's pulse
        let fill = self.t.widget_mut(self.fill);
        fill.transform.scale = [line.fill, 1.0];
        fill.color = if m.gold_bar { blend(GOLD, [1.0; 4], 0.6 * m.pulse) } else { color };
        self.t.widget_mut(self.bar).transform.scale = [1.0, 1.0 + PULSE_BAR * m.pulse];
        self.t.widget_mut(self.count).transform.scale = [1.0 + PULSE_COUNT * m.pulse; 2];
        // the reel: the window clips the names while they scroll, and no longer once landed (the punch outgrows it)
        self.t.widget_mut(self.window).clip = line.reel.is_some();
        let frac = line.reel.map_or(0.0, |l| l.frac);
        let shown = |y: f64| {
            let d = (y.abs() / ROW).min(1.0);
            (1.0 - EDGE_FADE * d, 1.0 - EDGE_SHRINK * d)
        };
        let x = self.t.widget_mut(self.name);
        let (fade_by, shrink) = shown(frac * ROW);
        x.transform.scale = [(1.0 + m.punch) * shrink; 2];
        x.transform.translation = [0.0, frac * ROW];
        x.color = blend(blend(color, GOLD, m.gold), [1.0; 4], m.flash);
        x.opacity = fade_by;
        for (w, y) in [(self.above, (frac - 1.0) * ROW), (self.below, (frac + 1.0) * ROW)] {
            let (fade_by, shrink) = shown(y);
            let x = self.t.widget_mut(w);
            x.vis = if line.reel.is_some() { Vis::HitTestInvisible } else { Vis::Collapsed };
            x.transform.scale = [shrink; 2];
            x.transform.translation = [0.0, y];
            x.opacity = fade_by;
        }
        // the landing's burst: sparks flying out from behind the name, turning, shortening and fading
        for (i, &w) in self.rays.iter().enumerate() {
            let x = self.t.widget_mut(w);
            let Some(t) = m.burst else {
                x.vis = Vis::Collapsed;
                continue;
            };
            let q = (t / BURST_TICKS).clamp(0.0, 1.0);
            let out = 1.0 - (1.0 - q) * (1.0 - q);
            x.vis = Vis::HitTestInvisible;
            #[allow(clippy::cast_precision_loss, reason = "a spark's index, below RAYS")]
            let turn = i as f64 * 360.0 / RAYS as f64 + BURST_TURN * t;
            let reach = if i % 2 == 0 { 1.0 } else { BURST_INNER };
            let r = (BURST_FROM + (BURST_TO - BURST_FROM) * out) * reach;
            let (sin, cos) = turn.to_radians().sin_cos();
            // (the box stands centred on the middle: its inner end moved to the middle, then out along the angle)
            x.transform.angle = turn;
            x.transform.translation = [RAY[0] / 2.0 + BURST_WIDE * r * cos, r * sin];
            x.transform.scale = [1.0 - (1.0 - BURST_END_LENGTH) * q, 1.0];
            x.opacity = (1.0 - q) * (1.0 - q);
        }
        let x = self.t.widget_mut(root);
        x.vis = Vis::HitTestInvisible;
        x.opacity = opacity;
        // (measured first, so a card above the clock stands on it)
        crate::slate::measure_tree(&mut self.t, fonts, dpi);
        let height = self.t.desired_size(root)[1];
        let cx = (r[0] + r[2] / 2.0) / dpi;
        let y = if bottom { r[1] / dpi - GAP - height } else { (r[1] + r[3]) / dpi + GAP };
        layout(&mut self.t, fonts, Geometry { pos: [cx - WIDTH / 2.0, y], size: [WIDTH, height] }, dpi);
        paint(&self.t, fonts, out, 1.0);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POOL: [usize; 13] = [4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

    fn hud(current: Option<usize>, next: Option<usize>, left: Option<u32>) -> WheelHud {
        WheelHud { current, next, ticks_left: left, window_ticks: 1200, warn_ticks: 180 }
    }

    #[test]
    fn the_card_of_each_part_of_the_match() {
        // before the start and once the match is over: nothing
        assert_eq!(card_line(&hud(None, None, None), &POOL), None);
        assert_eq!(card_line(&hud(Some(6), None, None), &POOL), None);
        // the plain first window: the rule's own name, the bar full at the start, no motion, reel or count
        let l = card_line(&hud(None, None, Some(1200)), &POOL).unwrap();
        assert_eq!((l.label, l.name, l.fill, l.motion), (Label::First, Shown::Rule, 1.0, Motion::default()));
        assert_eq!((l.reel, l.count), (None, None));
        // a window: the mutator on, the bar at the share left, still
        let l = card_line(&hud(Some(6), None, Some(300)), &POOL).unwrap();
        assert_eq!((l.label, l.name, l.fill, l.motion), (Label::Rule, Shown::Mutator(6), 0.25, Motion::default()));
        // the warning: NEXT, and the spin's name, the bar gold, the seconds counted
        let l = card_line(&hud(Some(6), Some(9), Some(10)), &POOL).unwrap();
        assert_eq!((l.label, l.name, l.motion.gold_bar, l.count), (Label::Next, Shown::Mutator(9), true, Some(1)));
        let l = card_line(&hud(None, Some(9), Some(0)), &POOL).unwrap();
        assert_eq!((l.fill, l.count), (0.0, None));
    }

    #[test]
    fn the_reel_spins_lands_with_a_punch_and_a_burst_and_comes_on_gold() {
        let at = |left| card_line(&hud(None, Some(9), Some(left)), &POOL).unwrap();
        // the spin: a reel, no gold on the name, the count at 3 pulsing at its start
        let first = at(180);
        assert!(first.reel.is_some() && first.motion.gold == 0.0 && first.motion.punch == 0.0, "{first:?}");
        assert_eq!((first.count, first.motion.pulse), (Some(3), 1.0));
        // a pulse at each whole second, settled between
        assert_eq!(at(150).motion.pulse, 0.0);
        assert_eq!((at(120).count, at(120).motion.pulse), (Some(2), 1.0));
        assert_eq!((at(60).count, at(60).motion.pulse), (Some(1), 1.0));
        // the landing (90 ticks in): no reel, the full punch, gold, the burst; settled to the beat a punch later
        let land = at(90).motion;
        assert!(at(90).reel.is_none());
        assert!((land.punch - LAND_PUNCH).abs() < 1e-9 && land.gold == 1.0 && land.burst == Some(0.0), "{land:?}");
        assert!(land.flash == 1.0 && at(80).motion.flash == 0.0, "the flash goes white to gold");
        assert!(at(60).motion.punch <= BEAT + 1e-9 && at(60).motion.burst.is_none());
        // the change: a smaller punch, the gold leaving, the bar back to the name's colour, no count
        let on = |left| card_line(&hud(Some(9), None, Some(left)), &POOL).unwrap();
        let c = on(1200);
        assert!((c.motion.punch - CHANGE_PUNCH).abs() < 1e-9 && c.motion.gold == 1.0 && !c.motion.gold_bar, "{c:?}");
        assert!(c.count.is_none() && c.reel.is_none());
        assert!(on(1150).motion == Motion::default());
    }

    #[test]
    fn the_spin_runs_fast_then_slows_and_lands_by_half_the_warning() {
        let names: Vec<usize> = (0..=180).rev().map(|left| spin(9, left, 180, &POOL)).collect();
        // landed by 90 ticks in, the last steps' slowing landing it a little ahead of that
        let landed = (0..names.len()).find(|&i| names[i..].iter().all(|&m| m == 9)).unwrap();
        assert!((70..=90).contains(&landed), "{landed}");
        // every name shown is the pool's
        assert!(names.iter().all(|m| POOL.contains(m)));
        // more changes of name in the first quarter than in the second
        let changes = |s: &[usize]| s.windows(2).filter(|p| p[0] != p[1]).count();
        assert!(changes(&names[..45]) > changes(&names[45..90]), "{names:?}");
        assert!(changes(&names[..90]) >= POOL.len());
        // the reel: the one above comes next and the one below went before, the step never goes back, the share
        // moved on stays in 0..1
        for left in (91..180).rev() {
            let (a, b) = (reel(9, left + 1, 180, &POOL), reel(9, left, 180, &POOL));
            assert!(b.step >= a.step && (0.0..1.0).contains(&b.frac), "{left}");
            if b.step == a.step + 1 {
                assert_eq!((b.name, b.below), (a.above, a.name), "{left}");
            }
        }
        // a short warning lands too, and a name outside the pool or a pool of one shows at once
        assert_eq!(spin(9, 30, 60, &POOL), 9);
        assert_eq!(spin(3, 180, 180, &POOL), 3);
        assert_eq!(spin(9, 180, 180, &[9]), 9);
        assert_eq!(spin(9, 1, 1, &POOL), 9);
        assert_eq!(spin(9, 2, 2, &POOL), 9);
    }

    #[test]
    fn the_roll_the_clicks_the_landing_and_the_change_are_seen_once() {
        let b = Bundle::default();
        let w = Widget::default();
        let mut c = WheelCard::new(&b, POOL.to_vec(), &w, &w, &["a"; 18]);
        let none = Seen::default();
        let rolled = Seen { rolled: true, ..none };
        let click = Seen { click: true, ..none };
        let landed = Seen { landed: true, ..none };
        let changed = |m| Seen { changed: Some(m), ..none };
        assert_eq!(c.observe(Some(&hud(None, None, Some(500)))), none);
        assert_eq!(c.observe(Some(&hud(None, Some(9), Some(180)))), rolled);
        assert_eq!(c.observe(Some(&hud(None, Some(9), Some(179)))), none);
        // the clicks: one a name, no closer than CLICK_GAP ticks
        let clicks: Vec<u32> =
            (91..179).rev().filter(|&l| c.observe(Some(&hud(None, Some(9), Some(l)))).click).collect();
        assert!(clicks.len() >= POOL.len(), "{clicks:?}");
        assert!(clicks.windows(2).all(|p| p[0] - p[1] >= CLICK_GAP), "{clicks:?}");
        // the landing at half the warning, once, and not again, nor a click, when a rollback goes back into the spin
        assert_eq!(c.observe(Some(&hud(None, Some(9), Some(90)))), landed);
        assert_eq!(c.observe(Some(&hud(None, Some(9), Some(89)))), none);
        assert_eq!(c.observe(Some(&hud(None, Some(9), Some(95)))), none);
        assert_eq!(c.observe(Some(&hud(None, Some(9), Some(88)))), none);
        assert_eq!(c.observe(Some(&hud(Some(9), None, Some(1200)))), changed(9));
        assert_eq!(c.observe(Some(&hud(Some(9), None, Some(1199)))), none);
        // a predicted end a rollback takes back (no card, or no status at all, between two frames of one window):
        // neither the change nor, inside the warning, the roll or the landing plays again
        assert_eq!(c.observe(Some(&hud(Some(9), None, None))), none);
        assert_eq!(c.observe(None), none);
        assert_eq!(c.observe(Some(&hud(Some(9), None, Some(1198)))), none);
        assert_eq!(c.observe(Some(&hud(Some(9), Some(4), Some(180)))), rolled);
        assert_eq!(c.observe(Some(&hud(Some(9), Some(4), None))), none);
        assert_eq!(c.observe(Some(&hud(Some(9), Some(4), Some(150)))), click);
        assert_eq!(c.observe(Some(&hud(Some(9), Some(4), Some(80)))), landed);
        assert_eq!(c.observe(Some(&hud(Some(9), Some(4), None))), none);
        assert_eq!(c.observe(Some(&hud(Some(9), Some(4), Some(79)))), none);
        assert_eq!(c.observe(Some(&hud(Some(4), None, Some(1200)))), changed(4));
    }
}
