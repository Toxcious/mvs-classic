//! The Referee against the tape of the oracle Referee (`referee-tape.json`): the same calls at the same clocks, up to
//! the first slot the oracle found silent (the oracle's `silent(slot)` is this referee's `Away`; at that call the
//! sends to the slot itself aside: an away peer waits for the CLOCK_MS beat), every checksum passed on the same, in
//! order, and the same ruled streams: each ruled tick a peer is sent the oracle's input or substitution, and sent it
//! no later than the oracle sent it first; the stream ends the oracle's, each sent a peer no later than the oracle's;
//! a ruled packet's clock and stamp echo the oracle's of the same call. The packets themselves differ: the oracle sent a peer every tick it had not
//! acknowledged at every pump, the port sends a ruled tick once and again only past the peer's resend timeout. From
//! the first silent on the rule is the port's own (away, back, gone) and the tape's calls are run on unchecked but
//! for the news. A test that finds no tape says so and passes.
//!
//! The port's rule itself is checked on peers of this file's own ([`Peer`]), each taking what the referee sends and
//! acknowledging it, whose way is lost for a time.

use std::collections::BTreeMap;

use mvs_protocol::match_wire::{
    ALL_TICKS, CLOCK_MS, INPUT_DELAY, InputHeader, MAX_PACKET, MAX_SLOTS, MSG_CHECKSUM, MSG_INPUT, MSG_RULED, NO_END,
    RESUME_MS, RuledHeader, SILENT_MS, TICK_MS, WireInput, input_packet, packet_fits, runs,
};
use mvs_protocol::referee::{
    BEHIND_TICKS, CATCH_UP_PACKETS, GONE_TICKS, Referee, RefereeOut, SlotNews, catch_up_packets,
};
use mvs_protocol::vectors;
use serde_json::{Value, json};

/// The oracle's slot count: the tape's input headers are of four.
const ORACLE_SLOTS: usize = 4;

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).expect("hex")).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[derive(Default)]
struct Tape(Vec<Value>);

impl RefereeOut for Tape {
    fn send(&mut self, slot: usize, bytes: &[u8]) {
        self.0.push(json!({ "send": slot, "bytes": hex(bytes) }));
    }

    fn news(&mut self, slot: usize, n: SlotNews) {
        // (the oracle's silent is the first news of a slot: away)
        let n = match n {
            SlotNews::Away => "silent",
            SlotNews::Back => "back",
            SlotNews::Gone => "gone",
        };
        self.0.push(json!({ n: slot }));
    }
}

/// A ruled tick a packet carries: its stream, its tick, the input (None: substituted) and the stream's end.
type Ticked = (usize, u32, Option<WireInput>, u32);

/// The ruled ticks of a ruled packet.
fn ticked(b: &[u8]) -> Vec<Ticked> {
    let mut out = Vec::new();
    for run in runs(b) {
        let mut at = 0;
        for k in 0..u32::from(run.head.count) {
            let i = if (run.head.mask >> k) & 1 == 1 {
                None
            } else {
                at += 1;
                Some(run.inputs[at - 1])
            };
            out.push((usize::from(run.head.slot), run.head.first + k, i, run.head.end));
        }
        if run.head.count == 0 {
            out.push((usize::from(run.head.slot), u32::MAX, None, run.head.end));
        }
    }
    out
}

/// The sends of a call that are compared: up to the parting (the first call with a silent), its sends to the silent
/// slots aside.
fn compared(out: &[Value], want: &[Value]) -> Vec<(usize, Vec<u8>)> {
    let silents: Vec<&Value> = want.iter().filter_map(|w| w.get("silent")).collect();
    out.iter()
        .filter(|o| o.get("bytes").is_some() && !silents.contains(&&o["send"]))
        .map(|o| (usize::try_from(o["send"].as_u64().unwrap()).unwrap(), unhex(o["bytes"].as_str().unwrap())))
        .collect()
}

/// An oracle input packet with the port's sequence byte after its acks: 0 (the oracle's had none, and the referee reads
/// no hole in a run of equal ones); any other packet, or one too short for the acks, as it is.
fn with_seq(mut b: Vec<u8>) -> Vec<u8> {
    if b.first() == Some(&MSG_INPUT) && b.len() >= 11 + 4 * ORACLE_SLOTS {
        b.insert(11 + 4 * ORACLE_SLOTS, 0);
    }
    b
}

