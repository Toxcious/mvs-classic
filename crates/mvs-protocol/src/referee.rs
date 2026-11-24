//! A match's referee: the one place that owns the match's tick clock and rules every human
//! slot's input stream. It runs in the hosting player's net thread or on a relay; it reads a packet's type, slot and
//! ticks and never what an input means ([`crate::match_wire`] has the packets). Sans-IO: the clock is passed in (ms
//! of a clock that only runs forward) and what it sends goes to a [`RefereeOut`].
//!
//! Deviation: the port's own netcode, the rule of DEADLINE and FAR_AHEAD; the build ran dedicated servers.

use std::collections::{BTreeMap, VecDeque};

use crate::match_wire::{
    ALL_TICKS, CHECKSUM_BYTES, CLOCK_MS, DEADLINE, FAR_AHEAD, INPUT_BYTES, INPUT_DELAY, InputHeader, MAX_PACKET,
    MAX_RUN, MAX_SLOTS, MSG_CHECKSUM, MSG_INPUT, NO_END, NO_STAMP, RESUME_MS, RULED_HEADER, RUN_HEADER, RuledHeader,
    RunHeader, SILENT_MS, TICK_MS, WireInput, max_runs, packet_fits, packet_inputs,
};

/// Where a referee's packets go (`RefereeOptions.send` / `onSilent`).
pub trait RefereeOut {
    /// A packet to the peer of `slot`.
    fn send(&mut self, slot: usize, bytes: &[u8]);
    /// News of the peer of `slot` (see [`SlotNews`]), each once in its turn.
    fn news(&mut self, slot: usize, n: SlotNews);
}

/// What a referee tells of a slot's peer. Deviation: the port's own rule; the build has no way back into a match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotNews {
    /// It has sent nothing for SILENT_MS, or has taken nothing for that long (its acknowledgement of a stream in the
    /// match trails the ruled line by more than [`BEHIND_TICKS`]): its ticks go on being ruled as substitutions and
    /// the streams it lacks are kept for it; its owner tells the others.
    Away,
    /// An away peer sends again and has taken the streams up to BEHIND_TICKS of the ruled line.
    Back,
    /// It has sent nothing for RESUME_MS, or trails a stream by more than [`GONE_TICKS`]: its owner takes it out of
    /// the match. Nothing more is told of it.
    Gone,
}

/// The most ruled packets a burst sends to one peer catching up after it was away, in a match of up to four slots
/// (each holds a run of MAX_RUN inputs or more): at 8 a peer back from 19 s away (about 1140 ticks) has every stream
/// within a few round trips. A burst goes when the peer's acknowledgements moved since the last one, else once a
/// CLOCK_MS (a deaf peer is sent a burst a CLOCK_MS, not one a pump). Deviation: the port's own bound.
pub const CATCH_UP_PACKETS: usize = 8;

/// The most ruled packets a burst sends in a match of `slots` slots: [`CATCH_UP_PACKETS`] up to four slots (its run
/// budget, catch_up_packets x [`max_runs`], 64 as it always was), and in a bigger match a packet per run of a send:
/// the oldest run of each stream and as many of the gaps', as at four.
/// Deviation: the port's own bound.
pub const fn catch_up_packets(slots: usize) -> usize {
    if max_runs(slots) > CATCH_UP_PACKETS { max_runs(slots) } else { CATCH_UP_PACKETS }
}
/// A peer catching up is sent the ticks up to this many past what it has of a stream (a game keeps no more than 600
/// ticks that came ahead of its line, mvs-net session.rs EARLY_TICKS). Deviation: the port's own bound.
pub const CATCH_UP_AHEAD: u32 = 512;

/// A slot away for this many ticks is gone (`Math.ceil(RESUME_MS / TICK_MS)`). Deviation: the port's own rule.
pub const GONE_TICKS: u32 = 1200;
const _: () = assert!(GONE_TICKS as f64 >= RESUME_MS / TICK_MS && (GONE_TICKS - 1) as f64 <= RESUME_MS / TICK_MS);

/// A peer whose acknowledgement of a stream still in the match trails that stream's ruled line by more ticks than
/// this takes nothing the referee sends: it is away as one that sends nothing (`Math.ceil(SILENT_MS / TICK_MS)`).
/// Deviation: the port's own bound.
pub const BEHIND_TICKS: u32 = 300;
const _: () = assert!(BEHIND_TICKS as f64 >= SILENT_MS / TICK_MS && (BEHIND_TICKS - 1) as f64 <= SILENT_MS / TICK_MS);

