//! The trace matches over the network: one peer per human slot, each its own World, session and bots, the referee
//! between them on a steady in-memory link. Each peer's device gives at frame f the trace's input for the tick the
//! session puts it at (f + its delay), so the ruled streams are the trace's inputs: no tick is substituted, every
//! confirmed frame of the first peer is the baseline's frame, each checksum and the hash at the end are the straight
//! run's.
//!
//! A session's first [`INPUT_DELAY`] ticks take no input (no device is read for them), while a trace that does not
//! start on the match's intro has its humans pressing from tick 0. So the sessions start from the straight run's World
//! and bots at the first tick K whose next [`INPUT_DELAY`] ticks the humans press nothing: the session's frame k is
//! the trace's tick K + k.
//!
//! A sample of the matches, each its first 6000 ticks, by default; all whole with `MVS_ROLLBACK_ALL=1`. Without
//! traces or a store nothing is checked.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use mvs_net::codec::same_input;
use mvs_net::link::{MatchLink, SimNet, Way, Ways};
use mvs_net::session::{Session, SessionConfig, SlotKind, wire_hash};
use mvs_protocol::match_wire::{INPUT_DELAY, TICK_MS};
use mvs_sim::ai::bt::BtBot;
use mvs_sim::input::NO_INPUT;
use mvs_sim::state::hash_of;
use mvs_sim::{InputState, World};
use mvs_sim_trace::run::{Filter, baseline};
use mvs_sim_trace::{Book, bots_for, frame_of, input_of, world_for};
use mvs_trace::{Field, Trace};

/// Matches with two or more humans, and with a human beside bots (the bots think on every peer).
const SAMPLE: &[&str] = &[
    "m5-combat",
    "m4-flow",
    "bot-lvl3",
    "ffa-items-a",
    "velma-police-combat",
    "steven-melon-combat",
    "lab-picture",
    "mut-giant",
    "stage-m011",
];

/// The compare of the plain trace run (`sim-trace`'s default): the known gaps of the record left out; against the
/// oracle's traces only the stage-B class (`run::baseline`).
fn gaps_left() -> Filter {
    Filter { skip_gaps: true, owner: Vec::new(), not: Vec::new(), stable: baseline().stable }
}

/// Where the traces are (`run::baseline`: the Rust baseline unless `MVS_TRACES` names the oracle's).
fn default_traces() -> std::path::PathBuf {
    baseline().dir
}

/// The ticks on_ruled gave, each with its inputs.
type Ruled = Rc<RefCell<Vec<(u32, Vec<InputState>)>>>;

/// One peer: its World, bots and session.
struct Peer {
    slot: usize,
    w: World,
    bots: Vec<Option<BtBot>>,
    s: Session,
    sums: Rc<RefCell<Vec<(u32, u32)>>>,
    /// Each tick on_ruled gave, with its inputs.
    ruled: Ruled,
}

/// The straight run of the trace's first `n` ticks: the inputs by tick (a bot's its thought), the hash after each
/// tick, the World and bots at the start tick, and the book of the writer that wrote the frames before it.
struct Straight {
    inputs: Vec<Vec<InputState>>,
    hashes: Vec<u32>,
    at_start: Option<(World, Vec<Option<BtBot>>)>,
    book: Option<Book>,
}

/// The first tick whose next [`INPUT_DELAY`] ticks no human presses anything, if any.
fn start_of(trace: &Trace, store: &mvs_store::Store, n: usize) -> Result<Option<usize>, String> {
    let (w, book) = world_for(&trace.header, store)?;
    let humans: Vec<usize> = w.setup.players.iter().enumerate().filter(|(_, p)| !p.bot).map(|(i, _)| i).collect();
    let mut idle = 0;
    for t in 0..n {
        let frame = trace.frame(t).map_err(|e| e.to_string())?;
        let quiet = humans.iter().all(|&h| same_input(&input_of(&frame.inputs[h], &w, &book), &NO_INPUT));
        idle = if quiet { idle + 1 } else { 0 };
        if idle == INPUT_DELAY as usize {
            return Ok(Some(t + 1 - idle));
        }
    }
    Ok(None)
}

fn straight(trace: &Trace, store: &mvs_store::Store, n: usize, start: usize) -> Result<Straight, String> {
    let (mut w, book) = world_for(&trace.header, store)?;
    let mut bots = bots_for(&trace.header.spec, &w);
    let mut s = Straight { inputs: Vec::with_capacity(n), hashes: Vec::with_capacity(n), at_start: None, book: None };
    for t in 0..n {
        if t == start {
            s.at_start = Some((w.clone(), bots.clone()));
        }
        let frame = trace.frame(t).map_err(|e| e.to_string())?;
        let mut inputs: Vec<InputState> = frame.inputs.iter().map(|i| input_of(i, &w, &book)).collect();
        for (slot, bot) in bots.iter_mut().enumerate() {
            if let Some(bot) = bot {
                let me = w.players[slot];
                inputs[slot] = bot.think(&mut w, me);
            }
        }
        w.step(&inputs);
        // (the writer keeps the ids of the actors it saw live: a reference after the start to one gone before it
        // writes its id only if the frames before the start were written with the same book)
        if t < start {
            let _ = frame_of(&w, &book, &inputs, &w.events);
        }
        w.events.clear();
        s.hashes.push(wire_hash(hash_of(&w)));
        s.inputs.push(inputs);
    }
    s.book = Some(book);
    Ok(s)
}

