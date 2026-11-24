//! The launcher (app/launcher/launcher.ts), a program of its own (`mvs-launcher.exe`, host.rs): the splash, the
//! setup steps (the install location, the beta's folder, the build), Home with its Play button and its rebuild, the
//! settings overlay; drawn by `mvs_ui::launcher` in its own window. Its asks of the import (`state`, `detect`,
//! `check`) and its runs go to the import worker (`crate::import`), one process each, polled each frame. Play
//! starts the game's program on the store (game.rs) and Home shows it running until it ends (window.ts: the
//! launcher stays open beside the game). The updater (`crate::update`) checks the feed when the launcher opens and
//! the splash waits for it a moment; an update found is offered on Home, downloaded when the player asks and handed
//! over to, and the launcher quits.
//!
//! Deviation: no Account tab in the settings (the Online tab: online.rs), no stats export.

pub mod game;
pub mod host;
pub mod link;
pub mod net_test;
pub mod notes;
pub mod online;
pub mod setup_os;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Instant;

use mvs_ui::launcher::import_view::{
    CountLine, GroupName, ImportEvent, ImportState, ImportView, LogLevel, Say, SourceInfo, SourceKind, Why,
};
use mvs_ui::launcher::notes::PICTURES;
use mvs_ui::launcher::{
    Action, Build, HomeState, LauncherUi, LogLine, Model, Phase, PhaseState, Screen, Source, Tone, clock_at, clock_now,
    words as w,
};

use crate::Result;
use crate::import::{ImportRequest, Worker, WorkerOp};
use crate::update::{UpdateEvent, Updater};
use game::Game;
use host::Host;
use setup_os::{ShortcutKind, store_dir};

/// The launcher log's lines kept (launcher.ts LOG_KEEP).
const LOG_KEEP: usize = 400;
/// The splash's least time, s (launcher.ts SPLASH_MIN_MS).
const SPLASH_MIN: f64 = 0.7;
/// The splash waits for the update check at most this long, s (launcher.ts UPDATE_WAIT_MS).
const UPDATE_WAIT: f64 = 4.0;
/// Home's picture (launcher.ts HOME_PICTURE): the night Treehouse.
const HOME_PICTURE: &str =
    "/Game/Panda_Main/PreMatch/Maps/MapBG_TreeFort2v2SinglePlatform.MapBG_TreeFort2v2SinglePlatform";
/// The store's kind that holds Home's picture.
// Deviation: the TS read it from a `picture` kind of its own; this store keeps it as an entry of the texture kind
// (GPU-ready), so that kind is the one read and the one whose staleness skips the picture.
const PICTURE_KIND: &str = "texture";

/// What the launcher's frame asks of its program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    Stay,
    /// The window's Close, an update handed over to, or `--play`'s game started.
    Quit,
}

/// What a state answer goes on to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Then {
    /// The screen the state asks for (launcher.ts route), once the splash stood its time; with `--play` the game at
    /// once when it is ready.
    Boot,
    Route,
    Home,
    /// A run ended well: Play when it was asked and the store is now current (runEnded).
    Ended {
        then_play: bool,
    },
    /// Setup's step 1 again, on the state of a new install location (launcher.ts stepInstall's refresh).
    Install,
    /// Only the facts refreshed.
    Nothing,
}

/// An op of the worker waiting for its answer.
#[derive(Debug)]
enum Ask {
    State(Then),
    Detect,
    /// A folder the player picked.
    Check,
}

/// Home's picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Picture {
    None,
    /// Its bytes being read off the frame's thread.
    Loading,
    On,
    /// Asked for again only after a run.
    Failed,
}

/// A note picture's entry read: `PICTURES`' index and its bytes.
type NotePicture = (usize, std::result::Result<Vec<u8>, String>);

/// The launcher running.
pub struct Launcher {
    ui: LauncherUi,
    m: Model,
    clock: Instant,
    state: Option<ImportState>,
    asks: Vec<(Ask, Worker)>,
    run: Option<(Worker, ImportView)>,
    /// The usable paks folder chosen at step 2.
    chosen: Option<PathBuf>,
    /// Step 2's folders, checked.
    found: Vec<SourceInfo>,
    /// The boot's route waits for the splash's least time.
    boot: Option<Then>,
    /// The game started from PLAY, while it runs (launcher.ts `game`).
    game: Option<Game>,
    picture: Picture,
    /// The picture's entry being read.
    picture_load: Option<Receiver<std::result::Result<Vec<u8>, String>>>,
    /// The releases the patch notes show (the latest first), and their list being read from the feed.
    notes: Vec<mvs_release::ReleaseNotes>,
    notes_load: Option<Receiver<std::result::Result<Vec<mvs_release::ReleaseNotes>, String>>>,
    /// The notes' pictures asked of the store (`PICTURES`' indices) and their entries being read.
    note_pictures: Vec<usize>,
    note_picture_loads: Vec<Receiver<NotePicture>>,
    /// The pointer's left button last frame.
    was_down: bool,
    /// The Online tab's look at a typed relay address and online.json's values.
    online: Box<online::OnlineTab>,
    update: Updater,
    /// The launcher's own view of the update (launcher.ts `update`): the check at the start is not answered yet
    /// (the splash waits for it), a download goes on, the player asked for this check (the menu's).
    checking: bool,
    updating: bool,
    update_asked: bool,
    /// The answer to the menu's check, on Home's detail line while it is ready ("" none): the log is not drawn.
    update_note: String,
}

impl std::fmt::Debug for Launcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Launcher").field("screen", &self.m.screen).finish_non_exhaustive()
    }
}

fn tone(l: LogLevel) -> Tone {
    match l {
        LogLevel::Info => Tone::Plain,
        LogLevel::Warn => Tone::Warn,
        LogLevel::Error => Tone::Bad,
    }
}

fn source(s: &SourceInfo) -> Source {
    let path = if s.paks.as_os_str().is_empty() { &s.dir } else { &s.paks };
    Source { note: s.note.clone(), path: path.display().to_string(), paks: s.paks.clone(), usable: usable(s) }
}

