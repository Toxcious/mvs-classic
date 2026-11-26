//! The window: a Win32 window (windowed or borderless full screen) and its message pump, raw keyboard input
//! (WM_INPUT), the mouse's buttons, its pointer and wheel and the typed characters handed to the input devices, and
//! the HID devices' arrivals (WM_DEVICECHANGE) that send the pads looking for Sony pads again.

use std::cell::RefCell;
use std::collections::HashSet;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use windows::Win32::Devices::HumanInterfaceDevice::HidD_GetHidGuid;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAK, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, HDC,
    HMONITOR, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MONITORINFOEXW, MonitorFromPoint,
    MonitorFromWindow, ScreenToClient,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICE_FLAGS, RAWINPUTHEADER, RID_INPUT,
    RIM_TYPEKEYBOARD, RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HSTRING, PCWSTR, w};

use mvs_settings::WindowMode;
use mvs_ui::menu::MenuInput;

use crate::input::Devices;
use crate::input::keys::key_name;
use crate::{Error, Result};

pub use crate::platform_common::{FIRST_RUN, content_size, launcher_size};

/// RAWKEYBOARD.Flags: RI_KEY_BREAK (the key went up) and RI_KEY_E0 (the E0 prefix).
const RI_KEY_BREAK: u16 = 1;
const RI_KEY_E0: u16 = 2;
/// The OS cursor is not shown over the window (WM_SETCURSOR).
static CURSOR_HIDDEN: AtomicBool = AtomicBool::new(false);

/// MONITORINFO.dwFlags: the primary monitor.
const MONITORINFOF_PRIMARY: u32 = 1;
/// WM_MOUSELEAVE (in the windows crate's Controls module, a feature this crate does not take).
const WM_MOUSELEAVE: u32 = 0x02a3;

/// What the window procedure saw, drained by `pump`.
#[derive(Clone, Copy, Debug)]
enum Msg {
    Key(&'static str, bool),
    Mouse(&'static str, bool),
    /// The pointer moved to a point of the client area (pixels), or left it (None).
    Pointer(Option<(f32, f32)>),
    /// The wheel turned, notches (WHEEL_DELTA units over 120; up positive).
    Wheel(f32),
    Char(char),
    Blur,
    Size([u32; 2]),
    DeviceArrived,
    Close,
}

thread_local! {
    static QUEUE: RefCell<Vec<Msg>> = const { RefCell::new(Vec::new()) };
    /// A WM_CHAR's high surrogate waiting for its low one.
    static SURROGATE: RefCell<Option<u16>> = const { RefCell::new(None) };
    /// TrackMouseEvent asked for the pointer's WM_MOUSELEAVE (asked again at the next move after it came).
    static TRACKING: RefCell<bool> = const { RefCell::new(false) };
}

fn push(m: Msg) {
    QUEUE.with(|q| q.borrow_mut().push(m));
}

fn os(what: &str) -> impl FnOnce(windows::core::Error) -> Error + '_ {
    move |source| Error::Os { what: what.to_owned(), source }
}

#[derive(Debug)]
pub struct Window {
    pub hwnd: HWND,
    /// The client area's size, pixels.
    pub size: [u32; 2],
    keys: KeyRecord,
    /// The window lost the keyboard since the last `take_blur`.
    blurred: bool,
    /// The menus' input since the last `take_menu_input`, in the order it came.
    menu_input: Vec<MenuInput>,
    mode: WindowMode,
    /// The windowed client size (display.ts windowedPx): what a placement gave, what a resize while framed made, a
    /// size set while borderless (pendingPx); what borderless returns to.
    windowed: [u32; 2],
    /// The OS pointer's last point in the client area (kept when it leaves): a WM_MOUSEMOVE there again is no motion.
    mouse_at: Option<(f32, f32)>,
}

/// The keyboard as the window keeps it between `take_menu_keys` calls.
#[derive(Debug, Default)]
struct KeyRecord {
    /// The keys that went down (a screen's menu input), a held key's repeats included.
    menu_keys: Vec<&'static str>,
    /// The keys pressed, no repeat (`pressed`: a keydown with !e.repeat, as the pause's and the Lab's Escape and Tab
    /// read it: lab.ts 441, pause.ts 224).
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
    /// Borderless over the primary monitor.
    FullScreen,
    /// The launcher's: `page` (its layout's size, in the monitor's 96-DPI units) at the monitor's scale, made smaller
    /// to fit its work area down to `least` of that, centred there; frameless and of a fixed size.
    Launcher { page: [u32; 2], least: f32 },
}

/// The launcher's drag band (its page's title bar): its height and the width kept at its right for the buttons
/// there, client pixels; 0 tall: none (WM_NCHITTEST).
static DRAG_BAND: AtomicI32 = AtomicI32::new(0);
static DRAG_RIGHT: AtomicI32 = AtomicI32::new(0);
/// The game's windows open in the background (`set_background`).
static BACKGROUND: AtomicBool = AtomicBool::new(false);

/// A test run's windows: never activated, off the taskbar and cloaked (DWM draws nothing of them; the swap chain
/// presents and the frames read back as ever), placed on the third display (else the last other than the primary)
/// should the cloak not hold. The launcher's window is always shown.
pub fn set_background(on: bool) {
    BACKGROUND.store(on, Ordering::Relaxed);
}

/// The display a background window sits on: the third, else the last that is not the primary, else the primary.
fn background_display() -> Option<Display> {
    let all = refresh_displays();
    all.get(2).or_else(|| all.iter().rev().find(|d| !d.primary)).or(all.first()).cloned()
}

impl Window {
    /// A window titled `title` with a client area of `size`, or borderless over its monitor (`full_screen`).
    pub fn new(title: &str, size: [u32; 2], full_screen: bool) -> Result<Window> {
        Self::open(title, if full_screen { Placement::FullScreen } else { Placement::Windowed(size) })
    }

