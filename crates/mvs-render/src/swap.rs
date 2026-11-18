//! The window's swap chain: flip discard, R8G8B8A8_UNORM (the post chain's output format, so post writes the back
//! buffer directly), two buffers, a frame latency waitable object at latency 1, and tearing allowed when the factory
//! reports it (the present waits on vsync unless the frame rate limit is above the refresh or unlimited; the limit holds
//! back the start of the next frame, before its input is read).

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::Threading::WaitForSingleObjectEx;
use windows::core::{BOOL, Interface};

use crate::Result;
use crate::gpu::{Gpu, TexDesc, TexKind, TexViews, Texture, dx};

const BUFFERS: u32 = 2;
const FORMAT: DXGI_FORMAT = DXGI_FORMAT_R8G8B8A8_UNORM;

#[derive(Debug)]
pub struct SwapChain {
    /// The back buffer's size, pixels.
    pub size: [u32; 2],
    swap: IDXGISwapChain2,
    /// The frame latency waitable object.
    latency: HANDLE,
    /// DXGI_PRESENT_ALLOW_TEARING is passed with a present without vsync (the factory's
    /// DXGI_FEATURE_PRESENT_ALLOW_TEARING).
    tearing: bool,
    back: Texture,
    /// The frame rate limit (the Graphics screen's Max Framerate): the most presents a second, 0: none.
    limit: u32,
    /// Presents asked with vsync wait on the display's refresh (off for a limit above the refresh, or none at all).
    vsync: bool,
    /// When the next frame is due to start under the limit.
    due: Option<Instant>,
    /// A present since the last wait (the latency object is signalled once a frame per present: a frame that
    /// presented nothing, a screen's last before the next one's, has nothing to wait for).
    presented: bool,
    /// The last present was paced: the next wait holds the frame's start to the limit.
    pace_next: bool,
}

/// The swap chain's flags: the waitable object, and tearing when allowed (ResizeBuffers takes the same).
fn flags(tearing: bool) -> u32 {
    let mut f = DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32;
    if tearing {
        f |= DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING.0 as u32;
    }
    f
}

/// Buffer 0 of `swap` as a render target of `size`.
fn back_buffer(gpu: &Gpu, swap: &IDXGISwapChain2, size: [u32; 2]) -> Result<Texture> {
    // SAFETY: buffer 0 of a live swap chain on this device; the view is made on it with its own format.
    unsafe {
        let tex: ID3D11Texture2D = swap.GetBuffer(0).map_err(dx("IDXGISwapChain GetBuffer"))?;
        let mut rtv = None;
        gpu.dev.CreateRenderTargetView(&tex, None, Some(&mut rtv)).map_err(dx("CreateRenderTargetView back buffer"))?;
        Ok(Texture {
            res: tex.cast().map_err(dx("back buffer as a resource"))?,
            srv: None,
            rtv,
            dsv: None,
            uav: None,
            desc: TexDesc {
                kind: TexKind::D2,
                size: [size[0], size[1], 1],
                mips: 1,
                format: FORMAT,
                views: TexViews { rtv: true, ..Default::default() },
            },
        })
    }
}

