//! The `mesh` kind: every top-level StaticMesh and SkeletalMesh export of the build (/Game and /Engine) as an MVM1
//! container: the header as JSON (a skeletal mesh's `skeleton` written by
//! `super::skeleton::skeleton_entry`), the vertex streams and indices as its body. The readers are
//! `mvs_uasset::natives::{mesh_buffers, static_mesh, skeletal_mesh}`.
//!
//! An entry holds the build's data as cooked: UE space, the cooked vertex and index order and winding, every LOD.
//! Per LOD: `position` f32x3, `tangentX` / `tangentZ` snorm8x4 (the file's bytes: TangentZ.w is the binormal sign),
//! `uv0`..`uvN-1` f32x2 or f16x2 as the LOD stores them, `color` unorm8x4 R, G, B, A (FColor's B, G, R, A bytes
//! reordered: the one change of order made at import) where the LOD has a colour buffer, the index u16 or u32 as
//! stored; sections from FStaticMeshSection (count = NumTriangles x 3). `slots` from StaticMaterials (slot name, the
//! material's object path), `lightmapUV` LightMapCoordinateIndex, `bounds` ExtendedBounds where the tagged properties
//! hold it, else the render data's bounds (every static mesh of the build holds it; it differs from the render data's
//! on one, C017's Junkyard_Car1). `lightmapUV` is 0 where the tag is absent (93 meshes): no mesh of the build tags 0
//! (1,184 tag 1, 2 or 3), so 0 is the class default the tagged form leaves out.
//!
//! A skeletal mesh's LOD has the same streams (`tangentX` / `tangentZ` snorm16x4 on the one LOD whose buffer has
//! bUseHighPrecisionTangentBasis, Batman_Cape1_SkelMesh: FPackedRGBA16N's int16 words as stored), then `joints0` /
//! `weights0` and, where the skin weight buffer holds 8 influences a vertex, `joints1` / `weights1`: each vertex's
//! influences in the buffer's order (unused: joint 0, weight 0), the weights the stored u8, the bone indices remapped
//! from the vertex's section's BoneMap to the mesh's own reference skeleton (u8x4 for 256 bones or fewer, else u16x4).
//! Sections from FSkelMeshSection: `slot` its MaterialIndex (47 meshes hold a LODInfo LODMaterialMap; none names a
//! valid slot other than the section's own, so the cooked index is the slot the build draws), `first` BaseIndex,
//! `count` NumTriangles x 3, `minVertex` / `maxVertex` its BaseVertexIndex range, `maxInfluences`, `castShadow`,
//! `disabled`, `cloth` (ClothMappingData not empty; the clothing simulation is not read). `screenSize` the LODInfo's
//! ScreenSize, `bounds` ImportedBounds, `slots` SkeletalMaterials, `skeleton` the mesh's FReferenceSkeleton through
//! `skeleton_entry` (the stored pose, no AdjustBoneScales), `morphs` the UMorphTarget exports its MorphTargets
//! property names, per LOD the sparse deltas split into vertex index u32, position delta f32x3 and TangentZ delta
//! f32x3, as stored. Not carried: the cloth mapping and clothing assets, skin weight profiles (none in the build), the
//! duplicated-vertex and adjacency buffers, RequiredBones / ActiveBoneIndices, the section's bRecomputeTangent and
//! colour mask channel, the PositiveBoundsExtension / NegativeBoundsExtension (object kind).
//!
//! Every export's read ends at its serial end (the readers check it).

use std::borrow::Cow;
use std::sync::Arc;

use indexmap::IndexMap;
use mvs_uasset::natives::mesh_buffers::{BoxSphereBounds, color_rgba, tangent_streams, uv_stream};
use mvs_uasset::natives::skeletal_mesh::{SkelMeshLod, read_morph_target, read_skeletal_mesh_tail};
use mvs_uasset::natives::static_mesh::read_static_mesh_tail;
use mvs_uasset::typed::f32_dotnet;
use mvs_uasset::value::{Props, Value};
use mvs_uasset::{Package, PackageCache, Resolved};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

