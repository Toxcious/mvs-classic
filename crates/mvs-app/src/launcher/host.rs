//! The launcher's program (`mvs-launcher.exe`, app/src/window.ts openLauncher): its own frameless window of the
//! page's size, the GPU and swap chain the page is drawn on, the pointer and keys, the settings, and the frame loop.
//! It never opens the store for the game and never reads the game's window mode: PLAY starts the game's program.

use std::path::PathBuf;
use std::time::Duration;

use mvs_render::SwapChain;
use mvs_render::gpu::Gpu;
use mvs_settings::Settings;
use mvs_ui::launcher::draw::{PAGE, fit};
use mvs_ui::launcher::{TITLE_BAR, TITLE_BAR_KEEP};

use super::game::GameArgs;
use super::{Launcher, Next};
use crate::Result;
use crate::input::Devices;
use crate::platform::Window;

/// The smallest the window is made to fit a small screen, of the page's size (window.ts LAUNCHER_MIN_ZOOM).
const LEAST_ZOOM: f32 = 0.6;
/// A minimised launcher's poll of its workers, the game and the updater, instead of a frame.
const MINIMISED_POLL: Duration = Duration::from_millis(50);

/// How the launcher starts (`mvs-launcher.exe`'s arguments).
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// The data folder (settings.json; the default data root): mvs_settings::data_dir.
    pub data: PathBuf,
    /// `--data` was given: the game is given it too.
    pub data_given: bool,
    /// `--store`: a store already imported, played as it is: Home at once, no import state asked, no rebuild.
    pub store: Option<PathBuf>,
    /// The release's online defaults (online.json beside the program, or `--online-defaults`).
    pub online_defaults: PathBuf,
    pub adapter: Option<String>,
    /// `--play` (window.ts openWindow's `--play`): the game at once when it is ready, the launcher closing; else the
    /// launcher shows what is needed.
    pub play: bool,
    /// The game's program.
    pub game: PathBuf,
    /// The arguments the launcher does not know, for the game (GameArgs::pass_on).
    pub pass_on: Vec<String>,
}

/// The parts the launcher runs on.
pub struct Host {
    pub window: Window,
    pub gpu: Gpu,
    pub swap: SwapChain,
    pub devices: Devices,
    pub settings: Settings,
    /// The settings as last read or written (`save_settings` writes what changed since over the file).
    pub saved: Settings,
    /// The data folder.
    pub data: PathBuf,
    /// The release's online defaults.
    pub online_defaults: PathBuf,
    /// `Options::store`.
    pub store: Option<PathBuf>,
    /// `Options::play`.
    pub play: bool,
    /// What PLAY starts the game with.
    pub game: GameArgs,
}

impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host").field("size", &self.window.size).field("store", &self.store).finish_non_exhaustive()
    }
}

impl Host {
    /// The window (titled MVSC Launcher, frameless, the page's size fitted to the screen), the GPU and the settings.
    pub fn new(o: &Options) -> Result<Host> {
        let page = PAGE.map(|v| v as u32);
        let window = Window::launcher("MVSC Launcher", page, LEAST_ZOOM)?;
        let (_, s) = fit(window.size);
        window.set_drag_band((TITLE_BAR * s).round() as u32, (TITLE_BAR_KEEP * s).round() as u32);
        let gpu = Gpu::new(o.adapter.as_deref(), false)?;
        let swap = SwapChain::new(&gpu, window.hwnd, window.size)?;
        let settings = Settings::load(&o.data);
        Ok(Host {
            window,
            gpu,
            swap,
            devices: Devices::default(),
            saved: settings.clone(),
            settings,
            data: o.data.clone(),
            online_defaults: o.online_defaults.clone(),
            store: o.store.clone(),
            play: o.play,
            game: GameArgs {
                exe: o.game.clone(),
                data: o.data_given.then(|| o.data.clone()),
                online_defaults: o.online_defaults.clone(),
                adapter: o.adapter.clone(),
                pass_on: o.pass_on.clone(),
            },
        })
    }
}

impl Host {
    /// Writes the launcher's own changes over the settings file as it is now (the game may have changed it since,
    /// a volume, a fighter, a known relay: `Settings::save_changes`).
    pub fn save_settings(&mut self) -> std::result::Result<(), mvs_settings::SettingsError> {
        self.settings.save_changes(&mut self.saved, &self.data)
    }
}

/// The launcher from its splash until its window closes, it quits for an update or `--play` started the game.
pub fn run(o: &Options) -> Result<()> {
    let mut host = Host::new(o)?;
    let mut l = Launcher::new(&mut host)?;
    loop {
        host.swap.wait(1000);
        let mut resized = None;
        if !host.window.pump(&mut host.devices, &mut resized) {
            return Ok(());
        }
        if let Some(size) = resized {
            host.swap.resize(&host.gpu, size)?;
        }
        // (the menus' input queue: the launcher reads its keys and pointer alone)
        host.window.take_menu_input();
        let next = if host.window.minimized() {
            let next = l.poll(&mut host);
            std::thread::sleep(MINIMISED_POLL);
            next
        } else {
            l.frame(&mut host)?
        };
        if next == Next::Quit {
            return Ok(());
        }
    }
}
