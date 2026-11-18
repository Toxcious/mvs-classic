//! FAnimNode_LayeredBoneBlend in a fighter AnimBP (Update 142493bf0, Evaluate
//! 14246fe80, CreateMaskWeights 14281de60; the per-bone blend FAnimationRuntime::BlendPosesPerBoneFilter 142817220), on
//! a view's local pose.
//!
//! The node as the data states it (the slot graph's `Layered`): base, blends, BlendWeights, LayerSetup's branch
//! filters, bMeshSpaceRotationBlend.
//!   - The mask (CreateMaskWeights): per layer i and per branch filter of it, inc = BlendDepth != 0 ? 1 / BlendDepth :
//!     1; the branch bone and each bone under it get weight = clamp(weight + (depth + 1) x inc, 0, 1) and SourceIndex
//!     i (depth 0 at the branch bone); filters add up in their order, a negative BlendDepth takes weight away.
//!   - The blend, per bone, parents first: x = the bone's mask weight x BlendWeights[SourceIndex] (0 when that blend
//!     pose is not relevant: its weight at or under 1e-5); x <= 1e-5 the base's transform, x >= 0.99999 the blend
//!     pose's, between a linear blend of translation and scale and of the rotation: local rotations on the shorter
//!     arc then normalised, or with bMeshSpaceRotationBlend the mesh-space rotations of both chains (not normalised)
//!     blended so and the local one conjugate(B[parent]) x B, normalised (the identity under a squared length of
//!     1e-8).
//!
//! Refused where it is made: bMeshSpaceScaleBlend (no fighter node of the data has it). The mask is made from the
//! LayerSetup filters (the rule above, bone names without case) where the build reads the node's cooked
//! PerBoneBlendWeights, which CreateMaskWeights rebuilds from them only when the skeleton differs: checked equal once
//! for C023A_AnimBP's three nodes on C023A_SkelMesh_Skeleton (every one of its 344 bones, weight and SourceIndex; the
//! cooked arrays have 348 entries, the 4 more its virtual bones). The update side (which pose is updated) is the sim
//! walk's (mvs-sim fighter/anim/slot_blend.rs); a view decides which poses it samples by the same weights
//! ([`LayeredBlend::relevant`]).

use mvs_data::fighter::SlotGraphNode;

use crate::anim::Pose;

/// ZERO_ANIMWEIGHT_THRESH.
const MIN_W: f64 = 0.00001;
const FULL_W: f64 = 0.99999;
const TINY_Q: f64 = 1e-8;

#[derive(Clone, Debug)]
pub struct LayeredBlend {
    /// Per bone: the layer it takes (None: none) and its mask weight.
    pub src: Vec<Option<usize>>,
    pub mask: Vec<f32>,
    parents: Vec<i32>,
    /// The bones, parents first.
    order: Vec<usize>,
    layers: usize,
    mesh_rot: bool,
    /// The mesh-space rotations: the base's, each blend pose's (n x 4 each), the blended.
    m_s: Vec<f64>,
    m_t: Vec<f64>,
    m_b: Vec<f64>,
}

