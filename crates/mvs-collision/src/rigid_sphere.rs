//! A simulating body against the stage's static boxes: PhysX 3.4's contact
//! rows for one sphere (sphereStep), the sphere with a welded box (compoundStep) and a capsule (capsuleStep). The
//! settings, the materials and the mass properties are read once per class by the sim and come in here as numbers,
//! narrowed to float32 at the step's entry. The steps run in float32 in a fixed order of operations, each value stored
//! into the scratch flushed as PhysX's FTZ does (`ftz`).

use crate::solids::{Corners, Poly, bounds, corners, edge_nearest_at, edge_normal, inside};
use crate::{Box2, Ground, arg, ftz, max, min, out};
use mvs_math::f32::{crt, vector};

const GRAVITY_Z: f32 = -980.0;
const BOUNCE_THRESHOLD: f32 = 200.0; // BounceThresholdVelocity
const INERTIA_K: f32 = 2.5; // m R^2 / I of a solid sphere
const BIAS: f32 = 0.8;
const POS_ITERS: usize = 8;
const VEL_ITERS: usize = 1;
const FRICTION_ITERS: usize = 3;
// FBodyInstance MaxAngularVelocity: UPhysicsSettings MaxAngularVelocity=3600 (deg/s), FMath::DegreesToRadians (one
// float multiply).
const MAX_ANGULAR: f32 = 3600.0 * vector::DEG_TO_RAD;
// PxSceneDesc frictionOffsetThreshold: 0.04 x the tolerance length 100.
const FRICTION_OFFSET: f32 = 4.0;
const SAME_NORMAL: f32 = 0.999;
const CORRELATION: f32 = 2.5;
const MAX_ROWS: usize = 8;
const MAX_CROWS: usize = 32;
const MAX_PATCHES: usize = 16;

/// SphereSettings: the body's damping and its material combined with the stage's, and its rotation locks.
#[derive(Clone, Copy, Debug, Default)]
pub struct Settings {
    pub linear_damping: f64,
    pub angular_damping: f64,
    pub restitution: f64,
    pub friction: f64,
    pub lock_spin: bool,
    pub lock_y: bool,
    pub lock_z: bool,
}

/// A static the body meets (SphereBox), with what the solid's mark says of it: its kinematic move this step
/// (kinematicOf: None for a box that is no marked piece) and whether it is a WorldStatic stage solid
/// (isStaticSolid). `poly`: a stage solid's polygon section in place of the box (`g` unread).
#[derive(Clone, Debug, Default)]
pub struct Static {
    pub g: Ground,
    pub kinematic: Option<(f64, f64)>,
    pub static_solid: bool,
    pub poly: Option<PolyAt>,
}

/// A polygon section (`solids::Poly`) moved by (ox, oy): a polygon stage solid where it stands.
#[derive(Clone, Debug, Default)]
pub struct PolyAt {
    pub poly: std::sync::Arc<Poly>,
    pub ox: f64,
    pub oy: f64,
}

/// SphereBody, CompoundBody and CapsuleBody in one: the fields each step reads and writes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Body {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub spin: f64,
    pub radius: f64,
    pub angle: f64,
    pub wa: f64,
    pub wb: f64,
    pub wc: f64,
    pub half: f64,
}

/// WeldedBox: a child box welded into the sphere's body.
#[derive(Clone, Copy, Debug, Default)]
pub struct WeldedBox {
    pub hx: f64,
    pub hy: f64,
    pub hd: f64,
    pub oy: f64,
    pub offset: f64,
    pub platforms: bool,
    pub restitution: f64,
    pub friction: f64,
}

/// WeldedMass: the welded actor's body in the play plane.
#[derive(Clone, Copy, Debug, Default)]
pub struct WeldedMass {
    pub m: f64,
    pub ox: f64,
    pub oy: f64,
    pub k: f64,
    pub ia: f64,
    pub ib: f64,
    pub ic: f64,
}

/// A `Body` narrowed to float32 for a step.
#[derive(Clone, Copy, Debug)]
struct B {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    spin: f32,
    radius: f32,
    angle: f32,
    wa: f32,
    wb: f32,
    wc: f32,
    half: f32,
}

impl B {
    fn of(b: &Body) -> B {
        B {
            x: arg(b.x),
            y: arg(b.y),
            vx: arg(b.vx),
            vy: arg(b.vy),
            spin: arg(b.spin),
            radius: arg(b.radius),
            angle: arg(b.angle),
            wa: arg(b.wa),
            wb: arg(b.wb),
            wc: arg(b.wc),
            half: arg(b.half),
        }
    }
    // The place, the velocity and the spin every step writes.
    fn write(&self, b: &mut Body) {
        (b.x, b.y, b.vx, b.vy, b.spin) = (out(self.x), out(self.y), out(self.vx), out(self.vy), out(self.spin));
    }
}

/// The settings' numbers narrowed to float32 (the locks read as they are).
#[derive(Clone, Copy, Debug)]
struct Set {
    linear_damping: f32,
    angular_damping: f32,
    restitution: f32,
    friction: f32,
}

impl Set {
    fn of(m: &Settings) -> Set {
        Set {
            linear_damping: arg(m.linear_damping),
            angular_damping: arg(m.angular_damping),
            restitution: arg(m.restitution),
            friction: arg(m.friction),
        }
    }
}

/// The module's scratch: the rows of the running step (rebuilt by every step: no state between steps).
pub struct Solver {
    rnx: [f32; MAX_ROWS],
    rny: [f32; MAX_ROWS],
    rgap: [f32; MAX_ROWS],
    rtb: [f32; MAX_ROWS],
    rtu: [f32; MAX_ROWS],
    r_f: [f32; MAX_ROWS],
    r_ft: [f32; MAX_ROWS],
    rkn: [f32; MAX_ROWS],
    rkt: [f32; MAX_ROWS],
    rows: usize,
    kvx: f32,
    kvy: f32,
    cnx: [f32; MAX_CROWS],
    cny: [f32; MAX_CROWS],
    cgap: [f32; MAX_CROWS],
    crx: [f32; MAX_CROWS],
    cry: [f32; MAX_CROWS],
    crz: [f32; MAX_CROWS],
    ce: [f32; MAX_CROWS],
    cmu: [f32; MAX_CROWS],
    cst: [i32; MAX_CROWS],
    ca0: [f32; MAX_CROWS],
    ca1: [f32; MAX_CROWS],
    ca2: [f32; MAX_CROWS],
    cinv: [f32; MAX_CROWS],
    ctb: [f32; MAX_CROWS],
    ctu: [f32; MAX_CROWS],
    c_f: [f32; MAX_CROWS],
    ckx: [f32; MAX_CROWS],
    cky: [f32; MAX_CROWS],
    ckn: [f32; MAX_CROWS],
    crows: usize,
    pfrom: [usize; MAX_PATCHES],
    pto: [usize; MAX_PATCHES],
    ffrom: [usize; MAX_PATCHES],
    fto: [usize; MAX_PATCHES],
    patches: usize,
    fx: [f32; 4 * MAX_PATCHES],
    fy: [f32; 4 * MAX_PATCHES],
    fa0: [f32; 4 * MAX_PATCHES],
    fa1: [f32; 4 * MAX_PATCHES],
    fa2: [f32; 4 * MAX_PATCHES],
    fmul: [f32; 4 * MAX_PATCHES],
    f_ft: [f32; 4 * MAX_PATCHES],
    fkv: [f32; 4 * MAX_PATCHES],
    frows: usize,
    bpx: [f32; 2],
    bpy: [f32; 2],
    bgap: [f32; 2],
    bpts: usize,
    bnx: f32,
    bny: f32,
    kax: f32,
    kay: f32,
    kbx: f32,
    kby: f32,
}

