//! Shader maps in their cooked archive form (4.26's frozen memory images): a material's inline shader maps (the tail
//! of a UMaterial / UMaterialInstance with bHasStaticPermutationResource) and the global shader cache's, their code
//! entries, and the frozen content walked into the shadermap kind's records.
//! It follows CUE4Parse 1.2.2's reads for a UE 4.26 cooked build (`ilspycmd -t` of
//! UMaterialInterface.DeserializeInlineShaderMaps, FMaterialResourceProxyReader, FMaterial, FShaderMapBase,
//! FMemoryImageResult, FShaderMapPointerTable, FShaderMapResourceCode, FGlobalShaderCache and the frozen classes under
//! FMemoryImageArchive). Every branch is CUE4Parse's for GAME_UE4_26 with ShaderMap.UseNewCookedFormat off.
//!
//! Two steps:
//! - [`read_inline_shader_maps`] / [`read_global_shader_cache`] read the archive form: per map its id, the
//!   FMemoryImageResult (the frozen object's bytes, its vtable and name patches), the pointer table and the code (each
//!   entry a view of its still-compressed bytes and where they sit). The views borrow the archive's buffer (a
//!   package's export data, or the global shader cache file): this step copies nothing.
//! - [`material_map_content`] / [`global_map_content`] walk the frozen bytes (FMemoryImageArchive): the shaders, the
//!   mesh maps, the compilation output. Each FName field is the name the map's ScriptNames / MinimalNames patch at its
//!   offset, while it is read (none is searched for in the bytes).
//!
//! [`decode_entry`] decodes one entry (LZ4, `lz4_flex`) and [`split_code`] splits it into the resource table, the DXBC
//! container and UE's optional data. Hashes in the records are upper-case hex, as the shadermap kind writes them.

use std::borrow::Cow;
use std::fmt::Write as _;

use indexmap::{IndexMap, IndexSet};

use crate::archive::{Archive, ReadError, Result};

/// A 64-bit type name hash (FHashedName) as the frozen layout stores it.
pub type Hash64 = u64;
/// A SHA-1 (FSHAHash) as stored.
pub type Sha1 = [u8; 20];

/// 16 upper-case hex digits.
pub fn hash64_hex(h: Hash64) -> String {
    format!("{h:016X}")
}

/// 40 upper-case hex digits.
pub fn sha_hex(h: &Sha1) -> String {
    let mut s = String::with_capacity(40);
    for b in h {
        let _ = write!(s, "{b:02X}");
    }
    s
}

/// The SHA-1 that 40 upper-case hex digits name (`None` for any other text: no hash the reader writes).
pub fn sha_of_hex(s: &str) -> Option<Sha1> {
    let b = s.as_bytes();
    if b.len() != 40 {
        return None;
    }
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    let mut out = [0u8; 20];
    for (i, o) in out.iter_mut().enumerate() {
        *o = digit(b[2 * i])? << 4 | digit(b[2 * i + 1])?;
    }
    Some(out)
}

// ---------------------------------------------------------------- the archive form

/// FMaterialResourceProxyReader: the archive with the shader maps' own name map (`None` for the global cache, whose
/// FString fields are plain strings).
struct ProxyReader<'n> {
    names: Option<&'n [String]>,
}

impl ProxyReader<'_> {
    /// FName through the name map: `name`, or `name_<n-1>` for number n.
    fn fname(&self, ar: &mut Archive<'_>) -> Result<String> {
        let Some(names) = self.names else {
            return Err(ReadError::parse(format!("{}: an FName in the shader cache, which has no name map", ar.name)));
        };
        let i = ar.i32()?;
        let n = ar.i32()?;
        let Some(base) = usize::try_from(i).ok().and_then(|i| names.get(i)) else {
            return Err(ReadError::parse(format!("{}: shader map FName index {i} of {}", ar.name, names.len())));
        };
        Ok(if n == 0 { base.clone() } else { format!("{base}_{}", i64::from(n) - 1) })
    }

    /// ReadFString: an FName through the map when there is one, else an FString.
    fn fstring(&self, ar: &mut Archive<'_>) -> Result<String> {
        if self.names.is_some() { self.fname(ar) } else { ar.fstring() }
    }
}

fn hex64(ar: &mut Archive<'_>) -> Result<Hash64> {
    ar.u64()
}

fn sha(ar: &mut Archive<'_>) -> Result<Sha1> {
    Ok(ar.bytes(20)?.try_into().expect("20 bytes"))
}

/// An int32 count, then that many elements; a count past the bytes left fails.
fn array<'a, T>(
    ar: &mut Archive<'a>,
    what: &str,
    mut read: impl FnMut(&mut Archive<'a>) -> Result<T>,
) -> Result<Vec<T>> {
    let n = ar.i32()?;
    let Some(n) = usize::try_from(n).ok().filter(|&n| n <= ar.remaining()) else {
        return Err(ReadError::parse(format!("{}: {what} count {n} at {}", ar.name, ar.pos - 4)));
    };
    let mut out = Vec::with_capacity(n.min(1024));
    for _ in 0..n {
        out.push(read(ar)?);
    }
    Ok(out)
}

/// FMemoryImageResult: the frozen bytes and their patches.
#[derive(Clone, Debug, Default)]
pub struct MemoryImage<'a> {
    pub frozen: &'a [u8],
    /// (TypeNameHash, its patches as (offset, vtable offset)).
    pub vtables: Vec<(Hash64, Vec<(i32, i32)>)>,
    /// (Name, its patch offsets).
    pub script_names: Vec<(String, Vec<i32>)>,
    pub minimal_names: Vec<(String, Vec<i32>)>,
}

/// One saved layout of FShaderMapPointerTable's TypeDependencies.
#[derive(Clone, Debug, Default)]
pub struct TypeDependency {
    pub name: Hash64,
    pub saved_layout_size: u32,
    pub saved_layout_hash: Sha1,
}

/// FShaderMapPointerTable (4.26, the old cooked format): the shader and vertex factory types an FShader's indexed
/// pointers index, and the frozen layouts the map was saved with.
#[derive(Clone, Debug, Default)]
pub struct PointerTable {
    pub types: Vec<Hash64>,
    pub vf_types: Vec<Hash64>,
    pub type_dependencies: Vec<TypeDependency>,
}

/// One FShaderEntry: its stored bytes (LZ4 unless as long as the code), where they start in the archive, the code's
/// size and its EShaderFrequency.
#[derive(Clone, Debug, Default)]
pub struct CodeEntry<'a> {
    pub code: &'a [u8],
    pub at: usize,
    pub uncompressed_size: i32,
    pub frequency: u8,
}

/// FShaderMapResourceCode. When the map shares its code (bShareCode) only the resource hash is stored.
#[derive(Clone, Debug, Default)]
pub struct ResourceCode<'a> {
    pub b_share_code: bool,
    pub resource_hash: Sha1,
    pub shader_hashes: Vec<Sha1>,
    pub entries: Vec<CodeEntry<'a>>,
}