impl LayeredBlend {
    /// The node of the slot graph (a `Layered` node; panics on another kind) on a view's skeleton.
    pub fn new(def: &SlotGraphNode, names: &[String], parents: &[i32]) -> LayeredBlend {
        let SlotGraphNode::Layered { node, filters, mesh_rot, mesh_scale, .. } = def else {
            panic!("LayeredBlend: not a layered node: {def:?}")
        };
        assert!(!mesh_scale, "{node}: bMeshSpaceScaleBlend is not ported");
        let n = names.len();
        let mut src = vec![None; n];
        let mut mask = vec![0.0f32; n];
        // depth of bone b below bone a (0: b is a), None when b is not under a
        let depth_under = |b: usize, a: usize| -> Option<u32> {
            let mut d = 0;
            let mut k = b as i32;
            while k >= 0 {
                if k as usize == a {
                    return Some(d);
                }
                k = parents[k as usize];
                d += 1;
            }
            None
        };
        for (layer, fl) in filters.iter().enumerate() {
            for f in fl {
                // an FName: matched without case (C023A_AnimBP LayeredBoneBlend_2's filter `root` is the skeleton's
                // `Root`; its cooked PerBoneBlendWeights agree with this mask only so)
                let Some(at) = names.iter().position(|x| x.eq_ignore_ascii_case(&f.bone)) else {
                    // a filter of no bone (BoneName None: C023A_AnimBP's AnimGraphNode_LayeredBoneBlend) adds nothing,
                    // as a bone index of INDEX_NONE
                    if f.bone != "None" {
                        eprintln!("warning: {node}: branch bone {} is not in the skeleton", f.bone);
                    }
                    continue;
                };
                let inc = if f.depth != 0 { 1.0 / f64::from(f.depth) } else { 1.0 };
                for b in 0..n {
                    let Some(d) = depth_under(b, at) else { continue };
                    src[b] = Some(layer);
                    // (the mask is a Float32Array)
                    mask[b] = (f64::from(mask[b]) + f64::from(d + 1) * inc).clamp(0.0, 1.0) as f32;
                }
            }
        }
        let mut order = Vec::with_capacity(n);
        let mut done = vec![false; n];
        fn visit(b: usize, parents: &[i32], done: &mut [bool], order: &mut Vec<usize>) {
            if done[b] {
                return;
            }
            if parents[b] >= 0 {
                visit(parents[b] as usize, parents, done, order);
            }
            done[b] = true;
            order.push(b);
        }
        for b in 0..n {
            visit(b, parents, &mut done, &mut order);
        }
        let layers = filters.len();
        LayeredBlend {
            src,
            mask,
            parents: parents.to_vec(),
            order,
            layers,
            mesh_rot: *mesh_rot,
            m_s: vec![0.0; n * 4],
            m_t: vec![0.0; layers.max(1) * n * 4],
            m_b: vec![0.0; n * 4],
        }
    }

    /// A blend pose is updated and evaluated only at a weight over 1e-5.
    pub fn relevant(w: f64) -> bool {
        w > MIN_W
    }

