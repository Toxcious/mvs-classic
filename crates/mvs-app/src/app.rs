//! The program from its start to its quit: the parts every screen shares (the window,
//! the GPU, the devices, the store's renderer, sound and UI, the settings) and the screen that runs on them, the
//! title, the menus, a match's load or the match, one at a time, each frame's answer naming the next. The launcher
//! is a program of its own (`mvs-launcher`, crate::launcher) that starts this one on its store.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use mvs_audio::{Audio, Backend};
use mvs_render::gpu::{Gpu, GpuTimer};
use mvs_render::{Assets, Passes, Renderer, SwapChain};
use mvs_settings::Settings;
use mvs_sim::MatchSetup;
use mvs_store::Store;
use mvs_ui::Ui;
use mvs_ui::bindings::Bindings;
use mvs_ui::bundle::Bundle;
use mvs_ui::element::ElementList;
use mvs_ui::menu::input::Pointer;
use mvs_ui::menu::lobby::LocalLobby;
use mvs_ui::menu::session::Session;
use mvs_ui::text::{Fonts, GameText};

use crate::apply::SettingsApply;
use crate::input::{DeviceKind, Devices};
use crate::lab_setup::LabCarry;
use crate::loading::Loading;
use crate::menu_audio::MenuMusic;
use crate::menus::MenuHost;
use crate::perf::{Perf, Totals};
use crate::platform::Window;
use crate::run::{MatchLoop, MatchOptions, default_setup};
use crate::script::ScriptRun;
use crate::{Error, Result};

/// A paced script frame's shortest length, s (60 a second).
const SCRIPT_FRAME_S: f64 = 1.0 / 60.0;

/// How the shell opens.
#[derive(Debug)]
pub struct ShellOptions {
    /// The store opened at the start (None: a load bench's own, or none).
    pub store: Option<PathBuf>,
    /// The data folder (settings.json, saves): mvs_settings::data_dir.
    pub data: PathBuf,
    /// The client area's size (the monitor's in full screen).
    pub size: [u32; 2],
    pub full_screen: bool,
    pub adapter: Option<String>,
    /// The HUD, the menus, the fade and the results drawn (the mvs-ui crate).
    pub ui: bool,
    /// The sound played on the default device (off with `--no-audio`).
    pub audio: bool,
    /// The sound mixed to a buffer nobody hears, not to the device (`--load-bench`).
    pub audio_offline: bool,
    /// The release's online defaults (`online.json` beside the program; `--online-defaults`): read by a normal run's
    /// sign-in and the launcher's Online tab.
    pub online_defaults: PathBuf,
}

/// What the UI crate's screens share, loaded once with the store: the element batches, the menus' widget bundle,
/// its fonts, the game's text.
pub struct UiShared {
    pub ui: Ui,
    pub menu: Arc<Bundle>,
    pub menu_fonts: Fonts,
    pub text: Arc<GameText>,
    /// The frame's elements (cleared by whoever draws).
    pub list: ElementList,
    pub timer: GpuTimer,
    /// The menus' key bindings (LoadInputBindings: the saved ones, else the build's defaults).
    pub bindings: Bindings,
    /// The window's pointer as the menu screens share it.
    pub pointer: Pointer,
}

