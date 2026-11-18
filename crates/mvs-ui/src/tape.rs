//! The HUD tape: per drawn frame of an oracle run, every widget the oracle's HUD painted, root by root in paint
//! order, with its painted geometry, opacity, visibility, text, colour and brush (the oracle's recorder writes it for
//! the trace match `hud-run`, or for `HUD_RUN=<match>`, another trace match). The HUD's check reads it: a frame of
//! the native HUD against the oracle's of the same tick. The characters' own HUD
//! parts are on it (`FinnShopWidgets`, `TomJerryHud`, `CorkCount`, `Reticle`), with the dash and skill shot targets
//! and the item indicators; the header's `hud` is the HUD's setup.
//!
//! The file is JSON lines, zstd-compressed when its name ends in `.zst`: a header line, then one line per frame.
//! Tapes hold the build's text and names: they stay in `.scratch/`, never committed.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::slate::{Tree, Vis, WidgetId};
use crate::{Error, Result};

/// The tape's first line: `tape` "hud", its `version`, and what the run says of itself (`run`, `size`,
/// `worldSeed`, `map`).
#[derive(Clone, Debug, Deserialize)]
pub struct TapeHeader {
    pub tape: String,
    pub version: u32,
    #[serde(default)]
    pub run: String,
    /// The oracle's drawing buffer in device pixels.
    #[serde(default)]
    pub size: [u32; 2],
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

/// One drawn frame: the World's tick it shows, FighterHud.draw's HUD game time and real time (s), and the roots
/// painted, in paint order.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TapeFrame {
    pub tick: u32,
    pub now: f64,
    pub real: f64,
    pub roots: Vec<TapeRoot>,
}

/// A HUD part's widget tree as painted: `owner` its class (`AttachedBottom[i]` / `AttachedTop[i]` by fighter, else
/// the class: `Scoreboard`, `Countdown`, `VictoryText`, `PopText`, `ConnectionState`, ...), its painted widgets in
/// paint order (a parent before its children). A widget not painted (Collapsed or Hidden, a switcher's other
/// children, what is under them) is not on the tape.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TapeRoot {
    pub owner: String,
    pub widgets: Vec<TapeWidget>,
}

/// A painted widget.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TapeWidget {
    /// The path from the root: names joined by '/', a name met again among siblings with `#<n>` (n from 1).
    #[serde(rename = "p")]
    pub path: String,
    /// The UMG class (Image, TextBlock, Border, CanvasPanel, User for a user widget's root, ...).
    #[serde(rename = "t")]
    pub ty: String,
    /// ESlateVisibility as the tape numbers it: 0 Visible, 3 HitTestInvisible, 4 SelfHitTestInvisible (1 Collapsed
    /// and 2 Hidden are not painted).
    #[serde(rename = "v")]
    pub vis: u8,
    /// RenderOpacity.
    #[serde(rename = "o")]
    pub opacity: f64,
    /// The opacity it is painted at: its parents' times its own.
    #[serde(rename = "a")]
    pub alpha: f64,
    /// The paint matrix [a, b, c, d, e, f] in device pixels: x' = a x + c y + e, y' = b x + d y + f.
    #[serde(rename = "m")]
    pub matrix: [f64; 6],
    /// The arranged size (slate units, before the matrix).
    #[serde(rename = "s")]
    pub size: [f64; 2],
    /// ColorAndOpacity, linear RGBA.
    #[serde(rename = "c")]
    pub color: [f64; 4],
    /// The brush's texture or material name.
    #[serde(default)]
    pub tex: Option<String>,
    /// A Border's BrushColor.
    #[serde(rename = "bc", default)]
    pub brush_color: Option<[f64; 4]>,
    /// The dynamic material's parameters as set.
    #[serde(default)]
    pub mid: Option<TapeMid>,
    /// A TextBlock's text as shown (after ToUpper).
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub font: Option<TapeFont>,
    /// A WidgetSwitcher's active index.
    #[serde(default)]
    pub active: Option<u32>,
}

/// A UMaterialInstanceDynamic's scalar and vector parameters by name.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TapeMid {
    pub s: indexmap::IndexMap<String, f64>,
    pub v: indexmap::IndexMap<String, [f64; 4]>,
}

/// A TextBlock's font as the tape holds it.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TapeFont {
    /// The typeface (the bundle's name), None for the default.
    pub face: Option<String>,
    pub size: f64,
    pub outline: f64,
    pub outline_color: [f64; 4],
    /// LetterSpacing, 1/1000 em.
    pub spacing: f64,
    /// ETextJustify's tail.
    pub justify: String,
    pub shadow: [f64; 2],
    pub shadow_color: [f64; 4],
}

