//! A component's instance parameters and the distributions the modules sample:
//! baked lookup tables, unbaked curves, parameter distributions.

use indexmap::IndexMap;
use mvs_math::js::{fround, max, min};

use crate::data::{Curve, Dist, ParamDist};
use crate::random::RandomSource;

/// A component's parameters by name. A vector parameter is one number for its three axes, or `Name.X` / `.Y` /
/// `.Z` per axis (an axis key wins over the name's number).
pub type Params = IndexMap<String, f64>;

const CURVE_STRIDE: usize = 11;

/// FInterpCurve<FVector>::Eval (140e69c70) with the Default the zero vector, then the distribution's LockedAxes
/// (UDistributionVectorConstantCurve::GetValue 142a36250), all in float32. No points: zero. Unverified: the order of
/// the float32 operations inside the cubic is the stock FMath::CubicInterp's.
fn eval_curve(c: &Curve, t: f64) -> [f64; 3] {
    let f = fround;
    let v = &c.v;
    let n = v.len() / CURVE_STRIDE;
    let mut out = [0.0; 3];
    if n == 0 {
        return out;
    }
    let x = f(t);
    let last = (n - 1) * CURVE_STRIDE;
    if x < f(v[0]) {
        for k in 0..3 {
            out[k] = f(v[1 + k]);
        }
    } else if x >= f(v[last]) {
        for k in 0..3 {
            out[k] = f(v[last + 1 + k]);
        }
    } else {
        let mut i = 0;
        while i + 2 < n && x >= f(v[(i + 1) * CURVE_STRIDE]) {
            i += 1;
        }
        let p = i * CURVE_STRIDE;
        let q = p + CURVE_STRIDE;
        let diff = f(f(v[q]) - f(v[p]));
        let mode = v[p + 10];
        if diff > 0.0 && mode != 2.0 {
            let a = f(f(x - f(v[p])) / diff);
            if mode == 0.0 {
                for k in 0..3 {
                    out[k] = f(f(v[p + 1 + k]) + f(f(f(v[q + 1 + k]) - f(v[p + 1 + k])) * a));
                }
            } else {
                let a2 = f(a * a);
                let a3 = f(a2 * a);
                let w0 = f(f(f(2.0 * a3) - f(3.0 * a2)) + 1.0);
                let w1 = f(f(a3 - f(2.0 * a2)) + a);
                let w2 = f(a3 - a2);
                let w3 = f(f(3.0 * a2) - f(2.0 * a3));
                for k in 0..3 {
                    let leave = f(diff * f(v[p + 7 + k]));
                    let arrive = f(diff * f(v[q + 4 + k]));
                    out[k] =
                        f(f(f(f(w0 * f(v[p + 1 + k])) + f(w1 * leave)) + f(w2 * arrive)) + f(w3 * f(v[q + 1 + k])));
                }
            }
        } else {
            for k in 0..3 {
                out[k] = f(v[p + 1 + k]);
            }
        }
    }
    lock_axes(c.lock, &mut out);
    out
}

fn lock_axes(lock: f64, out: &mut [f64; 3]) {
    if lock == 1.0 {
        out[1] = out[0];
    } else if lock == 2.0 {
        out[2] = out[0];
    } else if lock == 3.0 {
        out[2] = out[1];
    } else if lock == 4.0 {
        out[1] = out[0];
        out[2] = out[0];
    }
}

/// DistributionFloatParticleParameter (GetValue 142a36030, GetParamValue 142c134f0): the component's parameter,
/// else Constant; DPM_Direct returns it; DPM_Abs takes |x| first; then clamped to MinInput..MaxInput and mapped to
/// MinOutput + (x - MinInput) x gradient. The vector one per axis. Unverified: the vector one is the stock 4.26
/// source's (UDistributionVectorParticleParameter::GetValue), not read in the exe.
fn eval_param(q: &ParamDist, params: &Params) -> [f64; 3] {
    let set = params.get(&q.name).copied();
    let map = |a: &crate::data::ParamAxis, x: f64| -> f64 {
        if a.direct {
            return x;
        }
        let x = if a.abs { x.abs() } else { x };
        a.min_out + (min(max(x, a.min_in), a.max_in) - a.min_in) * a.g
    };
    if let Some(keys) = &q.keys {
        let mut out = [0.0; 3];
        for k in 0..3 {
            let a = &q.axes[k];
            let x = params.get(&keys[k]).copied().or(set).unwrap_or(a.constant);
            out[k] = map(a, x);
        }
        return out;
    }
    let a = &q.axes[0];
    let y = map(a, set.unwrap_or(a.constant));
    [y; 3]
}