impl Default for Solver {
    fn default() -> Self {
        Solver {
            rnx: [0.0; MAX_ROWS],
            rny: [0.0; MAX_ROWS],
            rgap: [0.0; MAX_ROWS],
            rtb: [0.0; MAX_ROWS],
            rtu: [0.0; MAX_ROWS],
            r_f: [0.0; MAX_ROWS],
            r_ft: [0.0; MAX_ROWS],
            rkn: [0.0; MAX_ROWS],
            rkt: [0.0; MAX_ROWS],
            rows: 0,
            kvx: 0.0,
            kvy: 0.0,
            cnx: [0.0; MAX_CROWS],
            cny: [0.0; MAX_CROWS],
            cgap: [0.0; MAX_CROWS],
            crx: [0.0; MAX_CROWS],
            cry: [0.0; MAX_CROWS],
            crz: [0.0; MAX_CROWS],
            ce: [0.0; MAX_CROWS],
            cmu: [0.0; MAX_CROWS],
            cst: [0; MAX_CROWS],
            ca0: [0.0; MAX_CROWS],
            ca1: [0.0; MAX_CROWS],
            ca2: [0.0; MAX_CROWS],
            cinv: [0.0; MAX_CROWS],
            ctb: [0.0; MAX_CROWS],
            ctu: [0.0; MAX_CROWS],
            c_f: [0.0; MAX_CROWS],
            ckx: [0.0; MAX_CROWS],
            cky: [0.0; MAX_CROWS],
            ckn: [0.0; MAX_CROWS],
            crows: 0,
            pfrom: [0; MAX_PATCHES],
            pto: [0; MAX_PATCHES],
            ffrom: [0; MAX_PATCHES],
            fto: [0; MAX_PATCHES],
            patches: 0,
            fx: [0.0; 4 * MAX_PATCHES],
            fy: [0.0; 4 * MAX_PATCHES],
            fa0: [0.0; 4 * MAX_PATCHES],
            fa1: [0.0; 4 * MAX_PATCHES],
            fa2: [0.0; 4 * MAX_PATCHES],
            fmul: [0.0; 4 * MAX_PATCHES],
            f_ft: [0.0; 4 * MAX_PATCHES],
            fkv: [0.0; 4 * MAX_PATCHES],
            frows: 0,
            bpx: [0.0; 2],
            bpy: [0.0; 2],
            bgap: [0.0; 2],
            bpts: 0,
            bnx: 0.0,
            bny: 0.0,
            kax: 0.0,
            kay: 0.0,
            kbx: 0.0,
            kby: 0.0,
        }
    }
}

// A box's contact offset: 0.01 x its smallest half extent in the plane, clamped to [0.0001, 1].
fn box_offset(s: &Box2) -> f32 {
    min(max(0.01 * min((s.right - s.left) / 2.0, (s.top - s.bottom) / 2.0), 0.0001), 1.0)
}

// A polygon's contact offset: the box rule over the box round it.
// Unverified: PhysX's convex mesh contact offset is not read; the box rule stands in.
fn poly_offset(c: Corners) -> f32 {
    let (l, r, b, t) = bounds(c);
    box_offset(&Box2 { left: l, right: r, bottom: b, top: t })
}

// The body's own contact offset: 0.01 x its radius, clamped to [0.0001, 1].
fn own_offset(radius: f32) -> f32 {
    min(max(0.01 * radius, 0.0001), 1.0)
}

// The axis of a planar body's roll (cos, sin).
// Unverified: the CRT cosf / sinf (PhysX's PxCos / PxSin) of the port's planar angle in radians; the build's body
// keeps a quaternion, which the port's planar body does not.
fn roll(angle: f32) -> (f32, f32) {
    (crt::cosf(angle), crt::sinf(angle))
}

impl Solver {
    // startPose: sets kvx / kvy and returns the box to make the contacts against.
    fn start_pose(&mut self, s: &Static, dt: f32) -> Box2 {
        let g = s.g.narrow();
        match s.kinematic.map(|(dx, dy)| (arg(dx), arg(dy))) {
            Some((dx, dy)) if !(dx == 0.0 && dy == 0.0) => {
                self.kvx = ftz(dx / dt);
                self.kvy = ftz(dy / dt);
                Box2 { left: g.left - dx, right: g.right - dx, top: g.top - dy, bottom: g.bottom - dy }
            }
            _ => {
                self.kvx = 0.0;
                self.kvy = 0.0;
                g
            }
        }
    }

