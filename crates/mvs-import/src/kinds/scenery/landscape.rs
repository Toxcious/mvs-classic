//! A level's landscapes (ALandscape actors and their ULandscapeComponents) for the scenery entry: its `landscape`
//! block, which the renderer draws (`mvs-render` landscape.rs). The Court's Map_M010_Background is the only level of
//! the build's maps with one.
//!
//! Per landscape actor: its root's transform (location, scale; a rotation fails), the section grid of its render
//! system (Min, Size in components: SectionBase / ComponentSizeQuads) and LastLOD (max(MaxLOD, min(first resident
//! heightmap mip, CeilLogTwo(SubsectionSizeVerts) - 1)) = 4 with every mip resident).
//! Unverified: how the build's grid rectangle grows as components register was not read; the grid here is the
//! components' bounding rectangle, and a cell no component fills holds LOD 0.
//! Per component (FLandscapeComponentSceneProxy):
//! - `base`: SectionBaseX / Y in quads, the component's place under the root (its RelativeLocation, which must equal
//!   it) and LandscapeParameters cb[5].zw; `cell`: SectionBase / ComponentSizeQuads (cb[0].xy).
//! - `hm` / `hsb`: its HeightmapTexture (an index into `heightmaps`) and HeightmapScaleBias (cb[2]); `wsb`: its
//!   WeightmapScaleBias (cb[3]); its weightmap and layer masks are its material instance's.
//! - `box`: CachedLocalBox (quads, heights in local units), `sphere`: the bounds sphere in UE world units (the box's
//!   centre and half diagonal through the root). Unverified: that sphere is the proxy's bounds is the 4.26 source's.
//! - `material`: the static permutation its MaterialInstances[0] is a child of, in the landscape context; `vectors`:
//!   the child's VectorParameterValues (LayerMask_*), `weightmap`: the child's Weightmap0 texture (an index into
//!   `weightmaps`). The child has no static permutation of its own and no other parameter (the Heightmap texture
//!   parameter, which the shaders do not read, must be the component's heightmap); anything else fails.
//! - `lm`: its lightmap (by MapBuildDataId; no shadow map here); `channels`: its LightingChannels.
//!
//! A component with more than one subsection, a ComponentSizeQuads other than 31, an XYOffsetmap, tessellation, a LOD
//! bias or a forced LOD fails. Heightmaps and weightmaps (the level's own Texture2D objects) are named by their build
//! object paths.

use indexmap::{IndexMap, IndexSet};
use mvs_math::hypot::hypot3;

use super::bind::SceneryLm;
use super::materials::SceneryMaterials;
use super::part::JsonProps;
use super::parts::util::{class_of, name_of, number, outer_name, props_of, vec};
use crate::json::Json;
use crate::kind::KindError;
use crate::objview::ObjView;
use crate::stage_levels::StageLevel;

/// The core's lightmap of a component: (its name, its MapBuildDataId) -> its `lm`, `None` without build data.
pub type LightmapOf<'f> = dyn Fn(&str, Option<&str>) -> Result<Option<SceneryLm>, KindError> + 'f;

const COMPONENT_PROPS: [&str; 25] = [
    "SectionBaseX",
    "SectionBaseY",
    "ComponentSizeQuads",
    "SubsectionSizeQuads",
    "NumSubsections",
    "MaterialInstances",
    "LODIndexToMaterialIndex",
    "MaterialIndexToDisabledTessellationMaterial",
    "WeightmapScaleBias",
    "WeightmapSubsectionOffset",
    "HeightmapScaleBias",
    "CachedLocalBox",
    "CollisionComponent",
    "HeightmapTexture",
    "WeightmapLayerAllocations",
    "WeightmapTextures",
    "LODStreamingProxy",
    "MapBuildDataId",
    "StateId",
    "CastShadow",
    "bCastFarShadow",
    "LightingChannels",
    "VisibilityId",
    "AttachParent",
    "RelativeLocation",
];
const LANDSCAPE_PROPS: [&str; 12] = [
    "LandscapeGuid",
    "LandscapeMaterial",
    "LandscapeComponents",
    "CollisionComponents",
    "bHasLandscapeGrass",
    "LightingChannels",
    "BodyInstance",
    "ComponentSizeQuads",
    "SubsectionSizeQuads",
    "NumSubsections",
    "bHasLayersContent",
    "RootComponent",
];
const CHILD_PROPS: [&str; 5] =
    ["Parent", "VectorParameterValues", "TextureParameterValues", "BasePropertyOverrides", "CachedReferencedTextures"];

