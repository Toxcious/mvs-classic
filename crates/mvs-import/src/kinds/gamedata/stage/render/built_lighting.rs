//! What the render part reads of a stage's built lighting: the LightBuildData of the gameplay and lighting levels'
//! MapBuildDataRegistry (each level's PersistentLevel MapBuildData), through the `lighting` kind's entries. It gives a
//! radial light's baked shadow channel (by its LightGuid) and the stage's `tlvShadow` block: the translucency volume's
//! static shadow maps of the injected lights, laid out as one atlas (the block holds the layout and, per light, its
//! registry and LightGuid).

use std::collections::HashMap;

use indexmap::IndexMap;
use mvs_data::stage::{RadialLight, StageLight, TlvLight, TlvShadow};
use mvs_material::jsonbin::{BodyRef, JsonBin};
use serde_json::Value as Json;

use super::super::super::shape::{fail, short_name};
use super::super::context::StageCtx;
use crate::kind::{Item, Kind, KindError};
use crate::kinds::lighting::{LIGHTING_MAGIC, Lighting};
use crate::stage_levels::level_built_data;

/// A light's build data and the registry it is in.
pub struct BuiltLight {
    pub built_data: String,
    pub shadow_map_channel: Option<f64>,
    pub size: (usize, usize),
    /// WorldToLight, row-major (`M00` .. `M33`).
    pub world_to_light: Vec<f64>,
    /// The depth samples (FFloat16 bits, x fastest).
    pub samples: Vec<u16>,
}

/// A stage's built lighting: LightBuildData by LightGuid (32 hex digits, no dashes, as the registry keys it) over the
/// gameplay and lighting levels' registries, a later level's over an earlier one's.
pub struct StageBuild {
    pub lights: HashMap<String, BuiltLight>,
}

/// A JSON number as `Number(v)` reads it: a non-finite one is written as its name.
fn number(v: Option<&Json>) -> f64 {
    match v {
        Some(Json::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Json::String(s)) => match s.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            t => t.parse().unwrap_or(f64::NAN),
        },
        None | Some(Json::Null) => 0.0,
        Some(_) => f64::NAN,
    }
}

/// The built lighting of the stage's gameplay and lighting levels ([level] + subs).
pub fn stage_build(ctx: &StageCtx<'_>) -> Result<StageBuild, KindError> {
    let mut lights: HashMap<String, BuiltLight> = HashMap::new();
    for lv in &ctx.levels.levels {
        if lv.role != "gameplay" && lv.role != "lighting" {
            continue;
        }
        let Some(built) = level_built_data(&lv.objects) else { continue };
        let what = format!("lighting {built}");
        let bytes = Lighting.build(&ctx.cx, &Item::new(&built))?;
        let e = JsonBin::split(LIGHTING_MAGIC, &bytes, &what).map_err(|e| KindError::Failed(e.to_string()))?;
        let header: Json = e.parse(&what).map_err(|e| KindError::Failed(e.to_string()))?;
        let Some(Json::Object(ls)) = header.get("lights") else { continue };
        for (guid, l) in ls {
            let dm = l.get("DepthMap");
            let r: BodyRef =
                serde_json::from_value(dm.and_then(|d| d.get("DepthSamples")).cloned().unwrap_or(Json::Null))
                    .map_err(|e| KindError::Failed(format!("{what}: {guid}: DepthSamples: {e}")))?;
            let raw = e.section(r, &what).map_err(|e| KindError::Failed(e.to_string()))?;
            let samples = raw.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect();
            let size = |k: &str| number(dm.and_then(|d| d.get(k))) as usize;
            let wtl = dm.and_then(|d| d.get("WorldToLight"));
            let mut m = Vec::with_capacity(16);
            for r in 0..4 {
                for k in 0..4 {
                    m.push(number(wtl.and_then(|w| w.get(format!("M{r}{k}")))));
                }
            }
            lights.insert(
                guid.clone(),
                BuiltLight {
                    built_data: built.clone(),
                    shadow_map_channel: l.get("ShadowMapChannel").and_then(Json::as_f64),
                    size: (size("ShadowMapSizeX"), size("ShadowMapSizeY")),
                    world_to_light: m,
                    samples,
                },
            );
        }
    }
    Ok(StageBuild { lights })
}

