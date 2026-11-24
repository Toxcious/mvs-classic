//! What the launcher asks of Windows (app/src/setup.ts's main-side calls, paths.ts, shortcuts.ts): the data root and
//! its store, the install location's free space and its problems, the app folder's problem, the folder picker,
//! Explorer on a folder, Steam's console, the clipboard, the desktop and Start menu shortcuts.

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

#[cfg(not(windows))]
use mvs_d3d::Foundation::HWND;
use mvs_settings::Settings;
use mvs_ui::launcher::words as w;
#[cfg(windows)]
use windows::Win32::Foundation::{HANDLE, HWND};
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
#[cfg(windows)]
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile,
    STGM_READ,
};
#[cfg(windows)]
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
};
#[cfg(windows)]
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
#[cfg(windows)]
use windows::Win32::System::Ole::CF_UNICODETEXT;
#[cfg(windows)]
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Programs, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog,
    IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath, SIGDN_FILESYSPATH, ShellExecuteW, ShellLink,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
#[cfg(windows)]
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
#[cfg(windows)]
fn under(dir: &Path, root: Option<PathBuf>) -> bool {
    let Some(root) = root else { return false };
    let norm = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_lowercase();
    let (d, r) = (norm(dir), norm(&root));
    let r = r.trim_end_matches('\\');
    d == r || d.starts_with(&format!("{r}\\"))
}

/// Whether `dir` is `root` or under it (case-blind, as on Windows; `/` or `\\` between the folders: the warnings
/// it gives are a guess either way).
#[cfg(not(windows))]
fn under(dir: &Path, root: Option<PathBuf>) -> bool {
    let Some(root) = root else { return false };
    let norm = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_lowercase();
    let (d, r) = (norm(dir), norm(&root));
    let r = r.trim_end_matches(['/', '\\']);
    d == r || d.strip_prefix(r).is_some_and(|rest| rest.starts_with(['/', '\\']))
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
#[cfg(windows)]
fn free_space(dir: &Path) -> Result<u64, String> {
    let at = HSTRING::from(nearest_dir(dir).as_os_str());
    let mut free = 0u64;
    // SAFETY: the out pointer is a live u64; the path a null-terminated wide string.
    unsafe { GetDiskFreeSpaceExW(&at, Some(&mut free), None, None) }.map_err(|e| e.message())?;
    Ok(free)
}

/// Free bytes on the file system of `dir` for an unprivileged user (statvfs: f_bavail blocks of f_frsize).
#[cfg(not(windows))]
fn free_space(dir: &Path) -> Result<u64, String> {
    use std::os::unix::ffi::OsStrExt;
    let at = nearest_dir(dir);
    let c = std::ffi::CString::new(at.as_os_str().as_bytes()).map_err(|e| format!("{}: {e}", at.display()))?;
    let mut st = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: a null-terminated path and a struct the call fills, read only after it says it did.
    let r = unsafe { libc::statvfs(c.as_ptr(), st.as_mut_ptr()) };
    if r != 0 {
        return Err(format!("{}: {}", at.display(), std::io::Error::last_os_error()));
    }
    // SAFETY: statvfs returned 0, so it filled the struct.
    let st = unsafe { st.assume_init() };
    Ok(st.f_bavail.saturating_mul(st.f_frsize))
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
#[cfg(windows)]
fn com() {
    // SAFETY: no reserved pointer; a thread already in another mode keeps it (the call's error is then ignored).
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
}

/// The folder picker over `hwnd` (setup.ts pick: Electron's openDirectory dialog), titled `title`: the folder chosen,
/// None when the player closed it.
// Deviation: modal on the launcher's thread, so the page stops drawing while it is open (Electron's dialog left the
// page running).
#[cfg(windows)]
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

/// The folder picker over `hwnd`, titled `title`: none yet on Linux.
// Unverified: stub, replaced in Task 7
#[cfg(not(windows))]
pub fn pick_folder(_hwnd: HWND, _title: &str) -> Option<PathBuf> {
    None
}

/// Opens `target` (a folder in Explorer, a URL in its handler).
#[cfg(windows)]
fn shell_open(target: &std::ffi::OsStr) -> Result<(), String> {
    let t = HSTRING::from(target);
    // SAFETY: the strings are null-terminated and live through the call.
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &t, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    // (ShellExecute's result is over 32 when it worked)
    if r.0 as usize > 32 { Ok(()) } else { Err(w::windows_answered(r.0 as usize)) }
}

/// Opens `target` (a folder in the file manager, a URL in its handler) through `xdg-open`, which is not waited for:
/// a thread of its own reaps it.
#[cfg(not(windows))]
fn shell_open(target: &std::ffi::OsStr) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let mut child = Command::new("xdg-open")
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("xdg-open: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
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
#[cfg(windows)]
pub fn copy_text_foreground(text: &str) -> Result<(), String> {
    // SAFETY: GetForegroundWindow has no preconditions.
    let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    copy_text(hwnd, text)
}

/// `text` on the clipboard for the window that has the keyboard: not yet on Linux.
// Unverified: stub, replaced in Task 7
#[cfg(not(windows))]
pub fn copy_text_foreground(_text: &str) -> Result<(), String> {
    Err("not built yet".to_owned())
}

/// `text` on the clipboard (the guide's Copy: navigator.clipboard.writeText).
#[cfg(windows)]
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

/// `text` on the clipboard: not yet on Linux.
// Unverified: stub, replaced in Task 7
#[cfg(not(windows))]
pub fn copy_text(_hwnd: HWND, _text: &str) -> Result<(), String> {
    Err("not built yet".to_owned())
}

/// The clipboard's text (a menu text box's Ctrl+V, the port's own); None when it holds no text or will not open.
#[cfg(windows)]
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

/// The clipboard's text: none yet on Linux.
// Unverified: stub, replaced in Task 7
#[cfg(not(windows))]
pub fn paste_text(_hwnd: HWND) -> Option<String> {
    None
}

/// A shortcut the settings make (bridge.ts ShortcutKind).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutKind {
    Desktop,
    StartMenu,
}

/// The shortcut's file: `MVS Classic.lnk` on the desktop or in the Start menu's programs (shortcuts.ts link).
#[cfg(windows)]
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

#[cfg(windows)]
fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain([0]).collect()
}

/// Whether the shortcut is there and points at `exe` (shortcuts.ts state's ours).
#[cfg(windows)]
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

/// The shortcut's file name, in the desktop folder or the menu's applications folder.
#[cfg(not(windows))]
const SHORTCUT_FILE: &str = "mvsclassic.desktop";

/// The shortcut's file: a desktop entry in the desktop folder (`xdg-user-dir DESKTOP`, else `~/Desktop`) or in the
/// menu's applications folder ([`applications_dir`]).
#[cfg(not(windows))]
fn link(kind: ShortcutKind) -> Option<PathBuf> {
    let dir = match kind {
        ShortcutKind::Desktop => desktop_dir()?,
        ShortcutKind::StartMenu => applications_dir()?,
    };
    Some(dir.join(SHORTCUT_FILE))
}

/// The desktop folder: `xdg-user-dir DESKTOP`'s answer, else `~/Desktop`.
#[cfg(not(windows))]
fn desktop_dir() -> Option<PathBuf> {
    let asked = std::process::Command::new("xdg-user-dir")
        .arg("DESKTOP")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|t| PathBuf::from(t.trim_end_matches('\n')))
        .filter(|p| p.is_absolute());
    asked.or_else(|| std::env::var_os("HOME").filter(|h| !h.is_empty()).map(|h| PathBuf::from(h).join("Desktop")))
}

