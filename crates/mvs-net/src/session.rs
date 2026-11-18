//! One peer's side of an online match: rollback on the deterministic sim, inputs only on the
//! wire.
//!
//! Every peer runs the same World. A referee (`mvs_protocol::referee`) owns the match's clock and rules every human
//! slot's input stream: each tick of a slot is the input its peer sent in time, or a substitution. A peer sends its own
//! inputs to the referee alone and takes every slot's ruled stream from it, its own among them. A tick is confirmed
//! once it is ruled for every human slot, so the confirmed line is the same on every peer, tick by tick.
//! The peer keeps a snapshot at the end of the confirmed line and shows a predicted line beyond it: its own inputs as
//! it sent them, and for the others what a substitution would be. When rulings arrive, the World goes back to the
//! snapshot, the confirmed line moves on, a new snapshot is taken and the predicted ticks are simulated again; when the
//! guesses held, the predicted ticks are taken as they ran (`holds`).
//! No peer waits for another: a peer waits only when its own way to the referee is late, and its clock follows the
//! referee's, not the other peers'.
//! Bots are not part of the World's state and cannot go back. They think only on the confirmed line, once per tick and
//! in order, so every peer gets the same bot inputs; on the predicted line a bot repeats its last input. (A minion's
//! controller does the same inside the step: `World::predicted`.)
//!
//! Sans-IO: the clock is passed in (ms of a clock that only runs forward); what the session sends waits in its outbox
//! ([`Session::packets`]) for the caller's link.
//!
//! Deviation: the port's own netcode; the build ran dedicated servers with client prediction.

use std::collections::{BTreeMap, VecDeque};
use std::time::Instant;

use mvs_protocol::match_wire::{
    ALL_TICKS, Checksum, DEADLINE, INPUT_DELAY, InputHeader, MAX_LOCAL_DELAY, MAX_RUN, MAX_SLOTS, MSG_CHECKSUM,
    MSG_RULED, NO_END, NO_STAMP, RuledHeader, TICK_MS, WireInput, input_packet, packet_fits, runs,
};
use mvs_sim::ai::bt::BtBot;
use mvs_sim::events::EndPhase;
use mvs_sim::input::{NO_INPUT, canon_input, held};
use mvs_sim::math::dbl::{round, to_uint32};
use mvs_sim::snapshot::SnapshotRing;
use mvs_sim::{Event, InputState, World};

use crate::codec::{decode, encode, same_input};
use crate::events::{EventLog, undo};

/// Predicted ticks beyond the confirmed line; the peer waits at the limit. The referee rules a tick within DEADLINE
/// ticks, so the limit is reached only when this peer's own way to and from the referee is the late one.
pub const MAX_PREDICTION: u32 = 16;
/// A substituted tick of a slot (and the guess for a tick not ruled yet): the slot's last input ruled with its peer's
/// input held for HOLD_TICKS ticks, an empty input after (a player who is away stops instead of walking off the stage).
/// Deviation: the port's own rule.
pub const HOLD_TICKS: u32 = 8;
/// Ticks behind the referee's clock from which the peer jumps to it in one frame. Up to it a faster pace (17 ticks in
/// 16 frames) closes the gap, and a peer more than a tick ahead runs 15 in 16.
pub const CATCHUP: u32 = 4;
/// The share of a frame left out or run twice per frame while the peer is more than a tick off the referee's clock.
const PACE: f64 = 1.0 / 16.0;
/// Ticks one frame's jump runs at the most (10 s of the match): a game that was away for longer comes back over
/// several frames.
pub const MAX_JUMP: u32 = 600;
/// Frames without a late input after which the local delay goes down by one.
pub const LOWER_AFTER: u32 = 300;
/// Ticks the local delay goes up when an input this peer sent came back substituted.
pub const MISS_RAISE: u32 = 2;
/// Confirmed ticks between state hashes sent to the other peers.
pub const CHECKSUM_EVERY: u32 = 30;
/// Confirmed ticks the snapshot may fall behind the confirmed line while the predicted line is kept: the ticks a later
/// rollback runs again on top of the predicted ones. The snapshot follows the confirmed line by its candidates
/// (`cand_at`), a prediction's length behind at the most, so this is reached only past 7 ticks of prediction; a longer
/// prediction takes no candidate (one past KEEP_BEHIND of the snapshot could never be taken) and goes back as before.
pub const KEEP_BEHIND: u32 = 8;

/// The referee's clock is read from the ruled packets of the last CLOCK_WINDOW_MS: the one whose round trip was the
/// shortest gives the referee's clock at its arrival but for its way here (half that round trip).
const CLOCK_WINDOW_MS: f64 = 5000.0;
/// Ticks past a slot's contiguous line kept of the rulings that came ahead of the ones before.
const EARLY_TICKS: u32 = 600;
/// Own state hashes kept for the others' to be compared with.
const SUMS_KEPT: u32 = 64;
/// No end yet: a stream still in the match, a miss counted from any tick.
const NEVER: u32 = u32::MAX;

/// `SlotKind`: one per fighter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    Local,
    Remote,
    Bot,
}

/// `SessionConfig` but the World, the bots and the callbacks (passed to the calls).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionConfig {
    /// One per fighter; exactly one [`SlotKind::Local`], at most `MAX_SLOTS`.
    pub slots: Vec<SlotKind>,
    /// The smallest input delay: at least [`INPUT_DELAY`] (where the referee starts the streams).
    pub delay: u32,
    pub max_prediction: u32,
    /// Confirmed ticks the snapshot may fall behind the confirmed line while a predicted line that held is kept
    /// ([`KEEP_BEHIND`]); 0: every frame that confirms a tick goes back to the snapshot (the checks' comparison: the
    /// shown line, its events and the packets are the same either way). A match with a bot keeps none.
    pub keep_behind: u32,
}

