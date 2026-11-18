//! What the launcher asks of Windows (app/src/setup.ts's main-side calls, paths.ts, shortcuts.ts): the data root and
//! its store, the install location's free space and its problems, the app folder's problem, the folder picker,
//! Explorer on a folder, Steam's console, the clipboard, the desktop and Start menu shortcuts.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use mvs_settings::Settings;
use mvs_ui::launcher::words as w;
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile,
    STGM_READ,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Programs, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog,
    IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath, SIGDN_FILESYSPATH, ShellExecuteW, ShellLink,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{GUID, HSTRING, PCWSTR};

/// The store's size after a full import of the open beta, with room for its largest kind rebuilt (setup.ts
/// STORE_BYTES).
pub const STORE_BYTES: u64 = 8_000_000_000;

/// The store's folder in a data root (paths.ts storeDir).
pub fn store_dir(root: &Path) -> PathBuf {
    root.join("store")
}

/// The data root: the settings' choice, else the data folder itself (paths.ts dataRoot, defaultDataRoot).
// Deviation: `--data` names the data folder, the default data root, but the settings' choice still wins over it
// (paths.ts's `--data` ruled the data root whatever the settings said); a test's `--data` starts with no settings.
pub fn data_root(settings: &Settings, data: &Path) -> PathBuf {
    settings.launcher.data_root.clone().unwrap_or_else(|| data.to_path_buf())
}

/// An install location (bridge.ts Place): its drive's free space, what the store needs there, what is wrong with it.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub free: u64,
    pub need: u64,
    /// One line for the player.
    pub problem: Option<&'static str>,
    /// The problem rules the folder out (it cannot be written).
    pub blocked: bool,
}

/// `dir`'s nearest folder that is there (the install location is made at the import).
fn nearest_dir(dir: &Path) -> PathBuf {
    let mut at = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    while !at.is_dir() {
        match at.parent() {
            Some(p) => at = p.to_path_buf(),
            None => break,
        }
    }
    at
}

/// Bytes of the files in `dir` (one level: the store's layout is flat).
fn files_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map_or(0, |d| d.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum())
}

/// Whether `dir` (or, before it is made, its nearest folder) takes a new folder: Program Files and the like do not
/// without administrator rights.
fn writable(dir: &Path) -> bool {
    let probe = nearest_dir(dir).join(format!(".mvs-write-{}", std::process::id()));
    std::fs::create_dir(&probe).and_then(|()| std::fs::remove_dir(&probe)).is_ok()
}

/// Whether `dir` is `root` or under it (case-blind: Windows' paths).
fn under(dir: &Path, root: Option<PathBuf>) -> bool {
    let Some(root) = root else { return false };
    let norm = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_lowercase();
    let (d, r) = (norm(dir), norm(&root));
    let r = r.trim_end_matches('\\');
    d == r || d.starts_with(&format!("{r}\\"))
}

