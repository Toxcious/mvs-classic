//! UStaticMesh's native tail for the mesh kind: the cooked render data (LODs, sections, buffers), the static
//! materials, the body setup and sockets. Never entered in NATIVES: the object kind keeps the class `partial`. It
//! follows CUE4Parse 1.2.2 UStaticMesh.Deserialize, FStaticMeshRenderData, the UE 4.26 cooked branch only.

use std::sync::Arc;

use super::mesh_buffers::{
    BoxSphereBounds, ColorBuffer, IndexBuffer, PositionBuffer, StaticVertexBuffer, read_box_sphere_bounds,
    read_color_buffer, read_position_buffer, read_raw_static_index_buffer, read_static_vertex_buffer, read_strip_flags,
    skip_box, skip_bulk_array, skip_fixed_array, skip_weighted_random_sampler,
};
use super::skeleton::{NativeData, at_native_data, ends_at};
use crate::archive::{ReadError, Result};
use crate::objects::ExportArchive;
use crate::package::{Package, PackageCache};
use crate::value::Props;
use crate::versions;

#[derive(Clone, Debug, Default)]
pub struct StaticMeshSection {
    pub material_index: i32,
    pub first_index: i32,
    pub num_triangles: i32,
    pub min_vertex_index: i32,
    pub max_vertex_index: i32,
    pub enable_collision: bool,
    pub cast_shadow: bool,
    pub force_opaque: bool,
    pub visible_in_ray_tracing: bool,
}

#[derive(Clone, Debug, Default)]
pub struct StaticMeshLod<'a> {
    pub sections: Vec<StaticMeshSection>,
    pub max_deviation: f32,
    /// bIsLODCookedOut: the LOD's buffers were not cooked (none of the fields below).
    pub cooked_out: bool,
    pub position: Option<PositionBuffer<'a>>,
    pub vertex: Option<StaticVertexBuffer<'a>>,
    pub color: Option<ColorBuffer<'a>>,
    pub index: Option<IndexBuffer<'a>>,
}

#[derive(Clone, Debug, Default)]
pub struct StaticMeshRenderData<'a> {
    pub lods: Vec<StaticMeshLod<'a>>,
    /// NumInlinedLODs (UE4.23+).
    pub inlined_lods: u8,
    pub bounds: BoxSphereBounds,
    pub lods_share_static_lighting: bool,
    /// The 8 ScreenSize entries (FPerPlatformFloat Default; the cooked platform's value).
    pub screen_size: Vec<f32>,
}

/// FStaticMaterial (native in 4.26: FEditorObjectVersion >= RefactorMeshEditorMaterials).
#[derive(Clone, Debug, Default)]
pub struct StaticMaterial {
    /// FPackageIndex of the material interface (0: none).
    pub material: i32,
    pub slot_name: String,
}

#[derive(Clone, Debug, Default)]
pub struct StaticMeshTail<'a> {
    /// The tagged properties, read on the way.
    pub props: Props,
    /// FPackageIndex of the BodySetup and the NavCollision.
    pub body_setup: i32,
    pub nav_collision: i32,
    pub lighting_guid: String,
    pub sockets: Vec<i32>,
    pub render_data: Option<StaticMeshRenderData<'a>>,
    /// `None` when the build keeps StaticMaterials in the tagged properties (the bool before them set).
    pub static_materials: Option<Vec<StaticMaterial>>,
}

fn read_section(ar: &mut ExportArchive<'_>) -> Result<StaticMeshSection> {
    let material_index = ar.i32()?;
    let first_index = ar.i32()?;
    let num_triangles = ar.i32()?;
    let min_vertex_index = ar.i32()?;
    let max_vertex_index = ar.i32()?;
    let enable_collision = ar.bool()?;
    let cast_shadow = ar.bool()?;
    // StaticMeshSectionForceOpaqueField
    let force_opaque = if ar.custom(versions::RENDERING) >= 37 { ar.bool()? } else { false };
    let visible_in_ray_tracing = ar.bool()?; // StaticMesh.HasVisibleInRayTracing (UE4.26+)
    Ok(StaticMeshSection {
        material_index,
        first_index,
        num_triangles,
        min_vertex_index,
        max_vertex_index,
        enable_collision,
        cast_shadow,
        force_opaque,
        visible_in_ray_tracing,
    })
}