#[test]
fn referee_tape() {
    let Some(v) = vectors::read("referee-tape.json") else { return };
    let (mut calls, mut outs) = (0usize, 0usize);
    for s in v["scenarios"].as_array().unwrap() {
        let name = s["name"].as_str().unwrap();
        // (a slot below 0 is passed over by the oracle; usize::MAX is past MAX_SLOTS)
        let humans: Vec<usize> = s["humans"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| usize::try_from(h.as_i64().unwrap()).unwrap_or(usize::MAX))
            .collect();
        let mut r = Referee::new(ORACLE_SLOTS, &humans, s["tick0"].as_f64().unwrap());
        // (past the oracle's first silent the rules part: only the news are kept, by slot)
        let mut parted = false;
        let mut news: BTreeMap<u64, Vec<(String, f64)>> = BTreeMap::new();
        let mut ts_silent: BTreeMap<u64, Vec<f64>> = BTreeMap::new();
        // (the oracle's ruled streams up to the parting: by stream and tick the input, by stream the end, and by peer,
        // stream and tick the call that first sent it)
        let mut ruled: BTreeMap<(usize, u32), Option<WireInput>> = BTreeMap::new();
        let mut ends: BTreeMap<usize, u32> = BTreeMap::new();
        let mut first: BTreeMap<(usize, usize, u32), usize> = BTreeMap::new();
        // (by peer and stream the call that first sent it the stream's end)
        let mut end_first: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        for (n, c) in s["calls"].as_array().unwrap().iter().enumerate() {
            let Some(want) = c["out"].as_array() else { continue };
            for (x, b) in compared(want, want) {
                if b[0] != MSG_RULED {
                    continue;
                }
                for (y, t, i, end) in ticked(&b) {
                    if end != NO_END {
                        assert_eq!(*ends.entry(y).or_insert(end), end, "{name}: call {n}: two ends of slot {y}");
                        end_first.entry((x, y)).or_insert(n);
                    }
                    if t != u32::MAX {
                        assert_eq!(*ruled.entry((y, t)).or_insert(i), i, "{name}: call {n}: slot {y} tick {t} twice");
                        first.entry((x, y, t)).or_insert(n);
                    }
                }
            }
            if want.iter().any(|w| w.get("silent").is_some()) {
                break;
            }
        }
        let mut got_first: BTreeMap<(usize, usize, u32), usize> = BTreeMap::new();
        let mut got_end: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        for (n, c) in s["calls"].as_array().unwrap().iter().enumerate() {
            let now = c["now"].as_f64();
            let slot = c["slot"].as_u64().map(|x| usize::try_from(x).unwrap());
            let mut out = Tape::default();
            match c["call"].as_str().unwrap() {
                "receive" => {
                    r.receive(slot.unwrap(), &with_seq(unhex(c["bytes"].as_str().unwrap())), now.unwrap(), &mut out)
                }
                "pump" => r.pump(now.unwrap(), &mut out),
                "left" => r.left(slot.unwrap(), now.unwrap()),
                "tick" => {
                    assert_eq!(r.tick(now.unwrap()), c["value"].as_i64().unwrap(), "{name}: call {n}: tick");
                    calls += 1;
                    continue;
                }
                "kept" => {
                    if !parted {
                        assert_eq!(
                            r.kept(slot.unwrap()) as u64,
                            c["value"].as_u64().unwrap(),
                            "{name}: call {n}: kept"
                        );
                    }
                    calls += 1;
                    continue;
                }
                other => panic!("{name}: call {n}: {other}"),
            }
            let want = c["out"].as_array().unwrap();
            for w in want {
                if let Some(x) = w["silent"].as_u64() {
                    ts_silent.entry(x).or_default().push(now.unwrap());
                }
            }
            for o in &out.0 {
                if let Some((k, x)) =
                    o.as_object().and_then(|m| m.iter().find(|(k, _)| matches!(k.as_str(), "silent" | "back" | "gone")))
                {
                    news.entry(x.as_u64().unwrap()).or_default().push((k.clone(), now.unwrap()));
                }
                if let Some(b) = o["bytes"].as_str() {
                    assert!(packet_fits(&unhex(b), ORACLE_SLOTS), "{name}: call {n}: a packet that does not fit");
                }
            }
            if !parted {
                // (at the parting call the slot told silent is sent no more at once: an away peer waits for the
                // CLOCK_MS beat; what that call sends the others is compared)
                let (got, wanted) = (compared(&out.0, want), compared(want, want));
                let at = format!("{name}: call {n} ({}) at {:?}", c["call"], now);
                let sums = |l: &[(usize, Vec<u8>)]| -> Vec<(usize, Vec<u8>)> {
                    l.iter().filter(|x| x.1[0] == MSG_CHECKSUM).cloned().collect()
                };
                assert_eq!(sums(&got), sums(&wanted), "{at}: the checksums passed on");
                for (x, b) in got.iter().filter(|x| x.1[0] == MSG_RULED) {
                    let h = RuledHeader::read(b).unwrap();
                    if let Some((_, w)) = wanted.iter().find(|w| w.0 == *x && w.1[0] == MSG_RULED) {
                        let o = RuledHeader::read(w).unwrap();
                        assert_eq!((h.clock, h.stamp, h.held), (o.clock, o.stamp, o.held), "{at}: slot {x}'s header");
                    }
                    for (y, t, i, end) in ticked(b) {
                        if end != NO_END {
                            assert_eq!(ends.get(&y), Some(&end), "{at}: slot {y}'s end to slot {x}");
                            got_end.entry((*x, y)).or_insert(n);
                        }
                        if t == u32::MAX {
                            continue;
                        }
                        // (a tick the oracle sent nobody before the parting is not known: the port may send it sooner)
                        if let Some(w) = ruled.get(&(y, t)) {
                            assert_eq!(&i, w, "{at}: slot {y} tick {t} to slot {x}");
                            outs += 1;
                        }
                        got_first.entry((*x, y, t)).or_insert(n);
                    }
                }
                for (k, _) in first.iter().filter(|(_, m)| **m == n) {
                    assert!(got_first.contains_key(k), "{at}: slot {} tick {} not sent slot {} yet", k.1, k.2, k.0);
                }
                for (k, _) in end_first.iter().filter(|(_, m)| **m == n) {
                    assert!(got_end.contains_key(k), "{at}: slot {}'s end not sent slot {} yet", k.1, k.0);
                }
                parted = want.iter().any(|w| w.get("silent").is_some());
            }
            calls += 1;
        }
        // (each slot the oracle found silent is away at the same call; the news of a slot alternate away / back, gone
        // last)
        for (slot, at) in &ts_silent {
            let first = news.get(slot).and_then(|l| l.first()).cloned();
            assert_eq!(first, Some(("silent".to_owned(), at[0])), "{name}: slot {slot}: its first news");
        }
        for (slot, l) in &news {
            for (k, (what, _)) in l.iter().enumerate() {
                let want = if what == "gone" {
                    assert_eq!(k + 1, l.len(), "{name}: slot {slot}: news after gone: {l:?}");
                    continue;
                } else if k % 2 == 0 {
                    "silent"
                } else {
                    "back"
                };
                assert_eq!(what, want, "{name}: slot {slot}: its news {l:?}");
            }
        }
        eprintln!("{name}: news past the parting {news:?}");
    }
    eprintln!("referee-tape: {calls} calls, {outs} ruled ticks sent compared");
}