/// The folders OneDrive syncs (its environment variables).
fn one_drives() -> Vec<PathBuf> {
    ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"]
        .iter()
        .filter_map(std::env::var_os)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Free bytes on the drive of `dir`.
fn free_space(dir: &Path) -> Result<u64, String> {
    let at = HSTRING::from(nearest_dir(dir).as_os_str());
    let mut free = 0u64;
    // SAFETY: the out pointer is a live u64; the path a null-terminated wide string.
    unsafe { GetDiskFreeSpaceExW(&at, Some(&mut free), None, None) }.map_err(|e| e.message())?;
    Ok(free)
}

/// What a rebuild of the stale kinds needs: the size of their packs in the store (a rebuilt kind writes its new pack
/// before the old one goes), all kinds' when the whole store is rebuilt (`all`: a forced run or another build). A
/// stale kind the store has no pack of (a kind a new version adds) counts the mean size of the packs it has.
pub fn rebuild_need(root: &Path, stale: &[String], all: bool) -> u64 {
    let dir = store_dir(root);
    let Ok(Some(m)) = mvs_store::manage::read_manifest(&dir) else { return 0 };
    let sizes: Vec<(&str, u64)> = m
        .kinds
        .iter()
        .map(|(k, r)| (k.as_str(), std::fs::metadata(dir.join(&r.file)).map_or(r.bytes, |f| f.len())))
        .collect();
    need_of(&sizes, stale, all)
}

/// `rebuild_need` over the (kind, pack bytes) the store holds.
fn need_of(sizes: &[(&str, u64)], stale: &[String], all: bool) -> u64 {
    let held: u64 = sizes.iter().filter(|(k, _)| all || stale.iter().any(|s| s == k)).map(|(_, n)| n).sum();
    let total: u64 = sizes.iter().map(|(_, n)| n).sum();
    let mean = total / u64::try_from(sizes.len().max(1)).unwrap_or(1);
    let added = stale.iter().filter(|s| !sizes.iter().any(|(k, _)| k == s)).count();
    held + mean * u64::try_from(added).unwrap_or(0)
}

/// The (free, need) bytes of a rebuild when the drive of `root` cannot hold it; None when it can, or when
/// the drive's space is not known.
pub fn rebuild_short(root: &Path, stale: &[String], all: bool) -> Option<(u64, u64)> {
    let need = rebuild_need(root, stale, all);
    let free = free_space(root).ok()?;
    (free < need).then_some((free, need))
}

/// The install location `dir`: its drive's free space, the store's need there, its problem (setup.ts space).
pub fn space(dir: &Path) -> Result<Place, String> {
    let free = free_space(dir)?;
    let need = STORE_BYTES.saturating_sub(files_size(&store_dir(dir)));
    let mut place = Place { free, need, problem: None, blocked: false };
    if !writable(dir) {
        place.problem = Some(w::NOT_WRITABLE);
        place.blocked = true;
    } else if one_drives().into_iter().any(|d| under(dir, Some(d))) {
        place.problem = Some(w::ONEDRIVE);
    } else if under(dir, Some(std::env::temp_dir())) {
        place.problem = Some(w::TEMP_FOLDER);
    }
    Ok(place)
}

/// The app's folder: the exe's in a release; None in a dev run (an exe in a Cargo workspace's `target`).
pub fn app_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dev = exe.ancestors().any(|d| {
        d.file_name().is_some_and(|n| n == "target") && d.parent().is_some_and(|r| r.join("Cargo.toml").is_file())
    });
    if dev { None } else { exe.parent().map(Path::to_path_buf) }
}

/// What is wrong with the app's own folder, for a line at the top of the launcher (setup.ts appProblem).
pub fn app_problem() -> Option<&'static str> {
    let dir = app_dir()?;
    if under(&dir, Some(std::env::temp_dir())) {
        return Some(w::APP_IN_TEMP);
    }
    if !writable(&dir) {
        return Some(w::APP_NOT_WRITABLE);
    }
    None
}

/// COM on this thread (the picker and the shortcuts ask for it; the second call is a no-op).
fn com() {
    // SAFETY: no reserved pointer; a thread already in another mode keeps it (the call's error is then ignored).
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
}

/// The folder picker over `hwnd` (setup.ts pick: Electron's openDirectory dialog), titled `title`: the folder chosen,
/// None when the player closed it.
// Deviation: modal on the launcher's thread, so the page stops drawing while it is open (Electron's dialog left the
// page running).
pub fn pick_folder(hwnd: HWND, title: &str) -> Option<PathBuf> {
    com();
    // SAFETY: COM calls on the dialog this thread made; the strings outlive the calls; the result's buffer freed
    // with CoTaskMemFree as the API asks.
    unsafe {
        let d: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let o = d.GetOptions().ok()?;
        d.SetOptions(o | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM).ok()?;
        d.SetTitle(&HSTRING::from(title)).ok()?;
        d.Show(Some(hwnd)).ok()?;
        let item = d.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const _));
        path.map(PathBuf::from)
    }
}

/// Opens `target` (a folder in Explorer, a URL in its handler).
fn shell_open(target: &std::ffi::OsStr) -> Result<(), String> {
    let t = HSTRING::from(target);
    // SAFETY: the strings are null-terminated and live through the call.
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &t, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    // (ShellExecute's result is over 32 when it worked)
    if r.0 as usize > 32 { Ok(()) } else { Err(w::windows_answered(r.0 as usize)) }
}

/// Explorer on the data root, or on its nearest folder before it is made (setup.ts setupOpenDataRoot).
pub fn open_folder(root: &Path) -> Result<(), String> {
    shell_open(nearest_dir(root).as_os_str())
}

/// Steam's console (setup.ts setupSteamConsole).
pub fn steam_console() -> Result<(), String> {
    shell_open(std::ffi::OsStr::new("steam://open/console"))
}

