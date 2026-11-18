//! The boxes bind's frame half: FK over the fighter's skeletal mesh's reference skeleton with the animation kind's
//! clips per frame, COM Replace, root lock, additives, pose additive, the skel block (`skel.rs`), socket axes and
//! actor tracks, packed as the boxes' float32 section.
//!
//! The FK runs in the build's space (UE cm, X fwd, Y depth, Z up) in float64 and writes actor space (fwd = X, up = Z,
//! depth = Y), float32 stored. The skel block alone stays in glTF metres (`skel.rs`: the sim's pose code reads it so).
//!
//! Per frame:
//! - a bone's local transform is the clip's (rotation normalised); a mesh bone the clip does not give is the mesh's
//!   reference; an additive clip's frame is its base plus its delta, its scale the reference's; a POSE_ADDITIVE delta
//!   is added to the translation of every non-additive clip's bones;
//! - root lock (bEnableRootMotion taken as always extracted, or bForceRootLock): bone 0 is replaced by
//!   RootMotionRootLock (RefPose the reference's, AnimFirstFrame the root at frame 0, Zero the identity); the frame's
//!   root pair stays the root's own translation;
//! - COM Replace: in the clips BoxBindInput.com_replaced names, COM_BONE's component rotation is the identity (its
//!   place and scale kept, the bones below following);
//! - a shape at its bone's component transform: RelativeLocation x the bone's scale turned by its rotation; the sizes
//!   by the bone's scale (sphere min |axis|, capsule radius min(|X|, |Y|), half height |Z|); a box's three half extent
//!   vectors; a virtual bone's ModifyBone scale and bone-space translation; an on_capsule shape in capsule space.
//!
//! A bone keyed to scale 0 (Harley's bat) gives its shapes a centre at the bone's origin and size 0: the engine's rule
//! too (a point shape that still overlaps).

// (the index loops run over parallel arrays by index)
#![allow(clippy::needless_range_loop)]

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use mvs_anim::clip::AnimClip;
use mvs_anim::pose::BonePose;
use mvs_data::boxes::{ActorBoxIndex, BoxAnim, BoxCom, BoxKind, BoxShape, SocketAxes};
use mvs_data::fighter::{ActorDef, Map};
use mvs_data::{BoxIndex, FighterData, Value};

use super::super::shape::{cdo_path, defaults_or_properties, fail};
use super::super::transform::{Quat, Vec3};
use super::consts::{COM_BONE, PoseAdditive, pose_additive, skel_spec};
use super::index::{MeshOf, bone_box_inputs, com_shared};
use super::skel::{SkelBlock, skel_blend, skel_wanted};
use super::{ActorBindInput, BoneBoxInput, BoxBindInput, BoxClip, Placed, export_name, get};
use crate::kind::{Cx, Item, Kind, KindError};
use crate::kinds::animation::Animation;
use crate::kinds::{mesh, skeleton};

/// The skeleton kinds' pose: 10 numbers a bone.
const POSE_STRIDE: usize = 10;

/// A skeletal mesh's reference skeleton as the loop reads it: UE space, float64 (of the float32 pose), a parent
/// before its children.
pub struct RefSkeleton {
    pub bones: Vec<String>,
    pub parents: Vec<i32>,
    pub t: Vec<f64>,
    pub r: Vec<f64>,
    pub s: Vec<f64>,
    by: HashMap<String, usize>,
}

impl RefSkeleton {
    /// The bone of a name (exact; a name given twice, its last), -1 none.
    pub fn index(&self, name: &str) -> i32 {
        self.by.get(name).map_or(-1, |b| *b as i32)
    }

    fn parent(&self, b: usize) -> i32 {
        self.parents[b]
    }
}

/// A local pose by mesh bone (UE).
#[derive(Clone)]
pub struct LocalPose {
    pub t: Vec<f64>,
    pub r: Vec<f64>,
    pub s: Vec<f64>,
}

