//! (the port's own) Flood's and Storm Flood's sound: a cold splash as the water starts to rise and its loop (a stream,
//! or a waterfall for the storm sea) while it is up, from the surface under the listener. The strokes' and splashes'
//! sounds are the sim's (plain sound events on the fighters, named in `SIM_SOUNDS`).

use mvs_audio::tape::post::Call;
use mvs_sim::World;
use mvs_sim::world::own_rules::{
    Rule, WATER_SPLASH_BIG_SOUND, WATER_SPLASH_SOUND, WATER_STROKE_SOUND, WaterStyle, water,
};

use super::Calls;

/// The water's game object.
const AK_WATER: i32 = 5003;
// Unverified: the beta has no such rule and so no sound for it; the rise's cold-water ring-out sound and the two
// loops (a map's stream, the trophy room's waterfall) are picked by their name.
const RISE: &str = "sfx_blastbox_coldwater_enemy";
const STREAM: &str = "amb_map_shared_emit_water_flow_small";
const FALLS: &str = "amb_map_trophyroom_emit_waterfall";
const KEY: &str = "flood";
/// The loop's fade out as the water leaves (seconds).
const FADE: f64 = 1.0;
/// The sounds the sim posts for a stroke and a splash (by name, not in the base scope).
const SIM_SOUNDS: [&str; 3] = [WATER_SPLASH_BIG_SOUND, WATER_SPLASH_SOUND, WATER_STROKE_SOUND];

/// Flood's sounds: the scope a match where it can rise loads (not the base scope's).
pub(super) const SOUNDS: [&str; 6] = [RISE, STREAM, FALLS, SIM_SOUNDS[0], SIM_SOUNDS[1], SIM_SOUNDS[2]];

/// The events a match where a flood can rise loads (none in another).
pub(super) fn scope(world: &World) -> Vec<String> {
    if super::may_have(world, Rule::Flood) || super::may_have(world, Rule::StormFlood) {
        SOUNDS.map(str::to_owned).to_vec()
    } else {
        Vec::new()
    }
}

/// The water as heard: the surface's height (UE Z) while it is up.
#[derive(Debug, Default)]
pub(super) struct Flood {
    level: Option<f64>,
}

impl Flood {
    /// The tick's water: the splash and the loop as it starts to rise, the loop stopped as it is gone.
    pub(super) fn tick(&mut self, world: &World, c: &mut Calls) {
        let w = water(world);
        let level = w.map(|w| f64::from(w.level));
        match (self.level, w) {
            (None, Some(w)) => {
                c.post(RISE, Some(AK_WATER), None, None, None);
                let ev = if w.style == WaterStyle::Storm { FALLS } else { STREAM };
                c.post(ev, Some(AK_WATER), Some(KEY.to_owned()), None, None);
            }
            (Some(_), None) => c.push(Call::StopKey { key: KEY.to_owned(), fade: FADE, curve: None }),
            _ => {}
        }
        self.level = level;
    }

    /// The frame's listener `pos`: the loop comes from the surface under (or over) it, as the lava's roar does.
    pub(super) fn place(&self, pos: [f64; 3], c: &mut Calls) {
        if let Some(l) = self.level {
            c.emitter(AK_WATER, [pos[0], pos[1], l], [1.0, 0.0, 0.0]);
        }
    }
}
