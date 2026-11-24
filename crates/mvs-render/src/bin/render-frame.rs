//! Draws one held frame offscreen, a trace match stepped to a tick or a pose tape's frame, writes its PNG and times
//! it: GPU and CPU ms over `--frames` draws after `--warmup`.
//!
//! render-frame [--store <dir>] (--match <trace match> --tick <n> [--match-stage <stage>] | --pose-tape <file> [--tick <n>]
//!   | --stage-test <stage> --camera <frame.json>) [--size 1920x1080] [--adapter <text>] [--frames 500]
//!   [--warmup 50] [--target <name>]... [--particles <seed>] [--debug] --out <dir>
//!
//! `--match` steps the trace match `<MVS_TRACES>/<name>.trace` (its World and inputs) to `--tick`, the views placed
//! and the match camera stepped after each step, and draws the views at alpha 1 from that camera, with the last
//! step's events; `--match-stage` draws it on another stage than the trace's (a pose tape's, to compare with the
//! oracle's pictures of the match): the level, its emitters and surfaces only, the World and the camera stay the
//! trace's. `--fx` runs the match's effects too (Fx::step after each step, its ring-out anchors fed to the
//! camera) and hands the particle player's draws to the frame; with a pose tape or a stage test it spawns the stage's
//! Emitter actors alone, ticked 2 s. `--pose-tape` draws a frame the oracle recorded (its camera, views and actors).
//! Both draw the stage of the match or the tape.
//!
//! `--grow` (with `--match`) is the match warm-up's check: the match loaded as the game loads one, warmed
//! (Renderer::prewarm, Fx::warm; `--no-warm`: not), then stepped to `--tick` with a frame of every pass drawn after
//! each step; a frame that made a shader, state object, input layout, asset or particle material prints a line of
//! what it made.
//!
//! `--stage-test` draws a stage (Renderer::set_stage, as the other two) and Shaggy's body in its reference pose, from a captured frame's
//! camera (three's viewMatrix, projectionMatrix and cameraPosition under `view`).
//!
//! The heap allocations are counted (a global allocator that counts): a match's steps print the views', effects'
//! and camera's allocations per step (the World's step and the trace's reading not counted), and the timed frames'
//! line has `allocsPerFrame`, the renderer's draw alone.

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use mvs_d3d::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
use mvs_render::assets::Assets;
use mvs_render::fx::surfaces::level_surfaces;
use mvs_render::fx::{CameraRoot, Fx};
use mvs_render::gpu::{Gpu, GpuTimer, TexDesc, TexKind, TexViews, Texture};
use mvs_render::image::{diff, half_to_gamma, read_png, write_png};
use mvs_render::lighting;
use mvs_render::renderer::{FrameTimes, Passes, Renderer, Scalability};
use mvs_render::scenery::level_emitters;
use mvs_render::stage::{to_world, transform};
use mvs_view::{
    ActorPose, Camera, FighterPose, FrameInput, FrameTime, MatchCamera, MaterialRef, MeshPose, PoseTape, ViewPoser,
};
use serde::Deserialize;

/// The system allocator, its allocations and reallocations counted while `COUNTING` is set.
struct Counting;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards every call to the system allocator unchanged; the counter is an atomic.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: the caller's contract is the system allocator's.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: `p` came from `alloc` above, which is the system allocator's.
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: as `alloc`.
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// `f`'s result and the allocations it made (this thread's and any other's meanwhile).
fn counted<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let before = ALLOCS.load(Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    let r = f();
    COUNTING.store(false, Ordering::Relaxed);
    (r, ALLOCS.load(Ordering::Relaxed) - before)
}

/// Where the frame comes from.
enum Source {
    /// A trace match's World stepped to `tick`, posed by mvs-view; `fx`: with its effects; `stage`: the stage drawn
    /// (the trace's when None).
    Match { name: String, tick: u32, fx: bool, stage: Option<String> },
    /// A pose tape's frame at `tick` (its first when None).
    Tape { path: PathBuf, tick: Option<u32> },
    /// A stage's scenery and Shaggy, from a captured frame's camera.
    StageTest { stage: String, camera: PathBuf },
}

struct Args {
    store: PathBuf,
    source: Source,
    size: [u32; 2],
    adapter: Option<String>,
    frames: u32,
    warmup: u32,
    targets: Vec<String>,
    debug: bool,
    passes: Passes,
    /// `--aa <level>` and `--scale <percent>`: the Graphics screen's anti-aliasing level and resolution scale.
    scalability: Scalability,
    /// `--particles <seed>`: Shaggy's particle systems spawned by a player at this seed, drawn over the frame.
    particles: Option<u32>,
    /// `--game-offset <seconds>`: added to a match's GameTime. A capture's clock runs a harness-dependent 0.05
    /// to 0.15 s ahead of the ticks (the tapes' `time.game` minus tick / 60), so a time-pulsed material compares at
    /// that phase only with it.
    game_offset: f32,
    /// `--desaturation <v>`: MPC TutorialEnvironmentDesat's Desaturation forced to `v` (a stage's materials that read it).
    desaturation: Option<f32>,
    /// `--desat-probe`: the tutorial platform's mesh (SwordBlade_Mat, the one drawn material that reads
    /// TutorialEnvironmentDesat) placed in front of the camera, to show `--desaturation` on a stage.
    desat_probe: bool,
    /// `--fx` with a pose tape or a stage test: the stage's Emitter actors, ticked LEVEL_FX_STEPS steps.
    level_fx: bool,
    /// The lighting's flags: the light set's capture folder, the G-buffer dump lit alone, the picture compared.
    light: LightFlags,
    /// `--grow`: the warm-up's check instead of a picture; `warm` false with `--no-warm`.
    grow: bool,
    warm: bool,
    out: PathBuf,
}

/// `--light-set <dir>`: the lights and sky of a captured frame (lighting::capture) while the stage gives none.
/// `--gbuffer-dump <dir>`: no base pass: the captured frame's targets are lit alone (and timed). `--compare <png>`:
/// the difference of the first `--target` named (SceneColor when none is) from a picture of the same frame.
#[derive(Default)]
struct LightFlags {
    set: Option<PathBuf>,
    dump: Option<PathBuf>,
    compare: Option<PathBuf>,
    /// The target `--compare` reads.
    compare_target: String,
}

