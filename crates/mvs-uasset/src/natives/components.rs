//! Actors, components and the other classes whose CUE4Parse 1.2.2 Deserialize / WriteJson add nothing to the JSON of
//! an unversioned UE 4.26 package (entered empty, each with the reason), and the few component classes that do write
//! something: ULocalLightComponent's IntensityNits, UStaticMeshComponent's LODData, UParticleModuleRequired's
//! BoundingGeometry, USkyAtmosphereComponent's bStaticLightingBuiltGUID; and what the object kind keeps beyond them:
//! UAtmosphericFogComponent's precomputed tables and the instanced static mesh components' instances.

use std::sync::Arc;

use super::Native;
use crate::archive::{Guid, ReadError, Result};
use crate::bulk::{read_bulk_array, read_bulk_payload};
use crate::objects::{ExportArchive, UObj, super_deserialize};
use crate::value::{Props, Value};
use crate::versions;

const EMPTY: Native = Native { deserialize: None, json: None, extra: None };

/// By C# class name. The empty entries: classes whose override reads nothing more for 4.26 and writes nothing to the
/// JSON, the reason beside each.
pub static NATIVES: &[(&str, Native)] = &[
    // ActorLabel needs FUE5SpecialProjectStreamObjectVersion >= 4, ActorInstanceGuid FFortniteMainBranch >= 150
    // (4.26: 0, 43)
    ("AActor", EMPTY),
    // UCSModifiedProperties needs FFortniteReleaseBranchCustom >= 4 (4.26: 1)
    ("UActorComponent", EMPTY),
    // GetOrDefault only; Bounds needs FUE5SpecialProjectStreamObjectVersion >= 2 (4.26: 0)
    ("USceneComponent", EMPTY),
    // component GUID only for UE3 versions 196..272 (Ar.Ver UE3 is 864)
    ("UPrimitiveComponent", EMPTY),
    ("UMeshComponent", EMPTY), // GetOrDefault only
    // GetOrDefault only; BodySetup only before UE4 version 410
    ("USkeletalMeshComponent", EMPTY),
    ("USCS_Node", EMPTY),                    // GetOrDefault only
    ("USimpleConstructionScript", EMPTY),    // GetOrDefault only
    ("UInheritableComponentHandler", EMPTY), // GetOrDefault only
    ("USkeletalMeshSocket", EMPTY),          // GetOrDefault only
    ("ULightComponentBase", EMPTY),          // GetOrDefault only
    // LegacyData needs FRenderingObjectVersion < 9 (4.26: 44)
    ("ULightComponent", EMPTY),
    // GetOrDefault only (its GetNitIntensity is ULocalLightComponent's json below)
    ("UPointLightComponent", EMPTY),
    ("USpotLightComponent", EMPTY), // GetOrDefault only (cone angles: ULocalLightComponent's json)
    ("URectLightComponent", EMPTY), // GetOrDefault only (source size: ULocalLightComponent's json)
    ("UDirectionalLightComponent", EMPTY), // GetOrDefault only
    ("USkyLightComponent", EMPTY),  // reads nothing for 4.26
    ("UDecalComponent", EMPTY),     // GetOrDefault only
    ("UArrowComponent", EMPTY),     // GetOrDefault only
    // GetOrDefault only; the brush bulk array before UE4 only
    ("UBrushComponent", EMPTY),
    ("UParticleSystemComponent", EMPTY),   // skips bytes for another game only
    ("UParticleSystem", EMPTY),            // skips bytes for another game only
    ("ADecalActor", EMPTY),                // GetOrDefault only
    ("AWorldSettings", EMPTY),             // GetOrDefault only
    ("ULevelStreaming", EMPTY),            // GetOrDefault only
    ("UPhysicsConstraintTemplate", EMPTY), // GetOrDefault only
    ("UMaterialInstanceConstant", EMPTY),  // GetOrDefault only
    ("UMaterialFunction", EMPTY),          // skips bytes for another game only
    ("UAnimSequenceBase", EMPTY),          // GetOrDefault only
    ("UAnimMontage", EMPTY),               // GetOrDefault only
    ("UAnimComposite", EMPTY),             // GetOrDefault only
    ("UBlendSpaceBase", EMPTY),            // GetOrDefault / TryGetAllValues only
    // reads more for another game only (UDataTable reads the rows)
    ("UCompositeDataTable", EMPTY),
    // GetOrDefault only (UBodySetup below it is not ported)
    ("USkeletalBodySetup", EMPTY),
    // Get only (throws when Codecs is missing; the JSON keeps the properties)
    ("UAnimBoneCompressionSettings", EMPTY),
    ("UAnimCurveCompressionSettings", EMPTY), // GetOrDefault only
    // reads the character remap after the properties; its WriteJson writes none of it
    ("UFont", EMPTY),
    // reads the font face data after the properties; its WriteJson writes none of it
    ("UFontFace", EMPTY),
    // reads three ints after UTexture; its WriteJson writes none of them
    ("UTextureRenderTarget", EMPTY),
    ("ULocalLightComponent", Native { deserialize: None, json: Some(local_light_json), extra: None }),
    (
        "UStaticMeshComponent",
        Native { deserialize: Some(static_mesh_deserialize), json: Some(static_mesh_json), extra: None },
    ),
    (
        "UParticleModuleRequired",
        Native { deserialize: Some(particle_required_deserialize), json: Some(particle_required_json), extra: None },
    ),
    ("USkyAtmosphereComponent", Native { deserialize: Some(sky_deserialize), json: Some(sky_json), extra: None }),
    ("UAtmosphericFogComponent", Native { deserialize: Some(fog_deserialize), json: None, extra: Some(fog_extra) }),
    (
        "UInstancedStaticMeshComponent",
        Native { deserialize: Some(ism_deserialize), json: None, extra: Some(ism_extra) },
    ),
    (
        "UHierarchicalInstancedStaticMeshComponent",
        Native { deserialize: Some(hism_deserialize), json: None, extra: None },
    ),
];

