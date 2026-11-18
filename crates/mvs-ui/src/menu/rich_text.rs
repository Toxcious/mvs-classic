//! RichTextBlock: URichTextBlock with the default markup parser. A text whose
//! `<tag>...</>` spans draw with the row of that name of the block's TextStyleSet table and the rest with the row
//! "Default" (the bundle's `rich_styles`). The tables of the ported screens are UI_CharacterMoveDescription (the Move
//! List's descriptions: Default, Callout, ArmorBreak, Dimmed) and the ranked help's.
//!
//! The block's DecoratorClasses: a RichTextBlockImageDecorator child (the bundle's `rich_images`: its ImageSet's
//! rows) takes `<img id="row"/>` (FRichInlineImage::Supports: the tag img with an id its table has; the first class
//! of the list that has it). Its run is one widget (FRichTextDecorator::Create: an atomic character in the text): an
//! SBox of IconHeight x IconHeight, IconHeight = Min((uint32) the Default style's max character height at scale 1,
//! the brush's ImageSize.Y), holding the brush's SImage scaled to fit (SRichInlineImage::Construct; the port draws
//! the Image at that size: the same picture for a square brush); its baseline is the Default style's (GetBaseline
//! at scale 1 - Min(0, ShadowOffset.Y)), so its bottom sits that far under the line's baseline. The width, height
//! and stretch attributes are not read (no text of the ported screens has them).
//!
//! The block is laid out with the widgets the tree has: the block (measured and arranged as an Overlay) holds a
//! VerticalBox of lines, a line a HorizontalBox with one TextBlock for each run of the line. That is the build's own
//! shape of it: FTextLayout makes one FSlateTextRun for each style span and one block of a run on each line it
//! reaches, shaped on its own (no kerning across a block's ends).
//! - Wrapping (FTextLayout::FlowLineLayout 14179ca90): a line breaks before the first word whose trimmed end passes
//!   the wrapping width; the spaces at a break stay on the line they end. A word may run through several runs.
//!   AutoWrapText wraps at the width of the block's last geometry (Unverified, as for a TextBlock), so a text's
//!   first frame is one line.
//! - A line (FTextLayout::CreateLineViewBlocks 1417988c0): MaxAbove = the largest of its blocks' (run max height +
//!   baseline), MaxBelow the largest -baseline, its height (MaxAbove + MaxBelow) x LineHeightPercentage; a block sits
//!   MaxAbove - block height - baseline under the line's top. With no outline and no shadow (every row of the table)
//!   a run's block height is its max height, so the offset is MaxAbove - the run's own.
//!
//! Unverified: the tag syntax, the fall back to the Default row for a tag without a row and the image run are from the
//! 4.26 source (FDefaultRichTextMarkupParser, URichTextBlock, URichTextBlockImageDecorator), not read from the exe; a
//! self-closing tag no decorator takes stays as its text. The escapes (&lt; &gt; &quot; &amp;), a Justification
//! other than Left, MinDesiredWidth and a style with an outline or a shadow are not handled (none is in the ported
//! screens' texts and tables).
//!
//! A reflow destroys the block's old lines (`Tree::destroy`: their ids are taken by the new ones), and a block that
//! leaves the view's layers has its lines destroyed at the next tick, so the tree does not grow with the reflows.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use super::view::{Layer, ViewRich};
use crate::bundle::Bundle;
use crate::slate::{HAlign, Margin, Tree, VAlign, Vis, WidgetId};
use crate::text::{FontInfo, Fonts};

/// One row of a TextStyleSet table.
struct Style {
    typeface: String,
    size: f64,
    letter_spacing: f64,
    outline: f64,
    color: [f64; 4],
    shadow: [f64; 2],
}

impl Style {
    fn of(v: &Value) -> Style {
        let n = |k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let c = |k: &str| v.get("color").and_then(|c| c.get(k)).and_then(Value::as_f64).unwrap_or(0.0);
        let s = |i: usize| v.get("shadow").and_then(|s| s.get(i)).and_then(Value::as_f64).unwrap_or(0.0);
        Style {
            typeface: v.get("typeface").and_then(Value::as_str).unwrap_or("").to_string(),
            size: n("size"),
            letter_spacing: n("letterSpacing"),
            outline: n("outline"),
            color: [c("R"), c("G"), c("B"), c("A")],
            shadow: [s(0), s(1)],
        }
    }

