//! USplineMeshComponents sharing one static mesh, laid along their curves as one dynamic mesh in UE world space
//! (SplineMeshes: ForwardAxis X with the template's offsets and 2D scales,
//! and SplineMeshView: any axis with normals and tangents and draw groups; one part serves both here).
//!
//! Each vertex is placed by its component's slice transform (USplineMeshComponent::CalcSliceTransformAtSplineOffset,
//! which FSplineMeshVertexFactory's vertex shader repeats): Alpha along the forward axis, the position and direction
//! of the cubic Hermite curve at Alpha, BaseX = Normal(SplineUpDir ^ Dir), BaseY = Normal(Dir ^ BaseX), the roll about
//! the curve (XVec = cos x BaseX - sin x BaseY, YVec = cos x BaseY + sin x BaseX), the offset and scale lerped by
//! Alpha (bSmoothInterpRollScale off). The local axes' world directions: X (Dir, XVec, YVec) scaled (1, S.X, S.Y),
//! Y (YVec, Dir, XVec) scaled (S.Y, 1, S.X), Z (XVec, YVec, Dir) scaled (S.X, S.Y, 1), the forward coordinate itself
//! dropped; the vertex at Pos + Offset.X x BaseX + Offset.Y x BaseY + its other two coordinates on their axes. The
//! normal and tangent are turned by the slice's rotation alone (the vertex factory's SplineMeshCalcSliceRot).
//! Unverified: the slice transform and the normal rule are stock 4.26 source from memory; StartRoll / EndRoll are
//! taken in radians as the stock code does.

type V3 = [f64; 3];

/// One component's curve (UE world, cm) and its slice parameters (SplineParams).
#[derive(Clone, Copy, Debug)]
pub struct SplineSlice {
    pub a: V3,
    pub at: V3,
    pub b: V3,
    pub bt: V3,
    pub scale0: [f64; 2],
    pub scale1: [f64; 2],
    pub offset0: [f64; 2],
    pub offset1: [f64; 2],
    pub roll0: f64,
    pub roll1: f64,
    /// Its draw group (one material instance each: SplineMeshView's).
    pub group: usize,
}

impl SplineSlice {
    /// A curve with the template's defaults: scale 1, no offset, no roll, group 0.
    pub fn curve(a: V3, at: V3, b: V3, bt: V3) -> SplineSlice {
        SplineSlice {
            a,
            at,
            b,
            bt,
            scale0: [1.0; 2],
            scale1: [1.0; 2],
            offset0: [0.0; 2],
            offset1: [0.0; 2],
            roll0: 0.0,
            roll1: 0.0,
            group: 0,
        }
    }
}

/// How a vertex's Alpha is found from its forward coordinate.
#[derive(Clone, Copy, Debug)]
pub enum AlphaRule {
    /// SplineMeshes: the mesh's bounds at origin 0, `x x 0.5 / extent + 0.5`.
    Extent(f64),
    /// SplineMeshView: the vertices' own bounds, `(f - min) / (max - min)`.
    Bounds,
}

/// The mesh laid along the slices: its source vertices and the output streams.
#[derive(Clone, Debug)]
pub struct SplineMesh {
    axis: usize,
    pos: Vec<V3>,
    nrm: Vec<V3>,
    tan: Vec<V3>,
    alpha: Vec<f64>,
    index: Vec<u32>,
    /// The deformed vertices, a slice's run after another, in group order (UE world, cm).
    out_pos: Vec<[f32; 3]>,
    out_nrm: Vec<[f32; 3]>,
    out_tan: Vec<[f32; 3]>,
    out_index: Vec<u32>,
    /// Each group's run of `out_index`: (group, first index, count).
    groups: Vec<(usize, usize, usize)>,
}

