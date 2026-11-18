//! The menu tape: per frame of a scripted run, every widget the menus painted, root by root in paint order, with its
//! painted geometry, opacity, visibility, text, colour and brush. The oracle's menu run writes the same format (an
//! input script played on a stepped clock). The
//! native run writes the same lines (`mvs --input-script <jsonl> --menu-tape <out>`), and `compare` reads the two.
//!
//! The file is JSON lines, zstd-compressed when its name ends in `.zst`: a header line, then one line per frame.
//! The widgets are the HUD tape's (`crate::tape`): path, type, visibility, opacity, matrix, size, colour, brush,
//! text. Tapes hold the build's text and names: they stay in `.scratch/`, never committed.
//!
//! The frame's roots are the view's world widgets (`world[i]:<name>`) and layers (`<i>:<name>`) in paint order;
//! the offscreen targets (the spare trees painted into a canvas) are not on the tape. `fader` is the manager's
//! Fader opacity, `mouse` the view's pointer in device pixels (null before it moves).

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::slate::{Tree, WidgetId};
use crate::tape::{TapeRoot, TapeWidget, walk};
use crate::{Error, Result};

/// The tape's first line.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MenuHeader {
    /// "menu".
    pub tape: String,
    pub version: u32,
    /// The input script's name (its file stem).
    #[serde(default)]
    pub script: String,
    /// Where the script starts: "title" or "menu" (the main menu, settled).
    #[serde(default)]
    pub start: String,
    /// The drawing buffer in device pixels.
    #[serde(default)]
    pub size: [u32; 2],
}

/// One frame of the run: `frame` counts from 0 at the script's first frame.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MenuFrame {
    pub frame: u32,
    /// The view's clock, s (frame / 60 on both sides: a tape whose clocks differ was not played at 60 Hz).
    pub now: f64,
    pub mouse: Option<[f64; 2]>,
    pub fader: f64,
    pub roots: Vec<TapeRoot>,
}

/// A whole tape.
#[derive(Clone, Debug)]
pub struct MenuTape {
    pub header: MenuHeader,
    pub frames: Vec<MenuFrame>,
}

fn bad(what: impl Into<String>, why: impl ToString) -> Error {
    Error::Format { what: what.into(), why: why.to_string() }
}

impl MenuTape {
    /// The tape at `path` (`.zst`: zstd over the lines).
    pub fn read(path: &Path) -> Result<MenuTape> {
        let what = path.display().to_string();
        let raw = std::fs::read(path).map_err(|e| bad(&what, e))?;
        let bytes = if path.extension().is_some_and(|e| e == "zst") {
            zstd::decode_all(raw.as_slice()).map_err(|e| bad(&what, e))?
        } else {
            raw
        };
        let text = std::str::from_utf8(&bytes).map_err(|e| bad(&what, e))?;
        MenuTape::parse(text).map_err(|e| match e {
            Error::Format { what: w, why } => bad(format!("{what} {w}"), why),
            e => e,
        })
    }

    /// A tape from its lines.
    pub fn parse(text: &str) -> Result<MenuTape> {
        let mut lines = text.lines().enumerate().filter(|(_, l)| !l.is_empty());
        let (_, first) = lines.next().ok_or_else(|| bad("line 1", "no header"))?;
        let header: MenuHeader = serde_json::from_str(first).map_err(|e| bad("line 1", e))?;
        if header.tape != "menu" || header.version != 1 {
            return Err(bad(
                "line 1",
                format!("a {} tape of version {}, not a menu tape of version 1", header.tape, header.version),
            ));
        }
        let mut frames = Vec::new();
        for (i, l) in lines {
            frames.push(serde_json::from_str::<MenuFrame>(l).map_err(|e| bad(format!("line {}", i + 1), e))?);
        }
        Ok(MenuTape { header, frames })
    }

    /// The tape's lines (the header, then a line per frame).
    pub fn to_text(&self) -> String {
        let mut out = serde_json::to_string(&self.header).expect("a header serializes");
        out.push('\n');
        for f in &self.frames {
            out.push_str(&serde_json::to_string(f).expect("a frame serializes"));
            out.push('\n');
        }
        out
    }

