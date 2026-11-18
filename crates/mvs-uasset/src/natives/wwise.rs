//! The Wwise integration's native classes (CUE4Parse 1.2.2 UE4/Wwise assets): what follows their tagged properties.
//! UAkAudioEvent, UAkInitBank and UAkAuxBus keep UObject::Serialize (no tail);
//! the rest:
//! - media: UAkMediaAsset (AkMediaAsset, AkLocalizedMediaAsset, AkExternalMediaAsset) its FPackageIndex
//!   CurrentMediaAssetData; UAkMediaAssetData its FAkMediaDataChunk array {bool IsPrefetch, FByteBulkData};
//! - banks: UAkAssetData (AkAudioEventData, AkInitBankAssetData, AkAssetDataWithMedia, AkAssetDataSwitchContainer)
//!   the FByteBulkData holding its bank, always (14090cc80); UAkAssetPlatformData its FPackageIndex
//!   CurrentAssetData.
//!
//! What they keep in `UObj::data`: UAkMediaAsset `CurrentMediaAssetDataIndex` (the raw index) and
//! `CurrentMediaAssetData` (resolved); UAkMediaAssetData `DataChunks`, one struct a chunk of its bulk header and
//! payload place (read back by [`media_chunks`]); UAkAssetData `Data` (the bank's tree, null when the bulk holds
//! nothing) and `Hidden` (what the tree leaves out, one entry per hierarchy item; read back by [`bank_hidden`],
//! not written to the object kind's JSON); UAkAssetPlatformData `CurrentAssetDataIndex` (not written: its class has no WriteJson).

use super::Native;
use super::wwise_bank::{ItemHidden, read_bank_hidden};
use crate::archive::{ReadError, Result};
use crate::bulk::{BulkHeader, UNUSED, bulk_json, read_bulk_payload};
use crate::objects::{ExportArchive, UObj, super_deserialize};
use crate::value::{Props, Value};

/// By C# class name.
pub static NATIVES: &[(&str, Native)] = &[
    ("UAkMediaAsset", Native { deserialize: Some(media_asset_deserialize), json: Some(media_asset_json), extra: None }),
    (
        "UAkMediaAssetData",
        Native { deserialize: Some(media_data_deserialize), json: Some(media_data_json), extra: None },
    ),
    ("UAkAssetData", Native { deserialize: Some(asset_data_deserialize), json: Some(asset_data_json), extra: None }),
    ("UAkAssetPlatformData", Native { deserialize: Some(platform_data_deserialize), json: None, extra: None }),
    // UAkAudioEventData reads only tagged properties after its base; the rest are UObject::Serialize alone
    ("UAkAudioEventData", Native { deserialize: None, json: None, extra: None }),
    ("UAkAudioEvent", Native { deserialize: None, json: None, extra: None }),
    ("UAkInitBank", Native { deserialize: None, json: None, extra: None }),
    ("UAkAuxBus", Native { deserialize: None, json: None, extra: None }),
];

/// One FAkMediaDataChunk as read: its bulk header and, for an inline payload, its place in the export data
/// (`BulkRead::inline_at`). What the sound kind reads the media's bytes by.
#[derive(Clone, Copy, Debug)]
pub struct MediaChunk {
    pub is_prefetch: bool,
    pub header: BulkHeader,
    pub inline_at: Option<usize>,
}

fn chunk_value(c: &MediaChunk) -> Value {
    let mut p = Props::new();
    p.insert("IsPrefetch".into(), Value::Bool(c.is_prefetch));
    p.insert("Flags".into(), Value::Int(c.header.flags.into()));
    p.insert("ElementCount".into(), Value::Int(c.header.element_count.into()));
    p.insert("SizeOnDisk".into(), Value::Int(c.header.size_on_disk.into()));
    p.insert("OffsetInFile".into(), Value::Int(c.header.offset_in_file));
    p.insert("InlineAt".into(), c.inline_at.map_or(Value::Null, |a| Value::Int(a as i64)));
    Value::Struct(p)
}

fn chunk_of(v: &Value) -> Option<MediaChunk> {
    let p = v.as_struct()?;
    let get = |k: &str| p.get(k).and_then(Value::as_i64);
    Some(MediaChunk {
        is_prefetch: p.get("IsPrefetch")?.as_bool()?,
        header: BulkHeader {
            flags: u32::try_from(get("Flags")?).ok()?,
            element_count: i32::try_from(get("ElementCount")?).ok()?,
            size_on_disk: u32::try_from(get("SizeOnDisk")?).ok()?,
            offset_in_file: get("OffsetInFile")?,
        },
        inline_at: get("InlineAt").and_then(|a| usize::try_from(a).ok()),
    })
}

/// A UAkMediaAssetData's chunks as read (`None`: not one, or its data was not reached).
pub fn media_chunks(obj: &UObj) -> Option<Vec<MediaChunk>> {
    match obj.data.get("DataChunks")? {
        Value::Array(a) => a.iter().map(chunk_of).collect(),
        _ => None,
    }
}

fn media_asset_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    let i = ar.i32()?;
    obj.data.insert("CurrentMediaAssetDataIndex".into(), Value::Int(i.into()));
    let r = ar.cache.try_resolve(ar.pkg, i)?;
    obj.data.insert("CurrentMediaAssetData".into(), Value::Ref(r));
    Ok(())
}

fn media_asset_json(obj: &UObj, o: &mut Props) {
    if let Some(c @ Value::Ref(Some(_))) = obj.data.get("CurrentMediaAssetData") {
        o.insert("CurrentMediaAssetData".into(), c.clone());
    }
}

