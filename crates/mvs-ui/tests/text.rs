//! The text module against the build's numbers (the live reads of FSlateTextRun
//! and the batcher's shaped sequence) and against the oracle's metrics (over the bundle's hmtx and
//! kern data). Needs the dev store; without one each test says so and passes.

use indexmap::IndexMap;
use mvs_ui::Bundle;
use mvs_ui::text::{FaceMetrics, FontInfo, Fonts, GameText, GlyphPart, pixel_size, wrap_width};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HudFaces {
    typefaces: IndexMap<String, String>,
    faces: IndexMap<String, FaceJson>,
    default_typeface: String,
}

#[derive(Deserialize)]
struct FaceJson {
    upm: i32,
    height: i32,
    descender: i32,
    adv: IndexMap<String, i64>,
    kern: IndexMap<String, i64>,
}

fn store() -> Option<mvs_store::Store> {
    match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) if s.version("font").is_some() && s.version("ui").is_some() => Some(s),
        _ => {
            eprintln!("no dev store with the font and ui kinds: skipped");
            None
        }
    }
}

/// The hud bundle's font part (the ui kind's JSON), and a Bundle holding it.
fn hud(store: &mvs_store::Store) -> (HudFaces, Bundle) {
    let pack = store.kind("ui").unwrap();
    let path = pack.paths().find(|p| p.ends_with("/hud.hud")).unwrap().to_string();
    let h: HudFaces = serde_json::from_slice(&pack.bytes(&path).unwrap()).unwrap();
    let b = Bundle {
        typefaces: h.typefaces.clone(),
        faces: h
            .faces
            .iter()
            .map(|(k, f)| (k.clone(), FaceMetrics { upm: f.upm, height: f.height, descender: f.descender }))
            .collect(),
        default_typeface: h.default_typeface.clone(),
        ..Default::default()
    };
    (h, b)
}

fn font(typeface: &str, size: f64, outline: f64) -> FontInfo {
    FontInfo { typeface: typeface.into(), size, outline, ..Default::default() }
}

#[test]
fn pixel_sizes() {
    // Size 60 at 0.625 is 50 px; size 20 at 0.5686 is 15 px; size 18 at 0.5 is 12 px.
    assert_eq!(pixel_size(60.0, 0.625), 50);
    assert_eq!(pixel_size(20.0, 0.5686), 15);
    assert_eq!(pixel_size(18.0, 0.5), 12);
}

#[test]
fn wrapping_width() {
    assert_eq!(wrap_width(false, 300.0, 0.0), 0.0);
    assert_eq!(wrap_width(true, 300.0, 0.0), 300.0);
    assert_eq!(wrap_width(true, 300.0, 200.0), 200.0);
    assert_eq!(wrap_width(true, 0.5, 200.0), 200.0);
}

#[test]
fn run_metrics_match_the_live_reads() {
    let Some(s) = store() else { return };
    let (_, b) = hud(&s);
    let mut fonts = Fonts::new(&s, &b).unwrap();
    // The fixed name label (usernameLabel, NormsCond ExtraBold Italic 20, outline 2) at 1600 x 900: shaped
    // sequence height 19, baseline -4, desired height 36.934 (run height 21 px).
    let scale = 0.5686;
    let m = fonts.measure(&font("NormsCond-ExtraBoldItalic", 20.0, 2.0), "Shaggy", scale);
    assert_eq!(js(m.height * scale), 21.0);
    assert!((m.height - 36.934).abs() < 2e-3, "{}", m.height);
    // Block offset 0: the glyph baseline is sequence height + baseline = 15 px under the line top.
    assert_eq!(js(m.ascent * scale), 15.0);
    // The P1 damage digit (Norms ExtraBold Italic 60, outline 5, shadow 7 / 7) at 0.625: run height 77, glyph
    // baseline 53.
    let digit = FontInfo { shadow: [7.0, 7.0], ..font("Norms-ExtraBoldItalic", 60.0, 5.0) };
    let m = fonts.measure(&digit, "0", 0.625);
    assert_eq!(js(m.height * 0.625), 77.0);
    assert_eq!(js(m.ascent * 0.625), 53.0);
    assert_eq!(m.fill_x * 0.625, 3.0);
}