impl SessionConfig {
    /// The defaults for these slots.
    pub fn new(slots: Vec<SlotKind>) -> SessionConfig {
        SessionConfig { slots, delay: INPUT_DELAY, max_prediction: MAX_PREDICTION, keep_behind: KEEP_BEHIND }
    }
}

/// What a session cannot start with.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    #[error("session: the input delay {0} is under the referee's {INPUT_DELAY}")]
    Delay(u32),
    #[error("session: bad slots (no local one, two, or more than the match's)")]
    Slots,
}

/// A state hash of the confirmed line that another peer's checksum disagrees with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Desync {
    /// The confirmed ticks the hashes are of.
    pub tick: u32,
    /// The other peer's slot.
    pub slot: usize,
    pub mine: u32,
    pub theirs: u32,
}

/// `Session.stats`. steps: the World's ticks run in all (confirmed, simulated again and predicted); late: rulings for
/// a tick the shown line had already run on a guess; stalls: frames this peer did not step because its own rulings were
/// late (the prediction limit); skips: frames left out because this peer ran ahead of the referee's clock; extra:
/// frames that ran a second tick because it ran behind; catchups / jumped: jumps to the referee's clock and the ticks
/// they ran beyond the frame's own; missed: the times an input this peer sent came back substituted and the delay was
/// raised for it; substituted: by slot, the ticks ruled without an input; the ms are the time spent, by part.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub rollbacks: u64,
    pub resimulated: u64,
    pub deepest: u32,
    pub stalls: u64,
    pub skips: u64,
    pub extra: u64,
    pub catchups: u64,
    pub jumped: u64,
    pub steps: u64,
    pub late: u64,
    pub missed: u64,
    pub ms_restore: f64,
    pub ms_confirm: f64,
    pub ms_save: f64,
    pub ms_predict: f64,
    pub substituted: Vec<u64>,
}

/// What one frame of [`Session::advance`] gives back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Advance {
    /// The events to show: the fresh ones of every tick the frame ran, and the undoing of those a wrong prediction
    /// showed (`events::undo`).
    pub events: Vec<Event>,
    /// The shown line may not run further ahead of the confirmed one (the referee's rulings reach this peer too late).
    pub stalled: bool,
    /// The frame was left out because this peer ran ahead of the referee's clock (it costs the caller that one tick).
    pub skipped: bool,
    /// It jumped to the referee's clock (the caller owes no more ticks this frame).
    pub caught: bool,
}

/// (the lag panel) Of one human slot: the ticks of it the shown line runs on a guess, the ticks ruled without its
/// input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SlotInfo {
    pub missing: u32,
    pub substituted: u64,
}

/// A ruled stream's tick: the input its peer sent, or a substitution.
type Ruled = Option<InputState>;

/// One sample of the referee's clock: its arrival, its round trip, and the referee's ms of the match less this peer's
/// own ms at the arrival (one-way time added).
#[derive(Clone, Copy, Debug)]
struct ClockSample {
    at: f64,
    rtt: f64,
    offset: f64,
}

/// Events of a wrong prediction that cannot be taken back: shown only once their tick is confirmed.
fn confirmed_only(e: &Event) -> bool {
    matches!(e, Event::MatchOver { .. } | Event::MatchEnd { .. })
}

/// The u64 state hash as the wire's u32 (its two halves folded).
/// The Rust walk (`mvs_sim::state`) is 64 bits, folded here to the wire's 32.
#[allow(clippy::cast_possible_truncation, reason = "the fold keeps the low 32 bits of the halves' xor")]
pub fn wire_hash(h: u64) -> u32 {
    (h ^ (h >> 32)) as u32
}