    fn font(&self) -> FontInfo {
        FontInfo {
            typeface: self.typeface.clone(),
            size: self.size,
            letter_spacing: self.letter_spacing,
            outline: self.outline,
            shadow: self.shadow,
            wrap: 0.0,
            separate_fill_alpha: false,
        }
    }
}

/// A word piece or the spaces between words, with the style of its run; or a decorator's image.
#[derive(Clone)]
struct Atom {
    style: String,
    text: String,
    space: bool,
    image: Option<Arc<RichImage>>,
}

/// A row of an image decorator's ImageSet (RichImageRow): the brush's texture, ImageSize and TintColor.
#[derive(Debug)]
struct RichImage {
    texture: String,
    size: [f64; 2],
    tint: [f64; 4],
}

impl RichImage {
    fn of(v: &Value) -> RichImage {
        let s = |i: usize| v.get("size").and_then(|s| s.get(i)).and_then(Value::as_f64).unwrap_or(0.0);
        let c = |k: &str| v.get("tint").and_then(|c| c.get(k)).and_then(Value::as_f64).unwrap_or(1.0);
        RichImage {
            texture: v.get("texture").and_then(Value::as_str).unwrap_or("").to_string(),
            size: [s(0), s(1)],
            tint: [c("R"), c("G"), c("B"), c("A")],
        }
    }
}

/// An image decorator's rows by name.
type ImageSet = HashMap<String, Arc<RichImage>>;

/// A run of a text: (style row, text), or a decorator's image.
enum Run {
    Text(String, String),
    Image(Arc<RichImage>),
}

#[derive(Default)]
struct State {
    text: Option<String>,
    wrap: f64,
    scale: f64,
    lines: Option<WidgetId>,
}

/// A TextStyleSet table: its rows by name, and their names sorted (the tag lookup's order).
struct Table {
    rows: HashMap<String, Style>,
    names: Vec<String>,
}

/// A run's block: its width, its height above the baseline and in all.
struct Piece {
    width: f64,
    above: f64,
    height: f64,
}

/// A tag's or an attribute's name character (`[\w\d\.-]`).
fn name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'
}

/// A tag at `text[open..]` (FDefaultRichTextMarkupParser's pattern: `<name( attr="value")*`, then `/>` or
/// `>content</>`): its name, its attributes, its content (None: self-closing) and the end of the whole tag.
type Tag<'a> = (&'a str, Vec<(&'a str, &'a str)>, Option<&'a str>, usize);

fn tag_at(text: &str, open: usize) -> Option<Tag<'_>> {
    let name_end = text[open + 1..].find(|c: char| !name_char(c)).map(|n| open + 1 + n)?;
    if name_end == open + 1 {
        return None;
    }
    let mut attrs = Vec::new();
    let mut at = name_end;
    while text[at..].starts_with(' ') {
        let k0 = at + 1;
        let k1 = text[k0..].find(|c: char| !name_char(c)).map(|n| k0 + n)?;
        if k1 == k0 || !text[k1..].starts_with("=\"") {
            return None;
        }
        let v0 = k1 + 2;
        let v1 = text[v0..].find('"').map(|n| v0 + n)?;
        attrs.push((&text[k0..k1], &text[v0..v1]));
        at = v1 + 1;
    }
    if text[at..].starts_with("/>") {
        return Some((&text[open + 1..name_end], attrs, None, at + 2));
    }
    if !text[at..].starts_with('>') {
        return None;
    }
    let close = text[at + 1..].find("</>").map(|n| at + 1 + n)?;
    Some((&text[open + 1..name_end], attrs, Some(&text[at + 1..close]), close + 3))
}

/// The tagged runs of a text: the text outside a tag is Default, `<img id="row"/>` the image of the first of `images`
/// that has the row, a tag's content takes the row of the tag's name (no case, as an FName), else Default. A
/// self-closing tag no decorator takes stays as text.
fn runs(text: &str, rows: &[String], images: &[Arc<ImageSet>]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    let mut at = 0;
    let mut i = 0;
    while let Some(off) = text[i..].find('<') {
        let open = i + off;
        let Some((name, attrs, content, end)) = tag_at(text, open) else {
            i = open + 1;
            continue;
        };
        let id = attrs.iter().find(|(k, _)| *k == "id").map(|(_, v)| *v).filter(|_| name == "img");
        let image = id.and_then(|id| images.iter().find_map(|set| set.get(id)));
        if image.is_none() && content.is_none() {
            i = open + 1;
            continue;
        }
        if open > at {
            out.push(Run::Text("Default".to_string(), text[at..open].to_string()));
        }
        match (image, content) {
            (Some(img), _) => out.push(Run::Image(img.clone())),
            (None, Some(content)) if !content.is_empty() => {
                let tag = name.to_lowercase();
                let row = rows.iter().find(|k| k.to_lowercase() == tag).map_or("Default", |k| k.as_str());
                out.push(Run::Text(row.to_string(), content.to_string()));
            }
            _ => {}
        }
        at = end;
        i = at;
    }
    if at < text.len() {
        out.push(Run::Text("Default".to_string(), text[at..].to_string()));
    }
    out
}

