//! The `material` kind: a material resolved for the native renderer (`mvs-material`), one entry per material the
//! game draws, and the global shader cache's rows (`/Engine/GlobalShaderCache`). A json-bin container
//! (MATERIAL_MAGIC): the header (the instance chain's blend, shading model and draw state, the uniform buffer layout
//! with the preshaders evaluated, the textures and their sampling, the collections, the shader codes and parameter
//! records) and the body (the shader rows, the preshader program).
//! Read by `mvs-material` (material.rs Entry::decode).
//!
//! The list: every material the namer kinds' entries name as DrawRefs (`namers`), then those the stagedraw kind's
//! entries name (`stagedraw::stage_draw_materials`), each once in the order met, then the global shader cache. The
//! objects through `Cx::object`; the shader maps (`shadermap::with_material_maps`, `global_cache`) and the ini files
//! (`config::ini`) from those kinds' builds in-process.
//!
//! Parts:
//! - `resolve`: the instance chain, its parameters and the shader map it draws with;
//! - `layout`: the uniform buffer layout and its values, the collections, the shader rows;
//! - `sampling`: a texture's sampling from its asset and the LOD groups;
//! - `namers`: the DrawRefs each namer kind's entries name.

pub mod layout;
pub mod namers;
pub mod resolve;
pub mod sampling;

use indexmap::IndexSet;
use mvs_uasset::value::Value;

use self::layout::{RowBuilder, material_collections, material_layout, parameter_texture};
use self::resolve::{
    BaseOverride, ResolvedMaterial, conv_bool, conv_num, conv_string, disable_depth_test, material_shader_map,
    overridden, resolve_material,
};
use self::sampling::{TextureLodGroups, load_texture_lod_groups, texture_sampling};
use super::shadermap::{GLOBAL_CACHE_FILE, GLOBAL_SHADER_CACHE, Js, Obj, global_cache, write_array};
use super::stagedraw::stage_draw_materials;
use crate::game::{
    BANNER, FIGHTER, MATERIAL_FORMAT_VERSION, MATERIAL_KIND_VERSION, MENUSCENE, PARTICLES, SCENERY, STAGE,
    STAGEDRAW_KIND_VERSION, UI,
};
use crate::json::write_number;
use crate::jsonbin::{lay_out_body, write_json_bin};
use crate::kind::{Cx, Item, Kind, KindError};
use crate::objview::{field, num, string};
use mvs_uasset::natives::frozen::global_map_content;

/// The entry's magic.
pub const MATERIAL_MAGIC: &[u8; 4] = b"MVMT";

/// EMaterialShadingModel (4.26's order, which the base Materials' ShadingModelField bits follow: MSM_Unlit 1,
/// MSM_Subsurface 4, MSM_TwoSidedFoliage 64).
const SHADING_MODELS: [&str; 12] = [
    "MSM_Unlit",
    "MSM_DefaultLit",
    "MSM_Subsurface",
    "MSM_PreintegratedSkin",
    "MSM_ClearCoat",
    "MSM_SubsurfaceProfile",
    "MSM_TwoSidedFoliage",
    "MSM_Hair",
    "MSM_Cloth",
    "MSM_Eye",
    "MSM_SingleLayerWater",
    "MSM_ThinTranslucent",
];

/// A number written as JavaScript prints a double.
struct N(f64);

impl Js for N {
    fn js(&self, out: &mut Vec<u8>) {
        write_number(out, self.0);
    }
}

fn numbers(out: &mut Vec<u8>, v: &[f64]) {
    write_array(out, v, |out, x| write_number(out, *x));
}

fn failed<T>(why: String) -> Result<T, KindError> {
    Err(KindError::Failed(why))
}

/// What a texture parameter binds: a texture of the `texture` kind; the
/// engine's white texture of its type (no texture, or one the build does not hold or holds with no pixels:
/// FillUniformBuffer binds GWhiteTexture or the white cube; `missing` names the reference that did not resolve); or a
/// render target the game fills (its object path and ETextureRenderTargetFormat).
enum Bind {
    Texture {
        path: String,
        filter: &'static str,
        address_x: &'static str,
        address_y: &'static str,
        group: String,
        lod_bias: f64,
    },
    White {
        missing: Option<String>,
    },
    RenderTarget {
        path: String,
        format: Option<String>,
    },
}

