//! FAnimNode_SpringBone: a bone's world position on a damped spring
//! toward where the pose has it, stepped at a fixed 1/120 s with the world's dilation; the state lives in world space
//! and survives frames in which the node does not run.

use mvs_data::value::Props;

use super::{
    AnimNode, AnimNodeSkel, BoneXfs, CsEval, UeXf, bone_of, check_alpha_defaults, clamp01, find_between_normals, num,
    qmul, safe_normal, truthy,
};
use crate::rotator::{euler_quat, quat_euler};

#[derive(Debug)]
pub struct SpringBoneNode {
    pub bone: Option<usize>,
    pub parent: Option<usize>,
    pub max_displacement: f32,
    pub stiffness: f32,
    pub damping: f32,
    pub thresh: f32,
    pub limit: bool,
    pub translate: [bool; 3],
    pub rotate: [bool; 3],
    // the node's state (float32, as the exe holds it)
    /// World.
    pub bone_location: [f32; 3],
    /// World.
    pub bone_velocity: [f32; 3],
    /// Zero for a pawn's own mesh (PreUpdate).
    pub owner_velocity: [f32; 3],
    /// Component-space translation.
    pub local_bone_transform: [f32; 3],
    pub remaining_time: f32,
    pub fixed_time_step: f32,
    pub time_dilation: f32,
    pub actual_alpha: f64,
    /// (the port's own) the substeps of the last evaluation, for the tests.
    pub substeps: u32,
    out: BoneXfs,
}

const TY: &str = "AnimNode_SpringBone";

impl SpringBoneNode {
    pub fn new(props: &Props, skel: &AnimNodeSkel<'_>) -> SpringBoneNode {
        check_alpha_defaults(TY, props);
        let bone = bone_of(TY, skel, props.get("SpringBone"));
        let parent = bone.and_then(|b| usize::try_from(skel.parents[b]).ok());
        // the ctor's defaults (142453ea0) where the data leaves a value out
        let f = |k: &str, d: f64| num(props, k).unwrap_or(d) as f32;
        let on = |k: &str| props.get(k).filter(|v| !v.is_null()).is_none_or(|v| truthy(Some(v)));
        let flag = |k: &str| truthy(props.get(k));
        SpringBoneNode {
            bone,
            parent,
            max_displacement: f("MaxDisplacement", 0.0),
            stiffness: f("SpringStiffness", 50.0),
            damping: f("SpringDamping", 4.0),
            thresh: f("ErrorResetThresh", 256.0),
            limit: flag("bLimitDisplacement"),
            translate: [on("bTranslateX"), on("bTranslateY"), on("bTranslateZ")],
            rotate: [flag("bRotateX"), flag("bRotateY"), flag("bRotateZ")],
            bone_location: [0.0; 3],
            bone_velocity: [0.0; 3],
            owner_velocity: [0.0; 3],
            local_bone_transform: [0.0; 3],
            remaining_time: 0.0,
            fixed_time_step: 0.0,
            time_dilation: 1.0,
            actual_alpha: 0.0,
            substeps: 0,
            out: BoneXfs::default(),
        }
    }
}

