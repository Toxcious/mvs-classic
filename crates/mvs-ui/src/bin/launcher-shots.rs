//! Draws each launcher screen to a PNG, with made-up facts: the splash, setup's three steps (the build running and
//! stopped, its details), the guide, Home ready, rebuilding, with a problem, with an update offered (and its more
//! menu), updating, the more menu, the settings' tabs (the Online tab in each of its states: the default relay, the
//! relay off, another relay, Advanced open, a running game holding a change), the patch notes over Home. Home's picture and the patch
//! notes' come from a store when one is given. `--notes` shows the releases of a file in place of the made-up ones,
//! the first as the one installed: the launcher's kept list (`patch-notes.json` of its data folder), or one written
//! by hand to see a release's text before it is published (`[{"version": "0.2.0", "name": "..", "published_at":
//! "2026-03-01T12:00:00Z", "body": "<its Markdown>"}]`).
//!
//! `--only <prefix>` draws the screens whose name starts with it alone.
//!
//! Usage: launcher-shots --out <dir> [--size 1600x900] [--store <dir>] [--notes <file>] [--time <seconds>]
//!        [--only <prefix>]

use std::path::{Path, PathBuf};

use mvs_d3d::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
use mvs_render::Assets;
use mvs_render::gpu::{Gpu, TexDesc, TexKind, TexViews};
use mvs_render::image::write_png;
use mvs_store::Store;
use mvs_ui::launcher::draw::Pointer;
use mvs_ui::launcher::notes::{self, NoteAge, PatchNote};
use mvs_ui::launcher::{
    Build, HomeState, LauncherUi, LogLine, Model, Note, OnlineSettingsView, OnlineSummary, Phase, PhaseState, Screen,
    Shortcuts, Source, Tone, Way, words as w,
};

const PICTURE: &str = "/Game/Panda_Main/PreMatch/Maps/MapBG_TreeFort2v2SinglePlatform.MapBG_TreeFort2v2SinglePlatform";

struct Args {
    out: PathBuf,
    size: [u32; 2],
    store: Option<PathBuf>,
    notes: Option<PathBuf>,
    time: f64,
    only: String,
}

