//! The game's program as the launcher sees it (app/src/window.ts openGame, the `started` / `ended` game events): PLAY
//! starts the game (`MVS Classic.exe`, `mvs.exe` in a build) with `--play --store <the store>` beside the launcher's exe, Home shows it running until it ends, and
//! PLAY while it runs brings its window forward. The game keeps running when the launcher closes.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Instant;

#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, EnumWindows, GW_OWNER, GetWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    SW_RESTORE, SetForegroundWindow, ShowWindow,
};
#[cfg(windows)]
use windows::core::BOOL;

/// What the game is started with besides its store.
#[derive(Clone, Debug, Default)]
pub struct GameArgs {
    /// The game's program (beside the launcher's, mvs_release::game_exe).
    pub exe: PathBuf,
    /// `--data`: the data folder the launcher was given (None: the default, which the game finds the same way).
    pub data: Option<PathBuf>,
    /// `--online-defaults`: the release's online.json the launcher reads.
    pub online_defaults: PathBuf,
    /// `--adapter`: the GPU the launcher was told to use.
    pub adapter: Option<String>,
    /// The launcher's arguments it does not know itself, passed on to the game as they came (`--no-audio`,
    /// `--size 1920x1080`).
    pub pass_on: Vec<String>,
}

impl GameArgs {
    /// The game's program for this launcher: in the release's game folder, else (a dev build) beside it.
    pub fn beside_this_exe() -> PathBuf {
        let exe = std::env::current_exe().unwrap_or_default();
        exe.parent().map_or_else(|| PathBuf::from(mvs_release::PLATFORM_GAME_EXE), mvs_release::game_exe)
    }

    /// The command line for a run on `store`: the menus from the title (`--play`), the store opened at once.
    pub fn line(&self, store: &Path) -> Vec<OsString> {
        let mut a: Vec<OsString> = vec!["--play".into(), "--store".into(), store.into()];
        a.extend(["--online-defaults".into(), self.online_defaults.clone().into()]);
        if let Some(d) = &self.data {
            a.extend(["--data".into(), d.into()]);
        }
        if let Some(g) = &self.adapter {
            a.extend(["--adapter".into(), g.into()]);
        }
        a.extend(self.pass_on.iter().map(OsString::from));
        a
    }

    /// The command line for a run on `store` that joins the lobby of `code` once online play lets it (a join link,
    /// launcher::link): `line`'s, then `--online-open join:<code>`.
    pub fn join_line(&self, store: &Path, code: &str) -> Vec<OsString> {
        let mut a = self.line(store);
        a.extend(["--online-open".into(), format!("join:{code}").into()]);
        a
    }
}

/// The game's process, started from PLAY.
#[derive(Debug)]
pub struct Game {
    child: Child,
    started: Instant,
}

impl Game {
    /// The game started on `store`. Its console lines go where the launcher's go (a console it was started from).
    pub fn start(args: &GameArgs, store: &Path) -> std::io::Result<Game> {
        let dir = args.exe.parent().map(Path::to_path_buf).unwrap_or_default();
        let child = Command::new(&args.exe).args(args.line(store)).current_dir(dir).spawn()?;
        Ok(Game { child, started: Instant::now() })
    }

    /// The game started on `store` to join the lobby of `code` (GameArgs::join_line).
    pub fn start_joining(args: &GameArgs, store: &Path, code: &str) -> std::io::Result<Game> {
        let dir = args.exe.parent().map(Path::to_path_buf).unwrap_or_default();
        let child = Command::new(&args.exe).args(args.join_line(store, code)).current_dir(dir).spawn()?;
        Ok(Game { child, started: Instant::now() })
    }

    /// Whether the process ended (polled each frame).
    pub fn ended(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(None))
    }

    /// The seconds since it started.
    pub fn seconds(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// When it started, on the wall clock (a file it wrote is as new or newer).
    pub fn started_at(&self) -> std::time::SystemTime {
        std::time::SystemTime::now() - self.started.elapsed()
    }

    /// Its window brought forward, restored when minimised (window.ts openGame on a game window that is open).
    pub fn bring_forward(&self) {
        raise(self.child.id());
    }
}

/// The top-level window of process `pid` brought forward, restored when minimised (the game's, by its pid: Play, a
/// join link).
#[cfg(windows)]
pub fn raise(pid: u32) {
    let mut found = (pid, HWND::default());
    extern "system" fn each(hwnd: HWND, l: LPARAM) -> BOOL {
        // SAFETY: `l` is the `found` pair EnumWindows was handed, alive through the call.
        let found = unsafe { &mut *(l.0 as *mut (u32, HWND)) };
        let mut pid = 0;
        // SAFETY: plain reads of a top-level window EnumWindows handed over.
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == found.0 && IsWindowVisible(hwnd).as_bool() && GetWindow(hwnd, GW_OWNER).is_err() {
                found.1 = hwnd;
                return BOOL(0);
            }
        }
        BOOL(1)
    }
    // SAFETY: the callback reads `found` only during the call; the window found is the game's own.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut (u32, HWND) as isize));
        if found.1.is_invalid() {
            return;
        }
        let _ = AllowSetForegroundWindow(found.0);
        if IsIconic(found.1).as_bool() {
            let _ = ShowWindow(found.1, SW_RESTORE);
        }
        let _ = SetForegroundWindow(found.1);
    }
}

/// The top-level window of process `pid` brought forward: nothing on Linux.
// Deviation: Wayland lets no program raise another's window, so the launcher leaves the game's where it is (the
// game can raise its own).
#[cfg(not(windows))]
pub fn raise(_pid: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_is_started_on_the_store_with_the_launchers_folders() {
        let a = GameArgs {
            exe: PathBuf::from("C:/MVS/mvs.exe"),
            data: Some(PathBuf::from("D:/data")),
            online_defaults: PathBuf::from("C:/MVS/online.json"),
            adapter: None,
            pass_on: Vec::new(),
        };
        let line: Vec<String> =
            a.line(Path::new("D:/data/store")).iter().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(
            line,
            ["--play", "--store", "D:/data/store", "--online-defaults", "C:/MVS/online.json", "--data", "D:/data"]
        );
    }

    #[test]
    fn the_launchers_unknown_arguments_go_to_the_game_last() {
        let a = GameArgs {
            exe: PathBuf::from("C:/MVS/mvs.exe"),
            data: None,
            online_defaults: PathBuf::from("C:/MVS/online.json"),
            adapter: None,
            pass_on: vec!["--size".into(), "1920x1080".into(), "--no-audio".into()],
        };
        let line: Vec<String> =
            a.join_line(Path::new("D:/store"), "ABCD").iter().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(
            line,
            [
                "--play",
                "--store",
                "D:/store",
                "--online-defaults",
                "C:/MVS/online.json",
                "--size",
                "1920x1080",
                "--no-audio",
                "--online-open",
                "join:ABCD"
            ]
        );
    }
}
