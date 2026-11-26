//! (the port's own, dev) A feel test of 3D movement (`--test-3d`): the 2D fight runs in one vertical "fight plane"
//! that turns about a fixed origin O on the floor, and each fighter has a depth `d` off that plane.
//!
//! The floor is a world plane with coordinates (a, b): the plane passes through O = (`origin`, 0) at angle `theta`,
//! along u = (cos theta, sin theta), its normal n = (-sin theta, cos theta). A fighter's sim x is its place along
//! the plane (O at sim x = `origin`), its sim y its height as ever, and `Body::d` its offset along n: its world point
//! is O + (x - origin) u + d n. Theta 0 and every d 0 is the 2D game: the floor's a is sim x, its b UE X.
//!
//! Each tick, ahead of the fighters' ticks: the strafe inputs move each player's d, d is held inside the floor's
//! depth, the plane turns toward the line through the two first players at a capped rate (the lock-on), and every
//! player is projected again onto the turned plane (its world point kept). Hits need the two sides' depths within
//! [`DEPTH_REACH`] (`combat::phases`); a projectile keeps its owner's depth at its spawn. Nothing else of the sim sees
//! depth: the pushes, the stage's edges, ledges and blast box stay the 2D plane's. Off unless a run switches it on
//! (`enable`): a World without it steps as before.

use crate::Real;
use crate::State;
use crate::actors::ProjectileId;
use crate::fighter::FighterId;
use crate::math::{max, min, narrow, ue};
use crate::world::World;

/// The fastest the plane turns toward the fighters' line, radians a second (240 degrees).
pub const TURN_RATE: Real = 240.0 * ue::DEG_TO_RAD;
/// The strafe's speed on the ground and in the air, cm a second at a full axis.
pub const STRAFE_GROUND: Real = 550.0;
pub const STRAFE_AIR: Real = 300.0;
/// The most two depths may differ for a hit, cm.
pub const DEPTH_REACH: Real = 110.0;
/// The floor's half depth: |d| is held inside it (an invisible wall), cm.
pub const HALF_DEPTH: Real = 700.0;
/// The plane does not turn while the two fighters are closer than this on the floor, cm (their line is no line).
const MIN_LINE: Real = 1.0;

/// A projectile's depth, its owner's at its spawn.
#[derive(State, Copy, PartialEq, Debug)]
pub struct ShotDepth {
    pub pr: ProjectileId,
    pub d: Real,
}

/// The test's state: the plane's angle and the inputs of the tick.
#[derive(State, Debug, Default)]
pub struct Depth3d {
    /// The plane's angle about the vertical, radians in (-PI, PI].
    pub theta: Real,
    /// `theta` at the start of the last step (the view's interpolation).
    pub prev_theta: Real,
    /// O's sim x: the middle of the stage's ground.
    pub origin: Real,
    /// The tick's strafe axis per player (the place in `World::players`), -1 .. 1: positive moves d up (toward the
    /// camera). Written by the caller ahead of each step.
    pub strafe: Vec<Real>,
    /// The live projectiles' depths.
    pub shots: Vec<ShotDepth>,
}

/// Switches the test on for `w` (again after a reset: the plane back to the 2D one, every depth 0).
pub fn enable(w: &mut World) {
    let g = w.stage.ground;
    let origin = narrow((g.left + g.right) / 2.0);
    let n = w.players.len();
    w.depth3d = Some(Depth3d { origin, strafe: vec![0.0; n], ..Depth3d::default() });
    for f in w.players.clone() {
        w.fighter_mut(f).body.d = 0.0;
    }
}

/// The step's part: the strafes, the floor's wall, the lock-on's turn and the projection onto the turned plane.
pub fn tick(w: &mut World) {
    let Some(mut s) = w.depth3d.take() else { return };
    s.prev_theta = s.theta;
    // (a projectile spawned in the last step keeps its owner's depth: no depth has changed since)
    s.shots.retain(|k| w.actors.projectiles.get(k.pr).is_some_and(|p| !p.dead));
    for (pr, p) in w.actors.projectiles.iter() {
        if !p.dead && !s.shots.iter().any(|k| k.pr == pr) {
            let d = p.owner.and_then(|o| w.fighters.get(o)).map_or(0.0, |o| o.body.d);
            s.shots.push(ShotDepth { pr, d });
        }
    }
    s.strafe.resize(w.players.len(), 0.0);
    let closed = w.scratch.closed;
    for (i, &f) in w.players.clone().iter().enumerate() {
        let fi = w.fighter_mut(f);
        let axis = if closed { 0.0 } else { max(-1.0, min(1.0, s.strafe[i])) };
        // (a fighter whose attack has its hit boxes out is committed: no strafe)
        if axis != 0.0 && fi.montages.active_hits.is_empty() {
            let speed = if fi.body.on_floor { STRAFE_GROUND } else { STRAFE_AIR };
            fi.body.d += axis * speed * crate::consts::DELTA_SECONDS;
        }
        fi.body.d = max(-HALF_DEPTH, min(HALF_DEPTH, fi.body.d));
    }
    if let [a, b, ..] = w.players[..] {
        let step = lock_on_turn(&s, w.fighter(a).body.x, w.fighter(a).body.d, w.fighter(b).body.x, w.fighter(b).body.d);
        if step != 0.0 {
            let (sin, cos) = (ue::sinf(step), ue::cosf(step));
            for f in w.players.clone() {
                let body = &mut w.fighter_mut(f).body;
                let (x, d) = turned(body.x - s.origin, body.d, sin, cos);
                let moved = x + s.origin - body.x;
                body.x += moved;
                body.prev_x += moved;
                body.d = d;
            }
            s.theta = wrap(s.theta + step);
        }
    }
    w.depth3d = Some(s);
}