// ---------- the port's rule on peers of its own ----------

/// What a pump or a receive sent.
#[derive(Default)]
struct Out {
    sends: Vec<(usize, Vec<u8>)>,
    news: Vec<(usize, SlotNews)>,
}

impl RefereeOut for Out {
    fn send(&mut self, slot: usize, bytes: &[u8]) {
        self.sends.push((slot, bytes.to_vec()));
    }

    fn news(&mut self, slot: usize, n: SlotNews) {
        self.news.push((slot, n));
    }
}

/// A peer of a match of `n` slots that sends an input a tick (for the tick its clock is at, the referee's) with what it
/// has of each stream, and takes every ruled packet: no rollback, only the streams.
struct Peer {
    slot: usize,
    n: usize,
    /// The input packets it sent in pairs (wrapping).
    seq: u8,
    /// Per slot the first tick it lacks, and the ticks that came ahead of it.
    have: Vec<u32>,
    early: Vec<BTreeMap<u32, ()>>,
}

impl Peer {
    fn new(slot: usize, n: usize) -> Peer {
        Peer { slot, n, seq: 0, have: vec![INPUT_DELAY; n], early: vec![BTreeMap::new(); n] }
    }

    fn take(&mut self, b: &[u8]) {
        assert!(b.len() <= MAX_PACKET && packet_fits(b, self.n), "a ruled packet that does not fit");
        assert_eq!(b[0], MSG_RULED);
        for run in runs(b) {
            let s = usize::from(run.head.slot);
            for k in 0..u32::from(run.head.count) {
                let t = run.head.first + k;
                if t >= self.have[s] {
                    self.early[s].insert(t, ());
                }
            }
            while self.early[s].remove(&self.have[s]).is_some() {
                self.have[s] += 1;
            }
            if run.head.end != NO_END && self.have[s] >= run.head.end {
                self.have[s] = ALL_TICKS;
            }
        }
    }