/// The open beta with its key: it can be imported (launcher.ts sourceItem's usable).
fn usable(s: &SourceInfo) -> bool {
    s.kind == SourceKind::Beta && s.key_ok
}

impl Launcher {
    /// The launcher at its splash, its state asked, the update feed checked.
    pub fn new(host: &mut Host) -> Result<Launcher> {
        let ui = LauncherUi::new(&host.gpu)?;
        let update = Updater::new(mvs_release::FEED, None);
        let mut l = Launcher {
            ui,
            m: Model::default(),
            clock: Instant::now(),
            state: None,
            asks: Vec::new(),
            run: None,
            chosen: None,
            found: Vec::new(),
            boot: None,
            game: None,
            picture: Picture::None,
            picture_load: None,
            notes: notes::kept(&host.data),
            notes_load: None,
            note_pictures: Vec::new(),
            note_picture_loads: Vec::new(),
            was_down: false,
            online: Box::new(online::OnlineTab::new(&host.online_defaults)),
            updating: false,
            update,
            checking: false,
            update_asked: false,
            update_note: String::new(),
        };
        l.log(w::LAUNCHER_OPENED, Tone::Plain, w::EM_STARTED);
        l.m.app_problem = setup_os::app_problem().map(str::to_string);
        if let Some(p) = setup_os::app_problem() {
            l.log(p, Tone::Warn, "");
        }
        if l.update.enabled() {
            l.checking = true;
            l.m.checking = true;
            l.update.check(mvs_release::VERSION);
        }
        l.m.notes = notes::model(&l.notes, mvs_release::VERSION);
        l.fetch_notes(host);
        // (join links open this launcher; a developer's or a test's run, `--store`, `--data` or the dev tree's
        // `.dev` data folder, leaves them alone)
        if host.store.is_none()
            && host.game.data.is_none()
            && host.data.file_name().is_none_or(|n| n != ".dev")
            && let Err(e) = link::register()
        {
            l.log(&w::link_not_registered(&e), Tone::Warn, "");
        }
        l.ask_state(host, Then::Boot);
        Ok(l)
    }

    fn now(&self) -> f64 {
        self.clock.elapsed().as_secs_f64()
    }

    /// A line of the launcher log; `em` a word shown in the level's colour before the text (launcher.ts log).
    fn log(&mut self, text: &str, tone: Tone, em: &str) {
        self.m.log.push(LogLine { time: clock_now(), tone, em: em.to_string(), text: text.to_string() });
        if self.m.log.len() > LOG_KEEP {
            let over = self.m.log.len() - LOG_KEEP;
            self.m.log.drain(..over);
        }
    }

    /// The menu's update check said this: Home's detail line shows it, Home drawn again when it is up.
    fn note_update(&mut self, host: &mut Host, note: String) {
        self.update_note = note;
        if self.m.screen == Screen::Home {
            self.home(host);
        }
    }

    fn root(host: &Host) -> PathBuf {
        setup_os::data_root(&host.settings, &host.data)
    }

    /// The store the game plays: `--store`'s, else the data root's.
    fn store(host: &Host) -> PathBuf {
        host.store.clone().unwrap_or_else(|| store_dir(&Self::root(host)))
    }

    fn request(host: &Host) -> ImportRequest {
        ImportRequest { data_root: Self::root(host), ..Default::default() }
    }

    fn ask(&mut self, op: WorkerOp, req: &ImportRequest, ask: Ask) {
        match Worker::start(op, req) {
            Ok(wk) => self.asks.push((ask, wk)),
            Err(e) => self.answer(None, ask, &ImportEvent::Failed { say: Say::Error { detail: e.to_string() } }),
        }
    }

    fn ask_state(&mut self, host: &mut Host, then: Then) {
        // (a run holds the store: its last answer stands meanwhile and goes on to its own next step, setup.ts
        // importState)
        if self.run.is_some() {
            self.then(Some(host), then);
            return;
        }
        // (`--store`: a store imported already, played as it is: setup holds and every kind is current)
        if host.store.is_some() {
            self.state = Some(ImportState { setup: true, current: true, ..Default::default() });
            self.facts(host);
            self.then(Some(host), then);
            return;
        }
        self.ask(WorkerOp::State, &Self::request(host), Ask::State(then));
    }

    /// Whether the game can open now: setup done, the store current, no run (setup.ts storeReady).
    fn ready(&self) -> bool {
        self.run.is_none()
            && self.state.as_ref().is_some_and(|s| s.setup && s.stale.is_empty() && !s.new_build && s.current)
    }

    // -----------------------------------------------------------------------------------------------------------
    // The answers

    /// An op's answer (`host` None: the op did not start, nothing more to do than its failure).
    fn answer(&mut self, host: Option<&mut Host>, ask: Ask, e: &ImportEvent) {
        match (ask, e) {
            (Ask::State(then), ImportEvent::State { state }) => {
                self.state = Some(state.clone());
                if let Some(h) = &host {
                    self.facts(h);
                }
                self.then(host, then);
            }
            (Ask::State(then), ImportEvent::Failed { say }) => {
                self.log(&w::say(say), Tone::Bad, w::EM_ERROR);
                if self.state.is_none() {
                    self.state = Some(ImportState { why: Some(Why::SetupNotDone), ..Default::default() });
                }
                if let Some(h) = &host {
                    self.facts(h);
                }
                self.then(host, then);
            }
            (Ask::Detect, ImportEvent::Detected { found }) => {
                self.found = found.iter().map(|c| c.source.clone()).collect();
                self.m.beta.sources = Some(self.found.iter().map(source).collect());
                // (the first usable folder found is chosen)
                if let Some(first) = self.found.iter().find(|s| usable(s)).cloned() {
                    self.choose(&first);
                }
            }
            (Ask::Detect, ImportEvent::Failed { say }) => {
                self.m.beta.sources = Some(Vec::new());
                self.log(&w::say(say), Tone::Warn, "");
            }
            (Ask::Check, ImportEvent::Source { source: s }) => {
                self.found.insert(0, s.clone());
                self.m.beta.sources.get_or_insert_with(Vec::new).insert(0, source(s));
                if usable(s) {
                    self.choose(s);
                } else {
                    let dir = s.dir.display().to_string();
                    self.log(&dir, Tone::Warn, &s.note.clone());
                }
            }
            (Ask::Check, ImportEvent::Failed { say }) => self.log(&w::say(say), Tone::Warn, ""),
            _ => {}
        }
    }

