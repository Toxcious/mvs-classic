//! The sweep of one gathered body (sweepBody, with hull, hullPoly, chain, closest
//! and cast). The gathering of the bodies off the world (gather) and the hits' sort (sweepAll) stay in the sim. The
//! port's own geometry standing in for PhysX's sweeps, in float32 in a fixed order of operations.

use crate::{arg, out};

/// The query shape: a box of half extents (qx, qy) rounded by qr, qd its half extent along the depth.
#[derive(Clone, Copy, Debug, Default)]
pub struct Query {
    pub x0: f64,
    pub y0: f64,
    pub dx: f64,
    pub dy: f64,
    pub qx: f64,
    pub qy: f64,
    pub qr: f64,
    pub qd: f64,
}

/// The parts of a TraceBody the sweep reads: its core segment, half extents, rounding, depth (NaN: extruded) and a
/// stage solid's polygon corners (x y after one another).
#[derive(Clone, Debug, Default)]
pub struct Body {
    pub ax: f64,
    pub ay: f64,
    pub bx: f64,
    pub by: f64,
    pub hx: f64,
    pub hy: f64,
    pub r: f64,
    pub z: f64,
    pub poly: Option<Vec<f64>>,
    // What the hit carries over from the body: the object type query index, the collision profile and component
    // names (ids here), the actor and floor (ids, -1 for none) and a stage platform's index (-1 for none).
    pub object_type: u32,
    pub profile: u32,
    pub component: u32,
    pub actor: i32,
    pub platform: i32,
    pub floor: i32,
}

/// A TraceHit, its object references as ids.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hit {
    pub time: f64,
    pub distance: f64,
    pub blocking: bool,
    pub initial: bool,
    pub x: f64,
    pub y: f64,
    pub ix: f64,
    pub iy: f64,
    pub nx: f64,
    pub ny: f64,
    pub depth: f64,
    pub object_type: u32,
    pub profile: u32,
    pub component: u32,
    pub actor: i32,
    pub platform: i32,
    pub floor: i32,
}

/// A `Query` narrowed to float32.
#[derive(Clone, Copy, Debug)]
struct Q {
    x0: f32,
    y0: f32,
    dx: f32,
    dy: f32,
    qx: f32,
    qy: f32,
    qr: f32,
    qd: f32,
}

impl Q {
    fn of(q: &Query) -> Q {
        Q {
            x0: arg(q.x0),
            y0: arg(q.y0),
            dx: arg(q.dx),
            dy: arg(q.dy),
            qx: arg(q.qx),
            qy: arg(q.qy),
            qr: arg(q.qr),
            qd: arg(q.qd),
        }
    }
}

// SweepSingleByChannel 14294e050: a sphere of radius 1e-4 or less is a line trace.
const LINE_RADIUS: f32 = 1e-4;

#[derive(Clone, Copy, Debug, Default)]
struct Close {
    x: f32,
    y: f32,
    d: f32,
    inside: bool,
    nx: f32,
    ny: f32,
    s: f32,
}

/// A body's geometry as the sweep reads it, borrowed: the core segment (a, b), the half extents, the rounding, the
/// depth (NaN: extruded) and a stage solid's polygon corners.
#[derive(Clone, Copy, Debug)]
pub struct Shape<'a> {
    pub ax: f64,
    pub ay: f64,
    pub bx: f64,
    pub by: f64,
    pub hx: f64,
    pub hy: f64,
    pub r: f64,
    pub z: f64,
    pub poly: Option<&'a [f64]>,
}

/// The module's scratch (VX, VY, NV, PX, PY, ORD, NP, CLOSE, HIT_N): kept module-wide in the sim, carried here.
#[derive(Clone, Debug)]
pub struct Tracer {
    vx: Vec<f32>,
    vy: Vec<f32>,
    nv: usize,
    px: Vec<f32>,
    py: Vec<f32>,
    ord: Vec<usize>,
    np: usize,
    close: Close,
    hit_n: (f32, f32),
}

impl Default for Tracer {
    fn default() -> Self {
        Tracer {
            vx: vec![0.0; 8],
            vy: vec![0.0; 8],
            nv: 0,
            px: vec![0.0; 8],
            py: vec![0.0; 8],
            ord: vec![0; 8],
            np: 0,
            close: Close { ny: 1.0, ..Close::default() },
            hit_n: (0.0, 1.0),
        }
    }
}

