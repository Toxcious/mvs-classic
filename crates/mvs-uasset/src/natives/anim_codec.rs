//! An AnimSequence's compressed tracks as per-track blocks, and its CompressedRichCurve stream decoded, for the
//! animation kind.
//!
//! Bone tracks. Every block the entry holds is a per-track block: a header word (format << 28 | flags << 24 | key
//! count; flags 1 / 2 / 4 the stored X / Y / Z, 8 a frame table), the interval bounds of format 3 (min, range a stored
//! component), the keys, 4-byte aligned, then with flag 8 the frame of each key (u8, u16 when the sequence has more
//! than 255 frames), 4-byte aligned. Formats: 1 ACF_Float96NoW, 2 ACF_Fixed48NoW, 3 ACF_IntervalFixed32NoW, the only
//! ones the build's per-track streams hold (anything else fails by name).
//! - AKF_PerTrackCompression: the build's stream as it is, its offsets as they are (every block is checked to lie in
//!   the stream and to have a known format).
//! - AKF_VariableKeyLerp (ACF_Float96NoW rotations, ACF_None translations and scales: 3 floats a key, a frame table
//!   behind the keys when there are more than one): each track rewritten as a format 1 block, all three components,
//!   the keys and table copied; a table that says a key a frame is left out (the block's keys are then spread evenly,
//!   which reads the same keys).
//! - AKF_ConstantKeyLerp (ACF_Fixed48NoW rotations, ACF_None vectors; keys spread evenly, no table): a rotation of
//!   one key is one Float96NoW key (format 1), of more keys 6-byte Fixed48NoW keys (format 2, the same decode:
//!   (u16 - 32767) / 32767, w from x, y, z); a vector is 12-byte floats (format 1).
//!
//! Offset -1 is "none stored" in the per-track stream (identity rotation, zero translation, zero scale); the two
//! key-lerp codecs have no -1 test, so a negative offset or a count below 1 there fails by name. A sequence that
//! stores no scale stream at all (CompressedScaleOffsets empty) gives every track scale [`NO_SCALE_STREAM`]: the pose
//! keeps its start value there (the ref pose, or the additive identity), which a -1 (zero) does not.
//!
//! Curves: the stream opens with one 12-byte FCurveDesc a curve, in CompressedCurveNames order (u8
//! ERichCurveCompressionFormat, u8 key time format, u8 pre / post extrapolation, f32 ConstantValue or i32 NumKeys, i32
//! KeyDataOffset into the stream). A keyed curve's data: for Mixed one byte a key (its ERichCurveCompressionFormat),
//! padded to 4; the key times (float32); the values, 3 floats a key (value, arrive, leave) for Cubic and Mixed, 1 for
//! Linear. The keys keep the exe's evaluation in the richCurve form (the key's `interp` is the mode of the interval
//! that starts at the key):
//! - Empty: ConstantValue, MAX_FLT read as 0 (InDefaultValue 0); Constant: ConstantValue. One constant key at 0.
//! - Linear: every key linear; Cubic: every key cubic, Bezier with leave_k dt/3 and arrive_k+1 dt/3 (the richCurve
//!   rule); Mixed: key byte 1 constant, 2 linear, 5 weighted (refused), anything else cubic.
//!
//! Refused by name (none in the build): the Weighted format or a weighted key (its evaluator 1429f87b0 was not read),
//! u16 key times, an extrapolation other than Constant (AnimCurveKey has no field for it).

use super::anim_sequence::AnimSequenceTail;
use crate::archive::{ReadError, Result};

/// A track's scale offset of a sequence with no scale stream: the pose keeps its start value.
pub const NO_SCALE_STREAM: i32 = -2;

const FMT_FLOAT96: u32 = 1;
const FMT_FIXED48: u32 = 2;
const FMT_INTERVAL32: u32 = 3;

/// ERichCurveInterpMode as the key's `interp` holds it.
pub const INTERP_LINEAR: u8 = 0;
pub const INTERP_CONSTANT: u8 = 1;
pub const INTERP_CUBIC: u8 = 2;

const RCCF: [&str; 6] = ["Empty", "Constant", "Linear", "Cubic", "Mixed", "Weighted"];
const RCCE_CONSTANT: u8 = 4;

/// A track's block offsets in [`TrackBlocks::data`].
#[derive(Clone, Copy, Debug, Default)]
pub struct TrackOffsets {
    pub rot: i32,
    pub trans: i32,
    pub scale: i32,
}

/// A sequence's bone tracks as per-track blocks: per compressed track its block offsets, and the bytes.
#[derive(Clone, Debug, Default)]
pub struct TrackBlocks {
    pub offsets: Vec<TrackOffsets>,
    pub data: Vec<u8>,
}