/// FUE5MainStreamObjectVersion.
const UE5_MAIN: Guid = Guid::from_parts(0x697DD581, 0xE64F41AB, 0xAA4A51EC, 0xBEB7B628);
/// FUE5MainStreamObjectVersion::RemovedAtmosphericFog.
const REMOVED_ATMOSPHERIC_FOG: i32 = 46;
/// FRenderingObjectVersion::PerInstanceCustomData.
const R_PER_INSTANCE_CUSTOM_DATA: i32 = 38;
const NO_GUID: &str = "00000000-00000000-00000000-00000000";

// ---------------------------------------------------------------------------------------------------------------
// ULocalLightComponent: GetNitIntensity, in float as C# computes it

/// ELightUnits by name; an EnumProperty value is "ELightUnits::Name".
const LIGHT_UNITS: &[&str] = &["Unitless", "Candelas", "Lumens", "EV", "Nits"];
const PI32: f32 = std::f32::consts::PI;

/// Math.max as JS has it on two floats: NaN when either is, +0 over -0.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { b } else { a }
    } else if a > b {
        a
    } else {
        b
    }
}

/// Math.min as JS has it on two floats: NaN when either is, -0 over +0.
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { a } else { b }
    } else if a < b {
        a
    } else {
        b
    }
}

/// js_max / js_min of two f32 (exact: the result is one of them).
fn max32(a: f32, b: f32) -> f32 {
    js_max(f64::from(a), f64::from(b)) as f32
}
fn min32(a: f32, b: f32) -> f32 {
    js_min(f64::from(a), f64::from(b)) as f32
}

/// A number property as float (C#'s GetOrDefault<float>), else `d`.
fn num(o: &UObj, k: &str, d: f32) -> f32 {
    match o.props.get(k) {
        Some(Value::Float(v)) => *v,
        Some(Value::Double(v)) => *v as f32,
        Some(Value::Int(v)) => *v as f64 as f32,
        Some(Value::UInt(v)) => *v as f64 as f32,
        _ => d,
    }
}

fn bool_prop(o: &UObj, k: &str) -> Option<bool> {
    o.props.get(k).and_then(Value::as_bool)
}

