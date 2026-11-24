//! The stage set on the renderer (its lights and sky): its static meshes with their lightmaps, the View members its sky
//! light and volumetric lightmap set, and its lights.
//!
//! What a stage draws comes from the store: the stagedraw kind names its meshes' materials and its sky, stagelight
//! and lighting entries; the gamedata stage places the Lab's meshes and holds the lights; the scenery kind places the
//! scenery's static and skeletal meshes; the lighting kind holds the levels' volumetric lightmaps; the sky kind the
//! processed sky.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use mvs_d3d::Direct3D11::ID3D11Resource;
use mvs_d3d::Dxgi::Common::*;
use mvs_d3d::core::Interface;
use mvs_data::stage::{RadialLight, StageData, StageLight};
use mvs_material::jsonbin::{BodyRef, JsonBin};
use mvs_view::{M4, MaterialRef, MeshPose};
use serde::Deserialize;

use crate::assets::upload_texture;
use crate::draw::LightmapTextures;
use crate::gpu::{BufDesc, BufKind, Gpu, Recorded, Subresource, TexDesc, TexKind, TexViews, Texture};
use crate::landscape::Landscape;
use crate::material::LightMapScene;
use crate::renderer::{Renderer, StaticLightmap, StaticMesh};
use crate::scene::{Light, LightKind, LightSet, ReflectionCapture, SkyLight};
use crate::stagedraw::StageDraw;
use crate::view::{LightmapParams, ViewMembers};
use crate::{Error, Result};

pub(crate) fn bad(what: &str, why: impl Into<String>) -> Error {
    Error::Format { what: what.to_string(), why: why.into() }
}

/// A stage's key from its MapData or map name, in any case, with or without `Map_` (`Map_M010`, `m010`).
pub fn stage_key(map: &str) -> String {
    let lower = map.to_ascii_lowercase();
    lower.strip_prefix("map_").unwrap_or(&lower).to_string()
}

/// What a stage's set reads and makes that needs no renderer: its entries, static shadows, volumetric lightmap,
/// sky and atmosphere members, and the scenery's parse with its statics' meshes and lightmaps. Made on a loader thread
/// (`Renderer::stage_job`) or by `Renderer::set_stage`, taken by `Renderer::set_stage_prepared`.
pub struct StagePrep {
    key: String,
    draw: StageDraw,
    data: StageData,
    shadows: crate::lighting::tlv::StaticShadows,
    vlm: Option<VlmVolume>,
    view: ViewMembers,
    sky: SkyLight,
    sc: crate::scenery::SceneryOut,
    /// A loader's recorded copies and writes (None: made on the frame thread's context).
    recorded: Option<Recorded>,
    /// Its time, ms.
    pub ms: f64,
}

impl std::fmt::Debug for StagePrep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StagePrep").field("key", &self.key).field("ms", &self.ms).finish_non_exhaustive()
    }
}

/// A stage's prep for a loader thread (`Renderer::stage_job`, then [`StageJob::run`] anywhere).
#[derive(Debug)]
pub struct StageJob {
    gpu: Gpu,
    assets: Arc<crate::Assets>,
    map: String,
}

impl StageJob {
    /// The stage's prep on this thread, with a deferred context of its own.
    pub fn run(self) -> Result<StagePrep> {
        let gpu = self.gpu.deferred()?;
        let mut p = prepare_stage(&gpu, &self.assets, &self.map)?;
        p.recorded = Some(gpu.recorded()?);
        Ok(p)
    }
}

/// [`StagePrep`] of the stage `map` made with `gpu`'s context. The landscape's textures and the foliage's meshes are
/// read into the shared caches for the set.
fn prepare_stage(gpu: &Gpu, assets: &crate::Assets, map: &str) -> Result<StagePrep> {
    let t0 = std::time::Instant::now();
    let key = stage_key(map);
    let store = assets.store();
    let draw = StageDraw::load(store, &key)?;
    let data = stage_data(store, &key)?;
    let shadows = crate::lighting::tlv::static_shadows(gpu, store, data.tlv_shadow.as_ref())?;
    let vlm = {
        let pack = store.kind("lighting")?;
        let entries = draw.lighting.vlm.iter().map(|p| LightingEntry::read(&pack, p)).collect::<Result<Vec<_>>>()?;
        if entries.is_empty() { None } else { Some(VlmVolume::merge(&entries)?) }
    };
    let sky = load_sky(gpu, assets, &draw.sky)?;
    let sc = crate::scenery::scenery(assets, &draw.scenery.entry)?;
    let mut view = ViewMembers::default();
    if let Some(v) = &vlm {
        v.upload(gpu, &mut view)?;
    }
    sky_members(gpu, &sky, &mut view)?;
    if let Some(a) = &data.atmosphere {
        atmosphere_members(a, &mut view)?;
    }
    if let Some(path) = &draw.atmosphere {
        let table = load_inscatter(gpu, store, path)?;
        view.set_texture("AtmosphereInscatterTexture", Arc::new(table));
    }
    // (what the set reads through the caches: a failure is the set's to meet)
    if let Some(l) = &sc.landscape {
        for p in l.heightmaps.iter().chain(&l.weightmaps) {
            let _ = assets.texture(p);
        }
    }
    for p in sc.meshes.values() {
        let _ = assets.mesh(p);
    }
    let ms = t0.elapsed().as_secs_f64() * 1e3;
    Ok(StagePrep { key, draw, data, shadows, vlm, view, sky, sc, recorded: None, ms })
}

impl Renderer {
    /// The stage to draw, by MapData name: fills `statics`, `landscape`, `members`, `lightmap_scene`, `lights`,
    /// the decal pass's DecalActors and `level_emitters` (the caller hands them to `Fx::begin_level`).
    pub fn set_stage(&mut self, map: &str) -> Result<()> {
        let p = prepare_stage(self.gpu(), self.assets(), map)?;
        self.set_stage_prepared(p)
    }

    /// The prep of the stage `map` for a loader thread.
    pub fn stage_job(&self, map: &str) -> StageJob {
        StageJob { gpu: self.gpu().share(), assets: self.assets.clone(), map: map.to_owned() }
    }

