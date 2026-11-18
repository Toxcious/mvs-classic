//! The sim's own skeleton poses: the bones a character's collision shapes ride,
//! each clip's local transforms of those bones, and the engine's blend kernels the character graphs put together
//! (FAnimationRuntime::AccumulateAdditivePose, BlendPosesTogether, FAnimNode_LayeredBoneBlend, ModifyBone), then the
//! shapes placed on the pose. The space is the generator's (glTF axes, metres); the floats are stored as the sim
//! stores them (f32 poses and scratch, f64 world transforms).
//!
//! The scratch buffers are not on the Skel; they are a `SkelScratch` beside it, so a function can read the
//! skeleton's data while it writes the scratch.

use crate::clip::STEP_EPS;
use indexmap::IndexMap;
use mvs_data::boxes::BoxKind;
pub use mvs_data::boxes::{BoxIndex, SkelIndex};
use mvs_math::f32::estimate::reciprocal_sqrt_accurate;
use mvs_math::js;

const MIN_W: f32 = 1e-5; // ZERO_ANIMWEIGHT_THRESH (DAT_143b3a298)
const FULL_W: f32 = 0.99999; // DAT_143ce1700
const TINY_Q: f32 = 1e-8; // DAT_143b46c24, SMALL_NUMBER

#[derive(Debug, Clone, PartialEq)]
pub struct SkelPose {
    pub n: usize,
    pub t: Vec<f32>,
    pub r: Vec<f32>,
    pub s: Vec<f32>,
}

impl SkelPose {
    pub fn new(n: usize) -> SkelPose {
        SkelPose { n, t: vec![0.0; n * 3], r: vec![0.0; n * 4], s: vec![0.0; n * 3] }
    }
    pub fn copy(&mut self, o: &SkelPose) {
        self.t.copy_from_slice(&o.t);
        self.r.copy_from_slice(&o.r);
        self.s.copy_from_slice(&o.s);
    }
}

pub struct SkelClip {
    pub name: String,
    pub additive: bool,
    pub nf: f64,
    pub length: f64,
    pub step: bool,
    pub base: usize,
    pub frames: usize,
    pub width: usize,
    pub off: Vec<i32>,
    pub bones: Vec<i32>,
    pub still: bool,
}

pub struct SkelBlendSpace {
    pub min: Vec<f64>,
    pub max: Vec<f64>,
    pub grid: Vec<f64>,
    pub rows: Vec<f64>,
    /// By sample: its clip (an index into `Skel::clips`), None when the skel has no such clip.
    pub clips: Vec<Option<usize>>,
    pub bones: Vec<i32>,
    pub full: bool,
    pub ok: bool,
}

// The box index's parts the skeleton reads are the bound data's records (`mvs_data::boxes`).

pub struct Skel {
    pub n: usize,
    pub parents: Vec<i32>,
    pub rest: SkelPose,
    /// In the index's order (`clip_names` gives it: insertion order).
    pub clips: Vec<SkelClip>,
    pub clip_index: IndexMap<String, usize>,
    pub masks: IndexMap<String, Vec<f32>>,
    pub blend: Vec<SkelBlendSpace>,
    pub blend_index: IndexMap<String, usize>,
    pub all: Vec<i32>,
    pub bone: IndexMap<String, usize>,
    pub index: BoxIndex,
    pub sk: SkelIndex,
    pub data: Vec<f32>,
}

/// The Skel's scratch: the mesh space rotations of `layered`, the FK's world transforms, a blend space's rows.
#[derive(Debug, Clone)]
pub struct SkelScratch {
    pub m_s: Vec<f32>,
    pub m_t: Vec<f32>,
    pub m_b: Vec<f32>,
    pub wt: Vec<f64>,
    pub wr: Vec<f64>,
    pub ws: Vec<f64>,
    pub row_sample: [i32; 12],
    pub row_w: [f32; 12],
    pub row_count: usize,
}

impl SkelScratch {
    pub fn new(n: usize) -> SkelScratch {
        SkelScratch {
            m_s: vec![0.0; n * 4],
            m_t: vec![0.0; n * 4],
            m_b: vec![0.0; n * 4],
            wt: vec![0.0; n * 3],
            wr: vec![0.0; n * 4],
            ws: vec![0.0; n * 3],
            row_sample: [0; 12],
            row_w: [0.0; 12],
            row_count: 0,
        }
    }
}

impl std::fmt::Debug for Skel {
    /// The bone and clip counts (the data is the box index's whole section).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Skel")
            .field("n", &self.n)
            .field("clips", &self.clips.len())
            .field("blend", &self.blend.len())
            .finish()
    }
}

