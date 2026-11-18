//! The animation nodes over the sampled clips (clip sampling, head scale, additive accumulate):
//! the view's pose in the view's space (metres, Y up: the bone-local transforms of a rig's bones), the clips sampled
//! into it, the blends and the additive accumulate. Blend spaces, virtual bones and the rotator nodes come with the
//! characters whose graphs use them.

use std::sync::Arc;

use indexmap::IndexMap;
use mvs_anim::clip::{AnimClip, ROOT_LOCK_FIRST_FRAME, ROOT_LOCK_REF_POSE, ROOT_LOCK_ZERO};
use mvs_anim::pose::BonePose;

use crate::mesh::SkeletonEntry;
use crate::{Error, Result};

// The additive thresholds: DAT_143b3a298, DAT_143ce1700, DAT_143b46c24.
const ADD_MIN: f64 = 1e-5;
const ADD_FULL: f64 = 0.99999;
const ADD_BASE_MIN: f64 = 1e-8;

/// EAdditiveAnimationType.
pub const ADDITIVE_LOCAL: f64 = 1.0;
pub const ADDITIVE_MESH: f64 = 2.0;

/// `Pose`: per bone of a rig, its local translation (m), rotation and scale in the view's
/// space, kept in float32.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pose {
    pub n: usize,
    pub t: Vec<f32>,
    pub r: Vec<f32>,
    pub s: Vec<f32>,
}

impl Pose {
    pub fn new(n: usize) -> Pose {
        Pose { n, t: vec![0.0; n * 3], r: vec![0.0; n * 4], s: vec![0.0; n * 3] }
    }

    pub fn copy(&mut self, o: &Pose) {
        self.t.copy_from_slice(&o.t);
        self.r.copy_from_slice(&o.r);
        self.s.copy_from_slice(&o.s);
    }

    /// Bone `i`'s values of `src`.
    pub fn copy_bone(&mut self, src: &Pose, i: usize) {
        self.t[i * 3..i * 3 + 3].copy_from_slice(&src.t[i * 3..i * 3 + 3]);
        self.s[i * 3..i * 3 + 3].copy_from_slice(&src.s[i * 3..i * 3 + 3]);
        self.r[i * 4..i * 4 + 4].copy_from_slice(&src.r[i * 4..i * 4 + 4]);
    }

    pub fn quat(&self, i: usize) -> [f64; 4] {
        let r = &self.r[i * 4..i * 4 + 4];
        [f64::from(r[0]), f64::from(r[1]), f64::from(r[2]), f64::from(r[3])]
    }
}

/// `nlerp`: normalised lerp along the shorter arc, `a` the weight of `qb`.
fn nlerp(qa: &[f32], qb: &[f32], a: f64, out: &mut [f32]) {
    let f = |v: f32| f64::from(v);
    let dot = f(qa[0]) * f(qb[0]) + f(qa[1]) * f(qb[1]) + f(qa[2]) * f(qb[2]) + f(qa[3]) * f(qb[3]);
    let s = if dot < 0.0 { -a } else { a };
    let x = f(qa[0]) * (1.0 - a) + f(qb[0]) * s;
    let y = f(qa[1]) * (1.0 - a) + f(qb[1]) * s;
    let z = f(qa[2]) * (1.0 - a) + f(qb[2]) * s;
    let w = f(qa[3]) * (1.0 - a) + f(qb[3]) * s;
    let l = (x * x + y * y + z * z + w * w).sqrt();
    let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
    out[0] = (x / l) as f32;
    out[1] = (y / l) as f32;
    out[2] = (z / l) as f32;
    out[3] = (w / l) as f32;
}

/// `blendPose`: out = blend(a, b, w), w the weight of b.
pub fn blend_pose(a: &Pose, b: &Pose, w: f64, out: &mut Pose) {
    let lerp = |x: f32, y: f32| (f64::from(x) + (f64::from(y) - f64::from(x)) * w) as f32;
    for i in 0..a.t.len() {
        out.t[i] = lerp(a.t[i], b.t[i]);
        out.s[i] = lerp(a.s[i], b.s[i]);
    }
    for i in 0..a.n {
        let o = i * 4;
        nlerp(&a.r[o..o + 4], &b.r[o..o + 4], w, &mut out.r[o..o + 4]);
    }
}

