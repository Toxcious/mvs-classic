//! The Linux output, beside `wasapi.rs` (selected as `out::wasapi` on Linux, so callers keep its name): the same
//! public surface, the mixer on a thread of its own rendered into nothing at the audio clock's pace, as the WASAPI
//! output does while it has no device.

// Unverified: stub, replaced in Task 8

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::mix::Mixer;
use crate::{AudioError, QUANTUM, RATE};

/// The running output: its thread, which owns the mixer.
pub struct Wasapi {
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
    pub(crate) stop: Arc<AtomicBool>,
}

/// The device gets silence (here there is no device; kept for the same surface as `wasapi.rs`).
static MUTED: AtomicBool = AtomicBool::new(false);

/// Mutes or unmutes every output of the process (a scripted or test run's sound mixed but not heard).
pub fn set_muted(on: bool) {
    MUTED.store(on, Ordering::Relaxed);
}

/// The channels the stub mixes for: stereo.
const CHANNELS: usize = 2;

impl Wasapi {
    /// Starts the mixer on the output thread.
    pub fn start(mixer: Box<Mixer>) -> Result<Wasapi, AudioError> {
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let thread = std::thread::Builder::new()
            .name("audio-out".into())
            .spawn(move || run(mixer, &s))
            .map_err(|e| AudioError::Output(format!("the output thread: {e}")))?;
        Ok(Wasapi { thread: Some(thread), stop })
    }

    /// The device's output channels (what the mixer is made for), before the mixer is.
    pub fn device_channels() -> Result<usize, AudioError> {
        Ok(CHANNELS)
    }
}

impl Drop for Wasapi {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The mixer at the clock's pace, its output dropped (the pacing of `wasapi.rs`'s run without a device).
fn run(mut mixer: Box<Mixer>, stop: &AtomicBool) {
    let mut buf = vec![0.0; QUANTUM * mixer.channels];
    let since = Instant::now();
    let mut done = 0u64;
    while !stop.load(Ordering::Acquire) {
        let due = since.elapsed().as_secs_f64() * f64::from(RATE) / QUANTUM as f64;
        while (done as f64) < due {
            mixer.render(&mut buf);
            done += 1;
        }
        std::thread::sleep(Duration::from_secs_f64(QUANTUM as f64 / f64::from(RATE)));
    }
}
