//! The stages' own sim (`stages/`): each gameplay level's entry (`StageEntry`, by its map name), the classes of its
//! placed actors (`StageActorEntry`), the entry's own state (a [`StageOwn`] variant) and each class's own state (a
//! [`StageActorOwn`] variant). The shared parts are the World's: the run, its Delays and timers, the solids and the
//! moving pieces (`world::stage`), the placed actors' hit path (`world::stage_actors`), the destructible walls
//! (`world::wall_bp`).
//!
//! [`StageOwn`]: crate::world::stage::StageOwn
//! [`StageActorOwn`]: crate::world::stage_actors::StageActorOwn

pub mod batcave;
pub mod cromulons;
pub mod scoobys_haunted_mansion;
pub mod statics;
pub mod the_court;
pub mod throne_room;
pub mod tree_fort;

use crate::registry::{Registry, RegistryError};

/// Every stage's entries, in registration order.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    statics::register(r)?;
    tree_fort::register(r)?;
    cromulons::register(r)?;
    scoobys_haunted_mansion::register(r)?;
    batcave::register(r)?;
    throne_room::register(r)?;
    the_court::register(r)
}
