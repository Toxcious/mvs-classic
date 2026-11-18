//! UCableComponent (the stock UE 4.26 CableComponent plugin): a chain of
//! NumSegments + 1 particles between the component and the end attachment, Verlet-integrated in fixed substeps with
//! distance (and stiffness) constraints, drawn as a tube of NumSides faces (FCableSceneProxy BuildCableMesh).
//! Unverified: taken from the 4.26 source from memory, not read from the exe (TickComponent, PerformSubstep,
//! VerletIntegrate, SolveConstraints, BuildCableMesh; SubstepTime 0.02, CableWidth 10, NumSides 4 and TileMaterial 1
//! are the class defaults, absent from the component's dump); the world gravity is taken as -980.
//!
//! The tube's positions are in UE world space (cm); its UVs and index list are fixed when it is made. A caller ticks
//! it once a world tick with its two ends' places, pins the ends at a drawn frame's places and builds the tube.

use mvs_data::Value;

type V3 = [f64; 3];

const WORLD_GRAVITY_Z: f64 = -980.0;

/// The component's properties the cable reads (None: the class default).
#[derive(Clone, Debug, Default)]
pub struct CableDef {
    pub cable_length: Option<f64>,
    pub num_segments: Option<usize>,
    pub solver_iterations: Option<usize>,
    pub enable_stiffness: Option<bool>,
    pub gravity_scale: Option<f64>,
    pub substep_time: Option<f64>,
    pub cable_width: Option<f64>,
    pub num_sides: Option<usize>,
    pub tile_material: Option<f64>,
}

impl CableDef {
    /// From a component's dumped properties (`actorDef(...).components.Cable`).
    pub fn of(v: Option<&Value>) -> CableDef {
        let o = v.and_then(Value::as_obj);
        let f = |k: &str| o.and_then(|o| o.get(k)).and_then(Value::as_f64);
        let n = |k: &str| f(k).map(|x| x.max(0.0) as usize);
        CableDef {
            cable_length: f("CableLength"),
            num_segments: n("NumSegments"),
            solver_iterations: n("SolverIterations"),
            enable_stiffness: o.and_then(|o| o.get("bEnableStiffness")).and_then(Value::as_bool),
            gravity_scale: f("CableGravityScale"),
            substep_time: f("SubstepTime"),
            cable_width: f("CableWidth"),
            num_sides: n("NumSides"),
            tile_material: f("TileMaterial"),
        }
    }
}

