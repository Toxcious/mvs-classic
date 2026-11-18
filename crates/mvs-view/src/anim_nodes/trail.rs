//! FAnimNode_Trail (stock 4.26, read in the exe: Initialize 142483f70, InitializeBoneReferences 142481880,
//! UpdateInternal 142491710, EvaluateSkeletalControl 14246b790, the rotation limit 1424443d0): a chain of ChainLength
//! bones ending at TrailBone whose bones below the first trail behind where the pose has them, each pulled toward its
//! animated offset from its (trailed) parent at a per-joint relaxation speed, its parent turned to point at it. The
//! No pose tape covers these nodes.
//!
//! What the node refuses where it is made (no data of the build uses it): a MaxDeltaTime, fake velocity, a BaseJoint,
//! a non-default RelaxationSpeedScaleInputProcessor, a relaxation curve key other than linear or constant, an alpha
//! other than a plain float, bLimitRotation without the skeleton's reference pose.
//! Unverified: the base's gate (an alpha at 0 accumulates no time and keeps the "had valid strength" flag) is the
//! 4.26 source's, not read in the exe.

use mvs_data::value::{Props, Value};

use super::anim_dynamics::{compose, relative};
use super::{
    AnimNode, AnimNodeSkel, BoneXfs, CsEval, Q4, UeXf, V3, bone_of, check_alpha_defaults, clamp01,
    find_between_normals, local_ue, num, qmul, qrot, safe_normal, text, truthy,
};
use crate::rotator::{normalize_axis, quat_euler, rotator_quat};

const TY: &str = "AnimNode_Trail";

#[derive(Clone, Debug)]
struct Planar {
    bone: Option<usize>,
    xf: UeXf,
}

#[derive(Debug)]
pub struct TrailNode {
    /// The chain, top first; the last is the trail bone.
    pub chain: Vec<usize>,
    /// The chain axis in the bone's frame (ChainBoneAxis, negated with bInvertChainBoneAxis).
    axis: V3,
    limit_stretch: bool,
    stretch_limit: f64,
    reorient: bool,
    relaxation_scale: f64,
    per_joint: Vec<f64>,
    planar: Vec<Planar>,
    /// bLimitRotation: per chain bone, (min, max) of (Yaw, Pitch, Roll) as FVector X / Y / Z (limits + offsets), and
    /// each bone's reference-pose local rotation.
    rot_limits: Option<Vec<(V3, V3)>>,
    ref_rot: Vec<Q4>,
    /// The parent of the chain's top bone (the rotation limit's grandparent at i == 1).
    top_parent: Option<usize>,
    last_alpha_blend: f64,
    // the node's state
    pub locations: Vec<V3>,
    this_timestep: f64,
    had_valid_strength: bool,
    old_base: UeXf,
    actual_alpha: f64,
    out: BoneXfs,
    scratch: Vec<UeXf>,
}

fn vec3(v: Option<&Value>) -> V3 {
    let o = v.and_then(Value::as_obj);
    let c = |k: &str| o.and_then(|o| num(o, k)).unwrap_or(0.0);
    [c("X"), c("Y"), c("Z")]
}

fn xf_of(v: Option<&Value>) -> UeXf {
    let o = v.and_then(Value::as_obj);
    let r = o.and_then(|o| o.get("Rotation")).and_then(Value::as_obj);
    let q = |k: &str, d: f64| r.and_then(|r| num(r, k)).unwrap_or(d);
    let s = o.and_then(|o| o.get("Scale3D"));
    UeXf {
        t: vec3(o.and_then(|o| o.get("Translation"))),
        q: [q("X", 0.0), q("Y", 0.0), q("Z", 0.0), q("W", 1.0)],
        s: if s.is_some() { vec3(s) } else { [1.0; 3] },
    }
}

/// The relaxation curve (an FRuntimeFloatCurve's rich curve) at t: linear or constant keys, constant beyond the ends.
fn curve_at(keys: &[(f64, f64, bool)], t: f64) -> f64 {
    let Some(first) = keys.first() else { return 0.0 };
    if t <= first.0 {
        return first.1;
    }
    for w in keys.windows(2) {
        let (a, b) = (w[0], w[1]);
        if t < b.0 {
            if a.2 {
                return a.1;
            }
            let d = b.0 - a.0;
            return if d > 0.0 { a.1 + (b.1 - a.1) * ((t - a.0) / d) } else { a.1 };
        }
    }
    keys[keys.len() - 1].1
}