impl SplineMesh {
    /// `pos` the mesh's vertices (UE local, cm), `nrm` / `tan` their normals and tangents (empty: none), `index` its
    /// triangles in the cooked order, `axis` the template's ForwardAxis (0 X, 1 Y, 2 Z).
    pub fn new(pos: Vec<V3>, nrm: Vec<V3>, tan: Vec<V3>, index: Vec<u32>, axis: usize, rule: AlphaRule) -> SplineMesh {
        let alpha = match rule {
            AlphaRule::Extent(e) => {
                let k = 0.5 / e;
                pos.iter().map(|p| p[axis] * k + 0.5).collect()
            }
            AlphaRule::Bounds => {
                let lo = pos.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min);
                let hi = pos.iter().map(|p| p[axis]).fold(f64::NEG_INFINITY, f64::max);
                let inv = if hi > lo { 1.0 / (hi - lo) } else { 0.0 };
                pos.iter().map(|p| (p[axis] - lo) * inv).collect()
            }
        };
        SplineMesh {
            axis,
            pos,
            nrm,
            tan,
            alpha,
            index,
            out_pos: Vec::new(),
            out_nrm: Vec::new(),
            out_tan: Vec::new(),
            out_index: Vec::new(),
            groups: Vec::new(),
        }
    }

    /// The template's ForwardAxis (`ESplineMeshAxis::X` unless it says Y or Z).
    pub fn axis_of(name: Option<&str>) -> usize {
        match name {
            Some("ESplineMeshAxis::Y") => 1,
            Some("ESplineMeshAxis::Z") => 2,
            _ => 0,
        }
    }

    /// Lays the mesh along each slice under the components' SplineUpDir in the world (`up`), the slices of each
    /// group together, the groups in order.
    pub fn update(&mut self, slices: &[SplineSlice], up: V3) {
        self.out_pos.clear();
        self.out_nrm.clear();
        self.out_tan.clear();
        self.out_index.clear();
        self.groups.clear();
        let n = self.pos.len();
        let last = slices.iter().map(|s| s.group).max();
        let Some(last) = last else { return };
        let mut s = 0u32;
        for grp in 0..=last {
            let from = self.out_index.len();
            for g in slices.iter().filter(|g| g.group == grp) {
                for k in 0..n {
                    self.vertex(g, k, up);
                }
                self.out_index.extend(self.index.iter().map(|i| i + s * n as u32));
                s += 1;
            }
            if self.out_index.len() > from {
                self.groups.push((grp, from, self.out_index.len() - from));
            }
        }
    }

    fn vertex(&mut self, g: &SplineSlice, k: usize, up: V3) {
        let a = self.alpha[k];
        let (a2, a3) = (a * a, a * a * a);
        let (h0, h1, h2, h3) = (2.0 * a3 - 3.0 * a2 + 1.0, a3 - 2.0 * a2 + a, a3 - a2, -2.0 * a3 + 3.0 * a2);
        let (d0, d1, d2, d3) = (6.0 * a2 - 6.0 * a, 3.0 * a2 - 4.0 * a + 1.0, 3.0 * a2 - 2.0 * a, -6.0 * a2 + 6.0 * a);
        let mut p: V3 = std::array::from_fn(|q| h0 * g.a[q] + h1 * g.at[q] + h2 * g.bt[q] + h3 * g.b[q]);
        let dir = safe(std::array::from_fn(|q| d0 * g.a[q] + d1 * g.at[q] + d2 * g.bt[q] + d3 * g.b[q]));
        let bx = safe(cross(up, dir));
        let by = safe(cross(dir, bx));
        let roll = g.roll0 + (g.roll1 - g.roll0) * a;
        let (sn, cs) = roll.sin_cos();
        let xv: V3 = std::array::from_fn(|q| cs * bx[q] - sn * by[q]);
        let yv: V3 = std::array::from_fn(|q| cs * by[q] + sn * bx[q]);
        let lerp = |u: [f64; 2], v: [f64; 2]| [u[0] + (v[0] - u[0]) * a, u[1] + (v[1] - u[1]) * a];
        let o = lerp(g.offset0, g.offset1);
        let sc = lerp(g.scale0, g.scale1);
        // the local axes' world directions and scales
        let (e, s) = match self.axis {
            0 => ([dir, xv, yv], [0.0, sc[0], sc[1]]),
            1 => ([yv, dir, xv], [sc[1], 0.0, sc[0]]),
            _ => ([xv, yv, dir], [sc[0], sc[1], 0.0]),
        };
        let v = self.pos[k];
        for q in 0..3 {
            p[q] += o[0] * bx[q] + o[1] * by[q] + v[0] * s[0] * e[0][q] + v[1] * s[1] * e[1][q] + v[2] * s[2] * e[2][q];
        }
        self.out_pos.push(p.map(|c| c as f32));
        let turn =
            |d: V3| -> [f32; 3] { std::array::from_fn(|q| (d[0] * e[0][q] + d[1] * e[1][q] + d[2] * e[2][q]) as f32) };
        if let Some(&d) = self.nrm.get(k) {
            self.out_nrm.push(turn(d));
        }
        if let Some(&d) = self.tan.get(k) {
            self.out_tan.push(turn(d));
        }
    }

    /// The deformed vertices (UE world, cm).
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.out_pos
    }

    /// The turned normals (empty when the mesh gave none).
    pub fn normals(&self) -> &[[f32; 3]] {
        &self.out_nrm
    }

    /// The turned tangents (empty when the mesh gave none).
    pub fn tangents(&self) -> &[[f32; 3]] {
        &self.out_tan
    }

    /// The triangles of every slice, in group order.
    pub fn indices(&self) -> &[u32] {
        &self.out_index
    }

    /// Each drawn group's run of `indices`: (group, first, count).
    pub fn groups(&self) -> &[(usize, usize, usize)] {
        &self.groups
    }

    /// The source vertex count (a slice's run of the output).
    pub fn vertices_per_slice(&self) -> usize {
        self.pos.len()
    }
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// FVector::GetSafeNormal: zero under a squared size of 1e-8.
fn safe(v: V3) -> V3 {
    let sq = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    if sq < 1e-8 {
        return [0.0; 3];
    }
    let l = sq.sqrt();
    [v[0] / l, v[1] / l, v[2] / l]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit box's eight corners, X from -50 to 50.
    fn box_mesh(rule: AlphaRule) -> SplineMesh {
        let mut pos = Vec::new();
        for x in [-50.0, 50.0] {
            for y in [-1.0, 1.0] {
                for z in [-2.0, 2.0] {
                    pos.push([x, y, z]);
                }
            }
        }
        SplineMesh::new(pos, Vec::new(), Vec::new(), vec![0, 1, 2], 0, rule)
    }

    #[test]
    fn straight_curve_keeps_the_mesh() {
        // (a curve from (0, 0, 0) to (100, 0, 0) with tangents 100: the mesh's own X from 0 to 100, up Z)
        for rule in [AlphaRule::Extent(50.0), AlphaRule::Bounds] {
            let mut m = box_mesh(rule);
            let t = [100.0, 0.0, 0.0];
            m.update(&[SplineSlice::curve([0.0; 3], t, [100.0, 0.0, 0.0], t)], [0.0, 0.0, 1.0]);
            let want = [[0.0, -1.0, -2.0], [0.0, -1.0, 2.0], [0.0, 1.0, -2.0], [0.0, 1.0, 2.0]];
            for (k, w) in want.iter().enumerate() {
                let p = m.positions()[k];
                // BaseX = Z ^ X = Y, BaseY = X ^ Y = Z
                assert!((0..3).all(|q| (f64::from(p[q]) - w[q]).abs() < 1e-4), "{k}: {p:?}");
                let p = m.positions()[k + 4];
                assert!((f64::from(p[0]) - 100.0).abs() < 1e-4, "{p:?}");
            }
        }
    }

    #[test]
    fn groups_and_scales() {
        let mut m = box_mesh(AlphaRule::Bounds);
        let t = [100.0, 0.0, 0.0];
        let mut a = SplineSlice::curve([0.0; 3], t, [100.0, 0.0, 0.0], t);
        a.group = 1;
        a.scale0 = [2.0, 2.0];
        a.scale1 = [2.0, 2.0];
        let b = SplineSlice::curve([0.0, 0.0, 10.0], t, [100.0, 0.0, 10.0], t);
        m.update(&[a, b], [0.0, 0.0, 1.0]);
        assert_eq!(m.groups(), &[(0, 0, 3), (1, 3, 3)]);
        assert_eq!(&m.indices()[3..], &[8, 9, 10]);
        // (group 0 first: b; then a at twice the size)
        assert!((f64::from(m.positions()[0][2]) - 8.0).abs() < 1e-4);
        assert!((f64::from(m.positions()[8][1]) + 2.0).abs() < 1e-4);
    }
}
