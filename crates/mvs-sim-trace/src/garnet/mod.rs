//! Garnet's state, actors and buff extensions as the record writes them (`chars/garnet/`): `state.rs` her fighter state
//! (`GarnetRecord`), a file per port file with its classes' `put` and `load`. `fighter.rs`, `actors.rs` and `parts.rs`
//! call them.

pub mod gauntlets;
pub mod groove;
pub mod marker;
pub mod shockwave;
pub mod sing;
pub mod state;
pub mod stop_projectile;
pub mod zone;

pub use state::{garnet_load, garnet_put};
