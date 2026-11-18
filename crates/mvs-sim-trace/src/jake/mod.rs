//! Jake's state, actors, buff extensions and hold variables as the record writes them (`chars/jake/`): `state.rs` the
//! fighter state (`JakeRecord`), a file per port file with its classes' `put` and `load`. `fighter.rs`, `actors.rs`,
//! `parts.rs` and `holds.rs` call them.

pub mod aim;
pub mod ally_spit;
pub mod eat_buffs;
pub mod horse;
pub mod house;
pub mod perks;
pub mod spit;
pub mod state;
pub mod stretch;
pub mod weapons;

pub use state::{jake_load, jake_put, jake_refs_load};
