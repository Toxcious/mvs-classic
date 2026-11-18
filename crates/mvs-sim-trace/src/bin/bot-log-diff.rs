//! `bot-log-diff <a> <b> [--lines <n>]`: two bot logs compared (`sim-trace --bot-log`, the oracle's
//! bot log): two files, or two folders whose `.botlog` files of the same name are compared. One
//! line a log: `same`, or the first line that differs, with its frame and the think it is in (who thinks), then the
//! think's lines on both sides up to that line (the last `--lines` of them, 12 by default) and the next one of each.
//! A log missing on one side is named. Exit 1 when a pair differs.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: bot-log-diff <a file or folder> <b file or folder> [--lines <n>]";

fn main() -> ExitCode {
    match main_() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("bot-log-diff: {e}");
            ExitCode::from(2)
        }
    }
}

fn main_() -> Result<bool, String> {
    let mut paths = Vec::new();
    let mut lines = 12;
    let mut it = std::env::args().skip(1);
    while let Some(s) = it.next() {
        match s.as_str() {
            "--lines" => lines = it.next().and_then(|v| v.parse().ok()).ok_or("--lines needs a count")?,
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(true);
            }
            _ if s.starts_with("--") => return Err(format!("unknown option {s}\n{USAGE}")),
            _ => paths.push(PathBuf::from(s)),
        }
    }
    let [a, b] = <[PathBuf; 2]>::try_from(paths).map_err(|_| USAGE.to_owned())?;
    let pairs = if a.is_dir() {
        let names = |d: &Path| -> Result<Vec<String>, String> {
            let mut v: Vec<String> = std::fs::read_dir(d)
                .map_err(|e| format!("{}: {e}", d.display()))?
                .filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter(|n| n.ends_with(".botlog"))
                .collect();
            v.sort();
            Ok(v)
        };
        let (na, nb) = (names(&a)?, names(&b)?);
        let mut all = true;
        for n in na.iter().filter(|n| !nb.contains(n)) {
            println!("{n}: only in {}", a.display());
            all = false;
        }
        for n in nb.iter().filter(|n| !na.contains(n)) {
            println!("{n}: only in {}", b.display());
            all = false;
        }
        let pairs: Vec<_> = na.iter().filter(|n| nb.contains(n)).map(|n| (n.clone(), a.join(n), b.join(n))).collect();
        if pairs.is_empty() {
            return Err(format!("no .botlog file in both {} and {}", a.display(), b.display()));
        }
        (pairs, all)
    } else {
        (vec![(a.display().to_string(), a.clone(), b.clone())], true)
    };
    let (pairs, mut all) = pairs;
    for (name, pa, pb) in pairs {
        let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
        let (ta, tb) = (read(&pa)?, read(&pb)?);
        let (la, lb): (Vec<&str>, Vec<&str>) = (ta.lines().collect(), tb.lines().collect());
        let Some(k) = (0..la.len().max(lb.len())).find(|&k| la.get(k) != lb.get(k)) else {
            println!("{name}: same ({} lines)", la.len());
            continue;
        };
        all = false;
        let at = la.get(k).or(lb.get(k)).copied().unwrap_or_default();
        let frame = at.split(' ').next().unwrap_or("?");
        // (the think the line is in: the last `think` line before it, the same on both sides)
        let start = (0..k).rev().find(|&j| la[j].split(' ').nth(1) == Some("think")).unwrap_or(0);
        let who = la.get(start).and_then(|l| l.split(' ').nth(2)).unwrap_or("?");
        println!("{name}: differs at line {} (frame {frame}, {who}'s think from line {})", k + 1, start + 1);
        let from = start.max(k.saturating_sub(lines));
        for l in &la[from..k] {
            println!("      {l}");
        }
        for (side, l) in [("a", &la), ("b", &lb)] {
            for x in l.iter().skip(k).take(2) {
                println!("    {side} {x}");
            }
            if l.len() <= k {
                println!("    {side} (the log ends)");
            }
        }
    }
    Ok(all)
}
