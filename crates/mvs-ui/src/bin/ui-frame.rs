//! Draws one widget class of a UI bundle to a PNG through the element batches: the class instantiated, laid out
//! over the target at a DPI scale, one of its animations applied at a time, painted and drawn over a grey
//! target, read back.
//!
//! Usage: ui-frame --class <name> [--bundle hud] [--anim <name>@<seconds> ...] [--size 1920x1080]
//! [--scale <dpi>] [--time <seconds>] [--out <file.png>]

use std::path::PathBuf;

use mvs_d3d::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
use mvs_render::gpu::{Gpu, TexDesc, TexKind, TexViews};
use mvs_render::image::write_png;
use mvs_render::{Assets, Error as RenderError};
use mvs_store::{Store, dev_store_dir};
use mvs_ui::element::{ElementList, Ui};
use mvs_ui::slate::{Geometry, apply_anim, instantiate, layout, paint};
use mvs_ui::{Bundle, Fonts};

struct Args {
    class: String,
    bundle: String,
    anims: Vec<(String, f64)>,
    size: [u32; 2],
    time: f64,
    scale: f64,
    out: PathBuf,
}

fn args() -> Result<Args, String> {
    let mut a = Args {
        class: String::new(),
        bundle: "hud".into(),
        anims: Vec::new(),
        size: [1920, 1080],
        time: 0.0,
        // DefaultEngine.ini's UIScaleCurve at 1080 lines (ShortestSide).
        scale: 0.75,
        out: PathBuf::from("ui-frame.png"),
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().ok_or_else(|| format!("{k}: no value"));
        match k.as_str() {
            "--class" => a.class = v()?,
            "--bundle" => a.bundle = v()?,
            "--anim" => {
                let s = v()?;
                let (n, at) = s.split_once('@').ok_or("--anim: <name>@<seconds>")?;
                a.anims.push((n.to_string(), at.parse().map_err(|e| format!("--anim: {e}"))?));
            }
            "--scale" => a.scale = v()?.parse().map_err(|e| format!("--scale: {e}"))?,
            "--time" => a.time = v()?.parse().map_err(|e| format!("--time: {e}"))?,
            "--out" => a.out = PathBuf::from(v()?),
            "--size" => {
                let s = v()?;
                let (w, h) = s.split_once('x').ok_or("--size: WxH")?;
                a.size = [w.parse().map_err(|_| "--size: W")?, h.parse().map_err(|_| "--size: H")?];
            }
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    if a.class.is_empty() {
        return Err("usage: ui-frame --class <name> [--bundle hud] [--anim <name>@<s> ...] [--size WxH] [--scale <dpi>] [--time <s>] \
                    [--out <png>]"
            .into());
    }
    Ok(a)
}

fn run() -> Result<(), String> {
    let a = args()?;
    let e = |x: RenderError| x.to_string();
    let store = Store::open(dev_store_dir()).map_err(|x| x.to_string())?;
    let gpu = Gpu::new(None, false).map_err(e)?;
    let assets = Assets::open(Store::open(dev_store_dir()).map_err(|x| x.to_string())?, &gpu).map_err(e)?;
    let bundle = Bundle::load(&store, &a.bundle).map_err(|x| x.to_string())?;
    let mut fonts = Fonts::new(&store, &bundle).map_err(|x| x.to_string())?;
    let mut tree = instantiate(&bundle, &a.class, &a.class);
    let scale = a.scale;
    let area = Geometry { pos: [0.0, 0.0], size: [f64::from(a.size[0]) / scale, f64::from(a.size[1]) / scale] };
    layout(&mut tree, &mut fonts, area, scale);
    for (name, at) in &a.anims {
        let root = tree.root();
        apply_anim(&mut tree, root, bundle.anim(&a.class, name), *at, 0.0);
    }
    if !a.anims.is_empty() {
        layout(&mut tree, &mut fonts, area, scale);
    }
    let mut list = ElementList::default();
    tree.set_time(a.time);
    paint(&tree, &mut fonts, &mut list, 1.0);
    let [w, h] = a.size;
    let grey = [0x40u8, 0x40, 0x40, 0xff].repeat((w * h) as usize);
    let target = gpu
        .texture(
            &TexDesc {
                kind: TexKind::D2,
                size: [w, h, 1],
                mips: 1,
                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                views: TexViews { srv: true, rtv: true, ..Default::default() },
            },
            &[mvs_render::gpu::Subresource { data: &grey, row_pitch: w * 4, slice_pitch: 0 }],
        )
        .map_err(e)?;
    let mut ui = Ui::new(&gpu, &assets).map_err(|x| x.to_string())?;
    ui.time = Some(a.time);
    ui.draw(&gpu, &assets, &mut fonts, &list, &target).map_err(|x| x.to_string())?;
    let px = gpu.read_texture(&target).map_err(e)?;
    write_png(&a.out, w, h, &px)?;
    println!("{} elements, {}", list.elements.len(), a.out.display());
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("ui-frame: {e}");
        std::process::exit(1);
    }
}
