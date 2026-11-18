//! The three uses of a baseline trace: the round trip (each frame loaded into a World and written back, byte for
//! byte), the run (the World stepped with the recorded inputs, each frame compared with the oracle one) and the seeded
//! run (each tick from the oracle frame before it).

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use mvs_trace::{Difference, Field, Frame, Trace, TraceWriter, diff_frames};

use crate::{bots_for, frame_of, load_frame, owners::owner_in, world_for};

/// The record paths (list indexes left out) whose Rust field cannot hold the record's value yet, each with the Rust
/// field it waits on (the README, "What a load leaves"). An entry goes when its field is fixed.
pub const GAPS: &[(&str, &str)] = &[
    ("world.fighters[].rage_input.section", "the record holds no instance past the main slot"),
    ("world.fighters[].rage_input.position", "the record holds no instance past the main slot"),
    ("world.fighters[].rage_input.age", "the record holds no instance past the main slot"),
    ("world.fighters[].rage_input.play_rate", "the record holds no instance past the main slot"),
    ("world.fighters[].rage_input.ended", "the record holds no instance past the main slot"),
    ("world.fighters[].rage_input.started", "the record holds no instance past the main slot"),
    ("world.fighters[].rage_input.blend_out_fired", "the record holds no instance past the main slot"),
    ("world.fighters[].rage_input.interrupt_fired", "the record holds no instance past the main slot"),
    ("world.fighters[].rage_input.blend_left", "the record holds no instance past the main slot"),
    ("world.fighters[].rage_input.walking", "the record holds no instance past the main slot"),
    ("world.fighters[].rage_input.proxy", "the record holds no instance past the main slot"),
];

/// The load paths (indexes left out) of the misses the gaps make, and the timers' payloads the record does not hold.
pub const GAP_MISSES: &[&str] = &["timers[]"];

/// The unwritten values the gaps make.
pub const GAP_UNMAPPED: &[&str] = &[
    "rageInput past its main instance (the record holds the main instance only)",
    "an active hit's own packet (the record names the notify only)",
];

// Deviation: the input flags `canonInput` writes as booleans are a Rust `bool`, so an oracle
// input that never went through it (a fighter's first `prev`, a pawn's empty input) has them undefined where the Rust
// has `false` (the recorded inputs too, which load into an InputState). The sim reads each as truthy or not, so the
// compares take undefined and `false` as one value there.
const INPUT_FLAGS: &[&str] = &["jump_local", "item", "down_btn", "force", "neutral_evade", "axis"];

/// A difference that is an oracle undefined against a Rust `false` (or back) of an input flag.
pub(crate) fn unset_input_flag(d: &Difference) -> bool {
    let p = general(&d.path);
    let flag = ["world.fighters[].inp.", "world.fighters[].prev.", "inputs[]."]
        .iter()
        .any(|pre| p.strip_prefix(pre).is_some_and(|f| INPUT_FLAGS.contains(&f)));
    flag && matches!((d.expected.as_str(), d.actual.as_str()), ("none", "false") | ("false", "none"))
}

pub fn is_gap(general_path: &str) -> bool {
    GAPS.iter().any(|(p, _)| *p == general_path)
}

/// A miss's path (the part before its reason), indexes left out.
pub fn miss_path(m: &str) -> String {
    general(m.split(": ").next().unwrap_or(m))
}

pub fn is_gap_miss(m: &str) -> bool {
    GAP_MISSES.contains(&miss_path(m).as_str())
}

/// A difference's path with its list indexes left out (`fighters[].x`), the key its count is kept under.
pub use mvs_trace::field::general_path as general;

/// The traces of a directory, by match name (`<name>.trace`), in name order; `only` keeps the named ones.
pub fn traces(dir: &Path, only: &[String]) -> Result<Vec<(String, PathBuf)>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("trace") {
            continue;
        }
        let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_owned();
        if only.is_empty() || only.contains(&name) {
            out.push((name, p));
        }
    }
    out.sort();
    for n in only {
        if !out.iter().any(|(m, _)| m == n) {
            return Err(format!("no trace {n}.trace in {}", dir.display()));
        }
    }
    Ok(out)
}

