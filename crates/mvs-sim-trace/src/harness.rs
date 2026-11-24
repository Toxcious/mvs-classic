//! A World for a test of a path no trace runs: one loaded at a baseline trace's frame (the match's own setup and
//! state, then changed by the test where its path starts), or one fresh from a setup. The test steps it, with the
//! trace's inputs or its own, and reads the World or its record. With no traces or no store the constructors return
//! `None` after a note, so a test passes there as the round trip does.
//!
//! ```ignore
//! let Some(mut h) = harness::at("morty-morty", 120) else { return };
//! let f = h.fighter(0);
//! h.w.fighter_mut(f).damage = 99.0; // the path's start the trace never reaches
//! h.step_recorded();
//! assert_eq!(h.record().world.fighters[0].damage, ...);
//! ```

use mvs_sim::fighter::FighterId;
use mvs_sim::setup::MatchSetup;
use mvs_sim::{InputState, World};
use mvs_trace::{Frame, Trace};

use crate::{Book, frame_of, load_frame, run, world_for};

/// A World under test, its trace's book of names, and where it is in the trace (if it came from one).
pub struct Harness {
    pub w: World,
    pub book: Book,
    trace: Option<Trace>,
    /// The trace frame the World is at (the World after that frame's step); the next recorded step is `next`'s.
    at: usize,
    /// The inputs and events of the last step, for `record`.
    inputs: Vec<InputState>,
    /// `diffs` keeps only the stage-B class (the oracle's trace, whose double floats the float32 sim does not
    /// meet bit for bit); against the Rust baseline it compares the whole record.
    stable: bool,
}

/// The World of the oracle's trace `name` at frame `frame` (the World after that step), loaded with `load_frame`: a
/// path test picks its frame in the oracle's match, which no float32 batch moves. What the load cannot set is
/// printed; the test decides whether that matters to its path.
pub fn at(name: &str, frame: usize) -> Option<Harness> {
    load_at(&run::default_traces(), true, name, frame)
}

/// [`at`] on the Rust baseline (`run::baseline`): `diffs` against it compares the whole record.
pub fn at_baseline(name: &str, frame: usize) -> Option<Harness> {
    let base = run::baseline();
    load_at(&base.dir, base.stable, name, frame)
}

/// [`at_baseline`] at the first frame from `from` on whose World `holds` is true, stepped there with the recorded
/// inputs: a path test names the state it needs, not the tick that state had in one recording (a baseline recorded
/// again moves it). Panics when no frame to the trace's end holds it.
pub fn at_baseline_where(name: &str, from: usize, holds: impl Fn(&World) -> bool) -> Option<Harness> {
    let mut h = at_baseline(name, from)?;
    let last = h.trace.as_ref().map_or(0, |t| t.len().saturating_sub(1));
    while !holds(&h.w) {
        assert!(h.at < last, "{name}: no frame from {from} on holds the state the test needs");
        h.step_recorded();
    }
    Some(h)
}

fn load_at(dir: &std::path::Path, stable: bool, name: &str, frame: usize) -> Option<Harness> {
    let path = dir.join(format!("{name}.trace"));
    let Ok(trace) = Trace::read(&path) else {
        eprintln!("note: no trace {} (the oracle's trace run writes it); nothing checked", path.display());
        return None;
    };
    let store = store()?;
    let (mut w, book) = world_for(&trace.header, &store).expect("the World of the header");
    let f = trace.frame(frame).expect("a frame of the trace");
    let loaded = load_frame(&mut w, &book, &f);
    for m in &loaded.misses {
        eprintln!("note: {name} frame {frame}: not loaded {m}");
    }
    Some(Harness { w, book, trace: Some(trace), at: frame, inputs: loaded.inputs, stable })
}

/// A fresh World of `setup` with `seed`, the characters' data from the dev store.
pub fn fresh(setup: &MatchSetup, seed: i32) -> Option<Harness> {
    fresh_on(setup, seed, crate::TRACE_STAGE)
}

/// [`fresh`] on the stage `stage` (its key, as a header's `stage`).
pub fn fresh_on(setup: &MatchSetup, seed: i32, stage: &str) -> Option<Harness> {
    let store = store()?;
    let keys = crate::keys_of(setup);
    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let data = mvs_data::GameData::load(&store, &refs, stage).expect("the gamedata kind loads");
    let book = Book::new(&data);
    let w = World::new(std::sync::Arc::new(data), seed, setup).expect("binds");
    Some(Harness { w, book, trace: None, at: 0, inputs: Vec::new(), stable: false })
}

fn store() -> Option<mvs_store::Store> {
    let dir = mvs_store::dev_store_dir();
    let s = mvs_store::Store::open(&dir).ok();
    if s.is_none() {
        eprintln!("note: no store at {}; nothing checked", dir.display());
    }
    s
}

impl Harness {
    /// The id of the `n`th fighter in the record's order (the pawns, else the players).
    pub fn fighter(&self, n: usize) -> FighterId {
        self.w.pawns.as_ref().unwrap_or(&self.w.players)[n]
    }

    /// The trace frame the World was last stepped to or loaded at.
    pub fn frame_index(&self) -> usize {
        self.at
    }

    /// One step with the inputs the trace recorded for the next frame (a fresh World: none pressed).
    pub fn step_recorded(&mut self) {
        if let Some(t) = &self.trace {
            // (a tutorial's page calls before that frame's step)
            crate::page_calls(&mut self.w, &t.header, self.at + 1);
        }
        let inputs = match &self.trace {
            Some(t) => {
                let f = t.frame(self.at + 1).expect("a next frame in the trace");
                f.inputs.iter().map(|i| crate::input_of(i, &self.w, &self.book)).collect()
            }
            None => vec![InputState::default(); self.w.players.len()],
        };
        self.step(&inputs);
    }

    /// One step with `inputs`, one per player.
    pub fn step(&mut self, inputs: &[InputState]) {
        self.w.step(inputs);
        self.inputs = inputs.to_vec();
        self.at += 1;
    }

    /// The trace's own frame `n`, as the oracle wrote it.
    pub fn recorded(&self, n: usize) -> Frame {
        self.trace.as_ref().expect("a harness made from a trace").frame(n).expect("a frame of the trace")
    }

    /// The World now as the oracle writer would write it (the last step's inputs, no events).
    pub fn record(&self) -> Frame {
        frame_of(&self.w, &self.book, &self.inputs, &[]).frame
    }

    /// The differences of the World now from the trace's frame at the same index, each as `path: oracle a | Rust b`:
    /// events left out (`record` writes none), an input flag the oracle left unset read as false, as the round trip
    /// reads them, and only those of the stage-B class (`mvs_trace::stable`): the float32 sim does not meet the
    /// oracle's double floats bit for bit.
    pub fn diffs(&self) -> Vec<String> {
        mvs_trace::diff_frames(&self.recorded(self.at), &self.record())
            .into_iter()
            .filter(|d| {
                !d.path.contains("events") && !run::unset_input_flag(d) && (!self.stable || mvs_trace::stable::keeps(d))
            })
            .map(|d| format!("{}: oracle {} | Rust {}", d.path, d.expected, d.actual))
            .collect()
    }
}
