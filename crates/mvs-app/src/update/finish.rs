//! The unpacked release's copy over the exe's folder: it waits for the old exe to end, then replaces the old
//! release's files with the new one's, file by file, keeping the old ones aside until all are in, and sets aside the
//! old release's files the new one no longer has. Only the releases' own files are touched: those the new release
//! holds and those the old one lists in its [`RELEASE_FILES`]; a file the player put in the folder stays, and
//! `data\` is never touched.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mvs_release::{DATA_DIR, GAME_DIR, OLD_TOP_LEVEL, RELEASE_FILES};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};

use super::{RM_PAUSE, RM_TRIES, UpdateError, remove};

/// How long the old exe is waited for, ms.
const WAIT_MS: u32 = 60_000;
/// A file's copy is tried this often, a second apart: the old exe's helpers may hold a file a few seconds after it
/// ended.
const COPY_TRIES: u32 = 10;
/// The folder under `data\update\` the old release's files are set aside in while the new one is copied (cleared at
/// the next update: it holds only the releases' own files).
const ASIDE: &str = "previous";

/// Waits for process `pid` to end, at most `WAIT_MS`; one that cannot be opened has ended.
fn wait_for(pid: u32) {
    // SAFETY: a handle opened here and closed here; waiting reads it only.
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else { return };
        WaitForSingleObject(h, WAIT_MS);
        let _ = CloseHandle(h);
    }
}

/// Whether a top-level name is the player's data folder (Windows drops a trailing dot or space: `data.` is `data`).
fn is_data(name: &str) -> bool {
    name.trim_end_matches(['.', ' ']).eq_ignore_ascii_case(DATA_DIR)
}

/// The files under `root`, as `/`-separated paths from it, `data\` left out, sorted.
fn files_under(root: &Path) -> std::io::Result<Vec<String>> {
    fn walk(dir: &Path, at: &str, out: &mut Vec<String>) -> std::io::Result<()> {
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            let name = e.file_name().to_string_lossy().into_owned();
            if at.is_empty() && is_data(&name) {
                continue;
            }
            let rel = if at.is_empty() { name } else { format!("{at}/{name}") };
            let kind = e.file_type()?;
            if kind.is_dir() {
                walk(&e.path(), &rel, out)?;
            } else if kind.is_file() {
                out.push(rel);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, "", &mut out)?;
    out.sort();
    Ok(out)
}

/// The files the release in `root` lists in its [`RELEASE_FILES`]; None when there is no list (a release before it)
/// or a line is not a plain path inside the folder outside `data\` (then the list is not trusted at all).
fn listed(root: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(root.join(GAME_DIR).join(RELEASE_FILES)).ok()?;
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let parts = mvs_release::zip::parts(line).ok()?;
        if is_data(parts[0]) {
            return None;
        }
        out.push(parts.join("/"));
    }
    Some(out)
}

/// A `/`-separated path under `root`.
fn at(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(root.to_path_buf(), |p, s| p.join(s))
}

/// Removes the empty folders from `rel`'s folder up to (not) `root`; stops at the first that is not empty.
fn prune(root: &Path, rel: &str) {
    let mut dir = at(root, rel);
    while dir.pop() && dir != root && dir.starts_with(root) {
        if std::fs::remove_dir(&dir).is_err() {
            break;
        }
    }
}

/// One file of the copy: its path, whether an old file there was set aside, whether the new one was copied in.
struct Done {
    rel: String,
    had_old: bool,
    copied: bool,
}

