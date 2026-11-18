//! The updater. A release is a zip the player unpacks into a folder of their choosing, and the
//! exe updates that folder itself. The feed (`mvs_release::FEED`, set when the release was built; none: never
//! checked) is the GitHub API's latest release, whose asset is the zip (`mvs_release::Release`). The launcher checks it when it opens, off the
//! frame's thread; when the player asks, the zip is downloaded into `<exe dir>\data\update\`, its size and SHA-256
//! checked, unpacked there, and the unpacked copy started with `--finish-update <exe dir> --wait-pid <pid>` while
//! this one quits: a running exe cannot overwrite its own files. That copy waits for this one to end, copies its
//! files over the exe's folder (`data\`, the player's, never), starts the folder's exe and ends. The next start
//! clears `data\update\`. `--update-now` checks, downloads and hands over with no window.

pub mod finish;
pub mod http;

use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use mvs_release::{DATA_DIR, Release, UPDATE_DIR, newer};
use mvs_ui::launcher::words as w;
use sha2::{Digest, Sha256};

/// Why a check, a download or a hand-over did not finish.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UpdateError {
    #[error("{}", w::NO_FEED)]
    NoFeed,
    #[error("{}", w::bad_feed_url(.0))]
    BadUrl(String),
    /// WinHTTP's own failure (no network, a name not found, TLS).
    #[error("{}", w::feed_unreached(.0))]
    Http(String),
    #[error("{}", w::feed_answered(*.0))]
    FeedStatus(u32),
    #[error("{}", w::download_answered(*.0))]
    DownloadStatus(u32),
    #[error("{}", w::latest_unread(.0))]
    Latest(String),
    #[error("{}", w::download_short(*.got, *.size))]
    Short { got: u64, size: u64 },
    #[error("{}", w::download_long(*.0))]
    Long(u64),
    #[error("{}", w::DOWNLOAD_HASH)]
    Hash,
    #[error("{}", w::not_unpacked(.0))]
    Unpack(String),
    #[error("{}", w::no_exe(.0))]
    NoExe(String),
    #[error("{}", w::not_under(.from, .target))]
    NotUnder { from: String, target: String },
    /// A file or folder of the update's that could not be written, removed or started.
    #[error("{0}")]
    Io(String),
}

fn io_at(p: &Path) -> impl Fn(std::io::Error) -> UpdateError + '_ {
    move |e| UpdateError::Io(format!("{}: {e}", p.display()))
}

/// Where this exe lives: its release folder and its path in it (`game/MVS Classic.exe` for the game's).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub app_dir: PathBuf,
    pub exe: String,
}

impl Place {
    /// This exe's.
    pub fn current() -> Result<Place, UpdateError> {
        let exe = std::env::current_exe().map_err(|e| UpdateError::Io(e.to_string()))?;
        let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        let root = mvs_release::app_root(&dir);
        let name = exe.strip_prefix(&root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        Ok(Place { app_dir: root, exe: name })
    }

    /// Where a release is downloaded and unpacked: under `data\`, which the copy over the folder leaves alone.
    pub fn stage(&self) -> PathBuf {
        self.app_dir.join(DATA_DIR).join(UPDATE_DIR)
    }
}

/// The feed's release when it is newer than `version`, else None.
pub fn latest(feed: &str, version: &str) -> Result<Option<Release>, UpdateError> {
    let mut body = Vec::new();
    http::get(
        feed,
        |s| if (200..300).contains(&s) { Ok(()) } else { Err(UpdateError::FeedStatus(s)) },
        |chunk| {
            // (a release's JSON is a few KB, its notes with it: a feed that answers more is not one)
            if body.len() + chunk.len() > 1024 * 1024 {
                return Err(UpdateError::Latest(w::LATEST_TOO_LONG.to_owned()));
            }
            body.extend_from_slice(chunk);
            Ok(())
        },
    )?;
    let text = String::from_utf8(body).map_err(|e| UpdateError::Latest(e.to_string()))?;
    let r = Release::from_github(&text).map_err(UpdateError::Latest)?;
    Ok(newer(&r.version, version).then_some(r))
}

/// Removes a file or a folder whole; a missing one is fine. A virus scanner may hold a file just written for a
/// moment: tried `tries` times, `pause` apart.
pub fn remove(p: &Path, tries: u32, pause: Duration) -> std::io::Result<()> {
    let mut k = 1;
    loop {
        let r = match std::fs::symlink_metadata(p) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => Err(e),
            Ok(m) if m.is_dir() => std::fs::remove_dir_all(p),
            Ok(_) => std::fs::remove_file(p),
        };
        match r {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) if k >= tries => return Err(e),
            Err(_) => {
                k += 1;
                std::thread::sleep(pause);
            }
        }
    }
}

