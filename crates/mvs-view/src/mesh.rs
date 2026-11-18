//! What the views need of a mesh, GPU-free: the mesh kind's header (its material slots,
//! LOD 0's sections, a skeletal mesh's reference skeleton) and, for a skeletal mesh, what the view makes
//! of it: the reference pose in the view's space and each bone's inverse bind matrix in UE's.

use serde::Deserialize;

use crate::anim::Pose;
use crate::math::{self, Mat4};
use crate::{Error, Result};

/// The mesh kind's container magic.
const MESH_MAGIC: &[u8; 4] = b"MVM1";

/// A skeleton entry: the skeleton kind's entry and a skeletal mesh header's `skeleton`.
#[derive(Clone, Debug, Deserialize)]
pub struct SkeletonEntry {
    pub path: String,
    pub bones: Vec<String>,
    pub parents: Vec<i32>,
    /// 10 a bone: translation xyz cm, rotation xyzw, scale xyz, float32 values.
    pub pose: Vec<f32>,
}

impl SkeletonEntry {
    /// `skeletonAsset`'s checks: a parent before its child, a pose of the bones' length.
    pub fn check(&self) -> Result<()> {
        let n = self.bones.len();
        let bad = |why: String| Err(Error::Format { what: format!("skeleton {}", self.path), why });
        if self.parents.len() != n {
            return bad(format!("{n} bones, {} parents", self.parents.len()));
        }
        if self.pose.len() != n * 10 {
            return bad(format!("{n} bones, {} pose values", self.pose.len()));
        }
        for (i, &p) in self.parents.iter().enumerate() {
            if p >= i as i32 || p < -1 || (p == -1 && i != 0) {
                return bad(format!("bone {i} ({}) has parent {p}", self.bones[i]));
            }
        }
        Ok(())
    }

