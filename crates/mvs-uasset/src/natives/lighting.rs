//! UMapBuildDataRegistry's native tail for the lighting kind: per mesh its light and shadow maps, the precomputed light
//! volumes, the volumetric lightmaps, the lights' depth maps and the reflection captures. Never entered in NATIVES:
//! the object kind keeps the class `partial`. It follows CUE4Parse 1.2.2
//! UMapBuildDataRegistry.Deserialize and its structs, the UE 4.26 cooked branch only. Its native structs are read
//! with `crate::structs::read_native_struct`, a float kept as the `Value::Float` the object kind writes. The byte
//! views borrow the package's export data.
//!
//! Everything is inline in the .uexp: no field here sits in the registry's .ubulk (that holds its inner lightmap and
//! shadow map textures' streamed mips, the texture kind's). A reference is the resolved object, `None` for none; a
//! byte array (a volumetric lightmap layer's data, the depth samples, the captured HDR cube, a light volume's
//! fixed-size FVolumeLightingSample array) is the build's bytes as stored (FFloat16 depth samples little-endian, no
//! decode). The reads use UE 4.26's custom versions (`crate::versions`; FReflectionCaptureObjectVersion is not in that
//! table: CUE4Parse's default for 4.19 and later, MoveReflectionCaptureDataToMapBuildData).

use std::sync::Arc;

use indexmap::IndexMap;

use crate::archive::{Guid, ReadError, Result};
use crate::objects::{ExportArchive, base_deserialize, export_archive, new_object};
use crate::package::{Package, PackageCache, Resolved};
use crate::props::ReadType;
use crate::structs::read_native_struct;
use crate::value::{Props, Value};
use crate::versions;

/// A reference as the reader gives it: the resolved object, or `None` for none.
pub type Ref = Option<Resolved>;

#[derive(Clone, Debug, Default)]
pub struct LightMap2D {
    pub textures: [Ref; 2],
    pub sky_occlusion_texture: Option<Ref>,
    pub ao_material_mask_texture: Option<Ref>,
    pub virtual_textures: [Ref; 2],
    pub scale_vectors: Vec<Props>,
    pub add_vectors: Vec<Props>,
    pub coordinate_scale: Props,
    pub coordinate_bias: Props,
    pub inv_uniform_penumbra_size: Option<Props>,
    pub b_shadow_channel_valid: Option<Vec<bool>>,
    pub light_guids: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ShadowMap2D {
    pub texture: Ref,
    pub coordinate_scale: Props,
    pub coordinate_bias: Props,
    pub b_channel_valid: Vec<bool>,
    pub inv_uniform_penumbra_size: Props,
    pub light_guids: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct InstanceLightmapData {
    pub lightmap_uv_bias: Props,
    pub shadowmap_uv_bias: Props,
}

#[derive(Clone, Debug, Default)]
pub struct MeshMapBuildData {
    pub light_map: Option<LightMap2D>,
    pub shadow_map: Option<ShadowMap2D>,
    pub irrelevant_lights: Vec<String>,
    pub per_instance_lightmap_data: Vec<InstanceLightmapData>,
}

/// FVolumeLightingSample[] as stored: `count` samples of `stride` bytes, each Position (3 float), Radius, Lighting
/// (3 channels of order * order float: 4 or 9), PackedSkyBentNormal (FColor, B G R A), DirectionalLightShadowing.
#[derive(Clone, Debug, Default)]
pub struct VolumeLightingSamples<'a> {
    pub count: usize,
    pub stride: usize,
    pub data: &'a [u8],
}

/// FPrecomputedLightVolumeData (the indirect lighting cache's samples).
#[derive(Clone, Debug)]
pub struct PrecomputedLightVolumeData<'a> {
    pub bounds: Props,
    /// A float: written as a number, or the string a non-finite float is written as.
    pub sample_spacing: Value,
    pub num_sh_samples: i32,
    pub high_quality_samples: VolumeLightingSamples<'a>,
    pub low_quality_samples: VolumeLightingSamples<'a>,
}

/// FVolumetricLightmapDataLayer: the layer's bytes and its pixel format's name.
#[derive(Clone, Debug, Default)]
pub struct DataLayer<'a> {
    pub data: &'a [u8],
    pub pixel_format_string: String,
}

