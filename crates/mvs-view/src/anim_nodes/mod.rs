//! Native skeletal-control nodes a character's AnimBP graph places, ported from the exe (UE 4.26):
//! FAnimNode_SpringBone,
//! FAnimNode_LegIK, FAnimNode_Constraint, FAnimNode_CopyBone and FAnimNode_BoneDrivenController, with the
//! component-space pose they work on (FCSPose, its SafeSetCSBoneTransforms and LocalBlendCSBoneTransforms); and
//! FAnimNode_AnimDynamics (with its solver, FAnimPhys) and FAnimNode_Trail.
//!
//! A node is looked up by its struct name ([`anim_node_of`]) and made from the node's properties as the AnimBP's data
//! has them (the CDO's AnimGraphNode_* value: SpringBone.BoneName, SpringStiffness, ...) on a view's skeleton, once a
//! view. A type no maker is entered for panics where it is looked up, and a property the port does not act on panics
//! where the node is made (no default reading of a feature the port lacks).
//!
//! Units and axes. The view's pose ([`Pose`]) is in the three.js axes, (x, y, z) = the build's (X, Z, Y) / 100 and a
//! quaternion (x, y, z, w) = the build's (X, Z, Y, -W). The nodes work in the build's units and axes (cm, UE X / Y / Z,
//! UE quaternions): their thresholds are in cm, their axis flags are the build's axes, and the spring's sums are kept
//! in float32 as the exe's SSE code keeps them.
//!
//! What the caller does (a character's pose rule; nothing here runs unless one makes a node):
//!   - per anim update, in the graph's order: pre_update (the world's time dilation), then update(dt, alpha) with the
//!     mesh tick's DeltaTime, which carries the actor's CustomTimeDilation (in hitpause the spring gets 0.005 of a
//!     frame), and the node's Alpha input;
//!   - per evaluation: [`CsPose::begin`] on the local pose, each node's evaluate in the graph's order with the
//!     component's world transform ([`UeXf`]: the mesh component, location in cm, the actor's yaw -90 facing right,
//!     +90 facing left by SetRotationLogic's K2_SetActorRotation; [`mesh_xf`]), then the [`CsEval`] is dropped;
//!   - a node made on a view starts as the build's does after Initialize_AnyThread; the build's
//!     USkeletalMeshComponent InitAnim ticks the instance once with DeltaTime 0 before the first frame (exe 142994590;
//!     the SpringBone ctor zeroes its state), so the first evaluation takes the bone where the pose has it: call
//!     update(0, alpha) once after making the nodes, before the first update with a frame's time.
//!
//! [`SkinNodes`] does that for a stretch of an AnimBP's nodes made from the fighter's data (the skins' Trail /
//! AnimDynamics / SpringBone nodes), leaving out a node whose bones the view's mesh lacks; [`RespawnFlag`] is the Is
//! Respawning those AnimBPs gate them by.
//!
//! Deviation, in each of these:
//!   - InvSqrt is rsqrtss plus two Newton steps in the exe; here the float32 of 1 / sqrt: up to 1 ulp.
//!   - The component-space pose is composed in doubles from the float32 local pose; the exe caches float32 component
//!     transforms. Sub-ulp, not seen in a picture.
//!   - LegIK's two-bone solve uses f64 acos / sin where the exe calls the CRT's acosf / sinf: up to 1 ulp on the knee.
//!   - FABRIK (a leg of more than three bones, FAnimNode_LegIK 14248d140) is not read: such a leg is refused where
//!     the node is made.
//!   - A component transform with a negative scale takes the exe's matrix path; here the quaternion path alone (a
//!     fighter's mesh scale is positive; a negative one panics in evaluate).

use mvs_data::value::{Props, Value};

use crate::anim::Pose;

mod anim_dynamics;
pub mod anim_phys;
mod bone_driven;
mod constraint;
mod copy_bone;
mod leg_ik;
mod spring_bone;
mod trail;

pub use anim_dynamics::{AnimDynamicsNode, SimSpace};
pub use bone_driven::BoneDrivenControllerNode;
pub use constraint::ConstraintNode;
pub use copy_bone::CopyBoneNode;
pub use leg_ik::LegIkNode;
pub use spring_bone::SpringBoneNode;
pub use trail::TrailNode;

