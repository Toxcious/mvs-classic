//! FAnimNode_AnimDynamics (stock 4.26, read in the exe: Initialize 142483160, UpdateInternal 142491510,
//! EvaluateSkeletalControl 14245f250, InitPhysics 14247dca0, UpdateLimits 142491730): one box body per bone (a chain
//! from BoundBone down to ChainEnd with bChain), held to the bone's parent (or the body above it in the chain) by the
//! limits and springs of [`super::anim_phys`], stepped once per anim update in its simulation space. The bodies live
//! across frames; the bone transforms are the bodies' poses. No pose tape covers them.
//!
//! What the node refuses where it is made (no data of the build uses it): wind, retargeting, the gravity override, an
//! external force, the cone limit, the RootRelative / BoneRelative spaces, an alpha other than a plain float.
//!
//! Units: the build's (cm, UE axes, UE quaternions), as every node here; the solver's sums in float32.
//! Unverified: the chain is walked from ChainEnd up through the parents to BoundBone (142489f90 not read line by
//! line); the bodies' "BodiesToReset" (a LOD change) and the teleport reset are not ported (the view has no LOD and
//! no teleport request).

use mvs_data::value::{Props, Value};

use super::anim_phys::{self, Body, F3, FQ, Limits};
use super::{
    AnimNode, AnimNodeSkel, BoneXfs, CsEval, UeXf, bone_of, check_alpha_defaults, clamp01, num, qmul, qrot, text,
    truthy,
};

const TY: &str = "AnimNode_AnimDynamics";

/// UPhysicsSettings as DefaultEngine.ini sets them (InitPhysics copies them from the CDO).
const MIN_DELTA: f32 = 0.0;
const MAX_PHYSICS_DELTA: f32 = 0.033333;
/// |DefaultGravityZ|.
const GRAVITY: f32 = 980.0;

/// AnimPhysSimSpaceType: the three the data uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimSpace {
    Component,
    Actor,
    World,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Collision {
    CoM,
    CustomSphere,
    InnerSphere,
    OuterSphere,
}

#[derive(Clone, Debug)]
struct Planar {
    bone: Option<usize>,
    xf: UeXf,
}

#[derive(Clone, Debug)]
struct Spherical {
    bone: Option<usize>,
    offset: [f64; 3],
    radius: f32,
    outer: bool,
}

#[derive(Debug)]
pub struct AnimDynamicsNode {
    /// The bones of the bodies, BoundBone first (a chain: down to ChainEnd).
    pub bones: Vec<usize>,
    pub space: SimSpace,
    half_extents: F3,
    local_joint_offset: F3,
    gravity_scale: f32,
    lin_damp: Option<f32>,
    ang_damp: Option<f32>,
    angular_bias: f32,
    collision: Collision,
    sphere_radius: f32,
    k_linear: f32,
    k_angular: f32,
    linear_spring: bool,
    angular_spring: bool,
    acc_scale: F3,
    vel_scale: F3,
    acc_clamp: F3,
    iters: (u32, u32),
    /// The linear axes: Limited (true) or Free, with their range.
    lin_limited: [bool; 3],
    lin_min: F3,
    lin_max: F3,
    lin_locked: bool,
    twist_axis: usize,
    target_axis: usize,
    ang_min: F3,
    ang_max: F3,
    angular_target: F3,
    planar: Vec<Planar>,
    spherical: Vec<Spherical>,
    do_update: bool,
    do_eval: bool,
    // the node's state
    pub bodies: Vec<Body>,
    /// The joint offset of each body (JointOffsets +0x3f8).
    joint_offsets: Vec<F3>,
    limits: Limits,
    /// A full reset is pending (+0x243 = 2): the bodies are made on the next evaluation.
    reset: bool,
    next_time_step: f32,
    /// The component's world translation and velocity of the last evaluation (+0xe0, +0x434).
    prev_comp_t: [f64; 3],
    prev_vel: F3,
    actual_alpha: f64,
    out: BoneXfs,
}