    /// What a state answer goes on to.
    fn then(&mut self, host: Option<&mut Host>, then: Then) {
        let Some(host) = host else {
            // (the op did not start: the next frame routes)
            if then != Then::Nothing {
                self.boot = Some(Then::Route);
            }
            return;
        };
        match then {
            // (the frame routes it once the splash stood its time and the update check answered)
            Then::Boot => self.boot = Some(Then::Boot),
            Then::Route => self.route(host),
            Then::Home => self.home(host),
            Then::Install => self.step_install(host),
            Then::Ended { then_play } => {
                if then_play && self.ready() {
                    self.boot = Some(Then::Ended { then_play });
                } else {
                    self.home(host);
                }
            }
            Then::Nothing => {
                if self.m.screen == Screen::Home {
                    self.home(host);
                }
            }
        }
    }

    /// Setup's build while setup's run goes on, setup when it was never done, Home otherwise (launcher.ts route).
    fn route(&mut self, host: &mut Host) {
        let s = self.state.clone().unwrap_or_default();
        if self.run.is_some() && !s.setup {
            self.m.screen = Screen::Build;
            self.draw_run();
            return;
        }
        if !s.setup && s.why == Some(Why::SetupNotDone) {
            self.step_install(host);
        } else {
            self.home(host);
        }
    }

    // -----------------------------------------------------------------------------------------------------------
    // Setup

    /// Step 1: the install location (launcher.ts stepInstall, showRoot).
    fn step_install(&mut self, host: &Host) {
        self.m.screen = Screen::Install;
        let root = Self::root(host);
        let i = &mut self.m.install;
        i.root = root.display().to_string();
        i.can_default = host.settings.launcher.data_root.as_ref().is_some_and(|c| *c != host.data);
        i.can_cancel = self.state.as_ref().is_some_and(|s| s.setup);
        i.problem = None;
        match setup_os::space(&root) {
            Ok(p) => {
                let ok = p.free >= p.need && !p.blocked;
                i.problem =
                    p.problem.map(|t| mvs_ui::launcher::Note::new(t, if p.blocked { Tone::Bad } else { Tone::Warn }));
                i.space = Some(if ok {
                    mvs_ui::launcher::Note::new(w::space_ok(&w::gb(p.free)), Tone::Ok)
                } else {
                    mvs_ui::launcher::Note::new(w::space_short(&w::gb(p.free), &w::gb(p.need)), Tone::Bad)
                });
                i.can_continue = ok;
            }
            Err(e) => {
                i.space = Some(mvs_ui::launcher::Note::new(w::folder_unusable(&e), Tone::Bad));
                i.can_continue = false;
            }
        }
    }

    /// Step 2: the beta's folder; the first usable folder found is chosen (launcher.ts stepBeta).
    fn step_beta(&mut self, host: &Host) {
        self.m.screen = Screen::Beta;
        let s = self.state.clone().unwrap_or_default();
        self.m.beta.why = s.why.filter(|why| !s.setup && *why != Why::SetupNotDone).map(|why| w::why(&why));
        self.chosen = None;
        self.m.beta.chosen = None;
        self.m.beta.sources = None;
        self.found.clear();
        let last = s.beta_dir.or_else(|| host.settings.launcher.beta_dir.clone());
        let req = ImportRequest { last, ..Self::request(host) };
        self.asks.retain(|(a, _)| !matches!(a, Ask::Detect));
        self.ask(WorkerOp::Detect, &req, Ask::Detect);
    }

    /// A usable folder chosen (launcher.ts choose).
    fn choose(&mut self, s: &SourceInfo) {
        let paks = s.paks.display().to_string();
        self.log(&paks, Tone::Ok, w::EM_FOUND);
        self.chosen = Some(s.paks.clone());
        self.m.beta.chosen = Some(s.paks.clone());
    }

    // -----------------------------------------------------------------------------------------------------------
    // Runs

    /// A run started: setup's step 3, else Home's progress line (launcher.ts startImport). Not while the game runs
    /// (its store's packs are open: window.ts gameRunning), nor on `--store`'s store.
    fn start_run(&mut self, host: &mut Host, setup: bool, then_play: bool, force: bool) {
        // (one run at a time: setup.ts startImport)
        if self.run.is_some() {
            return;
        }
        if self.game_running(host) {
            self.log(w::GAME_RUNNING, Tone::Warn, "");
            return;
        }
        if host.store.is_some() {
            self.log(w::STORE_GIVEN, Tone::Warn, "");
            return;
        }
        let root = Self::root(host);
        // a rebuild writes each new pack before the old one goes: the drive must hold the stale kinds' packs too
        if !setup && let Some(s) = &self.state {
            let all = force || s.new_build;
            if let Some((free, need)) = setup_os::rebuild_short(&root, &s.stale, all) {
                self.log(&w::rebuild_space_short(&w::gb(free), &w::gb(need)), Tone::Bad, "");
                return;
            }
        }
        self.ui.set_picture(None);
        self.picture = Picture::None;
        self.picture_load = None;
        for k in 0..PICTURES.len() {
            self.ui.set_note_picture(k, None);
        }
        self.note_pictures.clear();
        self.note_picture_loads.clear();
        if setup && let Some(p) = &self.chosen {
            host.settings.launcher.beta_dir = Some(p.clone());
            if let Err(e) = host.save_settings() {
                self.log(&e.to_string(), Tone::Warn, "");
            }
        }
        let view = ImportView::new(setup, then_play, force, self.state.as_ref(), Some(root.clone()), self.now());
        let what = if setup {
            w::FROM_BETA.to_string()
        } else if force {
            w::WHOLE_GAME.to_string()
        } else {
            w::plural(view.kinds.len(), "part")
        };
        self.log(&what, Tone::Plain, if setup { w::EM_BUILDING } else { w::EM_REBUILDING });
        let req = ImportRequest {
            data_root: root,
            paks: if setup { self.chosen.clone().unwrap_or_default() } else { PathBuf::new() },
            last: None,
            setup,
            then_play,
            force,
            threads: 0,
        };
        self.m.build = Build::default();
        match Worker::start(WorkerOp::Run, &req) {
            Ok(wk) => {
                self.run = Some((wk, view));
                self.draw_run();
                if setup {
                    self.m.screen = Screen::Build;
                } else {
                    self.home(host);
                }
            }
            Err(e) => {
                if setup {
                    self.m.screen = Screen::Build;
                }
                self.stopped(host, setup, &w::not_started(&e.to_string()));
            }
        }
    }

