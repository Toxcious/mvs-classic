//! The coverage map (`coverage/map.txt`, written by a coverage-instrumented sim-trace run): each function of the sim's
//! crates with its line range at the map's commit and the trace matches that run it; the kept set
//! (`coverage/kept.txt`), the matches a plain run checks; and `--changed`, the matches a diff can reach (the README,
//! "Coverage map").

use std::path::{Path, PathBuf};
use std::process::Command;

use indexmap::{IndexMap, IndexSet};

/// The crates the sim's run reaches (`mvs-sim` and the workspace crates it depends on): their `src/*.rs` lines are
/// in the map.
pub const SIM_CRATES: &[&str] = &["mvs-sim", "mvs-math", "mvs-data", "mvs-anim", "mvs-collision", "mvs-store"];

/// The sim's proc macros: no line of theirs runs in a match, so a change to one reaches every match.
const SIM_MACROS: &[&str] = &["mvs-sim-derive"];

/// The record and the trace tools: a change to them can move every match.
const TRACE_CRATES: &[&str] = &["mvs-trace", "mvs-sim-trace"];

/// The folder of the map and the kept set.
pub fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("coverage")
}

/// One function of the map: its first and last line and its set of matches.
#[derive(Clone, Copy, Debug)]
struct Function {
    first: u32,
    last: u32,
    set: usize,
}

#[derive(Debug, Default)]
pub struct Map {
    /// The last commit that changed the sim's crates when the map was made: every line range is at it.
    pub commit: String,
    /// The matches the map was made from, in its numbering.
    pub matches: Vec<String>,
    sets: Vec<Vec<usize>>,
    files: IndexMap<String, Vec<Function>>,
}

impl Map {
    pub fn read(path: &Path) -> Result<Map, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Map::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Map, String> {
        let mut m = Map::default();
        let mut file: Option<String> = None;
        let mut raw_sets: Vec<String> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let bad = || format!("line {}: {line}", n + 1);
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let (head, rest) = line.split_once(' ').unwrap_or((line, ""));
            match head {
                "commit" => rest.clone_into(&mut m.commit),
                "match" => m.matches.push(rest.split(' ').next().ok_or_else(bad)?.to_owned()),
                "set" => {
                    let (id, members) = rest.split_once(' ').ok_or_else(bad)?;
                    if id.parse::<usize>().ok() != Some(raw_sets.len()) {
                        return Err(bad());
                    }
                    raw_sets.push(members.to_owned());
                }
                "file" => {
                    m.files.entry(rest.to_owned()).or_default();
                    file = Some(rest.to_owned());
                }
                _ => {
                    let mut it = line.split(' ').map(str::parse::<usize>);
                    let (Some(Ok(first)), Some(Ok(last)), Some(Ok(set)), None) =
                        (it.next(), it.next(), it.next(), it.next())
                    else {
                        return Err(bad());
                    };
                    let f = file.as_ref().ok_or_else(bad)?;
                    let line_of = |v: usize| u32::try_from(v).map_err(|_| bad());
                    m.files[f.as_str()].push(Function { first: line_of(first)?, last: line_of(last)?, set });
                }
            }
        }
        let n = m.matches.len();
        for s in &raw_sets {
            m.sets.push(match s.as_str() {
                "*" => (0..n).collect(),
                "-" => Vec::new(),
                _ => {
                    let (not, list) = s.strip_prefix("! ").map_or((false, s.as_str()), |l| (true, l));
                    let v: Vec<usize> =
                        list.split(' ').map(str::parse).collect::<Result<_, _>>().map_err(|_| s.clone())?;
                    if not { (0..n).filter(|i| !v.contains(i)).collect() } else { v }
                }
            });
        }
        if m.commit.is_empty() || m.files.values().flatten().any(|f| f.set >= m.sets.len()) {
            return Err("no commit, or a function names a set the map lacks".to_owned());
        }
        Ok(m)
    }

    /// The matches whose functions hold one of `lines` (the innermost function holding a line: a closure's own
    /// set, not its parent's), and whether some line is in no function (the matches of every function of the file
    /// are then added: a type, a constant or an attribute between functions reaches what the file's code reaches).
    /// `None` when the map has no function of the file.
    fn place(&self, file: &str, lines: &[u32]) -> Option<(IndexSet<usize>, usize, bool)> {
        let fns = self.files.get(file).filter(|f| !f.is_empty())?;
        let mut picked = IndexSet::new();
        let mut held: IndexSet<usize> = IndexSet::new();
        let mut outside = false;
        for &l in lines {
            let inner = fns
                .iter()
                .enumerate()
                .filter(|(_, f)| f.first <= l && l <= f.last)
                .min_by_key(|(_, f)| f.last - f.first);
            match inner {
                Some((i, f)) => {
                    held.insert(i);
                    picked.extend(self.sets[f.set].iter().copied());
                }
                None => outside = true,
            }
        }
        if outside {
            for f in fns {
                picked.extend(self.sets[f.set].iter().copied());
            }
        }
        Some((picked, held.len(), outside))
    }
}

