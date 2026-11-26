//! The import as a child process: `mvs-launcher.exe
//! --import-worker <op>` (the exe that starts it: the self-test's is `mvs.exe`) reads its request as one JSON line
//! on stdin and writes `mvs_ui::launcher::import_view::ImportEvent` JSON lines on stdout; the parent reads them on
//! a thread into a channel the launcher drains each frame. The child runs
//! below normal priority in a job object (`job`) that caps its memory and kills it when its parent ends; a stdin line
//! `cancel` sets the import's cancel flag, a kill follows after 5 s, and the next start cleans what a killed run
//! left (the store's dead lock, its temp files: `serve`'s state op and the run).
//!
//! The setup flow over it: `detect` lists the beta's folders found (one
//! `Detected`), a folder the player picks is named by `check` (one `Source`); a usable one (`Beta`, its key opening
//! the paks) goes in the run's request as `paks`. No run starts while the game's program runs (its store's packs are
//! open; the run switches them). `state` says whether setup holds and what is stale.

pub mod job;
pub mod serve;
pub mod steam;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use mvs_ui::launcher::import_view::{ImportEvent, LogLevel, Say};

use crate::{Error, Result};

pub use crate::launcher::setup_os::store_dir;

/// An op of the worker (the request's fields each op reads are named on them).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportRequest {
    /// The data root: the store (`store_dir`) and setup.json go in it (run, state).
    pub data_root: PathBuf,
    /// The run: the beta folder setup chose (empty: setup.json's). The check: the folder to name.
    pub paks: PathBuf,
    /// The detect op: the folder setup used last (settings' beta_dir), offered first.
    #[serde(default)]
    pub last: Option<PathBuf>,
    /// The run is setup's step 3 (else a rebuild from Home): the launcher's view of it.
    #[serde(default)]
    pub setup: bool,
    /// The game opens once the run ends well.
    #[serde(default)]
    pub then_play: bool,
    /// Every kind rebuilt.
    pub force: bool,
    /// The worker threads (0: the import's own choice, the machine's logical cores).
    pub threads: usize,
}

/// The worker's operations (`--import-worker <op>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerOp {
    /// The import whole.
    Run,
    /// Whether setup holds, and the store's state against the paks (staleness).
    State,
    /// The beta's folders on this machine (Steam's libraries).
    Detect,
    /// A folder's check (mvs_pak::setup's SourceCheck).
    Check,
}

impl WorkerOp {
    /// Its name on the command line.
    pub fn name(self) -> &'static str {
        match self {
            WorkerOp::Run => "run",
            WorkerOp::State => "state",
            WorkerOp::Detect => "detect",
            WorkerOp::Check => "check",
        }
    }

    pub fn parse(s: &str) -> Option<WorkerOp> {
        [WorkerOp::Run, WorkerOp::State, WorkerOp::Detect, WorkerOp::Check].into_iter().find(|o| o.name() == s)
    }
}

const GB: u64 = 1 << 30;
/// The least and most memory a worker may commit: the import's peak at 16 threads measured 1.7 to 2.3 GB of
/// working set (mvs-import run/memory.rs); the min-spec run takes 2.5 GB.
const MIN_CAP: u64 = 5 * GB / 2;
const MAX_CAP: u64 = 8 * GB;
/// The time a cancelled worker has to end before it is killed.
// Deviation: the import process is not killed 1 s after its `ended` reply, and only then;
// here a worker that has not ended 5 s after the cancel is killed (a cancel the importer is slow to see, a hung
// worker), and one that answers is let end on its own.
const CANCEL_GRACE: Duration = Duration::from_secs(5);

/// The worker's memory cap on this machine: three quarters of the memory free now, within 2.5 to 8 GB.
// Unverified: the share and the bounds are this port's choice (an import in one process is uncapped).
pub fn default_memory_cap() -> u64 {
    (mvs_import::run::memory::free_memory() / 4 * 3).clamp(MIN_CAP, MAX_CAP)
}

