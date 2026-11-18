//! The `shader` kind's entry: one cooked shader code by its hash
//! (`/ShaderCode/<sha1>`), its FD3D11ShaderResourceTable, what the D3D11 RHI reads of the optional data, and the DXBC
//! container D3D's Create*Shader takes unchanged (RHICreatePixelShader 141873a10: the table first, the container
//! next, the optional data last).

use std::fmt;

use serde::{Deserialize, Deserializer};

use crate::jsonbin::{BodyRef, JsonBin};
use crate::{Error, Result};

const SHADER_MAGIC: &[u8; 4] = b"MVSC";
/// A shader code entry's path: this, then the code's SHA-1 in upper-case hex.
pub const SHADER_CODE_PREFIX: &str = "/ShaderCode/";

/// An FSHAHash: a cooked code's ShaderHashes value.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sha1(pub [u8; 20]);

impl Sha1 {
    pub fn parse(hex: &str) -> Option<Sha1> {
        if hex.len() != 40 {
            return None;
        }
        let mut out = [0u8; 20];
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
        }
        Some(Sha1(out))
    }

    /// The `shader` kind's path of the code.
    pub fn path(&self) -> String {
        format!("{SHADER_CODE_PREFIX}{self}")
    }
}

impl fmt::Display for Sha1 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02X}"))
    }
}

impl fmt::Debug for Sha1 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl<'de> Deserialize<'de> for Sha1 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Sha1, D::Error> {
        let s = <&str>::deserialize(d)?;
        Sha1::parse(s).ok_or_else(|| serde::de::Error::custom(format!("not a SHA-1: {s}")))
    }
}

/// 4.26's EShaderFrequency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Frequency {
    Vertex,
    Hull,
    Domain,
    Pixel,
    Geometry,
    Compute,
    /// The ray tracing frequencies (6 and up): DXIL in the build, no D3D11 shader.
    RayTracing(u8),
}

impl Frequency {
    pub fn from_u8(f: u8) -> Frequency {
        match f {
            0 => Frequency::Vertex,
            1 => Frequency::Hull,
            2 => Frequency::Domain,
            3 => Frequency::Pixel,
            4 => Frequency::Geometry,
            5 => Frequency::Compute,
            n => Frequency::RayTracing(n),
        }
    }
}

/// FD3D11ShaderResourceTable as the D3D11 shader compiler writes it before the container.
#[derive(Clone, Debug, Deserialize)]
pub struct ResourceTable {
    #[serde(rename = "ResourceTableBits")]
    pub bits: u32,
    #[serde(rename = "ShaderResourceViewMap")]
    pub srv_map: Vec<u32>,
    #[serde(rename = "SamplerMap")]
    pub sampler_map: Vec<u32>,
    #[serde(rename = "UnorderedAccessViewMap")]
    pub uav_map: Vec<u32>,
    /// The layout hash of each uniform buffer the table indexes (by its register): ApplyStaticUniformBuffers binds a
    /// static (pass) buffer by it.
    #[serde(rename = "ResourceTableLayoutHashes")]
    pub layout_hashes: Vec<u32>,
    #[serde(rename = "TextureMap")]
    pub texture_map: Vec<u32>,
}

/// One entry of a resource table map: a D3D register bound from resource `resource` of the uniform buffer at
/// register `buffer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableSlot {
    pub register: u8,
    pub buffer: u8,
    pub resource: u16,
}

impl ResourceTable {
    /// One map (`srv_map`, `sampler_map`, `uav_map`, `texture_map`) as its entries in order: the map is a uint32 per
    /// bit position of `bits` (its highest set bit + 1), then entries `buffer << 24 | resource << 8 | register` up to
    /// 0xffffffff; an empty map has none.
    pub fn slots(&self, map: &[u32]) -> Result<Vec<TableSlot>> {
        if map.is_empty() {
            return Ok(Vec::new());
        }
        let head = (32 - self.bits.leading_zeros()) as usize;
        let end = map.iter().skip(head).position(|&w| w == u32::MAX).map(|i| i + head).ok_or_else(|| {
            Error::format("resource table", format!("a map with no end ({head} header words, {} words)", map.len()))
        })?;
        Ok(map[head..end]
            .iter()
            .map(|&e| TableSlot { register: e as u8, buffer: (e >> 24) as u8, resource: (e >> 8) as u16 })
            .collect())
    }
}

#[derive(Deserialize)]
struct CodeHeader {
    #[serde(rename = "Frequency")]
    frequency: u8,
    #[serde(rename = "ResourceTable")]
    resource_table: ResourceTable,
    #[serde(rename = "RayEntryPoints", default)]
    ray_entry_points: Vec<String>,
    #[serde(rename = "bGlobalUniformBufferUsed", default)]
    global_uniform_buffer_used: bool,
    #[serde(rename = "OutputMask", default)]
    output_mask: u32,
    #[serde(rename = "UniformBufferNames", default)]
    uniform_buffer_names: Vec<String>,
    #[serde(rename = "VendorExtensions", default)]
    vendor_extensions: Option<String>,
    #[serde(rename = "NoCreate", default)]
    no_create: bool,
    #[serde(rename = "Code")]
    code: BodyRef,
}