    /// The launcher's window (window.ts openLauncher): frameless and of a fixed size, `page` at the monitor's scale
    /// or smaller to fit, down to `least` of it; centred on the work area of the monitor under the pointer.
    pub fn launcher(title: &str, page: [u32; 2], least: f32) -> Result<Window> {
        Self::open(title, Placement::Launcher { page, least })
    }

    /// The band at the top of the client area that drags the window, `height` pixels tall less `right` pixels at its
    /// right (the page's buttons there): the frameless launcher's title bar (styles.css .titlebar's app-region).
    pub fn set_drag_band(&self, height: u32, right: u32) {
        DRAG_BAND.store(i32::try_from(height).unwrap_or(i32::MAX), Ordering::Relaxed);
        DRAG_RIGHT.store(i32::try_from(right).unwrap_or(i32::MAX), Ordering::Relaxed);
    }

    /// The window minimised to the taskbar.
    pub fn minimize(&self) {
        // SAFETY: the window's own handle.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
        }
    }

    /// Whether the window is the foreground one (a test run's background window counts as it: it is never activated).
    pub fn foreground(&self) -> bool {
        // SAFETY: a plain read of the foreground window.
        BACKGROUND.load(Ordering::Relaxed) || unsafe { GetForegroundWindow() } == self.hwnd
    }

    /// Whether the window is minimised.
    pub fn minimized(&self) -> bool {
        // SAFETY: the window's own handle.
        unsafe { IsIconic(self.hwnd) }.as_bool()
    }