#[derive(Clone, Debug, Default)]
pub struct BrickData<'a> {
    pub lq_light_color: Option<DataLayer<'a>>,
    pub lq_light_direction: Option<DataLayer<'a>>,
    pub ambient_vector: DataLayer<'a>,
    pub sh_coefficients: Vec<DataLayer<'a>>,
    pub sky_bent_normal: DataLayer<'a>,
    pub directional_light_shadowing: DataLayer<'a>,
}

#[derive(Clone, Debug, Default)]
pub struct VolumetricLightmapData<'a> {
    pub bounds: Props,
    pub indirection_texture_dimensions: Props,
    pub indirection_texture: DataLayer<'a>,
    pub brick_size: i32,
    pub brick_data_dimensions: Props,
    pub brick_data: BrickData<'a>,
    pub sub_level_brick_positions: Option<Vec<Props>>,
    pub indirection_texture_original_values: Option<Vec<Props>>,
}

#[derive(Clone, Debug, Default)]
pub struct DepthMap<'a> {
    pub world_to_light: Props,
    pub shadow_map_size_x: i32,
    pub shadow_map_size_y: i32,
    /// FFloat16 samples, 2 bytes each, as stored.
    pub depth_samples: &'a [u8],
}

#[derive(Clone, Debug, Default)]
pub struct LightComponentMapBuildData<'a> {
    pub shadow_map_channel: i32,
    pub depth_map: DepthMap<'a>,
}

#[derive(Clone, Debug)]
pub struct ReflectionCaptureMapBuildData<'a> {
    pub cubemap_size: i32,
    pub average_brightness: Value,
    pub brightness: Value,
    /// The full HDR capture.
    pub full_hdr_captured_data: &'a [u8],
    pub encoded_capture_data: Ref,
}

/// What the registry's native tail holds; each map keyed by its guid as CUE4Parse writes a map key (32 hex digits),
/// in stored order. A map the build does not serialize (or an empty one) is empty.
#[derive(Clone, Debug, Default)]
pub struct MapBuildData<'a> {
    pub mesh_build_data: IndexMap<String, MeshMapBuildData>,
    /// By level; an invalid one (bValid or bInitialized false) is `None`.
    pub level_precomputed_light_volume_build_data: IndexMap<String, Option<PrecomputedLightVolumeData<'a>>>,
    /// An invalid volume (its bValid false) is `None`.
    pub level_precomputed_volumetric_lightmap_build_data: IndexMap<String, Option<VolumetricLightmapData<'a>>>,
    pub light_build_data: IndexMap<String, LightComponentMapBuildData<'a>>,
    pub reflection_capture_build_data: IndexMap<String, ReflectionCaptureMapBuildData<'a>>,
    /// FSkyAtmosphereMapBuildData holds nothing in this version: the keys alone.
    pub sky_atmosphere_build_data: Vec<String>,
}

/// The guid of FReflectionCaptureObjectVersion.
const REFLECTION_CAPTURE_VERSION: Guid = Guid::from_parts(0x6B266CEC, 0x1EC74B8F, 0xA30BE4D9, 0x0942FC07);
/// FRenderingObjectVersion values the reads test.
const R_INDIRECT_LIGHTING_CACHE_3BAND: i32 = 1;
const R_VOLUMETRIC_LIGHTMAPS: i32 = 20;
const R_STORE_REFLECTION_CAPTURE_BRIGHTNESS: i32 = 22;
const R_VIRTUAL_TEXTURED_LIGHTMAPS: i32 = 27;
const R_LIGHTMAP_HAS_SHADOWMAP_DATA: i32 = 29;
const R_SKY_ATMOSPHERE_STATIC_LIGHTING: i32 = 32;
const R_VOLUMETRIC_LIGHTMAP_STREAMING: i32 = 34;
const R_VIRTUAL_TEXTURED_LIGHTMAPS_V3: i32 = 45;
/// FMobileObjectVersion values.
const M_LQ_VOLUMETRIC_LIGHTMAP_LAYERS: i32 = 2;
const M_STORE_REFLECTION_CAPTURE_COMPRESSED: i32 = 3;
/// FFortniteMainBranchObjectVersion.VolumetricLightMapGridDescSupport.
const F_VOLUMETRIC_LIGHTMAP_GRID_DESC: i32 = 139;
/// EUnrealEngineObjectUE4Version values.
const VER_COMBINED_LIGHTMAP_TEXTURES: i32 = 305;
const VER_SKY_LIGHT_COMPONENT: i32 = 328;
const VER_STATIC_SHADOWMAP_PENUMBRA_SIZE: i32 = 468;
const VER_AO_MATERIAL_MASK: i32 = 477;
const VER_SKY_BENT_NORMAL: i32 = 346;

