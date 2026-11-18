//! USkeletalMesh's native tail for the mesh kind (bounds, materials, the reference skeleton, the cooked LODs with
//! their sections, buffers and skin weights) and UMorphTarget's deltas. Never entered in NATIVES: the object kind keeps
//! these classes `partial`. It follows CUE4Parse 1.2.2 USkeletalMesh.Deserialize,
//! FSkeletalMeshRenderData, FSkinWeightVertexBuffer, UMorphTarget, the UE 4.26 cooked branch only. The reference
//! skeleton is `super::skeleton::read_reference_skeleton`.

use std::sync::Arc;

use super::mesh_buffers::{
    BoxSphereBounds, ColorBuffer, IndexBuffer, PositionBuffer, StaticVertexBuffer, read_box_sphere_bounds,
    read_color_buffer, read_multisize_index_container, read_position_buffer, read_static_vertex_buffer,
    read_strip_flags, skip_bulk_array, skip_fixed_array,
};
use super::skeleton::{NativeData, ReferenceSkeleton, at_native_data, ends_at, read_reference_skeleton};
use crate::archive::{ReadError, Result};
use crate::bulk::read_bulk_array;
use crate::objects::ExportArchive;
use crate::package::{Package, PackageCache};
use crate::value::{Props, Value};
use crate::versions;

#[derive(Clone, Debug, Default)]
pub struct SkelMeshSection {
    pub material_index: i32,
    pub base_index: i32,
    pub num_triangles: i32,
    pub recompute_tangent: bool,
    pub cast_shadow: bool,
    pub base_vertex_index: i32,
    /// Section bone index -> the mesh's reference skeleton bone index.
    pub bone_map: Vec<i32>,
    pub num_vertices: i32,
    pub max_bone_influences: i32,
    /// ClothMappingData holds entries (the section is drawn from a clothing asset's simulation).
    pub cloth: bool,
    pub correspond_cloth_asset_index: i32,
    pub disabled: bool,
}

/// FSkinWeightVertexBuffer (4.26: the unlimited-influences layout).
#[derive(Clone, Debug, Default)]
pub struct SkinWeightBuffer<'a> {
    pub variable_bones_per_vertex: bool,
    pub max_bone_influences: i32,
    pub num_bones: i32,
    pub vertices: i32,
    pub use16_bit_bone_index: bool,
    /// The weight data as stored: per vertex its influences' bone indices (u8 or u16), then their u8 weights.
    pub data: &'a [u8],
    /// Variable layout: per vertex (offset << 8 | influence count) into `data`.
    pub lookup: Option<Vec<u32>>,
}

#[derive(Clone, Debug, Default)]
pub struct SkelMeshLod<'a> {
    pub required_bones: Vec<i32>,
    pub active_bone_indices: Vec<i32>,
    /// bIsLODCookedOut: the LOD's buffers were not cooked (none of the fields below).
    pub cooked_out: bool,
    pub sections: Vec<SkelMeshSection>,
    pub index: Option<IndexBuffer<'a>>,
    pub position: Option<PositionBuffer<'a>>,
    pub vertex: Option<StaticVertexBuffer<'a>>,
    pub weights: Option<SkinWeightBuffer<'a>>,
    pub color: Option<ColorBuffer<'a>>,
    /// FSkinWeightProfilesData's profile count (read past).
    pub skin_weight_profiles: Option<i32>,
}

#[derive(Clone, Debug, Default)]
pub struct SkeletalMaterial {
    /// FPackageIndex of the material interface (0: none).
    pub material: i32,
    pub slot_name: String,
}

#[derive(Clone, Debug, Default)]
pub struct SkeletalMeshTail<'a> {
    /// The tagged properties, read on the way.
    pub props: Props,
    pub imported_bounds: BoxSphereBounds,
    pub materials: Vec<SkeletalMaterial>,
    pub ref_skeleton: ReferenceSkeleton,
    pub lods: Vec<SkelMeshLod<'a>>,
    /// FSkeletalMeshRenderData's NumInlinedLODs and NumNonOptionalLODs.
    pub inlined_lods: u8,
    pub non_optional_lods: u8,
}

