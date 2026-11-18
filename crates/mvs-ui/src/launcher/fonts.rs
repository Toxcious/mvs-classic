//! The launcher's type (app/launcher/styles.css's faces): the bundled Barlow, Barlow Condensed and Rubik, the
//! system's Segoe UI for body text and its monospace face for paths and logs; glyphs rasterised by FreeType at the size the window shows them,
//! packed in the painter's atlas; a line's width, its glyphs placed, paragraphs wrapped at word boundaries.

use std::collections::HashMap;

use mvs_freetype::{FaceId, LOAD_TARGET_LIGHT, Library};
use mvs_render::gpu::Gpu;

use super::paint::{ATLAS, Painter};
use crate::{Error, Result};

/// The page's faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    /// Segoe UI, else Rubik 400 (`--body`).
    // Deviation: the design's body text is Rubik 400; Segoe UI reads more easily at the launcher's small sizes.
    Body,
    /// Rubik 500.
    Medium,
    /// Rubik 600.
    Semi,
    /// Rubik 700.
    Bold,
    /// Barlow Condensed 800 italic (`--display`, the headings).
    Display,
    /// Barlow 900 italic (the wordmark's MVS).
    Wordmark,
    /// Cascadia Mono, else Consolas, else Rubik (`--mono`).
    Mono,
}

const FACES: [Face; 7] = [Face::Body, Face::Medium, Face::Semi, Face::Bold, Face::Display, Face::Wordmark, Face::Mono];

fn bundled(f: Face) -> &'static [u8] {
    match f {
        Face::Body | Face::Mono => include_bytes!("../../launcher/fonts/Rubik-Regular.ttf"),
        Face::Medium => include_bytes!("../../launcher/fonts/Rubik-Medium.ttf"),
        Face::Semi => include_bytes!("../../launcher/fonts/Rubik-SemiBold.ttf"),
        Face::Bold => include_bytes!("../../launcher/fonts/Rubik-Bold.ttf"),
        Face::Display => include_bytes!("../../launcher/fonts/BarlowCondensed-ExtraBoldItalic.ttf"),
        Face::Wordmark => include_bytes!("../../launcher/fonts/Barlow-BlackItalic.ttf"),
    }
}

/// A glyph placed in the atlas.
#[derive(Clone, Copy, Debug)]
struct Glyph {
    /// Atlas texels x, y, w, h.
    at: [u32; 4],
    /// The bitmap's left and top from the pen (top up).
    left: i32,
    top: i32,
    /// Advance, pixels.
    advance: f32,
}

/// One glyph of a laid-out line: its quad from the line's pen origin (baseline), its atlas texels.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
}

/// The faces and the glyphs rasterised so far.
pub struct Fonts {
    lib: Library,
    ids: HashMap<Face, FaceId>,
    /// units per em, ascender, descender, line height (font units)
    metrics: HashMap<Face, [f32; 4]>,
    /// By face, size, character and the pen's quarter-pixel phase.
    glyphs: HashMap<(Face, u32, char, u8), Glyph>,
    /// The packer's shelf: x, y, height.
    shelf: [u32; 3],
}

impl std::fmt::Debug for Fonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fonts").field("glyphs", &self.glyphs.len()).finish()
    }
}

fn ft(e: mvs_freetype::Error) -> Error {
    Error::Format { what: "launcher font".into(), why: e.to_string() }
}

impl Fonts {
    pub fn new() -> Result<Fonts> {
        let mut lib = Library::new().map_err(ft)?;
        let mut ids = HashMap::new();
        let mut metrics = HashMap::new();
        let windir = std::env::var_os("WINDIR").map_or_else(|| "C:\\Windows".into(), std::path::PathBuf::from);
        for f in FACES {
            let system: &[&str] = match f {
                Face::Body => &["segoeui.ttf"],
                Face::Mono => &["CascadiaMono.ttf", "consola.ttf"],
                _ => &[],
            };
            let bytes = system
                .iter()
                .find_map(|n| std::fs::read(windir.join("Fonts").join(n)).ok())
                .unwrap_or_else(|| bundled(f).to_vec());
            let id = lib.open(bytes).map_err(ft)?;
            let i = lib.info(id);
            metrics.insert(f, [i.units_per_em as f32, i.ascender as f32, i.descender as f32, i.height as f32]);
            ids.insert(f, id);
        }
        Ok(Fonts { lib, ids, metrics, glyphs: HashMap::new(), shelf: [0, 0, 0] })
    }

    /// The face's ascender and descender (below, positive) at `px`.
    pub fn ascent(&self, f: Face, px: f32) -> (f32, f32) {
        let [em, a, d, _] = self.metrics[&f];
        (a / em * px, -d / em * px)
    }

    /// CSS's `line-height: normal` of the face at `px`.
    pub fn normal(&self, f: Face, px: f32) -> f32 {
        let [em, _, _, h] = self.metrics[&f];
        h / em * px
    }

    /// Where a line of `px` text in a line box of `line_h` puts its baseline below the box's top (CSS: the content
    /// area centred in the line box).
    pub fn baseline(&self, f: Face, px: f32, line_h: f32) -> f32 {
        let (a, d) = self.ascent(f, px);
        (line_h - (a + d)) * 0.5 + a
    }

