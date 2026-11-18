//! The `shader` kind: one entry per unique shader code of the build, named by its own hash (`/ShaderCode/<sha1>`,
//! FShaderMapResourceCode.ShaderHashes): the materials' inline maps' and the global shader cache's. An entry is an
//! MVSC container (the frequency, the FD3D11ShaderResourceTable, a ray tracing
//! shader's entry points, what the RHI reads of the optional data's 'p' item, its 'u' item) and one section: the DXBC
//! container, the code LZ4-decoded. What else the optional data holds is not kept: the codes one hash names differ in
//! the 'p' item's padding byte (5 hashes of the global cache), so the entry would depend on which of them came first.
//!
//! The hash is the shader compiler's output hash (the code and its parameter map, which the cook does not keep): it
//! cannot be recomputed from the code, so the import takes the build's word for it and keeps the first code met under
//! a hash: the materials in catalog order, then the global cache (`shadermap::scan_materials`, the same order at any
//! thread count). An item's `source` is the material its code was first met in (or the global cache's path), so the
//! runner reads that package once for all its codes.

use mvs_uasset::natives::frozen::{CodeEntry, MaterialShaderMapArchive, decode_entry, sha_of_hex, split_code};

use super::shadermap::{GLOBAL_SHADER_CACHE, Obj, global_cache, scan_materials, with_material_maps};
use crate::jsonbin::{lay_out_body, write_json_bin};
use crate::kind::{Cx, Item, Kind, KindError};

/// The kind's version.
pub const SHADER_KIND_VERSION: u64 = 1;
/// The container's magic.
pub const SHADER_MAGIC: &[u8; 4] = b"MVSC";
/// A code entry's path: this, then the code's SHA-1 (ShaderHashes) in upper-case hex.
pub const SHADER_CODE_PREFIX: &str = "/ShaderCode/";

/// The entry of one code.
pub fn shader_entry(entry: &CodeEntry<'_>, what: &str) -> Result<Vec<u8>, KindError> {
    let code = decode_entry(entry)?;
    let s = split_code(&code, entry.frequency, what)?;
    let body = &code[s.container_at..s.container_at + s.container_size];
    let (refs, _) = lay_out_body(&[body]);
    let mut json = Vec::new();
    let mut o = Obj::new(&mut json);
    o.f("Frequency", &s.frequency);
    let t = &s.resource_table;
    let mut table = Obj::new(o.key("ResourceTable"));
    table
        .f("ResourceTableBits", &t.resource_table_bits)
        .f("ShaderResourceViewMap", &t.shader_resource_view_map)
        .f("SamplerMap", &t.sampler_map)
        .f("UnorderedAccessViewMap", &t.unordered_access_view_map)
        .f("ResourceTableLayoutHashes", &t.resource_table_layout_hashes)
        .f("TextureMap", &t.texture_map);
    table.end();
    if let Some(r) = &s.ray_entry_points {
        o.f("RayEntryPoints", r);
    }
    if let Some((used, mask)) = s.packed {
        o.f("bGlobalUniformBufferUsed", &used).f("OutputMask", &mask);
    }
    if let Some(n) = &s.uniform_buffer_names {
        o.f("UniformBufferNames", n);
    }
    if let Some(v) = &s.vendor_extensions {
        o.f("VendorExtensions", v);
    }
    if s.no_create {
        o.f("NoCreate", &true);
    }
    o.f("Code", &refs[0]);
    o.end();
    Ok(write_json_bin(SHADER_MAGIC, &json, &[body], &refs))
}

/// The entry the first map that holds `hash` has for it.
fn find<'m, 'a>(maps: &'m [Option<MaterialShaderMapArchive<'a>>], hash: &[u8; 20]) -> Option<&'m CodeEntry<'a>> {
    maps.iter().flatten().find_map(|m| {
        let k = m.map.code.shader_hashes.iter().position(|h| h == hash)?;
        m.map.code.entries.get(k)
    })
}

/// The `shader` kind.
pub struct Shader;

impl Kind for Shader {
    fn id(&self) -> &'static str {
        "shader"
    }

    fn version(&self) -> u64 {
        SHADER_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/octet-stream"
    }

    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        let scan = scan_materials(cx)?;
        Ok(scan
            .code_source
            .iter()
            .map(|(h, from)| Item {
                path: format!("{SHADER_CODE_PREFIX}{}", mvs_uasset::natives::frozen::sha_hex(h)),
                source: Some(from.clone()),
            })
            .collect())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let path = item.path.as_str();
        let Some(hash_text) = path.strip_prefix(SHADER_CODE_PREFIX) else {
            return Err(KindError::NotFound(format!("shader: {path} is not a shader code path")));
        };
        let hash = sha_of_hex(hash_text);
        // a caller that did not list (the CLI's entry without --source) finds it by the scan
        let source = match &item.source {
            Some(s) => s.clone(),
            None => {
                let scan = scan_materials(cx)?;
                match hash.and_then(|h| scan.code_source.get(&h).cloned()) {
                    Some(s) => s,
                    None => return Err(KindError::NotFound(format!("shader: the build holds no code {hash_text}"))),
                }
            }
        };
        let out = match hash {
            None => None,
            Some(h) if source == GLOBAL_SHADER_CACHE => {
                global_cache(cx)?.entry(&h).map(|e| shader_entry(&e, path)).transpose()?
            }
            Some(h) => with_material_maps(cx, &source, |maps| {
                find(maps, &h).map(|e| shader_entry(e, &format!("{path} ({source})"))).transpose()
            })?,
        };
        out.ok_or_else(|| KindError::NotFound(format!("shader: {source} holds no code {hash_text}")))
    }
}