/// One key of a decoded curve.
#[derive(Clone, Copy, Debug, Default)]
pub struct AnimCurveKey {
    pub time: f32,
    pub value: f32,
    /// ERichCurveInterpMode as the format holds it.
    pub interp: u8,
    pub arrive: f32,
    pub leave: f32,
}

/// A decoded curve by its CompressedCurveNames entry.
#[derive(Clone, Debug, Default)]
pub struct AnimCurve {
    pub name: String,
    pub keys: Vec<AnimCurveKey>,
}

const fn bits3(m: u32) -> usize {
    ((m & 1) + ((m >> 1) & 1) + ((m >> 2) & 1)) as usize
}

const fn align4(n: usize) -> usize {
    (n + 3) & !3
}

fn fail<T>(msg: String) -> Result<T> {
    Err(ReadError::parse(msg))
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().expect("four bytes"))
}

fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(b[o..o + 4].try_into().expect("four bytes"))
}

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().expect("four bytes"))
}

/// The stored components of a block: Float96 with flags & 7 == 0 reads all three.
fn stored_components(format: u32, flags: u32) -> usize {
    if format == FMT_FLOAT96 && flags & 7 == 0 { 3 } else { bits3(flags & 7) }
}

/// The bytes of the keys of a block (bounds included), before the 4-byte pad and the frame table.
fn key_bytes(format: u32, flags: u32, keys: usize, what: &str) -> Result<usize> {
    let nc = stored_components(format, flags);
    match format {
        FMT_FLOAT96 => Ok(4 * nc * keys),
        FMT_FIXED48 => Ok(2 * nc * keys),
        FMT_INTERVAL32 => Ok(8 * nc + 4 * keys),
        _ => fail(format!(
            "{what}: per-track format {format} has no decoder (1 Float96NoW, 2 Fixed48NoW, 3 IntervalFixed32NoW)"
        )),
    }
}

/// The size of the per-track block at byte `o` of `dv` (header included), checked against the view's end.
fn block_size(dv: &[u8], o: i32, frames: i32, what: &str) -> Result<usize> {
    let at = match usize::try_from(o) {
        Ok(at) if at & 3 == 0 && at + 4 <= dv.len() => at,
        _ => {
            return fail(format!("{what}: block offset {o} outside the {}-byte stream or not 4-aligned", dv.len()));
        }
    };
    let h = u32_at(dv, at);
    let (format, flags, keys) = (h >> 28, (h >> 24) & 15, (h & 0x00ff_ffff) as usize);
    if keys < 1 {
        return fail(format!("{what}: block at {o} has no key"));
    }
    let mut n = align4(4 + key_bytes(format, flags, keys, what)?);
    if flags & 8 != 0 {
        n = align4(n + keys * if frames > 0xff { 2 } else { 1 });
    }
    if at + n > dv.len() {
        return fail(format!("{what}: block at {o} ({n} bytes) runs past the {}-byte stream", dv.len()));
    }
    Ok(n)
}

/// The tracks of `tail` as per-track blocks (see the module comment); `what` names the sequence in an error.
pub fn track_blocks(tail: &AnimSequenceTail<'_>, what: &str) -> Result<TrackBlocks> {
    let n = tail.compressed_track_to_skeleton_map_table.len();
    if n == 0 {
        return Ok(TrackBlocks::default()); // nothing decompressed: the ref pose
    }
    let Some(cd) = &tail.compressed_data_structure else {
        return fail(format!("{what}: {n} tracks and no compressed data description (an empty bone codec)"));
    };
    match cd.key_encoding_format {
        "AKF_PerTrackCompression" => per_track(tail, n, what),
        "AKF_VariableKeyLerp" | "AKF_ConstantKeyLerp" => key_lerp(tail, n, what),
        other => fail(format!("{what}: key encoding {other} has no codec")),
    }
}

/// The index in the scale offsets of track `ti`'s slot; `None` when the sequence stores no scale offsets.
fn scale_slot(tail: &AnimSequenceTail<'_>, ti: usize, what: &str) -> Result<Option<usize>> {
    let cd = tail.compressed_data_structure.as_ref().expect("a description");
    let (len, strip) = (cd.compressed_scale_offsets.len(), cd.compressed_scale_strip_size);
    if len == 0 {
        return Ok(None);
    }
    let at = usize::try_from(strip).ok().filter(|&s| s >= 1).and_then(|s| ti.checked_mul(s)).filter(|&at| at < len);
    match at {
        Some(at) => Ok(Some(at)),
        None => fail(format!("{what}: scale offsets ({len}, strip {strip}) do not cover track {ti}")),
    }
}