/// A ruled tick not acknowledged by a peer is sent it again once this long (ms) has passed since it was last sent, while
/// no round trip of the peer's is known yet. Deviation: the port's own bound.
pub const RESEND_FIRST_MS: f64 = 200.0;
/// Added to a peer's resend timeout past its round trip and four times its spread: a peer acknowledges once a frame
/// (one tick), so its acknowledgement may wait that long past a packet's arrival. Deviation: the port's own bound.
pub const RESEND_MARGIN_MS: f64 = TICK_MS;

/// A ruled tick is sent a healthy peer again with the next ticks of its stream, while not acknowledged: a run starts this
/// many ticks before the first never sent, so one lost packet costs nothing (the peer's prediction limit is shorter than
/// a resend timeout's round trip). A peer losing packets (within [`LOSSY_MS`] of a resend timeout passing or a hole
/// in its own packets' sequence, or [`LOSSY_FIRST_MS`] of its first) is sent every tick it has not acknowledged (MAX_RUN at the
/// most) with the next ticks of the stream, as the oracle sent every peer at every pump: with eight streams a peer that
/// loses one packet in twenty loses two in a row of some stream about once a second, and each such pair would cost it a
/// resend timeout's wait. Deviation: the port's own rule.
pub const REPEAT_TICKS: u32 = 1;
/// How long (ms) a peer counts as losing packets after a resend timeout passed or a packet of its own went missing.
/// Deviation: the port's own bound.
pub const LOSSY_MS: f64 = 5000.0;
/// How long (ms) a peer counts as losing packets after its first input packet (and while none of its round trips is
/// known): nothing is known of its ways yet. Deviation: the port's own bound.
pub const LOSSY_FIRST_MS: f64 = 300.0;
/// An input packet's sequence byte this far past the last one's or more is taken as an older packet come late, not as
/// packets lost between them. Deviation: the port's own bound.
pub const SEQ_LATE: u8 = 128;

/// One tick of a stream as sent a peer: when it first and last went, and whether its acknowledgement is a round trip
/// sample (not when it went again past a resend timeout: which of its sends came back is not known).
#[derive(Clone, Copy, Debug)]
struct Went {
    first: f64,
    last: f64,
    sample: bool,
}

/// What a healthy peer was sent of one slot's ruled stream: each ruled tick goes soon after it is ruled and with its
/// stream's next ([`REPEAT_TICKS`]), and again only when the peer has not acknowledged it within its resend timeout
/// ([`Referee::resend_ms`]).
#[derive(Clone, Debug, Default)]
struct Paced {
    /// The first tick never sent.
    upto: u32,
    /// Per tick from `base` to `upto`: how it went.
    base: u32,
    sent: VecDeque<Went>,
    /// When a run carrying the stream's end last went (none: NEG_INFINITY).
    end_at: f64,
}

impl Paced {
    fn new(tick: u32) -> Paced {
        Paced { upto: tick, base: tick, sent: VecDeque::new(), end_at: f64::NEG_INFINITY }
    }

    /// Ticks before `from` forgotten (acknowledged or no longer kept).
    fn trim(&mut self, from: u32) {
        while self.base < from && self.sent.pop_front().is_some() {
            self.base += 1;
        }
        if self.sent.is_empty() || self.base > from {
            self.base = self.base.max(from);
        }
        if self.upto < self.base {
            (self.upto, self.base) = (self.base, self.base);
            self.sent.clear();
        }
    }

    /// Ticks `first .. first + count` sent at `now`; `sample`: their acknowledgement stays a round trip sample (a
    /// tick's first send or its repeat, not a resend past the timeout).
    fn mark(&mut self, first: u32, count: u32, now: f64, sample: bool) {
        for t in first..first + count {
            if t < self.base {
                continue;
            }
            let i = (t - self.base) as usize;
            if let Some(w) = self.sent.get_mut(i) {
                (w.last, w.sample) = (now, w.sample && sample);
            } else {
                while self.sent.len() < i {
                    self.sent.push_back(Went { first: now, last: now, sample: false });
                }
                self.sent.push_back(Went { first: now, last: now, sample });
            }
        }
        self.upto = self.upto.max(first + count);
    }

