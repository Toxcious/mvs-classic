//! The worker's job object: the worker dies with the game (the job's last handle, the game's, closing kills it, a
//! crash of the game included) and its committed memory is capped (an allocation past the cap fails, the worker
//! ends and the parent reports it).

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, QueryInformationJobObject,
    SetInformationJobObject, TerminateJobObject,
};
#[cfg(windows)]
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

#[cfg(windows)]
use crate::{Error, Result};

#[cfg(windows)]
fn os(what: &str) -> impl FnOnce(windows::core::Error) -> Error + '_ {
    move |source| Error::Os { what: what.to_string(), source }
}

/// A job object holding one process.
#[cfg(windows)]
#[derive(Debug)]
pub struct Job(HANDLE);

// SAFETY: a job handle is a kernel object handle, usable from any thread.
#[cfg(windows)]
unsafe impl Send for Job {}

#[cfg(windows)]
impl Job {
    /// A job that kills its processes when its handle closes, each capped at `memory` committed bytes.
    pub fn new(memory: u64) -> Result<Job> {
        // SAFETY: an unnamed job with default security; the handle is owned by the Job and closed on drop.
        let h = unsafe { CreateJobObjectW(None, None) }.map_err(os("CreateJobObjectW"))?;
        let job = Job(h);
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
        info.ProcessMemoryLimit = usize::try_from(memory).unwrap_or(usize::MAX);
        // SAFETY: the job's live handle and a limit struct of the size given, read by the call.
        unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
        .map_err(os("SetInformationJobObject"))?;
        Ok(job)
    }