    // The polygon of a static, its velocity zero (a polygon is no marked piece: kinematicOf None).
    fn poly_pose<'a>(&mut self, s: &'a Static) -> Option<Corners<'a>> {
        let p = s.poly.as_ref()?;
        self.kvx = 0.0;
        self.kvy = 0.0;
        Some(corners(&p.poly, arg(p.ox), arg(p.oy)))
    }

    // The sphere against one static: its polygon or its box.
    fn contact_any(&mut self, b: &B, s: &Static, dt: f32) {
        match self.poly_pose(s) {
            Some(c) => self.poly_contact(b, c),
            None => {
                let g = self.start_pose(s, dt);
                self.contact(b, &g);
            }
        }
    }

    // poly_contact: contact's rule against a polygon: its nearest point for the box's clamp; inside it, the nearest
    // point of its outline (the way out) for the box's nearer face.
    // Unverified: PhysX's sphere-convex contact is not read; the box rule over the polygon's nearest point.
    fn poly_contact(&mut self, b: &B, c: Corners) {
        let (i, qx, qy) = edge_nearest_at(c, b.x, b.y);
        let (nx, ny, gap): (f32, f32, f32);
        if inside(c, b.x, b.y) {
            let (ex, ey) = (ftz(qx - b.x), ftz(qy - b.y));
            let d = ftz((ex * ex + ey * ey).sqrt());
            (nx, ny) = if d > 0.0 { (ftz(ex / d), ftz(ey / d)) } else { edge_normal(c, i) };
            gap = ftz(-d - b.radius);
        } else {
            let (ex, ey) = (ftz(b.x - qx), ftz(b.y - qy));
            let d = ftz((ex * ex + ey * ey).sqrt());
            gap = ftz(d - b.radius);
            if !(gap < ftz(own_offset(b.radius) + poly_offset(c))) {
                return;
            }
            (nx, ny) = if d > 0.0 { (ftz(ex / d), ftz(ey / d)) } else { edge_normal(c, i) };
        }
        if self.rows == MAX_ROWS {
            return;
        }
        let r = self.rows;
        self.rnx[r] = nx;
        self.rny[r] = ny;
        self.rgap[r] = gap;
        self.rkn[r] = ftz(nx * self.kvx + ny * self.kvy);
        self.rkt[r] = ftz(ny * self.kvx - nx * self.kvy);
        self.rows += 1;
    }

    // contact: the sphere's contact point against one box at the pose the step starts from.
    fn contact(&mut self, b: &B, s: &Box2) {
        let qx = max(s.left, min(s.right, b.x));
        let qy = max(s.bottom, min(s.top, b.y));
        let (ex, ey) = (ftz(b.x - qx), ftz(b.y - qy));
        let (nx, ny, gap): (f32, f32, f32);
        if ex == 0.0 && ey == 0.0 {
            let (dl, dr, db, dtop) = (b.x - s.left, s.right - b.x, b.y - s.bottom, s.top - b.y);
            let (px, py) = (min(dl, dr), min(db, dtop));
            if px <= py {
                nx = if dr < dl { 1.0 } else { -1.0 };
                ny = 0.0;
                gap = ftz(-px - b.radius);
            } else {
                nx = 0.0;
                ny = if dtop <= db { 1.0 } else { -1.0 };
                gap = ftz(-py - b.radius);
            }
        } else {
            let d = ftz((ex * ex + ey * ey).sqrt());
            gap = ftz(d - b.radius);
            let off = box_offset(s);
            if !(gap < ftz(own_offset(b.radius) + off)) {
                return;
            }
            nx = ftz(ex / d);
            ny = ftz(ey / d);
        }
        if self.rows == MAX_ROWS {
            return; // more contacts than rows: the step stops
        }
        let r = self.rows;
        self.rnx[r] = nx;
        self.rny[r] = ny;
        self.rgap[r] = gap;
        self.rkn[r] = ftz(nx * self.kvx + ny * self.kvy);
        self.rkt[r] = ftz(ny * self.kvx - nx * self.kvy);
        self.rows += 1;
    }

    /// sphereStep: one physics step of the sphere. Returns the normal impulse the solver applied, as a velocity change.
    pub fn sphere_step(
        &mut self,
        body: &mut Body,
        floor: &Static,
        platforms: Option<&[Static]>,
        dt: impl Into<f64>,
        settings: &Settings,
        k: impl Into<f64>,
    ) -> f64 {
        let (dt, k) = (arg(dt), arg(k));
        let mut b = B::of(body);
        let m = Set::of(settings);
        let mut vx = b.vx;
        let mut vy = ftz(b.vy + ftz(dt * GRAVITY_Z));
        let damp = ftz(max(0.0, 1.0 - dt * m.linear_damping));
        vx = ftz(vx * damp);
        vy = ftz(vy * damp);
        let lock = settings.lock_spin;
        let mut s = if lock { 0.0 } else { ftz(b.spin * ftz(max(0.0, 1.0 - dt * m.angular_damping))) };
        self.rows = 0;
        self.contact_any(&b, floor, dt);
        if let Some(ps) = platforms {
            for p in ps {
                self.contact_any(&b, p, dt);
            }
        }
        for i in 0..self.rows {
            let vn = ftz(ftz(self.rnx[i] * vx + self.rny[i] * vy) - self.rkn[i]);
            let g = self.rgap[i];
            if vn < -BOUNCE_THRESHOLD && -vn > g / dt {
                let t = ftz(-m.restitution * vn);
                self.rtu[i] = t;
                self.rtb[i] = t;
            } else {
                let a = ftz((BIAS * g) / dt);
                self.rtb[i] = -a;
                self.rtu[i] = -max(a, 0.0);
            }
            self.r_f[i] = 0.0;
            self.r_ft[i] = 0.0;
        }
        let (mut mx, mut my) = (vx, vy);
        if self.rows > 0 {
            for it in 0..POS_ITERS + VEL_ITERS {
                let vel_phase = it >= POS_ITERS;
                let fric = POS_ITERS as isize - it as isize <= FRICTION_ITERS as isize;
                if lock {
                    s = 0.0;
                }
                for i in 0..self.rows {
                    let (nx, ny) = (self.rnx[i], self.rny[i]);
                    let target = if vel_phase { self.rtu[i] } else { self.rtb[i] };
                    let mut d = ftz(target - ftz(ftz(nx * vx + ny * vy) - self.rkn[i]));
                    let rf = self.r_f[i];
                    if d < -rf {
                        d = -rf;
                    }
                    self.r_f[i] = ftz(rf + d);
                    vx = ftz(vx + d * nx);
                    vy = ftz(vy + d * ny);
                    if !fric {
                        continue;
                    }
                    let (tx, ty) = (ny, -nx);
                    let u = ftz(ftz(tx * vx + ty * vy + s) - self.rkt[i]);
                    let rft = self.r_ft[i];
                    let mut tot = ftz(rft - (BIAS * u) / (1.0 + k));
                    let lim = ftz(m.friction * self.r_f[i]);
                    if tot > lim {
                        tot = lim;
                    } else if tot < -lim {
                        tot = -lim;
                    }
                    let df = ftz(tot - rft);
                    self.r_ft[i] = tot;
                    vx = ftz(vx + df * tx);
                    vy = ftz(vy + df * ty);
                    s = ftz(s + k * df);
                }
                if it == POS_ITERS - 1 {
                    mx = vx;
                    my = vy;
                }
            }
        }
        b.x = ftz(b.x + ftz(mx * dt));
        b.y = ftz(b.y + ftz(my * dt));
        b.vx = vx;
        b.vy = vy;
        b.spin = s;
        b.write(body);
        let mut sum = 0.0f32;
        for i in 0..self.rows {
            sum += self.r_f[i];
        }
        out(sum)
    }

    #[allow(clippy::too_many_arguments)]
    fn push_row(&mut self, nx: f32, ny: f32, gap: f32, rx: f32, ry: f32, rz: f32, e: f32, mu: f32, st: i32) {
        if self.crows == MAX_CROWS {
            return; // the step stops
        }
        let c = self.crows;
        self.cnx[c] = ftz(nx);
        self.cny[c] = ftz(ny);
        self.cgap[c] = ftz(gap);
        self.crx[c] = ftz(rx);
        self.cry[c] = ftz(ry);
        self.crz[c] = ftz(rz);
        self.ce[c] = e;
        self.cmu[c] = mu;
        self.cst[c] = st;
        self.ckx[c] = self.kvx;
        self.cky[c] = self.kvy;
        self.ckn[c] = ftz(nx * self.kvx + ny * self.kvy);
        self.crows += 1;
    }

    // boxContact: the welded box against one of the stage's boxes (PhysX's box-box manifold in the plane).
    fn box_contact(&mut self, b: &B, bx: &Welded, s: &Box2) {
        self.bpts = 0;
        let (c, sn) = roll(b.angle);
        let lx = [-bx.hx, bx.hx, bx.hx, -bx.hx];
        let ly = [bx.oy - bx.hy, bx.oy - bx.hy, bx.oy + bx.hy, bx.oy + bx.hy];
        let (mut qx, mut qy) = ([0.0f32; 4], [0.0f32; 4]);
        let (mut min_x, mut max_x, mut min_y, mut max_y) =
            (f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY);
        for i in 0..4 {
            qx[i] = b.x + (c * lx[i] - sn * ly[i]);
            qy[i] = b.y + (sn * lx[i] + c * ly[i]);
            min_x = min(min_x, qx[i]);
            max_x = max(max_x, qx[i]);
            min_y = min(min_y, qy[i]);
            max_y = max(max_y, qy[i]);
        }
        let (mut nx, mut ny, mut plane, mut sep) = (0.0f32, 1.0f32, s.top, min_y - s.top);
        if s.bottom - max_y > sep {
            (nx, ny, plane, sep) = (0.0, -1.0, -s.bottom, s.bottom - max_y);
        }
        if min_x - s.right > sep {
            (nx, ny, plane, sep) = (1.0, 0.0, s.right, min_x - s.right);
        }
        if s.left - max_x > sep {
            (nx, ny, plane, sep) = (-1.0, 0.0, -s.left, s.left - max_x);
        }
        let off = bx.offset + box_offset(s);
        if !(sep < off) {
            return;
        }
        let (mut best, mut ia) = (f32::INFINITY, 0usize);
        for i in 0..4 {
            let j = (i + 1) & 3;
            let (ex, ey) = (qx[j] - qx[i], qy[j] - qy[i]);
            let d = (ey * nx - ex * ny) / (ex * ex + ey * ey).sqrt();
            if d < best {
                best = d;
                ia = i;
            }
        }
        let ib = (ia + 1) & 3;
        let (lo, hi) = if ny != 0.0 { (s.left, s.right) } else { (s.bottom, s.top) };
        self.bnx = nx;
        self.bny = ny;
        for k in 0..2 {
            let (p, q) = if k == 0 { (ia, ib) } else { (ib, ia) };
            let (mut px, mut py) = (qx[p], qy[p]);
            let (tp, tq) = if ny != 0.0 { (px, qx[q]) } else { (py, qy[q]) };
            if tp < lo || tp > hi {
                let edge = if tp < lo { lo } else { hi };
                if !((tq - edge) * (tp - edge) < 0.0) {
                    continue;
                }
                let u = (edge - tp) / (tq - tp);
                px += u * (qx[q] - px);
                py += u * (qy[q] - py);
            }
            let d = nx * px + ny * py - plane;
            if !(d < off) {
                continue;
            }
            let n = self.bpts;
            self.bpx[n] = px - nx * d;
            self.bpy[n] = py - ny * d;
            self.bgap[n] = ftz(d);
            self.bpts += 1;
        }
    }

    // The polygon's edge of least penetration against the body's support `lo(nx, ny)` (its least projection on the
    // edge's outward normal): (edge, nx, ny, plane, separation), the first of equal ones; none for a polygon of no
    // edges.
    fn poly_face(c: Corners, lo: impl Fn(f32, f32) -> f32) -> Option<(usize, f32, f32, f32, f32)> {
        let mut best: Option<(usize, f32, f32, f32, f32)> = None;
        for i in 0..c.n() {
            let j = (i + 1) % c.n();
            let (ex, ey) = (c.x(j) - c.x(i), c.y(j) - c.y(i));
            let l = (ex * ex + ey * ey).sqrt();
            if !(l > 0.0) {
                continue;
            }
            let (nx, ny) = (ey / l, -ex / l);
            let plane = nx * c.x(i) + ny * c.y(i);
            let sep = lo(nx, ny) - plane;
            if best.is_none_or(|b| sep > b.4) {
                best = Some((i, nx, ny, plane, sep));
            }
        }
        best
    }

    // poly_box_contact: box_contact's manifold against a polygon: the reference face is the polygon's edge of least
    // penetration (its outward normal), the welded box's edge facing it clipped to the face's span.
    // Unverified: PhysX's box-convex manifold is not read; box_contact's rule over the polygon's edges.
    fn poly_box_contact(&mut self, b: &B, bx: &Welded, c: Corners) {
        self.bpts = 0;
        let (cs, sn) = roll(b.angle);
        let lx = [-bx.hx, bx.hx, bx.hx, -bx.hx];
        let ly = [bx.oy - bx.hy, bx.oy - bx.hy, bx.oy + bx.hy, bx.oy + bx.hy];
        let (mut qx, mut qy) = ([0.0f32; 4], [0.0f32; 4]);
        for i in 0..4 {
            qx[i] = b.x + (cs * lx[i] - sn * ly[i]);
            qy[i] = b.y + (sn * lx[i] + cs * ly[i]);
        }
        let support = |nx: f32, ny: f32| (0..4).fold(f32::INFINITY, |m, i| min(m, nx * qx[i] + ny * qy[i]));
        let Some((f, nx, ny, plane, sep)) = Self::poly_face(c, support) else { return };
        let off = bx.offset + poly_offset(c);
        if !(sep < off) {
            return;
        }
        let (mut best, mut ia) = (f32::INFINITY, 0usize);
        for i in 0..4 {
            let j = (i + 1) & 3;
            let (ex, ey) = (qx[j] - qx[i], qy[j] - qy[i]);
            let d = (ey * nx - ex * ny) / (ex * ex + ey * ey).sqrt();
            if d < best {
                best = d;
                ia = i;
            }
        }
        let ib = (ia + 1) & 3;
        // the face's span along its tangent (the edge's direction)
        let (tx, ty) = (-ny, nx);
        let g = (f + 1) % c.n();
        let (lo, hi) = (tx * c.x(f) + ty * c.y(f), tx * c.x(g) + ty * c.y(g));
        self.bnx = nx;
        self.bny = ny;
        for k in 0..2 {
            let (p, q) = if k == 0 { (ia, ib) } else { (ib, ia) };
            let (mut px, mut py) = (qx[p], qy[p]);
            let (tp, tq) = (tx * px + ty * py, tx * qx[q] + ty * qy[q]);
            if tp < lo || tp > hi {
                let edge = if tp < lo { lo } else { hi };
                if !((tq - edge) * (tp - edge) < 0.0) {
                    continue;
                }
                let u = (edge - tp) / (tq - tp);
                px += u * (qx[q] - px);
                py += u * (qy[q] - py);
            }
            let d = nx * px + ny * py - plane;
            if !(d < off) {
                continue;
            }
            let n = self.bpts;
            self.bpx[n] = px - nx * d;
            self.bpy[n] = py - ny * d;
            self.bgap[n] = ftz(d);
            self.bpts += 1;
        }
    }

    // poly_capsule_contact: capsule_contact's rows against a polygon: the reference face is the polygon's edge of
    // least penetration, the capsule's axis clipped to its span; an axis wholly past the span meets the face's corner.
    // Unverified: PhysX's capsule-convex contact is not read; capsule_contact's rule over the polygon's edges.
    fn poly_capsule_contact(&mut self, b: &B, c: Corners, k: i32, e: f32, mu: f32) {
        let r_ = b.radius;
        let (kax, kay, kbx, kby) = (self.kax, self.kay, self.kbx, self.kby);
        let support = |nx: f32, ny: f32| min(nx * kax + ny * kay, nx * kbx + ny * kby) - r_;
        let Some((f, nx, ny, plane, sep)) = Self::poly_face(c, support) else { return };
        let off = own_offset(r_) + poly_offset(c);
        if !(sep < off) {
            return;
        }
        let (tx, ty) = (-ny, nx);
        let g = (f + 1) % c.n();
        let (lo, hi) = (tx * c.x(f) + ty * c.y(f), tx * c.x(g) + ty * c.y(g));
        let (ta, tb) = (tx * kax + ty * kay, tx * kbx + ty * kby);
        if max(ta, tb) < lo || min(ta, tb) > hi {
            // wholly past the face's span: the face's corner against the segment
            let (cx, cy) = if max(ta, tb) < lo { (c.x(f), c.y(f)) } else { (c.x(g), c.y(g)) };
            let (dx, dy) = (kbx - kax, kby - kay);
            let ll = dx * dx + dy * dy;
            let u = if ll > 0.0 { min(max(((cx - kax) * dx + (cy - kay) * dy) / ll, 0.0), 1.0) } else { 0.0 };
            let (qx, qy) = (kax + u * dx - cx, kay + u * dy - cy);
            let d = (qx * qx + qy * qy).sqrt();
            let gap = ftz(d - r_);
            if !(gap < off) {
                return;
            }
            let ex = if d > 0.0 { ftz(qx / d) } else { nx };
            let ey = if d > 0.0 { ftz(qy / d) } else { ny };
            self.push_row(ex, ey, gap, cx - b.x, cy - b.y, 0.0, e, mu, k);
            return;
        }
        let first = self.crows;
        for q in 0..2 {
            let (mut px, mut py) = if q == 1 { (kbx, kby) } else { (kax, kay) };
            let (ox, oy) = if q == 1 { (kax, kay) } else { (kbx, kby) };
            let (tp, tq) = (tx * px + ty * py, tx * ox + ty * oy);
            if tp < lo || tp > hi {
                let edge = if tp < lo { lo } else { hi };
                let u = (edge - tp) / (tq - tp);
                px += u * (ox - px);
                py += u * (oy - py);
            }
            let d = ftz(nx * px + ny * py - plane - r_);
            if !(d < off) {
                continue;
            }
            self.push_row(nx, ny, d, px - nx * r_ - b.x, py - ny * r_ - b.y, 0.0, e, mu, k);
        }
        if self.crows - first == 2 && self.cgap[first + 1] < self.cgap[first] {
            self.cnx.swap(first, first + 1);
            self.cny.swap(first, first + 1);
            self.cgap.swap(first, first + 1);
            self.crx.swap(first, first + 1);
            self.cry.swap(first, first + 1);
        }
    }

    fn row_dist2(&self, i: usize, j: usize) -> f32 {
        let x = self.crx[i] - self.crx[j];
        let y = self.cry[i] - self.cry[j];
        let z = self.crz[i] - self.crz[j];
        x * x + y * y + z * z
    }

    // growPatches: the two friction anchors of patch p (-1: none).
    fn anchors(&self, p: usize) -> (isize, isize) {
        let (mut n0, mut n1, mut dd) = (-1isize, -1isize, 0.0f32);
        for i in self.pfrom[p]..self.pto[p] {
            if !(self.cgap[i] < FRICTION_OFFSET) {
                continue;
            }
            if n0 < 0 {
                n0 = i as isize;
            } else if n1 < 0 {
                let d = self.row_dist2(i, n0 as usize);
                if d > CORRELATION * CORRELATION {
                    n1 = i as isize;
                    dd = d;
                }
            } else {
                let d0 = self.row_dist2(i, n0 as usize);
                let d1 = self.row_dist2(i, n1 as usize);
                if d0 > d1 {
                    if d0 > dd {
                        n1 = i as isize;
                        dd = d0;
                    }
                } else if d1 > dd {
                    n0 = i as isize;
                    dd = d1;
                }
            }
        }
        (n0, n1)
    }

    // the bounce or bias targets of row i (the rows' setup in compoundStep / capsuleStep)
    fn targets(&mut self, i: usize, vn: f32, dt: f32) {
        let g = self.cgap[i];
        if vn < -BOUNCE_THRESHOLD && -vn > g / dt {
            let t = ftz(-self.ce[i] * vn);
            self.ctu[i] = t;
            self.ctb[i] = t;
        } else {
            let t = ftz((BIAS * g) / dt);
            self.ctb[i] = -t;
            self.ctu[i] = -max(t, 0.0);
        }
        self.c_f[i] = 0.0;
    }

    /// compoundStep: one physics step of the sphere with the welded box.
    #[allow(clippy::too_many_arguments)]
    pub fn compound_step(
        &mut self,
        body: &mut Body,
        floor: &Static,
        platforms: Option<&[Static]>,
        dt: impl Into<f64>,
        settings: &Settings,
        welded: &WeldedBox,
        mass: &WeldedMass,
    ) -> f64 {
        let dt = arg(dt);
        let mut b = B::of(body);
        let m = Set::of(settings);
        let bx = Welded::of(welded);
        let (wm_m, wm_ox, wm_oy) = (arg(mass.m), arg(mass.ox), arg(mass.oy));
        let (la, lb, lc) = (settings.lock_y, settings.lock_z, settings.lock_spin);
        let mut vx = b.vx;
        let mut vy = ftz(b.vy + ftz(dt * GRAVITY_Z));
        let damp = ftz(max(0.0, 1.0 - dt * m.linear_damping));
        vx = ftz(vx * damp);
        vy = ftz(vy * damp);
        let r_ = b.radius;
        let adamp = ftz(max(0.0, 1.0 - dt * m.angular_damping));
        let (mut wa, mut wb, mut wc) = (ftz(b.wa * adamp), ftz(b.wb * adamp), ftz(b.wc * adamp));
        let wl = (wa * wa + wb * wb + wc * wc).sqrt();
        // Unverified: the port's clamp (scale by MAX / |w|), not PhysX's (|w|^2 against the squared limit, then
        // times sqrt(limit^2 / |w|^2)).
        if wl > MAX_ANGULAR {
            let k = MAX_ANGULAR / wl;
            wa = ftz(wa * k);
            wb = ftz(wb * k);
            wc = ftz(wc * k);
        }
        if la {
            wa = 0.0;
        }
        if lb {
            wb = 0.0;
        }
        if lc {
            wc = 0.0;
        }
        let (c, sn) = roll(b.angle);
        let comx = ftz(b.x + (c * wm_ox - sn * wm_oy));
        let comy = ftz(b.y + (sn * wm_ox + c * wm_oy));
        let (ja, jb, jc, im) =
            (ftz(1.0 / arg(mass.ia)), ftz(1.0 / arg(mass.ib)), ftz(1.0 / arg(mass.ic)), ftz(1.0 / wm_m));
        let j00 = ftz(c * c * ja + sn * sn * jb);
        let j01 = ftz(c * sn * (ja - jb));
        let j11 = ftz(sn * sn * ja + c * c * jb);
        self.crows = 0;
        let n = 1 + platforms.map_or(0, |p| p.len());
        for k in 0..n {
            let raw = if k == 0 { floor } else { &platforms.unwrap()[k - 1] };
            let poly = self.poly_pose(raw);
            let s = if poly.is_some() { Box2::default() } else { self.start_pose(raw, dt) };
            self.rows = 0;
            match poly {
                Some(c) => self.poly_contact(&b, c),
                None => self.contact(&b, &s),
            }
            if self.rows > 0 {
                let (rnx, rny, rgap) = (self.rnx[0], self.rny[0], self.rgap[0]);
                self.push_row(
                    rnx,
                    rny,
                    rgap,
                    b.x - rnx * r_ - comx,
                    b.y - rny * r_ - comy,
                    0.0,
                    m.restitution,
                    m.friction,
                    k as i32,
                );
            }
            if k > 0 && !bx.platforms && !raw.static_solid {
                continue;
            }
            match poly {
                Some(c) => self.poly_box_contact(&b, &bx, c),
                None => self.box_contact(&b, &bx, &s),
            }
            for i in 0..self.bpts {
                for h in 0..2 {
                    let (bnx, bny, bg, px, py) = (self.bnx, self.bny, self.bgap[i], self.bpx[i], self.bpy[i]);
                    let rz = if h == 1 { -bx.hd } else { bx.hd };
                    self.push_row(bnx, bny, bg, px - comx, py - comy, rz, bx.restitution, bx.friction, k as i32);
                }
            }
        }
        for i in 0..self.crows {
            let (nx, ny) = (self.cnx[i], self.cny[i]);
            let (rx, ry, rz) = (self.crx[i], self.cry[i], self.crz[i]);
            let (a0, a1, a2) = (-rz * ny, rz * nx, rx * ny - ry * nx);
            self.ca0[i] = ftz(a0);
            self.ca1[i] = ftz(a1);
            self.ca2[i] = ftz(a2);
            self.cinv[i] = ftz(1.0 / (im + a0 * (j00 * a0 + j01 * a1) + a1 * (j01 * a0 + j11 * a1) + a2 * jc * a2));
            let vn = ftz(ftz(nx * vx + ny * vy + a0 * wa + a1 * wb + a2 * wc) - self.ckn[i]);
            self.targets(i, vn, dt);
        }
        self.patches = 0;
        self.frows = 0;
        for i in 0..self.crows {
            if self.patches > 0 {
                let f = self.pfrom[self.patches - 1];
                if self.cst[i] == self.cst[f]
                    && self.ce[i] == self.ce[f]
                    && self.cmu[i] == self.cmu[f]
                    && self.cnx[i] * self.cnx[f] + self.cny[i] * self.cny[f] >= SAME_NORMAL
                {
                    self.pto[self.patches - 1] = i + 1;
                    continue;
                }
            }
            if self.patches == MAX_PATCHES {
                break; // the step stops
            }
            self.pfrom[self.patches] = i;
            self.pto[self.patches] = i + 1;
            self.patches += 1;
        }
        for p in 0..self.patches {
            let (n0, n1) = self.anchors(p);
            self.ffrom[p] = self.frows;
            let (nx, ny) = (self.cnx[self.pfrom[p]], self.cny[self.pfrom[p]]);
            for q in 0..2 {
                let i = if q == 1 { n1 } else { n0 };
                if i < 0 {
                    continue;
                }
                let i = i as usize;
                let (rx, ry, rz) = (self.crx[i], self.cry[i], self.crz[i]);
                for t in 0..2 {
                    let (tx, ty, tz) = if t == 0 { (ny, -nx, 0.0f32) } else { (0.0, 0.0, -1.0) };
                    let (a0, a1, a2) = (ry * tz - rz * ty, rz * tx - rx * tz, rx * ty - ry * tx);
                    let r = self.frows;
                    self.fx[r] = tx;
                    self.fy[r] = ty;
                    self.fa0[r] = ftz(a0);
                    self.fa1[r] = ftz(a1);
                    self.fa2[r] = ftz(a2);
                    let resp = im * (tx * tx + ty * ty)
                        + a0 * (j00 * a0 + j01 * a1)
                        + a1 * (j01 * a0 + j11 * a1)
                        + a2 * jc * a2;
                    self.fmul[r] = ftz(if resp > 0.0 { BIAS / resp } else { 0.0 });
                    self.f_ft[r] = 0.0;
                    self.fkv[r] = ftz(tx * self.ckx[i] + ty * self.cky[i]);
                    self.frows += 1;
                }
            }
            self.fto[p] = self.frows;
        }
        let (mut mx, mut my, mut mc) = (vx, vy, wc);
        if self.crows > 0 {
            for it in 0..POS_ITERS + VEL_ITERS {
                let vel_phase = it >= POS_ITERS;
                let fric = POS_ITERS as isize - it as isize <= FRICTION_ITERS as isize;
                if la {
                    wa = 0.0;
                }
                if lb {
                    wb = 0.0;
                }
                if lc {
                    wc = 0.0;
                }
                for p in 0..self.patches {
                    let mut sum_f = 0.0f32;
                    for i in self.pfrom[p]..self.pto[p] {
                        let (a0, a1, a2) = (self.ca0[i], self.ca1[i], self.ca2[i]);
                        let (cnx, cny) = (self.cnx[i], self.cny[i]);
                        let vn = ftz(ftz(cnx * vx + cny * vy + a0 * wa + a1 * wb + a2 * wc) - self.ckn[i]);
                        let target = if vel_phase { self.ctu[i] } else { self.ctb[i] };
                        let mut d = ftz(ftz(target - vn) * self.cinv[i]);
                        let cf = self.c_f[i];
                        if d < -cf {
                            d = -cf;
                        }
                        self.c_f[i] = ftz(cf + d);
                        sum_f += self.c_f[i];
                        vx = ftz(vx + d * cnx * im);
                        vy = ftz(vy + d * cny * im);
                        wa = ftz(wa + d * (j00 * a0 + j01 * a1));
                        wb = ftz(wb + d * (j01 * a0 + j11 * a1));
                        wc = ftz(wc + d * jc * a2);
                    }
                    if !fric {
                        continue;
                    }
                    let lim = ftz(self.cmu[self.pfrom[p]] * sum_f);
                    for r in self.ffrom[p]..self.fto[p] {
                        let (a0, a1, a2) = (self.fa0[r], self.fa1[r], self.fa2[r]);
                        let (fx, fy) = (self.fx[r], self.fy[r]);
                        let u = ftz(ftz(fx * vx + fy * vy + a0 * wa + a1 * wb + a2 * wc) - self.fkv[r]);
                        let ft = self.f_ft[r];
                        let mut tot = ftz(ft - self.fmul[r] * u);
                        if tot > lim {
                            tot = lim;
                        } else if tot < -lim {
                            tot = -lim;
                        }
                        let df = ftz(tot - ft);
                        self.f_ft[r] = tot;
                        vx = ftz(vx + df * fx * im);
                        vy = ftz(vy + df * fy * im);
                        wa = ftz(wa + df * (j00 * a0 + j01 * a1));
                        wb = ftz(wb + df * (j01 * a0 + j11 * a1));
                        wc = ftz(wc + df * jc * a2);
                    }
                }
                if it == POS_ITERS - 1 {
                    mx = vx;
                    my = vy;
                    mc = wc;
                }
            }
        }
        let ncx = ftz(comx + ftz(mx * dt));
        let ncy = ftz(comy + ftz(my * dt));
        if !lc {
            b.angle = ftz(b.angle + ftz(mc * dt));
        }
        let (c2, s2) = roll(b.angle);
        b.x = ftz(ncx - (c2 * wm_ox - s2 * wm_oy));
        b.y = ftz(ncy - (s2 * wm_ox + c2 * wm_oy));
        b.vx = vx;
        b.vy = vy;
        b.wc = wc;
        b.spin = ftz(wc * r_);
        b.wa = wa;
        b.wb = wb;
        b.write(body);
        (body.angle, body.wa, body.wb, body.wc) = (out(b.angle), out(wa), out(wb), out(wc));
        let mut sum = 0.0f32;
        for i in 0..self.crows {
            sum += self.c_f[i];
        }
        out(sum * im)
    }

    // capsuleContact: the capsule's rows against one box (one patch, the box face's normal).
    #[allow(clippy::too_many_arguments)]
    fn capsule_contact(&mut self, b: &B, s: &Box2, k: i32, e: f32, mu: f32) {
        let r_ = b.radius;
        let (kax, kay, kbx, kby) = (self.kax, self.kay, self.kbx, self.kby);
        let min_x = min(kax, kbx) - r_;
        let max_x = max(kax, kbx) + r_;
        let min_y = min(kay, kby) - r_;
        let max_y = max(kay, kby) + r_;
        let (mut nx, mut ny, mut plane, mut sep) = (0.0f32, 1.0f32, s.top, min_y - s.top);
        if s.bottom - max_y > sep {
            (nx, ny, plane, sep) = (0.0, -1.0, -s.bottom, s.bottom - max_y);
        }
        if min_x - s.right > sep {
            (nx, ny, plane, sep) = (1.0, 0.0, s.right, min_x - s.right);
        }
        if s.left - max_x > sep {
            (nx, ny, plane, sep) = (-1.0, 0.0, -s.left, s.left - max_x);
        }
        let off = own_offset(r_) + box_offset(s);
        if !(sep < off) {
            return;
        }
        let (lo, hi) = if ny != 0.0 { (s.left, s.right) } else { (s.bottom, s.top) };
        let (ta, tb) = if ny != 0.0 { (kax, kbx) } else { (kay, kby) };
        if max(ta, tb) < lo || min(ta, tb) > hi {
            // wholly past the face's span: the box's edge against the segment
            let t = if max(ta, tb) < lo { lo } else { hi };
            let cx = if ny != 0.0 {
                t
            } else if nx > 0.0 {
                s.right
            } else {
                s.left
            };
            let cy = if ny != 0.0 { if ny > 0.0 { s.top } else { s.bottom } } else { t };
            let (dx, dy) = (kbx - kax, kby - kay);
            let ll = dx * dx + dy * dy;
            let u = if ll > 0.0 { min(max(((cx - kax) * dx + (cy - kay) * dy) / ll, 0.0), 1.0) } else { 0.0 };
            let (qx, qy) = (kax + u * dx - cx, kay + u * dy - cy);
            let d = (qx * qx + qy * qy).sqrt();
            let gap = ftz(d - r_);
            if !(gap < off) {
                return;
            }
            let ex = if d > 0.0 { ftz(qx / d) } else { nx };
            let ey = if d > 0.0 { ftz(qy / d) } else { ny };
            self.push_row(ex, ey, gap, cx - b.x, cy - b.y, 0.0, e, mu, k);
            return;
        }
        let first = self.crows;
        for q in 0..2 {
            let (mut px, mut py) = if q == 1 { (kbx, kby) } else { (kax, kay) };
            let (ox, oy) = if q == 1 { (kax, kay) } else { (kbx, kby) };
            let (tp, tq) = if ny != 0.0 { (px, ox) } else { (py, oy) };
            if tp < lo || tp > hi {
                let edge = if tp < lo { lo } else { hi };
                let u = (edge - tp) / (tq - tp);
                px += u * (ox - px);
                py += u * (oy - py);
            }
            let d = ftz(nx * px + ny * py - plane - r_);
            if !(d < off) {
                continue;
            }
            self.push_row(nx, ny, d, px - nx * r_ - b.x, py - ny * r_ - b.y, 0.0, e, mu, k);
        }
        if self.crows - first == 2 && self.cgap[first + 1] < self.cgap[first] {
            // the deeper first: only cnx, cny, cgap, crx and cry swap
            self.cnx.swap(first, first + 1);
            self.cny.swap(first, first + 1);
            self.cgap.swap(first, first + 1);
            self.crx.swap(first, first + 1);
            self.cry.swap(first, first + 1);
        }
    }

    /// capsuleStep: one physics step of the capsule against the floor and the platforms.
    pub fn capsule_step(
        &mut self,
        body: &mut Body,
        floor: &Static,
        platforms: Option<&[Static]>,
        dt: impl Into<f64>,
        settings: &Settings,
    ) -> f64 {
        let dt = arg(dt);
        let mut b = B::of(body);
        let m = Set::of(settings);
        let mut vx = b.vx;
        let mut vy = ftz(b.vy + ftz(dt * GRAVITY_Z));
        let damp = ftz(max(0.0, 1.0 - dt * m.linear_damping));
        vx = ftz(vx * damp);
        vy = ftz(vy * damp);
        let mut w = if settings.lock_spin { 0.0 } else { ftz(b.wc * ftz(max(0.0, 1.0 - dt * m.angular_damping))) };
        if w.abs() > MAX_ANGULAR {
            w = if w > 0.0 { MAX_ANGULAR } else { -MAX_ANGULAR };
        }
        let r_ = b.radius;
        let l = max(b.half - r_, 0.0);
        let (c, sn) = roll(b.angle);
        let (ux, uy) = (-sn, c);
        self.kax = b.x + ux * l;
        self.kay = b.y + uy * l;
        self.kbx = b.x - ux * l;
        self.kby = b.y - uy * l;
        self.crows = 0;
        let n = 1 + platforms.map_or(0, |p| p.len());
        for k in 0..n {
            let raw = if k == 0 { floor } else { &platforms.unwrap()[k - 1] };
            match self.poly_pose(raw) {
                Some(c) => self.poly_capsule_contact(&b, c, k as i32, m.restitution, m.friction),
                None => {
                    let s = self.start_pose(raw, dt);
                    self.capsule_contact(&b, &s, k as i32, m.restitution, m.friction);
                }
            }
        }
        for i in 0..self.crows {
            let (nx, ny) = (self.cnx[i], self.cny[i]);
            self.ca2[i] = ftz(self.crx[i] * ny - self.cry[i] * nx);
            self.crz[i] = 0.0;
            let vn = ftz(ftz(nx * vx + ny * vy + self.ca2[i] * w) - self.ckn[i]);
            self.targets(i, vn, dt);
        }
        self.patches = 0;
        self.frows = 0;
        for i in 0..self.crows {
            if self.patches > 0 {
                let f = self.pfrom[self.patches - 1];
                if self.cst[i] == self.cst[f] && self.cnx[i] * self.cnx[f] + self.cny[i] * self.cny[f] >= SAME_NORMAL {
                    self.pto[self.patches - 1] = i + 1;
                    continue;
                }
            }
            if self.patches == MAX_PATCHES {
                break;
            }
            self.pfrom[self.patches] = i;
            self.pto[self.patches] = i + 1;
            self.patches += 1;
        }
        for p in 0..self.patches {
            let (n0, n1) = self.anchors(p);
            self.ffrom[p] = self.frows;
            let (nx, ny) = (self.cnx[self.pfrom[p]], self.cny[self.pfrom[p]]);
            for q in 0..2 {
                let i = if q == 1 { n1 } else { n0 };
                if i < 0 {
                    continue;
                }
                let i = i as usize;
                let r = self.frows;
                self.fx[r] = ny;
                self.fy[r] = -nx;
                self.fa2[r] = ftz(self.crx[i] * -nx - self.cry[i] * ny);
                self.f_ft[r] = 0.0;
                self.fkv[r] = ftz(ny * self.ckx[i] - nx * self.cky[i]);
                self.frows += 1;
            }
            self.fto[p] = self.frows;
        }
        let (mut mx, mut my) = (vx, vy);
        if self.crows > 0 {
            for it in 0..POS_ITERS + VEL_ITERS {
                let vel_phase = it >= POS_ITERS;
                let fric = POS_ITERS as isize - it as isize <= FRICTION_ITERS as isize;
                for p in 0..self.patches {
                    let mut sum_f = 0.0f32;
                    for i in self.pfrom[p]..self.pto[p] {
                        let (cnx, cny) = (self.cnx[i], self.cny[i]);
                        let vn = ftz(ftz(cnx * vx + cny * vy + self.ca2[i] * w) - self.ckn[i]);
                        let target = if vel_phase { self.ctu[i] } else { self.ctb[i] };
                        let mut d = ftz(target - vn);
                        let cf = self.c_f[i];
                        if d < -cf {
                            d = -cf;
                        }
                        self.c_f[i] = ftz(cf + d);
                        sum_f += self.c_f[i];
                        vx = ftz(vx + d * cnx);
                        vy = ftz(vy + d * cny);
                    }
                    if !fric {
                        continue;
                    }
                    let lim = ftz(self.cmu[self.pfrom[p]] * sum_f);
                    for r in self.ffrom[p]..self.fto[p] {
                        let (fx, fy) = (self.fx[r], self.fy[r]);
                        let u = ftz(ftz(fx * vx + fy * vy + self.fa2[r] * w) - self.fkv[r]);
                        let ft = self.f_ft[r];
                        let mut tot = ftz(ft - BIAS * u);
                        if tot > lim {
                            tot = lim;
                        } else if tot < -lim {
                            tot = -lim;
                        }
                        let df = ftz(tot - ft);
                        self.f_ft[r] = tot;
                        vx = ftz(vx + df * fx);
                        vy = ftz(vy + df * fy);
                    }
                }
                if it == POS_ITERS - 1 {
                    mx = vx;
                    my = vy;
                }
            }
        }
        b.x = ftz(b.x + ftz(mx * dt));
        b.y = ftz(b.y + ftz(my * dt));
        b.angle = ftz(b.angle + ftz(w * dt));
        b.vx = vx;
        b.vy = vy;
        b.wc = w;
        b.spin = ftz(w * r_);
        b.write(body);
        (body.angle, body.wc) = (out(b.angle), out(w));
        let mut sum = 0.0f32;
        for i in 0..self.crows {
            sum += self.c_f[i];
        }
        out(sum)
    }
}

