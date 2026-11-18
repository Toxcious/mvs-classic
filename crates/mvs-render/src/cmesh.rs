//! The effects' component meshes (componentMeshOwn, removeComponentMesh,
//! componentGeometry): a mesh component a class's graph adds, drawn with a component material of the build (a
//! `<material>#local` or `#spline` key, FLocalVertexFactory's shaders) over a static mesh of the store at a world
//! transform, or over geometry of its own laid on the CPU (a cable's tube, spline mesh components). The effects' hooks
//! add, change and remove them here; the renderer draws the shown ones each frame (`Renderer::set_component_meshes`).
//!
//! A masked or opaque one draws in the base pass. A translucent one draws in the translucent pass after the other
//! translucent meshes of its TranslucencySortPriority and before the particles (render order 999), the
//! component meshes among themselves by the priority, then the view depth of their bounds' origin, far first; one of a
//! priority above 0 draws after the particles, with the translucent meshes of such a priority (the blast border's
//! frame).
// Deviation: the build sorts a translucent component mesh with the particles (priority first, then the view depth of
// its bounds' origin: UpdateTranslucentMeshSortKeys 141b2abf0); the renderer draws the particles as a group of their
// own, so a mesh keeps its place, after the translucent meshes and before every particle. A mesh may be sorted
// given a priority with the particle draws (no caller gives one); here it takes the order above.
// Deviation: a `#spline` material draws with FLocalVertexFactory's shaders over vertices laid on the CPU (a
// SplineMeshes), not FSplineMeshVertexFactory's over the source mesh.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use mvs_view::meshes::SplineMesh;

use crate::{Error, Result};

/// The next mesh's generation: one count for the program, so a slot of the next match's set never takes the
/// generation a mesh of an earlier match had (the renderer keeps a slot's material and buffers by it).
static NEXT_AGE: AtomicU32 = AtomicU32::new(0);

/// A component mesh's handle: its slot and the mesh's generation (a removed mesh's handle finds nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CMeshId {
    pub(crate) slot: u32,
    pub(crate) age: u32,
}

/// What a component mesh draws.
#[derive(Debug, PartialEq)]
pub enum CMeshGeometry {
    /// A static mesh of the store (its object path; its one section) at a component's LocalToWorld (FMatrix3x4, as
    /// `MeshPose::to_world`).
    Static { mesh: Arc<str>, to_world: [f32; 12] },
    /// Geometry of its own in UE world space.
    Own(OwnGeometry),
}

// (clone_from in the target's room: the renderer copies the shown meshes each frame)
impl Clone for CMeshGeometry {
    fn clone(&self) -> CMeshGeometry {
        match self {
            CMeshGeometry::Static { mesh, to_world } => {
                CMeshGeometry::Static { mesh: mesh.clone(), to_world: *to_world }
            }
            CMeshGeometry::Own(g) => CMeshGeometry::Own(g.clone()),
        }
    }

    fn clone_from(&mut self, o: &CMeshGeometry) {
        match (self, o) {
            (CMeshGeometry::Own(a), CMeshGeometry::Own(b)) => a.clone_from(b),
            (a, b) => *a = b.clone(),
        }
    }
}

impl CMeshGeometry {
    /// Own geometry of positions, UV0 and triangles only (white, zero tangents).
    pub fn own(positions: Vec<[f32; 3]>, uvs: Vec<[f32; 2]>, indices: Vec<u32>) -> CMeshGeometry {
        CMeshGeometry::Own(OwnGeometry { positions, uvs, indices, ..OwnGeometry::default() })
    }

    /// Own geometry of no triangles (laid at a later place).
    pub fn empty() -> CMeshGeometry {
        CMeshGeometry::Own(OwnGeometry::default())
    }
}

/// A component mesh's own geometry (FLocalVertexFactory's streams), UE world space (cm).
#[derive(Debug, Default, PartialEq)]
pub struct OwnGeometry {
    pub positions: Vec<[f32; 3]>,
    /// UV0 per vertex.
    pub uvs: Vec<[f32; 2]>,
    /// The triangles in the cooked order.
    pub indices: Vec<u32>,
    /// Per vertex TangentX and TangentZ (the normal, its W the binormal sign); empty: zero tangents.
    pub tangent_x: Vec<[f32; 3]>,
    pub tangent_z: Vec<[f32; 4]>,
    /// Per vertex the colour stream; empty: every vertex white (ColorIndexMask 0).
    pub colors: Vec<[u8; 4]>,
}