impl Skel {
    /// `new Skel(index, sk, data)`, the clips and blend spaces in the order given (the index's key order).
    pub fn new(
        index: BoxIndex,
        sk: SkelIndex,
        data: Vec<f32>,
        clip_names: &[String],
        blend_names: &[String],
    ) -> Result<Skel, String> {
        let n = sk.bones.len();
        let mut bone = IndexMap::new();
        for b in 0..n {
            if sk.parents[b] >= b as i32 {
                return Err(format!("skel: a bone before its parent: {}", sk.bones[b]));
            }
            bone.insert(sk.bones[b].clone(), b);
        }
        let mut rest = SkelPose::new(n);
        for b in 0..n {
            for k in 0..3 {
                rest.t[b * 3 + k] = sk.rest[b * 10 + k] as f32;
                rest.s[b * 3 + k] = sk.rest[b * 10 + 7 + k] as f32;
            }
            for k in 0..4 {
                rest.r[b * 4 + k] = sk.rest[b * 10 + 3 + k] as f32;
            }
        }
        let mut clips = Vec::new();
        let mut clip_index = IndexMap::new();
        for name in clip_names {
            let c = &sk.clips[name];
            let a = index.anims.get(name).ok_or_else(|| format!("skel: clip without an anim entry: {name}"))?;
            let mut off = vec![0i32; n * 3];
            let mut tracked = Vec::new();
            let mut w = 0i32;
            let additive = c.a.is_some_and(|a| a != 0);
            for b in 0..n {
                let fl = c.v[b];
                off[b * 3] = if fl & 1 != 0 { w } else { -1 };
                if fl & 1 != 0 {
                    w += 3;
                }
                off[b * 3 + 1] = if fl & 2 != 0 { w } else { -1 };
                if fl & 2 != 0 {
                    w += 4;
                }
                off[b * 3 + 2] = if fl & 4 != 0 { w } else { -1 };
                if fl & 4 != 0 {
                    w += 3;
                }
                if !additive || fl & 8 != 0 {
                    tracked.push(b as i32);
                }
            }
            let base = sk.offset as usize + c.o as usize;
            clip_index.insert(name.clone(), clips.len());
            clips.push(SkelClip {
                name: name.clone(),
                additive,
                nf: f64::from(a.num_frames),
                length: a.length,
                step: a.step.unwrap_or(false),
                base,
                frames: base + n * 10,
                width: w as usize,
                off,
                bones: tracked,
                still: w == 0,
            });
        }
        let mut masks = IndexMap::new();
        for (name, list) in &sk.masks {
            let mut m = vec![0.0f32; n];
            for &b in list {
                m[b as usize] = 1.0;
            }
            masks.insert(name.clone(), m);
        }
        let mut blend = Vec::new();
        let mut blend_index = IndexMap::new();
        for name in blend_names {
            let b = &sk.blend[name];
            let cl: Vec<Option<usize>> = b.samples.iter().map(|x| clip_index.get(&x.clip).copied()).collect();
            let mut used: Vec<i32> = Vec::new();
            for &c in cl.iter().flatten() {
                for &i in &clips[c].bones {
                    if !used.contains(&i) {
                        used.push(i);
                    }
                }
            }
            used.sort_unstable();
            let full = b.full.is_some_and(|f| f != 0);
            let ok = if full {
                cl.iter().all(|c| c.is_some_and(|c| !clips[c].additive))
            } else {
                cl.iter().all(|c| c.is_some_and(|c| clips[c].additive && clips[c].still))
            };
            blend_index.insert(name.clone(), blend.len());
            blend.push(SkelBlendSpace {
                min: b.axes.iter().map(|a| a.min).collect(),
                max: b.axes.iter().map(|a| a.max).collect(),
                grid: b.axes.iter().map(|a| a.grid).collect(),
                rows: b.grid.clone(),
                clips: cl,
                bones: used,
                full,
                ok,
            });
        }
        Ok(Skel {
            n,
            parents: sk.parents.clone(),
            rest,
            clips,
            clip_index,
            masks,
            blend,
            blend_index,
            all: (0..n as i32).collect(),
            bone,
            index,
            sk,
            data,
        })
    }
}

/// Math.min on float32 (NaN wins, -0 below +0): exact through the double.
#[inline]
fn min32(a: f32, b: f32) -> f32 {
    js::min(f64::from(a), f64::from(b)) as f32
}

/// Math.max on float32 (NaN wins, +0 above -0).
#[inline]
fn max32(a: f32, b: f32) -> f32 {
    js::max(f64::from(a), f64::from(b)) as f32
}