    /// The process `pid` put in the job.
    pub fn assign(&self, pid: u32) -> Result<()> {
        // SAFETY: a process handle opened with the rights the assignment needs, closed after it.
        unsafe {
            let p = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid).map_err(os("OpenProcess"))?;
            let r = AssignProcessToJobObject(self.0, p).map_err(os("AssignProcessToJobObject"));
            let _ = CloseHandle(p);
            r
        }
    }

    /// The most memory a process of the job has committed (bytes).
    pub fn peak_memory(&self) -> u64 {
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        // SAFETY: the job's live handle; the struct of the size given is written by the call.
        let r = unsafe {
            QueryInformationJobObject(
                Some(self.0),
                JobObjectExtendedLimitInformation,
                (&raw mut info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                None,
            )
        };
        if r.is_ok() { info.PeakProcessMemoryUsed as u64 } else { 0 }
    }

    /// Every process of the job ended now.
    pub fn kill(&self) {
        // SAFETY: the job's live handle.
        let _ = unsafe { TerminateJobObject(self.0, 1) };
    }
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle CreateJobObjectW gave, closed once (its processes end with it).
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// The memory cap of the job this process runs in (the worker's own), or None outside a capped job.
#[cfg(windows)]
pub fn own_memory_cap() -> Option<u64> {
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    // SAFETY: a null handle names the calling process's job; the struct of the size given is written by the call.
    let r = unsafe {
        QueryInformationJobObject(
            None,
            JobObjectExtendedLimitInformation,
            (&raw mut info).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            None,
        )
    };
    let capped = info.BasicLimitInformation.LimitFlags.0 & JOB_OBJECT_LIMIT_PROCESS_MEMORY.0 != 0;
    (r.is_ok() && capped).then_some(info.ProcessMemoryLimit as u64)
}

// Deviation: Linux has no job object. The worker's memory is capped by RLIMIT_DATA (its private writable memory,
// malloc's maps included, where the job caps its committed memory; a read-only file map is counted by neither), set
// in the child before its exec with PR_SET_PDEATHSIG (the worker is killed when the thread that started it ends: the
// game's main thread, so the game's end, a crash included). The kill and the drop signal it through a pidfd.
#[cfg(not(windows))]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
#[cfg(not(windows))]
use std::os::unix::process::CommandExt;
#[cfg(not(windows))]
use std::path::Path;
#[cfg(not(windows))]
use std::process::Command;
#[cfg(not(windows))]
use std::sync::OnceLock;
#[cfg(not(windows))]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(not(windows))]
use crate::{Error, Result};

/// One capped process: its cap, the process once assigned, the most memory seen of it.
#[cfg(not(windows))]
#[derive(Debug)]
pub struct Job {
    memory: u64,
    process: OnceLock<Process>,
    peak: AtomicU64,
}

/// The job's process, as signals reach it.
#[cfg(not(windows))]
#[derive(Debug)]
enum Process {
    /// Its pidfd (and pid): a signal goes to it, never to a later process given its reaped pid.
    Fd(OwnedFd, u32),
    /// A kernel without pidfd_open (before 5.3): its pid alone.
    Pid(u32),
}

#[cfg(not(windows))]
impl Job {
    /// A job that kills its process when it drops, capped at `memory` private writable bytes.
    pub fn new(memory: u64) -> Result<Job> {
        Ok(Job { memory, process: OnceLock::new(), peak: AtomicU64::new(0) })
    }

    /// A command of the job's process at `exe`: in the child, before its exec, the memory cap set and the parent's
    /// death signal asked (a child whose parent ended before the ask ends itself).
    pub fn command(&self, exe: &Path) -> Command {
        let parent = std::process::id() as libc::pid_t;
        let mut now = libc::rlimit { rlim_cur: libc::RLIM_INFINITY, rlim_max: libc::RLIM_INFINITY };
        // SAFETY: the calling process's limit, written to the struct given.
        let hard = if unsafe { libc::getrlimit(libc::RLIMIT_DATA, &mut now) } == 0 {
            now.rlim_max
        } else {
            libc::RLIM_INFINITY
        };
        let cap = libc::rlimit { rlim_cur: (self.memory as libc::rlim_t).min(hard), rlim_max: hard };
        let mut c = Command::new(exe);
        // SAFETY: the closure runs in the forked child before its exec and calls only async-signal-safe functions
        // (setrlimit, prctl, getppid, _exit; the error is errno's, nothing allocated).
        unsafe {
            c.pre_exec(move || {
                if libc::setrlimit(libc::RLIMIT_DATA, &cap) != 0
                    || libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong) != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                // (the parent ended before the prctl: its death signal never comes)
                if libc::getppid() != parent {
                    libc::_exit(1);
                }
                Ok(())
            });
        }
        c
    }

    /// The process `pid` put in the job.
    pub fn assign(&self, pid: u32) -> Result<()> {
        // SAFETY: a plain syscall; the fd it gives is owned below.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
        let p = if fd >= 0 {
            // SAFETY: the new fd of the call above, owned once.
            Process::Fd(unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) }, pid)
        } else {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::ENOSYS) {
                return Err(Error::Platform(format!("pidfd_open: {e}")));
            }
            Process::Pid(pid)
        };
        let _ = self.process.set(p);
        Ok(())
    }

    /// The signal `sig` sent to the job's process (0: none, its presence tested); whether it was sent.
    fn signal(&self, sig: libc::c_int) -> bool {
        match self.process.get() {
            // SAFETY: the job's live pidfd and no siginfo.
            Some(Process::Fd(fd, _)) => unsafe {
                libc::syscall(libc::SYS_pidfd_send_signal, fd.as_raw_fd(), sig, std::ptr::null::<libc::siginfo_t>(), 0)
                    == 0
            },
            // SAFETY: a plain signal to the job's pid.
            Some(Process::Pid(pid)) => unsafe { libc::kill(*pid as libc::pid_t, sig) == 0 },
            None => false,
        }
    }

    /// The most memory the job's process has held so far (bytes).
    // Deviation: its peak resident memory (/proc/<pid>/status VmHWM), where the job gives its peak committed memory.
    pub fn peak_memory(&self) -> u64 {
        let pid = match self.process.get() {
            Some(Process::Fd(_, pid) | Process::Pid(pid)) => *pid,
            None => return 0,
        };
        // (read while the process is there: a zombie's status has no memory, a reaped pid may be another's)
        let now = if self.signal(0) {
            std::fs::read_to_string(format!("/proc/{pid}/status")).ok().and_then(|s| status_bytes(&s, "VmHWM"))
        } else {
            None
        };
        let seen = now.unwrap_or(0);
        self.peak.fetch_max(seen, Ordering::Relaxed).max(seen)
    }

    /// Every process of the job ended now.
    pub fn kill(&self) {
        self.signal(libc::SIGKILL);
    }
}