pub type V3 = [f64; 3];
pub type Q4 = [f64; 4];

// ---- small float32 helpers (the exe's SSE math) ----

/// `Math.fround`.
pub(crate) fn f32r(x: f64) -> f64 {
    f64::from(x as f32)
}

/// FMath::InvSqrt (rsqrtss + two Newton steps in the exe; Deviation: 1 ulp).
pub(crate) fn inv_sqrt(x: f64) -> f64 {
    f32r(1.0 / x.sqrt())
}

/// a * b (FQuat operator*: b first, then a), in a fixed order of terms.
pub fn qmul(a: Q4, b: Q4) -> Q4 {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// v rotated by q (FQuat::RotateVector); `conj`: by the inverse.
pub fn qrot(q: Q4, v: V3, conj: bool) -> V3 {
    let s = if conj { -1.0 } else { 1.0 };
    let (qx, qy, qz, qw) = (s * q[0], s * q[1], s * q[2], q[3]);
    let [x, y, z] = v;
    let (tx, ty, tz) = (2.0 * (qy * z - qz * y), 2.0 * (qz * x - qx * z), 2.0 * (qx * y - qy * x));
    [x + qw * tx + qy * tz - qz * ty, y + qw * ty + qz * tx - qx * tz, z + qw * tz + qx * ty - qy * tx]
}

/// FVector::GetSafeNormal(tol): |v|^2 == 1 unchanged, < tol zero, else v * InvSqrt.
pub(crate) fn safe_normal(v: V3, tol: f64) -> V3 {
    let [x, y, z] = v;
    let s = x * x + y * y + z * z;
    if s == 1.0 {
        v
    } else if s < tol {
        [0.0; 3]
    } else {
        let k = inv_sqrt(s);
        [x * k, y * k, z * k]
    }
}

/// FQuat::FindBetweenNormals 14135d040 of unit a, b.
pub(crate) fn find_between_normals(a: V3, b: V3) -> Q4 {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    let mut w = ax * bx + ay * by + az * bz + 1.0;
    let mut q = if w >= 1e-6 {
        [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx, 0.0]
    } else {
        w = 0.0;
        if ax.abs() > ay.abs() { [-az, 0.0, ax, 0.0] } else { [0.0, -az, ay, 0.0] }
    };
    q[3] = w;
    let s = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + w * w;
    if s <= 1e-8 {
        [0.0, 0.0, 0.0, 1.0]
    } else {
        let k = inv_sqrt(s);
        q.map(|c| c * k)
    }
}

/// FQuat::Equals(other, tol): q or -q within tol on every component.
pub(crate) fn quat_equals(a: Q4, b: Q4, tol: f64) -> bool {
    let same = (0..4).all(|k| (a[k] - b[k]).abs() <= tol);
    let neg = (0..4).all(|k| (a[k] + b[k]).abs() <= tol);
    same || neg
}

pub(crate) fn vec_equals(a: V3, b: V3, tol: f64) -> bool {
    (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol && (a[2] - b[2]).abs() <= tol
}

/// `Math.hypot` as V8 computes it (src/builtins/math.tq: scaled by the largest, Kahan-summed).
pub fn hypot(v: &[f64]) -> f64 {
    let max = v.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    if max == 0.0 || max.is_infinite() {
        return max;
    }
    let (mut sum, mut comp) = (0.0f64, 0.0f64);
    for x in v {
        let n = x.abs() / max;
        let r = n * n - comp;
        let p = sum + r;
        comp = (p - sum) - r;
        sum = p;
    }
    sum.sqrt() * max
}

pub(crate) fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

// ---- the component's world transform ----

/// An FTransform in the build's units: translation (cm), rotation (UE quaternion), scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UeXf {
    pub t: V3,
    pub q: Q4,
    pub s: V3,
}

impl Default for UeXf {
    fn default() -> UeXf {
        UeXf { t: [0.0; 3], q: [0.0, 0.0, 0.0, 1.0], s: [1.0; 3] }
    }
}

impl UeXf {
    /// FTransform::TransformPosition: R(S v) + T.
    pub fn position(&self, v: V3) -> V3 {
        let r = qrot(self.q, [v[0] * self.s[0], v[1] * self.s[1], v[2] * self.s[2]], false);
        [r[0] + self.t[0], r[1] + self.t[1], r[2] + self.t[2]]
    }

    /// FTransform::InverseTransformPosition: R^-1 (w - T) / S (a zero scale component gives 0).
    pub fn inverse_position(&self, w: V3) -> V3 {
        let r = qrot(self.q, [w[0] - self.t[0], w[1] - self.t[1], w[2] - self.t[2]], true);
        [0, 1, 2].map(|k| if self.s[k] == 0.0 { 0.0 } else { r[k] / self.s[k] })
    }
}

/// A fighter's mesh component in the world: at UE (0, y, z) (y = -the sim's x, z = the sim's y plus the mesh's
/// offset: CharView::place), the actor's yaw -90 facing right and +90 facing left (SetRotationLogic,
/// K2_SetActorRotation), the mesh's world scale. The pose a node reads is the one before the AnimBP's mirror node
/// (Batman_AnimBP: every node of this module sits below WAnimationMirrorNode), so this is the build's transform, not
/// the view's drawn reflection.
pub fn mesh_xf(y: f64, z: f64, facing: f64, scale: f64) -> UeXf {
    let h = (if facing > 0.0 { -90.0 } else { 90.0 }) * std::f64::consts::PI / 180.0 / 2.0;
    UeXf { t: [0.0, y, z], q: [0.0, 0.0, h.sin(), h.cos()], s: [scale; 3] }
}

/// Bone i's local transform of a view's pose in the build's units and axes (cm; UE X / Y / Z; a UE quaternion).
pub fn local_ue(p: &Pose, i: usize) -> UeXf {
    let (i3, i4) = (i * 3, i * 4);
    let f = |v: &[f32], k: usize| f64::from(v[k]);
    UeXf {
        t: [f(&p.t, i3) * 100.0, f(&p.t, i3 + 2) * 100.0, f(&p.t, i3 + 1) * 100.0],
        q: [f(&p.r, i4), f(&p.r, i4 + 2), f(&p.r, i4 + 1), -f(&p.r, i4 + 3)],
        s: [f(&p.s, i3), f(&p.s, i3 + 2), f(&p.s, i3 + 1)],
    }
}

// ---- the component-space pose (FCSPose over the view's local pose) ----

/// The bone transforms a node hands to LocalBlendCSBoneTransforms (FBoneTransform list), in the build's units.
#[derive(Clone, Debug, Default)]
pub struct BoneXfs {
    pub list: Vec<(usize, UeXf)>,
}

impl BoneXfs {
    pub fn clear(&mut self) {
        self.list.clear();
    }

    pub fn push(&mut self, b: usize, x: UeXf) {
        self.list.push((b, x));
    }

    /// Sorted by bone index (14244e700), in place (the insertion sort of a handful of entries: stable).
    pub fn sort(&mut self) {
        self.list.sort_by_key(|e| e.0);
    }
}

/// The skeleton a node is made on: the view's bone names and parents, and its reference pose (CharView::rest) for a
/// node that reads it (BoneDrivenController; one without it refuses to be made).
#[derive(Clone, Copy, Debug)]
pub struct AnimNodeSkel<'a> {
    pub names: &'a [String],
    pub parents: &'a [i32],
    pub rest: Option<&'a Pose>,
}