/// FShaderMapBase in the archive form.
#[derive(Clone, Debug, Default)]
pub struct ShaderMapArchive<'a> {
    pub image: MemoryImage<'a>,
    pub pointers: PointerTable,
    pub code: ResourceCode<'a>,
}

/// A material's shader map: the archive form and its FMaterialShaderMapId.
#[derive(Clone, Debug, Default)]
pub struct MaterialShaderMapArchive<'a> {
    pub map: ShaderMapArchive<'a>,
    pub id: MaterialShaderMapId,
}

/// FMaterialShaderMapId as the cook stores it.
#[derive(Clone, Debug, Default)]
pub struct MaterialShaderMapId {
    pub quality_level: i32,
    pub feature_level: i32,
    pub cooked_shader_map_id_hash: Sha1,
    /// LayoutParams.
    pub max_field_alignment: u32,
    pub flags: i32,
}

fn read_image<'a>(p: &ProxyReader<'_>, ar: &mut Archive<'a>) -> Result<MemoryImage<'a>> {
    let len = ar.u32()?;
    let frozen = ar.bytes(len as usize)?;
    let nv = ar.i32()?;
    let ns = ar.i32()?;
    let nm = ar.i32()?;
    let (Ok(nv), Ok(ns), Ok(nm)) = (usize::try_from(nv), usize::try_from(ns), usize::try_from(nm)) else {
        return Err(ReadError::parse(format!("{}: memory image counts {nv} {ns} {nm}", ar.name)));
    };
    let mut vtables = Vec::new();
    for _ in 0..nv {
        let hash = hex64(ar)?;
        vtables.push((hash, array(ar, "vtable patch", |ar| Ok((ar.i32()?, ar.i32()?)))?));
    }
    let names = |ar: &mut Archive<'a>, n: usize| -> Result<Vec<(String, Vec<i32>)>> {
        let mut out = Vec::new();
        for _ in 0..n {
            let name = p.fstring(ar)?;
            out.push((name, array(ar, "name patch", |ar| ar.i32())?));
        }
        Ok(out)
    };
    let script_names = names(ar, ns)?;
    let minimal_names = names(ar, nm)?;
    Ok(MemoryImage { frozen, vtables, script_names, minimal_names })
}

fn read_pointers(ar: &mut Archive<'_>) -> Result<PointerTable> {
    let nt = ar.i32()?;
    let nv = ar.i32()?;
    let (Ok(nt), Ok(nv)) = (usize::try_from(nt), usize::try_from(nv)) else {
        return Err(ReadError::parse(format!("{}: pointer table counts {nt} {nv}", ar.name)));
    };
    let mut types = Vec::new();
    for _ in 0..nt {
        types.push(hex64(ar)?);
    }
    let mut vf_types = Vec::new();
    for _ in 0..nv {
        vf_types.push(hex64(ar)?);
    }
    let type_dependencies = array(ar, "type dependency", |ar| {
        Ok(TypeDependency { name: hex64(ar)?, saved_layout_size: ar.u32()?, saved_layout_hash: sha(ar)? })
    })?;
    Ok(PointerTable { types, vf_types, type_dependencies })
}

fn read_code<'a>(ar: &mut Archive<'a>) -> Result<ResourceCode<'a>> {
    let b_share_code = ar.bool()?;
    if b_share_code {
        return Ok(ResourceCode { b_share_code, resource_hash: sha(ar)?, ..ResourceCode::default() });
    }
    let resource_hash = sha(ar)?;
    let shader_hashes = array(ar, "shader hash", sha)?;
    let entries = array(ar, "shader entry", |ar| {
        let n = ar.i32()?;
        let at = ar.pos;
        let Ok(len) = usize::try_from(n) else {
            return Err(ReadError::parse(format!("{}: a shader entry of {n} bytes at {}", ar.name, at - 4)));
        };
        let code = ar.bytes(len)?;
        Ok(CodeEntry { code, at, uncompressed_size: ar.i32()?, frequency: ar.u8()? })
    })?;
    if entries.len() != shader_hashes.len() {
        return Err(ReadError::parse(format!(
            "{}: {} code entries for {} hashes",
            ar.name,
            entries.len(),
            shader_hashes.len()
        )));
    }
    Ok(ResourceCode { b_share_code, resource_hash, shader_hashes, entries })
}

/// FShaderMapBase.Deserialize (old cooked format): the memory image, its pointer table, the code.
fn read_map_base<'a>(p: &ProxyReader<'_>, ar: &mut Archive<'a>) -> Result<ShaderMapArchive<'a>> {
    let image = read_image(p, ar)?;
    let pointers = read_pointers(ar)?;
    Ok(ShaderMapArchive { image, pointers, code: read_code(ar)? })
}

/// UMaterialInterface.DeserializeInlineShaderMaps at the archive's place: the resource count, the proxy reader's name
/// map and locations, then per resource FMaterial.DeserializeInlineShaderMap: bCooked, bValid, the
/// FMaterialShaderMapId and the map. A resource that is not cooked or whose map is not valid is `None` (CUE4Parse
/// warns and keeps no map).
pub fn read_inline_shader_maps<'a>(ar: &mut Archive<'a>) -> Result<Vec<Option<MaterialShaderMapArchive<'a>>>> {
    let count = ar.i32()?;
    if count <= 0 {
        return Ok(Vec::new());
    }
    // FNameEntrySerialized: the string and its two 16-bit hashes (NAME_HASHES_SERIALIZED). The string is kept whole: a
    // parameter name can end in a space (M_Banner_MaskedLayers_Advanced's "Layer 1 OffsetZ (Set Bump Offset) "), and
    // an instance's override names it so (a trimmed name would miss the banners' overrides of it)
    let names = array(ar, "shader map name", |ar| {
        let s = ar.fstring()?;
        ar.skip(4)?;
        Ok(s)
    })?;
    let locs = ar.i32()?;
    let Ok(locs) = usize::try_from(locs) else {
        return Err(ReadError::parse(format!("{}: {locs} resource locations", ar.name)));
    };
    // FMaterialResourceLocOnDisk (uint32 offset, feature level byte, quality level byte, packed), then 4 bytes
    ar.skip(locs.checked_mul(6).and_then(|n| n.checked_add(4)).unwrap_or(usize::MAX))?;
    let p = ProxyReader { names: Some(&names) };
    let mut out = Vec::new();
    for _ in 0..count {
        // bCooked, then bValid only when cooked
        if !ar.bool()? || !ar.bool()? {
            out.push(None);
            continue;
        }
        let id = MaterialShaderMapId {
            quality_level: ar.i32()?,
            feature_level: ar.i32()?,
            cooked_shader_map_id_hash: sha(ar)?,
            max_field_alignment: ar.u32()?,
            flags: ar.i32()?,
        };
        out.push(Some(MaterialShaderMapArchive { map: read_map_base(&p, ar)?, id }));
    }
    Ok(out)
}