/// A light's baked shadow channel (ShadowMapChannel), or -1: no build data for it.
pub fn shadow_channel(build: &StageBuild, guid: Option<&str>) -> f64 {
    guid.and_then(|g| build.lights.get(g)).and_then(|l| l.shadow_map_channel).unwrap_or(-1.0)
}

/// A light component's LightGuid as LightBuildData keys it, or none (a Movable light has none).
pub fn light_guid_key(guid: Option<&mvs_data::Value>) -> Option<String> {
    match guid {
        Some(mvs_data::Value::Str(g)) if !g.is_empty() => Some(g.replace('-', "")),
        _ => None,
    }
}

/// An IEEE half's value.
fn half_value(h: u16) -> f64 {
    let e = i32::from((h >> 10) & 0x1f);
    let m = f64::from(h & 0x3ff);
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    if e == 0 {
        s * m * 2f64.powi(-24)
    } else if e == 31 {
        if m != 0.0 { f64::NAN } else { s * f64::INFINITY }
    } else {
        s * 2f64.powi(e - 15) * (1.0 + m / 1024.0)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind_ {
    Directional,
    Point,
    Spot,
}

struct Map_ {
    name: String,
    w: usize,
    h: usize,
    m: Option<Vec<f64>>,
    size: [f64; 2],
    crop: [f64; 4],
    built_data: String,
    guid: String,
}

/// The static shadow depth maps of the lights the stage injects into the translucency lighting volume, or none. The
/// inject's cb0[35].x is 1 when the light's proxy has a StaticShadowDepthMap with data and a texture (no CastShadows
/// test): a Stationary light's FLightComponentMapBuildData DepthMap (ShadowMapSizeX / Y > 0). Laid out as one atlas
/// (`size` its width and height), each light's map `size`, `crop` (x, y, width, height: the box of its texels that can
/// shadow, a point light's under 1.0 and a spot / directional light's at 0.99 or under), the crop's place in the atlas
/// `at`, its WorldToLight matrix (a spot or directional light's alone), its registry `built_data` and its LightGuid
/// `guid`, under its actor name. Left out: a map with no samples; a spot or directional light's whose samples are all
/// over 0.99 or whose matrix is not finite; a point light's whose samples are all 1.0 or more.
pub fn tlv_static_shadows(
    ctx: &StageCtx<'_>,
    lights: &[StageLight],
    radial: &[RadialLight],
    build: &StageBuild,
) -> Result<Option<TlvShadow>, KindError> {
    let names: Vec<&str> =
        lights.iter().map(|l| l.name.as_str()).chain(radial.iter().map(|l| l.name.as_str())).collect();
    let mut twice: Vec<&str> = names
        .iter()
        .enumerate()
        .filter(|(i, x)| names.iter().position(|y| y == *x) != Some(*i))
        .map(|(_, x)| *x)
        .collect();
    twice.sort_unstable();
    twice.dedup();
    if !twice.is_empty() {
        return fail(format!("a light name twice in the stage: {}", twice.join(", ")));
    }
    let mut injected: HashMap<&str, Kind_> = HashMap::new();
    for l in lights {
        if l.affects_world && l.mobility != "Static" && l.translucent {
            injected.insert(&l.name, Kind_::Directional);
        }
    }
    for l in radial {
        if !l.translucent {
            continue;
        }
        if injected.contains_key(l.name.as_str()) {
            return fail(format!("tlv: {} injected twice", l.name));
        }
        injected.insert(&l.name, if l.att[1] == -2.0 { Kind_::Point } else { Kind_::Spot });
    }
    let mut maps: Vec<Map_> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for a in ctx.actors() {
        let name = short_name(a.path()).to_owned();
        let Some(&kind) = injected.get(name.as_str()) else { continue };
        let typ = match short_name(a.class()) {
            "DirectionalLight" => "DirectionalLightComponent",
            "PointLight" => "PointLightComponent",
            "SpotLight" => "SpotLightComponent",
            _ => continue,
        };
        // (one actor per injected light name: two would take one shadow)
        if seen.contains(&name) {
            return fail(format!("tlv: two injected lights named {name}"));
        }
        seen.push(name.clone());
        // (a light with no LightGuid has no build data: Movable)
        let Some(c) = ctx.comps(&a, Some(typ)).into_iter().next() else {
            return fail(format!("tlv: {name}: no {typ}"));
        };
        let cp = ctx.properties(&c)?;
        let Some(guid) = light_guid_key(cp.get("LightGuid")) else { continue };
        let Some(b) = build.lights.get(&guid) else { continue };
        let (w, h) = b.size;
        if !(w > 0 && h > 0 && !b.samples.is_empty()) {
            continue;
        }
        if b.samples.len() != w * h {
            return fail(format!("tlv: {name}: {} depth samples for {w} x {h}", b.samples.len()));
        }
        let depth: Vec<f64> = b.samples.iter().map(|&s| half_value(s)).collect();
        // the texels that can shadow: a point light's under 1.0, a spot / directional light's at 0.99 or under
        let occ = |v: f64| if kind == Kind_::Point { v < 1.0 } else { v <= 0.99 };
        if !depth.iter().any(|&v| occ(v)) || (kind != Kind_::Point && !b.world_to_light.iter().all(|x| x.is_finite())) {
            continue;
        }
        // cropped to the box of those texels: a texel outside it reads as 1.0, which gives what it would
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        for (i, &v) in depth.iter().enumerate() {
            if !occ(v) {
                continue;
            }
            let (x, y) = (i % w, i / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        let crop = [x0, y0, x1 + 1 - x0, y1 + 1 - y0];
        maps.push(Map_ {
            name,
            w: crop[2],
            h: crop[3],
            m: (kind != Kind_::Point).then(|| b.world_to_light.clone()),
            size: [w as f64, h as f64],
            crop: crop.map(|x| x as f64),
            built_data: b.built_data.clone(),
            guid,
        });
    }
    if maps.is_empty() {
        return Ok(None);
    }
    // shelves, tallest first, in a width that keeps the atlas about square (sorted by (-h, -w, name), code points)
    maps.sort_by(|p, q| q.h.cmp(&p.h).then(q.w.cmp(&p.w)).then_with(|| p.name.cmp(&q.name)));
    let area: usize = maps.iter().map(|e| e.w * e.h).sum();
    let widest = maps.iter().map(|e| e.w).max().unwrap_or(0);
    let wide = widest.max(((area as f64) * 1.25).sqrt().ceil() as usize);
    let (mut x, mut y, mut shelf) = (0usize, 0usize, 0usize);
    let mut at: HashMap<&str, [f64; 2]> = HashMap::new();
    for e in &maps {
        if x + e.w > wide {
            (x, y, shelf) = (0, y + shelf, 0);
        }
        at.insert(&e.name, [x as f64, y as f64]);
        x += e.w;
        shelf = shelf.max(e.h);
    }
    let high = y + shelf;
    let mut out: IndexMap<String, TlvLight> = IndexMap::new();
    for e in &maps {
        out.insert(
            e.name.clone(),
            TlvLight {
                at: at[e.name.as_str()],
                size: e.size,
                crop: e.crop,
                world_to_light: e.m.clone(),
                built_data: e.built_data.clone(),
                guid: e.guid.clone(),
            },
        );
    }
    Ok(Some(TlvShadow { size: [wide as f64, high as f64], lights: out }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves() {
        assert_eq!(half_value(0x3c00), 1.0);
        assert_eq!(half_value(0xc000), -2.0);
        assert_eq!(half_value(0x0001), 2f64.powi(-24));
        assert_eq!(half_value(0x7c00), f64::INFINITY);
        assert!(half_value(0x7c01).is_nan());
        assert_eq!(half_value(0x3555), 0.333251953125);
    }

    #[test]
    fn guid_keys() {
        use mvs_data::Value;
        assert_eq!(light_guid_key(Some(&Value::Str("AB-CD".into()))).as_deref(), Some("ABCD"));
        assert_eq!(light_guid_key(Some(&Value::Str(String::new()))), None);
        assert_eq!(light_guid_key(None), None);
    }
}
