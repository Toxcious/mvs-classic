//! (the port's own) The Chaos Wheel's card under the scoreboard's clock: a small label, the mutator's name and a
//! bar that runs down over the window. While the next mutator is drawn the name spins through the pool as a reel
//! (each name drops in from above, fast, then slowing) and lands on it with a punch, gold and beating until the
//! change; the bar is gold through the warning, and the new mutator comes on with a smaller punch as the gold leaves
//! it. The card is the change's one sign: no text over the screen repeats the name. The beta has no such rule, so the
//! card is made here by hand, in the scoreboard's own fonts and colours, and laid out on its own: nothing of the
//! scoreboard moves for it.
//!
//! What the card shows is read off the rule's status each frame (`WheelHud`), the spin and the motion included: they
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
/// The build's events the card posts (UI_Game's PF_Post_Global_Event): at the roll (the spin's start) and at the
/// change, both of the match's base scope (the banks a match keeps loaded).
// Unverified: the beta has no such rule and so no sound for it; the two are picked by their names, not by ear.
pub const ROLL_SOUND: &str = "ui_countdown_card_intro";
pub const CHANGE_SOUND: &str = "ui_tutorial_task_complete";
/// The share of the warning the spin takes: it lands at half, so the name stands a moment before the change.
const SPIN_SHARE: u32 = 2;
/// The reel: the ticks a name takes to drop into place, and how far above its place it starts, in name sizes.
const DROP_TICKS: f64 = 3.0;
const DROP_HEIGHT: f64 = 0.3;
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
/// The drawn mutator's colour, on the name and the bar.
const GOLD: Rgba = [1.0, 0.78, 0.18, 1.0];

/// What the card reads in a frame: the label, the name, the bar's fill and the motion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardLine {
    pub label: Label,
    pub name: Shown,
    /// The window left, 1 at its start, 0 at the change.
    pub fill: f64,
    pub motion: Motion,
}

/// The card's motion in a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    /// The share of the reel's drop still to go: 1 a name just come up (above its place), 0 in place.
    pub drop: f64,
    /// The name's scale over 1.
    pub punch: f64,
    /// The share of gold in the name's colour.
    pub gold: f64,
    /// The share of white over the gold (the landing's flash).
    pub flash: f64,
    /// The bar in gold (the warning).
    pub gold_bar: bool,
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
        let motion = match r.landed {
            Some(t) => {
                let t = f64::from(t);
                let beat = BEAT * (0.5 - 0.5 * (std::f64::consts::TAU * t / BEAT_TICKS).cos());
                Motion {
                    drop: 0.0,
                    punch: LAND_PUNCH * fade(t, PUNCH_TICKS) + beat,
                    gold: 1.0,
                    flash: fade(t, FLASH_TICKS),
                    gold_bar: true,
                }
            }
            None => Motion { drop: fade(f64::from(r.since), DROP_TICKS), gold_bar: true, ..Motion::default() },
        };
        return Some(CardLine { label: Label::Next, name: Shown::Mutator(r.name), fill, motion });
    }
    Some(match h.current {
        Some(c) => {
            let on = f64::from(h.window_ticks.saturating_sub(left));
            let motion = Motion {
                punch: CHANGE_PUNCH * fade(on, PUNCH_TICKS),
                gold: fade(on, GOLD_OUT_TICKS),
                ..Motion::default()
            };
            CardLine { label: Label::Rule, name: Shown::Mutator(c), fill, motion }
        }
        None => CardLine { label: Label::First, name: Shown::Rule, fill, motion: Motion::default() },
    })
}

/// The reel at a tick of the warning: the name shown, the ticks since it came up, and once landed the ticks since
/// the landing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reel {
    pub name: usize,
    pub since: u32,
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
    let landed = |t: u32| Reel { name: next, since: t, landed: Some(t) };
    let Some(at) = pool.iter().position(|&m| m == next) else { return landed(gone) };
    let n = pool.len();
    let land = (warn / SPIN_SHARE).max(1);
    // (two turns of the pool, fewer in a short warning: about one name every one and a half ticks at the start)
    let steps = (2 * n).min(usize::try_from(land * 2 / 3).unwrap_or(usize::MAX));
    if n < 2 || steps == 0 {
        return landed(gone);
    }
    if gone >= land {
        return landed(gone - land);
    }
    let step = |g: u32| {
        let p = f64::from(g) / f64::from(land);
        let eased = 1.0 - (1.0 - p) * (1.0 - p);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a floored count in 0..steps")]
        let k = (eased * steps as f64).floor().clamp(0.0, steps as f64) as usize;
        k
    };
    let k = step(gone);
    // (the ticks back to the step's first: at most the warning's half)
    let mut since = 0;
    while since < gone && step(gone - since - 1) == k {
        since += 1;
    }
    let back = (steps - k) % n;
    Reel { name: pool[(at + n - back) % n], since, landed: None }
}

