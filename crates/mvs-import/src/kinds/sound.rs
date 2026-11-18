//! The `sound` kind: one entry per Wwise medium the banks name (its AkMediaAsset's build path; a localized medium
//! its English(US) asset), the wem's own Vorbis packets with an identification header rebuilt from its 'fmt ' /
//! 'vorb' chunks and a setup header rebuilt from the codebooks the player's executable holds, stored once per
//! distinct setup under `/Sound/Setup/<hash>` ([`mvs_data::sound::MediumHeader`] gives the layout).
//! The Ogg pages are not written here.
//!
//! Lists the AkMediaAsset exports under /Game/WwiseAudio/Media and the AkLocalizedMediaAsset exports under
//! /Game/WwiseAudio/Localized/English_US_/Media, each by its object path, less the media packages that hold no data
//! (no .ubulk; the list reads each one it drops and fails unless its CurrentMediaAssetData is null). The setup entries
//! are named by the wem's setup hash (its 'vorb' uid) and listed from each medium's first bytes.

mod codebooks;
mod wem;
mod ww2ogg;

#[cfg(test)]
mod check;

use std::collections::BTreeMap;
use std::sync::Arc;

use mvs_data::sound::{MediumHeader, SOUND_KIND, SOUND_VERSION, encode_medium};
use mvs_pak::paths::split_object_path;
use mvs_uasset::bulk::PAYLOAD_IN_SEPERATE_FILE;
use mvs_uasset::natives::texture::read_bulk_range;
use mvs_uasset::natives::wwise::media_chunks;
use mvs_uasset::value::Value;
use rayon::prelude::*;

use crate::kind::{Cx, Item, Kind, KindError};
use crate::objview::ObjView;
use codebooks::{EXE_PATH, exe_codebooks};
use wem::{parse_wem, u32_at};
use ww2ogg::{CodebookLibrary, wem_to_vorbis};

/// The folders the kind lists, and the class of the media in each.
const ROOTS: [(&str, &str); 2] = [
    ("/Game/WwiseAudio/Media/", "/Script/AkAudio.AkMediaAsset"),
    ("/Game/WwiseAudio/Localized/English_US_/Media/", "/Script/AkAudio.AkLocalizedMediaAsset"),
];

/// The classes a medium is (the ones whose native class is UAkMediaAsset), by their name.
const MEDIA_TYPES: [&str; 3] = ["AkMediaAsset", "AkLocalizedMediaAsset", "AkExternalMediaAsset"];

/// The entry path prefix of a shared setup header.
const SETUP_PREFIX: &str = "/Sound/Setup/";

/// How many bytes of a wem reach its 'fmt ' chunk's setup hash.
const HEAD: u32 = 96;

/// The exe's codebook library, read once per run.
struct Books(Result<CodebookLibrary, String>);

fn books(cx: &Cx<'_>) -> Result<Arc<Books>, KindError> {
    let b = cx.shared.get_or_init(|| {
        Books(
            mvs_pak::setup::read_build_file(cx.paks, EXE_PATH)
                .map_err(|e| e.to_string())
                .and_then(|exe| exe_codebooks(&exe)),
        )
    });
    match &b.0 {
        Ok(_) => Ok(b),
        Err(e) => Err(KindError::Failed(e.clone())),
    }
}

/// Where a medium's wem lies in its package's .ubulk.
struct Place {
    offset: u64,
    size: u32,
}

