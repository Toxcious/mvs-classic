//! An animation curve's value at a time: the exe's CompressedRichCurve
//! evaluation on the keys the animation kind stores.

use crate::format::AnimCurveKey;

const INTERP_LINEAR: f64 = 0.0;
const INTERP_CONSTANT: f64 = 1.0;
const THIRD: f64 = 1.0 / 3.0;

/// `evalCurve`: in the key interval (k, k+1) with dt = t1 - t0, dt <= 0 or a constant key gives v_k, a linear key
/// the lerp, a cubic key the Bezier with P1 = v_k + leave_k dt/3 and P2 = v_k+1 - arrive_k+1 dt/3; Constant
/// extrapolation on both sides.
pub fn eval_curve(keys: &[AnimCurveKey], time: f64) -> f64 {
    let n = keys.len();
    assert!(n > 0, "a curve without keys (the importer writes one at least)");
    let first = &keys[0];
    if n == 1 || time <= first.time {
        return first.value;
    }
    let last = &keys[n - 1];
    if time >= last.time {
        return last.value;
    }
    // the last key at or before `time`
    let mut lo = 0usize;
    let mut hi = n - 2;
    while lo < hi {
        let mid = (lo + hi + 1) >> 1;
        if keys[mid].time <= time {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let a = &keys[lo];
    let b = &keys[lo + 1];
    let dt = b.time - a.time;
    if dt <= 0.0 || a.interp == INTERP_CONSTANT {
        return a.value;
    }
    let x = (time - a.time) / dt;
    let p0 = a.value;
    let p3 = b.value;
    if a.interp == INTERP_LINEAR {
        return p0 + (p3 - p0) * x;
    }
    let p1 = p0 + a.leave * dt * THIRD;
    let p2 = p3 - b.arrive * dt * THIRD;
    let p01 = p0 + (p1 - p0) * x;
    let p12 = p1 + (p2 - p1) * x;
    let p23 = p2 + (p3 - p2) * x;
    let p012 = p01 + (p12 - p01) * x;
    let p123 = p12 + (p23 - p12) * x;
    p012 + (p123 - p012) * x
}
