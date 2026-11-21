//! The HUD's check against a recorded HUD tape: the trace match the tape ran (its trace in the traces folder) stepped
//! through the native World, each step's events and state given to the native Hud at the tape frame's HUD time and
//! real time, each frame drawn and its painted widgets compared with the tape's frame of the same tick.
//!
//! The fighters' widget anchors are read from the tape (the root of each AttachedBottom / AttachedTop painted there):
//! the projection of the fighters is the match loop's, not the HUD's. Pop texts and input labels are compared relative
//! to their root (their place is the camera's projection). Everything else is compared as painted.
//!
//! Usage: hud-check --tape <hudtape.zst> [--traces <dir>] [--match hud-run] [--from <tick>] [--until <tick>]
//! [--show <n>] [--dump <tick> [--png <file>]]
//!
//! --png draws the --dump tick's elements through the UI renderer over a grey target of the tape's size.

use std::collections::BTreeMap;
use std::path::PathBuf;

use mvs_sim::events::{EndPhase, Event};
use mvs_store::{Store, dev_store_dir};
use mvs_trace::Trace;
use mvs_ui::element::ElementList;
use mvs_ui::hud::{Anchor, Hud, HudFrame, HudPlayer, HudSetup};
use mvs_ui::tape::{HudTape, TapeRoot, TapeWidget, root_of};
use mvs_ui::{Bundle, Fonts, GameText};
use mvs_view::Camera;

struct Args {
    tape: PathBuf,
    traces: PathBuf,
    name: String,
    from: u32,
    until: u32,
    show: usize,
    dump: Option<u32>,
    track: Option<String>,
    lead: f64,
    png: Option<PathBuf>,
}

