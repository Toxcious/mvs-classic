//! Textures: UTexture (its WriteJson writes the platform data), UTexture2D's and UTextureCube's cooked platform data
//! and ULightMapTexture2D's flags (CUE4Parse 1.2.2 UTexture / UTexture2D / UTextureCube / ULightMapTexture2D /
//! FTexturePlatformData / FTexture2DMipMap).
//!
//! Each mip's payload place is kept beside the JSON ([`texture_platform`]): an inline payload by its position in the
//! package's export data, a .ubulk one by its OffsetInFile; [`mip_bytes`] slices it (the texture kind).
//!
//! What a texture keeps in `UObj::data`: `Format` (the first cooked pixel format's name), `PlatformData` (a struct of
//! the fields UTexture's WriteJson writes: SizeX, SizeY, PackedData, OptData, FirstMipToSerialize, Mips) and
//! `MipPlaces` (one [`MipPlace`] struct a mip, or null; not written).

use std::borrow::Cow;

use mvs_pak::{BuildPaths, PakSet};

use super::Native;
use crate::archive::{ReadError, Result};
use crate::bulk::{PAYLOAD_IN_SEPERATE_FILE, bulk_json, read_bulk_payload};
use crate::objects::{ExportArchive, UObj, super_deserialize};
use crate::package::Package;
use crate::value::{Props, Value};

/// By C# class name.
pub static NATIVES: &[(&str, Native)] = &[
    ("UTexture", Native { deserialize: Some(texture_deserialize), json: Some(texture_json), extra: None }),
    ("UTexture2D", Native { deserialize: Some(deserialize_cooked), json: None, extra: None }),
    // the same cooked platform data, PackedData's bit 31 set and its slice count 6: each mip's bulk holds the six
    // faces back to back
    ("UTextureCube", Native { deserialize: Some(deserialize_cooked), json: None, extra: None }),
    ("ULightMapTexture2D", Native { deserialize: Some(lightmap_deserialize), json: Some(lightmap_json), extra: None }),
];

/// Which file a mip's bytes are in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MipFile {
    /// The package's export data (`Package::uexp`).
    Uexp,
    /// The package's .ubulk.
    Ubulk,
}

impl MipFile {
    fn ext(self) -> &'static str {
        match self {
            MipFile::Uexp => "uexp",
            MipFile::Ubulk => "ubulk",
        }
    }
}

/// Where a mip's bytes are: inline in the package's export data (`at` a position in `Package::uexp`) or in its
/// .ubulk (`at` OffsetInFile). `size` is the bulk's SizeOnDisk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MipPlace {
    pub width: i32,
    pub height: i32,
    pub depth: i32,
    pub size: u32,
    pub flags: u32,
    pub file: MipFile,
    pub at: i64,
}

/// A texture's cooked platform data, typed: what the texture kind writes.
#[derive(Clone, Debug)]
pub struct TexturePlatform {
    /// The build's EPixelFormat name (`PF_DXT1`).
    pub pixel_format: String,
    pub width: i32,
    pub height: i32,
    /// PackedData bit 31.
    pub cube: bool,
    /// PackedData's slice count (6 for a cube, 1 else).
    pub slices: u32,
    /// Largest first, as cooked; a mip without a payload place (none in this build) fails [`mip_bytes`].
    pub mips: Vec<Option<MipPlace>>,
}

fn int(v: i64) -> Value {
    Value::Int(v)
}

fn place_value(m: &MipPlace) -> Value {
    let mut p = Props::new();
    p.insert("width".into(), int(m.width.into()));
    p.insert("height".into(), int(m.height.into()));
    p.insert("depth".into(), int(m.depth.into()));
    p.insert("size".into(), int(m.size.into()));
    p.insert("flags".into(), int(m.flags.into()));
    p.insert("file".into(), Value::Str(m.file.ext().into()));
    p.insert("at".into(), int(m.at));
    Value::Struct(p)
}

fn place_of(v: &Value) -> Option<MipPlace> {
    let p = v.as_struct()?;
    let get = |k: &str| p.get(k).and_then(Value::as_i64);
    let file = match p.get("file")?.as_str()? {
        "uexp" => MipFile::Uexp,
        _ => MipFile::Ubulk,
    };
    Some(MipPlace {
        width: i32::try_from(get("width")?).ok()?,
        height: i32::try_from(get("height")?).ok()?,
        depth: i32::try_from(get("depth")?).ok()?,
        size: u32::try_from(get("size")?).ok()?,
        flags: u32::try_from(get("flags")?).ok()?,
        file,
        at: get("at")?,
    })
}