/// LightUtils.ConvertToIntensityToNits, in float as C# computes it; `units` an index of [`LIGHT_UNITS`].
fn to_nits(i: f32, area: f32, angle: f32, units: usize) -> f32 {
    match units {
        1 => i / area,
        2 => i / (angle * area),
        // MathF.Pow. Unverified: pow in double rounded to float, as JavaScript's Math.pow; powf can differ by one ulp
        3 => 2f64.powf(f64::from(i)) as f32,
        4 => i,
        _ => (i / 625.0) / area,
    }
}

/// GetNitIntensity of a ULocalLightComponent, its point / spot / rect overrides by the object's chain.
fn nit_intensity(o: &UObj) -> f32 {
    let intensity = num(o, "Intensity", num(o, "Brightness", PI32));
    // GetOrDefault("IntensityUnits", Provider.DefaultLightUnit): the build's DefaultEngine.ini sets no
    // r.DefaultFeature.LightUnits, so the default is Unitless
    let units = match o.props.get("IntensityUnits").and_then(Value::as_str) {
        Some(u) => {
            let n = u.rsplit_once("::").map_or(u, |(_, n)| n);
            LIGHT_UNITS.iter().position(|x| *x == n).unwrap_or(0)
        }
        None => 0,
    };
    let has = |c: &str| o.chain.contains(&c);
    if has("URectLightComponent") {
        let area = (num(o, "SourceWidth", 64.0) / 100.0) * (num(o, "SourceHeight", 64.0) / 100.0);
        return to_nits(if area.abs() <= 0.0001 { 0.0 } else { intensity }, area, PI32, units);
    }
    if has("UPointLightComponent") {
        let inverse = bool_prop(o, "bUseInverseSquaredFalloff").or_else(|| bool_prop(o, "InverseSquaredFalloff"));
        if !inverse.unwrap_or(true) {
            return intensity;
        }
        let mut angle = PI32 * 4.0;
        if has("USpotLightComponent") {
            let inner = (min32(max32(num(o, "InnerConeAngle", 0.0), 0.0), 89.0) * PI32) / 180.0;
            let outer = (num(o, "OuterConeAngle", 44.0) * PI32) / 180.0;
            let half = min32(max32(outer, inner + 0.001), 1.554_343_1);
            // MathF.Cos. Unverified: cos in double rounded to float, as JavaScript's Math.cos; cosf can differ by one
            // ulp
            angle = (PI32 * 2.0) * (1.0 - f64::from(half).cos() as f32);
        }
        let radius = num(o, "SourceRadius", 0.0);
        let r = f64::from(radius / 100.0);
        // in double, as C# promotes Math.Pow's result
        let area = js_max(f64::from(angle) * (r * r), 9.999_999_747_378_752e-5) as f32;
        return to_nits(if radius.abs() <= 0.0001 { 0.0 } else { intensity }, area, angle, units);
    }
    intensity
}

fn local_light_json(obj: &UObj, o: &mut Props) {
    o.insert("IntensityNits".into(), Value::Float(nit_intensity(obj)));
}

// ---------------------------------------------------------------------------------------------------------------
// UStaticMeshComponent

/// FStaticMeshComponentLODInfo for 4.26 (FRenderingObjectVersion 44 >= MapBuildDataSeparatePackage: the map build
/// data id; UE3 version 864: the override colours flag), as FStaticMeshComponentLODInfoConverter writes it.
fn read_lod_info(ar: &mut ExportArchive<'_>) -> Result<Value> {
    let global_strip = ar.u8()?;
    let class_strip = ar.u8()?;
    let mut id = "00000000000000000000000000000000".to_owned();
    if global_strip & 2 == 0 {
        let Guid([a, b, c, d]) = ar.guid()?;
        id = format!("{a:08X}{b:08X}{c:08X}{d:08X}"); // FGuid.ToString(): Digits
    }
    if class_strip & 1 == 0 && ar.u8()? == 1 {
        return Err(ReadError::not_ported("FColorVertexBuffer (StaticMeshComponent OverrideVertexColors) not ported"));
    }
    // painted vertices: editor data only (the package is FilterEditorOnly)
    let mut p = Props::new();
    p.insert("MapBuildDataId".into(), Value::Str(id));
    Ok(Value::Struct(p))
}

