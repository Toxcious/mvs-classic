//! The worker's job object: the worker dies with the game (the job's last handle, the game's, closing kills it, a
//! crash of the game included) and its committed memory is capped (an allocation past the cap fails, the worker
//! ends and the parent reports it).

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, QueryInformationJobObject,
    SetInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

use crate::{Error, Result};

fn os(what: &str) -> impl FnOnce(windows::core::Error) -> Error + '_ {
    move |source| Error::Os { what: what.to_string(), source }
}

/// A job object holding one process.
#[derive(Debug)]
pub struct Job(HANDLE);

// SAFETY: a job handle is a kernel object handle, usable from any thread.
unsafe impl Send for Job {}

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

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle CreateJobObjectW gave, closed once (its processes end with it).
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// The memory cap of the job this process runs in (the worker's own), or None outside a capped job.
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
