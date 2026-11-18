//! The WASAPI output: the default render device in shared mode, event-driven (IAudioClient3 at its smallest
//! period, else IAudioClient with AUTOCONVERTPCM), f32 at the device's channels, on a thread of its own under MMCSS
//! "Pro Audio", following device changes (IMMNotificationClient).
//!
//! The thread owns the mixer: each time the device asks, it fills the device's buffer from the mixer's quanta (the
//! rest of a quantum waits for the next call). When the default device changes or the device goes away, it opens the
//! new default and goes on; with none, it retries every second and meanwhile renders the mixer into nothing at the
//! audio clock's pace (so stops and scope releases still take effect).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, PROPERTYKEY};
use windows::Win32::Media::Audio::{
    AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, DEVICE_STATE, EDataFlow, ERole,
    IAudioClient, IAudioClient3, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, WAVEFORMATEXTENSIBLE_0,
    eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW, WaitForSingleObject,
};
use windows::core::{GUID, PCWSTR, implement, w};

use crate::mix::Mixer;
use crate::{AudioError, QUANTUM, RATE};

/// The running output: its thread, which owns the mixer.
pub struct Wasapi {
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
    pub(crate) stop: Arc<AtomicBool>,
}

const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xfffe;
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID = GUID::from_u128(0x0000_0003_0000_0010_8000_00aa_0038_9b71);
/// The fallback stream's buffer, 100 ns units (20 ms).
const FALLBACK_BUFFER: i64 = 200_000;
/// How often a missing device is looked for again.
const RETRY: Duration = Duration::from_secs(1);

/// The device gets silence: the mixer runs and renders as ever, its output is zeroed before the device takes it.
static MUTED: AtomicBool = AtomicBool::new(false);

/// Mutes or unmutes every output of the process (a scripted or test run's sound mixed but not heard).
pub fn set_muted(on: bool) {
    MUTED.store(on, Ordering::Relaxed);
}

fn err(what: &str, e: windows::core::Error) -> AudioError {
    AudioError::Output(format!("{what}: {e}"))
}

/// COM on the calling thread for its scope.
struct Com;

impl Com {
    fn init() -> Result<Com, AudioError> {
        // Safety: paired with CoUninitialize in drop, on this thread.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok().map_err(|e| err("CoInitializeEx", e))?;
        Ok(Com)
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        // Safety: this thread's CoInitializeEx succeeded.
        unsafe { CoUninitialize() };
    }
}

fn enumerator() -> Result<IMMDeviceEnumerator, AudioError> {
    // Safety: COM is initialised on this thread.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(|e| err("MMDeviceEnumerator", e))
}

fn default_device(en: &IMMDeviceEnumerator) -> Result<IMMDevice, AudioError> {
    // Safety: a COM call on a live enumerator.
    unsafe { en.GetDefaultAudioEndpoint(eRender, eConsole) }.map_err(|e| err("default render device", e))
}

/// The device's mix format: channels, rate, whether it is 32-bit float, and the format itself when it is the
/// extensible kind.
fn mix_format(client: &IAudioClient) -> Result<(usize, u32, bool, Option<WAVEFORMATEXTENSIBLE>), AudioError> {
    // Safety: GetMixFormat returns a CoTaskMemAlloc'd WAVEFORMATEX (EXTENSIBLE when its tag says so), read here and
    // freed once.
    unsafe {
        let p = client.GetMixFormat().map_err(|e| err("GetMixFormat", e))?;
        let f = p.read_unaligned();
        let (float, ext) = match f.wFormatTag {
            WAVE_FORMAT_IEEE_FLOAT => (f.wBitsPerSample == 32, None),
            WAVE_FORMAT_EXTENSIBLE => {
                let x = (p as *const WAVEFORMATEXTENSIBLE).read_unaligned();
                let sub = x.SubFormat;
                (sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT && f.wBitsPerSample == 32, Some(x))
            }
            _ => (false, None),
        };
        CoTaskMemFree(Some(p as *const _));
        Ok((usize::from(f.nChannels), f.nSamplesPerSec, float, ext))
    }
}

/// The mixer's format: f32 at the mixer's rate on `channels`.
fn our_format(channels: usize) -> WAVEFORMATEXTENSIBLE {
    let ch = channels as u16;
    // the first `channels` speakers of the standard order
    let mask = if channels >= 32 { u32::MAX } else { (1u32 << channels) - 1 };
    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_EXTENSIBLE,
            nChannels: ch,
            nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * u32::from(ch) * 4,
            nBlockAlign: ch * 4,
            wBitsPerSample: 32,
            cbSize: 22,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 { wValidBitsPerSample: 32 },
        dwChannelMask: mask,
        SubFormat: KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
    }
}