/// `quatMul` into float32 (the result lands in a float32 scratch).
fn quat_mul32(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    let r = |v: f64| f64::from(v as f32);
    [
        r(aw * bx + ax * bw + ay * bz - az * by),
        r(aw * by - ax * bz + ay * bw + az * bx),
        r(aw * bz + ax * by - ay * bx + az * bw),
        r(aw * bw - ax * bx - ay * by - az * bz),
    ]
}

// ---- the clips --------------------------------------------------------------------------------------------------

/// A clip entry's bytes for the process: an AnimClip borrows its entry, and a view's clips live as long as the match
/// that loaded them. Each entry is read once per store (the store's cache: a second match, or a preview, of the same
/// store reads nothing; a store opened again after an import reads it again).
// Deviation: the bytes stay for the process (nothing frees a clip with its scope); mvs-anim's AnimClip has no
// owned form.
fn interned(store: &mvs_store::Store, pack: &mvs_store::Pack, path: &str) -> Result<&'static [u8]> {
    let b = store.cached(
        &format!("animation:{path}"),
        || -> Result<&'static [u8]> { Ok(Box::leak(pack.bytes(path)?.into_boxed_slice())) },
        |_| 0,
    )?;
    Ok(*b)
}

/// A Skeleton entry, parsed and checked once per store.
fn skeleton(store: &mvs_store::Store, pack: &mvs_store::Pack, path: &str) -> Result<Arc<SkeletonEntry>> {
    store.cached(
        &format!("skeleton:{path}"),
        || {
            let e: SkeletonEntry = serde_json::from_slice(&pack.bytes(path)?)
                .map_err(|e| Error::Format { what: path.to_owned(), why: e.to_string() })?;
            e.check()?;
            Ok(e)
        },
        |_| 0,
    )
}

// RenderClip.bones flags, per Skeleton bone.
const TRACKED: u8 = 1;
const NO_SCALE: u8 = 2;

/// `RenderClip`: an AnimSequence's animation entry with its Skeleton.
pub struct RenderClip {
    /// The asset name (the clips' key).
    pub name: String,
    pub anim: AnimClip<'static>,
    /// Its Skeleton, an index into [`Clips`]' skeletons.
    pub skel: usize,
    pub additive: bool,
    /// Per Skeleton bone (and past it, to the clip's last track bone): TRACKED, NO_SCALE.
    bones: Vec<u8>,
}

impl std::fmt::Debug for RenderClip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderClip").field("name", &self.name).field("skel", &self.skel).finish_non_exhaustive()
    }
}

/// A Skeleton the clips are of, with the scratch poses a sample is made in: the reference pose in
/// UE space for its bones, identity past them (the track slots of virtual bones), and the last sample out holds.
#[derive(Debug)]
pub struct ClipSkeleton {
    pub entry: SkeletonEntry,
    ref_: BonePose,
    out: BonePose,
    last: Option<(usize, f64, bool)>,
}

impl ClipSkeleton {
    fn new(entry: SkeletonEntry, bones: usize) -> ClipSkeleton {
        let nb = entry.bones.len();
        let n = bones.max(nb);
        let mut ref_ = BonePose::new(n);
        for i in 0..n {
            let o = i * 10;
            for k in 0..3 {
                ref_.t[i * 3 + k] = if i < nb { entry.pose[o + k] } else { 0.0 };
                ref_.s[i * 3 + k] = if i < nb { entry.pose[o + 7 + k] } else { 1.0 };
            }
            for k in 0..4 {
                ref_.r[i * 4 + k] = if i < nb {
                    entry.pose[o + 3 + k]
                } else if k == 3 {
                    1.0
                } else {
                    0.0
                };
            }
        }
        ClipSkeleton { entry, ref_, out: BonePose::new(n), last: None }
    }
}