    fn open(title: &str, placement: Placement) -> Result<Window> {
        // SAFETY: plain Win32 calls with valid arguments; the class's procedure is `proc` below.
        unsafe {
            // (pixels are pixels: the back buffer is the client area's size on every monitor's scale)
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = GetModuleHandleW(None).map_err(os("GetModuleHandleW"))?;
            let class = w!("MvsClassic");
            // (the exe's icon group, id 1 of build.rs's resources, at the system's two icon sizes; none: the default)
            let icon = |cx, cy| {
                LoadImageW(
                    Some(instance.into()),
                    PCWSTR(std::ptr::without_provenance(1)),
                    IMAGE_ICON,
                    GetSystemMetrics(cx),
                    GetSystemMetrics(cy),
                    LR_SHARED,
                )
                .map(|h| HICON(h.0))
                .unwrap_or_default()
            };
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).map_err(os("LoadCursorW"))?,
                hIcon: icon(SM_CXICON, SM_CYICON),
                hIconSm: icon(SM_CXSMICON, SM_CYSMICON),
                lpszClassName: class,
                ..Default::default()
            };
            if RegisterClassExW(&wc) == 0 {
                return Err(os("RegisterClassExW")(windows::core::Error::from_thread()));
            }
            let background = BACKGROUND.load(Ordering::Relaxed) && !matches!(placement, Placement::Launcher { .. });
            let away = if background { background_display() } else { None };
            let (style, (x, y, w, h)) = match placement {
                Placement::FullScreen if away.is_some() => {
                    let [l, t, r, b] = away.as_ref().map_or([0; 4], |d| d.rect);
                    (WS_POPUP | WS_VISIBLE, (l, t, r - l, b - t))
                }
                Placement::FullScreen => {
                    let m = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
                    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
                    if !GetMonitorInfoW(m, &mut mi).as_bool() {
                        return Err(os("GetMonitorInfoW")(windows::core::Error::from_thread()));
                    }
                    let r = mi.rcMonitor;
                    (WS_POPUP | WS_VISIBLE, (r.left, r.top, r.right - r.left, r.bottom - r.top))
                }
                Placement::Windowed(size) => {
                    let style = WS_OVERLAPPEDWINDOW | WS_VISIBLE;
                    let mut r = RECT { left: 0, top: 0, right: size[0] as i32, bottom: size[1] as i32 };
                    AdjustWindowRectEx(&mut r, style, false, WINDOW_EX_STYLE::default())
                        .map_err(os("AdjustWindowRectEx"))?;
                    let (x, y) = away.as_ref().map_or((CW_USEDEFAULT, CW_USEDEFAULT), |d| (d.work[0], d.work[1]));
                    (style, (x, y, r.right - r.left, r.bottom - r.top))
                }
                Placement::Launcher { page, least } => {
                    // (the monitor under the pointer, its scale and its work area: window.ts openLauncher)
                    let mut at = POINT::default();
                    let _ = GetCursorPos(&mut at);
                    let m = MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST);
                    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
                    if !GetMonitorInfoW(m, &mut mi).as_bool() {
                        return Err(os("GetMonitorInfoW")(windows::core::Error::from_thread()));
                    }
                    let (mut dx, mut dy) = (96, 96);
                    let _ = GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
                    let r = mi.rcWork;
                    let work = [(r.right - r.left).max(1) as u32, (r.bottom - r.top).max(1) as u32];
                    let [w, h] = launcher_size(page, dx as f32 / 96.0, work, least);
                    let (w, h) = (w as i32, h as i32);
                    // (frameless: the page draws its title bar; a minimise box for the taskbar's click)
                    let style = WS_POPUP | WS_VISIBLE | WS_SYSMENU | WS_MINIMIZEBOX;
                    (style, (r.left + (r.right - r.left - w) / 2, r.top + (r.bottom - r.top - h) / 2, w, h))
                }
            };
            let full_screen = matches!(placement, Placement::FullScreen);
            // (a background window is made hidden, cloaked, then shown without taking the foreground)
            let (ex, style) = if background {
                (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW, style & !WS_VISIBLE)
            } else {
                (WINDOW_EX_STYLE::default(), style)
            };
            let hwnd = CreateWindowExW(
                ex,
                class,
                &HSTRING::from(title),
                style,
                x,
                y,
                w,
                h,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .map_err(os("CreateWindowExW"))?;
            if background {
                let cloak = BOOL(1);
                let _ = DwmSetWindowAttribute(hwnd, DWMWA_CLOAK, (&raw const cloak).cast(), size_of::<BOOL>() as u32);
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
            let keyboard =
                RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x06, dwFlags: RAWINPUTDEVICE_FLAGS(0), hwndTarget: hwnd };
            RegisterRawInputDevices(&[keyboard], size_of::<RAWINPUTDEVICE>() as u32)
                .map_err(os("RegisterRawInputDevices"))?;
            // (the HID interfaces' arrivals: a Sony pad plugged in or paired; kept for the window's life)
            let filter = DEV_BROADCAST_DEVICEINTERFACE_W {
                dbcc_size: size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32,
                dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE.0,
                dbcc_classguid: HidD_GetHidGuid(),
                ..Default::default()
            };
            RegisterDeviceNotificationW(
                HANDLE(hwnd.0),
                (&filter as *const DEV_BROADCAST_DEVICEINTERFACE_W).cast(),
                DEVICE_NOTIFY_WINDOW_HANDLE,
            )
            .map_err(os("RegisterDeviceNotificationW"))?;
            let mut r = RECT::default();
            GetClientRect(hwnd, &mut r).map_err(os("GetClientRect"))?;
            let size = [(r.right - r.left).max(1) as u32, (r.bottom - r.top).max(1) as u32];
            let mode = if full_screen { WindowMode::WindowedFullscreen } else { WindowMode::Windowed };
            Ok(Window {
                hwnd,
                size,
                keys: KeyRecord::default(),
                blurred: false,
                menu_input: Vec::new(),
                mode,
                windowed: size,
                mouse_at: None,
            })
        }
    }

    /// The waiting messages handled: raw input, the pointer, the wheel and the typed characters into `devices`;
    /// false once the window is closed (WM_CLOSE). `resized`: the new client size when it changed.
    pub fn pump(&mut self, devices: &mut Devices, resized: &mut Option<[u32; 2]>) -> bool {
        let mut msg = MSG::default();
        // SAFETY: the standard message loop on the thread that made the window.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let mut open = true;
        for m in QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut())) {
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
                    if down {
                        self.keys.held.insert(k);
                    } else {
                        self.keys.held.remove(k);
                    }
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
                    // (the page's mousemove comes only when the mouse moves: Windows also sends WM_MOUSEMOVE where
                    // the pointer stands, at the window's creation, its activation and a cursor shown or hidden.
                    // The first point is not a move either: no motion is known before it.)
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
                // (the port's own) Ctrl+V (WM_CHAR 0x16): the clipboard's text typed into the menus' text box with
                // the keyboard, as the characters it holds (MenuView::pasted)
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
                Msg::DeviceArrived => devices.pads_mut().device_arrived(),
                Msg::Size(s) => {
                    if s != self.size && s[0] > 0 && s[1] > 0 {
                        self.size = s;
                        *resized = Some(s);
                    }
                    // (display.ts settle's 'resized': a framed window's new size is what borderless returns to;
                    // a maximised one keeps the size it had)
                    // SAFETY: the window's own handle.
                    if self.mode == WindowMode::Windowed
                        && s[0] > 0
                        && s[1] > 0
                        && !unsafe { IsZoomed(self.hwnd) }.as_bool()
                    {
                        self.windowed = s;
                    }
                }
            }
        }
        open
    }

    /// `key` was pressed since the last `take_menu_keys` (a fresh press: a held key's repeats are not).
    pub fn pressed(&self, key: &str) -> bool {
        self.keys.pressed(key)
    }

    /// `key` is held now.
    pub fn held(&self, key: &str) -> bool {
        self.keys.held.contains(key)
    }

    /// Hide the OS cursor over the window, or show it again. Windows asks for the cursor (WM_SETCURSOR) only when the
    /// pointer moves, so a change while the pointer stands over the client area is applied at once: the build's
    /// cursor query runs at every analog event as well (the engine's cursor query), so a pad in use never leaves
    /// the OS arrow standing over a screen that draws its own cursor.
    pub fn set_cursor_hidden(&mut self, hidden: bool) {
        if CURSOR_HIDDEN.swap(hidden, Ordering::Relaxed) == hidden || self.pointer_in_client().is_none() {
            return;
        }
        // SAFETY: the window's thread sets the cursor over its own window; the arrow is the class's cursor.
        unsafe {
            let cursor = if hidden { None } else { LoadCursorW(None, IDC_ARROW).ok() };
            SetCursor(cursor);
        }
    }

    /// The OS pointer's point in the client area, when it is over the window's client area (no other window over it).
    fn pointer_in_client(&self) -> Option<POINT> {
        let mut at = POINT::default();
        let mut r = RECT::default();
        // SAFETY: plain reads of the pointer and of the window's own rectangle into locals.
        unsafe {
            GetCursorPos(&mut at).ok()?;
            if WindowFromPoint(at) != self.hwnd || !ScreenToClient(self.hwnd, &mut at).as_bool() {
                return None;
            }
            GetClientRect(self.hwnd, &mut r).ok()?;
        }
        (at.x >= r.left && at.x < r.right && at.y >= r.top && at.y < r.bottom).then_some(at)
    }

    /// APlayerController::SetMouseLocation for the game's cursor (UI_Cursor's Set Position): the OS pointer moved to
    /// `at` (client pixels, floor(x + 0.5) as FSceneViewport::SetMouse), and the window's last pointer point with it,
    /// so the move it makes is no mouse motion (SetPointerPosition sets the current and the previous position alike,
    /// FViewport::SetMouse). Only while the window is the foreground one: a pad read in the background
    /// never takes the pointer from another program. The client point the pointer went to, if it moved.
    pub fn warp_pointer(&mut self, at: [f64; 2]) -> Option<[f64; 2]> {
        let [w, h] = self.client_size();
        if w == 0 || h == 0 {
            return None;
        }
        let x = (at[0] + 0.5).floor().clamp(0.0, f64::from(w - 1)) as i32;
        let y = (at[1] + 0.5).floor().clamp(0.0, f64::from(h - 1)) as i32;
        let mut p = POINT { x, y };
        // SAFETY: plain calls on the window's own handle and a POINT local.
        unsafe {
            if GetForegroundWindow() != self.hwnd || !ClientToScreen(self.hwnd, &mut p).as_bool() {
                return None;
            }
            SetCursorPos(p.x, p.y).ok()?;
        }
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

    /// The menus' input since the last call, in the order it came (what no screen took is dropped at the frame's
    /// end: app.rs).
    pub fn take_menu_input(&mut self) -> Vec<MenuInput> {
        std::mem::take(&mut self.menu_input)
    }

    /// Whether the window lost the keyboard since the last call.
    pub fn take_blur(&mut self) -> bool {
        std::mem::take(&mut self.blurred)
    }

    /// The window's scale: its monitor's DPI over 96 (1 when unknown).
    pub fn scale(&self) -> f32 {
        // SAFETY: the window's own handle.
        let dpi = unsafe { GetDpiForWindow(self.hwnd) };
        if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
    }

    /// The window's title (the perf line when the UI is off).
    pub fn set_title(&self, title: &str) {
        // SAFETY: a valid window and string.
        unsafe {
            let _ = SetWindowTextW(self.hwnd, &HSTRING::from(title));
        }
    }
}