fn args() -> Result<Args, String> {
    let mut a = Args {
        tape: PathBuf::new(),
        // (the repo's own scratch folder; a worktree passes the main tree's with --traces)
        traces: PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.scratch/traces")),
        name: "hud-run".into(),
        from: 0,
        until: u32::MAX,
        show: 3,
        dump: None,
        track: None,
        lead: 0.0,
        png: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().ok_or_else(|| format!("{k}: no value"));
        match k.as_str() {
            "--tape" => a.tape = PathBuf::from(v()?),
            "--traces" => a.traces = PathBuf::from(v()?),
            "--match" => a.name = v()?,
            "--from" => a.from = v()?.parse().map_err(|e| format!("--from: {e}"))?,
            "--until" => a.until = v()?.parse().map_err(|e| format!("--until: {e}"))?,
            "--show" => a.show = v()?.parse().map_err(|e| format!("--show: {e}"))?,
            "--dump" => a.dump = Some(v()?.parse().map_err(|e| format!("--dump: {e}"))?),
            "--track" => a.track = Some(v()?),
            "--png" => a.png = Some(PathBuf::from(v()?)),
            "--card-lead" => a.lead = v()?.parse().map_err(|e| format!("--card-lead: {e}"))?,
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    if a.tape.as_os_str().is_empty() {
        return Err(
            "usage: hud-check --tape <hudtape.zst> [--traces <dir>] [--match <name>] [--from <tick>] [--until <tick>]"
                .into(),
        );
    }
    Ok(a)
}

/// The HUD's setup as the program makes it from the trace match's World (mvs-app run.rs ui_parts), P1 local and
/// offline; the usernames the recording gave them, from the tape header's `hud`, else hud-run's
/// ("Player", "Shaggy": a tape recorded before the header had it).
fn hud_setup(w: &mvs_sim::World, header: &mvs_ui::tape::TapeHeader) -> HudSetup {
    let named = header.rest.get("hud").and_then(|h| h.get("players")).and_then(|p| p.as_array());
    let setup = &w.setup;
    let players = w
        .players
        .iter()
        .enumerate()
        .map(|(i, &f)| {
            let fi = w.fighter(f);
            let username = match named {
                Some(p) => p.get(i).and_then(|p| p.get("username")).and_then(|u| u.as_str()).unwrap_or("").to_string(),
                None => ["Player", "Shaggy"].get(i).copied().unwrap_or("").to_string(),
            };
            HudPlayer {
                team: u8::try_from(fi.team.team).unwrap_or(0),
                bot: setup.players.get(i).is_some_and(|p| p.bot),
                username,
                character: Some(w.names.str(fi.ch).to_owned()),
            }
        })
        .collect();
    HudSetup {
        // UI context mode: 1 1v1, 2 2v2, 3 free for all.
        mode: setup.team_style as u8 + 1,
        me: 0,
        players,
        #[allow(clippy::cast_possible_truncation)]
        max_score: setup.rules.map_or(-1, |r| r.knockouts_to_win as i32),
        stocks: setup.rules.is_some_and(|r| r.stocks),
        local_play: setup.local_play,
        online: false,
        arcade: None,
        ui: mvs_settings::UiSettings::default(),
        wheel: setup.wheel.as_ref().map(|w| {
            let lobby = mvs_protocol::lobby_rules::LOBBY_MUTATORS;
            w.pool.iter().filter_map(|k| lobby.iter().position(|l| l == k)).collect()
        }),
    }
}

/// The anchor of a fighter's widget from the tape's painted root: the root's canvas sits at the anchor minus half its
/// DrawSize (slate units), under the view's DPI scale.
fn anchor(roots: &[TapeRoot], owner: &str, half: [f64; 2]) -> Option<Anchor> {
    let k = roots.iter().position(|r| r.owner == owner)?;
    let w = roots[k].widgets.first()?;
    let s = w.matrix[0];
    #[allow(clippy::cast_possible_truncation)]
    let at = |e: f64, h: f64| ((e / s + h) * s).round() as i32;
    // (the recording paints the farther first: its place in the paint order stands for its distance)
    #[allow(clippy::cast_precision_loss)]
    let dist = (roots.len() - k) as f32;
    Some(Anchor { x: at(w.matrix[4], half[0]), y: at(w.matrix[5], half[1]), dist })
}

/// A camera whose every point is in front: clip = (x, y, 1, 1) of the translated world point.
fn flat_camera() -> Camera {
    let mut v = [[0.0; 4]; 4];
    v[0][0] = 1.0;
    v[1][1] = 1.0;
    v[2][2] = 1.0;
    v[3][3] = 1.0;
    let mut p = [[0.0; 4]; 4];
    p[0][0] = 1.0;
    p[1][1] = 1.0;
    p[3][2] = 1.0;
    p[3][3] = 1.0;
    Camera { origin: [0.0; 3], translated_world_to_view: v, view_to_clip: p }
}

/// The widget's fields as comparable lines: (field, value).
fn fields(w: &TapeWidget, relative: Option<[f64; 2]>) -> Vec<(&'static str, String)> {
    let n = |x: f64| format!("{:.3}", if x.abs() < 0.0005 { 0.0 } else { x });
    let v = |x: &[f64]| x.iter().map(|&x| n(x)).collect::<Vec<_>>().join(",");
    let mut m = w.matrix;
    if let Some(o) = relative {
        m[4] -= o[0];
        m[5] -= o[1];
    }
    let mut out = vec![
        ("type", w.ty.clone()),
        ("vis", w.vis.to_string()),
        ("opacity", n(w.opacity)),
        ("alpha", n(w.alpha)),
        ("matrix", v(&m)),
        ("size", v(&w.size)),
        ("color", v(&w.color)),
    ];
    if let Some(t) = &w.tex {
        // (the tape holds the bundle's name, the native widget the object path: compared by the object's name)
        let name = t.rsplit(['.', '/']).next().unwrap_or(t);
        out.push(("tex", name.to_string()));
    }
    if let Some(c) = &w.brush_color {
        out.push(("brushColor", v(c)));
    }
    if let Some(m) = &w.mid {
        let mut s: Vec<_> = m.s.iter().map(|(k, x)| format!("{k}={}", n(*x))).collect();
        s.sort();
        let mut vv: Vec<_> = m.v.iter().map(|(k, x)| format!("{k}={}", v(x))).collect();
        vv.sort();
        out.push(("mid", format!("{} | {}", s.join(" "), vv.join(" "))));
    }
    if let Some(t) = &w.text {
        out.push(("text", t.clone()));
    }
    if let Some(f) = &w.font {
        out.push((
            "font",
            format!(
                "{:?} {} ol {} {} sp {} {} sh {} {}",
                f.face,
                n(f.size),
                n(f.outline),
                v(&f.outline_color),
                n(f.spacing),
                f.justify,
                v(&f.shadow),
                v(&f.shadow_color)
            ),
        ));
    }
    if let Some(a) = w.active {
        out.push(("active", a.to_string()));
    }
    out
}

/// Two field values alike: their numbers within 0.02 (a hundredth of a pixel or of a unit is the formats' rounding,
/// f32 against f64), the rest equal.
fn same(x: &str, y: &str) -> bool {
    let split =
        |s: &str| s.split([',', ' ', '=', '|']).filter(|t| !t.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let (a, b) = (split(x), split(y));
    a.len() == b.len()
        && a.iter().zip(&b).all(|(p, q)| match (p.parse::<f64>(), q.parse::<f64>()) {
            (Ok(u), Ok(v)) => (u - v).abs() <= 0.02,
            _ => p == q,
        })
}

#[derive(Default)]
struct Diff {
    first: u32,
    last: u32,
    count: usize,
    samples: Vec<(u32, String, String)>,
}

/// The elements drawn through the UI renderer over a grey target, written to `png`.
fn shot(
    store: &Store,
    fonts: &mut Fonts,
    list: &ElementList,
    size: [u32; 2],
    real: f64,
    png: &std::path::Path,
) -> Result<(), String> {
    use mvs_render::gpu::{Gpu, Subresource, TexDesc, TexKind, TexViews};
    let e = |x: mvs_render::Error| x.to_string();
    let gpu = Gpu::new(None, false).map_err(e)?;
    let assets = mvs_render::Assets::open(Store::open(store.dir()).map_err(|x| x.to_string())?, &gpu).map_err(e)?;
    let [w, h] = size;
    let grey = [0x40u8, 0x40, 0x40, 0xff].repeat((w * h) as usize);
    let desc = TexDesc {
        kind: TexKind::D2,
        size: [w, h, 1],
        mips: 1,
        format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM,
        views: TexViews { srv: true, rtv: true, ..Default::default() },
    };
    let target = gpu.texture(&desc, &[Subresource { data: &grey, row_pitch: w * 4, slice_pitch: 0 }]).map_err(e)?;
    let mut ui = mvs_ui::element::Ui::new(&gpu, &assets).map_err(|x| x.to_string())?;
    ui.time = Some(real);
    ui.draw(&gpu, &assets, fonts, list, &target).map_err(|x| x.to_string())?;
    let px = gpu.read_texture(&target).map_err(e)?;
    mvs_render::image::write_png(png, w, h, &px)?;
    println!("{} elements, {}", list.elements.len(), png.display());
    Ok(())
}

fn run() -> Result<(), String> {
    let a = args()?;
    let tape = HudTape::read(&a.tape).map_err(|e| e.to_string())?;
    let store = Store::open(dev_store_dir()).map_err(|e| e.to_string())?;
    let trace = Trace::read(&a.traces.join(format!("{}.trace", a.name))).map_err(|e| e.to_string())?;
    let (mut w, book) = mvs_sim_trace::world_for(&trace.header, &store)?;
    let bundle = Bundle::load(&store, "hud").map_err(|e| e.to_string())?;
    if let Some(t) = &a.track {
        // --track <class>/<animation>/<widget>: the animation's tracks of that widget, as the bundle holds them
        let mut it = t.splitn(3, '/');
        let (c, n, wn) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
        let an = bundle.anim(c, n);
        println!("{c} {n}: res {} range {:?}", an.res, an.range);
        for tr in an.tracks.iter().filter(|x| x.widget == wn) {
            println!("{tr:?}");
        }
        return Ok(());
    }
    let text = GameText::load(&store).map_err(|e| e.to_string())?;
    let mut fonts = Fonts::new(&store, &bundle).map_err(|e| e.to_string())?;
    let setup = hud_setup(&w, &tape.header);
    let mut hud = Hud::new(&bundle, &text, &w, &setup).map_err(|e| e.to_string())?;
    let camera = flat_camera();
    let size = tape.header.size;
    let mut diffs: BTreeMap<(String, String, &'static str), Diff> = BTreeMap::new();
    let mut frames = 0usize;
    let mut started = false;
    let mut ended = false;
    for (k, tf) in tape.frames.iter().enumerate() {
        if tf.tick > a.until {
            break;
        }
        // (a frame after a step: the tick before it is the trace's frame of that step)
        if k > 0 {
            let i = tf.tick as usize - 1;
            let frame = trace.frame(i).map_err(|e| e.to_string())?;
            let inputs: Vec<_> = frame.inputs.iter().map(|r| mvs_sim_trace::input_of(r, &w, &book)).collect();
            w.step(&inputs);
            let events = std::mem::take(&mut w.events);
            // (the recording's match loop draws no frame after the travel: the tape's frame there is empty)
            ended |= events.iter().any(|e| matches!(e, Event::MatchEnd { phase: EndPhase::Travel, .. }));
            hud.observe(&w, &events, tf.now, tf.real);
        }
        if !started {
            hud.start_card(tf.real - a.lead);
            started = true;
        }
        let bottom: Vec<_> =
            (0..2).map(|i| anchor(&tf.roots, &format!("AttachedBottom[{i}]"), [200.0, 55.0])).collect();
        let top: Vec<_> = (0..2).map(|i| anchor(&tf.roots, &format!("AttachedTop[{i}]"), [125.0, 80.0])).collect();
        let f = HudFrame {
            size,
            hud_time: tf.now,
            real: tf.real,
            bottom: &bottom,
            top: &top,
            camera: &camera,
            items: &[],
            skillshots: &[],
            round_time_remaining: f64::from(w.round_time_remaining()),
            hold: false,
        };
        let mut out = ElementList::default();
        if !ended {
            hud.draw(&f, &mut fonts, &mut out);
        }
        if tf.tick < a.from {
            continue;
        }
        frames += 1;
        let ours: Vec<TapeRoot> =
            if ended { Vec::new() } else { hud.painted() }.into_iter().map(|(o, t)| root_of(&o, t)).collect();
        if a.dump == Some(tf.tick) {
            if let Some(p) = &a.png {
                shot(&store, &mut fonts, &out, size, tf.real, p)?;
            }
            for (side, roots) in [("tape", &tf.roots), ("ours", &ours)] {
                for r in roots {
                    println!("{side} {} {}", r.owner, r.widgets.len());
                }
            }
        }
        // roots by owner and its occurrence
        let key = |roots: &[TapeRoot]| {
            let mut seen: BTreeMap<String, usize> = BTreeMap::new();
            roots
                .iter()
                .map(|r| {
                    let n = seen.entry(r.owner.clone()).or_default();
                    *n += 1;
                    (format!("{}#{}", r.owner, *n), r.clone())
                })
                .collect::<BTreeMap<_, _>>()
        };
        let theirs = key(&tf.roots);
        let mine = key(&ours);
        let mut note = |owner: &str, path: &str, field: &'static str, t: String, o: String| {
            let d = diffs
                .entry((owner.to_string(), path.to_string(), field))
                .or_insert_with(|| Diff { first: tf.tick, ..Diff::default() });
            d.count += 1;
            d.last = tf.tick;
            if d.samples.len() < a.show {
                d.samples.push((tf.tick, t, o));
            }
        };
        let order = |roots: &[TapeRoot]| roots.iter().map(|r| r.owner.clone()).collect::<Vec<_>>().join(" ");
        if order(&tf.roots) != order(&ours) {
            note("*", "*", "roots", order(&tf.roots), order(&ours));
        }
        for (owner, t) in &theirs {
            let Some(o) = mine.get(owner) else { continue };
            let rel = owner.starts_with("PopText") || owner.starts_with("PopInputLabel");
            let origin = |r: &TapeRoot| r.widgets.first().map(|w| [w.matrix[4], w.matrix[5]]);
            let (tr, or) = if rel { (origin(t), origin(o)) } else { (None, None) };
            for tw in &t.widgets {
                let Some(ow) = o.widget(&tw.path) else {
                    note(owner, &tw.path, "missing", String::new(), String::new());
                    continue;
                };
                let (a1, b1) = (fields(tw, tr), fields(ow, or));
                for ((f, x), (_, y)) in a1.iter().zip(&b1) {
                    // (a user widget is "User" on the tape, its class natively)
                    let user = *f == "type" && (x == "User" || (x == "Quad" && y == "WBP_Quadrilateral"));
                    if !same(x, y) && !user {
                        note(owner, &tw.path, f, x.clone(), y.clone());
                    }
                }
                for (f, x) in a1.iter().skip(b1.len()) {
                    note(owner, &tw.path, f, x.clone(), String::new());
                }
                for (f, y) in b1.iter().skip(a1.len()) {
                    note(owner, &tw.path, f, String::new(), y.clone());
                }
            }
            for ow in &o.widgets {
                if t.widget(&ow.path).is_none() {
                    note(owner, &ow.path, "extra", String::new(), String::new());
                }
            }
        }
    }
    println!("{frames} frames compared, {} differing (owner, widget, field)", diffs.len());
    for ((owner, path, field), d) in &diffs {
        println!("{owner} {path} {field}: {} frames, ticks {}..{}", d.count, d.first, d.last);
        for (tick, t, o) in &d.samples {
            println!("    @{tick} tape {t}\n          ours {o}");
        }
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("hud-check: {e}");
        std::process::exit(1);
    }
}