/// FStaticMeshLODResources.SerializeBuffers: the render buffers, the extra index buffers and the samplers read past.
fn read_buffers<'a>(ar: &mut ExportArchive<'a>, lod: &mut StaticMeshLod<'a>) -> Result<()> {
    let strip = read_strip_flags(ar)?;
    lod.position = Some(read_position_buffer(ar)?);
    lod.vertex = Some(read_static_vertex_buffer(ar)?);
    lod.color = Some(read_color_buffer(ar)?);
    lod.index = Some(read_raw_static_index_buffer(ar)?);
    if !strip.class_stripped(4) {
        read_raw_static_index_buffer(ar)?; // ReversedIndexBuffer
    }
    read_raw_static_index_buffer(ar)?; // DepthOnlyIndexBuffer
    if !strip.class_stripped(4) {
        read_raw_static_index_buffer(ar)?; // ReversedDepthOnlyIndexBuffer
    }
    if !strip.editor_stripped() {
        read_raw_static_index_buffer(ar)?; // WireframeIndexBuffer
    }
    if !strip.class_stripped(1) {
        read_raw_static_index_buffer(ar)?; // AdjacencyIndexBuffer (before RemovingTessellation)
    }
    if !strip.class_stripped(8) {
        skip_bulk_array(ar)?; // ray-tracing geometry (StaticMesh.HasRayTracingGeometry, 4.25+)
    }
    for _ in 0..lod.sections.len() {
        skip_weighted_random_sampler(ar)?; // per-section area samplers
    }
    skip_weighted_random_sampler(ar) // AreaWeightedSampler
}

fn read_lod<'a>(ar: &mut ExportArchive<'a>) -> Result<StaticMeshLod<'a>> {
    let strip = read_strip_flags(ar)?;
    let n = ar.i32()?;
    let mut sections = Vec::new();
    for _ in 0..n {
        sections.push(read_section(ar)?);
    }
    let max_deviation = ar.f32()?;
    // StaticMesh.UseNewCookedFormat (4.23+)
    let cooked_out = ar.bool()?;
    let inlined = ar.bool()?;
    let mut lod = StaticMeshLod { sections, max_deviation, cooked_out, ..Default::default() };
    if strip.av_stripped() || cooked_out {
        return Ok(lod);
    }
    if !inlined {
        return Err(ReadError::not_ported("FStaticMeshLODResources: a streamed LOD (buffers in bulk data) not ported"));
    }
    read_buffers(ar, &mut lod)?;
    ar.skip(12)?; // FStaticMeshBuffersSize: SerializedBuffersSize, DepthOnlyIBSize, ReversedIBsSize
    Ok(lod)
}

/// FDistanceFieldVolumeData (4.16+): read past.
fn skip_distance_field(ar: &mut ExportArchive<'_>) -> Result<()> {
    skip_fixed_array(ar, 1)?; // CompressedDistanceFieldVolume
    ar.skip(12)?; // Size
    skip_box(ar)?; // LocalBoundingBox
    ar.skip(8)?; // DistanceMinMax
    ar.bool()?; // bMeshWasClosed
    ar.bool()?; // bBuiltAsIfTwoSided
    ar.bool()?; // bMeshWasPlane
    Ok(())
}

/// TPerPlatformProperty<float>: bCooked, Default, the per-platform map when not cooked.
fn read_per_platform_float(ar: &mut ExportArchive<'_>) -> Result<f32> {
    let cooked = ar.bool()?;
    let v = ar.f32()?;
    if !cooked && ar.filter_editor_only() {
        let n = ar.i32()?;
        for _ in 0..n {
            ar.fname()?;
            ar.f32()?;
        }
    }
    Ok(v)
}

