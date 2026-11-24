//! Draws the pre-match level's four 2v2 banners alone (no level, previews or screen) from the face-off camera and
//! writes the post chain's picture: the check of the banner port against the oracle's banner frames.
//!
//! banner-frame [--store <dir>] [--size 1920x1080] [--slugs <s1>,<s2>,<s3>,<s4>] [--cursor <x>,<y>] [--ticks <n>]
//!   --out <dir>
//!
//! Slot k (1..4) is BP_Banner_PM_Player<k> at BannerMover<k> (Seq_AnimInRed keeps the movers at their FaceOff
//! anchors), from the store's PreMatch entry: its BP_Banner_PM pieces (BannerView, started and landed) and its
//! element at the Base Element Root, shown as the pre-match's showPieces has them (the filter hidden, the background
//! under an element, the trim when the element hides it; the nameplate with its default icon). `-` for a slug leaves
//! the default banner. `--cursor` is MousePosNormalized (0..1), `--ticks` the effects' ticks before the frame (60 a
//! second).

use std::path::PathBuf;
use std::process::ExitCode;

use mvs_d3d::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
use mvs_render::assets::Assets;
use mvs_render::gpu::{Gpu, TexDesc, TexKind, TexViews};
use mvs_render::image::write_png;
use mvs_render::menuscene::banner::{
    BannerElement, BannerInit, BannerParts, BannerView, PIECE_BACKGROUND, PIECE_FILTER, PIECE_TRIM, entry,
};
use mvs_render::menuscene::format::{LevelName, SceneData, load_level};
use mvs_render::renderer::{Passes, Renderer, StaticMesh};
use mvs_view::{Camera, FrameInput, FrameTime};

/// The face-off camera's actor; MainMenu's CineCamera lens: FieldOfView 26.731283 (horizontal), 16:9.
const CAMERA: &str = "CameraTarget_Faceoff";
const CAMERA_FOV: f64 = 26.731283;
/// GNearClippingPlane.
const NEAR: f64 = 10.0;

struct Args {
    store: PathBuf,
    size: [u32; 2],
    slugs: Vec<Option<String>>,
    cursor: [f64; 2],
    ticks: u32,
    out: PathBuf,
}

