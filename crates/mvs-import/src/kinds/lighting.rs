//! The `lighting` kind: one entry per MapBuildDataRegistry (a level's `_BuiltData`), its native data
//! (`mvs_uasset::natives::lighting`) as an MVLT container: the maps' records in the header, every
//! byte array in a section of its own in the build's bytes (the volumetric lightmap's indirection texture and brick
//! layers, each light's FFloat16 depth samples, each reflection capture's HDR cube, a light volume's samples), no
//! float decode. References are object paths, the inner lightmap and shadow map textures under the object kind's inner
//! naming (`<package>.<registry>:<inner>`). What the game makes of it (the sub-level brick merge, the TLV shadow
//! atlas, a light's channel) is the game's, not this.
//!
//! The records hold the build's fields by the names CUE4Parse 1.2.2 writes, in its JSON shapes ({X, Y, Z, W},
//! {M00 .. M33}, {B, G, R, A}, guids "AAAAAAAA-BBBBBBBB-CCCCCCCC-DDDDDDDD", float32 values as .NET prints them),
//! keyed by guid as CUE4Parse writes a map key (32 hex digits, no dashes), with three changes: a reference is the
//! object path (null for none), a byte array is a section, FColor drops CUE4Parse's derived `Hex`.
//!
//! Covers the build's 89 registries. Three registries (Lobby and the two overviewFresnel material scenes) also hold a
//! light volume (FPrecomputedLightVolumeData of 9 SH coefficients: Lobby 26 samples, the two overviewFresnel scenes
//! 236 and 237); no registry holds sky atmosphere data. Nothing is read from the registries' .ubulk: it holds only
//! their inner textures' streamed mips (the texture kind's).

use indexmap::IndexMap;
use mvs_uasset::PackageCache;
use mvs_uasset::natives::lighting::{
    DataLayer, MapBuildData, MeshMapBuildData, Ref, VolumeLightingSamples, read_map_build_data,
};
use mvs_uasset::typed::f32_dotnet;
use mvs_uasset::value::{Props, Value};
use serde::Serialize;
use serde::ser::{SerializeMap, SerializeSeq, Serializer};

use crate::jsonbin::{BodyRef, json_bin};
use crate::kind::{Cx, Item, Kind, KindError};

/// The kind's version.
pub const LIGHTING_KIND_VERSION: u64 = 1;
/// The container's magic.
pub const LIGHTING_MAGIC: &[u8; 4] = b"MVLT";
/// The header's `version`.
const LIGHTING_FORMAT_VERSION: u32 = 1;
const REGISTRY: &str = "/Script/Engine.MapBuildDataRegistry";

/// A reader's value as the object kind's JSON shows it: a float as its shortest decimal, a non-finite one as its name.
struct Val<'a>(&'a Value);

impl Serialize for Val<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Null | Value::Ref(None) => s.serialize_none(),
            Value::Bool(b) => s.serialize_bool(*b),
            Value::Int(i) => s.serialize_i64(*i),
            Value::UInt(u) => s.serialize_u64(*u),
            Value::Float(f) if f.is_finite() => s.serialize_f64(f32_dotnet(*f)),
            Value::Float(f) => s.serialize_str(non_finite(f64::from(*f))),
            Value::Double(d) if d.is_finite() => s.serialize_f64(*d),
            Value::Double(d) => s.serialize_str(non_finite(*d)),
            Value::Str(t) => s.serialize_str(t),
            Value::Array(a) => {
                let mut q = s.serialize_seq(Some(a.len()))?;
                for x in a {
                    q.serialize_element(&Val(x))?;
                }
                q.end()
            }
            Value::Struct(p) => Members(p).serialize(s),
            other => Err(serde::ser::Error::custom(format!("lighting: a {other:?} in a record"))),
        }
    }
}

fn non_finite(d: f64) -> &'static str {
    if d.is_nan() {
        "NaN"
    } else if d > 0.0 {
        "Infinity"
    } else {
        "-Infinity"
    }
}

/// A struct's members as a JSON object.
struct Members<'a>(&'a Props);

impl Serialize for Members<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in self.0 {
            m.serialize_entry(k, &Val(v))?;
        }
        m.end()
    }
}

