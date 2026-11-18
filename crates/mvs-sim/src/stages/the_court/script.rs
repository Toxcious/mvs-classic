//! The Court's level script (Map_M010_C).
//! BeginPlay's authority branch reads the match's AllowHazards (the success result is ignored): true asks
//! SpawnBall, then PowerupSpawner (@37-41), each a Delay(ObjectSpawnDelay 24) to its spawn; false asks neither. The
//! rest of BeginPlay (InToCountdown, EnvironmentGameplaySettings.DepthOffset 0) is the drawing's. Its chains: the
//! ball's (`ball`: spawn at (0, 0, 1500), OnDestroyed -> Delay(5) -> spawn) and the powerup's (`pickups`: a random
//! class of PowerupList at (0, 0, 900), OnDestroyed -> Delay(30) -> spawn). The level script never ticks: its Delays
//! run in the leftover pass. Nothing resets: only a new World restarts the chains.
//! The owner and event numbers are in `ids`.

use super::ball::{self, CourtBall};
use super::pickups::{self, CourtPowerup};
use crate::State;
use crate::world::World;

/// `CourtScript`: the level script's state.
#[derive(State, Debug, Default)]
pub struct CourtScript {
    pub ball: CourtBall,
    pub powerup: CourtPowerup,
}

/// `CourtScript` as `scriptCreate` makes it (`{ ball: { item: -1 }, powerup: { actor: -1 } }`).
pub fn new() -> CourtScript {
    CourtScript { ball: CourtBall { item: -1 }, powerup: CourtPowerup { actor: -1, ..CourtPowerup::default() } }
}

/// `scriptCreate`'s chains, once the stage's own state holds [`new`]: AllowHazards starts both.
pub fn begin(w: &mut World) {
    if w.setup.allow_hazards {
        ball::begin(w);
        pickups::begin(w);
    }
}