/// `--passes`: a comma list of captures, custom_depth, decals, lighting, fog, translucency, particles, post, or all.
fn passes_of(v: &str) -> Result<Passes, String> {
    if v == "all" {
        return Ok(Passes::ALL);
    }
    let mut p = Passes::default();
    for n in v.split(',').filter(|n| !n.is_empty()) {
        match n {
            "captures" => p.captures = true,
            "custom_depth" => p.custom_depth = true,
            "decals" => p.decals = true,
            "lighting" => p.lighting = true,
            "fog" => p.fog = true,
            "translucency" => p.translucency = true,
            "particles" => p.particles = true,
            "post" => p.post = true,
            _ => return Err(format!("--passes: {n} is not a pass")),
        }
    }
    Ok(p)
}

const USAGE: &str = "render-frame [--store <dir>] (--match <trace match> --tick <n> [--match-stage <stage>] | --pose-tape <file> [--tick <n>] \
| --stage-test <stage> --camera <frame.json>) [--fx] [--size 1920x1080] [--adapter <text>] [--frames 500] [--warmup 50] \
[--target scenecolor|gbuffera..gbuffere|final]... [--passes all|<pass>,...] [--light-set <dir>] [--gbuffer-dump <dir>] [--compare <png>] [--particles <seed>] [--game-offset <seconds>] [--grow [--no-warm]] [--aa <0..4>] [--scale <10..100>] [--debug] --out <dir>";

const TARGETS: [&str; 7] = ["scenecolor", "gbuffera", "gbufferb", "gbufferc", "gbufferd", "gbuffere", "final"];

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let (mut store, mut name, mut tape, mut tick, mut out, mut adapter) = (None, None, None, None, None, None);
    let (mut stage, mut camera, mut match_stage) = (None, None, None);
    let (mut size, mut frames, mut warmup, mut debug, mut targets) = ([1920, 1080], 500, 50, false, Vec::new());
    let mut fx = false;
    let mut passes = Passes::default();
    let mut scalability = Scalability::default();
    let mut particles = None;
    let mut desaturation = None;
    let mut desat_probe = false;
    let mut game_offset = 0.0;
    let mut light = LightFlags::default();
    let (mut grow, mut warm) = (false, true);
    let num = |v: Option<String>, what: &str| -> Result<u32, String> {
        let v = v.ok_or(format!("{what}: no value"))?;
        v.parse().map_err(|_| format!("{what}: {v} is not a count"))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--store" => store = Some(PathBuf::from(it.next().ok_or("--store: no value")?)),
            "--match" => name = Some(it.next().ok_or("--match: no value")?),
            "--pose-tape" => tape = Some(PathBuf::from(it.next().ok_or("--pose-tape: no value")?)),
            "--stage-test" => stage = Some(it.next().ok_or("--stage-test: no value")?),
            "--match-stage" => match_stage = Some(it.next().ok_or("--match-stage: no value")?),
            "--camera" => camera = Some(PathBuf::from(it.next().ok_or("--camera: no value")?)),
            "--tick" => tick = Some(num(it.next(), "--tick")?),
            "--fx" => fx = true,
            "--grow" => grow = true,
            "--no-warm" => warm = false,
            "--size" => {
                let v = it.next().ok_or("--size: no value")?;
                let (w, h) = v.split_once('x').ok_or(format!("--size: {v} is not WxH"))?;
                size = [num(Some(w.to_string()), "--size")?, num(Some(h.to_string()), "--size")?];
            }
            "--adapter" => adapter = Some(it.next().ok_or("--adapter: no value")?),
            "--frames" => frames = num(it.next(), "--frames")?,
            "--warmup" => warmup = num(it.next(), "--warmup")?,
            "--target" => {
                let t = it.next().ok_or("--target: no value")?;
                if !TARGETS.contains(&t.as_str()) {
                    return Err(format!("--target: {t} is not one of {TARGETS:?}"));
                }
                targets.push(t);
            }
            "--debug" => debug = true,
            "--aa" => scalability.aa = u8::try_from(num(it.next(), "--aa")?).map_err(|_| "--aa: 0..4")?,
            "--scale" => scalability.resolution_quality = f64::from(num(it.next(), "--scale")?),
            "--passes" => passes = passes_of(&it.next().ok_or("--passes: no value")?)?,
            "--game-offset" => {
                let v = it.next().ok_or("--game-offset: no value")?;
                game_offset = v.parse().map_err(|_| format!("--game-offset: {v} is not seconds"))?;
            }
            "--desaturation" => {
                let v = it.next().ok_or("--desaturation: no value")?;
                desaturation = Some(v.parse().map_err(|_| format!("--desaturation: {v} is not a number"))?);
            }
            "--desat-probe" => desat_probe = true,
            "--particles" => particles = Some(num(it.next(), "--particles")?),
            "--light-set" => light.set = Some(PathBuf::from(it.next().ok_or("--light-set: no value")?)),
            "--gbuffer-dump" => light.dump = Some(PathBuf::from(it.next().ok_or("--gbuffer-dump: no value")?)),
            "--compare" => light.compare = Some(PathBuf::from(it.next().ok_or("--compare: no value")?)),
            "--out" => out = Some(PathBuf::from(it.next().ok_or("--out: no value")?)),
            _ => return Err(format!("unknown argument {a}")),
        }
    }
    let source = match (name, tape, stage) {
        (Some(name), None, None) => {
            Source::Match { name, tick: tick.ok_or("--match needs --tick")?, fx, stage: match_stage }
        }
        (None, Some(path), None) => Source::Tape { path, tick },
        (None, None, Some(stage)) => Source::StageTest { stage, camera: camera.ok_or("--stage-test needs --camera")? },
        _ => return Err("one of --match, --pose-tape and --stage-test".to_string()),
    };
    let level_fx = fx && !matches!(source, Source::Match { .. });
    light.compare_target = targets.first().cloned().unwrap_or_else(|| "scenecolor".to_string());
    if targets.is_empty() {
        targets = TARGETS.iter().filter(|t| **t != "final" || passes.post).map(|s| s.to_string()).collect();
    }
    Ok(Args {
        store: store.unwrap_or_else(mvs_store::dev_store_dir),
        source,
        size,
        adapter,
        frames,
        warmup,
        targets,
        debug,
        passes,
        scalability,
        particles,
        desaturation,
        desat_probe,
        game_offset,
        level_fx,
        light,
        grow,
        warm,
        out: out.ok_or("--out: no value")?,
    })
}

