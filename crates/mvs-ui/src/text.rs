//! Text (metrics and line layout): the font kind's faces through
//! FreeType at whole-pixel sizes, the kerning-only shaper (FSlateTextRun), letter spacing, outline glyphs, the glyph
//! atlas the element batches sample; the player's localization (Game.locres and the key names of Engine.locres, the
//! text kind).

use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;
use mvs_freetype::{FaceId, Library};
use serde::Deserialize;

use crate::bundle::Bundle;
use crate::key_names::KeyNames;
use crate::{Error, Result};

mod emoji;

/// The text kind's entry of the Game target in the culture the game draws in (English, the build's default).
pub const GAME_TEXT: &str = "/Game/Localization/Game/en/Game.Game";

/// The text kind's entry of the Engine target in the same culture: the keys' display names.
pub const ENGINE_TEXT: &str = "/Engine/Localization/Engine/en/Engine.Engine";

/// A localization target's strings: namespace -> key -> string.
type Table = HashMap<String, HashMap<String, String>>;

/// The player's localization: the Game target's strings and the key names (the Game target's and the Engine
/// target's, `KeyNames`).
#[derive(Debug, Default)]
pub struct GameText {
    table: Table,
    keys: Arc<KeyNames>,
}

impl GameText {
    /// The text kind's Game and Engine entries.
    pub fn load(store: &mvs_store::Store) -> Result<GameText> {
        /// The Engine target's one namespace the game reads (the others skipped as they are parsed).
        #[derive(Deserialize)]
        struct Engine {
            #[serde(rename = "InputKeys", default)]
            input_keys: HashMap<String, String>,
        }
        let text = store.kind("text")?;
        let bytes = text.bytes(GAME_TEXT)?;
        let table: Table = serde_json::from_slice(&bytes)
            .map_err(|e| Error::Format { what: GAME_TEXT.to_string(), why: e.to_string() })?;
        // The Engine entry only supplies key display names: a store without it reads every key as its own name
        // rather than failing the game's start.
        let input_keys = match text.bytes(ENGINE_TEXT) {
            Ok(bytes) => {
                serde_json::from_slice::<Engine>(&bytes)
                    .map_err(|e| Error::Format { what: ENGINE_TEXT.to_string(), why: e.to_string() })?
                    .input_keys
            }
            Err(e) => {
                eprintln!("[ui] {ENGINE_TEXT}: {e}; key names fall back to the key's own name");
                HashMap::new()
            }
        };
        let keys = Arc::new(KeyNames::new(table.get(""), input_keys));
        Ok(GameText { table, keys })
    }

    /// The string of `key` in namespace `ns` (the empty namespace for most of the Game target's).
    pub fn get(&self, ns: &str, key: &str) -> Option<&str> {
        self.table.get(ns)?.get(key).map(String::as_str)
    }

    /// The key names (UI_Functions GetKeyLocalizedName).
    pub fn key_names(&self) -> &Arc<KeyNames> {
        &self.keys
    }
}

/// A font face's design metrics as the ui kind's bundle holds them, font units: the
/// line height and descender chosen by the FontFace asset's LayoutMethod (Metrics: the hhea line height and
/// descender; BoundingBox: the head table's yMax - yMin and yMin).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct FaceMetrics {
    pub upm: i32,
    pub height: i32,
    pub descender: i32,
}

/// A text run's font (FSlateFontInfo as a TextBlock gives it, with the block's shadow and wrapping).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FontInfo {
    /// The typeface name (the bundle's `typefaces`); empty, or a name the composite lacks, for the default typeface.
    pub typeface: String,
    /// The size, Slate units (points at 96 DPI).
    pub size: f64,
    /// LetterSpacing, 1/1000 em.
    pub letter_spacing: f64,
    /// OutlineSettings.OutlineSize.
    pub outline: f64,
    /// OutlineSettings.bSeparateFillAlpha: the outline glyph leaves the fill's inside clear.
    pub separate_fill_alpha: bool,
    /// The block's ShadowOffset, Slate units (the run's height counts |Y|).
    pub shadow: [f64; 2],
    /// The wrapping width, Slate units (`wrap_width`); 0: no wrapping.
    pub wrap: f64,
}

/// A run's metrics at a layout scale: in Slate units.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextMetrics {
    /// The widest line's `draw` width (no outline or shadow at the ends: the run's measure adds them).
    pub width: f64,
    /// The line top to the glyph baseline.
    pub ascent: f64,
    /// One line's run height (FSlateTextRun::GetMaxHeight).
    pub height: f64,
    /// The fill glyphs' shift inside the outline.
    pub fill_x: f64,
    pub lines: Vec<TextLine>,
}

/// A laid out line: its width without the spaces at a soft break (JustificationWidth) and with them (LineSize.X).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextLine {
    pub width: f64,
    pub draw: f64,
}