/// The stage's removal's tries and their pause.
const RM_TRIES: u32 = 10;
const RM_PAUSE: Duration = Duration::from_millis(300);

/// Downloads release `r` into the place's stage, checks its size and hash and unpacks it; `progress`
/// in whole percent, each change. The unpacked release's folder, which holds the exe (the zip's top level, or the
/// one folder at its top).
pub fn stage(place: &Place, r: &Release, mut progress: impl FnMut(u32)) -> Result<PathBuf, UpdateError> {
    let stage = place.stage();
    remove(&stage, RM_TRIES, RM_PAUSE).map_err(io_at(&stage))?;
    std::fs::create_dir_all(&stage).map_err(io_at(&stage))?;
    let zip = stage.join(&r.file);
    let mut out = std::io::BufWriter::new(std::fs::File::create(&zip).map_err(io_at(&zip))?);
    let mut hash = Sha256::new();
    let mut got = 0u64;
    let mut shown = None;
    http::get(
        &r.url,
        |s| if (200..300).contains(&s) { Ok(()) } else { Err(UpdateError::DownloadStatus(s)) },
        |chunk| {
            got += chunk.len() as u64;
            if got > r.size {
                return Err(UpdateError::Long(r.size));
            }
            hash.update(chunk);
            out.write_all(chunk).map_err(io_at(&zip))?;
            let pc = u32::try_from(got * 100 / r.size.max(1)).unwrap_or(100);
            if shown != Some(pc) {
                shown = Some(pc);
                progress(pc);
            }
            Ok(())
        },
    )?;
    out.flush().map_err(io_at(&zip))?;
    drop(out);
    if got != r.size {
        return Err(UpdateError::Short { got, size: r.size });
    }
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if digest != r.sha256 {
        return Err(UpdateError::Hash);
    }
    let dir = stage.join(&r.version);
    // Deviation: unpacked in process (mvs_release::zip: stored and deflate, CRC-32 checked, names kept inside).
    mvs_release::zip::unpack(&zip, &dir).map_err(UpdateError::Unpack)?;
    remove(&zip, RM_TRIES, RM_PAUSE).map_err(io_at(&zip))?;
    if dir.join(&place.exe).is_file() {
        return Ok(dir);
    }
    // Deviation: a zip of the release's folder rather than of its files is taken too, as well as one of the files at
    // the zip's root.
    let inner: Vec<PathBuf> =
        std::fs::read_dir(&dir).map_err(io_at(&dir))?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    match inner.as_slice() {
        [one] if one.join(&place.exe).is_file() => Ok(one.clone()),
        _ => Err(UpdateError::NoExe(place.exe.clone())),
    }
}

/// The process flags of what the updater starts: no console window, its own process group.
const SPAWN_FLAGS: u32 = 0x0800_0000 | 0x0000_0200;

/// Starts the unpacked release in `staged` to copy itself over the place's folder once this process ends. The caller
/// quits next.
pub fn hand_over(place: &Place, staged: &Path) -> Result<(), UpdateError> {
    let exe = staged.join(&place.exe);
    Command::new(&exe)
        .arg("--finish-update")
        .arg(&place.app_dir)
        .arg("--wait-pid")
        .arg(std::process::id().to_string())
        .current_dir(staged)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(SPAWN_FLAGS)
        .spawn()
        .map(drop)
        .map_err(io_at(&exe))
}

/// What the launcher hears of the updater.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateEvent {
    /// A newer release.
    Available(String),
    /// None newer.
    None,
    /// The download, in whole percent.
    Progress(u32),
    /// Downloaded and unpacked: ready to hand over.
    Ready(String),
    /// The check failed; a download going on goes on.
    CheckError(String),
    /// The download, its unpack or its hand-over failed.
    DownloadError(String),
}

/// How long after the start the last update's folder is cleared: the copy that finished
/// it may still be ending.
const CLEAR_AFTER: Duration = Duration::from_secs(5);

