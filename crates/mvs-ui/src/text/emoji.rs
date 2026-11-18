//! (the port's own: the beta has no chat and its fonts no emoji) The system's colour emoji face, read where the text
//! falls back for a character the game's faces lack: Windows' Segoe UI Emoji from the fonts folder at runtime (nothing
//! bundled). Its GSUB `ccmp` / `liga` lookups join the characters one emoji is made of (ZWJ sequences, skin tones,
//! flags, keycaps) and its COLR v0 layers give each glyph's coloured parts, in the CPAL palette's colours.

use std::path::PathBuf;

/// The face's file: `%WINDIR%\Fonts\seguiemj.ttf` (Windows 8.1 on; COLR v0 layers since Windows 10).
pub(super) fn path() -> Option<PathBuf> {
    let dir = std::env::var_os("WINDIR")?;
    let p = PathBuf::from(dir).join("Fonts").join("seguiemj.ttf");
    p.is_file().then_some(p)
}

/// A character that joins or modifies the emoji before it (it goes with that emoji's run): ZERO WIDTH JOINER, the
/// variation selectors, COMBINING ENCLOSING KEYCAP, the skin tone modifiers and the tag characters of a subdivision
/// flag.
pub(super) fn joins(c: char) -> bool {
    matches!(c, '\u{200D}' | '\u{FE00}'..='\u{FE0F}' | '\u{20E3}' | '\u{1F3FB}'..='\u{1F3FF}' | '\u{E0020}'..='\u{E007F}')
}

/// A joiner that draws nothing of its own: ZERO WIDTH JOINER, a variation selector, a tag character.
pub(super) fn invisible(c: char) -> bool {
    matches!(c, '\u{200D}' | '\u{FE00}'..='\u{FE0F}' | '\u{E0020}'..='\u{E007F}')
}

/// VARIATION SELECTOR-16: the character before it asks for its emoji form.
pub(super) const EMOJI_FORM: char = '\u{FE0F}';

/// The face's tables the shaper and the painter read.
#[derive(Debug, Default)]
pub(super) struct Tables {
    gsub: Gsub,
    /// COLR v0 base glyph records: glyph, first layer, layer count (sorted by glyph).
    bases: Vec<[u16; 3]>,
    /// COLR v0 layer records: glyph, palette entry (0xFFFF: the text's colour).
    layers: Vec<[u16; 2]>,
    /// CPAL palette 0, RGBA.
    palette: Vec<[u8; 4]>,
}

impl Tables {
    /// A face file's GSUB, COLR and CPAL; a table missing or out of bounds is left empty (no joins, no colour).
    pub(super) fn parse(bytes: &[u8]) -> Tables {
        let r = Reader(bytes);
        let mut t = Tables::default();
        let Some(dir) = r.tables() else { return t };
        let find = |tag: &[u8; 4]| dir.iter().find(|(t, _)| t == tag).map(|&(_, o)| o);
        if let Some(o) = find(b"GSUB") {
            t.gsub = Gsub::parse(&r, o).unwrap_or_default();
        }
        if let Some(o) = find(b"COLR") {
            (t.bases, t.layers) = colr(&r, o).unwrap_or_default();
        }
        if let Some(o) = find(b"CPAL") {
            t.palette = cpal(&r, o).unwrap_or_default();
        }
        t
    }

    /// The glyphs of a run of the face (each with the index of the character it came from), its sequences joined.
    pub(super) fn shape(&self, glyphs: &mut Vec<(u16, usize)>) {
        self.gsub.apply(glyphs);
    }

    /// A glyph's colour layers in drawing order, as indices for `layer`; None for a glyph without layers (drawn as
    /// one glyph in the text's colour).
    pub(super) fn layers(&self, glyph: u16) -> Option<std::ops::Range<usize>> {
        let i = self.bases.binary_search_by_key(&glyph, |b| b[0]).ok()?;
        let [_, first, count] = self.bases[i];
        let (first, count) = (usize::from(first), usize::from(count));
        self.layers.get(first..first + count)?;
        Some(first..first + count)
    }

    /// A layer of `layers`: its glyph and its colour (None: the text's).
    pub(super) fn layer(&self, k: usize) -> (u16, Option<[u8; 4]>) {
        let [g, p] = self.layers[k];
        (g, if p == 0xFFFF { None } else { self.palette.get(usize::from(p)).copied() })
    }
}

/// Big-endian reads that fail past the end.
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn u16(&self, o: usize) -> Option<u16> {
        Some(u16::from_be_bytes(self.0.get(o..o + 2)?.try_into().ok()?))
    }

    fn u32(&self, o: usize) -> Option<usize> {
        usize::try_from(u32::from_be_bytes(self.0.get(o..o + 4)?.try_into().ok()?)).ok()
    }

    /// The sfnt table directory: tag and offset.
    fn tables(&self) -> Option<Vec<([u8; 4], usize)>> {
        let n = usize::from(self.u16(4)?);
        (0..n)
            .map(|i| {
                let o = 12 + i * 16;
                Some((self.0.get(o..o + 4)?.try_into().ok()?, self.u32(o + 8)?))
            })
            .collect()
    }

    /// A u16 array of `n` at `o`.
    fn u16s(&self, o: usize, n: usize) -> Option<Vec<u16>> {
        (0..n).map(|i| self.u16(o + 2 * i)).collect()
    }
}