#[cfg(not(windows))]
impl Drop for Job {
    fn drop(&mut self) {
        // (as the job's close does: a process still there ends; its pidfd closes after)
        self.kill();
        // Deviation: a Linux process ended stays a zombie until its parent waits on it, where the job's close is the
        // whole cleanup. The wait runs on a thread of its own (the launcher's frame drops workers and must not
        // block); through the pidfd it can only reap this process. ECHILD (the poll's try_wait reaped it) is fine.
        if let Some(p) = self.process.take() {
            let _ = std::thread::Builder::new().name("import reap".into()).spawn(move || reap(&p));
        }
    }
}

/// The job's process waited on until it ends and reaped (the pidfd closes after, with `p`).
#[cfg(not(windows))]
fn reap(p: &Process) {
    loop {
        let r = match p {
            Process::Fd(fd, _) => {
                // SAFETY: zeroed siginfo is a valid value of the plain struct, written by the call.
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                // SAFETY: the job's live pidfd and a siginfo the call writes.
                unsafe { libc::waitid(libc::P_PIDFD, fd.as_raw_fd() as libc::id_t, &mut info, libc::WEXITED) }
            }
            // (a kernel without pidfd: the pid, as std's own wait does)
            // SAFETY: a wait on the job's pid, no status asked.
            Process::Pid(pid) => unsafe { libc::waitpid(*pid as libc::pid_t, std::ptr::null_mut(), 0) },
        };
        if r != -1 || std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return;
        }
    }
}

/// A `/proc/<pid>/status` line's kB in bytes (`VmHWM:    1234 kB`).
#[cfg(not(windows))]
fn status_bytes(status: &str, key: &str) -> Option<u64> {
    let kb = status.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(':')?.trim().strip_suffix("kB"))?;
    kb.trim().parse::<u64>().ok().map(|k| k * 1024)
}

/// The memory cap of the job this process runs in (the worker's own), or None outside a capped job.
#[cfg(not(windows))]
pub fn own_memory_cap() -> Option<u64> {
    let mut r = libc::rlimit { rlim_cur: libc::RLIM_INFINITY, rlim_max: libc::RLIM_INFINITY };
    // SAFETY: the calling process's limit, written to the struct given.
    let ok = unsafe { libc::getrlimit(libc::RLIMIT_DATA, &mut r) } == 0;
    (ok && r.rlim_cur != libc::RLIM_INFINITY).then_some(r.rlim_cur)
}

#[cfg(all(test, not(windows)))]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::Job;

    /// Set in the helper's run: it is the worker's parent.
    const PARENT: &str = "MVS_JOB_TEST_PARENT";

    /// The helper of the test below, run in a child process of it: starts a worker (`sleep`) as
    /// `Worker::start_exe` does, prints its pid and waits to be killed.
    #[test]
    #[ignore = "the_worker_dies_with_its_parent runs it in a process of its own"]
    fn a_worker_parent() {
        if std::env::var_os(PARENT).is_none() {
            return;
        }
        let job = Job::new(1 << 30).unwrap();
        let mut child = job.command(Path::new("sleep")).arg("30").stdin(Stdio::null()).spawn().unwrap();
        job.assign(child.id()).unwrap();
        println!("worker pid {}", child.id());
        std::thread::sleep(Duration::from_secs(30));
        // (not reached when the test kills this process as it should)
        let _ = child.kill();
        let _ = child.wait();
    }

    /// Whether process `pid` has ended (gone, or a zombie its new parent has not reaped yet).
    fn ended(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map_or(true, |s| s.rsplit_once(") ").is_some_and(|(_, rest)| rest.starts_with('Z')))
    }

    #[test]
    fn the_worker_dies_with_its_parent() {
        let mut parent = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "import::job::tests::a_worker_parent", "--ignored", "--nocapture"])
            .env(PARENT, "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let out = BufReader::new(parent.stdout.take().unwrap());
        let pid: u32 = out
            .lines()
            .map_while(Result::ok)
            .find_map(|l| l.strip_prefix("worker pid ").and_then(|p| p.parse().ok()))
            .expect("the helper's worker pid");
        assert!(!ended(pid), "the worker runs");
        parent.kill().unwrap();
        parent.wait().unwrap();
        let t = Instant::now();
        while !ended(pid) && t.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let gone = ended(pid);
        if !gone {
            // SAFETY: a plain signal to the worker this test started, still there.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
        assert!(gone, "the worker {pid} outlived its parent");
    }
}