impl SwapChain {
    /// The swap chain of `hwnd` at `size`, with FRAME_LATENCY_WAITABLE_OBJECT (SetMaximumFrameLatency 1) and
    /// ALLOW_TEARING when the factory reports it.
    pub fn new(gpu: &Gpu, hwnd: HWND, size: [u32; 2]) -> Result<SwapChain> {
        // SAFETY: DXGI calls on the device's own adapter and factory with valid descriptors and out-pointers;
        // `hwnd` is the caller's live window.
        unsafe {
            let dxgi: IDXGIDevice = gpu.dev.cast().map_err(dx("ID3D11Device as IDXGIDevice"))?;
            let adapter = dxgi.GetAdapter().map_err(dx("IDXGIDevice GetAdapter"))?;
            let factory: IDXGIFactory2 = adapter.GetParent().map_err(dx("IDXGIAdapter GetParent"))?;
            let mut tearing = BOOL(0);
            let allowed = factory.cast::<IDXGIFactory5>().is_ok_and(|f5| {
                f5.CheckFeatureSupport(
                    DXGI_FEATURE_PRESENT_ALLOW_TEARING,
                    (&mut tearing as *mut BOOL).cast(),
                    std::mem::size_of::<BOOL>() as u32,
                )
                .is_ok()
            }) && tearing.as_bool();
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: size[0],
                Height: size[1],
                Format: FORMAT,
                Stereo: false.into(),
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: BUFFERS,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                Flags: flags(allowed),
            };
            let swap1 = factory
                .CreateSwapChainForHwnd(&gpu.dev, hwnd, &desc, None, None)
                .map_err(dx("CreateSwapChainForHwnd"))?;
            // (the window handles Alt+Enter itself: borderless full screen, never DXGI's exclusive mode)
            factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER).map_err(dx("MakeWindowAssociation"))?;
            let swap: IDXGISwapChain2 = swap1.cast().map_err(dx("IDXGISwapChain2"))?;
            swap.SetMaximumFrameLatency(1).map_err(dx("SetMaximumFrameLatency"))?;
            let latency = swap.GetFrameLatencyWaitableObject();
            let back = back_buffer(gpu, &swap, size)?;
            Ok(SwapChain {
                size,
                swap,
                latency,
                tearing: allowed,
                back,
                limit: 0,
                vsync: true,
                due: None,
                presented: true,
                pace_next: false,
            })
        }
    }

    /// The frame's start: waits on the frame latency object, at most `timeout_ms`, when a frame was presented since the
    /// last wait (with none the object stays unsignalled until the timeout), then, when that present was paced, until
    /// the frame is due under the frame rate limit. The frame's input is read after this, so the limit's wait never
    /// sits between the input and its picture.
    pub fn wait(&mut self, timeout_ms: u32) {
        if !std::mem::replace(&mut self.presented, false) {
            return;
        }
        // SAFETY: the swap chain's own waitable object, open until drop.
        unsafe {
            WaitForSingleObjectEx(self.latency, timeout_ms, true);
        }
        if self.pace_next {
            self.pace();
        }
    }

    /// The back buffer: its RTV set, `desc.size` the chain's size.
    pub fn back_buffer(&self) -> &Texture {
        &self.back
    }

    /// The buffers resized (ResizeBuffers; the back buffer's views made again).
    pub fn resize(&mut self, gpu: &Gpu, size: [u32; 2]) -> Result<()> {
        let size = size.map(|v| v.max(1));
        if size == self.size {
            return Ok(());
        }
        // Every reference to the old buffers goes before ResizeBuffers, as DXGI requires: the back buffer is
        // swapped for a 1 x 1 stand-in and the context's targets unbound.
        let stand_in = TexDesc {
            kind: TexKind::D2,
            size: [1, 1, 1],
            mips: 1,
            format: FORMAT,
            views: TexViews { rtv: true, ..Default::default() },
        };
        drop(std::mem::replace(&mut self.back, gpu.texture(&stand_in, &[])?));
        // SAFETY: unbinds the targets and submits the context's work on the old buffers.
        unsafe {
            gpu.ctx.OMSetRenderTargets(None, None);
            gpu.ctx.Flush();
        }
        // SAFETY: no reference to the old buffers is left.
        unsafe {
            self.swap
                .ResizeBuffers(BUFFERS, size[0], size[1], FORMAT, DXGI_SWAP_CHAIN_FLAG(flags(self.tearing) as i32))
                .map_err(dx("ResizeBuffers"))?;
        }
        self.back = back_buffer(gpu, &self.swap, size)?;
        self.size = size;
        Ok(())
    }

    /// The most presents a second (0: no limit), and whether they wait on the display's refresh (`vsync`: false for a
    /// limit above the refresh or no limit at all, so the frames are not held to it). Applies to presents asked with
    /// vsync; one asked without (a held clock's run) is neither paced nor synced.
    pub fn set_frame_limit(&mut self, fps: u32, vsync: bool) {
        self.limit = fps;
        self.vsync = vsync;
        self.due = None;
    }

    /// Waits until the next frame is due under the frame rate limit, and sets the one after.
    fn pace(&mut self) {
        if self.limit == 0 {
            return;
        }
        let interval = 1.0 / f64::from(self.limit);
        let now = Instant::now();
        let start = *self.due.get_or_insert(now);
        // (signed: an early present is a negative lateness, the wait it owes)
        let late = if now >= start { (now - start).as_secs_f64() } else { -(start - now).as_secs_f64() };
        let (wait, next) = pace_step(late, interval);
        if wait > 0.0 {
            // (sleep to a millisecond short of the time, then spin: Windows' sleeps round up to its timer tick)
            let until = Instant::now() + Duration::from_secs_f64(wait);
            if wait > 0.002 {
                std::thread::sleep(Duration::from_secs_f64(wait - 0.002));
            }
            while Instant::now() < until {
                std::hint::spin_loop();
            }
        }
        self.due = Some(start + Duration::from_secs_f64(next));
    }

    /// Present: with a sync interval of 1 when `paced` and the limit syncs, else 0 with ALLOW_TEARING when the factory
    /// allows it. A `paced` present has the next frame's start held to the frame rate limit (`wait`).
    pub fn present(&mut self, paced: bool) -> Result<()> {
        self.pace_next = paced;
        let (interval, flags) = if paced && self.vsync {
            (1, DXGI_PRESENT(0))
        } else if self.tearing {
            (0, DXGI_PRESENT_ALLOW_TEARING)
        } else {
            (0, DXGI_PRESENT(0))
        };
        self.presented = true;
        // SAFETY: a present of this swap chain from the thread that owns its device context.
        unsafe { self.swap.Present(interval, flags).ok().map_err(dx("IDXGISwapChain Present")) }
    }
}

impl Drop for SwapChain {
    fn drop(&mut self) {
        // SAFETY: the waitable object GetFrameLatencyWaitableObject handed over, closed once.
        unsafe {
            let _ = CloseHandle(self.latency);
        }
    }
}

/// One step of the frame rate limit: `late` is how far past its due time the present is (negative: early), `interval`
/// the limit's period; the wait before presenting and the next due time, both from the due time. A present more than
/// an interval late owes nothing (the next is due an interval from now), as a long frame does not owe the frames it
/// missed.
// Deviation: the limit paces the present (a wait before it, the display's vsync then holds it to a
// refresh): a present on a display's refresh grid takes the first refresh at or after the due time.
fn pace_step(late: f64, interval: f64) -> (f64, f64) {
    if late < 0.0 {
        return (-late, interval);
    }
    if late > interval { (0.0, late + interval) } else { (0.0, interval) }
}

#[cfg(test)]
mod tests {
    use super::pace_step;

    #[test]
    fn limit_waits_until_due_and_forgets_a_long_frame() {
        let i = 1.0 / 60.0;
        // early by 5 ms: wait 5 ms, the next one an interval after the due time
        assert_eq!(pace_step(-0.005, i), (0.005, i));
        // a little late: no wait, the next is still due on the grid
        assert_eq!(pace_step(0.004, i), (0.0, i));
        // two intervals late: nothing owed, the next an interval from now
        let (w, next) = pace_step(2.0 * i, i);
        assert_eq!(w, 0.0);
        assert!((next - 3.0 * i).abs() < 1e-12);
    }
}