type Ar<'a, 'b> = &'b mut ExportArchive<'a>;

/// A native struct as its members (`structs.rs`; each of these types is one it reads).
fn native_struct(ar: Ar<'_, '_>, ty: &str) -> Result<Props> {
    match read_native_struct(ar, ty, ReadType::Normal)? {
        Some(Value::Struct(p)) => Ok(p),
        other => Err(ReadError::parse(format!("{}: {ty} read as {other:?}", ar.name))),
    }
}

fn guid_key(ar: Ar<'_, '_>) -> Result<String> {
    Ok(ar.guid()?.to_string().replace('-', ""))
}

fn guid_text(ar: Ar<'_, '_>) -> Result<String> {
    Ok(ar.guid()?.to_string())
}

fn array<'a, T>(ar: Ar<'a, '_>, mut read: impl FnMut(Ar<'a, '_>) -> Result<T>) -> Result<Vec<T>> {
    let n = ar.i32()?;
    let n = match usize::try_from(n) {
        Ok(n) if n <= ar.len().saturating_sub(ar.pos) => n,
        _ => return Err(ReadError::parse(format!("{}: array of {n} at {}", ar.name, ar.pos - 4))),
    };
    (0..n).map(|_| read(ar)).collect()
}

/// A TArray<uint8> (or another element of `size` bytes) as its bytes.
fn byte_array<'a>(ar: Ar<'a, '_>, size: usize) -> Result<&'a [u8]> {
    let n = ar.i32()?;
    let Ok(n) = usize::try_from(n) else {
        return Err(ReadError::parse(format!("{}: array of {n} at {}", ar.name, ar.pos - 4)));
    };
    ar.bytes(n * size)
}

/// FArchive.ReadMap over FGuid keys; a key met twice is an error (a C# Dictionary refuses it).
fn guid_map<'a, T>(ar: Ar<'a, '_>, mut read: impl FnMut(Ar<'a, '_>) -> Result<T>) -> Result<IndexMap<String, T>> {
    let mut out = IndexMap::new();
    let n = ar.i32()?;
    let n = match usize::try_from(n) {
        Ok(n) if n * 16 <= ar.len().saturating_sub(ar.pos) => n,
        _ => return Err(ReadError::parse(format!("{}: map of {n} at {}", ar.name, ar.pos - 4))),
    };
    for _ in 0..n {
        let k = guid_key(ar)?;
        if out.contains_key(&k) {
            return Err(ReadError::parse(format!("{}: map key {k} twice", ar.name)));
        }
        let v = read(ar)?;
        out.insert(k, v);
    }
    Ok(out)
}

fn bools4(ar: Ar<'_, '_>) -> Result<Vec<bool>> {
    Ok(vec![ar.bool()?, ar.bool()?, ar.bool()?, ar.bool()?])
}