/// Where the baseline traces are: `MVS_TRACES`, else `.scratch/traces` of the main tree (the recorder runs there; a
/// worktree's main tree is the parent of git's common dir), else of this tree.
pub fn default_traces() -> PathBuf {
    if let Some(d) = std::env::var_os("MVS_TRACES") {
        return PathBuf::from(d);
    }
    main_scratch().join("traces")
}

/// The Rust baseline's folder under the main tree's `.scratch`: the traces `sim-trace --record` wrote before the
/// change being checked, from the same store.
pub const RUST_BASELINE: &str = "baseline";

/// The traces a test that steps the Rust sim compares with, and how.
pub struct Baseline {
    pub dir: PathBuf,
    /// The compare: the whole record against the Rust baseline; only the stage-B class (`Filter::stable`) against
    /// the oracle's traces, whose floats the float32 sim moves.
    pub stable: bool,
}

/// `MVS_RUST_TRACES` (a Rust baseline), else `MVS_TRACES` (the oracle's traces, compared by the stage-B class),
/// else the main tree's [`RUST_BASELINE`].
pub fn baseline() -> Baseline {
    if let Some(d) = std::env::var_os("MVS_RUST_TRACES") {
        return Baseline { dir: PathBuf::from(d), stable: false };
    }
    if let Some(d) = std::env::var_os("MVS_TRACES") {
        return Baseline { dir: PathBuf::from(d), stable: true };
    }
    Baseline { dir: main_scratch().join(RUST_BASELINE), stable: false }
}

/// `.scratch` of the main tree (a worktree's main tree is the parent of git's common dir), else of this tree.
fn main_scratch() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let common = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(here)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok());
    if let Some(main) = common.as_deref().map(str::trim).and_then(|c| Path::new(c).parent()) {
        return main.join(".scratch");
    }
    here.join("../../.scratch")
}

/// What the round trip of one trace found.
#[derive(Debug, Default)]
pub struct RoundTrip {
    pub frames: usize,
    /// The frames written back byte for byte.
    pub equal: usize,
    /// The first differing frame: its index and differences.
    pub first: Option<(usize, Vec<Difference>)>,
    /// Each differing path (indexes left out) and the frames it differs in.
    pub paths: IndexMap<String, usize>,
    /// What the loads could not set, and in how many frames.
    pub misses: IndexMap<String, usize>,
    /// The values the writer had no rule for, and in how many frames.
    pub unmapped: IndexMap<String, usize>,
}

/// Loads every frame (up to `until`) into one World made from the header, writes it back and compares the bytes.
pub fn round_trip(trace: &Trace, store: &mvs_store::Store, until: Option<usize>) -> Result<RoundTrip, String> {
    let (mut w, book) = world_for(&trace.header, store)?;
    let n = until.map_or(trace.len(), |u| u.min(trace.len()));
    let mut rt = RoundTrip { frames: n, ..RoundTrip::default() };
    let mut bytes = Vec::with_capacity(1 << 16);
    for i in 0..n {
        let frame = trace.frame(i).map_err(|e| e.to_string())?;
        let loaded = load_frame(&mut w, &book, &frame);
        for m in loaded.misses {
            *rt.misses.entry(general(&m)).or_default() += 1;
        }
        let back = frame_of(&w, &book, &loaded.inputs, &loaded.events);
        for u in back.unmapped {
            *rt.unmapped.entry(u).or_default() += 1;
        }
        bytes.clear();
        back.frame.put(&mut bytes);
        if bytes == trace.frame_bytes(i) {
            rt.equal += 1;
            continue;
        }
        let diffs: Vec<Difference> =
            diff_frames(&frame, &back.frame).into_iter().filter(|d| !unset_input_flag(d)).collect();
        if diffs.is_empty() {
            rt.equal += 1;
            continue;
        }
        count_paths(&mut rt.paths, &diffs);
        if rt.first.is_none() {
            rt.first = Some((i, diffs));
        }
    }
    Ok(rt)
}