/// FTexture2DMipMap: its JSON and its payload place.
fn read_mip(ar: &mut ExportArchive<'_>) -> Result<(Value, Option<MipPlace>)> {
    let cooked = ar.bool()?;
    let read = read_bulk_payload(ar)?;
    let header = read.header;
    let size_x = ar.i32()?;
    let size_y = ar.i32()?;
    let size_z = ar.i32()?;
    if !cooked {
        ar.u8()?;
        ar.fstring()?;
    }
    let place = |file, at| MipPlace {
        width: size_x,
        height: size_y,
        depth: size_z,
        size: header.size_on_disk,
        flags: header.flags,
        file,
        at,
    };
    let place = if let Some(at) = read.inline_at {
        Some(place(MipFile::Uexp, at as i64))
    } else if header.flags & PAYLOAD_IN_SEPERATE_FILE != 0 {
        Some(place(MipFile::Ubulk, header.offset_in_file))
    } else {
        None // BULKDATA_PayloadAtEndOfFile alone: none in this build
    };
    let mut json = Props::new();
    json.insert("BulkData".into(), Value::Struct(bulk_json(&header)));
    json.insert("SizeX".into(), int(size_x.into()));
    json.insert("SizeY".into(), int(size_y.into()));
    json.insert("SizeZ".into(), int(size_z.into()));
    Ok((Value::Struct(json), place))
}

/// FTexturePlatformData: the struct UTexture's WriteJson writes from, and the mips' places.
fn read_platform_data(ar: &mut ExportArchive<'_>) -> Result<(Props, Vec<Value>)> {
    let mut size_x = ar.i32()?;
    let mut size_y = ar.i32()?;
    let packed = ar.u32()?;
    ar.fstring()?; // PixelFormat
    let mut opt = None;
    if packed & 0x4000_0000 != 0 {
        opt = Some((ar.u32()?, ar.u32()?));
    }
    if packed & 0x2000_0000 != 0 {
        return Err(ReadError::not_ported("FTexturePlatformData CPU copy not ported"));
    }
    let first_mip = ar.i32()?;
    let n = ar.i32()?;
    let mut mips = Vec::new();
    let mut places = Vec::new();
    for _ in 0..n {
        let (json, place) = read_mip(ar)?;
        mips.push(json);
        places.push(place.as_ref().map_or(Value::Null, place_value));
    }
    // Versions["VirtualTextures"] (UE4.23+): a bool, then FVirtualTextureBuiltData
    if ar.bool()? {
        return Err(ReadError::not_ported("FVirtualTextureBuiltData not ported"));
    }
    if let Some(Value::Struct(m)) = mips.first() {
        size_x = m["SizeX"].as_i64().and_then(|v| i32::try_from(v).ok()).unwrap_or(size_x);
        size_y = m["SizeY"].as_i64().and_then(|v| i32::try_from(v).ok()).unwrap_or(size_y);
    }
    let mut p = Props::new();
    p.insert("SizeX".into(), int(size_x.into()));
    p.insert("SizeY".into(), int(size_y.into()));
    p.insert("PackedData".into(), int(packed.into()));
    if let Some((ext, tail)) = opt {
        let mut o = Props::new();
        o.insert("ExtData".into(), int(ext.into()));
        o.insert("NumMipsInTail".into(), int(tail.into()));
        p.insert("OptData".into(), Value::Struct(o));
    }
    p.insert("FirstMipToSerialize".into(), int(first_mip.into()));
    p.insert("Mips".into(), Value::Array(mips));
    Ok((p, places))
}

/// UTexture.Deserialize: the strip flags; a texture with source art (not cooked) is not ported.
fn texture_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    let global = ar.u8()?;
    ar.u8()?;
    if global & 1 == 0 {
        return Err(ReadError::not_ported("texture source art not ported"));
    }
    Ok(())
}