    fn packet(&self, tick: i64, now: f64, humans: &[usize]) -> Vec<u8> {
        let mut acks = [0u32; MAX_SLOTS];
        for &h in humans {
            acks[h] = self.have[h];
        }
        let first = u32::try_from(tick.max(0)).unwrap() + INPUT_DELAY;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a test's clock")]
        let stamp = now as u32;
        let h = InputHeader { slot: self.slot as u8, first, count: 1, stamp, acks, seq: 0 };
        let i: WireInput = [self.slot as u8 + 1; 22];
        input_packet(&h, self.n, &[i])
    }

    /// An input packet of the last two ticks (the tick its clock is at and the one before), its sequence byte the next.
    fn pair(&mut self, tick: i64, now: f64, humans: &[usize]) -> Vec<u8> {
        let mut acks = [0u32; MAX_SLOTS];
        for &h in humans {
            acks[h] = self.have[h];
        }
        let first = u32::try_from((tick - 1).max(0)).unwrap() + INPUT_DELAY;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a test's clock")]
        let stamp = now as u32;
        let h = InputHeader { slot: self.slot as u8, first, count: 2, stamp, acks, seq: self.seq };
        self.seq = self.seq.wrapping_add(1);
        let i: WireInput = [self.slot as u8 + 1; 22];
        input_packet(&h, self.n, &[i, i])
    }
}

/// How a lost way went: the news with their tick, the most packets one call sent one peer, the most ticks kept, and
/// the lost peer's ticks behind the referee's line once it was back (by ticks after the way came back).
#[derive(Debug, Default)]
struct Run {
    news: Vec<(usize, SlotNews, i64)>,
    most_sent: usize,
    most_kept: usize,
    level_after: Option<i64>,
    /// The most runs of one ruled packet.
    most_runs: usize,
    /// Per tick the bytes sent each slot.
    bytes: Vec<[usize; MAX_SLOTS]>,
}

impl Run {
    /// The bytes a second sent `slot` over ticks `from .. to`.
    fn rate(&self, slot: usize, from: i64, to: i64) -> f64 {
        let (a, b) = (usize::try_from(from).unwrap(), usize::try_from(to).unwrap());
        let n: usize = self.bytes[a..b].iter().map(|x| x[slot]).sum();
        #[allow(clippy::cast_precision_loss, reason = "a test's counts")]
        let r = n as f64 / ((to - from) as f64 * TICK_MS / 1000.0);
        r
    }
}

/// `peers` peers (every slot of the match a human's) 1 tick (one way) from the referee; peer `lost`'s way lost both ways (`deaf`: only the way to it)
/// over ticks `from .. to`; the match runs `ticks`. A gone peer is taken out (Referee::left).
fn lost_way(peers: usize, lost: usize, from: i64, to: i64, deaf: bool, ticks: i64) -> Run {
    lost_way_pumped(peers, lost, from, to, deaf, ticks, false)
}

/// `lost_way`; `each`: a pump after every receive as well (as the relay's core and the in-process host do).
fn lost_way_pumped(peers: usize, lost: usize, from: i64, to: i64, deaf: bool, ticks: i64, each: bool) -> Run {
    lost_way_muted(peers, lost, from, to, deaf, ticks, each, &[])
}