fn read_light_map_2d(ar: Ar<'_, '_>) -> Result<LightMap2D> {
    let light_guids = array(ar, guid_text)?; // FLightMap
    if ar.ver() <= VER_COMBINED_LIGHTMAP_TEXTURES {
        return Err(ReadError::not_ported("FLightMap2D before COMBINED_LIGHTMAP_TEXTURES"));
    }
    let textures = [ar.index()?, ar.index()?];
    let (mut sky_occlusion_texture, mut ao_material_mask_texture) = (None, None);
    if ar.ver() >= VER_SKY_LIGHT_COMPONENT {
        sky_occlusion_texture = Some(ar.index()?);
        if ar.ver() >= VER_AO_MATERIAL_MASK {
            ao_material_mask_texture = Some(ar.index()?);
        }
    }
    let (mut scale_vectors, mut add_vectors) = (Vec::new(), Vec::new());
    for _ in 0..4 {
        scale_vectors.push(native_struct(ar, "Vector4")?);
        add_vectors.push(native_struct(ar, "Vector4")?);
    }
    let coordinate_scale = native_struct(ar, "Vector2D")?;
    let coordinate_bias = native_struct(ar, "Vector2D")?;
    let rendering = ar.custom(versions::RENDERING);
    let (mut b_shadow_channel_valid, mut inv_uniform_penumbra_size) = (None, None);
    if rendering >= R_LIGHTMAP_HAS_SHADOWMAP_DATA {
        b_shadow_channel_valid = Some(bools4(ar)?);
        inv_uniform_penumbra_size = Some(native_struct(ar, "Vector4")?);
    }
    let mut virtual_textures = [None, None];
    if rendering >= R_VIRTUAL_TEXTURED_LIGHTMAPS {
        virtual_textures[0] = ar.index()?;
        // V2 and V3 each read the first; V3 adds the second
        if rendering >= R_VIRTUAL_TEXTURED_LIGHTMAPS_V3 {
            virtual_textures[1] = ar.index()?;
        }
    }
    Ok(LightMap2D {
        textures,
        sky_occlusion_texture,
        ao_material_mask_texture,
        virtual_textures,
        scale_vectors,
        add_vectors,
        coordinate_scale,
        coordinate_bias,
        inv_uniform_penumbra_size,
        b_shadow_channel_valid,
        light_guids,
    })
}

fn read_shadow_map_2d(ar: Ar<'_, '_>) -> Result<ShadowMap2D> {
    let light_guids = array(ar, guid_text)?; // FShadowMap
    let texture = ar.index()?;
    let coordinate_scale = native_struct(ar, "Vector2D")?;
    let coordinate_bias = native_struct(ar, "Vector2D")?;
    let b_channel_valid = bools4(ar)?;
    if ar.ver() < VER_STATIC_SHADOWMAP_PENUMBRA_SIZE {
        return Err(ReadError::not_ported("FShadowMap2D before STATIC_SHADOWMAP_PENUMBRA_SIZE"));
    }
    let inv_uniform_penumbra_size = native_struct(ar, "Vector4")?;
    Ok(ShadowMap2D {
        texture,
        coordinate_scale,
        coordinate_bias,
        b_channel_valid,
        inv_uniform_penumbra_size,
        light_guids,
    })
}

fn read_mesh_map_build_data(ar: Ar<'_, '_>) -> Result<MeshMapBuildData> {
    let mut o = MeshMapBuildData::default();
    let lm_type = ar.u32()?; // ELightMapType: LMT_None 0, LMT_1D 1, LMT_2D 2
    if lm_type == 1 {
        return Err(ReadError::not_ported("FLegacyLightMap1D not ported"));
    }
    if lm_type == 2 {
        o.light_map = Some(read_light_map_2d(ar)?);
    }
    let sm_type = ar.u32()?; // EShadowMapType: SMT_None 0, SMT_2D 2
    if sm_type == 2 {
        o.shadow_map = Some(read_shadow_map_2d(ar)?);
    }
    o.irrelevant_lights = array(ar, guid_text)?;
    // ReadBulkArray: element size, count, then the elements (FPerInstanceLightmapData: two FVector2D)
    let size = ar.i32()?;
    let n = ar.i32()?;
    if n > 0 {
        if size != 16 {
            return Err(ReadError::parse(format!("{}: FPerInstanceLightmapData of {size} bytes", ar.name)));
        }
        for _ in 0..n {
            let lightmap_uv_bias = native_struct(ar, "Vector2D")?;
            let shadowmap_uv_bias = native_struct(ar, "Vector2D")?;
            o.per_instance_lightmap_data.push(InstanceLightmapData { lightmap_uv_bias, shadowmap_uv_bias });
        }
    }
    Ok(o)
}