fn count_paths(paths: &mut IndexMap<String, usize>, diffs: &[Difference]) {
    let mut seen: Vec<String> = Vec::new();
    for d in diffs {
        let g = general(&d.path);
        if !seen.contains(&g) {
            *paths.entry(g.clone()).or_default() += 1;
            seen.push(g);
        }
    }
}

thread_local! {
    static PANIC: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    static CATCHING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// One step, a panic (an `unported!` call reached) caught and returned as its message and place.
pub fn step_caught(w: &mut mvs_sim::World, inputs: &[mvs_sim::InputState]) -> Result<(), String> {
    caught(|| w.step(inputs))
}

/// `f`, a panic in it caught and returned as its message and place.
pub fn caught<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let next = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if CATCHING.with(std::cell::Cell::get) {
                let at = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
                let msg = info
                    .payload()
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_owned())
                    .or_else(|| info.payload().downcast_ref::<String>().cloned())
                    .unwrap_or_default();
                PANIC.with(|p| *p.borrow_mut() = format!("{msg}{at}"));
            } else {
                next(info);
            }
        }));
    });
    CATCHING.with(|c| c.set(true));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    CATCHING.with(|c| c.set(false));
    r.map_err(|_| PANIC.with(|p| std::mem::take(&mut *p.borrow_mut())))
}

/// What a compare leaves out and keeps.
#[derive(Clone, Debug, Default)]
pub struct Filter {
    /// Leave the known gaps out (`GAPS`, `GAP_MISSES`, `GAP_UNMAPPED`).
    pub skip_gaps: bool,
    /// Keep only the differences whose owner module starts with one of these (`fighter::body`, `combat`; `--owner`
    /// repeated); the unwritten values, which name no field, are then printed but not counted.
    pub owner: Vec<String>,
    /// Drop the differences whose record path starts with one of these (`--not`), as the owner filter drops the
    /// others'.
    pub not: Vec<String>,
    /// Keep only the differences of the stage-B class (`mvs_trace::stable`: what the float32 sim must not move).
    pub stable: bool,
}

impl Filter {
    fn owned(&self, path: &str, frame: &Frame) -> bool {
        let bare = path.strip_prefix("world.").unwrap_or(path);
        !self.not.iter().any(|n| match n.strip_prefix("*.") {
            // `*.key`: the key at any depth (the path's last segment, or a segment followed by more)
            Some(key) => bare == key || bare.split(['.', '[']).any(|seg| seg == key),
            None => path.starts_with(n.as_str()) || bare.starts_with(n.as_str()),
        }) && (self.owner.is_empty() || owner_in(path, frame).split(" + ").any(|m| self.owns(m)))
    }

    /// Whether a module is one of the owners' (starts with one of the prefixes).
    fn owns(&self, module: &str) -> bool {
        self.owner.iter().any(|o| module.starts_with(o.as_str()))
    }

    /// For a panic message with its place (`msg at file:line`) of a step: the place and message when the owner
    /// filter is on and the place is outside the owner's module (another module's stub), else `None`.
    pub fn blocked_by(&self, panic: &str) -> Option<String> {
        if self.owner.is_empty() {
            return None;
        }
        let (msg, place) = panic.rsplit_once(" at ").unwrap_or((panic, ""));
        let file = place.rsplit_once(':').map_or(place, |(f, _)| f);
        if crate::module_of_file(file).is_some_and(|m| self.owns(&m)) {
            return None;
        }
        Some(format!("{} {msg}", place.replace('\\', "/")).trim().to_owned())
    }

