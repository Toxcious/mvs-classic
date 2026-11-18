//! A material's instance chain, its parameters merged down the chain and the shader map it draws with.

use indexmap::IndexMap;
use mvs_uasset::natives::frozen::{MaterialMapContent, material_map_content};
use mvs_uasset::value::Value;

use super::super::shadermap::with_material_maps;
use crate::kind::{Cx, KindError};
use crate::objview::{ObjView, field, num, string};

fn failed<T>(why: String) -> Result<T, KindError> {
    Err(KindError::Failed(why))
}

/// A material resolved through its instance chain.
pub struct ResolvedMaterial<'a> {
    /// The material asked for (an instance's own path).
    pub path: String,
    /// The chain's base Material (its path and its object).
    pub base: String,
    pub base_object: ObjView<'a>,
    /// The instances above the base, nearest first.
    pub instances: Vec<ObjView<'a>>,
    /// The shader map the material draws with: the nearest instance with bHasStaticPermutationResource, else the base.
    pub shader_map: String,
    /// That instance's CachedReferencedTextures (the texture table its map indexes); `None` for the base's.
    pub ref_tex: Option<Vec<Option<String>>>,
    /// The nearest instance's value of each parameter, by its full name.
    pub scalars: IndexMap<String, f64>,
    pub vectors: IndexMap<String, [f64; 4]>,
    pub textures: IndexMap<String, Option<String>>,
}

impl ResolvedMaterial<'_> {
    /// A property of the base Material.
    pub fn prop(&self, name: &str) -> Option<&Value> {
        self.base_object.prop(name)
    }
}

/// An array property's elements as structs' member (`ParameterInfo.Name` of each): `(name, value)`.
fn parameter_values<'v>(cx: &Cx<'_>, p: &'v ObjView<'_>, key: &str) -> Result<Vec<(String, &'v Value)>, KindError> {
    let Some(Value::Array(items)) = p.prop(key) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for item in items {
        let Value::Struct(s) = item else {
            return failed(format!("drawmat: {}: a {key} entry is no struct", p.path()));
        };
        let name = match field(s, "ParameterInfo") {
            Some(Value::Struct(i)) => field(i, "Name").and_then(|n| string(cx.cache, n)),
            _ => None,
        };
        let Some(name) = name else { return failed(format!("drawmat: {}: a {key} entry has no name", p.path())) };
        let Some(v) = field(s, "ParameterValue") else {
            return failed(format!("drawmat: {}: {key} {name} has no value", p.path()));
        };
        out.push((name, v));
    }
    Ok(out)
}

/// A colour struct's R, G, B, A as numbers (a missing one is NaN, which JSON writes as null).
pub fn color(v: &Value) -> Option<[f64; 4]> {
    let Value::Struct(s) = v else { return None };
    let c = |k: &str| field(s, k).and_then(num).unwrap_or(f64::NAN);
    Some([c("R"), c("G"), c("B"), c("A")])
}

