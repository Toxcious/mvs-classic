//! The Linux window, beside `platform.rs` (selected as `platform` on Linux): an SDL3 window (windowed or borderless
//! full screen) and its event pump, the keyboard, the mouse's buttons, its pointer and wheel and the typed
//! characters handed to the input devices, with the same public items and signatures as the Windows file.

use std::collections::HashSet;
use std::ffi::{CStr, CString, c_int, c_void};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use mvs_d3d::Foundation::HWND;
use mvs_settings::WindowMode;
use mvs_ui::menu::MenuInput;
use sdl3_sys::events::*;
use sdl3_sys::init::{SDL_INIT_VIDEO, SDL_Init};
use sdl3_sys::keyboard::{SDL_GetModState, SDL_StartTextInput};
use sdl3_sys::keycode::*;
use sdl3_sys::messagebox::{SDL_MESSAGEBOX_ERROR, SDL_ShowSimpleMessageBox};
use sdl3_sys::mouse::*;
use sdl3_sys::rect::{SDL_Point, SDL_Rect};
use sdl3_sys::scancode::*;
use sdl3_sys::stdinc::SDL_free;
use sdl3_sys::video::*;

use crate::input::Devices;
use crate::{Error, Result};

pub use crate::platform_common::{FIRST_RUN, content_size, launcher_size};

/// What the event pump saw, as `platform.rs`'s window procedure queues it.
#[derive(Clone, Copy, Debug)]
enum Msg {
    Key(&'static str, bool),
    Mouse(&'static str, bool),
    /// The pointer moved to a point of the client area (pixels), or left it (None).
    Pointer(Option<(f32, f32)>),
    /// The wheel turned, notches (up positive).
    Wheel(f32),
    Char(char),
    Blur,
    Size([u32; 2]),
    Close,
}

/// SDL's last error as the crate's error, naming the call that failed.
fn sdl(what: &str) -> Error {
    // SAFETY: SDL_GetError returns a nul-terminated string SDL owns (empty when none).
    let why = unsafe { CStr::from_ptr(sdl3_sys::error::SDL_GetError()) }.to_string_lossy().into_owned();
    Error::Platform(format!("{what}: {why}"))
}

/// SDL's video, started once for the program's life (on the main thread: the window, its events and the displays
/// are all read there).
fn video() -> Result<()> {
    static STARTED: Mutex<bool> = Mutex::new(false);
    let mut started = STARTED.lock().unwrap_or_else(|e| e.into_inner());
    if !*started {
        // SAFETY: SDL_Init with the video subsystem, from the thread that makes the window.
        if !unsafe { SDL_Init(SDL_INIT_VIDEO) } {
            return Err(sdl("SDL_Init"));
        }
        *started = true;
    }
    Ok(())
}

#[derive(Debug)]
pub struct Window {
    /// The `SDL_Window*`: DXVK-native's SDL3 window system takes it where Windows takes an HWND.
    pub hwnd: HWND,
    /// The client area's size, pixels.
    pub size: [u32; 2],
    win: *mut SDL_Window,
    keys: KeyRecord,
    /// The window lost the keyboard since the last `take_blur`.
    blurred: bool,
    /// The menus' input since the last `take_menu_input`, in the order it came.
    menu_input: Vec<MenuInput>,
    mode: WindowMode,
    /// The windowed client size (display.ts windowedPx): what a placement gave, what a resize while framed made, a
    /// size set while borderless (pendingPx); what borderless returns to.
    windowed: [u32; 2],
    /// The OS pointer's last point in the client area (kept when it leaves): a motion there again is no motion.
    mouse_at: Option<(f32, f32)>,
}

/// The keyboard as the window keeps it between `take_menu_keys` calls (as `platform.rs`'s).
#[derive(Debug, Default)]
struct KeyRecord {
    /// The keys that went down (a screen's menu input), a held key's repeats included.
    menu_keys: Vec<&'static str>,
    /// The keys pressed, no repeat (`pressed`).
    fresh_keys: Vec<&'static str>,
    /// The keys held (a key down again is the menus' repeat).
    held: HashSet<&'static str>,
}

impl KeyRecord {
    /// A key down or up; true for a down of a key held (a repeat).
    fn key(&mut self, k: &'static str, down: bool) -> bool {
        if down && !self.menu_keys.contains(&k) {
            self.menu_keys.push(k);
        }
        let repeat = down && !self.held.insert(k);
        if !down {
            self.held.remove(k);
        }
        if down && !repeat && !self.fresh_keys.contains(&k) {
            self.fresh_keys.push(k);
        }
        repeat
    }

    fn pressed(&self, key: &str) -> bool {
        self.fresh_keys.contains(&key)
    }

    fn take(&mut self) -> Vec<&'static str> {
        self.fresh_keys.clear();
        std::mem::take(&mut self.menu_keys)
    }
}

/// Where a window opens.
#[derive(Clone, Copy, Debug)]
enum Placement {
    /// Framed, its client area this size.
    Windowed([u32; 2]),
    /// Borderless over the primary display.
    FullScreen,
    /// The launcher's: `page` at the display's scale, made smaller to fit its work area down to `least` of that,
    /// centered there; frameless and of a fixed size.
    Launcher { page: [u32; 2], least: f32 },
}

/// The launcher's drag band (its page's title bar): its height and the width kept at its right for the buttons
/// there, client pixels; 0 tall: none (the window's hit test).
static DRAG_BAND: AtomicI32 = AtomicI32::new(0);
static DRAG_RIGHT: AtomicI32 = AtomicI32::new(0);
/// The game's windows open in the background (`set_background`).
static BACKGROUND: AtomicBool = AtomicBool::new(false);

/// A test run's windows count as the foreground one (`foreground`).
// Deviation: Windows also opens them unactivated, off the taskbar and cloaked; SDL has no such window, so a test run
// belongs in a headless compositor (gamescope --backend headless).
pub fn set_background(on: bool) {
    BACKGROUND.store(on, Ordering::Relaxed);
}

/// A display's desktop rectangle and work area in pixels (left, top, right, bottom), from SDL's, which are in the
/// display's points: times the pixel density of its desktop mode.
// Unverified: a scaled display (a Wayland output at 150 %) is not run; X11 and an unscaled output have points that are
// pixels.
fn display_rects(id: SDL_DisplayID) -> Option<([i32; 4], [i32; 4])> {
    let mut b = SDL_Rect::default();
    let mut u = SDL_Rect::default();
    // SAFETY: plain reads of a display SDL listed into locals; the mode SDL returns is its own and read at once.
    let density = unsafe {
        if !SDL_GetDisplayBounds(id, &mut b) || !SDL_GetDisplayUsableBounds(id, &mut u) {
            return None;
        }
        let mode = SDL_GetDesktopDisplayMode(id);
        if mode.is_null() || (*mode).pixel_density <= 0.0 { 1.0 } else { (*mode).pixel_density }
    };
    let px = |v: c_int| (v as f32 * density).round() as i32;
    let r = |r: SDL_Rect| [px(r.x), px(r.y), px(r.x + r.w), px(r.y + r.h)];
    Some((r(b), r(u)))
}

/// The pixel density of the window (its pixels over its points; 1 on X11).
fn density(win: *mut SDL_Window) -> f32 {
    // SAFETY: the window's own pointer.
    let d = unsafe { SDL_GetWindowPixelDensity(win) };
    if d > 0.0 { d } else { 1.0 }
}

/// Pixels to the window's points.
fn points(px: u32, density: f32) -> c_int {
    ((px as f32 / density).round() as c_int).max(1)
}

/// The launcher's drag band, asked by SDL for each press in the window: the title bar drags the window.
unsafe extern "C" fn hit_test(win: *mut SDL_Window, area: *const SDL_Point, _: *mut c_void) -> SDL_HitTestResult {
    let band = DRAG_BAND.load(Ordering::Relaxed);
    if band <= 0 || area.is_null() {
        return SDL_HITTEST_NORMAL;
    }
    let d = density(win);
    // SAFETY: SDL passes the point in the window it asks about.
    let at = unsafe { *area };
    let (x, y) = ((at.x as f32 * d) as i32, (at.y as f32 * d) as i32);
    let (mut w, mut h) = (0, 0);
    // SAFETY: the window SDL asks about, into locals.
    unsafe { SDL_GetWindowSizeInPixels(win, &mut w, &mut h) };
    let right = DRAG_RIGHT.load(Ordering::Relaxed);
    if y >= 0 && y < band && x < w - right { SDL_HITTEST_DRAGGABLE } else { SDL_HITTEST_NORMAL }
}

impl Window {
    /// A window titled `title` with a client area of `size`, or borderless over its monitor (`full_screen`).
    pub fn new(title: &str, size: [u32; 2], full_screen: bool) -> Result<Window> {
        Self::open(title, if full_screen { Placement::FullScreen } else { Placement::Windowed(size) })
    }

