//! The menus' curves and timelines: FMovieSceneFloatChannel and FRichCurve evaluation, UTimelineComponent's play and
//! tick, and a level sequence's player.

use super::format::{Channel, CurveKey, Key, SeqChannel, SeqKey, Sequence, WeightedKey};

/// The cubic between two keys with unweighted tangents: Bezier control points a third of the interval along each.
/// `a` is the key before (time, value, leave tangent), `b` the one after (time, value, arrive tangent).
fn cubic([a_t, a_v, a_lt]: [f64; 3], [b_t, b_v, b_at]: [f64; 3], t: f64) -> f64 {
    let d = b_t - a_t;
    let x = (t - a_t) / d;
    let p1 = a_v + (a_lt * d) / 3.0;
    let p2 = b_v - (b_at * d) / 3.0;
    let y = 1.0 - x;
    y * y * y * a_v + 3.0 * y * y * x * p1 + 3.0 * y * x * x * p2 + x * x * x * b_v
}

/// The index of the key whose interval holds `t` (the first key at or before it, with a key after it).
fn interval(times: impl Fn(usize) -> f64, t: f64) -> usize {
    let mut i = 0;
    while times(i + 1) <= t {
        i += 1;
    }
    i
}

/// FMovieSceneFloatChannel with cubic keys and unweighted tangents, constant before the first and after the last key
/// `keys` is not empty.
pub fn eval_channel(keys: &[Key], t: f64) -> f64 {
    let (first, last) = (&keys[0], &keys[keys.len() - 1]);
    if t <= first.t {
        return first.v;
    }
    if t >= last.t {
        return last.v;
    }
    let i = interval(|i| keys[i].t, t);
    let (a, b) = (&keys[i], &keys[i + 1]);
    cubic([a.t, a.v, a.lt], [b.t, b.v, b.at], t)
}

/// A podium channel's value (the keys', else the default's; None with neither).
pub fn eval_podium_channel(c: &Channel, t: f64) -> Option<f64> {
    match c {
        Channel::Keys { keys } if !keys.is_empty() => Some(eval_channel(keys, t)),
        Channel::Keys { .. } => None,
        Channel::Value { value } => *value,
    }
}

/// FRichCurve with weighted tangents: the Bezier parameter solved from the time by 48
/// halvings. `keys` is not empty.
// Unverified: FRichCurve's weighted cubic evaluation is written from the 4.26 source (control points at the
// tangent's angle and weight, the Bezier parameter solved from the time).
pub fn eval_weighted(keys: &[WeightedKey], t: f64) -> f64 {
    let (first, last) = (&keys[0], &keys[keys.len() - 1]);
    if t <= first.t {
        return first.v;
    }
    if t >= last.t {
        return last.v;
    }
    let i = interval(|i| keys[i].t, t);
    let (a, b) = (&keys[i], &keys[i + 1]);
    let range = b.t - a.t;
    let (la, aa) = (a.lt.atan(), b.at.atan());
    let x1 = (la.cos() * a.lw) / range;
    let y1 = a.v + la.sin() * a.lw;
    let x2 = (range - aa.cos() * b.aw) / range;
    let y2 = b.v - aa.sin() * b.aw;
    // x(s) = 3 (1 - s)^2 s x1 + 3 (1 - s) s^2 x2 + s^3 = alpha; x is monotonic for control points inside the interval
    let alpha = (t - a.t) / range;
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..48 {
        let s = (lo + hi) / 2.0;
        let x = 3.0 * (1.0 - s) * (1.0 - s) * s * x1 + 3.0 * (1.0 - s) * s * s * x2 + s * s * s;
        if x < alpha {
            lo = s;
        } else {
            hi = s;
        }
    }
    let s = (lo + hi) / 2.0;
    let r = 1.0 - s;
    r * r * r * a.v + 3.0 * r * r * s * y1 + 3.0 * r * s * s * y2 + s * s * s * b.v
}

/// A key's interpolation to the next: Constant and Linear by name, everything else cubic.
fn interp(i: &str, a: [f64; 3], b: [f64; 3], t: f64) -> f64 {
    match i {
        "Constant" => a[1],
        "Linear" => a[1] + (b[1] - a[1]) * ((t - a[0]) / (b[0] - a[0])),
        _ => cubic(a, b, t),
    }
}