pub fn new_pose(n: usize) -> LocalPose {
    LocalPose { t: vec![0.0; n * 3], r: vec![0.0; n * 4], s: vec![0.0; n * 3] }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootLock {
    RefPose,
    AnimFirstFrame,
    Zero,
}

// ---- float64 quaternion arithmetic

/// `a b`.
fn qmul(a: Quat, b: Quat) -> Quat {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// `q` turning (vx, vy, vz).
fn qrot(q: Quat, vx: f64, vy: f64, vz: f64) -> Vec3 {
    let [x, y, z, w] = q;
    let tx = 2.0 * (y * vz - z * vy);
    let ty = 2.0 * (z * vx - x * vz);
    let tz = 2.0 * (x * vy - y * vx);
    [vx + w * tx + y * tz - z * ty, vy + w * ty + z * tx - x * tz, vz + w * tz + x * ty - y * tx]
}

/// The quaternion of `r` at `o`.
fn q4<T: Copy + Into<f64>>(r: &[T], o: usize) -> Quat {
    [r[o].into(), r[o + 1].into(), r[o + 2].into(), r[o + 3].into()]
}

/// The quaternion normalised (`|| 1`: a zero or NaN length leaves it).
fn norm(q: Quat) -> Quat {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let n = if n == 0.0 || n.is_nan() { 1.0 } else { n };
    [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
}

// ---- the store's source

/// An additive sequence's header: its type, its base's type and sequence, its frame.
#[derive(Clone, Debug)]
struct Additive {
    ty: f64,
    ref_pose_type: f64,
    ref_pose_seq: Option<String>,
    ref_frame_index: f64,
}

/// A clip's entry as the source loaded it (its base's beside it).
pub struct Loaded {
    name: String,
    path: String,
    bytes: Vec<u8>,
    add: Option<Additive>,
    base: Option<Vec<u8>>,
}

/// One clip as the loop reads it.
pub struct ClipSource<'a> {
    pub name: String,
    pub num_frames: usize,
    pub length: f64,
    pub rate_scale: f64,
    pub root_motion: bool,
    pub step: bool,
    /// The pose root's lock (bEnableRootMotion or bForceRootLock), `None` none.
    pub lock: Option<RootLock>,
    pub additive: bool,
    /// By mesh bone: the clip has a track of its own for it (the skel block's flag 8).
    pub tracked: Vec<bool>,
    a: AnimClip<'a>,
    base: Option<AnimClip<'a>>,
    add: Option<&'a Additive>,
    path: &'a str,
}

impl ClipSource<'_> {
    /// An additive clip's base as the skel block takes it (AAT_LocalSpaceBase over ABPT_AnimFrame; anything else is
    /// refused): its RefPoseSeq and RefFrameIndex.
    pub fn skel_base(&self) -> Result<(BoxClip, f64), KindError> {
        match self.add {
            Some(add) if add.ty == 1.0 && add.ref_pose_type == 3.0 && self.base.is_some() => {
                let p = add.ref_pose_seq.clone().unwrap_or_default();
                Ok((BoxClip { name: export_name(&p).to_owned(), path: p }, add.ref_frame_index))
            }
            _ => fail(format!(
                "boxes: {}: additive type {:?} over base type {:?} (the skel block takes local space over AnimFrame)",
                self.path,
                self.add.map(|a| a.ty),
                self.add.map(|a| a.ref_pose_type)
            )),
        }
    }
}

fn is_same_path(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// A JSON number member of the header's additive.
fn jnum(v: &serde_json::Value, k: &str) -> f64 {
    v.get(k).and_then(serde_json::Value::as_f64).unwrap_or(0.0)
}

/// The store's bones and clips: the mesh kind's own reference skeleton (the FK's bones and the pose of a bone a clip
/// does not track), the skeleton kind (the clips' bone indices, the linkup) and the animation kind's AnimClip
/// sampler.
// Unverified: a bone of the Skeleton a clip has no track for starts from the mesh's reference pose; a
// Skeleton bone the mesh lacks takes the Skeleton's.
pub struct PoseSource<'c, 'x> {
    cx: &'c Cx<'x>,
    skeleton_path: String,
    pub ref_: RefSkeleton,
    skel_bones: Vec<String>,
    skel_parents: Vec<i32>,
    mesh_to_skel: Vec<i32>,
    skel_to_mesh: Vec<i32>,
    /// By mesh bone: the clip gives the bone (else the mesh's reference).
    pub listed: Vec<bool>,
    /// The sampler's start pose, by Skeleton bone; a clip whose tracks reach past the Skeleton's bones (tracks of
    /// bones it no longer has, Shaggy_AirAttack_Heavy_N) is sampled into poses that long, the extra bones read by
    /// nothing.
    start: BonePose,
    p0: BonePose,
    p1: BonePose,
    p2: BonePose,
}

/// A pose value's float32, as `Float32Array.from` takes it.
fn f32s(v: &[f64]) -> Vec<f32> {
    v.iter().map(|x| *x as f32).collect()
}

impl<'c, 'x> PoseSource<'c, 'x> {
    pub fn new(cx: &'c Cx<'x>, mesh_path: &str, skeleton_path: &str) -> Result<Self, KindError> {
        let m = mesh::header(cx, mesh_path)?;
        let Some(msk) = &m.skeleton else { return fail(format!("boxes: mesh {mesh_path} is not skinned")) };
        let skel = skeleton::entry(cx, skeleton_path)?;
        let mpose = f32s(&msk.pose);
        let spose = f32s(&skel.pose);
        let nm = msk.bones.len();
        let ns = skel.bones.len();
        let (mut t, mut r, mut s) = (vec![0.0; nm * 3], vec![0.0; nm * 4], vec![0.0; nm * 3]);
        for b in 0..nm {
            let k = b * POSE_STRIDE;
            for i in 0..3 {
                t[b * 3 + i] = mpose[k + i] as f64;
                s[b * 3 + i] = mpose[k + 7 + i] as f64;
            }
            for i in 0..4 {
                r[b * 4 + i] = mpose[k + 3 + i] as f64;
            }
        }
        let mut by = HashMap::new();
        for (i, b) in msk.bones.iter().enumerate() {
            by.insert(b.clone(), i);
        }
        let ref_ = RefSkeleton { bones: msk.bones.clone(), parents: msk.parents.clone(), t, r, s, by };
        // FSkeletonToMeshLinkup: the Skeleton's bone of a mesh bone's name (without case, the first), and per
        // Skeleton bone the first mesh bone that names it
        let mut by_lower: HashMap<String, i32> = HashMap::new();
        for (i, b) in skel.bones.iter().enumerate() {
            by_lower.entry(b.to_lowercase()).or_insert(i as i32);
        }
        let mesh_to_skel: Vec<i32> =
            msk.bones.iter().map(|b| *by_lower.get(&b.to_lowercase()).unwrap_or(&-1)).collect();
        let mut skel_to_mesh = vec![-1; ns];
        for (m, s) in mesh_to_skel.iter().enumerate() {
            if *s >= 0 && skel_to_mesh[*s as usize] == -1 {
                skel_to_mesh[*s as usize] = m as i32;
            }
        }
        let mut start = BonePose::new(ns);
        for b in 0..ns {
            let m = skel_to_mesh[b];
            let (src, k) = if m >= 0 { (&mpose, m as usize * POSE_STRIDE) } else { (&spose, b * POSE_STRIDE) };
            for i in 0..3 {
                start.t[b * 3 + i] = src[k + i];
                start.s[b * 3 + i] = src[k + 7 + i];
            }
            for i in 0..4 {
                start.r[b * 4 + i] = src[k + 3 + i];
            }
        }
        let listed = mesh_to_skel.iter().map(|s| *s >= 0).collect();
        Ok(PoseSource {
            cx,
            skeleton_path: skeleton_path.to_owned(),
            ref_,
            skel_bones: skel.bones.clone(),
            skel_parents: skel.parents.clone(),
            mesh_to_skel,
            skel_to_mesh,
            listed,
            p0: BonePose::new(ns),
            p1: BonePose::new(ns),
            p2: BonePose::new(ns),
            start,
        })
    }

    /// Grows the start pose to a clip's bones.
    fn fit(&mut self, bone_end: usize) {
        if bone_end <= self.start.n {
            return;
        }
        let mut s = BonePose::new(bone_end);
        let n = self.start.n;
        s.t[..n * 3].copy_from_slice(&self.start.t);
        s.r[..n * 4].copy_from_slice(&self.start.r);
        s.s[..n * 3].copy_from_slice(&self.start.s);
        for b in n..bone_end {
            s.r[b * 4 + 3] = 1.0;
            for k in 0..3 {
                s.s[b * 3 + k] = 1.0;
            }
        }
        self.start = s;
        self.p0 = BonePose::new(bone_end);
        self.p1 = BonePose::new(bone_end);
        self.p2 = BonePose::new(bone_end);
    }

    /// A sequence's entry and its header's skeleton and additive.
    fn entry(&self, path: &str) -> Result<(Vec<u8>, String, Option<Additive>), KindError> {
        let bytes = Animation.build(self.cx, &Item::new(path))?;
        let (json, _) = mvs_anim::format::read_json_bin(b"MVA3", &bytes).map_err(KindError::Failed)?;
        let h: serde_json::Value =
            serde_json::from_slice(json).map_err(|e| KindError::Failed(format!("{path}: animation header: {e}")))?;
        let skeleton = h.get("skeleton").and_then(serde_json::Value::as_str).unwrap_or("").to_owned();
        let add = h.get("additive").filter(|a| !a.is_null()).map(|a| Additive {
            ty: jnum(a, "type"),
            ref_pose_type: jnum(a, "refPoseType"),
            ref_pose_seq: a.get("refPoseSeq").and_then(serde_json::Value::as_str).map(str::to_owned),
            ref_frame_index: jnum(a, "refFrameIndex"),
        });
        Ok((bytes, skeleton, add))
    }

    /// The clip, or `None` when it is not on this mesh's Skeleton (a prop's sequence).
    pub fn load(&self, c: &BoxClip) -> Result<Option<Loaded>, KindError> {
        let (bytes, sk, add) = self.entry(&c.path)?;
        if !is_same_path(&sk, &self.skeleton_path) {
            return Ok(None);
        }
        let add = add.filter(|a| a.ty != 0.0);
        let mut base = None;
        if let Some(a) = &add
            && a.ref_pose_type >= 2.0
        {
            let Some(seq) = &a.ref_pose_seq else {
                let t = if a.ref_pose_type == 2.0 { "AnimScaled" } else { "AnimFrame" };
                return fail(format!("boxes: {} is additive over {t} with no RefPoseSeq", c.path));
            };
            let (b, bsk, badd) = self.entry(seq)?;
            if !is_same_path(&bsk, &self.skeleton_path) {
                return fail(format!("boxes: {}'s RefPoseSeq {seq} is on another Skeleton", c.path));
            }
            if badd.is_some_and(|x| x.ty != 0.0) {
                return fail(format!("boxes: {}'s base {seq} is additive", c.path));
            }
            base = Some(b);
        }
        Ok(Some(Loaded { name: c.name.clone(), path: c.path.clone(), bytes, add, base }))
    }

    /// The loaded clip ready to sample.
    pub fn open<'a>(&mut self, l: &'a Loaded) -> Result<ClipSource<'a>, KindError> {
        let a = AnimClip::new(&l.bytes, &l.path).map_err(KindError::Failed)?;
        self.fit(a.bone_end);
        let base = match &l.base {
            Some(b) => {
                let seq = l.add.as_ref().and_then(|x| x.ref_pose_seq.as_deref()).unwrap_or("");
                let bc = AnimClip::new(b, seq).map_err(KindError::Failed)?;
                self.fit(bc.bone_end);
                Some(bc)
            }
            None => None,
        };
        let nm = self.ref_.bones.len();
        let mut tracked = vec![false; nm];
        for i in 0..a.tracks {
            let b = a.track_bone(i);
            if b >= 0
                && let Some(&m) = self.skel_to_mesh.get(b as usize)
                && m >= 0
            {
                tracked[m as usize] = true;
            }
        }
        let lock = if a.force_root_lock || a.root_motion {
            match a.root_lock as i64 {
                0 => Some(RootLock::RefPose),
                1 => Some(RootLock::AnimFirstFrame),
                2 => Some(RootLock::Zero),
                _ => return fail(format!("boxes: {}: RootMotionRootLock {}", l.path, a.root_lock)),
            }
        } else {
            None
        };
        if lock.is_some() && l.add.is_some() {
            return fail(format!(
                "boxes: {}: an additive sequence with a root lock (the scale rule is not handled)",
                l.path
            ));
        }
        Ok(ClipSource {
            name: l.name.clone(),
            num_frames: a.frames as usize,
            length: a.length,
            rate_scale: a.rate_scale,
            root_motion: a.root_motion,
            step: a.step,
            lock,
            additive: l.add.is_some(),
            tracked,
            base,
            add: l.add.as_ref(),
            path: &l.path,
            a,
        })
    }

    /// The clip's pose at frame position f of n frames (its time), by Skeleton bone, into out.
    fn at(a: &mut AnimClip<'_>, start: &BonePose, f: f64, n: f64, out: &mut BonePose) {
        let time = if n > 1.0 { if f >= n - 1.0 { a.length } else { (f / (n - 1.0)) * a.length } } else { 0.0 };
        a.sample(time, start, out, false, true); // unlocked: the loop takes the root pair, then locks (lock_root)
    }

    fn to_mesh(&self, p: &BonePose, out: &mut LocalPose) {
        let r = &self.ref_;
        for m in 0..r.bones.len() {
            let b = self.mesh_to_skel[m];
            if b < 0 {
                out.t[m * 3..m * 3 + 3].copy_from_slice(&r.t[m * 3..m * 3 + 3]);
                out.r[m * 4..m * 4 + 4].copy_from_slice(&r.r[m * 4..m * 4 + 4]);
                out.s[m * 3..m * 3 + 3].copy_from_slice(&r.s[m * 3..m * 3 + 3]);
                continue;
            }
            let b = b as usize;
            for i in 0..3 {
                out.t[m * 3 + i] = p.t[b * 3 + i] as f64;
                out.s[m * 3 + i] = p.s[b * 3 + i] as f64;
            }
            let q = norm(q4(&p.r, b * 4));
            out.r[m * 4..m * 4 + 4].copy_from_slice(&q);
        }
    }

    /// Frame f's local pose into out (every mesh bone; rotations normalised; an additive clip's base + delta).
    pub fn frame(&mut self, cs: &mut ClipSource<'_>, f: f64, out: &mut LocalPose) {
        let n = cs.num_frames as f64;
        let Some(add) = cs.add else {
            Self::at(&mut cs.a, &self.start, f, n, &mut self.p0);
            let p0 = std::mem::replace(&mut self.p0, BonePose::new(0));
            self.to_mesh(&p0, out);
            self.p0 = p0;
            return;
        };
        // base + delta (AccumulateAdditivePose at weight 1: rotation delta x base, translation base + delta, scale
        // base x (1 + delta); UAnimSequence GetAnimationPose 142871c50)
        Self::at(&mut cs.a, &self.start, f, n, &mut self.p1);
        match cs.base.as_mut() {
            Some(base) if add.ref_pose_type != 1.0 => {
                if add.ref_pose_type == 2.0 {
                    // AnimScaled: the base at the same share of its length
                    let time = if n > 1.0 { (f / (n - 1.0)) * base.length } else { 0.0 };
                    base.sample(time, &self.start, &mut self.p0, false, true);
                } else {
                    let frames = base.frames;
                    Self::at(base, &self.start, add.ref_frame_index, frames, &mut self.p0);
                }
            }
            _ => self.p0.copy(&self.start),
        }
        if add.ty == 2.0 {
            mesh_space_additive(&self.skel_parents, &self.p0, &self.p1, &mut self.p2);
        } else {
            local_additive(&self.p0, &self.p1, &mut self.p2);
        }
        let p2 = std::mem::replace(&mut self.p2, BonePose::new(0));
        self.to_mesh(&p2, out);
        self.p2 = p2;
    }

    /// POSE_ADDITIVE's sequence: per mesh bone its translation delta (UE cm). `rides(bone)`: a shape other than a
    /// socket at the bone itself rides the bone or a bone under it (a scale track there is refused).
    fn pose_additive(&mut self, seq: &str, rides: &dyn Fn(usize) -> bool) -> Result<IndexMap<usize, Vec3>, KindError> {
        let c = BoxClip { name: export_name(seq).to_owned(), path: seq.to_owned() };
        let Some(l) = self.load(&c)? else { return fail(format!("boxes: pose additive {seq} is on another Skeleton")) };
        let mut a = AnimClip::new(&l.bytes, seq).map_err(KindError::Failed)?;
        self.fit(a.bone_end);
        if a.additive_type != 1.0 {
            return fail(format!("boxes: pose additive {seq}: additive type {} (not handled)", a.additive_type));
        }
        // POSE_ADDITIVE's rule: every translation track a constant (one key: the player's time cannot matter), no
        // rotation track, a scale track only where nothing but a socket at the bone rides it. Read from the sampled
        // delta over every frame (a track that changes nothing passes as none).
        let mut out = IndexMap::new();
        let mut frames: Vec<BonePose> = Vec::new();
        let nf = a.frames;
        for f in 0..(nf.max(1.0) as usize) {
            let mut p = BonePose::new(self.start.n);
            Self::at(&mut a, &self.start, f as f64, nf, &mut p);
            frames.push(p);
        }
        for i in 0..a.tracks {
            let b = a.track_bone(i);
            let m = if b >= 0 { self.skel_to_mesh.get(b as usize).copied().unwrap_or(-1) } else { -1 };
            if m < 0 {
                continue; // a bone of another skin's mesh
            }
            let b = b as usize;
            let bone = self.skel_bones.get(b).cloned().unwrap_or_default();
            let p = &frames[0];
            for q in &frames {
                if q.r[b * 4] != 0.0 || q.r[b * 4 + 1] != 0.0 || q.r[b * 4 + 2] != 0.0 {
                    return fail(format!("boxes: pose additive {seq}: a rotation on {bone} (not handled)"));
                }
                for k in 0..3 {
                    if q.t[b * 3 + k] != p.t[b * 3 + k] {
                        return fail(format!("boxes: pose additive {seq}: the translation of {bone} is not one key"));
                    }
                }
            }
            if a.track_has_scale(i)
                && frames.iter().any(|q| q.s[b * 3] != 0.0 || q.s[b * 3 + 1] != 0.0 || q.s[b * 3 + 2] != 0.0)
                && rides(m as usize)
            {
                return fail(format!(
                    "boxes: pose additive {seq}: a scale track on {bone}, which shapes ride (not handled)"
                ));
            }
            out.insert(m as usize, [p.t[b * 3] as f64, p.t[b * 3 + 1] as f64, p.t[b * 3 + 2] as f64]);
        }
        Ok(out)
    }
}