/// One match over the network: `Err` with the first thing that differs, else its counts.
#[allow(clippy::too_many_lines, reason = "one match's run and checks, in order")]
fn over_the_net(name: &str, store: &mvs_store::Store, until: usize) -> Result<String, String> {
    let trace = std::sync::Arc::new(
        Trace::read(&default_traces().join(format!("{name}.trace"))).map_err(|e| format!("{name}: {e}"))?,
    );
    let n = trace.len().min(until);
    let Some(start) = start_of(&trace, store, n)? else {
        return Ok(format!("{name}: the humans never rest {INPUT_DELAY} ticks, nothing on the net"));
    };
    let mut want = straight(&trace, store, n, start)?;
    let mut book0 = want.book.take();
    let (w0, b0) = want.at_start.take().ok_or_else(|| format!("{name}: no World at tick {start}"))?;
    let humans: Vec<usize> = w0.setup.players.iter().enumerate().filter(|(_, p)| !p.bot).map(|(i, _)| i).collect();
    if humans.is_empty() {
        return Ok(format!("{name}: no human, nothing on the net"));
    }
    // (the session's ticks: frame k is the trace's tick start + k)
    let last = u32::try_from(n - start).expect("a tick");
    let bad = Rc::new(RefCell::new(Vec::<String>::new()));
    let ends = Rc::new(RefCell::new(Vec::<u32>::new()));
    // (three ticks of latency each way, past the delay: the peers predict and roll back; nothing lost or late)
    let way = Way { latency: 3.0 * TICK_MS, ..Way::PERFECT };
    let mut net = SimNet::new(&humans, Ways { up: way, down: way }, 0.0, 7);
    let mut frames = 0usize;
    let checked = Rc::new(RefCell::new(0usize));
    let hashes = Rc::new(want.hashes.clone());
    let mut peers: Vec<Peer> = Vec::new();
    for (p, &slot) in humans.iter().enumerate() {
        let book = match book0.take() {
            Some(b) => b,
            None => world_for(&trace.header, store)?.1,
        };
        let (w, bots) = (w0.clone(), b0.clone());
        let kinds = w
            .setup
            .players
            .iter()
            .enumerate()
            .map(|(i, x)| {
                if x.bot {
                    SlotKind::Bot
                } else if i == slot {
                    SlotKind::Local
                } else {
                    SlotKind::Remote
                }
            })
            .collect();
        // (the first peer goes back to the confirmed line every frame, so its hook sees every confirmed tick; the
        // others keep their predicted line where it held, KEEP_BEHIND)
        let keep_behind = if p == 0 { 0 } else { SessionConfig::new(Vec::new()).keep_behind };
        let cfg = SessionConfig { keep_behind, ..SessionConfig::new(kinds) };
        let mut s = Session::new(&w, cfg, 0.0).map_err(|e| e.to_string())?;
        let sums = Rc::new(RefCell::new(Vec::new()));
        let g = sums.clone();
        s.on_checksum(Box::new(move |t, v, _| g.borrow_mut().push((t, v))));
        let (tr, bad2, ends2, checked2, hashes2) =
            (trace.clone(), bad.clone(), ends.clone(), checked.clone(), hashes.clone());
        // (the first peer writes each confirmed frame and compares it with the baseline's, and the hash at the end)
        let mut bytes = Vec::new();
        s.on_confirmed(Box::new(move |k, w, inputs, events| {
            if p != 0 || k >= last || !bad2.borrow().is_empty() {
                return;
            }
            let t = start + k as usize;
            if k + 1 == last {
                ends2.borrow_mut().push(k);
                if wire_hash(hash_of(w)) != hashes2[t] {
                    bad2.borrow_mut().push(format!("peer {p}: the hash at the end is not the straight run's"));
                }
            }
            let Ok(frame) = tr.frame(t) else { return };
            let mut ours = frame_of(w, &book, inputs, events);
            for (slot, rec) in frame.inputs.iter().enumerate() {
                if w.setup.players.get(slot).is_none_or(|x| !x.bot) {
                    ours.frame.inputs[slot].clone_from(rec);
                }
            }
            bytes.clear();
            ours.frame.put(&mut bytes);
            *checked2.borrow_mut() += 1;
            if bytes != tr.frame_bytes(t)
                && let Some(d) = gaps_left().diffs(&frame, &ours.frame).first()
            {
                bad2.borrow_mut().push(format!("tick {t}: {}: oracle {} | Rust {}", d.path, d.expected, d.actual));
            }
        }));
        let ruled = Rc::new(RefCell::new(Vec::new()));
        let r = ruled.clone();
        s.on_ruled(Box::new(move |k, inputs| r.borrow_mut().push((k, inputs.to_vec()))));
        peers.push(Peer { slot, w, bots, s, sums, ruled });
    }
    let mut now = 0.0;
    let mut tick = 0u32;
    while peers.iter().any(|p| p.s.confirmed() < last) && bad.borrow().is_empty() {
        if tick > last * 2 + 600 {
            let at: Vec<u32> = peers.iter().map(|p| p.s.confirmed()).collect();
            return Err(format!("{name}: the peers confirmed only {at:?} ticks"));
        }
        net.pump(now);
        for p in &mut peers {
            while let Some(b) = net.link(p.slot).recv(now) {
                p.s.receive(&b, now);
            }
            let d = p.s.local_delay();
            if d != INPUT_DELAY {
                return Err(format!(
                    "{name}: peer at slot {} raised its delay to {d} at frame {}",
                    p.slot,
                    p.s.frame()
                ));
            }
            let at = start + (p.s.frame() + d) as usize;
            let device = want.inputs.get(at).map_or(NO_INPUT, |i| i[p.slot]);
            let _ = p.s.advance(&mut p.w, &mut p.bots, device, now);
            frames += 1;
            let sent: Vec<Vec<u8>> = p.s.packets().collect();
            for b in &sent {
                net.link(p.slot).send(b, now);
            }
        }
        now += TICK_MS;
        tick += 1;
    }
    if let Some(e) = bad.borrow().first() {
        return Err(format!("{name}: {e}"));
    }
    let mut sums = 0;
    for (i, p) in peers.iter().enumerate() {
        let subs: u64 = p.s.stats().substituted.iter().sum();
        if subs > 0 {
            return Err(format!("{name}: peer {i} saw {subs} ticks substituted"));
        }
        // (on_ruled: every confirmed tick once, in order, with the straight run's inputs, on both ways of the line)
        for (k, (t, ins)) in p.ruled.borrow().iter().enumerate() {
            if *t as usize != k {
                return Err(format!("{name}: peer {i}: on_ruled gave tick {t} as its tick {k}"));
            }
            let straight = want.inputs.get(start + k);
            if (*t) < last
                && straight.is_none_or(|s| s.len() != ins.len() || s.iter().zip(ins).any(|(a, b)| !same_input(a, b)))
            {
                return Err(format!("{name}: peer {i}: on_ruled's inputs of tick {t} are not the straight run's"));
            }
        }
        if p.ruled.borrow().len() < p.s.confirmed() as usize {
            return Err(format!(
                "{name}: peer {i}: on_ruled gave {} of {} ticks",
                p.ruled.borrow().len(),
                p.s.confirmed()
            ));
        }
        for &(t, v) in p.sums.borrow().iter().filter(|(t, _)| *t <= last) {
            if want.hashes[start + t as usize - 1] != v {
                return Err(format!("{name}: peer {i}: the checksum after {t} ticks is not the straight run's"));
            }
            sums += 1;
        }
    }
    if ends.borrow().is_empty() {
        return Err(format!("{name}: the first peer never confirmed the last tick"));
    }
    let rolls: u64 = peers.iter().map(|p| p.s.stats().rollbacks).sum();
    Ok(format!(
        "{name}: {} peers from tick {start}, {} ticks, {} frames compared, {sums} checksums equal, {rolls} rollbacks, \
         {frames} frames run",
        peers.len(),
        last,
        checked.borrow()
    ))
}

