//! Reads a trace: `trace-show <file>` prints its header (the schema left out) and size; `--tick <n>` (repeatable,
//! `a..b` for a range) prints frame n (its index in the file) one field a line; `--path <prefix>` keeps the fields
//! whose path starts with the prefix; `--grep <text>` keeps the lines that hold the text; `--changed` keeps a field
//! only on the frames where its value differs from the frame before; `--schema` prints the schema. A frame with no
//! line kept is not printed.

use std::collections::HashMap;
use std::process::ExitCode;

use mvs_trace::{Field, Trace};

const USAGE: &str = "usage: trace-show <file> [--tick <n>|<a>..<b>]... [--path <prefix>]... [--grep <text>] [-i] \
                     [--changed] [--schema]\n  \
                     --tick     frames to print (index in the file; repeatable; a..b a range)\n  \
                     --path     keep fields whose path starts with the prefix (repeatable; e.g. world.fighters[0].buffs)\n  \
                     --grep     keep lines holding the text\n  \
                     -i         --grep ignores case\n  \
                     --changed  keep a field only where its value differs from the previous frame printed or read\n  \
                     --schema   print the schema's leaves\n\
                     With no --tick: the header and the size.";

fn run() -> Result<(), String> {
    let mut file = None;
    let mut ticks: Vec<(usize, usize)> = Vec::new();
    let mut paths: Vec<String> = Vec::new();
    let mut grep: Option<String> = None;
    let mut changed = false;
    let mut nocase = false;
    let mut schema = false;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--tick" => {
                let v = it.next().ok_or("--tick needs a frame")?;
                let (lo, hi) = match v.split_once("..") {
                    Some((lo, hi)) => (lo.parse(), hi.parse()),
                    None => (v.parse(), v.parse()),
                };
                ticks.push((lo.map_err(|_| "bad --tick")?, hi.map_err(|_| "bad --tick")?));
            }
            "--path" => paths.push(it.next().ok_or("--path needs a prefix")?),
            "--grep" => grep = Some(it.next().ok_or("--grep needs text")?),
            "--changed" => changed = true,
            "-i" | "--ignore-case" => nocase = true,
            "--schema" => schema = true,
            _ if a.starts_with("--") => return Err(format!("unknown option {a}\n{USAGE}")),
            _ => file = Some(a),
        }
    }
    let file = file.ok_or(USAGE)?;
    let grep = grep.map(|g| if nocase { g.to_lowercase() } else { g });
    let t = Trace::read(file.as_ref()).map_err(|e| e.to_string())?;
    if schema {
        for s in &t.header.schema {
            println!("{s}");
        }
        return Ok(());
    }
    if ticks.is_empty() {
        let mut h = serde_json::to_value(&t.header).map_err(|e| e.to_string())?;
        if let Some(o) = h.as_object_mut() {
            o.insert("schema".into(), format!("{} leaves", t.header.schema.len()).into());
        }
        println!("{}", serde_json::to_string_pretty(&h).map_err(|e| e.to_string())?);
        let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        println!("{} frames, {size} bytes", t.len());
        return Ok(());
    }
    // The last value seen per path, for --changed.
    let mut last: HashMap<String, String> = HashMap::new();
    for (lo, hi) in ticks {
        for i in lo..=hi {
            if i >= t.len() {
                return Err(format!("frame {i}: the trace has {}", t.len()));
            }
            let f = t.frame(i).map_err(|e| format!("frame {i}: {e}"))?;
            let mut tree = String::new();
            f.show_tree(&mut String::new(), &mut tree);
            let mut kept = Vec::new();
            for l in tree.lines() {
                let (path, value) = l.split_once(" = ").unwrap_or((l, ""));
                if !paths.is_empty() && !paths.iter().any(|p| path.starts_with(p.as_str())) {
                    continue;
                }
                if changed {
                    let same = last.get(path).is_some_and(|v| v == value);
                    last.insert(path.to_string(), value.to_string());
                    if same {
                        continue;
                    }
                }
                if grep.as_ref().is_some_and(|g| {
                    if nocase { !l.to_lowercase().contains(g.as_str()) } else { !l.contains(g.as_str()) }
                }) {
                    continue;
                }
                kept.push(l);
            }
            if kept.is_empty() {
                continue;
            }
            println!("frame {i} (World.tick {})", f.world.tick);
            for l in kept {
                println!("  {l}");
            }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
