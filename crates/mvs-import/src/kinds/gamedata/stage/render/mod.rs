//! The stage data's render part: meshes, materials, lights, radial lights, tlvShadow, postProcess, bloomIntensity, fog,
//! atmosphere and skyLight. The shadow channels and
//! the tlvShadow block are read from the levels' build data (`built_lighting.rs`).

pub mod built_lighting;
pub mod colour;

use indexmap::IndexMap;
use mvs_data::stage::{
    Atmosphere, Fog, RadialLight, SecondFog, SkyLight, StageLight, StageMaterial, StageMesh, TlvShadow,
};
use mvs_data::{Props, Value};
use mvs_math::js::{max, min};

use self::built_lighting::{light_guid_key, shadow_channel, stage_build, tlv_static_shadows};
use self::colour::{byte_col, col, cos_round, sin_round, srgb_linear, temperature_col};
use super::super::shape::{fail, short_name};
use super::collision::floor::{is_edge_box, traced_boxes};
use super::context::{StageCtx, Vec3, ar, f, s, st, truthy, v3};
use crate::kind::KindError;

/// Where the materials of the stage meshes are: Test_Gym's DEV_ material instances.
const MATS: &str = "/Game/Panda_Main/Maps/Test_Gym/Materials/";
/// The engine's BasicShapes (Plane 100 x 100 centred, normal +Z; Cube 100^3 centred): the renderer draws those.
const BASIC_SHAPES: &str = "/Engine/BasicShapes/";
const D: f64 = std::f64::consts::PI / 180.0;

/// What the render part writes.
pub struct RenderOut {
    pub meshes: Vec<StageMesh>,
    pub materials: IndexMap<String, StageMaterial>,
    pub lights: Vec<StageLight>,
    pub radial_lights: Option<Vec<RadialLight>>,
    pub tlv_shadow: Option<TlvShadow>,
    pub post_process: Option<Vec<String>>,
    pub bloom_intensity: Option<f64>,
    pub fog: Option<Fog>,
    pub atmosphere: Option<Atmosphere>,
    pub sky_light: Option<SkyLight>,
}

/// A number member, `d` where the member is absent or null (`num(v, d)`; a member of another type reads as NaN).
fn num(p: &Props, k: &str, d: f64) -> f64 {
    match p.get(k) {
        None | Some(Value::Null) => d,
        Some(Value::Num(n)) => *n,
        Some(_) => f64::NAN,
    }
}

/// A member's truth value, `d` where it is absent or null (`v ?? d`, then JS truth).
fn or_true(p: &Props, k: &str, d: bool) -> bool {
    match p.get(k) {
        None | Some(Value::Null) => d,
        Some(Value::Bool(b)) => *b,
        v => super::context::js_truthy(v),
    }
}

/// A light component's mobility name (Stationary unless the level says otherwise: ALight sets it, 4.26 source; the exe
/// read covers ADirectionalLight alone, 142b34fc0).
fn mobility(lc: &Props) -> Result<String, KindError> {
    let m = s(lc, "Mobility").unwrap_or("EComponentMobility::Stationary");
    match m.split("::").nth(1) {
        Some(x) => Ok(x.to_owned()),
        None => fail(format!("Mobility {m}")),
    }
}

/// bAffectTranslucentLighting, whether the light is injected into the translucency lighting volume (default true: every
/// level that stores it stores false).
fn translucent(lc: &Props) -> Result<bool, KindError> {
    match lc.get("bAffectTranslucentLighting") {
        None | Some(Value::Null) => Ok(true),
        Some(Value::Bool(b)) => Ok(*b),
        Some(v) => fail(format!("bAffectTranslucentLighting {v:?}")),
    }
}

/// A light's lighting channels 0..2 (FLightingChannels: channel 0 on by default).
fn channels(lc: &Props) -> Vec<bool> {
    let empty = Props::new();
    let ch = st(lc, "LightingChannels").unwrap_or(&empty);
    let b = |k: &str, d: bool| match ch.get(k) {
        None | Some(Value::Null) => d,
        Some(Value::Bool(x)) => *x,
        v => super::context::js_truthy(v),
    };
    vec![b("bChannel0", true), b("bChannel1", false), b("bChannel2", false)]
}