/// FGlobalShaderCache: the map count, then each FGlobalShaderMap (no id) through a proxy reader with no name map.
pub fn read_global_shader_cache<'a>(ar: &mut Archive<'a>) -> Result<Vec<ShaderMapArchive<'a>>> {
    let p = ProxyReader { names: None };
    array(ar, "global shader map", |ar| read_map_base(&p, ar))
}

// ---------------------------------------------------------------- the records

/// FShaderParameterInfo.
#[derive(Clone, Debug, Default)]
pub struct ShaderParameterInfo {
    pub base_index: u16,
    pub size: u16,
}

/// FShaderLooseParameterBufferInfo.
#[derive(Clone, Debug, Default)]
pub struct LooseParameterBuffer {
    pub base_index: u16,
    pub size: u16,
    pub parameters: Vec<ShaderParameterInfo>,
}

/// FShaderParameterMapInfo (4.26): what the RHI binds by register.
#[derive(Clone, Debug, Default)]
pub struct ShaderParameterMapInfo {
    pub uniform_buffers: Vec<ShaderParameterInfo>,
    pub texture_samplers: Vec<ShaderParameterInfo>,
    pub srvs: Vec<ShaderParameterInfo>,
    pub loose_parameter_buffers: Vec<LooseParameterBuffer>,
    pub hash: String,
}

/// FShaderParameterBindings' FParameter.
#[derive(Clone, Debug, Default)]
pub struct BindingParameter {
    pub buffer_index: u16,
    pub base_index: u16,
    pub byte_offset: u16,
    pub byte_size: u16,
}

/// FShaderParameterBindings' FResourceParameter.
#[derive(Clone, Debug, Default)]
pub struct ResourceParameter {
    pub byte_offset: u16,
    pub base_index: u8,
    pub base_type: u8,
}

/// FShaderParameterBindings' FParameterStructReference.
#[derive(Clone, Debug, Default)]
pub struct ParameterStructReference {
    pub buffer_index: u16,
    pub byte_offset: u16,
}

/// FShaderParameterBindings (4.26).
#[derive(Clone, Debug, Default)]
pub struct ShaderParameterBindings {
    pub parameters: Vec<BindingParameter>,
    pub resource_parameters: Vec<ResourceParameter>,
    pub graph_uniform_buffers: Vec<ParameterStructReference>,
    pub parameter_references: Vec<ParameterStructReference>,
    pub structure_layout_hash: u32,
    pub root_parameter_buffer_index: u16,
}

/// A frozen FShader.
#[derive(Clone, Debug, Default)]
pub struct ShaderRecord {
    pub type_: String,
    /// `None` for a shader with no vertex factory (a material shader's null pointer, which CUE4Parse reads as index 0).
    pub vf_type: Option<String>,
    /// ShaderPermutations at the shader's place in its map (0 in a pipeline: the pipeline's PermutationIds hold it).
    pub permutation: i32,
    /// FShaderTarget: 4.26's EShaderFrequency and EShaderPlatform.
    pub frequency: u32,
    pub platform: u32,
    pub resource_index: i32,
    /// ShaderHashes[ResourceIndex]; `None` when the map shares its code (bShareCode).
    pub hash: Option<String>,
    pub num_instructions: u32,
    pub bindings: ShaderParameterBindings,
    pub parameter_map_info: ShaderParameterMapInfo,
    pub uniform_buffer_parameter_structs: Vec<String>,
    /// FShaderUniformBufferParameter BaseIndex, one per struct.
    pub uniform_buffer_parameters: Vec<u16>,
}

#[derive(Clone, Debug, Default)]
pub struct ShaderPipelineRecord {
    pub type_name: String,
    /// One per graphics frequency (vs, hs, ds, ps, gs); `None` where the pipeline has none.
    pub shaders: Vec<Option<ShaderRecord>>,
    pub permutation_ids: Vec<i32>,
}

/// FShaderMapContent.
#[derive(Clone, Debug, Default)]
pub struct ShaderMapContentRecord {
    pub shaders: Vec<ShaderRecord>,
    pub shader_pipelines: Vec<ShaderPipelineRecord>,
    pub shader_platform: u8,
}

/// FMeshMaterialShaderMap.
#[derive(Clone, Debug, Default)]
pub struct MeshShaderMapRecord {
    pub content: ShaderMapContentRecord,
    pub vertex_factory_type_name: String,
}

/// FMemoryImageMaterialParameterInfo.
#[derive(Clone, Debug, Default)]
pub struct MaterialParameterInfo {
    pub name: String,
    pub index: i32,
    pub association: u8,
}

#[derive(Clone, Debug, Default)]
pub struct MaterialScalarParameterInfo {
    pub parameter_info: MaterialParameterInfo,
    pub default_value: f32,
}

#[derive(Clone, Debug, Default)]
pub struct MaterialVectorParameterInfo {
    pub parameter_info: MaterialParameterInfo,
    /// R, G, B, A.
    pub default_value: [f32; 4],
}

#[derive(Clone, Debug, Default)]
pub struct MaterialTextureParameterInfo {
    pub parameter_info: MaterialParameterInfo,
    pub texture_index: i32,
    pub sampler_source: u8,
    pub virtual_texture_layer_index: u8,
}

#[derive(Clone, Debug, Default)]
pub struct MaterialExternalTextureParameterInfo {
    pub parameter_name: String,
    /// "AAAAAAAA-BBBBBBBB-CCCCCCCC-DDDDDDDD".
    pub external_texture_guid: String,
    pub source_texture_index: i32,
}

#[derive(Clone, Debug, Default)]
pub struct UniformPreshaderHeader {
    pub opcode_offset: u32,
    pub opcode_size: u32,
}

/// FRHIUniformBufferLayoutInitializer (4.26). Each resource list is FRHIUniformBufferResource's
/// (MemberOffset, MemberType).
#[derive(Clone, Debug, Default)]
pub struct UniformBufferLayout {
    pub name: String,
    pub constant_buffer_size: u32,
    pub static_slot: u8,
    pub render_targets_offset: u16,
    pub b_has_non_graph_outputs: bool,
    pub resources: Vec<(u16, u8)>,
    pub graph_resources: Vec<(u16, u8)>,
    pub graph_textures: Vec<(u16, u8)>,
    pub graph_buffers: Vec<(u16, u8)>,
    pub graph_uniform_buffers: Vec<(u16, u8)>,
    pub uniform_buffers: Vec<(u16, u8)>,
    pub hash: u32,
}

/// FMaterialVirtualTextureStack.
#[derive(Clone, Debug, Default)]
pub struct VtStack {
    pub num_layers: u32,
    pub layer_uniform_expression_indices: [i32; 8],
    pub preallocated_stack_texture_index: i32,
}

