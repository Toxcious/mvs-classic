//! The launcher's screens (app/launcher): its own 2D kit (paint.rs: rectangles, rounded rectangles, gradients,
//! strokes, the picture, text from bundled fonts), the splash, the setup steps, the import's progress, Home and the
//! settings overlay. The app crate keeps the launcher's state (the worker, the settings, the routes) and hands a
//! `Model` each frame; the screens draw it and answer the player's choices as `Action`s.
//!
//! The launcher is a program of its own (`mvs-launcher.exe`) in a frameless window of the page's size: the page
//! draws its title bar (its drag band, the minimise and close buttons) and Home its RUNNING state while the game's
//! program it started runs.
//!
//! Deviation: no game log (the game's program writes its lines to its own console; the Game log tab stays off).

pub mod draw;
pub mod fonts;
pub mod import_view;
pub mod paint;
mod screens;
mod sky;
pub mod words;

use std::path::PathBuf;

use mvs_render::gpu::{Gpu, Texture};

use self::draw::{Canvas, Pointer, fit};
use self::fonts::Fonts;
use self::paint::Painter;
use crate::Result;

/// The title bar's height, page pixels (styles.css .titlebar): the window's drag band.
pub const TITLE_BAR: f32 = 48.0;
/// A window button's width (.wbtn): Minimize and Close at the bar's right.
pub const WINDOW_BUTTON: f32 = 50.0;
/// The bar's width at its right that does not drag: the window buttons and Home's settings button left of them
/// (.topright's 34 px button, 12 px from them).
pub const TITLE_BAR_KEEP: f32 = 2.0 * WINDOW_BUTTON + 12.0 + 34.0;

/// A line's colour (the page's ok / warn / bad notes and log levels).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Plain,
    Ok,
    Warn,
    Bad,
}

/// A line of text in its tone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Note {
    pub text: String,
    pub tone: Tone,
}

impl Note {
    pub fn new(text: impl Into<String>, tone: Tone) -> Note {
        Note { text: text.into(), tone }
    }
}

/// The screen showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Splash,
    /// Setup's step 1, the install location.
    Install,
    /// Step 2, the beta's folder.
    Beta,
    /// Step 3, the build.
    Build,
    Home,
}

/// Step 1's facts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstallStep {
    pub root: String,
    /// The free space line (None while it is asked).
    pub space: Option<Note>,
    /// What is wrong with the place.
    pub problem: Option<Note>,
    pub can_continue: bool,
    /// "Back to the default" shows (a chosen root other than the default).
    pub can_default: bool,
    /// Setup was done before: Cancel goes back to Home.
    pub can_cancel: bool,
}

/// A folder of step 2's list (setup.ts SourceCandidate).
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub note: String,
    /// The folder shown (the paks, else the folder looked at).
    pub path: String,
    pub paks: PathBuf,
    /// The open beta with its key: it can be chosen.
    pub usable: bool,
}

/// Step 2's facts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BetaStep {
    /// Why the folder is asked for again (setup lost it).
    pub why: Option<String>,
    /// The folders found (None while they are looked for).
    pub sources: Option<Vec<Source>>,
    pub chosen: Option<PathBuf>,
}

/// A group's line of the build (launcher.ts runGroups).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseState {
    Done,
    Now,
    Wait,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Phase {
    pub name: String,
    pub state: PhaseState,
    /// The kinds done of its kinds ('' when done).
    pub n: String,
}

/// A run's progress as the screens draw it (launcher.ts drawRun).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Build {
    /// The run stopped with this message (setup's step shows it; Home goes back to its state).
    pub stopped: Option<String>,
    /// 0..1.
    pub frac: f64,
    /// The step going on.
    pub now: String,
    /// The line under it (bytes checked, a kind's entries).
    pub count: String,
    pub left: String,
    pub written: String,
    /// "Installing to ...".
    pub place: String,
    pub phases: Vec<Phase>,
    /// The details: time, level, text.
    pub log: Vec<(String, Tone, String)>,
}