/// A `WeldedBox` narrowed to float32.
#[derive(Clone, Copy, Debug)]
struct Welded {
    hx: f32,
    hy: f32,
    hd: f32,
    oy: f32,
    offset: f32,
    platforms: bool,
    restitution: f32,
    friction: f32,
}

impl Welded {
    fn of(w: &WeldedBox) -> Welded {
        Welded {
            hx: arg(w.hx),
            hy: arg(w.hy),
            hd: arg(w.hd),
            oy: arg(w.oy),
            offset: arg(w.offset),
            platforms: w.platforms,
            restitution: arg(w.restitution),
            friction: arg(w.friction),
        }
    }
}

/// sphereStep's default k: m R^2 / I of a lone sphere.
pub const DEFAULT_K: f64 = INERTIA_K as f64;

#[cfg(test)]
mod tests {
    use super::*;

    /// A sphere dropped on a square polygon comes to rest on it as on the box of the same sides.
    #[test]
    fn a_sphere_rests_on_a_polygon_as_on_its_box() {
        let g = Ground { left: -200.0, right: 200.0, bottom: -100.0, top: 0.0 };
        let poly =
            std::sync::Arc::new(Poly { xs: vec![-200.0, 200.0, 200.0, -200.0], ys: vec![-100.0, -100.0, 0.0, 0.0] });
        let as_box = Static { g, ..Static::default() };
        let as_poly = Static { g, poly: Some(PolyAt { poly, ox: 0.0, oy: 0.0 }), ..Static::default() };
        let set = Settings {
            linear_damping: 0.1,
            angular_damping: 0.1,
            restitution: 0.3,
            friction: 0.5,
            ..Settings::default()
        };
        let rest = |floor: &Static| {
            let mut b = Body { y: 100.0, radius: 20.0, ..Body::default() };
            for _ in 0..240 {
                Solver::default().sphere_step(&mut b, floor, None, 1.0 / 60.0, &set, DEFAULT_K);
            }
            b
        };
        let (a, p) = (rest(&as_box), rest(&as_poly));
        assert!((a.y - 20.0).abs() < 1.0, "on the box ({})", a.y);
        assert!(
            (p.y - a.y).abs() < 1.0 && p.x.abs() < 1.0,
            "on the polygon ({}, {}) as on the box ({})",
            p.x,
            p.y,
            a.y
        );
    }
}