    /// How tick `t` went.
    fn went(&self, t: u32) -> Option<Went> {
        t.checked_sub(self.base).and_then(|i| self.sent.get(i as usize)).copied()
    }
}

/// One human slot: its stream as ruled, and its peer.
#[derive(Clone, Debug)]
struct Slot {
    slot: usize,
    /// The first tick not ruled yet.
    next: u32,
    /// The first tick still kept of the ruled stream (the oldest some peer has not acknowledged).
    base: u32,
    /// Ticks base .. next - 1: the input as its peer sent it, `None` a substitution.
    ruled: std::collections::VecDeque<Option<WireInput>>,
    /// Inputs that came for ticks from next on, by tick.
    pending: BTreeMap<u32, WireInput>,
    /// The tick its stream ends at (its player left), NO_END.
    end: u32,
    /// What its peer has of each slot's ruled stream, by slot of the match.
    acks: Vec<u32>,
    /// Its peer's newest clock stamp (-1: none yet) and when that came.
    stamp: i64,
    stamp_at: f64,
    /// The sequence byte of its peer's newest input packet.
    seq: u8,
    /// When its peer was last heard from, when it was last sent a packet.
    heard: f64,
    sent_at: f64,
    /// Told away and not back yet.
    away: bool,
    /// Told gone: no more news of it, and nothing more is kept for it.
    gone: bool,
    /// Was away and still lacks more than two runs of a stream: it is sent bursts of up to catch_up_packets.
    catch_up: bool,
    /// Catching up: per slot the tick its runs of the last burst reached.
    pushed: Vec<u32>,
    /// When its last burst went, and its acknowledgements then.
    burst_at: f64,
    burst_acks: Vec<u32>,
    /// What it was sent of each slot's ruled stream, by slot of the match.
    paced: Vec<Paced>,
    /// Its peer's round trip (ms; NaN: not known yet) and its spread, from the ticks it acknowledged that were sent
    /// once.
    srtt: f64,
    rttvar: f64,
    /// Until when its peer counts as losing packets ([`LOSSY_MS`]).
    lossy_until: f64,
}

/// `Math.round`: halves go up.
fn js_round(v: f64) -> f64 {
    let f = v.floor();
    if v - f >= 0.5 { f + 1.0 } else { f }
}

/// One match's referee (`Referee`).
#[derive(Clone, Debug)]
pub struct Referee {
    slots: Vec<Slot>,
    /// The index in `slots` of each slot's, by slot of the match: its length is the match's slot count.
    by_slot: Vec<Option<usize>>,
    /// A tick was ruled since the last pump.
    fresh: bool,
    /// The match's tick 0, in ms of the caller's clock.
    tick0: f64,
}

impl Slot {
    /// A round trip sample (ms) into its peer's smoothed round trip and spread (RFC 6298's gains).
    fn round_trip(&mut self, ms: f64) {
        if self.srtt.is_nan() {
            (self.srtt, self.rttvar) = (ms, ms / 2.0);
        } else {
            self.rttvar = 0.75 * self.rttvar + 0.25 * (self.srtt - ms).abs();
            self.srtt = 0.875 * self.srtt + 0.125 * ms;
        }
    }
}

impl Referee {
    /// `new Referee({ humans, tick0 })`: the match's slot count `slots` (its bots' among them: the length of its
    /// peers' input headers; [`MAX_SLOTS`] at the most, more is taken as that), the human slots of the match (bots are
    /// not ruled: every peer thinks them alike; a slot past the match's or named twice is passed over) and the match's
    /// tick 0 in ms of the clock every later call passes.
    pub fn new(slots: usize, humans: &[usize], tick0: f64) -> Referee {
        debug_assert!(
            slots <= MAX_SLOTS && humans.iter().all(|&s| s < slots),
            "a match of {slots} slots does not hold its humans' {humans:?}"
        );
        let n = slots.min(MAX_SLOTS);
        let mut r = Referee { slots: Vec::new(), by_slot: vec![None; n], fresh: false, tick0 };
        for &s in humans {
            if s >= n || r.by_slot[s].is_some() {
                continue;
            }
            // Nothing is read from a device for the first ticks: they are empty inputs on every peer, and not ruled.
            r.by_slot[s] = Some(r.slots.len());
            r.slots.push(Slot {
                slot: s,
                next: INPUT_DELAY,
                base: INPUT_DELAY,
                ruled: Default::default(),
                pending: BTreeMap::new(),
                end: NO_END,
                acks: vec![INPUT_DELAY; n],
                stamp: -1,
                seq: 0,
                stamp_at: 0.0,
                heard: tick0,
                sent_at: f64::NEG_INFINITY,
                away: false,
                gone: false,
                catch_up: false,
                pushed: vec![0; n],
                burst_at: f64::NEG_INFINITY,
                burst_acks: vec![INPUT_DELAY; n],
                paced: vec![Paced::new(INPUT_DELAY); n],
                srtt: f64::NAN,
                lossy_until: f64::NEG_INFINITY,
                rttvar: 0.0,
            });
        }
        r
    }