/// A character's clips by name (loaded over the character's clip map) and their Skeletons.
#[derive(Debug, Default)]
pub struct Clips {
    pub skeletons: Vec<ClipSkeleton>,
    pub clips: Vec<RenderClip>,
    pub by_name: IndexMap<String, usize>,
}

impl Clips {
    /// The clips `paths` names (clip name -> AnimSequence object path), with their Skeletons, from `store`.
    pub fn load<'p>(store: &mvs_store::Store, paths: impl IntoIterator<Item = (&'p str, &'p str)>) -> Result<Clips> {
        let anims = store.kind("animation")?;
        let skels = store.kind("skeleton")?;
        let mut out = Clips::default();
        let mut skel_entries: Vec<SkeletonEntry> = Vec::new();
        let mut skel_bones: Vec<usize> = Vec::new();
        let fmt = |what: &str, why: String| Error::Format { what: what.to_owned(), why };
        for (name, path) in paths {
            let bytes = interned(store, &anims, path)?;
            let anim = AnimClip::new(bytes, path).map_err(|e| fmt(path, e))?;
            let skel = match skel_entries.iter().position(|s| s.path == anim.skeleton) {
                Some(k) => k,
                None => {
                    skel_entries.push(SkeletonEntry::clone(&*skeleton(store, &skels, &anim.skeleton)?));
                    skel_bones.push(0);
                    skel_entries.len() - 1
                }
            };
            let mut bones = vec![0u8; skel_entries[skel].bones.len().max(anim.bone_end)];
            for i in 0..anim.tracks {
                bones[anim.track_bone(i) as usize] = TRACKED | if anim.track_has_scale(i) { 0 } else { NO_SCALE };
            }
            skel_bones[skel] = skel_bones[skel].max(bones.len());
            let additive = anim.additive_type != 0.0;
            out.by_name.insert(name.to_owned(), out.clips.len());
            out.clips.push(RenderClip { name: asset_name(path).to_owned(), anim, skel, additive, bones });
        }
        out.skeletons = skel_entries.into_iter().zip(skel_bones).map(|(e, n)| ClipSkeleton::new(e, n)).collect();
        Ok(out)
    }

    pub fn get(&self, name: &str) -> Option<usize> {
        self.by_name.get(name).copied()
    }
}

/// `assetName`: an object path's object name.
pub fn asset_name(path: &str) -> &str {
    path.rsplit_once('.').map_or(path, |(_, n)| n)
}

// ---- rig binding ------------------------------------------------------------------------------------------------

/// A rig's bones bound to each Skeleton of the clips: per rig bone its Skeleton bone (-1 none), and the
/// rig's hierarchy for the mesh-space additive rule.
#[derive(Debug)]
struct RigSkel {
    skel: Vec<i32>,
    mesh: MeshRig,
}

/// A mesh rig: each rig bone's parent (the nearest Skeleton ancestor the rig has; -1 a root) and the rig's bones
/// parents first.
#[derive(Debug)]
struct MeshRig {
    parents: Vec<i32>,
    order: Vec<usize>,
}

/// A view's rig: its bone names (the main mesh's reference skeleton) bound to the clips' Skeletons, and per clip the
/// rig bones it tracks.
#[derive(Debug)]
pub struct Rig {
    pub names: Vec<String>,
    skels: Vec<RigSkel>,
    tracked: Vec<Vec<usize>>,
}