/// FStaticMeshRenderData (cooked): the LODs with their buffers, the bounds, the screen sizes.
pub fn read_static_mesh_render_data<'a>(ar: &mut ExportArchive<'a>) -> Result<StaticMeshRenderData<'a>> {
    let n = ar.i32()?;
    let mut lods = Vec::new();
    for _ in 0..n {
        lods.push(read_lod(ar)?);
    }
    let inlined_lods = ar.u8()?;
    let strip = read_strip_flags(ar)?;
    if !strip.av_stripped() && !strip.class_stripped(1) {
        for _ in 0..lods.len() {
            if ar.bool()? {
                skip_distance_field(ar)?;
            }
        }
    }
    let bounds = read_box_sphere_bounds(ar)?;
    let lods_share_static_lighting = ar.bool()?;
    let mut screen_size = Vec::with_capacity(8);
    for _ in 0..8 {
        screen_size.push(read_per_platform_float(ar)?);
    }
    Ok(StaticMeshRenderData { lods, inlined_lods, bounds, lods_share_static_lighting, screen_size })
}

/// UStaticMesh.Deserialize after UObject's (tagged properties, object GUID): the tail of a StaticMesh export, read
/// to the export's end. Its read must end at the export's serial end; anything else is an error.
pub fn read_static_mesh_tail<'a>(
    cache: &'a PackageCache,
    pkg: &'a Arc<Package>,
    index: usize,
) -> Result<StaticMeshTail<'a>> {
    let NativeData { obj, mut ar, end } = at_native_data(cache, pkg, index, "UStaticMesh")?;
    let ar = &mut ar;
    let strip = read_strip_flags(ar)?;
    let cooked = ar.bool()?; // STATIC_MESH_REFACTOR
    let body_setup = ar.i32()?;
    let nav_collision = ar.i32()?; // StaticMesh.HasNavCollision
    if !strip.editor_stripped() && ar.custom(versions::RENDERING) < 49 {
        // DeprecatedHighResSourceMesh: HighResSourceMeshName, HighResSourceMeshCRC
        ar.fstring()?;
        ar.u32()?;
    }
    let lighting_guid = ar.guid()?.to_string();
    let ns = ar.i32()?;
    let mut sockets = Vec::new();
    for _ in 0..ns {
        sockets.push(ar.i32()?);
    }
    let mut out = StaticMeshTail {
        props: obj.props,
        body_setup,
        nav_collision,
        lighting_guid,
        sockets,
        render_data: None,
        static_materials: None,
    };
    if !ar.filter_editor_only() {
        return Err(ReadError::not_ported("UStaticMesh of an editor package not ported"));
    }
    if cooked {
        out.render_data = Some(read_static_mesh_render_data(ar)?);
    }
    if cooked && ar.bool()? {
        // bHasSpeedTreeWind (4.20 to 4.27): CUE4Parse reads past two arrays
        skip_fixed_array(ar, 12)?;
        skip_fixed_array(ar, 2)?;
    }
    if ar.bool()? {
        // the StaticMaterials are in the tagged properties
        ar.pos = end;
        return Ok(out);
    }
    if ar.custom(versions::EDITOR) >= 8 {
        // RefactorMeshEditorMaterials: FStaticMaterial[]
        let nm = ar.i32()?;
        let mut mats = Vec::new();
        for _ in 0..nm {
            let material = ar.i32()?;
            let slot_name = ar.fname()?;
            if ar.custom(versions::RENDERING) >= 10 {
                ar.skip(4 + 4 + 16)?; // FMeshUVChannelInfo: bInitialized, bOverrideDensities, LocalUVDensities[4]
            }
            if ar.custom(versions::FORTNITE_MAIN) >= 196 {
                ar.i32()?; // OverlayMaterialInterface
            }
            mats.push(StaticMaterial { material, slot_name });
        }
        out.static_materials = Some(mats);
    }
    ends_at(ar, end, &format!("UStaticMesh {}", pkg.exports[index].object_name))?;
    Ok(out)
}