    /// The differences of a frame that count.
    pub fn diffs(&self, expected: &Frame, actual: &Frame) -> Vec<Difference> {
        diff_frames(expected, actual)
            .into_iter()
            .filter(|d| {
                !(self.skip_gaps && is_gap(&general(&d.path)))
                    && !unset_input_flag(d)
                    && (!self.stable || mvs_trace::stable::keeps(d))
                    && self.owned(&d.path, expected)
            })
            .collect()
    }

    pub fn misses(&self, m: Vec<String>) -> Vec<String> {
        m.into_iter().filter(|m| !(self.skip_gaps && is_gap_miss(m))).collect()
    }

    pub fn unmapped(&self, u: Vec<String>) -> Vec<String> {
        u.into_iter().filter(|u| !(self.skip_gaps && GAP_UNMAPPED.contains(&u.as_str()))).collect()
    }

    /// Whether unwritten values make a tick a mismatch.
    pub fn counts_unmapped(&self) -> bool {
        self.owner.is_empty() && self.not.is_empty()
    }
}

/// One tick that did not match: its frame index, what the load before it could not set, what the writer could not
/// write and the differences.
#[derive(Debug, Default)]
pub struct Tick {
    pub tick: usize,
    pub misses: Vec<String>,
    pub unmapped: Vec<String>,
    pub diffs: Vec<Difference>,
    /// The step's panic message.
    pub panic: Option<String>,
}

/// What a run or seeded run of one match found.
#[derive(Debug, Default)]
pub struct Checked {
    pub ticks: usize,
    pub matching: usize,
    /// The first few ticks that did not match.
    pub first: Vec<Tick>,
    /// Each differing path (indexes left out) and the ticks it differs in.
    pub paths: IndexMap<String, usize>,
    /// The ticks whose step panicked (in the owner's module, or with no owner named).
    pub panics: usize,
    /// The ticks whose step panicked outside the owner's module (a seeded run with `--owner`): neither matches nor
    /// mismatches.
    pub blocked: usize,
    /// Each place and message that blocked, and the ticks it blocked.
    pub blocked_by: IndexMap<String, usize>,
    /// The Rust trace (a plain run's, for `--out`).
    pub trace: Option<Vec<u8>>,
}

impl Checked {
    /// A tick of a seeded run: a panic outside the owner's module is counted as blocked, not kept as a mismatch.
    pub fn tick_seeded(&mut self, t: Tick, keep: usize, filter: &Filter) {
        if let Some(place) = t.panic.as_deref().and_then(|p| filter.blocked_by(p)) {
            self.blocked += 1;
            *self.blocked_by.entry(place).or_default() += 1;
            return;
        }
        self.tick(t, keep);
    }

    fn tick(&mut self, t: Tick, keep: usize) {
        count_paths(&mut self.paths, &t.diffs);
        if t.panic.is_some() {
            self.panics += 1;
        }
        if self.first.len() < keep {
            self.first.push(t);
        }
    }
}