impl Rig {
    pub fn bind(clips: &Clips, names: Vec<String>) -> Rig {
        let skels: Vec<RigSkel> = clips
            .skeletons
            .iter()
            .map(|cs| {
                let sk = &cs.entry;
                let map: Vec<i32> = names.iter().map(|n| sk.bone_index(n).map_or(-1, |i| i as i32)).collect();
                let n = map.len();
                let mut rig_of_skel = vec![-1i32; sk.bones.len()];
                for (i, &s) in map.iter().enumerate() {
                    if s >= 0 && (s as usize) < rig_of_skel.len() && rig_of_skel[s as usize] == -1 {
                        rig_of_skel[s as usize] = i as i32;
                    }
                }
                let mut parents = vec![-1i32; n];
                for (i, &s) in map.iter().enumerate() {
                    if s < 0 || s as usize >= rig_of_skel.len() {
                        continue;
                    }
                    let mut s = sk.parents[s as usize];
                    while s >= 0 && rig_of_skel[s as usize] == -1 {
                        s = sk.parents[s as usize];
                    }
                    if s >= 0 {
                        parents[i] = rig_of_skel[s as usize];
                    }
                }
                let mut order: Vec<usize> = (0..n).collect();
                order.sort_by_key(|&i| (map[i], i));
                RigSkel { skel: map, mesh: MeshRig { parents, order } }
            })
            .collect();
        let tracked = clips
            .clips
            .iter()
            .map(|c| {
                let map = &skels[c.skel].skel;
                (0..names.len())
                    .filter(|&i| {
                        let s = map[i];
                        s >= 0 && (s as usize) < c.bones.len() && c.bones[s as usize] & TRACKED != 0
                    })
                    .collect()
            })
            .collect();
        Rig { names, skels, tracked }
    }

    pub fn index(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }

    /// `trackedBones`: the rig bones clip `c` has a track for, in rig order.
    pub fn tracked(&self, c: usize) -> &[usize] {
        &self.tracked[c]
    }
}

// ---- sampling ---------------------------------------------------------------------------------------------------

/// `sampleClip`: clip `c` at `time` into `out` over the rig (rest: the rig's reference pose, for the bones the clip
/// does not track). `root_index >= 0`: root motion is extracted (a bEnableRootMotion clip locks its root); -1: not (a
/// bForceRootLock clip still does). An additive clip gives its delta.
pub fn sample_clip(clips: &mut Clips, c: usize, rig: &Rig, rest: &Pose, time: f64, out: &mut Pose, root_index: i32) {
    let Clips { skeletons, clips, .. } = clips;
    let clip = &mut clips[c];
    let st = &mut skeletons[clip.skel];
    let extract = root_index >= 0;
    if st.last != Some((c, time, extract)) {
        clip.anim.sample(time, &st.ref_, &mut st.out, extract, false);
        st.last = Some((c, time, extract));
    }
    let a = &clip.anim;
    let lock = if a.force_root_lock || (extract && a.root_motion) { a.root_lock } else { -1.0 };
    let additive = clip.additive;
    let flags = &clip.bones;
    let map = &rig.skels[clip.skel].skel;
    let p = &st.out;
    for (i, &s) in map.iter().enumerate() {
        let f = if s >= 0 && (s as usize) < flags.len() { flags[s as usize] } else { 0 };
        let (t3, q4) = (i * 3, i * 4);
        // (AnimFirstFrame: the root track at time 0, written by the sample as any track)
        if s == 0 && lock >= 0.0 && lock != ROOT_LOCK_FIRST_FRAME {
            if lock == ROOT_LOCK_ZERO {
                out.t[t3..t3 + 3].fill(0.0);
                out.r[q4..q4 + 4].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
                out.s[t3..t3 + 3].fill(if additive { 0.0 } else { 1.0 });
            } else if lock == ROOT_LOCK_REF_POSE {
                out.copy_bone(rest, i);
                if additive {
                    for k in 0..3 {
                        out.s[t3 + k] -= 1.0;
                    }
                }
            } else {
                panic!("{}: RootMotionRootLock {lock}", a.path);
            }
            continue;
        }
        if f & TRACKED == 0 {
            if additive {
                out.t[t3..t3 + 3].fill(0.0);
                out.r[q4..q4 + 4].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
                out.s[t3..t3 + 3].fill(0.0);
            } else {
                out.copy_bone(rest, i);
            }
            continue;
        }
        let (a3, a4) = (s as usize * 3, s as usize * 4);
        out.r[q4] = p.r[a4];
        out.r[q4 + 1] = p.r[a4 + 2];
        out.r[q4 + 2] = p.r[a4 + 1];
        out.r[q4 + 3] = -p.r[a4 + 3];
        // CM = Math.fround(0.01): a float32 times it, the product rounded to float32 by the store
        let cm = |v: f32| (f64::from(v) * f64::from(0.01f32)) as f32;
        out.t[t3] = cm(p.t[a3]);
        out.t[t3 + 1] = cm(p.t[a3 + 2]);
        out.t[t3 + 2] = cm(p.t[a3 + 1]);
        if f & NO_SCALE != 0 {
            // no scale stream: the start pose's (the rig's rest; the additive identity's 0)
            if additive {
                out.s[t3..t3 + 3].fill(0.0);
            } else {
                out.s[t3..t3 + 3].copy_from_slice(&rest.s[t3..t3 + 3]);
            }
        } else {
            out.s[t3] = p.s[a3];
            out.s[t3 + 1] = p.s[a3 + 2];
            out.s[t3 + 2] = p.s[a3 + 1];
        }
    }
}