/// An FRichCurve of the pre-match level: 0 with no keys, weighted when its first key
/// is (`w` set), else per key Constant, Linear or cubic; constant outside the keys.
pub fn eval_curve(keys: &[CurveKey], t: f64) -> f64 {
    if keys.is_empty() {
        return 0.0;
    }
    if keys[0].w.as_deref().is_some_and(|w| !w.is_empty()) {
        let w: Vec<WeightedKey> = keys
            .iter()
            .map(|k| WeightedKey {
                t: k.t,
                v: k.v,
                at: k.at,
                aw: k.aw.unwrap_or(0.0),
                lt: k.lt,
                lw: k.lw.unwrap_or(0.0),
            })
            .collect();
        return eval_weighted(&w, t);
    }
    let (first, last) = (&keys[0], &keys[keys.len() - 1]);
    if t <= first.t {
        return first.v;
    }
    if t >= last.t {
        return last.v;
    }
    let i = interval(|i| keys[i].t, t);
    let (a, b) = (&keys[i], &keys[i + 1]);
    interp(&a.i, [a.t, a.v, a.lt], [b.t, b.v, b.at], t)
}

/// A level sequence's float keys (a sequence channel's keys are never weighted).
pub fn eval_seq_keys(keys: &[SeqKey], t: f64) -> f64 {
    if keys.is_empty() {
        return 0.0;
    }
    let (first, last) = (&keys[0], &keys[keys.len() - 1]);
    if t <= first.t {
        return first.v;
    }
    if t >= last.t {
        return last.v;
    }
    let i = interval(|i| keys[i].t, t);
    let (a, b) = (&keys[i], &keys[i + 1]);
    interp(&a.i, [a.t, a.v, a.lt], [b.t, b.v, b.at], t)
}

/// A sequence channel's value : its keys', else its default (None: no default).
pub fn eval_seq_channel(c: &SeqChannel, t: f64) -> Option<f64> {
    if c.keys.is_empty() { c.default } else { Some(eval_seq_keys(&c.keys, t)) }
}

/// UTimelineComponent as the menus play it : Play from the position, PlayFromStart
/// from 0; a playing timeline's tick moves the position by dt x rate, clamped to the length.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Timeline {
    pub length: f64,
    pub pos: f64,
    pub rate: f64,
    pub playing: bool,
}

/// What a timeline's tick did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimelineStep {
    /// Not playing: no update.
    Idle,
    /// The update at the position.
    Update(f64),
    /// The update at the length, then the finished event.
    Finished(f64),
}

impl Timeline {
    pub fn new(length: f64) -> Timeline {
        Timeline { length, pos: 0.0, rate: 1.0, playing: false }
    }

    /// Plays from the position.
    pub fn play(&mut self) {
        self.playing = true;
    }

    /// Plays from the start.
    pub fn play_from_start(&mut self) {
        self.pos = 0.0;
        self.playing = true;
    }

    /// The caller runs the update (and the finished event) the step names.
    pub fn tick(&mut self, dt: f64) -> TimelineStep {
        if !self.playing {
            return TimelineStep::Idle;
        }
        self.pos = (self.pos + dt * self.rate).min(self.length);
        if self.pos >= self.length {
            self.playing = false;
            TimelineStep::Finished(self.pos)
        } else {
            TimelineStep::Update(self.pos)
        }
    }
}

/// A playing level sequence of the pre-match level.
#[derive(Debug)]
pub struct SequencePlayer {
    pub name: String,
    /// Seconds since the start.
    pub t: f64,
    /// `t` at the last evaluation (the event and audio tracks fire the keys between).
    pub prev: f64,
}

/// One evaluation of a sequence: from `prev` (exclusive) to `t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeqSpan {
    pub prev: f64,
    pub t: f64,
    /// `t` is the sequence's end: it stops after this evaluation.
    pub end: bool,
}

impl SeqSpan {
    /// An event key or section start between the last evaluation and now (a key at the start fires on the first one).
    pub fn crossed(&self, at: f64) -> bool {
        at > self.prev && at <= self.t
    }
}

impl SequencePlayer {
    /// From the sequence's start, a step before it so that a key at the start fires.
    pub fn new(name: &str, s: &Sequence) -> SequencePlayer {
        SequencePlayer { name: name.to_string(), t: s.start, prev: s.start - 1.0 }
    }