    /// [`Renderer::set_stage`] on a stage's prep.
    pub fn set_stage_prepared(&mut self, p: StagePrep) -> Result<()> {
        if let Some(r) = &p.recorded {
            self.gpu().run_recorded(r);
        }
        let StagePrep { key, draw, data, shadows, vlm, view, sky, mut sc, .. } = p;
        let store = self.assets().store();
        let (mut statics, key_channel) = stage_meshes(self, &draw, &data)?;
        let mut run = crate::scenery::SceneryRun::new(
            store,
            statics.len(),
            std::mem::take(&mut sc.links),
            std::mem::take(&mut sc.instances),
            std::mem::take(&mut sc.skeletal),
            std::mem::take(&mut sc.skeletal_meshes),
            &sc.blocks,
        )?;
        // (the blocks' own skeletal meshes after the entry's, their links after the entry's)
        let own = run.own_statics(self)?;
        statics.append(&mut sc.statics);
        statics.extend(own);
        statics.extend(crate::ism::foliage_statics(self, &mut sc)?);
        // A lightmap in a level with no volumetric lightmap (one with no build data) is refused.
        if vlm.is_none() && statics.iter().any(|s| s.lightmap.is_some()) {
            return Err(bad(&draw.stage, "a lightmap in a level with no volumetric lightmap"));
        }

        // A level with no build data (Map_Classic3Platform_Small / _Large) has no volumetric lightmap: every lit
        // primitive takes the NoLightMap policy (FBasePassMeshProcessor::AddMeshBatch 1419234e0).
        let mut scene = LightMapScene::STAGE;
        if vlm.is_none() {
            scene.volumetric_lightmap = false;
        }
        let (directional, radial) = lights_of(&data, key_channel)?;
        eprintln!(
            "stage {key}: {} static meshes ({} lightmapped), {} directional and {} radial lights, volumetric \
             lightmap {}, atmospheric fog {}",
            statics.len(),
            statics.iter().filter(|s| s.lightmap.is_some()).count(),
            directional.len(),
            radial.len(),
            vlm.as_ref().map_or("none".to_string(), |v| format!("{:?} bricks of {}", v.brick_dims, v.brick_size)),
            if view.texture("AtmosphereInscatterTexture").is_some() { "and its table" } else { "none" },
        );

        self.set_post_stage(&data, &draw)?;
        let landscape = match &sc.landscape {
            Some(l) => Some(Landscape::new(self, l, &sc.lightmaps, &draw)?),
            None => None,
        };
        self.set_decal_stage(&sc.decals)?;
        // (BlastBoxPlayerExit's BeginPlay: the edge around the box as the map has it; `set_blast_box` follows
        // World.blast from then on)
        let mut border = sc.blast_border.take();
        if let Some(b) = &mut border {
            let bb = data.blast_boxes.get("CollisionComp").ok_or_else(|| bad(&key, "no blast box CollisionComp"))?;
            b.place(&mvs_sim::world::BlastBounds {
                left: mvs_sim::math::narrow(-bb.y[1]),
                right: mvs_sim::math::narrow(-bb.y[0]),
                bottom: mvs_sim::math::narrow(bb.z[0]),
                top: mvs_sim::math::narrow(bb.z[1]),
            });
        }

        // The stage's members replace the last stage's; the lighting's and the post's stay.
        let names: Vec<&str> = view.rows.iter().map(|r| r.0).chain(view.textures.iter().map(|t| t.0)).collect();
        self.members.rows.retain(|r| !STAGE_MEMBERS.contains(&r.0));
        self.members.textures.retain(|t| !STAGE_MEMBERS.contains(&t.0));
        debug_assert!(names.iter().all(|n| STAGE_MEMBERS.contains(n)), "{names:?}");
        self.members.rows.extend(view.rows.iter().cloned());
        self.members.textures.extend(view.textures.iter().cloned());
        self.statics = statics;
        self.landscape = landscape;
        self.level_emitters = sc.emitters;
        self.blast_border = border;
        self.scenery = Some(run);
        self.lightmap_scene = scene;
        self.lights = LightSet { directional, radial, sky: Some(sky), view };
        self.tlv.set_shadows(shadows);
        Ok(())
    }
}

/// The View members and resources a stage sets.
const STAGE_MEMBERS: &[&str] = &[
    "VolumetricLightmapWorldToUVScale",
    "VolumetricLightmapWorldToUVAdd",
    "VolumetricLightmapIndirectionTextureSize",
    "VolumetricLightmapBrickSize",
    "VolumetricLightmapBrickTexelSize",
    "VolumetricLightmapIndirectionTexture",
    "VolumetricLightmapBrickAmbientVector",
    "VolumetricLightmapBrickSHCoefficients0",
    "VolumetricLightmapBrickSHCoefficients1",
    "VolumetricLightmapBrickSHCoefficients2",
    "VolumetricLightmapBrickSHCoefficients3",
    "VolumetricLightmapBrickSHCoefficients4",
    "VolumetricLightmapBrickSHCoefficients5",
    "SkyLightColor",
    "SkyIrradianceEnvironmentMap",
    "AtmosphericFogSunPower",
    "AtmosphericFogPower",
    "AtmosphericFogDensityScale",
    "AtmosphericFogDensityOffset",
    "AtmosphericFogGroundOffset",
    "AtmosphericFogDistanceScale",
    "AtmosphericFogAltitudeScale",
    "AtmosphericFogHeightScaleRayleigh",
    "AtmosphericFogStartDistance",
    "AtmosphericFogDistanceOffset",
    "AtmosphereLightDirection[0]",
    "AtmosphereLightColor[0]",
    "AtmosphericFogRenderMask",
    "AtmosphericFogInscatterAltitudeSampleNum",
    "AtmosphereInscatterTexture",
];

/// The light-map choice's scene a stage sets (`Renderer::set_stage`), read from its draw entry alone: a stage with no
/// volumetric lightmap takes the NoLightMap policy.
pub(crate) fn stage_lightmap_scene(store: &mvs_store::Store, map: &str) -> Result<LightMapScene> {
    let draw = StageDraw::load(store, &stage_key(map))?;
    let mut scene = LightMapScene::STAGE;
    scene.volumetric_lightmap = !draw.lighting.vlm.is_empty();
    Ok(scene)
}

pub(crate) fn stage_data(store: &mvs_store::Store, key: &str) -> Result<StageData> {
    let pack = store.kind(mvs_data::KIND)?;
    let path = mvs_data::path::stage(key);
    let e: mvs_data::StageEntry = mvs_data::entry(&pack, &path).map_err(|e| bad(&path, e.to_string()))?;
    Ok(e.data)
}

/// FRotationTranslationMatrix of (pitch, yaw, roll) in degrees, scaled, at `loc`: UE's row-vector form.
pub fn transform(loc: [f64; 3], rot: [f64; 3], scale: [f64; 3]) -> M4 {
    let [p, y, r] = rot.map(f64::to_radians);
    let (sp, cp, sy, cy, sr, cr) = (p.sin(), p.cos(), y.sin(), y.cos(), r.sin(), r.cos());
    let rows = [
        [cp * cy, cp * sy, sp],
        [sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp],
        [-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp],
    ];
    let mut m = [[0.0; 4]; 4];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = rows[i][j] * scale[i];
        }
        m[3][i] = loc[i];
    }
    m[3][3] = 1.0;
    m
}

/// The row-vector M4 as MeshPose's FMatrix3x4 (the three columns, each with its translation).
pub fn to_world(m: &M4) -> [f32; 12] {
    let mut o = [0.0; 12];
    for c in 0..3 {
        for r in 0..4 {
            o[4 * c + r] = m[r][c] as f32;
        }
    }
    o
}

fn v3(v: &[f64], what: &str) -> Result<[f64; 3]> {
    <[f64; 3]>::try_from(v).map_err(|_| bad(what, format!("{} values for a vector of 3", v.len())))
}

/// Lighting channels as LightingChannelMask: channel k at bit k.
pub(crate) fn channel_mask(c: &[bool]) -> u8 {
    c.iter().enumerate().fold(0, |m, (k, on)| m | ((*on as u8) << k))
}

// ---- The Lab's stage meshes ----