/// base + delta in local space into out.
fn local_additive(base: &BonePose, d: &BonePose, out: &mut BonePose) {
    for b in 0..base.n {
        for k in 0..3 {
            let i = b * 3 + k;
            out.t[i] = (base.t[i] as f64 + d.t[i] as f64) as f32;
            out.s[i] = (base.s[i] as f64 * (1.0 + d.s[i] as f64)) as f32;
        }
        let q = norm(qmul(q4(&d.r, b * 4), q4(&base.r, b * 4)));
        for k in 0..4 {
            out.r[b * 4 + k] = q[k] as f32;
        }
    }
}

/// AAT_RotationOffsetMeshSpace over base into out: the base's rotations to component space, the delta's rotation in
/// front of each, back to local; translation and scale as in local space.
// Unverified: stock 4.26 AccumulateMeshSpaceRotationAdditiveToLocalPose taken, not read in the exe; WW's LassoGrab
// clips only.
fn mesh_space_additive(parents: &[i32], base: &BonePose, d: &BonePose, out: &mut BonePose) {
    let n = base.n;
    let mut comp: Vec<Quat> = vec![[0.0; 4]; n];
    let mut next: Vec<Quat> = vec![[0.0; 4]; n];
    let parent = |b: usize| if b < parents.len() { parents[b] } else { -1 };
    for b in 0..n {
        let p = parent(b);
        comp[b] = if p < 0 { q4(&base.r, b * 4) } else { qmul(comp[p as usize], q4(&base.r, b * 4)) };
        next[b] = norm(qmul(q4(&d.r, b * 4), comp[b]));
    }
    for b in 0..n {
        let p = parent(b);
        let q = if p < 0 {
            next[b]
        } else {
            let np = next[p as usize];
            qmul([-np[0], -np[1], -np[2], np[3]], next[b])
        };
        let q = norm(q);
        for k in 0..4 {
            out.r[b * 4 + k] = q[k] as f32;
        }
        for k in 0..3 {
            let i = b * 3 + k;
            out.t[i] = (base.t[i] as f64 + d.t[i] as f64) as f32;
            out.s[i] = (base.s[i] as f64 * (1.0 + d.s[i] as f64)) as f32;
        }
    }
}