/// A material instance's parent and parameters (colours linear, textures by name).
fn material(ctx: &StageCtx<'_>, path: &str) -> Result<StageMaterial, KindError> {
    let mi = ctx.load(path)?;
    if short_name(mi.class()) != "MaterialInstanceConstant" {
        return fail(format!("{path}: a {}, not a MaterialInstanceConstant", mi.class()));
    }
    let p = ctx.properties(&mi)?;
    let params = |k: &str| -> &[Value] { ar(&p, k).unwrap_or(&[]) };
    let name_of = |v: &Value| -> Result<String, KindError> {
        let o = v.as_obj().and_then(|o| st(o, "ParameterInfo")).and_then(|i| s(i, "Name"));
        o.map(str::to_owned).map_or_else(|| fail(format!("{path}: a parameter with no name")), Ok)
    };
    let mut scalars = IndexMap::new();
    let mut vectors = IndexMap::new();
    let mut textures = IndexMap::new();
    for v in params("ScalarParameterValues") {
        let value = v.as_obj().and_then(|o| o.get("ParameterValue")).and_then(Value::as_f64).unwrap_or(f64::NAN);
        scalars.insert(name_of(v)?, value);
    }
    for v in params("VectorParameterValues") {
        let pv = v.as_obj().and_then(|o| st(o, "ParameterValue"));
        let g = |k: &str| pv.and_then(|o| f(o, k)).unwrap_or(f64::NAN);
        vectors.insert(name_of(v)?, vec![g("R"), g("G"), g("B"), g("A")]);
    }
    for v in params("TextureParameterValues") {
        let n = name_of(v)?;
        let Some(t) = v.as_obj().and_then(|o| s(o, "ParameterValue")) else {
            return fail(format!("{path}: texture parameter {n} names no texture"));
        };
        textures.insert(n, short_name(t).to_owned());
    }
    let Some(parent) = s(&p, "Parent") else { return fail(format!("{path}: no Parent")) };
    Ok(StageMaterial { parent: short_name(parent).to_owned(), scalars, vectors, textures })
}

/// A SpotLight whose root (its LightComponent0) stores no RelativeRotation has its archetype's, ASpotLight's
/// constructor's (-90, 0, 0), pointing down (4.26 source; the build's own evidence: the static shadow maps' WorldToLight
/// of Scooby's SpotLight_1 / SpotLight2_nonVolumetric2 and Batcave's SpotLight_1 look along -Z). An attached root is not
/// covered.
fn spot_default_rot(t: &str, rp: &Props, r: Vec3, n: &str) -> Result<Vec3, KindError> {
    if t != "SpotLight" || rp.contains_key("RelativeRotation") {
        return Ok(r);
    }
    if truthy(rp.get("AttachParent")) {
        return fail(format!("{n}: a SpotLight with no rotation under an attach parent"));
    }
    Ok([-90.0, 0.0, 0.0])
}

#[derive(Default)]
struct Set {
    post_process: Option<Vec<String>>,
    bloom_intensity: Option<f64>,
    fog: Option<Fog>,
    atmosphere: Option<Atmosphere>,
    sky_light: Option<SkyLight>,
}