fn args() -> Result<Args, String> {
    let mut a =
        Args { out: PathBuf::from("."), size: [1600, 900], store: None, notes: None, time: 3.0, only: String::new() };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().ok_or(format!("{arg}: no value"));
        match arg.as_str() {
            "--out" => a.out = PathBuf::from(val()?),
            "--store" => a.store = Some(PathBuf::from(val()?)),
            "--notes" => a.notes = Some(PathBuf::from(val()?)),
            "--time" => a.time = val()?.parse().map_err(|_| "--time: a number")?,
            "--only" => a.only = val()?,
            "--size" => {
                let v = val()?;
                let (x, y) = v.split_once('x').ok_or("--size: WxH")?;
                a.size = [x.parse().map_err(|_| "--size")?, y.parse().map_err(|_| "--size")?];
            }
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    Ok(a)
}

fn line(time: &str, tone: Tone, em: &str, text: &str) -> LogLine {
    LogLine { time: time.into(), tone, em: em.into(), text: text.into() }
}

/// Made-up releases' texts, in the Markdown a release's page is written in.
const NOTES: [([u32; 3], &str, &str); 3] = [
    (
        [0, 1, 1],
        "2026-02-14T18:00:00Z",
        "A second release, a few weeks after the first.

> [!IMPORTANT]
> This release changes the lobby protocol: everyone in a lobby needs it, and a [relay](https://example.org/relay)
> you host yourself must be updated with its players.

## New

- **Patch notes in the launcher.** The latest releases show as cards on the home screen; click one to read what
  changed.
- **A longer practice list.** The Lab's bot menu holds more behaviours, and remembers the one you picked.
  - The dummy can be told to tech in place, away or toward you.
  - A recorded input can loop.
- **Quicker rematches.** Both players pressing **REMATCH** start the next match without the results screen.

## Fixes

- **A queue that lost its connection** no longer leaves the pre-match screen waiting; it ends and says why.
- A long list in a drop-down now scrolls instead of running off the window.
- The settings' **Online** tab keeps the relay address you typed when the test fails.
- Text chat no longer opens while a controller's menu button is held.

## Performance

- Lower memory use in long sessions: screens and effects no longer in use are let go.
- The first match after the launcher opens starts sooner.

## Download

Download `MVS-Classic-0.1.1.zip`, unzip it and run the launcher.",
    ),
    (
        [0, 1, 0],
        "2026-01-20T12:00:00Z",
        "The first release.

## What's in it

- The open beta's roster, stages and match rules
- Local matches, the Lab and the tutorials
- Online play with rollback netcode",
    ),
    ([0, 0, 9], "2026-01-02T12:00:00Z", ""),
];

/// A release of `--notes`' file.
#[derive(serde::Deserialize)]
struct Kept {
    version: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    published_at: String,
    #[serde(default)]
    body: String,
}

/// The notes shown: the releases of `file`, else the made-up ones; the first is the one installed.
fn patch_notes(file: Option<&Path>) -> Result<Vec<PatchNote>, String> {
    let list: Vec<([u32; 3], String, String, String)> = match file {
        Some(f) => {
            let at = |e: &dyn std::fmt::Display| format!("{}: {e}", f.display());
            let kept: Vec<Kept> = serde_json::from_slice(&std::fs::read(f).map_err(|e| at(&e))?).map_err(|e| at(&e))?;
            kept.into_iter()
                .map(|k| {
                    let mut numbers = [0; 3];
                    for (n, part) in numbers.iter_mut().zip(k.version.split('.')) {
                        *n = part.parse().unwrap_or(0);
                    }
                    (numbers, k.name, k.published_at, k.body)
                })
                .collect()
        }
        None => NOTES
            .iter()
            .map(|([a, b, c], at, text)| {
                ([*a, *b, *c], format!("MVS Classic {a}.{b}.{c}"), (*at).into(), (*text).into())
            })
            .collect(),
    };
    let pictures = notes::pictures(&list.iter().map(|n| n.0).collect::<Vec<_>>());
    Ok(list
        .into_iter()
        .zip(pictures)
        .enumerate()
        .map(|(k, (([major, minor, patch], title, at, text), picture))| PatchNote {
            version: format!("{major}.{minor}.{patch}"),
            title,
            date: notes::day(&at).map(|(y, m, d)| w::note_day(y, m, d)).unwrap_or_default(),
            age: if k == 0 { NoteAge::Current } else { NoteAge::Old },
            picture,
            blocks: notes::parse(&text),
        })
        .collect())
}

/// The facts the screens are drawn with.
fn model(notes: Vec<PatchNote>) -> Model {
    let mut m = Model::default();
    m.install.root = "D:\\Games\\MVS Classic\\data".into();
    m.install.space = Some(Note::new(w::space_ok("412.6 GB"), Tone::Ok));
    m.install.can_continue = true;
    m.install.can_default = true;
    m.beta.sources = Some(vec![
        Source {
            note: "The MultiVersus open beta".into(),
            path: "C:\\Program Files (x86)\\Steam\\steamapps\\content\\app_1818750\\depot_1818751\\MultiVersus\\Content\\Paks".into(),
            paks: PathBuf::from("C:/beta/Paks"),
            usable: true,
        },
        Source {
            note: "MultiVersus (2024): a later version, not the open beta".into(),
            path: "C:\\Program Files (x86)\\Steam\\steamapps\\common\\MultiVersus".into(),
            paks: PathBuf::from("C:/later/Paks"),
            usable: false,
        },
    ]);
    m.beta.chosen = Some(PathBuf::from("C:/beta/Paks"));
    let phase = |name: &str, state, n: &str| Phase { name: name.into(), state, n: n.into() };
    m.build = Build {
        stopped: None,
        frac: 0.43,
        now: "Models and textures".into(),
        count: "texture 3,214 / 7,980".into(),
        left: w::time_left(4),
        written: w::written("1.9 GB"),
        place: w::installing_to("D:\\Games\\MVS Classic\\data"),
        phases: vec![
            phase("Checking your copy", PhaseState::Done, ""),
            phase("Reading the beta's packages", PhaseState::Done, ""),
            phase("Characters", PhaseState::Done, ""),
            phase("Stages", PhaseState::Done, ""),
            phase("Models and textures", PhaseState::Now, "1 / 2"),
            phase("Effects and materials", PhaseState::Wait, "0 / 5"),
            phase("Sound and music", PhaseState::Wait, "0 / 2"),
            phase("Menus and text", PhaseState::Wait, "0 / 10"),
        ],
        log: vec![
            ("21:04:11".into(), Tone::Plain, "checking 61 files (24.8 GB)".into()),
            ("21:05:40".into(), Tone::Plain, "reading 31,206 packages".into()),
            ("21:06:02".into(), Tone::Warn, "texture T_Unused_01: no mips, skipped".into()),
            ("21:07:15".into(), Tone::Plain, "fighter: 26 built".into()),
        ],
    };
    m.home.state = HomeState::Ready;
    m.home.status = (Tone::Plain, w::READY_TO_PLAY.into(), String::new());
    m.home.can_rebuild = true;
    m.home.check_updates = Some(true);
    m.home.problem = w::unreadable("the folder D:\\Beta is gone");
    m.log = vec![
        line("21:03:58", Tone::Plain, w::EM_STARTED, w::LAUNCHER_OPENED),
        line(
            "21:04:02",
            Tone::Ok,
            w::EM_FOUND,
            "C:\\Program Files (x86)\\Steam\\steamapps\\content\\app_1818750\\depot_1818751",
        ),
        line("21:04:03", Tone::Plain, w::EM_BUILDING, w::FROM_BETA),
        line("21:16:40", Tone::Ok, w::EM_BUILD_DONE, &w::built(33, 12, 4)),
        line("21:17:02", Tone::Warn, "", "The game could not start: the game data is not ready"),
    ];
    m.settings.root = "D:\\Games\\MVS Classic\\data".into();
    m.settings.beta = Some("C:\\Program Files (x86)\\Steam\\steamapps\\content\\app_1818750\\depot_1818751".into());
    m.settings.data = (Tone::Plain, w::READY.into());
    m.settings.beta_health = (Tone::Plain, w::FOUND.into());
    m.settings.can_rebuild = true;
    m.settings.shortcuts = Shortcuts { available: true, desktop: true, start_menu: false };
    m.notes = notes;
    m
}

/// Home's picture and the note pictures, from the store.
fn pictures(gpu: &Gpu, store: &Path, ui: &mut LauncherUi) -> Result<(), String> {
    let assets = Assets::open(Store::open(store).map_err(|e| e.to_string())?, gpu).map_err(|e| e.to_string())?;
    match assets.texture(PICTURE) {
        Ok(t) => ui.set_picture(Some(t)),
        Err(e) => eprintln!("launcher-shots: home's picture: {e}"),
    }
    for (k, name) in notes::PICTURES.iter().enumerate() {
        match assets.texture(name) {
            Ok(t) => ui.set_note_picture(k, Some(t)),
            Err(e) => eprintln!("launcher-shots: note picture {name}: {e}"),
        }
    }
    Ok(())
}

fn main() -> Result<(), String> {
    // (DXVK-native's settings, before any thread: the device reads them)
    mvs_d3d::set_linux_wsi();
    let a = args()?;
    std::fs::create_dir_all(&a.out).map_err(|e| e.to_string())?;
    let gpu = Gpu::new(None, false).map_err(|e| e.to_string())?;
    let target = gpu
        .texture(
            &TexDesc {
                kind: TexKind::D2,
                size: [a.size[0], a.size[1], 1],
                mips: 1,
                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                views: TexViews { rtv: true, srv: true, ..Default::default() },
            },
            &[],
        )
        .map_err(|e| e.to_string())?;
    let mut ui = LauncherUi::new(&gpu).map_err(|e| e.to_string())?;
    let base = model(patch_notes(a.notes.as_deref())?);
    type Shot = (&'static str, fn(&mut Model), (bool, Option<usize>, bool, bool));
    let none = (false, None, false, false);
    let shots: Vec<Shot> = vec![
        ("splash", |m| m.screen = Screen::Splash, none),
        ("setup-1-install", |m| m.screen = Screen::Install, none),
        ("setup-2-beta", |m| m.screen = Screen::Beta, none),
        ("setup-2-guide", |m| m.screen = Screen::Beta, (true, None, false, false)),
        ("setup-3-build", |m| m.screen = Screen::Build, none),
        ("home", |m| m.screen = Screen::Home, none),
        ("setup-3-details", |m| m.screen = Screen::Build, (false, None, false, true)),
        (
            "setup-3-stopped",
            |m| {
                m.screen = Screen::Build;
                m.build.stopped = Some(w::stopped("the beta's file pakchunk3-WindowsNoEditor.pak could not be read"));
                m.build.left.clear();
            },
            none,
        ),
        ("home-ready", |m| m.screen = Screen::Home, none),
        ("home-menu", |m| m.screen = Screen::Home, (false, None, true, false)),
        (
            "home-rebuild",
            |m| {
                m.screen = Screen::Home;
                m.home.state = HomeState::Rebuild;
                m.home.status = (Tone::Warn, w::BUILDING_STATUS.into(), w::STARTS_AFTER.into());
            },
            none,
        ),
        (
            "home-problem",
            |m| {
                m.screen = Screen::Home;
                m.home.state = HomeState::Problem;
                m.home.status = (Tone::Bad, w::CANT_PLAY.into(), w::UNREADABLE.into());
            },
            none,
        ),
        (
            "home-update",
            |m| {
                m.screen = Screen::Home;
                m.home.state = HomeState::Update;
                m.home.status = (Tone::Warn, w::is_out("0.1.1"), w::INSTALLS_SOON.into());
                for (n, age) in m.notes.iter_mut().zip([NoteAge::New, NoteAge::Current]) {
                    n.age = age;
                }
            },
            none,
        ),
        (
            "home-update-menu",
            |m| {
                m.screen = Screen::Home;
                m.home.state = HomeState::Update;
                m.home.status = (Tone::Warn, w::is_out("0.1.1"), w::INSTALLS_SOON.into());
            },
            (false, None, true, false),
        ),
        (
            "home-updating",
            |m| {
                m.screen = Screen::Home;
                m.home.state = HomeState::Updating;
                m.home.status = (Tone::Warn, w::UPDATING_STATUS.into(), w::STARTS_AGAIN.into());
                m.home.progress_label = w::downloading("0.1.1");
                m.home.percent = 37.0;
                m.home.check_updates = Some(false);
            },
            none,
        ),
        ("settings-general", |m| m.screen = Screen::Home, (false, Some(0), false, false)),
        ("settings-files", |m| m.screen = Screen::Home, (false, Some(1), false, false)),
        (
            "settings-online",
            |m| {
                m.screen = Screen::Home;
                online_default(&mut m.settings.online);
            },
            (false, Some(2), false, false),
        ),
        (
            "settings-online-relay-off",
            |m| {
                m.screen = Screen::Home;
                let o = &mut m.settings.online;
                online_default(o);
                o.relay_on = false;
                o.relay_far = String::new();
                o.relay_now = Note::new(w::RELAY_OFF, Tone::Plain);
                o.summary = OnlineSummary { custom: Way::Epic, queue: Way::Epic };
            },
            (false, Some(2), false, false),
        ),
        (
            "settings-online-other-relay",
            |m| {
                m.screen = Screen::Home;
                let o = &mut m.settings.online;
                online_default(o);
                o.relay = "mine.example.org:9000".into();
                o.relay_far = String::new();
                o.relay_status = Note::new(w::reached(42, 7, true), Tone::Ok);
                o.eos = false;
                o.epic_now = Note::new(w::EPIC_OFF, Tone::Plain);
                o.lan = Some(7777);
                o.lan_now = Note::new(w::LAN_ON, Tone::Plain);
            },
            (false, Some(2), false, false),
        ),
        (
            "settings-online-relay-version",
            |m| {
                m.screen = Screen::Home;
                let o = &mut m.settings.online;
                online_default(o);
                // (a look at a relay of the version before, Epic off: a local lobby, no matchmaking)
                let ours = mvs_protocol::version::PROTOCOL;
                o.relay = "mine.example.org:9000".into();
                o.relay_far = String::new();
                o.relay_status = Note::new(w::relay_other_version(ours - 1, ours), Tone::Warn);
                o.eos = false;
                o.epic_now = Note::new(w::EPIC_OFF, Tone::Plain);
                o.summary = OnlineSummary { custom: Way::Local, queue: Way::RelayVersion };
            },
            (false, Some(2), false, false),
        ),
        (
            "settings-online-nothing",
            |m| {
                m.screen = Screen::Home;
                let o = &mut m.settings.online;
                online_default(o);
                o.relay_on = false;
                o.relay_far = String::new();
                o.relay_now = Note::new(w::RELAY_OFF, Tone::Plain);
                o.eos = false;
                o.epic_now = Note::new(w::EPIC_OFF, Tone::Plain);
                o.summary = OnlineSummary { custom: Way::Local, queue: Way::Off };
                let line = mvs_ui::menu::online::OnlineLine::Offline(mvs_ui::menu::online::OfflineWhy::TurnedOff);
                o.line = Note::new(mvs_ui::menu::online_line::headline(&line), Tone::Bad);
            },
            (false, Some(2), false, false),
        ),
        (
            "settings-online-advanced",
            |m| {
                m.screen = Screen::Home;
                let o = &mut m.settings.online;
                online_default(o);
                o.lan = Some(7777);
                o.lan_now = Note::new(w::lan_ready("192.168.1.20", 7777), Tone::Ok);
                o.direct = true;
                o.direct_now = Note::new(w::DIRECT_ON, Tone::Plain);
                o.advanced = true;
            },
            (false, Some(2), false, false),
        ),
        (
            "settings-online-running-held",
            |m| {
                m.screen = Screen::Home;
                m.home.state = HomeState::Running;
                let o = &mut m.settings.online;
                online_default(o);
                // (the player turned the relay off in a lobby: the game keeps it until the lobby closes)
                o.relay_on = false;
                o.relay_far = String::new();
                o.held = true;
                o.relay_now = Note::new(w::relay_connected("relay.example.org"), Tone::Ok);
                o.epic_now = Note::new(w::EPIC_SIGNED_IN, Tone::Ok);
                let line = mvs_ui::menu::online::OnlineLine::Online;
                o.line = Note::new(mvs_ui::menu::online_line::headline(&line), Tone::Ok);
                o.line_detail = mvs_ui::menu::online_line::advice(&line).unwrap_or_default();
                o.test = vec![
                    Note::new(w::test_relay_reached(42), Tone::Ok),
                    Note::new(w::TEST_UDP_ON, Tone::Ok),
                    Note::new(w::TEST_EPIC_ON, Tone::Ok),
                    Note::new(w::TEST_LAN_OFF, Tone::Plain),
                    Note::new(w::TEST_USES_RELAY, Tone::Ok),
                ];
            },
            (false, Some(2), false, false),
        ),
        ("settings-about", |m| m.screen = Screen::Home, (false, Some(3), false, false)),
    ];
    if let Some(s) = &a.store
        && let Err(e) = pictures(&gpu, s, &mut ui)
    {
        eprintln!("launcher-shots: the store's pictures: {e}");
    }
    let pointer = Pointer { at: None, down: false, click: false, wheel: 0.0 };
    // (the patch notes over Home, after the others: the newest release's, an older one's, one with no text)
    let notes: [(&str, Option<usize>); 3] = [("notes", Some(0)), ("notes-older", Some(1)), ("notes-empty", Some(2))];
    let shots = shots.into_iter().map(|s| (s, None)).chain(notes.map(|(name, k)| {
        let home: Shot = (name, |m| m.screen = Screen::Home, none);
        (home, k)
    }));
    for ((name, set, (guide, settings, menu, details)), note) in shots {
        if !name.starts_with(a.only.as_str()) {
            continue;
        }
        let mut m = base.clone();
        set(&mut m);
        // (a first frame at 0 starts the screen's clocks: the pictures' fade in)
        ui.frame(&gpu, &target, &m, pointer, &[], &[], 0.0).map_err(|e| e.to_string())?;
        ui.open(guide, settings, menu, details);
        ui.open_note(note);
        // (Advanced open: the tab scrolled to its foot)
        ui.scroll_online(if name.ends_with("advanced") { 10_000.0 } else { 0.0 });
        ui.frame(&gpu, &target, &m, pointer, &[], &[], a.time).map_err(|e| e.to_string())?;
        // (once more: the first draw at the scroll asked clamps it to the tab's foot)
        ui.frame(&gpu, &target, &m, pointer, &[], &[], a.time).map_err(|e| e.to_string())?;
        let px = gpu.read_texture(&target).map_err(|e| e.to_string())?;
        let file = a.out.join(format!("launcher-{name}.png"));
        write_png(&file, a.size[0], a.size[1], &px)?;
        println!("{}", file.display());
    }
    Ok(())
}

/// The Online tab as the launcher shows it before a test, the release's relay in use: its address the default.
fn online_default(o: &mut OnlineSettingsView) {
    o.summary = OnlineSummary { custom: Way::Relay, queue: Way::Relay };
    o.relay = "relay.example.org".into();
    o.relay_on = true;
    o.relay_default = "relay.example.org".into();
    o.relay_region = "Europe".into();
    o.relay_far = w::relay_region("Europe");
    o.eos = true;
    o.lan_port = "7777".into();
    o.epic_now = Note::default();
    o.lan_now = Note::new(w::LAN_OFF, Tone::Plain);
    o.direct_now = Note::new(w::DIRECT_OFF, Tone::Plain);
}