/// A TextBlock's Justification (ETextJustify).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Justify {
    #[default]
    Left,
    Center,
    Right,
}

impl TextMetrics {
    /// Line `i`'s top left in a block `allotted` wide, Slate units. FTextLayout::JustifyLayout
    /// (exe 1417a5fa0) offsets a line by (max(allotted width, layout width) - line width) x 0.5 (Center) or x 1
    /// (Right), and FSlateTextBlockLayout::OnPaint (14182ee20) scrolls a layout wider than the allotted width back
    /// by (layout width - allotted width) x the same factor: a line is justified inside the allotted width,
    /// overflowing both ways when centred. The line width is the run's measure (the outline at both ends, |shadow
    /// X|). FTextLayout::CreateLineViewBlocks (1417988c0): lines are run height x LineHeightPercentage apart, so a
    /// shortened line keeps its glyphs at the top and the overflow goes down.
    pub fn line_origin(&self, i: usize, f: &FontInfo, allotted: f64, justify: Justify, line_height: f64) -> [f64; 2] {
        let jx = match justify {
            Justify::Left => 0.0,
            Justify::Center => 0.5,
            Justify::Right => 1.0,
        };
        let ends = 2.0 * f.outline + f.shadow[0].abs();
        [(allotted - self.lines[i].width - ends) * jx, i as f64 * self.height * line_height]
    }
}

/// A shaped run: each glyph's atlas entry and pen place, pixels at the layout scale; the outline layer's glyphs
/// first (drawn under the fill), then the fill's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedRun {
    pub glyphs: Vec<ShapedGlyph>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShapedGlyph {
    /// The glyph's entry in the atlas (`Fonts::glyph`); an entry of size 0 (a space) draws nothing.
    pub atlas: u32,
    /// The pen's place of the glyph's origin from its line's left (the fill layer's glyphs shifted by
    /// round(OutlineSize x scale)) and baseline's place from its line's top, pixels.
    pub x: f64,
    pub y: f64,
    /// The outline layer's glyph (drawn under the fill).
    pub outline: bool,
    /// The line it is on (`TextMetrics::lines`).
    pub line: u32,
    /// The glyph's own colour, sRGB RGBA, drawn in place of the text's (a colour emoji's layer; its alpha times the
    /// text's); None: the text's colour.
    pub color: Option<[u8; 4]>,
    /// What the glyph is of a colour emoji, if anything.
    pub part: GlyphPart,
}

/// A glyph's part in a colour emoji: a shadow draws the silhouette in place of the layers (which would darken where
/// they overlap); the rest draws the layers and skips the silhouette.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GlyphPart {
    /// A glyph of its own (no emoji's, or an emoji without colour layers).
    #[default]
    Whole,
    /// A colour emoji's base glyph: its whole shape in one.
    Silhouette,
    /// One of a colour emoji's COLR layers.
    Layer,
}

/// A glyph's place in the atlas: its page, its rect (x, y, w, h, atlas pixels) and its offset from the pen
/// (HorizontalOffset right, VerticalOffset up to the bitmap's top): the quad is at (x + offset[0], y - offset[1]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlyphEntry {
    pub page: u16,
    pub rect: [u16; 4],
    pub offset: [i16; 2],
}

/// The glyph atlas: R8 coverage pages of `size`.
#[derive(Clone, Debug, Default)]
pub struct GlyphAtlas {
    /// Which `Fonts` the atlas is (one per `Fonts::new`): an element renderer keeps the GPU pages of each it draws
    /// with apart, as each packs its own glyphs from the same corner.
    pub id: u64,
    pub size: [u32; 2],
    pub pages: Vec<AtlasPage>,
}

/// One R8 page and the rect written since the last upload (x0, y0, x1, y1).
#[derive(Clone, Debug, Default)]
pub struct AtlasPage {
    pub pixels: Vec<u8>,
    pub dirty: Option<[u32; 4]>,
    // The shelf packer: the current row's top, its height and the next free x.
    shelf: [u32; 3],
}

// Unverified: FSlateFontAtlas's page size (Slate's grayscale font atlas, 1024 x 1024) and its one pixel of zero
// padding around each glyph (ESlateTextureAtlasPaddingStyle::PadWithZero) are 4.26 defaults, not read in the exe.
const ATLAS_SIZE: u32 = 1024;
/// The next `GlyphAtlas::id` (1 up: 0 is no atlas).
static NEXT_ATLAS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
const ATLAS_PAD: u32 = 1;