/// An array of FVolumeLightingSample (fixed size: Position, Radius, Lighting[3][order * order], PackedSkyBentNormal,
/// DirectionalLightShadowing for this version) as its count and bytes.
fn volume_samples<'a>(ar: Ar<'a, '_>, order: usize) -> Result<VolumeLightingSamples<'a>> {
    let stride = 12 + 4 + 3 * order * order * 4 + 4 + 4;
    let count = ar.i32()?;
    let Ok(n) = usize::try_from(count) else {
        return Err(ReadError::parse(format!("{}: array of {count} at {}", ar.name, ar.pos - 4)));
    };
    Ok(VolumeLightingSamples { count: n, stride, data: ar.bytes(n * stride)? })
}

fn read_precomputed_light_volume<'a>(ar: Ar<'a, '_>) -> Result<Option<PrecomputedLightVolumeData<'a>>> {
    // operator<<(FPrecomputedLightVolumeData*): bValid, then the volume's own bInitialized and its samples
    if !ar.bool()? || !ar.bool()? {
        return Ok(None);
    }
    if ar.ver() < VER_SKY_BENT_NORMAL {
        return Err(ReadError::not_ported("FVolumeLightingSample before SKY_BENT_NORMAL"));
    }
    let bounds = native_struct(ar, "Box")?;
    let sample_spacing = Value::Float(ar.f32()?);
    let num_sh_samples = if ar.custom(versions::RENDERING) >= R_INDIRECT_LIGHTING_CACHE_3BAND { ar.i32()? } else { 4 };
    let order = if num_sh_samples == 9 { 3 } else { 2 };
    let high_quality_samples = volume_samples(ar, order)?;
    let low_quality_samples = volume_samples(ar, order)?;
    Ok(Some(PrecomputedLightVolumeData {
        bounds,
        sample_spacing,
        num_sh_samples,
        high_quality_samples,
        low_quality_samples,
    }))
}

fn data_layer<'a>(ar: Ar<'a, '_>) -> Result<DataLayer<'a>> {
    Ok(DataLayer { data: byte_array(ar, 1)?, pixel_format_string: ar.fstring()? })
}

fn read_volumetric_lightmap<'a>(ar: Ar<'a, '_>) -> Result<Option<VolumetricLightmapData<'a>>> {
    if !ar.bool()? {
        return Ok(None);
    }
    let bounds = native_struct(ar, "Box")?;
    let indirection_texture_dimensions = native_struct(ar, "IntVector")?;
    let indirection_texture = data_layer(ar)?;
    let brick_size = ar.i32()?;
    let brick_data_dimensions = native_struct(ar, "IntVector")?;
    let ambient_vector = data_layer(ar)?;
    let mut sh_coefficients = Vec::new();
    for _ in 0..6 {
        sh_coefficients.push(data_layer(ar)?);
    }
    let sky_bent_normal = data_layer(ar)?;
    let directional_light_shadowing = data_layer(ar)?;
    let mut brick_data = BrickData {
        lq_light_color: None,
        lq_light_direction: None,
        ambient_vector,
        sh_coefficients,
        sky_bent_normal,
        directional_light_shadowing,
    };
    // FUE5MainStreamObjectVersion is 0 for 4.26 (at most MobileStationaryLocalLights): the LQ layers follow
    if ar.custom(versions::MOBILE) >= M_LQ_VOLUMETRIC_LIGHTMAP_LAYERS {
        brick_data.lq_light_color = Some(data_layer(ar)?);
        brick_data.lq_light_direction = Some(data_layer(ar)?);
    }
    let mut o = VolumetricLightmapData {
        bounds,
        indirection_texture_dimensions,
        indirection_texture,
        brick_size,
        brick_data_dimensions,
        brick_data,
        sub_level_brick_positions: None,
        indirection_texture_original_values: None,
    };
    if ar.custom(versions::RENDERING) >= R_VOLUMETRIC_LIGHTMAP_STREAMING {
        o.sub_level_brick_positions = Some(array(ar, |a| native_struct(a, "IntVector"))?);
        o.indirection_texture_original_values = Some(array(ar, |a| native_struct(a, "Color"))?);
    }
    Ok(Some(o))
}

