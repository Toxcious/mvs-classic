//! Draws the pre-match level of the menus (the PreMatch scene behind the screens: camera lerps, slanted plates,
//! background, previews, banners) at chosen frames of a scripted pre-match and writes the post chain's pictures: the
//! check of the level's port against the oracle's pre-match pictures. No screen is drawn over it.
//!
//! prematch-frame [--store <dir>] [--size 1920x1080] [--style 0|1|2] [--chars <key>,<key>,<key>,<key>]
//!   [--slugs <s1>,<s2>,<s3>,<s4>] [--skins <SkinData name>,... ("-" the default skin)][--found <frame>] [--ready <frame>] [--events <slug>,..] [--frames <f1>,<f2>,...] --out <dir>
//!
//! Frames count from the level's opening (60 a second, each a `menu_tick` of 1/60 s). `--found` is the frame where
//! BPI_PreMatchTransition("PlayersFound") goes in (default 150: after the character select's 2.1 s camera lerp),
//! `--ready` the frame of "MatchReady" (default none). `--chars` are the roster keys of the preview slots 1..4 ("-"
//! none; default shaggy for the slots the style shows). `--slugs` are the preview slots' BannerData slugs ("-" the
//! default banner). `--map <path>` sends the lobby's map (PreloadMap and the transition of its path) at frame 2; `--map ?`
//! lists the background table's keys. `--cursor <x>,<y>` is the normalised mouse position (default 0,0). `--events
//! <slug>,..` are the active events the background's material follows (default none: the build offline). One picture
//! per frame of `--frames`, `frame<n>.png`.

use std::path::PathBuf;
use std::process::ExitCode;

use mvs_render::assets::Assets;
use mvs_render::gpu::{Gpu, TexDesc, TexKind, TexViews};
use mvs_render::image::write_png;
use mvs_render::menuscene::format::{LevelName, SceneData, load_level};
use mvs_render::menuscene::{MenuLevel, PreviewSpec, SeqCmd};
use mvs_render::renderer::Renderer;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

struct Args {
    store: PathBuf,
    size: [u32; 2],
    style: u8,
    chars: Vec<Option<String>>,
    slugs: Vec<Option<String>>,
    skins: Vec<Option<String>>,
    found: u32,
    ready: Option<u32>,
    map: Option<String>,
    cursor: [f64; 2],
    events: Vec<String>,
    frames: Vec<u32>,
    out: PathBuf,
}