fn at(v: &[f64], i: f64) -> f64 {
    // (an index past the table reads undefined: NaN in a sum, as in JavaScript)
    if i >= 0.0 && (i as usize) < v.len() { v[i as usize] } else { f64::NAN }
}

/// A distribution's value at `t` (the first of the three). A random table draws its
/// fractions from `rng` in the recorded order.
pub fn sample(d: &Dist, t: f64, params: &Params, rng: &mut dyn RandomSource) -> [f64; 3] {
    if let Some(c) = &d.curve {
        return eval_curve(c, t);
    }
    let Some(tb) = &d.table else {
        return [0.0; 3];
    };
    if let Some(q) = &d.param {
        return eval_param(q, params);
    }
    let v = &tb.v;
    if tb.n == 0.0 {
        return [v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0)];
    }
    let rt = max((t - tb.tb) * tb.ts, 0.0);
    let idx = rt.trunc();
    let a = rt - idx;
    let e1 = min(idx, tb.n - 1.0) * tb.st;
    let e2 = min(idx + 1.0, tb.n - 1.0) * tb.st;
    let lerp = |c: f64| at(v, e1 + c) + (at(v, e2 + c) - at(v, e1 + c)) * a;
    let mut out = [0.0; 3];
    if tb.op == 2.0 {
        // Random between the two sub-entries per component, honouring the axis lock.
        let r0 = rng.next_f64();
        let mut r1 = rng.next_f64();
        let mut r2 = rng.next_f64();
        if tb.lock == 1.0 {
            r1 = r0;
        } else if tb.lock == 2.0 {
            r2 = r0;
        } else if tb.lock == 3.0 {
            r2 = r1;
        } else if tb.lock == 4.0 {
            r1 = r0;
            r2 = r0;
        }
        let r = [r0, r1, r2];
        let mut c = 0;
        while (c as f64) < tb.sub && c < 3 {
            let cf = c as f64;
            let lo = lerp(cf);
            let hi = lerp(tb.sub + cf);
            out[c] = lo + r[c] * (hi - lo);
            c += 1;
        }
    } else if tb.op == 3.0 {
        let base = if rng.next_f64() > 0.5 { tb.sub } else { 0.0 };
        let mut c = 0;
        while (c as f64) < tb.sub && c < 3 {
            out[c] = lerp(base + c as f64);
            c += 1;
        }
    } else {
        let mut c = 0;
        while (c as f64) < tb.st && c < 3 {
            out[c] = lerp(c as f64);
            c += 1;
        }
    }
    out
}

/// The first value of `sample`.
pub fn sample1(d: &Dist, t: f64, params: &Params, rng: &mut dyn RandomSource) -> f64 {
    sample(d, t, params, rng)[0]
}

/// UParticleSystemComponent::SetColorParameter's colour as the vector distribution reads it back (exe 142bf8fc0:
/// each channel clamped to 0..1, sRGB-encoded, floor(x x 255.999) / 255, then through the sRGB table, 142beb580):
/// the three values a caller sets as `Name.X` / `.Y` / `.Z`.
pub fn color_param(rgb: [f64; 3]) -> [f64; 3] {
    rgb.map(|v| {
        let x = min(max(v, 0.0), 1.0);
        let s = if x <= 0.0031308 { x * 12.92 } else { 1.055 * mvs_math::libm::pow(x, 1.0 / 2.4) - 0.055 };
        let c = (s * 255.999).floor() / 255.0;
        if c <= 0.04045 { c / 12.92 } else { mvs_math::libm::pow((c + 0.055) / 1.055, 2.4) }
    })
}
