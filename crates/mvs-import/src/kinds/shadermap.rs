//! The `shadermap` kind: the inline shader maps of every material that serializes one, and the global shader cache's
//! (`/Engine/GlobalShaderCache`), as MVSM containers (the preshader programs as
//! sections). Built only as the proof of its port (the material bind reads it): `stored` false.
//!
//! What it lists: every material interface export whose native tail holds inline shader map resources
//! (UMaterialInterface.DeserializeInlineShaderMaps, `mvs_uasset::natives::frozen`): each Material, each
//! MaterialInstanceConstant with bHasStaticPermutationResource, and the LandscapeMaterialInstanceConstants inside the
//! levels (by their inner path, `<level>.<level>:PersistentLevel.<landscape>.<instance>`), plus
//! [`GLOBAL_SHADER_CACHE`] for the paks' [`GLOBAL_CACHE_FILE`]. The list reads every candidate ([`scan_materials`],
//! shared with the `shader` kind's list through `Cx::shared`, which needs every code hash and where it is first met).
//!
//! A resource that is not cooked or whose map is not valid is null in the entry (LaserTrail_MAT): the bind reports it.
//! A map's FName fields are the names its ScriptNames / MinimalNames patch in; its FHashedName fields are hashes
//! (16 upper-case hex digits); parameter names are kept whole.

use std::sync::Arc;

use indexmap::IndexMap;
use mvs_pak::paths::{join_object_path, split_object_path};
use mvs_uasset::Package;
use mvs_uasset::archive::Archive;
use mvs_uasset::classes;
use mvs_uasset::natives::frozen::{
    CodeEntry, GlobalShaderMapRecord, LooseParameterBuffer, MaterialCompilationOutput,
    MaterialExternalTextureParameterInfo, MaterialMapContent, MaterialParameterInfo, MaterialScalarParameterInfo,
    MaterialShaderMapArchive, MaterialShaderMapId, MaterialShaderMapRecord, MaterialTextureParameterInfo,
    MaterialVectorParameterInfo, MeshShaderMapRecord, ParameterStructReference, Sha1, ShaderMapArchive, ShaderMapCode,
    ShaderMapContentRecord, ShaderParameterBindings, ShaderParameterInfo, ShaderParameterMapInfo, ShaderPipelineRecord,
    ShaderRecord, UniformBufferLayout, UniformExpressionSet, UniformPreshaderHeader, VtStack, global_map_content,
    material_map_content, read_global_shader_cache, read_inline_shader_maps, sha_hex,
};
use mvs_uasset::natives::material::SHADER_MAPS_AT;
use mvs_uasset::objects::IssueKind;
use mvs_uasset::value::Value;
use rayon::prelude::*;

use crate::catalog::header_object_path;
use crate::json::{write_int, write_number, write_string};
use crate::jsonbin::{BodyRef, lay_out_body, write_json_bin};
use crate::kind::{Cx, Item, Kind, KindError};

/// The kind's version.
pub const SHADERMAP_KIND_VERSION: u64 = 2;
/// The container's magic.
pub const SHADERMAP_MAGIC: &[u8; 4] = b"MVSM";
/// The path of the global shader cache's entry.
pub const GLOBAL_SHADER_CACHE: &str = "/Engine/GlobalShaderCache";
/// The global shader cache's file in the paks.
pub const GLOBAL_CACHE_FILE: &str = "Engine/GlobalShaderCache-PCD3D_SM5.bin";

/// The levels: the packages that hold inner landscape material instances.
const LEVEL_CLASS: &str = "/Script/Engine.World";

/// Whether a class (an object path, `/Script/Engine.Material`) is read by a material native that has a tail.
fn is_material_class(cls: &str) -> bool {
    let Some(rest) = cls.strip_prefix("/Script/") else { return false };
    let name = &rest[rest.rfind('.').map_or(0, |d| d + 1)..];
    classes::native_chain(name).is_some_and(|c| c.contains(&"UMaterial") || c.contains(&"UMaterialInstance"))
}