/// FUniformExpressionSet (4.26), its preshader program's bytes apart (the map content's `preshaders`).
#[derive(Clone, Debug, Default)]
pub struct UniformExpressionSet {
    pub uniform_vector_preshaders: Vec<UniformPreshaderHeader>,
    pub uniform_scalar_preshaders: Vec<UniformPreshaderHeader>,
    pub uniform_scalar_parameters: Vec<MaterialScalarParameterInfo>,
    pub uniform_vector_parameters: Vec<MaterialVectorParameterInfo>,
    /// By EMaterialTextureParameterType: Standard2D, Cube, Array2D, Volume, Virtual.
    pub uniform_texture_parameters: Vec<Vec<MaterialTextureParameterInfo>>,
    pub uniform_external_texture_parameters: Vec<MaterialExternalTextureParameterInfo>,
    /// FMaterialPreshaderData's Names (its opcodes index them).
    pub preshader_names: Vec<String>,
    pub vt_stacks: Vec<VtStack>,
    pub parameter_collections: Vec<String>,
    pub uniform_buffer_layout_initializer: UniformBufferLayout,
}

#[derive(Clone, Debug, Default)]
pub struct MaterialCompilationOutput {
    pub uniform_expression_set: UniformExpressionSet,
    pub used_scene_textures: u32,
    pub used_d_buffer_textures: u8,
    pub runtime_virtual_texture_output_attribute_mask: u8,
    /// The two packed bytes after it (bNeedsSceneTextures / bUsesEyeAdaptation ... as the build packs them).
    pub b1: u8,
    pub b2: u8,
}

/// FShaderMapResourceCode, less the code: one hash and frequency per entry.
#[derive(Clone, Debug, Default)]
pub struct ShaderMapCode {
    pub b_share_code: bool,
    pub resource_hash: String,
    pub shader_hashes: Vec<String>,
    pub frequencies: Vec<u8>,
}

/// FMaterialShaderMap: its id, its frozen FMaterialShaderMapContent, its code's hashes.
#[derive(Clone, Debug, Default)]
pub struct MaterialShaderMapRecord {
    pub shader_map_id: MaterialShaderMapId,
    pub content: ShaderMapContentRecord,
    pub ordered_mesh_shader_maps: Vec<MeshShaderMapRecord>,
    pub material_compilation_output: MaterialCompilationOutput,
    pub shader_content_hash: String,
    pub code: ShaderMapCode,
}

/// A material map's record with its preshader bytes apart: the writer lays them
/// out as a section and names it in the record.
#[derive(Clone, Debug, Default)]
pub struct MaterialMapContent {
    pub record: MaterialShaderMapRecord,
    pub preshaders: Vec<u8>,
}

/// A global shader map section (FGlobalShaderMapContent): its shaders, by the source file they come from.
#[derive(Clone, Debug, Default)]
pub struct GlobalShaderMapRecord {
    pub content: ShaderMapContentRecord,
    pub hashed_source_filename: String,
    pub code: ShaderMapCode,
}

// ---------------------------------------------------------------- the frozen content

const fn align(n: i64, a: i64) -> i64 {
    (n + a - 1) & !(a - 1)
}

/// FMemoryImageArchive over a map's frozen bytes. Positions are signed: a frozen pointer's offset can lead before
/// the start, which the next read reports.
struct Frozen<'m> {
    pos: i64,
    bytes: &'m [u8],
    /// Frozen offset -> (name, a script name (12 bytes) or a minimal one (8)).
    names: IndexMap<i64, (&'m str, bool)>,
    /// The patched offsets read so far (every one is, once the map is read: `names_read`).
    used: IndexSet<i64>,
    pointers: &'m PointerTable,
    what: &'m str,
}

/// A frozen array's head: where its data starts and its count.
struct Head {
    at: i64,
    n: usize,
}

impl<'m> Frozen<'m> {
    fn new(image: &'m MemoryImage<'_>, pointers: &'m PointerTable, what: &'m str) -> Frozen<'m> {
        let mut names = IndexMap::new();
        for (name, patches) in &image.script_names {
            for &o in patches {
                names.insert(i64::from(o), (name.as_str(), true));
            }
        }
        for (name, patches) in &image.minimal_names {
            for &o in patches {
                names.insert(i64::from(o), (name.as_str(), false));
            }
        }
        Frozen { pos: 0, bytes: image.frozen, names, used: IndexSet::new(), pointers, what }
    }

    fn take(&mut self, n: usize) -> Result<&'m [u8]> {
        let at = usize::try_from(self.pos).ok().filter(|&p| p.checked_add(n).is_some_and(|e| e <= self.bytes.len()));
        let Some(at) = at else {
            return Err(ReadError::parse(format!(
                "{}: frozen read of {n} at {} past {}",
                self.what,
                self.pos,
                self.bytes.len()
            )));
        };
        self.pos += n as i64;
        Ok(&self.bytes[at..at + n])
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().expect("2 bytes")))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }
    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }
    fn hex64(&mut self) -> Result<String> {
        let lo = self.u32()?;
        let hi = self.u32()?;
        Ok(hash64_hex(u64::from(hi) << 32 | u64::from(lo)))
    }
    /// A TIndexedPtr: its index (the uint64 shifted right by one), -1 for a null one (0).
    fn index(&mut self) -> Result<i64> {
        let lo = self.u32()?;
        let hi = self.u32()?;
        Ok(if lo == 0 && hi == 0 { -1 } else { i64::from(hi) * 2_147_483_648 + i64::from(lo >> 1) })
    }
    fn sha(&mut self) -> Result<String> {
        Ok(sha_hex(self.take(20)?.try_into().expect("20 bytes")))
    }
    fn guid(&mut self) -> Result<String> {
        Ok(format!("{:08X}-{:08X}-{:08X}-{:08X}", self.u32()?, self.u32()?, self.u32()?, self.u32()?))
    }
    fn align(&mut self, a: i64) {
        self.pos = align(self.pos, a);
    }
    /// FFrozenMemoryImagePtr (4.26): bit 0 frozen, the rest the signed offset from the pointer itself.
    fn ptr(&mut self) -> Result<(bool, i64)> {
        let lo = self.u32()?;
        let hi = self.i32()?;
        Ok((lo & 1 != 0, i64::from(hi) * 2_147_483_648 + i64::from(lo >> 1)))
    }
    /// A TMemoryImageArray header: where its data starts and its count (Num == Max, or the read is wrong).
    fn array_head(&mut self) -> Result<Head> {
        let start = self.pos;
        let (_, offset) = self.ptr()?;
        let n = self.i32()?;
        let max = self.i32()?;
        if n != max {
            return Err(ReadError::parse(format!("{}: frozen array at {start}: Num {n} != Max {max}", self.what)));
        }
        let Ok(n) = usize::try_from(n) else {
            return Err(ReadError::parse(format!("{}: frozen array at {start}: Num {n}", self.what)));
        };
        Ok(Head { at: start + offset, n })
    }
    /// ReadArray(getter): each element read in turn; with `realign`, the position aligned to 4 after each.
    fn array<T>(&mut self, realign: bool, mut read: impl FnMut(&mut Self) -> Result<T>) -> Result<Vec<T>> {
        let Head { at, n } = self.array_head()?;
        let mut out = Vec::new();
        if n == 0 {
            return Ok(out);
        }
        let back = self.pos;
        self.pos = at;
        for _ in 0..n {
            out.push(read(self)?);
            if realign {
                self.align(4);
            }
        }
        self.pos = back;
        Ok(out)
    }
    /// ReadArray<T>(): `n` packed elements of `size` bytes, each read by `read` from its own start.
    fn raw<T>(&mut self, size: i64, mut read: impl FnMut(&mut Self) -> Result<T>) -> Result<Vec<T>> {
        let Head { at, n } = self.array_head()?;
        let mut out = Vec::new();
        if n == 0 {
            return Ok(out);
        }
        let back = self.pos;
        for i in 0..n {
            self.pos = at + i as i64 * size;
            out.push(read(self)?);
        }
        self.pos = back;
        Ok(out)
    }
    /// ReadArrayOfPtrs: an array of frozen pointers, each followed to its object.
    fn ptrs<T>(&mut self, mut read: impl FnMut(&mut Self) -> Result<T>) -> Result<Vec<T>> {
        let Head { at, n } = self.array_head()?;
        let mut out = Vec::new();
        if n == 0 {
            return Ok(out);
        }
        let back = self.pos;
        self.pos = at;
        for _ in 0..n {
            let here = self.pos;
            let (_, offset) = self.ptr()?;
            self.pos = here + offset;
            out.push(read(self)?);
            self.pos = align(here + 8, 8);
        }
        self.pos = back;
        Ok(out)
    }
    /// Fails unless every offset a name patch names was read as an FName: a read that misplaces a field meets a name
    /// at another offset or none.
    fn names_read(&self) -> Result<()> {
        for (o, (name, _)) in &self.names {
            if !self.used.contains(o) {
                return Err(ReadError::parse(format!(
                    "{}: the name {name} patched at {o} is not where the read met an FName",
                    self.what
                )));
            }
        }
        Ok(())
    }
    /// ReadHashTable: two pointers and two counts, the table itself not read (24 bytes).
    fn hash_table(&mut self) {
        self.pos += 24;
    }
    /// FMemoryImageArchive.ReadFString: a frozen UTF-16 string.
    fn fstring(&mut self) -> Result<String> {
        let Head { at, n } = self.array_head()?;
        if n <= 1 {
            return Ok(String::new());
        }
        let span = usize::try_from(at).ok().and_then(|a| Some((a, a.checked_add(n.checked_mul(2)?)?)));
        let Some((start, end)) = span.filter(|&(_, e)| e <= self.bytes.len()) else {
            return Err(ReadError::parse(format!("{}: frozen FString at {at} past the bytes", self.what)));
        };
        let units: Vec<u16> =
            self.bytes[start..end].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        if units[units.len() - 1] != 0 {
            return Err(ReadError::parse(format!("{}: frozen FString at {at} not null terminated", self.what)));
        }
        Ok(String::from_utf16_lossy(&units[..units.len() - 1]))
    }
    /// ReadFName: the name a patch puts at this offset (12 bytes for a script name, 8 for a minimal one), else "None"
    /// over 12 bytes.
    fn fname(&mut self) -> String {
        if let Some(&(name, script)) = self.names.get(&self.pos) {
            self.used.insert(self.pos);
            self.pos += if script { 12 } else { 8 };
            return name.to_owned();
        }
        self.pos += 12;
        "None".to_owned()
    }
}

