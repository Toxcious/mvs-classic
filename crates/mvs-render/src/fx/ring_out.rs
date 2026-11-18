//! A ring-out effect's entry and transform on this machine (BlastBoxPlayerExit, URingOutVfxData;
//! The sim gives the asset and the inputs (the koVfx event); what is drawn is
//! cosmetic and local, as the build's GetParticleDataToPlay is.

use indexmap::IndexMap;
use mvs_data::tables::{RingOutEntry, RingOutTable};
use mvs_sim::world::blast::DEFAULT_RING_OUT;

type Quat = [f64; 4];
const D2R: f64 = std::f64::consts::PI / 180.0;

/// URingOutVfxData::GetParticleDataToPlay (14115f6b0): the transient index (+0x19c, 0 at construction) of each
/// asset.
#[derive(Debug, Default)]
pub struct RingOuts {
    index: IndexMap<String, usize>,
    /// CRT rand()'s state (MSVC's LCG).
    rand: u32,
}

impl RingOuts {
    pub fn new(seed: u32) -> RingOuts {
        RingOuts { index: IndexMap::new(), rand: seed }
    }

    /// CRT rand(): 0..0x7fff.
    // Unverified: the pick uses MSVC's rand() sequence from the Fx seed (the pick is local
    // and cosmetic in the build too).
    fn crt_rand(&mut self) -> u32 {
        self.rand = self.rand.wrapping_mul(214013).wrapping_add(2531011);
        (self.rand >> 16) & 0x7fff
    }

    pub fn particle_data_to_play<'t>(&mut self, table: &'t RingOutTable, asset: &str) -> Option<&'t RingOutEntry> {
        let a = table.assets.get(asset)?;
        let n = a.entries.len();
        if n < 1 {
            return None; // (Success false: GetEmitterDataToPlay falls back to ROV_Default's)
        }
        let mut i = *self.index.get(asset).unwrap_or(&0);
        if n == 1 {
            i = 0;
        } else if a.random.is_none_or(|r| r == 0.0) {
            i = (i + 1) % n; // (the first call gives entry 1)
        } else {
            let r = f64::from(self.crt_rand() & 0x7fff);
            let k = ((r * 3.0518509e-5 * n as f64).trunc() as usize).min(n - 1);
            i = if k == i { (i + 1) % n } else { k };
        }
        self.index.insert(asset.to_string(), i);
        Some(&a.entries[i])
    }

    /// GetEmitterDataToPlay (@0-343): the asset's entry, else ROV_Default's.
    pub fn emitter_data_to_play<'t>(&mut self, table: &'t RingOutTable, asset: &str) -> Option<&'t RingOutEntry> {
        match self.particle_data_to_play(table, asset) {
            Some(e) => Some(e),
            None => self.particle_data_to_play(table, DEFAULT_RING_OUT),
        }
    }
}

/// FRotator::Quaternion (degrees).
fn quat_of(pitch: f64, yaw: f64, roll: f64) -> Quat {
    let (sp, cp) = ((pitch * D2R * 0.5).sin(), (pitch * D2R * 0.5).cos());
    let (sy, cy) = ((yaw * D2R * 0.5).sin(), (yaw * D2R * 0.5).cos());
    let (sr, cr) = ((roll * D2R * 0.5).sin(), (roll * D2R * 0.5).cos());
    [
        cr * sp * sy - sr * cp * cy,
        -cr * sp * cy - sr * cp * sy,
        cr * cp * sy - sr * sp * cy,
        cr * cp * cy + sr * sp * sy,
    ]
}

fn normalize_axis(a: f64) -> f64 {
    let mut r = a % 360.0;
    if r < 0.0 {
        r += 360.0;
    }
    if r > 180.0 { r - 360.0 } else { r }
}

/// FQuat::Rotator: pitch, yaw, roll.
fn rotator_of(q: Quat) -> [f64; 3] {
    let [x, y, z, w] = q;
    let sing = z * x - w * y;
    let yaw = (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z)) / D2R;
    const T: f64 = 0.4999995;
    if sing < -T {
        return [-90.0, yaw, normalize_axis(-yaw - 2.0 * x.atan2(w) / D2R)];
    }
    if sing > T {
        return [90.0, yaw, normalize_axis(yaw - 2.0 * x.atan2(w) / D2R)];
    }
    [(2.0 * sing).asin() / D2R, yaw, (-2.0 * (w * x + y * z)).atan2(1.0 - 2.0 * (x * x + y * y)) / D2R]
}

