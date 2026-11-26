//! (the port's own) Flood's swim pose: a fighter in the water and off the floor bobs on it and leans into the way it
//! swims. The view's alone: the sim's body is not moved.

use mvs_sim::World;
use mvs_sim::fighter::FighterId;
use mvs_sim::world::own_rules::{submerged, water};

/// How deep (`submerged`, 0..1) a fighter is before it floats.
const FLOATS: f32 = 0.35;
/// The lean at full swim (radians) and the sideways speed it takes (cm/s).
const LEAN: f64 = 0.26;
const LEAN_SPEED: f64 = 600.0;
/// The bob's height (cm).
const BOB: f64 = 4.5;
/// How fast the pose eases in and out, and the lean follows, a tick.
const EASE: f64 = 0.08;
const FOLLOW: f64 = 0.1;

/// A view's swim pose.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Swim {
    /// How far in the pose (0..1).
    weight: f64,
    /// The lean (radians about the view's depth; above 0 tips the top toward sim -x), before the weight.
    lean: f64,
}

impl Swim {
    /// One tick of fighter `f` (the `k`th view: its bob's phase): eases toward its pose, returns the bob (cm up).
    pub fn tick(&mut self, w: &World, f: FighterId, k: usize) -> f64 {
        let fi = w.fighter(f);
        let swims = water(w).is_some_and(|wa| submerged(w, f, &wa) > FLOATS)
            && !fi.body.on_floor
            && fi.life.dead == 0.0
            && fi.hit.hitpause <= 0.0
            && fi.hit.launch.is_none();
        let target = if swims { 1.0 } else { 0.0 };
        self.weight += (target - self.weight) * EASE;
        if self.weight < 1e-3 {
            *self = Swim::default();
            return 0.0;
        }
        let lean = if swims { -(f64::from(fi.body.vx) / LEAN_SPEED).clamp(-1.0, 1.0) * LEAN } else { 0.0 };
        self.lean += (lean - self.lean) * FOLLOW;
        // Two swells of unrelated periods, each fighter on its own phase, so no two bob together.
        #[allow(clippy::cast_precision_loss)]
        let (t, ph) = (f64::from(w.flow.tick) * f64::from(mvs_sim::consts::DT), k as f64 * 1.9);
        self.weight * BOB * (0.65 * (t * 2.1 + ph).sin() + 0.35 * (t * 3.37 + ph * 2.0).sin())
    }

    /// The lean drawn (radians about the view's depth axis).
    pub fn lean(&self) -> f64 {
        self.lean * self.weight
    }
}