/// COLR v0 records: the base glyph records and the layer records.
type Colr = (Vec<[u16; 3]>, Vec<[u16; 2]>);

/// COLR's version 0 records (a version 1 table starts with the same fields).
fn colr(r: &Reader<'_>, o: usize) -> Option<Colr> {
    let nb = usize::from(r.u16(o + 2)?);
    let bo = o + r.u32(o + 4)?;
    let lo = o + r.u32(o + 8)?;
    let nl = usize::from(r.u16(o + 12)?);
    let bases = (0..nb)
        .map(|i| Some([r.u16(bo + i * 6)?, r.u16(bo + i * 6 + 2)?, r.u16(bo + i * 6 + 4)?]))
        .collect::<Option<Vec<_>>>()?;
    let layers = (0..nl).map(|i| Some([r.u16(lo + i * 4)?, r.u16(lo + i * 4 + 2)?])).collect::<Option<Vec<_>>>()?;
    Some((bases, layers))
}

/// CPAL palette 0 as RGBA (the table's records are BGRA).
fn cpal(r: &Reader<'_>, o: usize) -> Option<Vec<[u8; 4]>> {
    let entries = usize::from(r.u16(o + 2)?);
    let records = o + r.u32(o + 8)?;
    let first = usize::from(r.u16(o + 12)?);
    (0..entries)
        .map(|i| {
            let c = records + (first + i) * 4;
            let b = r.0.get(c..c + 4)?;
            Some([b[2], b[1], b[0], b[3]])
        })
        .collect()
}

/// A coverage table: ranges of glyphs (first, last, coverage index of the first).
#[derive(Debug, Default)]
struct Coverage(Vec<[u16; 3]>);

impl Coverage {
    fn parse(r: &Reader<'_>, o: usize) -> Option<Coverage> {
        let n = usize::from(r.u16(o + 2)?);
        match r.u16(o)? {
            1 => Some(Coverage(r.u16s(o + 4, n)?.into_iter().enumerate().map(|(i, g)| [g, g, i as u16]).collect())),
            2 => (0..n)
                .map(|i| Some([r.u16(o + 4 + i * 6)?, r.u16(o + 6 + i * 6)?, r.u16(o + 8 + i * 6)?]))
                .collect::<Option<Vec<_>>>()
                .map(Coverage),
            _ => None,
        }
    }

    fn index(&self, g: u16) -> Option<usize> {
        let i = self.0.partition_point(|r| r[1] < g);
        let [first, last, at] = *self.0.get(i)?;
        (first <= g && g <= last).then(|| usize::from(at) + usize::from(g - first))
    }
}

/// The subtables the emoji face's lookups use: single substitution, ligature substitution and the chained context
/// by coverage (format 3); any other kind is skipped.
#[derive(Debug)]
enum Sub {
    Single { cov: Coverage, delta: Option<u16>, to: Vec<u16> },
    Ligature { cov: Coverage, sets: Vec<Vec<(Vec<u16>, u16)>> },
    Chain { back: Vec<Coverage>, input: Vec<Coverage>, ahead: Vec<Coverage>, records: Vec<(u16, u16)> },
}

#[derive(Debug, Default)]
struct Gsub {
    lookups: Vec<Vec<Sub>>,
    /// The lookups of the features applied (ccmp, rlig, liga, clig), in lookup order.
    applied: Vec<u16>,
}

/// How deep a chained context's nested lookups may call.
const NEST_MAX: u32 = 8;

impl Gsub {
    fn parse(r: &Reader<'_>, g: usize) -> Option<Gsub> {
        let fl = g + usize::from(r.u16(g + 6)?);
        let ll = g + usize::from(r.u16(g + 8)?);
        let mut applied = Vec::new();
        for i in 0..usize::from(r.u16(fl)?) {
            let rec = fl + 2 + i * 6;
            let tag = r.0.get(rec..rec + 4)?;
            if [b"ccmp", b"rlig", b"liga", b"clig"].iter().any(|t| t.as_slice() == tag) {
                let fo = fl + usize::from(r.u16(rec + 4)?);
                applied.extend(r.u16s(fo + 4, usize::from(r.u16(fo + 2)?))?);
            }
        }
        applied.sort_unstable();
        applied.dedup();
        let mut lookups = Vec::new();
        for i in 0..usize::from(r.u16(ll)?) {
            let lo = ll + usize::from(r.u16(ll + 2 + i * 2)?);
            let ty = r.u16(lo)?;
            let mut subs = Vec::new();
            for s in 0..usize::from(r.u16(lo + 4)?) {
                let so = lo + usize::from(r.u16(lo + 6 + s * 2)?);
                // an extension subtable (type 7) points on to the subtable of its own type
                let (ty, so) = if ty == 7 { (r.u16(so + 2)?, so + r.u32(so + 4)?) } else { (ty, so) };
                if let Some(sub) = Sub::parse(r, ty, so) {
                    subs.push(sub);
                }
            }
            lookups.push(subs);
        }
        Some(Gsub { lookups, applied })
    }