impl Wasapi {
    /// Opens the default device and starts the mixer on the output thread.
    pub fn start(mixer: Box<Mixer>) -> Result<Wasapi, AudioError> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let s = stop.clone();
        let thread = std::thread::Builder::new()
            .name("audio-out".into())
            .spawn(move || run(mixer, &s, ready_tx))
            .map_err(|e| AudioError::Output(format!("the output thread: {e}")))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Wasapi { thread: Some(thread), stop }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err(AudioError::Output("the output thread ended".into())),
        }
    }

    /// The device's output channels (what the mixer is made for), before the mixer is. Asked on a thread of its
    /// own, so the caller's COM apartment is left as it is.
    pub fn device_channels() -> Result<usize, AudioError> {
        let ask = || -> Result<usize, AudioError> {
            let _com = Com::init()?;
            let dev = default_device(&enumerator()?)?;
            // Safety: a COM call on a live device.
            let client: IAudioClient = unsafe { dev.Activate(CLSCTX_ALL, None) }.map_err(|e| err("IAudioClient", e))?;
            Ok(mix_format(&client)?.0)
        };
        std::thread::Builder::new()
            .name("audio-query".into())
            .spawn(ask)
            .map_err(|e| AudioError::Output(format!("the query thread: {e}")))?
            .join()
            .map_err(|_| AudioError::Output("the query thread panicked".into()))?
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

/// Sets a flag when the default render device changes.
#[implement(IMMNotificationClient)]
struct Notify {
    changed: Arc<AtomicBool>,
}

impl IMMNotificationClient_Impl for Notify_Impl {
    fn OnDeviceStateChanged(&self, _id: &PCWSTR, _state: DEVICE_STATE) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDeviceAdded(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _id: &PCWSTR) -> windows::core::Result<()> {
        if flow == eRender && role == eConsole {
            self.changed.store(true, Ordering::Release);
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _id: &PCWSTR, _key: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

/// An open stream on a device.
struct Open {
    client: IAudioClient,
    render: IAudioRenderClient,
    event: HANDLE,
    frames: u32,
}

impl Drop for Open {
    fn drop(&mut self) {
        // Safety: the client was started by this stream and the event is its own.
        unsafe {
            let _ = self.client.Stop();
            let _ = CloseHandle(self.event);
        }
    }
}

/// Opens the default device for the mixer's `channels`: IAudioClient3 at its smallest period when the device mixes
/// f32 at the mixer's rate and channels, else IAudioClient converting the mixer's format.
fn open(en: &IMMDeviceEnumerator, channels: usize) -> Result<Open, AudioError> {
    let dev = default_device(en)?;
    // Safety: CreateEventW's handle is owned by Open, or closed here when the stream does not open.
    let event = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }.map_err(|e| err("CreateEventW", e))?;
    open_with(&dev, channels, event).inspect_err(|_| {
        // Safety: the event made above, not handed to an Open.
        unsafe {
            let _ = CloseHandle(event);
        }
    })
}

fn open_with(dev: &IMMDevice, channels: usize, event: HANDLE) -> Result<Open, AudioError> {
    // Safety: COM calls on live objects; the formats passed outlive the calls.
    unsafe {
        let fmt = our_format(channels);
        let pfmt = &fmt as *const WAVEFORMATEXTENSIBLE as *const WAVEFORMATEX;
        let mut client: Option<IAudioClient> = None;
        if let Ok(c3) = dev.Activate::<IAudioClient3>(CLSCTX_ALL, None) {
            let base: IAudioClient = windows::core::Interface::cast(&c3).map_err(|e| err("IAudioClient", e))?;
            // (the engine period is only offered at the device's own format: its speaker mask included)
            if let (dch, RATE, true, Some(dev_fmt)) = mix_format(&base)?
                && dch == channels
            {
                let dfmt = &dev_fmt as *const WAVEFORMATEXTENSIBLE as *const WAVEFORMATEX;
                let (mut def, mut fun, mut min, mut max) = (0, 0, 0, 0);
                if c3.GetSharedModeEnginePeriod(dfmt, &mut def, &mut fun, &mut min, &mut max).is_ok()
                    && c3.InitializeSharedAudioStream(AUDCLNT_STREAMFLAGS_EVENTCALLBACK, min, dfmt, None).is_ok()
                {
                    client = Some(base);
                }
            }
        }
        let client = match client {
            Some(c) => c,
            None => {
                let c: IAudioClient = dev.Activate(CLSCTX_ALL, None).map_err(|e| err("IAudioClient", e))?;
                let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                    | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
                c.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, FALLBACK_BUFFER, 0, pfmt, None)
                    .map_err(|e| err("IAudioClient::Initialize", e))?;
                c
            }
        };
        client.SetEventHandle(event).map_err(|e| err("SetEventHandle", e))?;
        let frames = client.GetBufferSize().map_err(|e| err("GetBufferSize", e))?;
        let render: IAudioRenderClient = client.GetService().map_err(|e| err("IAudioRenderClient", e))?;
        client.Start().map_err(|e| err("IAudioClient::Start", e))?;
        Ok(Open { client, render, event, frames })
    }
}

/// The mixer's quanta handed out in the device's sizes.
struct Feed {
    buf: Vec<f32>,
    at: usize,
}

impl Feed {
    fn fill(&mut self, mixer: &mut Mixer, out: &mut [f32]) {
        let mut done = 0;
        while done < out.len() {
            if self.at == self.buf.len() {
                mixer.render(&mut self.buf);
                self.at = 0;
            }
            let n = (self.buf.len() - self.at).min(out.len() - done);
            out[done..done + n].copy_from_slice(&self.buf[self.at..self.at + n]);
            self.at += n;
            done += n;
        }
    }
}

fn run(mut mixer: Box<Mixer>, stop: &AtomicBool, ready: std::sync::mpsc::Sender<Result<(), AudioError>>) {
    let com = match Com::init() {
        Ok(c) => c,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let mut task = 0u32;
    // Safety: the task handle is reverted on this thread before it ends.
    let mmcss = unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task) }.ok();
    let setup = || -> Result<(IMMDeviceEnumerator, IMMNotificationClient, Arc<AtomicBool>), AudioError> {
        let en = enumerator()?;
        let changed = Arc::new(AtomicBool::new(false));
        let notify: IMMNotificationClient = Notify { changed: changed.clone() }.into();
        // Safety: the client stays registered until unregistered below, while `notify` lives.
        unsafe { en.RegisterEndpointNotificationCallback(&notify) }.map_err(|e| err("device notifications", e))?;
        Ok((en, notify, changed))
    };
    let channels = mixer.channels;
    let (en, notify, changed) = match setup() {
        Ok(s) => s,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let mut dev = open(&en, channels).inspect_err(|e| eprintln!("audio: {e}")).ok();
    let _ = ready.send(Ok(()));
    let mut feed = Feed { buf: vec![0.0; QUANTUM * channels], at: QUANTUM * channels };
    // without a device: when to try again, and the quanta rendered into nothing since the device went
    let mut retry = Instant::now() + RETRY;
    let mut idle: Option<(Instant, u64)> = None;
    while !stop.load(Ordering::Acquire) {
        if changed.swap(false, Ordering::AcqRel) {
            dev = None;
        }
        let Some(d) = &dev else {
            let now = Instant::now();
            if now >= retry {
                retry = now + RETRY;
                match open(&en, channels) {
                    Ok(o) => {
                        dev = Some(o);
                        idle = None;
                        continue;
                    }
                    Err(e) => eprintln!("audio: {e}"),
                }
            }
            // the mixer goes on at the clock's pace, its output dropped
            let (since, done) = idle.get_or_insert((now, 0));
            let due = (now - *since).as_secs_f64() * f64::from(RATE) / QUANTUM as f64;
            while (*done as f64) < due {
                mixer.render(&mut feed.buf);
                *done += 1;
            }
            feed.at = feed.buf.len();
            std::thread::sleep(Duration::from_secs_f64(QUANTUM as f64 / f64::from(RATE)));
            continue;
        };
        // Safety: COM calls on the open stream; the device buffer is written within the frames GetBuffer gave.
        let r = unsafe {
            WaitForSingleObject(d.event, 200);
            (|| -> windows::core::Result<()> {
                let pad = d.client.GetCurrentPadding()?;
                let n = d.frames.saturating_sub(pad);
                if n == 0 {
                    return Ok(());
                }
                let p = d.render.GetBuffer(n)?;
                let out = std::slice::from_raw_parts_mut(p.cast::<f32>(), n as usize * channels);
                feed.fill(&mut mixer, out);
                if MUTED.load(Ordering::Relaxed) {
                    out.fill(0.0);
                }
                d.render.ReleaseBuffer(n, 0)
            })()
        };
        if let Err(e) = r {
            if e.code() != AUDCLNT_E_DEVICE_INVALIDATED {
                eprintln!("audio: output: {e}");
            }
            dev = None;
            retry = Instant::now();
        }
    }
    drop(dev);
    // Safety: registered above with this enumerator.
    unsafe {
        let _ = en.UnregisterEndpointNotificationCallback(&notify);
        if let Some(h) = mmcss {
            let _ = AvRevertMmThreadCharacteristics(h);
        }
    }
    // (COM goes last)
    drop((en, notify, com));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opens the default device the way the output does and plays 200 ms of silence. A machine without a render
    /// device says so and passes.
    #[test]
    fn plays_silence() {
        let Ok(channels) = Wasapi::device_channels() else {
            eprintln!("wasapi test: no render device, skipped");
            return;
        };
        let _com = Com::init().unwrap();
        let en = enumerator().unwrap();
        let d = open(&en, channels).unwrap();
        let mut written = 0u64;
        let t = std::time::Instant::now();
        while t.elapsed() < std::time::Duration::from_millis(200) {
            // Safety: COM calls on the open stream; the buffer is written within the frames GetBuffer gave.
            unsafe {
                WaitForSingleObject(d.event, 200);
                let pad = d.client.GetCurrentPadding().unwrap();
                let n = d.frames.saturating_sub(pad);
                if n == 0 {
                    continue;
                }
                let p = d.render.GetBuffer(n).unwrap();
                std::slice::from_raw_parts_mut(p.cast::<f32>(), n as usize * channels).fill(0.0);
                d.render.ReleaseBuffer(n, 0).unwrap();
                written += u64::from(n);
            }
        }
        eprintln!("wasapi test: {channels} channels, buffer {} frames, {written} frames in 200 ms", d.frames);
        assert!(written > u64::from(RATE) / 10);
    }
}