/// The stagelight kind's entry: the stage meshes' lightmaps by mesh name.
#[derive(Deserialize)]
struct StageLightEntry {
    lightmaps: HashMap<String, StageLightmap>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StageLightmap {
    lightmap: StagePath,
    scale: [[f32; 4]; 2],
    add: [[f32; 4]; 2],
    coord_scale: [f32; 2],
    coord_bias: [f32; 2],
    shadow: StageShadowMap,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StageShadowMap {
    path: String,
    coord_scale: [f32; 2],
    coord_bias: [f32; 2],
    channel_valid: [bool; 4],
    inv_penumbra: [f32; 4],
    /// The Key's channel (its LightGuid's index in the map's LightGuids), -1 for none.
    key_channel: i8,
}

#[derive(Deserialize)]
struct StagePath {
    path: String,
}

/// The stage meshes (the engine BasicShapes with Test_Gym's DEV_ materials: the Lab's floor, wall and stripes), with
/// their cooked materials and, where the level's build data has one, their HQ lightmap and shadow map.
// Unverified: lighting channel 1 alone, as the reader refuses a stage mesh that stores another (the
// StageData mesh holds no channels).
/// With them the Key's channel in their shadow maps (the first lightmapped mesh's, as the reader asks
/// it), -1 for none.
// Deviation: the Lab's DEV_ materials are drawn with their cooked shaders, not ported by hand.
fn stage_meshes(r: &Renderer, draw: &StageDraw, data: &StageData) -> Result<(Vec<StaticMesh>, i8)> {
    if draw.meshes.is_empty() {
        return Ok((Vec::new(), -1));
    }
    let store = r.assets().store();
    let pack = store.kind("stagelight")?;
    let bytes = pack.bytes(&draw.stagelight)?;
    let light: StageLightEntry = serde_json::from_slice(&bytes).map_err(|e| bad(&draw.stagelight, e.to_string()))?;
    if draw.meshes.len() != data.meshes.len() {
        return Err(bad(
            &draw.stage,
            format!("{} stagedraw meshes, {} in the data", draw.meshes.len(), data.meshes.len()),
        ));
    }
    let mut out = Vec::new();
    let mut key_channel = None;
    for (d, m) in draw.meshes.iter().zip(&data.meshes) {
        if d.name != m.name {
            return Err(bad(&draw.stage, format!("stagedraw mesh {} where the data has {}", d.name, m.name)));
        }
        let what = &m.name;
        let pose = MeshPose {
            mesh: Arc::from(d.mesh.as_str()),
            to_world: to_world(&transform(v3(&m.loc, what)?, v3(&m.rot, what)?, v3(&m.scale, what)?)),
            materials: d
                .materials
                .iter()
                .map(|p| MaterialRef { path: Arc::from(p.as_deref().unwrap_or("")), ..Default::default() })
                .collect(),
            ..Default::default()
        };
        let lightmap = match light.lightmaps.get(&m.name) {
            None => None,
            Some(lm) => {
                let sh = &lm.shadow;
                // Only a Key baked in channel 0 shadows it.
                if sh.key_channel > 0 {
                    return Err(bad(what, format!("Key shadow in channel {}", sh.key_channel)));
                }
                if sh.key_channel == 0 && !sh.channel_valid[0] {
                    return Err(bad(what, "shadow map channel 0 invalid"));
                }
                key_channel.get_or_insert(sh.key_channel);
                let tex = r.assets().texture(&lm.lightmap.path)?;
                // FShadowMap2D: StaticShadowMapMasks a channel's 1 where it is valid.
                let masks = sh.channel_valid.map(|v| if v { 1.0 } else { 0.0 });
                Some(StaticLightmap {
                    params: LightmapParams {
                        static_shadow_map_masks: masks,
                        inv_uniform_penumbra_sizes: sh.inv_penumbra,
                        lightmap_coordinate_scale_bias: [
                            lm.coord_scale[0],
                            lm.coord_scale[1],
                            lm.coord_bias[0],
                            lm.coord_bias[1],
                        ],
                        shadow_map_coordinate_scale_bias: [
                            sh.coord_scale[0],
                            sh.coord_scale[1],
                            sh.coord_bias[0],
                            sh.coord_bias[1],
                        ],
                        lightmap_scale: lm.scale,
                        lightmap_add: lm.add,
                    },
                    // The sky occlusion and AO mask registers take the lightmap texture: the pass never samples
                    // them (View.SkyLightApplyPrecomputedBentNormalShadowingFlag 0).
                    textures: Arc::new(LightmapTextures {
                        lightmap: Some(tex.clone()),
                        sky_occlusion: Some(tex.clone()),
                        ao_material_mask: Some(tex),
                        static_shadow: Some(r.assets().texture(&sh.path)?),
                    }),
                    shadow_map: true,
                })
            }
        };
        out.push(StaticMesh { pose, lightmap, channels: 0b010, instances: None, receives_decals: true, sort: 0.0 });
    }
    Ok((out, key_channel.unwrap_or(-1)))
}

// ---- The lights ----

/// FDirectionalLightSceneProxy::GetLightShaderParameters' source radius of an angle in degrees (exe 1429680dd):
/// sin(0.5 x angle), in float32.
fn source_radius(deg: f64) -> f32 {
    ((deg as f32) * 0.008726646f32).sin()
}

/// FLinearColor(FColor): the sRGB curve on a 0..1 channel.
fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// A directional light: one that affects the world and is not Static (a Static one's light is in the lightmaps).
/// `key_channel`: the Key's channel in the stage meshes' shadow map, -1 for none; the Key is
/// the world-affecting directional light on lighting channel 1 alone. Every other
/// directional light has no precomputed shadow factor.
fn directional(l: &StageLight, key_channel: i8) -> Option<Light> {
    if !l.affects_world || l.mobility == "Static" {
        return None;
    }
    let (p, y) = (l.rot[0].to_radians(), l.rot[1].to_radians());
    // FRotator::Vector: the direction the light travels.
    let f = [p.cos() * y.cos(), p.cos() * y.sin(), p.sin()];
    Some(Light {
        name: l.name.clone(),
        kind: LightKind::Directional,
        position: [0.0; 3],
        direction: f.map(|x| x as f32),
        // FDirectionalLightSceneProxy: Tangent = -GetDirection(), toward the light.
        tangent: f.map(|x| -x as f32),
        color: [0, 1, 2].map(|k| (srgb_to_linear(l.color[k]) * l.intensity) as f32),
        inv_radius: 0.0,
        falloff_exponent: 0.0,
        spot_angles: [0.0; 2],
        source_radius: source_radius(l.source_angle),
        soft_source_radius: source_radius(l.soft_source_angle),
        source_length: 0.0,
        specular_scale: l.specular_scale as f32,
        channels: channel_mask(&l.channels),
        shadow_channel: if channel_mask(&l.channels) == 0b010 { key_channel } else { -1 },
        cast_shadows: false,
        translucent: l.translucent,
    })
}

/// A point or spot light as data/stage/render bound it: `dir` is toward the light (the spot's axis reversed), `att`
/// InvRadius and the spot angles ((-2, 1) for a point light), `src` the source radius, soft source radius and
/// specular scale; the colour already linear times the brightness.
fn radial(l: &RadialLight) -> Result<Light> {
    let dir = [0, 1, 2].map(|k| l.dir[k]);
    // A stationary light's ShadowMapChannel (GBufferE's precomputed shadow factor), -1 for none.
    let c = l.shadow_channel;
    if !(-1.0..=3.0).contains(&c) || c.fract() != 0.0 {
        return Err(bad(&l.name, format!("shadow channel {c}")));
    }
    Ok(Light {
        name: l.name.clone(),
        kind: if l.att[1] == -2.0 { LightKind::Point } else { LightKind::Spot },
        position: [l.pos[0], l.pos[1], l.pos[2]],
        direction: dir.map(|x| -x as f32),
        tangent: l.tangent.as_ref().map_or(dir, |t| [t[0], t[1], t[2]]).map(|x| x as f32),
        color: [0, 1, 2].map(|k| l.color[k] as f32),
        inv_radius: l.att[0] as f32,
        falloff_exponent: l.falloff_exponent as f32,
        spot_angles: [l.att[1] as f32, l.att[2] as f32],
        source_radius: l.src[0] as f32,
        soft_source_radius: l.src[1] as f32,
        source_length: l.source_length.unwrap_or(0.0) as f32,
        specular_scale: l.src[2] as f32,
        channels: channel_mask(&l.channels),
        shadow_channel: c as i8,
        cast_shadows: false,
        translucent: l.translucent,
    })
}

fn lights_of(data: &StageData, key_channel: i8) -> Result<(Vec<Light>, Vec<Light>)> {
    let d = data.lights.iter().filter_map(|l| directional(l, key_channel)).collect();
    let r = data.radial_lights.iter().flatten().map(radial).collect::<Result<_>>()?;
    Ok((d, r))
}

// ---- The menus' levels (crate::menuscene) ----

/// A menu level's View members: `base` (the lighting's and the post's) without a stage's, with the volumetric
/// lightmap of the lighting kind's registry `vlm` and no sky light.
pub(crate) fn menu_vlm_members(r: &Renderer, vlm: &str) -> Result<ViewMembers> {
    let pack = r.assets().store().kind("lighting")?;
    let volume = VlmVolume::merge(&[LightingEntry::read(&pack, vlm)?])?;
    let mut view = ViewMembers::default();
    volume.upload(r.gpu(), &mut view)?;
    Ok(view)
}

/// The podium's own light: the volumetric lightmap of the lighting kind's registry `vlm` and the
/// sky kind's entry ending in `sky_suffix`, as View members (the sky light's colour and irradiance), with the sky light
/// itself.
pub(crate) fn menu_podium_light(r: &Renderer, vlm: &str, sky_suffix: &str) -> Result<(ViewMembers, SkyLight)> {
    let mut view = menu_vlm_members(r, vlm)?;
    let pack = r.assets().store().kind("sky")?;
    let mut found = pack.paths().filter(|p| p.ends_with(sky_suffix));
    let Some(path) = found.next().map(str::to_owned) else {
        return Err(bad(sky_suffix, "no such entry in the sky kind"));
    };
    let sky = load_sky(r.gpu(), r.assets(), &path)?;
    sky_members(r.gpu(), &sky, &mut view)?;
    Ok((view, sky))
}

/// `base` without the members a stage sets (its volumetric lightmap, sky light and atmospheric fog).
pub(crate) fn without_stage_members(base: &ViewMembers) -> ViewMembers {
    let mut m = base.clone();
    m.rows.retain(|r| !STAGE_MEMBERS.contains(&r.0));
    m.textures.retain(|t| !STAGE_MEMBERS.contains(&t.0));
    m
}

/// A menu level's spot light (`dir` toward the light, inverse squared) on
/// the lighting channels `channels`; its CastShadows goes to the light (lighting/shadow.rs draws the map).
// Unverified: the spot light class defaults and the candela conversion are 4.26 source.
pub(crate) fn menu_spot(l: &crate::menuscene::format::SpotLight, channels: u8) -> Light {
    Light {
        name: l.name.clone(),
        kind: if l.att[1] == -2.0 { LightKind::Point } else { LightKind::Spot },
        position: l.pos,
        direction: l.dir.map(|x| -x as f32),
        tangent: l.dir.map(|x| x as f32),
        color: l.color.map(|x| x as f32),
        inv_radius: l.att[0] as f32,
        falloff_exponent: 0.0,
        spot_angles: [l.att[1] as f32, l.att[2] as f32],
        source_radius: l.src[0] as f32,
        soft_source_radius: l.src[1] as f32,
        source_length: 0.0,
        specular_scale: l.src[2] as f32,
        channels,
        shadow_channel: -1,
        cast_shadows: l.cast_shadows,
        translucent: true,
    }
}

/// A menu level's directional light (`dir` toward the light, `src` the
/// source angles' sines) on the lighting channels `channels`.
// Unverified: the level's sky light is taken as static (baked into the volumetric lightmap, no sky irradiance or
// specular), a 4.26 source rule.
pub(crate) fn menu_directional(l: &crate::menuscene::format::DirLight, channels: u8) -> Light {
    Light {
        name: l.name.clone(),
        kind: LightKind::Directional,
        position: [0.0; 3],
        direction: l.dir.map(|x| -x as f32),
        tangent: l.dir.map(|x| x as f32),
        color: l.color.map(|x| x as f32),
        inv_radius: 0.0,
        falloff_exponent: 0.0,
        spot_angles: [0.0; 2],
        source_radius: l.src[0] as f32,
        soft_source_radius: l.src[1] as f32,
        source_length: 0.0,
        specular_scale: l.src[2] as f32,
        channels,
        shadow_channel: -1,
        cast_shadows: false,
        translucent: l.translucent,
    }
}

// ---- The sky ----

/// The sky kind's entry header: the parts the renderer reads.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkyHeader {
    version: u64,
    average_brightness: f32,
    /// The light's linear colour times its intensity.
    color: [f64; 3],
    /// The diffuse SH times the light's colour and intensity (three.js basis: x = UE X, y = UE Z, z = UE Y).
    sh: Vec<[f64; 3]>,
    cube: BodyRef,
    gf: BodyRef,
    gf_size: [u32; 2],
    captures: Vec<CaptureHeader>,
}

/// A reflection capture of the sky entry (mvs-import sky.rs Capture).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CaptureHeader {
    component: String,
    position: [f64; 3],
    influence_radius: f64,
    brightness: f64,
    capture_offset: [f64; 3],
    average_brightness: f64,
    cube: BodyRef,
}