    /// Writes the tape to `path` (`.zst`: zstd over the lines).
    pub fn write(&self, path: &Path) -> Result<()> {
        let what = path.display().to_string();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| bad(&what, e))?;
        }
        let text = self.to_text();
        let bytes = if path.extension().is_some_and(|e| e == "zst") {
            zstd::encode_all(text.as_bytes(), 3).map_err(|e| bad(&what, e))?
        } else {
            text.into_bytes()
        };
        let mut f = std::fs::File::create(path).map_err(|e| bad(&what, e))?;
        f.write_all(&bytes).map_err(|e| bad(&what, e))
    }

    /// The frame numbered `n`.
    pub fn frame(&self, n: u32) -> Option<&MenuFrame> {
        self.frames.iter().find(|f| f.frame == n)
    }
}

/// The widgets one root of a view's tree painted (the HUD tape's walk from `root`).
pub fn root_in(owner: &str, tree: &Tree, root: WidgetId) -> TapeRoot {
    let mut widgets = Vec::new();
    walk(tree, root, tree.widget(root).name.clone(), 1.0, &mut widgets);
    TapeRoot { owner: owner.to_string(), widgets }
}

/// A frame of the native menus as the tape writes it. `roots`: the view's world widgets, then its layers, in paint
/// order, as (name of the root widget, id); the owner of the i-th layer is `<i>:<name>`, of the i-th world widget
/// `world[i]:<name>`. Called by the view after its paint (`MenuView::tape_frame`).
pub fn frame_of(
    frame: u32,
    now: f64,
    mouse: Option<[f64; 2]>,
    fader: f64,
    tree: &Tree,
    world: &[WidgetId],
    layers: &[WidgetId],
) -> MenuFrame {
    let mut roots = Vec::new();
    for (i, &w) in world.iter().enumerate() {
        roots.push(root_in(&format!("world[{i}]:{}", tree.widget(w).name), tree, w));
    }
    for (i, &w) in layers.iter().enumerate() {
        roots.push(root_in(&format!("{i}:{}", tree.widget(w).name), tree, w));
    }
    MenuFrame { frame, now, mouse, fader, roots }
}

// ---- the comparison -------------------------------------------------------------------------------------------

/// The geometry tolerance, device pixels (a widget's corners).
pub const GEOMETRY_TOLERANCE: f64 = 0.5;
/// The tolerance of opacities, colours and the fader (animated values the two sides evaluate in different float
/// widths).
pub const VALUE_TOLERANCE: f64 = 1e-3;

/// One difference of a frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Diff {
    pub frame: u32,
    /// The root's owner ("" for a frame field).
    pub root: String,
    /// The widget's path ("" for a root or frame field).
    pub path: String,
    pub field: &'static str,
    pub oracle: String,
    pub native: String,
}

impl std::fmt::Display for Diff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "frame {} {} {} {}: oracle {} native {}",
            self.frame, self.root, self.path, self.field, self.oracle, self.native
        )
    }
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn near_all(a: &[f64], b: &[f64], tol: f64) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| near(*x, *y, tol))
}

/// The widget's rect corners in device pixels: the matrix applied to (0, 0), (w, 0), (0, h), (w, h).
fn corners(w: &TapeWidget) -> [f64; 8] {
    let [a, b, c, d, e, f] = w.matrix;
    let [sw, sh] = w.size;
    let at = |x: f64, y: f64| [a * x + c * y + e, b * x + d * y + f];
    let p = [at(0.0, 0.0), at(sw, 0.0), at(0.0, sh), at(sw, sh)];
    [p[0][0], p[0][1], p[1][0], p[1][1], p[2][0], p[2][1], p[3][0], p[3][1]]
}

fn json<T: Serialize + ?Sized>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

