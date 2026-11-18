//! UAnimSequence's native tail for the animation kind: the compressed track table, the curve names, the compressed
//! data description and its byte streams. Never entered in NATIVES: the object kind keeps the class `partial`. It
//! follows CUE4Parse 1.2.2 UAnimSequence.Deserialize and SerializeCompressedData3, FUECompressedAnimData, the UE 4.26
//! cooked branch only. The skeleton part of the animation natives is `super::skeleton`. The byte streams borrow the
//! package's export data.

use std::sync::Arc;

use super::skeleton::{at_native_data, ends_at};
use crate::archive::{ReadError, Result};
use crate::objects::ExportArchive;
use crate::package::{Package, PackageCache};
use crate::value::Value;

/// AnimationKeyFormat as the build names it.
const KEY_FORMATS: [&str; 3] = ["AKF_ConstantKeyLerp", "AKF_VariableKeyLerp", "AKF_PerTrackCompression"];
/// AnimationCompressionFormat as the build names it.
const COMPRESSION_FORMATS: [&str; 7] = [
    "ACF_None",
    "ACF_Float96NoW",
    "ACF_Fixed48NoW",
    "ACF_IntervalFixed32NoW",
    "ACF_Fixed32NoW",
    "ACF_Float32NoW",
    "ACF_Identity",
];

/// The compressed data description (absent when the bone codec handle is empty).
#[derive(Clone, Debug, Default)]
pub struct CompressedDataStructure<'a> {
    pub compressed_number_of_frames: i32,
    /// AnimationKeyFormat as the build names it (`AKF_ConstantKeyLerp`, ...).
    pub key_encoding_format: &'static str,
    /// AnimationCompressionFormat as the build names it (`ACF_None`, ...).
    pub translation_compression_format: &'static str,
    pub rotation_compression_format: &'static str,
    pub scale_compression_format: &'static str,
    /// Per track the offset (and, for the lerp formats, the key count) of its translation and rotation, as the
    /// format lays them out (4 int32 a track for the key-lerp formats, 2 for per-track).
    pub compressed_track_offsets: Vec<i32>,
    /// The scale offsets (strided as the format says), empty when the sequence stores no scale.
    pub compressed_scale_offsets: Vec<i32>,
    pub compressed_scale_strip_size: i32,
    pub compressed_byte_stream: &'a [u8],
}

/// What the tail holds after the tagged properties, by CUE4Parse's field names.
#[derive(Clone, Debug, Default)]
pub struct AnimSequenceTail<'a> {
    pub skeleton_guid: String,
    pub compressed_raw_data_size: i32,
    /// Per compressed track, an index into the Skeleton's reference skeleton.
    pub compressed_track_to_skeleton_map_table: Vec<i32>,
    pub compressed_curve_names: Vec<String>,
    /// Absent when the bone codec handle is empty: no codec, so no description is serialized (the three curve-only
    /// RickTeaser face sequences, whose bone byte stream is empty).
    pub compressed_data_structure: Option<CompressedDataStructure<'a>>,
    pub bone_codec_ddc_handle: String,
    pub curve_codec_path: String,
    /// The CompressedRichCurve stream as the build stores it (decoded at import).
    pub compressed_curve_byte_stream: &'a [u8],
}