// ---- the loop

/// The world transforms the loop computes, by mesh bone.
struct World {
    t: Vec<f64>,
    r: Vec<f64>,
    s: Vec<f64>,
}

impl World {
    fn new(n: usize) -> World {
        World { t: vec![0.0; n * 3], r: vec![0.0; n * 4], s: vec![0.0; n * 3] }
    }

    fn q(&self, b: usize) -> Quat {
        q4(&self.r, b * 4)
    }
}

/// The FK of `order` (mesh bones, parents first) of pose p into w. Returns the rotation (deg) COM Replace took off.
fn forward(ref_: &RefSkeleton, order: &[usize], p: &LocalPose, w: &mut World, com: i32, com_replace: bool) -> f64 {
    let mut removed = 0.0;
    for &b in order {
        let par = ref_.parent(b);
        let (b3, b4) = (b * 3, b * 4);
        if par < 0 {
            for k in 0..3 {
                w.t[b3 + k] = p.t[b3 + k];
                w.s[b3 + k] = p.s[b3 + k];
            }
            for k in 0..4 {
                w.r[b4 + k] = p.r[b4 + k];
            }
        } else {
            let par = par as usize;
            let p3 = par * 3;
            let v = qrot(w.q(par), p.t[b3] * w.s[p3], p.t[b3 + 1] * w.s[p3 + 1], p.t[b3 + 2] * w.s[p3 + 2]);
            w.t[b3] = w.t[p3] + v[0];
            w.t[b3 + 1] = w.t[p3 + 1] + v[1];
            w.t[b3 + 2] = w.t[p3 + 2] + v[2];
            let q = qmul(w.q(par), q4(&p.r, b4));
            w.r[b4..b4 + 4].copy_from_slice(&q);
            for k in 0..3 {
                w.s[b3 + k] = w.s[p3 + k] * p.s[b3 + k];
            }
        }
        if com_replace && b as i32 == com {
            // COM Replace: the bone's component rotation becomes the identity, its place and scale stay
            let [x, y, z, q] = w.q(b);
            if x != 0.0 || y != 0.0 || z != 0.0 || q != 1.0 {
                let c = (q.abs() / (x * x + y * y + z * z + q * q).sqrt()).min(1.0);
                removed = (2.0 * mvs_math::libm::acos(c) * 180.0) / std::f64::consts::PI;
            }
            w.r[b4..b4 + 4].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
        }
    }
    removed
}

/// The bones of `ref_` that `bones` hang from, themselves included, in ref's order (parents first).
fn chain_of(ref_: &RefSkeleton, bones: impl IntoIterator<Item = i32>) -> Vec<usize> {
    let mut need = vec![false; ref_.bones.len()];
    for b in bones {
        let mut b = b;
        while b >= 0 && !need[b as usize] {
            need[b as usize] = true;
            b = ref_.parent(b as usize);
        }
    }
    (0..need.len()).filter(|b| need[*b]).collect()
}

fn bone_of(ref_: &RefSkeleton, name: &str, what: &str) -> Result<i32, KindError> {
    let b = ref_.index(name);
    if b < 0 {
        return fail(format!("boxes: bone {name} of {what} is not on the mesh"));
    }
    Ok(b)
}

/// Floats a shape takes in a frame.
fn size_of(sh: &BoxShape) -> usize {
    if sh.kind == BoxKind::Socket {
        2
    } else if sh.is_box == Some(true) {
        12
    } else if sh.capsule {
        7
    } else {
        4
    }
}

/// A capsule's or a sphere's numbers at (cf, cu, cd), its bone's world rotation `wr` and scale (sx, sy, sz) (UE
/// axes), into out.
#[allow(clippy::too_many_arguments)]
fn round_vals(sh: &Placed, wr: Quat, sx: f64, sy: f64, sz: f64, cf: f64, cu: f64, cd: f64, out: &mut [f32]) {
    if sh.shape.capsule {
        let v = qrot(qmul(wr, sh.rot), 0.0, 0.0, 1.0); // the capsule's local Z
        let rs = sx.abs().min(sy.abs());
        let h = (sh.shape.half_height * sz.abs() - sh.shape.radius * rs).max(0.0);
        out[0] = (cf - v[0] * h) as f32;
        out[1] = (cu - v[2] * h) as f32;
        out[2] = (cd - v[1] * h) as f32;
        out[3] = (cf + v[0] * h) as f32;
        out[4] = (cu + v[2] * h) as f32;
        out[5] = (cd + v[1] * h) as f32;
        out[6] = rs as f32;
        return;
    }
    out[0] = cf as f32;
    out[1] = cu as f32;
    out[2] = cd as f32;
    out[3] = sx.abs().min(sy.abs()).min(sz.abs()) as f32;
}

