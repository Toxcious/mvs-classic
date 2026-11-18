//! The store's entry containers the sampler reads: the json-bin container and the `animation` kind's
//! header.

use serde::Deserialize;

/// A section of a jsonbin body: offset from the body's start, length in bytes (`BodyRef`).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct BodyRef {
    pub offset: usize,
    pub length: usize,
}

const fn align16(n: usize) -> usize {
    (n + 15) & !15
}

/// `readJsonBin`: the header's JSON bytes and the body (a view) of an entry; an error on another magic.
pub fn read_json_bin<'a>(magic: &[u8; 4], bytes: &'a [u8]) -> Result<(&'a [u8], &'a [u8]), String> {
    if bytes.len() < 8 || &bytes[..4] != magic {
        let got = String::from_utf8_lossy(&bytes[..bytes.len().min(4)]).into_owned();
        return Err(format!("jsonbin: not an {} entry ({got})", String::from_utf8_lossy(magic)));
    }
    let len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let body = align16(8 + len).min(bytes.len());
    Ok((&bytes[8..8 + len], &bytes[body..]))
}

pub const ANIM_MAGIC: &[u8; 4] = b"MVA3";
pub const ANIM_FORMAT_VERSION: u32 = 2;
/// int32 a track in the `tracks` section: bone, rot, trans, scale.
pub const ANIM_TRACK_INTS: usize = 4;

/// `AnimCurveKey`.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct AnimCurveKey {
    pub time: f64,
    pub value: f64,
    pub arrive: f64,
    pub leave: f64,
    pub interp: f64,
}

#[derive(Debug, Deserialize)]
pub struct AnimCurve {
    pub name: String,
    pub keys: Vec<AnimCurveKey>,
}

#[derive(Debug, Deserialize)]
pub struct AnimAdditive {
    #[serde(rename = "type")]
    pub kind: f64,
}

/// `AnimHeader`: the fields the sampler reads.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnimHeader {
    pub version: u32,
    pub path: String,
    pub skeleton: String,
    pub frames: f64,
    pub length: f64,
    pub rate_scale: f64,
    pub interpolation: f64,
    pub root_motion: bool,
    pub force_root_lock: bool,
    pub root_lock: f64,
    pub normalized_root_motion_scale: bool,
    #[serde(default)]
    pub additive: Option<AnimAdditive>,
    pub tracks: BodyRef,
    pub curves: Vec<AnimCurve>,
    pub data: BodyRef,
}

/// `animTrackTable`: the tracks section as int32.
pub fn anim_track_table(h: &AnimHeader, body: &[u8]) -> Result<Vec<i32>, String> {
    let BodyRef { offset, length } = h.tracks;
    if offset + length > body.len() {
        return Err(format!("{}: tracks section past the body", h.path));
    }
    if length % (4 * ANIM_TRACK_INTS) != 0 {
        return Err(format!("{}: tracks section of {length} bytes", h.path));
    }
    Ok(body[offset..offset + length].as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes(*c)).collect())
}