/// FQuat x FQuat: VectorQuaternionMultiply2 (the w lane's product first, then the x, y and z lanes' signed products
/// added in that order; the bone accumulate's at 142815bc9), one float32 rounding per operation.
#[inline]
fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// VectorDot4 of a quaternion with itself: (x x + z z) + (y y + w w), the shuffle-and-add order (141370e81).
#[inline]
fn square_sum(q: [f32; 4]) -> f32 {
    (q[0] * q[0] + q[2] * q[2]) + (q[1] * q[1] + q[3] * q[3])
}

/// FQuat::Normalize / VectorNormalizeSafe with the identity as default (141370e81, the accumulate's at 142815b6a):
/// a squared length at or above 1e-8 scales by VectorReciprocalSqrtAccurate, anything else (a NaN included) is the
/// identity.
#[inline]
fn quat_normalize(q: [f32; 4]) -> [f32; 4] {
    let sum = square_sum(q);
    if !(sum >= TINY_Q) {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let r = reciprocal_sqrt_accurate(sum);
    [r * q[0], r * q[1], r * q[2], r * q[3]]
}

#[inline]
fn quat_at(a: &[f32], o: usize) -> [f32; 4] {
    [a[o], a[o + 1], a[o + 2], a[o + 3]]
}

/// `sampleClip`: a clip at a time into out (every bone's local transform, or an additive clip's delta).
// Unverified: the build samples its compressed tracks (AnimEncoding's TimeToIndex and key lerps, unread in the exe);
// the port samples the importer's per-frame keys in float32 in a fixed order of operations: the frame position,
// FMath::Lerp of translation and scale, the rotation lerp on the shorter arc, then FQuat::Normalize.
pub fn sample_clip(sk: &Skel, c: &SkelClip, time: impl Into<f64>, out: &mut SkelPose) {
    // (the sim's times are float32: the narrowing is exact)
    let time = time.into() as f32;
    let d = &sk.data;
    let nf = c.nf as f32;
    let len = c.length as f32;
    let f = if nf > 1.0 && len > 0.0 { min32(max32(time / len, 0.0), 1.0) * (nf - 1.0) } else { 0.0 };
    let (i0, w) = if c.step {
        (min32((f + STEP_EPS as f32).floor(), nf - 1.0), 0.0)
    } else {
        let i0 = f.floor();
        (i0, f - i0)
    };
    // frame indices: whole numbers below 2^24, exact in float32
    let f0 = c.frames + i0 as usize * c.width;
    let f1 = c.frames + min32(i0 + 1.0, nf - 1.0) as usize * c.width;
    let off = &c.off;
    for b in 0..sk.n {
        let cb = c.base + b * 10;
        let (b3, b4) = (b * 3, b * 4);
        let o = off[b3];
        if o < 0 {
            out.t[b3..b3 + 3].copy_from_slice(&d[cb..cb + 3]);
        } else {
            let (p, q) = (f0 + o as usize, f1 + o as usize);
            for k in 0..3 {
                out.t[b3 + k] = d[p + k] + (d[q + k] - d[p + k]) * w;
            }
        }
        let o = off[b3 + 1];
        if o < 0 {
            out.r[b4..b4 + 4].copy_from_slice(&d[cb + 3..cb + 7]);
        } else if w == 0.0 {
            let p = f0 + o as usize;
            out.r[b4..b4 + 4].copy_from_slice(&d[p..p + 4]);
        } else {
            let (p, q) = (f0 + o as usize, f1 + o as usize);
            let dot = d[p] * d[q] + d[p + 1] * d[q + 1] + d[p + 2] * d[q + 2] + d[p + 3] * d[q + 3];
            let h = if dot < 0.0 { -w } else { w };
            let cw = 1.0 - w;
            let q = [
                d[p] * cw + d[q] * h,
                d[p + 1] * cw + d[q + 1] * h,
                d[p + 2] * cw + d[q + 2] * h,
                d[p + 3] * cw + d[q + 3] * h,
            ];
            out.r[b4..b4 + 4].copy_from_slice(&quat_normalize(q));
        }
        let o = off[b3 + 2];
        if o < 0 {
            out.s[b3..b3 + 3].copy_from_slice(&d[cb + 7..cb + 10]);
        } else {
            let (p, q) = (f0 + o as usize, f1 + o as usize);
            for k in 0..3 {
                out.s[b3 + k] = d[p + k] + (d[q + k] - d[p + k]) * w;
            }
        }
    }
}

/// `identityDelta`: the additive identity as a delta pose.
pub fn identity_delta(p: &mut SkelPose) {
    p.t.fill(0.0);
    p.s.fill(0.0);
    p.r.fill(0.0);
    for i in (3..p.r.len()).step_by(4) {
        p.r[i] = 1.0;
    }
}

/// `accumulate`: FAnimationRuntime::AccumulateAdditivePose of a local space additive at the weight w (exe
/// FUN_142815970), the delta d on the bones listed (the per-bone rule is in the body). `scale` false: out's scale
/// is not written.
///
/// Below full weight it is FTransform::BlendFromIdentityAndAccumulate as the exe inlines it (142815b00): the delta
/// rotation d x w with w (1 - w) on its own side of the identity, FQuat::Normalize, then delta x out, not normalised;
/// out's translation + d x w, its scale x (1 + d x w).
pub fn accumulate(d: &SkelPose, bones: &[i32], w: impl Into<f64>, scale: bool, out: &mut SkelPose) {
    let w = w.into() as f32;
    if !(w > MIN_W) {
        return;
    }
    let part = w < FULL_W;
    for &bi in bones {
        let i = bi as usize;
        let o = i * 4;
        let dr = quat_at(&d.r, o);
        let mut q = [dr[0] * w, dr[1] * w, dr[2] * w, 0.0f32];
        if part {
            // (cmpleps 0 <= d.w: a NaN takes the negative side)
            q[3] = dr[3] * w + if dr[3] >= 0.0 { 1.0 - w } else { -(1.0 - w) };
            q = quat_normalize(q);
            let r = quat_mul(q, quat_at(&out.r, o));
            out.r[o..o + 4].copy_from_slice(&r);
        } else {
            // Unverified: the full-weight path is the exe's ISPC kernel (140296c20, by CPU level), unread; the sim's
            // rule (the turn only when w^2 < 1, then normalised) kept in float32.
            q[3] = dr[3] * w;
            if q[3] * q[3] < 1.0 {
                let r = quat_mul(q, quat_at(&out.r, o));
                out.r[o..o + 4].copy_from_slice(&quat_normalize(r));
            }
        }
        for k in i * 3..i * 3 + 3 {
            out.t[k] += d.t[k] * w;
            if scale {
                out.s[k] *= 1.0 + d.s[k] * w;
            }
        }
    }
}

/// `weightPose`: out = a x w (BlendPosesTogether's first kernel).
pub fn weight_pose(a: &SkelPose, w: impl Into<f64>, out: &mut SkelPose) {
    let w = w.into() as f32;
    for i in 0..out.t.len() {
        out.t[i] = a.t[i] * w;
        out.s[i] = a.s[i] * w;
    }
    for i in 0..out.r.len() {
        out.r[i] = a.r[i] * w;
    }
}

/// `addWeightedPose`: out += a x w, a's rotation on the side of out's.
pub fn add_weighted_pose(a: &SkelPose, w: impl Into<f64>, out: &mut SkelPose) {
    let w = w.into() as f32;
    for i in 0..out.t.len() {
        out.t[i] += a.t[i] * w;
        out.s[i] += a.s[i] * w;
    }
    let or = &mut out.r;
    let ar = &a.r;
    for i in (0..or.len()).step_by(4) {
        let g = if or[i] * ar[i] + or[i + 1] * ar[i + 1] + or[i + 2] * ar[i + 2] + or[i + 3] * ar[i + 3] < 0.0 {
            -w
        } else {
            w
        };
        for k in 0..4 {
            or[i + k] += ar[i + k] * g;
        }
    }
}

/// `normalizeRotations`: every rotation through FQuat::Normalize (the identity under a squared length of 1e-8).
pub fn normalize_rotations(p: &mut SkelPose) {
    for q in p.r.as_chunks_mut::<4>().0 {
        *q = quat_normalize(*q);
    }
}

/// `replace`: a local space FAnimNode_LayeredBoneBlend at weight 1: the masked bones take the blend pose's local
/// transforms.
pub fn replace(p: &mut SkelPose, blend: &SkelPose, mask: &[f32]) {
    for (b, &m) in mask.iter().enumerate() {
        // (a mask past the pose: the stores past the array are dropped)
        if b >= p.n {
            break;
        }
        // (`!mask[b]`: 0 and NaN are unmasked)
        if m == 0.0 || m.is_nan() {
            continue;
        }
        let (b3, b4) = (b * 3, b * 4);
        p.t[b3..b3 + 3].copy_from_slice(&blend.t[b3..b3 + 3]);
        p.s[b3..b3 + 3].copy_from_slice(&blend.s[b3..b3 + 3]);
        p.r[b4..b4 + 4].copy_from_slice(&blend.r[b4..b4 + 4]);
    }
}

/// `layered`: FAnimNode_LayeredBoneBlend with one blend pose and bMeshSpaceRotationBlend (BlendPosesPerBoneFilter
/// FUN_142817220, flags = 1): out = base with blend laid over it at the per-bone weights mask x alpha.
pub fn layered(
    sk: &Skel,
    sc: &mut SkelScratch,
    base: &SkelPose,
    blend: &SkelPose,
    mask: &[f32],
    alpha: impl Into<f64>,
    out: &mut SkelPose,
) {
    layered_into(sk, sc, Some(base), blend, mask, alpha.into() as f32, out);
}

/// `layered` with out the base pose itself (`layered(sk, p, S, mask, alpha, p)`).
pub fn layered_in_place(
    sk: &Skel,
    sc: &mut SkelScratch,
    p: &mut SkelPose,
    blend: &SkelPose,
    mask: &[f32],
    alpha: impl Into<f64>,
) {
    layered_into(sk, sc, None, blend, mask, alpha.into() as f32, p);
}

// base None: out is the base. Each bone reads the base's values of its own index before it writes them, so the one
// pose serves both.
// Unverified: the float32 order of the per-bone blend is the sim's (BlendPosesPerBoneFilter's own code unread), with
// FQuat::Normalize for its two normalisations.
fn layered_into(
    sk: &Skel,
    sc: &mut SkelScratch,
    base: Option<&SkelPose>,
    blend: &SkelPose,
    mask: &[f32],
    alpha: f32,
    out: &mut SkelPose,
) {
    let SkelScratch { m_s, m_t, m_b, .. } = sc;
    let lr = &blend.r;
    for b in 0..sk.n {
        let pa = sk.parents[b];
        let (b3, b4) = (b * 3, b * 4);
        let base_r = quat_at(base.map_or(&out.r, |p| &p.r), b4);
        if pa < 0 {
            m_s[b4..b4 + 4].copy_from_slice(&base_r);
            m_t[b4..b4 + 4].copy_from_slice(&lr[b4..b4 + 4]);
        } else {
            let p4 = pa as usize * 4;
            let q = quat_mul(quat_at(m_s, p4), base_r);
            m_s[b4..b4 + 4].copy_from_slice(&q);
            let q = quat_mul(quat_at(m_t, p4), quat_at(lr, b4));
            m_t[b4..b4 + 4].copy_from_slice(&q);
        }
        // (a mask shorter than the skeleton: the read is undefined, a NaN weight, and takes the base path)
        let x = min32(mask.get(b).map_or(f32::NAN, |m| *m * alpha), 1.0);
        if !(x > MIN_W) {
            if let Some(bp) = base {
                out.t[b3..b3 + 3].copy_from_slice(&bp.t[b3..b3 + 3]);
                out.s[b3..b3 + 3].copy_from_slice(&bp.s[b3..b3 + 3]);
            }
            m_b[b4..b4 + 4].copy_from_slice(&m_s[b4..b4 + 4]);
        } else if x >= FULL_W {
            out.t[b3..b3 + 3].copy_from_slice(&blend.t[b3..b3 + 3]);
            out.s[b3..b3 + 3].copy_from_slice(&blend.s[b3..b3 + 3]);
            m_b[b4..b4 + 4].copy_from_slice(&m_t[b4..b4 + 4]);
        } else {
            for c in 0..3 {
                let t0 = base.map_or(out.t[b3 + c], |p| p.t[b3 + c]);
                out.t[b3 + c] = t0 + (blend.t[b3 + c] - t0) * x;
                let s0 = base.map_or(out.s[b3 + c], |p| p.s[b3 + c]);
                out.s[b3 + c] = s0 + (blend.s[b3 + c] - s0) * x;
            }
            let (s, t) = (quat_at(m_s, b4), quat_at(m_t, b4));
            let dot = s[0] * t[0] + s[1] * t[1] + s[2] * t[2] + s[3] * t[3];
            let g = if dot < 0.0 { x - 1.0 } else { 1.0 - x };
            let q = [s[0] * g + t[0] * x, s[1] * g + t[1] * x, s[2] * g + t[2] * x, s[3] * g + t[3] * x];
            m_b[b4..b4 + 4].copy_from_slice(&quat_normalize(q));
        }
        if pa < 0 {
            out.r[b4..b4 + 4].copy_from_slice(&m_b[b4..b4 + 4]);
            continue;
        }
        let p4 = pa as usize * 4;
        let [px, py, pz, pw] = quat_at(m_b, p4);
        let q = quat_mul([-px, -py, -pz, pw], quat_at(m_b, b4));
        out.r[b4..b4 + 4].copy_from_slice(&quat_normalize(q));
    }
}

/// `turnBone`: a bone's mesh space rotation becomes (x, y, z, w) x its own when `additive`, else (x, y, z, w)
/// itself (FAnimNode_ModifyBone on the rotation alone, BMM_Additive / BMM_Replace), the local result through
/// FQuat::Normalize.
#[allow(clippy::too_many_arguments)]
pub fn turn_bone(
    sk: &Skel,
    p: &mut SkelPose,
    bone: i32,
    x: impl Into<f64>,
    y: impl Into<f64>,
    z: impl Into<f64>,
    w: impl Into<f64>,
    additive: bool,
) {
    let rot = [x.into() as f32, y.into() as f32, z.into() as f32, w.into() as f32];
    // (a bone outside the skeleton: the stores past the array are dropped, so it writes nothing)
    if bone < 0 || bone as usize >= sk.n {
        return;
    }
    let bone = bone as usize;
    // the parent's mesh space rotation (a root: the identity)
    let mut pq = [0.0f32, 0.0, 0.0, 1.0];
    let pa = sk.parents[bone];
    if pa >= 0 {
        let pa = pa as usize;
        // walk down from the root: the chain's indices are found going up, so multiply from the left
        pq = quat_at(&p.r, pa * 4);
        let mut k = sk.parents[pa];
        while k >= 0 {
            pq = quat_mul(quat_at(&p.r, k as usize * 4), pq);
            k = sk.parents[k as usize];
        }
    }
    let o = bone * 4;
    let mut q = if additive {
        // local' = conj(P) x R x P x local
        let q = quat_mul(pq, quat_at(&p.r, o));
        quat_mul(rot, q)
    } else {
        rot
    };
    q = quat_mul([-pq[0], -pq[1], -pq[2], pq[3]], q);
    p.r[o..o + 4].copy_from_slice(&quat_normalize(q));
}

#[inline]
fn clamp01(v: f32) -> f32 {
    min32(max32(v, 0.0), 1.0)
}

/// `blendRows`: a blend space's rows for the inputs x, y into sc.row_sample / row_w / row_count (the clamp and
/// the grid position, the grid elements, the rows by sample in append order, the merge by animation, the sort by
/// weight, the 1e-5 cut and the normalise). False: no row.
// Unverified: float32 in a fixed order of operations (UBlendSpaceBase::GetSamplesFromBlendInput's own code unread).
pub fn blend_rows(sc: &mut SkelScratch, bs: &SkelBlendSpace, x: impl Into<f64>, y: impl Into<f64>) -> bool {
    let x = x.into() as f32;
    let y = y.into() as f32;
    let SkelScratch { row_sample: rs, row_w: rw, row_count, .. } = sc;
    let mut count = 0usize;
    let (min0, max0, grid0) = (bs.min[0] as f32, bs.max[0] as f32, bs.grid[0] as f32);
    let px = ((min32(max32(x, min0), max0) - min0) * grid0) / (max0 - min0);
    let ix = px.trunc();
    let fx = px - ix;
    // a BlendSpace1D: the two grid elements ix (1 - fx) and ix + 1 (fx); a BlendSpace: four
    let two = bs.min.len() == 1;
    let py = if two {
        0.0
    } else {
        let (min1, max1, grid1) = (bs.min[1] as f32, bs.max[1] as f32, bs.grid[1] as f32);
        ((min32(max32(y, min1), max1) - min1) * grid1) / (max1 - min1)
    };
    let iy = py.trunc();
    let fy = py - iy;
    let ny = if two { 1.0 } else { bs.grid[1] as f32 + 1.0 };
    for e in 0..(if two { 2 } else { 4 }) {
        let row = (ix + (e & 1) as f32) * ny + iy + (e >> 1) as f32;
        let w = (if e & 1 != 0 { fx } else { 1.0 - fx }) * (if e >> 1 != 0 { fy } else { 1.0 - fy });
        if row < 0.0 || f64::from(row) * 6.0 >= bs.rows.len() as f64 {
            continue;
        }
        if row.is_nan() {
            // (a NaN row: the read is undefined, so each of the three appends sample 0 at a NaN weight)
            for _ in 0..3 {
                rs[count] = 0;
                rw[count] = f32::NAN;
                count += 1;
            }
            continue;
        }
        let row = row as usize;
        for k in 0..3 {
            let si = bs.rows[row * 6 + k];
            if si < 0.0 {
                continue;
            }
            let mut r = 0;
            while r < count && rs[r] as f64 != si {
                r += 1;
            }
            if r == count {
                rs[r] = js::to_int32(si);
                rw[r] = 0.0;
                count += 1;
            }
            rw[r] += w * bs.rows[row * 6 + 3 + k] as f32;
        }
    }
    let clip_of = |s: i32| bs.clips.get(s as usize).copied().flatten();
    let mut i = 0;
    while i < count {
        let mut j = i + 1;
        while j < count {
            if clip_of(rs[i]) != clip_of(rs[j]) {
                j += 1;
                continue;
            }
            let w1 = clamp01(rw[i]);
            let w2 = clamp01(rw[j]);
            if (w1 + w2).abs() > TINY_Q {
                rw[i] += w2;
            }
            count -= 1;
            rs[j] = rs[count];
            rw[j] = rw[count];
        }
        i += 1;
    }
    let mut end = count as isize - 1;
    while end > 0 {
        let e = end as usize;
        let mut m = 0;
        for k in 1..=e {
            if rw[m] > rw[k] {
                m = k;
            }
        }
        if m != e {
            rs.swap(m, e);
            rw.swap(m, e);
        }
        end -= 1;
    }
    let mut sum = 0.0f32;
    let mut keep = 0;
    while keep < count && !(rw[keep] < MIN_W) {
        sum += rw[keep];
        keep += 1;
    }
    *row_count = keep;
    let inv = 1.0 / sum;
    for v in rw.iter_mut().take(keep) {
        *v *= inv;
    }
    keep > 0
}

/// `blendDelta`: the blend of the rows' additive samples as a delta pose into out (no row: the additive identity).
/// s: a scratch pose.
pub fn blend_delta(sk: &Skel, sc: &SkelScratch, bs: &SkelBlendSpace, out: &mut SkelPose, s: &mut SkelPose) {
    if sc.row_count == 0 {
        identity_delta(out);
        return;
    }
    for k in 0..sc.row_count {
        let c = bs.clips[sc.row_sample[k] as usize].expect("blendDelta: a row of no clip");
        sample_clip(sk, &sk.clips[c], 0.0, s);
        let w = clamp01(sc.row_w[k]);
        if k == 0 {
            weight_pose(s, w, out);
        } else {
            add_weighted_pose(s, w, out);
        }
    }
    normalize_rotations(out);
}

/// The quaternion product in double, for the generator's FK.
#[inline]
fn quat_mul64(ax: f64, ay: f64, az: f64, aw: f64, bx: f64, by: f64, bz: f64, bw: f64) -> [f64; 4] {
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// `forward`: the pose's world transforms (sc.wt / wr / ws), the generator's FK.
// Deviation: the shapes are placed in the generator's space (glTF axes, metres) in double, as the sim does: the build's
// component space transforms are float32 FTransforms in its own space and units, which this placement does not
// reproduce operation by operation (the port's collision stand-in).
pub fn forward(sk: &Skel, sc: &mut SkelScratch, p: &SkelPose) {
    let SkelScratch { wt, wr, ws, .. } = sc;
    let f = |a: &[f32], i: usize| a[i] as f64;
    for b in 0..sk.n {
        let pa = sk.parents[b];
        let (b3, b4) = (b * 3, b * 4);
        if pa < 0 {
            for k in 0..3 {
                wt[b3 + k] = f(&p.t, b3 + k);
                ws[b3 + k] = f(&p.s, b3 + k);
            }
            for k in 0..4 {
                wr[b4 + k] = f(&p.r, b4 + k);
            }
            continue;
        }
        let (p3, p4) = (pa as usize * 3, pa as usize * 4);
        let (qx, qy, qz, qw) = (wr[p4], wr[p4 + 1], wr[p4 + 2], wr[p4 + 3]);
        let vx = f(&p.t, b3) * ws[p3];
        let vy = f(&p.t, b3 + 1) * ws[p3 + 1];
        let vz = f(&p.t, b3 + 2) * ws[p3 + 2];
        let tx = 2.0 * (qy * vz - qz * vy);
        let ty = 2.0 * (qz * vx - qx * vz);
        let tz = 2.0 * (qx * vy - qy * vx);
        wt[b3] = wt[p3] + vx + qw * tx + qy * tz - qz * ty;
        wt[b3 + 1] = wt[p3 + 1] + vy + qw * ty + qz * tx - qx * tz;
        wt[b3 + 2] = wt[p3 + 2] + vz + qw * tz + qx * ty - qy * tx;
        let q = quat_mul64(qx, qy, qz, qw, f(&p.r, b4), f(&p.r, b4 + 1), f(&p.r, b4 + 2), f(&p.r, b4 + 3));
        wr[b4..b4 + 4].copy_from_slice(&q);
        for k in 0..3 {
            ws[b3 + k] = ws[p3 + k] * f(&p.s, b3 + k);
        }
    }
}

#[inline]
fn rotate(q: &[f64], o: usize, vx: f64, vy: f64, vz: f64) -> [f64; 3] {
    let (x, y, z, w) = (q[o], q[o + 1], q[o + 2], q[o + 3]);
    let tx = 2.0 * (y * vz - z * vy);
    let ty = 2.0 * (z * vx - x * vz);
    let tz = 2.0 * (x * vy - y * vx);
    [vx + w * tx + y * tz - z * ty, vy + w * ty + z * tx - x * tz, vz + w * tz + x * ty - y * tx]
}
const AXES: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0]; // a box's local X, Y, Z in the generator's space
const WORLD: [f64; 9] = [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]; // bAbsoluteRotation: the world's axes
const SCALE_OF: [usize; 3] = [0, 2, 1]; // the bone scale's component on each of those axes

/// `writeShapes`: the shapes of the pose p in the box tracks' layout into s: every shape on a bone and the hit
/// rotation's bone point.
pub fn write_shapes(sk: &Skel, sc: &mut SkelScratch, p: &SkelPose, s: &mut [f32]) {
    forward(sk, sc, p);
    let (ix, d) = (&sk.index, &sk.sk);
    let (wt, wr, ws) = (&sc.wt, &sc.wr, &sc.ws);
    let ms = ix.mesh_scale * 100.0;
    let mz = ix.mesh_z;
    let mut o = 2usize;
    for (i, sh) in ix.shapes.iter().enumerate() {
        let size = if sh.kind == BoxKind::Socket {
            2
        } else if sh.is_box == Some(true) {
            12
        } else if sh.capsule {
            7
        } else {
            4
        };
        let b = d.shape_bone[i];
        if b < 0 {
            o += size;
            continue;
        }
        let (b3, b4) = (b as usize * 3, b as usize * 4);
        let (mut sx, mut sy, mut sz) = (ws[b3], ws[b3 + 1], ws[b3 + 2]);
        let v = rotate(wr, b4, d.shape_loc[i * 3] * sx, d.shape_loc[i * 3 + 1] * sy, d.shape_loc[i * 3 + 2] * sz);
        let cf = (wt[b3] + v[0]) * ms;
        let cu = (wt[b3 + 1] + v[1]) * ms + mz;
        let cd = (wt[b3 + 2] + v[2]) * ms;
        if sh.abs_scale == Some(true) {
            (sx, sy, sz) = (1.0, 1.0, 1.0);
        }
        let rot = |i: usize| -> [f64; 4] {
            let r = &d.shape_rot[i * 4..i * 4 + 4];
            quat_mul64(wr[b4], wr[b4 + 1], wr[b4 + 2], wr[b4 + 3], r[0], r[1], r[2], r[3])
        };
        if sh.kind == BoxKind::Socket {
            s[o] = cf as f32;
            s[o + 1] = cu as f32;
        } else if sh.is_box == Some(true) {
            s[o] = cf as f32;
            s[o + 1] = cu as f32;
            s[o + 2] = cd as f32;
            let qs = rot(i);
            let es = if sh.abs_scale == Some(true) { 1.0 } else { ix.mesh_scale };
            let extent = sh.extent.as_ref().expect("a box shape without an extent");
            for k in 0..3 {
                let v = if sh.abs_rot == Some(true) {
                    [WORLD[k * 3], WORLD[k * 3 + 1], WORLD[k * 3 + 2]]
                } else {
                    rotate(&qs, 0, AXES[k * 3], AXES[k * 3 + 1], AXES[k * 3 + 2])
                };
                let bs = match SCALE_OF[k] {
                    0 => sx,
                    1 => sy,
                    _ => sz,
                };
                let ln = extent[k] * es * bs.abs();
                s[o + 3 + k * 3] = (v[0] * ln) as f32;
                s[o + 4 + k * 3] = (v[1] * ln) as f32;
                s[o + 5 + k * 3] = (v[2] * ln) as f32;
            }
        } else if sh.capsule {
            let qs = rot(i);
            let v = rotate(&qs, 0, 0.0, 1.0, 0.0);
            let rs = js::min(sx.abs(), sz.abs());
            let h = js::max(sh.half_height * sy.abs() - sh.radius * rs, 0.0);
            s[o] = (cf - v[0] * h) as f32;
            s[o + 1] = (cu - v[1] * h) as f32;
            s[o + 2] = (cd - v[2] * h) as f32;
            s[o + 3] = (cf + v[0] * h) as f32;
            s[o + 4] = (cu + v[1] * h) as f32;
            s[o + 5] = (cd + v[2] * h) as f32;
            s[o + 6] = rs as f32;
        } else {
            s[o] = cf as f32;
            s[o + 1] = cu as f32;
            s[o + 2] = cd as f32;
            s[o + 3] = js::min(js::min(sx.abs(), sy.abs()), sz.abs()) as f32;
        }
        o += size;
    }
    if let Some(Some(com)) = &ix.com
        && d.com >= 0
    {
        let c = d.com as usize * 3;
        let co = com.offset as usize;
        s[co] = (wt[c] * ms) as f32;
        s[co + 1] = (wt[c + 1] * ms + mz) as f32;
    }
}
