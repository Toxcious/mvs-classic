//! The sweep of a sphere against one box of the play plane, in UE's Y and Z.
//! The port's own slab test standing in for PhysX's sphere sweep, in float32 in a fixed order of operations.

use crate::{Ground, arg, max, min, out};

/// sweepBox: the fraction of the sweep from (y0, z0) along (dy, dz) at the first touch of the box grown by r (0 for a
/// sweep that starts inside), -1 for a miss.
pub fn sweep_box(
    g: &Ground,
    y0: impl Into<f64>,
    z0: impl Into<f64>,
    dy: impl Into<f64>,
    dz: impl Into<f64>,
    r: impl Into<f64>,
) -> f64 {
    let g = g.narrow();
    let (y0, z0, dy, dz, r) = (arg(y0), arg(z0), arg(dy), arg(dz), arg(r));
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for i in 0..2 {
        let (p, d) = if i == 1 { (z0, dz) } else { (y0, dy) };
        let lo = if i == 1 { g.bottom - r } else { -g.right - r };
        let hi = if i == 1 { g.top + r } else { -g.left + r };
        if d == 0.0 {
            if p <= lo + 0.01 || p >= hi - 0.01 {
                return -1.0;
            }
            continue;
        }
        let mut a = (lo - p) / d;
        let mut b = (hi - p) / d;
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        t0 = max(t0, a);
        t1 = min(t1, b);
        if t0 > t1 {
            return -1.0;
        }
    }
    out(t0)
}
