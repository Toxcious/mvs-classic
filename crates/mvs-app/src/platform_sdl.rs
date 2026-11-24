//! The Linux window, beside `platform.rs` (selected as `platform` on Linux): the same public items and signatures.
//! For now no window opens: making one is an error, and the rest answers with empty values.

// Unverified: stub, replaced in Task 7

use std::sync::Arc;

use mvs_d3d::Foundation::HWND;
use mvs_settings::WindowMode;
use mvs_ui::menu::MenuInput;

use crate::input::Devices;
use crate::{Error, Result};

fn not_built() -> Error {
    Error::Platform("not built yet".into())
}

#[derive(Debug)]
pub struct Window {
    pub hwnd: HWND,
    /// The client area's size, pixels.
    pub size: [u32; 2],
    mode: WindowMode,
}

/// The window's first-run client size (display.ts FIRST_RUN): a saved size of 0 or larger than its display.
pub const FIRST_RUN: [u32; 2] = [1280, 720];

/// The client size for an asked one on a display of `display` pixels whose work area is `work`: the asked one.
pub fn content_size(asked: [u32; 2], _display: [u32; 2], _work: [u32; 2], _frame: [u32; 2]) -> [u32; 2] {
    asked
}

/// The launcher window's client size, pixels: the page's.
pub fn launcher_size(page: [u32; 2], _scale: f32, _work: [u32; 2], _least: f32) -> [u32; 2] {
    page
}

/// A test run's windows open in the background: nothing to open yet.
pub fn set_background(_on: bool) {}

impl Window {
    /// A window titled `title` with a client area of `size`, or borderless over its monitor (`full_screen`).
    pub fn new(_title: &str, _size: [u32; 2], _full_screen: bool) -> Result<Window> {
        Err(not_built())
    }

    /// The launcher's window: frameless and of a fixed size, `page` at the monitor's scale or smaller to fit, down
    /// to `least` of it.
    pub fn launcher(_title: &str, _page: [u32; 2], _least: f32) -> Result<Window> {
        Err(not_built())
    }

    /// The band at the top of the client area that drags the window.
    pub fn set_drag_band(&self, _height: u32, _right: u32) {}

    /// The window minimised to the taskbar.
    pub fn minimize(&self) {}

    /// Whether the window is the foreground one.
    pub fn foreground(&self) -> bool {
        false
    }

    /// Whether the window is minimised.
    pub fn minimized(&self) -> bool {
        false
    }

    /// The waiting events handled into `devices`; false once the window is closed.
    pub fn pump(&mut self, _devices: &mut Devices, _resized: &mut Option<[u32; 2]>) -> bool {
        false
    }

    /// `key` was pressed since the last `take_menu_keys`.
    pub fn pressed(&self, _key: &str) -> bool {
        false
    }

    /// `key` is held now.
    pub fn held(&self, _key: &str) -> bool {
        false
    }

    /// Hide the OS cursor over the window, or show it again.
    pub fn set_cursor_hidden(&mut self, _hidden: bool) {}

    /// The OS pointer moved to `at` (client pixels); the client point it went to, if it moved.
    pub fn warp_pointer(&mut self, _at: [f64; 2]) -> Option<[f64; 2]> {
        None
    }

    /// The keys that went down since the last call.
    pub fn take_menu_keys(&mut self) -> Vec<&'static str> {
        Vec::new()
    }

    /// The menus' input since the last `take_menu_input`, to read.
    pub fn menu_input(&self) -> &[MenuInput] {
        &[]
    }

    /// The menus' input since the last call, in the order it came.
    pub fn take_menu_input(&mut self) -> Vec<MenuInput> {
        Vec::new()
    }

    /// Whether the window lost the keyboard since the last call.
    pub fn take_blur(&mut self) -> bool {
        false
    }

    /// The window's title (the perf line when the UI is off).
    pub fn set_title(&self, _title: &str) {}

    /// The window's mode (set by `set_window_mode`, or its creation's).
    pub fn mode(&self) -> WindowMode {
        self.mode
    }

    /// The window's size as the Graphics page reads it.
    pub fn windowed_size(&self) -> [u32; 2] {
        self.size
    }

    /// The display the window is mostly on, as a value that changes when the window moves to another.
    pub fn monitor_id(&self) -> isize {
        0
    }

    /// The refresh rate in Hz of the display the window is mostly on; 0 when unknown.
    pub fn refresh_hz(&self) -> f64 {
        0.0
    }

    /// The index in `displays` of the display the window is mostly on (0 when it matches none).
    pub fn display_index(&self, _displays: &[Display]) -> usize {
        0
    }

    /// The window put in `mode` on `display` (None: the one it is on), framed at `size` or borderless.
    pub fn set_window_mode(
        &mut self,
        _mode: WindowMode,
        _size: Option<[u32; 2]>,
        _display: Option<&Display>,
    ) -> Result<()> {
        Err(not_built())
    }
}

/// A display: its system name (the window settings' `display`), its desktop rectangle and its work area (the
/// desktop less the taskbar) in physical pixels (left, top, right, bottom), its refresh rate in Hz (0: unknown),
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

/// The displays, the primary first, as last listed: none yet.
pub fn displays() -> Arc<Vec<Display>> {
    Arc::new(Vec::new())
}

/// The displays listed again: none yet.
pub fn refresh_displays() -> Arc<Vec<Display>> {
    Arc::new(Vec::new())
}