    /// `p` = itself with the blend poses laid over it (blends[i] at weights[i]; a None or irrelevant one counts as
    /// weight 0): `out` is the base.
    pub fn evaluate(&mut self, p: &mut Pose, blends: &[Option<&Pose>], weights: &[f64]) {
        let nb4 = self.parents.len() * 4;
        for k in 0..self.order.len() {
            let b = self.order[k];
            let pa = self.parents[b];
            let (b3, b4) = (b * 3, b * 4);
            let si = self.src[b];
            let bl = si.and_then(|i| blends.get(i).copied().flatten());
            let wi = si.map_or(0.0, |i| weights.get(i).copied().unwrap_or(0.0));
            let x = match bl {
                Some(_) if wi > MIN_W => (f64::from(self.mask[b]) * wi).min(1.0),
                _ => 0.0,
            };
            if !self.mesh_rot {
                if let Some(bl) = bl {
                    local(p, bl, b, x);
                }
                continue;
            }
            // the mesh-space rotations of the base and of each relevant blend pose, every bone of each (the bone's
            // target is its own layer's pose's, through that pose's own chain)
            let br = p.quat(b);
            if pa < 0 {
                self.m_s[b4..b4 + 4].copy_from_slice(&br);
            } else {
                let q = hamilton(q4(&self.m_s, pa as usize * 4), br);
                self.m_s[b4..b4 + 4].copy_from_slice(&q);
            }
            for l in 0..self.layers {
                let Some(lp) = blends.get(l).copied().flatten() else { continue };
                if !(weights.get(l).copied().unwrap_or(0.0) > MIN_W) {
                    continue;
                }
                let o = l * nb4;
                let q = if pa < 0 { lp.quat(b) } else { hamilton(q4(&self.m_t, o + pa as usize * 4), lp.quat(b)) };
                self.m_t[o + b4..o + b4 + 4].copy_from_slice(&q);
            }
            let t4 = si.map_or(0, |i| i * nb4) + b4;
            if !(x > MIN_W) {
                let q = q4(&self.m_s, b4);
                self.m_b[b4..b4 + 4].copy_from_slice(&q);
            } else if x >= FULL_W {
                let bl = bl.expect("x > 0 has a blend pose");
                p.t[b3..b3 + 3].copy_from_slice(&bl.t[b3..b3 + 3]);
                p.s[b3..b3 + 3].copy_from_slice(&bl.s[b3..b3 + 3]);
                let q = q4(&self.m_t, t4);
                self.m_b[b4..b4 + 4].copy_from_slice(&q);
            } else {
                let bl = bl.expect("x > 0 has a blend pose");
                for j in 0..3 {
                    let (bt, bs) = (f64::from(p.t[b3 + j]), f64::from(p.s[b3 + j]));
                    p.t[b3 + j] = (bt + (f64::from(bl.t[b3 + j]) - bt) * x) as f32;
                    p.s[b3 + j] = (bs + (f64::from(bl.s[b3 + j]) - bs) * x) as f32;
                }
                let q = nlerp(q4(&self.m_s, b4), q4(&self.m_t, t4), x);
                self.m_b[b4..b4 + 4].copy_from_slice(&q);
            }
            if pa < 0 {
                let q = q4(&self.m_b, b4);
                set_q(p, b4, q);
                continue;
            }
            // conjugate(B[parent]) x B[b]
            let a = q4(&self.m_b, pa as usize * 4);
            let r = hamilton([-a[0], -a[1], -a[2], a[3]], q4(&self.m_b, b4));
            let l2 = r[0] * r[0] + r[1] * r[1] + r[2] * r[2] + r[3] * r[3];
            if l2 < TINY_Q {
                set_q(p, b4, [0.0, 0.0, 0.0, 1.0]);
                continue;
            }
            let l = l2.sqrt();
            set_q(p, b4, [r[0] / l, r[1] / l, r[2] / l, r[3] / l]);
        }
    }
}

/// The local-space blend of one bone into p (the base).
fn local(p: &mut Pose, blend: &Pose, b: usize, x: f64) {
    let (b3, b4) = (b * 3, b * 4);
    if !(x > MIN_W) {
        return;
    }
    if x >= FULL_W {
        p.copy_bone(blend, b);
        return;
    }
    for j in 0..3 {
        let (bt, bs) = (f64::from(p.t[b3 + j]), f64::from(p.s[b3 + j]));
        p.t[b3 + j] = (bt + (f64::from(blend.t[b3 + j]) - bt) * x) as f32;
        p.s[b3 + j] = (bs + (f64::from(blend.s[b3 + j]) - bs) * x) as f32;
    }
    let q = nlerp(p.quat(b), blend.quat(b), x);
    set_q(p, b4, q);
}

fn q4(a: &[f64], i: usize) -> [f64; 4] {
    [a[i], a[i + 1], a[i + 2], a[i + 3]]
}

fn set_q(p: &mut Pose, b4: usize, q: [f64; 4]) {
    for (d, v) in p.r[b4..b4 + 4].iter_mut().zip(q) {
        *d = v as f32;
    }
}

/// a x b (the parent first).
fn hamilton(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// a x (1 - x) + b x x on b's shorter arc from a, normalised (the identity under 1e-8).
fn nlerp(a: [f64; 4], b: [f64; 4], x: f64) -> [f64; 4] {
    let g = if a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3] < 0.0 { x - 1.0 } else { 1.0 - x };
    let q = [0, 1, 2, 3].map(|k| a[k] * g + b[k] * x);
    let l2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if l2 < TINY_Q {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let l = l2.sqrt();
    q.map(|v| v / l)
}