/// The kept set: the match names of `kept.txt`.
pub fn read_kept(path: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_owned).collect())
}

/// What `--changed` picked.
#[derive(Debug, PartialEq)]
pub enum Pick {
    /// These matches of the map (its numbering), and why.
    Matches(Vec<usize>, String),
    /// The default set, and why (a change the map cannot place, or one that can move every match).
    Default(String),
    /// No match: the change is outside the sim and the trace tools, or in code no match runs.
    Nothing(String),
}

/// What a changed file is to the trace.
#[derive(Debug, PartialEq)]
enum Kind {
    /// A source file of the sim's crates: placed through the map.
    Sim,
    /// A change that can move every match: the record, the trace tools, a sim crate's manifest or proc macro, the
    /// workspace's manifest or lock.
    All(&'static str),
    /// Not run by sim-trace: another crate, a test, a document, the map itself.
    Outside,
}

fn kind(file: &str) -> Kind {
    if matches!(file, "Cargo.toml" | "Cargo.lock" | "rust-toolchain.toml") || file.starts_with(".cargo/") {
        return Kind::All("the workspace's build");
    }
    let Some((krate, rest)) = file.strip_prefix("crates/").and_then(|r| r.split_once('/')) else {
        return Kind::Outside;
    };
    let doc_or_test = rest.starts_with("tests/")
        || rest.starts_with("benches/")
        || rest.starts_with("examples/")
        || rest.starts_with("coverage/")
        || rest.ends_with(".md");
    if doc_or_test || rest == "clippy.toml" {
        return Kind::Outside;
    }
    if TRACE_CRATES.contains(&krate) {
        return Kind::All("the record or the trace tools");
    }
    if SIM_MACROS.contains(&krate) {
        return Kind::All("the sim's proc macros");
    }
    if SIM_CRATES.contains(&krate) {
        return if rest.starts_with("src/") && rest.ends_with(".rs") {
            Kind::Sim
        } else {
            Kind::All("a sim crate's build or data")
        };
    }
    Kind::Outside
}

fn git(root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git").args(args).current_dir(root).output().map_err(|e| format!("git {}: {e}", args.join(" ")))
}

fn git_text(root: &Path, args: &[&str]) -> Result<String, String> {
    let o = git(root, args)?;
    if !o.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// This tree's root.
fn root() -> Result<PathBuf, String> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    Ok(PathBuf::from(git_text(here, &["rev-parse", "--show-toplevel"])?.trim()))
}

/// The revision `--changed` compares with when none is named: HEAD's merge base with `main` (on `main` itself,
/// HEAD: the working tree's changes), else the map's commit.
pub fn default_rev(map: &Map) -> String {
    root()
        .and_then(|r| git_text(&r, &["merge-base", "HEAD", "main"]))
        .map_or_else(|_| map.commit.clone(), |s| s.trim().to_owned())
}

/// The old-side lines a diff of one file changes, comment-only and blank-only hunks left out. A hunk that only adds
/// lines changes the two lines around the place it adds them.
fn changed_lines(diff: &str) -> Vec<u32> {
    let mut lines = Vec::new();
    let mut hunk: Option<(u32, u32)> = None;
    let mut code = false;
    let flush = |hunk: Option<(u32, u32)>, code: bool, lines: &mut Vec<u32>| {
        if let (Some((a, b)), true) = (hunk, code) {
            if b == 0 {
                lines.extend([a.max(1), a + 1]);
            } else {
                lines.extend(a..a + b);
            }
        }
    };
    for l in diff.lines() {
        if let Some(h) = l.strip_prefix("@@ -") {
            flush(hunk, code, &mut lines);
            let old = h.split(' ').next().unwrap_or("");
            let (a, b) = old.split_once(',').unwrap_or((old, "1"));
            hunk = Some((a.parse().unwrap_or(1), b.parse().unwrap_or(1)));
            code = false;
        } else if hunk.is_some() && (l.starts_with('-') || l.starts_with('+')) {
            let t = l[1..].trim();
            code |= !(t.is_empty() || t.starts_with("//"));
        }
    }
    flush(hunk, code, &mut lines);
    lines
}

/// The matches `git diff <rev>` (the working tree included, untracked files too) can reach.
pub fn changed(rev: &str, map: &Map) -> Result<Pick, String> {
    let root = root()?;
    let mut files: Vec<String> =
        git_text(&root, &["diff", "--name-only", "--no-renames", rev])?.lines().map(str::to_owned).collect();
    files.extend(git_text(&root, &["ls-files", "--others", "--exclude-standard"])?.lines().map(str::to_owned));
    files.sort();
    files.dedup();
    if files.is_empty() {
        return Ok(Pick::Nothing(format!("nothing changed since {rev}")));
    }
    let untracked: Vec<String> =
        git_text(&root, &["ls-files", "--others", "--exclude-standard"])?.lines().map(str::to_owned).collect();
    let map_older = git(&root, &["merge-base", "--is-ancestor", &map.commit, rev])?.status.success();
    let mut picked: IndexSet<usize> = IndexSet::new();
    let mut why: Vec<String> = Vec::new();
    let mut sim_files = 0;
    for f in &files {
        match kind(f) {
            Kind::Outside => {}
            Kind::All(what) => return Ok(Pick::Default(format!("{f} changed ({what})"))),
            Kind::Sim => {
                sim_files += 1;
                if untracked.contains(f) || !map.files.get(f.as_str()).is_some_and(|v| !v.is_empty()) {
                    return Ok(Pick::Default(format!("{f} is not in the coverage map")));
                }
                // The map's lines are at its commit: a file unchanged between the map and `rev` takes `rev`'s
                // diff; else the map's own diff (a wider one) when the map is older than `rev`.
                let same = git(&root, &["diff", "--quiet", &map.commit, rev, "--", f])?.status.success();
                let base = if same {
                    rev
                } else if map_older {
                    map.commit.as_str()
                } else {
                    return Ok(Pick::Default(format!(
                        "{f} changed between {rev} and the map's commit {}, which is not older: its lines cannot be \
                         placed",
                        short(&map.commit)
                    )));
                };
                let diff = git_text(&root, &["diff", "-U0", "--no-color", "--no-ext-diff", base, "--", f])?;
                let lines = changed_lines(&diff);
                if lines.is_empty() {
                    continue;
                }
                let (m, fns, outside) = map.place(f, &lines).expect("the file is in the map");
                let name = f.strip_prefix("crates/").unwrap_or(f);
                why.push(match (fns, outside) {
                    (_, true) => format!("{name} ({fns} functions and lines outside one: the file's matches)"),
                    _ => format!("{name} ({fns} functions)"),
                });
                picked.extend(m);
            }
        }
    }
    if sim_files == 0 {
        return Ok(Pick::Nothing(format!("no file of the sim or the trace tools changed since {}", short(rev))));
    }
    if why.is_empty() {
        return Ok(Pick::Nothing(format!("only comments or blank lines of the sim changed since {}", short(rev))));
    }
    if picked.is_empty() {
        return Ok(Pick::Nothing(format!(
            "the changes since {} are in code no match runs: {}",
            short(rev),
            why.join(", ")
        )));
    }
    let mut v: Vec<usize> = picked.into_iter().collect();
    v.sort_unstable();
    Ok(Pick::Matches(v, format!("since {}: {}", short(rev), why.join(", "))))
}

fn short(rev: &str) -> &str {
    if rev.len() == 40 && rev.bytes().all(|b| b.is_ascii_hexdigit()) { &rev[..9] } else { rev }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = "# a map\ncommit abc\nmatch a 10\nmatch b 20\nmatch c 30\nset 0 *\nset 1 -\nset 2 0\nset 3 ! 0\n\
                       file crates/mvs-sim/src/x.rs\n10 40 2\n20 25 3\n50 60 1\n";

    #[test]
    fn a_line_takes_its_innermost_function_and_one_outside_the_files_union() {
        let m = Map::parse(MAP).unwrap();
        assert_eq!(m.matches, ["a", "b", "c"]);
        let at = |lines: &[u32]| {
            let (s, n, out) = m.place("crates/mvs-sim/src/x.rs", lines).unwrap();
            let mut s: Vec<usize> = s.into_iter().collect();
            s.sort_unstable();
            (s, n, out)
        };
        assert_eq!(at(&[12]), (vec![0], 1, false));
        assert_eq!(at(&[22]), (vec![1, 2], 1, false));
        assert_eq!(at(&[55]), (vec![], 1, false));
        assert_eq!(at(&[45]), (vec![0, 1, 2], 0, true));
        assert!(m.place("crates/mvs-sim/src/y.rs", &[1]).is_none());
    }

    #[test]
    fn comment_hunks_change_nothing_and_an_insert_touches_its_neighbours() {
        let d =
            "diff\n@@ -5,2 +5,2 @@\n-    // old\n+    // new\n@@ -9 +9,0 @@\n-x = 1;\n@@ -20,0 +21,3 @@\n+fn f() {}\n";
        assert_eq!(changed_lines(d), vec![9, 20, 21]);
    }

    #[test]
    fn files_are_sorted_by_what_they_reach() {
        assert_eq!(kind("crates/mvs-sim/src/fighter/body.rs"), Kind::Sim);
        assert_eq!(kind("crates/mvs-sim/tests/x.rs"), Kind::Outside);
        assert!(matches!(kind("crates/mvs-sim/Cargo.toml"), Kind::All(_)));
        assert!(matches!(kind("crates/mvs-trace/src/record.rs"), Kind::All(_)));
        assert!(matches!(kind("crates/mvs-sim-trace/src/bin/sim-trace.rs"), Kind::All(_)));
        assert_eq!(kind("crates/mvs-sim-trace/coverage/map.txt"), Kind::Outside);
        assert_eq!(kind("crates/mvs-render/src/lib.rs"), Kind::Outside);
        assert_eq!(kind("docs/workflow/testing.md"), Kind::Outside);
        assert!(matches!(kind("Cargo.lock"), Kind::All(_)));
    }
}