/// bAbsoluteRotation: the world's axes as (fwd, up, depth) for a box's local X, Y, Z (the fighter's yaw is +-90: X
/// the depth, Y the play plane's horizontal).
const WORLD_AXES: [f64; 9] = [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

const IDENTITY: Quat = [0.0, 0.0, 0.0, 1.0];

/// The unit axis k.
fn axis(k: usize) -> Vec3 {
    [(k == 0) as u8 as f64, (k == 1) as u8 as f64, (k == 2) as u8 as f64]
}

/// The index with its offsets laid out, and the float32 section they count from.
pub fn sample_box_frames(
    cx: &Cx<'_>,
    input: &BoxBindInput,
    fd: &FighterData,
) -> Result<(BoxIndex, Vec<f32>), KindError> {
    let mut src = PoseSource::new(cx, &input.mesh, &input.skeleton)?;
    let nb = src.ref_.bones.len();
    let (m_scale, m_z) = (input.mesh_scale, input.mesh_z);
    let shapes = &input.shapes;
    let mut shape_bone: Vec<i32> = Vec::new();
    for sh in shapes {
        shape_bone.push(match &sh.shape.bone {
            None => -1,
            Some(b) => bone_of(&src.ref_, b, &sh.shape.name)?,
        });
    }
    let com = src.ref_.index(COM_BONE);
    let root = bone_of(&src.ref_, "Root", "the root pair")? as usize;
    let under = |ref_: &RefSkeleton, b: i32, top: i32| {
        let mut b = b;
        while b >= 0 {
            if b == top {
                return true;
            }
            b = ref_.parent(b as usize);
        }
        false
    };
    let mut out_shapes: Vec<BoxShape> = shapes
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut o = s.shape.clone();
            o.axes = None;
            o.axes_const = None;
            o.com = (com >= 0 && shape_bone[i] >= 0 && under(&src.ref_, shape_bone[i], com)).then_some(true);
            o
        })
        .collect();
    let mut axis_shapes: Vec<usize> = Vec::new();
    for sk in &input.axis_sockets {
        let Some(i) = shapes.iter().position(|s| s.shape.name == format!("socket:{sk}")) else {
            return fail(format!("boxes {}: no shape socket:{sk} for its axes", input.key));
        };
        axis_shapes.push(i);
    }
    let order = chain_of(
        &src.ref_,
        shape_bone.iter().copied().filter(|b| *b >= 0).chain([root as i32]).chain((com >= 0).then_some(com)),
    );
    let com_rep: HashSet<&str> = input.com_replaced.iter().map(String::as_str).collect();
    let mut com_removed: IndexMap<String, f64> = IndexMap::new(); // clip -> the largest rotation (deg) the Replace took off
    let scale: Vec<Vec<f64>> = shapes.iter().map(|sh| sh.shape.scale.clone().unwrap_or_else(|| vec![1.0; 3])).collect();

    // the skel block (skel.rs)
    let spec = skel_spec(&input.key);
    let mut skel: Option<SkelBlock> = None;
    if let Some(spec) = spec {
        let blend = skel_blend(cx, spec)?;
        let want = skel_wanted(spec, fd, &blend, &input.clips)?;
        skel = Some(SkelBlock::new(&src.ref_, spec, shapes, &shape_bone, com, blend, want)?);
        if shapes.iter().any(|s| s.vb.is_some()) {
            return fail(format!("boxes {}: a socket on a virtual bone with skel tracks (not handled)", input.key));
        }
    }

    // the pose additive (consts.rs POSE_ADDITIVE)
    let mut padd: Option<IndexMap<usize, Vec3>> = None;
    if let Some(pa) = pose_additive(&input.key) {
        if spec.is_some() {
            return fail(format!("boxes {}: a pose additive with skel tracks (not handled)", input.key));
        }
        let seq = pose_additive_seq(cx, pa)?;
        let rides = |bone: usize| {
            shapes.iter().enumerate().any(|(i, sh)| {
                shape_bone[i] >= 0
                    && under(&src.ref_, shape_bone[i], bone as i32)
                    && (sh.shape.kind != BoxKind::Socket
                        || shape_bone[i] != bone as i32
                        || sh.loc.iter().any(|v| *v != 0.0))
            })
        };
        // (rides reads the reference skeleton only: a copy of what it needs keeps src free to sample)
        let ref_parents = src.ref_.parents.clone();
        let rides_ok: Vec<bool> = (0..nb).map(rides).collect();
        let _ = ref_parents;
        padd = Some(src.pose_additive(&seq, &|b| rides_ok[b])?);
    }

    let mut stride = 2 + out_shapes.iter().map(size_of).sum::<usize>();
    let com_offset = if com >= 0 { stride } else { 0 };
    if com >= 0 {
        stride += 2;
    }

    let mut anims: Map<BoxAnim> = Map::new();
    let mut chunks: Vec<Vec<f32>> = Vec::new();
    let mut ax_chunks: Vec<Vec<f64>> = Vec::new();
    let mut floats = 0usize;
    let mut pose = new_pose(nb);
    let mut w = World::new(nb);
    let mut root_first = LocalPose { t: vec![0.0; 3], r: vec![0.0, 0.0, 0.0, 1.0], s: vec![1.0; 3] };
    let mut seen: HashSet<String> = HashSet::new();
    for c in &input.clips {
        let Some(l) = src.load(c)? else { continue };
        let mut cs = src.open(&l)?;
        if !seen.insert(cs.name.clone()) {
            return fail(format!("boxes {}: two sequences named {}", input.key, cs.name));
        }
        let nf = cs.num_frames;
        let lock = cs.lock;
        if lock == Some(RootLock::AnimFirstFrame) {
            src.frame(&mut cs, 0.0, &mut pose);
            first_of(&pose, root, &mut root_first);
        }
        let mut sk = match &mut skel {
            Some(s) if s.wants(&cs.name) => Some(s.begin(&cs, &mut src)?),
            _ => None,
        };
        let mut bx = vec![0f32; nf * stride];
        let mut ax = vec![0f64; nf * 9 * axis_shapes.len()];
        let replace = com_rep.contains(cs.name.as_str());
        for f in 0..nf {
            src.frame(&mut cs, f as f64, &mut pose);
            // the root pair: the root's own translation (after the pose additive), before its lock
            if let Some(padd) = &padd
                && !cs.additive
            {
                for (b, d) in padd {
                    for k in 0..3 {
                        pose.t[b * 3 + k] += d[k];
                    }
                }
            }
            let (rx, rz) = (pose.t[root * 3], pose.t[root * 3 + 2]);
            if let Some(lock) = lock {
                lock_root(lock, &src.ref_, root, &mut pose, &root_first);
            }
            if let (Some(sk), Some(blk)) = (&mut sk, &skel) {
                sk.frame(blk, f, &pose);
            }
            if cs.additive {
                for b in 0..nb {
                    if src.listed[b] {
                        pose.s[b * 3..b * 3 + 3].copy_from_slice(&src.ref_.s[b * 3..b * 3 + 3]);
                    }
                }
            }
            let off = forward(&src.ref_, &order, &pose, &mut w, com, replace);
            if off > *com_removed.get(&cs.name).unwrap_or(&0.0) {
                com_removed.insert(cs.name.clone(), off);
            }
            let o = f * stride;
            bx[o] = (rx * m_scale) as f32;
            bx[o + 1] = (rz * m_scale) as f32;
            write_shapes(shapes, &out_shapes, &shape_bone, &w, m_scale, m_z, &mut bx[o + 2..]);
            if com >= 0 {
                let c = com as usize;
                bx[o + com_offset] = (w.t[c * 3] * m_scale) as f32;
                bx[o + com_offset + 1] = (w.t[c * 3 + 2] * m_scale + m_z) as f32;
            }
            for (a, &i) in axis_shapes.iter().enumerate() {
                let b = shape_bone[i];
                let sh = &shapes[i];
                let wq = if b >= 0 { w.q(b as usize) } else { [f64::NAN; 4] };
                let qa = qmul(wq, sh.srot.unwrap_or(IDENTITY));
                let sc = &scale[i];
                for k in 0..3 {
                    let e = axis(k);
                    let v = qrot(qa, e[0], e[1], e[2]);
                    let ws = if b >= 0 { w.s[b as usize * 3 + k] } else { f64::NAN };
                    let ln = ws.abs() * sc[k] * m_scale;
                    let p = (f * axis_shapes.len() + a) * 9 + k * 3;
                    ax[p] = v[0] * ln;
                    ax[p + 1] = v[2] * ln;
                    ax[p + 2] = v[1] * ln;
                }
            }
        }
        if let (Some(sk), Some(blk)) = (sk, &mut skel) {
            sk.end(blk);
        }
        anims.insert(
            cs.name.clone(),
            BoxAnim {
                num_frames: nf as u32,
                length: cs.length,
                rate_scale: cs.rate_scale,
                root_motion: cs.root_motion,
                offset: floats as u32,
                step: cs.step.then_some(true),
            },
        );
        floats += bx.len();
        chunks.push(bx);
        ax_chunks.push(ax);
    }

    let mut index = BoxIndex {
        mesh_scale: m_scale,
        mesh_z: m_z,
        stride: stride as u32,
        com: Some((com >= 0).then(|| BoxCom { bone: COM_BONE.to_owned(), offset: com_offset as u32 })),
        shapes: Vec::new(),
        anims,
        skel: None,
        socket_axes: None,
        actors: None,
        bone_boxes: None,
    };
    if let Some(skel) = skel {
        let (skel_index, data) = skel.finish(&src.ref_, floats as u32)?;
        index.skel = Some(skel_index);
        floats += data.len();
        chunks.push(data);
    }
    // socket axes: a socket whose axes are the same in every frame (within 1e-5) has them once as axes_const
    if !axis_shapes.is_empty() {
        let na = axis_shapes.len();
        let all: Vec<f64> = ax_chunks.concat();
        let rows = all.len() / (9 * na);
        let mut keep: Vec<usize> = Vec::new();
        for (a, &si) in axis_shapes.iter().enumerate() {
            let mut same = true;
            'rows: for r in 0..rows {
                for k in 0..9 {
                    if (all[(r * na + a) * 9 + k] - all[a * 9 + k]).abs() > 1e-5 {
                        same = false;
                        break 'rows;
                    }
                }
            }
            let sh = &mut out_shapes[si];
            if same {
                sh.axes_const = Some(all[a * 9..a * 9 + 9].iter().map(|v| round_to6(*v)).collect());
            } else {
                sh.axes = Some(keep.len() as u32);
                keep.push(a);
            }
        }
        if !keep.is_empty() {
            index.socket_axes = Some(SocketAxes {
                offset: floats as u32,
                stride: (9 * keep.len()) as u32,
                names: keep.iter().map(|a| input.axis_sockets[*a].clone()).collect(),
            });
            let mut block = Vec::with_capacity(rows * 9 * keep.len());
            for r in 0..rows {
                for &a in &keep {
                    for k in 0..9 {
                        block.push(all[(r * na + a) * 9 + k] as f32);
                    }
                }
            }
            floats += block.len();
            chunks.push(block);
        }
    }
    index.shapes = out_shapes;
    // A clip whose COM rotation the Replace took off (>= 0.05 deg) that a state without the Replace plays too has two
    // poses for one track (not handled); a montage that plays it is only a note there
    let turned: Vec<String> = com_removed.iter().filter(|(_, d)| **d >= 0.05).map(|(c, _)| c.clone()).collect();
    if !turned.is_empty() {
        for (c, users) in com_shared(fd, &turned)? {
            let states: Vec<&String> =
                users.iter().filter(|u| u.starts_with("Anim_") || u.starts_with("BlendSpace_")).collect();
            if !states.is_empty() {
                return fail(format!(
                    "boxes {}: COM Replace: {c} turns {COM_BONE} and a state without the Replace plays it too ({}): not handled",
                    input.key,
                    states.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                ));
            }
        }
    }
    // the actors' own meshes
    if let Some(actors) = input.actors.as_ref().filter(|a| !a.is_empty()) {
        let mut out = Map::new();
        for (cls, a) in actors {
            let (sub, data) = actor_tracks(cx, cls, a, floats)?;
            out.insert(cls.clone(), sub);
            floats += data.len();
            chunks.push(data);
        }
        index.actors = Some(out);
    }
    if let Some(bbs) = &input.bone_boxes {
        let mut all = Map::new();
        for (cls, by) in bbs {
            let mut out = Map::new();
            for (comp, bb) in by {
                out.insert(comp.clone(), bone_box_track(cx, &format!("{cls}.{comp}"), bb)?);
            }
            all.insert(cls.clone(), out);
        }
        index.bone_boxes = Some(all);
    }
    let _ = floats;
    Ok((index, chunks.concat()))
}

