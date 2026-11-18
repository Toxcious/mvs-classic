//! The vector helpers and rotation rows (FRotationMatrix, a camera-facing mesh particle's) the player shares.
//! Every routine goes through mvs-math, so the numbers come out with the same bits as JavaScript's double math.

use mvs_math::hypot::{hypot2, hypot3};
use mvs_math::libm::{atan2, cos, sin};

pub type V3 = [f64; 3];
/// A rotation's rows: the X, Y and Z axes, three numbers each.
pub type Rows = [f64; 9];

pub const ZERO3: V3 = [0.0, 0.0, 0.0];
pub const UNIT_X: V3 = [1.0, 0.0, 0.0];
pub const UNIT_Z: V3 = [0.0, 0.0, 1.0];
pub const ONE3: V3 = [1.0, 1.0, 1.0];
pub const IDENTITY: Rows = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// FVector::GetSafeNormal: unchanged unless |v|^2 > 1e-8.
pub fn safe_normal(v: V3) -> V3 {
    let [x, y, z] = v;
    let l2 = x * x + y * y + z * z;
    if !(l2 > 1e-8) {
        return v;
    }
    let k = 1.0 / l2.sqrt();
    [x * k, y * k, z * k]
}

/// FRotationMatrix rows for a rotator in radians.
pub fn rotation_rows(p: f64, y: f64, r: f64) -> Rows {
    let (cp, sp, cy, sy, cr, sr) = (cos(p), sin(p), cos(y), sin(y), cos(r), sin(r));
    [
        cp * cy,
        cp * sy,
        sp,
        sr * sp * cy - cr * sy,
        sr * sp * sy + cr * cy,
        -sr * cp,
        -(cr * sp * cy + sr * sy),
        cy * sr - cr * sp * sy,
        cr * cp,
    ]
}

/// A child's relative rotation under its parent (FTransform Child * Parent): rows of the world axes.
pub fn compose_rows(child: &Rows, parent: &Rows) -> Rows {
    let mut o = [0.0; 9];
    for i in 0..3 {
        for c in 0..3 {
            o[3 * i + c] =
                child[3 * i] * parent[c] + child[3 * i + 1] * parent[3 + c] + child[3 * i + 2] * parent[6 + c];
        }
    }
    o
}

/// The world rotation rows of a relative rotator (pitch, yaw, roll in degrees) under its parent's rows (none: the
/// rotator's own rows).
pub fn rotator_rows(rel: V3, parent: Option<&Rows>) -> Rows {
    let d = std::f64::consts::PI / 180.0;
    let (cp, sp) = (cos(rel[0] * d), sin(rel[0] * d));
    let (cy, sy) = (cos(rel[1] * d), sin(rel[1] * d));
    let (cr, sr) = (cos(rel[2] * d), sin(rel[2] * d));
    let (x0, x1, x2) = (cp * cy, cp * sy, sp);
    let (y0, y1, y2) = (sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp);
    let (z0, z1, z2) = (-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp);
    let Some(p) = parent else {
        return [x0, x1, x2, y0, y1, y2, z0, z1, z2];
    };
    let mut out = [0.0; 9];
    for c in 0..3 {
        out[c] = x0 * p[c] + x1 * p[3 + c] + x2 * p[6 + c];
        out[3 + c] = y0 * p[c] + y1 * p[3 + c] + y2 * p[6 + c];
        out[6 + c] = z0 * p[c] + z1 * p[3 + c] + z2 * p[6 + c];
    }
    out
}

/// FRotator::NormalizeAxis: fmod 360, into (-180, 180].
pub fn normalize_axis(a: f64) -> f64 {
    let mut a = a % 360.0;
    if a < 0.0 {
        a += 360.0;
    }
    if a > 180.0 { a - 360.0 } else { a }
}

/// FMatrix::Rotator() of rotation rows, as FRotator::Euler(): (roll, pitch, yaw) in degrees.
pub fn rows_to_euler(r: &Rows) -> V3 {
    let d = 180.0 / std::f64::consts::PI;
    let pitch = atan2(r[2], hypot2(r[0], r[1]));
    let yaw = atan2(r[1], r[0]);
    let s = rotation_rows(pitch, yaw, 0.0);
    let sy = [s[3], s[4], s[5]];
    let roll = atan2(r[6] * sy[0] + r[7] * sy[1] + r[8] * sy[2], r[3] * sy[0] + r[4] * sy[1] + r[5] * sy[2]);
    [roll * d, pitch * d, yaw * d]
}

/// The rows of a camera-facing mesh particle (FDynamicMeshEmitterData::CalculateParticleTransform's facing block):
/// X toward the camera, then turned by the particle rotation `rot` (radians) about X. XAxisFacing_ZUp: Z the world
/// up with its X part removed, Y = Z x X. XAxisFacing_NoUp (`no_up`): Y and Z the images of the mesh's Y and Z
/// under the shortest arc from +X to X (FQuat::FindBetween), a half turn about Z when X is -X.
pub fn facing_rows(pos: V3, cam: V3, rot: f64, no_up: bool) -> Rows {
    let (mut x0, mut x1, mut x2) = (cam[0] - pos[0], cam[1] - pos[1], cam[2] - pos[2]);
    let l = hypot3(x0, x1, x2);
    if l * l >= 0.5 {
        x0 /= l;
        x1 /= l;
        x2 /= l;
    } else {
        (x0, x1, x2) = (1.0, 0.0, 0.0);
    }
    let (y0, y1, y2, z0, z1, z2);
    if no_up {
        let k = 1.0 + x0;
        if k >= 1e-6 {
            y0 = -x1;
            y1 = 1.0 - (x1 * x1) / k;
            y2 = (-x1 * x2) / k;
            z0 = -x2;
            z1 = (-x1 * x2) / k;
            z2 = 1.0 - (x2 * x2) / k;
        } else {
            (y0, y1, y2) = (0.0, -1.0, 0.0);
            (z0, z1, z2) = (0.0, 0.0, 1.0);
        }
    } else {
        let (mut a, mut b, mut c) = (-x2 * x0, -x2 * x1, 1.0 - x2 * x2);
        let lz = hypot3(a, b, c);
        if lz > 1e-8 {
            a /= lz;
            b /= lz;
            c /= lz;
        } else {
            (a, b, c) = (0.0, 0.0, 1.0);
        }
        (z0, z1, z2) = (a, b, c);
        y0 = z1 * x2 - z2 * x1;
        y1 = z2 * x0 - z0 * x2;
        y2 = z0 * x1 - z1 * x0;
    }
    let (c, sn) = (cos(rot), sin(rot));
    [
        x0,
        x1,
        x2,
        c * y0 + sn * z0,
        c * y1 + sn * z1,
        c * y2 + sn * z2,
        c * z0 - sn * y0,
        c * z1 - sn * y1,
        c * z2 - sn * y2,
    ]
}