fn fail(msg: String) -> KindError {
    KindError::Failed(msg)
}

fn others<'p>(p: &'p JsonProps, known: &[&str]) -> Vec<&'p str> {
    p.keys().map(String::as_str).filter(|k| !known.contains(k)).collect()
}

fn v4(v: Option<&Json>) -> Json {
    Json::Arr(["X", "Y", "Z", "W"].iter().map(|k| v.and_then(|s| s.get(k)).cloned().unwrap_or(Json::Null)).collect())
}

fn arr(v: Option<&Json>) -> &[Json] {
    v.and_then(Json::as_array).unwrap_or(&[])
}

/// A parameter array's entries as (name, value).
fn params(v: Option<&Json>) -> Vec<(String, Json)> {
    arr(v)
        .iter()
        .map(|e| {
            let n = e.get("ParameterInfo").and_then(|i| i.get("Name")).and_then(Json::as_str).unwrap_or_default();
            (n.to_owned(), e.get("ParameterValue").cloned().unwrap_or(Json::Null))
        })
        .collect()
}

fn index_of(list: &mut IndexSet<String>, actor: &ObjView<'_>, p: Option<&Json>, lvl: &str) -> Result<usize, KindError> {
    let Some(s) = p.and_then(Json::as_str).filter(|s| s.starts_with(&format!("{}.", actor.path()))) else {
        return Err(fail(format!("{lvl} {}: texture {p:?} is not under the actor", name_of(actor))));
    };
    Ok(list.insert_full(s.to_owned()).0)
}