/// The root's (t, r, s) of a pose (AnimFirstFrame's lock).
fn first_of(pose: &LocalPose, root: usize, first: &mut LocalPose) {
    first.t.copy_from_slice(&pose.t[root * 3..root * 3 + 3]);
    first.r.copy_from_slice(&pose.r[root * 4..root * 4 + 4]);
    first.s.copy_from_slice(&pose.s[root * 3..root * 3 + 3]);
}

/// `Number(v.toFixed(6))` for the index's axes_const (a 0 is +0, a small negative -0, which the index's JSON round
/// trip turns to 0).
fn round_to6(v: f64) -> f64 {
    if v == 0.0 {
        return 0.0;
    }
    let s = fixed_abs(v.abs(), 6, false);
    let n: f64 = s.parse().unwrap_or(f64::NAN);
    if v < 0.0 { -n } else { n }
}

/// |v| (finite, at least about 1e-18 where it matters) to `d` decimals from its exact binary value: a tie to even
/// (`half_even`, Python's '%.nf') or up (toFixed's larger n).
fn fixed_abs(a: f64, d: usize, half_even: bool) -> String {
    let exact = format!("{a:.80}"); // every digit of a double of this size
    let dot = exact.find('.').unwrap_or(exact.len());
    let digits = format!("{}{}", &exact[..dot], &exact[dot + 1..dot + 1 + d]);
    let rest = &exact[dot + 1 + d..];
    let r0 = rest.as_bytes()[0];
    let odd = (digits.as_bytes()[digits.len() - 1] - b'0') % 2 == 1;
    let tie_up = !half_even || odd;
    let up = r0 > b'5' || (r0 == b'5' && (rest[1..].bytes().any(|c| (b'1'..=b'9').contains(&c)) || tie_up));
    let n = if up { (digits.parse::<u128>().unwrap_or(0) + 1).to_string() } else { digits };
    let n = format!("{n:0>w$}", w = d + 1);
    format!("{}.{}", &n[..n.len() - d], &n[n.len() - d..])
}

/// Bone `root`'s (t, r, s) replaced by the lock.
fn lock_root(lock: RootLock, ref_: &RefSkeleton, root: usize, p: &mut LocalPose, first: &LocalPose) {
    let (t3, t4) = (root * 3, root * 4);
    for k in 0..3 {
        p.t[t3 + k] = match lock {
            RootLock::AnimFirstFrame => first.t[k],
            RootLock::Zero => 0.0,
            RootLock::RefPose => ref_.t[t3 + k],
        };
        p.s[t3 + k] = match lock {
            RootLock::AnimFirstFrame => first.s[k],
            RootLock::Zero => 1.0,
            RootLock::RefPose => ref_.s[t3 + k],
        };
    }
    for k in 0..4 {
        p.r[t4 + k] = match lock {
            RootLock::AnimFirstFrame => first.r[k],
            RootLock::Zero => {
                if k == 3 {
                    1.0
                } else {
                    0.0
                }
            }
            RootLock::RefPose => ref_.r[t4 + k],
        };
    }
}