/// `Session`: one peer's side of the match.
pub struct Session {
    cfg: SessionConfig,
    local: usize,
    /// Ticks of the shown (predicted) line: the next tick to simulate.
    frame: u32,
    /// Ticks of the confirmed line.
    confirmed: u32,
    /// Ticks the World itself has run (snap_at <= confirmed <= simulated <= frame).
    simulated: u32,
    desync: Option<Desync>,
    /// When the last ruled packet came (ms of the caller's clock).
    ruled_heard: Option<f64>,
    stats: Stats,
    /// Ticks between reading the local device and the tick the input is sent for. Two things ask for more of them on
    /// a long way to the referee:
    ///   the referee takes an input for tick T until its own tick T + DEADLINE: the one-way time in ticks - DEADLINE +
    ///     2 (a tick of margin and the tick the frame itself takes)
    ///   this peer's own input is confirmed when it has come back, a round trip after it was sent, and the shown line
    ///     may be max_prediction ticks ahead of that: the round trip in ticks + 2 - max_prediction
    /// The larger of the two, not under the smallest delay and not over MAX_LOCAL_DELAY, is the floor (`base`): it
    /// rests on the shortest round trip of the last few seconds (the clock's), which says nothing of jitter or of a way
    /// that has just got longer. What says so is the ruling itself: a tick this peer sent an input for that comes back
    /// as a substitution (a miss; not a tick it left out itself). A miss raises the delay by MISS_RAISE at once, and
    /// the misses after it are not counted until the first tick sent after the raise is ruled: one late burst is one
    /// raise. After LOWER_AFTER frames without a miss the delay goes down by one, never under the floor (the frame of a
    /// lowering sends nothing: its tick was sent before, and its device input is lost). Only this peer's own way
    /// counts: a far player does not raise anyone else's. Deviation: the port's own rule.
    local_delay: u32,
    /// The tick of the confirmed line that ended the match (matchEnd, GameEnd).
    game_end: Option<u32>,
    /// The snapshot at the confirmed line and the predicted line's candidate (`snap` / `cand`): two slots, the ring's
    /// tick being the slot's number here (`snap_slot` and the other one).
    ring: SnapshotRing<World>,
    snap_slot: u32,
    snap_at: u32,
    /// A snapshot of the predicted line at this tick (`candAt`): the snapshot's next line once the predicted line has
    /// held up to it (`holds`), with no rollback for it.
    cand_at: Option<u32>,
    log: EventLog,
    /// By slot, by tick: a human slot's as the referee ruled them, a bot's as thought on the confirmed line.
    inputs: Vec<Vec<InputState>>,
    /// By human slot: the referee's rulings of ticks 0 .. have - 1 are in.
    have: Vec<u32>,
    /// By human slot: the latest tick ruled with an input of its peer (-1: none yet).
    last: Vec<i64>,
    /// By human slot: the tick its stream ends at (its player left), NEVER before.
    end: Vec<u32>,
    /// Rulings that came ahead of the ones before them, by slot, by tick.
    early: Vec<indexmap::IndexMap<u32, Ruled>>,
    /// The local device inputs sent and not ruled yet, by tick.
    mine: BTreeMap<u32, InputState>,
    /// The last tick an input was sent for.
    sent: u32,
    /// The first tick of the run of own inputs that ends at `sent` (a gap starts a new one).
    stream_from: u32,
    /// The frame of the last miss, raise or lowering: LOWER_AFTER frames on, the delay goes down.
    quiet: u32,
    /// Misses of ticks before this one are not counted (NEVER: until the next tick is sent).
    miss_from: u32,
    /// The share of a frame to leave out or to run twice, summed (CATCHUP).
    pace: f64,
    /// By tick of the predicted line (a ring over its ticks since the confirmed one): the inputs the World stepped it
    /// with, and the tick each entry is of.
    stepped: Vec<Vec<InputState>>,
    stepped_at: Vec<Option<u32>>,
    ring_mask: u32,
    /// The events a predicted tick held back until it is confirmed (`confirmed_only`: its whole list), by tick.
    held_back: BTreeMap<u32, Vec<Event>>,
    /// 0: never kept (a bot thinks on the confirmed state before each tick).
    keep_behind: u32,
    /// A ruling arrived for a tick the predicted line has already run on a guess.
    late: bool,
    sums: BTreeMap<u32, u32>,
    their_sums: BTreeMap<u32, Vec<(usize, u32)>>,
    clock: VecDeque<ClockSample>,
    /// The packets to the referee not yet taken by the caller.
    outbox: Vec<Vec<u8>>,
    on_checksum: Option<ChecksumHook>,
    on_confirmed: Option<ConfirmedHook>,
    on_ruled: Option<RuledHook>,
    /// (tests) The first own state hash at or past this tick is made wrong (`spoil_hash_at`).
    spoil_at: Option<u32>,
}

/// `onChecksum`: called with each state hash of the confirmed line as it is made (its confirmed ticks, the hash
/// sent), and the World at that state.
pub type ChecksumHook = Box<dyn FnMut(u32, u32, &World)>;

/// Called with each tick the confirmed line steps (not one taken as the predicted line ran it, `holds`): the tick, the
/// World after it, the inputs it was stepped with and its events (the whole list, before the log). The checks' window
/// on the confirmed line (a desync dump, a trace of it).
pub type ConfirmedHook = Box<dyn FnMut(u32, &World, &[InputState], &[Event])>;

/// Called with every tick of the confirmed line once, in order from 0, on both of its ways (stepped again, or taken as
/// the predicted line ran it): the tick and each slot's input it ran with (a bot's its thought). The desync report's
/// record of the match.
pub type RuledHook = Box<dyn FnMut(u32, &[InputState])>;

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("frame", &self.frame).field("confirmed", &self.confirmed).finish()
    }
}

/// ms since `t`, and `t` moved to now.
fn lap(t: &mut Instant) -> f64 {
    let n = Instant::now();
    let ms = (n - *t).as_secs_f64() * 1000.0;
    *t = n;
    ms
}

/// A tick as the u32 the streams count in (the session's ticks are never below 0 nor past a u32).
fn tick_of(t: i64) -> u32 {
    u32::try_from(t).expect("a tick of the match")
}