/// A display (EnumDisplayMonitors): its system name (the window settings' `display`), its desktop rectangle and its
/// work area (the desktop less the taskbar) in physical pixels (left, top, right, bottom), its refresh rate in Hz
/// (0: unknown), whether it is the primary.
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

/// The refresh rate in Hz of the display device named `device` (a MONITORINFOEXW's szDevice, nul-terminated); 0 when
/// unknown.
fn device_refresh_hz(device: &[u16; 32]) -> f64 {
    let mut mode = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
    // SAFETY: a nul-terminated device name of a monitor and a DEVMODEW with its size set.
    let known = unsafe { EnumDisplaySettingsW(PCWSTR(device.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode) }.as_bool();
    // (dmDisplayFrequency 0 and 1 mean the hardware's default)
    if known && mode.dmDisplayFrequency > 1 { f64::from(mode.dmDisplayFrequency) } else { 0.0 }
}

/// The displays as last listed (the program reads them every menu frame for the window's state, and enumerating them
/// asks each driver for its modes, which is slow on some): listed on the first call, again by `refresh_displays`.
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
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, list: LPARAM) -> BOOL {
        // SAFETY: `list` is the `Vec` `displays` passed, alive for the enumeration.
        let out = unsafe { &mut *(list.0 as *mut Vec<Display>) };
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        // SAFETY: `m` is the monitor the enumeration gave; `mi` is a MONITORINFOEXW with its size set.
        if !unsafe { GetMonitorInfoW(m, (&mut mi as *mut MONITORINFOEXW).cast()) }.as_bool() {
            return true.into();
        }
        let name_len = mi.szDevice.iter().position(|c| *c == 0).unwrap_or(mi.szDevice.len());
        let name = String::from_utf16_lossy(&mi.szDevice[..name_len]);
        let hz = device_refresh_hz(&mi.szDevice);
        let r = |r: RECT| [r.left, r.top, r.right, r.bottom];
        out.push(Display {
            name,
            rect: r(mi.monitorInfo.rcMonitor),
            work: r(mi.monitorInfo.rcWork),
            refresh_hz: hz,
            primary: mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
        true.into()
    }
    let mut list: Vec<Display> = Vec::new();
    // SAFETY: the callback only reads its monitors and pushes into `list`, which outlives the call.
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM((&mut list as *mut Vec<Display>) as isize));
    }
    // (a stable order: the primary first, the rest as the system lists them)
    list.sort_by_key(|d| !d.primary);
    list
}