/// The parts every screen runs on.
pub struct Shell {
    pub window: Window,
    pub swap: SwapChain,
    /// The GPU while no store is open; it moves into the renderer when one opens (exactly one of the two is set).
    gpu: Option<Gpu>,
    /// The store's renderer (its Assets hold the store).
    pub renderer: Option<Renderer>,
    pub devices: Devices,
    pub perf: Perf,
    /// The sound engine (None with `--no-audio` or before a store opens).
    pub audio: Option<Audio>,
    /// The UI crate's shared parts (None with `--no-ui` or before a store opens).
    pub ui: Option<UiShared>,
    pub settings: Settings,
    /// What the settings' targets were last given, and the save (apply.rs).
    pub apply: SettingsApply,
    /// The data folder.
    pub data: PathBuf,
    /// The release's online defaults (ShellOptions::online_defaults).
    pub online_defaults: PathBuf,
    /// What the menus keep across screens (the homescreen queue, the Local Play flag, the kept lobby): handed to a
    /// menu view when it opens and taken back when it ends.
    pub session: Session,
    /// The menu music's flow.
    pub music: MenuMusic,
    /// A scripted run's script and tape (`--input-script`): the menus take their input and clock from it.
    pub script: Option<ScriptRun>,
    /// `--press-results`: the V key (the menus' OnSubmit3: NEXT) pressed on every results screen of the run this
    /// many seconds after it opens.
    pub results_press: Option<f64>,
    /// `--real-pace` on a scripted run: its frames at 60 a second on the wall clock, presented synced.
    pub real_pace: bool,
    /// The directory a scripted run's pictures go to.
    pub script_out: PathBuf,
    /// How far a scripted run is from its first frame (the menus' settling).
    pub script_gate: crate::menus::ScriptGate,
    /// A match's scenery has begun and not ended (the next match's stage change and the match's end end it).
    pub scenery_open: bool,
    /// The effects of a level a Lab fighter change keeps, from the match's finish to the next match's start
    /// (run.rs: the scenery's blocks hold ids on its particle player).
    pub kept_level: Option<mvs_render::fx::KeptLevel>,
    /// Which controller ids had a pad at the menus' last poll (a change is a pad plugged in or out).
    pub pads_seen: [bool; crate::input::pad::SLOTS],
    /// The menus' online client and the net service under it (None: no `--online-config`).
    pub online: Option<std::rc::Rc<std::cell::RefCell<crate::online::hub::OnlineHub>>>,
    pub net: Option<mvs_net::service::NetHandle>,
    /// `--feed` with `--online-config`: a menus' online match's local inputs are that trace's.
    pub online_feed: Option<String>,
    want_ui: bool,
    want_audio: bool,
    audio_backend: Backend,
}

impl std::fmt::Debug for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shell").field("size", &self.swap.size).field("store", &self.renderer.is_some()).finish()
    }
}

impl Shell {
    /// A load is waited for at the frame that asks for it: a scripted run's (its frames and tape, for the
    /// compare), unless
    /// it runs with `--live-loads`. Else the frames run on while it loads on the loader.
    pub fn waits_loads(&self) -> bool {
        self.script.as_ref().is_some_and(|s| !s.live_loads)
    }

    /// The window, the GPU and its swap chain, the devices, the settings; the store's parts when `o.store` is given.
    pub fn new(o: &ShellOptions) -> Result<Box<Shell>> {
        let window = Window::new("MVS Classic", o.size, o.full_screen)?;
        let gpu = Gpu::new(o.adapter.as_deref(), false)?;
        let swap = SwapChain::new(&gpu, window.hwnd, window.size)?;
        let perf = Perf::new(&gpu, true);
        let settings = Settings::load(&o.data);
        let mut shell = Box::new(Shell {
            window,
            swap,
            gpu: Some(gpu),
            renderer: None,
            devices: Devices::default(),
            perf,
            audio: None,
            ui: None,
            apply: SettingsApply::new(&settings, &o.data),
            settings,
            data: o.data.clone(),
            online_defaults: o.online_defaults.clone(),
            session: Session::new(mvs_settings::stats::StatsFile::open(&o.data)),
            music: MenuMusic::default(),
            script: None,
            results_press: None,
            script_out: PathBuf::from("."),
            script_gate: crate::menus::ScriptGate::default(),
            real_pace: false,
            scenery_open: false,
            kept_level: None,
            pads_seen: [false; crate::input::pad::SLOTS],
            online: None,
            net: None,
            online_feed: None,
            want_ui: o.ui,
            want_audio: o.audio,
            audio_backend: if o.audio_offline { Backend::Offline } else { Backend::Wasapi },
        });
        if let Some(dir) = &o.store {
            shell.open_store(dir)?;
        }
        Ok(shell)
    }

    /// The online client's events and timers run (a screen that shows no menu: its view is kept for the next).
    pub fn online_poll(&mut self) {
        if let Some(h) = &self.online {
            let mut h = h.borrow_mut();
            let now = h.clock();
            h.poll(now);
        }
    }

