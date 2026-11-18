//! The `texture` kind: every texture object of the build (top-level, and the inner textures of a level's
//! registry, a world and a font) as an MVTX container: its cooked mips in the build's own GPU
//! formats, largest first, B8G8R8A8 stored as RGBA8. One fails here (WINTER_BANNER_3b_-_OVERLAY_5
//! PF_Unknown); the other, Map01_LionWatch/Sky (PF_BC6H, the build's one HDR cube), is written as its BC6H blocks
//! (format code 9).
//!
//! A mip's bytes are its bulk payload as cooked: inline in the .uexp, or (the build's streamed mips) in the
//! package's .ubulk, the span of the texture's own mips read through the pak blocks it lies in
//! (`natives::texture::read_bulk_range`). `streamed` is how many top mips are in the .ubulk. Each mip's SizeOnDisk is
//! checked against what its format needs for its size (times six faces for a cube).
//!
//! Two ring-out sprites the build ships as B8G8R8A8 with one mip are stored cooked (`cook`): DXT5 at 1024 with a mip
//! chain.

mod cook;

use std::collections::HashSet;
use std::sync::Arc;

use mvs_pak::paths::{join_object_path, split_object_path};
use mvs_uasset::Package;
use mvs_uasset::natives::texture::{MipFile, MipPlace, mip_bytes, read_bulk_range, slice_mip, texture_platform};
use mvs_uasset::value::Value;

use crate::catalog::header_object_path;
use crate::kind::{Cx, Item, Kind, KindError};

/// The kind's version (3, plus one: the cooked sprites, `cook`).
pub const TEXTURE_KIND_VERSION: u64 = 4;
/// The container's magic.
pub const TEXTURE_MAGIC: &[u8; 4] = b"MVTX";
/// The container's own version.
const TEXTURE_FORMAT_VERSION: u16 = 1;

/// The classes the kind writes (the class itself, as the catalog names it).
const TEXTURE_CLASSES: &[&str] = &[
    "/Script/Engine.Texture2D",
    "/Script/Engine.TextureCube",
    "/Script/Engine.CurveLinearColorAtlas",
    "/Script/Engine.LightMapTexture2D",
    "/Script/Engine.ShadowMapTexture2D",
];

/// Top-level classes whose packages hold inner textures: a registry's lightmaps, a level's landscape maps, an
/// offline font's glyph pages.
const HOLDERS: &[&str] = &["/Script/Engine.MapBuildDataRegistry", "/Script/Engine.World", "/Script/Engine.Font"];

/// The entry's format codes, by index, with the build's pixel format each
/// stores (`PIXEL_FORMAT_OF`). B8G8R8A8 is stored as RGBA8.
/// Deviation: BC6H (code 9) is the native port's own code (an earlier import refused it).
const FORMATS: [(&str, &str); 10] = [
    ("PF_DXT1", "DXT1"),
    ("PF_DXT3", "DXT3"),
    ("PF_DXT5", "DXT5"),
    ("PF_BC4", "BC4"),
    ("PF_BC5", "BC5"),
    ("PF_BC7", "BC7"),
    ("PF_G8", "G8"),
    ("PF_B8G8R8A8", "RGBA8"),
    ("PF_FloatRGBA", "FloatRGBA"),
    ("PF_BC6H", "BC6H"),
];

/// The pixel formats stored in 4 x 4 blocks.
const BLOCK_FORMATS: &[&str] = &["PF_DXT1", "PF_DXT3", "PF_DXT5", "PF_BC4", "PF_BC5", "PF_BC6H", "PF_BC7"];

/// `flags` bits.
const TEXTURE_SRGB: u8 = 1;
const TEXTURE_CUBE: u8 = 2;

const HEADER_FIXED: usize = 18;
const HEADER_PER_MIP: usize = 12;

/// Bytes of a header with `mips` mips, padded to 16: the first mip's offset.
pub fn texture_header_size(mips: usize) -> usize {
    (HEADER_FIXED + HEADER_PER_MIP * mips + 15) & !15
}

/// Bytes a mip of `w` x `h` (one face) takes in the build's pixel format.
fn face_bytes(pf: &str, w: i32, h: i32) -> Result<i64, KindError> {
    let (w, h) = (i64::from(w), i64::from(h));
    let blocks = |b: i64| ((w + 3) / 4).max(1) * ((h + 3) / 4).max(1) * b;
    Ok(match pf {
        "PF_DXT1" | "PF_BC4" => blocks(8),
        "PF_DXT3" | "PF_DXT5" | "PF_BC5" | "PF_BC6H" | "PF_BC7" => blocks(16),
        "PF_G8" => w * h,
        "PF_B8G8R8A8" => w * h * 4,
        "PF_FloatRGBA" => w * h * 8,
        _ => return Err(KindError::Failed(format!("texture: no size rule for {pf}"))),
    })
}

