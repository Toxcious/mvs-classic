//! (the port's own) Draws a look (`mvs_render::look`) into a recorded match: a visual idea seen on a real stage, with
//! the fighters, the effects and the post chain, before it is built as a feature.
//!
//! preview [--store <dir>] --match <trace match> --tick <n> [--stage <stage>] --look <dir> [--clip <seconds>]
//!   [--fps 30] [--size 1920x1080] --out <dir>
//!
//! The trace match `<MVS_TRACES>/<name>.trace` is stepped to `--tick` (on `--stage` when given: its level, emitters
//! and surfaces, the World the trace's), then the look is added: its lights, meshes and particle systems, its camera
//! roll and zoom, its shapes and screen passes. A still writes `<out>/still.png`; `--clip` steps on for that many
//! seconds and writes `<out>/clip.mp4` through ffmpeg (on PATH). The look's clock starts at 0 at `--tick`. It prints
//! the camera, the blast box and the fighters' places (UE cm), what a look file is written against.
//!
//! The fighters move as the recording moved them: a preview judges a look, not how a rule plays.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mvs_render::assets::Assets;
use mvs_render::fx::surfaces::level_surfaces;
use mvs_render::fx::{CameraRoot, Fx};
use mvs_render::gpu::{Gpu, TexDesc, TexKind, TexViews};
use mvs_render::image::write_png;
use mvs_render::look::{Look, LookFile};
use mvs_render::renderer::{Passes, Renderer, StaticMesh};
use mvs_render::scenery::level_emitters;
use mvs_render::stage::{to_world, transform};
use mvs_view::{FrameInput, FrameTime, MatchCamera, MaterialRef, MeshPose, ViewPoser};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

const USAGE: &str = "usage: preview [--store <dir>] --match <trace match> --tick <n> [--stage <stage>] --look <dir> \
                     [--clip <seconds>] [--fps 30] [--size 1920x1080] --out <dir>";

struct Args {
    store: PathBuf,
    name: String,
    tick: u32,
    stage: Option<String>,
    look: PathBuf,
    clip: Option<f64>,
    fps: u32,
    size: [u32; 2],
    out: PathBuf,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let (mut store, mut name, mut tick, mut stage, mut look, mut clip, mut out) =
        (None, None, None, None, None, None, None);
    let (mut fps, mut size) = (30, [1920, 1080]);
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or(format!("{a}: no value"));
        match a.as_str() {
            "--store" => store = Some(PathBuf::from(val()?)),
            "--match" => name = Some(val()?),
            "--tick" => tick = Some(val()?.parse().map_err(|e| format!("--tick: {e}"))?),
            "--stage" => stage = Some(val()?),
            "--look" => look = Some(PathBuf::from(val()?)),
            "--clip" => clip = Some(val()?.parse().map_err(|e| format!("--clip: {e}"))?),
            "--fps" => fps = val()?.parse().map_err(|e| format!("--fps: {e}"))?,
            "--size" => {
                let v = val()?;
                let (w, h) = v.split_once('x').ok_or("--size: WxH")?;
                size = [w.parse().map_err(|_| "--size: WxH")?, h.parse().map_err(|_| "--size: WxH")?];
            }
            "--out" => out = Some(PathBuf::from(val()?)),
            _ => return Err(format!("unknown argument {a}")),
        }
    }
    if !(1..=60).contains(&fps) || !60u32.is_multiple_of(fps) {
        return Err("--fps: a divisor of 60".into());
    }
    Ok(Args {
        store: store.unwrap_or_else(mvs_store::dev_store_dir),
        name: name.ok_or("--match: no value")?,
        tick: tick.ok_or("--tick: no value")?,
        stage,
        look: look.ok_or("--look: no value")?,
        clip,
        fps,
        size,
        out: out.ok_or("--out: no value")?,
    })
}

/// A look's mesh as a stage static.
fn static_of(m: &mvs_render::look::LookMesh) -> StaticMesh {
    StaticMesh {
        pose: MeshPose {
            mesh: m.mesh.as_str().into(),
            to_world: to_world(&transform(m.at, m.rotation, m.scale)),
            shown: vec![true; 64],
            materials: m
                .materials
                .iter()
                .map(|p| MaterialRef { path: p.as_str().into(), ..Default::default() })
                .collect(),
            ..Default::default()
        },
        lightmap: None,
        channels: 1,
        instances: None,
        receives_decals: false,
        sort: 0.0,
    }
}