/// The differences of widget `o` (oracle) and `n` (native) at one path.
fn widget_diffs(frame: u32, root: &str, o: &TapeWidget, n: &TapeWidget, out: &mut Vec<Diff>) {
    let mut put = |field: &'static str, oracle: String, native: String| {
        out.push(Diff { frame, root: root.to_string(), path: o.path.clone(), field, oracle, native });
    };
    if o.ty != n.ty {
        put("type", o.ty.clone(), n.ty.clone());
    }
    if o.vis != n.vis {
        put("visibility", o.vis.to_string(), n.vis.to_string());
    }
    if !near(o.opacity, n.opacity, VALUE_TOLERANCE) {
        put("opacity", o.opacity.to_string(), n.opacity.to_string());
    }
    if !near(o.alpha, n.alpha, VALUE_TOLERANCE) {
        put("painted opacity", o.alpha.to_string(), n.alpha.to_string());
    }
    let (co, cn) = (corners(o), corners(n));
    if !near_all(&co, &cn, GEOMETRY_TOLERANCE) {
        put("geometry", json(&co), json(&cn));
    }
    if !near_all(&o.color, &n.color, VALUE_TOLERANCE) {
        put("color", json(&o.color), json(&n.color));
    }
    if o.tex != n.tex {
        put("brush", json(&o.tex), json(&n.tex));
    }
    let same_bc = match (&o.brush_color, &n.brush_color) {
        (Some(a), Some(b)) => near_all(a, b, VALUE_TOLERANCE),
        (None, None) => true,
        _ => false,
    };
    if !same_bc {
        put("brush color", json(&o.brush_color), json(&n.brush_color));
    }
    if o.text != n.text {
        put("text", json(&o.text), json(&n.text));
    }
    if o.active != n.active {
        put("active", json(&o.active), json(&n.active));
    }
    if let (Some(a), Some(b)) = (&o.font, &n.font) {
        let same = a.face == b.face
            && near(a.size, b.size, VALUE_TOLERANCE)
            && near(a.outline, b.outline, VALUE_TOLERANCE)
            && near(a.spacing, b.spacing, VALUE_TOLERANCE)
            && a.justify == b.justify
            && near_all(&a.shadow, &b.shadow, VALUE_TOLERANCE);
        if !same {
            put("font", json(a), json(b));
        }
    } else if o.font.is_some() != n.font.is_some() {
        put("font", json(&o.font), json(&n.font));
    }
}

/// The differences of two frames of the same script frame: the roots by position (the owner must agree), the
/// widgets of a root by path (one only in either is a difference).
pub fn compare_frame(o: &MenuFrame, n: &MenuFrame) -> Vec<Diff> {
    let mut out = Vec::new();
    let frame = o.frame;
    let mut put = |field: &'static str, oracle: String, native: String| {
        out.push(Diff { frame, root: String::new(), path: String::new(), field, oracle, native });
    };
    if !near(o.fader, n.fader, VALUE_TOLERANCE) {
        put("fader", o.fader.to_string(), n.fader.to_string());
    }
    if o.mouse != n.mouse {
        put("mouse", json(&o.mouse), json(&n.mouse));
    }
    let owners = |f: &MenuFrame| f.roots.iter().map(|r| r.owner.clone()).collect::<Vec<_>>();
    if owners(o) != owners(n) {
        put("roots", json(&owners(o)), json(&owners(n)));
        return out;
    }
    for (ro, rn) in o.roots.iter().zip(&n.roots) {
        for wo in &ro.widgets {
            match rn.widget(&wo.path) {
                Some(wn) => widget_diffs(frame, &ro.owner, wo, wn, &mut out),
                None => out.push(Diff {
                    frame,
                    root: ro.owner.clone(),
                    path: wo.path.clone(),
                    field: "painted",
                    oracle: "yes".into(),
                    native: "no".into(),
                }),
            }
        }
        for wn in &rn.widgets {
            if ro.widget(&wn.path).is_none() {
                out.push(Diff {
                    frame,
                    root: ro.owner.clone(),
                    path: wn.path.clone(),
                    field: "painted",
                    oracle: "no".into(),
                    native: "yes".into(),
                });
            }
        }
    }
    out
}

/// The result of comparing two tapes of one script.
#[derive(Debug, Default)]
pub struct Report {
    /// Frames compared (the frames both tapes have).
    pub compared: usize,
    /// Frames with at least one difference.
    pub differing: usize,
    /// The first differing frame's differences.
    pub first: Vec<Diff>,
    /// Frames only one tape has: (oracle count, native count).
    pub lengths: (usize, usize),
}

impl Report {
    pub fn same(&self) -> bool {
        self.differing == 0 && self.lengths.0 == self.lengths.1
    }
}