/// The inner texture objects of a holder package, from its header alone: their object paths.
fn inner_textures(pkg: &Package, pkg_path: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, e) in pkg.exports.iter().enumerate() {
        if e.outer == 0 {
            continue;
        }
        let class = header_object_path(pkg, e.class_index);
        if !class.as_deref().is_some_and(|c| TEXTURE_CLASSES.contains(&c)) {
            continue;
        }
        let mut names: Vec<&str> = Vec::new();
        let mut j = i;
        for _ in 0..=pkg.exports.len() {
            let x = &pkg.exports[j];
            names.insert(0, &x.object_name);
            if x.outer <= 0 {
                break;
            }
            j = x.outer as usize - 1;
        }
        out.push(join_object_path(pkg_path, &names));
    }
    out
}

/// The export a path names below its top-level export `top`: `B:Inner.Sub` -> the export named Sub whose outer is
/// Inner whose outer is top. `None` when there is none.
fn inner_index(pkg: &Package, top: usize, chain: &[&str]) -> Option<usize> {
    let mut at = top;
    for name in chain {
        let want = name.to_lowercase();
        at = pkg.exports.iter().position(|e| e.outer == (at + 1) as i32 && e.object_name.to_lowercase() == want)?;
    }
    Some(at)
}

/// What the build's texture object gave, the mips with their places.
struct Read {
    format: u8,
    pixel_format: String,
    srgb: bool,
    cube: bool,
    faces: u16,
    mips: Vec<Mip>,
}

struct Mip {
    place: MipPlace,
    width: u16,
    height: u16,
    /// An inline payload (a .ubulk one is read after, in one span).
    bytes: Option<Vec<u8>>,
}

/// The `texture` kind.
pub struct Texture;

impl Kind for Texture {
    fn id(&self) -> &'static str {
        "texture"
    }

    fn version(&self) -> u64 {
        TEXTURE_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/octet-stream"
    }

    /// Every top-level texture of the catalog, then the inner textures of the packages that hold them.
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        let mut out = Vec::new();
        let mut holders: Vec<&str> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for e in cx.catalog.exports() {
            if TEXTURE_CLASSES.contains(&e.class) {
                out.push(Item::new(e.object_path()));
            } else if HOLDERS.contains(&e.class) && seen.insert(e.package) {
                holders.push(e.package);
            }
        }
        for p in holders {
            let Some(pkg) = cx.cache.package(p)? else { continue };
            out.extend(inner_textures(&pkg, p).into_iter().map(Item::new));
        }
        Ok(out)
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let path = &item.path;
        let (pkg_path, object) = split_object_path(path);
        let colon = object.and_then(|o| o.find(':'));
        let top_path = match (object, colon) {
            (Some(o), Some(c)) => format!("{pkg_path}.{}", &o[..c]),
            _ => path.clone(),
        };
        let chain: Vec<&str> = match (object, colon) {
            (Some(o), Some(c)) => o[c + 1..].split('.').collect(),
            _ => Vec::new(),
        };
        let (pkg, top) = cx.read(&top_path)?;
        let mut r = read_texture(cx, &pkg, top, &chain, path)?;

        let mut streamed = 0;
        while streamed < r.mips.len() && r.mips[streamed].place.file == MipFile::Ubulk {
            streamed += 1;
        }
        if r.mips[streamed..].iter().any(|m| m.place.file == MipFile::Ubulk) {
            return Err(KindError::Failed(format!("texture: {path}: a .ubulk mip below an inline one")));
        }
        // the streamed mips' span of the .ubulk (a registry's lightmaps share one file: each reads its own part)
        let mut span: Option<Vec<u8>> = None;
        let mut base = 0;
        if streamed > 0 {
            let top = &r.mips[..streamed];
            base = top.iter().map(|m| m.place.at).min().unwrap_or(0);
            let end = top.iter().map(|m| m.place.at + i64::from(m.place.size)).max().unwrap_or(0);
            let (Ok(off), Ok(len)) = (u64::try_from(base), u64::try_from(end - base)) else {
                return Err(KindError::Failed(format!("texture: {path}: a .ubulk span {base}..{end}")));
            };
            span = read_bulk_range(cx.paks, cx.paths, pkg_path, off, len)?;
            if span.is_none() {
                return Err(KindError::Failed(format!(
                    "texture: {path}: {streamed} mips in a .ubulk the paks do not hold"
                )));
            }
        }

        // the header, then the mips each at a 16-byte boundary
        let mut at = texture_header_size(r.mips.len());
        let mut offsets = Vec::with_capacity(r.mips.len());
        for m in &r.mips {
            offsets.push(at);
            at = (at + m.place.size as usize + 15) & !15;
        }
        let last = r.mips.len() - 1;
        let total = offsets[last] + r.mips[last].place.size as usize;
        let mut out = vec![0u8; total];
        let header = TextureHeader {
            format: r.format,
            srgb: r.srgb,
            cube: r.cube,
            width: r.mips[0].width,
            height: r.mips[0].height,
            depth: r.faces,
            streamed: streamed as u8,
            mips: r
                .mips
                .iter()
                .zip(&offsets)
                .map(|(m, &o)| TextureMip { width: m.width, height: m.height, offset: o as u32, length: m.place.size })
                .collect(),
        };
        write_header(&header, &mut out);
        let swap = r.pixel_format == "PF_B8G8R8A8";
        for (k, m) in r.mips.iter_mut().enumerate() {
            let dst = &mut out[offsets[k]..offsets[k] + m.place.size as usize];
            match m.bytes.take() {
                Some(b) => dst.copy_from_slice(&b),
                None => {
                    let rel = MipPlace { at: m.place.at - base, ..m.place };
                    dst.copy_from_slice(slice_mip(&rel, span.as_deref(), path)?);
                }
            }
            if swap {
                for px in dst.as_chunks_mut::<4>().0 {
                    px.swap(0, 2);
                }
            }
        }
        if cook::COOKED.contains(&path.as_str()) {
            return cooked(&header, &out, path);
        }
        Ok(out)
    }
}

