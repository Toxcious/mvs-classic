//! A medium's meta as the banks name it: the AkMediaAsset's wem bytes and what its 'fmt ', 'smpl' and 'akd ' chunks
//! say The wem's bytes are read
//! as the media natives place them: the non-prefetch chunk of the medium's data, from the .ubulk.

use mvs_data::sound::MediaRef;
use mvs_math::libm::log10;
use mvs_uasset::bulk::PAYLOAD_IN_SEPERATE_FILE;
use mvs_uasset::natives::texture::read_bulk_range;
use mvs_uasset::natives::wwise::media_chunks;
use mvs_uasset::value::Value;

use super::tree::{R, fail};
use crate::kind::{Cx, KindError};

/// The classes a medium is: the ones whose native class is UAkMediaAsset.
const MEDIA_TYPES: [&str; 3] = ["AkMediaAsset", "AkLocalizedMediaAsset", "AkExternalMediaAsset"];

/// The wem bytes of the AkMediaAsset / AkLocalizedMediaAsset at `path`; `None` when the paks hold no such medium's
/// bytes (24 shared, 7 English(US), 5 Spanish(MX) packages of the build hold no data).
pub(super) fn read_wem(cx: &Cx<'_>, path: &str) -> Result<Option<Vec<u8>>, KindError> {
    let what = format!("wem: {path}");
    let Ok((pkg, i)) = cx.read(path) else { return Ok(None) };
    let obj = cx.cache.object(&pkg, i);
    if !MEDIA_TYPES.contains(&obj.type_name.as_str()) {
        return Ok(None);
    }
    if let Some(e) = obj.issues.first() {
        return fail(&what, &e.message);
    }
    let at = match obj.data.get("CurrentMediaAssetDataIndex") {
        Some(Value::Int(n)) => *n,
        _ => return fail(&what, "CurrentMediaAssetData was not read"),
    };
    if at == 0 {
        return Ok(None);
    }
    let Ok(at) = usize::try_from(at - 1) else {
        return fail(&what, format!("CurrentMediaAssetData is {at}, not an export of its package"));
    };
    if at >= pkg.exports.len() {
        return fail(&what, "CurrentMediaAssetData is not an export of its package");
    }
    let data = cx.cache.object(&pkg, at);
    if data.type_name != "AkMediaAssetData" {
        return fail(&what, format!("CurrentMediaAssetData is a {}", data.type_name));
    }
    if let Some(e) = data.issues.first() {
        return fail(&what, &e.message);
    }
    let chunks = media_chunks(&data).unwrap_or_default();
    let Some(c) = chunks.iter().find(|c| !c.is_prefetch) else {
        return fail(&what, format!("no non-prefetch media chunk ({} chunks)", chunks.len()));
    };
    let h = c.header;
    if i64::from(h.element_count) != i64::from(h.size_on_disk) {
        return fail(
            &what,
            format!("a compressed media chunk ({} bytes on disk for {})", h.size_on_disk, h.element_count),
        );
    }
    if c.inline_at.is_some() {
        return Err(KindError::NotPorted(format!("{what}: an inline media chunk")));
    }
    if h.flags & PAYLOAD_IN_SEPERATE_FILE == 0 {
        return fail(&what, format!("a media chunk neither inline nor in the .ubulk (flags {:x})", h.flags));
    }
    let Ok(offset) = u64::try_from(h.offset_in_file) else { return fail(&what, "a negative chunk offset") };
    let bytes = read_bulk_range(cx.paks, cx.paths, path, offset, u64::from(h.size_on_disk))?;
    let Some(bytes) = bytes else { return fail(&what, "its media chunk is in a .ubulk the paks do not hold") };
    if bytes.len() < 22 || &bytes[..4] != b"RIFF" || bytes[20] != 0xff || bytes[21] != 0xff {
        return fail(&what, "not a Wwise Vorbis wem");
    }
    Ok(Some(bytes))
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn f32_at(b: &[u8], o: usize) -> f64 {
    f64::from(f32::from_bits(u32_at(b, o)))
}

/// Python's round(x, 3), the media meta's rounding: toFixed's ties go away from zero, Python's to even. A tie is
/// exact only for x * 16 an odd integer.
fn round3(x: f64) -> f64 {
    let s = x * 16.0;
    let r = if s.fract() == 0.0 && s.abs() < 9e15 && s.rem_euclid(2.0) != 0.0 {
        let f = (x * 1000.0).floor();
        (if f.rem_euclid(2.0) == 0.0 { f } else { f + 1.0 }) / 1000.0
    } else {
        format!("{x:.3}").parse().unwrap_or(x)
    };
    if r == 0.0 { 0.0 } else { r }
}

/// A medium's meta from its wem (`what`: its path, for the error), its `path` set.
pub(super) fn media_ref(path: &str, wem: &[u8], what: &str) -> R<MediaRef> {
    if wem.len() < 12 || &wem[..4] != b"RIFF" || &wem[8..12] != b"WAVE" {
        return fail(what, "not a RIFF WAVE file");
    }
    let mut m = MediaRef { path: path.to_owned(), ..MediaRef::default() };
    let (mut have_fmt, mut have_data) = (false, false);
    let mut pos = 12;
    while pos + 8 <= wem.len() {
        let id = &wem[pos..pos + 4];
        let size = u32_at(wem, pos + 4) as usize;
        let at = pos + 8;
        if at + size > wem.len() {
            return fail(what, format!("chunk '{}' runs past the file", String::from_utf8_lossy(id)));
        }
        match id {
            b"fmt " => {
                if size != 0x42 {
                    return fail(what, format!("'fmt ' of {size} bytes, not Wwise Vorbis (0x42)"));
                }
                if u16_at(wem, at) != 0xffff {
                    return fail(what, format!("format tag {:x}, not Wwise Vorbis (ffff)", u16_at(wem, at)));
                }
                m.channels = u16_at(wem, at + 2);
                m.rate = u32_at(wem, at + 4);
                let config = u32_at(wem, at + 20);
                if m.channels > 2 {
                    let mask = config >> 12;
                    if config & 0xff != u32::from(m.channels)
                        || (config >> 8) & 0xf != 1
                        || mask.count_ones() != u32::from(m.channels)
                    {
                        return fail(what, format!("channel config {config:x} is not a standard speaker mask"));
                    }
                    m.mask = Some(mask);
                }
                m.frames = u32_at(wem, at + 24);
                have_fmt = true;
            }
            b"smpl" => {
                if size < 36 {
                    return fail(what, format!("'smpl' of {size} bytes"));
                }
                let n = u32_at(wem, at + 28) as usize;
                if 36 + 24 * n > size {
                    return fail(what, format!("'smpl' names {n} loops in {size} bytes"));
                }
                // the first loop; the 'smpl' end is inclusive
                if n > 0 && m.loop_points.is_none() {
                    m.loop_points = Some([u32_at(wem, at + 36 + 8), u32_at(wem, at + 36 + 12).wrapping_add(1)]);
                }
            }
            b"akd " => {
                if size < 12 {
                    return fail(what, format!("'akd ' of {size} bytes"));
                }
                let n = u32_at(wem, at + 8) as usize;
                if n != 0 && 16 + 6 * n > size {
                    return fail(what, format!("'akd ' names {n} envelope points in {size} bytes"));
                }
                let norm_gain = f32_at(wem, at);
                if norm_gain > 0.0 {
                    m.norm = Some(round3(20.0 * log10(norm_gain)));
                }
                if n > 0 {
                    m.peak = Some(round3(f32_at(wem, at + 12)));
                    m.env = (0..n)
                        .map(|i| [f64::from(u32_at(wem, at + 16 + 6 * i)), f64::from(u16_at(wem, at + 20 + 6 * i))])
                        .collect();
                }
            }
            b"data" => have_data = true,
            _ => {}
        }
        pos = at + size + (size & 1);
    }
    if !have_fmt {
        return fail(what, "no 'fmt ' chunk");
    }
    if !have_data {
        return fail(what, "no 'data' chunk");
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round3_ties_to_even() {
        assert_eq!(round3(0.0625), 0.062);
        assert_eq!(round3(0.1875), 0.188);
        assert_eq!(round3(-0.0625), -0.062);
        assert_eq!(round3(1.23456), 1.235);
        assert_eq!(round3(-0.0001), 0.0);
        assert_eq!(round3(5.0), 5.0);
    }
}