/// A whole tape.
#[derive(Clone, Debug)]
pub struct HudTape {
    pub header: TapeHeader,
    pub frames: Vec<TapeFrame>,
}

fn bad(what: impl Into<String>, why: impl ToString) -> Error {
    Error::Format { what: what.into(), why: why.to_string() }
}

impl HudTape {
    /// The tape at `path` (`.zst`: zstd over the lines).
    pub fn read(path: &Path) -> Result<HudTape> {
        let what = path.display().to_string();
        let raw = std::fs::read(path).map_err(|e| bad(&what, e))?;
        let bytes = if path.extension().is_some_and(|e| e == "zst") {
            zstd::decode_all(raw.as_slice()).map_err(|e| bad(&what, e))?
        } else {
            raw
        };
        let text = std::str::from_utf8(&bytes).map_err(|e| bad(&what, e))?;
        HudTape::parse(text).map_err(|e| match e {
            Error::Format { what: w, why } => bad(format!("{what} {w}"), why),
            e => e,
        })
    }

    /// A tape from its lines.
    pub fn parse(text: &str) -> Result<HudTape> {
        let mut lines = text.lines().enumerate().filter(|(_, l)| !l.is_empty());
        let (_, first) = lines.next().ok_or_else(|| bad("line 1", "no header"))?;
        let header: TapeHeader = serde_json::from_str(first).map_err(|e| bad("line 1", e))?;
        if header.tape != "hud" || header.version != 1 {
            return Err(bad(
                "line 1",
                format!("a {} tape of version {}, not a hud tape of version 1", header.tape, header.version),
            ));
        }
        let mut frames = Vec::new();
        for (i, l) in lines {
            frames.push(serde_json::from_str::<TapeFrame>(l).map_err(|e| bad(format!("line {}", i + 1), e))?);
        }
        Ok(HudTape { header, frames })
    }

    /// The last frame drawn at `tick` (a tick drawn again, `mvsTest.step(0)`, has more than one).
    pub fn frame(&self, tick: u32) -> Option<&TapeFrame> {
        self.frames.iter().rev().find(|f| f.tick == tick)
    }
}

impl TapeFrame {
    /// The root painted by `owner`, the first if it painted more than one (`PopText`).
    pub fn root(&self, owner: &str) -> Option<&TapeRoot> {
        self.roots.iter().find(|r| r.owner == owner)
    }
}

impl TapeRoot {
    /// The widget at `path` (from the root, the root's own name first).
    pub fn widget(&self, path: &str) -> Option<&TapeWidget> {
        self.widgets.iter().find(|w| w.path == path)
    }
}

/// A tree the native HUD painted as the tape writes one: the widgets the last
/// paint visited (shown, a switcher's active child), a parent before its children, with their painted geometry. The
/// brush is the object path the native widget holds, not the bundle's name.
pub fn root_of(owner: &str, t: &Tree) -> TapeRoot {
    let mut widgets = Vec::new();
    walk(t, t.root(), t.widget(t.root()).name.clone(), 1.0, &mut widgets);
    TapeRoot { owner: owner.to_string(), widgets }
}