/// Every shape of a frame into `bx` (from its start).
fn write_shapes(
    shapes: &[Placed],
    out: &[BoxShape],
    shape_bone: &[i32],
    w: &World,
    m_scale: f64,
    m_z: f64,
    bx: &mut [f32],
) {
    let mut o = 0;
    for (i, sh) in shapes.iter().enumerate() {
        let b = shape_bone[i];
        let (mut bx_, mut by_, mut bz_) = (0.0, 0.0, 0.0);
        let (mut sx, mut sy, mut sz) = (1.0, 1.0, 1.0);
        let mut wr = IDENTITY;
        if b >= 0 {
            let b = b as usize;
            bx_ = w.t[b * 3];
            by_ = w.t[b * 3 + 1];
            bz_ = w.t[b * 3 + 2];
            sx = w.s[b * 3];
            sy = w.s[b * 3 + 1];
            sz = w.s[b * 3 + 2];
            wr = w.q(b);
        }
        let vb = sh.vb;
        if let Some(add) = vb.and_then(|v| v.add) {
            // the ModifyBone node's bone-space translation: turned and scaled by the bone's input transform
            let v = qrot(wr, add[0] * sx, add[1] * sy, add[2] * sz);
            bx_ += v[0];
            by_ += v[1];
            bz_ += v[2];
        }
        if let Some(v) = vb {
            // the node's Scale in place of the bone's
            [sx, sy, sz] = v.scale;
        }
        let v = qrot(wr, sh.loc[0] * sx, sh.loc[1] * sy, sh.loc[2] * sz);
        let (cx, cy, cz) = if vb.is_some_and(|v| v.world) {
            // the bone's world scale is Scale: the offset takes no mesh scale (and no actor scale: 'unscaled')
            (bx_ + v[0] / m_scale, by_ + v[1] / m_scale, bz_ + v[2] / m_scale)
        } else {
            (bx_ + v[0], by_ + v[1], bz_ + v[2])
        };
        let (cf, cu, cd) = if sh.shape.on_capsule == Some(true) {
            // capsule space: no mesh scale, no mesh place
            (cx, cz, cy)
        } else {
            (cx * m_scale, cz * m_scale + m_z, cy * m_scale)
        };
        let abs_scale = sh.shape.abs_scale == Some(true);
        if abs_scale {
            (sx, sy, sz) = (1.0, 1.0, 1.0);
        }
        let os = &out[i];
        if os.is_box == Some(true) {
            bx[o] = cf as f32;
            bx[o + 1] = cu as f32;
            bx[o + 2] = cd as f32;
            let es = if abs_scale { 1.0 } else { m_scale };
            let qa = qmul(wr, sh.rot);
            let extent = os.extent.as_deref().unwrap_or(&[]);
            for k in 0..3 {
                let (vf, vu, vd) = if os.abs_rot == Some(true) {
                    (WORLD_AXES[k * 3], WORLD_AXES[k * 3 + 1], WORLD_AXES[k * 3 + 2])
                } else {
                    let e = axis(k);
                    let v = qrot(qa, e[0], e[1], e[2]);
                    (v[0], v[2], v[1])
                };
                let s = [sx, sy, sz][k];
                let ln = extent.get(k).copied().unwrap_or(f64::NAN) * es * s.abs();
                bx[o + 3 + k * 3] = (vf * ln) as f32;
                bx[o + 4 + k * 3] = (vu * ln) as f32;
                bx[o + 5 + k * 3] = (vd * ln) as f32;
            }
        } else if os.kind == BoxKind::Socket {
            bx[o] = cf as f32;
            bx[o + 1] = cu as f32;
        } else {
            round_vals(sh, wr, sx, sy, sz, cf, cu, cd, &mut bx[o..]);
        }
        o += size_of(os);
    }
}

/// POSE_ADDITIVE's sequence from the AnimBP's class default: the ApplyAdditive node at a constant alpha of 1 and the
/// SequencePlayer on its Additive pin.
fn pose_additive_seq(cx: &Cx<'_>, pa: &PoseAdditive) -> Result<String, KindError> {
    let cdo = cx.object(&cdo_path(pa.anim_bp))?;
    let p = Value::Obj(defaults_or_properties(&cdo)?);
    let (Some(node), Some(player)) = (get(&p, pa.node), get(&p, pa.player)) else {
        return fail(format!("boxes: {}: no {} or {}", pa.anim_bp, pa.node, pa.player));
    };
    let enum_of = |v: Option<&Value>, d: &str| match v {
        Some(Value::Str(s)) => s.rsplit("::").next().unwrap_or("").to_owned(),
        _ => d.to_owned(),
    };
    let sb = get(node, "AlphaScaleBias").and_then(Value::as_obj);
    let sbc = get(node, "AlphaScaleBiasClamp").and_then(Value::as_obj);
    let sb_bad = sb.is_some_and(|sb| {
        !(sb.is_empty() || (sb.get("Scale") == Some(&Value::Num(1.0)) && sb.get("Bias") == Some(&Value::Num(0.0))))
    });
    let sbc_bad =
        sbc.is_some_and(|c| ["bMapRange", "bClampResult", "bInterpResult"].iter().any(|k| super::truthy(c.get(*k))));
    let curve = get(node, "AlphaCurveName").filter(|v| !v.is_null()).and_then(Value::as_str).unwrap_or("None");
    if get(node, "Alpha") != Some(&Value::Num(1.0))
        || enum_of(get(node, "AlphaInputType"), "Float") != "Float"
        || curve != "None"
        || sb_bad
        || sbc_bad
    {
        return fail(format!("boxes: {}: {} has an alpha other than a constant 1 (not handled)", pa.anim_bp, pa.node));
    }
    match get(player, "Sequence") {
        Some(Value::Str(s)) => Ok(s.clone()),
        _ => fail(format!("boxes: {}: {} plays no sequence", pa.anim_bp, pa.player)),
    }
}

/// One class's actor tracks: its sub-index (offsets from the section's start, `base` floats before its first) and
/// its floats.
fn actor_tracks(
    cx: &Cx<'_>,
    cls: &str,
    a: &ActorBindInput,
    base: usize,
) -> Result<(ActorBoxIndex, Vec<f32>), KindError> {
    let mut src = PoseSource::new(cx, &a.mesh, &a.skeleton)?;
    let (mesh_scale, mesh_z) = (a.mesh_scale, a.mesh_z);
    let nb = src.ref_.bones.len();
    let mut shape_bone = Vec::new();
    for sh in &a.shapes {
        let b = sh.shape.bone.as_deref().unwrap_or("");
        shape_bone.push(bone_of(&src.ref_, b, &format!("{cls}.{}", sh.shape.name))? as usize);
    }
    let root_bone = 0usize;
    let order = chain_of(&src.ref_, shape_bone.iter().map(|b| *b as i32).chain([root_bone as i32]));
    let stride = 2 + a.shapes.iter().map(|sh| if sh.shape.capsule { 7 } else { 4 }).sum::<usize>();
    let mut anims = Map::new();
    let mut chunks: Vec<Vec<f32>> = Vec::new();
    let mut floats = 0usize;
    let mut pose = new_pose(nb);
    let mut w = World::new(nb);
    let mut first = LocalPose { t: vec![0.0; 3], r: vec![0.0, 0.0, 0.0, 1.0], s: vec![1.0; 3] };
    let root = src.ref_.index("Root");
    let mut clips = a.clips.clone();
    clips.sort_by(|x, y| x.name.cmp(&y.name));
    for c in &clips {
        let Some(l) = src.load(c)? else { return fail(format!("boxes: {cls}: {} is not on {}", c.path, a.skeleton)) };
        let mut cs = src.open(&l)?;
        if cs.additive {
            return fail(format!("boxes: {cls}: {} is additive (not handled)", c.name));
        }
        if cs.lock.is_some() && root < 0 {
            return fail(format!("boxes: {cls}: a root lock and no bone Root"));
        }
        if cs.lock == Some(RootLock::AnimFirstFrame) {
            src.frame(&mut cs, 0.0, &mut pose);
            first_of(&pose, root as usize, &mut first);
        }
        let nf = cs.num_frames;
        let mut bx = vec![0f32; nf * stride];
        for f in 0..nf {
            src.frame(&mut cs, f as f64, &mut pose);
            if let Some(lock) = cs.lock {
                lock_root(lock, &src.ref_, root as usize, &mut pose, &first);
            }
            forward(&src.ref_, &order, &pose, &mut w, -1, false);
            let mut o = f * stride;
            bx[o] = (w.t[root_bone * 3] * mesh_scale) as f32;
            bx[o + 1] = (w.t[root_bone * 3 + 2] * mesh_scale) as f32;
            o += 2;
            for (i, sh) in a.shapes.iter().enumerate() {
                let b = shape_bone[i];
                let (sx, sy, sz) = (w.s[b * 3], w.s[b * 3 + 1], w.s[b * 3 + 2]);
                let v = qrot(w.q(b), sh.loc[0] * sx, sh.loc[1] * sy, sh.loc[2] * sz);
                let (cx_, cy, cz) = (w.t[b * 3] + v[0], w.t[b * 3 + 1] + v[1], w.t[b * 3 + 2] + v[2]);
                round_vals(
                    sh,
                    w.q(b),
                    sx,
                    sy,
                    sz,
                    cx_ * mesh_scale,
                    cz * mesh_scale + mesh_z,
                    cy * mesh_scale,
                    &mut bx[o..],
                );
                o += if sh.shape.capsule { 7 } else { 4 };
            }
        }
        anims.insert(
            cs.name.clone(),
            BoxAnim {
                num_frames: nf as u32,
                length: cs.length,
                rate_scale: cs.rate_scale,
                root_motion: cs.root_motion,
                offset: (base + floats) as u32,
                step: cs.step.then_some(true),
            },
        );
        floats += bx.len();
        chunks.push(bx);
    }
    let sub = ActorBoxIndex {
        mesh: a.mesh_comp.clone(),
        mesh_scale,
        mesh_z,
        stride: stride as u32,
        com: Some(None),
        shapes: a.shapes.iter().map(|s| s.shape.clone()).collect(),
        anims,
        skel: None,
        socket_axes: None,
    };
    Ok((sub, chunks.concat()))
}