fn js(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

#[test]
fn pens_match_the_capture() {
    let Some(s) = store() else { return };
    let (_, b) = hud(&s);
    let mut fonts = Fonts::new(&s, &b).unwrap();
    // f_idle.png at DPI scale 0.5 ("Shaggy (Bot)" at 12 px): h 6, g 18 / 24, y 30, o 48, t 54 px after the S.
    let f = FontInfo { letter_spacing: -25.0, ..font("NormsCond-ExtraBoldItalic", 18.0, 2.0) };
    let run = fonts.shape(&f, "Shaggy (Bot)", 0.5);
    let fill: Vec<f64> = run.glyphs.iter().filter(|g| !g.outline).map(|g| g.x - 1.0).collect();
    assert_eq!(fill.len(), 12);
    for (i, want) in [(1, 6.0), (3, 18.0), (4, 24.0), (5, 30.0), (9, 48.0), (10, 54.0)] {
        assert_eq!(fill[i], want, "glyph {i}: {fill:?}");
    }
    // The outline layer first, at the pen; the fill one round(2 x 0.5) px right.
    assert!(run.glyphs[..12].iter().all(|g| g.outline));
    assert_eq!(run.glyphs[12].x - run.glyphs[0].x, 1.0);
}

/// The oracle's advance and kerning (over the bundle's hmtx and kern data) against FreeType's
/// at the HUD's sizes: the counts of characters and pairs that differ.
#[test]
fn advances_and_kerning_match_the_oracle() {
    let Some(s) = store() else { return };
    let (h, b) = hud(&s);
    let mut fonts = Fonts::new(&s, &b).unwrap();
    let mul_fix = |a: i64, b: i64| {
        let p = a * b;
        if p < 0 { -((-p + 0x8000) >> 16) } else { (p + 0x8000) >> 16 }
    };
    let pix_round = |v: i64| (v + 32) >> 6;
    let mut checked = 0;
    let mut off = Vec::new();
    for (typeface, path) in &h.typefaces {
        let face = &h.faces[path];
        for ppem_size in [10.0, 15.0, 18.0, 20.0, 28.0, 32.0, 60.0] {
            for scale in [0.5, 0.5686, 0.625, 1.0] {
                let p = i64::from(pixel_size(ppem_size, scale));
                let fx = (p * 64 * 0x10000 + i64::from(face.upm / 2)) / i64::from(face.upm);
                let f = font(typeface, ppem_size, 0.0);
                for (ch, &u) in face.adv.iter().filter(|(c, _)| c.chars().all(|c| (' '..='~').contains(&c))) {
                    let want = pix_round(mul_fix(u, fx)) as f64;
                    // A two-glyph line: the second pen is the first's advance (+ kerning, none with a space).
                    let text = format!("{ch} ");
                    let m = fonts.shape(&f, &text, scale);
                    let got = m.glyphs[1].x - m.glyphs[0].x;
                    checked += 1;
                    if got != want {
                        off.push(format!("{typeface} {p}px {ch:?}: {got} vs {want}"));
                    }
                }
                for (pair, &k) in face.kern.iter().take(300) {
                    let cs: Vec<char> = pair.chars().collect();
                    if cs.len() != 2 || cs.contains(&' ') || !cs.iter().all(|c| (' '..='~').contains(c)) {
                        continue;
                    }
                    let mut kk = mul_fix(k, fx);
                    if p < 25 {
                        kk = kk.signum() * ((kk.abs() * p + 12) / 25);
                    }
                    let first = fonts.shape(&f, &format!("{} ", cs[0]), scale);
                    let both = fonts.shape(&f, pair, scale);
                    let want = (first.glyphs[1].x - first.glyphs[0].x) + pix_round(kk) as f64;
                    checked += 1;
                    if both.glyphs[1].x - both.glyphs[0].x != want {
                        off.push(format!("{typeface} {p}px kern {pair:?}"));
                    }
                }
            }
        }
    }
    eprintln!("{checked} advances and pairs, {} differ", off.len());
    for o in off.iter().take(20) {
        eprintln!("  {o}");
    }
    assert!(off.is_empty());
}

#[test]
fn face_metrics_are_freetype_s() {
    let Some(s) = store() else { return };
    let (h, _) = hud(&s);
    let pack = s.kind("font").unwrap();
    let mut lib = mvs_freetype::Library::new().unwrap();
    for (path, f) in &h.faces {
        let base = path.rsplit('/').next().unwrap();
        let id = lib.open(pack.bytes(&format!("{path}.{base}")).unwrap()).unwrap();
        let i = lib.info(id);
        assert_eq!(i.units_per_em, f.upm);
        let metrics = (i.height, i.descender);
        let bbox = (i.bbox_y[1] - i.bbox_y[0], i.bbox_y[0]);
        assert!((f.height, f.descender) == metrics || (f.height, f.descender) == bbox, "{path}");
    }
}

#[test]
fn wrapping_and_hard_breaks() {
    let Some(s) = store() else { return };
    let (_, b) = hud(&s);
    let mut fonts = Fonts::new(&s, &b).unwrap();
    let f = font("Body", 20.0, 0.0);
    let one = fonts.measure(&f, "RING OUT", 1.0);
    let two = fonts.measure(&f, "RING\r\nOUT", 1.0);
    assert_eq!(two.lines.len(), 2);
    let wrapped = fonts.measure(&FontInfo { wrap: one.width - 1.0, ..f.clone() }, "RING OUT", 1.0);
    assert_eq!(wrapped.lines.len(), 2);
    // The space at the break belongs to the first line's draw width, not its justification width.
    assert!(wrapped.lines[0].draw > wrapped.lines[0].width);
    assert_eq!(wrapped.lines[1].width, two.lines[1].width);
    let run = fonts.shape(&FontInfo { wrap: one.width - 1.0, ..f }, "RING OUT", 1.0);
    assert_eq!(run.glyphs.iter().filter(|g| g.line == 1).count(), 3);
    assert_eq!(run.glyphs.iter().find(|g| g.line == 1).unwrap().x, 0.0);
}

#[test]
fn glyphs_fill_the_atlas() {
    let Some(s) = store() else { return };
    let (_, b) = hud(&s);
    let mut fonts = Fonts::new(&s, &b).unwrap();
    let f = FontInfo { separate_fill_alpha: true, ..font("Norms-ExtraBoldItalic", 60.0, 5.0) };
    let run = fonts.shape(&f, "0 0", 0.625);
    // The same glyph shares its entry; the space has an empty one.
    assert_eq!(run.glyphs[0].atlas, run.glyphs[2].atlas);
    assert_eq!(fonts.glyph(run.glyphs[1].atlas).rect[2], 0);
    let fill = fonts.glyph(run.glyphs[3].atlas);
    let outline = fonts.glyph(run.glyphs[0].atlas);
    assert!(fill.rect[2] > 0 && outline.rect[2] >= fill.rect[2] + 2 * 3 - 1);
    // The outline surrounds the fill: its left at the pen + the stroke is the fill's left less the stroke.
    assert!((i32::from(outline.offset[0]) - i32::from(fill.offset[0])).abs() <= 1);
    assert!((i32::from(outline.offset[1]) - i32::from(fill.offset[1]) - 3).abs() <= 1);
    let a = fonts.atlas();
    assert_eq!(a.pages.len(), 1);
    assert!(a.pages[0].dirty.is_some());
    // The cut-out outline (bSeparateFillAlpha) is clear wherever the fill is fully inked: fill pixel (x, y) is
    // outline pixel (x + dx, y + dy), the fill's quad being round(5 x 0.625) = 3 px right of the outline's pen.
    let p = &a.pages[0].pixels;
    let at = |e: mvs_ui::text::GlyphEntry, x: i32, y: i32| {
        p[(i32::from(e.rect[1]) + y) as usize * 1024 + (i32::from(e.rect[0]) + x) as usize]
    };
    let dx = 3 + i32::from(fill.offset[0]) - i32::from(outline.offset[0]);
    let dy = i32::from(outline.offset[1]) - i32::from(fill.offset[1]);
    let mut inked = 0;
    for y in 0..i32::from(fill.rect[3]) {
        for x in 0..i32::from(fill.rect[2]) {
            if at(fill, x, y) == 255 {
                inked += 1;
                assert_eq!(at(outline, x + dx, y + dy), 0, "fill pixel {x}, {y}");
            }
        }
    }
    assert!(inked > 0);
    fonts.take_dirty();
    assert!(fonts.atlas().pages[0].dirty.is_none());
}

#[test]
fn game_text() {
    let Some(s) = store() else { return };
    let t = GameText::load(&s).unwrap();
    assert!(t.get("", "no such key").is_none());
    assert!(t.get("no such namespace", "x").is_none());
}

#[test]
fn lines_are_justified_in_the_allotted_width() {
    use mvs_ui::text::{Justify, TextLine, TextMetrics};
    let m = TextMetrics {
        height: 20.0,
        lines: vec![TextLine { width: 140.0, draw: 140.0 }, TextLine { width: 60.0, draw: 60.0 }],
        ..Default::default()
    };
    let f = FontInfo { outline: 2.0, shadow: [3.0, 0.0], ..Default::default() };
    // An over-wide centred line overflows both ways: (120 - 140 - 7) x 0.5.
    assert_eq!(m.line_origin(0, &f, 120.0, Justify::Center, 1.0), [-13.5, 0.0]);
    assert_eq!(m.line_origin(1, &f, 120.0, Justify::Right, 0.9), [53.0, 18.0]);
    assert_eq!(m.line_origin(1, &f, 120.0, Justify::Left, 1.0), [0.0, 20.0]);
}

/// The emoji fallback: a character the game's faces lack is drawn from the system's colour emoji face (Windows'
/// Segoe UI Emoji), its sequences joined into one emoji and its layers coloured. Without that face: skipped.
#[test]
fn emoji_fall_back_to_the_system_face() {
    let Some(s) = store() else { return };
    let have = std::env::var_os("WINDIR")
        .is_some_and(|w| std::path::Path::new(&w).join("Fonts").join("seguiemj.ttf").is_file());
    if !have {
        eprintln!("no system emoji face: skipped");
        return;
    }
    let (_, b) = hud(&s);
    let mut fonts = Fonts::new(&s, &b).unwrap();
    let f = font("Body", 20.0, 0.0);
    // each emoji is one place on the line, drawn as its coloured layers
    let one = |fonts: &mut Fonts, text: &str| {
        let run = fonts.shape(&f, text, 1.0);
        let mut pens: Vec<f64> = run.glyphs.iter().filter(|g| g.color.is_some()).map(|g| g.x).collect();
        pens.dedup();
        (pens, run.glyphs.len())
    };
    for text in ["\u{1F600}", "\u{2764}\u{FE0F}", "\u{1F44D}\u{1F3FD}"] {
        let (pens, n) = one(&mut fonts, text);
        assert_eq!(pens.len(), 1, "{text:?}: one emoji");
        assert!(n > 1, "{text:?}: coloured layers");
    }
    // one silhouette per emoji for a shadow: the base glyph, with a shape of its own
    let run = fonts.shape(&f, "😀", 1.0);
    let base: Vec<_> = run.glyphs.iter().filter(|g| g.part == GlyphPart::Silhouette).collect();
    assert_eq!(base.len(), 1);
    let rect = fonts.glyph(base[0].atlas).rect;
    assert!(rect[2] > 0 && rect[3] > 0, "silhouette {rect:?}");
    assert!(run.glyphs.iter().filter(|g| g.part == GlyphPart::Layer).count() > 1);
    // a ZWJ sequence is joined into parts that overlap at their advances: no wider than one emoji
    let single = fonts.measure(&f, "\u{1F600}", 1.0).width;
    let family = fonts.measure(&f, "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}", 1.0).width;
    assert!(family > 0.0 && family <= single, "family {family} against {single}");
    // VS16 after an emoji takes no room of its own
    assert_eq!(fonts.measure(&f, "\u{2764}\u{FE0F}", 1.0).width, fonts.measure(&f, "\u{2764}", 1.0).width);
    // a joiner outside an emoji draws nothing
    assert_eq!(fonts.measure(&f, "a\u{FE0F}", 1.0).width, fonts.measure(&f, "a", 1.0).width);
    // the game's own text around an emoji keeps its face and colour; the emoji takes room on the line
    let plain = fonts.measure(&f, "ab", 1.0).width;
    let with = fonts.measure(&f, "a\u{1F600}b", 1.0).width;
    assert!(with > plain + 10.0);
    let run = fonts.shape(&f, "a\u{1F600}b", 1.0);
    assert!(run.glyphs.first().unwrap().color.is_none() && run.glyphs.last().unwrap().color.is_none());
}