    /// The launcher's window (window.ts openLauncher): frameless and of a fixed size, `page` at the display's scale
    /// or smaller to fit, down to `least` of it; centered on the work area of the display under the pointer.
    pub fn launcher(title: &str, page: [u32; 2], least: f32) -> Result<Window> {
        Self::open(title, Placement::Launcher { page, least })
    }

    /// The band at the top of the client area that drags the window, `height` pixels tall less `right` pixels at its
    /// right (the page's buttons there): the frameless launcher's title bar.
    pub fn set_drag_band(&self, height: u32, right: u32) {
        DRAG_BAND.store(i32::try_from(height).unwrap_or(i32::MAX), Ordering::Relaxed);
        DRAG_RIGHT.store(i32::try_from(right).unwrap_or(i32::MAX), Ordering::Relaxed);
    }

    /// The window minimized to the taskbar.
    pub fn minimize(&self) {
        // SAFETY: the window's own pointer.
        unsafe { SDL_MinimizeWindow(self.win) };
    }

    /// Whether the window has the keyboard (a test run's background window counts as it).
    pub fn foreground(&self) -> bool {
        BACKGROUND.load(Ordering::Relaxed) || self.flags().0 & SDL_WINDOW_INPUT_FOCUS.0 != 0
    }

    /// Whether SDL's left button is down with the pointer's focus on this window (the launcher's pointer; Windows'
    /// reads the button while its window is the foreground one). SDL's button state is the pointer's, kept for the
    /// window it is over (or holds, while a button is down), so the mouse's focus is the test.
    pub fn left_down(&self) -> bool {
        // SAFETY: plain reads of SDL's mouse state, updated by the window's pump on this thread.
        unsafe {
            SDL_GetMouseFocus() == self.win
                && SDL_GetMouseState(std::ptr::null_mut(), std::ptr::null_mut()).0 & SDL_BUTTON_LMASK.0 != 0
        }
    }

    /// Whether the window is minimized.
    pub fn minimized(&self) -> bool {
        self.flags().0 & SDL_WINDOW_MINIMIZED.0 != 0
    }

    fn flags(&self) -> SDL_WindowFlags {
        // SAFETY: the window's own pointer.
        unsafe { SDL_GetWindowFlags(self.win) }
    }