fn compression_format(ar: &mut ExportArchive<'_>) -> Result<&'static str> {
    let v = ar.u8()?;
    match COMPRESSION_FORMATS.get(usize::from(v)) {
        Some(f) => Ok(f),
        None => Err(ReadError::parse(format!("{}: AnimationCompressionFormat {v} at {}", ar.name, ar.pos - 1))),
    }
}

fn i32_array(ar: &mut ExportArchive<'_>, n: i32) -> Result<Vec<i32>> {
    let Ok(n) = usize::try_from(n) else {
        return Err(ReadError::parse(format!("{}: array length {n} at {}", ar.name, ar.pos)));
    };
    ar.need(n * 4)?;
    (0..n).map(|_| ar.i32()).collect()
}

/// The tail of an AnimSequence export (UAnimSequence.Deserialize after its base classes, the 4.26 branch:
/// FFrameworkObjectVersion past MoveCompressedAnimDataToTheDDC, SerializeCompressedData3). A sequence whose bone
/// codec handle is empty (CUE4Parse "Unknown bone compression codec") has no compressed data description: nothing is
/// serialized after its curves, and it may hold no bone bytes. A cooked sequence that kept its raw data or one that
/// stores no compressed data fails.
pub fn read_anim_sequence_tail<'a>(
    cache: &'a PackageCache,
    pkg: &'a Arc<Package>,
    index: usize,
) -> Result<AnimSequenceTail<'a>> {
    let d = at_native_data(cache, pkg, index, "UAnimSequence")?;
    let (mut ar, end) = (d.ar, d.end);
    let skeleton_guid = match d.obj.data.get("SkeletonGuid") {
        Some(Value::Str(s)) => s.clone(),
        other => return Err(ReadError::parse(format!("{}: SkeletonGuid is {other:?}", pkg.path))),
    };
    // FStripDataFlags: global, class
    let global_strip = ar.u8()?;
    ar.u8()?;
    if global_strip & 1 == 0 {
        return Err(ReadError::not_ported(format!(
            "{}: UAnimSequence with editor data (raw tracks) not ported",
            pkg.path
        )));
    }
    if !ar.bool()? {
        return Err(ReadError::not_ported(format!(
            "{}: UAnimSequence with no compressed data (bSerializeCompressedData false) not ported",
            pkg.path
        )));
    }
    // SerializeCompressedData3 (FCompressedAnimSequence::SerializeCompressedData)
    let compressed_raw_data_size = ar.i32()?;
    let n = ar.i32()?;
    let compressed_track_to_skeleton_map_table = i32_array(&mut ar, n)?;
    let n_curves = ar.i32()?;
    let Ok(n_curves) = usize::try_from(n_curves) else {
        return Err(ReadError::parse(format!("{}: curve name count {n_curves}", pkg.path)));
    };
    // FSmartName: the DisplayName alone (FAnimPhysObjectVersion past RemoveUIDFromSmartNameSerialize; cooked, so no
    // GUID)
    let mut compressed_curve_names = Vec::new();
    for _ in 0..n_curves {
        compressed_curve_names.push(ar.fname()?);
    }
    // ReadSerializedByteStream
    let bulk_bytes = ar.i32()?;
    if ar.bool()? {
        return Err(ReadError::not_ported(format!("{}: UAnimSequence byte stream in bulk data not ported", pkg.path)));
    }
    let bulk_at = ar.pos;
    let Ok(bulk_len) = usize::try_from(bulk_bytes) else {
        return Err(ReadError::parse(format!("{}: byte stream length {bulk_bytes}", ar.name)));
    };
    ar.skip(bulk_len)?;
    let bone_codec_ddc_handle = ar.fstring()?;
    let curve_codec_path = ar.fstring()?;
    let curve_bytes = ar.i32()?;
    let Ok(curve_len) = usize::try_from(curve_bytes) else {
        return Err(ReadError::parse(format!("{}: curve stream length {curve_bytes}", ar.name)));
    };
    let compressed_curve_byte_stream = ar.bytes(curve_len)?;
    // UAnimBoneCompressionSettings.GetCodec(BoneCodecDDCHandle): every codec of this build's one settings asset
    // (/Engine/Animation/DefaultAnimBoneCompressionSettings) allocates an FUECompressedAnimData; an empty handle (the
    // three RickTeaser face sequences) names none, and then no description is serialized.
    if bone_codec_ddc_handle.is_empty() {
        // bUseRawDataOnly, as below (UAnimSequence's own, after the compressed data)
        ar.skip(4)?;
        ends_at(&ar, end, "the UAnimSequence tail (no bone codec)")?;
        if bulk_bytes != 0 {
            return Err(ReadError::parse(format!(
                "{}: {bulk_bytes} bone bytes with no bone codec (empty BoneCodecDDCHandle)",
                pkg.path
            )));
        }
        return Ok(AnimSequenceTail {
            skeleton_guid,
            compressed_raw_data_size,
            compressed_track_to_skeleton_map_table,
            compressed_curve_names,
            compressed_data_structure: None,
            bone_codec_ddc_handle,
            curve_codec_path,
            compressed_curve_byte_stream,
        });
    }
    // FUECompressedAnimData.SerializeCompressedData, Ar.Game >= UE4_25
    let compressed_number_of_frames = ar.i32()?;
    let kf = ar.u8()?;
    let Some(&key_encoding_format) = KEY_FORMATS.get(usize::from(kf)) else {
        return Err(ReadError::parse(format!("{}: AnimationKeyFormat {kf}", pkg.path)));
    };
    let translation_compression_format = compression_format(&mut ar)?;
    let rotation_compression_format = compression_format(&mut ar)?;
    let scale_compression_format = compression_format(&mut ar)?;
    let n_stream = ar.i32()?;
    let n_track = ar.i32()?;
    let n_scale = ar.i32()?;
    let strip_size = ar.i32()?;
    // bUseRawDataOnly (FFortniteMainBranchObjectVersion before AnimSequenceRawDataOnlyFlagRemoval: all of 4.26)
    ar.skip(4)?;
    ends_at(&ar, end, "the UAnimSequence tail")?;
    let fills = i64::from(n_track) * 4 + i64::from(n_scale) * 4 + i64::from(n_stream) == i64::from(bulk_bytes);
    if n_stream < 0 || n_track < 0 || n_scale < 0 || !fills {
        return Err(ReadError::parse(format!(
            "{}: compressed data views ({n_track} track offsets, {n_scale} scale offsets, {n_stream} stream bytes) do not fill the {bulk_bytes}-byte buffer",
            pkg.path
        )));
    }
    // Bind / InitViewsFromBuffer: the offsets, the scale offsets, the key stream, back to back
    ar.pos = bulk_at;
    let compressed_track_offsets = i32_array(&mut ar, n_track)?;
    let compressed_scale_offsets = i32_array(&mut ar, n_scale)?;
    let compressed_byte_stream = ar.bytes(usize::try_from(n_stream).expect("n_stream checked"))?;
    Ok(AnimSequenceTail {
        skeleton_guid,
        compressed_raw_data_size,
        compressed_track_to_skeleton_map_table,
        compressed_curve_names,
        compressed_data_structure: Some(CompressedDataStructure {
            compressed_number_of_frames,
            key_encoding_format,
            translation_compression_format,
            rotation_compression_format,
            scale_compression_format,
            compressed_track_offsets,
            compressed_scale_offsets,
            compressed_scale_strip_size: strip_size,
            compressed_byte_stream,
        }),
        bone_codec_ddc_handle,
        curve_codec_path,
        compressed_curve_byte_stream,
    })
}