/// `text` on the clipboard for the window that has the keyboard (a menu's Copy, clicked in the game's window).
pub fn copy_text_foreground(text: &str) -> Result<(), String> {
    // SAFETY: GetForegroundWindow has no preconditions.
    let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    copy_text(hwnd, text)
}

/// `text` on the clipboard (the guide's Copy: navigator.clipboard.writeText).
pub fn copy_text(hwnd: HWND, text: &str) -> Result<(), String> {
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    // SAFETY: the clipboard opened, filled with a moveable global block of the text's wide characters and closed;
    // the block is the clipboard's once SetClipboardData takes it.
    unsafe {
        OpenClipboard(Some(hwnd)).map_err(|e| e.message())?;
        let done = (|| {
            EmptyClipboard()?;
            let mem = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2)?;
            let p = GlobalLock(mem) as *mut u16;
            if p.is_null() {
                let e = windows::core::Error::from_thread();
                let _ = windows::Win32::Foundation::GlobalFree(Some(mem));
                return Err(e);
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
            let _ = GlobalUnlock(mem);
            // (the block stays ours, to free, when the clipboard does not take it)
            if let Err(e) = SetClipboardData(u32::from(CF_UNICODETEXT.0), Some(HANDLE(mem.0))) {
                let _ = windows::Win32::Foundation::GlobalFree(Some(mem));
                return Err(e);
            }
            windows::core::Result::Ok(())
        })();
        let _ = CloseClipboard();
        done.map_err(|e| e.message())
    }
}

/// The clipboard's text (a menu text box's Ctrl+V, the port's own); None when it holds no text or will not open.
pub fn paste_text(hwnd: HWND) -> Option<String> {
    // SAFETY: the clipboard opened and closed around the read; the text block is the clipboard's, read under its
    // lock up to its terminating zero and within its size, and not freed here.
    unsafe {
        IsClipboardFormatAvailable(u32::from(CF_UNICODETEXT.0)).ok()?;
        OpenClipboard(Some(hwnd)).ok()?;
        let text = (|| {
            let h = GetClipboardData(u32::from(CF_UNICODETEXT.0)).ok()?;
            let mem = windows::Win32::Foundation::HGLOBAL(h.0);
            let p = GlobalLock(mem) as *const u16;
            if p.is_null() {
                return None;
            }
            let cap = windows::Win32::System::Memory::GlobalSize(mem) / 2;
            let s = std::slice::from_raw_parts(p, cap);
            let len = s.iter().position(|&c| c == 0).unwrap_or(cap);
            let t = String::from_utf16_lossy(&s[..len]);
            let _ = GlobalUnlock(mem);
            Some(t)
        })();
        let _ = CloseClipboard();
        text
    }
}

/// A shortcut the settings make (bridge.ts ShortcutKind).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutKind {
    Desktop,
    StartMenu,
}

/// The shortcut's file: `MVS Classic.lnk` on the desktop or in the Start menu's programs (shortcuts.ts link).
fn link(kind: ShortcutKind) -> Option<PathBuf> {
    let id: GUID = match kind {
        ShortcutKind::Desktop => FOLDERID_Desktop,
        ShortcutKind::StartMenu => FOLDERID_Programs,
    };
    // SAFETY: the known folder's path freed with CoTaskMemFree as the API asks.
    unsafe {
        let p = SHGetKnownFolderPath(&id, KF_FLAG_DEFAULT, None).ok()?;
        let dir = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        dir.map(|d| PathBuf::from(d).join("MVS Classic.lnk"))
    }
}

fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain([0]).collect()
}

/// Whether the shortcut is there and points at `exe` (shortcuts.ts state's ours).
fn ours(kind: ShortcutKind, exe: &Path) -> bool {
    let Some(file) = link(kind).filter(|f| f.is_file()) else { return false };
    com();
    // SAFETY: COM calls on the link this thread made; the buffers live through the calls.
    unsafe {
        let Ok(l) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else { return false };
        let Ok(pf) = windows::core::Interface::cast::<IPersistFile>(&l) else { return false };
        if pf.Load(PCWSTR(wide(&file).as_ptr()), STGM_READ).is_err() {
            return false;
        }
        let mut buf = [0u16; 1024];
        if l.GetPath(&mut buf, std::ptr::null_mut(), 0).is_err() {
            return false;
        }
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..n]).eq_ignore_ascii_case(&exe.to_string_lossy())
    }
}