    /// A scripted run's frame on a screen the script does not drive (`ScriptRun::passive`), drawn and not yet
    /// presented: the picture of it written to the script's `--out` when one is due.
    pub fn script_frame(&mut self, in_play: bool) -> Result<()> {
        if self.script.is_none() {
            return Ok(());
        }
        let renderer = self.renderer.as_ref().ok_or_else(|| Error::Match("no store is open".into()))?;
        let hub = self.online.as_ref().map(|h| h.borrow());
        script_frame(&mut self.script, &self.script_out, renderer, &self.swap, in_play, hub.as_deref())
    }

    /// The store at `dir` opened: the renderer on it (every pass), the sound engine, the UI's shared parts. Each part
    /// is built before the shell takes it: on any failure the shell keeps its GPU and no store, as before the call.
    pub fn open_store(&mut self, dir: &Path) -> Result<()> {
        let Some(gpu) = self.gpu.as_ref() else {
            return Err(Error::Match("a store is open already".into()));
        };
        eprintln!("mvs: {} on {}", dir.display(), gpu.adapter);
        // (one store for every part: its packs and its cache shared)
        let store = Store::open(dir)?;
        let assets = Assets::open(store.clone(), gpu)?;
        let audio = if self.want_audio {
            Some(Audio::new(Arc::new(store), self.audio_backend).map_err(|e| Error::Match(format!("audio: {e}")))?)
        } else {
            None
        };
        let gpu = self.gpu.take().expect("checked above");
        let mut renderer = match Renderer::try_new(gpu, assets, self.swap.size) {
            Ok(r) => r,
            Err(b) => {
                let (e, gpu, _) = *b;
                self.gpu = Some(gpu);
                return Err(e.into());
            }
        };
        renderer.passes = Passes::ALL;
        // (the main menu's backgrounds the home screen's switch offers; the chosen one reaches the renderer in
        // `apply_settings`)
        self.session.menu_backgrounds = renderer.menu_background_options().unwrap_or_else(|e| {
            eprintln!("mvs: the main menu's backgrounds: {e}: the home screen shows no background switch");
            Vec::new()
        });
        let ui = if self.want_ui {
            match ui_shared(&renderer, &self.settings) {
                Ok(u) => Some(u),
                Err(e) => {
                    self.gpu = Some(renderer.into_gpu());
                    return Err(e);
                }
            }
        } else {
            None
        };
        self.renderer = Some(renderer);
        self.audio = audio;
        // (the base scope, held from the sound engine's start)
        crate::audio::scope_base(&mut self.audio);
        self.ui = ui;
        // (the settings' scalability and volumes reach the renderer and the sound engine before the first picture)
        if let Err(e) = self.apply_settings() {
            self.close_store();
            return Err(e);
        }
        Ok(())
    }

    /// The open store closed: the sound engine, the UI's shared parts and the renderer dropped with their hold on
    /// the store's packs (an import switches them: Windows removes no pack still open), the GPU kept for the next
    /// `open_store`.
    pub fn close_store(&mut self) {
        self.audio = None;
        self.ui = None;
        self.apply.store_closed();
        if let Some(r) = self.renderer.take() {
            self.gpu = Some(r.into_gpu());
        }
    }

    /// The GPU (the renderer's once a store is open).
    pub fn gpu(&self) -> &Gpu {
        match (&self.gpu, &self.renderer) {
            (Some(g), _) => g,
            (None, Some(r)) => r.gpu(),
            (None, None) => unreachable!("the GPU is the shell's or its renderer's"),
        }
    }

    /// The open store.
    pub fn store(&self) -> Option<&Store> {
        self.renderer.as_ref().map(|r| r.assets().store())
    }

    /// The frame's start: waits for the swap chain to take a frame (latency 1), then for the frame rate limit.
    pub fn wait(&mut self) {
        self.swap.wait(1000);
    }