    /// The match's slot count (what its packets are checked against: [`packet_fits`]).
    pub fn match_slots(&self) -> usize {
        self.by_slot.len()
    }

    /// `receive`: a packet from the peer of `slot` (the link vouches for the sender; a packet that names another slot
    /// is dropped). A checksum is passed on to the others at once.
    pub fn receive(&mut self, slot: usize, bytes: &[u8], now: f64, out: &mut dyn RefereeOut) {
        let Some(xi) = self.by_slot.get(slot).copied().flatten() else { return };
        let n = self.match_slots();
        if self.slots[xi].end != NO_END || !packet_fits(bytes, n) || usize::from(bytes[1]) != slot {
            return;
        }
        self.slots[xi].heard = now;
        if bytes[0] == MSG_CHECKSUM {
            // (passed on unchanged to the others)
            for (yi, y) in self.slots.iter().enumerate() {
                if yi != xi && y.end == NO_END {
                    out.send(y.slot, &bytes[..CHECKSUM_BYTES]);
                }
            }
            return;
        }
        if bytes[0] != MSG_INPUT {
            return;
        }
        let Some(h) = InputHeader::read(bytes, n) else { return };
        let x = &mut self.slots[xi];
        // (a peer first heard from counts as losing packets a while: nothing is known of its ways yet)
        if x.stamp < 0 {
            x.lossy_until = now + LOSSY_FIRST_MS;
        } else {
            // (a hole in its packets' sequence: its way up lost one, this one's way down most likely loses too; one
            // that comes late is no news and leaves the newest as it was)
            let past = h.seq.wrapping_sub(x.seq);
            if past > 1 && past < SEQ_LATE {
                x.lossy_until = now + LOSSY_MS;
            }
        }
        if self.slots[xi].stamp < 0 || h.seq.wrapping_sub(self.slots[xi].seq) < SEQ_LATE {
            self.slots[xi].seq = h.seq;
        }
        self.slots[xi].stamp = i64::from(h.stamp);
        self.slots[xi].stamp_at = now;
        // (the newest send of a tick it now acknowledges that went once: its round trip's sample)
        let mut newest: Option<f64> = None;
        for yi in 0..self.slots.len() {
            let (ys, yend, ynext) = (self.slots[yi].slot, self.slots[yi].end, self.slots[yi].next);
            let a = h.acks[ys];
            // (an acknowledgement only goes forward; ALL_TICKS: the whole stream of a slot that left)
            let most = if a == ALL_TICKS && yend != NO_END { ALL_TICKS } else { a.min(ynext) };
            if most > self.slots[xi].acks[ys] {
                self.slots[xi].acks[ys] = most;
                if most != ALL_TICKS
                    && let Some(w) = self.slots[xi].paced[ys].went(most - 1).filter(|w| w.sample)
                {
                    newest = Some(newest.map_or(w.first, |n: f64| n.max(w.first)));
                }
            }
        }
        if let Some(at) = newest {
            self.slots[xi].round_trip(now - at);
        }
        // (heard from and taking again: back, and told away once more if it stops again)
        let x = &self.slots[xi];
        if x.away && !x.gone && !self.behind(xi, BEHIND_TICKS) {
            self.slots[xi].away = false;
            out.news(slot, SlotNews::Back);
        }
        // Lateness is judged by the clock at the input's arrival: what came due meanwhile is ruled first.
        let tick = self.tick(now);
        self.due(xi, tick);
        for (k, i) in packet_inputs(bytes, n).iter().enumerate() {
            let t = i64::from(h.first) + k as i64;
            let x = &mut self.slots[xi];
            // (already ruled: it came too late, or twice; or far ahead of the clock)
            if t < i64::from(x.next) || t > tick + i64::from(FAR_AHEAD) {
                continue;
            }
            let Ok(t) = u32::try_from(t) else { continue };
            x.pending.entry(t).or_insert(*i);
        }
        self.take(xi);
    }