/// The component-space transforms of a view's skeleton, worked out on demand over a local pose (a bone's from its
/// parent's), in the build's units: the cache, made once a view. [`CsPose::begin`] puts it on a pose.
#[derive(Clone, Debug)]
pub struct CsPose {
    pub names: Vec<String>,
    pub parents: Vec<i32>,
    xf: Vec<UeXf>,
    valid: Vec<bool>,
    /// local_blend's scratch: the blended bones' local transforms before the set.
    saved: Vec<[f32; 10]>,
}

impl CsPose {
    pub fn new(names: &[String], parents: &[i32]) -> CsPose {
        let n = names.len();
        CsPose {
            names: names.to_vec(),
            parents: parents.to_vec(),
            xf: vec![UeXf::default(); n],
            valid: vec![false; n],
            saved: Vec::new(),
        }
    }

    pub fn bone(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }

    /// The local pose of this evaluation (the view's, changed in place).
    pub fn begin<'a>(&'a mut self, p: &'a mut Pose) -> CsEval<'a> {
        assert_eq!(p.n, self.names.len(), "CsPose: a pose of {} bones on a skeleton of {}", p.n, self.names.len());
        self.valid.fill(false);
        CsEval { cs: self, p }
    }
}

/// A [`CsPose`] on one evaluation's local pose. The local pose stays the one the view draws: a node's output is
/// written back into it as local transforms, so the bones under a moved bone keep their local transforms and follow
/// it (SafeSetCSBoneTransforms 14248ae90).
#[derive(Debug)]
pub struct CsEval<'a> {
    cs: &'a mut CsPose,
    p: &'a mut Pose,
}

