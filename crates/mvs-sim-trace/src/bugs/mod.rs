//! Bugs Bunny's state, actors and buff extensions as the record writes them (`chars/bugs/`): `state.rs` his fighter
//! state (`BugsRecord`), a file per port file with its classes' `put` and `load`. `fighter.rs`, `actors.rs` and
//! `parts.rs` call them.

pub mod charm;
pub mod dispenser;
pub mod perks;
pub mod rockets;
pub mod safe;
pub mod state;
pub mod tunnel;

pub use state::{bugs_load, bugs_put};