    /// `pump`: rules the ticks that came due and sends each peer what it lacks; called at least every TICK_MS (a late
    /// call rules what came due meanwhile).
    pub fn pump(&mut self, now: f64, out: &mut dyn RefereeOut) {
        let tick = self.tick(now);
        for xi in 0..self.slots.len() {
            self.due(xi, tick);
            let x = &self.slots[xi];
            if x.end != NO_END || x.gone {
                continue;
            }
            let quiet = now - x.heard;
            if quiet >= RESUME_MS || self.behind(xi, GONE_TICKS) {
                let x = &mut self.slots[xi];
                (x.gone, x.away, x.catch_up) = (true, false, false);
                out.news(x.slot, SlotNews::Gone);
            } else if !x.away && (quiet >= SILENT_MS || self.behind(xi, BEHIND_TICKS)) {
                let x = &mut self.slots[xi];
                (x.away, x.catch_up) = (true, true);
                out.news(x.slot, SlotNews::Away);
            }
        }
        for xi in 0..self.slots.len() {
            let x = &self.slots[xi];
            // (an away peer is sent on the CLOCK_MS beat, and a burst when it is heard from and its acknowledgements
            // moved; not at each newly ruled tick, which it is not taking)
            // (a healthy peer: every pump, which sends it what was ruled since and what its resend timeout let go)
            let due =
                if x.away { now - x.heard < CLOCK_MS && x.acks != x.burst_acks } else { !x.catch_up || self.fresh };
            // Deviation: a gone peer is sent nothing, not even the CLOCK_MS beat (the referee's own rule before paced
            // sends sent it one): the referee gave it up, its owner tells the match it left, and a packet to it would
            // be read by no session.
            if x.end == NO_END && !x.gone && (due || now - x.sent_at >= CLOCK_MS) {
                self.send_to(xi, now, out);
            }
        }
        self.fresh = false;
        self.forget();
    }

    /// `left`: the slot's player left: its stream ends at the ruled line, empty inputs from there on, for every peer
    /// alike. It sends nothing; the next pump tells the others.
    pub fn left(&mut self, slot: usize, now: f64) {
        let Some(xi) = self.by_slot.get(slot).copied().flatten() else { return };
        if self.slots[xi].end != NO_END {
            return;
        }
        let tick = self.tick(now);
        self.due(xi, tick);
        let x = &mut self.slots[xi];
        x.end = x.next;
        x.pending.clear();
        self.fresh = true;
        // (its peer gets nothing more, and what it has not acknowledged is kept for it no more)
        self.forget();
    }

    /// `tick`: the referee's tick of the match at `now` (below 0 before tick 0).
    pub fn tick(&self, now: f64) -> i64 {
        // (Math.floor of a finite ms count: far inside i64)
        #[allow(clippy::cast_possible_truncation, reason = "a floored ms count of the match's clock")]
        let t = ((now - self.tick0) / TICK_MS).floor() as i64;
        t
    }

    // The slot's ticks whose deadline has passed: the inputs that are in, substitutions for the rest.
    fn due(&mut self, xi: usize, tick: i64) {
        self.take(xi);
        while self.slots[xi].end == NO_END && i64::from(self.slots[xi].next) < tick - i64::from(DEADLINE) {
            let x = &mut self.slots[xi];
            let i = x.pending.remove(&x.next);
            x.ruled.push_back(i);
            x.next += 1;
            self.fresh = true;
            self.take(xi);
        }
    }

    // The inputs that are in from the slot's next tick on, in order.
    fn take(&mut self, xi: usize) {
        let x = &mut self.slots[xi];
        while x.end == NO_END {
            let Some(i) = x.pending.remove(&x.next) else { break };
            x.ruled.push_back(Some(i));
            x.next += 1;
            self.fresh = true;
        }
    }