impl CsEval<'_> {
    pub fn names(&self) -> &[String] {
        &self.cs.names
    }

    pub fn parents(&self) -> &[i32] {
        &self.cs.parents
    }

    /// The local pose under evaluation.
    pub fn pose(&self) -> &Pose {
        self.p
    }

    /// Bone i's local transform in the build's units and axes (FCSPose's local pose).
    pub fn local_of(&self, i: usize) -> UeXf {
        local_ue(self.p, i)
    }

    /// A local transform `l` of bone b, in the build's units, composed with b's parent's component-space transform
    /// (the root's taken as it is) (FTransform local x parent).
    pub fn to_component(&mut self, b: usize, l: &UeXf) -> UeXf {
        let pa = self.cs.parents[b];
        if pa < 0 {
            return *l;
        }
        let p = self.get(pa as usize);
        let t = qrot(p.q, [l.t[0] * p.s[0], l.t[1] * p.s[1], l.t[2] * p.s[2]], false);
        UeXf {
            t: [t[0] + p.t[0], t[1] + p.t[1], t[2] + p.t[2]],
            q: qmul(p.q, l.q),
            s: [p.s[0] * l.s[0], p.s[1] * l.s[1], p.s[2] * l.s[2]],
        }
    }

    /// Bone i's component-space transform.
    pub fn get(&mut self, i: usize) -> UeXf {
        if self.cs.valid[i] {
            return self.cs.xf[i];
        }
        let pa = self.cs.parents[i];
        let l = local_ue(self.p, i);
        let x = if pa < 0 {
            l
        } else {
            let p = self.get(pa as usize);
            // local * parent: translation R_p (S_p t) + T_p, rotation R_p R, scale S_p S
            let t = qrot(p.q, [l.t[0] * p.s[0], l.t[1] * p.s[1], l.t[2] * p.s[2]], false);
            UeXf {
                t: [t[0] + p.t[0], t[1] + p.t[1], t[2] + p.t[2]],
                q: qmul(p.q, l.q),
                s: [p.s[0] * l.s[0], p.s[1] * l.s[1], p.s[2] * l.s[2]],
            }
        };
        self.cs.xf[i] = x;
        self.cs.valid[i] = true;
        x
    }

    /// Bone b's component-space transform set to `x`: its local one becomes that relative to the parent's
    /// (GetRelativeTransform: R_p^-1 (T - T_p) / S_p, R_p^-1 R, S / S_p, a zero parent scale component giving 0), and
    /// every component transform below it is worked out again from the local ones.
    fn set(&mut self, b: usize, x: &UeXf) {
        let pa = self.cs.parents[b];
        let (lt, lr, ls) = if pa < 0 {
            (x.t, x.q, x.s)
        } else {
            let p = self.get(pa as usize);
            let r = qrot(p.q, [x.t[0] - p.t[0], x.t[1] - p.t[1], x.t[2] - p.t[2]], true);
            let div = |v: f64, d: f64| if d == 0.0 { 0.0 } else { v / d };
            let lt = [div(r[0], p.s[0]), div(r[1], p.s[1]), div(r[2], p.s[2])];
            let ls = [div(x.s[0], p.s[0]), div(x.s[1], p.s[1]), div(x.s[2], p.s[2])];
            // R_p^-1 R
            (lt, qmul([-p.q[0], -p.q[1], -p.q[2], p.q[3]], x.q), ls)
        };
        // back to the view's axes
        let (b3, b4) = (b * 3, b * 4);
        let p = &mut *self.p;
        p.t[b3] = (lt[0] / 100.0) as f32;
        p.t[b3 + 1] = (lt[2] / 100.0) as f32;
        p.t[b3 + 2] = (lt[1] / 100.0) as f32;
        p.r[b4] = lr[0] as f32;
        p.r[b4 + 1] = lr[2] as f32;
        p.r[b4 + 2] = lr[1] as f32;
        p.r[b4 + 3] = (-lr[3]) as f32;
        p.s[b3] = ls[0] as f32;
        p.s[b3 + 1] = ls[2] as f32;
        p.s[b3 + 2] = ls[1] as f32;
        self.cs.valid.fill(false);
        self.cs.xf[b] = *x;
        self.cs.valid[b] = true;
    }

    /// LocalBlendCSBoneTransforms 142485bc0 (sorted list): alpha < 1e-5 nothing; >= 0.99999 the component transforms
    /// set outright; between, the bones' local transforms are saved, the component transforms set, and each bone's
    /// local transform blended from the saved one to the new one by alpha (translation and scale linear, rotation
    /// linear on the shorter arc then normalised: FTransform::BlendWith).
    pub fn local_blend(&mut self, x: &BoneXfs, alpha: f64) {
        if x.list.is_empty() || alpha < 1e-5 {
            return;
        }
        let partial = alpha < 0.99999;
        // (the local transforms as the pose holds them: float32)
        if partial {
            let p = &*self.p;
            self.cs.saved.clear();
            self.cs.saved.extend(x.list.iter().map(|&(b, _)| {
                let mut s = [0.0f32; 10];
                s[..3].copy_from_slice(&p.t[b * 3..b * 3 + 3]);
                s[3..7].copy_from_slice(&p.r[b * 4..b * 4 + 4]);
                s[7..].copy_from_slice(&p.s[b * 3..b * 3 + 3]);
                s
            }));
        }
        for (b, xf) in &x.list {
            self.set(*b, xf);
        }
        if !partial {
            return;
        }
        // The blend runs in the local transforms, so it is the same in the view's axes (a linear map of each channel).
        let (a, o) = (alpha, 1.0 - alpha);
        let p = &mut *self.p;
        for (&(b, _), sv) in x.list.iter().zip(&self.cs.saved) {
            let sv = sv.map(f64::from);
            let (b3, b4) = (b * 3, b * 4);
            for j in 0..3 {
                p.t[b3 + j] = (sv[j] * o + f64::from(p.t[b3 + j]) * a) as f32;
                p.s[b3 + j] = (sv[7 + j] * o + f64::from(p.s[b3 + j]) * a) as f32;
            }
            let r = [0, 1, 2, 3].map(|k| f64::from(p.r[b4 + k]));
            let dot = sv[3] * r[0] + sv[4] * r[1] + sv[5] * r[2] + sv[6] * r[3];
            let bias = if dot >= 0.0 { a } else { -a };
            let q = [0, 1, 2, 3].map(|k| sv[3 + k] * o + r[k] * bias);
            let l = hypot(&q);
            let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
            for (d, v) in p.r[b4..b4 + 4].iter_mut().zip(q) {
                *d = (v / l) as f32;
            }
        }
        self.cs.valid.fill(false);
    }
}