/// The render part (RENDER_PART.bind).
pub fn bind(ctx: &StageCtx<'_>) -> Result<RenderOut, KindError> {
    let mut meshes: Vec<StageMesh> = Vec::new();
    let mut materials: IndexMap<String, StageMaterial> = IndexMap::new();
    let mut lights: Vec<StageLight> = Vec::new();
    let mut radial: Vec<RadialLight> = Vec::new();
    let mut set = Set::default();
    let build = stage_build(ctx)?;

    let mut add_mesh = |n: &str, loc: Vec3, r: Vec3, sc: Vec3, smc: &Props| -> Result<(), KindError> {
        // (a component that is not drawn: bVisible false (The Court's Floor) or bHiddenInGame)
        if matches!(smc.get("bVisible"), Some(Value::Bool(false))) || truthy(smc.get("bHiddenInGame")) {
            return Ok(());
        }
        // (a mesh that is not one of the engine's BasicShapes is no stage mesh: the renderer draws those alone and the
        // stagelight kind lights them by the Key; Trophy's sky backdrop M003_Sky_BG_01_2 is a scenery step's)
        let Some(sm) = s(smc, "StaticMesh") else { return fail(format!("{n}: no StaticMesh")) };
        if !sm.starts_with(BASIC_SHAPES) {
            return Ok(());
        }
        // (nor is a BasicShape with a material that is not one of Test_Gym's DEV_ instances: Tree Fort's BushesRight3 / 4,
        // an engine Plane with M_BackgroundBushes_Wind_Top)
        let over: &[Value] = ar(smc, "OverrideMaterials").unwrap_or(&[]);
        if over.iter().any(|m| truthy(Some(m)) && !m.as_str().is_some_and(|m| m.starts_with(MATS))) {
            return Ok(());
        }
        let mats: Vec<&str> = over.iter().filter(|m| truthy(Some(m))).filter_map(Value::as_str).collect();
        meshes.push(StageMesh {
            name: n.to_owned(),
            shape: short_name(sm).to_owned(),
            loc: v3(loc),
            rot: v3(r),
            scale: v3(sc),
            material: mats.first().map(|m| short_name(m).to_owned()),
        });
        for m in mats {
            if !materials.contains_key(short_name(m)) {
                materials.insert(short_name(m).to_owned(), material(ctx, m)?);
            }
        }
        Ok(())
    };

    for a in ctx.actors() {
        let t = short_name(a.class());
        let n = short_name(a.path()).to_owned();
        let ap = ctx.properties(&a)?;
        let (loc, r, sc) = ctx.root_xf(&a)?;
        // (a pass-through platform is a Blueprint actor: none of the classes below)
        // A stage block candidate (an actor with a Box tagged TraceEdges): its mesh is drawn unless its Box is the edge
        // box (Training Room large's TrainingFloor: the tagged Box is the floor, its Cube mesh, NoCollision, is drawn).
        let tagged = traced_boxes(ctx, &a)?;
        if !tagged.is_empty() {
            if t == "StaticMeshActor" && !truthy(ap.get("bHidden")) && !is_edge_box(ctx, &a, r, &tagged)? {
                let Some(c) = ctx.comps(&a, Some("StaticMeshComponent")).into_iter().next() else {
                    return fail(format!("{n}: no StaticMeshComponent"));
                };
                let cp = ctx.properties(&c)?;
                add_mesh(&n, loc, r, sc, &cp)?;
            }
            continue;
        }
        // hidden in game (Superman10 of Classic 3 Platform and Training Room large)
        if truthy(ap.get("bHidden")) {
            continue;
        }
        let comp = |cls: &str| -> Result<std::rc::Rc<Props>, KindError> {
            match ctx.comps(&a, Some(cls)).into_iter().next() {
                Some(c) => ctx.properties(&c),
                None => fail(format!("{n}: no {cls}")),
            }
        };
        match t {
            "StaticMeshActor" => {
                let cp = comp("StaticMeshComponent")?;
                add_mesh(&n, loc, r, sc, &cp)?;
            }
            "DirectionalLight" => {
                let lc = comp("DirectionalLightComponent")?;
                lights.push(StageLight {
                    name: n,
                    rot: v3(r),
                    // UE 4.26 default 10 lux
                    intensity: num(&lc, "Intensity", 10.0),
                    color: byte_col(lc.get("LightColor")),
                    affects_world: or_true(&lc, "bAffectsWorld", true),
                    // class defaults from the exe ctors (UDirectionalLightComponent 142955fa0, ULightComponent 142956650)
                    source_angle: num(&lc, "LightSourceAngle", 0.5357),
                    soft_source_angle: num(&lc, "LightSourceSoftAngle", 0.0),
                    specular_scale: num(&lc, "SpecularScale", 1.0),
                    mobility: mobility(&lc)?,
                    channels: channels(&lc),
                    translucent: translucent(&lc)?,
                });
            }
            "PointLight" | "SpotLight" => {
                // A light that is not baked: its direct light is the deferred pass's. Brightness: with
                // bUseInverseSquaredFalloff off the Intensity is used as it is (UPointLightComponent::
                // ComputeLightBrightness, exe 14295d210. Unverified: the spot light's override is taken to be the same).
                // Colour = FLinearColor(LightColor) x brightness (FLightSceneProxy, 4.26 source). Defaults:
                // ULocalLightComponent AttenuationRadius 1000, UPointLightComponent LightFalloffExponent 8,
                // USpotLightComponent cones 0 / 44 with FSpotLightSceneProxy's clamps (stock 4.26. Unverified); a point
                // light's spot angles are (-2, 1): no cone.
                let lc = comp(&format!("{t}Component"))?;
                if !(or_true(&lc, "bAffectsWorld", true) && mobility(&lc)? != "Static") {
                    continue;
                }
                for k in ["IESTexture", "LightFunctionMaterial", "RelativeScale3D", "IntensityUnits"] {
                    if truthy(lc.get(k)) {
                        return fail(format!("{n}: {k}"));
                    }
                }
                if !matches!(lc.get("bUseInverseSquaredFalloff"), Some(Value::Bool(false))) {
                    return fail(format!("{n}: inverse squared falloff"));
                }
                let cast = or_true(&lc, "CastShadows", true) && or_true(&lc, "CastStaticShadows", true);
                // the channel of the light's baked shadow map (FLightComponentMapBuildData, built_lighting.rs); -1: none
                let mut channel = -1.0;
                if cast {
                    let guid = light_guid_key(lc.get("LightGuid"));
                    channel = shadow_channel(&build, guid.as_deref());
                    if channel < 0.0 {
                        return fail(format!("{n}: casts static shadows and has no shadow channel"));
                    }
                }
                let mut att = [1.0 / num(&lc, "AttenuationRadius", 1000.0), -2.0, 1.0];
                if t == "SpotLight" {
                    let inner = min(max(num(&lc, "InnerConeAngle", 0.0), 0.0), 89.0) * D;
                    let outer = min(max(num(&lc, "OuterConeAngle", 44.0) * D, inner + 0.001), 89.0 * D + 0.001);
                    att[1] = cos_round(outer);
                    att[2] = 1.0 / (cos_round(inner) - cos_round(outer));
                }
                let root = ctx.root(&a)?;
                let empty = std::rc::Rc::new(Props::new());
                let rp = match &root {
                    Some(rc) => ctx.properties(rc)?,
                    None => empty,
                };
                let rr = spot_default_rot(t, &rp, r, &n)?;
                let (p, y) = (rr[0] * D, rr[1] * D);
                // bUseTemperature: the colour times MakeFromColorTemperature(Temperature) (temperature_col; exe 1429554b0, no clamp);
                // a Temperature stored without it changes nothing (Scooby's SpotLight_1). ULightComponent's Temperature
                // default 6500 (ctor 142956650, EF "Directional light parameters").
                let tint = if truthy(lc.get("bUseTemperature")) {
                    temperature_col(num(&lc, "Temperature", 6500.0))
                } else {
                    [1.0, 1.0, 1.0]
                };
                let Some(Value::Num(intensity)) = lc.get("Intensity") else {
                    return fail(format!("{n}: no Intensity"));
                };
                let intensity = *intensity;
                let mut light = RadialLight {
                    name: n.clone(),
                    pos: v3(loc),
                    dir: vec![-cos_round(p) * cos_round(y), -cos_round(p) * sin_round(y), -sin_round(p)],
                    color: srgb_linear(lc.get("LightColor"))
                        .iter()
                        .enumerate()
                        .map(|(i, c)| c * intensity * tint[i])
                        .collect(),
                    src: vec![
                        num(&lc, "SourceRadius", 0.0),
                        num(&lc, "SoftSourceRadius", 0.0),
                        num(&lc, "SpecularScale", 1.0),
                    ],
                    att: att.to_vec(),
                    falloff_exponent: num(&lc, "LightFalloffExponent", 8.0),
                    shadow_channel: channel,
                    channels: channels(&lc),
                    translucent: translucent(&lc)?,
                    source_length: None,
                    tangent: None,
                };
                // SourceLength (Throne Room's PointLight_FGFloor_1, a tube light) with the tube's axis, the light's
                // Tangent: the component's local Z in world space (FRotationMatrix's Z row of (Pitch, Yaw, Roll)).
                // Unverified: FPointLightSceneProxy::GetLightShaderParameters' Tangent (WorldToLight's third column) is
                // 4.26 source, not read in the exe.
                if truthy(lc.get("SourceLength")) {
                    let rl = rr[2] * D;
                    let (cp, sp, cy, sy, cr, sr) =
                        (cos_round(p), sin_round(p), cos_round(y), sin_round(y), cos_round(rl), sin_round(rl));
                    light.source_length = lc.get("SourceLength").and_then(Value::as_f64);
                    light.tangent = Some(vec![-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp]);
                }
                radial.push(light);
            }
            "RectLight" => {
                // (Cromulons' one, Scooby No Roof's two: Static, their light is in the lightmaps alone; one that is not
                // baked would be a radial light of a shape the runtime has no kind for)
                let lc = comp("RectLightComponent")?;
                if or_true(&lc, "bAffectsWorld", true) && mobility(&lc)? != "Static" {
                    return fail(format!("{n}: a RectLight that is not Static ({}): no runtime kind", mobility(&lc)?));
                }
            }
            "SkyLight" => {
                let sl = comp("SkyLightComponent")?;
                if or_true(&sl, "bAffectsWorld", true) {
                    let Some(cube) = s(&sl, "Cubemap") else { return fail(format!("{n}: no Cubemap")) };
                    let Some(lower) = col(sl.get("LowerHemisphereColor")) else {
                        return fail(format!("{n}: no LowerHemisphereColor"));
                    };
                    set.sky_light = Some(SkyLight {
                        cubemap: short_name(cube).to_owned(),
                        intensity: num(&sl, "Intensity", 1.0),
                        color: byte_col(sl.get("LightColor")),
                        lower_hemisphere: lower,
                    });
                }
            }
            "ExponentialHeightFog" => {
                // The renderer draws only the first fog component added to the scene (FScene's list, InitFogConstants
                // reads element 0): the persistent (gameplay) level's before the streamed ones', each level in its actor
                // order (export order here). A component is added only when visible (bVisible; a hidden actor is passed
                // over above), its density sum is positive and FogMaxOpacity is (AddFog 142959670). (FogDensity's stock
                // default 0.02 for the test alone.)
                let fc = comp("ExponentialHeightFogComponent")?;
                let empty = Props::new();
                let second = st(&fc, "SecondFogData").unwrap_or(&empty);
                let dens = num(&fc, "FogDensity", 0.02) + num(second, "FogDensity", 0.0);
                if set.fog.is_some()
                    || matches!(fc.get("bVisible"), Some(Value::Bool(false)))
                    || dens <= 0.0
                    || num(&fc, "FogMaxOpacity", 1.0) <= 0.0
                {
                    continue;
                }
                // FogInscatteringColor and FogDensity defaults: UExponentialHeightFogComponent's constructor (stock
                // 4.26: 0.02; Unverified: the exe's ctor not read)
                let mut fog = Fog {
                    density: num(&fc, "FogDensity", 0.02),
                    height_falloff: fc.get("FogHeightFalloff").and_then(Value::as_f64).unwrap_or(f64::NAN),
                    inscatter: col(fc.get("FogInscatteringColor")).unwrap_or_else(|| vec![0.447, 0.638, 1.0]),
                    start_distance: num(&fc, "StartDistance", 0.0),
                    height: loc[2],
                    max_opacity: num(&fc, "FogMaxOpacity", 1.0),
                    cutoff_distance: num(&fc, "FogCutoffDistance", 0.0),
                    second: None,
                };
                // SecondFogData where the component stores one (Batcave), its unstored fields FExponentialHeightFogData's
                // stock 4.26 defaults (density 0, falloff 0.2, offset 0. Unverified); post/fog.rs draws it.
                if truthy(fc.get("SecondFogData")) {
                    fog.second = Some(SecondFog {
                        density: num(second, "FogDensity", 0.0),
                        height_falloff: num(second, "FogHeightFalloff", 0.2),
                        height_offset: num(second, "FogHeightOffset", 0.0),
                    });
                }
                set.fog = Some(fog);
            }
            "AtmosphericFog" => {
                // FAtmosphericFogSceneInfo's constructor (exe 141916700) and the View parameters it feeds (141cda280).
                // Class defaults: UAtmosphericFogComponent's constructor (exe, at 1428b167c); DensityHeight 0.5 is 4.26
                // source.
                let fc = comp("AtmosphericFogComponent")?;
                if r.iter().any(|x| *x != 0.0) || sc.iter().any(|x| *x != 1.0) || fc.contains_key("PrecomputeParams") {
                    return fail(format!("{n}: atmosphere shape"));
                }
                // no directional light is the atmosphere sun light, so the component's default sun applies
                for lv in &ctx.levels.levels {
                    for o in &lv.objects {
                        if short_name(o.class()) == "DirectionalLightComponent"
                            && ctx.properties(o)?.keys().any(|k| k.contains("tmos"))
                        {
                            return fail(format!("{n}: {} is an atmosphere sun light", o.path()));
                        }
                    }
                }
                let ds = num(&fc, "DistanceScale", 1.0);
                let dm = num(&fc, "DensityMultiplier", 1.0);
                let bright = num(&fc, "DefaultBrightness", 50.0);
                set.atmosphere = Some(Atmosphere {
                    sun_power: num(&fc, "SunMultiplier", 1.0),
                    power: num(&fc, "FogMultiplier", 1.0),
                    density_scale: if dm > 0.0 { 1.0 / dm } else { 1.0 },
                    density_offset: num(&fc, "DensityOffset", 0.0),
                    ground_offset: num(&fc, "GroundOffset", -100000.0) + loc[2],
                    distance_scale: ds,
                    altitude_scale: num(&fc, "AltitudeScale", 1.0),
                    height_scale_rayleigh: 0.5f64.powi(3) * 64.0,
                    start_distance: num(&fc, "StartDistance", 15000.0) * ds * 0.00001,
                    distance_offset: num(&fc, "DistanceOffset", 0.0),
                    // toward the sun: minus DefaultSunDirection, the component's X axis
                    light_direction: vec![-1.0, 0.0, 0.0],
                    light_color: srgb_linear(fc.get("DefaultLightColor")).iter().map(|c| c * bright).collect(),
                    altitude_samples: 2.0,
                    render_mask: f64::from(
                        u8::from(truthy(fc.get("bDisableSunDisk")))
                            | (u8::from(truthy(fc.get("bDisableGroundScattering"))) << 1),
                    ),
                });
            }
            "PostProcessVolume" => {
                // The volume's blendables by name, and its BloomIntensity where it overrides the default
                let Some(sg) = st(&ap, "Settings") else { return fail(format!("{n}: no Settings")) };
                let empty = Props::new();
                let wb = st(sg, "WeightedBlendables").unwrap_or(&empty);
                let mut names = Vec::new();
                for b in ar(wb, "Array").unwrap_or(&[]) {
                    let Some(o) = b.as_obj().and_then(|o| s(o, "Object")) else {
                        return fail(format!("{n}: a blendable with no object"));
                    };
                    names.push(short_name(o).to_owned());
                }
                set.post_process = Some(names);
                if truthy(sg.get("bOverride_BloomIntensity")) {
                    set.bloom_intensity = f(sg, "BloomIntensity");
                }
            }
            _ => {}
        }
    }

    let tlv = tlv_static_shadows(ctx, &lights, &radial, &build)?;
    Ok(RenderOut {
        meshes,
        materials,
        lights,
        radial_lights: (!radial.is_empty()).then_some(radial),
        tlv_shadow: tlv,
        post_process: set.post_process,
        bloom_intensity: set.bloom_intensity,
        fog: set.fog,
        atmosphere: set.atmosphere,
        sky_light: set.sky_light,
    })
}