/// The plain run: the World made from the header, stepped with each frame's recorded inputs, each frame compared
/// with the oracle one. A step that panics ends the run there; a panic while the World is made (its match reset) is
/// the first tick's. `write` keeps the Rust trace (`Checked::trace`): its frames are held whole until the end, as
/// large as the trace read.
pub fn run_match(
    trace: &Trace,
    store: &mvs_store::Store,
    until: Option<usize>,
    filter: &Filter,
    keep: usize,
    write: bool,
) -> Result<Checked, String> {
    let n = until.map_or(trace.len(), |u| u.min(trace.len()));
    let mut c = Checked { ticks: n, ..Checked::default() };
    // (the bots too: a bot's controller is made with the World, its tree runner's panic is the first tick's)
    let made = caught(|| {
        let (w, book) = world_for(&trace.header, store)?;
        let bots = bots_for(&trace.header.spec, &w);
        Ok::<_, String>((w, book, bots))
    });
    let (mut w, book, mut bots) = match made {
        Ok(r) => r?,
        Err(e) => {
            c.tick(Tick { tick: 0, panic: Some(e), ..Tick::default() }, keep);
            return Ok(c);
        }
    };
    let mut out = write.then(TraceWriter::new);
    let mut bytes = Vec::with_capacity(1 << 16);
    for i in 0..n {
        let frame = trace.frame(i).map_err(|e| e.to_string())?;
        let mut inputs: Vec<_> = frame.inputs.iter().map(|r| crate::input_of(r, &w, &book)).collect();
        // (a tutorial's page calls of the frame first, as the recorder makes them: their events are the frame's)
        if let Err(e) = caught(|| crate::page_calls(&mut w, &trace.header, i)) {
            c.tick(Tick { tick: i, panic: Some(e), ..Tick::default() }, keep);
            break;
        }
        // A bot's slot takes its controller's input, thought before the step as the app's loop does; a difference
        // from the recorded one shows as `inputs[i].*` lines.
        let mut thought = Ok(());
        for (slot, bot) in bots.iter_mut().enumerate() {
            if let Some(bot) = bot {
                let me = w.players[slot];
                match caught(|| bot.think(&mut w, me)) {
                    Ok(input) => inputs[slot] = input,
                    Err(e) => {
                        thought = Err(e);
                        break;
                    }
                }
            }
        }
        if let Err(e) = thought.and_then(|()| step_caught(&mut w, &inputs)) {
            c.tick(Tick { tick: i, panic: Some(e), ..Tick::default() }, keep);
            break;
        }
        let mut ours = frame_of(&w, &book, &inputs, &w.events);
        w.events.clear();
        // (the other slots' inputs are the trace's own, not the sim's output: written as recorded)
        for (slot, rec) in frame.inputs.iter().enumerate() {
            if bots.get(slot).is_none_or(Option::is_none) {
                ours.frame.inputs[slot].clone_from(rec);
            }
        }
        if let Some(out) = &mut out {
            out.push(&ours.frame);
        }
        check(&mut c, i, &frame, ours, Vec::new(), trace.frame_bytes(i), &mut bytes, filter, keep);
    }
    if let Some(out) = out {
        let mut header = trace.header.clone();
        header.source = serde_json::json!({ "writer": "mvs-sim-trace" });
        c.trace = Some(out.finish(header));
    }
    Ok(c)
}

/// The seeded run of one match from frame `from`: each tick t loaded from the oracle frame t - 1 (the header's World
/// for the first), stepped once with frame t's inputs and compared with frame t. A tick whose step panics is a
/// mismatch (with an owner filter, one outside the owner's module is blocked instead); the World goes back to its
/// state before that step.
pub fn seeded(
    trace: &Trace,
    store: &mvs_store::Store,
    from: usize,
    until: Option<usize>,
    filter: &Filter,
    keep: usize,
) -> Result<Checked, String> {
    let (mut w, book) = world_for(&trace.header, store)?;
    if from > 0 {
        // The set-up's own events (the match start's) belong to the first tick's output: a run that starts later
        // loads a frame whose step drained them long ago, and a load puts no events in the World.
        w.events.clear();
        let ids: Vec<_> = w.fighters.ids().collect();
        for id in ids {
            if let Some(fr) = w.fighters.get_mut(id) {
                fr.events.clear();
            }
        }
    }
    let mut before = w.clone();
    let n = until.map_or(trace.len(), |u| u.min(trace.len()));
    let from = from.min(n);
    let mut c = Checked { ticks: n - from, ..Checked::default() };
    let mut bytes = Vec::with_capacity(1 << 16);
    let mut prev: Option<Frame> = match from {
        0 => None,
        f => Some(trace.frame(f - 1).map_err(|e| e.to_string())?),
    };
    for i in from..n {
        let misses = match &prev {
            Some(p) => filter.misses(load_frame(&mut w, &book, p).misses),
            None => Vec::new(),
        };
        let frame = trace.frame(i).map_err(|e| e.to_string())?;
        let inputs: Vec<_> = frame.inputs.iter().map(|r| crate::input_of(r, &w, &book)).collect();
        before.clone_from(&w);
        // (a tutorial's page calls of the frame first, caught with its step)
        if let Err(e) =
            caught(|| crate::page_calls(&mut w, &trace.header, i)).and_then(|()| step_caught(&mut w, &inputs))
        {
            // (no half-stepped state goes on to the next tick)
            w.clone_from(&before);
            w.events.clear();
            // (the fighters' events are not state: a load keeps them, so a caught tick's would be read again)
            let ids: Vec<_> = w.fighters.ids().collect();
            for id in ids {
                if let Some(fr) = w.fighters.get_mut(id) {
                    fr.events.clear();
                }
            }
            c.tick_seeded(Tick { tick: i, misses, panic: Some(e), ..Tick::default() }, keep, filter);
            prev = Some(frame);
            continue;
        }
        let mut ours = frame_of(&w, &book, &inputs, &w.events);
        w.events.clear();
        ours.frame.inputs.clone_from(&frame.inputs);
        check(&mut c, i, &frame, ours, misses, trace.frame_bytes(i), &mut bytes, filter, keep);
        prev = Some(frame);
    }
    Ok(c)
}