/// The export a path names below its top-level export `top`: `B:Inner.Sub` -> the export named Sub whose outer is
/// Inner whose outer is top (names compared without regard to case, the first that matches).
fn inner_index(pkg: &Package, top: usize, chain: &[&str]) -> Option<usize> {
    let mut at = top;
    for name in chain {
        let want = name.to_lowercase();
        let outer = i32::try_from(at + 1).ok()?;
        at = pkg.exports.iter().position(|e| e.outer == outer && e.object_name.to_lowercase() == want)?;
    }
    Some(at)
}

/// Runs `f` over the shader maps of the material a path names (a top-level export, or an inner one after `:`), as
/// the material natives read them (`mvs_uasset::natives::material`): none when the export serializes none. A path
/// that is no material is `NotFound`; a material the reader does not read whole fails.
pub fn with_material_maps<T>(
    cx: &Cx<'_>,
    path: &str,
    f: impl FnOnce(&[Option<MaterialShaderMapArchive<'_>>]) -> Result<T, KindError>,
) -> Result<T, KindError> {
    let (pkg_path, object) = split_object_path(path);
    let (top_path, chain) = match object.and_then(|o| o.find(':').map(|c| (o, c))) {
        Some((o, c)) => (format!("{pkg_path}.{}", &o[..c]), o[c + 1..].split('.').collect()),
        None => (path.to_owned(), Vec::new()),
    };
    let (pkg, top) = cx.read(&top_path)?;
    let Some(i) = inner_index(&pkg, top, &chain) else {
        return Err(KindError::NotFound(format!("shadermap: {path}: no such inner object")));
    };
    let obj = cx.cache.object(&pkg, i);
    if !obj.chain.contains(&"UMaterialInterface") {
        return Err(KindError::NotFound(format!("shadermap: {path} is a {}, not a material", obj.type_name)));
    }
    if let Some(p) = obj.partial {
        return Err(KindError::Failed(format!(
            "shadermap: {path}: a {}, whose native class {p} the reader does not port",
            obj.type_name
        )));
    }
    // the read's error (the object's `error`): any issue of a material read whole
    if !obj.issues.is_empty() {
        let list: Vec<String> = obj
            .issues
            .iter()
            .map(|e| {
                let k = match e.kind {
                    IssueKind::Read => "read",
                    IssueKind::Native => "native",
                    IssueKind::NotPorted => "not-ported",
                };
                format!("[{k}] {}", e.message)
            })
            .collect();
        return Err(KindError::Failed(format!("shadermap: {path}: {}", list.join("; "))));
    }
    let Some(at) = obj.data.get(SHADER_MAPS_AT) else { return f(&[]) };
    let Value::UInt(at) = *at else { unreachable!("{SHADER_MAPS_AT} is a UInt") };
    let mut ar = Archive::new(pkg.uexp()?, &pkg.path);
    ar.pos = usize::try_from(at).expect("an offset in the export data");
    let maps = read_inline_shader_maps(&mut ar)?;
    f(&maps)
}

/// Where a global cache code entry's stored bytes are in the file.
#[derive(Clone, Copy, Debug)]
struct CodeAt {
    at: usize,
    len: usize,
    uncompressed_size: i32,
    frequency: u8,
}

/// The global shader cache file, read once per run: its bytes, and per code hash its first entry (maps in the file's
/// order, entries in each map's).
pub struct GlobalCache {
    bytes: Vec<u8>,
    codes: IndexMap<Sha1, CodeAt>,
}

impl GlobalCache {
    fn read(cx: &Cx<'_>) -> Result<GlobalCache, KindError> {
        if cx.paks.entry(GLOBAL_CACHE_FILE).is_none() {
            return Err(KindError::Failed(format!("shadermap: the paks hold no {GLOBAL_CACHE_FILE}")));
        }
        let bytes = cx.paks.read(GLOBAL_CACHE_FILE).map_err(|e| KindError::Failed(e.to_string()))?;
        let mut codes = IndexMap::new();
        for m in parse_global(&bytes)? {
            for (h, e) in m.code.shader_hashes.iter().zip(&m.code.entries) {
                codes.entry(*h).or_insert(CodeAt {
                    at: e.at,
                    len: e.code.len(),
                    uncompressed_size: e.uncompressed_size,
                    frequency: e.frequency,
                });
            }
        }
        Ok(GlobalCache { bytes, codes })
    }

    /// The maps in their archive form.
    pub fn maps(&self) -> Result<Vec<ShaderMapArchive<'_>>, KindError> {
        parse_global(&self.bytes)
    }

    /// The code entry the first map that holds `hash` has for it.
    pub fn entry(&self, hash: &Sha1) -> Option<CodeEntry<'_>> {
        self.codes.get(hash).map(|c| CodeEntry {
            code: &self.bytes[c.at..c.at + c.len],
            at: c.at,
            uncompressed_size: c.uncompressed_size,
            frequency: c.frequency,
        })
    }
}