#[derive(Serialize)]
struct Volume {
    format: String,
    size: [i64; 3],
    data: BodyRef,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct LightMap<'a> {
    textures: Vec<Option<String>>,
    virtual_textures: Vec<Option<String>>,
    scale_vectors: Vec<Members<'a>>,
    add_vectors: Vec<Members<'a>>,
    coordinate_scale: Members<'a>,
    coordinate_bias: Members<'a>,
    inv_uniform_penumbra_size: Option<Members<'a>>,
    #[serde(rename = "bShadowChannelValid")]
    b_shadow_channel_valid: &'a Option<Vec<bool>>,
    light_guids: &'a [String],
    // written only when set, as CUE4Parse's FLightMap2DConverter does
    #[serde(skip_serializing_if = "Option::is_none")]
    sky_occlusion_texture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "AOMaterialMaskTexture")]
    ao_material_mask_texture: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct ShadowMap<'a> {
    texture: Option<String>,
    coordinate_scale: Members<'a>,
    coordinate_bias: Members<'a>,
    #[serde(rename = "bChannelValid")]
    b_channel_valid: &'a [bool],
    inv_uniform_penumbra_size: Members<'a>,
    light_guids: &'a [String],
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Instance<'a> {
    #[serde(rename = "LightmapUVBias")]
    lightmap_uv_bias: Members<'a>,
    #[serde(rename = "ShadowmapUVBias")]
    shadowmap_uv_bias: Members<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Mesh<'a> {
    irrelevant_lights: &'a [String],
    per_instance_lightmap_data: Vec<Instance<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    light_map: Option<LightMap<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shadow_map: Option<ShadowMap<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct DepthMap<'a> {
    world_to_light: Members<'a>,
    shadow_map_size_x: i32,
    shadow_map_size_y: i32,
    depth_samples: BodyRef,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Light<'a> {
    shadow_map_channel: i32,
    depth_map: DepthMap<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Bricks {
    ambient_vector: Volume,
    #[serde(rename = "SHCoefficients")]
    sh_coefficients: Vec<Volume>,
    sky_bent_normal: Volume,
    directional_light_shadowing: Volume,
    #[serde(skip_serializing_if = "Option::is_none", rename = "LQLightColor")]
    lq_light_color: Option<Volume>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "LQLightDirection")]
    lq_light_direction: Option<Volume>,
}

#[derive(Serialize)]
#[serde(rename_all = "UPPERCASE")]
struct Bgra {
    b: i64,
    g: i64,
    r: i64,
    a: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Vlm<'a> {
    bounds: Members<'a>,
    indirection_texture_dimensions: Members<'a>,
    indirection_texture: Volume,
    brick_size: i32,
    brick_data_dimensions: Members<'a>,
    brick_data: Bricks,
    #[serde(skip_serializing_if = "Option::is_none")]
    sub_level_brick_positions: Option<Vec<Members<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    indirection_texture_original_values: Option<Vec<Bgra>>,
}

#[derive(Serialize)]
struct Samples {
    count: usize,
    stride: usize,
    data: BodyRef,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct LightVolume<'a> {
    bounds: Members<'a>,
    sample_spacing: Val<'a>,
    #[serde(rename = "NumSHSamples")]
    num_sh_samples: i32,
    high_quality_samples: Samples,
    low_quality_samples: Samples,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct ReflectionCapture<'a> {
    cubemap_size: i32,
    average_brightness: Val<'a>,
    brightness: Val<'a>,
    #[serde(rename = "FullHDRCapturedData")]
    full_hdr_captured_data: BodyRef,
    encoded_capture_data: Option<String>,
}

/// The MVLT header.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Header<'a> {
    version: u32,
    path: &'a str,
    meshes: IndexMap<&'a str, Mesh<'a>>,
    lights: IndexMap<&'a str, Light<'a>>,
    vlm: IndexMap<&'a str, Option<Vlm<'a>>>,
    light_volumes: IndexMap<&'a str, Option<LightVolume<'a>>>,
    reflection_captures: IndexMap<&'a str, ReflectionCapture<'a>>,
}

/// The body's sections as they are named: each at the next 16-byte boundary, its ref given at once.
#[derive(Default)]
struct Sections<'a> {
    bytes: Vec<&'a [u8]>,
    refs: Vec<BodyRef>,
    end: usize,
}

impl<'a> Sections<'a> {
    fn add(&mut self, b: &'a [u8]) -> BodyRef {
        let r = BodyRef { offset: self.end, length: b.len() };
        self.end = (self.end + b.len() + 15) & !15;
        self.bytes.push(b);
        self.refs.push(r);
        r
    }
}

fn fail<T>(msg: String) -> Result<T, KindError> {
    Err(KindError::Failed(msg))
}

fn path_of(cache: &PackageCache, r: &Ref) -> Option<String> {
    r.as_ref().map(|r| r.object_path(cache))
}

fn int3(p: &Props, what: &str) -> Result<[i64; 3], KindError> {
    let at = |k: &str| p.get(k).and_then(Value::as_i64);
    match (at("X"), at("Y"), at("Z")) {
        (Some(x), Some(y), Some(z)) => Ok([x, y, z]),
        _ => fail(format!("lighting: {what} is not an IntVector")),
    }
}