/// The entry of a cooked texture (`cook`) from its uncooked entry `out` (one RGBA8 mip).
fn cooked(h: &TextureHeader, out: &[u8], path: &str) -> Result<Vec<u8>, KindError> {
    if h.format != format_code("RGBA8").unwrap_or(u8::MAX) || h.mips.len() != 1 || h.cube {
        return Err(KindError::Failed(format!("texture: {path}: the cook takes one RGBA8 mip, not {h:?}")));
    }
    let m = h.mips[0];
    let top = &out[m.offset as usize..(m.offset + m.length) as usize];
    let mips = cook::cook_dxt5(top, u32::from(m.width), u32::from(m.height), h.srgb);
    let mut at = texture_header_size(mips.len());
    let mut list = Vec::with_capacity(mips.len());
    for c in &mips {
        let (Ok(width), Ok(height), Ok(length)) =
            (u16::try_from(c.width), u16::try_from(c.height), u32::try_from(c.bytes.len()))
        else {
            return Err(KindError::Failed(format!("texture: {path}: a cooked mip of {} x {}", c.width, c.height)));
        };
        list.push(TextureMip { width, height, offset: at as u32, length });
        at = (at + c.bytes.len() + 15) & !15;
    }
    let last = list[list.len() - 1];
    let mut entry = vec![0u8; (last.offset + last.length) as usize];
    let header = TextureHeader {
        format: format_code("DXT5").unwrap_or(2),
        width: list[0].width,
        height: list[0].height,
        streamed: 0,
        mips: list,
        ..h.clone()
    };
    write_header(&header, &mut entry);
    for (c, m) in mips.iter().zip(&header.mips) {
        entry[m.offset as usize..(m.offset + m.length) as usize].copy_from_slice(&c.bytes);
    }
    Ok(entry)
}