fn static_mesh_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    if ar.pos == valid_pos {
        return Ok(()); // LODData stays null, and the JSON writes it so
    }
    let n = ar.i32()?;
    let mut lods = Vec::new();
    for _ in 0..n {
        lods.push(read_lod_info(ar)?);
    }
    obj.data.insert("LODData".into(), Value::Array(lods));
    // MeshPaintTextureCooked needs FFortniteMainBranch >= 166 (4.26: 43); the doubly serialised vertex colours only
    // UE3 versions 801..819
    Ok(())
}

fn static_mesh_json(obj: &UObj, o: &mut Props) {
    match obj.data.get("LODData") {
        None => {
            o.insert("LODData".into(), Value::Null);
        }
        Some(Value::Array(a)) if a.is_empty() => {}
        Some(v) => {
            o.insert("LODData".into(), v.clone());
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// UParticleModuleRequired: FRenderingObjectVersion >= MovedParticleCutoutsToRequiredModule (8; 4.26: 44)

fn particle_required_deserialize(
    obj: &mut UObj,
    ar: &mut ExportArchive<'_>,
    valid_pos: usize,
    level: usize,
) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    if !ar.bool()? {
        return Ok(());
    }
    let n = ar.i32()?;
    let mut g = Vec::new();
    for _ in 0..n {
        let mut p = Props::new();
        p.insert("X".into(), Value::Float(ar.f32()?));
        p.insert("Y".into(), Value::Float(ar.f32()?));
        g.push(Value::Struct(p));
    }
    obj.data.insert("BoundingGeometry".into(), Value::Array(g));
    Ok(())
}

fn particle_required_json(obj: &UObj, o: &mut Props) {
    if let Some(g @ Value::Array(a)) = obj.data.get("BoundingGeometry")
        && !a.is_empty()
    {
        o.insert("BoundingGeometry".into(), g.clone());
    }
}

// ---------------------------------------------------------------------------------------------------------------
// USkyAtmosphereComponent, UAtmosphericFogComponent

fn sky_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    let fog = obj.chain.contains(&"UAtmosphericFogComponent");
    if !fog || ar.custom(UE5_MAIN) >= REMOVED_ATMOSPHERIC_FOG {
        let g = ar.guid()?;
        obj.data.insert("bStaticLightingBuiltGUID".into(), Value::Str(g.to_string()));
    }
    Ok(())
}

fn sky_json(obj: &UObj, o: &mut Props) {
    let g = obj.data.get("bStaticLightingBuiltGUID").cloned().unwrap_or_else(|| Value::Str(NO_GUID.into()));
    o.insert("bStaticLightingBuiltGUID".into(), g);
}

/// One precomputed table of UAtmosphericFogComponent: its name, texel size, pixel format and bytes a texel.
struct FogTable {
    name: &'static str,
    size: [usize; 3],
    format: &'static str,
    bytes: usize,
}

/// UAtmosphericFogComponent's precomputed tables (4.26 AtmosphericFogComponent.h: TransmittanceData, IrradianceData,
/// InscatterData, each an FByteBulkData inline after the properties): their texel layout. The sizes are 4.26's
/// FAtmospherePrecomputeParameters defaults (TransmittanceTexWidth 256 x Height 64, IrradianceTexWidth 64 x Height 16,
/// InscatterMuSNum 32 x NuNum 8, MuNum 128, InscatterAltitudeSampleNum 2), the FColor tables as PF_B8G8R8A8 and the
/// FFloat16Color inscatter table as PF_FloatRGBA.
// Unverified: the 4.26 source's, not read from the exe; no component of the build sets PrecomputeParams, and each
// block's byte count is checked against them (65,536, 4,096, 524,288).
const FOG_TABLES: [FogTable; 3] = [
    FogTable { name: "TransmittanceData", size: [256, 64, 1], format: "PF_B8G8R8A8", bytes: 4 },
    FogTable { name: "IrradianceData", size: [64, 16, 1], format: "PF_B8G8R8A8", bytes: 4 },
    FogTable { name: "InscatterData", size: [256, 128, 2], format: "PF_FloatRGBA", bytes: 8 },
];

impl FogTable {
    fn byte_len(&self) -> usize {
        self.size[0] * self.size[1] * self.size[2] * self.bytes
    }
}

/// One table's bulk block: its texels (inline in the export) with their layout; `None` for an empty block.
fn fog_table(ar: &mut ExportArchive<'_>, t: &FogTable) -> Result<Option<Value>> {
    let read = read_bulk_payload(ar)?;
    let h = read.header;
    if h.size_on_disk == 0 && h.element_count == 0 {
        return Ok(None);
    }
    let want = t.byte_len();
    let Some(at) = read.inline_at else {
        return Err(ReadError::parse(format!(
            "UAtmosphericFogComponent: {} is not inline (flags 0x{:x})",
            t.name, h.flags
        )));
    };
    if h.size_on_disk as usize != want || usize::try_from(h.element_count).ok() != Some(want) {
        return Err(ReadError::parse(format!(
            "UAtmosphericFogComponent: {} holds {} bytes, {want} expected",
            t.name, h.size_on_disk
        )));
    }
    let mut p = Props::new();
    p.insert("SizeX".into(), Value::Int(t.size[0] as i64));
    p.insert("SizeY".into(), Value::Int(t.size[1] as i64));
    p.insert("SizeZ".into(), Value::Int(t.size[2] as i64));
    p.insert("PixelFormat".into(), Value::Str(t.format.into()));
    p.insert("Data".into(), Value::Bytes(Arc::from(&ar.ar.buf[at..at + want])));
    Ok(Some(Value::Struct(p)))
}

fn fog_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    if ar.custom(UE5_MAIN) >= REMOVED_ATMOSPHERIC_FOG {
        return Ok(());
    }
    // ATMOSPHERIC_FOG_CACHE_DATA (281) <= 522: transmittance and irradiance, then inscatter and the counter
    // (PrecomputeCounter). The three tables are kept (the atmosphere kind reads them); CUE4Parse skips them, so they
    // are written by `extra`, not `json`. A component whose tables were never computed (three empty blocks) writes
    // none.
    let mut tables = Vec::with_capacity(3);
    for t in &FOG_TABLES {
        tables.push(fog_table(ar, t)?);
    }
    let some = tables.iter().any(Option::is_some);
    if some != tables.iter().all(Option::is_some) {
        return Err(ReadError::parse("UAtmosphericFogComponent: some of its tables are empty"));
    }
    if some {
        obj.data.insert("atmosphereTables".into(), Value::Array(tables.into_iter().flatten().collect()));
    }
    ar.i32()?;
    obj.data.insert("bStaticLightingBuiltGUID".into(), Value::Str("00000001-00000000-00000000-00000000".into()));
    Ok(())
}