impl Session {
    /// `new Session(c)`: `world` at its start state (its snapshot ring is made from it); `now` the clock's ms.
    pub fn new(world: &World, cfg: SessionConfig, now: f64) -> Result<Session, SessionError> {
        let _ = now;
        // (the referee's streams start at INPUT_DELAY: an input for a tick before it is never taken)
        if cfg.delay < INPUT_DELAY {
            return Err(SessionError::Delay(cfg.delay));
        }
        let locals = cfg.slots.iter().filter(|&&k| k == SlotKind::Local).count();
        let local = cfg.slots.iter().position(|&k| k == SlotKind::Local).ok_or(SessionError::Slots)?;
        if locals > 1 || cfg.slots.len() > MAX_SLOTS {
            return Err(SessionError::Slots);
        }
        let n = cfg.slots.len();
        let bot = |s: usize| cfg.slots[s] == SlotKind::Bot;
        // Nothing is read from a device for the first ticks: they are empty inputs on every peer.
        let inputs = (0..n).map(|s| if bot(s) { Vec::new() } else { vec![NO_INPUT; INPUT_DELAY as usize] }).collect();
        let have = (0..n).map(|s| if bot(s) { 0 } else { INPUT_DELAY }).collect();
        let mut ring = SnapshotRing::new(2, world);
        ring.save(0, world);
        // (the predicted line is never longer than max_prediction ticks past the confirmed one)
        let mut size = 1u32;
        while size <= cfg.max_prediction {
            size *= 2;
        }
        let keep_behind = if cfg.slots.contains(&SlotKind::Bot) { 0 } else { cfg.keep_behind };
        Ok(Session {
            local,
            frame: 0,
            confirmed: 0,
            simulated: 0,
            desync: None,
            ruled_heard: None,
            stats: Stats { substituted: vec![0; n], ..Stats::default() },
            local_delay: cfg.delay,
            game_end: None,
            ring,
            snap_slot: 0,
            snap_at: 0,
            cand_at: None,
            log: EventLog::new(),
            inputs,
            have,
            last: vec![-1; n],
            end: vec![NEVER; n],
            early: vec![indexmap::IndexMap::new(); n],
            mine: BTreeMap::new(),
            sent: INPUT_DELAY - 1,
            stream_from: INPUT_DELAY,
            quiet: 0,
            miss_from: 0,
            pace: 0.0,
            stepped: vec![Vec::new(); size as usize],
            stepped_at: vec![None; size as usize],
            ring_mask: size - 1,
            held_back: BTreeMap::new(),
            keep_behind,
            late: false,
            sums: BTreeMap::new(),
            their_sums: BTreeMap::new(),
            clock: VecDeque::new(),
            outbox: Vec::new(),
            on_checksum: None,
            on_confirmed: None,
            on_ruled: None,
            spoil_at: None,
            cfg,
        })
    }

    fn human(&self, s: usize) -> bool {
        matches!(self.cfg.slots.get(s), Some(SlotKind::Local | SlotKind::Remote))
    }

    /// A human slot's input for tick t as a frame would step it now: ruled, or the guess.
    fn input_at(&self, s: usize, t: u32) -> InputState {
        if t < self.have[s] { self.ruled_input(s, t) } else { self.guess(s, t) }
    }

    /// The predicted line holds up to `reach` (the ticks to confirm in this frame, those it has run): the World needs
    /// no rollback when every tick it ran since the confirmed one was stepped with the inputs a frame would give it
    /// now, so that going back to the confirmed line and running those ticks again would come to the very state it is
    /// in, with the same events. Only when
    ///   no slot is a bot, and no pawn past the players has a controller (`World::pawn_ctl`: a predicted step holds the
    ///     controller's input where a confirmed one thinks; the entries are taken away by a confirmed step alone, so
    ///     none now is none in any of those steps): `World::predicted` then changes nothing in a step
    ///   no state hash falls in the ticks confirmed by this (one is made on the confirmed state: `checksum`)
    ///   the snapshot stays within keep_behind ticks of the confirmed line.
    /// The port's own (a rollback session's cost): what is shown and sent is the same either way.
    fn holds(&self, w: &World, reach: u32) -> bool {
        if self.keep_behind == 0 || !w.pawn_ctl.ctl.is_empty() {
            return false;
        }
        if reach / CHECKSUM_EVERY != self.confirmed / CHECKSUM_EVERY {
            return false;
        }
        if reach - self.snap_at > self.keep_behind {
            return false;
        }
        for k in self.confirmed..self.simulated {
            let i = (k & self.ring_mask) as usize;
            if self.stepped_at[i] != Some(k) {
                return false;
            }
            let used = &self.stepped[i];
            for (s, u) in used.iter().enumerate() {
                if !same_input(&self.input_at(s, k), u) {
                    return false;
                }
            }
        }
        true
    }

    /// What a tick of a human slot is when the referee rules it without an input, and so also the guess for a tick
    /// not ruled yet (HOLD_TICKS). The same on every peer: `last` is the latest tick before this one ruled with an
    /// input.
    fn substitute(&self, s: usize, t: u32) -> InputState {
        let l = self.last[s];
        if l >= 0 && i64::from(t) - l <= i64::from(HOLD_TICKS) {
            held(&self.inputs[s][usize::try_from(l).expect("a ruled tick")])
        } else {
            NO_INPUT
        }
    }

    /// The predicted line's input of a human slot for a tick the referee has not ruled. The local slot's: the device
    /// input sent for it, and past the sent ones (ticks a wait or a jump left out) what the referee will substitute.
    fn guess(&self, s: usize, t: u32) -> InputState {
        if t >= self.end[s] {
            return NO_INPUT;
        }
        if s == self.local {
            if let Some(m) = self.mine.get(&t) {
                return *m;
            }
            let mut k = i64::from(t) - 1;
            while k > self.last[s] && k >= i64::from(t) - i64::from(HOLD_TICKS) {
                if let Some(before) = self.mine.get(&tick_of(k)) {
                    return held(before);
                }
                k -= 1;
            }
        }
        self.substitute(s, t)
    }

    /// A human slot's ticks that are ruled: all from a leaver's end on (empty inputs, NEVER), else its contiguous
    /// line.
    fn avail(&self, s: usize) -> u32 {
        if self.have[s] >= self.end[s] { NEVER } else { self.have[s] }
    }

    fn ruled_input(&self, s: usize, t: u32) -> InputState {
        if t >= self.end[s] { NO_INPUT } else { self.inputs[s][t as usize] }
    }