// The font cache's load flags (advance cache 141641100) for a scalable face whose FontFace has no Hinting property
// (EFontHinting::Default): FT_LOAD_COLOR | FT_LOAD_NO_BITMAP, the TrueType hinter at the normal target.
// Unverified: that FSlateFontRenderer::GetRenderData loads the glyph it rasterises with the same flags and renders
// it in FT_RENDER_MODE_NORMAL is from the 4.26 source.
const LOAD_FLAGS: i32 = mvs_freetype::LOAD_COLOR | mvs_freetype::LOAD_NO_BITMAP;

/// The wrapping width of a TextBlock, Slate units: with AutoWrapText the width of the block's
/// last geometry when that is at least 1, and the smaller of it and WrapTextAt when both are set.
/// Unverified: FSlateTextBlockLayout::CalculateWrappingWidth from the 4.26 source.
pub fn wrap_width(auto_wrap: bool, last_width: f64, wrap_at: f64) -> f64 {
    let auto = if auto_wrap && last_width >= 1.0 { last_width } else { 0.0 };
    if auto > 0.0 && wrap_at > 0.0 {
        auto.min(wrap_at)
    } else if auto != 0.0 {
        auto
    } else {
        wrap_at
    }
}

// FreeType's 16.16 and 26.6 helpers (FT_MulFix exe 143854990, FT_PIX_ROUND).
fn mul_fix(a: i64, b: i64) -> i64 {
    let p = a * b;
    if p < 0 { -((-p + 0x8000) >> 16) } else { (p + 0x8000) >> 16 }
}

fn pix_round(v: i64) -> i64 {
    (v + 32) >> 6
}

/// Rounds half up: floor(v + 0.5).
fn js_round(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// FreeTypeUtils::ApplySizeAndScale (exe 14163c460): the 26.6 pixel size is FT_MulFix((Size x 64 x 96 + 36) / 72,
/// trunc(scale x 65536)) and a scalable face is set to its rounding with FT_Set_Pixel_Sizes, so glyphs are always at a
/// whole pixel size (Slate sizes are points at 96 DPI).
pub fn pixel_size(size: f64, scale: f64) -> u32 {
    let pt = (size.trunc() as i64 * 0x1800 + 0x24) / 0x48;
    let px = pix_round(mul_fix(pt, (scale * 65536.0).trunc() as i64)).max(1);
    u32::try_from(px).unwrap_or(u32::MAX)
}

struct Face {
    id: FaceId,
    metrics: FaceMetrics,
    kerning: bool,
    /// The glyph load flags.
    flags: i32,
    /// The pixel size the FreeType face is set to now and its 16.16 scale.
    ppem: u32,
    scale: i64,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct LayoutKey {
    face: usize,
    size: u64,
    letter_spacing: u64,
    outline: u64,
    shadow_y: u64,
    wrap: u64,
    scale: u64,
    text: String,
}

/// A laid out text: its metrics and each line's glyphs (face, glyph index, pen in pixels).
struct Layout {
    metrics: TextMetrics,
    ppem: u32,
    glyphs: Vec<Vec<(usize, u32, i64)>>,
    ascent_px: f64,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    face: usize,
    ppem: u32,
    glyph: u32,
    /// The outline's stroke radius, whole pixels (0: the fill).
    stroke: u32,
    separate: bool,
}

// (the port's own) The layout cache's bound: a clock or a damage number lays out a new text every second or hit.
const LAYOUT_MAX: usize = 4096;

/// The faces, their glyph caches and the atlas.
pub struct Fonts {
    lib: Library,
    faces: Vec<Face>,
    /// Typeface name -> face.
    typefaces: IndexMap<String, usize>,
    default: usize,
    advances: HashMap<(usize, u32, u32), i64>,
    layouts: HashMap<LayoutKey, Layout>,
    glyphs: HashMap<GlyphKey, u32>,
    entries: Vec<GlyphEntry>,
    atlas: GlyphAtlas,
    emoji: Emoji,
}

/// The fallback emoji face (`text/emoji.rs`): its file read and its tables parsed on a thread from the start, the
/// face opened the first time a text needs it.
enum Emoji {
    Loading(std::thread::JoinHandle<Option<(Vec<u8>, Box<emoji::Tables>)>>),
    Missing,
    Open { face: usize, tables: Box<emoji::Tables> },
}

impl std::fmt::Debug for Fonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fonts")
            .field("faces", &self.faces.len())
            .field("glyphs", &self.entries.len())
            .field("pages", &self.atlas.pages.len())
            .finish()
    }
}

fn ft(what: &str, e: mvs_freetype::Error) -> Error {
    Error::Format { what: what.to_string(), why: e.to_string() }
}