fn pair(v: &str) -> Result<[f64; 2], String> {
    let p: Vec<f64> =
        v.split(',').map(|x| x.trim().parse::<f64>().map_err(|e| format!("{v}: {e}"))).collect::<Result<_, _>>()?;
    match p[..] {
        [x, y] => Ok([x, y]),
        _ => Err(format!("{v}: two numbers")),
    }
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args {
        store: mvs_store::dev_store_dir(),
        size: [1920, 1080],
        slugs: ["banner_default", "banner_bravos", "banner_arkham_asylum", "banner_encore_encore"]
            .map(|s| Some(s.to_string()))
            .to_vec(),
        cursor: [0.5, 0.5],
        ticks: 60,
        out: PathBuf::new(),
    };
    while let Some(k) = it.next() {
        let mut v = || it.next().ok_or(format!("{k} needs a value"));
        match k.as_str() {
            "--store" => a.store = v()?.into(),
            "--size" => {
                let s = v()?;
                let (w, h) = s.split_once('x').ok_or(format!("--size {s}"))?;
                a.size = [w.parse().map_err(|_| format!("--size {s}"))?, h.parse().map_err(|_| format!("--size {s}"))?];
            }
            "--slugs" => {
                a.slugs = v()?.split(',').map(|s| (s != "-").then(|| s.to_string())).collect();
                if a.slugs.len() != 4 {
                    return Err("--slugs: four".into());
                }
            }
            "--cursor" => a.cursor = pair(&v()?)?,
            "--ticks" => a.ticks = v()?.parse().map_err(|e| format!("--ticks: {e}"))?,
            "--out" => a.out = v()?.into(),
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    if a.out.as_os_str().is_empty() {
        return Err("--out <dir>".into());
    }
    Ok(a)
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scaled(v: [f64; 3], s: f64) -> [f64; 3] {
    v.map(|x| x * s)
}

/// A camera at `loc` looking down -X (yaw -180, the actor's), UE's view axes and reversed-Z projection.
fn camera(loc: [f64; 3], size: [u32; 2]) -> Camera {
    let mut v = [[0.0; 4]; 4];
    // view X (right) = world -Y, view Y (up) = world Z, view Z (forward) = world -X
    v[1][0] = -1.0;
    v[2][1] = 1.0;
    v[0][2] = -1.0;
    v[3][3] = 1.0;
    let x = 1.0 / (CAMERA_FOV.to_radians() / 2.0).tan();
    let y = x * f64::from(size[0]) / f64::from(size[1]);
    let mut p = [[0.0; 4]; 4];
    p[0][0] = x;
    p[1][1] = y;
    p[2][3] = 1.0;
    p[3][2] = NEAR;
    Camera { origin: loc, translated_world_to_view: v, view_to_clip: p }
}

fn run(a: &Args) -> Result<(), String> {
    let e = |x: &dyn std::fmt::Display| x.to_string();
    let gpu = Gpu::new(None, false).map_err(|x| e(&x))?;
    let assets = Assets::open(mvs_store::Store::open(&a.store).map_err(|x| e(&x))?, &gpu).map_err(|x| e(&x))?;
    let mut r = Renderer::new(gpu, assets, a.size).map_err(|x| e(&x))?;
    r.passes = Passes { captures: false, decals: false, lighting: false, fog: false, ..Passes::ALL };
    let SceneData::PreMatch(scene) = load_level(r.assets().store(), LevelName::PreMatch).map_err(|x| e(&x))?.data
    else {
        return Err("the PreMatch entry is not a pre-match scene".into());
    };
    let point = |n: &str| scene.anchors.get(n).map(|p| p.loc).ok_or(format!("no anchor {n}"));
    let cam = camera(point(CAMERA)?, a.size);
    let mut vfx = mvs_vfx::Player::new(r.assets().store(), 7).map_err(|x| e(&x))?;
    vfx.set_view(cam.origin, [0, 1, 2].map(|i| cam.translated_world_to_view[i][2]));
    let index = entry::load_index(r.assets().store()).map_err(|x| e(&x))?;
    let mut views: Vec<BannerView> = Vec::new();
    let mut elements: Vec<(BannerElement, [[f64; 3]; 3])> = Vec::new();
    for (k, slug) in a.slugs.iter().enumerate() {
        let name = format!("BP_Banner_PM_Player{}", k + 1);
        let b = scene.banners.iter().find(|b| b.name == name).ok_or(format!("no banner {name}"))?;
        let mover = scene.movers.get(&b.mover).ok_or(format!("no mover {}", b.mover))?;
        let root = add(point(&mover.parent)?, mover.loc);
        let pc = |n: &str| b.pieces.get(n).ok_or(format!("{name}: no {n}"));
        let placement = pc("Banner Placement")?;
        let s = placement.scale[0];
        let mask = add(add(root, placement.loc), scaled(pc("Banner Mask")?.loc, s));
        let parts = BannerParts::of_prematch(&scene, b, r.assets()).map_err(|x| e(&x))?;
        let slant = parts.slant_angle;
        let color = pc("Banner Trim Mask")?.set.as_ref().and_then(|m| m.get("ColorType")).copied().unwrap_or(1.0);
        let color = color.trunc() as u8;
        let mut view = BannerView::new(parts, mask, s, b.index, color);
        view.start();
        view.tick(1.0);
        view.shown[PIECE_FILTER] = false;
        let element = match slug {
            Some(slug) => entry::load_banner(r.assets().store(), &index, slug).map_err(|x| e(&x))?.map(|d| (slug, d)),
            None => None,
        };
        if let Some((slug, data)) = element {
            let base = pc("Base Element Root")?;
            let el_root = [add(add(root, placement.loc), scaled(base.loc, s)), base.rot, scaled(base.scale, s)];
            let init = BannerInit { banner_index: b.index, color_type: color, slant_angle: slant };
            let assets = r.assets();
            let mut rng = mvs_vfx::RandomStream::new(11);
            let mut el = BannerElement::of_entry(
                data,
                init,
                el_root,
                cam.origin,
                Some(&mut vfx),
                &mut |m| assets.mesh(m).is_ok(),
                &mut rng,
            );
            el.show(true);
            el.parallax(a.cursor, cam.origin);
            view.shown[PIECE_BACKGROUND] = false;
            view.shown[PIECE_TRIM] = !el.hide_trim;
            eprintln!("banner-frame: slot {}: {slug}", k + 1);
            elements.push((el, el_root));
        }
        views.push(view);
    }
    // the frame's place (the parallax applied), then the effects ticked there
    let mut meshes = Vec::new();
    for v in &views {
        v.meshes(&mut meshes);
        v.custom_depth(&mut r.custom_depth_only);
    }
    for (el, root) in &mut elements {
        el.place_at(*root, &mut meshes);
        el.place_systems(&mut vfx);
    }
    for _ in 0..a.ticks {
        vfx.tick(1.0 / 60.0);
    }
    r.statics = meshes
        .into_iter()
        .map(|(pose, sort)| StaticMesh {
            pose,
            lightmap: None,
            channels: 1,
            instances: None,
            receives_decals: false,
            sort: sort as f32,
        })
        .collect();
    let desc = TexDesc {
        kind: TexKind::D2,
        size: [a.size[0], a.size[1], 1],
        mips: 1,
        format: DXGI_FORMAT_R8G8B8A8_UNORM,
        views: TexViews { srv: true, rtv: true, ..Default::default() },
    };
    let out = r.gpu().texture(&desc, &[]).map_err(|x| e(&x))?;
    let t = a.ticks as f32 / 60.0;
    let input = FrameInput {
        tick: a.ticks,
        alpha: 1.0,
        time: FrameTime { game: t, real: t, delta: 1.0 / 60.0 },
        camera: &cam,
        fighters: &[],
        actors: &[],
        events: &[],
    };
    r.draw(&input, Some(&mut vfx), Some(&out)).map_err(|x| e(&x))?;
    r.gpu().finish().map_err(|x| e(&x))?;
    std::fs::create_dir_all(&a.out).map_err(|x| format!("{}: {x}", a.out.display()))?;
    let px = r.read_back(&out).map_err(|x| e(&x))?;
    let path = a.out.join("final.png");
    write_png(&path, a.size[0], a.size[1], &px)?;
    eprintln!(
        "banner-frame: {} banners, {} elements, {} translucent items, {} custom depth items -> {}",
        views.len(),
        elements.len(),
        r.draw_list().translucent.len(),
        r.draw_list().custom_depth.len(),
        path.display()
    );
    Ok(())
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)).and_then(|a| run(&a)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(x) => {
            eprintln!("banner-frame: {x}");
            ExitCode::FAILURE
        }
    }
}
