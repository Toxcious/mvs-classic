//! (the port's own) Black Hole's sound: a dark burst as it opens, its hum while open, a fall as it closes, from where
//! it is across the stage. Its core's throw is the sim's (`own_rules::HOLE_HIT_SOUND`, a plain sound event on the
//! fighter it throws).

use mvs_audio::tape::post::Call;
use mvs_sim::World;
use mvs_sim::world::own_rules::{HOLE_OPEN, Rule, hole_at};

use super::Calls;

/// The hole's game object.
const AK_HOLE: i32 = 5002;
// Unverified: the beta has no such rule and so no sound for it; the dark matter ring-out, the portal's loop and the
// support ray's stop are picked by their name.
const OPEN: &str = "sfx_blastbox_darkmatter_ally";
const HUM: &str = "sfx_cha_c019_portal_loop";
const CLOSE: &str = "sfx_shared_class_support_ray_stop";
const KEY: &str = "black hole";
/// The hum's fade out as it goes (seconds).
const FADE: f64 = 0.5;

/// The hole's sounds: the scope a match where it can open loads (not its core's, which is the base scope's).
pub(super) const SOUNDS: [&str; 3] = [OPEN, HUM, CLOSE];

/// The events a match where the hole can open loads (none in another).
pub(super) fn scope(world: &World) -> Vec<String> {
    if super::may_have(world, Rule::BlackHole) { SOUNDS.map(str::to_owned).to_vec() } else { Vec::new() }
}

/// The hole as heard: where it is (UE Y, Z) while open, and whether it has started to close.
#[derive(Debug, Default)]
pub(super) struct Hum {
    at: Option<[f64; 2]>,
    closing: bool,
}

impl Hum {
    /// The tick's hole: the burst and the hum as it opens, the fall as it starts to close, the hum stopped as it goes.
    pub(super) fn tick(&mut self, world: &World, c: &mut Calls) {
        let h = hole_at(world, 0.0).filter(|h| h.open > 0.0);
        // (sim x is UE -Y)
        let at = h.map(|h| [-f64::from(h.x), f64::from(h.y)]);
        #[allow(clippy::cast_precision_loss)]
        let closing = h.is_some_and(|h| h.open < 1.0 && h.age > HOLE_OPEN as f32);
        match (self.at, at) {
            (None, Some(p)) => {
                c.emitter(AK_HOLE, [0.0, p[0], p[1]], [1.0, 0.0, 0.0]);
                c.post(OPEN, Some(AK_HOLE), None, None, None);
                c.post(HUM, Some(AK_HOLE), Some(KEY.to_owned()), None, None);
            }
            (Some(_), None) => c.push(Call::StopKey { key: KEY.to_owned(), fade: FADE, curve: None }),
            _ => {}
        }
        if closing && !self.closing {
            c.post(CLOSE, Some(AK_HOLE), None, None, None);
        }
        self.at = at;
        self.closing = closing;
    }

    /// The frame's listener `pos`: the sound comes from the hole's place across the stage at the listener's depth,
    /// so it pans with the hole and does not fade with the camera's distance.
    pub(super) fn place(&self, pos: [f64; 3], c: &mut Calls) {
        if let Some(p) = self.at {
            c.emitter(AK_HOLE, [pos[0], p[0], p[1]], [1.0, 0.0, 0.0]);
        }
    }
}