impl Fonts {
    /// The bundle's typefaces' faces from the font kind.
    pub fn new(store: &mvs_store::Store, b: &Bundle) -> Result<Fonts> {
        let pack = store.kind("font")?;
        let mut lib = Library::new().map_err(|e| ft("FreeType", e))?;
        let mut faces: Vec<Face> = Vec::new();
        let mut by_path: IndexMap<&str, usize> = IndexMap::new();
        let mut typefaces = IndexMap::new();
        for (name, path) in &b.typefaces {
            let i = match by_path.get(path.as_str()) {
                Some(&i) => i,
                None => {
                    let metrics = *b.faces.get(path).ok_or_else(|| Error::Format {
                        what: path.clone(),
                        why: format!("typeface {name}: the bundle has no metrics for its face"),
                    })?;
                    // The font kind's entry is the FontFace's object path.
                    let base = path.rsplit('/').next().unwrap_or(path);
                    let entry = format!("{path}.{base}");
                    let id = lib.open(pack.bytes(&entry)?).map_err(|e| ft(&entry, e))?;
                    let info = lib.info(id);
                    if !info.scalable {
                        return Err(Error::Format { what: entry, why: "not a scalable face".into() });
                    }
                    faces.push(Face { id, metrics, kerning: info.kerning, flags: LOAD_FLAGS, ppem: 0, scale: 0 });
                    by_path.insert(path, faces.len() - 1);
                    faces.len() - 1
                }
            };
            typefaces.insert(name.clone(), i);
        }
        let default = *typefaces.get(&b.default_typeface).ok_or_else(|| Error::Format {
            what: b.default_typeface.clone(),
            why: "the default typeface is not among the bundle's typefaces".into(),
        })?;
        Ok(Fonts {
            lib,
            faces,
            typefaces,
            default,
            advances: HashMap::new(),
            layouts: HashMap::new(),
            glyphs: HashMap::new(),
            entries: Vec::new(),
            atlas: GlyphAtlas {
                id: NEXT_ATLAS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                size: [ATLAS_SIZE, ATLAS_SIZE],
                pages: Vec::new(),
            },
            emoji: Fonts::load_emoji(),
        })
    }

    /// The emoji face's file and tables, read on a thread (a 13 MB file: not in the frame that first draws an emoji).
    fn load_emoji() -> Emoji {
        let read = || {
            let bytes = std::fs::read(emoji::path()?).ok()?;
            let tables = Box::new(emoji::Tables::parse(&bytes));
            Some((bytes, tables))
        };
        match std::thread::Builder::new().name("emoji face".into()).spawn(read) {
            Ok(h) => Emoji::Loading(h),
            Err(_) => Emoji::Missing,
        }
    }

    /// The emoji face's index, opened on first use (waiting for its read if still running); None where the system
    /// has none.
    fn emoji_face(&mut self) -> Option<usize> {
        if let Emoji::Loading(_) = self.emoji {
            let Emoji::Loading(h) = std::mem::replace(&mut self.emoji, Emoji::Missing) else { unreachable!() };
            if let Some((bytes, tables)) = h.join().ok().flatten()
                && let Ok(id) = self.lib.open(bytes)
                && self.lib.info(id).scalable
            {
                // FT_LOAD_COLOR left out: the layers are drawn one by one from the COLR table, each a coverage glyph
                // in its colour.
                let flags = mvs_freetype::LOAD_NO_BITMAP;
                let metrics = FaceMetrics::default();
                self.faces.push(Face { id, metrics, kerning: false, flags, ppem: 0, scale: 0 });
                self.emoji = Emoji::Open { face: self.faces.len() - 1, tables };
            }
        }
        match self.emoji {
            Emoji::Open { face, .. } => Some(face),
            _ => None,
        }
    }

    fn emoji_tables(&self) -> Option<(usize, &emoji::Tables)> {
        match &self.emoji {
            Emoji::Open { face, tables } => Some((*face, tables)),
            _ => None,
        }
    }