/// The texture export `top` (or the inner object `chain` below it) of `pkg`, checked, with its mips' places and the
/// inline mips' bytes.
fn read_texture(cx: &Cx<'_>, pkg: &Arc<Package>, top: usize, chain: &[&str], path: &str) -> Result<Read, KindError> {
    let i = if chain.is_empty() {
        top
    } else {
        inner_index(pkg, top, chain)
            .ok_or_else(|| KindError::NotFound(format!("texture: {path}: no such inner object")))?
    };
    let obj = cx.cache.object(pkg, i);
    if !obj.chain.contains(&"UTexture") {
        return Err(KindError::NotFound(format!("texture: {path} is a {}, not a texture", obj.type_name)));
    }
    if let Some(p) = obj.partial {
        return Err(KindError::Failed(format!(
            "texture: {path}: a {}, whose native class {p} the reader does not port",
            obj.type_name
        )));
    }
    if !obj.issues.is_empty() {
        let list: Vec<&str> = obj.issues.iter().map(|e| e.message.as_str()).collect();
        return Err(KindError::Failed(format!("texture: {path}: {}", list.join("; "))));
    }
    let Some(plat) = texture_platform(&obj) else {
        return Err(KindError::Failed(format!("texture: {path}: a {} with no cooked platform data", obj.type_name)));
    };
    let Some(code) = FORMATS.iter().position(|(pf, _)| *pf == plat.pixel_format) else {
        return Err(KindError::Failed(format!(
            "texture: {path}: pixel format {} has no entry format",
            plat.pixel_format
        )));
    };
    if plat.mips.is_empty() {
        return Err(KindError::Failed(format!("texture: {path}: no mips")));
    }
    let faces: u32 = if plat.cube { 6 } else { 1 };
    let is_cube = obj.chain.contains(&"UTextureCube");
    if plat.cube != is_cube {
        return Err(KindError::Failed(format!(
            "texture: {path}: a {} with the cube flag {}",
            obj.type_name, plat.cube
        )));
    }
    if plat.slices != faces {
        return Err(KindError::Failed(format!("texture: {path}: {} slices", plat.slices)));
    }
    let block = BLOCK_FORMATS.contains(&plat.pixel_format.as_str());
    let Some(top0) = plat.mips[0] else {
        return Err(KindError::Failed(format!("texture: {path}: mip 0 has no payload in the .uexp or the .ubulk")));
    };
    let mut mips = Vec::with_capacity(plat.mips.len());
    for (k, m) in plat.mips.iter().enumerate() {
        let Some(m) = m else {
            return Err(KindError::Failed(format!(
                "texture: {path}: mip {k} has no payload in the .uexp or the .ubulk"
            )));
        };
        // the mip's own size: the top's halved k times, at least 1 (what the RHI uploads); the cooked SizeX / SizeY of
        // a block format's mip under 4 px is padded to the block (a 2 x 2 DXT1 mip is cooked as 4 x 4, 8 bytes)
        let width = (top0.width >> (k & 31)).max(1);
        let height = (top0.height >> (k & 31)).max(1);
        let cooked = |n: i32| if block { n.max(4) } else { n };
        if m.width != width && m.width != cooked(width) {
            return Err(KindError::Failed(format!(
                "texture: {path}: mip {k} is cooked {} wide, {width} expected",
                m.width
            )));
        }
        if m.height != height && m.height != cooked(height) {
            return Err(KindError::Failed(format!(
                "texture: {path}: mip {k} is cooked {} high, {height} expected",
                m.height
            )));
        }
        let want = face_bytes(&plat.pixel_format, width, height)? * i64::from(faces);
        if m.depth != 1 || i64::from(m.size) != want {
            return Err(KindError::Failed(format!(
                "texture: {path}: mip {k} ({width} x {height} x {}) is {} bytes, {} needs {want}",
                m.depth, m.size, plat.pixel_format
            )));
        }
        let bytes = if m.file == MipFile::Uexp { Some(mip_bytes(cx.paks, pkg, m, None)?.into_owned()) } else { None };
        let (Ok(w), Ok(h)) = (u16::try_from(width), u16::try_from(height)) else {
            return Err(KindError::Failed(format!("texture: {path}: a {width} x {height} mip")));
        };
        mips.push(Mip { place: *m, width: w, height: h, bytes });
    }
    // (an unset SRGB is true)
    let srgb = !matches!(obj.props.get("SRGB"), Some(Value::Bool(false)));
    Ok(Read { format: code as u8, pixel_format: plat.pixel_format, srgb, cube: plat.cube, faces: faces as u16, mips })
}

/// An entry's header: what a game kind that reads a texture's entry in-process
/// (the sky's cubes) reads, and writes for an entry of its own in the same container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureHeader {
    /// The format's code, an index of [`TEXTURE_FORMATS`].
    pub format: u8,
    pub srgb: bool,
    pub cube: bool,
    pub width: u16,
    pub height: u16,
    /// The faces (6 for a cube).
    pub depth: u16,
    /// How many of the top mips the build streams from the .ubulk.
    pub streamed: u8,
    pub mips: Vec<TextureMip>,
}

/// One mip of an entry: its size and its bytes' place from the entry's start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureMip {
    pub width: u16,
    pub height: u16,
    pub offset: u32,
    pub length: u32,
}

/// The entry's format names by code.
pub const TEXTURE_FORMATS: [&str; 10] = {
    let mut n = [""; 10];
    let mut i = 0;
    while i < FORMATS.len() {
        n[i] = FORMATS[i].1;
        i += 1;
    }
    n
};

/// The code of format `name` (`DXT1`, `FloatRGBA`).
pub fn format_code(name: &str) -> Option<u8> {
    TEXTURE_FORMATS.iter().position(|f| *f == name).map(|i| i as u8)
}