fn vec3(v: Option<&Value>) -> [f64; 3] {
    let o = v.and_then(Value::as_obj);
    let c = |k: &str| o.and_then(|o| num(o, k)).unwrap_or(0.0);
    [c("X"), c("Y"), c("Z")]
}

fn f3(v: [f64; 3]) -> F3 {
    v.map(|c| c as f32)
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

fn axis_enum(ty: &str, v: Option<&str>) -> usize {
    match v.and_then(|s| s.rsplit("::").next()) {
        Some("AxisX") => 0,
        Some("AxisY") => 1,
        Some("AxisZ") | None => 2,
        Some(o) => panic!("{ty}: twist axis {o} is not known"),
    }
}

/// a * b as FTransform's operator* (a first, then b).
pub(crate) fn compose(a: &UeXf, b: &UeXf) -> UeXf {
    let t = qrot(b.q, [a.t[0] * b.s[0], a.t[1] * b.s[1], a.t[2] * b.s[2]], false);
    UeXf {
        t: [t[0] + b.t[0], t[1] + b.t[1], t[2] + b.t[2]],
        q: qmul(b.q, a.q),
        s: [a.s[0] * b.s[0], a.s[1] * b.s[1], a.s[2] * b.s[2]],
    }
}

/// x.GetRelativeTransform(base): x * base^-1 (a zero scale component gives 0).
pub(crate) fn relative(x: &UeXf, base: &UeXf) -> UeXf {
    let div = |v: f64, d: f64| if d == 0.0 { 0.0 } else { v / d };
    let r = qrot(base.q, [x.t[0] - base.t[0], x.t[1] - base.t[1], x.t[2] - base.t[2]], true);
    UeXf {
        t: [div(r[0], base.s[0]), div(r[1], base.s[1]), div(r[2], base.s[2])],
        q: qmul([-base.q[0], -base.q[1], -base.q[2], base.q[3]], x.q),
        s: [div(x.s[0], base.s[0]), div(x.s[1], base.s[1]), div(x.s[2], base.s[2])],
    }
}

impl AnimDynamicsNode {
    pub fn new(props: &Props, skel: &AnimNodeSkel<'_>) -> AnimDynamicsNode {
        check_alpha_defaults(TY, props);
        let flag = |k: &str| truthy(props.get(k));
        let f = |k: &str, d: f64| num(props, k).unwrap_or(d) as f32;
        for k in ["bEnableWind", "bUseGravityOverride"] {
            assert!(!flag(k), "{TY}: {k} is not ported");
        }
        let retarget = props.get("RetargetingSettings").and_then(Value::as_obj);
        assert!(!retarget.is_some_and(|r| truthy(r.get("bEnabled"))), "{TY}: retargeting is not ported");
        assert!(vec3(props.get("ExternalForce")) == [0.0; 3], "{TY}: an external force is not ported");
        let space = match text(props, "SimulationSpace").and_then(|s| s.rsplit("::").next()) {
            Some("Component") | None => SimSpace::Component,
            Some("Actor") => SimSpace::Actor,
            Some("World") => SimSpace::World,
            Some(o) => panic!("{TY}: simulation space {o} is not ported"),
        };
        let collision = match text(props, "CollisionType").and_then(|s| s.rsplit("::").next()) {
            Some("CoM") | None => Collision::CoM,
            Some("CustomSphere") => Collision::CustomSphere,
            Some("InnerSphere") => Collision::InnerSphere,
            Some("OuterSphere") => Collision::OuterSphere,
            Some(o) => panic!("{TY}: collision type {o} is not known"),
        };
        let cs = props.get("ConstraintSetup").and_then(Value::as_obj).cloned().unwrap_or_default();
        let ang_type = text(&cs, "AngularConstraintType").unwrap_or("AnimPhysAngularConstraintType::Angular");
        assert!(ang_type.ends_with("::Angular"), "{TY}: the cone limit is not ported");
        let lin_type = |k: &str| match text(&cs, k).and_then(|s| s.rsplit("::").next()) {
            Some("Limited") => true,
            Some("Free") | None => false,
            Some(o) => panic!("{TY}: linear limit type {o} is not known"),
        };
        let lin_limited = [lin_type("LinearXLimitType"), lin_type("LinearYLimitType"), lin_type("LinearZLimitType")];
        let (lin_min, lin_max) = (f3(vec3(cs.get("LinearAxesMin"))), f3(vec3(cs.get("LinearAxesMax"))));
        // bLinearFullyLocked: every axis limited with min == max
        let lin_locked = (0..3).all(|k| lin_limited[k] && lin_min[k] == lin_max[k]);

        let bound = bone_of(TY, skel, props.get("BoundBone"));
        let mut bones = Vec::new();
        if let Some(b) = bound {
            if flag("bChain") {
                let end = bone_of(TY, skel, props.get("ChainEnd"));
                let mut c = end;
                while let Some(i) = c {
                    bones.push(i);
                    if i == b {
                        break;
                    }
                    c = usize::try_from(skel.parents[i]).ok();
                }
                if bones.last() != Some(&b) {
                    eprintln!("warning: {TY}: ChainEnd is not below BoundBone: the node does nothing");
                    bones.clear();
                }
                bones.reverse();
            } else {
                bones.push(b);
            }
        }
        let limit_list = |k: &str| props.get(k).and_then(Value::as_arr).unwrap_or(&[]).to_vec();
        let driving = |o: &Props| {
            let name = o.get("DrivingBone").and_then(Value::as_obj).and_then(|d| text(d, "BoneName"));
            name.filter(|n| *n != "None").and_then(|n| skel.names.iter().position(|x| x == n))
        };
        let planar = if flag("bUsePlanarLimit") {
            limit_list("PlanarLimits")
                .iter()
                .filter_map(Value::as_obj)
                .map(|o| Planar { bone: driving(o), xf: xf_of(o.get("PlaneTransform")) })
                .collect()
        } else {
            Vec::new()
        };
        let spherical = if flag("bUseSphericalLimits") {
            limit_list("SphericalLimits")
                .iter()
                .filter_map(Value::as_obj)
                .map(|o| Spherical {
                    bone: driving(o),
                    offset: vec3(o.get("SphereLocalOffset")),
                    radius: num(o, "LimitRadius").unwrap_or(0.0) as f32,
                    outer: text(o, "LimitType").is_some_and(|t| t.ends_with("::Outer")),
                })
                .collect()
        } else {
            Vec::new()
        };
        let it = |k: &str, d: f64| num(props, k).unwrap_or(d).max(0.0) as u32;
        AnimDynamicsNode {
            bones,
            space,
            half_extents: f3(vec3(props.get("BoxExtents"))).map(|c| c * 0.5),
            local_joint_offset: f3(vec3(props.get("LocalJointOffset"))),
            gravity_scale: f("GravityScale", 1.0),
            lin_damp: flag("bOverrideLinearDamping").then(|| f("LinearDampingOverride", 0.0)),
            ang_damp: flag("bOverrideAngularDamping").then(|| f("AngularDampingOverride", 0.0)),
            angular_bias: if flag("bOverrideAngularBias") {
                f("AngularBiasOverride", 0.0)
            } else {
                anim_phys::JOINT_BIAS
            },
            collision,
            sphere_radius: f("SphereCollisionRadius", 0.0),
            k_linear: f("LinearSpringConstant", 0.0),
            k_angular: f("AngularSpringConstant", 0.0),
            linear_spring: flag("bLinearSpring"),
            angular_spring: flag("bAngularSpring"),
            acc_scale: f3(vec3(props.get("ComponentLinearAccScale"))),
            vel_scale: f3(vec3(props.get("ComponentLinearVelScale"))),
            acc_clamp: f3(vec3(props.get("ComponentAppliedLinearAccClamp"))),
            iters: (it("NumSolverIterationsPreUpdate", 4.0), it("NumSolverIterationsPostUpdate", 1.0)),
            lin_limited,
            lin_min,
            lin_max,
            lin_locked,
            twist_axis: axis_enum(TY, text(&cs, "TwistAxis")),
            target_axis: axis_enum(TY, text(&cs, "AngularTargetAxis")),
            ang_min: f3(vec3(cs.get("AngularLimitsMin"))),
            ang_max: f3(vec3(cs.get("AngularLimitsMax"))),
            angular_target: f3(vec3(cs.get("AngularTarget"))),
            planar,
            spherical,
            do_update: props.get("bDoUpdate").is_none_or(|v| truthy(Some(v))),
            do_eval: props.get("bDoEval").is_none_or(|v| truthy(Some(v))),
            bodies: Vec::new(),
            joint_offsets: Vec::new(),
            limits: Limits::default(),
            reset: false,
            next_time_step: 0.0,
            prev_comp_t: [0.0; 3],
            prev_vel: [0.0; 3],
            actual_alpha: 0.0,
            out: BoneXfs::default(),
        }
    }

    /// The simulation space's transform in the component's (14247c970 / 14247a810): Component identity, Actor the
    /// actor's transform relative to the component, World the component's world transform taken whole.
    fn space_xf(&self, comp: &UeXf, actor: &UeXf) -> UeXf {
        match self.space {
            SimSpace::Component => UeXf::default(),
            SimSpace::Actor => relative(actor, comp),
            SimSpace::World => UeXf::default(),
        }
    }

    /// A component-space transform taken into simulation space.
    fn to_sim(&self, x: &UeXf, comp: &UeXf, actor: &UeXf) -> UeXf {
        match self.space {
            SimSpace::Component => *x,
            SimSpace::Actor => relative(x, &self.space_xf(comp, actor)),
            SimSpace::World => compose(x, comp),
        }
    }

    /// A simulation-space transform taken back into component space.
    fn sim_to_comp(&self, x: &UeXf, comp: &UeXf, actor: &UeXf) -> UeXf {
        match self.space {
            SimSpace::Component => *x,
            SimSpace::Actor => compose(x, &self.space_xf(comp, actor)),
            SimSpace::World => relative(x, comp),
        }
    }

    /// A world vector taken into simulation space (TransformWorldVectorToSimSpace 14248ea30: the inverse rotation of
    /// the component or the actor, no scale; World as it is).
    fn world_vec_to_sim(&self, v: [f64; 3], comp: &UeXf, actor: &UeXf) -> F3 {
        f3(match self.space {
            SimSpace::Component => qrot(comp.q, v, true),
            SimSpace::Actor => qrot(actor.q, v, true),
            SimSpace::World => v,
        })
    }

    fn bone_sim(&self, cs: &mut CsEval<'_>, b: usize, comp: &UeXf, actor: &UeXf) -> UeXf {
        let x = cs.get(b);
        self.to_sim(&x, comp, actor)
    }

    /// InitPhysics 14247dca0 (a full reset): one body per bone, BoundBone first, each at its bone's pose less the
    /// joint offset; a body after the first is based on the one before it, its joint offset the LocalJointOffset's
    /// direction times half the distance between the two bones (the offset itself when it is a unit vector; with a
    /// zero offset, the previous body's place less this bone's, as read).
    fn init_physics(&mut self, cs: &mut CsEval<'_>, comp: &UeXf, actor: &UeXf) {
        self.bodies.clear();
        self.joint_offsets.clear();
        let off = self.local_joint_offset;
        let neg = off.map(|c| -c);
        let radius = {
            let e = self.half_extents.map(|c| (c * 2.0).abs());
            match self.collision {
                Collision::CoM => None,
                Collision::CustomSphere => Some(self.sphere_radius),
                Collision::InnerSphere => Some(e[0].min(e[1]).min(e[2]) * 0.5),
                Collision::OuterSphere => Some(e[0].max(e[1]).max(e[2]) * 0.5),
            }
        };
        let bones = self.bones.clone();
        for (i, &b) in bones.iter().enumerate() {
            let x = self.bone_sim(cs, b, comp, actor);
            let (q, t) = (f3_q(x.q), f3(x.t));
            if i == 0 {
                self.joint_offsets.push(off);
            } else {
                let p = self.bone_sim(cs, bones[i - 1], comp, actor);
                let (pq, pt, ps) = (f3_q(p.q), f3(p.t), f3(p.s));
                let prev_body =
                    anim_phys::add(anim_phys::qrot(pq, [neg[0] * ps[0], neg[1] * ps[1], neg[2] * ps[2]]), pt);
                let d = anim_phys::sub(pt, t);
                let half = anim_phys::dot(d, d).sqrt() * 0.5;
                let s = anim_phys::dot(off, off);
                let jo = if s < 1e-8 {
                    anim_phys::sub(prev_body, t)
                } else if s == 1.0 {
                    anim_phys::scale(off, half)
                } else {
                    anim_phys::scale(anim_phys::scale(off, mvs_math::f32::estimate::reciprocal_sqrt_accurate(s)), half)
                };
                self.joint_offsets.push(jo);
            }
            let pos = anim_phys::add(anim_phys::qrot(q, neg), t);
            let mut body = Body::new(self.half_extents, pos, q);
            body.lin_damp = self.lin_damp;
            body.ang_damp = self.ang_damp;
            body.gravity_scale = self.gravity_scale;
            if let Some(r) = radius {
                body.collides = true;
                body.radius = r;
            }
            self.bodies.push(body);
        }
    }

    /// UpdateLimits 142491730: every body's limits and springs anew for a step of dt.
    fn update_limits(&mut self, cs: &mut CsEval<'_>, comp: &UeXf, actor: &UeXf, dt: f32) {
        self.limits.clear();
        let bones = self.bones.clone();
        for (i, &b) in bones.iter().enumerate() {
            let prev = i.checked_sub(1);
            let (shape_q, shape_t, body1_off) = match prev {
                None => {
                    let x = self.bone_sim(cs, b, comp, actor);
                    (f3_q(x.q), f3(x.t), self.local_joint_offset)
                }
                Some(_) => {
                    let jo = self.joint_offsets[i];
                    ([0.0, 0.0, 0.0, 1.0], jo.map(|c| -c), jo)
                }
            };
            let bodies = &self.bodies;
            let lim = &mut self.limits;
            if self.lin_locked {
                lim.nailed(bodies, dt, prev, shape_t, Some(i), body1_off);
            } else {
                for k in 0..3 {
                    if self.lin_limited[k] {
                        let axis = anim_phys::axis_of(shape_q, k);
                        let range = (self.lin_min[k], self.lin_max[k]);
                        let m = anim_phys::MAX;
                        lim.along_direction(bodies, dt, prev, shape_t, Some(i), body1_off, axis, range, -m, m);
                    }
                }
            }
            let (amin, amax, bias, ta) = (self.ang_min, self.ang_max, self.angular_bias, self.twist_axis);
            lim.angular_range(bodies, dt, prev, Some(i), shape_q, ta, amin, amax, bias);
            for p in &self.planar {
                let x = match p.bone {
                    Some(d) => {
                        let dx = cs.get(d);
                        compose(&p.xf, &self.to_sim(&dx, comp, actor))
                    }
                    None => p.xf,
                };
                self.limits.planar(&self.bodies, dt, i, f3_q(x.q), f3(x.t));
            }
            for s in &self.spherical {
                let local = UeXf { t: s.offset, ..UeXf::default() };
                let x = match s.bone {
                    Some(d) => {
                        let dx = cs.get(d);
                        compose(&local, &self.to_sim(&dx, comp, actor))
                    }
                    None => local,
                };
                self.limits.spherical(&self.bodies, dt, i, f3(x.t), s.radius, s.outer);
            }
            if self.linear_spring || self.angular_spring {
                self.limits.spring(
                    prev,
                    shape_t,
                    Some(i),
                    (self.k_linear, self.k_angular),
                    self.angular_target,
                    self.target_axis,
                    shape_q,
                    (self.linear_spring, self.angular_spring),
                );
            }
        }
    }
}

fn f3_q(q: [f64; 4]) -> FQ {
    q.map(|c| c as f32)
}

/// The node's evaluation needs the actor's world transform too (the Actor space): [`AnimDynamicsNode::set_actor`]
/// before each evaluation; a node in another space ignores it.
impl AnimDynamicsNode {
    pub fn evaluate_with(&mut self, cs: &mut CsEval<'_>, comp: &UeXf, actor: &UeXf) {
        if self.bones.is_empty() || !(self.actual_alpha > 1e-5) {
            return;
        }
        if self.reset {
            self.init_physics(cs, comp, actor);
            self.prev_comp_t = comp.t;
            self.prev_vel = [0.0; 3];
            self.reset = false;
        }
        let dt_frame = self.next_time_step;
        if self.do_update && MIN_DELTA < dt_frame {
            let gravity = anim_phys::scale(self.world_vec_to_sim([0.0, 0.0, -1.0], comp, actor), GRAVITY);
            let mut accel = [0.0f32; 3];
            if self.space != SimSpace::World {
                // Unverified: the exe multiplies by 1 / dt in f32 where this divides in f64 (a rounding-level difference).
                let inv = 1.0 / f64::from(dt_frame);
                let vel = [0, 1, 2].map(|k| ((comp.t[k] - self.prev_comp_t[k]) * inv) as f32);
                let a = self.world_vec_to_sim(vel.map(|c| -f64::from(c)), comp, actor);
                let dv = [0, 1, 2].map(|k| f64::from((self.prev_vel[k] - vel[k]) / dt_frame));
                let b = self.world_vec_to_sim(dv, comp, actor);
                for k in 0..3 {
                    let v = a[k] * self.vel_scale[k] + b[k] * self.acc_scale[k];
                    accel[k] = v.clamp(-self.acc_clamp[k], self.acc_clamp[k]);
                }
                self.prev_vel = vel;
            }
            let dt = dt_frame.min(MAX_PHYSICS_DELTA);
            self.update_limits(cs, comp, actor, dt);
            anim_phys::physics_update(&mut self.bodies, &mut self.limits, dt, gravity, accel, self.iters);
        }
        if self.do_eval {
            self.out.clear();
            for (i, &b) in self.bones.iter().enumerate() {
                let body = &self.bodies[i];
                let t = anim_phys::add(body.pos, anim_phys::qrot(body.q, self.joint_offsets[i]));
                let x = UeXf { t: t.map(f64::from), q: body.q.map(f64::from), s: [1.0; 3] };
                let c = self.sim_to_comp(&x, comp, actor);
                self.out.push(b, c);
            }
            self.out.sort();
            cs.local_blend(&self.out, self.actual_alpha);
        }
        self.prev_comp_t = comp.t;
    }
}

impl AnimNode for AnimDynamicsNode {
    fn ty(&self) -> &'static str {
        TY
    }

    /// Initialize 142483160: a full reset when the bone is valid; NextTimeStep 0.
    fn initialize(&mut self) {
        self.reset = !self.bones.is_empty();
        self.next_time_step = 0.0;
    }

    /// UpdateInternal 142491510 under the base's gate: NextTimeStep = DeltaTime (not accumulated).
    fn update(&mut self, dt: f64, alpha: f64) {
        self.actual_alpha = clamp01(alpha);
        if !(self.actual_alpha > 1e-5) || self.bones.is_empty() {
            return;
        }
        self.next_time_step = dt as f32;
    }

    /// Evaluate with the component at scale 1 as the actor's transform (a caller that holds the actor's own gives it
    /// through [`AnimDynamicsNode::evaluate_with`]).
    /// Unverified: the actor's transform of a fighter is taken as its mesh component's without the scale (the mesh's
    /// relative rotation and offset are not read), which only an Actor-space node sees.
    fn evaluate(&mut self, cs: &mut CsEval<'_>, comp: &UeXf) {
        let actor = UeXf { s: [1.0; 3], ..*comp };
        self.evaluate_with(cs, comp, &actor);
    }
}