/// The FMatrix3x4 of identity: a bone in its reference pose.
const IDENTITY_3X4: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// UE's place for three's metres (x, y up, z): cm, Z up.
fn ue_point(v: [f64; 3]) -> [f64; 3] {
    [100.0 * v[0], 100.0 * v[2], 100.0 * v[1]]
}

fn ue_dir(v: [f64; 3]) -> [f64; 3] {
    [v[0], v[2], v[1]]
}

/// The camera of a captured frame's `view` (three's column-major viewMatrix and projectionMatrix, cameraPosition).
fn camera_of(path: &Path) -> Result<Camera, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct View {
        view_matrix: [f64; 16],
        projection_matrix: [f64; 16],
        camera_position: [f64; 3],
    }
    #[derive(Deserialize)]
    struct Frame {
        view: View,
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let f: Frame = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let (vm, pm) = (&f.view.view_matrix, &f.view.projection_matrix);
    // three's element (row r, column c) is m[4c + r]; the view matrix's rows are the camera's right, up and back.
    let el = |m: &[f64; 16], r: usize, c: usize| m[4 * c + r];
    let row = |r: usize| ue_dir([el(vm, r, 0), el(vm, r, 1), el(vm, r, 2)]);
    let (right, up, back) = (row(0), row(1), row(2));
    let mut rot = [[0.0; 4]; 4];
    for i in 0..3 {
        rot[i] = [right[i], up[i], -back[i], 0.0];
    }
    rot[3][3] = 1.0;
    // A clip row of three's projection, taken on (x, y, z_back, 1) in metres: on UE's (x, y, z_forward, 1) in cm the
    // z column changes sign and the constant column scales by 100.
    let mut proj = [[0.0; 4]; 4];
    for (j, col) in (0..4).map(|j| (j, [el(pm, j, 0), el(pm, j, 1), -el(pm, j, 2), 100.0 * el(pm, j, 3)])) {
        for (i, v) in col.into_iter().enumerate() {
            proj[i][j] = v;
        }
    }
    Ok(Camera { origin: ue_point(f.view.camera_position), translated_world_to_view: rot, view_to_clip: proj })
}

/// Shaggy's body and the materials his default skin draws it with.
const SHAGGY: &str = "/Game/Panda_Main/Characters/Shaggy/Mesh/Shaggy.Shaggy";
const SHAGGY_MATERIALS: [&str; 2] = [
    "/Game/Panda_Main/Characters/Shaggy/Shaggy_Base_BLUE_MIC.Shaggy_Base_BLUE_MIC",
    "/Game/Panda_Main/Characters/Shaggy/Mesh/Shaggy_Eyes_MIC.Shaggy_Eyes_MIC",
];
/// Where the test stands him: on The Court's floor, facing the camera.
const SHAGGY_AT: ([f64; 3], f64) = ([0.0, 330.0, 0.0], 0.0);

/// The stage test's Shaggy: his body in its reference pose.
fn stage_test_shaggy(r: &Renderer) -> Result<ActorPose, String> {
    let shaggy = r.assets().mesh(SHAGGY).map_err(|e| e.to_string())?;
    let (at, yaw) = SHAGGY_AT;
    Ok(ActorPose {
        meshes: vec![MeshPose {
            mesh: Arc::from(SHAGGY),
            to_world: to_world(&transform(at, [0.0, yaw, 0.0], [1.0; 3])),
            bones: vec![IDENTITY_3X4; shaggy.bones as usize],
            materials: SHAGGY_MATERIALS
                .iter()
                .map(|p| MaterialRef { path: Arc::from(*p), ..Default::default() })
                .collect(),
            ..Default::default()
        }],
        ..Default::default()
    })
}

/// The frame a source gives: the stage to draw it on and what FrameInput borrows.
struct Held {
    stage: String,
    camera: Camera,
    tick: u32,
    alpha: f32,
    game: f32,
    fighters: Vec<FighterPose>,
    actors: Vec<ActorPose>,
    events: Vec<mvs_sim::Event>,
    /// The match's effects (`--fx`).
    fx: Option<Fx>,
    /// The match's blast box (World.blast) the stage's drawn edge follows; None: the map's.
    blast: Option<mvs_sim::world::BlastBounds>,
    /// The camera rig's centre (UE) at the level's begin and at the frame (a stage tape's `<tape>.camera.json`).
    centres: [Option<[f64; 3]>; 2],
}

/// A stage pose tape's camera centres (`<tape>.camera.json`), at the match's tick 0 and at
/// `tick`; none without the file.
fn tape_centres(path: &Path, tick: u32) -> Result<[Option<[f64; 3]>; 2], String> {
    #[derive(Deserialize)]
    struct Side {
        frames: Vec<SideFrame>,
    }
    #[derive(Deserialize)]
    struct SideFrame {
        tick: u32,
        center: [f64; 3],
    }
    let mut side = path.as_os_str().to_owned();
    side.push(".camera.json");
    let Ok(bytes) = std::fs::read(&side) else { return Ok([None, None]) };
    let s: Side = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", Path::new(&side).display()))?;
    let at = |t: u32| s.frames.iter().find(|f| f.tick == t).map(|f| f.center);
    Ok([at(0), at(tick)])
}

/// A pose tape's frame at `tick`, its first when None.
fn tape_frame(path: &Path, tick: Option<u32>) -> Result<Held, String> {
    let tape = PoseTape::open(path).map_err(|e| e.to_string())?;
    let tick = tick.unwrap_or(*tape.ticks().start());
    let f =
        tape.frame(tick).map_err(|e| e.to_string())?.ok_or(format!("{}: no frame at tick {tick}", path.display()))?;
    Ok(Held {
        stage: tape.header.stage_key().to_owned(),
        camera: f.camera,
        tick: f.tick,
        alpha: f.alpha,
        game: f.time.game,
        fighters: f.fighters,
        actors: f.actors,
        events: Vec::new(),
        fx: None,
        blast: None,
        centres: tape_centres(path, f.tick)?,
    })
}