fn parse_global(bytes: &[u8]) -> Result<Vec<ShaderMapArchive<'_>>, KindError> {
    let mut ar = Archive::new(bytes, GLOBAL_CACHE_FILE);
    let maps = read_global_shader_cache(&mut ar)?;
    if ar.pos != ar.len() {
        return Err(KindError::Failed(format!("shadermap: {GLOBAL_CACHE_FILE} read to {} of {}", ar.pos, ar.len())));
    }
    Ok(maps)
}

/// The run's global shader cache (a failed read is the run's: every kind that asks fails with it).
struct GlobalSlot(Result<Arc<GlobalCache>, String>);

/// The global shader cache, read once per run and shared by the shadermap and shader kinds.
pub fn global_cache(cx: &Cx<'_>) -> Result<Arc<GlobalCache>, KindError> {
    let slot = cx.shared.get_or_init(|| GlobalSlot(GlobalCache::read(cx).map(Arc::new).map_err(|e| e.to_string())));
    slot.0.clone().map_err(KindError::Failed)
}

/// What the lists read: the materials with inline maps, in catalog order, and per code hash where it is first met (a
/// material path or [`GLOBAL_SHADER_CACHE`]), in the order first met: the materials in catalog order (the catalog's
/// exports, then the levels' inner instances), then the global cache.
pub struct MaterialScan {
    pub materials: Vec<String>,
    pub code_source: IndexMap<Sha1, String>,
}

/// The run's material scan (a failed scan is the run's).
struct ScanSlot(Result<Arc<MaterialScan>, String>);

/// The material scan, read once per run and shared by the shadermap and shader kinds' lists.
pub fn scan_materials(cx: &Cx<'_>) -> Result<Arc<MaterialScan>, KindError> {
    let slot = cx.shared.get_or_init(|| ScanSlot(read_scan(cx).map(Arc::new).map_err(|e| e.to_string())));
    slot.0.clone().map_err(KindError::Failed)
}