    /// Whether the referee counts the slot's peer as losing packets at `now` (its sends carry more again; for
    /// readouts and tests).
    pub fn losing(&self, slot: usize, now: f64) -> bool {
        self.by_slot
            .get(slot)
            .copied()
            .flatten()
            .is_some_and(|i| now < self.slots[i].lossy_until || self.slots[i].srtt.is_nan())
    }

    /// `kept`: the ticks of the slot's ruled stream the referee keeps (for readouts and tests).
    pub fn kept(&self, slot: usize) -> usize {
        self.by_slot.get(slot).copied().flatten().map_or(0, |i| self.slots[i].ruled.len())
    }

    // Whether the peer's acknowledgement of a stream still in the match trails its ruled line by more than `ticks`.
    fn behind(&self, xi: usize, ticks: u32) -> bool {
        let x = &self.slots[xi];
        self.slots.iter().any(|y| y.end == NO_END && i64::from(y.next) - i64::from(x.acks[y.slot]) > i64::from(ticks))
    }

    // Of each slot's ruled stream what some peer still in the match has not acknowledged is kept, for an away peer
    // too (it may come back); not for a gone one (its owner takes it out).
    fn forget(&mut self) {
        for yi in 0..self.slots.len() {
            let mut least = self.slots[yi].next;
            let ys = self.slots[yi].slot;
            for xi in 0..self.slots.len() {
                if self.slots[xi].end == NO_END && !self.slots[xi].gone {
                    least = least.min(self.slots[xi].acks[ys]);
                }
            }
            let y = &mut self.slots[yi];
            if least > y.base {
                y.ruled.drain(..(least - y.base) as usize);
                y.base = least;
            }
        }
    }

    // A peer's packets: per slot the oldest ticks it lacks (MAX_RUN of them) and the newest, with the slot's end; the
    // header's clock and the echo of the peer's stamp in each. They go in as few packets as hold them: max_runs runs
    // and MAX_PACKET bytes each at the most. An away peer not heard from within CLOCK_MS gets one
    // packet of the oldest runs alone. A peer catching up after it was away (heard from again within CLOCK_MS) gets no
    // newest of a stream it lacks more of: a burst (catch_up_packets) of the ticks after the oldest when its
    // acknowledgements moved since the last burst or CLOCK_MS has passed, the oldest runs alone between bursts.
    fn send_to(&mut self, xi: usize, now: f64, out: &mut dyn RefereeOut) {
        if !self.slots[xi].away && !self.slots[xi].catch_up {
            self.send_paced(xi, now, out);
            return;
        }
        // (slot index, first tick, count)
        let mut runs: Vec<(usize, u32, u32)> = Vec::new();
        // (per slot index: the first tick after its oldest run, its next)
        let mut gaps: Vec<(usize, u32, u32)> = Vec::new();
        let max_run = MAX_RUN as u32;
        let max_runs = max_runs(self.match_slots());
        let burst_packets = catch_up_packets(self.match_slots());
        let x = &self.slots[xi];
        let unheard = x.away && now - x.heard >= CLOCK_MS;
        let catching = x.catch_up && !x.gone && !unheard;
        let burst = catching && (x.acks != x.burst_acks || now - x.burst_at >= CLOCK_MS);
        // (it lacks more than two runs of a stream)
        let mut far = false;
        for (yi, y) in self.slots.iter().enumerate() {
            let a = self.slots[xi].acks[y.slot];
            if a == ALL_TICKS {
                continue;
            }
            let from = a.max(y.base);
            if from >= y.next && y.end == NO_END {
                continue;
            }
            let count = max_run.min(y.next.saturating_sub(from));
            runs.push((yi, from, count));
            if y.next - from > max_run {
                far |= y.next - from > 2 * max_run;
                let newest = (from + max_run).max(y.next - max_run);
                if (catching || unheard) && newest > from + max_run {
                    if burst {
                        gaps.push((yi, from + max_run, y.next));
                    }
                } else if !unheard {
                    runs.push((yi, newest, y.next - newest));
                }
            }
        }
        let x = &self.slots[xi];
        if x.catch_up && !x.away && !far {
            self.slots[xi].catch_up = false;
        }
        let mut most = if unheard { 1 } else { usize::MAX };
        let regular = runs.len();
        if !gaps.is_empty() {
            let x = &mut self.slots[xi];
            x.burst_at = now;
            x.burst_acks.clone_from(&x.acks);
            // (the gaps' runs, a run of each slot in turn, each slot's from where the last pump's stopped (none came
            // back yet) up to CATCH_UP_AHEAD ticks past what the peer has, then from the gap's start again; a packet
            // holds max_runs runs at the most)
            most = burst_packets;
            let mut at: Vec<(usize, u32, u32)> = gaps
                .iter()
                .map(|&(yi, lo, newest)| {
                    let hi = newest.min(lo - max_run + CATCH_UP_AHEAD);
                    let p = self.slots[xi].pushed[self.slots[yi].slot];
                    (yi, if p > lo && p < hi { p } else { lo }, hi)
                })
                .collect();
            while at.iter().any(|g| g.1 < g.2) && runs.len() < burst_packets * max_runs {
                for g in &mut at {
                    if g.1 < g.2 {
                        let count = max_run.min(g.2 - g.1);
                        runs.push((g.0, g.1, count));
                        g.1 += count;
                    }
                }
            }
        }
        self.send_runs(xi, now, &runs, regular, most, out);
        // (what went counts as sent, as resent: no round trip sample, and a resend only past the timeout)
        for &(yi, first, count) in &runs {
            let ys = self.slots[yi].slot;
            let (from, end) = (self.slots[xi].acks[ys].max(self.slots[yi].base), self.slots[yi].end);
            let p = &mut self.slots[xi].paced[ys];
            p.trim(from);
            p.mark(first, count, now, false);
            if end != NO_END {
                p.end_at = now;
            }
        }
    }