/// One cable: its particles and its tube.
#[derive(Clone, Debug)]
pub struct Cable {
    /// Segments.
    n: usize,
    sides: usize,
    /// UE world, a particle each.
    pos: Vec<V3>,
    old: Vec<V3>,
    local: Vec<V3>,
    seg_len: f64,
    iters: usize,
    stiff: bool,
    gz: f64,
    substep: f64,
    width: f64,
    rem: f64,
    started: bool,
    /// The tube: (NumSegments + 1) rings of NumSides + 1 vertices, UE world.
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl Cable {
    pub fn new(d: &CableDef) -> Cable {
        let n = d.num_segments.unwrap_or(10).max(1);
        let sides = d.num_sides.unwrap_or(4).max(1);
        let tile = d.tile_material.unwrap_or(1.0);
        let ring = sides + 1;
        let mut uvs = Vec::with_capacity((n + 1) * ring);
        for p in 0..=n {
            for v in 0..ring {
                uvs.push([(p as f64 / n as f64 * tile) as f32, (v as f64 / sides as f64) as f32]);
            }
        }
        let mut indices = Vec::with_capacity(n * sides * 6);
        for s in 0..n {
            for k in 0..sides {
                let tl = (s * ring + k) as u32;
                let bl = tl + 1;
                let tr = ((s + 1) * ring + k) as u32;
                let br = tr + 1;
                indices.extend_from_slice(&[tl, bl, tr, tr, bl, br]);
            }
        }
        Cable {
            n,
            sides,
            pos: vec![[0.0; 3]; n + 1],
            old: vec![[0.0; 3]; n + 1],
            local: vec![[0.0; 3]; n + 1],
            seg_len: d.cable_length.unwrap_or(100.0) / n as f64,
            iters: d.solver_iterations.unwrap_or(1),
            stiff: d.enable_stiffness.unwrap_or(false),
            gz: WORLD_GRAVITY_Z * d.gravity_scale.unwrap_or(1.0),
            substep: d.substep_time.unwrap_or(0.02).max(0.005),
            width: d.cable_width.unwrap_or(10.0),
            rem: 0.0,
            started: false,
            positions: vec![[0.0; 3]; (n + 1) * ring],
            uvs,
            indices,
        }
    }

    /// OnRegister: the particles lie on the line from the start to the end (the end is the component itself until
    /// SetAttachEndToComponent, so all of them start at the component).
    fn init(&mut self, start: V3) {
        self.pos.fill(start);
        self.old.fill(start);
        self.started = true;
    }

    /// TickComponent: the two attached ends are placed, then whole substeps of the accumulated time are run.
    pub fn tick(&mut self, dt: f64, start: V3, end: V3) {
        if !self.started {
            self.init(start);
        }
        let e = self.n;
        self.pos[0] = start;
        self.old[0] = start;
        self.pos[e] = end;
        self.old[e] = end;
        self.rem += dt;
        while self.rem > self.substep {
            self.verlet(self.substep);
            self.solve();
            self.rem -= self.substep;
        }
    }

    /// The two attached ends moved to a drawn frame's places between ticks (no substep run): the line's ends stay
    /// on the drawn actor and socket.
    pub fn pin(&mut self, start: V3, end: V3) {
        if !self.started {
            return;
        }
        let e = self.n;
        self.pos[0] = start;
        self.pos[e] = end;
    }

    fn verlet(&mut self, h: f64) {
        let h2 = h * h;
        for i in 1..self.n {
            for k in 0..3 {
                let p = self.pos[i][k];
                let np = p + (p - self.old[i][k]) + h2 * if k == 2 { self.gz } else { 0.0 };
                self.old[i][k] = p;
                self.pos[i][k] = np;
            }
        }
    }

    fn solve(&mut self) {
        for _ in 0..self.iters {
            for s in 0..self.n {
                self.distance(s, s + 1, self.seg_len);
            }
            if self.stiff {
                for s in 0..self.n.saturating_sub(1) {
                    self.distance(s, s + 2, 2.0 * self.seg_len);
                }
            }
        }
    }

    /// SolveDistanceConstraint; the end particles are not free. Two coincident particles are left alone (the stock
    /// code divides by the zero distance).
    fn distance(&mut self, a: usize, b: usize, want: f64) {
        let (pa, pb) = (self.pos[a], self.pos[b]);
        let d = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
        let cur = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if cur == 0.0 {
            return;
        }
        let err = (cur - want) / cur;
        let af = a != 0 && a != self.n;
        let bf = b != 0 && b != self.n;
        let (wa, wb) = match (af, bf) {
            (true, true) => (0.5, 0.5),
            (true, false) => (1.0, 0.0),
            (false, true) => (0.0, 1.0),
            (false, false) => (0.0, 0.0),
        };
        for (k, dk) in d.iter().enumerate() {
            self.pos[a][k] += err * wa * dk;
            self.pos[b][k] -= err * wb * dk;
        }
    }

    /// BuildCableMesh on the particles in the component's space (world to local by the inverse component
    /// transform), then back to the world: the ring's CableWidth is in local units, so the component's scale
    /// stretches it. `rot`: the component's rotation rows (the world directions of its X, Y and Z), `scale` its
    /// scale, `origin` its location.
    pub fn build(&mut self, origin: V3, rot: &[f64; 9], scale: V3) {
        let n = self.n;
        let ring = self.sides + 1;
        for i in 0..=n {
            let p = self.pos[i];
            let d = [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
            for k in 0..3 {
                self.local[i][k] = (d[0] * rot[k * 3] + d[1] * rot[k * 3 + 1] + d[2] * rot[k * 3 + 2]) / scale[k];
            }
        }
        for i in 0..=n {
            let (a, b) = (self.local[i.saturating_sub(1)], self.local[(i + 1).min(n)]);
            let mut f = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let fl = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
            if fl > 1e-4 {
                f = [f[0] / fl, f[1] / fl, f[2] / fl];
            } else {
                f = [0.0; 3];
            }
            // FQuat::FindBetween((0, -1, 0), ForwardDir), then RightDir = q (0, 0, 1) and UpDir = q (1, 0, 0).
            let mut q = [-f[2], 0.0, f[0], 1.0 - f[1]]; // cross(A, B), 1 + dot(A, B)
            if q[3] < 1e-6 {
                // Opposite vectors: FindBetween's fallback axis for A = (0, -1, 0) is (0, -A.Z, A.Y) = (0, 0, -1), W 0.
                q = [0.0, 0.0, -1.0, 0.0];
            }
            let ql = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
            let q = [q[0] / ql, q[1] / ql, q[2] / ql, q[3] / ql];
            let right = rotate(q, [0.0, 0.0, 1.0]);
            let up = rotate(q, [1.0, 0.0, 0.0]);
            let l = self.local[i];
            for v in 0..ring {
                let ang = 2.0 * std::f64::consts::PI * v as f64 / self.sides as f64;
                let c = ang.cos() * 0.5 * self.width;
                let s = ang.sin() * 0.5 * self.width;
                let lv: V3 = std::array::from_fn(|k| (l[k] + c * up[k] + s * right[k]) * scale[k]);
                let w: V3 =
                    std::array::from_fn(|k| origin[k] + lv[0] * rot[k] + lv[1] * rot[3 + k] + lv[2] * rot[6 + k]);
                self.positions[i * ring + v] = [w[0] as f32, w[1] as f32, w[2] as f32];
            }
        }
    }

    /// The tube's vertices as the last `build` placed them (UE world, cm).
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// The tube's UVs: (the particle / NumSegments x TileMaterial, the side / NumSides).
    pub fn uvs(&self) -> &[[f32; 2]] {
        &self.uvs
    }

    /// The tube's triangles.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// The particles (UE world, cm).
    pub fn particles(&self) -> &[V3] {
        &self.pos
    }
}

/// FQuat::RotateVector: v + 2 w (q x v) + 2 q x (q x v).
fn rotate(q: [f64; 4], v: V3) -> V3 {
    let [qx, qy, qz, qw] = q;
    let [x, y, z] = v;
    let tx = 2.0 * (qy * z - qz * y);
    let ty = 2.0 * (qz * x - qx * z);
    let tz = 2.0 * (qx * y - qy * x);
    [x + qw * tx + (qy * tz - qz * ty), y + qw * ty + (qz * tx - qx * tz), z + qw * tz + (qx * ty - qy * tx)]
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

    #[test]
    fn hangs_between_ends() {
        let mut c = Cable::new(&CableDef::default());
        for _ in 0..600 {
            c.tick(1.0 / 60.0, [0.0, 0.0, 0.0], [50.0, 0.0, 0.0]);
        }
        let p = c.particles();
        assert_eq!(p[0], [0.0; 3]);
        assert_eq!(p[10], [50.0, 0.0, 0.0]);
        // (100 of cable over 50: the middle sags under gravity, on the plane of the ends)
        assert!(p[5][2] < -10.0, "{:?}", p[5]);
        assert!(p[5][1].abs() < 1e-9);
    }

    #[test]
    fn tube_rings_around_a_straight_line() {
        let mut c = Cable::new(&CableDef { gravity_scale: Some(0.0), cable_length: Some(10.0), ..CableDef::default() });
        c.tick(0.0, [0.0, 0.0, 0.0], [0.0, -10.0, 0.0]);
        // (the particles start at the start: a few substeps lay them on the line)
        for _ in 0..200 {
            c.tick(0.03, [0.0, 0.0, 0.0], [0.0, -10.0, 0.0]);
        }
        c.build([0.0; 3], &ID, [1.0; 3]);
        assert_eq!(c.positions().len(), 11 * 5);
        assert_eq!(c.indices().len(), 10 * 4 * 6);
        // (a forward of -Y: the identity quaternion, up X and right Z; radius CableWidth / 2)
        for v in &c.positions()[..5] {
            let r = (f64::from(v[0]).powi(2) + f64::from(v[2]).powi(2)).sqrt();
            assert!((r - 5.0).abs() < 1e-4, "{v:?}");
        }
        assert!((f64::from(c.positions()[0][0]) - 5.0).abs() < 1e-5);
        assert_eq!(c.uvs()[5 * 10 + 4], [1.0, 1.0]);
    }
}