/// `lost_way_pumped`; `mute`: peers that never send (their streams all substituted, away once SILENT_MS passed).
#[allow(clippy::too_many_arguments, reason = "a test's knobs")]
fn lost_way_muted(
    peers: usize,
    lost: usize,
    from: i64,
    to: i64,
    deaf: bool,
    ticks: i64,
    each: bool,
    mute: &[usize],
) -> Run {
    let humans: Vec<usize> = (0..peers).collect();
    let mut r = Referee::new(peers, &humans, 0.0);
    let mut ps: Vec<Peer> = humans.iter().map(|&s| Peer::new(s, peers)).collect();
    let mut run = Run::default();
    // (packets on their way: due tick, to the referee (true) or to a peer, slot, bytes)
    let mut way: Vec<(i64, bool, usize, Vec<u8>)> = Vec::new();
    let mut out_of: Vec<bool> = vec![false; peers];
    for tick in 0..ticks {
        let now = tick as f64 * TICK_MS + 1.0;
        let cut = |s: usize, up: bool| s == lost && tick >= from && tick < to && !(deaf && up);
        // what has come
        let due: Vec<_> = way.iter().filter(|p| p.0 <= tick).cloned().collect();
        way.retain(|p| p.0 > tick);
        let mut out = Out::default();
        let mut calls: Vec<Out> = Vec::new();
        for (_, up, s, b) in due {
            if up {
                r.receive(s, &b, now, &mut out);
                if each {
                    r.pump(now, &mut out);
                    calls.push(std::mem::take(&mut out));
                }
            } else if !out_of[s] {
                ps[s].take(&b);
            }
        }
        r.pump(now, &mut out);
        calls.push(out);
        let mut out = Out::default();
        let mut bytes = [0usize; MAX_SLOTS];
        for c in calls {
            let mut per: [usize; MAX_SLOTS] = [0; MAX_SLOTS];
            for (s, b) in c.sends {
                assert!(b.len() <= MAX_PACKET && packet_fits(&b, peers), "a packet to slot {s} that does not fit");
                per[s] += 1;
                bytes[s] += b.len();
                run.most_runs = run.most_runs.max(runs(&b).count());
                if !cut(s, false) {
                    way.push((tick + 1, false, s, b));
                }
            }
            run.most_sent = run.most_sent.max(per.into_iter().max().unwrap_or(0));
            out.news.extend(c.news);
        }
        run.bytes.push(bytes);
        for (s, n) in out.news {
            run.news.push((s, n, tick));
            if n == SlotNews::Gone {
                out_of[s] = true;
                r.left(s, now);
            }
        }
        for &h in &humans {
            run.most_kept = run.most_kept.max(r.kept(h));
        }
        for p in &ps {
            if !out_of[p.slot] && !cut(p.slot, true) && !mute.contains(&p.slot) {
                way.push((tick + 1, true, p.slot, p.packet(r.tick(now), now, &humans)));
            }
        }
        // (level: the lost peer has every stream up to a few ticks of the referee's line)
        if tick >= to && run.level_after.is_none() && !out_of[lost] {
            let line = r.tick(now) - 8;
            if humans.iter().all(|&h| i64::from(ps[lost].have[h]) >= line) {
                run.level_after = Some(tick - to);
            }
        }
    }
    run
}

fn kinds(run: &Run, slot: usize) -> Vec<SlotNews> {
    run.news.iter().filter(|n| n.0 == slot).map(|n| n.1).collect()
}

fn at(run: &Run, slot: usize, k: SlotNews) -> i64 {
    run.news.iter().find(|n| n.0 == slot && n.1 == k).map(|n| n.2).expect("the news")
}

#[allow(clippy::cast_possible_truncation, reason = "whole ticks of a few ms constants")]
fn ticks_of(ms: f64) -> i64 {
    (ms / TICK_MS).ceil() as i64
}

