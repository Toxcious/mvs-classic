//! Marvin's state, actors and buff extensions as the record writes them (`chars/marvin/`): `state.rs` the fighter
//! state (`MarvinRecord`), a file per port file with its classes' `put` and `load`. `fighter.rs`, `actors.rs` and
//! `parts.rs` call them.

pub mod blaster;
pub mod bubble;
pub mod flag;
pub mod homing;
pub mod perks;
pub mod ray;
pub mod reverse;
pub mod state;
pub mod ufo;

pub use state::{marvin_load, marvin_put};