/// The program a shortcut starts: the launcher in the app's folder (None in a dev run).
fn launcher_exe() -> Option<PathBuf> {
    app_dir().map(|d| d.join(mvs_release::LAUNCHER_EXE))
}

/// The shortcuts (bridge.ts ShortcutState): made only in a release (a dev run's exe moves with each build).
pub fn shortcuts() -> mvs_ui::launcher::Shortcuts {
    let exe = launcher_exe();
    mvs_ui::launcher::Shortcuts {
        available: exe.is_some(),
        desktop: exe.as_deref().is_some_and(|e| ours(ShortcutKind::Desktop, e)),
        start_menu: exe.as_deref().is_some_and(|e| ours(ShortcutKind::StartMenu, e)),
    }
}

/// The shortcut made, pointing at the launcher in the app's folder (shortcuts.ts create); one already there is
/// written again (a moved folder's shortcut points nowhere).
pub fn create_shortcut(kind: ShortcutKind) -> Result<(), String> {
    let dir = app_dir().ok_or(w::NO_SHORTCUTS_DEV)?;
    let exe = dir.join(mvs_release::LAUNCHER_EXE);
    let file = link(kind).ok_or(w::NO_SHORTCUT_FOLDER)?;
    com();
    // SAFETY: COM calls on the link this thread made; the strings live through the calls.
    unsafe {
        let l: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.message())?;
        l.SetPath(PCWSTR(wide(&exe).as_ptr())).map_err(|e| e.message())?;
        l.SetWorkingDirectory(PCWSTR(wide(&dir).as_ptr())).map_err(|e| e.message())?;
        l.SetDescription(&HSTRING::from("MVS Classic")).map_err(|e| e.message())?;
        let pf: IPersistFile = windows::core::Interface::cast(&l).map_err(|e| e.message())?;
        pf.Save(PCWSTR(wide(&file).as_ptr()), true).map_err(|e| format!("{}: {}", file.display(), e.message()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mvs-setup-os-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn rebuild_need_counts_a_kind_without_a_pack_as_the_mean() {
        let sizes = [("a", 100), ("b", 300), ("c", 500)];
        let stale = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(need_of(&sizes, &stale(&["b"]), false), 300);
        assert_eq!(need_of(&sizes, &stale(&["b", "new"]), false), 600);
        assert_eq!(need_of(&sizes, &stale(&["new"]), true), 900 + 300);
    }

    #[test]
    fn under_is_case_blind_and_whole_folders() {
        let root = Some(PathBuf::from(r"C:\Users\A\OneDrive"));
        assert!(under(Path::new(r"c:\users\a\onedrive\Games"), root.clone()));
        assert!(under(Path::new(r"C:\Users\A\OneDrive"), root.clone()));
        assert!(!under(Path::new(r"C:\Users\A\OneDriveOld\Games"), root));
        assert!(!under(Path::new(r"C:\Games"), None));
    }

    #[test]
    fn nearest_dir_climbs_to_a_folder_that_is_there() {
        let d = scratch("nearest");
        let at = nearest_dir(&d.join("not").join("yet"));
        assert_eq!(at, std::path::absolute(&d).unwrap());
        std::fs::remove_dir(&d).unwrap();
    }

    #[test]
    fn data_root_takes_the_settings_choice_first() {
        let mut s = Settings::default();
        assert_eq!(data_root(&s, Path::new("D:/data")), PathBuf::from("D:/data"));
        s.launcher.data_root = Some(PathBuf::from("E:/MVS"));
        assert_eq!(data_root(&s, Path::new("D:/data")), PathBuf::from("E:/MVS"));
    }

    #[test]
    fn space_counts_the_store_already_there() {
        let d = scratch("space");
        let store = store_dir(&d);
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(store.join("a.bin"), vec![0u8; 1000]).unwrap();
        let p = space(&d).unwrap();
        assert_eq!(p.need, STORE_BYTES - 1000);
        assert!(p.free > 0);
        assert!(!p.blocked);
        // (the temp folder: the warning, not a block)
        assert_eq!(p.problem, Some(w::TEMP_FOLDER));
        std::fs::remove_file(store.join("a.bin")).unwrap();
        std::fs::remove_dir(&store).unwrap();
        std::fs::remove_dir(&d).unwrap();
    }
}
