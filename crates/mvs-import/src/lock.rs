//! One import on the machine at a time, from any tree and either importer: a second waits for the first (two at once
//! spoil each other's time and memory). The lock is `mvs-import.lock` in the temp folder holding its process's id; one
//! whose process has ended is taken over.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::time::Duration;

/// The machine's import lock, held until dropped.
pub struct MachineLock {
    file: PathBuf,
    pid: String,
}

/// The lock file's path.
pub fn machine_lock_file() -> PathBuf {
    std::env::temp_dir().join("mvs-import.lock")
}

impl MachineLock {
    /// Takes the lock, waiting while a live process holds it; `waiting` is told once whose import it waits for.
    pub fn take(waiting: impl Fn(&str)) -> std::io::Result<MachineLock> {
        let file = machine_lock_file();
        let pid = std::process::id().to_string();
        let mut said = false;
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&file) {
                Ok(mut f) => {
                    f.write_all(pid.as_bytes())?;
                    return Ok(MachineLock { file, pid });
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
            let Ok(held) = std::fs::read_to_string(&file) else {
                continue; // (released between the two calls)
            };
            // (an empty file is one being written: its holder is alive)
            if !held.is_empty() && !held.trim().parse().is_ok_and(alive) {
                let _ = std::fs::remove_file(&file);
                continue;
            }
            if !said {
                waiting(if held.is_empty() { "?" } else { held.trim() });
                said = true;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }
}

impl Drop for MachineLock {
    fn drop(&mut self) {
        if std::fs::read_to_string(&self.file).is_ok_and(|h| h == self.pid) {
            let _ = std::fs::remove_file(&self.file);
        }
    }
}

/// Whether process `pid` runs (`process.kill(pid, 0)`: a process this one may not query counts as alive).
pub(crate) fn alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, STILL_ACTIVE};
        use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        // SAFETY: OpenProcess with a pid and an access right; the handle it gives is closed below.
        match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
            Ok(h) => {
                let mut code = 0u32;
                // SAFETY: a live handle from OpenProcess and a u32 to write the exit code to.
                let ok = unsafe { GetExitCodeProcess(h, &mut code) }.is_ok();
                // SAFETY: the handle is ours and closed once.
                let _ = unsafe { CloseHandle(h) };
                !ok || code == STILL_ACTIVE.0 as u32
            }
            Err(e) => e.code() == ERROR_ACCESS_DENIED.to_hresult(),
        }
    }
    #[cfg(not(windows))]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_alive() {
        assert!(alive(std::process::id()));
    }
}
