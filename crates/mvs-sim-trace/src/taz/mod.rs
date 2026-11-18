//! Taz's state, actors and buff extensions as the record writes them (`chars/taz/`): `state.rs` his fighter state
//! (`TazRecord`), a file per port file with its classes' `put` and `load`. `fighter.rs`, `actors.rs` and `parts.rs`
//! call them.

pub mod chicken;
pub mod dogpile;
pub mod perks;
pub mod perks_cloud;
pub mod projectile_eat;
pub mod projectiles;
pub mod state;
pub mod tasty;

pub use state::{taz_load, taz_put, taz_refs_load};
