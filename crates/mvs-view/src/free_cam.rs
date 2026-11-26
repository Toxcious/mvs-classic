//! (the port's own) A developer free camera: an orbit about a pivot that replaces the match camera's view after the
//! rig has placed it (the rig keeps running underneath).
//!
//! The orbit is the game's view turned: the pivot is the point on the stage's plane (UE X = 0) the game's lens looks
//! at, the distance the lens's from it. `yaw` turns the lens's heading about UE Z, `pitch` raises the lens (degrees,
//! positive looks down more), `zoom` scales the distance, `pan` moves the pivot. All zero and zoom 1 give the game's
//! camera. The pivot follows the rig's view each frame, or stays where it was when the follow was switched off.

use crate::camera::{Pov, look_at};
use crate::frame::Camera;

/// The farthest the lens goes from the pivot, cm: inside the projection's far plane.
const MAX_DIST: f64 = 80_000.0;
/// The distance taken where the game's view does not look at the stage's plane, cm.
const FALLBACK_DIST: f64 = 7000.0;
const MAX_ELEVATION: f64 = 89.0;

fn rad(d: f64) -> f64 {
    d * std::f64::consts::PI / 180.0
}

fn norm(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    v.map(|c| c / l)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The orbit's state.
#[derive(Clone, Debug, PartialEq)]
pub struct FreeCam {
    /// Degrees added to the game's heading about UE Z.
    pub yaw: f64,
    /// Degrees the lens is raised over the game's view (positive looks down more).
    pub pitch: f64,
    /// The distance from the pivot as a factor of the game's.
    pub zoom: f64,
    /// The pivot's move from the game's, UE cm.
    pub pan: [f64; 3],
    /// The pivot tracks the game's view; false: it stays where `follow(false)` found it.
    following: bool,
    held: Option<([f64; 3], f64)>,
    start: [f64; 3],
}

impl FreeCam {
    /// An orbit that starts `yaw`, `pitch` degrees and `zoom` times the distance from the game's view.
    pub fn new(yaw: f64, pitch: f64, zoom: f64) -> FreeCam {
        FreeCam {
            yaw,
            pitch,
            zoom: zoom.clamp(0.02, 40.0),
            pan: [0.0; 3],
            following: true,
            held: None,
            start: [yaw, pitch, zoom],
        }
    }

    /// Back to the game's view (the start orbit's when the run gave one is not kept: 5 on the keypad is the game's).
    pub fn reset(&mut self) {
        *self = FreeCam::new(0.0, 0.0, 1.0);
    }

    /// Back to the orbit the flag gave.
    pub fn restart(&mut self) {
        let [y, p, z] = self.start;
        *self = FreeCam::new(y, p, z);
    }

    pub fn orbit(&mut self, dyaw: f64, dpitch: f64) {
        self.yaw = (self.yaw + dyaw) % 360.0;
        self.pitch = (self.pitch + dpitch).clamp(-170.0, 170.0);
    }

    /// `notches` of the wheel (up positive): each takes a tenth off the distance.
    pub fn zoom_by(&mut self, notches: f64) {
        self.zoom = (self.zoom * 0.9f64.powf(notches)).clamp(0.02, 40.0);
    }

    /// Moves the pivot `right` and `up` of `pov`'s view, as fractions of the distance to it.
    pub fn pan_by(&mut self, pov: &Pov, right: f64, up: f64) {
        let d = self.dist(pov);
        for k in 0..3 {
            self.pan[k] += (pov.right[k] * right + pov.up[k] * up) * d;
        }
    }

    /// Whether the pivot follows the game's view.
    pub fn following(&self) -> bool {
        self.following
    }

    /// Switches following: off holds the pivot (and its distance) where the game's view has it now.
    pub fn toggle_follow(&mut self, pov: &Pov) {
        if self.following {
            self.held = Some(pivot_of(pov));
        } else {
            self.held = None;
        }
        self.following = !self.following;
    }

    fn dist(&self, pov: &Pov) -> f64 {
        let d = self.held.map_or_else(|| pivot_of(pov).1, |h| h.1);
        (d * self.zoom).min(MAX_DIST)
    }

    /// The free view of the game's `pov`.
    pub fn view(&self, pov: &Pov) -> Pov {
        let (pivot, d0) = self.held.unwrap_or_else(|| pivot_of(pov));
        let pivot = [0, 1, 2].map(|k| pivot[k] + self.pan[k]);
        let d = (d0 * self.zoom).min(MAX_DIST);
        let f = pov.front;
        let heading = f[1].atan2(f[0]) + rad(self.yaw);
        let elevation = (f[2].clamp(-1.0, 1.0).asin() - rad(self.pitch)).clamp(rad(-MAX_ELEVATION), rad(MAX_ELEVATION));
        let front = [elevation.cos() * heading.cos(), elevation.cos() * heading.sin(), elevation.sin()];
        let right = norm(cross([0.0, 0.0, 1.0], front));
        let up = cross(front, right);
        let pos = [0, 1, 2].map(|k| pivot[k] - front[k] * d);
        Pov { pos, right, up, front }
    }

    /// The drawn camera of the game's `pov` and `game` (its projection): the free view's origin and rotation.
    pub fn camera(&self, pov: &Pov, game: &Camera) -> (Camera, Pov) {
        let p = self.view(pov);
        let target = [0, 1, 2].map(|k| p.pos[k] + p.front[k] * 1000.0);
        let cam =
            Camera { origin: p.pos, translated_world_to_view: look_at(p.pos, target), view_to_clip: game.view_to_clip };
        (cam, p)
    }
}

/// The point of the stage's plane (UE X = 0) the game's lens looks at and the lens's distance from it.
fn pivot_of(pov: &Pov) -> ([f64; 3], f64) {
    let f = pov.front;
    let d = if f[0] < -0.05 { -pov.pos[0] / f[0] } else { FALLBACK_DIST };
    let d = if d > 1.0 { d } else { FALLBACK_DIST };
    ([0, 1, 2].map(|k| pov.pos[k] + f[k] * d), d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The game's lens: 9000 out on UE X, 100 up, 5 degrees down.
    fn game_pov() -> Pov {
        let a = rad(5.0);
        Pov {
            pos: [9000.0, 120.0, 800.0],
            right: [0.0, -1.0, 0.0],
            up: [-a.sin(), 0.0, a.cos()],
            front: [-a.cos(), 0.0, -a.sin()],
        }
    }

    fn near(a: [f64; 3], b: [f64; 3]) {
        for k in 0..3 {
            assert!((a[k] - b[k]).abs() < 1e-6, "{a:?} {b:?}");
        }
    }

    #[test]
    fn the_default_orbit_is_the_games_camera() {
        let g = game_pov();
        let p = FreeCam::new(0.0, 0.0, 1.0).view(&g);
        near(p.pos, g.pos);
        near(p.front, g.front);
        near(p.right, g.right);
        near(p.up, g.up);
    }

    #[test]
    fn a_quarter_turn_looks_from_the_side_at_the_same_pivot() {
        let g = game_pov();
        let (pivot, d) = pivot_of(&g);
        let p = FreeCam::new(90.0, 0.0, 1.0).view(&g);
        assert!(p.front[0].abs() < 1e-9);
        let d2 = (0..3).map(|k| (p.pos[k] - pivot[k]).powi(2)).sum::<f64>().sqrt();
        assert!((d2 - d).abs() < 1e-6);
        let seen = [0, 1, 2].map(|k| p.pos[k] + p.front[k] * d);
        near(seen, pivot);
    }

    #[test]
    fn a_held_pivot_stays_while_the_game_moves() {
        let g = game_pov();
        let mut c = FreeCam::new(0.0, 0.0, 1.0);
        c.toggle_follow(&g);
        let mut moved = g;
        moved.pos[1] += 500.0;
        near(c.view(&moved).pos, g.pos);
    }
}