/// UMorphTarget's LOD model: the sparse deltas as stored (position delta f32x3, TangentZ delta f32x3, source vertex
/// index u32: 28 bytes a delta), NumBaseMeshVerts and the section indices.
#[derive(Clone, Debug, Default)]
pub struct MorphTargetLod<'a> {
    pub count: usize,
    /// count x 28 bytes as stored.
    pub deltas: &'a [u8],
    pub num_base_mesh_verts: i32,
    pub section_indices: Vec<i32>,
    pub generated_by_engine: bool,
}

/// A u32 as the i32 the structs keep it in.
fn int(v: u32, what: &str, ar: &ExportArchive<'_>) -> Result<i32> {
    i32::try_from(v).map_err(|_| ReadError::parse(format!("{}: {what} {v}", ar.name)))
}

/// A count read from the package, as a length.
fn count(n: i32, what: &str, ar: &ExportArchive<'_>) -> Result<usize> {
    usize::try_from(n).map_err(|_| ReadError::parse(format!("{}: {what} {n}", ar.name)))
}

fn i16_array(ar: &mut ExportArchive<'_>) -> Result<Vec<i32>> {
    let n = ar.i32()?;
    let n =
        usize::try_from(n).map_err(|_| ReadError::parse(format!("{}: array length {n} at {}", ar.name, ar.pos - 4)))?;
    ar.need(n * 2)?;
    (0..n).map(|_| Ok(i32::from(ar.i16()?))).collect()
}

fn read_skeletal_material(ar: &mut ExportArchive<'_>) -> Result<SkeletalMaterial> {
    let material = ar.i32()?;
    // FEditorObjectVersion >= RefactorMeshEditorMaterials (8)
    let slot_name = ar.fname()?;
    // FCoreObjectVersion >= SkeletalMaterialEditorDataStripping (3): a bool says whether the imported slot name follows
    if ar.bool()? {
        ar.fname()?;
    }
    // FRenderingObjectVersion >= TextureStreamingMeshUVChannelData (10): FMeshUVChannelInfo (bInitialized,
    // bOverrideDensities, LocalUVDensities[4])
    ar.skip(4 + 4 + 16)?;
    if ar.custom(versions::FORTNITE_MAIN) >= 196 {
        ar.i32()?; // OverlayMaterialInterface (MeshMaterialSlotOverlayMaterialAdded)
    }
    Ok(SkeletalMaterial { material, slot_name })
}

fn read_section(ar: &mut ExportArchive<'_>) -> Result<SkelMeshSection> {
    let strip = read_strip_flags(ar)?;
    let material_index = i32::from(ar.i16()?);
    let base_index = ar.i32()?;
    let num_triangles = ar.i32()?;
    let recompute_tangent = ar.bool()?;
    ar.u8()?; // RecomputeTangentsVertexMaskChannel (FRecomputeTangentCustomVersion >= RecomputeTangentVertexColorMask)
    let cast_shadow = ar.bool()?; // FEditorObjectVersion >= RefactorMeshEditorMaterials
    // bVisibleInRayTracing: FUE5MainStreamObjectVersion 0, not serialized
    let base_vertex_index = ar.u32()?;
    let base_vertex_index = int(base_vertex_index, "base vertex index", ar)?;
    // ClothMappingData: one array (FUE5ReleaseStreamObjectVersion before AddClothMappingLODBias) of FMeshToMeshVertData:
    // three FVector4, four int16 source indices, Weight, Padding (FReleaseObjectVersion >= WeightFMeshToMeshVertData)
    let n_cloth = ar.i32()?;
    let n_cloth = count(n_cloth, "cloth mapping count", ar)?;
    ar.skip(n_cloth * 64)?;
    let n_map = ar.i32()?;
    let n_map = count(n_map, "BoneMap length", ar)?;
    ar.need(n_map * 2)?;
    let bone_map = (0..n_map).map(|_| Ok(i32::from(ar.u16()?))).collect::<Result<Vec<_>>>()?;
    let num_vertices = ar.i32()?;
    let max_bone_influences = ar.i32()?;
    let correspond_cloth_asset_index = i32::from(ar.i16()?);
    ar.skip(20)?; // FClothingSectionData: AssetGuid, AssetLodIndex
    if !strip.class_stripped(1) {
        // FDuplicatedVerticesBuffer: DupVertData, DupVertIndexData
        skip_fixed_array(ar, 4)?;
        skip_fixed_array(ar, 8)?;
    }
    let disabled = ar.bool()?; // FReleaseObjectVersion >= AddSkeletalMeshSectionDisable
    Ok(SkelMeshSection {
        material_index,
        base_index,
        num_triangles,
        recompute_tangent,
        cast_shadow,
        base_vertex_index,
        bone_map,
        num_vertices,
        max_bone_influences,
        cloth: n_cloth > 0,
        correspond_cloth_asset_index,
        disabled,
    })
}