/// Python's '%.3f' % v (the exact binary value rounded half to even), its trailing zeros and point stripped, '0'
/// under 5e-4.
fn py_fixed3(v: f64) -> String {
    if v.abs() < 5e-4 {
        return "0".to_owned();
    }
    let s = fixed_abs(v.abs(), 3, true);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{}{s}", if v < 0.0 { "-" } else { "" })
}

/// Python's repr of a float (the shortest round trip, '.0' on an integer; no exponent form is met here).
fn py_repr(v: f64) -> Result<String, KindError> {
    let t = ryu_js::Buffer::new().format(v).to_owned();
    if t.contains('e') {
        return fail(format!("boxes: {v}: an exponent form (not handled)"));
    }
    Ok(if v.fract() == 0.0 && v.is_finite() { format!("{t}.0") } else { t })
}

/// The bone box tracks of the ACTOR_BONE_BOXES classes `actors` holds (index.rs bone_box_inputs: `mesh_of` gives a
/// mesh component's SkeletalMesh), sampled as the boxes bind samples a fighter's: by class and component, each
/// component's track (the items part's, whose actor defs no boxes entry has).
pub fn actor_bone_boxes(
    cx: &Cx<'_>,
    actors: &Map<ActorDef>,
    mesh_of: &MeshOf<'_>,
) -> Result<Map<Map<String>>, KindError> {
    let mut out = Map::new();
    for (cls, by) in bone_box_inputs(cx, Some(actors), mesh_of)?.unwrap_or_default() {
        let mut m = Map::new();
        for (comp, bb) in &by {
            m.insert(comp.clone(), bone_box_track(cx, &format!("{cls}.{comp}"), bb)?);
        }
        out.insert(cls, m);
    }
    Ok(out)
}

/// ACTOR_BONE_BOXES: a BoxComponent on a bone of a spawned actor's mesh, per frame of each clip: the bone's FK (the
/// clip's root lock on Root), the component's RelativeLocation x the bone's scale turned by its rotation, its axes
/// the bone's, each half extent x |the bone's scale on that axis|. Mesh space in UE units before the mesh's scale
/// and place: fwd = mesh X, up = mesh Z, depth = mesh Y. One string: clips joined by ';', each 'name,length,frames,'
/// and per frame 12 numbers (the centre, then the half extent vectors of local X, Y, Z).
fn bone_box_track(cx: &Cx<'_>, what: &str, bb: &BoneBoxInput) -> Result<String, KindError> {
    let mut src = PoseSource::new(cx, &bb.mesh, &bb.skeleton)?;
    let bone = bone_of(&src.ref_, &bb.bone, what)? as usize;
    let order = chain_of(&src.ref_, [bone as i32]);
    let root = src.ref_.index("Root");
    let nb = src.ref_.bones.len();
    let mut pose = new_pose(nb);
    let mut w = World::new(nb);
    let mut first = LocalPose { t: vec![0.0; 3], r: vec![0.0, 0.0, 0.0, 1.0], s: vec![1.0; 3] };
    let mut out: Vec<String> = Vec::new();
    for c in &bb.clips {
        let Some(l) = src.load(c)? else { continue };
        let mut cs = src.open(&l)?;
        let (n, ln) = (cs.num_frames, cs.length);
        if ((n as f64 - 1.0) / 60.0 - ln).abs() > 1e-5 || cs.additive || cs.step {
            return fail(format!("boxes: {what}: {} is not a 60 fps full-pose linear clip (not handled)", c.path));
        }
        if cs.lock.is_some() && root < 0 {
            return fail(format!("boxes: {what}: a root lock and no bone Root"));
        }
        if cs.lock == Some(RootLock::AnimFirstFrame) {
            src.frame(&mut cs, 0.0, &mut pose);
            first_of(&pose, root as usize, &mut first);
        }
        let mut vals: Vec<String> = vec![cs.name.clone(), py_repr(ln)?, n.to_string()];
        for f in 0..n {
            src.frame(&mut cs, f as f64, &mut pose);
            if let Some(lock) = cs.lock {
                lock_root(lock, &src.ref_, root as usize, &mut pose, &first);
            }
            forward(&src.ref_, &order, &pose, &mut w, -1, false);
            let b3 = bone * 3;
            let (sx, sy, sz) = (w.s[b3], w.s[b3 + 1], w.s[b3 + 2]);
            let v = qrot(w.q(bone), bb.loc[0] * sx, bb.loc[1] * sy, bb.loc[2] * sz);
            let (cx_, cy, cz) = (w.t[b3] + v[0], w.t[b3 + 1] + v[1], w.t[b3 + 2] + v[2]);
            vals.extend([py_fixed3(cx_), py_fixed3(cz), py_fixed3(cy)]);
            let sc = [sx, sy, sz];
            for k in 0..3 {
                let e = axis(k);
                let v = qrot(w.q(bone), e[0], e[1], e[2]);
                let ex = bb.extent[k] * sc[k].abs();
                vals.extend([py_fixed3(v[0] * ex), py_fixed3(v[2] * ex), py_fixed3(v[1] * ex)]);
            }
        }
        out.push(vals.join(","));
    }
    if out.is_empty() {
        return fail(format!("boxes: {what}: no clip of {}", bb.mesh));
    }
    Ok(out.join(";"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_formats() {
        assert_eq!(py_fixed3(0.0004), "0");
        assert_eq!(py_fixed3(-0.0004), "0");
        assert_eq!(py_fixed3(1.0), "1");
        assert_eq!(py_fixed3(-2.5), "-2.5");
        assert_eq!(py_fixed3(0.0005), "0.001");
        assert_eq!(py_fixed3(0.0015), "0.002");
        assert_eq!(py_fixed3(1.2345), "1.234"); // 1.2345 is below the half in binary
        assert_eq!(py_fixed3(0.125), "0.125");
        assert_eq!(py_fixed3(9.9996), "10");
        assert_eq!(py_repr(1.0).unwrap(), "1.0");
        assert_eq!(py_repr(0.5).unwrap(), "0.5");
        assert_eq!(py_repr(1.0333333333333334).unwrap(), "1.0333333333333334");
        assert_eq!(round_to6(0.1234565), 0.123456); // 0.1234565 is below the half in binary
        assert_eq!(round_to6(0.0078125), 0.007813); // a tie in binary: toFixed takes the larger
        assert_eq!(round_to6(-0.0078125), -0.007813);
        assert_eq!(py_fixed3(0.0625), "0.062"); // a tie: Python takes the even
        assert_eq!(round_to6(-0.0000001).to_bits(), (-0.0f64).to_bits());
    }
}