impl Tracer {
    fn put(&mut self, x: f32, y: f32) {
        for i in 0..self.np {
            if self.px[i] == x && self.py[i] == y {
                return;
            }
        }
        self.px[self.np] = x;
        self.py[self.np] = y;
        self.np += 1;
    }

    fn hull_poly(&mut self, c: &[f64], ox: f32, oy: f32, hx: f32, hy: f32) {
        let m = (c.len() / 2) * 4;
        if self.px.len() < m {
            self.px = vec![0.0; m];
            self.py = vec![0.0; m];
            self.vx = vec![0.0; 2 * m];
            self.vy = vec![0.0; 2 * m];
            self.ord = vec![0; m];
        }
        self.np = 0;
        let mut i = 0;
        while i < c.len() {
            let (x, y) = (arg(c[i]) + ox, arg(c[i + 1]) + oy);
            self.put(x - hx, y - hy);
            self.put(x + hx, y - hy);
            self.put(x + hx, y + hy);
            self.put(x - hx, y + hy);
            i += 2;
        }
        self.chain(self.np);
    }

    // hull_poly over corners given as two lists (a stage solid's `solids::Poly`).
    fn hull_xy(&mut self, xs: &[f64], ys: &[f64], ox: f32, oy: f32, hx: f32, hy: f32) {
        let m = xs.len() * 4;
        if self.px.len() < m {
            self.px = vec![0.0; m];
            self.py = vec![0.0; m];
            self.vx = vec![0.0; 2 * m];
            self.vy = vec![0.0; 2 * m];
            self.ord = vec![0; m];
        }
        self.np = 0;
        for (&x, &y) in xs.iter().zip(ys) {
            let (x, y) = (arg(x) + ox, arg(y) + oy);
            self.put(x - hx, y - hy);
            self.put(x + hx, y - hy);
            self.put(x + hx, y + hy);
            self.put(x - hx, y + hy);
        }
        self.chain(self.np);
    }

    #[allow(clippy::too_many_arguments)]
    fn hull(&mut self, ax: f32, ay: f32, bx: f32, by: f32, hx: f32, hy: f32) {
        self.np = 0;
        self.put(ax - hx, ay - hy);
        self.put(ax + hx, ay - hy);
        self.put(ax + hx, ay + hy);
        self.put(ax - hx, ay + hy);
        self.put(bx - hx, by - hy);
        self.put(bx + hx, by - hy);
        self.put(bx + hx, by + hy);
        self.put(bx - hx, by + hy);
        self.chain(self.np);
    }

    fn chain(&mut self, n: usize) {
        if n <= 2 {
            for i in 0..n {
                self.vx[i] = self.px[i];
                self.vy[i] = self.py[i];
            }
            self.nv = n;
            return;
        }
        // The points by x, then y (an insertion sort).
        let (px, py) = (&self.px, &self.py);
        let ord = &mut self.ord;
        for i in 0..n {
            let mut k = i;
            while k > 0 && (px[ord[k - 1]] > px[i] || (px[ord[k - 1]] == px[i] && py[ord[k - 1]] > py[i])) {
                ord[k] = ord[k - 1];
                k -= 1;
            }
            ord[k] = i;
        }
        // Andrew's monotone chain: the hull's last two vertices against a point (cross reads VX / VY at o, a and the
        // point at b).
        let (vx, vy) = (&mut self.vx, &mut self.vy);
        let cross = |vx: &[f32], vy: &[f32], o: usize, a: usize, b: usize| {
            (vx[a] - vx[o]) * (py[b] - vy[o]) - (vy[a] - vy[o]) * (px[b] - vx[o])
        };
        let mut k = 0usize;
        for &i in ord.iter().take(n) {
            while k >= 2 && cross(vx, vy, k - 2, k - 1, i) <= 0.0 {
                k -= 1;
            }
            vx[k] = px[i];
            vy[k] = py[i];
            k += 1;
        }
        let lower = k + 1;
        for m in (0..n - 1).rev() {
            let i = ord[m];
            while k >= lower && cross(vx, vy, k - 2, k - 1, i) <= 0.0 {
                k -= 1;
            }
            vx[k] = px[i];
            vy[k] = py[i];
            k += 1;
        }
        self.nv = k - 1;
    }