// ---- additives --------------------------------------------------------------------------------------------------

/// `applyAdditive`: FAnimNode_ApplyAdditive at alpha 1, AccumulateAdditivePose at weight 1 by the clip's type. `ta`: a
/// scratch pose.
pub fn apply_additive(clips: &mut Clips, c: usize, rig: &Rig, rest: &Pose, time: f64, out: &mut Pose, ta: &mut Pose) {
    accumulate_additive(clips, c, rig, rest, time, 1.0, true, out, ta);
}

/// `accumulateAdditive`: AccumulateAdditivePose of the clip's delta at `time` and weight `w`.
#[allow(clippy::too_many_arguments)]
pub fn accumulate_additive(
    clips: &mut Clips,
    c: usize,
    rig: &Rig,
    rest: &Pose,
    time: f64,
    w: f64,
    scale: bool,
    out: &mut Pose,
    ta: &mut Pose,
) {
    if !(w > ADD_MIN) {
        return;
    }
    assert!(clips.clips[c].additive, "{} is not an additive sequence", clips.clips[c].anim.path);
    sample_clip(clips, c, rig, rest, time, ta, -1);
    let clip = &clips.clips[c];
    let mesh = (clip.anim.additive_type == ADDITIVE_MESH).then(|| &rig.skels[clip.skel].mesh);
    accumulate_held_delta(ta, &rig.tracked[c], w, scale, out, mesh);
}

/// `accumulateHeldDelta` of a local-space delta (`mesh` null): a held delta `d` on the bones listed, at weight `w`.
pub fn accumulate_held(d: &Pose, bones: &[usize], w: f64, scale: bool, out: &mut Pose) {
    accumulate_held_delta(d, bones, w, scale, out, None);
}

/// `accumulateHeldDelta` of the delta of clip `c` held in `d` (a machine's blend of additive sequences): the clip's
/// additive type picks the space the delta is in (mesh space: `rig`'s hierarchy).
#[allow(clippy::too_many_arguments)]
pub fn accumulate_held_of(
    clips: &Clips,
    c: usize,
    rig: &Rig,
    d: &Pose,
    bones: &[usize],
    w: f64,
    scale: bool,
    out: &mut Pose,
) {
    let clip = &clips.clips[c];
    let mesh = (clip.anim.additive_type == ADDITIVE_MESH).then(|| &rig.skels[clip.skel].mesh);
    accumulate_held_delta(d, bones, w, scale, out, mesh);
}

/// `weightPose` (FAnimationRuntime::BlendPosesTogether's first kernel, FUN_1402970a0): out = a x w.
pub fn weight_pose(a: &Pose, w: f64, out: &mut Pose) {
    let m = |v: f32| (f64::from(v) * w) as f32;
    for i in 0..out.t.len() {
        out.t[i] = m(a.t[i]);
        out.s[i] = m(a.s[i]);
    }
    for i in 0..out.r.len() {
        out.r[i] = m(a.r[i]);
    }
}

