//! Positioning: CAkSpeakerPan with focus and spread for a stereo output, the speaker tables and the attenuation's
//! distance curves (dry volume, aux sends, LPF, HPF, spread, focus).

use std::sync::OnceLock;

use mvs_data::sound::{Att, Scaling};
use mvs_math::hypot::hypot3;
use mvs_math::js::max;

use super::curves::pts_at;
use crate::math::{cos, sin};
use crate::{Emitter, Listener, V3};

const R2: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// Directions of a source's channels around its emitter as (sin, height, cos) of the turn from the front
/// (AkSoundEngineDLL table 18010e0b0, one block per channel count without the LFE), for 1 to 7 channels.
pub const PAN_DIRS: [&[V3]; 8] = [
    &[],
    &[[0.0, 0.0, 1.0]],
    &[[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
    &[[-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]],
    &[[-R2, 0.0, -R2], [-R2, 0.0, R2], [R2, 0.0, R2], [R2, 0.0, -R2]],
    &[[-0.5878, 0.0, -0.809], [-0.9511, 0.0, 0.309], [0.0, 0.0, 1.0], [0.9511, 0.0, 0.309], [0.5878, 0.0, -0.809]],
    &[
        [-0.5, 0.0, -0.866],
        [-1.0, 0.0, 0.0],
        [-0.5, 0.0, 0.866],
        [0.5, 0.0, 0.866],
        [1.0, 0.0, 0.0],
        [0.5, 0.0, -0.866],
    ],
    &[
        [-0.4339, 0.0, -0.901],
        [-0.9749, 0.0, -0.2225],
        [-0.7818, 0.0, 0.6235],
        [0.0, 0.0, 1.0],
        [0.7818, 0.0, 0.6235],
        [0.9749, 0.0, -0.2225],
        [0.4339, 0.0, -0.901],
    ],
];

/// The speaker whose channel takes each direction of a block, for sources with more than two full-range channels
/// (AkSoundEngineDLL table 18014e300: speaker bit | channel index << 16, read by GetSpeakerVolumes 18006cce0). The
/// blocks run round the emitter from the rear left. Indexed by the full-range channel count (3 to 7).
pub const RING_BITS: [&[u8]; 8] =
    [&[], &[], &[], &[0, 2, 1], &[9, 0, 1, 10], &[9, 0, 2, 1, 10], &[4, 9, 0, 1, 10, 5], &[4, 9, 0, 2, 1, 10, 5]];

/// Direct speaker assignment of a multichannel source on a stereo output (FUN_180068b20, table 18010d8b0): left
/// and right gain per speaker bit (FL FR FC LFE BL BR - - BC SL SR). The LFE only passes when the output has one
/// (FUN_18006f420).
pub const STEREO_MIX: [[f64; 2]; 11] = [
    [1.0, 0.0],
    [0.0, 1.0],
    [R2, R2],
    [0.0, 0.0],
    [R2, 0.0],
    [0.0, R2],
    [0.0, 0.0],
    [0.0, 0.0],
    [R2, R2],
    [R2, 0.0],
    [0.0, R2],
];
pub const LFE_BIT: u8 = 3;

/// The media keep the wem's channel order (the mask's bits, low to high) in the Vorbis channel layouts: decoded
/// channel -> stream channel, by channel count (3 to 6).
pub const VORBIS_ORDER: [&[u8]; 7] = [&[], &[], &[], &[0, 2, 1], &[0, 1, 2, 3], &[0, 2, 1, 3, 4], &[0, 2, 1, 5, 3, 4]];

/// The stereo output's speakers sit at 45 degrees left and right; the arcs behind fold onto the same two.
const PAN_VERT: [[f64; 2]; 5] = [[-R2, R2], [R2, R2], [R2, -R2], [-R2, -R2], [-R2, R2]];
/// Which side each of those speakers is: 0 left, 1 right.
const PAN_CH: [u8; 5] = [0, 1, 1, 0, 0];

/// The attenuation's curve `i` (`Att.uses`) at `x`, None when it has none.
pub fn att_curve(a: Option<&Att>, i: usize, x: f64) -> Option<f64> {
    let a = a?;
    let u = usize::try_from(a.uses[i]).ok()?;
    Some(pts_at(&a.curves[u].pts, x))
}

/// The attenuation's curve `i` at `x` as a linear gain (a dB curve stores gain - 1), 1 when it has none.
pub fn att_lin(a: Option<&Att>, i: usize, x: f64) -> f64 {
    let (Some(att), Some(y)) = (a, att_curve(a, i, x)) else {
        return 1.0;
    };
    let u = usize::try_from(att.uses[i]).unwrap_or(0);
    if att.curves[u].scaling == Scaling::Db { max(y + 1.0, 0.0) } else { y }
}

/// CAkSpeakerPan's two contractions of a unit vector `p` about an axis, by a factor `f` in 0..1 (1 leaves it
/// alone). Up to one half (ContractPoints<ContractOutside> 1800678c0, FUN_180067e20) the point is pulled toward the
/// axis: normalise(p + k * axis) with k = 1 / sin(f * pi), the sine by the engine's parabola approximation. Above
/// (ContractPoints<ContractInside> 1800676a0, FUN_180067a20) the point's elevation over the plane across the axis
/// is scaled: asin(z) * f + (1 - f) * pi / 2, both functions as the engine's polynomials.
fn contract(p: V3, axis: V3, f: f64) -> V3 {
    let [p0, p1, p2] = p;
    let [a0, a1, a2] = axis;
    let mut z = p0 * a0 + p1 * a1 + p2 * a2;
    // the part of p across the axis
    let e0 = p0 - z * a0;
    let e1 = p1 - z * a1;
    let e2 = p2 - z * a2;
    if f <= 0.5 {
        let s = f * std::f64::consts::PI;
        let mut t = 1.27324 * s - 0.405285 * s.abs() * s;
        t = 0.225 * (t.abs() * t - t) + t;
        let zk = max(z, -0.999_000_012_874_603_3) + 1.0 / (t + 1e-4);
        let x = e0 + zk * a0;
        let y = e1 + zk * a1;
        let w = e2 + zk * a2;
        let l = nonzero(hypot3(x, y, w));
        return [x / l, y / l, w / l];
    }
    let r2 = e0 * e0 + e1 * e1 + e2 * e2;
    z = max(z, -1.0);
    if r2 < 1e-4 {
        z = if z >= 0.0 { 1.0 } else { -1.0 };
    }
    let z2 = z * z;
    let ph = ((0.719_088_017_940_521_2 * z2 - 0.357_859_998_941_421_5) * z2 + 1.085_865_020_751_953_1) * z * f
        + (1.0 - f) * (std::f64::consts::PI / 2.0);
    let ph2 = ph * ph;
    let zn = ((0.008_332_160_301_506_52 * ph2 - 0.166_666_552_424_430_85) * ph2 + 1.0) * ph;
    let c2 = 1.0 - zn * zn;
    let sc = c2 / max(c2 * r2, 1e-4).sqrt();
    [e0 * sc + zn * a0, e1 * sc + zn * a1, e2 * sc + zn * a2]
}

/// JavaScript's `x || 1`: a zero or NaN length is 1.
fn nonzero(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() { 1.0 } else { x }
}

/// The points of a channel's ring and the turn between them, for a channel count.
fn ring_size(n_ch: usize) -> (usize, f64) {
    let c = n_ch as f64;
    let n = (((std::f64::consts::PI / c * 1.2732 + 1.01).trunc() as i32 + 3) & !3) as usize;
    (n, std::f64::consts::PI / c / n as f64)
}

/// The sine and cosine of each point's turn on a channel's ring, by channel count (1 to 8): up to 8 points.
fn ring_of(n_ch: usize) -> &'static [f64; 16] {
    static RINGS: OnceLock<Vec<[f64; 16]>> = OnceLock::new();
    &RINGS.get_or_init(|| {
        (0..=8)
            .map(|c| {
                let mut t = [0.0; 16];
                if c > 0 {
                    let (n, step) = ring_size(c);
                    for i in 0..n {
                        let th = -((n as f64 - 1.0) * step - 2.0 * i as f64 * step);
                        t[2 * i] = sin(th);
                        t[2 * i + 1] = cos(th);
                    }
                }
                t
            })
            .collect()
    })[n_ch]
}

/// CAkSpeakerPan::GetSpeakerVolumes (the exe's 143361640, AkSoundEngineDLL 180069a40 with its helpers), for a
/// stereo output: left / right gain per input channel into `out` (as long as
/// `dirs`). Each channel is a ring of points around its direction in the emitter's horizontal plane (mono: 8 points
/// over the whole circle); the spread pulls the points toward the direction of the emitter as seen by the listener
/// (spread 0: all on it), each point is panned between the two speakers of its arc with constant power, and the
/// channel's gains are the root of the mean power over the number of source channels. The focus narrows each
/// channel's ring toward the channel's own direction first, with the same two contractions. Validated against the
/// live game within 2e-4 (the engine normalises with rsqrtps). Not modelled: the Headphones output mode of the
/// game's settings (panning rule 1, another law).
pub fn speaker_volumes(lis: &Listener, em: &Emitter, dirs: &[V3], spread: f64, focus: f64, out: &mut [[f64; 2]]) {
    let (lr, lu, lf) = (lis.right, lis.up, lis.front);
    let dot = |v: V3, b: V3| v[0] * b[0] + v[1] * b[1] + v[2] * b[2];
    // the emitter's direction, front and top in the listener's frame (right, up, front)
    let v = [em.pos[0] - lis.pos[0], em.pos[1] - lis.pos[1], em.pos[2] - lis.pos[2]];
    let (d0, d1, d2) = (dot(v, lr), dot(v, lu), dot(v, lf));
    let dl = nonzero(hypot3(d0, d1, d2));
    let d = [d0 / dl, d1 / dl, d2 / dl];
    let (ef, et) = (em.front, em.top);
    let (f0, f1, f2) = (dot(ef, lr), dot(ef, lu), dot(ef, lf));
    let (t0, t1, t2) = (dot(et, lr), dot(et, lu), dot(et, lf));
    let (r0, r1, r2) = (t1 * f2 - t2 * f1, t2 * f0 - t0 * f2, t0 * f1 - t1 * f0);
    let n_ch = dirs.len();
    let (n, _) = ring_size(n_ch);
    let fo = 1.0 - focus;
    let ring = (spread > 0.0).then(|| ring_of(n_ch));
    let nf = n as f64;
    for c in 0..n_ch {
        let [ds, dh, dc] = dirs[c];
        let (mut pl, mut pr) = (0.0, 0.0);
        for i in 0..n {
            let (mut q0, mut q2) = (d[0], d[2]);
            if let Some(ring) = ring {
                let mut x = ring[2 * i];
                let mut z = ring[2 * i + 1];
                if fo != 1.0 {
                    let q = contract([x, 0.0, z], [0.0, 0.0, 1.0], fo);
                    x = q[0];
                    z = q[2];
                }
                let p0 = z * ds + x * dc;
                let p1 = z * dh;
                let p2 = z * dc - x * ds;
                let q = contract(
                    [p0 * r0 + p1 * t0 + p2 * f0, p0 * r1 + p1 * t1 + p2 * f1, p0 * r2 + p1 * t2 + p2 * f2],
                    d,
                    spread,
                );
                q0 = q[0];
                q2 = q[2];
            }
            let mut found = false;
            for a in 0..4 {
                let [x0, z0] = PAN_VERT[a];
                let [x1, z1] = PAN_VERT[a + 1];
                let det = x0 * z1 - x1 * z0;
                let ga = (q0 * z1 - q2 * x1) / det;
                let gb = (q2 * x0 - q0 * z0) / det;
                let w = ga * ga + gb * gb;
                if ga < -1e-7 || gb < -1e-7 || w < 1e-12 {
                    continue;
                }
                found = true;
                let l = (if PAN_CH[a] != 0 { 0.0 } else { ga * ga }) + (if PAN_CH[a + 1] != 0 { 0.0 } else { gb * gb });
                pl += l / w / nf;
                pr += (w - l) / w / nf;
                break;
            }
            if !found {
                // straight above or below the listener
                pl += 0.5 / nf;
                pr += 0.5 / nf;
            }
        }
        out[c] = [(pl / n_ch as f64).sqrt(), (pr / n_ch as f64).sqrt()];
    }
}

#[cfg(test)]
mod tests {
    use mvs_data::sound::{AttCurve, Curve, Pt};

    use super::*;

    fn v3(v: &serde_json::Value) -> V3 {
        [v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap()]
    }

    /// The oracle's own results.
    #[test]
    fn oracle_speaker_volumes_and_curves() {
        let probe: serde_json::Value = serde_json::from_str(include_str!("pan_probe.json")).unwrap();
        let lis = [
            Listener { pos: [100.0, 20.0, -50.0], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], front: [0.0, 0.0, 1.0] },
            Listener { pos: [0.0; 3], right: [0.8, 0.0, -0.6], up: [0.0, 1.0, 0.0], front: [0.6, 0.0, 0.8] },
        ];
        for c in probe["cases"].as_array().unwrap() {
            let em = Emitter { pos: v3(&c["em"]["pos"]), front: v3(&c["em"]["front"]), top: v3(&c["em"]["top"]) };
            let n = c["n"].as_u64().unwrap() as usize;
            let mut out = vec![[0.0; 2]; n];
            let l = &lis[c["lis"].as_u64().unwrap() as usize];
            speaker_volumes(l, &em, PAN_DIRS[n], c["spread"].as_f64().unwrap(), c["focus"].as_f64().unwrap(), &mut out);
            for (ch, o) in out.iter().enumerate() {
                for side in 0..2 {
                    let want = c["out"][ch][side].as_f64().unwrap();
                    assert_eq!(o[side], want, "{c}");
                }
            }
        }
        let pt = |x, y| Pt { x, y, curve: Curve::Linear };
        let att = Att {
            uses: [0, 1, -1, 2, -1, 3, -1],
            curves: vec![
                AttCurve { scaling: Scaling::Db, pts: vec![pt(0.0, 0.0), pt(1000.0, -1.0)] },
                AttCurve { scaling: Scaling::None, pts: vec![pt(0.0, 0.5), pt(500.0, 1.0)] },
                AttCurve { scaling: Scaling::None, pts: vec![pt(0.0, 0.0), pt(2000.0, 80.0)] },
                AttCurve { scaling: Scaling::None, pts: vec![pt(0.0, 100.0), pt(800.0, 0.0)] },
            ],
        };
        for a in probe["atts"].as_array().unwrap() {
            let (x, i) = (a[0].as_f64().unwrap(), a[1].as_u64().unwrap() as usize);
            assert_eq!(att_curve(Some(&att), i, x), a[2].as_f64(), "{a}");
            assert_eq!(att_lin(Some(&att), i, x), a[3].as_f64().unwrap(), "{a}");
        }
        assert_eq!((att_curve(None, 0, 1.0), att_lin(None, 0, 1.0)), (None, 1.0));
    }
}