#[test]
fn trace_matches_over_the_network() {
    let all = std::env::var_os("MVS_ROLLBACK_ALL").is_some();
    let names: Vec<String> = if all {
        match mvs_sim_trace::run::traces(&default_traces(), &[]) {
            Ok(t) => t.into_iter().map(|(n, _)| n).collect(),
            Err(e) => {
                eprintln!("note: no traces ({e}); nothing checked");
                return;
            }
        }
    } else {
        SAMPLE.iter().map(|&s| s.to_owned()).collect()
    };
    if !default_traces().join(format!("{}.trace", names[0])).exists() {
        eprintln!("note: no traces at {}; nothing checked", default_traces().display());
        return;
    }
    if common::store().is_none() {
        return;
    }
    let next = AtomicUsize::new(0);
    let found = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    std::thread::scope(|sc| {
        for _ in 0..workers {
            sc.spawn(|| {
                let store = common::store().expect("the store");
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(name) = names.get(i) else { break };
                    let r = over_the_net(name, &store, if all { usize::MAX } else { 6000 });
                    match &r {
                        Ok(s) => println!("ok   {s}"),
                        Err(e) => println!("FAIL {e}"),
                    }
                    found.lock().expect("the results").push(r);
                }
            });
        }
    });
    let found = found.into_inner().expect("the results");
    let bad: Vec<&String> = found.iter().filter_map(|r| r.as_ref().err()).collect();
    assert!(bad.is_empty(), "{} of {} matches differ: {bad:?}", bad.len(), found.len());
}