    /// A hard line's glyphs: (character, face, glyph) in order.
    // Deviation: the build's composite fonts have no emoji; a character the face lacks takes its .notdef glyph
    // (FT_Get_Char_Index 0), not the composite font's fallback typeface. The port draws an emoji (a player's chat or
    // name) from the system's colour emoji face: a run of characters the face lacks and the emoji face has (with the
    // joiners after them, and a character the face has that VS16 asks in its emoji form) goes there, shaped into
    // whole emoji.
    fn glyphs_of(&mut self, face: usize, line: &str) -> Vec<(char, usize, u32)> {
        let id = self.faces[face].id;
        let chars: Vec<char> = line.chars().collect();
        let lacks: Vec<bool> = chars
            .iter()
            .enumerate()
            .map(|(k, &c)| {
                // (VS16 asks a symbol, a digit, # or * in its emoji form: the emoji face has letters, never emoji)
                let asked = chars.get(k + 1) == Some(&emoji::EMOJI_FORM) && !c.is_ascii_alphabetic();
                self.lib.char_index(id, c) == 0 || asked
            })
            .collect();
        let mut out = Vec::with_capacity(chars.len());
        let mut i = 0;
        while i < chars.len() {
            let ef = if lacks[i] && !emoji::joins(chars[i]) { self.emoji_face() } else { None };
            let has = |lib: &Library, k: usize| ef.is_some_and(|ef| lib.char_index(self.faces[ef].id, chars[k]) != 0);
            let Some(ef) = ef.filter(|_| has(&self.lib, i)) else {
                // (a joiner or selector the face lacks, outside an emoji, draws nothing)
                let g = self.lib.char_index(id, chars[i]);
                if g != 0 || !emoji::invisible(chars[i]) {
                    out.push((chars[i], face, g));
                }
                i += 1;
                continue;
            };
            let start = i;
            i += 1;
            while i < chars.len() && (emoji::joins(chars[i]) || (lacks[i] && has(&self.lib, i))) {
                i += 1;
            }
            let eid = self.faces[ef].id;
            let mut run: Vec<(u16, usize)> =
                (start..i).map(|k| (u16::try_from(self.lib.char_index(eid, chars[k])).unwrap_or(0), k)).collect();
            if let Some((_, t)) = self.emoji_tables() {
                t.shape(&mut run);
            }
            // a selector or joiner the shaping left as it was draws nothing (the face gives it an emoji's advance)
            let left =
                |g: u16, k: usize| emoji::invisible(chars[k]) && u32::from(g) == self.lib.char_index(eid, chars[k]);
            out.extend(run.into_iter().filter(|&(g, k)| !left(g, k)).map(|(g, k)| (chars[k], ef, u32::from(g))));
        }
        out
    }

    /// The PFG composite's face of a typeface: a name it lacks ("Display 1") takes the default typeface's.
    fn face_of(&self, typeface: &str) -> usize {
        self.typefaces.get(typeface).copied().unwrap_or(self.default)
    }

    fn set_size(&mut self, face: usize, ppem: u32) {
        let f = &mut self.faces[face];
        if f.ppem == ppem {
            return;
        }
        let [_, y] =
            self.lib.set_pixel_size(f.id, ppem).unwrap_or_else(|e| panic!("text: face {face} at {ppem} px: {e}"));
        f.ppem = ppem;
        f.scale = y;
    }

    /// The kerning-only shaper's advance of a glyph (exe 14164c940): ((FT_Get_Advance 16.16 + 0x200) >> 10 + 0x20)
    /// >> 6, whole pixels. The faces have no Hinting property, so the advance is the hinted glyph's.
    fn advance(&mut self, face: usize, ppem: u32, glyph: u32) -> i64 {
        if let Some(&a) = self.advances.get(&(face, ppem, glyph)) {
            return a;
        }
        self.set_size(face, ppem);
        let (id, flags) = (self.faces[face].id, self.faces[face].flags);
        // a glyph the face cannot load (past its count: a system emoji file other than the one its tables were read
        // from) takes .notdef's advance, else none
        let a = self.lib.advance(id, glyph, flags).or_else(|_| self.lib.advance(id, 0, flags)).unwrap_or(0);
        let px = (((a + 0x200) >> 10) + 0x20) >> 6;
        self.advances.insert((face, ppem, glyph), px);
        px
    }

    fn layout(&mut self, f: &FontInfo, text: &str, scale: f64) -> &Layout {
        let face = self.face_of(&f.typeface);
        let key = LayoutKey {
            face,
            size: f.size.to_bits(),
            letter_spacing: f.letter_spacing.to_bits(),
            outline: f.outline.to_bits(),
            shadow_y: f.shadow[1].to_bits(),
            wrap: f.wrap.to_bits(),
            scale: scale.to_bits(),
            text: text.to_string(),
        };
        if !self.layouts.contains_key(&key) {
            if self.layouts.len() >= LAYOUT_MAX {
                self.layouts.clear();
            }
            let l = self.lay_out(face, f, text, scale);
            self.layouts.insert(key.clone(), l);
        }
        &self.layouts[&key]
    }

