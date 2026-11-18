//! Compares two menu tapes (`.zst` or plain lines) of one script and prints the report: the frame counts, the frames
//! that differ (the first ones, with their differences), the root owners only one side has, and the counts per
//! field. Exits 1 when the tapes differ.
//!
//! Usage: menu-tape-diff <oracle.menutape.zst> <native.menutape.zst> [--frames <n>] [--lines <n>] [--ignore <field,..>]
//! [--from <n>] [--to <n>] [--path <text>]... [--summary <n>]
//! (`--frames`: how many differing frames are listed, default 5; `--lines`: differences listed per frame, default 12;
//! `--ignore`: fields not counted, by the report's names: type, brush, painted opacity, ...; `--from` / `--to`: only
//! the frames in that range (inclusive) are compared; `--path`: only differences whose root or widget path holds one
//! of the texts; `--summary`: the n (root, widget, field) groups with the most differing frames, with their frames)

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use mvs_ui::menu::tape::{MenuTape, compare, compare_frame};

fn main() -> ExitCode {
    match run() {
        Ok(same) => {
            if same {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("menu-tape-diff: {e}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool, String> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut list_frames = 5usize;
    let mut list_lines = 12usize;
    let mut ignore: Vec<String> = Vec::new();
    let (mut from, mut to) = (0u32, u32::MAX);
    let mut only_paths: Vec<String> = Vec::new();
    let mut summary = 0usize;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--frames" => list_frames = it.next().and_then(|v| v.parse().ok()).ok_or("--frames needs a number")?,
            "--lines" => list_lines = it.next().and_then(|v| v.parse().ok()).ok_or("--lines needs a number")?,
            "--ignore" => ignore = it.next().ok_or("--ignore needs fields")?.split(',').map(str::to_string).collect(),
            "--from" => from = it.next().and_then(|v| v.parse().ok()).ok_or("--from needs a number")?,
            "--to" => to = it.next().and_then(|v| v.parse().ok()).ok_or("--to needs a number")?,
            "--path" => only_paths.push(it.next().ok_or("--path needs a text")?),
            "--summary" => summary = it.next().and_then(|v| v.parse().ok()).ok_or("--summary needs a number")?,
            _ => paths.push(PathBuf::from(a)),
        }
    }
    let [oracle_path, native_path] = paths.as_slice() else {
        return Err("usage: menu-tape-diff <oracle tape> <native tape> [--frames n] [--lines n] [--ignore fields]                     [--from n] [--to n] [--path text]... [--summary n]"
            .into());
    };
    let oracle = MenuTape::read(oracle_path).map_err(|e| e.to_string())?;
    let native = MenuTape::read(native_path).map_err(|e| e.to_string())?;
    let report = compare(&oracle, &native);
    let mut differing = 0usize;

    let mut shown = 0usize;
    let mut by_field: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut only_oracle: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    let mut only_native: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    // (root, widget, field) -> the frames it differs in
    let mut groups: BTreeMap<(String, String, &'static str), Vec<u32>> = BTreeMap::new();
    for o in &oracle.frames {
        if o.frame < from || o.frame > to {
            continue;
        }
        let Some(n) = native.frame(o.frame) else { continue };
        for r in &o.roots {
            if !n.roots.iter().any(|x| x.owner == r.owner) {
                only_oracle.entry(r.owner.clone()).or_default().push(o.frame);
            }
        }
        for r in &n.roots {
            if !o.roots.iter().any(|x| x.owner == r.owner) {
                only_native.entry(r.owner.clone()).or_default().push(o.frame);
            }
        }
        let mut diffs = compare_frame(o, n);
        diffs.retain(|d| !ignore.iter().any(|i| i == d.field));
        if !only_paths.is_empty() {
            diffs.retain(|d| only_paths.iter().any(|p| d.root.contains(p.as_str()) || d.path.contains(p.as_str())));
        }
        if diffs.is_empty() {
            continue;
        }
        differing += 1;
        for d in &diffs {
            *by_field.entry(d.field).or_default() += 1;
            let widget = d.path.rsplit('/').next().unwrap_or(&d.path).to_string();
            let g = groups.entry((d.root.to_string(), widget, d.field)).or_default();
            if g.last() != Some(&o.frame) {
                g.push(o.frame);
            }
        }
        if shown < list_frames {
            shown += 1;
            println!("frame {}: {} differences", o.frame, diffs.len());
            for d in diffs.iter().take(list_lines) {
                println!("  [{}] {} {}: oracle {} native {}", d.root, d.path, d.field, d.oracle, d.native);
            }
            if diffs.len() > list_lines {
                println!("  ... {} more", diffs.len() - list_lines);
            }
        }
    }
    println!(
        "oracle {} frames, native {} frames, {} compared, {} differ",
        report.lengths.0, report.lengths.1, report.compared, differing
    );
    for (title, map) in [("root only in the oracle", &only_oracle), ("root only in the native", &only_native)] {
        for (owner, frames) in map {
            println!("{title}: {owner} in {} frames ({}..{})", frames.len(), frames[0], frames[frames.len() - 1]);
        }
    }
    if !by_field.is_empty() {
        let counts: Vec<String> = by_field.iter().map(|(k, v)| format!("{k} {v}")).collect();
        println!("differences by field: {}", counts.join(", "));
    }
    if summary > 0 {
        let mut gs: Vec<_> = groups.iter().collect();
        gs.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(b.0)));
        println!("by widget ({} groups):", gs.len());
        for ((root, widget, field), frames) in gs.into_iter().take(summary) {
            println!(
                "  [{root}] {widget} {field}: {} frames ({}..{})",
                frames.len(),
                frames[0],
                frames[frames.len() - 1]
            );
        }
    }
    Ok(differing == 0 && report.lengths.0 == report.lengths.1)
}