/// Split into words and runs of spaces.
fn split_spaces(s: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    for c in s.chars() {
        let sp = c == ' ';
        match out.last_mut() {
            Some((t, k)) if *k == sp => t.push(c),
            _ => out.push((c.to_string(), sp)),
        }
    }
    out
}

/// The RichTextBlocks of a view's layers.
pub struct RichTexts {
    bundle: Arc<Bundle>,
    entries: Vec<WidgetId>,
    states: HashMap<WidgetId, State>,
    /// The tables read so far by their short name (None: the bundle has none, or it has no Default row).
    tables: HashMap<String, Option<Arc<Table>>>,
    /// The image decorators' rows read so far by their class's short name.
    images: HashMap<String, Option<Arc<ImageSet>>>,
    /// The lines of the blocks that left the layers, destroyed at the next tick.
    stale: Vec<WidgetId>,
}

impl std::fmt::Debug for RichTexts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RichTexts").field("entries", &self.entries.len()).finish()
    }
}

impl RichTexts {
    pub fn new(bundle: Arc<Bundle>) -> RichTexts {
        RichTexts {
            bundle,
            entries: Vec::new(),
            states: HashMap::new(),
            tables: HashMap::new(),
            images: HashMap::new(),
            stale: Vec::new(),
        }
    }