    fn lay_out(&mut self, face: usize, f: &FontInfo, text: &str, s: f64) -> Layout {
        let p = pixel_size(f.size, s);
        self.set_size(face, p);
        let fx = self.faces[face].scale;
        let kerning = self.faces[face].kerning;
        let id = self.faces[face].id;
        // LetterSpacing: Size x LetterSpacing / 1000 as an int32 division (truncated toward zero; Size is the point
        // size) in pixels, not scaled with the font, added to every glyph but the last; so -25 at size 32 is 0 and
        // -100 at size 16 is -1 px.
        let sp = ((f.size.trunc() * f.letter_spacing) / 1000.0).trunc() as i64;
        // Wrapping (FTextLayout::FlowLineLayout 14179ca90): a hard line wider than the wrapping width x scale breaks
        // before the first word whose trimmed end passes it. The spaces at a break belong to the line they end
        // (CreateLineViewBlocks 1417988c0 runs to the break candidate's ActualRange end), so a line's draw width
        // runs to the end of that whitespace and the layout's DrawWidth (+0xb8) is the widest such line; its
        // JustificationWidth is the trimmed one. Unverified: that the TextBlock's desired width is the DrawWidth
        // (FTextLayout::GetSize, not GetWrappedSize) is from the 4.26 source.
        let wrap = if f.wrap > 0.0 { f.wrap * s } else { f64::INFINITY };
        let mut lines = Vec::new();
        let mut glyph_lines = Vec::new();
        // Hard line breaks (CR LF or LF) make separate lines; a CR not followed by LF stays a character.
        let hards: Vec<&str> = text.split('\n').collect();
        let last = hards.len() - 1;
        for (hi, &hard) in hards.iter().enumerate() {
            let hard = if hi < last { hard.strip_suffix('\r').unwrap_or(hard) } else { hard };
            // character, (face, glyph), pen x and advance in pixels
            let mut px: Vec<(char, (usize, u32), i64, i64)> = Vec::new();
            let (mut x, mut adv) = (0i64, 0i64);
            let mut prev: Option<(char, usize, u32)> = None;
            for (ch, gf, g) in self.glyphs_of(face, hard) {
                if let Some((pc, pf, pg)) = prev {
                    // Kerning (cache 1416415c0): FT_Get_Kerning in FT_KERNING_DEFAULT mode when the face has a kern
                    // table, (x + 0x20) >> 6 added to the previous glyph.
                    // Deviation: no kerning on a pair with a space (the faces' kern tables pull T, K, O... toward it,
                    // so the narrow spaces of the condensed faces read as missing).
                    let k = if kerning && pc != ' ' && ch != ' ' && pf == face && gf == face {
                        let v = self.lib.kerning(id, pg, g).unwrap_or_else(|e| panic!("text: kerning: {e}"));
                        (v + 0x20) >> 6
                    } else {
                        0
                    };
                    x += adv + k + sp;
                }
                adv = self.advance(gf, p, g);
                px.push((ch, (gf, g), x, adv));
                prev = Some((ch, gf, g));
            }
            if (x + adv) as f64 <= wrap {
                let w = (x + adv) as f64 / s;
                lines.push(TextLine { width: w, draw: w });
                glyph_lines.push(px.iter().map(|&(_, (gf, g), gx, _)| (gf, g, gx)).collect());
                continue;
            }
            // `a`: the line's first character; `end`: the last character of the line's last whole word.
            let mut a = 0usize;
            let mut end: Option<usize> = None;
            let mut flush = |a: usize, to: usize, last: usize| {
                let x0 = px[a].2;
                lines.push(TextLine {
                    width: (px[to].2 + px[to].3 - x0) as f64 / s,
                    draw: (px[last].2 + px[last].3 - x0) as f64 / s,
                });
                glyph_lines.push(px[a..=to].iter().map(|&(_, (gf, g), gx, _)| (gf, g, gx - x0)).collect());
            };
            for i in 0..px.len() {
                if px[i].0 == ' ' {
                    continue;
                }
                let word_end = i + 1 == px.len() || px[i + 1].0 == ' ';
                if !word_end {
                    continue;
                }
                if let Some(e) = end
                    && e >= a
                    && (px[i].2 + px[i].3 - px[a].2) as f64 > wrap
                {
                    let mut next = e + 1;
                    while next < px.len() && px[next].0 == ' ' {
                        next += 1;
                    }
                    flush(a, e, next - 1);
                    a = next;
                }
                end = Some(i);
            }
            if a < px.len() {
                flush(a, px.len() - 1, px.len() - 1);
            }
        }
        // The font measure's max character height = round(FT_MulFix(height, y scale)) + trunc(OutlineSize x scale)
        // as a uint16, and the baseline (141642bb0) = round(FT_MulFix(descender, y scale)); height / descender by the
        // face's LayoutMethod. FSlateTextRun::GetMaxHeight (14179fbb0) = trunc(OutlineSize x scale + |shadow Y| +
        // that max height): the line holds the outline twice and the shadow unscaled. The shaped sequence's own max
        // text height has no outline term. FSlateTextRun::Measure (1417a6620) gives the block height |shadow Y x
        // scale| + sequence height + OutlineSize x scale, GetBaseLine (14179daa0) = trunc(baseline - (OutlineSize x
        // scale + min(shadow Y, 0))), and FTextLayout::CreateLineViewBlocks (1417988c0) puts the block at
        // (int16)(MaxAbove - block height - block baseline) under the line top with MaxAbove = (int16)(run max
        // height + block baseline); the batcher puts the glyph baseline at sequence height + baseline below the
        // element's top, the fill layer shifted right by round(OutlineSize x scale) (14165fcd0).
        let m = self.faces[face].metrics;
        let shadow_y = f.shadow[1];
        let o = f64::from((f.outline * s) as f32);
        let seq_h = pix_round(mul_fix(i64::from(m.height), fx)) & 0xffff;
        let face_h = (seq_h + o.trunc() as i64) & 0xffff;
        let base = pix_round(mul_fix(i64::from(m.descender), fx));
        let run_h = ((o + shadow_y.abs() + face_h as f64).trunc() as i64) & 0xffff;
        let block_base = (base as f64 - (o + shadow_y.min(0.0))).trunc();
        let block_h = f64::from(((shadow_y * s).abs() + seq_h as f64 + o) as f32);
        let block_y = (((run_h as f64 + block_base).trunc() - block_h - block_base).trunc()) as i64;
        let ascent_px = (block_y + seq_h + base) as f64;
        let width = lines.iter().map(|l| l.draw).fold(f64::NEG_INFINITY, f64::max);
        Layout {
            metrics: TextMetrics {
                width,
                ascent: ascent_px / s,
                height: run_h as f64 / s,
                fill_x: js_round(f.outline * s) / s,
                lines,
            },
            ppem: p,
            glyphs: glyph_lines,
            ascent_px,
        }
    }