    fn closest(&mut self, x: f32, y: f32) {
        let nv = self.nv;
        let (vx, vy) = (&self.vx, &self.vy);
        let c = &mut self.close;
        let (mut best, mut inside, mut bs) = (f32::INFINITY, nv >= 3, f32::NEG_INFINITY);
        for i in 0..nv {
            let j = if nv == 1 { i } else { (i + 1) % nv };
            let (ex, ey) = (vx[j] - vx[i], vy[j] - vy[i]);
            let l2 = ex * ex + ey * ey;
            let mut t = if l2 > 0.0 { ((x - vx[i]) * ex + (y - vy[i]) * ey) / l2 } else { 0.0 };
            t = if t < 0.0 {
                0.0
            } else if t > 1.0 {
                1.0
            } else {
                t
            };
            let (cx, cy) = (vx[i] + ex * t, vy[i] + ey * t);
            let d2 = (x - cx) * (x - cx) + (y - cy) * (y - cy);
            if d2 < best {
                best = d2;
                c.x = cx;
                c.y = cy;
            }
            if nv >= 3 && l2 > 0.0 {
                let l = l2.sqrt();
                let (nx, ny) = (ey / l, -ex / l);
                let s = (x - vx[i]) * nx + (y - vy[i]) * ny;
                if s > 0.0 {
                    inside = false;
                }
                if s > bs {
                    bs = s;
                    c.nx = nx;
                    c.ny = ny;
                }
            }
        }
        c.inside = inside;
        c.s = bs;
        c.d = if inside { 0.0 } else { best.sqrt() };
        if inside {
            c.x = x;
            c.y = y;
        }
    }

    fn cast(&mut self, x0: f32, y0: f32, dx: f32, dy: f32, rho: f32) -> f32 {
        let nv = self.nv;
        let (vx, vy) = (&self.vx, &self.vy);
        let mut best = -1.0f32;
        let mut hit_n = self.hit_n;
        let mut try_ = |t: f32, nx: f32, ny: f32| {
            if (0.0..=1.0).contains(&t) && (best < 0.0 || t < best) {
                best = t;
                hit_n = (nx, ny);
            }
        };
        for i in 0..nv {
            if nv >= 2 {
                let j = (i + 1) % nv;
                let (ex, ey) = (vx[j] - vx[i], vy[j] - vy[i]);
                let l = (ex * ex + ey * ey).sqrt();
                if l > 0.0 {
                    let (nx, ny) = (ey / l, -ex / l);
                    let dn = dx * nx + dy * ny;
                    if dn < 0.0 {
                        let t = (rho - ((x0 - vx[i]) * nx + (y0 - vy[i]) * ny)) / dn;
                        let px = x0 + dx * t - rho * nx - vx[i];
                        let py = y0 + dy * t - rho * ny - vy[i];
                        let u = (px * ex + py * ey) / (l * l);
                        if (0.0..=1.0).contains(&u) {
                            try_(t, nx, ny);
                        }
                    }
                }
            }
            if rho > 0.0 {
                let (fx, fy) = (x0 - vx[i], y0 - vy[i]);
                let (a, b, c) = (dx * dx + dy * dy, fx * dx + fy * dy, fx * fx + fy * fy - rho * rho);
                if a > 0.0 && b < 0.0 {
                    let disc = b * b - a * c;
                    if disc >= 0.0 {
                        let t = (-b - disc.sqrt()) / a;
                        try_(t, (fx + dx * t) / rho, (fy + dy * t) / rho);
                    }
                }
            }
        }
        self.hit_n = hit_n;
        best
    }

    /// sweepBody: the sweep of the query against body b, a hit or None.
    pub fn sweep_body(&mut self, q: &Query, b: &Body) -> Option<Hit> {
        let s = Shape {
            ax: b.ax,
            ay: b.ay,
            bx: b.bx,
            by: b.by,
            hx: b.hx,
            hy: b.hy,
            r: b.r,
            z: b.z,
            poly: b.poly.as_deref(),
        };
        let h = self.sweep(q, &s)?;
        Some(Hit {
            object_type: b.object_type,
            profile: b.profile,
            component: b.component,
            actor: b.actor,
            platform: b.platform,
            floor: b.floor,
            ..h
        })
    }