    fn open(title: &str, placement: Placement) -> Result<Window> {
        // (DXVK-native reads its window system when the device is made; `main` set it first, so this writes nothing)
        mvs_d3d::set_linux_wsi();
        video()?;
        let title = CString::new(title.replace('\0', "")).unwrap_or_default();
        // (Vulkan: DXVK makes its surface on the window; high pixel density: the back buffer is the client area's
        // pixels on a scaled display too, as Windows' per-monitor DPI awareness gives)
        let mut flags = SDL_WINDOW_VULKAN | SDL_WINDOW_HIGH_PIXEL_DENSITY;
        // SAFETY: plain SDL calls on the main thread with valid arguments; the window is destroyed by `Drop`.
        unsafe {
            let primary = SDL_GetPrimaryDisplay();
            let (size, at) = match placement {
                Placement::Windowed(size) => {
                    flags |= SDL_WINDOW_RESIZABLE;
                    (size, None)
                }
                Placement::FullScreen => {
                    // (with no mode set, full screen is the desktop's: borderless over the display)
                    flags |= SDL_WINDOW_FULLSCREEN;
                    let rect = display_rects(primary).map_or([0, 0, 1280, 720], |(r, _)| r);
                    ([(rect[2] - rect[0]).max(1) as u32, (rect[3] - rect[1]).max(1) as u32], None)
                }
                Placement::Launcher { page, least } => {
                    // (the display under the pointer, its scale and its work area: window.ts openLauncher)
                    // Deviation: Wayland does not tell a program where the pointer is outside its windows: there
                    // the launcher opens on the primary display.
                    let (mut px, mut py) = (0.0, 0.0);
                    SDL_GetGlobalMouseState(&mut px, &mut py);
                    let display = match SDL_GetDisplayForPoint(&SDL_Point { x: px as c_int, y: py as c_int }) {
                        SDL_DisplayID(0) => primary,
                        d => d,
                    };
                    let scale = SDL_GetDisplayContentScale(display);
                    let scale = if scale > 0.0 { scale } else { 1.0 };
                    let mut u = SDL_Rect::default();
                    if !SDL_GetDisplayUsableBounds(display, &mut u) {
                        return Err(sdl("SDL_GetDisplayUsableBounds"));
                    }
                    let mode = SDL_GetDesktopDisplayMode(display);
                    let pd = if mode.is_null() || (*mode).pixel_density <= 0.0 { 1.0 } else { (*mode).pixel_density };
                    let work = [(u.w as f32 * pd).max(1.0) as u32, (u.h as f32 * pd).max(1.0) as u32];
                    let size = launcher_size(page, scale, work, least);
                    flags |= SDL_WINDOW_BORDERLESS;
                    let (w, h) = (points(size[0], pd), points(size[1], pd));
                    // Deviation: Wayland does not let a program place its window: the compositor puts it.
                    (size, Some((u.x + (u.w - w) / 2, u.y + (u.h - h) / 2)))
                }
            };
            let win = SDL_CreateWindow(title.as_ptr(), size[0].max(1) as c_int, size[1].max(1) as c_int, flags);
            if win.is_null() {
                let e = sdl("SDL_CreateWindow");
                // (SDL loads Vulkan for a Vulkan window: when a plain window opens where it did not, the Vulkan
                // driver is what is missing, said in the box as a device that cannot be made is)
                let plain = SDL_CreateWindow(title.as_ptr(), 1, 1, SDL_WINDOW_HIDDEN);
                if !plain.is_null() {
                    SDL_DestroyWindow(plain);
                    vulkan_box(std::ptr::null_mut(), &e);
                }
                return Err(e);
            }
            // (the size asked is in points: a scaled display's window is made again at its pixels' points)
            let d = density(win);
            if d != 1.0 && !matches!(placement, Placement::FullScreen) {
                SDL_SetWindowSize(win, points(size[0], d), points(size[1], d));
            }
            if let Some((x, y)) = at {
                SDL_SetWindowPosition(win, x, y);
            }
            if matches!(placement, Placement::Launcher { .. }) {
                SDL_SetWindowHitTest(win, Some(hit_test), std::ptr::null_mut());
            }
            SDL_SyncWindow(win);
            // (the typed characters: SDL sends them only while text input is on)
            SDL_StartTextInput(win);
            let mut window = Window {
                hwnd: HWND(win.cast()),
                size: [1, 1],
                win,
                keys: KeyRecord::default(),
                blurred: false,
                menu_input: Vec::new(),
                mode: if matches!(placement, Placement::FullScreen) {
                    WindowMode::WindowedFullscreen
                } else {
                    WindowMode::Windowed
                },
                windowed: [1, 1],
                mouse_at: None,
            };
            window.size = window.client_size();
            window.windowed = window.size;
            Ok(window)
        }
    }

    /// The waiting events handled: the keys, the pointer, the wheel and the typed characters into `devices`; false
    /// once the window is closed. `resized`: the new client size when it changed.
    pub fn pump(&mut self, devices: &mut Devices, resized: &mut Option<[u32; 2]>) -> bool {
        let mut open = true;
        for m in self.poll() {
            match m {
                Msg::Close => open = false,
                Msg::Key(k, down) => {
                    devices.key(k, down);
                    // (the menus' own input: a key down while held is a repeat; Deviation: the mouse is player 0's)
                    let repeat = self.keys.key(k, down);
                    self.menu_input.push(MenuInput::Key { key: k, down, repeat, player: 0 });
                }
                Msg::Mouse(k, down) => {
                    devices.mouse(k, down);
                    let button = match k {
                        "LeftMouseButton" => Some(0),
                        "MiddleMouseButton" => Some(1),
                        "RightMouseButton" => Some(2),
                        _ => None,
                    };
                    if let Some(button) = button {
                        self.menu_input.push(MenuInput::MouseButton { button, down });
                    }
                }
                Msg::Pointer(at) => {
                    devices.pointer = at;
                    let Some((x, y)) = at else { continue };
                    // (the first point is no move: no motion is known before it)
                    let Some((fx, fy)) = self.mouse_at.replace((x, y)) else { continue };
                    if (fx, fy) == (x, y) {
                        continue;
                    }
                    if devices.mouse_moved(f64::from((x - fx).hypot(y - fy))) {
                        self.menu_input.push(MenuInput::MouseMove { x: f64::from(x), y: f64::from(y) });
                    }
                }
                Msg::Wheel(n) => {
                    devices.wheel += n;
                    self.menu_input.push(MenuInput::Wheel { notches: f64::from(n) });
                }
                // (the port's own) Ctrl+V (U+0016): the clipboard's text typed into the menus' text box with the
                // keyboard, as the characters it holds (MenuView::pasted)
                Msg::Char('\u{16}') => {
                    devices.chars.push('\u{16}');
                    let text = crate::launcher::setup_os::paste_text(self.hwnd).unwrap_or_default();
                    self.menu_input.extend(mvs_ui::menu::MenuView::pasted(&text).map(MenuInput::Char));
                }
                Msg::Char(c) => {
                    devices.chars.push(c);
                    self.menu_input.push(MenuInput::Char(c));
                }
                Msg::Blur => {
                    self.keys.held.clear();
                    self.blurred = true;
                    devices.blur();
                }
                Msg::Size(s) => {
                    if s != self.size && s[0] > 0 && s[1] > 0 {
                        self.size = s;
                        *resized = Some(s);
                    }
                    // (display.ts settle's 'resized': a framed window's new size is what borderless returns to; a
                    // maximized one keeps the size it had)
                    if self.mode == WindowMode::Windowed
                        && s[0] > 0
                        && s[1] > 0
                        && self.flags().0 & SDL_WINDOW_MAXIMIZED.0 == 0
                    {
                        self.windowed = s;
                    }
                }
            }
        }
        open
    }