/// Every material interface of the build read: the materials with inline maps and where each code is first met. The
/// candidates are read on all cores; the result is put together in their order, the same at any thread count.
fn read_scan(cx: &Cx<'_>) -> Result<MaterialScan, KindError> {
    let mut candidates: Vec<String> = Vec::new();
    let mut levels: Vec<&str> = Vec::new();
    for e in cx.catalog.exports() {
        if is_material_class(e.class) {
            candidates.push(e.object_path());
        } else if e.class == LEVEL_CLASS {
            levels.push(e.package);
        }
    }
    // a level's inner material instances, from its header alone
    for p in levels {
        let Some(pkg) = cx.cache.package(p)? else { continue };
        for (i, e) in pkg.exports.iter().enumerate() {
            if e.outer == 0 {
                continue;
            }
            let Some(c) = header_object_path(&pkg, e.class_index) else { continue };
            if !is_material_class(&c) {
                continue;
            }
            let mut names: Vec<&str> = Vec::new();
            let mut j = i;
            loop {
                let x = &pkg.exports[j];
                names.push(&x.object_name);
                if x.outer <= 0 || names.len() > pkg.exports.len() {
                    break;
                }
                j = usize::try_from(x.outer - 1).expect("a positive outer");
            }
            names.reverse();
            candidates.push(join_object_path(p, &names));
        }
    }
    let read: Vec<Result<Option<Vec<Sha1>>, KindError>> = candidates
        .par_iter()
        .map(|path| {
            let r = with_material_maps(cx, path, |maps| {
                Ok((!maps.is_empty())
                    .then(|| maps.iter().flatten().flat_map(|m| m.map.code.shader_hashes.iter().copied()).collect()))
            });
            cx.cache.trim();
            r
        })
        .collect();
    let mut materials = Vec::new();
    let mut code_source: IndexMap<Sha1, String> = IndexMap::new();
    for (path, r) in candidates.iter().zip(read) {
        let Some(hashes) = r? else { continue };
        materials.push(path.clone());
        for h in hashes {
            code_source.entry(h).or_insert_with(|| path.clone());
        }
    }
    for h in global_cache(cx)?.codes.keys() {
        code_source.entry(*h).or_insert_with(|| GLOBAL_SHADER_CACHE.to_owned());
    }
    Ok(MaterialScan { materials, code_source })
}

/// The `shadermap` kind.
pub struct Shadermap;

impl Kind for Shadermap {
    fn id(&self) -> &'static str {
        "shadermap"
    }

    fn version(&self) -> u64 {
        SHADERMAP_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/octet-stream"
    }

    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        let scan = scan_materials(cx)?;
        let mut items: Vec<Item> = scan.materials.iter().map(Item::new).collect();
        items.push(Item::new(GLOBAL_SHADER_CACHE));
        Ok(items)
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let path = item.path.as_str();
        let mut json = Vec::new();
        if path == GLOBAL_SHADER_CACHE {
            let global = global_cache(cx)?;
            let mut records = Vec::new();
            for (i, m) in global.maps()?.iter().enumerate() {
                records.push(global_map_content(m, &format!("{GLOBAL_CACHE_FILE} map {i}"))?);
            }
            let mut o = Obj::new(&mut json);
            o.f("kind", "global");
            write_array(o.key("Maps"), &records, global_record);
            o.end();
            return Ok(write_json_bin::<&[u8]>(SHADERMAP_MAGIC, &json, &[], &[]));
        }
        let read: Vec<Option<MaterialMapContent>> = with_material_maps(cx, path, |maps| {
            let mut out = Vec::with_capacity(maps.len());
            for m in maps {
                out.push(m.as_ref().map(|m| material_map_content(m, path)).transpose()?);
            }
            Ok(out)
        })?;
        if read.is_empty() {
            return Err(KindError::NotFound(format!("shadermap: {path} serializes no inline shader map")));
        }
        let sections: Vec<&[u8]> = read.iter().flatten().map(|r| r.preshaders.as_slice()).collect();
        let (refs, _) = lay_out_body(&sections);
        let mut k = 0;
        let mut o = Obj::new(&mut json);
        o.f("kind", "material");
        write_array(o.key("Resources"), &read, |out, r| match r {
            Some(r) => {
                material_record(out, &r.record, refs[k]);
                k += 1;
            }
            None => out.extend_from_slice(b"null"),
        });
        o.end();
        Ok(write_json_bin(SHADERMAP_MAGIC, &json, &sections, &refs))
    }

    fn stored(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------- the records' JSON

/// A value as JSON.stringify prints it.
pub(crate) trait Js {
    fn js(&self, out: &mut Vec<u8>);
}

impl Js for bool {
    fn js(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(if *self { b"true" } else { b"false" });
    }
}

macro_rules! js_int {
    ($($t:ty),*) => {$(
        impl Js for $t {
            fn js(&self, out: &mut Vec<u8>) {
                write_int(out, i128::from(*self));
            }
        }
    )*};
}
js_int!(u8, u16, u32, i32, u64);

impl Js for f32 {
    fn js(&self, out: &mut Vec<u8>) {
        write_number(out, f64::from(*self));
    }
}

impl Js for str {
    fn js(&self, out: &mut Vec<u8>) {
        write_string(out, self);
    }
}

impl Js for String {
    fn js(&self, out: &mut Vec<u8>) {
        write_string(out, self);
    }
}

impl<T: Js> Js for Option<T> {
    fn js(&self, out: &mut Vec<u8>) {
        match self {
            Some(v) => v.js(out),
            None => out.extend_from_slice(b"null"),
        }
    }
}

impl<T: Js> Js for [T] {
    fn js(&self, out: &mut Vec<u8>) {
        write_array(out, self, |out, v| v.js(out));
    }
}

impl<T: Js> Js for Vec<T> {
    fn js(&self, out: &mut Vec<u8>) {
        self.as_slice().js(out);
    }
}

impl<T: Js, const N: usize> Js for [T; N] {
    fn js(&self, out: &mut Vec<u8>) {
        self.as_slice().js(out);
    }
}

impl<A: Js, B: Js> Js for (A, B) {
    fn js(&self, out: &mut Vec<u8>) {
        out.push(b'[');
        self.0.js(out);
        out.push(b',');
        self.1.js(out);
        out.push(b']');
    }
}

impl Js for BodyRef {
    fn js(&self, out: &mut Vec<u8>) {
        let mut o = Obj::new(out);
        o.f("offset", &(self.offset as u64)).f("length", &(self.length as u64));
        o.end();
    }
}

/// An array, each element written by `each`.
pub(crate) fn write_array<T>(out: &mut Vec<u8>, items: &[T], mut each: impl FnMut(&mut Vec<u8>, &T)) {
    out.push(b'[');
    for (i, v) in items.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        each(out, v);
    }
    out.push(b']');
}