// ---- the nodes ----

/// What every node is to its caller.
pub trait AnimNode: std::fmt::Debug + Send {
    /// The node's struct name (AnimNode_SpringBone).
    fn ty(&self) -> &'static str;
    /// Initialize_AnyThread: the node's own reset.
    fn initialize(&mut self) {}
    /// PreUpdate (game thread, once per anim update): the world settings' effective time dilation (not the actor's).
    fn pre_update(&mut self, _world_dilation: f64) {}
    /// Update_AnyThread: the base's gate (clamp(alpha, 0, 1) > 1e-5 and valid) around the node's UpdateInternal; dt
    /// is the anim update's DeltaTime. The alpha is kept for the evaluation (ActualAlpha).
    fn update(&mut self, dt: f64, alpha: f64);
    /// EvaluateComponentSpace_AnyThread: under the same gate, the node's bone transforms blended into cs by the
    /// alpha.
    fn evaluate(&mut self, cs: &mut CsEval<'_>, comp: &UeXf);
}

/// What makes a node from its properties on a view's skeleton.
pub type AnimNodeMaker = fn(&Props, &AnimNodeSkel<'_>) -> Box<dyn AnimNode>;

/// The maker of a node struct; panics for a type no maker is entered for.
pub fn anim_node_of(ty: &str) -> AnimNodeMaker {
    match ty {
        "AnimNode_SpringBone" => |p, s| Box::new(SpringBoneNode::new(p, s)),
        "AnimNode_LegIK" => |p, s| Box::new(LegIkNode::new(p, s)),
        "AnimNode_Constraint" => |p, s| Box::new(ConstraintNode::new(p, s)),
        "AnimNode_CopyBone" => |p, s| Box::new(CopyBoneNode::new(p, s)),
        "AnimNode_BoneDrivenController" => |p, s| Box::new(BoneDrivenControllerNode::new(p, s)),
        "AnimNode_AnimDynamics" => |p, s| Box::new(AnimDynamicsNode::new(p, s)),
        "AnimNode_Trail" => |p, s| Box::new(TrailNode::new(p, s)),
        _ => panic!("no anim node entered for {ty} (anim_nodes)"),
    }
}

/// A property's JavaScript truthiness (`!!props.x`).
pub(crate) fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Value::Str(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

pub(crate) fn num(p: &Props, k: &str) -> Option<f64> {
    p.get(k).and_then(Value::as_f64)
}

pub(crate) fn text<'a>(p: &'a Props, k: &str) -> Option<&'a str> {
    p.get(k).and_then(Value::as_str)
}

/// FAnimNode_SkeletalControlBase's alpha: the Float input through AlphaScaleBias and AlphaScaleBiasClamp, which the
/// port takes only at their defaults (Scale 1, Bias 0, no map / clamp / interp: every node of Batman_AnimBP has
/// them); and no LODThreshold (-1: the node runs at every LOD, as Batman's do), since the port has no mesh LOD to test
/// it on.
pub(crate) fn check_alpha_defaults(ty: &str, props: &Props) {
    let obj = |k: &str| props.get(k).and_then(Value::as_obj);
    let n = |o: &Props, k: &str| num(o, k);
    let sb_ok = obj("AlphaScaleBias").is_none_or(|o| n(o, "Scale") == Some(1.0) && n(o, "Bias") == Some(0.0));
    // (a clamp of the result to a range holding [0, 1] changes no alpha the base then clamps to [0, 1]: the skins'
    // SpringBone / AnimDynamics nodes of C023A_AnimBP and Batman_AnimBP clamp to [0, 1])
    let clamp_ok = |c: &Props| {
        !truthy(c.get("bClampResult"))
            || (n(c, "ClampMin").unwrap_or(0.0) <= 0.0 && n(c, "ClampMax").unwrap_or(1.0) >= 1.0)
    };
    let c_ok = obj("AlphaScaleBiasClamp").is_none_or(|c| {
        !truthy(c.get("bMapRange"))
            && clamp_ok(c)
            && !truthy(c.get("bInterpResult"))
            && n(c, "Scale") == Some(1.0)
            && n(c, "Bias") == Some(0.0)
    });
    let input_ok = text(props, "AlphaInputType").is_none_or(|t| t == "EAnimAlphaInputType::Float");
    let curve_ok = text(props, "AlphaCurveName").is_none_or(|t| t == "None");
    assert!(input_ok && sb_ok && c_ok && curve_ok, "{ty}: an alpha input other than a plain float is not ported");
    check_lod(ty, props);
}

fn check_lod(ty: &str, props: &Props) {
    if let Some(l) = num(props, "LODThreshold") {
        assert!(l < 0.0, "{ty}: a LODThreshold of {l} is not ported (only -1)");
    }
}

/// The same for the Constraint, CopyBone and BoneDrivenController nodes: also a Bool input with both AlphaBoolBlend
/// times 0 (Arya_AnimBP's Constraints), whose alpha is the bool itself (0 or 1: the caller gives it).
pub(crate) fn check_control_alpha(ty: &str, props: &Props) {
    if text(props, "AlphaInputType") == Some("EAnimAlphaInputType::Bool") {
        if let Some(b) = props.get("AlphaBoolBlend").and_then(Value::as_obj) {
            assert!(
                num(b, "BlendInTime") == Some(0.0) && num(b, "BlendOutTime") == Some(0.0),
                "{ty}: a bool alpha with a blend time is not ported"
            );
        }
        check_lod(ty, props);
        return;
    }
    check_alpha_defaults(ty, props);
}

/// `clamp01` (NaN stays NaN).
pub fn clamp01(a: f64) -> f64 {
    a.clamp(0.0, 1.0)
}

/// The bone a node property names (`{ BoneName }`); None (warned): the node does nothing.
pub(crate) fn bone_of(ty: &str, skel: &AnimNodeSkel<'_>, r: Option<&Value>) -> Option<usize> {
    let name = r.and_then(Value::as_obj).and_then(|o| text(o, "BoneName")).unwrap_or("None");
    let i = skel.names.iter().position(|n| n == name);
    if i.is_none() {
        eprintln!("warning: {ty}: bone {name} is not in the skeleton: the node does nothing");
    }
    i
}

// ---- a run of nodes in a graph's order ----

/// The nodes of one stretch of a graph between two space conversions, made once a view, in evaluation order (the
/// input first). update / evaluate take one alpha per node.
#[derive(Debug)]
pub struct AnimNodeChain {
    pub nodes: Vec<Box<dyn AnimNode>>,
    pub cs: CsPose,
}

impl AnimNodeChain {
    /// `defs`: each node's struct name (AnimNode_SpringBone) and its properties, as the AnimBP's data has them.
    pub fn new(defs: &[(&str, &Props)], skel: &AnimNodeSkel<'_>) -> AnimNodeChain {
        AnimNodeChain {
            nodes: defs.iter().map(|(ty, props)| anim_node_of(ty)(props, skel)).collect(),
            cs: CsPose::new(skel.names, skel.parents),
        }
    }