    // A healthy peer's packets: per slot what was ruled since it was last sent that stream (MAX_RUN ticks a run, from
    // REPEAT_TICKS before it; for a peer losing packets from the oldest it has not acknowledged, MAX_RUN ticks back at
    // the most), and the oldest ticks it has not acknowledged when they were last sent [`Referee::resend_ms`] ago or
    // more (one run); a stream that ended, a run with its end when the end has not gone or went that long ago. A packet
    // that goes to a peer losing packets also carries, of each stream with nothing new, the ticks first sent it within
    // the last tick. Nothing to send: a packet of no runs once CLOCK_MS has passed since the last (the clock and the
    // stamp's echo). Every carrier's owner pumps after each packet it takes and on its beat: a ruled tick goes out at
    // the pump that ruled it.
    fn send_paced(&mut self, xi: usize, now: f64, out: &mut dyn RefereeOut) {
        let max_run = MAX_RUN as u32;
        let resend = self.resend_ms(xi);
        let losing = self.losing(self.slots[xi].slot, now);
        let mut lossy = false;
        let mut runs: Vec<(usize, u32, u32)> = Vec::new();
        // (the runs that ride on a packet that goes anyway)
        let mut riders: Vec<(usize, u32, u32)> = Vec::new();
        for yi in 0..self.slots.len() {
            let (ys, next, end, base) =
                (self.slots[yi].slot, self.slots[yi].next, self.slots[yi].end, self.slots[yi].base);
            let a = self.slots[xi].acks[ys];
            if a == ALL_TICKS {
                continue;
            }
            let from = a.max(base);
            let p = &mut self.slots[xi].paced[ys];
            p.trim(from);
            let had = runs.len();
            // (the oldest it lacks, sent long enough ago)
            let mut resent = from;
            if from < p.upto && p.went(from).is_none_or(|w| now - w.last >= resend) {
                let count = max_run.min(p.upto - from);
                runs.push((yi, from, count));
                p.mark(from, count, now, false);
                resent = from + count;
                // (a tick sent before and not acknowledged in time: lost, or its acknowledgement was)
                if p.went(from).is_some_and(|w| w.first < now) {
                    lossy = true;
                }
            }
            // (what was ruled since, from the last ticks sent before it that are not acknowledged nor resent now)
            let mut start = if losing {
                resent.max(next.saturating_sub(max_run)).min(p.upto)
            } else {
                p.upto.saturating_sub(REPEAT_TICKS).max(resent)
            };
            // (a peer losing packets: with no new tick of the stream, the ticks first sent it within the last tick go
            // again in each packet, so a lost one comes with the next packet of any stream)
            let mut window = p.upto;
            while losing && window > resent && p.went(window - 1).is_some_and(|w| now - w.first < TICK_MS) {
                window -= 1;
            }
            if p.upto >= next && window < p.upto {
                riders.push((yi, window, p.upto - window));
            }
            while start < next && p.upto < next {
                let count = max_run.min(next - start);
                runs.push((yi, start, count));
                p.mark(start, count, now, true);
                start += count;
            }
            if end != NO_END {
                if runs.len() == had && now - p.end_at >= resend {
                    runs.push((yi, from, max_run.min(next.saturating_sub(from))));
                }
                if runs.len() > had {
                    p.end_at = now;
                }
            }
        }
        if lossy {
            self.slots[xi].lossy_until = now + LOSSY_MS;
        }
        if !runs.is_empty() {
            for &(yi, first, count) in &riders {
                let ys = self.slots[yi].slot;
                self.slots[xi].paced[ys].mark(first, count, now, true);
            }
            runs.extend(riders);
        }
        if runs.is_empty() && now - self.slots[xi].sent_at < CLOCK_MS {
            return;
        }
        let regular = runs.len();
        self.send_runs(xi, now, &runs, regular, usize::MAX, out);
    }