impl Clone for OwnGeometry {
    fn clone(&self) -> OwnGeometry {
        let mut g = OwnGeometry::default();
        g.clone_from(self);
        g
    }

    fn clone_from(&mut self, o: &OwnGeometry) {
        self.positions.clone_from(&o.positions);
        self.uvs.clone_from(&o.uvs);
        self.indices.clone_from(&o.indices);
        self.tangent_x.clone_from(&o.tangent_x);
        self.tangent_z.clone_from(&o.tangent_z);
        self.colors.clone_from(&o.colors);
    }
}

/// One component mesh.
#[derive(Debug, PartialEq)]
pub struct ComponentMesh {
    /// The material's object path in the material kind.
    pub material: Arc<str>,
    /// Its dynamic instance's scalar parameters.
    pub scalars: Vec<(Arc<str>, f32)>,
    pub geometry: CMeshGeometry,
    /// SetVisibility: a hidden one is not drawn.
    pub shown: bool,
    /// The component's TranslucencySortPriority (None: the template's 0).
    pub sort_priority: Option<i32>,
    /// Sorted by depth among the translucent meshes (a mesh added to the scene on its own, as Jake's aim arc),
    /// not after them as the effect component meshes (render order 999).
    pub by_depth: bool,
}

impl Clone for ComponentMesh {
    fn clone(&self) -> ComponentMesh {
        ComponentMesh {
            material: self.material.clone(),
            scalars: self.scalars.clone(),
            geometry: self.geometry.clone(),
            shown: self.shown,
            sort_priority: self.sort_priority,
            by_depth: self.by_depth,
        }
    }

    fn clone_from(&mut self, o: &ComponentMesh) {
        self.material.clone_from(&o.material);
        self.scalars.clone_from(&o.scalars);
        self.geometry.clone_from(&o.geometry);
        (self.shown, self.sort_priority, self.by_depth) = (o.shown, o.sort_priority, o.by_depth);
    }
}

impl ComponentMesh {
    /// Sets a scalar parameter of its instance.
    pub fn set_scalar(&mut self, name: &str, v: f64) {
        match self.scalars.iter_mut().find(|(n, _)| &**n == name) {
            Some(s) => s.1 = v as f32,
            None => self.scalars.push((Arc::from(name), v as f32)),
        }
    }
}

/// The match's component meshes, by handle.
#[derive(Debug, Default)]
pub struct ComponentMeshes {
    slots: Vec<(u32, Option<ComponentMesh>)>,
    free: Vec<u32>,
}

impl ComponentMeshes {
    /// Adds a shown mesh: `key` the component material's key (`<package path under /Game>#local` or `#spline`, as the
    /// build's), `scalars` its instance's parameters.
    pub fn add(&mut self, key: &str, scalars: &[(&str, f64)], geometry: CMeshGeometry) -> CMeshId {
        self.add_material(&component_material_path(key), scalars, geometry)
    }

    /// Adds a shown mesh drawn with the material of object path `material` (a character material of the draws, its
    /// vertex factory the renderer's for a static mesh: FLocalVertexFactory).
    pub fn add_material(&mut self, material: &str, scalars: &[(&str, f64)], geometry: CMeshGeometry) -> CMeshId {
        let m = ComponentMesh {
            material: Arc::from(material),
            scalars: scalars.iter().map(|(n, v)| (Arc::from(*n), *v as f32)).collect(),
            geometry,
            shown: true,
            sort_priority: None,
            by_depth: false,
        };
        let age = NEXT_AGE.fetch_add(1, Ordering::Relaxed);
        match self.free.pop() {
            Some(slot) => {
                self.slots[slot as usize] = (age, Some(m));
                CMeshId { slot, age }
            }
            None => {
                self.slots.push((age, Some(m)));
                CMeshId { slot: self.slots.len() as u32 - 1, age }
            }
        }
    }

    pub fn get_mut(&mut self, id: CMeshId) -> Option<&mut ComponentMesh> {
        match self.slots.get_mut(id.slot as usize) {
            Some((g, Some(m))) if *g == id.age => Some(m),
            _ => None,
        }
    }

