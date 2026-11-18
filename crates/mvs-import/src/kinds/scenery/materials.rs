//! The scenery bind's material choice: which material entry (a DrawRef: material and draw context) each slot of a
//! component draws with, by the component's lighting policy and the material's blend. A context's entry is refused
//! by the two rules of the build's data (the unlit translucency of the static context, the shader
//! map lookup of the lit contexts) and by a table of refusals here.
//!
//! The contexts:
//! - an opaque or masked material on a component with a lightmap in its level's build data: `static` (no shadow map)
//!   or `shadowmap`; one with no build data: `locallm`. An unlit opaque one (base ShadingModel MSM_Unlit, BLEND_Opaque)
//!   takes NoLightMap everywhere: `locallm`, the one shader of every policy. One whose lightmap entry is refused falls
//!   back to its `locallm` entry when an earlier component drew it so, else is not drawn.
//! - a translucent one (BLEND_Translucent or BLEND_Additive after the instance chain's overrides): the effects'
//!   `local`, or on a component with a lightmap the `static` one where it is built, else `local`.
//! - a skeletal mesh's: `skin`.
//!
//! A shadow-mapped component's translucency takes `static`, the HQ lightmap policy's shader. Where the material's
//! distance-field shadows policy has a pixel shader of its own (its FLocalVertexFactory
//! TBasePassPSTDistanceFieldShadowsAndLightMapPolicyHQSkylight is another resource than its
//! TBasePassPSTLightMapPolicyHQSkylight: The Court's and Trophy's waterfalls) the build draws that one, whose extra
//! shadow factors only the forward directional's and the local lights' static terms read: with no forward light on the
//! component's channels both are 0 and the HQ shader draws the same; otherwise the bind fails.

use std::cell::RefCell;

use indexmap::IndexSet;
use mvs_material::draw::context;
use mvs_material::hashed_name;
use mvs_uasset::natives::frozen::{MaterialShaderMapRecord, ShaderRecord, hash64_hex};

use crate::kind::{Cx, KindError, Memo};
use crate::kinds::material::resolve::{BaseOverride, conv_string, material_shader_map, overridden, resolve_material};
use crate::objview::string;

use super::part::SlotLightmap;

fn fail(msg: String) -> KindError {
    KindError::Failed(format!("scenery: {msg}"))
}

/// An enum value's name without its type (`EBlendMode::BLEND_Opaque` -> `BLEND_Opaque`).
pub fn bare(v: &str) -> &str {
    match v.find("::") {
        Some(i) if i > 0 && v[..i].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') => &v[i + 2..],
        _ => v,
    }
}

/// A material in a draw context.
pub fn draw_ref(material: &str, ctx: &str) -> Result<String, KindError> {
    if !material.starts_with('/') || !material.contains('.') || material.contains('?') {
        return Err(fail(format!("drawRef: {material} is not a <package>.<export> path")));
    }
    if context(ctx).is_none() {
        return Err(fail(format!("drawRef: {ctx} is not a draw context")));
    }
    Ok(format!("{material}?ctx={ctx}"))
}

/// The shader of type `ty` a material's map holds, in the mesh map of vertex factory `vf` (`None`: the map's own
/// shaders); by the hashed names the map holds.
pub fn find_shader<'m>(map: &'m MaterialShaderMapRecord, vf: Option<&str>, ty: &str) -> Option<&'m ShaderRecord> {
    let t = hash64_hex(hashed_name(ty));
    let shaders = match vf {
        None => &map.content.shaders,
        Some(vf) => {
            let v = hash64_hex(hashed_name(vf));
            &map.ordered_mesh_shader_maps.iter().find(|m| m.vertex_factory_type_name == v)?.content.shaders
        }
    };
    shaders.iter().find(|s| s.type_ == t)
}

/// A material's state as the choice reads it.
#[derive(Clone, Debug)]
pub struct MatState {
    pub translucent: bool,
    /// Base ShadingModel MSM_Unlit with BLEND_Opaque: NoLightMap in every context.
    pub unlit_opaque: bool,
    /// Base ShadingModel MSM_Unlit, neither opaque nor translucent (masked): drawn with the effects' entry.
    pub unlit_masked: bool,
}

#[derive(Default)]
struct States(Memo<Result<MatState, KindError>>);