    /// The run's model from its view (launcher.ts drawRun).
    fn draw_run(&mut self) {
        let Some((_, r)) = &self.run else { return };
        let now = self.now();
        let f = r.fraction().min(1.0);
        let written = r.written();
        let b = &mut self.m.build;
        b.frac = f;
        b.now = w::group(r.now).to_string();
        b.count = match &r.count {
            CountLine::None => String::new(),
            CountLine::Check { done, total } => w::checked(*done, *total),
            CountLine::Build { kind, done, total } => w::kind_count(kind, *done, *total),
            CountLine::Kind(k) => k.clone(),
        };
        b.left = r.minutes_left(now).map_or_else(|| w::WORKING_OUT.to_string(), w::time_left);
        b.written = if written > 0 { w::written(&w::size(written)) } else { String::new() };
        b.place = r.data_root.as_ref().map_or_else(String::new, |d| w::installing_to(&d.display().to_string()));
        b.phases = r
            .groups()
            .iter()
            .map(|g| Phase {
                name: w::group(g.name).to_string(),
                state: if g.frac >= 1.0 {
                    PhaseState::Done
                } else if g.name == r.now {
                    PhaseState::Now
                } else {
                    PhaseState::Wait
                },
                n: if g.frac >= 1.0 || matches!(g.name, GroupName::Check | GroupName::Read) {
                    String::new()
                } else {
                    format!("{} / {}", g.done, g.kinds)
                },
            })
            .collect();
        b.log = r.log.iter().map(|l| (clock_at(l.at), tone(l.level), l.text.clone())).collect();
    }

    /// The run's events taken in (launcher.ts onImportEvent).
    fn run_events(&mut self, host: &mut Host) {
        let Some((wk, _)) = &mut self.run else { return };
        let events = wk.poll();
        for e in events {
            let Some((_, view)) = &mut self.run else { return };
            view.event(&e);
            match &e {
                // (the launcher log keeps the errors only: a build's warnings are per file, its details' business)
                ImportEvent::Log { level: LogLevel::Error, say } => self.log(&w::say(&say.clone()), Tone::Bad, ""),
                ImportEvent::Done { kinds } => {
                    self.draw_run();
                    let Some((_, view)) = self.run.take() else { return };
                    let failed: u64 = kinds.iter().map(|k| k.failed).sum();
                    let built = kinds.iter().filter(|k| k.status == "built").count();
                    let min = view.minutes_spent(self.now());
                    self.log(&w::built(built, u64::from(min), failed), Tone::Ok, w::EM_BUILD_DONE);
                    self.picture = Picture::None;
                    self.ask_state(host, Then::Ended { then_play: view.then_play });
                    return;
                }
                ImportEvent::Failed { say } => {
                    let setup = self.run.take().is_some_and(|(_, v)| v.setup);
                    self.stopped(host, setup, &w::stopped(&w::say(say)));
                    return;
                }
                ImportEvent::Cancelled => {
                    let setup = self.run.take().is_some_and(|(_, v)| v.setup);
                    self.stopped(host, setup, w::CANCELLED);
                    return;
                }
                _ => {}
            }
        }
        self.draw_run();
    }

    /// A run that stopped (launcher.ts runStopped): setup's step shows why, Home goes back to its state.
    fn stopped(&mut self, host: &mut Host, setup: bool, message: &str) {
        self.log(message, Tone::Warn, "");
        self.run = None;
        if self.picture == Picture::Failed {
            self.picture = Picture::None;
        }
        if setup {
            self.m.build.stopped = Some(message.to_string());
            self.m.build.left.clear();
        } else {
            self.ask_state(host, Then::Home);
        }
    }

    // -----------------------------------------------------------------------------------------------------------
    // Home

    /// Home with its state (launcher.ts home, homeState).
    fn home(&mut self, host: &mut Host) {
        self.m.screen = Screen::Home;
        let s = self.state.clone().unwrap_or_default();
        let rebuild = self.run.as_ref().is_some_and(|(_, v)| !v.setup);
        let available = self.update.available().map(str::to_owned);
        let running = self.game.is_some();
        let h = &mut self.m.home;
        h.state = if running {
            HomeState::Running
        } else if rebuild {
            HomeState::Rebuild
        } else if self.updating {
            HomeState::Updating
        } else if !s.setup {
            HomeState::Problem
        } else if available.is_some() {
            HomeState::Update
        } else {
            HomeState::Ready
        };
        h.can_rebuild = s.setup && self.run.is_none() && !self.updating && !running && host.store.is_none();
        h.check_updates = self.update.enabled().then_some(!self.updating);
        let version = available.unwrap_or_default();
        h.progress_label = w::downloading(&version);
        let stale = !s.stale.is_empty() || s.new_build;
        h.status = match h.state {
            HomeState::Ready if stale => (
                Tone::Warn,
                w::READY.into(),
                if s.new_build { w::REBUILT_FIRST.into() } else { w::parts_rebuilt_first(s.stale.len()) },
            ),
            // Deviation: no version after "Ready to play" (launcher.ts's status line).
            HomeState::Ready => (Tone::Plain, w::READY_TO_PLAY.into(), self.update_note.clone()),
            HomeState::Rebuild => (Tone::Warn, w::BUILDING_STATUS.into(), w::STARTS_AFTER.into()),
            HomeState::Update => (Tone::Warn, w::is_out(&version), w::INSTALLS_SOON.into()),
            HomeState::Updating => (Tone::Warn, w::UPDATING_STATUS.into(), w::STARTS_AGAIN.into()),
            HomeState::Problem => (Tone::Bad, w::CANT_PLAY.into(), w::UNREADABLE.into()),
            HomeState::Running => {
                (Tone::Plain, w::PLAYING.into(), w::elapsed(self.game.as_ref().map_or(0, Game::seconds)))
            }
        };
        h.problem = w::unreadable(&s.why.as_ref().map(w::why).unwrap_or_default());
        self.home_picture(host);
        self.load_note_pictures(host);
    }