    /// removeComponentMesh (a removed or stale handle: nothing).
    pub fn remove(&mut self, id: CMeshId) {
        if let Some((g, m @ Some(_))) = self.slots.get_mut(id.slot as usize)
            && *g == id.age
        {
            *m = None;
            self.free.push(id.slot);
        }
    }

    /// Every mesh gone (the match's end: endAll).
    pub fn clear(&mut self) {
        for (k, (_, m)) in self.slots.iter_mut().enumerate() {
            if m.take().is_some() {
                self.free.push(k as u32);
            }
        }
    }

    /// The live meshes, in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (CMeshId, &ComponentMesh)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(k, (g, m))| m.as_ref().map(|m| (CMeshId { slot: k as u32, age: *g }, m)))
    }

    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.1.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// An asset key (a package path under /Game, or under /Engine as `Engine/Content/...`) as its object
/// path (the importer's bv_key_path).
pub fn object_path(key: &str) -> String {
    let name = &key[key.rfind('/').map_or(0, |i| i + 1)..];
    match key.strip_prefix("Engine/Content/") {
        Some(rest) => format!("/Engine/{rest}.{name}"),
        None => format!("/Game/{key}.{name}"),
    }
}

/// A component material's key (`<key>#local`, `<key>#spline`) as the material's object path.
pub fn component_material_path(key: &str) -> String {
    match key.split_once('#') {
        Some((k, "local" | "spline")) => object_path(k),
        _ => panic!("component material {key}: not #local or #spline"),
    }
}