    pub fn initialize(&mut self) {
        for n in &mut self.nodes {
            n.initialize();
        }
    }

    pub fn update(&mut self, dt: f64, alphas: &[f64], world_dilation: f64) {
        for (n, &a) in self.nodes.iter_mut().zip(alphas) {
            n.pre_update(world_dilation);
            n.update(dt, a);
        }
    }

    /// The nodes on the local pose p, in place.
    pub fn evaluate(&mut self, p: &mut Pose, comp: &UeXf) {
        let mut cs = self.cs.begin(p);
        for n in &mut self.nodes {
            n.evaluate(&mut cs, comp);
        }
    }
}

#[cfg(test)]
mod tests;

// ---- nodes made from an AnimBP's data ----

/// The bone properties a node names its bones by (the node's own bones: a planar or spherical limit's driving bone is
/// not one of them).
const OWN_BONES: [&str; 4] = ["BoundBone", "ChainEnd", "TrailBone", "SpringBone"];

/// Whether every bone the node's properties name (OWN_BONES) is in the skeleton. A node on a bone the view's mesh does
/// not hold (another skin's) is invalid in the build and does nothing: it is left out, with no warning.
pub fn bones_held(props: &Props, skel: &AnimNodeSkel<'_>) -> bool {
    OWN_BONES.iter().all(|k| {
        let name = props.get(*k).and_then(Value::as_obj).and_then(|o| text(o, "BoneName"));
        name.is_none_or(|n| n == "None" || skel.names.iter().any(|x| x == n))
    })
}

/// The nodes `names` (AnimGraphNode_* names, in the graph's evaluation order) of an AnimBP's data, each made on the
/// view's skeleton when it holds the node's bones ([`bones_held`]), with the index in `names` of each made node. A name
/// the data lacks panics (the data is the AnimBP the caller names).
pub fn nodes_of_data(
    data: &mvs_data::fighter::Map<mvs_data::fighter::AnimNodeData>,
    names: &[&str],
    skel: &AnimNodeSkel<'_>,
) -> (AnimNodeChain, Vec<usize>) {
    let mut defs = Vec::new();
    let mut idx = Vec::new();
    for (i, n) in names.iter().enumerate() {
        let d = data.get(*n).unwrap_or_else(|| panic!("anim nodes: the AnimBP's data has no {n}"));
        if bones_held(&d.props, skel) {
            defs.push((d.ty.as_str(), &d.props));
            idx.push(i);
        }
    }
    (AnimNodeChain::new(&defs, skel), idx)
}

/// Nodes made from an AnimBP's data, with the index in the name list of each.
type MadeNodes = (Vec<Box<dyn AnimNode>>, Vec<usize>);

/// Nodes of an AnimBP's data that a view runs as one stretch of the graph (the skins' Trail / AnimDynamics /
/// SpringBone nodes), made at the fighter's first update: the data is the fighter's, which a view gets there.
#[derive(Debug)]
pub struct SkinNodes {
    /// The AnimGraphNode_* names, in the graph's evaluation order.
    names: &'static [&'static str],
    /// The nodes the view's mesh holds the bones of, with the index in `names` of each (None: not made yet).
    made: Option<MadeNodes>,
    cs: CsPose,
}

