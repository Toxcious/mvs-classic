//! The `material` kind's entries: a material resolved at import for the D3D11 renderer (its
//! shader map's FShaders as rows, their codes and parameter records, its uniform expression set and parameters merged
//! down the instance chain, its textures and draw state), and the global shader cache's rows.

use serde::{Deserialize, Deserializer};

use crate::hash::hashed_name;
use crate::jsonbin::{BodyRef, JsonBin};
use crate::shader::{Frequency, Sha1};
use crate::{Error, Result};

const MATERIAL_MAGIC: &[u8; 4] = b"MVMT";
/// The entry layout this reader reads.
pub const MATERIAL_FORMAT_VERSION: u32 = 1;
/// The global shader cache's entry path.
pub const GLOBAL_SHADER_CACHE: &str = "/Engine/GlobalShaderCache";
const ROW_BYTES: usize = 40;

/// An FHashedName as the entries write it: 16 hex digits.
fn hash64<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<u64, D::Error> {
    let s = <&str>::deserialize(d)?;
    u64::from_str_radix(s, 16).map_err(|_| serde::de::Error::custom(format!("not a hashed name: {s}")))
}

/// FShaderParameterInfo.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ParameterInfo {
    #[serde(rename = "BaseIndex")]
    pub base_index: u32,
    #[serde(rename = "Size")]
    pub size: u32,
}

/// FShaderLooseParameterBufferInfo: a loose ($Globals) buffer's register, size and its parameters' byte ranges.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct LooseParameterBuffer {
    #[serde(rename = "BaseIndex")]
    pub base_index: u32,
    #[serde(rename = "Size")]
    pub size: u32,
    #[serde(rename = "Parameters")]
    pub parameters: Vec<ParameterInfo>,
}