/// An object being written: its keys in the order given.
pub(crate) struct Obj<'o> {
    out: &'o mut Vec<u8>,
    first: bool,
}

impl<'o> Obj<'o> {
    pub(crate) fn new(out: &'o mut Vec<u8>) -> Obj<'o> {
        out.push(b'{');
        Obj { out, first: true }
    }

    /// The next key; its value is written into the buffer it gives.
    pub(crate) fn key(&mut self, k: &str) -> &mut Vec<u8> {
        if !self.first {
            self.out.push(b',');
        }
        self.first = false;
        write_string(self.out, k);
        self.out.push(b':');
        self.out
    }

    /// The next key and its value.
    pub(crate) fn f<V: Js + ?Sized>(&mut self, k: &str, v: &V) -> &mut Self {
        v.js(self.key(k));
        self
    }

    pub(crate) fn end(self) {
        self.out.push(b'}');
    }
}

/// An object written by `body`.
fn object(out: &mut Vec<u8>, body: impl FnOnce(&mut Obj<'_>)) {
    let mut o = Obj::new(out);
    body(&mut o);
    o.end();
}

impl Js for ShaderParameterInfo {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            o.f("BaseIndex", &self.base_index).f("Size", &self.size);
        });
    }
}

impl Js for LooseParameterBuffer {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            o.f("BaseIndex", &self.base_index).f("Size", &self.size).f("Parameters", &self.parameters);
        });
    }
}

impl Js for ShaderParameterMapInfo {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            o.f("UniformBuffers", &self.uniform_buffers)
                .f("TextureSamplers", &self.texture_samplers)
                .f("SRVs", &self.srvs)
                .f("LooseParameterBuffers", &self.loose_parameter_buffers)
                .f("Hash", &self.hash);
        });
    }
}

