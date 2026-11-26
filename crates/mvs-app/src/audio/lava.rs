//! (the port's own) Rising Lava's sound: the lava's roar while it is up. Its touch's sound is the sim's
//! (`own_rules::LAVA_HIT_SOUND`, a plain sound event on the fighter it bounces).

use mvs_audio::tape::post::Call;
use mvs_sim::World;
use mvs_sim::world::own_rules::{LAVA_HIT_SOUND, lava_level};

use super::Calls;

/// The lava's game object.
const AK_LAVA: i32 = 5000;
/// The roar, looped while the lava is over the blast box's bottom.
// Unverified: the beta has no such rule and so no sound for it; the shared firewall's ground loop is picked by its
// name.
const LOOP: &str = "sfx_shared_class_mage_firewall_ground_loop";
const KEY: &str = "rising lava";
/// The roar's fade out as the lava leaves (seconds).
const FADE: f64 = 1.0;
/// The mutator's key.
const RULE: &str = "Chaos_LavaFloor";

/// The events a match where the lava can rise loads (none in another).
pub(super) fn scope(world: &World) -> Vec<String> {
    let on = world.setup.mutators.iter().any(|m| m == RULE)
        || world.setup.wheel.as_ref().is_some_and(|w| w.pool.iter().any(|m| m == RULE));
    if on { vec![LOOP.to_owned(), LAVA_HIT_SOUND.to_owned()] } else { Vec::new() }
}

/// The roar's state: the surface's height (UE Z) while it plays.
#[derive(Debug, Default)]
pub(super) struct Roar {
    level: Option<f64>,
}

impl Roar {
    /// The tick's lava: the roar started as the lava rises over the blast box's bottom, stopped as it leaves.
    pub(super) fn tick(&mut self, world: &World, c: &mut Calls) {
        let bottom = world.stage.blast.bottom + 1.0;
        let level = lava_level(world).filter(|&l| l > bottom).map(f64::from);
        match (self.level, level) {
            (None, Some(l)) => {
                c.emitter(AK_LAVA, [0.0, 0.0, l], [1.0, 0.0, 0.0]);
                c.post(LOOP, Some(AK_LAVA), Some(KEY.to_owned()), None, None);
            }
            (Some(_), None) => c.push(Call::StopKey { key: KEY.to_owned(), fade: FADE, curve: None }),
            _ => {}
        }
        self.level = level;
    }

    /// The frame's listener `pos`: the roar comes from the surface right under (or over) it, so it grows as the
    /// lava nears the camera and does not pan across the stage's width.
    pub(super) fn place(&self, pos: [f64; 3], c: &mut Calls) {
        if let Some(l) = self.level {
            c.emitter(AK_LAVA, [pos[0], pos[1], l], [1.0, 0.0, 0.0]);
        }
    }
}