/// `$XDG_DATA_HOME` when it is an absolute path, else `$HOME/.local/share` (the XDG Base Directory spec).
#[cfg(not(windows))]
fn data_home(xdg: Option<std::ffi::OsString>, home: Option<std::ffi::OsString>) -> Option<PathBuf> {
    if let Some(x) = xdg.map(PathBuf::from).filter(|x| x.is_absolute()) {
        return Some(x);
    }
    home.filter(|h| !h.is_empty()).map(|h| PathBuf::from(h).join(".local").join("share"))
}

/// The user's applications folder, where the menu reads desktop entries: `<data home>/applications`.
#[cfg(not(windows))]
pub(crate) fn applications_dir() -> Option<PathBuf> {
    data_home(std::env::var_os("XDG_DATA_HOME"), std::env::var_os("HOME")).map(|d| d.join("applications"))
}

/// A desktop entry's string value (the Desktop Entry spec's string type): `\\`, `\n`, `\t`, `\r` escaped.
#[cfg(not(windows))]
fn desktop_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t").replace('\r', "\\r")
}

/// One argument of a desktop entry's `Exec=` (the Desktop Entry spec, "The Exec key"), as the file holds it: quoted
/// whole, with `"`, `` ` ``, `$` and `\\` escaped inside the quotes and `%` doubled, then escaped as a string.
#[cfg(not(windows))]
pub(crate) fn desktop_exec_arg(p: &Path) -> String {
    let mut q = String::from('"');
    for c in p.to_string_lossy().chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                q.push('\\');
                q.push(c);
            }
            '%' => q.push_str("%%"),
            c => q.push(c),
        }
    }
    q.push('"');
    desktop_string(&q)
}

