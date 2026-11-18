//! The views' matrix and quaternion math in the render form (three.js Matrix4 / Quaternion: column vectors, the
//! elements column-major, doubles), and the change between the view's space (metres, Y up) and UE's (cm, Z up).

/// A 4x4 matrix, column-major (three.js Matrix4.elements: `e[c * 4 + r]`), column vectors (p' = M p).
pub type Mat4 = [f64; 16];

pub const IDENTITY: Mat4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// Matrix4.compose(position, quaternion, scale).
pub fn compose(t: [f64; 3], q: [f64; 4], s: [f64; 3]) -> Mat4 {
    let [x, y, z, w] = q;
    let (x2, y2, z2) = (x + x, y + y, z + z);
    let (xx, xy, xz) = (x * x2, x * y2, x * z2);
    let (yy, yz, zz) = (y * y2, y * z2, z * z2);
    let (wx, wy, wz) = (w * x2, w * y2, w * z2);
    let [sx, sy, sz] = s;
    [
        (1.0 - (yy + zz)) * sx,
        (xy + wz) * sx,
        (xz - wy) * sx,
        0.0,
        (xy - wz) * sy,
        (1.0 - (xx + zz)) * sy,
        (yz + wx) * sy,
        0.0,
        (xz + wy) * sz,
        (yz - wx) * sz,
        (1.0 - (xx + yy)) * sz,
        0.0,
        t[0],
        t[1],
        t[2],
        1.0,
    ]
}

/// Matrix4.multiplyMatrices(a, b): a b.
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut o = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            o[c * 4 + r] =
                a[r] * b[c * 4] + a[4 + r] * b[c * 4 + 1] + a[8 + r] * b[c * 4 + 2] + a[12 + r] * b[c * 4 + 3];
        }
    }
    o
}

/// Matrix4.invert (the general inverse; zero for a singular matrix, as three.js gives).
pub fn invert(m: &Mat4) -> Mat4 {
    let (n11, n21, n31, n41) = (m[0], m[1], m[2], m[3]);
    let (n12, n22, n32, n42) = (m[4], m[5], m[6], m[7]);
    let (n13, n23, n33, n43) = (m[8], m[9], m[10], m[11]);
    let (n14, n24, n34, n44) = (m[12], m[13], m[14], m[15]);
    let t11 = n23 * n34 * n42 - n24 * n33 * n42 + n24 * n32 * n43 - n22 * n34 * n43 - n23 * n32 * n44 + n22 * n33 * n44;
    let t12 = n14 * n33 * n42 - n13 * n34 * n42 - n14 * n32 * n43 + n12 * n34 * n43 + n13 * n32 * n44 - n12 * n33 * n44;
    let t13 = n13 * n24 * n42 - n14 * n23 * n42 + n14 * n22 * n43 - n12 * n24 * n43 - n13 * n22 * n44 + n12 * n23 * n44;
    let t14 = n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33 + n12 * n24 * n33 + n13 * n22 * n34 - n12 * n23 * n34;
    let det = n11 * t11 + n21 * t12 + n31 * t13 + n41 * t14;
    if det == 0.0 {
        return [0.0; 16];
    }
    let d = 1.0 / det;
    [
        t11 * d,
        (n24 * n33 * n41 - n23 * n34 * n41 - n24 * n31 * n43 + n21 * n34 * n43 + n23 * n31 * n44 - n21 * n33 * n44) * d,
        (n22 * n34 * n41 - n24 * n32 * n41 + n24 * n31 * n42 - n21 * n34 * n42 - n22 * n31 * n44 + n21 * n32 * n44) * d,
        (n23 * n32 * n41 - n22 * n33 * n41 - n23 * n31 * n42 + n21 * n33 * n42 + n22 * n31 * n43 - n21 * n32 * n43) * d,
        t12 * d,
        (n13 * n34 * n41 - n14 * n33 * n41 + n14 * n31 * n43 - n11 * n34 * n43 - n13 * n31 * n44 + n11 * n33 * n44) * d,
        (n14 * n32 * n41 - n12 * n34 * n41 - n14 * n31 * n42 + n11 * n34 * n42 + n12 * n31 * n44 - n11 * n32 * n44) * d,
        (n12 * n33 * n41 - n13 * n32 * n41 + n13 * n31 * n42 - n11 * n33 * n42 - n12 * n31 * n43 + n11 * n32 * n43) * d,
        t13 * d,
        (n14 * n23 * n41 - n13 * n24 * n41 - n14 * n21 * n43 + n11 * n24 * n43 + n13 * n21 * n44 - n11 * n23 * n44) * d,
        (n12 * n24 * n41 - n14 * n22 * n41 + n14 * n21 * n42 - n11 * n24 * n42 - n12 * n21 * n44 + n11 * n22 * n44) * d,
        (n13 * n22 * n41 - n12 * n23 * n41 - n13 * n21 * n42 + n11 * n23 * n42 + n12 * n21 * n43 - n11 * n22 * n43) * d,
        t14 * d,
        (n13 * n24 * n31 - n14 * n23 * n31 + n14 * n21 * n33 - n11 * n24 * n33 - n13 * n21 * n34 + n11 * n23 * n34) * d,
        (n14 * n22 * n31 - n12 * n24 * n31 - n14 * n21 * n32 + n11 * n24 * n32 + n12 * n21 * n34 - n11 * n22 * n34) * d,
        (n12 * n23 * n31 - n13 * n22 * n31 + n13 * n21 * n32 - n11 * n23 * n32 - n12 * n21 * n33 + n11 * n22 * n33) * d,
    ]
}