/// Held while a release is staged; true once one was in this process (the start's clearing then leaves the stage
/// alone). One for the process: two updaters never clear and fill the stage at once.
// Deviation: the clear and the download share a lock, so a download that starts in the clear's 5 s is not
// cleared under it.
static STAGE_LOCK: Mutex<bool> = Mutex::new(false);

/// The launcher's updater: its check and its download run on threads of their own; `poll` takes their events.
#[derive(Debug)]
pub struct Updater {
    feed: Option<String>,
    place: Option<Place>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// The release the last check found, while it is newer than this one.
    available: Option<Release>,
    /// The unpacked release, ready to hand over to.
    staged: Option<PathBuf>,
    busy: bool,
}

#[derive(Debug)]
enum Msg {
    Checked(Result<Option<Release>, UpdateError>),
    Progress(u32),
    Staged(Result<PathBuf, UpdateError>),
}

impl Updater {
    /// The updater of a build with `feed` (None: no check, nothing offered), at `place` (None: this exe's); the
    /// last update's folder cleared a moment after.
    pub fn new(feed: Option<&str>, place: Option<Place>) -> Updater {
        let (tx, rx) = mpsc::channel();
        let place = place.or_else(|| Place::current().ok());
        if let Some(p) = &place
            && p.stage().exists()
        {
            let stage = p.stage();
            // Deviation: cleared in every build, a dev run's included.
            let clear = move || {
                std::thread::sleep(CLEAR_AFTER);
                let started = STAGE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
                if !*started && let Err(e) = remove(&stage, RM_TRIES, RM_PAUSE) {
                    eprintln!("mvs: update: {} was not cleared: {e}", stage.display());
                }
            };
            let _ = std::thread::Builder::new().name("update clear".into()).spawn(clear);
        }
        Updater { feed: feed.map(str::to_owned), place, tx, rx, available: None, staged: None, busy: false }
    }

    /// The build names a feed: the launcher offers checks.
    pub fn enabled(&self) -> bool {
        self.feed.is_some()
    }

    /// The release found, while it is newer than this one.
    pub fn available(&self) -> Option<&str> {
        self.available.as_ref().map(|r| r.version.as_str())
    }

    /// A download or its hand-over goes on.
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Checks the feed off this thread against `version`: one `Available`, `None` or `Error` follows.
    pub fn check(&mut self, version: &str) {
        let Some(feed) = self.feed.clone() else {
            let _ = self.tx.send(Msg::Checked(Ok(None)));
            return;
        };
        let (tx, version) = (self.tx.clone(), version.to_owned());
        let run = move || {
            let _ = tx.send(Msg::Checked(latest(&feed, &version)));
        };
        if let Err(e) = std::thread::Builder::new().name("update check".into()).spawn(run) {
            let _ = self.tx.send(Msg::Checked(Err(UpdateError::Io(e.to_string()))));
        }
    }

    /// Downloads and unpacks the release found, off this thread: `Progress` events, then `Ready` or `Error`.
    pub fn download(&mut self) {
        let got = match (&self.feed, &self.place, &self.available) {
            (None, _, _) => Err(UpdateError::NoFeed),
            (_, None, _) => Err(UpdateError::Io(w::NO_EXE_DIR.to_owned())),
            (_, _, None) => Err(UpdateError::Io(w::NOTHING_FOUND.to_owned())),
            (Some(_), Some(place), Some(r)) => Ok((place.clone(), r.clone())),
        };
        let (place, r) = match got {
            Ok(got) => got,
            Err(e) => {
                let _ = self.tx.send(Msg::Staged(Err(e)));
                return;
            }
        };
        if self.busy {
            return;
        }
        self.busy = true;
        let tx = self.tx.clone();
        let run = move || {
            let mut started = STAGE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
            *started = true;
            let progress = tx.clone();
            let r = stage(&place, &r, |pc| {
                let _ = progress.send(Msg::Progress(pc));
            });
            drop(started);
            let _ = tx.send(Msg::Staged(r));
        };
        if let Err(e) = std::thread::Builder::new().name("update download".into()).spawn(run) {
            let _ = self.tx.send(Msg::Staged(Err(UpdateError::Io(e.to_string()))));
        }
    }