/// The blend mode and the shading model after the instance chain's overrides.
fn blend_and_shading(cx: &Cx<'_>, mk: &str) -> Result<(String, String, bool), KindError> {
    let m = resolve_material(cx, mk)?;
    let (blend, _) =
        overridden(cx, &m, BaseOverride::BlendMode, "BLEND_Opaque".into(), "BLEND_Opaque".into(), conv_string)?;
    let (shading, _) =
        overridden(cx, &m, BaseOverride::ShadingModel, "MSM_DefaultLit".into(), "MSM_DefaultLit".into(), conv_string)?;
    let base_unlit = m.prop("ShadingModel").and_then(|v| string(cx.cache, v)).is_some_and(|s| bare(&s) == "MSM_Unlit");
    Ok((bare(&blend).to_owned(), bare(&shading).to_owned(), base_unlit))
}

/// The state of material `mk`, once per run.
pub fn mat_state(cx: &Cx<'_>, mk: &str) -> Result<MatState, KindError> {
    cx.shared.get_or_init(States::default).0.get(mk, || {
        let (blend, _, base_unlit) = blend_and_shading(cx, mk)?;
        let translucent = blend == "BLEND_Translucent" || blend == "BLEND_Additive";
        Ok(MatState {
            unlit_opaque: base_unlit && blend == "BLEND_Opaque",
            unlit_masked: base_unlit && blend != "BLEND_Opaque" && !translucent,
            translucent,
        })
    })
}

/// Why a material entry is not built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The context's base pass shader is not in the material's shader map (`no <shader type> for <vertex factory>`):
    /// an opaque or masked material goes to the effects' entry.
    NoShader(String),
    /// Any other refusal.
    Other(String),
}

const LOCAL_VF: &str = "FLocalVertexFactory";

#[derive(Default)]
struct Probes(Memo<Result<Option<Refusal>, KindError>>);

/// Whether material `mk`'s entry in context `ctx` is built (`None`) or refused, once per run.
pub fn probe(cx: &Cx<'_>, r: &str, mk: &str, ctx: &str) -> Result<Option<Refusal>, KindError> {
    cx.shared.get_or_init(Probes::default).0.get(r, || {
        match ctx {
            "locallm" | "static" | "shadowmap" => lit_probe(cx, mk, ctx),
            // Unverified: the effects, skin, landscape and decal entries the scenery names are taken as built, none
            // of them refused
            _ => Ok(None),
        }
    })
}

/// The lit contexts' refusals (up to its shader lookup; a translucency in `static`:
/// the lighting with the HQ lightmap policy's shader).
fn lit_probe(cx: &Cx<'_>, mk: &str, ctx: &str) -> Result<Option<Refusal>, KindError> {
    let (blend, shading, base_unlit) = blend_and_shading(cx, mk)?;
    let translucent = blend == "BLEND_Translucent" || blend == "BLEND_Additive";
    let m = resolve_material(cx, mk)?;
    let map = material_shader_map(cx, &m.shader_map)?;
    if translucent {
        if ctx != "static" {
            return Err(fail(format!("{mk}: a translucent material asked in {ctx}")));
        }
        // A lightmapped translucency is lit DefaultLit alone (no other lit model is taken; an
        // unlit one has no lightmap policy)
        if shading != "MSM_DefaultLit" {
            let why = if shading == "MSM_Unlit" { "has no lightmap policy here" } else { "not supported yet" };
            return Ok(Some(Refusal::Other(format!("{blend} {shading} {why}"))));
        }
        let ps = "TBasePassPSTLightMapPolicyHQSkylight";
        if find_shader(&map.record, Some(LOCAL_VF), ps).is_none() {
            return Err(fail(format!("{mk}?ctx=static: no {ps} for {LOCAL_VF}")));
        }
        return Ok(None);
    }
    // An unlit opaque material (and in `locallm` an unlit masked one) takes NoLightMap; a lit one the light-map
    // policy of the context's surface (FBasePassMeshProcessor::AddMeshBatch 1419234e0, the build's scene): `locallm`
    // a movable proxy with no texture lightmap (the volumetric lightmap policy), `static` the HQ lightmap, `shadowmap`
    // the distance-field shadows and HQ lightmap; the Skylight variant of every lit one
    let unlit = base_unlit && (blend == "BLEND_Opaque" || (ctx == "locallm" && blend == "BLEND_Masked"));
    let ps = match (unlit, ctx) {
        (true, _) => "TBasePassPSFNoLightMapPolicy",
        (false, "locallm") => "TBasePassPSFPrecomputedVolumetricLightmapLightingPolicySkylight",
        (false, "static") => "TBasePassPSTLightMapPolicyHQSkylight",
        _ => "TBasePassPSTDistanceFieldShadowsAndLightMapPolicyHQSkylight",
    };
    Ok(find_shader(&map.record, Some(LOCAL_VF), ps)
        .is_none()
        .then(|| Refusal::NoShader(format!("no {ps} for {LOCAL_VF}"))))
}