    /// `receive`: a packet from the referee: rulings of any human slot (this peer's own come back too) with the
    /// referee's clock, or another peer's checksum passed on. A packet that is not one (`packet_fits`) is dropped.
    pub fn receive(&mut self, bytes: &[u8], now: f64) {
        if !packet_fits(bytes) {
            return;
        }
        if bytes[0] == MSG_RULED {
            self.ruled_heard = Some(now);
            if let Some(h) = RuledHeader::read(bytes) {
                self.clock_sample(&h, now);
            }
            for run in runs(bytes) {
                let s = usize::from(run.head.slot);
                if !self.human(s) {
                    continue;
                }
                if run.head.end != NO_END {
                    self.ends(s, run.head.end);
                }
                let mut at = 0;
                for k in 0..u32::from(run.head.count) {
                    let sub = (run.head.mask >> k) & 1 == 1;
                    let input = if sub {
                        None
                    } else {
                        at += 1;
                        Some(decode(&run.inputs[at - 1]))
                    };
                    self.take(s, run.head.first + k, input);
                }
            }
        } else if bytes[0] == MSG_CHECKSUM
            && self.cfg.slots.get(usize::from(bytes[1])) == Some(&SlotKind::Remote)
            && let Some(c) = Checksum::read(bytes)
        {
            let (slot, tick, value) = (usize::from(c.slot), c.ticks, c.value);
            match self.sums.get(&tick) {
                None => {
                    // (not for a tick whose own hash is gone already, nor far past this peer's line: nothing would free
                    // it)
                    if tick <= self.confirmed
                        || u64::from(tick) > u64::from(self.confirmed) + u64::from(SUMS_KEPT * CHECKSUM_EVERY)
                    {
                        return;
                    }
                    // (one entry per slot and tick, and only for the ticks a checksum is made of: a peer cannot fill
                    // this)
                    if tick % CHECKSUM_EVERY != 0 {
                        return;
                    }
                    let l = self.their_sums.entry(tick).or_default();
                    match l.iter_mut().find(|x| x.0 == slot) {
                        Some(had) => had.1 = value,
                        None => l.push((slot, value)),
                    }
                }
                Some(&mine) if mine != value => {
                    self.desync.get_or_insert(Desync { tick, slot, mine, theirs: value });
                }
                Some(_) => {}
            }
        }
    }

    /// A ruled packet's header: the referee's clock, and this peer's round trip from the echo of its stamp.
    fn clock_sample(&mut self, h: &RuledHeader, now: f64) {
        if h.held == NO_STAMP {
            return;
        }
        let rtt = f64::from(to_uint32(round(now) - f64::from(h.stamp))) - f64::from(h.held);
        // (a stamp from before a wrap of the u32, or one the referee held for longer than its field: no measurement)
        if !(0.0..=60000.0).contains(&rtt) {
            return;
        }
        self.clock.push_back(ClockSample { at: now, rtt, offset: f64::from(h.clock) + rtt / 2.0 - now });
        while self.clock.front().is_some_and(|c| c.at < now - CLOCK_WINDOW_MS) {
            self.clock.pop_front();
        }
    }

    /// The clock's best sample (the shortest round trip of the window), `None` before the first.
    fn best(&self) -> Option<ClockSample> {
        let mut b: Option<ClockSample> = None;
        for x in &self.clock {
            if b.is_none_or(|b| x.rtt < b.rtt) {
                b = Some(*x);
            }
        }
        b
    }

    /// One ruling of a slot's stream (`None`: a substitution). Taken once (the first that comes: a repeat changes
    /// nothing), in order: one that came ahead of the ticks before it waits for them.
    fn take(&mut self, s: usize, t: u32, input: Ruled) {
        let have = self.have[s];
        if t < have || u64::from(t) >= u64::from(have) + u64::from(EARLY_TICKS) || t >= self.end[s] {
            return;
        }
        self.early[s].entry(t).or_insert(input);
        self.flush(s);
    }

    fn flush(&mut self, s: usize) {
        let me = s == self.local;
        while self.have[s] < self.end[s] {
            let t = self.have[s];
            let Some(input) = self.early[s].shift_remove(&t) else { break };
            // (my own input as I sent it: the shown line ran exactly that)
            let exact = me && input.is_some() && self.mine.contains_key(&t);
            // (an input of mine that came too late: see local_delay)
            if me && input.is_none() && self.mine.contains_key(&t) && t >= self.miss_from {
                self.local_delay = (self.local_delay + MISS_RAISE).min(MAX_LOCAL_DELAY);
                self.miss_from = NEVER;
                self.quiet = self.frame;
                self.stats.missed += 1;
            }
            let ruled = input.unwrap_or_else(|| self.substitute(s, t));
            debug_assert_eq!(self.inputs[s].len(), t as usize);
            self.inputs[s].push(ruled);
            if input.is_some() {
                self.last[s] = i64::from(t);
            } else {
                self.stats.substituted[s] += 1;
            }
            if me {
                self.mine.remove(&t);
            }
            self.have[s] = t + 1;
            if t < self.simulated && !exact {
                self.late = true;
                self.stats.late += 1;
            }
        }
    }

    /// The slot's player left: its stream ends before `end`, empty inputs from there on (`Referee::left`).
    fn ends(&mut self, s: usize, end: u32) {
        if self.end[s] != NEVER {
            return;
        }
        self.end[s] = end;
        self.early[s].retain(|&t, _| t < end);
        if s == self.local {
            self.mine.retain(|&t, _| t < end);
        }
        self.flush(s);
        if self.simulated > end {
            self.late = true;
        }
    }

    /// The referee's tick of the match at `now` (a fraction with it), `None` before its first ruled packet.
    fn referee_tick(&self, now: f64) -> Option<f64> {
        self.best().map(|b| (now + b.offset) / TICK_MS)
    }

