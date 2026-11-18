//! The unpacked release's copy over the exe's folder: it waits for the old exe to end, then
//! replaces each of the folder's top-level names with its own, whole, keeping the old ones aside until
//! all are in, then sets aside an older layout's names it no longer has; `data\` is never touched.

use std::path::Path;
use std::time::Duration;

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};

use super::{RM_PAUSE, RM_TRIES, UpdateError, remove};

/// How long the old exe is waited for, ms.
const WAIT_MS: u32 = 60_000;
/// A name's copy is tried this often, a second apart: the old exe's helpers may hold a file a few seconds after it
/// ended.
const COPY_TRIES: u32 = 10;
/// The folder under `data\update\` the old release's names are set aside in while the new one is copied.
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

/// Copies a file or a folder whole.
fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            copy(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(drop)
    }
}

/// Copies the release unpacked at `from` over `target` once `pid` (the old exe) ended. `from` must lie under
/// `target`'s `data\update\`: only an unpacked release copies itself over a folder. Each old top-level name is
/// renamed aside into `data\update\previous\` first (a running or just-ended exe can be renamed where it cannot be
/// removed) and the new one copied in its place; when a copy fails for good every name done so far is put back, so a
/// failed update leaves the old release working.
// Deviation: a failed copy can be undone; removing each old name and copying the new one over it has no way back.
pub fn finish(from: &Path, target: &Path, pid: Option<u32>) -> Result<(), UpdateError> {
    let not_under = || UpdateError::NotUnder { from: from.display().to_string(), target: target.display().to_string() };
    let stage = target.join(mvs_release::DATA_DIR).join(mvs_release::UPDATE_DIR);
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
    remove(&aside, RM_TRIES, RM_PAUSE).map_err(|e| io(&aside, e))?;
    std::fs::create_dir_all(&aside).map_err(|e| io(&aside, e))?;
    let mut names = Vec::new();
    for e in std::fs::read_dir(from).map_err(|e| io(from, e))? {
        let name = e.map_err(|e| io(from, e))?.file_name();
        // (Windows drops a trailing dot or space: `data.` is `data`)
        if !name.to_string_lossy().trim_end_matches(['.', ' ']).eq_ignore_ascii_case(mvs_release::DATA_DIR) {
            names.push(name);
        }
    }
    // the names replaced so far, and whether each had an old one set aside
    let mut done: Vec<(&std::ffi::OsStr, bool)> = Vec::new();
    let put_back = |done: &[(&std::ffi::OsStr, bool)]| {
        for &(name, had_old) in done.iter().rev() {
            let (to, old) = (target.join(name), aside.join(name));
            let _ = remove(&to, RM_TRIES, RM_PAUSE);
            if had_old {
                let _ = retry(|| std::fs::rename(&old, &to));
            }
        }
    };
    for name in &names {
        let (src, to, old) = (from.join(name), target.join(name), aside.join(name));
        let had_old = to.symlink_metadata().is_ok();
        if had_old && let Err(e) = retry(|| std::fs::rename(&to, &old)) {
            put_back(&done);
            return Err(io(&to, e));
        }
        done.push((name, had_old));
        if let Err(e) = retry(|| remove(&to, RM_TRIES, RM_PAUSE).and_then(|()| copy(&src, &to))) {
            put_back(&done);
            return Err(io(&to, e));
        }
    }
    // an older layout's top-level names the new release has under its game folder: set aside with the rest
    for old in mvs_release::OLD_TOP_LEVEL {
        let to = target.join(old);
        if !names.iter().any(|n| n.eq_ignore_ascii_case(old)) && to.symlink_metadata().is_ok() {
            let _ = retry(|| std::fs::rename(&to, aside.join(old)));
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