fn run(a: &Args) -> Result<(), String> {
    let e = |x: &dyn std::fmt::Display| x.to_string();
    let file = LookFile::read(&a.look.join("look.json")).map_err(|x| e(&x))?;
    let st = mvs_store::Store::open(&a.store).map_err(|x| e(&x))?;
    let gpu = Gpu::new(None, false).map_err(|x| e(&x))?;
    let assets = Assets::open(mvs_store::Store::open(&a.store).map_err(|x| e(&x))?, &gpu).map_err(|x| e(&x))?;
    let mut r = Renderer::new(gpu, assets, a.size).map_err(|x| e(&x))?;
    r.passes = Passes::ALL;
    if let Some(p) = mvs_view::skins::hud_palette(&st).map_err(|x| e(&x))? {
        r.set_collection(mvs_view::skins::CENTRAL_PALETTE, &p.central_palette());
    }
    let path = mvs_sim_trace::run::default_traces().join(format!("{}.trace", a.name));
    let mut trace = mvs_trace::Trace::read(&path).map_err(|x| format!("{}: {x}", path.display()))?;
    mvs_sim_trace::drawn_with_cast(&mut trace.header.setup);
    let own = trace.header.stage.as_deref().unwrap_or(mvs_sim_trace::TRACE_STAGE).to_owned();
    let stage = a.stage.clone().unwrap_or_else(|| own.clone());
    // (the scenery's blocks read the World only where its stage is theirs)
    let world_on = stage == own;
    let (mut w, book) = mvs_sim_trace::world_for(&trace.header, &st)?;
    let mut poser = ViewPoser::new(&st, &w).map_err(|x| e(&x))?;
    let mut cam = MatchCamera::new(&w, f64::from(a.size[0]) / f64::from(a.size[1]));
    let mut fx = Fx::new(&st, trace.header.world_seed as u32).map_err(|x| e(&x))?;
    fx.begin_level(&level_emitters(&st, &stage).map_err(|x| e(&x))?, level_surfaces(&st, &stage).map_err(|x| e(&x))?)
        .map_err(|x| e(&x))?;
    r.set_stage(&stage).map_err(|x| e(&x))?;
    r.scenery_begin(Some(&mut fx.player), None, Some(cam.rig.center));
    r.statics.extend(file.meshes.iter().map(static_of));
    let mut look = Look::load(r.gpu(), r.assets(), &a.look).map_err(|x| e(&x))?;
    let (mut fighters, mut actors) = (Vec::new(), Vec::new());
    let ticks = a.clip.map_or(0, |s| (s * 60.0).round() as u32);
    let every = 60 / a.fps;
    let last = a.tick + ticks;
    let mut stepped = 0;
    std::fs::create_dir_all(&a.out).map_err(|x| format!("{}: {x}", a.out.display()))?;
    let out_tex = r
        .gpu()
        .texture(
            &TexDesc {
                kind: TexKind::D2,
                size: [a.size[0], a.size[1], 1],
                mips: 1,
                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                views: TexViews { srv: true, rtv: true, ..Default::default() },
            },
            &[],
        )
        .map_err(|x| e(&x))?;
    let mut frames = 0u32;
    loop {
        let now = u32::try_from(w.flow.tick.max(0)).unwrap_or(0);
        if now >= a.tick {
            if now == a.tick {
                // The look's particle systems, at its start.
                for p in &file.particles {
                    if fx.player.spawn(&p.system, p.at, p.rotation, p.scale, &[]).is_none() {
                        eprintln!("preview: {} not spawned", p.system);
                    }
                }
            }
            if (now - a.tick).is_multiple_of(every) {
                let t = (now - a.tick) as f64 / 60.0;
                let mut camera = cam.camera(1.0);
                file.camera.apply(&mut camera, t);
                poser.set_camera(cam.rig.root, cam.rig.pov.up);
                poser.present(&w, 1.0, &mut fighters, &mut actors);
                let ball = poser
                    .scenery_places()
                    .iter()
                    .find(|p| (p.block, p.part) == ("court", "ball"))
                    .and_then(|p| p.shown);
                r.scenery_place("court", "ball", ball);
                r.scenery_frame(1.0, Some(cam.rig.center), Some(&mut fx.player));
                fx.frame(&w, 1.0);
                r.floor_decals = fx.decals().to_vec();
                r.set_component_meshes(&fx.components);
                r.set_blast_box(&w.stage.blast);
                let forward = [0, 1, 2].map(|i| camera.translated_world_to_view[i][2]);
                fx.player.set_view(camera.origin, forward);
                if now == a.tick {
                    print_places(&camera, &w, &fighters);
                    for l in &r.lights.radial {
                        eprintln!(
                            "preview: stage light {} at [{:.0}, {:.0}, {:.0}] colour [{:.1}, {:.1}, {:.1}] radius {:.0}",
                            l.name,
                            l.position[0],
                            l.position[1],
                            l.position[2],
                            l.color[0],
                            l.color[1],
                            l.color[2],
                            1.0 / l.inv_radius
                        );
                    }
                }
                let base = r.lights.radial.len();
                r.lights.radial.extend(file.lights.iter().enumerate().map(|(k, l)| l.light(k, t)));
                look.time = t as f32;
                r.look = Some(look);
                let game = fx.player.game_time() as f32;
                let input = FrameInput {
                    tick: now,
                    alpha: 1.0,
                    time: FrameTime { game, real: now as f32 / 60.0, delta: 1.0 / 60.0 },
                    camera: &camera,
                    fighters: &fighters,
                    actors: &actors,
                    events: &w.events,
                };
                let drawn = r.draw(&input, Some(&mut fx.player), Some(&out_tex)).map(|_| ());
                look = r.look.take().expect("set above");
                r.lights.radial.truncate(base);
                drawn.map_err(|x| e(&x))?;
                let px = r.read_back(&out_tex).map_err(|x| e(&x))?;
                let name = if a.clip.is_some() { format!("frame_{frames:04}.png") } else { "still.png".into() };
                write_png(&a.out.join(name), a.size[0], a.size[1], &px)?;
                frames += 1;
            }
        }
        if now >= last {
            break;
        }
        if stepped >= trace.len() {
            eprintln!("preview: the trace ends at tick {now}");
            break;
        }
        let fr = trace.frame(stepped).map_err(|x| e(&x))?;
        let inputs: Vec<_> = fr.inputs.iter().map(|i| mvs_sim_trace::input_of(i, &w, &book)).collect();
        w.events.clear();
        mvs_sim_trace::page_calls(&mut w, &trace.header, stepped);
        w.step(&inputs);
        stepped += 1;
        poser.place(&w);
        poser.present(&w, 1.0, &mut fighters, &mut actors);
        r.scenery_tick(1.0 / 60.0, Some(&mut fx.player), world_on.then_some(&w));
        fx.camera = Some(CameraRoot::of(&cam.rig));
        fx.step(&w, &fighters, &actors).map_err(|x| e(&x))?;
        for k in fx.ko_anchors.drain(..) {
            cam.ko_anchor(k.y, k.z, k.hold);
        }
        cam.step(&w);
    }
    if a.clip.is_some() {
        clip(&a.out, a.fps, frames)?;
    }
    for m in r.gpu().debug_messages() {
        eprintln!("preview: debug layer {m}");
    }
    Ok(())
}