/// A trace match stepped to `tick` on the stage its header names (`stage`: another one's level instead): the views
/// placed, the scenery's blocks ticked (with the effects' particle player under `fx`), the effects stepped (`fx`)
/// and the camera stepped after each step (as each presented tick), drawn at alpha 1 with the last step's events. The
/// stage is set on `r` and its scenery begun, ticked and framed here.
#[allow(clippy::too_many_arguments)]
fn match_frame(
    store: &mvs_store::Store,
    r: &mut Renderer,
    name: &str,
    tick: u32,
    size: [u32; 2],
    fx: bool,
    stage: Option<&str>,
) -> Result<Held, String> {
    let path = mvs_sim_trace::run::default_traces().join(format!("{name}.trace"));
    let mut trace = mvs_trace::Trace::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    mvs_sim_trace::drawn_with_cast(&mut trace.header.setup);
    let own = trace.header.stage.as_deref().unwrap_or(mvs_sim_trace::TRACE_STAGE);
    let stage = stage.unwrap_or(own);
    // (the blocks read the World only where its stage is the scenery's)
    let world_on = stage == own;
    let (mut w, book) = mvs_sim_trace::world_for(&trace.header, store)?;
    let mut poser = ViewPoser::new(store, &w).map_err(|e| e.to_string())?;
    let mut cam = MatchCamera::new(&w, f64::from(size[0]) / f64::from(size[1]));
    let mut fx =
        if fx { Some(Fx::new(store, trace.header.world_seed as u32).map_err(|e| e.to_string())?) } else { None };
    // The level's Emitter actors on the particle player, before the first step.
    if let Some(fx) = &mut fx {
        let emitters = level_emitters(store, stage).map_err(|e| e.to_string())?;
        let surfaces = level_surfaces(store, stage).map_err(|e| e.to_string())?;
        fx.begin_level(&emitters, surfaces).map_err(|e| e.to_string())?;
        eprintln!("render-frame: {} level emitters, {} spawned", emitters.len(), fx.level_live());
    }
    r.set_stage(stage).map_err(|e| e.to_string())?;
    r.scenery_begin(fx.as_mut().map(|f| &mut f.player), None, Some(cam.rig.center));
    let (mut fighters, mut actors) = (Vec::new(), Vec::new());
    let mut stepped = 0;
    let mut allocs = [0usize; 5];
    // (frame i of the trace is the inputs of tick i + 1)
    while i64::from(w.flow.tick) < i64::from(tick) {
        if stepped >= trace.len() {
            return Err(format!("{name}: the trace ends at tick {}", w.flow.tick));
        }
        let fr = trace.frame(stepped).map_err(|e| e.to_string())?;
        let inputs: Vec<_> = fr.inputs.iter().map(|r| mvs_sim_trace::input_of(r, &w, &book)).collect();
        w.events.clear();
        // (a tutorial trace's page calls: IntroComplete, the end screen's DesaturateEnvironment)
        mvs_sim_trace::page_calls(&mut w, &trace.header, stepped);
        w.step(&inputs);
        stepped += 1;
        let mut n = [0; 5];
        ((), n[0]) = counted(|| poser.place(&w));
        // (the effects ride the poses at the latest tick; a ring-out's anchor is the camera's before its step; the
        // scenery's tick before the particles')
        if fx.is_some() {
            ((), n[1]) = counted(|| poser.present(&w, 1.0, &mut fighters, &mut actors));
        }
        ((), n[2]) = counted(|| r.scenery_tick(1.0 / 60.0, fx.as_mut().map(|f| &mut f.player), world_on.then_some(&w)));
        if let Some(fx) = &mut fx {
            fx.camera = Some(CameraRoot::of(&cam.rig));
            let s;
            (s, n[3]) = counted(|| fx.step(&w, &fighters, &actors));
            s.map_err(|e| e.to_string())?;
            for a in fx.ko_anchors.drain(..) {
                cam.ko_anchor(a.y, a.z, a.hold);
            }
        }
        ((), n[4]) = counted(|| cam.step(&w));
        // (the first steps load the views' and effects' assets: not counted)
        if stepped > ALLOC_SKIP {
            for (a, b) in allocs.iter_mut().zip(n) {
                *a += b;
            }
        }
    }
    if stepped > ALLOC_SKIP {
        let k = (stepped - ALLOC_SKIP) as f64;
        let [place, present, scenery, fx, cam] = allocs.map(|a| a as f64 / k);
        eprintln!(
            "render-frame: allocations per step over {k} steps: place {place:.1}, present {present:.1}, scenery {scenery:.1}, fx {fx:.1}, camera {cam:.1}"
        );
    }
    let camera = cam.camera(1.0);
    poser.set_camera(cam.rig.root, cam.rig.pov.up);
    poser.present(&w, 1.0, &mut fighters, &mut actors);
    // (the stage items whose meshes are the scenery's: every frame, none placed is hidden)
    let ball = poser.scenery_places().iter().find(|p| (p.block, p.part) == ("court", "ball")).and_then(|p| p.shown);
    r.scenery_place("court", "ball", ball);
    r.scenery_frame(1.0, Some(cam.rig.center), fx.as_mut().map(|f| &mut f.player));
    // (the frame's part of the effects: the decals, at alpha 1)
    if let Some(fx) = &mut fx {
        fx.frame(&w, 1.0);
    }
    if let Some(fx) = &fx {
        eprintln!("render-frame: {} level emitter systems live at tick {tick}", fx.level_live());
    }
    if !poser.undrawn_actors().is_empty() {
        eprintln!("render-frame: actors not drawn (no ported rule): {:?}", poser.undrawn_actors());
    }
    Ok(Held {
        stage: stage.to_owned(),
        camera,
        tick,
        alpha: 1.0,
        // (View GameTime is the particle player's clock, the steps it was ticked)
        game: fx.as_ref().map_or(tick as f32 / 60.0, |f| f.player.game_time() as f32),
        fighters,
        actors,
        events: std::mem::take(&mut w.events),
        fx,
        blast: Some(w.stage.blast),
        centres: [None, None],
    })
}

/// The steps of a match not counted in its allocations per step (its assets' loads).
const ALLOC_SKIP: usize = 120;

/// The steps the level's Emitter actors run before a pose tape's or a stage test's frame (`--fx`): 2 s.
const LEVEL_FX_STEPS: u32 = 120;

