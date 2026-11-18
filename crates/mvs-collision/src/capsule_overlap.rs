//! A spawned actor's simulating Capsule against a fighter's root capsule:
//! the port's own segment geometry standing in for PhysX's capsule overlap, in float32 in a fixed order of operations.

use crate::{arg, max, min};
use mvs_math::f32::crt;

/// The squared distance between the segments (ax, ay)-(bx, by) and (cx, cy)-(dx, dy) (segSegDist2): Ericson's closest
/// points, a zero-length segment as a point on the other.
#[allow(clippy::too_many_arguments)]
pub fn seg_seg_dist2(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32, dx: f32, dy: f32) -> f32 {
    let (ux, uy, vx, vy, wx, wy) = (bx - ax, by - ay, dx - cx, dy - cy, ax - cx, ay - cy);
    let a = ux * ux + uy * uy;
    let b = ux * vx + uy * vy;
    let c = vx * vx + vy * vy;
    let d = ux * wx + uy * wy;
    let e = vx * wx + vy * wy;
    let clamp = |x: f32| min(max(x, 0.0), 1.0);
    let (mut s, mut t) = (0.0f32, 0.0f32);
    if a <= 1e-9 {
        t = if c > 1e-9 { clamp(e / c) } else { 0.0 };
    } else if c <= 1e-9 {
        s = clamp(-d / a);
    } else {
        let den = a * c - b * b;
        s = if den > 1e-9 { clamp((b * e - c * d) / den) } else { 0.0 };
        t = (b * s + e) / c;
        if t < 0.0 {
            t = 0.0;
            s = clamp(-d / a);
        } else if t > 1.0 {
            t = 1.0;
            s = clamp((b - d) / a);
        }
    }
    let px = wx + s * ux - t * vx;
    let py = wy + s * uy - t * vy;
    px * px + py * py
}

/// OverlapCapsule: the actor's Capsule (centre, roll `angle` in radians, CapsuleRadius, CapsuleHalfHeight).
#[derive(Clone, Copy, Debug)]
pub struct OverlapCapsule {
    pub x: f64,
    pub y: f64,
    pub angle: f64,
    pub radius: f64,
    pub half: f64,
}

/// The fighter's values capsuleOver reads.
#[derive(Clone, Copy, Debug)]
pub struct FighterCapsule {
    pub dead: bool,
    pub no_collision: bool,
    pub cap_radius: f64,
    pub capsule_half: f64,
    pub actor_y: f64,
    pub x: f64,
}

/// capsuleOver: the actor's Capsule against the fighter's root capsule, both as segments in the play plane.
pub fn capsule_over(a: &OverlapCapsule, f: &FighterCapsule) -> bool {
    if f.dead || f.no_collision {
        return false;
    }
    let (ax, ay, angle, radius, half) = (arg(a.x), arg(a.y), arg(a.angle), arg(a.radius), arg(a.half));
    let l = max(half - radius, 0.0);
    // Unverified: the capsule's axis from the body's roll in radians through the CRT sinf / cosf (PhysX's PxSin /
    // PxCos); the build's shape pose is the body's quaternion, which the port's planar body does not keep.
    let ux = -crt::sinf(angle) * l;
    let uy = crt::cosf(angle) * l;
    let r = arg(f.cap_radius);
    let s = max(arg(f.capsule_half) - r, 0.0);
    let cy = arg(f.actor_y);
    let fx = arg(f.x);
    let rr = radius + r;
    seg_seg_dist2(ax + ux, ay + uy, ax - ux, ay - uy, fx, cy - s, fx, cy + s) <= rr * rr
}