impl SkinNodes {
    pub fn new(names: &'static [&'static str], skel: &AnimNodeSkel<'_>) -> SkinNodes {
        SkinNodes { names, made: None, cs: CsPose::new(skel.names, skel.parents) }
    }

    /// The nodes dropped: made anew at the next make (a new pawn).
    pub fn clear(&mut self) {
        self.made = None;
    }

    pub fn is_made(&self) -> bool {
        self.made.is_some()
    }

    /// Whether any node runs on the view's mesh.
    pub fn any(&self) -> bool {
        self.made.as_ref().is_some_and(|m| !m.0.is_empty())
    }

    /// The nodes of `data` (None: no node, with a warning naming `who`), then InitAnim's DeltaTime-0 tick (the
    /// module's header) at `alpha_of(index in names)`.
    pub fn make(
        &mut self,
        data: Option<&mvs_data::fighter::Map<mvs_data::fighter::AnimNodeData>>,
        skel: &AnimNodeSkel<'_>,
        who: &str,
        alpha_of: impl Fn(usize) -> f64,
    ) {
        let Some(data) = data else {
            eprintln!("warning: {who}: the fighter data has no anim nodes: the skin's nodes are not run");
            self.made = Some((Vec::new(), Vec::new()));
            return;
        };
        let (chain, idx) = nodes_of_data(data, self.names, skel);
        let mut nodes = chain.nodes;
        for (n, &i) in nodes.iter_mut().zip(&idx) {
            n.initialize();
            n.pre_update(1.0);
            n.update(0.0, alpha_of(i));
        }
        self.made = Some((nodes, idx));
    }

    /// The nodes' update (the world's dilation 1) at `alpha_of(index in names)`.
    pub fn update(&mut self, dt: f64, alpha_of: impl Fn(usize) -> f64) {
        let Some((nodes, idx)) = &mut self.made else { return };
        for (n, &i) in nodes.iter_mut().zip(idx.iter()) {
            n.pre_update(1.0);
            n.update(dt, alpha_of(i));
        }
    }

    /// The nodes on the local pose p, in place.
    pub fn evaluate(&mut self, p: &mut Pose, comp: &UeXf) {
        let Some((nodes, _)) = &mut self.made else { return };
        if nodes.is_empty() {
            return;
        }
        let mut cs = self.cs.begin(p);
        for n in nodes.iter_mut() {
            n.evaluate(&mut cs, comp);
        }
    }
}

