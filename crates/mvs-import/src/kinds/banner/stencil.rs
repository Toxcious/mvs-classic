//! Which draw context a banner's material is named in (`bannerlocal` / `bannersprite`, or the plain `local` /
//! `sprite`).
//!
//! The banners' own contexts bind the custom stencil. A material whose banner build reads no custom stencil draws
//! the same in the plain context, so it is named there, one entry with the match effects' (the five such sprite
//! materials the match effects hold, CircleSoftGlow_Team_MAT and the like). Read by the banner bind and by the
//! particles kind for the systems the banner entries name (a sprite emitter's context), each through its own
//! [`BannerProbes`].

use std::sync::Arc;

use mvs_material::draw::{DISTORTION_PS, DrawContextRow, context};
use mvs_material::dxbc::reads_custom_stencil;
use mvs_material::{ShaderCode, hashed_name};
use mvs_uasset::natives::frozen::{MaterialShaderMapRecord, ShaderRecord, sha_of_hex};

use super::super::material::layout::material_layout;
use super::super::material::resolve::{
    BaseOverride, ResolvedMaterial, conv_string, material_shader_map, overridden, resolve_material,
};
use super::super::shader::shader_entry;
use super::super::shadermap::with_material_maps;
use crate::kind::{Cx, KindError, Memo, not_ported};
use crate::objview::{string, truthy};

/// A banner context's build of a material: refused (the reason), or built, with whether a pass reads the custom
/// stencil (`mvs_material::dxbc::reads_custom_stencil`) and its scalar parameters (the layout's named scalar
/// params and the value each has).
#[derive(Clone, Debug, PartialEq)]
pub enum BannerProbe {
    Refused(String),
    Built { reads: bool, scalars: Vec<BannerScalar> },
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct BannerScalar {
    pub name: String,
    pub def: f64,
}

/// The verdicts by banner-context DrawRef, kept across the banners of an import (or the systems of the particles
/// kind's run).
#[derive(Default)]
pub struct BannerProbes(pub Memo<Result<Arc<BannerProbe>, KindError>>);

/// The plain context a banner's mesh slot (`Local`) or sprite emitter (`Sprite`) is named in when its banner build
/// reads no custom stencil.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plain {
    Local,
    Sprite,
}

impl Plain {
    fn name(self) -> &'static str {
        match self {
            Plain::Local => "local",
            Plain::Sprite => "sprite",
        }
    }

    /// Its banner context (`BANNER`).
    fn banner(self) -> &'static str {
        match self {
            Plain::Local => "bannerlocal",
            Plain::Sprite => "bannersprite",
        }
    }
}

/// A DrawRef.
pub fn draw_ref(material: &str, ctx: &DrawContextRow) -> Result<String, KindError> {
    if !material.starts_with('/') || !material.contains('.') || material.contains('?') {
        return Err(KindError::Failed(format!("drawRef: {material} is not a <package>.<export> path")));
    }
    Ok(format!("{material}?ctx={}", ctx.name))
}

/// The material's build in `ctx` (once per `probes`): a banner context, or the `mesh` / `trail` a banner system's
/// mesh or trail emitter names it in. A material not in the paks (an import the build does not hold:
/// banner_pfg_snowman's Martian_Sparkle_MAT under _Unreleased) is refused.
///
/// The verdicts follow an effects builder as far as they read it: the
/// material's state and the base pass it is drawn with (the refusals that are rules of the material's data), its
/// shader map, the pixel shaders of the base and distortion passes and whether their code declares the custom
/// stencil, and the layout's scalar parameters.
// The cooked DXBC is not translated to GLSL here, so no check of a translation's buffers, textures and interpolants
// is run (`unsupported dcl_...`, an interpolant its vertex shader does not write): the native renderer draws the
// DXBC itself. The banner entries' `refused` lists hold the one missing material only.
pub fn banner_probe(
    cx: &Cx<'_>,
    probes: &BannerProbes,
    material: &str,
    ctx: &'static DrawContextRow,
) -> Result<Arc<BannerProbe>, KindError> {
    let key = draw_ref(material, ctx)?;
    probes.0.get(&key, || probe(cx, material, ctx))
}

fn refused(why: String) -> Result<Arc<BannerProbe>, KindError> {
    Ok(Arc::new(BannerProbe::Refused(why)))
}

fn failed<T>(why: String) -> Result<T, KindError> {
    Err(KindError::Failed(why))
}

