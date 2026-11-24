//! The stages with no sim of their own (`stages/sky-arena`, `stages/training-room-small`, `stages/classic-3-platform`,
//! `stages/training-room-large`, `stages/trophys-edge`): an empty entry each, so the World knows the level. Their
//! platforms, moving platforms (BasePlatformBP), solids, starts and blast box are the stage data's, run by the World.

use crate::registry::entries::StageEntry;
use crate::registry::{Registry, RegistryError};

/// The levels entered with an empty entry, in registration order.
const LEVELS: [&str; 10] = [
    // sky-arena: its four gameplay levels
    "Map_M011_LargeNoPlat",
    "Map_M011_SmallNoPlat",
    "Map_M011",
    "Map_M011_Small",
    // training-room-small
    "Map_TrainingRoomSmall",
    // classic-3-platform
    "Map_Classic3Platform_Small",
    "Map_Classic3Platform_Large",
    // training-room-large
    "Map_TrainingRoomLarge",
    // trophys-edge
    "Map_TrophyRoom_LargePlatform",
    "Map_TrophyRoom_2platforms",
];

/// A custom stage's name starts so (`Custom_<key>`, the port's own: `mvs-stage`): each is entered with an empty
/// entry, its platforms, moving platforms, solids, starts and blast box its composed stage data's.
pub const CUSTOM_PREFIX: &str = "Custom_";

/// The entry of a custom stage's name: the empty one.
pub fn custom_entry(name: &str) -> Option<StageEntry> {
    name.starts_with(CUSTOM_PREFIX).then(StageEntry::default)
}

pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    for name in LEVELS {
        r.stages.enter(name, StageEntry::default())?;
    }
    Ok(())
}