    /// `advance`: one 60 Hz frame of this peer: `device` is the local input read now; `bots` by slot (a bot slot's
    /// controller thinks on the confirmed line). The World is stepped as the frame needs (back to the snapshot and on,
    /// confirmed and predicted ticks) and is left at the shown line. The frame's input packet (and any checksum) waits
    /// in the outbox.
    pub fn advance(&mut self, w: &mut World, bots: &mut [Option<BtBot>], device: InputState, now: f64) -> Advance {
        let me = self.local;
        let mut out = Vec::new();
        // the referee has ruled every human slot up to here
        let all = (0..self.cfg.slots.len()).filter(|&s| self.human(s)).map(|s| self.avail(s)).min().unwrap_or(NEVER);
        // The referee's clock (CATCHUP): far behind it (the game stood still, the way did), up to it in this one frame;
        // a little behind, now and then a second tick; ahead, now and then none.
        let sv = self.referee_tick(now);
        let frame = i64::from(self.frame);
        let ahead = sv.map_or(0.0, |sv| f64::from(self.frame) - sv);
        let jump = sv.is_some() && -ahead > f64::from(CATCHUP);
        let (mut skip, mut step) = (false, 1i64);
        if let Some(sv) = sv.filter(|_| jump) {
            self.pace = 0.0;
            #[allow(clippy::cast_possible_truncation, reason = "a tick of the match: floor of a finite clock")]
            let to = (sv.floor() as i64).min(frame + i64::from(MAX_JUMP));
            step = to - frame;
        } else if ahead > 1.0 || ahead < -1.0 {
            self.pace += PACE;
            if self.pace >= 1.0 {
                self.pace -= 1.0;
                if ahead > 1.0 {
                    skip = true;
                } else {
                    step = 2;
                }
            }
        } else {
            self.pace = 0.0;
        }
        let limit = i64::from(all) + i64::from(self.cfg.max_prediction);
        let target = if skip { frame } else { frame.max((frame + step).min(limit)) };
        // The input delay's floor, and its way down (local_delay above).
        let base = match self.rtt() {
            None => self.cfg.delay,
            Some(p) => {
                let one_way = (p / 2.0 / TICK_MS).ceil() - f64::from(DEADLINE) + 2.0;
                let round_trip = (p / TICK_MS).ceil() + 2.0 - f64::from(self.cfg.max_prediction);
                #[allow(clippy::cast_possible_truncation, reason = "a few ticks")]
                let need = one_way.max(round_trip).min(f64::from(MAX_LOCAL_DELAY)) as i64;
                tick_of(need.max(i64::from(self.cfg.delay)))
            }
        };
        if base > self.local_delay {
            self.local_delay = base;
            self.quiet = self.frame;
        } else if base == self.local_delay {
            self.quiet = self.frame;
        } else if self.frame - self.quiet >= LOWER_AFTER {
            self.local_delay -= 1;
            self.quiet = self.frame;
        }
        let stalled = target == frame && !skip;
        let caught = jump && target > frame + 1;
        if skip {
            self.stats.skips += 1;
        } else if stalled {
            self.stats.stalls += 1;
        } else {
            if caught {
                self.stats.catchups += 1;
                self.stats.jumped += u64::try_from(target - frame - 1).expect("a jump forward");
            } else if target > frame + 1 {
                self.stats.extra += 1;
            }
            // The device input applies local_delay ticks after the last tick of this frame. A peer behind the referee's
            // clock counts from the clock's tick and not from its own: from its own, every input of its way back to the
            // clock would reach the referee those ticks later and miss the deadline. Nothing is sent for a tick the
            // referee has ruled already, nor for one sent before (the frame after the delay was lowered): the ticks
            // sent only go up. The ticks in between (a raised delay's, a jump's) take the last input held and stay in
            // the run that is sent again until ruled, so a lost packet after a jump costs nothing; their misses are not
            // counted after a jump (they were late before they were sent). A gap of more than MAX_LOCAL_DELAY starts a
            // new run: the referee substitutes it.
            #[allow(clippy::cast_possible_truncation, reason = "a tick of the match: floor of a finite clock")]
            let clock = sv.map_or(0, |sv| sv.floor() as i64);
            let tick = target.max(clock) - 1 + i64::from(self.local_delay);
            if tick >= i64::from(self.have[me]) && tick > i64::from(self.sent) {
                let tick = tick_of(tick);
                match self.mine.get(&self.sent).copied() {
                    Some(prev) if tick - self.sent <= MAX_LOCAL_DELAY => {
                        for k in self.sent + 1..tick {
                            self.mine.insert(k, held(&prev));
                        }
                    }
                    _ => self.stream_from = tick,
                }
                if caught {
                    self.miss_from = self.miss_from.max(tick);
                }
                self.mine.insert(tick, decode(&encode(&canon_input(&device))));
                self.sent = tick;
                if self.miss_from == NEVER {
                    self.miss_from = tick;
                }
            }
        }
        let target = tick_of(target);
        let confirmable = all.min(target);

        let mut t0 = Instant::now();
        // this frame went back to the snapshot
        let mut back = false;
        if confirmable > self.confirmed || self.late {
            let reach = confirmable.min(self.simulated);
            let from;
            if self.holds(w, reach) {
                // The predicted line up to reach is the confirmed one: its ticks are taken as they ran, with what they
                // held back.
                for k in self.confirmed..reach {
                    if let Some(hook) = &mut self.on_ruled {
                        hook(k, &self.stepped[(k & self.ring_mask) as usize]);
                    }
                    if let Some(all) = self.held_back.remove(&k) {
                        self.show(w, &mut out, k, true, Some(all));
                    }
                }
                self.confirmed = reach;
                from = reach;
            } else {
                if self.simulated > self.snap_at {
                    assert!(self.ring.load(self.snap_slot, w), "the session's snapshot is in its ring");
                    self.stats.ms_restore += lap(&mut t0);
                    let depth = self.simulated - self.snap_at;
                    self.stats.rollbacks += 1;
                    self.stats.resimulated += u64::from(depth);
                    self.stats.deepest = self.stats.deepest.max(depth);
                }
                self.held_back.clear();
                self.cand_at = None;
                from = self.snap_at;
                self.simulated = from;
                back = true;
            }
            self.late = false;
            let first = from.max(self.simulated);
            let mut ins = Vec::with_capacity(self.cfg.slots.len());
            for k in first..confirmable {
                // the bots think here: on the confirmed state before tick k, as on every other peer
                ins.clear();
                for s in 0..self.cfg.slots.len() {
                    if self.cfg.slots[s] == SlotKind::Bot {
                        let me = w.players[s];
                        let i = bots.get_mut(s).and_then(Option::as_mut).map_or(NO_INPUT, |b| b.think(w, me));
                        let l = &mut self.inputs[s];
                        if (k as usize) < l.len() {
                            l[k as usize] = i;
                        } else {
                            debug_assert_eq!(l.len(), k as usize);
                            l.push(i);
                        }
                        ins.push(i);
                    } else {
                        ins.push(self.ruled_input(s, k));
                    }
                }
                w.step(&ins);
                // (a tick of the confirmed line stepped again from a snapshot behind it was given before)
                if k >= self.confirmed
                    && let Some(hook) = &mut self.on_ruled
                {
                    hook(k, &ins);
                }
                if let Some(hook) = &mut self.on_confirmed {
                    hook(k, w, &ins, &w.events);
                }
                self.show(w, &mut out, k, true, None);
                if (k + 1) % CHECKSUM_EVERY == 0 {
                    self.checksum(w, k + 1);
                }
            }
            self.stats.steps += u64::from(confirmable.saturating_sub(first));
            self.stats.ms_confirm += lap(&mut t0);
            self.confirmed = confirmable;
            self.simulated = self.simulated.max(confirmable);
            if self.simulated == confirmable && confirmable > self.snap_at {
                // (the World at the confirmed line, past the snapshot's: a new snapshot)
                self.ring.save(self.snap_slot, w);
                self.snap_at = confirmable;
                self.log.forget(confirmable - 1);
                self.cand_at = None;
                self.stats.ms_save += lap(&mut t0);
            } else if let Some(c) = self.cand_at.filter(|&c| c <= confirmable) {
                // (the predicted line held up to the candidate's tick: it is the confirmed line's state there)
                self.snap_slot ^= 1;
                self.snap_at = c;
                self.log.forget(c - 1);
                self.cand_at = None;
            }
        }
        self.stats.steps += u64::from(target.saturating_sub(self.simulated));
        // (a minion's controller, as the players' bots, thinks on the confirmed line alone: World::predicted)
        w.predicted = true;
        for k in self.simulated..target {
            let n = self.confirmed;
            let ins: Vec<InputState> = (0..self.cfg.slots.len())
                .map(|s| {
                    if self.cfg.slots[s] != SlotKind::Bot {
                        self.input_at(s, k)
                    } else if k < n {
                        self.inputs[s][k as usize]
                    } else if n > 0 {
                        held(&self.inputs[s][n as usize - 1])
                    } else {
                        NO_INPUT
                    }
                })
                .collect();
            w.step(&ins);
            let i = (k & self.ring_mask) as usize;
            self.stepped[i] = ins;
            self.stepped_at[i] = Some(k);
            self.held_back.remove(&k);
            self.show(w, &mut out, k, false, None);
        }
        w.predicted = false;
        self.stats.ms_predict += lap(&mut t0);
        // The next snapshot's candidate: the predicted line's newest state, while none is waiting (holds: the gates),
        // and only one that can be taken (within keep_behind of the snapshot); a frame that went back has saved a
        // snapshot already and takes its candidate on the next frame (a rollback frame's cost stays one save).
        if self.keep_behind > 0
            && !back
            && self.cand_at.is_none()
            && target > self.confirmed
            && target - self.snap_at <= self.keep_behind
            && w.pawn_ctl.ctl.is_empty()
        {
            self.ring.save(self.snap_slot ^ 1, w);
            self.cand_at = Some(target);
            self.stats.ms_save += lap(&mut t0);
        }
        self.simulated = target;
        self.frame = target;
        self.send_inputs(now);
        Advance { events: out, stalled, skipped: skip, caught }
    }