/// Home's state (launcher.ts HomeState).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HomeState {
    #[default]
    Ready,
    /// A newer release is out: PLAY updates; the new version starts on Home.
    Update,
    /// The release is being downloaded; the launcher starts again as the new version.
    Updating,
    /// A rebuild runs; the game opens after it.
    Rebuild,
    /// The beta's folder cannot be read.
    Problem,
    /// The game started from PLAY runs: PLAY brings its window forward.
    Running,
}

/// A line of the launcher log.
#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    /// hh:mm:ss.
    pub time: String,
    pub tone: Tone,
    /// A word shown in the level's colour first ('' none: the whole line takes it).
    pub em: String,
    pub text: String,
}

/// Home's facts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HomeView {
    pub state: HomeState,
    /// The status line: its dot, its bold words, the rest.
    pub status: (Tone, String, String),
    pub problem: String,
    pub can_rebuild: bool,
    /// Updating: the download's line and its percent.
    pub progress_label: String,
    pub percent: f64,
    /// The more menu's "Check for updates": None hidden (a build with no feed), else whether it can be pressed.
    pub check_updates: Option<bool>,
    /// (the port's own) A Test connection runs: PLAY waits for it.
    pub testing: bool,
}

/// The desktop and Start menu shortcuts (bridge.ts ShortcutState).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shortcuts {
    /// Shortcuts can be made (a release's exe; not a dev run).
    pub available: bool,
    pub desktop: bool,
    pub start_menu: bool,
}

/// The settings' Online tab (launcher.ts drawOnline): the config the program runs on and each backend's line. A
/// `Note` with no text is hidden. The boxes' text is the page's while one has the focus.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnlineSettingsView {
    /// The relay's address ('' none).
    pub relay: String,
    pub eos: bool,
    /// LAN's port; None: LAN is off.
    pub lan: Option<u16>,
    /// The port box's text while it has no focus.
    pub lan_port: String,
    pub direct: bool,
    /// The look at the typed address (launcher.ts relayStatus), and the relay's, Epic's, LAN's and the direct
    /// links' lines.
    pub relay_status: Note,
    pub relay_now: Note,
    pub epic_now: Note,
    pub lan_now: Note,
    pub direct_now: Note,
    /// The boxes' text is not an address / a port.
    pub relay_bad: bool,
    pub port_bad: bool,
    /// (the port's own) The one line of online play, above the test and the switches: its headline, and what it
    /// means or what to do under it ('' none).
    pub line: Note,
    pub line_detail: String,
    /// The last Test connection's lines (none: not run), and a test under way.
    pub test: Vec<Note>,
    pub testing: bool,
    /// The Advanced fold (the four switches) is open.
    pub advanced: bool,
}

/// The settings overlay's facts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsView {
    pub online: Box<OnlineSettingsView>,
    pub root: String,
    pub beta: Option<String>,
    pub data: (Tone, String),
    pub beta_health: (Tone, String),
    /// A run goes on: the folders and the rebuild wait.
    pub busy: bool,
    pub can_rebuild: bool,
    pub shortcuts: Shortcuts,
}

/// What the launcher shows this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub screen: Screen,
    pub install: InstallStep,
    pub beta: BetaStep,
    pub build: Build,
    pub home: HomeView,
    pub settings: SettingsView,
    /// The launcher's log lines (kept, not drawn).
    pub log: Vec<LogLine>,
    /// A line at the top about the app's own folder.
    pub app_problem: Option<String>,
    /// The splash waits for the update check.
    pub checking: bool,
}