    fn apply(&self, buf: &mut Vec<(u16, usize)>) {
        for &l in &self.applied {
            let mut i = 0;
            while i < buf.len() {
                i = self.apply_at(l, buf, i, 0).unwrap_or(i + 1);
            }
        }
    }

    /// Lookup `l` at `buf[i]`: the place after what it wrote, or None when no subtable applies.
    fn apply_at(&self, l: u16, buf: &mut Vec<(u16, usize)>, i: usize, depth: u32) -> Option<usize> {
        let g = buf.get(i)?.0;
        for sub in self.lookups.get(usize::from(l))? {
            match sub {
                Sub::Single { cov, delta, to } => {
                    let Some(k) = cov.index(g) else { continue };
                    buf[i].0 = match delta {
                        Some(d) => g.wrapping_add(*d),
                        None => *to.get(k)?,
                    };
                    return Some(i + 1);
                }
                Sub::Ligature { cov, sets } => {
                    let Some(k) = cov.index(g) else { continue };
                    for (comps, lig) in sets.get(k)? {
                        let rest = buf.get(i + 1..i + 1 + comps.len());
                        if rest.is_some_and(|rest| rest.iter().map(|x| x.0).eq(comps.iter().copied())) {
                            buf[i].0 = *lig;
                            buf.drain(i + 1..i + 1 + comps.len());
                            return Some(i + 1);
                        }
                    }
                }
                Sub::Chain { back, input, ahead, records } => {
                    let covered = |at: Option<usize>, c: &Coverage| {
                        at.and_then(|at| buf.get(at)).is_some_and(|x| c.index(x.0).is_some())
                    };
                    let matched = input.iter().enumerate().all(|(k, c)| covered(Some(i + k), c))
                        && back.iter().enumerate().all(|(k, c)| covered(i.checked_sub(k + 1), c))
                        && ahead.iter().enumerate().all(|(k, c)| covered(Some(i + input.len() + k), c));
                    if !matched {
                        continue;
                    }
                    let len0 = buf.len();
                    if depth < NEST_MAX {
                        for &(seq, nested) in records {
                            self.apply_at(nested, buf, i + usize::from(seq), depth + 1);
                        }
                    }
                    let end = (i + input.len() + buf.len()).saturating_sub(len0);
                    return Some(end.max(i + 1));
                }
            }
        }
        None
    }
}

impl Sub {
    fn parse(r: &Reader<'_>, ty: u16, o: usize) -> Option<Sub> {
        let format = r.u16(o)?;
        match (ty, format) {
            (1, 1) => Some(Sub::Single {
                cov: Coverage::parse(r, o + usize::from(r.u16(o + 2)?))?,
                delta: Some(r.u16(o + 4)?),
                to: Vec::new(),
            }),
            (1, 2) => Some(Sub::Single {
                cov: Coverage::parse(r, o + usize::from(r.u16(o + 2)?))?,
                delta: None,
                to: r.u16s(o + 6, usize::from(r.u16(o + 4)?))?,
            }),
            (4, 1) => {
                let cov = Coverage::parse(r, o + usize::from(r.u16(o + 2)?))?;
                let mut sets = Vec::new();
                for k in 0..usize::from(r.u16(o + 4)?) {
                    let s = o + usize::from(r.u16(o + 6 + k * 2)?);
                    let mut set = Vec::new();
                    for j in 0..usize::from(r.u16(s)?) {
                        let l = s + usize::from(r.u16(s + 2 + j * 2)?);
                        let comps = usize::from(r.u16(l + 2)?).saturating_sub(1);
                        set.push((r.u16s(l + 4, comps)?, r.u16(l)?));
                    }
                    sets.push(set);
                }
                Some(Sub::Ligature { cov, sets })
            }
            (6, 3) => {
                let mut p = o + 2;
                let covs = |p: &mut usize| -> Option<Vec<Coverage>> {
                    let n = usize::from(r.u16(*p)?);
                    let v = (0..n)
                        .map(|k| Coverage::parse(r, o + usize::from(r.u16(*p + 2 + k * 2)?)))
                        .collect::<Option<Vec<_>>>()?;
                    *p += 2 + n * 2;
                    Some(v)
                };
                let back = covs(&mut p)?;
                let input = covs(&mut p)?;
                let ahead = covs(&mut p)?;
                let n = usize::from(r.u16(p)?);
                let records =
                    (0..n).map(|k| Some((r.u16(p + 2 + k * 4)?, r.u16(p + 4 + k * 4)?))).collect::<Option<Vec<_>>>()?;
                Some(Sub::Chain { back, input, ahead, records })
            }
            _ => None,
        }
    }
}