    /// The events of tick `tick` to show: `held` its whole list held back before, else the World's buffer (emptied).
    fn show(&mut self, w: &mut World, out: &mut Vec<Event>, tick: u32, fin: bool, held: Option<Vec<Event>>) {
        let from_world = held.is_none();
        let all = held.unwrap_or_else(|| std::mem::take(&mut w.events));
        let mut kept = None;
        if !fin && all.iter().any(confirmed_only) {
            kept = Some(all.iter().filter(|e| !confirmed_only(e)).cloned().collect::<Vec<_>>());
            self.held_back.insert(tick, all.clone());
        }
        let events = kept.as_deref().unwrap_or(&all);
        if fin
            && self.game_end.is_none()
            && events.iter().any(|e| matches!(e, Event::MatchEnd { phase: EndPhase::GameEnd, .. }))
        {
            self.game_end = Some(tick);
        }
        let taken = self.log.take(tick, events);
        out.extend(taken.gone.iter().filter_map(undo));
        out.extend(taken.fresh);
        if from_world {
            // (the World's buffer back, empty, with its room)
            let mut all = all;
            all.clear();
            w.events = all;
        }
    }

    /// This frame's input packet, also while the peer waits (it carries the acknowledgements and the clock stamp):
    /// its own inputs from the first one the referee has not ruled back, MAX_RUN of them at the most, so that a lost
    /// packet costs nothing. Further behind than that, a second packet carries the newest MAX_RUN.
    fn send_inputs(&mut self, now: f64) {
        let max_run = MAX_RUN as u32;
        let from = self.have[self.local].max(self.stream_from);
        let count = (self.sent + 1).saturating_sub(from);
        let newest = from.max((self.sent + 1).saturating_sub(max_run));
        if newest > from {
            self.send_run(newest, self.sent + 1 - newest, now);
        }
        self.send_run(from, count.min(max_run), now);
    }