/// UTexture2D.Deserialize / UTextureCube.Deserialize: the strip flags, bCooked, then the cooked platform data (one
/// entry per pixel format, the first one read).
fn deserialize_cooked(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    ar.u8()?;
    ar.u8()?; // FStripDataFlags
    let cooked = ar.bool()?;
    if !cooked || ar.pos >= valid_pos {
        return Ok(());
    }
    loop {
        let fmt = ar.fname()?;
        if fmt == "None" {
            break;
        }
        // an absolute offset in the package
        let skip_to = ar.i64()? - ar.pkg.uasset_len as i64;
        let skip_to = usize::try_from(skip_to)
            .map_err(|_| ReadError::parse(format!("{}: platform data ends at {skip_to}", ar.name)))?;
        if !obj.data.contains_key("Format") {
            let (data, places) = read_platform_data(ar)?;
            obj.data.insert("PlatformData".into(), Value::Struct(data));
            obj.data.insert("MipPlaces".into(), Value::Array(places));
            obj.data.insert("Format".into(), Value::Str(fmt));
        }
        ar.pos = skip_to;
    }
    Ok(())
}

fn texture_json(obj: &UObj, o: &mut Props) {
    let p = obj.data.get("PlatformData").and_then(Value::as_struct);
    // its fields are the platform data a class above it reads (UTexture2D, UTextureCube): on a partial object whose
    // class with that data is not ported (a UVolumeTexture ...) nothing was read, so nothing is written; a whole
    // object writes CUE4Parse's defaults
    if obj.partial.is_some() && p.is_none() {
        return;
    }
    let field = |k: &str, d: i64| p.and_then(|p| p.get(k)).cloned().unwrap_or(Value::Int(d));
    o.insert("SizeX".into(), field("SizeX", 0));
    o.insert("SizeY".into(), field("SizeY", 0));
    o.insert("PackedData".into(), field("PackedData", 0));
    let format = obj.data.get("Format").and_then(Value::as_str).unwrap_or("PF_Unknown");
    o.insert("PixelFormat".into(), Value::Str(format.into()));
    if let Some(opt) = p.and_then(|p| p.get("OptData")).and_then(Value::as_struct) {
        let nonzero = |k: &str| opt.get(k).and_then(Value::as_i64).is_some_and(|v| v != 0);
        if nonzero("ExtData") && nonzero("NumMipsInTail") {
            o.insert("OptData".into(), Value::Struct(opt.clone()));
        }
    }
    o.insert("FirstMipToSerialize".into(), field("FirstMipToSerialize", -1));
    if let Some(Value::Array(m)) = p.and_then(|p| p.get("Mips"))
        && !m.is_empty()
    {
        o.insert("Mips".into(), Value::Array(m.clone()));
    }
}

fn lightmap_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    let f = ar.u32()?;
    obj.data.insert("LightmapFlags".into(), Value::Int(f.into()));
    Ok(())
}

/// ELightMapFlags as CUE4Parse names them.
fn lightmap_flags_text(f: i64) -> Value {
    match f {
        0 => Value::Str("LMF_None".into()),
        1 => Value::Str("LMF_Streamed".into()),
        _ => Value::Int(f),
    }
}

fn lightmap_json(obj: &UObj, o: &mut Props) {
    if let Some(f) = obj.data.get("LightmapFlags").and_then(Value::as_i64) {
        o.insert("LightmapFlags".into(), lightmap_flags_text(f));
    }
}

/// The platform data an object read (`None`: none, a class without it or a texture cooked without it).
pub fn texture_platform(obj: &UObj) -> Option<TexturePlatform> {
    let p = obj.data.get("PlatformData")?.as_struct()?;
    let pixel_format = obj.data.get("Format")?.as_str()?.to_owned();
    let get = |k: &str| p.get(k).and_then(Value::as_i64);
    let packed = u32::try_from(get("PackedData")?).ok()?;
    let mips = match obj.data.get("MipPlaces") {
        Some(Value::Array(a)) => a.iter().map(place_of).collect(),
        _ => Vec::new(),
    };
    Some(TexturePlatform {
        pixel_format,
        width: i32::try_from(get("SizeX")?).ok()?,
        height: i32::try_from(get("SizeY")?).ok()?,
        cube: packed & 0x8000_0000 != 0,
        slices: packed & 0x1fff_ffff,
        mips,
    })
}

/// A mip's bytes: a slice of the package's export data, or of its .ubulk (`ubulk`: the caller's read of it with
/// offsets from the file's start, else the mip's own span is read here from the paks, as [`read_bulk_range`] does, not
/// the whole file). Fails naming the mip when its place lies outside the file.
pub fn mip_bytes<'a>(paks: &PakSet, pkg: &'a Package, m: &MipPlace, ubulk: Option<&'a [u8]>) -> Result<Cow<'a, [u8]>> {
    match m.file {
        MipFile::Uexp => slice_mip(m, Some(pkg.uexp()?), &pkg.path).map(Cow::Borrowed),
        MipFile::Ubulk => match ubulk {
            Some(b) => slice_mip(m, Some(b), &pkg.path).map(Cow::Borrowed),
            None => {
                let file = format!("{}.ubulk", pkg.path);
                let Some(e) = paks.entry(&file) else { return slice_mip(m, None, &pkg.path).map(Cow::Borrowed) };
                let at = u64::try_from(m.at).ok().filter(|at| at.saturating_add(m.size.into()) <= e.uncompressed_size);
                let Some(at) = at else { return Err(outside(m, &pkg.path, e.uncompressed_size)) };
                let span = paks.read_range(&e.path, at, m.size.into())?;
                Ok(Cow::Owned(span))
            }
        },
    }
}