    /// SDL's waiting events as the messages `pump` handles.
    fn poll(&self) -> Vec<Msg> {
        let mut out = Vec::new();
        let d = density(self.win);
        let mut ev = SDL_Event::default();
        // SAFETY: the event loop on the thread that made the window; each union field read is the one its type names,
        // and a text event's string is SDL's, read before the next poll.
        unsafe {
            while SDL_PollEvent(&mut ev) {
                let t = SDL_EventType(ev.r#type);
                if t == SDL_EVENT_QUIT || t == SDL_EVENT_WINDOW_CLOSE_REQUESTED {
                    out.push(Msg::Close);
                } else if t == SDL_EVENT_KEY_DOWN || t == SDL_EVENT_KEY_UP {
                    let k = ev.key;
                    let numlock = k.r#mod.0 & SDL_KMOD_NUM.0 != 0;
                    let Some(name) = sdl_key_name(k.scancode, k.key, numlock) else { continue };
                    out.push(Msg::Key(name, k.down));
                    if k.down {
                        out.extend(control_char(name, ctrl_only(k.r#mod)).map(Msg::Char));
                    }
                } else if t == SDL_EVENT_TEXT_INPUT {
                    // (Ctrl with a key types its control character above, as Windows' WM_CHAR does, not the letter)
                    if ctrl_only(SDL_GetModState()) || ev.text.text.is_null() {
                        continue;
                    }
                    out.extend(CStr::from_ptr(ev.text.text).to_string_lossy().chars().map(Msg::Char));
                } else if t == SDL_EVENT_MOUSE_BUTTON_DOWN || t == SDL_EVENT_MOUSE_BUTTON_UP {
                    let name = match i32::from(ev.button.button) {
                        SDL_BUTTON_LEFT => "LeftMouseButton",
                        SDL_BUTTON_MIDDLE => "MiddleMouseButton",
                        SDL_BUTTON_RIGHT => "RightMouseButton",
                        SDL_BUTTON_X1 => "ThumbMouseButton",
                        SDL_BUTTON_X2 => "ThumbMouseButton2",
                        _ => continue,
                    };
                    out.push(Msg::Mouse(name, ev.button.down));
                } else if t == SDL_EVENT_MOUSE_MOTION {
                    // (SDL's points to the client area's pixels)
                    out.push(Msg::Pointer(Some((ev.motion.x * d, ev.motion.y * d))));
                } else if t == SDL_EVENT_WINDOW_MOUSE_LEAVE {
                    out.push(Msg::Pointer(None));
                } else if t == SDL_EVENT_MOUSE_WHEEL {
                    let w = ev.wheel;
                    let flip = if w.direction == SDL_MOUSEWHEEL_FLIPPED { -1.0 } else { 1.0 };
                    if w.y != 0.0 {
                        out.push(Msg::Wheel(w.y * flip));
                    }
                } else if t == SDL_EVENT_WINDOW_FOCUS_LOST {
                    out.push(Msg::Blur);
                } else if t == SDL_EVENT_WINDOW_PIXEL_SIZE_CHANGED {
                    let w = ev.window;
                    out.push(Msg::Size([w.data1.max(0) as u32, w.data2.max(0) as u32]));
                }
            }
        }
        out
    }

    /// `key` was pressed since the last `take_menu_keys` (a fresh press: a held key's repeats are not).
    pub fn pressed(&self, key: &str) -> bool {
        self.keys.pressed(key)
    }

    /// `key` is held now.
    pub fn held(&self, key: &str) -> bool {
        self.keys.held.contains(key)
    }

    /// Hide the OS cursor over the window, or show it again.
    pub fn set_cursor_hidden(&mut self, hidden: bool) {
        // SAFETY: plain cursor calls on the main thread.
        unsafe {
            if hidden {
                SDL_HideCursor();
            } else {
                SDL_ShowCursor();
            }
        }
    }

    /// APlayerController::SetMouseLocation for the game's cursor (as `platform.rs`'s): the OS pointer moved to `at`
    /// (client pixels, floor(x + 0.5)), and the window's last pointer point with it, so the move it makes is no
    /// mouse motion. Only while the window has the keyboard. The client point the pointer went to.
    // Deviation: Wayland lets a program move the pointer only where the compositor offers the pointer-warp protocol;
    // elsewhere the OS pointer stays where it is (the game's own cursor still moves).
    pub fn warp_pointer(&mut self, at: [f64; 2]) -> Option<[f64; 2]> {
        let [w, h] = self.client_size();
        if w == 0 || h == 0 || self.flags().0 & SDL_WINDOW_INPUT_FOCUS.0 == 0 {
            return None;
        }
        let x = (at[0] + 0.5).floor().clamp(0.0, f64::from(w - 1)) as i32;
        let y = (at[1] + 0.5).floor().clamp(0.0, f64::from(h - 1)) as i32;
        let d = density(self.win);
        // SAFETY: the window's own pointer.
        unsafe { SDL_WarpMouseInWindow(self.win, x as f32 / d, y as f32 / d) };
        self.mouse_at = Some((x as f32, y as f32));
        Some([f64::from(x), f64::from(y)])
    }

    /// The keys that went down since the last call.
    pub fn take_menu_keys(&mut self) -> Vec<&'static str> {
        self.keys.take()
    }

    /// The menus' input since the last `take_menu_input`, to read.
    pub fn menu_input(&self) -> &[MenuInput] {
        &self.menu_input
    }

    /// The menus' input since the last call, in the order it came.
    pub fn take_menu_input(&mut self) -> Vec<MenuInput> {
        std::mem::take(&mut self.menu_input)
    }

    /// Whether the window lost the keyboard since the last call.
    pub fn take_blur(&mut self) -> bool {
        std::mem::take(&mut self.blurred)
    }

    /// The window's title (the perf line when the UI is off).
    pub fn set_title(&self, title: &str) {
        let title = CString::new(title.replace('\0', "")).unwrap_or_default();
        // SAFETY: the window's own pointer and a nul-terminated string alive through the call.
        unsafe { SDL_SetWindowTitle(self.win, title.as_ptr()) };
    }

    /// The window's mode (set by `set_window_mode`, or its creation's).
    pub fn mode(&self) -> WindowMode {
        self.mode
    }

    /// The window's size as the Graphics page reads it (display.ts stateOf): its client size now while framed, the
    /// size it returns to while borderless.
    pub fn windowed_size(&self) -> [u32; 2] {
        match self.mode {
            WindowMode::Windowed => self.client_size(),
            WindowMode::WindowedFullscreen => self.windowed,
        }
    }

    /// The client area's size now, pixels; the last known one when SDL gives none.
    fn client_size(&self) -> [u32; 2] {
        let (mut w, mut h) = (0, 0);
        // SAFETY: the window's own pointer, into locals.
        if unsafe { SDL_GetWindowSizeInPixels(self.win, &mut w, &mut h) } && w > 0 && h > 0 {
            [w as u32, h as u32]
        } else {
            self.size
        }
    }

    /// The display the window is mostly on, as a value that changes when the window moves to another.
    pub fn monitor_id(&self) -> isize {
        // SAFETY: the window's own pointer.
        unsafe { SDL_GetDisplayForWindow(self.win) }.0 as isize
    }

    /// The refresh rate in Hz of the display the window is mostly on; 0 when unknown.
    pub fn refresh_hz(&self) -> f64 {
        // SAFETY: the window's own pointer; the mode SDL returns is its own and read at once.
        unsafe {
            let mode = SDL_GetCurrentDisplayMode(SDL_GetDisplayForWindow(self.win));
            if mode.is_null() { 0.0 } else { f64::from((*mode).refresh_rate) }
        }
    }

    /// The index in `displays` of the display the window is mostly on (0 when it matches none).
    pub fn display_index(&self, displays: &[Display]) -> usize {
        // SAFETY: the window's own pointer.
        let id = unsafe { SDL_GetDisplayForWindow(self.win) };
        let Some((rect, _)) = display_rects(id) else { return 0 };
        displays.iter().position(|d| d.rect == rect).unwrap_or(0)
    }

    /// The window put in `mode` on `display` (None: the one it is on): framed at `size` (client pixels; None: its own
    /// windowed size), centered in the display's work area and kept inside it, or borderless over the whole display,
    /// where `size` is the one it returns to (app/src/display.ts: setMode, setSize, setDisplay, place, contentDip).
    /// The swap chain follows the size event it sends.
    // Deviation: no exclusive full screen (EWindowMode Fullscreen): borderless covers the display, as the TS port did.
    // Deviation: Wayland does not let a program place its window: a framed window is not centered (the compositor
    // places it), and borderless goes to the display the compositor gives.
    pub fn set_window_mode(
        &mut self,
        mode: WindowMode,
        size: Option<[u32; 2]>,
        display: Option<&Display>,
    ) -> Result<()> {
        let all = refresh_displays();
        let target = match display {
            Some(d) => d.clone(),
            None => all
                .get(self.display_index(&all))
                .or(all.first())
                .cloned()
                .ok_or_else(|| Error::Platform("no display".to_owned()))?,
        };
        let [wl, wt, wr, wb] = target.work;
        let work = [(wr - wl).max(1) as u32, (wb - wt).max(1) as u32];
        let id = display_id(&target);
        let d = density(self.win);
        // SAFETY: SDL window calls on this window's pointer, from its own thread.
        unsafe {
            match mode {
                WindowMode::WindowedFullscreen => {
                    if self.mode == WindowMode::Windowed {
                        self.windowed = self.client_size();
                    }
                    // (a size set while borderless: display.ts pendingPx, and settle's contentDip without a frame)
                    if let Some(s) = size {
                        self.windowed = content_size(s, target.size(), work, [0, 0]);
                    }
                    if let Some(id) = id {
                        let at = SDL_WINDOWPOS_CENTERED_DISPLAY(id);
                        SDL_SetWindowPosition(self.win, at, at);
                    }
                    // (no mode: the desktop's, borderless over the display)
                    SDL_SetWindowFullscreenMode(self.win, std::ptr::null());
                    if !SDL_SetWindowFullscreen(self.win, true) {
                        return Err(sdl("SDL_SetWindowFullscreen"));
                    }
                    SDL_SyncWindow(self.win);
                }
                WindowMode::Windowed => {
                    if !SDL_SetWindowFullscreen(self.win, false) {
                        return Err(sdl("SDL_SetWindowFullscreen"));
                    }
                    if self.flags().0 & SDL_WINDOW_MAXIMIZED.0 != 0 {
                        SDL_RestoreWindow(self.win);
                    }
                    SDL_SetWindowBordered(self.win, true);
                    SDL_SyncWindow(self.win);
                    let (mut top, mut left, mut bottom, mut right) = (0, 0, 0, 0);
                    SDL_GetWindowBordersSize(self.win, &mut top, &mut left, &mut bottom, &mut right);
                    let px = |v: c_int| (v.max(0) as f32 * d).round() as u32;
                    let frame = [px(left) + px(right), px(top) + px(bottom)];
                    // (no size asked: the one it has while framed, else the one it returns to: pendingPx ?? windowedPx)
                    let own = if self.mode == WindowMode::Windowed { self.client_size() } else { self.windowed };
                    let c = content_size(size.unwrap_or(own), target.size(), work, frame);
                    SDL_SetWindowSize(self.win, points(c[0], d), points(c[1], d));
                    let (w, h) = ((c[0] + frame[0]) as i32, (c[1] + frame[1]) as i32);
                    let (x, y) = (wl + (wr - wl - w).max(0) / 2, wt + (wb - wt - h).max(0) / 2);
                    // (SDL places the client area: its corner inside the frame, in points)
                    let pt = |v: i32| (v as f32 / d).round() as c_int;
                    SDL_SetWindowPosition(self.win, pt(x + px(left) as i32), pt(y + px(top) as i32));
                    SDL_SyncWindow(self.win);
                    // (display.ts place: windowedPx is the size the window took, not the one asked)
                    self.windowed = self.client_size();
                }
            }
        }
        self.mode = mode;
        Ok(())
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: the window this made.
        unsafe { SDL_DestroyWindow(self.win) };
    }
}

/// The graphics device could not be made (no Vulkan driver DXVK can use, or a broken one): a message box says so,
/// over `window`; the error then ends the program as on Windows, where it is only printed.
pub fn no_device(window: &Window, error: &dyn std::fmt::Display) {
    vulkan_box(window.win, error);
}

/// The box that names Vulkan and DXVK, over `parent` (null: none), with `error`.
fn vulkan_box(parent: *mut SDL_Window, error: &dyn std::fmt::Display) {
    let text =
        CString::new(mvs_ui::launcher::words::no_vulkan(&error.to_string()).replace('\0', "")).unwrap_or_default();
    eprintln!("mvs: no graphics device ({error}): showing the message box");
    // SAFETY: a modal box over the window, its strings alive through the call.
    let shown =
        unsafe { SDL_ShowSimpleMessageBox(SDL_MESSAGEBOX_ERROR, c"MVS Classic".as_ptr(), text.as_ptr(), parent) };
    eprintln!("mvs: the message box closed (shown: {shown})");
}

/// Whether Ctrl is held without Alt (AltGr, Ctrl+Alt on Windows, types the layout's characters).
fn ctrl_only(m: SDL_Keymod) -> bool {
    m.0 & SDL_KMOD_CTRL.0 != 0 && m.0 & SDL_KMOD_ALT.0 == 0
}

/// The character Windows' TranslateMessage makes of a key down that types none of its own (WM_CHAR): Backspace
/// (U+0008, the chat box's delete), Tab, Enter, Escape, and a letter with Ctrl alone its control code (Ctrl+V is
/// U+0016, the menus' paste).
fn control_char(name: &str, ctrl: bool) -> Option<char> {
    match name {
        "BackSpace" => Some('\u{8}'),
        "Tab" => Some('\t'),
        "Enter" => Some('\r'),
        "Escape" => Some('\u{1b}'),
        _ if ctrl => {
            let [l] = name.as_bytes() else { return None };
            l.is_ascii_uppercase().then(|| char::from(l - b'A' + 1))
        }
        _ => None,
    }
}

/// The left- and right-hand modifiers by scancode: the key's position, as Windows' `key_name` tells the sides apart
/// by the make code (Shift) and the E0 prefix (Ctrl, Alt).
const SIDES: [(SDL_Scancode, &str); 6] = [
    (SDL_SCANCODE_LSHIFT, "LeftShift"),
    (SDL_SCANCODE_RSHIFT, "RightShift"),
    (SDL_SCANCODE_LCTRL, "LeftControl"),
    (SDL_SCANCODE_RCTRL, "RightControl"),
    (SDL_SCANCODE_LALT, "LeftAlt"),
    (SDL_SCANCODE_RALT, "RightAlt"),
];

/// The keys named by keycode (the layout's key, as Windows' `key_name` names them by the virtual key), besides the
/// letters, the digits and the function keys.
const BY_KEYCODE: [(SDL_Keycode, &str); 52] = [
    (SDLK_BACKSPACE, "BackSpace"),
    (SDLK_TAB, "Tab"),
    (SDLK_RETURN, "Enter"),
    (SDLK_KP_ENTER, "Enter"),
    (SDLK_LSHIFT, "LeftShift"),
    (SDLK_RSHIFT, "RightShift"),
    (SDLK_LCTRL, "LeftControl"),
    (SDLK_RCTRL, "RightControl"),
    (SDLK_LALT, "LeftAlt"),
    (SDLK_RALT, "RightAlt"),
    (SDLK_PAUSE, "Pause"),
    (SDLK_CAPSLOCK, "CapsLock"),
    (SDLK_ESCAPE, "Escape"),
    (SDLK_SPACE, "SpaceBar"),
    (SDLK_PAGEUP, "PageUp"),
    (SDLK_PAGEDOWN, "PageDown"),
    (SDLK_END, "End"),
    (SDLK_HOME, "Home"),
    (SDLK_LEFT, "Left"),
    (SDLK_UP, "Up"),
    (SDLK_RIGHT, "Right"),
    (SDLK_DOWN, "Down"),
    (SDLK_INSERT, "Insert"),
    (SDLK_DELETE, "Delete"),
    (SDLK_KP_MULTIPLY, "Multiply"),
    (SDLK_KP_PLUS, "Add"),
    (SDLK_KP_MINUS, "Subtract"),
    (SDLK_KP_PERIOD, "Decimal"),
    (SDLK_KP_DIVIDE, "Divide"),
    (SDLK_NUMLOCKCLEAR, "NumLock"),
    (SDLK_SCROLLLOCK, "ScrollLock"),
    (SDLK_SEMICOLON, "Semicolon"),
    (SDLK_EQUALS, "Equals"),
    (SDLK_COMMA, "Comma"),
    (SDLK_MINUS, "Hyphen"),
    (SDLK_PERIOD, "Period"),
    (SDLK_SLASH, "Slash"),
    (SDLK_GRAVE, "Tilde"),
    (SDLK_LEFTBRACKET, "LeftBracket"),
    (SDLK_BACKSLASH, "Backslash"),
    (SDLK_RIGHTBRACKET, "RightBracket"),
    (SDLK_APOSTROPHE, "Apostrophe"),
    (SDLK_KP_0, "NumPadZero"),
    (SDLK_KP_1, "NumPadOne"),
    (SDLK_KP_2, "NumPadTwo"),
    (SDLK_KP_3, "NumPadThree"),
    (SDLK_KP_4, "NumPadFour"),
    (SDLK_KP_5, "NumPadFive"),
    (SDLK_KP_6, "NumPadSix"),
    (SDLK_KP_7, "NumPadSeven"),
    (SDLK_KP_8, "NumPadEight"),
    (SDLK_KP_9, "NumPadNine"),
];

/// The keypad's keys while NumLock is off: the edit block's keys, as Windows' virtual keys for them are then (the
/// keypad's 5 gives VK_CLEAR, which has no name).
// Unverified: the keypad's virtual keys under NumLock are Windows' keyboard driver's, not on disk; SDL keeps its
// keypad keycodes whatever NumLock is (SDL_HINT_KEYCODE_OPTIONS without hide_numpad), so the switch is made here.
const KEYPAD_OFF: [(SDL_Keycode, Option<&str>); 11] = [
    (SDLK_KP_0, Some("Insert")),
    (SDLK_KP_1, Some("End")),
    (SDLK_KP_2, Some("Down")),
    (SDLK_KP_3, Some("PageDown")),
    (SDLK_KP_4, Some("Left")),
    (SDLK_KP_5, None),
    (SDLK_KP_6, Some("Right")),
    (SDLK_KP_7, Some("Home")),
    (SDLK_KP_8, Some("Up")),
    (SDLK_KP_9, Some("PageUp")),
    (SDLK_KP_PERIOD, Some("Delete")),
];

/// The FKey name of an SDL key event, by the rule of Windows' `key_name` (input/keys.rs), which names a key from its
/// virtual key (the layout's key: letters, digits, punctuation, the keypad, the function and edit keys) and tells
/// the left and right modifiers apart by the key's position (the make code for Shift, the E0 prefix for Ctrl and
/// Alt). Here the keycode is the virtual key (SDL's default keycode options give the digits on a French number row
/// and the Latin letters on a Cyrillic layout, as Windows' virtual keys do) and the scancode the position.
// Unverified: a punctuation key whose character has no US key (the German ß) has no name here; Windows names it
// by the VK_OEM code its layout gives it.
fn sdl_key_name(scancode: SDL_Scancode, keycode: SDL_Keycode, numlock: bool) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V",
        "W", "X", "Y", "Z",
    ];
    const DIGITS: [&str; 10] = ["Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine"];
    const F: [&str; 24] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15", "F16", "F17",
        "F18", "F19", "F20", "F21", "F22", "F23", "F24",
    ];
    if let Some((_, n)) = SIDES.iter().find(|(s, _)| *s == scancode) {
        return Some(n);
    }
    if !numlock && let Some((_, n)) = KEYPAD_OFF.iter().find(|(k, _)| *k == keycode) {
        return *n;
    }
    let k = keycode.0;
    let at = |first: SDL_Keycode, last: SDL_Keycode| (first.0..=last.0).contains(&k).then(|| (k - first.0) as usize);
    if let Some(i) = at(SDLK_A, SDLK_Z) {
        return Some(LETTERS[i]);
    }
    if let Some(i) = at(SDLK_0, SDLK_9) {
        return Some(DIGITS[i]);
    }
    if let Some(i) = at(SDLK_F1, SDLK_F12) {
        return Some(F[i]);
    }
    if let Some(i) = at(SDLK_F13, SDLK_F24) {
        return Some(F[12 + i]);
    }
    BY_KEYCODE.iter().find(|(c, _)| *c == keycode).map(|(_, n)| *n)
}

/// A display: its system name (the window settings' `display`), its desktop rectangle and its work area (the
/// desktop less the panels) in physical pixels (left, top, right, bottom), its refresh rate in Hz (0: unknown),
/// whether it is the primary.
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    pub name: String,
    pub rect: [i32; 4],
    pub work: [i32; 4],
    pub refresh_hz: f64,
    pub primary: bool,
}