    /// `text`'s metrics in `f` at the layout `scale` (what layout calls).
    pub fn measure(&mut self, f: &FontInfo, text: &str, scale: f64) -> TextMetrics {
        self.layout(f, text, scale).metrics.clone()
    }

    /// `text` shaped in `f` at the layout `scale`, its glyphs in the atlas (what paint pushes).
    pub fn shape(&mut self, f: &FontInfo, text: &str, scale: f64) -> ShapedRun {
        let (ppem, ascent, lines) = {
            let l = self.layout(f, text, scale);
            (l.ppem, l.ascent_px, l.glyphs.clone())
        };
        // FSlateFontRenderer::GetRenderData: the outline glyph is the glyph stroked at the outline size x scale.
        // FSlateFontRenderer::GetRenderData (141647c90): o = floor(OutlineSize x scale + 0.5), FT_Stroker radius
        // o x 64 (26.6), round caps and joins, the outside border only.
        let stroke = js_round(f.outline * scale).max(0.0) as u32;
        let fill_x = js_round(f.outline * scale);
        let mut run = ShapedRun::default();
        let layers: &[bool] = if stroke > 0 { &[true, false] } else { &[false] };
        for &outline in layers {
            for (li, line) in lines.iter().enumerate() {
                for &(face, glyph, pen) in line {
                    let key = GlyphKey {
                        face,
                        ppem,
                        glyph,
                        stroke: if outline { stroke } else { 0 },
                        separate: outline && f.separate_fill_alpha,
                    };
                    let x = pen as f64 + if outline { 0.0 } else { fill_x };
                    let mut g = ShapedGlyph {
                        atlas: 0,
                        x,
                        y: ascent,
                        outline,
                        line: li as u32,
                        color: None,
                        part: GlyphPart::Whole,
                    };
                    // a colour emoji's fill is its COLR layers, each a glyph in its colour, after its base glyph for a
                    // shadow; its outline strokes the base glyph
                    let layers = match self.emoji_tables() {
                        Some((ef, t)) if ef == face && !outline => u16::try_from(glyph).ok().and_then(|g| t.layers(g)),
                        _ => None,
                    };
                    if layers.is_some() {
                        g.part = GlyphPart::Silhouette;
                    }
                    g.atlas = self.glyph_entry(key);
                    run.glyphs.push(g);
                    for k in layers.into_iter().flatten() {
                        let Some((_, t)) = self.emoji_tables() else { break };
                        let (layer, color) = t.layer(k);
                        g.atlas = self.glyph_entry(GlyphKey { glyph: u32::from(layer), ..key });
                        g.color = color;
                        g.part = GlyphPart::Layer;
                        run.glyphs.push(g);
                    }
                }
            }
        }
        run
    }

    /// A glyph's atlas entry (`ShapedGlyph::atlas`).
    pub fn glyph(&self, atlas: u32) -> GlyphEntry {
        self.entries[atlas as usize]
    }

    /// The atlas the entries are in; a page's `dirty` rect is what changed since `take_dirty`.
    pub fn atlas(&self) -> &GlyphAtlas {
        &self.atlas
    }

    /// The pages' changes are uploaded: their dirty rects cleared.
    pub fn take_dirty(&mut self) {
        for p in &mut self.atlas.pages {
            p.dirty = None;
        }
    }

