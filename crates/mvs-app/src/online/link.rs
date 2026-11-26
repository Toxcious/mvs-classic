//! (the port's own) The game's side of a join link (mvs_release::URL_SCHEME): one running game per data folder
//! holds the instance (a named mutex and a pid file the launcher raises its window by), and a link the launcher
//! hands over in the data folder's join-link file is joined from the menus.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use mvs_net::service::api::SessionTarget;
use mvs_ui::menu::online::{OnlineCmd, OnlineNotice};
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
#[cfg(windows)]
use windows::Win32::System::Threading::{CreateMutexW, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
#[cfg(windows)]
use windows::core::HSTRING;

use super::hub::OnlineHub;

/// The data folder's file the launcher writes a running game's join link to (written to a temporary name, then
/// renamed).
pub const JOIN_LINK_FILE: &str = "join-link.json";
/// The data folder's file the game holding the instance writes its process id to (the launcher raises its window).
pub const GAME_PID_FILE: &str = "game.pid";
/// The longest lobby code a link holds (the join box's CODE_LIMIT).
pub const CODE_LIMIT: usize = 64;
/// How often the menus look for a handed-over link, in ms.
const POLL_MS: f64 = 250.0;

/// The join-link file's text: `{"code": "<code>"}`.
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct JoinLink {
    pub code: String,
}

/// This game holds the data folder's instance for its life (dropped: let go).
#[cfg(windows)]
#[derive(Debug)]
pub struct Instance {
    mutex: HANDLE,
    pid_file: PathBuf,
}

#[cfg(windows)]
impl Drop for Instance {
    fn drop(&mut self) {
        HOLDING.store(false, Ordering::Relaxed);
        // (the file is this game's while it names this process)
        if std::fs::read_to_string(&self.pid_file).is_ok_and(|t| t.trim() == std::process::id().to_string()) {
            let _ = std::fs::remove_file(&self.pid_file);
        }
        // SAFETY: the mutex this instance made and nothing else closes.
        unsafe {
            let _ = CloseHandle(self.mutex);
        }
    }
}

/// This process holds the instance: the menus look for links.
static HOLDING: AtomicBool = AtomicBool::new(false);
/// The hub's clock at the last look (f64 bits).
static LOOKED: AtomicU64 = AtomicU64::new(0);

/// The instance's mutex name for a data folder: `Local\MvsClassic-<hash of its full path, lowercase>` (one per
/// Windows session and data folder).
pub fn instance_name(data: &Path) -> String {
    let full = std::path::absolute(data).unwrap_or_else(|_| data.to_path_buf());
    let text = full.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    // (FNV-1a, 64 bits)
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for b in text.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("Local\\MvsClassic-{h:016x}")
}

/// Takes the data folder's instance; None when another game holds it, or it cannot be made.
#[cfg(windows)]
pub fn hold_instance(data: &Path) -> Option<Instance> {
    let name = HSTRING::from(instance_name(data));
    // SAFETY: a named mutex made with default security; its handle is owned by the Instance (or closed here).
    let mutex = unsafe { CreateMutexW(None, false, &name) };
    let mutex = match mutex {
        Ok(m) => m,
        Err(e) => {
            eprintln!("mvs: the game's instance: {e}");
            return None;
        }
    };
    // SAFETY: the calling thread's last error, read straight after CreateMutexW.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        eprintln!("mvs: another game holds this data folder: join links go to it");
        // SAFETY: the handle just made and not handed on.
        unsafe {
            let _ = CloseHandle(mutex);
        }
        return None;
    }
    let pid_file = data.join(GAME_PID_FILE);
    let _ = std::fs::create_dir_all(data);
    // (a link left for a game that ended before it looked is not this game's to join)
    let _ = std::fs::remove_file(data.join(JOIN_LINK_FILE));
    if let Err(e) = std::fs::write(&pid_file, std::process::id().to_string()) {
        eprintln!("mvs: {}: {e}", pid_file.display());
    }
    HOLDING.store(true, Ordering::Relaxed);
    Some(Instance { mutex, pid_file })
}