/// Where the AkMediaAsset at `path` keeps its medium: the non-prefetch chunk of its CurrentMediaAssetData (a streamed
/// medium's prefetch copy is a short head of it). A medium with no data is NotFound.
fn media_place(cx: &Cx<'_>, path: &str) -> Result<Place, KindError> {
    let (pkg, i) = cx.read(path)?;
    let class = cx.class_of(&pkg, i)?;
    let name = class.rsplit('.').next().unwrap_or("");
    if !MEDIA_TYPES.contains(&name) {
        return Err(KindError::NotFound(format!("wem: {path} is a {name}, not a medium")));
    }
    let view = ObjView::new(cx.cache, pkg.clone(), i, i, String::new())?;
    let at = match view.uobj().data.get("CurrentMediaAssetDataIndex") {
        Some(Value::Int(a)) => *a,
        _ => return Err(KindError::Failed(format!("wem: {path}: CurrentMediaAssetData was not read"))),
    };
    if at == 0 {
        return Err(KindError::NotFound(format!("wem: {path}: a medium with no data (CurrentMediaAssetData null)")));
    }
    let Some(data) = usize::try_from(at - 1).ok().filter(|&d| d < pkg.exports.len()) else {
        return Err(KindError::Failed(format!(
            "wem: {path}: CurrentMediaAssetData is {at}, not an export of its package"
        )));
    };
    let data_class = cx.class_of(&pkg, data)?;
    if !data_class.ends_with(".AkMediaAssetData") {
        return Err(KindError::Failed(format!("wem: {path}: CurrentMediaAssetData is a {data_class}")));
    }
    let data_view = ObjView::new(cx.cache, pkg.clone(), data, data, String::new())?;
    let chunks = media_chunks(data_view.uobj());
    let Some(c) = chunks.as_ref().and_then(|c| c.iter().find(|c| !c.is_prefetch)) else {
        return Err(KindError::Failed(format!(
            "wem: {path}: no non-prefetch media chunk ({} chunks)",
            chunks.map_or(0, |c| c.len())
        )));
    };
    let h = c.header;
    if i64::from(h.element_count) != i64::from(h.size_on_disk) {
        return Err(KindError::Failed(format!(
            "wem: {path}: a compressed media chunk ({} bytes on disk for {})",
            h.size_on_disk, h.element_count
        )));
    }
    if c.inline_at.is_some() {
        // Unverified: no medium of this build holds its chunk inline (the list drops the packages with no .ubulk after
        // reading them); an inline chunk would be read from the package's export data.
        return Err(KindError::NotPorted(format!("wem: {path}: an inline media chunk")));
    }
    if h.flags & PAYLOAD_IN_SEPERATE_FILE == 0 {
        return Err(KindError::Failed(format!(
            "wem: {path}: a media chunk neither inline nor in the .ubulk (flags {:x})",
            h.flags
        )));
    }
    let offset = u64::try_from(h.offset_in_file)
        .map_err(|_| KindError::Failed(format!("wem: {path}: a media chunk at {}", h.offset_in_file)))?;
    Ok(Place { offset, size: h.size_on_disk })
}

/// The first `len` bytes (at most) of the medium's wem.
fn read_wem_head(cx: &Cx<'_>, path: &str, len: u32) -> Result<Vec<u8>, KindError> {
    let place = media_place(cx, path)?;
    let n = place.size.min(len);
    read_bulk_range(cx.paks, cx.paths, split_object_path(path).0, place.offset, u64::from(n))?
        .ok_or_else(|| KindError::Failed(format!("wem: {path}: its media chunk is in a .ubulk the paks do not hold")))
}

/// The wem bytes of the AkMediaAsset / AkLocalizedMediaAsset at `path`.
fn read_wem(cx: &Cx<'_>, path: &str) -> Result<Vec<u8>, KindError> {
    let bytes = read_wem_head(cx, path, u32::MAX)?;
    if bytes.len() < 22 || &bytes[0..4] != b"RIFF" || bytes[20] != 0xff || bytes[21] != 0xff {
        return Err(KindError::Failed(format!("wem: {path}: not a Wwise Vorbis wem")));
    }
    Ok(bytes)
}

/// The setup hash of a wem from its first bytes: the 'fmt ' chunk (the first chunk) holds the `vorb` header at +0x18.
fn setup_hash(head: &[u8], path: &str) -> Result<u32, KindError> {
    let at = 12 + 8 + 0x18 + 0x24;
    if head.len() < at + 4 || &head[0..4] != b"RIFF" || &head[12..16] != b"fmt " || u32_at(head, 16) != 0x42 {
        return Err(KindError::Failed(format!("wem: {path}: no 'fmt ' chunk first")));
    }
    Ok(u32_at(head, at))
}

fn setup_path(uid: u32) -> String {
    format!("{SETUP_PREFIX}{uid:08x}")
}

/// Python's round(x, 3), the media meta's rounding (a decimal of the double's exact value, ties to even).
fn round3(x: f64) -> f64 {
    format!("{x:.3}").parse().unwrap_or(x)
}

pub struct Sound;