/// Walks MaterialInstance -> Parent to the base Material, the nearest value of each parameter winning.
pub fn resolve_material<'a>(cx: &Cx<'a>, path: &str) -> Result<ResolvedMaterial<'a>, KindError> {
    let mut scalars = IndexMap::new();
    let mut vectors = IndexMap::new();
    let mut textures = IndexMap::new();
    let mut instances = Vec::new();
    let mut perm: Option<(String, Vec<Option<String>>)> = None;
    let mut cur = path.to_owned();
    let mut depth = 0;
    loop {
        if depth > 32 {
            return failed(format!("drawmat: {path}: an instance chain past 32 parents"));
        }
        depth += 1;
        let o = cx.object(&cur)?;
        if o.class() == "/Script/Engine.Material" {
            return Ok(ResolvedMaterial {
                path: path.to_owned(),
                base: cur.clone(),
                shader_map: perm.as_ref().map_or_else(|| cur.clone(), |p| p.0.clone()),
                ref_tex: perm.map(|p| p.1),
                base_object: o,
                instances,
                scalars,
                vectors,
                textures,
            });
        }
        if !o.class().contains("MaterialInstance") {
            return failed(format!("drawmat: {cur} is a {}, not a material", o.class()));
        }
        if perm.is_none() && matches!(o.prop("bHasStaticPermutationResource"), Some(Value::Bool(true))) {
            let refs = match o.prop("CachedReferencedTextures") {
                Some(Value::Array(a)) => a.iter().map(|v| string(cx.cache, v)).collect(),
                _ => Vec::new(),
            };
            perm = Some((cur.clone(), refs));
        }
        for (name, v) in parameter_values(cx, &o, "ScalarParameterValues")? {
            if !scalars.contains_key(&name) {
                let Some(n) = num(v) else { return failed(format!("drawmat: {cur}: scalar {name} is no number")) };
                scalars.insert(name, n);
            }
        }
        for (name, v) in parameter_values(cx, &o, "VectorParameterValues")? {
            if !vectors.contains_key(&name) {
                let Some(c) = color(v) else { return failed(format!("drawmat: {cur}: vector {name} is no colour")) };
                vectors.insert(name, c);
            }
        }
        for (name, v) in parameter_values(cx, &o, "TextureParameterValues")? {
            if !textures.contains_key(&name) {
                textures.insert(name, string(cx.cache, v));
            }
        }
        let Some(parent) = o.prop("Parent").and_then(|p| string(cx.cache, p)) else {
            return failed(format!("drawmat: {cur} has no parent"));
        };
        instances.push(o);
        cur = parent;
    }
}

/// The FMaterialInstanceBasePropertyOverrides members: the bOverride_ flag's name (without the prefix), the value's
/// member (also the base Material's property).
// Unverified: the constructor values (`init`) are 4.26's source (FMaterialInstanceBasePropertyOverrides), not read
// from the exe.
#[derive(Clone, Copy, Debug)]
pub enum BaseOverride {
    OpacityMaskClipValue,
    BlendMode,
    ShadingModel,
    DitheredLodTransition,
    CastDynamicShadowAsMasked,
    TwoSided,
}

impl BaseOverride {
    fn flag(self) -> &'static str {
        match self {
            BaseOverride::OpacityMaskClipValue => "bOverride_OpacityMaskClipValue",
            BaseOverride::BlendMode => "bOverride_BlendMode",
            BaseOverride::ShadingModel => "bOverride_ShadingModel",
            BaseOverride::DitheredLodTransition => "bOverride_DitheredLODTransition",
            BaseOverride::CastDynamicShadowAsMasked => "bOverride_CastDynamicShadowAsMasked",
            BaseOverride::TwoSided => "bOverride_TwoSided",
        }
    }

    /// The member of the overrides struct and of the base Material.
    fn value(self) -> &'static str {
        match self {
            BaseOverride::OpacityMaskClipValue => "OpacityMaskClipValue",
            BaseOverride::BlendMode => "BlendMode",
            BaseOverride::ShadingModel => "ShadingModel",
            BaseOverride::DitheredLodTransition => "DitheredLODTransition",
            BaseOverride::CastDynamicShadowAsMasked => "bCastDynamicShadowAsMasked",
            BaseOverride::TwoSided => "TwoSided",
        }
    }
}