/// The sky kind's entry version this reader reads.
const SKY_FORMAT_VERSION: u64 = 2;

/// View.SkyIrradianceEnvironmentMap's seven rows (E(n) / pi = r0..2 . (n, 1) + r3..5 . (n.yzzx * n.xyzz) + r6 (nx^2 -
/// ny^2) per channel, n in UE axes) of the sky kind's SH, with y^2 and x^2 - z^2 rewritten through x^2 + y^2 + z^2 = 1.
pub fn sky_irradiance(sh: &[[f64; 3]]) -> [[f32; 4]; 7] {
    let mut sb = [[0.0f64; 4]; 7];
    for ch in 0..3 {
        let c: Vec<f64> = sh.iter().map(|v| v[ch] / std::f64::consts::PI).collect();
        let (q6, q8) = (0.743125 * c[6], 0.429043 * c[8]);
        sb[ch] =
            [1.023328 * c[3], 1.023328 * c[2], 1.023328 * c[1], 0.886227 * c[0] - 0.247708 * c[6] + (q6 + q8) / 2.0];
        sb[3 + ch] = [0.858086 * c[7], 0.858086 * c[5], -q6 / 2.0 - 1.5 * q8, 0.858086 * c[4]];
        sb[6][ch] = (q8 - q6) / 2.0;
    }
    sb.map(|r| r.map(|x| x as f32))
}

/// SkyIrradianceEnvironmentMap's rows of an SH that holds the light's colour and intensity, with them taken out: the
/// build's map is the capture's SH alone, and the base pass and the sky's specular multiply by SkyLightColor
/// (View cb0[187], the colour times the intensity). A channel of colour 0 has an SH of 0 and stays 0.
pub fn sky_rows_unscaled(rows: [[f32; 4]; 7], color: [f32; 3]) -> [[f32; 4]; 7] {
    let mut out = rows;
    for (ch, &c) in color.iter().enumerate() {
        let k = if c == 0.0 { 0.0 } else { 1.0 / c };
        out[ch] = rows[ch].map(|x| x * k);
        out[3 + ch] = rows[3 + ch].map(|x| x * k);
        out[6][ch] = rows[6][ch] * k;
    }
    out
}