    /// `boneIndex`: the first bone of that name, the names compared without case (FName); None for none.
    pub fn bone_index(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.eq_ignore_ascii_case(name))
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct MeshSlot {
    pub name: String,
    pub material: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MeshSection {
    pub slot: usize,
    pub disabled: bool,
    /// First index and index count of the section's triangles.
    #[serde(default)]
    pub first: usize,
    #[serde(default)]
    pub count: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct BodyRef {
    offset: usize,
    length: usize,
    format: String,
}

#[derive(Clone, Debug, Deserialize)]
struct MeshLod {
    sections: Vec<MeshSection>,
    #[serde(default)]
    vertices: usize,
    #[serde(default)]
    streams: std::collections::BTreeMap<String, BodyRef>,
    index: Option<BodyRef>,
}

#[derive(Clone, Debug, Deserialize)]
struct MeshHeader {
    path: String,
    class: String,
    slots: Vec<MeshSlot>,
    skeleton: Option<SkeletonEntry>,
    lods: Vec<MeshLod>,
}

/// A mesh as the views pose it (LOD 0).
#[derive(Clone, Debug)]
pub struct MeshInfo {
    pub path: String,
    pub slots: Vec<MeshSlot>,
    /// LOD 0's render sections.
    pub sections: Vec<MeshSection>,
    /// A skeletal mesh's reference skeleton (its own bones, order and pose); None for a static mesh.
    pub skeleton: Option<SkeletonEntry>,
    /// Per bone of `skeleton`: the inverse of its reference component-space matrix, UE cm (USkeletalMesh::
    /// CalculateInvRefMatrices: the local ToMatrixWithScale times the parent's, a matrix product).
    pub inv_ref: Vec<Mat4>,
    /// Per bone: the reference pose in the view's space (rotation (x, z, y, -w),
    /// translation (X, Z, Y) x 0.01, scale (X, Z, Y)).
    pub rest: Pose,
    /// A skeletal mesh's LOD 0, per section: the joints a vertex of its triangles is weighted to
    /// (ascending), as the geometry has the weights (four a vertex: the four heaviest).
    pub used_joints: Vec<Vec<usize>>,
}

impl MeshInfo {
    /// The mesh kind's entry `path` of `store`.
    pub fn load(store: &mvs_store::Store, path: &str) -> Result<MeshInfo> {
        let bytes = store.kind("mesh")?.bytes(path)?;
        MeshInfo::read(&bytes, path)
    }

    /// [`MeshInfo::load`] once per store: the store's one copy, shared by every match and preview that draws it.
    pub fn shared(store: &mvs_store::Store, path: &str) -> Result<std::sync::Arc<MeshInfo>> {
        let size = store.kind("mesh")?.record(path).map_or(0, |r| r.size as usize);
        store.cached(&format!("meshinfo:{path}"), || MeshInfo::load(store, path), |_| size)
    }

    pub fn read(bytes: &[u8], path: &str) -> Result<MeshInfo> {
        let fmt = |why: String| Error::Format { what: format!("mesh {path}"), why };
        let (json, _) = mvs_anim::format::read_json_bin(MESH_MAGIC, bytes).map_err(fmt)?;
        let h: MeshHeader = serde_json::from_slice(json).map_err(|e| fmt(e.to_string()))?;
        let (_, body) = mvs_anim::format::read_json_bin(MESH_MAGIC, bytes).map_err(fmt)?;
        let lod = h.lods.into_iter().next().ok_or_else(|| fmt("no LOD".into()))?;
        let skinned = h.class == "SkeletalMesh";
        if skinned && h.skeleton.is_none() {
            return Err(fmt("a SkeletalMesh without its reference skeleton".into()));
        }
        let skeleton = if skinned { h.skeleton } else { None };
        let used_joints = if skinned { used_joints(&lod, body).map_err(fmt)? } else { Vec::new() };
        let sections = lod.sections;
        let (mut inv_ref, mut rest) = (Vec::new(), Pose::new(0));
        if let Some(sk) = &skeleton {
            sk.check()?;
            let n = sk.bones.len();
            rest = Pose::new(n);
            let mut world: Vec<Mat4> = Vec::with_capacity(n);
            for b in 0..n {
                let a = |k: usize| f64::from(sk.pose[b * 10 + k]);
                let local = math::compose([a(0), a(1), a(2)], [a(3), a(4), a(5), a(6)], [a(7), a(8), a(9)]);
                let m = match sk.parents[b] {
                    p if p >= 0 => math::mul(&world[p as usize], &local),
                    _ => local,
                };
                inv_ref.push(math::invert(&m));
                world.push(m);
                let (t, r, s) = (b * 3, b * 4, b * 3);
                rest.t[t..t + 3].copy_from_slice(&[(a(0) * 0.01) as f32, (a(2) * 0.01) as f32, (a(1) * 0.01) as f32]);
                rest.r[r..r + 4].copy_from_slice(&[a(3) as f32, a(5) as f32, a(4) as f32, -a(6) as f32]);
                rest.s[s..s + 3].copy_from_slice(&[a(7) as f32, a(9) as f32, a(8) as f32]);
            }
        }
        Ok(MeshInfo { path: h.path, slots: h.slots, sections, skeleton, inv_ref, rest, used_joints })
    }

    pub fn bones(&self) -> &[String] {
        self.skeleton.as_ref().map_or(&[], |s| &s.bones)
    }
}

/// A bone's vertex box: the min and max corners (UE cm, the mesh's component space in its reference pose).
pub type BoneBox = [[f32; 3]; 2];

/// Per bone of a skeletal mesh's reference skeleton: the box of the LOD 0 vertices whose heaviest influence is that
/// bone (None: no vertex). Put through a pose's skinning matrices, the boxes bound the posed mesh (each box as rigid
/// as its bone). Made once per store and mesh.
pub fn bone_boxes(store: &mvs_store::Store, path: &str) -> Result<std::sync::Arc<Vec<Option<BoneBox>>>> {
    store.cached(
        &format!("boneboxes:{path}"),
        || {
            let bytes = store.kind("mesh")?.bytes(path)?;
            boxes_of(&bytes, path)
        },
        |b| b.len() * std::mem::size_of::<Option<BoneBox>>(),
    )
}

fn boxes_of(bytes: &[u8], path: &str) -> Result<Vec<Option<BoneBox>>> {
    let fmt = |why: String| Error::Format { what: format!("mesh {path}"), why };
    let (json, body) = mvs_anim::format::read_json_bin(MESH_MAGIC, bytes).map_err(fmt)?;
    let h: MeshHeader = serde_json::from_slice(json).map_err(|e| fmt(e.to_string()))?;
    let bones = h.skeleton.as_ref().map_or(0, |s| s.bones.len());
    let lod = h.lods.into_iter().next().ok_or_else(|| fmt("no LOD".into()))?;
    let get = |name: &str| lod.streams.get(name).ok_or_else(|| fmt(format!("no {name} stream")));
    let n = lod.vertices;
    let p = get("position")?;
    let pos = body.get(p.offset..p.offset + p.length).filter(|_| p.format == "f32x3" && p.length == 12 * n);
    let pos = pos.ok_or_else(|| fmt(format!("position stream {} of {} bytes", p.format, p.length)))?;
    let pos: Vec<f32> = pos.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
    let mut sets =
        vec![(stream_values(body, get("joints0")?).map_err(fmt)?, stream_values(body, get("weights0")?).map_err(fmt)?)];
    if let (Some(j), Some(w)) = (lod.streams.get("joints1"), lod.streams.get("weights1")) {
        sets.push((stream_values(body, j).map_err(fmt)?, stream_values(body, w).map_err(fmt)?));
    }
    if sets.iter().any(|(j, w)| j.len() != n * 4 || w.len() != n * 4) {
        return Err(fmt(format!("{n} vertices: joints or weights of another length")));
    }
    let mut out: Vec<Option<BoneBox>> = vec![None; bones];
    for v in 0..n {
        // (the heaviest influence; the first of equal ones)
        let (mut joint, mut best) = (0usize, 0u32);
        for (j, w) in &sets {
            for c in 0..4 {
                if w[v * 4 + c] > best {
                    best = w[v * 4 + c];
                    joint = j[v * 4 + c] as usize;
                }
            }
        }
        let Some(b) = out.get_mut(joint) else { continue };
        let at = [pos[v * 3], pos[v * 3 + 1], pos[v * 3 + 2]];
        match b {
            Some([lo, hi]) => {
                for k in 0..3 {
                    lo[k] = lo[k].min(at[k]);
                    hi[k] = hi[k].max(at[k]);
                }
            }
            None => *b = Some([at, at]),
        }
    }
    Ok(out)
}

/// A stream's values as u32, by its format (u8x4 / u16x4 joints, unorm8x4 weights; u16 / u32 indices).
fn stream_values(body: &[u8], r: &BodyRef) -> std::result::Result<Vec<u32>, String> {
    let d = body.get(r.offset..r.offset + r.length).ok_or_else(|| format!("a stream past the body ({})", r.format))?;
    Ok(match r.format.as_str() {
        "u8x4" | "unorm8x4" => d.iter().map(|&b| u32::from(b)).collect(),
        "u16x4" | "u16" => d.as_chunks::<2>().0.iter().map(|b| u32::from(u16::from_le_bytes(*b))).collect(),
        "u32" => d.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect(),
        f => return Err(format!("stream format {f}")),
    })
}

/// `usedJoints` of each section of a skeletal LOD: the joints with a weight above 0 over the section's index range, the
/// weights as the geometry takes them (the four heaviest of eight influences, renormalised to
/// sum 255, ties keeping the earlier slot).
fn used_joints(lod: &MeshLod, body: &[u8]) -> std::result::Result<Vec<Vec<usize>>, String> {
    let get = |name: &str| lod.streams.get(name).ok_or_else(|| format!("no {name} stream"));
    let j0 = stream_values(body, get("joints0")?)?;
    let w0 = stream_values(body, get("weights0")?)?;
    let eight = match (lod.streams.get("joints1"), lod.streams.get("weights1")) {
        (Some(j), Some(w)) => Some((stream_values(body, j)?, stream_values(body, w)?)),
        _ => None,
    };
    let n = lod.vertices;
    if j0.len() != n * 4 || w0.len() != n * 4 {
        return Err(format!("{n} vertices, {} joints, {} weights", j0.len(), w0.len()));
    }
    // per vertex its joints of weight > 0 as the geometry has them
    let mut vert: Vec<[Option<u32>; 4]> = Vec::with_capacity(n);
    for v in 0..n {
        let b = v * 4;
        let Some((j1, w1)) = &eight else {
            vert.push(std::array::from_fn(|c| (w0[b + c] > 0).then_some(j0[b + c])));
            continue;
        };
        let w = |k: usize| if k < 4 { w0[b + k] } else { w1[b + k - 4] };
        let j = |k: usize| if k < 4 { j0[b + k] } else { j1[b + k - 4] };
        let mut taken = 0u32;
        let mut pick = [0usize; 4];
        for p in &mut pick {
            let (mut best, mut bw) = (usize::MAX, -1i64);
            for k in 0..8 {
                if taken & (1 << k) == 0 && i64::from(w(k)) > bw {
                    bw = i64::from(w(k));
                    best = k;
                }
            }
            taken |= 1 << best;
            *p = best;
        }
        let sum: u32 = pick.iter().map(|&k| w(k)).sum();
        // (the rounding's remainder goes on the heaviest, which keeps a weight above 0)
        vert.push(std::array::from_fn(|r| {
            let k = pick[r];
            let q = if sum > 0 {
                (f64::from(w(k)) * 255.0 / f64::from(sum) + 0.5).floor()
            } else if r == 0 {
                255.0
            } else {
                0.0
            };
            (q > 0.0 || (r == 0 && sum > 0)).then_some(j(k))
        }));
    }
    let index = lod.index.as_ref().map(|i| stream_values(body, i)).transpose()?.unwrap_or_default();
    Ok(lod
        .sections
        .iter()
        .map(|s| {
            let mut seen = std::collections::BTreeSet::new();
            let to = (s.first + s.count).min(index.len());
            for &v in &index[s.first.min(to)..to] {
                for j in vert[v as usize].iter().flatten() {
                    seen.insert(*j as usize);
                }
            }
            seen.into_iter().collect()
        })
        .collect())
}