/// Materials whose Masked DefaultLit build draws Emissive (and Normal) alone, with the no-lightmap base pass (by the
/// material asked for).
const EMISSIVE_ONLY_LIT: [&str; 2] = [
    "/Game/Panda_Main/VFX_Ringouts/Yosemite_Sam/M_Smoke.M_Smoke",
    "/Game/Panda_Main/VFX_Ringouts/GreenLantern/GreenLanternGuns_MAT.GreenLanternGuns_MAT",
];
/// Masked DefaultLit metals drawn the same way, by the base Material.
const EMISSIVE_ONLY_METAL: [&str; 1] =
    ["/Game/Panda_Main/PreMatch/Banners/GhostInThemThereHills/M_Banner_Foreground_Gold.M_Banner_Foreground_Gold"];
/// The blend modes the effects contexts draw (BLEND_Modulate unlit only).
const BLENDS: [&str; 5] = ["BLEND_Translucent", "BLEND_Additive", "BLEND_Opaque", "BLEND_Masked", "BLEND_Modulate"];
/// The base pass of a lit translucency, and its Skylight policy.
const PS_VLM: &str = "TBasePassPSFPrecomputedVolumetricLightmapLightingPolicy";
const PS_VLM_SKYLIGHT: &str = "TBasePassPSFPrecomputedVolumetricLightmapLightingPolicySkylight";
/// The base pass with no light map.
const NO_LIGHTMAP: &str = "TBasePassPSFNoLightMapPolicy";

/// The base pass pixel shader the effects builder draws a material with (with no lightmap
/// policy): the shader type, or the refusal's reason; an error where a plain one is the builder's.
fn base_pass(
    cx: &Cx<'_>,
    m: &ResolvedMaterial<'_>,
    ctx: &DrawContextRow,
    blend: &str,
    shading: &str,
    what: &str,
) -> Result<Result<&'static str, String>, KindError> {
    let has = |k: &str| m.prop(k).is_some_and(truthy);
    let lit = shading == "MSM_DefaultLit" && matches!(blend, "BLEND_Translucent" | "BLEND_Additive");
    let sprite = matches!(ctx.name, "sprite" | "bannersprite");
    let sss = shading == "MSM_Subsurface" && blend == "BLEND_Translucent" && sprite;
    let masked_lit = blend == "BLEND_Masked" && shading == "MSM_DefaultLit";
    let metal = EMISSIVE_ONLY_METAL.contains(&m.base.as_str()) && masked_lit;
    if metal && (!has("Metallic") || has("Roughness") || has("SubsurfaceColor")) {
        return failed(format!("{what}: in EMISSIVE_ONLY_METAL with no Metallic"));
    }
    let emissive_only = metal || (EMISSIVE_ONLY_LIT.contains(&m.path.as_str()) && masked_lit);
    if emissive_only
        && !metal
        && ["BaseColor", "Metallic", "Specular", "Roughness", "SubsurfaceColor"].iter().any(|p| has(p))
    {
        return failed(format!("{what}: in EMISSIVE_ONLY_LIT with a lit input connected"));
    }
    if !BLENDS.contains(&blend)
        || (blend == "BLEND_Modulate" && shading != "MSM_Unlit")
        || (shading != "MSM_Unlit" && !lit && !sss && !emissive_only)
    {
        return Ok(Err(format!("{blend} {shading} not supported yet")));
    }
    let mode = m.prop("TranslucencyLightingMode").and_then(|v| string(cx.cache, v));
    let surface = lit && mode.as_deref().unwrap_or("").starts_with("TLM_Surface");
    if (lit || sss) && !surface && has("BaseColor") {
        return Ok(Err("lit translucency with a BaseColor needs the lighting inputs".into()));
    }
    if sss && has("SubsurfaceColor") {
        return Ok(Err("connected SubsurfaceColor".into()));
    }
    Ok(Ok(if sss || surface {
        PS_VLM_SKYLIGHT
    } else if lit {
        PS_VLM
    } else {
        ctx.pixel_shader.unwrap_or(NO_LIGHTMAP)
    }))
}

/// A shader of the map by vertex factory and type.
fn find_shader<'m>(map: &'m MaterialShaderMapRecord, vf: Option<&str>, ty: &str) -> Option<&'m ShaderRecord> {
    let t = format!("{:016X}", hashed_name(ty));
    match vf {
        None => map.content.shaders.iter().find(|s| s.type_ == t),
        Some(vf) => {
            let v = format!("{:016X}", hashed_name(vf));
            let m = map.ordered_mesh_shader_maps.iter().find(|x| x.vertex_factory_type_name == v)?;
            m.content.shaders.iter().find(|s| s.type_ == t)
        }
    }
}