/// The player's choices.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Step 1: the folder picker for the install location.
    PickRoot,
    /// Step 1: the default install location again.
    DefaultRoot,
    /// Step 1's Cancel (setup done before): Home.
    CancelSetup,
    /// Step 1's Continue.
    ToBeta,
    /// Step 2's Back.
    BackToInstall,
    /// Step 2: the folder picker for the beta.
    PickBeta,
    /// Step 2: look for the beta's folders again.
    Detect,
    /// Step 2: a usable folder clicked.
    Choose(PathBuf),
    /// The guide's Steam console button.
    SteamConsole,
    /// The guide's Copy: the depot command to the clipboard.
    CopyDepot,
    /// Step 2's Continue: the build.
    StartBuild,
    /// Step 3's Cancel.
    CancelBuild,
    /// Step 3's Try again.
    Retry,
    /// Step 3's Back to setup.
    BackToSetup,
    /// PLAY (ready: play, through a rebuild when parts are stale; problem: step 2).
    Play,
    /// Home's banner's Locate it.
    Locate,
    /// Rebuild everything (the more menu, the settings).
    Rebuild,
    /// The more menu's "Play without updating" (an update offered).
    PlayNow,
    /// The more menu's "Check for updates".
    CheckUpdates,
    OpenRoot,
    /// The settings opened: the app refreshes its facts.
    SettingsOpened,
    /// The settings' Move: step 1.
    MoveRoot,
    /// The settings' Change: step 2.
    ChangeBeta,
    ShortcutDesktop,
    ShortcutStartMenu,
    /// The relay box's text, RELAY_TYPE_MS after the last key (launcher.ts oninput): looked at, saved when it
    /// answers; an empty box is saved as no relay.
    RelayTyped(String),
    /// Enter in the relay box: saved at once, then looked at.
    RelayEnter(String),
    /// The port box's text, RELAY_TYPE_MS after the last key / on Enter: saved when it is a port.
    PortTyped(String),
    PortEnter(String),
    /// The Epic, LAN and direct-link switches clicked.
    ToggleEos,
    ToggleLan,
    ToggleDirect,
    /// (the port's own) Test connection, its Copy (the result as text to the clipboard), and the Advanced fold.
    TestConnection,
    CopyTest,
    ToggleAdvanced,
    /// The title bar's buttons (launcher.ts win-min, win-close).
    Minimize,
    Close,
}

/// A box of the Online tab that has the keyboard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Focus {
    #[default]
    None,
    Relay,
    Port,
}

/// How long after the last key a box's text is looked at (launcher.ts RELAY_TYPE_MS), s.
const TYPE_PAUSE: f64 = 0.6;

/// The overlays and the parts the page keeps between frames.
#[derive(Clone, Debug, Default)]
struct PageState {
    guide: bool,
    settings: bool,
    tab: usize,
    menu: bool,
    details: bool,
    /// The body's scroll of each setup step, the log's, the details'.
    scroll: [f32; 3],
    /// (the port's own) The Online tab's scroll (its Advanced fold and test lines open).
    online_scroll: f32,
    copied_at: Option<f64>,
    /// The picture's fade in started.
    picture_at: Option<f64>,
    screen: Screen,
    /// The Online tab's boxes: the focus, their text, and the page clock of the last key in each.
    focus: Focus,
    relay_box: String,
    port_box: String,
    typed_at: [Option<f64>; 2],
}

impl PageState {
    /// The keys typed into the Online tab's focused box: its text, the pause that looks at it, Enter. The relay
    /// box holds 255 characters (maxlength), the port box 5.
    fn type_into(&mut self, m: &Model, keys: &[&str], typed: &[char], t: f64, out: &mut Vec<Action>) {
        let _ = m;
        if !self.settings || self.tab != screens::ONLINE_TAB || self.focus == Focus::None {
            self.focus = Focus::None;
            return;
        }
        let (k, max) = if self.focus == Focus::Relay { (0, 255) } else { (1, 5) };
        let text = if k == 0 { &mut self.relay_box } else { &mut self.port_box };
        let mut edited = false;
        for ch in typed.iter().filter(|c| !c.is_control()) {
            if text.chars().count() < max {
                text.push(*ch);
                edited = true;
            }
        }
        if keys.contains(&"BackSpace") && text.pop().is_some() {
            edited = true;
        }
        if edited {
            self.typed_at[k] = Some(t);
        }
        let text = text.clone();
        if keys.contains(&"Enter") {
            self.typed_at[k] = None;
            out.push(if k == 0 { Action::RelayEnter(text) } else { Action::PortEnter(text) });
        } else if self.typed_at[k].is_some_and(|at| t - at >= TYPE_PAUSE) {
            self.typed_at[k] = None;
            out.push(if k == 0 { Action::RelayTyped(text) } else { Action::PortTyped(text) });
        }
    }
}