fn read_light_component_map_build_data<'a>(ar: Ar<'a, '_>) -> Result<LightComponentMapBuildData<'a>> {
    let shadow_map_channel = ar.i32()?;
    let world_to_light = native_struct(ar, "Matrix")?;
    let shadow_map_size_x = ar.i32()?;
    let shadow_map_size_y = ar.i32()?;
    let depth_samples = byte_array(ar, 2)?;
    Ok(LightComponentMapBuildData {
        shadow_map_channel,
        depth_map: DepthMap { world_to_light, shadow_map_size_x, shadow_map_size_y, depth_samples },
    })
}

fn read_reflection_capture<'a>(ar: Ar<'a, '_>) -> Result<ReflectionCaptureMapBuildData<'a>> {
    let cubemap_size = ar.i32()?;
    let average_brightness = Value::Float(ar.f32()?);
    // FUE5ReleaseStreamObjectVersion is 0 for 4.26: before ExcludeBrightnessFromEncodedHDRCubemap
    let brightness = if ar.custom(versions::RENDERING) >= R_STORE_REFLECTION_CAPTURE_BRIGHTNESS {
        Value::Float(ar.f32()?)
    } else {
        Value::Int(0)
    };
    let full_hdr_captured_data = byte_array(ar, 1)?;
    // before StoreReflectionCaptureEncodedHDRDataInRG11B10Format (UE5): a reference, else a byte array
    let mut encoded_capture_data = None;
    if ar.custom(versions::MOBILE) >= M_STORE_REFLECTION_CAPTURE_COMPRESSED {
        encoded_capture_data = ar.index()?;
    } else {
        byte_array(ar, 1)?;
    }
    Ok(ReflectionCaptureMapBuildData {
        cubemap_size,
        average_brightness,
        brightness,
        full_hdr_captured_data,
        encoded_capture_data,
    })
}

/// The native tail of a MapBuildDataRegistry export. Fails when the read does not end where the export does.
pub fn read_map_build_data<'a>(
    cache: &'a PackageCache,
    pkg: &'a Arc<Package>,
    index: usize,
) -> Result<MapBuildData<'a>> {
    // the tagged properties and object guid (baseDeserialize of a fresh object)
    let mut obj = new_object(cache, pkg, index);
    let (mut ar, valid_pos) = export_archive(cache, pkg, index)?;
    base_deserialize(&mut obj, &mut ar, valid_pos)?;
    let ar = &mut ar;
    let mut out = MapBuildData::default();
    let global_strip = ar.u8()?;
    ar.u8()?; // FStripDataFlags: class flags
    if global_strip & 2 != 0 {
        return Ok(out); // audio-visual data stripped
    }
    let rendering = ar.custom(versions::RENDERING);
    out.mesh_build_data = guid_map(ar, read_mesh_map_build_data)?;
    out.level_precomputed_light_volume_build_data = guid_map(ar, read_precomputed_light_volume)?;
    if rendering >= R_VOLUMETRIC_LIGHTMAPS {
        out.level_precomputed_volumetric_lightmap_build_data = guid_map(ar, read_volumetric_lightmap)?;
    }
    out.light_build_data = guid_map(ar, read_light_component_map_build_data)?;
    // FReflectionCaptureObjectVersion: none in the package and none in versions: CUE4Parse's default for 4.19 on
    let rc = ar.custom(REFLECTION_CAPTURE_VERSION);
    if (if rc < 0 { 1 } else { rc }) >= 1 {
        out.reflection_capture_build_data = guid_map(ar, read_reflection_capture)?;
    }
    if rendering >= R_SKY_ATMOSPHERE_STATIC_LIGHTING {
        out.sky_atmosphere_build_data = guid_map(ar, |_| Ok(()))?.into_keys().collect();
    }
    if ar.custom(versions::FORTNITE_MAIN) >= F_VOLUMETRIC_LIGHTMAP_GRID_DESC {
        ar.bool()?;
    }
    if ar.pos != valid_pos {
        return Err(ReadError::parse(format!(
            "{}: MapBuildDataRegistry read ended at {}, the export at {valid_pos}",
            pkg.path, ar.pos
        )));
    }
    Ok(out)
}