    fn glyph_entry(&mut self, key: GlyphKey) -> u32 {
        if let Some(&e) = self.glyphs.get(&key) {
            return e;
        }
        let bmp = self.raster(key);
        let entry = self.place(&bmp);
        self.entries.push(entry);
        let i = u32::try_from(self.entries.len() - 1).expect("text: atlas entries");
        self.glyphs.insert(key, i);
        i
    }

    /// The glyph's coverage and its offset from the pen (left, top).
    fn raster(&mut self, key: GlyphKey) -> mvs_freetype::Bitmap {
        self.set_size(key.face, key.ppem);
        let (id, flags) = (self.faces[key.face].id, self.faces[key.face].flags);
        // a glyph the face cannot load (past its count) draws its .notdef, else nothing
        let load = |lib: &mut Library, stroke: i64| match lib.bitmap(id, key.glyph, flags, stroke) {
            Ok(b) => b,
            Err(_) => lib.bitmap(id, 0, flags, stroke).unwrap_or_default(),
        };
        if key.stroke == 0 {
            return load(&mut self.lib, 0);
        }
        let r = i64::from(key.stroke);
        let mut b = load(&mut self.lib, r * 64);
        if key.separate {
            // bSeparateFillAlpha (GetRenderData 141647c90): where the fill covers, |stroke - fill|; the outside
            // border covers the fill, so the difference is stroke - fill.
            let fill = load(&mut self.lib, 0);
            let (dx, dy) = (i64::from(fill.left - b.left), i64::from(b.top - fill.top));
            for y in 0..fill.rows as i64 {
                for x in 0..fill.width as i64 {
                    let (ox, oy) = (x + dx, y + dy);
                    if ox < 0 || oy < 0 || ox >= i64::from(b.width) || oy >= i64::from(b.rows) {
                        continue;
                    }
                    let o = (oy * i64::from(b.width) + ox) as usize;
                    b.pixels[o] = b.pixels[o].saturating_sub(fill.pixels[(y * i64::from(fill.width) + x) as usize]);
                }
            }
        }
        // The outline glyph is placed from the pen of an outline sequence at line X 0 while the fill's sequence
        // sits round(OutlineSize x scale) to the right (AddShapedTextElement 14165fcd0, X only): its horizontal
        // offset is the stroked bitmap's left plus the stroke, so it surrounds the fill. Unverified: the 4.26
        // source's GetRenderData offsets, not read in the exe.
        b.left += key.stroke as i32;
        b
    }

    /// Packs a bitmap into the atlas (shelves, ATLAS_PAD zero pixels around it).
    fn place(&mut self, b: &mvs_freetype::Bitmap) -> GlyphEntry {
        let offset = [b.left as i16, b.top as i16];
        if b.width == 0 || b.rows == 0 {
            return GlyphEntry { page: 0, rect: [0; 4], offset };
        }
        let (w, h) = (b.width + 2 * ATLAS_PAD, b.rows + 2 * ATLAS_PAD);
        assert!(w <= ATLAS_SIZE && h <= ATLAS_SIZE, "text: a {} x {} glyph is over the atlas page", b.width, b.rows);
        let fits = |p: &AtlasPage| {
            let [top, height, x] = p.shelf;
            (x + w <= ATLAS_SIZE && top + h.max(height) <= ATLAS_SIZE) || top + height + h <= ATLAS_SIZE
        };
        let page = match self.atlas.pages.iter().position(fits) {
            Some(i) => i,
            None => {
                self.atlas.pages.push(AtlasPage {
                    pixels: vec![0; (ATLAS_SIZE * ATLAS_SIZE) as usize],
                    dirty: None,
                    shelf: [0; 3],
                });
                self.atlas.pages.len() - 1
            }
        };
        let p = &mut self.atlas.pages[page];
        let [mut top, mut height, mut x] = p.shelf;
        if x + w > ATLAS_SIZE || top + h.max(height) > ATLAS_SIZE {
            top += height;
            height = 0;
            x = 0;
        }
        let (gx, gy) = (x + ATLAS_PAD, top + ATLAS_PAD);
        for row in 0..b.rows {
            let at = ((gy + row) * ATLAS_SIZE + gx) as usize;
            let src = (row * b.width) as usize;
            p.pixels[at..at + b.width as usize].copy_from_slice(&b.pixels[src..src + b.width as usize]);
        }
        p.shelf = [top, height.max(h), x + w];
        let d = [gx, gy, gx + b.width, gy + b.rows];
        p.dirty = Some(match p.dirty {
            Some(o) => [o[0].min(d[0]), o[1].min(d[1]), o[2].max(d[2]), o[3].max(d[3])],
            None => d,
        });
        GlyphEntry { page: page as u16, rect: [gx as u16, gy as u16, b.width as u16, b.rows as u16], offset }
    }
}
