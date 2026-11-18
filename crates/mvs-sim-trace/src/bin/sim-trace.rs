//! `sim-trace`: the Rust sim against the oracle traces (the crate README).
//!
//! - default: each match stepped from its header with the recorded inputs, each frame compared with the oracle one; one
//!   line a match, `same` or the first differing tick, field, both values and the field's owner module.
//! - `--seeded`: each tick stepped from the oracle frame before it; the matching ticks counted, the first few
//!   mismatches printed (`--show <n>` of them, 3 by default), from tick `--from <tick>` on.
//! - `--round-trip`: each frame loaded into a World and written back; the byte-equal frames counted, the differing
//!   paths and what the loads could not set listed.
//! - `--bot-log <dir>`: each match stepped as in the default mode, nothing compared: its bots' log written to
//!   `<dir>/<name>.botlog` (`mvs_sim_trace::bot_log`; `bot-log-diff` compares it with the oracle's).
//! - `--record <dir>`: each match of `matches.json` stepped from its definition on the store's data, its trace
//!   written to `<dir>/<name>.trace`: the baseline a plain run with `--traces <dir>` compares a change with.
//!
//! Which matches: `--match <name>` (repeatable); `--changed [<rev>]`, the matches whose sim code `git diff <rev>`
//! touches (`coverage/map.txt`; default rev HEAD's merge base with main); `--all`, every trace of the folder; none of
//! them, the kept set (`coverage/kept.txt`, the matches that each run sim code no other kept match runs) and the
//! traces the map does not know.
//!
//! Options: `--until <tick>`, `--out <dir>` (the Rust traces written there),
//! `--traces <dir>` (default: `MVS_TRACES`, else the main tree's `.scratch/traces`), `--fields <n>` (differences
//! printed per mismatch), `--owner <module prefix or source file>` (repeatable: only the differences those modules own;
//! a prefix no owner has or a file that does not exist is refused, and so is `--owner` with `--round-trip`, which
//! compares whole frames; a seeded run counts a step that panicked outside the module as blocked), `--not <path
//! prefix>` (repeatable: drops the differences under that record path), `--stage-b` (only the differences of the fields
//! the float32 sim must not move, `mvs_trace::stable`), `--all-paths` (the known gaps counted too),
//! `--ignore-header` (run a trace whose header names another store), `--jobs <n>` (matches run at once; default half
//! the machine's threads, fewer when their memory would pass a quarter of its RAM), `--help`. Each match runs on a
//! worker thread below normal priority; the output is printed in the list's order, then a line on stderr with the
//! count, the time and the peak memory.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use mvs_sim_trace::coverage::{self, Pick};
use mvs_sim_trace::run::{self, Checked, Filter, describe};
use mvs_trace::Trace;

#[derive(PartialEq)]
enum Mode {
    Run,
    Seeded,
    RoundTrip,
    BotLog(PathBuf),
    Record(PathBuf),
}

struct Args {
    mode: Mode,
    matches: Vec<String>,
    /// `--all`: every trace of the folder, not the kept set.
    all: bool,
    /// `--changed [<rev>]`: the revision, or none for the default.
    changed: Option<Option<String>>,
    from: usize,
    until: Option<usize>,
    show: usize,
    out: Option<PathBuf>,
    traces: PathBuf,
    fields: usize,
    filter: Filter,
    ignore_header: bool,
    jobs: usize,
}

const USAGE: &str = "usage: sim-trace [--seeded [--from <tick>] [--show <n>] | --round-trip | --bot-log <dir> | \
                     --record <dir>] \
                     [--match <name>... | --changed [<rev>] | --all] \
                     [--until <tick>] [--out <dir>] [--traces <dir>] [--fields <n>] [--owner <module|file>] \
                     [--not <path>]... [--stage-b] [--all-paths] [--ignore-header] [--jobs <n>] [--help]";