/// Windows' process creation flags: no console window, below normal priority.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// The Windows process flags taken and dropped (a Linux child has no console to hide).
// (the Linux branch of the flags: the spawn's chain stays the Windows one)
// Deviation: the import worker runs at normal priority on Linux, where Windows starts it below normal.
#[cfg(not(windows))]
trait CommandExt {
    fn creation_flags(&mut self, flags: u32) -> &mut Self;
}

#[cfg(not(windows))]
impl CommandExt for Command {
    fn creation_flags(&mut self, _flags: u32) -> &mut Self {
        self
    }
}

/// A worker process and its event reader.
#[derive(Debug)]
pub struct Worker {
    child: Child,
    stdin: Option<ChildStdin>,
    events: Receiver<ImportEvent>,
    /// Holds the child: closing it (the Worker dropped, the game ended) kills it.
    job: job::Job,
    /// An event that ends the op was seen, or the poll gave the worker's end.
    ended: bool,
    /// When the cancel was asked.
    cancelled: Option<Instant>,
    /// The job was told to end the worker.
    killed: bool,
}

impl Worker {
    /// The worker started on `op` with `req` (below normal priority, in a job object with the memory cap).
    pub fn start(op: WorkerOp, req: &ImportRequest) -> Result<Worker> {
        Worker::start_capped(op, req, default_memory_cap())
    }

    /// [`Worker::start`] with the memory cap given (bytes).
    pub fn start_capped(op: WorkerOp, req: &ImportRequest, memory: u64) -> Result<Worker> {
        let exe =
            std::env::current_exe().map_err(|e| Error::Match(format!("the import worker: the exe's path: {e}")))?;
        Worker::start_exe(&exe, op, req, memory)
    }

