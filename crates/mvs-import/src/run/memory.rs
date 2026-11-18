//! The import's memory: its bounds from the memory free when it starts, and the gate that bounds the bytes of the
//! built entries on their way to the pack writers.

use std::sync::{Condvar, Mutex};

/// What a run may hold, from the free memory at its start.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    /// The physical memory free at the start (bytes; an estimate where the OS does not say).
    pub free: u64,
    /// The package cache's budget ([`mvs_uasset::PackageCache::trim`]).
    pub cache: usize,
    /// The bytes of compressed entries in flight to the writers.
    pub entries: u64,
}

const MB: u64 = 1 << 20;

impl Budget {
    /// A thirty-second of the free memory for the cache, 128 to 256 MB; a sixty-fourth for the entries in flight,
    /// 16 to 256 MB. Measured on the cache's heap count, every kind with the dev kinds into an empty store at 16
    /// threads, one run each: a cache of 128 MB ran 138 s at a peak commit of 3.1 GB, 256 MB 108 and 120 s at 2.9 and
    /// 3.1 GB, 512 MB 131 s at 3.0 GB, 1 GB 122 s at 3.5 GB. The times are within one another's noise and the peak
    /// is set by the builds themselves (the first wave's character entries), not the cache, up to 512 MB; 1 GB adds
    /// its size.
    pub fn from_free(free: u64) -> Budget {
        let cache = (free / 32).clamp(128 * MB, 256 * MB);
        Budget {
            free,
            cache: usize::try_from(cache).unwrap_or(usize::MAX),
            entries: (free / 64).clamp(16 * MB, 256 * MB),
        }
    }

    pub fn now() -> Budget {
        Budget::from_free(free_memory())
    }
}

/// The physical memory free now.
pub fn free_memory() -> u64 {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut s = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        // SAFETY: a MEMORYSTATUSEX with its length set, written by the call.
        if unsafe { GlobalMemoryStatusEx(&mut s) }.is_ok() {
            return s.ullAvailPhys;
        }
    }
    #[cfg(not(windows))]
    if let Some(kb) = std::fs::read_to_string("/proc/meminfo").ok().and_then(|t| {
        t.lines().find_map(|l| l.strip_prefix("MemAvailable:")?.trim().strip_suffix("kB")?.trim().parse::<u64>().ok())
    }) {
        return kb * 1024;
    }
    4 << 30
}

/// Lowers this process's priority to below normal, never raising it (an idle-priority run stays idle): the machine
/// stays usable while the import takes every core. An earlier importer ran below
/// normal priority too.
pub fn lower_priority() {
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{
            BELOW_NORMAL_PRIORITY_CLASS, GetCurrentProcess, GetPriorityClass, IDLE_PRIORITY_CLASS, SetPriorityClass,
        };
        // SAFETY: the current process's pseudo handle, which needs no closing.
        let class = unsafe { GetPriorityClass(GetCurrentProcess()) };
        if class != 0 && class != IDLE_PRIORITY_CLASS.0 && class != BELOW_NORMAL_PRIORITY_CLASS.0 {
            // SAFETY: as above.
            let _ = unsafe { SetPriorityClass(GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS) };
        }
    }
}

/// Bounds the bytes held between the workers and the writers: a worker waits before it sends while the bytes in
/// flight and its own would pass the bound, unless none are in flight (an entry larger than the bound still goes, so
/// the import never stops).
pub struct Gate {
    bound: u64,
    held: Mutex<(u64, u64)>,
    freed: Condvar,
}

impl Gate {
    pub fn new(bound: u64) -> Gate {
        Gate { bound, held: Mutex::new((0, 0)), freed: Condvar::new() }
    }

    /// Takes `n` bytes, waiting while they do not fit.
    pub fn take(&self, n: u64) {
        let mut h = self.held.lock().unwrap_or_else(|e| e.into_inner());
        while h.0 > 0 && h.0 + n > self.bound {
            h = self.freed.wait(h).unwrap_or_else(|e| e.into_inner());
        }
        h.0 += n;
        h.1 = h.1.max(h.0);
    }

    /// Gives back `n` bytes a writer has appended.
    pub fn give(&self, n: u64) {
        let mut h = self.held.lock().unwrap_or_else(|e| e.into_inner());
        h.0 -= n;
        drop(h);
        self.freed.notify_all();
    }

    /// The most bytes held at once.
    pub fn peak(&self) -> u64 {
        self.held.lock().unwrap_or_else(|e| e.into_inner()).1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn budget_bounds() {
        let b = Budget::from_free(0);
        assert_eq!((b.cache as u64, b.entries), (128 * MB, 16 * MB));
        let b = Budget::from_free(64 << 30);
        assert_eq!((b.cache as u64, b.entries), (256 * MB, 256 * MB));
    }

    #[test]
    fn gate_waits_and_lets_a_large_one_through() {
        let g = Arc::new(Gate::new(10));
        g.take(25); // larger than the bound, nothing in flight: goes
        let g2 = g.clone();
        let t = std::thread::spawn(move || g2.take(5));
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(!t.is_finished(), "waits while 25 are held");
        g.give(25);
        t.join().unwrap();
        g.take(5);
        assert_eq!(g.peak(), 25);
    }
}
