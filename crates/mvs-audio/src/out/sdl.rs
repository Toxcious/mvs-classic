//! The Linux output, beside `wasapi.rs` (selected as `out::wasapi` on Linux, so callers keep its name): an SDL3 audio
//! stream on the default playback device, f32 at the device's channels, fed from the mixer's quanta by a thread of its
//! own that keeps about one device period and one quantum queued.
//!
//! SDL moves a stream opened on the default device to a new default by itself. It can still leave the stream silent
//! without saying so, in two ways: a lost device whose stream nothing takes from any more (PipeWire runs the device
//! from its own thread, which stops with the device), and a stream left on a lost device that SDL drains at the normal
//! rate but plays nowhere (when its move to the new default failed). Either one, held for a second, closes the stream
//! and opens the default again, at most once a second. With no device the mixer is rendered into nothing at the audio
//! clock's pace, as `wasapi.rs` does, and the default is tried again every second.

use std::ffi::{CStr, CString, c_int, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sdl3_sys::audio::{
    SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, SDL_AUDIO_F32, SDL_AudioDeviceID, SDL_AudioSpec, SDL_AudioStream,
    SDL_DestroyAudioStream, SDL_GetAudioDeviceFormat, SDL_GetAudioDeviceName, SDL_GetAudioPlaybackDevices,
    SDL_GetAudioStreamDevice, SDL_GetAudioStreamQueued, SDL_OpenAudioDeviceStream, SDL_PutAudioStreamData,
    SDL_ResumeAudioStreamDevice, SDL_SetAudioStreamGetCallback,
};
use sdl3_sys::error::SDL_GetError;
use sdl3_sys::hints::{SDL_GetHint, SDL_HINT_AUDIO_DEVICE_SAMPLE_FRAMES, SDL_SetHint};
use sdl3_sys::init::{SDL_INIT_AUDIO, SDL_InitSubSystem, SDL_QuitSubSystem};
use sdl3_sys::stdinc::SDL_free;

use crate::mix::Mixer;
use crate::{AudioError, QUANTUM, RATE};

/// The running output: its thread, which owns the mixer.
pub struct Wasapi {
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
    pub(crate) stop: Arc<AtomicBool>,
}

/// The channels mixed for when SDL cannot say what the device has.
const FALLBACK_CHANNELS: usize = 2;
/// How often a missing device is looked for again, and the shortest time between two opens.
const RETRY: Duration = Duration::from_secs(1);
/// How long a stream may go untaken, or stay on a lost device, before it is opened again.
const STALL: Duration = Duration::from_secs(1);
/// How often the stream's device is checked against the devices SDL lists.
const LOOK: Duration = Duration::from_millis(250);

/// The device gets silence: the mixer runs and renders as ever, its output is zeroed before the stream takes it.
static MUTED: AtomicBool = AtomicBool::new(false);

/// Mutes or unmutes every output of the process (a scripted or test run's sound mixed but not heard).
pub fn set_muted(on: bool) {
    MUTED.store(on, Ordering::Relaxed);
}

fn sdl_err(what: &str) -> AudioError {
    // SAFETY: SDL_GetError returns a nul-terminated string SDL owns (empty when none).
    let why = unsafe { CStr::from_ptr(SDL_GetError()) }.to_string_lossy().into_owned();
    AudioError::Output(format!("{what}: {why}"))
}

/// SDL's audio subsystem for its scope.
struct Sdl;

impl Sdl {
    /// Starts SDL's audio, on the caller's thread (SDL starts its subsystems on the main thread).
    fn init() -> Result<Sdl, AudioError> {
        // Deviation: SDL has no query for a device's smallest period, only this request, which it reads when the
        // device opens and its backend raises to its floor (PipeWire's is 32 frames). The request is the mixer's
        // quantum: the smallest that a quantum at a time fills, without setting a 32-frame period on every program
        // the PipeWire graph runs. A period the player sets in the environment is kept.
        let period = CString::new(QUANTUM.to_string()).expect("digits");
        // SAFETY: hint calls are safe from any thread; the strings are nul-terminated and SDL copies the value.
        // SDL_InitSubSystem is paired with SDL_QuitSubSystem in drop.
        unsafe {
            if SDL_GetHint(SDL_HINT_AUDIO_DEVICE_SAMPLE_FRAMES).is_null() {
                SDL_SetHint(SDL_HINT_AUDIO_DEVICE_SAMPLE_FRAMES, period.as_ptr());
            }
            if !SDL_InitSubSystem(SDL_INIT_AUDIO) {
                return Err(sdl_err("SDL audio"));
            }
        }
        Ok(Sdl)
    }
}

impl Drop for Sdl {
    fn drop(&mut self) {
        // SAFETY: this scope's SDL_InitSubSystem succeeded. The output thread quits it as it ends, while the thread
        // that started it waits on it in `Wasapi::drop`, so no other start or quit of SDL's runs at the same time.
        unsafe { SDL_QuitSubSystem(SDL_INIT_AUDIO) };
    }
}

impl Wasapi {
    /// Opens the default device and starts the mixer on the output thread.
    pub fn start(mut mixer: Box<Mixer>) -> Result<Wasapi, AudioError> {
        Wasapi::spawn(mixer.channels, move |b: &mut [f32]| mixer.render(b))
    }

    /// The output thread, pulling quanta of `channels` from `render`. Without SDL's audio it runs on the clock alone.
    fn spawn(channels: usize, render: impl FnMut(&mut [f32]) + Send + 'static) -> Result<Wasapi, AudioError> {
        let sdl = Sdl::init().inspect_err(|e| eprintln!("audio: {e}")).ok();
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let thread = std::thread::Builder::new()
            .name("audio-out".into())
            .spawn(move || run(render, channels, &s, sdl))
            .map_err(|e| AudioError::Output(format!("the output thread: {e}")))?;
        Ok(Wasapi { thread: Some(thread), stop })
    }

    /// The device's output channels (what the mixer is made for), before the mixer is. With no device, stereo: the
    /// mixer then runs on the clock (where `wasapi.rs` fails and the game has no sound engine).
    pub fn device_channels() -> Result<usize, AudioError> {
        let asked = Sdl::init().ok().and_then(|_sdl| {
            let mut spec = SDL_AudioSpec::default();
            // SAFETY: SDL's audio is started for this scope; spec is a live SDL_AudioSpec.
            unsafe { SDL_GetAudioDeviceFormat(SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, &mut spec, std::ptr::null_mut()) }
                .then_some(spec.channels)
        });
        Ok(asked.and_then(|c| usize::try_from(c).ok()).filter(|&c| c > 0).unwrap_or(FALLBACK_CHANNELS))
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

/// Wakes the output thread when SDL takes from the stream (on SDL's device thread).
unsafe extern "C" fn woken(userdata: *mut c_void, _stream: *mut SDL_AudioStream, _more: c_int, _total: c_int) {
    // SAFETY: userdata is the output thread's handle, which outlives the stream (`run`).
    unsafe { &*userdata.cast::<std::thread::Thread>() }.unpark();
}

/// An open stream on the default device.
struct Open {
    stream: *mut SDL_AudioStream,
    /// The stream's (logical) device.
    device: SDL_AudioDeviceID,
    /// The frames kept queued: the device's period and a quantum.
    target: usize,
    opened: Instant,
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: the stream is this Open's; destroying it closes the device SDL opened for it.
        unsafe { SDL_DestroyAudioStream(self.stream) };
    }
}

/// The frames to keep queued for a device period of `frames` at `freq`, in the mixer's frames.
fn target_for(frames: c_int, freq: c_int) -> usize {
    let frames = usize::try_from(frames).unwrap_or(0);
    let freq = usize::try_from(freq).ok().filter(|&f| f > 0).unwrap_or(RATE as usize);
    frames * RATE as usize / freq + QUANTUM
}

impl Open {
    /// The stream on the default device for the mixer's `channels`, its device resumed, `wake` woken at each take.
    fn new(channels: usize, wake: &std::thread::Thread) -> Result<Open, AudioError> {
        let spec = SDL_AudioSpec { format: SDL_AUDIO_F32, channels: channels as c_int, freq: RATE as c_int };
        // SAFETY: SDL's audio is started (the caller holds it); spec outlives the call. The stream is owned by the
        // Open made at once, so every return below destroys it.
        let stream =
            unsafe { SDL_OpenAudioDeviceStream(SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, &spec, None, std::ptr::null_mut()) };
        if stream.is_null() {
            return Err(sdl_err("the default playback device"));
        }
        // SAFETY: a live stream.
        let device = unsafe { SDL_GetAudioStreamDevice(stream) };
        let mut o = Open { stream, device, target: QUANTUM * 2, opened: Instant::now() };
        let (frames, freq) = o.period().ok_or_else(|| sdl_err("the device's format"))?;
        o.target = target_for(frames, freq);
        // SAFETY: `wake` outlives the stream (`run` holds it past every Open); SDL calls back on its device thread.
        unsafe {
            let ud = std::ptr::from_ref(wake).cast_mut().cast::<c_void>();
            if !SDL_SetAudioStreamGetCallback(stream, Some(woken), ud) {
                return Err(sdl_err("the stream's callback"));
            }
            if !SDL_ResumeAudioStreamDevice(stream) {
                return Err(sdl_err("resuming the device"));
            }
        }
        let name = o.name().map_or_else(|| "?".into(), |n| n.to_string_lossy().into_owned());
        eprintln!("audio: {name}, a period of {frames} frames at {freq} Hz");
        Ok(o)
    }

    /// The device's period (frames) and rate.
    fn period(&self) -> Option<(c_int, c_int)> {
        let mut spec = SDL_AudioSpec::default();
        let mut frames = 0;
        // SAFETY: a live device id; both outputs are live locals.
        unsafe { SDL_GetAudioDeviceFormat(self.device, &mut spec, &mut frames) }.then_some((frames, spec.freq))
    }

    /// The name of the device the stream plays on now (SDL's physical device, moved when the default changes).
    fn name(&self) -> Option<CString> {
        // SAFETY: SDL_GetAudioDeviceName returns a string SDL owns or null; it is copied at once.
        let p = unsafe { SDL_GetAudioDeviceName(self.device) };
        (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_owned())
    }

    /// Whether the stream plays on a device SDL still lists (lost devices are left out).
    fn on_live_device(&self) -> bool {
        let mut n = 0;
        // SAFETY: SDL_GetAudioPlaybackDevices returns `n` ids in memory SDL_free releases, or null.
        let live: Vec<CString> = unsafe {
            let ids = SDL_GetAudioPlaybackDevices(&mut n);
            if ids.is_null() {
                return true;
            }
            let names = std::slice::from_raw_parts(ids, usize::try_from(n).unwrap_or(0))
                .iter()
                .filter_map(|&id| {
                    let p = SDL_GetAudioDeviceName(id);
                    (!p.is_null()).then(|| CStr::from_ptr(p).to_owned())
                })
                .collect();
            SDL_free(ids.cast());
            names
        };
        on_live(self.name().as_deref(), &live)
    }
}

/// Whether a stream on the device named `ours` is on a live one, given the names of the devices SDL lists: yes when
/// none is listed (there is nowhere else to go) or one has its name (two devices of one name pass as live).
fn on_live(ours: Option<&CStr>, live: &[CString]) -> bool {
    live.is_empty() || ours.is_some_and(|o| live.iter().any(|l| l.as_c_str() == o))
}

/// Notices when an open stream has gone silent without SDL saying so.
struct Watch {
    /// The last time the stream wanted more.
    taken: Instant,
    /// The last time its device was a live one.
    live: Instant,
}

impl Watch {
    fn new(now: Instant) -> Watch {
        Watch { taken: now, live: now }
    }

    /// Why the stream should be opened again, if it should: `taken` says the device took from it since the last call
    /// (it wanted more), `live` that its device is one SDL lists.
    fn lost(&mut self, taken: bool, live: bool, now: Instant) -> Option<&'static str> {
        if taken {
            self.taken = now;
        }
        if live {
            self.live = now;
        }
        if now.duration_since(self.taken) >= STALL {
            Some("stopped taking audio")
        } else if now.duration_since(self.live) >= STALL {
            Some("was lost and its stream left on it")
        } else {
            None
        }
    }
}

fn run(mut render: impl FnMut(&mut [f32]), channels: usize, stop: &AtomicBool, sdl: Option<Sdl>) {
    // (the get callback's target: declared before any Open, so it outlives every stream)
    let wake = std::thread::current();
    let frame = 4 * channels;
    let mut buf = vec![0.0f32; QUANTUM * channels];
    let mut dev: Option<Open> = None;
    let mut watch = Watch::new(Instant::now());
    let (mut live, mut looked) = (true, Instant::now());
    // without a device: when to try again, whether its failure was said, whether a lost stream is being replaced,
    // and the quanta rendered into nothing since the device went
    let mut retry = Instant::now();
    let (mut said, mut replacing) = (false, false);
    let mut idle: Option<(Instant, u64)> = None;
    while !stop.load(Ordering::Acquire) {
        let now = Instant::now();
        let Some(d) = &mut dev else {
            if sdl.is_some() && now >= retry {
                retry = now + RETRY;
                match Open::new(channels, &wake) {
                    Ok(o) => {
                        if said || replacing {
                            eprintln!("audio: the default device reopened");
                        }
                        (said, replacing) = (false, false);
                        dev = Some(o);
                        idle = None;
                        watch = Watch::new(now);
                        (live, looked) = (true, now);
                        continue;
                    }
                    Err(e) if !said => {
                        eprintln!("audio: {e}");
                        said = true;
                    }
                    Err(_) => {}
                }
            }
            // the mixer goes on at the clock's pace, its output dropped
            let (since, done) = idle.get_or_insert((now, 0));
            let due = (now - *since).as_secs_f64() * f64::from(RATE) / QUANTUM as f64;
            while (*done as f64) < due {
                render(&mut buf);
                *done += 1;
            }
            std::thread::sleep(Duration::from_secs_f64(QUANTUM as f64 / f64::from(RATE)));
            continue;
        };
        // SAFETY: a live stream.
        let queued = unsafe { SDL_GetAudioStreamQueued(d.stream) };
        let mut ok = queued >= 0;
        let mut q = usize::try_from(queued).unwrap_or(0) / frame;
        let taken = ok && q < d.target;
        while ok && q < d.target {
            render(&mut buf);
            if MUTED.load(Ordering::Relaxed) {
                buf.fill(0.0);
            }
            // SAFETY: a live stream; SDL copies the buffer.
            ok = unsafe { SDL_PutAudioStreamData(d.stream, buf.as_ptr().cast(), (buf.len() * 4) as c_int) };
            q += QUANTUM;
        }
        if now >= looked + LOOK {
            looked = now;
            live = d.on_live_device();
            // (a move to a new default can bring a new period)
            if let Some((frames, freq)) = d.period() {
                d.target = target_for(frames, freq);
            }
        }
        let why = if ok { watch.lost(taken, live, now) } else { Some("refused audio") };
        if let Some(why) = why {
            eprintln!("audio: the output device {why}; reopening the default device");
            retry = now.max(d.opened + RETRY);
            replacing = true;
            dev = None;
            continue;
        }
        std::thread::park_timeout(Duration::from_secs_f64(QUANTUM as f64 / f64::from(RATE)));
    }
    drop(dev);
    drop(sdl);
}

#[cfg(test)]
mod tests {
    use std::ffi::{CStr, c_char};
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU64;

    use sdl3_sys::hints::{
        SDL_HINT_AUDIO_DEVICE_SAMPLE_FRAMES, SDL_HINT_AUDIO_DRIVER, SDL_HINT_AUDIO_DUMMY_TIMESCALE, SDL_HINT_OVERRIDE,
        SDL_SetHintWithPriority,
    };

    use super::*;

    /// SDL's hints and its audio subsystem are the process's: one test at a time sets them and runs an output.
    static SDL: Mutex<()> = Mutex::new(());

    /// Sets an SDL hint over the environment (SDL reads its own copy of the environment, taken once).
    fn hint(name: *const c_char, value: &CStr) {
        // SAFETY: both strings are nul-terminated and outlive the call; SDL copies the value.
        assert!(unsafe { SDL_SetHintWithPriority(name, value.as_ptr(), SDL_HINT_OVERRIDE) });
    }

    /// The frames per second an output on stereo pulls from its mixer (a counter in the mixer's place), over 1 s of
    /// wall time once it has settled (the first pass fills the stream's cushion at once).
    fn pull_rate() -> f64 {
        let pulled = Arc::new(AtomicU64::new(0));
        let p = pulled.clone();
        let out = Wasapi::spawn(2, move |b: &mut [f32]| {
            b.fill(0.0);
            p.fetch_add((b.len() / 2) as u64, Ordering::Relaxed);
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let (t0, n0) = (Instant::now(), pulled.load(Ordering::Relaxed));
        std::thread::sleep(Duration::from_secs(1));
        let (t1, n1) = (Instant::now(), pulled.load(Ordering::Relaxed));
        drop(out);
        (n1 - n0) as f64 / (t1 - t0).as_secs_f64()
    }

    /// The dummy driver is a device that takes its period's frames each time its delay has passed: 480 frames each
    /// 20 ms (its 10 ms period at a timescale of 2, whole milliseconds as it counts them) is 24 000 frames a second,
    /// half the audio clock, so an output paced by the clock fails here.
    #[test]
    fn sdl_output_pulls_the_mixer_at_the_device_rate() {
        let _one = SDL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        hint(SDL_HINT_AUDIO_DRIVER, c"dummy");
        hint(SDL_HINT_AUDIO_DEVICE_SAMPLE_FRAMES, c"480");
        hint(SDL_HINT_AUDIO_DUMMY_TIMESCALE, c"2");
        let rate = pull_rate();
        eprintln!("sdl test: the dummy device pulled {rate:.0} frames/s (24000 expected)");
        assert!((rate / 24_000.0 - 1.0).abs() < 0.05, "{rate:.0} frames/s");
    }

    /// A driver that does not exist: SDL's audio does not start, and the mixer still runs at the audio clock's pace.
    #[test]
    fn sdl_output_runs_at_the_audio_clock_without_a_device() {
        let _one = SDL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        hint(SDL_HINT_AUDIO_DRIVER, c"no-such-driver");
        let rate = pull_rate();
        eprintln!("sdl test: with no device the mixer ran at {rate:.0} frames/s ({RATE} expected)");
        assert!((rate / f64::from(RATE) - 1.0).abs() < 0.05, "{rate:.0} frames/s");
    }

    /// A stream the device takes from is kept; one untaken for a second, or left a second on a lost device, is not.
    #[test]
    fn a_stream_is_reopened_after_a_second_untaken_or_lost() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut w = Watch::new(t0);
        for n in (0..3000).step_by(5) {
            assert_eq!(w.lost(true, true, ms(n)), None, "taken at {n} ms");
        }
        // nothing taken from 3000 ms: kept for a second, then reopened
        assert_eq!(w.lost(false, true, ms(3500)), None);
        assert_eq!(w.lost(false, true, ms(3990)), None);
        assert_eq!(w.lost(false, true, ms(4000)), Some("stopped taking audio"));
        // taken but on a lost device from 0 ms
        let mut w = Watch::new(t0);
        assert_eq!(w.lost(true, false, ms(500)), None);
        assert_eq!(w.lost(true, false, ms(999)), None);
        assert_eq!(w.lost(true, false, ms(1000)), Some("was lost and its stream left on it"));
        // a lost spell shorter than a second (a default device changing) is let be
        let mut w = Watch::new(t0);
        assert_eq!(w.lost(true, false, ms(900)), None);
        assert_eq!(w.lost(true, true, ms(950)), None);
        assert_eq!(w.lost(true, false, ms(1900)), None);
    }

    #[test]
    fn a_device_is_live_while_sdl_lists_its_name_or_lists_none() {
        let names = [c"Speakers".to_owned(), c"Headset".to_owned()];
        assert!(on_live(Some(c"Headset"), &names));
        assert!(!on_live(Some(c"USB DAC"), &names));
        assert!(!on_live(None, &names));
        assert!(on_live(Some(c"USB DAC"), &[]));
    }
}