impl Display {
    /// The display's size, pixels.
    pub fn size(&self) -> [u32; 2] {
        [(self.rect[2] - self.rect[0]).max(1) as u32, (self.rect[3] - self.rect[1]).max(1) as u32]
    }
}

/// The SDL display whose desktop rectangle is `d`'s.
fn display_id(d: &Display) -> Option<SDL_DisplayID> {
    each_display(|id| (display_rects(id).is_some_and(|(r, _)| r == d.rect)).then_some(id)).into_iter().next()
}

/// `f` over SDL's displays, what it gives kept.
fn each_display<T>(mut f: impl FnMut(SDL_DisplayID) -> Option<T>) -> Vec<T> {
    if video().is_err() {
        return Vec::new();
    }
    let mut n = 0;
    // SAFETY: SDL's list of `n` display ids, freed with SDL_free after the reads.
    unsafe {
        let ids = SDL_GetDisplays(&mut n);
        if ids.is_null() {
            return Vec::new();
        }
        let out = std::slice::from_raw_parts(ids, n.max(0) as usize).iter().filter_map(|id| f(*id)).collect();
        SDL_free(ids.cast());
        out
    }
}

/// The displays as last listed: listed on the first call, again by `refresh_displays`.
static DISPLAYS: Mutex<Option<Arc<Vec<Display>>>> = Mutex::new(None);