/// FSkinWeightVertexBuffer for FAnimObjectVersion >= UnlimitedBoneInfluences: the data buffer (strip flags, the
/// variable flag, MaxBoneInfluences, NumBones, NumVertices, the 16-bit index flag, the weight bytes), then the lookup
/// buffer (strip flags, NumVertices, u32 a vertex).
pub fn read_skin_weight_buffer<'a>(ar: &mut ExportArchive<'a>) -> Result<SkinWeightBuffer<'a>> {
    let strip = read_strip_flags(ar)?;
    let variable_bones_per_vertex = ar.bool()?;
    let max = ar.u32()?;
    let num_bones = ar.u32()?;
    let vertices = ar.u32()?;
    // IncreaseBoneIndexLimitPerChunk
    let use16_bit_bone_index = if ar.custom(versions::ANIM) >= 4 { ar.bool()? } else { false };
    if strip.av_stripped() {
        return Err(ReadError::not_ported(format!(
            "{}: FSkinWeightVertexBuffer with its audio-visual data stripped not ported",
            ar.name
        )));
    }
    let (_, data) = read_bulk_array(&mut ar.ar, 1, "FSkinWeightVertexBuffer")?;
    let lookup_strip = read_strip_flags(ar)?;
    let lookup_vertices = ar.i32()?;
    let mut out = SkinWeightBuffer {
        variable_bones_per_vertex,
        max_bone_influences: int(max, "MaxBoneInfluences", ar)?,
        num_bones: int(num_bones, "NumBones", ar)?,
        vertices: int(vertices, "NumVertices", ar)?,
        use16_bit_bone_index,
        data,
        lookup: None,
    };
    if !lookup_strip.av_stripped() {
        let (n, l) = read_bulk_array(&mut ar.ar, 4, "FSkinWeightLookupVertexBuffer")?;
        if variable_bones_per_vertex {
            if i64::try_from(n).ok() != Some(i64::from(vertices)) || i64::from(lookup_vertices) != i64::from(vertices) {
                return Err(ReadError::parse(format!(
                    "{}: skin weight lookup of {n} for {vertices} vertices",
                    ar.name
                )));
            }
            out.lookup = Some(l.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect());
        }
    }
    if variable_bones_per_vertex && out.lookup.is_none() {
        return Err(ReadError::parse(format!("{}: variable bones per vertex with no lookup buffer", ar.name)));
    }
    if !variable_bones_per_vertex {
        let per = i64::from(out.max_bone_influences) * (if use16_bit_bone_index { 2 } else { 1 } + 1);
        if i64::try_from(data.len()).ok() != Some(i64::from(vertices) * per) {
            return Err(ReadError::parse(format!(
                "{}: {} skin weight bytes, {vertices} x {per} expected",
                ar.name,
                data.len()
            )));
        }
    }
    Ok(out)
}