impl Js for Bind {
    fn js(&self, out: &mut Vec<u8>) {
        let mut o = Obj::new(out);
        match self {
            Bind::Texture { path, filter, address_x, address_y, group, lod_bias } => {
                o.f("bind", "texture").f("path", path.as_str());
                let mut s = Obj::new(o.key("sampler"));
                s.f("filter", *filter)
                    .f("addressX", *address_x)
                    .f("addressY", *address_y)
                    .f("group", group.as_str())
                    .f("lodBias", &N(*lod_bias));
                s.end();
            }
            Bind::White { missing } => {
                o.f("bind", "white");
                if let Some(m) = missing {
                    o.f("missing", m.as_str());
                }
            }
            Bind::RenderTarget { path, format } => {
                o.f("bind", "renderTarget").f("path", path.as_str());
                if let Some(f) = format {
                    o.f("format", f.as_str());
                }
            }
        }
        o.end();
    }
}

/// What texture parameter k binds (FillUniformBuffer 142ba5110: the instance chain's value, else the referenced
/// texture at its TextureIndex; none, or one with no resource, binds the white texture of its type).
fn texture_bind(cx: &Cx<'_>, groups: &TextureLodGroups, r: Option<&str>) -> Result<Bind, KindError> {
    let Some(r) = r.filter(|r| !r.is_empty()) else { return Ok(Bind::White { missing: None }) };
    if !cx.exists(r) {
        return Ok(Bind::White { missing: Some(r.to_owned()) });
    }
    let t = cx.object(r)?;
    if t.class() == "/Script/Engine.TextureRenderTarget2D" {
        let format = match t.prop("RenderTargetFormat") {
            Some(Value::Str(f)) => Some(f.clone()),
            _ => None,
        };
        return Ok(Bind::RenderTarget { path: r.to_owned(), format });
    }
    // Unverified: a texture cooked with no pixels (PF_Unknown) is taken to have no RHI resource, so the white texture.
    let native = t.native()?;
    if matches!(native.as_ref().and_then(|n| n.get("PixelFormat")), Some(Value::Str(f)) if f == "PF_Unknown") {
        return Ok(Bind::White { missing: Some(r.to_owned()) });
    }
    let s = texture_sampling(&t, groups)?;
    Ok(Bind::Texture {
        path: r.to_owned(),
        filter: s.filter.name(),
        address_x: s.address_x.name(),
        address_y: s.address_y.name(),
        group: s.group.group,
        lod_bias: s.group.lod_bias,
    })
}