    /// The time moved by `dt`, clamped to the end; the span to evaluate. The caller evaluates
    /// the tracks over it (the level's actors are its own) and lets the sequence go when it is `end`.
    pub fn advance(&mut self, s: &Sequence, dt: f64) -> SeqSpan {
        self.t = (self.t + dt).min(s.end);
        let span = SeqSpan { prev: self.prev, t: self.t, end: self.t >= s.end };
        self.prev = self.t;
        span
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(t: f64, v: f64, at: f64, lt: f64) -> Key {
        Key { t, v, at, lt }
    }

    #[test]
    fn channel_is_constant_outside_and_cubic_inside() {
        let keys = [key(0.0, 1.0, 0.0, 0.0), key(1.0, 3.0, 0.0, 0.0)];
        assert_eq!(eval_channel(&keys, -1.0), 1.0);
        assert_eq!(eval_channel(&keys, 2.0), 3.0);
        // flat tangents: the smoothstep, half way at the middle
        assert!((eval_channel(&keys, 0.5) - 2.0).abs() < 1e-12);
        assert!((eval_channel(&keys, 0.25) - (1.0 + 2.0 * 0.15625)).abs() < 1e-12);
    }

    #[test]
    fn channel_with_tangents_follows_the_line() {
        // tangents of the line between the keys give the line
        let keys = [key(0.0, 0.0, 2.0, 2.0), key(2.0, 4.0, 2.0, 2.0)];
        assert!((eval_channel(&keys, 0.5) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn weighted_with_line_tangents_follows_the_line() {
        let w = (2.0f64).sqrt() / 3.0;
        let keys = [
            WeightedKey { t: 0.0, v: 0.0, at: 1.0, aw: w, lt: 1.0, lw: w },
            WeightedKey { t: 1.0, v: 1.0, at: 1.0, aw: w, lt: 1.0, lw: w },
        ];
        for t in [0.1, 0.5, 0.9] {
            assert!((eval_weighted(&keys, t) - t).abs() < 1e-9, "{t}");
        }
    }

    fn ck(t: f64, v: f64, i: &str) -> CurveKey {
        CurveKey { t, v, i: i.into(), at: 0.0, lt: 0.0, w: None, aw: None, lw: None }
    }

    #[test]
    fn curve_interpolations() {
        assert_eq!(eval_curve(&[], 0.5), 0.0);
        assert_eq!(eval_curve(&[ck(0.0, 1.0, "Constant"), ck(1.0, 3.0, "Linear")], 0.5), 1.0);
        assert_eq!(eval_curve(&[ck(0.0, 1.0, "Linear"), ck(1.0, 3.0, "Linear")], 0.5), 2.0);
        assert!((eval_curve(&[ck(0.0, 1.0, "Cubic"), ck(1.0, 3.0, "Cubic")], 0.5) - 2.0).abs() < 1e-12);
        let sk = [
            SeqKey { t: 0.0, v: 0.0, i: "Linear".into(), at: 0.0, lt: 0.0 },
            SeqKey { t: 2.0, v: 1.0, i: "Linear".into(), at: 0.0, lt: 0.0 },
        ];
        assert_eq!(eval_seq_channel(&SeqChannel { keys: sk.to_vec(), default: None }, 1.0), Some(0.5));
        assert_eq!(eval_seq_channel(&SeqChannel { keys: vec![], default: Some(4.0) }, 1.0), Some(4.0));
    }

    #[test]
    fn timeline_clamps_and_finishes() {
        let mut tl = Timeline::new(1.0);
        assert_eq!(tl.tick(0.5), TimelineStep::Idle);
        tl.play_from_start();
        assert_eq!(tl.tick(0.5), TimelineStep::Update(0.5));
        assert_eq!(tl.tick(0.75), TimelineStep::Finished(1.0));
        assert!(!tl.playing);
    }

    #[test]
    fn sequence_player_fires_the_start_key_and_ends_clamped() {
        let seq = Sequence { start: 0.0, end: 1.0, display_rate: 60.0, director: None, tracks: vec![] };
        let mut p = SequencePlayer::new("s", &seq);
        let first = p.advance(&seq, 0.0);
        assert!(first.crossed(0.0) && !first.end);
        let second = p.advance(&seq, 0.5);
        assert!(!second.crossed(0.0) && second.crossed(0.5) && !second.crossed(0.6));
        let last = p.advance(&seq, 2.0);
        assert!(last.end && last.t == 1.0 && last.crossed(1.0));
    }
}