fn per_track(tail: &AnimSequenceTail<'_>, n: usize, what: &str) -> Result<TrackBlocks> {
    let cd = tail.compressed_data_structure.as_ref().expect("a description");
    let off = &cd.compressed_track_offsets;
    if off.len() != n * 2 {
        return fail(format!("{what}: {} track offsets for {n} per-track tracks (2 a track)", off.len()));
    }
    let data = cd.compressed_byte_stream.to_vec();
    let frames = cd.compressed_number_of_frames;
    let check = |o: i32, ti: usize, part: &str| -> Result<i32> {
        if o == -1 {
            return Ok(-1);
        }
        block_size(&data, o, frames, &format!("{what} track {ti} {part}"))?;
        Ok(o)
    };
    let mut offsets = Vec::with_capacity(n);
    for ti in 0..n {
        let s = scale_slot(tail, ti, what)?;
        let trans = check(off[ti * 2], ti, "translation")?;
        let rot = check(off[ti * 2 + 1], ti, "rotation")?;
        let scale = match s {
            None => NO_SCALE_STREAM,
            Some(s) => check(cd.compressed_scale_offsets[s], ti, "scale")?,
        };
        offsets.push(TrackOffsets { rot, trans, scale });
    }
    Ok(TrackBlocks { offsets, data })
}

/// VariableKeyLerp and ConstantKeyLerp rewritten as per-track blocks.
fn key_lerp(tail: &AnimSequenceTail<'_>, n: usize, what: &str) -> Result<TrackBlocks> {
    let cd = tail.compressed_data_structure.as_ref().expect("a description");
    let variable = cd.key_encoding_format == "AKF_VariableKeyLerp";
    let want_rot = if variable { "ACF_Float96NoW" } else { "ACF_Fixed48NoW" };
    if cd.rotation_compression_format != want_rot
        || cd.translation_compression_format != "ACF_None"
        || cd.scale_compression_format != "ACF_None"
    {
        return fail(format!(
            "{what}: {} with {} / {} / {} has no codec ({want_rot} / ACF_None / ACF_None)",
            cd.key_encoding_format,
            cd.rotation_compression_format,
            cd.translation_compression_format,
            cd.scale_compression_format
        ));
    }
    let off = &cd.compressed_track_offsets;
    if off.len() != n * 4 {
        return fail(format!(
            "{what}: {} track offsets for {n} {} tracks (4 a track)",
            off.len(),
            cd.key_encoding_format
        ));
    }
    let so = &cd.compressed_scale_offsets;
    if !so.is_empty() && cd.compressed_scale_strip_size != 2 {
        return fail(format!("{what}: scale strip {} (2: offset, keys)", cd.compressed_scale_strip_size));
    }
    let src = cd.compressed_byte_stream;
    let frames = cd.compressed_number_of_frames;
    let wide = frames > 0xff;
    let mut data: Vec<u8> = Vec::new();
    let mut track = |o: i32, keys: i32, rotation: bool, part: &str| -> Result<i32> {
        let w = format!("{what} track {part}");
        let (Ok(o), Ok(keys)) = (usize::try_from(o), usize::try_from(keys)) else {
            return fail(format!("{w}: offset {o}, {keys} keys (the key-lerp codecs store every track)"));
        };
        if keys < 1 {
            return fail(format!("{w}: offset {o}, {keys} keys (the key-lerp codecs store every track)"));
        }
        let (format, flags, len, table_len, table);
        if !variable {
            // ConstantKeyLerp: spread evenly; a rotation of more than one key is Fixed48NoW
            let fixed = rotation && keys > 1;
            len = keys * if fixed { 6 } else { 12 };
            if o + len > src.len() {
                return fail(format!("{w}: {keys} keys at {o} run past the {}-byte stream", src.len()));
            }
            format = if fixed { FMT_FIXED48 } else { FMT_FLOAT96 };
            flags = 7u32;
            table_len = 0;
            table = false;
        } else {
            if o & 3 != 0 {
                return fail(format!("{w}: offset {o} not 4-aligned"));
            }
            len = keys * 12;
            table_len = if keys > 1 { keys * if wide { 2 } else { 1 } } else { 0 };
            if o + len + table_len > src.len() {
                return fail(format!("{w}: {keys} keys at {o} run past the {}-byte stream", src.len()));
            }
            let frame_of = |k: usize| -> usize {
                if wide {
                    usize::from(u16::from_le_bytes([src[o + len + k * 2], src[o + len + k * 2 + 1]]))
                } else {
                    usize::from(src[o + len + k])
                }
            };
            let mut t = false;
            let mut k = 0;
            while k < keys && table_len != 0 && !t {
                t = usize::try_from(frames).is_ok_and(|f| keys != f) || frames < 0 || frame_of(k) != k;
                k += 1;
            }
            format = FMT_FLOAT96;
            flags = if t { 15 } else { 7 };
            table = t;
        }
        let table_bytes = if table { table_len } else { 0 };
        let mut b = vec![0u8; align4(align4(4 + len) + table_bytes)];
        let header = (format << 28) | (flags << 24) | u32::try_from(keys).map_err(|_| ReadError::parse("keys"))?;
        b[..4].copy_from_slice(&header.to_le_bytes());
        b[4..4 + len].copy_from_slice(&src[o..o + len]);
        if table {
            b[4 + len..4 + len + table_len].copy_from_slice(&src[o + len..o + len + table_len]);
        }
        let at = data.len();
        data.extend_from_slice(&b);
        i32::try_from(at).map_err(|_| ReadError::parse(format!("{w}: blocks past 2 GB")))
    };
    let mut offsets = Vec::with_capacity(n);
    for ti in 0..n {
        let trans = track(off[ti * 4], off[ti * 4 + 1], false, &format!("{ti} translation"))?;
        let rot = track(off[ti * 4 + 2], off[ti * 4 + 3], true, &format!("{ti} rotation"))?;
        let scale = match scale_slot(tail, ti, what)? {
            None => NO_SCALE_STREAM,
            Some(s) => {
                let Some(&keys) = so.get(s + 1) else {
                    return fail(format!("{what} track {ti} scale: no key count at scale offset {}", s + 1));
                };
                track(so[s], keys, false, &format!("{ti} scale"))?
            }
        };
        offsets.push(TrackOffsets { rot, trans, scale });
    }
    Ok(TrackBlocks { offsets, data })
}