    /// [`Worker::start_capped`] of the game's exe at `exe` (the tests': Cargo's `mvs`).
    pub fn start_exe(exe: &Path, op: WorkerOp, req: &ImportRequest, memory: u64) -> Result<Worker> {
        #[cfg(windows)]
        use std::os::windows::process::CommandExt;
        let os = |what: &str, e: std::io::Error| Error::Match(format!("the import worker: {what}: {e}"));
        let job = job::Job::new(memory)?;
        #[cfg(windows)]
        let mut child = Command::new(exe)
            .args(["--import-worker", op.name()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
            .spawn()
            .map_err(|e| os("its start", e))?;
        // (the job's command: its cap and its parent's death signal are set in the child before its exec)
        #[cfg(not(windows))]
        let mut child = job
            .command(exe)
            .args(["--import-worker", op.name()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
            .spawn()
            .map_err(|e| os("its start", e))?;
        // Unverified: the child runs a moment before it is in the job (std starts no process suspended); it reads
        // its request from stdin first, written below, so nothing of the import runs outside the cap.
        if let Err(e) = job.assign(child.id()) {
            let _ = child.kill();
            return Err(e);
        }
        let mut stdin = child.stdin.take();
        let line = serde_json::to_string(req).map_err(|e| Error::Match(format!("the import request: {e}")))?;
        if let Some(s) = stdin.as_mut() {
            writeln!(s, "{line}").and_then(|()| s.flush()).map_err(|e| os("its request", e))?;
        }
        let stdout = child.stdout.take().ok_or_else(|| Error::Match("the import worker: no stdout".into()))?;
        let (tx, events) = mpsc::channel();
        std::thread::Builder::new()
            .name("import events".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if line.trim().is_empty() {
                        continue;
                    }
                    // (a line that is no event: a note of the worker's, kept in the log)
                    let e = serde_json::from_str(&line).unwrap_or_else(|_| ImportEvent::Log {
                        level: LogLevel::Info,
                        say: Say::Error { detail: line.clone() },
                    });
                    if tx.send(e).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| os("its reader thread", e))?;
        Ok(Worker { child, stdin, events, job, ended: false, cancelled: None, killed: false })
    }

    /// The events come since the last poll; never blocks. A worker that ended without its answer (a crash, a kill,
    /// the memory cap) gives one `Failed` naming its exit code, or `Cancelled`
    /// once the cancel was asked; a cancelled worker still there after 5 s is killed.
    pub fn poll(&mut self) -> Vec<ImportEvent> {
        let mut out = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(e) => {
                    self.ended |= e.ends();
                    out.push(e);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // the worker's stdout closed without its answer: its end, once the process is gone (asked,
                    // never waited on: the launcher's frame must not block)
                    if !self.ended {
                        match self.child.try_wait() {
                            Ok(None) => {}
                            #[cfg(windows)]
                            Ok(Some(status)) => self.gone(status.code(), &mut out),
                            // Deviation: a worker killed by a signal is named by 128 + the signal (the shell's
                            // convention: SIGKILL 137), where the job's TerminateJobObject gives 1.
                            #[cfg(not(windows))]
                            Ok(Some(status)) => {
                                use std::os::unix::process::ExitStatusExt;
                                self.gone(status.code().or_else(|| status.signal().map(|s| 128 + s)), &mut out);
                            }
                            Err(_) => self.gone(None, &mut out),
                        }
                    }
                    break;
                }
            }
        }
        if let Some(at) = self.cancelled
            && !self.ended
            && !self.killed
            && at.elapsed() >= CANCEL_GRACE
        {
            self.kill();
        }
        out
    }

    /// The end of a worker that ended without its answer: `Cancelled` when the cancel was asked (it was killed after
    /// its grace, or ended before it could answer), else `Failed` naming its exit code.
    fn gone(&mut self, code: Option<i32>, out: &mut Vec<ImportEvent>) {
        self.ended = true;
        if self.cancelled.is_some() {
            out.push(ImportEvent::Log { level: LogLevel::Info, say: Say::Cancelled });
            out.push(ImportEvent::Cancelled);
            return;
        }
        let say = Say::WorkerStopped { code };
        out.push(ImportEvent::Log { level: LogLevel::Error, say: say.clone() });
        out.push(ImportEvent::Failed { say });
    }

    /// The worker's process id.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// The most memory the worker has committed so far (bytes).
    pub fn peak_memory(&self) -> u64 {
        self.job.peak_memory()
    }

    /// Whether the op has ended (its answer or its end polled).
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// The cancel asked (a stdin line); the kill follows after 5 s if the worker is still there (`poll`).
    pub fn cancel(&mut self) {
        if self.cancelled.is_some() {
            return;
        }
        self.cancelled = Some(Instant::now());
        if let Some(s) = self.stdin.as_mut() {
            let _ = writeln!(s, "cancel").and_then(|()| s.flush());
        }
    }

    /// The worker killed now (the program quits). Its partial files are the next start's to clean.
    pub fn kill(&mut self) {
        // (asynchronous: the poll gives its end once the process is gone)
        self.killed = true;
        self.job.kill();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // (the job's handle closes after: its kill-on-close ends a worker still there)
        self.stdin = None;
    }
}

/// The worker's main (`mvs.exe --import-worker <op>`): the request read from stdin, the events written to stdout;
/// the process's exit code (0: a run done or an op answered).
pub fn worker_main(op: &str) -> i32 {
    let out = serve::Out::default();
    let Some(op) = WorkerOp::parse(op) else {
        out.send(&ImportEvent::Failed { say: Say::Error { detail: format!("the import worker: no op {op}") } });
        return 2;
    };
    let cancel = Arc::new(AtomicBool::new(false));
    // stdin: the request's line, then `cancel`; its end (the game gone) is no cancel: the job ends the worker
    let (tx, rx) = mpsc::channel::<String>();
    {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            let mut lines = std::io::stdin().lines();
            match lines.next() {
                Some(Ok(l)) => {
                    let _ = tx.send(l);
                }
                _ => return,
            }
            for l in lines {
                match l {
                    Ok(l) if l.trim() == "cancel" => cancel.store(true, Ordering::Relaxed),
                    Ok(_) => {}
                    Err(_) => return,
                }
            }
        });
    }
    let req: ImportRequest = match rx
        .recv()
        .map_err(|e| e.to_string())
        .and_then(|l| serde_json::from_str(&l).map_err(|e| format!("the import request does not read: {e}")))
    {
        Ok(r) => r,
        Err(e) => {
            out.send(&ImportEvent::Failed { say: Say::Error { detail: e } });
            return 2;
        }
    };
    serve::serve(op, &req, &out, &cancel)
}