fn list(v: &str) -> Vec<Option<String>> {
    v.split(',').map(|s| (s != "-").then(|| s.to_string())).collect()
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args {
        store: mvs_store::dev_store_dir(),
        size: [1920, 1080],
        style: 0,
        chars: Vec::new(),
        slugs: vec![None; 4],
        skins: vec![None; 4],
        found: 150,
        ready: None,
        map: None,
        cursor: [0.0, 0.0],
        events: Vec::new(),
        frames: vec![140],
        out: PathBuf::new(),
    };
    let mut chars = None;
    while let Some(k) = it.next() {
        let mut v = || it.next().ok_or(format!("{k} needs a value"));
        let num = |s: String| s.parse::<u32>().map_err(|e| format!("{k}: {e}"));
        match k.as_str() {
            "--store" => a.store = v()?.into(),
            "--size" => {
                let s = v()?;
                let (w, h) = s.split_once('x').ok_or(format!("--size {s}"))?;
                a.size = [w.parse().map_err(|_| format!("--size {s}"))?, h.parse().map_err(|_| format!("--size {s}"))?];
            }
            "--style" => a.style = num(v()?)? as u8,
            "--chars" => chars = Some(list(&v()?)),
            "--slugs" => a.slugs = list(&v()?),
            "--skins" => a.skins = list(&v()?),
            "--found" => a.found = num(v()?)?,
            "--ready" => a.ready = Some(num(v()?)?),
            "--cursor" => {
                let s = v()?;
                let (x, y) = s.split_once(',').ok_or(format!("--cursor {s}"))?;
                a.cursor =
                    [x.parse().map_err(|_| format!("--cursor {s}"))?, y.parse().map_err(|_| format!("--cursor {s}"))?];
            }
            "--map" => a.map = Some(v()?),
            "--events" => a.events = v()?.split(',').map(str::to_owned).collect(),
            "--frames" => a.frames = v()?.split(',').map(|s| num(s.to_string())).collect::<Result<_, _>>()?,
            "--out" => a.out = v()?.into(),
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    if a.out.as_os_str().is_empty() {
        return Err("--out <dir>".into());
    }
    if a.slugs.len() != 4 {
        return Err("--slugs: four".into());
    }
    // the slots the style shows: 1v1 Player1 and Player4, the others all four
    a.chars = chars.unwrap_or_else(|| {
        let on = |i: usize| a.style != 0 || i == 0 || i == 3;
        (0..4).map(|i| on(i).then(|| "shaggy".to_string())).collect()
    });
    if a.chars.len() != 4 {
        return Err("--chars: four".into());
    }
    a.frames.sort_unstable();
    Ok(a)
}

fn run(a: &Args) -> Result<(), String> {
    let e = |x: &dyn std::fmt::Display| x.to_string();
    let gpu = Gpu::new(None, false).map_err(|x| e(&x))?;
    let assets = Assets::open(mvs_store::Store::open(&a.store).map_err(|x| e(&x))?, &gpu).map_err(|x| e(&x))?;
    let mut r = Renderer::new(gpu, assets, a.size).map_err(|x| e(&x))?;
    let mut vfx = mvs_vfx::Player::new(r.assets().store(), 7).map_err(|x| e(&x))?;
    if a.map.as_deref() == Some("?") {
        let entry = load_level(r.assets().store(), LevelName::PreMatch).map_err(|x| e(&x))?;
        let SceneData::PreMatch(pm) = entry.data else { return Err("not the PreMatch entry".into()) };
        for (k, v) in &pm.background.lookup {
            eprintln!("prematch-frame: map {k} -> {v:?}");
        }
        return Ok(());
    }
    r.set_menu_team_style(Some(a.style));
    r.set_menu_cursor(a.cursor);
    r.set_menu_active_events(&a.events);
    r.set_menu_level(MenuLevel::PreMatch, Some(&mut vfx)).map_err(|x| e(&x))?;
    let spec = |i: usize| {
        a.chars[i].as_ref().map(|key| PreviewSpec {
            key: key.clone(),
            skin: a.skins.get(i).cloned().flatten().unwrap_or_default(),
            team: u8::from(i % 2 == 1),
        })
    };
    for i in 0..4 {
        if let Some(s) = spec(i) {
            r.menu_preview(i, Some(s)).map_err(|x| e(&x))?;
        }
        r.menu_banner(i, a.slugs[i].as_deref()).map_err(|x| e(&x))?;
    }
    let desc = TexDesc {
        kind: TexKind::D2,
        size: [a.size[0], a.size[1], 1],
        mips: 1,
        format: DXGI_FORMAT_R8G8B8A8_UNORM,
        views: TexViews { srv: true, rtv: true, ..Default::default() },
    };
    let out = r.gpu().texture(&desc, &[]).map_err(|x| e(&x))?;
    std::fs::create_dir_all(&a.out).map_err(|x| format!("{}: {x}", a.out.display()))?;
    let last = *a.frames.last().ok_or("--frames: none")?;
    for frame in 1..=last {
        if let (2, Some(map)) = (frame, &a.map) {
            r.menu_sequence(SeqCmd::PreloadMap(map.clone())).map_err(|x| e(&x))?;
            r.menu_sequence(SeqCmd::Transition(map.clone())).map_err(|x| e(&x))?;
        }
        if frame == a.found {
            r.menu_sequence(SeqCmd::Transition("PlayersFound".into())).map_err(|x| e(&x))?;
        }
        if Some(frame) == a.ready {
            r.menu_sequence(SeqCmd::Transition("MatchReady".into())).map_err(|x| e(&x))?;
        }
        r.menu_tick(1.0 / 60.0, Some(&mut vfx)).map_err(|x| e(&x))?;
        for ev in r.take_menu_events() {
            eprintln!("prematch-frame: frame {frame}: {ev:?}");
        }
        if a.frames.contains(&frame) {
            r.menu_draw(Some(&mut vfx), Some(&out)).map_err(|x| e(&x))?;
            r.gpu().finish().map_err(|x| e(&x))?;
            let px = r.read_back(&out).map_err(|x| e(&x))?;
            let path = a.out.join(format!("frame{frame}.png"));
            write_png(&path, a.size[0], a.size[1], &px)?;
            eprintln!("prematch-frame: frame {frame} -> {}", path.display());
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)).and_then(|a| run(&a)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(x) => {
            eprintln!("prematch-frame: {x}");
            ExitCode::FAILURE
        }
    }
}