/// The material's entry.
fn bind_material(cx: &Cx<'_>, path: &str) -> Result<Vec<u8>, KindError> {
    let groups = load_texture_lod_groups(cx)?;
    let m: ResolvedMaterial<'_> = resolve_material(cx, path)?;
    let (blend, _) =
        overridden(cx, &m, BaseOverride::BlendMode, "BLEND_Opaque".to_owned(), "BLEND_Opaque".to_owned(), conv_string)?;
    let (shading, override_model) = overridden(
        cx,
        &m,
        BaseOverride::ShadingModel,
        "MSM_DefaultLit".to_owned(),
        "MSM_DefaultLit".to_owned(),
        conv_string,
    )?;
    let (two_sided, _) = overridden(cx, &m, BaseOverride::TwoSided, false, false, conv_bool)?;
    let content = material_shader_map(cx, &m.shader_map)?;
    let map = &content.record;
    let layout = material_layout(cx, &m, &content)?;
    let ues = &map.material_compilation_output.uniform_expression_set;
    let mut textures = Vec::new();
    for (k, t) in layout.textures.iter().enumerate() {
        let r = parameter_texture(cx, &m, &layout, k)?;
        textures.push((t, texture_bind(cx, &groups, r.as_deref())?));
    }
    let collections = material_collections(cx, &m, ues)?;
    let mut rows = RowBuilder::new(path);
    rows.add(&map.content, None)?;
    for mm in &map.ordered_mesh_shader_maps {
        rows.add(&mm.content, Some(&mm.vertex_factory_type_name))?;
    }
    let Some(model) = SHADING_MODELS.iter().position(|s| *s == shading) else {
        return failed(format!("material: {path}: shading model {shading}"));
    };
    // Unverified: the defaults of a property the base Material does not store are UE 4.26's UMaterial constructor's
    // (MD_Surface, 0.3333, TLM_VolumetricNonDirectional, DBM_Translucent, a DefaultLit field, the two fog and separate
    // translucency flags set), not read from the exe.
    let field_bits = match m.prop("ShadingModels") {
        Some(Value::Struct(s)) => field(s, "ShadingModelField").and_then(num),
        _ => None,
    }
    .unwrap_or(2.0);
    let base_string = |k: &str, d: &str| m.prop(k).and_then(|v| string(cx.cache, v)).unwrap_or_else(|| d.to_owned());
    let flag = |k: &str, d: bool| match m.prop(k) {
        Some(Value::Bool(b)) => *b,
        // (`=== true` is false for anything else; `!== false` is true for anything else)
        Some(_) => d,
        None => d,
    };
    let sections = [rows.shader_rows(), content.preshaders.clone()];
    let (refs, _) = lay_out_body(&sections);
    let (clip, _) = overridden(
        cx,
        &m,
        BaseOverride::OpacityMaskClipValue,
        0.3333,
        m.prop("OpacityMaskClipValue").and_then(num).unwrap_or(0.3333),
        conv_num,
    )?;
    let (dithered, _) = overridden(cx, &m, BaseOverride::DitheredLodTransition, false, false, conv_bool)?;
    let (cast_masked, _) = overridden(cx, &m, BaseOverride::CastDynamicShadowAsMasked, false, false, conv_bool)?;
    let mut json = Vec::new();
    let mut o = Obj::new(&mut json);
    o.f("kind", "material")
        .f("version", &MATERIAL_FORMAT_VERSION)
        .f("material", path)
        .f("base", m.base.as_str())
        .f("shaderMap", m.shader_map.as_str())
        .f("qualityLevel", &map.shader_map_id.quality_level)
        .f("featureLevel", &map.shader_map_id.feature_level)
        .f("domain", base_string("MaterialDomain", "MD_Surface").as_str())
        .f("blend", blend.as_str())
        .f("shadingModel", shading.as_str())
        .f("shadingModels", &N(if override_model { f64::from(1u32 << model) } else { field_bits }))
        .f("twoSided", &two_sided)
        .f("opacityMaskClipValue", &N(clip))
        .f("ditheredLODTransition", &dithered)
        .f("castDynamicShadowAsMasked", &cast_masked)
        .f("translucencyLightingMode", base_string("TranslucencyLightingMode", "TLM_VolumetricNonDirectional").as_str())
        .f("decalBlendMode", base_string("DecalBlendMode", "DBM_Translucent").as_str())
        .f("disableDepthTest", &disable_depth_test(&m, &blend))
        .f("responsiveAA", &flag("bEnableResponsiveAA", false))
        .f("translucencyVertexFog", &flag("bUseTranslucencyVertexFog", true))
        .f("separateTranslucency", &flag("bEnableSeparateTranslucency", true))
        .f("allowTranslucentCustomDepthWrites", &flag("AllowTranslucentCustomDepthWrites", false))
        .f("usedSceneTextures", &map.material_compilation_output.used_scene_textures)
        .f("usedDBufferTextures", &map.material_compilation_output.used_d_buffer_textures)
        .f("compilationFlags", &[map.material_compilation_output.b1, map.material_compilation_output.b2]);
    let layout_init = &ues.uniform_buffer_layout_initializer;
    let mut u = Obj::new(o.key("uniforms"));
    u.f("layoutName", layout_init.name.as_str())
        .f("size", &layout.size)
        .f("resources", &layout_init.resources)
        .f(
            "vectorPreshaders",
            &ues.uniform_vector_preshaders.iter().map(|h| (h.opcode_offset, h.opcode_size)).collect::<Vec<_>>(),
        )
        .f(
            "scalarPreshaders",
            &ues.uniform_scalar_preshaders.iter().map(|h| (h.opcode_offset, h.opcode_size)).collect::<Vec<_>>(),
        )
        .f("preshaderNames", &ues.preshader_names);
    write_array(u.key("scalars"), &layout.scalar_params, |out, (name, value)| {
        let mut s = Obj::new(out);
        s.f("name", name.as_str()).f("value", &N(*value));
        s.end();
    });
    write_array(u.key("vectors"), &layout.vector_params, |out, (name, value)| {
        let mut s = Obj::new(out);
        s.f("name", name.as_str());
        numbers(s.key("value"), value);
        s.end();
    });
    write_array(u.key("textures"), &textures, |out, (t, bind)| {
        let mut s = Obj::new(out);
        s.f("name", t.name.as_str())
            .f("type", t.type_)
            .f("textureIndex", &t.texture_index)
            .f("samplerSource", &t.sampler_source)
            .f("texture", bind);
        s.end();
    });
    if let Some(sizes) = &layout.texture_sizes {
        write_array(u.key("textureSizes"), sizes, |out, s| match s {
            Some(v) => numbers(out, v),
            None => out.extend_from_slice(b"null"),
        });
    }
    numbers(u.key("values"), &layout.values);
    u.end();
    write_array(o.key("collections"), &collections, |out, c| {
        let mut s = Obj::new(out);
        s.f("path", c.path.as_str()).f("scalars", &c.scalars).f("vectors", &c.vectors);
        numbers(s.key("values"), &c.values);
        s.end();
    });
    o.f("codes", &rows.codes);
    write_array(o.key("params"), &rows.params, |out, p| out.extend_from_slice(p));
    o.f("shaders", &refs[0]).f("preshader", &refs[1]);
    o.end();
    Ok(write_json_bin(MATERIAL_MAGIC, &json, &sections, &refs))
}