/// SetIsRespawning's Delay (Batman_AnimBP @12649, C020_AnimBPV2 @4438, Superman_AnimBP @8544, C023A_AnimBP @12120)
/// before Is Respawning goes back to false.
const RESPAWN_DELAY: f32 = 0.1;

/// An AnimBP's Is Respawning as a view sees it: true from the respawn for SetIsRespawning's Delay(0.1) (its latent
/// frames). The respawn is the first update after ticks the view was not posed (the fighter was dead between), as
/// Rick's pose hook sees it. Unverified: OnFighterReset's call reaches the AnimBP at the respawn's tick.
#[derive(Debug, Default)]
pub struct RespawnFlag {
    last_tick: Option<i32>,
    left: f32,
}

impl RespawnFlag {
    /// The update of World tick `tick`: Is Respawning.
    pub fn step(&mut self, tick: i32) -> bool {
        if self.last_tick.is_some_and(|t| tick > t + 1) {
            self.left = mvs_sim::fighter::common::latent_frames(RESPAWN_DELAY);
        } else if self.left > 0.0 {
            self.left -= 1.0;
        }
        self.last_tick = Some(tick);
        self.left > 0.0
    }

    /// A new pawn: no update seen.
    pub fn reset(&mut self) {
        *self = RespawnFlag::default();
    }
}