    /// The swap chain and the renderer's targets at the window's new size.
    pub fn resize(&mut self, size: [u32; 2]) -> Result<()> {
        match (&self.gpu, &mut self.renderer) {
            (Some(g), _) => self.swap.resize(g, size)?,
            (None, Some(r)) => {
                self.swap.resize(r.gpu(), size)?;
                r.resize(size)?;
            }
            (None, None) => unreachable!("the GPU is the shell's or its renderer's"),
        }
        Ok(())
    }
}

/// One frame's input to the screen that runs it.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// The real time since the last frame, s.
    pub dt: f64,
    /// The window's new client size when it changed this frame (the shell's swap chain and renderer resized already).
    pub resized: Option<[u32; 2]>,
    /// When the frame's work began (before the window's messages).
    pub cpu_start: Instant,
}

/// A Lab match's state carried through its restart, and the custom lobby a custom match returns to.
pub type LobbyKept = LocalLobby;

/// What kind of match runs, and what it keeps for its end.
#[derive(Debug, Default)]
pub enum MatchKind {
    /// No menu picked it: `default_setup` or the command line's.
    #[default]
    Default,
    /// A trace match whose set-up, seed and inputs drive the loop (`--replay`); its Rust trace written with
    /// `--trace-out`.
    Replay { name: String },
    /// The Lab: a restart's carry, None for the Lab's first match.
    Lab { carry: Option<Box<LabCarry>> },
    /// A custom lobby's match: the lobby the match returns to.
    Custom { lobby: Box<LobbyKept> },
    /// Local Play: back to the main menu at its end.
    LocalPlay,
    /// An arcade round: the run in the session, advanced at its end.
    Arcade,
    /// An online match (the command line's): its tick source, taken by the match loop.
    Online { net: Option<Box<crate::online::Online>> },
    /// A tutorial (Map_TrophyRoom_Tutorial's TutorialGameMode): its widgets over
    /// the match, back to the main menu at its end screen.
    Tutorial,
}

/// A match to load and run (a lobby's match, the Lab's first set-up).
#[derive(Debug)]
pub struct MatchStart {
    pub setup: MatchSetup,
    pub kind: MatchKind,
    /// The World's seed (a replay's: its trace's).
    pub seed: i32,
    /// The stage's key (StageName): the trace stage's unless a map, `--stage` or the host's set-up names another.
    pub stage: String,
    /// The match's MapData (None: a dev room on `stage`).
    pub map: Option<String>,
    /// The local player.
    pub me: usize,
    /// Each player's name ('' where none is given).
    pub usernames: Vec<String>,
    /// The local humans: (player slot, device, binding set) each (Local Play); None: `me` alone on the keyboard and
    /// pad 0.
    pub humans: Option<Vec<(usize, DeviceKind, usize)>>,
    /// Its parts built on a loader thread (`crate::prepare`): taken when their key is this start's.
    pub prepared: Option<Box<crate::prepare::Prepared>>,
    /// A new Lab's menu built on the loading screen (`crate::lab_host::LabAhead`).
    pub lab_ahead: Option<Box<crate::lab_host::LabAhead>>,
}

impl MatchStart {
    /// A match on `setup` in the trace stage, `me` 0 with the keyboard and pad 0.
    pub fn new(setup: MatchSetup, kind: MatchKind, seed: i32) -> MatchStart {
        MatchStart {
            setup,
            kind,
            seed,
            stage: mvs_sim_trace::TRACE_STAGE.to_owned(),
            map: None,
            me: 0,
            usernames: Vec::new(),
            humans: None,
            prepared: None,
            lab_ahead: None,
        }
    }

    /// The match no menu picks (`default_setup`).
    pub fn default_match(seed: i32) -> MatchStart {
        MatchStart::new(default_setup(), MatchKind::Default, seed)
    }

    /// A trace match replayed (its set-up and seed are the trace's).
    pub fn replay(name: &str) -> MatchStart {
        MatchStart::new(MatchSetup::default(), MatchKind::Replay { name: name.to_owned() }, 0)
    }
}