fn mesh<'a>(cache: &PackageCache, m: &'a MeshMapBuildData) -> Mesh<'a> {
    let light_map = m.light_map.as_ref().map(|l| LightMap {
        textures: l.textures.iter().map(|t| path_of(cache, t)).collect(),
        virtual_textures: l.virtual_textures.iter().map(|t| path_of(cache, t)).collect(),
        scale_vectors: l.scale_vectors.iter().map(Members).collect(),
        add_vectors: l.add_vectors.iter().map(Members).collect(),
        coordinate_scale: Members(&l.coordinate_scale),
        coordinate_bias: Members(&l.coordinate_bias),
        inv_uniform_penumbra_size: l.inv_uniform_penumbra_size.as_ref().map(Members),
        b_shadow_channel_valid: &l.b_shadow_channel_valid,
        light_guids: &l.light_guids,
        sky_occlusion_texture: l.sky_occlusion_texture.as_ref().and_then(|r| path_of(cache, r)),
        ao_material_mask_texture: l.ao_material_mask_texture.as_ref().and_then(|r| path_of(cache, r)),
    });
    let shadow_map = m.shadow_map.as_ref().map(|s| ShadowMap {
        texture: path_of(cache, &s.texture),
        coordinate_scale: Members(&s.coordinate_scale),
        coordinate_bias: Members(&s.coordinate_bias),
        b_channel_valid: &s.b_channel_valid,
        inv_uniform_penumbra_size: Members(&s.inv_uniform_penumbra_size),
        light_guids: &s.light_guids,
    });
    Mesh {
        irrelevant_lights: &m.irrelevant_lights,
        per_instance_lightmap_data: m
            .per_instance_lightmap_data
            .iter()
            .map(|i| Instance {
                lightmap_uv_bias: Members(&i.lightmap_uv_bias),
                shadowmap_uv_bias: Members(&i.shadowmap_uv_bias),
            })
            .collect(),
        light_map,
        shadow_map,
    }
}

/// A volume layer's record, its bytes a section.
fn volume<'a>(sections: &mut Sections<'a>, l: &DataLayer<'a>, size: [i64; 3]) -> Volume {
    Volume { format: l.pixel_format_string.clone(), size, data: sections.add(l.data) }
}

/// A light volume's samples record, its bytes a section.
fn samples<'a>(sections: &mut Sections<'a>, x: &VolumeLightingSamples<'a>) -> Samples {
    Samples { count: x.count, stride: x.stride, data: sections.add(x.data) }
}