#[derive(Default)]
struct LightmapOnly(Memo<Result<bool, KindError>>);

/// The material's distance-field shadows policy has a pixel shader of its own, or none (a missing one differs from the
/// HQ lightmap one too).
fn has_lightmap_only(cx: &Cx<'_>, mk: &str) -> Result<bool, KindError> {
    cx.shared.get_or_init(LightmapOnly::default).0.get(mk, || {
        let m = resolve_material(cx, mk)?;
        let map = material_shader_map(cx, &m.shader_map)?;
        let df =
            find_shader(&map.record, Some(LOCAL_VF), "TBasePassPSTDistanceFieldShadowsAndLightMapPolicyHQSkylight");
        let hq = find_shader(&map.record, Some(LOCAL_VF), "TBasePassPSTLightMapPolicyHQSkylight");
        Ok(df.map(|s| s.resource_index) != hq.map(|s| s.resource_index))
    })
}

#[derive(Default)]
struct Choice {
    /// The materials a component drew in `locallm`, for the lightmap entry's fallback.
    plain: IndexSet<String>,
    /// Opaque-path materials drawn with the effects' entry.
    effects_route: IndexSet<String>,
    /// The `static` translucencies named (effects, not lit entries).
    static_translucent: IndexSet<String>,
}

/// Whether a component of these lighting channels has forward lights.
pub type FwdLights<'c> = Box<dyn Fn(&[bool]) -> bool + 'c>;

/// One stage's material choice (`SceneryMaterials`).
// Deviation: a lit entry's translated pixel shader is not checked (its interpolant inputs and the Primitive rows it
// reads, which decided an instance's `bounds`), nor its `wpo` inputs, and no list of materials not drawn is kept: those
// belong to a browser renderer and an import log.
pub struct SceneryMaterials<'c, 'a> {
    cx: &'c Cx<'a>,
    /// Whether a component of these lighting channels has forward lights.
    fwd_lights_on: FwdLights<'c>,
    choice: RefCell<Choice>,
}

impl<'c, 'a> SceneryMaterials<'c, 'a> {
    pub fn new(cx: &'c Cx<'a>, fwd_lights_on: FwdLights<'c>) -> Self {
        SceneryMaterials { cx, fwd_lights_on, choice: RefCell::default() }
    }

    /// A component's slots (material paths, `None` for a slot no LOD 0 section draws) for its lightmap (`None`: none)
    /// and lighting channels (`None`: not known; a shadow-mapped LIGHTMAP_ONLY translucency then fails).
    pub fn build_for(
        &self,
        mats: &[Option<String>],
        lm: Option<&SlotLightmap>,
        channels: Option<&[bool]>,
    ) -> Result<Vec<Option<String>>, KindError> {
        mats.iter().map(|mk| mk.as_deref().map_or(Ok(None), |mk| self.slot(mk, lm, channels))).collect()
    }