/// `len` bytes of a package's .ubulk from `offset`, through the pak blocks they lie in (`PakSet::read_range`):
/// the span of one texture's mips. `None` when the paks hold no .ubulk of the package;
/// a span outside the file is an error.
pub fn read_bulk_range(
    paks: &PakSet,
    paths: &BuildPaths,
    object_path: &str,
    offset: u64,
    len: u64,
) -> Result<Option<Vec<u8>>> {
    let Some(file) = paths.package_file(object_path) else { return Ok(None) };
    let Some(e) = paks.entry(&format!("{file}.ubulk")) else { return Ok(None) };
    if offset.checked_add(len).is_none_or(|end| end > e.uncompressed_size) {
        return Err(ReadError::parse(format!(
            "{}: bytes {offset} + {len} lie outside it ({} bytes)",
            e.path, e.uncompressed_size
        )));
    }
    Ok(Some(paks.read_range(&e.path, offset, len)?))
}

/// The mip's bytes in the file it is in (`src`: the export data or the .ubulk, as `m.file` says; `None` when that
/// file was not read or is missing); `name` for the message.
pub fn slice_mip<'a>(m: &MipPlace, src: Option<&'a [u8]>, name: &str) -> Result<&'a [u8]> {
    let ext = m.file.ext();
    let Some(src) = src else {
        return Err(ReadError::parse(format!(
            "{name}: a {} x {} mip is in the .{ext}, which was not read",
            m.width, m.height
        )));
    };
    let end = m.at.checked_add(i64::from(m.size));
    match (usize::try_from(m.at), end.and_then(|e| usize::try_from(e).ok())) {
        (Ok(at), Some(end)) if end <= src.len() => Ok(&src[at..end]),
        _ => Err(outside(m, name, src.len() as u64)),
    }
}

/// The error of a mip whose place lies outside its file of `len` bytes.
fn outside(m: &MipPlace, name: &str, len: u64) -> ReadError {
    ReadError::parse(format!(
        "{name}: a {} x {} mip at {} + {} lies outside its .{} ({len} bytes)",
        m.width,
        m.height,
        m.at,
        m.size,
        m.file.ext()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mip(file: MipFile, at: i64, size: u32) -> MipPlace {
        MipPlace { width: 4, height: 4, depth: 1, size, flags: 0, file, at }
    }

    #[test]
    fn slice_mip_bounds() {
        let src = [0u8, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(slice_mip(&mip(MipFile::Ubulk, 2, 4), Some(&src), "p").unwrap(), &[2, 3, 4, 5]);
        assert_eq!(slice_mip(&mip(MipFile::Ubulk, 0, 8), Some(&src), "p").unwrap().len(), 8);
        assert!(slice_mip(&mip(MipFile::Ubulk, 5, 4), Some(&src), "p").is_err());
        assert!(slice_mip(&mip(MipFile::Ubulk, -1, 4), Some(&src), "p").is_err());
        assert!(slice_mip(&mip(MipFile::Uexp, 0, 4), None, "p").is_err());
    }

    #[test]
    fn place_round_trips() {
        let m = MipPlace {
            width: 512,
            height: 256,
            depth: 1,
            size: 131072,
            flags: 0x10101,
            file: MipFile::Ubulk,
            at: 0x20000,
        };
        assert_eq!(place_of(&place_value(&m)), Some(m));
        let m = MipPlace { file: MipFile::Uexp, ..m };
        assert_eq!(place_of(&place_value(&m)), Some(m));
    }

    #[test]
    fn lightmap_flag_names() {
        assert!(matches!(lightmap_flags_text(0), Value::Str(s) if s == "LMF_None"));
        assert!(matches!(lightmap_flags_text(1), Value::Str(s) if s == "LMF_Streamed"));
        assert!(matches!(lightmap_flags_text(3), Value::Int(3)));
    }
}