/// The level's landscapes, or `None` for none. `lightmap_of`: the core's, by a component's MapBuildDataId.
pub fn landscapes_of(
    level: &StageLevel<'_>,
    lightmap_of: &LightmapOf<'_>,
    materials: &SceneryMaterials<'_, '_>,
) -> Result<Option<Json>, KindError> {
    let lvl = level.level.as_str();
    let objs = &level.objects;
    let actors: Vec<&ObjView<'_>> = level.actors.iter().map(|&i| &objs[i]).collect();
    if actors.iter().any(|a| matches!(class_of(a), "LandscapeProxy" | "LandscapeStreamingProxy")) {
        return Err(fail(format!("{lvl}: a landscape proxy")));
    }
    let actors: Vec<&ObjView<'_>> = actors.into_iter().filter(|a| class_of(a) == "Landscape").collect();
    if actors.is_empty() {
        return Ok(None);
    }
    let at = |p: Option<&Json>, what: &str| -> Result<&ObjView<'_>, KindError> {
        p.and_then(Json::as_str)
            .and_then(|s| objs.iter().find(|o| o.path() == s))
            .ok_or_else(|| fail(format!("{lvl}: {what}: {p:?} is not an object of the level")))
    };
    let mut heightmaps = IndexSet::new();
    let mut weightmaps = IndexSet::new();
    let mut out = Vec::new();
    for a in actors {
        let an = name_of(a);
        let ap = props_of(a);
        let other = others(&ap, &LANDSCAPE_PROPS);
        if !other.is_empty() {
            return Err(fail(format!("{an}: landscape properties {}", other.join(","))));
        }
        let n = |p: &JsonProps, k: &str| number(p.get(k));
        if n(&ap, "ComponentSizeQuads") != 31.0
            || n(&ap, "SubsectionSizeQuads") != 31.0
            || n(&ap, "NumSubsections") != 1.0
        {
            return Err(fail(format!("{an}: component size")));
        }
        let root = at(ap.get("RootComponent"), &format!("{an} root"))?;
        let rp = props_of(root);
        if rp.keys().any(|k| k != "RelativeLocation" && k != "RelativeScale3D") {
            return Err(fail(format!("{an}: root {}", rp.keys().cloned().collect::<Vec<_>>().join(","))));
        }
        let prefix = format!("{}.", a.path());
        let comps: Vec<&ObjView<'_>> = objs
            .iter()
            .filter(|o| class_of(o) == "LandscapeComponent" && outer_name(o) == an && o.path().starts_with(&prefix))
            .collect();
        if comps.len() != arr(ap.get("LandscapeComponents")).len() {
            return Err(fail(format!("{an}: components")));
        }
        let mut components = Vec::new();
        let mut cells: Vec<[f64; 2]> = Vec::new();
        for c in comps {
            let cn = name_of(c);
            let p = props_of(c);
            let other = others(&p, &COMPONENT_PROPS);
            if !other.is_empty() {
                return Err(fail(format!("{cn}: component properties {}", other.join(","))));
            }
            let or0 = |k: &str| p.get(k).map_or(0.0, |v| number(Some(v)));
            let base = [or0("SectionBaseX"), or0("SectionBaseY")];
            let rel = vec(p.get("RelativeLocation"), [0.0; 3]);
            if rel[0] != base[0] || rel[1] != base[1] || rel[2] != 0.0 {
                return Err(fail(format!("{cn}: placed off its section base")));
            }
            if n(&p, "ComponentSizeQuads") != 31.0
                || n(&p, "SubsectionSizeQuads") != 31.0
                || n(&p, "NumSubsections") != 1.0
                || base.iter().any(|b| b % 31.0 != 0.0)
            {
                return Err(fail(format!("{cn}: component size")));
            }
            let mis = arr(p.get("MaterialInstances"));
            if arr(p.get("LODIndexToMaterialIndex")).iter().any(|i| number(Some(i)) != 0.0)
                || mis.iter().skip(1).any(super::parts::util::json_truthy)
                || arr(p.get("MaterialIndexToDisabledTessellationMaterial")).iter().any(|i| number(Some(i)) != -1.0)
            {
                return Err(fail(format!("{cn}: material per LOD")));
            }
            if at(p.get("AttachParent"), &format!("{cn} attach"))?.path() != root.path() {
                return Err(fail(format!("{cn}: attach")));
            }
            // the material instance: a child of a static permutation, with the component's weightmap and layer masks
            let mi = at(mis.first(), &format!("{cn} material"))?;
            if class_of(mi) != "LandscapeMaterialInstanceConstant" || !mi.path().starts_with(&prefix) {
                return Err(fail(format!("{cn}: material {}", mi.path())));
            }
            let mn_ = name_of(mi);
            let mp = props_of(mi);
            if mp.keys().any(|k| !CHILD_PROPS.contains(&k.as_str()))
                || mp.get("bHasStaticPermutationResource").is_some_and(super::parts::util::json_truthy)
            {
                return Err(fail(format!(
                    "{mn_}: instance properties {}",
                    mp.keys().cloned().collect::<Vec<_>>().join(",")
                )));
            }
            if mp
                .get("BasePropertyOverrides")
                .and_then(Json::as_object)
                .is_some_and(|o| o.keys().any(|k| k != "OpacityMaskClipValue"))
            {
                return Err(fail(format!("{mn_}: property overrides")));
            }
            let parent = at(mp.get("Parent"), &format!("{mn_} parent"))?;
            if class_of(parent) != "LandscapeMaterialInstanceConstant"
                || !parent.path().starts_with(&prefix)
                || !props_of(parent).get("bHasStaticPermutationResource").is_some_and(super::parts::util::json_truthy)
            {
                return Err(fail(format!("{mn_}: parent {}", parent.path())));
            }
            let material = materials.landscape(parent.path())?;
            let tex = params(mp.get("TextureParameterValues"));
            if tex.iter().any(|(n, _)| n != "Weightmap0" && n != "Heightmap") {
                return Err(fail(format!("{mn_}: texture parameters")));
            }
            let hm_tex = p.get("HeightmapTexture").cloned().unwrap_or(Json::Null);
            if tex.iter().find(|(n, _)| n == "Heightmap").is_some_and(|(_, v)| *v != hm_tex) {
                return Err(fail(format!("{mn_}: another heightmap")));
            }
            let wts = arr(p.get("WeightmapTextures"));
            let Some((_, wp)) =
                tex.iter().find(|(n, _)| n == "Weightmap0").filter(|(_, v)| wts.len() == 1 && *v == wts[0])
            else {
                return Err(fail(format!("{mn_}: weightmap")));
            };
            let mut vectors = IndexMap::new();
            for (name, v) in params(mp.get("VectorParameterValues")) {
                let ok = name
                    .strip_prefix("LayerMask_")
                    .is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
                if !ok {
                    return Err(fail(format!("{mn_}: vector parameters")));
                }
                let c = |k: &str| v.get(k).cloned().unwrap_or(Json::Null);
                vectors.insert(name, Json::Arr(vec![c("R"), c("G"), c("B"), c("A")]));
            }
            let id = p.get("MapBuildDataId").map(|v| v.as_str().map_or_else(String::new, |s| s.replace('-', "")));
            let lm = lightmap_of(cn, id.as_deref())?;
            let Some(lm) = lm.filter(|l| !l.shadow) else { return Err(fail(format!("{cn}: build data"))) };
            let b = p.get("CachedLocalBox");
            let is_obj = |v: Option<&Json>| matches!(v, Some(Json::Obj(_)));
            if !b.and_then(|b| b.get("IsValid")).is_some_and(super::parts::util::json_truthy)
                || !is_obj(b.and_then(|b| b.get("Min")))
                || !is_obj(b.and_then(|b| b.get("Max")))
            {
                return Err(fail(format!("{cn}: bounds")));
            }
            let (mn, mx) = (vec(b.and_then(|b| b.get("Min")), [0.0; 3]), vec(b.and_then(|b| b.get("Max")), [0.0; 3]));
            let s = vec(rp.get("RelativeScale3D"), [1.0; 3]);
            let t = vec(rp.get("RelativeLocation"), [0.0; 3]);
            let off = |k: usize| if k < 2 { base[k] } else { 0.0 };
            let lo: [f64; 3] = std::array::from_fn(|k| t[k] + s[k] * (off(k) + mn[k]));
            let hi: [f64; 3] = std::array::from_fn(|k| t[k] + s[k] * (off(k) + mx[k]));
            let ch = p.get("LightingChannels");
            let chan = |k: &str, d: bool| ch.and_then(|c| c.get(k)).cloned().unwrap_or(Json::Bool(d));
            let cell = [base[0] / 31.0, base[1] / 31.0];
            cells.push(cell);
            let nums = |v: &[f64]| Json::Arr(v.iter().map(|x| Json::Num(*x)).collect());
            components.push(Json::object([
                ("name", Json::from(cn)),
                ("base", nums(&base)),
                ("cell", nums(&cell)),
                ("hm", Json::Num(index_of(&mut heightmaps, a, p.get("HeightmapTexture"), lvl)? as f64)),
                ("hsb", v4(p.get("HeightmapScaleBias"))),
                ("wsb", v4(p.get("WeightmapScaleBias"))),
                ("box", nums(&[mn[0], mn[1], mn[2], mx[0], mx[1], mx[2]])),
                (
                    "sphere",
                    nums(&[
                        (lo[0] + hi[0]) / 2.0,
                        (lo[1] + hi[1]) / 2.0,
                        (lo[2] + hi[2]) / 2.0,
                        hypot3((hi[0] - lo[0]) / 2.0, (hi[1] - lo[1]) / 2.0, (hi[2] - lo[2]) / 2.0),
                    ]),
                ),
                ("material", Json::from(material)),
                ("vectors", Json::Obj(vectors)),
                ("weightmap", Json::Num(index_of(&mut weightmaps, a, Some(wp), lvl)? as f64)),
                ("lm", lm.json),
                (
                    "channels",
                    Json::Arr(vec![chan("bChannel0", true), chan("bChannel1", false), chan("bChannel2", false)]),
                ),
            ]));
        }
        let fold = |k: usize, f: fn(f64, f64) -> f64, init: f64| cells.iter().map(|c| c[k]).fold(init, f);
        let min = [fold(0, mvs_math::js::min, f64::INFINITY), fold(1, mvs_math::js::min, f64::INFINITY)];
        let max = [fold(0, mvs_math::js::max, f64::NEG_INFINITY), fold(1, mvs_math::js::max, f64::NEG_INFINITY)];
        let mut seen = IndexSet::new();
        for c in &cells {
            if !seen.insert(format!("{},{}", c[0], c[1])) {
                return Err(fail(format!("{an}: two components in cell {},{}", c[0], c[1])));
            }
        }
        let nums = |v: &[f64]| Json::Arr(v.iter().map(|x| Json::Num(*x)).collect());
        out.push(Json::object([
            ("name", Json::from(an)),
            ("loc", nums(&vec(rp.get("RelativeLocation"), [0.0; 3]))),
            ("scale", nums(&vec(rp.get("RelativeScale3D"), [1.0; 3]))),
            ("grid", nums(&[min[0], min[1], max[0] - min[0] + 1.0, max[1] - min[1] + 1.0])),
            // MaxLOD of SubsectionSizeVerts 32 (all mips resident): ceil(log2(32)) - 1
            ("lastLOD", Json::Num(4.0)),
            ("components", Json::Arr(components)),
        ]));
    }
    let strs = |s: IndexSet<String>| Json::Arr(s.into_iter().map(Json::Str).collect());
    Ok(Some(Json::object([
        ("heightmaps", strs(heightmaps)),
        ("weightmaps", strs(weightmaps)),
        ("actors", Json::Arr(out)),
    ])))
}