    /// sweepBody over a borrowed shape: the hit's geometry, its body fields 0.
    pub fn sweep(&mut self, q: &Query, b: &Shape<'_>) -> Option<Hit> {
        let q = Q::of(q);
        let (bz, br) = (arg(b.z), arg(b.r));
        let mut rho = br + q.qr;
        if !bz.is_nan() {
            let dz = crate::max(bz.abs() - q.qd, 0.0);
            if dz > rho {
                return None;
            }
            rho = (rho * rho - dz * dz).sqrt();
        }
        match b.poly {
            Some(p) => self.hull_poly(p, arg(b.ax), arg(b.ay), q.qx, q.qy),
            None => self.hull(arg(b.ax), arg(b.ay), arg(b.bx), arg(b.by), arg(b.hx) + q.qx, arg(b.hy) + q.qy),
        }
        self.hull_hit(&q, rho)
    }

    /// [`Tracer::sweep`] of the query against a convex polygon given as its corners' two lists moved by (ox, oy), its
    /// depth extruded: a stage solid's section (`solids::Poly`) met by a reader of the stage's boxes.
    pub fn sweep_corners(
        &mut self,
        q: &Query,
        xs: &[f64],
        ys: &[f64],
        ox: impl Into<f64>,
        oy: impl Into<f64>,
    ) -> Option<Hit> {
        let q = Q::of(q);
        self.hull_xy(xs, ys, arg(ox), arg(oy), q.qx, q.qy);
        self.hull_hit(&q, q.qr)
    }

    // The sweep against the hull made (grown by the query's half extents), rounded by rho.
    fn hull_hit(&mut self, q: &Q, rho: f32) -> Option<Hit> {
        self.closest(q.x0, q.y0);
        let (t, nx, ny);
        let (mut depth, mut cx, mut cy) = (0.0f32, q.x0, q.y0);
        let c = self.close;
        let initial = c.d <= rho;
        if initial {
            t = 0.0;
            if c.d > 1e-6 {
                nx = (q.x0 - c.x) / c.d;
                ny = (q.y0 - c.y) / c.d;
                depth = rho - c.d;
            } else if c.inside {
                nx = c.nx;
                ny = c.ny;
                depth = rho - c.s;
            } else {
                // On a segment or a point: against the sweep.
                let l = (q.dx * q.dx + q.dy * q.dy).sqrt();
                nx = if l > 0.0 { -q.dx / l } else { 0.0 };
                ny = if l > 0.0 { -q.dy / l } else { 1.0 };
                depth = rho;
            }
        } else {
            t = self.cast(q.x0, q.y0, q.dx, q.dy, rho);
            if t < 0.0 {
                return None;
            }
            cx = q.x0 + q.dx * t;
            cy = q.y0 + q.dy * t;
            self.closest(cx, cy);
            let c = self.close;
            if c.d > 1e-6 {
                nx = (cx - c.x) / c.d;
                ny = (cy - c.y) / c.d;
            } else {
                (nx, ny) = self.hit_n;
            }
        }
        let sgn = |v: f32| -> f32 {
            if v > 1e-6 {
                1.0
            } else if v < -1e-6 {
                -1.0
            } else {
                0.0
            }
        };
        let (sx, sy) = (sgn(nx), sgn(ny));
        let line = q.qr <= LINE_RADIUS && q.qx == 0.0 && q.qy == 0.0;
        let ix = if !initial {
            cx - nx * q.qr - sx * q.qx
        } else if line {
            cx
        } else {
            cx - nx * (q.qr - depth) - sx * q.qx
        };
        let iy = if !initial {
            cy - ny * q.qr - sy * q.qy
        } else if line {
            cy
        } else {
            cy - ny * (q.qr - depth) - sy * q.qy
        };
        Some(Hit {
            time: out(t),
            distance: out(t * (q.dx * q.dx + q.dy * q.dy).sqrt()),
            blocking: true,
            initial,
            x: out(cx),
            y: out(cy),
            ix: out(ix),
            iy: out(iy),
            nx: out(nx),
            ny: out(ny),
            depth: out(depth),
            ..Hit::default()
        })
    }
}