#[allow(clippy::too_many_arguments)]
fn check(
    c: &mut Checked,
    i: usize,
    expected: &Frame,
    ours: crate::Written,
    misses: Vec<String>,
    expected_bytes: &[u8],
    bytes: &mut Vec<u8>,
    filter: &Filter,
    keep: usize,
) {
    let unmapped = filter.unmapped(ours.unmapped);
    bytes.clear();
    ours.frame.put(bytes);
    let diffs = if bytes == expected_bytes { Vec::new() } else { filter.diffs(expected, &ours.frame) };
    if diffs.is_empty() && (unmapped.is_empty() || !filter.counts_unmapped()) {
        c.matching += 1;
        return;
    }
    c.tick(Tick { tick: i, misses, unmapped, diffs, panic: None }, keep);
}

/// A difference as the runner prints it: the field, both values and the field's owner in that frame.
pub fn describe(d: &Difference, frame: &Frame) -> String {
    format!("{}: oracle {} | Rust {} ({})", d.path, d.expected, d.actual, owner_in(&d.path, frame))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panicked(tick: usize, at: &str) -> Tick {
        Tick { tick, panic: Some(format!("unported: x at {at}")), ..Tick::default() }
    }

    #[test]
    fn a_panic_outside_the_owner_is_blocked_not_a_mismatch() {
        let f = Filter { owner: vec!["chars::creature::tether".to_owned()], ..Filter::default() };
        let mut c = Checked::default();
        c.tick_seeded(panicked(0, r"crates\mvs-sim\src\fighter\body.rs:10"), 3, &f);
        c.tick_seeded(panicked(1, "crates/mvs-sim/src/fighter/body.rs:10"), 3, &f);
        c.tick_seeded(panicked(2, "crates/mvs-sim/src/fighter/body.rs:99"), 3, &f);
        c.tick_seeded(panicked(3, "crates/mvs-sim/src/chars/creature/tether.rs:7"), 3, &f);
        assert_eq!((c.blocked, c.panics, c.first.len()), (3, 1, 1));
        assert_eq!(c.blocked_by.len(), 2);
        assert_eq!(c.blocked_by["crates/mvs-sim/src/fighter/body.rs:10 unported: x"], 2);
        // (no owner: every panic is a mismatch)
        let mut c = Checked::default();
        c.tick_seeded(panicked(0, "crates/mvs-sim/src/fighter/body.rs:10"), 3, &Filter::default());
        assert_eq!((c.blocked, c.panics), (0, 1));
    }
}