impl Js for ShaderParameterBindings {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            write_array(o.key("Parameters"), &self.parameters, |out, p| {
                object(out, |o| {
                    o.f("BufferIndex", &p.buffer_index)
                        .f("BaseIndex", &p.base_index)
                        .f("ByteOffset", &p.byte_offset)
                        .f("ByteSize", &p.byte_size);
                });
            });
            write_array(o.key("ResourceParameters"), &self.resource_parameters, |out, p| {
                object(out, |o| {
                    o.f("ByteOffset", &p.byte_offset).f("BaseIndex", &p.base_index).f("BaseType", &p.base_type);
                });
            });
            let reference = |out: &mut Vec<u8>, r: &ParameterStructReference| {
                object(out, |o| {
                    o.f("BufferIndex", &r.buffer_index).f("ByteOffset", &r.byte_offset);
                });
            };
            write_array(o.key("GraphUniformBuffers"), &self.graph_uniform_buffers, reference);
            write_array(o.key("ParameterReferences"), &self.parameter_references, reference);
            o.f("StructureLayoutHash", &self.structure_layout_hash)
                .f("RootParameterBufferIndex", &self.root_parameter_buffer_index);
        });
    }
}

impl Js for ShaderRecord {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            o.f("Type", &self.type_)
                .f("VFType", &self.vf_type)
                .f("Permutation", &self.permutation)
                .f("Frequency", &self.frequency)
                .f("Platform", &self.platform)
                .f("ResourceIndex", &self.resource_index)
                .f("Hash", &self.hash)
                .f("NumInstructions", &self.num_instructions)
                .f("Bindings", &self.bindings)
                .f("ParameterMapInfo", &self.parameter_map_info)
                .f("UniformBufferParameterStructs", &self.uniform_buffer_parameter_structs)
                .f("UniformBufferParameters", &self.uniform_buffer_parameters);
        });
    }
}

impl Js for ShaderPipelineRecord {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            o.f("TypeName", &self.type_name).f("Shaders", &self.shaders).f("PermutationIds", &self.permutation_ids);
        });
    }
}

/// FShaderMapContent's keys, at the start of the record that holds it.
fn content_keys(o: &mut Obj<'_>, c: &ShaderMapContentRecord) {
    o.f("Shaders", &c.shaders).f("ShaderPipelines", &c.shader_pipelines).f("ShaderPlatform", &c.shader_platform);
}

impl Js for MeshShaderMapRecord {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            content_keys(o, &self.content);
            o.f("VertexFactoryTypeName", &self.vertex_factory_type_name);
        });
    }
}

impl Js for MaterialParameterInfo {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            o.f("Name", &self.name).f("Index", &self.index).f("Association", &self.association);
        });
    }
}

impl Js for MaterialScalarParameterInfo {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            o.f("ParameterInfo", &self.parameter_info).f("DefaultValue", &self.default_value);
        });
    }
}

impl Js for MaterialVectorParameterInfo {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            o.f("ParameterInfo", &self.parameter_info);
            let [r, g, b, a] = &self.default_value;
            object(o.key("DefaultValue"), |c| {
                c.f("R", r).f("G", g).f("B", b).f("A", a);
            });
        });
    }
}

impl Js for MaterialTextureParameterInfo {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            o.f("ParameterInfo", &self.parameter_info)
                .f("TextureIndex", &self.texture_index)
                .f("SamplerSource", &self.sampler_source)
                .f("VirtualTextureLayerIndex", &self.virtual_texture_layer_index);
        });
    }
}

impl Js for MaterialExternalTextureParameterInfo {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            o.f("ParameterName", &self.parameter_name)
                .f("ExternalTextureGuid", &self.external_texture_guid)
                .f("SourceTextureIndex", &self.source_texture_index);
        });
    }
}

impl Js for UniformPreshaderHeader {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            o.f("OpcodeOffset", &self.opcode_offset).f("OpcodeSize", &self.opcode_size);
        });
    }
}