// UE axis i is the view's axis AXIS[i]: three (x, y, z) = UE (X, Z, Y) x 0.01.
const AXIS: [usize; 3] = [0, 2, 1];

/// U⁻¹ M U, U = UE_TO_RENDER: a transform of the view's space as the same transform of UE's (the linear part with
/// its axes exchanged, the translation in cm).
pub fn to_ue(m: &Mat4) -> Mat4 {
    let mut o = IDENTITY;
    for c in 0..3 {
        for r in 0..3 {
            o[c * 4 + r] = m[AXIS[c] * 4 + AXIS[r]];
        }
        o[12 + c] = m[12 + AXIS[c]] * 100.0;
    }
    o
}

/// The top three rows of an affine matrix, each with its translation as the fourth value: UE's FMatrix3x4 of the
/// row-vector matrix (its three columns).
pub fn rows3x4(m: &Mat4) -> [f32; 12] {
    let mut o = [0.0; 12];
    for r in 0..3 {
        for c in 0..4 {
            o[r * 4 + c] = m[c * 4 + r] as f32;
        }
    }
    o
}

/// Quaternion.multiplyQuaternions(a, b): a b.
pub fn qmul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        ax * bw + aw * bx + ay * bz - az * by,
        ay * bw + aw * by + az * bx - ax * bz,
        az * bw + aw * bz + ax * by - ay * bx,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// `rotatorQuat`: FRotator(pitch, yaw, roll) degrees as UE's quaternion.
pub fn rotator_quat(p: f64, y: f64, r: f64) -> [f64; 4] {
    let d = std::f64::consts::PI / 360.0;
    let (sp, cp) = ((p * d).sin(), (p * d).cos());
    let (sy, cy) = ((y * d).sin(), (y * d).cos());
    let (sr, cr) = ((r * d).sin(), (r * d).cos());
    [
        cr * sp * sy - sr * cp * cy,
        -cr * sp * cy - sr * cp * sy,
        cr * cp * sy - sr * sp * cy,
        cr * cp * cy + sr * sp * sy,
    ]
}

/// `gq`: a UE rotator as the view's quaternion (x, z, y, -w).
pub fn gq(r: [f64; 3]) -> [f64; 4] {
    let q = rotator_quat(r[0], r[1], r[2]);
    [q[0], q[2], q[1], -q[3]]
}

/// `gv`: a UE vector as the view's, (X, Z, Y) x k.
pub fn gv(v: [f64; 3], k: f64) -> [f64; 3] {
    [v[0] * k, v[2] * k, v[1] * k]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invert_undoes_compose() {
        let m = compose([1.0, -2.0, 3.0], [0.1, 0.2, 0.3, (1.0f64 - 0.14).sqrt()], [1.5, 0.5, 2.0]);
        let p = mul(&m, &invert(&m));
        for (i, v) in p.iter().enumerate() {
            assert!((v - IDENTITY[i]).abs() < 1e-12, "{i}: {v}");
        }
    }

    #[test]
    fn to_ue_of_ue_to_render_is_the_identity_change() {
        // U itself: three (x, y, z) = UE (X, Z, Y) x 0.01; U⁻¹ U U = U, so to_ue(U) is U's own matrix in UE terms
        let u = [0.01, 0.0, 0.0, 0.0, 0.0, 0.0, 0.01, 0.0, 0.0, 0.01, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        assert_eq!(to_ue(&u), u);
        let t = compose([0.01, 0.02, 0.03], [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0]);
        let o = to_ue(&t);
        assert_eq!([o[12], o[13], o[14]], [1.0, 3.0, 2.0]);
    }
}
