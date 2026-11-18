//! What a run takes of the machine: its priority, its default number of jobs and its peak memory.

/// One job's peak memory at most: the largest trace (bot-ffa-b, 18000 frames) run alone with `--jobs 1` peaks at
/// 1046 MB, the frames read held whole.
pub const JOB_BYTES: u64 = 1 << 30;

/// The process's own memory before its jobs (lab-picture, the smallest trace, peaks at 67 MB).
pub const BASE_BYTES: u64 = 100 << 20;

/// The default `--jobs`: half the machine's threads, or the build slot's share (`MVS_MAX_THREADS`, set by
/// `tools/cargo-slot.ps1`) when that is fewer, and fewer still when the jobs' peak memory would pass a quarter of its
/// physical memory. A run then leaves the desktop and the rest of the CPU to the user.
pub fn default_jobs() -> usize {
    let threads = std::thread::available_parallelism().map_or(2, std::num::NonZero::get);
    let slot = std::env::var("MVS_MAX_THREADS").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(usize::MAX);
    let threads = threads.min(slot.saturating_mul(2));
    let by_memory = physical_memory()
        .map_or(usize::MAX, |m| usize::try_from((m / 4).saturating_sub(BASE_BYTES) / JOB_BYTES).unwrap_or(usize::MAX));
    (threads / 2).min(by_memory).max(1)
}

/// The machine's physical memory.
pub fn physical_memory() -> Option<u64> {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut s = MEMORYSTATUSEX { dwLength: u32::try_from(size_of::<MEMORYSTATUSEX>()).ok()?, ..Default::default() };
        // SAFETY: a MEMORYSTATUSEX with its length set, written by the call.
        if unsafe { GlobalMemoryStatusEx(&mut s) }.is_ok() {
            return Some(s.ullTotalPhys);
        }
        None
    }
    #[cfg(not(windows))]
    {
        let t = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kb = t
            .lines()
            .find_map(|l| l.strip_prefix("MemTotal:")?.trim().strip_suffix("kB")?.trim().parse::<u64>().ok())?;
        Some(kb * 1024)
    }
}

/// Lowers this process's priority below normal (its threads, the jobs, inherit it): the desktop stays responsive
/// while a run takes the CPU.
pub fn lower_priority() {
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{BELOW_NORMAL_PRIORITY_CLASS, GetCurrentProcess, SetPriorityClass};
        // SAFETY: the current process's pseudo handle, which needs no closing.
        let _ = unsafe { SetPriorityClass(GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS) };
    }
}

/// The process's peak working set, in bytes.
pub fn peak_memory() -> Option<u64> {
    #[cfg(windows)]
    {
        use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows::Win32::System::Threading::GetCurrentProcess;
        let mut c = PROCESS_MEMORY_COUNTERS::default();
        let size = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
        // SAFETY: the current process's pseudo handle and a counters struct of the size given.
        if unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut c, size) }.is_ok() {
            return u64::try_from(c.PeakWorkingSetSize).ok();
        }
        None
    }
    #[cfg(not(windows))]
    None
}