/// The card.
#[derive(Debug)]
pub struct WheelCard {
    pub t: Tree,
    label: WidgetId,
    name: WidgetId,
    fill: WidgetId,
    /// The name's and the bar's own colour (the scoreboard's), under the gold.
    color: Rgba,
    /// The pool by `LOBBY_MUTATORS` index.
    pool: Vec<usize>,
    /// The upper-case names by `LOBBY_MUTATORS` index.
    names: Vec<String>,
    /// The status of the last observe: its mutator on and its next.
    seen: Option<(Option<usize>, Option<usize>)>,
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

impl WheelCard {
    /// The card of a match whose wheel's pool is `pool`; `label_like` and `name_like` the scoreboard's text blocks
    /// whose fonts and colours the label and the name take; `names` the menus' mutator names.
    pub fn new(b: &Bundle, pool: Vec<usize>, label_like: &Widget, name_like: &Widget, names: &[&str]) -> WheelCard {
        let mut t = Tree::empty(b);
        let root = t.create_plain(b, "VerticalBox", "WheelCard");
        let label = text_like(&mut t, b, "WheelLabel", label_like, LABEL_SIZE);
        let name = text_like(&mut t, b, "WheelName", name_like, NAME_SIZE);
        let bar = t.create_plain(b, "Overlay", "WheelBar");
        {
            let x = t.widget_mut(bar);
            x.h_align = HAlign::Center;
            x.v_align = VAlign::Top;
            x.pad.t = BAR_GAP;
            x.vis = Vis::HitTestInvisible;
        }
        let ground = bar_box(&mut t, b, "WheelBarGround", BAR, BAR_GROUND);
        let fill = bar_box(&mut t, b, "WheelBarFill", BAR, name_like.color);
        t.widget_mut(fill).transform.pivot = [0.0, 0.5];
        t.add(bar, ground);
        t.add(bar, fill);
        for w in [label, name, bar] {
            t.add(root, w);
        }
        t.widget_mut(root).vis = Vis::Collapsed;
        let names = names.iter().map(|s| s.to_uppercase()).collect();
        WheelCard { t, label, name, fill, color: name_like.color, pool, names, seen: None }
    }

    fn name_of(&self, s: Shown) -> &str {
        match s {
            Shown::Rule => WHEEL_RULE,
            Shown::Mutator(i) => self.names.get(i).map_or("", String::as_str),
        }
    }

    /// After each observe: what changed since the last one, as (the spin started, the mutator that came on). A
    /// status that shows no card (before the start, the match over) changes nothing and is not kept: online the
    /// status is the predicted match's, and a match end a rollback takes back must not play the change again.
    pub fn observe(&mut self, h: Option<&WheelHud>) -> (bool, Option<usize>) {
        let Some(h) = h.filter(|h| h.ticks_left.is_some()) else { return (false, None) };
        let (current, next) = (h.current, h.next);
        let (was_current, was_next) = self.seen.replace((current, next)).unwrap_or((None, None));
        let rolled = next.is_some() && was_next.is_none();
        let changed = current.filter(|_| current != was_current);
        (rolled, changed)
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
        for (w, s) in [(self.label, label), (self.name, name.as_str())] {
            let x = self.t.widget_mut(w);
            if x.text != s {
                s.clone_into(&mut x.text);
            }
        }
        let m = line.motion;
        let color = self.color;
        let fill = self.t.widget_mut(self.fill);
        fill.transform.scale = [line.fill, 1.0];
        fill.color = if m.gold_bar { GOLD } else { color };
        let x = self.t.widget_mut(self.name);
        x.transform.scale = [1.0 + m.punch; 2];
        x.transform.translation = [0.0, -m.drop * DROP_HEIGHT * NAME_SIZE];
        x.color = std::array::from_fn(|i| {
            let gold = color[i] + (GOLD[i] - color[i]) * m.gold;
            gold + (1.0 - gold) * m.flash
        });
        // (a name dropping in fades in with it)
        x.opacity = 1.0 - 0.8 * m.drop;
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
        // the plain first window: the rule's own name, the bar full at the start, no motion
        let l = card_line(&hud(None, None, Some(1200)), &POOL).unwrap();
        assert_eq!((l.label, l.name, l.fill, l.motion), (Label::First, Shown::Rule, 1.0, Motion::default()));
        // a window: the mutator on, the bar at the share left, still
        let l = card_line(&hud(Some(6), None, Some(300)), &POOL).unwrap();
        assert_eq!((l.label, l.name, l.fill, l.motion), (Label::Rule, Shown::Mutator(6), 0.25, Motion::default()));
        // the warning: NEXT, and the spin's name, the bar gold
        let l = card_line(&hud(Some(6), Some(9), Some(10)), &POOL).unwrap();
        assert_eq!((l.label, l.name, l.motion.gold_bar), (Label::Next, Shown::Mutator(9), true));
        assert_eq!(card_line(&hud(None, Some(9), Some(0)), &POOL).unwrap().fill, 0.0);
    }