fn parameter_info(f: &mut Frozen<'_>) -> Result<ShaderParameterInfo> {
    Ok(ShaderParameterInfo { base_index: f.u16()?, size: f.u16()? })
}

fn struct_reference(f: &mut Frozen<'_>) -> Result<ParameterStructReference> {
    Ok(ParameterStructReference { buffer_index: f.u16()?, byte_offset: f.u16()? })
}

fn bindings(f: &mut Frozen<'_>) -> Result<ShaderParameterBindings> {
    let parameters = f.raw(8, |f| {
        Ok(BindingParameter {
            buffer_index: f.u16()?,
            base_index: f.u16()?,
            byte_offset: f.u16()?,
            byte_size: f.u16()?,
        })
    })?;
    let resource_parameters =
        f.array(true, |f| Ok(ResourceParameter { byte_offset: f.u16()?, base_index: f.u8()?, base_type: f.u8()? }))?;
    let graph_uniform_buffers = f.raw(4, struct_reference)?;
    let parameter_references = f.raw(4, struct_reference)?;
    let structure_layout_hash = f.u32()?;
    let root_parameter_buffer_index = f.u16()?;
    f.align(4);
    Ok(ShaderParameterBindings {
        parameters,
        resource_parameters,
        graph_uniform_buffers,
        parameter_references,
        structure_layout_hash,
        root_parameter_buffer_index,
    })
}

fn parameter_map(f: &mut Frozen<'_>) -> Result<ShaderParameterMapInfo> {
    let uniform_buffers = f.array(false, parameter_info)?;
    let texture_samplers = f.array(false, parameter_info)?;
    let srvs = f.array(false, parameter_info)?;
    let loose_parameter_buffers = f.array(true, |f| {
        let base_index = f.u16()?;
        let size = f.u16()?;
        f.align(8);
        Ok(LooseParameterBuffer { base_index, size, parameters: f.raw(4, parameter_info)? })
    })?;
    Ok(ShaderParameterMapInfo { uniform_buffers, texture_samplers, srvs, loose_parameter_buffers, hash: f.hex64()? })
}

/// The code hashes a map's shaders name by ResourceIndex (`None` when the map shares its code).
type Hashes<'h> = Option<&'h [Sha1]>;

fn shader(f: &mut Frozen<'_>, hashes: Hashes<'_>) -> Result<ShaderRecord> {
    let bindings = bindings(f)?;
    let parameter_map_info = parameter_map(f)?;
    let uniform_buffer_parameter_structs = f.array(true, Frozen::hex64)?;
    let uniform_buffer_parameters = f.raw(2, Frozen::u16)?;
    let t = f.index()?;
    let v = f.index()?;
    // an index past the table is the hash itself in CUE4Parse (FHashedName(num)): an error here; a null vertex factory
    // pointer (a material shader's) is None, where CUE4Parse reads index 0
    let pick = |list: &[Hash64], i: i64, what: &str| -> Result<String> {
        match usize::try_from(i).ok().and_then(|i| list.get(i)) {
            Some(&h) => Ok(hash64_hex(h)),
            None => Err(ReadError::parse(format!(
                "{}: shader {what} index {i} of the pointer table's {}",
                f.what,
                list.len()
            ))),
        }
    };
    let type_ = pick(&f.pointers.types, t, "type")?;
    let vf_type = if v < 0 { None } else { Some(pick(&f.pointers.vf_types, v, "vertex factory")?) };
    let target = f.u32()?;
    let resource_index = f.i32()?;
    let num_instructions = f.u32()?;
    let hash = match hashes {
        None => None,
        Some(h) => match usize::try_from(resource_index).ok().and_then(|i| h.get(i)) {
            Some(s) => Some(sha_hex(s)),
            None => {
                return Err(ReadError::parse(format!(
                    "{}: shader resource index {resource_index} of {} code entries",
                    f.what,
                    h.len()
                )));
            }
        },
    };
    Ok(ShaderRecord {
        type_,
        vf_type,
        permutation: 0,
        frequency: target & 0xf,
        platform: (target >> 4) & 0xffff,
        resource_index,
        hash,
        num_instructions,
        bindings,
        parameter_map_info,
        uniform_buffer_parameter_structs,
        uniform_buffer_parameters,
    })
}