/// `peers` peers 1 tick (one way) from the referee, no packet lost, each sending an input packet a tick, or (`pairs`)
/// one of two ticks every other tick (a game drawing at 30 fps: its stamps 33 ms apart): whether any was counted as
/// losing packets after its first 300 ms, and the bytes a second sent each from then on.
fn paired(peers: usize, pairs: bool, ticks: i64) -> (bool, f64) {
    let humans: Vec<usize> = (0..peers).collect();
    let mut r = Referee::new(peers, &humans, 0.0);
    let mut ps: Vec<Peer> = humans.iter().map(|&s| Peer::new(s, peers)).collect();
    let mut way: Vec<(i64, bool, usize, Vec<u8>)> = Vec::new();
    let (mut losing, mut bytes) = (false, 0usize);
    let settled = 30;
    for tick in 0..ticks {
        let now = tick as f64 * TICK_MS + 1.0;
        let due: Vec<_> = way.iter().filter(|p| p.0 <= tick).cloned().collect();
        way.retain(|p| p.0 > tick);
        let mut out = Out::default();
        for (_, up, s, b) in due {
            if up {
                r.receive(s, &b, now, &mut out);
                r.pump(now, &mut out);
            } else {
                ps[s].take(&b);
            }
        }
        r.pump(now, &mut out);
        for (s, b) in out.sends {
            if tick >= settled {
                bytes += b.len();
            }
            way.push((tick + 1, false, s, b));
        }
        if tick >= settled {
            losing |= humans.iter().any(|&h| r.losing(h, now));
        }
        if !pairs || tick % 2 == 1 {
            for p in &mut ps {
                let b = if pairs { p.pair(r.tick(now), now, &humans) } else { p.packet(r.tick(now), now, &humans) };
                way.push((tick + 1, true, p.slot, b));
            }
        }
    }
    #[allow(clippy::cast_precision_loss, reason = "a test's counts")]
    let rate = bytes as f64 / peers as f64 / ((ticks - settled) as f64 * TICK_MS / 1000.0);
    (losing, rate)
}

/// A game that runs its ticks in pairs (drawing under 40 fps) sends a packet every 33 ms with no hole in its sequence:
/// it is not taken as losing packets, and it is sent no more than a game that sends one a tick.
#[test]
fn a_peer_sending_its_ticks_in_pairs_is_not_losing() {
    for peers in [2, 8] {
        let (single_losing, single) = paired(peers, false, 900);
        let (pair_losing, pair) = paired(peers, true, 900);
        eprintln!("{peers} peers: bytes a second to each, a packet a tick {single:.0}, two ticks a packet {pair:.0}");
        assert!(!single_losing && !pair_losing, "{peers} peers: counted as losing packets");
        assert!(pair <= single * 1.1, "{peers} peers: {pair:.0} B/s in pairs, {single:.0} one a tick");
    }
}

#[test]
fn a_way_lost_3_s_is_no_news() {
    let run = lost_way(3, 1, 300, 480, false, 900);
    eprintln!("{run:?}");
    assert!(run.news.is_empty(), "{run:?}");
    assert!(run.level_after.is_some_and(|t| t <= 20), "{run:?}");
}

#[test]
fn a_way_lost_10_s_is_away_then_back_and_level() {
    let run = lost_way(3, 1, 300, 900, false, 1500);
    eprintln!("{run:?}");
    assert_eq!(kinds(&run, 1), [SlotNews::Away, SlotNews::Back], "{run:?}");
    // (away SILENT_MS after it was last heard)
    let away = at(&run, 1, SlotNews::Away) - 300;
    assert!((ticks_of(SILENT_MS) - 2..=ticks_of(SILENT_MS) + 2).contains(&away), "{run:?}");
    assert!(at(&run, 1, SlotNews::Back) - 900 <= 20, "{run:?}");
    assert!(run.level_after.is_some_and(|t| t <= 20), "{run:?}");
    assert!(run.most_sent <= CATCH_UP_PACKETS, "{run:?}");
    assert!(kinds(&run, 0).is_empty() && kinds(&run, 2).is_empty(), "{run:?}");
}