/// FQuat operator* (b's rotation first).
fn qmul(a: Quat, b: Quat) -> Quat {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// RLerp(A, B, alpha, bShortestPath) (142b2bbd0): FQuat::Slerp_NotNormalized (141370870), normalised.
fn slerp(a: Quat, b: Quat, t: f64) -> Quat {
    let raw = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let cosom = raw.abs();
    let (s0, mut s1) = if cosom < 0.9999 {
        let omega = cosom.acos();
        let inv = 1.0 / omega.sin();
        (((1.0 - t) * omega).sin() * inv, (t * omega).sin() * inv)
    } else {
        (1.0 - t, t)
    };
    if raw < 0.0 {
        s1 = -s1;
    }
    let q = [0, 1, 2, 3].map(|k| s0 * a[k] + s1 * b[k]);
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if len > 1e-8 { q.map(|v| v / len) } else { [0.0, 0.0, 0.0, 1.0] }
}

/// Get Angle Between Two Points(Center, Outer) on (Y, Z).
fn angle_between(c: [f64; 2], o: [f64; 2]) -> f64 {
    (o[1] - c[1]).atan2(c[0] - o[0]) / D2R
}

/// The koVfx event's inputs (Y, Z pairs).
pub struct KoInputs {
    pub p: [f64; 2],
    pub tl: [f64; 2],
    pub tr: [f64; 2],
    pub bl: [f64; 2],
    pub center: [f64; 2],
}

/// The effect's world location, rotator (degrees) and scale.
pub struct KoTransform {
    pub loc: [f64; 3],
    pub rot: [f64; 3],
    pub scale: [f64; 3],
}

fn at(v: &[f64], k: usize) -> f64 {
    v.get(k).copied().unwrap_or(0.0)
}

/// BlastBoxPlayerExit GetLocationRotationScale (@0-2569) for one entry.
// Unverified: GetLastRenderedLocation is taken as the actor location at the ring-out (it weighs only where
// LocationLerpAlpha is under 1, and every entry the paks draw has 1).
pub fn ko_transform(e: &RingOutEntry, k: &KoInputs) -> KoTransform {
    let [py, pz] = k.p;
    // C: the actor location with Z clamped to TopLeft.Z (@446-539); VLerp(LastRendered, C, alpha) @594
    let cz = if pz >= k.tl[1] { k.tl[1] } else { pz };
    let (ly, lz) = (py, pz + (cz - pz) * e.lerp);
    // GetDefaultOffsets (@0-387)
    let def = if pz >= 0.0 { [-100.0, 200.0, 0.0] } else { [-100.0, 200.0, 500.0] };
    let off = [at(&e.off, 0), at(&e.off, 1), at(&e.off, 2)];
    let zsel = match e.ceil {
        Some(c) if lz > 0.0 => off[2] + c,
        _ => off[2],
    };
    let len = ly.hypot(lz);
    let (uy, uz) = if len > 1e-8 { (-ly / len, -lz / len) } else { (0.0, 0.0) };
    let loc = [def[0] + off[0], ly + uy * (off[1] + def[1]), lz + uz * (zsel + def[2])];
    let truthy = |v: Option<f64>| v.is_some_and(|v| v != 0.0);
    let flip = (truthy(e.flip_l) && ly >= k.tl[0]) || (truthy(e.flip_r) && ly <= k.tr[0]);
    let scale = [at(&e.scale, 0), if flip { -1.0 } else { 1.0 } * at(&e.scale, 1), at(&e.scale, 2)];
    let r = [at(&e.rot, 0), at(&e.rot, 1), at(&e.rot, 2)];
    if !e.rotate {
        return KoTransform { loc, rot: [r[1], r[2], r[0]], scale }; // MakeRotator(X, Y, Z) @2471
    }
    let offq = quat_of(r[1], r[2], r[0]);
    let roll = |a: Quat, deg: f64| qmul(quat_of(0.0, 0.0, deg), a);
    let radial = roll(offq, angle_between([ly, lz], k.center) - 90.0);
    let v = if k.tl[1] <= pz {
        -1.0
    } else if k.bl[1] >= pz {
        1.0
    } else {
        0.0
    };
    let h = if k.tl[0] <= py {
        -1.0
    } else if k.tr[0] >= py {
        1.0
    } else {
        0.0
    };
    let perp = roll(offq, angle_between([py, pz], [py + h * 100.0, pz + v * 100.0]) - 90.0);
    KoTransform { loc, rot: rotator_of(slerp(radial, perp, e.perp)), scale }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quat_round_trips() {
        let r = rotator_of(quat_of(10.0, 20.0, 30.0));
        assert!((r[0] - 10.0).abs() < 1e-9 && (r[1] - 20.0).abs() < 1e-9 && (r[2] - 30.0).abs() < 1e-9);
    }
}