/// The curves of `tail`'s CompressedRichCurve stream, by CompressedCurveNames (see the module comment).
pub fn decode_curves(tail: &AnimSequenceTail<'_>, what: &str) -> Result<Vec<AnimCurve>> {
    let names = &tail.compressed_curve_names;
    let s = tail.compressed_curve_byte_stream;
    if names.is_empty() {
        return Ok(Vec::new());
    }
    if s.len() < names.len() * 12 {
        return fail(format!("{what}: curve stream of {} bytes for {} curves", s.len(), names.len()));
    }
    let mut curves = Vec::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        let w = format!("{what} curve {name}");
        let d = i * 12;
        let (format, time_format, pre, post) = (s[d], s[d + 1], s[d + 2], s[d + 3]);
        if pre != RCCE_CONSTANT || post != RCCE_CONSTANT {
            return fail(format!("{w}: extrapolation {pre} / {post} (only Constant, 4, is kept)"));
        }
        if format == 0 || format == 1 {
            let v = f32_at(s, d + 4);
            let value = if format == 0 && v == f32::MAX { 0.0 } else { v };
            let key = AnimCurveKey { time: 0.0, value, arrive: 0.0, leave: 0.0, interp: INTERP_CONSTANT };
            curves.push(AnimCurve { name: name.clone(), keys: vec![key] });
            continue;
        }
        if !(2..=4).contains(&format) {
            let shown = RCCF.get(usize::from(format)).map_or_else(|| format.to_string(), |f| (*f).to_owned());
            return fail(format!("{w}: format {shown} has no decoder"));
        }
        if time_format != 1 {
            return fail(format!("{w}: key time format {time_format} (only float32, 1, is read)"));
        }
        let nk = i64::from(i32_at(s, d + 4));
        let at = i64::from(i32_at(s, d + 8));
        let tangents = format != 2;
        let times_at = if format == 4 { (at + nk + 3) & !3 } else { at };
        let values_at = times_at + nk * 4;
        let end = values_at + nk * if tangents { 12 } else { 4 };
        let len = i64::try_from(s.len()).expect("a stream under 2^63 bytes");
        if nk < 1 || at < 0 || end > len {
            return fail(format!("{w}: {nk} keys at {at} run past the {}-byte stream", s.len()));
        }
        let (nk, at, times_at, values_at) = (
            usize::try_from(nk).expect("checked"),
            usize::try_from(at).expect("checked"),
            usize::try_from(times_at).expect("checked"),
            usize::try_from(values_at).expect("checked"),
        );
        let mut keys = Vec::with_capacity(nk);
        for k in 0..nk {
            let mut interp = if format == 2 { INTERP_LINEAR } else { INTERP_CUBIC };
            if format == 4 {
                let m = s[at + k];
                if m == 5 {
                    return fail(format!("{w}: key {k} is weighted (its evaluator was not read)"));
                }
                interp = match m {
                    1 => INTERP_CONSTANT,
                    2 => INTERP_LINEAR,
                    _ => INTERP_CUBIC,
                };
            }
            let v = values_at + k * if tangents { 12 } else { 4 };
            keys.push(AnimCurveKey {
                time: f32_at(s, times_at + k * 4),
                value: f32_at(s, v),
                arrive: if tangents { f32_at(s, v + 4) } else { 0.0 },
                leave: if tangents { f32_at(s, v + 8) } else { 0.0 },
                interp,
            });
        }
        curves.push(AnimCurve { name: name.clone(), keys });
    }
    Ok(curves)
}