/// What a spline mesh lays along its curve (a static mesh's LOD 0
/// positions (UE local, cm), its UV0, its normals (TangentZ), TangentX and binormal signs (TangentZ's W), its colour
/// stream (empty: none), its one section's triangles in the cooked order and its bounds' extent.
#[derive(Clone, Debug)]
pub struct SplineSource {
    pub positions: Vec<[f64; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub normals: Vec<[f64; 3]>,
    pub tangents: Vec<[f64; 3]>,
    pub signs: Vec<f32>,
    pub colors: Vec<[u8; 4]>,
    pub indices: Vec<u32>,
    pub extent: [f64; 3],
}

impl SplineSource {
    /// From the mesh kind's entry `path` (an object path).
    pub fn read(pack: &mvs_store::Pack, path: &str) -> Result<SplineSource> {
        let s = crate::assets::mesh_source(path, &pack.bytes(path)?)?;
        if s.sections != 1 {
            return Err(Error::Format {
                what: path.to_string(),
                why: format!("a component mesh of {} sections", s.sections),
            });
        }
        let xyz = |t: &[f32; 4]| [t[0], t[1], t[2]].map(f64::from);
        Ok(SplineSource {
            positions: s.positions.iter().map(|p| p.map(f64::from)).collect(),
            uvs: s.uv0,
            normals: s.tangent_z.iter().map(xyz).collect(),
            tangents: s.tangent_x.iter().map(xyz).collect(),
            signs: s.tangent_z.iter().map(|t| if t[3] < 0.0 { -1.0 } else { 1.0 }).collect(),
            colors: s.colors,
            indices: s.indices,
            extent: s.extent.map(f64::from),
        })
    }
}

/// A spline mesh's vertices laid along its slices (mvs_view::meshes::SplineMesh) as a mesh's own geometry: the
/// source's UVs, colours and triangles repeated per slice, the slices' turned normals and tangents where the mesh
/// laid them.
pub fn spline_geometry(src: &SplineSource, mesh: &SplineMesh) -> CMeshGeometry {
    let slices = mesh.positions().len().checked_div(src.positions.len()).unwrap_or(0);
    own_slices(src, mesh, 0..slices)
}

/// One draw group's slices of a spline mesh (SplineMesh::groups: the group's run of triangles) as own geometry: the
/// group's slices' vertices only, its triangles counted from them (None: the group has no slice).
pub fn spline_group_geometry(src: &SplineSource, mesh: &SplineMesh, group: usize) -> Option<CMeshGeometry> {
    let per = src.indices.len();
    let &(_, first, count) = mesh.groups().iter().find(|g| g.0 == group)?;
    let (from, to) = (first.checked_div(per)?, (first + count).checked_div(per)?);
    Some(own_slices(src, mesh, from..to))
}

fn own_slices(src: &SplineSource, mesh: &SplineMesh, slices: std::ops::Range<usize>) -> CMeshGeometry {
    let n = src.positions.len();
    let k = slices.len();
    let verts = slices.start * n..slices.end * n;
    let mut g = OwnGeometry {
        positions: mesh.positions()[verts.clone()].to_vec(),
        uvs: Vec::with_capacity(k * n),
        indices: Vec::with_capacity(k * src.indices.len()),
        ..OwnGeometry::default()
    };
    if mesh.normals().len() >= verts.end && mesh.tangents().len() >= verts.end && src.signs.len() == n {
        g.tangent_x = mesh.tangents()[verts.clone()].to_vec();
        g.tangent_z =
            mesh.normals()[verts].iter().enumerate().map(|(v, d)| [d[0], d[1], d[2], src.signs[v % n]]).collect();
    }
    for s in 0..k {
        g.uvs.extend_from_slice(&src.uvs);
        if src.colors.len() == n {
            g.colors.extend_from_slice(&src.colors);
        }
        g.indices.extend(src.indices.iter().map(|i| i + (s * n) as u32));
    }
    CMeshGeometry::Own(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_turn_to_object_paths() {
        assert_eq!(
            component_material_path("Panda_Main/Characters/C019/VFX/C019_PlasmaWhip#local"),
            "/Game/Panda_Main/Characters/C019/VFX/C019_PlasmaWhip.C019_PlasmaWhip"
        );
        assert_eq!(object_path("Engine/Content/BasicShapes/Sphere"), "/Engine/BasicShapes/Sphere.Sphere");
    }

    #[test]
    fn handles_go_stale_with_their_mesh() {
        let mut c = ComponentMeshes::default();
        let g = CMeshGeometry::empty;
        let a = c.add("A/M#local", &[("Team", 1.0)], g());
        c.remove(a);
        let b = c.add("A/N#spline", &[], g());
        assert_eq!(a.slot, b.slot);
        assert!(c.get_mut(a).is_none());
        assert_eq!(&*c.get_mut(b).unwrap().material, "/Game/A/N.N");
        c.remove(a);
        assert_eq!(c.len(), 1);
        c.clear();
        assert!(c.is_empty());
    }

    fn source() -> SplineSource {
        SplineSource {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
            uvs: vec![[0.5, 1.0]; 3],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            tangents: vec![[1.0, 0.0, 0.0]; 3],
            signs: vec![1.0, -1.0, 1.0],
            colors: vec![[255, 0, 0, 255]; 3],
            indices: vec![0, 2, 1],
            extent: [1.0; 3],
        }
    }

    fn laid(src: &SplineSource, groups: &[usize]) -> SplineMesh {
        use mvs_view::meshes::{AlphaRule, SplineSlice};
        let mut m = SplineMesh::new(
            src.positions.clone(),
            src.normals.clone(),
            src.tangents.clone(),
            src.indices.clone(),
            0,
            AlphaRule::Bounds,
        );
        let t = [100.0, 0.0, 0.0];
        let slices: Vec<SplineSlice> = groups
            .iter()
            .map(|&g| SplineSlice { group: g, ..SplineSlice::curve([0.0; 3], t, [100.0, 0.0, 0.0], t) })
            .collect();
        m.update(&slices, [0.0, 0.0, 1.0]);
        m
    }

    #[test]
    fn spline_geometry_repeats_per_slice() {
        let src = source();
        let CMeshGeometry::Own(g) = spline_geometry(&src, &laid(&src, &[0, 0])) else { unreachable!() };
        assert_eq!(g.uvs.len(), 6);
        assert_eq!(g.colors.len(), 6);
        assert_eq!(g.indices, vec![0, 2, 1, 3, 5, 4]);
        assert_eq!(g.tangent_z[4][3], -1.0);
    }

    #[test]
    fn a_group_takes_its_own_slices() {
        let src = source();
        let m = laid(&src, &[1, 0, 1]);
        let CMeshGeometry::Own(g) = spline_group_geometry(&src, &m, 1).expect("group 1") else { unreachable!() };
        assert_eq!(g.positions.len(), 6);
        assert_eq!(g.indices, vec![0, 2, 1, 3, 5, 4]);
        assert!(spline_group_geometry(&src, &m, 2).is_none());
    }
}
