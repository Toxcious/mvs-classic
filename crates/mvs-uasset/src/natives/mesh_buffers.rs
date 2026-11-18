//! The render buffers a static and a skeletal mesh share (positions, tangents and UVs, colours, indices, bounds), read
//! as stored for the mesh kind. It follows CUE4Parse 1.2.2's FPositionVertexBuffer,
//! FStaticMeshVertexBuffer, FColorVertexBuffer, FRawStaticIndexBuffer, FMultisizeIndexContainer), the UE 4.26 cooked
//! layouts only. An array's BulkSerialize is `crate::bulk::read_bulk_array`. The byte views borrow the package's
//! export data.
//!
//! Every buffer is kept as the bytes the package stores (a view into the package's data, no per-element read): the
//! store holds the cooked formats. FPackedNormal: the file holds the snorm8 bytes; CUE4Parse XORs each word with
//! 0x80808080 on load for FRenderingObjectVersion >= IncreaseNormalPrecision (4.20+), the reader here keeps the
//! file's bytes, which is that XOR undone.

use crate::archive::{ReadError, Result};
use crate::bulk::read_bulk_array;
use crate::objects::ExportArchive;

/// FStripDataFlags (two bytes in every UE4 package the build holds).
#[derive(Clone, Copy, Debug, Default)]
pub struct StripFlags {
    pub global: u8,
    pub cls: u8,
}

impl StripFlags {
    pub fn editor_stripped(self) -> bool {
        self.global & 1 != 0
    }

    pub fn av_stripped(self) -> bool {
        self.global & 2 != 0
    }

    pub fn class_stripped(self, flag: u8) -> bool {
        self.cls & flag != 0
    }
}

/// FPositionVertexBuffer: float32 x, y, z a vertex (cm, UE space).
#[derive(Clone, Debug, Default)]
pub struct PositionBuffer<'a> {
    pub stride: i32,
    pub vertices: i32,
    /// vertices x 12 bytes as stored.
    pub data: &'a [u8],
}

/// FStaticMeshVertexBuffer: tangents and UVs as stored.
#[derive(Clone, Debug, Default)]
pub struct StaticVertexBuffer<'a> {
    pub tex_coords: i32,
    pub vertices: i32,
    pub full_precision_uvs: bool,
    pub high_precision_tangents: bool,
    /// vertices x (8 | 16) bytes: TangentX, TangentZ a vertex, as stored. Empty when the audio-visual data is stripped.
    pub tangents: &'a [u8],
    /// At least vertices x texCoords x (8 | 4) bytes: a vertex's channels together, as stored.
    pub uvs: &'a [u8],
}

/// FColorVertexBuffer: FColor a vertex (B, G, R, A); empty when the mesh has no colours.
#[derive(Clone, Debug, Default)]
pub struct ColorBuffer<'a> {
    pub stride: i32,
    pub vertices: i32,
    pub data: &'a [u8],
}

/// An index buffer: u16 or u32 indices as stored.
#[derive(Clone, Debug, Default)]
pub struct IndexBuffer<'a> {
    pub is32: bool,
    pub count: usize,
    pub data: &'a [u8],
}

/// FBoxSphereBounds: Origin, BoxExtent, SphereRadius (float32, UE4).
#[derive(Clone, Copy, Debug, Default)]
pub struct BoxSphereBounds {
    pub origin: [f32; 3],
    pub extent: [f32; 3],
    pub radius: f32,
}

/// The tangent stream of a vertex buffer as the mesh format lays it out.
#[derive(Clone, Debug, Default)]
pub struct TangentStreams {
    /// `snorm8x4` or `snorm16x4`.
    pub format: &'static str,
    pub tangent_x: Vec<u8>,
    pub tangent_z: Vec<u8>,
}

pub fn read_strip_flags(ar: &mut ExportArchive<'_>) -> Result<StripFlags> {
    Ok(StripFlags { global: ar.u8()?, cls: ar.u8()? })
}

/// SkipBulkArrayData: element size, count, past the elements.
pub fn skip_bulk_array(ar: &mut ExportArchive<'_>) -> Result<()> {
    let elem = i64::from(ar.i32()?);
    let count = i64::from(ar.i32()?);
    skip_bytes(ar, elem * count)
}

/// SkipFixedArray(size): count, past count x size bytes.
pub fn skip_fixed_array(ar: &mut ExportArchive<'_>, size: usize) -> Result<()> {
    let count = i64::from(ar.i32()?);
    skip_bytes(ar, count * i64::try_from(size).unwrap_or(i64::MAX))
}

/// Past `n` bytes; a negative length is a read error.
fn skip_bytes(ar: &mut ExportArchive<'_>, n: i64) -> Result<()> {
    match usize::try_from(n) {
        Ok(n) => ar.skip(n),
        Err(_) => Err(ReadError::parse(format!("{}: skip of {n} at {}", ar.name, ar.pos))),
    }
}