/// A plane point (`x` along it from O, `d` off it) on the plane turned by an angle of sine `sin` and cosine `cos`:
/// the same world point's place on the turned plane.
pub fn turned(x: Real, d: Real, sin: Real, cos: Real) -> (Real, Real) {
    (x * cos + d * sin, d * cos - x * sin)
}

/// The world floor point (a, b) of a plane point `x` (sim x), `d` on the plane at `theta` through O at `origin`.
pub fn floor_point(theta: Real, origin: Real, x: Real, d: Real) -> (Real, Real) {
    let (sin, cos) = (ue::sinf(theta), ue::cosf(theta));
    let s = x - origin;
    (origin + s * cos - d * sin, s * sin + d * cos)
}

/// The turn of this tick toward the line through the two fighters' floor points (either way along it: the nearer),
/// at most [`TURN_RATE`] a tick.
fn lock_on_turn(s: &Depth3d, xa: Real, da: Real, xb: Real, db: Real) -> Real {
    let pa = floor_point(s.theta, s.origin, xa, da);
    let pb = floor_point(s.theta, s.origin, xb, db);
    let (ea, eb) = (pb.0 - pa.0, pb.1 - pa.1);
    if (ea * ea + eb * eb).sqrt() < MIN_LINE {
        return 0.0;
    }
    let mut delta = wrap(ue::atan2(eb, ea) - s.theta);
    // (the line, not the arrow: within a quarter turn either way)
    let half = std::f32::consts::FRAC_PI_2;
    if delta > half {
        delta -= std::f32::consts::PI;
    } else if delta < -half {
        delta += std::f32::consts::PI;
    }
    let cap = TURN_RATE * crate::consts::DELTA_SECONDS;
    max(-cap, min(cap, delta))
}

/// An angle into (-PI, PI].
fn wrap(a: Real) -> Real {
    let tau = std::f32::consts::TAU;
    let mut a = a;
    while a > std::f32::consts::PI {
        a -= tau;
    }
    while a <= -std::f32::consts::PI {
        a += tau;
    }
    a
}

/// Whether `att`'s hits reach `vic` in depth: always with the test off.
pub fn reach(w: &World, att: FighterId, vic: FighterId) -> bool {
    w.depth3d.is_none() || (w.fighter(att).body.d - w.fighter(vic).body.d).abs() < DEPTH_REACH
}

/// Whether projectile `pr`'s hits reach `vic` in depth: always with the test off. One spawned in this step has its
/// owner's depth (no depth changes inside a step).
pub fn shot_reach(w: &World, pr: ProjectileId, vic: FighterId) -> bool {
    let Some(s) = &w.depth3d else { return true };
    let d = match s.shots.iter().find(|k| k.pr == pr) {
        Some(k) => k.d,
        None => {
            let owner = w.actors.projectiles.get(pr).and_then(|p| p.owner).and_then(|o| w.fighters.get(o));
            owner.map_or(w.fighter(vic).body.d, |o| o.body.d)
        }
    };
    (d - w.fighter(vic).body.d).abs() < DEPTH_REACH
}

/// A projectile's depth for the view; None with the test off.
pub fn shot_depth(w: &World, pr: ProjectileId) -> Option<Real> {
    let s = w.depth3d.as_ref()?;
    Some(s.shots.iter().find(|k| k.pr == pr).map_or_else(
        || {
            let owner = w.actors.projectiles.get(pr).and_then(|p| p.owner).and_then(|o| w.fighters.get(o));
            owner.map_or(0.0, |o| o.body.d)
        },
        |k| k.d,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_keeps_the_world_point() {
        let (origin, x, d) = (100.0, 600.0, -150.0);
        let theta = 0.3;
        let p = floor_point(theta, origin, x, d);
        let step: Real = 0.25;
        let (x2, d2) = turned(x - origin, d, ue::sinf(step), ue::cosf(step));
        let q = floor_point(theta + step, origin, x2 + origin, d2);
        assert!((p.0 - q.0).abs() < 1e-3 && (p.1 - q.1).abs() < 1e-3, "{p:?} {q:?}");
    }

    #[test]
    fn the_plane_turns_toward_the_line_either_way_and_at_most_the_cap() {
        let s = Depth3d::default();
        // b off the line by its depth: a turn toward it, capped
        let t = lock_on_turn(&s, 0.0, 0.0, 300.0, 300.0);
        assert!((t - TURN_RATE * crate::consts::DELTA_SECONDS).abs() < 1e-6);
        // b behind a: the line's other way, the same small turn
        let t = lock_on_turn(&s, 0.0, 0.0, -300.0, 3.0);
        assert!(t < 0.0 && t > -0.02, "{t}");
        // in line: none
        assert!(lock_on_turn(&s, 0.0, 50.0, 300.0, 50.0).abs() < 1e-6);
    }
}