    /// The block's lines again for `text` wrapped at `wrap` (Infinity: no wrapping), its styles from `table` and its
    /// images from `images` (the block's decorators, in order).
    fn flow(
        b: &Bundle,
        tree: &mut Tree,
        fonts: &mut Fonts,
        (w, state): (WidgetId, &mut State),
        (table, images): (&Table, &[Arc<ImageSet>]),
        wrap: f64,
    ) {
        let scale = tree.text_scale(w);
        let (text, line_height) = {
            let x = tree.widget(w);
            (x.text.clone(), x.line_height)
        };
        if let Some(old) = state.lines.take()
            && tree.alive(old)
        {
            tree.destroy(old);
        }
        let (names, table) = (&table.names, &table.rows);
        let lines = tree.create_plain(b, "VerticalBox", "lines");
        {
            let x = tree.widget_mut(lines);
            x.vis = Vis::HitTestInvisible;
            x.h_align = HAlign::Fill;
            x.v_align = VAlign::Top;
        }
        state.lines = Some(lines);
        let atoms_of = |runs: Vec<Run>| -> Vec<Vec<Atom>> {
            let mut hard: Vec<Vec<Atom>> = vec![Vec::new()];
            for run in runs {
                let (style, text) = match run {
                    Run::Text(style, text) => (style, text),
                    Run::Image(img) => {
                        let a =
                            Atom { style: "Default".to_string(), text: String::new(), space: false, image: Some(img) };
                        hard.last_mut().expect("one line").push(a);
                        continue;
                    }
                };
                for (i, part) in text.split('\n').enumerate() {
                    if i > 0 {
                        hard.push(Vec::new());
                    }
                    let part = part.strip_suffix('\r').unwrap_or(part);
                    for (t, space) in split_spaces(part) {
                        let a = Atom { style: style.clone(), text: t, space, image: None };
                        hard.last_mut().expect("one line").push(a);
                    }
                }
            }
            hard
        };
        let hard = atoms_of(runs(&text, names, images));
        let piece = |fonts: &mut Fonts, seg: &Atom| -> Piece {
            if let Some(img) = &seg.image {
                // SRichInlineImage: IconHeight = Min((uint32) GetMaxCharacterHeight(Default font, 1), (uint32)
                // ImageSize.Y), square; the run's baseline (int16) GetBaseline(Default font, 1) - Min(0, ShadowOffset.Y)
                // Unverified: the text measure's height and ascent at scale 1 stand for the font measure's max
                // character height and baseline.
                let d = &table["Default"];
                let m = fonts.measure(&d.font(), "", 1.0);
                let icon = m.height.trunc().min(img.size[1].trunc());
                let baseline = (m.ascent - m.height).trunc() - d.shadow[1].min(0.0);
                return Piece { width: icon, above: icon + baseline, height: icon };
            }
            let s = &table[&seg.style];
            let m = fonts.measure(&s.font(), &seg.text, scale);
            Piece { width: m.width + 2.0 * s.outline + s.shadow[0].abs(), above: m.ascent, height: m.height }
        };
        // one block for each run of a line (an image is a run of its own)
        let blocks = |atoms: &[&Atom]| -> Vec<Atom> {
            let mut segs: Vec<Atom> = Vec::new();
            for &a in atoms {
                match segs.last_mut() {
                    Some(seg) if seg.image.is_none() && a.image.is_none() && seg.style == a.style => {
                        seg.text.push_str(&a.text);
                    }
                    _ => segs.push(a.clone()),
                }
            }
            segs
        };
        let trimmed = |fonts: &mut Fonts, atoms: &[&Atom]| -> f64 {
            let mut n = atoms.len();
            while n > 0 && atoms[n - 1].space {
                n -= 1;
            }
            blocks(&atoms[..n]).iter().map(|seg| piece(fonts, seg).width).sum()
        };
        let emit = |tree: &mut Tree, fonts: &mut Fonts, atoms: &[&Atom]| {
            let mut segs = blocks(atoms);
            if segs.is_empty() {
                segs.push(Atom { style: "Default".to_string(), text: String::new(), space: false, image: None });
            }
            let ms: Vec<Piece> = segs
                .iter()
                .map(|seg| piece(fonts, &Atom { text: String::new(), space: false, ..seg.clone() }))
                .collect();
            let above = ms.iter().map(|m| m.above).fold(f64::MIN, f64::max);
            let below = ms.iter().map(|m| m.height - m.above).fold(f64::MIN, f64::max);
            // the row's own height is its tallest block under its offset; the slot's bottom padding makes up the
            // line's
            let natural = ms.iter().map(|m| above - m.above + m.height).fold(f64::MIN, f64::max);
            let row = tree.create_plain(b, "HorizontalBox", "line");
            {
                let x = tree.widget_mut(row);
                x.vis = Vis::HitTestInvisible;
                x.h_align = HAlign::Left;
                x.pad = Margin { b: (above + below) * line_height - natural, ..Margin::default() };
            }
            for (seg, m) in segs.iter().zip(&ms) {
                let run = if let Some(img) = &seg.image {
                    let run = tree.create_plain(b, "Image", "image");
                    let x = tree.widget_mut(run);
                    x.tex = Some(img.texture.clone());
                    x.brush_w = m.width;
                    x.brush_h = m.height;
                    x.brush_tint = img.tint;
                    run
                } else {
                    let s = &table[&seg.style];
                    let run = tree.create_plain(b, "TextBlock", "run");
                    let x = tree.widget_mut(run);
                    x.text = seg.text.clone();
                    x.typeface = Some(s.typeface.clone());
                    x.font_size = s.size;
                    x.letter_spacing = s.letter_spacing;
                    x.outline = s.outline;
                    x.color = s.color;
                    x.shadow_offset = s.shadow;
                    run
                };
                {
                    let x = tree.widget_mut(run);
                    x.vis = Vis::HitTestInvisible;
                    x.pad = Margin { t: above - m.above, ..Margin::default() };
                    x.v_align = VAlign::Top;
                    x.h_align = HAlign::Left;
                }
                tree.add(row, run);
            }
            tree.add(lines, row);
        };
        for atoms in &hard {
            let mut line: Vec<&Atom> = Vec::new();
            let mut flushed: Vec<Vec<&Atom>> = Vec::new();
            let mut i = 0;
            while i < atoms.len() {
                // a word (its pieces may be of several runs) and the spaces after it
                let mut j = i;
                while j < atoms.len() && !atoms[j].space {
                    j += 1;
                }
                let end = j;
                while j < atoms.len() && atoms[j].space {
                    j += 1;
                }
                if !line.is_empty() && end > i && wrap.is_finite() {
                    let mut probe = line.clone();
                    probe.extend(&atoms[i..end]);
                    if trimmed(fonts, &probe) > wrap {
                        flushed.push(std::mem::take(&mut line));
                    }
                }
                line.extend(&atoms[i..j]);
                i = j;
            }
            flushed.push(line);
            for l in flushed {
                emit(tree, fonts, &l);
            }
        }
        tree.add(w, lines);
    }

