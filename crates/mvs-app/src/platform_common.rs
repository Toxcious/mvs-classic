//! The window's sizes, the same on every platform: the game window's client size inside its display, the launcher
//! window's size at its monitor's scale. Shared by the Windows window (`platform.rs`) and the Linux one
//! (`platform_sdl.rs`).

/// The window's first-run client size (display.ts FIRST_RUN): a saved size of 0 or larger than its display.
pub const FIRST_RUN: [u32; 2] = [1280, 720];

/// The client size for an asked one on a display of `display` pixels whose work area is `work` (display.ts
/// contentDip): 0 or larger than the display gives the first-run size; then kept inside the work area less `frame`
/// (the window's border and title; 0 while it covers its display).
pub fn content_size(asked: [u32; 2], display: [u32; 2], work: [u32; 2], frame: [u32; 2]) -> [u32; 2] {
    let fits = asked[0] > 0 && asked[1] > 0 && asked[0] <= display[0] && asked[1] <= display[1];
    let want = if fits { asked } else { FIRST_RUN };
    [0, 1].map(|i| want[i].min(work[i].saturating_sub(frame[i])).max(1))
}

/// The launcher window's client size, pixels (window.ts openLauncher's zoom, the page's size times it): the page at
/// the monitor's `scale` (its DPI over 96), smaller to fit the work area of `work` pixels, no smaller than `least`.
// Deviation: a fixed size (window.ts let the player drag it at the page's aspect ratio and kept the zoom in the
// settings: Settings.launcherZoom); its first zoom is the saved one's default, 1.
pub fn launcher_size(page: [u32; 2], scale: f32, work: [u32; 2], least: f32) -> [u32; 2] {
    let [pw, ph] = [page[0] as f32 * scale, page[1] as f32 * scale];
    let fits = (work[0] as f32 / pw).min(work[1] as f32 / ph);
    let zoom = fits.min(1.0).max(least);
    [(pw * zoom).round() as u32, (ph * zoom).round() as u32]
}