/// The game holding the data folder's instance: Some(its process id, when its pid file reads), None when no game
/// holds it.
#[cfg(windows)]
pub fn running_game(data: &Path) -> Option<Option<u32>> {
    let name = HSTRING::from(instance_name(data));
    // SAFETY: opened to test that the mutex is there, closed at once.
    let m = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &name) }.ok()?;
    // SAFETY: the handle just opened.
    unsafe {
        let _ = CloseHandle(m);
    }
    Some(std::fs::read_to_string(data.join(GAME_PID_FILE)).ok().and_then(|t| t.trim().parse().ok()))
}

/// The data folder's file a game holding the instance keeps locked (flock) for its life.
#[cfg(not(windows))]
const LOCK_FILE: &str = "game.lock";

/// This game holds the data folder's instance for its life (dropped: let go).
// Deviation: an exclusive flock on the data folder's `game.lock` stands for the named mutex (one game per data
// folder; the lock ends with the game's process, a crash included).
#[cfg(not(windows))]
#[derive(Debug)]
pub struct Instance {
    /// The locked file: closed after the drop below, which lets the lock go.
    _lock: std::fs::File,
    pid_file: PathBuf,
}

#[cfg(not(windows))]
impl Drop for Instance {
    fn drop(&mut self) {
        HOLDING.store(false, Ordering::Relaxed);
        // (the file is this game's while it names this process)
        if std::fs::read_to_string(&self.pid_file).is_ok_and(|t| t.trim() == std::process::id().to_string()) {
            let _ = std::fs::remove_file(&self.pid_file);
        }
    }
}

/// Takes the data folder's instance; None when another game holds it, or it cannot be made.
#[cfg(not(windows))]
pub fn hold_instance(data: &Path) -> Option<Instance> {
    let _ = std::fs::create_dir_all(data);
    let file = data.join(LOCK_FILE);
    let lock = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&file);
    let lock = match lock.map_err(std::fs::TryLockError::Error).and_then(|f| f.try_lock().map(|()| f)) {
        Ok(f) => f,
        Err(std::fs::TryLockError::WouldBlock) => {
            eprintln!("mvs: another game holds this data folder: join links go to it");
            return None;
        }
        Err(std::fs::TryLockError::Error(e)) => {
            eprintln!("mvs: the game's instance: {}: {e}", file.display());
            return None;
        }
    };
    let pid_file = data.join(GAME_PID_FILE);
    // (a link left for a game that ended before it looked is not this game's to join)
    let _ = std::fs::remove_file(data.join(JOIN_LINK_FILE));
    if let Err(e) = std::fs::write(&pid_file, std::process::id().to_string()) {
        eprintln!("mvs: {}: {e}", pid_file.display());
    }
    HOLDING.store(true, Ordering::Relaxed);
    Some(Instance { _lock: lock, pid_file })
}

/// The game holding the data folder's instance: Some(its process id, when its pid file reads), None when no game
/// holds it.
// Deviation: the lock is tested by taking it on a file opened anew (flock locks per open file, so this process's
// own instance refuses it too) and letting it go at once: a game starting in that moment finds it held and does not
// take the instance, where opening the named mutex takes nothing.
#[cfg(not(windows))]
pub fn running_game(data: &Path) -> Option<Option<u32>> {
    // (no lock file: no game ever held the folder)
    let file = std::fs::File::open(data.join(LOCK_FILE)).ok()?;
    match file.try_lock() {
        Err(std::fs::TryLockError::WouldBlock) => {
            Some(std::fs::read_to_string(data.join(GAME_PID_FILE)).ok().and_then(|t| t.trim().parse().ok()))
        }
        // (taken: no game holds it; closing the file lets it go)
        Ok(()) | Err(std::fs::TryLockError::Error(_)) => None,
    }
}

/// The link's code as the join box would take it: ASCII letters and digits only, at most CODE_LIMIT; else why not.
pub fn plain_code(code: &str) -> Result<&str, String> {
    if code.is_empty() {
        return Err("no code".to_owned());
    }
    if code.len() > CODE_LIMIT {
        return Err(format!("a code of more than {CODE_LIMIT} characters"));
    }
    if !code.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err("a character a code does not have".to_owned());
    }
    Ok(code)
}

