//! The perf readouts: CPU sections (sim:step, sim:show, views, render, hud, ui), the
//! renderer's GPU passes and a UI mark, the frame intervals (mean and worst over 0.5 s), the process's and the GPU's
//! memory, to stderr and as an overlay line; the `stat fps` line.

use windows::Win32::Graphics::Dxgi::{
    DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter3, IDXGIDevice,
};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::core::Interface;

use mvs_render::gpu::Gpu;

/// How long a readout counts, s (0.5 s of frames).
const WINDOW: f64 = 0.5;

/// A readout of one window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Readout {
    pub frames: u32,
    /// The frame intervals' mean and worst, ms.
    pub mean_ms: f64,
    pub worst_ms: f64,
    /// Each CPU section's mean per frame, ms, in first-met order.
    pub sections: Vec<(&'static str, f64)>,
    /// The GPU's frame and passes, the last read, ms.
    pub gpu_ms: Option<f64>,
    pub passes: Vec<(&'static str, f64)>,
    /// The process's private bytes and the GPU's local memory in use, MB.
    pub private_mb: f64,
    pub gpu_mb: Option<f64>,
}

impl Readout {
    /// `stat fps`'s line: "<n> fps".
    pub fn fps_line(&self) -> String {
        if self.mean_ms > 0.0 { format!("{:.0} fps", 1000.0 / self.mean_ms) } else { String::new() }
    }

    /// The whole line: the rate, the intervals, the sections, the GPU and the memory.
    pub fn line(&self) -> String {
        let mut s = format!("{} | frame {:.2} ms (worst {:.2})", self.fps_line(), self.mean_ms, self.worst_ms);
        for (n, ms) in &self.sections {
            s += &format!(" {n} {ms:.2}");
        }
        if let Some(g) = self.gpu_ms {
            s += &format!(" | gpu {g:.2}");
            for (n, ms) in &self.passes {
                s += &format!(" {n} {ms:.2}");
            }
        }
        s += &format!(" | mem {:.0} MB", self.private_mb);
        if let Some(g) = self.gpu_mb {
            s += &format!(" gpu {g:.0} MB");
        }
        s
    }
}

/// The readouts' sections and intervals.
#[derive(Debug, Default)]
pub struct Perf {
    sections: Vec<(&'static str, f64)>,
    intervals: Vec<f64>,
    last: Option<f64>,
    start: f64,
    gpu_ms: Option<f64>,
    passes: Vec<(&'static str, f64)>,
    adapter: Option<IDXGIAdapter3>,
    /// The last window's readout (None before the first).
    pub readout: Option<Readout>,
    /// Write each readout's line to stderr.
    pub log: bool,
    /// A timed run's totals (`--bench`), counted from when it is set.
    pub totals: Option<Totals>,
}

/// A timed run's totals: every frame interval, each CPU section's and GPU pass's sum, the GPU frame's reads.
#[derive(Debug, Default)]
pub struct Totals {
    intervals: Vec<f64>,
    sections: Vec<(&'static str, f64)>,
    gpu: (f64, u32),
    passes: Vec<(&'static str, f64, u32)>,
}

impl Totals {
    fn add(list: &mut Vec<(&'static str, f64)>, name: &'static str, ms: f64) {
        match list.iter_mut().find(|s| s.0 == name) {
            Some(s) => s.1 += ms,
            None => list.push((name, ms)),
        }
    }

    fn pass(&mut self, name: &'static str, ms: f64) {
        match self.passes.iter_mut().find(|s| s.0 == name) {
            Some(s) => {
                s.1 += ms;
                s.2 += 1;
            }
            None => self.passes.push((name, ms, 1)),
        }
    }

    /// The run's line: the frames, the intervals' mean, median, 95th and 99th percentiles and worst, the intervals
    /// over 1.5 times the median, each section's mean per frame, the GPU frame's and passes' means, the memory.
    pub fn line(&self) -> String {
        let mut iv = self.intervals.clone();
        if iv.is_empty() {
            return "no frames".to_owned();
        }
        iv.sort_by(f64::total_cmp);
        let n = iv.len();
        let q = |p: f64| iv[(((n - 1) as f64) * p).round() as usize];
        let mean = iv.iter().sum::<f64>() / n as f64;
        let missed = iv.iter().filter(|&&v| v > 1.5 * q(0.5)).count();
        let mut s = format!(
            "{n} frames | frame mean {mean:.2} p50 {:.2} p95 {:.2} p99 {:.2} worst {:.2} ms, {missed} over 1.5x median",
            q(0.5),
            q(0.95),
            q(0.99),
            iv[n - 1]
        );
        s += " | cpu";
        for (name, ms) in &self.sections {
            s += &format!(" {name} {:.3}", ms / n as f64);
        }
        if self.gpu.1 > 0 {
            s += &format!(" | gpu {:.3}", self.gpu.0 / f64::from(self.gpu.1));
            for (name, ms, k) in &self.passes {
                s += &format!(" {name} {:.3}", ms / f64::from(*k));
            }
        }
        let (private, peak) = memory();
        s += &format!(" | mem {:.0} MB, peak working set {:.0} MB", mb(private), mb(peak));
        s
    }
}

fn mb(b: usize) -> f64 {
    b as f64 / (1 << 20) as f64
}

impl Perf {
    /// Readouts with the GPU memory of `gpu`'s adapter.
    pub fn new(gpu: &Gpu, log: bool) -> Perf {
        let adapter = gpu
            .dev
            .cast::<IDXGIDevice>()
            .and_then(|d| {
                // SAFETY: a live DXGI device.
                unsafe { d.GetAdapter() }
            })
            .and_then(|a| a.cast::<IDXGIAdapter3>())
            .ok();
        Perf { adapter, log, ..Perf::default() }
    }

    /// A CPU section's time of this frame, ms.
    pub fn section(&mut self, name: &'static str, ms: f64) {
        Totals::add(&mut self.sections, name, ms);
        if let Some(t) = &mut self.totals {
            Totals::add(&mut t.sections, name, ms);
        }
    }

    /// The renderer's GPU frame and passes (a frame a few back), and the UI's mark.
    pub fn gpu(&mut self, frame_ms: Option<f64>, passes: &[(&'static str, f64)]) {
        if let Some(g) = frame_ms {
            self.gpu_ms = Some(g);
            self.passes.clear();
            self.passes.extend_from_slice(passes);
            if let Some(t) = &mut self.totals {
                t.gpu.0 += g;
                t.gpu.1 += 1;
                for &(n, ms) in passes {
                    t.pass(n, ms);
                }
            }
        }
    }

    /// A GPU pass of the app's own (the UI), ms: added to the passes of the last read.
    pub fn gpu_pass(&mut self, name: &'static str, ms: f64) {
        if let Some(t) = &mut self.totals {
            t.pass(name, ms);
        }
        match self.passes.iter_mut().find(|s| s.0 == name) {
            Some(s) => s.1 = ms,
            None => self.passes.push((name, ms)),
        }
    }

    /// The frame ended at `now` (seconds): its interval taken; a readout every 0.5 s.
    pub fn frame(&mut self, now: f64) {
        let Some(last) = self.last.replace(now) else {
            self.start = now;
            return;
        };
        self.intervals.push((now - last) * 1e3);
        if let Some(t) = &mut self.totals {
            t.intervals.push((now - last) * 1e3);
        }
        if now - self.start < WINDOW {
            return;
        }
        let n = self.intervals.len() as f64;
        let r = Readout {
            frames: self.intervals.len() as u32,
            mean_ms: self.intervals.iter().sum::<f64>() / n,
            worst_ms: self.intervals.iter().copied().fold(0.0, f64::max),
            sections: self.sections.iter().map(|&(s, ms)| (s, ms / n)).collect(),
            gpu_ms: self.gpu_ms,
            passes: self.passes.clone(),
            private_mb: mb(memory().0),
            gpu_mb: self.adapter.as_ref().and_then(gpu_bytes).map(|b| b as f64 / (1 << 20) as f64),
        };
        if self.log {
            eprintln!("mvs: {}", r.line());
        }
        self.readout = Some(r);
        self.intervals.clear();
        self.sections.clear();
        self.start = now;
    }
}

/// The steps of a load timed one after another: `lap` closes the step that ran since the last lap. Its line is the
/// `mvs: load` readout.
#[derive(Clone, Debug)]
pub struct LoadLaps {
    start: std::time::Instant,
    last: std::time::Instant,
    /// Each step's name and ms, in order (a name met again adds to its first entry).
    pub laps: Vec<(&'static str, f64)>,
    /// Notes after the steps (the sound scopes' own lines).
    pub notes: Vec<String>,
}

impl Default for LoadLaps {
    fn default() -> LoadLaps {
        LoadLaps::new()
    }
}

impl LoadLaps {
    pub fn new() -> LoadLaps {
        let now = std::time::Instant::now();
        LoadLaps { start: now, last: now, laps: Vec::new(), notes: Vec::new() }
    }

    /// Closes the step `name` at now.
    pub fn lap(&mut self, name: &'static str) {
        let now = std::time::Instant::now();
        let ms = (now - self.last).as_secs_f64() * 1e3;
        self.last = now;
        match self.laps.iter_mut().find(|(n, _)| *n == name) {
            Some((_, t)) => *t += ms,
            None => self.laps.push((name, ms)),
        }
    }

    /// The time since the start, ms.
    pub fn total_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1e3
    }

    /// A step's ms (0 when it did not run).
    pub fn get(&self, name: &str) -> f64 {
        self.laps.iter().find(|(n, _)| *n == name).map_or(0.0, |(_, t)| *t)
    }

    /// "<total> ms: <step> <ms>, ... | <note>; <note>".
    pub fn line(&self) -> String {
        let mut s = format!("{:.0} ms:", self.total_ms());
        for (i, (n, ms)) in self.laps.iter().enumerate() {
            s += &format!("{} {n} {ms:.0}", if i == 0 { "" } else { "," });
        }
        if !self.notes.is_empty() {
            s += " | ";
            s += &self.notes.join("; ");
        }
        s
    }
}

/// The process's private bytes, MB.
pub fn private_mb() -> f64 {
    memory().0 as f64 / (1024.0 * 1024.0)
}

/// The process's private bytes and peak working set (PROCESS_MEMORY_COUNTERS_EX PrivateUsage, PeakWorkingSetSize).
fn memory() -> (usize, usize) {
    let mut c = PROCESS_MEMORY_COUNTERS_EX::default();
    // SAFETY: the current process and a counters struct of the size given.
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut c as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    };
    if ok.is_ok() { (c.PrivateUsage, c.PeakWorkingSetSize) } else { (0, 0) }
}

/// Where the process's private memory sits, MB: the heaps' bytes in use and committed (the program's own
/// allocations, the driver's too where it uses a heap), the private memory committed outside them (the driver's
/// own allocations, stacks), and the GPU's local and non-local memory in use by the process.
pub fn private_parts(gpu: &Gpu) -> String {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Graphics::Dxgi::DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL;
    use windows::Win32::System::Memory::{
        GetProcessHeaps, HEAP_SUMMARY, HeapSummary, MEM_COMMIT, MEM_PRIVATE, MEMORY_BASIC_INFORMATION, VirtualQuery,
    };
    const MB: usize = 1 << 20;
    let mut heaps = vec![HANDLE::default(); 512];
    // SAFETY: a buffer of the length given.
    let n = (unsafe { GetProcessHeaps(&mut heaps) } as usize).min(heaps.len());
    let (mut used, mut held) = (0, 0);
    for &h in &heaps[..n] {
        let mut s = HEAP_SUMMARY { cb: size_of::<HEAP_SUMMARY>() as u32, ..Default::default() };
        // SAFETY: a heap of this process and a summary of the size given.
        if unsafe { HeapSummary(h, 0, &mut s) }.as_bool() {
            used += s.cbAllocated;
            held += s.cbCommitted;
        }
    }
    let mut private = 0;
    let mut at = 0usize;
    loop {
        let mut m = MEMORY_BASIC_INFORMATION::default();
        // SAFETY: an address of this process and an out struct of the size given.
        let got = unsafe { VirtualQuery(Some(at as *const _), &mut m, size_of::<MEMORY_BASIC_INFORMATION>()) };
        if got == 0 || m.RegionSize == 0 {
            break;
        }
        if m.State == MEM_COMMIT && m.Type == MEM_PRIVATE {
            private += m.RegionSize;
        }
        at = m.BaseAddress as usize + m.RegionSize;
    }
    let adapter = gpu
        .dev
        .cast::<IDXGIDevice>()
        .and_then(|d| {
            // SAFETY: a live DXGI device.
            unsafe { d.GetAdapter() }
        })
        .and_then(|a| a.cast::<IDXGIAdapter3>())
        .ok();
    let video = |group| {
        let mut i = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        adapter
            .as_ref()
            // SAFETY: a live adapter, node 0 and an out struct.
            .and_then(|a| unsafe { a.QueryVideoMemoryInfo(0, group, &mut i) }.ok())
            .map_or(0, |()| i.CurrentUsage as usize)
    };
    format!(
        "heap {} MB used of {} MB; other private {} MB; gpu {} MB, shared {} MB",
        used / MB,
        held / MB,
        private.saturating_sub(held) / MB,
        video(DXGI_MEMORY_SEGMENT_GROUP_LOCAL) / MB,
        video(DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL) / MB
    )
}

/// The adapter's local memory in use by this process.
fn gpu_bytes(a: &IDXGIAdapter3) -> Option<u64> {
    let mut i = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
    // SAFETY: a live adapter, node 0 and an out struct.
    unsafe { a.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut i) }.ok().map(|()| i.CurrentUsage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn laps_add_by_name() {
        let mut l = LoadLaps::new();
        l.lap("data");
        l.lap("world");
        l.lap("data");
        assert_eq!(l.laps.iter().map(|(n, _)| *n).collect::<Vec<_>>(), ["data", "world"]);
        assert!(l.line().contains(" data ") && l.line().contains(", world "));
    }

    #[test]
    fn a_window_averages_its_intervals() {
        let mut p = Perf::default();
        p.frame(0.0);
        for k in 1..=40 {
            p.section("sim:step", 1.0);
            p.frame(f64::from(k) / 60.0 + if k == 10 { 0.01 } else { 0.0 });
        }
        let r = p.readout.expect("a readout");
        assert_eq!(r.frames, 30);
        assert!((r.mean_ms - 1000.0 / 60.0).abs() < 1e-9);
        assert!(r.worst_ms > 26.0);
        assert_eq!(r.sections, [("sim:step", 1.0)]);
        assert_eq!(r.fps_line(), "60 fps");
    }
}
