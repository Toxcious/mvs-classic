//! A font face's metrics for the HUD's text layout, read from the face's TrueType file (the `font` kind's entry).
//!
//! The metrics the font cache reads through FreeType (FSlateFontRenderer::GetMaxHeight exe 1416477b0, GetBaseline
//! 141642bb0, advance cache 141641100, kerning cache 1416415c0), in font units: the line height and descender by the
//! FontFace asset's LayoutMethod (Metrics: hhea ascender - descender + line gap and the hhea descender; BoundingBox:
//! head yMax - yMin and yMin), the hmtx advance per character and the kern table's format 0 horizontal pairs (what
//! FT_Get_Kerning serves; GPOS is not read by the kerning-only shaper).

use std::collections::HashMap;

use indexmap::IndexMap;
use serde::Serialize;

use crate::kind::KindError;

/// A face's metrics (`hud/slate` FaceData): `adv` by character, `kern` by the two characters of a pair.
#[derive(Clone, Debug, Serialize)]
pub struct FaceData {
    pub upm: u16,
    pub height: i32,
    pub descender: i32,
    pub adv: IndexMap<String, u16>,
    pub kern: IndexMap<String, i32>,
}

/// A big-endian reader that reports a read past the end (a DataView's RangeError).
struct Be<'b>(&'b [u8], &'b str);

impl Be<'_> {
    fn at<const N: usize>(&self, o: usize) -> Result<[u8; N], KindError> {
        o.checked_add(N)
            .and_then(|end| self.0.get(o..end))
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| KindError::Failed(format!("ui: font {}: read past the end at {o}", self.1)))
    }

    fn u16(&self, o: usize) -> Result<u16, KindError> {
        self.at(o).map(u16::from_be_bytes)
    }

    fn i16(&self, o: usize) -> Result<i16, KindError> {
        self.at(o).map(i16::from_be_bytes)
    }

    fn u32(&self, o: usize) -> Result<usize, KindError> {
        self.at(o).map(|b| u32::from_be_bytes(b) as usize)
    }
}

/// The metrics of the face in `bytes` (`faceMetrics`); `what` names it in a message.
pub fn face_metrics(bytes: &[u8], bounding_box: bool, what: &str) -> Result<FaceData, KindError> {
    let b = Be(bytes, what);
    let fail = |m: String| KindError::Failed(format!("ui: font {what}: {m}"));
    let mut tabs: HashMap<&[u8], usize> = HashMap::new();
    for i in 0..usize::from(b.u16(4)?) {
        let tag = bytes.get(12 + i * 16..16 + i * 16).ok_or_else(|| fail("a table record past the end".into()))?;
        tabs.insert(tag, b.u32(20 + i * 16)?);
    }
    let table = |t: &[u8; 4]| tabs.get(&t[..]).copied();
    let (Some(head), Some(hhea), Some(hmtx), Some(cmap)) =
        (table(b"head"), table(b"hhea"), table(b"hmtx"), table(b"cmap"))
    else {
        return Err(fail("no head, hhea, hmtx or cmap table".into()));
    };
    let kern = table(b"kern");
    let upm = b.u16(head + 18)?;
    let (asc, desc, gap) = (b.i16(hhea + 4)?, b.i16(hhea + 6)?, b.i16(hhea + 8)?);
    if asc == 0 && desc == 0 {
        return Err(fail("hhea metrics empty (FreeType falls back to OS/2)".into()));
    }
    let (y_min, y_max) = (b.i16(head + 38)?, b.i16(head + 42)?);
    let n_h = usize::from(b.u16(hhea + 34)?);
    let adv_of = |gid: u16| -> Result<u16, KindError> { b.u16(hmtx + usize::from(gid).min(n_h.wrapping_sub(1)) * 4) };
    // cmap: the Windows BMP subtable (format 4)
    let mut sub = 0;
    for i in 0..usize::from(b.u16(cmap + 2)?) {
        if b.u16(cmap + 4 + i * 8)? == 3 && b.u16(cmap + 6 + i * 8)? == 1 {
            sub = cmap + b.u32(cmap + 8 + i * 8)?;
        }
    }
    if sub == 0 || b.u16(sub)? != 4 {
        return Err(fail("no format 4 cmap".into()));
    }
    let segs = usize::from(b.u16(sub + 6)?) / 2;
    let mut adv: IndexMap<String, u16> = IndexMap::new();
    let mut chars: HashMap<u16, Vec<char>> = HashMap::new();
    for s in 0..segs {
        let end = b.u16(sub + 14 + s * 2)?;
        let start = b.u16(sub + 16 + segs * 2 + s * 2)?;
        let delta = i32::from(b.i16(sub + 16 + segs * 4 + s * 2)?);
        let roff = sub + 16 + segs * 6 + s * 2;
        let ro = usize::from(b.u16(roff)?);
        let mut c = u32::from(start);
        while c <= u32::from(end) && c < 0xffff {
            let mut gid = if ro != 0 { u32::from(b.u16(roff + ro + (c - u32::from(start)) as usize * 2)?) } else { c };
            if ro != 0 && gid == 0 {
                c += 1;
                continue;
            }
            gid = (gid as i32 + delta) as u32 & 0xffff;
            if gid != 0 {
                // Deviation: a code point that is a lone surrogate has no Rust string and is skipped (no face of the build maps one).
                if let Some(ch) = char::from_u32(c) {
                    adv.insert(ch.to_string(), adv_of(gid as u16)?);
                    chars.entry(gid as u16).or_default().push(ch);
                }
            }
            c += 1;
        }
    }
    let mut pairs: IndexMap<String, i32> = IndexMap::new();
    if let Some(kern) = kern {
        let mut p = kern + 4;
        for _ in 0..b.u16(kern + 2)? {
            let len = usize::from(b.u16(p + 2)?);
            let cov = b.u16(p + 4)?;
            // tt_face_load_kern: horizontal, not minimum, format 0 subtables only
            if cov & 3 == 1 && cov >> 8 == 0 {
                for i in 0..usize::from(b.u16(p + 6)?) {
                    let o = p + 14 + i * 6;
                    let v = i32::from(b.i16(o + 4)?);
                    let (left, right) = (chars.get(&b.u16(o)?), chars.get(&b.u16(o + 2)?));
                    for a in left.into_iter().flatten() {
                        for c in right.into_iter().flatten() {
                            let key = format!("{a}{c}");
                            let was = if cov & 8 != 0 { 0 } else { pairs.get(&key).copied().unwrap_or(0) };
                            pairs.insert(key, was + v);
                        }
                    }
                }
            }
            p += len;
        }
    }
    let (height, descender) = if bounding_box {
        (i32::from(y_max) - i32::from(y_min), i32::from(y_min))
    } else {
        (i32::from(asc) - i32::from(desc) + i32::from(gap), i32::from(desc))
    };
    Ok(FaceData { upm, height, descender, adv, kern: pairs })
}