#[test]
fn a_way_lost_19_s_is_away_then_back_and_level() {
    let run = lost_way(4, 2, 300, 1440, false, 2000);
    eprintln!("{run:?}");
    assert_eq!(kinds(&run, 2), [SlotNews::Away, SlotNews::Back], "{run:?}");
    assert!(run.level_after.is_some_and(|t| t <= 30), "{run:?}");
    assert!(run.most_sent <= CATCH_UP_PACKETS, "{run:?}");
    // (the streams kept for it the while: up to GONE_TICKS)
    assert!(run.most_kept <= GONE_TICKS as usize + 10, "{run:?}");
    assert!(run.most_kept > BEHIND_TICKS as usize, "{run:?}");
}

#[test]
fn a_way_lost_21_s_is_away_then_gone() {
    let run = lost_way(2, 0, 300, 1560, false, 2000);
    eprintln!("{run:?}");
    assert_eq!(kinds(&run, 0), [SlotNews::Away, SlotNews::Gone], "{run:?}");
    // (gone RESUME_MS after it was last heard: its last packet came at tick 300)
    let gone = at(&run, 0, SlotNews::Gone) - 300;
    assert!((ticks_of(RESUME_MS) - 2..=ticks_of(RESUME_MS) + 2).contains(&gone), "{run:?}");
    assert!(kinds(&run, 1).is_empty(), "{run:?}");
}

#[test]
fn a_deaf_peer_is_away_then_gone_by_its_acknowledgements() {
    let run = lost_way(2, 1, 300, 5000, true, 2000);
    eprintln!("{run:?}");
    assert_eq!(kinds(&run, 1), [SlotNews::Away, SlotNews::Gone], "{run:?}");
    let (away, gone) = (at(&run, 1, SlotNews::Away) - 300, at(&run, 1, SlotNews::Gone) - 300);
    assert!((i64::from(BEHIND_TICKS) - 12..=i64::from(BEHIND_TICKS) + 12).contains(&away), "{run:?}");
    assert!((i64::from(GONE_TICKS) - 12..=i64::from(GONE_TICKS) + 12).contains(&gone), "{run:?}");
    // (a deaf peer that hears again is sent the gap's runs too: the cap holds)
    assert!(run.most_sent <= CATCH_UP_PACKETS, "{run:?}");
}

/// Pumped after every receive: an away peer's link is sent no more a second than one that had just fallen silent (the
/// referee before away: what it sends the first SILENT_MS), a healthy peer what it is sent with no peer away, and a
/// peer back from away catches up all the same.
#[test]
fn an_away_peer_is_sent_little() {
    let (from, to, ticks) = (300, 1440, 2000);
    let quiet = lost_way_pumped(4, 2, from, from, false, ticks, true);
    let run = lost_way_pumped(4, 2, from, to, false, ticks, true);
    let away = at(&run, 2, SlotNews::Away);
    let silent = run.rate(2, from + 10, away - 10);
    let away_rate = run.rate(2, away + 10, to);
    // (a healthy peer while the other is silent, the referee's rule before away, and while it is away: the same
    // packets, as many receives; with no peer lost, more receives and so more pumps)
    let (healthy_silent, healthy_away) = (run.rate(0, from + 10, away - 10), run.rate(0, away + 10, to));
    eprintln!(
        "bytes a second: to the silent peer {silent:.0}, to it away {away_rate:.0}; to a healthy peer {healthy_silent:.0} \
         then {healthy_away:.0} (no peer lost: {:.0}); level after {:?}",
        quiet.rate(0, from, to),
        run.level_after
    );
    assert!(away_rate <= silent, "away {away_rate:.0} B/s, silent {silent:.0} B/s");
    // (one packet of the oldest runs a CLOCK_MS)
    assert!(away_rate <= 4.0 * MAX_PACKET as f64 * 1000.0 / CLOCK_MS, "away {away_rate:.0} B/s");
    assert!(
        (healthy_away - healthy_silent).abs() <= healthy_silent * 0.1,
        "healthy {healthy_away:.0} B/s, {healthy_silent:.0} B/s"
    );
    assert_eq!(kinds(&run, 2), [SlotNews::Away, SlotNews::Back], "{run:?}");
    assert!(run.level_after.is_some_and(|t| t <= 30), "{run:?}");
    assert!(run.most_sent <= CATCH_UP_PACKETS, "{run:?}");
}