fn pipeline(f: &mut Frozen<'_>, hashes: Hashes<'_>) -> Result<ShaderPipelineRecord> {
    let type_name = f.hex64()?;
    let mut shaders = Vec::with_capacity(5);
    for _ in 0..5 {
        let here = f.pos;
        let (frozen, offset) = f.ptr()?;
        if frozen {
            f.pos = here + offset;
            shaders.push(Some(shader(f, hashes)?));
        } else {
            shaders.push(None);
        }
        f.pos = align(here + 8, 8);
    }
    let mut permutation_ids = Vec::with_capacity(5);
    for _ in 0..5 {
        permutation_ids.push(f.i32()?);
    }
    Ok(ShaderPipelineRecord { type_name, shaders, permutation_ids })
}

/// FShaderMapContent.Deserialize (4.26). The type list must be the shaders' own types, in their order.
fn content(f: &mut Frozen<'_>, hashes: Hashes<'_>) -> Result<ShaderMapContentRecord> {
    f.hash_table();
    let types = f.array(true, Frozen::hex64)?;
    let perms = f.raw(4, Frozen::i32)?;
    let mut shaders = f.ptrs(|f| shader(f, hashes))?;
    let shader_pipelines = f.ptrs(|f| pipeline(f, hashes))?;
    let shader_platform = f.u8()?;
    f.align(8);
    if types.len() != shaders.len() || perms.len() != shaders.len() {
        return Err(ReadError::parse(format!(
            "{}: {} types, {} permutations, {} shaders",
            f.what,
            types.len(),
            perms.len(),
            shaders.len()
        )));
    }
    for (i, s) in shaders.iter_mut().enumerate() {
        if s.type_ != types[i] {
            return Err(ReadError::parse(format!(
                "{}: shader {i} is of type {}, the map lists {}",
                f.what, s.type_, types[i]
            )));
        }
        s.permutation = perms[i];
    }
    Ok(ShaderMapContentRecord { shaders, shader_pipelines, shader_platform })
}

fn material_parameter_info(f: &mut Frozen<'_>) -> Result<MaterialParameterInfo> {
    let name = f.fname();
    let index = f.i32()?;
    let association = f.u8()?;
    f.align(4);
    Ok(MaterialParameterInfo { name, index, association })
}

fn preshader(f: &mut Frozen<'_>) -> Result<UniformPreshaderHeader> {
    Ok(UniformPreshaderHeader { opcode_offset: f.u32()?, opcode_size: f.u32()? })
}

fn texture_parameter(f: &mut Frozen<'_>) -> Result<MaterialTextureParameterInfo> {
    let parameter_info = material_parameter_info(f)?;
    let texture_index = f.i32()?;
    let sampler_source = f.u8()?;
    let virtual_texture_layer_index = f.u8()?;
    f.align(4);
    Ok(MaterialTextureParameterInfo { parameter_info, texture_index, sampler_source, virtual_texture_layer_index })
}

fn layout(f: &mut Frozen<'_>) -> Result<UniformBufferLayout> {
    let constant_buffer_size = f.u32()?;
    let static_slot = f.u8()?;
    f.pos += 1;
    let render_targets_offset = f.u16()?;
    let b_has_non_graph_outputs = f.u8()? != 0;
    f.align(8);
    let res = |f: &mut Frozen<'_>| f.raw(4, |f| Ok((f.u16()?, f.u8()?)));
    let resources = res(f)?;
    let graph_resources = res(f)?;
    let graph_textures = res(f)?;
    let graph_buffers = res(f)?;
    let graph_uniform_buffers = res(f)?;
    let uniform_buffers = res(f)?;
    f.u32()?;
    f.align(8);
    let name = f.fstring()?;
    let hash = f.u32()?;
    f.align(8);
    Ok(UniformBufferLayout {
        name,
        constant_buffer_size,
        static_slot,
        render_targets_offset,
        b_has_non_graph_outputs,
        resources,
        graph_resources,
        graph_textures,
        graph_buffers,
        graph_uniform_buffers,
        uniform_buffers,
        hash,
    })
}

/// FUniformExpressionSet (4.26); the preshader bytes come back apart, for their section.
fn expression_set(f: &mut Frozen<'_>) -> Result<(UniformExpressionSet, Vec<u8>)> {
    let uniform_vector_preshaders = f.array(true, preshader)?;
    let uniform_scalar_preshaders = f.array(true, preshader)?;
    let uniform_scalar_parameters = f.array(true, |f| {
        let parameter_info = material_parameter_info(f)?;
        // 4-aligned, 24 bytes (CUE4Parse 1.2.2 aligns each to 8: with the array at an offset of 4 mod 8 it reads every
        // element after the first 4 bytes late; the name patches sit at 24-byte steps)
        let default_value = f.f32()?;
        Ok(MaterialScalarParameterInfo { parameter_info, default_value })
    })?;
    let uniform_vector_parameters = f.array(true, |f| {
        let parameter_info = material_parameter_info(f)?;
        Ok(MaterialVectorParameterInfo { parameter_info, default_value: [f.f32()?, f.f32()?, f.f32()?, f.f32()?] })
    })?;
    let mut uniform_texture_parameters = Vec::with_capacity(5);
    for _ in 0..5 {
        uniform_texture_parameters.push(f.array(true, texture_parameter)?);
    }
    let uniform_external_texture_parameters = f.array(true, |f| {
        Ok(MaterialExternalTextureParameterInfo {
            parameter_name: f.fname(),
            external_texture_guid: f.guid()?,
            source_texture_index: f.i32()?,
        })
    })?;
    let preshader_names = f.array(true, |f| Ok(f.fname()))?;
    let data = f.raw(1, Frozen::u8)?;
    let vt_stacks = f.array(true, |f| {
        let num_layers = f.u32()?;
        let mut layer_uniform_expression_indices = [0; 8];
        for v in &mut layer_uniform_expression_indices {
            *v = f.i32()?;
        }
        Ok(VtStack { num_layers, layer_uniform_expression_indices, preallocated_stack_texture_index: f.i32()? })
    })?;
    let parameter_collections = f.raw(16, Frozen::guid)?;
    let uniform_buffer_layout_initializer = layout(f)?;
    let set = UniformExpressionSet {
        uniform_vector_preshaders,
        uniform_scalar_preshaders,
        uniform_scalar_parameters,
        uniform_vector_parameters,
        uniform_texture_parameters,
        uniform_external_texture_parameters,
        preshader_names,
        vt_stacks,
        parameter_collections,
        uniform_buffer_layout_initializer,
    };
    Ok((set, data))
}