/// FShaderParameterMapInfo (4.26): what the RHI binds by register.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ParameterMapInfo {
    #[serde(rename = "UniformBuffers")]
    pub uniform_buffers: Vec<ParameterInfo>,
    #[serde(rename = "TextureSamplers")]
    pub texture_samplers: Vec<ParameterInfo>,
    #[serde(rename = "SRVs")]
    pub srvs: Vec<ParameterInfo>,
    #[serde(rename = "LooseParameterBuffers")]
    pub loose_parameter_buffers: Vec<LooseParameterBuffer>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct BindingParameter {
    #[serde(rename = "BufferIndex")]
    pub buffer_index: u32,
    #[serde(rename = "BaseIndex")]
    pub base_index: u32,
    #[serde(rename = "ByteOffset")]
    pub byte_offset: u32,
    #[serde(rename = "ByteSize")]
    pub byte_size: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct BindingResource {
    #[serde(rename = "ByteOffset")]
    pub byte_offset: u32,
    #[serde(rename = "BaseIndex")]
    pub base_index: u32,
    #[serde(rename = "BaseType")]
    pub base_type: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct BindingBuffer {
    #[serde(rename = "BufferIndex")]
    pub buffer_index: u32,
    #[serde(rename = "ByteOffset")]
    pub byte_offset: u32,
}

/// FShaderParameterBindings (4.26): a shader parameter struct's members by byte offset in the struct.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ParameterBindings {
    #[serde(rename = "Parameters")]
    pub parameters: Vec<BindingParameter>,
    #[serde(rename = "ResourceParameters")]
    pub resource_parameters: Vec<BindingResource>,
    #[serde(rename = "GraphUniformBuffers")]
    pub graph_uniform_buffers: Vec<BindingBuffer>,
    #[serde(rename = "ParameterReferences")]
    pub parameter_references: Vec<BindingBuffer>,
    #[serde(rename = "StructureLayoutHash")]
    pub structure_layout_hash: u32,
    #[serde(rename = "RootParameterBufferIndex")]
    pub root_parameter_buffer_index: u32,
}

#[derive(Deserialize)]
struct UniformBufferParam(#[serde(deserialize_with = "hash64")] u64, u32);

#[derive(Deserialize)]
struct ParamsRaw {
    #[serde(rename = "uniformBuffers")]
    uniform_buffers: Vec<UniformBufferParam>,
    #[serde(rename = "parameterMap")]
    parameter_map: ParameterMapInfo,
    bindings: ParameterBindings,
}

/// An FShader's parameter records.
#[derive(Clone, Debug)]
pub struct ShaderParams {
    /// UniformBufferParameterStructs[k] (the struct's FHashedName) and its register (UniformBufferParameters[k]).
    pub uniform_buffers: Vec<(u64, u32)>,
    pub parameter_map: ParameterMapInfo,
    pub bindings: ParameterBindings,
}

impl From<ParamsRaw> for ShaderParams {
    fn from(r: ParamsRaw) -> ShaderParams {
        ShaderParams {
            uniform_buffers: r.uniform_buffers.into_iter().map(|u| (u.0, u.1)).collect(),
            parameter_map: r.parameter_map,
            bindings: r.bindings,
        }
    }
}

/// One FShader of a map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShaderRow {
    /// The mesh map's vertex factory type (0: a material shader); in the global entry the section's
    /// HashedSourceFilename.
    pub map: u64,
    pub shader_type: u64,
    /// The shader pipeline type it is in (0: none).
    pub pipeline: u64,
    pub permutation: i32,
    pub frequency: Frequency,
    /// Index into the entry's codes and parameter records.
    pub code: u32,
    pub params: u32,
}

fn rows(b: &[u8], codes: usize, params: usize, what: &str) -> Result<Vec<ShaderRow>> {
    if !b.len().is_multiple_of(ROW_BYTES) {
        return Err(Error::format(what, format!("a rows section of {} bytes", b.len())));
    }
    let u64at = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let u32at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let out: Vec<ShaderRow> = (0..b.len() / ROW_BYTES)
        .map(|i| {
            let o = i * ROW_BYTES;
            ShaderRow {
                map: u64at(o),
                shader_type: u64at(o + 8),
                pipeline: u64at(o + 16),
                permutation: u32at(o + 24) as i32,
                frequency: Frequency::from_u8(b[o + 28]),
                code: u32at(o + 32),
                params: u32at(o + 36),
            }
        })
        .collect();
    if let Some(r) = out.iter().find(|r| r.code as usize >= codes || r.params as usize >= params) {
        return Err(Error::format(
            what,
            format!("a row names code {} of {codes}, params {} of {params}", r.code, r.params),
        ));
    }
    Ok(out)
}

/// A set of FShaders: the rows, their codes and parameter records.
#[derive(Clone, Debug)]
pub struct ShaderSet {
    pub rows: Vec<ShaderRow>,
    pub codes: Vec<Sha1>,
    pub params: Vec<ShaderParams>,
}

impl ShaderSet {
    /// The shader of `shader_type` at `permutation` and `frequency` in map `map` (a vertex factory's hashed name, 0 for
    /// a material shader; a source file's in the global set; None: any map), outside a pipeline.
    pub fn find(
        &self,
        map: Option<u64>,
        shader_type: u64,
        permutation: i32,
        frequency: Frequency,
    ) -> Option<&ShaderRow> {
        self.rows.iter().find(|r| {
            map.is_none_or(|m| r.map == m)
                && r.shader_type == shader_type
                && r.permutation == permutation
                && r.frequency == frequency
                && r.pipeline == 0
        })
    }

    /// The same by names (FHashedName of each; `vertex_factory` None: a material shader).
    pub fn find_named(
        &self,
        vertex_factory: Option<&str>,
        shader_type: &str,
        permutation: i32,
        frequency: Frequency,
    ) -> Option<&ShaderRow> {
        self.find(Some(vertex_factory.map_or(0, hashed_name)), hashed_name(shader_type), permutation, frequency)
    }

    /// A pass's shaders the way a 4.26 mesh pass processor takes them (FMaterial::TryGetShaders, MaterialShared.cpp,
    /// with RHISupportsShaderPipelines true on D3D11 SM5): when `pipeline` names a pipeline the map holds for the
    /// vertex factory, its shaders (each of `types` must be in it, at its permutation); else each of `types`
    /// standalone. `types`: (shader type name, permutation id) per stage; None when one is missing. The rows come in
    /// `types`' order.
    // Unverified: the rule is 4.26's source (FMeshPassProcessor's Get*Shaders filling FMaterialShaderTypes with a
    // PipelineType, then TryGetShaders), not read from the exe.
    pub fn pass(
        &self,
        vertex_factory: Option<&str>,
        pipeline: Option<&str>,
        types: &[(&str, i32)],
    ) -> Option<(Option<u64>, Vec<&ShaderRow>)> {
        let map = vertex_factory.map_or(0, hashed_name);
        let take = |p: u64| -> Option<Vec<&ShaderRow>> {
            types
                .iter()
                .map(|&(t, perm)| {
                    let t = hashed_name(t);
                    self.rows
                        .iter()
                        .find(|r| r.map == map && r.pipeline == p && r.shader_type == t && r.permutation == perm)
                })
                .collect()
        };
        if let Some(p) = pipeline.map(hashed_name)
            && self.rows.iter().any(|r| r.map == map && r.pipeline == p)
        {
            return take(p).map(|rows| (Some(p), rows));
        }
        take(0).map(|rows| (None, rows))
    }

    pub fn code(&self, row: &ShaderRow) -> Sha1 {
        self.codes[row.code as usize]
    }

    pub fn params(&self, row: &ShaderRow) -> &ShaderParams {
        &self.params[row.params as usize]
    }

    /// The vertex factories the set has mesh maps for, in the rows' order.
    pub fn vertex_factories(&self) -> Vec<u64> {
        let mut out = Vec::new();
        for r in &self.rows {
            if r.map != 0 && !out.contains(&r.map) {
                out.push(r.map);
            }
        }
        out
    }
}

/// What a texture parameter's sampler is.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Filter {
    Point,
    Bilinear,
    Trilinear,
    AnisotropicPoint,
    AnisotropicLinear,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Address {
    Wrap,
    Clamp,
    Mirror,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sampler {
    pub filter: Filter,
    pub address_x: Address,
    pub address_y: Address,
    /// The texture's LOD group and its LODBias.
    pub group: String,
    pub lod_bias: f64,
}

/// What a texture parameter binds.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "bind", rename_all = "camelCase")]
pub enum TextureBind {
    /// A `texture` kind entry, sampled so.
    Texture { path: String, sampler: Sampler },
    /// The engine's white texture of the parameter's type (`missing`: the reference the build does not hold, or
    /// holds with no pixels).
    White { missing: Option<String> },
    /// A render target the game fills.
    RenderTarget { path: String, format: Option<String> },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TextureType {
    #[serde(rename = "2d")]
    Texture2d,
    Cube,
    Array,
    Volume,
}

/// A texture parameter: resources 2k (the texture) and 2k + 1 (its sampler) of the material buffer.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialTexture {
    pub name: String,
    #[serde(rename = "type")]
    pub texture_type: TextureType,
    pub texture_index: u32,
    /// ESamplerSourceMode: 0 the texture's own sampler, 1 the Wrap WorldGroupSettings sampler, 2 the Clamp one.
    pub sampler_source: u32,
    pub texture: TextureBind,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ScalarParam {
    pub name: String,
    pub value: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VectorParam {
    pub name: String,
    pub value: [f64; 4],
}

/// A preshader: its program's range in the preshader section.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Preshader(pub u32, pub u32);

/// The material uniform buffer: its layout and the uniform expression set that fills it
/// (FUniformExpressionSet::FillUniformBuffer 142ba5110).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uniforms {
    pub layout_name: String,
    /// ConstantBufferSize.
    pub size: u32,
    /// [MemberOffset, MemberType] of each resource (the texture and sampler pairs, then the shared samplers).
    pub resources: Vec<(u32, u32)>,
    pub vector_preshaders: Vec<Preshader>,
    pub scalar_preshaders: Vec<Preshader>,
    pub preshader_names: Vec<String>,
    pub scalars: Vec<ScalarParam>,
    pub vectors: Vec<VectorParam>,
    pub textures: Vec<MaterialTexture>,
    #[serde(default)]
    pub texture_sizes: Option<Vec<Option<[f64; 4]>>>,
    /// The rows as evaluated at import: vectors * 4 + ceil(scalars / 4) * 4 numbers.
    pub values: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Collection {
    pub path: String,
    pub scalars: Vec<String>,
    pub vectors: Vec<String>,
    pub values: Vec<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MaterialRaw {
    version: u32,
    material: String,
    base: String,
    shader_map: String,
    quality_level: u32,
    feature_level: u32,
    domain: String,
    blend: String,
    shading_model: String,
    shading_models: u32,
    two_sided: bool,
    opacity_mask_clip_value: f64,
    #[serde(rename = "ditheredLODTransition")]
    dithered_lod_transition: bool,
    cast_dynamic_shadow_as_masked: bool,
    translucency_lighting_mode: String,
    decal_blend_mode: String,
    disable_depth_test: bool,
    #[serde(rename = "responsiveAA")]
    responsive_aa: bool,
    translucency_vertex_fog: bool,
    separate_translucency: bool,
    allow_translucent_custom_depth_writes: bool,
    used_scene_textures: u32,
    used_d_buffer_textures: u32,
    compilation_flags: [u8; 2],
    uniforms: Uniforms,
    collections: Vec<Collection>,
    codes: Vec<Sha1>,
    params: Vec<ParamsRaw>,
    shaders: BodyRef,
    preshader: BodyRef,
}

#[derive(Deserialize)]
struct GlobalRaw {
    version: u32,
    codes: Vec<Sha1>,
    params: Vec<ParamsRaw>,
    shaders: BodyRef,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum EntryRaw {
    Material(Box<MaterialRaw>),
    Global(GlobalRaw),
}

/// A material's draw state, after the instance chain's BasePropertyOverrides.
#[derive(Clone, Debug)]
pub struct DrawState {
    /// EMaterialDomain, EBlendMode, EMaterialShadingModel names.
    pub domain: String,
    pub blend: String,
    pub shading_model: String,
    /// FMaterialShadingModelField's bits.
    pub shading_models: u32,
    pub two_sided: bool,
    pub opacity_mask_clip_value: f64,
    pub dithered_lod_transition: bool,
    pub cast_dynamic_shadow_as_masked: bool,
    pub translucency_lighting_mode: String,
    pub decal_blend_mode: String,
    pub disable_depth_test: bool,
    pub responsive_aa: bool,
    pub translucency_vertex_fog: bool,
    pub separate_translucency: bool,
    pub allow_translucent_custom_depth_writes: bool,
}

/// A material entry.
#[derive(Clone, Debug)]
pub struct Material {
    pub path: String,
    pub base: String,
    /// The shadermap kind's entry the shaders come from and the resource's FMaterialShaderMapId levels.
    pub shader_map: String,
    pub quality_level: u32,
    pub feature_level: u32,
    pub state: DrawState,
    /// FMaterialCompilationOutput's UsedSceneTextures, UsedDBufferTextures and its two packed flag bytes.
    pub used_scene_textures: u32,
    pub used_dbuffer_textures: u32,
    pub compilation_flags: [u8; 2],
    pub uniforms: Uniforms,
    pub collections: Vec<Collection>,
    pub shaders: ShaderSet,
    /// UniformPreshaderData.Data: the preshader programs.
    pub preshader: Vec<u8>,
}

/// The global shader cache's rows.
#[derive(Clone, Debug)]
pub struct GlobalShaders {
    pub shaders: ShaderSet,
}

impl GlobalShaders {
    /// A global shader by its type name, permutation id and frequency (a type is in one source file's section).
    pub fn find(&self, shader_type: &str, permutation: i32, frequency: Frequency) -> Option<&ShaderRow> {
        self.shaders.find(None, hashed_name(shader_type), permutation, frequency)
    }

    /// The permutation ids the cache holds of a type at a frequency, ascending (empty: no such type).
    pub fn permutations(&self, shader_type: &str, frequency: Frequency) -> Vec<i32> {
        let h = hashed_name(shader_type);
        let mut out: Vec<i32> = self
            .shaders
            .rows
            .iter()
            .filter(|r| r.shader_type == h && r.frequency == frequency && r.pipeline == 0)
            .map(|r| r.permutation)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// A `material` kind entry.
#[derive(Clone, Debug)]
pub enum Entry {
    Material(Box<Material>),
    Global(GlobalShaders),
}

fn check_version(v: u32, what: &str) -> Result<()> {
    if v != MATERIAL_FORMAT_VERSION {
        return Err(Error::format(what, format!("entry version {v}, this reader's {MATERIAL_FORMAT_VERSION}")));
    }
    Ok(())
}

impl Entry {
    /// Decodes an entry's bytes; `what` names it in errors.
    pub fn decode(bytes: &[u8], what: &str) -> Result<Entry> {
        let jb = JsonBin::split(MATERIAL_MAGIC, bytes, what)?;
        Ok(match jb.parse::<EntryRaw>(what)? {
            EntryRaw::Global(g) => {
                check_version(g.version, what)?;
                let rows = rows(jb.section(g.shaders, what)?, g.codes.len(), g.params.len(), what)?;
                let shaders =
                    ShaderSet { rows, codes: g.codes, params: g.params.into_iter().map(Into::into).collect() };
                Entry::Global(GlobalShaders { shaders })
            }
            EntryRaw::Material(m) => {
                let m = *m;
                check_version(m.version, what)?;
                let rows = rows(jb.section(m.shaders, what)?, m.codes.len(), m.params.len(), what)?;
                let preshader = jb.section(m.preshader, what)?.to_vec();
                let n = m.uniforms.vector_preshaders.iter().chain(&m.uniforms.scalar_preshaders);
                if let Some(p) = n.into_iter().find(|p| (p.0 + p.1) as usize > preshader.len()) {
                    return Err(Error::format(
                        what,
                        format!("a preshader at {}+{} past {}", p.0, p.1, preshader.len()),
                    ));
                }
                Entry::Material(Box::new(Material {
                    path: m.material,
                    base: m.base,
                    shader_map: m.shader_map,
                    quality_level: m.quality_level,
                    feature_level: m.feature_level,
                    state: DrawState {
                        domain: m.domain,
                        blend: m.blend,
                        shading_model: m.shading_model,
                        shading_models: m.shading_models,
                        two_sided: m.two_sided,
                        opacity_mask_clip_value: m.opacity_mask_clip_value,
                        dithered_lod_transition: m.dithered_lod_transition,
                        cast_dynamic_shadow_as_masked: m.cast_dynamic_shadow_as_masked,
                        translucency_lighting_mode: m.translucency_lighting_mode,
                        decal_blend_mode: m.decal_blend_mode,
                        disable_depth_test: m.disable_depth_test,
                        responsive_aa: m.responsive_aa,
                        translucency_vertex_fog: m.translucency_vertex_fog,
                        separate_translucency: m.separate_translucency,
                        allow_translucent_custom_depth_writes: m.allow_translucent_custom_depth_writes,
                    },
                    used_scene_textures: m.used_scene_textures,
                    used_dbuffer_textures: m.used_d_buffer_textures,
                    compilation_flags: m.compilation_flags,
                    uniforms: m.uniforms,
                    collections: m.collections,
                    shaders: ShaderSet { rows, codes: m.codes, params: m.params.into_iter().map(Into::into).collect() },
                    preshader,
                }))
            }
        })
    }
}