fn fog_extra(obj: &UObj, o: &mut Props) {
    let Some(Value::Array(tables)) = obj.data.get("atmosphereTables") else { return };
    for (t, v) in FOG_TABLES.iter().zip(tables) {
        o.insert(t.name.into(), v.clone());
    }
}

// ---------------------------------------------------------------------------------------------------------------
// UInstancedStaticMeshComponent, UHierarchicalInstancedStaticMeshComponent

/// 4.26 UInstancedStaticMeshComponent::Serialize, after UStaticMeshComponent's: bCooked (FMobileObjectVersion >=
/// InstancedStaticMeshLightmapSerialization, 1; 4.26: 3), PerInstanceSMData (BulkSerialize of
/// FInstancedStaticMeshInstanceData: its FMatrix Transform, 64 bytes), PerInstanceSMCustomData (BulkSerialize of
/// float, FRenderingObjectVersion >= PerInstanceCustomData). The instances are kept (the scenery kind's foliage reads
/// them) as `extra`, the bytes as stored (16 float32 an instance, the matrix's rows: X, Y, Z axes times scale, then
/// the translation; UE axes); CUE4Parse writes them as decomposed FTransforms, which the object kind does not. Then
/// the cooked render data (bCooked and FFortniteMainBranchObjectVersion / FEditorObjectVersion >=
/// SerializeInstancedStaticMeshRenderData): its size as an int64 and that many bytes (the GPU instance buffer the
/// engine would upload), skipped.
// Unverified: the layout read from the build's bytes against 4.26's source from memory (every foliage component of
// the four foliage levels ends at its export's end: the HISM read checks it). PerInstanceCustomData's number is not
// checked: 4.26's rendering version (44) is above it either way.
fn ism_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    if ar.custom(versions::MOBILE) >= 1 {
        ar.bool()?; // bCooked
    }
    let (n, inst) = read_bulk_array(&mut ar.ar, 64, "PerInstanceSMData")?;
    if n > 0 {
        obj.data.insert("perInstance".into(), Value::Bytes(Arc::from(inst)));
    }
    if ar.custom(versions::RENDERING) >= R_PER_INSTANCE_CUSTOM_DATA {
        let (n, custom) = read_bulk_array(&mut ar.ar, 4, "PerInstanceSMCustomData")?;
        if n > 0 {
            obj.data.insert("perInstanceCustom".into(), Value::Bytes(Arc::from(custom)));
        }
    }
    let size = ar.i64()?;
    match usize::try_from(size) {
        Ok(s) if ar.pos.checked_add(s).is_some_and(|end| end <= valid_pos) => ar.skip(s)?,
        _ => return Err(ReadError::parse(format!("UInstancedStaticMeshComponent: render data of {size} bytes"))),
    }
    // the export's end, where no class below on the chain reads on: a subclass's reader checks it after its own data,
    // as the HISM's; every one of them is in this table
    let below = obj.chain[..level].iter().any(|c| NATIVES.iter().any(|(n, x)| n == c && x.deserialize.is_some()));
    if !below && ar.pos != valid_pos {
        return Err(ReadError::parse(format!(
            "UInstancedStaticMeshComponent: read ended at {}, the export at {valid_pos}",
            ar.pos
        )));
    }
    Ok(())
}