/// FSkinWeightProfilesData: a map of profile name to FRuntimeSkinWeightProfileData (the 4.26 layout, package version
/// >= SKINWEIGHT_PROFILE_DATA_LAYOUT_CHANGES): read past.
fn skip_skin_weight_profiles(ar: &mut ExportArchive<'_>) -> Result<i32> {
    let n = ar.i32()?;
    for _ in 0..n {
        ar.fname()?;
        skip_fixed_array(ar, 1)?; // BoneIDs
        skip_fixed_array(ar, 1)?; // BoneWeights
        ar.u8()?; // NumWeightsPerVertex
        skip_fixed_array(ar, 8)?; // VertexIndexToInfluenceOffset
    }
    Ok(n)
}

/// FStaticLODModel.SerializeStreamedData (an inlined LOD's buffers).
fn read_streamed_data<'a>(
    ar: &mut ExportArchive<'a>,
    lod: &mut SkelMeshLod<'a>,
    has_vertex_colors: bool,
) -> Result<()> {
    let strip = read_strip_flags(ar)?;
    lod.index = Some(read_multisize_index_container(ar)?);
    lod.position = Some(read_position_buffer(ar)?);
    lod.vertex = Some(read_static_vertex_buffer(ar)?);
    lod.weights = Some(read_skin_weight_buffer(ar)?);
    if has_vertex_colors {
        lod.color = Some(read_color_buffer(ar)?);
    }
    if !strip.class_stripped(1) {
        read_multisize_index_container(ar)?; // AdjacencyIndexBuffer (before RemovingTessellation)
    }
    if lod.sections.iter().any(|s| s.cloth) {
        // FSkeletalMeshVertexClothBuffer: the vertex data, ClothIndexMapping (FSkeletalMeshCustomVersion >=
        // CompactClothVertexBuffer)
        if !read_strip_flags(ar)?.av_stripped() {
            skip_bulk_array(ar)?;
            skip_fixed_array(ar, 8)?;
        }
    }
    lod.skin_weight_profiles = Some(skip_skin_weight_profiles(ar)?);
    // SkeletalMesh.HasRayTracingData: false for UE4.26; the UE5 morph, attribute and half-edge buffers: not in 4.26
    Ok(())
}

/// FStaticLODModel.SerializeRenderItem (SkeletalMesh.UseNewCookedFormat, UE4.24+).
fn read_lod<'a>(ar: &mut ExportArchive<'a>, has_vertex_colors: bool) -> Result<SkelMeshLod<'a>> {
    let strip = read_strip_flags(ar)?;
    let cooked_out = ar.bool()?;
    let inlined = ar.bool()?;
    let required_bones = i16_array(ar)?;
    let mut lod = SkelMeshLod { required_bones, cooked_out, ..Default::default() };
    if strip.av_stripped() || cooked_out {
        return Ok(lod);
    }
    let n = ar.i32()?;
    let n = count(n, "section count", ar)?;
    for _ in 0..n {
        lod.sections.push(read_section(ar)?);
    }
    lod.active_bone_indices = i16_array(ar)?;
    ar.skip(4)?; // BuffersSize
    if !inlined {
        return Err(ReadError::not_ported(format!(
            "{}: FSkeletalMeshLODRenderData: a streamed LOD (buffers in bulk data) not ported",
            ar.name
        )));
    }
    read_streamed_data(ar, &mut lod, has_vertex_colors)?;
    Ok(lod)
}