pub(crate) fn walk(t: &Tree, w: WidgetId, path: String, alpha: f64, out: &mut Vec<TapeWidget>) {
    let x = t.widget(w);
    if !x.shown() {
        return;
    }
    let Some(p) = t.painted(w) else { return };
    let a = alpha * x.opacity;
    let vis = match x.vis {
        Vis::Visible => 0,
        Vis::Collapsed => 1,
        Vis::Hidden => 2,
        Vis::HitTestInvisible => 3,
        Vis::SelfHitTestInvisible => 4,
    };
    let text = x.kind == "TextBlock";
    let mid = x.mid.as_ref().map(|m| TapeMid {
        s: m.scalars.iter().map(|(k, v)| (k.clone(), f64::from(*v))).collect(),
        v: m.vectors.iter().map(|(k, v)| (k.clone(), v.map(f64::from))).collect(),
    });
    out.push(TapeWidget {
        path: path.clone(),
        ty: x.kind.clone(),
        vis,
        opacity: x.opacity,
        alpha: a,
        matrix: p.transform,
        size: p.size,
        color: x.color,
        tex: x.tex.clone(),
        brush_color: (x.kind == "Border").then_some(x.brush_color),
        mid,
        text: text.then(|| x.shown_text()),
        font: text.then(|| TapeFont {
            face: x.typeface.clone(),
            size: x.font_size,
            outline: x.outline,
            outline_color: x.outline_color,
            spacing: x.letter_spacing,
            justify: format!("{:?}", x.justify),
            shadow: x.shadow_offset,
            shadow_color: x.shadow_color,
        }),
        active: (x.kind == "WidgetSwitcher").then(|| u32::try_from(x.active).unwrap_or(u32::MAX)),
    });
    let only =
        (x.kind == "WidgetSwitcher").then(|| usize::try_from(x.active).ok().and_then(|i| x.children.get(i)).copied());
    let mut seen: Vec<(&str, usize)> = Vec::new();
    for &c in &x.children {
        let name = t.widget(c).name.as_str();
        let n = match seen.iter_mut().find(|e| e.0 == name) {
            Some(e) => {
                e.1 += 1;
                e.1
            }
            None => {
                seen.push((name, 1));
                1
            }
        };
        let cp = if n > 1 { format!("{path}/{name}#{}", n - 1) } else { format!("{path}/{name}") };
        if only.is_none_or(|o| o == Some(c)) {
            walk(t, c, cp, a, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"tape":"hud","version":1,"run":"hud-run","size":[1920,1080],"worldSeed":1,"map":"map_training"}
{"tick":0,"now":0,"real":0,"roots":[]}
{"tick":60,"now":0,"real":1,"roots":[{"owner":"Countdown","widgets":[{"p":"Countdown","t":"User","v":4,"o":1,"a":1,"m":[0.75,0,0,0.75,0,0],"s":[2560,1440],"c":[1,1,1,1]},{"p":"Countdown/Text","t":"TextBlock","v":3,"o":0.5,"a":0.5,"m":[0.75,0,0,0.75,10,20.5],"s":[300,80],"c":[1,0.5,0,1],"text":"3","font":{"face":"Norms","size":64,"outline":2,"outlineColor":[0,0,0,1],"spacing":0,"justify":"Center","shadow":[1,1],"shadowColor":[0,0,0,0]}},{"p":"Countdown/Bg","t":"Image","v":3,"o":1,"a":1,"m":[1,0,0,1,0,0],"s":[10,10],"c":[1,1,1,1],"tex":"MI_Glow","mid":{"s":{"Progress":0.25},"v":{"Tint":[1,0,0,1]}}}]}]}
"#;

    #[test]
    fn a_tape_reads_back() {
        let t = HudTape::parse(SAMPLE).unwrap();
        assert_eq!(t.header.size, [1920, 1080]);
        assert_eq!(t.header.rest["worldSeed"], 1);
        assert_eq!(t.frames.len(), 2);
        let f = t.frame(60).unwrap();
        let r = f.root("Countdown").unwrap();
        let text = r.widget("Countdown/Text").unwrap();
        assert_eq!(text.text.as_deref(), Some("3"));
        assert_eq!(text.matrix[5], 20.5);
        assert_eq!(text.font.as_ref().unwrap().justify, "Center");
        let bg = r.widget("Countdown/Bg").unwrap();
        assert_eq!(bg.mid.as_ref().unwrap().s["Progress"], 0.25);
        assert!(t.frame(61).is_none());
    }

    #[test]
    fn a_field_the_reader_does_not_know_is_refused() {
        let s = SAMPLE.replace(r#""tick":0,"#, r#""tick":0,"extra":1,"#);
        assert!(HudTape::parse(&s).is_err());
    }

    /// The tape `HUD_TAPE` names (an oracle run's), when it is set: every frame reads, the ticks
    /// rise, every widget path starts at its root's name.
    #[test]
    fn the_runs_tape_reads() {
        let Some(p) = std::env::var_os("HUD_TAPE") else {
            eprintln!("note: HUD_TAPE names no tape: not checked");
            return;
        };
        let t = HudTape::read(Path::new(&p)).unwrap();
        assert!(!t.frames.is_empty());
        for w in t.frames.windows(2) {
            assert!(w[1].tick >= w[0].tick, "tick {} after {}", w[1].tick, w[0].tick);
        }
        for f in &t.frames {
            for r in &f.roots {
                let root = &r.widgets[0].path;
                assert!(
                    r.widgets.iter().all(|w| w.path == *root || w.path.starts_with(&format!("{root}/"))),
                    "{} at {}",
                    r.owner,
                    f.tick
                );
            }
        }
        eprintln!(
            "{}: {} frames, ticks {}..{}",
            Path::new(&p).display(),
            t.frames.len(),
            t.frames[0].tick,
            t.frames.last().unwrap().tick
        );
    }
}