/// Whether a shader's code reads the custom stencil (`mvs_material::dxbc::reads_custom_stencil`): its code is read
/// from the material's inline shader maps (the `shader` kind's entry of it, built in-process).
fn shader_reads_stencil(cx: &Cx<'_>, material_map: &str, shader: &ShaderRecord, what: &str) -> Result<bool, KindError> {
    let Some(hash) = &shader.hash else {
        return failed(format!("drawmat: {what}: the shader's code is shared (bShareCode), not read"));
    };
    let Some(sha) = sha_of_hex(hash) else {
        return failed(format!("drawmat: {what}: {hash} is no code hash"));
    };
    let bytes = with_material_maps(cx, material_map, |maps| {
        let entry = maps.iter().flatten().find_map(|m| {
            let k = m.map.code.shader_hashes.iter().position(|h| *h == sha)?;
            m.map.code.entries.get(k)
        });
        entry
            .map(|e| shader_entry(e, &format!("{hash} ({material_map})")))
            .transpose()?
            .ok_or_else(|| KindError::NotFound(format!("shader: {material_map} holds no code {hash}")))
    })?;
    let code = ShaderCode::decode(&bytes, what).map_err(|e| KindError::Failed(e.to_string()))?;
    if code.no_create {
        return failed(format!("drawmat: {what}: {hash} is DXIL the D3D11 RHI makes no shader of (NoCreate)"));
    }
    reads_custom_stencil(&code, what).map_err(|e| KindError::Failed(e.to_string()))
}

/// One build of the material in `ctx`.
fn probe(cx: &Cx<'_>, material: &str, ctx: &'static DrawContextRow) -> Result<Arc<BannerProbe>, KindError> {
    if !cx.exists(material) {
        return refused(
            "not in the paks (an unresolved import): what the build draws for an emitter whose material does not load \
             is native"
                .into(),
        );
    }
    let what = format!("drawmat {material}?ctx={}", ctx.name);
    let m = resolve_material(cx, material)?;
    let (blend, _) =
        overridden(cx, &m, BaseOverride::BlendMode, "BLEND_Opaque".to_owned(), "BLEND_Opaque".to_owned(), conv_string)?;
    let (shading, _) = overridden(
        cx,
        &m,
        BaseOverride::ShadingModel,
        "MSM_DefaultLit".to_owned(),
        "MSM_DefaultLit".to_owned(),
        conv_string,
    )?;
    if ctx.name == "mesh" {
        // (a mesh particle's Opaque or Masked DefaultLit material that is not emissive-only is the lit builder's)
        let emissive_only = (EMISSIVE_ONLY_METAL.contains(&m.base.as_str()) || EMISSIVE_ONLY_LIT.contains(&material))
            && blend == "BLEND_Masked";
        if shading == "MSM_DefaultLit" && matches!(blend.as_str(), "BLEND_Opaque" | "BLEND_Masked") && !emissive_only {
            return not_ported(&format!("bannerProbe {material}?ctx=mesh: the lit builder's material"));
        }
    }
    let pixel_shader = match base_pass(cx, &m, ctx, &blend, &shading, &what)? {
        Ok(ps) => ps,
        Err(why) => return refused(why),
    };
    let map = material_shader_map(cx, &m.shader_map)?;
    let Some(ps) = find_shader(&map.record, ctx.vertex_factory, pixel_shader) else {
        return failed(format!("{what}: no {pixel_shader} for {}", ctx.vertex_factory.unwrap_or("none")));
    };
    // (the banners' contexts bind the custom stencil: it is read by the base pass or the distortion pass)
    let mut reads = shader_reads_stencil(cx, &m.shader_map, ps, &what)?;
    if ctx.distortion
        && let Some(d) = find_shader(&map.record, ctx.vertex_factory, DISTORTION_PS)
    {
        reads |= shader_reads_stencil(cx, &m.shader_map, d, &what)?;
    }
    let reads = reads && ctx.stencil_bind;
    let layout = material_layout(cx, &m, &map)?;
    let scalars = layout.scalar_params.into_iter().map(|(name, def)| BannerScalar { name, def }).collect();
    Ok(Arc::new(BannerProbe::Built { reads, scalars }))
}

/// The DrawRef a banner's mesh slot or sprite emitter names its material by: the banner context where that build
/// reads the custom stencil or is refused, else the plain one; with the verdict (`bannerRef`).
pub fn banner_ref(
    cx: &Cx<'_>,
    probes: &BannerProbes,
    material: &str,
    plain: Plain,
) -> Result<(String, Arc<BannerProbe>), KindError> {
    let ctx = |n: &str| context(n).expect("a draw context");
    let probe = banner_probe(cx, probes, material, ctx(plain.banner()))?;
    let name = match &*probe {
        BannerProbe::Built { reads: false, .. } => plain.name(),
        _ => plain.banner(),
    };
    Ok((draw_ref(material, ctx(name))?, probe))
}
