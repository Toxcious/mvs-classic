//! The store's import lock: one import into a store folder at a time. The
//! lock file, `import.lock`, holds `{"pid","started","nonce"}`; it is made whole (written to a temp file, then
//! hard-linked into place, which fails while a lock is there), and its holder touches it every 10 s. A lock is dead
//! when its process is gone or it was not touched for a minute (a crash, a reused pid); a dead lock is moved aside
//! and checked to be the one judged dead before a new one is made, so two imports cannot both take it over.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};

use mvs_store::manage::{LOCK, write_synced};

use super::ImportError;

/// A lock whose file was not touched for this long is a dead import's, whatever process its pid names now.
const STALE: Duration = Duration::from_secs(60);
const HEARTBEAT: Duration = Duration::from_secs(10);

/// The store's lock, held until dropped.
pub struct StoreLock {
    file: PathBuf,
    nonce: String,
    stop: Arc<(Mutex<bool>, Condvar)>,
    beat: Option<std::thread::JoinHandle<()>>,
}

/// A nonce of 16 hex digits, from the time and the process.
fn nonce() -> String {
    let t = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let mut x = (t.as_nanos() as u64) ^ (u64::from(std::process::id()) << 32);
    // (splitmix64's finalizer: the bits spread)
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    format!("{:016x}", x ^ (x >> 31))
}

fn busy(why: String) -> ImportError {
    ImportError::Busy(why)
}

impl StoreLock {
    /// Takes the lock of the store in `dir`; [`ImportError::Busy`] while another import holds it.
    pub fn take(dir: &Path) -> Result<StoreLock, ImportError> {
        let file = dir.join(LOCK);
        let nonce = nonce();
        let started = iso_utc(SystemTime::now());
        let body = format!("{{\"pid\":{},\"started\":\"{started}\",\"nonce\":\"{nonce}\"}}", std::process::id());
        let tmp = dir.join(format!("{LOCK}.{nonce}.tmp"));
        write_synced(&tmp, body.as_bytes())?;
        let r = Self::link(&file, &tmp, &nonce, dir);
        let _ = std::fs::remove_file(&tmp);
        r?;
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let beat = {
            let (file, stop) = (file.clone(), stop.clone());
            std::thread::spawn(move || {
                let (m, cv) = &*stop;
                let mut done = m.lock().unwrap_or_else(|e| e.into_inner());
                while !*done {
                    done = cv.wait_timeout(done, HEARTBEAT).unwrap_or_else(|e| e.into_inner()).0;
                    if !*done && let Ok(f) = std::fs::File::options().write(true).open(&file) {
                        let _ = f.set_modified(SystemTime::now());
                    }
                }
            })
        };
        Ok(StoreLock { file, nonce, stop, beat: Some(beat) })
    }

    fn link(file: &Path, tmp: &Path, nonce: &str, dir: &Path) -> Result<(), ImportError> {
        let io = |e| ImportError::Io(file.to_path_buf(), e);
        for _ in 0..5 {
            match std::fs::hard_link(tmp, file) {
                Ok(()) => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(io(e)),
            }
            let (text, mtime) =
                match (std::fs::read_to_string(file), std::fs::metadata(file).and_then(|m| m.modified())) {
                    (Ok(t), Ok(m)) => (t, m),
                    (Err(e), _) | (_, Err(e)) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    (Err(e), _) | (_, Err(e)) => return Err(io(e)),
                };
            // empty or torn: judged by its age alone
            let pid = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("pid").and_then(|p| p.as_u64()))
                .and_then(|p| u32::try_from(p).ok())
                .unwrap_or(0);
            let fresh = mtime.elapsed().map_or(true, |a| a < STALE);
            if fresh && (pid == 0 || crate::lock::alive(pid)) {
                let who = if pid == 0 { String::new() } else { format!(" (process {pid})") };
                return Err(busy(format!("an import{who} is running on {}", dir.display())));
            }
            let aside = dir.join(format!("{LOCK}.{nonce}.dead"));
            match std::fs::rename(file, &aside) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(io(e)),
            }
            let moved = std::fs::read_to_string(&aside).unwrap_or_default();
            if moved != text {
                // another import replaced the dead lock between our read and our move: give its lock back
                let _ = std::fs::hard_link(&aside, file);
                let _ = std::fs::remove_file(&aside);
                return Err(busy(format!("another import took {}", file.display())));
            }
            let _ = std::fs::remove_file(&aside);
        }
        Err(busy(format!("could not take {}", file.display())))
    }
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        *self.stop.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.stop.1.notify_all();
        if let Some(b) = self.beat.take() {
            let _ = b.join();
        }
        if std::fs::read_to_string(&self.file).is_ok_and(|t| t.contains(&self.nonce)) {
            let _ = std::fs::remove_file(&self.file);
        }
    }
}

/// A time as ISO 8601 UTC with milliseconds (`Date.toISOString`).
fn iso_utc(t: SystemTime) -> String {
    let d = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let (days, rem) = (secs / 86400, secs % 86400);
    // days since 1970-01-01 to a civil date (Howard Hinnant's days_from_civil, inverted)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        d.subsec_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_time() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_millis(1_762_639_573_123);
        assert_eq!(iso_utc(t), "2025-11-08T22:06:13.123Z");
        assert_eq!(iso_utc(SystemTime::UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn one_holder_then_taken_over_when_dead() {
        let dir = std::env::temp_dir().join(format!("mvs-import-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = StoreLock::take(&dir).unwrap();
        assert!(matches!(StoreLock::take(&dir), Err(ImportError::Busy(_))));
        drop(a);
        assert!(!dir.join(LOCK).exists());
        // a dead process's lock (pid 0 is no process; touched long ago) is taken over
        std::fs::write(dir.join(LOCK), "{\"pid\":4000000000}").unwrap();
        let old = SystemTime::now() - Duration::from_secs(120);
        std::fs::File::options().write(true).open(dir.join(LOCK)).unwrap().set_modified(old).unwrap();
        let b = StoreLock::take(&dir).unwrap();
        drop(b);
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert!(left.is_empty(), "{left:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
