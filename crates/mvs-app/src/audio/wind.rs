//! (the port's own) Wind's sound: a howl as each gust's warning starts, a second as the gust blows, both faded out
//! at its end, from the upwind side.

use mvs_audio::tape::post::Call;
use mvs_sim::World;
use mvs_sim::world::own_rules::{Rule, WIND_WARN, gust_at};

use super::Calls;

/// Wind's game object.
const AK_WIND: i32 = 5001;
// Unverified: the beta has no such rule and so no sound for it; the haunting-winds ring-out's two sounds are picked
// by their name.
const WARN: &str = "sfx_blastbox_hauntingwinds_ally";
const GUST: &str = "sfx_blastbox_hauntingwinds_enemy";
const KEY_WARN: &str = "wind warning";
const KEY_GUST: &str = "wind gust";
/// The fade out at a gust's end (seconds).
const FADE: f64 = 0.6;
/// How far upwind of the listener the sound is (cm), so it pans from the way the gust comes.
const UPWIND: f64 = 400.0;

/// Wind's sounds: the scope a match where it can blow loads.
pub(super) const SOUNDS: [&str; 2] = [WARN, GUST];

/// The events a match where Wind can blow loads (none in another).
pub(super) fn scope(world: &World) -> Vec<String> {
    if super::may_have(world, Rule::Wind) { SOUNDS.map(str::to_owned).to_vec() } else { Vec::new() }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Calm,
    Warn,
    Blow,
}

/// The gust's state as heard: its phase and way (sim x).
#[derive(Debug, Default)]
pub(super) struct Gusts {
    phase: Phase,
    dir: f64,
}

impl Gusts {
    /// The tick's gust: the warning's howl as it starts, the gust's as it blows, both faded out as it ends.
    pub(super) fn tick(&mut self, world: &World, c: &mut Calls) {
        let g = gust_at(world, 0.0);
        #[allow(clippy::cast_precision_loss)]
        let phase = match g {
            None => Phase::Calm,
            Some(g) if g.age < WIND_WARN as f32 => Phase::Warn,
            Some(_) => Phase::Blow,
        };
        if let Some(g) = g {
            self.dir = f64::from(g.dir);
        }
        match (self.phase, phase) {
            (Phase::Calm, Phase::Warn) => c.post(WARN, Some(AK_WIND), Some(KEY_WARN.to_owned()), None, None),
            (Phase::Calm | Phase::Warn, Phase::Blow) => {
                c.post(GUST, Some(AK_WIND), Some(KEY_GUST.to_owned()), None, None);
            }
            (Phase::Warn | Phase::Blow, Phase::Calm) => {
                for key in [KEY_WARN, KEY_GUST] {
                    c.push(Call::StopKey { key: key.to_owned(), fade: FADE, curve: None });
                }
            }
            _ => {}
        }
        self.phase = phase;
    }

    /// The frame's listener `pos`: the sound comes from the upwind side of it (sim x is UE -Y).
    pub(super) fn place(&self, pos: [f64; 3], c: &mut Calls) {
        if self.phase != Phase::Calm {
            c.emitter(AK_WIND, [pos[0], pos[1] + self.dir * UPWIND, pos[2]], [1.0, 0.0, 0.0]);
        }
    }
}