pub fn read_position_buffer<'a>(ar: &mut ExportArchive<'a>) -> Result<PositionBuffer<'a>> {
    let stride = ar.i32()?;
    let vertices = ar.i32()?;
    if stride != 12 {
        return Err(ReadError::not_ported(format!("FPositionVertexBuffer stride {stride} not ported")));
    }
    let (count, data) = read_bulk_array(&mut ar.ar, 12, "FPositionVertexBuffer")?;
    if i64::try_from(count).ok() != Some(i64::from(vertices)) {
        return Err(ReadError::parse(format!(
            "{}: FPositionVertexBuffer holds {count} vertices, {vertices} named",
            ar.name
        )));
    }
    Ok(PositionBuffer { stride, vertices, data })
}

/// FStaticMeshVertexBuffer (4.19+ layout): the tangent basis (TangentX then TangentZ a vertex, FPackedNormal or
/// FPackedRGBA16N) and the texture coordinates (all channels of a vertex together, float32 or float16 pairs).
pub fn read_static_vertex_buffer<'a>(ar: &mut ExportArchive<'a>) -> Result<StaticVertexBuffer<'a>> {
    let strip = read_strip_flags(ar)?;
    let tex_coords = ar.i32()?;
    let vertices = ar.i32()?;
    let full_precision_uvs = ar.bool()?;
    let high_precision_tangents = ar.bool()?;
    let mut out = StaticVertexBuffer {
        tex_coords,
        vertices,
        full_precision_uvs,
        high_precision_tangents,
        tangents: &[],
        uvs: &[],
    };
    if strip.av_stripped() {
        return Ok(out);
    }
    let tsize = if high_precision_tangents { 16 } else { 8 };
    let (count, tangents) = read_bulk_array(&mut ar.ar, tsize, "FStaticMeshVertexBuffer tangents")?;
    if i64::try_from(count).ok() != Some(i64::from(vertices)) {
        return Err(ReadError::parse(format!(
            "{}: FStaticMeshVertexBuffer: {count} tangents, {vertices} vertices",
            ar.name
        )));
    }
    out.tangents = tangents;
    // the texture coordinates: one element per channel; CUE4Parse's GetTexCoordNumVerts allows one padding vertex
    let uv_size = if full_precision_uvs { 8 } else { 4 };
    let (count, uvs) = read_bulk_array(&mut ar.ar, uv_size, "FStaticMeshVertexBuffer uvs")?;
    let (v, t, c) = (i64::from(vertices), i64::from(tex_coords), i64::try_from(count).unwrap_or(i64::MAX));
    let per = if c == v * t { v } else { v + if v > 0 { t % 2 } else { 0 } };
    if c != per * t {
        return Err(ReadError::parse(format!(
            "{}: FStaticMeshVertexBuffer: {count} uvs, {per} x {tex_coords} expected",
            ar.name
        )));
    }
    out.uvs = uvs;
    Ok(out)
}

/// A count read from a buffer, as a length.
fn length(n: i32, what: &str) -> Result<usize> {
    usize::try_from(n).map_err(|_| ReadError::parse(format!("{what}: length {n}")))
}

/// `src` copied into a zeroed buffer of `n` bytes (a typed array the stored bytes are set into: a
/// shorter view leaves the rest zero, a longer one is cut).
fn zero_padded(src: &[u8], n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n];
    let m = n.min(src.len());
    out[..m].copy_from_slice(&src[..m]);
    out
}

/// The TangentX and TangentZ streams of a vertex buffer (copies): each 4 snorm8 a vertex (FPackedNormal), or 4 snorm16
/// a vertex (FPackedRGBA16N, a buffer with bUseHighPrecisionTangentBasis: the file's int16 words, CUE4Parse's XOR with
/// 0x8000 a component undone as for FPackedNormal).
pub fn tangent_streams(vb: &StaticVertexBuffer<'_>) -> Result<TangentStreams> {
    let n = length(vb.vertices, "tangent streams")?;
    let width = if vb.high_precision_tangents { 8 } else { 4 }; // bytes a tangent
    let src = zero_padded(vb.tangents, n * 2 * width);
    let mut tx = Vec::with_capacity(n * width);
    let mut tz = Vec::with_capacity(n * width);
    for v in src.chunks_exact(2 * width) {
        tx.extend_from_slice(&v[..width]);
        tz.extend_from_slice(&v[width..]);
    }
    let format = if vb.high_precision_tangents { "snorm16x4" } else { "snorm8x4" };
    Ok(TangentStreams { format, tangent_x: tx, tangent_z: tz })
}

/// Channel `c` of a vertex buffer's texture coordinates, 2 float32 (8 bytes) or 2 float16 (4 bytes) a vertex (a copy).
pub fn uv_stream(vb: &StaticVertexBuffer<'_>, c: usize) -> Result<Vec<u8>> {
    let n = length(vb.vertices, "uv stream")?;
    let width = if vb.full_precision_uvs { 8 } else { 4 }; // bytes a channel
    let stride = length(vb.tex_coords, "uv stream")? * width;
    let src = zero_padded(vb.uvs, n * stride);
    let mut out = Vec::with_capacity(n * width);
    for i in 0..n {
        let at = i * stride + c * width;
        // (a typed array reads undefined past its end: a channel the buffer does not hold gives 0)
        match src.get(at..at + width) {
            Some(s) => out.extend_from_slice(s),
            None => out.extend(std::iter::repeat_n(0, width)),
        }
    }
    Ok(out)
}