fn load_sky(gpu: &Gpu, assets: &crate::Assets, path: &str) -> Result<SkyLight> {
    let pack = assets.store().kind("sky")?;
    let bytes = pack.bytes(path)?;
    let j = JsonBin::split(b"MVSK", &bytes, path)?;
    let h: SkyHeader = j.parse(path)?;
    if h.version != SKY_FORMAT_VERSION {
        return Err(bad(path, format!("sky entry version {}, this reader reads {SKY_FORMAT_VERSION}", h.version)));
    }
    if h.sh.len() != 9 {
        return Err(bad(path, format!("{} SH coefficients", h.sh.len())));
    }
    let cube = upload_texture(gpu, j.section(h.cube, path)?, path)?;
    if cube.desc.kind != TexKind::Cube || cube.desc.format != DXGI_FORMAT_R16G16B16A16_FLOAT {
        return Err(bad(path, "the sky's cube is not a FloatRGBA cube"));
    }
    let [w, h_] = h.gf_size;
    let gf = j.section(h.gf, path)?;
    if gf.len() != (w * h_ * 4) as usize {
        return Err(bad(path, format!("a {w} x {h_} PreIntegratedGF table of {} bytes", gf.len())));
    }
    // PF_G16R16: R the scale, G the bias.
    let gf = gpu.texture(
        &TexDesc {
            kind: TexKind::D2,
            size: [w, h_, 1],
            mips: 1,
            format: DXGI_FORMAT_R16G16_UNORM,
            views: TexViews { srv: true, ..Default::default() },
        },
        &[Subresource { data: gf, row_pitch: w * 4, slice_pitch: w * h_ * 4 }],
    )?;
    // The build's View: SkyLightColor the light's colour times its intensity (FSkyLightSceneProxy), which scales the
    // sky's specular as well as the SH the sky kind holds with it folded in (FReflectionEnvironmentSkyLightingPS
    // multiplies the cube by cb0[187]).
    let color = h.color.map(|c| c as f32);
    let mut captures = Vec::with_capacity(h.captures.len());
    for c in &h.captures {
        let what = format!("{path}: {}", c.component);
        let cube = upload_texture(gpu, j.section(c.cube, &what)?, &what)?;
        if cube.desc.kind != TexKind::Cube || cube.desc.format != DXGI_FORMAT_R16G16B16A16_FLOAT {
            return Err(bad(&what, "a reflection capture's cube is not a FloatRGBA cube"));
        }
        captures.push(ReflectionCapture {
            position: c.position,
            influence_radius: c.influence_radius as f32,
            brightness: c.brightness as f32,
            capture_offset: c.capture_offset.map(|v| v as f32),
            average_brightness: c.average_brightness as f32,
            cube: Arc::new(cube),
        });
    }
    // Unverified: the composite takes the captures smallest InfluenceRadius first (4.26's capture sort and
    // the light grid's per-cell order, not read in the exe); it decides only where two spheres overlap.
    captures.sort_by(|a, b| a.influence_radius.total_cmp(&b.influence_radius));
    Ok(SkyLight {
        color,
        cube: Arc::new(cube),
        brightness: h.average_brightness,
        irradiance: sky_rows_unscaled(sky_irradiance(&h.sh), color),
        pre_integrated_gf: Arc::new(gf),
        captures,
    })
}

// ---- The atmospheric fog ----

/// The AtmosphericFog View members of the stage's `atmosphere` block (FAtmosphericFogSceneInfo's constructor, exe
/// 141916700, and the View set-up, exe 141cda280, as data/stage/render read them).
fn atmosphere_members(a: &mvs_data::stage::Atmosphere, view: &mut ViewMembers) -> Result<()> {
    if a.light_direction.len() != 3 || a.light_color.len() != 3 {
        return Err(bad("atmosphere", "a light direction or colour of other than three components"));
    }
    let f1 = |view: &mut ViewMembers, name: &'static str, v: f64| view.set_f(name, [v as f32, 0.0, 0.0, 0.0]);
    f1(view, "AtmosphericFogSunPower", a.sun_power);
    f1(view, "AtmosphericFogPower", a.power);
    f1(view, "AtmosphericFogDensityScale", a.density_scale);
    f1(view, "AtmosphericFogDensityOffset", a.density_offset);
    f1(view, "AtmosphericFogGroundOffset", a.ground_offset);
    f1(view, "AtmosphericFogDistanceScale", a.distance_scale);
    f1(view, "AtmosphericFogAltitudeScale", a.altitude_scale);
    f1(view, "AtmosphericFogHeightScaleRayleigh", a.height_scale_rayleigh);
    f1(view, "AtmosphericFogStartDistance", a.start_distance);
    f1(view, "AtmosphericFogDistanceOffset", a.distance_offset);
    let [x, y, z] = [0, 1, 2].map(|k| a.light_direction[k] as f32);
    view.set_f("AtmosphereLightDirection[0]", [x, y, z, 0.0]);
    let [r, g, b] = [0, 1, 2].map(|k| a.light_color[k] as f32);
    view.set_f("AtmosphereLightColor[0]", [r, g, b, 0.0]);
    let uint = |v: f64, what: &str| -> Result<u32> {
        if v < 0.0 || v.fract() != 0.0 || v > u32::MAX as f64 {
            return Err(bad("atmosphere", format!("{what} {v} is not a uint")));
        }
        Ok(v as u32)
    };
    // The permutation is built without sun disk and ground scattering alone.
    let mask = uint(a.render_mask, "RenderMask")?;
    if mask != 3 {
        return Err(bad("atmosphere", format!("RenderMask {mask}: only 3 (no sun disk, no ground scattering)")));
    }
    view.set_u("AtmosphericFogRenderMask", [mask, 0, 0, 0]);
    view.set_u("AtmosphericFogInscatterAltitudeSampleNum", [uint(a.altitude_samples, "altitude samples")?, 0, 0, 0]);
    Ok(())
}

/// The atmosphere kind's entry header: the inscatter table's part.
#[derive(Deserialize)]
struct AtmosphereHeader {
    version: u64,
    size: [u32; 3],
    table: BodyRef,
}

/// The atmosphere kind's entry version this reader reads.
const ATMOSPHERE_FORMAT_VERSION: u64 = 1;

/// The level's AtmosphericFogComponent's precomputed inscatter table (the atmosphere kind's entry of the stage's own
/// component, as its stagedraw entry names it): a 3D PF_FloatRGBA texture, x fastest.
fn load_inscatter(gpu: &Gpu, store: &mvs_store::Store, path: &str) -> Result<Texture> {
    let pack = store.kind("atmosphere")?;
    let bytes = pack.bytes(path)?;
    let j = JsonBin::split(b"MVAF", &bytes, path)?;
    let h: AtmosphereHeader = j.parse(path)?;
    if h.version != ATMOSPHERE_FORMAT_VERSION {
        return Err(bad(
            path,
            format!("atmosphere entry version {}, this reader reads {ATMOSPHERE_FORMAT_VERSION}", h.version),
        ));
    }
    let [w, hh, d] = h.size;
    let table = j.section(h.table, path)?;
    if table.len() != (w * hh * d * 8) as usize {
        return Err(bad(path, format!("a {w} x {hh} x {d} inscatter table of {} bytes", table.len())));
    }
    gpu.texture(
        &TexDesc {
            kind: TexKind::D3,
            size: [w, hh, d],
            mips: 1,
            format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            views: TexViews { srv: true, ..Default::default() },
        },
        &[Subresource { data: table, row_pitch: w * 8, slice_pitch: w * hh * 8 }],
    )
}

/// The sky light's View members: SkyLightColor and SkyIrradianceEnvironmentMap (a StructuredBuffer<float4> of seven).
fn sky_members(gpu: &Gpu, sky: &SkyLight, view: &mut ViewMembers) -> Result<()> {
    view.set_f("SkyLightColor", [sky.color[0], sky.color[1], sky.color[2], 0.0]);
    let bytes: Vec<u8> = sky.irradiance.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
    let b = gpu.buffer(
        &BufDesc { kind: BufKind::Structured { stride: 16 }, size: bytes.len() as u32, dynamic: false },
        Some(&bytes),
    )?;
    // The View's resources are bound by their SRV: the buffer rides in a Texture of its view.
    // Unverified: a StructuredBuffer in ViewMembers' textures (its resource the buffer, a placeholder TexDesc) until
    // the View's resources take a buffer.
    let res: ID3D11Resource =
        b.buf.cast().map_err(|e| Error::Device { what: "SkyIrradianceEnvironmentMap".into(), source: e })?;
    view.set_texture(
        "SkyIrradianceEnvironmentMap",
        Arc::new(Texture {
            res,
            srv: b.srv.clone(),
            rtv: None,
            dsv: None,
            uav: None,
            desc: TexDesc {
                kind: TexKind::D2,
                size: [7, 1, 1],
                mips: 1,
                format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                views: TexViews { srv: true, ..Default::default() },
            },
        }),
    );
    Ok(())
}