    fn send_run(&mut self, from: u32, count: u32, now: f64) {
        let mut acks = [0u32; MAX_SLOTS];
        for (s, a) in acks.iter_mut().enumerate().take(self.cfg.slots.len()) {
            if self.human(s) {
                *a = if self.avail(s) == NEVER { ALL_TICKS } else { self.have[s] };
            }
        }
        let h = InputHeader {
            slot: u8::try_from(self.local).expect("a slot of the match"),
            first: from,
            count: u8::try_from(count).expect("a run of MAX_RUN at the most"),
            stamp: to_uint32(round(now)),
            acks,
        };
        let inputs: Vec<WireInput> =
            (from..from + count).map(|t| encode(self.mine.get(&t).expect("an input of mine not ruled yet"))).collect();
        self.outbox.push(input_packet(&h, &inputs));
    }

    /// The state hash after `ticks` confirmed ticks: kept, sent, and compared with what the others sent for it.
    fn checksum(&mut self, w: &World, ticks: u32) {
        let mut value = wire_hash(mvs_sim::state::hash_of(w));
        if self.spoil_at.is_some_and(|t| ticks >= t) {
            self.spoil_at = None;
            value ^= 1;
        }
        self.sums.insert(ticks, value);
        if let Some(old) = ticks.checked_sub(SUMS_KEPT * CHECKSUM_EVERY) {
            self.sums.remove(&old);
        }
        if let Some(hook) = &mut self.on_checksum {
            hook(ticks, value, w);
        }
        let slot = u8::try_from(self.local).expect("a slot of the match");
        self.outbox.push(Checksum { slot, ticks, value }.bytes().to_vec());
        for &(slot, theirs) in self.their_sums.get(&ticks).map_or(&[][..], Vec::as_slice) {
            if theirs != value {
                self.desync.get_or_insert(Desync { tick: ticks, slot, mine: value, theirs });
            }
        }
        // (and what is left of older ticks: a confirmed line that passed them in one frame)
        self.their_sums.retain(|&t, _| t > ticks);
    }

    /// (tests: `--online-desync-at`) The first own state hash made at or past `tick` has a bit flipped, as a world
    /// that went its own way would: the others find a desync there, and this peer finds theirs.
    #[doc(hidden)]
    pub fn spoil_hash_at(&mut self, tick: u32) {
        self.spoil_at = Some(tick);
    }

    /// `onChecksum`.
    pub fn on_checksum(&mut self, hook: ChecksumHook) {
        self.on_checksum = Some(hook);
    }

    /// Each tick the confirmed line steps, as [`ConfirmedHook`] has it.
    pub fn on_confirmed(&mut self, hook: ConfirmedHook) {
        self.on_confirmed = Some(hook);
    }

    /// Each tick of the confirmed line, in order, whether stepped again or taken as the predicted line ran it, as
    /// [`RuledHook`] has it.
    pub fn on_ruled(&mut self, hook: RuledHook) {
        self.on_ruled = Some(hook);
    }

    /// The packets to the referee since the last call, in order.
    pub fn packets(&mut self) -> std::vec::Drain<'_, Vec<u8>> {
        self.outbox.drain(..)
    }

    /// Ticks of the shown (predicted) line: the next tick to simulate.
    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// When the last ruled packet came (ms of the caller's clock), None before the first.
    pub fn ruled_at(&self) -> Option<f64> {
        self.ruled_heard
    }

    /// Ticks of the confirmed line.
    pub fn confirmed(&self) -> u32 {
        self.confirmed
    }

    /// The first state hash another peer disagreed with.
    pub fn desync(&self) -> Option<Desync> {
        self.desync
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// `localDelay`: ticks between reading the local device and the tick the input is sent for.
    pub fn local_delay(&self) -> u32 {
        self.local_delay
    }

    /// `gameEnd`: the tick of the confirmed line that ended the match (matchEnd, GameEnd).
    pub fn game_end(&self) -> Option<u32> {
        self.game_end
    }

    /// `ahead`: ticks this peer's shown line is ahead of the referee's clock at `now` (below 0: behind; 0 while the
    /// clock is not known).
    pub fn ahead(&self, now: f64) -> f64 {
        self.referee_tick(now).map_or(0.0, |sv| f64::from(self.frame) - sv)
    }

    /// `rtt`: ms to the referee and back, the shortest of the clock's window; `None` before the first.
    pub fn rtt(&self) -> Option<f64> {
        self.best().map(|b| b.rtt)
    }

    /// `slotInfo`.
    pub fn slot_info(&self, slot: usize) -> SlotInfo {
        let ruled = if slot < self.cfg.slots.len() { self.avail(slot).min(self.frame) } else { self.frame };
        SlotInfo { missing: self.frame - ruled, substituted: self.stats.substituted.get(slot).copied().unwrap_or(0) }
    }

    /// `predicted`: ticks the shown line is ahead of the confirmed one.
    pub fn predicted(&self) -> u32 {
        self.frame - self.confirmed
    }

    /// `endOf`: the tick a slot's stream ends at (its player left), `None` while it has not.
    pub fn end_of(&self, slot: usize) -> Option<u32> {
        self.end.get(slot).copied().filter(|&e| e != NEVER)
    }

    pub fn slot_kinds(&self) -> &[SlotKind] {
        &self.cfg.slots
    }
}