fn compilation_output(f: &mut Frozen<'_>) -> Result<(MaterialCompilationOutput, Vec<u8>)> {
    let (uniform_expression_set, data) = expression_set(f)?;
    let used_scene_textures = f.u32()?;
    let used_d_buffer_textures = f.u8()?;
    let runtime_virtual_texture_output_attribute_mask = f.u8()?;
    let b1 = f.u8()?;
    let b2 = f.u8()?;
    f.align(8);
    let out = MaterialCompilationOutput {
        uniform_expression_set,
        used_scene_textures,
        used_d_buffer_textures,
        runtime_virtual_texture_output_attribute_mask,
        b1,
        b2,
    };
    Ok((out, data))
}

fn map_code(c: &ResourceCode<'_>) -> ShaderMapCode {
    ShaderMapCode {
        b_share_code: c.b_share_code,
        resource_hash: sha_hex(&c.resource_hash),
        shader_hashes: c.shader_hashes.iter().map(sha_hex).collect(),
        frequencies: c.entries.iter().map(|e| e.frequency).collect(),
    }
}

fn hashes_of<'h>(c: &'h ResourceCode<'_>) -> Hashes<'h> {
    if c.b_share_code { None } else { Some(&c.shader_hashes) }
}

/// FMaterialShaderMapContent.Deserialize (4.26) over a map's frozen bytes, read to their end.
pub fn material_map_content(map: &MaterialShaderMapArchive<'_>, what: &str) -> Result<MaterialMapContent> {
    let m = &map.map;
    let mut f = Frozen::new(&m.image, &m.pointers, what);
    let hashes = hashes_of(&m.code);
    let base = content(&mut f, hashes)?;
    let ordered_mesh_shader_maps = f.ptrs(|f| {
        let mesh = content(f, hashes)?;
        Ok(MeshShaderMapRecord { content: mesh, vertex_factory_type_name: f.hex64()? })
    })?;
    let (material_compilation_output, preshaders) = compilation_output(&mut f)?;
    let shader_content_hash = f.sha()?;
    f.names_read()?;
    let record = MaterialShaderMapRecord {
        shader_map_id: map.id.clone(),
        content: base,
        ordered_mesh_shader_maps,
        material_compilation_output,
        shader_content_hash,
        code: map_code(&m.code),
    };
    Ok(MaterialMapContent { record, preshaders })
}

/// FGlobalShaderMapContent.Deserialize over a global map section's frozen bytes.
pub fn global_map_content(map: &ShaderMapArchive<'_>, what: &str) -> Result<GlobalShaderMapRecord> {
    let mut f = Frozen::new(&map.image, &map.pointers, what);
    let base = content(&mut f, hashes_of(&map.code))?;
    let hashed_source_filename = f.hex64()?;
    f.names_read()?;
    Ok(GlobalShaderMapRecord { content: base, hashed_source_filename, code: map_code(&map.code) })
}

// ---------------------------------------------------------------- the code

/// The decoded bytes of a code entry: LZ4 unless stored as long as the code.
pub fn decode_entry<'a>(e: &CodeEntry<'a>) -> Result<Cow<'a, [u8]>> {
    if i64::try_from(e.code.len()).ok() == Some(i64::from(e.uncompressed_size)) {
        return Ok(Cow::Borrowed(e.code));
    }
    let Ok(size) = usize::try_from(e.uncompressed_size) else {
        return Err(ReadError::parse(format!("lz4: an entry of {} bytes uncompressed", e.uncompressed_size)));
    };
    let mut out = vec![0u8; size];
    let n = lz4_flex::block::decompress_into(e.code, &mut out)
        .map_err(|err| ReadError::parse(format!("lz4: the code at {}: {err}", e.at)))?;
    if n != size {
        return Err(ReadError::parse(format!("lz4: the block decodes to {n} bytes, not {size}")));
    }
    Ok(Cow::Owned(out))
}

/// EShaderFrequency (4.26) of the first ray tracing frequency (SF_RayGen): its code holds three entry point names
/// between the resource table and the container.
const SF_RAY_GEN: u8 = 6;

/// FD3D11ShaderResourceTable as the D3D11 shader compiler writes it before the container.
#[derive(Clone, Debug, Default)]
pub struct ShaderResourceTable {
    pub resource_table_bits: u32,
    pub shader_resource_view_map: Vec<u32>,
    pub sampler_map: Vec<u32>,
    pub unordered_access_view_map: Vec<u32>,
    pub resource_table_layout_hashes: Vec<u32>,
    pub texture_map: Vec<u32>,
}

/// A code entry's decoded bytes split: where the DXBC container lies and the header fields
/// around it.
#[derive(Clone, Debug, Default)]
pub struct SplitCode {
    pub container_at: usize,
    pub container_size: usize,
    pub frequency: u8,
    pub resource_table: ShaderResourceTable,
    /// A ray tracing shader's: RayEntryPoint, RayAnyHitEntryPoint, RayIntersectionEntryPoint.
    pub ray_entry_points: Option<Vec<String>>,
    /// The optional data's 'p' item (FShaderCodePackedResourceCounts) as the RHI reads it: byte 0, whether the loose
    /// $Globals buffer is committed, and the u16 at 6, the output mask. Byte 5 is padding the cook left unset.
    pub packed: Option<(bool, u16)>,
    /// The optional data's 'u' item: the name of each constant-buffer register.
    pub uniform_buffer_names: Option<Vec<String>>,
    /// The optional data's 'v' item as lower-case hex, when it is not empty.
    pub vendor_extensions: Option<String>,
    /// The optional data's '6' item is set: the D3D11 RHI makes no shader of it.
    pub no_create: bool,
}

/// A reader over decoded code bytes.
struct Code<'c> {
    code: &'c [u8],
    p: usize,
    what: &'c str,
}

impl Code<'_> {
    fn u32(&mut self) -> Result<u32> {
        if self.p + 4 > self.code.len() {
            return Err(ReadError::parse(format!("{}: read past the code at {}", self.what, self.p)));
        }
        let v = u32::from_le_bytes(self.code[self.p..self.p + 4].try_into().expect("4 bytes"));
        self.p += 4;
        Ok(v)
    }

    fn list(&mut self) -> Result<Vec<u32>> {
        let n = self.u32()?;
        if u64::from(n) * 4 > (self.code.len() - self.p) as u64 {
            return Err(ReadError::parse(format!("{}: resource table map of {n}", self.what)));
        }
        (0..n).map(|_| self.u32()).collect()
    }

    /// An FString of ANSI characters (the compiler's names).
    fn str(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        if n == 0 {
            return Ok(String::new());
        }
        if n > self.code.len() - self.p || self.code[self.p + n - 1] != 0 {
            return Err(ReadError::parse(format!("{}: a name of {n} bytes at {}", self.what, self.p - 4)));
        }
        let s = self.code[self.p..self.p + n - 1].iter().map(|&b| char::from(b)).collect();
        self.p += n;
        Ok(s)
    }
}

