//! (the port's own) The Chaos rule Wind as the match camera feels it: while a gust (`own_rules::gust_at`) blows, the
//! lens is pushed downwind and turned a little upwind, so the downwind side of the picture swings in nearer, and it
//! sways with the gust's buffeting; it leans faintly through the warning and eases back after. Nothing moves in a
//! match without Wind.
//!
//! It is stepped once a tick beside the rig and read between two ticks as the rig's own place is, on top of the rig's
//! eased and blast-box-clamped centre and its shakes.

use mvs_sim::World;
use mvs_sim::world::own_rules::{Gust, WIND_WARN, gust_at};

/// How far the lens is pushed downwind at a gust's full blow (UE units, along the picture's width).
const PAN: f64 = 520.0;
/// How far it turns upwind at full blow (degrees about UE Z): the downwind side comes nearer.
const YAW: f64 = 3.0;
/// The share of the full lean reached by the warning's end, eased in over the warning.
const WARN_LEAN: f64 = 0.15;
/// The share of the way to its aim the lean covers each tick (an exponential ease: about a quarter second).
const EASE: f64 = 0.07;
/// The buffeting at full blow: its push (UE units) and its turn (degrees), each two waves of the frequencies below.
const SWAY_PAN: f64 = 45.0;
const SWAY_YAW: f64 = 0.25;
/// The buffeting's two frequencies (cycles a second).
const SWAY_SLOW: f64 = 0.7;
const SWAY_FAST: f64 = 1.9;
/// The rig's tick.
const TICK: f64 = 1.0 / 60.0;

/// The camera's lean: its push along UE Y and its turn about UE Z (degrees, positive turns the view toward UE -Y).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lean {
    pub pan: f64,
    pub yaw: f64,
}

/// The wind's hold on the camera, stepped per tick.
#[derive(Clone, Debug, Default)]
pub struct WindCam {
    /// The eased strength (0..1, signed by the gust's way in sim x) and the buffeting's.
    lean: f64,
    sway: f64,
    /// The buffeting's clock (seconds), running while anything leans.
    clock: f64,
    /// The lean of the last tick and the latest.
    prev: Lean,
    now: Lean,
}

/// How hard gust `g` leans the camera (0..1): `WARN_LEAN` eased in through the warning, the blow's own ramp after.
fn aim(g: Option<Gust>) -> (f64, f64) {
    let Some(g) = g else { return (0.0, 0.0) };
    let (dir, age, blow) = (f64::from(g.dir), f64::from(g.age), f64::from(g.blow));
    let smooth = |t: f64| t * t * (3.0 - 2.0 * t);
    let blow = smooth(blow.clamp(0.0, 1.0));
    let lean =
        if age < f64::from(WIND_WARN) { WARN_LEAN * smooth(age / f64::from(WIND_WARN)) } else { WARN_LEAN.max(blow) };
    (dir * lean, blow)
}

impl WindCam {
    /// One tick, after the World's step.
    pub fn step(&mut self, w: &World) {
        let (lean, sway) = aim(gust_at(w, 0.0));
        self.lean += (lean - self.lean) * EASE;
        self.sway += (sway - self.sway) * EASE;
        if self.lean.abs() < 1e-4 && lean == 0.0 {
            self.lean = 0.0;
        }
        if self.sway < 1e-4 && sway == 0.0 {
            self.sway = 0.0;
        }
        self.clock = if self.lean == 0.0 && self.sway == 0.0 { 0.0 } else { self.clock + TICK };
        let tau = std::f64::consts::TAU;
        let wave = 0.6 * (tau * SWAY_SLOW * self.clock).sin() + 0.4 * (tau * SWAY_FAST * self.clock + 1.3).sin();
        self.prev = self.now;
        // sim x is UE -Y: downwind (sim x `dir`) is UE Y -dir; the view turns toward UE -Y (sim +x) with a positive yaw,
        // so turning upwind is -dir.
        self.now = Lean {
            pan: -(self.lean * PAN) - self.lean.signum() * self.sway * SWAY_PAN * wave,
            yaw: -(self.lean * YAW) - self.lean.signum() * self.sway * SWAY_YAW * wave,
        };
    }

    /// The lean at `a` between the last tick (0) and the latest (1).
    pub fn at(&self, a: f64) -> Lean {
        Lean {
            pan: self.prev.pan + (self.now.pan - self.prev.pan) * a,
            yaw: self.prev.yaw + (self.now.yaw - self.prev.yaw) * a,
        }
    }

    /// The Lab's reset: the camera stands straight at once.
    pub fn reset(&mut self) {
        *self = WindCam::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gust(dir: f32, age: f32, blow: f32) -> Option<Gust> {
        Some(Gust { dir, age, blow })
    }

    /// No wind, no lean; the warning leans at most `WARN_LEAN`; a full blow leans all the way, the gust's way.
    #[test]
    fn the_lean_follows_the_gust() {
        assert_eq!(aim(None), (0.0, 0.0));
        let warn = aim(gust(1.0, 45.0, 0.0));
        assert!(warn.0 > 0.0 && warn.0 <= WARN_LEAN && warn.1 == 0.0);
        assert_eq!(aim(gust(-1.0, 45.0, 0.0)).0, -warn.0);
        assert_eq!(aim(gust(-1.0, 150.0, 1.0)), (-1.0, 1.0));
    }
}