/// `addWeightedPose` (FUN_140296ff0): out += a x w, a's rotation on the side of out's (dot >= 0 adds). No division by
/// the weights' sum.
pub fn add_weighted_pose(a: &Pose, w: f64, out: &mut Pose) {
    let add = |o: f32, v: f32, w: f64| (f64::from(o) + f64::from(v) * w) as f32;
    for i in 0..out.t.len() {
        out.t[i] = add(out.t[i], a.t[i], w);
        out.s[i] = add(out.s[i], a.s[i], w);
    }
    for i in (0..out.r.len()).step_by(4) {
        let f = |v: f32| f64::from(v);
        let dot = (0..4).map(|k| f(out.r[i + k]) * f(a.r[i + k])).fold(0.0, |s, v| s + v);
        let g = if dot < 0.0 { -w } else { w };
        for k in 0..4 {
            out.r[i + k] = add(out.r[i + k], a.r[i + k], g);
        }
    }
}

/// `normalizeRotations` (FUN_1404d6360).
pub fn normalize_rotations(p: &mut Pose) {
    normalize_all(&mut p.r);
}

/// `accumulateHeldDelta`: AccumulateAdditivePose (NQ7) with a held delta `d` on the bones listed; `mesh` the rig's
/// hierarchy for a mesh-space delta. Both types end with NormalizeRotations (1428a1cf0) over every bone.
fn accumulate_held_delta(d: &Pose, bones: &[usize], w: f64, scale: bool, out: &mut Pose, mesh: Option<&MeshRig>) {
    if !(w > ADD_MIN) {
        return;
    }
    if let Some(m) = mesh {
        to_mesh(out, m);
    }
    let part = w < ADD_FULL;
    let r32 = |v: f64| f64::from(v as f32);
    for &i in bones {
        let o = i * 4;
        let dr = d.quat(i);
        let mut q = [r32(dr[0] * w), r32(dr[1] * w), r32(dr[2] * w), 0.0];
        let mut turn = true;
        if part {
            // the identity on the delta's side (the build's test on its own w, turned round in this space)
            q[3] = r32(dr[3] * w + if dr[3] > 0.0 { 1.0 - w } else { w - 1.0 });
            let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
            if l > 1e-8 {
                q = [r32(q[0] / l), r32(q[1] / l), r32(q[2] / l), r32(q[3] / l)];
            } else {
                q = [0.0, 0.0, 0.0, 1.0];
            }
        } else {
            q[3] = r32(dr[3] * w);
            turn = ((q[3] * q[3]) as f32) < 1.0;
        }
        if turn {
            let q = quat_mul32(q, out.quat(i));
            let l2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
            let r = &mut out.r[o..o + 4];
            if l2 >= ADD_BASE_MIN {
                let l = l2.sqrt();
                for k in 0..4 {
                    r[k] = (q[k] / l) as f32;
                }
            } else {
                r.copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
            }
        }
        for k in i * 3..i * 3 + 3 {
            out.t[k] = (f64::from(out.t[k]) + f64::from(d.t[k]) * w) as f32;
            if scale {
                out.s[k] = (f64::from(out.s[k]) * (1.0 + f64::from(d.s[k]) * w)) as f32;
            }
        }
    }
    if let Some(m) = mesh {
        from_mesh(out, m);
    }
    normalize_all(&mut out.r);
}

/// ConvertPoseToMeshRotation: parents first, R_i = R_parent R_i.
fn to_mesh(p: &mut Pose, m: &MeshRig) {
    for &i in &m.order {
        let pa = m.parents[i];
        if pa < 0 {
            continue;
        }
        let q = quat_mul32(p.quat(pa as usize), p.quat(i));
        for (k, v) in q.iter().enumerate() {
            p.r[i * 4 + k] = *v as f32;
        }
    }
}