/// The global shader cache's entry.
fn bind_global(cx: &Cx<'_>) -> Result<Vec<u8>, KindError> {
    let global = global_cache(cx)?;
    let mut rows = RowBuilder::new(GLOBAL_SHADER_CACHE);
    for (i, m) in global.maps()?.iter().enumerate() {
        let rec = global_map_content(m, &format!("{GLOBAL_CACHE_FILE} map {i}"))?;
        rows.add(&rec.content, Some(&rec.hashed_source_filename))?;
    }
    let sections = [rows.shader_rows()];
    let (refs, _) = lay_out_body(&sections);
    let mut json = Vec::new();
    let mut o = Obj::new(&mut json);
    o.f("kind", "global").f("version", &MATERIAL_FORMAT_VERSION).f("codes", &rows.codes);
    write_array(o.key("params"), &rows.params, |out, p| out.extend_from_slice(p));
    o.f("shaders", &refs[0]);
    o.end();
    Ok(write_json_bin(MATERIAL_MAGIC, &json, &sections, &refs))
}

/// The `material` kind.
pub struct Material;

impl Kind for Material {
    fn id(&self) -> &'static str {
        "material"
    }

    fn version(&self) -> u64 {
        MATERIAL_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/octet-stream"
    }

    /// The materials the namers' DrawRefs name, those the stagedraw entries name, then the global shader cache
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        let mut out: IndexSet<String> = IndexSet::new();
        for r in namers::named_draw_refs(cx)? {
            out.insert(namers::draw_ref_material(&r)?.to_owned());
        }
        for p in cx.store.paths("stagedraw")? {
            out.extend(stage_draw_materials(&cx.store.entry("stagedraw", &p)?)?);
        }
        let mut items: Vec<Item> = out.into_iter().map(Item::new).collect();
        items.push(Item::new(GLOBAL_SHADER_CACHE));
        Ok(items)
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        if item.path == GLOBAL_SHADER_CACHE { bind_global(cx) } else { bind_material(cx, &item.path) }
    }

    /// The stagedraw kind and the namers (and namerReads; the object, shadermap and config
    /// kinds are built in-process).
    fn reads(&self) -> &'static [(&'static str, u64)] {
        &[("stagedraw", STAGEDRAW_KIND_VERSION), FIGHTER, STAGE, PARTICLES, SCENERY, UI, BANNER, MENUSCENE]
    }
}