/// Pumped after every receive: a deaf peer still heard from (away by its acknowledgements) is sent a catch-up burst a
/// CLOCK_MS, not one a pump.
#[test]
fn a_deaf_peer_is_sent_a_burst_a_beat() {
    let run = lost_way_pumped(4, 2, 300, 5000, true, 1700, true);
    let away = at(&run, 2, SlotNews::Away);
    let deaf = run.rate(2, away + 10, 1490);
    let silent = run.rate(2, 310, away - 10);
    eprintln!("bytes a second to the deaf peer: before away {silent:.0}, away {deaf:.0}");
    assert!(deaf <= silent, "deaf {deaf:.0} B/s, before away {silent:.0} B/s");
    // (CATCH_UP_PACKETS full packets a CLOCK_MS at the most)
    #[allow(clippy::cast_precision_loss, reason = "small constants")]
    let most = (CATCH_UP_PACKETS * MAX_PACKET) as f64 * 1000.0 / CLOCK_MS;
    assert!(deaf <= most, "deaf {deaf:.0} B/s, {most:.0} at the most");
}

/// A match of two or three slots keeps the four-slot run cap: a peer back from 10 s away, catching up on streams of
/// substituted ticks (a run header each, no inputs: the muted peers' and its own), is sent packets of 8 runs, as before
/// the cap followed the match's slots.
#[test]
fn two_and_three_slots_keep_the_four_slot_run_cap() {
    for peers in [2, 3] {
        let run = lost_way_muted(peers, 1, 300, 900, false, 1100, false, &[0, 2][..peers - 1]);
        eprintln!(
            "{peers} slots: most runs {}, most sent {}, level after {:?}",
            run.most_runs, run.most_sent, run.level_after
        );
        assert_eq!(run.most_runs, 8, "{peers} slots: {run:?}");
        assert!(run.most_sent <= CATCH_UP_PACKETS, "{peers} slots: {run:?}");
        assert!(run.level_after.is_some_and(|t| t <= 20), "{peers} slots: {run:?}");
    }
}

// ---- matches of six and eight slots

#[test]
fn six_slots_a_way_lost_10_s_is_away_then_back_and_level() {
    let run = lost_way(6, 4, 300, 900, false, 1500);
    eprintln!("{run:?}");
    assert_eq!(kinds(&run, 4), [SlotNews::Away, SlotNews::Back], "{run:?}");
    assert!(run.level_after.is_some_and(|t| t <= 20), "{run:?}");
    assert!(run.most_sent <= catch_up_packets(6), "{run:?}");
    assert!((0..6).filter(|&s| s != 4).all(|s| kinds(&run, s).is_empty()), "{run:?}");
}

#[test]
fn eight_slots_a_way_lost_3_s_is_no_news() {
    let run = lost_way(8, 6, 300, 480, false, 900);
    eprintln!("{run:?}");
    assert!(run.news.is_empty(), "{run:?}");
    assert!(run.level_after.is_some_and(|t| t <= 20), "{run:?}");
}

/// The ruled packets' worst case: eight slots, every stream but the lost one full of inputs, a peer back after 19 s
/// away catching up on all eight (a burst of catch_up_packets a beat, the oldest runs between). Every packet the
/// referee sends fits (MAX_PACKET bytes, max_runs runs: `lost_way` and `Peer::take` check each), and it is level
/// within a second.
#[test]
fn eight_slots_a_way_lost_19_s_catches_up_in_packets_that_fit() {
    let run = lost_way(8, 7, 300, 1440, false, 2000);
    eprintln!("{run:?}");
    assert_eq!(kinds(&run, 7), [SlotNews::Away, SlotNews::Back], "{run:?}");
    assert!(run.level_after.is_some_and(|t| t <= 60), "{run:?}");
    assert!(run.most_sent <= catch_up_packets(8), "{run:?}");
    assert!(run.most_kept <= GONE_TICKS as usize + 10, "{run:?}");
    assert!((0..7).all(|s| kinds(&run, s).is_empty()), "{run:?}");
}

#[test]
fn eight_slots_a_deaf_peer_is_away_then_gone() {
    let run = lost_way(8, 3, 300, 5000, true, 2000);
    eprintln!("{run:?}");
    assert_eq!(kinds(&run, 3), [SlotNews::Away, SlotNews::Gone], "{run:?}");
    assert!(run.most_sent <= catch_up_packets(8), "{run:?}");
}