fn ism_extra(obj: &UObj, o: &mut Props) {
    if let Some(v) = obj.data.get("perInstance") {
        o.insert("PerInstanceSMData".into(), v.clone());
    }
    if let Some(v) = obj.data.get("perInstanceCustom") {
        o.insert("PerInstanceSMCustomData".into(), v.clone());
    }
}

/// 4.26 UHierarchicalInstancedStaticMeshComponent::Serialize: ClusterTree (BulkSerialize of FClusterNode, 64 bytes:
/// BoundMin, FirstChild, BoundMax, LastChild, FirstInstance, LastInstance, MinInstanceScale, MaxInstanceScale), the
/// culling tree, read and left (the port draws every instance).
fn hism_deserialize(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()> {
    super_deserialize(obj, ar, valid_pos, level + 1)?;
    read_bulk_array(&mut ar.ar, 64, "ClusterTree")?;
    if ar.pos != valid_pos {
        return Err(ReadError::parse(format!(
            "UHierarchicalInstancedStaticMeshComponent: read ended at {}, the export at {valid_pos}",
            ar.pos
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_min_max() {
        assert!(js_max(f64::NAN, 1.0).is_nan());
        assert!(js_min(1.0, f64::NAN).is_nan());
        assert!(js_max(-0.0, 0.0).is_sign_positive());
        assert!(js_max(0.0, -0.0).is_sign_positive());
        assert!(js_min(0.0, -0.0).is_sign_negative());
        assert_eq!(js_max(2.0, 3.0), 3.0);
        assert_eq!(js_min(2.0, 3.0), 2.0);
    }

    #[test]
    fn nits_units() {
        assert_eq!(to_nits(625.0, 2.0, 1.0, 0), 0.5);
        assert_eq!(to_nits(10.0, 2.0, 1.0, 1), 5.0);
        assert_eq!(to_nits(12.0, 2.0, 3.0, 2), 2.0);
        assert_eq!(to_nits(3.0, 2.0, 3.0, 3), 8.0);
        assert_eq!(to_nits(7.0, 2.0, 3.0, 4), 7.0);
    }

    #[test]
    fn fog_table_sizes() {
        let lens: Vec<usize> = FOG_TABLES.iter().map(FogTable::byte_len).collect();
        assert_eq!(lens, [65_536, 4_096, 524_288]);
    }
}