/// A decoded `shader` entry.
#[derive(Clone, Debug)]
pub struct ShaderCode {
    pub frequency: Frequency,
    pub resource_table: ResourceTable,
    pub ray_entry_points: Vec<String>,
    /// The optional data's 'p' item: byte 0 (whether the loose $Globals buffer is committed) and the output mask.
    pub global_uniform_buffer_used: bool,
    pub output_mask: u32,
    /// The optional data's 'u' item: each constant-buffer register's uniform buffer name ('' the loose $Globals slot
    /// or a gap): `View`, `Primitive`, `Material`, `MaterialCollection<n>`, the pass buffers.
    pub uniform_buffer_names: Vec<String>,
    /// The optional data's 'v' item as hex (vendor extensions the RHI applies after the create).
    pub vendor_extensions: Option<String>,
    /// The optional data's '6' item is set: DXIL, no D3D11 shader is made of it.
    pub no_create: bool,
    /// The DXBC container, as D3D's Create*Shader takes it.
    pub dxbc: Vec<u8>,
}

impl ShaderCode {
    /// Decodes an entry's bytes; `what` names it in errors.
    pub fn decode(bytes: &[u8], what: &str) -> Result<ShaderCode> {
        let jb = JsonBin::split(SHADER_MAGIC, bytes, what)?;
        let h: CodeHeader = jb.parse(what)?;
        let dxbc = jb.section(h.code, what)?.to_vec();
        Ok(ShaderCode {
            frequency: Frequency::from_u8(h.frequency),
            resource_table: h.resource_table,
            ray_entry_points: h.ray_entry_points,
            global_uniform_buffer_used: h.global_uniform_buffer_used,
            output_mask: h.output_mask,
            uniform_buffer_names: h.uniform_buffer_names,
            vendor_extensions: h.vendor_extensions,
            no_create: h.no_create,
            dxbc,
        })
    }

    /// The register of the uniform buffer named `name` ('u' item), if the code binds one.
    pub fn buffer_register(&self, name: &str) -> Option<u32> {
        self.uniform_buffer_names.iter().position(|n| n == name).map(|r| r as u32)
    }

    /// The container's chunks.
    pub fn container(&self) -> Result<Dxbc<'_>> {
        Dxbc::parse(&self.dxbc)
    }
}

/// A DXBC container: "DXBC", a 16-byte checksum, u32 1, u32 total size, u32 chunk count, the chunks' offsets, then
/// each chunk (its FourCC, u32 size, data).
pub struct Dxbc<'a> {
    pub chunks: Vec<(&'a [u8; 4], &'a [u8])>,
}

impl<'a> Dxbc<'a> {
    pub fn parse(b: &'a [u8]) -> Result<Dxbc<'a>> {
        let bad = |why: String| Error::format("DXBC", why);
        let u32at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()));
        if b.len() < 32 || &b[..4] != b"DXBC" {
            return Err(bad(format!("no DXBC magic in {} bytes", b.len())));
        }
        if u32at(20) != Some(1) {
            return Err(bad(format!("version {:?}", u32at(20))));
        }
        if u32at(24) != Some(b.len() as u32) {
            return Err(bad(format!("total size {:?} for {} bytes", u32at(24), b.len())));
        }
        let n = u32at(28).unwrap() as usize;
        // (the offsets table must fit in the bytes: a malformed count is an error, not an allocation)
        if n > b.len().saturating_sub(32) / 4 {
            return Err(bad(format!("{n} chunks in {} bytes", b.len())));
        }
        let mut chunks = Vec::with_capacity(n);
        for i in 0..n {
            let at = u32at(32 + 4 * i).ok_or_else(|| bad(format!("chunk {i}'s offset past the end")))? as usize;
            let fourcc: &[u8; 4] =
                b.get(at..at + 4).and_then(|s| s.try_into().ok()).ok_or_else(|| bad(format!("chunk {i} at {at}")))?;
            let size = u32at(at + 4).ok_or_else(|| bad(format!("chunk {i} at {at}")))? as usize;
            let data = b.get(at + 8..at + 8 + size).ok_or_else(|| bad(format!("chunk {i} of {size} bytes at {at}")))?;
            chunks.push((fourcc, data));
        }
        Ok(Dxbc { chunks })
    }

    pub fn chunk(&self, fourcc: &[u8; 4]) -> Option<&'a [u8]> {
        self.chunks.iter().find(|(c, _)| *c == fourcc).map(|(_, d)| *d)
    }
}