/// A base property after the instance chain (UMaterialInstance::UpdateOverridableBaseProperties): the nearest
/// instance whose bOverride_<key> is set gives its BasePropertyOverrides member, `init` (the struct's constructor
/// value) when the cooked struct leaves the member out (it writes only members that differ from it); no instance sets
/// it: the base Material's property, else `fallback`. The bool: an instance overrides it. `conv` reads a value as the
/// property's type.
// A flag set with no member beside it takes the struct's value, as the build does.
pub fn overridden<T>(
    cx: &Cx<'_>,
    m: &ResolvedMaterial<'_>,
    key: BaseOverride,
    init: T,
    fallback: T,
    conv: impl Fn(&Cx<'_>, &Value) -> Option<T>,
) -> Result<(T, bool), KindError> {
    let read = |v: &Value| match v {
        Value::Null | Value::Ref(None) => Ok(None),
        v => conv(cx, v)
            .map(Some)
            .ok_or_else(|| KindError::Failed(format!("drawmat: {}: {} is {v:?}", m.path, key.value()))),
    };
    for inst in &m.instances {
        let Some(Value::Struct(bpo)) = inst.prop("BasePropertyOverrides") else { continue };
        if matches!(field(bpo, key.flag()), Some(Value::Bool(true))) {
            let v = match field(bpo, key.value()) {
                Some(v) => read(v)?,
                None => None,
            };
            return Ok((v.unwrap_or(init), true));
        }
    }
    let v = match m.prop(key.value()) {
        Some(v) => read(v)?,
        None => None,
    };
    Ok((v.unwrap_or(fallback), false))
}

/// A string-valued property as the entry writes it.
pub fn conv_string(cx: &Cx<'_>, v: &Value) -> Option<String> {
    string(cx.cache, v)
}

pub fn conv_bool(_: &Cx<'_>, v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

pub fn conv_num(_: &Cx<'_>, v: &Value) -> Option<f64> {
    num(v)
}

/// The material quality level the port draws at: EMaterialQualityLevel High (1), r.MaterialQualityLevel's default and
/// the level PandaGameInstance re-applies at start-up on PC.
// Deviation: the build re-caches every material when the player changes the setting; the port draws High always.
const MATERIAL_QUALITY_LEVEL: i32 = 1;
/// EMaterialQualityLevel::Num: the resource a material draws when it has none of the level.
const QUALITY_NUM: i32 = 4;

/// The shader map record a material draws (UMaterial::GetMaterialResource 142b6ff40, FindMaterialResource
/// 142b6e870): the resource of exactly MATERIAL_QUALITY_LEVEL, else the Num one, with its preshader program. A
/// material with neither (LaserTrail_MAT, whose one resource has no valid map) is refused with that reason.
pub fn material_shader_map(cx: &Cx<'_>, path: &str) -> Result<MaterialMapContent, KindError> {
    with_material_maps(cx, path, |maps| {
        if maps.is_empty() {
            return Err(KindError::NotFound(format!("shadermap: {path} serializes no inline shader map")));
        }
        let of = |level: i32| maps.iter().flatten().find(|m| m.id.quality_level == level);
        let Some(r) = of(MATERIAL_QUALITY_LEVEL).or_else(|| of(QUALITY_NUM)) else {
            let levels: Vec<String> = maps
                .iter()
                .map(|m| m.as_ref().map_or_else(|| "null".to_owned(), |m| m.id.quality_level.to_string()))
                .collect();
            return failed(format!(
                "drawmat: {path} has no shader map of quality {MATERIAL_QUALITY_LEVEL} or Num ({})",
                levels.join(", ")
            ));
        };
        Ok(material_map_content(r, path)?)
    })
}

/// The base Material's bDisableDepthTest (DrawMatHeader.disableDepthTest): a UMaterial property no instance overrides
/// (BasePropertyOverrides holds none), so the instance chain's base decides; false for an opaque or masked `blend`
/// (the chain's, after its overrides): UE 4.26 applies it to translucent passes only.
// Unverified: that no instance's own value counts is not read from the exe (FMaterialResource's depth-test getter).
pub fn disable_depth_test(m: &ResolvedMaterial<'_>, blend: &str) -> bool {
    if blend == "BLEND_Opaque" || blend == "BLEND_Masked" {
        return false;
    }
    matches!(m.prop("bDisableDepthTest"), Some(Value::Bool(true)))
}