    /// Home's picture, once the store holds it and no run goes on (launcher.ts homePicture): its entry read off the
    /// frame's thread from the store's texture kind alone (no renderer's assets), the upload when it comes
    /// (`poll_picture`).
    fn home_picture(&mut self, host: &Host) {
        let Some(s) = &self.state else { return };
        if self.picture != Picture::None
            || !s.setup
            || self.run.is_some()
            || s.new_build
            || s.stale.iter().any(|k| k == PICTURE_KIND)
        {
            return;
        }
        let dir = Self::store(host);
        let (tx, rx) = mpsc::channel();
        let read = move || {
            let bytes = mvs_store::Store::open(&dir)
                .and_then(|st| st.kind(PICTURE_KIND))
                .and_then(|p| p.bytes(HOME_PICTURE))
                .map_err(|e| e.to_string());
            let _ = tx.send(bytes);
        };
        match std::thread::Builder::new().name("home picture".into()).spawn(read) {
            Ok(_) => {
                self.picture = Picture::Loading;
                self.picture_load = Some(rx);
            }
            Err(e) => self.picture_failed(&e.to_string()),
        }
    }

    /// The picture's entry taken in once read: uploaded and shown.
    fn poll_picture(&mut self, host: &Host) {
        let Some(rx) = &self.picture_load else { return };
        let got = match rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("its reader ended".into()),
        };
        self.picture_load = None;
        match got
            .and_then(|b| mvs_render::assets::upload_texture(&host.gpu, &b, HOME_PICTURE).map_err(|e| e.to_string()))
        {
            Ok(t) => {
                self.ui.set_picture(Some(std::sync::Arc::new(t)));
                self.picture = Picture::On;
            }
            Err(e) => self.picture_failed(&e),
        }
    }

    fn picture_failed(&mut self, e: &str) {
        eprintln!("mvs-launcher: home's picture: {e}");
        self.picture = Picture::Failed;
    }

    // Patch notes (the port's own)

    /// The releases' list asked of the feed, unless it is being read or this build has none.
    fn fetch_notes(&mut self, host: &Host) {
        if self.notes_load.is_none()
            && let Some(url) = notes::feed()
        {
            self.notes_load = notes::fetch(url, host.data.clone());
        }
    }

    /// The list taken in once read: the notes shown are its releases, their pictures asked for.
    fn poll_notes(&mut self, host: &Host) {
        let Some(rx) = &self.notes_load else { return };
        let got = match rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("its reader ended".into()),
        };
        self.notes_load = None;
        match got {
            Ok(list) if list != self.notes => {
                self.notes = list;
                self.m.notes = notes::model(&self.notes, mvs_release::VERSION);
                self.load_note_pictures(host);
            }
            Ok(_) => {}
            Err(e) => self.log(&w::notes_unread(&e), Tone::Warn, ""),
        }
    }

    /// The pictures of the notes shown that were not asked for yet, under Home's picture's own conditions: their
    /// entries read off the frame's thread, the first note's first, each uploaded when it comes
    /// (`poll_note_pictures`). A note whose picture the store does not hold shows its plain card.
    fn load_note_pictures(&mut self, host: &Host) {
        let Some(s) = &self.state else { return };
        if !s.setup || self.run.is_some() || s.new_build || s.stale.iter().any(|k| k == PICTURE_KIND) {
            return;
        }
        let mut want: Vec<usize> = Vec::new();
        for n in &self.m.notes {
            if n.picture < PICTURES.len() && !self.note_pictures.contains(&n.picture) && !want.contains(&n.picture) {
                want.push(n.picture);
            }
        }
        if want.is_empty() {
            return;
        }
        let dir = Self::store(host);
        let asked = want.clone();
        let (tx, rx) = mpsc::channel();
        let read = move || {
            let kind = mvs_store::Store::open(&dir).and_then(|st| st.kind(PICTURE_KIND)).map_err(|e| e.to_string());
            for k in want {
                let bytes = kind.clone().and_then(|p| p.bytes(PICTURES[k]).map_err(|e| e.to_string()));
                if tx.send((k, bytes)).is_err() {
                    return;
                }
            }
        };
        match std::thread::Builder::new().name("note pictures".into()).spawn(read) {
            Ok(_) => {
                self.note_pictures.extend(asked);
                self.note_picture_loads.push(rx);
            }
            Err(e) => eprintln!("mvs-launcher: the patch notes' pictures: {e}"),
        }
    }

    /// A note picture's entry taken in once read: uploaded and shown. One a call, so a frame uploads one picture.
    fn poll_note_pictures(&mut self, host: &Host) {
        let mut k = 0;
        while k < self.note_picture_loads.len() {
            match self.note_picture_loads[k].try_recv() {
                Ok((i, got)) => {
                    match got.and_then(|b| {
                        mvs_render::assets::upload_texture(&host.gpu, &b, PICTURES[i]).map_err(|e| e.to_string())
                    }) {
                        Ok(t) => self.ui.set_note_picture(i, Some(std::sync::Arc::new(t))),
                        Err(e) => eprintln!("mvs-launcher: a patch note's picture: {e}"),
                    }
                    return;
                }
                Err(TryRecvError::Empty) => k += 1,
                Err(TryRecvError::Disconnected) => {
                    self.note_picture_loads.remove(k);
                }
            }
        }
    }

    /// Play (launcher.ts play, window.ts openGame): the game's program started on the store and Home showing it
    /// running; while it runs, its window brought forward. Not ready: the screen the state asks. True when the game
    /// started.
    fn play(&mut self, host: &mut Host) -> bool {
        if let Some(g) = &self.game {
            g.bring_forward();
            return false;
        }
        // (a game this launcher did not start, a join link's, holds the data folder: its window forward)
        if let Some(pid) = crate::online::link::running_game(&host.data) {
            match pid {
                Some(pid) => game::raise(pid),
                None => self.log(w::GAME_RUNNING, Tone::Warn, ""),
            }
            return false;
        }
        if !self.ready() {
            self.log(w::NOT_READY, Tone::Warn, "");
            self.ask_state(host, Then::Route);
            return false;
        }
        match Game::start(&host.game, &Self::store(host)) {
            Ok(g) => {
                self.game = Some(g);
                self.log(w::GAME_OPEN, Tone::Ok, w::EM_STARTED);
                self.home(host);
                true
            }
            Err(e) => {
                let why = format!("{}: {e}", host.game.exe.display());
                self.log(&w::game_not_started(&why), Tone::Warn, "");
                false
            }
        }
    }

    /// The game runs: the one this launcher started, or another holding the data folder (a join link's). It holds the
    /// store's packs and the program's files: no rebuild, update or data move meanwhile.
    fn game_running(&self, host: &Host) -> bool {
        self.game.is_some() || crate::online::link::running_game(&host.data).is_some()
    }

    /// Play, through a rebuild when the store has stale kinds (launcher.ts playOrRebuild).
    fn play_or_rebuild(&mut self, host: &mut Host) -> bool {
        let stale = self.state.as_ref().is_some_and(|s| !s.stale.is_empty() || s.new_build);
        if stale && self.run.is_none() && !self.game_running(host) {
            self.start_run(host, false, true, false);
            false
        } else {
            self.play(host)
        }
    }

    /// The game's end taken in (launcher.ts's `ended` game event): its line, the state asked again, Home.
    fn game_events(&mut self, host: &mut Host) {
        let Some(g) = &mut self.game else { return };
        if !g.ended() {
            // (the status line's clock: launcher.ts ticks it each second)
            if self.m.screen == Screen::Home {
                self.m.home.status.2 = w::elapsed(g.seconds());
            }
            return;
        }
        let after = w::after(&w::elapsed(g.seconds()));
        self.game = None;
        self.log(&after, Tone::Plain, w::EM_GAME_CLOSED);
        self.ask_state(host, Then::Home);
    }

    // -----------------------------------------------------------------------------------------------------------
    // Updates (launcher.ts onUpdate, startUpdate, updateFailed)

    /// The updater's events (launcher.ts onUpdate); `Next::Quit` once the release downloaded is handed over to.
    fn update_events(&mut self, host: &mut Host) -> Next {
        let events = self.update.poll();
        if events.is_empty() {
            return Next::Stay;
        }
        for e in events {
            match e {
                UpdateEvent::Available(v) => {
                    self.checking = false;
                    self.update_asked = false;
                    self.log(&w::is_out(&v), Tone::Ok, w::EM_UPDATE);
                    // (a release the notes do not hold yet: the list read again)
                    if !self.notes.iter().any(|n| n.version == v) {
                        self.fetch_notes(host);
                    }
                    self.note_update(host, String::new());
                }
                UpdateEvent::None => {
                    self.checking = false;
                    if std::mem::take(&mut self.update_asked) {
                        self.log(w::UP_TO_DATE, Tone::Ok, w::EM_UPDATE);
                        self.note_update(host, w::UP_TO_DATE.to_owned());
                    }
                }
                UpdateEvent::Progress(pc) => self.m.home.percent = f64::from(pc),
                UpdateEvent::Ready(v) => {
                    // (a game started during the download: the hand-over waits for the next start)
                    if self.game_running(host) {
                        self.update_failed(host, w::GAME_RUNNING);
                        continue;
                    }
                    self.log(&w::installing(&v), Tone::Ok, w::EM_UPDATE);
                    match self.update.install() {
                        Ok(()) => return Next::Quit,
                        Err(e) => self.update_failed(host, &e.to_string()),
                    }
                }
                UpdateEvent::CheckError(m) => {
                    self.checking = false;
                    self.log(&w::check_unfinished(&m), Tone::Warn, "");
                    if std::mem::take(&mut self.update_asked) {
                        self.note_update(host, w::check_unfinished(&m));
                    }
                }
                UpdateEvent::DownloadError(m) => self.update_failed(host, &m),
            }
        }
        self.m.checking = self.checking;
        if self.m.screen == Screen::Home {
            self.home(host);
        }
        Next::Stay
    }

    /// The download of the release offered (launcher.ts startUpdate). The new version starts on Home.
    // Deviation: launcher.ts played the game at the new version's first start; here the player presses PLAY.
    fn start_update(&mut self, host: &mut Host) {
        let Some(v) = self.update.available().map(str::to_owned) else { return };
        if self.updating {
            return;
        }
        // Deviation: refused while the game runs (its exe cannot be copied over while it runs; in launcher.ts the game
        // was a window of the same program, which the hand-over closed).
        if self.game_running(host) {
            self.log(w::GAME_RUNNING, Tone::Warn, "");
            return;
        }
        self.updating = true;
        self.m.home.percent = 0.0;
        self.log(&w::downloading(&v), Tone::Plain, w::EM_UPDATE);
        self.update.download();
        self.home(host);
    }

    /// A download or its hand-over that failed (launcher.ts updateFailed).
    fn update_failed(&mut self, host: &mut Host, message: &str) {
        self.updating = false;
        self.log(&w::update_unfinished(message), Tone::Warn, "");
        if self.m.screen == Screen::Home {
            self.home(host);
        }
    }

    /// The settings' facts from the state (launcher.ts openSettings).
    fn facts(&mut self, host: &Host) {
        let Some(s) = &self.state else { return };
        let v = &mut self.m.settings;
        v.beta = s.beta_dir.as_ref().map(|d| d.display().to_string());
        v.data = if s.new_build {
            (Tone::Warn, w::REBUILD_NEEDED.into())
        } else if !s.stale.is_empty() {
            (Tone::Warn, w::parts_to_rebuild(s.stale.len()))
        } else {
            (Tone::Plain, w::READY.into())
        };
        v.beta_health = if s.setup { (Tone::Plain, w::FOUND.into()) } else { (Tone::Bad, w::NOT_FOUND.into()) };
        v.can_rebuild = s.setup && host.store.is_none();
    }

    // -----------------------------------------------------------------------------------------------------------
    // The player's choices

    fn act(&mut self, host: &mut Host, a: Action) -> Next {
        let hwnd = host.window.hwnd;
        match a {
            Action::PickRoot => {
                if let Some(dir) = setup_os::pick_folder(hwnd, w::PICK_ROOT) {
                    self.log(&dir.display().to_string(), Tone::Plain, w::EM_INSTALL);
                    host.settings.launcher.data_root = Some(dir);
                    self.save(host);
                    self.new_root(host);
                }
            }
            Action::DefaultRoot => {
                host.settings.launcher.data_root = None;
                self.save(host);
                self.new_root(host);
            }
            Action::CancelSetup => self.ask_state(host, Then::Route),
            Action::ToBeta | Action::Detect | Action::Locate | Action::ChangeBeta => self.step_beta(host),
            Action::BackToInstall | Action::BackToSetup | Action::MoveRoot => self.step_install(host),
            Action::PickBeta => {
                if let Some(dir) = setup_os::pick_folder(hwnd, w::PICK_BETA) {
                    let req = ImportRequest { paks: dir, ..Self::request(host) };
                    self.ask(WorkerOp::Check, &req, Ask::Check);
                }
            }
            // Deviation: a folder of the list is chosen on the detect op's own check (launcher.ts choose checked it
            // again through setup.check); the run checks it once more at its start.
            Action::Choose(paks) => {
                if let Some(s) = self.found.iter().find(|s| s.paks == paks && usable(s)).cloned() {
                    self.choose(&s);
                }
            }
            Action::SteamConsole => {
                if let Err(e) = setup_os::steam_console() {
                    self.log(&e, Tone::Warn, "");
                }
            }
            Action::CopyDepot => {
                if let Err(e) = setup_os::copy_text(hwnd, w::DEPOT) {
                    self.log(&e, Tone::Warn, "");
                }
            }
            Action::StartBuild | Action::Retry => self.start_run(host, true, false, false),
            Action::CancelBuild => {
                if let Some((wk, _)) = &mut self.run {
                    wk.cancel();
                }
            }
            // (a Test connection holds PLAY: its Epic sign-in would take the game's place)
            Action::Play if self.online.testing() && self.m.home.state != HomeState::Running => {}
            Action::Play => match self.m.home.state {
                HomeState::Ready => {
                    self.play_or_rebuild(host);
                }
                HomeState::Update => self.start_update(host),
                HomeState::Problem => self.step_beta(host),
                // (the game's window brought forward)
                HomeState::Running => {
                    self.play(host);
                }
                HomeState::Rebuild | HomeState::Updating => {}
            },
            Action::PlayNow if self.online.testing() => {}
            Action::PlayNow => {
                self.play_or_rebuild(host);
            }
            Action::CheckUpdates => {
                if !self.updating {
                    self.update_asked = true;
                    self.log(w::CHECKING_UPDATES, Tone::Plain, "");
                    self.note_update(host, format!("{}\u{2026}", w::CHECKING_UPDATES));
                    self.update.check(mvs_release::VERSION);
                }
            }
            Action::Rebuild => {
                if self.m.home.can_rebuild {
                    self.start_run(host, false, false, true);
                }
            }
            Action::OpenRoot => {
                if let Err(e) = setup_os::open_folder(&Self::root(host)) {
                    self.log(&w::open_failed(&e), Tone::Warn, "");
                }
            }
            Action::SettingsOpened => {
                self.m.settings.root = Self::root(host).display().to_string();
                // (launcher.ts openSettings: a run or the game holds the store)
                self.m.settings.busy = self.run.is_some() || self.game_running(host);
                self.m.settings.shortcuts = setup_os::shortcuts();
                *self.m.settings.online = self.online.view(&host.settings.online);
                self.facts(host);
                self.ask_state(host, Then::Nothing);
            }
            Action::RelayTyped(t) => {
                self.online.relay_text.clone_from(&t);
                // (an emptied box is no relay: saved at once, nothing to look at; an address is saved when its look
                // reaches the relay, `frame`'s poll)
                if t.trim().is_empty() {
                    let said = self.online.set_relay(&mut host.settings.online, &t);
                    self.online_saved(host, said);
                }
                self.online.start_look(&t);
            }
            Action::RelayEnter(t) => {
                self.online.relay_text.clone_from(&t);
                let said = self.online.set_relay(&mut host.settings.online, &t);
                self.online_saved(host, said);
                self.online.start_look(&t);
            }
            Action::PortTyped(t) | Action::PortEnter(t) => {
                let said = self.online.set_port(&mut host.settings.online, &t);
                self.online_saved(host, said);
            }
            Action::ToggleRelay => {
                let said = self.online.toggle_relay(&mut host.settings.online);
                self.online_saved(host, Some(said));
            }
            Action::ToggleEos => {
                let said = self.online.toggle_eos(&mut host.settings.online);
                self.online_saved(host, Some(said));
            }
            Action::ToggleDirect => {
                let said = self.online.toggle_direct(&mut host.settings.online);
                self.online_saved(host, Some(said));
            }
            Action::ToggleLan => {
                let port = self.m.settings.online.lan_port.clone();
                let said = self.online.toggle_lan(&mut host.settings.online, &port);
                self.online_saved(host, Some(said));
            }
            Action::TestConnection => {
                self.online.start_test(&host.game);
                *self.m.settings.online = self.online.view(&host.settings.online);
            }
            Action::CopyTest => {
                if let Some(text) = self.online.copy_test() {
                    match setup_os::copy_text(hwnd, &text) {
                        Ok(()) => self.log(w::TEST_COPIED, Tone::Ok, w::EM_TEST),
                        Err(e) => self.log(&e, Tone::Warn, ""),
                    }
                }
            }
            Action::ToggleAdvanced => {
                self.online.toggle_advanced();
                *self.m.settings.online = self.online.view(&host.settings.online);
            }
            Action::ShortcutDesktop | Action::ShortcutStartMenu => {
                let (kind, done) = if a == Action::ShortcutDesktop {
                    (ShortcutKind::Desktop, w::ON_DESKTOP)
                } else {
                    (ShortcutKind::StartMenu, w::IN_START_MENU)
                };
                match setup_os::create_shortcut(kind) {
                    Ok(()) => self.log(done, Tone::Ok, w::EM_SHORTCUT),
                    Err(e) => self.log(&w::shortcut_failed(&e), Tone::Warn, ""),
                }
                self.m.settings.shortcuts = setup_os::shortcuts();
            }
            Action::Minimize => host.window.minimize(),
            // (the game keeps running when the launcher closes: window.ts launcherClose)
            Action::Close => return Next::Quit,
        }
        Next::Stay
    }

    /// Step 1 on a new install location: shown at once, its Cancel (setup done there) once that root's state is in.
    fn new_root(&mut self, host: &mut Host) {
        self.step_install(host);
        self.m.install.can_cancel = false;
        self.ask_state(host, Then::Install);
    }

    /// An online setting changed (`said`: its log line): saved, shown again.
    fn online_saved(&mut self, host: &mut Host, said: Option<String>) {
        if let Some(said) = said {
            match host.save_settings() {
                Ok(()) => self.log(&said, Tone::Ok, w::EM_ONLINE),
                Err(e) => self.log(&w::not_saved(&e.to_string()), Tone::Warn, ""),
            }
        }
        *self.m.settings.online = self.online.view(&host.settings.online);
    }

    fn save(&mut self, host: &mut Host) {
        if let Err(e) = host.save_settings() {
            self.log(&e.to_string(), Tone::Warn, "");
        }
    }

    /// What goes on with no frame drawn (a minimised window's too): the ops' answers, the run's events, Home's
    /// picture, the patch notes and their pictures, the game's end, the updater, and the boot's route once the
    /// splash has stood its time.
    pub fn poll(&mut self, host: &mut Host) -> Next {
        // The ops' answers.
        let mut k = 0;
        while k < self.asks.len() {
            let events = self.asks[k].1.poll();
            let ended = self.asks[k].1.ended();
            let mut answered = None;
            for e in events {
                if e.ends() {
                    answered = Some(e);
                }
            }
            if ended || answered.is_some() {
                let (ask, _) = self.asks.remove(k);
                let e = answered.unwrap_or(ImportEvent::Failed { say: Say::NoAnswer });
                self.answer(Some(host), ask, &e);
            } else {
                k += 1;
            }
        }
        self.run_events(host);
        self.poll_picture(host);
        self.poll_notes(host);
        self.poll_note_pictures(host);
        self.game_events(host);
        if self.update_events(host) == Next::Quit {
            return Next::Quit;
        }
        if self.checking && self.now() >= UPDATE_WAIT {
            self.checking = false;
            self.m.checking = false;
        }
        // The boot's route once the splash has stood its time and the update check answered; Play after a run that
        // asked for it.
        if self.now() >= SPLASH_MIN
            && !self.checking
            && let Some(then) = self.boot.take()
        {
            // (an update offered keeps `--play` on Home: launcher.ts plays from `ready`)
            let offer = self.update.available().is_some();
            match then {
                // window.ts openWindow: `--play` opens the game alone when the store is ready
                Then::Boot if host.play && !offer && self.ready() => {
                    if self.play(host) {
                        return Next::Quit;
                    }
                }
                Then::Ended { .. } => {
                    self.play(host);
                }
                _ => self.route(host),
            }
        }
        Next::Stay
    }

    /// One frame: `poll`, then the window's pointer and keys, the draw and the present.
    pub fn frame(&mut self, host: &mut Host) -> Result<Next> {
        if self.poll(host) == Next::Quit {
            return Ok(Next::Quit);
        }
        // The page.
        let keys = host.window.take_menu_keys();
        let pointer = self.pointer(host);
        let typed = std::mem::take(&mut self.online.typed);
        let t = self.now();
        self.online.read_state(&host.data, self.game.as_ref().map(Game::started_at));
        self.online.poll_test();
        self.m.home.testing = self.online.testing();
        if let Some(text) = self.online.poll(&host.settings.online) {
            let said = self.online.set_relay(&mut host.settings.online, &text);
            self.online_saved(host, said);
        } else if *self.m.settings.online != self.online.view(&host.settings.online) {
            *self.m.settings.online = self.online.view(&host.settings.online);
        }
        let actions = self.ui.frame(&host.gpu, host.swap.back_buffer(), &self.m, pointer, &keys, &typed, t)?;
        let mut next = Next::Stay;
        for a in actions {
            if self.act(host, a) == Next::Quit {
                next = Next::Quit;
            }
        }
        host.swap.present(true)?;
        Ok(next)
    }

    /// The pointer in the client area's pixels (the window's WM_MOUSEMOVE), the wheel's notches since the last frame,
    /// the left button and its click (on the button's release where the pointer is).
    // Unverified: the button is polled (GetAsyncKeyState while the window has the focus): the window hands its
    // buttons only to the bound devices' actions, which the launcher has none of. A press and release inside one
    // frame (8 ms) is missed.
    fn pointer(&mut self, host: &mut Host) -> mvs_ui::launcher::draw::Pointer {
        #[cfg(windows)]
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
        #[cfg(windows)]
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let wheel = host.devices.take_wheel();
        self.online.typed.extend(host.devices.take_chars());
        let at = host.devices.pointer.map(|(x, y)| [x, y]);
        // SAFETY: plain reads of the foreground window and the button's state.
        #[cfg(windows)]
        let down = unsafe {
            GetForegroundWindow() == host.window.hwnd
                && (GetAsyncKeyState(i32::from(VK_LBUTTON.0)) as u16 & 0x8000) != 0
        };
        // Unverified: stub, replaced in Task 7 (the left button is never down)
        #[cfg(not(windows))]
        let down = false;
        let click = self.was_down && !down && at.is_some();
        self.was_down = down;
        mvs_ui::launcher::draw::Pointer { at, down, click, wheel }
    }
}
