//! A match's referee: the one place that owns the match's tick clock and rules every human
//! slot's input stream. It runs in the hosting player's net thread or on a relay; it reads a packet's type, slot and
//! ticks and never what an input means ([`crate::match_wire`] has the packets). Sans-IO: the clock is passed in (ms
//! of a clock that only runs forward) and what it sends goes to a [`RefereeOut`].
//!
//! Deviation: the port's own netcode, the rule of DEADLINE and FAR_AHEAD; the build ran dedicated servers.

use std::collections::BTreeMap;

use crate::match_wire::{
    ALL_TICKS, CHECKSUM_BYTES, CLOCK_MS, DEADLINE, FAR_AHEAD, INPUT_BYTES, INPUT_DELAY, InputHeader, MAX_PACKET,
    MAX_RUN, MAX_RUNS, MAX_SLOTS, MSG_CHECKSUM, MSG_INPUT, NO_END, NO_STAMP, RESUME_MS, RULED_HEADER, RUN_HEADER,
    RuledHeader, RunHeader, SILENT_MS, TICK_MS, WireInput, packet_fits, packet_inputs,
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

/// The most ruled packets a burst sends to one peer catching up after it was away (each holds a run of MAX_RUN
/// inputs or more): at 8 a peer back from 19 s away (about 1140 ticks) has every stream within a few round trips. A
/// burst goes when the peer's acknowledgements moved since the last one, else once a CLOCK_MS (a deaf peer is sent
/// a burst a CLOCK_MS, not one a pump). Deviation: the port's own bound.
pub const CATCH_UP_PACKETS: usize = 8;
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
    /// What its peer has of each slot's ruled stream, by slot.
    acks: [u32; MAX_SLOTS],
    /// Its peer's newest clock stamp (-1: none yet) and when that came.
    stamp: i64,
    stamp_at: f64,
    /// When its peer was last heard from, when it was last sent a packet.
    heard: f64,
    sent_at: f64,
    /// Told away and not back yet.
    away: bool,
    /// Told gone: no more news of it, and nothing more is kept for it.
    gone: bool,
    /// Was away and still lacks more than two runs of a stream: it is sent bursts of up to CATCH_UP_PACKETS.
    catch_up: bool,
    /// Catching up: per slot the tick its runs of the last burst reached.
    pushed: [u32; MAX_SLOTS],
    /// When its last burst went, and its acknowledgements then.
    burst_at: f64,
    burst_acks: [u32; MAX_SLOTS],
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
    /// The index in `slots` of each slot's.
    by_slot: [Option<usize>; MAX_SLOTS],
    /// A tick was ruled since the last pump.
    fresh: bool,
    /// The match's tick 0, in ms of the caller's clock.
    tick0: f64,
}

impl Referee {
    /// `new Referee({ humans, tick0 })`: the human slots of the match (bots are not ruled: every peer thinks them
    /// alike; a slot past [`MAX_SLOTS`] or named twice is passed over) and the match's tick 0 in ms of the clock every
    /// later call passes.
    pub fn new(humans: &[usize], tick0: f64) -> Referee {
        let mut r = Referee { slots: Vec::new(), by_slot: [None; MAX_SLOTS], fresh: false, tick0 };
        for &s in humans {
            if s >= MAX_SLOTS || r.by_slot[s].is_some() {
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
                acks: [INPUT_DELAY; MAX_SLOTS],
                stamp: -1,
                stamp_at: 0.0,
                heard: tick0,
                sent_at: f64::NEG_INFINITY,
                away: false,
                gone: false,
                catch_up: false,
                pushed: [0; MAX_SLOTS],
                burst_at: f64::NEG_INFINITY,
                burst_acks: [INPUT_DELAY; MAX_SLOTS],
            });
        }
        r
    }

    /// `receive`: a packet from the peer of `slot` (the link vouches for the sender; a packet that names another slot
    /// is dropped). A checksum is passed on to the others at once.
    pub fn receive(&mut self, slot: usize, bytes: &[u8], now: f64, out: &mut dyn RefereeOut) {
        let Some(xi) = self.by_slot.get(slot).copied().flatten() else { return };
        if self.slots[xi].end != NO_END || !packet_fits(bytes) || usize::from(bytes[1]) != slot {
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
        let Some(h) = InputHeader::read(bytes) else { return };
        self.slots[xi].stamp = i64::from(h.stamp);
        self.slots[xi].stamp_at = now;
        for yi in 0..self.slots.len() {
            let (ys, yend, ynext) = (self.slots[yi].slot, self.slots[yi].end, self.slots[yi].next);
            let a = h.acks[ys];
            // (an acknowledgement only goes forward; ALL_TICKS: the whole stream of a slot that left)
            let most = if a == ALL_TICKS && yend != NO_END { ALL_TICKS } else { a.min(ynext) };
            if most > self.slots[xi].acks[ys] {
                self.slots[xi].acks[ys] = most;
            }
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
        for (k, i) in packet_inputs(bytes).iter().enumerate() {
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
            let due = if x.away { now - x.heard < CLOCK_MS && x.acks != x.burst_acks } else { self.fresh };
            if x.end == NO_END && (due || now - x.sent_at >= CLOCK_MS) {
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
    // header's clock and the echo of the peer's stamp in each. An away peer not heard from within CLOCK_MS gets one
    // packet of the oldest runs alone. A peer catching up after it was away (heard from again within CLOCK_MS) gets no
    // newest of a stream it lacks more of: a burst (CATCH_UP_PACKETS) of the ticks after the oldest when its
    // acknowledgements moved since the last burst or CLOCK_MS has passed, the oldest runs alone between bursts.
    fn send_to(&mut self, xi: usize, now: f64, out: &mut dyn RefereeOut) {
        // (slot index, first tick, count)
        let mut runs: Vec<(usize, u32, u32)> = Vec::new();
        // (per slot index: the first tick after its oldest run, its next)
        let mut gaps: Vec<(usize, u32, u32)> = Vec::new();
        let max_run = MAX_RUN as u32;
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
            (x.burst_at, x.burst_acks) = (now, x.acks);
            // (the gaps' runs, a run of each slot in turn, each slot's from where the last pump's stopped (none came
            // back yet) up to CATCH_UP_AHEAD ticks past what the peer has, then from the gap's start again; a packet
            // holds MAX_RUNS runs at the most)
            most = CATCH_UP_PACKETS;
            let mut at: Vec<(usize, u32, u32)> = gaps
                .iter()
                .map(|&(yi, lo, newest)| {
                    let hi = newest.min(lo - max_run + CATCH_UP_AHEAD);
                    let p = self.slots[xi].pushed[self.slots[yi].slot];
                    (yi, if p > lo && p < hi { p } else { lo }, hi)
                })
                .collect();
            while at.iter().any(|g| g.1 < g.2) && runs.len() < CATCH_UP_PACKETS * MAX_RUNS {
                for g in &mut at {
                    if g.1 < g.2 {
                        let count = max_run.min(g.2 - g.1);
                        runs.push((g.0, g.1, count));
                        g.1 += count;
                    }
                }
            }
        }
        let x = &self.slots[xi];
        let mut packets = 0;
        let mut pushed = x.pushed;
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
            if run.is_none() || at + RUN_HEADER + inputs * INPUT_BYTES > MAX_PACKET || usize::from(n) == MAX_RUNS {
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