/// The stage's Emitter actors on a particle player of seed 0, ticked LEVEL_FX_STEPS steps.
fn level_fx_of(store: &mvs_store::Store, stage: &str) -> Result<Fx, String> {
    let mut fx = Fx::new(store, 0).map_err(|e| e.to_string())?;
    let emitters = level_emitters(store, stage).map_err(|e| e.to_string())?;
    fx.begin_level(&emitters, level_surfaces(store, stage).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    for _ in 0..LEVEL_FX_STEPS {
        fx.player.tick(1.0 / 60.0);
    }
    eprintln!(
        "render-frame: {} level emitters, {} systems live after {LEVEL_FX_STEPS} steps",
        emitters.len(),
        fx.level_live()
    );
    Ok(fx)
}

/// Target `name`'s difference from `png` (a picture of the same frame: the trial's picture map, the oracle's final):
/// numbers on stderr and the difference picture `<name>-diff.png`.
fn compare(r: &Renderer, name: &str, final_out: Option<&Texture>, png: &Path, out: &Path) -> Result<(), String> {
    let [w, h] = target_size(r, name, final_out);
    let ours = picture(r, name, final_out)?;
    let (pw, ph, theirs) = read_png(png)?;
    if [pw, ph] != [w, h] {
        return Err(format!("{}: {pw} x {ph}, the frame's {w} x {h}", png.display()));
    }
    let d = diff(&ours, &theirs, w, h);
    write_png(&out.join(format!("{name}-diff.png")), w, h, &d.picture)?;
    let grid: Vec<String> = d.grid.iter().map(|g| format!("{g:.2}")).collect();
    eprintln!(
        "render-frame: {name} vs {}: mean |diff| {:.3} / 255, pixels over 8: {:.2} %, over 32: {:.2} %, bias rgb {:.2} {:.2} {:.2}, 4x4 grid [{}]",
        png.display(),
        d.mean,
        100.0 * d.over8,
        100.0 * d.over32,
        d.bias[0],
        d.bias[1],
        d.bias[2],
        grid.join(" ")
    );
    Ok(())
}

/// `--gbuffer-dump`: the captured frame's targets lit alone by the light set, its SceneColor written (and
/// compared), and the lighting pass timed over `--frames`.
fn light_dump(r: &Renderer, dir: &Path, camera: &Camera, args: &Args) -> Result<(), String> {
    let gpu = r.gpu();
    let base = lighting::capture::load_targets(gpu, dir, &r.targets).map_err(|e| e.to_string())?;
    let masks = lighting::capture::stencil_values(dir).map_err(|e| e.to_string())?;
    let mut l = lighting::Lighting::new(gpu, r.assets()).map_err(|e| e.to_string())?;
    l.light(gpu, &r.targets, &r.lights, camera, masks, None).map_err(|e| e.to_string())?;
    gpu.finish().map_err(|e| e.to_string())?;
    for m in gpu.debug_messages() {
        eprintln!("render-frame: debug layer {m}");
    }
    write_target(r, "scenecolor", None, &args.out)?;
    if let Some(png) = &args.light.compare {
        compare(r, "scenecolor", None, png, &args.out)?;
    }
    let mut timer = GpuTimer::new(gpu, 4).map_err(|e| e.to_string())?;
    let (mut gpu_ms, mut cpu_ms) = (Vec::new(), Vec::new());
    for k in 0..args.warmup + args.frames {
        lighting::capture::restore_scene_color(gpu, &r.targets, &base);
        timer.begin_frame(gpu);
        timer.mark(gpu, "restore");
        let start = Instant::now();
        l.light(gpu, &r.targets, &r.lights, camera, masks, None).map_err(|e| e.to_string())?;
        let cpu = start.elapsed().as_secs_f64() * 1e3;
        timer.mark(gpu, "lighting");
        timer.end_frame(gpu);
        gpu.finish().map_err(|e| e.to_string())?;
        let p = timer.read(gpu);
        if k < args.warmup {
            continue;
        }
        cpu_ms.push(cpu);
        if let Some(ms) = p.and_then(|p| p.iter().find(|x| x.0 == "lighting").map(|x| x.1)) {
            gpu_ms.push(ms);
        }
    }
    if args.frames > 0 {
        println!(
            "{{\"adapter\":{:?},\"source\":\"gbuffer dump\",\"frames\":{},\"lightingGpuMedianMs\":{:.3},\"lightingCpuMedianMs\":{:.3}}}",
            gpu.adapter,
            gpu_ms.len(),
            median(&mut gpu_ms),
            median(&mut cpu_ms)
        );
    }
    Ok(())
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// gbuffera's R10G10B10A2 as 16-bit RGBA, each channel widened to 16 bits.
fn write_png16(path: &Path, w: u32, h: u32, px: &[u8]) -> Result<(), String> {
    let mut out = Vec::with_capacity(px.len() * 2);
    for p in px.as_chunks::<4>().0 {
        let p = u32::from_le_bytes(*p);
        for c in 0..3 {
            let v = (p >> (10 * c)) & 0x3ff;
            out.extend_from_slice(&(((v << 6) | (v >> 4)) as u16).to_be_bytes());
        }
        out.extend_from_slice(&(((p >> 30) * 21845) as u16).to_be_bytes());
    }
    let f = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Sixteen);
    enc.write_header().and_then(|mut wr| wr.write_image_data(&out)).map_err(|e| format!("{}: {e}", path.display()))
}

/// Target `name`'s bytes.
fn read_target(r: &Renderer, name: &str, out: Option<&Texture>) -> Result<Vec<u8>, String> {
    let t = &r.targets;
    let tex: &Texture = match name {
        "scenecolor" => &t.scene_color,
        "gbuffera" => &t.gbuffer[0],
        "gbufferb" => &t.gbuffer[1],
        "gbufferc" => &t.gbuffer[2],
        "gbufferd" => &t.gbuffer[3],
        "gbuffere" => &t.gbuffer[4],
        "final" => out.ok_or("--target final: no output drawn")?,
        _ => unreachable!("checked by parse"),
    };
    r.read_back(tex).map_err(|e| e.to_string())
}

/// Target `name` as 8-bit RGBA (SceneColor gamma-encoded, gbuffera's 10-bit channels cut to 8).
fn picture(r: &Renderer, name: &str, out: Option<&Texture>) -> Result<Vec<u8>, String> {
    let px = read_target(r, name, out)?;
    Ok(match name {
        "scenecolor" => half_to_gamma(&px),
        "final" => px,
        "gbuffera" => px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let p = u32::from_le_bytes(*p);
                [(p >> 2) as u8, (p >> 12) as u8, (p >> 22) as u8, ((p >> 30) * 85) as u8]
            })
            .collect(),
        // BGRA8 to RGBA.
        _ => px.as_chunks::<4>().0.iter().flat_map(|p| [p[2], p[1], p[0], p[3]]).collect(),
    })
}