impl AnimNode for SpringBoneNode {
    fn ty(&self) -> &'static str {
        TY
    }

    fn initialize(&mut self) {
        self.remaining_time = 0.0;
    }

    /// PreUpdate 142487d20: the world's effective dilation; OwnerVelocity is an attach parent owner's velocity for an
    /// ownerless attached mesh only, so zero for a pawn's mesh (owner_velocity left at zero; set it for such a mesh).
    fn pre_update(&mut self, world_dilation: f64) {
        self.time_dilation = world_dilation as f32;
    }

    /// UpdateInternal 1424916e0 under the base's gate: RemainingTime += DeltaTime; FixedTimeStep = dilation / 120.
    fn update(&mut self, dt: f64, alpha: f64) {
        self.actual_alpha = clamp01(alpha);
        if !(self.actual_alpha > 1e-5) || self.bone.is_none() {
            return;
        }
        self.remaining_time += dt as f32;
        self.fixed_time_step = self.time_dilation * 0.008333334f32;
    }

    fn evaluate(&mut self, cs: &mut CsEval<'_>, comp: &UeXf) {
        let Some(b) = self.bone.filter(|_| self.actual_alpha > 1e-5) else { return };
        assert!(
            comp.s.iter().all(|&s| s >= 0.0),
            "SpringBone: a component scale below zero (the exe's matrix path) is not ported"
        );
        self.substeps = 0;
        let tr = self.translate;
        if !tr[0] && !tr[1] && !tr[2] {
            return; // 1. no translate bit: no output
        }
        let bx = cs.get(b);
        // 2. the target: the bone's component transform through the component's (translation only)
        let t = comp.position(bx.t).map(|v| v as f32);
        let ov = self.owner_velocity;
        // 3. the first evaluation after Initialize
        if self.remaining_time == 0.0 {
            self.bone_location = t;
            self.bone_velocity = [0.0; 3];
        }
        let dt = self.fixed_time_step;
        if f64::from(dt.abs()) <= 1e-4 {
            // 4. the world paused: the bone stays where it was on the component
            let w = comp.position(self.local_bone_transform.map(f64::from));
            self.bone_location = w.map(|v| v as f32);
        } else {
            // 5. the substeps (float32, each operation rounded as the exe's scalar / SSE code does)
            let (k, d, th, mx) = (self.stiffness, self.damping, self.thresh, self.max_displacement);
            let (th2, mx2) = (th * th, mx * mx);
            let inv = 1.0f32 / dt;
            let (bl, v) = (&mut self.bone_location, &mut self.bone_velocity);
            while dt < self.remaining_time {
                for c in 0..3 {
                    bl[c] += dt * ov[c];
                }
                let e = [t[0] - bl[0], t[1] - bl[1], t[2] - bl[2]];
                if th2 < e[0] * e[0] + e[1] * e[1] + e[2] * e[2] {
                    *bl = t;
                    *v = [0.0; 3];
                }
                for c in 0..3 {
                    let acc = (t[c] - bl[c]) * k - v[c] * d;
                    v[c] = if d <= inv { v[c] + acc * dt } else { v[c] + (acc * dt) * (inv / d) };
                }
                let s = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() * dt;
                if s > th {
                    let q = th / s;
                    for c in v.iter_mut() {
                        *c *= q;
                    }
                }
                let old = *bl;
                for c in 0..3 {
                    bl[c] += v[c] * dt;
                    if !tr[c] {
                        bl[c] = t[c];
                    }
                }
                if self.limit {
                    let dd = [bl[0] - t[0], bl[1] - t[1], bl[2] - t[2]];
                    let d2 = dd[0] * dd[0] + dd[1] * dd[1] + dd[2] * dd[2];
                    if d2 > mx2 {
                        let n = safe_normal(dd.map(f64::from), 1e-8);
                        for c in 0..3 {
                            bl[c] = t[c] + (n[c] as f32) * mx;
                        }
                    }
                }
                for c in 0..3 {
                    v[c] = (bl[c] - old[c]) * inv;
                }
                self.remaining_time -= dt;
                self.substeps += 1;
            }
        }
        let tw = t.map(f64::from);
        let bw = self.bone_location.map(f64::from);
        // 6. the bone's place on the component
        self.local_bone_transform = comp.inverse_position(bw).map(|v| v as f32);
        // 7. the rotation, with a rotate bit: the turn from the parent toward the target to the parent toward the
        // bone, T and B in world space and the parent in component space (as the exe: the mix is kept)
        let rot = self.rotate;
        let mut rq = bx.q;
        if (rot[0] || rot[1] || rot[2])
            && let Some(pa) = self.parent
        {
            let p = cs.get(pa).t;
            let a = safe_normal([tw[0] - p[0], tw[1] - p[1], tw[2] - p[2]], 1e-8);
            let c = safe_normal([bw[0] - p[0], bw[1] - p[1], bw[2] - p[2]], 1e-8);
            let mut e = quat_euler(find_between_normals(a, c));
            for k in 0..3 {
                if !rot[k] {
                    e[k] = 0.0;
                }
            }
            rq = qmul(euler_quat(e), bx.q);
        }
        // 8. one bone transform: that rotation, the spring's translation, the pose's scale
        self.out.clear();
        self.out.push(b, UeXf { t: self.local_bone_transform.map(f64::from), q: rq, s: bx.s });
        cs.local_blend(&self.out, self.actual_alpha);
    }
}