/// How a match ended: where the program goes next.
#[derive(Debug)]
pub enum MatchEnd {
    /// The main menu.
    Home,
    /// Back to the custom lobby it started from.
    Lobby(Box<LobbyKept>),
    /// The Lab's Spawn All Players on another roster: the match made anew on the carry.
    LabRestart(Box<LabCarry>),
    /// The results screen over the podium, then the end it holds.
    Results(Box<crate::results::ResultsStart>),
    /// The window closed.
    Quit,
}

/// Where the menus open (the return from a match).
#[derive(Debug, Default)]
pub struct MenuReturn {
    /// The custom lobby to show again.
    pub lobby: Option<Box<LobbyKept>>,
}

/// A screen's answer at the end of its frame.
#[derive(Debug)]
pub enum Go {
    Stay,
    /// The title screen (UI_Welcome).
    Title,
    /// The menus.
    Menu(MenuReturn),
    /// The pre-match screen over the PreMatch level (a custom lobby's Start).
    PreMatch(Box<mvs_ui::menu::screens::pre_match::setup::PreMatchStart>),
    /// The loading screen, then the match.
    Load(Box<MatchStart>),
    /// The match at once (no loading screen).
    Match(Box<MatchStart>),
    /// The results screen over the podium after a match.
    Results(Box<crate::results::ResultsStart>),
    Quit,
}

/// The screen running.
pub enum State {
    Title(Box<MenuHost>),
    Menu(Box<MenuHost>),
    Loading(Box<Loading>),
    Match(Box<MatchLoop>),
    Results(Box<crate::results::ResultsHost>),
}

impl State {
    /// The screen's frame.
    fn frame(&mut self, shell: &mut Shell, f: &Frame) -> Result<StateOut> {
        // (the online client runs under every screen: the menus take its view, the others its timers alone)
        if !matches!(self, State::Title(_) | State::Menu(_)) {
            shell.online_poll();
        }
        Ok(match self {
            State::Title(m) | State::Menu(m) => StateOut::Go(m.frame(shell, f)?),
            State::Loading(l) => StateOut::Go(l.frame(shell, f)?),
            State::Match(m) => m.frame(shell, f)?.map_or(StateOut::Go(Go::Stay), StateOut::End),
            State::Results(r) => StateOut::Go(r.frame(shell, f)?),
        })
    }

    /// The program leaves while the screen runs (the window closed, a timed run's end).
    fn leave(&mut self, shell: &mut Shell) -> Result<()> {
        match self {
            State::Match(m) => m.end(shell),
            State::Title(_) | State::Menu(_) | State::Loading(_) | State::Results(_) => Ok(()),
        }
    }
}

enum StateOut {
    Go(Go),
    End(MatchEnd),
}

/// How the program runs.
#[derive(Debug, Default)]
pub struct RunOptions {
    /// The first screen's: a match's shots, replay pace and throttle (the later matches take the defaults).
    pub first: MatchOptions,
    /// `--press-results` (`Shell::results_press`).
    pub results_press: Option<f64>,
    /// One match and out (the command line's match): every match end quits.
    pub once: bool,
    /// `--bench <secs>`: the frames counted from the first drawn one for that long, one `mvs: bench` line, out.
    pub bench: Option<f64>,
    /// `--input-script`, `--menu-tape`, `--shot-frames`: a scripted menu run (script.rs).
    pub script: crate::script::ScriptOptions,
    /// `--relay`, `--host`, `--join`, `--key`: an online match first (None: none).
    pub online: Option<crate::online::OnlineOptions>,
}

/// The program: the shell and its screen.
pub struct App {
    /// Boxed: the shell is a large value, and the program's start-up frames of a debug build each held copies of it.
    pub shell: Box<Shell>,
    state: State,
    once: bool,
}

impl App {
    /// The first screen opened on the shell.
    pub fn new(mut shell: Box<Shell>, first: Go, o: &RunOptions) -> Result<App> {
        shell.script = crate::script::ScriptRun::new(&o.script, shell.swap.size)?;
        shell.results_press = o.results_press;
        if let Some(s) = &shell.script {
            shell.script_out = o.first.out.clone();
            shell.real_pace = o.first.real_pace;
            shell.devices.script_pads(None);
            shell.script_gate = crate::menus::ScriptGate::new(s.player.script().start == crate::script::Start::Menu);
        }
        let state = match first {
            Go::Match(start) => State::Match(Box::new(MatchLoop::new(&mut shell, *start, &o.first)?)),
            go => enter(&mut shell, go)?.ok_or_else(|| Error::Match("no first screen".into()))?,
        };
        Ok(App { shell, state, once: o.once })
    }