/// Writes the join-link file for the game holding `data` (to a temporary name of this process's own, then renamed
/// over it: of two links before the game looks, the later wins).
pub fn hand_over(data: &Path, code: &str) -> std::io::Result<()> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let file = data.join(JOIN_LINK_FILE);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let tmp = data.join(format!("{JOIN_LINK_FILE}.{}.{n}.tmp", std::process::id()));
    let text = serde_json::to_string(&JoinLink { code: code.to_owned() }).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, text)?;
    let r = mvs_store::manage::rename_retried(&tmp, &file);
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

/// The join-link file taken (read and removed): its code when it holds a plain one.
pub fn take_link(data: &Path) -> Option<String> {
    let file = data.join(JOIN_LINK_FILE);
    let text = std::fs::read_to_string(&file).ok()?;
    let _ = std::fs::remove_file(&file);
    match serde_json::from_str::<JoinLink>(&text).map_err(|e| e.to_string()).and_then(|l| {
        plain_code(&l.code)?;
        Ok(l.code)
    }) {
        Ok(code) => Some(code),
        Err(e) => {
            eprintln!("mvs: {}: {e}", file.display());
            None
        }
    }
}

/// A join link the launcher handed over since the last call, kept as the hub's link join (held while a gate is set;
/// the menus then open the lobby screen on it). Each frame of the menus.
pub fn poll(hub: &mut OnlineHub, data: &Path) {
    if !HOLDING.load(Ordering::Relaxed) {
        return;
    }
    let now = hub.clock();
    let last = f64::from_bits(LOOKED.load(Ordering::Relaxed));
    if now >= last && now - last < POLL_MS {
        return;
    }
    LOOKED.store(now.to_bits(), Ordering::Relaxed);
    let Some(code) = take_link(data) else { return };
    eprintln!("mvs: a join link: {code}");
    hub.push_open(OnlineCmd::Open(SessionTarget::Join(code)));
    if hub.gate().is_some() {
        hub.notice(OnlineNotice::Host(mvs_ui::menu::custom_text::LINK_HELD.to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_name_per_data_folder_whatever_its_spelling() {
        let a = instance_name(Path::new("C:/Users/x/AppData/Roaming/MVS Classic"));
        assert_eq!(a, instance_name(Path::new("c:\\users\\X\\AppData\\Roaming\\MVS Classic\\")));
        assert_ne!(a, instance_name(Path::new("C:/Users/x/AppData/Roaming/MVS Classic 2")));
        assert!(a.starts_with("Local\\MvsClassic-") && a.len() == "Local\\MvsClassic-".len() + 16);
    }

    #[test]
    fn a_plain_code_only() {
        assert_eq!(plain_code("abc234"), Ok("abc234"));
        for bad in ["", "abc 234", "abc&calc", "a\"b", "ab/c", "\u{e9}abcde", &"a".repeat(CODE_LIMIT + 1)] {
            assert!(plain_code(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_instance_is_one_and_hands_links_over() {
        let dir = std::env::temp_dir().join(format!("mvs-link-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(running_game(&dir), None);
        hand_over(&dir, "left0over").unwrap();
        let i = hold_instance(&dir).expect("the first holds it");
        assert!(hold_instance(&dir).is_none(), "a second game does not");
        assert_eq!(running_game(&dir), Some(Some(std::process::id())));
        assert_eq!(take_link(&dir), None, "a link left from before is not this game's");
        hand_over(&dir, "first1").unwrap();
        hand_over(&dir, "abc234").unwrap();
        let tmp = std::fs::read_dir(&dir).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".tmp"));
        assert!(!tmp, "no temporary file left");
        assert_eq!(take_link(&dir).as_deref(), Some("abc234"), "the later link wins");
        assert_eq!(take_link(&dir), None, "taken once");
        std::fs::write(dir.join(JOIN_LINK_FILE), r#"{"code":"a b"}"#).unwrap();
        assert_eq!(take_link(&dir), None, "not a plain code");
        drop(i);
        assert_eq!(running_game(&dir), None);
        assert!(!dir.join(GAME_PID_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(windows))]
    #[test]
    fn a_second_lock_on_one_data_folder_is_refused() {
        let dir = std::env::temp_dir().join(format!("mvs-lock-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let first = hold_instance(&dir).expect("the first lock");
        assert!(hold_instance(&dir).is_none(), "a second lock is refused");
        drop(first);
        let third = hold_instance(&dir).expect("released on drop: a third lock holds");
        drop(third);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