    /// The resend timeout of the peer of slot index `xi` (ms): its round trip, four times its spread (a tick at the
    /// least) and [`RESEND_MARGIN_MS`]; [`RESEND_FIRST_MS`] while no round trip is known.
    fn resend_ms(&self, xi: usize) -> f64 {
        let x = &self.slots[xi];
        if x.srtt.is_nan() { RESEND_FIRST_MS } else { x.srtt + (4.0 * x.rttvar).max(TICK_MS) + RESEND_MARGIN_MS }
    }

    // The runs as packets to the peer of slot index `xi`: as few as hold them, max_runs runs and MAX_PACKET bytes each
    // at the most, `most` packets at the most; the runs from `regular` on are a burst's (`pushed`).
    fn send_runs(
        &mut self,
        xi: usize,
        now: f64,
        runs: &[(usize, u32, u32)],
        regular: usize,
        most: usize,
        out: &mut dyn RefereeOut,
    ) {
        let max_runs = max_runs(self.match_slots());
        let x = &self.slots[xi];
        let mut packets = 0;
        let mut pushed = x.pushed.clone();
        let mut msg = vec![0u8; MAX_PACKET];
        // (held NO_STAMP: no stamp of the peer's has come yet)
        #[allow(clippy::cast_possible_truncation, reason = "Math.round of the referee's ms clock into an i32 field")]
        let clock = js_round(now - self.tick0) as i32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 0 ..= NO_STAMP - 1")]
        let held =
            if x.stamp < 0 { NO_STAMP } else { js_round(now - x.stamp_at).clamp(0.0, f64::from(NO_STAMP - 1)) as u16 };
        RuledHeader { runs: 0, clock, stamp: u32::try_from(x.stamp).unwrap_or(u32::MAX), held }.write(&mut msg);
        let (mut at, mut n) = (RULED_HEADER, 0u8);
        for r in 0..=runs.len() {
            let run = runs.get(r);
            let (mut inputs, mut mask) = (0usize, 0u32);
            if let Some(&(yi, first, count)) = run {
                let y = &self.slots[yi];
                for k in 0..count {
                    if y.ruled.get((first + k - y.base) as usize).is_some_and(Option::is_some) {
                        inputs += 1;
                    } else {
                        mask |= 1 << k;
                    }
                }
            }
            // (the packet so far, when it is the last or the next run does not fit in it)
            if run.is_none() || at + RUN_HEADER + inputs * INPUT_BYTES > MAX_PACKET || usize::from(n) == max_runs {
                msg[1] = n;
                out.send(x.slot, &msg[..at]);
                at = RULED_HEADER;
                n = 0;
                packets += 1;
            }
            let Some(&(yi, first, count)) = run else { break };
            if packets == most {
                break;
            }
            let y = &self.slots[yi];
            RunHeader { slot: y.slot as u8, first, count: count as u8, mask, end: y.end }.write(&mut msg[at..]);
            at += RUN_HEADER;
            for k in 0..count {
                if let Some(Some(i)) = y.ruled.get((first + k - y.base) as usize) {
                    msg[at..at + INPUT_BYTES].copy_from_slice(i);
                    at += INPUT_BYTES;
                }
            }
            n += 1;
            if r >= regular {
                pushed[y.slot] = first + count;
            }
        }
        if regular < runs.len() {
            self.slots[xi].pushed = pushed;
        }
        self.slots[xi].sent_at = now;
    }
}