/// The size of target `name`: the frame's for `final` (the post chain's output), else the scene targets' (the view's,
/// below the frame at a resolution scale under 100).
fn target_size(r: &Renderer, name: &str, out: Option<&Texture>) -> [u32; 2] {
    match (name, out) {
        ("final", Some(t)) => [t.desc.size[0], t.desc.size[1]],
        _ => r.targets.size,
    }
}

fn write_target(r: &Renderer, name: &str, out: Option<&Texture>, dir: &Path) -> Result<(), String> {
    let [w, h] = target_size(r, name, out);
    let path = dir.join(format!("{name}.png"));
    match name {
        "gbuffera" => write_png16(&path, w, h, &read_target(r, name, out)?),
        _ => write_png(&path, w, h, &picture(r, name, out)?),
    }?;
    eprintln!("render-frame: {}", path.display());
    Ok(())
}

fn run(args: &Args) -> Result<(), String> {
    let Args {
        store,
        source,
        size,
        adapter,
        frames,
        warmup,
        targets,
        debug,
        passes,
        scalability,
        particles,
        desaturation,
        desat_probe,
        game_offset,
        level_fx,
        light,
        grow: _,
        warm: _,
        out,
    } = args;
    let st = mvs_store::Store::open(store).map_err(|e| e.to_string())?;
    let gpu = Gpu::new(adapter.as_deref(), *debug).map_err(|e| e.to_string())?;
    eprintln!("render-frame: {} on {}", store.display(), gpu.adapter);
    let assets =
        Assets::open(mvs_store::Store::open(store).map_err(|e| e.to_string())?, &gpu).map_err(|e| e.to_string())?;
    let mut r = Renderer::new(gpu, assets, *size).map_err(|e| e.to_string())?;
    r.passes = *passes;
    r.set_scalability(*scalability).map_err(|e| e.to_string())?;
    // MPC_CentralPalette as set at the start with the default UI preferences; a store with no
    // HUD bundle keeps the material entries' rows.
    if let Some(p) = mvs_view::skins::hud_palette(&st).map_err(|e| e.to_string())? {
        r.set_collection(mvs_view::skins::CENTRAL_PALETTE, &p.central_palette());
    }
    r.force_tutorial_desaturation(*desaturation);
    let mut held = match source {
        Source::Match { name, tick, fx, stage } => {
            let mut held = match_frame(&st, &mut r, name, *tick, *size, *fx, stage.as_deref())?;
            held.game += *game_offset;
            held
        }
        Source::Tape { path, tick } => tape_frame(path, *tick)?,
        Source::StageTest { stage, camera } => Held {
            stage: stage.clone(),
            camera: camera_of(camera)?,
            tick: 0,
            alpha: 1.0,
            game: 0.0,
            fighters: Vec::new(),
            actors: Vec::new(),
            events: Vec::new(),
            fx: None,
            blast: None,
            centres: [None, None],
        },
    };
    if !matches!(source, Source::Match { .. }) {
        r.set_stage(&held.stage).map_err(|e| e.to_string())?;
    }
    if let Some(b) = &held.blast {
        r.set_blast_box(b);
    }
    if let Source::StageTest { .. } = source {
        held.actors.push(stage_test_shaggy(&r)?);
    }
    if *desat_probe {
        r.statics.push(desat_probe_mesh(&held.camera));
    }
    if *level_fx {
        held.fx = Some(level_fx_of(r.assets().store(), &held.stage)?);
    }
    // The scenery's blocks from the level's begin to the frame's tick, a step per world tick (the blocks, then the
    // particle player's tick), then the frame. A
    // match's were stepped with its World (match_frame).
    // Unverified: the level's Emitter actors are ticked LEVEL_FX_STEPS before the scenery begins, not
    // together with the scenery from the begin to the frame.
    if !matches!(source, Source::Match { .. }) {
        let mut level = if *level_fx { held.fx.as_mut().map(|f| &mut f.player) } else { None };
        r.scenery_begin(level.as_deref_mut(), None, held.centres[0]);
        for _ in 0..held.tick {
            r.scenery_tick(1.0 / 60.0, level.as_deref_mut(), None);
            if let Some(player) = level.as_deref_mut() {
                player.tick(1.0 / 60.0);
            }
        }
        r.scenery_frame(f64::from(held.alpha), held.centres[1], level);
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    if let Some(dir) = &light.set {
        r.lights = lighting::capture::light_set(r.gpu(), dir).map_err(|e| e.to_string())?;
        eprintln!(
            "render-frame: light set {}: {} directional, {} radial",
            dir.display(),
            r.lights.directional.len(),
            r.lights.radial.len()
        );
    }
    if let Some(dir) = &light.dump {
        return light_dump(&r, dir, &held.camera, args);
    }
    // (the frame's events only in its first draw: the effects they start are started once)
    let input = |k: u32| FrameInput {
        tick: held.tick,
        alpha: held.alpha,
        time: FrameTime { game: held.game + k as f32 / 60.0, real: held.game + k as f32 / 60.0, delta: 1.0 / 60.0 },
        camera: &held.camera,
        fighters: &held.fighters,
        actors: &held.actors,
        events: if k == 0 { &held.events } else { &[] },
    };
    // The particles drawn: the match's effects (`--fx`) or Shaggy's systems on a grid (`--particles`).
    let mut grid = match particles {
        Some(_) if held.fx.is_some() => return Err("--fx and --particles each give the frame's particles".into()),
        Some(seed) => Some(shaggy_particles(&r, &held.camera, *seed)?),
        None => None,
    };
    let mut vfx = match &mut held.fx {
        Some(fx) => {
            r.floor_decals = fx.decals().to_vec();
            r.set_component_meshes(&fx.components);
            let forward = [0, 1, 2].map(|i| held.camera.translated_world_to_view[i][2]);
            fx.player.set_view(held.camera.origin, forward);
            Some(&mut fx.player)
        }
        None => grid.as_mut(),
    };
    // `--target final`: the post chain's picture, drawn into an RGBA8 target of the frame's size.
    let final_out = if targets.iter().any(|t| t == "final") {
        if !passes.post {
            return Err("--target final needs the post pass (--passes ...,post)".to_string());
        }
        let desc = TexDesc {
            kind: TexKind::D2,
            size: [size[0], size[1], 1],
            mips: 1,
            format: DXGI_FORMAT_R8G8B8A8_UNORM,
            views: TexViews { srv: true, rtv: true, ..Default::default() },
        };
        Some(r.gpu().texture(&desc, &[]).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let first = Instant::now();
    r.draw(&input(0), vfx.as_deref_mut(), final_out.as_ref()).map_err(|e| e.to_string())?;
    r.gpu().finish().map_err(|e| e.to_string())?;
    for m in r.gpu().debug_messages() {
        eprintln!("render-frame: debug layer {m}");
    }
    eprintln!(
        "render-frame: first frame {:.1} ms, {} draws, {} shaders",
        first.elapsed().as_secs_f64() * 1e3,
        r.draw_list().opaque.len(),
        r.gpu().shader_count()
    );
    for t in targets {
        write_target(&r, t, final_out.as_ref(), out)?;
    }
    if let Some(png) = &light.compare {
        compare(&r, &light.compare_target, final_out.as_ref(), png, out)?;
    }
    let (mut gpu_ms, mut cpu_ms, mut passes) = (Vec::new(), Vec::new(), Vec::<(&str, Vec<f64>)>::new());
    let mut allocs = 0;
    for k in 0..warmup + frames {
        let (times, n) = counted(|| r.draw(&input(k + 1), vfx.as_deref_mut(), None));
        let FrameTimes { cpu, gpu, passes: p } = times.map_err(|e| e.to_string())?;
        // One frame in flight: the timer reads the frame before.
        r.gpu().finish().map_err(|e| e.to_string())?;
        if k < *warmup {
            continue;
        }
        allocs += n;
        cpu_ms.push(cpu);
        if let Some(g) = gpu {
            gpu_ms.push(g);
            for (name, ms) in p {
                match passes.iter_mut().find(|x| x.0 == name) {
                    Some(x) => x.1.push(ms),
                    None => passes.push((name, vec![ms])),
                }
            }
        }
    }
    if *frames > 0 {
        let line: Vec<String> = passes.iter_mut().map(|(n, v)| format!("{n} {:.3}", median(v))).collect();
        println!(
            "{{\"adapter\":{:?},\"frames\":{},\"gpuMedianMs\":{:.3},\"cpuMedianMs\":{:.3},\"allocsPerFrame\":{:.1},\"passes\":{:?}}}",
            r.gpu().adapter,
            gpu_ms.len(),
            median(&mut gpu_ms),
            median(&mut cpu_ms),
            allocs as f64 / f64::from(*frames),
            line.join(", ")
        );
    }
    Ok(())
}

/// `--desat-probe`: TUT_Platform_A_01 with SwordBlade_Mat, 1200 cm along the camera's forward axis.
fn desat_probe_mesh(cam: &Camera) -> mvs_render::renderer::StaticMesh {
    use mvs_view::{MaterialRef, MeshPose};
    let f = [0, 1, 2].map(|i| cam.translated_world_to_view[i][2]);
    let at = [0, 1, 2].map(|i| (cam.origin[i] + 1200.0 * f[i]) as f32);
    let m = |s: &str| std::sync::Arc::<str>::from(s);
    let pose = MeshPose {
        mesh: m("/Game/Panda_Main/Maps/Tutorials/Meshes/TUT_Platform_A_01.TUT_Platform_A_01"),
        lod: 0,
        to_world: [1.25, 0.0, 0.0, at[0], 0.0, 1.25, 0.0, at[1], 0.0, 0.0, 1.25, at[2]],
        bones: Vec::new(),
        shown: vec![true; 8],
        materials: vec![MaterialRef {
            path: m("/Game/Panda_Main/Maps/Map01_LionWatch/SwordBlade_Mat.SwordBlade_Mat"),
            scalars: Vec::new(),
            vectors: Vec::new(),
            textures: Vec::new(),
        }],
        custom_stencil: 0,
        capture_skip: false,
        no_decals: false,
        morphs: Vec::new(),
    };
    mvs_render::renderer::StaticMesh {
        pose,
        lightmap: None,
        channels: 1,
        instances: None,
        receives_decals: false,
        sort: 0.0,
    }
}

/// The ticks the particle test runs its systems before the frame.
const PARTICLE_TICKS: u32 = 12;

/// `--particles`: a player with every particle system Shaggy's parts load, spawned on a grid in front of the camera
/// around where he stands and ticked PARTICLE_TICKS frames; the frames draw it through Renderer::draw (the particles'
/// check while the poser does not spawn them).
fn shaggy_particles(r: &Renderer, cam: &Camera, seed: u32) -> Result<mvs_vfx::Player, String> {
    let mut player = mvs_vfx::Player::new(r.assets().store(), seed).map_err(|e| e.to_string())?;
    let mut names: Vec<String> = player
        .systems()
        .filter(|(_, owners)| owners.iter().any(|o| o.to_ascii_lowercase().contains("shaggy")))
        .map(|(n, _)| n.to_string())
        .collect();
    names.sort();
    let forward = [0, 1, 2].map(|i| cam.translated_world_to_view[i][2]);
    player.set_view(cam.origin, forward);
    let columns = (names.len() as f64).sqrt().ceil().max(1.0) as usize;
    let rows = names.len().div_ceil(columns).max(1);
    let (at, _) = SHAGGY_AT;
    let mut spawned = 0;
    for (k, name) in names.iter().enumerate() {
        let (c, row) = ((k % columns) as f64, (k / columns) as f64);
        let x = at[0] - 600.0 + 1200.0 * (c + 0.5) / columns as f64;
        let z = at[2] + 50.0 + 400.0 * (rows as f64 - row - 0.5) / rows as f64;
        if player.spawn(name, [x, at[1], z], [0.0; 3], [1.0; 3], &[]).is_some() {
            spawned += 1;
        }
    }
    for _ in 0..PARTICLE_TICKS {
        player.tick(1.0 / 60.0);
    }
    eprintln!("render-frame: {spawned} of {} Shaggy systems spawned", names.len());
    Ok(player)
}

/// `--grow`: the match `name` loaded as the game's match loop loads one (mvs-app run.rs MatchLoop::new: the views, the
/// effects and the level's emitters, the stage, the scenery's begin), warmed unless `--no-warm`, then stepped to tick
/// `until` as the loop's present_tick and draw run (alpha 1), each frame drawn with every pass; prints what each
/// frame made.
fn grow_run(args: &Args, name: &str, until: u32) -> Result<(), String> {
    let e = |e: &dyn std::fmt::Display| e.to_string();
    let st = mvs_store::Store::open(&args.store).map_err(|x| e(&x))?;
    let gpu = Gpu::new(args.adapter.as_deref(), args.debug).map_err(|x| e(&x))?;
    eprintln!("render-frame: {} on {}", args.store.display(), gpu.adapter);
    let assets = Assets::open(mvs_store::Store::open(&args.store).map_err(|x| e(&x))?, &gpu).map_err(|x| e(&x))?;
    let mut r = Renderer::new(gpu, assets, args.size).map_err(|x| e(&x))?;
    r.passes = Passes::ALL;
    let path = mvs_sim_trace::run::default_traces().join(format!("{name}.trace"));
    let trace = mvs_trace::Trace::read(&path).map_err(|x| format!("{}: {x}", path.display()))?;
    let (mut w, book) = mvs_sim_trace::world_for(&trace.header, &st)?;
    let mut poser = ViewPoser::new(&st, &w).map_err(|x| e(&x))?;
    let mut cam = MatchCamera::new(&w, f64::from(args.size[0]) / f64::from(args.size[1]));
    let mut fx = Fx::new(&st, trace.header.world_seed as u32).map_err(|x| e(&x))?;
    let stage = trace.header.stage.as_deref().unwrap_or(mvs_sim_trace::TRACE_STAGE);
    fx.begin_level(&level_emitters(&st, stage).map_err(|x| e(&x))?, level_surfaces(&st, stage).map_err(|x| e(&x))?)
        .map_err(|x| e(&x))?;
    r.set_stage(stage).map_err(|x| e(&x))?;
    r.scenery_begin(Some(&mut fx.player), None, Some(cam.rig.center));
    let loaded = r.census();
    if args.warm {
        let t = Instant::now();
        let [meshes, emitters, decals] = r.prewarm_match(&w, &poser, &mut fx).map_err(|x| e(&x))?;
        eprintln!(
            "render-frame: warm-up of {meshes} meshes, {emitters} emitters and {decals} decals {:.0} ms: {}",
            t.elapsed().as_secs_f64() * 1e3,
            loaded.grown(&r.census()).join("; ")
        );
    }
    let mut before = r.census();
    let (mut fighters, mut actors) = (Vec::new(), Vec::new());
    let mut grew = 0;
    for k in 0..trace.len() {
        if i64::from(w.flow.tick) >= i64::from(until) {
            break;
        }
        let fr = trace.frame(k).map_err(|x| e(&x))?;
        let inputs: Vec<_> = fr.inputs.iter().map(|r| mvs_sim_trace::input_of(r, &w, &book)).collect();
        w.events.clear();
        w.step(&inputs);
        // (run.rs present_tick)
        poser.place(&w);
        poser.set_camera(cam.rig.root, cam.rig.pov.up);
        poser.present(&w, 1.0, &mut fighters, &mut actors);
        r.scenery_tick(1.0 / 60.0, Some(&mut fx.player), Some(&w));
        fx.camera = Some(CameraRoot::of(&cam.rig));
        fx.step(&w, &fighters, &actors).map_err(|x| e(&x))?;
        for a in fx.ko_anchors.drain(..) {
            cam.ko_anchor(a.y, a.z, a.hold);
        }
        cam.step(&w);
        // (run.rs draw)
        let camera = cam.camera(1.0);
        poser.set_camera(cam.rig.root, cam.rig.pov.up);
        poser.present(&w, 1.0, &mut fighters, &mut actors);
        // (the stage items whose meshes are the scenery's: every frame, none placed is hidden)
        let ball = poser.scenery_places().iter().find(|p| (p.block, p.part) == ("court", "ball")).and_then(|p| p.shown);
        r.scenery_place("court", "ball", ball);
        r.scenery_frame(1.0, Some(cam.rig.center), Some(&mut fx.player));
        fx.frame(&w, 1.0);
        r.floor_decals = fx.decals().to_vec();
        r.set_component_meshes(&fx.components);
        r.set_blast_box(&w.stage.blast);
        let forward = [0, 1, 2].map(|i| camera.translated_world_to_view[i][2]);
        fx.player.set_view(camera.origin, forward);
        let tick = u32::try_from(w.flow.tick.max(0)).unwrap_or(0);
        let game = fx.player.game_time() as f32;
        let input = FrameInput {
            tick,
            alpha: 1.0,
            time: FrameTime { game, real: tick as f32 / 60.0, delta: 1.0 / 60.0 },
            camera: &camera,
            fighters: &fighters,
            actors: &actors,
            events: &w.events,
        };
        r.draw(&input, Some(&mut fx.player), None).map_err(|x| e(&x))?;
        let now = r.census();
        let g = before.grown(&now);
        if !g.is_empty() {
            println!("t{tick}: {}", g.join("; "));
            grew += 1;
        }
        before = now;
    }
    println!(
        "{name}: {grew} frames made something to tick {}{}",
        w.flow.tick,
        if args.warm { "" } else { " (no warm-up)" }
    );
    Ok(())
}

fn main() -> ExitCode {
    let go = |a: Args| match &a.source {
        Source::Match { name, tick, .. } if a.grow => grow_run(&a, name, *tick),
        _ if a.grow => Err("--grow needs --match".into()),
        _ => run(&a),
    };
    match parse(std::env::args().skip(1)).and_then(go) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("render-frame: {e}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}