    /// Frames until the program quits.
    pub fn run(&mut self, o: &RunOptions) -> Result<()> {
        let clock = Instant::now();
        let mut last = 0.0;
        // (the screen's first frame: its load is not caught up)
        let mut first = true;
        let mut bench_start = None;
        // (--real-pace on a script: the due time of the frame, seconds)
        let mut due = clock.elapsed().as_secs_f64();
        let mut notes = crate::audio::NoteLog::default();
        // (a scripted run's frames: the online client's clock, 60 a second)
        let mut frames = 0u32;
        let bench_line = |shell: &Shell| {
            if let Some(t) = &shell.perf.totals {
                eprintln!("mvs: bench {}", t.line());
            }
        };
        loop {
            self.shell.wait();
            let now = clock.elapsed().as_secs_f64();
            let cpu_start = Instant::now();
            let mut resized = None;
            if !self.shell.window.pump(&mut self.shell.devices, &mut resized) {
                self.state.leave(&mut self.shell)?;
                bench_line(&self.shell);
                return Ok(());
            }
            if let Some(size) = resized {
                self.shell.resize(size)?;
            }
            // (the window's pointer follows the mouse whatever shows; no screen has owned it or taken input yet. A
            // scripted run takes no real mouse or keyboard: its script drives the screens.)
            let scripted = self.shell.script.is_some();
            if let Some(ui) = self.shell.ui.as_mut() {
                ui.pointer.begin_frame();
                if !scripted {
                    ui.pointer.window_moves(self.shell.window.menu_input());
                }
            }
            // (a scripted run over the net service: real time, its frames at most 60 a second, so the script's frames
            // and the host core's clock keep together; else the fake queue's clock is the script's frames)
            let paced = self.shell.script.is_some() && self.shell.net.is_some();
            if paced {
                // Deviation: the port's own pace of a scripted run with --online-config (the two-exe runs).
                let left = SCRIPT_FRAME_S - cpu_start.elapsed().as_secs_f64();
                if left > 0.0 {
                    std::thread::sleep(std::time::Duration::from_secs_f64(left));
                }
            } else if self.shell.script.is_some() && self.shell.real_pace {
                // A scripted run at real time (--real-pace): frame n is due at n / 60 s on the wall clock (the
                // script's own frame counter is unchanged), the present synced.
                due += SCRIPT_FRAME_S;
                let late = clock.elapsed().as_secs_f64() - due;
                if late > 0.25 {
                    // (a load or a stall: the schedule restarts here, not a burst of frames to catch up)
                    due += late;
                }
                loop {
                    let left = due - clock.elapsed().as_secs_f64();
                    if left <= 0.0 {
                        break;
                    }
                    if left > 0.002 {
                        std::thread::sleep(std::time::Duration::from_secs_f64(left - 0.002));
                    } else {
                        std::hint::spin_loop();
                    }
                }
            } else if self.shell.script.is_some()
                && let Some(h) = &self.shell.online
            {
                h.borrow_mut().script_clock = Some(f64::from(frames) * 1000.0 / 60.0);
            }
            frames += 1;
            let out = self.state.frame(&mut self.shell, &Frame { dt: now - last, resized, cpu_start })?;
            // (the device the player last touched: an active screen that took the frame's input wrote it, else the
            // window's touches do, a match's play and the title's included; the input no screen took is dropped)
            let touches = self.shell.devices.take_touches();
            if let Some(ui) = self.shell.ui.as_mut()
                && !ui.pointer.input_taken()
                && !scripted
            {
                ui.pointer.touched(&touches);
            }
            // (the game's cursor moved the pointer: the OS pointer goes there, SetMouseLocation, and a screen measures
            // the next mouse move from it. A scripted run leaves the user's pointer alone.)
            if let Some(ui) = self.shell.ui.as_mut()
                && let Some(at) = ui.pointer.take_warp()
                && !scripted
                && let Some(moved) = self.shell.window.warp_pointer(at)
            {
                ui.pointer.mouse = Some(moved);
            }
            self.shell.window.take_menu_input();
            // (every screen's sound, the menus' and the match's: once a frame)
            crate::audio::drain_notes(&mut self.shell.audio, &mut notes);
            if let Some(s) = &mut self.shell.script {
                s.player.frame_passed().map_err(Error::Match)?;
            }
            // (the settings the screen's frame wrote, applied: apply.rs)
            self.shell.apply_settings()?;
            last = now;
            // (a script's end on a screen it does not drive: the menus end theirs themselves)
            let script_over = !matches!(self.state, State::Title(_) | State::Menu(_))
                && self.shell.script.as_ref().is_some_and(crate::script::ScriptRun::done);
            if script_over && let Some(s) = self.shell.script.take() {
                s.finish()?;
                self.state.leave(&mut self.shell)?;
                bench_line(&self.shell);
                return Ok(());
            }
            // (a local online match's end: its result and match.leave, OnlineHub::local_over)
            if let (State::Match(_), StateOut::End(end), Some(h)) = (&self.state, &out, &self.shell.online) {
                let podium = match end {
                    MatchEnd::Results(s) => Some(&s.eog.players[..]),
                    _ => None,
                };
                h.borrow_mut().local_over(podium);
            }
            // (the return path: a match left with no results and the queue it ran beside; the Lab's way back to
            // the queue's pre-match screen)
            let back = match (&self.state, &out, &self.shell.online) {
                (State::Match(_), StateOut::End(MatchEnd::Home | MatchEnd::Quit), Some(h)) => {
                    let mut h = h.borrow_mut();
                    // (a match left with no results: its set's end session closed)
                    h.close_queue_end();
                    let now = h.clock();
                    match h.lab_return(now) {
                        crate::online::hub::LabReturn::PreMatch { modes, resume } => {
                            Some(Box::new(crate::menus::queue_pre_match(&h, &self.shell.settings, modes, Some(resume))))
                        }
                        crate::online::hub::LabReturn::Note(n) => {
                            self.shell.session.main_menu.note = mvs_ui::menu::online_text::kick_text(&n);
                            None
                        }
                        crate::online::hub::LabReturn::None => None,
                    }
                }
                _ => None,
            };
            let out = match back {
                Some(start) if matches!(out, StateOut::End(MatchEnd::Home)) => StateOut::Go(Go::PreMatch(start)),
                _ => out,
            };
            let go = match out {
                // (one match and out: the results screen's end quits)
                StateOut::Go(go) if self.once && matches!(self.state, State::Results(_)) && !matches!(go, Go::Stay) => {
                    Go::Quit
                }
                StateOut::Go(go) => go,
                StateOut::End(MatchEnd::Results(start)) => Go::Results(start),
                StateOut::End(_) if self.once => Go::Quit,
                StateOut::End(end) => go_of(&self.shell, end),
            };
            match go {
                Go::Stay => {}
                Go::Quit => {
                    bench_line(&self.shell);
                    return Ok(());
                }
                go => {
                    // (after a match and its results: the stage's music is let go as it plays out under
                    // the menu's)
                    if matches!(self.state, State::Match(_) | State::Results(_))
                        && matches!(go, Go::Menu(_) | Go::PreMatch(_))
                    {
                        crate::audio::release_stage_music(&mut self.shell.audio);
                    }
                    if let Some(s) = enter(&mut self.shell, go)? {
                        self.state = s;
                    }
                    first = true;
                    last = clock.elapsed().as_secs_f64();
                    continue;
                }
            }
            if first {
                // The screen's clock starts after its first frame's draw and present (after the
                // warm-up): that frame's lazy work is not caught up in steps.
                first = false;
                last = clock.elapsed().as_secs_f64();
                if o.bench.is_some() && bench_start.is_none() {
                    self.shell.perf.totals = Some(Totals::default());
                    bench_start = Some(last);
                }
            }
            self.shell.perf.frame(clock.elapsed().as_secs_f64());
            if let (Some(secs), Some(s)) = (o.bench, bench_start)
                && clock.elapsed().as_secs_f64() - s >= secs
            {
                bench_line(&self.shell);
                self.state.leave(&mut self.shell)?;
                return Ok(());
            }
            if self.shell.ui.is_none() {
                // Without the UI the perf line is the window's title.
                if let Some(r) = self.shell.perf.readout.take() {
                    self.shell.window.set_title(&format!("MVS Classic  {}", r.line()));
                }
            }
        }
    }
}