impl Window {
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

    /// The client area's size now (display.ts contentPx), pixels; the last known one when the system gives none.
    fn client_size(&self) -> [u32; 2] {
        let mut r = RECT::default();
        // SAFETY: the window's own handle and a RECT to fill.
        match unsafe { GetClientRect(self.hwnd, &mut r) } {
            Ok(()) if r.right > r.left && r.bottom > r.top => [(r.right - r.left) as u32, (r.bottom - r.top) as u32],
            _ => self.size,
        }
    }

    /// The display the window is mostly on, as a handle value that changes when the window moves to another (cheap to
    /// ask each frame).
    pub fn monitor_id(&self) -> isize {
        // SAFETY: the window's own handle.
        unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTOPRIMARY) }.0 as isize
    }

    /// The refresh rate in Hz of the display the window is mostly on; 0 when unknown.
    pub fn refresh_hz(&self) -> f64 {
        // SAFETY: the window's own handle.
        let m = unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTOPRIMARY) };
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        // SAFETY: a monitor handle and a MONITORINFOEXW with its size set.
        if !unsafe { GetMonitorInfoW(m, (&mut mi as *mut MONITORINFOEXW).cast()) }.as_bool() {
            return 0.0;
        }
        device_refresh_hz(&mi.szDevice)
    }

    /// The index in `displays` of the display the window is mostly on (0 when it matches none).
    pub fn display_index(&self, displays: &[Display]) -> usize {
        // SAFETY: the window's own handle.
        let m = unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTOPRIMARY) };
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        // SAFETY: a monitor handle and a MONITORINFO with its size set.
        if !unsafe { GetMonitorInfoW(m, &mut mi) }.as_bool() {
            return 0;
        }
        let rc = [mi.rcMonitor.left, mi.rcMonitor.top, mi.rcMonitor.right, mi.rcMonitor.bottom];
        displays.iter().position(|d| d.rect == rc).unwrap_or(0)
    }

    /// The window put in `mode` on `display` (None: the one it is on): framed at `size` (client pixels; None: its own
    /// windowed size), centred in the display's work area and kept inside it, or borderless over the whole display,
    /// where `size` is the one it returns to (app/src/display.ts: setMode, setSize, setDisplay, place, contentDip;
    /// `BrowserWindow.setFullScreen`, which on Windows takes the frame off and covers the whole display). The swap
    /// chain follows the WM_SIZE it sends.
    // Deviation: no exclusive full screen (EWindowMode Fullscreen): borderless covers the display, as the TS port did.
    pub fn set_window_mode(
        &mut self,
        mode: WindowMode,
        size: Option<[u32; 2]>,
        display: Option<&Display>,
    ) -> Result<()> {
        let all = refresh_displays();
        let background = BACKGROUND.load(Ordering::Relaxed);
        // (a background window stays on its display, never activated)
        let display = if background { None } else { display };
        let quiet = if background { SWP_NOACTIVATE } else { SET_WINDOW_POS_FLAGS(0) };
        let target = match display {
            Some(d) => d.clone(),
            None => all
                .get(self.display_index(&all))
                .or(all.first())
                .cloned()
                .ok_or_else(|| Error::Os { what: "no display".to_owned(), source: windows::core::Error::empty() })?,
        };
        let [wl, wt, wr, wb] = target.work;
        let work = [(wr - wl).max(1) as u32, (wb - wt).max(1) as u32];
        // SAFETY: Win32 window calls on this window's handle, from its own thread.
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
                    let [l, t, r, b] = target.rect;
                    SetWindowLongPtrW(self.hwnd, GWL_STYLE, (WS_POPUP | WS_VISIBLE).0 as isize);
                    SetWindowPos(
                        self.hwnd,
                        Some(HWND_TOP),
                        l,
                        t,
                        r - l,
                        b - t,
                        SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOOWNERZORDER | quiet,
                    )
                    .map_err(os("SetWindowPos"))?;
                }
                WindowMode::Windowed => {
                    let style = WS_OVERLAPPEDWINDOW | WS_VISIBLE;
                    if IsZoomed(self.hwnd).as_bool() {
                        let _ = ShowWindow(self.hwnd, SW_RESTORE);
                    }
                    SetWindowLongPtrW(self.hwnd, GWL_STYLE, style.0 as isize);
                    let mut f = RECT::default();
                    AdjustWindowRectEx(&mut f, style, false, WINDOW_EX_STYLE::default())
                        .map_err(os("AdjustWindowRectEx"))?;
                    let frame = [(f.right - f.left).max(0) as u32, (f.bottom - f.top).max(0) as u32];
                    // (no size asked: the one it has while framed, else the one it returns to: pendingPx ?? windowedPx)
                    let own = if self.mode == WindowMode::Windowed { self.client_size() } else { self.windowed };
                    let c = content_size(size.unwrap_or(own), target.size(), work, frame);
                    let (w, h) = (c[0] as i32 + frame[0] as i32, c[1] as i32 + frame[1] as i32);
                    let (x, y) = (wl + (wr - wl - w).max(0) / 2, wt + (wb - wt - h).max(0) / 2);
                    SetWindowPos(
                        self.hwnd,
                        Some(HWND_NOTOPMOST),
                        x,
                        y,
                        w,
                        h,
                        SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOOWNERZORDER | quiet,
                    )
                    .map_err(os("SetWindowPos"))?;
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
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// A WM_INPUT's keyboard event: its FKey name and whether it went down.
fn raw_key(lparam: LPARAM) -> Option<(&'static str, bool)> {
    let mut raw = RAWINPUT::default();
    let mut size = size_of::<RAWINPUT>() as u32;
    // SAFETY: the WM_INPUT's handle and a buffer of `size` bytes.
    let n = unsafe {
        GetRawInputData(
            HRAWINPUT(lparam.0 as _),
            RID_INPUT,
            Some((&mut raw as *mut RAWINPUT).cast()),
            &mut size,
            size_of::<RAWINPUTHEADER>() as u32,
        )
    };
    if n == u32::MAX || raw.header.dwType != RIM_TYPEKEYBOARD.0 {
        return None;
    }
    // SAFETY: a keyboard's RAWINPUT holds its RAWKEYBOARD.
    let k = unsafe { raw.data.keyboard };
    let name = key_name(k.VKey, k.MakeCode, k.Flags & RI_KEY_E0 != 0)?;
    Some((name, k.Flags & RI_KEY_BREAK == 0))
}

extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let mouse = |name, down| {
        push(Msg::Mouse(name, down));
        LRESULT(0)
    };
    match msg {
        WM_INPUT => {
            if let Some((k, down)) = raw_key(lparam) {
                push(Msg::Key(k, down));
            }
        }
        WM_LBUTTONDOWN => return mouse("LeftMouseButton", true),
        WM_LBUTTONUP => return mouse("LeftMouseButton", false),
        WM_RBUTTONDOWN => return mouse("RightMouseButton", true),
        WM_RBUTTONUP => return mouse("RightMouseButton", false),
        WM_MBUTTONDOWN => return mouse("MiddleMouseButton", true),
        WM_MBUTTONUP => return mouse("MiddleMouseButton", false),
        WM_XBUTTONDOWN | WM_XBUTTONUP => {
            // (HIWORD of wParam: XBUTTON1 1, XBUTTON2 2)
            let name = if (wparam.0 >> 16) & 0xffff == 1 { "ThumbMouseButton" } else { "ThumbMouseButton2" };
            push(Msg::Mouse(name, msg == WM_XBUTTONDOWN));
            return LRESULT(1);
        }
        WM_MOUSEMOVE => {
            // (GET_X_LPARAM / GET_Y_LPARAM: signed words, negative while a button drags the pointer outside)
            let x = f32::from((lparam.0 & 0xffff) as u16 as i16);
            let y = f32::from(((lparam.0 >> 16) & 0xffff) as u16 as i16);
            push(Msg::Pointer(Some((x, y))));
            if !TRACKING.with(|t| t.replace(true)) {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                // SAFETY: a valid window and a filled TRACKMOUSEEVENT.
                let _ = unsafe { TrackMouseEvent(&mut tme) };
            }
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            TRACKING.with(|t| t.replace(false));
            push(Msg::Pointer(None));
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            // (HIWORD of wParam: the signed delta in WHEEL_DELTA (120) units)
            let delta = ((wparam.0 >> 16) & 0xffff) as u16 as i16;
            push(Msg::Wheel(f32::from(delta) / WHEEL_DELTA as f32));
            return LRESULT(0);
        }
        WM_CHAR => {
            // UTF-16 code units: a surrogate pair comes as two messages
            let u = (wparam.0 & 0xffff) as u16;
            let c = match u {
                0xd800..=0xdbff => {
                    SURROGATE.with(|s| s.replace(Some(u)));
                    None
                }
                0xdc00..=0xdfff => {
                    SURROGATE.with(|s| s.take()).and_then(|hi| char::decode_utf16([hi, u]).next().and_then(|r| r.ok()))
                }
                _ => char::from_u32(u32::from(u)),
            };
            if let Some(c) = c {
                push(Msg::Char(c));
            }
            return LRESULT(0);
        }
        WM_KILLFOCUS => push(Msg::Blur),
        WM_DEVICECHANGE if wparam.0 == DBT_DEVICEARRIVAL as usize => push(Msg::DeviceArrived),
        WM_SIZE => {
            let w = (lparam.0 & 0xffff) as u32;
            let h = ((lparam.0 >> 16) & 0xffff) as u32;
            push(Msg::Size([w, h]));
        }
        WM_CLOSE => {
            push(Msg::Close);
            return LRESULT(0);
        }
        // The OS cursor hidden over the client area (a screen that draws its own pointer).
        WM_SETCURSOR if CURSOR_HIDDEN.load(Ordering::Relaxed) && (lparam.0 & 0xffff) as u32 == HTCLIENT => {
            // SAFETY: no cursor: the window's own.
            unsafe { SetCursor(None) };
            return LRESULT(1);
        }
        // The launcher's title bar drags its frameless window (Window::set_drag_band).
        WM_NCHITTEST if DRAG_BAND.load(Ordering::Relaxed) > 0 => {
            // SAFETY: the default procedure, then plain reads of the window's own rectangle.
            unsafe {
                let hit = DefWindowProcW(hwnd, msg, wparam, lparam);
                if hit.0 != HTCLIENT as isize {
                    return hit;
                }
                // (GET_X_LPARAM / GET_Y_LPARAM: signed screen coordinates)
                let mut at = POINT {
                    x: i32::from((lparam.0 & 0xffff) as u16 as i16),
                    y: i32::from(((lparam.0 >> 16) & 0xffff) as u16 as i16),
                };
                let mut r = RECT::default();
                if ScreenToClient(hwnd, &mut at).as_bool() && GetClientRect(hwnd, &mut r).is_ok() {
                    let band = DRAG_BAND.load(Ordering::Relaxed);
                    let right = DRAG_RIGHT.load(Ordering::Relaxed);
                    if at.y >= 0 && at.y < band && at.x < r.right - right {
                        return LRESULT(HTCAPTION as isize);
                    }
                }
                return hit;
            }
        }
        // (the keyboard's keys are the raw input's: F10 and Alt open no window menu)
        WM_SYSCOMMAND if wparam.0 & 0xfff0 == SC_KEYMENU as usize => return LRESULT(0),
        _ => {}
    }
    // SAFETY: the default procedure for the rest.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_held_key_is_pressed_once() {
        let mut k = super::KeyRecord::default();
        assert!(!k.key("Escape", true));
        assert!(k.pressed("Escape"));
        k.take();
        // the OS's auto-repeat while Escape is held: downs again, no press (the pause would toggle at each)
        for _ in 0..5 {
            assert!(k.key("Escape", true));
            assert!(!k.pressed("Escape"));
            assert_eq!(k.take(), ["Escape"]);
        }
        k.key("Escape", false);
        assert!(!k.key("Escape", true));
        assert!(k.pressed("Escape"));
    }

    use super::*;

    #[test]
    fn a_saved_size_is_kept_inside_its_display() {
        let (display, work) = ([1920, 1080], [1920, 1040]);
        // (none saved, or larger than the display: the first-run size)
        assert_eq!(content_size([0, 0], display, work, [0, 0]), FIRST_RUN);
        assert_eq!(content_size([2560, 1440], display, work, [0, 0]), FIRST_RUN);
        assert_eq!(content_size([1600, 900], display, work, [16, 39]), [1600, 900]);
        // (the display's own size, framed: the work area less the frame)
        assert_eq!(content_size([1920, 1080], display, work, [16, 39]), [1904, 1001]);
    }

    #[test]
    fn the_launcher_fits_its_work_area() {
        // (the page at the monitor's scale when it fits; smaller to fit; never below the least zoom)
        assert_eq!(launcher_size([1200, 720], 1.0, [1920, 1040], 0.6), [1200, 720]);
        assert_eq!(launcher_size([1200, 720], 1.5, [3840, 2100], 0.6), [1800, 1080]);
        assert_eq!(launcher_size([1200, 720], 1.5, [1920, 1040], 0.6), [1733, 1040]);
        assert_eq!(launcher_size([1200, 720], 1.0, [600, 400], 0.6), [720, 432]);
    }
}
