//! FAnimPhys, the rigid-body solver under FAnimNode_AnimDynamics (stock 4.26 AnimPhysicsSolver.cpp, read in the exe):
//! box bodies, the linear / angular limits and springs between them, and the step (PhysicsUpdate 142875f70). Float32
//! as the exe keeps it; the build's units and axes (cm, UE X / Y / Z, UE quaternions).
//!
//! Matrices are the exe's FMatrix rows (row vectors: v x M); a body's world inverse tensor is R (InvMass InvTensor)
//! R^T with R the rotation matrix of its orientation (InitializeBodyVelocity 1428744d0, UpdatePose 142879620), and
//! the angular velocity is AngularMomentum x that matrix (the limit iterations 142875180 / 142874f20).
//!
//! Unverified: the box's inertia is the closed form of a uniform box (per unit mass, half extents h: (hy^2 + hz^2) / 3
//! ...), where the exe sums the hull's tetrahedra (MakeBox 142875630, the body ctor 14286a270): the same value up to
//! float rounding. The orientation's normalise falls back to the identity below 1e-8 (the masked constant of
//! 14286d326 not read).

use mvs_math::f32::crt::{acosf, powf, sinf};
use mvs_math::f32::estimate::reciprocal_sqrt_accurate;

pub type F3 = [f32; 3];
pub type FQ = [f32; 4];
/// An FMatrix's upper 3 x 3, by rows.
pub type M3 = [F3; 3];

pub const MAX: f32 = f32::MAX;
const KINDA_SMALL: f32 = 1e-8;
/// AnimPhysicsConstants::JointBiasFactor (0.3 at 143c17324).
pub const JOINT_BIAS: f32 = 0.3;

// ---- vector and quaternion math (float32) ----

pub fn add(a: F3, b: F3) -> F3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: F3, b: F3) -> F3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: F3, s: f32) -> F3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub fn dot(a: F3, b: F3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: F3, b: F3) -> F3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn neg(a: F3) -> F3 {
    [-a[0], -a[1], -a[2]]
}

/// FVector::GetSafeNormal(1e-8): |v|^2 == 1 unchanged, below the tolerance zero, else v x InvSqrt (rsqrtss and two
/// Newton steps).
pub fn safe_normal(v: F3) -> F3 {
    let s = dot(v, v);
    if s == 1.0 {
        v
    } else if s < KINDA_SMALL {
        [0.0; 3]
    } else {
        scale(v, reciprocal_sqrt_accurate(s))
    }
}

