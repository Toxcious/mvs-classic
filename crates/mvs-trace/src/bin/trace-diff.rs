//! Compares traces: `trace-diff <expected> <actual> [--all] [--around <n>] [--ignore-header] [--stage-b]`, two trace
//! files or two folders (each `*.trace` of the first against the one of the same name in the second). Per match it
//! first checks
//! the headers name the same run (the match's name, its set-up, the World seed, the store's build id and kind
//! versions): a mismatch is reported and the frames are not compared, unless `--ignore-header`. Then it prints
//! `same`, or the first differing tick, its first field (every field with `--all`) with both values, and that field
//! over the ticks around it on both sides. `--stage-b` counts only the differences of the fields the float32 sim
//! must not move (`mvs_trace::stable`). Exit 1 when a match differs, 2 on a bad call or an unreadable file.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mvs_trace::{Field, Header, Outcome, Trace, compare_kept, stable};

struct Args {
    expected: PathBuf,
    actual: PathBuf,
    all: bool,
    around: usize,
    ignore_header: bool,
    stage_b: bool,
}

fn args() -> Result<Args, String> {
    let mut pos = Vec::new();
    let (mut all, mut around, mut ignore_header, mut stage_b) = (false, 3, false, false);
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--all" => all = true,
            "--ignore-header" => ignore_header = true,
            "--stage-b" => stage_b = true,
            "--around" => around = it.next().and_then(|v| v.parse().ok()).ok_or("--around needs a count")?,
            _ if a.starts_with("--") => return Err(format!("unknown option {a}")),
            _ => pos.push(PathBuf::from(a)),
        }
    }
    if pos.len() != 2 {
        return Err("usage: trace-diff <expected> <actual> [--all] [--around <n>] [--ignore-header] [--stage-b]".into());
    }
    let actual = pos.pop().expect("two");
    let expected = pos.pop().expect("two");
    Ok(Args { expected, actual, all, around, ignore_header, stage_b })
}

/// The pairs to compare: the two files, or each `*.trace` of the first folder with its namesake in the second.
fn pairs(a: &Args) -> Result<Vec<(String, PathBuf, PathBuf)>, String> {
    if !a.expected.is_dir() {
        let name = a.expected.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        return Ok(vec![(name, a.expected.clone(), a.actual.clone())]);
    }
    let mut out = Vec::new();
    let rd = std::fs::read_dir(&a.expected).map_err(|e| format!("{}: {e}", a.expected.display()))?;
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "trace") {
            let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            out.push((name, p.clone(), a.actual.join(e.file_name())));
        }
    }
    out.sort();
    if out.is_empty() {
        return Err(format!("no .trace file in {}", a.expected.display()));
    }
    Ok(out)
}

/// What the two headers disagree on of the run they name: the match, its set-up, the seed and the store.
fn header_differences(e: &Header, a: &Header) -> Vec<String> {
    let mut out = Vec::new();
    let mut check = |what: &str, x: &serde_json::Value, y: &serde_json::Value| {
        if x != y {
            out.push(format!("{what}: expected {x}, actual {y}"));
        }
    };
    check("name", &e.name.clone().into(), &a.name.clone().into());
    check("setup", &e.setup, &a.setup);
    check("worldSeed", &e.world_seed.into(), &a.world_seed.into());
    let none = serde_json::Value::Null;
    check("store.buildId", e.store.get("buildId").unwrap_or(&none), a.store.get("buildId").unwrap_or(&none));
    check("store.kinds", e.store.get("kinds").unwrap_or(&none), a.store.get("kinds").unwrap_or(&none));
    out
}

/// A field's value in frame `i` of `t`, by its path (`-` when the frame has no such field).
fn value_at(t: &Trace, i: usize, path: &str) -> String {
    if i >= t.len() {
        return "(no frame)".into();
    }
    match t.frame(i) {
        Ok(f) => f.find(path).unwrap_or_else(|| "-".into()),
        Err(_) => "(unreadable)".into(),
    }
}

fn world_tick(t: &Trace, i: usize) -> String {
    t.frame(i).map(|f| f.world.tick.to_string()).unwrap_or_else(|_| "?".into())
}

fn one(name: &str, e: &Path, a: &Path, args: &Args) -> Result<bool, String> {
    if !a.exists() {
        println!("{name}: no actual trace ({})", a.display());
        return Ok(false);
    }
    let te = Trace::read(e).map_err(|x| x.to_string())?;
    let ta = Trace::read(a).map_err(|x| x.to_string())?;
    let header = header_differences(&te.header, &ta.header);
    if !header.is_empty() {
        println!("{name}: the headers name another run{}", if args.ignore_header { " (--ignore-header)" } else { "" });
        for h in &header {
            println!("  {h}");
        }
        if !args.ignore_header {
            return Ok(false);
        }
    }
    let keep = |d: &mvs_trace::Difference| !args.stage_b || stable::keeps(d);
    match compare_kept(&te, &ta, keep).map_err(|x| x.to_string())? {
        Outcome::Same { ticks } => {
            println!("{name}: same ({ticks} ticks)");
            Ok(header.is_empty() || args.ignore_header)
        }
        Outcome::Length { expected, actual } => {
            println!(
                "{name}: the first {} ticks the same; expected {expected} ticks, actual {actual}",
                expected.min(actual)
            );
            Ok(false)
        }
        Outcome::Differs { tick, fields } => {
            println!(
                "{name}: differs at frame {tick} (World.tick {}), {} field(s)",
                world_tick(&te, tick),
                fields.len()
            );
            let shown = if args.all { fields.len() } else { 1 };
            for d in &fields[..shown] {
                println!("  {}", d.path);
                println!("    expected {}", d.expected);
                println!("    actual   {}", d.actual);
            }
            if !args.all && fields.len() > 1 {
                println!("  (+{} more in this frame: --all)", fields.len() - 1);
            }
            let path = &fields[0].path;
            println!("  {path} around it:");
            for i in tick.saturating_sub(args.around)..=tick + args.around {
                if i >= te.len() && i >= ta.len() {
                    break;
                }
                let mark = if i == tick { '>' } else { ' ' };
                println!("  {mark} frame {i}");
                println!("      expected {}", value_at(&te, i, path));
                println!("      actual   {}", value_at(&ta, i, path));
            }
            Ok(false)
        }
    }
}

fn main() -> ExitCode {
    let args = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let pairs = match pairs(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let mut same = true;
    for (name, e, a) in &pairs {
        match one(name, e, a, &args) {
            Ok(s) => same &= s,
            Err(x) => {
                eprintln!("{name}: {x}");
                return ExitCode::from(2);
            }
        }
    }
    if same { ExitCode::SUCCESS } else { ExitCode::from(1) }
}