impl Kind for Sound {
    fn id(&self) -> &'static str {
        SOUND_KIND
    }

    fn version(&self) -> u64 {
        SOUND_VERSION
    }

    fn content_type(&self, _bytes: &[u8]) -> &'static str {
        "application/octet-stream"
    }

    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        let mut out: Vec<String> = Vec::new();
        let mut dropped: Vec<String> = Vec::new();
        for e in cx.catalog.exports() {
            if !ROOTS.iter().any(|(root, class)| e.package.starts_with(root) && e.class == *class) {
                continue;
            }
            let path = e.object_path();
            let has_bulk =
                cx.paths.package_file(e.package).is_some_and(|file| cx.paks.entry(&format!("{file}.ubulk")).is_some());
            if has_bulk {
                out.push(path);
            } else {
                dropped.push(path);
            }
        }
        // a medium with no .ubulk could still hold its data inline: each dropped one has to be a medium with none
        for path in &dropped {
            match media_place(cx, path) {
                Err(KindError::NotFound(_)) => {}
                Err(e) => return Err(e),
                Ok(_) => {
                    return Err(KindError::Failed(format!(
                        "sound: {path} has no .ubulk but its CurrentMediaAssetData is not null"
                    )));
                }
            }
        }
        // each distinct setup once, named by its hash and built from the first medium that has it
        let hashes: Vec<Option<u32>> =
            out.par_iter().map(|p| read_wem_head(cx, p, HEAD).ok().and_then(|h| setup_hash(&h, p).ok())).collect();
        let mut setups: BTreeMap<u32, &str> = BTreeMap::new();
        for (p, h) in out.iter().zip(&hashes) {
            if let Some(h) = h {
                setups.entry(*h).or_insert(p);
            }
        }
        let mut items: Vec<Item> = out.iter().map(Item::new).collect();
        items.extend(setups.into_iter().map(|(uid, p)| Item { path: setup_path(uid), source: Some(p.to_owned()) }));
        Ok(items)
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let setup = item.path.strip_prefix(SETUP_PREFIX);
        let path = match (setup, &item.source) {
            (Some(_), Some(source)) => source,
            (Some(_), None) => return Err(KindError::Failed(format!("sound: {} names no medium", item.path))),
            (None, _) => {
                if !ROOTS.iter().any(|(root, _)| item.path.starts_with(root)) {
                    return Err(KindError::NotFound(format!(
                        "sound: {} is not under the media folders the kind lists",
                        item.path
                    )));
                }
                &item.path
            }
        };
        let lib = books(cx)?;
        let Ok(lib) = &lib.0 else { unreachable!("books() fails for an error") };
        let wem_bytes = read_wem(cx, path)?;
        let w = parse_wem(&wem_bytes, path).map_err(KindError::Failed)?;
        let v = wem_to_vorbis(&wem_bytes, lib, path).map_err(KindError::Failed)?;
        let vorbis = w.vorbis;
        if let Some(hash) = setup {
            if u32::from_str_radix(hash, 16).ok() != Some(vorbis.uid) {
                return Err(KindError::Failed(format!("sound: {path}: its setup hash is not {hash}")));
            }
            return Ok(v.setup);
        }

        // the granule at the end of each audio packet: a packet decodes a quarter of its own block plus a quarter of
        // the one before it (the first none); the last is cut to the medium's declared length
        let mut granules = Vec::with_capacity(v.audio.len());
        let mut end = 0u32;
        for (i, b) in v.blocks.iter().enumerate() {
            if i > 0 {
                end += (v.blocks[i - 1] + b) / 4;
            }
            granules.push(end.min(vorbis.samples));
        }
        if granules.is_empty() || end < vorbis.samples {
            return Err(KindError::Failed(format!(
                "sound: {path}: {} samples declared, the packets hold {end}",
                vorbis.samples
            )));
        }
        if w.loops.len() > 1 {
            return Err(KindError::Failed(format!("sound: {path}: {} loops", w.loops.len())));
        }
        let norm = match &w.analysis {
            Some(a) if a.norm_gain > 0.0 => round3(20.0 * mvs_math::libm::log10(f64::from(a.norm_gain))),
            _ => 0.0,
        };
        let header = MediumHeader {
            rate: w.rate,
            channels: w.channels,
            mask: w.mask.unwrap_or(0),
            frames: vorbis.samples,
            // the 'smpl' end is inclusive: the exclusive end is one more
            loop_points: w.loops.first().map(|l| [l.start, l.end + 1]),
            norm,
            setup: setup_path(vorbis.uid),
            ident: v.ident,
        };
        let packets: Vec<(&[u8], u32)> = v.audio.iter().map(Vec::as_slice).zip(granules).collect();
        Ok(encode_medium(&header, &packets))
    }
}