/// The screen a match's end goes to.
pub(crate) fn go_of(shell: &Shell, end: MatchEnd) -> Go {
    match end {
        MatchEnd::Home => Go::Menu(MenuReturn::default()),
        MatchEnd::Lobby(lobby) => Go::Menu(MenuReturn { lobby: Some(lobby) }),
        MatchEnd::LabRestart(carry) => Go::Match(Box::new(crate::lab_setup::restart(shell, *carry))),
        MatchEnd::Results(start) => Go::Results(start),
        MatchEnd::Quit => Go::Quit,
    }
}

/// `Shell::script_frame` on the shell's parts, for a screen that holds them apart: the back buffer of `swap` drawn,
/// not yet presented.
pub fn script_frame(
    script: &mut Option<ScriptRun>,
    out: &Path,
    renderer: &Renderer,
    swap: &SwapChain,
    in_play: bool,
    hub: Option<&crate::online::hub::OnlineHub>,
) -> Result<()> {
    let Some(s) = script else { return Ok(()) };
    let Some(n) = s.passive(in_play, hub) else { return Ok(()) };
    let px = renderer.read_back(swap.back_buffer())?;
    let [w, h] = swap.size;
    let path = out.join(format!("{}-f{n}.png", s.player.script().name));
    mvs_render::image::write_png(&path, w, h, &px).map_err(Error::Match)?;
    eprintln!("mvs: {}", path.display());
    Ok(())
}