    /// Starts the staged release to copy itself over this folder; the caller quits next.
    pub fn install(&mut self) -> Result<(), UpdateError> {
        let (Some(place), Some(staged)) = (&self.place, &self.staged) else {
            return Err(UpdateError::Io(w::NOTHING_STAGED.to_owned()));
        };
        let r = hand_over(place, staged);
        if r.is_err() {
            self.busy = false;
        }
        r
    }

    /// The events since the last poll.
    pub fn poll(&mut self) -> Vec<UpdateEvent> {
        let mut out = Vec::new();
        while let Ok(m) = self.rx.try_recv() {
            out.push(match m {
                Msg::Checked(Ok(r)) => {
                    let ev = r.as_ref().map_or(UpdateEvent::None, |r| UpdateEvent::Available(r.version.clone()));
                    // (a check while a download goes on leaves the release being downloaded as it is)
                    if !self.busy {
                        self.available = r;
                    }
                    ev
                }
                Msg::Checked(Err(e)) => UpdateEvent::CheckError(e.to_string()),
                Msg::Progress(pc) => UpdateEvent::Progress(pc),
                Msg::Staged(Ok(dir)) => {
                    self.staged = Some(dir);
                    UpdateEvent::Ready(self.available().unwrap_or_default().to_owned())
                }
                Msg::Staged(Err(e)) => {
                    self.busy = false;
                    UpdateEvent::DownloadError(e.to_string())
                }
            });
        }
        out
    }
}

/// The exit codes of `--update-now`.
// Deviation: "nothing to install" and a hand-over do not share a code; the release
// check tells the three apart.
pub const UPDATE_HANDED_OVER: u8 = 0;
pub const UPDATE_FAILED: u8 = 1;
pub const UPDATE_NONE: u8 = 3;

/// `mvs.exe --update-now`: check, download and hand over with no window, a line a step on
/// stdout. The exit code: `UPDATE_HANDED_OVER` (the unpacked release now waits for this process to end),
/// `UPDATE_NONE` (no newer release) or `UPDATE_FAILED` (no feed in this build, or a step failed).
pub fn update_now_main() -> u8 {
    let say = |line: &str| println!("update-now: {line}");
    let Some(feed) = mvs_release::FEED else {
        say("not a release build (it names no update feed)");
        return UPDATE_FAILED;
    };
    let run = || -> Result<u8, UpdateError> {
        let place = Place::current()?;
        say(&format!("this is {}; checking", mvs_release::VERSION));
        let Some(r) = latest(feed, mvs_release::VERSION)? else {
            say("no newer release");
            return Ok(UPDATE_NONE);
        };
        say(&format!("{} is out", r.version));
        let dir = stage(&place, &r, |_| {})?;
        say("downloaded; installing");
        hand_over(&place, &dir)?;
        Ok(UPDATE_HANDED_OVER)
    };
    match run() {
        Ok(code) => code,
        Err(e) => {
            say(&format!("failed: {e}"));
            UPDATE_FAILED
        }
    }
}

/// `--finish-update <dir> --wait-pid <pid>`, run by the unpacked release's program the
/// updater ran (the launcher's, or the game's for `--update-now`): waits for `pid` to end, copies this copy's files
/// over `target`'s (both programs among them) and starts `target`'s launcher (either way). Exit 0 done, 1 not.
pub fn finish_update_main(target: &Path, pid: Option<u32>) -> u8 {
    let say = |line: &str| println!("finish-update: {line}");
    let place = match Place::current() {
        Ok(p) => p,
        Err(e) => {
            say(&format!("failed: {e}"));
            return 1;
        }
    };
    let code = match finish::finish(&place.app_dir, target, pid) {
        Ok(()) => {
            say(&format!("copied into {}", target.display()));
            0
        }
        Err(e @ UpdateError::NotUnder { .. }) => {
            say(&e.to_string());
            return 1;
        }
        Err(e) => {
            say(&format!("failed: {e}"));
            1
        }
    };
    // (the release starts as a player's double-click does: the launcher, with no argument)
    let exe = target.join(mvs_release::LAUNCHER_EXE);
    let started = Command::new(&exe)
        .current_dir(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(SPAWN_FLAGS)
        .spawn();
    if let Err(e) = started {
        say(&format!("{} did not start: {e}", exe.display()));
        return 1;
    }
    code
}