fn media_data_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    let n = ar.i32()?;
    let mut chunks = Vec::new();
    for _ in 0..n {
        let is_prefetch = ar.bool()?;
        let read = read_bulk_payload(ar)?;
        chunks.push(chunk_value(&MediaChunk { is_prefetch, header: read.header, inline_at: read.inline_at }));
    }
    obj.data.insert("DataChunks".into(), Value::Array(chunks));
    Ok(())
}

fn media_data_json(obj: &UObj, o: &mut Props) {
    let chunks = media_chunks(obj);
    // C#'s WriteJson writes the array whatever was read: an empty one when the tail was not reached
    if obj.partial.is_some() && chunks.is_none() {
        return;
    }
    let out = chunks
        .unwrap_or_default()
        .iter()
        .map(|c| {
            let mut p = Props::new();
            p.insert("BulkData".into(), Value::Struct(bulk_json(&c.header)));
            p.insert("IsPrefetch".into(), Value::Bool(c.is_prefetch));
            Value::Struct(p)
        })
        .collect();
    o.insert("DataChunks".into(), Value::Array(out));
}

/// UAkAssetData: the bulk holding its bank and the bank itself (`read_bank`). Every bank of the build is inline; one
/// in the .ubulk or at the end of the file is not ported (a native reads no other file).
fn asset_data_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    if ar.pos >= valid_pos {
        return Ok(());
    }
    let read = read_bulk_payload(ar)?;
    let h = read.header;
    if h.element_count <= 0 || h.flags & UNUSED != 0 {
        obj.data.insert("Data".into(), Value::Null);
        return Ok(());
    }
    let Some(at) = read.inline_at else {
        return Err(ReadError::not_ported(format!(
            "{}: an AkAssetData bank outside the export (bulk flags {:x})",
            ar.pkg.path, h.flags
        )));
    };
    if i64::from(h.element_count) != i64::from(h.size_on_disk) {
        return Err(ReadError::not_ported(format!("{}: a compressed AkAssetData bank", ar.pkg.path)));
    }
    let bytes = &ar.ar.buf[at..at + h.size_on_disk as usize];
    let (data, hidden) = read_bank_hidden(bytes, &format!("{}:{}", ar.pkg.path, obj.name))?;
    obj.data.insert("Data".into(), data);
    obj.data.insert("Hidden".into(), Value::Array(hidden.iter().map(hidden_value).collect()));
    Ok(())
}

fn hidden_value(h: &ItemHidden) -> Value {
    if h.is_empty() {
        return Value::Null;
    }
    let mut p = Props::new();
    if let Some(a) = h.att {
        p.insert("Att".into(), Value::UInt(a.into()));
    }
    if let Some((bits, b3d)) = h.pos {
        p.insert("Pos".into(), Value::Array(vec![Value::UInt(bits.into()), Value::UInt(b3d.into())]));
    }
    if let Some(c) = h.curve_to_use {
        p.insert("CurveToUse".into(), Value::Array(c.iter().map(|&x| Value::Int(x.into())).collect()));
    }
    if !h.rs_type.is_empty() {
        p.insert("RSType".into(), Value::Array(h.rs_type.iter().map(|&x| Value::Int(x.into())).collect()));
    }
    Value::Struct(p)
}

fn hidden_of(v: &Value) -> Option<ItemHidden> {
    let Value::Struct(p) = v else { return Some(ItemHidden::default()) };
    let ints = |k: &str| -> Option<Vec<i64>> {
        match p.get(k) {
            Some(Value::Array(a)) => a.iter().map(Value::as_i64).collect(),
            _ => Some(Vec::new()),
        }
    };
    let pos = ints("Pos")?;
    let curve = ints("CurveToUse")?;
    Some(ItemHidden {
        att: p.get("Att").and_then(Value::as_i64).and_then(|a| u32::try_from(a).ok()),
        pos: match pos[..] {
            [a, b] => Some((u8::try_from(a).ok()?, u8::try_from(b).ok()?)),
            _ => None,
        },
        curve_to_use: match curve.len() {
            7 => Some(
                <[i8; 7]>::try_from(curve.iter().map(|&x| i8::try_from(x).ok()).collect::<Option<Vec<_>>>()?).ok()?,
            ),
            _ => None,
        },
        rs_type: ints("RSType")?.into_iter().map(|x| i32::try_from(x).ok()).collect::<Option<_>>()?,
    })
}

/// What a UAkAssetData's bank holds beside its tree, per hierarchy item (`None`: not read, or not one).
pub fn bank_hidden(obj: &UObj) -> Option<Vec<ItemHidden>> {
    match obj.data.get("Hidden")? {
        Value::Array(a) => a.iter().map(hidden_of).collect(),
        _ => None,
    }
}

fn asset_data_json(obj: &UObj, o: &mut Props) {
    if let Some(d) = obj.data.get("Data")
        && !matches!(d, Value::Null)
    {
        o.insert("Data".into(), d.clone());
    }
}

fn platform_data_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    let i = ar.i32()?;
    obj.data.insert("CurrentAssetDataIndex".into(), Value::Int(i.into()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_round_trips() {
        let c = MediaChunk {
            is_prefetch: true,
            header: BulkHeader { flags: 0x10501, element_count: 5590, size_on_disk: 5590, offset_in_file: 0x1234 },
            inline_at: Some(77),
        };
        let back = chunk_of(&chunk_value(&c)).unwrap();
        assert!(back.is_prefetch);
        assert_eq!(back.header.flags, 0x10501);
        assert_eq!(back.header.offset_in_file, 0x1234);
        assert_eq!(back.inline_at, Some(77));
        let c = MediaChunk { inline_at: None, ..c };
        assert_eq!(chunk_of(&chunk_value(&c)).unwrap().inline_at, None);
    }
}