    fn slot(
        &self,
        mk: &str,
        lm: Option<&SlotLightmap>,
        channels: Option<&[bool]>,
    ) -> Result<Option<String>, KindError> {
        let cx = self.cx;
        let st = mat_state(cx, mk)?;
        // An opaque or masked material whose base pass shader for the component's policy is not cooked is drawn with
        // the effects' entry (`local`, which the effects' builder takes in the unlit or NoLightMap form), on this
        // component and every later one, unless a component without a lightmap drew it in `locallm` before. A masked
        // one of base shading model MSM_Unlit goes there at once: the cooked shader map of an unlit material holds the
        // NoLightMap policy's shaders alone (Trophy's TROPHY_WaterShoreline_MAT, Batcave's screens, Tree Fort's bush
        // cards).
        if !st.translucent && !self.choice.borrow().effects_route.contains(mk) {
            let ctx = match lm {
                _ if st.unlit_opaque => "locallm",
                None => "locallm",
                Some(l) if l.shadow => "shadowmap",
                Some(_) => "static",
            };
            let r = draw_ref(mk, ctx)?;
            let p = if st.unlit_masked { Some(Refusal::Other("unlit masked".into())) } else { probe(cx, &r, mk, ctx)? };
            let Some(why) = p else {
                if ctx == "locallm" {
                    self.choice.borrow_mut().plain.insert(mk.to_owned());
                }
                return Ok(Some(r));
            };
            if ctx != "locallm" && self.choice.borrow().plain.contains(mk) {
                return draw_ref(mk, "locallm").map(Some);
            }
            if !st.unlit_masked && !matches!(why, Refusal::NoShader(_)) {
                return Ok(None);
            }
            self.choice.borrow_mut().effects_route.insert(mk.to_owned());
        }
        if let Some(lm) = lm {
            let r = draw_ref(mk, "static")?;
            if self.refusal(&r, mk, "static")?.is_none() {
                if lm.shadow && has_lightmap_only(cx, mk)? && channels.is_none_or(|c| (self.fwd_lights_on)(c)) {
                    return Err(fail(format!(
                        "{mk}: a component with a shadow map and forward lights, whose policy has its own shader (its shadow factors are not ported)"
                    )));
                }
                self.choice.borrow_mut().static_translucent.insert(r.clone());
                return Ok(Some(r));
            }
        }
        let r = draw_ref(mk, "local")?;
        Ok(self.refusal(&r, mk, "local")?.is_none().then_some(r))
    }

    /// The entry's refusal.
    // (five lightmapped translucencies of ScoobyMansion and M010 in `static` in a stage with a reflection capture are
    // not refused: the cooked shaders draw them, the renderer's TranslucentBasePass binding no capture)
    fn refusal(&self, r: &str, mk: &str, ctx: &str) -> Result<Option<Refusal>, KindError> {
        probe(self.cx, r, mk, ctx)
    }

    /// An effects' entry: a `local` one, or a `static` translucency.
    pub fn is_effect(&self, r: &str) -> bool {
        r.ends_with("?ctx=local") || self.choice.borrow().static_translucent.contains(r)
    }

    /// A skeletal mesh's material: `skin`.
    pub fn skin(&self, mk: &str, name: &str) -> Result<Option<String>, KindError> {
        let r = draw_ref(mk, "skin")?;
        if let Some(why) = probe(self.cx, &r, mk, "skin")? {
            return Err(fail(format!("{name}: material not built {mk} ({why:?})")));
        }
        Ok(Some(r))
    }

    /// A landscape component's material: `landscape`.
    pub fn landscape(&self, mk: &str) -> Result<String, KindError> {
        self.built(mk, "landscape")
    }

    /// A translucency drawn with its own scalars (the blast border): `local`.
    pub fn local(&self, mk: &str) -> Result<String, KindError> {
        self.built(mk, "local")
    }

    /// A decal material: `decal`.
    pub fn decal(&self, mk: &str) -> Result<String, KindError> {
        self.built(mk, "decal")
    }

    fn built(&self, mk: &str, ctx: &str) -> Result<String, KindError> {
        let r = draw_ref(mk, ctx)?;
        match probe(self.cx, &r, mk, ctx)? {
            None => Ok(r),
            Some(why) => Err(fail(format!("{ctx} material {mk} not built ({why:?})"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_refs() {
        assert_eq!(bare("EBlendMode::BLEND_Opaque"), "BLEND_Opaque");
        assert_eq!(bare("BLEND_Opaque"), "BLEND_Opaque");
        assert_eq!(draw_ref("/Game/A/M.M", "static").unwrap(), "/Game/A/M.M?ctx=static");
        assert!(draw_ref("/Game/A/M", "static").is_err());
        assert!(draw_ref("/Game/A/M.M", "nope").is_err());
    }
}