/// An entry's decoded bytes split: the FD3D11ShaderResourceTable, for a ray tracing shader its entry points (the
/// global cache's 7, FStrings RayEntryPoint, RayAnyHitEntryPoint, RayIntersectionEntryPoint as the D3D shader compiler
/// writes them), the DXBC container, then UE's optional data (CodeSize = the bytes - the table - OptionalDataSize, the
/// last int32, which counts itself; each item a key byte, an int32 size and its bytes).
pub fn split_code(code: &[u8], frequency: u8, what: &str) -> Result<SplitCode> {
    let mut c = Code { code, p: 0, what };
    let resource_table = ShaderResourceTable {
        resource_table_bits: c.u32()?,
        shader_resource_view_map: c.list()?,
        sampler_map: c.list()?,
        unordered_access_view_map: c.list()?,
        resource_table_layout_hashes: c.list()?,
        texture_map: c.list()?,
    };
    let ray_entry_points = if frequency >= SF_RAY_GEN { Some(vec![c.str()?, c.str()?, c.str()?]) } else { None };
    let container_at = c.p;
    if code.len() < container_at + 32 || &code[container_at..container_at + 4] != b"DXBC" {
        return Err(ReadError::parse(format!("{what}: no DXBC container at {container_at}, after the resource table")));
    }
    let le32 = |at: usize| u32::from_le_bytes(code[at..at + 4].try_into().expect("4 bytes"));
    let container_size = le32(container_at + 24) as usize;
    let optional = le32(code.len() - 4).cast_signed();
    let end = code.len() - 4;
    if optional < 4 || (container_at + container_size) as i64 + i64::from(optional) != code.len() as i64 {
        return Err(ReadError::parse(format!(
            "{what}: {} bytes are not the table and names ({container_at}), the container ({container_size}) and the \
             optional data ({optional})",
            code.len()
        )));
    }
    let mut out =
        SplitCode { container_at, container_size, frequency, resource_table, ray_entry_points, ..SplitCode::default() };
    // the optional data's items
    c.p = container_at + container_size;
    while c.p < end {
        let p = c.p;
        if p + 5 > end {
            return Err(ReadError::parse(format!("{what}: optional data item at {p} past its end")));
        }
        let key = code[p];
        let size = le32(p + 1).cast_signed();
        let at = p + 5;
        let Some(size) = usize::try_from(size).ok().filter(|&s| at + s <= end) else {
            return Err(ReadError::parse(format!("{what}: optional data item '{}' of {size} at {p}", char::from(key))));
        };
        match key {
            b'p' => {
                if size != 8 {
                    return Err(ReadError::parse(format!("{what}: the 'p' item is {size} bytes, not 8")));
                }
                let packed = &code[at..at + 8];
                out.packed = Some((packed[0] != 0, u16::from(packed[6]) | u16::from(packed[7]) << 8));
            }
            b'u' => {
                c.p = at;
                let n = c.u32()?;
                let mut names = Vec::new();
                for _ in 0..n {
                    names.push(c.str()?);
                }
                if c.p != at + size {
                    return Err(ReadError::parse(format!(
                        "{what}: the 'u' item's {n} names end at {}, the item at {}",
                        c.p,
                        at + size
                    )));
                }
                out.uniform_buffer_names = Some(names);
            }
            b'v' => {
                let mut hex = String::with_capacity(size * 2);
                for b in &code[at..at + size] {
                    let _ = write!(hex, "{b:02x}");
                }
                out.vendor_extensions = (!hex.is_empty()).then_some(hex);
            }
            b'6' => out.no_create = size == 1 && code[at] != 0,
            // 'x' (the global cache's SM6 entries: 4.26's FShaderCodeFeatures, always 0 here) is not read by the RHI
            // (ReadShaderOptionalData takes 'p', 'v', '6')
            b'x' => {}
            _ => {
                return Err(ReadError::parse(format!(
                    "{what}: optional data item '{}' (the build's are 'p', 'u', 'v', '6' and 'x')",
                    char::from(key)
                )));
            }
        }
        c.p = at + size;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let mut h = [0u8; 20];
        h[0] = 0xab;
        h[19] = 0x01;
        let s = sha_hex(&h);
        assert_eq!(s, "AB00000000000000000000000000000000000001");
        assert_eq!(sha_of_hex(&s), Some(h));
        assert_eq!(sha_of_hex(&s.to_lowercase()), None);
        assert_eq!(hash64_hex(0x0123_4567_89ab_cdef), "0123456789ABCDEF");
    }

    /// A code: an empty resource table, a 32-byte DXBC container, the optional data's 'p' and 'u' items.
    fn code() -> Vec<u8> {
        let mut c = Vec::new();
        c.extend_from_slice(&5u32.to_le_bytes()); // bits
        for _ in 0..5 {
            c.extend_from_slice(&0u32.to_le_bytes());
        }
        let mut dxbc = vec![0u8; 32];
        dxbc[..4].copy_from_slice(b"DXBC");
        dxbc[24..28].copy_from_slice(&32u32.to_le_bytes());
        c.extend_from_slice(&dxbc);
        let mut opt = Vec::new();
        opt.push(b'p');
        opt.extend_from_slice(&8i32.to_le_bytes());
        opt.extend_from_slice(&[1, 0, 0, 0, 0, 9, 0x0f, 0x01]);
        let mut u = Vec::new();
        u.extend_from_slice(&2u32.to_le_bytes());
        u.extend_from_slice(&0u32.to_le_bytes());
        u.extend_from_slice(&5u32.to_le_bytes());
        u.extend_from_slice(b"View\0");
        opt.push(b'u');
        opt.extend_from_slice(&i32::try_from(u.len()).unwrap().to_le_bytes());
        opt.extend_from_slice(&u);
        let size = i32::try_from(opt.len() + 4).unwrap();
        c.extend_from_slice(&opt);
        c.extend_from_slice(&size.to_le_bytes());
        c
    }

    #[test]
    fn split() {
        let c = code();
        let s = split_code(&c, 3, "t").unwrap();
        assert_eq!((s.container_at, s.container_size), (24, 32));
        assert_eq!(s.resource_table.resource_table_bits, 5);
        assert_eq!(s.packed, Some((true, 0x010f)));
        assert_eq!(s.uniform_buffer_names, Some(vec![String::new(), "View".into()]));
        assert!(s.ray_entry_points.is_none() && s.vendor_extensions.is_none() && !s.no_create);
        let mut bad = c.clone();
        bad[24] = b'X';
        assert!(split_code(&bad, 3, "t").is_err());
    }

    #[test]
    fn decode() {
        let c = code();
        let stored = CodeEntry { code: &c, at: 0, uncompressed_size: i32::try_from(c.len()).unwrap(), frequency: 0 };
        assert!(matches!(decode_entry(&stored).unwrap(), Cow::Borrowed(_)));
        let packed = lz4_flex::block::compress(&c);
        let e = CodeEntry { code: &packed, at: 0, uncompressed_size: i32::try_from(c.len()).unwrap(), frequency: 0 };
        assert_eq!(&*decode_entry(&e).unwrap(), &c[..]);
        let short = CodeEntry { uncompressed_size: e.uncompressed_size + 1, ..e };
        assert!(decode_entry(&short).is_err());
    }
}