/// Writes `h` into the start of `out`, at least [`texture_header_size`] bytes.
pub fn write_header(h: &TextureHeader, out: &mut [u8]) {
    out[0..4].copy_from_slice(TEXTURE_MAGIC);
    out[4..6].copy_from_slice(&TEXTURE_FORMAT_VERSION.to_le_bytes());
    out[6] = h.format;
    out[7] = (if h.srgb { TEXTURE_SRGB } else { 0 }) | (if h.cube { TEXTURE_CUBE } else { 0 });
    out[8..10].copy_from_slice(&h.width.to_le_bytes());
    out[10..12].copy_from_slice(&h.height.to_le_bytes());
    out[12..14].copy_from_slice(&h.depth.to_le_bytes());
    out[14] = h.mips.len() as u8;
    out[15] = h.streamed;
    out[16..18].fill(0);
    for (i, m) in h.mips.iter().enumerate() {
        let o = HEADER_FIXED + HEADER_PER_MIP * i;
        out[o..o + 2].copy_from_slice(&m.width.to_le_bytes());
        out[o + 2..o + 4].copy_from_slice(&m.height.to_le_bytes());
        out[o + 4..o + 8].copy_from_slice(&m.offset.to_le_bytes());
        out[o + 8..o + 12].copy_from_slice(&m.length.to_le_bytes());
    }
}

/// An entry's header: the magic, the container's version and the format
/// code checked.
pub fn read_header(bytes: &[u8]) -> Result<TextureHeader, KindError> {
    let fail = |m: String| KindError::Failed(format!("texture: {m}"));
    if bytes.len() < HEADER_FIXED {
        return Err(fail(format!("an entry of {} bytes", bytes.len())));
    }
    if &bytes[0..4] != TEXTURE_MAGIC {
        return Err(fail(format!("not an MVTX entry ({})", String::from_utf8_lossy(&bytes[0..4]))));
    }
    let u16_at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
    let version = u16_at(4);
    if version != TEXTURE_FORMAT_VERSION {
        return Err(fail(format!("entry version {version}, the reader's {TEXTURE_FORMAT_VERSION}")));
    }
    if usize::from(bytes[6]) >= TEXTURE_FORMATS.len() {
        return Err(fail(format!("unknown format code {}", bytes[6])));
    }
    let n = usize::from(bytes[14]);
    if bytes.len() < HEADER_FIXED + HEADER_PER_MIP * n {
        return Err(fail(format!("a header of {n} mips in {} bytes", bytes.len())));
    }
    let u32_at = |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
    let mips = (0..n)
        .map(|i| {
            let o = HEADER_FIXED + HEADER_PER_MIP * i;
            TextureMip { width: u16_at(o), height: u16_at(o + 2), offset: u32_at(o + 4), length: u32_at(o + 8) }
        })
        .collect();
    Ok(TextureHeader {
        format: bytes[6],
        srgb: bytes[7] & TEXTURE_SRGB != 0,
        cube: bytes[7] & TEXTURE_CUBE != 0,
        width: u16_at(8),
        height: u16_at(10),
        depth: u16_at(12),
        streamed: bytes[15],
        mips,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(face_bytes("PF_DXT1", 2, 2).unwrap(), 8);
        assert_eq!(face_bytes("PF_BC7", 8, 5).unwrap(), 64);
        assert_eq!(face_bytes("PF_B8G8R8A8", 3, 2).unwrap(), 24);
        assert_eq!(face_bytes("PF_BC6H", 128, 128).unwrap(), 16_384);
        assert!(face_bytes("PF_Unknown", 4, 4).is_err());
        assert_eq!(texture_header_size(1), 32);
        assert_eq!(texture_header_size(13), 176);
    }

    #[test]
    fn header_round_trip() {
        let h = TextureHeader {
            format: format_code("FloatRGBA").unwrap(),
            srgb: false,
            cube: true,
            width: 128,
            height: 128,
            depth: 6,
            streamed: 0,
            mips: vec![
                TextureMip { width: 128, height: 128, offset: 48, length: 786_432 },
                TextureMip { width: 64, height: 64, offset: 786_480, length: 196_608 },
            ],
        };
        let mut out = vec![0xffu8; texture_header_size(2)];
        write_header(&h, &mut out);
        assert_eq!(read_header(&out).unwrap(), h);
        assert_eq!(TEXTURE_FORMATS[0], "DXT1");
        assert_eq!(format_code("BC6H"), Some(9));
        assert!(read_header(b"MVOB0000000000000000").is_err());
    }
}