/// The displays, the primary first, as last listed.
pub fn displays() -> Arc<Vec<Display>> {
    let mut cache = DISPLAYS.lock().unwrap_or_else(|e| e.into_inner());
    cache.get_or_insert_with(|| Arc::new(list_displays())).clone()
}

/// The displays listed again (a window or settings change, the Graphics page opening).
pub fn refresh_displays() -> Arc<Vec<Display>> {
    let list = Arc::new(list_displays());
    *DISPLAYS.lock().unwrap_or_else(|e| e.into_inner()) = Some(list.clone());
    list
}

/// The displays, the primary first (app/src/display.ts stateOf: `screen.getAllDisplays`).
fn list_displays() -> Vec<Display> {
    // SAFETY: a plain read.
    let primary = unsafe { SDL_GetPrimaryDisplay() };
    let mut list = each_display(|id| {
        let (rect, work) = display_rects(id)?;
        // SAFETY: a display SDL listed; its name and mode are SDL's own and read at once.
        let (name, hz) = unsafe {
            let name = SDL_GetDisplayName(id);
            let name = if name.is_null() { String::new() } else { CStr::from_ptr(name).to_string_lossy().into_owned() };
            let mode = SDL_GetCurrentDisplayMode(id);
            (name, if mode.is_null() { 0.0 } else { f64::from((*mode).refresh_rate) })
        };
        Some(Display { name, rect, work, refresh_hz: hz, primary: id == primary })
    });
    // (a stable order: the primary first, the rest as SDL lists them)
    list.sort_by_key(|d| !d.primary);
    list
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::input::keys::key_name;

    #[test]
    fn key_names_cover_every_name_the_input_layer_reads() {
        // (the names Windows' raw input hands the input layer: key_name's whole range, so every name the input
        // layer, the key bindings and the menus can look for)
        let mut windows = BTreeSet::new();
        for vk in 0..=0xffu16 {
            for scan in [0, 0x36] {
                for e0 in [false, true] {
                    windows.extend(key_name(vk, scan, e0));
                }
            }
        }
        assert!(windows.len() > 100, "{windows:?}");
        let mut linux = BTreeSet::new();
        let keycodes = (0..0x80u32).chain((0..512).map(|s| SDLK_SCANCODE_MASK.0 | s));
        for kc in keycodes {
            for sc in 0..512 {
                for numlock in [false, true] {
                    linux.extend(sdl_key_name(SDL_Scancode(sc), SDL_Keycode(kc), numlock));
                }
            }
        }
        let missing: Vec<_> = windows.difference(&linux).collect();
        assert!(missing.is_empty(), "no Linux key gives {missing:?}");
    }

    #[test]
    fn a_french_layout_a_key_is_named_as_on_windows() {
        // AZERTY: the key at QWERTY's Q types 'a'. Windows' raw input gives it the virtual key VK_A (0x41) with the
        // Q key's make code (0x10), and key_name names it from the virtual key: A. SDL gives it the scancode Q and
        // the keycode 'a'.
        let name = |sc, kc| sdl_key_name(sc, kc, true);
        assert_eq!(key_name(0x41, 0x10, false), Some("A"));
        assert_eq!(name(SDL_SCANCODE_Q, SDLK_A), Some("A"));
        assert_eq!(key_name(0x51, 0x1e, false), Some("Q"));
        assert_eq!(name(SDL_SCANCODE_A, SDLK_Q), Some("Q"));
        assert_eq!(name(SDL_SCANCODE_W, SDLK_Z), key_name(0x5a, 0x11, false));
        assert_eq!(name(SDL_SCANCODE_Z, SDLK_W), key_name(0x57, 0x2c, false));
        // (M sits at QWERTY's semicolon; the number row gives the digits' keys, SDL's french_numbers)
        assert_eq!(name(SDL_SCANCODE_SEMICOLON, SDLK_M), key_name(0x4d, 0x27, false));
        assert_eq!(name(SDL_SCANCODE_1, SDLK_1), key_name(0x31, 0x02, false));
        // (AltGr: the right Alt, from the key's position as Windows' E0 flag gives it, whatever its keycode)
        assert_eq!(name(SDL_SCANCODE_RALT, SDLK_MODE), key_name(0x12, 0x38, true));
        assert_eq!(name(SDL_SCANCODE_RSHIFT, SDLK_RSHIFT), key_name(0x10, 0x36, false));
    }

    #[test]
    fn content_size_is_shared() {
        // (platform.rs's a_saved_size_is_kept_inside_its_display, on the Linux window's)
        let (display, work) = ([1920, 1080], [1920, 1040]);
        assert_eq!(content_size([0, 0], display, work, [0, 0]), FIRST_RUN);
        assert_eq!(content_size([2560, 1440], display, work, [0, 0]), FIRST_RUN);
        assert_eq!(content_size([1600, 900], display, work, [16, 39]), [1600, 900]);
        assert_eq!(content_size([1920, 1080], display, work, [16, 39]), [1904, 1001]);
        assert_eq!(launcher_size([1200, 720], 1.5, [1920, 1040], 0.6), [1733, 1040]);
    }
}