/// Compares the tapes frame by frame (by frame number).
pub fn compare(oracle: &MenuTape, native: &MenuTape) -> Report {
    let mut r = Report { lengths: (oracle.frames.len(), native.frames.len()), ..Report::default() };
    for o in &oracle.frames {
        let Some(n) = native.frame(o.frame) else { continue };
        r.compared += 1;
        let d = compare_frame(o, n);
        if !d.is_empty() {
            r.differing += 1;
            if r.first.is_empty() {
                r.first = d;
            }
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"tape":"menu","version":1,"script":"nav","start":"menu","size":[1920,1080]}
{"frame":0,"now":0,"mouse":null,"fader":0,"roots":[{"owner":"0:UI_Home","widgets":[{"p":"UI_Home","t":"User","v":3,"o":1,"a":1,"m":[0.75,0,0,0.75,0,0],"s":[2560,1440],"c":[1,1,1,1]},{"p":"UI_Home/Title","t":"TextBlock","v":3,"o":1,"a":1,"m":[0.75,0,0,0.75,10,20],"s":[300,80],"c":[1,1,1,1],"text":"PLAY","font":{"face":null,"size":32,"outline":0,"outlineColor":[0,0,0,1],"spacing":0,"justify":"Left","shadow":[1,1],"shadowColor":[0,0,0,0]}}]}]}
{"frame":1,"now":0.016666666666666666,"mouse":[100,200],"fader":0,"roots":[]}
"#;

    #[test]
    fn a_tape_reads_back_and_writes_the_same() {
        let t = MenuTape::parse(SAMPLE).unwrap();
        assert_eq!(t.header.script, "nav");
        assert_eq!(t.frames.len(), 2);
        let w = t.frame(0).unwrap().roots[0].widget("UI_Home/Title").unwrap();
        assert_eq!(w.text.as_deref(), Some("PLAY"));
        let again = MenuTape::parse(&t.to_text()).unwrap();
        assert_eq!(again.to_text(), t.to_text());
    }

    #[test]
    fn a_hud_tape_is_refused() {
        assert!(MenuTape::parse(&SAMPLE.replace("\"menu\"", "\"hud\"")).is_err());
    }

    #[test]
    fn equal_tapes_compare_equal() {
        let t = MenuTape::parse(SAMPLE).unwrap();
        let r = compare(&t, &t);
        assert!(r.same(), "{r:?}");
        assert_eq!(r.compared, 2);
    }

    #[test]
    fn geometry_within_half_a_pixel_passes_and_a_move_does_not() {
        let o = MenuTape::parse(SAMPLE).unwrap();
        let mut n = o.clone();
        n.frames[0].roots[0].widgets[1].matrix[4] += 0.4;
        assert!(compare(&o, &n).same());
        n.frames[0].roots[0].widgets[1].matrix[4] += 0.4;
        let r = compare(&o, &n);
        assert_eq!(r.differing, 1);
        assert_eq!(r.first[0].field, "geometry");
        assert_eq!(r.first[0].path, "UI_Home/Title");
    }

    #[test]
    fn a_text_a_missing_widget_and_the_fader_are_named() {
        let o = MenuTape::parse(SAMPLE).unwrap();
        let mut n = o.clone();
        n.frames[0].roots[0].widgets[1].text = Some("LAB".into());
        n.frames[0].fader = 0.5;
        let fields: Vec<_> = compare(&o, &n).first.iter().map(|d| d.field).collect();
        assert_eq!(fields, ["fader", "text"]);
        n.frames[0].roots[0].widgets.pop();
        let d = compare(&o, &n).first;
        assert!(d.iter().any(|d| d.field == "painted" && d.native == "no"));
    }

    #[test]
    fn a_different_set_of_roots_stops_the_frame() {
        let o = MenuTape::parse(SAMPLE).unwrap();
        let mut n = o.clone();
        n.frames[0].roots[0].owner = "1:UI_Home".into();
        let d = compare_frame(&o.frames[0], &n.frames[0]);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].field, "roots");
    }

    #[test]
    fn the_runs_tape_reads() {
        let Some(p) = std::env::var_os("MENU_TAPE") else {
            eprintln!("note: MENU_TAPE names no tape: not checked");
            return;
        };
        let t = MenuTape::read(Path::new(&p)).unwrap();
        assert!(!t.frames.is_empty());
        for (i, f) in t.frames.iter().enumerate() {
            assert_eq!(f.frame as usize, i, "frames count from 0");
        }
        eprintln!("{}: {} frames", Path::new(&p).display(), t.frames.len());
    }
}