/// USkeletalMesh.Deserialize after UObject's: the tail of a SkeletalMesh export. Its read must end at the export's
/// serial end; anything else is an error.
pub fn read_skeletal_mesh_tail<'a>(
    cache: &'a PackageCache,
    pkg: &'a Arc<Package>,
    index: usize,
) -> Result<SkeletalMeshTail<'a>> {
    let NativeData { obj, mut ar, end } = at_native_data(cache, pkg, index, "USkeletalMesh")?;
    let ar = &mut ar;
    if !ar.filter_editor_only() {
        return Err(ReadError::not_ported(format!("{}: USkeletalMesh of an editor package not ported", pkg.path)));
    }
    let has_vertex_colors = obj.props.get("bHasVertexColors").and_then(Value::as_bool) == Some(true);
    let strip = read_strip_flags(ar)?;
    let imported_bounds = read_box_sphere_bounds(ar)?;
    let nm = ar.i32()?;
    let nm =
        usize::try_from(nm).map_err(|_| ReadError::parse(format!("{}: SkeletalMaterials count {nm}", pkg.path)))?;
    let mut materials = Vec::new();
    for _ in 0..nm {
        materials.push(read_skeletal_material(ar)?);
    }
    let ref_skeleton = read_reference_skeleton(ar)?;
    // FSkeletalMeshCustomVersion >= SplitModelAndRenderData: the editor's LOD models (stripped when cooked), then the
    // cooked render data
    if !strip.editor_stripped() {
        return Err(ReadError::not_ported(format!("{}: USkeletalMesh with editor LOD models not ported", pkg.path)));
    }
    let cooked = ar.bool()?;
    let mut out = SkeletalMeshTail {
        props: obj.props,
        imported_bounds,
        materials,
        ref_skeleton,
        lods: Vec::new(),
        inlined_lods: 0,
        non_optional_lods: 0,
    };
    if cooked {
        let n = ar.i32()?;
        let n = usize::try_from(n).map_err(|_| ReadError::parse(format!("{}: LOD count {n}", pkg.path)))?;
        for _ in 0..n {
            out.lods.push(read_lod(ar, has_vertex_colors)?);
        }
        out.inlined_lods = ar.u8()?;
        out.non_optional_lods = ar.u8()?;
    }
    // TArray<FPackageIndex> (UE4: CUE4Parse reads past it)
    skip_fixed_array(ar, 4)?;
    // TextureStreamingMeshUVChannelData is past (FRenderingObjectVersion 44); ClothingAssets: a cooked package with
    // FSkeletalMeshCustomVersion past NewClothingSystemAdded serializes none
    ends_at(ar, end, "the USkeletalMesh tail")?;
    Ok(out)
}

/// UMorphTarget.Deserialize for 4.26 (FEditorObjectVersion >= AddedMorphTargetSectionIndices, FFortniteMainBranch
/// past SaveGeneratedMorphTargetByEngine and before MorphTargetCustomImport / MorphTargetCookedCPUDataCompressed):
/// its LOD models.
pub fn read_morph_target<'a>(
    cache: &'a PackageCache,
    pkg: &'a Arc<Package>,
    index: usize,
) -> Result<Vec<MorphTargetLod<'a>>> {
    let NativeData { mut ar, end, .. } = at_native_data(cache, pkg, index, "UMorphTarget")?;
    let ar = &mut ar;
    let fortnite = ar.custom(versions::FORTNITE_MAIN);
    if ar.custom(versions::EDITOR) < 23 || !(6..158).contains(&fortnite) {
        return Err(ReadError::not_ported(format!("{}: UMorphTarget of this engine version not ported", pkg.path)));
    }
    let strip = read_strip_flags(ar)?;
    if strip.av_stripped() {
        ends_at(ar, end, "the UMorphTarget tail")?;
        return Ok(Vec::new());
    }
    let n = ar.i32()?;
    let n = usize::try_from(n).map_err(|_| ReadError::parse(format!("{}: morph LOD count {n}", pkg.path)))?;
    let mut lods = Vec::new();
    for _ in 0..n {
        let c = ar.i32()?;
        let c = usize::try_from(c).map_err(|_| ReadError::parse(format!("{}: morph delta count {c}", pkg.path)))?;
        let deltas = ar.bytes(c * 28)?;
        let num_base_mesh_verts = ar.i32()?;
        let ns = ar.i32()?;
        let ns =
            usize::try_from(ns).map_err(|_| ReadError::parse(format!("{}: morph section count {ns}", pkg.path)))?;
        let mut section_indices = Vec::new();
        for _ in 0..ns {
            section_indices.push(ar.i32()?);
        }
        let generated_by_engine = ar.bool()?;
        lods.push(MorphTargetLod { count: c, deltas, num_base_mesh_verts, section_indices, generated_by_engine });
    }
    ends_at(ar, end, "the UMorphTarget tail")?;
    Ok(lods)
}