    /// A glyph rasterised with its pen at `phase` quarters of a pixel right of a whole pixel.
    fn glyph(&mut self, gpu: Option<(&Gpu, &Painter)>, f: Face, ppem: u32, c: char, phase: u8) -> Glyph {
        if let Some(g) = self.glyphs.get(&(f, ppem, c, phase)) {
            return *g;
        }
        let id = self.ids[&f];
        let none = Glyph { at: [0; 4], left: 0, top: 0, advance: 0.0 };
        if self.lib.set_pixel_size(id, ppem).is_err() {
            return none;
        }
        let index = self.lib.char_index(id, c);
        let advance = self.lib.advance(id, index, LOAD_TARGET_LIGHT).map_or(0.0, |a| a as f32 / 65536.0);
        let mut g = Glyph { advance, ..none };
        if let Some((gpu, painter)) = gpu
            && !c.is_whitespace()
            && let Ok(b) = self.lib.bitmap_at(id, index, LOAD_TARGET_LIGHT, 0, i64::from(phase) * 16)
        {
            let (w, h) = (b.width, b.rows);
            if self.shelf[0] + w + 1 > ATLAS {
                self.shelf = [0, self.shelf[1] + self.shelf[2] + 1, 0];
            }
            if self.shelf[1] + h + 1 > ATLAS {
                // (a full atlas starts again: the glyphs already placed are drawn again next frame)
                self.glyphs.clear();
                self.shelf = [0, 0, 0];
            }
            let at = [self.shelf[0], self.shelf[1], w, h];
            painter.atlas_write(gpu, [at[0], at[1]], w, h, &b.pixels);
            self.shelf[0] += w + 1;
            self.shelf[2] = self.shelf[2].max(h);
            g = Glyph { at, left: b.left, top: b.top, advance };
        }
        // (a measure-only call caches nothing: the glyph is rasterised at its first draw)
        if gpu.is_some() {
            self.glyphs.insert((f, ppem, c, phase), g);
        }
        g
    }

    /// The width of `text` at `px` with `spacing` added after each character (CSS letter-spacing), pixels.
    pub fn width(&mut self, f: Face, px: f32, spacing: f32, text: &str) -> f32 {
        let ppem = ppem(px);
        let k = px / ppem as f32;
        text.chars().map(|c| self.glyph(None, f, ppem, c, 0).advance * k + spacing).sum()
    }

    /// `text`'s glyphs from the pen at 0, 0 (baseline), rasterised as needed.
    // Deviation: no kerning and no shaping (the advances alone): a pair Chromium kerns sits a little apart here.
    // Each glyph sits at its pen to a quarter pixel (a bitmap per phase), so the gaps between letters stay even.
    pub fn layout(&mut self, gpu: &Gpu, painter: &Painter, f: Face, px: f32, spacing: f32, text: &str) -> Vec<Placed> {
        let ppem = ppem(px);
        let k = px / ppem as f32;
        let mut x = 0.0f32;
        let mut out = Vec::new();
        for c in text.chars() {
            let quarters = (x * 4.0).round();
            let whole = (quarters / 4.0).floor();
            let phase = (quarters - whole * 4.0) as u8;
            let g = self.glyph(Some((gpu, painter)), f, ppem, c, phase);
            if g.at[2] > 0 {
                let [ax, ay, w, h] = g.at.map(|v| v as f32);
                let left = whole + g.left as f32;
                out.push(Placed { rect: [left, -g.top as f32, w, h], uv: [ax, ay, ax + w, ay + h] });
            }
            x += g.advance * k + spacing;
        }
        out
    }

    /// `text` cut with an ellipsis to fit `max` (CSS text-overflow: ellipsis).
    pub fn ellipsis(&mut self, f: Face, px: f32, spacing: f32, text: &str, max: f32) -> String {
        if self.width(f, px, spacing, text) <= max {
            return text.to_string();
        }
        let dots = self.width(f, px, spacing, "\u{2026}");
        let mut out = String::new();
        let mut w = 0.0;
        for c in text.chars() {
            let cw = self.width(f, px, spacing, c.encode_utf8(&mut [0; 4]));
            if w + cw + dots > max {
                break;
            }
            w += cw;
            out.push(c);
        }
        out.push('\u{2026}');
        out
    }

    /// `text` broken into lines no wider than `max` at its spaces; a word longer than a line (a path) is broken after
    /// its last `\` or `/` that fits, else between characters.
    pub fn wrap(&mut self, f: Face, px: f32, text: &str, max: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for para in text.split('\n') {
            let mut line = String::new();
            for word in para.split(' ') {
                if self.width(f, px, 0.0, word) > max {
                    if !line.is_empty() {
                        lines.push(std::mem::take(&mut line));
                    }
                    let mut parts = self.break_word(f, px, word, max);
                    line = parts.pop().unwrap_or_default();
                    lines.extend(parts);
                    continue;
                }
                let next = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
                if !line.is_empty() && self.width(f, px, 0.0, &next) > max {
                    lines.push(std::mem::replace(&mut line, word.to_string()));
                } else {
                    line = next;
                }
            }
            lines.push(line);
        }
        lines
    }

    /// A word wider than `max` in pieces that fit: each ends after its last `\` or `/`, else where the next
    /// character would pass `max`.
    fn break_word(&mut self, f: Face, px: f32, word: &str, max: f32) -> Vec<String> {
        let mut parts = Vec::new();
        let mut cur = String::new();
        for ch in word.chars() {
            cur.push(ch);
            if cur.chars().count() > 1 && self.width(f, px, 0.0, &cur) > max {
                cur.pop();
                // (the piece up to its last separator, the rest carried to the next)
                let cut = cur.rfind(['\\', '/']).map_or(cur.len(), |i| i + 1);
                let rest = cur.split_off(cut);
                parts.push(std::mem::replace(&mut cur, rest));
                cur.push(ch);
            }
        }
        parts.push(cur);
        parts
    }
}

/// The FreeType size a CSS size draws at: whole pixels per em.
fn ppem(px: f32) -> u32 {
    px.round().max(1.0) as u32
}