impl Js for UniformBufferLayout {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            o.f("Name", &self.name)
                .f("ConstantBufferSize", &self.constant_buffer_size)
                .f("StaticSlot", &self.static_slot)
                .f("RenderTargetsOffset", &self.render_targets_offset)
                .f("bHasNonGraphOutputs", &self.b_has_non_graph_outputs)
                .f("Resources", &self.resources)
                .f("GraphResources", &self.graph_resources)
                .f("GraphTextures", &self.graph_textures)
                .f("GraphBuffers", &self.graph_buffers)
                .f("GraphUniformBuffers", &self.graph_uniform_buffers)
                .f("UniformBuffers", &self.uniform_buffers)
                .f("Hash", &self.hash);
        });
    }
}

impl Js for VtStack {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            o.f("NumLayers", &self.num_layers)
                .f("LayerUniformExpressionIndices", &self.layer_uniform_expression_indices)
                .f("PreallocatedStackTextureIndex", &self.preallocated_stack_texture_index);
        });
    }
}

/// FUniformExpressionSet, its preshader program named by `data` (the entry's section).
fn expression_set(out: &mut Vec<u8>, s: &UniformExpressionSet, data: BodyRef) {
    object(out, |o| {
        o.f("UniformVectorPreshaders", &s.uniform_vector_preshaders)
            .f("UniformScalarPreshaders", &s.uniform_scalar_preshaders)
            .f("UniformScalarParameters", &s.uniform_scalar_parameters)
            .f("UniformVectorParameters", &s.uniform_vector_parameters)
            .f("UniformTextureParameters", &s.uniform_texture_parameters)
            .f("UniformExternalTextureParameters", &s.uniform_external_texture_parameters);
        object(o.key("UniformPreshaderData"), |p| {
            p.f("Names", &s.preshader_names).f("Data", &data);
        });
        o.f("VTStacks", &s.vt_stacks)
            .f("ParameterCollections", &s.parameter_collections)
            .f("UniformBufferLayoutInitializer", &s.uniform_buffer_layout_initializer);
    });
}

fn compilation_output(out: &mut Vec<u8>, c: &MaterialCompilationOutput, data: BodyRef) {
    object(out, |o| {
        expression_set(o.key("UniformExpressionSet"), &c.uniform_expression_set, data);
        o.f("UsedSceneTextures", &c.used_scene_textures)
            .f("UsedDBufferTextures", &c.used_d_buffer_textures)
            .f("RuntimeVirtualTextureOutputAttributeMask", &c.runtime_virtual_texture_output_attribute_mask)
            .f("b1", &c.b1)
            .f("b2", &c.b2);
    });
}

impl Js for ShaderMapCode {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            o.f("bShareCode", &self.b_share_code)
                .f("ResourceHash", &self.resource_hash)
                .f("ShaderHashes", &self.shader_hashes)
                .f("Frequencies", &self.frequencies);
        });
    }
}

impl Js for MaterialShaderMapId {
    fn js(&self, out: &mut Vec<u8>) {
        object(out, |o| {
            o.f("QualityLevel", &self.quality_level)
                .f("FeatureLevel", &self.feature_level)
                .f("CookedShaderMapIdHash", &sha_hex(&self.cooked_shader_map_id_hash));
            object(o.key("LayoutParams"), |l| {
                l.f("MaxFieldAlignment", &self.max_field_alignment).f("Flags", &self.flags);
            });
        });
    }
}

/// A material's map record, its preshader program named by `data`.
fn material_record(out: &mut Vec<u8>, r: &MaterialShaderMapRecord, data: BodyRef) {
    object(out, |o| {
        o.f("ShaderMapId", &r.shader_map_id);
        content_keys(o, &r.content);
        o.f("OrderedMeshShaderMaps", &r.ordered_mesh_shader_maps);
        compilation_output(o.key("MaterialCompilationOutput"), &r.material_compilation_output, data);
        o.f("ShaderContentHash", &r.shader_content_hash).f("Code", &r.code);
    });
}

fn global_record(out: &mut Vec<u8>, r: &GlobalShaderMapRecord) {
    object(out, |o| {
        content_keys(o, &r.content);
        o.f("HashedSourceFilename", &r.hashed_source_filename).f("Code", &r.code);
    });
}