/// ConvertMeshRotationPoseToLocalSpace: children first, R_i = R_parent⁻¹ R_i (the parent still in mesh space).
fn from_mesh(p: &mut Pose, m: &MeshRig) {
    for &i in m.order.iter().rev() {
        let pa = m.parents[i];
        if pa < 0 {
            continue;
        }
        let a = p.quat(pa as usize);
        let q = quat_mul32([-a[0], -a[1], -a[2], a[3]], p.quat(i));
        for (k, v) in q.iter().enumerate() {
            p.r[i * 4 + k] = *v as f32;
        }
    }
}

/// NormalizeRotations: every rotation unit length, the identity under a squared length of 1e-8.
fn normalize_all(r: &mut [f32]) {
    for q in r.as_chunks_mut::<4>().0 {
        let f = |v: f32| f64::from(v);
        let l2 = f(q[0]) * f(q[0]) + f(q[1]) * f(q[1]) + f(q[2]) * f(q[2]) + f(q[3]) * f(q[3]);
        if l2 < 1e-8 {
            q.copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
            continue;
        }
        let l = l2.sqrt();
        for v in q.iter_mut() {
            *v = (f(*v) / l) as f32;
        }
    }
}

// ---- head scale -------------------------------------------------------------------------------------------------

/// One ModifyBone node fed from HeadScale.
#[derive(Clone, Copy, Debug)]
pub struct HeadScaleNode {
    pub node: &'static str,
    pub bone: &'static str,
    pub factor: f64,
    pub gated: bool,
    pub moves: [f64; 3],
    pub scale: bool,
}

#[derive(Clone, Debug)]
struct HeadResolved {
    i: usize,
    factor: f64,
    gated: bool,
    scale: bool,
    m: [f64; 3],
    moves: bool,
}

/// `headScaleStep`: the view's HeadScale nodes resolved on its bones; None when the mesh has none of them.
#[derive(Clone, Debug)]
pub struct HeadScaleStep(Vec<HeadResolved>);

impl HeadScaleStep {
    pub fn new(names: &[String], nodes: &[HeadScaleNode]) -> Option<HeadScaleStep> {
        let list: Vec<HeadResolved> = nodes
            .iter()
            .filter_map(|n| {
                let i = names.iter().position(|b| b == n.bone)?;
                let m = n.moves;
                Some(HeadResolved {
                    i,
                    factor: n.factor,
                    gated: n.gated,
                    scale: n.scale,
                    m: [m[0] / 100.0, m[2] / 100.0, m[1] / 100.0],
                    moves: m != [0.0; 3],
                })
            })
            .collect();
        (!list.is_empty()).then_some(HeadScaleStep(list))
    }

    /// The step at HeadScale.X `k`.
    pub fn apply(&self, p: &mut Pose, k: f64) {
        for n in &self.0 {
            if n.gated && !(k > 1.0) {
                continue; // alpha 0: the node is not evaluated (alpha <= 1e-5)
            }
            let i3 = n.i * 3;
            let (sx, sy, sz) = (f64::from(p.s[i3]), f64::from(p.s[i3 + 1]), f64::from(p.s[i3 + 2]));
            if n.moves {
                // t += q (s * m): the translation in the bone's frame, before its scale changes
                let (vx, vy, vz) = (sx * n.m[0], sy * n.m[1], sz * n.m[2]);
                let [qx, qy, qz, qw] = p.quat(n.i);
                let (cx, cy, cz) = (qy * vz - qz * vy, qz * vx - qx * vz, qx * vy - qy * vx);
                p.t[i3] = (f64::from(p.t[i3]) + vx + 2.0 * (qw * cx + qy * cz - qz * cy)) as f32;
                p.t[i3 + 1] = (f64::from(p.t[i3 + 1]) + vy + 2.0 * (qw * cy + qz * cx - qx * cz)) as f32;
                p.t[i3 + 2] = (f64::from(p.t[i3 + 2]) + vz + 2.0 * (qw * cz + qx * cy - qy * cx)) as f32;
            }
            if n.scale {
                let f = k * n.factor;
                p.s[i3] = (sx * f) as f32;
                p.s[i3 + 1] = (sy * f) as f32;
                p.s[i3 + 2] = (sz * f) as f32;
            }
        }
    }
}