/// The entry of one registry export.
fn build_entry(cache: &PackageCache, path: &str, data: &MapBuildData<'_>) -> Result<Vec<u8>, KindError> {
    let mut sections = Sections::default();

    let meshes: IndexMap<&str, Mesh<'_>> =
        data.mesh_build_data.iter().map(|(guid, m)| (guid.as_str(), mesh(cache, m))).collect();

    let mut lights = IndexMap::new();
    for (guid, l) in &data.light_build_data {
        let d = &l.depth_map;
        let want = i64::from(d.shadow_map_size_x) * i64::from(d.shadow_map_size_y) * 2;
        if i64::try_from(d.depth_samples.len()).ok() != Some(want) {
            return fail(format!(
                "lighting: {path}: light {guid} has {} depth samples for {} x {}",
                js_half(d.depth_samples.len()),
                d.shadow_map_size_x,
                d.shadow_map_size_y
            ));
        }
        lights.insert(
            guid.as_str(),
            Light {
                shadow_map_channel: l.shadow_map_channel,
                depth_map: DepthMap {
                    world_to_light: Members(&d.world_to_light),
                    shadow_map_size_x: d.shadow_map_size_x,
                    shadow_map_size_y: d.shadow_map_size_y,
                    depth_samples: sections.add(d.depth_samples),
                },
            },
        );
    }

    let mut vlm = IndexMap::new();
    for (guid, v) in &data.level_precomputed_volumetric_lightmap_build_data {
        let Some(v) = v else {
            vlm.insert(guid.as_str(), None);
            continue;
        };
        let ind = int3(&v.indirection_texture_dimensions, "IndirectionTextureDimensions")?;
        let brick = int3(&v.brick_data_dimensions, "BrickDataDimensions")?;
        let b = &v.brick_data;
        let indirection_texture = volume(&mut sections, &v.indirection_texture, ind);
        let mut bricks = Bricks {
            ambient_vector: volume(&mut sections, &b.ambient_vector, brick),
            sh_coefficients: b.sh_coefficients.iter().map(|l| volume(&mut sections, l, brick)).collect(),
            sky_bent_normal: volume(&mut sections, &b.sky_bent_normal, brick),
            directional_light_shadowing: volume(&mut sections, &b.directional_light_shadowing, brick),
            lq_light_color: None,
            lq_light_direction: None,
        };
        bricks.lq_light_color = b.lq_light_color.as_ref().map(|l| volume(&mut sections, l, brick));
        bricks.lq_light_direction = b.lq_light_direction.as_ref().map(|l| volume(&mut sections, l, brick));
        let original = match &v.indirection_texture_original_values {
            None => None,
            Some(list) => {
                let mut out = Vec::with_capacity(list.len());
                for c in list {
                    let at = |k: &str| {
                        c.get(k).and_then(Value::as_i64).ok_or_else(|| {
                            KindError::Failed(format!(
                                "lighting: {path}: an IndirectionTextureOriginalValues color has no {k}"
                            ))
                        })
                    };
                    out.push(Bgra { b: at("B")?, g: at("G")?, r: at("R")?, a: at("A")? });
                }
                Some(out)
            }
        };
        vlm.insert(
            guid.as_str(),
            Some(Vlm {
                bounds: Members(&v.bounds),
                indirection_texture_dimensions: Members(&v.indirection_texture_dimensions),
                indirection_texture,
                brick_size: v.brick_size,
                brick_data_dimensions: Members(&v.brick_data_dimensions),
                brick_data: bricks,
                sub_level_brick_positions: v
                    .sub_level_brick_positions
                    .as_ref()
                    .map(|l| l.iter().map(Members).collect()),
                indirection_texture_original_values: original,
            }),
        );
    }

    let mut light_volumes = IndexMap::new();
    for (guid, v) in &data.level_precomputed_light_volume_build_data {
        let Some(v) = v else {
            light_volumes.insert(guid.as_str(), None);
            continue;
        };
        light_volumes.insert(
            guid.as_str(),
            Some(LightVolume {
                bounds: Members(&v.bounds),
                sample_spacing: Val(&v.sample_spacing),
                num_sh_samples: v.num_sh_samples,
                high_quality_samples: samples(&mut sections, &v.high_quality_samples),
                low_quality_samples: samples(&mut sections, &v.low_quality_samples),
            }),
        );
    }

    let mut reflection_captures = IndexMap::new();
    for (guid, r) in &data.reflection_capture_build_data {
        reflection_captures.insert(
            guid.as_str(),
            ReflectionCapture {
                cubemap_size: r.cubemap_size,
                average_brightness: Val(&r.average_brightness),
                brightness: Val(&r.brightness),
                full_hdr_captured_data: sections.add(r.full_hdr_captured_data),
                encoded_capture_data: path_of(cache, &r.encoded_capture_data),
            },
        );
    }

    let header =
        Header { version: LIGHTING_FORMAT_VERSION, path, meshes, lights, vlm, light_volumes, reflection_captures };
    Ok(json_bin(LIGHTING_MAGIC, &header, &sections.bytes, &sections.refs)?)
}

/// `length / 2` as JavaScript prints it (a half is `.5`).
fn js_half(len: usize) -> String {
    if len.is_multiple_of(2) { (len / 2).to_string() } else { format!("{}.5", len / 2) }
}

/// The header of registry `path`'s entry as the kind builds it, parsed back (numbers as JSON.parse reads them): what a
/// game kind reads of a registry in-process (the stagelight's meshes' build data) instead of from the store.
pub fn entry_header(cx: &Cx<'_>, path: &str) -> Result<serde_json::Value, KindError> {
    let bytes = Lighting.build(cx, &Item::new(path))?;
    let what = format!("lighting {path}");
    let e = mvs_material::jsonbin::JsonBin::split(LIGHTING_MAGIC, &bytes, &what)
        .map_err(|e| KindError::Failed(e.to_string()))?;
    e.parse(&what).map_err(|e| KindError::Failed(e.to_string()))
}

/// The `lighting` kind.
pub struct Lighting;

impl Kind for Lighting {
    fn id(&self) -> &'static str {
        "lighting"
    }

    fn version(&self) -> u64 {
        LIGHTING_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/octet-stream"
    }

    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(cx.catalog.paths_of(&[REGISTRY]).into_iter().map(Item::new).collect())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let path = &item.path;
        let (pkg, i) = cx.read(path)?;
        let name = cx.class_of(&pkg, i)?;
        if name != REGISTRY {
            return Err(KindError::NotFound(format!("lighting: {path} is a {name}, not a {REGISTRY}")));
        }
        // the whole entry is made inside the read: the sections are views into the package's export data
        let data = read_map_build_data(cx.cache, &pkg, i)?;
        build_entry(cx.cache, path, &data)
    }
}