    /// The image decorator class `name` (a short name)'s rows, read once (None: the bundle has no such class).
    fn images(&mut self, name: &str) -> Option<Arc<ImageSet>> {
        if let Some(t) = self.images.get(name) {
            return t.clone();
        }
        let t =
            self.bundle.menu.as_ref().and_then(|m| m.rich_images.get(name)?.as_object()).map(|t| {
                Arc::new(t.iter().map(|(k, v)| (k.clone(), Arc::new(RichImage::of(v)))).collect::<ImageSet>())
            });
        self.images.insert(name.to_string(), t.clone());
        t
    }

    /// The table named `name` (a short name), read once.
    fn table(&mut self, name: &str) -> Option<Arc<Table>> {
        if let Some(t) = self.tables.get(name) {
            return t.clone();
        }
        let read = || -> Option<Arc<Table>> {
            let t = self.bundle.menu.as_ref()?.rich_styles.get(name)?.as_object()?;
            let rows: HashMap<String, Style> = t.iter().map(|(k, v)| (k.clone(), Style::of(v))).collect();
            if !rows.contains_key("Default") {
                return None;
            }
            let mut names: Vec<String> = rows.keys().cloned().collect();
            names.sort();
            Some(Arc::new(Table { rows, names }))
        };
        let t = read();
        self.tables.insert(name.to_string(), t.clone());
        t
    }
}

impl ViewRich for RichTexts {
    /// The RichTextBlocks of the layers' trees (when a tree or the set of layers changed).
    fn collect(&mut self, tree: &Tree, layers: &[Layer]) {
        fn walk(t: &Tree, w: WidgetId, out: &mut Vec<WidgetId>) {
            if t.widget(w).kind == "RichTextBlock" {
                out.push(w);
                return;
            }
            for &c in &t.widget(w).children {
                walk(t, c, out);
            }
        }
        self.entries.clear();
        for l in layers {
            walk(tree, l.w, &mut self.entries);
        }
        // (a block gone from the layers: its lines go at the next tick, and it flows again if it comes back)
        let entries = &self.entries;
        let stale = &mut self.stale;
        self.states.retain(|w, st| {
            let keep = entries.contains(w);
            if !keep && let Some(l) = st.lines {
                stale.push(l);
            }
            keep
        });
    }

    /// After the user widgets' ticks: a block whose text, wrapping width or scale changed gets its lines again.
    fn tick(&mut self, tree: &mut Tree, fonts: &mut Fonts) {
        for l in self.stale.drain(..) {
            // (a block's lines are its children: a block freed has taken them along)
            if tree.alive(l) {
                tree.destroy(l);
            }
        }
        for i in 0..self.entries.len() {
            let w = self.entries[i];
            let x = tree.widget(w);
            let Some(set) = x.style_set.as_deref() else { continue };
            let short = set.rsplit(['.', '/']).next().unwrap_or("");
            let (auto_wrap, wrap_at, decorators) = (x.auto_wrap, x.wrap_at, x.decorators.clone());
            let Some(table) = self.table(short) else { continue };
            let images: Vec<Arc<ImageSet>> = decorators.iter().filter_map(|d| self.images(d)).collect();
            let width = tree.arranged_size(w)[0];
            let auto = if auto_wrap && width >= 1.0 { width } else { 0.0 };
            let wrap = if auto > 0.0 && wrap_at > 0.0 {
                auto.min(wrap_at)
            } else if auto != 0.0 {
                auto
            } else {
                wrap_at
            };
            let scale = tree.text_scale(w);
            let st = self.states.entry(w).or_insert_with(|| State { wrap: -1.0, scale: -1.0, ..State::default() });
            let text = &tree.widget(w).text;
            if st.text.as_deref() != Some(text.as_str()) || st.wrap != wrap || st.scale != scale {
                st.text = Some(text.clone());
                st.wrap = wrap;
                st.scale = scale;
                let wrap = if wrap > 0.0 { wrap } else { f64::INFINITY };
                Self::flow(&self.bundle, tree, fonts, (w, st), (&table, &images), wrap);
            }
        }
    }
}
