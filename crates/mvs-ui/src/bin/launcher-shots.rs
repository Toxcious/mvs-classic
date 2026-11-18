//! Draws each launcher screen to a PNG, with made-up facts: the splash, setup's three steps (the build running and
//! stopped, its details), the guide, Home ready, rebuilding, with a problem, with an update offered (and its more
//! menu), updating, the more menu, the settings' tabs. Home's picture comes from a store
//! when one is given.
//!
//! Usage: launcher-shots --out <dir> [--size 1600x900] [--store <dir>] [--time <seconds>]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mvs_render::Assets;
use mvs_render::gpu::{Gpu, TexDesc, TexKind, TexViews};
use mvs_render::image::write_png;
use mvs_store::Store;
use mvs_ui::launcher::draw::Pointer;
use mvs_ui::launcher::{
    Build, HomeState, LauncherUi, LogLine, Model, Note, Phase, PhaseState, Screen, Shortcuts, Source, Tone, words as w,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

const PICTURE: &str = "/Game/Panda_Main/PreMatch/Maps/MapBG_TreeFort2v2SinglePlatform.MapBG_TreeFort2v2SinglePlatform";

struct Args {
    out: PathBuf,
    size: [u32; 2],
    store: Option<PathBuf>,
    time: f64,
}

fn args() -> Result<Args, String> {
    let mut a = Args { out: PathBuf::from("."), size: [1600, 900], store: None, time: 3.0 };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().ok_or(format!("{arg}: no value"));
        match arg.as_str() {
            "--out" => a.out = PathBuf::from(val()?),
            "--store" => a.store = Some(PathBuf::from(val()?)),
            "--time" => a.time = val()?.parse().map_err(|_| "--time: a number")?,
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

/// The facts the screens are drawn with.
fn model() -> Model {
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
    m
}

fn picture(gpu: &Gpu, store: &Path) -> Result<Arc<mvs_render::gpu::Texture>, String> {
    let assets = Assets::open(Store::open(store).map_err(|e| e.to_string())?, gpu).map_err(|e| e.to_string())?;
    assets.texture(PICTURE).map_err(|e| e.to_string())
}

fn main() -> Result<(), String> {
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
    let base = model();
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
                m.settings.online.relay = "relay.example.org".into();
                m.settings.online.eos = true;
                m.settings.online.lan = Some(7777);
                m.settings.online.lan_port = "7777".into();
                m.settings.online.direct = true;
                m.settings.online.relay_status = mvs_ui::launcher::Note::new(w::reached(42, 1, true), Tone::Ok);
                m.settings.online.direct_now = mvs_ui::launcher::Note::new(w::DIRECT_ON, Tone::Plain);
                let line = mvs_ui::menu::online::OnlineLine::Slower;
                m.settings.online.line = Note::new(mvs_ui::menu::online_line::headline(&line), Tone::Plain);
                m.settings.online.line_detail = mvs_ui::menu::online_line::advice(&line).unwrap_or_default();
                m.settings.online.test = vec![
                    Note::new(w::test_relay_reached(42), Tone::Ok),
                    Note::new(w::TEST_UDP_OFF, Tone::Bad),
                    Note::new(w::TEST_EPIC_ON, Tone::Ok),
                    Note::new(w::test_lan_free(7777), Tone::Ok),
                    Note::new(w::TEST_USES_RELAY_SLOWER, Tone::Plain),
                ];
                m.settings.online.advanced = true;
            },
            (false, Some(2), false, false),
        ),
        (
            "settings-online-untested",
            |m| {
                m.screen = Screen::Home;
                m.settings.online.relay = "relay.example.org".into();
                m.settings.online.eos = true;
            },
            (false, Some(2), false, false),
        ),
        ("settings-about", |m| m.screen = Screen::Home, (false, Some(3), false, false)),
    ];
    if let Some(s) = &a.store {
        match picture(&gpu, s) {
            Ok(t) => ui.set_picture(Some(t)),
            Err(e) => eprintln!("launcher-shots: home's picture: {e}"),
        }
    }
    let pointer = Pointer { at: None, down: false, click: false, wheel: 0.0 };
    for (name, set, (guide, settings, menu, details)) in shots {
        let mut m = base.clone();
        set(&mut m);
        // (a first frame at 0 starts the screen's clocks: the picture's fade in)
        ui.frame(&gpu, &target, &m, pointer, &[], &[], 0.0).map_err(|e| e.to_string())?;
        ui.open(guide, settings, menu, details);
        ui.frame(&gpu, &target, &m, pointer, &[], &[], a.time).map_err(|e| e.to_string())?;
        let px = gpu.read_texture(&target).map_err(|e| e.to_string())?;
        let file = a.out.join(format!("launcher-{name}.png"));
        write_png(&file, a.size[0], a.size[1], &px)?;
        println!("{}", file.display());
    }
    Ok(())
}
