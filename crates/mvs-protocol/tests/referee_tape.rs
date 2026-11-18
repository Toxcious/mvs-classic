//! The Referee against the tape of the oracle Referee (`referee-tape.json`): the same
//! calls at the same clocks, and every `send(slot, bytes)` the same, in order, up to the first slot the oracle found
//! silent (the oracle's `silent(slot)` is this referee's `Away`; at that call the sends to the slot itself aside: an
//! away peer waits for the CLOCK_MS beat). From there the rule is the port's own (away, back, gone) and
//! the tape's calls are run on unchecked but for the news. A test that finds no tape says so and passes.
//!
//! The port's rule itself is checked on peers of this file's own ([`Peer`]), each taking what the referee sends and
//! acknowledging it, whose way is lost for a time.

use std::collections::BTreeMap;

use mvs_protocol::match_wire::{
    ALL_TICKS, CLOCK_MS, INPUT_DELAY, InputHeader, MAX_PACKET, MAX_SLOTS, MSG_RULED, NO_END, RESUME_MS, SILENT_MS,
    TICK_MS, WireInput, input_packet, packet_fits, runs,
};
use mvs_protocol::referee::{BEHIND_TICKS, CATCH_UP_PACKETS, GONE_TICKS, Referee, RefereeOut, SlotNews};
use mvs_protocol::vectors;
use serde_json::{Value, json};

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
        let mut r = Referee::new(&humans, s["tick0"].as_f64().unwrap());
        // (past the oracle's first silent the rules part: only the news are kept, by slot)
        let mut parted = false;
        let mut news: BTreeMap<u64, Vec<(String, f64)>> = BTreeMap::new();
        let mut ts_silent: BTreeMap<u64, Vec<f64>> = BTreeMap::new();
        for (n, c) in s["calls"].as_array().unwrap().iter().enumerate() {
            let now = c["now"].as_f64();
            let slot = c["slot"].as_u64().map(|x| usize::try_from(x).unwrap());
            let mut out = Tape::default();
            match c["call"].as_str().unwrap() {
                "receive" => r.receive(slot.unwrap(), &unhex(c["bytes"].as_str().unwrap()), now.unwrap(), &mut out),
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
                    assert!(packet_fits(&unhex(b)), "{name}: call {n}: a packet that does not fit");
                }
            }
            if !parted {
                // (at the parting call the slot told silent is sent no more at once: an away peer waits for the
                // CLOCK_MS beat; what that call sends the others is compared)
                let silents: Vec<&Value> = want.iter().filter_map(|w| w.get("silent")).collect();
                let others = |l: &[Value]| -> Vec<Value> {
                    l.iter().filter(|o| !silents.contains(&&o["send"])).cloned().collect()
                };
                let (got, want) = (others(&out.0), others(want));
                assert_eq!(got.len(), want.len(), "{name}: call {n} ({}) at {:?}: how many it sent", c["call"], now);
                for (k, (got, want)) in got.iter().zip(&want).enumerate() {
                    assert_eq!(got, want, "{name}: call {n} ({}) at {:?}: output {k}", c["call"], now);
                }
                outs += got.len();
                parted = !silents.is_empty();
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
    eprintln!("referee-tape: {calls} calls, {outs} sends / silents compared");
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

/// A peer that sends an input a tick (for the tick its clock is at, the referee's) with what it has of each stream,
/// and takes every ruled packet: no rollback, only the streams.
struct Peer {
    slot: usize,
    /// Per slot the first tick it lacks, and the ticks that came ahead of it.
    have: [u32; MAX_SLOTS],
    early: [BTreeMap<u32, ()>; MAX_SLOTS],
}

impl Peer {
    fn new(slot: usize) -> Peer {
        Peer { slot, have: [INPUT_DELAY; MAX_SLOTS], early: Default::default() }
    }

    fn take(&mut self, b: &[u8]) {
        assert!(packet_fits(b), "a ruled packet that does not fit");
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
        let h = InputHeader { slot: self.slot as u8, first, count: 1, stamp, acks };
        let i: WireInput = [self.slot as u8 + 1; 22];
        input_packet(&h, &[i])
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

/// `peers` peers 1 tick (one way) from the referee; peer `lost`'s way lost both ways (`deaf`: only the way to it)
/// over ticks `from .. to`; the match runs `ticks`. A gone peer is taken out (Referee::left).
fn lost_way(peers: usize, lost: usize, from: i64, to: i64, deaf: bool, ticks: i64) -> Run {
    lost_way_pumped(peers, lost, from, to, deaf, ticks, false)
}

/// `lost_way`; `each`: a pump after every receive as well (as the relay's core and the in-process host do).
fn lost_way_pumped(peers: usize, lost: usize, from: i64, to: i64, deaf: bool, ticks: i64, each: bool) -> Run {
    let humans: Vec<usize> = (0..peers).collect();
    let mut r = Referee::new(&humans, 0.0);
    let mut ps: Vec<Peer> = humans.iter().map(|&s| Peer::new(s)).collect();
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
                assert!(b.len() <= MAX_PACKET);
                per[s] += 1;
                bytes[s] += b.len();
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
            if !out_of[p.slot] && !cut(p.slot, true) {
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