/// The launcher's page on the GPU.
pub struct LauncherUi {
    painter: Painter,
    fonts: Fonts,
    stars: [Vec<sky::Star>; 3],
    page: PageState,
}

impl std::fmt::Debug for LauncherUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LauncherUi").finish_non_exhaustive()
    }
}

impl LauncherUi {
    pub fn new(gpu: &Gpu) -> Result<LauncherUi> {
        Ok(LauncherUi {
            painter: Painter::new(gpu)?,
            fonts: Fonts::new()?,
            stars: sky::stars(),
            page: PageState::default(),
        })
    }

    /// Home's picture (the night Treehouse), or none.
    pub fn set_picture(&mut self, t: Option<std::sync::Arc<Texture>>) {
        if t.is_none() {
            self.page.picture_at = None;
        }
        self.painter.set_picture(t);
    }

    pub fn has_picture(&self) -> bool {
        self.painter.has_picture()
    }

    /// An overlay opened as a click would (a tool's pictures of each screen): the guide, the settings at `tab`
    /// (0 General, 1 Game files, 2 Online, 3 About), the more menu, the build's details.
    pub fn open(&mut self, guide: bool, settings: Option<usize>, menu: bool, details: bool) {
        self.page.guide = guide;
        self.page.settings = settings.is_some();
        self.page.tab = settings.unwrap_or(0);
        self.page.menu = menu;
        self.page.details = details;
    }

    /// One frame of `m` drawn over `target` at the page's clock `t` (s): the player's choices. `pointer` in the
    /// target's pixels; `keys` the keys that went down (Escape closes the overlays).
    #[allow(clippy::too_many_arguments)] // the frame's inputs: gpu, target, model, pointer, keys, typed, clock
    pub fn frame(
        &mut self,
        gpu: &Gpu,
        target: &Texture,
        m: &Model,
        pointer: Pointer,
        keys: &[&str],
        typed: &[char],
        t: f64,
    ) -> Result<Vec<Action>> {
        let (size, s) = fit([target.desc.size[0], target.desc.size[1]]);
        let pointer = Pointer { at: pointer.at.map(|p| [p[0] / s, p[1] / s]), ..pointer };
        if keys.contains(&"Escape") {
            self.page.settings = false;
            self.page.guide = false;
            self.page.menu = false;
        }
        if m.screen != self.page.screen {
            if m.screen != Screen::Home {
                self.page.settings = false;
                self.page.menu = false;
            }
            if m.screen != Screen::Beta {
                self.page.guide = false;
            }
            self.page.scroll = [0.0; 3];
            self.page.screen = m.screen;
        }
        if self.painter.has_picture() && self.page.picture_at.is_none() && m.screen == Screen::Home {
            self.page.picture_at = Some(t);
        }
        let mut c =
            Canvas { gpu, p: &mut self.painter, fonts: &mut self.fonts, s, size, pointer, t, blocked: false, xf: None };
        let mut out = Vec::new();
        self.page.type_into(m, keys, typed, t, &mut out);
        screens::page(&mut c, m, &mut self.page, &self.stars, &mut out);
        self.painter.draw(gpu, target)?;
        Ok(out)
    }
}

/// hh:mm:ss of the local time now (launcher.ts clock).
pub fn clock_now() -> String {
    clock(local_seconds())
}

/// hh:mm:ss of the local time `at` was (a log line's time).
pub fn clock_at(at: std::time::SystemTime) -> String {
    let ago = std::time::SystemTime::now().duration_since(at).map_or(0, |d| d.as_secs());
    clock(local_seconds() + 86_400 - ago % 86_400)
}

/// hh:mm:ss of seconds since midnight.
pub fn clock(secs: u64) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600 % 24, secs / 60 % 60, secs % 60)
}

/// The local time of day, s (the system's local time; UTC when it cannot be read).
fn local_seconds() -> u64 {
    // SAFETY: GetLocalTime only reads the clock.
    let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    u64::from(st.wHour) * 3600 + u64::from(st.wMinute) * 60 + u64::from(st.wSecond)
}