pub fn read_color_buffer<'a>(ar: &mut ExportArchive<'a>) -> Result<ColorBuffer<'a>> {
    let strip = read_strip_flags(ar)?;
    let stride = ar.i32()?;
    let vertices = ar.i32()?;
    if strip.av_stripped() || vertices <= 0 {
        return Ok(ColorBuffer { stride, vertices, data: &[] });
    }
    let (count, data) = read_bulk_array(&mut ar.ar, 4, "FColorVertexBuffer")?;
    if i64::try_from(count).ok() != Some(i64::from(vertices)) {
        return Err(ReadError::parse(format!(
            "{}: FColorVertexBuffer holds {count} colours, {vertices} named",
            ar.name
        )));
    }
    Ok(ColorBuffer { stride, vertices, data })
}

/// FColor bytes (B, G, R, A) as R, G, B, A (a copy): the order of a unorm8x4 colour stream.
pub fn color_rgba(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = data.to_vec();
    for c in out.as_chunks_mut::<4>().0 {
        c.swap(0, 2);
    }
    Ok(out)
}

/// FRawStaticIndexBuffer (4.25+): a 32-bit flag, the indices as a byte bulk array, the ShouldExpandTo32Bit flag.
pub fn read_raw_static_index_buffer<'a>(ar: &mut ExportArchive<'a>) -> Result<IndexBuffer<'a>> {
    let is32 = ar.bool()?;
    let (_, data) = read_bulk_array(&mut ar.ar, 1, "FRawStaticIndexBuffer")?;
    ar.bool()?; // bShouldExpandTo32Bit (RawIndexBuffer.HasShouldExpandTo32Bit, UE4.25+)
    let w = if is32 { 4 } else { 2 };
    if data.len() % w != 0 {
        return Err(ReadError::parse(format!(
            "{}: FRawStaticIndexBuffer of {} bytes, {w}-byte indices",
            ar.name,
            data.len()
        )));
    }
    Ok(IndexBuffer { is32, count: data.len() / w, data })
}

/// FMultisizeIndexContainer (skeletal meshes): the index size in bytes, then a bulk array of u16 or u32.
pub fn read_multisize_index_container<'a>(ar: &mut ExportArchive<'a>) -> Result<IndexBuffer<'a>> {
    let size = ar.u8()?;
    if size != 2 && size != 4 {
        return Err(ReadError::parse(format!("{}: FMultisizeIndexContainer index size {size}", ar.name)));
    }
    let (count, data) = read_bulk_array(&mut ar.ar, usize::from(size), "FMultisizeIndexContainer")?;
    Ok(IndexBuffer { is32: size == 4, count, data })
}

/// FWeightedRandomSampler: Prob float[], Alias int[], TotalWeight: read past.
pub fn skip_weighted_random_sampler(ar: &mut ExportArchive<'_>) -> Result<()> {
    skip_fixed_array(ar, 4)?;
    skip_fixed_array(ar, 4)?;
    ar.skip(4)
}

pub fn read_box_sphere_bounds(ar: &mut ExportArchive<'_>) -> Result<BoxSphereBounds> {
    let origin = [ar.f32()?, ar.f32()?, ar.f32()?];
    let extent = [ar.f32()?, ar.f32()?, ar.f32()?];
    Ok(BoxSphereBounds { origin, extent, radius: ar.f32()? })
}

/// FBox: Min, Max, IsValid byte (25 bytes).
pub fn skip_box(ar: &mut ExportArchive<'_>) -> Result<()> {
    ar.skip(25)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer<'a>(tangents: &'a [u8], uvs: &'a [u8]) -> StaticVertexBuffer<'a> {
        StaticVertexBuffer {
            tex_coords: 2,
            vertices: 2,
            full_precision_uvs: false,
            high_precision_tangents: false,
            tangents,
            uvs,
        }
    }

    #[test]
    fn tangents_split_per_vertex() {
        let t: Vec<u8> = (0..16).collect();
        let s = tangent_streams(&buffer(&t, &[])).unwrap();
        assert_eq!(s.format, "snorm8x4");
        assert_eq!(s.tangent_x, [0, 1, 2, 3, 8, 9, 10, 11]);
        assert_eq!(s.tangent_z, [4, 5, 6, 7, 12, 13, 14, 15]);
        // stripped tangents read as zero
        assert_eq!(tangent_streams(&buffer(&[], &[])).unwrap().tangent_x, [0; 8]);
    }

    #[test]
    fn uv_channels_split_per_vertex() {
        // 2 vertices x 2 channels x f16x2
        let u: Vec<u8> = (0..16).collect();
        assert_eq!(uv_stream(&buffer(&[], &u), 1).unwrap(), [4, 5, 6, 7, 12, 13, 14, 15]);
    }

    #[test]
    fn colours_reorder() {
        assert_eq!(color_rgba(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap(), [3, 2, 1, 4, 7, 6, 5, 8]);
    }
}