/// a x b (FQuat operator*: b first, then a).
pub fn qmul(a: FQ, b: FQ) -> FQ {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

pub fn conj(q: FQ) -> FQ {
    [-q[0], -q[1], -q[2], q[3]]
}

/// FQuat::RotateVector (4.26): T = 2 (q x v); v + w T + q x T.
pub fn qrot(q: FQ, v: F3) -> F3 {
    let qv = [q[0], q[1], q[2]];
    let t = scale(cross(qv, v), 2.0);
    add(add(v, scale(t, q[3])), cross(qv, t))
}

/// FQuat::UnrotateVector.
pub fn qunrot(q: FQ, v: F3) -> F3 {
    qrot(conj(q), v)
}

/// The exe's vector normalise of a quaternion (rsqrtps and two Newton steps), the identity below 1e-8.
pub fn qnorm(q: FQ) -> FQ {
    let s = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if s >= KINDA_SMALL {
        let k = reciprocal_sqrt_accurate(s);
        q.map(|c| c * k)
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

/// FQuat::FindBetweenVectors 14135d1d0 (vectors of any length): NormAB = sqrt(|a|^2 |b|^2), w = NormAB + a . b; the
/// cross product when w >= 1e-6 NormAB, else the perpendicular of the larger axis; normalised.
pub fn find_between_vectors(a: F3, b: F3) -> FQ {
    let norm_ab = (dot(a, a) * dot(b, b)).sqrt();
    let mut w = norm_ab + dot(a, b);
    let q = if w >= 1e-6 * norm_ab {
        let c = cross(a, b);
        [c[0], c[1], c[2], w]
    } else {
        w = 0.0;
        if a[0].abs() > a[1].abs() { [-a[2], 0.0, a[0], w] } else { [0.0, -a[2], a[1], w] }
    };
    qnorm(q)
}

/// The unit axis k (0 X, 1 Y, 2 Z) of a rotation.
pub fn axis_of(q: FQ, k: usize) -> F3 {
    let mut v = [0.0; 3];
    v[k] = 1.0;
    qrot(q, v)
}

/// FQuatRotationMatrix's rows (141362dc0): row k = the image of axis k.
pub fn quat_mat(q: FQ) -> M3 {
    let [x, y, z, w] = q;
    let (x2, y2, z2) = (x + x, y + y, z + z);
    let (xx, xy, xz) = (x * x2, x * y2, x * z2);
    let (yy, yz, zz) = (y * y2, y * z2, z * z2);
    let (wx, wy, wz) = (w * x2, w * y2, w * z2);
    [[1.0 - (yy + zz), xy + wz, xz - wy], [xy - wz, 1.0 - (xx + zz), yz + wx], [xz + wy, yz - wx, 1.0 - (xx + yy)]]
}

/// v x M (row vector), in the exe's order of sums: (z m2 + (y m1 + x m0)).
pub fn vmul(v: F3, m: &M3) -> F3 {
    let r = |k: usize| v[2] * m[2][k] + (v[1] * m[1][k] + v[0] * m[0][k]);
    [r(0), r(1), r(2)]
}

/// a x b: row i = a[i] x b, as ((a_i0 b_0 + a_i1 b_1) + a_i2 b_2).
pub fn mat_mul(a: &M3, b: &M3) -> M3 {
    let row = |r: F3| [0, 1, 2].map(|k| (r[0] * b[0][k] + r[1] * b[1][k]) + r[2] * b[2][k]);
    [row(a[0]), row(a[1]), row(a[2])]
}

pub fn transpose(m: &M3) -> M3 {
    [[m[0][0], m[1][0], m[2][0]], [m[0][1], m[1][1], m[2][1]], [m[0][2], m[1][2], m[2][2]]]
}

/// R (s M) R^T with R = the rotation matrix of q.
fn world_tensor(q: FQ, s: f32, m: &M3) -> M3 {
    let r = quat_mat(q);
    let sm = m.map(|row| row.map(|c| c * s));
    mat_mul(&mat_mul(&r, &sm), &transpose(&r))
}

// ---- the rigid body ----

/// FAnimPhysRigidBody: a box of mass 1 (Mass = InvMass = 1, the ctor 14286a270).
#[derive(Clone, Debug)]
pub struct Body {
    pub pos: F3,
    pub q: FQ,
    pub lin: F3,
    pub ang: F3,
    pub mass: f32,
    pub inv_mass: f32,
    /// InverseTensorWithoutMass.
    pub inv_tensor: M3,
    /// InverseWorldSpaceTensor.
    pub inv_world: M3,
    pub next_pos: F3,
    pub next_q: FQ,
    pub prev_pos: F3,
    pub prev_q: FQ,
    /// The damping overrides (bLinearDampingOverriden / bAngularDampingOverriden), else 0.7 (143cd70f0).
    pub lin_damp: Option<f32>,
    pub ang_damp: Option<f32>,
    pub gravity_scale: f32,
    /// A CollisionType other than CoM, with its radius.
    pub collides: bool,
    pub radius: f32,
}

impl Body {
    /// A box of half extents `h` (MakeBox halves BoxExtents) at `pos`, oriented `q`.
    pub fn new(h: F3, pos: F3, q: FQ) -> Body {
        let (x, y, z) = (h[0] * h[0], h[1] * h[1], h[2] * h[2]);
        let inv = |a: f32| 1.0 / (a / 3.0);
        let it = [[inv(y + z), 0.0, 0.0], [0.0, inv(x + z), 0.0], [0.0, 0.0, inv(x + y)]];
        Body {
            pos,
            q,
            lin: [0.0; 3],
            ang: [0.0; 3],
            mass: 1.0,
            inv_mass: 1.0,
            inv_tensor: it,
            inv_world: world_tensor(q, 1.0, &it),
            next_pos: pos,
            next_q: q,
            prev_pos: pos,
            prev_q: q,
            lin_damp: None,
            ang_damp: None,
            gravity_scale: 1.0,
            collides: false,
            radius: 0.0,
        }
    }

    /// The body's pose x a local point (14286b020).
    pub fn point(&self, p: F3) -> F3 {
        add(qrot(self.q, p), self.pos)
    }

    /// The velocity of a point at lever arm r: LinMom InvMass + (AngMom x I_w) x r.
    fn velocity_at(&self, r: F3) -> F3 {
        add(cross(vmul(self.ang, &self.inv_world), r), scale(self.lin, self.inv_mass))
    }

    /// InitializeBodyVelocity 1428744d0.
    fn init_velocity(&mut self, dt: f32, gravity: F3) {
        self.prev_pos = self.pos;
        self.prev_q = self.q;
        let ld = powf(1.0 - self.lin_damp.unwrap_or(0.7), dt);
        let ad = powf(1.0 - self.ang_damp.unwrap_or(0.7), dt);
        self.lin = scale(self.lin, ld);
        self.ang = scale(self.ang, ad);
        let g = scale(gravity, self.mass * self.gravity_scale);
        self.lin = add(self.lin, scale(g, dt));
        self.inv_world = world_tensor(self.q, self.inv_mass, &self.inv_tensor);
    }

    /// The orientation's derivative 14286f9f0: 0.5 (w, 0) x q, w = L x (R M R^T) of the normalised q.
    fn spin(q: FQ, m: &M3, l: F3) -> FQ {
        let qn = qnorm(q);
        let w = vmul(l, &world_tensor(qn, 1.0, m));
        qmul([w[0] * 0.5, w[1] * 0.5, w[2] * 0.5, 0.0], qn)
    }

    /// CalculateNextPose 14286d0f0: the position by the momentum, the orientation by RK4.
    fn next_pose(&mut self, dt: f32) {
        self.next_pos = add(scale(scale(self.lin, self.inv_mass), dt), self.pos);
        let m = self.inv_tensor.map(|r| r.map(|c| c * self.inv_mass));
        let q = self.q;
        let step = |k: FQ, s: f32| [0, 1, 2, 3].map(|i| s * k[i] + q[i]);
        let h = dt * 0.5;
        let k1 = Body::spin(q, &m, self.ang);
        let k2 = Body::spin(step(k1, h), &m, self.ang);
        let k3 = Body::spin(step(k2, h), &m, self.ang);
        let k4 = Body::spin(step(k3, dt), &m, self.ang);
        let (a, b) = (dt * 0.16666667, dt * 0.33333334);
        let r = [0, 1, 2, 3].map(|i| a * k1[i] + q[i] + b * k2[i] + b * k3[i] + a * k4[i]);
        self.next_q = qnorm(r);
    }

    /// UpdatePose 142879620.
    fn update_pose(&mut self) {
        self.prev_pos = self.pos;
        self.prev_q = self.q;
        self.pos = self.next_pos;
        self.q = self.next_q;
        self.inv_world = world_tensor(self.q, self.inv_mass, &self.inv_tensor);
    }
}

// ---- limits and springs ----

/// FAnimPhysLinearLimit (0x68 bytes): bodies by index into the solver's list.
#[derive(Clone, Debug)]
pub struct LinearLimit {
    pub b0: Option<usize>,
    pub b1: Option<usize>,
    pub first: F3,
    pub second: F3,
    pub n: F3,
    pub ts: f32,
    pub ts_unbiased: f32,
    pub min: f32,
    pub max: f32,
    pub sum: f32,
    pub eff: f32,
    r0: F3,
    r1: F3,
}

/// FAnimPhysAngularLimit (0x30 bytes).
#[derive(Clone, Debug)]
pub struct AngularLimit {
    pub b0: Option<usize>,
    pub b1: Option<usize>,
    pub axis: F3,
    pub sum: f32,
    pub ts: f32,
    pub min: f32,
    pub max: f32,
    pub eff: f32,
}

/// FAnimPhysSpring (0x60 bytes).
#[derive(Clone, Debug)]
pub struct Spring {
    pub b0: Option<usize>,
    pub b1: Option<usize>,
    pub target_offset: FQ,
    pub angular_target: F3,
    pub angular_axis: usize,
    pub anchor0: F3,
    pub anchor1: F3,
    pub k_linear: f32,
    pub k_angular: f32,
    pub linear: bool,
    pub angular: bool,
}

/// The limits and springs of one step (UpdateLimits rebuilds them each step).
#[derive(Clone, Debug, Default)]
pub struct Limits {
    pub linear: Vec<LinearLimit>,
    pub angular: Vec<AngularLimit>,
    pub springs: Vec<Spring>,
}

fn opt<T>(bodies: &[Body], i: Option<usize>, f: impl Fn(&Body) -> T, d: T) -> T {
    i.map_or(d, |i| f(&bodies[i]))
}

impl Limits {
    pub fn clear(&mut self) {
        self.linear.clear();
        self.angular.clear();
        self.springs.clear();
    }

    /// A linear limit, its effective mass worked out (142879060).
    #[allow(clippy::too_many_arguments)]
    fn add_linear(
        &mut self,
        bodies: &[Body],
        b0: Option<usize>,
        first: F3,
        b1: Option<usize>,
        second: F3,
        n: F3,
        ts: f32,
        ts_unbiased: f32,
        min: f32,
        max: f32,
    ) {
        let r0 = opt(bodies, b0, |b| qrot(b.q, first), first);
        let r1 = opt(bodies, b1, |b| qrot(b.q, second), second);
        let term = |b: &Body, r: F3| dot(n, cross(vmul(cross(r, n), &b.inv_world), r)) + b.inv_mass;
        let s = opt(bodies, b0, |b| term(b, r0), 0.0) + opt(bodies, b1, |b| term(b, r1), 0.0);
        self.linear.push(LinearLimit {
            b0,
            b1,
            first,
            second,
            n,
            ts,
            ts_unbiased,
            min,
            max,
            sum: 0.0,
            eff: 1.0 / s,
            r0,
            r1,
        });
    }

    /// An angular limit, its effective mass worked out (142878ea0).
    #[allow(clippy::too_many_arguments)]
    fn add_angular(
        &mut self,
        bodies: &[Body],
        b0: Option<usize>,
        b1: Option<usize>,
        axis: F3,
        ts: f32,
        min: f32,
        max: f32,
    ) {
        let term = |b: &Body| dot(axis, vmul(axis, &b.inv_world));
        let s = opt(bodies, b0, term, 0.0) + opt(bodies, b1, term, 0.0);
        self.angular.push(AngularLimit { b0, b1, axis, sum: 0.0, ts, min, max, eff: 1.0 / s });
    }

    /// ConstrainAlongDirection 14286d480: along `axis`, the distance between the two points held in [lim.0, lim.1];
    /// min == max (within 1e-8) gives one limit of force range [min_f, max_f], else two.
    #[allow(clippy::too_many_arguments)]
    pub fn along_direction(
        &mut self,
        bodies: &[Body],
        dt: f32,
        b0: Option<usize>,
        p0: F3,
        b1: Option<usize>,
        p1: F3,
        axis: F3,
        lim: (f32, f32),
        min_f: f32,
        max_f: f32,
    ) {
        let w0 = opt(bodies, b0, |b| b.point(p0), p0);
        let w1 = opt(bodies, b1, |b| b.point(p1), p1);
        let d = dot(sub(w1, w0), axis);
        let (x, y) = lim;
        if (x - y).abs() < KINDA_SMALL {
            let ts = d / dt;
            self.add_linear(bodies, b0, p0, b1, p1, axis, ts, ts, min_f.min(max_f), min_f.max(max_f));
        } else {
            let ts = (d - x) / dt;
            self.add_linear(bodies, b0, p0, b1, p1, axis, ts, ts, max_f.min(0.0), max_f.max(0.0));
            let ts = (d - y) / dt;
            self.add_linear(bodies, b0, p0, b1, p1, axis, ts, ts, min_f.min(0.0), min_f.max(0.0));
        }
    }

    /// ConstrainPositionNailed 14286eed0: three limits along X, Y, Z, each of target speed the gap / dt, unbiased 0,
    /// range +-FLT_MAX.
    pub fn nailed(&mut self, bodies: &[Body], dt: f32, b0: Option<usize>, p0: F3, b1: Option<usize>, p1: F3) {
        let w0 = opt(bodies, b0, |b| b.point(p0), p0);
        let w1 = opt(bodies, b1, |b| b.point(p1), p1);
        let inv = 1.0 / dt;
        let d = sub(w1, w0);
        for k in 0..3 {
            let mut n = [0.0; 3];
            n[k] = 1.0;
            self.add_linear(bodies, b0, p0, b1, p1, n, d[k] * inv, 0.0, -MAX, MAX);
        }
    }

    /// ConstrainPlanar 14286eba0: the body (less its collision radius) kept on the plane's +Z side.
    pub fn planar(&mut self, bodies: &[Body], dt: f32, b: usize, plane_q: FQ, plane_t: F3) {
        let body = &bodies[b];
        let n = safe_normal(qrot(plane_q, [0.0, 0.0, 1.0]));
        let mut dist = dot(n, body.pos) - dot(n, plane_t);
        if body.collides {
            dist -= body.radius;
        }
        let ts = dist / dt;
        self.add_linear(bodies, None, plane_t, Some(b), [0.0; 3], n, ts, ts, 0.0, MAX);
    }

    /// ConstrainSphericalInner 14286f1f0 / Outer 14286f460: the body kept inside / outside a sphere.
    pub fn spherical(&mut self, bodies: &[Body], dt: f32, b: usize, center: F3, radius: f32, outer: bool) {
        let body = &bodies[b];
        let d = sub(body.pos, center);
        let n = safe_normal(d);
        let mut dist = dot(d, d).sqrt() - radius;
        if body.collides {
            dist = if outer { dist - body.radius } else { dist + body.radius };
        }
        let ts = dist / dt;
        let (min, max) = if outer { (0.0, MAX) } else { (-MAX, 0.0) };
        self.add_linear(bodies, None, center, Some(b), [0.0; 3], n, ts, ts, min, max);
    }

    /// ConstrainAngularRange 14286d7d0 / 14286d8d0: the swing of body 1 relative to (body 0 x) the joint frame
    /// limited per axis in [min, max] (degrees), the twist about the twist axis locked.
    #[allow(clippy::too_many_arguments)]
    pub fn angular_range(
        &mut self,
        bodies: &[Body],
        dt: f32,
        b0: Option<usize>,
        b1: Option<usize>,
        frame: FQ,
        twist_axis: usize,
        min: F3,
        max: F3,
        bias: f32,
    ) {
        let q0 = opt(bodies, b0, |b| qmul(b.q, frame), frame);
        let q1 = opt(bodies, b1, |b| b.q, [0.0, 0.0, 0.0, 1.0]);
        let delta = qmul(conj(q0), q1);
        let mut ta = [0.0; 3];
        ta[twist_axis] = 1.0;
        let mut swing = find_between_vectors(ta, qrot(delta, ta));
        let mut twist = qmul(conj(swing), delta);
        if swing[3] < 0.0 {
            swing = swing.map(|c| -c);
        }
        if twist[3] < 0.0 {
            twist = twist.map(|c| -c);
        }
        let axes = [axis_of(q1, 0), axis_of(q1, 1), axis_of(q1, 2)];
        let rad = |d: f32| d * 0.017_453_292;
        let (a, b) = match twist_axis {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        for (slot, k) in [(0, a), (1, b)] {
            let (l, u, s) = (rad(min[k]), rad(max[k]), swing[k]);
            if l == u {
                let ts = if slot == 0 { (2.0 * sinf(l * 0.5) - 2.0 * s) / dt } else { (l - s) * (2.0 * bias) / dt };
                self.add_angular(bodies, b0, b1, axes[k], ts, -MAX, MAX);
            } else if u - l < 2.0 * std::f32::consts::PI {
                let ts = (2.0 * sinf(l * 0.5) - 2.0 * s) / dt;
                self.add_angular(bodies, b0, b1, axes[k], ts, 0.0, MAX);
                let ts = 2.0 * (s - sinf(u * 0.5)) / dt;
                self.add_angular(bodies, b0, b1, neg(axes[k]), ts, 0.0, MAX);
            }
        }
        let ts = -2.0 * bias * twist[twist_axis] / dt;
        self.add_angular(bodies, b0, b1, axes[twist_axis], ts, -MAX, MAX);
    }

    /// CreateSpring 14286f920 with the fields UpdateLimits sets after it.
    #[allow(clippy::too_many_arguments)]
    pub fn spring(
        &mut self,
        b0: Option<usize>,
        anchor0: F3,
        b1: Option<usize>,
        k: (f32, f32),
        target: F3,
        axis: usize,
        offset: FQ,
        flags: (bool, bool),
    ) {
        self.springs.push(Spring {
            b0,
            b1,
            target_offset: offset,
            angular_target: safe_normal(target),
            angular_axis: axis,
            anchor0,
            anchor1: [0.0; 3],
            k_linear: k.0,
            k_angular: k.1,
            linear: flags.0,
            angular: flags.1,
        });
    }
}

fn apply_spring(s: &Spring, bodies: &mut [Body], dt: f32) {
    if s.linear {
        let w0 = opt(bodies, s.b0, |b| b.point(s.anchor0), s.anchor0);
        let w1 = opt(bodies, s.b1, |b| b.point(s.anchor1), s.anchor1);
        let r0 = opt(bodies, s.b0, |b| qrot(b.q, s.anchor0), s.anchor0);
        let r1 = opt(bodies, s.b1, |b| qrot(b.q, s.anchor1), s.anchor1);
        let d = sub(w1, w0);
        let f = -(dot(d, d).sqrt() * s.k_linear * dt);
        let n = safe_normal(d);
        let j = scale(n, f);
        if let Some(i) = s.b0 {
            let b = &mut bodies[i];
            b.lin = add(b.lin, neg(j));
            b.ang = add(b.ang, cross(r0, neg(j)));
        }
        if let Some(i) = s.b1 {
            let b = &mut bodies[i];
            b.lin = add(b.lin, j);
            b.ang = add(b.ang, cross(r1, j));
        }
    }
    if s.angular {
        let Some(i1) = s.b1 else { return };
        let axis1 = axis_of(bodies[i1].q, s.angular_axis);
        let target = qrot(s.target_offset, s.angular_target);
        let q = find_between_vectors(axis1, target);
        let w = q[3].clamp(-1.0, 1.0);
        let axis = {
            let v = [q[0], q[1], q[2]];
            if dot(v, v) >= KINDA_SMALL { scale(v, reciprocal_sqrt_accurate(dot(v, v))) } else { [1.0, 0.0, 0.0] }
        };
        let k = 2.0 * acosf(w) * s.k_angular;
        let imp = [axis[0] * k * dt, axis[1] * k * dt, axis[2] * k * dt];
        if let Some(i) = s.b0 {
            bodies[i].ang = sub(bodies[i].ang, imp);
        }
        bodies[i1].ang = add(bodies[i1].ang, imp);
    }
}

fn iterate_linear(l: &mut LinearLimit, bodies: &mut [Body], dt: f32) {
    let v0 = opt(bodies, l.b0, |b| b.velocity_at(l.r0), [0.0; 3]);
    let v1 = opt(bodies, l.b1, |b| b.velocity_at(l.r1), [0.0; 3]);
    let rel = dot(sub(v1, v0), l.n);
    let mut lambda = (-l.ts - rel) * l.eff;
    lambda = lambda.min(dt * l.max - l.sum);
    lambda = lambda.max(dt * l.min - l.sum);
    let j = scale(l.n, lambda);
    if let Some(i) = l.b0 {
        let b = &mut bodies[i];
        b.lin = sub(b.lin, j);
        b.ang = add(b.ang, cross(l.r0, neg(j)));
    }
    if let Some(i) = l.b1 {
        let b = &mut bodies[i];
        b.lin = add(b.lin, j);
        b.ang = add(b.ang, cross(l.r1, j));
    }
    l.sum += lambda;
}

fn iterate_angular(l: &mut AngularLimit, bodies: &mut [Body], dt: f32) {
    if l.ts == -MAX {
        return;
    }
    let w = |i: Option<usize>| opt(bodies, i, |b| dot(vmul(b.ang, &b.inv_world), l.axis), 0.0);
    let rel = w(l.b1) - w(l.b0);
    let mut lambda = (l.ts - rel) * l.eff;
    lambda = lambda.min(dt * l.max - l.sum);
    lambda = lambda.max(dt * l.min - l.sum);
    let j = scale(l.axis, lambda);
    if let Some(i) = l.b0 {
        bodies[i].ang = sub(bodies[i].ang, j);
    }
    if let Some(i) = l.b1 {
        bodies[i].ang = add(bodies[i].ang, j);
    }
    l.sum += lambda;
}

/// PhysicsUpdate 142875f70: one step of dt. `gravity` is the gravity direction x |DefaultGravityZ|; `accel` the
/// component's linear acceleration in sim space.
pub fn physics_update(bodies: &mut [Body], lim: &mut Limits, dt: f32, gravity: F3, accel: F3, iters: (u32, u32)) {
    for b in bodies.iter_mut() {
        b.init_velocity(dt, gravity);
    }
    if accel.iter().any(|c| c.abs() > 1e-4) {
        for b in bodies.iter_mut().filter(|b| b.inv_mass > 1e-4) {
            b.lin = add(b.lin, scale(scale(accel, 1.0 / b.inv_mass), dt));
        }
    }
    for s in &lim.springs {
        apply_spring(s, bodies, dt);
    }
    let solve = |lim: &mut Limits, bodies: &mut [Body], n: u32| {
        for _ in 0..n {
            for l in &mut lim.linear {
                iterate_linear(l, bodies, dt);
            }
            for l in &mut lim.angular {
                iterate_angular(l, bodies, dt);
            }
        }
    };
    solve(lim, bodies, iters.0);
    for b in bodies.iter_mut() {
        b.next_pose(dt);
    }
    for l in &mut lim.linear {
        l.ts = l.ts_unbiased;
    }
    for l in &mut lim.angular {
        l.ts = if l.min < 0.0 { 0.0 } else { l.ts.min(0.0) };
    }
    solve(lim, bodies, iters.1);
    for b in bodies.iter_mut() {
        b.update_pose();
    }
}