/// The screen `go` opens (None: Stay and Quit open none).
fn enter(shell: &mut Shell, go: Go) -> Result<Option<State>> {
    Ok(Some(match go {
        Go::Stay | Go::Quit => return Ok(None),
        Go::Title => State::Title(Box::new(MenuHost::title(shell)?)),
        Go::Menu(ret) => State::Menu(Box::new(MenuHost::new(shell, ret)?)),
        Go::PreMatch(start) => State::Menu(Box::new(MenuHost::pre_match(shell, *start)?)),
        Go::Load(start) => State::Loading(Box::new(Loading::new(shell, *start)?)),
        Go::Match(start) => State::Match(Box::new(MatchLoop::new(shell, *start, &MatchOptions::default())?)),
        Go::Results(start) => State::Results(Box::new(crate::results::ResultsHost::new(shell, *start)?)),
    }))
}

/// The UI's shared parts on a renderer's GPU and store; the key bindings the settings saved.
fn ui_shared(renderer: &Renderer, settings: &Settings) -> Result<UiShared> {
    let (gpu, store) = (renderer.gpu(), renderer.assets().store());
    let menu = Bundle::load(store, "menu")?;
    let menu_fonts = Fonts::new(store, &menu)?;
    let defaults: mvs_data::InputDefaults = mvs_data::entry(&*store.kind(mvs_data::KIND)?, mvs_data::path::INPUT)?;
    let mut ui = Ui::new(gpu, renderer.assets())?;
    // (the menus', HUD's and results' pictures load off the frame thread: an element shows once its picture is made;
    // each screen turns it off for a frame whose picture is written: a held clock, a shot, an unpaced script)
    ui.stream_pictures = true;
    Ok(UiShared {
        ui,
        menu: Arc::new(menu),
        menu_fonts,
        text: Arc::new(GameText::load(store)?),
        list: ElementList::default(),
        timer: GpuTimer::new(gpu, 2)?,
        bindings: Bindings::load(&defaults, settings.input.bindings.as_ref()),
        pointer: Pointer::default(),
    })
}