/// The shortcut's desktop entry: the launcher `exe`, started in its folder `dir`.
#[cfg(not(windows))]
fn shortcut_desktop_entry(exe: &Path, dir: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=MVS Classic\nExec={}\nPath={}\nTerminal=false\nCategories=Game;\n",
        desktop_exec_arg(exe),
        desktop_string(&dir.to_string_lossy())
    )
}

/// Whether the desktop entry `entry` starts `exe` (its `Exec=` line is the one [`shortcut_desktop_entry`] writes).
#[cfg(not(windows))]
fn names_exe(entry: &str, exe: &Path) -> bool {
    let want = format!("Exec={}", desktop_exec_arg(exe));
    entry.lines().any(|l| l.trim_end() == want)
}

/// Whether the shortcut is there and starts `exe` (shortcuts.ts state's ours).
#[cfg(not(windows))]
fn ours(kind: ShortcutKind, exe: &Path) -> bool {
    link(kind).and_then(|f| std::fs::read_to_string(f).ok()).is_some_and(|t| names_exe(&t, exe))
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
#[cfg(windows)]
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

/// The shortcut made, a desktop entry starting the launcher in the app's folder (shortcuts.ts create), mode 0755
/// (a desktop launches only an executable entry); one already there is written again.
#[cfg(not(windows))]
pub fn create_shortcut(kind: ShortcutKind) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let dir = app_dir().ok_or(w::NO_SHORTCUTS_DEV)?;
    let exe = dir.join(mvs_release::LAUNCHER_EXE);
    if exe.to_str().is_none() {
        return Err(format!("{}: not UTF-8, which a desktop entry cannot name", exe.display()));
    }
    let file = link(kind).ok_or(w::NO_SHORTCUT_FOLDER)?;
    let at = |e: std::io::Error| format!("{}: {e}", file.display());
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    std::fs::write(&file, shortcut_desktop_entry(&exe, &dir)).map_err(at)?;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).map_err(at)
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

    #[cfg(not(windows))]
    #[test]
    fn an_exec_argument_is_quoted_and_escaped() {
        assert_eq!(desktop_exec_arg(Path::new("/a b/MVSC Launcher")), r#""/a b/MVSC Launcher""#);
        // (`$`, `"`, `` ` `` and `\` escaped inside the quotes, then the backslashes again for the string)
        assert_eq!(desktop_exec_arg(Path::new("/a$b/L")), r#""/a\\$b/L""#);
        assert_eq!(desktop_exec_arg(Path::new("/a\"b/L")), r#""/a\\"b/L""#);
        assert_eq!(desktop_exec_arg(Path::new("/a`b\\c/L")), r#""/a\\`b\\\\c/L""#);
        assert_eq!(desktop_exec_arg(Path::new("/100%/L")), r#""/100%%/L""#);
    }

    #[cfg(not(windows))]
    #[test]
    fn the_data_home_follows_xdg_data_home() {
        let os = |s: &str| Some(std::ffi::OsString::from(s));
        assert_eq!(data_home(os("/x/data"), os("/home/a")), Some(PathBuf::from("/x/data")));
        // (empty or relative: the spec's default)
        assert_eq!(data_home(os(""), os("/home/a")), Some(PathBuf::from("/home/a/.local/share")));
        assert_eq!(data_home(os("rel"), os("/home/a")), Some(PathBuf::from("/home/a/.local/share")));
        assert_eq!(data_home(None, os("/home/a")), Some(PathBuf::from("/home/a/.local/share")));
        assert_eq!(data_home(None, None), None);
    }

    #[cfg(not(windows))]
    #[test]
    fn the_shortcut_starts_the_launcher_in_its_folder() {
        let exe = Path::new("/home/a/MVS Classic tést/MVSC Launcher.exe");
        let text = shortcut_desktop_entry(exe, exe.parent().unwrap());
        assert!(text.starts_with("[Desktop Entry]\n"));
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.contains(&r#"Exec="/home/a/MVS Classic tést/MVSC Launcher.exe""#));
        assert!(lines.contains(&"Path=/home/a/MVS Classic tést"));
        assert!(lines.contains(&"Type=Application") && lines.contains(&"Name=MVS Classic"));
        assert!(names_exe(&text, exe));
        assert!(!names_exe(&text, Path::new("/elsewhere/MVSC Launcher.exe")));
    }
}