const HELP: &str = "\
modes:
  (none)              each match stepped from its header with the recorded inputs; one line a match
  --seeded            each tick stepped from the oracle frame before it; the matching ticks counted
  --round-trip        each frame loaded and written back
  --bot-log <dir>     each match stepped as by default, nothing compared: what its bots' trees did per think (the
                      tasks executed and aborted, each decorator's result, each service's schedule) written to
                      <dir>/<name>.botlog, the lines the oracle's replay writes (its
                      bot-log option); bot-log-diff <oracle> <ours> compares them
  --record <dir>      each match of matches.json (or --match) stepped from its definition on the store's
                      data and written to <dir>/<name>.trace: a baseline, made before a change, that a plain
                      run with --traces <dir> compares with after it (takes --match and --jobs only)
options:
  --match <name>      run this match only (repeatable)
  --changed [<rev>]   the matches whose sim code git diff <rev> (the working tree included) changes, by the
                      coverage map (crates/mvs-sim-trace/coverage); rev defaults to HEAD's merge base with main.
                      Prints what it picked and why; a change the map cannot place, or one to the record, the
                      trace tools or a manifest, runs the default set; one outside the sim runs nothing
  --all               every trace of the folder
                      (none of the three: the kept set, coverage/kept.txt, and the traces the map lacks)
  --until <tick>      stop a match there
  --from <tick>       (seeded) start there
  --show <n>          (seeded) print the first n mismatching ticks (3)
  --out <dir>         write the Rust traces there (run mode)
  --traces <dir>      the traces compared with (default: a plain run and a round trip the Rust baseline,
                      MVS_RUST_TRACES else the main tree's .scratch/traces-<batch>; --seeded and --stage-b the
                      oracle's, MVS_TRACES else the main tree's .scratch/traces)
  --fields <n>        differences or paths printed (3)
  --owner <m>         only the differences of the fields module m owns (a prefix, as fighter::body), or of a
                      source file under crates/mvs-sim/src (chars/creature/tether.rs; mod.rs is its folder's
                      module). A seeded run counts a tick whose step panicked outside m as blocked, not a
                      mismatch, and prints each blocking place once with its tick count. Repeatable: a
                      difference any of them owns is kept (a track's files in one run)
  --not <path>        drop the differences whose record path starts with it (repeatable; run and seeded)
  --stage-b           only the differences of the stage-B class (mvs_trace::stable: the events, states, names,
                      ids, counts, ticks, inputs and the HUD's damage; not the floats); run and seeded
  --all-paths         count the known gaps too
  --ignore-header     run a trace whose header names another store
  --jobs <n>          matches run at once (default half the machine's threads, fewer when their peak memory
                      would pass a quarter of its RAM; 1 runs them in turn). The run is below normal priority
  --help              this text";

fn args() -> Result<Args, String> {
    let mut a = Args {
        mode: Mode::Run,
        matches: Vec::new(),
        all: false,
        changed: None,
        from: 0,
        until: None,
        show: 3,
        out: None,
        traces: PathBuf::new(),
        fields: 3,
        filter: Filter { skip_gaps: true, owner: Vec::new(), not: Vec::new(), stable: false },
        ignore_header: false,
        jobs: mvs_sim_trace::machine::default_jobs(),
    };
    let mut it = std::env::args().skip(1).peekable();
    while let Some(s) = it.next() {
        if s == "--changed" {
            a.changed = Some(it.next_if(|r| !r.starts_with("--")));
            continue;
        }
        let mut val = |what: &str| it.next().ok_or(format!("{s} needs {what}"));
        match s.as_str() {
            "--match" => a.matches.push(val("a match name")?),
            "--all" => a.all = true,
            "--until" => a.until = Some(val("a tick")?.parse().map_err(|_| "bad --until")?),
            // (ticks are printed from 1: tick t is frame t - 1)
            "--from" => a.from = val("a tick")?.parse::<usize>().map_err(|_| "bad --from")?.saturating_sub(1),
            "--show" => a.show = val("a count")?.parse().map_err(|_| "bad --show")?,
            "--out" => a.out = Some(PathBuf::from(val("a directory")?)),
            "--traces" => a.traces = PathBuf::from(val("a directory")?),
            "--fields" => a.fields = val("a count")?.parse().map_err(|_| "bad --fields")?,
            "--owner" => {
                let o = val("a module prefix or a source file")?;
                if o.ends_with(".rs") {
                    a.filter.owner.push(mvs_sim_trace::module_of_existing_file(&o)?);
                } else if mvs_sim_trace::is_owner_prefix(&o) {
                    a.filter.owner.push(o);
                } else {
                    return Err(format!(
                        "--owner {o} names no owner (a module path with ::, as fighter::body or combat)"
                    ));
                }
            }
            "--not" => a.filter.not.push(val("a record path prefix")?),
            // (the empty error is the help request)
            "--help" | "-h" => return Err(String::new()),
            "--all-paths" => a.filter.skip_gaps = false,
            "--stage-b" => a.filter.stable = true,
            "--ignore-header" => a.ignore_header = true,
            "--jobs" => a.jobs = val("a count")?.parse::<usize>().map_err(|_| "bad --jobs")?.max(1),
            "--seeded" => a.mode = Mode::Seeded,
            "--round-trip" => a.mode = Mode::RoundTrip,
            "--bot-log" => a.mode = Mode::BotLog(PathBuf::from(val("a directory")?)),
            "--record" => a.mode = Mode::Record(PathBuf::from(val("a directory")?)),
            _ => return Err(format!("unknown option {s}\n{USAGE}")),
        }
    }
    // The traces when none are named: a seeded or a stage-B run reads the oracle's (it steps from oracle frames, or
    // compares the class the float32 sim must not move); a plain run and a round trip the Rust baseline (the gate
    // since stage B), or the oracle's by the stage-B class when MVS_TRACES names them (run::baseline).
    if a.traces.as_os_str().is_empty() {
        if a.mode == Mode::Seeded || a.filter.stable {
            a.traces = run::default_traces();
        } else {
            let b = run::baseline();
            a.traces = b.dir;
            a.filter.stable |= b.stable && a.mode == Mode::Run;
        }
    }
    if !a.filter.owner.is_empty() && a.mode == Mode::RoundTrip {
        return Err("--owner does not apply to --round-trip (it compares whole frames)".to_owned());
    }
    if a.mode == Mode::RoundTrip && a.filter.stable {
        return Err("--stage-b does not apply to --round-trip (it compares whole frames)".to_owned());
    }
    if a.mode == Mode::RoundTrip && !a.filter.not.is_empty() {
        return Err("--not does not apply to --round-trip (it compares whole frames)".to_owned());
    }
    if usize::from(!a.matches.is_empty()) + usize::from(a.all) + usize::from(a.changed.is_some()) > 1 {
        return Err("--match, --changed and --all each choose the matches: name one".to_owned());
    }
    if matches!(a.mode, Mode::BotLog(_))
        && (!a.filter.owner.is_empty() || a.filter.stable || !a.filter.not.is_empty() || a.out.is_some())
    {
        return Err("--owner, --stage-b, --not and --out do not apply to --bot-log (it compares nothing)".to_owned());
    }
    if matches!(a.mode, Mode::Record(_))
        && (a.all
            || a.changed.is_some()
            || a.until.is_some()
            || a.out.is_some()
            || !a.filter.owner.is_empty()
            || !a.filter.not.is_empty()
            || a.ignore_header)
    {
        return Err("--record takes --match and --jobs only (it records every match by default)".to_owned());
    }
    if a.mode != Mode::Seeded && (a.from > 0 || a.show != 3) {
        return Err("--from and --show apply to --seeded only".to_owned());
    }
    Ok(a)
}

fn main() -> ExitCode {
    match main_() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) if e.is_empty() => {
            println!("{USAGE}\n{HELP}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("sim-trace: {e}");
            ExitCode::from(2)
        }
    }
}

/// Whether every match was the same.
fn main_() -> Result<bool, String> {
    let a = args()?;
    mvs_sim_trace::machine::lower_priority();
    let started = std::time::Instant::now();
    if let Mode::Record(out) = &a.mode {
        return record_all(&a, out, &open_store()?, started);
    }
    let Some(list) = chosen(&a)? else { return Ok(true) };
    let store = open_store()?;
    for o in a.out.iter().chain(match &a.mode {
        Mode::BotLog(d) => Some(d),
        _ => None,
    }) {
        std::fs::create_dir_all(o).map_err(|e| format!("{}: {e}", o.display()))?;
    }
    let jobs = a.jobs.min(list.len());
    // The largest traces start first (the longest matches: the run ends soonest when they do not start last); the
    // lines are still printed in the list's order.
    let mut order: Vec<usize> = (0..list.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(std::fs::metadata(&list[i].1).map_or(0, |m| m.len())));
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel::<(usize, Result<(String, bool), String>)>();
    let mut all_same = true;
    let mut first_err = None;
    std::thread::scope(|sc| {
        for _ in 0..jobs {
            let tx = tx.clone();
            let (next, list, order, a, store) = (&next, &list, &order, &a, &store);
            sc.spawn(move || {
                while let Some(&i) = order.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let (name, path) = &list[i];
                    if tx.send((i, one(name, path, a, store))).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        // Printed in the list's order as each next one is done.
        let mut done: Vec<Option<Result<(String, bool), String>>> = (0..list.len()).map(|_| None).collect();
        let mut shown = 0;
        for (i, r) in rx {
            done[i] = Some(r);
            while let Some(r) = done.get_mut(shown).and_then(Option::take) {
                match r {
                    Ok((out, same)) => {
                        print!("{out}");
                        all_same &= same;
                    }
                    Err(e) => {
                        first_err.get_or_insert(e);
                    }
                }
                shown += 1;
            }
        }
    });
    if let Some(e) = first_err {
        return Err(e);
    }
    let peak = mvs_sim_trace::machine::peak_memory().map_or(String::new(), |b| format!(", peak memory {} MB", b >> 20));
    eprintln!("sim-trace: {} matches, {jobs} jobs, {:.1} s{peak}", list.len(), started.elapsed().as_secs_f64());
    Ok(all_same)
}

/// The store the matches' data comes from (`CONTENT_STORE`, else the tree's `content/.store`).
fn open_store() -> Result<mvs_store::Store, String> {
    let dir = mvs_store::dev_store_dir();
    mvs_store::Store::open(&dir).map_err(|e| format!("the store at {}: {e:?}", dir.display()))
}

/// `--record`: each match of the list (or those `--match` names) recorded into `out`, a few at once; one line a
/// match, in the list's order. Whether every match recorded.
fn record_all(a: &Args, out: &Path, store: &mvs_store::Store, started: std::time::Instant) -> Result<bool, String> {
    let all = mvs_sim_trace::matches::all()?;
    let name = |m: &serde_json::Value| m["name"].as_str().unwrap_or_default().to_owned();
    for n in &a.matches {
        if !all.iter().any(|m| &name(m) == n) {
            return Err(format!("--match {n}: no such match in matches.json"));
        }
    }
    let list: Vec<&serde_json::Value> =
        all.iter().filter(|m| a.matches.is_empty() || a.matches.contains(&name(m))).collect();
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let jobs = a.jobs.min(list.len()).max(1);
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel::<(usize, String, bool)>();
    let mut ok = true;
    std::thread::scope(|sc| {
        for _ in 0..jobs {
            let tx = tx.clone();
            let (next, list) = (&next, &list);
            sc.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(m) = list.get(i) else { break };
                    let n = name(m);
                    let (line, good) = match mvs_sim_trace::matches::record(m, store) {
                        Ok(r) => match std::fs::write(out.join(format!("{n}.trace")), &r.bytes) {
                            Ok(()) => (format!("{n}: {} ticks ({})\n", r.ticks, r.stop), true),
                            Err(e) => (format!("{n}: not written: {e}\n"), false),
                        },
                        Err(e) => (format!("{n}: not recorded: {e}\n"), false),
                    };
                    if tx.send((i, line, good)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        let mut done: Vec<Option<(String, bool)>> = (0..list.len()).map(|_| None).collect();
        let mut shown = 0;
        for (i, line, good) in rx {
            done[i] = Some((line, good));
            while let Some((line, good)) = done.get_mut(shown).and_then(Option::take) {
                print!("{line}");
                ok &= good;
                shown += 1;
            }
        }
    });
    eprintln!(
        "sim-trace: {} matches recorded into {}, {jobs} jobs, {:.1} s",
        list.len(),
        out.display(),
        started.elapsed().as_secs_f64()
    );
    Ok(ok)
}

/// The traces the run checks (`--match`, `--changed`, `--all` or the default set), or `None` when `--changed`
/// picked none. What `--changed` and the default set chose is printed on stderr.
fn chosen(a: &Args) -> Result<Option<Vec<(String, PathBuf)>>, String> {
    let empty = || format!("no traces in {} (the oracle's trace run writes them)", a.traces.display());
    if !a.matches.is_empty() || a.all {
        let list = run::traces(&a.traces, &a.matches)?;
        return if list.is_empty() { Err(empty()) } else { Ok(Some(list)) };
    }
    let all = run::traces(&a.traces, &[])?;
    if all.is_empty() {
        return Err(empty());
    }
    let dir = coverage::dir();
    let map = coverage::Map::read(&dir.join("map.txt"))?;
    // (a trace the map does not know is new since the map: neither its reach nor its share of the cover is known)
    let unmapped: Vec<&String> = all.iter().map(|(n, _)| n).filter(|n| !map.matches.contains(n)).collect();
    let new = if unmapped.is_empty() { String::new() } else { format!(" and {} the map lacks", unmapped.len()) };
    let kept = coverage::read_kept(&dir.join("kept.txt"))?;
    let default = |why: &str| {
        let list: Vec<(String, PathBuf)> =
            all.iter().filter(|(n, _)| kept.contains(n) || unmapped.contains(&n)).cloned().collect();
        eprintln!(
            "sim-trace: {why}the kept set, {} of {} traces{new} (--all runs every one)",
            list.len() - unmapped.len(),
            all.len()
        );
        list
    };
    let Some(rev) = &a.changed else { return Ok(Some(default(""))) };
    let rev = rev.clone().unwrap_or_else(|| coverage::default_rev(&map));
    match coverage::changed(&rev, &map)? {
        Pick::Nothing(why) => {
            eprintln!("sim-trace --changed: {why}: no match to run");
            Ok(None)
        }
        Pick::Default(why) => Ok(Some(default(&format!("--changed: {why}: ")))),
        Pick::Matches(picked, why) => {
            let names: Vec<&String> = picked.iter().map(|&i| &map.matches[i]).collect();
            let list: Vec<(String, PathBuf)> =
                all.iter().filter(|(n, _)| names.contains(&n) || unmapped.contains(&n)).cloned().collect();
            eprintln!(
                "sim-trace --changed: {} of {} matches{new} reach the code changed {why}: {}",
                names.len(),
                map.matches.len(),
                list.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(" ")
            );
            Ok(Some(list))
        }
    }
}

/// One match's lines and whether it was the same.
fn one(name: &str, path: &Path, a: &Args, store: &mvs_store::Store) -> Result<(String, bool), String> {
    let mut o = String::new();
    let mut same = true;
    let t = Trace::read(path).map_err(|e| e.to_string())?;
    let header = mvs_sim_trace::store_differences(&t.header, store);
    if !header.is_empty() {
        let _ = writeln!(
            o,
            "{name}: the header names another store{}",
            if a.ignore_header { " (--ignore-header)" } else { "" }
        );
        for h in &header {
            let _ = writeln!(o, "    {h}");
        }
        if !a.ignore_header {
            return Ok((o, false));
        }
    }
    match &a.mode {
        Mode::Run => {
            let c = run::run_match(&t, store, a.until, &a.filter, 1, a.out.is_some())?;
            if let (Some(o), Some(bytes)) = (&a.out, &c.trace) {
                let p = o.join(format!("{name}.trace"));
                std::fs::write(&p, bytes).map_err(|e| format!("{}: {e}", p.display()))?;
            }
            same &= c.matching == c.ticks;
            match c.first.first() {
                None => {
                    let _ = writeln!(o, "{name}: same ({} ticks){}", c.ticks, scope(&a.filter));
                }
                Some(tk) => {
                    let _ = writeln!(o, "{name}: differs at tick {}{}", tk.tick + 1, scope(&a.filter));
                    print_tick(&mut o, &t, tk, a.fields)?;
                }
            }
        }
        Mode::Seeded => {
            let c = run::seeded(&t, store, a.from, a.until, &a.filter, a.show)?;
            // (a tick blocked by another module's panic is not the owner's: the owner's check passes without it)
            same &= c.matching + c.blocked == c.ticks;
            let _ = write!(o, "{name}: seeded {} of {} ticks match", c.matching, c.ticks);
            if !a.filter.owner.is_empty() {
                let _ = write!(o, " ({} mismatched, {} blocked)", c.ticks - c.matching - c.blocked, c.blocked);
            }
            let _ = writeln!(o, "{}", scope(&a.filter));
            print_checked(&mut o, &t, &c, a.fields)?;
        }
        Mode::RoundTrip => {
            let rt = run::round_trip(&t, store, a.until)?;
            same &= rt.equal == rt.frames;
            let _ = writeln!(o, "{name}: round trip {} of {} frames byte-equal", rt.equal, rt.frames);
            if let Some((i, diffs)) = &rt.first {
                let f = t.frame(*i).map_err(|e| e.to_string())?;
                for d in diffs.iter().take(a.fields) {
                    let _ = writeln!(o, "    frame {i}: {}", describe(d, &f));
                }
            }
            print_paths(&mut o, "    differing", &rt.paths, a.fields);
            print_paths(&mut o, "    not loaded", &rt.misses, a.fields);
            print_paths(&mut o, "    not written", &rt.unmapped, a.fields);
        }
        Mode::BotLog(dir) => {
            let l = mvs_sim_trace::bot_log::bot_log(&t, store, a.until)?;
            let p = dir.join(format!("{name}.botlog"));
            std::fs::write(&p, &l.text).map_err(|e| format!("{}: {e}", p.display()))?;
            let _ = writeln!(o, "{name}: bot log of {} frames, {} thinks: {}", l.ticks, l.thinks, p.display());
            if let Some((i, e)) = &l.panic {
                same = false;
                let _ = writeln!(o, "    frame {i}: panicked: {e}");
            }
        }
        Mode::Record(_) => unreachable!("--record runs from record_all"),
    }
    Ok((o, same))
}

fn scope(f: &Filter) -> String {
    let mut s = String::new();
    if !f.owner.is_empty() {
        s += &format!(" in {}'s fields", f.owner.join(", "));
    }
    for n in &f.not {
        s += &format!(" without {n}");
    }
    if f.stable {
        s += " (stage-B class)";
    }
    if !f.skip_gaps {
        s += " (the known gaps counted)";
    }
    s
}

fn print_checked(o: &mut String, t: &Trace, c: &Checked, fields: usize) -> Result<(), String> {
    if c.panics > 0 {
        let _ = writeln!(o, "    {} ticks panicked", c.panics);
    }
    for tk in &c.first {
        print_tick(o, t, tk, fields)?;
    }
    print_paths(o, "    differing", &c.paths, fields);
    for (place, n) in &c.blocked_by {
        let _ = writeln!(o, "    blocked by {place}: {n} ticks");
    }
    Ok(())
}

/// A tick that did not match: the panic, then what the load could not set (a bad load, not the step), what the
/// writer could not write, then the differences.
fn print_tick(o: &mut String, t: &Trace, tk: &run::Tick, fields: usize) -> Result<(), String> {
    let at = tk.tick + 1;
    if let Some(p) = &tk.panic {
        let _ = writeln!(o, "    tick {at}: the step panicked: {p}");
    }
    for m in tk.misses.iter().take(fields) {
        let _ = writeln!(o, "    tick {at}: not loaded {m}");
    }
    for u in tk.unmapped.iter().take(fields) {
        let _ = writeln!(o, "    tick {at}: not written {u}");
    }
    if !tk.diffs.is_empty() {
        let f = t.frame(tk.tick).map_err(|e| e.to_string())?;
        let _ = writeln!(o, "    tick {at} ({} fields):", tk.diffs.len());
        for d in tk.diffs.iter().take(fields) {
            let _ = writeln!(o, "      {}", describe(d, &f));
        }
    }
    Ok(())
}

fn print_paths(o: &mut String, what: &str, m: &indexmap::IndexMap<String, usize>, n: usize) {
    let mut v: Vec<(&String, &usize)> = m.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (p, c) in v.iter().take(n) {
        let _ = writeln!(o, "{what} {p} ({c})");
    }
    if v.len() > n {
        let _ = writeln!(o, "{what} ... {} more", v.len() - n);
    }
}
