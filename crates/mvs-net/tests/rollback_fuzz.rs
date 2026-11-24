//! The rollback fuzz on the trace matches' set-ups: a World stepped with the recorded
//! inputs is saved now and then, run 1 to 12 predicted steps on other inputs, loaded back and run on. The loaded state
//! must hash as the saved one, and every tick after must equal the straight run: the same events and hash as a World
//! that never left, and the same trace frame as the baseline's.
//!
//! A sample of the matches, each its first 6000 ticks, by default; all 128 whole with `MVS_ROLLBACK_ALL=1`. Without
//! traces or a store nothing is checked.

mod common;

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::Masher;
use mvs_sim::ai::expert::Workers;
use mvs_sim::rng::Rng;
use mvs_sim::snapshot::SnapshotRing;
use mvs_sim::state::hash_of;
use mvs_sim::{InputState, World};
use mvs_sim_trace::run::{Filter, baseline};
use mvs_sim_trace::{Book, bots_for, frame_of, input_of, world_for};
use mvs_trace::{Field, Trace};

/// The matches of the default run: one per kind of match (combat, bots, the Expert bot, items, minions, mutators,
/// the Chaos Wheel, Dodgeball, stages, modes).
const SAMPLE: &[&str] = &[
    "m5-combat",
    "m4-flow",
    "bot-lvl3",
    "bot-expert",
    "bot-ffa-a",
    "ffa-items-a",
    "velma-police-combat",
    "morty-morty",
    "steven-melon-combat",
    "gizmo-gizmo",
    "mut-giant",
    "wheel-combat",
    "dodgeball-combat",
    "stage-m011",
    "lab-picture",
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

/// One World of the match and its player bots.
struct Line {
    w: World,
    bots: Vec<Option<mvs_sim::ai::bt::BtBot>>,
}

impl Line {
    /// One step with the frame's recorded inputs, a bot's slot its controller's thought.
    fn step(&mut self, recorded: &[InputState]) -> Vec<InputState> {
        let mut inputs = recorded.to_vec();
        for (slot, bot) in self.bots.iter_mut().enumerate() {
            if let Some(bot) = bot {
                let me = self.w.players[slot];
                inputs[slot] = bot.think(&mut self.w, me);
            }
        }
        self.w.step(&inputs);
        inputs
    }
}

/// The fuzz of one match: `Err` with the first tick that differs, else its counts.
fn fuzz(name: &str, store: &mvs_store::Store, until: usize) -> Result<String, String> {
    let trace = Trace::read(&default_traces().join(format!("{name}.trace"))).map_err(|e| format!("{name}: {e}"))?;
    // (the run's Expert bots decide on workers, the reference's inline)
    static WORKERS: std::sync::OnceLock<std::sync::Arc<Workers>> = std::sync::OnceLock::new();
    let line = |workers: Option<&std::sync::Arc<Workers>>| -> Result<(Line, Book), String> {
        let (w, book) = world_for(&trace.header, store)?;
        let mut bots = bots_for(&trace.header.spec, &w);
        for b in bots.iter_mut().flatten() {
            b.set_workers(workers);
        }
        Ok((Line { w, bots }, book))
    };
    let (mut reference, _) = line(None)?;
    let (mut run, book) = line(Some(WORKERS.get_or_init(|| std::sync::Arc::new(Workers::new(2)))))?;
    let players = run.w.players.len();
    let mut ring = SnapshotRing::new(1, &run.w);
    let mut wrong: Vec<Masher> = (0..players).map(|i| Masher::new(900 + i32::try_from(i).expect("a slot"))).collect();
    let mut r = Rng::new(5);
    let (mut rolls, mut events, mut bytes) = (0, 0, Vec::new());
    let n = trace.len().min(until);
    for t in 0..n {
        let tick = u32::try_from(t).expect("a tick");
        if r.int(6.0) == 0.0 {
            ring.save(tick, &run.w);
            let h0 = hash_of(&run.w);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a draw of 0..12")]
            let steps = 1 + r.int(12.0) as usize;
            // (the predicted line's steps, as a session runs them: the bots do not think, a minion's controller holds)
            run.w.predicted = true;
            for k in 0..steps {
                let ins: Vec<InputState> = wrong.iter_mut().map(|m| m.at(t + k)).collect();
                run.w.step(&ins);
                run.w.events.clear();
            }
            run.w.predicted = false;
            assert!(ring.load(tick, &mut run.w));
            if hash_of(&run.w) != h0 {
                return Err(format!("{name} tick {t}: the loaded state differs from the saved one"));
            }
            rolls += 1;
        }
        let frame = trace.frame(t).map_err(|e| e.to_string())?;
        let recorded: Vec<InputState> = frame.inputs.iter().map(|i| input_of(i, &run.w, &book)).collect();
        reference.step(&recorded);
        let inputs = run.step(&recorded);
        events += reference.w.events.len();
        if reference.w.events != run.w.events {
            return Err(format!("{name} tick {t}: the events differ"));
        }
        if hash_of(&reference.w) != hash_of(&run.w) {
            return Err(format!("{name} tick {t}: the hash differs from the straight run's"));
        }
        let mut ours = frame_of(&run.w, &book, &inputs, &run.w.events);
        // (the human slots' inputs are the trace's own, as the plain run writes them)
        for (slot, rec) in frame.inputs.iter().enumerate() {
            if run.bots.get(slot).is_none_or(Option::is_none) {
                ours.frame.inputs[slot].clone_from(rec);
            }
        }
        bytes.clear();
        ours.frame.put(&mut bytes);
        if bytes != trace.frame_bytes(t) {
            let d = gaps_left().diffs(&frame, &ours.frame);
            if let Some(d) = d.first() {
                return Err(format!("{name} tick {t}: {}: oracle {} | Rust {}", d.path, d.expected, d.actual));
            }
        }
        reference.w.events.clear();
        run.w.events.clear();
    }
    Ok(format!("{name}: {n} ticks, {rolls} rollbacks, {events} events"))
}

#[test]
fn rollback_fuzz_on_the_trace_matches() {
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
                    let r = fuzz(name, &store, if all { usize::MAX } else { 6000 });
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