impl TrailNode {
    pub fn new(props: &Props, skel: &AnimNodeSkel<'_>) -> TrailNode {
        check_alpha_defaults(TY, props);
        let flag = |k: &str| truthy(props.get(k));
        assert!(num(props, "MaxDeltaTime").unwrap_or(0.0) == 0.0, "{TY}: a MaxDeltaTime is not ported");
        assert!(vec3(props.get("FakeVelocity")) == [0.0; 3], "{TY}: fake velocity is not ported");
        let base = props.get("BaseJoint").and_then(Value::as_obj).and_then(|o| text(o, "BoneName"));
        assert!(base.is_none_or(|b| b == "None"), "{TY}: a BaseJoint is not ported");
        if let Some(p) = props.get("RelaxationSpeedScaleInputProcessor").and_then(Value::as_obj) {
            let ok = !truthy(p.get("bMapRange"))
                && !truthy(p.get("bClampResult"))
                && !truthy(p.get("bInterpResult"))
                && num(p, "Scale").unwrap_or(1.0) == 1.0
                && num(p, "Bias").unwrap_or(0.0) == 0.0;
            assert!(ok, "{TY}: a RelaxationSpeedScaleInputProcessor other than the identity is not ported");
        }
        let n = num(props, "ChainLength").unwrap_or(2.0).max(0.0) as usize;
        let mut chain = Vec::new();
        let mut c = bone_of(TY, skel, props.get("TrailBone"));
        while let Some(i) = c {
            if chain.len() == n {
                break;
            }
            chain.push(i);
            c = usize::try_from(skel.parents[i]).ok();
        }
        if chain.len() < n {
            eprintln!("warning: {TY}: the chain is shorter than ChainLength {n}: the node does nothing");
            chain.clear();
        }
        chain.reverse();
        let top_parent = chain.first().and_then(|&b| usize::try_from(skel.parents[b]).ok());
        let k = match text(props, "ChainBoneAxis").and_then(|s| s.rsplit("::").next()) {
            Some("X") | None => 0,
            Some("Y") => 1,
            Some("Z") => 2,
            Some(o) => panic!("{TY}: chain axis {o} is not ported"),
        };
        let mut axis = [0.0; 3];
        axis[k] = if flag("bInvertChainBoneAxis") { -1.0 } else { 1.0 };
        let keys: Vec<(f64, f64, bool)> = props
            .get("TrailRelaxationSpeed")
            .and_then(Value::as_obj)
            .and_then(|o| o.get("EditorCurveData"))
            .and_then(Value::as_obj)
            .and_then(|o| o.get("Keys"))
            .and_then(Value::as_arr)
            .unwrap_or(&[])
            .iter()
            .filter_map(Value::as_obj)
            .map(|k| {
                let mode = text(k, "InterpMode").unwrap_or("RCIM_Linear");
                assert!(mode == "RCIM_Linear" || mode == "RCIM_Constant", "{TY}: a {mode} curve key is not ported");
                (num(k, "Time").unwrap_or(0.0), num(k, "Value").unwrap_or(0.0), mode == "RCIM_Constant")
            })
            .collect();
        // the ctor's curve where the data has none: (0, 10) (1, 5)
        let keys = if keys.is_empty() { vec![(0.0, 10.0, false), (1.0, 5.0, false)] } else { keys };
        let len = chain.len();
        let per_joint = (0..len)
            .map(|i| {
                let t = if len > 1 { i as f64 / (len - 1) as f64 } else { 0.0 };
                f64::from(curve_at(&keys, t) as f32)
            })
            .collect();
        let planar = if flag("bUsePlanarLimit") {
            props
                .get("PlanarLimits")
                .and_then(Value::as_arr)
                .unwrap_or(&[])
                .iter()
                .filter_map(Value::as_obj)
                .map(|o| {
                    let name = o.get("DrivingBone").and_then(Value::as_obj).and_then(|d| text(d, "BoneName"));
                    let bone = name.filter(|n| *n != "None").and_then(|n| skel.names.iter().position(|x| x == n));
                    Planar { bone, xf: xf_of(o.get("PlaneTransform")) }
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut ref_rot = Vec::new();
        let rot_limits = if flag("bLimitRotation") {
            let rest = skel.rest.unwrap_or_else(|| panic!("{TY}: bLimitRotation needs the reference pose"));
            ref_rot = chain.iter().map(|&b| local_ue(rest, b).q).collect();
            let arr = |k: &str| props.get(k).and_then(Value::as_arr).unwrap_or(&[]).to_vec();
            let (lims, offs) = (arr("RotationLimits"), arr("RotationOffsets"));
            Some(
                (0..len)
                    .map(|i| {
                        let l = lims.get(i).and_then(Value::as_obj);
                        let o = offs.get(i).map_or([0.0; 3], |v| vec3(Some(v)));
                        let mn = vec3(l.and_then(|l| l.get("LimitMin")));
                        let mx = vec3(l.and_then(|l| l.get("LimitMax")));
                        ([0, 1, 2].map(|k| mn[k] + o[k]), [0, 1, 2].map(|k| mx[k] + o[k]))
                    })
                    .collect(),
            )
        } else {
            None
        };
        TrailNode {
            chain,
            axis,
            limit_stretch: flag("bLimitStretch"),
            stretch_limit: num(props, "StretchLimit").unwrap_or(0.0),
            reorient: props.get("bReorientParentToChild").is_none_or(|v| truthy(Some(v))),
            relaxation_scale: num(props, "RelaxationSpeedScale").unwrap_or(1.0),
            per_joint,
            planar,
            rot_limits,
            ref_rot,
            top_parent,
            last_alpha_blend: num(props, "LastBoneRotationAnimAlphaBlend").unwrap_or(0.0),
            locations: vec![[0.0; 3]; len],
            this_timestep: 0.0,
            had_valid_strength: false,
            old_base: UeXf::default(),
            actual_alpha: 0.0,
            out: BoneXfs::default(),
            scratch: Vec::new(),
        }
    }

    /// FUN_1424443d0: the local rotation `local` of chain bone j limited against its reference rotation: the delta to
    /// the reference as an FRotator, Pitch clamped to [Min.Y, Max.Y], Yaw to [Min.X, Max.X], Roll to [Min.Z, Max.Z];
    /// Some(the limited rotation) when a component moved by more than 1e-4.
    fn limit_rotation(&self, j: usize, local: Q4) -> Option<Q4> {
        let (mn, mx) = self.rot_limits.as_ref()?[j];
        let r = self.ref_rot[j];
        let delta = qmul([-r[0], -r[1], -r[2], r[3]], local);
        let [roll, pitch, yaw] = quat_euler(delta).map(normalize_axis);
        let p = pitch.clamp(mn[1], mx[1]);
        let y = yaw.clamp(mn[0], mx[0]);
        let ro = roll.clamp(mn[2], mx[2]);
        let moved = (p - pitch).abs() > 1e-4 || (y - yaw).abs() > 1e-4 || (ro - roll).abs() > 1e-4;
        moved.then(|| qmul(r, rotator_quat(p, y, ro)))
    }
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn len(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn qinv(q: Q4) -> Q4 {
    [-q[0], -q[1], -q[2], q[3]]
}

impl AnimNode for TrailNode {
    fn ty(&self) -> &'static str {
        TY
    }

    /// Initialize 142483f70: the locations zeroed, the time anew.
    fn initialize(&mut self) {
        self.locations.fill([0.0; 3]);
        self.this_timestep = 0.0;
        self.had_valid_strength = false;
    }

    /// UpdateInternal 142491710 under the base's gate: ThisTimestep += DeltaTime.
    fn update(&mut self, dt: f64, alpha: f64) {
        self.actual_alpha = clamp01(alpha);
        if !(self.actual_alpha > 1e-5) || self.chain.len() < 2 {
            return;
        }
        self.this_timestep += f64::from(dt as f32);
    }

    /// EvaluateSkeletalControl 14246b790.
    fn evaluate(&mut self, cs: &mut CsEval<'_>, comp: &UeXf) {
        let n = self.chain.len();
        if n < 2 || !(self.actual_alpha > 1e-5) {
            return;
        }
        let time_step = self.this_timestep;
        self.this_timestep = 0.0;
        let base = *comp;
        // the chain's animated transforms (component space), the output's start
        self.scratch.clear();
        for &b in &self.chain {
            let x = cs.get(b);
            self.scratch.push(x);
        }
        let anim: Vec<V3> = self.scratch.iter().map(|x| x.t).collect();
        if !self.had_valid_strength {
            self.locations.copy_from_slice(&anim);
            self.old_base = base;
        }
        self.had_valid_strength = self.actual_alpha > 0.0;
        // OldBase relative to Base: last frame's component space into this frame's
        let old_to_new = relative(&self.old_base, &base);
        self.locations[0] = anim[0];
        let planes: Vec<(V3, V3)> = self
            .planar
            .iter()
            .map(|p| {
                let x = match p.bone {
                    Some(d) => compose(&p.xf, &cs.get(d)),
                    None => p.xf,
                };
                (x.t, safe_normal(qrot(x.q, [0.0, 0.0, 1.0], false), 1e-8))
            })
            .collect();
        let grand0 = self.top_parent.map_or([0.0, 0.0, 0.0, 1.0], |p| cs.get(p).q);
        for i in 1..n {
            let target_delta = sub(anim[i], anim[i - 1]);
            let target = {
                let p = self.locations[i - 1];
                [p[0] + target_delta[0], p[1] + target_delta[1], p[2] + target_delta[2]]
            };
            let t = old_to_new.position(self.locations[i]);
            let correction = (self.relaxation_scale * self.per_joint[i] * time_step).clamp(0.0, 1.0);
            let mut loc = [0, 1, 2].map(|k| t[k] + (target[k] - t[k]) * correction);
            if self.limit_stretch {
                let ref_len = len(target_delta);
                let d = sub(loc, self.locations[i - 1]);
                let cur = len(d);
                if cur - ref_len > self.stretch_limit && cur > 1e-8 {
                    let k = (ref_len + self.stretch_limit) / cur;
                    let p = self.locations[i - 1];
                    loc = [p[0] + d[0] * k, p[1] + d[1] * k, p[2] + d[2] * k];
                }
            }
            for (o, nrm) in &planes {
                let dist = (loc[0] - o[0]) * nrm[0] + (loc[1] - o[1]) * nrm[1] + (loc[2] - o[2]) * nrm[2];
                if dist < 0.0 {
                    loc = [0, 1, 2].map(|k| loc[k] - dist * nrm[k]);
                }
            }
            self.locations[i] = loc;
            self.scratch[i].t = loc;
            if self.reorient {
                let par = self.scratch[i - 1];
                let s = [self.axis[0] * par.s[0], self.axis[1] * par.s[1], self.axis[2] * par.s[2]];
                let cur = safe_normal(qrot(par.q, s, false), 1e-8);
                let new = safe_normal(sub(loc, par.t), 1e-8);
                let delta = find_between_normals(cur, new);
                self.scratch[i - 1].q = qmul(delta, par.q);
            }
            if self.rot_limits.is_some() {
                let grand = if i == 1 { grand0 } else { self.scratch[i - 2].q };
                let local = qmul(qinv(grand), self.scratch[i - 1].q);
                if let Some(lim) = self.limit_rotation(i - 1, local) {
                    let pq = qmul(grand, lim);
                    self.scratch[i - 1].q = pq;
                    let pl = self.scratch[i - 1].t;
                    let d = sub(loc, pl);
                    let x = qrot(pq, [1.0, 0.0, 0.0], false);
                    let l = len(d);
                    let loc2 = [0, 1, 2].map(|k| pl[k] + d[k] + (x[k] * l - d[k]) * correction);
                    self.locations[i] = loc2;
                    self.scratch[i].t = loc2;
                }
            }
        }
        // the last bone: FastLerp(parent, own, LastBoneRotationAnimAlphaBlend), normalised
        let (a, b) = (self.scratch[n - 2].q, self.scratch[n - 1].q);
        let w = self.last_alpha_blend;
        let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
        let bias = if dot >= 0.0 { w } else { -w };
        let q = [0, 1, 2, 3].map(|k| a[k] * (1.0 - w) + b[k] * bias);
        let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        self.scratch[n - 1].q = if l > 1e-8 { q.map(|c| c / l) } else { [0.0, 0.0, 0.0, 1.0] };
        self.old_base = base;
        self.out.clear();
        for (k, &bone) in self.chain.iter().enumerate() {
            self.out.push(bone, self.scratch[k]);
        }
        self.out.sort();
        cs.local_blend(&self.out, self.actual_alpha);
    }
}