use super::skeleton::{SkeletonEntry, skeleton_entry};
use crate::json::Json;
use crate::jsonbin::{BodyRef, json_bin, lay_out_body};
use crate::kind::{Cx, Item, Kind, KindError, Memo};

/// The kind's version.
pub const MESH_KIND_VERSION: u64 = 2;
/// The container's magic.
pub const MESH_MAGIC: &[u8; 4] = b"MVM1";

const STATIC_MESH: &str = "/Script/Engine.StaticMesh";
const SKELETAL_MESH: &str = "/Script/Engine.SkeletalMesh";

/// A mesh's bounds.
#[derive(Serialize)]
struct Bounds {
    origin: [f64; 3],
    extent: [f64; 3],
    radius: f64,
}

/// One material slot: its name and the material's object path.
#[derive(Serialize)]
struct Slot {
    name: String,
    material: Option<String>,
}

/// A stream of a LOD: its section of the body and its element format.
#[derive(Serialize)]
struct Stream {
    offset: usize,
    length: usize,
    format: &'static str,
}

/// A LOD's streams in the order they were added (JSON.stringify prints an object's keys in insertion order).
struct Streams(Vec<(String, Stream)>);

impl Serialize for Streams {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

#[derive(Serialize)]
struct IndexRef {
    offset: usize,
    length: usize,
    format: &'static str,
    count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Section {
    slot: i32,
    first: i32,
    count: i64,
    min_vertex: i32,
    max_vertex: i32,
    max_influences: i32,
    cast_shadow: bool,
    disabled: bool,
    cloth: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Lod {
    screen_size: f64,
    vertices: i32,
    sections: Vec<Section>,
    streams: Streams,
    index: IndexRef,
}

#[derive(Serialize)]
struct MorphLod {
    count: usize,
    vertex: BodyRef,
    position: BodyRef,
    #[serde(rename = "tangentZ")]
    tangent_z: BodyRef,
}

#[derive(Serialize)]
struct Morph {
    name: String,
    lods: Vec<MorphLod>,
}

/// A mesh entry's header.
#[derive(Serialize)]
struct Header {
    path: String,
    class: &'static str,
    bounds: Bounds,
    slots: Vec<Slot>,
    #[serde(rename = "lightmapUV", skip_serializing_if = "Option::is_none")]
    lightmap_uv: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skeleton: Option<SkeletonEntry>,
    lods: Vec<Lod>,
    #[serde(skip_serializing_if = "Option::is_none")]
    morphs: Option<Vec<Morph>>,
}

/// A morph LOD's delta count and its three parts (vertex, position, TangentZ).
type MorphParts = (usize, usize, usize, usize);

/// A LOD before the body is laid out: its streams name the parts by index.
struct LodPlan {
    screen_size: f64,
    vertices: i32,
    sections: Vec<Section>,
    streams: Vec<(String, &'static str, usize)>,
    index: (usize, &'static str, usize),
}

impl LodPlan {
    fn finish(self, refs: &[BodyRef]) -> Lod {
        let streams = self
            .streams
            .into_iter()
            .map(|(name, format, at)| (name, Stream { offset: refs[at].offset, length: refs[at].length, format }))
            .collect();
        let (at, format, count) = self.index;
        Lod {
            screen_size: self.screen_size,
            vertices: self.vertices,
            sections: self.sections,
            streams: Streams(streams),
            index: IndexRef { offset: refs[at].offset, length: refs[at].length, format, count },
        }
    }
}

/// The body's sections as they are added: a stream's bytes by index.
#[derive(Default)]
struct Parts<'a>(Vec<Cow<'a, [u8]>>);

impl<'a> Parts<'a> {
    fn add(&mut self, bytes: impl Into<Cow<'a, [u8]>>) -> usize {
        self.0.push(bytes.into());
        self.0.len() - 1
    }
}

fn fail<T>(m: String) -> Result<T, KindError> {
    Err(KindError::Failed(m))
}

/// A tagged number as JS `Number(v)` reads it (absent: 0).
fn num(v: Option<&Value>) -> f64 {
    match v {
        None | Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Int(i)) => *i as f64,
        Some(Value::UInt(u)) => *u as f64,
        Some(Value::Float(f)) => f32_dotnet(*f),
        Some(Value::Double(d)) => *d,
        Some(Value::Str(s)) => s.trim().parse().unwrap_or(if s.trim().is_empty() { 0.0 } else { f64::NAN }),
        Some(_) => f64::NAN,
    }
}

/// A tagged FVector or struct member: absent members are the struct's zero default (a tagged struct holds only what
/// differs from it).
fn vec3(v: Option<&Value>) -> [f64; 3] {
    let p = v.and_then(Value::as_struct);
    let at = |k: &str| num(p.and_then(|p| p.get(k)));
    [at("X"), at("Y"), at("Z")]
}

fn bounds_of(b: &BoxSphereBounds) -> Bounds {
    Bounds { origin: b.origin.map(f32_dotnet), extent: b.extent.map(f32_dotnet), radius: f32_dotnet(b.radius) }
}

/// The material path of a package index, none for none.
fn material_path(cache: &PackageCache, pkg: &Arc<Package>, index: i32) -> Option<String> {
    cache.resolve(pkg, index).map(|r| r.object_path(cache))
}

/// One static mesh's entry (`staticMeshEntry`).
fn static_mesh_entry(cache: &PackageCache, pkg: &Arc<Package>, top: usize) -> Result<Vec<u8>, KindError> {
    let tail = read_static_mesh_tail(cache, pkg, top)?;
    let Some(rd) = &tail.render_data else {
        return fail(format!("{}: StaticMesh {} holds no cooked render data", pkg.path, pkg.exports[top].object_name));
    };
    let path = Resolved::Export { pkg: pkg.clone(), index: top }.object_path(cache);

    let slots: Vec<Slot> = if let Some(m) = &tail.static_materials {
        m.iter().map(|m| Slot { name: m.slot_name.clone(), material: material_path(cache, pkg, m.material) }).collect()
    } else {
        let list = match tail.props.get("StaticMaterials") {
            Some(Value::Array(a)) => a.as_slice(),
            _ => &[],
        };
        let mut slots = Vec::new();
        for m in list {
            let Some(m) = m.as_struct() else {
                return fail(format!("{path}: a StaticMaterials entry is not a struct"));
            };
            let name = m.get("MaterialSlotName").and_then(Value::as_str).unwrap_or("None").to_owned();
            slots.push(Slot { name, material: plain_path(cache, &path, m.get("MaterialInterface"))? });
        }
        slots
    };

    let bounds = match tail.props.get("ExtendedBounds").and_then(Value::as_struct) {
        Some(e) => Bounds {
            origin: vec3(e.get("Origin")),
            extent: vec3(e.get("BoxExtent")),
            radius: num(e.get("SphereRadius")),
        },
        None => bounds_of(&rd.bounds),
    };

    let mut parts = Parts::default();
    let mut lods = Vec::new();
    for (li, l) in rd.lods.iter().enumerate() {
        let (Some(pos), Some(vb), Some(index)) = (&l.position, &l.vertex, &l.index) else {
            return fail(format!("{path}: LOD {li} holds no buffers (cooked out or stripped)"));
        };
        if l.cooked_out {
            return fail(format!("{path}: LOD {li} holds no buffers (cooked out or stripped)"));
        }
        let n = pos.vertices;
        if vb.vertices != n {
            return fail(format!("{path}: LOD {li}: {n} positions, {} tangent / UV vertices", vb.vertices));
        }
        let mut streams = Vec::new();
        add_common(&mut streams, &mut parts, &path, li, l.position.as_ref(), vb, l.color.as_ref())?;
        let index = (parts.add(index.data), if index.is32 { "u32" } else { "u16" }, index.count);
        lods.push(LodPlan {
            screen_size: f32_dotnet(rd.screen_size.get(li).copied().unwrap_or(0.0)),
            vertices: n,
            sections: l
                .sections
                .iter()
                .map(|s| Section {
                    slot: s.material_index,
                    first: s.first_index,
                    count: i64::from(s.num_triangles) * 3,
                    min_vertex: s.min_vertex_index,
                    max_vertex: s.max_vertex_index,
                    max_influences: 0,
                    cast_shadow: s.cast_shadow,
                    disabled: false,
                    cloth: false,
                })
                .collect(),
            streams,
            index,
        });
    }

    let (refs, _) = lay_out_body(&parts.0);
    let header = Header {
        path,
        class: "StaticMesh",
        bounds,
        slots,
        lightmap_uv: Some(num(tail.props.get("LightMapCoordinateIndex"))),
        skeleton: None,
        lods: lods.into_iter().map(|l| l.finish(&refs)).collect(),
        morphs: None,
    };
    Ok(json_bin(MESH_MAGIC, &header, &parts.0, &refs)?)
}

/// plainValue of a material interface member: an object path, a soft path, or none.
fn plain_path(cache: &PackageCache, path: &str, v: Option<&Value>) -> Result<Option<String>, KindError> {
    match v {
        None | Some(Value::Null | Value::Ref(None)) => Ok(None),
        Some(Value::Ref(Some(r))) => Ok(Some(r.object_path(cache))),
        Some(Value::SoftPath { asset, sub }) => Ok(if asset == "None" || asset.is_empty() {
            None
        } else if sub.is_empty() {
            Some(asset.clone())
        } else {
            Some(format!("{asset}:{sub}"))
        }),
        Some(Value::Str(s)) => Ok(Some(s.clone())),
        Some(_) => fail(format!("{path}: a material interface that is no object path")),
    }
}

/// The streams a static and a skeletal LOD share: position, the tangents, the UV channels, the colour (a stream is
/// its name, its element format and the part holding it).
fn add_common<'a>(
    streams: &mut Vec<(String, &'static str, usize)>,
    parts: &mut Parts<'a>,
    path: &str,
    li: usize,
    position: Option<&mvs_uasset::natives::mesh_buffers::PositionBuffer<'a>>,
    vb: &mvs_uasset::natives::mesh_buffers::StaticVertexBuffer<'a>,
    color: Option<&mvs_uasset::natives::mesh_buffers::ColorBuffer<'a>>,
) -> Result<(), KindError> {
    let (Some(position), n) = (position, vb.vertices) else { unreachable!("the caller checked the position buffer") };
    streams.push(("position".into(), "f32x3", parts.add(position.data)));
    let t = tangent_streams(vb)?;
    streams.push(("tangentX".into(), t.format, parts.add(t.tangent_x)));
    streams.push(("tangentZ".into(), t.format, parts.add(t.tangent_z)));
    for c in 0..usize::try_from(vb.tex_coords).unwrap_or(0) {
        let format = if vb.full_precision_uvs { "f32x2" } else { "f16x2" };
        streams.push((format!("uv{c}"), format, parts.add(uv_stream(vb, c)?)));
    }
    if let Some(color) = color.filter(|c| !c.data.is_empty()) {
        if color.vertices != n {
            return fail(format!("{path}: LOD {li}: {n} vertices, {} colours", color.vertices));
        }
        streams.push(("color".into(), "unorm8x4", parts.add(color_rgba(color.data)?)));
    }
    Ok(())
}

/// A skeletal LOD's joint and weight streams: each vertex's influences in the buffer's order, the bone indices
/// remapped from its section's BoneMap to the mesh's reference skeleton, 4 a stream (`joints1` / `weights1` when the
/// buffer holds more than 4 a vertex); unused slots are joint 0, weight 0.
struct Skin {
    format: &'static str,
    joints: Vec<Vec<u8>>,
    weights: Vec<Vec<u8>>,
}

fn skin_streams(path: &str, li: usize, l: &SkelMeshLod<'_>, bones: usize) -> Result<Skin, KindError> {
    let w = l.weights.as_ref().expect("the caller checked the skin weights");
    let n = l.position.as_ref().expect("the caller checked the positions").vertices;
    if w.vertices != n {
        return fail(format!("{path}: LOD {li}: {n} vertices, {} skin weights", w.vertices));
    }
    if w.max_bone_influences > 8 {
        return Err(KindError::NotPorted(format!(
            "{path}: LOD {li}: {} influences a vertex (the format holds 8)",
            w.max_bone_influences
        )));
    }
    let n = usize::try_from(n).map_err(|_| KindError::Failed(format!("{path}: LOD {li}: {n} vertices")))?;
    let max = usize::try_from(w.max_bone_influences).unwrap_or(0);
    let streams = if max > 4 { 2 } else { 1 };
    let wide = bones > 256;
    let jw = if wide { 2 } else { 1 };
    let mut joints = vec![vec![0u8; n * 4 * jw]; streams];
    let mut weights = vec![vec![0u8; n * 4]; streams];
    let isz = if w.use16_bit_bone_index { 2 } else { 1 };
    let per = max * (isz + 1);
    let d = w.data;
    let mut covered = 0i64;
    for (si, s) in l.sections.iter().enumerate() {
        if i64::from(s.base_vertex_index) != covered {
            return fail(format!(
                "{path}: LOD {li}: section {si} starts at vertex {}, {covered} expected",
                s.base_vertex_index
            ));
        }
        let end = i64::from(s.base_vertex_index) + i64::from(s.num_vertices);
        if end > n as i64 {
            return fail(format!("{path}: LOD {li}: section {si} ends at vertex {end} of {n}"));
        }
        for v in usize::try_from(s.base_vertex_index).unwrap_or(0)..usize::try_from(end).unwrap_or(0) {
            let (mut off, mut cnt) = (v * per, max);
            if let Some(lookup) = &w.lookup {
                let e = lookup[v];
                off = (e >> 8) as usize;
                cnt = (e & 0xff) as usize;
                if cnt > streams * 4 {
                    return fail(format!(
                        "{path}: LOD {li}: vertex {v} has {cnt} influences, the buffer {}",
                        w.max_bone_influences
                    ));
                }
            }
            for k in 0..cnt {
                let idx = if isz == 2 {
                    match d.get(off + 2 * k..off + 2 * k + 2) {
                        Some(b) => usize::from(u16::from_le_bytes([b[0], b[1]])),
                        None => {
                            return fail(format!("{path}: LOD {li}: vertex {v}: a bone index past the weight data"));
                        }
                    }
                } else {
                    usize::from(d.get(off + k).copied().unwrap_or(0))
                };
                let wt = d.get(off + cnt * isz + k).copied().unwrap_or(0);
                let bone = match s.bone_map.get(idx) {
                    Some(&b) => b,
                    None if wt != 0 => {
                        return fail(format!(
                            "{path}: LOD {li}: vertex {v} names section bone {idx}, section {si}'s BoneMap has {}",
                            s.bone_map.len()
                        ));
                    }
                    None => 0,
                };
                if usize::try_from(bone).unwrap_or(usize::MAX) >= bones {
                    return fail(format!("{path}: LOD {li}: section {si} maps to bone {bone} of {bones}"));
                }
                let at = v * 4 + (k & 3);
                let j = &mut joints[k >> 2];
                if wide {
                    // (bone < bones <= 65535 for a u16 stream: the stored index space is 16 bits)
                    j[at * 2..at * 2 + 2].copy_from_slice(&(bone as u16).to_le_bytes());
                } else {
                    j[at] = bone as u8;
                }
                weights[k >> 2][at] = wt;
            }
        }
        covered = end;
    }
    if covered != n as i64 {
        return fail(format!("{path}: LOD {li}: the sections cover {covered} of {n} vertices"));
    }
    Ok(Skin { format: if wide { "u16x4" } else { "u8x4" }, joints, weights })
}

/// One skeletal mesh's entry (`skeletalMeshEntry`).
fn skeletal_mesh_entry(cache: &PackageCache, pkg: &Arc<Package>, top: usize) -> Result<Vec<u8>, KindError> {
    let tail = read_skeletal_mesh_tail(cache, pkg, top)?;
    let path = Resolved::Export { pkg: pkg.clone(), index: top }.object_path(cache);
    if tail.lods.is_empty() {
        return fail(format!("{path}: SkeletalMesh {} holds no cooked render data", pkg.exports[top].object_name));
    }
    let skeleton = skeleton_entry(&path, &tail.ref_skeleton)?;
    let bones = skeleton.bones.len();
    let lod_info: &[Value] = match tail.props.get("LODInfo") {
        Some(Value::Array(a)) => a,
        _ => &[],
    };

    let mut parts = Parts::default();
    let mut lods = Vec::new();
    for (li, l) in tail.lods.iter().enumerate() {
        let (Some(pos), Some(vb), Some(index), Some(_)) = (&l.position, &l.vertex, &l.index, &l.weights) else {
            return fail(format!("{path}: LOD {li} holds no buffers (cooked out or stripped)"));
        };
        if l.cooked_out {
            return fail(format!("{path}: LOD {li} holds no buffers (cooked out or stripped)"));
        }
        let n = pos.vertices;
        if vb.vertices != n {
            return fail(format!("{path}: LOD {li}: {n} positions, {} tangent / UV vertices", vb.vertices));
        }
        let mut streams = Vec::new();
        add_common(&mut streams, &mut parts, &path, li, l.position.as_ref(), vb, l.color.as_ref())?;
        let skin = skin_streams(&path, li, l, bones)?;
        for (s, (j, w)) in skin.joints.into_iter().zip(skin.weights).enumerate() {
            streams.push((format!("joints{s}"), skin.format, parts.add(j)));
            streams.push((format!("weights{s}"), "unorm8x4", parts.add(w)));
        }
        let index = (parts.add(index.data), if index.is32 { "u32" } else { "u16" }, index.count);
        let screen = lod_info
            .get(li)
            .and_then(Value::as_struct)
            .and_then(|p: &Props| p.get("ScreenSize"))
            .and_then(Value::as_struct)
            .and_then(|p| p.get("Default"));
        lods.push(LodPlan {
            screen_size: num(screen),
            vertices: n,
            sections: l
                .sections
                .iter()
                .map(|s| Section {
                    slot: s.material_index,
                    first: s.base_index,
                    count: i64::from(s.num_triangles) * 3,
                    min_vertex: s.base_vertex_index,
                    max_vertex: s.base_vertex_index.wrapping_add(s.num_vertices).wrapping_sub(1),
                    max_influences: s.max_bone_influences,
                    cast_shadow: s.cast_shadow,
                    disabled: s.disabled,
                    cloth: s.cloth,
                })
                .collect(),
            streams,
            index,
        });
    }

    // the MorphTargets property names UMorphTarget exports of the same package, in the mesh's order
    let mut morphs: Vec<(String, Vec<MorphParts>)> = Vec::new();
    let targets: &[Value] = match tail.props.get("MorphTargets") {
        Some(Value::Array(a)) => a,
        _ => &[],
    };
    for target in targets {
        let r = match target {
            Value::Ref(Some(r)) => r.as_export().filter(|(p, _)| Arc::ptr_eq(p, pkg)),
            _ => None,
        };
        let Some((_, ri)) = r else {
            return fail(format!("{path}: a MorphTargets entry is not an export of the package ({target:?})"));
        };
        let mut lod_parts = Vec::new();
        for m in read_morph_target(cache, pkg, ri)? {
            let (mut vertex, mut position, mut tangent_z) = (Vec::new(), Vec::new(), Vec::new());
            for d in m.deltas.as_chunks::<28>().0 {
                position.extend_from_slice(&d[..12]);
                tangent_z.extend_from_slice(&d[12..24]);
                vertex.extend_from_slice(&d[24..28]);
            }
            lod_parts.push((m.count, parts.add(vertex), parts.add(position), parts.add(tangent_z)));
        }
        morphs.push((pkg.exports[ri].object_name.clone(), lod_parts));
    }

    let (refs, _) = lay_out_body(&parts.0);
    let header = Header {
        path,
        class: "SkeletalMesh",
        bounds: bounds_of(&tail.imported_bounds),
        slots: tail
            .materials
            .iter()
            .map(|m| Slot { name: m.slot_name.clone(), material: material_path(cache, pkg, m.material) })
            .collect(),
        lightmap_uv: None,
        skeleton: Some(skeleton),
        lods: lods.into_iter().map(|l| l.finish(&refs)).collect(),
        morphs: (!morphs.is_empty()).then(|| {
            morphs
                .into_iter()
                .map(|(name, lods)| Morph {
                    name,
                    lods: lods
                        .into_iter()
                        .map(|(count, v, p, t)| MorphLod {
                            count,
                            vertex: refs[v],
                            position: refs[p],
                            tangent_z: refs[t],
                        })
                        .collect(),
                })
                .collect()
        }),
    };
    Ok(json_bin(MESH_MAGIC, &header, &parts.0, &refs)?)
}

// the header as a game kind reads it

/// A mesh entry's header as the game kinds read it (parsed back, numbers as JSON.parse
/// reads them): what scenery, the fighter draws, particles and banner read of the mesh kind (`scope.load('mesh', p)`).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshHeader {
    pub path: String,
    /// `StaticMesh` or `SkeletalMesh` (the entry header's short class name).
    pub class: String,
    pub bounds: MeshBounds,
    pub slots: Vec<MeshSlot>,
    /// A static mesh's LightMapCoordinateIndex.
    #[serde(rename = "lightmapUV")]
    pub lightmap_uv: Option<f64>,
    /// A skeletal mesh's reference skeleton.
    pub skeleton: Option<MeshSkeleton>,
    pub lods: Vec<MeshLod>,
    /// A skeletal mesh's morph targets, as written.
    pub morphs: Option<Vec<Json>>,
}

impl MeshHeader {
    /// Whether this is a SkeletalMesh.
    pub fn skinned(&self) -> bool {
        self.class == "SkeletalMesh"
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshBounds {
    pub origin: [f64; 3],
    pub extent: [f64; 3],
    pub radius: f64,
}

/// A material slot: its name and the material's object path (`None` for an empty slot).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshSlot {
    pub name: String,
    pub material: Option<String>,
}

/// A skeletal mesh header's reference skeleton.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshSkeleton {
    pub path: String,
    pub bones: Vec<String>,
    pub parents: Vec<i32>,
    pub pose: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MeshLod {
    pub screen_size: f64,
    pub vertices: f64,
    pub sections: Vec<MeshSection>,
    /// The LOD's streams by name (`position`, `color`, `uv0`, ...), in the entry's order.
    pub streams: IndexMap<String, MeshStream>,
    pub index: MeshStream,
}

/// A section of the body: where, and its element format (the index's `count` beside).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshStream {
    pub offset: f64,
    pub length: f64,
    pub format: String,
    pub count: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MeshSection {
    pub slot: f64,
    pub first: f64,
    pub count: f64,
    pub min_vertex: f64,
    pub max_vertex: f64,
    pub max_influences: f64,
    pub cast_shadow: bool,
    pub disabled: bool,
    pub cloth: bool,
}

/// The run's headers, by path.
#[derive(Default)]
struct Headers(Memo<Result<Arc<MeshHeader>, KindError>>);

/// The header of the mesh entry at `path` as the kind builds it, parsed back; built once per run (the body is
/// built and dropped: the precedent of `lighting::entry_header`).
pub fn header(cx: &Cx<'_>, path: &str) -> Result<Arc<MeshHeader>, KindError> {
    cx.shared.get_or_init(Headers::default).0.get(path, || {
        let bytes = Mesh.build(cx, &Item::new(path))?;
        let what = format!("mesh {path}");
        let e = mvs_material::jsonbin::JsonBin::split(MESH_MAGIC, &bytes, &what)
            .map_err(|e| KindError::Failed(e.to_string()))?;
        e.parse::<MeshHeader>(&what).map(Arc::new).map_err(|e| KindError::Failed(e.to_string()))
    })
}

/// The `mesh` kind.
pub struct Mesh;

impl Kind for Mesh {
    fn id(&self) -> &'static str {
        "mesh"
    }

    fn version(&self) -> u64 {
        MESH_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/octet-stream"
    }

    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(cx.catalog.paths_of(&[STATIC_MESH, SKELETAL_MESH]).into_iter().map(Item::new).collect())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let path = &item.path;
        let (pkg, top) = cx.read(path)?;
        let class = cx.class_of(&pkg, top)?;
        match class.as_str() {
            STATIC_MESH => static_mesh_entry(cx.cache, &pkg, top),
            SKELETAL_MESH => skeletal_mesh_entry(cx.cache, &pkg, top),
            c => Err(KindError::NotFound(format!(
                "mesh: {path} is a {}, not a StaticMesh or a SkeletalMesh",
                if c == "(none)" { "UObject" } else { c }
            ))),
        }
    }
}