    #[test]
    fn the_name_drops_in_lands_with_a_punch_and_comes_on_gold() {
        // a name just come up stands above its place, then drops into it
        let at = |left| card_line(&hud(None, Some(9), Some(left)), &POOL).unwrap().motion;
        let first = at(180);
        assert!(first.drop > 0.9 && first.gold == 0.0 && first.punch == 0.0, "{first:?}");
        // the landing (90 ticks in): the full punch, gold; settled to the beat a punch later
        let land = at(90);
        assert!((land.punch - LAND_PUNCH).abs() < 1e-9 && land.gold == 1.0 && land.drop == 0.0, "{land:?}");
        assert!(land.flash == 1.0 && at(80).flash == 0.0, "the flash goes white to gold");
        assert!(at(60).punch <= BEAT + 1e-9);
        // the change: a smaller punch, the gold leaving, the bar back to the name's colour
        let on = |left| card_line(&hud(Some(9), None, Some(left)), &POOL).unwrap().motion;
        let c = on(1200);
        assert!((c.punch - CHANGE_PUNCH).abs() < 1e-9 && c.gold == 1.0 && !c.gold_bar, "{c:?}");
        assert!(on(1150) == Motion::default());
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
        // the reel counts the ticks since each name came up, 0 at each change of name
        for left in (91..180).rev() {
            let (a, b) = (reel(9, left + 1, 180, &POOL), reel(9, left, 180, &POOL));
            assert_eq!(b.since, if a.name == b.name && a.landed.is_none() { a.since + 1 } else { 0 }, "{left}");
        }
        // a short warning lands too, and a name outside the pool or a pool of one shows at once
        assert_eq!(spin(9, 30, 60, &POOL), 9);
        assert_eq!(spin(3, 180, 180, &POOL), 3);
        assert_eq!(spin(9, 180, 180, &[9]), 9);
        assert_eq!(spin(9, 1, 1, &POOL), 9);
        assert_eq!(spin(9, 2, 2, &POOL), 9);
    }

    #[test]
    fn the_roll_and_the_change_are_seen_once() {
        let b = Bundle::default();
        let w = Widget::default();
        let mut c = WheelCard::new(&b, POOL.to_vec(), &w, &w, &["a"; 18]);
        assert_eq!(c.observe(Some(&hud(None, None, Some(500)))), (false, None));
        assert_eq!(c.observe(Some(&hud(None, Some(9), Some(180)))), (true, None));
        assert_eq!(c.observe(Some(&hud(None, Some(9), Some(179)))), (false, None));
        assert_eq!(c.observe(Some(&hud(Some(9), None, Some(1200)))), (false, Some(9)));
        assert_eq!(c.observe(Some(&hud(Some(9), None, Some(1199)))), (false, None));
        // a predicted end a rollback takes back (no card, or no status at all, between two frames of one window):
        // neither the change nor, inside the warning, the roll plays again
        assert_eq!(c.observe(Some(&hud(Some(9), None, None))), (false, None));
        assert_eq!(c.observe(None), (false, None));
        assert_eq!(c.observe(Some(&hud(Some(9), None, Some(1198)))), (false, None));
        assert_eq!(c.observe(Some(&hud(Some(9), Some(4), Some(100)))), (true, None));
        assert_eq!(c.observe(Some(&hud(Some(9), Some(4), None))), (false, None));
        assert_eq!(c.observe(Some(&hud(Some(9), Some(4), Some(99)))), (false, None));
        assert_eq!(c.observe(Some(&hud(Some(4), None, Some(1200)))), (false, Some(4)));
    }
}