// ---- The volumetric lightmap ----

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[allow(non_snake_case)]
struct Dims {
    X: u32,
    Y: u32,
    Z: u32,
}

#[derive(Deserialize, Clone, Copy)]
#[allow(non_snake_case)]
struct Bgra {
    B: u8,
    G: u8,
    R: u8,
    A: u8,
}

#[derive(Deserialize)]
struct Volume {
    format: String,
    data: BodyRef,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct Bounds {
    Min: BTreeMap<String, serde_json::Value>,
    Max: BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct BrickData {
    AmbientVector: Volume,
    SHCoefficients: Vec<Volume>,
}

/// FPrecomputedVolumetricLightmapData as the lighting kind writes it.
#[derive(Deserialize)]
#[allow(non_snake_case)]
struct VlmData {
    Bounds: Bounds,
    IndirectionTextureDimensions: Dims,
    IndirectionTexture: Volume,
    BrickSize: u32,
    BrickDataDimensions: Dims,
    BrickData: BrickData,
    SubLevelBrickPositions: Option<Vec<Dims>>,
    IndirectionTextureOriginalValues: Option<Vec<Bgra>>,
}

#[derive(Deserialize)]
struct LightingHeader {
    version: u64,
    vlm: BTreeMap<String, Option<VlmData>>,
}

/// The lighting kind's entry version this reader reads.
const LIGHTING_FORMAT_VERSION: u64 = 1;

/// A lighting kind entry (one MapBuildDataRegistry) with its volumetric lightmap.
struct LightingEntry {
    path: String,
    vlm: VlmData,
    bytes: Vec<u8>,
}

impl LightingEntry {
    fn read(pack: &mvs_store::Pack, path: &str) -> Result<LightingEntry> {
        let bytes = pack.bytes(path)?;
        let h: LightingHeader = JsonBin::split(b"MVLT", &bytes, path)?.parse(path)?;
        if h.version != LIGHTING_FORMAT_VERSION {
            return Err(bad(
                path,
                format!("lighting entry version {}, this reader reads {LIGHTING_FORMAT_VERSION}", h.version),
            ));
        }
        // (the build's registries hold one volume at most)
        let vlm = h.vlm.into_values().flatten().next().ok_or_else(|| bad(path, "no volumetric lightmap"))?;
        Ok(LightingEntry { path: path.to_string(), vlm, bytes })
    }

    fn section(&self, v: &Volume, format: &str) -> Result<&[u8]> {
        if v.format != format {
            return Err(bad(&self.path, format!("a {} layer where {format} is read", v.format)));
        }
        Ok(JsonBin::split(b"MVLT", &self.bytes, &self.path)?.section(v.data, &self.path)?)
    }
}

/// The stage's volumetric lightmap: the level's and its sub-levels' bricks merged into one atlas.
struct VlmVolume {
    bounds_min: [f64; 3],
    bounds_max: [f64; 3],
    indirection_size: [u32; 3],
    brick_size: u32,
    brick_dims: [u32; 3],
    /// PF_R8G8B8A8_UINT cells, x fastest: the brick's atlas position in bricks (xyz), its size in cells (w).
    indirection: Vec<u8>,
    /// AmbientVector, PF_FloatR11G11B10 as the build stores it.
    ambient: Vec<u8>,
    /// The six SH coefficient layers (0R 1R 0G 1G 0B 1B), PF_R8G8B8A8.
    sh: Vec<Vec<u8>>,
}

/// Bytes a texel of every layer.
const TEXEL: usize = 4;

/// An atlas's bricks in their order (x fastest, then y, then z, in brick units), each brick's texels x fastest.
fn bricks(data: &[u8], dims: Dims, padded: usize, what: &str) -> Result<Vec<Vec<u8>>> {
    let (dx, dy, dz) = (dims.X as usize, dims.Y as usize, dims.Z as usize);
    if dx % padded != 0 || dy % padded != 0 || dz % padded != 0 || data.len() != dx * dy * dz * TEXEL {
        return Err(bad(
            what,
            format!("a {dx} x {dy} x {dz} brick atlas of {} bytes in bricks of {padded}", data.len()),
        ));
    }
    let (bx, by, bz) = (dx / padded, dy / padded, dz / padded);
    let row = padded * TEXEL;
    let mut out = Vec::with_capacity(bx * by * bz);
    for z in 0..bz {
        for y in 0..by {
            for x in 0..bx {
                let mut b = vec![0u8; padded * padded * row];
                for k in 0..padded {
                    for j in 0..padded {
                        let at = (((z * padded + k) * dims.Y as usize + y * padded + j) * dims.X as usize + x * padded)
                            * TEXEL;
                        let to = (k * padded + j) * row;
                        b[to..to + row].copy_from_slice(&data[at..at + row]);
                    }
                }
                out.push(b);
            }
        }
    }
    Ok(out)
}

/// Bricks laid out `per_row` along X, then rows along Y (one slab in Z): the atlas bytes and its texel dims.
fn layout(list: &[Vec<u8>], per_row: usize, padded: usize) -> (Vec<u8>, [u32; 3]) {
    let rows = list.len().div_ceil(per_row);
    let cols = list.len().min(per_row);
    let (dx, dy, dz) = (cols * padded, rows * padded, padded);
    let mut out = vec![0u8; dx * dy * dz * TEXEL];
    let row = padded * TEXEL;
    for (i, b) in list.iter().enumerate() {
        let (x, y) = (i % per_row, i / per_row);
        for k in 0..padded {
            for j in 0..padded {
                let from = (k * padded + j) * row;
                let to = ((k * dy + y * padded + j) * dx + x * padded) * TEXEL;
                out[to..to + row].copy_from_slice(&b[from..from + row]);
            }
        }
    }
    (out, [dx as u32, dy as u32, dz as u32])
}

fn bound(m: &BTreeMap<String, serde_json::Value>, k: &str, what: &str) -> Result<f64> {
    match m.get(k) {
        Some(serde_json::Value::Number(n)) => n.as_f64().ok_or_else(|| bad(what, format!("bounds {k}"))),
        Some(serde_json::Value::String(s)) => s.parse().map_err(|_| bad(what, format!("bounds {k} {s}"))),
        _ => Err(bad(what, format!("no bounds {k}"))),
    }
}

impl VlmVolume {
    /// The stage's volumetric lightmap from its registries (the stagedraw's `vlm`: the level's, then the sub-levels'
    /// with bricks of their own).
    ///
    /// A streamed sub-level's registry can hold more bricks with SubLevelBrickPositions, the indirection cell of
    /// each, and IndirectionTextureOriginalValues, what the persistent level has there. When the sub-level is added
    /// to the scene the engine appends its bricks to the atlas and points those cells at them with brick size 1
    /// (FPrecomputedVolumetricLightmap::AddToScene). The sub-levels are merged the same way: their bricks go after
    /// the level's, 256 to an atlas row. The original values are checked against the level's indirection texture.
    // Unverified: the engine's merge (AddToSceneData) is the 4.26 source's, its layout not read in the exe; the
    // sampled light does not depend on it.
    fn merge(entries: &[LightingEntry]) -> Result<VlmVolume> {
        const PER_ROW: usize = 256;
        let (first, subs) = entries.split_first().ok_or_else(|| bad("vlm", "no registry"))?;
        let vlm = &first.vlm;
        let what = first.path.as_str();
        let mut dims = vlm.BrickDataDimensions;
        let n0 = (dims.X * dims.Y * dims.Z) as usize;
        let mut ambient = first.section(&vlm.BrickData.AmbientVector, "PF_FloatR11G11B10")?.to_vec();
        let mut sh: Vec<Vec<u8>> = vlm
            .BrickData
            .SHCoefficients
            .iter()
            .map(|t| first.section(t, "PF_R8G8B8A8").map(<[u8]>::to_vec))
            .collect::<Result<_>>()?;
        if ambient.len() != TEXEL * n0 || sh.len() != 6 || sh.iter().any(|s| s.len() != TEXEL * n0) {
            return Err(bad(what, format!("the brick layers are not 6 SH layers of {n0} texels")));
        }
        let idim = vlm.IndirectionTextureDimensions;
        let mut ind = first.section(&vlm.IndirectionTexture, "PF_R8G8B8A8_UINT")?.to_vec();
        if ind.len() != TEXEL * (idim.X * idim.Y * idim.Z) as usize {
            return Err(bad(what, format!("the indirection texture is not {} x {} x {}", idim.X, idim.Y, idim.Z)));
        }
        let padded = vlm.BrickSize as usize + 1;
        let bounds = |v: &VlmData, what: &str| -> Result<[f64; 6]> {
            let (lo, hi) = (&v.Bounds.Min, &v.Bounds.Max);
            Ok([
                bound(lo, "X", what)?,
                bound(lo, "Y", what)?,
                bound(lo, "Z", what)?,
                bound(hi, "X", what)?,
                bound(hi, "Y", what)?,
                bound(hi, "Z", what)?,
            ])
        };
        let b0 = bounds(vlm, what)?;
        if !subs.is_empty() {
            let mut amb_b = bricks(&ambient, dims, padded, what)?;
            let mut sh_b: Vec<Vec<Vec<u8>>> =
                sh.iter().map(|s| bricks(s, dims, padded, what)).collect::<Result<_>>()?;
            let n0 = amb_b.len();
            // (byte offset of the indirection cell, the brick's index in the merged list)
            let mut sub_cells: Vec<(usize, usize)> = Vec::new();
            for se in subs {
                let sub = &se.vlm;
                let sdims = sub.BrickDataDimensions;
                let cells = sub.SubLevelBrickPositions.as_deref().unwrap_or(&[]);
                let was = sub.IndirectionTextureOriginalValues.as_deref().unwrap_or(&[]);
                if sub.BrickSize != vlm.BrickSize || bounds(sub, &se.path)? != b0 {
                    return Err(bad(&se.path, format!("another brick size or bounds than {what}")));
                }
                let p = padded as u32;
                if sdims.Y != p || sdims.Z != p || sdims.X != p * cells.len() as u32 {
                    return Err(bad(&se.path, format!("its atlas is not its {} bricks in a row", cells.len())));
                }
                let at0 = amb_b.len();
                for (i, c) in cells.iter().enumerate() {
                    if c.X >= idim.X || c.Y >= idim.Y || c.Z >= idim.Z {
                        return Err(bad(&se.path, format!("cell {} {} {} outside the indirection", c.X, c.Y, c.Z)));
                    }
                    let at = TEXEL * ((c.Z * idim.Y + c.Y) * idim.X + c.X) as usize;
                    match was.get(i) {
                        Some(w) if ind[at..at + 4] == [w.R, w.G, w.B, w.A] => {}
                        _ => {
                            return Err(bad(
                                &se.path,
                                format!("cell {} {} {} is not its original value in {what}", c.X, c.Y, c.Z),
                            ));
                        }
                    }
                    sub_cells.push((at, at0 + i));
                }
                amb_b.extend(bricks(
                    se.section(&sub.BrickData.AmbientVector, "PF_FloatR11G11B10")?,
                    sdims,
                    padded,
                    &se.path,
                )?);
                for (k, layer) in sh_b.iter_mut().enumerate() {
                    let t = sub.BrickData.SHCoefficients.get(k).ok_or_else(|| bad(&se.path, "no SH layer"))?;
                    layer.extend(bricks(se.section(t, "PF_R8G8B8A8")?, sdims, padded, &se.path)?);
                }
            }
            if amb_b.len() > PER_ROW * 256 {
                return Err(bad(what, format!("{} bricks", amb_b.len())));
            }
            let (bx, by) = ((dims.X as usize / padded), (dims.Y as usize / padded));
            let set = |ind: &mut [u8], lin: usize, at: usize| {
                ind[at] = (lin % PER_ROW) as u8;
                ind[at + 1] = (lin / PER_ROW) as u8;
                ind[at + 2] = 0;
            };
            let mut is_sub = vec![false; ind.len() / TEXEL];
            for &(at, _) in &sub_cells {
                is_sub[at / TEXEL] = true;
            }
            for at in (0..ind.len()).step_by(TEXEL) {
                if !is_sub[at / TEXEL] {
                    let (x, y, z) = (ind[at] as usize, ind[at + 1] as usize, ind[at + 2] as usize);
                    let lin = x + bx * (y + by * z);
                    if x >= bx || y >= by || lin >= n0 {
                        return Err(bad(what, format!("indirection cell {} names brick {x} {y} {z}", at / TEXEL)));
                    }
                    set(&mut ind, lin, at);
                }
            }
            for &(at, lin) in &sub_cells {
                set(&mut ind, lin, at);
                ind[at + 3] = 1;
            }
            let (a, d) = layout(&amb_b, PER_ROW, padded);
            ambient = a;
            dims = Dims { X: d[0], Y: d[1], Z: d[2] };
            sh = sh_b.iter().map(|s| layout(s, PER_ROW, padded).0).collect();
        }
        Ok(VlmVolume {
            bounds_min: [b0[0], b0[1], b0[2]],
            bounds_max: [b0[3], b0[4], b0[5]],
            indirection_size: [idim.X, idim.Y, idim.Z],
            brick_size: vlm.BrickSize,
            brick_dims: [dims.X, dims.Y, dims.Z],
            indirection: ind,
            ambient,
            sh,
        })
    }

    /// The volumes as the View's textures and the View members the cooked shaders read them by
    /// WorldToUVScale / Add, the indirection size and the brick size,
    /// the brick texel size.
    fn upload(&self, gpu: &Gpu, view: &mut ViewMembers) -> Result<()> {
        let tex = |data: &[u8], size: [u32; 3], format| {
            gpu.texture(
                &TexDesc {
                    kind: TexKind::D3,
                    size,
                    mips: 1,
                    format,
                    views: TexViews { srv: true, ..Default::default() },
                },
                &[Subresource { data, row_pitch: size[0] * 4, slice_pitch: size[0] * size[1] * 4 }],
            )
            .map(Arc::new)
        };
        view.set_texture(
            "VolumetricLightmapIndirectionTexture",
            tex(&self.indirection, self.indirection_size, DXGI_FORMAT_R8G8B8A8_UINT)?,
        );
        view.set_texture(
            "VolumetricLightmapBrickAmbientVector",
            // PF_FloatR11G11B10 uploaded as the build stores it (not decoded to half floats).
            tex(&self.ambient, self.brick_dims, DXGI_FORMAT_R11G11B10_FLOAT)?,
        );
        const SH: [&str; 6] = [
            "VolumetricLightmapBrickSHCoefficients0",
            "VolumetricLightmapBrickSHCoefficients1",
            "VolumetricLightmapBrickSHCoefficients2",
            "VolumetricLightmapBrickSHCoefficients3",
            "VolumetricLightmapBrickSHCoefficients4",
            "VolumetricLightmapBrickSHCoefficients5",
        ];
        for (name, s) in SH.iter().zip(&self.sh) {
            view.set_texture(name, tex(s, self.brick_dims, DXGI_FORMAT_R8G8B8A8_UNORM)?);
        }
        let ext: [f64; 3] = [0, 1, 2].map(|i| self.bounds_max[i] - self.bounds_min[i]);
        view.set_f(
            "VolumetricLightmapWorldToUVScale",
            [(1.0 / ext[0]) as f32, (1.0 / ext[1]) as f32, (1.0 / ext[2]) as f32, 0.0],
        );
        view.set_f(
            "VolumetricLightmapWorldToUVAdd",
            [
                (-self.bounds_min[0] / ext[0]) as f32,
                (-self.bounds_min[1] / ext[1]) as f32,
                (-self.bounds_min[2] / ext[2]) as f32,
                0.0,
            ],
        );
        let [ix, iy, iz] = self.indirection_size.map(|x| x as f32);
        view.set_f("VolumetricLightmapIndirectionTextureSize", [ix, iy, iz, 0.0]);
        view.set_f("VolumetricLightmapBrickSize", [self.brick_size as f32, 0.0, 0.0, 0.0]);
        let [dx, dy, dz] = self.brick_dims.map(|x| 1.0 / x as f32);
        view.set_f("VolumetricLightmapBrickTexelSize", [dx, dy, dz, 0.0]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(stage_key("Map_M010"), "m010");
        assert_eq!(stage_key("TrainingRoomSmall"), "trainingroomsmall");
    }

    #[test]
    fn sky_rows_of_a_constant_sky() {
        // A constant sky of radiance L: SH c0 = L sqrt(pi) x 2 (three's basis has Y00 = 0.282095), E / pi = L.
        let l = 0.5;
        let mut sh = vec![[0.0; 3]; 9];
        sh[0] = [l / 0.282095; 3];
        let rows = sky_irradiance(&sh);
        // 0.886227 x c0 / pi = L x 0.886227 / (0.282095 pi) = L.
        assert!((rows[0][3] - l as f32).abs() < 1e-5, "{:?}", rows[0]);
        assert_eq!(rows[6], [0.0; 4]);
    }

    #[test]
    fn sky_rows_unscaled_by_channel() {
        let rows = [[1.5; 4]; 7];
        let out = sky_rows_unscaled(rows, [0.75, 0.5, 0.0]);
        assert_eq!(out[0], [2.0; 4]);
        assert_eq!(out[4], [3.0; 4]);
        assert_eq!(out[2], [0.0; 4]);
        assert_eq!(out[6], [2.0, 3.0, 0.0, 1.5]);
    }

    #[test]
    fn bricks_round_trip_through_layout() {
        // Two bricks of padded size 2 side by side along X: 4 x 2 x 2 texels.
        let dims = Dims { X: 4, Y: 2, Z: 2 };
        let data: Vec<u8> = (0..(4 * 2 * 2 * TEXEL) as u32).map(|x| x as u8).collect();
        let b = bricks(&data, dims, 2, "test").unwrap();
        assert_eq!(b.len(), 2);
        let (back, d) = layout(&b, 256, 2);
        assert_eq!(d, [4, 2, 2]);
        assert_eq!(back, data);
    }

    #[test]
    fn atmosphere_rows() {
        let a = mvs_data::stage::Atmosphere {
            sun_power: 2.0,
            power: 1.0,
            density_scale: 1.0,
            density_offset: 0.0,
            ground_offset: -96368.0,
            distance_scale: 1.0,
            altitude_scale: 1.0,
            height_scale_rayleigh: 8.0,
            start_distance: 15000.0,
            distance_offset: 0.0,
            light_direction: vec![0.0, 0.0, 1.0],
            light_color: vec![1.0, 0.5, 0.25],
            altitude_samples: 32.0,
            render_mask: 3.0,
        };
        let mut v = ViewMembers::default();
        atmosphere_members(&a, &mut v).unwrap();
        let row = |n: &str| v.rows.iter().find(|r| r.0 == n).unwrap().1;
        assert_eq!(row("AtmosphericFogInscatterAltitudeSampleNum"), [32, 0, 0, 0]);
        assert_eq!(row("AtmosphericFogRenderMask"), [3, 0, 0, 0]);
        assert_eq!(row("AtmosphericFogGroundOffset")[0], (-96368.0f32).to_bits());
        assert_eq!(row("AtmosphereLightColor[0]"), [1.0f32, 0.5, 0.25, 0.0].map(f32::to_bits));
        assert!(v.rows.iter().all(|r| STAGE_MEMBERS.contains(&r.0)));
        let mut v = ViewMembers::default();
        assert!(atmosphere_members(&mvs_data::stage::Atmosphere { render_mask: 1.0, ..a.clone() }, &mut v).is_err());
        assert!(atmosphere_members(&mvs_data::stage::Atmosphere { altitude_samples: 2.5, ..a }, &mut v).is_err());
    }

    #[test]
    fn directional_light() {
        let l = StageLight {
            name: "Key".into(),
            rot: vec![-90.0, 0.0, 0.0],
            intensity: 2.0,
            color: vec![1.0, 1.0, 1.0],
            affects_world: true,
            source_angle: 0.5357,
            soft_source_angle: 0.0,
            specular_scale: 1.0,
            mobility: "Stationary".into(),
            channels: vec![false, true, false],
            translucent: true,
        };
        let d = directional(&l, 0).unwrap();
        assert_eq!(d.shadow_channel, 0);
        assert_eq!(
            directional(&StageLight { channels: vec![true, false, false], ..l.clone() }, 0).unwrap().shadow_channel,
            -1
        );
        assert!((d.direction[2] + 1.0).abs() < 1e-6);
        assert_eq!(d.color, [2.0; 3]);
        assert_eq!(d.channels, 0b010);
        assert!(directional(&StageLight { mobility: "Static".into(), ..l }, -1).is_none());
    }

    /// The developer's store's The Court: the directional lights that affect the world and are not Static, and every
    /// radial light.
    #[test]
    fn court_lights_of_the_store() {
        let dir = mvs_store::dev_store_dir();
        let Ok(store) = mvs_store::Store::open(&dir) else {
            eprintln!("no store at {}: nothing to check", dir.display());
            return;
        };
        let Ok(data) = stage_data(&store, "m010") else {
            eprintln!("the store at {} has no Map_M010 stage entry", dir.display());
            return;
        };
        let (d, r) = lights_of(&data, -1).unwrap();
        assert!(d.iter().chain(&r).all(|l| (-1..=3).contains(&l.shadow_channel)));
        let names: Vec<&str> = d.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "DirectionalLight_1",
                "DirectionalLight_BackRight2",
                "DirectionalLight_Left2",
                "DirectionalLight_Right2",
                "DirectionalLight_Top2"
            ]
        );
        assert_eq!(d[0].channels, 0b010);
        assert_eq!(d[1].color, [30.0; 3]);
        assert_eq!(r.len(), 17);
        let sign = r.iter().find(|l| l.name == "PointLight_ACMEsign_1").unwrap();
        assert_eq!(sign.kind, LightKind::Point);
        assert_eq!(sign.direction, [1.0, 0.0, 0.0]);
        assert_eq!(sign.color[0], 25.0);
        assert_eq!(sign.inv_radius, (1.0f64 / 1500.0) as f32);
        assert_eq!(sign.falloff_exponent, 8.0);
    }
}