/// Copies the release unpacked at `from` over `target` once `pid` (the old exe) ended. `from` must lie under
/// `target`'s `data\update\`: only an unpacked release copies itself over a folder. Each of the new release's files
/// renames the old file at its path aside into `data\update\previous\` first (a running or just-ended exe can be
/// renamed where it cannot be removed) and is copied in its place; then the old release's listed files the new one
/// does not have are set aside too. When a copy fails for good every file done so far is put back, so a failed update
/// leaves the old release working. An old release without a list (0.1.4 and before) has its files known by the new
/// release's paths and by [`OLD_TOP_LEVEL`]: its files the new release dropped elsewhere stay.
// Deviation: a failed copy can be undone; removing each old name and copying the new one over it has no way back.
pub fn finish(from: &Path, target: &Path, pid: Option<u32>) -> Result<(), UpdateError> {
    let not_under = || UpdateError::NotUnder { from: from.display().to_string(), target: target.display().to_string() };
    let stage = target.join(DATA_DIR).join(mvs_release::UPDATE_DIR);
    let (Ok(from_c), Ok(stage_c)) = (from.canonicalize(), stage.canonicalize()) else {
        return Err(not_under());
    };
    if from_c == stage_c || !from_c.starts_with(&stage_c) {
        return Err(not_under());
    }
    let aside = stage.join(ASIDE);
    if from_c.starts_with(stage_c.join(ASIDE)) {
        return Err(not_under());
    }
    if let Some(pid) = pid {
        wait_for(pid);
    }
    let io = |p: &Path, e: std::io::Error| UpdateError::Io(format!("{}: {e}", p.display()));
    let new = files_under(from).map_err(|e| io(from, e))?;
    let old = listed(target);
    remove(&aside, RM_TRIES, RM_PAUSE).map_err(|e| io(&aside, e))?;
    std::fs::create_dir_all(&aside).map_err(|e| io(&aside, e))?;

    let mut done: Vec<Done> = Vec::new();
    let put_back = |done: &[Done]| {
        for d in done.iter().rev() {
            let (to, old) = (at(target, &d.rel), at(&aside, &d.rel));
            if d.copied {
                let _ = remove(&to, RM_TRIES, RM_PAUSE);
            }
            if d.had_old {
                let _ = retry(|| std::fs::rename(&old, &to));
            } else {
                prune(target, &d.rel);
            }
        }
    };
    // (the old file at a path is set aside, its folders made in the aside folder first)
    let set_aside = |rel: &str| -> std::io::Result<()> {
        let old = at(&aside, rel);
        if let Some(p) = old.parent() {
            std::fs::create_dir_all(p)?;
        }
        retry(|| std::fs::rename(at(target, rel), &old))
    };
    for rel in &new {
        let to = at(target, rel);
        let had_old = to.symlink_metadata().is_ok();
        if had_old && let Err(e) = set_aside(rel) {
            put_back(&done);
            return Err(io(&to, e));
        }
        done.push(Done { rel: rel.clone(), had_old, copied: false });
        let copy = || {
            if let Some(p) = to.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(at(from, rel), &to).map(drop)
        };
        if let Err(e) = retry(copy) {
            put_back(&done);
            return Err(io(&to, e));
        }
        if let Some(d) = done.last_mut() {
            d.copied = true;
        }
    }
    let kept: BTreeSet<String> = new.iter().map(|r| r.to_lowercase()).collect();
    match old {
        // the old release's files the new one does not have
        Some(old) => {
            for rel in old.iter().filter(|r| !kept.contains(&r.to_lowercase())) {
                if at(target, rel).symlink_metadata().is_ok_and(|m| m.is_file()) && set_aside(rel).is_ok() {
                    prune(target, rel);
                }
            }
        }
        // no list: an older layout's top-level names the new release has under its game folder
        None => {
            for name in OLD_TOP_LEVEL {
                let to = target.join(name);
                let lower = name.to_lowercase();
                let in_new = kept.iter().any(|r| *r == lower || r.starts_with(&format!("{lower}/")));
                if !in_new && to.symlink_metadata().is_ok() {
                    let _ = retry(|| std::fs::rename(&to, aside.join(name)));
                }
            }
        }
    }
    Ok(())
}

/// Runs `f` until it succeeds, `COPY_TRIES` times at most, a second apart.
fn retry(mut f: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut k = 1;
    loop {
        match f() {
            Err(_) if k < COPY_TRIES => {
                k += 1;
                std::thread::sleep(Duration::from_secs(1));
            }
            r => return r,
        }
    }
}
