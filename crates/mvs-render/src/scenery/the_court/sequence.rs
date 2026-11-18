//! The Court's level sequence (Map_M010_Animation_C's BeginPlay: SpaceJam_AnimSeq_2.PlayLooping(-1) @52;
//! M010_AnimSeq), drawn from the block's `sequence`. Its one binding, the
//! SkeletalMeshActor M010_BalloonBoat_01:
//! - The transform section (blend absolute, every channel on) sets the actor's root (its SkeletalMeshComponent0) to
//!   Translation (X, Y, Z), Rotation (channels X roll, Y pitch, Z yaw) and Scale at the sequence time, each a
//!   FMovieSceneFloatChannel (`rich_curve`: the cubic keys' Bezier form, the same as FRichCurve's; constant
//!   extrapolation). The component is moved to it, the latest two ticks' places blended by the frame's alpha.
//! - The skeletal section plays M010_BalloonBoat_Anim_Idle at weight 1 from the section's start, rate 1, no offsets:
//!   the component's animation time is (sequence time - section start) mod the animation's length (the drawn clip's
//!   time, its own rate 0).
//! - The sequence time starts at the playback range's start (1/30 s) at the level's begin and runs with the ticks,
//!   wrapping at the range's end (79.3667 s) to its start.
// Unverified: the movie scene evaluation (the channel Bezier, the sequencer taking a single-node component's animation
// over at the section's time, the first evaluation at Play and the loop wrap with no remainder lost) is 4.26 source
// from memory, not read in the exe; the boat has no collision the port takes in.

use mvs_view::M4;
use mvs_view::math::{compose, rotator_quat};
use serde::Deserialize;
use serde_json::Value;

use crate::Result;
use crate::scenery::host::SceneryHost;
use crate::scenery::{CurveKey, rich_curve};
use crate::stage::bad;

#[derive(Debug, Deserialize)]
pub(super) struct SequenceAnim {
    path: String,
    from: f64,
    rate: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct SequenceBlock {
    skeletal: String,
    start: f64,
    end: f64,
    translation: [Vec<CurveKey>; 3],
    rotation: [Vec<CurveKey>; 3],
    scale: [Vec<CurveKey>; 3],
    anim: SequenceAnim,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Xf {
    t: [f64; 3],
    q: [f64; 4],
    s: [f64; 3],
}

/// Quaternion.slerpQuaternions(a, b, t).
pub(super) fn slerp(a: [f64; 4], b: [f64; 4], t: f64) -> [f64; 4] {
    if t == 0.0 {
        return a;
    }
    if t == 1.0 {
        return b;
    }
    let [x, y, z, w] = a;
    let mut cos_half = w * b[3] + x * b[0] + y * b[1] + z * b[2];
    let mut o = b;
    if cos_half < 0.0 {
        o = [-b[0], -b[1], -b[2], -b[3]];
        cos_half = -cos_half;
    }
    if cos_half >= 1.0 {
        return a;
    }
    let sqr_sin = 1.0 - cos_half * cos_half;
    if sqr_sin <= f64::EPSILON {
        let s = 1.0 - t;
        let q = [s * x + t * o[0], s * y + t * o[1], s * z + t * o[2], s * w + t * o[3]];
        let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        return if l == 0.0 { [0.0, 0.0, 0.0, 1.0] } else { q.map(|c| c / l) };
    }
    let sin_half = sqr_sin.sqrt();
    let half = sin_half.atan2(cos_half);
    let ra = ((1.0 - t) * half).sin() / sin_half;
    let rb = (t * half).sin() / sin_half;
    [x * ra + o[0] * rb, y * ra + o[1] * rb, z * ra + o[2] * rb, w * ra + o[3] * rb]
}

#[derive(Debug)]
pub(super) struct Sequence {
    d: SequenceBlock,
    time: f64,
    cur: Xf,
    prev: Xf,
}

impl Sequence {
    pub(super) fn make(d: SequenceBlock, host: &SceneryHost) -> Result<Sequence> {
        let what = "scenery: the court sequence";
        let k = host
            .skeletals()
            .iter()
            .find(|k| k.name == d.skeletal)
            .ok_or_else(|| bad(what, format!("the binding {} not in the file", d.skeletal)))?;
        let anim = k.fields.get("anim").and_then(Value::as_str);
        if anim != Some(&d.anim.path) {
            return Err(bad(what, format!("plays {} on {}, the file {anim:?}", d.anim.path, k.name)));
        }
        Ok(Sequence { time: d.start, d, cur: Xf::default(), prev: Xf::default() })
    }

    fn evaluate(&mut self, host: &mut SceneryHost) {
        let (t, d) = (self.time, &self.d);
        let ch = |c: &[Vec<CurveKey>; 3]| [rich_curve(&c[0], t), rich_curve(&c[1], t), rich_curve(&c[2], t)];
        let r = ch(&d.rotation);
        self.cur = Xf { t: ch(&d.translation), q: rotator_quat(r[1], r[2], r[0]), s: ch(&d.scale) };
        // the skeletal section's time; the clip wraps it at its length as it loops (`% clip.length`)
        let from = d.anim.from;
        let rate = d.anim.rate;
        if let Some(k) = host.skeletal(&d.skeletal) {
            k.time = if t < from { 0.0 } else { (t - from) * rate };
        }
    }

    fn place(&self, host: &mut SceneryHost, a: f64) {
        let (p, c) = (&self.prev, &self.cur);
        let lerp = |x: [f64; 3], y: [f64; 3]| -> [f64; 3] { std::array::from_fn(|i| x[i] + (y[i] - x[i]) * a) };
        let e = compose(lerp(p.t, c.t), slerp(p.q, c.q, a), lerp(p.s, c.s));
        let m: M4 = std::array::from_fn(|i| std::array::from_fn(|j| e[i * 4 + j]));
        host.place_skeletal(&self.d.skeletal, Some(m));
    }

    pub(super) fn begin(&mut self, host: &mut SceneryHost) {
        // (its time is the sequence's: the clip's own rate 0)
        if let Some(k) = host.skeletal(&self.d.skeletal) {
            k.rate = 0.0;
            k.looping = true;
        }
        self.time = self.d.start;
        self.evaluate(host);
        self.prev = self.cur;
        self.place(host, 1.0);
    }

    pub(super) fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        self.prev = self.cur;
        self.time += dt;
        if self.time >= self.d.end {
            self.time = self.d.start + (self.time - self.d.start) % (self.d.end - self.d.start);
        }
        self.evaluate(host);
    }

    pub(super) fn frame(&self, host: &mut SceneryHost, a: f64) {
        self.place(host, a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slerp_halfway() {
        let a = rotator_quat(0.0, 0.0, 0.0);
        let b = rotator_quat(0.0, 90.0, 0.0);
        let m = slerp(a, b, 0.5);
        let want = rotator_quat(0.0, 45.0, 0.0);
        for c in 0..4 {
            assert!((m[c] - want[c]).abs() < 1e-12, "{m:?} {want:?}");
        }
        assert_eq!(slerp(a, b, 0.0), a);
        assert_eq!(slerp(a, b, 1.0), b);
    }
}