/// What a look file is written against: the camera, the blast box and where each fighter stands.
fn print_places(camera: &mvs_view::Camera, w: &mvs_sim::world::World, fighters: &[mvs_view::FighterPose]) {
    let f = [0, 1, 2].map(|i| camera.translated_world_to_view[i][2]);
    let o = camera.origin;
    eprintln!(
        "preview: camera at [{:.0}, {:.0}, {:.0}] looking [{:.2}, {:.2}, {:.2}]",
        o[0], o[1], o[2], f[0], f[1], f[2]
    );
    let b = &w.stage.blast;
    eprintln!("preview: blast box Y {:.0}..{:.0}, Z {:.0}..{:.0}", b.left, b.right, b.bottom, b.top);
    for p in fighters {
        if let Some(m) = p.meshes.first() {
            let t = &m.to_world;
            eprintln!("preview: fighter {} at [{:.0}, {:.0}, {:.0}]", p.fighter, t[3], t[7], t[11]);
        }
    }
}

/// The frames as `<out>/clip.mp4`, the frames deleted after.
fn clip(out: &Path, fps: u32, frames: u32) -> Result<(), String> {
    let mp4 = out.join("clip.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-framerate", &fps.to_string(), "-i"])
        .arg(out.join("frame_%04d.png"))
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "18"])
        .arg(&mp4)
        .status()
        .map_err(|x| format!("ffmpeg: {x}"))?;
    if !status.success() {
        return Err(format!("ffmpeg: {status}"));
    }
    for k in 0..frames {
        let _ = std::fs::remove_file(out.join(format!("frame_{k:04}.png")));
    }
    eprintln!("preview: {} ({frames} frames)", mp4.display());
    Ok(())
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)).and_then(|a| run(&a)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(x) => {
            eprintln!("preview: {x}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}
